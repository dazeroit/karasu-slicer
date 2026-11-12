//! Fixed-width signed integers for the Boolean kernel's exact arithmetic (TD-140).
//!
//! An [`Int<L>`] is a sign and a magnitude of `L` 64-bit limbs, least significant first, so it
//! holds every integer of absolute value below `2^(64 L)`. Zero is never negative. The kernel's
//! quantities have proved bounds (see [`super::plane`]), and each type is chosen wide enough for
//! its bound; an operation whose exact result does not fit is a violated internal invariant and
//! panics, which the tests at the bounds' extremes exercise. Nothing allocates, and every result
//! is the exact integer on every platform.
use std::cmp::Ordering;

/// A signed integer of `L` 64-bit limbs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Int<const L: usize> {
    negative: bool,
    magnitude: [u64; L],
}

#[allow(clippy::cast_possible_truncation)] // Deliberately keeps the low 64 bits.
const fn low(value: u128) -> u64 {
    value as u64
}

/// `a` compared with `b`, as magnitudes of the same width.
fn compare_magnitudes<const L: usize>(a: &[u64; L], b: &[u64; L]) -> Ordering {
    for k in (0..L).rev() {
        match a[k].cmp(&b[k]) {
            Ordering::Equal => {}
            other => return other,
        }
    }
    Ordering::Equal
}

impl<const L: usize> Int<L> {
    /// Zero.
    pub(crate) const ZERO: Self = Self {
        negative: false,
        magnitude: [0; L],
    };

    /// `value`, which must fit (`L` of 2 or more always holds an `i128`).
    pub(crate) fn from_i128(value: i128) -> Self {
        let magnitude = value.unsigned_abs();
        let mut limbs = [0; L];
        limbs[0] = low(magnitude);
        if L > 1 {
            limbs[1] = low(magnitude >> 64);
        } else {
            assert!(magnitude >> 64 == 0, "an i128 beyond one limb");
        }
        Self {
            negative: value < 0,
            magnitude: limbs,
        }
    }

    /// The value as an `i64`, when it fits.
    pub(crate) fn to_i64(self) -> Option<i64> {
        if self.magnitude.iter().skip(1).any(|&limb| limb != 0) {
            return None;
        }
        let magnitude = i128::from(self.magnitude[0]);
        i64::try_from(if self.negative { -magnitude } else { magnitude }).ok()
    }

    /// Whether it is zero.
    pub(crate) fn is_zero(&self) -> bool {
        self.magnitude.iter().all(|&limb| limb == 0)
    }

    /// Its sign: `Less` below zero, `Equal` at zero, `Greater` above.
    pub(crate) fn sign(&self) -> Ordering {
        if self.is_zero() {
            Ordering::Equal
        } else if self.negative {
            Ordering::Less
        } else {
            Ordering::Greater
        }
    }

    /// Its opposite.
    #[must_use]
    pub(crate) fn neg(&self) -> Self {
        Self {
            negative: !self.negative && !self.is_zero(),
            magnitude: self.magnitude,
        }
    }

    /// The same value with `M` limbs. Panics if it does not fit.
    #[cfg(test)]
    pub(crate) fn resize<const M: usize>(&self) -> Int<M> {
        let mut magnitude = [0; M];
        for (k, &limb) in self.magnitude.iter().enumerate() {
            if k < M {
                magnitude[k] = limb;
            } else {
                assert!(
                    limb == 0,
                    "an Int too wide to resize: a TD-140 bound is violated"
                );
            }
        }
        Int {
            negative: self.negative,
            magnitude,
        }
    }

    /// The exact sum. Panics if it does not fit in `L` limbs.
    #[must_use]
    pub(crate) fn add(&self, other: &Self) -> Self {
        if self.negative == other.negative {
            let mut magnitude = [0; L];
            let mut carry = 0_u128;
            for (k, limb) in magnitude.iter_mut().enumerate() {
                let sum = u128::from(self.magnitude[k]) + u128::from(other.magnitude[k]) + carry;
                *limb = low(sum);
                carry = sum >> 64;
            }
            assert!(
                carry == 0,
                "an Int sum overflowed: a TD-140 bound is violated"
            );
            return Self {
                negative: self.negative && magnitude.iter().any(|&limb| limb != 0),
                magnitude,
            };
        }
        // Opposite signs: the larger magnitude less the smaller, with the larger's sign.
        let (big, small) = match compare_magnitudes(&self.magnitude, &other.magnitude) {
            Ordering::Less => (other, self),
            Ordering::Equal => return Self::ZERO,
            Ordering::Greater => (self, other),
        };
        let mut magnitude = [0; L];
        let mut borrow = false;
        for (k, limb) in magnitude.iter_mut().enumerate() {
            let (difference, first) = big.magnitude[k].overflowing_sub(small.magnitude[k]);
            let (difference, second) = difference.overflowing_sub(u64::from(borrow));
            *limb = difference;
            borrow = first || second;
        }
        debug_assert!(!borrow);
        Self {
            negative: big.negative,
            magnitude,
        }
    }

    /// The exact difference. Panics if it does not fit in `L` limbs.
    #[must_use]
    pub(crate) fn sub(&self, other: &Self) -> Self {
        self.add(&other.neg())
    }

    /// The exact product of `a` and `b`, in `L` limbs. Panics if it does not fit.
    pub(crate) fn product<const A: usize, const B: usize>(a: &Int<A>, b: &Int<B>) -> Self {
        let mut magnitude = [0; L];
        for (i, &x) in a.magnitude.iter().enumerate() {
            if x == 0 {
                continue;
            }
            let mut carry = 0_u128;
            for (j, &y) in b.magnitude.iter().enumerate() {
                let at = i + j;
                let current = if at < L { magnitude[at] } else { 0 };
                // At most (2^64 - 1)^2 + 2 (2^64 - 1) = 2^128 - 1: no overflow.
                let sum = u128::from(x) * u128::from(y) + u128::from(current) + carry;
                if at < L {
                    magnitude[at] = low(sum);
                } else {
                    assert!(
                        low(sum) == 0,
                        "an Int product overflowed: a TD-140 bound is violated"
                    );
                }
                carry = sum >> 64;
            }
            let at = i + B;
            if at < L {
                // The limb at `i + B` has not been written by this row yet: it is free.
                magnitude[at] = low(carry);
            } else {
                assert!(
                    carry == 0,
                    "an Int product overflowed: a TD-140 bound is violated"
                );
            }
        }
        let zero = magnitude.iter().all(|&limb| limb == 0);
        Self {
            negative: (a.negative != b.negative) && !zero,
            magnitude,
        }
    }

    /// The exact product of two `i128`s, in `L` limbs.
    pub(crate) fn product_i128(a: i128, b: i128) -> Self {
        Self::product(&Int::<2>::from_i128(a), &Int::<2>::from_i128(b))
    }

    /// The exact comparison of two values.
    pub(crate) fn compare(&self, other: &Self) -> Ordering {
        match (self.sign(), other.sign()) {
            (Ordering::Less, Ordering::Less) => {
                compare_magnitudes(&other.magnitude, &self.magnitude)
            }
            (Ordering::Less, _) => Ordering::Less,
            (_, Ordering::Less) => Ordering::Greater,
            _ => compare_magnitudes(&self.magnitude, &other.magnitude),
        }
    }

    /// The value as an `f64` within a few units in the last place: for display and diagnostics,
    /// never for a decision.
    pub(crate) fn approximate(&self) -> f64 {
        #[allow(clippy::cast_precision_loss)] // An approximation by design.
        let magnitude = self.magnitude.iter().rev().fold(0.0, |sum, &limb| {
            sum * 18_446_744_073_709_551_616.0 + limb as f64
        });
        if self.negative {
            -magnitude
        } else {
            magnitude
        }
    }

    /// The number of significant bits of the magnitude (0 for zero).
    #[cfg(test)]
    pub(crate) fn bits(&self) -> u32 {
        for k in (0..L).rev() {
            if self.magnitude[k] != 0 {
                #[allow(clippy::cast_possible_truncation)] // A few limbs.
                let base = (k as u32) * 64;
                return base + 64 - self.magnitude[k].leading_zeros();
            }
        }
        0
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::Int;
    use std::cmp::Ordering;

    /// A deterministic pseudo-random sequence (xorshift64*).
    pub(crate) struct Rng(u64);

    impl Rng {
        pub(crate) fn new(seed: u64) -> Self {
            Self(seed.max(1))
        }
        pub(crate) fn next(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x >> 12;
            x ^= x << 25;
            x ^= x >> 27;
            self.0 = x;
            x.wrapping_mul(0x2545_F491_4F6C_DD1D)
        }
        /// A value in `-2^bits < v < 2^bits`, small ones often.
        pub(crate) fn signed(&mut self, bits: u32) -> i128 {
            let width = u32::try_from(self.next() % u64::from(bits + 1)).unwrap_or(0);
            let magnitude = if width == 0 {
                0
            } else {
                (i128::from(self.next()) << 64 | i128::from(self.next())) & ((1_i128 << width) - 1)
            };
            if self.next() & 1 == 0 {
                magnitude
            } else {
                -magnitude
            }
        }
    }

    fn int(v: i128) -> Int<4> {
        Int::from_i128(v)
    }

    #[test]
    fn small_values_agree_with_i128() {
        let mut rng = Rng::new(7);
        for _ in 0..20_000 {
            let (a, b) = (rng.signed(62), rng.signed(62));
            assert_eq!(int(a).add(&int(b)), int(a + b));
            assert_eq!(int(a).sub(&int(b)), int(a - b));
            assert_eq!(Int::<4>::product(&int(a), &int(b)), int(a * b));
            assert_eq!(int(a).compare(&int(b)), a.cmp(&b));
            assert_eq!(int(a).sign(), a.cmp(&0));
            assert_eq!(int(a).neg(), int(-a));
        }
        assert_eq!(int(0).neg().sign(), Ordering::Equal);
        assert_eq!(int(5).sub(&int(5)), Int::ZERO);
    }

    #[test]
    fn wide_products_obey_the_ring_laws() {
        // (a b) c = a (b c), a (b + c) = a b + a c, and (a - b)(a + b) = a² - b², with factors
        // of 100 to 126 bits, products far beyond i128.
        let mut rng = Rng::new(11);
        for _ in 0..5_000 {
            let (a, b, c) = (rng.signed(126), rng.signed(126), rng.signed(100));
            let (a2, b2, c2) = (
                Int::<2>::from_i128(a),
                Int::<2>::from_i128(b),
                Int::<2>::from_i128(c),
            );
            let ab: Int<4> = Int::product(&a2, &b2);
            let bc: Int<4> = Int::product(&b2, &c2);
            let left: Int<6> = Int::product(&ab, &c2);
            let right: Int<6> = Int::product(&a2, &bc);
            assert_eq!(left, right);
            let sum = Int::<3>::from_i128(b).add(&Int::from_i128(c));
            let distributed: Int<6> = Int::product(&a2, &sum);
            let ac: Int<6> = Int::product(&a2, &c2);
            assert_eq!(distributed, ab.resize::<6>().add(&ac));
            let difference = Int::<3>::from_i128(a).sub(&Int::from_i128(b));
            let total = Int::<3>::from_i128(a).add(&Int::from_i128(b));
            let squares: Int<6> = Int::product(&difference, &total);
            let aa: Int<6> = Int::product(&a2, &a2);
            let bb: Int<6> = Int::product(&b2, &b2);
            assert_eq!(squares, aa.sub(&bb));
            assert_eq!(left.compare(&left.add(&Int::from_i128(1))), Ordering::Less);
        }
    }

    #[test]
    fn the_extremes_fit_and_one_beyond_overflows() {
        // (2^128 - 1)^2 fills four limbs exactly.
        let max = Int::<2> {
            negative: false,
            magnitude: [u64::MAX; 2],
        };
        let square: Int<4> = Int::product(&max, &max);
        assert_eq!(square.bits(), 256);
        assert_eq!(square.neg().sign(), Ordering::Less);
        let beyond = std::panic::catch_unwind(|| {
            let _: Int<3> = Int::product(&max, &max);
        });
        assert!(beyond.is_err());
        let full = Int::<4> {
            negative: false,
            magnitude: [u64::MAX; 4],
        };
        assert!(std::panic::catch_unwind(|| full.add(&Int::from_i128(1))).is_err());
        assert_eq!(full.add(&Int::from_i128(-1)).bits(), 256);
        assert!(std::panic::catch_unwind(|| full.resize::<3>()).is_err());
        assert_eq!(
            Int::<2>::from_i128(-3).resize::<5>(),
            Int::<5>::from_i128(-3)
        );
    }
}
