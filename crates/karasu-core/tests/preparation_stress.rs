//! Bounded multiple-support geometry, independent occupancy and resource regressions.
#[allow(dead_code)]
#[path = "../examples/preparation_stress.rs"]
mod fixture;
use karasu_core::preparation::supports::{SupportError, SupportLimits};

#[test]
fn eight_separated_supports_preserve_component_topology_and_independent_pixels() {
    let model = fixture::evaluate(SupportLimits::default()).unwrap();
    assert_eq!(model.certificates().len(), 8);
    assert_eq!(model.components().len(), 24);
    for component in model.components() {
        let topology = component.mesh().analyze_topology().unwrap();
        assert!(topology.summary().closed_combinatorial_surface);
        assert_eq!(topology.summary().edge_components, 1);
    }
    let pixels = fixture::pixels(&model, 0.5).unwrap();
    let mut count = 0;
    for y in 0_u32..200 {
        for x in 0_u32..200 {
            let point = [(f64::from(x) + 0.5) / 10., (f64::from(y) + 0.5) / 10.];
            let expected = [3., 9.].into_iter().any(|cy| {
                [9., 12., 15., 18.]
                    .into_iter()
                    .any(|cx| (point[0] - cx).abs() < 1. && (point[1] - cy).abs() < 1.)
            });
            let actual = pixels[usize::try_from(y * 200 + x).unwrap()] != 0;
            assert_eq!(actual, expected, "base pixel {x},{y}");
            count += usize::from(actual);
        }
    }
    assert_eq!(count, 3200);
    let pixels = fixture::pixels(&model, 11.).unwrap();
    let mut count = 0;
    for y in 0_u32..200 {
        for x in 0_u32..200 {
            let point = [(f64::from(x) + 0.5) / 10., (f64::from(y) + 0.5) / 10.];
            let distance = ((point[0] - 5.).powi(2) + (point[1] - 5.).powi(2)).sqrt();
            let actual = pixels[usize::try_from(y * 200 + x).unwrap()] != 0;
            // Only the documented cutter radial approximation band is unconstrained.
            if distance < 0.999 {
                assert!(!actual);
            }
            if distance > 1.001 {
                assert!(actual);
            }
            count += usize::from(actual);
        }
    }
    assert_eq!(count, 39684);
}
#[test]
fn eight_support_plan_admission_limits_have_exact_boundaries() {
    let admitted = SupportLimits {
        max_supports: 8,
        max_stations: 4,
        max_profile_vertices: 4,
        ..SupportLimits::default()
    };
    fixture::plan(admitted).unwrap();
    for limits in [
        SupportLimits {
            max_supports: 7,
            ..admitted
        },
        SupportLimits {
            max_stations: 3,
            ..admitted
        },
        SupportLimits {
            max_profile_vertices: 3,
            ..admitted
        },
    ] {
        assert!(matches!(
            fixture::plan(limits),
            Err(SupportError::Resource { .. })
        ));
    }
}
