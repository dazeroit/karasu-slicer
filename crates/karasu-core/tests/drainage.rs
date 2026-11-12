//! Public drainage regression checks using analytic sections and independent occupancy.
#![allow(clippy::cast_precision_loss)] // Test-only approximate decoding for independent area checks.
use karasu_core::{
    cancellation::{Cancellation, Checkpoint},
    geometry::{ObjectTransform, Point3},
    mesh::TriangleMesh,
    preparation::{
        drainage::*, HoleSpec, HollowSpec, Operation, OperationKind, PlanLimits, PreparationPlan,
        SamplingSpec, SourceBinding,
    },
    slicing::{
        construct::{ConstructLimits, ConstructedMaterial, ExactPoint},
        engine::{LayerConfig, LayerStageLimits},
        grid::BinaryGridSpec,
        regions::RegionFillRule,
        schedule::*,
    },
};
fn cube(side: f64) -> TriangleMesh {
    let points = [
        [0., 0., 0.],
        [side, 0., 0.],
        [side, side, 0.],
        [0., side, 0.],
        [0., 0., side],
        [side, 0., side],
        [side, side, side],
        [0., side, side],
    ];
    TriangleMesh::try_new(
        points.map(|[x, y, z]| Point3::new(x, y, z)).to_vec(),
        vec![
            [0, 2, 1],
            [0, 3, 2],
            [4, 5, 6],
            [4, 6, 7],
            [0, 1, 5],
            [0, 5, 4],
            [1, 2, 6],
            [1, 6, 5],
            [2, 3, 7],
            [2, 7, 6],
            [3, 0, 4],
            [3, 4, 7],
        ],
    )
    .unwrap()
}

fn cut() -> HoleSpec {
    HoleSpec {
        entry_mm: [10., 10., -1.],
        direction: [0., 0., 1.],
        length_mm: 4.,
        diameter_mm: 2.,
    }
}
fn plan(cuts: &[HoleSpec]) -> PreparationPlan {
    let mut operations = vec![Operation {
        id: 1,
        kind: OperationKind::Hollow(HollowSpec {
            blockers: Vec::new(),
            thickness_mm: 2.,
            guarantee: None,
        }),
    }];
    operations.extend(cuts.iter().enumerate().map(|(i, &s)| Operation {
        id: u64::try_from(i).unwrap() + 2,
        kind: OperationKind::Hole(s),
    }));
    PreparationPlan::new(
        SourceBinding {
            original_sha256: [61; 32],
            placement: ObjectTransform::identity(),
        },
        SamplingSpec {
            cell_mm: 0.1,
            distance_error_mm: 0.001,
            max_cells: 1,
        },
        operations,
        PlanLimits::default(),
        &mut Cancellation::never(),
    )
    .unwrap()
}
fn run(cuts: &[HoleSpec]) -> Result<DrainedModel, DrainageError> {
    drain_model(
        &cube(20.),
        &plan(cuts),
        DrainageLimits::default(),
        &mut Cancellation::never(),
    )
}
fn with_section<T>(model: &DrainedModel, z: f64, f: impl FnOnce(&ConstructedMaterial) -> T) -> T {
    let schedule = build_layer_schedule(
        UniformScheduleSpec::try_new(z, 1., ScheduleEnd::Count(1)).unwrap(),
        ScheduleLimits {
            max_layers: 1,
            max_payload_bytes: 1024,
        },
    )
    .unwrap();
    let config = LayerConfig {
        grid: BinaryGridSpec::try_new([0., 0.], 0).unwrap(),
        fill_rule: RegionFillRule::EvenOddStrict,
        stages: LayerStageLimits::default(),
        max_output_payload_bytes: 64 * 1024 * 1024,
    };
    let mut engine = model.layer_engine(
        &schedule,
        config,
        ConstructLimits::default(),
        Cancellation::never(),
        None,
    );
    let layer = engine.next().unwrap().unwrap();
    assert!(engine.next().is_none());
    f(layer.material_union().unwrap())
}
fn section_area(model: &DrainedModel, z: f64) -> f64 {
    with_section(model, z, area)
}
fn magnitude(words: [u64; 8]) -> f64 {
    words
        .iter()
        .rev()
        .fold(0., |n, &word| n * 18_446_744_073_709_551_616. + word as f64)
}
fn xy(p: ExactPoint, exp: i16) -> [f64; 2] {
    let den = magnitude(p.denominator());
    [p.numerator_x(), p.numerator_y()].map(|(negative, words)| {
        let x = magnitude(words) / den * 2_f64.powi(i32::from(exp));
        if negative {
            -x
        } else {
            x
        }
    })
}
fn area(m: &ConstructedMaterial) -> f64 {
    let exp = m.dyadic_exponent_mm().unwrap();
    m.edges()
        .iter()
        .map(|e| {
            let a = xy(m.vertices()[e.endpoints[0]], exp);
            let b = xy(m.vertices()[e.endpoints[1]], exp);
            (a[0] * b[1] - a[1] * b[0]) * 0.5
        })
        .sum()
}
fn contains(m: &ConstructedMaterial, q: [f64; 2]) -> bool {
    let exp = m.dyadic_exponent_mm().unwrap();
    let mut result = false;
    for edge in m.edges() {
        let a = xy(m.vertices()[edge.endpoints[0]], exp);
        let b = xy(m.vertices()[edge.endpoints[1]], exp);
        if (a[1] > q[1]) != (b[1] > q[1])
            && q[0] < (b[0] - a[0]) * (q[1] - a[1]) / (b[1] - a[1]) + a[0]
        {
            result = !result;
        }
    }
    result
}
#[test]
fn bottom_wall_section_has_actual_opening_and_analytic_polygon_area() {
    let model = run(&[cut()]).unwrap();
    with_section(&model, 1., |layer| {
        let n = model.openings()[0].segments as f64;
        let polygon = n * 0.5 * (std::f64::consts::TAU / n).sin();
        assert!((area(layer) - (400. - polygon)).abs() < 1e-8);
        assert!(!contains(layer, [10., 10.]));
        assert!(contains(layer, [5., 5.]));
        assert_eq!(layer.rings().len(), 2);
    });
}
#[test]
fn cavity_section_and_cut_end_preserve_original_hollow_wall() {
    let model = run(&[cut()]).unwrap();
    for z in [2.5, 3., 3.5, 5.] {
        with_section(&model, z, |m| {
            assert!((area(m) - 144.).abs() < 1e-7);
            assert!(!contains(m, [10., 10.]));
            assert!(contains(m, [1., 10.]));
        });
    }
}
#[test]
fn source_and_immutable_plan_records_are_preserved() {
    let source = cube(20.);
    let vertices = source.vertices().to_vec();
    let triangles = source.triangles().to_vec();
    let p = plan(&[cut()]);
    let record = p.canonical_record().to_vec();
    let m = drain_model(
        &source,
        &p,
        DrainageLimits::default(),
        &mut Cancellation::never(),
    )
    .unwrap();
    assert_eq!(source.vertices(), vertices);
    assert_eq!(source.triangles(), triangles);
    assert_eq!(p.canonical_record(), record);
    assert_eq!(m.plan_record(), record);
    assert_eq!(m.enclosed_cavities(), 0);
    assert_eq!(m.openings()[0].operation_id, 2);
    assert!(m.openings()[0].radial_error_mm <= 0.001);
    assert!(m.openings()[0].terminal_clearance_mm > 0.9);
}
#[test]
fn zero_cuts_preserve_one_enclosed_cavity() {
    let m = run(&[]).unwrap();
    assert_eq!(m.enclosed_cavities(), 1);
    assert_eq!(m.openings(), []);
    assert!((section_area(&m, 1.) - 400.).abs() < 1e-9);
    assert!((section_area(&m, 3.) - 144.).abs() < 1e-9);
}
#[test]
fn miss_is_distinct_from_shallow_and_nonexterior_entry() {
    let mut s = cut();
    s.entry_mm[0] = 25.;
    assert!(matches!(
        run(&[s]),
        Err(DrainageError::Opening {
            issue: OpeningIssue::MissedModel,
            ..
        })
    ));
    s = cut();
    s.length_mm = 2.;
    assert!(matches!(
        run(&[s]),
        Err(DrainageError::Opening {
            issue: OpeningIssue::Shallow,
            ..
        })
    ));
    s = cut();
    s.entry_mm[2] = 1.;
    assert!(matches!(
        run(&[s]),
        Err(DrainageError::Opening {
            issue: OpeningIssue::EntryNotExterior,
            ..
        })
    ));
}
#[test]
fn oversized_terminal_cap_is_rejected_without_clamping() {
    let mut s = cut();
    s.diameter_mm = 18.;
    assert!(matches!(
        run(&[s]),
        Err(DrainageError::Opening {
            issue: OpeningIssue::InsufficientClearance,
            ..
        })
    ));
}
#[test]
fn oblique_cut_rejected_explicitly() {
    let mut s = cut();
    s.direction = [0.1, 0., 1.];
    assert!(matches!(run(&[s]), Err(DrainageError::Unsupported { .. })));
}
#[test]
fn overlap_is_rejected_but_separate_openings_are_certified() {
    let a = cut();
    let mut b = cut();
    b.entry_mm[0] += 0.5;
    assert!(matches!(
        run(&[a, b]),
        Err(DrainageError::PotentialOverlap {
            first: 2,
            second: 3
        })
    ));
    b.entry_mm[0] = 14.;
    let m = run(&[a, b]).unwrap();
    assert_eq!(m.openings().len(), 2);
    with_section(&m, 1., |layer| {
        assert_eq!(layer.rings().len(), 3);
        assert!(!contains(layer, [14., 10.]));
    });
}
#[test]
fn cancellation_at_all_public_phases_is_atomic() {
    let source = cube(20.);
    let p = plan(&[cut()]);
    for checkpoint in [
        Checkpoint::DrainageStart,
        Checkpoint::DrainageWork,
        Checkpoint::DrainagePublish,
    ] {
        let mut predicate = |at| at == checkpoint;
        assert!(matches!(
            drain_model(
                &source,
                &p,
                DrainageLimits::default(),
                &mut Cancellation::new(&mut predicate)
            ),
            Err(DrainageError::Cancelled(_))
        ));
    }
}
#[test]
fn independent_resource_limits_reject_before_publication() {
    for limits in [
        DrainageLimits {
            max_holes: 0,
            ..DrainageLimits::default()
        },
        DrainageLimits {
            max_segments: 8,
            ..DrainageLimits::default()
        },
        DrainageLimits {
            max_work: 0,
            ..DrainageLimits::default()
        },
        DrainageLimits {
            max_payload_bytes: 0,
            ..DrainageLimits::default()
        },
    ] {
        assert!(matches!(
            drain_model(
                &cube(20.),
                &plan(&[cut()]),
                limits,
                &mut Cancellation::never()
            ),
            Err(DrainageError::Resource { .. })
        ));
    }
}
#[test]
fn recomputation_after_edit_or_removal_retains_original_result_identity() {
    let p = plan(&[cut()]);
    let old = drain_model(
        &cube(20.),
        &p,
        DrainageLimits::default(),
        &mut Cancellation::never(),
    )
    .unwrap();
    let mut edited = cut();
    edited.diameter_mm = 3.;
    let changed = run(&[edited]).unwrap();
    let removed = run(&[]).unwrap();
    assert_ne!(old.plan_record(), changed.plan_record());
    assert_ne!(old.plan_record(), removed.plan_record());
    assert_eq!(old.plan_record(), p.canonical_record());
    assert!(section_area(&changed, 1.) < section_area(&old, 1.));
    assert_eq!(removed.enclosed_cavities(), 1);
}
#[test]
fn top_and_side_cardinal_openings_connect_the_same_cavity() {
    for (entry, direction) in [
        ([10., 10., 21.], [0., 0., -1.]),
        ([-1., 10., 10.], [1., 0., 0.]),
        ([21., 10., 10.], [-1., 0., 0.]),
        ([10., -1., 10.], [0., 1., 0.]),
        ([10., 21., 10.], [0., -1., 0.]),
    ] {
        let s = HoleSpec {
            entry_mm: entry,
            direction,
            ..cut()
        };
        let m = run(&[s]).unwrap();
        assert_eq!(m.enclosed_cavities(), 0);
        assert_eq!(m.openings().len(), 1);
        if direction[2] == 0. {
            with_section(&m, 10., |material| {
                let wall = if direction[0] > 0. {
                    [1., 10.]
                } else if direction[0] < 0. {
                    [19., 10.]
                } else if direction[1] > 0. {
                    [10., 1.]
                } else {
                    [10., 19.]
                };
                assert!(!contains(material, wall));
                assert!(area(material) < 144.);
            });
        } else {
            with_section(&m, 19., |material| {
                assert!(!contains(material, [10., 10.]));
                assert!(area(material) < 400.);
            });
        }
    }
}
