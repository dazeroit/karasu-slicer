//! Bounded prepared drainage admission and one-layer material-difference invariants.
use karasu_core::{
    cancellation::{Cancellation, Checkpoint},
    geometry::{ObjectTransform, Point3},
    mesh::TriangleMesh,
    preparation::{
        drainage::{drain_model, DrainageLimits},
        hollow::HollowLimits,
        HoleSpec, HollowSpec, Operation, OperationKind, PlanLimits, PreparationPlan, SamplingSpec,
        SourceBinding,
    },
    slicing::{
        construct::ConstructLimits,
        engine::{LayerConfig, LayerStageLimits},
        grid::BinaryGridSpec,
        regions::RegionFillRule,
        schedule::{build_layer_schedule, ScheduleEnd, ScheduleLimits, UniformScheduleSpec},
    },
};

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

/// Exercise immutable scene construction and exact section subtraction under independent small caps.
/// Errors are expected for malformed, unsupported, overlapping, shallow and resource-limited inputs.
pub fn check_drainage(data: &[u8]) -> bool {
    if data.len() > 256 {
        return false;
    }
    let source = cube();
    let original_vertices = source.vertices().to_vec();
    let original_faces = source.triangles().to_vec();
    let mode = byte(data, 0) % 10;
    let axis = usize::from(byte(data, 1) % 3);
    let sign = if byte(data, 2).is_multiple_of(2) {
        1.0
    } else {
        -1.0
    };
    let mut direction = [0.; 3];
    direction[axis] = sign;
    let mut entry_mm = [4.; 3];
    entry_mm[axis] = if sign > 0. { -1. } else { 9. };
    let mut hole = HoleSpec {
        entry_mm,
        direction,
        length_mm: 3.,
        diameter_mm: 1.,
    };
    match mode {
        1 => hole.length_mm = 1.5,
        2 => hole.length_mm = 0.5,
        3 => hole.direction[(axis + 1) % 3] = 1.,
        4 => hole.diameter_mm = f64::NAN,
        5 => hole.diameter_mm = 12.,
        _ => {}
    }
    if byte(data, 3) & 1 != 0 {
        hole.entry_mm[(axis + 1) % 3] = f64::from(byte(data, 4)) / 16.;
        hole.length_mm = f64::from(byte(data, 5)) / 16.;
    }
    let mut operations = vec![Operation {
        id: 1,
        kind: OperationKind::Hollow(HollowSpec { thickness_mm: 1. }),
    }];
    if mode != 7 {
        operations.push(Operation {
            id: 2,
            kind: OperationKind::Hole(hole),
        });
    }
    if mode == 6 {
        operations.push(Operation {
            id: 3,
            kind: OperationKind::Hole(hole),
        });
    }
    let Ok(plan) = PreparationPlan::new(
        SourceBinding {
            original_sha256: [56; 32],
            placement: ObjectTransform::identity(),
        },
        SamplingSpec {
            cell_mm: 0.1,
            distance_error_mm: 0.002,
            max_cells: 1,
        },
        operations,
        PlanLimits::default(),
        &mut Cancellation::never(),
    ) else {
        return false;
    };
    let original_record = plan.canonical_record().to_vec();
    let limits = DrainageLimits {
        hollow: HollowLimits {
            max_input_vertices: 16,
            max_input_triangles: 32,
            max_planes: 32,
            max_output_vertices: 256,
            max_output_triangles: 512,
            max_work: 200_000,
            max_payload_bytes: 2 * 1024 * 1024,
        },
        max_holes: 2,
        max_segments: if mode == 9 { 4 } else { 64 },
        max_work: 200_000,
        max_payload_bytes: 4 * 1024 * 1024,
        minimum_clearance_mm: 1e-6,
    };
    let mut predicate = |point| mode == 8 && point == Checkpoint::DrainagePublish;
    let result = drain_model(
        &source,
        &plan,
        limits,
        &mut Cancellation::new(&mut predicate),
    );
    assert_eq!(source.vertices(), original_vertices);
    assert_eq!(source.triangles(), original_faces);
    assert_eq!(plan.canonical_record(), original_record);
    let Ok(scene) = result else {
        return false;
    };
    assert_eq!(scene.plan_record(), original_record);
    assert_eq!(scene.openings().len(), usize::from(mode != 7));
    assert_eq!(scene.enclosed_cavities(), usize::from(mode == 7));
    for opening in scene.openings() {
        assert!(opening.radial_error_mm.is_finite() && opening.radial_error_mm >= 0.);
        assert!(opening.axial_error_mm.is_finite() && opening.axial_error_mm >= 0.);
        assert!(opening.radial_error_mm + opening.axial_error_mm <= 0.002);
        assert!(opening.terminal_clearance_mm > limits.minimum_clearance_mm);
        assert!(opening.segments <= limits.max_segments);
    }
    let height = 0.25 + f64::from(byte(data, 6) % 32) / 4.;
    let schedule = build_layer_schedule(
        UniformScheduleSpec::try_new(height, 1., ScheduleEnd::Count(1)).unwrap(),
        ScheduleLimits {
            max_layers: 1,
            max_payload_bytes: 1024,
        },
    )
    .unwrap();
    let config = LayerConfig {
        grid: BinaryGridSpec::try_new([0.; 2], 0).unwrap(),
        fill_rule: RegionFillRule::EvenOddStrict,
        stages: LayerStageLimits::default(),
        max_output_payload_bytes: 4 * 1024 * 1024,
    };
    let construct = ConstructLimits {
        max_input_edges: 512,
        max_pairs: 100_000,
        max_vertices: 2048,
        max_atoms: 4096,
        max_work: 200_000,
        max_payload_bytes: 4 * 1024 * 1024,
    };
    let mut engine = scene.layer_engine(&schedule, config, construct, Cancellation::never(), None);
    if let Some(Ok(layer)) = engine.next() {
        assert!(layer.material_union().is_some());
    }
    assert!(engine.next().is_none());
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixed_acceptance_and_failure_families() {
        for axis in 0..3 {
            for sign in 0..2 {
                assert!(check_drainage(&[0, axis, sign]));
            }
        }
        assert!(check_drainage(&[7]));
        for mode in [1, 2, 3, 4, 5, 6, 8, 9] {
            assert!(!check_drainage(&[mode]));
        }
    }
}
