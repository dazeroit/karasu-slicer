//! Shared private exact XY sign for finite represented points, not polygon quantization.
//!
//! Every finite f64 is an integer times eta=2^-1074. Its magnitude uses at most 2098 bits in those
//! units; differences use at most 2099, products 4198, determinant 4199. 132 base-2^32 limbs (4224
//! bits) therefore suffice with no heap or truncation. This private sign-only routine receives
//! validated finite coordinates. It makes no claim about geometry before transform rounding.

use crate::geometry::Point3;
use std::cmp::Ordering;

const WORDS: usize = 132;

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

    fn magnitude_cmp(&self, other: &Self) -> Ordering {
        self.words.iter().rev().cmp(other.words.iter().rev())
    }

    fn used(&self) -> usize {
        self.words
            .iter()
            .rposition(|&word| word != 0)
            .map_or(0, |i| i + 1)
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
            let (large, small, negative) = if self.magnitude_cmp(other).is_lt() {
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
            // Only coordinate differences or determinant subtraction use this routine. Their
            // independently bounded 2099/4199 bits fit 4224, including this carry.
            debug_assert_eq!(carry, 0);
            result.negative = self.negative;
        }
        result
    }

    fn multiply_differences(&self, other: &Self) -> Self {
        let mut result = Self::zero();
        let left = self.used();
        let right = other.used();
        // Each operand is a coordinate difference, not an arbitrary 4224-bit Integer.
        debug_assert!(left <= 66 && right <= 66);
        for i in 0..left {
            let mut carry = 0_u64;
            for j in 0..right {
                // (2^32-1)^2 + (2^32-1) + (2^32-1) <= 2^64-1.
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

/// Exact sign of the represented-XY orientation `(b - a) x (c - a)`.
///
/// A point equal to `a` or `b` gives an exactly zero determinant. Otherwise an outward-rounded
/// interval evaluation decides the sign whenever it excludes zero: every correctly rounded IEEE
/// operation is widened by one representable step in each direction (including into subnormal
/// and infinite values), so the interval always contains the exact real determinant. Intervals
/// containing zero or NaN fall back to the exact integer evaluation.
pub(crate) fn orientation(a: Point3, b: Point3, c: Point3) -> Ordering {
    #[allow(clippy::float_cmp)] // Exact coordinate identity, deliberately not proximity.
    let same = |p: Point3, q: Point3| p.x == q.x && p.y == q.y;
    if same(c, a) || same(c, b) || same(a, b) {
        return Ordering::Equal;
    }
    if let Some(sign) = filtered_orientation(a, b, c) {
        return sign;
    }
    exact_orientation(a, b, c)
}

fn widen(v: f64) -> (f64, f64) {
    (v.next_down(), v.next_up())
}

fn interval_product((a0, a1): (f64, f64), (b0, b1): (f64, f64)) -> (f64, f64) {
    let p = [a0 * b0, a0 * b1, a1 * b0, a1 * b1];
    let lo = p.iter().copied().fold(f64::INFINITY, f64::min);
    let hi = p.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    (lo.next_down(), hi.next_up())
}

fn filtered_orientation(a: Point3, b: Point3, c: Point3) -> Option<Ordering> {
    let dx = widen(b.x - a.x);
    let dy = widen(b.y - a.y);
    let ex = widen(c.x - a.x);
    let ey = widen(c.y - a.y);
    let left = interval_product(dx, ey);
    let right = interval_product(dy, ex);
    let lo = (left.0 - right.1).next_down();
    let hi = (left.1 - right.0).next_up();
    if lo.is_nan() || hi.is_nan() {
        return None;
    }
    if lo > 0.0 {
        Some(Ordering::Greater)
    } else if hi < 0.0 {
        Some(Ordering::Less)
    } else {
        None
    }
}

fn exact_orientation(a: Point3, b: Point3, c: Point3) -> Ordering {
    let dx = Integer::from_float(b.x).subtract(&Integer::from_float(a.x));
    let dy = Integer::from_float(b.y).subtract(&Integer::from_float(a.y));
    let ex = Integer::from_float(c.x).subtract(&Integer::from_float(a.x));
    let ey = Integer::from_float(c.y).subtract(&Integer::from_float(a.y));
    dx.multiply_differences(&ey)
        .subtract(&dy.multiply_differences(&ex))
        .sign()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(clippy::many_single_char_names)]
    fn filter_never_disagrees_with_the_exact_evaluation() {
        // Near-degenerate, large, tiny and subnormal configurations stress the interval bounds.
        let eta = f64::from_bits(1);
        let mut state = 0x9e37_79b9_7f4a_7c15_u64;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        for _ in 0..20_000 {
            let scale = [1.0, 1e-300, 1e300, eta * 1e5, 1e8][(next() % 5) as usize];
            let mut v = || (f64::from((next() % 2001) as u32) - 1000.0) * scale;
            let (a, b) = (Point3::new(v(), v(), 0.0), Point3::new(v(), v(), 0.0));
            // c nearly on line ab: midpoint perturbed by a few ulps.
            let t = 0.5;
            let mid = Point3::new(a.x + t * (b.x - a.x), a.y + t * (b.y - a.y), 0.0);
            let c = Point3::new(
                f64::from_bits(mid.x.to_bits().wrapping_add(next() % 3)),
                mid.y,
                0.0,
            );
            if !(c.x.is_finite() && mid.y.is_finite()) {
                continue;
            }
            for (p, q, r) in [(a, b, c), (b, c, a), (a, b, mid)] {
                if let Some(sign) = filtered_orientation(p, q, r) {
                    assert_eq!(sign, exact_orientation(p, q, r), "{p:?} {q:?} {r:?}");
                }
                assert_eq!(orientation(p, q, r), exact_orientation(p, q, r));
            }
        }
    }

    #[test]
    fn exact_sign_matches_independent_small_integer_determinants() {
        for seed in 0..2048_i32 {
            let values = [
                seed % 31 - 15,
                seed % 17 - 8,
                seed % 23 - 11,
                seed % 13 - 6,
                seed % 19 - 9,
                seed % 29 - 14,
            ];
            let [ax, ay, bx, by, cx, cy] = values.map(i128::from);
            let expected = ((bx - ax) * (cy - ay) - (by - ay) * (cx - ax)).cmp(&0);
            let [ax, ay, bx, by, cx, cy] = values.map(f64::from);
            assert_eq!(
                orientation(
                    Point3::new(ax, ay, 0.0),
                    Point3::new(bx, by, 0.0),
                    Point3::new(cx, cy, 0.0)
                ),
                expected
            );
        }
    }

    #[test]
    fn extremes_subnormals_cancellation_and_zero_have_exact_signs() {
        let zero = Point3::new(-0.0, 0.0, 0.0);
        let eta = f64::from_bits(1);
        for scale in [eta, f64::MIN_POSITIVE, 1.0, f64::MAX] {
            let a = Point3::new(-scale, -scale, 0.0);
            let b = Point3::new(scale, -scale, 0.0);
            let c = Point3::new(0.0, scale, 0.0);
            assert_eq!(orientation(a, b, c), Ordering::Greater);
            assert_eq!(orientation(b, a, c), Ordering::Less);
            assert_eq!(orientation(a, a, c), Ordering::Equal);
            assert_eq!(
                orientation(a, zero, Point3::new(scale, scale, 0.0)),
                Ordering::Equal
            );
        }
        assert_eq!(
            orientation(
                zero,
                Point3::new(f64::MAX, eta, 0.0),
                Point3::new(f64::MAX, 2.0 * eta, 0.0)
            ),
            Ordering::Greater
        );
        let next = f64::from_bits(1.0_f64.to_bits() + 1);
        assert_eq!(
            orientation(
                zero,
                Point3::new(1.0, 1.0, 0.0),
                Point3::new(1.0, next, 0.0)
            ),
            Ordering::Greater
        );
    }

    #[test]
    fn determinant_cancellation_below_binary64_product_precision_is_not_zero() {
        let zero = Point3::new(0.0, 0.0, 0.0);
        let step = f64::EPSILON;
        let b = Point3::new(1.0, 1.0 + step, 0.0);
        let c = Point3::new(1.0 - step, 1.0, 0.0);
        // The exact determinant is step^2, while rounded products can both be 1.
        assert_eq!(orientation(zero, b, c), Ordering::Greater);
        assert_eq!(orientation(zero, c, b), Ordering::Less);
        for scale in [
            2.0_f64.powi(-1000),
            2.0_f64.powi(-500),
            2.0_f64.powi(500),
            2.0_f64.powi(1000),
        ] {
            let scaled_b = Point3::new(b.x * scale, b.y * scale, 0.0);
            let scaled_c = Point3::new(c.x * scale, c.y * scale, 0.0);
            assert_eq!(orientation(zero, scaled_b, scaled_c), Ordering::Greater);
        }
    }
}
