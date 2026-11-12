//! Independent integer/bit oracles for exact arithmetic and scalar conversion.
use super::*;

fn encoded(q: u64, shift: usize) -> Accumulator {
    let mut out = Accumulator::zero();
    for bit in 0..64 {
        if q & (1_u64 << bit) != 0 {
            set(&mut out, bit + shift);
        }
    }
    out
}
fn set(out: &mut Accumulator, bit: usize) {
    out.words[bit / 32] |= 1 << (bit % 32);
}
fn expected(bits: u64) -> AreaMagnitude {
    if bits == 0 {
        AreaMagnitude::RoundedToZero
    } else if bits == 0x7ff0_0000_0000_0000 {
        AreaMagnitude::Overflow
    } else {
        AreaMagnitude::Finite(f64::from_bits(bits))
    }
}

#[test]
fn rounder_matches_adjacent_binary64_midpoints_in_every_normal_binade() {
    for exponent in 1..=2046_u64 {
        for fraction in [0, 1, 2, (1_u64 << 52) - 2, (1_u64 << 52) - 1] {
            let bits = (exponent << 52) | fraction;
            let q = (1_u64 << 52) | fraction;
            let shift = usize::try_from(exponent).unwrap() + 1074;
            assert_eq!(encoded(q, shift).magnitude(), expected(bits));
            for delta in [-1, 0, 1] {
                let mut midpoint = encoded(q, shift);
                if delta == -1 {
                    for bit in 0..shift - 1 {
                        set(&mut midpoint, bit);
                    }
                } else {
                    set(&mut midpoint, shift - 1);
                    if delta == 1 {
                        set(&mut midpoint, 0);
                    }
                }
                let up = delta == 1 || (delta == 0 && q & 1 != 0);
                assert_eq!(
                    midpoint.magnitude(),
                    expected(bits + u64::from(up)),
                    "e={exponent} f={fraction} d={delta}"
                );
            }
        }
    }
}

#[test]
fn subnormal_zero_and_minimum_normal_ties_have_explicit_outcomes() {
    for q in [0, 1, 2, (1_u64 << 52) - 2, (1_u64 << 52) - 1] {
        for delta in [-1, 0, 1] {
            let mut value = encoded(q, 1075);
            if delta == -1 {
                for bit in 0..1074 {
                    set(&mut value, bit);
                }
            } else {
                set(&mut value, 1074);
                if delta == 1 {
                    set(&mut value, 0);
                }
            }
            let up = delta == 1 || (delta == 0 && q & 1 != 0);
            assert_eq!(value.magnitude(), expected(q + u64::from(up)));
        }
    }
    assert_eq!(encoded(1, 0).magnitude(), AreaMagnitude::RoundedToZero);
    assert_eq!(encoded(1, 4000).magnitude(), AreaMagnitude::Overflow);
}

// Schoolbook bit convolution, deliberately unlike the native mantissa product/shift implementation.
fn product_oracle(a: f64, b: f64) -> [u32; WORDS] {
    let positions = |value: f64| {
        let bits = value.to_bits();
        let e = usize::try_from((bits >> 52) & 2047).unwrap();
        let q = (bits & ((1 << 52) - 1)) | if e == 0 { 0 } else { 1 << 52 };
        (0..53)
            .filter(|&i| q & (1 << i) != 0)
            .map(|i| i + e.saturating_sub(1))
            .collect::<Vec<_>>()
    };
    let mut bits = vec![0_u32; WORDS * 32];
    for i in positions(a) {
        for j in positions(b) {
            bits[i + j] += 1;
        }
    }
    for i in 0..bits.len() - 1 {
        bits[i + 1] += bits[i] / 2;
        bits[i] %= 2;
    }
    let mut words = [0; WORDS];
    for (i, &bit) in bits.iter().enumerate() {
        words[i / 32] |= bit << (i % 32);
    }
    words
}

#[test]
fn mantissa_products_match_bit_convolution_at_all_alignment_residues_and_extremes() {
    let fractions = [0, 1, (1_u64 << 52) - 1];
    for exponent in (1..=32_u64).chain([0, 1023, 2045, 2046]) {
        for fraction in fractions {
            let a = f64::from_bits((exponent << 52) | fraction);
            for b in [f64::from_bits(1), 1.0, f64::MAX] {
                for negative in [false, true] {
                    let a = if negative { -a } else { a };
                    let mut sum = Accumulator::zero();
                    sum.product(a, b, false).unwrap();
                    assert_eq!(sum.words, product_oracle(a, b));
                    assert_eq!(sum.negative, negative && a != 0.0);
                    sum.product(a, b, true).unwrap();
                    assert_eq!(sum.sign(), Ordering::Equal);
                    assert!(!sum.negative);
                }
            }
        }
    }
}

#[test]
fn carries_borrows_sign_changes_and_overflow_are_not_truncated() {
    let mut one = encoded(1, 0);
    let mut below = encoded(1, 4200);
    one.negative = true;
    below.combine(&one).unwrap();
    assert_eq!(below.words[..131], [u32::MAX; 131]);
    assert_eq!(below.words[131], 255);
    one.negative = false;
    below.combine(&one).unwrap();
    assert_eq!(below.words, encoded(1, 4200).words);
    let mut negative = encoded(2, 0);
    negative.negative = true;
    let mut result = encoded(1, 0);
    result.combine(&negative).unwrap();
    assert_eq!(result.sign(), Ordering::Less);
    assert_eq!(result.words, encoded(1, 0).words);
    let mut full = Accumulator {
        words: [u32::MAX; WORDS],
        negative: false,
    };
    assert_eq!(full.combine(&one), Err(AreaError::SizeOverflow));
    assert_eq!(add(usize::MAX, 1), Err(AreaError::SizeOverflow));
    assert_eq!(payload(usize::MAX), Err(AreaError::SizeOverflow));
    assert_eq!(payload(0), Ok(0));
}

fn polygon(points: &[[f64; 2]]) -> Accumulator {
    let mut sum = Accumulator::zero();
    for (a, b) in points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .take(points.len())
    {
        sum.product(a[0], b[1], false).unwrap();
        sum.product(a[1], b[0], true).unwrap();
    }
    sum
}

#[test]
fn exact_integer_polygon_oracle_reversal_rotation_and_lossless_maps() {
    for seed in 0..512_i32 {
        let points = [
            [seed % 17 - 8, seed % 13 - 6],
            [seed % 7, seed % 11],
            [seed % 19 - 9, seed % 23 - 11],
        ];
        let [a, b, c] = points.map(|p| p.map(i64::from));
        let determinant = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
        let base = points.map(|p| p.map(f64::from));
        for scale in [0.125, 1., 8.] {
            let mapped = base.map(|[x, y]| [x * scale + 16., y * scale - 4.]);
            let sum = polygon(&mapped);
            assert_eq!(sum.sign(), determinant.cmp(&0));
            let area = f64::from(u32::try_from(determinant.abs()).unwrap()) * scale * scale / 2.;
            assert_eq!(
                sum.magnitude(),
                if determinant == 0 {
                    AreaMagnitude::RoundedToZero
                } else {
                    AreaMagnitude::Finite(area)
                }
            );
            assert_eq!(polygon(&[mapped[1], mapped[2], mapped[0]]).words, sum.words);
            let reverse = polygon(&[mapped[2], mapped[1], mapped[0]]);
            assert_eq!(reverse.sign(), sum.sign().reverse());
            assert_eq!(reverse.magnitude(), sum.magnitude());
        }
    }
}

#[test]
fn cancellation_sensitive_products_extreme_sums_and_zero_invariant() {
    let d = f64::EPSILON;
    let points = [[0., 0.], [1., 1. + d], [1. - d, 1.]];
    let sum = polygon(&points);
    assert_eq!(sum.sign(), Ordering::Greater);
    assert_eq!(sum.magnitude(), AreaMagnitude::Finite(2.0_f64.powi(-105)));
    for scale in [2.0_f64.powi(-500), 2.0_f64.powi(500)] {
        assert_eq!(
            polygon(&points.map(|p| p.map(|x| x * scale))).sign(),
            Ordering::Greater
        );
    }
    let shifted = [[0., 0.], [1., 0.], [1., 1.], [0., 1.]].map(|p| p.map(|x| x + 2.0_f64.powi(51)));
    assert_eq!(polygon(&shifted).magnitude(), AreaMagnitude::Finite(1.));
    let mut sum = Accumulator::zero();
    for _ in 0..512 {
        sum.product(f64::MAX, f64::MAX, false).unwrap();
    }
    assert_eq!(sum.sign(), Ordering::Greater);
    for _ in 0..512 {
        sum.product(f64::MAX, f64::MAX, true).unwrap();
    }
    assert_eq!(sum.sign(), Ordering::Equal);
    assert_eq!(
        cycle_orientation(&sum, 7),
        Err(invalid(Some(7), None, AreaInputIssue::ZeroAreaSimpleCycle))
    );
    // The count-domain proof is mathematical; these assertions check its width arithmetic.
    for n in [1_u64, 3_000_000, 1 << 27, u64::from(u32::MAX), u64::MAX] {
        let ceil_log = 64 - (n - 1).leading_zeros();
        assert!(4197 + usize::try_from(ceil_log).unwrap() <= WORDS * 32);
    }
}

#[test]
fn defensive_coordinate_and_eligibility_failures_keep_structured_context() {
    use crate::mesh::TriangleMesh;
    use crate::slicing::graph::{build_incidence_graph, GraphLimits};
    use crate::slicing::normalize::{normalize_section, NormalizationLimits};
    use crate::slicing::paths::{extract_paths, PathLimits};
    use crate::slicing::validity::{inspect_planar_paths, ValidityLimits};
    use crate::slicing::{section_mesh_z, SectionLimits, SectionTransform};
    for coordinates in [
        CoordinateAgreement::Conflict,
        CoordinateAgreement::Agreed(Point3::new(f64::INFINITY, 0., 0.)),
        CoordinateAgreement::Agreed(Point3::new(0., f64::NAN, 0.)),
        CoordinateAgreement::Agreed(Point3::new(0., 0., f64::NEG_INFINITY)),
    ] {
        assert_eq!(
            agreed_point(coordinates, 4, 9),
            Err(invalid(Some(4), Some(9), AreaInputIssue::Coordinates))
        );
    }
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
    let paths = extract_paths(&graph, PathLimits::default()).unwrap();
    let input = inspect_planar_paths(&paths, ValidityLimits::default()).unwrap();
    assert_eq!(
        point(&input, 0, usize::MAX),
        Err(invalid(
            Some(0),
            Some(usize::MAX),
            AreaInputIssue::Association
        ))
    );
    let original = &paths.paths()[0];
    let embedding = input.embeddings()[0];
    for e in [
        PathEmbedding {
            status: EmbeddingStatus::Ineligible,
            ..embedding
        },
        PathEmbedding {
            collapsed_edges: 1,
            ..embedding
        },
        PathEmbedding {
            status: EmbeddingStatus::NonSimpleCycle,
            ..embedding
        },
    ] {
        assert_eq!(
            eligibility(original, e, 0, 3, 3),
            Err(invalid(Some(0), None, AreaInputIssue::Eligibility))
        );
    }
    for (vertices, edges) in [(2, 3), (3, 0), (2, 2)] {
        assert_eq!(
            eligibility(original, embedding, 0, vertices, edges),
            Err(invalid(Some(0), None, AreaInputIssue::CycleShape))
        );
    }
    let mut broken = original.clone();
    broken.closed = false;
    assert_eq!(
        eligibility(&broken, embedding, 0, 4, 3),
        Err(invalid(Some(0), None, AreaInputIssue::CycleShape))
    );
}
