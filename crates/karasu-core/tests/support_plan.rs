//! Independent support recipe identity, invalidation and transactional history checks.
use karasu_core::{
    cancellation::{Cancellation, Checkpoint},
    geometry::{ObjectTransform, Rotation3},
    preparation::{
        supports::*, HollowSpec, Operation, OperationKind, PlanLimits, PreparationPlan,
        SamplingSpec, SourceBinding,
    },
};
fn model(thickness_mm: f64) -> PreparationPlan {
    PreparationPlan::new(
        SourceBinding {
            original_sha256: [42; 32],
            placement: ObjectTransform::identity(),
        },
        SamplingSpec {
            cell_mm: 0.1,
            distance_error_mm: 1e-6,
            max_cells: 1,
        },
        vec![Operation {
            id: 1,
            kind: OperationKind::Hollow(HollowSpec {
                blockers: Vec::new(),
                thickness_mm,
                guarantee: None,
            }),
        }],
        PlanLimits::default(),
        &mut Cancellation::never(),
    )
    .unwrap()
}
fn recipe(model: &PreparationPlan) -> SupportRecipe {
    SupportRecipe {
        id: 7,
        surface: model.model_key(),
        contact: SupportContact {
            source_triangle: 2,
            position_mm: [0., 0., 10.],
            maximum_penetration_mm: 0.2,
        },
        stations: [0., 1., 9., 10.1]
            .into_iter()
            .map(|z| SupportStation {
                center_mm: [0., 0., z],
                rotation: Rotation3::identity(),
                profile: SupportProfile::Rectangle { size_mm: [1., 2.] },
            })
            .collect(),
    }
}
fn plan(model: PreparationPlan, recipes: Vec<SupportRecipe>) -> SupportPlan {
    SupportPlan::new(
        model,
        recipes,
        SupportIntersectionPolicy::RejectUncertified,
        SupportLimits::default(),
        &mut Cancellation::never(),
    )
    .unwrap()
}
#[test]
fn canonical_record_binds_full_model_and_every_recipe_edit() {
    let model_plan = model(2.);
    let support_recipe = recipe(&model_plan);
    let support_plan = plan(model_plan.clone(), vec![support_recipe.clone()]);
    let record = support_plan.canonical_record();
    assert_eq!(&record[..5], b"KSUP1");
    let length = usize::try_from(u64::from_le_bytes(record[5..13].try_into().unwrap())).unwrap();
    assert_eq!(&record[13..13 + length], model_plan.canonical_record());
    assert_eq!(
        record,
        plan(model_plan.clone(), vec![support_recipe.clone()]).canonical_record()
    );
    assert_eq!(&model_plan.canonical_record()[..6], b"KPREP1");
    for edit in 0..7 {
        let mut support_recipe = support_recipe.clone();
        match edit {
            0 => support_recipe.id = 8,
            1 => support_recipe.stations[1].center_mm[0] = 0.5,
            2 => support_recipe.stations[2].rotation = Rotation3::around_x_radians(0.2).unwrap(),
            3 => {
                support_recipe.stations[1].profile = SupportProfile::RegularPolygon {
                    radius_mm: 1.,
                    sides: 8,
                }
            }
            4 => support_recipe.contact.maximum_penetration_mm = 0.3,
            5 => support_recipe.contact.source_triangle = 3,
            _ => support_recipe.contact.position_mm[0] = 0.1,
        }
        assert_ne!(
            record,
            plan(model_plan.clone(), vec![support_recipe]).canonical_record()
        );
    }
    assert_ne!(record, plan(model_plan, vec![]).canonical_record());
}
#[test]
fn orientation_representation_and_signed_zero_identity_are_explicit() {
    let m = model(2.);
    let r = recipe(&m);
    let p = plan(m.clone(), vec![r.clone()]);
    let mut zero = r.clone();
    zero.stations[0].center_mm[0] = -0.;
    assert_eq!(
        p.canonical_record(),
        plan(m.clone(), vec![zero]).canonical_record()
    );
    let mut negative = r;
    negative.stations[1].rotation = Rotation3::from_quaternion_xyzw([0., 0., 0., -1.]).unwrap();
    assert_ne!(
        p.canonical_record(),
        plan(m, vec![negative]).canonical_record()
    );
}
#[test]
fn stale_contact_and_invalid_parameters_are_typed() {
    let m = model(2.);
    let r = recipe(&m);
    assert!(matches!(
        SupportPlan::new(
            model(3.),
            vec![r.clone()],
            SupportIntersectionPolicy::Union,
            SupportLimits::default(),
            &mut Cancellation::never()
        ),
        Err(SupportError::StaleAnchor { support: 7 })
    ));
    for edit in 0..6 {
        let mut r = r.clone();
        match edit {
            0 => r.stations[1].center_mm[0] = f64::NAN,
            1 => r.stations[0].center_mm[2] = 1.,
            2 => r.stations[1].center_mm[2] = 0.,
            3 => r.stations[0].profile = SupportProfile::Point,
            4 => r.contact.maximum_penetration_mm = f64::INFINITY,
            _ => r.id = 0,
        }
        assert!(matches!(
            SupportPlan::new(
                m.clone(),
                vec![r],
                SupportIntersectionPolicy::Union,
                SupportLimits::default(),
                &mut Cancellation::never()
            ),
            Err(SupportError::Invalid { .. })
        ));
    }
}
#[test]
fn resources_and_cancellation_reject_atomically() {
    let m = model(2.);
    let r = recipe(&m);
    for limits in [
        SupportLimits {
            max_payload_bytes: 1,
            ..SupportLimits::default()
        },
        SupportLimits {
            max_supports: 0,
            ..SupportLimits::default()
        },
        SupportLimits {
            max_work: 0,
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
        assert!(matches!(
            SupportPlan::new(
                m.clone(),
                vec![r.clone()],
                SupportIntersectionPolicy::Union,
                limits,
                &mut Cancellation::never()
            ),
            Err(SupportError::Resource { .. })
        ));
    }
    for checkpoint in [
        Checkpoint::SupportStart,
        Checkpoint::SupportWork,
        Checkpoint::SupportPublish,
    ] {
        let mut stop = |point| point == checkpoint;
        assert!(matches!(
            SupportPlan::new(
                m.clone(),
                vec![r.clone()],
                SupportIntersectionPolicy::Union,
                SupportLimits::default(),
                &mut Cancellation::new(&mut stop)
            ),
            Err(SupportError::Cancelled(_))
        ));
    }
}
#[test]
fn history_edit_remove_undo_redo_and_failed_publish_are_transactional() {
    let m = model(2.);
    let initial = plan(m.clone(), vec![recipe(&m)]);
    let mut history = SupportHistory::new(initial.clone(), 2, 1 << 20).unwrap();
    assert!(!history.apply(initial.clone()).unwrap());
    let removed = plan(m.clone(), vec![]);
    assert!(history.apply(removed.clone()).unwrap());
    assert_eq!(history.generation(), 1);
    assert!(history.apply(initial.clone()).is_err());
    assert_eq!(
        history.current().canonical_record(),
        removed.canonical_record()
    );
    assert!(history.undo().unwrap());
    assert_eq!(
        history.current().canonical_record(),
        initial.canonical_record()
    );
    assert!(history.redo().unwrap());
    assert!(history.undo().unwrap());
    let mut edited = recipe(&m);
    edited.stations[1].profile = SupportProfile::Rectangle { size_mm: [2., 3.] };
    let edited = plan(m, vec![edited]);
    assert!(history.apply(edited.clone()).unwrap());
    assert!(!history.redo().unwrap());
    assert_eq!(
        history.current().canonical_record(),
        edited.canonical_record()
    );
    assert_eq!(initial.recipes().len(), 1);
}

#[test]
fn all_profile_variants_policy_and_tolerances_are_recorded_but_caps_are_not() {
    let m = model(2.);
    let mut r = recipe(&m);
    r.stations[0].profile = SupportProfile::EllipsePolygon {
        radii_mm: [2., 3.],
        sides: 12,
    };
    r.stations[1].profile = SupportProfile::ConvexPolygon {
        vertices_mm: vec![[-1., -1.], [1., -1.], [0., 1.]],
    };
    r.stations[2].profile = SupportProfile::RegularPolygon {
        radius_mm: 0.5,
        sides: 6,
    };
    r.stations[3].profile = SupportProfile::Point;
    let p = plan(m.clone(), vec![r.clone()]);
    let modified = SupportPlan::new(
        m.clone(),
        vec![r.clone()],
        SupportIntersectionPolicy::Union,
        SupportLimits::default(),
        &mut Cancellation::never(),
    )
    .unwrap();
    assert_ne!(p.canonical_record(), modified.canonical_record());
    let limits = SupportLimits {
        clearance_mm: 2e-6,
        ..SupportLimits::default()
    };
    let modified = SupportPlan::new(
        m.clone(),
        vec![r.clone()],
        SupportIntersectionPolicy::RejectUncertified,
        limits,
        &mut Cancellation::never(),
    )
    .unwrap();
    assert_ne!(p.canonical_record(), modified.canonical_record());
    let limits = SupportLimits {
        max_supports: 100,
        max_work: 1_000_000,
        ..SupportLimits::default()
    };
    let modified = SupportPlan::new(
        m,
        vec![r],
        SupportIntersectionPolicy::RejectUncertified,
        limits,
        &mut Cancellation::never(),
    )
    .unwrap();
    assert_eq!(p.canonical_record(), modified.canonical_record());
}

#[test]
fn horizontal_and_descending_centers_remain_explicit_recipes() {
    let m = model(2.);
    let mut r = recipe(&m);
    r.stations[1].center_mm = [0., 0., 2.];
    r.stations[2].center_mm = [1., 0., 2.];
    r.stations[3].center_mm = [2., 0., 1.];
    let p = plan(m, vec![r]);
    assert_eq!(p.recipes()[0].stations[2].center_mm, [1., 0., 2.]);
    assert_eq!(p.recipes()[0].stations[3].center_mm, [2., 0., 1.]);
}

#[test]
fn history_cancellation_preserves_current_redo_and_generation() {
    let model = model(2.);
    let initial = plan(model.clone(), vec![recipe(&model)]);
    let removed = plan(model, vec![]);
    let mut history = SupportHistory::new(initial.clone(), 4, 1 << 20).unwrap();
    history.apply(removed.clone()).unwrap();
    history.undo().unwrap();
    let generation = history.generation();
    let mut stop = |point| point == Checkpoint::SupportPublish;
    assert!(matches!(
        history.apply_with_cancellation(removed.clone(), &mut Cancellation::new(&mut stop)),
        Err(SupportError::Cancelled(_))
    ));
    assert_eq!(history.generation(), generation);
    assert_eq!(
        history.current().canonical_record(),
        initial.canonical_record()
    );
    assert!(matches!(
        history.redo_with_cancellation(&mut Cancellation::new(&mut stop)),
        Err(SupportError::Cancelled(_))
    ));
    assert_eq!(history.generation(), generation);
    assert!(history.redo().unwrap());
    assert_eq!(
        history.current().canonical_record(),
        removed.canonical_record()
    );
    let generation = history.generation();
    assert!(matches!(
        history.undo_with_cancellation(&mut Cancellation::new(&mut stop)),
        Err(SupportError::Cancelled(_))
    ));
    assert_eq!(history.generation(), generation);
    assert_eq!(
        history.current().canonical_record(),
        removed.canonical_record()
    );
}
