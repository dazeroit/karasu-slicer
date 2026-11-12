//! Public-pipeline `ExactOnly` regressions using owned geometry and independent bounded oracles.
use karasu_core::cancellation::{Cancellation, Cancelled, Checkpoint};
use karasu_core::geometry::{Affine3, Point3, Vector3};
use karasu_core::mesh::TriangleMesh;
use karasu_core::slicing::graph::{build_incidence_graph, GraphLimits};
use karasu_core::slicing::grid::*;
use karasu_core::slicing::normalize::{
    normalize_section, CoordinateAgreement, NormalizationLimits,
};
use karasu_core::slicing::paths::{extract_paths, PathLimits};
use karasu_core::slicing::validity::{inspect_planar_paths, PlanarInspection, ValidityLimits};
use karasu_core::slicing::{section_mesh_z, SectionLimits, SectionTransform};

fn triangle(points: [[f64; 2]; 3]) -> TriangleMesh {
    TriangleMesh::try_new(
        points.map(|[x, y]| Point3::new(x, y, 0.)).to_vec(),
        vec![[0, 1, 2]],
    )
    .unwrap()
}
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
        &(0..n).map(|id| [id, (id + 1) % n]).collect::<Vec<_>>(),
    )
}
fn evaluate_at(
    mesh: &TriangleMesh,
    plane: f64,
    transform: SectionTransform<'_>,
    test: impl FnOnce(&PlanarInspection<'_, '_, '_, '_>),
) {
    let raw = section_mesh_z(mesh, plane, transform, SectionLimits::default()).unwrap();
    let normalized = normalize_section(&raw, NormalizationLimits::default()).unwrap();
    let graph = build_incidence_graph(&normalized, GraphLimits::default()).unwrap();
    let paths = extract_paths(&graph, PathLimits::default()).unwrap();
    let inspection = inspect_planar_paths(&paths, ValidityLimits::default()).unwrap();
    test(&inspection);
}
fn evaluate(mesh: &TriangleMesh, test: impl FnOnce(&PlanarInspection<'_, '_, '_, '_>)) {
    evaluate_at(mesh, 0., SectionTransform::Identity, test);
}
fn spec(origin: [f64; 2], k: i16) -> BinaryGridSpec {
    BinaryGridSpec::try_new(origin, k).unwrap()
}
fn map<'i, 'p, 'g, 'n, 'r>(
    input: &'i PlanarInspection<'p, 'g, 'n, 'r>,
    spec: BinaryGridSpec,
) -> ExactGridView<'i, 'p, 'g, 'n, 'r> {
    map_exact_grid_vertices(input, spec, GridLimits::default()).unwrap()
}
fn conservation(view: &ExactGridView<'_, '_, '_, '_, '_>) {
    let s = view.summary();
    assert_eq!(s.vertices, view.outcomes().len());
    assert_eq!(s.vertices, s.exact_vertices + s.unresolved_vertices);
    assert_eq!(s.axis_conversions, 2 * s.exact_vertices);
    assert_eq!(
        s.requested_payload_bytes,
        std::mem::size_of_val(view.outcomes())
    );
    assert_eq!(
        s.vertices,
        view.inspection().paths().graph().vertices().len()
    );
    assert_eq!(view.vertex(usize::MAX), None);
    assert_eq!(view.vertex(s.vertices), None);
}

#[test]
fn explicit_specs_validate_origins_exponents_and_canonical_metadata_zero() {
    for k in -1074..=1023 {
        let metadata = spec([-0., 0.], k);
        assert_eq!(metadata.origin_xy_mm().map(f64::to_bits), [0, 0]);
        assert_eq!(metadata.step_exponent(), k);
        assert_eq!(metadata.version(), 1);
        assert_eq!(metadata.max_abs_coordinate(), 1 << 52);
    }
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(
            BinaryGridSpec::try_new([bad, bad], i16::MIN),
            Err(GridSpecError::NonFiniteOrigin { axis: GridAxis::X })
        );
        assert_eq!(
            BinaryGridSpec::try_new([0., bad], i16::MAX),
            Err(GridSpecError::NonFiniteOrigin { axis: GridAxis::Y })
        );
    }
    for exponent in [i16::MIN, -1075, 1024, i16::MAX] {
        let error = BinaryGridSpec::try_new([0., 0.], exponent).unwrap_err();
        assert_eq!(error, GridSpecError::StepExponentOutOfRange { exponent });
        assert!(std::error::Error::source(&error).is_none());
        assert_ne!(error.to_string(), "");
    }
    assert_ne!(spec([0., 0.], 0), spec([f64::from_bits(1), 0.], 0));
    assert_ne!(spec([0., 0.], 0), spec([0., 0.], 1));
}

#[test]
fn analytic_triangle_has_exact_indices_complete_accounting_and_borrowed_inspection() {
    evaluate(&triangle([[2., -4.], [4., -4.], [2., -2.]]), |input| {
        let metadata = spec([2., -4.], -1);
        let view = map(input, metadata);
        assert_eq!(
            view.outcomes(),
            &[
                GridVertexOutcome::Exact { xy: [0, 0] },
                GridVertexOutcome::Exact { xy: [4, 0] },
                GridVertexOutcome::Exact { xy: [0, 4] }
            ]
        );
        assert!(std::ptr::eq(input, view.inspection()));
        assert_eq!(view.spec(), metadata);
        assert_eq!(view.summary().axis_conversions, 6);
        assert_eq!(
            view.vertex(1),
            Some(GridVertexOutcome::Exact { xy: [4, 0] })
        );
        conservation(&view);
    });
}

#[test]
fn inclusive_signed_domain_boundaries_are_not_rounded_or_saturated() {
    let bound = 2_f64.powi(52);
    evaluate(
        &triangle([[-bound, -bound], [bound, -bound], [0., bound]]),
        |input| {
            let view = map(input, spec([0., 0.], 0));
            assert_eq!(
                view.vertex(0),
                Some(GridVertexOutcome::Exact {
                    xy: [-(1 << 52), -(1 << 52)]
                })
            );
            assert_eq!(
                view.vertex(1),
                Some(GridVertexOutcome::Exact {
                    xy: [1 << 52, -(1 << 52)]
                })
            );
            conservation(&view);
        },
    );
    for x in [bound + 1., -bound - 1.] {
        evaluate(&triangle([[x, 0.], [0., 1.], [1., 0.]]), |input| {
            assert_eq!(
                map_exact_grid_vertices(input, spec([0., 0.], 0), GridLimits::default())
                    .unwrap_err(),
                GridError::CoordinateOutOfRange {
                    vertex: 0,
                    axis: GridAxis::X
                }
            );
        });
    }
}

#[test]
fn first_vertex_then_x_y_and_residual_before_range_define_failure_order() {
    evaluate(&triangle([[0.5, 0.25], [-2.25, 0.], [0., 2.]]), |input| {
        assert_eq!(
            map_exact_grid_vertices(input, spec([0., 0.], 0), GridLimits::default()).unwrap_err(),
            GridError::OffGrid {
                vertex: 0,
                axis: GridAxis::X
            }
        );
        assert_eq!(
            map_exact_grid_vertices(input, spec([0.5, 0.], 0), GridLimits::default()).unwrap_err(),
            GridError::OffGrid {
                vertex: 0,
                axis: GridAxis::Y
            }
        );
    });
    evaluate(&triangle([[f64::MAX, 0.], [0., 1.], [0., 2.]]), |input| {
        assert_eq!(
            map_exact_grid_vertices(
                input,
                spec([f64::from_bits(1), 0.], 0),
                GridLimits::default()
            )
            .unwrap_err(),
            GridError::OffGrid {
                vertex: 0,
                axis: GridAxis::X
            }
        );
    });
}

#[test]
fn represented_origin_subtraction_is_not_replaced_by_floating_subtraction() {
    evaluate(&triangle([[3., 0.], [2., 2.], [0., 0.]]), |input| {
        assert_eq!(
            map_exact_grid_vertices(input, spec([2_f64.powi(-53), 0.], 1), GridLimits::default())
                .unwrap_err(),
            GridError::OffGrid {
                vertex: 0,
                axis: GridAxis::X
            }
        );
    });
}

#[test]
fn complete_only_internal_and_duplicate_context_remains_borrowed() {
    let points = vec![
        Point3::new(0., 0., 0.),
        Point3::new(4., 0., 0.),
        Point3::new(4., 4., 0.),
        Point3::new(0., 4., 0.),
    ];
    for mesh in [
        TriangleMesh::try_new(points.clone(), vec![[0, 1, 2], [0, 2, 3]]).unwrap(),
        TriangleMesh::try_new(points, vec![[0, 1, 2], [0, 1, 2]]).unwrap(),
    ] {
        evaluate(&mesh, |input| {
            let before = format!("{input:?}");
            let view = map(input, spec([0., 0.], 0));
            assert!(
                input.summary().unassessed_internal_edges
                    + input.summary().unassessed_unresolved_edges
                    > 0
            );
            assert_eq!(format!("{:?}", view.inspection()), before);
            conservation(&view);
        });
    }
}

#[test]
fn isolated_equal_positions_never_weld_original_feature_identity() {
    let mesh = TriangleMesh::try_new(
        vec![
            Point3::new(0., 0., 0.),
            Point3::new(1., 0., 1.),
            Point3::new(0., 1., 1.),
            Point3::new(0., 0., 0.),
            Point3::new(-1., 0., 1.),
            Point3::new(0., -1., 1.),
        ],
        vec![[0, 1, 2], [3, 4, 5]],
    )
    .unwrap();
    evaluate(&mesh, |input| {
        let view = map(input, spec([0., 0.], 0));
        assert_eq!(input.summary().unassessed_isolated_vertices, 2);
        assert_eq!(view.summary().vertices, 2);
        assert_eq!(
            view.outcomes(),
            &[GridVertexOutcome::Exact { xy: [0, 0] }; 2]
        );
        conservation(&view);
    });
}

#[test]
fn off_grid_isolated_and_internal_only_vertices_cannot_be_skipped() {
    let tangent = TriangleMesh::try_new(
        vec![
            Point3::new(0.5, 0., 0.),
            Point3::new(1., 0., 1.),
            Point3::new(0., 1., 1.),
        ],
        vec![[0, 1, 2]],
    )
    .unwrap();
    evaluate(&tangent, |input| {
        assert_eq!(input.paths().paths(), []);
        assert_eq!(
            map_exact_grid_vertices(input, spec([0., 0.], 0), GridLimits::default()).unwrap_err(),
            GridError::OffGrid {
                vertex: 0,
                axis: GridAxis::X
            }
        );
    });
    let fan = TriangleMesh::try_new(
        vec![
            Point3::new(0., 0., 0.),
            Point3::new(2., 0., 0.),
            Point3::new(2., 2., 0.),
            Point3::new(0., 2., 0.),
            Point3::new(0.5, 0.5, 0.),
        ],
        vec![[0, 1, 4], [1, 2, 4], [2, 3, 4], [3, 0, 4]],
    )
    .unwrap();
    evaluate(&fan, |input| {
        assert!(input.summary().unassessed_internal_edges > 0);
        assert!(input
            .paths()
            .paths()
            .iter()
            .enumerate()
            .all(|(id, _)| !input.paths().path_vertices(id).unwrap().contains(&4)));
        assert_eq!(
            map_exact_grid_vertices(input, spec([0., 0.], 0), GridLimits::default()).unwrap_err(),
            GridError::OffGrid {
                vertex: 4,
                axis: GridAxis::X
            }
        );
    });
}

#[test]
fn transformed_world_coordinates_and_origin_reindexing_are_explicit() {
    let mesh = triangle([[0., 0.], [2., 0.], [0., 2.]]);
    let transform = Affine3::from_translation(Vector3::new(8., -4., 2.)).unwrap();
    evaluate_at(&mesh, 2., SectionTransform::Affine(&transform), |input| {
        let first = map(input, spec([8., -4.], -1));
        let second = map(input, spec([9., -6.], -1));
        assert_ne!(first.spec(), second.spec());
        for (a, b) in first.outcomes().iter().zip(second.outcomes()) {
            let (GridVertexOutcome::Exact { xy: a }, GridVertexOutcome::Exact { xy: b }) = (a, b)
            else {
                panic!("agreed");
            };
            assert_eq!(*b, [a[0] - 2, a[1] + 4]);
        }
        assert_eq!(
            first.vertex(0),
            Some(GridVertexOutcome::Exact { xy: [0, 0] })
        );
        assert_eq!(
            input.paths().graph().normalized().plane_z().to_bits(),
            2_f64.to_bits()
        );
    });
}

#[test]
fn open_ambiguous_crossed_collapsed_coincident_and_nested_context_is_not_material() {
    let nested = ribbon(
        &[
            [0., 0.],
            [8., 0.],
            [8., 8.],
            [0., 8.],
            [2., 2.],
            [6., 2.],
            [6., 6.],
            [2., 6.],
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
    for mesh in [
        ribbon(&[[0., 0.], [4., 0.]], &[[0, 1]]),
        ribbon(
            &[[0., 0.], [4., 0.], [0., 4.], [-4., 0.]],
            &[[0, 1], [0, 2], [0, 3]],
        ),
        cycle(&[[0., 0.], [4., 4.], [0., 4.], [4., 0.]]),
        triangle([[0., 0.], [0., 0.], [4., 0.]]),
        TriangleMesh::try_new(
            vec![
                Point3::new(0., 0., 0.),
                Point3::new(2., 0., 0.),
                Point3::new(0., 2., 0.),
                Point3::new(0., 0., 0.),
                Point3::new(2., 0., 0.),
                Point3::new(0., 2., 0.),
            ],
            vec![[0, 1, 2], [3, 4, 5]],
        )
        .unwrap(),
        nested,
    ] {
        evaluate(&mesh, |input| {
            let before = format!("{input:?}");
            let view = map(input, spec([0., 0.], -1));
            assert!(std::ptr::eq(view.inspection(), input));
            assert_eq!(format!("{input:?}"), before);
            conservation(&view);
        });
    }
}

#[test]
fn exact_and_one_below_caps_are_independent_and_fail_before_reservation() {
    evaluate(&triangle([[0., 0.], [2., 0.], [0., 2.]]), |input| {
        let metadata = spec([0., 0.], 0);
        let base = map(input, metadata);
        let summary = base.summary();
        let exact = GridLimits {
            max_vertices: summary.vertices,
            max_axis_conversions: summary.axis_conversions,
            max_payload_bytes: summary.requested_payload_bytes,
        };
        assert_eq!(
            map_exact_grid_vertices(input, metadata, exact).unwrap(),
            base
        );
        for (resource, limit, limits) in [
            (
                GridResource::Vertices,
                summary.vertices - 1,
                GridLimits {
                    max_vertices: summary.vertices - 1,
                    ..exact
                },
            ),
            (
                GridResource::AxisConversions,
                summary.axis_conversions - 1,
                GridLimits {
                    max_axis_conversions: summary.axis_conversions - 1,
                    ..exact
                },
            ),
            (
                GridResource::PayloadBytes,
                summary.requested_payload_bytes - 1,
                GridLimits {
                    max_payload_bytes: summary.requested_payload_bytes - 1,
                    ..exact
                },
            ),
        ] {
            let mut stored = false;
            let mut predicate = |phase| {
                stored |= phase == Checkpoint::GridStorage;
                false
            };
            assert_eq!(
                map_exact_grid_vertices_with_cancellation(
                    input,
                    metadata,
                    limits,
                    &mut Cancellation::new(&mut predicate)
                )
                .unwrap_err(),
                GridError::LimitExceeded { resource, limit }
            );
            assert!(!stored);
        }
        assert_eq!(
            map_exact_grid_vertices(
                input,
                metadata,
                GridLimits {
                    max_vertices: usize::MAX,
                    max_axis_conversions: usize::MAX,
                    max_payload_bytes: usize::MAX
                }
            )
            .unwrap(),
            base
        );
    });
}

#[test]
fn every_observed_checkpoint_cancels_atomically_and_retry_is_identical() {
    evaluate(&triangle([[0., 0.], [2., 0.], [0., 2.]]), |input| {
        let metadata = spec([0., 0.], 0);
        let before = format!("{input:?}");
        let base = map(input, metadata);
        let mut trace = Vec::new();
        let mut predicate = |phase| {
            trace.push(phase);
            false
        };
        assert_eq!(
            map_exact_grid_vertices_with_cancellation(
                input,
                metadata,
                GridLimits::default(),
                &mut Cancellation::new(&mut predicate)
            )
            .unwrap(),
            base
        );
        assert_eq!(
            trace,
            [
                Checkpoint::GridStart,
                Checkpoint::GridInput,
                Checkpoint::GridStorage,
                Checkpoint::GridStorage,
                Checkpoint::GridRecords,
                Checkpoint::GridCoordinates,
                Checkpoint::GridComplete
            ]
        );
        for (target, &checkpoint) in trace.iter().enumerate() {
            let mut visits = 0;
            let mut predicate = |_| {
                let stop = visits == target;
                visits += 1;
                stop
            };
            let error = map_exact_grid_vertices_with_cancellation(
                input,
                metadata,
                GridLimits::default(),
                &mut Cancellation::new(&mut predicate),
            )
            .unwrap_err();
            assert_eq!(error, GridError::Cancelled(Cancelled { checkpoint }));
            assert_eq!(visits, target + 1);
            assert!(std::error::Error::source(&error).is_some());
            assert_eq!(format!("{input:?}"), before);
            assert_eq!(map(input, metadata), base);
        }
    });
}

#[test]
fn global_axis_and_vertex_cadence_spans_more_than_256_vertices() {
    let mut vertices = Vec::new();
    let mut faces = Vec::new();
    for index in 0..513_u32 {
        let x = f64::from(index);
        vertices.extend([
            Point3::new(x, 0., 0.),
            Point3::new(x + 0.25, 0., 1.),
            Point3::new(x, 0.25, 1.),
        ]);
        faces.push([3 * index, 3 * index + 1, 3 * index + 2]);
    }
    let mesh = TriangleMesh::try_new(vertices, faces).unwrap();
    evaluate(&mesh, |input| {
        let metadata = spec([0., 0.], 0);
        let mut trace = Vec::new();
        let mut predicate = |phase| {
            trace.push(phase);
            false
        };
        let view = map_exact_grid_vertices_with_cancellation(
            input,
            metadata,
            GridLimits::default(),
            &mut Cancellation::new(&mut predicate),
        )
        .unwrap();
        for (phase, count) in [
            (Checkpoint::GridInput, 3),
            (Checkpoint::GridRecords, 3),
            (Checkpoint::GridCoordinates, 5),
            (Checkpoint::GridStorage, 2),
        ] {
            assert_eq!(trace.iter().filter(|&&p| p == phase).count(), count);
        }
        assert_eq!(view.summary().axis_conversions, 1026);
        for (id, outcome) in view.outcomes().iter().enumerate() {
            assert_eq!(
                *outcome,
                GridVertexOutcome::Exact {
                    xy: [i64::try_from(id).unwrap(), 0]
                }
            );
        }
        let mut count = 0;
        let mut predicate = |phase| {
            if phase == Checkpoint::GridCoordinates {
                count += 1;
            }
            phase == Checkpoint::GridCoordinates && count == 4
        };
        assert_eq!(
            map_exact_grid_vertices_with_cancellation(
                input,
                metadata,
                GridLimits::default(),
                &mut Cancellation::new(&mut predicate)
            )
            .unwrap_err(),
            GridError::Cancelled(Cancelled {
                checkpoint: Checkpoint::GridCoordinates
            })
        );
        assert_eq!(map(input, metadata), view);
    });
}

#[test]
fn empty_final_precancellation_and_unobserved_error_precedence_are_explicit() {
    evaluate(&TriangleMesh::try_new(vec![], vec![]).unwrap(), |input| {
        let limits = GridLimits {
            max_vertices: 0,
            max_axis_conversions: 0,
            max_payload_bytes: 0,
        };
        let mut trace = Vec::new();
        let mut predicate = |phase| {
            trace.push(phase);
            false
        };
        let view = map_exact_grid_vertices_with_cancellation(
            input,
            spec([0., 0.], 0),
            limits,
            &mut Cancellation::new(&mut predicate),
        )
        .unwrap();
        assert_eq!(trace, [Checkpoint::GridStart, Checkpoint::GridComplete]);
        conservation(&view);
        let mut predicate = |phase| phase == Checkpoint::GridComplete;
        assert_eq!(
            map_exact_grid_vertices_with_cancellation(
                input,
                spec([0., 0.], 0),
                limits,
                &mut Cancellation::new(&mut predicate)
            )
            .unwrap_err(),
            GridError::Cancelled(Cancelled {
                checkpoint: Checkpoint::GridComplete
            })
        );
    });
    evaluate(&triangle([[0.5, 0.], [2., 0.], [0., 2.]]), |input| {
        let mut predicate = |_| true;
        assert_eq!(
            map_exact_grid_vertices_with_cancellation(
                input,
                spec([0., 0.], 0),
                GridLimits {
                    max_vertices: 0,
                    ..GridLimits::default()
                },
                &mut Cancellation::new(&mut predicate)
            )
            .unwrap_err(),
            GridError::Cancelled(Cancelled {
                checkpoint: Checkpoint::GridStart
            })
        );
        let mut seen_final = false;
        let mut predicate = |phase| {
            seen_final |= phase == Checkpoint::GridComplete;
            phase == Checkpoint::GridComplete
        };
        let error = map_exact_grid_vertices_with_cancellation(
            input,
            spec([0., 0.], 0),
            GridLimits::default(),
            &mut Cancellation::new(&mut predicate),
        )
        .unwrap_err();
        assert_eq!(
            error,
            GridError::OffGrid {
                vertex: 0,
                axis: GridAxis::X
            }
        );
        assert!(!seen_final);
        assert!(std::error::Error::source(&error).is_none());
        assert_ne!(error.to_string(), "");
    });
}

#[test]
fn source_permutations_reindex_instead_of_changing_geometry_or_welding() {
    let points = [[0., 0.], [4., 0.], [0., 2.]];
    let original = triangle(points);
    let swapped = triangle([points[2], points[0], points[1]]);
    let mut original_outcomes = Vec::new();
    evaluate(&original, |input| {
        original_outcomes = map(input, spec([0., 0.], -1)).outcomes().to_vec();
    });
    evaluate(&swapped, |input| {
        let view = map(input, spec([0., 0.], -1));
        assert_eq!(
            view.outcomes(),
            &[
                original_outcomes[2],
                original_outcomes[0],
                original_outcomes[1]
            ]
        );
    });
    let reversed = TriangleMesh::try_new(original.vertices().to_vec(), vec![[0, 2, 1]]).unwrap();
    evaluate(&reversed, |input| {
        assert_eq!(map(input, spec([0., 0.], -1)).outcomes(), original_outcomes);
    });
}

#[test]
fn canonical_small_fixtures_use_exact_binary_scaling_or_explicit_rejection() {
    use karasu_core::mesh::stl::{import_stl_path, StlOptions, StlUnitPolicy};
    let root =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/reference/stl");
    let mut files = std::fs::read_dir(root)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            // FX-11 (generated) and FX-12 (owner-supplied) are large local-only fixtures.
            let name = path.file_name().unwrap().to_string_lossy();
            path.extension().is_some_and(|s| s == "stl")
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
            let mut failure = None;
            for (vertex, feature) in input
                .paths()
                .graph()
                .normalized()
                .features()
                .iter()
                .enumerate()
            {
                if let CoordinateAgreement::Agreed(point) = feature.coordinates {
                    for (value, axis) in [point.x, point.y]
                        .into_iter()
                        .zip([GridAxis::X, GridAxis::Y])
                    {
                        // For this bounded corpus, multiplication by 2^32 is exact, finite and
                        // cannot underflow. Integrality is an independent scalar oracle here only.
                        assert!(value.abs() < 1000.);
                        let scaled = value * 4_294_967_296.;
                        if scaled.fract() != 0. {
                            failure = Some(GridError::OffGrid { vertex, axis });
                            break;
                        }
                    }
                }
                if failure.is_some() {
                    break;
                }
            }
            let result = map_exact_grid_vertices(input, spec([0., 0.], -32), GridLimits::default());
            if let Some(error) = failure {
                assert_eq!(result.unwrap_err(), error);
            } else {
                let view = result.unwrap();
                conservation(&view);
                assert_eq!(map(input, spec([0., 0.], -32)), view);
            }
        });
        assert_eq!(format!("{imported:?}"), before);
    }
}
