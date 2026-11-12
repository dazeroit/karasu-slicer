//! Analytic public-engine, provenance, lifecycle and atomic-collection regressions.
use karasu_core::slicing::{
    area::*, engine::*, graph::*, grid::*, normalize::*, paths::*, regions::*, schedule::*,
    validity::*, *,
};
use karasu_core::{
    cancellation::{Cancellation, Cancelled, Checkpoint},
    geometry::{Affine3, Point3, Vector3},
    mesh::TriangleMesh,
};
use std::error::Error;

fn ribbon(points: &[[f64; 2]], links: &[[u32; 2]]) -> TriangleMesh {
    TriangleMesh::try_new(
        points
            .iter()
            .flat_map(|&[x, y]| [Point3::new(x, y, -1.), Point3::new(x, y, 1.)])
            .collect(),
        links
            .iter()
            .flat_map(|&[a, b]| [[2 * a, 2 * b, 2 * b + 1], [2 * a, 2 * b + 1, 2 * a + 1]])
            .collect(),
    )
    .unwrap()
}
fn square() -> TriangleMesh {
    ribbon(
        &[[0., 0.], [4., 0.], [4., 4.], [0., 4.]],
        &[[0, 1], [1, 2], [2, 3], [3, 0]],
    )
}
fn schedule(first: f64, step: f64, count: u64) -> LayerSchedule {
    build_layer_schedule(
        UniformScheduleSpec::try_new(first, step, ScheduleEnd::Count(count)).unwrap(),
        ScheduleLimits {
            max_layers: count,
            max_payload_bytes: 1 << 20,
        },
    )
    .unwrap()
}
fn config() -> LayerConfig {
    LayerConfig {
        grid: BinaryGridSpec::try_new([0., 0.], -2).unwrap(),
        fill_rule: RegionFillRule::EvenOddStrict,
        stages: LayerStageLimits::default(),
        max_output_payload_bytes: 1 << 24,
    }
}
fn limits() -> CollectionLimits {
    CollectionLimits {
        max_layers: 100,
        max_payload_bytes: 1 << 26,
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

fn compare_pipeline(mesh: &TriangleMesh, layer: &RegionLayer, transform: SectionTransform<'_>) {
    let configuration = config();
    let raw = section_mesh_z(
        mesh,
        layer.plane().z_mm,
        transform,
        configuration.stages.section,
    )
    .unwrap();
    let normalized = normalize_section(&raw, configuration.stages.normalization).unwrap();
    let graph = build_incidence_graph(&normalized, configuration.stages.graph).unwrap();
    let paths = extract_paths(&graph, configuration.stages.paths).unwrap();
    let inspection = inspect_planar_paths(&paths, configuration.stages.validity).unwrap();
    let areas = measure_cycle_areas(&inspection, configuration.stages.area).unwrap();
    let grid = map_exact_grid_vertices(&inspection, configuration.grid, configuration.stages.grid)
        .unwrap();
    let regions =
        assemble_strict_regions(&inspection, &areas, &grid, configuration.stages.regions).unwrap();
    assert_eq!(layer.raw(), &raw);
    assert_eq!(
        layer.summary(),
        LayerSummary {
            normalization: normalized.summary(),
            graph: graph.summary(),
            paths: paths.summary(),
            validity: inspection.summary(),
            area: areas.summary(),
            grid: grid.summary(),
            regions: regions.summary()
        }
    );
    assert_eq!(layer.observations(), normalized.observations());
    assert_eq!(layer.segments(), normalized.segments());
    assert_eq!(layer.graph_diagnostics(), graph.diagnostics());
    assert_eq!(layer.path_diagnostics(), paths.diagnostics());
    assert_eq!(layer.planar_diagnostics(), inspection.diagnostics());
    assert_eq!(layer.region_diagnostics(), regions.diagnostics());
    assert_eq!(layer.components(), paths.components());
    assert_eq!(layer.boundaries(), regions.boundaries());
    for (id, vertex) in layer.vertices().iter().enumerate() {
        assert_eq!(vertex.feature, normalized.features()[id].feature);
        assert_eq!(vertex.coordinates, normalized.features()[id].coordinates);
        assert_eq!(vertex.degrees, graph.vertices()[id].degrees);
        assert_eq!(vertex.grid, grid.vertex(id).unwrap());
        assert_eq!(
            layer.vertex_observations(id).unwrap(),
            &normalized.feature_order()[normalized.features()[id].observations.clone()]
        );
    }
    for (id, edge) in layer.edges().iter().enumerate() {
        assert_eq!(edge.graph, graph.edges()[id]);
        assert_eq!(edge.ownership, normalized.groups()[id].ownership);
        assert_eq!(edge.issues, normalized.groups()[id].issues);
        assert_eq!(edge.source_faces, normalized.groups()[id].source_faces);
        assert_eq!(edge.outcome, paths.edge_outcome(id).unwrap());
        assert_eq!(
            layer.edge_occurrences(id).unwrap(),
            &normalized.segment_order()[normalized.groups()[id].occurrences.clone()]
        );
    }
    for (id, path) in layer.paths().iter().enumerate() {
        let original = &paths.paths()[id];
        assert_eq!(
            (
                path.component,
                path.kind,
                path.closed,
                path.issues,
                path.endpoints
            ),
            (
                original.component,
                original.kind,
                original.closed,
                original.issues,
                original.endpoints
            )
        );
        assert_eq!(path.embedding, inspection.embeddings()[id]);
        assert_eq!(path.area, areas.outcomes()[id]);
        assert_eq!(path.region_outcome, regions.path_outcomes()[id]);
        assert_eq!(layer.path_vertices(id), paths.path_vertices(id));
        assert_eq!(layer.path_edges(id), paths.path_edges(id));
    }
    for (id, region) in layer.regions().iter().enumerate() {
        assert_eq!(
            region.outer_boundary(),
            regions.regions()[id].outer_boundary()
        );
        assert_eq!(layer.region_holes(id), regions.region_holes(id));
    }
    assert_eq!(layer.completeness(), regions.completeness());
}

// Explicit fixed-width little-endian words and IEEE bits: independent of Debug and host usize width.
fn encode(layer: &RegionLayer) -> Vec<u8> {
    let mut out = Vec::new();
    let mut word = |n: u64| out.extend_from_slice(&n.to_le_bytes());
    word(layer.plane().index);
    word(layer.plane().z_mm.to_bits());
    word(layer.plane().nominal_height_mm.to_bits());
    for size in [
        layer.raw().events().len(),
        layer.vertices().len(),
        layer.edges().len(),
        layer.paths().len(),
        layer.boundaries().len(),
        layer.regions().len(),
    ] {
        word(u64::try_from(size).unwrap());
    }
    for (id, vertex) in layer.vertices().iter().enumerate() {
        match vertex.coordinates {
            CoordinateAgreement::Agreed(p) => {
                word(0);
                for x in [p.x, p.y, p.z] {
                    word(x.to_bits());
                }
            }
            CoordinateAgreement::Conflict => word(1),
        }
        for &ob in layer.vertex_observations(id).unwrap() {
            word(u64::try_from(ob).unwrap());
        }
    }
    for id in 0..layer.edges().len() {
        for &occ in layer.edge_occurrences(id).unwrap() {
            word(u64::try_from(occ).unwrap());
        }
    }
    for id in 0..layer.paths().len() {
        for &v in layer.path_vertices(id).unwrap() {
            word(u64::try_from(v).unwrap());
        }
        for &e in layer.path_edges(id).unwrap() {
            word(u64::try_from(e).unwrap());
        }
    }
    out
}

#[test]
fn analytic_layers_match_individual_stages_and_preserve_every_flat_reference() {
    let mesh = square();
    let plan = schedule(-0.5, 0.5, 3);
    let first = collect_layers(engine(&mesh, &plan), limits()).unwrap();
    for (i, layer) in first.layers().iter().enumerate() {
        assert_eq!(layer.plane(), plan.planes()[i]);
        assert_eq!(layer.completeness(), RegionCompleteness::Complete);
        assert_eq!(layer.regions().len(), 1);
        assert_eq!(layer.paths().len(), 1);
        compare_pipeline(&mesh, layer, SectionTransform::Identity);
    }
    let second = collect_layers(engine(&mesh, &plan), limits()).unwrap();
    assert_eq!(
        first.layers().iter().map(encode).collect::<Vec<_>>(),
        second.layers().iter().map(encode).collect::<Vec<_>>()
    );
}

#[test]
fn nested_and_disconnected_regions_keep_analytic_hole_forest() {
    let mesh = ribbon(
        &[
            [0., 0.],
            [12., 0.],
            [12., 12.],
            [0., 12.],
            [2., 2.],
            [10., 2.],
            [10., 10.],
            [2., 10.],
            [4., 4.],
            [8., 4.],
            [8., 8.],
            [4., 8.],
            [20., 0.],
            [24., 0.],
            [24., 4.],
            [20., 4.],
        ],
        &(0..4)
            .flat_map(|c| (0..4).map(move |i| [4 * c + i, 4 * c + (i + 1) % 4]))
            .collect::<Vec<_>>(),
    );
    let plan = schedule(-0.5, 0.5, 3);
    for layer in engine(&mesh, &plan) {
        let layer = layer.unwrap();
        compare_pipeline(&mesh, &layer, SectionTransform::Identity);
        assert_eq!(layer.regions().len(), 3);
        assert_eq!(
            layer
                .boundaries()
                .iter()
                .map(|b| b.depth)
                .collect::<Vec<_>>(),
            [0, 1, 2, 0],
            "z={} boundaries={:?}",
            layer.plane().z_mm,
            layer.boundaries()
        );
        assert_eq!(layer.region_holes(0).unwrap().len(), 1);
    }
}

#[test]
fn owned_outputs_outlive_mesh_schedule_and_transform_without_source_mutation() {
    let layer = {
        let mesh = square();
        let original = square();
        let plan = schedule(0., 1., 1);
        let transform = Affine3::from_translation(Vector3::new(8., -4., 0.)).unwrap();
        let mut stream = LayerEngine::new(
            &mesh,
            &plan,
            SectionTransform::Affine(&transform),
            config(),
            Cancellation::never(),
            None,
        );
        let layer = stream.next().unwrap().unwrap();
        compare_pipeline(&mesh, &layer, SectionTransform::Affine(&transform));
        assert_eq!(mesh.vertices(), original.vertices());
        assert_eq!(mesh.triangles(), original.triangles());
        layer
    };
    assert_eq!(layer.regions().len(), 1);
    assert!(layer
        .observations()
        .iter()
        .all(|o| o.position.x >= 8. && o.position.y >= -4.));
    assert_ne!(encode(&layer).len(), 0);
}

#[test]
fn completion_requires_exhaustion_and_progress_is_monotonic() {
    let mesh = square();
    let plan = schedule(0., 0.5, 2);
    let mut events = Vec::new();
    {
        let mut callback = |e| events.push(e);
        let mut stream = LayerEngine::new(
            &mesh,
            &plan,
            SectionTransform::Identity,
            config(),
            Cancellation::never(),
            Some(&mut callback),
        );
        assert_eq!(stream.state(), StreamState::NotStarted);
        assert_eq!(stream.size_hint(), (0, Some(3)));
        for _ in 0..2 {
            stream.next().unwrap().unwrap();
            assert_eq!(stream.state(), StreamState::Running);
        }
        assert_eq!(stream.completed_layers(), 2);
        assert_eq!(stream.size_hint(), (0, Some(1)));
        assert!(stream.next().is_none());
        assert_eq!(stream.state(), StreamState::Complete);
        for _ in 0..3 {
            assert!(stream.next().is_none());
        }
        assert_eq!(stream.size_hint(), (0, Some(0)));
    }
    assert_eq!(
        events.iter().map(|e| e.kind).collect::<Vec<_>>(),
        [
            LayerProgressKind::Started,
            LayerProgressKind::LayerStarted,
            LayerProgressKind::LayerCompleted,
            LayerProgressKind::LayerStarted,
            LayerProgressKind::LayerCompleted,
            LayerProgressKind::Completed
        ]
    );
    assert_eq!(
        events
            .iter()
            .map(|e| e.completed_layers)
            .collect::<Vec<_>>(),
        [0, 0, 1, 1, 2, 2]
    );
    assert!(events
        .iter()
        .all(|e| e.total_layers == 2 && e.stage == LayerProgressStage::M3LayerGeneration));
    assert_eq!(
        events.iter().map(|e| e.current_layer).collect::<Vec<_>>(),
        [None, Some(0), Some(0), Some(1), Some(1), None]
    );
}

#[test]
fn early_drop_and_empty_stream_have_distinct_completion() {
    let mesh = square();
    for count in [0, 1] {
        let plan = schedule(0., 1., count);
        let mut events = Vec::new();
        {
            let mut callback = |e: LayerProgress| events.push(e.kind);
            let mut stream = LayerEngine::new(
                &mesh,
                &plan,
                SectionTransform::Identity,
                config(),
                Cancellation::never(),
                Some(&mut callback),
            );
            if count == 0 {
                assert!(stream.next().is_none());
            } else {
                stream.next().unwrap().unwrap();
            }
        }
        assert_eq!(events.contains(&LayerProgressKind::Completed), count == 0);
    }
}

#[test]
fn orchestration_and_stage_cancellation_preserve_context_and_are_fused() {
    let mesh = square();
    let plan = schedule(0., 1., 1);
    for point in [
        Checkpoint::LayersStart,
        Checkpoint::LayerStart,
        Checkpoint::SectionStart,
        Checkpoint::NormalizationStart,
        Checkpoint::GraphStart,
        Checkpoint::PathsStart,
        Checkpoint::ValidityStart,
        Checkpoint::AreaStart,
        Checkpoint::GridStart,
        Checkpoint::RegionsStart,
        Checkpoint::LayerStorage,
        Checkpoint::LayerOutput,
        Checkpoint::LayerComplete,
        Checkpoint::LayersComplete,
    ] {
        let mut predicate = |p| p == point;
        let mut events = Vec::new();
        {
            let mut callback = |e: LayerProgress| events.push(e.kind);
            let mut stream = LayerEngine::new(
                &mesh,
                &plan,
                SectionTransform::Identity,
                config(),
                Cancellation::new(&mut predicate),
                Some(&mut callback),
            );
            if point == Checkpoint::LayersComplete {
                stream.next().unwrap().unwrap();
            }
            let error = stream.next().unwrap().unwrap_err();
            assert_eq!(
                error.plane.is_none(),
                matches!(point, Checkpoint::LayersStart | Checkpoint::LayersComplete)
            );
            assert!(error.to_string().contains("cancelled"));
            assert!(error.source().unwrap().source().is_some());
            let observed = match error.cause {
                LayerFailure::Section(SectionError::Cancelled(c))
                | LayerFailure::Normalization(NormalizationError::Cancelled(c))
                | LayerFailure::Graph(GraphError::Cancelled(c))
                | LayerFailure::Paths(PathError::Cancelled(c))
                | LayerFailure::Validity(ValidityError::Cancelled(c))
                | LayerFailure::Area(AreaError::Cancelled(c))
                | LayerFailure::Grid(GridError::Cancelled(c))
                | LayerFailure::Regions(RegionError::Cancelled(c))
                | LayerFailure::Cancelled(c) => c,
                other => panic!("expected structured cancellation, got {other:?}"),
            };
            assert_eq!(observed.checkpoint, point);
            assert_eq!(stream.state(), StreamState::Failed);
            assert!(stream.next().is_none());
            assert!(stream.next().is_none());
        }
        assert_eq!(events.last(), Some(&LayerProgressKind::Failed));
        assert!(!events.contains(&LayerProgressKind::Completed));
    }
}

#[test]
fn each_underlying_stage_resource_error_is_preserved() {
    let mesh = square();
    let plan = schedule(0., 1., 1);
    for stage in 0..8 {
        let mut c = config();
        match stage {
            0 => c.stages.section.max_faces = 0,
            1 => c.stages.normalization.max_events = 0,
            2 => c.stages.graph.max_vertices = 0,
            3 => c.stages.paths.max_graph_vertices = 0,
            4 => c.stages.validity.max_graph_vertices = 0,
            5 => c.stages.area.max_paths = 0,
            6 => c.stages.grid.max_vertices = 0,
            7 => c.stages.regions.max_paths = 0,
            _ => unreachable!(),
        }
        let mut stream = LayerEngine::new(
            &mesh,
            &plan,
            SectionTransform::Identity,
            c,
            Cancellation::never(),
            None,
        );
        let error = stream.next().unwrap().unwrap_err();
        assert_eq!(error.plane, Some(plan.planes()[0]));
        assert!(matches!(
            (&error.cause, stage),
            (LayerFailure::Section(_), 0)
                | (LayerFailure::Normalization(_), 1)
                | (LayerFailure::Graph(_), 2)
                | (LayerFailure::Paths(_), 3)
                | (LayerFailure::Validity(_), 4)
                | (LayerFailure::Area(_), 5)
                | (LayerFailure::Grid(_), 6)
                | (LayerFailure::Regions(_), 7)
        ));
        assert!(error.source().unwrap().source().is_some());
        assert!(stream.next().is_none());
    }
}

#[test]
fn collectors_enforce_caps_freshness_and_all_checkpoints_without_false_completion() {
    let mesh = square();
    let plan = schedule(0., 0.5, 2);
    let mut consumed = engine(&mesh, &plan);
    consumed.next().unwrap().unwrap();
    assert_eq!(
        collect_layers(consumed, limits()).unwrap_err().cause,
        LayerFailure::CollectionAlreadyStarted
    );
    for point in [
        Checkpoint::CollectionStart,
        Checkpoint::CollectionStorage,
        Checkpoint::CollectionComplete,
    ] {
        let mut predicate = |p| p == point;
        let mut events = Vec::new();
        let error = {
            let mut callback = |e: LayerProgress| events.push(e.kind);
            let stream = LayerEngine::new(
                &mesh,
                &plan,
                SectionTransform::Identity,
                config(),
                Cancellation::new(&mut predicate),
                Some(&mut callback),
            );
            collect_layers(stream, limits()).unwrap_err()
        };
        assert_eq!(
            error.cause,
            LayerFailure::Cancelled(Cancelled { checkpoint: point })
        );
        assert_eq!(error.plane, None);
        assert!(!events.contains(&LayerProgressKind::Completed));
        assert_eq!(events.last(), Some(&LayerProgressKind::Failed));
    }
    assert_eq!(
        collect_layers(
            engine(&mesh, &plan),
            CollectionLimits {
                max_layers: 1,
                ..limits()
            }
        )
        .unwrap_err()
        .cause,
        LayerFailure::CollectionLayerLimit { limit: 1 }
    );
    let full = collect_layers(engine(&mesh, &plan), limits()).unwrap();
    let exact = full.payload_bytes();
    assert_eq!(
        collect_layers(
            engine(&mesh, &plan),
            CollectionLimits {
                max_payload_bytes: exact,
                ..limits()
            }
        )
        .unwrap(),
        full
    );
    let mut events = Vec::new();
    let error = {
        let mut callback = |e: LayerProgress| events.push(e.kind);
        let stream = LayerEngine::new(
            &mesh,
            &plan,
            SectionTransform::Identity,
            config(),
            Cancellation::never(),
            Some(&mut callback),
        );
        collect_layers(
            stream,
            CollectionLimits {
                max_payload_bytes: exact - 1,
                ..limits()
            },
        )
        .unwrap_err()
    };
    assert_eq!(
        error.cause,
        LayerFailure::CollectionPayloadLimit { limit: exact - 1 }
    );
    assert_eq!(error.plane, Some(plan.planes()[1]));
    assert!(!events.contains(&LayerProgressKind::Completed));
    assert_eq!(events.last(), Some(&LayerProgressKind::Failed));
}

#[test]
fn unresolved_open_contact_duplicate_and_tangent_evidence_is_not_discarded() {
    let open = ribbon(&[[0., 0.], [4., 0.]], &[[0, 1]]);
    let duplicate = ribbon(&[[0., 0.], [4., 0.]], &[[0, 1], [0, 1]]);
    let contact = ribbon(
        &[
            [0., 0.],
            [4., 0.],
            [4., 4.],
            [0., 4.],
            [4., 0.],
            [8., 0.],
            [8., 4.],
            [4., 4.],
        ],
        &[
            [0, 1],
            [1, 2],
            [2, 3],
            [3, 0],
            [4, 5],
            [5, 6],
            [6, 7],
            [7, 4],
        ],
    );
    let tangent = TriangleMesh::try_new(
        vec![
            Point3::new(0., 0., 0.),
            Point3::new(4., 0., 1.),
            Point3::new(0., 4., 1.),
        ],
        vec![[0, 1, 2]],
    )
    .unwrap();
    let coplanar = TriangleMesh::try_new(
        vec![
            Point3::new(0., 0., 0.),
            Point3::new(4., 0., 0.),
            Point3::new(0., 4., 0.),
        ],
        vec![[0, 1, 2]],
    )
    .unwrap();
    for (case, (mesh, unresolved)) in [
        (&open, true),
        (&duplicate, true),
        (&contact, true),
        (&tangent, false),
        (&coplanar, false),
    ]
    .into_iter()
    .enumerate()
    {
        let plan = schedule(0., 1., 1);
        let layer = engine(mesh, &plan).next().unwrap().unwrap();
        assert_ne!(layer.raw().events().len(), 0);
        compare_pipeline(mesh, &layer, SectionTransform::Identity);
        assert_eq!(
            layer.completeness() == RegionCompleteness::GeometricallyUnresolved,
            unresolved,
            "case {case}"
        );
        if unresolved {
            assert_eq!(layer.regions().len(), 0);
        }
    }
}

#[test]
fn off_grid_second_plane_fails_without_invalidating_first_atomic_output() {
    let mesh = TriangleMesh::try_new(
        vec![
            Point3::new(0., 0., -1.),
            Point3::new(2., 0., -1.),
            Point3::new(0., 2., -1.),
            Point3::new(0., 0., 2.),
            Point3::new(2., 0., 2.),
            Point3::new(0., 2., 2.),
        ],
        vec![
            [0, 1, 4],
            [0, 4, 3],
            [1, 2, 5],
            [1, 5, 4],
            [2, 0, 3],
            [2, 3, 5],
        ],
    )
    .unwrap();
    // At the bottom plane source vertices are integral; at Z=0 diagonal crossings are thirds.
    let plan = schedule(-1., 1., 2);
    let mut stream = engine(&mesh, &plan);
    let first = stream.next().unwrap().unwrap();
    let before = encode(&first);
    let error = stream.next().unwrap().unwrap_err();
    assert_eq!(error.plane, Some(plan.planes()[1]));
    assert!(matches!(error.cause, LayerFailure::Grid(_)));
    assert_eq!(stream.completed_layers(), 1);
    assert_eq!(stream.state(), StreamState::Failed);
    assert!(stream.next().is_none());
    assert_eq!(encode(&first), before);
}

#[test]
fn closed_cube_has_one_material_region_and_outside_plane_is_explicitly_empty() {
    let mesh = TriangleMesh::try_new(
        vec![
            Point3::new(0., 0., 0.),
            Point3::new(4., 0., 0.),
            Point3::new(4., 4., 0.),
            Point3::new(0., 4., 0.),
            Point3::new(0., 0., 4.),
            Point3::new(4., 0., 4.),
            Point3::new(4., 4., 4.),
            Point3::new(0., 4., 4.),
        ],
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
    .unwrap();
    let plan = schedule(1., 2., 3);
    let layers = collect_layers(engine(&mesh, &plan), limits()).unwrap();
    assert_eq!(layers.layers().len(), 3);
    for layer in layers.layers() {
        compare_pipeline(&mesh, layer, SectionTransform::Identity);
        assert_eq!(layer.completeness(), RegionCompleteness::Complete);
        if layer.plane().index < 2 {
            assert_eq!(layer.regions().len(), 1);
            assert_eq!(layer.paths().len(), 1);
            assert_eq!(layer.edges().len(), 8);
        } else {
            assert_eq!(layer.raw().events().len(), 0);
            assert_eq!(layer.vertices().len(), 0);
            assert_eq!(layer.regions().len(), 0);
        }
    }
}

#[test]
fn storage_cancellation_after_reservation_and_empty_exhaustion_are_observed() {
    let mesh = square();
    let plan = schedule(0., 1., 1);
    for point in [Checkpoint::LayerStorage, Checkpoint::CollectionStorage] {
        let mut visits = 0;
        let mut predicate = |p| {
            if p == point {
                visits += 1;
            }
            p == point && visits == 2
        };
        let stream = LayerEngine::new(
            &mesh,
            &plan,
            SectionTransform::Identity,
            config(),
            Cancellation::new(&mut predicate),
            None,
        );
        let error = collect_layers(stream, limits()).unwrap_err();
        assert_eq!(
            error.cause,
            LayerFailure::Cancelled(Cancelled { checkpoint: point })
        );
        assert_eq!(error.plane.is_some(), point == Checkpoint::LayerStorage);
        assert_eq!(visits, 2);
    }
    let empty = schedule(0., 1., 0);
    let mut predicate = |p| p == Checkpoint::LayersComplete;
    let mut stream = LayerEngine::new(
        &mesh,
        &empty,
        SectionTransform::Identity,
        config(),
        Cancellation::new(&mut predicate),
        None,
    );
    let error = stream.next().unwrap().unwrap_err();
    assert_eq!(error.plane, None);
    assert_eq!(
        error.cause,
        LayerFailure::Cancelled(Cancelled {
            checkpoint: Checkpoint::LayersComplete
        })
    );
    assert_eq!(stream.state(), StreamState::Failed);
    assert!(stream.next().is_none());
}

/// One closed shell: a 20 x 20 plate of height `p` carrying a 10 x 10 boss up to `top`, or the
/// same solid upside down (a 10 x 10 stem below a plate) when `inverted`.
fn step(p: f64, top: f64, inverted: bool) -> TriangleMesh {
    let flip = |z: f64| if inverted { top - z } else { z };
    let p = if inverted { top - p } else { p };
    let points = [
        [0., 0., 0.],
        [20., 0., 0.],
        [20., 20., 0.],
        [0., 20., 0.],
        [0., 0., p],
        [20., 0., p],
        [20., 20., p],
        [0., 20., p],
        [5., 5., p],
        [15., 5., p],
        [15., 15., p],
        [5., 15., p],
        [5., 5., top],
        [15., 5., top],
        [15., 15., top],
        [5., 15., top],
    ];
    let mut triangles: Vec<[u32; 3]> = vec![
        [0, 2, 1],
        [0, 3, 2],
        [0, 1, 5],
        [0, 5, 4],
        [1, 2, 6],
        [1, 6, 5],
        [2, 3, 7],
        [2, 7, 6],
        [3, 0, 4],
        [3, 4, 7],
        [8, 9, 13],
        [8, 13, 12],
        [9, 10, 14],
        [9, 14, 13],
        [10, 11, 15],
        [10, 15, 14],
        [11, 8, 12],
        [11, 12, 15],
        [12, 13, 14],
        [12, 14, 15],
    ];
    for k in 0..4u32 {
        let n = (k + 1) % 4;
        triangles.push([4 + k, 4 + n, 8 + n]);
        triangles.push([4 + k, 8 + n, 8 + k]);
    }
    if inverted {
        for t in &mut triangles {
            t.swap(1, 2);
        }
    }
    TriangleMesh::try_new(
        points
            .iter()
            .map(|&[x, y, z]| Point3::new(x, y, flip(z)))
            .collect(),
        triangles,
    )
    .unwrap()
}

#[test]
fn planes_through_a_step_face_sample_the_material_just_below() {
    // Planes 0.125, 0.375, 0.625, ...: the second lies exactly on the step face at 0.375.
    let plan = schedule(0.125, 0.25, 4);
    for inverted in [false, true] {
        let mesh = step(0.375, 1.0, inverted);
        // Without the option the plane yields the step face itself: an annulus whose hole is
        // the footprint of the material above (upright) or below (inverted).
        let unmoved: Vec<RegionLayer> = engine(&mesh, &plan)
            .with_represented_coordinates()
            .map(Result::unwrap)
            .collect();
        assert_eq!(unmoved[1].regions().len(), 1);
        assert_eq!(unmoved[1].region_holes(0).unwrap().len(), 1, "{inverted}");
        let layers: Vec<RegionLayer> = engine(&mesh, &plan)
            .with_represented_coordinates()
            .with_vertex_avoiding_planes(1 << 20)
            .map(Result::unwrap)
            .collect();
        for (k, layer) in layers.iter().enumerate() {
            assert_eq!(
                layer.completeness(),
                RegionCompleteness::Complete,
                "{inverted} {k}"
            );
            assert_eq!(
                layer.plane(),
                plan.planes()[k],
                "the scheduled identity is kept"
            );
            assert_eq!(layer.regions().len(), 1);
            assert!(layer.region_holes(0).unwrap().is_empty(), "{inverted} {k}");
        }
        // Only the plane on the step moved, into the lower half of its slab.
        let moved = layers[1].sampled_z_mm();
        assert!(moved < 0.375 && moved > 0.25, "{moved}");
        for k in [0, 2, 3] {
            assert_eq!(layers[k].sampled_z_mm(), plan.planes()[k].z_mm);
        }
        // The moved layer has the material just below: the plate (upright) or the stem.
        let width = |layer: &RegionLayer| {
            let xs: Vec<f64> = layer
                .vertices()
                .iter()
                .filter_map(|v| match v.coordinates {
                    CoordinateAgreement::Agreed(p) => Some(p.x),
                    CoordinateAgreement::Conflict => None,
                })
                .collect();
            xs.iter().copied().fold(f64::NEG_INFINITY, f64::max)
                - xs.iter().copied().fold(f64::INFINITY, f64::min)
        };
        let expected = if inverted { 10.0 } else { 20.0 };
        assert!((width(&layers[1]) - expected).abs() < 1e-9, "{inverted}");
    }
}
