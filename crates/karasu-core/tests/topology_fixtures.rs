//! Owned fixture regressions with independent construction counts and source-preservation checks.

use karasu_core::geometry::{ObjectTransform, Point3, Rotation3, Vector3};
use karasu_core::mesh::stl::{import_stl_path, ImportedStl, StlOptions, StlUnitPolicy};
use karasu_core::mesh::topology::{
    CheckStatus, DirectedEdge, EdgeIncidence, EdgeKind, TopologyIssue, TopologyReport,
    VertexDefect, VertexKind,
};
use karasu_core::mesh::{Degeneracy, MeshDiagnostic};
use std::{path::PathBuf, time::Instant};

const MANIFEST: &str = include_str!("../../../fixtures/reference/manifest.json");

fn text_field<'a>(entry: &'a str, name: &str) -> &'a str {
    entry
        .split_once(&format!("\"{name}\":"))
        .unwrap()
        .1
        .trim_start()
        .strip_prefix('"')
        .unwrap()
        .split('"')
        .next()
        .unwrap()
}

fn fixture_path(id: &str, variant: Option<&str>) -> PathBuf {
    // Read only controlled manifest primitive fields, not arbitrary JSON. No extra dependency.
    let entry = MANIFEST
        .split("\"fixture_id\":")
        .skip(1)
        .find(|entry| {
            text_field(&format!("\"fixture_id\":{entry}"), "fixture_id") == id
                && match variant {
                    Some(v) => text_field(entry, "variant") == v,
                    None => entry
                        .split_once("\"variant\":")
                        .unwrap()
                        .1
                        .trim_start()
                        .starts_with("null"),
                }
        })
        .unwrap();
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/reference")
        .join(text_field(entry, "filename"))
}

fn fixture(id: &str, variant: Option<&str>) -> ImportedStl {
    import_stl_path(
        fixture_path(id, variant),
        StlOptions::new(StlUnitPolicy::AssumeMillimeters),
    )
    .unwrap()
}

fn counts(r: &TopologyReport, expected: [usize; 7]) {
    let s = r.summary();
    assert_eq!(
        [
            s.source_vertices,
            s.source_triangles,
            s.usable_triangles,
            s.edges,
            s.boundary_edges,
            s.non_manifold_edges,
            s.winding_conflicts
        ],
        expected
    );
    assert_eq!(
        s.two_face_edges,
        s.edges - s.boundary_edges - s.non_manifold_edges
    );
    assert_eq!(s.edge_components, 1);
    assert_eq!(s.vertex_components, 1);
    for partition in [r.edge_components(), r.vertex_components()] {
        let c = &partition.components()[0];
        assert_eq!(
            (
                c.triangle_count,
                c.edge_count,
                c.boundary_edges,
                c.non_manifold_edges,
                c.winding_conflicts
            ),
            (
                s.usable_triangles,
                s.edges,
                s.boundary_edges,
                s.non_manifold_edges,
                s.winding_conflicts
            )
        );
    }
}

fn closed(r: &TopologyReport, vertex_count: usize) {
    assert!(r.summary().closed_combinatorial_surface);
    assert_eq!(r.summary().interior_vertices, vertex_count);
    assert_eq!(r.summary().edge_manifold, CheckStatus::Satisfied);
    assert_eq!(r.summary().vertex_manifold, CheckStatus::Satisfied);
    assert_eq!(r.summary().local_winding, CheckStatus::Satisfied);
    assert_eq!(r.summary().excluded_triangles, 0);
    assert_eq!(r.summary().duplicate_triangles, 0);
    assert_eq!(r.issues(), []);
}

#[test]
fn fx01_cube_has_eighteen_edges_and_eight_cycle_links() {
    // Six quads, each with one diagonal: 12 sides + 6 diagonals, 12 triangles.
    let r = fixture("FX-01", None).mesh.analyze_topology().unwrap();
    counts(&r, [8, 12, 12, 18, 0, 0, 0]);
    closed(&r, 8);
}

#[test]
fn fx02_128_segment_cylinder_has_768_edges_and_258_cycle_links() {
    // Two 128-vertex rings + 2 cap centers. Per segment: 2 wall triangles + 2 cap triangles.
    // Edges: 256 ring + 128 vertical + 128 wall diagonals + 256 cap spokes = 768.
    let r = fixture("FX-02", None).mesh.analyze_topology().unwrap();
    counts(&r, [258, 512, 512, 768, 0, 0, 0]);
    closed(&r, 258);
}

#[test]
fn fx05_baked_oblique_prism_retains_cube_combinatorics() {
    let cube = fixture("FX-01", None).mesh.analyze_topology().unwrap();
    let r = fixture("FX-05", None).mesh.analyze_topology().unwrap();
    counts(&r, [8, 12, 12, 18, 0, 0, 0]);
    closed(&r, 8);
    assert_eq!(r, cube); // Serialization retains the same first-occurrence index/face ordering.
}

#[test]
fn fx08_excludes_two_faces_but_retains_duplicate_edge_contributions() {
    let imported = fixture("FX-08", None);
    let r = imported.mesh.analyze_topology().unwrap();
    // Cube edges remain 18. Added face 14 duplicates face 0, so its 3 edges gain a third use.
    // Faces 12 (0,0,3) and 13 (0,8,3) are excluded; vertex 8 has no usable incidence.
    counts(&r, [9, 15, 13, 18, 0, 3, 0]);
    assert_eq!(r.summary().excluded_triangles, 2);
    assert_eq!(r.summary().duplicate_triangles, 1);
    assert_eq!(
        r.geometry_diagnostics(),
        [
            MeshDiagnostic::DegenerateTriangle {
                triangle: 12,
                kind: Degeneracy::RepeatedVertex
            },
            MeshDiagnostic::DegenerateTriangle {
                triangle: 13,
                kind: Degeneracy::ZeroComputedArea
            },
            MeshDiagnostic::DuplicateTriangle {
                triangle: 14,
                first: 0
            },
        ]
    );
    let non_manifold: Vec<_> = r
        .edges()
        .iter()
        .filter(|e| e.kind == EdgeKind::NonManifold)
        .map(|e| e.key.vertices())
        .collect();
    assert_eq!(non_manifold, [[0, 1], [0, 2], [1, 2]]);
    for vertex in [0, 3, 8] {
        assert_eq!(r.vertices()[vertex].kind, VertexKind::Indeterminate);
    }
    for vertex in [1, 2] {
        assert_eq!(
            r.vertices()[vertex].kind,
            VertexKind::NonManifold(VertexDefect::DuplicateLinkEdge)
        );
    }
    assert_eq!(
        (
            r.summary().interior_vertices,
            r.summary().indeterminate_vertices,
            r.summary().non_manifold_vertices
        ),
        (4, 3, 2)
    );
    assert_eq!(r.vertices()[8].usable_faces, 0);
    assert_eq!(r.vertices()[8].excluded_faces, 1);
    assert_eq!(r.incident_faces(8), Some([13].as_slice()));
    assert_eq!(
        r.edge_components().vertices(0),
        Some([0, 1, 2, 3, 4, 5, 6, 7].as_slice())
    );
    assert_eq!(r.summary().edge_manifold, CheckStatus::Failed);
    assert_eq!(r.summary().vertex_manifold, CheckStatus::Failed);
    assert_eq!(r.summary().local_winding, CheckStatus::Indeterminate);
    assert!(!r.summary().closed_combinatorial_surface);
    assert_eq!(imported.mesh.triangles().len(), 15);
    assert_eq!(imported.mesh.triangles()[12], [0, 0, 3]);
    assert_eq!(imported.mesh.triangles()[13], [0, 8, 3]);
}

#[test]
fn fx09_reversing_one_face_conflicts_on_exactly_its_three_edges() {
    let imported = fixture("FX-09", None);
    let before = imported.mesh.triangles().to_vec();
    let r = imported.mesh.analyze_topology().unwrap();
    counts(&r, [8, 12, 12, 18, 0, 0, 3]);
    let conflicts: Vec<_> = r
        .issues()
        .iter()
        .filter_map(|issue| match *issue {
            TopologyIssue::WindingConflict { edge, triangles } => {
                Some((r.edges()[edge].key.vertices(), triangles))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        conflicts,
        [([0, 1], [0, 1]), ([0, 2], [0, 10]), ([1, 2], [0, 8])]
    );
    assert_eq!(r.summary().local_winding, CheckStatus::Failed);
    assert_eq!(r.summary().interior_vertices, 8);
    assert!(r.summary().closed_combinatorial_surface);
    assert_eq!(imported.mesh.triangles(), before);
}

#[test]
fn fx10a_missing_top_quad_leaves_four_boundary_edges() {
    // Remove two top triangles: 10 faces, 4 boundary edges. 3F=2*13+4 => E=17.
    let r = fixture("FX-10", Some("A")).mesh.analyze_topology().unwrap();
    counts(&r, [8, 10, 10, 17, 4, 0, 0]);
    assert_eq!(
        (r.summary().interior_vertices, r.summary().boundary_vertices),
        (4, 4)
    );
    assert_eq!(r.summary().vertex_manifold, CheckStatus::Satisfied);
    assert_eq!(r.summary().local_winding, CheckStatus::Satisfied);
    assert!(!r.summary().closed_combinatorial_surface);
}

#[test]
fn fx10b_added_face_creates_one_three_face_edge_and_two_boundary_edges() {
    // One cube edge gains a third use; two new edges bound the added triangle: E=18+2.
    // Generator edge 0-1 becomes imported edge 0-3 under first-occurrence STL indexing.
    let r = fixture("FX-10", Some("B")).mesh.analyze_topology().unwrap();
    counts(&r, [9, 13, 13, 20, 2, 1, 0]);
    let defective: Vec<_> = r
        .edges()
        .iter()
        .enumerate()
        .filter(|(_, e)| e.kind == EdgeKind::NonManifold)
        .collect();
    assert_eq!(defective.len(), 1);
    let (edge, record) = defective[0];
    assert_eq!(record.key.vertices(), [0, 3]);
    assert_eq!(
        r.incidents(edge)
            .unwrap()
            .iter()
            .map(|u| u.triangle)
            .collect::<Vec<_>>(),
        [1, 4, 12]
    );
    for vertex in [0, 3] {
        assert_eq!(
            r.vertices()[vertex].kind,
            VertexKind::NonManifold(VertexDefect::BranchedLink)
        );
    }
    assert_eq!(
        (
            r.summary().interior_vertices,
            r.summary().boundary_vertices,
            r.summary().non_manifold_vertices
        ),
        (6, 1, 2)
    );
    assert_eq!(r.summary().edge_manifold, CheckStatus::Failed);
    assert_eq!(r.summary().local_winding, CheckStatus::Indeterminate);
    assert!(!r.summary().closed_combinatorial_surface);
}

#[test]
fn all_small_fixture_reports_repeat_without_source_or_metadata_mutation() {
    for (id, variant) in [
        ("FX-01", None),
        ("FX-02", None),
        ("FX-03", None),
        ("FX-04", None),
        ("FX-05", None),
        ("FX-06", None),
        ("FX-07", None),
        ("FX-08", None),
        ("FX-09", None),
        ("FX-10", Some("A")),
        ("FX-10", Some("B")),
    ] {
        let imported = fixture(id, variant);
        let original = fixture(id, variant);
        let report = imported.mesh.analyze_topology().unwrap();
        let transform = ObjectTransform::new(
            Vector3::new(15., 10., 5.),
            Rotation3::around_z_radians(0.7).unwrap(),
            Vector3::new(-1.25, 2., 3.),
            Point3::new(1., 2., 3.),
        )
        .unwrap();
        imported.mesh.world_bounds(&transform).unwrap();
        imported
            .mesh
            .transformed_bounds(&transform.to_affine().unwrap())
            .unwrap();
        for _ in 0..3 {
            assert_eq!(imported.mesh.analyze_topology().unwrap(), report);
        }
        assert_eq!(imported, original); // Coordinates, indices, bounds, file normals/attributes/header/diagnostics.
    }
}

#[test]
fn other_closed_fixtures_have_expected_component_counts_without_nesting_claims() {
    for (id, v, f, e, components) in [
        ("FX-03", 642, 1280, 1920, 1),
        ("FX-04", 16, 24, 36, 2),
        ("FX-06", 16, 24, 36, 2),
        ("FX-07", 170, 332, 498, 2),
    ] {
        let r = fixture(id, None).mesh.analyze_topology().unwrap();
        let s = r.summary();
        assert_eq!(
            (
                s.source_vertices,
                s.source_triangles,
                s.edges,
                s.edge_components,
                s.vertex_components
            ),
            (v, f, e, components, components)
        );
        closed(&r, v);
    }
}

#[test]
#[ignore = "requires the existing optional FX-11 file; not a controlled benchmark"]
fn optional_fx11_bounded_topology_measurement() {
    let read_start = Instant::now();
    let imported = fixture("FX-11", None);
    let import_elapsed = read_start.elapsed();
    let topology_start = Instant::now();
    let r = imported.mesh.analyze_topology().unwrap();
    let topology_elapsed = topology_start.elapsed();
    // Subdivision-7 icosphere: F=20*4^7, V=10*4^7+2, E=30*4^7.
    counts(&r, [163_842, 327_680, 327_680, 491_520, 0, 0, 0]);
    closed(&r, 163_842);
    println!("FX-11 read/import={import_elapsed:?}; topology={topology_elapsed:?}; directed use={} bytes, edge record={} bytes",
        size_of::<DirectedEdge>(),size_of::<EdgeIncidence>());
}
