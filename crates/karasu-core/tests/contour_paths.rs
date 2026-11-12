//! Independent expected path sequences, conservative boundaries and complete incidence accounting.

use karasu_core::cancellation::{Cancellation, Cancelled, Checkpoint};
use karasu_core::geometry::{Affine3, Point3, Vector3};
use karasu_core::mesh::{
    stl::{import_stl_path, StlOptions, StlUnitPolicy},
    TriangleMesh,
};
use karasu_core::slicing::graph::{
    build_incidence_graph, EdgeEligibility, GraphLimits, IncidenceGraph,
};
use karasu_core::slicing::normalize::{
    normalize_section, IncidenceIssue, NormalizationLimits, NormalizedSection,
};
use karasu_core::slicing::paths::*;
use karasu_core::slicing::{section_mesh_z, MeshSection, SectionLimits, SectionTransform};
use std::{collections::BTreeSet, error::Error, path::PathBuf};

fn mesh(points: &[[f64; 3]], faces: Vec<[u32; 3]>) -> TriangleMesh {
    TriangleMesh::try_new(
        points
            .iter()
            .map(|&[x, y, z]| Point3::new(x, y, z))
            .collect(),
        faces,
    )
    .unwrap()
}
fn raw(mesh: &TriangleMesh, plane: f64) -> MeshSection {
    section_mesh_z(
        mesh,
        plane,
        SectionTransform::Identity,
        SectionLimits::default(),
    )
    .unwrap()
}
fn normalized(raw: &MeshSection) -> NormalizedSection<'_> {
    normalize_section(raw, NormalizationLimits::default()).unwrap()
}
fn graph<'n, 'r>(normalized: &'n NormalizedSection<'r>) -> IncidenceGraph<'n, 'r> {
    build_incidence_graph(normalized, GraphLimits::default()).unwrap()
}
fn extract<'g, 'n, 'r>(graph: &'g IncidenceGraph<'n, 'r>) -> SectionPaths<'g, 'n, 'r> {
    extract_paths(graph, PathLimits::default()).unwrap()
}
fn evaluate(mesh: &TriangleMesh, plane: f64, check: impl FnOnce(&SectionPaths<'_, '_, '_>)) {
    let raw = raw(mesh, plane);
    let normalized = normalized(&raw);
    let graph = graph(&normalized);
    let paths = extract(&graph);
    accounting(&paths);
    check(&paths);
}
fn square() -> TriangleMesh {
    mesh(
        &[[0., 0., 0.], [4., 0., 0.], [4., 4., 0.], [0., 4., 0.]],
        vec![[0, 1, 2], [0, 2, 3]],
    )
}
fn ribbon(points: &[[f64; 2]], links: &[[u32; 2]]) -> TriangleMesh {
    let vertices = points
        .iter()
        .flat_map(|&[x, y]| [Point3::new(x, y, -1.), Point3::new(x, y, 1.)])
        .collect();
    let faces = links
        .iter()
        .flat_map(|&[start, end]| {
            [
                [2 * start, 2 * end, 2 * end + 1],
                [2 * start, 2 * end + 1, 2 * start + 1],
            ]
        })
        .collect();
    TriangleMesh::try_new(vertices, faces).unwrap()
}
fn square_ribbon() -> TriangleMesh {
    ribbon(
        &[[0., 0.], [4., 0.], [4., 4.], [0., 4.]],
        &[[0, 1], [1, 2], [2, 3], [3, 0]],
    )
}
fn star(tips: &[[f64; 2]]) -> TriangleMesh {
    let mut vertices = vec![Point3::new(0., 0., 0.)];
    let mut faces = vec![];
    for &[x, y] in tips {
        let next = u32::try_from(vertices.len()).unwrap();
        vertices.push(Point3::new(x, y, -1.));
        vertices.push(Point3::new(x, y, 1.));
        faces.push([0, next, next + 1]);
    }
    TriangleMesh::try_new(vertices, faces).unwrap()
}
fn long_chain(count: u32) -> TriangleMesh {
    let points = (0..count).map(|id| [f64::from(id), 0.]).collect::<Vec<_>>();
    let links = (1..count).map(|id| [id - 1, id]).collect::<Vec<_>>();
    ribbon(&points, &links)
}
fn long_cycle(count: u32) -> TriangleMesh {
    let points = (0..count)
        .map(|id| {
            let angle = f64::from(id) / f64::from(count) * std::f64::consts::TAU;
            [angle.cos(), angle.sin()]
        })
        .collect::<Vec<_>>();
    let links = (0..count)
        .map(|id| [id, (id + 1) % count])
        .collect::<Vec<_>>();
    ribbon(&points, &links)
}
fn fixture(name: &str) -> TriangleMesh {
    import_stl_path(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/reference/stl")
            .join(name),
        StlOptions::new(StlUnitPolicy::AssumeMillimeters),
    )
    .unwrap()
    .mesh
}

fn candidate_multiplicity(group: &karasu_core::slicing::normalize::SegmentGroup) -> usize {
    use karasu_core::slicing::normalize::Ownership;
    if group.ownership == Ownership::OnPlaneCrossingCandidate
        || (group.ownership == Ownership::CoplanarPatchBoundary && group.occurrences.len() == 2)
    {
        2
    } else {
        1
    }
}

fn accounting(result: &SectionPaths<'_, '_, '_>) {
    let graph = result.graph();
    let mut consumed = BTreeSet::new();
    let mut previous = None;
    for (id, path) in result.paths().iter().enumerate() {
        let vertices = result.path_vertices(id).unwrap();
        let edges = result.path_edges(id).unwrap();
        assert_ne!(edges, [] as [usize; 0]);
        assert_eq!(vertices.len(), edges.len() + usize::from(!path.closed));
        assert_eq!(
            vertices.iter().copied().collect::<BTreeSet<_>>().len(),
            vertices.len()
        );
        let key = (vertices[0], edges[0]);
        assert!(previous.is_none_or(|last| last < key));
        previous = Some(key);
        if path.closed {
            assert_eq!(vertices[0], *vertices.iter().min().unwrap());
            assert!(edges[0] < edges[edges.len() - 1]);
            assert_eq!(path.endpoints, None);
        } else {
            assert!(vertices[0] < vertices[vertices.len() - 1]);
            assert_eq!(
                path.endpoints,
                Some([vertices[0], vertices[vertices.len() - 1]])
            );
        }
        for (offset, &edge) in edges.iter().enumerate() {
            assert!(consumed.insert(edge));
            assert_eq!(graph.edges()[edge].eligibility, EdgeEligibility::Candidate);
            let mut pair = [vertices[offset], vertices[(offset + 1) % vertices.len()]];
            pair.sort_unstable();
            assert_eq!(pair, graph.edges()[edge].endpoints);
            assert_eq!(
                result.edge_outcome(edge),
                Some(EdgeOutcome::Path { path: id, offset })
            );
            let expected = candidate_multiplicity(&graph.normalized().groups()[edge]);
            assert_eq!(graph.occurrences(edge).unwrap().len(), expected);
        }
        if path.kind != PathKind::AmbiguousTrail {
            assert!(path.issues.is_empty());
        }
        if path.kind == PathKind::ClosedCycleCandidate {
            assert!(path.closed && vertices.len() >= 3);
        }
    }
    let mut raw_occurrences = 0;
    let mut internal = 0;
    let mut unresolved = 0;
    for (id, edge) in graph.edges().iter().enumerate() {
        raw_occurrences += graph.occurrences(id).unwrap().len();
        match edge.eligibility {
            EdgeEligibility::Candidate => assert!(consumed.contains(&id)),
            EdgeEligibility::InternalCoplanar => {
                assert_eq!(result.edge_outcome(id), Some(EdgeOutcome::InternalCoplanar));
                internal += 1;
            }
            EdgeEligibility::Unresolved => {
                assert_eq!(result.edge_outcome(id), Some(EdgeOutcome::Unresolved));
                unresolved += 1;
            }
        }
    }
    for (id, component) in result.components().iter().enumerate() {
        let paths = result
            .paths()
            .iter()
            .enumerate()
            .filter(|(_, p)| p.component == id)
            .collect::<Vec<_>>();
        assert_eq!(component.paths, paths.len());
        let mut vertices = BTreeSet::new();
        let mut edge_count = 0;
        for (path, _) in paths {
            vertices.extend(result.path_vertices(path).unwrap().iter().copied());
            edge_count += result.path_edges(path).unwrap().len();
        }
        assert_eq!(component.vertices, vertices.len());
        assert_eq!(component.edges, edge_count);
        assert_eq!(component.minimum_vertex, *vertices.first().unwrap());
    }
    let summary = result.summary();
    assert_eq!(summary.consumed_edges, consumed.len());
    assert_eq!(summary.candidate_edges, consumed.len());
    assert_eq!(summary.graph_edges, consumed.len() + internal + unresolved);
    assert_eq!(summary.raw_occurrences, raw_occurrences);
    assert_eq!(summary.paths, result.paths().len());
    assert_eq!(summary.components, result.components().len());
    assert_eq!(summary.diagnostics, result.diagnostics().len());
    assert_eq!(result.path_vertices(usize::MAX), None);
    assert_eq!(result.path_edges(usize::MAX), None);
    assert_eq!(result.edge_outcome(usize::MAX), None);
}

#[test]
fn empty_graph_accepts_all_zero_caps_and_borrows_input() {
    let mesh = mesh(&[], vec![]);
    let raw = raw(&mesh, 0.);
    let normalized = normalized(&raw);
    let graph = graph(&normalized);
    let limits = PathLimits {
        max_graph_vertices: 0,
        max_graph_edges: 0,
        max_paths: 0,
        max_path_vertices: 0,
        max_path_edges: 0,
        max_components: 0,
        max_diagnostics: 0,
        max_payload_bytes: 0,
    };
    let paths = extract_paths(&graph, limits).unwrap();
    assert_eq!(paths.summary(), PathSummary::default());
    assert!(std::ptr::eq(paths.graph(), std::ptr::from_ref(&graph)));
    accounting(&paths);
}
#[test]
fn single_edge_yields_one_canonical_open_chain() {
    let mesh = mesh(
        &[[0., 0., -1.], [4., 0., 1.], [0., 4., 1.]],
        vec![[0, 1, 2]],
    );
    evaluate(&mesh, 0., |p| {
        assert_eq!(p.path_vertices(0), Some([0, 1].as_slice()));
        assert_eq!(p.path_edges(0), Some([0].as_slice()));
        assert_eq!(p.paths()[0].kind, PathKind::OpenChain);
        assert_eq!(p.summary().open_chains, 1);
        assert_eq!(p.components()[0].vertices, 2);
    });
}
#[test]
fn four_edge_square_has_exact_canonical_sequence_and_retained_internal_diagonal() {
    evaluate(&square(), 0., |p| {
        assert_eq!(p.path_vertices(0), Some([0, 1, 2, 3].as_slice()));
        assert_eq!(p.path_edges(0), Some([0, 3, 4, 2].as_slice()));
        assert_eq!(p.paths()[0].kind, PathKind::ClosedCycleCandidate);
        assert_eq!(p.edge_outcome(1), Some(EdgeOutcome::InternalCoplanar));
        assert_eq!(p.graph().occurrences(1).unwrap().len(), 2);
        assert_eq!(p.summary().raw_occurrences, 6);
        assert_eq!(p.summary().closed_walks, 1);
        assert_eq!(p.diagnostics(), []);
    });
}
#[test]
fn two_disconnected_squares_produce_distinct_ordered_cycles() {
    let mesh = mesh(
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
    evaluate(&mesh, 0., |p| {
        assert_eq!(p.summary().cycle_candidates, 2);
        assert_eq!(p.summary().components, 2);
        assert_eq!(p.path_vertices(0), Some([0, 1, 2, 3].as_slice()));
        assert_eq!(p.path_vertices(1), Some([4, 5, 6, 7].as_slice()));
        assert_eq!(p.paths()[1].component, 1);
    });
}
#[test]
fn linear_ribbon_has_one_maximal_exact_open_sequence() {
    evaluate(&long_chain(3), 0., |p| {
        assert_eq!(p.path_vertices(0), Some([0, 1, 2, 3, 4].as_slice()));
        assert_eq!(p.path_edges(0), Some([0, 1, 2, 3].as_slice()));
        assert_eq!(p.summary().open_chains, 1);
        assert_eq!(p.paths()[0].endpoints, Some([0, 4]));
    });
}
#[test]
fn multiple_disconnected_open_chains_are_all_consumed_once() {
    let mesh = ribbon(
        &[[0., 0.], [4., 0.], [8., 0.], [12., 0.]],
        &[[0, 1], [2, 3]],
    );
    evaluate(&mesh, 0., |p| {
        assert_eq!(p.summary().open_chains, 2);
        assert_eq!(p.summary().components, 2);
        assert_eq!(p.summary().consumed_edges, 4);
    });
}
#[test]
fn t_junction_stops_three_trails_at_one_shared_branch_without_routing() {
    evaluate(&star(&[[-2., 0.], [2., 0.], [0., 2.]]), 0., |p| {
        assert_eq!(p.summary().ambiguous_trails, 3);
        assert_eq!(p.summary().ambiguous_components, 1);
        assert_eq!(p.summary().open_chains, 0);
        for id in 0..3 {
            assert_eq!(p.path_vertices(id), Some([0, id + 1].as_slice()));
            assert_eq!(p.path_edges(id), Some([id].as_slice()));
            assert!(p.paths()[id].issues.contains(PathIssue::Branch));
        }
        assert!(matches!(
            p.diagnostics(),
            [PathDiagnostic::Vertex {
                vertex: 0,
                isolated: false,
                ..
            }]
        ));
    });
}
#[test]
fn degree_four_crossroad_yields_four_unrouted_single_edge_trails() {
    evaluate(
        &star(&[[-2., 0.], [2., 0.], [0., 2.], [0., -2.]]),
        0.,
        |p| {
            assert_eq!(p.summary().ambiguous_trails, 4);
            assert_eq!(p.components()[0].edges, 4);
            assert!(p
                .paths()
                .iter()
                .all(|path| !path.closed && path.issues.contains(PathIssue::Branch)));
        },
    );
}
#[test]
fn branch_attached_to_cycle_retains_a_closed_ambiguous_walk_and_dangling_trail() {
    let base = square_ribbon();
    let mut points = base.vertices().to_vec();
    let mut faces = base.triangles().to_vec();
    let next = u32::try_from(points.len()).unwrap();
    points.push(Point3::new(-4., 0., 1.));
    faces.push([4, 5, next]);
    let mesh = TriangleMesh::try_new(points, faces).unwrap();
    evaluate(&mesh, 0., |p| {
        assert_eq!(
            (
                p.summary().components,
                p.summary().paths,
                p.summary().closed_walks,
                p.summary().cycle_candidates
            ),
            (1, 2, 1, 0)
        );
        assert!(p
            .paths()
            .iter()
            .all(|path| path.kind == PathKind::AmbiguousTrail
                && path.issues.contains(PathIssue::Branch)));
        let closed = p.paths().iter().position(|path| path.closed).unwrap();
        assert_eq!(p.path_edges(closed).unwrap().len(), 8);
        assert_eq!(p.path_vertices(closed).unwrap()[0], 0);
    });
}
#[test]
fn two_cycles_sharing_a_branch_vertex_remain_two_ambiguous_closed_walks() {
    let mesh = mesh(
        &[
            [0., 0., 0.],
            [4., 0., 0.],
            [0., 4., 0.],
            [-4., 0., 0.],
            [0., -4., 0.],
        ],
        vec![[0, 1, 2], [0, 3, 4]],
    );
    evaluate(&mesh, 0., |p| {
        assert_eq!(
            (
                p.summary().components,
                p.summary().closed_walks,
                p.summary().ambiguous_trails
            ),
            (1, 2, 2)
        );
        assert_eq!(p.summary().cycle_candidates, 0);
    });
}
#[test]
fn unresolved_neighbor_blocks_degree_two_cycle_certification() {
    let mesh = mesh(
        &[
            [0., 0., 0.],
            [4., 0., 0.],
            [4., 4., 0.],
            [0., 4., 0.],
            [-2., 0., 0.],
            [-2., 2., 1.],
        ],
        vec![[0, 1, 2], [0, 2, 3], [0, 4, 5]],
    );
    evaluate(&mesh, 0., |p| {
        assert_eq!(p.summary().candidate_edges, 4);
        assert_eq!(p.summary().cycle_candidates, 0);
        assert_eq!(p.summary().closed_walks, 1);
        assert_eq!(p.paths()[0].kind, PathKind::AmbiguousTrail);
        assert!(p.paths()[0]
            .issues
            .contains(PathIssue::UnresolvedNeighborhood));
        assert_eq!(p.summary().unresolved_edges, 1);
    });
}
#[test]
fn two_uncertain_cycle_vertices_split_into_two_maximal_ambiguous_open_trails() {
    let mesh = mesh(
        &[
            [0., 0., 0.],
            [4., 0., 0.],
            [4., 4., 0.],
            [0., 4., 0.],
            [-2., 0., 0.],
            [-2., 2., 1.],
            [6., 4., 0.],
            [6., 2., 1.],
        ],
        vec![[0, 1, 2], [0, 2, 3], [0, 4, 5], [2, 6, 7]],
    );
    evaluate(&mesh, 0., |p| {
        assert_eq!(p.summary().ambiguous_trails, 2);
        assert_eq!(p.summary().closed_walks, 0);
        assert_eq!(p.path_vertices(0), Some([0, 1, 2].as_slice()));
        assert_eq!(p.path_vertices(1), Some([0, 3, 2].as_slice()));
        assert!(p.paths().iter().all(|path| path.endpoints == Some([0, 2])));
    });
}
#[test]
fn duplicate_parallel_raw_occurrences_stay_unresolved_not_double_walked() {
    let mesh = mesh(
        &[[0., 0., -1.], [4., 0., 1.], [0., 4., 1.]],
        vec![[0, 1, 2], [0, 1, 2], [2, 1, 0]],
    );
    evaluate(&mesh, 0., |p| {
        assert_eq!(p.paths(), []);
        assert_eq!(p.summary().components, 0);
        assert_eq!(p.summary().raw_occurrences, 3);
        assert_eq!(p.edge_outcome(0), Some(EdgeOutcome::Unresolved));
        assert_eq!(p.graph().occurrences(0), Some([0, 1, 2].as_slice()));
        assert_eq!(p.graph().summary().candidate_edges, 0);
    });
}
#[test]
fn self_loop_and_parallel_group_retained_with_collapse_diagnostics_without_traversal() {
    let mesh = mesh(&[[0., 0., 0.], [4., 0., 0.]], vec![[0, 0, 1]]);
    evaluate(&mesh, 0., |p| {
        assert_eq!(p.paths(), []);
        assert_eq!(p.summary().unresolved_edges, 2);
        assert_eq!(p.summary().raw_occurrences, 3);
        assert!(p
            .graph()
            .edges()
            .iter()
            .any(|edge| edge.endpoints[0] == edge.endpoints[1]));
        assert!(p.diagnostics().iter().any(|d|matches!(d,PathDiagnostic::UnresolvedEdge {issues,..} if issues.contains(IncidenceIssue::Collapsed))));
    });
}
#[test]
fn isolated_tangent_remains_a_diagnostic_not_a_fake_path_or_component() {
    let mesh = mesh(&[[0., 0., 0.], [1., 0., 1.], [0., 1., 1.]], vec![[0, 1, 2]]);
    evaluate(&mesh, 0., |p| {
        assert_eq!(p.paths(), []);
        assert_eq!(p.components(), []);
        assert!(matches!(
            p.diagnostics(),
            [PathDiagnostic::Vertex {
                vertex: 0,
                isolated: true,
                ..
            }]
        ));
    });
}
#[test]
fn coincident_disconnected_indexed_loops_are_never_welded() {
    let mesh = mesh(
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
    evaluate(&mesh, 0., |p| {
        assert_eq!(p.summary().cycle_candidates, 2);
        assert_eq!(p.summary().components, 2);
        assert_eq!(
            p.graph()
                .normalized()
                .summary()
                .coincident_feature_positions,
            4
        );
        assert!(p.path_vertices(0).unwrap().iter().all(|&v| v < 4));
        assert!(p.path_vertices(1).unwrap().iter().all(|&v| v >= 4));
    });
}
#[test]
fn geometric_self_crossing_cycle_is_only_a_provisional_graph_candidate() {
    let mesh = ribbon(
        &[[-2., -2.], [2., 2.], [-2., 2.], [2., -2.]],
        &[[0, 1], [1, 2], [2, 3], [3, 0]],
    );
    evaluate(&mesh, 0., |p| {
        assert_eq!(p.summary().cycle_candidates, 1);
        assert_eq!(p.path_vertices(0).unwrap().len(), 8);
        assert_eq!(
            p.graph()
                .normalized()
                .summary()
                .coincident_feature_positions,
            1
        );
    });
}
#[test]
fn overlapping_zero_area_cycle_is_not_promoted_to_polygon_validity() {
    let mesh = ribbon(
        &[[0., 0.], [4., 0.], [1., 0.], [3., 0.]],
        &[[0, 1], [1, 2], [2, 3], [3, 0]],
    );
    evaluate(&mesh, 0., |p| {
        assert_eq!(p.summary().cycle_candidates, 1);
        assert!(p.paths()[0].issues.is_empty());
        assert_eq!(p.path_edges(0).unwrap().len(), 8);
    });
}
#[test]
fn crossing_independent_chain_interiors_do_not_split_or_connect() {
    let mesh = ribbon(
        &[[-2., 0.], [2., 0.], [1., -3.], [1., 1.]],
        &[[0, 1], [2, 3]],
    );
    evaluate(&mesh, 0., |p| {
        assert_eq!(p.summary().open_chains, 2);
        assert_eq!(p.summary().components, 2);
        assert_eq!(p.summary().consumed_edges, 4);
    });
}
#[test]
fn singular_transform_collapse_remains_unresolved_with_no_false_closed_contour() {
    let mesh = square();
    let transform = Affine3::from_scale(Vector3::new(0., 0., 0.)).unwrap();
    let raw = section_mesh_z(
        &mesh,
        0.,
        SectionTransform::Affine(&transform),
        SectionLimits::default(),
    )
    .unwrap();
    let normalized = normalized(&raw);
    let graph = graph(&normalized);
    let paths = extract(&graph);
    accounting(&paths);
    assert_eq!(paths.paths(), []);
    assert_eq!(paths.summary().unresolved_edges, 5);
    assert_eq!(paths.summary().raw_occurrences, 6);
}
#[test]
fn cube_interior_yields_one_eight_edge_provisional_cycle() {
    evaluate(&fixture("fx-01-cube.stl"), 10., |p| {
        assert_eq!(p.summary().cycle_candidates, 1);
        assert_eq!(p.path_edges(0).unwrap().len(), 8);
    });
}
#[test]
fn cube_exact_cap_patch_forms_a_provisional_cycle_and_other_tangencies_remain_context() {
    evaluate(&fixture("fx-01-cube.stl"), 0., |p| {
        assert_eq!(p.summary().cycle_candidates, 1);
        assert_eq!(p.path_edges(0).unwrap().len(), 4);
        assert_eq!(p.summary().internal_edges, 1);
        assert_eq!(p.summary().unresolved_edges, 0);
        assert_eq!(p.summary().raw_occurrences, 10);
    });
}
#[test]
fn cylinder_midplane_retains_one_256_edge_cycle() {
    evaluate(&fixture("fx-02-cylinder.stl"), 10., |p| {
        assert_eq!(p.summary().cycle_candidates, 1);
        assert_eq!(p.path_edges(0).unwrap().len(), 256);
    });
}
#[test]
fn rotated_prism_retains_one_eight_edge_cycle() {
    evaluate(&fixture("fx-05-rotated-prism.stl"), 17., |p| {
        assert_eq!(p.summary().cycle_candidates, 1);
        assert_eq!(p.path_edges(0).unwrap().len(), 8);
    });
}
#[test]
fn open_cube_vertical_section_is_one_open_chain_without_automatic_closure() {
    let mesh = fixture("fx-10a-open.stl");
    // FX-10A removes the top, not a side. Rotate source X into world Z to cut through the opening.
    let transform = Affine3::from_parts(
        [[0., 0., 1.], [0., 1., 0.], [-1., 0., 0.]],
        Vector3::new(0., 0., 0.),
    )
    .unwrap();
    let raw = section_mesh_z(
        &mesh,
        0.,
        SectionTransform::Affine(&transform),
        SectionLimits::default(),
    )
    .unwrap();
    let normalized = normalized(&raw);
    let graph = graph(&normalized);
    let paths = extract(&graph);
    accounting(&paths);
    assert_eq!(paths.summary().open_chains, 1);
    assert!(!paths.paths()[0].closed);
    assert_eq!(paths.path_edges(0).unwrap().len(), 6);
}

#[test]
fn horizontal_open_cube_cycle_does_not_certify_a_closed_source_mesh() {
    evaluate(&fixture("fx-10a-open.stl"), 10., |p| {
        assert_eq!(p.summary().cycle_candidates, 1);
        assert_eq!(p.path_edges(0).unwrap().len(), 8);
    });
}
#[test]
fn canonical_nonmanifold_degenerate_reversed_and_cavity_fixtures_preserve_all_groups() {
    for name in [
        "fx-10b-non-manifold.stl",
        "fx-08-degenerate-faces.stl",
        "fx-09-reversed-face.stl",
        "fx-04-hollow-box.stl",
        "fx-06-multiple-volumes.stl",
        "fx-07-internal-cavity.stl",
        "fx-03-sphere.stl",
    ] {
        let mesh = fixture(name);
        let before = fixture(name);
        for plane in [0., 10., 20.] {
            evaluate(&mesh, plane, |_| {});
        }
        assert_eq!(mesh, before);
    }
}
#[test]
fn source_winding_reversal_preserves_canonical_path_sequences() {
    let first = square_ribbon();
    let second = TriangleMesh::try_new(
        first.vertices().to_vec(),
        first
            .triangles()
            .iter()
            .map(|&[start, middle, end]| [end, middle, start])
            .collect(),
    )
    .unwrap();
    let first_raw = raw(&first, 0.);
    let second_raw = raw(&second, 0.);
    let first_n = normalized(&first_raw);
    let second_n = normalized(&second_raw);
    let first_g = graph(&first_n);
    let second_g = graph(&second_n);
    let first_p = extract(&first_g);
    let second_p = extract(&second_g);
    assert_eq!(first_p.paths(), second_p.paths());
    assert_eq!(first_p.path_vertices(0), second_p.path_vertices(0));
    assert_eq!(first_p.path_edges(0), second_p.path_edges(0));
    assert_eq!(first_p.summary(), second_p.summary());
}
#[test]
fn face_permutation_preserves_paths_and_remaps_every_original_occurrence() {
    let mesh = square();
    let reordered =
        TriangleMesh::try_new(mesh.vertices().to_vec(), vec![[0, 2, 3], [0, 1, 2]]).unwrap();
    let ar = raw(&mesh, 0.);
    let br = raw(&reordered, 0.);
    let an = normalized(&ar);
    let bn = normalized(&br);
    let ag = graph(&an);
    let bg = graph(&bn);
    let ap = extract(&ag);
    let bp = extract(&bg);
    assert_eq!(ap.paths(), bp.paths());
    assert_eq!(ap.path_vertices(0), bp.path_vertices(0));
    assert_eq!(ap.path_edges(0), bp.path_edges(0));
    for edge in 0..ag.edges().len() {
        let original = ag
            .occurrences(edge)
            .unwrap()
            .iter()
            .map(|&id| an.raw().events()[an.segments()[id].event].source_face)
            .collect::<BTreeSet<_>>();
        let mapped = bg
            .occurrences(edge)
            .unwrap()
            .iter()
            .map(|&id| 1 - bn.raw().events()[bn.segments()[id].event].source_face)
            .collect::<BTreeSet<_>>();
        assert_eq!(original, mapped);
    }
}
#[test]
fn deterministic_small_wire_families_conserve_edges_under_face_reordering() {
    for count in 2..18 {
        let first = long_chain(count);
        let second = TriangleMesh::try_new(
            first.vertices().to_vec(),
            first.triangles().iter().rev().copied().collect(),
        )
        .unwrap();
        let ar = raw(&first, 0.);
        let br = raw(&second, 0.);
        let an = normalized(&ar);
        let bn = normalized(&br);
        let ag = graph(&an);
        let bg = graph(&bn);
        let ap = extract(&ag);
        let bp = extract(&bg);
        accounting(&ap);
        accounting(&bp);
        assert_eq!(ap.paths(), bp.paths());
        assert_eq!(ap.path_vertices(0), bp.path_vertices(0));
        assert_eq!(ap.path_edges(0), bp.path_edges(0));
        assert_eq!(
            ap.path_edges(0).unwrap().len(),
            2 * usize::try_from(count - 1).unwrap()
        );
    }
}
#[test]
fn repeated_extraction_preserves_source_raw_normalized_graph_and_bit_payloads() {
    let mesh = square_ribbon();
    let vertices = mesh
        .vertices()
        .iter()
        .map(|p| p.coordinates().map(f64::to_bits))
        .collect::<Vec<_>>();
    let faces = mesh.triangles().to_vec();
    let raw = raw(&mesh, 0.);
    let normalized = normalized(&raw);
    let graph = graph(&normalized);
    let snapshot = format!("{graph:?}");
    let paths = extract(&graph);
    for _ in 0..16 {
        assert_eq!(extract(&graph), paths);
    }
    assert_eq!(format!("{graph:?}"), snapshot);
    assert_eq!(mesh.triangles(), faces);
    assert_eq!(
        mesh.vertices()
            .iter()
            .map(|p| p.coordinates().map(f64::to_bits))
            .collect::<Vec<_>>(),
        vertices
    );
}
#[test]
fn long_chain_has_no_front_insertion_or_missing_edge() {
    evaluate(&long_chain(1500), 0., |p| {
        assert_eq!(p.summary().open_chains, 1);
        assert_eq!(p.path_edges(0).unwrap().len(), 2998);
        assert_eq!(p.path_vertices(0).unwrap().len(), 2999);
    });
}
#[test]
fn exact_output_caps_pass_and_one_below_caps_fail_with_typed_resources() {
    let mesh = square();
    let raw = raw(&mesh, 0.);
    let normalized = normalized(&raw);
    let graph = graph(&normalized);
    let exact = PathLimits {
        max_graph_vertices: 4,
        max_graph_edges: 5,
        max_paths: 1,
        max_path_vertices: 4,
        max_path_edges: 4,
        max_components: 1,
        max_diagnostics: 0,
        max_payload_bytes: usize::MAX,
    };
    assert!(extract_paths(&graph, exact).is_ok());
    for (limits, resource, limit) in [
        (
            PathLimits {
                max_graph_vertices: 3,
                ..exact
            },
            PathResource::GraphVertices,
            3,
        ),
        (
            PathLimits {
                max_graph_edges: 4,
                ..exact
            },
            PathResource::GraphEdges,
            4,
        ),
        (
            PathLimits {
                max_paths: 0,
                ..exact
            },
            PathResource::Paths,
            0,
        ),
        (
            PathLimits {
                max_path_vertices: 3,
                ..exact
            },
            PathResource::PathVertices,
            3,
        ),
        (
            PathLimits {
                max_path_edges: 3,
                ..exact
            },
            PathResource::PathEdges,
            3,
        ),
        (
            PathLimits {
                max_components: 0,
                ..exact
            },
            PathResource::Components,
            0,
        ),
        (
            PathLimits {
                max_payload_bytes: 0,
                ..exact
            },
            PathResource::PayloadBytes,
            0,
        ),
    ] {
        assert_eq!(
            extract_paths(&graph, limits),
            Err(PathError::LimitExceeded { resource, limit })
        );
    }
}
#[test]
fn actual_diagnostic_cap_is_not_its_conservative_capacity_bound() {
    let mesh = star(&[[-2., 0.], [2., 0.], [0., 2.]]);
    let raw = raw(&mesh, 0.);
    let normalized = normalized(&raw);
    let graph = graph(&normalized);
    assert!(extract_paths(
        &graph,
        PathLimits {
            max_diagnostics: 1,
            ..PathLimits::default()
        }
    )
    .is_ok());
    assert_eq!(
        extract_paths(
            &graph,
            PathLimits {
                max_diagnostics: 0,
                ..PathLimits::default()
            }
        ),
        Err(PathError::LimitExceeded {
            resource: PathResource::Diagnostics,
            limit: 0
        })
    );
}
#[test]
fn exact_requested_payload_budget_passes_and_one_byte_less_fails() {
    let mesh = square();
    let raw = raw(&mesh, 0.);
    let normalized = normalized(&raw);
    let graph = graph(&normalized);
    let bytes = extract(&graph).summary().requested_payload_bytes;
    assert!(extract_paths(
        &graph,
        PathLimits {
            max_payload_bytes: bytes,
            ..PathLimits::default()
        }
    )
    .is_ok());
    assert_eq!(
        extract_paths(
            &graph,
            PathLimits {
                max_payload_bytes: bytes - 1,
                ..PathLimits::default()
            }
        ),
        Err(PathError::LimitExceeded {
            resource: PathResource::PayloadBytes,
            limit: bytes - 1
        })
    );
}
#[test]
fn huge_configured_caps_do_not_trigger_speculative_huge_allocations() {
    let mesh = square();
    let raw = raw(&mesh, 0.);
    let normalized = normalized(&raw);
    let graph = graph(&normalized);
    let limits = PathLimits {
        max_graph_vertices: usize::MAX,
        max_graph_edges: usize::MAX,
        max_paths: usize::MAX,
        max_path_vertices: usize::MAX,
        max_path_edges: usize::MAX,
        max_components: usize::MAX,
        max_diagnostics: usize::MAX,
        max_payload_bytes: usize::MAX,
    };
    assert_eq!(extract_paths(&graph, limits).unwrap(), extract(&graph));
}
#[test]
fn zero_path_caps_allow_unresolved_only_groups_without_hiding_occurrences() {
    let mesh = mesh(&[[0., 0., 0.], [4., 0., 0.]], vec![[0, 0, 1]]);
    let raw = raw(&mesh, 0.);
    let normalized = normalized(&raw);
    let graph = graph(&normalized);
    let result = extract_paths(
        &graph,
        PathLimits {
            max_paths: 0,
            max_path_vertices: 0,
            max_path_edges: 0,
            max_components: 0,
            ..PathLimits::default()
        },
    )
    .unwrap();
    accounting(&result);
    assert_eq!(result.summary().raw_occurrences, 3);
}
#[test]
fn pre_cancel_wins_before_input_resource_failure() {
    let mesh = square();
    let raw = raw(&mesh, 0.);
    let normalized = normalized(&raw);
    let graph = graph(&normalized);
    let mut predicate = |_| true;
    assert_eq!(
        extract_paths_with_cancellation(
            &graph,
            PathLimits {
                max_graph_vertices: 0,
                ..PathLimits::default()
            },
            &mut Cancellation::new(&mut predicate)
        ),
        Err(PathError::Cancelled(Cancelled {
            checkpoint: Checkpoint::PathsStart
        }))
    );
}
#[test]
fn cancellation_at_every_phase_preserves_input_and_retry() {
    let mesh = square();
    let raw = raw(&mesh, 0.);
    let normalized = normalized(&raw);
    let graph = graph(&normalized);
    let before = format!("{graph:?}");
    for checkpoint in [
        Checkpoint::PathsInput,
        Checkpoint::PathsStorage,
        Checkpoint::PathsComponents,
        Checkpoint::PathsTraversal,
        Checkpoint::PathsCanonicalization,
        Checkpoint::PathsSorting,
        Checkpoint::PathsDiagnostics,
        Checkpoint::PathsComplete,
    ] {
        let mut predicate = |p| p == checkpoint;
        assert_eq!(
            extract_paths_with_cancellation(
                &graph,
                PathLimits::default(),
                &mut Cancellation::new(&mut predicate)
            ),
            Err(PathError::Cancelled(Cancelled { checkpoint }))
        );
        assert_eq!(format!("{graph:?}"), before);
    }
    assert_eq!(extract(&graph).summary().cycle_candidates, 1);
}
#[test]
fn all_allocation_and_sort_boundaries_are_cancellable() {
    let mesh = square();
    let raw = raw(&mesh, 0.);
    let normalized = normalized(&raw);
    let graph = graph(&normalized);
    for (checkpoint, total) in [
        (Checkpoint::PathsStorage, 16),
        (Checkpoint::PathsSorting, 2),
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
                extract_paths_with_cancellation(
                    &graph,
                    PathLimits::default(),
                    &mut Cancellation::new(&mut predicate)
                ),
                Err(PathError::Cancelled(Cancelled { checkpoint }))
            );
            assert_eq!(seen, selected);
        }
    }
}
#[test]
fn cancellation_inside_long_walk_terminates_without_partial_success() {
    let mesh = long_chain(1000);
    let raw = raw(&mesh, 0.);
    let normalized = normalized(&raw);
    let graph = graph(&normalized);
    let mut polls = 0;
    let mut predicate = |point| {
        if point == Checkpoint::PathsTraversal {
            polls += 1;
        }
        point == Checkpoint::PathsTraversal && polls == 4
    };
    assert_eq!(
        extract_paths_with_cancellation(
            &graph,
            PathLimits::default(),
            &mut Cancellation::new(&mut predicate)
        ),
        Err(PathError::Cancelled(Cancelled {
            checkpoint: Checkpoint::PathsTraversal
        }))
    );
    assert_eq!(polls, 4);
    assert_eq!(extract(&graph).summary().consumed_edges, 1998);
}
#[test]
fn cancellation_inside_long_cycle_canonicalization_prevents_publication() {
    let mesh = long_cycle(300);
    let raw = raw(&mesh, 0.);
    let normalized = normalized(&raw);
    let graph = graph(&normalized);
    let mut polls = 0;
    let mut predicate = |point| {
        if point == Checkpoint::PathsCanonicalization {
            polls += 1;
        }
        point == Checkpoint::PathsCanonicalization && polls == 3
    };
    assert_eq!(
        extract_paths_with_cancellation(
            &graph,
            PathLimits::default(),
            &mut Cancellation::new(&mut predicate)
        ),
        Err(PathError::Cancelled(Cancelled {
            checkpoint: Checkpoint::PathsCanonicalization
        }))
    );
    assert_eq!(polls, 3);
    assert_eq!(extract(&graph).summary().cycle_candidates, 1);
}
#[test]
fn cancellation_inside_high_degree_component_discovery_and_boundary_scans() {
    let tips = (1..=800).map(|id| [f64::from(id), 2.]).collect::<Vec<_>>();
    let mesh = star(&tips);
    let raw = raw(&mesh, 0.);
    let normalized = normalized(&raw);
    let graph = graph(&normalized);
    for checkpoint in [
        Checkpoint::PathsComponents,
        Checkpoint::PathsTraversal,
        Checkpoint::PathsDiagnostics,
    ] {
        let mut polls = 0;
        let mut predicate = |point| {
            if point == checkpoint {
                polls += 1;
            }
            point == checkpoint && polls == 3
        };
        assert_eq!(
            extract_paths_with_cancellation(
                &graph,
                PathLimits::default(),
                &mut Cancellation::new(&mut predicate)
            ),
            Err(PathError::Cancelled(Cancelled { checkpoint }))
        );
    }
    assert_eq!(extract(&graph).summary().ambiguous_trails, 800);
}
#[test]
fn final_publication_cancellation_also_covers_empty_results() {
    let mesh = mesh(&[], vec![]);
    let raw = raw(&mesh, 0.);
    let normalized = normalized(&raw);
    let graph = graph(&normalized);
    let mut predicate = |p| p == Checkpoint::PathsComplete;
    assert_eq!(
        extract_paths_with_cancellation(
            &graph,
            PathLimits::default(),
            &mut Cancellation::new(&mut predicate)
        ),
        Err(PathError::Cancelled(Cancelled {
            checkpoint: Checkpoint::PathsComplete
        }))
    );
}
#[test]
fn never_cancel_wrapper_matches_and_error_causes_remain_structured() {
    let mesh = square();
    let raw = raw(&mesh, 0.);
    let normalized = normalized(&raw);
    let graph = graph(&normalized);
    assert_eq!(
        extract_paths_with_cancellation(&graph, PathLimits::default(), &mut Cancellation::never())
            .unwrap(),
        extract(&graph)
    );
    let error = PathError::Cancelled(Cancelled {
        checkpoint: Checkpoint::PathsComplete,
    });
    assert!(error.source().is_some());
    assert!(error.to_string().contains("PathsComplete"));
    assert!(PathError::Allocation.source().is_none());
    assert_ne!(PathError::SizeOverflow.to_string(), "");
}
