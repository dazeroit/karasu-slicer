//! Suction cups of a print, found on its layer masks (TD-114).
//!
//! **Definition.** The print grows from the plate, one layer at a time, and each layer is peeled
//! from the vat's film. While layer `k` is printed only layers `0..=k` exist. An empty region of
//! layer `k` (resin, as the masks see it) is a **suction cup** at layer `k` when, through layers
//! `0..=k`, it does not reach the outside: material surrounds it in its layer and caps it
//! towards the plate, and the plate caps the first layer. Peeling layer `k` then pulls on the
//! resin it encloses. A cup is open towards the film at every layer it spans; it ends where it
//! reaches the outside (through a vent, or past the rim of an open shape) or where the layers
//! close over it, leaving its resin trapped.
//!
//! **Vents.** An opening narrower than a vent does not relieve the cup. An empty passage counts
//! only where a circle of the minimum vent area fits through it, less one pixel all round
//! for the rasterization: the empty pixels are eroded by that circle's radius and joined again.
//! A cup whose openings are all narrower is reported with [`Opening::TooSmall`]; one with no
//! opening at all with [`Opening::None`]. A minimum vent area under a pixel's erodes nothing:
//! any opening of a whole empty pixel vents.
//!
//! **Pixels.** A pixel of layer `k` is material when its gray reaches `material_gray`; the
//! caller passes 1 so that a pixel cured at all seals, and no opening is credited that the
//! printer would not leave open.
//!
//! **Connectivity.** Empty pixels join by an edge in their layer, the dual of the material's
//! corners (TD-107), and the same pixel of consecutive layers joins vertically. Empty pixels on
//! the masks' border are the outside.
//!
//! **Cost.** Work follows the masks' runs; two layers are held at a time, and the union-find
//! holds one node per empty component of each layer. The report is the same for equal inputs:
//! layers are read in order, components are numbered by their first run, and cups are sorted.
use super::runs::{disc, reach_row, Runs, Union};
use crate::{
    cancellation::{Cancellation, Cancelled, Checkpoint},
    raster::mask::SpanMask,
};

/// One layer of the print, from the plate up.
#[derive(Clone, Copy, Debug)]
pub struct CupLayer<'a> {
    /// Height of the layer's plane, in mm.
    pub z_mm: f64,
    /// The layer's slab height, in mm, for the cups' volumes.
    pub height_mm: f64,
    /// The printed mask.
    pub mask: &'a SpanMask,
}

/// What to analyse.
#[derive(Clone, Copy, Debug)]
pub struct CupInput<'a> {
    /// The layers, with strictly increasing planes.
    pub layers: &'a [CupLayer<'a>],
    /// Pixel pitch along the masks' columns and rows, in mm.
    pub pitch_mm: [f64; 2],
    /// Smallest gray of a material pixel, at least 1.
    pub material_gray: u8,
    /// The smallest opening that vents a cup, in mm² (the owner's setting): the area of the
    /// circle that must fit through it. Zero or more.
    pub min_vent_mm2: f64,
}

/// How a cup is open to the outside.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Opening {
    /// No opening at all.
    None,
    /// Openings, all narrower than the minimum vent.
    TooSmall,
}

/// Where a separately capped part of a cup starts: its lowest point, a vent's place (owner,
/// 2026-10-31). A cavity with several lowest points, such as the hollow legs and arms of a
/// figure turned over, is one cup once its parts meet above, yet each part pulls on its own
/// resin from its cap up to where it meets an older part, and needs its own vent.
#[derive(Clone, Debug, PartialEq)]
pub struct CupCap {
    /// The layer it starts at.
    pub layer: usize,
    /// Its last layer as a part of its own: the layer before it meets an older part, or the
    /// cup's last layer for the oldest.
    pub last_layer: usize,
    /// The centroid of its first layer's pixels, in pixel coordinates (column, row; a pixel's
    /// centre is at half units).
    pub cap: [f64; 2],
    /// Its first layer's bounds in pixels: first column and row, one past the last.
    pub bounds: [u32; 4],
    /// Its pixels at its widest layer, as a part of its own.
    pub largest_pixels: u64,
    /// Its volume as a part of its own, in mm³.
    pub volume_mm3: f64,
    /// The first layer where a circle of the minimum vent fits in it: where a vent opens into
    /// it wide enough (its first layer without a minimum vent, or when none ever fits). Its
    /// lowest point may be a narrow tip a vent there would open into only through a passage too
    /// small.
    pub vent_layer: usize,
    /// The centroid of where the circle fits at that layer, in pixel coordinates.
    pub vent: [f64; 2],
}

/// A suction cup: the empty regions, layer after layer, that enclose the same resin.
#[derive(Clone, Debug, PartialEq)]
pub struct SuctionCup {
    /// The first layer it is a cup at: the one on its cap.
    pub first_layer: usize,
    /// The last layer it is a cup at.
    pub last_layer: usize,
    /// Its openings.
    pub opening: Opening,
    /// Whether the layers close over it with its resin trapped, rather than it reaching the
    /// outside (or the print ending).
    pub closes: bool,
    /// The layer it is widest at.
    pub largest_layer: usize,
    /// Its pixels at that layer.
    pub largest_pixels: u64,
    /// Its pixels, over its layers.
    pub pixels: u64,
    /// Its volume, in mm³: each layer's pixels by the pixel's area and the layer's height.
    pub volume_mm3: f64,
    /// The centroid of its first layer's pixels, in pixel coordinates (column, row; a pixel's
    /// centre is at half units): where it is capped, and a vent would relieve it.
    pub cap: [f64; 2],
    /// Its bounds in pixels over its layers: first column and row, one past the last.
    pub bounds: [u32; 4],
    /// Where each of its separately capped parts starts, oldest first (by layer, then column,
    /// then row): the first is the cup's own cap.
    pub caps: Vec<CupCap>,
}

/// The analysis's result.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CupReport {
    /// The cups, by first layer, then cap column, then cap row.
    pub cups: Vec<SuctionCup>,
}

/// Why the analysis could not run.
#[derive(Clone, Debug, PartialEq)]
pub enum CupError {
    /// The layers' masks differ in size, or their planes do not increase.
    InconsistentLayers,
    /// A parameter is out of range (the pitch, a height, the gray or the vent area).
    InvalidParameter(&'static str),
    /// More cup pieces than `limit` were found.
    TooManyPieces {
        /// The limit reached.
        limit: usize,
    },
    /// The work was cancelled.
    Cancelled(Cancelled),
}

impl std::fmt::Display for CupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InconsistentLayers => {
                write!(
                    f,
                    "the layers differ in size or their planes do not increase"
                )
            }
            Self::InvalidParameter(what) => write!(f, "invalid {what}"),
            Self::TooManyPieces { limit } => {
                write!(f, "more than {limit} suction-cup pieces were found")
            }
            Self::Cancelled(c) => write!(f, "{c}"),
        }
    }
}

impl std::error::Error for CupError {}

impl From<Cancelled> for CupError {
    fn from(c: Cancelled) -> Self {
        Self::Cancelled(c)
    }
}

/// Cup pieces held at most (one per layer of each cup): each costs a few dozen bytes.
pub const MAX_PIECES: usize = 50_000_000;

/// The node of the outside in each sweep's union.
const OUTSIDE: usize = 0;

/// One layer's empty runs, labelled.
#[derive(Default)]
struct Labelled {
    runs: Runs,
    /// Each run's component, numbered by first run.
    component: Vec<usize>,
    /// Each component's first run.
    first: Vec<usize>,
    /// Each component's node in the sweep's union.
    node: Vec<usize>,
    /// Each component's cup piece, when it is (in) one.
    piece: Vec<Option<usize>>,
}

impl Labelled {
    /// Each run with its row and index, row by row.
    fn runs(&self) -> impl Iterator<Item = (usize, usize, [u32; 2])> + '_ {
        (0..self.runs.height()).flat_map(move |y| {
            let base = self.runs.rows[y];
            self.runs
                .row(y)
                .iter()
                .enumerate()
                .map(move |(i, &run)| (y, base + i, run))
        })
    }
}

/// The empty regions of the layers so far, joined across layers.
struct Sweep {
    union: Union,
    below: Option<Labelled>,
}

impl Sweep {
    fn new() -> Self {
        let mut union = Union::default();
        union.add();
        Self { union, below: None }
    }

    /// Whether `node` reaches the outside.
    fn outside(&mut self, node: usize) -> bool {
        self.union.find(node) == self.union.find(OUTSIDE)
    }

    /// Label `empty` (a layer of `width` columns) and join it to the layer under it. Returns
    /// the layer and the pairs of its components and the layer under's that touch.
    fn add(&mut self, empty: Runs, width: u32) -> (Labelled, Vec<(usize, usize)>) {
        let height = empty.height();
        let mut local = Union::default();
        for y in 0..height {
            let base = empty.rows[y];
            for i in 0..empty.row(y).len() {
                local.add();
                if y > 0 {
                    let above = empty.rows[y - 1];
                    for (j, _) in overlaps(empty.row(y)[i], empty.row(y - 1)) {
                        local.join(base + i, above + j);
                    }
                }
            }
        }
        // Components numbered by their first run: the union's roots are the lowest index.
        let mut component = vec![0; empty.runs.len()];
        let mut number = vec![usize::MAX; empty.runs.len()];
        let (mut node, mut first) = (Vec::new(), Vec::new());
        for (i, slot) in component.iter_mut().enumerate() {
            let root = local.find(i);
            if number[root] == usize::MAX {
                number[root] = node.len();
                node.push(self.union.add());
                first.push(i);
            }
            *slot = number[root];
        }
        for y in 0..height {
            let edge_row = y == 0 || y + 1 == height;
            for (i, run) in empty.row(y).iter().enumerate() {
                if edge_row || run[0] == 0 || run[1] == width {
                    let n = node[component[empty.rows[y] + i]];
                    self.union.join(n, OUTSIDE);
                }
            }
        }
        let mut touching = Vec::new();
        if let Some(below) = &self.below {
            for y in 0..height {
                for (i, &run) in empty.row(y).iter().enumerate() {
                    let here = component[empty.rows[y] + i];
                    for (j, _) in overlaps(run, below.runs.row(y)) {
                        let there = below.component[below.runs.rows[y] + j];
                        self.union.join(node[here], below.node[there]);
                        touching.push((here, there));
                    }
                }
            }
        }
        touching.sort_unstable();
        touching.dedup();
        let components = node.len();
        (
            Labelled {
                runs: empty,
                component,
                first,
                node,
                piece: vec![None; components],
            },
            touching,
        )
    }
}

/// The runs of `row` overlapping `run` by at least one column, with their indices.
fn overlaps(run: [u32; 2], row: &[[u32; 2]]) -> impl Iterator<Item = (usize, [u32; 2])> + '_ {
    let first = row.partition_point(|r| r[1] <= run[0]);
    row[first..]
        .iter()
        .take_while(move |r| r[0] < run[1])
        .enumerate()
        .map(move |(k, &r)| (first + k, r))
}

/// The complement of each row of `runs` within `width` columns.
fn complement(runs: &Runs, width: u32) -> Runs {
    let mut out = Runs {
        rows: Vec::with_capacity(runs.rows.len()),
        runs: Vec::new(),
    };
    out.rows.push(0);
    for y in 0..runs.height() {
        let mut at = 0;
        for &[a, b] in runs.row(y) {
            if a > at {
                out.runs.push([at, a]);
            }
            at = b;
        }
        if at < width {
            out.runs.push([at, width]);
        }
        out.rows.push(out.runs.len());
    }
    out
}

/// `material` grown by the disc `half` within `width` columns.
fn grown(material: &Runs, half: &[u32], width: u32) -> Runs {
    let mut out = Runs {
        rows: Vec::with_capacity(material.rows.len()),
        runs: Vec::new(),
    };
    out.rows.push(0);
    let mut row = Vec::new();
    for y in 0..material.height() {
        reach_row(material, y, half, width, &mut row);
        out.runs.extend_from_slice(&row);
        out.rows.push(out.runs.len());
    }
    out
}

/// One layer's part of a cup.
#[derive(Clone, Debug)]
struct Piece {
    layer: usize,
    /// Found on the eroded empty pixels: the cup's openings are too small.
    eroded: bool,
    /// Its node in its sweep's union.
    node: usize,
    pixels: u64,
    sum: [f64; 2],
    bounds: [u32; 4],
    /// The pixels of a piece of the whole sweep where a vent's circle fits (its eroded empty
    /// pixels inside it), and their sum.
    wide: u64,
    wide_sum: [f64; 2],
}

impl Piece {
    fn new(layer: usize, eroded: bool, node: usize) -> Self {
        Self {
            layer,
            eroded,
            node,
            pixels: 0,
            sum: [0.0; 2],
            bounds: [u32::MAX, u32::MAX, 0, 0],
            wide: 0,
            wide_sum: [0.0; 2],
        }
    }

    /// Where a vent's circle fits in it, when anywhere: its pixels and their sum. A piece of
    /// the eroded sweep fits it throughout, as does every piece when `eroding` is off.
    fn wide(&self, eroding: bool) -> (u64, [f64; 2]) {
        if self.eroded || !eroding {
            (self.pixels, self.sum)
        } else {
            (self.wide, self.wide_sum)
        }
    }

    fn add(&mut self, y: usize, run: [u32; 2]) {
        let count = u64::from(run[1] - run[0]);
        self.pixels += count;
        #[allow(clippy::cast_precision_loss)] // Pixel counts and coordinates.
        {
            let middle = f64::midpoint(f64::from(run[0]), f64::from(run[1]));
            self.sum[0] += middle * count as f64;
            self.sum[1] += (y as f64 + 0.5) * count as f64;
        }
        #[allow(clippy::cast_possible_truncation)] // Rows fit the mask's u32 height.
        let row = y as u32;
        self.bounds = [
            self.bounds[0].min(run[0]),
            self.bounds[1].min(row),
            self.bounds[2].max(run[1]),
            self.bounds[3].max(row + 1),
        ];
    }
}

/// A separately capped part of a cup while it grows: its first piece, its last layer as a part
/// of its own, and its size meanwhile.
#[derive(Clone, Debug)]
struct Chamber {
    piece: usize,
    last: usize,
    largest_pixels: u64,
    volume_mm3: f64,
    /// The first layer a vent's circle fits in it, and the centroid of where.
    vent: Option<(usize, [f64; 2])>,
}

/// The cup pieces found so far, their cups and their parts.
#[derive(Default)]
struct Analysis {
    pieces: Vec<Piece>,
    cups: Union,
    /// Each piece's part, by its chamber, and the chambers.
    chamber_of: Vec<usize>,
    chambers: Vec<Chamber>,
    /// The parts that met, the oldest (the lowest index) kept as their root.
    parts: Union,
    /// The layer's joins between pieces: a piece and one under it or beside it.
    joins: Vec<(usize, usize)>,
}

impl Analysis {
    fn add(&mut self, piece: Piece) -> Result<usize, CupError> {
        if self.pieces.len() >= MAX_PIECES {
            return Err(CupError::TooManyPieces { limit: MAX_PIECES });
        }
        self.pieces.push(piece);
        self.chamber_of.push(usize::MAX);
        Ok(self.cups.add())
    }

    /// Give the pieces of layer `k` (from `first` on) their parts: a piece joined to none under
    /// it starts a part; parts that meet go on as the oldest. Then each part's size grows by its
    /// pieces at this layer, `height_mm` high with pixels of `pixel_mm2`.
    fn chambers(
        &mut self,
        k: usize,
        first: usize,
        (height_mm, pixel_mm2): (f64, f64),
        eroding: bool,
    ) {
        let joins = std::mem::take(&mut self.joins);
        for p in first..self.pieces.len() {
            let mut under: Vec<usize> = joins
                .iter()
                .filter_map(|&(a, b)| {
                    let other = if a == p {
                        b
                    } else if b == p {
                        a
                    } else {
                        return None;
                    };
                    (self.pieces[other].layer + 1 == k).then(|| self.chamber_of[other])
                })
                .map(|c| self.parts.find(c))
                .collect();
            under.sort_unstable();
            under.dedup();
            let Some(&oldest) = under.first() else {
                self.chamber_of[p] = self.chambers.len();
                self.chambers.push(Chamber {
                    piece: p,
                    last: k,
                    largest_pixels: 0,
                    volume_mm3: 0.0,
                    vent: None,
                });
                self.parts.add();
                continue;
            };
            self.chamber_of[p] = oldest;
            for &younger in &under[1..] {
                self.parts.join(oldest, younger);
            }
        }
        // Pieces of this layer joined beside each other (through layers under them) are one part.
        for &(a, b) in &joins {
            if self.pieces[a].layer == k && self.pieces[b].layer == k {
                let (ca, cb) = (self.chamber_of[a], self.chamber_of[b]);
                self.parts.join(ca, cb);
            }
        }
        let mut grown: std::collections::BTreeMap<usize, (u64, u64, [f64; 2])> =
            std::collections::BTreeMap::new();
        for p in first..self.pieces.len() {
            let part = self.parts.find(self.chamber_of[p]);
            self.chamber_of[p] = part;
            let (wide, sum) = self.pieces[p].wide(eroding);
            let entry = grown.entry(part).or_default();
            entry.0 += self.pieces[p].pixels;
            entry.1 += wide;
            entry.2 = [entry.2[0] + sum[0], entry.2[1] + sum[1]];
        }
        // Each part still growing takes this layer: its last layer, its widest and its volume,
        // and where a vent first fits in it.
        for (part, (pixels, wide, sum)) in grown {
            let chamber = &mut self.chambers[part];
            if chamber.vent.is_none() && wide > 0 {
                #[allow(clippy::cast_precision_loss)] // Pixel counts.
                let n = wide as f64;
                chamber.vent = Some((k, [sum[0] / n, sum[1] / n]));
            }
            chamber.last = k;
            chamber.largest_pixels = chamber.largest_pixels.max(pixels);
            #[allow(clippy::cast_precision_loss)] // Pixel counts.
            let added = pixels as f64 * pixel_mm2 * height_mm;
            chamber.volume_mm3 += added;
        }
    }

    /// Mark the components of `layer` (layer `k` of `sweep`) that do not reach the outside as
    /// pieces, unless `covered` already gives them one, and join the pieces of one cup: those
    /// sharing a node, and those over `touching` pieces of the layer under.
    fn mark(
        &mut self,
        (k, eroded): (usize, bool),
        sweep: &mut Sweep,
        layer: &mut Labelled,
        touching: &[(usize, usize)],
        covered: &dyn Fn(&Labelled, usize) -> Option<usize>,
    ) -> Result<(), CupError> {
        let mut by_root: Vec<(usize, usize)> = Vec::new();
        for c in 0..layer.node.len() {
            if sweep.outside(layer.node[c]) {
                continue;
            }
            let piece = match covered(layer, c) {
                Some(piece) => piece,
                None => self.add(Piece::new(k, eroded, layer.node[c]))?,
            };
            layer.piece[c] = Some(piece);
            by_root.push((sweep.union.find(layer.node[c]), piece));
        }
        by_root.sort_unstable();
        for pair in by_root.windows(2) {
            if pair[0].0 == pair[1].0 && pair[0].1 != pair[1].1 {
                self.cups.join(pair[0].1, pair[1].1);
                self.joins.push((pair[0].1, pair[1].1));
            }
        }
        if let Some(below) = &sweep.below {
            for &(here, there) in touching {
                if let (Some(a), Some(b)) = (layer.piece[here], below.piece[there]) {
                    self.cups.join(a, b);
                    self.joins.push((a, b));
                }
            }
        }
        Ok(())
    }
}

/// The eroding disc for `min_vent_mm2` (the circle's radius less one pixel), or `None` when it
/// erodes nothing.
fn vent_disc(min_vent_mm2: f64, pitch: [f64; 2]) -> Option<Vec<u32>> {
    let radius = (min_vent_mm2 / std::f64::consts::PI).sqrt() - pitch[0].max(pitch[1]);
    let half = disc(radius.max(0.0), pitch);
    (half != [0]).then_some(half)
}

/// Find the suction cups of `input` (TD-114).
///
/// # Errors
/// [`CupError`] for inconsistent layers or parameters, too many pieces, or cancellation.
pub fn find_suction_cups(
    input: &CupInput<'_>,
    cancel: &mut Cancellation<'_>,
) -> Result<CupReport, CupError> {
    validate(input)?;
    let vent = vent_disc(input.min_vent_mm2, input.pitch_mm);
    let mut found = Analysis::default();
    let mut full = Sweep::new();
    let mut eroded = Sweep::new();
    // Each layer's empty pixels, whole and eroded, are found on every core a batch at a time;
    // the sweeps then join the layers in order.
    let batch = std::thread::available_parallelism().map_or(1, usize::from) * 2;
    let mut empty: Vec<(Runs, Option<Runs>)> = Vec::new();
    for (k, layer) in input.layers.iter().enumerate() {
        cancel.check(Checkpoint::SuctionLayer)?;
        if k % batch == 0 {
            let layers = &input.layers[k..(k + batch).min(input.layers.len())];
            empty.clear();
            empty.resize_with(layers.len(), Default::default);
            crate::parallel::fill_costly_with(&mut empty, |i| {
                let mask = layers[i].mask;
                let material = Runs::of(mask, input.material_gray);
                let open = vent
                    .as_ref()
                    .map(|half| complement(&grown(&material, half, mask.width()), mask.width()));
                (complement(&material, mask.width()), open)
            });
            empty.reverse();
        }
        let (whole, open) = empty.pop().unwrap_or_default();
        let width = layer.mask.width();
        let first = found.pieces.len();
        let (mut here, touching) = full.add(whole, width);
        found.mark((k, false), &mut full, &mut here, &touching, &|_, _| None)?;
        for (y, i, run) in here.runs() {
            if let Some(piece) = here.piece[here.component[i]] {
                found.pieces[piece].add(y, run);
            }
        }
        if let (Some(half), Some(open)) = (&vent, open) {
            let (mut narrow, touching) = eroded.add(open, width);
            // An eroded region inside a cup of the whole empty pixels is part of that cup.
            let covered = |narrow: &Labelled, c: usize| -> Option<usize> {
                let i = narrow.first[c];
                let y = narrow.runs.rows.partition_point(|&r| r <= i) - 1;
                let run = narrow.runs.runs[i];
                let row = here.runs.row(y);
                let j = row.partition_point(|r| r[1] <= run[0]);
                here.piece[here.component[here.runs.rows[y] + j]]
            };
            let before = found.pieces.len();
            found.mark((k, true), &mut eroded, &mut narrow, &touching, &covered)?;
            // Where a vent's circle fits in the pieces of the whole sweep.
            for (y, i, run) in narrow.runs() {
                if let Some(piece) = narrow.piece[narrow.component[i]].filter(|&p| p < before) {
                    let piece = &mut found.pieces[piece];
                    let count = u64::from(run[1] - run[0]);
                    piece.wide += count;
                    #[allow(clippy::cast_precision_loss)] // Pixel counts and coordinates.
                    {
                        let middle = f64::midpoint(f64::from(run[0]), f64::from(run[1]));
                        piece.wide_sum[0] += middle * count as f64;
                        piece.wide_sum[1] += (y as f64 + 0.5) * count as f64;
                    }
                }
            }
            opened(&narrow, before, half, width, &mut found.pieces);
            eroded.below = Some(narrow);
        }
        full.below = Some(here);
        let pixel_mm2 = input.pitch_mm[0] * input.pitch_mm[1];
        found.chambers(k, first, (layer.height_mm, pixel_mm2), vent.is_some());
    }
    let last = input.layers.len().saturating_sub(1);
    Ok(CupReport {
        cups: collect(&mut found, input, last, [&mut full, &mut eroded]),
    })
}

/// The statistics of the new eroded pieces (from `first` on) of `layer`: the pixels their
/// runs reach grown again by `half`, the regions a vent's circle sweeps.
fn opened(layer: &Labelled, first: usize, half: &[u32], width: u32, pieces: &mut [Piece]) {
    let reach = half.len() - 1;
    let Some(new) = pieces.get_mut(first..) else {
        return;
    };
    let mut by_piece: Vec<Vec<(usize, [u32; 2])>> = vec![Vec::new(); new.len()];
    for (y, i, run) in layer.runs() {
        if let Some(piece) = layer.piece[layer.component[i]].filter(|&p| p >= first) {
            by_piece[piece - first].push((y, run));
        }
    }
    for (stats, runs) in new.iter_mut().zip(by_piece) {
        let (Some(&(top, _)), Some(&(bottom, _))) = (runs.first(), runs.last()) else {
            continue;
        };
        // The piece's runs alone, with `reach` empty rows above and below.
        let from = top.saturating_sub(reach);
        let to = (bottom + reach + 1).min(layer.runs.height());
        let mut own = Runs {
            rows: vec![0],
            runs: Vec::new(),
        };
        let mut next = runs.iter().peekable();
        for y in from..to {
            while let Some(&&(_, run)) = next.peek().filter(|(r, _)| *r == y) {
                own.runs.push(run);
                next.next();
            }
            own.rows.push(own.runs.len());
        }
        let mut row = Vec::new();
        for y in 0..own.height() {
            reach_row(&own, y, half, width, &mut row);
            for &run in &row {
                stats.add(from + y, run);
            }
        }
    }
}

/// The caps of each cup's parts, by the cup's root, oldest first.
fn caps_by_cup(
    pieces: &[Piece],
    chambers: &[Chamber],
    cups: &mut Union,
) -> std::collections::BTreeMap<usize, Vec<CupCap>> {
    let mut by_cup: std::collections::BTreeMap<usize, Vec<CupCap>> =
        std::collections::BTreeMap::new();
    for chamber in chambers {
        let start = &pieces[chamber.piece];
        #[allow(clippy::cast_precision_loss)] // Pixel counts.
        let n = start.pixels.max(1) as f64;
        let cap = [start.sum[0] / n, start.sum[1] / n];
        let (vent_layer, vent) = chamber.vent.unwrap_or((start.layer, cap));
        by_cup
            .entry(cups.find(chamber.piece))
            .or_default()
            .push(CupCap {
                layer: start.layer,
                last_layer: chamber.last,
                cap,
                bounds: start.bounds,
                largest_pixels: chamber.largest_pixels,
                volume_mm3: chamber.volume_mm3,
                vent_layer,
                vent,
            });
    }
    for list in by_cup.values_mut() {
        list.sort_by(|a, b| {
            a.layer
                .cmp(&b.layer)
                .then(a.cap[0].total_cmp(&b.cap[0]))
                .then(a.cap[1].total_cmp(&b.cap[1]))
        });
    }
    by_cup
}

/// The pieces grouped by cup, each cup's statistics from its pieces and its parts' caps.
fn collect(
    found: &mut Analysis,
    input: &CupInput<'_>,
    last: usize,
    mut sweeps: [&mut Sweep; 2],
) -> Vec<SuctionCup> {
    let pieces = &found.pieces;
    let cups = &mut found.cups;
    let mut grouped: std::collections::BTreeMap<usize, Vec<&Piece>> =
        std::collections::BTreeMap::new();
    for (id, piece) in pieces.iter().enumerate() {
        grouped.entry(cups.find(id)).or_default().push(piece);
    }
    let mut by_cup = caps_by_cup(pieces, &found.chambers, cups);
    let mut caps_of = |root: usize| by_cup.remove(&root).unwrap_or_default();
    let pixel_mm2 = input.pitch_mm[0] * input.pitch_mm[1];
    let mut out: Vec<SuctionCup> = grouped
        .into_iter()
        .map(|(root, parts)| {
            let first = parts.iter().map(|p| p.layer).min().unwrap_or(0);
            let end = parts.iter().map(|p| p.layer).max().unwrap_or(0);
            let mut by_layer: std::collections::BTreeMap<usize, u64> =
                std::collections::BTreeMap::new();
            for p in &parts {
                *by_layer.entry(p.layer).or_default() += p.pixels;
            }
            // The widest layer, the lowest of equals.
            let (largest_layer, largest_pixels) = by_layer.iter().fold(
                (first, 0),
                |best, (&l, &n)| if n > best.1 { (l, n) } else { best },
            );
            #[allow(clippy::cast_precision_loss)] // Pixel counts.
            let volume_mm3 = by_layer
                .iter()
                .map(|(&l, &n)| n as f64 * pixel_mm2 * input.layers[l].height_mm)
                .sum();
            let (mut sum, mut cap_pixels) = ([0.0; 2], 0_u64);
            for p in parts.iter().filter(|p| p.layer == first) {
                sum[0] += p.sum[0];
                sum[1] += p.sum[1];
                cap_pixels += p.pixels;
            }
            #[allow(clippy::cast_precision_loss)] // Pixel counts.
            let n = cap_pixels.max(1) as f64;
            // Closed when, after its last layer, none of its last pieces reaches the outside.
            let closes = end < last
                && parts
                    .iter()
                    .filter(|p| p.layer == end)
                    .all(|p| !sweeps[usize::from(p.eroded)].outside(p.node));
            SuctionCup {
                first_layer: first,
                last_layer: end,
                opening: if parts.iter().any(|p| p.eroded) {
                    Opening::TooSmall
                } else {
                    Opening::None
                },
                closes,
                largest_layer,
                largest_pixels,
                pixels: by_layer.values().sum(),
                volume_mm3,
                cap: [sum[0] / n, sum[1] / n],
                bounds: parts.iter().fold([u32::MAX, u32::MAX, 0, 0], |b, p| {
                    [
                        b[0].min(p.bounds[0]),
                        b[1].min(p.bounds[1]),
                        b[2].max(p.bounds[2]),
                        b[3].max(p.bounds[3]),
                    ]
                }),
                caps: caps_of(root),
            }
        })
        .collect();
    out.sort_by(|a, b| {
        a.first_layer
            .cmp(&b.first_layer)
            .then(a.cap[0].total_cmp(&b.cap[0]))
            .then(a.cap[1].total_cmp(&b.cap[1]))
    });
    out
}

fn validate(input: &CupInput<'_>) -> Result<(), CupError> {
    if !input.pitch_mm.iter().all(|p| p.is_finite() && *p > 0.0) {
        return Err(CupError::InvalidParameter("pixel pitch"));
    }
    if input.material_gray == 0 {
        return Err(CupError::InvalidParameter("gray threshold"));
    }
    if !(input.min_vent_mm2.is_finite() && input.min_vent_mm2 >= 0.0) {
        return Err(CupError::InvalidParameter("minimum vent area"));
    }
    if !input
        .layers
        .iter()
        .all(|l| l.height_mm.is_finite() && l.height_mm > 0.0)
    {
        return Err(CupError::InvalidParameter("layer height"));
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
        Err(CupError::InconsistentLayers)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZE: u32 = 40;

    /// A layer of `SIZE` × `SIZE` pixels: `material` rectangles (first column and row, one
    /// past the last), then `empty` ones cleared.
    fn layer(material: &[[u32; 4]], empty: &[[u32; 4]]) -> SpanMask {
        let mut pixels = vec![0_u8; (SIZE * SIZE) as usize];
        for (rects, value) in [(material, 255), (empty, 0)] {
            for &[x0, y0, x1, y1] in rects {
                for y in y0..y1 {
                    for x in x0..x1 {
                        pixels[(y * SIZE + x) as usize] = value;
                    }
                }
            }
        }
        SpanMask::from_pixels(SIZE, SIZE, &pixels).unwrap()
    }

    const OUTER: [u32; 4] = [5, 5, 35, 35];
    const INNER: [u32; 4] = [10, 10, 30, 30];

    fn solid() -> SpanMask {
        layer(&[OUTER], &[])
    }

    fn ring() -> SpanMask {
        layer(&[OUTER], &[INNER])
    }

    /// The analysis of `layers` at 0.05 mm layers and pixels.
    fn analyse(layers: &[SpanMask], min_vent_mm2: f64) -> CupReport {
        let layers: Vec<CupLayer<'_>> = layers
            .iter()
            .enumerate()
            .map(|(k, mask)| CupLayer {
                #[allow(clippy::cast_precision_loss)] // A few layers.
                z_mm: 0.025 + 0.05 * k as f64,
                height_mm: 0.05,
                mask,
            })
            .collect();
        find_suction_cups(
            &CupInput {
                layers: &layers,
                pitch_mm: [0.05, 0.05],
                material_gray: 1,
                min_vent_mm2,
            },
            &mut Cancellation::never(),
        )
        .unwrap()
    }

    /// A box on the plate: a floor of 2 layers, `walls` (layers of the cavity), a roof of 2.
    fn closed_box(walls: &[SpanMask]) -> Vec<SpanMask> {
        let mut layers = vec![solid(), solid()];
        layers.extend_from_slice(walls);
        layers.extend([solid(), solid()]);
        layers
    }

    #[test]
    fn a_closed_box_is_a_cup_through_its_cavity_and_closes_over_its_resin() {
        let layers = closed_box(&vec![ring(); 6]);
        let report = analyse(&layers, 0.0);
        assert_eq!(report.cups.len(), 1, "{report:?}");
        let cup = &report.cups[0];
        assert_eq!((cup.first_layer, cup.last_layer), (2, 7));
        assert_eq!(cup.opening, Opening::None);
        assert!(cup.closes);
        assert_eq!((cup.largest_layer, cup.largest_pixels), (2, 400));
        assert_eq!(cup.pixels, 2400);
        assert!((cup.volume_mm3 - 2400.0 * 0.05 * 0.05 * 0.05).abs() < 1e-12);
        assert_eq!(cup.cap, [20.0, 20.0]);
        assert_eq!(cup.bounds, INNER);
    }

    #[test]
    fn a_cavity_taller_than_a_batch_of_layers_is_one_cup() {
        // The layers' runs are found a batch at a time (twice the cores); the sweeps join them.
        let layers = closed_box(&vec![ring(); 300]);
        let report = analyse(&layers, 0.2);
        assert_eq!(report.cups.len(), 1, "{report:?}");
        let cup = &report.cups[0];
        assert_eq!(
            (cup.first_layer, cup.last_layer, cup.pixels),
            (2, 301, 300 * 400)
        );
        assert_eq!(cup.opening, Opening::None);
    }

    #[test]
    fn the_plate_caps_a_ring_on_it_and_a_lifted_ring_is_open_below() {
        let mut on_plate = vec![ring(); 4];
        on_plate.push(layer(&[], &[]));
        let report = analyse(&on_plate, 0.0);
        assert_eq!(report.cups.len(), 1, "{report:?}");
        let cup = &report.cups[0];
        assert_eq!((cup.first_layer, cup.last_layer), (0, 3));
        // Past its rim the resin reaches the outside: nothing is trapped.
        assert!(!cup.closes);
        // Held by a pillar, the ring's inside is open to the resin under it.
        let mut lifted = vec![layer(&[[0, 0, 3, 3]], &[])];
        lifted.extend(vec![layer(&[OUTER, [0, 0, 3, 3]], &[INNER]); 4]);
        assert_eq!(analyse(&lifted, 0.0).cups, []);
    }

    /// A ring with a side opening 6 pixels (0.3 mm) wide through its wall.
    fn vented() -> SpanMask {
        layer(&[OUTER], &[INNER, [5, 17, 10, 23]])
    }

    #[test]
    fn a_side_vent_ends_the_cup_unless_it_is_narrower_than_the_minimum_vent() {
        let walls = [ring(), ring(), vented(), vented(), ring(), ring()];
        let layers = closed_box(&walls);
        // A circle of 0.05 mm² (0.25 mm across, 0.15 less a pixel) passes the opening.
        let report = analyse(&layers, 0.05);
        assert_eq!(report.cups.len(), 1, "{report:?}");
        let cup = &report.cups[0];
        assert_eq!((cup.first_layer, cup.last_layer), (2, 3));
        assert_eq!(cup.opening, Opening::None);
        assert!(!cup.closes);
        // One of 0.2 mm² (0.5 mm across) does not: the cup goes on, behind a vent too small.
        let report = analyse(&layers, 0.2);
        assert_eq!(report.cups.len(), 1, "{report:?}");
        let cup = &report.cups[0];
        assert_eq!((cup.first_layer, cup.last_layer), (2, 7));
        assert_eq!(cup.opening, Opening::TooSmall);
        assert!(cup.closes);
        assert_eq!((cup.largest_layer, cup.largest_pixels), (2, 400));
        // Past the opening, the cavity as far as the vent's circle sweeps it: all but the
        // corners.
        let swept = cup.pixels - 2 * 400;
        assert!((4 * 360..4 * 400).contains(&swept), "{swept}");
    }

    #[test]
    fn a_floor_vent_relieves_a_lifted_box_when_wide_enough() {
        // Pillars hold the box; its floor has a hole of 2 × 2 pixels (0.1 mm).
        let pillars = [[5, 5, 8, 8], [32, 32, 35, 35]];
        let floor = layer(&[OUTER], &[[19, 19, 21, 21]]);
        let mut layers = vec![layer(&pillars, &[]), floor.clone(), floor];
        layers.extend(vec![ring(); 4]);
        layers.extend([solid(), solid()]);
        // Any whole empty pixel vents without a minimum.
        assert_eq!(analyse(&layers, 0.0).cups, []);
        // A circle of 0.05 mm² does not pass 0.1 mm: the cavity over the floor is a cup.
        let report = analyse(&layers, 0.05);
        assert_eq!(report.cups.len(), 1, "{report:?}");
        let cup = &report.cups[0];
        assert_eq!((cup.first_layer, cup.last_layer), (3, 6));
        assert_eq!(cup.opening, Opening::TooSmall);
        assert!(cup.closes);
    }

    #[test]
    fn cups_joined_below_are_one_and_separate_ones_are_sorted() {
        // Two chimneys over one chamber are one cup; a second box beside is another.
        let chamber = layer(&[OUTER], &[INNER]);
        let chimneys = layer(&[OUTER], &[[10, 10, 15, 30], [25, 10, 30, 30]]);
        let report = analyse(&closed_box(&[chamber, chimneys.clone(), chimneys]), 0.0);
        assert_eq!(report.cups.len(), 1, "{report:?}");
        assert_eq!(report.cups[0].largest_pixels, 400);
        let two = layer(&[[1, 1, 39, 39]], &[[2, 2, 8, 8], [20, 2, 26, 8]]);
        let report = analyse(&[layer(&[[1, 1, 39, 39]], &[]), two.clone(), two], 0.0);
        let caps: Vec<[f64; 2]> = report.cups.iter().map(|c| c.cap).collect();
        assert_eq!(caps, vec![[5.0, 5.0], [23.0, 5.0]]);
    }

    #[test]
    fn each_lowest_point_of_a_cavity_is_a_cap_and_a_vent_relieves_only_its_own() {
        // Two lobes, the left from layer 2 and the right from layer 3, meet at layer 5.
        const LEFT: [u32; 4] = [8, 10, 18, 30];
        const RIGHT: [u32; 4] = [22, 10, 32, 30];
        let lobes = |floor: SpanMask| {
            vec![
                floor.clone(),
                floor,
                layer(&[OUTER], &[LEFT]),
                layer(&[OUTER], &[LEFT, RIGHT]),
                layer(&[OUTER], &[LEFT, RIGHT]),
                layer(&[OUTER], &[[8, 10, 32, 30]]),
                layer(&[OUTER], &[[8, 10, 32, 30]]),
                solid(),
                solid(),
            ]
        };
        let report = analyse(&lobes(solid()), 0.0);
        assert_eq!(report.cups.len(), 1, "{report:?}");
        let cup = &report.cups[0];
        assert_eq!((cup.first_layer, cup.last_layer), (2, 6));
        let caps: Vec<(usize, usize, [f64; 2], u64)> = cup
            .caps
            .iter()
            .map(|c| (c.layer, c.last_layer, c.cap, c.largest_pixels))
            .collect();
        assert_eq!(
            caps,
            vec![(2, 6, [13.0, 20.0], 480), (3, 4, [27.0, 20.0], 200)]
        );
        assert!((cup.caps[1].volume_mm3 - 400.0 * 0.05 * 0.05 * 0.05).abs() < 1e-12);
        // A hole under the left lobe vents it: the right one is still a cup until they meet.
        let mut vented = lobes(layer(&[OUTER], &[[12, 18, 15, 21]]));
        vented[0] = layer(&[[5, 5, 8, 8], [32, 32, 35, 35]], &[]);
        let report = analyse(&vented, 0.0);
        assert_eq!(report.cups.len(), 1, "{report:?}");
        let cup = &report.cups[0];
        assert_eq!(
            (cup.first_layer, cup.last_layer, cup.cap),
            (3, 4, [27.0, 20.0])
        );
        assert!(!cup.closes);
        assert_eq!(cup.caps.len(), 1);
    }

    #[test]
    fn a_vent_goes_where_its_circle_first_fits_above_a_narrow_tip() {
        // A tip two pixels wide, then the cavity: a circle of 0.05 mm² fits only from layer 3.
        let layers = closed_box(&[layer(&[OUTER], &[[19, 10, 21, 30]]), ring(), ring()]);
        let report = analyse(&layers, 0.05);
        assert_eq!(report.cups.len(), 1, "{report:?}");
        let cap = &report.cups[0].caps[0];
        assert_eq!((cap.layer, cap.cap), (2, [20.0, 20.0]));
        assert_eq!((cap.vent_layer, cap.vent), (3, [20.0, 20.0]));
        // Without a minimum vent it fits anywhere: at the tip itself.
        let cap = &analyse(&layers, 0.0).cups[0].caps[0];
        assert_eq!(cap.vent_layer, 2);
    }

    #[test]
    fn bad_parameters_and_layers_are_refused() {
        let mask = solid();
        let layers = [CupLayer {
            z_mm: 0.025,
            height_mm: 0.05,
            mask: &mask,
        }];
        let input = CupInput {
            layers: &layers,
            pitch_mm: [0.05, 0.05],
            material_gray: 1,
            min_vent_mm2: 1.0,
        };
        let run = |input: &CupInput<'_>| find_suction_cups(input, &mut Cancellation::never());
        assert!(run(&input).is_ok());
        for bad in [
            CupInput {
                min_vent_mm2: -1.0,
                ..input
            },
            CupInput {
                min_vent_mm2: f64::NAN,
                ..input
            },
            CupInput {
                material_gray: 0,
                ..input
            },
            CupInput {
                pitch_mm: [0.0, 0.05],
                ..input
            },
        ] {
            assert!(matches!(run(&bad), Err(CupError::InvalidParameter(_))));
        }
        let small = SpanMask::from_pixels(2, 2, &[0; 4]).unwrap();
        let mixed = [
            layers[0],
            CupLayer {
                z_mm: 0.075,
                height_mm: 0.05,
                mask: &small,
            },
        ];
        assert_eq!(
            run(&CupInput {
                layers: &mixed,
                ..input
            }),
            Err(CupError::InconsistentLayers)
        );
    }
}
