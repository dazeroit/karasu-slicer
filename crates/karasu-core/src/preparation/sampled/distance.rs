//! Binary64 point, segment and triangle distances with an explicit floating-point error budget.
//!
//! Closest points come from first-order vertex-region tests, edge candidates and a Gram-Schmidt
//! face foot that stays accurate for long thin triangles. Every returned candidate is an actual
//! point of the closed primitive up to rounding, so computed distances are upper bounds up to
//! rounding: a region or parameter misjudged near a boundary (or for nearly parallel segments)
//! selects another point and overestimates, and a near-degenerate face cannot pull the result
//! below its true distance. [`QUERY_RELATIVE_ERROR`] bounds rounding relative to
//! the coordinate magnitudes for well-shaped input. Where a certified lower bound is required,
//! [`separation`] provides one with interval arithmetic.

use crate::preparation::hollow::numeric::I;

/// One placed source triangle in world millimeters.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Triangle {
    pub(crate) corners: [[f64; 3]; 3],
}

impl Triangle {
    pub(crate) const fn new(corners: [[f64; 3]; 3]) -> Self {
        Self { corners }
    }
}

/// Relative rounding budget of [`point_triangle_distance_sq`]'s distance, multiplied by
/// `1 + max |coordinate|` (mm).
///
/// The routine performs a bounded sequence (fewer than 40 dependent operations) of correctly
/// rounded binary64 additions, multiplications and at most one division per coordinate of the
/// selected candidate point. Each candidate coordinate therefore carries a relative error below
/// 64 unit roundoffs (2^-47) of the magnitudes involved, and the Euclidean distance inherits at
/// most `sqrt(3)` times that. 2^-40 leaves more than two orders of magnitude of margin; it is a
/// documented analytic budget, checked against closed-form distances in the tests, not an
/// interval proof, and it assumes triangles that are not extremely thin: beyond an aspect ratio
/// of about 10^6 the Gram-Schmidt complement loses relative accuracy. The edge candidates then
/// still apply, so the result may exceed the true distance by at most the triangle's inradius
/// but never falls below it by more than rounding.
pub(crate) const QUERY_RELATIVE_ERROR: f64 = 1.0 / 1_099_511_627_776.0; // 2^-40

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn along(a: [f64; 3], d: [f64; 3], t: f64) -> [f64; 3] {
    [a[0] + t * d[0], a[1] + t * d[1], a[2] + t * d[2]]
}

fn length_sq(v: [f64; 3]) -> f64 {
    dot(v, v)
}

/// Squared Euclidean distance from `p` to the closed triangle `t`.
pub(crate) fn point_triangle_distance_sq(p: [f64; 3], t: &Triangle) -> f64 {
    length_sq(sub(p, closest_point_on_triangle(p, t)))
}

/// Closest point of segment `a b` to `p`.
fn closest_on_segment(p: [f64; 3], a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    let d = sub(b, a);
    let len = length_sq(d);
    if len <= 0.0 || !len.is_finite() {
        return a;
    }
    along(a, d, (dot(sub(p, a), d) / len).clamp(0.0, 1.0))
}

/// Closest point of the closed triangle `t` to `p`.
///
/// Vertex regions are decided from first-order dot products, which is exact up to rounding for
/// every triangle shape. Otherwise the result is the nearest of the three edge candidates and,
/// when it lies in the triangle, the foot of the perpendicular ([`face_foot`]). Every candidate
/// is a point of the closed triangle up to rounding, so a near-degenerate face (whose computed
/// plane is meaningless) can only make the face candidate unavailable or farther, never closer
/// than the triangle: the computed distance stays an upper bound up to rounding, and the edges
/// supply the true value whenever the face is too thin to define it.
#[allow(clippy::many_single_char_names)] // Standard Voronoi-region notation (a, b, c, p).
pub(crate) fn closest_point_on_triangle(p: [f64; 3], t: &Triangle) -> [f64; 3] {
    let [a, b, c] = t.corners;
    let ab = sub(b, a);
    let ac = sub(c, a);
    let ap = sub(p, a);
    let d1 = dot(ab, ap);
    let d2 = dot(ac, ap);
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = sub(p, b);
    let d3 = dot(ab, bp);
    let d4 = dot(ac, bp);
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let cp = sub(p, c);
    let d5 = dot(ab, cp);
    let d6 = dot(ac, cp);
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let mut best =
        face_foot(a, ab, ac, ap).map_or((f64::INFINITY, a), |q| (length_sq(sub(p, q)), q));
    for (from, to) in [(a, b), (b, c), (c, a)] {
        let q = closest_on_segment(p, from, to);
        let d = length_sq(sub(p, q));
        if d < best.0 {
            best = (d, q);
        }
    }
    best.1
}

/// Foot of the perpendicular from `a + ap` onto the plane of the triangle `a, a + ab, a + ac`,
/// as the convex combination `a + β ab + γ ac`, or `None` when the computed foot is outside the
/// closed triangle or the triangle has no computable plane.
///
/// `h = ac - k ab` (with `k = ac·ab / ab·ab`) is the Gram-Schmidt complement of `ab`, so the
/// coordinates `γ = ap·h / h·h` and `β = ap·ab / ab·ab - k γ` are built from first-order dot
/// products. They are accurate for long thin triangles. For a near-degenerate triangle they may
/// be arbitrary, but an accepted foot is still a convex combination of the corners, i.e. a point
/// of the triangle up to rounding.
fn face_foot(a: [f64; 3], ab: [f64; 3], ac: [f64; 3], ap: [f64; 3]) -> Option<[f64; 3]> {
    let ee = length_sq(ab);
    if ee.is_nan() || ee <= 0.0 || !ee.is_finite() {
        return None;
    }
    let k = dot(ac, ab) / ee;
    let h = [ac[0] - k * ab[0], ac[1] - k * ab[1], ac[2] - k * ab[2]];
    let hh = length_sq(h);
    if hh.is_nan() || hh <= 0.0 || !hh.is_finite() {
        return None;
    }
    let gamma = dot(ap, h) / hh;
    let beta = dot(ap, ab) / ee - k * gamma;
    (beta >= 0.0 && gamma >= 0.0 && beta + gamma <= 1.0).then(|| {
        [
            a[0] + beta * ab[0] + gamma * ac[0],
            a[1] + beta * ab[1] + gamma * ac[1],
            a[2] + beta * ab[2] + gamma * ac[2],
        ]
    })
}

/// Squared distance between closed segments `p0 p1` and `q0 q1` (binary64).
///
/// Minimizes over the segment parameters with clamping, handling parallel and degenerate segments.
/// The result is the distance between two actual points of the segments up to rounding.
#[cfg(test)]
pub(crate) fn segment_segment_distance_sq(
    p0: [f64; 3],
    p1: [f64; 3],
    q0: [f64; 3],
    q1: [f64; 3],
) -> f64 {
    let (x, y) = segment_segment_closest(p0, p1, q0, q1);
    length_sq(sub(x, y))
}

/// Closest points of closed segments `p0 p1` and `q0 q1` (binary64, clamped parameters).
#[allow(clippy::many_single_char_names)] // Standard closest-points notation.
fn segment_segment_closest(
    p0: [f64; 3],
    p1: [f64; 3],
    q0: [f64; 3],
    q1: [f64; 3],
) -> ([f64; 3], [f64; 3]) {
    let d1 = sub(p1, p0);
    let d2 = sub(q1, q0);
    let r = sub(p0, q0);
    let a = dot(d1, d1);
    let e = dot(d2, d2);
    let f = dot(d2, r);
    if a <= 0.0 && e <= 0.0 {
        return (p0, q0);
    }
    let (s, t) = if a <= 0.0 {
        (0.0, (f / e).clamp(0.0, 1.0))
    } else {
        let c = dot(d1, r);
        if e <= 0.0 {
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
    (along(p0, d1, s), along(q0, d2, t))
}

/// Closest-point candidates of two closed triangles known not to intersect: the minimum over the
/// vertex-to-triangle and edge-to-edge pairs (15 candidates), as `(squared distance, on s, on t)`.
///
/// For disjoint triangles the true minimum is attained by one of these pairs; the computed pair
/// is two actual points of the triangles up to rounding, so its distance is an upper bound. Use
/// [`separation`] for a rigorous lower bound.
pub(crate) fn disjoint_triangles_closest(s: &Triangle, t: &Triangle) -> (f64, [f64; 3], [f64; 3]) {
    let mut best = (f64::INFINITY, s.corners[0], t.corners[0]);
    let mut consider = |x: [f64; 3], y: [f64; 3]| {
        let d = length_sq(sub(x, y));
        if d < best.0 {
            best = (d, x, y);
        }
    };
    for v in s.corners {
        consider(v, closest_point_on_triangle(v, t));
    }
    for v in t.corners {
        consider(closest_point_on_triangle(v, s), v);
    }
    for i in 0..3 {
        for j in 0..3 {
            let (x, y) = segment_segment_closest(
                s.corners[i],
                s.corners[(i + 1) % 3],
                t.corners[j],
                t.corners[(j + 1) % 3],
            );
            consider(x, y);
        }
    }
    best
}

/// Squared distance between two closed triangles known not to intersect (upper bound; see
/// [`disjoint_triangles_closest`]).
#[cfg(test)]
pub(crate) fn disjoint_triangles_distance_sq(s: &Triangle, t: &Triangle) -> f64 {
    disjoint_triangles_closest(s, t).0
}

/// Computed squared distance of two disjoint triangles (an upper bound up to rounding, see
/// [`disjoint_triangles_closest`]) and a rigorous lower bound on their distance in mm (zero if
/// none is proven).
///
/// For any direction `u`, the gap between the projections of the two vertex sets onto `u / |u|`
/// cannot exceed the distance between the triangles (projection is 1-Lipschitz). The bound is
/// evaluated with outward-rounded interval arithmetic for the direction between the computed
/// closest points and for both triangle normals, and the largest proven gap is returned. It is
/// exact up to rounding when the computed closest points are right and still valid when they are
/// not.
#[allow(clippy::many_single_char_names)] // Triangles s, t; closest points x, y.
pub(crate) fn separation(s: &Triangle, t: &Triangle) -> (f64, f64) {
    let (distance_sq, x, y) = disjoint_triangles_closest(s, t);
    let bound = [sub(y, x), triangle_normal(s), triangle_normal(t)]
        .into_iter()
        .map(|direction| vertex_gap(&s.corners, &t.corners, direction))
        .fold(0.0, f64::max);
    (distance_sq, bound)
}

/// Rigorous lower bound on the distance between two triangles (see [`separation`]).
#[cfg(test)]
pub(crate) fn separation_lower_bound(s: &Triangle, t: &Triangle) -> f64 {
    separation(s, t).1
}

/// Computed (not normalized) normal `ab x ac` of a triangle; zero for a degenerate one.
pub(crate) fn triangle_normal(t: &Triangle) -> [f64; 3] {
    let [a, b, c] = t.corners;
    let (ab, ac) = (sub(b, a), sub(c, a));
    [
        ab[1] * ac[2] - ab[2] * ac[1],
        ab[2] * ac[0] - ab[0] * ac[2],
        ab[0] * ac[1] - ab[1] * ac[0],
    ]
}

/// Proven lower bound, in mm, on the gap between the projections of the point sets `s` and `t`
/// onto `u / |u|` (in either order), or zero. It bounds the distance between their convex hulls
/// from below, because projection onto a unit direction is 1-Lipschitz. Every direction is
/// valid; a computed or even arbitrary `u` only affects tightness.
#[allow(clippy::many_single_char_names)] // Point sets s, t; direction u; vertex v.
pub(crate) fn vertex_gap(s: &[[f64; 3]], t: &[[f64; 3]], u: [f64; 3]) -> f64 {
    let project = |v: [f64; 3]| {
        I::point(u[0])
            .mul(I::point(v[0]))
            .add(I::point(u[1]).mul(I::point(v[1])))
            .add(I::point(u[2]).mul(I::point(v[2])))
    };
    let gap = |below: &[[f64; 3]], above: &[[f64; 3]]| {
        let low = above
            .iter()
            .map(|&v| project(v).lo)
            .fold(f64::INFINITY, f64::min);
        let high = below
            .iter()
            .map(|&v| project(v).hi)
            .fold(f64::NEG_INFINITY, f64::max);
        I::point(low).sub(I::point(high)).lo
    };
    let gap = gap(s, t).max(gap(t, s));
    if gap.is_nan() || gap <= 0.0 {
        return 0.0;
    }
    let norm = I::point(u[0])
        .mul(I::point(u[0]))
        .add(I::point(u[1]).mul(I::point(u[1])))
        .add(I::point(u[2]).mul(I::point(u[2])))
        .sqrt();
    match norm {
        Some(norm) if norm.hi.is_finite() && norm.hi > 0.0 => I::point(gap)
            .div(I::point(norm.hi))
            .map_or(0.0, |q| q.lo.max(0.0)),
        _ => 0.0,
    }
}

/// Whether the projections of the point sets `s` and `t` onto `u / |u|` are proven more than
/// `threshold` apart (in either order): the same certificate as [`vertex_gap`] `> threshold`,
/// decided without a square root by comparing `gap²` with `threshold² |u|²` in outward intervals.
pub(crate) fn projection_gap_exceeds(
    s: &[[f64; 3]],
    t: &[[f64; 3]],
    u: [f64; 3],
    threshold: f64,
) -> bool {
    let project = |v: [f64; 3]| interval_dot(u, v);
    let gap = |below: &[[f64; 3]], above: &[[f64; 3]]| {
        let low = above
            .iter()
            .map(|&v| project(v).lo)
            .fold(f64::INFINITY, f64::min);
        let high = below
            .iter()
            .map(|&v| project(v).hi)
            .fold(f64::NEG_INFINITY, f64::max);
        I::point(low).sub(I::point(high)).lo
    };
    let gap = gap(s, t).max(gap(t, s));
    if gap.is_nan() || gap <= 0.0 || threshold.is_nan() || threshold < 0.0 {
        return false;
    }
    let squared = I::point(gap).mul(I::point(gap)).lo;
    let bound = I::point(threshold)
        .mul(I::point(threshold))
        .mul(interval_dot(u, u))
        .hi;
    squared.is_finite() && bound.is_finite() && squared > bound
}

/// Proven lower bound, in mm, on the distance between triangle `t` and the disk of the given
/// centre, normal world `axis` and radius, from the gap between their projections onto
/// `u / |u|` (zero if none is proven).
#[allow(clippy::many_single_char_names)] // Triangle t, centre c, radius r, direction u.
pub(crate) fn disk_gap(t: &Triangle, c: [f64; 3], axis: usize, r: f64, u: [f64; 3]) -> f64 {
    let low = t
        .corners
        .iter()
        .map(|&v| interval_dot(u, v).lo)
        .fold(f64::INFINITY, f64::min);
    let high = t
        .corners
        .iter()
        .map(|&v| interval_dot(u, v).hi)
        .fold(f64::NEG_INFINITY, f64::max);
    disk_gap_range(low, high, c, axis, r, u)
}

/// [`disk_gap`] for the closed box `[min, max]`, whose projection onto `u` is bounded through
/// the corner that minimizes or maximizes each coordinate term.
#[allow(clippy::many_single_char_names)] // Centre c, radius r, direction u.
pub(crate) fn disk_box_gap(
    min: [f64; 3],
    max: [f64; 3],
    c: [f64; 3],
    axis: usize,
    r: f64,
    u: [f64; 3],
) -> f64 {
    let (mut low, mut high) = (I::point(0.0), I::point(0.0));
    for k in 0..3 {
        let (near, far) = if u[k] >= 0.0 {
            (min[k], max[k])
        } else {
            (max[k], min[k])
        };
        low = low.add(I::point(u[k]).mul(I::point(near)));
        high = high.add(I::point(u[k]).mul(I::point(far)));
    }
    disk_gap_range(low.lo, high.hi, c, axis, r, u)
}

pub(crate) fn interval_dot(u: [f64; 3], v: [f64; 3]) -> I {
    I::point(u[0])
        .mul(I::point(v[0]))
        .add(I::point(u[1]).mul(I::point(v[1])))
        .add(I::point(u[2]).mul(I::point(v[2])))
}

/// Proven gap, divided by `|u|`, between a set whose projection onto `u` lies in `[low, high]`
/// and the disk `(c, axis, r)`, which projects onto `[c·u - r |u'|, c·u + r |u'|]` with `u'` the
/// part of `u` across the axis; zero if none is proven. Outward-rounded intervals throughout.
#[allow(clippy::many_single_char_names)] // Centre c, radius r, direction u.
fn disk_gap_range(low: f64, high: f64, c: [f64; 3], axis: usize, r: f64, u: [f64; 3]) -> f64 {
    let mut across = I::point(0.0);
    for k in (0..3).filter(|&k| k != axis) {
        across = across.add(I::point(u[k]).mul(I::point(u[k])));
    }
    // A sum of squares is never negative; outward rounding of exact zeros must not make its
    // square root fail (an axis-parallel `u` then has no reach across the axis).
    across.lo = across.lo.max(0.0);
    let (Some(across), Some(norm)) = (across.sqrt(), interval_dot(u, u).sqrt()) else {
        return 0.0;
    };
    let reach = I::point(r).mul(I::point(across.hi));
    let center = interval_dot(u, c);
    let disk_low = center.sub(reach).lo;
    let disk_high = center.add(reach).hi;
    let gap = I::point(low)
        .sub(I::point(disk_high))
        .lo
        .max(I::point(disk_low).sub(I::point(high)).lo);
    if gap.is_nan() || gap <= 0.0 || !norm.hi.is_finite() || norm.hi <= 0.0 {
        return 0.0;
    }
    I::point(gap)
        .div(I::point(norm.hi))
        .map_or(0.0, |q| q.lo.max(0.0))
}

#[cfg(test)]
mod tests {
    use super::{point_triangle_distance_sq, Triangle};

    #[test]
    fn disk_gaps_along_axes_and_tilted_directions() {
        use super::{disk_box_gap, disk_gap};
        let above = Triangle::new([[-5.0, -5.0, 1.0], [5.0, -5.0, 1.0], [0.0, 5.0, 1.0]]);
        // Along the disk axis the gap is the full axial distance (review counterexample: it
        // used to be zero because outward rounding made a zero sum of squares negative).
        let axial = disk_gap(&above, [0.0; 3], 2, 1.0, [0.0, 0.0, 1.0]);
        assert!(axial <= 1.0 && axial > 1.0 - 1e-12, "{axial}");
        let boxed = disk_box_gap(
            [-5.0, -5.0, 1.0],
            [5.0, 5.0, 2.0],
            [0.0; 3],
            2,
            1.0,
            [0.0, 0.0, 1.0],
        );
        assert!(boxed <= 1.0 && boxed > 1.0 - 1e-12, "{boxed}");
        // Across the axis the disk reaches out by its radius.
        let side = Triangle::new([[3.0, -1.0, -1.0], [3.0, 1.0, -1.0], [3.0, 0.0, 1.0]]);
        let radial = disk_gap(&side, [0.0; 3], 2, 1.0, [1.0, 0.0, 0.0]);
        assert!(radial <= 2.0 && radial > 2.0 - 1e-12, "{radial}");
        // A tilted direction is still a valid, smaller bound.
        let tilted = disk_gap(&above, [0.0; 3], 2, 1.0, [1e-3, 0.0, 1.0]);
        assert!(tilted <= 1.0 && tilted > 0.99, "{tilted}");
    }

    #[test]
    fn analytic_regions_match_closed_form_distances() {
        let t = Triangle::new([[0.0, 0.0, 0.0], [4.0, 0.0, 0.0], [0.0, 4.0, 0.0]]);
        let cases = [
            ([1.0, 1.0, 3.0], 9.0),    // face
            ([-3.0, -4.0, 0.0], 25.0), // vertex a
            ([6.0, -1.0, 0.0], 5.0),   // vertex b
            ([2.0, -2.0, 1.0], 5.0),   // edge ab
            ([3.0, 3.0, 0.0], 2.0),    // edge bc
            ([-1.0, 2.0, 2.0], 5.0),   // edge ca
            ([1.0, 1.0, 0.0], 0.0),    // on the face
        ];
        for (p, expected) in cases {
            assert_eq!(point_triangle_distance_sq(p, &t), expected, "{p:?}");
        }
    }

    #[test]
    fn segment_and_triangle_pair_distances_match_closed_forms() {
        use super::{disjoint_triangles_distance_sq, segment_segment_distance_sq};
        // Skew perpendicular segments 3 apart; parallel overlapping; collinear gap; degenerate.
        assert_eq!(
            segment_segment_distance_sq(
                [0.0, 0.0, 0.0],
                [2.0, 0.0, 0.0],
                [1.0, -1.0, 3.0],
                [1.0, 1.0, 3.0]
            ),
            9.0
        );
        assert_eq!(
            segment_segment_distance_sq(
                [0.0, 0.0, 0.0],
                [4.0, 0.0, 0.0],
                [1.0, 2.0, 0.0],
                [3.0, 2.0, 0.0]
            ),
            4.0
        );
        assert_eq!(
            segment_segment_distance_sq(
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [4.0, 0.0, 0.0],
                [6.0, 0.0, 0.0]
            ),
            9.0
        );
        assert_eq!(
            segment_segment_distance_sq(
                [1.0, 1.0, 1.0],
                [1.0, 1.0, 1.0],
                [0.0, 0.0, 0.0],
                [2.0, 0.0, 0.0]
            ),
            2.0
        );
        let base = Triangle::new([[0.0, 0.0, 0.0], [4.0, 0.0, 0.0], [0.0, 4.0, 0.0]]);
        // Parallel plane above (vertex-face), and an edge-edge configuration.
        let above = Triangle::new([[1.0, 1.0, 2.0], [2.0, 1.0, 2.0], [1.0, 2.0, 2.0]]);
        assert_eq!(disjoint_triangles_distance_sq(&base, &above), 4.0);
        let skew = Triangle::new([[3.0, 3.0, 1.0], [5.0, 1.0, 1.0], [6.0, 6.0, 5.0]]);
        // Closest pair: edge (4,0)-(0,4) at (2,2,0)... distance from segment z=1 line x+y=6 is
        // reached at the edge midpoint (4,2,1)-(2,2,0) direction: compute by brute minimum.
        let brute = {
            let mut best = f64::INFINITY;
            let n = 200;
            for i in 0..=n {
                for j in 0..=n - i {
                    let (u, v) = (f64::from(i) / f64::from(n), f64::from(j) / f64::from(n));
                    let q = [0, 1, 2].map(|k| {
                        skew.corners[0][k]
                            + u * (skew.corners[1][k] - skew.corners[0][k])
                            + v * (skew.corners[2][k] - skew.corners[0][k])
                    });
                    best = best.min(point_triangle_distance_sq(q, &base));
                }
            }
            best
        };
        let exact = disjoint_triangles_distance_sq(&base, &skew);
        assert!(exact <= brute + 1e-12 && exact > 0.0, "{exact} {brute}");
    }

    #[test]
    fn thin_triangles_and_near_parallel_segments_keep_certified_bounds_sound() {
        use super::{disjoint_triangles_distance_sq, separation_lower_bound};
        // Review counterexample: a 100 mm long, 2.6e-5 mm wide source triangle almost touching a
        // horizontal cap triangle. The exact rational distance is 1.166e-7 mm.
        let cap = Triangle::new([
            [
                -1.065_263_224_105_496_2,
                19.129_469_510_065_032,
                12.559_484_021_283_385,
            ],
            [
                -2.882_232_433_015_661_2,
                15.565_956_791_710_8,
                12.559_484_021_283_385,
            ],
            [
                -2.230_882_158_580_505_4,
                15.303_070_092_911_44,
                12.559_484_021_283_385,
            ],
        ]);
        let sliver = Triangle::new([
            [
                27.215_197_969_190_87,
                6.404_070_907_882_634,
                4.736_357_359_996_107,
            ],
            [
                -61.207_806_925_245_41,
                46.191_902_847_070_58,
                29.196_542_048_943_776,
            ],
            [
                11.773_798_678_643_6,
                13.352_253_594_504_056,
                9.007_837_765_917_259,
            ],
        ]);
        let exact = 1.166e-7;
        let computed = disjoint_triangles_distance_sq(&cap, &sliver).sqrt();
        let lower = separation_lower_bound(&cap, &sliver);
        assert!(
            lower <= exact * 1.001,
            "lower bound {lower} exceeds {exact}"
        );
        // The normal-projection face case removes the former 1e-5 mm overestimate.
        assert!(computed < 1e-6, "computed {computed}");
        // Nearly parallel 10 mm segments 1e-9 mm apart: the certified bound never exceeds it.
        let a = Triangle::new([[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [0.0, -1.0, 0.0]]);
        let b = Triangle::new([[0.0, 1e-9, 1e-12], [10.0, 1e-9, 0.0], [0.0, 1.0, 0.0]]);
        assert!(separation_lower_bound(&a, &b) <= 1e-9 * 1.001);
    }

    #[test]
    fn degenerate_triangles_use_their_segments() {
        let collinear = Triangle::new([[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]]);
        assert_eq!(
            point_triangle_distance_sq([1.0, 3.0, 4.0], &collinear),
            25.0
        );
        let point = Triangle::new([[1.0, 1.0, 1.0]; 3]);
        assert_eq!(point_triangle_distance_sq([1.0, 1.0, 3.0], &point), 4.0);
    }
}
