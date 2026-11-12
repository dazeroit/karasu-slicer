//! Large-model support (TD-053) with the owner-supplied FX-12 real model.
//!
//! FX-12 is local only (ignored by Git, about 128 MiB), so the test is ignored by default and
//! skips with a message when the file is absent. Run it with
//! `cargo test --release -p karasu-core --test large_models -- --ignored`.
use karasu_core::{
    cancellation::{Cancellation, Checkpoint},
    digest::{sha256, to_hex},
    mesh::{
        repair::RepairPolicy,
        stl::{import_stl_bytes, StlOptions, StlUnitPolicy},
        topology::TopologyLimits,
        TriangleMesh,
    },
    raster::{
        parallel::{with_parallel_raster, LayerFill, ParallelRaster},
        Compensation, LcdProfile, Orientation, OutsidePolicy, RasterConfig, RasterLimits,
    },
    slicing::{
        engine::{LayerConfig, LayerStageLimits},
        grid::BinaryGridSpec,
        regions::RegionFillRule,
        schedule::{build_layer_schedule, ScheduleEnd, ScheduleLimits, UniformScheduleSpec},
        winding::WindingRule,
        SectionTransform,
    },
};
use std::path::Path;

const FX12_SHA256: &str = "decf78c163e833133ee8c6c671bba4a88006b74a617f740647196125e9c1f518";

fn never(_: Checkpoint) -> bool {
    false
}

/// The two duplicated faces are opposite pairs: the default repairs cancel them and leave one
/// closed, edge-manifold, consistently oriented surface (which hollowing admits).
fn repairs_to_a_closed_manifold(mesh: &TriangleMesh) {
    let (repaired, report) = mesh
        .repair_with_cancellation(
            RepairPolicy {
                duplicates: true,
                orientation: true,
                outward: false,
            },
            TopologyLimits::default(),
            &mut Cancellation::never(),
        )
        .unwrap();
    assert_eq!(
        (
            report.dropped_duplicate_faces,
            report.cancelled_duplicate_groups,
            report.flipped_faces
        ),
        (4, 2, 0)
    );
    let defects = repaired
        .unwrap()
        .defect_report_with_cancellation(TopologyLimits::default(), &mut Cancellation::never())
        .unwrap();
    assert_eq!(
        [
            defects.non_manifold_edges,
            defects.boundary_edges,
            defects.components,
            defects.closed_components
        ],
        [0, 0, 1, 1]
    );
}

#[test]
#[ignore = "large local fixture: run with --release -- --ignored"]
fn fx12_imports_admits_and_slices_with_default_limits() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/reference/stl/fx-12-real-model.stl");
    let Ok(bytes) = std::fs::read(&path) else {
        eprintln!("skipped: FX-12 is not present at {}", path.display());
        return;
    };
    assert_eq!(
        to_hex(&sha256(&bytes)),
        FX12_SHA256,
        "a different FX-12 file"
    );
    // Default import limits admit it (2.7 million triangles, 128 MiB).
    let mesh = import_stl_bytes(&bytes, StlOptions::new(StlUnitPolicy::AssumeMillimeters))
        .unwrap()
        .mesh;
    drop(bytes);
    assert_eq!(mesh.triangles().len(), 2_675_038);
    assert_eq!(mesh.vertices().len(), 1_337_327);
    // Closed and consistently oriented; two duplicated faces make four non-manifold edges, which
    // the winding union admits.
    let edges = mesh
        .edge_summary_with_cancellation(TopologyLimits::default(), &mut Cancellation::never())
        .unwrap();
    assert_eq!(
        [
            edges.edges,
            edges.boundary_edges,
            edges.non_manifold_edges,
            edges.winding_conflicts
        ],
        [4_012_553, 0, 4, 0]
    );
    repairs_to_a_closed_manifold(&mesh);
    // Ten union layers through the model's height on a 0.1 mm panel.
    let schedule = build_layer_schedule(
        UniformScheduleSpec::try_new(2.5, 5., ScheduleEnd::Count(10)).unwrap(),
        ScheduleLimits {
            max_layers: 10,
            max_payload_bytes: 1 << 20,
        },
    )
    .unwrap();
    let spec = ParallelRaster {
        mesh: &mesh,
        schedule: &schedule,
        transform: SectionTransform::Identity,
        layer_config: LayerConfig {
            grid: BinaryGridSpec::try_new([0., 0.], -20).unwrap(),
            fill_rule: RegionFillRule::EvenOddStrict,
            stages: LayerStageLimits::default(),
            max_output_payload_bytes: 64 << 20,
        },
        represented_coordinates: true,
        face_index_bytes: Some(1 << 30),
        vertex_avoiding_bytes: Some(1 << 30),
        profile: LcdProfile::try_new([600, 600], [-30., -30.], [60., 60.], Orientation::default())
            .unwrap(),
        raster: RasterConfig {
            samples_per_axis: 1,
            grayscale_levels: 256,
            minimum_gray: 0,
            outside: OutsidePolicy::Reject,
            compensation: Compensation::None,
            kept: karasu_core::raster::KeptSampling::Antialiased,
        },
        bottom: None,
        limits: RasterLimits::default(),
        cut: None,
        kept_faces: &[],
        fill: LayerFill::Winding(WindingRule::Positive),
        closure: None,
        threads: 0,
        window_bytes: ParallelRaster::DEFAULT_WINDOW_BYTES,
    };
    let images: Vec<_> = with_parallel_raster(&spec, &never, |items| items.collect());
    assert_eq!(
        images.len(),
        10,
        "{:?}",
        images
            .last()
            .map(|i| i.as_ref().err().map(ToString::to_string))
    );
    for image in images {
        let image = image.unwrap();
        assert!(image.stats().nonzero_pixels > 0, "{:?}", image.plane());
        assert_eq!(image.winding().unwrap().min, 0);
    }
}
