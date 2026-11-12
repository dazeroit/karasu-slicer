//! Bounded combined-preparation lifecycle snapshots, publication and current-only access.
use karasu_core::{
    cancellation::{Cancellation, Checkpoint},
    geometry::{ObjectTransform, Point3, Rotation3, Vector3},
    mesh::TriangleMesh,
    preparation::{
        drainage::DrainageLimits,
        hollow::HollowLimits,
        session::{PreparationSession, PreparationStatus, SessionError},
        supports::*,
        HoleSpec, HollowSpec, Operation, OperationKind, PlanLimits, PreparationPlan, SamplingSpec,
        SourceBinding,
    },
    slicing::{
        construct::ConstructLimits,
        engine::{LayerConfig, LayerStageLimits},
        grid::BinaryGridSpec,
        regions::RegionFillRule,
        schedule::*,
    },
};
use std::sync::Arc;
fn byte(data: &[u8], index: usize) -> u8 {
    data.get(index).copied().unwrap_or(0)
}
fn cube() -> TriangleMesh {
    TriangleMesh::try_new(
        [
            [0., 0., 0.],
            [8., 0., 0.],
            [8., 8., 0.],
            [0., 8., 0.],
            [0., 0., 8.],
            [8., 0., 8.],
            [8., 8., 8.],
            [0., 8., 8.],
        ]
        .map(|[x, y, z]| Point3::new(x, y, z))
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
    .unwrap()
}

fn limits() -> SupportLimits {
    let component = HollowLimits {
        max_input_vertices: 64,
        max_input_triangles: 128,
        max_planes: 64,
        max_output_vertices: 128,
        max_output_triangles: 256,
        max_work: 200_000,
        max_payload_bytes: 2 * 1024 * 1024,
    };
    SupportLimits {
        drainage: DrainageLimits {
            hollow: component,
            max_holes: 1,
            max_segments: 64,
            max_work: 100_000,
            max_payload_bytes: 4 * 1024 * 1024,
            minimum_clearance_mm: 1e-6,
        },
        max_supports: 1,
        max_stations: 4,
        max_profile_vertices: 8,
        component,
        max_work: 1_500_000,
        max_payload_bytes: 24 * 1024 * 1024,
        clearance_mm: 1e-6,
        contact_tolerance_mm: 1e-5,
    }
}
fn plan(diameter: f64, negative: bool, stale: bool) -> Result<SupportPlan, SupportError> {
    let model = PreparationPlan::new(
        SourceBinding {
            original_sha256: [94; 32],
            placement: ObjectTransform::new(
                Vector3::new(0., 0., 4.),
                Rotation3::identity(),
                Vector3::new(1., 1., 1.),
                Point3::new(0., 0., 0.),
            )
            .unwrap(),
        },
        SamplingSpec {
            cell_mm: 0.1,
            distance_error_mm: 0.002,
            max_cells: 1,
        },
        vec![
            Operation {
                id: 1,
                kind: OperationKind::Hollow(HollowSpec { thickness_mm: 1. }),
            },
            Operation {
                id: 2,
                kind: OperationKind::Hole(HoleSpec {
                    entry_mm: [1.5, 1.5, 3.],
                    direction: [0., 0., 1.],
                    length_mm: 3.,
                    diameter_mm: diameter,
                }),
            },
        ],
        PlanLimits::default(),
        &mut Cancellation::never(),
    )
    .unwrap();
    let mut recipe = SupportRecipe {
        id: 1,
        surface: model.model_key(),
        contact: SupportContact {
            source_triangle: 0,
            position_mm: [5., 3., 4.],
            maximum_penetration_mm: 0.5,
        },
        stations: [(0., 2.), (0.5, 2.), (3.5, 0.6), (4.2, 0.2)]
            .into_iter()
            .map(|(z, size)| SupportStation {
                center_mm: [5., 3., z],
                rotation: Rotation3::identity(),
                profile: SupportProfile::Rectangle {
                    size_mm: [size, size],
                },
            })
            .collect(),
    };
    if negative {
        recipe.stations[1].profile = SupportProfile::Rectangle { size_mm: [-1., 1.] };
    }
    if stale {
        recipe.surface = PreparationPlan::new(
            model.source(),
            model.sampling(),
            vec![],
            PlanLimits::default(),
            &mut Cancellation::never(),
        )
        .unwrap()
        .model_key();
    }
    SupportPlan::new(
        model,
        vec![recipe],
        SupportIntersectionPolicy::RejectUncertified,
        limits(),
        &mut Cancellation::never(),
    )
}
/// Exercise owned public API invariants; rejection includes intentionally invalid lifecycle work.
pub fn check_preparation_session(data: &[u8]) -> bool {
    if data.len() > 256 {
        return false;
    }
    let mode = byte(data, 0) % 33;
    let diameter = 0.4 + f64::from(byte(data, 2)) / 1024.;
    let proposed = plan(diameter, mode == 1, mode == 2);
    if mode == 1 || mode == 2 {
        assert!(proposed.is_err());
        return false;
    }
    let original = proposed.unwrap();
    let original_bytes = original.canonical_record().to_vec();
    let source = Arc::new(cube());
    let vertices = source.vertices().to_vec();
    let triangles = source.triangles().to_vec();
    let mut session =
        PreparationSession::new(source.clone(), original.clone(), 3, 64 * 1024).unwrap();
    assert_eq!(session.status(), PreparationStatus::Dirty);
    assert!(session.current_scene().is_err());
    assert!(session.last_completed().is_none());
    let mut selected = limits();
    if mode == 7 {
        selected.max_payload_bytes = 0;
    }
    let task = session.begin(selected, &mut Cancellation::never()).unwrap();
    assert_eq!(session.status(), PreparationStatus::Pending);
    assert!(session.current_scene().is_err());
    if mode == 4 {
        let mut foreign = PreparationSession::new(source.clone(), original, 3, 64 * 1024).unwrap();
        let outcome = task.evaluate(&mut Cancellation::never());
        assert!(matches!(
            foreign.accept(outcome, &mut Cancellation::never()),
            Err(SessionError::ForeignOutcome)
        ));
        assert_eq!(foreign.status(), PreparationStatus::Dirty);
        return false;
    }
    if mode == 5 {
        let newest = session.begin(limits(), &mut Cancellation::never()).unwrap();
        let old = task.evaluate(&mut Cancellation::never());
        assert!(matches!(
            session.accept(old, &mut Cancellation::never()),
            Err(SessionError::StaleOutcome)
        ));
        assert_eq!(session.status(), PreparationStatus::Pending);
        session
            .accept(
                newest.evaluate(&mut Cancellation::never()),
                &mut Cancellation::never(),
            )
            .unwrap();
    } else if mode == 6 {
        session
            .apply(
                plan(0.45, false, false).unwrap(),
                &mut Cancellation::never(),
            )
            .unwrap();
        session
            .apply(original.clone(), &mut Cancellation::never())
            .unwrap();
        assert!(matches!(
            session.accept(
                task.evaluate(&mut Cancellation::never()),
                &mut Cancellation::never()
            ),
            Err(SessionError::StaleOutcome)
        ));
        assert_eq!(session.status(), PreparationStatus::Dirty);
        session
            .recompute(limits(), &mut Cancellation::never())
            .unwrap();
    } else {
        let mut predicate = |p| mode == 3 && p == Checkpoint::DrainageWork;
        let outcome = task.evaluate(&mut Cancellation::new(&mut predicate));
        if mode == 3 || mode == 7 {
            assert!(outcome.error().is_some());
            assert!(session.accept(outcome, &mut Cancellation::never()).is_err());
            assert!(session.current_scene().is_err());
            return false;
        }
        let mut accept_cancel = |p| mode == 9 && p == Checkpoint::SessionPublish;
        let result = session.accept(outcome, &mut Cancellation::new(&mut accept_cancel));
        if mode == 9 {
            assert!(result.is_err());
            assert_eq!(session.status(), PreparationStatus::Cancelled);
            return false;
        }
        result.unwrap();
    }
    assert_eq!(session.status(), PreparationStatus::Ready);
    assert!(session.last_completed().is_some());
    if mode == 8 {
        session
            .apply(
                plan(0.45, false, false).unwrap(),
                &mut Cancellation::never(),
            )
            .unwrap();
        assert!(session.current_scene().is_err());
        assert!(session.last_completed().is_some());
        session.undo(&mut Cancellation::never()).unwrap();
        session.redo(&mut Cancellation::never()).unwrap();
        session
            .recompute(limits(), &mut Cancellation::never())
            .unwrap();
    }
    if mode == 10 {
        let before = session.plan().canonical_record().to_vec();
        let mut predicate = |p| p == Checkpoint::SupportPublish;
        assert!(session
            .apply(
                plan(0.45, false, false).unwrap(),
                &mut Cancellation::new(&mut predicate)
            )
            .is_err());
        assert_eq!(session.plan().canonical_record(), before);
        assert_eq!(session.status(), PreparationStatus::Ready);
    }
    // A failed reevaluation must retain preview-only geometry and block new layers.
    if mode == 11 {
        let mut failed = limits();
        failed.max_work = 0;
        assert!(session
            .recompute(failed, &mut Cancellation::never())
            .is_err());
        assert_eq!(session.status(), PreparationStatus::Failed);
        assert!(session.current_scene().is_err());
        assert!(session.last_completed().is_some());
        session
            .recompute(limits(), &mut Cancellation::never())
            .unwrap();
    }
    let scene = session.current_scene().unwrap();
    assert!(!scene.plan_record().is_empty());
    assert_eq!(scene.drained_model().unwrap().enclosed_cavities(), 0);
    let z = if byte(data, 1) & 1 == 0 { 0.25 } else { 4.5 };
    let schedule = build_layer_schedule(
        UniformScheduleSpec::try_new(z, 1., ScheduleEnd::Count(1)).unwrap(),
        ScheduleLimits {
            max_layers: 1,
            max_payload_bytes: 1024,
        },
    )
    .unwrap();
    let config = LayerConfig {
        grid: BinaryGridSpec::try_new([0.; 2], -10).unwrap(),
        fill_rule: RegionFillRule::EvenOddStrict,
        stages: LayerStageLimits::default(),
        max_output_payload_bytes: 4 * 1024 * 1024,
    };
    let construct = ConstructLimits {
        max_input_edges: 256,
        max_pairs: 40_000,
        max_vertices: 1024,
        max_atoms: 2048,
        max_work: 200_000,
        max_payload_bytes: 4 * 1024 * 1024,
    };
    let mut layers = scene.layer_engine(&schedule, config, construct, Cancellation::never(), None);
    let layer = layers.next().unwrap().unwrap();
    assert!(!layer.material_union().unwrap().regions().is_empty());
    assert!(layers.next().is_none());
    assert_eq!(source.vertices(), vertices);
    assert_eq!(source.triangles(), triangles);
    assert_eq!(original.canonical_record(), original_bytes);
    true
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stable_lifecycle_families() {
        for mode in 0..33 {
            assert_eq!(
                check_preparation_session(&[mode, mode & 1]),
                !matches!(mode, 1 | 2 | 3 | 4 | 7 | 9),
                "mode={mode}"
            );
        }
    }
}
