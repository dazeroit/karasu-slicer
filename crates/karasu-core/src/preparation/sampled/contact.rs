//! Exact closed-set intersection tests for segments and triangles in represented coordinates.
//!
//! Every decision uses exact orientation signs (`orient3d`, and projected 2D orientations obtained
//! from it), so the answer is exact for the binary64 coordinates as given. Two closed triangles
//! intersect exactly when an edge of one meets the other: their intersection is a convex set whose
//! extreme points lie on edges of one of them. Degenerate (zero-area) triangles are handled through
//! their edges, so no case is silently skipped.

// Predicate notation: segments p q / a b, triangles s t, axis k, orientations o1..o4.
#![allow(clippy::many_single_char_names)]

use crate::{
    geometry::Point3,
    preparation::hollow::numeric::{orient3d, I},
};
use std::cmp::Ordering;

fn point(c: [f64; 3]) -> Point3 {
    Point3::new(c[0], c[1], c[2])
}

/// Exact sign of the orientation of `a, b, c` projected onto the plane orthogonal to `axis`.
///
/// Equals the `axis` component of `(b - a) x (c - a)`. An outward-rounded interval evaluation of
/// that 2 x 2 determinant decides most signs; otherwise `orient3d` with the fourth point `a`
/// lifted by one along `axis` gives the exact sign for every admitted coordinate (|v| < 2^52).
pub(crate) fn orient_projected(a: [f64; 3], b: [f64; 3], c: [f64; 3], axis: usize) -> Ordering {
    let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
    let difference = |p: [f64; 3], k: usize| I::point(p[k]).sub(I::point(a[k]));
    let determinant = difference(b, u)
        .mul(difference(c, v))
        .sub(difference(b, v).mul(difference(c, u)));
    if determinant.lo > 0.0 {
        return Ordering::Greater;
    }
    if determinant.hi < 0.0 {
        return Ordering::Less;
    }
    let mut lifted = a;
    lifted[axis] += 1.0;
    orient3d(point(a), point(b), point(c), point(lifted))
}

/// Closed 2D segment intersection in the projection orthogonal to `axis` (exact).
fn segments_meet_projected(
    p: [f64; 3],
    q: [f64; 3],
    a: [f64; 3],
    b: [f64; 3],
    axis: usize,
) -> bool {
    let o1 = orient_projected(p, q, a, axis);
    let o2 = orient_projected(p, q, b, axis);
    let o3 = orient_projected(a, b, p, axis);
    let o4 = orient_projected(a, b, q, axis);
    if o1 != o2
        && o3 != o4
        && o1 != Ordering::Equal
        && o2 != Ordering::Equal
        && o3 != Ordering::Equal
        && o4 != Ordering::Equal
    {
        return true;
    }
    let (u, v) = match axis {
        0 => (1, 2),
        1 => (2, 0),
        _ => (0, 1),
    };
    // Collinear touching cases: a point of one segment inside the other's projected box.
    let on = |s: [f64; 3], e: [f64; 3], r: [f64; 3]| {
        r[u] >= s[u].min(e[u])
            && r[u] <= s[u].max(e[u])
            && r[v] >= s[v].min(e[v])
            && r[v] <= s[v].max(e[v])
    };
    (o1 == Ordering::Equal && on(p, q, a))
        || (o2 == Ordering::Equal && on(p, q, b))
        || (o3 == Ordering::Equal && on(a, b, p))
        || (o4 == Ordering::Equal && on(a, b, q))
}

/// Closed point-in-triangle test in the projection orthogonal to `axis` (exact).
fn inside_projected(t: [[f64; 3]; 3], r: [f64; 3], axis: usize) -> bool {
    let s = [
        orient_projected(t[0], t[1], r, axis),
        orient_projected(t[1], t[2], r, axis),
        orient_projected(t[2], t[0], r, axis),
    ];
    !(s.contains(&Ordering::Greater) && s.contains(&Ordering::Less))
}

/// Exact test whether the closed segment `pq` meets the closed triangle `t`.
pub(crate) fn segment_meets_triangle(p: [f64; 3], q: [f64; 3], t: [[f64; 3]; 3]) -> bool {
    let [a, b, c] = t;
    let sp = orient3d(point(a), point(b), point(c), point(p));
    let sq = orient3d(point(a), point(b), point(c), point(q));
    if sp == sq && sp != Ordering::Equal {
        return false;
    }
    if sp == Ordering::Equal && sq == Ordering::Equal {
        // Coplanar: project onto a coordinate plane where the triangle keeps positive area. A
        // zero-area triangle in every projection is a segment or point; test its edges instead.
        if let Some(axis) = (0..3)
            .rev()
            .find(|&k| orient_projected(a, b, c, k) != Ordering::Equal)
        {
            return inside_projected(t, p, axis)
                || inside_projected(t, q, axis)
                || [(a, b), (b, c), (c, a)]
                    .into_iter()
                    .any(|(u, v)| segments_meet_projected(p, q, u, v, axis));
        }
        return degenerate_triangle_meets_segment(t, p, q);
    }
    // The segment reaches the triangle's plane at one point; it lies in the closed triangle when
    // the segment line passes on one consistent side of (or through) all three edges.
    let s = [
        orient3d(point(p), point(q), point(a), point(b)),
        orient3d(point(p), point(q), point(b), point(c)),
        orient3d(point(p), point(q), point(c), point(a)),
    ];
    !(s.contains(&Ordering::Greater) && s.contains(&Ordering::Less))
}

/// A zero-area triangle coplanar with segment `pq`: test each of its edges as a segment.
fn degenerate_triangle_meets_segment(t: [[f64; 3]; 3], p: [f64; 3], q: [f64; 3]) -> bool {
    [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])]
        .into_iter()
        .any(|(u, v)| segments_meet(p, q, u, v))
}

/// Exact closed point-on-segment test.
#[allow(clippy::float_cmp)] // Exact coordinate identity.
fn point_on_segment(r: [f64; 3], a: [f64; 3], b: [f64; 3]) -> bool {
    if a == b {
        return r == a;
    }
    (0..3).all(|k| orient_projected(a, b, r, k) == Ordering::Equal)
        && (0..3).all(|k| r[k] >= a[k].min(b[k]) && r[k] <= a[k].max(b[k]))
}

/// Exact closed 3D segment intersection.
#[allow(clippy::float_cmp)] // Exact coordinate identity.
fn segments_meet(p: [f64; 3], q: [f64; 3], a: [f64; 3], b: [f64; 3]) -> bool {
    if p == q {
        return point_on_segment(p, a, b);
    }
    if a == b {
        return point_on_segment(a, p, q);
    }
    // Non-coplanar segments never meet.
    if orient3d(point(p), point(q), point(a), point(b)) != Ordering::Equal {
        return false;
    }
    // Coplanar (or collinear): any coordinate projection that is not degenerate for both
    // segments preserves incidence; try all three, requiring agreement with a box test.
    let in_box = |r: [f64; 3]| (0..3).all(|k| r[k] >= p[k].min(q[k]) && r[k] <= p[k].max(q[k]));
    let in_box_ab = |r: [f64; 3]| (0..3).all(|k| r[k] >= a[k].min(b[k]) && r[k] <= a[k].max(b[k]));
    let overlap =
        (0..3).all(|k| p[k].min(q[k]) <= a[k].max(b[k]) && a[k].min(b[k]) <= p[k].max(q[k]));
    if !overlap {
        return false;
    }
    // Coplanar segments meet iff they meet in every coordinate projection (projections are
    // necessary), and in a projection where they are not both collinear with the projection
    // direction the test is also sufficient; collinear-in-3D segments reduce to box overlap of
    // shared points.
    let all_collinear = (0..3).all(|k| {
        orient_projected(p, q, a, k) == Ordering::Equal
            && orient_projected(p, q, b, k) == Ordering::Equal
    });
    if all_collinear {
        return in_box(a) || in_box(b) || in_box_ab(p) || in_box_ab(q);
    }
    (0..3).all(|k| segments_meet_projected(p, q, a, b, k))
}

/// Exact test whether two closed triangles intersect (share at least one point).
pub(crate) fn triangles_meet(s: [[f64; 3]; 3], t: [[f64; 3]; 3]) -> bool {
    let edges = |u: [[f64; 3]; 3]| [(u[0], u[1]), (u[1], u[2]), (u[2], u[0])];
    edges(s)
        .into_iter()
        .any(|(p, q)| segment_meets_triangle(p, q, t))
        || edges(t)
            .into_iter()
            .any(|(p, q)| segment_meets_triangle(p, q, s))
}

/// Exact test whether two triangles of one mesh meet anywhere other than what their shared
/// vertex indices allow, i.e. whether the pair violates an embedded, pairwise-disjoint surface.
///
/// - No shared vertex: any contact is improper.
/// - One shared vertex `w`: improper if an edge opposite `w` meets the other triangle, or an edge
///   incident to `w` lies in the other triangle's plane and enters its closed corner at `w`.
/// - Two shared vertices (a shared edge): improper if the triangles are coplanar and fold onto
///   the same side of that edge.
/// - Three shared vertices: a duplicated face, improper.
///
/// Two quick exact decisions come first. If the unshared vertices of one triangle lie strictly on
/// one side of the other's plane, the triangles meet at most in their shared vertices or edge.
/// Coplanar triangles sharing exactly one vertex `w` are decided in the plane: their intersection
/// is convex and contains `w`, so it contains another point exactly when their corners at `w`
/// overlap, i.e. when one corner contains a boundary ray of the other.
pub(crate) fn improper_contact(
    fa: [u32; 3],
    ta: [[f64; 3]; 3],
    fb: [u32; 3],
    tb: [[f64; 3]; 3],
) -> bool {
    let mut shared = [(0, 0); 2];
    let mut count = 0;
    for (i, &va) in fa.iter().enumerate() {
        for (j, &vb) in fb.iter().enumerate() {
            if va == vb {
                if count == 2 {
                    // A duplicated face (or repeated indices): improper.
                    return true;
                }
                shared[count] = (i, j);
                count += 1;
            }
        }
    }
    let shared_a = |k: usize| shared[..count].iter().any(|&(i, _)| i == k);
    let shared_b = |k: usize| shared[..count].iter().any(|&(_, j)| j == k);
    let b_side = plane_sides(ta, tb, shared_b);
    if b_side == Sides::Strict {
        return false;
    }
    if count == 2 {
        // One unshared vertex each, and `b`'s lies in `a`'s plane (or `a` has none).
        let [(i0, j0), (i1, j1)] = shared;
        if i0 == i1 || j0 == j1 {
            // Repeated indices within a face (never admitted): treat as improper.
            return true;
        }
        return shared_edge_improper(ta[i0], ta[i1], ta[3 - i0 - i1], tb[3 - j0 - j1]);
    }
    if b_side == Sides::Coplanar {
        let decided = if count == 0 {
            coplanar_triangles_meet(ta, tb)
        } else {
            coplanar_corners_overlap(ta, shared[0].0, tb, shared[0].1)
        };
        if let Some(improper) = decided {
            return improper;
        }
    }
    if plane_sides(tb, ta, shared_a) == Sides::Strict {
        return false;
    }
    if count == 0 {
        return triangles_meet(ta, tb);
    }
    let (i, j) = shared[0];
    shared_vertex_improper(ta, i, tb, j) || shared_vertex_improper(tb, j, ta, i)
}

/// Projection axis in which `t` keeps positive area, with its projected turn; `None` for a
/// zero-area triangle.
fn turn(t: [[f64; 3]; 3]) -> Option<(usize, Ordering)> {
    (0..3).rev().find_map(|k| {
        let turn = orient_projected(t[0], t[1], t[2], k);
        (turn != Ordering::Equal).then_some((k, turn))
    })
}

/// Coplanar triangles sharing no vertex: do they meet? `None` when either has zero area.
///
/// Two closed convex polygons are disjoint exactly when one of them lies strictly outside the
/// supporting line of an edge of the other: the origin lies outside the Minkowski difference,
/// so strictly outside one of its edges, each of which is parallel to an edge of either polygon.
fn coplanar_triangles_meet(a: [[f64; 3]; 3], b: [[f64; 3]; 3]) -> Option<bool> {
    let (axis, turn_a) = turn(a)?;
    let turn_b = orient_projected(b[0], b[1], b[2], axis);
    if turn_b == Ordering::Equal {
        return None;
    }
    let separates = |t: [[f64; 3]; 3], turn: Ordering, other: [[f64; 3]; 3]| {
        (0..3).any(|k| {
            let (p, q) = (t[k], t[(k + 1) % 3]);
            other
                .iter()
                .all(|&r| orient_projected(p, q, r, axis) == turn.reverse())
        })
    };
    Some(!(separates(a, turn_a, b) || separates(b, turn_b, a)))
}

/// Triangles `u v c` and `u v d` sharing the edge `u v`, with `d` in the plane of `u v c` (or
/// `u v c` without a plane): do they meet beyond the shared edge?
///
/// - `u v c` has positive area: the line through `u` and `v` meets it in the edge only, so the
///   pair meets beyond the edge exactly when `u v d` folds onto the same side of the edge.
/// - `u v c` is collinear: a triangle `u v d` with positive area meets that line in the edge
///   only, so the pair is proper. When `d` is collinear too, both faces are segments on one line
///   and overlap beyond the edge exactly when `c` and `d` lie beyond the same endpoint.
#[allow(clippy::float_cmp)] // Exact coordinate order along the common line.
fn shared_edge_improper(u: [f64; 3], v: [f64; 3], c: [f64; 3], d: [f64; 3]) -> bool {
    if let Some(k) = (0..3)
        .rev()
        .find(|&k| orient_projected(u, v, c, k) != Ordering::Equal)
    {
        return orient_projected(u, v, c, k) == orient_projected(u, v, d, k);
    }
    if (0..3).any(|k| orient_projected(u, v, d, k) != Ordering::Equal) {
        return false;
    }
    // Coordinate `k` orders the points of the common line; coincident shared vertices leave no
    // line, and the pair is then treated as improper.
    let Some(k) = (0..3).find(|&k| u[k] != v[k]) else {
        return true;
    };
    let beyond = |x: [f64; 3], end: [f64; 3], other: [f64; 3]| {
        if other[k] > end[k] {
            x[k] < end[k]
        } else {
            x[k] > end[k]
        }
    };
    (beyond(c, u, v) && beyond(d, u, v)) || (beyond(c, v, u) && beyond(d, v, u))
}

/// Position of the unshared vertices of `b` relative to the plane of `a`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Sides {
    /// All strictly on one side.
    Strict,
    /// All exactly on the plane (also when `a` has no plane).
    Coplanar,
    /// Anything else.
    Mixed,
}

fn plane_sides(a: [[f64; 3]; 3], b: [[f64; 3]; 3], shared: impl Fn(usize) -> bool) -> Sides {
    let mut seen = [false; 3];
    for (k, &vertex) in b.iter().enumerate() {
        if shared(k) {
            continue;
        }
        let side = orient3d(point(a[0]), point(a[1]), point(a[2]), point(vertex));
        seen[match side {
            Ordering::Less => 0,
            Ordering::Equal => 1,
            Ordering::Greater => 2,
        }] = true;
    }
    match seen {
        [true, false, false] | [false, false, true] => Sides::Strict,
        [false, _, false] => Sides::Coplanar,
        _ => Sides::Mixed,
    }
}

/// Coplanar triangles sharing vertex `a[i] = b[j]` only: do their closed corners at that vertex
/// overlap beyond it? `None` when either triangle is degenerate (decided by the general test).
///
/// Each corner is a closed angular sector narrower than a half turn in a projection where the
/// common plane keeps positive area, so two corners overlap exactly when one contains a boundary
/// ray of the other.
fn coplanar_corners_overlap(
    a: [[f64; 3]; 3],
    i: usize,
    b: [[f64; 3]; 3],
    j: usize,
) -> Option<bool> {
    let w = a[i];
    let (x, y) = (a[(i + 1) % 3], a[(i + 2) % 3]);
    let (u, v) = (b[(j + 1) % 3], b[(j + 2) % 3]);
    // Cyclic order keeps each triangle's turn: (w, x, y) turns like `a`.
    let (axis, turn_a) = turn(a)?;
    let turn_b = orient_projected(w, u, v, axis);
    if turn_b == Ordering::Equal {
        return None;
    }
    // Ray from w through r inside the closed sector from `first` to `second` turning `turn`.
    let within = |first: [f64; 3], second: [f64; 3], turn: Ordering, r: [f64; 3]| {
        let start = orient_projected(w, first, r, axis);
        let end = orient_projected(w, r, second, axis);
        (start == turn || start == Ordering::Equal) && (end == turn || end == Ordering::Equal)
    };
    Some(
        within(x, y, turn_a, u)
            || within(x, y, turn_a, v)
            || within(u, v, turn_b, x)
            || within(u, v, turn_b, y),
    )
}

/// With `a[i]` the vertex shared with `b[j]`: does `a` reach into `b` beyond that vertex?
fn shared_vertex_improper(a: [[f64; 3]; 3], i: usize, b: [[f64; 3]; 3], j: usize) -> bool {
    let w = a[i];
    let (x, y) = (a[(i + 1) % 3], a[(i + 2) % 3]);
    // The edge opposite w does not contain w, so any contact with b is improper.
    if segment_meets_triangle(x, y, b) {
        return true;
    }
    // An edge w->x meets b's plane only at w unless x is coplanar with b; then it is improper
    // exactly when it enters b's closed corner at w (collinear overlap with an edge included).
    let (b1, b2) = (b[(j + 1) % 3], b[(j + 2) % 3]);
    [x, y].into_iter().any(|end| {
        if orient3d(point(b[0]), point(b[1]), point(b[2]), point(end)) != Ordering::Equal {
            return false;
        }
        let Some(k) = (0..3)
            .rev()
            .find(|&k| orient_projected(w, b1, b2, k) != Ordering::Equal)
        else {
            return true;
        };
        let corner = orient_projected(w, b1, b2, k);
        let first = orient_projected(w, b1, end, k);
        let second = orient_projected(w, end, b2, k);
        (first == corner || first == Ordering::Equal)
            && (second == corner || second == Ordering::Equal)
    })
}

#[cfg(test)]
mod tests {
    use super::{segment_meets_triangle, triangles_meet};

    const T: [[f64; 3]; 3] = [[0.0, 0.0, 0.0], [4.0, 0.0, 0.0], [0.0, 4.0, 0.0]];

    #[test]
    fn segments_crossing_touching_and_missing_the_triangle() {
        assert!(segment_meets_triangle([1.0, 1.0, -1.0], [1.0, 1.0, 1.0], T));
        // Touching at a vertex, an edge and the plane boundary exactly.
        assert!(segment_meets_triangle([0.0, 0.0, -1.0], [0.0, 0.0, 1.0], T));
        assert!(segment_meets_triangle([2.0, 2.0, -1.0], [2.0, 2.0, 1.0], T));
        assert!(segment_meets_triangle([1.0, 1.0, 0.0], [1.0, 1.0, 5.0], T));
        assert!(!segment_meets_triangle(
            [2.5, 2.5, -1.0],
            [2.5, 2.5, 1.0],
            T
        ));
        assert!(!segment_meets_triangle([1.0, 1.0, 0.5], [1.0, 1.0, 1.0], T));
        // Coplanar: crossing an edge, inside, and disjoint.
        assert!(segment_meets_triangle([-1.0, 1.0, 0.0], [1.0, 1.0, 0.0], T));
        assert!(segment_meets_triangle([0.5, 0.5, 0.0], [1.0, 1.0, 0.0], T));
        assert!(!segment_meets_triangle([3.0, 3.0, 0.0], [5.0, 3.0, 0.0], T));
        // Coplanar and collinear with an edge, overlapping and separate.
        assert!(segment_meets_triangle([3.0, 0.0, 0.0], [6.0, 0.0, 0.0], T));
        assert!(!segment_meets_triangle([5.0, 0.0, 0.0], [6.0, 0.0, 0.0], T));
    }

    #[test]
    fn mesh_pairs_respect_shared_vertices() {
        use super::improper_contact;
        let a = [[0.0, 0.0, 0.0], [4.0, 0.0, 0.0], [0.0, 4.0, 0.0]];
        // Proper edge neighbour (folded up) and an improper coplanar fold onto the same side.
        let up = [[4.0, 0.0, 0.0], [0.0, 4.0, 0.0], [3.0, 3.0, 2.0]];
        assert!(!improper_contact([0, 1, 2], a, [1, 2, 3], up));
        let folded = [[4.0, 0.0, 0.0], [0.0, 4.0, 0.0], [1.0, 1.0, 0.0]];
        assert!(improper_contact([0, 1, 2], a, [1, 2, 3], folded));
        // Vertex neighbours: touching only at the vertex, and crossing through the face.
        let fan = [[0.0, 0.0, 0.0], [-1.0, -3.0, 1.0], [-3.0, -1.0, 1.0]];
        assert!(!improper_contact([0, 1, 2], a, [0, 3, 4], fan));
        let piercing = [[0.0, 0.0, 0.0], [2.0, 1.0, -1.0], [1.0, 2.0, 1.0]];
        assert!(improper_contact([0, 1, 2], a, [0, 3, 4], piercing));
        // Coplanar vertex neighbour entering the corner, and one staying outside it.
        let inside = [[0.0, 0.0, 0.0], [1.0, 1.0, 0.0], [-1.0, 2.0, 0.0]];
        assert!(improper_contact([0, 1, 2], a, [0, 3, 4], inside));
        let outside = [[0.0, 0.0, 0.0], [-1.0, -1.0, 0.0], [-2.0, 1.0, 0.0]];
        assert!(!improper_contact([0, 1, 2], a, [0, 3, 4], outside));
        // Unrelated faces: separated and touching.
        let far = [[5.0, 5.0, 0.0], [6.0, 5.0, 0.0], [5.0, 6.0, 0.0]];
        assert!(!improper_contact([0, 1, 2], a, [5, 6, 7], far));
        let lid = [[1.0, 1.0, 0.0], [3.0, 1.0, 1.0], [1.0, 3.0, 1.0]];
        assert!(improper_contact([0, 1, 2], a, [5, 6, 7], lid));
    }

    /// The general rules alone, without the quick plane-side and coplanar-corner decisions.
    fn reference_improper(
        fa: [u32; 3],
        ta: [[f64; 3]; 3],
        fb: [u32; 3],
        tb: [[f64; 3]; 3],
    ) -> bool {
        use super::{orient_projected, point, shared_vertex_improper};
        use crate::preparation::hollow::numeric::orient3d;
        use std::cmp::Ordering;
        let shared: Vec<(usize, usize)> = (0..3)
            .flat_map(|i| (0..3).map(move |j| (i, j)))
            .filter(|&(i, j)| fa[i] == fb[j])
            .collect();
        match shared.len() {
            0 => triangles_meet(ta, tb),
            1 => {
                let (i, j) = shared[0];
                shared_vertex_improper(ta, i, tb, j) || shared_vertex_improper(tb, j, ta, i)
            }
            2 => {
                let (i0, i1) = (shared[0].0, shared[1].0);
                let c = ta[3 - i0 - i1];
                let d = tb[(0..3)
                    .find(|&k| fb[k] != fa[i0] && fb[k] != fa[i1])
                    .unwrap()];
                let (u, v) = (ta[i0], ta[i1]);
                if orient3d(point(u), point(v), point(c), point(d)) != Ordering::Equal {
                    return false;
                }
                (0..3)
                    .rev()
                    .find(|&k| orient_projected(u, v, c, k) != Ordering::Equal)
                    .is_some_and(|k| orient_projected(u, v, c, k) == orient_projected(u, v, d, k))
            }
            _ => true,
        }
    }

    #[test]
    fn quick_decisions_agree_with_the_general_rules() {
        use super::improper_contact;
        fn next(state: &mut u64, n: u64) -> u64 {
            *state ^= *state << 13;
            *state ^= *state >> 7;
            *state ^= *state << 17;
            *state % n
        }
        // Small grid coordinates make coplanar, collinear and touching cases frequent; every
        // fourth round leaves the plane z = 0.
        fn random(state: &mut u64, planar: bool) -> [f64; 3] {
            let z = if planar { 2 } else { next(state, 5) };
            [next(state, 5), next(state, 5), z].map(|c| f64::from(u32::try_from(c).unwrap()) - 2.0)
        }
        let state = &mut 0x853c_49e6_748f_ea9b_u64;
        let mut improper = 0;
        for round in 0..40_000 {
            let planar = round % 4 != 0;
            let ta = [0; 3].map(|_| random(state, planar));
            let fa = [0, 1, 2];
            let count = usize::try_from(next(state, 3)).unwrap();
            let mut tb = [0; 3].map(|_| random(state, planar));
            let mut fb = [3, 4, 5];
            for k in 0..count {
                let from = (k + usize::try_from(next(state, 3)).unwrap()) % 3;
                if fb.contains(&fa[from]) {
                    continue;
                }
                tb[k] = ta[from];
                fb[k] = fa[from];
            }
            let turn = usize::try_from(next(state, 3)).unwrap();
            tb.rotate_left(turn);
            fb.rotate_left(turn);
            // The general rules are not exact for zero-area triangles; those are covered by
            // the targeted cases below.
            let degenerate = |t: [[f64; 3]; 3]| {
                (0..3).all(|k| super::orient_projected(t[0], t[1], t[2], k).is_eq())
            };
            if degenerate(ta) || degenerate(tb) {
                continue;
            }
            let expected = reference_improper(fa, ta, fb, tb);
            improper += usize::from(expected);
            assert_eq!(
                improper_contact(fa, ta, fb, tb),
                expected,
                "{fa:?} {ta:?} {fb:?} {tb:?}"
            );
            assert_eq!(improper_contact(fb, tb, fa, ta), expected);
        }
        assert!(improper > 4_000 && improper < 30_000, "{improper}");
        // A zero-area neighbour whose other points lie strictly on one side of the plane meets it
        // only at the shared vertex; the general corner rule could not decide this exactly.
        let segment = [[1.0, 1.0, -2.0], [1.0, -2.0, 0.0], [1.0, -2.0, 0.0]];
        let face = [[-2.0, -2.0, 0.0], [-2.0, -1.0, -1.0], [1.0, 1.0, -2.0]];
        assert!(!improper_contact([0, 1, 2], segment, [4, 5, 0], face));
        assert!(reference_improper([0, 1, 2], segment, [4, 5, 0], face));
        // A zero-area neighbour lying in the plane and entering the corner is improper.
        let a = [[0.0, 0.0, 0.0], [4.0, 0.0, 0.0], [0.0, 4.0, 0.0]];
        let inside = [[0.0, 0.0, 0.0], [1.0, 1.0, 0.0], [2.0, 2.0, 0.0]];
        assert!(improper_contact([0, 1, 2], a, [0, 3, 4], inside));
        // Collinear faces sharing an edge (review counterexamples): overlapping beyond the edge
        // is improper in both orders; extending to opposite sides, or a collinear face next to
        // one with positive area, is proper.
        let first = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [3.0, 0.0, 0.0]];
        let second = [[1.0, 0.0, 0.0], [0.0, 0.0, 0.0], [2.0, 0.0, 0.0]];
        assert!(improper_contact([0, 1, 2], first, [1, 0, 5], second));
        assert!(improper_contact([1, 0, 5], second, [0, 1, 2], first));
        let vertical_a = [[0.0, 0.25, 0.25], [0.0, 0.25, 0.125], [0.0, 0.25, -0.25]];
        let vertical_b = [[0.0, 0.25, 0.0], [0.0, 0.25, 0.25], [0.0, 0.25, 0.125]];
        assert!(improper_contact(
            [0, 1, 2],
            vertical_a,
            [5, 0, 1],
            vertical_b
        ));
        assert!(improper_contact(
            [5, 0, 1],
            vertical_b,
            [0, 1, 2],
            vertical_a
        ));
        let opposite = [[1.0, 0.0, 0.0], [0.0, 0.0, 0.0], [-2.0, 0.0, 0.0]];
        assert!(!improper_contact([0, 1, 2], first, [1, 0, 5], opposite));
        let flat = [[1.0, 0.0, 0.0], [0.0, 0.0, 0.0], [0.5, -1.0, 0.0]];
        assert!(!improper_contact([0, 1, 2], first, [1, 0, 5], flat));
        assert!(!improper_contact([1, 0, 5], flat, [0, 1, 2], first));
    }

    #[test]
    fn triangle_pairs_classify_exactly() {
        // Crossing, touching at a shared vertex, coplanar overlap, coplanar disjoint, parallel.
        let crossing = [[1.0, 1.0, -1.0], [1.0, 1.0, 1.0], [1.5, 0.5, 1.0]];
        assert!(triangles_meet(T, crossing));
        let vertex = [[4.0, 0.0, 0.0], [5.0, 0.0, 1.0], [5.0, 1.0, 1.0]];
        assert!(triangles_meet(T, vertex));
        let inner = [[0.5, 0.5, 0.0], [1.0, 0.5, 0.0], [0.5, 1.0, 0.0]];
        assert!(triangles_meet(T, inner));
        let disjoint = [[5.0, 5.0, 0.0], [6.0, 5.0, 0.0], [5.0, 6.0, 0.0]];
        assert!(!triangles_meet(T, disjoint));
        let parallel = [[0.0, 0.0, 1e-12], [4.0, 0.0, 1e-12], [0.0, 4.0, 1e-12]];
        assert!(!triangles_meet(T, parallel));
        let pierced = [[1.0, 1.0, -1.0], [1.2, 1.0, 1.0], [1.0, 1.2, 1.0]];
        assert!(triangles_meet(T, pierced));
    }
}
