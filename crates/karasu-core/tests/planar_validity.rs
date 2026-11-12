//! Independent path embedding, provenance, resource and synchronous cancellation regressions.
use karasu_core::cancellation::{Cancellation, Cancelled, Checkpoint};
use karasu_core::geometry::Point3;
use karasu_core::mesh::TriangleMesh;
use karasu_core::slicing::graph::{build_incidence_graph, GraphLimits};
use karasu_core::slicing::normalize::{normalize_section, NormalizationLimits};
use karasu_core::slicing::paths::{extract_paths, PathLimits, SectionPaths};
use karasu_core::slicing::validity::*;
use karasu_core::slicing::{section_mesh_z, SectionLimits, SectionTransform};

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
fn cycle(points: &[[f64; 2]]) -> TriangleMesh {
    let count = u32::try_from(points.len()).unwrap();
    ribbon(
        points,
        &(0..count).map(|i| [i, (i + 1) % count]).collect::<Vec<_>>(),
    )
}
fn evaluate(mesh: &TriangleMesh, test: impl FnOnce(&SectionPaths<'_, '_, '_>)) {
    let raw = section_mesh_z(
        mesh,
        0.,
        SectionTransform::Identity,
        SectionLimits::default(),
    )
    .unwrap();
    let normalized = normalize_section(&raw, NormalizationLimits::default()).unwrap();
    let graph = build_incidence_graph(&normalized, GraphLimits::default()).unwrap();
    let paths = extract_paths(&graph, PathLimits::default()).unwrap();
    test(&paths);
}
fn inspect(paths: &SectionPaths<'_, '_, '_>) -> ValiditySummary {
    inspect_planar_paths(paths, ValidityLimits::default())
        .unwrap()
        .summary()
}
fn square() -> TriangleMesh {
    cycle(&[[0., 0.], [4., 0.], [4., 4.], [0., 4.]])
}

#[test]
fn convex_concave_and_collinear_subdivision_cycles_are_only_locally_simple() {
    for points in [
        vec![[0., 0.], [4., 0.], [4., 4.], [0., 4.]],
        vec![[0., 0.], [4., 0.], [2., 1.], [4., 4.], [0., 4.]],
        vec![[0., 0.], [2., 0.], [4., 0.], [4., 4.], [0., 4.]],
    ] {
        evaluate(&cycle(&points), |paths| {
            let report = inspect_planar_paths(paths, ValidityLimits::default()).unwrap();
            assert_eq!(report.embeddings().len(), 1);
            assert_eq!(report.embeddings()[0].status, EmbeddingStatus::SimpleCycle);
            assert_eq!(report.diagnostics(), []);
            let edges = paths.summary().consumed_edges;
            assert_eq!(report.summary().examined_pairs, edges * (edges - 1) / 2);
            assert!(std::ptr::eq(report.paths(), paths));
        });
    }
}
#[test]
fn zero_and_nonzero_area_crossings_and_backtracking_are_not_simple() {
    for points in [
        vec![[0., 0.], [4., 4.], [0., 4.], [4., 0.]],
        vec![[0., 0.], [4., 3.], [0., 3.], [2., 0.]],
        vec![[0., 0.], [4., 0.], [2., 0.], [2., 3.], [0., 3.]],
    ] {
        evaluate(&cycle(&points), |paths| {
            let report = inspect_planar_paths(paths, ValidityLimits::default()).unwrap();
            assert_eq!(
                report.embeddings()[0].status,
                EmbeddingStatus::NonSimpleCycle
            );
            assert!(report.embeddings()[0].self_contacts > 0);
            assert_ne!(report.diagnostics(), []);
        });
    }
}
#[test]
fn open_and_branch_ambiguous_paths_never_become_rings() {
    for links in [vec![[0, 1], [1, 2]], vec![[0, 1], [1, 2], [1, 3]]] {
        evaluate(
            &ribbon(&[[0., 0.], [2., 0.], [4., 0.], [2., 3.]], &links),
            |paths| {
                let report = inspect_planar_paths(paths, ValidityLimits::default()).unwrap();
                assert!(report
                    .embeddings()
                    .iter()
                    .all(|p| p.status == EmbeddingStatus::Ineligible));
                if links.len() == 3 {
                    assert!(report.summary().cross_path_contacts > 0);
                }
            },
        );
    }
}
#[test]
fn distinct_simple_cycles_keep_cross_path_contacts_separate() {
    // Two disjoint source-feature loops, tested with disjoint/touch/cross/coincident XY embeddings.
    for shift in [[8., 0.], [4., 0.], [2., 1.], [0., 0.]] {
        let points = [[0., 0.], [4., 0.], [4., 4.], [0., 4.]]
            .into_iter()
            .chain(
                [[0., 0.], [4., 0.], [4., 4.], [0., 4.]].map(|[x, y]| [x + shift[0], y + shift[1]]),
            )
            .collect::<Vec<_>>();
        evaluate(
            &ribbon(
                &points,
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
            ),
            |paths| {
                let report = inspect_planar_paths(paths, ValidityLimits::default()).unwrap();
                assert_eq!(report.embeddings().len(), 2);
                assert!(report
                    .embeddings()
                    .iter()
                    .all(|p| p.status == EmbeddingStatus::SimpleCycle));
                assert_eq!(report.summary().cross_path_contacts == 0, shift[0] > 4.);
                for finding in report.diagnostics() {
                    if let PlanarDiagnostic::Contact { first, second, .. } = finding {
                        assert_ne!(first.path, second.path);
                    }
                }
            },
        );
    }
}
#[test]
fn complete_only_collapse_duplicates_and_tangencies_remain_unassessed() {
    let mesh = TriangleMesh::try_new(
        vec![
            Point3::new(0., 0., 0.),
            Point3::new(1., 0., 1.),
            Point3::new(0., 1., 1.),
        ],
        vec![[0, 1, 2]],
    )
    .unwrap();
    evaluate(&mesh, |paths| {
        let report = inspect_planar_paths(paths, ValidityLimits::default()).unwrap();
        assert_eq!(report.embeddings(), []);
        assert_eq!(report.summary().unassessed_isolated_vertices, 1);
        assert_eq!(report.summary().examined_pairs, 0);
    });
    let mesh = TriangleMesh::try_new(
        vec![
            Point3::new(0., 0., 0.),
            Point3::new(2., 0., 0.),
            Point3::new(0., 2., 0.),
        ],
        vec![[0, 1, 2], [0, 1, 2]],
    )
    .unwrap();
    evaluate(&mesh, |paths| {
        let report = inspect_planar_paths(paths, ValidityLimits::default()).unwrap();
        assert!(report.summary().unassessed_unresolved_edges > 0);
        assert_eq!(report.embeddings(), []);
    });
    let mesh = TriangleMesh::try_new(
        vec![
            Point3::new(0., 0., 0.),
            Point3::new(4., 0., 0.),
            Point3::new(4., 4., 0.),
            Point3::new(0., 4., 0.),
        ],
        vec![[0, 1, 2], [0, 2, 3]],
    )
    .unwrap();
    evaluate(&mesh, |paths| {
        assert_eq!(inspect(paths).unassessed_internal_edges, 1);
    });
}
#[test]
fn every_input_and_tested_pair_cap_accepts_exact_boundary_and_rejects_one_below() {
    evaluate(&square(), |paths| {
        let summary = inspect(paths);
        let counts = [
            paths.graph().vertices().len(),
            paths.graph().edges().len(),
            paths.paths().len(),
            paths.summary().path_vertices,
            summary.path_edges,
            summary.tested_pairs,
        ];
        // Every pair is resolved; only box-overlapping pairs need an exact test and count
        // against the pair budget.
        assert_eq!(summary.examined_pairs, 28);
        assert!(summary.tested_pairs < summary.examined_pairs);
        let resources = [
            ValidityResource::GraphVertices,
            ValidityResource::GraphEdges,
            ValidityResource::Paths,
            ValidityResource::PathVertices,
            ValidityResource::PathEdges,
            ValidityResource::Pairs,
        ];
        for (i, (&count, &resource)) in counts.iter().zip(&resources).enumerate() {
            for delta in [0, 1] {
                let mut limits = ValidityLimits::default();
                match i {
                    0 => limits.max_graph_vertices = count - delta,
                    1 => limits.max_graph_edges = count - delta,
                    2 => limits.max_paths = count - delta,
                    3 => limits.max_path_vertices = count - delta,
                    4 => limits.max_path_edges = count - delta,
                    _ => limits.max_pairs = count - delta,
                }
                let result = inspect_planar_paths(paths, limits);
                if delta == 0 {
                    assert!(result.is_ok());
                } else {
                    assert_eq!(
                        result.unwrap_err(),
                        ValidityError::LimitExceeded {
                            resource,
                            limit: count - 1
                        }
                    );
                }
            }
        }
        for delta in [0, 1] {
            let limits = ValidityLimits {
                max_payload_bytes: summary.requested_payload_bytes - delta,
                ..ValidityLimits::default()
            };
            let result = inspect_planar_paths(paths, limits);
            if delta == 0 {
                assert!(result.is_ok());
            } else {
                assert!(matches!(
                    result,
                    Err(ValidityError::LimitExceeded {
                        resource: ValidityResource::PayloadBytes,
                        ..
                    })
                ));
            }
        }
    });
}
#[test]
fn diagnostics_and_growth_bytes_are_bounded_without_partial_success() {
    let mut base_bytes = 0;
    evaluate(&square(), |paths| {
        base_bytes = inspect(paths).requested_payload_bytes;
    });
    evaluate(&cycle(&[[0., 0.], [4., 3.], [0., 3.], [2., 0.]]), |paths| {
        let baseline = inspect_planar_paths(paths, ValidityLimits::default()).unwrap();
        let count = baseline.diagnostics().len();
        assert!(count > 0);
        let exact = ValidityLimits {
            max_diagnostics: count,
            max_payload_bytes: baseline.summary().requested_payload_bytes,
            ..ValidityLimits::default()
        };
        assert_eq!(inspect_planar_paths(paths, exact).unwrap(), baseline);
        assert_eq!(
            inspect_planar_paths(
                paths,
                ValidityLimits {
                    max_diagnostics: count - 1,
                    ..exact
                }
            )
            .unwrap_err(),
            ValidityError::LimitExceeded {
                resource: ValidityResource::Diagnostics,
                limit: count - 1
            }
        );
        let minimum = base_bytes + std::mem::size_of_val(baseline.diagnostics());
        let tight = ValidityLimits {
            max_payload_bytes: minimum,
            ..exact
        };
        assert_eq!(
            inspect_planar_paths(paths, tight).unwrap().diagnostics(),
            baseline.diagnostics()
        );
        assert_eq!(
            inspect_planar_paths(
                paths,
                ValidityLimits {
                    max_payload_bytes: minimum - 1,
                    ..tight
                }
            )
            .unwrap_err(),
            ValidityError::LimitExceeded {
                resource: ValidityResource::PayloadBytes,
                limit: minimum - 1
            }
        );
        assert!(matches!(
            inspect_planar_paths(
                paths,
                ValidityLimits {
                    max_diagnostics: 0,
                    ..exact
                }
            ),
            Err(ValidityError::LimitExceeded {
                resource: ValidityResource::Diagnostics,
                ..
            })
        ));
    });
}
#[test]
fn deterministic_cancellation_all_phases_preserves_input_and_retry() {
    evaluate(&cycle(&[[0., 0.], [4., 3.], [0., 3.], [2., 0.]]), |paths| {
        let baseline = inspect_planar_paths(paths, ValidityLimits::default()).unwrap();
        let before = format!("{paths:?}");
        for phase in [
            Checkpoint::ValidityStart,
            Checkpoint::ValidityInput,
            Checkpoint::ValidityStorage,
            Checkpoint::ValidityPairs,
            Checkpoint::ValidityDiagnostics,
            Checkpoint::ValidityComplete,
        ] {
            let mut predicate = |point| point == phase;
            let result = inspect_planar_paths_with_cancellation(
                paths,
                ValidityLimits::default(),
                &mut Cancellation::new(&mut predicate),
            );
            assert_eq!(
                result.unwrap_err(),
                ValidityError::Cancelled(Cancelled { checkpoint: phase })
            );
            assert_eq!(
                inspect_planar_paths(paths, ValidityLimits::default()).unwrap(),
                baseline
            );
            assert_eq!(format!("{paths:?}"), before);
        }
        let mut never = |_| false;
        assert_eq!(
            inspect_planar_paths_with_cancellation(
                paths,
                ValidityLimits::default(),
                &mut Cancellation::new(&mut never)
            )
            .unwrap(),
            baseline
        );
    });
}
#[test]
fn inner_pair_checkpoint_and_diagnostic_allocation_boundaries_are_observable() {
    let points = (0..40).map(|i| [f64::from(i), 0.]).collect::<Vec<_>>();
    let links = (0..39).map(|i| [i, i + 1]).collect::<Vec<_>>();
    evaluate(&ribbon(&points, &links), |paths| {
        let mut seen = 0;
        let mut predicate = |point| {
            if point == Checkpoint::ValidityPairs {
                seen += 1;
            }
            seen == 3
        };
        assert!(matches!(
            inspect_planar_paths_with_cancellation(
                paths,
                ValidityLimits::default(),
                &mut Cancellation::new(&mut predicate)
            ),
            Err(ValidityError::Cancelled(Cancelled {
                checkpoint: Checkpoint::ValidityPairs
            }))
        ));
        assert_eq!(seen, 3); // Collapse pass at zero; pair scan at zero and 256 (inside first row).
    });
    evaluate(&cycle(&[[0., 0.], [4., 3.], [0., 3.], [2., 0.]]), |paths| {
        let mut trace = Vec::new();
        let mut predicate = |point| {
            trace.push(point);
            false
        };
        inspect_planar_paths_with_cancellation(
            paths,
            ValidityLimits::default(),
            &mut Cancellation::new(&mut predicate),
        )
        .unwrap();
        let count = trace
            .iter()
            .filter(|&&p| p == Checkpoint::ValidityStorage)
            .count();
        for target in 1..=count {
            let mut seen = 0;
            let mut predicate = |point| {
                if point == Checkpoint::ValidityStorage {
                    seen += 1;
                }
                seen == target
            };
            assert!(matches!(
                inspect_planar_paths_with_cancellation(
                    paths,
                    ValidityLimits::default(),
                    &mut Cancellation::new(&mut predicate)
                ),
                Err(ValidityError::Cancelled(Cancelled {
                    checkpoint: Checkpoint::ValidityStorage
                }))
            ));
        }
    });
}
#[test]
fn empty_input_final_cancellation_and_precedence() {
    evaluate(&TriangleMesh::try_new(vec![], vec![]).unwrap(), |paths| {
        let limits = ValidityLimits {
            max_graph_vertices: 0,
            max_graph_edges: 0,
            max_paths: 0,
            max_path_vertices: 0,
            max_path_edges: 0,
            max_pairs: 0,
            max_diagnostics: 0,
            max_payload_bytes: 0,
        };
        assert_eq!(
            inspect_planar_paths(paths, limits).unwrap().summary(),
            ValiditySummary::default()
        );
        let mut predicate = |phase| phase == Checkpoint::ValidityComplete;
        assert!(matches!(
            inspect_planar_paths_with_cancellation(
                paths,
                limits,
                &mut Cancellation::new(&mut predicate)
            ),
            Err(ValidityError::Cancelled(_))
        ));
    });
    evaluate(&square(), |paths| {
        let limits = ValidityLimits {
            max_graph_vertices: 0,
            ..ValidityLimits::default()
        };
        let mut predicate = |_| true;
        assert_eq!(
            inspect_planar_paths_with_cancellation(
                paths,
                limits,
                &mut Cancellation::new(&mut predicate)
            )
            .unwrap_err(),
            ValidityError::Cancelled(Cancelled {
                checkpoint: Checkpoint::ValidityStart
            })
        );
        let mut predicate = |phase| phase == Checkpoint::ValidityComplete;
        assert!(matches!(
            inspect_planar_paths_with_cancellation(
                paths,
                limits,
                &mut Cancellation::new(&mut predicate)
            ),
            Err(ValidityError::LimitExceeded {
                resource: ValidityResource::GraphVertices,
                ..
            })
        ));
    });
}

#[test]
fn face_order_and_winding_do_not_change_geometric_findings_or_mutate_source() {
    let mesh = cycle(&[[0., 0.], [4., 3.], [0., 3.], [2., 0.]]);
    let original = format!("{mesh:?}");
    let mut baseline = None;
    evaluate(&mesh, |paths| {
        let report = inspect_planar_paths(paths, ValidityLimits::default()).unwrap();
        baseline = Some((
            report.embeddings().to_vec(),
            report.diagnostics().to_vec(),
            report.summary(),
        ));
    });
    let mut faces = mesh.triangles().to_vec();
    faces.reverse();
    for face in &mut faces {
        face.swap(0, 2);
    }
    let reordered = TriangleMesh::try_new(mesh.vertices().to_vec(), faces).unwrap();
    evaluate(&reordered, |paths| {
        let report = inspect_planar_paths(paths, ValidityLimits::default()).unwrap();
        assert_eq!(
            Some((
                report.embeddings().to_vec(),
                report.diagnostics().to_vec(),
                report.summary()
            )),
            baseline
        );
    });
    assert_eq!(format!("{mesh:?}"), original);
}

#[test]
fn canonical_fixture_paths_retain_all_unassessed_context_and_repeat_exactly() {
    use karasu_core::mesh::stl::{import_stl_path, StlOptions, StlUnitPolicy};
    let root =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/reference/stl");
    let mut files = std::fs::read_dir(root)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| {
            // FX-11 (generated) and FX-12 (owner-supplied) are large local-only fixtures.
            let name = p.file_name().unwrap().to_string_lossy();
            p.extension().is_some_and(|s| s == "stl")
                && !name.starts_with("fx-11")
                && !name.starts_with("fx-12")
        })
        .collect::<Vec<_>>();
    files.sort();
    assert_eq!(files.len(), 11);
    for file in files {
        let mesh = import_stl_path(file, StlOptions::new(StlUnitPolicy::AssumeMillimeters))
            .unwrap()
            .mesh;
        evaluate(&mesh, |paths| {
            let first = inspect_planar_paths(paths, ValidityLimits::default()).unwrap();
            assert_eq!(
                inspect_planar_paths(paths, ValidityLimits::default()).unwrap(),
                first
            );
            assert_eq!(
                first.summary().unassessed_internal_edges,
                paths.summary().internal_edges
            );
            assert_eq!(
                first.summary().unassessed_unresolved_edges,
                paths.summary().unresolved_edges
            );
            assert_eq!(first.summary().path_edges, paths.summary().consumed_edges);
        });
    }
}
