//! Owned analytic geometry, provenance, resource and cooperative-cancellation area regressions.
use karasu_core::cancellation::{Cancellation, Cancelled, Checkpoint};
use karasu_core::geometry::Point3;
use karasu_core::mesh::TriangleMesh;
use karasu_core::slicing::area::*;
use karasu_core::slicing::graph::{build_incidence_graph, GraphLimits};
use karasu_core::slicing::normalize::{
    normalize_section, CoordinateAgreement, NormalizationLimits,
};
use karasu_core::slicing::paths::{extract_paths, PathLimits};
use karasu_core::slicing::validity::{inspect_planar_paths, PlanarInspection, ValidityLimits};
use karasu_core::slicing::{section_mesh_z, SectionLimits, SectionTransform};

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
fn cycle(points: &[[f64; 2]]) -> TriangleMesh {
    let n = u32::try_from(points.len()).unwrap();
    ribbon(
        points,
        &(0..n).map(|i| [i, (i + 1) % n]).collect::<Vec<_>>(),
    )
}
fn triangle(points: [[f64; 2]; 3]) -> TriangleMesh {
    TriangleMesh::try_new(
        points.map(|[x, y]| Point3::new(x, y, 0.)).to_vec(),
        vec![[0, 1, 2]],
    )
    .unwrap()
}
fn square() -> TriangleMesh {
    cycle(&[[0., 0.], [4., 0.], [4., 4.], [0., 4.]])
}
fn evaluate(mesh: &TriangleMesh, test: impl FnOnce(&PlanarInspection<'_, '_, '_, '_>)) {
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
    let inspection = inspect_planar_paths(&paths, ValidityLimits::default()).unwrap();
    test(&inspection);
}
fn measure<'i, 'p, 'g, 'n, 'r>(
    input: &'i PlanarInspection<'p, 'g, 'n, 'r>,
) -> SectionAreas<'i, 'p, 'g, 'n, 'r> {
    measure_cycle_areas(input, AreaLimits::default()).unwrap()
}
fn conservation(areas: &SectionAreas<'_, '_, '_, '_, '_>) {
    let s = areas.summary();
    assert_eq!(s.paths, areas.outcomes().len());
    assert_eq!(
        s.paths,
        s.measured + s.skipped_open + s.skipped_ambiguous + s.skipped_non_simple
    );
    assert_eq!(s.measured, s.finite + s.rounded_to_zero + s.overflow);
    assert_eq!(
        s.path_edges,
        areas.inspection().paths().summary().consumed_edges
    );
    assert_eq!(
        s.path_vertices,
        areas.inspection().paths().summary().path_vertices
    );
    assert_eq!(
        s.requested_payload_bytes,
        std::mem::size_of_val(areas.outcomes())
    );
}

// Only used on small dyadic coordinates whose products/sum are all exactly representable.
fn traversal_orientation(input: &PlanarInspection<'_, '_, '_, '_>, id: usize) -> CycleOrientation {
    let graph = input.paths().graph();
    let points = input
        .paths()
        .path_vertices(id)
        .unwrap()
        .iter()
        .map(|&v| {
            let CoordinateAgreement::Agreed(p) =
                graph.normalized().features()[graph.vertices()[v].feature_group].coordinates
            else {
                panic!("expected agreed coordinates")
            };
            p
        })
        .collect::<Vec<_>>();
    let sum = points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .take(points.len())
        .map(|(a, b)| a.x * b.y - a.y * b.x)
        .sum::<f64>();
    assert!(sum.abs() > 0.);
    if sum > 0. {
        CycleOrientation::Counterclockwise
    } else {
        CycleOrientation::Clockwise
    }
}

#[test]
fn analytic_triangle_convex_concave_and_subdivided_cycles_measure_stored_walk() {
    for (mesh, expected) in [
        (triangle([[0., 0.], [4., 0.], [0., 3.]]), 6.),
        (square(), 16.),
        (
            cycle(&[[0., 0.], [4., 0.], [2., 1.], [4., 4.], [0., 4.]]),
            12.,
        ),
        (
            cycle(&[[0., 0.], [2., 0.], [4., 0.], [4., 4.], [0., 4.]]),
            16.,
        ),
    ] {
        let source = format!("{mesh:?}");
        evaluate(&mesh, |input| {
            let before = format!("{input:?}");
            let areas = measure(input);
            assert!(std::ptr::eq(areas.inspection(), input));
            assert_eq!(
                areas.outcomes(),
                [PathAreaOutcome::Measured {
                    orientation: traversal_orientation(input, 0),
                    magnitude: AreaMagnitude::Finite(expected)
                }]
            );
            assert_eq!(
                areas.summary().assessed_terms,
                input.paths().path_edges(0).unwrap().len()
            );
            conservation(&areas);
            assert_eq!(measure(input), areas);
            assert_eq!(format!("{input:?}"), before);
        });
        assert_eq!(format!("{mesh:?}"), source);
    }
}

#[test]
fn real_coplanar_range_inputs_preserve_exact_orientation_without_zero_or_infinity_scalars() {
    for (e, magnitude) in [
        (-600, AreaMagnitude::RoundedToZero),
        (-537, AreaMagnitude::RoundedToZero),
        (-536, AreaMagnitude::Finite(f64::from_bits(2))),
        (0, AreaMagnitude::Finite(0.5)),
        (600, AreaMagnitude::Overflow),
    ] {
        let side = 2.0_f64.powi(e);
        for reflected in [false, true] {
            evaluate(
                &triangle([
                    [0., -0.],
                    [if reflected { -side } else { side }, 0.],
                    [0., side],
                ]),
                |input| {
                    let areas = measure(input);
                    assert_eq!(
                        areas.outcomes()[0],
                        PathAreaOutcome::Measured {
                            orientation: if reflected {
                                CycleOrientation::Clockwise
                            } else {
                                CycleOrientation::Counterclockwise
                            },
                            magnitude
                        }
                    );
                    // Canonical vertex identities are unchanged; reflection reverses their actual walk.
                    let first = input.paths().path_vertices(0).unwrap();
                    assert_eq!(first.len(), 3);
                    assert_eq!(areas.summary().measured, 1);
                    conservation(&areas);
                },
            );
        }
    }
}

#[test]
fn represented_product_cancellation_remains_positive_through_the_public_pipeline() {
    let d = f64::EPSILON;
    evaluate(&triangle([[0., 0.], [1., 1. + d], [1. - d, 1.]]), |input| {
        let areas = measure(input);
        assert!(
            matches!(areas.outcomes(), [PathAreaOutcome::Measured { magnitude: AreaMagnitude::Finite(a), .. }] if a.to_bits() == 2.0_f64.powi(-105).to_bits())
        );
    });
}

#[test]
fn open_ambiguous_and_non_simple_paths_are_explicitly_skipped_without_closure() {
    for (mesh, reason) in [
        (
            ribbon(&[[0., 0.], [2., 0.], [4., 0.]], &[[0, 1], [1, 2]]),
            AreaSkipReason::OpenPath,
        ),
        (
            ribbon(
                &[[0., 0.], [2., 0.], [4., 0.], [2., 3.]],
                &[[0, 1], [1, 2], [1, 3]],
            ),
            AreaSkipReason::AmbiguousPath,
        ),
        (
            cycle(&[[0., 0.], [4., 4.], [0., 4.], [4., 0.]]),
            AreaSkipReason::NonSimpleCycle,
        ),
        (
            cycle(&[[0., 0.], [4., 3.], [0., 3.], [2., 0.]]),
            AreaSkipReason::NonSimpleCycle,
        ),
        (
            cycle(&[[0., 0.], [4., 0.], [2., 0.], [2., 3.], [0., 3.]]),
            AreaSkipReason::NonSimpleCycle,
        ),
    ] {
        evaluate(&mesh, |input| {
            let areas = measure_cycle_areas(
                input,
                AreaLimits {
                    max_terms: 0,
                    ..AreaLimits::default()
                },
            )
            .unwrap();
            assert_ne!(areas.outcomes(), []);
            assert!(areas
                .outcomes()
                .iter()
                .all(|p| *p == PathAreaOutcome::Skipped(reason)));
            assert_eq!(areas.summary().assessed_terms, 0);
            conservation(&areas);
        });
    }
}

#[test]
fn collapsed_source_feature_context_is_not_repaired_into_a_measured_cycle() {
    let mesh = cycle(&[[0., 0.], [0., 0.], [2., 0.], [0., 2.]]);
    evaluate(&mesh, |input| {
        let before = format!("{input:?}");
        let areas = measure(input);
        assert_eq!(areas.summary().measured, 0);
        assert!(input.summary().unassessed_unresolved_edges > 0 || !input.diagnostics().is_empty());
        assert_eq!(format!("{input:?}"), before);
        conservation(&areas);
    });
}

#[test]
fn separate_cycles_retain_contacts_coincidence_and_nesting_without_material_roles() {
    for (other, area) in [
        ([[8., 0.], [12., 0.], [12., 4.], [8., 4.]], 16.),
        ([[4., 0.], [8., 0.], [8., 4.], [4., 4.]], 16.),
        ([[2., 1.], [6., 1.], [6., 5.], [2., 5.]], 16.),
        ([[0., 0.], [4., 0.], [4., 4.], [0., 4.]], 16.),
        ([[1., 1.], [3., 1.], [3., 3.], [1., 3.]], 4.),
    ] {
        let points = [[0., 0.], [4., 0.], [4., 4.], [0., 4.]]
            .into_iter()
            .chain(other)
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
            |input| {
                let before = format!("{input:?}");
                let areas = measure(input);
                assert_eq!(areas.summary().measured, 2);
                for (id, expected) in [16., area].into_iter().enumerate() {
                    assert_eq!(
                        areas.outcomes()[id],
                        PathAreaOutcome::Measured {
                            orientation: traversal_orientation(input, id),
                            magnitude: AreaMagnitude::Finite(expected)
                        }
                    );
                }
                assert!(std::ptr::eq(
                    areas.inspection().diagnostics(),
                    input.diagnostics()
                ));
                assert_eq!(format!("{input:?}"), before);
                conservation(&areas);
            },
        );
    }
}

#[test]
fn complete_only_internal_unresolved_and_isolated_context_remains_borrowed() {
    for mesh in [
        TriangleMesh::try_new(
            vec![
                Point3::new(0., 0., 0.),
                Point3::new(1., 0., 1.),
                Point3::new(0., 1., 1.),
            ],
            vec![[0, 1, 2]],
        )
        .unwrap(),
        TriangleMesh::try_new(
            vec![
                Point3::new(0., 0., 0.),
                Point3::new(2., 0., 0.),
                Point3::new(0., 2., 0.),
            ],
            vec![[0, 1, 2], [0, 1, 2]],
        )
        .unwrap(),
        TriangleMesh::try_new(
            vec![
                Point3::new(0., 0., 0.),
                Point3::new(4., 0., 0.),
                Point3::new(4., 4., 0.),
                Point3::new(0., 4., 0.),
            ],
            vec![[0, 1, 2], [0, 2, 3]],
        )
        .unwrap(),
    ] {
        evaluate(&mesh, |input| {
            let areas = measure(input);
            assert!(std::ptr::eq(areas.inspection(), input));
            assert_eq!(areas.inspection().summary(), input.summary());
            let s = input.summary();
            assert!(
                s.unassessed_internal_edges
                    + s.unassessed_unresolved_edges
                    + s.unassessed_isolated_vertices
                    > 0
            );
            conservation(&areas);
        });
    }
}

#[test]
fn independent_limits_accept_exact_and_reject_one_below_including_skipped_metadata() {
    for mesh in [square(), ribbon(&[[0., 0.], [4., 0.]], &[[0, 1]])] {
        evaluate(&mesh, |input| {
            let base = measure(input);
            let s = base.summary();
            for (resource, count) in [
                (AreaResource::Paths, s.paths),
                (AreaResource::PathVertices, s.path_vertices),
                (AreaResource::PathEdges, s.path_edges),
                (AreaResource::Terms, s.assessed_terms),
                (AreaResource::PayloadBytes, s.requested_payload_bytes),
            ] {
                for delta in 0..=usize::from(count != 0) {
                    let mut limits = AreaLimits::default();
                    match resource {
                        AreaResource::Paths => limits.max_paths = count - delta,
                        AreaResource::PathVertices => limits.max_path_vertices = count - delta,
                        AreaResource::PathEdges => limits.max_path_edges = count - delta,
                        AreaResource::Terms => limits.max_terms = count - delta,
                        AreaResource::PayloadBytes => limits.max_payload_bytes = count - delta,
                    }
                    let mut stored = false;
                    let mut predicate = |p| {
                        if p == Checkpoint::AreaStorage {
                            stored = true;
                        }
                        false
                    };
                    let result = measure_cycle_areas_with_cancellation(
                        input,
                        limits,
                        &mut Cancellation::new(&mut predicate),
                    );
                    if delta == 0 {
                        assert_eq!(result.unwrap(), base);
                    } else {
                        assert!(!stored);
                        assert_eq!(
                            result.unwrap_err(),
                            AreaError::LimitExceeded {
                                resource,
                                limit: count - 1
                            }
                        );
                    }
                }
            }
        });
    }
}

#[test]
fn every_observed_checkpoint_can_cancel_atomically_and_retry_without_mutation() {
    evaluate(&square(), |input| {
        let before = format!("{input:?}");
        let base = measure(input);
        let mut trace = Vec::new();
        let mut predicate = |p| {
            trace.push(p);
            false
        };
        assert_eq!(
            measure_cycle_areas_with_cancellation(
                input,
                AreaLimits::default(),
                &mut Cancellation::new(&mut predicate)
            )
            .unwrap(),
            base
        );
        for phase in [
            Checkpoint::AreaStart,
            Checkpoint::AreaInput,
            Checkpoint::AreaStorage,
            Checkpoint::AreaTerms,
            Checkpoint::AreaRounding,
            Checkpoint::AreaRecords,
            Checkpoint::AreaComplete,
        ] {
            assert!(trace.contains(&phase));
        }
        assert_eq!(
            trace
                .iter()
                .filter(|&&p| p == Checkpoint::AreaStorage)
                .count(),
            2
        );
        assert_eq!(
            trace
                .iter()
                .filter(|&&p| p == Checkpoint::AreaRounding)
                .count(),
            2
        );
        for (target, &checkpoint) in trace.iter().enumerate() {
            let mut count = 0;
            let mut predicate = |_| {
                let stop = count == target;
                count += 1;
                stop
            };
            let error = measure_cycle_areas_with_cancellation(
                input,
                AreaLimits::default(),
                &mut Cancellation::new(&mut predicate),
            )
            .unwrap_err();
            assert_eq!(error, AreaError::Cancelled(Cancelled { checkpoint }));
            assert!(std::error::Error::source(&error).is_some());
            assert_eq!(measure(input), base);
            assert_eq!(format!("{input:?}"), before);
        }
    });
}

#[test]
fn long_cycle_polls_inside_accumulation_at_global_256_term_cadence() {
    let mut points = (0..260).map(|x| [f64::from(x), 0.]).collect::<Vec<_>>();
    points.extend([[259., 4.], [0., 4.]]);
    evaluate(&cycle(&points), |input| {
        let base = measure(input);
        let terms = base.summary().assessed_terms;
        assert!(terms > 512);
        let mut calls = 0;
        let mut predicate = |p| {
            if p == Checkpoint::AreaTerms {
                calls += 1;
            }
            false
        };
        measure_cycle_areas_with_cancellation(
            input,
            AreaLimits::default(),
            &mut Cancellation::new(&mut predicate),
        )
        .unwrap();
        assert_eq!(calls, terms.div_ceil(256));
        let mut calls = 0;
        let mut rounded = false;
        let mut predicate = |p| {
            if p == Checkpoint::AreaRounding {
                rounded = true;
            }
            if p == Checkpoint::AreaTerms {
                calls += 1;
            }
            p == Checkpoint::AreaTerms && calls == 2
        };
        assert_eq!(
            measure_cycle_areas_with_cancellation(
                input,
                AreaLimits::default(),
                &mut Cancellation::new(&mut predicate)
            )
            .unwrap_err(),
            AreaError::Cancelled(Cancelled {
                checkpoint: Checkpoint::AreaTerms
            })
        );
        assert!(!rounded);
        assert_eq!(measure(input), base);
    });
}

#[test]
fn empty_and_all_skipped_results_poll_final_and_errors_keep_checkpoint_precedence() {
    for mesh in [
        TriangleMesh::try_new(vec![], vec![]).unwrap(),
        ribbon(&[[0., 0.], [2., 0.]], &[[0, 1]]),
    ] {
        evaluate(&mesh, |input| {
            let mut predicate = |p| p == Checkpoint::AreaComplete;
            assert_eq!(
                measure_cycle_areas_with_cancellation(
                    input,
                    AreaLimits::default(),
                    &mut Cancellation::new(&mut predicate)
                )
                .unwrap_err(),
                AreaError::Cancelled(Cancelled {
                    checkpoint: Checkpoint::AreaComplete
                })
            );
            if input.paths().paths().is_empty() {
                let limits = AreaLimits {
                    max_paths: 0,
                    max_path_vertices: 0,
                    max_path_edges: 0,
                    max_terms: 0,
                    max_payload_bytes: 0,
                };
                assert_eq!(
                    measure_cycle_areas(input, limits).unwrap().summary(),
                    AreaSummary::default()
                );
            } else {
                let limits = AreaLimits {
                    max_paths: 0,
                    ..AreaLimits::default()
                };
                let mut predicate = |_| true;
                assert_eq!(
                    measure_cycle_areas_with_cancellation(
                        input,
                        limits,
                        &mut Cancellation::new(&mut predicate)
                    )
                    .unwrap_err(),
                    AreaError::Cancelled(Cancelled {
                        checkpoint: Checkpoint::AreaStart
                    })
                );
                let mut predicate = |p| p == Checkpoint::AreaComplete;
                assert_eq!(
                    measure_cycle_areas_with_cancellation(
                        input,
                        limits,
                        &mut Cancellation::new(&mut predicate)
                    )
                    .unwrap_err(),
                    AreaError::LimitExceeded {
                        resource: AreaResource::Paths,
                        limit: 0
                    }
                );
            }
        });
    }
}

#[test]
fn inner_term_indices_continue_across_many_short_cycles() {
    let mut vertices = Vec::new();
    let mut triangles = Vec::new();
    for i in 0..100_u32 {
        let x = f64::from(i) * 4.;
        vertices.extend([
            Point3::new(x, 0., 0.),
            Point3::new(x + 2., 0., 0.),
            Point3::new(x, 2., 0.),
        ]);
        triangles.push([3 * i, 3 * i + 1, 3 * i + 2]);
    }
    let mesh = TriangleMesh::try_new(vertices, triangles).unwrap();
    evaluate(&mesh, |input| {
        let mut calls = 0;
        let mut predicate = |p| {
            if p == Checkpoint::AreaTerms {
                calls += 1;
            }
            false
        };
        let areas = measure_cycle_areas_with_cancellation(
            input,
            AreaLimits::default(),
            &mut Cancellation::new(&mut predicate),
        )
        .unwrap();
        assert_eq!(areas.summary().measured, 100);
        assert_eq!(areas.summary().assessed_terms, 300);
        assert_eq!(calls, 2); // Global indices 0 and 256, not a poll restarted at each triangle.
    });
}

#[test]
fn lossless_maps_and_face_permutations_preserve_area_and_source_identity() {
    let source = square();
    for scale in [0.125, 1., 8.] {
        for reflected in [false, true] {
            let points = source
                .vertices()
                .iter()
                .map(|p| {
                    Point3::new(
                        (if reflected { -p.x } else { p.x }) * scale + 16.,
                        p.y * scale - 4.,
                        p.z,
                    )
                })
                .collect::<Vec<_>>();
            let mut expected = None;
            for reverse in [false, true] {
                let mut faces = source.triangles().to_vec();
                if reverse {
                    faces.reverse();
                    for f in &mut faces {
                        f.swap(0, 2);
                    }
                }
                let mesh = TriangleMesh::try_new(points.clone(), faces).unwrap();
                let before = format!("{mesh:?}");
                evaluate(&mesh, |input| {
                    let areas = measure(input);
                    assert_eq!(
                        areas.outcomes(),
                        [PathAreaOutcome::Measured {
                            orientation: traversal_orientation(input, 0),
                            magnitude: AreaMagnitude::Finite(16. * scale * scale)
                        }]
                    );
                    if let Some(base) = &expected {
                        assert_eq!(areas.outcomes(), base);
                    } else {
                        expected = Some(areas.outcomes().to_vec());
                    }
                });
                assert_eq!(format!("{mesh:?}"), before);
            }
        }
    }
}

#[test]
fn all_small_canonical_fixtures_replay_without_hiding_inherited_context() {
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
        let imported =
            import_stl_path(file, StlOptions::new(StlUnitPolicy::AssumeMillimeters)).unwrap();
        let before = format!("{imported:?}");
        evaluate(&imported.mesh, |input| {
            let base = measure(input);
            assert_eq!(measure(input), base);
            assert!(std::ptr::eq(base.inspection(), input));
            conservation(&base);
        });
        assert_eq!(format!("{imported:?}"), before);
    }
}
