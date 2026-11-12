//! Bounded exact arithmetic for intersections of original integer segments.
//!
//! Eight 64-bit limbs cover the documented <496-bit maximum geometric expression (TD-031).
//! No rational area summation or iterative construction is performed here.

use std::cmp::Ordering;

/// Signed magnitude arithmetic with a checked, fixed 512-bit magnitude.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Int {
    negative: bool,
    words: [u64; 8],
}

impl Int {
    pub(super) const fn zero() -> Self {
        Self {
            negative: false,
            words: [0; 8],
        }
    }

    pub(super) fn from_u64(value: u64) -> Self {
        let mut result = Self::zero();
        result.words[0] = value;
        result
    }

    pub(super) fn from_i64(value: i64) -> Self {
        Self::from_i128(i128::from(value))
    }

    #[allow(clippy::cast_possible_truncation)] // Deliberately extract the low and high limbs.
    pub(super) fn from_i128(value: i128) -> Self {
        let magnitude = value.unsigned_abs();
        let mut result = Self::zero();
        result.words[0] = magnitude as u64;
        result.words[1] = (magnitude >> 64) as u64;
        result.negative = value < 0;
        result
    }

    pub(super) const fn words(self) -> [u64; 8] {
        self.words
    }
    pub(super) const fn is_negative(self) -> bool {
        self.negative
    }
    pub(super) fn is_zero(self) -> bool {
        self.words == [0; 8]
    }

    pub(super) fn sign(self) -> Ordering {
        if self.is_zero() {
            Ordering::Equal
        } else if self.negative {
            Ordering::Less
        } else {
            Ordering::Greater
        }
    }

    pub(super) fn neg(mut self) -> Self {
        if !self.is_zero() {
            self.negative = !self.negative;
        }
        self
    }

    fn cmp_magnitude(&self, other: &Self) -> Ordering {
        self.words.iter().rev().cmp(other.words.iter().rev())
    }

    pub(super) fn add(self, other: Self) -> Result<Self, ()> {
        if self.negative == other.negative {
            let mut result = Self::zero();
            result.negative = self.negative;
            let mut carry = false;
            for (index, word) in result.words.iter_mut().enumerate() {
                let (sum, first) = self.words[index].overflowing_add(other.words[index]);
                let (sum, second) = sum.overflowing_add(u64::from(carry));
                *word = sum;
                carry = first || second;
            }
            if carry {
                return Err(());
            }
            Ok(result)
        } else {
            let order = self.cmp_magnitude(&other);
            if order == Ordering::Equal {
                return Ok(Self::zero());
            }
            let (large, small) = if order == Ordering::Greater {
                (self, other)
            } else {
                (other, self)
            };
            let mut result = Self::zero();
            result.negative = large.negative;
            let mut borrow = false;
            for (index, word) in result.words.iter_mut().enumerate() {
                let (difference, first) = large.words[index].overflowing_sub(small.words[index]);
                let (difference, second) = difference.overflowing_sub(u64::from(borrow));
                *word = difference;
                borrow = first || second;
            }
            Ok(result)
        }
    }

    pub(super) fn sub(self, other: Self) -> Result<Self, ()> {
        self.add(other.neg())
    }

    #[allow(clippy::cast_possible_truncation)] // Carry preserves the deliberately extracted low limb.
    pub(super) fn mul(self, other: Self) -> Result<Self, ()> {
        let mut result = Self::zero();
        for i in 0..8 {
            let mut carry = 0_u128;
            for j in 0..(8 - i) {
                // One product, one existing limb and one carry fit exactly in u128.
                let value = u128::from(self.words[i]) * u128::from(other.words[j])
                    + u128::from(result.words[i + j])
                    + carry;
                result.words[i + j] = value as u64;
                carry = value >> 64;
            }
            if carry != 0
                || (self.words[i] != 0 && other.words[(8 - i)..].iter().any(|word| *word != 0))
            {
                return Err(());
            }
        }
        result.negative = (self.negative != other.negative) && !result.is_zero();
        Ok(result)
    }
}

impl Ord for Int {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self.negative, other.negative) {
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            (false, false) => self.cmp_magnitude(other),
            (true, true) => self.cmp_magnitude(other).reverse(),
        }
    }
}
impl PartialOrd for Int {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// An exact rational grid point with a shared strictly positive denominator.
///
/// Numerators use signed magnitude, least significant limb first. Representations need not
/// be reduced; equality and ordering compare mathematical coordinates. These coordinates
/// remain exact and are never implicitly converted to floating point.
#[derive(Clone, Copy, Debug)]
pub struct ExactPoint {
    x: Int,
    y: Int,
    w: Int,
}

impl ExactPoint {
    pub(super) fn integer(point: [i64; 2]) -> Self {
        Self {
            x: Int::from_i64(point[0]),
            y: Int::from_i64(point[1]),
            w: Int::from_u64(1),
        }
    }

    /// Sign and eight little-endian limbs of the exact X numerator.
    #[must_use]
    pub const fn numerator_x(self) -> (bool, [u64; 8]) {
        (self.x.is_negative(), self.x.words())
    }
    /// Sign and eight little-endian limbs of the exact Y numerator.
    #[must_use]
    pub const fn numerator_y(self) -> (bool, [u64; 8]) {
        (self.y.is_negative(), self.y.words())
    }
    /// Eight little-endian limbs of the strictly positive shared denominator.
    #[must_use]
    pub const fn denominator(self) -> [u64; 8] {
        self.w.words()
    }
}

impl Ord for ExactPoint {
    fn cmp(&self, other: &Self) -> Ordering {
        // Only integer points and intersections of original i64 segments construct this type.
        // Even the full i64 domain requires fewer than 330 bits for these products. Private
        // construction and boundary tests justify treating overflow as an internal invariant.
        let compare = |a: Int, b: Int| {
            a.mul(other.w)
                .expect("original-segment point comparison fits 512 bits")
                .cmp(
                    &b.mul(self.w)
                        .expect("original-segment point comparison fits 512 bits"),
                )
        };
        compare(self.x, other.x).then_with(|| compare(self.y, other.y))
    }
}
impl PartialOrd for ExactPoint {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl PartialEq for ExactPoint {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}
impl Eq for ExactPoint {}

fn cross(a: [Int; 2], b: [Int; 2]) -> Result<Int, ()> {
    a[0].mul(b[1])?.sub(a[1].mul(b[0])?)
}

fn delta(a: [i64; 2], b: [i64; 2]) -> [Int; 2] {
    [
        Int::from_i128(i128::from(b[0]) - i128::from(a[0])),
        Int::from_i128(i128::from(b[1]) - i128::from(a[1])),
    ]
}

/// Intersection of two nonparallel original segments, including their endpoints.
pub(super) fn intersection(
    a: [i64; 2],
    b: [i64; 2],
    c: [i64; 2],
    d: [i64; 2],
) -> Result<Option<ExactPoint>, ()> {
    let first_direction = delta(a, b);
    let second_direction = delta(c, d);
    let offset = delta(a, c);
    let mut denominator = cross(first_direction, second_direction)?;
    if denominator.is_zero() {
        return Ok(None);
    }
    let mut first_parameter = cross(offset, second_direction)?;
    let mut second_parameter = cross(offset, first_direction)?;
    if denominator.sign() == Ordering::Less {
        denominator = denominator.neg();
        first_parameter = first_parameter.neg();
        second_parameter = second_parameter.neg();
    }
    if first_parameter.sign() == Ordering::Less
        || second_parameter.sign() == Ordering::Less
        || first_parameter > denominator
        || second_parameter > denominator
    {
        return Ok(None);
    }
    Ok(Some(ExactPoint {
        x: Int::from_i64(a[0])
            .mul(denominator)?
            .add(first_direction[0].mul(first_parameter)?)?,
        y: Int::from_i64(a[1])
            .mul(denominator)?
            .add(first_direction[1].mul(first_parameter)?)?,
        w: denominator,
    }))
}

/// Midpoint plus a positive infinitesimal displacement in an explicit integer direction.
#[derive(Clone, Copy, Debug)]
pub(super) struct SymbolicPoint {
    x: Int,
    y: Int,
    w: Int,
    direction: [Int; 2],
}

fn leading_sign(constant: Int, infinitesimal: Int) -> Ordering {
    let sign = constant.sign();
    if sign == Ordering::Equal {
        infinitesimal.sign()
    } else {
        sign
    }
}

impl SymbolicPoint {
    /// The displacement is normally a left or right normal of an original supporting segment.
    pub(super) fn midpoint_normal(
        a: &ExactPoint,
        b: &ExactPoint,
        direction: [i64; 2],
    ) -> Result<Self, ()> {
        Ok(Self {
            x: a.x.mul(b.w)?.add(b.x.mul(a.w)?)?,
            y: a.y.mul(b.w)?.add(b.y.mul(a.w)?)?,
            w: a.w.mul(b.w)?.mul(Int::from_u64(2))?,
            direction: [Int::from_i64(direction[0]), Int::from_i64(direction[1])],
        })
    }

    /// Compare the symbolic query Y with an exact point Y using the infinitesimal tie break.
    pub(super) fn compare_y(&self, point: &ExactPoint) -> Result<Ordering, ()> {
        Ok(leading_sign(
            self.y.mul(point.w)?.sub(point.y.mul(self.w)?)?,
            self.direction[1],
        ))
    }

    /// Signed side of an original integer supporting line, oriented from a to b.
    pub(super) fn side_of_original_line(&self, a: [i64; 2], b: [i64; 2]) -> Result<Ordering, ()> {
        let vector = delta(a, b);
        let offset = [
            self.x.sub(Int::from_i64(a[0]).mul(self.w)?)?,
            self.y.sub(Int::from_i64(a[1]).mul(self.w)?)?,
        ];
        Ok(leading_sign(
            cross(vector, offset)?,
            cross(vector, self.direction)?,
        ))
    }

    /// Signed side of a line through an exact vertex with an original integer direction.
    pub(super) fn side_of_supported_line(
        &self,
        vertex: &ExactPoint,
        direction: [i64; 2],
    ) -> Result<Ordering, ()> {
        let vector = [Int::from_i64(direction[0]), Int::from_i64(direction[1])];
        let offset = [
            self.x.mul(vertex.w)?.sub(vertex.x.mul(self.w)?)?,
            self.y.mul(vertex.w)?.sub(vertex.y.mul(self.w)?)?,
        ];
        Ok(leading_sign(
            cross(vector, offset)?,
            cross(vector, self.direction)?,
        ))
    }
}

/// Exact orientation within the reviewed |coordinate| <= 2^60 internal input domain.
pub(super) fn orient_integer(a: [i64; 2], b: [i64; 2], c: [i64; 2]) -> i128 {
    let ab = [
        i128::from(b[0]) - i128::from(a[0]),
        i128::from(b[1]) - i128::from(a[1]),
    ];
    let ac = [
        i128::from(c[0]) - i128::from(a[0]),
        i128::from(c[1]) - i128::from(a[1]),
    ];
    ab[0] * ac[1] - ab[1] * ac[0]
}

/// Exact closed-segment membership within the reviewed integer input domain.
pub(super) fn on_segment_integer(point: [i64; 2], a: [i64; 2], b: [i64; 2]) -> bool {
    orient_integer(a, b, point) == 0
        && (a[0].min(b[0])..=a[0].max(b[0])).contains(&point[0])
        && (a[1].min(b[1])..=a[1].max(b[1])).contains(&point[1])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arithmetic_matches_independent_i128_for_small_signed_values() {
        for a in -80_i128..=80 {
            for b in -80_i128..=80 {
                let x = Int::from_i128(a);
                let y = Int::from_i128(b);
                assert_eq!(x.add(y), Ok(Int::from_i128(a + b)));
                assert_eq!(x.sub(y), Ok(Int::from_i128(a - b)));
                assert_eq!(x.mul(y), Ok(Int::from_i128(a * b)));
                assert_eq!(x.cmp(&y), a.cmp(&b));
            }
        }
    }

    #[test]
    fn limb_carry_borrow_and_checked_overflow() {
        let max = Int {
            negative: false,
            words: [u64::MAX; 8],
        };
        assert_eq!(max.add(Int::from_u64(1)), Err(()));
        assert_eq!(max.mul(Int::from_u64(2)), Err(()));
        assert_eq!(max.mul(max), Err(()));
        assert_eq!(max.sub(max), Ok(Int::zero()));
        let one = Int::from_u64(1);
        let mut high = Int::zero();
        high.words[7] = 1;
        let difference = high.sub(one).unwrap();
        assert_eq!(
            difference.words,
            [
                u64::MAX,
                u64::MAX,
                u64::MAX,
                u64::MAX,
                u64::MAX,
                u64::MAX,
                u64::MAX,
                0
            ]
        );
        assert_eq!(difference.add(one), Ok(high));
        assert_eq!(Int::from_i128(i128::MIN).neg().words[1], 1_u64 << 63);
        assert_eq!(Int::zero().neg().sign(), Ordering::Equal);
        // Carry across every word, with a known representable closed-form product.
        let mut low = Int::zero();
        low.words[..4].fill(u64::MAX);
        let square = low.mul(low).unwrap();
        assert_eq!(
            square.words,
            [1, 0, 0, 0, u64::MAX - 1, u64::MAX, u64::MAX, u64::MAX]
        );
    }

    #[test]
    fn small_intersections_match_independent_rational_oracle() {
        for x in -4_i64..=4 {
            for y in -4_i64..=4 {
                if y == 0 {
                    continue;
                }
                let a = [0, 0];
                let b = [6, 0];
                let c = [x, -y];
                let d = [x + 3, y];
                let result = intersection(a, b, c, d).unwrap();
                // At y=0 the second segment is exactly its midpoint.
                if (0..=12).contains(&(2 * x + 3)) {
                    let point = result.unwrap();
                    assert_eq!(
                        point.x.mul(Int::from_u64(2)),
                        Int::from_i64(2 * x + 3).mul(point.w)
                    );
                    assert_eq!(point.y, Int::zero());
                    assert_eq!(intersection(d, c, b, a).unwrap(), result);
                } else {
                    assert_eq!(result, None);
                }
            }
        }
        assert_eq!(intersection([0, 0], [1, 0], [2, 0], [3, 0]), Ok(None));
        assert_eq!(
            intersection([0, 0], [1, 0], [1, 0], [1, 3]).unwrap(),
            Some(ExactPoint::integer([1, 0]))
        );
    }

    #[test]
    fn domain_limit_counterexample_and_symbolic_sides_remain_exact() {
        let b = 1_i64 << 52;
        let point = intersection([0, 0], [b, b - 1], [b, 0], [0, b])
            .unwrap()
            .unwrap();
        assert!(point.numerator_x().1[2] != 0, "construction exceeds i128");
        let origin = ExactPoint::integer([0, 0]);
        let query = SymbolicPoint::midpoint_normal(&origin, &point, [-(b - 1), b]).unwrap();
        assert_eq!(
            query.side_of_original_line([0, 0], [b, b - 1]),
            Ok(Ordering::Greater)
        );
        assert_eq!(
            query.side_of_supported_line(&point, [b, b - 1]),
            Ok(Ordering::Greater)
        );
        assert_eq!(query.compare_y(&point), Ok(Ordering::Less));
        let other = intersection([-b, -b], [b, b], [-b, b], [b, -b])
            .unwrap()
            .unwrap();
        assert_eq!(other, origin);
        assert!(point > origin);
        let symbolic = SymbolicPoint::midpoint_normal(&point, &other, [b, -b]).unwrap();
        symbolic
            .side_of_supported_line(&point, [-2 * b, b])
            .unwrap();
        symbolic.compare_y(&point).unwrap();
    }

    #[test]
    fn mathematical_point_order_ignores_denominator_representation() {
        let integer = ExactPoint::integer([1, -2]);
        let scaled = ExactPoint {
            x: Int::from_i64(3),
            y: Int::from_i64(-6),
            w: Int::from_u64(3),
        };
        assert_eq!(integer, scaled);
        assert_eq!(integer.cmp(&scaled), Ordering::Equal);
        let query = SymbolicPoint::midpoint_normal(&integer, &scaled, [0, -1]).unwrap();
        assert_eq!(query.compare_y(&integer), Ok(Ordering::Less));
        assert_eq!(
            query.side_of_original_line([0, -2], [2, -2]),
            Ok(Ordering::Less)
        );
        assert!(on_segment_integer([1, 0], [0, 0], [2, 0]));
        assert!(!on_segment_integer([3, 0], [0, 0], [2, 0]));
    }

    #[test]
    fn every_single_bit_product_obeys_the_capacity_boundary() {
        for a in 0..512 {
            let mut left = Int::zero();
            left.words[a / 64] = 1_u64 << (a % 64);
            for b in 0..512 {
                let mut right = Int::zero();
                right.words[b / 64] = 1_u64 << (b % 64);
                if a + b >= 512 {
                    assert_eq!(left.mul(right), Err(()));
                } else {
                    let mut expected = Int::zero();
                    expected.words[(a + b) / 64] = 1_u64 << ((a + b) % 64);
                    assert_eq!(left.mul(right), Ok(expected));
                    assert_eq!(left.neg().mul(right), Ok(expected.neg()));
                }
            }
        }
    }

    #[test]
    fn exhaustive_small_segment_pairs_match_implicit_line_oracle() {
        let points: Vec<_> = (-1_i64..=1)
            .flat_map(|x| (-1_i64..=1).map(move |y| [x, y]))
            .collect();
        for &a in &points {
            for &b in &points {
                for &c in &points {
                    for &d in &points {
                        let coefficient = |p: [i64; 2], q: [i64; 2]| {
                            (
                                i128::from(p[1] - q[1]),
                                i128::from(q[0] - p[0]),
                                i128::from(p[0]) * i128::from(q[1])
                                    - i128::from(q[0]) * i128::from(p[1]),
                            )
                        };
                        let (ax, ay, az) = coefficient(a, b);
                        let (bx, by, bz) = coefficient(c, d);
                        let mut w = ax * by - ay * bx;
                        let result = intersection(a, b, c, d).unwrap();
                        if w == 0 {
                            assert_eq!(result, None);
                            continue;
                        }
                        let mut x = ay * bz - az * by;
                        let mut y = az * bx - ax * bz;
                        if w < 0 {
                            w = -w;
                            x = -x;
                            y = -y;
                        }
                        let inside = |p: [i64; 2], q: [i64; 2]| {
                            i128::from(p[0].min(q[0])) * w <= x
                                && x <= i128::from(p[0].max(q[0])) * w
                                && i128::from(p[1].min(q[1])) * w <= y
                                && y <= i128::from(p[1].max(q[1])) * w
                        };
                        if inside(a, b) && inside(c, d) {
                            let actual = result.unwrap();
                            assert_eq!(
                                actual.x.mul(Int::from_i128(w)),
                                actual.w.mul(Int::from_i128(x))
                            );
                            assert_eq!(
                                actual.y.mul(Int::from_i128(w)),
                                actual.w.mul(Int::from_i128(y))
                            );
                        } else {
                            assert_eq!(result, None);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn original_i64_extremes_still_preserve_point_order_invariant() {
        let min = i64::MIN;
        let max = i64::MAX;
        let point = intersection([min, min], [max, max - 1], [min, max], [max, min])
            .unwrap()
            .unwrap();
        assert!(point > ExactPoint::integer([min, min]));
        assert!(point < ExactPoint::integer([max, max]));
        assert_eq!(
            point,
            intersection([max, min], [min, max], [max, max - 1], [min, min])
                .unwrap()
                .unwrap()
        );
    }
}
