//! Parametric manual-support regressions with independently integrated layer sections.
#![allow(clippy::cast_precision_loss)] // Independent test-only rational decoding for area checks.
use karasu_core::{
    cancellation::{Cancellation, Checkpoint},
    geometry::{ObjectTransform, Point3, Rotation3, Vector3},
    mesh::TriangleMesh,
    preparation::{
        supports::*, HoleSpec, HollowSpec, Operation, OperationKind, PlanLimits, PreparationPlan,
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

fn model_plan() -> PreparationPlan {
    PreparationPlan::new(
        SourceBinding {
            original_sha256: [91; 32],
            placement: ObjectTransform::new(
                Vector3::new(0., 0., 10.),
                Rotation3::identity(),
                Vector3::new(1., 1., 1.),
                Point3::new(0., 0., 0.),
            )
            .unwrap(),
        },
        SamplingSpec {
            cell_mm: 0.1,
            distance_error_mm: 0.001,
            max_cells: 1,
        },
        vec![Operation {
            id: 1,
            kind: OperationKind::Hollow(HollowSpec {
                blockers: Vec::new(),
                thickness_mm: 2.,
                guarantee: None,
            }),
        }],
        PlanLimits::default(),
        &mut Cancellation::never(),
    )
    .unwrap()
}
fn station(z: f64, size: f64) -> SupportStation {
    SupportStation {
        center_mm: [12., 8., z],
        rotation: Rotation3::identity(),
        profile: SupportProfile::Rectangle {
            size_mm: [size, size],
        },
    }
}
fn recipe(model: &PreparationPlan) -> SupportRecipe {
    SupportRecipe {
        id: 1,
        surface: model.model_key(),
        contact: SupportContact {
            source_triangle: 0,
            position_mm: [12., 8., 10.],
            maximum_penetration_mm: 0.5,
        },
        stations: vec![
            station(0., 4.),
            station(1., 4.),
            station(9., 1.),
            station(10.3, 0.4),
        ],
    }
}
fn make_plan(
    model: PreparationPlan,
    recipes: Vec<SupportRecipe>,
    policy: SupportIntersectionPolicy,
) -> Result<SupportPlan, SupportError> {
    SupportPlan::new(
        model,
        recipes,
        policy,
        SupportLimits::default(),
        &mut Cancellation::never(),
    )
}
fn run_recipe(recipe: SupportRecipe) -> Result<SupportedModel, SupportError> {
    let p = make_plan(
        model_plan(),
        vec![recipe],
        SupportIntersectionPolicy::RejectUncertified,
    )?;
    generate_supports(
        &cube(20.),
        &p,
        SupportLimits::default(),
        &mut Cancellation::never(),
    )
}
fn run() -> SupportedModel {
    run_recipe(recipe(&model_plan())).unwrap()
}
fn with_section<T>(model: &SupportedModel, z: f64, f: impl FnOnce(&ConstructedMaterial) -> T) -> T {
    let schedule = build_layer_schedule(
        UniformScheduleSpec::try_new(z, 1., ScheduleEnd::Count(1)).unwrap(),
        ScheduleLimits {
            max_layers: 1,
            max_payload_bytes: 1024,
        },
    )
    .unwrap();
    let config = LayerConfig {
        grid: BinaryGridSpec::try_new([0., 0.], -16).unwrap(),
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
fn rectangular_loft_sections_match_analytic_taper_and_hollow_material() {
    let m = run();
    for (z, expected) in [
        (0.5, 16.),
        (5., 6.25),
        (9.5, (1.0_f64 - 0.6 * 0.5 / 1.3).powi(2)),
        (11., 400.),
        (13., 144.),
    ] {
        with_section(&m, z, |layer| {
            assert!(
                (area(layer) - expected).abs() < 0.0002,
                "z={z}, area={}, expected={expected}",
                area(layer)
            );
        });
    }
    with_section(&m, 0.5, |layer| {
        assert!(contains(layer, [12., 8.]));
        assert!(!contains(layer, [15., 8.]));
        assert_eq!(layer.rings().len(), 1);
    });
    with_section(&m, 13., |layer| {
        assert!(!contains(layer, [12., 8.]));
        assert!(contains(layer, [1., 8.]));
    });
}
#[test]
fn source_recipe_and_generated_geometry_are_deterministic() {
    let source = cube(20.);
    let original_v = source.vertices().to_vec();
    let original_t = source.triangles().to_vec();
    let model = model_plan();
    let p = make_plan(
        model.clone(),
        vec![recipe(&model)],
        SupportIntersectionPolicy::RejectUncertified,
    )
    .unwrap();
    let a = generate_supports(
        &source,
        &p,
        SupportLimits::default(),
        &mut Cancellation::never(),
    )
    .unwrap();
    let b = generate_supports(
        &source,
        &p,
        SupportLimits::default(),
        &mut Cancellation::never(),
    )
    .unwrap();
    assert_eq!(source.vertices(), original_v);
    assert_eq!(source.triangles(), original_t);
    assert_eq!(a.plan_record(), b.plan_record());
    assert_eq!(a.certificates(), b.certificates());
    assert_eq!(a.components().len(), 3);
    for (a, b) in a.components().iter().zip(b.components()) {
        assert_eq!(a.mesh().vertices(), b.mesh().vertices());
        assert_eq!(a.mesh().triangles(), b.mesh().triangles());
    }
    let c = a.certificates()[0];
    assert_eq!(c.support_id, 1);
    assert!(c.minimum_penetration_mm > 0.29);
    assert!(c.maximum_penetration_mm < 0.31);
}
#[test]
fn contact_depth_miss_and_cavity_intrusion_are_rejected() {
    let mut r = recipe(&model_plan());
    r.contact.maximum_penetration_mm = 0.1;
    assert!(run_recipe(r).is_err());
    let mut r = recipe(&model_plan());
    r.contact.position_mm = [40., 40., 10.];
    assert!(run_recipe(r).is_err());
    let mut r = recipe(&model_plan());
    r.stations[3].center_mm[2] = 13.;
    r.contact.maximum_penetration_mm = 4.;
    assert!(run_recipe(r).is_err());
}
#[test]
fn stale_model_key_is_rejected_before_geometry_publication() {
    let model = model_plan();
    let mut r = recipe(&model);
    let changed = PreparationPlan::new(
        SourceBinding {
            original_sha256: [92; 32],
            placement: model.source().placement,
        },
        model.sampling(),
        model.operations().to_vec(),
        PlanLimits::default(),
        &mut Cancellation::never(),
    )
    .unwrap();
    r.surface = changed.model_key();
    assert!(make_plan(model, vec![r], SupportIntersectionPolicy::Union).is_err());
}
#[test]
fn invalid_parameters_and_disconnected_platform_are_rejected() {
    for size in [0., -1., f64::NAN, f64::INFINITY] {
        let mut r = recipe(&model_plan());
        r.stations[1].profile = SupportProfile::Rectangle {
            size_mm: [size, 1.],
        };
        assert!(run_recipe(r).is_err());
    }
    let mut r = recipe(&model_plan());
    r.stations[0].center_mm[2] = 0.5;
    assert!(run_recipe(r).is_err());
    let mut r = recipe(&model_plan());
    r.stations[2].center_mm[2] = 1.;
    assert!(run_recipe(r).is_err());
}
#[test]
fn independent_resource_caps_and_cancellation_are_atomic() {
    let source = cube(20.);
    let model = model_plan();
    let p = make_plan(
        model.clone(),
        vec![recipe(&model)],
        SupportIntersectionPolicy::Union,
    )
    .unwrap();
    for limits in [
        SupportLimits {
            max_work: 0,
            ..SupportLimits::default()
        },
        SupportLimits {
            max_payload_bytes: 0,
            ..SupportLimits::default()
        },
        SupportLimits {
            max_supports: 0,
            ..SupportLimits::default()
        },
        SupportLimits {
            max_stations: 3,
            ..SupportLimits::default()
        },
        SupportLimits {
            max_profile_vertices: 3,
            ..SupportLimits::default()
        },
    ] {
        assert!(generate_supports(&source, &p, limits, &mut Cancellation::never()).is_err());
    }
    for at in [
        Checkpoint::SupportStart,
        Checkpoint::SupportWork,
        Checkpoint::SupportPublish,
    ] {
        let mut predicate = |q| q == at;
        assert!(matches!(
            generate_supports(
                &source,
                &p,
                SupportLimits::default(),
                &mut Cancellation::new(&mut predicate)
            ),
            Err(SupportError::Cancelled(_))
        ));
    }
}
#[test]
fn explicit_overlap_union_does_not_double_expose_material() {
    let model = model_plan();
    let a = recipe(&model);
    let mut b = a.clone();
    b.id = 2;
    let reject = make_plan(
        model.clone(),
        vec![a.clone(), b.clone()],
        SupportIntersectionPolicy::RejectUncertified,
    )
    .unwrap();
    assert!(generate_supports(
        &cube(20.),
        &reject,
        SupportLimits::default(),
        &mut Cancellation::never()
    )
    .is_err());
    let union = make_plan(model, vec![a, b], SupportIntersectionPolicy::Union).unwrap();
    let m = generate_supports(
        &cube(20.),
        &union,
        SupportLimits::default(),
        &mut Cancellation::never(),
    )
    .unwrap();
    with_section(&m, 0.5, |layer| {
        assert!((area(layer) - 16.).abs() < 0.0001);
        assert_eq!(layer.rings().len(), 1);
        assert!(contains(layer, [12., 8.]));
    });
}
#[test]
fn multiple_section_families_and_bent_center_paths_generate_real_geometry() {
    let families = [
        SupportProfile::RegularPolygon {
            radius_mm: 2.,
            sides: 6,
        },
        SupportProfile::EllipsePolygon {
            radii_mm: [2., 1.],
            sides: 12,
        },
        SupportProfile::ConvexPolygon {
            vertices_mm: vec![[-2., -1.], [2., -1.], [1., 1.], [-1., 1.]],
        },
    ];
    for profile in families {
        let mut r = recipe(&model_plan());
        r.stations[0].profile = profile.clone();
        r.stations[1].profile = profile;
        r.stations[1].center_mm[0] = 11.;
        r.stations[2].center_mm[0] = 12.5;
        r.stations[2].rotation = Rotation3::around_z_radians(0.3).unwrap();
        let m = run_recipe(r).unwrap();
        assert_eq!(m.components().len(), 3);
        with_section(&m, 0.5, |layer| {
            assert!(area(layer) > 1.);
        });
    }
}
#[test]
fn tilted_neck_and_sharp_tip_remain_explicit_parameter_choices() {
    let mut r = recipe(&model_plan());
    r.stations[2].rotation = Rotation3::around_x_radians(0.15).unwrap();
    r.stations[3].profile = SupportProfile::Point;
    let m = run_recipe(r).unwrap();
    assert_eq!(m.certificates().len(), 1);
    with_section(&m, 9.5, |layer| {
        assert!(area(layer) > 0.01);
        assert!(area(layer) < 1.);
    });
}
#[test]
fn custom_profiles_reject_nonconvex_clockwise_and_exterior_origin_without_repair() {
    let profiles = [
        vec![[-1., -1.], [1., -1.], [0., 0.], [1., 1.], [-1., 1.]],
        vec![[-1., -1.], [-1., 1.], [1., 1.], [1., -1.]],
        vec![[1., 1.], [2., 1.], [2., 2.], [1., 2.]],
    ];
    for vertices_mm in profiles {
        let mut r = recipe(&model_plan());
        r.stations[1].profile = SupportProfile::ConvexPolygon { vertices_mm };
        assert!(run_recipe(r).is_err());
    }
}
#[test]
fn recipe_dimensions_orientation_and_intersection_policy_change_canonical_identity() {
    let model = model_plan();
    let r = recipe(&model);
    let p = make_plan(
        model.clone(),
        vec![r.clone()],
        SupportIntersectionPolicy::Union,
    )
    .unwrap();
    let mut variants = Vec::new();
    let mut sized = r.clone();
    sized.stations[1].profile = SupportProfile::Rectangle { size_mm: [5., 4.] };
    variants.push(sized);
    let mut rotated = r.clone();
    rotated.stations[2].rotation = Rotation3::around_z_radians(0.25).unwrap();
    variants.push(rotated);
    let mut angled = r.clone();
    angled.stations[2].center_mm[0] += 0.5;
    variants.push(angled);
    for variant in variants {
        let q = make_plan(
            model.clone(),
            vec![variant],
            SupportIntersectionPolicy::Union,
        )
        .unwrap();
        assert_ne!(p.canonical_record(), q.canonical_record());
    }
    let q = make_plan(model, vec![r], SupportIntersectionPolicy::RejectUncertified).unwrap();
    assert_ne!(p.canonical_record(), q.canonical_record());
}
#[test]
fn solid_convex_model_accepts_supports_without_inventing_a_cavity() {
    let hollow = model_plan();
    let model = PreparationPlan::new(
        hollow.source(),
        hollow.sampling(),
        vec![],
        PlanLimits::default(),
        &mut Cancellation::never(),
    )
    .unwrap();
    let p = make_plan(
        model.clone(),
        vec![recipe(&model)],
        SupportIntersectionPolicy::Union,
    )
    .unwrap();
    let supported = generate_supports(
        &cube(20.),
        &p,
        SupportLimits::default(),
        &mut Cancellation::never(),
    )
    .unwrap();
    assert!(supported.drained_model().is_none());
    with_section(&supported, 13., |layer| {
        assert!((area(layer) - 400.).abs() < 0.0001);
        assert!(contains(layer, [12., 8.]));
    });
}
#[test]
fn bounded_history_preserves_undo_redo_and_failed_apply_state() {
    let model = model_plan();
    let r = recipe(&model);
    let first = make_plan(
        model.clone(),
        vec![r.clone()],
        SupportIntersectionPolicy::Union,
    )
    .unwrap();
    let mut changed = r;
    changed.stations[1].profile = SupportProfile::Rectangle { size_mm: [5., 4.] };
    let second = make_plan(model, vec![changed], SupportIntersectionPolicy::Union).unwrap();
    let first_record = first.canonical_record().to_vec();
    let second_record = second.canonical_record().to_vec();
    let mut history = SupportHistory::new(first.clone(), 2, 1_000_000).unwrap();
    assert!(history.apply(second.clone()).unwrap());
    assert_eq!(history.current().canonical_record(), second_record);
    assert!(history.undo().unwrap());
    assert_eq!(history.current().canonical_record(), first_record);
    assert!(history.redo().unwrap());
    assert_eq!(history.current().canonical_record(), second_record);
    assert!(!history.apply(second).unwrap());
    assert_eq!(history.generation(), 3);
    let mut bounded = SupportHistory::new(first, 1, 1_000_000).unwrap();
    let generation = bounded.generation();
    assert!(bounded.apply(history.current().clone()).is_err());
    assert_eq!(bounded.generation(), generation);
    assert_eq!(bounded.current().canonical_record(), first_record);
}
#[test]
fn drainage_remains_subtractive_and_cannot_be_blocked_by_a_support() {
    let upstream = model_plan();
    for (entry, accepted) in [([5., 5., 9.], true), ([12., 8., 9.], false)] {
        let mut operations = upstream.operations().to_vec();
        operations.push(Operation {
            id: 2,
            kind: OperationKind::Hole(HoleSpec {
                entry_mm: entry,
                direction: [0., 0., 1.],
                length_mm: 4.,
                diameter_mm: 2.,
            }),
        });
        let model = PreparationPlan::new(
            upstream.source(),
            upstream.sampling(),
            operations,
            PlanLimits::default(),
            &mut Cancellation::never(),
        )
        .unwrap();
        let p = make_plan(
            model.clone(),
            vec![recipe(&model)],
            SupportIntersectionPolicy::Union,
        )
        .unwrap();
        let result = generate_supports(
            &cube(20.),
            &p,
            SupportLimits::default(),
            &mut Cancellation::never(),
        );
        if accepted {
            let supported = result.unwrap();
            assert_eq!(supported.drained_model().unwrap().openings().len(), 1);
            with_section(&supported, 11., |layer| {
                assert!(!contains(layer, [5., 5.]));
                assert!(contains(layer, [12., 8.]));
                assert!(area(layer) < 400.);
                assert!(area(layer) > 396.);
            });
        } else {
            assert!(matches!(result, Err(SupportError::Collision { .. })));
        }
    }
}
#[test]
fn horizontal_side_contact_has_real_oriented_tip_geometry() {
    let model = model_plan();
    let mut r = recipe(&model);
    r.contact = SupportContact {
        source_triangle: 10,
        position_mm: [0., 8., 18.],
        maximum_penetration_mm: 0.5,
    };
    r.stations[0].center_mm = [-4., 8., 0.];
    r.stations[1].center_mm = [-4., 8., 1.];
    r.stations[2].center_mm = [-1., 8., 18.];
    r.stations[3].center_mm = [0.3, 8., 18.];
    let rotate = Rotation3::around_y_radians(std::f64::consts::FRAC_PI_2).unwrap();
    r.stations[2].rotation = rotate;
    r.stations[3].rotation = rotate;
    let p = make_plan(model, vec![r], SupportIntersectionPolicy::Union).unwrap();
    let supported = generate_supports(
        &cube(20.),
        &p,
        SupportLimits::default(),
        &mut Cancellation::never(),
    )
    .unwrap();
    let certificate = supported.certificates()[0];
    assert!(certificate.minimum_penetration_mm > 0.29);
    assert!(certificate.maximum_penetration_mm < 0.31);
    assert_eq!(supported.components().len(), 3);
    for component in supported.components() {
        let mesh = component.mesh();
        let mut edges = std::collections::BTreeMap::new();
        for &[a, b, c] in mesh.triangles() {
            for e in [(a, b), (b, c), (c, a)] {
                *edges.entry(e).or_insert(0usize) += 1;
            }
        }
        assert!(edges
            .iter()
            .all(|(&(a, b), &count)| count == 1 && edges.get(&(b, a)) == Some(&1)));
    }
    with_section(&supported, 18., |layer| {
        assert!(contains(layer, [-0.5, 8.]));
        assert!(contains(layer, [0.1, 8.]));
        assert!(!contains(layer, [3., 8.]));
        assert!(area(layer) > 144.);
    });
}
