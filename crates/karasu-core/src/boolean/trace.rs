//! Exact tracing of winding numbers along axis-parallel paths (TD-140).
//!
//! A winding-number vector is known at a reference point; the vector at another point is found by
//! walking there along the coordinate axes and counting, with their signs, the polygons crossed:
//! crossing a polygon from its back to its front (along its normal) lowers its input's winding
//! number by one, the other way raises it. Nothing is propagated around intersection curves.
//!
//! **Symbolic perturbation.** Every point of a path is moved by the same infinitesimal
//! `δ = (ε, ε², ε³)`, so a path never runs through a polygon's edge or vertex and never ends on a
//! plane: a point on a plane takes the side of the plane's first nonzero normal component, and a
//! crossing point on an edge plane takes the side its perturbation leads to. The winding numbers
//! are those of the perturbed points, consistently along every path.
//!
//! **Wide points.** A path ends at a point strictly inside a polygon of the arrangement, the
//! centroid of three of its vertices, whose coordinates outgrow [`super::plane::Point`]'s. With the
//! bounds of [`super::plane`] (`|x| < 2¹⁹⁶`, `0 < w < 2¹⁶⁸` for a vertex):
//! - a centroid `(x₀w₁w₂ + x₁w₀w₂ + x₂w₀w₁, …, 3w₀w₁w₂)` has coordinates below `2⁵³⁴` and `w`
//!   below `2⁵⁰⁶`; a path's corner mixing it with a reference point's half-integer coordinates
//!   (below `2²⁸` over 2) stays below `2⁵³⁶` (10 limbs hold `2⁶⁴⁰`);
//! - a point's side of a plane: `n · x + d·w` below `3·2⁵⁵·2⁵³⁶ + 2⁸³·2⁵⁰⁸ < 2⁵⁹³` (11 limbs);
//! - where a path's segment along axis `k` with fixed coordinates `f/W` meets a plane:
//!   `(f_j·n_k, −(Σ n_j f_j + d·W), n_k·W)` below `2⁵⁹³`, and its side of an edge plane below
//!   `3·2⁵⁵·2⁵⁹³ + 2⁸³·2⁵⁹³ < 2⁶⁷⁷` (12 limbs hold `2⁷⁶⁸`).
use super::{
    int::Int,
    plane::{Plane, Point},
    polygon::Sheet,
};
use std::cmp::Ordering;

/// A point `(x/w, y/w, z/w)` in grid steps with `w > 0`, wider than a vertex, with its
/// coordinates approximated for the filters.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Wide {
    c: [Int<10>; 3],
    w: Int<10>,
    /// `c / w`, each within a relative `2⁻⁴⁸` (see [`FILTER`]).
    approx: [f64; 3],
}

/// The relative bound of the floating-point filters: a value computed from the approximations,
/// whose magnitude exceeds `FILTER` times the sum of its terms' magnitudes, has the exact value's
/// sign.
///
/// **Why it holds.** An integer of `L ≤ 13` limbs is approximated by folding its limbs from the
/// top (one multiplication by `2⁶⁴`, exact, and one addition, rounded, per limb): within a relative
/// `13·2⁻⁵³`. A coordinate `c / w` adds both errors and the division's: `27·2⁻⁵³`. A normal or
/// offset converted to `f64` errs by `2⁻⁵³`; a product of two approximations by their errors and
/// one rounding; a sum of four terms by three roundings of at most its terms' magnitudes. A side
/// `n · x + d` therefore errs by at most `(27 + 3 + 3)·2⁻⁵³ < 2⁻⁴⁷` times the sum of its terms'
/// magnitudes; a crossing point's coordinate `−(Σ n_j a_j + d) / n_k` by `2⁻⁴⁶` times
/// `(Σ |n_j a_j| + |d|) / |n_k|`, and an edge plane's value there by `2⁻⁴⁵` times the bound used
/// below. `FILTER = 2⁻⁴⁰` leaves a factor of 32 to spare; magnitudes stay below `2⁸⁰⁰`, far
/// inside `f64`'s range.
const FILTER: f64 = 1.0 / 1_099_511_627_776.0;

/// `c / w` approximated.
fn approximate<const L: usize>(c: &[Int<L>; 3], w: &Int<L>) -> [f64; 3] {
    let w = w.approximate();
    c.map(|v| v.approximate() / w)
}

impl Wide {
    /// The point `h / 2` for half-steps `h`: a cell's corner.
    pub(crate) fn half(h: [i64; 3]) -> Self {
        #[allow(clippy::cast_precision_loss)] // Below 2^28: exact.
        let approx = h.map(|v| v as f64 / 2.0);
        Self {
            c: h.map(|v| Int::from_i128(i128::from(v))),
            w: Int::from_i128(2),
            approx,
        }
    }

    /// The point `c / w` for `w > 0`.
    #[cfg(test)]
    pub(crate) fn rational(c: [i64; 3], w: i64) -> Self {
        assert!(w > 0, "a denominator must be positive");
        let c = c.map(|v| Int::from_i128(i128::from(v)));
        let w = Int::from_i128(i128::from(w));
        Self {
            approx: approximate(&c, &w),
            c,
            w,
        }
    }

    /// The centroid of three vertices, strictly inside their triangle when they are not
    /// collinear.
    pub(crate) fn centroid(a: &Point, b: &Point, c: &Point) -> Self {
        let ((ca, wa), (cb, wb), (cc, wc)) = (a.homogeneous(), b.homogeneous(), c.homogeneous());
        let bc: Int<6> = Int::product(&wb, &wc);
        let ac: Int<6> = Int::product(&wa, &wc);
        let ab: Int<6> = Int::product(&wa, &wb);
        let coordinate = |k: usize| -> Int<10> {
            let sum: Int<10> = Int::<10>::product(&ca[k], &bc)
                .add(&Int::product(&cb[k], &ac))
                .add(&Int::product(&cc[k], &ab));
            sum
        };
        let abc: Int<10> = Int::product(&ab, &wc);
        let (c, w) = (
            [coordinate(0), coordinate(1), coordinate(2)],
            abc.add(&abc).add(&abc),
        );
        Self {
            approx: approximate(&c, &w),
            c,
            w,
        }
    }

    /// The corner of the axis-parallel path from cell corner `from` (in half-steps) to `self`
    /// after `taken` axes: coordinates `0..taken` are `self`'s, the rest `from`'s, over the common
    /// denominator `2·w`.
    fn corner(&self, from: [i64; 3], taken: usize) -> Self {
        let two = Int::<1>::from_i128(2);
        let mut c = [Int::ZERO; 3];
        for (k, value) in c.iter_mut().enumerate() {
            *value = if k < taken {
                Int::product(&self.c[k], &two)
            } else {
                Int::product(&Int::<1>::from_i128(i128::from(from[k])), &self.w)
            };
        }
        let mut approx = self.approx;
        for (k, value) in approx.iter_mut().enumerate().skip(taken) {
            #[allow(clippy::cast_precision_loss)] // Below 2^28: exact.
            let half = from[k] as f64 / 2.0;
            *value = half;
        }
        Self {
            c,
            w: Int::product(&self.w, &two),
            approx,
        }
    }

    /// The side of `plane` the point lies on, exactly (`Equal` on it).
    pub(crate) fn side(&self, plane: &Plane) -> Ordering {
        #[allow(clippy::cast_precision_loss)] // Approximations, bounded by FILTER.
        let (n, d) = (plane.n.map(|c| c as f64), plane.d as f64);
        let terms = [
            n[0] * self.approx[0],
            n[1] * self.approx[1],
            n[2] * self.approx[2],
            d,
        ];
        let value = terms.iter().sum::<f64>();
        let magnitude = terms.iter().map(|t| t.abs()).sum::<f64>();
        if value.abs() > FILTER * magnitude {
            return if value > 0.0 {
                Ordering::Greater
            } else {
                Ordering::Less
            };
        }
        self.exact_side(plane)
    }

    /// The side of `plane` the point lies on, in exact integers.
    fn exact_side(&self, plane: &Plane) -> Ordering {
        let mut value: Int<11> = Int::product(&Int::<2>::from_i128(plane.d), &self.w);
        for k in 0..3 {
            value = value.add(&Int::product(
                &Int::<1>::from_i128(i128::from(plane.n[k])),
                &self.c[k],
            ));
        }
        value.sign()
    }

    /// The side of `plane` the perturbed point lies on: never `Equal`.
    pub(crate) fn perturbed_side(&self, plane: &Plane) -> Ordering {
        match self.side(plane) {
            Ordering::Equal => first_nonzero(plane.n.map(|c| c.cmp(&0))),
            side => side,
        }
    }
}

/// The first of `signs` that is not `Equal` (`Equal` only when all are).
fn first_nonzero(signs: [Ordering; 3]) -> Ordering {
    signs
        .into_iter()
        .find(|s| *s != Ordering::Equal)
        .unwrap_or(Ordering::Equal)
}

/// The change of input `polygon.input`'s winding number on going from `from` to `to`, which
/// differ only in coordinate `axis`: `−1` crossing the polygon from back to front, `+1` the other
/// way, `0` not crossing it. Both points are perturbed.
pub(crate) fn crossing(polygon: Sheet<'_>, from: &Wide, to: &Wide, axis: usize) -> i64 {
    // A segment clear of the polygon's box crosses nothing (the box is widened past the
    // approximations' errors and the perturbation).
    let [lo, hi] = *polygon.reach;
    for k in 0..3 {
        let (a, b) = (from.approx[k], to.approx[k]);
        if a.max(b) < lo[k] || a.min(b) > hi[k] {
            return 0;
        }
    }
    let support = polygon.support;
    let (a, b) = (from.perturbed_side(support), to.perturbed_side(support));
    if a == b {
        return 0;
    }
    // The filter first: the crossing point approximated, each edge plane's side decided when its
    // value clearly exceeds its error bound. An edge clearly outside settles it.
    #[allow(clippy::cast_precision_loss)] // Approximations, bounded by FILTER.
    let (nf, df) = (support.n.map(|c| c as f64), support.d as f64);
    let mut rest = 0.0;
    let mut rest_magnitude = df.abs();
    let mut x = from.approx;
    for j in (0..3).filter(|&j| j != axis) {
        rest += nf[j] * x[j];
        rest_magnitude += (nf[j] * x[j]).abs();
    }
    x[axis] = -(rest + df) / nf[axis];
    let reach = rest_magnitude / nf[axis].abs();
    let mut undecided = Vec::new();
    for (index, edge) in polygon.edges.iter().enumerate() {
        #[allow(clippy::cast_precision_loss)] // Approximations, bounded by FILTER.
        let (e, ed) = (edge.n.map(|c| c as f64), edge.d as f64);
        let terms = [e[0] * x[0], e[1] * x[1], e[2] * x[2], ed];
        let value = terms.iter().sum::<f64>();
        let bound = FILTER * (terms.iter().map(|t| t.abs()).sum::<f64>() + e[axis].abs() * reach);
        if value < -bound {
            return 0;
        }
        if value <= bound {
            undecided.push(index);
        }
    }
    if !undecided.is_empty() && !exact_inside(polygon, from, axis, &undecided) {
        return 0;
    }
    if a == Ordering::Less {
        -1
    } else {
        1
    }
}

/// Whether the crossing of the perturbed segment along `axis` from `from` with `polygon`'s support
/// lies inside the edge planes `edges` (their indices), in exact integers.
fn exact_inside(polygon: Sheet<'_>, from: &Wide, axis: usize, edges: &[usize]) -> bool {
    let support = polygon.support;
    // The segment's fixed coordinates f/W (from `from`); n_k ≠ 0, or the sides would agree.
    let n = support.n.map(|c| Int::<1>::from_i128(i128::from(c)));
    let mut sum: Int<12> = Int::product(&Int::<2>::from_i128(support.d), &from.w);
    for j in (0..3).filter(|&j| j != axis) {
        sum = sum.add(&Int::product(&n[j], &from.c[j]));
    }
    let mut point = [Int::<12>::ZERO; 3];
    for (j, value) in point.iter_mut().enumerate() {
        *value = if j == axis {
            sum.neg()
        } else {
            Int::product(&from.c[j], &n[axis])
        };
    }
    let mut w: Int<12> = Int::product(&from.w, &n[axis]);
    if w.sign() == Ordering::Less {
        point = point.map(|v| v.neg());
        w = w.neg();
    }
    edges.iter().all(|&index| {
        let edge = &polygon.edges[index];
        let mut value: Int<13> = Int::product(&Int::<2>::from_i128(edge.d), &w);
        for (k, coordinate) in point.iter().enumerate() {
            value = value.add(&Int::product(
                &Int::<1>::from_i128(i128::from(edge.n[k])),
                coordinate,
            ));
        }
        let side = match value.sign() {
            Ordering::Equal => edge_perturbation(edge, support, axis),
            side => side,
        };
        side == Ordering::Greater
    })
}

/// The side of `edge` that the crossing of the perturbed segment along `axis` with `support`
/// takes, when the unperturbed crossing lies on `edge`. The perturbation moves the segment's fixed
/// coordinate `j` by `δ_j` (`ε`, `ε²`, `ε³` for x, y, z); the crossing then moves along the edge
/// plane's normal by `Σ δ_j (e_j n_k − e_k n_j) / n_k`, decided by the first nonzero term. Never
/// `Equal`: the edge plane is not parallel to the support, so some term is nonzero.
fn edge_perturbation(edge: &Plane, support: &Plane, axis: usize) -> Ordering {
    let (e, n) = (edge.n.map(i128::from), support.n.map(i128::from));
    let signs = [0, 1, 2].map(|j| {
        if j == axis {
            Ordering::Equal
        } else {
            (e[j] * n[axis] - e[axis] * n[j]).cmp(&0)
        }
    });
    let term = first_nonzero(signs);
    debug_assert_ne!(
        term,
        Ordering::Equal,
        "an edge plane parallel to its support"
    );
    if n[axis] < 0 {
        term.reverse()
    } else {
        term
    }
}

/// Add to `winding` the crossings of `polygons` along the axis-parallel path from the cell corner
/// `from` (in half-steps) to `to`: along x, then y, then z. Starting from a corner keeps every
/// point of the path within the bounds above.
pub(crate) fn walk<'a>(
    polygons: &(impl Iterator<Item = Sheet<'a>> + Clone),
    from: [i64; 3],
    to: &Wide,
    winding: &mut [i64],
) {
    let corners = [
        Wide::half(from),
        to.corner(from, 1),
        to.corner(from, 2),
        to.corner(from, 3),
    ];
    for axis in 0..3 {
        for polygon in polygons.clone() {
            let change = crossing(polygon, &corners[axis], &corners[axis + 1], axis);
            if change != 0 {
                winding[polygon.input as usize] += change;
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::{walk, Wide};
    use crate::boolean::{grid::GridPoint, polygon::Polygon};

    fn g(x: i64, y: i64, z: i64) -> GridPoint {
        GridPoint { x, y, z }
    }

    /// The twelve triangles of the box `[lo, hi]`, outward, as input `input`.
    pub(crate) fn cube(lo: [i64; 3], hi: [i64; 3], input: u32) -> Vec<Polygon> {
        let p = |i: usize| {
            g(
                if i & 1 == 0 { lo[0] } else { hi[0] },
                if i & 2 == 0 { lo[1] } else { hi[1] },
                if i & 4 == 0 { lo[2] } else { hi[2] },
            )
        };
        let faces = [
            [0, 2, 3, 1], // z = lo, facing down
            [4, 5, 7, 6], // z = hi, up
            [0, 1, 5, 4], // y = lo
            [2, 6, 7, 3], // y = hi
            [0, 4, 6, 2], // x = lo
            [1, 3, 7, 5], // x = hi
        ];
        faces
            .iter()
            .flat_map(|f| {
                [
                    Polygon::triangle(p(f[0]), p(f[1]), p(f[2]), input),
                    Polygon::triangle(p(f[0]), p(f[2]), p(f[3]), input),
                ]
            })
            .map(Option::unwrap)
            .collect()
    }

    #[test]
    fn walking_into_a_box_and_out_counts_its_faces() {
        let polygons = cube([0, 0, 0], [10, 10, 10], 0);
        // Half-steps: the box spans 0 to 20.
        let outside = [-3, -3, -3];
        let mut winding = [0_i64];
        walk(
            &polygons.iter().map(Polygon::sheet),
            outside,
            &Wide::half([9, 11, 7]),
            &mut winding,
        );
        assert_eq!(winding, [1]);
        let mut winding = [0_i64];
        walk(
            &polygons.iter().map(Polygon::sheet),
            [9, 11, 7],
            &Wide::half([45, 9, 9]),
            &mut winding,
        );
        assert_eq!(winding, [-1]);
        // Through a corner and along faces: the perturbation decides, and a walk there and back
        // sums to nothing.
        let mut there = [0_i64];
        walk(
            &polygons.iter().map(Polygon::sheet),
            [0, 0, 0],
            &Wide::half([20, 20, 20]),
            &mut there,
        );
        let mut back = [0_i64];
        walk(
            &polygons.iter().map(Polygon::sheet),
            [20, 20, 20],
            &Wide::half([0, 0, 0]),
            &mut back,
        );
        // A vertex of the box, perturbed by (ε, ε², ε³), is inside; the opposite one is not.
        let mut winding = [0_i64];
        walk(
            &polygons.iter().map(Polygon::sheet),
            outside,
            &Wide::half([0, 0, 0]),
            &mut winding,
        );
        assert_eq!(winding, [1]);
        let mut far = [0_i64];
        walk(
            &polygons.iter().map(Polygon::sheet),
            outside,
            &Wide::half([20, 20, 20]),
            &mut far,
        );
        assert_eq!(far, [0]);
        assert_eq!(there, [-1]);
        assert_eq!(back, [1]);
    }

    #[test]
    fn a_centroid_lies_inside_its_polygon() {
        let polygons = cube([0, 0, 0], [10, 10, 10], 0);
        let outside = [-3, -3, -3];
        for polygon in &polygons {
            let v = &polygon.vertices;
            let centroid = Wide::centroid(&v[0], &v[1], &v[2]);
            assert_eq!(centroid.side(&polygon.support), std::cmp::Ordering::Equal);
            // Perturbed, it lies on one side of its face: inside the box behind it, outside in
            // front, and the walk agrees.
            let mut winding = [0_i64];
            walk(
                &polygons.iter().map(Polygon::sheet),
                outside,
                &centroid,
                &mut winding,
            );
            let front = centroid.perturbed_side(&polygon.support) == std::cmp::Ordering::Greater;
            assert_eq!(winding, [i64::from(!front)]);
        }
    }

    #[test]
    fn the_filter_agrees_with_the_exact_sides() {
        // Centroids of random vertices against random planes, and against planes through them
        // (exactly zero): the filtered side is always the exact one.
        use crate::boolean::{
            int::tests::Rng,
            plane::{Plane, Point},
        };
        let mut rng = Rng::new(41);
        let mut g = |bits: u32| {
            let mut c = || i64::try_from(rng.signed(bits)).unwrap();
            g(c(), c(), c())
        };
        let mut zero = 0;
        for round in 0..20_000 {
            let bits = [26, 12, 4][round % 3];
            let planes: Vec<Plane> = (0..3)
                .filter_map(|_| Plane::through(g(bits), g(bits), g(bits)))
                .collect();
            let [p, q, r] = planes[..] else { continue };
            let Some(a) = Point::meet(&p, &q, &r) else {
                continue;
            };
            let (b, c) = (Point::grid(g(bits)), Point::grid(g(bits)));
            let centre = Wide::centroid(&a, &b, &c);
            for plane in [
                p,
                q,
                r,
                Plane::through(g(bits), g(bits), g(bits)).unwrap_or(p),
            ] {
                assert_eq!(centre.side(&plane), centre.exact_side(&plane));
            }
            // A plane through the origin: exactly zero there.
            let on = Plane::through(g(0), g(bits), g(bits));
            if let Some(on) = on {
                let origin = Wide::rational([0, 0, 0], 1);
                assert_eq!(origin.side(&on), origin.exact_side(&on));
                zero += usize::from(origin.side(&on) == std::cmp::Ordering::Equal);
            }
        }
        assert!(zero > 1_000, "{zero}");
    }

    #[test]
    fn two_overlapping_boxes_count_two_where_both_are() {
        let mut polygons = cube([0, 0, 0], [10, 10, 10], 0);
        polygons.extend(cube([5, 5, 5], [15, 15, 15], 1));
        let mut winding = [0_i64; 2];
        walk(
            &polygons.iter().map(Polygon::sheet),
            [-1, -1, -1],
            &Wide::half([15, 15, 15]),
            &mut winding,
        );
        assert_eq!(winding, [1, 1]);
        let mut winding = [0_i64; 2];
        walk(
            &polygons.iter().map(Polygon::sheet),
            [-1, -1, -1],
            &Wide::half([25, 25, 25]),
            &mut winding,
        );
        assert_eq!(winding, [0, 1]);
    }
}
