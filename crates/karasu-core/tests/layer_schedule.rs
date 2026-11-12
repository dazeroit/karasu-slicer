//! Independent numerical and policy checks for uniform world-Z plans.

use karasu_core::cancellation::{Cancellation, Cancelled, Checkpoint};
use karasu_core::slicing::schedule::{
    build_layer_schedule, build_layer_schedule_with_cancellation, build_slab_schedule, LayerPlane,
    ScheduleEnd, ScheduleError, ScheduleLimits, ScheduleSpecError, UniformScheduleSpec,
};

// The native integer cast is the independent nearest-even rounding oracle.
#[allow(clippy::cast_precision_loss)]
fn round_integer(value: i128) -> f64 {
    value as f64
}

fn limits() -> ScheduleLimits {
    ScheduleLimits {
        max_layers: 4096,
        max_payload_bytes: 4096 * std::mem::size_of::<LayerPlane>(),
    }
}

fn spec(first: f64, height: f64, end: ScheduleEnd) -> UniformScheduleSpec {
    UniformScheduleSpec::try_new(first, height, end).expect("valid test specification")
}

fn coordinates(first: f64, height: f64, count: u64) -> Vec<f64> {
    build_layer_schedule(spec(first, height, ScheduleEnd::Count(count)), limits())
        .expect("representable schedule")
        .planes()
        .iter()
        .map(|p| p.z_mm)
        .collect()
}

#[test]
fn specification_validation_has_documented_precedence() {
    assert!(matches!(
        UniformScheduleSpec::try_new(f64::NAN, 0.0, ScheduleEnd::Before(f64::NAN)),
        Err(ScheduleSpecError::NonFiniteFirstPlane)
    ));
    for first in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(matches!(
            UniformScheduleSpec::try_new(first, 1.0, ScheduleEnd::Count(0)),
            Err(ScheduleSpecError::NonFiniteFirstPlane)
        ));
    }
    for height in [0.0, -0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(matches!(
            UniformScheduleSpec::try_new(0.0, height, ScheduleEnd::Through(f64::NAN)),
            Err(ScheduleSpecError::InvalidHeight)
        ));
    }
    for terminal in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for end in [
            ScheduleEnd::Before(terminal),
            ScheduleEnd::Through(terminal),
        ] {
            assert!(matches!(
                UniformScheduleSpec::try_new(0.0, 1.0, end),
                Err(ScheduleSpecError::NonFiniteTerminal)
            ));
        }
    }
}

#[test]
fn specification_normalizes_signed_zero_and_retains_explicit_policy() {
    let request = spec(-0.0, 0.125, ScheduleEnd::Through(-0.0));
    assert_eq!(request.first_plane_z_mm().to_bits(), 0);
    assert_eq!(request.layer_height_mm().to_bits(), 0.125_f64.to_bits());
    assert!(matches!(request.end(), ScheduleEnd::Through(z) if z.to_bits() == 0));
    let schedule = build_layer_schedule(request, limits()).unwrap();
    assert_eq!(schedule.spec(), Some(request));
    assert_eq!(schedule.planes().len(), 1);
    assert_eq!(schedule.planes()[0].z_mm.to_bits(), 0);
    assert!(matches!(
        spec(-0.0, 1.0, ScheduleEnd::Before(-0.0)).end(),
        ScheduleEnd::Before(z) if z.to_bits() == 0
    ));
}

#[test]
fn empty_schedules_need_no_payload_capacity() {
    let zero = ScheduleLimits {
        max_layers: 0,
        max_payload_bytes: 0,
    };
    for end in [
        ScheduleEnd::Count(0),
        ScheduleEnd::Before(5.0),
        ScheduleEnd::Before(4.0),
        ScheduleEnd::Through(4.0),
    ] {
        assert_eq!(
            build_layer_schedule(spec(5.0, 0.25, end), zero)
                .unwrap()
                .planes(),
            []
        );
    }
}

#[test]
fn terminal_equality_is_explicit() {
    for (end, expected) in [
        (ScheduleEnd::Before(1.0), vec![-1.0, -0.5, 0.0, 0.5]),
        (ScheduleEnd::Through(1.0), vec![-1.0, -0.5, 0.0, 0.5, 1.0]),
        (ScheduleEnd::Through(-1.0), vec![-1.0]),
        (ScheduleEnd::Before(-1.0), vec![]),
    ] {
        let schedule = build_layer_schedule(spec(-1.0, 0.5, end), limits()).unwrap();
        assert_eq!(
            schedule.planes().iter().map(|p| p.z_mm).collect::<Vec<_>>(),
            expected
        );
    }
}

// This oracle works in a common rational unit, independently of binary64 decoding and
// fixed-limb arithmetic. Integer-to-f64 rounding is followed only by normal power-of-two
// scaling; that scaling is exact throughout these bounded cases.
#[test]
fn bounded_dyadic_sweep_matches_an_independent_integer_oracle() {
    for exponent in [-20, -3, 0, 7, 20] {
        let unit = 2_f64.powi(exponent);
        for first_units in [-511_i128, -127, -1, 0, 1, 127, 511] {
            for height_units in [1_i128, 3, 7, 31] {
                for terminal_units in [-520_i128, -1, 0, 128, 520] {
                    for inclusive in [false, true] {
                        let terminal = round_integer(terminal_units) * unit;
                        let end = if inclusive {
                            ScheduleEnd::Through(terminal)
                        } else {
                            ScheduleEnd::Before(terminal)
                        };
                        let expected: Vec<_> = (0_u64..2048)
                            .map(|index| (index, first_units + i128::from(index) * height_units))
                            .take_while(|&(_, value)| {
                                value < terminal_units || (inclusive && value == terminal_units)
                            })
                            .map(|(index, value)| (index, (round_integer(value) * unit).to_bits()))
                            .collect();
                        let schedule = build_layer_schedule(
                            spec(
                                round_integer(first_units) * unit,
                                round_integer(height_units) * unit,
                                end,
                            ),
                            limits(),
                        )
                        .unwrap();
                        let actual: Vec<_> = schedule
                            .planes()
                            .iter()
                            .map(|plane| {
                                assert_eq!(
                                    plane.nominal_height_mm.to_bits(),
                                    (round_integer(height_units) * unit).to_bits()
                                );
                                (plane.index, plane.z_mm.to_bits())
                            })
                            .collect();
                        assert_eq!(actual, expected);
                    }
                }
            }
        }
    }
}

#[test]
fn rounding_near_two_to_the_53_matches_integer_oracle() {
    for first_units in [-(1_i128 << 53) - 64, (1_i128 << 53) - 64] {
        for height_units in [3_i128, 5, 7, 9] {
            let schedule = build_layer_schedule(
                spec(
                    round_integer(first_units),
                    round_integer(height_units),
                    ScheduleEnd::Count(128),
                ),
                limits(),
            )
            .unwrap();
            for plane in schedule.planes() {
                let exact_integer = first_units + i128::from(plane.index) * height_units;
                assert_eq!(
                    plane.z_mm.to_bits(),
                    (round_integer(exact_integer)).to_bits()
                );
            }
        }
    }
}

#[test]
fn ties_are_even_for_positive_and_negative_values() {
    let pivot = 2_f64.powi(53);
    assert_eq!(
        coordinates(pivot, 3.0, 3),
        vec![pivot, pivot + 4.0, pivot + 6.0]
    );
    assert_eq!(
        coordinates(-pivot - 4.0, 3.0, 3),
        vec![-pivot - 4.0, -pivot, -pivot + 2.0]
    );
    // The first tie has an even lower significand, the next an even upper one.
    assert_eq!(
        coordinates(pivot, 5.0, 3),
        vec![pivot, pivot + 4.0, pivot + 10.0]
    );
}

#[test]
fn exact_endpoint_membership_precedes_rounding() {
    let pivot = 2_f64.powi(53);
    for end in [
        ScheduleEnd::Before(pivot + 4.0),
        ScheduleEnd::Through(pivot + 4.0),
    ] {
        let schedule = build_layer_schedule(spec(pivot, 3.0, end), limits()).unwrap();
        assert_eq!(schedule.planes().len(), 2);
        assert_eq!(schedule.planes()[1].z_mm, pivot + 4.0);
    }
}

#[test]
fn decimal_height_has_no_repeated_addition_drift() {
    let schedule =
        build_layer_schedule(spec(0.0, 0.1, ScheduleEnd::Count(1000)), limits()).unwrap();
    // Multiplication by an exactly represented small index has a single rounding and
    // serves as an independent oracle when the first plane is zero.
    for plane in schedule.planes() {
        assert_eq!(
            plane.z_mm.to_bits(),
            (f64::from(u32::try_from(plane.index).unwrap()) * 0.1).to_bits()
        );
    }
    assert_eq!(schedule.planes()[10].z_mm.to_bits(), 1.0_f64.to_bits());
}

#[test]
fn subnormals_and_signed_cancellation_remain_exact() {
    let eta = f64::from_bits(1);
    let planes = coordinates(-3.0 * eta, eta, 8);
    let expected = [
        -3.0 * eta,
        -2.0 * eta,
        -eta,
        0.0,
        eta,
        2.0 * eta,
        3.0 * eta,
        4.0 * eta,
    ];
    for (actual, expected) in planes.iter().zip(expected) {
        assert_eq!(actual.to_bits(), expected.to_bits());
    }
    assert_eq!(
        coordinates(-f64::MAX, f64::MAX, 3),
        vec![-f64::MAX, 0.0, f64::MAX]
    );
}

#[test]
fn rounding_carry_crosses_a_binade() {
    let lower = f64::from_bits(1.0_f64.to_bits() - 1);
    let half_gap_below_one = 2_f64.powi(-54);
    assert_eq!(coordinates(lower, half_gap_below_one, 2), vec![lower, 1.0]);
}

#[test]
fn overflowing_rounded_plane_reports_its_index() {
    assert!(matches!(
        build_layer_schedule(spec(f64::MAX, f64::MAX, ScheduleEnd::Count(2)), limits()),
        Err(ScheduleError::PlaneOutOfRange { index: 1 })
    ));
    let half_ulp = 2_f64.powi(970);
    assert!(matches!(
        build_layer_schedule(spec(f64::MAX, half_ulp, ScheduleEnd::Count(2)), limits()),
        Err(ScheduleError::PlaneOutOfRange { index: 1 })
    ));
}

#[test]
fn resolution_loss_preserves_first_offending_context() {
    let pivot = 2_f64.powi(53);
    match build_layer_schedule(spec(pivot, 1.0, ScheduleEnd::Count(3)), limits()) {
        Err(ScheduleError::ResolutionLoss {
            index,
            previous_z_mm,
            z_mm,
        }) => {
            assert_eq!(index, 1);
            assert_eq!(previous_z_mm.to_bits(), pivot.to_bits());
            assert_eq!(z_mm.to_bits(), pivot.to_bits());
        }
        other => panic!("expected resolution failure, got {other:?}"),
    }
}

#[test]
fn mathematical_index_limit_precedes_policy_and_resolution_checks() {
    let zero = ScheduleLimits {
        max_layers: 0,
        max_payload_bytes: 0,
    };
    for end in [
        ScheduleEnd::Before(f64::MAX),
        ScheduleEnd::Through(f64::MAX),
    ] {
        assert!(matches!(
            build_layer_schedule(spec(0.0, f64::from_bits(1), end), zero),
            Err(ScheduleError::IndexRange)
        ));
    }
    // u64::MAX is an accepted count, regardless of host capacity. The explicit cap wins.
    assert!(matches!(
        build_layer_schedule(spec(0.0, 1.0, ScheduleEnd::Count(u64::MAX)), zero),
        Err(ScheduleError::LayerLimit { limit: 0 })
    ));
}

#[test]
fn exact_u64_boundary_is_not_rounded_through_f64() {
    let terminal = 2_f64.powi(64);
    let cap = ScheduleLimits {
        max_layers: 2,
        max_payload_bytes: 0,
    };
    // -1 + i < 2^64-2048 requires 2^64-2047 layers, a valid u64 count.
    assert!(matches!(
        build_layer_schedule(spec(-1.0, 1.0, ScheduleEnd::Before(terminal - 2048.0)), cap),
        Err(ScheduleError::LayerLimit { limit: 2 })
    ));
    // 1+i < 2^64 accepts exactly u64::MAX planes; including the endpoint adds one.
    assert!(matches!(
        build_layer_schedule(spec(1.0, 1.0, ScheduleEnd::Before(terminal)), cap),
        Err(ScheduleError::LayerLimit { limit: 2 })
    ));
    assert!(matches!(
        build_layer_schedule(spec(1.0, 1.0, ScheduleEnd::Through(terminal)), cap),
        Err(ScheduleError::IndexRange)
    ));
}

#[test]
fn count_limit_precedes_payload_and_plane_failures() {
    assert!(matches!(
        build_layer_schedule(
            spec(f64::MAX, f64::MAX, ScheduleEnd::Count(2)),
            ScheduleLimits {
                max_layers: 1,
                max_payload_bytes: 0
            }
        ),
        Err(ScheduleError::LayerLimit { limit: 1 })
    ));
    assert!(matches!(
        build_layer_schedule(
            spec(f64::MAX, f64::MAX, ScheduleEnd::Count(2)),
            ScheduleLimits {
                max_layers: 2,
                max_payload_bytes: 0
            }
        ),
        Err(ScheduleError::PayloadLimit { limit: 0 })
    ));
}

#[test]
fn payload_budget_counts_only_plane_storage() {
    let bytes = 3 * std::mem::size_of::<LayerPlane>();
    let request = spec(0.0, 0.25, ScheduleEnd::Count(3));
    assert!(build_layer_schedule(
        request,
        ScheduleLimits {
            max_layers: 3,
            max_payload_bytes: bytes
        }
    )
    .is_ok());
    assert!(matches!(
        build_layer_schedule(request, ScheduleLimits { max_layers: 3, max_payload_bytes: bytes - 1 }),
        Err(ScheduleError::PayloadLimit { limit }) if limit == bytes - 1
    ));
}

#[test]
fn every_schedule_checkpoint_can_cancel_without_partial_success() {
    for checkpoint in [
        Checkpoint::ScheduleStart,
        Checkpoint::ScheduleCount,
        Checkpoint::ScheduleStorage,
        Checkpoint::SchedulePlanes,
        Checkpoint::ScheduleComplete,
    ] {
        let mut predicate = |point| point == checkpoint;
        assert!(matches!(
            build_layer_schedule_with_cancellation(
                spec(-1.0, 0.125, ScheduleEnd::Through(65.0)),
                limits(),
                &mut Cancellation::new(&mut predicate)
            ),
            Err(ScheduleError::Cancelled(Cancelled { checkpoint: actual })) if actual == checkpoint
        ));
    }
}

#[test]
fn reservation_has_two_cancellation_boundaries() {
    for wanted in [1, 2] {
        let mut seen = 0;
        let mut predicate = |point| {
            if point == Checkpoint::ScheduleStorage {
                seen += 1;
            }
            seen == wanted
        };
        assert!(matches!(
            build_layer_schedule_with_cancellation(
                spec(0.0, 0.125, ScheduleEnd::Count(1)),
                limits(),
                &mut Cancellation::new(&mut predicate)
            ),
            Err(ScheduleError::Cancelled(Cancelled {
                checkpoint: Checkpoint::ScheduleStorage
            }))
        ));
    }
}

#[test]
fn empty_schedule_checks_start_and_completion_only() {
    let mut trace = Vec::new();
    let mut predicate = |point| {
        trace.push(point);
        false
    };
    build_layer_schedule_with_cancellation(
        spec(0.0, 1.0, ScheduleEnd::Count(0)),
        limits(),
        &mut Cancellation::new(&mut predicate),
    )
    .unwrap();
    assert_eq!(
        trace,
        vec![Checkpoint::ScheduleStart, Checkpoint::ScheduleComplete]
    );
}

#[test]
fn trace_and_results_replay_deterministically() {
    let request = spec(-32.0, 0.125, ScheduleEnd::Through(32.0));
    let mut traces = Vec::new();
    let mut results = Vec::new();
    for _ in 0..2 {
        let mut trace = Vec::new();
        let mut predicate = |point| {
            trace.push(point);
            false
        };
        let schedule = build_layer_schedule_with_cancellation(
            request,
            limits(),
            &mut Cancellation::new(&mut predicate),
        )
        .unwrap();
        results.push(
            schedule
                .planes()
                .iter()
                .map(|p| (p.index, p.z_mm.to_bits(), p.nominal_height_mm.to_bits()))
                .collect::<Vec<_>>(),
        );
        traces.push(trace);
    }
    assert_eq!(results[0], results[1]);
    assert_eq!(traces[0], traces[1]);
    assert_eq!(
        traces[0]
            .iter()
            .filter(|&&p| p == Checkpoint::SchedulePlanes)
            .count(),
        3
    );
    let ordinary = build_layer_schedule(request, limits()).unwrap();
    assert_eq!(
        results[0].iter().map(|p| p.1).collect::<Vec<_>>(),
        ordinary
            .planes()
            .iter()
            .map(|p| p.z_mm.to_bits())
            .collect::<Vec<_>>()
    );
}

#[test]
fn earlier_failure_precedes_a_later_cancellation_checkpoint() {
    let mut trace = Vec::new();
    let mut predicate = |point| {
        trace.push(point);
        point == Checkpoint::ScheduleComplete
    };
    assert!(matches!(
        build_layer_schedule_with_cancellation(
            spec(f64::MAX, f64::MAX, ScheduleEnd::Count(2)),
            limits(),
            &mut Cancellation::new(&mut predicate)
        ),
        Err(ScheduleError::PlaneOutOfRange { index: 1 })
    ));
    assert!(!trace.contains(&Checkpoint::ScheduleComplete));
}

#[test]
fn deterministic_raw_binary64_sweep_has_single_rounding() {
    let mut state = 0x6a09_e667_f3bc_c909_u64;
    for exponent in (0_u64..2047).step_by(17) {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let fraction = (state & ((1_u64 << 52) - 1)) | 1;
        let height = f64::from_bits((exponent << 52) | fraction);
        // With a=-h, the exact expression is (i-1)*h. This oracle needs just
        // one native multiplication, independently of any multi-limb recurrence.
        let expected: Vec<_> = (0_u32..32)
            .map(|index| {
                let z = (f64::from(index) - 1.0) * height;
                if z == 0.0 {
                    0.0
                } else {
                    z
                }
            })
            .collect();
        let actual = build_layer_schedule(spec(-height, height, ScheduleEnd::Count(32)), limits());
        if let Some(index) = expected.iter().position(|z| !z.is_finite()) {
            assert!(
                matches!(actual, Err(ScheduleError::PlaneOutOfRange { index: actual_index }) if actual_index == index as u64)
            );
        } else {
            let actual = actual.unwrap();
            for (plane, expected) in actual.planes().iter().zip(expected) {
                assert_eq!(
                    plane.z_mm.to_bits(),
                    expected.to_bits(),
                    "height bits {:016x}, index {}",
                    height.to_bits(),
                    plane.index
                );
            }
        }
    }
}

#[test]
fn impossible_host_payload_fails_before_reservation() {
    assert!(matches!(
        build_layer_schedule(
            spec(0.0, 1.0, ScheduleEnd::Count(u64::MAX)),
            ScheduleLimits {
                max_layers: u64::MAX,
                max_payload_bytes: usize::MAX
            }
        ),
        Err(ScheduleError::SizeOverflow)
    ));
}

#[test]
fn cancellation_checks_every_plane_batch() {
    for wanted in 1..=3 {
        let mut seen = 0;
        let mut predicate = |point| {
            if point == Checkpoint::SchedulePlanes {
                seen += 1;
            }
            point == Checkpoint::SchedulePlanes && seen == wanted
        };
        assert!(matches!(
            build_layer_schedule_with_cancellation(
                spec(0.0, 1.0, ScheduleEnd::Count(513)),
                limits(),
                &mut Cancellation::new(&mut predicate)
            ),
            Err(ScheduleError::Cancelled(Cancelled {
                checkpoint: Checkpoint::SchedulePlanes
            }))
        ));
        assert_eq!(seen, wanted);
    }
}

#[test]
fn count_search_cancellation_precedes_zero_policy_cap() {
    let mut trace = Vec::new();
    let mut predicate = |point| {
        trace.push(point);
        point == Checkpoint::ScheduleCount
    };
    assert!(matches!(
        build_layer_schedule_with_cancellation(
            spec(0.0, 0.125, ScheduleEnd::Through(1.0)),
            ScheduleLimits {
                max_layers: 0,
                max_payload_bytes: 0
            },
            &mut Cancellation::new(&mut predicate)
        ),
        Err(ScheduleError::Cancelled(Cancelled {
            checkpoint: Checkpoint::ScheduleCount
        }))
    ));
    assert_eq!(
        trace,
        vec![Checkpoint::ScheduleStart, Checkpoint::ScheduleCount]
    );
}

#[test]
fn slab_schedules_sum_integer_heights_without_drift() {
    // 30 000 layers alternating 0.01 and 0.03 mm: positions are exact nanometre sums.
    let heights: Vec<u64> = (0..30_000)
        .map(|i| if i % 2 == 0 { 10_000 } else { 30_000 })
        .collect();
    let limits = ScheduleLimits {
        max_layers: 30_000,
        max_payload_bytes: 30_000 * 64,
    };
    let schedule = build_slab_schedule(&heights, limits, &mut Cancellation::never()).unwrap();
    assert_eq!(schedule.spec(), None);
    let slabs = schedule.slabs().unwrap();
    assert_eq!(slabs.len(), 30_000);
    assert_eq!(slabs[29_999].top_mm, 600.0);
    assert_eq!(slabs[1].bottom_mm, 0.01);
    assert_eq!(slabs[1].height_mm, 0.03);
    for (plane, slab) in schedule.planes().iter().zip(slabs) {
        assert_eq!(plane.nominal_height_mm, slab.height_mm);
        assert!(slab.bottom_mm < plane.z_mm && plane.z_mm < slab.top_mm);
    }
    assert_eq!(schedule.planes()[1].z_mm, 0.025);
    // Zero heights, stacks above one metre and the caller's caps are refused.
    let mut cancel = Cancellation::never();
    assert!(matches!(
        build_slab_schedule(&[10_000, 0], limits, &mut cancel),
        Err(ScheduleError::PlaneOutOfRange { index: 1 })
    ));
    assert!(matches!(
        build_slab_schedule(&[600_000_000, 600_000_000], limits, &mut cancel),
        Err(ScheduleError::PlaneOutOfRange { index: 1 })
    ));
    assert!(matches!(
        build_slab_schedule(
            &heights,
            ScheduleLimits {
                max_layers: 10,
                ..limits
            },
            &mut cancel
        ),
        Err(ScheduleError::LayerLimit { limit: 10 })
    ));
}
