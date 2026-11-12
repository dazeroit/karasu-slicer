//! Owner-bound preparation workers, stale-result rejection and retained-preview lifecycle.
use karasu_core::{
    cancellation::{Cancellation, Checkpoint},
    geometry::{ObjectTransform, Point3},
    mesh::TriangleMesh,
    preparation::{
        session::*, supports::*, HollowSpec, Operation, OperationKind, PlanLimits, PreparationPlan,
        SamplingSpec, SourceBinding,
    },
};
use std::{error::Error, sync::Arc};
fn source() -> Arc<TriangleMesh> {
    Arc::new(
        TriangleMesh::try_new(
            [
                [0., 0., 0.],
                [20., 0., 0.],
                [20., 20., 0.],
                [0., 20., 0.],
                [0., 0., 20.],
                [20., 0., 20.],
                [20., 20., 20.],
                [0., 20., 20.],
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
fn plan(thickness: Option<f64>, stamp: u8) -> SupportPlan {
    let operations = thickness
        .map(|thickness_mm| Operation {
            id: 1,
            kind: OperationKind::Hollow(HollowSpec {
                blockers: Vec::new(),
                thickness_mm,
                guarantee: None,
            }),
        })
        .into_iter()
        .collect();
    let model = PreparationPlan::new(
        SourceBinding {
            original_sha256: [stamp; 32],
            placement: ObjectTransform::identity(),
        },
        SamplingSpec {
            cell_mm: 0.1,
            distance_error_mm: 1e-6,
            max_cells: 1,
        },
        operations,
        PlanLimits::default(),
        &mut Cancellation::never(),
    )
    .unwrap();
    SupportPlan::new(
        model,
        vec![],
        SupportIntersectionPolicy::RejectUncertified,
        SupportLimits::default(),
        &mut Cancellation::never(),
    )
    .unwrap()
}
fn session() -> PreparationSession {
    PreparationSession::new(source(), plan(None, 42), 10, 1024 * 1024).unwrap()
}
fn ready(session: &mut PreparationSession) {
    session
        .recompute(SupportLimits::default(), &mut Cancellation::never())
        .unwrap();
    assert_eq!(session.status(), PreparationStatus::Ready);
    assert!(session.current_scene().is_ok());
    assert!(session.last_completed().unwrap().is_current());
}
#[test]
fn ready_noop_and_retained_preview_are_revision_consistent() {
    let mut session = session();
    assert_eq!(session.status(), PreparationStatus::Dirty);
    assert!(session.last_completed().is_none());
    ready(&mut session);
    let plan = session.plan().clone();
    let record = plan.canonical_record().to_vec();
    assert!(!session.apply(plan, &mut Cancellation::never()).unwrap());
    assert_eq!(session.generation(), 0);
    assert!(session.current_scene().is_ok());
    assert_eq!(session.last_completed().unwrap().plan_record(), record);
    assert_eq!(
        session.last_completed().unwrap().plan().canonical_record(),
        record
    );
    assert_eq!(
        session.current_scene().unwrap().plan().canonical_record(),
        record
    );
    assert!(session.last_completed().unwrap().components().is_empty());
    assert_eq!(session.last_completed().unwrap().certificates(), []);
    assert!(session.last_completed().unwrap().cavity().is_none());
}
#[test]
fn same_record_foreign_owner_is_rejected_without_state_change() {
    let mut first = session();
    let mut second = session();
    ready(&mut second);
    let outcome = first
        .begin(SupportLimits::default(), &mut Cancellation::never())
        .unwrap()
        .evaluate(&mut Cancellation::never());
    assert!(matches!(
        second.accept(outcome, &mut Cancellation::never()),
        Err(SessionError::ForeignOutcome)
    ));
    assert!(second.current_scene().is_ok());
}
#[test]
fn newer_same_revision_request_supersedes_previous_worker() {
    let mut session = session();
    ready(&mut session);
    let old = session
        .begin(SupportLimits::default(), &mut Cancellation::never())
        .unwrap();
    let new = session
        .begin(SupportLimits::default(), &mut Cancellation::never())
        .unwrap();
    assert!(new.request() > old.request());
    assert_eq!(old.generation(), new.generation());
    assert_eq!(old.plan().canonical_record(), new.plan().canonical_record());
    assert!(session.current_scene().is_err());
    assert!(!session.last_completed().unwrap().is_current());
    session
        .accept(
            new.evaluate(&mut Cancellation::never()),
            &mut Cancellation::never(),
        )
        .unwrap();
    assert!(matches!(
        session.accept(
            old.evaluate(&mut Cancellation::never()),
            &mut Cancellation::never()
        ),
        Err(SessionError::StaleOutcome)
    ));
    assert!(session.current_scene().is_ok());
}
#[test]
fn edit_undo_aba_does_not_restore_worker_authority() {
    let mut session = session();
    ready(&mut session);
    let old = session
        .begin(SupportLimits::default(), &mut Cancellation::never())
        .unwrap();
    assert!(session
        .apply(plan(Some(2.), 42), &mut Cancellation::never())
        .unwrap());
    assert!(session.undo(&mut Cancellation::never()).unwrap());
    assert_eq!(
        session.plan().canonical_record(),
        old.plan().canonical_record()
    );
    assert!(session.generation() > old.generation());
    assert!(matches!(
        session.accept(
            old.evaluate(&mut Cancellation::never()),
            &mut Cancellation::never()
        ),
        Err(SessionError::StaleOutcome)
    ));
    assert!(session.current_scene().is_err());
    assert!(!session.last_completed().unwrap().is_current());
    assert!(session.redo(&mut Cancellation::never()).unwrap());
    ready(&mut session);
    assert!(session.last_completed().unwrap().cavity().is_some());
}
#[test]
fn failed_geometry_retains_previous_completed_scene_and_original_cause() {
    let mut session = session();
    ready(&mut session);
    let previous = session.last_completed().unwrap().plan_record().to_vec();
    session
        .apply(plan(Some(11.), 42), &mut Cancellation::never())
        .unwrap();
    let error = session
        .recompute(SupportLimits::default(), &mut Cancellation::never())
        .unwrap_err();
    assert!(matches!(error, SessionError::Preparation(_)));
    assert!(error.source().is_some());
    assert_eq!(session.status(), PreparationStatus::Failed);
    assert!(session.current_scene().is_err());
    assert_eq!(session.last_completed().unwrap().plan_record(), previous);
    assert_eq!(
        session.last_completed().unwrap().plan().canonical_record(),
        previous
    );
    assert_ne!(session.plan().canonical_record(), previous);
    assert!(!session.last_completed().unwrap().is_current());
    session.undo(&mut Cancellation::never()).unwrap();
    ready(&mut session);
}
#[test]
fn cancellation_during_evaluation_retains_last_good_and_can_retry() {
    let mut session = session();
    ready(&mut session);
    let request = session.last_completed().unwrap().request();
    let task = session
        .begin(SupportLimits::default(), &mut Cancellation::never())
        .unwrap();
    let outcome = task.evaluate(&mut Cancellation::new(&mut |p| {
        p == Checkpoint::SupportStart
    }));
    assert!(outcome.error().is_some());
    assert!(matches!(
        session.accept(outcome, &mut Cancellation::never()),
        Err(SessionError::Preparation(_))
    ));
    assert_eq!(session.status(), PreparationStatus::Cancelled);
    assert_eq!(session.last_completed().unwrap().request(), request);
    assert!(session.current_scene().is_err());
    ready(&mut session);
}
#[test]
fn publication_cancellation_never_replaces_completed_scene() {
    for checkpoint in [Checkpoint::SessionStart, Checkpoint::SessionPublish] {
        let mut session = session();
        ready(&mut session);
        let previous = session.last_completed().unwrap().request();
        let outcome = session
            .begin(SupportLimits::default(), &mut Cancellation::never())
            .unwrap()
            .evaluate(&mut Cancellation::never());
        assert!(matches!(
            session.accept(outcome, &mut Cancellation::new(&mut |p| p == checkpoint)),
            Err(SessionError::Cancelled(_))
        ));
        assert_eq!(session.status(), PreparationStatus::Cancelled);
        assert_eq!(session.last_completed().unwrap().request(), previous);
        assert!(session.current_scene().is_err());
    }
}
#[test]
fn cancelled_begin_and_edit_preserve_ready_state_and_history() {
    let mut session = session();
    ready(&mut session);
    for checkpoint in [Checkpoint::SessionStart, Checkpoint::SessionPublish] {
        assert!(session
            .begin(
                SupportLimits::default(),
                &mut Cancellation::new(&mut |p| p == checkpoint)
            )
            .is_err());
        assert!(session.current_scene().is_ok());
    }
    assert!(session
        .apply(
            plan(Some(2.), 42),
            &mut Cancellation::new(&mut |p| p == Checkpoint::SupportPublish)
        )
        .is_err());
    assert_eq!(session.generation(), 0);
    assert!(session.current_scene().is_ok());
    assert!(!session.redo(&mut Cancellation::never()).unwrap());
}
#[test]
fn source_swap_is_explicit_and_source_mesh_is_immutable() {
    let mesh = source();
    let vertices = mesh.vertices().to_vec();
    let faces = mesh.triangles().to_vec();
    let mut session =
        PreparationSession::new(mesh.clone(), plan(None, 42), 10, 1024 * 1024).unwrap();
    ready(&mut session);
    assert!(matches!(
        session.apply(plan(None, 43), &mut Cancellation::never()),
        Err(SessionError::SourceMismatch)
    ));
    assert_eq!(session.generation(), 0);
    assert!(session.current_scene().is_ok());
    assert_eq!(mesh.vertices(), vertices);
    assert_eq!(mesh.triangles(), faces);
    assert!(Arc::get_mut(&mut mesh.clone()).is_none());
}
#[test]
fn failed_resource_request_cannot_be_replaced_by_an_older_success() {
    let mut session = session();
    ready(&mut session);
    let old = session
        .begin(SupportLimits::default(), &mut Cancellation::never())
        .unwrap();
    let limits = SupportLimits {
        max_work: 0,
        ..SupportLimits::default()
    };
    let task = session.begin(limits, &mut Cancellation::never()).unwrap();
    let outcome = task.evaluate(&mut Cancellation::never());
    assert!(matches!(
        outcome.error(),
        Some(SupportError::Resource { .. })
    ));
    assert!(session.accept(outcome, &mut Cancellation::never()).is_err());
    assert!(matches!(
        session.accept(
            old.evaluate(&mut Cancellation::never()),
            &mut Cancellation::never()
        ),
        Err(SessionError::StaleOutcome)
    ));
    assert_eq!(session.status(), PreparationStatus::Failed);
    assert!(session.current_scene().is_err());
    ready(&mut session);
}
#[test]
fn worker_progress_is_advisory_until_accepted() {
    let mut session = session();
    let task = session
        .begin(SupportLimits::default(), &mut Cancellation::never())
        .unwrap();
    let mut events = Vec::new();
    let outcome =
        task.evaluate_with_progress(&mut Cancellation::never(), &mut |event| events.push(event));
    assert_eq!(events.last().unwrap().stage, SupportStage::Complete);
    assert_eq!(session.status(), PreparationStatus::Pending);
    assert!(session.current_scene().is_err());
    assert_eq!(outcome.request(), 1);
    session.accept(outcome, &mut Cancellation::never()).unwrap();
    assert!(session.current_scene().is_ok());
}

#[test]
fn owned_worker_can_cross_threads_without_borrowing_mutable_session() {
    let mut session = session();
    let task = session
        .begin(SupportLimits::default(), &mut Cancellation::never())
        .unwrap();
    let worker = std::thread::spawn(move || task.evaluate(&mut Cancellation::never()));
    session
        .apply(plan(Some(2.), 42), &mut Cancellation::never())
        .unwrap();
    let outcome = worker.join().unwrap();
    assert!(matches!(
        session.accept(outcome, &mut Cancellation::never()),
        Err(SessionError::StaleOutcome)
    ));
    assert_eq!(session.status(), PreparationStatus::Dirty);
    assert!(session.last_completed().is_none());
    ready(&mut session);
}

#[test]
fn history_admission_failure_preserves_ready_scene_and_revision() {
    let mut session = PreparationSession::new(source(), plan(None, 42), 1, 1024 * 1024).unwrap();
    ready(&mut session);
    assert!(matches!(
        session.apply(plan(Some(2.), 42), &mut Cancellation::never()),
        Err(SessionError::Preparation(SupportError::Resource { .. }))
    ));
    assert_eq!(session.generation(), 0);
    assert!(session.current_scene().is_ok());
    assert!(session.last_completed().unwrap().is_current());
}
