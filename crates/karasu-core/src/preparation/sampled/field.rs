//! Adaptive evaluation of the inward cavity field `F = d - t` on a uniform world grid.
//!
//! `d` is the Euclidean distance to the placed source surface and `t` the wall thickness. The
//! cavity is `{inside and d > t}`. Distance is 1-Lipschitz, so one exact query at a block centre
//! bounds `d` on the whole block ball of radius `r`: `d(c) - r <= d <= d(c) + r`. Blocks whose
//! bound lies entirely above or below `t` (beyond the query margin) are decided without sampling;
//! a block whose ball cannot touch the surface (`d(c) > r`) has one inside/outside state, decided
//! by an exact winding query at its centre (inside is a positive winding number, so overlapping
//! bodies are hollowed as their union, TD-092 H2; see [`Bvh::vertical_winding`]). Only leaf
//! blocks straddling the iso-level are sampled,
//! and every leaf is inside the source. Leaf diameter is chosen below `t`, so a leaf that may
//! touch the surface is always entirely wall material and is never sampled.
//!
//! Work items are fixed top-level blocks processed independently and merged in item order, so
//! the result is identical for any thread count.

use super::{
    bvh::Bvh,
    tetra::{polygonize_cell, CornerEdge},
};
use crate::cancellation::{Cancellation, Cancelled, Checkpoint};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        Mutex,
    },
};

const SQRT_3: f64 = 1.732_050_807_568_877_2;

/// Smallest admitted interpolation parameter along a crossed grid edge. Keeping iso-vertices this
/// far from grid vertices keeps distinct iso-vertices distinct after rounding; any point of the
/// edge satisfies the a priori band, so the clamp does not weaken the certificate.
const MIN_EDGE_PARAMETER: f64 = 1.0 / 1_048_576.0; // 2^-20

/// The regions left solid (owner, 2026-11-02), as the field sees them in world space.
///
/// Blockers are given in the plan's source frame; a world point `p` is taken back to it by the
/// placement's inverse, `q = A⁻¹ (p - t)`, where each blocker's bound is evaluated. Scaled by the
/// placement's least stretch `σ_min(A)`, the least of those bounds stays 1-Lipschitz in world
/// space (`|q - q'| ≤ |p - p'| / σ_min`) and keeps its zero set: the blockers' boundaries.
#[derive(Clone, Debug, Default)]
pub(super) struct Blockers {
    regions: Vec<crate::preparation::HollowBlocker>,
    inverse: [[f64; 3]; 3],
    translation: [f64; 3],
    stretch: f64,
}

impl Blockers {
    /// `blockers` in the source frame placed by `linear` (rows, acting on columns) and
    /// `translation`; `None` for a singular placement.
    pub(super) fn new(
        blockers: Vec<crate::preparation::HollowBlocker>,
        linear: [[f64; 3]; 3],
        translation: [f64; 3],
    ) -> Option<Self> {
        let a = linear;
        let det = a[0][0] * (a[1][1] * a[2][2] - a[1][2] * a[2][1])
            - a[0][1] * (a[1][0] * a[2][2] - a[1][2] * a[2][0])
            + a[0][2] * (a[1][0] * a[2][1] - a[1][1] * a[2][0]);
        if !(det.is_finite() && det != 0.0) {
            return None;
        }
        let inverse: [[f64; 3]; 3] = std::array::from_fn(|r| {
            std::array::from_fn(|c| {
                // The adjugate's entry (r, c) is the cofactor of (c, r).
                let (r1, r2) = ((c + 1) % 3, (c + 2) % 3);
                let (c1, c2) = ((r + 1) % 3, (r + 2) % 3);
                (a[r1][c1] * a[r2][c2] - a[r1][c2] * a[r2][c1]) / det
            })
        });
        Some(Self {
            regions: blockers,
            inverse,
            translation,
            stretch: least_stretch(linear),
        })
    }

    /// The least blocker bound at world point `p`, 1-Lipschitz: negative inside a blocker,
    /// positive outside every one; `+∞` without blockers.
    fn signed(&self, p: [f64; 3]) -> f64 {
        if self.regions.is_empty() {
            return f64::INFINITY;
        }
        let d: [f64; 3] = std::array::from_fn(|k| p[k] - self.translation[k]);
        let q: [f64; 3] = std::array::from_fn(|r| {
            self.inverse[r][0] * d[0] + self.inverse[r][1] * d[1] + self.inverse[r][2] * d[2]
        });
        let least = self
            .regions
            .iter()
            .map(|b| b.signed(q))
            .fold(f64::INFINITY, f64::min);
        least * self.stretch
    }
}

/// The least singular value of `a`, slightly under it: the square root of the least eigenvalue
/// of `AᵀA`, found in closed form for a symmetric 3 × 3 matrix and shrunk by a millionth for its
/// rounding, so the scaled bound never exceeds the true one.
#[allow(clippy::many_single_char_names)] // The closed form's usual names.
fn least_stretch(a: [[f64; 3]; 3]) -> f64 {
    let m: [[f64; 3]; 3] =
        std::array::from_fn(|i| std::array::from_fn(|j| (0..3).map(|k| a[k][i] * a[k][j]).sum()));
    let p1 = m[0][1] * m[0][1] + m[0][2] * m[0][2] + m[1][2] * m[1][2];
    let trace = m[0][0] + m[1][1] + m[2][2];
    let least = if p1 == 0.0 {
        m[0][0].min(m[1][1]).min(m[2][2])
    } else {
        let q = trace / 3.0;
        let p2 = (m[0][0] - q).powi(2) + (m[1][1] - q).powi(2) + (m[2][2] - q).powi(2) + 2.0 * p1;
        let p = (p2 / 6.0).sqrt();
        let b: [[f64; 3]; 3] = std::array::from_fn(|i| {
            std::array::from_fn(|j| (m[i][j] - if i == j { q } else { 0.0 }) / p)
        });
        let det_b = b[0][0] * (b[1][1] * b[2][2] - b[1][2] * b[2][1])
            - b[0][1] * (b[1][0] * b[2][2] - b[1][2] * b[2][0])
            + b[0][2] * (b[1][0] * b[2][1] - b[1][1] * b[2][0]);
        let phi = (det_b / 2.0).clamp(-1.0, 1.0).acos() / 3.0;
        // The eigenvalues are q + 2p cos(phi + 2kπ/3); the least is k = 1.
        q + 2.0 * p * (phi + 2.0 * std::f64::consts::PI / 3.0).cos()
    };
    least.max(0.0).sqrt() * (1.0 - 1e-6)
}

/// Uniform world grid; vertex `(i, j, k)` is at `origin + (i, j, k) * cell`.
#[derive(Clone, Copy, Debug)]
pub(super) struct Grid {
    pub(super) origin: [f64; 3],
    pub(super) cell: f64,
    /// Vertex count per axis; at least two.
    pub(super) vertices: [u64; 3],
}

impl Grid {
    // u64 -> f64 is exact for every admitted index (< 2^53, checked when the grid is built).
    #[allow(clippy::cast_precision_loss)]
    pub(super) fn position(&self, v: [u64; 3]) -> [f64; 3] {
        std::array::from_fn(|a| self.origin[a] + v[a] as f64 * self.cell)
    }

    fn linear(&self, v: [u64; 3]) -> u64 {
        v[0] + self.vertices[0] * (v[1] + self.vertices[1] * v[2])
    }
}

/// Decision parameters shared by every block.
#[derive(Clone, Copy, Debug)]
pub(super) struct FieldSpec {
    pub(super) thickness: f64,
    /// Absolute decision margin; at least the distance-query error budget.
    pub(super) margin: f64,
    /// Leaf block side in cells (power of two).
    pub(super) leaf: u64,
    /// Work-item block side in cells (power-of-two multiple of `leaf`).
    pub(super) item: u64,
    pub(super) max_queries: u64,
    pub(super) max_triangles: u64,
    pub(super) max_payload_bytes: u64,
    /// Rebuild the solid (TD-092 R9) instead of hollowing it: the field is the distance signed by
    /// the winding number (positive inside), its zero level the solid's surface; `thickness` is
    /// unused.
    pub(super) rebuild: bool,
}

#[derive(Debug)]
pub(super) enum FieldError {
    Stopped,
    Resource(&'static str),
    Uncertain(&'static str),
    /// A cavity-candidate region has negative winding (inside-out or unenclosed reversed shell).
    NegativeWinding,
}

/// Iso-surface fragment of one work item, with item-local vertex indices.
#[derive(Debug, Default)]
pub(super) struct ItemOutput {
    /// Global grid-edge key and interpolated world position.
    pub(super) vertices: Vec<(u64, [f64; 3])>,
    pub(super) triangles: Vec<[u32; 3]>,
}

/// Deterministic totals for diagnostics and admission.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct FieldStats {
    pub(super) items: u64,
    pub(super) distance_queries: u64,
    pub(super) parity_queries: u64,
    pub(super) leaves: u64,
}

#[derive(Default)]
struct Counters {
    queries: AtomicU64,
    parity: AtomicU64,
    leaves: AtomicU64,
    triangles: AtomicU64,
    bytes: AtomicU64,
    stop: AtomicBool,
}

struct Context<'a> {
    bvh: &'a Bvh,
    grid: Grid,
    spec: FieldSpec,
    blockers: &'a Blockers,
    counters: &'a Counters,
}

struct Builder {
    local: HashMap<u64, u32>,
    out: ItemOutput,
}

fn charge(
    counter: &AtomicU64,
    amount: u64,
    limit: u64,
    name: &'static str,
) -> Result<(), FieldError> {
    let total = counter
        .fetch_add(amount, Ordering::Relaxed)
        .saturating_add(amount);
    if total > limit {
        return Err(FieldError::Resource(name));
    }
    Ok(())
}

impl Context<'_> {
    fn item(&self, corner: [u64; 3]) -> Result<ItemOutput, FieldError> {
        let mut builder = Builder {
            local: HashMap::new(),
            out: ItemOutput::default(),
        };
        let mut hint = 0;
        self.block(corner, self.spec.item, false, &mut hint, &mut builder)?;
        let bytes =
            builder.out.vertices.len() as u64 * 32 + builder.out.triangles.len() as u64 * 12;
        charge(
            &self.counters.bytes,
            bytes,
            self.spec.max_payload_bytes,
            "field payload",
        )?;
        Ok(builder.out)
    }

    // Block sizes and corners are grid indices below 2^53, exact in binary64.
    #[allow(clippy::cast_precision_loss)]
    fn block(
        &self,
        corner: [u64; 3],
        size: u64,
        known_inside: bool,
        hint: &mut usize,
        builder: &mut Builder,
    ) -> Result<(), FieldError> {
        if self.counters.stop.load(Ordering::Relaxed) {
            return Err(FieldError::Stopped);
        }
        let grid = &self.grid;
        if (0..3).any(|a| corner[a] + 1 >= grid.vertices[a]) {
            return Ok(());
        }
        let half = size as f64 / 2.0;
        let center: [f64; 3] =
            std::array::from_fn(|a| grid.origin[a] + (corner[a] as f64 + half) * grid.cell);
        let radius = size as f64 * grid.cell * SQRT_3 / 2.0;
        let (squared, slot) = self.bvh.nearest(center, *hint);
        *hint = slot;
        charge(
            &self.counters.queries,
            1,
            self.spec.max_queries,
            "distance queries",
        )?;
        let d = squared.sqrt();
        let (t, m) = (self.spec.thickness, self.spec.margin);
        let one_sided = d > radius + m;
        if self.spec.rebuild {
            // A ball the surface does not touch has one sign: no zero crossing in it.
            if one_sided {
                return Ok(());
            }
            if size == self.spec.leaf {
                return self.leaf(corner, hint, builder);
            }
            let h = size / 2;
            for dz in 0..2 {
                for dy in 0..2 {
                    for dx in 0..2 {
                        let child = [corner[0] + dx * h, corner[1] + dy * h, corner[2] + dz * h];
                        self.block(child, h, false, hint, builder)?;
                    }
                }
            }
            return Ok(());
        }
        if d + radius + m < t {
            // Every point is closer than t to the surface: wall material or exterior.
            return Ok(());
        }
        // The blockers (owner, 2026-11-02): a block inside one has no cavity; one outside every
        // one is decided as without them; one their boundary may cross is sampled.
        let blocked = self.blockers.signed(center);
        if blocked + radius + m < 0.0 {
            return Ok(());
        }
        let clear = blocked - radius - m > 0.0;
        let mut inside = known_inside;
        if one_sided {
            if !inside {
                // The ball touches no face, so the winding number is constant on it. Material is a
                // positive winding number: inside overlapping bodies too (the assembly subtracts
                // such a cavity once per body); a negative one (a reversed shell outside every
                // body) cannot be hollowed.
                self.counters.parity.fetch_add(1, Ordering::Relaxed);
                match self.bvh.vertical_winding(center) {
                    Some(0) => return Ok(()),
                    Some(w) if w > 0 => inside = true,
                    Some(_) => return Err(FieldError::NegativeWinding),
                    None => {
                        return Err(FieldError::Uncertain("winding query point on the surface"))
                    }
                }
            }
            if d - radius - m > t && clear {
                // Entirely cavity interior; no iso-surface crosses this block.
                return Ok(());
            }
            if size == self.spec.leaf {
                return self.leaf(corner, hint, builder);
            }
        } else if size == self.spec.leaf {
            // The leaf diameter is below t - 2m, so this branch already returned above.
            return Err(FieldError::Uncertain("leaf block cannot be decided"));
        }
        let h = size / 2;
        for dz in 0..2 {
            for dy in 0..2 {
                for dx in 0..2 {
                    let child = [corner[0] + dx * h, corner[1] + dy * h, corner[2] + dz * h];
                    self.block(child, h, inside, hint, builder)?;
                }
            }
        }
        Ok(())
    }

    // Leaf sides are at most 9 vertices, so local indices always fit usize.
    #[allow(clippy::cast_possible_truncation)]
    fn leaf(
        &self,
        corner: [u64; 3],
        hint: &mut usize,
        builder: &mut Builder,
    ) -> Result<(), FieldError> {
        self.counters.leaves.fetch_add(1, Ordering::Relaxed);
        let grid = &self.grid;
        let n = self.spec.leaf + 1;
        let side = n as usize;
        let mut values = vec![f64::NAN; side * side * side];
        let mut evaluated = 0;
        for k in 0..n {
            for j in 0..n {
                for i in 0..n {
                    let v = [corner[0] + i, corner[1] + j, corner[2] + k];
                    if (0..3).any(|a| v[a] >= grid.vertices[a]) {
                        continue;
                    }
                    let position = grid.position(v);
                    let (squared, slot) = self.bvh.nearest(position, *hint);
                    *hint = slot;
                    evaluated += 1;
                    let value = if self.spec.rebuild {
                        let index = (i as usize) + side * ((j as usize) + side * (k as usize));
                        let before = [i > 0, j > 0, k > 0];
                        self.signed(position, squared.sqrt(), &values, index, side, before)
                    } else {
                        // The whole leaf is inside the source, so F = d - t; zero counts as
                        // material. A blocker takes the cavity away: the least of the two.
                        (squared.sqrt() - self.spec.thickness).min(self.blockers.signed(position))
                    };
                    values[(i as usize) + side * ((j as usize) + side * (k as usize))] = value;
                }
            }
        }
        charge(
            &self.counters.queries,
            evaluated,
            self.spec.max_queries,
            "distance queries",
        )?;
        let first_triangle = builder.out.triangles.len();
        let value = |c: [u64; 3]| {
            values[(c[0] as usize) + side * ((c[1] as usize) + side * (c[2] as usize))]
        };
        for k in 0..self.spec.leaf {
            for j in 0..self.spec.leaf {
                for i in 0..self.spec.leaf {
                    let base = [corner[0] + i, corner[1] + j, corner[2] + k];
                    if (0..3).any(|a| base[a] + 1 >= grid.vertices[a]) {
                        continue;
                    }
                    let cell: [f64; 8] = std::array::from_fn(|c| {
                        value([
                            i + (c as u64 & 1),
                            j + (c as u64 >> 1 & 1),
                            k + (c as u64 >> 2 & 1),
                        ])
                    });
                    let positive = cell.iter().filter(|&&f| f > 0.0).count();
                    if positive == 0 || positive == 8 {
                        continue;
                    }
                    let mut failure = None;
                    polygonize_cell(&cell, |triangle| {
                        if failure.is_some() {
                            return;
                        }
                        let mut ids = [0u32; 3];
                        for (slot, e) in triangle.into_iter().enumerate() {
                            match self.vertex(base, &cell, e, builder) {
                                Ok(id) => ids[slot] = id,
                                Err(error) => {
                                    failure = Some(error);
                                    return;
                                }
                            }
                        }
                        builder.out.triangles.push(ids);
                    });
                    if let Some(error) = failure {
                        return Err(error);
                    }
                }
            }
        }
        let emitted = (builder.out.triangles.len() - first_triangle) as u64;
        charge(
            &self.counters.triangles,
            emitted,
            self.spec.max_triangles,
            "cavity triangles",
        )
    }

    /// The rebuild field at a leaf vertex: the distance `d`, positive inside (a positive winding
    /// number), negative outside, zero on the surface (counted outside). The distance is
    /// 1-Lipschitz, so the surface cannot pass between two grid neighbours whose distances sum to
    /// more than their spacing: a neighbour already signed (`before`: the previous vertex along x,
    /// y and z exists) then gives the sign without a ray.
    fn signed(
        &self,
        position: [f64; 3],
        d: f64,
        values: &[f64],
        index: usize,
        side: usize,
        before: [bool; 3],
    ) -> f64 {
        let reach = self.grid.cell + 2.0 * self.spec.margin;
        let known = before
            .iter()
            .zip([1, side, side * side])
            .find_map(|(&exists, step)| {
                let other = exists.then(|| values[index - step])?;
                (other != 0.0 && !other.is_nan() && other.abs() + d > reach).then(|| other.signum())
            });
        if let Some(sign) = known {
            return sign * d;
        }
        self.counters.parity.fetch_add(1, Ordering::Relaxed);
        match self.bvh.vertical_winding(position) {
            Some(w) if w > 0 => d,
            Some(_) => -d,
            None => 0.0,
        }
    }

    #[allow(clippy::many_single_char_names)] // Edge (e), corners (a, b), parameter (s), point (p).
    fn vertex(
        &self,
        base: [u64; 3],
        cell: &[f64; 8],
        e: CornerEdge,
        builder: &mut Builder,
    ) -> Result<u32, FieldError> {
        let offset = |c: usize| [c as u64 & 1, c as u64 >> 1 & 1, c as u64 >> 2 & 1];
        let lower = offset(e.lower);
        let lower = [base[0] + lower[0], base[1] + lower[1], base[2] + lower[2]];
        let direction = (e.upper ^ e.lower) as u64;
        let key = self.grid.linear(lower) * 8 + direction;
        if let Some(&id) = builder.local.get(&key) {
            return Ok(id);
        }
        let upper = offset(e.upper);
        let upper = [base[0] + upper[0], base[1] + upper[1], base[2] + upper[2]];
        let (fa, fb) = (cell[e.lower], cell[e.upper]);
        let s = (fa / (fa - fb)).clamp(MIN_EDGE_PARAMETER, 1.0 - MIN_EDGE_PARAMETER);
        if !s.is_finite() {
            return Err(FieldError::Uncertain("non-finite edge interpolation"));
        }
        let (a, b) = (self.grid.position(lower), self.grid.position(upper));
        let p = std::array::from_fn(|axis| a[axis] + s * (b[axis] - a[axis]));
        let id = u32::try_from(builder.out.vertices.len())
            .map_err(|_| FieldError::Resource("item vertex index"))?;
        builder.local.insert(key, id);
        builder.out.vertices.push((key, p));
        Ok(id)
    }
}

/// Evaluate all work items, in parallel on `threads` threads (including the caller's).
///
/// Cancellation is polled on the calling thread between its items and stops the other threads at
/// their next block. `progress(done, total)` reports completed items from the calling thread.
pub(super) fn evaluate(
    bvh: &Bvh,
    grid: Grid,
    spec: FieldSpec,
    blockers: &Blockers,
    threads: usize,
    cancel: &mut Cancellation<'_>,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<(Vec<ItemOutput>, FieldStats), FieldOutcome> {
    let cells: [u64; 3] = std::array::from_fn(|a| grid.vertices[a] - 1);
    let counts: [u64; 3] = std::array::from_fn(|a| cells[a].div_ceil(spec.item));
    let total = counts[0]
        .checked_mul(counts[1])
        .and_then(|v| v.checked_mul(counts[2]))
        .ok_or(FieldOutcome::Failed(FieldError::Resource(
            "work item count",
        )))?;
    let total_usize = usize::try_from(total)
        .map_err(|_| FieldOutcome::Failed(FieldError::Resource("work item count")))?;
    let item_corner = |index: u64| {
        let x = index % counts[0];
        let y = index / counts[0] % counts[1];
        let z = index / (counts[0] * counts[1]);
        [x * spec.item, y * spec.item, z * spec.item]
    };
    let counters = Counters::default();
    let context = Context {
        bvh,
        grid,
        spec,
        blockers,
        counters: &counters,
    };
    let mut slots: Vec<Mutex<Option<Result<ItemOutput, FieldError>>>> = Vec::new();
    slots
        .try_reserve_exact(total_usize)
        .map_err(|_| FieldOutcome::Failed(FieldError::Resource("work item allocation")))?;
    slots.extend((0..total_usize).map(|_| Mutex::new(None)));
    let next = AtomicUsize::new(0);
    let done = AtomicU64::new(0);
    let run = |index: usize| {
        let result = context.item(item_corner(index as u64));
        if result.is_err() {
            counters.stop.store(true, Ordering::Relaxed);
        }
        if let Ok(mut slot) = slots[index].lock() {
            *slot = Some(result);
        }
        done.fetch_add(1, Ordering::Relaxed);
    };
    let mut cancelled: Option<Cancelled> = None;
    std::thread::scope(|scope| {
        for _ in 1..threads.max(1) {
            scope.spawn(|| loop {
                if counters.stop.load(Ordering::Relaxed) {
                    break;
                }
                let index = next.fetch_add(1, Ordering::Relaxed);
                if index >= total_usize {
                    break;
                }
                run(index);
            });
        }
        loop {
            if let Err(error) = cancel.check(Checkpoint::HollowWork) {
                cancelled = Some(error);
                counters.stop.store(true, Ordering::Relaxed);
                break;
            }
            if counters.stop.load(Ordering::Relaxed) {
                break;
            }
            let index = next.fetch_add(1, Ordering::Relaxed);
            if index >= total_usize {
                break;
            }
            run(index);
            progress(done.load(Ordering::Relaxed), total);
        }
    });
    if let Some(error) = cancelled {
        return Err(FieldOutcome::Cancelled(error));
    }
    let outputs = collect(slots)?;
    progress(total, total);
    let stats = FieldStats {
        items: total,
        distance_queries: counters.queries.load(Ordering::Relaxed),
        parity_queries: counters.parity.load(Ordering::Relaxed),
        leaves: counters.leaves.load(Ordering::Relaxed),
    };
    Ok((outputs, stats))
}

/// Terminal field outcome other than success.
#[derive(Debug)]
pub(super) enum FieldOutcome {
    Cancelled(Cancelled),
    Failed(FieldError),
}

/// Move item results out in item order; the lowest-indexed real failure wins over placeholders.
fn collect(
    slots: Vec<Mutex<Option<Result<ItemOutput, FieldError>>>>,
) -> Result<Vec<ItemOutput>, FieldOutcome> {
    let total_usize = slots.len();
    let mut outputs = Vec::new();
    outputs
        .try_reserve_exact(total_usize)
        .map_err(|_| FieldOutcome::Failed(FieldError::Resource("work item allocation")))?;
    let mut first_error = None;
    for slot in slots {
        match slot.into_inner().ok().flatten() {
            Some(Ok(output)) => outputs.push(output),
            Some(Err(FieldError::Stopped)) | None => {
                if first_error.is_none() {
                    first_error = Some(FieldError::Stopped);
                }
            }
            Some(Err(error)) => {
                // Report the lowest-indexed real failure; a Stopped placeholder never wins.
                if matches!(first_error, None | Some(FieldError::Stopped)) {
                    first_error = Some(error);
                }
            }
        }
    }
    if let Some(error) = first_error {
        return Err(FieldOutcome::Failed(error));
    }
    Ok(outputs)
}
