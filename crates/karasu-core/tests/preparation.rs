//! Independent admission, exact binding and transactional preparation-history regressions.
use karasu_core::{
    cancellation::{Cancellation, Checkpoint},
    geometry::{ObjectTransform, Point3, Rotation3, Vector3},
    preparation::{
        HistoryLimits, HoleSpec, HollowSpec, Operation, OperationKind, PlanLimits,
        PreparationError, PreparationHistory, PreparationPlan, SamplingSpec, SourceBinding,
        SupportSpec,
    },
};

fn source() -> SourceBinding {
    SourceBinding {
        original_sha256: [37; 32],
        placement: ObjectTransform::identity(),
    }
}
fn sampling() -> SamplingSpec {
    SamplingSpec {
        cell_mm: 0.1,
        distance_error_mm: 0.001,
        max_cells: 100_000,
    }
}
fn hollow(thickness_mm: f64) -> Operation {
    Operation {
        id: 1,
        kind: OperationKind::Hollow(HollowSpec {
            blockers: Vec::new(),
            thickness_mm,
            guarantee: None,
        }),
    }
}
fn hole(direction: [f64; 3]) -> Operation {
    Operation {
        id: 2,
        kind: OperationKind::Hole(HoleSpec {
            entry_mm: [0., 0., 21.],
            direction,
            length_mm: 4.,
            diameter_mm: 2.,
        }),
    }
}
fn plan_with(
    source: SourceBinding,
    operations: Vec<Operation>,
) -> Result<PreparationPlan, PreparationError> {
    PreparationPlan::new(
        source,
        sampling(),
        operations,
        PlanLimits::default(),
        &mut Cancellation::never(),
    )
}
fn plan(thickness: f64) -> PreparationPlan {
    plan_with(source(), vec![hollow(thickness)]).unwrap()
}
fn support(model: &PreparationPlan, id: u64) -> Operation {
    Operation {
        id,
        kind: OperationKind::Support(SupportSpec {
            surface: model.model_key(),
            contact_mm: [0., 0., 10.],
            base_xy_mm: [0., 0.],
            tip_radius_mm: 0.2,
            penetration_mm: 0.1,
            shaft_radius_mm: 0.5,
            base_radius_mm: 2.,
            base_height_mm: 1.,
        }),
    }
}
fn assert_invalid(result: Result<PreparationPlan, PreparationError>, id: Option<u64>) {
    let error = result.expect_err("invalid parameters must be rejected");
    assert!(matches!(error, PreparationError::Invalid { operation, .. } if operation == id));
}

#[test]
fn canonical_record_has_independently_decoded_version_source_and_affine() {
    let p = plan(2.);
    let bytes = p.canonical_record();
    assert_eq!(&bytes[..6], b"KPREP1");
    assert_eq!(&bytes[6..38], &[37; 32]);
    let values: Vec<f64> = bytes[38..134]
        .as_chunks::<8>()
        .0
        .iter()
        .map(|b| f64::from_le_bytes(*b))
        .collect();
    assert_eq!(values, [1., 0., 0., 0., 1., 0., 0., 0., 1., 0., 0., 0.]);
    assert_eq!(p.canonical_record(), plan(2.).canonical_record());
    assert_ne!(p.canonical_record(), plan(3.).canonical_record());
    assert_eq!(p.source(), source());
}

#[test]
fn direction_normalization_is_overflow_safe_and_preserves_orientation() {
    let ordinary = plan_with(source(), vec![hollow(2.), hole([0., 0., -1.])]).unwrap();
    for magnitude in [f64::MAX, f64::MIN_POSITIVE, f64::from_bits(1)] {
        let p = plan_with(source(), vec![hollow(2.), hole([0., 0., -magnitude])]).unwrap();
        let OperationKind::Hole(cut) = &p.operations()[1].kind else {
            panic!("expected cut")
        };
        assert_eq!(cut.direction, [0., 0., -magnitude]);
        assert_eq!(cut.unit_direction().unwrap(), [0., 0., -1.]);
        assert_ne!(p.canonical_record(), ordinary.canonical_record());
    }
    let diagonal = plan_with(source(), vec![hollow(2.), hole([f64::MAX; 3])]).unwrap();
    let OperationKind::Hole(cut) = &diagonal.operations()[1].kind else {
        panic!("expected cut")
    };
    let unit = cut.unit_direction().unwrap();
    assert!(unit.iter().all(|v| v.is_finite() && *v > 0.));
    assert!((unit.iter().map(|v| v * v).sum::<f64>() - 1.).abs() < 1e-15);
    let reverse = plan_with(source(), vec![hollow(2.), hole([0., 0., 1.])]).unwrap();
    assert_ne!(ordinary.canonical_record(), reverse.canonical_record());
}

#[test]
fn reconstructing_admitted_parameters_preserves_exact_replay_content() {
    let model = plan_with(source(), vec![hollow(2.), hole([3., 7., -11.])]).unwrap();
    let mut operations = model.operations().to_vec();
    operations.push(support(&model, 3));
    let original = plan_with(source(), operations).unwrap();
    let reconstructed = PreparationPlan::new(
        original.source(),
        original.sampling(),
        original.operations().to_vec(),
        PlanLimits::default(),
        &mut Cancellation::never(),
    )
    .unwrap();
    assert_eq!(
        original.canonical_record(),
        reconstructed.canonical_record()
    );
    assert_eq!(original.model_key(), reconstructed.model_key());
}

#[test]
fn signed_zero_does_not_change_replay_identity() {
    let positive = plan_with(source(), vec![hollow(2.), hole([0., 0., -1.])]).unwrap();
    let mut cut = hole([-0., -0., -1.]);
    if let OperationKind::Hole(s) = &mut cut.kind {
        s.entry_mm[0] = -0.;
    }
    let negative = plan_with(source(), vec![hollow(2.), cut]).unwrap();
    assert_eq!(positive.canonical_record(), negative.canonical_record());
}

#[test]
fn ids_and_operation_order_are_not_silently_repaired() {
    let mut zero = hollow(2.);
    zero.id = 0;
    assert_invalid(plan_with(source(), vec![zero]), Some(0));
    assert_invalid(plan_with(source(), vec![hollow(2.), hollow(3.)]), Some(1));
    let mut duplicate = hole([0., 0., -1.]);
    duplicate.id = 1;
    assert_invalid(plan_with(source(), vec![hollow(2.), duplicate]), Some(1));
    assert_invalid(plan_with(source(), vec![hole([0., 0., -1.])]), Some(2));
    let model = plan(2.);
    assert_invalid(
        plan_with(
            source(),
            vec![hollow(2.), support(&model, 3), hole([0., 0., -1.])],
        ),
        Some(2),
    );
}

#[test]
fn malformed_dimensions_directions_and_extents_report_operation() {
    for value in [
        0.,
        -1.,
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::from_bits(1),
    ] {
        assert_invalid(plan_with(source(), vec![hollow(value)]), Some(1));
    }
    for direction in [[0.; 3], [f64::NAN, 1., 0.], [f64::INFINITY, 0., 0.]] {
        assert_invalid(
            plan_with(source(), vec![hollow(2.), hole(direction)]),
            Some(2),
        );
    }
    let mut cut = hole([1., 0., 0.]);
    if let OperationKind::Hole(s) = &mut cut.kind {
        s.entry_mm[0] = 1_000_000.;
    }
    assert_invalid(plan_with(source(), vec![hollow(2.), cut]), Some(2));
}

#[test]
fn model_prefix_changes_require_explicit_support_rebinding() {
    let model = plan(2.);
    let anchored = support(&model, 3);
    assert!(matches!(
        plan_with(source(), vec![hollow(3.), anchored.clone()]),
        Err(PreparationError::StaleAnchor { operation: 3 })
    ));
    let mut changed_source = source();
    changed_source.original_sha256[0] ^= 1;
    assert!(matches!(
        plan_with(changed_source, vec![hollow(2.), anchored.clone()]),
        Err(PreparationError::StaleAnchor { operation: 3 })
    ));
    changed_source = source();
    changed_source.placement = ObjectTransform::new(
        Vector3::new(1., 0., 0.),
        Rotation3::identity(),
        Vector3::new(1., 1., 1.),
        Point3::new(0., 0., 0.),
    )
    .unwrap();
    assert!(matches!(
        plan_with(changed_source, vec![hollow(2.), anchored]),
        Err(PreparationError::StaleAnchor { operation: 3 })
    ));
    let rebound_model = plan_with(changed_source, vec![hollow(2.)]).unwrap();
    assert!(plan_with(changed_source, vec![hollow(2.), support(&rebound_model, 3)]).is_ok());
}

#[test]
fn support_edits_preserve_model_key_but_change_full_record() {
    let model = plan(2.);
    let first = plan_with(source(), vec![hollow(2.), support(&model, 3)]).unwrap();
    let mut moved = support(&model, 3);
    if let OperationKind::Support(s) = &mut moved.kind {
        s.contact_mm[0] = 2.;
    }
    let second = plan_with(source(), vec![hollow(2.), moved, support(&model, 4)]).unwrap();
    assert_eq!(model.model_key(), first.model_key());
    assert_eq!(first.model_key(), second.model_key());
    assert_ne!(first.canonical_record(), second.canonical_record());
    assert_eq!(model.operations().len(), 1);
}

#[test]
fn solid_models_can_request_supports_without_hollowing() {
    let solid = plan_with(source(), vec![]).unwrap();
    let supported = plan_with(source(), vec![support(&solid, 9)]).unwrap();
    assert_eq!(solid.model_key(), supported.model_key());
    assert_eq!(supported.operations().len(), 1);
}

#[test]
fn sampling_or_model_record_identity_changes_invalidate_surface_bindings() {
    let model = plan(2.);
    let anchored = support(&model, 3);
    let changed = SamplingSpec {
        max_cells: sampling().max_cells + 1,
        ..sampling()
    };
    assert!(matches!(
        PreparationPlan::new(
            source(),
            changed,
            vec![hollow(2.), anchored.clone()],
            PlanLimits::default(),
            &mut Cancellation::never()
        ),
        Err(PreparationError::StaleAnchor { operation: 3 })
    ));
    let mut renamed = hollow(2.);
    renamed.id = 7;
    assert!(matches!(
        plan_with(source(), vec![renamed, anchored]),
        Err(PreparationError::StaleAnchor { operation: 3 })
    ));
}

#[test]
fn moving_placement_does_not_silently_move_world_fixed_cut_parameters() {
    let original = plan_with(source(), vec![hollow(2.), hole([0., 0., -1.])]).unwrap();
    let mut moved_source = source();
    moved_source.placement = ObjectTransform::new(
        Vector3::new(4., 5., 6.),
        Rotation3::identity(),
        Vector3::new(2., 3., 4.),
        Point3::new(0., 0., 0.),
    )
    .unwrap();
    let moved = plan_with(moved_source, original.operations().to_vec()).unwrap();
    assert_eq!(original.operations(), moved.operations());
    assert_ne!(original.model_key(), moved.model_key());
}

#[test]
fn support_proportions_and_collapsed_or_reflected_placement_are_rejected() {
    let model = plan(2.);
    let mut bad = support(&model, 3);
    if let OperationKind::Support(s) = &mut bad.kind {
        s.contact_mm[2] = s.base_height_mm;
    }
    assert_invalid(plan_with(source(), vec![hollow(2.), bad]), Some(3));
    for scale in [0., -1.] {
        let mut s = source();
        s.placement = ObjectTransform::new(
            Vector3::new(0., 0., 0.),
            Rotation3::identity(),
            Vector3::new(scale, 1., 1.),
            Point3::new(0., 0., 0.),
        )
        .unwrap();
        assert_invalid(plan_with(s, vec![]), None);
    }
}

#[test]
fn sampling_and_plan_limits_are_explicit_and_enforced() {
    for s in [
        SamplingSpec {
            max_cells: 0,
            ..sampling()
        },
        SamplingSpec {
            distance_error_mm: 0.1,
            ..sampling()
        },
        SamplingSpec {
            cell_mm: f64::NAN,
            ..sampling()
        },
    ] {
        assert_invalid(
            PreparationPlan::new(
                source(),
                s,
                vec![],
                PlanLimits::default(),
                &mut Cancellation::never(),
            ),
            None,
        );
    }
    let limits = PlanLimits {
        max_operations: 1,
        ..PlanLimits::default()
    };
    assert!(PreparationPlan::new(
        source(),
        sampling(),
        vec![hollow(2.)],
        limits,
        &mut Cancellation::never()
    )
    .is_ok());
    assert!(matches!(
        PreparationPlan::new(
            source(),
            sampling(),
            vec![hollow(2.), hole([0., 0., -1.])],
            limits,
            &mut Cancellation::never()
        ),
        Err(PreparationError::Resource { .. })
    ));
    let baseline = plan(2.);
    let exact = PlanLimits {
        max_payload_bytes: baseline.payload_bytes(),
        ..PlanLimits::default()
    };
    assert!(PreparationPlan::new(
        source(),
        sampling(),
        vec![hollow(2.)],
        exact,
        &mut Cancellation::never()
    )
    .is_ok());
    assert!(matches!(
        PreparationPlan::new(
            source(),
            sampling(),
            vec![hollow(2.)],
            PlanLimits {
                max_payload_bytes: exact.max_payload_bytes - 1,
                ..exact
            },
            &mut Cancellation::never()
        ),
        Err(PreparationError::Resource { .. })
    ));
}

#[test]
fn plan_cancellation_at_entry_records_and_publication_never_returns_success() {
    for stop in [
        Checkpoint::PreparationStart,
        Checkpoint::PreparationRecords,
        Checkpoint::PreparationPublish,
    ] {
        let mut predicate = |point| point == stop;
        assert!(matches!(
            PreparationPlan::new(
                source(),
                sampling(),
                vec![hollow(2.)],
                PlanLimits::default(),
                &mut Cancellation::new(&mut predicate)
            ),
            Err(PreparationError::Cancelled(_))
        ));
    }
    assert_eq!(plan(2.).operations().len(), 1);
}

#[test]
fn history_undo_redo_branching_and_aba_generation() {
    let original = plan(2.);
    let mut history = PreparationHistory::new(original.clone(), HistoryLimits::default()).unwrap();
    let token = history.generation();
    assert!(history.matches(token, &original));
    assert!(!history.undo(&mut Cancellation::never()).unwrap());
    assert_eq!(history.generation(), token);
    history
        .commit(plan(3.), &mut Cancellation::never())
        .unwrap();
    assert!(history.undo(&mut Cancellation::never()).unwrap());
    assert_eq!(
        history.current().canonical_record(),
        original.canonical_record()
    );
    assert!(!history.matches(token, &original));
    assert!(history.redo(&mut Cancellation::never()).unwrap());
    assert!(history.undo(&mut Cancellation::never()).unwrap());
    history
        .commit(plan(4.), &mut Cancellation::never())
        .unwrap();
    assert_eq!(history.snapshot_count(), 2);
    assert!(!history.redo(&mut Cancellation::never()).unwrap());
    assert_eq!(
        history.current().canonical_record(),
        plan(4.).canonical_record()
    );
}

#[test]
fn failed_or_cancelled_history_changes_preserve_redo_and_generation() {
    for stop in [
        Checkpoint::PreparationStart,
        Checkpoint::PreparationRecords,
        Checkpoint::PreparationPublish,
    ] {
        let mut h = PreparationHistory::new(plan(2.), HistoryLimits::default()).unwrap();
        h.commit(plan(3.), &mut Cancellation::never()).unwrap();
        h.undo(&mut Cancellation::never()).unwrap();
        let generation = h.generation();
        let before = h.current().canonical_record().to_vec();
        let mut predicate = |point| point == stop;
        assert!(matches!(
            h.commit(plan(4.), &mut Cancellation::new(&mut predicate)),
            Err(PreparationError::Cancelled(_))
        ));
        assert_eq!(h.generation(), generation);
        assert_eq!(h.current().canonical_record(), before);
        assert_eq!(h.snapshot_count(), 2);
        assert!(h.redo(&mut Cancellation::never()).unwrap());
        assert_eq!(h.current().canonical_record(), plan(3.).canonical_record());
    }
}

#[test]
fn history_retained_payload_and_snapshot_limits_fail_atomically() {
    let p = plan(2.);
    let bytes = p.payload_bytes();
    let mut exact = PreparationHistory::new(
        p.clone(),
        HistoryLimits {
            max_snapshots: 2,
            max_payload_bytes: bytes * 2,
        },
    )
    .unwrap();
    exact.commit(p.clone(), &mut Cancellation::never()).unwrap();
    assert!(matches!(
        exact.commit(p.clone(), &mut Cancellation::never()),
        Err(PreparationError::Resource { .. })
    ));
    assert_eq!(exact.snapshot_count(), 2);
    assert_eq!(exact.generation(), 1);
    let mut short = PreparationHistory::new(
        p.clone(),
        HistoryLimits {
            max_snapshots: 2,
            max_payload_bytes: bytes * 2 - 1,
        },
    )
    .unwrap();
    assert!(matches!(
        short.commit(p, &mut Cancellation::never()),
        Err(PreparationError::Resource { .. })
    ));
    assert_eq!(short.snapshot_count(), 1);
    assert_eq!(short.generation(), 0);
}

#[test]
fn cancelled_undo_and_redo_preserve_cursor_and_generation() {
    let mut history = PreparationHistory::new(plan(2.), HistoryLimits::default()).unwrap();
    history
        .commit(plan(3.), &mut Cancellation::never())
        .unwrap();
    for stop in [Checkpoint::PreparationStart, Checkpoint::PreparationPublish] {
        let generation = history.generation();
        let mut predicate = |point| point == stop;
        assert!(matches!(
            history.undo(&mut Cancellation::new(&mut predicate)),
            Err(PreparationError::Cancelled(_))
        ));
        assert_eq!(history.generation(), generation);
        assert_eq!(
            history.current().canonical_record(),
            plan(3.).canonical_record()
        );
    }
    history.undo(&mut Cancellation::never()).unwrap();
    for stop in [Checkpoint::PreparationStart, Checkpoint::PreparationPublish] {
        let generation = history.generation();
        let mut predicate = |point| point == stop;
        assert!(matches!(
            history.redo(&mut Cancellation::new(&mut predicate)),
            Err(PreparationError::Cancelled(_))
        ));
        assert_eq!(history.generation(), generation);
        assert_eq!(
            history.current().canonical_record(),
            plan(2.).canonical_record()
        );
    }
    assert!(history.redo(&mut Cancellation::never()).unwrap());
}
