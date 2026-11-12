//! Dedicated analytic nesting, disconnected-region and contact-preservation oracles.
use karasu_core::{
    cancellation::Cancellation,
    geometry::Point3,
    mesh::TriangleMesh,
    slicing::{
        area::*, engine::*, graph::*, grid::*, normalize::*, paths::*, regions::*, schedule::*,
        validity::*, *,
    },
};
fn byte(data: &[u8], index: usize) -> u8 {
    data.get(index).copied().unwrap_or(0)
}
/// Generate up to six independent ribbons at an exact dyadic plane and compare reviewed stages.
/// Returns success for uninstrumented replay. Contact cases must remain explicitly unresolved.
pub fn check_strict_regions(data: &[u8]) -> bool {
    let represented = byte(data, 3) & 1 != 0;
    let count = 2 + usize::from(byte(data, 0) % 5);
    let mode = byte(data, 1) % 3;
    let mut vertices = Vec::new();
    let mut faces = Vec::new();
    for ring in 0..count {
        let (low_x, low_y, high_x, high_y) = if mode == 0 {
            let low = 2. * ring as f64;
            let high = 4. * count as f64 - low;
            (low, low, high, high)
        } else {
            let low = ring as f64 * if mode == 1 { 32. } else { 8. };
            (low, 0., low + 8., 8.)
        };
        let base = vertices.len() as u32;
        for [x, y] in [
            [low_x, low_y],
            [high_x, low_y],
            [high_x, high_y],
            [low_x, high_y],
        ] {
            let offset = if represented { 0.1 } else { 0.0 };
            vertices.extend([
                Point3::new(x + offset, y + offset, -2.),
                Point3::new(x + offset, y + offset, 2.),
            ]);
        }
        for [first, second] in [[0, 1], [1, 2], [2, 3], [3, 0]] {
            faces.extend([
                [base + 2 * first, base + 2 * second, base + 2 * second + 1],
                [
                    base + 2 * first,
                    base + 2 * second + 1,
                    base + 2 * first + 1,
                ],
            ]);
        }
    }
    let mesh = TriangleMesh::try_new(vertices, faces).unwrap();
    let plane = if represented {
        0.25 * f64::from(byte(data, 2) % 7) - 0.75
    } else {
        f64::from(byte(data, 2) % 3) - 1.
    };
    let plan = build_layer_schedule(
        UniformScheduleSpec::try_new(plane, 1., ScheduleEnd::Count(1)).unwrap(),
        ScheduleLimits {
            max_layers: 1,
            max_payload_bytes: 1024,
        },
    )
    .unwrap();
    let config = LayerConfig {
        grid: BinaryGridSpec::try_new([0., 0.], -2).unwrap(),
        fill_rule: RegionFillRule::EvenOddStrict,
        stages: LayerStageLimits::default(),
        max_output_payload_bytes: 1 << 20,
    };
    let generate = || {
        let stream = LayerEngine::new(
            &mesh,
            &plan,
            SectionTransform::Identity,
            config,
            Cancellation::never(),
            None,
        );
        let mut stream = if represented {
            stream.with_represented_coordinates()
        } else {
            stream
        };
        stream.next().unwrap().unwrap()
    };
    let layer = generate();
    assert_eq!(layer, generate());
    let raw = section_mesh_z(
        &mesh,
        plane,
        SectionTransform::Identity,
        config.stages.section,
    )
    .unwrap();
    let normalized = normalize_section(&raw, config.stages.normalization).unwrap();
    let graph = build_incidence_graph(&normalized, config.stages.graph).unwrap();
    let paths = extract_paths(&graph, config.stages.paths).unwrap();
    let validity = inspect_planar_paths(&paths, config.stages.validity).unwrap();
    let area = measure_cycle_areas(&validity, config.stages.area).unwrap();
    let grid = if represented {
        retain_represented_vertices_with_cancellation(
            &validity,
            config.grid,
            config.stages.grid,
            &mut Cancellation::never(),
        )
    } else {
        map_exact_grid_vertices(&validity, config.grid, config.stages.grid)
    }
    .unwrap();
    let regions = assemble_strict_regions(&validity, &area, &grid, config.stages.regions).unwrap();
    assert_eq!(layer.summary().regions, regions.summary());
    assert_eq!(layer.boundaries(), regions.boundaries());
    assert_eq!(layer.region_diagnostics(), regions.diagnostics());
    assert_eq!(layer.paths().len(), count);
    assert_eq!(layer.raw(), &raw);
    if mode == 2 {
        assert_eq!(
            layer.completeness(),
            RegionCompleteness::GeometricallyUnresolved
        );
        assert!(!layer.planar_diagnostics().is_empty());
        assert!(!layer.region_diagnostics().is_empty());
    } else {
        assert_eq!(layer.boundaries().len(), count);
        for boundary in layer.boundaries() {
            let expected = if mode == 0 { boundary.path } else { 0 };
            assert_eq!(boundary.depth, expected);
            assert_eq!(
                boundary.role,
                if expected % 2 == 0 {
                    BoundaryRole::Outer
                } else {
                    BoundaryRole::Hole
                }
            );
            if let Some(parent) = boundary.parent {
                assert!(parent < layer.boundaries().len());
            }
        }
        assert_eq!(
            layer.regions().len(),
            if mode == 0 { count.div_ceil(2) } else { count }
        );
    }
    for path in 0..layer.paths().len() {
        for &edge in layer.path_edges(path).unwrap() {
            for &occurrence in layer.edge_occurrences(edge).unwrap() {
                assert!(layer.segments()[occurrence].event < layer.raw().events().len());
            }
        }
    }
    assert!(layer.material_union().is_none());
    true
}
