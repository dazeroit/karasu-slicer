//! Cap refinement and fairing (TD-092 r5), after Liepa, "Filling holes in meshes" (2003).
//!
//! - **Scale:** every rim corner carries the mean length of the mesh edges at its vertex; a new
//!   vertex carries the mean of its triangle's corners.
//! - **Refinement:** a triangle whose centroid `c` lies farther than `scale / sqrt(2)` from each
//!   corner `m`, for the larger of the scales of `c` and `m`, is split at its centroid, in
//!   rounds; after each round, interior edges are swapped while the two angles opposite an edge
//!   sum to more than π (the Delaunay criterion on the two triangles), unless the other diagonal
//!   is already an edge. Rounds stop when nothing splits.
//! - **Fairing:** the new vertices are first moved to the average of their neighbours (the
//!   uniform umbrella operator, a discrete membrane), then to a discrete thin plate, the umbrella
//!   applied twice set to zero (Kobbelt, "Discrete fairing", 1997): `x <- x - U²(x) / ν`. The
//!   umbrella of a rim corner includes its mesh neighbours outside the cap, so the cap continues
//!   the surface's slope across the rim instead of spanning it flat. The rim and its neighbours
//!   stay fixed.
//! - **Creases:** a planar rim (its corners within [`PLANAR_RIM`] of its size from its plane)
//!   from which the surface leaves at [`CREASE_SINE`] or more on average, as at the edge of a
//!   missing box face or cylinder top, keeps the flat membrane: continuing a crease's slope would
//!   raise a dome (13 mm on a 300-sided prism of radius 10 mm, TD-092 r9). The thin plate's cap
//!   is kept as the alternative tried first when the flat cap meets the mesh. Both run by Gauss-Seidel sweeps in index order until no vertex moves more than
//!   [`FAIR_TOLERANCE`] of the mean scale.
//!
//! Everything runs in a fixed order, so results do not depend on the thread count. The cap is
//! validated by the caller like any other.

use super::{reserve, FillError};
use crate::cancellation::{Cancellation, Checkpoint};
use std::collections::{BTreeMap, BTreeSet};

type Vec3 = [f64; 3];

/// Most refinement rounds.
const MAX_ROUNDS: usize = 64;
/// Most edge-swap sweeps after each round.
const MAX_SWAP_SWEEPS: usize = 64;
/// Most fairing sweeps.
const MAX_FAIR_SWEEPS: usize = 4000;
/// Fairing stops when no vertex moves more than this fraction of the mean scale.
pub(super) const FAIR_TOLERANCE: f64 = 1e-6;
/// A rim is planar when no corner is farther from its plane than this fraction of its size
/// (the diagonal of its bounds): above the rounding of single-precision STL coordinates.
const PLANAR_RIM: f64 = 1e-4;
/// The mean sine of the angle between the rim's plane and its corners' edges to the mesh outside
/// it, at or above which a planar rim is a crease: 45 degrees.
const CREASE_SINE: f64 = std::f64::consts::FRAC_1_SQRT_2;
/// Most triangles of one cap.
pub(super) const MAX_CAP_TRIANGLES: usize = 4_000_000;

fn sub(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: Vec3, b: Vec3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: Vec3, b: Vec3) -> Vec3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn distance(a: Vec3, b: Vec3) -> f64 {
    dot(sub(a, b), sub(a, b)).sqrt()
}

/// The angle at `apex` between the directions to `a` and `b`, in radians.
fn angle(apex: Vec3, a: Vec3, b: Vec3) -> f64 {
    let (u, v) = (sub(a, apex), sub(b, apex));
    let denominator = (dot(u, u) * dot(v, v)).sqrt();
    if denominator > 0.0 {
        (dot(u, v) / denominator).clamp(-1.0, 1.0).acos()
    } else {
        0.0
    }
}

/// A cap being refined: positions (the rim's first), scales and triangles over them.
pub(super) struct Patch {
    pub(super) positions: Vec<Vec3>,
    scales: Vec<f64>,
    pub(super) triangles: Vec<[usize; 3]>,
    /// The rim's corners, never moved.
    fixed: usize,
    /// Each rim corner's mesh neighbours that are not rim corners, for the thin plate.
    outside: Vec<Vec<Vec3>>,
}

impl Patch {
    /// A patch over the rim corners `positions[..fixed]` with their `scales`; positions past
    /// `fixed` (a fan's centre) take the mean scale.
    /// `outside[j]` are rim corner `j`'s mesh neighbours outside the rim.
    pub(super) fn new(
        positions: Vec<Vec3>,
        scales: &[f64],
        triangles: Vec<[usize; 3]>,
        outside: Vec<Vec<Vec3>>,
    ) -> Self {
        let fixed = scales.len();
        #[allow(clippy::cast_precision_loss)] // Rim sizes are far below 2^52.
        let mean = scales.iter().sum::<f64>() / fixed.max(1) as f64;
        let mut all = scales.to_vec();
        all.resize(positions.len(), mean);
        Self {
            positions,
            scales: all,
            triangles,
            fixed,
            outside,
        }
    }

    /// Refine, relax and fair (see the module documentation). `existing(a, b)` tells whether
    /// the rim corners `a` and `b` are already joined by a mesh edge, which a swap must not
    /// duplicate. A cap kept flat at a crease also returns the positions the thin plate gives
    /// it, an alternative for when the flat cap meets the mesh.
    pub(super) fn refine_and_fair(
        &mut self,
        existing: &dyn Fn(usize, usize) -> bool,
        cancel: &mut Cancellation<'_>,
    ) -> Result<Option<Vec<Vec3>>, FillError> {
        let alpha = std::f64::consts::SQRT_2;
        for _ in 0..MAX_ROUNDS {
            cancel.check(Checkpoint::PreparationRecords)?;
            let count = self.triangles.len();
            let mut split = false;
            for t in 0..count {
                let corners = self.triangles[t];
                let at = corners.map(|v| self.positions[v]);
                let centre: Vec3 =
                    std::array::from_fn(|axis| (at[0][axis] + at[1][axis] + at[2][axis]) / 3.0);
                let scale = corners.iter().map(|&v| self.scales[v]).sum::<f64>() / 3.0;
                if corners.iter().all(|&m| {
                    alpha * distance(centre, self.positions[m]) > scale.max(self.scales[m])
                }) {
                    if self.triangles.len() + 2 > MAX_CAP_TRIANGLES {
                        return Err(FillError::Resource("refined cap triangles"));
                    }
                    let middle = self.positions.len();
                    reserve(&mut self.positions, 1)?;
                    self.positions.push(centre);
                    self.scales.push(scale);
                    let [first, second, third] = corners;
                    self.triangles[t] = [first, second, middle];
                    reserve(&mut self.triangles, 2)?;
                    self.triangles.push([second, third, middle]);
                    self.triangles.push([third, first, middle]);
                    split = true;
                }
            }
            if !split {
                break;
            }
            self.relax(existing, cancel)?;
        }
        self.relax(existing, cancel)?;
        self.fair(cancel)
    }

    /// Swap interior edges whose opposite angles sum to more than π.
    fn relax(
        &mut self,
        existing: &dyn Fn(usize, usize) -> bool,
        cancel: &mut Cancellation<'_>,
    ) -> Result<(), FillError> {
        for _ in 0..MAX_SWAP_SWEEPS {
            cancel.check(Checkpoint::PreparationRecords)?;
            let mut uses: BTreeMap<(usize, usize), Vec<usize>> = BTreeMap::new();
            for (t, tri) in self.triangles.iter().enumerate() {
                for k in 0..3 {
                    let (a, b) = (tri[k], tri[(k + 1) % 3]);
                    uses.entry((a.min(b), a.max(b))).or_default().push(t);
                }
            }
            let mut touched = vec![false; self.triangles.len()];
            let mut created = BTreeSet::new();
            let mut swapped = false;
            for (&(a, b), faces) in &uses {
                let [first, second] = faces[..] else {
                    continue;
                };
                if touched[first] || touched[second] {
                    continue;
                }
                // `first` runs from `start` to `end`, `second` back.
                let opposite = |t: usize, from: usize, to: usize| {
                    let tri = self.triangles[t];
                    (0..3)
                        .find(|&k| tri[k] == from && tri[(k + 1) % 3] == to)
                        .map(|k| tri[(k + 2) % 3])
                };
                let (start, end) = if opposite(first, a, b).is_some() {
                    (a, b)
                } else {
                    (b, a)
                };
                let (Some(left), Some(right)) =
                    (opposite(first, start, end), opposite(second, end, start))
                else {
                    continue;
                };
                let at = |v: usize| self.positions[v];
                let sum =
                    angle(at(left), at(start), at(end)) + angle(at(right), at(start), at(end));
                if sum <= std::f64::consts::PI + 1e-12 || left == right {
                    continue;
                }
                let diagonal = (left.min(right), left.max(right));
                if uses.contains_key(&diagonal)
                    || created.contains(&diagonal)
                    || (left < self.fixed && right < self.fixed && existing(left, right))
                {
                    continue;
                }
                // The new triangles must face like the old pair, without a fold between them.
                let normal =
                    |t: [usize; 3]| cross(sub(at(t[1]), at(t[0])), sub(at(t[2]), at(t[0])));
                let old = {
                    let (m, n) = (
                        normal(self.triangles[first]),
                        normal(self.triangles[second]),
                    );
                    [m[0] + n[0], m[1] + n[1], m[2] + n[2]]
                };
                let (one, two) = (normal([left, start, right]), normal([right, end, left]));
                if dot(one, two) <= 0.0 || dot(one, old) <= 0.0 || dot(two, old) <= 0.0 {
                    continue;
                }
                created.insert(diagonal);
                self.triangles[first] = [left, start, right];
                self.triangles[second] = [right, end, left];
                touched[first] = true;
                touched[second] = true;
                swapped = true;
            }
            if !swapped {
                break;
            }
        }
        Ok(())
    }

    /// Move the new vertices to the average of their neighbours until they settle; returns the
    /// thin plate's positions of a cap kept flat at a crease.
    fn fair(&mut self, cancel: &mut Cancellation<'_>) -> Result<Option<Vec<Vec3>>, FillError> {
        let count = self.positions.len();
        if count == self.fixed {
            return Ok(None);
        }
        let mut neighbours: Vec<Vec<usize>> = vec![Vec::new(); count];
        for tri in &self.triangles {
            for k in 0..3 {
                let (a, b) = (tri[k], tri[(k + 1) % 3]);
                neighbours[a].push(b);
                neighbours[b].push(a);
            }
        }
        for list in &mut neighbours {
            list.sort_unstable();
            list.dedup();
        }
        #[allow(clippy::cast_precision_loss)] // Vertex counts are far below 2^52.
        let mean = self.scales.iter().sum::<f64>() / count as f64;
        let tolerance = FAIR_TOLERANCE * mean;
        self.membrane(&neighbours, tolerance, cancel)?;
        if !self.crease() {
            self.thin_plate(&neighbours, tolerance, cancel)?;
            return Ok(None);
        }
        let mut flat = Vec::new();
        reserve(&mut flat, self.positions.len())?;
        flat.extend_from_slice(&self.positions);
        self.thin_plate(&neighbours, tolerance, cancel)?;
        Ok(Some(std::mem::replace(&mut self.positions, flat)))
    }

    /// Whether the rim is planar and the surface leaves it steeply (see the module
    /// documentation). The plane's normal is Newell's, over the rim corners in order.
    fn crease(&self) -> bool {
        let rim = &self.positions[..self.fixed];
        let mut normal = [0.0; 3];
        for (j, a) in rim.iter().enumerate() {
            let b = rim[(j + 1) % rim.len()];
            normal = std::array::from_fn(|k| {
                let (u, v) = ((k + 1) % 3, (k + 2) % 3);
                normal[k] + (a[u] - b[u]) * (a[v] + b[v])
            });
        }
        let length = dot(normal, normal).sqrt();
        if rim.is_empty() || length == 0.0 {
            return false;
        }
        let normal = normal.map(|c| c / length);
        let mut low = [f64::INFINITY; 3];
        let mut high = [f64::NEG_INFINITY; 3];
        for p in rim {
            for k in 0..3 {
                low[k] = low[k].min(p[k]);
                high[k] = high[k].max(p[k]);
            }
        }
        let size = distance(low, high);
        let offsets: Vec<f64> = rim.iter().map(|&p| dot(sub(p, rim[0]), normal)).collect();
        let spread = offsets.iter().copied().fold(f64::NEG_INFINITY, f64::max)
            - offsets.iter().copied().fold(f64::INFINITY, f64::min);
        if spread > PLANAR_RIM * size {
            return false;
        }
        let (mut sines, mut count) = (0.0, 0usize);
        for (p, outside) in rim.iter().zip(&self.outside) {
            for &q in outside {
                let d = sub(q, *p);
                let reach = dot(d, d).sqrt();
                if reach > 0.0 {
                    sines += dot(d, normal).abs() / reach;
                    count += 1;
                }
            }
        }
        #[allow(clippy::cast_precision_loss)] // Neighbour counts are far below 2^52.
        let mean = sines / count.max(1) as f64;
        count > 0 && mean >= CREASE_SINE
    }

    /// Gauss-Seidel sweeps of the umbrella operator over the new vertices.
    fn membrane(
        &mut self,
        neighbours: &[Vec<usize>],
        tolerance: f64,
        cancel: &mut Cancellation<'_>,
    ) -> Result<(), FillError> {
        for _ in 0..MAX_FAIR_SWEEPS {
            cancel.check(Checkpoint::PreparationRecords)?;
            let mut largest = 0.0f64;
            for (v, list) in neighbours.iter().enumerate().skip(self.fixed) {
                if list.is_empty() {
                    continue;
                }
                #[allow(clippy::cast_precision_loss)]
                let weight = list.len() as f64;
                let target: Vec3 = std::array::from_fn(|k| {
                    list.iter().map(|&w| self.positions[w][k]).sum::<f64>() / weight
                });
                largest = largest.max(distance(target, self.positions[v]));
                self.positions[v] = target;
            }
            if largest <= tolerance {
                break;
            }
        }
        Ok(())
    }

    /// The umbrella vector of vertex `v`: the mean of its neighbours (a rim corner's mesh
    /// neighbours outside the cap included) less its position, and the neighbour count.
    fn umbrella(&self, neighbours: &[Vec<usize>], v: usize) -> (Vec3, f64) {
        let outside: &[Vec3] = if v < self.fixed {
            &self.outside[v]
        } else {
            &[]
        };
        let inside = &neighbours[v];
        #[allow(clippy::cast_precision_loss)] // Neighbour counts are tiny.
        let count = (inside.len() + outside.len()) as f64;
        if count == 0.0 {
            return ([0.0; 3], 0.0);
        }
        let mean: Vec3 = std::array::from_fn(|k| {
            (inside.iter().map(|&w| self.positions[w][k]).sum::<f64>()
                + outside.iter().map(|p| p[k]).sum::<f64>())
                / count
        });
        (sub(mean, self.positions[v]), count)
    }

    /// Gauss-Seidel sweeps of the discrete thin plate, `x <- x - U²(x) / ν`, over the new
    /// vertices.
    fn thin_plate(
        &mut self,
        neighbours: &[Vec<usize>],
        tolerance: f64,
        cancel: &mut Cancellation<'_>,
    ) -> Result<(), FillError> {
        for _ in 0..MAX_FAIR_SWEEPS {
            cancel.check(Checkpoint::PreparationRecords)?;
            let mut largest = 0.0f64;
            for v in self.fixed..self.positions.len() {
                let (own, count) = self.umbrella(neighbours, v);
                if count == 0.0 {
                    continue;
                }
                let mut sum = [0.0; 3];
                let mut inverse = 0.0;
                for &w in &neighbours[v] {
                    let (other, others) = self.umbrella(neighbours, w);
                    for k in 0..3 {
                        sum[k] += other[k];
                    }
                    if others > 0.0 {
                        inverse += 1.0 / others;
                    }
                }
                let nu = 1.0 + inverse / count;
                let step: Vec3 = std::array::from_fn(|k| (sum[k] / count - own[k]) / nu);
                for (coordinate, delta) in self.positions[v].iter_mut().zip(step) {
                    *coordinate -= delta;
                }
                largest = largest.max(dot(step, step).sqrt());
            }
            if largest <= tolerance {
                break;
            }
        }
        Ok(())
    }
}
