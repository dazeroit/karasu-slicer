//! Independent bit/integer oracles and defensive read-metadata tests, not forged public reports.
use super::*;
use crate::geometry::Point3;
use crate::mesh::TriangleMesh;
use crate::slicing::graph::{build_incidence_graph, GraphLimits};
use crate::slicing::normalize::{normalize_section, NormalizationLimits};
use crate::slicing::{section_mesh_z, SectionLimits, SectionTransform};

fn bit_oracle(value: f64) -> [u32; WORDS] {
    let bits = value.to_bits();
    let exponent = usize::try_from((bits >> 52) & 2047).unwrap();
    let mantissa = (bits & ((1 << 52) - 1)) | if exponent == 0 { 0 } else { 1 << 52 };
    let mut result = [0; WORDS];
    for bit in 0..53 {
        if mantissa & (1 << bit) != 0 {
            let position = bit + exponent.saturating_sub(1);
            result[position / 32] |= 1 << (position % 32);
        }
    }
    result
}
fn step(k: i16) -> f64 {
    if k >= -1022 {
        f64::from_bits(u64::try_from(i32::from(k) + 1023).unwrap() << 52)
    } else {
        f64::from_bits(1 << u32::try_from(i32::from(k) + 1074).unwrap())
    }
}

#[test]
fn decoding_all_finite_exponent_fields_matches_individual_bit_oracle() {
    let mut words = [u32::MAX; WORDS];
    for exponent in 0..2047_u64 {
        for fraction in [0, 1, (1 << 51) - 1, 1 << 51, (1 << 52) - 1] {
            for sign in [0, 1_u64 << 63] {
                let value = f64::from_bits(sign | (exponent << 52) | fraction);
                let negative = Scratch::decode(value, &mut words).unwrap();
                assert_eq!(words, bit_oracle(value));
                assert_eq!(negative, value.is_sign_negative() && value != 0.0);
            }
        }
    }
}

#[test]
fn small_signed_origin_differences_match_integer_divisibility_oracle() {
    let mut scratch = Scratch::new();
    for x in -25..=25_i32 {
        for origin in -25..=25_i32 {
            for k in -3..=3_i16 {
                let difference = i64::from(x) - i64::from(origin);
                let expected = if k <= 0 {
                    Ok(difference << u32::try_from(-k).unwrap())
                } else {
                    let divisor = 1_i64 << u32::try_from(k).unwrap();
                    if difference % divisor == 0 {
                        Ok(difference / divisor)
                    } else {
                        Err(AxisFailure::OffGrid)
                    }
                };
                assert_eq!(
                    scratch.convert(f64::from(x), f64::from(origin), k),
                    expected
                );
            }
        }
    }
}

#[test]
fn exact_integer_boundaries_work_across_every_supported_step() {
    let mut scratch = Scratch::new();
    for k in -1074..=1023_i16 {
        for q in [
            -BOUND - 1,
            -BOUND,
            -BOUND + 1,
            -1,
            0,
            1,
            BOUND - 1,
            BOUND,
            BOUND + 1,
        ] {
            // These integers have <=53 significant bits. Binary scaling is exact whenever finite
            // and cannot underflow below eta; the expected quotient is the independent integer q.
            let value = q.to_string().parse::<f64>().unwrap() * step(k);
            if !value.is_finite() {
                continue;
            }
            let expected = if q.unsigned_abs() > (1 << 52) {
                Err(AxisFailure::OutOfRange)
            } else {
                Ok(q)
            };
            assert_eq!(scratch.convert(value, 0., k), expected, "k={k}, q={q}");
        }
    }
}

#[test]
fn half_steps_and_their_represented_neighbors_are_not_rounded() {
    let mut scratch = Scratch::new();
    for k in -1073..=1023_i16 {
        let midpoint = step(k - 1);
        for bits in [
            midpoint.to_bits() - 1,
            midpoint.to_bits(),
            midpoint.to_bits() + 1,
        ] {
            let value = f64::from_bits(bits);
            for sign in [-1., 1.] {
                let expected = if value == 0. {
                    Ok(0)
                } else if value.to_bits() == step(k).to_bits() {
                    Ok(if sign < 0. { -1 } else { 1 })
                } else {
                    Err(AxisFailure::OffGrid)
                };
                assert_eq!(scratch.convert(sign * value, 0., k), expected);
            }
        }
    }
}

#[test]
fn extremes_cancellation_residual_precedence_and_reuse_keep_exact_semantics() {
    let mut scratch = Scratch::new();
    let eta = f64::from_bits(1);
    assert_eq!(
        scratch.convert(f64::MAX, -f64::MAX, -1074),
        Err(AxisFailure::OutOfRange)
    );
    assert_eq!(
        scratch.convert(f64::MAX, -f64::MAX, 1023),
        Err(AxisFailure::OffGrid)
    );
    assert_eq!(scratch.convert(f64::MAX, eta, 0), Err(AxisFailure::OffGrid));
    assert_eq!(
        scratch.convert(3., 2_f64.powi(-53), 1),
        Err(AxisFailure::OffGrid)
    );
    let previous = f64::from_bits(f64::MAX.to_bits() - 1);
    assert_eq!(scratch.convert(f64::MAX, previous, 971), Ok(1));
    assert_eq!(scratch.convert(previous, f64::MAX, 971), Ok(-1));
    for k in [-1074, 0, 1023] {
        assert_eq!(scratch.convert(-f64::MAX, -f64::MAX, k), Ok(0));
        assert_eq!(scratch.convert(-0., 0., k), Ok(0));
    }
}

#[test]
fn fixed_capacity_guards_and_long_carry_borrow_are_explicit() {
    let mut scratch = Scratch::new();
    scratch.left.fill(u32::MAX);
    scratch.right[0] = 1;
    assert_eq!(scratch.difference(false, true), Err(AxisFailure::Overflow));
    scratch.left.fill(0);
    scratch.right.fill(0);
    scratch.left[65] = 1;
    scratch.right[0] = 1;
    assert_eq!(scratch.difference(false, false), Ok(false));
    assert!(scratch.left[..65].iter().all(|&word| word == u32::MAX));
    assert_eq!(scratch.left[65], 0);
    scratch.left.fill(0);
    assert_eq!(scratch.difference(false, false), Ok(true));
    assert_eq!(scratch.left[0], 1);
    assert_eq!(scratch.quotient(2098, false), Err(AxisFailure::Overflow));
    assert_eq!(scratch.convert(1., 0., -1075), Err(AxisFailure::Overflow));
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(scratch.convert(value, 0., 0), Err(AxisFailure::Overflow));
    }
}

// Signed binary digits form an independent base-2 subtraction oracle, unlike the production
// sign/magnitude u32 carry/borrow schedule. No production decoder supplies expected words.
fn difference_oracle(x: f64, origin: f64) -> (bool, [u32; WORDS]) {
    let negative = x.partial_cmp(&origin) == Some(Ordering::Less);
    let (a, b) = if negative { (origin, x) } else { (x, origin) };
    let a_words = bit_oracle(a);
    let b_words = bit_oracle(b);
    let sign_a = if a.is_sign_negative() { -1_i8 } else { 1 };
    let sign_b = if b.is_sign_negative() { -1_i8 } else { 1 };
    let mut expected = [0; WORDS];
    let mut carry = 0_i8;
    for bit in 0..WORDS * 32 {
        let a_bit = i8::try_from((a_words[bit / 32] >> (bit % 32)) & 1).unwrap();
        let b_bit = i8::try_from((b_words[bit / 32] >> (bit % 32)) & 1).unwrap();
        let digit = sign_a * a_bit - sign_b * b_bit + carry;
        if digit.rem_euclid(2) != 0 {
            expected[bit / 32] |= 1 << (bit % 32);
        }
        carry = digit.div_euclid(2);
    }
    assert_eq!(carry, 0);
    (negative, expected)
}

#[test]
fn signed_wide_differences_match_independent_binary_digit_arithmetic() {
    let mut scratch = Scratch::new();
    for e in (0..2047_u64).step_by(7).chain([2046]) {
        for f in [0, 1, (1_u64 << 52) - 1] {
            let positive = f64::from_bits((e << 52) | f);
            let other = f64::from_bits(((2046 - e) << 52) | f);
            for x in [positive, -positive] {
                for origin in [f64::MAX, -f64::MAX, other, -other, f64::from_bits(1)] {
                    let (negative, expected) = difference_oracle(x, origin);
                    let sx = Scratch::decode(x, &mut scratch.left).unwrap();
                    let so = Scratch::decode(origin, &mut scratch.right).unwrap();
                    assert_eq!(scratch.difference(sx, so).unwrap(), negative);
                    assert_eq!(scratch.left, expected);
                }
            }
        }
    }
}

fn metadata() -> (Vec<GraphVertex>, Vec<FeatureGroup>, usize) {
    let mesh = TriangleMesh::try_new(
        vec![
            Point3::new(0., 0., 0.),
            Point3::new(2., 0., 0.),
            Point3::new(0., 2., 0.),
        ],
        vec![[0, 1, 2]],
    )
    .unwrap();
    let raw = section_mesh_z(
        &mesh,
        0.,
        SectionTransform::Identity,
        SectionLimits::default(),
    )
    .unwrap();
    let normalized = normalize_section(&raw, NormalizationLimits::default()).unwrap();
    let graph = build_incidence_graph(&normalized, GraphLimits::default()).unwrap();
    (
        graph.vertices().to_vec(),
        normalized.features().to_vec(),
        normalized.feature_order().len(),
    )
}
fn check_metadata(
    vertices: &[GraphVertex],
    features: &[FeatureGroup],
    n: usize,
    plane: f64,
) -> Result<GridSummary> {
    preflight(
        vertices,
        features,
        n,
        plane,
        GridLimits::default(),
        &mut Cancellation::never(),
    )
}

#[test]
fn metadata_slice_checks_are_contextual_without_forging_an_inspection() {
    let (vertices, features, n) = metadata();
    assert_eq!(
        check_metadata(&vertices, &features[..2], n, 0.),
        Err(invalid(None, GridInputIssue::FeatureCount))
    );
    assert_eq!(
        check_metadata(&vertices, &features, n, f64::NAN),
        Err(invalid(None, GridInputIssue::Plane))
    );
    let mut bad_vertices = vertices.clone();
    bad_vertices[1].feature_group = 0;
    assert_eq!(
        check_metadata(&bad_vertices, &features, n, 0.),
        Err(invalid(Some(1), GridInputIssue::Association))
    );
    let mut bad = features.clone();
    bad[1].feature = bad[0].feature;
    assert_eq!(
        check_metadata(&vertices, &bad, n, 0.),
        Err(invalid(Some(1), GridInputIssue::FeatureOrder))
    );
    for range in [0..0, std::ops::Range { start: 1, end: 0 }, 0..n + 1] {
        let mut bad = features.clone();
        bad[0].observations = range;
        assert_eq!(
            check_metadata(&vertices, &bad, n, 0.),
            Err(invalid(Some(0), GridInputIssue::ObservationRange))
        );
    }
    for point in [
        Point3::new(f64::NAN, 0., 0.),
        Point3::new(0., f64::INFINITY, 0.),
        Point3::new(0., 0., f64::NEG_INFINITY),
        Point3::new(0., 0., 1.),
    ] {
        let mut bad = features.clone();
        bad[0].coordinates = CoordinateAgreement::Agreed(point);
        assert_eq!(
            check_metadata(&vertices, &bad, n, 0.),
            Err(invalid(Some(0), GridInputIssue::Coordinates))
        );
    }
}

#[test]
fn metadata_conflicts_consume_vertices_and_bytes_but_no_axis_work() {
    let (vertices, mut features, n) = metadata();
    for feature in &mut features {
        feature.coordinates = CoordinateAgreement::Conflict;
    }
    let limits = GridLimits {
        max_axis_conversions: 0,
        ..GridLimits::default()
    };
    let summary = preflight(
        &vertices,
        &features,
        n,
        0.,
        limits,
        &mut Cancellation::never(),
    )
    .unwrap();
    assert_eq!(summary.exact_vertices, 0);
    assert_eq!(summary.unresolved_vertices, 3);
    assert_eq!(summary.axis_conversions, 0);
    assert_eq!(summary.requested_payload_bytes, payload(3).unwrap());
    assert_eq!(
        preflight(
            &vertices,
            &[],
            n,
            f64::NAN,
            GridLimits {
                max_vertices: 0,
                ..limits
            },
            &mut Cancellation::never()
        ),
        Err(GridError::LimitExceeded {
            resource: GridResource::Vertices,
            limit: 0
        })
    );
}

#[test]
fn count_capacity_and_preflight_error_order_are_checked() {
    assert_eq!(payload(usize::MAX), Err(GridError::SizeOverflow));
    let beyond = usize::try_from(isize::MAX).unwrap() / size_of::<GridVertexOutcome>() + 1;
    assert_eq!(payload(beyond), Err(GridError::SizeOverflow));
    let (vertices, features, n) = metadata();
    let limits = GridLimits {
        max_axis_conversions: 5,
        max_payload_bytes: 0,
        ..GridLimits::default()
    };
    assert_eq!(
        preflight(
            &vertices,
            &features,
            n,
            0.,
            limits,
            &mut Cancellation::never()
        ),
        Err(GridError::LimitExceeded {
            resource: GridResource::AxisConversions,
            limit: 5
        })
    );
    assert_eq!(
        preflight(
            &[],
            &[],
            0,
            f64::INFINITY,
            GridLimits::default(),
            &mut Cancellation::never()
        ),
        Err(invalid(None, GridInputIssue::Plane))
    );
}

// Only raw, privately owned test records are perturbed. Normalization/graph/paths/inspection
// are built normally; no public inspection constructor or production visibility change is needed.
fn conflicting_tangents(count: u32, every_second: bool) -> crate::slicing::MeshSection {
    use crate::geometry::section::TrianglePlaneEvent;
    let mut points = Vec::new();
    let mut triangles = Vec::new();
    for id in 0..count {
        let x = f64::from(id);
        points.extend([
            Point3::new(x, 0., 0.),
            Point3::new(x + 0.25, 0., 1.),
            Point3::new(x, 0.25, 1.),
        ]);
        triangles.extend([[3 * id, 3 * id + 1, 3 * id + 2]; 2]);
    }
    let mesh = TriangleMesh::try_new(points, triangles).unwrap();
    let mut raw = section_mesh_z(
        &mesh,
        0.,
        SectionTransform::Identity,
        SectionLimits::default(),
    )
    .unwrap();
    for (id, pair) in raw.events.as_chunks_mut::<2>().0.iter_mut().enumerate() {
        if every_second && id % 2 == 1 {
            continue;
        }
        let TrianglePlaneEvent::TangentPoint { endpoint } = &mut pair[1].intersection.event else {
            panic!("tangent");
        };
        endpoint.position.x += 0.5;
    }
    raw
}

#[test]
fn all_conflict_public_views_keep_observations_and_cancel_across_conflict_only_stretches() {
    use crate::slicing::paths::{extract_paths, PathLimits};
    use crate::slicing::validity::{inspect_planar_paths, ValidityLimits};
    let raw = conflicting_tangents(513, false);
    let before = format!("{raw:?}");
    let normalized = normalize_section(&raw, NormalizationLimits::default()).unwrap();
    let graph = build_incidence_graph(&normalized, GraphLimits::default()).unwrap();
    let paths = extract_paths(&graph, PathLimits::default()).unwrap();
    let inspection = inspect_planar_paths(&paths, ValidityLimits::default()).unwrap();
    let metadata = BinaryGridSpec::try_new([0., 0.], 0).unwrap();
    let limits = GridLimits {
        max_axis_conversions: 0,
        ..GridLimits::default()
    };
    let mut trace = Vec::new();
    let mut predicate = |phase| {
        trace.push(phase);
        false
    };
    let base = map_exact_grid_vertices_with_cancellation(
        &inspection,
        metadata,
        limits,
        &mut Cancellation::new(&mut predicate),
    )
    .unwrap();
    assert_eq!(
        base.outcomes(),
        vec![GridVertexOutcome::UnresolvedCoordinates; 513]
    );
    assert_eq!(base.summary().unresolved_vertices, 513);
    assert_eq!(base.summary().axis_conversions, 0);
    assert_eq!(normalized.observations().len(), 1026);
    assert!(std::ptr::eq(
        base.inspection().paths().graph().normalized().raw(),
        std::ptr::from_ref(&raw)
    ));
    assert_eq!(
        trace,
        [
            Checkpoint::GridStart,
            Checkpoint::GridInput,
            Checkpoint::GridInput,
            Checkpoint::GridInput,
            Checkpoint::GridStorage,
            Checkpoint::GridStorage,
            Checkpoint::GridRecords,
            Checkpoint::GridRecords,
            Checkpoint::GridRecords,
            Checkpoint::GridComplete
        ]
    );
    for (target, &checkpoint) in trace.iter().enumerate() {
        let mut visits = 0;
        let mut predicate = |_| {
            let stop = visits == target;
            visits += 1;
            stop
        };
        assert_eq!(
            map_exact_grid_vertices_with_cancellation(
                &inspection,
                metadata,
                limits,
                &mut Cancellation::new(&mut predicate)
            )
            .unwrap_err(),
            GridError::Cancelled(Cancelled { checkpoint })
        );
        assert_eq!(
            map_exact_grid_vertices(&inspection, metadata, limits).unwrap(),
            base
        );
        assert_eq!(format!("{raw:?}"), before);
    }
}

#[test]
fn mixed_conflicts_and_agreed_features_have_one_outcome_and_only_agreed_axis_work() {
    use crate::slicing::paths::{extract_paths, PathLimits};
    use crate::slicing::validity::{inspect_planar_paths, ValidityLimits};
    let raw = conflicting_tangents(3, true);
    let normalized = normalize_section(&raw, NormalizationLimits::default()).unwrap();
    let graph = build_incidence_graph(&normalized, GraphLimits::default()).unwrap();
    let paths = extract_paths(&graph, PathLimits::default()).unwrap();
    let inspection = inspect_planar_paths(&paths, ValidityLimits::default()).unwrap();
    let view = map_exact_grid_vertices(
        &inspection,
        BinaryGridSpec::try_new([0., 0.], 0).unwrap(),
        GridLimits::default(),
    )
    .unwrap();
    assert_eq!(
        view.outcomes(),
        &[
            GridVertexOutcome::UnresolvedCoordinates,
            GridVertexOutcome::Exact { xy: [1, 0] },
            GridVertexOutcome::UnresolvedCoordinates
        ]
    );
    assert_eq!(view.summary().vertices, 3);
    assert_eq!(view.summary().exact_vertices, 1);
    assert_eq!(view.summary().unresolved_vertices, 2);
    assert_eq!(view.summary().axis_conversions, 2);
    assert_eq!(normalized.observations().len(), 6);
}
