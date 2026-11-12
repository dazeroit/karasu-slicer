//! Explicit material-source policy, unchanged strict reporting and atomic publication.
use karasu_core::{
    cancellation::{Cancellation, Checkpoint},
    geometry::Point3,
    mesh::TriangleMesh,
    slicing::{
        construct::ConstructLimits, engine::*, grid::BinaryGridSpec, regions::RegionFillRule,
        schedule::*, SectionTransform,
    },
};

fn ribbons() -> TriangleMesh {
    let points = [
        [0., 0.],
        [4., 0.],
        [4., 4.],
        [0., 4.],
        [2., 2.],
        [6., 2.],
        [6., 6.],
        [2., 6.],
    ];
    let vertices = points
        .iter()
        .flat_map(|&[x, y]| [Point3::new(x, y, -1.), Point3::new(x, y, 1.)])
        .collect();
    let faces = [
        [0, 1],
        [1, 2],
        [2, 3],
        [3, 0],
        [4, 5],
        [5, 6],
        [6, 7],
        [7, 4],
    ]
    .into_iter()
    .flat_map(|[a, b]| [[2 * a, 2 * b, 2 * b + 1], [2 * a, 2 * b + 1, 2 * a + 1]])
    .collect();
    TriangleMesh::try_new(vertices, faces).unwrap()
}
fn plan() -> LayerSchedule {
    build_layer_schedule(
        UniformScheduleSpec::try_new(0., 1., ScheduleEnd::Count(1)).unwrap(),
        ScheduleLimits {
            max_layers: 1,
            max_payload_bytes: 1024,
        },
    )
    .unwrap()
}
fn config() -> LayerConfig {
    LayerConfig {
        grid: BinaryGridSpec::try_new([0., 0.], 0).unwrap(),
        fill_rule: RegionFillRule::EvenOddStrict,
        stages: LayerStageLimits::default(),
        max_output_payload_bytes: 1 << 24,
    }
}
fn engine<'a>(
    mesh: &'a TriangleMesh,
    plan: &'a LayerSchedule,
) -> LayerEngine<'a, 'static, 'static> {
    LayerEngine::new(
        mesh,
        plan,
        SectionTransform::Identity,
        config(),
        Cancellation::never(),
        None,
    )
}

#[test]
fn explicit_source_partition_changes_material_while_preserving_strict_reports() {
    let mesh = ribbons();
    let plan = plan();
    let strict = engine(&mesh, &plan).next().unwrap().unwrap();
    assert!(strict.material_union().is_none());
    let same = [7; 16];
    let mut distinct = [7; 16];
    distinct[8..].fill(9);
    let parity = engine(&mesh, &plan)
        .with_material_union(&same, ConstructLimits::default())
        .next()
        .unwrap()
        .unwrap();
    let union = engine(&mesh, &plan)
        .with_material_union(&distinct, ConstructLimits::default())
        .next()
        .unwrap()
        .unwrap();
    assert_ne!(
        parity.material_union().unwrap(),
        union.material_union().unwrap()
    );
    assert_eq!(union.completeness(), strict.completeness());
    assert_eq!(union.region_diagnostics(), strict.region_diagnostics());
    assert_eq!(union.raw(), strict.raw());
    assert_eq!(
        union.payload_bytes(),
        strict.payload_bytes() + union.material_union().unwrap().payload_bytes()
    );
    assert_eq!(union.material_union().unwrap().regions().len(), 1);
}

#[test]
fn face_label_count_and_mixed_path_are_typed_fused_failures() {
    let mesh = ribbons();
    let plan = plan();
    let mut short = engine(&mesh, &plan).with_material_union(&[], ConstructLimits::default());
    let error = short.next().unwrap().unwrap_err();
    assert_eq!(error.plane.unwrap().index, 0);
    assert!(matches!(
        error.cause,
        LayerFailure::Composition(LayerCompositionError::FaceMaterialCount {
            expected: 16,
            actual: 0
        })
    ));
    assert!(short.next().is_none());
    let mut labels = [0; 16];
    labels[0] = 1;
    let error = engine(&mesh, &plan)
        .with_material_union(&labels, ConstructLimits::default())
        .next()
        .unwrap()
        .unwrap_err();
    assert!(matches!(
        error.cause,
        LayerFailure::Composition(LayerCompositionError::MixedPathMaterials { .. })
    ));
}

#[test]
fn adapter_cancellation_preserves_plane_and_publishes_no_layer() {
    let mesh = ribbons();
    let plan = plan();
    let labels = [0; 16];
    let mut predicate = |point| point == Checkpoint::ConstructInput;
    let mut engine = LayerEngine::new(
        &mesh,
        &plan,
        SectionTransform::Identity,
        config(),
        Cancellation::new(&mut predicate),
        None,
    )
    .with_material_union(&labels, ConstructLimits::default());
    let error = engine.next().unwrap().unwrap_err();
    assert_eq!(error.plane.unwrap().index, 0);
    assert!(matches!(
        error.cause,
        LayerFailure::Composition(LayerCompositionError::Cancelled(_))
    ));
    assert_eq!(engine.completed_layers(), 0);
    assert!(engine.next().is_none());
}

#[test]
fn adapter_scratch_and_total_retention_are_bounded() {
    let mesh = ribbons();
    let plan = plan();
    let labels = [0; 16];
    let edge_limits = ConstructLimits {
        max_input_edges: 0,
        ..ConstructLimits::default()
    };
    let error = engine(&mesh, &plan)
        .with_material_union(&labels, edge_limits)
        .next()
        .unwrap()
        .unwrap_err();
    assert!(matches!(
        error.cause,
        LayerFailure::Composition(LayerCompositionError::InputEdgeLimit { limit: 0 })
    ));
    let limits = ConstructLimits {
        max_payload_bytes: 1,
        ..ConstructLimits::default()
    };
    let error = engine(&mesh, &plan)
        .with_material_union(&labels, limits)
        .next()
        .unwrap()
        .unwrap_err();
    assert!(matches!(
        error.cause,
        LayerFailure::Composition(LayerCompositionError::PayloadLimit { limit: 1 })
    ));
    let strict = engine(&mesh, &plan).next().unwrap().unwrap();
    let mut configuration = config();
    configuration.max_output_payload_bytes = strict.payload_bytes();
    let error = LayerEngine::new(
        &mesh,
        &plan,
        SectionTransform::Identity,
        configuration,
        Cancellation::never(),
        None,
    )
    .with_material_union(&labels, ConstructLimits::default())
    .next()
    .unwrap()
    .unwrap_err();
    assert!(matches!(error.cause, LayerFailure::OutputLimit { .. }));
}
