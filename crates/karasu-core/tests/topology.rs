//! Independent small combinatorial examples; expected counts do not use the analyzer as an oracle.

use karasu_core::geometry::{ObjectTransform, Point3, Rotation3, Vector3};
use karasu_core::mesh::topology::{
    CheckStatus, EdgeKey, EdgeKind, TopologyError, TopologyIssue, TopologyLimits, TopologyReport,
    TopologyResource, VertexDefect, VertexKind,
};
use karasu_core::mesh::{Degeneracy, MeshDiagnostic, MeshError, TriangleMesh};
use std::error::Error;

fn mesh(points: &[[f64; 3]], faces: &[[u32; 3]]) -> TriangleMesh {
    TriangleMesh::try_new(
        points
            .iter()
            .map(|&[x, y, z]| Point3::new(x, y, z))
            .collect(),
        faces.to_vec(),
    )
    .unwrap()
}

fn tetra() -> TriangleMesh {
    mesh(
        &[[0., 0., 0.], [1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
        &[[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]],
    )
}

fn pair(same_direction: bool) -> TriangleMesh {
    mesh(
        &[[0., 0., 0.], [1., 0., 0.], [0., 1., 0.], [0., -1., 0.]],
        &[
            [0, 1, 2],
            if same_direction { [0, 1, 3] } else { [1, 0, 3] },
        ],
    )
}

fn counts(report: &TopologyReport, expected: [usize; 7]) {
    let s = report.summary();
    assert_eq!(
        [
            s.source_vertices,
            s.source_triangles,
            s.edges,
            s.boundary_edges,
            s.two_face_edges,
            s.non_manifold_edges,
            s.winding_conflicts
        ],
        expected
    );
    assert_eq!(
        s.edges,
        s.boundary_edges + s.two_face_edges + s.non_manifold_edges
    );
}

#[test]
fn canonical_edge_identity_rejects_collapsed_pairs() {
    assert_eq!(EdgeKey::new(8, 2).unwrap().vertices(), [2, 8]);
    assert_eq!(EdgeKey::new(8, 2), EdgeKey::new(2, 8));
    assert_eq!(EdgeKey::new(3, 3), None);
}

#[test]
fn single_triangle_has_three_boundary_edges_and_three_open_links() {
    let m = mesh(&[[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]], &[[0, 1, 2]]);
    let r = m.analyze_topology().unwrap();
    counts(&r, [3, 1, 3, 3, 0, 0, 0]);
    assert_eq!(r.summary().boundary_vertices, 3);
    assert_eq!(r.summary().edge_manifold, CheckStatus::Satisfied);
    assert_eq!(r.summary().vertex_manifold, CheckStatus::Satisfied);
    assert_eq!(r.summary().local_winding, CheckStatus::Satisfied);
    assert!(!r.summary().closed_combinatorial_surface);
    assert!(!r.summary().no_boundary_edges);
    assert_eq!(r.edge_components().triangles(0), Some([0].as_slice()));
    assert_eq!(r.edge_components().vertices(0), Some([0, 1, 2].as_slice()));
    assert_eq!(r.edge_components().components()[0].boundary_edges, 3);
}

#[test]
fn directed_incidence_retains_source_corner_order() {
    let m = mesh(&[[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]], &[[2, 0, 1]]);
    let r = m.analyze_topology().unwrap();
    let actual: Vec<_> = r
        .edges()
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let u = r.incidents(i).unwrap()[0];
            (e.key.vertices(), u.triangle, u.local_edge, u.from, u.to)
        })
        .collect();
    assert_eq!(
        actual,
        [
            ([0, 1], 0, 1, 0, 1),
            ([0, 2], 0, 0, 2, 0),
            ([1, 2], 0, 2, 1, 2)
        ]
    );
    assert_eq!(r.incident_faces(0), Some([0].as_slice()));
    assert_eq!(r.incidents(3), None);
    assert_eq!(r.incident_faces(3), None);
    assert_eq!(r.edge_components().triangles(1), None);
    assert_eq!(r.vertex_components().vertices(1), None);
}

#[test]
fn opposite_shared_edge_is_locally_consistent() {
    let r = pair(false).analyze_topology().unwrap();
    counts(&r, [4, 2, 5, 4, 1, 0, 0]);
    assert_eq!(r.summary().boundary_vertices, 4);
    assert_eq!(r.summary().local_winding, CheckStatus::Satisfied);
    let uses = r.incidents(0).unwrap();
    assert_eq!((uses[0].from, uses[0].to), (0, 1));
    assert_eq!((uses[1].from, uses[1].to), (1, 0));
    assert_eq!(r.edge_components().components()[0].two_face_edges, 1);
}

#[test]
fn same_direction_shared_edge_reports_exact_face_references() {
    let r = pair(true).analyze_topology().unwrap();
    counts(&r, [4, 2, 5, 4, 1, 0, 1]);
    assert_eq!(r.summary().local_winding, CheckStatus::Failed);
    assert_eq!(r.summary().vertex_manifold, CheckStatus::Satisfied);
    assert_eq!(
        r.issues()[0],
        TopologyIssue::WindingConflict {
            edge: 0,
            triangles: [0, 1]
        }
    );
    assert_eq!(r.edge_components().components()[0].winding_conflicts, 1);
}

#[test]
fn three_faces_on_one_edge_produce_branched_endpoint_links() {
    let m = mesh(
        &[
            [0., 0., 0.],
            [1., 0., 0.],
            [0., 1., 0.],
            [0., -1., 0.],
            [0., 0., 1.],
        ],
        &[[0, 1, 2], [1, 0, 3], [0, 1, 4]],
    );
    let r = m.analyze_topology().unwrap();
    counts(&r, [5, 3, 7, 6, 0, 1, 0]);
    assert_eq!(
        r.incidents(0)
            .unwrap()
            .iter()
            .map(|u| u.triangle)
            .collect::<Vec<_>>(),
        [0, 1, 2]
    );
    for v in &r.vertices()[..2] {
        assert_eq!(v.kind, VertexKind::NonManifold(VertexDefect::BranchedLink));
    }
    assert_eq!(r.summary().non_manifold_vertices, 2);
    assert_eq!(r.summary().boundary_vertices, 3);
    assert_eq!(r.summary().edge_manifold, CheckStatus::Failed);
    assert_eq!(r.summary().vertex_manifold, CheckStatus::Failed);
    assert_eq!(r.summary().local_winding, CheckStatus::Indeterminate);
    assert_eq!(r.edge_components().components().len(), 1);
    assert_eq!(r.edge_components().components()[0].non_manifold_edges, 1);
}

#[test]
fn tetrahedron_is_a_closed_combinatorial_surface() {
    let r = tetra().analyze_topology().unwrap();
    counts(&r, [4, 4, 6, 0, 6, 0, 0]);
    assert_eq!(r.summary().interior_vertices, 4);
    assert!(r.summary().closed_combinatorial_surface);
    assert!(r.summary().no_boundary_edges);
    assert_eq!(r.summary().local_winding, CheckStatus::Satisfied);
    assert_eq!(r.incident_faces(0), Some([0, 1, 2].as_slice()));
}

#[test]
fn globally_reversing_a_surface_does_not_establish_outwardness() {
    let m = tetra();
    let reversed = TriangleMesh::try_new(
        m.vertices().to_vec(),
        m.triangles().iter().map(|&[a, b, c]| [a, c, b]).collect(),
    )
    .unwrap();
    let a = m.analyze_topology().unwrap();
    let b = reversed.analyze_topology().unwrap();
    assert_eq!(a.summary(), b.summary());
    assert!(b.summary().closed_combinatorial_surface);
}

#[test]
fn closedness_and_supplied_local_winding_are_separate_findings() {
    let m = tetra();
    let mut faces = m.triangles().to_vec();
    faces[0].swap(1, 2);
    let r = TriangleMesh::try_new(m.vertices().to_vec(), faces)
        .unwrap()
        .analyze_topology()
        .unwrap();
    assert_eq!(r.summary().winding_conflicts, 3);
    assert_eq!(r.summary().local_winding, CheckStatus::Failed);
    assert!(r.summary().closed_combinatorial_surface);
}

#[test]
fn disconnected_closed_components_have_stable_members_and_summaries() {
    let m = tetra();
    let mut points = m.vertices().to_vec();
    points.extend(m.vertices().iter().map(|p| Point3::new(p.x + 3., p.y, p.z)));
    // Interleave faces so component IDs cannot just be contiguous source face ranges.
    let faces = m
        .triangles()
        .iter()
        .flat_map(|face| [*face, face.map(|v| v + 4)])
        .collect();
    let r = TriangleMesh::try_new(points, faces)
        .unwrap()
        .analyze_topology()
        .unwrap();
    counts(&r, [8, 8, 12, 0, 12, 0, 0]);
    assert!(r.summary().closed_combinatorial_surface);
    for partition in [r.edge_components(), r.vertex_components()] {
        assert_eq!(partition.components().len(), 2);
        assert_eq!(partition.triangles(0), Some([0, 2, 4, 6].as_slice()));
        assert_eq!(partition.triangles(1), Some([1, 3, 5, 7].as_slice()));
        assert_eq!(partition.vertices(0), Some([0, 1, 2, 3].as_slice()));
        assert_eq!(partition.vertices(1), Some([4, 5, 6, 7].as_slice()));
        for c in partition.components() {
            assert_eq!(
                (
                    c.triangle_count,
                    c.vertex_count,
                    c.edge_count,
                    c.boundary_edges,
                    c.non_manifold_edges
                ),
                (4, 4, 6, 0, 0)
            );
        }
    }
}

#[test]
fn closed_shells_touching_at_one_vertex_fail_vertex_manifoldness() {
    let m = mesh(
        &[
            [0., 0., 0.],
            [1., 0., 0.],
            [0., 1., 0.],
            [0., 0., 1.],
            [-1., 0., 0.],
            [0., -1., 0.],
            [0., 0., -1.],
        ],
        &[
            [0, 2, 1],
            [0, 1, 3],
            [0, 3, 2],
            [1, 2, 3],
            [0, 5, 4],
            [0, 4, 6],
            [0, 6, 5],
            [4, 5, 6],
        ],
    );
    let r = m.analyze_topology().unwrap();
    counts(&r, [7, 8, 12, 0, 12, 0, 0]);
    assert_eq!(
        (r.summary().edge_components, r.summary().vertex_components),
        (2, 1)
    );
    assert_eq!(
        r.vertices()[0].kind,
        VertexKind::NonManifold(VertexDefect::DisconnectedFans)
    );
    assert_eq!(r.vertices()[0].usable_link_components, 2);
    assert_eq!(r.summary().interior_vertices, 6);
    assert_eq!(r.summary().edge_manifold, CheckStatus::Satisfied);
    assert_eq!(r.summary().local_winding, CheckStatus::Satisfied);
    assert_eq!(r.summary().vertex_manifold, CheckStatus::Failed);
    assert!(!r.summary().closed_combinatorial_surface);
    assert_eq!(
        r.edge_components().vertices(0),
        Some([0, 1, 2, 3].as_slice())
    );
    assert_eq!(
        r.edge_components().vertices(1),
        Some([0, 4, 5, 6].as_slice())
    );
    assert_eq!(r.vertex_components().components()[0].vertex_count, 7);
}

#[test]
fn bow_tie_open_fans_are_not_a_single_boundary_vertex() {
    let m = mesh(
        &[
            [0., 0., 0.],
            [1., 0., 0.],
            [0., 1., 0.],
            [-1., 0., 0.],
            [0., -1., 0.],
        ],
        &[[0, 1, 2], [0, 3, 4]],
    );
    let r = m.analyze_topology().unwrap();
    counts(&r, [5, 2, 6, 6, 0, 0, 0]);
    assert_eq!(
        (r.summary().edge_components, r.summary().vertex_components),
        (2, 1)
    );
    assert_eq!(
        r.vertices()[0].kind,
        VertexKind::NonManifold(VertexDefect::DisconnectedFans)
    );
    assert_eq!(r.vertices()[0].usable_link_components, 2);
    assert_eq!(r.summary().boundary_vertices, 4);
}

#[test]
fn duplicate_faces_retain_incidence_and_fail_simple_link_condition() {
    for (second, conflicts) in [([0, 1, 2], 3), ([2, 1, 0], 0)] {
        let m = mesh(
            &[[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]],
            &[[0, 1, 2], second],
        );
        let r = m.analyze_topology().unwrap();
        counts(&r, [3, 2, 3, 0, 3, 0, conflicts]);
        assert_eq!(r.summary().duplicate_triangles, 1);
        assert_eq!(
            r.geometry_diagnostics(),
            [MeshDiagnostic::DuplicateTriangle {
                triangle: 1,
                first: 0
            }]
        );
        assert_eq!(r.summary().non_manifold_vertices, 3);
        for v in r.vertices() {
            assert_eq!(
                v.kind,
                VertexKind::NonManifold(VertexDefect::DuplicateLinkEdge)
            );
        }
        assert!(!r.summary().closed_combinatorial_surface);
        assert_eq!(r.summary().usable_triangles, 2);
    }
}

#[test]
fn repeated_indices_are_excluded_and_incident_faces_are_unique() {
    let m = mesh(&[[0., 0., 0.], [1., 0., 0.]], &[[0, 0, 1]]);
    let r = m.analyze_topology().unwrap();
    counts(&r, [2, 1, 0, 0, 0, 0, 0]);
    assert_eq!(r.summary().excluded_triangles, 1);
    assert_eq!(r.summary().indeterminate_vertices, 2);
    assert_eq!(r.incident_faces(0), Some([0].as_slice()));
    assert_eq!(r.incident_faces(1), Some([0].as_slice()));
    assert_eq!(
        r.issues()[0],
        TopologyIssue::ExcludedTriangle {
            triangle: 0,
            reason: Degeneracy::RepeatedVertex
        }
    );
    assert_eq!(r.summary().edge_manifold, CheckStatus::Indeterminate);
    assert_eq!(r.summary().vertex_manifold, CheckStatus::Indeterminate);
    assert_eq!(r.summary().local_winding, CheckStatus::Indeterminate);
    assert_eq!(r.summary().edge_components, 0);
    assert_eq!(r.summary().vertex_components, 0);
    assert_eq!(m.triangles(), [[0, 0, 1]]);
}

#[test]
fn fully_collapsed_face_is_still_one_source_incidence() {
    let m = mesh(&[[0., 0., 0.]], &[[0, 0, 0]]);
    let r = m.analyze_topology().unwrap();
    assert_eq!(r.incident_faces(0), Some([0].as_slice()));
    assert_eq!(r.vertices()[0].excluded_faces, 1);
    assert_eq!(r.vertices()[0].usable_faces, 0);
    assert_eq!(r.vertices()[0].kind, VertexKind::Indeterminate);
    assert_eq!(r.summary().edges, 0);
}

#[test]
fn distinct_indices_at_repeated_positions_are_also_geometrically_excluded() {
    let m = mesh(&[[0., 0., 0.], [0., 0., 0.], [0., 1., 0.]], &[[0, 1, 2]]);
    let r = m.analyze_topology().unwrap();
    assert_eq!(r.summary().edges, 0);
    assert_eq!(r.summary().indeterminate_vertices, 3);
    assert_eq!(
        r.geometry_diagnostics(),
        [MeshDiagnostic::DegenerateTriangle {
            triangle: 0,
            kind: Degeneracy::RepeatedVertex
        }]
    );
    assert_eq!(m.vertices().len(), 3);
}

#[test]
fn collinear_faces_are_excluded_without_an_exact_predicate_claim() {
    let m = mesh(&[[0., 0., 0.], [1., 0., 0.], [2., 0., 0.]], &[[0, 1, 2]]);
    let r = m.analyze_topology().unwrap();
    assert_eq!(r.summary().edges, 0);
    assert_eq!(r.summary().indeterminate_vertices, 3);
    assert_eq!(
        r.geometry_diagnostics(),
        [MeshDiagnostic::DegenerateTriangle {
            triangle: 0,
            kind: Degeneracy::ZeroComputedArea
        }]
    );
}

#[test]
fn computed_zero_area_is_indeterminate_even_for_mathematically_distinct_corners() {
    // Independent real-arithmetic cross product is nonzero, but normalized f64 directions lose
    // the 1e-600 components. This is deliberately not interpreted as proven exact collinearity.
    let m = mesh(
        &[[0., 0., 0.], [1e300, 1e-300, 0.], [1e300, 0., 1e-300]],
        &[[0, 1, 2]],
    );
    let r = m.analyze_topology().unwrap();
    assert_eq!(r.summary().excluded_triangles, 1);
    assert_eq!(r.summary().indeterminate_vertices, 3);
    assert_eq!(r.summary().vertex_manifold, CheckStatus::Indeterminate);
    assert_eq!(r.summary().edge_manifold, CheckStatus::Indeterminate);
    assert!(!r.summary().closed_combinatorial_surface);
}

#[test]
fn excluded_face_cannot_leave_a_false_closed_surface_claim() {
    let m = tetra();
    let mut faces = m.triangles().to_vec();
    faces.push([0, 0, 1]);
    let r = TriangleMesh::try_new(m.vertices().to_vec(), faces)
        .unwrap()
        .analyze_topology()
        .unwrap();
    assert_eq!(r.summary().interior_vertices, 2);
    assert_eq!(r.summary().indeterminate_vertices, 2);
    assert_eq!(r.summary().edge_manifold, CheckStatus::Indeterminate);
    assert_eq!(r.summary().vertex_manifold, CheckStatus::Indeterminate);
    assert!(r.summary().no_boundary_edges);
    assert!(!r.summary().closed_combinatorial_surface);
    assert_eq!(
        r.edge_components().triangles(0),
        Some([0, 1, 2, 3].as_slice())
    );
}

#[test]
fn manual_coincident_but_distinct_indices_do_not_create_connectivity() {
    for shift in [0., f64::EPSILON] {
        let m = mesh(
            &[
                [0., 0., 0.],
                [1., 0., 0.],
                [0., 1., 0.],
                [0., 0., shift],
                [1., 0., shift],
                [0., 1., shift],
            ],
            &[[0, 1, 2], [3, 4, 5]],
        );
        let r = m.analyze_topology().unwrap();
        counts(&r, [6, 2, 6, 6, 0, 0, 0]);
        assert_eq!(
            (r.summary().edge_components, r.summary().vertex_components),
            (2, 2)
        );
        assert_eq!(r.summary().boundary_vertices, 6);
        assert_eq!(r.summary().duplicate_triangles, usize::from(shift == 0.));
    }
}

#[test]
fn empty_mesh_has_no_applicable_surface_checks() {
    let r = mesh(&[], &[]).analyze_topology().unwrap();
    counts(&r, [0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(r.summary().edge_manifold, CheckStatus::NotApplicable);
    assert_eq!(r.summary().vertex_manifold, CheckStatus::NotApplicable);
    assert_eq!(r.summary().local_winding, CheckStatus::NotApplicable);
    assert!(r.summary().no_boundary_edges);
    assert!(!r.summary().closed_combinatorial_surface);
    assert_eq!(r.issues(), []);
    assert_eq!(r.edge_components().components(), []);
}

#[test]
fn isolated_vertices_are_explicit_and_prevent_whole_mesh_manifold_claims() {
    let m = tetra();
    let mut points = m.vertices().to_vec();
    points.push(Point3::new(10., 10., 10.));
    let r = TriangleMesh::try_new(points, m.triangles().to_vec())
        .unwrap()
        .analyze_topology()
        .unwrap();
    assert_eq!(r.summary().isolated_vertices, 1);
    assert_eq!(r.vertices()[4].kind, VertexKind::Isolated);
    assert_eq!(r.incident_faces(4), Some([].as_slice()));
    assert_eq!(r.summary().vertex_manifold, CheckStatus::Failed);
    assert!(!r.summary().closed_combinatorial_surface);
    assert_eq!(
        r.edge_components().vertices(0),
        Some([0, 1, 2, 3].as_slice())
    );
    assert_eq!(
        r.issues().last(),
        Some(&TopologyIssue::IsolatedVertex { vertex: 4 })
    );
    let only = mesh(&[[0., 0., 0.]], &[]).analyze_topology().unwrap();
    assert_eq!(only.summary().vertex_manifold, CheckStatus::Failed);
    assert_eq!(only.summary().edge_manifold, CheckStatus::NotApplicable);
}

#[test]
fn deterministic_order_includes_edges_components_and_issue_references() {
    let m = pair(true);
    let expected = m.analyze_topology().unwrap();
    assert_eq!(
        expected
            .edges()
            .iter()
            .map(|e| e.key.vertices())
            .collect::<Vec<_>>(),
        [[0, 1], [0, 2], [0, 3], [1, 2], [1, 3]]
    );
    assert_eq!(
        expected.issues(),
        [
            TopologyIssue::WindingConflict {
                edge: 0,
                triangles: [0, 1]
            },
            TopologyIssue::BoundaryEdge { edge: 1 },
            TopologyIssue::BoundaryEdge { edge: 2 },
            TopologyIssue::BoundaryEdge { edge: 3 },
            TopologyIssue::BoundaryEdge { edge: 4 }
        ]
    );
    for _ in 0..32 {
        assert_eq!(m.analyze_topology().unwrap(), expected);
    }
}

#[test]
fn source_topology_is_unchanged_by_invertible_scene_operations() {
    let m = tetra();
    let vertices = m.vertices().to_vec();
    let triangles = m.triangles().to_vec();
    let bounds = m.source_bounds();
    let diagnostics = m.diagnostics().unwrap();
    let before = m.analyze_topology().unwrap();
    for scale in [Vector3::new(2., 3., 4.), Vector3::new(-1., 1., 1.)] {
        let object = ObjectTransform::new(
            Vector3::new(15., 10., 5.),
            Rotation3::from_euler_xyz_radians(Vector3::new(0.3, -0.7, 1.2)).unwrap(),
            scale,
            Point3::new(0.25, 0.5, -0.5),
        )
        .unwrap();
        let saved = object;
        m.world_bounds(&object).unwrap();
        m.transformed_bounds(&object.to_affine().unwrap()).unwrap();
        assert_eq!(m.analyze_topology().unwrap(), before);
        assert_eq!(object, saved);
    }
    assert_eq!(m.vertices(), vertices);
    assert_eq!(m.triangles(), triangles);
    assert_eq!(m.source_bounds(), bounds);
    assert_eq!(m.diagnostics().unwrap(), diagnostics);
}

#[test]
fn invalid_indices_are_rejected_at_the_mesh_boundary() {
    assert_eq!(
        TriangleMesh::try_new(vec![Point3::new(0., 0., 0.)], vec![[0, 1, 0]]),
        Err(MeshError::InvalidIndex {
            triangle: 0,
            corner: 1,
            index: 1,
            vertex_count: 1
        })
    );
}

#[test]
fn resource_caps_include_excluded_faces_and_unused_vertices() {
    let m = mesh(&[[0., 0., 0.], [1., 0., 0.]], &[[0, 0, 0]]);
    assert_eq!(
        m.analyze_topology_with_limits(TopologyLimits {
            max_vertices: 1,
            max_triangles: 1
        }),
        Err(TopologyError::LimitExceeded {
            resource: TopologyResource::Vertices,
            limit: 1
        })
    );
    assert_eq!(
        m.analyze_topology_with_limits(TopologyLimits {
            max_vertices: 2,
            max_triangles: 0
        }),
        Err(TopologyError::LimitExceeded {
            resource: TopologyResource::Triangles,
            limit: 0
        })
    );
    assert!(m
        .analyze_topology_with_limits(TopologyLimits {
            max_vertices: 2,
            max_triangles: 1
        })
        .is_ok());
    assert!(mesh(&[], &[])
        .analyze_topology_with_limits(TopologyLimits {
            max_vertices: 0,
            max_triangles: 0
        })
        .is_ok());
}

#[test]
fn structured_error_display_and_underlying_cause_are_available() {
    let cause = MeshError::Allocation {
        context: "mesh topology buffers",
    };
    let error = TopologyError::from(cause);
    assert!(error.to_string().contains("topology"));
    assert_eq!(
        error.source().unwrap().to_string(),
        "unable to reserve memory for mesh topology buffers"
    );
    assert!(TopologyError::SizeOverflow.source().is_none());
    assert!(TopologyError::SizeOverflow
        .to_string()
        .contains("platform size"));
    let limit = TopologyError::LimitExceeded {
        resource: TopologyResource::Triangles,
        limit: 0,
    };
    assert!(limit.to_string().contains("Triangles limit exceeded: 0"));
}

#[test]
fn high_degree_vertex_uses_iterative_links_and_component_traversal() {
    let n = 4096_u32;
    let mut points = vec![Point3::new(0., 0., 0.)];
    for i in 0..n {
        let angle = f64::from(i) * std::f64::consts::TAU / f64::from(n);
        points.push(Point3::new(angle.cos(), angle.sin(), 0.));
    }
    let faces = (0..n).map(|i| [0, i + 1, (i + 1) % n + 1]).collect();
    let r = TriangleMesh::try_new(points, faces)
        .unwrap()
        .analyze_topology()
        .unwrap();
    assert_eq!(r.vertices()[0].kind, VertexKind::InteriorManifold);
    assert_eq!(r.summary().boundary_vertices, n as usize);
    assert_eq!(r.summary().boundary_edges, n as usize);
    assert_eq!(r.summary().edges, 2 * n as usize);
    assert_eq!(r.summary().edge_components, 1);
    assert_eq!(r.summary().non_manifold_vertices, 0);
}

#[test]
fn adversarial_high_incidence_edge_does_not_compare_all_face_pairs() {
    let n = 4096_u32;
    let mut points = vec![Point3::new(0., 0., 0.), Point3::new(1., 0., 0.)];
    points.extend((0..n).map(|i| Point3::new(0., 1., f64::from(i))));
    let faces = (0..n).map(|i| [0, 1, i + 2]).collect();
    let r = TriangleMesh::try_new(points, faces)
        .unwrap()
        .analyze_topology()
        .unwrap();
    assert_eq!(r.summary().edges, 2 * n as usize + 1);
    assert_eq!(r.summary().boundary_edges, 2 * n as usize);
    assert_eq!(r.summary().non_manifold_edges, 1);
    assert_eq!(r.summary().non_manifold_vertices, 2);
    assert_eq!(r.incidents(0).unwrap().len(), n as usize);
    assert_eq!(r.edges()[0].kind, EdgeKind::NonManifold);
    assert_eq!(r.summary().edge_components, 1);
    assert_eq!(r.summary().vertex_components, 1);
}

/// The lean edge summary equals the full report's edge fields on every small corpus fixture
/// (including the open, flipped, degenerate and non-manifold defects) and on random meshes.
#[test]
fn edge_summary_matches_the_full_report() {
    use karasu_core::{
        cancellation::Cancellation,
        geometry::Point3,
        mesh::{
            stl::{import_stl_path, StlOptions, StlUnitPolicy},
            topology::TopologyLimits,
            TriangleMesh,
        },
    };
    let check = |mesh: &TriangleMesh, name: &str| {
        let full = *mesh.analyze_topology().unwrap().summary();
        let lean = mesh
            .edge_summary_with_cancellation(TopologyLimits::default(), &mut Cancellation::never())
            .unwrap();
        assert_eq!(
            [
                lean.usable_triangles,
                lean.excluded_triangles,
                lean.edges,
                lean.boundary_edges,
                lean.two_face_edges,
                lean.non_manifold_edges,
                lean.winding_conflicts,
            ],
            [
                full.usable_triangles,
                full.excluded_triangles,
                full.edges,
                full.boundary_edges,
                full.two_face_edges,
                full.non_manifold_edges,
                full.winding_conflicts,
            ],
            "{name}"
        );
    };
    let root =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/reference/stl");
    let mut checked = 0;
    for entry in std::fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        // FX-11 (generated) and FX-12 (owner-supplied) are large local-only fixtures.
        let stl = path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("stl"));
        if !stl || name.starts_with("fx-11") || name.starts_with("fx-12") {
            continue;
        }
        let mesh = import_stl_path(&path, StlOptions::new(StlUnitPolicy::AssumeMillimeters))
            .unwrap()
            .mesh;
        check(&mesh, &name);
        checked += 1;
    }
    assert_eq!(checked, 11);
    let mut state = 0x9e37_79b9_7f4a_7c15_u64;
    let mut next = |m: u32| {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        u32::try_from(state % u64::from(m)).unwrap()
    };
    for case in 0..300 {
        // Few vertices on a small grid: shared, repeated, flipped and collinear faces abound.
        let points: Vec<Point3> = (0..=2 + next(12))
            .map(|_| Point3::new(f64::from(next(3)), f64::from(next(3)), f64::from(next(2))))
            .collect();
        let count = u32::try_from(points.len()).unwrap();
        let triangles = (0..=next(30))
            .map(|_| [next(count), next(count), next(count)])
            .collect();
        check(
            &TriangleMesh::try_new(points, triangles).unwrap(),
            &format!("random {case}"),
        );
    }
}
