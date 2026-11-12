//! Bounded independent ownership, stream/collection and cancellation oracles.
use karasu_core::{
    cancellation::{Cancellation, Checkpoint},
    geometry::Point3,
    mesh::TriangleMesh,
    slicing::{
        engine::*,
        grid::BinaryGridSpec,
        regions::{RegionCompleteness, RegionFillRule},
        schedule::*,
        SectionTransform,
    },
};

fn byte(data: &[u8], n: usize) -> u8 {
    data.get(n).copied().unwrap_or(0)
}

fn inspect(layer: &RegionLayer, source_faces: usize) {
    assert!(layer.plane().z_mm.is_finite());
    assert!(layer.payload_bytes() <= 1024 * 1024);
    assert_eq!(layer.raw().summary().source_faces, source_faces);
    assert!(layer.raw().events().len() <= source_faces);
    assert!(layer
        .raw()
        .events()
        .iter()
        .all(|e| e.source_face < source_faces));
    let summary = layer.summary().regions;
    assert_eq!(summary.paths, layer.paths().len());
    assert_eq!(summary.boundaries, layer.boundaries().len());
    assert_eq!(summary.regions, layer.regions().len());
    assert_eq!(summary.paths, summary.boundaries + summary.non_boundaries);
    if layer.completeness() == RegionCompleteness::GeometricallyUnresolved {
        assert!(summary.non_boundaries > 0 || !layer.region_diagnostics().is_empty());
    }
    for boundary in layer.boundaries() {
        assert!(boundary.path < layer.paths().len());
        assert!(boundary.parent.is_none_or(|p| p < layer.paths().len()));
    }
    for id in 0..layer.vertices().len() {
        for &observation in layer.vertex_observations(id).expect("vertex observations") {
            assert!(observation < layer.observations().len());
        }
    }
    assert!(layer.vertex_observations(layer.vertices().len()).is_none());
    for id in 0..layer.edges().len() {
        for &segment in layer.edge_occurrences(id).expect("edge occurrences") {
            assert!(segment < layer.segments().len());
        }
    }
    for id in 0..layer.paths().len() {
        let vertices = layer.path_vertices(id).expect("path vertices");
        let edges = layer.path_edges(id).expect("path edges");
        assert!(vertices.iter().all(|&v| v < layer.vertices().len()));
        assert!(edges.iter().all(|&e| e < layer.edges().len()));
    }
    for (id, region) in layer.regions().iter().enumerate() {
        assert!(region.outer_boundary() < layer.boundaries().len());
        assert!(layer
            .region_holes(id)
            .expect("region holes")
            .iter()
            .all(|&h| h < layer.boundaries().len()));
    }
}

/// Decode at most sixteen finite indexed triangles and four planes, then check real public APIs.
/// Missing bytes are zero. Coordinates are quarter millimeters; arbitrary triangle interpolation
/// can intentionally fail exact-grid admission. Empty, coplanar, duplicate and open data are valid.
pub fn check_layer_engine(data: &[u8]) -> bool {
    let mode = byte(data, 0) % 4;
    let count = usize::from(byte(data, 1) % 17);
    let mut vertices = Vec::new();
    let mut faces = Vec::new();
    for id in 0..count {
        let base = vertices.len() as u32;
        for corner in 0..3 {
            let offset = 4 + id * 9 + corner * 3;
            let xyz = [0, 1, 2]
                .map(|axis| f64::from(i8::from_ne_bytes([byte(data, offset + axis)]) % 9) / 4.0);
            vertices.push(Point3::new(xyz[0], xyz[1], xyz[2]));
        }
        faces.push(if mode == 1 && id > 0 {
            [0, 1, 2]
        } else {
            [base, base + 1, base + 2]
        });
    }
    if mode == 2 && !faces.is_empty() {
        vertices = vec![
            Point3::new(0., 0., -1.),
            Point3::new(1., 0., 1.),
            Point3::new(0., 1., 1.),
        ];
        faces.fill([0, 1, 2]);
    }
    if mode == 3 {
        vertices = vec![
            Point3::new(0., 0., -1.),
            Point3::new(1., 0., -1.),
            Point3::new(1., 1., -1.),
            Point3::new(0., 1., -1.),
            Point3::new(0., 0., 1.),
            Point3::new(1., 0., 1.),
            Point3::new(1., 1., 1.),
            Point3::new(0., 1., 1.),
        ];
        faces.clear();
        for a in 0..4 {
            let b = (a + 1) % 4;
            faces.push([a, b, b + 4]);
            faces.push([a, b + 4, a + 4]);
        }
    }
    let mesh = TriangleMesh::try_new(vertices, faces).expect("bounded mesh");
    let original_vertices = mesh.vertices().to_vec();
    let original_faces = mesh.triangles().to_vec();
    let schedule = build_layer_schedule(
        UniformScheduleSpec::try_new(-0.5, 0.5, ScheduleEnd::Count(u64::from(byte(data, 2) % 5)))
            .expect("finite schedule"),
        ScheduleLimits {
            max_layers: 4,
            max_payload_bytes: 1024,
        },
    )
    .expect("bounded schedule");
    let mut stages = LayerStageLimits::default();
    stages.section.max_faces = 16;
    stages.section.max_events = 16;
    stages.grid.max_payload_bytes = 1024 * 1024;
    stages.validity.max_payload_bytes = 1024 * 1024;
    stages.regions.max_payload_bytes = 1024 * 1024;
    let config = LayerConfig {
        grid: BinaryGridSpec::try_new([0., 0.], -2 - i16::from(byte(data, 3) & 1))
            .expect("binary grid"),
        fill_rule: RegionFillRule::EvenOddStrict,
        stages,
        max_output_payload_bytes: 1024 * 1024,
    };
    let run = || {
        LayerEngine::new(
            &mesh,
            &schedule,
            SectionTransform::Identity,
            config,
            Cancellation::never(),
            None,
        )
        .collect::<Result<Vec<_>, _>>()
    };
    let first = run();
    assert_eq!(first, run());
    let collected = collect_layers(
        LayerEngine::new(
            &mesh,
            &schedule,
            SectionTransform::Identity,
            config,
            Cancellation::never(),
            None,
        ),
        CollectionLimits {
            max_layers: 4,
            max_payload_bytes: 8 * 1024 * 1024,
        },
    );
    match (&first, collected) {
        (Ok(layers), Ok(collection)) => {
            assert_eq!(layers, collection.layers());
            for (index, layer) in layers.iter().enumerate() {
                assert_eq!(layer.plane().index, index as u64);
                inspect(layer, mesh.triangles().len());
            }
        }
        (Err(error), Err(other)) => assert_eq!(*error, other),
        _ => panic!("stream and atomic collection disagree"),
    }
    let mut cancellation = |point| point == Checkpoint::LayerComplete;
    let mut cancelled = LayerEngine::new(
        &mesh,
        &schedule,
        SectionTransform::Identity,
        config,
        Cancellation::new(&mut cancellation),
        None,
    );
    if let Some(item) = cancelled.next() {
        assert!(
            item.is_err(),
            "publication cancellation must suppress every layer"
        );
        assert_eq!(cancelled.state(), StreamState::Failed);
        assert!(cancelled.next().is_none());
        assert!(cancelled.next().is_none());
    }
    assert_eq!(mesh.vertices(), original_vertices);
    assert_eq!(mesh.triangles(), original_faces);
    first.is_ok()
}
