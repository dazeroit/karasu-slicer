//! End-to-end acceptance of the owned, independently generated M1/M3 corpus.
use karasu_core::{
    cancellation::Cancellation,
    mesh::{
        stl::{import_stl_path, StlOptions, StlUnitPolicy},
        TriangleMesh,
    },
    slicing::{
        area::{AreaMagnitude, PathAreaOutcome},
        engine::*,
        grid::BinaryGridSpec,
        regions::{BoundaryRole, RegionCompleteness, RegionFillRule},
        schedule::*,
        SectionTransform,
    },
};
use std::path::Path;

// Reuse the independently owned fixture generator only in this acceptance test. FX-11 is
// intentionally excluded from Git, so a clean checkout must not depend on a local 16 MiB file.
#[path = "../../../tools/reference-fixtures/src/geometry.rs"]
#[allow(dead_code)] // Only the sphere generator is needed; other shapes remain shared test source.
mod geometry;
#[path = "../../../tools/reference-fixtures/src/stl.rs"]
mod stl;

fn import(name: &str) -> TriangleMesh {
    if name == "fx-11-high-complexity.stl" {
        let sphere = geometry::icosphere(30.0, geometry::Vec3::new(0.0, 0.0, 30.0), 7, false);
        let bytes = stl::binary_stl(&sphere, "FX-11").unwrap();
        assert_eq!(bytes.len(), 16_384_084);
        return karasu_core::mesh::stl::import_stl_bytes(
            &bytes,
            StlOptions::new(StlUnitPolicy::AssumeMillimeters),
        )
        .unwrap()
        .mesh;
    }
    import_stl_path(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/reference/stl")
            .join(name),
        StlOptions::new(StlUnitPolicy::AssumeMillimeters),
    )
    .unwrap()
    .mesh
}
fn layers(mesh: &TriangleMesh, first: f64, step: f64, count: u64) -> Vec<RegionLayer> {
    let plan = build_layer_schedule(
        UniformScheduleSpec::try_new(first, step, ScheduleEnd::Count(count)).unwrap(),
        ScheduleLimits {
            max_layers: 100,
            max_payload_bytes: 8192,
        },
    )
    .unwrap();
    let mut config = LayerConfig {
        grid: BinaryGridSpec::try_new([0.0, 0.0], 0).unwrap(),
        fill_rule: RegionFillRule::EvenOddStrict,
        stages: LayerStageLimits::default(),
        max_output_payload_bytes: 128 << 20,
    };
    config.stages.validity.max_pairs = 16_000_000;
    LayerEngine::new(
        mesh,
        &plan,
        SectionTransform::Identity,
        config,
        Cancellation::never(),
        None,
    )
    .with_represented_coordinates()
    .map(Result::unwrap)
    .collect()
}
fn area(layer: &RegionLayer) -> f64 {
    layer
        .boundaries()
        .iter()
        .map(|boundary| {
            let PathAreaOutcome::Measured {
                magnitude: AreaMagnitude::Finite(value),
                ..
            } = layer.paths()[boundary.path].area
            else {
                panic!("accepted boundary must have finite area");
            };
            if boundary.role == BoundaryRole::Outer {
                value
            } else {
                -value
            }
        })
        .sum()
}
fn valid(
    name: &str,
    first: f64,
    step: f64,
    count: u64,
    regions: usize,
    holes: usize,
) -> Vec<RegionLayer> {
    let mesh = import(name);
    let result = layers(&mesh, first, step, count);
    assert_eq!(
        result,
        layers(&mesh, first, step, count),
        "{name}: deterministic rerun"
    );
    for layer in &result {
        assert_eq!(
            layer.completeness(),
            RegionCompleteness::Complete,
            "{name} at {:?}",
            layer.plane()
        );
        assert_eq!(layer.regions().len(), regions, "{name}");
        assert_eq!(layer.boundaries().len(), regions + holes, "{name}");
        assert_eq!(layer.raw().summary().source_faces, mesh.triangles().len());
        assert!(layer.planar_diagnostics().is_empty(), "{name}");
        assert!(layer.region_diagnostics().is_empty(), "{name}");
        for vertex in layer.vertices() {
            assert_eq!(
                vertex.grid,
                karasu_core::slicing::grid::GridVertexOutcome::Represented
            );
        }
    }
    result
}
#[test]
fn closed_reference_solids_have_analytic_regions_holes_and_area_envelopes() {
    for layer in valid("fx-01-cube.stl", 2.5, 2.5, 7, 1, 0) {
        assert_eq!(area(&layer), 400.0);
    }
    for layer in valid("fx-04-hollow-box.stl", 2.5, 2.5, 7, 1, 1) {
        assert_eq!(area(&layer), 144.0);
    }
    for layer in valid("fx-06-multiple-volumes.stl", 1.25, 1.25, 7, 2, 0) {
        assert_eq!(area(&layer), 200.0);
    }
    let polygon_area = 64.0 * 100.0 * (std::f64::consts::TAU / 128.0).sin();
    for layer in valid("fx-02-cylinder.stl", 2.5, 2.5, 7, 1, 0) {
        assert!((area(&layer) - polygon_area).abs() < 0.0001);
    }
    for layer in valid("fx-03-sphere.stl", 2.5, 2.5, 7, 1, 0) {
        let ideal = std::f64::consts::PI * (100.0 - (layer.plane().z_mm - 10.0).powi(2));
        assert!((area(&layer) - ideal).abs() / ideal < 0.02);
    }
    for layer in valid("fx-05-rotated-prism.stl", 5.0, 5.0, 6, 1, 0) {
        assert!(area(&layer) > 0.0 && area(&layer) < 600.0);
    }
    for layer in valid("fx-07-internal-cavity.stl", 7.5, 2.5, 3, 1, 1) {
        let ideal_hole = std::f64::consts::PI * (25.0 - (layer.plane().z_mm - 10.0).powi(2));
        assert!(((400.0 - area(&layer)) - ideal_hole).abs() / ideal_hole < 0.06);
    }
}
#[test]
fn defect_fixtures_retain_source_and_report_unresolved_geometry_without_repair() {
    for name in [
        "fx-08-degenerate-faces.stl",
        "fx-09-reversed-face.stl",
        "fx-10a-open.stl",
        "fx-10b-non-manifold.stl",
    ] {
        let mesh = import(name);
        let original = import(name);
        for layer in layers(&mesh, 0.0, 5.0, 5) {
            assert_eq!(layer.raw().summary().source_faces, mesh.triangles().len());
            // These are local section guarantees, not whole-source validity:
            // FX-08/09/10b defects lie below the coherent top cap. FX-10a
            // removes the top cap, while its intact bottom patch is coherent.
            let expected_unresolved = (layer.plane().index == 0 && name != "fx-10a-open.stl")
                || (layer.plane().index == 4 && name == "fx-10a-open.stl")
                || (name == "fx-10b-non-manifold.stl" && layer.plane().index == 1);
            assert_eq!(
                layer.completeness() == RegionCompleteness::GeometricallyUnresolved,
                expected_unresolved,
                "{name} at {:?}",
                layer.plane()
            );
            if expected_unresolved {
                assert!(
                    !layer.region_diagnostics().is_empty()
                        || layer.paths().iter().any(|path| !path.closed)
                );
            } else {
                assert_eq!(layer.regions().len(), 1);
            }
        }
        assert_eq!(mesh, original);
    }
}
#[test]
fn high_complexity_reference_produces_ordered_regions_under_explicit_pair_budget() {
    let layers = valid("fx-11-high-complexity.stl", 15.0, 15.0, 3, 1, 0);
    for layer in layers {
        let ideal = std::f64::consts::PI * (900.0 - (layer.plane().z_mm - 30.0).powi(2));
        assert!((area(&layer) - ideal).abs() / ideal < 0.001);
        assert!(layer.payload_bytes() < 128 << 20);
    }
}

#[test]
fn exact_equatorial_octahedron_retains_two_source_supports_per_contour_edge() {
    use karasu_core::geometry::Point3;
    let mesh = TriangleMesh::try_new(
        [
            [1., 0., 0.],
            [0., 1., 0.],
            [-1., 0., 0.],
            [0., -1., 0.],
            [0., 0., 1.],
            [0., 0., -1.],
        ]
        .map(|p| Point3::new(p[0], p[1], p[2]))
        .to_vec(),
        vec![
            [0, 1, 4],
            [1, 2, 4],
            [2, 3, 4],
            [3, 0, 4],
            [1, 0, 5],
            [2, 1, 5],
            [3, 2, 5],
            [0, 3, 5],
        ],
    )
    .unwrap();
    let layers = layers(&mesh, 0.0, 1.0, 1);
    let layer = &layers[0];
    assert_eq!(layer.completeness(), RegionCompleteness::Complete);
    assert_eq!(layer.regions().len(), 1);
    assert_eq!(area(layer), 2.0);
    assert_eq!(layer.segments().len(), 8);
    assert_eq!(layer.edges().len(), 4);
    for edge in 0..4 {
        assert_eq!(layer.edge_occurrences(edge).unwrap().len(), 2);
    }
}
