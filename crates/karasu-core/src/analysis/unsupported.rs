//! Unsupported regions of a print, found on its layer masks (TD-107, level 3).
//!
//! **Definition.** Layer `k` is supported where its material lies within the self-supporting
//! distance `d = Δz / tan(angle)` of the layer under it: a surface whose slope above level is
//! the critical angle stands out exactly `d` per layer. The rest of layer `k` is unsupported.
//! An unsupported part of a piece of layer `k` (a connected blob of its material) is:
//! - an **island** when the whole piece has nothing under it within `d`: it starts in mid-air;
//! - an **overhang** otherwise: the piece hangs out from material that holds it.
//!
//! The first layer rests on the plate. Unsupported parts of consecutive layers within reach of
//! each other are joined into regions in space. A region keeps where each of its islands starts
//! (owner, 2026-10-30): several pieces starting in mid-air at different layers may grow into
//! one region, and each needs its own support where it starts.
//!
//! **Pixels (owner, TD-107).** The analysis runs on the print's masks at the printer's
//! resolution: the exact contours' composition costs the square of a layer's edges, which a
//! sculpted model's thousands of edges per layer make impractical.
//! - A pixel of layer `k` is material when its gray reaches `material_gray`; the caller passes 1
//!   so a pixel touched at all counts, and no island smaller than a pixel is lost.
//! - A pixel of layer `k - 1` supports when its gray reaches `support_gray` (half coverage,
//!   as the wall verifier's material).
//! - Within reach means the pixels' centres lie at most `d` apart, measured exactly on the
//!   lattice with each axis' pitch.
//!
//! **Connectivity.** Pieces and regions join pixels touching by an edge or a corner
//! (8-neighbours): a diagonal contact is material printed together.
//!
//! **Cost.** Work and memory follow the masks' runs, not their pixels; only the rows a layer's
//! material occupies are examined. Two layers are held at a time. The report is the same for
//! equal inputs: layers are read in order, and regions are sorted.
use super::runs::{disc, reach_row, subtract, touch, Runs, Union};
use crate::{
    cancellation::{Cancellation, Cancelled, Checkpoint},
    raster::mask::SpanMask,
};

/// One layer of the print, from the plate up.
#[derive(Clone, Copy, Debug)]
pub struct SupportLayer<'a> {
    /// Height of the layer's plane, in mm.
    pub z_mm: f64,
    /// The printed mask.
    pub mask: &'a SpanMask,
}

/// What to analyse.
#[derive(Clone, Copy, Debug)]
pub struct SupportInput<'a> {
    /// The layers, with strictly increasing planes.
    pub layers: &'a [SupportLayer<'a>],
    /// Pixel pitch along the masks' columns and rows, in mm.
    pub pitch_mm: [f64; 2],
    /// The critical angle, in degrees from the plate's plane, strictly between 0 and 90.
    pub angle_deg: f64,
    /// Smallest gray of a material pixel, at least 1.
    pub material_gray: u8,
    /// Smallest gray of a supporting pixel, at least 1.
    pub support_gray: u8,
}

/// What an unsupported region is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegionKind {
    /// A piece starting in mid-air.
    Island,
    /// A piece hanging out from material that holds it.
    Overhang,
}

/// Where a piece starts in mid-air: an island's part at its first layer, the whole piece there.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IslandStart {
    /// Its layer.
    pub layer: usize,
    /// Its pixels.
    pub pixels: u64,
    /// The centroid of its pixels, in pixel coordinates (column, row; a pixel's centre is at
    /// half units).
    pub centroid: [f64; 2],
    /// Its bounds in pixels: first column and row, one past the last.
    pub bounds: [u32; 4],
}

/// An unsupported region: the unsupported pixels of consecutive layers within reach.
#[derive(Clone, Debug, PartialEq)]
pub struct UnsupportedRegion {
    /// Island or overhang (an island when any of its layers' parts starts in mid-air).
    pub kind: RegionKind,
    /// Its lowest layer.
    pub first_layer: usize,
    /// Its highest layer.
    pub last_layer: usize,
    /// Its pixels, over its layers.
    pub pixels: u64,
    /// The centroid of its lowest layer's pixels, in pixel coordinates (column, row; a
    /// pixel's centre is at half units).
    pub lowest: [f64; 2],
    /// Its bounds in pixels over its layers: first column and row, one past the last.
    pub bounds: [u32; 4],
    /// Where its islands start, by layer, then column, then row; none for an overhang.
    pub starts: Vec<IslandStart>,
}

/// The analysis's result.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SupportReport {
    /// The regions, by lowest layer, then column, then row.
    pub regions: Vec<UnsupportedRegion>,
}

impl SupportReport {
    /// How many regions are islands.
    #[must_use]
    pub fn islands(&self) -> usize {
        self.regions
            .iter()
            .filter(|r| r.kind == RegionKind::Island)
            .count()
    }
}

/// Why the analysis could not run.
#[derive(Clone, Debug, PartialEq)]
pub enum SupportError {
    /// The layers' masks differ in size, or their planes do not increase.
    InconsistentLayers,
    /// A parameter is out of range (the pitch, the angle or a gray).
    InvalidParameter(&'static str),
    /// More parts than `limit` were found.
    TooManyParts {
        /// The limit reached.
        limit: usize,
    },
    /// The work was cancelled.
    Cancelled(Cancelled),
}

impl std::fmt::Display for SupportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InconsistentLayers => {
                write!(
                    f,
                    "the layers differ in size or their planes do not increase"
                )
            }
            Self::InvalidParameter(what) => write!(f, "invalid {what}"),
            Self::TooManyParts { limit } => {
                write!(f, "more than {limit} unsupported parts were found")
            }
            Self::Cancelled(c) => write!(f, "{c}"),
        }
    }
}

impl std::error::Error for SupportError {}

impl From<Cancelled> for SupportError {
    fn from(c: Cancelled) -> Self {
        Self::Cancelled(c)
    }
}

/// Unsupported parts held at most: each costs a few dozen bytes.
pub const MAX_PARTS: usize = 50_000_000;

/// One layer's unsupported part: its runs (by row) and statistics.
#[derive(Clone, Debug)]
struct Part {
    layer: usize,
    island: bool,
    pixels: u64,
    sum: [f64; 2],
    bounds: [u32; 4],
}

/// A layer's unsupported runs by row, each with its part.
type RowParts = Vec<Vec<([u32; 2], usize)>>;

/// The pieces of a layer: its material runs joined across rows by edges and corners. Returns
/// the union and each run's entry in it, in run order.
fn pieces(material: &Runs) -> (Union, Vec<usize>) {
    let mut pieces = Union::default();
    let mut piece_of: Vec<usize> = Vec::with_capacity(material.runs.len());
    for y in 0..material.height() {
        for &run in material.row(y) {
            let id = pieces.add();
            piece_of.push(id);
            if y > 0 {
                let base = material.rows[y - 1];
                for (j, &above) in material.row(y - 1).iter().enumerate() {
                    if touch(run, above, 0) {
                        pieces.join(id, piece_of[base + j]);
                    }
                }
            }
        }
    }
    (pieces, piece_of)
}

/// The parts found so far and their regions.
#[derive(Default)]
struct Analysis {
    parts: Vec<Part>,
    regions: Union,
}

/// One layer's geometry for joining its parts: the reach's slack in rows and columns.
struct Reach {
    rows: usize,
    columns: u32,
}

impl Analysis {
    /// Add the unsupported `run` of row `y` of layer `layer` (an island's when `island`): to the
    /// part it touches in the row above, else to a new part, joined with the parts of the layer
    /// under it within `reach`.
    fn add(
        &mut self,
        (layer, y, run, island): (usize, usize, [u32; 2], bool),
        here: &mut RowParts,
        under: &RowParts,
        reach: &Reach,
    ) -> Result<(), SupportError> {
        let mut joined = None;
        if y > 0 {
            for &(other, part) in &here[y - 1] {
                if touch(run, other, 0) {
                    match joined {
                        None => joined = Some(part),
                        Some(first) => self.regions.join(first, part),
                    }
                }
            }
        }
        let part = if let Some(part) = joined {
            part
        } else {
            if self.parts.len() >= MAX_PARTS {
                return Err(SupportError::TooManyParts { limit: MAX_PARTS });
            }
            self.parts.push(Part {
                layer,
                island,
                pixels: 0,
                sum: [0.0; 2],
                bounds: [u32::MAX, u32::MAX, 0, 0],
            });
            self.regions.add()
        };
        let stats = &mut self.parts[part];
        let count = u64::from(run[1] - run[0]);
        stats.pixels += count;
        #[allow(clippy::cast_precision_loss)] // Pixel counts and coordinates.
        {
            let middle = f64::midpoint(f64::from(run[0]), f64::from(run[1]));
            stats.sum[0] += middle * count as f64;
            stats.sum[1] += (y as f64 + 0.5) * count as f64;
        }
        #[allow(clippy::cast_possible_truncation)] // Rows fit the mask's u32 height.
        let row = y as u32;
        stats.bounds = [
            stats.bounds[0].min(run[0]),
            stats.bounds[1].min(row),
            stats.bounds[2].max(run[1]),
            stats.bounds[3].max(row + 1),
        ];
        let rows = under
            .iter()
            .skip(y.saturating_sub(reach.rows))
            .take(y.min(reach.rows) + reach.rows + 1);
        for row in rows {
            for &(other, under_part) in row {
                if touch(run, other, reach.columns) {
                    self.regions.join(part, under_part);
                }
            }
        }
        here[y].push((run, part));
        Ok(())
    }
}

/// The unsupported runs of `material` (pieces `piece_of`) out of reach (`half`) of `under`, by
/// row, and for each piece whether any of it is in reach.
fn out_of_reach(
    material: &Runs,
    under: &Runs,
    half: &[u32],
    width: u32,
    (pieces, piece_of): (&mut Union, &[usize]),
) -> (Vec<(usize, [u32; 2])>, Vec<bool>) {
    let mut anchored = vec![false; piece_of.len()];
    let mut pending = Vec::new();
    let (mut reach, mut left) = (Vec::new(), Vec::new());
    for y in 0..material.height() {
        let row = material.row(y);
        if row.is_empty() {
            continue;
        }
        reach_row(under, y, half, width, &mut reach);
        for (i, &run) in row.iter().enumerate() {
            subtract(&[run], &reach, &mut left);
            let out: u32 = left.iter().map(|[a, b]| b - a).sum();
            if out < run[1] - run[0] {
                anchored[pieces.find(piece_of[material.rows[y] + i])] = true;
            }
            pending.extend(left.iter().map(|&u| (y, u)));
        }
    }
    (pending, anchored)
}

/// Find the unsupported regions of `input` (TD-107).
///
/// # Errors
/// [`SupportError`] for inconsistent layers or parameters, too many parts, or cancellation.
pub fn find_unsupported(
    input: &SupportInput<'_>,
    cancel: &mut Cancellation<'_>,
) -> Result<SupportReport, SupportError> {
    validate(input)?;
    let tan = input.angle_deg.to_radians().tan();
    let mut analysis = Analysis::default();
    // The layer under: its supporting runs, and its unsupported runs with their parts by row.
    let mut below: Option<(Runs, RowParts, f64)> = None;
    for (k, layer) in input.layers.iter().enumerate() {
        cancel.check(Checkpoint::UnsupportedLayer)?;
        let material = Runs::of(layer.mask, input.material_gray);
        let mut here: RowParts = vec![Vec::new(); material.height()];
        if let Some((under, under_parts, under_z)) = &below {
            let half = disc((layer.z_mm - under_z) / tan, input.pitch_mm);
            let reach = Reach {
                rows: half.len(),
                columns: half.first().copied().unwrap_or(0) + 1,
            };
            let (mut pieces, piece_of) = pieces(&material);
            let (pending, anchored) = out_of_reach(
                &material,
                under,
                &half,
                layer.mask.width(),
                (&mut pieces, &piece_of),
            );
            for (y, run) in pending {
                // The piece holding the run: an island's part when none of it is in reach.
                let piece = material
                    .row(y)
                    .iter()
                    .position(|m| m[0] <= run[0] && run[1] <= m[1])
                    .map_or(0, |i| pieces.find(piece_of[material.rows[y] + i]));
                let island = !anchored[piece];
                analysis.add((k, y, run, island), &mut here, under_parts, &reach)?;
            }
        }
        below = Some((Runs::of(layer.mask, input.support_gray), here, layer.z_mm));
    }
    let Analysis { parts, mut regions } = analysis;
    Ok(SupportReport {
        regions: collect(parts, &mut regions),
    })
}

/// The parts grouped by region, each region's statistics from its parts.
fn collect(parts: Vec<Part>, regions: &mut Union) -> Vec<UnsupportedRegion> {
    let mut grouped: std::collections::BTreeMap<usize, Vec<Part>> =
        std::collections::BTreeMap::new();
    for (id, part) in parts.into_iter().enumerate() {
        grouped.entry(regions.find(id)).or_default().push(part);
    }
    let mut out: Vec<UnsupportedRegion> = grouped
        .into_values()
        .map(|parts| {
            let first = parts.iter().map(|p| p.layer).min().unwrap_or(0);
            let last = parts.iter().map(|p| p.layer).max().unwrap_or(0);
            let (mut sum, mut lowest_pixels) = ([0.0; 2], 0_u64);
            for p in parts.iter().filter(|p| p.layer == first) {
                sum[0] += p.sum[0];
                sum[1] += p.sum[1];
                lowest_pixels += p.pixels;
            }
            #[allow(clippy::cast_precision_loss)] // Pixel counts.
            let n = lowest_pixels.max(1) as f64;
            let mut starts: Vec<IslandStart> = parts
                .iter()
                .filter(|p| p.island)
                .map(|p| {
                    #[allow(clippy::cast_precision_loss)] // Pixel counts.
                    let count = p.pixels.max(1) as f64;
                    IslandStart {
                        layer: p.layer,
                        pixels: p.pixels,
                        centroid: [p.sum[0] / count, p.sum[1] / count],
                        bounds: p.bounds,
                    }
                })
                .collect();
            starts.sort_by(|a, b| {
                a.layer
                    .cmp(&b.layer)
                    .then(a.centroid[0].total_cmp(&b.centroid[0]))
                    .then(a.centroid[1].total_cmp(&b.centroid[1]))
            });
            UnsupportedRegion {
                kind: if parts.iter().any(|p| p.island) {
                    RegionKind::Island
                } else {
                    RegionKind::Overhang
                },
                first_layer: first,
                last_layer: last,
                pixels: parts.iter().map(|p| p.pixels).sum(),
                lowest: [sum[0] / n, sum[1] / n],
                bounds: parts.iter().fold([u32::MAX, u32::MAX, 0, 0], |b, p| {
                    [
                        b[0].min(p.bounds[0]),
                        b[1].min(p.bounds[1]),
                        b[2].max(p.bounds[2]),
                        b[3].max(p.bounds[3]),
                    ]
                }),
                starts,
            }
        })
        .collect();
    out.sort_by(|a, b| {
        a.first_layer
            .cmp(&b.first_layer)
            .then(a.lowest[0].total_cmp(&b.lowest[0]))
            .then(a.lowest[1].total_cmp(&b.lowest[1]))
    });
    out
}

fn validate(input: &SupportInput<'_>) -> Result<(), SupportError> {
    if !(input.angle_deg > 0.0 && input.angle_deg < 90.0) {
        return Err(SupportError::InvalidParameter("critical angle"));
    }
    if !input.pitch_mm.iter().all(|p| p.is_finite() && *p > 0.0) {
        return Err(SupportError::InvalidParameter("pixel pitch"));
    }
    if input.material_gray == 0 || input.support_gray == 0 {
        return Err(SupportError::InvalidParameter("gray threshold"));
    }
    let size = input
        .layers
        .first()
        .map(|l| (l.mask.width(), l.mask.height()));
    let consistent = input
        .layers
        .iter()
        .all(|l| Some((l.mask.width(), l.mask.height())) == size)
        && input.layers.windows(2).all(|w| w[1].z_mm > w[0].z_mm)
        && input.layers.iter().all(|l| l.z_mm.is_finite());
    if consistent {
        Ok(())
    } else {
        Err(SupportError::InconsistentLayers)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A mask from rows of '#' (material, 255) and '.' (empty), all of one width.
    fn mask(rows: &[&str]) -> SpanMask {
        let width = u32::try_from(rows[0].len()).unwrap();
        let height = u32::try_from(rows.len()).unwrap();
        let pixels: Vec<u8> = rows
            .iter()
            .flat_map(|r| r.bytes().map(|b| if b == b'#' { 255 } else { 0 }))
            .collect();
        SpanMask::from_pixels(width, height, &pixels).unwrap()
    }

    /// The analysis of `layers` at 0.05 mm, 0.05 mm pixels and 45 degrees: one pixel of reach.
    fn analyse(layers: &[SpanMask]) -> SupportReport {
        analyse_at(layers, 45.0)
    }

    fn analyse_at(layers: &[SpanMask], angle_deg: f64) -> SupportReport {
        let layers: Vec<SupportLayer<'_>> = layers
            .iter()
            .enumerate()
            .map(|(k, mask)| SupportLayer {
                #[allow(clippy::cast_precision_loss)] // A few layers.
                z_mm: 0.025 + 0.05 * k as f64,
                mask,
            })
            .collect();
        find_unsupported(
            &SupportInput {
                layers: &layers,
                pitch_mm: [0.05, 0.05],
                angle_deg,
                material_gray: 1,
                support_gray: 128,
            },
            &mut Cancellation::never(),
        )
        .unwrap()
    }

    #[test]
    fn a_piece_starting_in_mid_air_is_an_island_and_the_plate_holds_the_first_layer() {
        let layers = [
            mask(&["##......", "##......"]),
            mask(&["##......", "##......"]),
            mask(&["##...##.", "##...##."]),
            mask(&["##...##.", "##...##."]),
        ];
        let report = analyse(&layers);
        assert_eq!(report.regions.len(), 1, "{report:?}");
        let island = &report.regions[0];
        assert_eq!(island.kind, RegionKind::Island);
        assert_eq!((island.first_layer, island.last_layer), (2, 2));
        assert_eq!(island.pixels, 4);
        assert_eq!(island.lowest, [6.0, 1.0]);
        assert_eq!(island.bounds, [5, 0, 7, 2]);
        assert_eq!(report.islands(), 1);
    }

    #[test]
    fn pieces_starting_in_mid_air_at_different_layers_keep_their_starts_in_one_region() {
        // A piece starts at layer 1 (columns 3-4) and hangs out to both sides at layer 2;
        // another starts there (columns 10-11). At layer 3 a slab spans them: its column 8,
        // out of reach of both, joins their parts. One region, two starts.
        let layers = [
            mask(&["#............", "#............"]),
            mask(&["#..##........", "#..##........"]),
            mask(&["#######...##.", "#######...##."]),
            mask(&["############.", "############."]),
        ];
        let report = analyse(&layers);
        assert_eq!(report.regions.len(), 1, "{report:?}");
        let region = &report.regions[0];
        assert_eq!(region.kind, RegionKind::Island);
        let starts: Vec<(usize, [f64; 2], u64)> = region
            .starts
            .iter()
            .map(|s| (s.layer, s.centroid, s.pixels))
            .collect();
        assert_eq!(starts, vec![(1, [4.0, 1.0], 4), (2, [11.0, 1.0], 4)]);
        assert_eq!(region.starts[1].bounds, [10, 0, 12, 2]);
    }

    #[test]
    fn a_slope_at_the_critical_angle_holds_and_a_flatter_one_hangs_out() {
        // One pixel more per layer: 0.05 mm over 0.05 mm, exactly 45 degrees.
        let steep = [
            mask(&["##......"]),
            mask(&["###....."]),
            mask(&["####...."]),
            mask(&["#####..."]),
        ];
        assert_eq!(analyse(&steep).regions, []);
        // Two pixels more per layer: each layer hangs out by one, joined into one overhang.
        let flat = [
            mask(&["##......."]),
            mask(&["####....."]),
            mask(&["######..."]),
            mask(&["########."]),
        ];
        let report = analyse(&flat);
        assert_eq!(report.regions.len(), 1, "{report:?}");
        let overhang = &report.regions[0];
        assert_eq!(overhang.kind, RegionKind::Overhang);
        assert_eq!((overhang.first_layer, overhang.last_layer), (1, 3));
        assert_eq!(overhang.pixels, 3);
        // At 20 degrees the reach is 0.137 mm, 2.7 pixels: the same slope holds.
        assert_eq!(analyse_at(&flat, 20.0).regions, []);
    }

    #[test]
    fn a_ceiling_over_a_pillar_hangs_out_on_both_sides_as_one_piece_overhangs() {
        let layers = [
            mask(&["...##...", "...##..."]),
            mask(&["...##...", "...##..."]),
            mask(&["########", "########"]),
        ];
        let report = analyse(&layers);
        // Within reach: one column on each side of the pillar. The rest hangs out, left and
        // right: two overhang regions of the same piece.
        assert_eq!(report.regions.len(), 2, "{report:?}");
        assert!(report
            .regions
            .iter()
            .all(|r| r.kind == RegionKind::Overhang));
        assert_eq!(report.regions.iter().map(|r| r.pixels).sum::<u64>(), 8);
    }

    #[test]
    fn pixels_touching_by_a_corner_are_one_piece() {
        // Layer 1: a piece of two pixels touching by a corner, one of them over the pillar.
        let layers = [mask(&["#...", "...."]), mask(&["#...", ".#.."])];
        // The corner pixel lies 1.41 pixels from the pixel under it, beyond the one pixel of
        // reach: it hangs out from a piece that rests, an overhang, not an island.
        let report = analyse(&layers);
        assert_eq!(report.regions.len(), 1, "{report:?}");
        assert_eq!(report.regions[0].kind, RegionKind::Overhang);
    }

    #[test]
    fn bad_parameters_and_layers_are_refused() {
        let a = mask(&["#."]);
        let b = mask(&["#..."]);
        let layers = [
            SupportLayer {
                z_mm: 0.1,
                mask: &a,
            },
            SupportLayer {
                z_mm: 0.2,
                mask: &b,
            },
        ];
        let input = SupportInput {
            layers: &layers,
            pitch_mm: [0.05, 0.05],
            angle_deg: 45.0,
            material_gray: 1,
            support_gray: 128,
        };
        let run = |input: &SupportInput<'_>| find_unsupported(input, &mut Cancellation::never());
        assert_eq!(run(&input), Err(SupportError::InconsistentLayers));
        for angle_deg in [0.0, 90.0, f64::NAN] {
            let input = SupportInput { angle_deg, ..input };
            assert!(matches!(
                run(&input),
                Err(SupportError::InvalidParameter(_))
            ));
        }
    }
}
