//! The opt-in face Z index must reproduce the full section scan exactly.
//!
//! Every fixture is sliced twice through the layer engine, with and without the index, over
//! schedules that include planes exactly at vertex heights and coplanar faces. All layer content,
//! diagnostics and failures must be identical; only the raw-section prefilter accounting differs.

use karasu_core::{
    cancellation::Cancellation,
    geometry::{ObjectTransform, Point3, Rotation3, Vector3},
    mesh::{
        stl::{import_stl_bytes, import_stl_path, StlOptions, StlUnitPolicy},
        TriangleMesh,
    },
    slicing::{
        engine::{LayerConfig, LayerEngine, LayerEngineError, LayerStageLimits, RegionLayer},
        grid::BinaryGridSpec,
        regions::RegionFillRule,
        schedule::{build_layer_schedule, ScheduleEnd, ScheduleLimits, UniformScheduleSpec},
        validity::ValidityLimits,
        SectionLimits, SectionTransform,
    },
};
use std::path::PathBuf;

#[path = "../../../tools/reference-fixtures/src/geometry.rs"]
#[allow(dead_code)] // The shared generator includes shapes outside this test.
mod geometry;
#[path = "../../../tools/reference-fixtures/src/stl.rs"]
mod stl;

fn fixture(name: &str) -> TriangleMesh {
    let options = StlOptions::new(StlUnitPolicy::AssumeMillimeters);
    if name == "fx-11-high-complexity.stl" {
        // Generated as in the raster corpus replay: the 16-MiB STL is not tracked in Git.
        let sphere = geometry::icosphere(30., geometry::Vec3::new(0., 0., 30.), 7, false);
        return import_stl_bytes(&stl::binary_stl(&sphere, "FX-11").unwrap(), options)
            .unwrap()
            .mesh;
    }
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/reference/stl")
        .join(name);
    import_stl_path(path, options).unwrap().mesh
}

fn config() -> LayerConfig {
    LayerConfig {
        grid: BinaryGridSpec::try_new([0.0, 0.0], -20).unwrap(),
        fill_rule: RegionFillRule::EvenOddStrict,
        stages: LayerStageLimits {
            section: SectionLimits {
                max_faces: 2_000_000,
                ..SectionLimits::default()
            },
            validity: ValidityLimits {
                max_pairs: 40_000_000,
                ..ValidityLimits::default()
            },
            ..LayerStageLimits::default()
        },
        max_output_payload_bytes: 64 * 1024 * 1024,
    }
}

fn layers(
    mesh: &TriangleMesh,
    first: f64,
    height: f64,
    count: u64,
    transform: SectionTransform<'_>,
    indexed: bool,
) -> Vec<Result<RegionLayer, String>> {
    let plan = build_layer_schedule(
        UniformScheduleSpec::try_new(first, height, ScheduleEnd::Count(count)).unwrap(),
        ScheduleLimits {
            max_layers: count,
            max_payload_bytes: 1 << 24,
        },
    )
    .unwrap();
    let engine = LayerEngine::new(
        mesh,
        &plan,
        transform,
        config(),
        Cancellation::never(),
        None,
    )
    .with_represented_coordinates();
    let engine = if indexed {
        engine.with_face_z_index(1 << 30)
    } else {
        engine
    };
    engine
        .map(|r| r.map_err(|e: LayerEngineError| e.to_string()))
        .collect()
}

fn assert_identical(full: &[Result<RegionLayer, String>], indexed: &[Result<RegionLayer, String>]) {
    assert_eq!(full.len(), indexed.len());
    for (a, b) in full.iter().zip(indexed) {
        match (a, b) {
            (Ok(a), Ok(b)) => {
                assert_eq!(a.plane(), b.plane());
                assert_eq!(a.raw().events(), b.raw().events(), "{:?}", a.plane());
                assert_eq!(a.completeness(), b.completeness());
                assert_eq!(a.summary(), b.summary());
                assert_eq!(a.vertices(), b.vertices());
                assert_eq!(a.edges(), b.edges());
                assert_eq!(a.paths(), b.paths());
                assert_eq!(a.boundaries(), b.boundaries());
                assert_eq!(a.regions(), b.regions());
                assert_eq!(a.graph_diagnostics(), b.graph_diagnostics());
                assert_eq!(a.path_diagnostics(), b.path_diagnostics());
                assert_eq!(a.planar_diagnostics(), b.planar_diagnostics());
                assert_eq!(a.region_diagnostics(), b.region_diagnostics());
                let (sa, sb) = (a.raw().summary(), b.raw().summary());
                assert_eq!(sa.source_faces, sb.source_faces);
                assert_eq!(sa.retained_events, sb.retained_events);
                assert_eq!(sa.non_intersecting_faces, sb.non_intersecting_faces);
                assert_eq!(sa.prefiltered_faces, 0);
            }
            (Err(a), Err(b)) => assert_eq!(a, b),
            _ => panic!("full {a:?} vs indexed {b:?}"),
        }
    }
}

#[test]
fn corpus_layers_including_vertex_heights_are_identical() {
    for name in [
        "fx-01-cube.stl",
        "fx-02-cylinder.stl",
        "fx-03-sphere.stl",
        "fx-04-hollow-box.stl",
        "fx-05-rotated-prism.stl",
        "fx-06-multiple-volumes.stl",
        "fx-07-internal-cavity.stl",
        "fx-08-degenerate-faces.stl",
        "fx-10a-open.stl",
        "fx-10b-non-manifold.stl",
    ] {
        let mesh = fixture(name);
        // Planes every 0.5 mm from Z = 0 hit the 0/10/20 mm vertex and face heights exactly.
        let full = layers(&mesh, 0.0, 0.5, 42, SectionTransform::Identity, false);
        let indexed = layers(&mesh, 0.0, 0.5, 42, SectionTransform::Identity, true);
        assert_identical(&full, &indexed);
        assert!(full.iter().any(Result::is_ok), "{name}");
    }
}

#[test]
fn transformed_dense_mesh_is_identical_and_mostly_prefiltered() {
    let mesh = fixture("fx-11-high-complexity.stl");
    let placement = ObjectTransform::new(
        Vector3::new(1.0, -2.0, 0.25),
        Rotation3::identity(),
        Vector3::new(1.0, 1.0, 1.0),
        Point3::new(0.0, 0.0, 0.0),
    )
    .unwrap();
    let transform = SectionTransform::Object(&placement);
    let full = layers(&mesh, 0.025, 7.5, 8, transform, false);
    let indexed = layers(&mesh, 0.025, 7.5, 8, transform, true);
    assert_identical(&full, &indexed);
    for layer in indexed.iter().flatten() {
        let summary = layer.raw().summary();
        assert_eq!(summary.source_faces, mesh.triangles().len());
        assert!(summary.prefiltered_faces > mesh.triangles().len() * 9 / 10);
    }
}

#[test]
fn index_budget_is_enforced_before_the_first_layer() {
    let mesh = fixture("fx-01-cube.stl");
    let plan = build_layer_schedule(
        UniformScheduleSpec::try_new(1.0, 1.0, ScheduleEnd::Count(2)).unwrap(),
        ScheduleLimits {
            max_layers: 2,
            max_payload_bytes: 1 << 16,
        },
    )
    .unwrap();
    let mut engine = LayerEngine::new(
        &mesh,
        &plan,
        SectionTransform::Identity,
        config(),
        Cancellation::never(),
        None,
    )
    .with_face_z_index(10);
    let error = engine.next().unwrap().unwrap_err();
    assert!(error.to_string().contains("IndexBytes"), "{error}");
    assert!(engine.next().is_none());
}
