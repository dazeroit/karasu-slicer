use super::*;

fn expression(first: f64, height: f64, index: u64) -> Exact {
    let mut result = Exact::decode(height).multiply(index).unwrap();
    result.add(&Exact::decode(first)).unwrap();
    result
}

#[test]
fn decode_round_trips_every_exponent_and_representative_significands() {
    for exponent in 0..2047_u64 {
        for fraction in [0, 1, (1 << 51) - 1, 1 << 51, (1 << 52) - 1] {
            for sign in [0, 1_u64 << 63] {
                let bits = sign | (exponent << 52) | fraction;
                let value = f64::from_bits(bits);
                let expected = if value == 0.0 { 0 } else { bits };
                assert_eq!(Exact::decode(value).round().unwrap().to_bits(), expected);
            }
        }
    }
}

#[test]
fn exact_index_does_not_round_to_binary64_above_two_to_53() {
    let index = (1_u64 << 53) + 1;
    assert_eq!(
        expression(-9_007_199_254_740_992.0, 1.0, index).round(),
        Some(1.0)
    );
    // u64::MAX - 2^64 = -1, despite u64::MAX rounding to 2^64 as f64.
    assert_eq!(
        expression(-18_446_744_073_709_551_616.0, 1.0, u64::MAX).round(),
        Some(-1.0)
    );
}

#[test]
fn full_domain_products_fit_and_report_rounding_overflow_without_arithmetic_overflow() {
    for first in [-f64::MAX, 0.0, f64::MAX] {
        let value = expression(first, f64::MAX, u64::MAX);
        assert!(!value.negative);
        assert_eq!(value.round(), None);
        assert!(value.words[WORDS - 1] != 0);
    }
    let mut negative = Exact::decode(-f64::MAX);
    negative.add(&Exact::decode(f64::MAX)).unwrap();
    assert_eq!(negative.round().unwrap().to_bits(), 0);
}

#[test]
fn guard_and_sticky_rounding_work_across_limb_boundaries() {
    // Half an ulp at 1 is 2^-53; adding the smallest subnormal makes it strictly above half.
    let mut value = Exact::decode(1.0);
    value.add(&Exact::decode(2.0_f64.powi(-53))).unwrap();
    assert_eq!(value.round(), Some(1.0));
    value.add(&Exact::decode(f64::from_bits(1))).unwrap();
    assert_eq!(value.round().unwrap().to_bits(), 1.0_f64.to_bits() + 1);
    value.negative = true;
    assert_eq!(value.round().unwrap().to_bits(), (-1.0_f64).to_bits() + 1);
}

#[test]
fn guard_and_sticky_masks_cover_word_boundary_offsets_of_both_signs() {
    for shift in [1, 63, 64, 65, 127, 128, 129] {
        for negative in [false, true] {
            let mut value = Exact::zero();
            value.negative = negative;
            // Even retained significand with an exact half-ulp remainder.
            for bit in [shift + 52, shift + 1, shift - 1] {
                value.words[bit / 64] |= 1_u64 << (bit % 64);
            }
            let sign = u64::from(negative) << 63;
            let expected = sign | (u64::try_from(shift + 1).unwrap() << 52) | 2;
            assert_eq!(value.round().unwrap().to_bits(), expected);
            if shift > 1 {
                value.words[0] |= 1;
                assert_eq!(value.round().unwrap().to_bits(), expected + 1);
            }
        }
    }
}

#[test]
fn rounding_carry_and_finite_overflow_threshold_are_distinct() {
    let predecessor_of_two = f64::from_bits(2.0_f64.to_bits() - 1);
    assert_eq!(
        expression(predecessor_of_two, 2.0_f64.powi(-53), 1).round(),
        Some(2.0)
    );
    let half_ulp = 2.0_f64.powi(970);
    assert_eq!(expression(f64::MAX, half_ulp, 1).round(), None);
    let below_half = f64::from_bits(half_ulp.to_bits() - 1);
    assert_eq!(expression(f64::MAX, below_half, 1).round(), Some(f64::MAX));
}

#[test]
fn terminal_count_has_u64_semantics_without_output_allocation() {
    let first = Exact::decode(0.0);
    let height = Exact::decode(1.0);
    let mut cancel = Cancellation::never();
    let two_to_64 = 18_446_744_073_709_551_616.0;
    assert_eq!(
        plane_count(ScheduleEnd::Before(two_to_64), &first, &height, &mut cancel),
        Err(ScheduleError::IndexRange)
    );
    // The first excluded index is exactly u64::MAX when a=1 and terminal=2^64.
    assert_eq!(
        plane_count(
            ScheduleEnd::Before(two_to_64),
            &Exact::decode(1.0),
            &height,
            &mut cancel
        ),
        Ok(u64::MAX)
    );
    assert_eq!(
        plane_count(
            ScheduleEnd::Through(two_to_64),
            &Exact::decode(1.0),
            &height,
            &mut cancel
        ),
        Err(ScheduleError::IndexRange)
    );
}

#[test]
fn exact_recurrence_matches_direct_expression_over_sign_changes_and_large_spacing() {
    for (first, height) in [
        (-f64::MAX, f64::MAX),
        (-1024.0, 0.1),
        (-1.0, f64::from_bits(1)),
        (1.0, 2.0_f64.powi(970)),
    ] {
        let mut running = Exact::decode(first);
        let step = Exact::decode(height);
        for index in 0..1024 {
            let direct = expression(first, height, index);
            assert_eq!(running.compare(&direct), Ordering::Equal);
            assert_eq!(
                running.round().map(f64::to_bits),
                direct.round().map(f64::to_bits)
            );
            running.add(&step).unwrap();
        }
    }
}

#[test]
fn defensive_capacity_errors_are_typed() {
    let full = Exact {
        words: [u64::MAX; WORDS],
        negative: false,
    };
    assert!(matches!(
        full.multiply(2),
        Err(ScheduleError::ArithmeticOverflow)
    ));
    let mut sum = full.clone();
    assert_eq!(sum.add(&full), Err(ScheduleError::ArithmeticOverflow));
}
