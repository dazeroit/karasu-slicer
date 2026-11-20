//! Parallel layer generation (TD-050) against the sequential engine and raster streams: identical
//! images for every thread count, the same first failure in layer order, and cancellation.

use karasu_core::{
    cancellation::{Cancellation, Checkpoint},
    geometry::Point3,
    mesh::TriangleMesh,
    raster::{
        parallel::{with_parallel_raster, BottomRaster, LayerFill, ParallelItem, ParallelRaster},
        stream::{RasterStream, RasterStreamError},
        Compensation, LcdProfile, Orientation, OutsidePolicy, RasterConfig, RasterError,
        RasterLimits,
    },
    slicing::{
        engine::{LayerConfig, LayerEngine, LayerStageLimits},
        grid::BinaryGridSpec,
        regions::RegionFillRule,
        schedule::{
            build_layer_schedule, LayerSchedule, ScheduleEnd, ScheduleLimits, UniformScheduleSpec,
        },
        SectionTransform,
    },
};
use std::{
    f64::consts::TAU,
    sync::atomic::{AtomicUsize, Ordering},
};

/// A closed torus (non-convex, nested section contours) centred at (10, 10, 5).
#[allow(clippy::many_single_char_names)] // Parametric torus notation.
fn torus() -> TriangleMesh {
    let (u, v) = (48u32, 24u32);
    let mut points = Vec::new();
    for i in 0..u {
        let a = TAU * f64::from(i) / f64::from(u);
        for j in 0..v {
            let b = TAU * f64::from(j) / f64::from(v);
            let r = 6.0 + 3.0 * b.cos();
            points.push(Point3::new(
                10.0 + r * a.cos(),
                10.0 + r * a.sin(),
                5.0 + 3.0 * b.sin(),
            ));
        }
    }
    let id = |i: u32, j: u32| (i % u) * v + (j % v);
    let mut triangles = Vec::new();
    for i in 0..u {
        for j in 0..v {
            let (a, b, c, d) = (id(i, j), id(i + 1, j), id(i + 1, j + 1), id(i, j + 1));
            triangles.push([a, b, c]);
            triangles.push([a, c, d]);
        }
    }
    TriangleMesh::try_new(points, triangles).unwrap()
}

fn schedule() -> LayerSchedule {
    build_layer_schedule(
        UniformScheduleSpec::try_new(2.05, 0.1, ScheduleEnd::Count(60)).unwrap(),
        ScheduleLimits {
            max_layers: 60,
            max_payload_bytes: 1 << 20,
        },
    )
    .unwrap()
}

fn layer_config() -> LayerConfig {
    LayerConfig {
        grid: BinaryGridSpec::try_new([0., 0.], -20).unwrap(),
        fill_rule: RegionFillRule::EvenOddStrict,
        stages: LayerStageLimits::default(),
        max_output_payload_bytes: 1 << 26,
    }
}

fn profile() -> LcdProfile {
    LcdProfile::try_new([200, 200], [0., 0.], [20., 20.], Orientation::default()).unwrap()
}

fn raster() -> RasterConfig {
    RasterConfig {
        samples_per_axis: 2,
        grayscale_levels: 256,
        minimum_gray: 0,
        outside: OutsidePolicy::Reject,
        compensation: Compensation::None,
        kept: karasu_core::raster::KeptSampling::Antialiased,
    }
}

fn spec<'a>(
    mesh: &'a TriangleMesh,
    plan: &'a LayerSchedule,
    threads: usize,
    limits: RasterLimits,
) -> ParallelRaster<'a> {
    ParallelRaster {
        mesh,
        schedule: plan,
        transform: SectionTransform::Identity,
        layer_config: layer_config(),
        represented_coordinates: true,
        face_index_bytes: Some(1 << 24),
        vertex_avoiding_bytes: Some(1 << 24),
        profile: profile(),
        raster: raster(),
        bottom: None,
        limits,
        cut: None,
        kept_faces: &[],
        fill: LayerFill::Strict,
        closure: None,
        threads,
        window_bytes: ParallelRaster::DEFAULT_WINDOW_BYTES,
    }
}

/// The sequential reference: the engine plus the raster stream, with the same options.
fn sequential(
    mesh: &TriangleMesh,
    plan: &LayerSchedule,
    limits: RasterLimits,
) -> Vec<ParallelItem> {
    sequential_with(mesh, plan, limits, raster())
}

fn sequential_with(
    mesh: &TriangleMesh,
    plan: &LayerSchedule,
    limits: RasterLimits,
    config: RasterConfig,
) -> Vec<ParallelItem> {
    let layers = LayerEngine::new(
        mesh,
        plan,
        SectionTransform::Identity,
        layer_config(),
        Cancellation::never(),
        None,
    )
    .with_represented_coordinates()
    .with_face_z_index(1 << 24)
    .with_vertex_avoiding_planes(1 << 24);
    RasterStream::new(layers, profile(), config, limits, Cancellation::never()).collect()
}

fn parallel(
    spec: &ParallelRaster<'_>,
    stop: &(dyn Fn(Checkpoint) -> bool + Sync),
) -> Vec<ParallelItem> {
    with_parallel_raster(spec, stop, |images| images.collect())
}

fn never(_: Checkpoint) -> bool {
    false
}

#[test]
fn parallel_images_match_the_sequential_stream_for_every_thread_count() {
    let mesh = torus();
    let plan = schedule();
    let reference = sequential(&mesh, &plan, RasterLimits::default());
    assert_eq!(reference.len(), 60);
    let reference: Vec<_> = reference.into_iter().map(Result::unwrap).collect();
    assert!(reference
        .iter()
        .any(|image| image.stats().nonzero_pixels > 0));
    for threads in [1, 2, 3, 8] {
        let images = parallel(
            &spec(&mesh, &plan, threads, RasterLimits::default()),
            &never,
        );
        assert_eq!(images.len(), reference.len(), "{threads}");
        for (k, (image, expected)) in images.into_iter().zip(&reference).enumerate() {
            let image = image.unwrap();
            assert_eq!(&image, expected, "threads {threads}, layer {k}");
        }
    }
    // A one-layer window still delivers every layer in order.
    let mut narrow = spec(&mesh, &plan, 4, RasterLimits::default());
    narrow.window_bytes = 1;
    let images = parallel(&narrow, &never);
    assert_eq!(images.len(), 60);
    for (image, expected) in images.into_iter().zip(&reference) {
        assert_eq!(&image.unwrap(), expected);
    }
}

#[test]
fn bottom_layers_use_their_own_raster_configuration() {
    let mesh = torus();
    let plan = schedule();
    let eroded = RasterConfig {
        compensation: Compensation::Erode([2, 1]),
        ..raster()
    };
    let reference = sequential(&mesh, &plan, RasterLimits::default());
    let bottom = sequential_with(&mesh, &plan, RasterLimits::default(), eroded);
    for threads in [1, 3] {
        let mut with_bottom = spec(&mesh, &plan, threads, RasterLimits::default());
        with_bottom.bottom = Some(BottomRaster {
            layers: 25,
            raster: eroded,
        });
        let images = parallel(&with_bottom, &never);
        assert_eq!(images.len(), 60);
        for (k, image) in images.into_iter().enumerate() {
            let expected = if k < 25 { &bottom[k] } else { &reference[k] };
            let image = image.unwrap();
            assert_eq!(
                &image,
                expected.as_ref().unwrap(),
                "threads {threads}, layer {k}"
            );
            assert_eq!(
                image.config().compensation,
                expected.as_ref().unwrap().config().compensation
            );
        }
    }
    // The eroded bottom layers differ wherever the torus is sectioned.
    assert!((0..25).any(|k| bottom[k].as_ref().unwrap() != reference[k].as_ref().unwrap()));
    // An invalid bottom configuration fails before any layer.
    let mut invalid = spec(&mesh, &plan, 2, RasterLimits::default());
    invalid.bottom = Some(BottomRaster {
        layers: 1,
        raster: RasterConfig {
            samples_per_axis: 3,
            ..raster()
        },
    });
    let images = parallel(&invalid, &never);
    assert_eq!(images.len(), 1);
    assert!(images[0].is_err());
}

#[test]
fn the_first_failure_in_layer_order_is_reported_as_sequentially() {
    // Planes from 0.05 mm: the first 20 layers lie below the torus and are empty; the first one
    // crossing it needs more edges than this limit and fails.
    let mesh = torus();
    let plan = build_layer_schedule(
        UniformScheduleSpec::try_new(0.05, 0.1, ScheduleEnd::Count(100)).unwrap(),
        ScheduleLimits {
            max_layers: 100,
            max_payload_bytes: 1 << 20,
        },
    )
    .unwrap();
    let limits = RasterLimits {
        max_edges: 60,
        ..RasterLimits::default()
    };
    let reference = sequential(&mesh, &plan, limits);
    let failing = reference.iter().position(Result::is_err).unwrap();
    assert!(failing == 20 && reference.len() == failing + 1, "{failing}");
    for threads in [1, 2, 8] {
        let images = parallel(&spec(&mesh, &plan, threads, limits), &never);
        assert_eq!(images.len(), failing + 1, "{threads}");
        assert!(images[..failing].iter().all(Result::is_ok));
        assert!(matches!(
            images[failing],
            Err(RasterStreamError::Raster(RasterError::Limit { .. }))
        ));
    }
}

#[test]
fn cancellation_ends_the_stream_without_publishing_later_layers() {
    static CALLS: AtomicUsize = AtomicUsize::new(0);
    let mesh = torus();
    let plan = schedule();
    // Cancel after a few hundred checkpoint polls, from whichever thread observes it.
    let stop = |_: Checkpoint| CALLS.fetch_add(1, Ordering::Relaxed) > 400;
    let images = parallel(&spec(&mesh, &plan, 4, RasterLimits::default()), &stop);
    let failed = images.iter().position(Result::is_err).unwrap();
    assert_eq!(images.len(), failed + 1);
    // Later layers are never delivered; the stream ends at the first failure.
    let reference = sequential(&mesh, &plan, RasterLimits::default());
    for (image, expected) in images[..failed].iter().zip(&reference) {
        assert_eq!(image.as_ref().unwrap(), expected.as_ref().unwrap());
    }
}

#[test]
fn an_early_consumer_stop_joins_the_workers() {
    let mesh = torus();
    let plan = schedule();
    let first: Vec<ParallelItem> = with_parallel_raster(
        &spec(&mesh, &plan, 8, RasterLimits::default()),
        &never,
        |images| images.take(3).collect(),
    );
    assert_eq!(first.len(), 3);
    assert!(first.iter().all(Result::is_ok));
}

/// A cut whose square grows with Z, so every layer's cut depends on its sampled plane.
struct GrowingSquare;
impl karasu_core::raster::LayerCut for GrowingSquare {
    fn rings(&self, plane: karasu_core::slicing::schedule::LayerPlane) -> Vec<Vec<[f64; 2]>> {
        let half = 0.5 + 0.3 * plane.z_mm;
        vec![vec![
            [4.0 - half, 10.0 - half],
            [4.0 + half, 10.0 - half],
            [4.0 + half, 10.0 + half],
            [4.0 - half, 10.0 + half],
        ]]
    }
}

#[test]
fn parallel_cuts_match_the_sequential_difference_stream() {
    use karasu_core::raster::stream::DifferenceRasterStream;
    let mesh = torus();
    let plan = schedule();
    let layers = LayerEngine::new(
        &mesh,
        &plan,
        SectionTransform::Identity,
        layer_config(),
        Cancellation::never(),
        None,
    )
    .with_represented_coordinates()
    .with_face_z_index(1 << 24)
    .with_vertex_avoiding_planes(1 << 24);
    let reference: Vec<ParallelItem> = DifferenceRasterStream::new(
        layers,
        &GrowingSquare,
        profile(),
        raster(),
        RasterLimits::default(),
        Cancellation::never(),
    )
    .collect();
    for threads in [1, 5] {
        let mut with_cut = spec(&mesh, &plan, threads, RasterLimits::default());
        with_cut.cut = Some(&GrowingSquare);
        let images = parallel(&with_cut, &never);
        assert_eq!(images.len(), reference.len());
        for (k, (image, expected)) in images.iter().zip(&reference).enumerate() {
            assert_eq!(
                image.as_ref().unwrap(),
                expected.as_ref().unwrap(),
                "{threads} {k}"
            );
        }
    }
}

/// A closed outward box from `min` to `max`, appended to `points` and `triangles`.
fn push_box(min: [f64; 3], max: [f64; 3], points: &mut Vec<Point3>, triangles: &mut Vec<[u32; 3]>) {
    let base = u32::try_from(points.len()).unwrap();
    for k in 0..8u32 {
        let pick = |axis: u32| if k >> axis & 1 == 1 { max } else { min };
        points.push(Point3::new(pick(0)[0], pick(1)[1], pick(2)[2]));
    }
    // Corner k has bit 0 for X, bit 1 for Y and bit 2 for Z; each face is wound outward.
    for [a, b, c, d] in [
        [0, 2, 3, 1],
        [4, 5, 7, 6],
        [0, 1, 5, 4],
        [2, 6, 7, 3],
        [0, 4, 6, 2],
        [1, 3, 7, 5],
    ] {
        triangles.push([base + a, base + b, base + c]);
        triangles.push([base + a, base + c, base + d]);
    }
}

/// A cut square from X 9 to 15 and Y 9 to 11 on every layer.
struct Strip;
impl karasu_core::raster::LayerCut for Strip {
    fn rings(&self, _: karasu_core::slicing::schedule::LayerPlane) -> Vec<Vec<[f64; 2]>> {
        vec![vec![[9.0, 9.0], [15.0, 9.0], [15.0, 11.0], [9.0, 11.0]]]
    }
}

#[test]
fn a_cut_leaves_the_material_of_kept_faces() {
    // TD-123: a model (faces 0 to 11) and a support overlapping it (faces 12 to 23); the strip
    // cut crosses both.
    let (mut points, mut triangles) = (Vec::new(), Vec::new());
    push_box(
        [2.0, 2.0, 0.0],
        [12.0, 18.0, 8.0],
        &mut points,
        &mut triangles,
    );
    push_box(
        [10.0, 8.0, 0.0],
        [14.0, 12.0, 8.0],
        &mut points,
        &mut triangles,
    );
    let mesh = TriangleMesh::try_new(points, triangles).unwrap();
    let plan = schedule();
    let kept: Vec<std::ops::Range<usize>> = std::iter::once(12..24).collect();
    let image = |cut: bool, kept: &[std::ops::Range<usize>], threads| {
        let mut spec = spec(&mesh, &plan, threads, RasterLimits::default());
        spec.fill = LayerFill::Winding(karasu_core::slicing::winding::WindingRule::Positive);
        spec.cut = cut.then_some(&Strip as &(dyn karasu_core::raster::LayerCut + Sync));
        spec.kept_faces = kept;
        let images = parallel(&spec, &never);
        images
            .into_iter()
            .nth(10)
            .unwrap()
            .unwrap()
            .to_pixels()
            .unwrap()
    };
    // Pixels are 0.1 mm, X along rows: positions are in tenths of a millimetre.
    let at = |pixels: &[u8], x: usize, y: usize| pixels[y * 200 + x];
    let whole = image(false, &[], 1);
    let cut_all = image(true, &[], 1);
    for threads in [1, 4] {
        let leaving = image(true, &kept, threads);
        // The model alone, in the strip: cut either way.
        assert_eq!(at(&whole, 95, 100), 255);
        assert_eq!(at(&leaving, 95, 100), 0);
        // The support, in the strip: cut before, kept now.
        assert_eq!(at(&whole, 130, 100), 255);
        assert_eq!(at(&cut_all, 130, 100), 0);
        assert_eq!(at(&leaving, 130, 100), 255);
        // Where the support overlaps the model, in the strip: its material stays.
        assert_eq!(at(&leaving, 110, 100), 255);
        // Outside the strip nothing changes.
        assert_eq!(at(&leaving, 50, 50), 255);
        assert_eq!(at(&leaving, 130, 115), 255);
        // Beyond the strip and both boxes: still empty.
        assert_eq!(at(&leaving, 145, 100), 0);
    }
    // Without a cut, kept faces change nothing.
    assert_eq!(image(false, &kept, 2), whole);
    // Strict layers cannot tell kept faces apart: refused, not ignored.
    let mut strict = spec(&mesh, &plan, 1, RasterLimits::default());
    strict.kept_faces = &kept;
    assert!(parallel(&strict, &never)[0].is_err());
}

/// A cut of cut groups (TD-166): the strip removes the material of group `strip_group` alone, the
/// other groups having no rings.
struct GroupedStrip {
    groups: Vec<karasu_core::slicing::winding::FaceGroup>,
    strip_group: usize,
    count: usize,
}
impl karasu_core::raster::LayerCut for GroupedStrip {
    fn rings(&self, plane: karasu_core::slicing::schedule::LayerPlane) -> Vec<Vec<[f64; 2]>> {
        Strip.rings(plane)
    }
    fn groups(&self) -> &[karasu_core::slicing::winding::FaceGroup] {
        &self.groups
    }
    fn group_rings(
        &self,
        plane: karasu_core::slicing::schedule::LayerPlane,
    ) -> Vec<Vec<Vec<[f64; 2]>>> {
        (0..self.count)
            .map(|g| {
                if g == self.strip_group {
                    Strip.rings(plane)
                } else {
                    Vec::new()
                }
            })
            .collect()
    }
}

/// Group `group` over the box pushed `index`-th (each box has 12 faces and 8 vertices).
fn box_group(group: usize, index: usize) -> karasu_core::slicing::winding::FaceGroup {
    karasu_core::slicing::winding::FaceGroup {
        group,
        faces: 12 * index..12 * (index + 1),
        vertices: 8 * index..8 * (index + 1),
    }
}

/// The images of `mesh` in winding layers with `cut` and `kept` faces, on `threads` threads.
fn winding_images(
    mesh: &TriangleMesh,
    plan: &LayerSchedule,
    cut: Option<&(dyn karasu_core::raster::LayerCut + Sync)>,
    kept: &[std::ops::Range<usize>],
    config: RasterConfig,
    threads: usize,
) -> Vec<ParallelItem> {
    let mut spec = spec(mesh, plan, threads, RasterLimits::default());
    spec.fill = LayerFill::Winding(karasu_core::slicing::winding::WindingRule::Positive);
    spec.cut = cut;
    spec.kept_faces = kept;
    spec.raster = config;
    parallel(&spec, &never)
}

#[test]
fn a_grouped_cut_removes_only_its_own_groups_material() {
    // TD-166: model A (group 1) and model B (group 0) beside it; the strip, A's cut, crosses
    // both.
    let (mut points, mut triangles) = (Vec::new(), Vec::new());
    push_box(
        [2.0, 2.0, 0.0],
        [10.0, 18.0, 8.0],
        &mut points,
        &mut triangles,
    );
    push_box(
        [11.0, 8.0, 0.0],
        [16.0, 12.0, 8.0],
        &mut points,
        &mut triangles,
    );
    let mesh = TriangleMesh::try_new(points, triangles).unwrap();
    let plan = schedule();
    let grouped = GroupedStrip {
        groups: vec![box_group(1, 0)],
        strip_group: 1,
        count: 2,
    };
    let at = |pixels: &[u8], x: usize, y: usize| pixels[y * 200 + x];
    let layer = |cut: &(dyn karasu_core::raster::LayerCut + Sync), threads| {
        winding_images(&mesh, &plan, Some(cut), &[], raster(), threads)
            .into_iter()
            .nth(10)
            .unwrap()
            .unwrap()
            .to_pixels()
            .unwrap()
    };
    let one_cut = layer(&Strip, 1);
    for threads in [1, 4] {
        let own = layer(&grouped, threads);
        // A in the strip: cut either way.
        assert_eq!(at(&one_cut, 95, 100), 0);
        assert_eq!(at(&own, 95, 100), 0);
        // B in the strip: cut by one cut, whole when the strip is A's alone.
        assert_eq!(at(&one_cut, 130, 100), 0);
        assert_eq!(at(&own, 130, 100), 255);
        // Outside the strip nothing changes, and between the boxes stays empty.
        assert_eq!(at(&own, 50, 50), 255);
        assert_eq!(at(&own, 130, 85), 255);
        assert_eq!(at(&own, 105, 100), 0);
    }
}

#[test]
fn grouped_cuts_that_reach_no_other_group_slice_as_one_cut() {
    // TD-166: model A and a support overlapping it in group 1 with the strip, model B in group
    // 0 clear of the strip. Every layer equals one cut's, anti-aliased or with binary supports.
    let (mut points, mut triangles) = (Vec::new(), Vec::new());
    push_box(
        [2.03, 2.07, 0.0],
        [10.05, 18.01, 8.0],
        &mut points,
        &mut triangles,
    );
    push_box(
        [12.02, 2.04, 0.0],
        [16.06, 6.09, 8.0],
        &mut points,
        &mut triangles,
    );
    push_box(
        [6.01, 8.03, 0.0],
        [9.57, 12.02, 8.0],
        &mut points,
        &mut triangles,
    );
    let mesh = TriangleMesh::try_new(points, triangles).unwrap();
    let plan = schedule();
    let kept: Vec<std::ops::Range<usize>> = std::iter::once(24..36).collect();
    let grouped = GroupedStrip {
        groups: vec![box_group(1, 0), box_group(1, 2)],
        strip_group: 1,
        count: 2,
    };
    for kept_sampling in [
        karasu_core::raster::KeptSampling::Antialiased,
        karasu_core::raster::KeptSampling::Binary,
    ] {
        let config = RasterConfig {
            samples_per_axis: 4,
            kept: kept_sampling,
            ..raster()
        };
        let reference = winding_images(&mesh, &plan, Some(&Strip), &kept, config, 1);
        let images = winding_images(&mesh, &plan, Some(&grouped), &kept, config, 3);
        assert_eq!(images.len(), reference.len());
        for (k, (image, expected)) in images.iter().zip(&reference).enumerate() {
            let (image, expected) = (image.as_ref().unwrap(), expected.as_ref().unwrap());
            assert_eq!(image.mask(), expected.mask(), "{kept_sampling:?} layer {k}");
            assert_eq!(
                image.stats(),
                expected.stats(),
                "{kept_sampling:?} layer {k}"
            );
        }
    }
}

#[test]
fn overlapping_groups_are_united_per_sample() {
    // TD-166: two overlapping models in different groups, no rings: the same gray levels as
    // their union in one group, where their edges cross pixels.
    let (mut points, mut triangles) = (Vec::new(), Vec::new());
    push_box(
        [2.03, 2.07, 0.0],
        [10.05, 18.01, 8.0],
        &mut points,
        &mut triangles,
    );
    push_box(
        [8.02, 4.04, 0.0],
        [14.06, 12.09, 8.0],
        &mut points,
        &mut triangles,
    );
    let mesh = TriangleMesh::try_new(points, triangles).unwrap();
    let plan = schedule();
    let grouped = GroupedStrip {
        groups: vec![box_group(2, 1)],
        strip_group: 5,
        count: 3,
    };
    let config = RasterConfig {
        samples_per_axis: 4,
        ..raster()
    };
    let reference = winding_images(&mesh, &plan, None, &[], config, 1);
    let images = winding_images(&mesh, &plan, Some(&grouped), &[], config, 2);
    for (image, expected) in images.iter().zip(&reference) {
        let (image, expected) = (image.as_ref().unwrap(), expected.as_ref().unwrap());
        assert_eq!(image.mask(), expected.mask());
    }
    // Some pixel is gray, so the union was sampled, not its levels.
    let pixels = reference[10].as_ref().unwrap().to_pixels().unwrap();
    assert!(pixels.iter().any(|&p| p != 0 && p != 255));
}

#[test]
fn closure_chords_follow_their_bodys_group() {
    // TD-166 with TD-054: model A, open on its +X side, in group 1 with the strip; model B in
    // group 0. Each closure chord must close A's sections in A's group, or its winding breaks.
    let (mut points, mut triangles) = (Vec::new(), Vec::new());
    push_box(
        [2.0, 2.0, 0.0],
        [10.0, 18.0, 8.0],
        &mut points,
        &mut triangles,
    );
    // The +X face of the first box (corners 1, 3, 7, 5) is its last two triangles.
    triangles.truncate(10);
    push_box(
        [11.0, 8.0, 0.0],
        [16.0, 12.0, 8.0],
        &mut points,
        &mut triangles,
    );
    let mesh = TriangleMesh::try_new(points, triangles).unwrap();
    let plan = schedule();
    let grouped = GroupedStrip {
        groups: vec![karasu_core::slicing::winding::FaceGroup {
            group: 1,
            faces: 0..10,
            vertices: 0..8,
        }],
        strip_group: 1,
        count: 2,
    };
    let mut spec = spec(&mesh, &plan, 2, RasterLimits::default());
    spec.fill = LayerFill::Winding(karasu_core::slicing::winding::WindingRule::Positive);
    spec.closure = Some(karasu_core::slicing::winding::SectionClosure { max_gap_mm: 20.0 });
    spec.cut = Some(&grouped);
    let image = parallel(&spec, &never)
        .into_iter()
        .nth(10)
        .unwrap()
        .unwrap();
    assert_eq!(image.closure().unwrap().chords, 1);
    let pixels = image.to_pixels().unwrap();
    let at = |x: usize, y: usize| pixels[y * 200 + x];
    // A is closed and cut in the strip; B keeps its material there.
    assert_eq!(at(50, 50), 255);
    assert_eq!(at(95, 100), 0);
    assert_eq!(at(130, 100), 255);
}

#[test]
fn strict_layers_refuse_a_grouped_cut() {
    let (mut points, mut triangles) = (Vec::new(), Vec::new());
    push_box(
        [2.0, 2.0, 0.0],
        [10.0, 18.0, 8.0],
        &mut points,
        &mut triangles,
    );
    let mesh = TriangleMesh::try_new(points, triangles).unwrap();
    let plan = schedule();
    let grouped = GroupedStrip {
        groups: vec![box_group(1, 0)],
        strip_group: 1,
        count: 2,
    };
    let mut strict = spec(&mesh, &plan, 1, RasterLimits::default());
    strict.cut = Some(&grouped);
    let images = parallel(&strict, &never);
    assert!(matches!(
        &images[0],
        Err(RasterStreamError::Source(e))
            if e.cause == karasu_core::slicing::engine::LayerFailure::FaceGroupsNeedWinding
    ));
}
