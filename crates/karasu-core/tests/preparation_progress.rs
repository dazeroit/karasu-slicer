//! Advisory progress describes immutable computations, never current-revision publication.
use karasu_core::{
    cancellation::{Cancellation, Checkpoint},
    geometry::{ObjectTransform, Point3, Rotation3},
    mesh::TriangleMesh,
    preparation::{
        session::{PreparationSession, PreparationStatus, SessionError},
        supports::*,
        PlanLimits, PreparationPlan, SamplingSpec, SourceBinding,
    },
};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
fn source() -> Arc<TriangleMesh> {
    Arc::new(
        TriangleMesh::try_new(
            [
                [0., 0., 4.],
                [8., 0., 4.],
                [8., 8., 4.],
                [0., 8., 4.],
                [0., 0., 12.],
                [8., 0., 12.],
                [8., 8., 12.],
                [0., 8., 12.],
            ]
            .map(|p| Point3::new(p[0], p[1], p[2]))
            .to_vec(),
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
        .unwrap(),
    )
}
fn plan(count: usize) -> SupportPlan {
    let model = PreparationPlan::new(
        SourceBinding {
            original_sha256: [42; 32],
            placement: ObjectTransform::identity(),
        },
        SamplingSpec {
            cell_mm: 0.1,
            distance_error_mm: 1e-6,
            max_cells: 1,
        },
        vec![],
        PlanLimits::default(),
        &mut Cancellation::never(),
    )
    .unwrap();
    let recipes = [(41, 2.), (73, 6.)]
        .into_iter()
        .take(count)
        .map(|(id, x)| SupportRecipe {
            id,
            surface: model.model_key(),
            contact: SupportContact {
                source_triangle: 0,
                position_mm: [x, 4., 4.],
                maximum_penetration_mm: 0.5,
            },
            stations: [(0., 2.), (1., 2.), (3., 0.8), (4.25, 0.3)]
                .into_iter()
                .map(|(z, size)| SupportStation {
                    center_mm: [x, 4., z],
                    rotation: Rotation3::identity(),
                    profile: SupportProfile::Rectangle {
                        size_mm: [size, size],
                    },
                })
                .collect(),
        })
        .collect();
    SupportPlan::new(
        model,
        recipes,
        SupportIntersectionPolicy::RejectUncertified,
        SupportLimits::default(),
        &mut Cancellation::never(),
    )
    .unwrap()
}
fn event(
    stage: SupportStage,
    completed_supports: usize,
    total_supports: usize,
    support_id: Option<u64>,
    component_index: Option<usize>,
) -> SupportProgress {
    SupportProgress {
        stage,
        completed_supports,
        total_supports,
        support_id,
        component_index,
    }
}
#[test]
fn exact_recipe_component_order_counts_and_ids_are_deterministic() {
    for total in [1, 2] {
        let source = source();
        let plan = plan(total);
        let mut events = Vec::new();
        let result = generate_supports_with_progress(
            &source,
            &plan,
            SupportLimits::default(),
            &mut Cancellation::never(),
            &mut |event| events.push(event),
        )
        .unwrap();
        let mut expected = vec![
            event(SupportStage::Admission, 0, total, None, None),
            event(SupportStage::Source, 0, total, None, None),
        ];
        for (done, recipe) in plan.recipes().iter().enumerate() {
            expected.push(event(
                SupportStage::Profiles,
                done,
                total,
                Some(recipe.id),
                None,
            ));
            for component in 0..3 {
                expected.push(event(
                    SupportStage::Components,
                    done,
                    total,
                    Some(recipe.id),
                    Some(component),
                ));
            }
            expected.push(event(
                SupportStage::Validation,
                done,
                total,
                Some(recipe.id),
                None,
            ));
        }
        expected.push(event(SupportStage::Assembly, total, total, None, None));
        expected.push(event(SupportStage::Complete, total, total, None, None));
        assert_eq!(events, expected);
        assert_eq!(result.components().len(), 3 * total);
        assert_eq!(result.certificates().len(), total);
        let mut repeated = Vec::new();
        generate_supports_with_progress(
            &source,
            &plan,
            SupportLimits::default(),
            &mut Cancellation::never(),
            &mut |event| repeated.push(event),
        )
        .unwrap();
        assert_eq!(events, repeated);
    }
}
#[test]
fn zero_support_scene_reports_real_global_phases_without_fabricated_recipes() {
    let mut events = Vec::new();
    generate_supports_with_progress(
        &source(),
        &plan(0),
        SupportLimits::default(),
        &mut Cancellation::never(),
        &mut |event| events.push(event),
    )
    .unwrap();
    assert_eq!(
        events,
        [
            SupportStage::Admission,
            SupportStage::Source,
            SupportStage::Assembly,
            SupportStage::Complete
        ]
        .map(|stage| event(stage, 0, 0, None, None))
    );
}
#[test]
fn admission_failure_and_entry_final_cancellation_never_report_complete() {
    let mut events = Vec::new();
    let result = generate_supports_with_progress(
        &source(),
        &plan(1),
        SupportLimits {
            max_work: 0,
            ..SupportLimits::default()
        },
        &mut Cancellation::never(),
        &mut |event| events.push(event),
    );
    assert!(matches!(result, Err(SupportError::Resource { .. })));
    assert_eq!(events, [event(SupportStage::Admission, 0, 1, None, None)]);
    for checkpoint in [
        Checkpoint::SupportStart,
        Checkpoint::SupportWork,
        Checkpoint::SupportPublish,
    ] {
        let mut events = Vec::new();
        let result = generate_supports_with_progress(
            &source(),
            &plan(1),
            SupportLimits::default(),
            &mut Cancellation::new(&mut |point| point == checkpoint),
            &mut |event| events.push(event),
        );
        assert!(result.is_err());
        assert!(events
            .iter()
            .all(|event| event.stage != SupportStage::Complete));
        if checkpoint == Checkpoint::SupportStart {
            assert_eq!(events, [] as [SupportProgress; 0]);
        }
        if checkpoint == Checkpoint::SupportPublish {
            assert_eq!(events.last().unwrap().stage, SupportStage::Assembly);
        }
    }
}
#[test]
fn assembly_observer_can_request_cooperative_cancellation_without_publication() {
    let cancelled = AtomicBool::new(false);
    let mut events = Vec::new();
    let result = generate_supports_with_progress(
        &source(),
        &plan(1),
        SupportLimits::default(),
        &mut Cancellation::new(&mut |_| cancelled.load(Ordering::Relaxed)),
        &mut |event| {
            events.push(event);
            if event.stage == SupportStage::Assembly {
                cancelled.store(true, Ordering::Relaxed);
            }
        },
    );
    assert!(matches!(result, Err(SupportError::Cancelled(_))));
    assert_eq!(events.last().unwrap().stage, SupportStage::Assembly);
    assert!(events
        .iter()
        .all(|event| event.stage != SupportStage::Complete));
}
#[test]
fn completed_stale_worker_does_not_grant_current_scene_or_replace_new_request() {
    let mut session = PreparationSession::new(source(), plan(1), 4, 1024 * 1024).unwrap();
    let old = session
        .begin(SupportLimits::default(), &mut Cancellation::never())
        .unwrap();
    let new = session
        .begin(SupportLimits::default(), &mut Cancellation::never())
        .unwrap();
    let mut events = Vec::new();
    let outcome =
        old.evaluate_with_progress(&mut Cancellation::never(), &mut |event| events.push(event));
    assert!(outcome.error().is_none());
    assert_eq!(events.last().unwrap().stage, SupportStage::Complete);
    assert_eq!(session.status(), PreparationStatus::Pending);
    assert!(session.current_scene().is_err());
    assert!(matches!(
        session.accept(outcome, &mut Cancellation::never()),
        Err(SessionError::StaleOutcome)
    ));
    assert_eq!(session.status(), PreparationStatus::Pending);
    assert!(session.last_completed().is_none());
    session
        .accept(
            new.evaluate(&mut Cancellation::never()),
            &mut Cancellation::never(),
        )
        .unwrap();
    assert!(session.current_scene().is_ok());
}
