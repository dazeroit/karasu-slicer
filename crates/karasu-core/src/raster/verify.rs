//! Raster-side wall verification (TD-095): the printed wall, breaches and undrained pockets,
//! measured on layer masks alone.
//!
//! **Lattice.** A voxel is one pixel of one layer. Its extent is the panel pitch in X and Y (the
//! mask's columns and rows) and the layer's slab height in Z, centred on the layer's plane.
//! Layer heights may vary. A pixel is material when its gray is at least the input's
//! `material_gray`; the caller derives it from the antialiasing (half coverage).
//!
//! **Stacks.** Three stacks are read:
//! - the print `M`, the exported masks;
//! - optionally the drain holes' cuts `H`, rasterized alone;
//! - optionally the expected cavity `K`, rasterized alone.
//!
//! The *closed* stack `M ∪ H` is the print with its holes filled in. Its empty voxels that are
//! 6-connected to the outside of the stack form the exterior `E`. The others form the cavity `C`.
//! Outside the panel, below the first layer and above the last one, everything is empty.
//!
//! **Findings:**
//! - *Wall:* at a cavity voxel, the exact Euclidean distance between its centre and the nearest
//!   exterior voxel centre. With an expected-cavity stack, only its voxels are measured: other
//!   enclosed voids (a model's own, or reversed slivers left empty) are pockets, not its wall. The measured wall subtracts the two voxels' extents along that line.
//!   The guaranteed wall subtracts both voxels' half-diagonals, which bounds the gap between any
//!   two of their points.
//! - *Breach:* an expected-cavity voxel (`K`) that is exterior in the closed stack.
//! - *Pocket:* a connected component of the empty voxels of `M` itself (holes open) that does
//!   not reach the outside: resin that cannot drain.
//!
//! **Algorithm.**
//! 1. *Components.* Empty runs of every row are labelled by union-find, across rows and layers
//!    (6-connectivity). Memory follows the runs, not the voxels.
//! 2. *Distances.* For each layer, a separable exact distance transform gives the squared
//!    in-plane distance to the nearest exterior pixel, along rows and then down columns by a
//!    lower envelope of parabolas. Values beyond the reach are dropped.
//! 3. *Across layers.* The 3D squared distance is the minimum, over the layers within reach, of
//!    `dz² + d²xy`. This is exact because the transform is separable. It is evaluated only at
//!    cavity voxels next to material, where the minimum lies.
//!
//! Work runs in a fixed order, and parallel parts write disjoint outputs or take minima, so the
//! report is the same for any thread count. Only the region of the panel that holds material is
//! visited, with a one-pixel empty margin around it.

use super::mask::SpanMask;
use crate::cancellation::{Cancellation, Cancelled, Checkpoint};
use std::{collections::VecDeque, fmt};

/// One layer to verify.
#[derive(Clone, Copy, Debug)]
pub struct VerifyLayer<'a> {
    /// Height of the layer's plane, in mm: the centre of its voxels.
    pub z_mm: f64,
    /// The layer's slab height, in mm: the extent of its voxels.
    pub height_mm: f64,
    /// The exported mask.
    pub print: &'a SpanMask,
    /// The drain holes' cuts, rasterized alone, when known.
    pub holes: Option<&'a SpanMask>,
    /// The expected cavity, rasterized alone, when known.
    pub cavity: Option<&'a SpanMask>,
}

/// What to verify.
#[derive(Clone, Copy, Debug)]
pub struct VerifyInput<'a> {
    /// Layers from the bottom up, with strictly increasing planes.
    pub layers: &'a [VerifyLayer<'a>],
    /// Pixel pitch along the masks' columns and rows, in mm.
    pub pitch_mm: [f64; 2],
    /// Smallest gray of a material pixel, at least 1.
    pub material_gray: u8,
    /// The requested wall, in mm. Walls are measured up to it, and thinner ones are thin spots.
    pub wall_mm: f64,
}

/// Bounds on the verifier's work and memory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VerifyLimits {
    /// Empty runs labelled in one stack.
    pub max_runs: usize,
    /// Bytes of one layer's distance buffers.
    pub max_layer_bytes: usize,
    /// Cavity voxels next to material held at once, over the layers within reach.
    pub max_boundary_voxels: usize,
    /// Runs kept for the map; further ones are counted only.
    pub max_marks: usize,
}

impl Default for VerifyLimits {
    fn default() -> Self {
        Self {
            max_runs: 400_000_000,
            max_layer_bytes: 2 << 30,
            max_boundary_voxels: 400_000_000,
            max_marks: 1_000_000,
        }
    }
}

/// A voxel: its layer and its pixel (column, row) on the panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VoxelAt {
    /// Index into the input's layers.
    pub layer: usize,
    /// Column and row on the panel.
    pub pixel: [u32; 2],
}

/// The thinnest printed wall found.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ThinnestWall {
    /// The smallest centre distance from a cavity voxel to an exterior voxel, in mm.
    pub distance_mm: f64,
    /// The smallest wall measured along those lines: the distance less both voxels' extents
    /// along it, in mm.
    pub measured_mm: f64,
    /// A lower bound on the material between any cavity point and any exterior point: the
    /// smallest distance less two voxel half-diagonals, in mm.
    pub guaranteed_mm: f64,
    /// Where the measured wall is thinnest, on the cavity side.
    pub at: VoxelAt,
}

/// Expected cavity that reaches the outside other than through a drain hole.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Breaches {
    /// Breached voxels.
    pub voxels: u64,
    /// Their volume, in mm³.
    pub volume_mm3: f64,
    /// The first one, bottom up, then by row and column.
    pub first: Option<VoxelAt>,
}

/// A region of empty voxels that does not reach the outside: resin that cannot drain.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pocket {
    /// Its voxels.
    pub voxels: u64,
    /// Its volume, in mm³.
    pub volume_mm3: f64,
    /// Its lowest voxel, then by row and column.
    pub first: VoxelAt,
    /// A place to show it: the middle of its longest run in the layer where it has the most
    /// voxels (the lowest such layer).
    pub widest: VoxelAt,
    /// The first and last layers it spans.
    pub layers: [usize; 2],
}

/// What one layer shows on the map, as runs `[row, start, end)` of panel pixels (pockets with their
/// index in [`WallReport::pockets`] as a fourth value).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LayerMarks {
    /// Cavity voxels whose measured wall is under the requested one.
    pub thin: Vec<[u32; 3]>,
    /// Breached voxels.
    pub breach: Vec<[u32; 3]>,
    /// Voxels of undrained pockets: `[row, start, end, pocket]`.
    pub pocket: Vec<[u32; 4]>,
}

/// The verifier's findings.
#[derive(Clone, Debug, PartialEq)]
pub struct WallReport {
    /// The requested wall, in mm.
    pub requested_mm: f64,
    /// The thinnest wall within reach of the requested one, or `None` when every cavity voxel
    /// is farther than that from the exterior (or there is no cavity).
    pub thinnest: Option<ThinnestWall>,
    /// Cavity voxels next to material whose measured wall is under the requested one.
    pub thin_voxels: u64,
    /// Cavity voxels (empty voxels of the closed stack that are not exterior).
    pub cavity_voxels: u64,
    /// Expected cavity reaching the outside; zero without an expected-cavity stack.
    pub breaches: Breaches,
    /// Undrained pockets, from the lowest.
    pub pockets: Vec<Pocket>,
    /// Per layer, what the map shows (empty for a layer without findings).
    pub marks: Vec<LayerMarks>,
    /// Runs left off the map beyond the limit.
    pub marks_dropped: u64,
}

/// Why the layers could not be verified.
#[derive(Clone, Debug, PartialEq)]
pub enum VerifyError {
    /// A mask's size differs from the first print mask's.
    Dimensions {
        /// The layer.
        layer: usize,
    },
    /// A layer's plane is not finite or not above the previous one, or its height is not
    /// positive.
    Layer {
        /// The layer.
        layer: usize,
    },
    /// The pitch, the wall or the material gray is not usable.
    Parameters,
    /// A limit was reached.
    Resource {
        /// What ran out.
        resource: &'static str,
    },
    /// The caller cancelled.
    Cancelled(Cancelled),
}

impl fmt::Display for VerifyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Dimensions { layer } => {
                write!(
                    f,
                    "layer {layer}'s masks differ in size from the first layer's"
                )
            }
            Self::Layer { layer } => write!(
                f,
                "layer {layer} is not above the previous one or has no height"
            ),
            Self::Parameters => {
                f.write_str("the pitch, the wall and the material gray must be positive")
            }
            Self::Resource { resource } => {
                write!(f, "wall verification resource unavailable: {resource}")
            }
            Self::Cancelled(cancelled) => cancelled.fmt(f),
        }
    }
}

impl std::error::Error for VerifyError {}

impl From<Cancelled> for VerifyError {
    fn from(cancelled: Cancelled) -> Self {
        Self::Cancelled(cancelled)
    }
}

/// An interval `[start, end)` of local columns.
type Run = (u32, u32);

/// The region visited: the panel box holding material, with a one-pixel empty margin, in local
/// coordinates (`local = panel - origin + 1`).
#[derive(Clone, Copy, Debug)]
struct Region {
    origin: [u32; 2],
    /// Local width and height, margins included.
    size: [u32; 2],
}

impl Region {
    fn panel(self, local: [u32; 2]) -> [u32; 2] {
        [local[0] - 1 + self.origin[0], local[1] - 1 + self.origin[1]]
    }
}

/// Verify `input` (TD-095).
///
/// # Errors
/// [`VerifyError`] for inconsistent layers or parameters, a reached limit, or cancellation.
pub fn verify_walls(
    input: &VerifyInput<'_>,
    limits: VerifyLimits,
    cancel: &mut Cancellation<'_>,
) -> Result<WallReport, VerifyError> {
    cancel.check(Checkpoint::VerifyStart)?;
    validate(input)?;
    let mut report = WallReport {
        requested_mm: input.wall_mm,
        thinnest: None,
        thin_voxels: 0,
        cavity_voxels: 0,
        breaches: Breaches::default(),
        pockets: Vec::new(),
        marks: vec![LayerMarks::default(); input.layers.len()],
        marks_dropped: 0,
    };
    let Some(region) = region(input, cancel)? else {
        return Ok(report);
    };
    let stacks = Stacks { input, region };
    let closed = stacks.label(true, limits, cancel)?;
    let open = stacks.label(false, limits, cancel)?;
    let mut map = Map {
        marks: &mut report.marks,
        kept: 0,
        dropped: 0,
        limit: limits.max_marks,
    };
    stacks.pockets(&open, &mut map, &mut report.pockets, cancel)?;
    report.breaches = stacks.breaches(&closed, &mut map, cancel)?;
    let walls = stacks.walls(&closed, limits, &mut map, cancel)?;
    report.thinnest = walls.thinnest;
    report.thin_voxels = walls.thin;
    report.cavity_voxels = walls.cavity;
    report.marks_dropped = map.dropped;
    Ok(report)
}

fn validate(input: &VerifyInput<'_>) -> Result<(), VerifyError> {
    let usable = |v: f64| v.is_finite() && v > 0.0;
    if !usable(input.pitch_mm[0])
        || !usable(input.pitch_mm[1])
        || !usable(input.wall_mm)
        || input.material_gray == 0
    {
        return Err(VerifyError::Parameters);
    }
    let Some(first) = input.layers.first() else {
        return Ok(());
    };
    let size = [first.print.width(), first.print.height()];
    let mut below = f64::NEG_INFINITY;
    for (layer, l) in input.layers.iter().enumerate() {
        let masks = [Some(l.print), l.holes, l.cavity];
        if masks
            .into_iter()
            .flatten()
            .any(|m| [m.width(), m.height()] != size)
        {
            return Err(VerifyError::Dimensions { layer });
        }
        if !l.z_mm.is_finite() || l.z_mm <= below || !usable(l.height_mm) {
            return Err(VerifyError::Layer { layer });
        }
        below = l.z_mm;
    }
    Ok(())
}

/// The material runs of row `y` of `mask`, in panel columns, merged across adjacent spans.
fn mask_runs(mask: &SpanMask, y: u32, threshold: u8, out: &mut Vec<Run>) {
    out.clear();
    for span in mask.row(y).unwrap_or(&[]) {
        if span.value() < threshold {
            continue;
        }
        match out.last_mut() {
            Some(last) if last.1 == span.start() => last.1 = span.end(),
            _ => out.push((span.start(), span.end())),
        }
    }
}

/// The union of two sorted, disjoint run lists.
fn union_runs(a: &[Run], b: &[Run], out: &mut Vec<Run>) {
    out.clear();
    let (mut i, mut j) = (0, 0);
    while i < a.len() || j < b.len() {
        let next = if j == b.len() || (i < a.len() && a[i].0 <= b[j].0) {
            i += 1;
            a[i - 1]
        } else {
            j += 1;
            b[j - 1]
        };
        match out.last_mut() {
            Some(last) if next.0 <= last.1 => last.1 = last.1.max(next.1),
            _ => out.push(next),
        }
    }
}

/// The intersection of two sorted, disjoint run lists.
fn intersect_runs(a: &[Run], b: &[Run], out: &mut Vec<Run>) {
    out.clear();
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        let (start, end) = (a[i].0.max(b[j].0), a[i].1.min(b[j].1));
        if start < end {
            out.push((start, end));
        }
        if a[i].1 < b[j].1 {
            i += 1;
        } else {
            j += 1;
        }
    }
}

/// The panel box holding material in any stack, or `None` when there is none.
fn region(
    input: &VerifyInput<'_>,
    cancel: &mut Cancellation<'_>,
) -> Result<Option<Region>, VerifyError> {
    let (mut min, mut max) = ([u32::MAX; 2], [0u32; 2]);
    let mut runs = Vec::new();
    for (layer, l) in input.layers.iter().enumerate() {
        cancel.poll(Checkpoint::VerifyStart, layer)?;
        let masks = [
            (Some(l.print), input.material_gray),
            (l.holes, 1),
            (l.cavity, input.material_gray),
        ];
        for (mask, threshold) in masks.into_iter().filter_map(|(m, t)| Some((m?, t))) {
            for y in 0..mask.height() {
                mask_runs(mask, y, threshold, &mut runs);
                if let (Some(first), Some(last)) = (runs.first(), runs.last()) {
                    min = [min[0].min(first.0), min[1].min(y)];
                    max = [max[0].max(last.1), max[1].max(y + 1)];
                }
            }
        }
    }
    if min[0] == u32::MAX {
        return Ok(None);
    }
    Ok(Some(Region {
        origin: min,
        size: [max[0] - min[0] + 2, max[1] - min[1] + 2],
    }))
}

/// Empty runs labelled across layers, and which component each belongs to.
struct Labels {
    /// The first run of each layer, and the total after the last.
    first: Vec<usize>,
    /// Each run's component, as a dense index; component 0 is the outside.
    component: Vec<u32>,
}

/// The stacks of the input in local coordinates.
struct Stacks<'a> {
    input: &'a VerifyInput<'a>,
    region: Region,
}

impl Stacks<'_> {
    /// The material runs of local row `row` of `layer`, in local columns: the print, with the
    /// holes filled in when `closed`.
    fn material(
        &self,
        layer: usize,
        row: u32,
        closed: bool,
        scratch: &mut [Vec<Run>; 2],
    ) -> Vec<Run> {
        let mut out = Vec::new();
        let size = self.region.size;
        if row == 0 || row + 1 >= size[1] {
            return out;
        }
        let l = &self.input.layers[layer];
        let y = row - 1 + self.region.origin[1];
        let [print, holes] = scratch;
        mask_runs(l.print, y, self.input.material_gray, print);
        match l.holes.filter(|_| closed) {
            Some(h) => {
                mask_runs(h, y, 1, holes);
                union_runs(print, holes, &mut out);
            }
            None => out.clone_from(print),
        }
        let origin = self.region.origin[0];
        for run in &mut out {
            *run = (run.0 + 1 - origin, run.1 + 1 - origin);
        }
        out
    }

    /// The expected cavity's runs of local row `row` of `layer`, in local columns.
    fn expected(&self, layer: usize, row: u32, out: &mut Vec<Run>) {
        out.clear();
        let Some(cavity) = self.input.layers[layer].cavity else {
            return;
        };
        if row == 0 || row + 1 >= self.region.size[1] {
            return;
        }
        mask_runs(
            cavity,
            row - 1 + self.region.origin[1],
            self.input.material_gray,
            out,
        );
        let origin = self.region.origin[0];
        for run in out.iter_mut() {
            *run = (run.0 + 1 - origin, run.1 + 1 - origin);
        }
    }

    /// The empty runs of every row of `layer`: the complement of its material in the region.
    fn empty_rows(&self, layer: usize, closed: bool) -> Vec<Vec<Run>> {
        let mut scratch = [Vec::new(), Vec::new()];
        (0..self.region.size[1])
            .map(|row| {
                complement(
                    &self.material(layer, row, closed, &mut scratch),
                    self.region.size[0],
                )
            })
            .collect()
    }

    /// Label the empty runs of the closed (`closed`) or open stack into components.
    fn label(
        &self,
        closed: bool,
        limits: VerifyLimits,
        cancel: &mut Cancellation<'_>,
    ) -> Result<Labels, VerifyError> {
        let layers = self.input.layers.len();
        let width = self.region.size[0];
        let mut parent: Vec<u32> = vec![0];
        let mut first = Vec::with_capacity(layers + 1);
        let mut previous: Vec<Vec<(Run, u32)>> = Vec::new();
        for layer in 0..layers {
            cancel.check(Checkpoint::VerifyComponents)?;
            first.push(parent.len());
            let rows = self.empty_rows(layer, closed);
            let mut current: Vec<Vec<(Run, u32)>> = Vec::with_capacity(rows.len());
            for (row, runs) in rows.into_iter().enumerate() {
                let mut labelled = Vec::with_capacity(runs.len());
                for run in runs {
                    let id = u32::try_from(parent.len())
                        .ok()
                        .filter(|&id| (id as usize) < limits.max_runs)
                        .ok_or(VerifyError::Resource {
                            resource: "empty runs",
                        })?;
                    parent.try_reserve(1).map_err(|_| VerifyError::Resource {
                        resource: "empty runs",
                    })?;
                    parent.push(id);
                    // The margins, and the layers below the first and above the last, are
                    // outside. A run touching the left or right margin is in its row; the top
                    // and bottom margin rows are single runs from column 0.
                    if run.0 == 0 || run.1 == width || layer == 0 || layer + 1 == layers {
                        join(&mut parent, id, 0);
                    }
                    labelled.push((run, id));
                }
                if let Some(above) = current.last() {
                    join_overlaps(&mut parent, above, &labelled);
                }
                if let Some(below) = previous.get(row) {
                    join_overlaps(&mut parent, below, &labelled);
                }
                current.push(labelled);
            }
            previous = current;
        }
        first.push(parent.len());
        // Dense component indexes, the outside first.
        let mut component = vec![u32::MAX; parent.len()];
        let mut next = 0u32;
        for id in 0..parent.len() {
            cancel.poll(Checkpoint::VerifyComponents, id)?;
            let root = find(&mut parent, u32::try_from(id).unwrap_or(u32::MAX)) as usize;
            if component[root] == u32::MAX {
                component[root] = next;
                next += 1;
            }
            component[id] = component[root];
        }
        Ok(Labels { first, component })
    }

    /// Voxel volume of `layer`, in mm³.
    fn voxel_volume(&self, layer: usize) -> f64 {
        self.input.pitch_mm[0] * self.input.pitch_mm[1] * self.input.layers[layer].height_mm
    }

    /// The undrained pockets: components of the open stack other than the outside.
    fn pockets(
        &self,
        open: &Labels,
        map: &mut Map<'_>,
        out: &mut Vec<Pocket>,
        cancel: &mut Cancellation<'_>,
    ) -> Result<(), VerifyError> {
        let mut index: Vec<u32> = Vec::new();
        // Per pocket, its voxels in the layer being read and its longest run there.
        let mut here: Vec<Widest> = Vec::new();
        for layer in 0..self.input.layers.len() {
            cancel.check(Checkpoint::VerifyComponents)?;
            let mut id = open.first[layer];
            for (row, runs) in self.empty_rows(layer, false).into_iter().enumerate() {
                for run in runs {
                    let component = open.component[id] as usize;
                    id += 1;
                    if component == 0 {
                        continue;
                    }
                    if index.len() <= component {
                        index.resize(component + 1, u32::MAX);
                    }
                    let row = u32::try_from(row).unwrap_or(u32::MAX);
                    let length = run.1 - run.0;
                    let middle = VoxelAt {
                        layer,
                        pixel: self.region.panel([run.0 + length / 2, row]),
                    };
                    if index[component] == u32::MAX {
                        index[component] = u32::try_from(out.len()).unwrap_or(u32::MAX);
                        out.push(Pocket {
                            voxels: 0,
                            volume_mm3: 0.0,
                            first: VoxelAt {
                                layer,
                                pixel: self.region.panel([run.0, row]),
                            },
                            widest: middle,
                            layers: [layer, layer],
                        });
                        here.push(Widest::default());
                    }
                    let number = index[component] as usize;
                    let pocket = &mut out[number];
                    pocket.voxels += u64::from(length);
                    pocket.volume_mm3 += f64::from(length) * self.voxel_volume(layer);
                    pocket.layers[1] = layer;
                    here[number].add(layer, length, middle, pocket);
                    map.mark_pocket(layer, self.region, row, run, number);
                }
            }
        }
        for (widest, pocket) in here.iter_mut().zip(out.iter_mut()) {
            widest.settle(pocket);
        }
        Ok(())
    }

    /// Expected-cavity voxels that are exterior in the closed stack.
    fn breaches(
        &self,
        closed: &Labels,
        map: &mut Map<'_>,
        cancel: &mut Cancellation<'_>,
    ) -> Result<Breaches, VerifyError> {
        let mut breaches = Breaches::default();
        let (mut expected, mut hits) = (Vec::new(), Vec::new());
        for layer in 0..self.input.layers.len() {
            cancel.check(Checkpoint::VerifyComponents)?;
            if self.input.layers[layer].cavity.is_none() {
                continue;
            }
            let mut id = closed.first[layer];
            for (row, runs) in self.empty_rows(layer, true).into_iter().enumerate() {
                let row = u32::try_from(row).unwrap_or(u32::MAX);
                let exterior: Vec<Run> = runs
                    .iter()
                    .enumerate()
                    .filter(|&(k, _)| closed.component[id + k] == 0)
                    .map(|(_, &run)| run)
                    .collect();
                id += runs.len();
                self.expected(layer, row, &mut expected);
                intersect_runs(&exterior, &expected, &mut hits);
                for &run in &hits {
                    breaches.voxels += u64::from(run.1 - run.0);
                    breaches.volume_mm3 += f64::from(run.1 - run.0) * self.voxel_volume(layer);
                    breaches.first.get_or_insert(VoxelAt {
                        layer,
                        pixel: self.region.panel([run.0, row]),
                    });
                    map.mark(layer, Kind::Breach, self.region, row, run);
                }
            }
        }
        Ok(breaches)
    }
}

/// The complement of sorted, disjoint `runs` in `0..width`.
fn complement(runs: &[Run], width: u32) -> Vec<Run> {
    let mut out = Vec::with_capacity(runs.len() + 1);
    let mut at = 0;
    for &(start, end) in runs {
        if start > at {
            out.push((at, start));
        }
        at = end;
    }
    if at < width {
        out.push((at, width));
    }
    out
}

/// Join runs of `b` with the runs of `a` they overlap (both sorted by start).
fn join_overlaps(parent: &mut [u32], a: &[(Run, u32)], b: &[(Run, u32)]) {
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        let ((sa, ea), ia) = a[i];
        let ((sb, eb), ib) = b[j];
        if sa.max(sb) < ea.min(eb) {
            join(parent, ia, ib);
        }
        if ea < eb {
            i += 1;
        } else {
            j += 1;
        }
    }
}

fn find(parent: &mut [u32], mut v: u32) -> u32 {
    while parent[v as usize] != v {
        let next = parent[parent[v as usize] as usize];
        parent[v as usize] = next;
        v = next;
    }
    v
}

/// Join the components of `a` and `b`, the smaller root becoming the root.
fn join(parent: &mut [u32], a: u32, b: u32) {
    let (a, b) = (find(parent, a), find(parent, b));
    if a != b {
        parent[a.max(b) as usize] = a.min(b);
    }
}

/// A pocket's voxels in the layer being read, its longest run there, and the most voxels it had
/// in an earlier layer.
#[derive(Default)]
struct Widest {
    layer: usize,
    voxels: u64,
    longest: u32,
    middle: Option<VoxelAt>,
    best: u64,
}

impl Widest {
    fn add(&mut self, layer: usize, length: u32, middle: VoxelAt, pocket: &mut Pocket) {
        if self.middle.is_some() && self.layer != layer {
            self.settle(pocket);
            self.voxels = 0;
            self.longest = 0;
        }
        self.layer = layer;
        self.voxels += u64::from(length);
        if length > self.longest {
            self.longest = length;
            self.middle = Some(middle);
        }
    }

    /// Keep the layer read so far as the pocket's widest when it has more voxels.
    fn settle(&mut self, pocket: &mut Pocket) {
        if let Some(middle) = self.middle.filter(|_| self.voxels > self.best) {
            self.best = self.voxels;
            pocket.widest = middle;
        }
    }
}

/// What a map run shows (pockets are marked with their index instead).
#[derive(Clone, Copy)]
enum Kind {
    Thin,
    Breach,
}

/// The map, bounded.
struct Map<'a> {
    marks: &'a mut [LayerMarks],
    kept: usize,
    dropped: u64,
    limit: usize,
}

impl Map<'_> {
    fn mark_pocket(&mut self, layer: usize, region: Region, row: u32, run: Run, pocket: usize) {
        if self.kept >= self.limit {
            self.dropped += 1;
            return;
        }
        self.kept += 1;
        let [start, y] = region.panel([run.0, row]);
        let number = u32::try_from(pocket).unwrap_or(u32::MAX);
        self.marks[layer]
            .pocket
            .push([y, start, start + (run.1 - run.0), number]);
    }

    fn mark(&mut self, layer: usize, kind: Kind, region: Region, row: u32, run: Run) {
        if self.kept >= self.limit {
            self.dropped += 1;
            return;
        }
        self.kept += 1;
        let [start, y] = region.panel([run.0, row]);
        let entry = [y, start, start + (run.1 - run.0)];
        let marks = &mut self.marks[layer];
        match kind {
            Kind::Thin => &mut marks.thin,
            Kind::Breach => &mut marks.breach,
        }
        .push(entry);
    }
}

/// What the distance pass found.
#[derive(Default)]
struct Walls {
    thinnest: Option<ThinnestWall>,
    thin: u64,
    cavity: u64,
}

/// A cavity voxel next to material, with the nearest exterior voxel found so far.
#[derive(Clone, Copy, Debug)]
struct Boundary {
    row: u32,
    x: u32,
    /// Smallest squared centre distance so far, in mm².
    best: f64,
    /// Both voxels' extents along the line to that exterior voxel, in mm.
    step: f64,
}

/// A layer within reach of the layer whose distances are being combined.
struct Active {
    layer: usize,
    voxels: Vec<Boundary>,
}

/// One layer's in-plane distances to its exterior, where they are read: at the pixels of the
/// cavity voxels next to material in the layers within reach. They are kept as integer offsets
/// to the nearest exterior pixel, from which the distance is computed where it is read.
struct Distances {
    /// Columns to the nearest exterior pixel of the row, or [`FAR`] beyond reach, row-major.
    along: Vec<u16>,
    /// Per pixel read, the columns and rows to the nearest exterior pixel, or [`FAR`] in both
    /// beyond reach, column-major (`[column * rows + row]`); other pixels are not kept current.
    cells: Vec<[u16; 2]>,
    /// How many layers within reach read each pixel, column-major.
    read: Vec<u16>,
}

/// Beyond reach.
const FAR: u16 = u16::MAX;

impl Distances {
    fn cell(&self, rows: usize, x: usize, row: usize) -> [u16; 2] {
        self.cells[x * rows + row]
    }

    /// Count the pixels of `voxels` as read (`add`) or no longer read.
    fn reading(&mut self, rows: usize, voxels: &[Boundary], add: bool) {
        for b in voxels {
            let count = &mut self.read[b.x as usize * rows + b.row as usize];
            *count = if add {
                count.saturating_add(1)
            } else {
                count.saturating_sub(1)
            };
        }
    }
}

/// The position of row `index` along a column, in mm.
fn position(index: usize, pitch: f64) -> f64 {
    f64::from(u32::try_from(index).unwrap_or(u32::MAX)) * pitch
}

/// The lower envelope of one column's parabolas `(t - row·pitch)² + (gap·pitch_x)²` (Felzenszwalb
/// and Huttenlocher), built row by row: rows with an exterior pixel within reach, each with where
/// it starts to be the lowest.
#[derive(Default)]
struct Hull {
    rows: Vec<usize>,
    starts: Vec<f64>,
    heights: Vec<f64>,
    gaps: Vec<u16>,
    /// The segment the last query fell in; queries go down the column.
    segment: usize,
}

impl Hull {
    fn clear(&mut self) {
        self.rows.clear();
        self.starts.clear();
        self.heights.clear();
        self.gaps.clear();
        self.segment = 0;
    }

    /// Add row `row`, whose nearest exterior pixel in its row is `gap` columns away.
    fn push(&mut self, row: usize, gap: u16, [pu, pv]: [f64; 2]) {
        if gap == FAR {
            return;
        }
        let along = f64::from(gap) * pu;
        let height = along * along;
        let at = position(row, pv);
        loop {
            let (Some(&top), Some(&top_height)) = (self.rows.last(), self.heights.last()) else {
                self.start(row, f64::NEG_INFINITY, height, gap);
                return;
            };
            let top_at = position(top, pv);
            let start =
                ((height + at * at) - (top_height + top_at * top_at)) / (2.0 * (at - top_at));
            if start <= self.starts[self.starts.len() - 1] {
                self.rows.pop();
                self.starts.pop();
                self.heights.pop();
                self.gaps.pop();
            } else {
                self.start(row, start, height, gap);
                return;
            }
        }
    }

    fn start(&mut self, row: usize, start: f64, height: f64, gap: u16) {
        self.rows.push(row);
        self.starts.push(start);
        self.heights.push(height);
        self.gaps.push(gap);
    }

    /// Row `row`'s offset to its nearest exterior pixel, or [`FAR`] beyond `reach2` (a squared
    /// distance); rows are queried in increasing order after every push.
    fn query(&mut self, row: usize, pv: f64, reach2: f64) -> [u16; 2] {
        if self.rows.is_empty() {
            return [FAR; 2];
        }
        let at = position(row, pv);
        while self.segment + 1 < self.rows.len() && self.starts[self.segment + 1] < at {
            self.segment += 1;
        }
        let nearest = self.rows[self.segment];
        let across = at - position(nearest, pv);
        let rows = u16::try_from(row.abs_diff(nearest))
            .ok()
            .filter(|&d| d != FAR);
        match rows {
            Some(rows) if across * across + self.heights[self.segment] < reach2 => {
                [self.gaps[self.segment], rows]
            }
            _ => [FAR; 2],
        }
    }
}

/// Threads for parallel parts.
fn threads() -> usize {
    std::thread::available_parallelism().map_or(1, usize::from)
}

/// Run `work(first, chunk)` on contiguous chunks of `items`, `first` being the chunk's offset.
fn chunks<T: Send>(items: &mut [T], unit: usize, work: &(dyn Fn(usize, &mut [T]) + Sync)) {
    let units = items.len() / unit.max(1);
    let per = units.div_ceil(threads()).max(1) * unit.max(1);
    if per >= items.len() {
        work(0, items);
        return;
    }
    std::thread::scope(|scope| {
        for (index, chunk) in items.chunks_mut(per).enumerate() {
            scope.spawn(move || work(index * per, chunk));
        }
    });
}

impl Stacks<'_> {
    /// The cavity voxels of `layer` next to material (sorted by row and column), and the count of
    /// its cavity voxels. Each starts from the empty space below the first layer and above the
    /// last one, as near as `reach`.
    fn boundary(&self, layer: usize, closed: &Labels, reach: f64) -> (Vec<Boundary>, u64) {
        let layers_in = self.input.layers;
        let (lowest, highest) = (&layers_in[0], &layers_in[layers_in.len() - 1]);
        let here = layers_in[layer];
        // The nearest virtual empty layer, one slab beyond the stack, and the line's step.
        let (start_best, start_step) = [
            (
                here.z_mm - (lowest.z_mm - lowest.height_mm),
                lowest.height_mm,
            ),
            (
                (highest.z_mm + highest.height_mm) - here.z_mm,
                highest.height_mm,
            ),
        ]
        .into_iter()
        .filter(|&(dz, _)| dz < reach)
        .map(|(dz, h)| (dz * dz, f64::midpoint(here.height_mm, h)))
        .fold((f64::INFINITY, 0.0), |a, b| if b.0 < a.0 { b } else { a });
        let size = self.region.size;
        let layers = self.input.layers.len();
        let mut scratch = [Vec::new(), Vec::new()];
        let mut material = |l: usize, row: u32| -> Vec<Run> {
            if l >= layers || row >= size[1] {
                Vec::new()
            } else {
                self.material(l, row, true, &mut scratch)
            }
        };
        let (mut out, mut cavity) = (Vec::new(), 0u64);
        let mut id = closed.first[layer];
        let mut touching = Vec::new();
        let mut columns = Vec::new();
        let mut expected = Vec::new();
        let expected_given = self.input.layers.iter().any(|l| l.cavity.is_some());
        for (row, runs) in self.empty_rows(layer, true).into_iter().enumerate() {
            let row = u32::try_from(row).unwrap_or(u32::MAX);
            let ids = id..id + runs.len();
            id += runs.len();
            let inside: Vec<Run> = runs
                .into_iter()
                .zip(ids)
                .filter(|&(_, k)| closed.component[k] != 0)
                .map(|(run, _)| run)
                .collect();
            if inside.is_empty() {
                continue;
            }
            columns.clear();
            for &(start, end) in &inside {
                cavity += u64::from(end - start);
                // Runs are maximal, so both ends border material in the row.
                columns.extend([start, end - 1]);
            }
            let neighbours = [
                material(layer, row.wrapping_sub(1)),
                material(layer, row + 1),
                layer
                    .checked_sub(1)
                    .map_or_else(Vec::new, |l| material(l, row)),
                material(layer + 1, row),
            ];
            for solid in &neighbours {
                intersect_runs(&inside, solid, &mut touching);
                for &(start, end) in &touching {
                    columns.extend(start..end);
                }
            }
            columns.sort_unstable();
            columns.dedup();
            // With an expected cavity, the wall is the expected cavity's: other enclosed voids
            // (such as reversed slivers left empty) are pockets, not the cavity's wall.
            if expected_given {
                self.expected(layer, row, &mut expected);
                let mut runs = expected.iter().peekable();
                columns.retain(|&x| {
                    while runs.peek().is_some_and(|r| r.1 <= x) {
                        runs.next();
                    }
                    runs.peek().is_some_and(|r| r.0 <= x)
                });
            }
            out.extend(columns.iter().map(|&x| Boundary {
                row,
                x,
                best: start_best,
                step: start_step,
            }));
        }
        (out, cavity)
    }

    /// The exterior runs of every row of `layer` in the closed stack.
    fn exterior_rows(&self, layer: usize, closed: &Labels) -> Vec<Vec<Run>> {
        let mut id = closed.first[layer];
        self.empty_rows(layer, true)
            .into_iter()
            .map(|runs| {
                let ids = id..id + runs.len();
                id += runs.len();
                runs.into_iter()
                    .zip(ids)
                    .filter(|&(_, k)| closed.component[k] == 0)
                    .map(|(run, _)| run)
                    .collect()
            })
            .collect()
    }

    /// The in-plane distances of `layer` to its exterior, up to `reach` mm.
    fn distances(&self, layer: usize, closed: &Labels, reach: f64, out: &mut Distances) {
        let [width, rows] = self.region.size.map(|v| v as usize);
        let [pu, pv] = self.input.pitch_mm;
        let exterior = self.exterior_rows(layer, closed);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // Small and positive.
        let cap = ((reach / pu).floor() as u32 + 1).min(u32::from(FAR) - 1);
        // Along rows: the distance in columns to the nearest exterior pixel, by two sweeps.
        chunks(&mut out.along, width, &|first, chunk| {
            for (k, line) in chunk.chunks_mut(width).enumerate() {
                let runs = &exterior[first / width + k];
                let mut near = u32::MAX;
                let mut next = runs.iter().peekable();
                for (x, slot) in (0u32..).zip(line.iter_mut()) {
                    while next.peek().is_some_and(|r| r.1 <= x) {
                        near = next.next().map_or(near, |r| r.1 - 1);
                    }
                    let gap_left = if near == u32::MAX { u32::MAX } else { x - near };
                    let gap_right = match next.peek() {
                        Some(r) if r.0 <= x => 0,
                        Some(r) => r.0 - x,
                        None => u32::MAX,
                    };
                    let gap = gap_left.min(gap_right);
                    *slot = u16::try_from(gap)
                        .ok()
                        .filter(|_| gap <= cap)
                        .unwrap_or(FAR);
                }
            }
        });
        // Down columns: the lower envelope of parabolas, on the anisotropic pitch, only over the
        // rows within reach of a pixel that is read; farther rows cannot be the nearest.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // Small and positive.
        let window = (reach / pv).ceil() as usize + 1;
        let (along, read) = (&out.along, &out.read);
        let reach2 = reach * reach;
        chunks(&mut out.cells, rows, &|first, chunk| {
            let mut hull = Hull::default();
            let mut wanted = Vec::new();
            for (k, cells) in chunk.chunks_mut(rows).enumerate() {
                let x = first / rows + k;
                wanted.clear();
                wanted.extend(
                    read[x * rows..(x + 1) * rows]
                        .iter()
                        .enumerate()
                        .filter(|&(_, &n)| n > 0)
                        .map(|(row, _)| row),
                );
                let mut i = 0;
                while i < wanted.len() {
                    let low = wanted[i].saturating_sub(window);
                    let mut high = (wanted[i] + window).min(rows - 1);
                    let mut j = i + 1;
                    while j < wanted.len() && wanted[j].saturating_sub(window) <= high + 1 {
                        high = (wanted[j] + window).min(rows - 1);
                        j += 1;
                    }
                    hull.clear();
                    for row in low..=high {
                        hull.push(row, along[row * width + x], [pu, pv]);
                    }
                    for &row in &wanted[i..j] {
                        cells[row] = hull.query(row, pv, reach2);
                    }
                    i = j;
                }
            }
        });
    }

    /// The thinnest walls: in-plane distances combined across the layers within reach.
    fn walls(
        &self,
        closed: &Labels,
        limits: VerifyLimits,
        map: &mut Map<'_>,
        cancel: &mut Cancellation<'_>,
    ) -> Result<Walls, VerifyError> {
        let layers = self.input.layers;
        let [pu, pv] = self.input.pitch_mm;
        let tallest = layers.iter().map(|l| l.height_mm).fold(0.0, f64::max);
        let half_diagonal = 0.5 * (pu * pu + pv * pv + tallest * tallest).sqrt();
        // A measured wall under the requested one needs a distance under this.
        let reach = self.input.wall_mm + 2.0 * half_diagonal;
        let [width, rows] = self.region.size.map(|v| v as usize);
        let pixels = width * rows;
        let bytes = pixels
            .checked_mul(2 + 4 + 2)
            .zip(Some(0))
            .and_then(|(a, b)| a.checked_add(b))
            .ok_or(VerifyError::Resource {
                resource: "distance buffers",
            })?;
        if bytes > limits.max_layer_bytes {
            return Err(VerifyError::Resource {
                resource: "distance buffers",
            });
        }
        let mut distances = Distances {
            along: vec![0; pixels],
            cells: vec![[FAR; 2]; pixels],
            read: vec![0; pixels],
        };
        let mut walls = Walls::default();
        let mut found = Found::default();
        let mut active: VecDeque<Active> = VecDeque::new();
        let (mut next, mut held) = (0usize, 0usize);
        for j in 0..layers.len() {
            cancel.check(Checkpoint::VerifyDistances)?;
            while next < layers.len() && layers[next].z_mm - layers[j].z_mm < reach {
                let (voxels, cavity) = self.boundary(next, closed, reach);
                walls.cavity += cavity;
                held += voxels.len();
                if held > limits.max_boundary_voxels {
                    return Err(VerifyError::Resource {
                        resource: "cavity voxels next to material",
                    });
                }
                distances.reading(rows, &voxels, true);
                active.push_back(Active {
                    layer: next,
                    voxels,
                });
                next += 1;
            }
            while active
                .front()
                .is_some_and(|a| layers[j].z_mm - layers[a.layer].z_mm >= reach)
            {
                if let Some(done) = active.pop_front() {
                    held -= done.voxels.len();
                    distances.reading(rows, &done.voxels, false);
                    self.finish(&done, map, &mut found);
                }
            }
            if active.iter().all(|a| a.voxels.is_empty()) {
                continue;
            }
            self.distances(j, closed, reach, &mut distances);
            self.combine(j, &distances, active.make_contiguous(), reach);
        }
        for done in active {
            self.finish(&done, map, &mut found);
        }
        walls.thin = found.thin;
        walls.thinnest = found.measured.map(|(measured_mm, at)| {
            let distance_mm = found.distance;
            ThinnestWall {
                distance_mm,
                measured_mm,
                guaranteed_mm: distance_mm - 2.0 * half_diagonal,
                at,
            }
        });
        Ok(walls)
    }

    /// Combine layer `j`'s in-plane distances into the nearest exterior of every cavity voxel of
    /// the `active` layers within `reach` of it.
    fn combine(&self, j: usize, distances: &Distances, active: &mut [Active], reach: f64) {
        let layers = self.input.layers;
        let [pu, pv] = self.input.pitch_mm;
        let rows = self.region.size[1] as usize;
        let here = layers[j];
        let reach2 = reach * reach;
        chunks(active, 1, &|_, chunk| {
            for a in chunk {
                let there = layers[a.layer];
                let dz = (here.z_mm - there.z_mm).abs();
                if dz * dz >= reach2 {
                    continue;
                }
                let extent = f64::midpoint(here.height_mm, there.height_mm);
                for b in &mut a.voxels {
                    let cell = distances.cell(rows, b.x as usize, b.row as usize);
                    if cell == [FAR; 2] {
                        continue;
                    }
                    let [dx, dy] = cell.map(f64::from);
                    let (across, down) = (dx * pu, dy * pv);
                    let candidate = dz * dz + (across * across + down * down);
                    if candidate < b.best {
                        b.best = candidate;
                        b.step = (pu * pu * dx + pv * pv * dy + extent * dz) / candidate.sqrt();
                    }
                }
            }
        });
    }

    /// Fold a layer whose distances are all combined into the findings and the map.
    fn finish(&self, done: &Active, map: &mut Map<'_>, found: &mut Found) {
        let wall = self.input.wall_mm;
        let mut thin: Option<(u32, Run)> = None;
        for b in &done.voxels {
            if !b.best.is_finite() {
                continue;
            }
            let distance = b.best.sqrt();
            let measured = distance - b.step;
            found.distance = found.distance.min(distance);
            if found.measured.is_none_or(|(m, _)| measured < m) {
                found.measured = Some((
                    measured,
                    VoxelAt {
                        layer: done.layer,
                        pixel: self.region.panel([b.x, b.row]),
                    },
                ));
            }
            if measured < wall {
                found.thin += 1;
                match &mut thin {
                    Some((row, run)) if *row == b.row && run.1 == b.x => run.1 += 1,
                    _ => {
                        if let Some((row, run)) = thin.take() {
                            map.mark(done.layer, Kind::Thin, self.region, row, run);
                        }
                        thin = Some((b.row, (b.x, b.x + 1)));
                    }
                }
            }
        }
        if let Some((row, run)) = thin {
            map.mark(done.layer, Kind::Thin, self.region, row, run);
        }
    }
}

/// The running findings of the distance pass.
struct Found {
    distance: f64,
    measured: Option<(f64, VoxelAt)>,
    thin: u64,
}

impl Default for Found {
    fn default() -> Self {
        Self {
            distance: f64::INFINITY,
            measured: None,
            thin: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    const PITCH: [f64; 2] = [0.017, 0.025];

    /// A stack of `layers` masks of `w × h`, gray from `gray(x, y, layer)`.
    fn stack(w: u32, h: u32, layers: usize, gray: impl Fn(u32, u32, usize) -> u8) -> Vec<SpanMask> {
        (0..layers)
            .map(|l| {
                let pixels: Vec<u8> = (0..h)
                    .flat_map(|y| (0..w).map(move |x| (x, y)))
                    .map(|(x, y)| gray(x, y, l))
                    .collect();
                SpanMask::from_pixels(w, h, &pixels).unwrap()
            })
            .collect()
    }

    fn layers<'a>(
        print: &'a [SpanMask],
        holes: Option<&'a [SpanMask]>,
        cavity: Option<&'a [SpanMask]>,
        heights: &[f64],
    ) -> Vec<VerifyLayer<'a>> {
        let mut z = 0.0;
        (0..print.len())
            .map(|l| {
                z += if l == 0 {
                    heights[0] / 2.0
                } else {
                    f64::midpoint(heights[l - 1], heights[l])
                };
                VerifyLayer {
                    z_mm: z,
                    height_mm: heights[l],
                    print: &print[l],
                    holes: holes.map(|h| &h[l]),
                    cavity: cavity.map(|c| &c[l]),
                }
            })
            .collect()
    }

    fn verify(layers: &[VerifyLayer<'_>], wall: f64) -> WallReport {
        verify_walls(
            &VerifyInput {
                layers,
                pitch_mm: PITCH,
                material_gray: 128,
                wall_mm: wall,
            },
            VerifyLimits::default(),
            &mut Cancellation::never(),
        )
        .unwrap()
    }

    #[allow(clippy::many_single_char_names)] // Lattice coordinates and sizes.
    /// Brute force on a padded lattice (one empty pixel around, one empty layer below and
    /// above): the closed stack's cavity voxel count, and the smallest centre distance from a
    /// cavity voxel to an exterior voxel.
    fn brute(layers: &[VerifyLayer<'_>]) -> (u64, f64) {
        let (w, h) = (
            layers[0].print.width() as usize + 2,
            layers[0].print.height() as usize + 2,
        );
        let n = layers.len() + 2;
        let z = |l: usize| match l {
            0 => layers[0].z_mm - layers[0].height_mm,
            l if l == n - 1 => layers[n - 3].z_mm + layers[n - 3].height_mm,
            l => layers[l - 1].z_mm,
        };
        let solid = |x: usize, y: usize, l: usize| {
            if x == 0 || y == 0 || l == 0 || x == w - 1 || y == h - 1 || l == n - 1 {
                return false;
            }
            let layer = &layers[l - 1];
            let (px, py) = (u32::try_from(x - 1).unwrap(), u32::try_from(y - 1).unwrap());
            layer.print.pixel(px, py).unwrap() >= 128
                || layer.holes.is_some_and(|m| m.pixel(px, py).unwrap() > 0)
        };
        let index = |x: usize, y: usize, l: usize| (l * h + y) * w + x;
        let mut exterior = vec![false; w * h * n];
        let mut queue: VecDeque<(usize, usize, usize)> = VecDeque::from([(0, 0, 0)]);
        exterior[0] = true;
        while let Some((x, y, l)) = queue.pop_front() {
            let near = [
                (x.wrapping_sub(1), y, l),
                (x + 1, y, l),
                (x, y.wrapping_sub(1), l),
                (x, y + 1, l),
                (x, y, l.wrapping_sub(1)),
                (x, y, l + 1),
            ];
            for (a, b, c) in near {
                if a < w && b < h && c < n && !exterior[index(a, b, c)] && !solid(a, b, c) {
                    exterior[index(a, b, c)] = true;
                    queue.push_back((a, b, c));
                }
            }
        }
        let mut cavity = Vec::new();
        let mut outside = Vec::new();
        for l in 0..n {
            for y in 0..h {
                for x in 0..w {
                    let p = [position(x, PITCH[0]), position(y, PITCH[1]), z(l)];
                    if exterior[index(x, y, l)] {
                        outside.push(p);
                    } else if !solid(x, y, l) {
                        cavity.push(p);
                    }
                }
            }
        }
        let mut best = f64::INFINITY;
        for c in &cavity {
            for e in &outside {
                let d = (0..3).map(|k| (c[k] - e[k]).powi(2)).sum::<f64>();
                best = best.min(d);
            }
        }
        (cavity.len() as u64, best.sqrt())
    }

    /// A box with a void; walls of 2 columns in X, 3 rows in Y and 3 layers in Z.
    fn boxed(x: u32, y: u32, l: usize) -> u8 {
        let outer = (3..21).contains(&x) && (3..17).contains(&y) && (2..14).contains(&l);
        let void = (5..19).contains(&x) && (6..14).contains(&y) && (5..11).contains(&l);
        if outer && !void {
            255
        } else {
            0
        }
    }

    #[test]
    fn a_boxed_void_has_its_walls_measured_and_is_an_undrained_pocket() {
        let print = stack(24, 20, 16, boxed);
        let layers = layers(&print, None, None, &[0.05; 16]);
        let report = verify(&layers, 0.3);
        let (cavity, distance) = brute(&layers);
        assert_eq!(report.cavity_voxels, cavity);
        let thinnest = report.thinnest.unwrap();
        assert!(
            (thinnest.distance_mm - distance).abs() < 1e-12,
            "{thinnest:?} {distance}"
        );
        // The thinnest wall is two columns: 3 pitches between centres, less one.
        assert!((thinnest.distance_mm - 3.0 * PITCH[0]).abs() < 1e-12);
        assert!(
            (thinnest.measured_mm - 2.0 * PITCH[0]).abs() < 1e-12,
            "{thinnest:?}"
        );
        assert!(thinnest.guaranteed_mm < thinnest.measured_mm);
        assert_eq!(thinnest.at.pixel[0], 5);
        assert_eq!(report.pockets.len(), 1);
        assert_eq!(report.pockets[0].voxels, 14 * 8 * 6);
        assert_eq!(report.pockets[0].layers, [5, 10]);
        assert_eq!(report.breaches, Breaches::default());
        // Every cavity voxel next to the X walls is under a 0.3 mm wall.
        assert!(report.thin_voxels > 0);
        assert!(report.marks.iter().any(|m| !m.thin.is_empty()));
    }

    #[test]
    fn a_floor_on_the_first_layer_is_measured_against_the_plate() {
        // One layer of floor, then the void: the nearest exterior is below the first layer.
        let print = stack(30, 30, 10, |x, y, l| {
            let outer = (2..28).contains(&x) && (2..28).contains(&y) && l < 8;
            let void = (8..22).contains(&x) && (8..22).contains(&y) && (1..6).contains(&l);
            if outer && !void {
                255
            } else {
                0
            }
        });
        let layers = layers(&print, None, None, &[0.02; 10]);
        let report = verify(&layers, 0.3);
        let (_, distance) = brute(&layers);
        let thinnest = report.thinnest.unwrap();
        assert!(
            (thinnest.distance_mm - distance).abs() < 1e-12,
            "{thinnest:?} {distance}"
        );
        assert!((thinnest.distance_mm - 0.04).abs() < 1e-12);
        assert!((thinnest.measured_mm - 0.02).abs() < 1e-12);
        assert_eq!(thinnest.at.layer, 1);
    }

    #[test]
    fn wide_layers_are_measured_across_column_blocks() {
        // 140 columns: two full blocks of the column pass and a partial one. A long void whose
        // thinnest wall is at the far end, beyond the first block.
        let print = stack(140, 14, 9, |x, y, l| {
            let outer = (2..138).contains(&x) && (2..12).contains(&y) && (1..8).contains(&l);
            let void = (5..136).contains(&x) && (5..9).contains(&y) && (3..6).contains(&l);
            if outer && !void {
                255
            } else {
                0
            }
        });
        let layers = layers(&print, None, None, &[0.05; 9]);
        let report = verify(&layers, 0.3);
        let (cavity, distance) = brute(&layers);
        assert_eq!(report.cavity_voxels, cavity);
        let thinnest = report.thinnest.unwrap();
        assert!(
            (thinnest.distance_mm - distance).abs() < 1e-12,
            "{thinnest:?} {distance}"
        );
        assert_eq!(thinnest.at.pixel[0], 135);
    }

    #[test]
    fn a_pocket_is_shown_where_it_is_widest() {
        // A void widening to layer 6 and narrowing again, beside a one-voxel void at layer 3.
        let print = stack(30, 20, 12, |x, y, l| {
            let outer = (2..28).contains(&x) && (2..18).contains(&y) && (1..11).contains(&l);
            let half = 6 - u32::try_from(l.abs_diff(6)).unwrap();
            let void = (3..10).contains(&l)
                && (14 - half..14 + half + 1).contains(&x)
                && (8..12).contains(&y);
            let speck = l == 3 && x == 5 && y == 5;
            if outer && !void && !speck {
                255
            } else {
                0
            }
        });
        let layers = layers(&print, None, None, &[0.05; 12]);
        let report = verify(&layers, 0.3);
        assert_eq!(report.pockets.len(), 2);
        let (speck, void) = (&report.pockets[0], &report.pockets[1]);
        assert_eq!(
            (speck.voxels, speck.widest.pixel, speck.widest.layer),
            (1, [5, 5], 3)
        );
        assert_eq!(void.first.layer, 3);
        assert_eq!(void.widest.layer, 6);
        assert_eq!(void.widest.pixel, [14, 8]);
        // Each pocket's runs carry its index.
        assert!(report.marks[3].pocket.iter().any(|m| m[3] == 0));
        assert!(report.marks[6].pocket.iter().all(|m| m[3] == 1));
    }

    #[test]
    fn walls_thicker_than_the_request_are_not_reported_as_thin() {
        let print = stack(24, 20, 16, boxed);
        let layers = layers(&print, None, None, &[0.05; 16]);
        let report = verify(&layers, 0.02);
        assert_eq!(report.thin_voxels, 0);
        // Still within reach of the requested wall and the voxel diagonal.
        assert!(report.thinnest.is_some());
    }

    #[test]
    fn a_slit_breaches_the_expected_cavity_and_a_drain_hole_does_not() {
        let void = |x: u32, y: u32, l: usize| {
            if (5..19).contains(&x) && (6..14).contains(&y) && (5..11).contains(&l) {
                255
            } else {
                0
            }
        };
        let cavity = stack(24, 20, 16, void);
        // A slit through the -X wall at layer 7.
        let slit = |x: u32, y: u32, l: usize| {
            if l == 7 && (9..11).contains(&y) && (3..5).contains(&x) {
                0
            } else {
                boxed(x, y, l)
            }
        };
        let print = stack(24, 20, 16, slit);
        let breached = layers(&print, None, Some(&cavity), &[0.05; 16]);
        let report = verify(&breached, 0.3);
        assert_eq!(report.breaches.voxels, 14 * 8 * 6);
        assert_eq!(report.breaches.first.unwrap().layer, 5);
        assert_eq!(report.pockets, Vec::new());
        assert_ne!(report.marks[7].breach, Vec::<[u32; 3]>::new());
        // The same opening as a drain hole: no breach, no pocket.
        let hole = |x: u32, y: u32, l: usize| {
            if l == 7 && (9..11).contains(&y) && (3..5).contains(&x) {
                255
            } else {
                0
            }
        };
        let holes = stack(24, 20, 16, hole);
        let drained = layers(&print, Some(&holes), Some(&cavity), &[0.05; 16]);
        let report = verify(&drained, 0.3);
        assert_eq!(report.breaches, Breaches::default());
        assert_eq!(report.pockets, Vec::new());
        let plain = stack(24, 20, 16, boxed);
        let closed = layers(&plain, None, None, &[0.05; 16]);
        assert_eq!(report.thinnest, verify(&closed, 0.3).thinnest);
    }

    /// A deterministic pseudo-random hollow solid on the lattice of `heights`: an ellipsoidal or
    /// box shell around an off-centre void (oblique walls of varying thickness), with a pillar
    /// across the void. Gray 200 is material, 100 a pixel under the threshold.
    fn shell(seed: u64, heights: &[f64]) -> impl Fn(u32, u32, usize) -> u8 {
        let mut state = seed;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            f64::from(u32::try_from(state % 10_000).unwrap()) / 10_000.0
        };
        let z: Vec<f64> = heights
            .iter()
            .scan(0.0, |top, h| {
                let centre = *top + h / 2.0;
                *top += h;
                Some(centre)
            })
            .collect();
        let centre = [13.0 * PITCH[0], 11.0 * PITCH[1], z[z.len() / 2]];
        let outer = [
            (6.0 + 5.0 * next()) * PITCH[0],
            (4.0 + 4.0 * next()) * PITCH[1],
            z[z.len() - 3] - centre[2] - 0.6 * next() * (z[1] - z[0]),
        ];
        let shrink = [0.4 + 0.3 * next(), 0.4 + 0.3 * next(), 0.4 + 0.3 * next()];
        let shift = [next() - 0.5, next() - 0.5, next() - 0.5];
        let inner: [f64; 3] = std::array::from_fn(|k| outer[k] * shrink[k]);
        let offset: [f64; 3] = std::array::from_fn(|k| shift[k] * (outer[k] - inner[k]) * 0.8);
        let boxy = next() < 0.4;
        let pillar = next() < 0.5;
        move |x, y, l| {
            let p = [f64::from(x) * PITCH[0], f64::from(y) * PITCH[1], z[l]];
            let norm = |c: [f64; 3], r: [f64; 3]| {
                let q: [f64; 3] = std::array::from_fn(|k| (p[k] - c[k]) / r[k]);
                if boxy {
                    q.iter().fold(0.0f64, |m, v| m.max(v.abs()))
                } else {
                    q.iter().map(|v| v * v).sum::<f64>()
                }
            };
            let void_centre: [f64; 3] = std::array::from_fn(|k| centre[k] + offset[k]);
            let in_pillar = pillar && (p[0] - void_centre[0]).abs() < 1.5 * PITCH[0];
            let solid = norm(centre, outer) <= 1.0;
            let void = norm(void_centre, inner) <= 1.0 && !in_pillar;
            match (solid, void) {
                (true, false) => 200,
                (true, true) => 0,
                _ if (x + y) % 7 == 0 => 100,
                _ => 0,
            }
        }
    }

    #[test]
    fn the_smallest_distance_is_the_brute_force_one_on_random_solids() {
        let mut measured = 0;
        for seed in 1..=12u64 {
            // Varied layer heights.
            let heights: Vec<f64> = (0..18).map(|l| [0.05, 0.03, 0.08][l % 3]).collect();
            let print = stack(26, 22, 18, shell(seed * 7919, &heights));
            let layers = layers(&print, None, None, &heights);
            let report = verify(&layers, 0.4);
            let (cavity, distance) = brute(&layers);
            assert_eq!(report.cavity_voxels, cavity, "seed {seed}");
            match report.thinnest {
                Some(t) => {
                    measured += 1;
                    assert!(
                        (t.distance_mm - distance).abs() < 1e-9,
                        "seed {seed}: {t:?} {distance}"
                    );
                    assert!(t.measured_mm <= t.distance_mm && t.guaranteed_mm < t.measured_mm);
                }
                None => assert!(cavity == 0 || distance >= 0.4, "seed {seed}: {distance}"),
            }
        }
        // Most solids enclose voids, so the comparison is not vacuous.
        assert!(measured >= 6, "{measured} solids with a cavity");
    }

    #[test]
    fn the_material_threshold_decides_antialiased_pixels() {
        // Gray 100 is under the threshold: the box's walls are only its 200 pixels.
        let heights = [0.05; 18];
        let print = stack(26, 22, 18, shell(5, &heights));
        let layers = layers(&print, None, None, &heights);
        let report = verify(&layers, 0.4);
        assert!(report.thinnest.is_some());
        let gray = shell(5, &heights);
        let binary = stack(
            26,
            22,
            18,
            |x, y, l| if gray(x, y, l) >= 128 { 255 } else { 0 },
        );
        let layers = super::tests::layers(&binary, None, None, &heights);
        assert_eq!(report, verify(&layers, 0.4));
    }

    #[test]
    fn invalid_layers_and_parameters_are_refused() {
        let print = stack(8, 8, 2, |_, _, _| 255);
        let mut layers = layers(&print, None, None, &[0.05; 2]);
        layers[1].z_mm = layers[0].z_mm;
        let input = VerifyInput {
            layers: &layers,
            pitch_mm: PITCH,
            material_gray: 128,
            wall_mm: 1.0,
        };
        let mut never = Cancellation::never();
        assert_eq!(
            verify_walls(&input, VerifyLimits::default(), &mut never),
            Err(VerifyError::Layer { layer: 1 })
        );
        let bad = VerifyInput {
            wall_mm: 0.0,
            ..input
        };
        assert_eq!(
            verify_walls(&bad, VerifyLimits::default(), &mut never),
            Err(VerifyError::Parameters)
        );
    }

    #[test]
    fn a_stack_without_material_has_no_findings() {
        let print = stack(8, 8, 3, |_, _, _| 0);
        let layers = layers(&print, None, None, &[0.05; 3]);
        let report = verify(&layers, 1.0);
        assert_eq!(report.thinnest, None);
        assert_eq!(report.cavity_voxels, 0);
    }
}

#[cfg(test)]
mod budget {
    use super::*;
    use std::time::Instant;

    /// Time and findings on an ellipsoidal shell at print scale (P1 pitch, 0.05 mm layers):
    /// `cargo test --release -p karasu-core --lib verify::budget -- --ignored --nocapture`.
    #[test]
    #[ignore = "measures the verifier at print scale"]
    #[allow(clippy::many_single_char_names)] // Lattice sizes and coordinates.
    fn an_ellipsoidal_shell_at_print_scale() {
        let (w, h, n) = (4000u32, 1300u32, 1000usize);
        let pitch = [0.0168, 0.0247];
        let centre = [
            f64::from(w) * pitch[0] / 2.0,
            f64::from(h) * pitch[1] / 2.0,
            25.0,
        ];
        let outer = [30.0, 14.0, 23.0];
        let wall = 1.3;
        let started = Instant::now();
        let masks: Vec<SpanMask> = (0..n)
            .map(|l| {
                let z = (f64::from(u32::try_from(l).unwrap()) + 0.5) * 0.05;
                let mut pixels = vec![0u8; (w * h) as usize];
                for y in 0..h {
                    for x in 0..w {
                        let p = [f64::from(x) * pitch[0], f64::from(y) * pitch[1], z];
                        let q = |r: [f64; 3]| {
                            (0..3)
                                .map(|k| ((p[k] - centre[k]) / r[k]).powi(2))
                                .sum::<f64>()
                        };
                        let inner = outer.map(|r| r - wall);
                        if q(outer) <= 1.0 && q(inner) > 1.0 {
                            pixels[(y * w + x) as usize] = 255;
                        }
                    }
                }
                SpanMask::from_pixels(w, h, &pixels).unwrap()
            })
            .collect();
        println!("masks built in {:.1} s", started.elapsed().as_secs_f64());
        let layers: Vec<VerifyLayer<'_>> = masks
            .iter()
            .enumerate()
            .map(|(l, print)| VerifyLayer {
                z_mm: (f64::from(u32::try_from(l).unwrap()) + 0.5) * 0.05,
                height_mm: 0.05,
                print,
                holes: None,
                cavity: None,
            })
            .collect();
        let started = Instant::now();
        let report = verify_walls(
            &VerifyInput {
                layers: &layers,
                pitch_mm: pitch,
                material_gray: 128,
                wall_mm: wall,
            },
            VerifyLimits::default(),
            &mut Cancellation::never(),
        )
        .unwrap();
        println!(
            "verified in {:.1} s: thinnest {:?}, thin {}, cavity {}, pockets {}",
            started.elapsed().as_secs_f64(),
            report.thinnest,
            report.thin_voxels,
            report.cavity_voxels,
            report.pockets.len()
        );
    }
}
