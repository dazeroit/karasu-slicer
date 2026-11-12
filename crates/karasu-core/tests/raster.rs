//! Independent analytic midpoint-sampling oracle through the public M3-to-M4 route.
use karasu_core::{
    cancellation::{Cancellation, Checkpoint},
    geometry::Point3,
    mesh::TriangleMesh,
    raster::*,
    slicing::{
        engine::*, grid::BinaryGridSpec, regions::RegionFillRule, schedule::*, SectionTransform,
    },
};

fn layer(rings: &[Vec<[f64; 2]>], close: bool) -> RegionLayer {
    let mut points = Vec::new();
    let mut triangles = Vec::new();
    for ring in rings {
        let base = u32::try_from(points.len()).unwrap();
        for &[x, y] in ring {
            points.extend([Point3::new(x, y, -1.), Point3::new(x, y, 1.)]);
        }
        let count = if close { ring.len() } else { ring.len() - 1 };
        for i in 0..count {
            let a = base + 2 * u32::try_from(i).unwrap();
            let b = base + 2 * u32::try_from((i + 1) % ring.len()).unwrap();
            triangles.extend([[a, b, b + 1], [a, b + 1, a + 1]]);
        }
    }
    let mesh = TriangleMesh::try_new(points, triangles).unwrap();
    let schedule = build_layer_schedule(
        UniformScheduleSpec::try_new(0., 0.25, ScheduleEnd::Count(1)).unwrap(),
        ScheduleLimits {
            max_layers: 1,
            max_payload_bytes: 1024,
        },
    )
    .unwrap();
    let config = LayerConfig {
        grid: BinaryGridSpec::try_new([0., 0.], -8).unwrap(),
        fill_rule: RegionFillRule::EvenOddStrict,
        stages: LayerStageLimits::default(),
        max_output_payload_bytes: 1 << 24,
    };
    LayerEngine::new(
        &mesh,
        &schedule,
        SectionTransform::Identity,
        config,
        Cancellation::never(),
        None,
    )
    .next()
    .unwrap()
    .unwrap()
}
fn rectangle(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<[f64; 2]> {
    vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]]
}
fn config(n: u8, levels: u16) -> RasterConfig {
    RasterConfig {
        samples_per_axis: n,
        grayscale_levels: levels,
        minimum_gray: 0,
        outside: OutsidePolicy::Reject,
        compensation: Compensation::None,
        kept: karasu_core::raster::KeptSampling::Antialiased,
    }
}
fn profile(o: Orientation) -> LcdProfile {
    LcdProfile::try_new([8, 6], [0., 0.], [8., 6.], o).unwrap()
}
fn inside(p: [f64; 2], rings: &[Vec<[f64; 2]>]) -> bool {
    let mut material = false;
    for ring in rings {
        for i in 0..ring.len() {
            let a = ring[i];
            let b = ring[(i + 1) % ring.len()];
            if (a[1] > p[1]) != (b[1] > p[1])
                && p[0] < a[0] + (p[1] - a[1]) * (b[0] - a[0]) / (b[1] - a[1])
            {
                material = !material;
            }
        }
    }
    material
}
fn oracle(rings: &[Vec<[f64; 2]>], p: LcdProfile, c: RasterConfig) -> Vec<u8> {
    let mapped: Vec<Vec<[f64; 2]>> = rings
        .iter()
        .map(|ring| {
            ring.iter()
                .map(|&point| p.world_to_pixel(point).unwrap())
                .collect()
        })
        .collect();
    let mut out = Vec::new();
    let n = u32::from(c.samples_per_axis);
    let samples = n * n;
    let levels = u32::from(c.grayscale_levels) - 1;
    for y in 0..p.dimensions()[1] {
        for x in 0..p.dimensions()[0] {
            let mut count = 0;
            for sy in 0..n {
                for sx in 0..n {
                    let point = [
                        f64::from(x) + (f64::from(sx) + 0.5) / f64::from(n),
                        f64::from(y) + (f64::from(sy) + 0.5) / f64::from(n),
                    ];
                    count += u32::from(inside(point, &mapped));
                }
            }
            let level = (2 * count * levels + samples) / (2 * samples);
            out.push(u8::try_from((2 * level * 255 + levels) / (2 * levels)).unwrap());
        }
    }
    out
}
#[test]
fn asymmetric_hole_and_island_match_fixed_bitmap_and_statistics() {
    let rings = vec![
        rectangle(1., 1., 7., 5.),
        rectangle(2., 2., 5., 4.),
        rectangle(3., 2.5, 4., 3.5),
    ];
    let image = rasterize_layer(
        &layer(&rings, true),
        profile(Orientation::default()),
        config(1, 256),
        RasterLimits::default(),
    )
    .unwrap();
    let expected = [
        "........", ".######.", ".#.#.##.", ".#...##.", ".######.", "........",
    ];
    let bytes: Vec<u8> = expected
        .iter()
        .flat_map(|row| row.bytes().map(|v| if v == b'#' { 255 } else { 0 }))
        .collect();
    // Island edges at sample centers follow the documented lower/left inclusive convention.
    assert_eq!(image.to_pixels().unwrap(), bytes);
    assert_eq!(
        image.stats().intensity_sum,
        image
            .to_pixels()
            .unwrap()
            .iter()
            .map(|&n| u64::from(n))
            .sum::<u64>()
    );
    assert_eq!(image.stats().bounds, Some([[1, 1], [7, 5]]));
}
#[test]
fn all_orientations_and_aa_grayscale_levels_match_independent_point_oracle() {
    let rings = vec![vec![[0.25, 0.75], [7.25, 1.25], [2.75, 5.25]]];
    let source = layer(&rings, true);
    for swap_xy in [false, true] {
        for flip_x in [false, true] {
            for flip_y in [false, true] {
                let p = profile(Orientation {
                    swap_xy,
                    flip_x,
                    flip_y,
                });
                for n in [1, 2, 4, 8, 16] {
                    for levels in [2, 3, 4, 16, 256] {
                        let c = config(n, levels);
                        let image =
                            rasterize_layer(&source, p, c, RasterLimits::default()).unwrap();
                        assert_eq!(
                            image.to_pixels().unwrap(),
                            oracle(&rings, p, c),
                            "orientation {:?}, N={n}, L={levels}",
                            p.orientation()
                        );
                    }
                }
                for point in [[0., 0.], [0.375, 1.25], [8., 6.], [-1., 7.]] {
                    let back = p.pixel_to_world(p.world_to_pixel(point).unwrap()).unwrap();
                    for axis in 0..2 {
                        assert!((back[axis] - point[axis]).abs() < 1e-12);
                    }
                }
            }
        }
    }
}
#[test]
fn clipping_uses_original_crossings_and_reject_is_explicit() {
    let rings = vec![vec![[-2., -1.], [12., 2.], [1., 8.]]];
    let source = layer(&rings, true);
    let p = profile(Orientation::default());
    assert_eq!(
        rasterize_layer(&source, p, config(4, 256), RasterLimits::default()).unwrap_err(),
        RasterError::OutsidePanel
    );
    let mut c = config(4, 256);
    c.outside = OutsidePolicy::Clip;
    assert_eq!(
        rasterize_layer(&source, p, c, RasterLimits::default())
            .unwrap()
            .to_pixels()
            .unwrap(),
        oracle(&rings, p, c)
    );
}
#[test]
fn unresolved_input_and_each_resource_limit_fail_without_image() {
    let rings = vec![rectangle(1., 1., 7., 5.)];
    let p = profile(Orientation::default());
    let c = config(2, 256);
    assert_eq!(
        rasterize_layer(&layer(&rings, false), p, c, RasterLimits::default()).unwrap_err(),
        RasterError::UnresolvedGeometry
    );
    let source = layer(&rings, true);
    for (resource, limits) in [
        (
            "pixels",
            RasterLimits {
                max_pixels: 0,
                ..RasterLimits::default()
            },
        ),
        (
            "edges",
            RasterLimits {
                max_edges: 0,
                ..RasterLimits::default()
            },
        ),
        (
            "payload bytes",
            RasterLimits {
                max_payload_bytes: 0,
                ..RasterLimits::default()
            },
        ),
        (
            "work",
            RasterLimits {
                max_work: 0,
                ..RasterLimits::default()
            },
        ),
    ] {
        assert!(
            matches!(rasterize_layer(&source,p,c,limits),Err(RasterError::Limit{resource:r,..}) if r==resource),
            "resource {resource}"
        );
    }
    let mut invalid = c;
    invalid.samples_per_axis = 3;
    let mut predicate = |checkpoint| checkpoint == Checkpoint::RasterStart;
    assert!(matches!(
        rasterize_layer_with_cancellation(
            &source,
            p,
            invalid,
            RasterLimits::default(),
            &mut Cancellation::new(&mut predicate)
        ),
        Err(RasterError::Cancelled(_))
    ));
    assert_eq!(
        rasterize_layer(&source, p, invalid, RasterLimits::default()).unwrap_err(),
        RasterError::InvalidConfig
    );
}
#[test]
fn grayscale_compensation_matches_naive_zero_exterior_oracle() {
    let rings = vec![vec![[0., 0.], [6., 0.], [1., 5.]]];
    let source = layer(&rings, true);
    let p = profile(Orientation::default());
    let c = config(4, 16);
    let original = rasterize_layer(&source, p, c, RasterLimits::default()).unwrap();
    for dilate in [false, true] {
        for radii in [[0, 0], [1, 2], [12, 9]] {
            let mut settings = c;
            settings.compensation = if dilate {
                Compensation::Dilate(radii)
            } else {
                Compensation::Erode(radii)
            };
            let actual = rasterize_layer(&source, p, settings, RasterLimits::default()).unwrap();
            let mut expected = Vec::new();
            for y in 0_i32..6 {
                for x in 0_i32..8 {
                    let mut value = if dilate { 0 } else { 255 };
                    for dy in -i32::from(radii[1])..=i32::from(radii[1]) {
                        for dx in -i32::from(radii[0])..=i32::from(radii[0]) {
                            let (xx, yy) = (x + dx, y + dy);
                            let v = if (0..8).contains(&xx) && (0..6).contains(&yy) {
                                original.to_pixels().unwrap()[usize::try_from(yy * 8 + xx).unwrap()]
                            } else {
                                0
                            };
                            value = if dilate { value.max(v) } else { value.min(v) };
                        }
                    }
                    expected.push(value);
                }
            }
            assert_eq!(
                actual.to_pixels().unwrap(),
                expected,
                "dilate={dilate}, radii={radii:?}"
            );
        }
    }
}

#[test]
fn stream_is_lazy_fused_and_preserves_source_errors() {
    use karasu_core::{
        raster::stream::{RasterStream, RasterStreamError},
        slicing::engine::StreamState,
    };
    use std::{cell::Cell, error::Error, io};
    let requests = Cell::new(0);
    let source = std::iter::from_fn(|| {
        requests.set(requests.get() + 1);
        Some(Err::<RegionLayer, _>(io::Error::other("upstream marker")))
    });
    let mut stream = RasterStream::new(
        source,
        profile(Orientation::default()),
        config(1, 256),
        RasterLimits::default(),
        Cancellation::never(),
    );
    assert_eq!(requests.get(), 0);
    let failure = stream.next().unwrap().unwrap_err();
    assert!(
        matches!(&failure,RasterStreamError::Source(error) if error.to_string()=="upstream marker")
    );
    assert!(failure.source().is_some());
    assert_eq!(stream.state(), StreamState::Failed);
    assert_eq!(stream.completed_layers(), 0);
    assert!(stream.next().is_none());
    assert!(stream.next().is_none());
    assert_eq!(requests.get(), 1);
    let source = vec![Ok::<_, io::Error>(layer(
        &[rectangle(1., 1., 3., 4.)],
        true,
    ))]
    .into_iter();
    let mut stream = RasterStream::new(
        source,
        profile(Orientation::default()),
        config(1, 256),
        RasterLimits::default(),
        Cancellation::never(),
    );
    assert!(stream.next().unwrap().is_ok());
    assert_eq!(stream.completed_layers(), 1);
    assert!(stream.next().is_none());
    assert_eq!(stream.state(), StreamState::Complete);
    assert!(stream.next().is_none());
}
#[test]
fn stream_invalid_configuration_does_not_consume_source() {
    use karasu_core::raster::stream::RasterStream;
    let mut invalid = config(1, 256);
    invalid.grayscale_levels = 1;
    let source = std::iter::from_fn(|| -> Option<Result<RegionLayer, std::io::Error>> {
        panic!("invalid configuration must precede source work")
    });
    let mut stream = RasterStream::new(
        source,
        profile(Orientation::default()),
        invalid,
        RasterLimits::default(),
        Cancellation::never(),
    );
    assert!(stream.next().unwrap().is_err());
    assert!(stream.next().is_none());
}

#[test]
fn cancellation_at_each_raster_phase_prevents_successful_publication() {
    let source = layer(&[rectangle(1., 1., 7., 5.)], true);
    for expected in [
        Checkpoint::RasterStart,
        Checkpoint::RasterRows,
        Checkpoint::RasterComplete,
    ] {
        let mut predicate = |checkpoint| checkpoint == expected;
        let result = rasterize_layer_with_cancellation(
            &source,
            profile(Orientation::default()),
            config(4, 256),
            RasterLimits::default(),
            &mut Cancellation::new(&mut predicate),
        );
        assert!(
            matches!(result, Err(RasterError::Cancelled(error)) if error.checkpoint == expected)
        );
    }
}

#[test]
fn cut_rings_are_preflighted_at_their_reserved_capacity() {
    let body = layer(&[rectangle(1., 1., 7., 5.)], true);
    let p = profile(Orientation::default());
    let c = config(1, 2);
    let limits = RasterLimits {
        max_payload_bytes: 1 << 16,
        ..RasterLimits::default()
    };
    let triangle = vec![[3., 2.], [5., 2.], [4., 4.]];
    let mut never = Cancellation::never();
    let plain = rasterize_layer_minus_rings_with_cancellation(
        &body,
        std::slice::from_ref(&triangle),
        p,
        c,
        limits,
        &mut never,
    );
    assert!(plain.is_ok(), "{plain:?}");
    // The same three effective edges, but 20,000 repeated vertices: the edge buffer reserves one
    // slot per ring vertex, so the payload preflight must count all of them before reserving.
    let mut repeated = triangle;
    repeated.extend(std::iter::repeat_n([4., 4.], 20_000));
    assert!(matches!(
        rasterize_layer_minus_rings_with_cancellation(&body, &[repeated], p, c, limits, &mut never),
        Err(RasterError::Limit {
            resource: "payload bytes",
            ..
        })
    ));
}

#[test]
fn default_limits_rasterize_a_full_anycubic_p1_panel() {
    // The 13312 x 5120 Photon P1 panel (68,157,440 pixels) is the main PP1 target; the CLI uses
    // the default limits, so they must admit it (a 1 mm pitch keeps the layer geometry simple).
    let body = layer(&[rectangle(1., 1., 7., 5.)], true);
    let p1 = LcdProfile::try_new(
        [13_312, 5_120],
        [0., 0.],
        [13_312., 5_120.],
        Orientation::default(),
    )
    .unwrap();
    let image = rasterize_layer(&body, p1, config(1, 2), RasterLimits::default()).unwrap();
    assert_eq!(image.stats().nonzero_pixels, 24);
}
