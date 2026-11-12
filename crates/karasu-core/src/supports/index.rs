//! Spatial queries against the model surface: nearest triangle, exact segment crossings and
//! conservative clearance proofs for convex support parts (TD-062).
#![allow(clippy::many_single_char_names)] // Geometric notation: points, edges, parameters.
use super::shapes::{add, cross, dot, length, scale, sub, unit, Part, V};
use crate::{
    cancellation::{Cancellation, Cancelled, Checkpoint},
    geometry::Point3,
    mesh::TriangleMesh,
    preparation::hollow::numeric::orient3d,
};
use std::cmp::Ordering;

/// Rounding margin of the clearance proof, in mm: projected gaps must exceed the clearance by it.
/// Coordinates within ±10⁴ mm give projection errors far below it.
pub(super) const PROOF_MARGIN_MM: f64 = 1e-6;
/// The grid holds at most this many cells.
const MAX_CELLS: usize = 1 << 22;

/// A uniform grid over a mesh's triangles' bounding boxes, kept apart from the mesh so it can
/// be kept with it (see [`super::route::SurfaceGrid`]).
pub(super) struct Grid {
    origin: V,
    cell: f64,
    dims: [usize; 3],
    /// Compressed rows: triangles of cell `c` are `items[starts[c]..starts[c + 1]]`.
    starts: Vec<u32>,
    items: Vec<u32>,
}

/// The grid of a mesh, with the mesh.
pub(super) struct TriangleIndex<'a> {
    mesh: &'a TriangleMesh,
    grid: Grid,
}

fn point(p: Point3) -> V {
    [p.x, p.y, p.z]
}

impl<'a> TriangleIndex<'a> {
    /// Index every triangle of `mesh`.
    pub(super) fn build(
        mesh: &'a TriangleMesh,
        cancel: &mut Cancellation<'_>,
    ) -> Result<Self, Cancelled> {
        Ok(Self {
            mesh,
            grid: Grid::build(mesh, cancel)?,
        })
    }

    /// Distinct triangles whose cells meet the box `[lo, hi]`, in increasing order.
    pub(super) fn candidates(&self, lo: V, hi: V) -> Vec<u32> {
        self.grid.candidates(self.mesh, lo, hi)
    }

    pub(super) fn corners(&self, triangle: u32) -> [V; 3] {
        self.mesh.triangles()[triangle as usize].map(|v| point(self.mesh.vertices()[v as usize]))
    }

    /// The nearest triangle within `tolerance` of `p`, with its distance; ties go to the lowest
    /// index.
    pub(super) fn nearest(&self, p: V, tolerance: f64) -> Option<(u32, f64)> {
        let r = [tolerance; 3];
        let mut best: Option<(u32, f64)> = None;
        for t in self.candidates(sub(p, r), add(p, r)) {
            let d = point_triangle_distance(p, self.corners(t));
            if d <= tolerance && best.is_none_or(|(_, bd)| d < bd) {
                best = Some((t, d));
            }
        }
        best
    }
}

impl Grid {
    /// Grid every triangle of `mesh`.
    pub(super) fn build(
        mesh: &TriangleMesh,
        cancel: &mut Cancellation<'_>,
    ) -> Result<Self, Cancelled> {
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        let mut extent_sum = 0.0;
        for (i, t) in mesh.triangles().iter().enumerate() {
            cancel.poll(Checkpoint::SupportWork, i)?;
            let (a, b) = triangle_box(mesh, *t);
            for k in 0..3 {
                lo[k] = lo[k].min(a[k]);
                hi[k] = hi[k].max(b[k]);
            }
            extent_sum += (0..3).map(|k| b[k] - a[k]).fold(0.0, f64::max);
        }
        let count = mesh.triangles().len().max(1);
        if !lo[0].is_finite() {
            lo = [0.0; 3];
            hi = [0.0; 3];
        }
        // Cells about twice the mean triangle size, enlarged until the grid fits its cap.
        #[allow(clippy::cast_precision_loss)] // Triangle counts are far below 2^52.
        let mut cell = (2.0 * extent_sum / count as f64).max(0.05);
        let dims = loop {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // Bounded below.
            let dims = [0, 1, 2].map(|k| (((hi[k] - lo[k]) / cell).floor() as usize) + 1);
            if dims
                .iter()
                .try_fold(1usize, |n, &d| n.checked_mul(d))
                .is_some_and(|n| n <= MAX_CELLS)
            {
                break dims;
            }
            cell *= 2.0;
        };
        let mut index = Self {
            origin: lo,
            cell,
            dims,
            starts: Vec::new(),
            items: Vec::new(),
        };
        let cells = dims[0] * dims[1] * dims[2];
        let mut counts = vec![0u32; cells + 1];
        for (i, t) in mesh.triangles().iter().enumerate() {
            cancel.poll(Checkpoint::SupportWork, i)?;
            index.for_cells(triangle_box(mesh, *t), |c| counts[c + 1] += 1);
        }
        for c in 0..cells {
            counts[c + 1] += counts[c];
        }
        let mut fill = counts.clone();
        let mut items = vec![0u32; counts[cells] as usize];
        for (i, t) in mesh.triangles().iter().enumerate() {
            cancel.poll(Checkpoint::SupportWork, i)?;
            let id = u32::try_from(i).expect("mesh triangle indices fit u32");
            index.for_cells(triangle_box(mesh, *t), |c| {
                items[fill[c] as usize] = id;
                fill[c] += 1;
            });
        }
        index.starts = counts;
        index.items = items;
        Ok(index)
    }

    fn for_cells(&self, (a, b): (V, V), mut visit: impl FnMut(usize)) {
        let range = |k: usize| {
            let at = |v: f64| {
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // Clamped.
                let i = ((v - self.origin[k]) / self.cell).floor().max(0.0) as usize;
                i.min(self.dims[k] - 1)
            };
            at(a[k])..=at(b[k])
        };
        for z in range(2) {
            for y in range(1) {
                for x in range(0) {
                    visit((z * self.dims[1] + y) * self.dims[0] + x);
                }
            }
        }
    }

    /// Distinct triangles of `mesh` whose cells meet the box `[lo, hi]`, in increasing order.
    pub(super) fn candidates(&self, mesh: &TriangleMesh, lo: V, hi: V) -> Vec<u32> {
        let mut found = Vec::new();
        if (0..3).any(|k| {
            hi[k] < self.origin[k] || lo[k] > self.origin[k] + self.cell * as_f64(self.dims[k])
        }) {
            return found;
        }
        self.for_cells((lo, hi), |c| {
            found.extend_from_slice(
                &self.items[self.starts[c] as usize..self.starts[c + 1] as usize],
            );
        });
        found.sort_unstable();
        found.dedup();
        found.retain(|&t| {
            let (a, b) = triangle_box(mesh, mesh.triangles()[t as usize]);
            (0..3).all(|k| a[k] <= hi[k] && b[k] >= lo[k])
        });
        found
    }
}

fn as_f64(n: usize) -> f64 {
    #[allow(clippy::cast_precision_loss)] // Grid dimensions are at most 2^22.
    let value = n as f64;
    value
}

fn triangle_box(mesh: &TriangleMesh, t: [u32; 3]) -> (V, V) {
    let [a, b, c] = t.map(|v| point(mesh.vertices()[v as usize]));
    (
        [0, 1, 2].map(|k| a[k].min(b[k]).min(c[k])),
        [0, 1, 2].map(|k| a[k].max(b[k]).max(c[k])),
    )
}

/// The unit normal of a triangle by its winding, or `None` when degenerate.
pub(super) fn normal([a, b, c]: [V; 3]) -> Option<V> {
    unit(cross(sub(b, a), sub(c, a)))
}

/// Euclidean distance from `p` to the closed triangle (closest-point regions).
pub(super) fn point_triangle_distance(p: V, [a, b, c]: [V; 3]) -> f64 {
    let ab = sub(b, a);
    let ac = sub(c, a);
    let ap = sub(p, a);
    let d1 = dot(ab, ap);
    let d2 = dot(ac, ap);
    if d1 <= 0.0 && d2 <= 0.0 {
        return length(ap);
    }
    let bp = sub(p, b);
    let d3 = dot(ab, bp);
    let d4 = dot(ac, bp);
    if d3 >= 0.0 && d4 <= d3 {
        return length(bp);
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        let v = d1 / (d1 - d3);
        return length(sub(p, add(a, scale(ab, v))));
    }
    let cp = sub(p, c);
    let d5 = dot(ab, cp);
    let d6 = dot(ac, cp);
    if d6 >= 0.0 && d5 <= d6 {
        return length(cp);
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        let w = d2 / (d2 - d6);
        return length(sub(p, add(a, scale(ac, w))));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
        return length(sub(p, add(b, scale(sub(c, b), w))));
    }
    let denominator = 1.0 / (va + vb + vc);
    let v = vb * denominator;
    let w = vc * denominator;
    length(sub(p, add(a, add(scale(ab, v), scale(ac, w)))))
}

fn p3(v: V) -> Point3 {
    Point3::new(v[0], v[1], v[2])
}

/// Whether the closed segment `[p, q]` meets the closed triangle, by exact orientation signs of
/// the represented coordinates. A segment in the triangle's plane counts as meeting it.
pub(super) fn segment_meets_triangle(p: V, q: V, [a, b, c]: [V; 3]) -> bool {
    let (a, b, c, p, q) = (p3(a), p3(b), p3(c), p3(p), p3(q));
    let sp = orient3d(a, b, c, p);
    let sq = orient3d(a, b, c, q);
    if sp == sq && sp != Ordering::Equal {
        return false;
    }
    if sp == Ordering::Equal && sq == Ordering::Equal {
        // Coplanar: reported as meeting (conservative for piercing checks).
        return true;
    }
    let signs = [
        orient3d(p, q, a, b),
        orient3d(p, q, b, c),
        orient3d(p, q, c, a),
    ];
    !(signs.contains(&Ordering::Less) && signs.contains(&Ordering::Greater))
}

/// Closest distance between segments `[p0, p1]` and `[q0, q1]`.
pub(super) fn segment_segment_distance(p0: V, p1: V, q0: V, q1: V) -> f64 {
    let d1 = sub(p1, p0);
    let d2 = sub(q1, q0);
    let r = sub(p0, q0);
    let a = dot(d1, d1);
    let e = dot(d2, d2);
    let f = dot(d2, r);
    let (s, t) = if a <= f64::MIN_POSITIVE && e <= f64::MIN_POSITIVE {
        (0.0, 0.0)
    } else if a <= f64::MIN_POSITIVE {
        (0.0, (f / e).clamp(0.0, 1.0))
    } else {
        let c = dot(d1, r);
        if e <= f64::MIN_POSITIVE {
            ((-c / a).clamp(0.0, 1.0), 0.0)
        } else {
            let b = dot(d1, d2);
            let denominator = a * e - b * b;
            let mut s = if denominator > 0.0 {
                ((b * f - c * e) / denominator).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let mut t = (b * s + f) / e;
            if t < 0.0 {
                t = 0.0;
                s = (-c / a).clamp(0.0, 1.0);
            } else if t > 1.0 {
                t = 1.0;
                s = ((b - c) / a).clamp(0.0, 1.0);
            }
            (s, t)
        }
    };
    length(sub(add(p0, scale(d1, s)), add(q0, scale(d2, t))))
}

/// Distance between segment `[p, q]` and a triangle, with the direction from the segment's
/// closest point towards the triangle's (zero when they meet).
pub(super) fn segment_triangle_distance(p: V, q: V, tri: [V; 3]) -> f64 {
    if segment_meets_triangle(p, q, tri) {
        return 0.0;
    }
    let [a, b, c] = tri;
    let mut best = point_triangle_distance(p, tri).min(point_triangle_distance(q, tri));
    for (u, v) in [(a, b), (b, c), (c, a)] {
        best = best.min(segment_segment_distance(p, q, u, v));
    }
    best
}

/// Whether `part` is proven to keep at least `clearance` from the triangle.
///
/// First its axis capsule; then separating axes over the part's face normals, the triangle
/// normal, edge cross products and the direction between the closest points of the part axis and
/// the triangle. A projected gap is a lower bound of the true distance, so a proof is sound; a
/// failed proof is treated as a collision.
pub(super) fn proven_clear(part: &Part, tri: [V; 3], clearance: f64) -> bool {
    let need = clearance + PROOF_MARGIN_MM;
    let [p, q] = part.axis;
    if segment_triangle_distance(p, q, tri) - part.radius >= need {
        return true;
    }
    let gap = |axis: V| -> bool {
        let Some(n) = unit(axis) else { return false };
        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
        for &v in &part.vertices {
            let d = dot(v, n);
            lo = lo.min(d);
            hi = hi.max(d);
        }
        let projected = tri.map(|v| dot(v, n));
        let t_lo = projected.iter().copied().fold(f64::INFINITY, f64::min);
        let t_hi = projected.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        t_lo - hi >= need || lo - t_hi >= need
    };
    if normal(tri).is_some_and(gap) {
        return true;
    }
    for t in &part.triangles {
        let face = t.map(|i| part.vertices[i as usize]);
        if normal(face).is_some_and(gap) {
            return true;
        }
    }
    let tri_edges = [
        sub(tri[1], tri[0]),
        sub(tri[2], tri[1]),
        sub(tri[0], tri[2]),
    ];
    for t in &part.triangles {
        for (i, j) in [(0, 1), (1, 2), (2, 0)] {
            let e = sub(part.vertices[t[j] as usize], part.vertices[t[i] as usize]);
            if tri_edges.iter().any(|&f| gap(cross(e, f))) {
                return true;
            }
        }
    }
    false
}

/// Whether `part` is clear of the solid cylinder from `start` to `end` of `radius`, by its tapered
/// capsule (TD-123): its axis in eight pieces, each a capsule as wide as the part at that
/// piece's wider end. A cone's thin end is tested as thin, where a single capsule would be as
/// wide as its base.
pub(super) fn clear_of_tapered(part: &Part, start: V, end: V, radius: f64) -> bool {
    const PIECES: u32 = 8;
    let [p, q] = part.axis;
    let [r0, r1] = part.taper;
    let at = |k: u32| {
        let t = f64::from(k) / f64::from(PIECES);
        (add(p, scale(sub(q, p), t)), r0 + (r1 - r0) * t)
    };
    (0..PIECES).all(|k| {
        let ((a, ra), (b, rb)) = (at(k), at(k + 1));
        segment_segment_distance(a, b, start, end) - ra.max(rb) - radius >= PROOF_MARGIN_MM
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::supports::shapes::{frustum, sphere};

    fn plate() -> TriangleMesh {
        // A 10 x 10 square at Z = 5, two triangles facing up.
        TriangleMesh::try_new(
            vec![
                Point3::new(0.0, 0.0, 5.0),
                Point3::new(10.0, 0.0, 5.0),
                Point3::new(10.0, 10.0, 5.0),
                Point3::new(0.0, 10.0, 5.0),
            ],
            vec![[0, 1, 2], [0, 2, 3]],
        )
        .unwrap()
    }

    #[test]
    fn nearest_triangle_and_point_distances() {
        let mesh = plate();
        let index = TriangleIndex::build(&mesh, &mut Cancellation::never()).unwrap();
        let (t, d) = index.nearest([2.0, 1.0, 5.01], 0.05).unwrap();
        assert_eq!(t, 0);
        assert!((d - 0.01).abs() < 1e-12);
        assert!(index.nearest([2.0, 1.0, 5.2], 0.05).is_none());
        assert!(index.nearest([20.0, 1.0, 5.0], 0.05).is_none());
        assert_eq!(
            point_triangle_distance([12.0, 0.0, 5.0], index.corners(0)),
            2.0
        );
    }

    #[test]
    fn exact_segment_crossings() {
        let tri = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
        assert!(segment_meets_triangle(
            [0.2, 0.2, -1.0],
            [0.2, 0.2, 1.0],
            tri
        ));
        assert!(!segment_meets_triangle(
            [0.8, 0.8, -1.0],
            [0.8, 0.8, 1.0],
            tri
        ));
        assert!(!segment_meets_triangle(
            [0.2, 0.2, 0.1],
            [0.2, 0.2, 1.0],
            tri
        ));
        // Touching an edge counts.
        assert!(segment_meets_triangle(
            [0.5, 0.0, -1.0],
            [0.5, 0.0, 1.0],
            tri
        ));
    }

    #[test]
    fn clearance_proofs_are_sound_and_tight() {
        let tri = [[-5.0, -5.0, 5.0], [5.0, -5.0, 5.0], [0.0, 5.0, 5.0]];
        // A vertical shaft of radius 0.5 ending 1 mm below the plane.
        let shaft = frustum([0.0, 0.0, 0.0], 0.5, [0.0, 0.0, 4.0], 0.5, 16);
        assert!(proven_clear(&shaft, tri, 0.9));
        assert!(!proven_clear(&shaft, tri, 1.1));
        // Through the plane: never clear.
        let through = frustum([0.0, 0.0, 0.0], 0.5, [0.0, 0.0, 6.0], 0.5, 16);
        assert!(!proven_clear(&through, tri, 0.0));
        // A wide flat pad below the triangle: the capsule fails, the face normal proves it.
        let pad = frustum([0.0, 0.0, 0.0], 4.0, [0.0, 0.0, 0.5], 4.0, 16);
        assert!(proven_clear(&pad, tri, 4.0));
        // A sphere beside the triangle's edge.
        let ball = sphere([0.0, -7.0, 5.0], 1.0, 16);
        assert!(proven_clear(&ball, tri, 0.9));
        assert!(!proven_clear(&ball, tri, 1.1));
    }
}
