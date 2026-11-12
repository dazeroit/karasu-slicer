//! Bounded public-API fuzz oracles. No parser or topology algorithm is substituted here.
//! Entry points also support uninstrumented seed replay, which is not coverage-guided fuzzing.

pub mod construct_material;
pub mod drainage;
pub mod hollow_mesh;
pub mod layer_engine;
pub mod preparation_session;
pub mod profile_json;
pub mod schedule;
pub mod strict_regions;
pub mod supports;

use karasu_core::geometry::{Point3, Vector3};
use karasu_core::mesh::stl::{
    import_stl_bytes, ImportLimits, ImportedStl, StlFormat, StlHeader, StlOptions, StlUnitPolicy,
};
use karasu_core::mesh::topology::{ComponentPartition, EdgeKind, TopologyLimits, TopologyReport};
use karasu_core::mesh::{MeshDiagnostic, TriangleMesh};

/// Maximum encoded parser input size in bytes.
pub const MAX_INPUT_BYTES: usize = 64 * 1024;
/// Maximum topology-generated source vertices, including unused entries.
pub const MAX_TOPOLOGY_VERTICES: u16 = 512;
/// Maximum topology-generated faces, including degenerates and duplicates.
pub const MAX_TOPOLOGY_TRIANGLES: u16 = 1024;

fn options(format: StlFormat) -> StlOptions {
    StlOptions {
        units: StlUnitPolicy::AssumeMillimeters,
        format,
        limits: ImportLimits {
            max_input_bytes: MAX_INPUT_BYTES,
            max_triangles: 1024,
            max_vertices: 3072,
            max_ascii_line_bytes: 4096,
            max_numeric_token_bytes: 128,
        },
    }
}

fn word(data: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([
        data.get(offset).copied().unwrap_or(0),
        data.get(offset + 1).copied().unwrap_or(0),
    ])
}

fn vector_coordinates(value: Vector3) -> [f64; 3] {
    [value.x, value.y, value.z]
}

fn check_mesh(mesh: &TriangleMesh) {
    let stats = mesh.statistics();
    assert_eq!(stats.vertex_count, mesh.vertices().len());
    assert_eq!(stats.triangle_count, mesh.triangles().len());
    assert_eq!(stats.source_bounds, mesh.source_bounds());
    assert_eq!(mesh.source_bounds().is_none(), mesh.vertices().is_empty());
    for point in mesh.vertices() {
        assert!(point.coordinates().iter().all(|v| v.is_finite()));
    }
    for face in mesh.triangles() {
        assert!(face.iter().all(|&v| (v as usize) < stats.vertex_count));
    }
    if let Some(bounds) = mesh.source_bounds() {
        let low = bounds.min().coordinates();
        let high = bounds.max().coordinates();
        for axis in 0..3 {
            assert!(low[axis].is_finite() && high[axis].is_finite());
            assert!(low[axis] <= high[axis]);
            assert!(mesh.vertices().iter().all(|v| {
                let value = v.coordinates()[axis];
                low[axis] <= value && value <= high[axis]
            }));
            assert!(mesh
                .vertices()
                .iter()
                .any(|v| v.coordinates()[axis] == low[axis]));
            assert!(mesh
                .vertices()
                .iter()
                .any(|v| v.coordinates()[axis] == high[axis]));
        }
        assert!(vector_coordinates(bounds.dimensions())
            .iter()
            .all(|v| v.is_finite() && *v >= 0.0));
        assert!(bounds.center().coordinates().iter().all(|v| v.is_finite()));
    }
    let diagnostics = mesh
        .diagnostics()
        .expect("bounded geometric diagnostics must complete");
    for diagnostic in diagnostics {
        match diagnostic {
            MeshDiagnostic::DegenerateTriangle { triangle, .. } => {
                assert!(triangle < stats.triangle_count)
            }
            MeshDiagnostic::DuplicateTriangle { triangle, first } => {
                assert!(first < triangle && triangle < stats.triangle_count);
            }
            _ => panic!("mesh diagnostics unexpectedly included file metadata"),
        }
    }
}

fn check_imported(imported: &ImportedStl) {
    check_mesh(&imported.mesh);
    assert_eq!(
        imported.mesh.triangles().len(),
        imported.metadata.facets.len()
    );
    let geometric = imported.mesh.diagnostics().expect("bounded diagnostics");
    assert!(imported.diagnostics.starts_with(&geometric));
}

fn check_parser(data: &[u8], format: StlFormat) -> bool {
    let full = options(format);
    let result = import_stl_bytes(data, full);
    if let Ok(ref imported) = result {
        check_imported(imported);
    }
    // Reuse the complete encoded input with stricter, fuzz-controlled caps, including zero.
    // This does not consume a prefix or damage valid seeds before the principal parser call.
    let mut tight = full;
    tight.limits.max_input_bytes = data
        .len()
        .saturating_sub(usize::from(word(data, 0) & 1))
        .min(MAX_INPUT_BYTES);
    tight.limits.max_triangles = usize::from(word(data, 0) % 1025);
    tight.limits.max_vertices = usize::from(word(data, 2) % 3073);
    tight.limits.max_ascii_line_bytes = usize::from(word(data, 4) % 4097);
    tight.limits.max_numeric_token_bytes = usize::from(word(data, 6) % 129);
    if let Ok(imported) = import_stl_bytes(data, tight) {
        check_imported(&imported);
    }
    if format == StlFormat::Auto {
        if let Ok(ref automatic) = result {
            compare_selected_parser(data, automatic);
        }
    }
    result.is_ok()
}

fn compare_selected_parser(data: &[u8], automatic: &ImportedStl) {
    // Compare only the representation actually selected by the public auto path. Do not assert
    // that malformed or ambiguous inputs must also succeed under a different grammar.
    let selected = match automatic.metadata.header {
        StlHeader::Binary(_) => StlFormat::Binary,
        StlHeader::Ascii { .. } => StlFormat::Ascii,
    };
    let explicit =
        import_stl_bytes(data, options(selected)).expect("auto-selected explicit parser");
    assert_eq!(automatic.mesh, explicit.mesh);
    assert_eq!(automatic.diagnostics, explicit.diagnostics);
    assert_eq!(automatic.metadata.header, explicit.metadata.header);
    assert_eq!(automatic.metadata.units, explicit.metadata.units);
    assert_eq!(
        automatic.metadata.facets.len(),
        explicit.metadata.facets.len()
    );
    for (a, b) in automatic
        .metadata
        .facets
        .iter()
        .zip(&explicit.metadata.facets)
    {
        assert_eq!(a.attribute, b.attribute);
        for (a, b) in vector_coordinates(a.stored_normal)
            .into_iter()
            .zip(vector_coordinates(b.stored_normal))
        {
            // Stored NaNs are legal metadata. Ordinary struct equality would reject NaN == NaN.
            assert!(a == b || (a.is_nan() && b.is_nan()));
        }
    }
}

/// Exercises explicitly selected binary import; returns principal-parser acceptance for seed replay.
pub fn check_binary(data: &[u8]) -> bool {
    check_parser(data, StlFormat::Binary)
}

/// Exercises explicitly selected ASCII import; returns principal-parser acceptance for seed replay.
pub fn check_ascii(data: &[u8]) -> bool {
    check_parser(data, StlFormat::Ascii)
}

/// Exercises actual automatic detection and the representation it successfully selected.
pub fn check_auto(data: &[u8]) -> bool {
    check_parser(data, StlFormat::Auto)
}

fn decode_mesh(data: &[u8]) -> TriangleMesh {
    let vertex_count = word(data, 0) % (MAX_TOPOLOGY_VERTICES + 1);
    let face_count = if vertex_count == 0 {
        0
    } else {
        word(data, 2) % (MAX_TOPOLOGY_TRIANGLES + 1)
    };
    let mut offset = 4;
    let mut next_word = || {
        let value = word(data, offset);
        offset += 2;
        value
    };
    let vertices = (0..vertex_count)
        .map(|_| {
            let xyz = [next_word(), next_word(), next_word()]
                .map(|v| f64::from(i16::from_le_bytes(v.to_le_bytes())) / 16.0);
            Point3::new(xyz[0], xyz[1], xyz[2])
        })
        .collect();
    let triangles = (0..face_count)
        .map(|_| [next_word(), next_word(), next_word()].map(|v| u32::from(v % vertex_count)))
        .collect();
    TriangleMesh::try_new(vertices, triangles).expect("bounded finite points and valid indices")
}

/// Decodes bounded finite geometry and valid indices, then checks the real topology report twice.
/// Encoding: LE u16 vertex/face counts modulo 513/1025; i16 XYZ / 16 mm; u16 face indices modulo V.
/// Missing bytes read as zero, V=0 implies F=0, and bytes beyond the bounded records are unused.
pub fn check_topology(data: &[u8]) {
    let mesh = decode_mesh(data);
    let vertices = mesh.vertices().to_vec();
    let triangles = mesh.triangles().to_vec();
    let bounds = mesh.source_bounds();
    let diagnostics = mesh.diagnostics().expect("bounded diagnostics");
    let limits = TopologyLimits {
        max_vertices: usize::from(MAX_TOPOLOGY_VERTICES),
        max_triangles: usize::from(MAX_TOPOLOGY_TRIANGLES),
    };
    let report = mesh
        .analyze_topology_with_limits(limits)
        .expect("bounded topology analysis");
    check_report(&mesh, &report);
    assert_eq!(
        mesh.analyze_topology_with_limits(limits)
            .expect("repeat topology"),
        report
    );
    assert_eq!(mesh.vertices(), vertices);
    assert_eq!(mesh.triangles(), triangles);
    assert_eq!(mesh.source_bounds(), bounds);
    assert_eq!(mesh.diagnostics().expect("repeat diagnostics"), diagnostics);
}

fn check_report(mesh: &TriangleMesh, report: &TopologyReport) {
    let summary = report.summary();
    assert_eq!(summary.source_vertices, mesh.vertices().len());
    assert_eq!(summary.source_triangles, mesh.triangles().len());
    assert_eq!(
        summary.source_triangles,
        summary.usable_triangles + summary.excluded_triangles
    );
    assert_eq!(summary.edges, report.edges().len());
    assert_eq!(
        summary.edges,
        summary.boundary_edges + summary.two_face_edges + summary.non_manifold_edges
    );
    assert!(report
        .edges()
        .windows(2)
        .all(|pair| pair[0].key < pair[1].key));
    let mut use_count = 0;
    for (edge, record) in report.edges().iter().enumerate() {
        let uses = report.incidents(edge).expect("reported edge");
        match record.kind {
            EdgeKind::Boundary => assert_eq!(uses.len(), 1),
            EdgeKind::TwoFace => assert_eq!(uses.len(), 2),
            EdgeKind::NonManifold => assert!(uses.len() > 2),
        }
        if record.winding_conflict {
            assert_eq!(record.kind, EdgeKind::TwoFace);
        }
        use_count += uses.len();
        for use_ in uses {
            let face = mesh.triangles()[use_.triangle];
            let corner = usize::from(use_.local_edge);
            assert!(corner < 3);
            assert_eq!((use_.from, use_.to), (face[corner], face[(corner + 1) % 3]));
            assert_eq!(
                record.key.vertices(),
                [use_.from.min(use_.to), use_.from.max(use_.to)]
            );
        }
    }
    assert_eq!(use_count, 3 * summary.usable_triangles);
    assert_eq!(report.vertices().len(), summary.source_vertices);
    for (vertex, record) in report.vertices().iter().enumerate() {
        let faces = report.incident_faces(vertex).expect("reported vertex");
        assert_eq!(faces.len(), record.usable_faces + record.excluded_faces);
        assert!(faces.windows(2).all(|pair| pair[0] < pair[1]));
        for &face in faces {
            assert!(mesh.triangles()[face].iter().any(|&v| v as usize == vertex));
        }
    }
    assert_eq!(
        report.geometry_diagnostics(),
        mesh.diagnostics().expect("bounded diagnostics")
    );
    let mut excluded = vec![false; summary.source_triangles];
    for issue in report.geometry_diagnostics() {
        if let MeshDiagnostic::DegenerateTriangle { triangle, .. } = *issue {
            excluded[triangle] = true;
        }
    }
    check_partition(mesh, report.edge_components(), &excluded);
    check_partition(mesh, report.vertex_components(), &excluded);
    assert_eq!(
        report.edge_components().components().len(),
        summary.edge_components
    );
    assert_eq!(
        report.vertex_components().components().len(),
        summary.vertex_components
    );
}

fn check_partition(mesh: &TriangleMesh, partition: &ComponentPartition, excluded: &[bool]) {
    let mut seen = vec![false; mesh.triangles().len()];
    let mut previous_min = None;
    for (id, component) in partition.components().iter().enumerate() {
        let faces = partition.triangles(id).expect("component faces");
        let vertices = partition.vertices(id).expect("component vertices");
        assert_eq!(faces.len(), component.triangle_count);
        assert_eq!(vertices.len(), component.vertex_count);
        assert!(!faces.is_empty());
        assert!(faces.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(vertices.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(previous_min.is_none_or(|min| min < faces[0]));
        previous_min = Some(faces[0]);
        for &vertex in vertices {
            assert!((vertex as usize) < mesh.vertices().len());
        }
        for &face in faces {
            assert!(!excluded[face] && !seen[face]);
            seen[face] = true;
            for vertex in mesh.triangles()[face] {
                assert!(vertices.binary_search(&vertex).is_ok());
            }
        }
        assert_eq!(
            component.edge_count,
            component.boundary_edges + component.two_face_edges + component.non_manifold_edges
        );
    }
    for (face, &excluded) in excluded.iter().enumerate() {
        assert_eq!(seen[face], !excluded);
    }
}
