//! Private outward binary64 intervals and exact signs for represented finite 3D points.
//!
//! Basic arithmetic uses one outward representable step after each correctly rounded IEEE
//! operation. Square root candidates are accepted only after exact integer squares prove their
//! bounds, including underflowing squares; no square-root accuracy assumption is used for acceptance.

use crate::geometry::Point3;
use std::cmp::Ordering;

#[derive(Clone, Copy, Debug)]
pub(crate) struct I {
    pub(crate) lo: f64,
    pub(crate) hi: f64,
}

impl I {
    pub(crate) fn point(value: f64) -> Self {
        Self {
            lo: value,
            hi: value,
        }
    }

    fn broad() -> Self {
        Self {
            lo: f64::NEG_INFINITY,
            hi: f64::INFINITY,
        }
    }

    fn outward(lo: f64, hi: f64) -> Self {
        if lo.is_nan() || hi.is_nan() {
            Self::broad()
        } else {
            Self {
                lo: lo.next_down(),
                hi: hi.next_up(),
            }
        }
    }

    pub(crate) fn is_finite(self) -> bool {
        self.lo.is_finite() && self.hi.is_finite() && self.lo <= self.hi
    }

    pub(crate) fn add(self, other: Self) -> Self {
        Self::outward(self.lo + other.lo, self.hi + other.hi)
    }

    pub(crate) fn sub(self, other: Self) -> Self {
        Self::outward(self.lo - other.hi, self.hi - other.lo)
    }

    pub(crate) fn mul(self, other: Self) -> Self {
        let products = [
            self.lo * other.lo,
            self.lo * other.hi,
            self.hi * other.lo,
            self.hi * other.hi,
        ];
        if products.iter().any(|value| value.is_nan()) {
            return Self::broad();
        }
        Self::outward(
            products.into_iter().fold(f64::INFINITY, f64::min),
            products.into_iter().fold(f64::NEG_INFINITY, f64::max),
        )
    }

    pub(crate) fn div(self, other: Self) -> Option<Self> {
        if other.lo <= 0.0 && other.hi >= 0.0 || other.lo.is_nan() || other.hi.is_nan() {
            return None;
        }
        let quotients = [
            self.lo / other.lo,
            self.lo / other.hi,
            self.hi / other.lo,
            self.hi / other.hi,
        ];
        if quotients.iter().any(|value| value.is_nan()) {
            return Some(Self::broad());
        }
        Some(Self::outward(
            quotients.into_iter().fold(f64::INFINITY, f64::min),
            quotients.into_iter().fold(f64::NEG_INFINITY, f64::max),
        ))
    }

    pub(crate) fn sqrt(self) -> Option<Self> {
        if !self.is_finite() || self.lo < 0.0 {
            return None;
        }
        if self.hi == 0.0 {
            return Some(Self::point(0.0));
        }
        let mut lo = if self.lo == 0.0 {
            0.0
        } else {
            self.lo.sqrt().next_down()
        };
        let mut hi = self.hi.sqrt().next_up();
        // A poor platform candidate causes an explicit failure, never an unproved enclosure.
        for _ in 0..64 {
            let lower_proved = square_cmp(lo, self.lo).is_le();
            let upper_proved = square_cmp(hi, self.hi).is_ge();
            if lower_proved && upper_proved && lo >= 0.0 && hi.is_finite() {
                return Some(Self { lo, hi });
            }
            if !lower_proved {
                lo = lo.next_down().max(0.0);
            }
            if !upper_proved {
                hi = hi.next_up();
            }
        }
        None
    }
}

// Every finite binary64 coordinate is an integer times 2^-1074. A coordinate difference uses at
// most 2099 bits; a triple product uses 6297 and six signed terms need at most 6300. These 6400
// bits therefore hold the complete determinant, with no heap, truncation or floating fallback.
const WORDS: usize = 200;

#[derive(Clone)]
struct Integer {
    negative: bool,
    words: [u32; WORDS],
}

impl Integer {
    fn zero() -> Self {
        Self {
            negative: false,
            words: [0; WORDS],
        }
    }

    fn from_float(value: f64) -> Self {
        debug_assert!(value.is_finite());
        let bits = value.to_bits();
        let exponent = usize::try_from((bits >> 52) & 0x7ff).unwrap();
        let fraction = bits & ((1_u64 << 52) - 1);
        let mantissa = if exponent == 0 {
            fraction
        } else {
            fraction | (1_u64 << 52)
        };
        let shift = exponent.saturating_sub(1);
        let mut result = Self::zero();
        result.negative = value < 0.0;
        for bit in 0..53 {
            if mantissa & (1_u64 << bit) != 0 {
                let index = shift + bit;
                result.words[index / 32] |= 1_u32 << (index % 32);
            }
        }
        result
    }

    fn used(&self) -> usize {
        self.words
            .iter()
            .rposition(|&word| word != 0)
            .map_or(0, |index| index + 1)
    }

    #[cfg(test)]
    fn shift(&self, bits: usize) -> Self {
        let mut result = Self::zero();
        let words = bits / 32;
        let remainder = bits % 32;
        debug_assert!(self.used() + words + usize::from(remainder != 0) <= WORDS);
        for index in 0..self.used() {
            result.words[index + words] |= self.words[index] << remainder;
            if remainder != 0 {
                result.words[index + words + 1] |= self.words[index] >> (32 - remainder);
            }
        }
        result.negative = self.negative;
        result
    }

    fn sign(&self) -> Ordering {
        if self.used() == 0 {
            Ordering::Equal
        } else if self.negative {
            Ordering::Less
        } else {
            Ordering::Greater
        }
    }

    fn subtract(&self, other: &Self) -> Self {
        let mut result = Self::zero();
        if self.negative == other.negative {
            let (large, small, negative) = if self
                .words
                .iter()
                .rev()
                .cmp(other.words.iter().rev())
                .is_lt()
            {
                (other, self, !self.negative)
            } else {
                (self, other, self.negative)
            };
            let mut borrow = false;
            for index in 0..WORDS {
                let (value, first) = large.words[index].overflowing_sub(small.words[index]);
                let (value, second) = value.overflowing_sub(u32::from(borrow));
                result.words[index] = value;
                borrow = first || second;
            }
            debug_assert!(!borrow);
            result.negative = negative;
        } else {
            let mut carry = 0_u64;
            for index in 0..WORDS {
                let sum = u64::from(self.words[index]) + u64::from(other.words[index]) + carry;
                result.words[index] = u32::try_from(sum & 0xffff_ffff).unwrap();
                carry = sum >> 32;
            }
            debug_assert_eq!(carry, 0);
            result.negative = self.negative;
        }
        result
    }

    fn multiply(&self, other: &Self) -> Self {
        let mut result = Self::zero();
        let left = self.used();
        let right = other.used();
        debug_assert!(left + right <= WORDS);
        for i in 0..left {
            let mut carry = 0_u64;
            for j in 0..right {
                let sum = u64::from(self.words[i]) * u64::from(other.words[j])
                    + u64::from(result.words[i + j])
                    + carry;
                result.words[i + j] = u32::try_from(sum & 0xffff_ffff).unwrap();
                carry = sum >> 32;
            }
            result.words[i + right] = u32::try_from(carry).unwrap();
        }
        result.negative = self.negative != other.negative;
        result
    }
}

/// Exact comparison of `candidate²` with `|value|` (an infinite candidate compares greater).
///
/// With `candidate = ±m · 2^e` and `|value| = n · 2^f` (odd `m`, `n`), `m² < 2^106` and `n < 2^53`.
/// Different leading-bit positions decide the order; otherwise aligning on the smaller exponent
/// keeps both operands below 2^106, so one `u128` comparison is exact.
fn square_cmp(candidate: f64, value: f64) -> Ordering {
    if !candidate.is_finite() {
        return Ordering::Greater;
    }
    match (odd_parts(candidate), odd_parts(value)) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Less,
        (Some(_), None) => Ordering::Greater,
        (Some((_, root, root_exponent)), Some((_, target, target_exponent))) => {
            let square = u128::from(root) * u128::from(root);
            let square_exponent = 2 * root_exponent;
            let top_square = square.bit_width().cast_signed() + square_exponent;
            let top_target = target.bit_width().cast_signed() + target_exponent;
            if top_square != top_target {
                return top_square.cmp(&top_target);
            }
            if square_exponent >= target_exponent {
                (square << (square_exponent - target_exponent).cast_unsigned())
                    .cmp(&u128::from(target))
            } else {
                square.cmp(
                    &(u128::from(target) << (target_exponent - square_exponent).cast_unsigned()),
                )
            }
        }
    }
}

/// Reference comparison through the 6400-bit integers.
#[cfg(test)]
fn general_square_cmp(candidate: f64, value: f64) -> Ordering {
    if !candidate.is_finite() {
        return Ordering::Greater;
    }
    let integer = Integer::from_float(candidate);
    let square = integer.multiply(&integer);
    let value = Integer::from_float(value).shift(1074);
    square.words.iter().rev().cmp(value.words.iter().rev())
}

fn interval_vector(a: Point3, b: Point3) -> [I; 3] {
    [
        I::point(b.x).sub(I::point(a.x)),
        I::point(b.y).sub(I::point(a.y)),
        I::point(b.z).sub(I::point(a.z)),
    ]
}

fn integer_vector(a: Point3, b: Point3) -> [Integer; 3] {
    [
        Integer::from_float(b.x).subtract(&Integer::from_float(a.x)),
        Integer::from_float(b.y).subtract(&Integer::from_float(a.y)),
        Integer::from_float(b.z).subtract(&Integer::from_float(a.z)),
    ]
}

/// Sign of `(b-a) cross (c-a) dot (d-a)` for finite represented points.
pub(crate) fn orient3d(a: Point3, b: Point3, c: Point3, d: Point3) -> Ordering {
    let edge_b = interval_vector(a, b);
    let edge_c = interval_vector(a, c);
    let edge_d = interval_vector(a, d);
    let determinant = edge_b[1]
        .mul(edge_c[2])
        .sub(edge_b[2].mul(edge_c[1]))
        .mul(edge_d[0])
        .add(
            edge_b[2]
                .mul(edge_c[0])
                .sub(edge_b[0].mul(edge_c[2]))
                .mul(edge_d[1]),
        )
        .add(
            edge_b[0]
                .mul(edge_c[1])
                .sub(edge_b[1].mul(edge_c[0]))
                .mul(edge_d[2]),
        );
    if determinant.lo > 0.0 {
        return Ordering::Greater;
    }
    if determinant.hi < 0.0 {
        return Ordering::Less;
    }
    exact_orient3d(a, b, c, d)
}

fn exact_orient3d(a: Point3, b: Point3, c: Point3, d: Point3) -> Ordering {
    scaled_orient3d([a, b, c, d]).unwrap_or_else(|| general_orient3d(a, b, c, d))
}

/// Exact sign through the 6400-bit integers; valid for every finite binary64 input.
fn general_orient3d(a: Point3, b: Point3, c: Point3, d: Point3) -> Ordering {
    let edge_b = integer_vector(a, b);
    let edge_c = integer_vector(a, c);
    let edge_d = integer_vector(a, d);
    let cross_x = edge_b[1]
        .multiply(&edge_c[2])
        .subtract(&edge_b[2].multiply(&edge_c[1]))
        .multiply(&edge_d[0]);
    let cross_y = edge_b[2]
        .multiply(&edge_c[0])
        .subtract(&edge_b[0].multiply(&edge_c[2]))
        .multiply(&edge_d[1]);
    let cross_z = edge_b[0]
        .multiply(&edge_c[1])
        .subtract(&edge_b[1].multiply(&edge_c[0]))
        .multiply(&edge_d[2]);
    // Addition through sign inversion keeps one independently tested signed arithmetic routine.
    let mut negative_y = cross_y;
    negative_y.negative = !negative_y.negative;
    let mut negative_z = cross_z;
    negative_z.negative = !negative_z.negative;
    cross_x.subtract(&negative_y).subtract(&negative_z).sign()
}

// Fixed-width exact path. Every nonzero finite coordinate is `±m · 2^e` with an odd `m < 2^53`.
// Dividing all twelve coordinates by `2^e_min`, the smallest such power among them, multiplies the
// determinant by the positive factor `2^(-3 · e_min)`, so its sign is unchanged. When every scaled
// coordinate is an integer below 2^SCALED_BITS, differences stay below 2^121 (`i128`), 2 x 2
// minors below 2^243, triple products below 2^364 and the determinant below 2^366, which six
// 64-bit limbs hold exactly. Coordinates spanning more binary orders of magnitude use the general
// 6400-bit path. Both paths compute the same exact integer sign.
const SCALED_BITS: u32 = 120;
const LIMBS: usize = 6;

#[derive(Clone, Copy)]
struct Wide {
    negative: bool,
    magnitude: [u64; LIMBS],
}

#[allow(clippy::cast_possible_truncation)] // Deliberately keeps the low 64 bits.
const fn low_word(value: u128) -> u64 {
    value as u64
}

impl Wide {
    fn from_i128(value: i128) -> Self {
        let magnitude = value.unsigned_abs();
        let mut limbs = [0; LIMBS];
        limbs[0] = low_word(magnitude);
        limbs[1] = low_word(magnitude >> 64);
        Self {
            negative: value < 0,
            magnitude: limbs,
        }
    }

    fn product(&self, other: &Self) -> Self {
        let mut wide = [0_u64; 2 * LIMBS];
        for (i, &x) in self.magnitude.iter().enumerate() {
            if x == 0 {
                continue;
            }
            let mut carry = 0_u128;
            for (j, &y) in other.magnitude.iter().enumerate() {
                // At most (2^64 - 1)^2 + 2 (2^64 - 1) = 2^128 - 1: no overflow.
                let sum = u128::from(x) * u128::from(y) + u128::from(wide[i + j]) + carry;
                wide[i + j] = low_word(sum);
                carry = sum >> 64;
            }
            wide[i + LIMBS] = low_word(carry);
        }
        debug_assert!(wide[LIMBS..].iter().all(|&limb| limb == 0));
        let mut magnitude = [0; LIMBS];
        magnitude.copy_from_slice(&wide[..LIMBS]);
        Self {
            negative: self.negative != other.negative,
            magnitude,
        }
    }

    fn sum(&self, other: &Self) -> Self {
        let mut magnitude = [0; LIMBS];
        if self.negative == other.negative {
            let mut carry = false;
            for (k, limb) in magnitude.iter_mut().enumerate() {
                let (value, first) = self.magnitude[k].overflowing_add(other.magnitude[k]);
                let (value, second) = value.overflowing_add(u64::from(carry));
                *limb = value;
                carry = first || second;
            }
            debug_assert!(!carry);
            return Self {
                negative: self.negative,
                magnitude,
            };
        }
        let (large, small) = if self
            .magnitude
            .iter()
            .rev()
            .cmp(other.magnitude.iter().rev())
            .is_lt()
        {
            (other, self)
        } else {
            (self, other)
        };
        let mut borrow = false;
        for (k, limb) in magnitude.iter_mut().enumerate() {
            let (value, first) = large.magnitude[k].overflowing_sub(small.magnitude[k]);
            let (value, second) = value.overflowing_sub(u64::from(borrow));
            *limb = value;
            borrow = first || second;
        }
        debug_assert!(!borrow);
        Self {
            negative: large.negative,
            magnitude,
        }
    }

    fn negated(self) -> Self {
        Self {
            negative: !self.negative,
            ..self
        }
    }

    fn sign(&self) -> Ordering {
        if self.magnitude.iter().all(|&limb| limb == 0) {
            Ordering::Equal
        } else if self.negative {
            Ordering::Less
        } else {
            Ordering::Greater
        }
    }
}

/// `(negative, m, e)` with `value = ±m · 2^e` and odd `m`; `None` for either zero.
fn odd_parts(value: f64) -> Option<(bool, u64, i32)> {
    let bits = value.to_bits();
    let biased = i32::try_from((bits >> 52) & 0x7ff).ok()?;
    let fraction = bits & ((1_u64 << 52) - 1);
    let (mantissa, exponent) = if biased == 0 {
        (fraction, -1074)
    } else {
        (fraction | (1_u64 << 52), biased - 1075)
    };
    if mantissa == 0 {
        return None;
    }
    let shift = mantissa.trailing_zeros();
    Some((
        value.is_sign_negative(),
        mantissa >> shift,
        exponent + shift.cast_signed(),
    ))
}

/// Exact orientation sign through common-exponent scaling, or `None` when the coordinates span
/// too many binary orders of magnitude for the fixed width.
fn scaled_orient3d(points: [Point3; 4]) -> Option<Ordering> {
    let parts: [Option<(bool, u64, i32)>; 12] = std::array::from_fn(|index| {
        let point = points[index / 3];
        odd_parts([point.x, point.y, point.z][index % 3])
    });
    let Some(minimum) = parts.iter().flatten().map(|part| part.2).min() else {
        // Every coordinate is zero: four coincident points.
        return Some(Ordering::Equal);
    };
    let mut scaled = [0_i128; 12];
    for (slot, part) in scaled.iter_mut().zip(parts) {
        if let Some((negative, mantissa, exponent)) = part {
            let shift = u32::try_from(exponent - minimum).ok()?;
            if shift + mantissa.bit_width() > SCALED_BITS {
                return None;
            }
            let magnitude = i128::from(mantissa) << shift;
            *slot = if negative { -magnitude } else { magnitude };
        }
    }
    let edge = |from: usize, to: usize| -> [Wide; 3] {
        std::array::from_fn(|axis| Wide::from_i128(scaled[3 * to + axis] - scaled[3 * from + axis]))
    };
    let (edge_b, edge_c, edge_d) = (edge(0, 1), edge(0, 2), edge(0, 3));
    let minor = |i: usize, j: usize| {
        edge_b[i]
            .product(&edge_c[j])
            .sum(&edge_b[j].product(&edge_c[i]).negated())
    };
    let determinant = minor(1, 2)
        .product(&edge_d[0])
        .sum(&minor(2, 0).product(&edge_d[1]))
        .sum(&minor(0, 1).product(&edge_d[2]));
    Some(determinant.sign())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_sign_matches_independent_integer_determinants() {
        let mut state = 503_u64;
        for _ in 0..4096 {
            let coordinates = std::array::from_fn::<_, 12, _>(|index| {
                state = state
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(u64::try_from(index).unwrap() + 1);
                i32::try_from((state >> 32) % 101).unwrap() - 50
            });
            let integers = coordinates.map(i128::from);
            let edge_b = [
                integers[3] - integers[0],
                integers[4] - integers[1],
                integers[5] - integers[2],
            ];
            let edge_c = [
                integers[6] - integers[0],
                integers[7] - integers[1],
                integers[8] - integers[2],
            ];
            let edge_d = [
                integers[9] - integers[0],
                integers[10] - integers[1],
                integers[11] - integers[2],
            ];
            let expected = ((edge_b[1] * edge_c[2] - edge_b[2] * edge_c[1]) * edge_d[0]
                + (edge_b[2] * edge_c[0] - edge_b[0] * edge_c[2]) * edge_d[1]
                + (edge_b[0] * edge_c[1] - edge_b[1] * edge_c[0]) * edge_d[2])
                .cmp(&0);
            let floats = coordinates.map(f64::from);
            let [a, b, c, d] = std::array::from_fn(|i| {
                Point3::new(floats[i * 3], floats[i * 3 + 1], floats[i * 3 + 2])
            });
            assert_eq!(orient3d(a, b, c, d), expected);
            assert_eq!(exact_orient3d(a, b, c, d), expected);
        }
    }

    #[test]
    fn scaled_sign_agrees_with_the_general_integers() {
        let mut state = 0x9e37_79b9_7f4a_7c15_u64;
        let mut next = move || {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            state
        };
        // A random binary64 with a biased exponent in `low..low + span` and a random sign.
        let mut value = |low: u64, span: u64| {
            let bits = next();
            f64::from_bits((bits & (1 << 63)) | ((low + (bits >> 12) % span) << 52) | (bits >> 12))
        };
        let (mut scaled, mut general) = (0, 0);
        for round in 0..30_000 {
            let mut coordinates: [f64; 12] = match round % 4 {
                // Small integers on a dyadic grid: frequent exact zeros of every kind.
                0 => std::array::from_fn(|_| {
                    let grid = value(1023, 3).trunc();
                    grid * [1.0, 0.125, 1024.0, 2.0f64.powi(-40)][round % 16 / 4]
                }),
                // Full mantissas over a few binary orders of magnitude.
                1 => std::array::from_fn(|_| value(1013, 20)),
                // Mixed magnitudes, beyond the fixed width when the span is large.
                2 => std::array::from_fn(|_| value(900, 200)),
                // Coplanar: four points on one horizontal plane, exactly zero.
                _ => std::array::from_fn(|_| value(1000, 40)),
            };
            if round % 4 == 3 {
                for index in [5, 8, 11] {
                    coordinates[index] = coordinates[2];
                }
            }
            let [a, b, c, d] = std::array::from_fn(|i| {
                Point3::new(
                    coordinates[3 * i],
                    coordinates[3 * i + 1],
                    coordinates[3 * i + 2],
                )
            });
            let expected = general_orient3d(a, b, c, d);
            match scaled_orient3d([a, b, c, d]) {
                Some(sign) => {
                    scaled += 1;
                    assert_eq!(sign, expected, "{coordinates:?}");
                }
                None => general += 1,
            }
            if round % 4 == 3 {
                assert_eq!(expected, Ordering::Equal);
            }
        }
        assert!(scaled > 20_000 && general > 1_000, "{scaled} {general}");
    }

    #[test]
    fn exact_sign_handles_extremes_subnormals_and_coplanarity() {
        for scale in [f64::from_bits(1), f64::MIN_POSITIVE, 1.0, f64::MAX] {
            let a = Point3::new(-scale, -scale, -scale);
            let b = Point3::new(scale, -scale, -scale);
            let c = Point3::new(-scale, scale, -scale);
            let d = Point3::new(-scale, -scale, scale);
            assert_eq!(orient3d(a, b, c, d), Ordering::Greater);
            assert_eq!(orient3d(a, c, b, d), Ordering::Less);
            assert_eq!(
                orient3d(a, b, c, Point3::new(scale, scale, -scale)),
                Ordering::Equal
            );
            assert_eq!(orient3d(a, a, c, d), Ordering::Equal);
        }
        let epsilon = f64::EPSILON;
        let a = Point3::new(-0.0, 0.0, 0.0);
        let b = Point3::new(1.0, 1.0 + epsilon, 0.0);
        let c = Point3::new(1.0 - epsilon, 1.0, 0.0);
        let d = Point3::new(0.0, 0.0, 1.0);
        // The rounded double products cancel, but the represented determinant is epsilon squared.
        assert_eq!(orient3d(a, b, c, d), Ordering::Greater);
        assert_eq!(orient3d(a, c, b, d), Ordering::Less);
    }

    #[test]
    fn intervals_enclose_arithmetic_and_reject_zero_denominators() {
        for a in -32..=32 {
            for b in -32..=32 {
                let x = f64::from(a) / 8.0;
                let y = f64::from(b) / 8.0;
                for (bound, expected) in [
                    (I::point(x).add(I::point(y)), x + y),
                    (I::point(x).sub(I::point(y)), x - y),
                    (I::point(x).mul(I::point(y)), x * y),
                ] {
                    assert!(bound.lo <= expected && expected <= bound.hi);
                }
                if y != 0.0 {
                    let bound = I::point(x).div(I::point(y)).unwrap();
                    assert!(bound.lo <= x / y && x / y <= bound.hi);
                }
            }
        }
        assert!(I::point(1.0).div(I { lo: -1.0, hi: 1.0 }).is_none());
        assert!(I::point(1.0).div(I::point(-0.0)).is_none());
        assert!(!I::point(f64::MAX).add(I::point(f64::MAX)).is_finite());
        let underflow = I::point(f64::from_bits(1)).mul(I::point(0.5));
        assert!(underflow.lo <= 0.0 && underflow.hi >= f64::from_bits(1));
    }

    #[test]
    fn square_comparison_agrees_with_the_general_integers() {
        let mut state = 0x2545_f491_4f6c_dd1d_u64;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let special = [
            0.0,
            -0.0,
            f64::from_bits(1),
            f64::MIN_POSITIVE,
            1.0,
            2.0,
            f64::MAX,
        ];
        for round in 0..40_000 {
            let bits = next();
            let candidate = if round % 7 == 0 {
                special[round / 7 % special.len()]
            } else {
                f64::from_bits(((bits >> 12) % 2046 + 1) << 52 | (bits & ((1 << 52) - 1)))
            };
            let value = match round % 3 {
                // Squares near the candidate's own square, including exact ones.
                0 => candidate * candidate,
                1 => (candidate * candidate).next_up(),
                _ => f64::from_bits(next() % 0x7fef_ffff_ffff_ffff),
            };
            if !value.is_finite() {
                continue;
            }
            assert_eq!(
                square_cmp(candidate, value),
                general_square_cmp(candidate, value),
                "{candidate:e} {value:e}"
            );
        }
        assert_eq!(square_cmp(3.0, 9.0), Ordering::Equal);
        assert_eq!(square_cmp(f64::from_bits(1), 0.0), Ordering::Greater);
        assert_eq!(square_cmp(f64::INFINITY, f64::MAX), Ordering::Greater);
    }

    #[test]
    fn square_root_accepts_only_verified_squared_bounds() {
        for value in [
            0.0,
            f64::from_bits(1),
            f64::MIN_POSITIVE,
            0.25,
            1.0,
            2.0,
            f64::MAX,
        ] {
            let result = I::point(value).sqrt().unwrap();
            assert!(square_cmp(result.lo, value).is_le());
            assert!(square_cmp(result.hi, value).is_ge());
            assert!(result.is_finite());
        }
        assert!(I::point(-f64::from_bits(1)).sqrt().is_none());
        assert!(I::point(f64::INFINITY).sqrt().is_none());
    }
}
