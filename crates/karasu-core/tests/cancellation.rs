//! Deterministic phase cancellation, atomic results, and compatibility through public APIs.

use karasu_core::cancellation::{Cancellation, Cancelled, Checkpoint};
use karasu_core::geometry::{ObjectTransform, Point3, Rotation3, Vector3};
use karasu_core::mesh::stl::{
    import_stl_bytes, import_stl_bytes_with_cancellation, import_stl_path,
    import_stl_path_with_cancellation, import_stl_reader, import_stl_reader_with_cancellation,
    ImportedStl, StlFormat, StlOptions, StlUnitPolicy,
};
use karasu_core::mesh::topology::{TopologyError, TopologyLimits};
use karasu_core::mesh::{MeshError, StlError, TransformedBoundsError, TriangleMesh};
use std::{cell::Cell, error::Error, io, path::PathBuf};

const ASCII: &[u8] = b"solid one\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\nendloop\nendfacet\nendsolid one\n";

fn options(format: StlFormat) -> StlOptions {
    let mut options = StlOptions::new(StlUnitPolicy::AssumeMillimeters);
    options.format = format;
    options
}

fn fixture_path(id: &str) -> PathBuf {
    let (id, variant) = match id {
        "FX-10A" => ("FX-10", Some("A")),
        "FX-10B" => ("FX-10", Some("B")),
        id => (id, None),
    };
    // Read controlled primitive fields from the owned manifest, like the existing fixture suites.
    let entry = include_str!("../../../fixtures/reference/manifest.json")
        .split("\"fixture_id\":")
        .skip(1)
        .find(|entry| {
            entry.trim_start().starts_with(&format!("\"{id}\""))
                && variant.is_none_or(|variant| {
                    entry
                        .split_once("\"variant\":")
                        .unwrap()
                        .1
                        .trim_start()
                        .starts_with(&format!("\"{variant}\""))
                })
        })
        .unwrap();
    let filename = entry
        .split_once("\"filename\":")
        .unwrap()
        .1
        .split('"')
        .nth(1)
        .unwrap();
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/reference")
        .join(filename)
}

fn fixture(id: &str) -> ImportedStl {
    import_stl_path(fixture_path(id), options(StlFormat::Auto)).unwrap()
}

fn binary(count: u32) -> Vec<u8> {
    let mut bytes = vec![0; 80];
    bytes.extend(count.to_le_bytes());
    for _ in 0..count {
        for value in [0_f32, 0., 1., 0., 0., 0., 1., 0., 0., 0., 1., 0.] {
            bytes.extend(value.to_le_bytes());
        }
        bytes.extend(0_u16.to_le_bytes());
    }
    bytes
}

fn fan() -> TriangleMesh {
    let mut vertices = vec![Point3::new(0., 0., 0.)];
    for index in 0..1024 {
        let angle = f64::from(index) * std::f64::consts::TAU / 1024.;
        vertices.push(Point3::new(angle.cos(), angle.sin(), 0.));
    }
    let triangles = (0..1024).map(|i| [0, i + 1, (i + 1) % 1024 + 1]).collect();
    TriangleMesh::try_new(vertices, triangles).unwrap()
}

fn at(point: Checkpoint) -> impl FnMut(Checkpoint) -> bool {
    move |actual| actual == point
}

fn assert_stl_cancelled(result: Result<ImportedStl, StlError>, point: Checkpoint) {
    let error = result.unwrap_err();
    assert!(matches!(&error, StlError::Cancelled(cause) if cause.checkpoint == point));
    assert_eq!(
        error
            .source()
            .unwrap()
            .downcast_ref::<Cancelled>()
            .unwrap()
            .checkpoint,
        point
    );
    assert!(error.to_string().contains("cancelled"));
}

#[test]
fn precancelled_binary_import_returns_cancellation_before_validation() {
    for bytes in [binary(1).as_slice(), &[]] {
        assert_stl_cancelled(
            import_stl_bytes_with_cancellation(
                bytes,
                options(StlFormat::Binary),
                &mut Cancellation::new(&mut |_| true),
            ),
            Checkpoint::ImportStart,
        );
    }
}

#[test]
fn precancelled_ascii_import_returns_cancellation() {
    assert_stl_cancelled(
        import_stl_bytes_with_cancellation(
            ASCII,
            options(StlFormat::Ascii),
            &mut Cancellation::new(&mut |_| true),
        ),
        Checkpoint::ImportStart,
    );
}

#[test]
fn precancelled_automatic_import_does_not_detect_or_parse() {
    let mut calls = Vec::new();
    let result = import_stl_bytes_with_cancellation(
        &binary(1),
        options(StlFormat::Auto),
        &mut Cancellation::new(&mut |point| {
            calls.push(point);
            true
        }),
    );
    assert_stl_cancelled(result, Checkpoint::ImportStart);
    assert_eq!(calls, [Checkpoint::ImportStart]);
}

#[test]
fn automatic_detection_checks_during_large_text_classification() {
    let mut bytes = vec![b' '; 32 * 1024];
    bytes.extend_from_slice(b"solid empty\nendsolid empty\n");
    let mut calls = 0;
    let mut predicate = |point| {
        if point == Checkpoint::Detection {
            calls += 1;
        }
        point == Checkpoint::Detection && calls == 3
    };
    assert_stl_cancelled(
        import_stl_bytes_with_cancellation(
            &bytes,
            options(StlFormat::Auto),
            &mut Cancellation::new(&mut predicate),
        ),
        Checkpoint::Detection,
    );
    assert_eq!(calls, 3);
}

#[test]
fn binary_parser_cancels_after_a_deterministic_number_of_checkpoints() {
    let bytes = binary(1024);
    let mut calls = 0;
    assert_stl_cancelled(
        import_stl_bytes_with_cancellation(
            &bytes,
            options(StlFormat::Binary),
            &mut Cancellation::new(&mut |point| {
                if point == Checkpoint::BinaryParsing {
                    calls += 1;
                }
                point == Checkpoint::BinaryParsing && calls == 3
            }),
        ),
        Checkpoint::BinaryParsing,
    );
    assert_eq!(calls, 3);
    assert_eq!(
        import_stl_bytes(&bytes, options(StlFormat::Binary))
            .unwrap()
            .mesh
            .triangles()
            .len(),
        1024
    );
}

#[test]
fn ascii_parser_cancels_during_record_consumption_after_validation() {
    let mut text = String::from("solid many\n");
    for _ in 0..1024 {
        text.push_str(
            std::str::from_utf8(ASCII)
                .unwrap()
                .split_once('\n')
                .unwrap()
                .1
                .split("endsolid")
                .next()
                .unwrap(),
        );
    }
    text.push_str("endsolid many\n");
    // Successful replay establishes an exact deterministic trace, then cancel the final
    // AsciiParsing poll, which is during record consumption rather than input pre-scans.
    let mut trace = Vec::new();
    import_stl_bytes_with_cancellation(
        text.as_bytes(),
        options(StlFormat::Ascii),
        &mut Cancellation::new(&mut |point| {
            trace.push(point);
            false
        }),
    )
    .unwrap();
    let target = trace
        .iter()
        .filter(|&&p| p == Checkpoint::AsciiParsing)
        .count();
    let mut calls = 0;
    assert_stl_cancelled(
        import_stl_bytes_with_cancellation(
            text.as_bytes(),
            options(StlFormat::Ascii),
            &mut Cancellation::new(&mut |point| {
                if point == Checkpoint::AsciiParsing {
                    calls += 1;
                }
                point == Checkpoint::AsciiParsing && calls == target
            }),
        ),
        Checkpoint::AsciiParsing,
    );
    assert_eq!(calls, target);
}

#[test]
fn cancellation_propagates_from_all_import_construction_and_diagnostic_phases() {
    let bytes = binary(1024);
    for point in [
        Checkpoint::Deduplication,
        Checkpoint::MeshStart,
        Checkpoint::MeshVertices,
        Checkpoint::MeshIndices,
        Checkpoint::SourceBounds,
        Checkpoint::MeshComplete,
        Checkpoint::DiagnosticsStart,
        Checkpoint::GeometryDiagnostics,
        Checkpoint::DiagnosticsComplete,
        Checkpoint::ImportDiagnostics,
    ] {
        assert_stl_cancelled(
            import_stl_bytes_with_cancellation(
                &bytes,
                options(StlFormat::Binary),
                &mut Cancellation::new(&mut at(point)),
            ),
            point,
        );
    }
}

#[test]
fn construction_can_cancel_after_partial_coordinate_validation() {
    let mut hits = 0;
    let result = TriangleMesh::try_new_with_cancellation(
        vec![Point3::new(1., 2., 3.); 1024],
        vec![],
        &mut Cancellation::new(&mut |point| {
            if point == Checkpoint::MeshVertices {
                hits += 1;
            }
            point == Checkpoint::MeshVertices && hits == 2
        }),
    );
    assert!(matches!(result, Err(MeshError::Cancelled(_))));
    assert_eq!(hits, 2);
}

#[test]
fn import_final_checkpoint_prevents_publication_for_small_and_empty_input() {
    for bytes in [
        ASCII,
        b"solid empty\nendsolid empty\n",
        binary(0).as_slice(),
    ] {
        assert_stl_cancelled(
            import_stl_bytes_with_cancellation(
                bytes,
                options(StlFormat::Auto),
                &mut Cancellation::new(&mut at(Checkpoint::ImportComplete)),
            ),
            Checkpoint::ImportComplete,
        );
    }
}

#[test]
fn standalone_diagnostics_cancel_midway_preserve_source_and_retry() {
    let mesh = fan();
    let before_vertices = mesh.vertices().to_vec();
    let before_faces = mesh.triangles().to_vec();
    let bounds = mesh.source_bounds();
    let mut hits = 0;
    let result = mesh.diagnostics_with_cancellation(&mut Cancellation::new(&mut |point| {
        if point == Checkpoint::GeometryDiagnostics {
            hits += 1;
        }
        point == Checkpoint::GeometryDiagnostics && hits == 2
    }));
    assert!(matches!(result, Err(MeshError::Cancelled(_))));
    assert_eq!(hits, 2);
    assert_eq!(mesh.vertices(), before_vertices);
    assert_eq!(mesh.triangles(), before_faces);
    assert_eq!(mesh.source_bounds(), bounds);
    assert_eq!(mesh.diagnostics().unwrap(), []);
}

#[test]
fn topology_cancellation_reaches_all_internal_phases_without_partial_report() {
    let mesh = fan();
    for point in [
        Checkpoint::TopologyStart,
        Checkpoint::GeometryDiagnostics,
        Checkpoint::TopologyPreparation,
        Checkpoint::EdgeIncidence,
        Checkpoint::EdgeSorting,
        Checkpoint::VertexIndex,
        Checkpoint::VertexNeighborhoods,
        Checkpoint::VertexSorting,
        Checkpoint::Components,
        Checkpoint::ComponentSorting,
        Checkpoint::TopologySummary,
        Checkpoint::TopologyComplete,
    ] {
        let error = mesh
            .analyze_topology_with_cancellation(
                TopologyLimits::default(),
                &mut Cancellation::new(&mut at(point)),
            )
            .unwrap_err();
        assert!(matches!(&error, TopologyError::Cancelled(cause) if cause.checkpoint == point));
        assert_eq!(
            error
                .source()
                .unwrap()
                .downcast_ref::<Cancelled>()
                .unwrap()
                .checkpoint,
            point
        );
    }
    assert_eq!(
        mesh.analyze_topology().unwrap().summary().usable_triangles,
        1024
    );
}

#[test]
fn high_degree_vertex_inner_loop_is_cancellable() {
    let mesh = fan();
    let mut hits = 0;
    let error = mesh
        .analyze_topology_with_cancellation(
            TopologyLimits::default(),
            &mut Cancellation::new(&mut |point| {
                if point == Checkpoint::VertexNeighborhoods {
                    hits += 1;
                }
                point == Checkpoint::VertexNeighborhoods && hits == 3
            }),
        )
        .unwrap_err();
    assert!(matches!(error, TopologyError::Cancelled(_)));
    // Outer vertex 0, then its incident face 0 and face 256: cancellation is inside one fan.
    assert_eq!(hits, 3);
}

#[test]
fn sorting_can_be_cancelled_immediately_after_an_uninterruptible_sort() {
    for point in [
        Checkpoint::EdgeSorting,
        Checkpoint::VertexSorting,
        Checkpoint::ComponentSorting,
    ] {
        let mut hits = 0;
        let error = fan()
            .analyze_topology_with_cancellation(
                TopologyLimits::default(),
                &mut Cancellation::new(&mut |actual| {
                    if actual == point {
                        hits += 1;
                    }
                    actual == point && hits == 2
                }),
            )
            .unwrap_err();
        assert!(matches!(error, TopologyError::Cancelled(cause) if cause.checkpoint == point));
        assert_eq!(hits, 2);
    }
}

#[test]
fn bounds_scan_cancels_after_multiple_vertex_batches() {
    let mesh = fan();
    let mut hits = 0;
    let result = mesh.world_bounds_with_cancellation(
        &ObjectTransform::identity(),
        &mut Cancellation::new(&mut |point| {
            if point == Checkpoint::WorldBounds {
                hits += 1;
            }
            point == Checkpoint::WorldBounds && hits == 3
        }),
    );
    assert!(matches!(result, Err(TransformedBoundsError::Cancelled(_))));
    assert_eq!(hits, 3);
    assert_eq!(
        mesh.world_bounds(&ObjectTransform::identity()).unwrap(),
        mesh.source_bounds()
    );
}

#[test]
fn empty_and_small_bounds_check_entry_and_final_publication() {
    for mesh in [TriangleMesh::try_new(vec![], vec![]).unwrap(), fan()] {
        for point in [Checkpoint::WorldBounds, Checkpoint::BoundsComplete] {
            let result = mesh.world_bounds_with_cancellation(
                &ObjectTransform::identity(),
                &mut Cancellation::new(&mut at(point)),
            );
            assert!(
                matches!(result, Err(TransformedBoundsError::Cancelled(cause)) if cause.checkpoint == point)
            );
            let result = mesh.transformed_bounds_with_cancellation(
                &ObjectTransform::identity().to_affine().unwrap(),
                &mut Cancellation::new(&mut at(point)),
            );
            assert!(
                matches!(result, Err(TransformedBoundsError::Cancelled(cause)) if cause.checkpoint == point)
            );
        }
    }
}

#[test]
fn empty_mesh_diagnostics_construction_and_topology_have_final_checkpoints() {
    let empty = TriangleMesh::try_new(vec![], vec![]).unwrap();
    assert!(matches!(
        TriangleMesh::try_new_with_cancellation(
            vec![],
            vec![],
            &mut Cancellation::new(&mut at(Checkpoint::MeshComplete))
        ),
        Err(MeshError::Cancelled(_))
    ));
    assert!(matches!(
        empty.diagnostics_with_cancellation(&mut Cancellation::new(&mut at(
            Checkpoint::DiagnosticsComplete
        ))),
        Err(MeshError::Cancelled(_))
    ));
    assert!(matches!(
        empty.analyze_topology_with_cancellation(
            TopologyLimits::default(),
            &mut Cancellation::new(&mut at(Checkpoint::TopologyComplete))
        ),
        Err(TopologyError::Cancelled(_))
    ));
}

#[test]
fn reference_metadata_and_geometry_survive_cancelled_derived_operations() {
    for id in ["FX-01", "FX-02", "FX-05", "FX-08"] {
        let imported = fixture(id);
        for point in [
            Checkpoint::VertexNeighborhoods,
            Checkpoint::Components,
            Checkpoint::TopologyComplete,
        ] {
            assert!(imported
                .mesh
                .analyze_topology_with_cancellation(
                    TopologyLimits::default(),
                    &mut Cancellation::new(&mut at(point))
                )
                .is_err());
        }
        assert!(imported
            .mesh
            .world_bounds_with_cancellation(
                &ObjectTransform::identity(),
                &mut Cancellation::new(&mut at(Checkpoint::BoundsComplete))
            )
            .is_err());
        assert_eq!(imported, fixture(id));
        imported.mesh.analyze_topology().unwrap();
    }
}

#[test]
fn never_cancelled_public_apis_match_ordinary_apis_on_all_small_fixtures() {
    for id in [
        "FX-01", "FX-02", "FX-03", "FX-04", "FX-05", "FX-06", "FX-07", "FX-08", "FX-09", "FX-10A",
        "FX-10B",
    ] {
        let path = fixture_path(id);
        let bytes = std::fs::read(&path).unwrap();
        let ordinary = fixture(id);
        let mut predicate = |_| false;
        let mut control = Cancellation::new(&mut predicate);
        assert_eq!(
            ordinary,
            import_stl_bytes_with_cancellation(&bytes, options(StlFormat::Auto), &mut control)
                .unwrap()
        );
        assert_eq!(
            ordinary,
            import_stl_path_with_cancellation(&path, options(StlFormat::Auto), &mut control)
                .unwrap()
        );
        assert_eq!(
            ordinary,
            import_stl_reader_with_cancellation(
                bytes.as_slice(),
                options(StlFormat::Auto),
                &mut control
            )
            .unwrap()
        );
        let mesh = &ordinary.mesh;
        assert_eq!(
            mesh.analyze_topology().unwrap(),
            mesh.analyze_topology_with_cancellation(TopologyLimits::default(), &mut control)
                .unwrap()
        );
        assert_eq!(
            mesh.diagnostics().unwrap(),
            mesh.diagnostics_with_cancellation(&mut control).unwrap()
        );
        assert_eq!(
            *mesh,
            TriangleMesh::try_new_with_cancellation(
                mesh.vertices().to_vec(),
                mesh.triangles().to_vec(),
                &mut control
            )
            .unwrap()
        );
        let transform = ObjectTransform::new(
            Vector3::new(15., -7., 5.),
            Rotation3::from_euler_xyz_radians(Vector3::new(0.31, -0.47, 0.29)).unwrap(),
            Vector3::new(1.25, -0.8, 1.1),
            Point3::new(2., -3., 4.),
        )
        .unwrap();
        assert_eq!(
            mesh.world_bounds(&transform).unwrap(),
            mesh.world_bounds_with_cancellation(&transform, &mut control)
                .unwrap()
        );
        let affine = transform.to_affine().unwrap();
        assert_eq!(
            mesh.transformed_bounds(&affine).unwrap(),
            mesh.transformed_bounds_with_cancellation(&affine, &mut control)
                .unwrap()
        );
    }
}

#[test]
fn ascii_and_empty_results_match_with_never_cancelled_predicate() {
    for bytes in [
        ASCII,
        b"solid empty\nendsolid empty\n",
        binary(0).as_slice(),
    ] {
        assert_eq!(
            import_stl_bytes(bytes, options(StlFormat::Auto)).unwrap(),
            import_stl_bytes_with_cancellation(
                bytes,
                options(StlFormat::Auto),
                &mut Cancellation::new(&mut |_| false)
            )
            .unwrap()
        );
    }
}

struct Reader<'a> {
    calls: &'a Cell<usize>,
    bytes: &'a [u8],
    interrupt: bool,
}

impl io::Read for Reader<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.calls.set(self.calls.get() + 1);
        if self.interrupt {
            self.interrupt = false;
            return Err(io::ErrorKind::Interrupted.into());
        }
        self.bytes.read(buffer)
    }
}

#[test]
fn precancelled_reader_is_not_consumed_and_missing_path_is_not_opened() {
    let calls = Cell::new(0);
    assert_stl_cancelled(
        import_stl_reader_with_cancellation(
            Reader {
                calls: &calls,
                bytes: ASCII,
                interrupt: false,
            },
            options(StlFormat::Auto),
            &mut Cancellation::new(&mut |_| true),
        ),
        Checkpoint::ImportStart,
    );
    assert_eq!(calls.get(), 0);
    assert_stl_cancelled(
        import_stl_path_with_cancellation(
            "not-a-real-mesh.stl",
            options(StlFormat::Auto),
            &mut Cancellation::new(&mut |_| true),
        ),
        Checkpoint::ImportStart,
    );
}

#[test]
fn cancellation_after_read_returns_no_mesh_and_does_not_roll_back_reader() {
    let calls = Cell::new(0);
    let mut reader = Reader {
        calls: &calls,
        bytes: ASCII,
        interrupt: false,
    };
    assert_stl_cancelled(
        import_stl_reader_with_cancellation(
            &mut reader,
            options(StlFormat::Auto),
            &mut Cancellation::new(&mut |_| calls.get() > 0),
        ),
        Checkpoint::Read,
    );
    assert_eq!(calls.get(), 1);
    assert_eq!(reader.bytes, [] as [u8; 0]);
}

#[test]
fn interrupted_reads_are_retried_without_becoming_cancellation() {
    let calls = Cell::new(0);
    let reader = Reader {
        calls: &calls,
        bytes: ASCII,
        interrupt: true,
    };
    let imported = import_stl_reader_with_cancellation(
        reader,
        options(StlFormat::Auto),
        &mut Cancellation::new(&mut |_| false),
    )
    .unwrap();
    assert_eq!(
        imported,
        import_stl_reader(ASCII, options(StlFormat::Auto)).unwrap()
    );
    assert_eq!(calls.get(), 3);
}

#[test]
fn cancellation_can_stop_repeatedly_interrupted_reader_between_calls() {
    struct Interrupted<'a>(&'a Cell<usize>);
    impl io::Read for Interrupted<'_> {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            self.0.set(self.0.get() + 1);
            Err(io::ErrorKind::Interrupted.into())
        }
    }
    let calls = Cell::new(0);
    let reader = Interrupted(&calls);
    assert_stl_cancelled(
        import_stl_reader_with_cancellation(
            reader,
            options(StlFormat::Auto),
            &mut Cancellation::new(&mut |_| calls.get() == 3),
        ),
        Checkpoint::Read,
    );
    assert_eq!(calls.get(), 3);
}

#[test]
fn io_failures_are_not_reclassified_as_cancellation_without_a_request() {
    struct Broken;
    impl io::Read for Broken {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "controlled reader failure",
            ))
        }
    }
    let error = import_stl_reader_with_cancellation(
        Broken,
        options(StlFormat::Auto),
        &mut Cancellation::new(&mut |_| false),
    )
    .unwrap_err();
    assert!(
        matches!(&error, StlError::Io(cause) if cause.kind() == io::ErrorKind::PermissionDenied)
    );
    assert!(error
        .source()
        .unwrap()
        .downcast_ref::<io::Error>()
        .is_some());
}

#[test]
fn numeric_bounds_failure_remains_contextual_and_source_is_preserved() {
    let mesh = TriangleMesh::try_new(vec![Point3::new(f64::MAX, 0., 0.)], vec![]).unwrap();
    let transform = ObjectTransform::new(
        Vector3::new(f64::MAX, 0., 0.),
        Rotation3::identity(),
        Vector3::new(1., 1., 1.),
        Point3::new(0., 0., 0.),
    )
    .unwrap();
    let ordinary = mesh.world_bounds(&transform).unwrap_err();
    let controlled = mesh
        .world_bounds_with_cancellation(&transform, &mut Cancellation::new(&mut |_| false))
        .unwrap_err();
    assert_eq!(ordinary, controlled);
    assert!(matches!(
        controlled,
        TransformedBoundsError::Vertex { vertex: 0, .. }
    ));
    assert!(controlled.source().is_some());
    assert_eq!(mesh.vertices(), [Point3::new(f64::MAX, 0., 0.)]);
}

#[test]
fn resource_errors_remain_distinct_and_precancellation_wins_at_entry() {
    let mut options = options(StlFormat::Binary);
    options.limits.max_input_bytes = 0;
    assert!(matches!(
        import_stl_bytes_with_cancellation(&binary(1), options, &mut Cancellation::never()),
        Err(StlError::LimitExceeded { .. })
    ));
    assert_stl_cancelled(
        import_stl_bytes_with_cancellation(
            &binary(1),
            options,
            &mut Cancellation::new(&mut |_| true),
        ),
        Checkpoint::ImportStart,
    );
    let limits = TopologyLimits {
        max_vertices: 0,
        max_triangles: 0,
    };
    assert!(matches!(
        fan().analyze_topology_with_cancellation(limits, &mut Cancellation::never()),
        Err(TopologyError::LimitExceeded { .. })
    ));
    assert!(matches!(
        fan().analyze_topology_with_cancellation(limits, &mut Cancellation::new(&mut |_| true)),
        Err(TopologyError::Cancelled(_))
    ));
}

#[test]
fn ordinary_errors_before_the_next_checkpoint_are_not_replaced() {
    let mut points = Vec::new();
    let result = import_stl_bytes_with_cancellation(
        &[],
        options(StlFormat::Auto),
        &mut Cancellation::new(&mut |point| {
            points.push(point);
            point == Checkpoint::ImportComplete
        }),
    );
    assert!(matches!(result, Err(StlError::EmptyInput)));
    assert_eq!(points, [Checkpoint::ImportStart]);
    let result = TriangleMesh::try_new_with_cancellation(
        vec![Point3::new(f64::NAN, 0., 0.)],
        vec![],
        &mut Cancellation::new(&mut at(Checkpoint::MeshComplete)),
    );
    assert!(matches!(result, Err(MeshError::NonFiniteVertex { .. })));
}

#[test]
fn identical_inputs_and_options_produce_identical_checkpoint_sequences() {
    let mut traces = Vec::new();
    for _ in 0..2 {
        let mut trace = Vec::new();
        let imported = import_stl_bytes_with_cancellation(
            &binary(512),
            options(StlFormat::Auto),
            &mut Cancellation::new(&mut |point| {
                trace.push(point);
                false
            }),
        )
        .unwrap();
        imported
            .mesh
            .analyze_topology_with_cancellation(
                TopologyLimits::default(),
                &mut Cancellation::new(&mut |point| {
                    trace.push(point);
                    false
                }),
            )
            .unwrap();
        traces.push(trace);
    }
    assert_eq!(traces[0], traces[1]);
    assert_eq!(traces[0].first(), Some(&Checkpoint::ImportStart));
    assert_eq!(traces[0].last(), Some(&Checkpoint::TopologyComplete));
}

#[test]
fn cancellation_requested_after_the_final_check_does_not_retroactively_cancel_success() {
    let requested = Cell::new(false);
    let result = import_stl_bytes_with_cancellation(
        ASCII,
        options(StlFormat::Auto),
        &mut Cancellation::new(&mut |point| {
            let observed = requested.get();
            if point == Checkpoint::ImportComplete {
                requested.set(true);
            }
            observed
        }),
    );
    assert!(result.is_ok());
    assert!(requested.get());
}
