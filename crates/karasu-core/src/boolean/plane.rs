//! Planes with integer coefficients and points as exact intersections of three planes (TD-140).
//!
//! Every plane the kernel uses passes through grid points ([`super::grid`]), whose coordinates
//! have magnitude at most `G = 65,536,000 < 2²⁶`. Every vertex the kernel makes is the meeting
//! point of three such planes, kept exactly as homogeneous integers, never rounded. Predicates
//! are integer signs, decided without any floating point, the same on every platform.
//!
//! **Bounds** (each proved here, and exercised at its extreme by the tests):
//! - A difference of grid coordinates is below `2·G < 2²⁷`.
//! - A **supporting plane** through grid points `a, b, c` has the normal `n = (b − a) × (c − a)`:
//!   each component is a difference of two products of differences, below `2·(2²⁷)² = 2⁵⁵`
//!   (`i64`). Its offset `d = −n · a` is below `3·2⁵⁵·2²⁶ < 2⁸³` (`i128`).
//! - An **extrusion plane** through grid points `a, b` and parallel to a coordinate axis has a
//!   normal `(b − a) × e`, whose components are differences below `2²⁷`: within the same bounds.
//! - The **meeting point** of three planes is `(x, y, z, w)` with `w = det[N]` and `x, y, z`
//!   determinants with the offsets in one column (Cramer's rule), `N` the matrix of normals:
//!   - a cross product of two columns of normals has components below `2·(2⁵⁵)² = 2¹¹¹`
//!     (`i128`);
//!   - `|w| < 3·2⁵⁵·2¹¹¹ < 2¹⁶⁸` (3 limbs);
//!   - `|x|, |y|, |z| < 3·2⁸³·2¹¹¹ < 2¹⁹⁶` (4 limbs).
//! - **Which side** of a plane `(n, d)` a point lies: the sign of `n · (x, y, z) + d·w`, below
//!   `3·2⁵⁵·2¹⁹⁶ + 2⁸³·2¹⁶⁸ < 2²⁵⁴` (computed in 5 limbs).
//! - **Equality** of two meeting points: `x₁·w₂ = x₂·w₁` on each axis, products below
//!   `2¹⁹⁶·2¹⁶⁸ = 2³⁶⁴` (6 limbs).
use super::{grid::GridPoint, int::Int};
use std::cmp::Ordering;

/// A plane `n · p + d = 0`; its positive side is where `n · p + d > 0`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Plane {
    /// The normal, each component below 2⁵⁵.
    pub(crate) n: [i64; 3],
    /// The offset, below 2⁸³.
    pub(crate) d: i128,
}

/// `a × b` for vectors of `i64` components below 2⁵⁵ apart from the first factor below 2²⁷:
/// generic over the exact products, in `i128`.
fn cross(a: [i128; 3], b: [i128; 3]) -> [i128; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// `b − a` for grid points.
fn difference(a: GridPoint, b: GridPoint) -> [i128; 3] {
    let (a, b) = (a.coordinates(), b.coordinates());
    [0, 1, 2].map(|k| i128::from(b[k]) - i128::from(a[k]))
}

impl Plane {
    /// The plane through `a`, `b` and `c`, its positive side where `(a, b, c, p)` turns
    /// positively (`orient3d(a, b, c, p) > 0`); `None` when the three are collinear.
    pub(crate) fn through(a: GridPoint, b: GridPoint, c: GridPoint) -> Option<Self> {
        Self::from_normal(cross(difference(a, b), difference(a, c)), a)
    }

    /// The plane through `a` and `b` parallel to the coordinate axis `axis` (0, 1, 2): it meets
    /// any plane not parallel to that axis along the line `ab`. `None` when `ab` is parallel to
    /// the axis, or `a = b`.
    pub(crate) fn extrusion(a: GridPoint, b: GridPoint, axis: usize) -> Option<Self> {
        let mut e = [0_i128; 3];
        e[axis] = 1;
        Self::from_normal(cross(difference(a, b), e), a)
    }

    /// The plane of normal `normal` through `a`; `None` for a zero normal.
    fn from_normal(normal: [i128; 3], a: GridPoint) -> Option<Self> {
        if normal == [0; 3] {
            return None;
        }
        let n = normal.map(|c| i64::try_from(c).expect("a normal below 2^55 (TD-140)"));
        let a = a.coordinates();
        let d = -(0..3)
            .map(|k| i128::from(n[k]) * i128::from(a[k]))
            .sum::<i128>();
        Some(Self { n, d })
    }

    /// The same plane with its sides exchanged.
    #[must_use]
    pub(crate) fn flipped(self) -> Self {
        Self {
            n: self.n.map(|c| -c),
            d: -self.d,
        }
    }

    /// Which side of the plane grid point `p` lies on: `Greater` positive, `Equal` on it.
    pub(crate) fn side_of_grid(&self, p: GridPoint) -> Ordering {
        // |n · p| < 3·2^55·2^26 and |d| < 2^83: below 2^85, exact in i128.
        let p = p.coordinates();
        let value = (0..3)
            .map(|k| i128::from(self.n[k]) * i128::from(p[k]))
            .sum::<i128>()
            + self.d;
        value.cmp(&0)
    }

    /// Which side of the plane point `p` lies on: `Greater` positive, `Equal` on it.
    pub(crate) fn side(&self, p: &Point) -> Ordering {
        let n = self.n.map(|c| Int::<1>::from_i128(i128::from(c)));
        let mut value: Int<5> = Int::product(&Int::<2>::from_i128(self.d), &p.w);
        for (k, coordinate) in [&p.x, &p.y, &p.z].into_iter().enumerate() {
            value = value.add(&Int::product(&n[k], coordinate));
        }
        // w > 0, so the sign is the side's.
        value.sign()
    }

    /// Whether the two planes are parallel (or the same), whichever their sides.
    pub(crate) fn parallel(&self, other: &Self) -> bool {
        let wide = |n: [i64; 3]| n.map(i128::from);
        cross(wide(self.n), wide(other.n)) == [0; 3]
    }
}

/// A point as the exact meeting of three planes: `(x/w, y/w, z/w)` in grid steps, `w > 0`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Point {
    x: Int<4>,
    y: Int<4>,
    z: Int<4>,
    w: Int<3>,
}

/// `c · (a × b)` for columns `c` of offsets below 2⁸³ and `a × b` below 2¹¹¹, in 4 limbs.
fn dot_offsets(c: [i128; 3], cross: [i128; 3]) -> Int<4> {
    let mut sum = Int::<4>::ZERO;
    for k in 0..3 {
        sum = sum.add(&Int::product_i128(c[k], cross[k]));
    }
    sum
}

impl Point {
    /// The point where `a`, `b` and `c` meet; `None` when they do not meet in exactly one point
    /// (two of them parallel, or all three through one line).
    pub(crate) fn meet(a: &Plane, b: &Plane, c: &Plane) -> Option<Self> {
        // Columns of the system N p = −D: the normals' x, y and z components and the offsets.
        let column = |k: usize| [a.n[k], b.n[k], c.n[k]].map(i128::from);
        let (nx, ny, nz) = (column(0), column(1), column(2));
        let offsets = [a.d, b.d, c.d];
        let yz = cross(ny, nz);
        let xz = cross(nx, nz);
        let xy = cross(nx, ny);
        // w = det[NX, NY, NZ] = NX · (NY × NZ): three products below 2^55 · 2^111.
        let mut w = Int::<3>::ZERO;
        for k in 0..3 {
            w = w.add(&Int::product_i128(nx[k], yz[k]));
        }
        if w.is_zero() {
            return None;
        }
        // Cramer: x = det[−D, NY, NZ] = −D·(NY × NZ); y = det[NX, −D, NZ] = D·(NX × NZ);
        // z = det[NX, NY, −D] = −D·(NX × NY).
        let mut point = Self {
            x: dot_offsets(offsets, yz).neg(),
            y: dot_offsets(offsets, xz),
            z: dot_offsets(offsets, xy).neg(),
            w,
        };
        if w.sign() == Ordering::Less {
            point = Self {
                x: point.x.neg(),
                y: point.y.neg(),
                z: point.z.neg(),
                w: point.w.neg(),
            };
        }
        Some(point)
    }

    /// Grid point `p` as an exact point (`w = 1`).
    pub(crate) fn grid(p: GridPoint) -> Self {
        let c = |v: i64| Int::from_i128(i128::from(v));
        Self {
            x: c(p.x),
            y: c(p.y),
            z: c(p.z),
            w: Int::from_i128(1),
        }
    }

    /// The homogeneous coordinates `(x, y, z)` and `w > 0`.
    pub(crate) fn homogeneous(&self) -> ([Int<4>; 3], Int<3>) {
        ([self.x, self.y, self.z], self.w)
    }

    /// Whether the two are the same point.
    pub(crate) fn same(&self, other: &Self) -> bool {
        [
            (&self.x, &other.x),
            (&self.y, &other.y),
            (&self.z, &other.z),
        ]
        .into_iter()
        .all(|(a, b)| {
            let left: Int<6> = Int::product(a, &other.w);
            let right: Int<6> = Int::product(b, &self.w);
            left == right
        })
    }

    /// The order of the point's coordinate on `axis` and grid coordinate `value`: `Less` when the
    /// point lies below `value`.
    pub(crate) fn compare_axis(&self, axis: usize, value: i64) -> Ordering {
        let coordinate = [&self.x, &self.y, &self.z][axis];
        // value · w < 2^26 · 2^168, within 4 limbs; w > 0 keeps the order.
        let scaled: Int<4> = Int::product(&Int::<1>::from_i128(i128::from(value)), &self.w);
        coordinate.compare(&scaled)
    }

    /// The order of the point's coordinate on `axis` and `half / 2`.
    pub(crate) fn compare_half(&self, axis: usize, half: i64) -> Ordering {
        let coordinate = [&self.x, &self.y, &self.z][axis];
        // 2·x against half · w: below 2^197 and 2^28 · 2^168, within 4 limbs.
        let doubled = coordinate.add(coordinate);
        let scaled: Int<4> = Int::product(&Int::<1>::from_i128(i128::from(half)), &self.w);
        doubled.compare(&scaled)
    }

    /// The nearest grid point, ties to even on each axis, exactly: the vertex's place in a
    /// mesh of `f64` grid coordinates.
    pub(crate) fn rounded(&self) -> GridPoint {
        let approx = self.approximate().coordinates();
        let mut c = [0_i64; 3];
        for (axis, value) in c.iter_mut().enumerate() {
            #[allow(clippy::cast_possible_truncation)] // Within ±2^26.
            let mut floor = (approx[axis] * super::grid::STEPS_PER_MM).floor() as i64;
            while self.compare_axis(axis, floor) == Ordering::Less {
                floor -= 1;
            }
            while self.compare_axis(axis, floor + 1) != Ordering::Less {
                floor += 1;
            }
            *value = match self.compare_half(axis, 2 * floor + 1) {
                Ordering::Less => floor,
                Ordering::Equal if floor % 2 == 0 => floor,
                _ => floor + 1,
            };
        }
        GridPoint {
            x: c[0],
            y: c[1],
            z: c[2],
        }
    }

    /// The grid point it is, when it was made as one (`w = 1`): cheap, and `None` decides
    /// nothing.
    pub(crate) fn as_grid(&self) -> Option<GridPoint> {
        if self.w != Int::from_i128(1) {
            return None;
        }
        Some(GridPoint {
            x: self.x.to_i64()?,
            y: self.y.to_i64()?,
            z: self.z.to_i64()?,
        })
    }

    /// Whether the point is a grid point.
    pub(crate) fn on_grid(&self) -> bool {
        let g = self.rounded();
        (0..3).all(|axis| self.compare_axis(axis, g.coordinates()[axis]) == Ordering::Equal)
    }

    /// The order of two points' coordinates on `axis`.
    pub(crate) fn compare_on(&self, other: &Self, axis: usize) -> Ordering {
        let (a, b) = (
            [&self.x, &self.y, &self.z][axis],
            [&other.x, &other.y, &other.z][axis],
        );
        let left: Int<6> = Int::product(a, &other.w);
        let right: Int<6> = Int::product(b, &self.w);
        left.compare(&right)
    }

    /// The point in millimetres, each coordinate the nearest `f64` to within a few units in the
    /// last place: for display and diagnostics only, never for a decision.
    pub(crate) fn approximate(&self) -> crate::geometry::Point3 {
        let w = self.w.approximate();
        let mm = |c: &Int<4>| c.approximate() / w / super::grid::STEPS_PER_MM;
        crate::geometry::Point3::new(mm(&self.x), mm(&self.y), mm(&self.z))
    }
}

#[cfg(test)]
mod tests {
    use super::{Plane, Point};
    use crate::boolean::{
        grid::{GridPoint, MAX_ABS_STEPS},
        int::tests::Rng,
    };
    use crate::preparation::hollow::numeric::orient3d;
    use std::cmp::Ordering;

    fn grid(rng: &mut Rng, bits: u32) -> GridPoint {
        let mut c = || {
            let v = i64::try_from(rng.signed(bits)).unwrap();
            v.clamp(-MAX_ABS_STEPS, MAX_ABS_STEPS)
        };
        GridPoint {
            x: c(),
            y: c(),
            z: c(),
        }
    }

    /// The farthest grid points: the corners of the admitted cube.
    fn corner(rng: &mut Rng) -> GridPoint {
        let mut c = || {
            if rng.next() & 1 == 0 {
                MAX_ABS_STEPS
            } else {
                -MAX_ABS_STEPS
            }
        };
        GridPoint {
            x: c(),
            y: c(),
            z: c(),
        }
    }

    #[test]
    fn a_plane_holds_its_points_and_its_sides_are_orient3d_s() {
        let mut rng = Rng::new(3);
        for round in 0..20_000 {
            let bits = if round % 2 == 0 { 26 } else { 8 };
            let (a, b, c, p) = (
                grid(&mut rng, bits),
                grid(&mut rng, bits),
                grid(&mut rng, bits),
                grid(&mut rng, bits),
            );
            let expected = orient3d(a.point(), b.point(), c.point(), p.point());
            match Plane::through(a, b, c) {
                Some(plane) => {
                    for q in [a, b, c] {
                        assert_eq!(plane.side_of_grid(q), Ordering::Equal);
                        assert_eq!(plane.side(&Point::grid(q)), Ordering::Equal);
                    }
                    assert_eq!(plane.side_of_grid(p), expected);
                    assert_eq!(plane.side(&Point::grid(p)), expected);
                    assert_eq!(plane.flipped().side_of_grid(p), expected.reverse());
                }
                None => assert_eq!(expected, Ordering::Equal),
            }
        }
        let a = GridPoint { x: 0, y: 0, z: 0 };
        let b = GridPoint { x: 1, y: 1, z: 1 };
        let c = GridPoint { x: 5, y: 5, z: 5 };
        assert_eq!(Plane::through(a, b, c), None);
        assert_eq!(Plane::extrusion(a, GridPoint { x: 0, y: 0, z: 9 }, 2), None);
    }

    #[test]
    fn three_planes_meet_exactly_on_all_three() {
        let mut rng = Rng::new(5);
        let mut met = 0;
        for round in 0..20_000 {
            let bits = [26, 20, 6][round % 3];
            let planes: Vec<Plane> = (0..3)
                .filter_map(|_| {
                    let (a, b, c) = (
                        grid(&mut rng, bits),
                        grid(&mut rng, bits),
                        grid(&mut rng, bits),
                    );
                    if round % 5 == 0 {
                        Plane::extrusion(a, b, (rng.next() % 3) as usize)
                    } else {
                        Plane::through(a, b, c)
                    }
                })
                .collect();
            let [p, q, r] = planes[..] else { continue };
            let Some(point) = Point::meet(&p, &q, &r) else {
                continue;
            };
            met += 1;
            for plane in [p, q, r] {
                assert_eq!(plane.side(&point), Ordering::Equal);
            }
            // The same point from the planes in another order.
            assert!(point.same(&Point::meet(&r, &p, &q).unwrap()));
            assert!(point.same(&Point::meet(&q, &p, &r).unwrap()));
        }
        assert!(met > 15_000, "{met}");
        // Parallel planes meet nowhere.
        let a = GridPoint { x: 0, y: 0, z: 0 };
        let floor = Plane::through(
            a,
            GridPoint { x: 1, y: 0, z: 0 },
            GridPoint { x: 0, y: 1, z: 0 },
        )
        .unwrap();
        let lifted = Plane::through(
            GridPoint { x: 0, y: 0, z: 7 },
            GridPoint { x: 1, y: 0, z: 7 },
            GridPoint { x: 0, y: 1, z: 7 },
        )
        .unwrap();
        let wall = Plane::through(
            a,
            GridPoint { x: 0, y: 0, z: 1 },
            GridPoint { x: 0, y: 1, z: 0 },
        )
        .unwrap();
        assert!(floor.parallel(&lifted));
        assert!(!floor.parallel(&wall));
        assert!(Point::meet(&floor, &lifted, &wall).is_none());
    }

    #[test]
    fn a_vertex_made_from_its_triangles_planes_is_that_grid_point() {
        // Three planes through a common grid point v meet at v, whatever else they pass through.
        let mut rng = Rng::new(9);
        for _ in 0..5_000 {
            let v = grid(&mut rng, 26);
            let planes: Vec<Plane> = (0..3)
                .filter_map(|_| Plane::through(v, grid(&mut rng, 26), grid(&mut rng, 26)))
                .collect();
            let [p, q, r] = planes[..] else { continue };
            let Some(point) = Point::meet(&p, &q, &r) else {
                continue;
            };
            assert!(point.same(&Point::grid(v)));
            for axis in 0..3 {
                let value = v.coordinates()[axis];
                assert_eq!(point.compare_axis(axis, value), Ordering::Equal);
                assert_eq!(point.compare_axis(axis, value + 1), Ordering::Less);
                assert_eq!(point.compare_axis(axis, value - 1), Ordering::Greater);
            }
            let approx = point.approximate();
            let exact = v.point();
            assert!((approx.x - exact.x).abs() <= 1e-9 * exact.x.abs().max(1.0));
        }
    }

    #[test]
    #[ignore = "a measurement: cargo test --release -p karasu-core boolean::plane -- --ignored --nocapture"]
    fn the_predicates_cost() {
        let mut rng = Rng::new(17);
        let planes: Vec<Plane> = (0..3_000)
            .filter_map(|_| {
                Plane::through(grid(&mut rng, 26), grid(&mut rng, 26), grid(&mut rng, 26))
            })
            .collect();
        let started = std::time::Instant::now();
        let points: Vec<Point> = planes
            .as_chunks::<3>()
            .0
            .iter()
            .cycle()
            .take(1_000_000)
            .filter_map(|t| Point::meet(&t[0], &t[1], &t[2]))
            .collect();
        let meet = started.elapsed();
        let started = std::time::Instant::now();
        let mut positive = 0_usize;
        for (k, point) in points.iter().enumerate() {
            positive += usize::from(planes[k % planes.len()].side(point) == Ordering::Greater);
        }
        let side = started.elapsed();
        let started = std::time::Instant::now();
        let mut equal = 0_usize;
        for pair in points.windows(2) {
            equal += usize::from(pair[0].same(&pair[1]));
        }
        let same = started.elapsed();
        #[allow(clippy::cast_precision_loss)] // A count of a million at most.
        let per = |d: std::time::Duration, n: usize| d.as_secs_f64() * 1e9 / n as f64;
        println!(
            "meet {:.0} ns, side {:.0} ns, same {:.0} ns ({} points, {positive} positive, {equal} equal)",
            per(meet, 1_000_000),
            per(side, points.len()),
            per(same, points.len() - 1),
            points.len()
        );
    }

    #[test]
    fn a_point_rounds_to_its_nearest_grid_point_ties_to_even() {
        // x = 1, y = 2, z = 0 crossed by planes through halves and thirds: a point (7/2, 5/3,
        // 9/2) made from planes 2x = 7, 3y = 5, 2z = 9.
        let plane = |n: [i64; 3], d: i128| Plane { n, d };
        let p = Point::meet(
            &plane([2, 0, 0], -7),
            &plane([0, 3, 0], -5),
            &plane([0, 0, 2], -9),
        )
        .unwrap();
        // 3.5 → 4 (even), 1.67 → 2, 4.5 → 4 (even).
        assert_eq!(p.rounded().coordinates(), [4, 2, 4]);
        assert!(!p.on_grid());
        let q = Point::meet(
            &plane([2, 0, 0], 5),
            &plane([0, 3, 0], 7),
            &plane([0, 0, 1], 3),
        )
        .unwrap();
        // -2.5 → -2, -2.33 → -2, -3 exactly.
        assert_eq!(q.rounded().coordinates(), [-2, -2, -3]);
        let mut rng = Rng::new(19);
        for _ in 0..5_000 {
            let g = grid(&mut rng, 26);
            assert_eq!(Point::grid(g).rounded(), g);
            assert!(Point::grid(g).on_grid());
        }
    }

    #[test]
    fn the_bounds_hold_at_the_admitted_cube_s_corners() {
        // The largest normals, offsets and determinants come from points at the corners of the
        // admitted cube (±MAX_ABS_STEPS): nothing overflows, and the results stay exact.
        let mut rng = Rng::new(13);
        let mut seen = [0_u32; 2];
        for _ in 0..20_000 {
            let planes: Vec<Plane> = (0..3)
                .filter_map(|_| {
                    Plane::through(corner(&mut rng), corner(&mut rng), grid(&mut rng, 26))
                })
                .collect();
            let [p, q, r] = planes[..] else { continue };
            for plane in [p, q, r] {
                assert!(plane.n.iter().all(|c| c.unsigned_abs() < 1 << 55));
                assert!(plane.d.unsigned_abs() < 1 << 83);
            }
            let Some(point) = Point::meet(&p, &q, &r) else {
                continue;
            };
            seen[0] = seen[0].max(point.w.bits());
            seen[1] = seen[1].max(point.x.bits().max(point.y.bits()).max(point.z.bits()));
            assert!(point.w.bits() < 168 && seen[1] < 196);
            for plane in [p, q, r] {
                assert_eq!(plane.side(&point), Ordering::Equal);
            }
            let other = corner(&mut rng);
            let _ = p.side(&Point::grid(other));
            let _ = point.compare_on(&Point::grid(other), 0);
        }
        // The extremes are reached closely, so the bounds are tight, not loose guesses.
        assert!(seen[0] > 150 && seen[1] > 175, "{seen:?}");
    }
}
