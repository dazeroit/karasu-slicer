//! Raw supplied-plane mesh sections: independent analytic oracles and immutable source incidence.
#![allow(clippy::float_cmp)] // Exact incidence, dyadic geometry and source copies are the contract.

use std::{error::Error, path::PathBuf};

use karasu_core::cancellation::{Cancellation, Cancelled, Checkpoint};
use karasu_core::geometry::section::{
    intersect_triangle_z, CornerId, EndpointCoincidence, EndpointOrigin, LocalEdge,
    TriangleAreaObservation, TrianglePlaneError, TrianglePlaneEvent, VertexPlaneSide,
};
use karasu_core::geometry::{Affine3, ObjectTransform, Point3, Rotation3, TransformError, Vector3};
use karasu_core::mesh::stl::{import_stl_path, ImportedStl, StlOptions, StlUnitPolicy};
use karasu_core::mesh::TriangleMesh;
use karasu_core::slicing::{
    section_mesh_z, section_mesh_z_with_cancellation, FaceSection, MeshEndpointOrigin, MeshSection,
    SectionError, SectionLimits, SectionResource, SectionTransform,
};

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

fn fixture(id: &str, variant: Option<&str>) -> ImportedStl {
    // Controlled manifest fields only, not an arbitrary JSON parser or a guessed filename.
    let entry = MANIFEST
        .split("\"fixture_id\":")
        .skip(1)
        .find(|entry| {
            text_field(&format!("\"fixture_id\":{entry}"), "fixture_id") == id
                && variant.map_or_else(
                    || {
                        entry
                            .split_once("\"variant\":")
                            .unwrap()
                            .1
                            .trim_start()
                            .starts_with("null")
                    },
                    |value| text_field(entry, "variant") == value,
                )
        })
        .unwrap();
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/reference")
        .join(text_field(entry, "filename"));
    import_stl_path(path, StlOptions::new(StlUnitPolicy::AssumeMillimeters)).unwrap()
}

fn one(points: [Point3; 3]) -> TriangleMesh {
    TriangleMesh::try_new(points.to_vec(), vec![[0, 1, 2]]).unwrap()
}

fn crossing() -> TriangleMesh {
    one([
        Point3::new(0.0, 0.0, -1.0),
        Point3::new(4.0, 0.0, 1.0),
        Point3::new(0.0, 4.0, 1.0),
    ])
}

fn section(mesh: &TriangleMesh, z: f64) -> MeshSection {
    section_mesh_z(
        mesh,
        z,
        SectionTransform::Identity,
        SectionLimits::default(),
    )
    .unwrap()
}

fn points(event: TrianglePlaneEvent) -> Vec<Point3> {
    match event {
        TrianglePlaneEvent::None => vec![],
        TrianglePlaneEvent::TangentPoint { endpoint } => vec![endpoint.position],
        TrianglePlaneEvent::Segment { endpoints, .. }
        | TrianglePlaneEvent::CoplanarEdge { endpoints, .. } => {
            endpoints.map(|e| e.position).to_vec()
        }
        TrianglePlaneEvent::CoplanarTriangle { corners } => corners.map(|c| c.position).to_vec(),
    }
}

fn kind(event: TrianglePlaneEvent) -> usize {
    match event {
        TrianglePlaneEvent::None => 0,
        TrianglePlaneEvent::TangentPoint { .. } => 1,
        TrianglePlaneEvent::Segment { .. } => 2,
        TrianglePlaneEvent::CoplanarEdge { .. } => 3,
        TrianglePlaneEvent::CoplanarTriangle { .. } => 4,
    }
}

fn kinds(result: &MeshSection) -> [usize; 5] {
    let mut counts = [result.summary().non_intersecting_faces, 0, 0, 0, 0];
    for face in result.events() {
        counts[kind(face.intersection.event)] += 1;
    }
    assert_eq!(counts.iter().sum::<usize>(), result.summary().source_faces);
    assert_eq!(result.events().len(), result.summary().retained_events);
    let s = result.summary();
    assert_eq!(
        s.source_faces,
        s.repeated_position_faces
            + s.zero_computed_cross_faces
            + s.nonzero_computed_cross_faces
            + s.unavailable_area_faces
    );
    counts
}

fn assert_bits_equal(left: &MeshSection, right: &MeshSection) {
    assert_eq!(left, right);
    assert_eq!(left.plane_z().to_bits(), right.plane_z().to_bits());
    for (a, b) in left.events().iter().zip(right.events()) {
        let a = points(a.intersection.event);
        let b = points(b.intersection.event);
        assert_eq!(a.len(), b.len());
        for (a, b) in a.into_iter().zip(b) {
            assert_eq!(
                a.coordinates().map(f64::to_bits),
                b.coordinates().map(f64::to_bits)
            );
        }
    }
}

fn assert_kernel_scan(mesh: &TriangleMesh, z: f64, transform: SectionTransform<'_>) {
    let result = section_mesh_z(mesh, z, transform, SectionLimits::default()).unwrap();
    let mut retained = 0;
    let mut none = 0;
    for (face, &indices) in mesh.triangles().iter().enumerate() {
        let world = indices.map(|v| {
            let p = mesh.vertices()[v as usize];
            match transform {
                SectionTransform::Identity => p,
                SectionTransform::Object(t) => t.transform_point(p).unwrap(),
                SectionTransform::Affine(t) => t.transform_point(p).unwrap(),
            }
        });
        let local = intersect_triangle_z(world, z).unwrap();
        if matches!(local.event, TrianglePlaneEvent::None) {
            none += 1;
            continue;
        }
        let record = &result.events()[retained];
        assert_eq!(record.source_face, face);
        assert_eq!(record.source_vertices, indices);
        assert_eq!(record.intersection, local);
        for corner in [CornerId::Corner0, CornerId::Corner1, CornerId::Corner2] {
            assert_eq!(record.source_vertex(corner), indices[corner as usize]);
        }
        assert_eq!(
            record.source_edge(LocalEdge::Edge01),
            [indices[0], indices[1]]
        );
        assert_eq!(
            record.source_edge(LocalEdge::Edge12),
            [indices[1], indices[2]]
        );
        assert_eq!(
            record.source_edge(LocalEdge::Edge20),
            [indices[2], indices[0]]
        );
        retained += 1;
    }
    assert_eq!(retained, result.events().len());
    assert_eq!(none, result.summary().non_intersecting_faces);
    kinds(&result);
}

#[test]
fn empty_and_unused_only_meshes_have_complete_zero_totals() {
    for mesh in [
        TriangleMesh::try_new(vec![], vec![]).unwrap(),
        TriangleMesh::try_new(vec![Point3::new(7.0, 2.0, 3.0)], vec![]).unwrap(),
    ] {
        let result = section(&mesh, 7.0);
        assert_eq!(result.plane_z(), 7.0);
        assert_eq!(kinds(&result), [0; 5]);
    }
}

#[test]
fn wholly_above_and_below_faces_are_counted_without_output_storage() {
    let mesh = crossing();
    for z in [-2.0, 2.0] {
        let result = section_mesh_z(
            &mesh,
            z,
            SectionTransform::Identity,
            SectionLimits {
                max_events: 0,
                max_event_bytes: 0,
                ..SectionLimits::default()
            },
        )
        .unwrap();
        assert_eq!(kinds(&result), [1, 0, 0, 0, 0]);
        assert_eq!(result.summary().nonzero_computed_cross_faces, 1);
    }
}

#[test]
fn analytic_triangle_preserves_canonical_endpoints_and_indexed_origins() {
    let result = section(&crossing(), 0.0);
    let record = result.events()[0];
    let TrianglePlaneEvent::Segment {
        endpoints,
        coincidence,
    } = record.intersection.event
    else {
        panic!("segment required")
    };
    assert_eq!(
        endpoints.map(|e| e.position),
        [Point3::new(0.0, 2.0, 0.0), Point3::new(2.0, 0.0, 0.0)]
    );
    assert_eq!(coincidence, EndpointCoincidence::Distinct);
    assert_eq!(
        record.endpoint_origin(endpoints[0].origin),
        MeshEndpointOrigin::Edge {
            edge: LocalEdge::Edge20,
            vertices: [2, 0]
        }
    );
    assert_eq!(
        record.endpoint_origin(endpoints[1].origin),
        MeshEndpointOrigin::Edge {
            edge: LocalEdge::Edge01,
            vertices: [0, 1]
        }
    );
}

#[test]
fn all_27_side_words_traverse_in_source_order_with_none_context_totals() {
    let mut vertices = Vec::new();
    let mut faces = Vec::new();
    for a in [-1.0, 0.0, 1.0] {
        for b in [-1.0, 0.0, 1.0] {
            for c in [-1.0, 0.0, 1.0] {
                let base = u32::try_from(vertices.len()).unwrap();
                vertices.extend([
                    Point3::new(0.0, 0.0, a),
                    Point3::new(4.0, 0.0, b),
                    Point3::new(0.0, 4.0, c),
                ]);
                faces.push([base, base + 1, base + 2]);
            }
        }
    }
    let mesh = TriangleMesh::try_new(vertices, faces).unwrap();
    let result = section(&mesh, 0.0);
    assert_eq!(kinds(&result), [2, 6, 12, 6, 1]);
    assert_eq!(result.summary().nonzero_computed_cross_faces, 27);
    assert!(result
        .events()
        .windows(2)
        .all(|pair| pair[0].source_face < pair[1].source_face));
    assert_kernel_scan(&mesh, 0.0, SectionTransform::Identity);
}

#[test]
fn cube_interior_has_eight_half_sides_with_exact_square_support() {
    let imported = fixture("FX-01", None);
    let result = section(&imported.mesh, 10.0);
    assert_eq!(kinds(&result), [4, 0, 8, 0, 0]);
    let expected = [
        [-10.0, -10.0],
        [-10.0, 0.0],
        [-10.0, 10.0],
        [0.0, -10.0],
        [0.0, 10.0],
        [10.0, -10.0],
        [10.0, 0.0],
        [10.0, 10.0],
    ];
    let mut occurrences = [0; 8];
    for record in result.events() {
        let p = points(record.intersection.event);
        for point in &p {
            assert_eq!(point.z, 10.0);
            let i = expected
                .iter()
                .position(|xy| *xy == [point.x, point.y])
                .unwrap();
            occurrences[i] += 1;
        }
        assert_eq!((p[1].x - p[0].x).abs() + (p[1].y - p[0].y).abs(), 10.0);
        assert!(
            (p[0].x == p[1].x && p[0].x.abs() == 10.0)
                || (p[0].y == p[1].y && p[0].y.abs() == 10.0)
        );
    }
    assert_eq!(occurrences, [2; 8]);
}

#[test]
fn cube_boundary_planes_retain_all_coplanar_and_tangent_incidence() {
    let imported = fixture("FX-01", None);
    for z in [0.0, 20.0] {
        let result = section(&imported.mesh, z);
        assert_eq!(kinds(&result), [2, 4, 0, 4, 2]);
        for record in result.events() {
            for p in points(record.intersection.event) {
                assert_eq!(p.z, z);
            }
        }
    }
}

#[test]
fn cylinder_midplane_is_the_128_chord_polygon_split_at_wall_diagonals() {
    let imported = fixture("FX-02", None);
    let result = section(&imported.mesh, 10.0);
    assert_eq!(kinds(&result), [256, 0, 256, 0, 0]);
    // Independent side-wall geometry: each event runs from a ring vertex XY to the midpoint of
    // a neighboring ring chord. Cap centers must never appear at this plane.
    for record in result.events() {
        let TrianglePlaneEvent::Segment { endpoints, .. } = record.intersection.event else {
            panic!("segment required")
        };
        let source = record
            .source_vertices
            .map(|v| imported.mesh.vertices()[v as usize]);
        let mut unique_xy = Vec::new();
        for p in source {
            if !unique_xy.contains(&[p.x, p.y]) {
                unique_xy.push([p.x, p.y]);
            }
        }
        assert_eq!(unique_xy.len(), 2);
        let midpoint = [
            unique_xy[0][0].midpoint(unique_xy[1][0]),
            unique_xy[0][1].midpoint(unique_xy[1][1]),
        ];
        let xy = endpoints.map(|e| [e.position.x, e.position.y]);
        assert!(xy.contains(&midpoint));
        assert!(unique_xy.iter().any(|p| xy.contains(p)));
        assert!(xy.iter().all(|p| p[0].abs() <= 10.0 && p[1].abs() <= 10.0));
    }
}

#[test]
fn cylinder_cap_planes_preserve_caps_edges_and_tangencies() {
    let imported = fixture("FX-02", None);
    for z in [0.0, 20.0] {
        assert_eq!(kinds(&section(&imported.mesh, z)), [128, 128, 0, 128, 128]);
    }
}

#[test]
fn inclined_triangular_prism_matches_independent_exact_midplane_support() {
    let mesh = TriangleMesh::try_new(
        vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(4.0, 0.0, 0.0),
            Point3::new(0.0, 6.0, 0.0),
            Point3::new(2.0, 0.0, 4.0),
            Point3::new(6.0, 0.0, 4.0),
            Point3::new(2.0, 6.0, 4.0),
        ],
        vec![
            [0, 2, 1],
            [3, 4, 5],
            [0, 1, 4],
            [0, 4, 3],
            [1, 2, 5],
            [1, 5, 4],
            [2, 0, 3],
            [2, 3, 5],
        ],
    )
    .unwrap();
    let result = section(&mesh, 2.0);
    assert_eq!(kinds(&result), [2, 0, 6, 0, 0]);
    let expected = [
        [1.0, 0.0],
        [3.0, 0.0],
        [5.0, 0.0],
        [3.0, 3.0],
        [1.0, 6.0],
        [1.0, 3.0],
    ];
    let mut counts = [0; 6];
    for record in result.events() {
        for p in points(record.intersection.event) {
            assert_eq!(p.z, 2.0);
            counts[expected.iter().position(|xy| *xy == [p.x, p.y]).unwrap()] += 1;
            assert!(p.x == 1.0 || p.y == 0.0 || 3.0 * p.x + 2.0 * p.y == 15.0);
        }
    }
    assert_eq!(counts, [2; 6]);
}

fn near_point(actual: Point3, expected: Point3, budget: f64) {
    for (a, e) in actual.coordinates().into_iter().zip(expected.coordinates()) {
        assert!((a - e).abs() <= budget, "{actual:?} != {expected:?}");
    }
}

#[test]
fn fx05_oblique_sections_match_independent_line_equations_at_serialized_vertices() {
    let imported = fixture("FX-05", None);
    for z in [5.0, 17.0, 25.0] {
        let result = section(&imported.mesh, z);
        assert!(result.events().len() >= 3);
        for record in result.events() {
            let TrianglePlaneEvent::Segment { endpoints, .. } = record.intersection.event else {
                panic!("strict interior plane required")
            };
            for endpoint in endpoints {
                let EndpointOrigin::Edge(edge) = endpoint.origin else {
                    panic!("no On corner at chosen planes")
                };
                let [a, b] = record
                    .source_edge(edge)
                    .map(|v| imported.mesh.vertices()[v as usize]);
                let t = (z - a.z) / (b.z - a.z);
                let expected = Point3::new(a.x + t * (b.x - a.x), a.y + t * (b.y - a.y), z);
                // Independent line equation vs checked convex formula, |coordinates|<50 mm.
                near_point(endpoint.position, expected, 128.0 * f64::EPSILON * 50.0);
            }
        }
    }
}

#[test]
fn coplanar_faces_and_collapsed_edges_keep_local_and_mesh_identity() {
    let mesh = TriangleMesh::try_new(
        vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(0.0, 0.0, 1.0),
        ],
        vec![[0, 0, 2], [0, 1, 0], [0, 0, 0]],
    )
    .unwrap();
    let result = section(&mesh, 0.0);
    assert_eq!(kinds(&result), [0, 0, 0, 1, 2]);
    assert_eq!(result.summary().repeated_position_faces, 3);
    let record = result.events()[0];
    assert_eq!(record.source_edge(LocalEdge::Edge01), [0, 0]);
    let TrianglePlaneEvent::CoplanarEdge {
        endpoints,
        coincidence,
        ..
    } = record.intersection.event
    else {
        panic!("coplanar edge required")
    };
    assert_eq!(coincidence, EndpointCoincidence::CoincidentRepresented);
    assert_ne!(
        record.endpoint_origin(endpoints[0].origin),
        record.endpoint_origin(endpoints[1].origin)
    );
    let TrianglePlaneEvent::CoplanarTriangle { corners } = result.events()[2].intersection.event
    else {
        panic!("all corners required")
    };
    assert_eq!(
        corners.map(|c| result.events()[2].source_vertex(c.corner)),
        [0, 0, 0]
    );
    assert_eq!(
        corners.map(|c| c.corner),
        [CornerId::Corner0, CornerId::Corner1, CornerId::Corner2]
    );
}

#[test]
fn tangent_vertex_is_a_source_vertex_not_a_fabricated_edge() {
    let mesh = one([
        Point3::new(2.0, 3.0, 0.0),
        Point3::new(0.0, 0.0, 1.0),
        Point3::new(4.0, 0.0, 1.0),
    ]);
    let record = section(&mesh, 0.0).events()[0];
    let TrianglePlaneEvent::TangentPoint { endpoint } = record.intersection.event else {
        panic!("tangent required")
    };
    assert_eq!(endpoint.position, mesh.vertices()[0]);
    assert_eq!(
        record.endpoint_origin(endpoint.origin),
        MeshEndpointOrigin::Vertex {
            corner: CornerId::Corner0,
            vertex: 0
        }
    );
}

#[test]
fn collinear_and_repeated_none_context_is_not_silently_excluded() {
    let mesh = TriangleMesh::try_new(
        vec![
            Point3::new(0.0, 0.0, -1.0),
            Point3::new(0.0, 0.0, 1.0),
            Point3::new(0.0, 0.0, 3.0),
        ],
        vec![[0, 1, 2], [0, 0, 0]],
    )
    .unwrap();
    let result = section(&mesh, 0.0);
    assert_eq!(kinds(&result), [1, 0, 1, 0, 0]);
    assert_eq!(result.summary().zero_computed_cross_faces, 1);
    assert_eq!(result.summary().repeated_position_faces, 1);
    let TrianglePlaneEvent::Segment {
        coincidence,
        endpoints,
    } = result.events()[0].intersection.event
    else {
        panic!("segment incidence required")
    };
    assert_eq!(coincidence, EndpointCoincidence::CoincidentRepresented);
    assert_ne!(endpoints[0].origin, endpoints[1].origin);
}

#[test]
fn duplicate_reversed_and_disconnected_coincident_faces_remain_separate_records() {
    let original = crossing();
    let mut v = original.vertices().to_vec();
    v.extend_from_slice(original.vertices());
    let mesh = TriangleMesh::try_new(v, vec![[0, 1, 2], [0, 1, 2], [2, 1, 0], [3, 4, 5]]).unwrap();
    let result = section(&mesh, 0.0);
    assert_eq!(kinds(&result), [0, 0, 4, 0, 0]);
    let base = points(result.events()[0].intersection.event);
    for (id, record) in result.events().iter().enumerate() {
        assert_eq!(record.source_face, id);
        assert_eq!(points(record.intersection.event), base);
        assert_eq!(record.source_vertices, mesh.triangles()[id]);
    }
    assert_eq!(
        result.events()[0].intersection,
        result.events()[1].intersection
    );
    assert_ne!(
        result.events()[0].source_edge(LocalEdge::Edge01),
        result.events()[3].source_edge(LocalEdge::Edge01)
    );
}

#[test]
fn all_small_canonical_fixtures_match_local_kernel_without_topology_filtering() {
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
        for z in [0.0, 10.0, 20.0] {
            assert_kernel_scan(&imported.mesh, z, SectionTransform::Identity);
        }
    }
}

#[test]
fn face_reordering_changes_only_source_order_and_ids_not_geometry_multiplicity() {
    let mesh = crossing();
    let v = mesh.vertices().to_vec();
    let a = TriangleMesh::try_new(v.clone(), vec![[0, 1, 2], [0, 0, 0], [2, 1, 0]]).unwrap();
    let b = TriangleMesh::try_new(v, vec![[2, 1, 0], [0, 1, 2], [0, 0, 0]]).unwrap();
    let sa = section(&a, 0.0);
    let sb = section(&b, 0.0);
    assert_eq!(
        sa.events()
            .iter()
            .map(|r| r.source_face)
            .collect::<Vec<_>>(),
        vec![0, 2]
    );
    assert_eq!(
        sb.events()
            .iter()
            .map(|r| r.source_face)
            .collect::<Vec<_>>(),
        vec![0, 1]
    );
    assert_eq!(sa.events()[0].intersection, sb.events()[1].intersection);
    assert_eq!(sa.events()[1].intersection, sb.events()[0].intersection);
    assert_eq!(sa.summary(), sb.summary());
}

#[test]
fn identity_modes_agree_and_translation_uses_world_not_source_z() {
    let mesh = crossing();
    let base = section(&mesh, 0.0);
    assert_bits_equal(
        &base,
        &section_mesh_z(
            &mesh,
            0.0,
            SectionTransform::Object(&ObjectTransform::identity()),
            SectionLimits::default(),
        )
        .unwrap(),
    );
    assert_bits_equal(
        &base,
        &section_mesh_z(
            &mesh,
            0.0,
            SectionTransform::Affine(&Affine3::identity()),
            SectionLimits::default(),
        )
        .unwrap(),
    );
    let t = Affine3::from_translation(Vector3::new(8.0, -4.0, 100.0)).unwrap();
    let result = section_mesh_z(
        &mesh,
        100.0,
        SectionTransform::Affine(&t),
        SectionLimits::default(),
    )
    .unwrap();
    assert_eq!(
        points(result.events()[0].intersection.event),
        vec![
            Point3::new(8.0, -2.0, 100.0),
            Point3::new(10.0, -4.0, 100.0)
        ]
    );
    assert_eq!(section(&mesh, 100.0).events().len(), 0);
}

#[test]
fn exact_axis_permutation_rotation_uses_transformed_z_not_source_bounds() {
    // Proper 90-degree Y rotation expressed exactly as a caller-supplied affine.
    let mesh = one([
        Point3::new(-2.0, 0.0, 10.0),
        Point3::new(2.0, 0.0, 10.0),
        Point3::new(2.0, 4.0, 10.0),
    ]);
    let t = Affine3::from_parts(
        [[0.0, 0.0, 1.0], [0.0, 1.0, 0.0], [-1.0, 0.0, 0.0]],
        Vector3::new(0.0, 0.0, 0.0),
    )
    .unwrap();
    let result = section_mesh_z(
        &mesh,
        0.0,
        SectionTransform::Affine(&t),
        SectionLimits::default(),
    )
    .unwrap();
    assert_eq!(
        points(result.events()[0].intersection.event),
        vec![Point3::new(10.0, 0.0, 0.0), Point3::new(10.0, 2.0, 0.0)]
    );
    assert_eq!(section(&mesh, 0.0).events().len(), 0);
}

#[test]
fn nonuniform_scale_pivot_and_translation_have_exact_analytic_section() {
    let mesh = crossing();
    let t = ObjectTransform::new(
        Vector3::new(5.0, -3.0, 7.0),
        Rotation3::identity(),
        Vector3::new(2.0, 3.0, 4.0),
        Point3::new(1.0, 2.0, 1.0),
    )
    .unwrap();
    let result = section_mesh_z(
        &mesh,
        4.0,
        SectionTransform::Object(&t),
        SectionLimits::default(),
    )
    .unwrap();
    assert_eq!(
        points(result.events()[0].intersection.event),
        vec![Point3::new(4.0, -1.0, 4.0), Point3::new(8.0, -7.0, 4.0)]
    );
}

#[test]
fn object_rotation_matches_independent_scalar_line_oracle() {
    let mesh = crossing();
    let angle = 0.7_f64;
    let t = ObjectTransform::new(
        Vector3::new(3.0, -2.0, 5.0),
        Rotation3::around_y_radians(angle).unwrap(),
        Vector3::new(2.0, 3.0, 0.5),
        Point3::new(1.0, -1.0, 2.0),
    )
    .unwrap();
    let result = section_mesh_z(
        &mesh,
        5.0,
        SectionTransform::Object(&t),
        SectionLimits::default(),
    )
    .unwrap();
    assert_eq!(result.events().len(), 1);
    let record = result.events()[0];
    let TrianglePlaneEvent::Segment { endpoints, .. } = record.intersection.event else {
        panic!("crossing required")
    };
    let scalar = |p: Point3| {
        let x = 2.0 * (p.x - 1.0);
        let y = 3.0 * (p.y + 1.0);
        let z = 0.5 * (p.z - 2.0);
        let (sin, cos) = angle.sin_cos();
        Point3::new(4.0 + cos * x + sin * z, -3.0 + y, 7.0 - sin * x + cos * z)
    };
    for e in endpoints {
        let EndpointOrigin::Edge(edge) = e.origin else {
            panic!("strict crossing required")
        };
        let [a, b] = record
            .source_edge(edge)
            .map(|v| scalar(mesh.vertices()[v as usize]));
        let w = (5.0 - a.z) / (b.z - a.z);
        near_point(
            e.position,
            Point3::new(a.x + w * (b.x - a.x), a.y + w * (b.y - a.y), 5.0),
            256.0 * f64::EPSILON * 32.0,
        );
    }
}

#[test]
fn affine_shear_and_composed_mapping_are_evaluated_without_decomposition() {
    let mesh = crossing();
    let t = Affine3::from_parts(
        [[2.0, 1.0, 0.0], [0.0, 3.0, 0.0], [0.0, 0.0, 1.0]],
        Vector3::new(1.0, 2.0, 3.0),
    )
    .unwrap();
    let result = section_mesh_z(
        &mesh,
        3.0,
        SectionTransform::Affine(&t),
        SectionLimits::default(),
    )
    .unwrap();
    assert_eq!(
        points(result.events()[0].intersection.event),
        vec![Point3::new(3.0, 8.0, 3.0), Point3::new(5.0, 2.0, 3.0)]
    );
    assert_kernel_scan(&mesh, 3.0, SectionTransform::Affine(&t));
}

#[test]
fn negative_scale_keeps_original_winding_and_vertex_identity() {
    let mesh = crossing();
    let t = Affine3::from_scale(Vector3::new(-2.0, 3.0, -1.0)).unwrap();
    let result = section_mesh_z(
        &mesh,
        0.0,
        SectionTransform::Affine(&t),
        SectionLimits::default(),
    )
    .unwrap();
    assert_eq!(
        points(result.events()[0].intersection.event),
        vec![Point3::new(-4.0, 0.0, 0.0), Point3::new(0.0, 6.0, 0.0)]
    );
    assert_eq!(result.events()[0].source_vertices, [0, 1, 2]);
    // One negative axis as well: negative determinant is not rejected or reoriented.
    let reflection = Affine3::from_scale(Vector3::new(-2.0, 3.0, 1.0)).unwrap();
    assert_kernel_scan(&mesh, 0.0, SectionTransform::Affine(&reflection));
}

#[test]
fn singular_transforms_report_post_transform_rank_loss_without_erasing_incidence() {
    let mesh = crossing();
    for scale in [
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(0.0, 1.0, 0.0),
        Vector3::new(0.0, 0.0, 0.0),
    ] {
        let t = Affine3::from_scale(scale).unwrap();
        let result = section_mesh_z(
            &mesh,
            0.0,
            SectionTransform::Affine(&t),
            SectionLimits::default(),
        )
        .unwrap();
        let record = result.events()[0];
        assert_eq!(record.source_vertices, [0, 1, 2]);
        assert_ne!(
            record.intersection.context.area_observation,
            TriangleAreaObservation::NonzeroComputedCross
        );
        if scale.z == 0.0 {
            assert!(matches!(
                record.intersection.event,
                TrianglePlaneEvent::CoplanarTriangle { .. }
            ));
        } else {
            assert!(matches!(
                record.intersection.event,
                TrianglePlaneEvent::Segment {
                    coincidence: EndpointCoincidence::CoincidentRepresented,
                    ..
                }
            ));
        }
    }
}

#[test]
fn object_mapping_is_not_silently_flattened_when_affine_conversion_overflows() {
    let pivot = Point3::new(f64::MAX, 0.0, 0.0);
    let mesh = one([pivot; 3]);
    let t = ObjectTransform::new(
        Vector3::new(0.0, 0.0, 0.0),
        Rotation3::identity(),
        Vector3::new(2.0, 1.0, 1.0),
        pivot,
    )
    .unwrap();
    assert!(t.to_affine().is_err());
    let result = section_mesh_z(
        &mesh,
        0.0,
        SectionTransform::Object(&t),
        SectionLimits::default(),
    )
    .unwrap();
    assert_eq!(kinds(&result), [0, 0, 0, 0, 1]);
    assert_eq!(
        points(result.events()[0].intersection.event),
        vec![pivot; 3]
    );
}

#[test]
fn unused_vertex_overflow_does_not_affect_face_only_traversal() {
    let mesh = TriangleMesh::try_new(
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(f64::MAX, 0.0, 0.0)],
        vec![[0, 0, 0]],
    )
    .unwrap();
    let t = Affine3::from_uniform_scale(2.0).unwrap();
    assert!(mesh.transformed_bounds(&t).is_err());
    assert_eq!(
        section_mesh_z(
            &mesh,
            0.0,
            SectionTransform::Affine(&t),
            SectionLimits::default()
        )
        .unwrap()
        .events()
        .len(),
        1
    );
}

#[test]
fn typed_transform_failure_has_first_face_corner_vertex_and_underlying_cause() {
    let mesh = TriangleMesh::try_new(
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(f64::MAX, 0.0, 0.0)],
        vec![[0, 0, 0], [0, 1, 0], [1, 0, 0]],
    )
    .unwrap();
    let t = Affine3::from_uniform_scale(2.0).unwrap();
    let error = section_mesh_z(
        &mesh,
        0.0,
        SectionTransform::Affine(&t),
        SectionLimits::default(),
    )
    .unwrap_err();
    assert!(matches!(
        error,
        SectionError::Transform {
            face: 1,
            corner: CornerId::Corner1,
            vertex: 1,
            source: TransformError::ArithmeticOverflow { .. }
        }
    ));
    assert!(error
        .source()
        .unwrap()
        .downcast_ref::<TransformError>()
        .is_some());
    assert!(error.to_string().contains("face 1"));
}

#[test]
fn typed_kernel_parameter_failure_is_not_wrapped_as_transform_or_mesh_error() {
    // Source extents remain finite, so this is a real accepted TriangleMesh, not a bypass.
    let mesh = one([
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(2.0, 0.0, f64::MAX),
        Point3::new(0.0, 2.0, f64::MAX),
    ]);
    let error = section_mesh_z(
        &mesh,
        f64::from_bits(1),
        SectionTransform::Identity,
        SectionLimits::default(),
    )
    .unwrap_err();
    assert!(matches!(
        error,
        SectionError::Intersection {
            face: 0,
            vertices: [0, 1, 2],
            source: TrianglePlaneError::UnrepresentableParameter { .. }
        }
    ));
    assert!(error
        .source()
        .unwrap()
        .downcast_ref::<TrianglePlaneError>()
        .is_some());
}

#[test]
fn world_distance_overflow_keeps_source_edge_resolution_and_area_context() {
    let mesh = one([
        Point3::new(0.0, 0.0, -1.0),
        Point3::new(1.0, 0.0, 1.0),
        Point3::new(0.0, 1.0, 1.0),
    ]);
    let t = Affine3::from_scale(Vector3::new(1.0, 1.0, f64::MAX)).unwrap();
    let error = section_mesh_z(
        &mesh,
        f64::MAX / 2.0,
        SectionTransform::Affine(&t),
        SectionLimits::default(),
    )
    .unwrap_err();
    assert!(matches!(
        error,
        SectionError::Intersection {
            face: 0,
            vertices: [0, 1, 2],
            source: TrianglePlaneError::DistanceOverflow { .. }
        }
    ));
    // At the balanced plane construction succeeds; world-area range failure remains context.
    let result = section_mesh_z(
        &mesh,
        0.0,
        SectionTransform::Affine(&t),
        SectionLimits::default(),
    )
    .unwrap();
    assert_eq!(result.summary().unavailable_area_faces, 1);
    assert_eq!(
        result.events()[0].intersection.context.area_observation,
        TriangleAreaObservation::UnavailableRange
    );
}

#[test]
fn nonfinite_planes_fail_even_on_empty_input_and_signed_zero_normalizes() {
    let empty = TriangleMesh::try_new(vec![], vec![]).unwrap();
    for z in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(
            section_mesh_z(
                &empty,
                z,
                SectionTransform::Identity,
                SectionLimits::default()
            ),
            Err(SectionError::NonFinitePlane)
        );
    }
    assert_eq!(section(&empty, -0.0).plane_z().to_bits(), 0);
    let mesh = one([
        Point3::new(-0.0, 0.0, -0.0),
        Point3::new(1.0, -0.0, -0.0),
        Point3::new(0.0, 1.0, 0.0),
    ]);
    let result = section(&mesh, -0.0);
    for p in points(result.events()[0].intersection.event) {
        for x in p.coordinates() {
            if x == 0.0 {
                assert_eq!(x.to_bits(), 0);
            }
        }
    }
}

#[test]
fn subnormal_and_adjacent_z_are_not_lost_to_a_traversal_prefilter() {
    let eta = f64::from_bits(1);
    let mesh = one([
        Point3::new(0.0, 0.0, eta),
        Point3::new(4.0, 0.0, 3.0 * eta),
        Point3::new(0.0, 4.0, 3.0 * eta),
    ]);
    let result = section(&mesh, 2.0 * eta);
    assert_eq!(
        points(result.events()[0].intersection.event),
        vec![
            Point3::new(0.0, 2.0, 2.0 * eta),
            Point3::new(2.0, 0.0, 2.0 * eta)
        ]
    );
    let mesh = one([
        Point3::new(0.0, 0.0, f64::from_bits(1.0_f64.to_bits() - 1)),
        Point3::new(4.0, 0.0, f64::from_bits(1.0_f64.to_bits() + 1)),
        Point3::new(0.0, 4.0, 1.0),
    ]);
    assert_eq!(
        section(&mesh, 1.0).events()[0].intersection.sides,
        [
            VertexPlaneSide::Below,
            VertexPlaneSide::Above,
            VertexPlaneSide::On
        ]
    );
}

#[test]
fn limits_are_independent_and_accept_exact_boundaries() {
    let mesh = crossing();
    let bytes = size_of::<FaceSection>();
    let exact = SectionLimits {
        max_faces: 1,
        max_events: 1,
        max_event_bytes: bytes,
    };
    assert_eq!(
        section_mesh_z(&mesh, 0.0, SectionTransform::Identity, exact)
            .unwrap()
            .events()
            .len(),
        1
    );
    for (limits, resource, limit) in [
        (
            SectionLimits {
                max_faces: 0,
                ..exact
            },
            SectionResource::Faces,
            0,
        ),
        (
            SectionLimits {
                max_events: 0,
                ..exact
            },
            SectionResource::Events,
            0,
        ),
        (
            SectionLimits {
                max_event_bytes: bytes - 1,
                ..exact
            },
            SectionResource::EventBytes,
            bytes - 1,
        ),
    ] {
        assert_eq!(
            section_mesh_z(&mesh, 0.0, SectionTransform::Identity, limits),
            Err(SectionError::LimitExceeded { resource, limit })
        );
    }
    // Huge configured budgets alone do not trigger speculative allocations or overflow.
    let huge = SectionLimits {
        max_faces: usize::MAX,
        max_events: usize::MAX,
        max_event_bytes: usize::MAX,
    };
    assert_bits_equal(
        &section(&mesh, 0.0),
        &section_mesh_z(&mesh, 0.0, SectionTransform::Identity, huge).unwrap(),
    );
}

fn repeated(count: usize, on: bool) -> TriangleMesh {
    let z = if on { 0.0 } else { 1.0 };
    TriangleMesh::try_new(vec![Point3::new(0.0, 0.0, z)], vec![[0, 0, 0]; count]).unwrap()
}

#[test]
fn limits_after_partial_work_return_only_error_and_preserve_source() {
    let mesh = repeated(17, true);
    let before = (
        mesh.vertices().to_vec(),
        mesh.triangles().to_vec(),
        mesh.source_bounds(),
    );
    for max in [1, 2, 3, 7, 16] {
        let limits = SectionLimits {
            max_events: max,
            ..SectionLimits::default()
        };
        assert!(matches!(
            section_mesh_z(&mesh, 0.0, SectionTransform::Identity, limits),
            Err(SectionError::LimitExceeded {
                resource: SectionResource::Events,
                ..
            })
        ));
        let limits = SectionLimits {
            max_event_bytes: max * size_of::<FaceSection>(),
            ..SectionLimits::default()
        };
        assert!(matches!(
            section_mesh_z(&mesh, 0.0, SectionTransform::Identity, limits),
            Err(SectionError::LimitExceeded {
                resource: SectionResource::EventBytes,
                ..
            })
        ));
    }
    assert_eq!(
        before,
        (
            mesh.vertices().to_vec(),
            mesh.triangles().to_vec(),
            mesh.source_bounds()
        )
    );
    assert_eq!(section(&mesh, 0.0).events().len(), 17);
}

#[test]
fn exact_event_byte_budget_survives_non_power_of_two_growth() {
    for count in [1, 2, 3, 5, 7, 9, 17, 257] {
        let mesh = repeated(count, true);
        let result = section_mesh_z(
            &mesh,
            0.0,
            SectionTransform::Identity,
            SectionLimits {
                max_faces: count,
                max_events: count,
                max_event_bytes: count * size_of::<FaceSection>(),
            },
        )
        .unwrap();
        assert_eq!(result.events().len(), count);
    }
}

fn cancelled_at(mesh: &TriangleMesh, point: Checkpoint) -> SectionError {
    let mut predicate = |p| p == point;
    section_mesh_z_with_cancellation(
        mesh,
        0.0,
        SectionTransform::Identity,
        SectionLimits::default(),
        &mut Cancellation::new(&mut predicate),
    )
    .unwrap_err()
}

#[test]
fn precancellation_wins_before_plane_validation_and_face_limits() {
    let mesh = crossing();
    let mut predicate = |_| true;
    assert_eq!(
        section_mesh_z_with_cancellation(
            &mesh,
            f64::NAN,
            SectionTransform::Identity,
            SectionLimits {
                max_faces: 0,
                ..SectionLimits::default()
            },
            &mut Cancellation::new(&mut predicate)
        ),
        Err(SectionError::Cancelled(Cancelled {
            checkpoint: Checkpoint::SectionStart
        }))
    );
}

#[test]
fn empty_and_no_hit_results_can_cancel_at_final_publication() {
    for mesh in [repeated(0, false), repeated(1, false)] {
        assert_eq!(
            cancelled_at(&mesh, Checkpoint::SectionComplete),
            SectionError::Cancelled(Cancelled {
                checkpoint: Checkpoint::SectionComplete
            })
        );
    }
}

#[test]
fn traversal_checkpoints_have_exact_documented_current_cadence_without_progress_claims() {
    let mesh = repeated(769, false);
    let mut trace = Vec::new();
    let mut predicate = |p| {
        trace.push(p);
        false
    };
    section_mesh_z_with_cancellation(
        &mesh,
        0.0,
        SectionTransform::Identity,
        SectionLimits {
            max_events: 0,
            max_event_bytes: 0,
            ..SectionLimits::default()
        },
        &mut Cancellation::new(&mut predicate),
    )
    .unwrap();
    assert_eq!(
        trace,
        vec![
            Checkpoint::SectionStart,
            Checkpoint::SectionFaces,
            Checkpoint::SectionFaces,
            Checkpoint::SectionFaces,
            Checkpoint::SectionFaces,
            Checkpoint::SectionComplete
        ]
    );
    // The third face poll is before face 512; no sleeps or scheduling assumption.
    let mut face_polls = 0;
    let mut predicate = |p| {
        if p == Checkpoint::SectionFaces {
            face_polls += 1;
        }
        p == Checkpoint::SectionFaces && face_polls == 3
    };
    assert_eq!(
        section_mesh_z_with_cancellation(
            &mesh,
            0.0,
            SectionTransform::Identity,
            SectionLimits::default(),
            &mut Cancellation::new(&mut predicate)
        ),
        Err(SectionError::Cancelled(Cancelled {
            checkpoint: Checkpoint::SectionFaces
        }))
    );
    assert_eq!(face_polls, 3);
}

#[test]
fn allocation_boundaries_can_cancel_before_or_after_reservation() {
    for wanted in [1, 2, 5, 6] {
        let mesh = repeated(10, true);
        let mut polls = 0;
        let mut predicate = |p| {
            if p == Checkpoint::SectionStorage {
                polls += 1;
            }
            p == Checkpoint::SectionStorage && polls == wanted
        };
        assert_eq!(
            section_mesh_z_with_cancellation(
                &mesh,
                0.0,
                SectionTransform::Identity,
                SectionLimits::default(),
                &mut Cancellation::new(&mut predicate)
            ),
            Err(SectionError::Cancelled(Cancelled {
                checkpoint: Checkpoint::SectionStorage
            }))
        );
        assert_eq!(polls, wanted);
        assert_eq!(section(&mesh, 0.0).events().len(), 10);
    }
}

#[test]
fn error_before_next_checkpoint_is_not_replaced_by_unobserved_cancellation() {
    let mesh = crossing();
    let mut trace = Vec::new();
    let mut predicate = |p| {
        trace.push(p);
        p == Checkpoint::SectionComplete
    };
    let error = section_mesh_z_with_cancellation(
        &mesh,
        0.0,
        SectionTransform::Identity,
        SectionLimits {
            max_events: 0,
            ..SectionLimits::default()
        },
        &mut Cancellation::new(&mut predicate),
    )
    .unwrap_err();
    assert!(matches!(
        error,
        SectionError::LimitExceeded {
            resource: SectionResource::Events,
            ..
        }
    ));
    assert_eq!(
        trace,
        vec![Checkpoint::SectionStart, Checkpoint::SectionFaces]
    );
}

#[test]
fn deterministic_repeats_and_never_cancelled_wrapper_preserve_complete_imported_source() {
    for id in ["FX-01", "FX-05", "FX-08", "FX-09"] {
        let imported = fixture(id, None);
        let snapshot = fixture(id, None);
        let t = ObjectTransform::new(
            Vector3::new(5.0, -2.0, 4.0),
            Rotation3::around_z_radians(0.5).unwrap(),
            Vector3::new(-1.0, 2.0, 0.5),
            Point3::new(1.0, -3.0, 2.0),
        )
        .unwrap();
        let t_before = t;
        let expected = section_mesh_z(
            &imported.mesh,
            10.0,
            SectionTransform::Object(&t),
            SectionLimits::default(),
        )
        .unwrap();
        for _ in 0..20 {
            let mut predicate = |_| false;
            let actual = section_mesh_z_with_cancellation(
                &imported.mesh,
                10.0,
                SectionTransform::Object(&t),
                SectionLimits::default(),
                &mut Cancellation::new(&mut predicate),
            )
            .unwrap();
            assert_bits_equal(&expected, &actual);
        }
        assert_eq!(
            cancelled_at(&imported.mesh, Checkpoint::SectionComplete),
            SectionError::Cancelled(Cancelled {
                checkpoint: Checkpoint::SectionComplete
            })
        );
        assert_eq!(imported, snapshot);
        assert_eq!(t, t_before);
    }
}
