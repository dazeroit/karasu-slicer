//! Bounded even-odd scanline sampling and separable grayscale morphology.
//!
//! Each output row is produced from the sorted sample-space span endpoints of its sub-rows, so the
//! per-row work is proportional to its span endpoints and covered runs rather than to the panel
//! width. Rows are emitted as nonzero spans of a [`SpanMask`] (TD-055): no dense image exists
//! unless compensation needs one.
use super::{
    cap,
    mask::{SpanMask, SpanMaskBuilder},
    reserve, Compensation, Edge, KeptSampling, LcdProfile, PreviewStats, RasterConfig, RasterError,
    RasterImage, RasterLimits, WindingRange,
};
use crate::cancellation::{Cancellation, Checkpoint};
use crate::geometry::Point3;
use crate::slicing::{exact::orientation, schedule::LayerPlane, winding::WindingRule};
use std::{cmp::Ordering, mem::size_of};

/// How body crossings define material along a sample row.
#[derive(Clone, Copy, Debug)]
pub(super) enum Fill {
    /// Even-odd parity of unoriented boundary edges (strict layers).
    Parity,
    /// The winding number of directed edges (material on their left in world XY) under `rule`.
    Winding(WindingRule),
}

pub(super) fn preflight(
    profile: LcdProfile,
    config: RasterConfig,
    limits: RasterLimits,
    count: usize,
) -> Result<usize, RasterError> {
    config.validate()?;
    cap(count, limits.max_edges, "edges")?;
    let [w, h] = profile
        .dimensions()
        .map(|n| usize::try_from(n).map_err(|_| RasterError::SizeOverflow));
    let (w, h) = (w?, h?);
    let pixels = w.checked_mul(h).ok_or(RasterError::SizeOverflow)?;
    cap(pixels, limits.max_pixels, "pixels")?;
    // Per edge: the edge, its order and active slots, one crossing, at most one span entry across
    // the body, cut and kept span buffers, and one row event per sub-row (plus spare entries).
    let samples = usize::from(config.samples_per_axis);
    let edge_bytes = size_of::<Edge>()
        + 2 * size_of::<usize>()
        + size_of::<f64>()
        + size_of::<(u32, u32)>()
        + samples * size_of::<u32>();
    let mut bytes = count
        .checked_add(3)
        .and_then(|n| n.checked_mul(edge_bytes))
        .and_then(|n| n.checked_add(samples * samples + 1))
        .and_then(|n| n.checked_add(SpanMaskBuilder::row_table_bytes(h)?))
        .ok_or(RasterError::SizeOverflow)?;
    // The retained spans are charged as they are produced, against the rest of the budget.
    if !matches!(config.compensation, Compensation::None) {
        // Compensation works on a dense image and its scratch copy.
        bytes = pixels
            .checked_mul(2)
            .and_then(|n| n.checked_add(bytes))
            .and_then(|n| n.checked_add(w.max(h).checked_mul(size_of::<usize>())?))
            .ok_or(RasterError::SizeOverflow)?;
    }
    cap(bytes, limits.max_payload_bytes, "payload bytes")?;
    Ok(bytes)
}

struct Work<'a, 'b> {
    used: usize,
    limit: usize,
    cancel: &'a mut Cancellation<'b>,
}
impl Work<'_, '_> {
    fn charge(&mut self, n: usize) -> Result<(), RasterError> {
        let next = self.used.checked_add(n).ok_or(RasterError::SizeOverflow)?;
        cap(next, self.limit, "work")?;
        if self.used / 256 != next / 256 || self.used == 0 {
            self.cancel.check(Checkpoint::RasterRows)?;
        }
        self.used = next;
        Ok(())
    }
    fn sort(&mut self, n: usize) -> Result<(), RasterError> {
        self.cancel.check(Checkpoint::RasterRows)?;
        let logarithm =
            usize::try_from(n.max(1).bit_width()).map_err(|_| RasterError::SizeOverflow)?;
        self.charge(
            n.checked_mul(logarithm)
                .and_then(|v| v.checked_mul(2))
                .ok_or(RasterError::SizeOverflow)?,
        )
    }
}
fn allocated<T>(n: usize, work: &mut Work<'_, '_>) -> Result<Vec<T>, RasterError> {
    work.cancel.check(Checkpoint::RasterRows)?;
    let values = reserve(n)?;
    work.cancel.check(Checkpoint::RasterRows)?;
    Ok(values)
}
fn filled<T: Clone>(n: usize, value: T, work: &mut Work<'_, '_>) -> Result<Vec<T>, RasterError> {
    let mut values = allocated(n, work)?;
    values.resize(n, value);
    work.cancel.check(Checkpoint::RasterRows)?;
    Ok(values)
}

// Dimensions and transformed coordinates are bounded by the validated profile/domain; the
// clamped integral sample index is at most 65536*16 and fits u32 exactly.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn sample_index(x: f64, samples: u32, width: u32) -> u32 {
    (x.mul_add(f64::from(samples), -0.5)
        .ceil()
        .clamp(0., f64::from(width * samples))) as u32
}
/// The intensity of a pixel with `count` covered samples: the coverage, raised to at least
/// `minimum_gray` when partial, rounded to the nearest of the equally spaced levels (ties up).
fn quantize(count: u16, config: RasterConfig) -> u8 {
    if count == 0 {
        return 0;
    }
    let total = u64::from(config.samples_per_axis).pow(2);
    let levels = u64::from(config.grayscale_levels) - 1;
    let floor = u64::from(config.minimum_gray);
    // The exact intensity on the 0..=255 scale is (floor * total + (255 - floor) * count) / total;
    // scaled to levels it stays below 2^32.
    let numerator = (floor * total + (255 - floor) * u64::from(count)) * levels;
    let denominator = 255 * total;
    let level = (2 * numerator + denominator) / (2 * denominator);
    u8::try_from((level * 255 + levels / 2) / levels).expect("quantized intensity is at most 255")
}
/// One output row written left to right as runs of constant sample count.
///
/// Runs arrive in increasing column order and never overlap; uncovered pixels are never touched.
struct RowWriter<'r> {
    out: &'r mut SpanMaskBuilder,
    /// Quantized intensity for every sample count `0..=n*n`.
    levels: &'r [u8],
    n: u32,
    y: u32,
    /// A partially covered pixel still accumulating samples from later segments.
    pending: Option<(u32, u32)>,
    /// Incremental preview statistics, or `None` when compensation recomputes them.
    stats: Option<&'r mut PreviewStats>,
}

impl RowWriter<'_> {
    /// Add `depth` covering sub-rows over the sample columns `[start, end)`.
    fn cover(&mut self, start: u32, end: u32, depth: u32) -> Result<(), RasterError> {
        let n = self.n;
        let (first, last) = (start / n, (end - 1) / n);
        if first == last {
            return self.add(first, depth * (end - start));
        }
        let mut full = first;
        if !start.is_multiple_of(n) {
            self.add(first, depth * ((first + 1) * n - start))?;
            full += 1;
        }
        let tail = !end.is_multiple_of(n);
        let stop = if tail { last } else { last + 1 };
        if full < stop {
            // Later segments start at or after `end`, so these pixels are complete.
            self.flush()?;
            self.run(full, stop, depth * n)?;
        }
        if tail {
            self.add(last, depth * (end - last * n))?;
        }
        Ok(())
    }

    fn add(&mut self, pixel: u32, count: u32) -> Result<(), RasterError> {
        match &mut self.pending {
            Some((current, total)) if *current == pixel => *total += count,
            _ => {
                self.flush()?;
                self.pending = Some((pixel, count));
            }
        }
        Ok(())
    }

    fn flush(&mut self) -> Result<(), RasterError> {
        match self.pending.take() {
            Some((pixel, count)) => self.run(pixel, pixel + 1, count),
            None => Ok(()),
        }
    }

    /// Write pixels `[from, to)` with the quantized intensity of `count` samples each.
    fn run(&mut self, from: u32, to: u32, count: u32) -> Result<(), RasterError> {
        // Disjoint spans per sub-row cover each sample at most once: `count <= n*n`.
        let value = usize::try_from(count)
            .ok()
            .and_then(|c| self.levels.get(c).copied())
            .ok_or(RasterError::SizeOverflow)?;
        if value == 0 {
            return Ok(());
        }
        self.out.push(from, to, value)?;
        if let Some(stats) = &mut self.stats {
            let length = usize::try_from(to - from).map_err(|_| RasterError::SizeOverflow)?;
            stats.nonzero_pixels += length;
            stats.intensity_sum += u64::from(value) * length as u64;
            let (y, next) = (self.y, self.y + 1);
            stats.bounds = Some(match stats.bounds {
                Some([[x0, y0], [x1, y1]]) => {
                    [[x0.min(from), y0.min(y)], [x1.max(to), y1.max(next)]]
                }
                None => [[from, y], [to, next]],
            });
        }
        Ok(())
    }
}

/// Scanline state of one boundary edge set over increasing sample rows.
struct Scanline<'e> {
    edges: &'e [Edge],
    order: Vec<usize>,
    active: Vec<usize>,
    /// First sample column at or right of each crossing, and the winding change past it: +1 for
    /// an edge running towards -Y.
    crossings: Vec<(u32, i32)>,
    next: usize,
    samples: u32,
    width: u32,
}

impl<'e> Scanline<'e> {
    fn new(
        edges: &'e [Edge],
        samples: u32,
        width: u32,
        work: &mut Work<'_, '_>,
    ) -> Result<Self, RasterError> {
        let mut order = allocated::<usize>(edges.len(), work)?;
        for (i, e) in edges.iter().enumerate() {
            work.charge(1)?;
            if e.a[1].partial_cmp(&e.b[1]) != Some(std::cmp::Ordering::Equal) {
                order.push(i);
            }
        }
        work.sort(order.len())?;
        order.sort_unstable_by(|&a, &b| {
            edges[a].a[1]
                .min(edges[a].b[1])
                .total_cmp(&edges[b].a[1].min(edges[b].b[1]))
                .then(a.cmp(&b))
        });
        Ok(Self {
            edges,
            order,
            active: allocated::<usize>(edges.len(), work)?,
            crossings: allocated::<(u32, i32)>(edges.len(), work)?,
            next: 0,
            samples,
            width,
        })
    }

    /// Crossings of sample row `sy` (rows must be nondecreasing), sorted by sample column; their
    /// count is even. An edge is active on rows `min y <= sy < max y`.
    fn crossings(
        &mut self,
        sy: f64,
        work: &mut Work<'_, '_>,
    ) -> Result<&[(u32, i32)], RasterError> {
        let edges = self.edges;
        while self.next < self.order.len() {
            work.charge(1)?;
            let id = self.order[self.next];
            let edge = edges[id];
            if edge.a[1].min(edge.b[1]) > sy {
                break;
            }
            self.active.push(id);
            self.next += 1;
        }
        self.crossings.clear();
        let mut retained = 0;
        for i in 0..self.active.len() {
            work.charge(1)?;
            let id = self.active[i];
            let edge = edges[id];
            if edge.a[1].max(edge.b[1]) <= sy {
                continue;
            }
            self.active[retained] = id;
            retained += 1;
            let (low, high, winding) = if edge.a[1] < edge.b[1] {
                (edge.a, edge.b, -1)
            } else {
                (edge.b, edge.a, 1)
            };
            // An estimate interpolated from the endpoint nearer the row, then corrected exactly.
            let (near, far) = if sy - low[1] <= high[1] - sy {
                (low, high)
            } else {
                (high, low)
            };
            let fraction = (sy - near[1]) / (far[1] - near[1]);
            let estimate = (far[0] - near[0]).mul_add(fraction, near[0]);
            if !estimate.is_finite() {
                return Err(RasterError::NumericRange);
            }
            let column = first_sample(low, high, sy, estimate, self.samples, self.width);
            self.crossings.push((column, winding));
        }
        self.active.truncate(retained);
        work.sort(self.crossings.len())?;
        self.crossings.sort_unstable_by_key(|c| c.0);
        if !self.crossings.len().is_multiple_of(2) {
            return Err(RasterError::InvalidGeometry);
        }
        Ok(&self.crossings)
    }
}

/// The first sample column (clamped to `0..=width*samples`) whose centre on row `sy` lies exactly
/// at or right of the edge from `low` to `high` (`low` below `high`), starting from the estimate.
///
/// The exact orientation sign decides every sample, so a sample is covered by the even-odd or
/// winding rule exactly as its centre lies, a centre on an edge belonging to the material on its
/// right, independently of the edge's direction and of interpolation rounding.
fn first_sample(
    low: [f64; 2],
    high: [f64; 2],
    sy: f64,
    estimate: f64,
    samples: u32,
    width: u32,
) -> u32 {
    let total = width * samples;
    let (low, high) = (
        Point3::new(low[0], low[1], 0.),
        Point3::new(high[0], high[1], 0.),
    );
    // Left of an upward edge is a positive orientation.
    let at_or_right = |k: u32| {
        let centre = (f64::from(k) + 0.5) / f64::from(samples);
        orientation(low, high, Point3::new(centre, sy, 0.)) != Ordering::Greater
    };
    let mut column = sample_index(estimate, samples, width);
    while column > 0 && at_or_right(column - 1) {
        column -= 1;
    }
    while column < total && !at_or_right(column) {
        column += 1;
    }
    column
}

/// Half-open sample-index spans `[left, right)` of consecutive crossing pairs (disjoint, ordered).
fn spans(crossings: &[(u32, i32)], out: &mut Vec<(u32, u32)>) {
    out.clear();
    for pair in crossings.as_chunks::<2>().0 {
        let (left, right) = (pair[0].0, pair[1].0);
        if left < right {
            out.push((left, right));
        }
    }
}

/// Half-open sample-index spans covered by at least one ring of a cut whose rings all run the
/// same way (see `input::ring_edges`): where the winding number is not zero, whatever the sign
/// a mirrored panel gives it. Overlapping rings remove their union (TD-089); for disjoint rings
/// this is the even-odd area.
fn union_spans(crossings: &[(u32, i32)], out: &mut Vec<(u32, u32)>) {
    out.clear();
    let (mut winding, mut open) = (0i64, None);
    for &(index, change) in crossings {
        winding += i64::from(change);
        match (open, winding != 0) {
            (None, true) => open = Some(index),
            (Some(left), false) => {
                if left < index {
                    out.push((left, index));
                }
                open = None;
            }
            _ => {}
        }
    }
}

/// Half-open sample-index spans where the winding number satisfies `rule` (disjoint, ordered).
///
/// `orientation` is -1 when pixel space mirrors world XY. `range` widens to the winding numbers of
/// the intervals between crossings that contain a sample, and counts their negative samples.
/// Returns the first sample index with a negative winding number, if any; deciding whether that
/// fails the layer is the caller's.
fn winding_spans(
    crossings: &[(u32, i32)],
    rule: WindingRule,
    orientation: i64,
    out: &mut Vec<(u32, u32)>,
    range: &mut WindingRange,
) -> Result<Option<u32>, RasterError> {
    out.clear();
    let (mut winding, mut previous, mut open) = (0i64, 0u32, None);
    let mut first_negative = None;
    for &(index, change) in crossings {
        if index > previous {
            range.min = range.min.min(winding);
            range.max = range.max.max(winding);
            if winding < 0 {
                first_negative.get_or_insert(previous);
                range.negative_samples += u64::from(index - previous);
            }
        }
        winding += orientation * i64::from(change);
        match (open, rule.fills(winding)) {
            (None, true) => open = Some(index),
            (Some(left), false) => {
                if left < index {
                    out.push((left, index));
                }
                open = None;
            }
            _ => {}
        }
        previous = index;
    }
    // A balanced closed section crosses every row with zero net winding.
    if winding != 0 {
        return Err(RasterError::InvalidGeometry);
    }
    Ok(first_negative)
}

/// `body \ cut` for ordered disjoint half-open spans, by a two-pointer sweep.
/// The sample columns of the whole pixels whose centre-most column (`centre` within each pixel
/// of `n` columns) lies in one of `spans`, as sorted disjoint half-open spans (TD-134).
fn whole_pixels(spans: &[(u32, u32)], n: u32, centre: u32, out: &mut Vec<(u32, u32)>) {
    out.clear();
    for &(left, right) in spans {
        // Pixel `p` is in when `left <= p * n + centre < right`.
        let from = if left <= centre {
            0
        } else {
            (left - centre).div_ceil(n)
        };
        let to = if right <= centre {
            0
        } else {
            (right - 1 - centre) / n + 1
        };
        if from < to {
            let (start, end) = (from * n, to * n);
            match out.last_mut() {
                Some(last) if start <= last.1 => last.1 = last.1.max(end),
                _ => out.push((start, end)),
            }
        }
    }
}

/// The union of two sorted lists of disjoint half-open spans, sorted and disjoint.
fn unite(a: &[(u32, u32)], b: &[(u32, u32)], out: &mut Vec<(u32, u32)>) {
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

fn subtract(body: &[(u32, u32)], cut: &[(u32, u32)], out: &mut Vec<(u32, u32)>) {
    out.clear();
    let mut j = 0;
    for &(mut left, right) in body {
        while j < cut.len() && cut[j].1 <= left {
            j += 1;
        }
        let mut k = j;
        while left < right {
            match cut.get(k) {
                Some(&(cl, cr)) if cl < right => {
                    if cl > left {
                        out.push((left, cl));
                    }
                    left = left.max(cr);
                    if cr <= right {
                        k += 1;
                    } else {
                        break;
                    }
                }
                _ => {
                    out.push((left, right));
                    break;
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)] // One atomic layer pipeline; phase helpers own bounded scratch.
pub(super) fn render(
    edges: &[Edge],
    plane: LayerPlane,
    profile: LcdProfile,
    config: RasterConfig,
    limits: RasterLimits,
    bytes: usize,
    cancel: &mut Cancellation<'_>,
) -> Result<RasterImage, RasterError> {
    render_difference(
        edges,
        &[],
        &[],
        Fill::Parity,
        plane,
        profile,
        config,
        limits,
        bytes,
        cancel,
    )
}

/// Rasterize the material of `body` under `fill` minus the even-odd material of `cut`, except
/// where `kept` has material (TD-123).
///
/// Each sample point is covered exactly when it is inside `body` and either outside `cut` or
/// where the winding number of `kept` is positive, which equals sampling the polygonal difference
/// `body - (cut - kept)`; no polygon is constructed. `kept` is a closed body inside `body` (the
/// supports): their material is never cut. Empty `cut` and `kept` give the plain mask.
///
/// With [`KeptSampling::Binary`] (TD-134), `body` must hold the layer's model alone, without the
/// kept bodies' segments: each sample is covered when it
/// is inside `body` and outside `cut`, or when its pixel's centre-most sample (column and
/// sub-row `n / 2`) has a positive winding number of `kept`, which makes the whole pixel full.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)] // One atomic layer pipeline; phase helpers own bounded scratch.
pub(super) fn render_difference(
    body: &[Edge],
    cut: &[Edge],
    kept: &[Edge],
    fill: Fill,
    plane: LayerPlane,
    profile: LcdProfile,
    config: RasterConfig,
    limits: RasterLimits,
    bytes: usize,
    cancel: &mut Cancellation<'_>,
) -> Result<RasterImage, RasterError> {
    let mut work = Work {
        used: 0,
        limit: limits.max_work,
        cancel,
    };
    work.charge(
        body.len()
            .checked_add(cut.len())
            .and_then(|n| n.checked_add(kept.len()))
            .and_then(|n| n.checked_mul(3))
            .ok_or(RasterError::SizeOverflow)?,
    )?;
    let [width, height] = profile.dimensions();
    let w = usize::try_from(width).map_err(|_| RasterError::SizeOverflow)?;
    let h = usize::try_from(height).map_err(|_| RasterError::SizeOverflow)?;
    let n = u32::from(config.samples_per_axis);
    let mut body_lines = Scanline::new(body, n, width, &mut work)?;
    let mut cut_lines = Scanline::new(cut, n, width, &mut work)?;
    let mut kept_lines = Scanline::new(kept, n, width, &mut work)?;
    // Binary kept bodies (TD-134): their own scanline at each row's centre-most sub-row.
    let binary = config.kept == KeptSampling::Binary && n > 1 && !kept.is_empty();
    let mut centre_lines = Scanline::new(if binary { kept } else { &[] }, n, width, &mut work)?;
    let centre = n / 2;
    let mut centre_spans = allocated::<(u32, u32)>(kept.len() / 2 + 1, &mut work)?;
    let mut whole = allocated::<(u32, u32)>(kept.len() / 2 + 1, &mut work)?;
    let mut with_whole =
        allocated::<(u32, u32)>(body.len() / 2 + cut.len() / 2 + kept.len() + 1, &mut work)?;
    let mut centre_range = WindingRange {
        min: 0,
        max: 0,
        negative_samples: 0,
    };
    work.cancel.check(Checkpoint::RasterRows)?;
    let max_spans = limits.max_payload_bytes.saturating_sub(bytes) / SpanMaskBuilder::SPAN_BYTES;
    let mut mask = SpanMaskBuilder::new(width, height, max_spans)?;
    let mut body_spans = allocated::<(u32, u32)>(body.len() / 2 + 1, &mut work)?;
    let mut cut_spans = allocated::<(u32, u32)>(cut.len() / 2 + 1, &mut work)?;
    let mut kept_spans = allocated::<(u32, u32)>(kept.len() / 2 + 1, &mut work)?;
    let mut kept_range = WindingRange {
        min: 0,
        max: 0,
        negative_samples: 0,
    };
    // The cut without what it leaves: at most one span more than the cut's per kept span.
    let mut leaving = allocated::<(u32, u32)>(cut.len() / 2 + kept.len() / 2 + 1, &mut work)?;
    let mut kept_body = allocated::<(u32, u32)>(
        body.len() / 2 + cut.len() / 2 + kept.len() / 2 + 1,
        &mut work,
    )?;
    // Every sub-row opens and closes each covered span once: at most `body + cut + kept + 2`
    // events.
    let row_events = body
        .len()
        .checked_add(cut.len())
        .and_then(|v| v.checked_add(kept.len()))
        .and_then(|v| v.checked_add(2))
        .and_then(|v| v.checked_mul(usize::from(config.samples_per_axis)))
        .ok_or(RasterError::SizeOverflow)?;
    let mut events = allocated::<u32>(row_events, &mut work)?;
    let total = u16::from(config.samples_per_axis).pow(2);
    work.charge(usize::from(total) + 1)?;
    let levels: Vec<u8> = (0..=total).map(|count| quantize(count, config)).collect();
    // Without compensation the written runs are final, so statistics are gathered as they go.
    let incremental = matches!(config.compensation, Compensation::None);
    let mut stats = PreviewStats {
        nonzero_pixels: 0,
        intensity_sum: 0,
        bounds: None,
    };
    let mut range = WindingRange {
        min: 0,
        max: 0,
        negative_samples: 0,
    };
    let mut first_negative: Option<[u32; 2]> = None;
    let o = profile.orientation();
    let orientation = if o.swap_xy ^ o.flip_x ^ o.flip_y {
        -1
    } else {
        1
    };
    for y in 0..height {
        events.clear();
        whole.clear();
        if binary {
            // Whole pixels whose centre-most sample lies in a kept body.
            let sy = f64::from(y) + (f64::from(centre) + 0.5) / f64::from(n);
            let crossings = centre_lines.crossings(sy, &mut work)?;
            winding_spans(
                crossings,
                WindingRule::Positive,
                orientation,
                &mut centre_spans,
                &mut centre_range,
            )?;
            work.charge(centre_spans.len())?;
            whole_pixels(&centre_spans, n, centre, &mut whole);
        }
        for sub in 0..n {
            let sy = f64::from(y) + (f64::from(sub) + 0.5) / f64::from(n);
            let crossings = body_lines.crossings(sy, &mut work)?;
            match fill {
                Fill::Parity => spans(crossings, &mut body_spans),
                Fill::Winding(rule) => {
                    let negative =
                        winding_spans(crossings, rule, orientation, &mut body_spans, &mut range)?;
                    if let Some(sample) = negative {
                        let pixel = [sample / n, y];
                        if rule == WindingRule::Positive {
                            return Err(RasterError::NegativeWinding {
                                layer: plane.index,
                                pixel,
                            });
                        }
                        first_negative.get_or_insert(pixel);
                    }
                }
            }
            let crossings = cut_lines.crossings(sy, &mut work)?;
            union_spans(crossings, &mut cut_spans);
            if !binary && !cut_spans.is_empty() && !kept.is_empty() {
                // The material the cut leaves: positive winding of the kept bodies alone, which
                // are closed and outward; a negative number there is outside them.
                let crossings = kept_lines.crossings(sy, &mut work)?;
                winding_spans(
                    crossings,
                    WindingRule::Positive,
                    orientation,
                    &mut kept_spans,
                    &mut kept_range,
                )?;
                work.charge(cut_spans.len() + kept_spans.len())?;
                subtract(&cut_spans, &kept_spans, &mut leaving);
                std::mem::swap(&mut cut_spans, &mut leaving);
            }
            work.cancel.check(Checkpoint::RasterRows)?;
            work.charge(body_spans.len() + cut_spans.len())?;
            let mut covered = if cut_spans.is_empty() {
                &body_spans
            } else {
                subtract(&body_spans, &cut_spans, &mut kept_body);
                &kept_body
            };
            if !whole.is_empty() {
                work.charge(covered.len() + whole.len())?;
                unite(covered, &whole, &mut with_whole);
                covered = &with_whole;
            }
            for &(left, right) in covered {
                work.charge(1)?;
                // Sample indices are at most 65536*16, so the tagged value fits u32: the low bit
                // marks an opening, and closings sort first at equal positions.
                events.push(left << 1 | 1);
                events.push(right << 1);
            }
        }
        work.sort(events.len())?;
        events.sort_unstable();
        let mut row = RowWriter {
            out: &mut mask,
            levels: &levels,
            n,
            y,
            pending: None,
            stats: incremental.then_some(&mut stats),
        };
        // Sweep the endpoints: between consecutive events the number of covering sub-rows is
        // constant. Each span opens before it closes, so the depth never underflows.
        let (mut depth, mut at) = (0u32, 0u32);
        for &event in &events {
            work.charge(1)?;
            let position = event >> 1;
            if depth > 0 && position > at {
                row.cover(at, position, depth)?;
            }
            at = position;
            if event & 1 == 1 {
                depth += 1;
            } else {
                depth -= 1;
            }
        }
        row.flush()?;
        mask.end_row()?;
    }
    if let (
        Fill::Winding(WindingRule::PositiveBounded {
            max_negative_samples,
        }),
        Some(pixel),
    ) = (fill, first_negative)
    {
        if range.negative_samples > max_negative_samples {
            return Err(RasterError::NegativeWindingAboveBound {
                layer: plane.index,
                pixel,
                samples: range.negative_samples,
                max_samples: max_negative_samples,
            });
        }
    }
    let mut mask = mask.finish()?;
    if !incremental {
        let mut pixels = mask.to_pixels()?;
        morphology(&mut pixels, w, h, config.compensation, &mut work)?;
        mask = SpanMask::from_pixels(width, height, &pixels)?;
        stats = statistics(&mask, &mut work)?;
    }
    let bytes = mask
        .span_count()
        .checked_mul(SpanMaskBuilder::SPAN_BYTES)
        .and_then(|n| n.checked_add(bytes))
        .ok_or(RasterError::SizeOverflow)?;
    work.cancel.check(Checkpoint::RasterComplete)?;
    Ok(RasterImage {
        profile,
        config,
        plane,
        stats,
        winding: matches!(fill, Fill::Winding(_)).then_some(range),
        closure: None,
        mask,
        requested_payload_bytes: bytes,
    })
}

fn morphology(
    pixels: &mut [u8],
    w: usize,
    h: usize,
    operation: Compensation,
    work: &mut Work<'_, '_>,
) -> Result<(), RasterError> {
    let (radius, dilate) = match operation {
        Compensation::None => return Ok(()),
        Compensation::Dilate(r) => (r, true),
        Compensation::Erode(r) => (r, false),
    };
    let mut scratch = filled(pixels.len(), 0u8, work)?;
    let mut queue = filled(w.max(h), 0usize, work)?;
    for y in 0..h {
        line_extrema(
            pixels,
            &mut scratch,
            y * w,
            1,
            w,
            usize::from(radius[0]),
            dilate,
            &mut queue,
            work,
        )?;
    }
    for x in 0..w {
        line_extrema(
            &scratch,
            pixels,
            x,
            w,
            h,
            usize::from(radius[1]),
            dilate,
            &mut queue,
            work,
        )?;
    }
    Ok(())
}
#[allow(clippy::too_many_arguments)] // Explicit strided line avoids allocations/copies for vertical passes.
fn line_extrema(
    input: &[u8],
    output: &mut [u8],
    offset: usize,
    stride: usize,
    len: usize,
    radius: usize,
    dilate: bool,
    queue: &mut [usize],
    work: &mut Work<'_, '_>,
) -> Result<(), RasterError> {
    let (mut head, mut tail, mut next) = (0, 0, 0);
    for pos in 0..len {
        work.charge(1)?;
        let end = pos.saturating_add(radius).min(len - 1);
        while next <= end {
            work.charge(1)?;
            let value = input[offset + next * stride];
            while tail > head {
                work.charge(1)?;
                let old = input[offset + queue[tail - 1] * stride];
                if if dilate { old > value } else { old < value } {
                    break;
                }
                tail -= 1;
            }
            queue[tail] = next;
            tail += 1;
            next += 1;
        }
        let start = pos.saturating_sub(radius);
        while head < tail && queue[head] < start {
            work.charge(1)?;
            head += 1;
        }
        output[offset + pos * stride] = if !dilate && (pos < radius || radius >= len - pos) {
            0
        } else {
            input[offset + queue[head] * stride]
        };
    }
    Ok(())
}
fn statistics(mask: &SpanMask, work: &mut Work<'_, '_>) -> Result<PreviewStats, RasterError> {
    let mut result = PreviewStats {
        nonzero_pixels: 0,
        intensity_sum: 0,
        bounds: None,
    };
    for (y, row) in (0u32..).zip(mask.rows()) {
        for span in row {
            work.charge(1)?;
            let length = span.end() - span.start();
            result.nonzero_pixels += length as usize;
            result.intensity_sum += u64::from(span.value()) * u64::from(length);
            let (x0, x1) = (span.start(), span.end());
            result.bounds = Some(match result.bounds {
                Some([[a, b], [c, d]]) => [[a.min(x0), b.min(y)], [c.max(x1), d.max(y + 1)]],
                None => [[x0, y], [x1, y + 1]],
            });
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::raster::{Orientation, OutsidePolicy};
    fn config(n: u8) -> RasterConfig {
        RasterConfig {
            samples_per_axis: n,
            grayscale_levels: 256,
            minimum_gray: 0,
            outside: OutsidePolicy::Clip,
            compensation: Compensation::None,
            kept: crate::raster::KeptSampling::Antialiased,
        }
    }
    fn profile() -> LcdProfile {
        LcdProfile::try_new([4, 3], [0., 0.], [4., 3.], Orientation::default()).unwrap()
    }
    fn plane() -> LayerPlane {
        LayerPlane {
            index: 0,
            z_mm: 0.05,
            nominal_height_mm: 0.05,
        }
    }
    fn draw(edges: &[Edge], c: RasterConfig) -> RasterImage {
        let p = profile();
        let l = RasterLimits::default();
        let bytes = preflight(p, c, l, edges.len()).unwrap();
        render(edges, plane(), p, c, l, bytes, &mut Cancellation::never()).unwrap()
    }
    fn rectangle(x0: f64, y0: f64, x1: f64, y1: f64) -> [Edge; 4] {
        let p = [[x0, y0], [x1, y0], [x1, y1], [x0, y1]];
        std::array::from_fn(|i| Edge {
            a: p[i],
            b: p[(i + 1) % 4],
        })
    }
    #[test]
    fn span_subtraction_matches_a_boolean_oracle() {
        let mut state = 0x2545_f491_4f6c_dd1d_u64;
        let mut next = |m: u32| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            u32::try_from(state % u64::from(m)).unwrap()
        };
        let mut random_spans = |count: u32| {
            let mut points: Vec<u32> = (0..count * 2).map(|_| next(64)).collect();
            points.sort_unstable();
            points
                .chunks(2)
                .map(|p| (p[0], p[1]))
                .filter(|(l, r)| l < r)
                .collect::<Vec<_>>()
        };
        for _ in 0..2000 {
            let (body, cut) = (random_spans(4), random_spans(4));
            // Overlapping random spans are merged by the even-odd construction in practice; the
            // oracle below uses union semantics, so dedupe overlaps first.
            let merge = |spans: Vec<(u32, u32)>| {
                let mut out: Vec<(u32, u32)> = Vec::new();
                for (l, r) in spans {
                    match out.last_mut() {
                        Some(last) if l <= last.1 => last.1 = last.1.max(r),
                        _ => out.push((l, r)),
                    }
                }
                out
            };
            let (body, cut) = (merge(body), merge(cut));
            let mut kept = Vec::new();
            subtract(&body, &cut, &mut kept);
            let covered =
                |spans: &[(u32, u32)], x: u32| spans.iter().any(|&(l, r)| l <= x && x < r);
            for x in 0..64 {
                assert_eq!(
                    covered(&kept, x),
                    covered(&body, x) && !covered(&cut, x),
                    "{body:?} - {cut:?} = {kept:?} at {x}"
                );
            }
            assert!(kept.windows(2).all(|w| w[0].1 <= w[1].0));
        }
    }

    #[test]
    fn difference_render_equals_per_sample_membership() {
        let p = profile();
        let l = RasterLimits::default();
        for n in [1, 4] {
            let c = config(n);
            // Body: 0.5..3.5 x 0.25..2.75 with a 1.5..2.5 x 1..2 hole; cut: 1..3.25 x 0.5..1.5.
            let mut body = rectangle(0.5, 0.25, 3.5, 2.75).to_vec();
            body.extend(rectangle(1.5, 1.0, 2.5, 2.0));
            let cut = rectangle(1.0, 0.5, 3.25, 1.5).to_vec();
            let bytes = preflight(p, c, l, body.len() + cut.len()).unwrap();
            let image = render_difference(
                &body,
                &cut,
                &[],
                Fill::Parity,
                plane(),
                p,
                c,
                l,
                bytes,
                &mut Cancellation::never(),
            )
            .unwrap();
            let inside =
                |x: f64, y: f64, r: [f64; 4]| x >= r[0] && x < r[2] && y >= r[1] && y < r[3];
            let samples = f64::from(n);
            for y in 0..3u32 {
                for x in 0..4u32 {
                    let mut count = 0u16;
                    for sy in 0..n {
                        for sx in 0..n {
                            let px = f64::from(x) + (f64::from(sx) + 0.5) / samples;
                            let py = f64::from(y) + (f64::from(sy) + 0.5) / samples;
                            let in_body = inside(px, py, [0.5, 0.25, 3.5, 2.75])
                                && !inside(px, py, [1.5, 1.0, 2.5, 2.0]);
                            if in_body && !inside(px, py, [1.0, 0.5, 3.25, 1.5]) {
                                count += 1;
                            }
                        }
                    }
                    assert_eq!(
                        image.to_pixels().unwrap()[(y * 4 + x) as usize],
                        quantize(count, c),
                        "n={n} pixel ({x},{y})"
                    );
                }
            }
            // An empty cut reproduces the plain render exactly.
            let plain = draw(&body, c);
            let same = render_difference(
                &body,
                &[],
                &[],
                Fill::Parity,
                plane(),
                p,
                c,
                l,
                bytes,
                &mut Cancellation::never(),
            )
            .unwrap();
            assert_eq!(plain.mask, same.mask);
        }
    }

    /// The per-pixel accumulation the span writer replaced: dense sample counts, then
    /// quantization and statistics over every pixel.
    fn dense_reference(
        body: &[Edge],
        cut: &[Edge],
        p: LcdProfile,
        c: RasterConfig,
    ) -> (Vec<u8>, PreviewStats) {
        let mut cancel = Cancellation::never();
        let mut work = Work {
            used: 0,
            limit: usize::MAX,
            cancel: &mut cancel,
        };
        let [width, height] = p.dimensions();
        let w = width as usize;
        let n = u32::from(c.samples_per_axis);
        let mut body_lines = Scanline::new(body, n, width, &mut work).unwrap();
        let mut cut_lines = Scanline::new(cut, n, width, &mut work).unwrap();
        let (mut body_spans, mut cut_spans, mut kept) = (Vec::new(), Vec::new(), Vec::new());
        let mut pixels = vec![0u8; w * height as usize];
        for y in 0..height {
            let mut row = vec![0u16; w];
            for sub in 0..n {
                let sy = f64::from(y) + (f64::from(sub) + 0.5) / f64::from(n);
                spans(
                    body_lines.crossings(sy, &mut work).unwrap(),
                    &mut body_spans,
                );
                union_spans(cut_lines.crossings(sy, &mut work).unwrap(), &mut cut_spans);
                subtract(&body_spans, &cut_spans, &mut kept);
                for &(left, right) in &kept {
                    for sample in left..right {
                        row[(sample / n) as usize] += 1;
                    }
                }
            }
            for (x, &count) in row.iter().enumerate() {
                pixels[y as usize * w + x] = quantize(count, c);
            }
        }
        (pixels.clone(), dense_statistics(&pixels, w))
    }

    /// Statistics over every dense pixel, as computed before span masks.
    #[allow(clippy::cast_possible_truncation)] // Small test dimensions, indices and levels.
    fn dense_statistics(pixels: &[u8], width: usize) -> PreviewStats {
        let mut result = PreviewStats {
            nonzero_pixels: 0,
            intensity_sum: 0,
            bounds: None,
        };
        for (i, &p) in pixels.iter().enumerate() {
            result.intensity_sum += u64::from(p);
            if p == 0 {
                continue;
            }
            result.nonzero_pixels += 1;
            let (column, row) = ((i % width) as u32, (i / width) as u32);
            result.bounds = Some(match result.bounds {
                Some([[x0, y0], [x1, y1]]) => [
                    [x0.min(column), y0.min(row)],
                    [x1.max(column + 1), y1.max(row + 1)],
                ],
                None => [[column, row], [column + 1, row + 1]],
            });
        }
        result
    }

    #[test]
    fn span_rows_match_the_dense_per_pixel_reference() {
        let mut seed = 0x9e37_79b9_7f4a_7c15_u64;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        let p =
            LcdProfile::try_new([29, 17], [0., 0.], [29., 17.], Orientation::default()).unwrap();
        let l = RasterLimits::default();
        let mut ring = |count: usize, out: &mut Vec<Edge>| {
            // Random closed rings, partly outside the panel and on exact pixel/sample boundaries.
            let points: Vec<[f64; 2]> = (0..count)
                .map(|_| {
                    let mut coordinate = |extent: f64| {
                        let value = f64::from(u32::try_from(next() % 1000).unwrap()) / 1000.
                            * (extent + 6.)
                            - 3.;
                        if next() % 4 == 0 {
                            (value * 4.).round() / 4.
                        } else {
                            value
                        }
                    };
                    [coordinate(29.), coordinate(17.)]
                })
                .collect();
            out.extend((0..count).map(|i| Edge {
                a: points[i],
                b: points[(i + 1) % count],
            }));
        };
        for case in 0..300 {
            let (mut body, mut cut) = (Vec::new(), Vec::new());
            for _ in 0..=case % 3 {
                ring(3 + case % 9, &mut body);
            }
            if case % 2 == 0 {
                ring(3 + case % 5, &mut cut);
            }
            if case % 7 == 0 {
                body.extend(rectangle(2., 2., 27., 15.));
            }
            let mut c = config([1, 2, 4, 8, 16][case % 5]);
            c.grayscale_levels = [256, 2, 5, 17][case % 4];
            c.minimum_gray = [0, 1, 77, 200, 255][case % 7 % 5];
            let bytes = preflight(p, c, l, body.len() + cut.len()).unwrap();
            let image = render_difference(
                &body,
                &cut,
                &[],
                Fill::Parity,
                plane(),
                p,
                c,
                l,
                bytes,
                &mut Cancellation::never(),
            )
            .unwrap();
            let (pixels, stats) = dense_reference(&body, &cut, p, c);
            assert_eq!(image.to_pixels().unwrap(), pixels, "case {case}");
            assert_eq!(image.stats, stats, "case {case}");
        }
    }

    #[test]
    fn binary_kept_bodies_fill_whole_pixels_and_the_model_stays_antialiased() {
        // TD-134: a model and two supports, one beside it and one across its edge. The model is
        // sampled alone; a pixel whose centre-most sample lies in a support is full; no support
        // edge is gray.
        let p = LcdProfile::try_new([9, 4], [0., 0.], [9., 4.], Orientation::default()).unwrap();
        let l = RasterLimits::default();
        let model = [0.3, 0.3, 3.7, 2.7];
        let supports = [[5.2, 0.6, 7.45, 3.3], [3.1, 1.2, 4.9, 1.85]];
        let rect = |r: [f64; 4]| rectangle(r[0], r[1], r[2], r[3]);
        let reversed = |r: [f64; 4]| rect(r).map(|e| Edge { a: e.b, b: e.a });
        let inside = |x: f64, y: f64, r: [f64; 4]| x >= r[0] && x < r[2] && y >= r[1] && y < r[3];
        for n in [2u8, 4, 16] {
            let mut c = config(n);
            c.kept = KeptSampling::Binary;
            let mut body = rect(model).to_vec();
            let mut kept = Vec::new();
            for &s in &supports {
                body.extend(rect(s));
                body.extend(reversed(s));
                kept.extend(rect(s));
            }
            let bytes = preflight(p, c, l, body.len() + kept.len()).unwrap();
            let image = render_difference(
                &body,
                &[],
                &kept,
                Fill::Winding(WindingRule::NonZero),
                plane(),
                p,
                c,
                l,
                bytes,
                &mut Cancellation::never(),
            )
            .unwrap();
            let pixels = image.to_pixels().unwrap();
            let samples = f64::from(n);
            let centre = (f64::from(n / 2) + 0.5) / samples;
            for y in 0..4u32 {
                for x in 0..9u32 {
                    let (cx, cy) = (f64::from(x) + centre, f64::from(y) + centre);
                    let expected = if supports.iter().any(|&s| inside(cx, cy, s)) {
                        255
                    } else {
                        let mut count = 0u16;
                        for sy in 0..n {
                            for sx in 0..n {
                                let px = f64::from(x) + (f64::from(sx) + 0.5) / samples;
                                let py = f64::from(y) + (f64::from(sy) + 0.5) / samples;
                                count += u16::from(inside(px, py, model));
                            }
                        }
                        quantize(count, c)
                    };
                    assert_eq!(
                        pixels[(y * 9 + x) as usize],
                        expected,
                        "n={n} pixel ({x},{y})"
                    );
                }
            }
            // Antialiased, the same layer has gray at the supports' edges.
            c.kept = KeptSampling::Antialiased;
            let all: Vec<Edge> = rect(model)
                .into_iter()
                .chain(supports.iter().flat_map(|&s| rect(s)))
                .collect();
            let bytes = preflight(p, c, l, all.len()).unwrap();
            let gray = render_difference(
                &all,
                &[],
                &[],
                Fill::Winding(WindingRule::NonZero),
                plane(),
                p,
                c,
                l,
                bytes,
                &mut Cancellation::never(),
            )
            .unwrap()
            .to_pixels()
            .unwrap();
            assert!(gray[6..9].iter().any(|&v| v != 0 && v != 255), "n={n}");
        }
    }

    #[test]
    fn overlapping_cut_rings_remove_their_union() {
        // TD-089: two overlapping holes. Under even-odd their shared square would stay filled;
        // the union removes it. A third, disjoint ring is removed as before.
        let p = LcdProfile::try_new([8, 8], [0., 0.], [8., 8.], Orientation::default()).unwrap();
        let c = config(1);
        let l = RasterLimits::default();
        let body = rectangle(0., 0., 8., 8.);
        let mut cut: Vec<Edge> = rectangle(1., 1., 4., 4.).into();
        cut.extend(rectangle(2., 2., 5., 5.));
        cut.extend(rectangle(6., 6., 7., 7.));
        let bytes = preflight(p, c, l, body.len() + cut.len()).unwrap();
        let image = render_difference(
            &body,
            &cut,
            &[],
            Fill::Parity,
            plane(),
            p,
            c,
            l,
            bytes,
            &mut Cancellation::never(),
        )
        .unwrap();
        let pixels = image.to_pixels().unwrap();
        let width = p.dimensions()[0] as usize;
        let at = |x: usize, y: usize| pixels[y * width + x];
        assert_eq!(at(2, 2), 0, "the shared square is removed");
        assert_eq!(at(1, 1), 0);
        assert_eq!(at(4, 4), 0);
        assert_eq!(at(6, 6), 0);
        assert_eq!(at(0, 0), 255);
        assert_eq!(at(5, 1), 255);
    }

    #[test]
    fn half_open_spans_and_rows() {
        let edges = rectangle(0.5, 0.5, 2.5, 2.5);
        let image = draw(&edges, config(1));
        assert_eq!(
            image.to_pixels().unwrap(),
            [255, 255, 0, 0, 255, 255, 0, 0, 0, 0, 0, 0]
        );
        assert_eq!(image.stats.bounds, Some([[0, 0], [2, 2]]));
    }
    #[test]
    fn aa_counts_and_holes() {
        for n in [2, 4, 8, 16] {
            let image = draw(&rectangle(0.5, 0., 1., 1.), config(n));
            assert_eq!(image.mask.pixel(0, 0), Some(128));
            assert_eq!(image.stats.intensity_sum, 128);
        }
        let mut edges = rectangle(-5., -5., 10., 10.).to_vec();
        edges.extend(rectangle(1., 1., 3., 2.));
        assert_eq!(
            draw(&edges, config(1)).to_pixels().unwrap(),
            [255, 255, 255, 255, 255, 0, 0, 255, 255, 255, 255, 255]
        );
    }
    /// The nearest level to the raised coverage by exhaustive search, ties to the higher level.
    fn nearest_level_oracle(count: u16, c: RasterConfig) -> u8 {
        if count == 0 {
            return 0;
        }
        let total = i128::from(c.samples_per_axis).pow(2);
        let levels = i128::from(c.grayscale_levels) - 1;
        let floor = i128::from(c.minimum_gray);
        // Intensity * total on the 0..=255 scale, compared with level / levels * 255.
        let target = floor * total + (255 - floor) * i128::from(count);
        let best = (0..=levels)
            .min_by_key(|&l| ((l * 255 * total - target * levels).abs(), -l))
            .unwrap();
        u8::try_from((best * 255 + levels / 2) / levels).unwrap()
    }
    #[test]
    fn minimum_gray_raises_partial_coverage_exactly() {
        for n in [1, 2, 4, 8, 16] {
            for levels in [2, 3, 5, 16, 17, 128, 256] {
                for minimum in [0, 1, 30, 77, 128, 254, 255] {
                    let mut c = config(n);
                    c.grayscale_levels = levels;
                    c.minimum_gray = minimum;
                    let total = u16::from(n).pow(2);
                    for count in 0..=total {
                        assert_eq!(
                            quantize(count, c),
                            nearest_level_oracle(count, c),
                            "n={n} levels={levels} minimum={minimum} count={count}"
                        );
                    }
                }
            }
        }
        // Half coverage: 100 + 155 / 2 = 177.5, rounded up at 256 levels.
        let mut c = config(4);
        c.minimum_gray = 100;
        let image = draw(&rectangle(0.5, 0., 1., 1.), c);
        assert_eq!(image.mask.pixel(0, 0), Some(178));
        // A single covered sample of 256 already reaches the minimum.
        let mut c = config(16);
        c.minimum_gray = 77;
        assert_eq!(quantize(1, c), 78);
        assert_eq!((quantize(0, c), quantize(256, c)), (0, 255));
    }
    #[test]
    fn every_quantizer_is_bounded_and_monotone() {
        for n in [1, 2, 4, 8, 16] {
            for levels in 2..=256 {
                for minimum in [0, 1, 128, 255] {
                    let mut c = config(n);
                    c.grayscale_levels = levels;
                    c.minimum_gray = minimum;
                    let total = u16::from(n).pow(2);
                    let values: Vec<_> = (0..=total).map(|v| quantize(v, c)).collect();
                    assert_eq!(values[0], 0);
                    assert_eq!(*values.last().unwrap(), 255);
                    assert!(values.windows(2).all(|p| p[0] <= p[1]));
                    // No nonzero value lies more than half a level step below the minimum.
                    let half_step = 255.0 / (f64::from(levels) - 1.0) / 2.0;
                    assert!(values[1..]
                        .iter()
                        .all(|&v| f64::from(v) >= f64::from(minimum) - half_step));
                }
            }
        }
    }
    #[test]
    fn compensation_matches_naive_grayscale_oracle() {
        let (w, h) = (7, 5);
        let original: Vec<u8> = (0..35)
            .map(|v| u8::try_from((v * 47) % 256).unwrap())
            .collect();
        for rx in [0, 1, 3, 8] {
            for ry in [0, 1, 3, 8] {
                for dilate in [false, true] {
                    let mut actual = original.clone();
                    let mut cancel = Cancellation::never();
                    let mut work = Work {
                        used: 0,
                        limit: 1_000_000,
                        cancel: &mut cancel,
                    };
                    let radii = [rx, ry];
                    let op = if dilate {
                        Compensation::Dilate(radii)
                    } else {
                        Compensation::Erode(radii)
                    };
                    morphology(&mut actual, w, h, op, &mut work).unwrap();
                    for y in 0..h {
                        for x in 0..w {
                            let mut value = if dilate { 0 } else { 255 };
                            for dy in -i32::from(ry)..=i32::from(ry) {
                                for dx in -i32::from(rx)..=i32::from(rx) {
                                    let xx = i32::try_from(x).unwrap() + dx;
                                    let yy = i32::try_from(y).unwrap() + dy;
                                    let sample = if xx < 0 || yy < 0 || xx >= 7 || yy >= 5 {
                                        0
                                    } else {
                                        original[usize::try_from(yy).unwrap() * w
                                            + usize::try_from(xx).unwrap()]
                                    };
                                    value = if dilate {
                                        value.max(sample)
                                    } else {
                                        value.min(sample)
                                    };
                                }
                            }
                            assert_eq!(
                                actual[y * w + x],
                                value,
                                "x={x} y={y} rx={rx} ry={ry} dilation={dilate}"
                            );
                        }
                    }
                }
            }
        }
    }
    #[test]
    fn limits_and_odd_crossings_are_errors() {
        let l = RasterLimits {
            max_payload_bytes: 0,
            ..RasterLimits::default()
        };
        assert!(matches!(
            preflight(profile(), config(1), l, 0),
            Err(RasterError::Limit {
                resource: "payload bytes",
                ..
            })
        ));
        let e = [Edge {
            a: [1., 0.],
            b: [1., 3.],
        }];
        let l = RasterLimits::default();
        let bytes = preflight(profile(), config(1), l, 1).unwrap();
        assert!(matches!(
            render(
                &e,
                plane(),
                profile(),
                config(1),
                l,
                bytes,
                &mut Cancellation::never()
            ),
            Err(RasterError::InvalidGeometry)
        ));
    }
}
