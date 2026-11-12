//! Adversarial public-boundary tests for raster conversion and terminal streams.
use karasu_core::{
    cancellation::{Cancellation, Checkpoint},
    geometry::Point3,
    mesh::TriangleMesh,
    raster::{
        stream::{RasterStream, RasterStreamError},
        *,
    },
    slicing::{
        construct::ConstructLimits, engine::*, grid::BinaryGridSpec, regions::RegionFillRule,
        schedule::*, SectionTransform,
    },
};
use std::{cell::Cell, io};

fn profile() -> LcdProfile {
    LcdProfile::try_new([8, 8], [0., 0.], [8., 8.], Orientation::default()).unwrap()
}
fn config() -> RasterConfig {
    RasterConfig {
        samples_per_axis: 16,
        grayscale_levels: 256,
        minimum_gray: 0,
        outside: OutsidePolicy::Clip,
        compensation: Compensation::None,
        kept: karasu_core::raster::KeptSampling::Antialiased,
    }
}
fn constructed(origin: [f64; 2], represented: bool) -> RegionLayer {
    let rings = [
        vec![[0., 0.], [6., 4.], [0., 8.]],
        vec![[2., 0.], [4., 0.], [4., 8.], [2., 8.]],
    ];
    let mut points = Vec::new();
    let mut faces = Vec::new();
    let mut labels = Vec::new();
    for (label, ring) in rings.iter().enumerate() {
        let base = u32::try_from(points.len()).unwrap();
        for &[x, y] in ring {
            points.extend([Point3::new(x, y, -1.), Point3::new(x, y, 1.)]);
        }
        for i in 0..ring.len() {
            let a = base + 2 * u32::try_from(i).unwrap();
            let b = base + 2 * u32::try_from((i + 1) % ring.len()).unwrap();
            faces.extend([[a, b, b + 1], [a, b + 1, a + 1]]);
            labels.extend([u64::try_from(label).unwrap(); 2]);
        }
    }
    let mesh = TriangleMesh::try_new(points, faces).unwrap();
    let schedule = build_layer_schedule(
        UniformScheduleSpec::try_new(0., 0.25, ScheduleEnd::Count(1)).unwrap(),
        ScheduleLimits {
            max_layers: 1,
            max_payload_bytes: 1024,
        },
    )
    .unwrap();
    let settings = LayerConfig {
        grid: BinaryGridSpec::try_new(origin, 0).unwrap(),
        fill_rule: RegionFillRule::EvenOddStrict,
        stages: LayerStageLimits::default(),
        max_output_payload_bytes: 1 << 24,
    };
    let mut engine = LayerEngine::new(
        &mesh,
        &schedule,
        SectionTransform::Identity,
        settings,
        Cancellation::never(),
        None,
    )
    .with_material_union(&labels, ConstructLimits::default());
    if represented {
        engine = engine.with_represented_coordinates();
    }
    let layer = engine.next().unwrap().unwrap();
    assert!(layer.material_union().is_some());
    layer
}
#[test]
fn rational_union_grid_and_represented_coordinates_agree() {
    let grid = constructed([0., 0.], false);
    let represented = constructed([0., 0.], true);
    let a = rasterize_layer(&grid, profile(), config(), RasterLimits::default()).unwrap();
    let b = rasterize_layer(&represented, profile(), config(), RasterLimits::default()).unwrap();
    assert_eq!(a.mask(), b.mask());
    assert!(a.stats().nonzero_pixels > 0);
}
#[test]
fn cancellation_prone_exact_grid_origin_is_rejected_before_rational_rounding() {
    let layer = constructed([2_f64.powi(52); 2], false);
    assert_eq!(
        rasterize_layer(&layer, profile(), config(), RasterLimits::default()).unwrap_err(),
        RasterError::NumericRange
    );
}
#[test]
fn source_error_is_preserved_and_stream_is_fused() {
    let source = std::iter::once(Err::<RegionLayer, _>(io::Error::new(
        io::ErrorKind::BrokenPipe,
        "adversarial source",
    )));
    let mut stream = RasterStream::new(
        source,
        profile(),
        config(),
        RasterLimits::default(),
        Cancellation::never(),
    );
    match stream.next().unwrap().unwrap_err() {
        RasterStreamError::Source(error) => assert_eq!(error.kind(), io::ErrorKind::BrokenPipe),
        other @ RasterStreamError::Raster(_) => panic!("unexpected error: {other}"),
    }
    assert_eq!(stream.state(), StreamState::Failed);
    assert_eq!(stream.completed_layers(), 0);
    assert!(stream.next().is_none());
    assert!(stream.next().is_none());
}
#[test]
fn invalid_config_precedes_source_request() {
    let requests = Cell::new(0);
    let source = std::iter::from_fn(|| {
        requests.set(requests.get() + 1);
        None::<Result<RegionLayer, io::Error>>
    });
    let mut invalid = config();
    invalid.samples_per_axis = 3;
    let mut stream = RasterStream::new(
        source,
        profile(),
        invalid,
        RasterLimits::default(),
        Cancellation::never(),
    );
    assert!(matches!(
        stream.next(),
        Some(Err(RasterStreamError::Raster(RasterError::InvalidConfig)))
    ));
    assert_eq!(requests.get(), 0);
    assert!(stream.next().is_none());
}
#[test]
fn empty_source_requires_final_cancellation_check() {
    let mut predicate = |point| point == Checkpoint::RasterComplete;
    let mut stream = RasterStream::new(
        std::iter::empty::<Result<RegionLayer, io::Error>>(),
        profile(),
        config(),
        RasterLimits::default(),
        Cancellation::new(&mut predicate),
    );
    assert!(matches!(
        stream.next(),
        Some(Err(RasterStreamError::Raster(RasterError::Cancelled(_))))
    ));
    assert_eq!(stream.state(), StreamState::Failed);
    assert!(stream.next().is_none());
}
