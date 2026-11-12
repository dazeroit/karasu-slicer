//! Owned M1 fixture replay through the strict M3-to-M4 boundary, including defects.
use karasu_core::{
    cancellation::Cancellation,
    mesh::{
        stl::{import_stl_bytes, import_stl_path, StlOptions, StlUnitPolicy},
        TriangleMesh,
    },
    raster::{
        rasterize_layer, Compensation, LcdProfile, Orientation, OutsidePolicy, RasterConfig,
        RasterError, RasterLimits,
    },
    slicing::{
        engine::{LayerConfig, LayerEngine, LayerStageLimits},
        grid::BinaryGridSpec,
        regions::{RegionCompleteness, RegionFillRule},
        schedule::{build_layer_schedule, ScheduleEnd, ScheduleLimits, UniformScheduleSpec},
        SectionTransform,
    },
};
use std::path::Path;
// Keep FX11 reproducible on a clean checkout, without retaining a generated 16-MiB STL in Git.
#[path = "../../../tools/reference-fixtures/src/geometry.rs"]
#[allow(dead_code)] // The shared generator includes shapes outside this fixture replay.
mod geometry;
#[path = "../../../tools/reference-fixtures/src/stl.rs"]
mod stl;
fn import(name: &str) -> TriangleMesh {
    let options = StlOptions::new(StlUnitPolicy::AssumeMillimeters);
    if name == "fx-11-high-complexity.stl" {
        let sphere = geometry::icosphere(30., geometry::Vec3::new(0., 0., 30.), 7, false);
        return import_stl_bytes(&stl::binary_stl(&sphere, "FX-11").unwrap(), options)
            .unwrap()
            .mesh;
    }
    import_stl_path(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/reference/stl")
            .join(name),
        options,
    )
    .unwrap()
    .mesh
}
fn replay(name: &str, first: f64, step: f64, count: u64) -> (usize, usize) {
    let mesh = import(name);
    let plan = build_layer_schedule(
        UniformScheduleSpec::try_new(first, step, ScheduleEnd::Count(count)).unwrap(),
        ScheduleLimits {
            max_layers: 100,
            max_payload_bytes: 8192,
        },
    )
    .unwrap();
    let mut config = LayerConfig {
        grid: BinaryGridSpec::try_new([0., 0.], 0).unwrap(),
        fill_rule: RegionFillRule::EvenOddStrict,
        stages: LayerStageLimits::default(),
        max_output_payload_bytes: 128 << 20,
    };
    config.stages.validity.max_pairs = 16_000_000;
    let profile = LcdProfile::try_new(
        [128, 128],
        [-64., -64.],
        [128., 128.],
        Orientation::default(),
    )
    .unwrap();
    let (mut accepted, mut rejected) = (0, 0);
    for layer in LayerEngine::new(
        &mesh,
        &plan,
        SectionTransform::Identity,
        config,
        Cancellation::never(),
        None,
    )
    .with_represented_coordinates()
    {
        let layer = layer.unwrap();
        for samples_per_axis in [1, 4] {
            let raster_config = RasterConfig {
                samples_per_axis,
                grayscale_levels: 256,
                minimum_gray: 0,
                outside: OutsidePolicy::Reject,
                compensation: Compensation::None,
                kept: karasu_core::raster::KeptSampling::Antialiased,
            };
            let result = rasterize_layer(&layer, profile, raster_config, RasterLimits::default());
            if layer.completeness() == RegionCompleteness::Complete {
                let image = result
                    .unwrap_or_else(|error| panic!("{name} plane {:?}: {error}", layer.plane()));
                assert_eq!(image.plane(), layer.plane());
                assert_eq!(image.to_pixels().unwrap().len(), 128 * 128);
                assert_eq!(
                    image.stats().intensity_sum,
                    image
                        .to_pixels()
                        .unwrap()
                        .iter()
                        .map(|&n| u64::from(n))
                        .sum::<u64>()
                );
                assert_eq!(
                    image,
                    rasterize_layer(&layer, profile, raster_config, RasterLimits::default())
                        .unwrap(),
                    "{name}: deterministic raster rerun"
                );
                if !layer.boundaries().is_empty() {
                    assert!(
                        image.stats().nonzero_pixels > 0,
                        "{name}: visible corpus material"
                    );
                }
                accepted += 1;
            } else {
                assert_eq!(
                    result,
                    Err(RasterError::UnresolvedGeometry),
                    "{name}: unresolved geometry must never publish a successful mask"
                );
                rejected += 1;
            }
        }
    }
    (accepted, rejected)
}
#[test]
fn closed_owned_corpus_layers_render_deterministically() {
    for (name, first, step, count) in [
        ("fx-01-cube.stl", 2.5, 2.5, 7),
        ("fx-02-cylinder.stl", 2.5, 2.5, 7),
        ("fx-03-sphere.stl", 2.5, 2.5, 7),
        ("fx-04-hollow-box.stl", 2.5, 2.5, 7),
        ("fx-05-rotated-prism.stl", 5., 5., 6),
        ("fx-06-multiple-volumes.stl", 1.25, 1.25, 7),
        ("fx-07-internal-cavity.stl", 7.5, 2.5, 3),
    ] {
        assert_eq!(
            replay(name, first, step, count),
            (usize::try_from(count * 2).unwrap(), 0),
            "{name}"
        );
    }
}
#[test]
fn defect_corpus_rejects_unresolved_layers_and_accepts_complete_sections() {
    for name in [
        "fx-08-degenerate-faces.stl",
        "fx-09-reversed-face.stl",
        "fx-10a-open.stl",
        "fx-10b-non-manifold.stl",
    ] {
        let (accepted, rejected) = replay(name, 0., 5., 5);
        assert!(accepted > 0, "{name}");
        assert!(rejected > 0, "{name}");
        assert_eq!(accepted + rejected, 10);
    }
}
#[test]
fn generated_high_complexity_fixture_reaches_raster_boundary() {
    assert_eq!(replay("fx-11-high-complexity.stl", 15., 15., 3), (6, 0));
}
