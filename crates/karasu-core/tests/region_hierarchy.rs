//! Public-pipeline strict exact containment and retained-ambiguity regressions.
use karasu_core::cancellation::{Cancellation, Cancelled, Checkpoint};
use karasu_core::geometry::Point3;
use karasu_core::mesh::TriangleMesh;
use karasu_core::slicing::area::{measure_cycle_areas, AreaLimits, SectionAreas};
use karasu_core::slicing::graph::{build_incidence_graph, GraphLimits};
use karasu_core::slicing::grid::{
    map_exact_grid_vertices, BinaryGridSpec, ExactGridView, GridLimits,
};
use karasu_core::slicing::normalize::{normalize_section, NormalizationLimits};
use karasu_core::slicing::paths::{extract_paths, PathLimits};
use karasu_core::slicing::regions::*;
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
    let count = u32::try_from(points.len()).unwrap();
    ribbon(
        points,
        &(0..count)
            .map(|index| [index, (index + 1) % count])
            .collect::<Vec<_>>(),
    )
}
fn evaluate(
    mesh: &TriangleMesh,
    test: impl FnOnce(
        &PlanarInspection<'_, '_, '_, '_>,
        &SectionAreas<'_, '_, '_, '_, '_>,
        &ExactGridView<'_, '_, '_, '_, '_>,
    ),
) {
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
    let areas = measure_cycle_areas(&inspection, AreaLimits::default()).unwrap();
    let grid = map_exact_grid_vertices(
        &inspection,
        BinaryGridSpec::try_new([0., 0.], 0).unwrap(),
        GridLimits::default(),
    )
    .unwrap();
    test(&inspection, &areas, &grid);
}
fn hierarchy_fixture() -> TriangleMesh {
    ribbon(
        &[
            [0., 0.],
            [10., 0.],
            [10., 10.],
            [0., 10.],
            [2., 2.],
            [8., 2.],
            [8., 8.],
            [2., 8.],
            [4., 4.],
            [6., 4.],
            [6., 6.],
            [4., 6.],
            [20., 0.],
            [22., 0.],
            [22., 2.],
            [20., 2.],
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
            [8, 9],
            [9, 10],
            [10, 11],
            [11, 8],
            [12, 13],
            [13, 14],
            [14, 15],
            [15, 12],
        ],
    )
}

#[test]
fn nested_and_disconnected_cycles_form_a_strict_even_odd_forest() {
    let mesh = hierarchy_fixture();
    evaluate(&mesh, |input, areas, grid| {
        let before_input = format!("{input:?}");
        let before_areas = format!("{areas:?}");
        let before_grid = format!("{grid:?}");
        let result = assemble_strict_regions(input, areas, grid, RegionLimits::default()).unwrap();
        assert!(std::ptr::eq(result.inspection(), input));
        assert!(std::ptr::eq(result.areas(), areas));
        assert!(std::ptr::eq(result.grid(), grid));
        assert_eq!(result.fill_rule(), RegionFillRule::EvenOddStrict);
        assert_eq!(result.completeness(), RegionCompleteness::Complete);
        assert_eq!(result.path_outcomes().len(), 4);
        assert_eq!(
            result.path_outcomes(),
            [
                PathRegionOutcome::Boundary { boundary: 0 },
                PathRegionOutcome::Boundary { boundary: 1 },
                PathRegionOutcome::Boundary { boundary: 2 },
                PathRegionOutcome::Boundary { boundary: 3 },
            ]
        );
        assert_eq!(
            result
                .boundaries()
                .iter()
                .map(|record| (record.path, record.parent, record.depth, record.role))
                .collect::<Vec<_>>(),
            [
                (0, None, 0, BoundaryRole::Outer),
                (1, Some(0), 1, BoundaryRole::Hole),
                (2, Some(1), 2, BoundaryRole::Outer),
                (3, None, 0, BoundaryRole::Outer),
            ]
        );
        for boundary in result.boundaries() {
            assert_eq!(
                boundary.reversed_view,
                matches!(
                    (boundary.role, boundary.orientation),
                    (
                        BoundaryRole::Outer,
                        karasu_core::slicing::area::CycleOrientation::Clockwise
                    ) | (
                        BoundaryRole::Hole,
                        karasu_core::slicing::area::CycleOrientation::Counterclockwise
                    )
                )
            );
        }
        assert_eq!(
            result
                .regions()
                .iter()
                .map(MaterialRegion::outer_boundary)
                .collect::<Vec<_>>(),
            [0, 2, 3]
        );
        assert_eq!(result.region_holes(0), Some(&[1][..]));
        assert_eq!(result.region_holes(1), Some(&[][..]));
        assert_eq!(result.region_holes(2), Some(&[][..]));
        assert_eq!(result.region_holes(3), None);
        assert_eq!(result.diagnostics(), []);
        assert_eq!(
            result.summary(),
            RegionSummary {
                paths: 4,
                cycle_candidates: 4,
                cycle_vertices: 32,
                boundaries: 4,
                non_boundaries: 0,
                pairs: 6,
                ray_tests: 24,
                relations: 3,
                regions: 3,
                holes: 1,
                diagnostics: 0,
                requested_payload_bytes: result.summary().requested_payload_bytes,
            }
        );
        assert!(result.summary().requested_payload_bytes > 0);
        assert_eq!(format!("{input:?}"), before_input);
        assert_eq!(format!("{areas:?}"), before_areas);
        assert_eq!(format!("{grid:?}"), before_grid);
    });
}

#[test]
fn open_non_simple_and_cross_path_contact_remain_explicit_non_boundaries() {
    let open = ribbon(&[[0., 0.], [2., 0.], [4., 0.]], &[[0, 1], [1, 2]]);
    evaluate(&open, |input, areas, grid| {
        let result = assemble_strict_regions(input, areas, grid, RegionLimits::default()).unwrap();
        assert_eq!(
            result.path_outcomes(),
            [PathRegionOutcome::NonBoundary(NonBoundaryReason::OpenChain)]
        );
        assert_eq!(result.boundaries(), []);
        assert_eq!(
            result.completeness(),
            RegionCompleteness::GeometricallyUnresolved
        );
    });

    let bow_tie = cycle(&[[0., 0.], [4., 4.], [0., 4.], [4., 0.]]);
    evaluate(&bow_tie, |input, areas, grid| {
        let result = assemble_strict_regions(input, areas, grid, RegionLimits::default()).unwrap();
        assert_eq!(
            result.path_outcomes(),
            [PathRegionOutcome::NonBoundary(
                NonBoundaryReason::NonSimpleCycle
            )]
        );
        assert_eq!(result.boundaries(), []);
        assert_eq!(
            result.completeness(),
            RegionCompleteness::GeometricallyUnresolved
        );
    });

    let touching = ribbon(
        &[
            [0., 0.],
            [4., 0.],
            [4., 4.],
            [0., 4.],
            [4., 0.],
            [8., 0.],
            [8., 4.],
            [4., 4.],
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
    evaluate(&touching, |input, areas, grid| {
        let result = assemble_strict_regions(input, areas, grid, RegionLimits::default()).unwrap();
        assert!(result.path_outcomes().iter().all(|outcome| *outcome
            == PathRegionOutcome::NonBoundary(NonBoundaryReason::CrossPathContact)));
        assert!(result
            .diagnostics()
            .iter()
            .any(|diagnostic| matches!(diagnostic, RegionDiagnostic::CrossPathContact { .. })));
        assert_eq!(result.boundaries(), []);
        assert_eq!(
            result.completeness(),
            RegionCompleteness::GeometricallyUnresolved
        );
    });
}

#[test]
fn tangencies_are_nonmaterial_while_unresolved_complete_only_context_is_reported() {
    let tangent = TriangleMesh::try_new(
        vec![
            Point3::new(0., 0., 0.),
            Point3::new(1., 0., 1.),
            Point3::new(0., 1., 1.),
        ],
        vec![[0, 1, 2]],
    )
    .unwrap();
    evaluate(&tangent, |input, areas, grid| {
        let result = assemble_strict_regions(input, areas, grid, RegionLimits::default()).unwrap();
        assert_eq!(result.path_outcomes(), []);
        assert_eq!(result.diagnostics(), []);
        assert_eq!(result.completeness(), RegionCompleteness::Complete);
    });

    let unresolved = TriangleMesh::try_new(
        vec![
            Point3::new(0., 0., 0.),
            Point3::new(2., 0., 0.),
            Point3::new(0., 2., 0.),
        ],
        vec![[0, 1, 2], [0, 1, 2]],
    )
    .unwrap();
    evaluate(&unresolved, |input, areas, grid| {
        let result = assemble_strict_regions(input, areas, grid, RegionLimits::default()).unwrap();
        assert!(result.diagnostics().iter().any(|diagnostic| matches!(
            diagnostic,
            RegionDiagnostic::UnresolvedCompleteOnly { edges } if *edges > 0
        )));
        assert_eq!(
            result.completeness(),
            RegionCompleteness::GeometricallyUnresolved
        );
    });
}

#[test]
fn independent_resource_limits_reject_without_a_partial_hierarchy() {
    let mesh = hierarchy_fixture();
    evaluate(&mesh, |input, areas, grid| {
        let baseline =
            assemble_strict_regions(input, areas, grid, RegionLimits::default()).unwrap();
        let summary = baseline.summary();
        for (limits, resource, limit) in [
            (
                RegionLimits {
                    max_cycle_vertices: summary.cycle_vertices - 1,
                    ..RegionLimits::default()
                },
                RegionResource::CycleVertices,
                summary.cycle_vertices - 1,
            ),
            (
                RegionLimits {
                    max_pairs: summary.pairs - 1,
                    ..RegionLimits::default()
                },
                RegionResource::Pairs,
                summary.pairs - 1,
            ),
            (
                RegionLimits {
                    max_ray_tests: summary.ray_tests - 1,
                    ..RegionLimits::default()
                },
                RegionResource::RayTests,
                summary.ray_tests - 1,
            ),
            (
                RegionLimits {
                    max_relations: summary.relations - 1,
                    ..RegionLimits::default()
                },
                RegionResource::Relations,
                summary.relations - 1,
            ),
            (
                RegionLimits {
                    max_regions: summary.regions - 1,
                    ..RegionLimits::default()
                },
                RegionResource::Regions,
                summary.regions - 1,
            ),
            (
                RegionLimits {
                    max_payload_bytes: summary.requested_payload_bytes - 1,
                    ..RegionLimits::default()
                },
                RegionResource::PayloadBytes,
                summary.requested_payload_bytes - 1,
            ),
        ] {
            assert_eq!(
                assemble_strict_regions(input, areas, grid, limits).unwrap_err(),
                RegionError::LimitExceeded { resource, limit }
            );
        }
    });
}

#[test]
fn cancellation_at_each_observed_phase_is_atomic_and_retryable() {
    let mesh = hierarchy_fixture();
    evaluate(&mesh, |input, areas, grid| {
        let baseline =
            assemble_strict_regions(input, areas, grid, RegionLimits::default()).unwrap();
        let mut trace = Vec::new();
        let mut record = |point| {
            trace.push(point);
            false
        };
        assert_eq!(
            assemble_strict_regions_with_cancellation(
                input,
                areas,
                grid,
                RegionLimits::default(),
                &mut Cancellation::new(&mut record),
            )
            .unwrap(),
            baseline
        );
        for checkpoint in [
            Checkpoint::RegionsStart,
            Checkpoint::RegionsInput,
            Checkpoint::RegionsStorage,
            Checkpoint::RegionsBounds,
            Checkpoint::RegionsPairs,
            Checkpoint::RegionsForest,
            Checkpoint::RegionsOutput,
            Checkpoint::RegionsComplete,
        ] {
            assert!(trace.contains(&checkpoint));
            let mut predicate = |point| point == checkpoint;
            assert_eq!(
                assemble_strict_regions_with_cancellation(
                    input,
                    areas,
                    grid,
                    RegionLimits::default(),
                    &mut Cancellation::new(&mut predicate),
                )
                .unwrap_err(),
                RegionError::Cancelled(Cancelled { checkpoint })
            );
            assert_eq!(
                assemble_strict_regions(input, areas, grid, RegionLimits::default()).unwrap(),
                baseline
            );
        }
    });
}
