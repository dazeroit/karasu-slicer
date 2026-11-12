//! Grid-independent topology and lossless dyadic construction regressions.
use karasu_core::{
    cancellation::{Cancellation, Checkpoint},
    geometry::Point3,
    mesh::TriangleMesh,
    slicing::{construct::*, engine::*, grid::*, regions::*, schedule::*, SectionTransform},
};

fn plan() -> LayerSchedule {
    build_layer_schedule(
        UniformScheduleSpec::try_new(-0.75, 0.25, ScheduleEnd::Count(7)).unwrap(),
        ScheduleLimits {
            max_layers: 100,
            max_payload_bytes: 4096,
        },
    )
    .unwrap()
}
fn mesh(polygons: &[Vec<[f64; 2]>]) -> TriangleMesh {
    let mut vertices = Vec::new();
    let mut faces = Vec::new();
    for polygon in polygons {
        let start = u32::try_from(vertices.len()).unwrap();
        for &[x, y] in polygon {
            vertices.extend([Point3::new(x, y, -1.0), Point3::new(x, y, 1.0)]);
        }
        for i in 0..polygon.len() {
            let a = start + 2 * u32::try_from(i).unwrap();
            let b = start + 2 * u32::try_from((i + 1) % polygon.len()).unwrap();
            faces.extend([[a, b, b + 1], [a, b + 1, a + 1]]);
        }
    }
    TriangleMesh::try_new(vertices, faces).unwrap()
}
fn rectangle(lo: f64, hi: f64) -> Vec<[f64; 2]> {
    vec![[lo, lo], [hi, lo], [hi, hi], [lo, hi]]
}
fn config() -> LayerConfig {
    LayerConfig {
        grid: BinaryGridSpec::try_new([0.0, 0.0], 0).unwrap(),
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
    .with_represented_coordinates()
}
#[test]
fn decimal_nested_ribbons_slice_at_every_quarter_plane_without_snapping() {
    let mesh = mesh(&[
        rectangle(0.1, 4.1),
        rectangle(1.1, 3.1),
        rectangle(1.6, 2.6),
    ]);
    let plan = plan();
    let legacy = LayerEngine::new(
        &mesh,
        &plan,
        SectionTransform::Identity,
        config(),
        Cancellation::never(),
        None,
    )
    .next()
    .unwrap();
    assert!(matches!(
        legacy,
        Err(LayerEngineError {
            cause: LayerFailure::Grid(_),
            ..
        })
    ));
    let first: Vec<_> = engine(&mesh, &plan).map(Result::unwrap).collect();
    let second: Vec<_> = engine(&mesh, &plan).map(Result::unwrap).collect();
    assert_eq!(first, second);
    assert_eq!(first.len(), 7);
    for layer in &first {
        assert!(layer.uses_represented_coordinates());
        assert_eq!(layer.completeness(), RegionCompleteness::Complete);
        assert_eq!(layer.regions().len(), 2);
        assert_eq!(layer.boundaries().len(), 3);
        assert_eq!(layer.summary().grid.exact_vertices, 0);
        assert_eq!(layer.summary().grid.axis_conversions, 0);
        assert_eq!(
            layer.summary().grid.represented_vertices,
            layer.vertices().len()
        );
        for vertex in layer.vertices() {
            assert_eq!(vertex.grid, GridVertexOutcome::Represented);
        }
    }
}
#[test]
fn quarter_plane_material_union_retains_scale_provenance_and_strict_contact_diagnostics() {
    let mesh = mesh(&[rectangle(0.1, 4.1), rectangle(2.1, 6.1)]);
    let plan = plan();
    let mut labels = vec![0; 16];
    labels[8..].fill(1);
    let baseline: Vec<_> = engine(&mesh, &plan).map(Result::unwrap).collect();
    let composed: Vec<_> = engine(&mesh, &plan)
        .with_material_union(&labels, ConstructLimits::default())
        .map(Result::unwrap)
        .collect();
    for (strict, layer) in baseline.iter().zip(&composed) {
        assert_eq!(strict.raw(), layer.raw());
        assert_eq!(strict.region_diagnostics(), layer.region_diagnostics());
        assert_eq!(
            layer.completeness(),
            RegionCompleteness::GeometricallyUnresolved
        );
        let union = layer.material_union().unwrap();
        assert!(union.dyadic_exponent_mm().is_some());
        assert_eq!(union.regions().len(), 1);
        assert_eq!(union.rings().len(), 1);
        for atom in 0..union.atoms().len() {
            assert_ne!(union.atom_supports(atom).unwrap(), []);
        }
    }
}
#[test]
fn represented_mode_preserves_resource_and_cancellation_failures() {
    let mesh = mesh(&[rectangle(0.1, 4.1)]);
    let plan = plan();
    let mut cfg = config();
    cfg.stages.grid.max_axis_conversions = 0;
    let mut stream = LayerEngine::new(
        &mesh,
        &plan,
        SectionTransform::Identity,
        cfg,
        Cancellation::never(),
        None,
    )
    .with_represented_coordinates();
    assert!(stream.next().unwrap().is_ok());
    cfg.stages.grid.max_vertices = 0;
    let mut stream = LayerEngine::new(
        &mesh,
        &plan,
        SectionTransform::Identity,
        cfg,
        Cancellation::never(),
        None,
    )
    .with_represented_coordinates();
    assert!(matches!(
        stream.next().unwrap(),
        Err(LayerEngineError {
            cause: LayerFailure::Grid(GridError::LimitExceeded {
                resource: GridResource::Vertices,
                ..
            }),
            ..
        })
    ));
    assert!(stream.next().is_none());
    for target in [
        Checkpoint::GridStart,
        Checkpoint::GridStorage,
        Checkpoint::GridRecords,
        Checkpoint::GridComplete,
        Checkpoint::RegionsStart,
        Checkpoint::ConstructInput,
    ] {
        let mut hook = |checkpoint| checkpoint == target;
        let mut stream = LayerEngine::new(
            &mesh,
            &plan,
            SectionTransform::Identity,
            config(),
            Cancellation::new(&mut hook),
            None,
        )
        .with_represented_coordinates()
        .with_material_union(&[0; 8], ConstructLimits::default());
        assert!(stream.next().unwrap().is_err(), "{target:?}");
        assert!(stream.next().is_none());
    }
}
#[test]
fn represented_construction_matches_independently_scaled_integer_inputs() {
    let originals = [
        vec![[0, 0], [8, 0], [8, 8], [0, 8]],
        vec![[3, -2], [10, 3], [3, 10], [-2, 3]],
    ];
    for exponent in [-1000, -8, 0, 900] {
        let factor = 2_f64.powi(exponent);
        let represented: Vec<Vec<_>> = originals
            .iter()
            .map(|ring| {
                ring.iter()
                    .map(|p| [f64::from(p[0]) * factor, f64::from(p[1]) * factor])
                    .collect()
            })
            .collect();
        let inputs: Vec<_> = represented
            .iter()
            .enumerate()
            .map(|(i, vertices)| RepresentedMaterialRing {
                vertices,
                material: u64::try_from(i).unwrap(),
                source_path: i,
            })
            .collect();
        let actual = union_represented_material_rings(&inputs, ConstructLimits::default()).unwrap();
        let integers: Vec<Vec<_>> = originals
            .iter()
            .map(|ring| ring.iter().map(|p| p.map(i64::from)).collect())
            .collect();
        let inputs: Vec<_> = integers
            .iter()
            .enumerate()
            .map(|(i, vertices)| MaterialRing {
                vertices,
                material: u64::try_from(i).unwrap(),
                source_path: i,
            })
            .collect();
        let expected = union_material_rings(&inputs, ConstructLimits::default()).unwrap();
        assert_eq!(
            actual.dyadic_exponent_mm(),
            Some(i16::try_from(exponent).unwrap())
        );
        assert_eq!(actual.vertices(), expected.vertices());
        assert_eq!(actual.edges(), expected.edges());
        assert_eq!(actual.rings(), expected.rings());
        assert_eq!(actual.regions(), expected.regions());
    }
}
#[test]
fn unsupported_dynamic_range_and_nonfinite_input_are_explicit_atomic_errors() {
    for (value, reason) in [
        (f64::from_bits(1), ConstructInputIssue::DyadicRange),
        (f64::NAN, ConstructInputIssue::NonFiniteCoordinate),
        (f64::INFINITY, ConstructInputIssue::NonFiniteCoordinate),
    ] {
        let points = [[value, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
        let ring = RepresentedMaterialRing {
            vertices: &points,
            material: 0,
            source_path: 37,
        };
        assert_eq!(
            union_represented_material_rings(&[ring], ConstructLimits::default()),
            Err(ConstructError::InvalidRing { path: 37, reason })
        );
    }
    let points = rectangle(0.1, 1.1);
    let ring = RepresentedMaterialRing {
        vertices: &points,
        material: 0,
        source_path: 0,
    };
    let limits = ConstructLimits {
        max_payload_bytes: 0,
        ..ConstructLimits::default()
    };
    assert!(matches!(
        union_represented_material_rings(&[ring], limits),
        Err(ConstructError::LimitExceeded {
            resource: ConstructResource::PayloadBytes,
            ..
        })
    ));
}

#[test]
fn sixty_bit_encoding_extremes_exercise_rational_topology_without_overflow() {
    let bound = 2_f64.powi(60);
    let first = [[-bound, -bound], [bound, bound - 128.0], [bound, -bound]];
    let second = [[-bound, bound], [bound, -bound + 128.0], [-bound, -bound]];
    // The small third source forces exponent zero rather than letting the large triangles
    // reduce to tiny integer coordinates through a common power-of-two factor.
    let third = [[1.0, 0.0], [2.0, 0.0], [1.0, 2.0]];
    let rings: Vec<_> = [&first[..], &second[..], &third[..]]
        .into_iter()
        .enumerate()
        .map(|(i, vertices)| RepresentedMaterialRing {
            vertices,
            material: u64::try_from(i).unwrap(),
            source_path: i,
        })
        .collect();
    let first = union_represented_material_rings(&rings, ConstructLimits::default()).unwrap();
    let second = union_represented_material_rings(&rings, ConstructLimits::default()).unwrap();
    assert_eq!(first, second);
    assert_eq!(first.dyadic_exponent_mm(), Some(0));
    assert_eq!(first.regions().len(), 1);
    assert!(first.vertices().iter().any(|p| p.denominator()[1] != 0));
}

#[test]
fn lossless_constructor_preserves_conflict_validation_and_independent_limits() {
    let points = rectangle(0.1, 1.1);
    let ring = RepresentedMaterialRing {
        vertices: &points,
        material: 0,
        source_path: 11,
    };
    for (limits, resource) in [
        (
            ConstructLimits {
                max_input_edges: 3,
                ..ConstructLimits::default()
            },
            ConstructResource::InputEdges,
        ),
        (
            ConstructLimits {
                max_work: 0,
                ..ConstructLimits::default()
            },
            ConstructResource::Work,
        ),
        (
            ConstructLimits {
                max_pairs: 0,
                ..ConstructLimits::default()
            },
            ConstructResource::Pairs,
        ),
        (
            ConstructLimits {
                max_vertices: 0,
                ..ConstructLimits::default()
            },
            ConstructResource::Vertices,
        ),
    ] {
        assert!(
            matches!(union_represented_material_rings(&[ring], limits), Err(ConstructError::LimitExceeded { resource: actual, .. }) if actual == resource)
        );
    }
    assert_eq!(
        union_represented_material_rings(&[ring, ring], ConstructLimits::default()),
        Err(ConstructError::InvalidRing {
            path: 11,
            reason: ConstructInputIssue::DuplicatePath
        })
    );
    let points = [[0.1, 0.1], [1.1, 1.1], [0.1, 1.1], [1.1, 0.1]];
    let ring = RepresentedMaterialRing {
        vertices: &points,
        material: 0,
        source_path: 11,
    };
    assert_eq!(
        union_represented_material_rings(&[ring], ConstructLimits::default()),
        Err(ConstructError::InvalidRing {
            path: 11,
            reason: ConstructInputIssue::SelfContact
        })
    );
}

#[test]
fn grid_independent_ray_work_remains_cancellable_and_bounded() {
    let mesh = mesh(&[rectangle(0.1, 4.1), rectangle(1.1, 3.1)]);
    let plan = plan();
    let mut hook = |checkpoint| checkpoint == Checkpoint::RegionsPairs;
    let mut stream = LayerEngine::new(
        &mesh,
        &plan,
        SectionTransform::Identity,
        config(),
        Cancellation::new(&mut hook),
        None,
    )
    .with_represented_coordinates();
    assert!(matches!(
        stream.next().unwrap(),
        Err(LayerEngineError {
            cause: LayerFailure::Regions(RegionError::Cancelled(_)),
            ..
        })
    ));
    assert!(stream.next().is_none());
    let mut cfg = config();
    cfg.stages.regions.max_ray_tests = 0;
    let mut stream = LayerEngine::new(
        &mesh,
        &plan,
        SectionTransform::Identity,
        cfg,
        Cancellation::never(),
        None,
    )
    .with_represented_coordinates();
    assert!(matches!(
        stream.next().unwrap(),
        Err(LayerEngineError {
            cause: LayerFailure::Regions(RegionError::LimitExceeded {
                resource: RegionResource::RayTests,
                ..
            }),
            ..
        })
    ));
    assert!(stream.next().is_none());
}
