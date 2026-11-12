//! Independent analytic graph structure and incidence provenance tests; no path extraction.

use karasu_core::cancellation::{Cancellation, Cancelled, Checkpoint};
use karasu_core::geometry::{Affine3, Point3, Vector3};
use karasu_core::mesh::{
    stl::{import_stl_path, StlOptions, StlUnitPolicy},
    TriangleMesh,
};
use karasu_core::slicing::graph::*;
use karasu_core::slicing::normalize::{
    normalize_section, CoordinateAgreement, NormalizationLimits, NormalizedSection, Ownership,
    SourceFeature,
};
use karasu_core::slicing::{section_mesh_z, MeshSection, SectionLimits, SectionTransform};
use std::{error::Error, path::PathBuf};

fn mesh(vertices: &[[f64; 3]], faces: Vec<[u32; 3]>) -> TriangleMesh {
    TriangleMesh::try_new(
        vertices
            .iter()
            .map(|&[x, y, z]| Point3::new(x, y, z))
            .collect(),
        faces,
    )
    .unwrap()
}
fn raw(mesh: &TriangleMesh) -> MeshSection {
    section_mesh_z(
        mesh,
        0.0,
        SectionTransform::Identity,
        SectionLimits::default(),
    )
    .unwrap()
}
fn norm(raw: &MeshSection) -> NormalizedSection<'_> {
    normalize_section(raw, NormalizationLimits::default()).unwrap()
}
fn graph<'n, 'r>(input: &'n NormalizedSection<'r>) -> IncidenceGraph<'n, 'r> {
    build_incidence_graph(input, GraphLimits::default()).unwrap()
}
fn planar_square() -> TriangleMesh {
    mesh(
        &[[0., 0., 0.], [4., 0., 0.], [4., 4., 0.], [0., 4., 0.]],
        vec![[0, 1, 2], [0, 2, 3]],
    )
}
// Each desired graph node is a vertical source edge. A triangle using two lower endpoints and
// the upper endpoint of one edge produces a segment joining their midpoint/diagonal intersections.
// For exact On-vertex branches, share source vertex 0; each face has a distinct opposite edge.
fn star(arms: usize) -> TriangleMesh {
    let mut points = vec![Point3::new(0., 0., 0.)];
    let mut faces = vec![];
    for i in 0..arms {
        let index = u32::try_from(points.len()).unwrap();
        let x = f64::from(u32::try_from(i + 1).unwrap());
        points.push(Point3::new(x, 2., -1.));
        points.push(Point3::new(x, 2., 1.));
        faces.push([0, index, index + 1]);
    }
    TriangleMesh::try_new(points, faces).unwrap()
}
// A vertical triangulated ribbon follows a supplied polyline. Its midplane graph has one node
// on each vertical source edge and one on each panel diagonal; no coordinate-based joining.
fn ribbon(points: &[[f64; 2]], links: &[[u32; 2]]) -> TriangleMesh {
    let vertices = points
        .iter()
        .flat_map(|&[x, y]| [Point3::new(x, y, -1.), Point3::new(x, y, 1.)])
        .collect();
    let faces = links
        .iter()
        .flat_map(|&[a, b]| [[2 * a, 2 * b, 2 * b + 1], [2 * a, 2 * b + 1, 2 * a + 1]])
        .collect();
    TriangleMesh::try_new(vertices, faces).unwrap()
}
fn square_ribbon() -> TriangleMesh {
    ribbon(
        &[[0., 0.], [4., 0.], [4., 4.], [0., 4.]],
        &[[0, 1], [1, 2], [2, 3], [3, 0]],
    )
}
fn fixture(name: &str) -> TriangleMesh {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/reference/stl")
        .join(name);
    import_stl_path(path, StlOptions::new(StlUnitPolicy::AssumeMillimeters))
        .unwrap()
        .mesh
}

fn invariants(g: &IncidenceGraph<'_, '_>) {
    let n = g.normalized();
    assert_eq!(g.vertices().len(), n.features().len());
    assert_eq!(g.edges().len(), n.groups().len());
    let mut complete = 0;
    let mut eligible = 0;
    let mut raw_ends = 0;
    for (id, v) in g.vertices().iter().enumerate() {
        assert_eq!(v.feature_group, id);
        let all = g.complete_adjacency(id).unwrap();
        let candidates = g.candidate_adjacency(id).unwrap();
        assert!(all
            .windows(2)
            .all(|p| (p[0].edge, p[0].endpoint) < (p[1].edge, p[1].endpoint)));
        assert!(candidates
            .windows(2)
            .all(|p| (p[0].edge, p[0].endpoint) < (p[1].edge, p[1].endpoint)));
        let mut distinct = all.iter().map(|a| a.edge).collect::<Vec<_>>();
        distinct.dedup();
        assert_eq!(v.degrees.unique_edges, distinct.len());
        assert_eq!(v.degrees.eligible_incidence_degree, candidates.len());
        assert_eq!(
            v.degrees.unresolved_edges,
            distinct
                .iter()
                .filter(|&&e| g.edges()[e].eligibility == EdgeEligibility::Unresolved)
                .count()
        );
        assert_eq!(
            v.degrees.raw_occurrence_ends,
            all.iter()
                .map(|a| g.edges()[a.edge].multiplicity)
                .sum::<usize>()
        );
        for a in all {
            let edge = g.edges()[a.edge];
            assert_eq!(a.vertex, id);
            assert_eq!(edge.endpoints[a.endpoint], id);
            let other = edge.endpoints[1 - a.endpoint];
            assert!(g
                .complete_adjacency(other)
                .unwrap()
                .contains(&AdjacentIncidence {
                    vertex: other,
                    edge: a.edge,
                    endpoint: 1 - a.endpoint
                }));
        }
        for a in candidates {
            assert!(all.contains(a));
            assert_eq!(g.edges()[a.edge].eligibility, EdgeEligibility::Candidate);
            assert!(matches!(
                n.features()[id].coordinates,
                CoordinateAgreement::Agreed(_)
            ));
        }
        complete += all.len();
        eligible += candidates.len();
        raw_ends += v.degrees.raw_occurrence_ends;
    }
    let mut occurrences = vec![];
    for (id, edge) in g.edges().iter().enumerate() {
        assert_eq!(edge.group, id);
        assert_eq!(
            edge.endpoints.map(|v| n.features()[v].feature),
            n.groups()[id].endpoints
        );
        let refs = g.occurrences(id).unwrap();
        assert_eq!(edge.multiplicity, refs.len());
        assert_eq!(refs, &n.segment_order()[n.groups()[id].occurrences.clone()]);
        for &occurrence in refs {
            let segment = n.segments()[occurrence];
            assert_eq!(
                segment.endpoints.map(|i| n.observations()[i].feature),
                n.groups()[id].endpoints
            );
            assert!(segment.event < n.raw().events().len());
        }
        occurrences.extend_from_slice(refs);
        assert_eq!(
            g.vertices()
                .iter()
                .enumerate()
                .flat_map(|(v, _)| g.complete_adjacency(v).unwrap())
                .filter(|a| a.edge == id)
                .count(),
            2
        );
    }
    occurrences.sort_unstable();
    assert_eq!(occurrences, (0..n.segments().len()).collect::<Vec<_>>());
    assert_eq!(complete, 2 * g.edges().len());
    assert_eq!(raw_ends, 2 * n.segments().len());
    assert_eq!(eligible, g.summary().candidate_adjacency_entries);
    assert_eq!(g.summary().diagnostics, g.diagnostics().len());
    assert_eq!(g.complete_adjacency(usize::MAX), None);
    assert_eq!(g.candidate_adjacency(usize::MAX), None);
    assert_eq!(g.occurrences(usize::MAX), None);
}

#[test]
fn empty_graph_accepts_zero_caps_and_preserves_borrow() {
    let m = mesh(&[], vec![]);
    let r = raw(&m);
    let n = norm(&r);
    let limits = GraphLimits {
        max_vertices: 0,
        max_edges: 0,
        max_incidence_references: 0,
        max_adjacency_entries: 0,
        max_diagnostics: 0,
        max_payload_bytes: 0,
    };
    let g = build_incidence_graph(&n, limits).unwrap();
    assert_eq!(g.summary(), GraphSummary::default());
    assert!(std::ptr::eq(g.normalized(), std::ptr::from_ref(&n)));
    invariants(&g);
}
#[test]
fn single_proper_segment_has_two_potential_endpoints_and_exact_provenance() {
    let m = mesh(
        &[[0., 0., -1.], [4., 0., 1.], [0., 4., 1.]],
        vec![[0, 1, 2]],
    );
    let r = raw(&m);
    let n = norm(&r);
    let g = graph(&n);
    assert_eq!(
        (
            g.summary().vertices,
            g.summary().edges,
            g.summary().candidate_edges
        ),
        (2, 1, 1)
    );
    assert_eq!(
        n.features().iter().map(|v| v.feature).collect::<Vec<_>>(),
        vec![SourceFeature::Edge([0, 1]), SourceFeature::Edge([0, 2])]
    );
    assert_eq!(g.occurrences(0), Some([0].as_slice()));
    assert_eq!(
        g.diagnostics(),
        [
            GraphDiagnostic::PotentialEndpoint { vertex: 0 },
            GraphDiagnostic::PotentialEndpoint { vertex: 1 }
        ]
    );
    assert!(g.vertices().iter().all(|v| v.degrees
        == VertexDegrees {
            unique_edges: 1,
            eligible_edges: 1,
            eligible_incidence_degree: 1,
            raw_occurrence_ends: 1,
            unresolved_edges: 0
        }));
    invariants(&g);
}
#[test]
fn analytic_four_edge_square_excludes_only_retained_internal_diagonal() {
    let m = planar_square();
    let r = raw(&m);
    let n = norm(&r);
    let g = graph(&n);
    assert_eq!(
        (
            g.summary().vertices,
            g.summary().edges,
            g.summary().candidate_edges,
            g.summary().internal_edges,
            g.summary().occurrences
        ),
        (4, 5, 4, 1, 6)
    );
    let keys = g
        .edges()
        .iter()
        .filter(|e| e.eligibility == EdgeEligibility::Candidate)
        .map(|e| e.endpoints)
        .collect::<Vec<_>>();
    assert_eq!(keys, vec![[0, 1], [0, 3], [1, 2], [2, 3]]);
    let internal = g
        .edges()
        .iter()
        .find(|e| e.eligibility == EdgeEligibility::InternalCoplanar)
        .unwrap();
    assert_eq!(internal.endpoints, [0, 2]);
    assert_eq!(internal.multiplicity, 2);
    assert!(g
        .vertices()
        .iter()
        .all(|v| v.degrees.eligible_incidence_degree == 2));
    assert_eq!(g.diagnostics(), []);
    invariants(&g);
}
#[test]
fn two_disconnected_analytic_four_edge_squares_remain_distinct() {
    let m = mesh(
        &[
            [0., 0., 0.],
            [4., 0., 0.],
            [4., 4., 0.],
            [0., 4., 0.],
            [8., 0., 0.],
            [12., 0., 0.],
            [12., 4., 0.],
            [8., 4., 0.],
        ],
        vec![[0, 1, 2], [0, 2, 3], [4, 5, 6], [4, 6, 7]],
    );
    let r = raw(&m);
    let n = norm(&r);
    let g = graph(&n);
    assert_eq!(
        (
            g.summary().vertices,
            g.summary().candidate_edges,
            g.summary().internal_edges
        ),
        (8, 8, 2)
    );
    assert!(g
        .edges()
        .iter()
        .all(|e| (e.endpoints[0] < 4) == (e.endpoints[1] < 4)));
    invariants(&g);
}
#[test]
fn open_chain_has_two_degree_one_ends_and_three_degree_two_vertices() {
    let m = ribbon(&[[0., 0.], [4., 0.], [4., 4.]], &[[0, 1], [1, 2]]);
    let r = raw(&m);
    let n = norm(&r);
    let g = graph(&n);
    assert_eq!(
        (
            g.summary().vertices,
            g.summary().candidate_edges,
            g.summary().potential_endpoints
        ),
        (5, 4, 2)
    );
    assert_eq!(
        g.vertices()
            .iter()
            .filter(|v| v.degrees.eligible_incidence_degree == 2)
            .count(),
        3
    );
    invariants(&g);
}
#[test]
fn closed_cycle_has_eight_midplane_edges_without_path_extraction() {
    let m = square_ribbon();
    let r = raw(&m);
    let n = norm(&r);
    let g = graph(&n);
    assert_eq!((g.summary().vertices, g.summary().candidate_edges), (8, 8));
    assert!(g
        .vertices()
        .iter()
        .all(|v| v.degrees.eligible_incidence_degree == 2));
    assert_eq!(g.diagnostics(), []);
    invariants(&g);
}
#[test]
fn multiple_disconnected_cycles_have_no_cross_component_incidence() {
    let m = ribbon(
        &[
            [0., 0.],
            [4., 0.],
            [4., 4.],
            [0., 4.],
            [10., 0.],
            [14., 0.],
            [14., 4.],
            [10., 4.],
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
    let r = raw(&m);
    let n = norm(&r);
    let g = graph(&n);
    assert_eq!(
        (g.summary().vertices, g.summary().candidate_edges),
        (16, 16)
    );
    for edge in g.edges() {
        let side = edge
            .endpoints
            .map(|vertex| matches!(n.features()[vertex].feature,SourceFeature::Edge([start,_]) if start<8));
        assert_eq!(side[0], side[1]);
    }
    invariants(&g);
}
#[test]
fn degree_three_vertex_is_reported_without_routing() {
    check_star(3);
}
#[test]
fn degree_greater_than_three_is_reported_without_routing() {
    check_star(7);
}
fn check_star(arms: usize) {
    let m = star(arms);
    let r = raw(&m);
    let n = norm(&r);
    let g = graph(&n);
    assert_eq!(n.features()[0].feature, SourceFeature::Vertex(0));
    assert_eq!(g.vertices()[0].degrees.eligible_incidence_degree, arms);
    assert_eq!(g.summary().potential_branches, 1);
    assert_eq!(g.summary().potential_endpoints, arms);
    assert!(g.diagnostics().contains(&GraphDiagnostic::PotentialBranch {
        vertex: 0,
        degree: arms
    }));
    invariants(&g);
}
#[test]
fn duplicates_and_reversed_faces_preserve_parallel_raw_occurrences_as_unresolved() {
    let m = mesh(
        &[[0., 0., -1.], [4., 0., 1.], [0., 4., 1.]],
        vec![[0, 1, 2], [2, 1, 0], [0, 1, 2]],
    );
    let r = raw(&m);
    let n = norm(&r);
    let g = graph(&n);
    assert_eq!(
        (
            g.summary().edges,
            g.summary().occurrences,
            g.summary().unresolved_edges,
            g.summary().candidate_edges
        ),
        (1, 3, 1, 0)
    );
    assert_eq!(g.occurrences(0), Some([0, 1, 2].as_slice()));
    assert!(g.vertices().iter().all(|v| v.degrees.unique_edges == 1
        && v.degrees.raw_occurrence_ends == 3
        && v.degrees.eligible_incidence_degree == 0));
    assert_eq!(n.groups()[0].source_faces, 3);
    invariants(&g);
}
#[test]
fn reversed_local_winding_preserves_canonical_graph_and_raw_faces() {
    let a = square_ribbon();
    let b = TriangleMesh::try_new(
        a.vertices().to_vec(),
        a.triangles().iter().map(|&[a, b, c]| [c, b, a]).collect(),
    )
    .unwrap();
    let ar = raw(&a);
    let br = raw(&b);
    let an = norm(&ar);
    let bn = norm(&br);
    let ag = graph(&an);
    let bg = graph(&bn);
    assert_eq!(ag.vertices(), bg.vertices());
    assert_eq!(ag.edges(), bg.edges());
    assert_eq!(ag.summary(), bg.summary());
    assert_eq!(ar.events()[0].source_vertices, [0, 2, 3]);
    assert_eq!(br.events()[0].source_vertices, [3, 2, 0]);
    invariants(&ag);
    invariants(&bg);
}
#[test]
fn opposite_shared_edge_interiors_connect_by_canonical_unordered_pair() {
    let m = mesh(
        &[[0., 0., -1.], [4., 0., 1.], [0., 4., 1.], [0., -4., 1.]],
        vec![[0, 1, 2], [1, 0, 3]],
    );
    let r = raw(&m);
    let n = norm(&r);
    let g = graph(&n);
    let id = n
        .features()
        .iter()
        .position(|v| v.feature == SourceFeature::Edge([0, 1]))
        .unwrap();
    assert_eq!(g.vertices()[id].degrees.eligible_incidence_degree, 2);
    assert_eq!(n.features()[id].observations.len(), 2);
    assert_eq!(g.summary().potential_endpoints, 2);
    invariants(&g);
}
#[test]
fn exact_on_vertex_is_one_vertex_identity_never_an_interior_edge_alias() {
    let m = star(3);
    let r = raw(&m);
    let n = norm(&r);
    let g = graph(&n);
    assert_eq!(n.features()[0].feature, SourceFeature::Vertex(0));
    assert_eq!(n.features()[0].observations.len(), 3);
    assert!(n.features().iter().skip(1).all(
        |feature| matches!(feature.feature,SourceFeature::Edge([start,end]) if start>0&&end>0)
    ));
    invariants(&g);
}
#[test]
fn tangent_points_remain_isolated_vertices_without_segment_edges() {
    let m = mesh(&[[0., 0., 0.], [1., 0., 1.], [0., 1., 1.]], vec![[0, 1, 2]]);
    let r = raw(&m);
    let n = norm(&r);
    let g = graph(&n);
    assert_eq!(
        (
            g.summary().vertices,
            g.summary().edges,
            g.summary().isolated_vertices
        ),
        (1, 0, 1)
    );
    assert_eq!(n.observations().len(), 1);
    assert_eq!(g.diagnostics(), []);
    invariants(&g);
}
#[test]
fn one_coplanar_face_has_three_local_candidates_not_a_filled_region() {
    let m = mesh(&[[0., 0., 0.], [4., 0., 0.], [0., 4., 0.]], vec![[0, 1, 2]]);
    let r = raw(&m);
    let n = norm(&r);
    let g = graph(&n);
    assert_eq!(
        (g.summary().candidate_edges, g.summary().occurrences),
        (3, 3)
    );
    assert!(n
        .groups()
        .iter()
        .all(|e| e.ownership == Ownership::CoplanarPatchBoundary));
    invariants(&g);
}
#[test]
fn repeated_source_index_preserves_self_loop_ends_and_multiplicity() {
    let m = mesh(&[[0., 0., 0.], [4., 0., 0.]], vec![[0, 0, 1]]);
    let r = raw(&m);
    let n = norm(&r);
    let g = graph(&n);
    assert_eq!(
        (
            g.summary().edges,
            g.summary().occurrences,
            g.summary().candidate_edges
        ),
        (2, 3, 0)
    );
    let loop_id = g
        .edges()
        .iter()
        .position(|e| e.endpoints == [0, 0])
        .unwrap();
    assert_eq!(
        g.complete_adjacency(0)
            .unwrap()
            .iter()
            .filter(|a| a.edge == loop_id)
            .count(),
        2
    );
    assert_eq!(g.vertices()[0].degrees.unique_edges, 2);
    assert_eq!(g.vertices()[0].degrees.raw_occurrence_ends, 4);
    invariants(&g);
}
#[test]
fn coincident_disconnected_squares_keep_distinct_feature_components() {
    let m = mesh(
        &[
            [0., 0., 0.],
            [4., 0., 0.],
            [4., 4., 0.],
            [0., 4., 0.],
            [0., 0., 0.],
            [4., 0., 0.],
            [4., 4., 0.],
            [0., 4., 0.],
        ],
        vec![[0, 1, 2], [0, 2, 3], [4, 5, 6], [4, 6, 7]],
    );
    let r = raw(&m);
    let n = norm(&r);
    let g = graph(&n);
    assert_eq!((g.summary().vertices, g.summary().candidate_edges), (8, 8));
    assert_eq!(n.summary().coincident_feature_positions, 4);
    assert!(g
        .edges()
        .iter()
        .all(|e| (e.endpoints[0] < 4) == (e.endpoints[1] < 4)));
    invariants(&g);
}
#[test]
fn geometrically_crossing_edge_interiors_are_not_split_or_joined() {
    let m = ribbon(
        &[[-2., 0.], [2., 0.], [1., -3.], [1., 1.]],
        &[[0, 1], [2, 3]],
    );
    let r = raw(&m);
    let n = norm(&r);
    let g = graph(&n);
    assert_eq!(
        (
            g.summary().vertices,
            g.summary().edges,
            g.summary().potential_endpoints
        ),
        (6, 4, 4)
    );
    // (1,0) is strictly inside both upper half-segments, not an existing feature endpoint.
    assert_eq!(n.summary().coincident_feature_positions, 0);
    assert!(!n
        .features()
        .iter()
        .any(|v| v.coordinates == CoordinateAgreement::Agreed(Point3::new(1., 0., 0.))));
    assert_eq!(g.summary().potential_branches, 0);
    invariants(&g);
}
#[test]
fn strict_crossing_at_an_on_vertex_coordinate_retains_separate_feature_identity() {
    let m = mesh(
        &[
            [8., 0., -1.],
            [8., 0., 1.],
            [8., 4., 1.],
            [8., 0., 0.],
            [9., 0., 1.],
            [8., 1., 1.],
        ],
        vec![[0, 1, 2], [3, 4, 5]],
    );
    let r = raw(&m);
    let n = norm(&r);
    let g = graph(&n);
    assert_eq!(n.features()[0].feature, SourceFeature::Vertex(3));
    assert_eq!(n.features()[1].feature, SourceFeature::Edge([0, 1]));
    assert_eq!(n.features()[0].coordinates, n.features()[1].coordinates);
    assert_eq!(g.vertices()[0].degrees.unique_edges, 0);
    assert_eq!(g.vertices()[1].degrees.eligible_incidence_degree, 1);
    assert_eq!(g.summary().vertices, 3);
    invariants(&g);
}
#[test]
fn nearby_endpoints_do_not_weld() {
    let delta = f64::from_bits(1);
    let m = mesh(
        &[
            [0., 0., 0.],
            [1., 0., 1.],
            [0., 1., 1.],
            [delta, 0., 0.],
            [1., 0., 1.],
            [0., 1., 1.],
        ],
        vec![[0, 1, 2], [3, 4, 5]],
    );
    let r = raw(&m);
    let n = norm(&r);
    let g = graph(&n);
    assert_eq!(
        (g.summary().vertices, g.summary().isolated_vertices),
        (2, 2)
    );
    invariants(&g);
}
#[test]
fn singular_transform_preserves_distinct_features_and_excludes_collapsed_edges() {
    let m = planar_square();
    let transform = Affine3::from_scale(Vector3::new(0., 0., 0.)).unwrap();
    let r = section_mesh_z(
        &m,
        0.,
        SectionTransform::Affine(&transform),
        SectionLimits::default(),
    )
    .unwrap();
    let n = norm(&r);
    let g = graph(&n);
    assert_eq!(
        (
            g.summary().vertices,
            g.summary().edges,
            g.summary().candidate_edges,
            g.summary().occurrences
        ),
        (4, 5, 0, 6)
    );
    assert_eq!(g.summary().unresolved_edges, 5);
    assert_eq!(n.coordinates().len(), 1);
    invariants(&g);
}
#[test]
fn zero_length_crossing_is_retained_only_in_complete_adjacency() {
    let m = mesh(
        &[[0., 0., -1.], [0., 0., 1.], [0., 0., 2.]],
        vec![[0, 1, 2]],
    );
    let r = raw(&m);
    let n = norm(&r);
    let g = graph(&n);
    assert_eq!(
        (
            g.summary().edges,
            g.summary().candidate_edges,
            g.summary().unresolved_edges
        ),
        (1, 0, 1)
    );
    invariants(&g);
}
#[test]
fn coherent_mixed_cap_edge_preserves_both_occurrences_as_candidate() {
    let m = mesh(
        &[[0., 0., 0.], [4., 0., 0.], [0., 4., 0.], [0., 0., 1.]],
        vec![[0, 1, 2], [1, 0, 3]],
    );
    let r = raw(&m);
    let n = norm(&r);
    let g = graph(&n);
    assert_eq!(
        (
            g.summary().edges,
            g.summary().candidate_edges,
            g.summary().unresolved_edges
        ),
        (3, 3, 0)
    );
    assert!(g
        .vertices()
        .iter()
        .all(|v| v.degrees.eligible_incidence_degree == 2 && v.degrees.unresolved_edges == 0));
    invariants(&g);
}
#[test]
fn degree_two_with_unresolved_incidence_is_not_silently_certified() {
    let m = mesh(
        &[
            [0., 0., 0.],
            [4., 0., 0.],
            [0., 4., 0.],
            [0., 0., 1.],
            [0., -4., 0.],
        ],
        vec![[0, 1, 2], [0, 1, 3], [0, 1, 4]],
    );
    let r = raw(&m);
    let n = norm(&r);
    let g = graph(&n);
    assert!(g
        .vertices()
        .iter()
        .any(|v| v.degrees.eligible_incidence_degree == 2 && v.degrees.unresolved_edges > 0));
    assert!(g
        .diagnostics()
        .iter()
        .any(|d| matches!(d, GraphDiagnostic::UnresolvedEdge { .. })));
    invariants(&g);
}
#[test]
fn nonmanifold_on_plane_edge_keeps_all_contributing_faces() {
    let m = mesh(
        &[
            [0., 0., 0.],
            [4., 0., 0.],
            [0., 4., 1.],
            [0., -4., 1.],
            [0., 2., -1.],
        ],
        vec![[0, 1, 2], [1, 0, 3], [0, 1, 4]],
    );
    let r = raw(&m);
    let n = norm(&r);
    let g = graph(&n);
    assert_eq!(
        (
            g.summary().edges,
            g.summary().occurrences,
            g.summary().candidate_edges
        ),
        (1, 3, 0)
    );
    invariants(&g);
}
#[test]
fn nonmanifold_vertex_contact_preserves_two_local_cycles() {
    let m = mesh(
        &[
            [0., 0., 0.],
            [4., 0., 0.],
            [0., 4., 0.],
            [-4., 0., 0.],
            [0., -4., 0.],
        ],
        vec![[0, 1, 2], [0, 3, 4]],
    );
    let r = raw(&m);
    let n = norm(&r);
    let g = graph(&n);
    assert_eq!(g.vertices()[0].degrees.eligible_incidence_degree, 4);
    assert_eq!(g.summary().potential_branches, 1);
    invariants(&g);
}

#[test]
fn interior_cube_slice_has_eight_half_sides_and_degree_two_features() {
    let m = fixture("fx-01-cube.stl");
    let r = section_mesh_z(
        &m,
        10.,
        SectionTransform::Identity,
        SectionLimits::default(),
    )
    .unwrap();
    let n = norm(&r);
    let g = graph(&n);
    assert_eq!((g.summary().vertices, g.summary().candidate_edges), (8, 8));
    assert!(g
        .vertices()
        .iter()
        .all(|v| v.degrees.eligible_incidence_degree == 2 && v.degrees.raw_occurrence_ends == 2));
    for v in n.features() {
        let CoordinateAgreement::Agreed(position) = v.coordinates else {
            panic!("agreement")
        };
        assert!(
            position.x.abs().to_bits() == 10_f64.to_bits()
                || position.y.abs().to_bits() == 10_f64.to_bits()
        );
    }
    invariants(&g);
}
#[test]
fn cube_exact_cap_retains_all_occurrences_and_certifies_local_patch_perimeter() {
    let m = fixture("fx-01-cube.stl");
    let r = raw(&m);
    let n = norm(&r);
    let g = graph(&n);
    assert_eq!(n.summary().tangent_events, 4);
    assert_eq!(n.summary().coplanar_faces, 2);
    assert_eq!(
        (
            g.summary().vertices,
            g.summary().edges,
            g.summary().occurrences,
            g.summary().internal_edges,
            g.summary().unresolved_edges,
            g.summary().candidate_edges
        ),
        (4, 5, 10, 1, 0, 4)
    );
    invariants(&g);
}
#[test]
fn cylinder_midplane_has_256_candidates_and_degree_two_vertices() {
    let m = fixture("fx-02-cylinder.stl");
    let r = section_mesh_z(
        &m,
        10.,
        SectionTransform::Identity,
        SectionLimits::default(),
    )
    .unwrap();
    let n = norm(&r);
    let g = graph(&n);
    assert_eq!(
        (g.summary().vertices, g.summary().candidate_edges),
        (256, 256)
    );
    assert!(g
        .vertices()
        .iter()
        .all(|v| v.degrees.eligible_incidence_degree == 2));
    invariants(&g);
}
#[test]
fn rotated_prism_preserves_eight_midplane_incidences() {
    let m = fixture("fx-05-rotated-prism.stl");
    let r = section_mesh_z(
        &m,
        17.,
        SectionTransform::Identity,
        SectionLimits::default(),
    )
    .unwrap();
    let n = norm(&r);
    let g = graph(&n);
    assert_eq!((g.summary().vertices, g.summary().candidate_edges), (8, 8));
    assert!(g
        .vertices()
        .iter()
        .all(|v| v.degrees.eligible_incidence_degree == 2));
    invariants(&g);
}
#[test]
fn canonical_open_nonmanifold_duplicate_and_degenerate_fixtures_keep_full_input() {
    for filename in [
        "fx-10a-open.stl",
        "fx-10b-non-manifold.stl",
        "fx-08-degenerate-faces.stl",
        "fx-09-reversed-face.stl",
    ] {
        let m = fixture(filename);
        let before = fixture(filename);
        for plane in [0., 10., 20.] {
            let r = section_mesh_z(
                &m,
                plane,
                SectionTransform::Identity,
                SectionLimits::default(),
            )
            .unwrap();
            let before_raw = r.events().to_vec();
            let n = norm(&r);
            let before_n = format!("{n:?}");
            let g = graph(&n);
            invariants(&g);
            assert_eq!(r.events(), before_raw);
            assert_eq!(format!("{n:?}"), before_n);
            assert_eq!(m, before);
        }
    }
}
#[test]
fn repeated_builds_preserve_exact_order_and_source_geometry() {
    let m = square_ribbon();
    let vertices = m.vertices().to_vec();
    let faces = m.triangles().to_vec();
    let bounds = m.source_bounds();
    let r = raw(&m);
    let n = norm(&r);
    let g = graph(&n);
    for _ in 0..16 {
        assert_eq!(graph(&n), g);
    }
    assert_eq!(m.vertices(), vertices);
    assert_eq!(m.triangles(), faces);
    assert_eq!(m.source_bounds(), bounds);
    invariants(&g);
}
#[test]
fn face_permutations_preserve_graph_ids_and_remapped_occurrence_provenance() {
    let a = square_ribbon();
    let permutation = [7, 2, 5, 0, 6, 1, 4, 3];
    let b = TriangleMesh::try_new(
        a.vertices().to_vec(),
        permutation.map(|i| a.triangles()[i]).to_vec(),
    )
    .unwrap();
    let ar = raw(&a);
    let br = raw(&b);
    let an = norm(&ar);
    let bn = norm(&br);
    let ag = graph(&an);
    let bg = graph(&bn);
    assert_eq!(ag.vertices(), bg.vertices());
    assert_eq!(ag.edges(), bg.edges());
    assert_eq!(ag.diagnostics(), bg.diagnostics());
    for id in 0..ag.edges().len() {
        let af = ag
            .occurrences(id)
            .unwrap()
            .iter()
            .map(|&i| an.raw().events()[an.segments()[i].event].source_face)
            .collect::<Vec<_>>();
        let bf = bg
            .occurrences(id)
            .unwrap()
            .iter()
            .map(|&i| permutation[bn.raw().events()[bn.segments()[i].event].source_face])
            .collect::<Vec<_>>();
        assert_eq!(af, bf);
    }
}
#[test]
fn exact_count_caps_pass_and_one_below_caps_fail_with_correct_resource() {
    let m = planar_square();
    let r = raw(&m);
    let n = norm(&r);
    let limits = GraphLimits {
        max_vertices: 4,
        max_edges: 5,
        max_incidence_references: 6,
        max_adjacency_entries: 18,
        max_diagnostics: 0,
        max_payload_bytes: usize::MAX,
    };
    assert!(build_incidence_graph(&n, limits).is_ok());
    for (tight, resource, limit) in [
        (
            GraphLimits {
                max_vertices: 3,
                ..limits
            },
            GraphResource::Vertices,
            3,
        ),
        (
            GraphLimits {
                max_edges: 4,
                ..limits
            },
            GraphResource::Edges,
            4,
        ),
        (
            GraphLimits {
                max_incidence_references: 5,
                ..limits
            },
            GraphResource::IncidenceReferences,
            5,
        ),
        (
            GraphLimits {
                max_adjacency_entries: 17,
                ..limits
            },
            GraphResource::AdjacencyEntries,
            17,
        ),
        (
            GraphLimits {
                max_payload_bytes: 0,
                ..limits
            },
            GraphResource::PayloadBytes,
            0,
        ),
    ] {
        assert_eq!(
            build_incidence_graph(&n, tight),
            Err(GraphError::LimitExceeded { resource, limit })
        );
    }
}
#[test]
fn diagnostic_cap_is_actual_count_not_an_upper_bound_guess() {
    let m = star(3);
    let r = raw(&m);
    let n = norm(&r);
    assert!(build_incidence_graph(
        &n,
        GraphLimits {
            max_diagnostics: 4,
            ..GraphLimits::default()
        }
    )
    .is_ok());
    assert_eq!(
        build_incidence_graph(
            &n,
            GraphLimits {
                max_diagnostics: 3,
                ..GraphLimits::default()
            }
        ),
        Err(GraphError::LimitExceeded {
            resource: GraphResource::Diagnostics,
            limit: 3
        })
    );
}
#[test]
fn exact_payload_budget_passes_and_one_byte_less_fails_before_publication() {
    let m = star(3);
    let r = raw(&m);
    let n = norm(&r);
    let required = graph(&n).summary().requested_payload_bytes;
    assert!(build_incidence_graph(
        &n,
        GraphLimits {
            max_payload_bytes: required,
            ..GraphLimits::default()
        }
    )
    .is_ok());
    assert_eq!(
        build_incidence_graph(
            &n,
            GraphLimits {
                max_payload_bytes: required - 1,
                ..GraphLimits::default()
            }
        ),
        Err(GraphError::LimitExceeded {
            resource: GraphResource::PayloadBytes,
            limit: required - 1
        })
    );
}
#[test]
fn huge_configured_caps_do_not_allocate_based_on_limits() {
    let m = star(1);
    let r = raw(&m);
    let n = norm(&r);
    let huge = GraphLimits {
        max_vertices: usize::MAX,
        max_edges: usize::MAX,
        max_incidence_references: usize::MAX,
        max_adjacency_entries: usize::MAX,
        max_diagnostics: usize::MAX,
        max_payload_bytes: usize::MAX,
    };
    assert_eq!(build_incidence_graph(&n, huge).unwrap(), graph(&n));
}
#[test]
fn pre_cancel_wins_before_resource_error() {
    let m = star(1);
    let r = raw(&m);
    let n = norm(&r);
    let mut predicate = |_| true;
    assert_eq!(
        build_incidence_graph_with_cancellation(
            &n,
            GraphLimits {
                max_vertices: 0,
                ..GraphLimits::default()
            },
            &mut Cancellation::new(&mut predicate)
        ),
        Err(GraphError::Cancelled(Cancelled {
            checkpoint: Checkpoint::GraphStart
        }))
    );
}
#[test]
fn cancellation_at_every_phase_and_successful_retry_preserve_input() {
    let m = star(600);
    let r = raw(&m);
    let n = norm(&r);
    let before = format!("{n:?}");
    for checkpoint in [
        Checkpoint::GraphRecords,
        Checkpoint::GraphStorage,
        Checkpoint::GraphSorting,
        Checkpoint::GraphAdjacency,
        Checkpoint::GraphDiagnostics,
        Checkpoint::GraphComplete,
    ] {
        let mut predicate = |p| p == checkpoint;
        assert_eq!(
            build_incidence_graph_with_cancellation(
                &n,
                GraphLimits::default(),
                &mut Cancellation::new(&mut predicate)
            ),
            Err(GraphError::Cancelled(Cancelled { checkpoint }))
        );
        assert_eq!(format!("{n:?}"), before);
    }
    assert_eq!(graph(&n).summary().candidate_edges, 600);
}
#[test]
fn cancellation_inside_record_and_high_degree_adjacency_loops() {
    let m = star(800);
    let r = raw(&m);
    let n = norm(&r);
    for checkpoint in [
        Checkpoint::GraphRecords,
        Checkpoint::GraphAdjacency,
        Checkpoint::GraphDiagnostics,
    ] {
        let mut calls = 0;
        let mut predicate = |p| {
            if p == checkpoint {
                calls += 1;
            }
            p == checkpoint && calls == 3
        };
        assert_eq!(
            build_incidence_graph_with_cancellation(
                &n,
                GraphLimits::default(),
                &mut Cancellation::new(&mut predicate)
            ),
            Err(GraphError::Cancelled(Cancelled { checkpoint }))
        );
        assert_eq!(calls, 3);
    }
}

#[test]
fn cancellation_reaches_inside_one_high_degree_vertex_after_both_sorts() {
    let m = star(800);
    let r = raw(&m);
    let n = norm(&r);
    let mut adjacency_polls = 0;
    let mut sort_polls = 0;
    let mut predicate = |checkpoint| {
        if checkpoint == Checkpoint::GraphSorting {
            sort_polls += 1;
        }
        if checkpoint == Checkpoint::GraphAdjacency {
            adjacency_polls += 1;
        }
        checkpoint == Checkpoint::GraphAdjacency && adjacency_polls == 7
    };
    assert_eq!(
        build_incidence_graph_with_cancellation(
            &n,
            GraphLimits::default(),
            &mut Cancellation::new(&mut predicate)
        ),
        Err(GraphError::Cancelled(Cancelled {
            checkpoint: Checkpoint::GraphAdjacency
        }))
    );
    assert_eq!(sort_polls, 4);
    assert_eq!(adjacency_polls, 7);
    assert_eq!(
        graph(&n).vertices()[0].degrees.eligible_incidence_degree,
        800
    );
}

#[test]
fn parallel_occurrences_remap_face_ids_as_a_multiset_under_face_permutation() {
    let vertices = [[0., 0., -1.], [4., 0., 1.], [0., 4., 1.]];
    let a = mesh(&vertices, vec![[0, 1, 2], [2, 1, 0], [0, 1, 2]]);
    let b = mesh(&vertices, vec![[0, 1, 2], [0, 1, 2], [2, 1, 0]]);
    let ar = raw(&a);
    let br = raw(&b);
    let an = norm(&ar);
    let bn = norm(&br);
    let ag = graph(&an);
    let bg = graph(&bn);
    assert_eq!(ag.vertices(), bg.vertices());
    assert_eq!(ag.edges(), bg.edges());
    let mut mapped = bg
        .occurrences(0)
        .unwrap()
        .iter()
        .map(|&id| [2, 0, 1][bn.raw().events()[bn.segments()[id].event].source_face])
        .collect::<Vec<_>>();
    mapped.sort_unstable();
    assert_eq!(mapped, vec![0, 1, 2]);
    assert_eq!(ag.summary(), bg.summary());
    assert_eq!(ag.diagnostics(), bg.diagnostics());
}
#[test]
fn cancellation_covers_both_sides_of_each_allocation_and_sort() {
    let m = star(3);
    let r = raw(&m);
    let n = norm(&r);
    for (checkpoint, total) in [
        (Checkpoint::GraphStorage, 10),
        (Checkpoint::GraphSorting, 4),
    ] {
        for selected in 1..=total {
            let mut seen = 0;
            let mut predicate = |p| {
                if p == checkpoint {
                    seen += 1;
                }
                p == checkpoint && seen == selected
            };
            assert_eq!(
                build_incidence_graph_with_cancellation(
                    &n,
                    GraphLimits::default(),
                    &mut Cancellation::new(&mut predicate)
                ),
                Err(GraphError::Cancelled(Cancelled { checkpoint }))
            );
            assert_eq!(seen, selected);
        }
    }
}
#[test]
fn final_cancellation_also_applies_to_empty_graph() {
    let m = mesh(&[], vec![]);
    let r = raw(&m);
    let n = norm(&r);
    let mut predicate = |p| p == Checkpoint::GraphComplete;
    assert_eq!(
        build_incidence_graph_with_cancellation(
            &n,
            GraphLimits::default(),
            &mut Cancellation::new(&mut predicate)
        ),
        Err(GraphError::Cancelled(Cancelled {
            checkpoint: Checkpoint::GraphComplete
        }))
    );
}
#[test]
fn never_cancelling_wrapper_and_typed_error_causes_match() {
    let m = star(2);
    let r = raw(&m);
    let n = norm(&r);
    assert_eq!(
        build_incidence_graph_with_cancellation(
            &n,
            GraphLimits::default(),
            &mut Cancellation::never()
        )
        .unwrap(),
        graph(&n)
    );
    let error = GraphError::Cancelled(Cancelled {
        checkpoint: Checkpoint::GraphComplete,
    });
    assert!(error.source().is_some());
    assert!(error.to_string().contains("GraphComplete"));
    assert!(GraphError::Allocation.source().is_none());
    assert_ne!(GraphError::SizeOverflow.to_string(), "");
}
