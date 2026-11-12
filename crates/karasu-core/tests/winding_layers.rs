//! Winding-union layers (TD-050): identical masks to the strict pipeline on every input it accepts
//! (U1), and agreement with an independent oracle on inputs it refuses: overlapping, touching and
//! self-intersecting bodies, cavities, open or flipped surfaces and random overlaps (U2-U6).
//!
//! The oracle is the generalized winding number of the closed source surface at each pixel centre,
//! a sum of solid angles that shares no code with the section, balance or scanline.
use karasu_core::{
    cancellation::Checkpoint,
    geometry::Point3,
    mesh::{
        stl::{import_stl_path, StlOptions, StlUnitPolicy},
        TriangleMesh,
    },
    raster::{
        parallel::{with_parallel_raster, LayerFill, ParallelItem, ParallelRaster},
        stream::RasterStreamError,
        Compensation, LcdProfile, Orientation, OutsidePolicy, RasterConfig, RasterError,
        RasterImage, RasterLimits, WindingRange,
    },
    slicing::{
        engine::{LayerConfig, LayerEngineError, LayerFailure, LayerStageLimits},
        grid::BinaryGridSpec,
        regions::RegionFillRule,
        schedule::{
            build_layer_schedule, LayerSchedule, ScheduleEnd, ScheduleLimits, UniformScheduleSpec,
        },
        winding::WindingRule,
        SectionTransform,
    },
};
use std::{f64::consts::PI, path::Path};

#[path = "../../../tools/reference-fixtures/src/geometry.rs"]
#[allow(dead_code)] // The shared generator includes shapes this test does not use.
mod geometry;

const RULES: [WindingRule; 3] = [
    WindingRule::Positive,
    WindingRule::NonZero,
    WindingRule::EvenOdd,
];

fn never(_: Checkpoint) -> bool {
    false
}

fn triangle_mesh(points: Vec<[f64; 3]>, triangles: Vec<[u32; 3]>) -> TriangleMesh {
    TriangleMesh::try_new(
        points
            .into_iter()
            .map(|[x, y, z]| Point3::new(x, y, z))
            .collect(),
        triangles,
    )
    .unwrap()
}

fn from_fixture(mesh: &geometry::Mesh) -> TriangleMesh {
    triangle_mesh(
        mesh.vertices.iter().map(|v| [v.x, v.y, v.z]).collect(),
        mesh.triangles
            .iter()
            .map(|t| t.map(|i| u32::try_from(i).unwrap()))
            .collect(),
    )
}

/// Several fixture bodies as one source mesh, without merging any vertex.
fn bodies(parts: &[geometry::Mesh]) -> TriangleMesh {
    from_fixture(&geometry::merge_meshes(parts))
}

fn cuboid(min: [f64; 3], max: [f64; 3], inward: bool) -> geometry::Mesh {
    let v = |p: [f64; 3]| geometry::Vec3::new(p[0], p[1], p[2]);
    geometry::box_mesh(v(min), v(max), inward)
}

/// `mesh` rotated by `yaw` about +Z, then `pitch` about +X, around `centre`.
fn rotated(mut mesh: geometry::Mesh, yaw: f64, pitch: f64, centre: [f64; 3]) -> geometry::Mesh {
    for v in &mut mesh.vertices {
        let (x, y, z) = (v.x - centre[0], v.y - centre[1], v.z - centre[2]);
        let (x, y) = (x * yaw.cos() - y * yaw.sin(), x * yaw.sin() + y * yaw.cos());
        let (y, z) = (
            y * pitch.cos() - z * pitch.sin(),
            y * pitch.sin() + z * pitch.cos(),
        );
        *v = geometry::Vec3::new(x + centre[0], y + centre[1], z + centre[2]);
    }
    mesh
}

fn sphere(radius: f64, centre: [f64; 3], inward: bool) -> geometry::Mesh {
    let c = geometry::Vec3::new(centre[0], centre[1], centre[2]);
    geometry::icosphere(radius, c, 2, inward)
}

/// A prism over a counterclockwise pentagram: one closed self-intersecting shell whose section has
/// winding number 2 in the central pentagon and 1 in the points.
fn pentagram_prism(centre: [f64; 2], radius: f64, z: [f64; 2]) -> TriangleMesh {
    let mut points = Vec::new();
    for level in z {
        for k in 0..5 {
            let angle = PI / 2. + f64::from(k) * 4. * PI / 5.;
            points.push([
                centre[0] + radius * angle.cos(),
                centre[1] + radius * angle.sin(),
                level,
            ]);
        }
    }
    points.push([centre[0], centre[1], z[0]]);
    points.push([centre[0], centre[1], z[1]]);
    let mut triangles = Vec::new();
    for i in 0..5 {
        let j = (i + 1) % 5;
        triangles.push([i, j, j + 5]);
        triangles.push([i, j + 5, i + 5]);
        triangles.push([10, j, i]);
        triangles.push([11, i + 5, j + 5]);
    }
    triangle_mesh(points, triangles)
}

fn plan(first: f64, step: f64, count: u64) -> LayerSchedule {
    build_layer_schedule(
        UniformScheduleSpec::try_new(first, step, ScheduleEnd::Count(count)).unwrap(),
        ScheduleLimits {
            max_layers: 1000,
            max_payload_bytes: 1 << 20,
        },
    )
    .unwrap()
}

fn layer_config(exponent: i16) -> LayerConfig {
    let mut config = LayerConfig {
        grid: BinaryGridSpec::try_new([0., 0.], exponent).unwrap(),
        fill_rule: RegionFillRule::EvenOddStrict,
        stages: LayerStageLimits::default(),
        max_output_payload_bytes: 128 << 20,
    };
    config.stages.validity.max_pairs = 16_000_000;
    config
}

fn binary() -> RasterConfig {
    RasterConfig {
        samples_per_axis: 1,
        grayscale_levels: 256,
        minimum_gray: 0,
        outside: OutsidePolicy::Reject,
        compensation: Compensation::None,
        kept: karasu_core::raster::KeptSampling::Antialiased,
    }
}

/// 40 x 30 pixels of 0.5 mm: pixel centres lie on odd quarter millimetres, never on the half
/// millimetre grid the box fixtures use.
fn small_panel(orientation: Orientation) -> LcdProfile {
    LcdProfile::try_new([40, 30], [0., 0.], [20., 15.], orientation).unwrap()
}

#[allow(clippy::too_many_arguments)] // Test helper; each input is distinct.
fn run(
    mesh: &TriangleMesh,
    schedule: &LayerSchedule,
    fill: LayerFill,
    profile: LcdProfile,
    raster: RasterConfig,
    exponent: i16,
    threads: usize,
) -> Vec<ParallelItem> {
    let spec = ParallelRaster {
        mesh,
        schedule,
        transform: SectionTransform::Identity,
        layer_config: layer_config(exponent),
        represented_coordinates: true,
        face_index_bytes: Some(1 << 26),
        vertex_avoiding_bytes: Some(1 << 26),
        profile,
        raster,
        bottom: None,
        limits: RasterLimits::default(),
        cut: None,
        kept_faces: &[],
        fill,
        closure: None,
        threads,
        window_bytes: ParallelRaster::DEFAULT_WINDOW_BYTES,
    };
    with_parallel_raster(&spec, &never, |images| images.collect())
}

fn images(items: Vec<ParallelItem>) -> Vec<RasterImage> {
    items
        .into_iter()
        .map(|item| item.unwrap_or_else(|e| panic!("{e}")))
        .collect()
}

/// Generalized winding number of the closed surface at `p`: the total solid angle over 4 pi.
fn winding_number(mesh: &TriangleMesh, p: [f64; 3]) -> f64 {
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let total: f64 = mesh
        .triangles()
        .iter()
        .map(|t| {
            let [a, b, c] = t.map(|i| {
                let v = mesh.vertices()[i as usize];
                [v.x - p[0], v.y - p[1], v.z - p[2]]
            });
            let cross = [
                b[1] * c[2] - b[2] * c[1],
                b[2] * c[0] - b[0] * c[2],
                b[0] * c[1] - b[1] * c[0],
            ];
            let length = |v: [f64; 3]| dot(v, v).sqrt();
            let (la, lb, lc) = (length(a), length(b), length(c));
            let denominator = la * lb * lc + dot(a, b) * lc + dot(a, c) * lb + dot(b, c) * la;
            2. * dot(a, cross).atan2(denominator)
        })
        .sum();
    total / (4. * PI)
}

/// No scheduled plane lies on a vertex height, so the layer is sectioned at its scheduled Z.
fn assert_off_vertices(mesh: &TriangleMesh, schedule: &LayerSchedule) {
    for plane in schedule.planes() {
        assert!(
            mesh.vertices()
                .iter()
                .all(|v| v.z.total_cmp(&plane.z_mm).is_ne()),
            "plane {} meets a vertex height",
            plane.z_mm
        );
    }
}

/// Every unambiguous pixel of every layer equals the oracle under each rule (panel clipping
/// enabled); returns, per rule, how many pixels were compared and how many were material.
fn assert_matches_oracle(
    mesh: &TriangleMesh,
    schedule: &LayerSchedule,
    rules: &[WindingRule],
    profile: LcdProfile,
) -> Vec<(usize, usize)> {
    assert_off_vertices(mesh, schedule);
    let clip = RasterConfig {
        outside: OutsidePolicy::Clip,
        ..binary()
    };
    let layers: Vec<Vec<RasterImage>> = rules
        .iter()
        .map(|&rule| {
            images(run(
                mesh,
                schedule,
                LayerFill::Winding(rule),
                profile,
                clip,
                -20,
                3,
            ))
        })
        .collect();
    let [width, height] = profile.dimensions();
    let mut counts = vec![(0, 0); rules.len()];
    for (k, plane) in schedule.planes().iter().enumerate() {
        for y in 0..height {
            for x in 0..width {
                let [wx, wy] = profile
                    .pixel_to_world([f64::from(x) + 0.5, f64::from(y) + 0.5])
                    .unwrap();
                let w = winding_number(mesh, [wx, wy, plane.z_mm]);
                let rounded = w.round();
                if (w - rounded).abs() > 1e-6 {
                    continue;
                }
                #[allow(clippy::cast_possible_truncation)] // A small exact integer.
                let winding = rounded as i64;
                for (r, &rule) in rules.iter().enumerate() {
                    let filled = rule.fills(winding);
                    let pixel = layers[r][k].to_pixels().unwrap()[(y * width + x) as usize];
                    assert_eq!(
                        pixel,
                        if filled { 255 } else { 0 },
                        "{rule:?} layer {} pixel ({x}, {y}): winding {w}",
                        plane.index
                    );
                    counts[r].0 += 1;
                    counts[r].1 += usize::from(filled);
                }
            }
        }
    }
    counts
}

fn failure(items: &[ParallelItem]) -> &LayerFailure {
    match items.last() {
        Some(Err(RasterStreamError::Source(LayerEngineError { cause, .. }))) => cause,
        other => panic!("expected a layer failure, got {other:?}"),
    }
}

fn corpus(name: &str) -> TriangleMesh {
    if name == "fx-11-high-complexity.stl" {
        // Generated as in the raster corpus replay, without a 16-MiB STL in Git.
        let centre = geometry::Vec3::new(0., 0., 30.);
        return from_fixture(&geometry::icosphere(30., centre, 7, false));
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

#[test]
fn winding_layers_match_strict_masks_on_the_closed_corpus() {
    let profile = LcdProfile::try_new(
        [128, 128],
        [-64., -64.],
        [128., 128.],
        Orientation::default(),
    )
    .unwrap();
    for (name, first, step, count) in [
        ("fx-01-cube.stl", 2.5, 2.5, 7),
        ("fx-02-cylinder.stl", 2.5, 2.5, 7),
        ("fx-03-sphere.stl", 2.5, 2.5, 7),
        ("fx-04-hollow-box.stl", 2.5, 2.5, 7),
        ("fx-05-rotated-prism.stl", 5., 5., 6),
        ("fx-06-multiple-volumes.stl", 1.25, 1.25, 7),
        ("fx-07-internal-cavity.stl", 7.5, 2.5, 3),
        ("fx-11-high-complexity.stl", 15., 15., 3),
    ] {
        let mesh = corpus(name);
        let schedule = plan(first, step, count);
        for samples_per_axis in [1, 4] {
            let raster = RasterConfig {
                samples_per_axis,
                ..binary()
            };
            let strict = images(run(
                &mesh,
                &schedule,
                LayerFill::Strict,
                profile,
                raster,
                0,
                0,
            ));
            assert!(strict.iter().any(|image| image.stats().nonzero_pixels > 0));
            for rule in RULES {
                let union = images(run(
                    &mesh,
                    &schedule,
                    LayerFill::Winding(rule),
                    profile,
                    raster,
                    0,
                    0,
                ));
                assert_eq!(union.len(), strict.len(), "{name}");
                for (u, s) in union.iter().zip(&strict) {
                    assert_eq!(u.mask(), s.mask(), "{name} {rule:?} {:?}", s.plane());
                    assert_eq!(u.stats(), s.stats());
                    assert_eq!(u.plane(), s.plane());
                    assert_eq!(s.winding(), None);
                    let range = u.winding().unwrap();
                    assert!(range.min == 0 && range.max <= 1, "{name} {range:?}");
                }
            }
        }
    }
}

#[test]
fn overlapping_touching_and_self_intersecting_bodies_match_the_winding_oracle() {
    let schedule = plan(0.3, 0.65, 6);
    let panel = small_panel(Orientation::default());
    let overlapping = bodies(&[
        cuboid([1., 1., 0.], [9., 8., 3.], false),
        cuboid([5., 4., 1.], [14., 12., 4.], false),
    ]);
    // Two boxes sharing the face x = 8 (separate vertices): strict layers are unresolved.
    let touching = bodies(&[
        cuboid([2., 2., 0.], [8., 10., 3.], false),
        cuboid([8., 4., 0.], [15., 12., 3.], false),
    ]);
    let kitbash = bodies(&[
        cuboid([2., 2., 0.], [12., 9., 2.5], false),
        sphere(4., [11., 8., 2.], false),
        rotated(
            cuboid([6., 4., 0.], [15., 7., 3.], false),
            0.5,
            0.3,
            [10.5, 5.5, 1.5],
        ),
    ]);
    let star = pentagram_prism([10., 7.5], 6., [0., 4.]);
    for (name, mesh) in [
        ("overlapping", &overlapping),
        ("touching", &touching),
        ("kitbash", &kitbash),
        ("pentagram", &star),
    ] {
        for (compared, material) in assert_matches_oracle(mesh, &schedule, &RULES, panel) {
            assert!(compared > 6000 && material > 300, "{name}");
        }
    }
    // The pentagram's centre has winding number two: the positive rule merges it, even-odd not.
    let positive = images(run(
        &star,
        &schedule,
        LayerFill::Winding(WindingRule::Positive),
        panel,
        binary(),
        -20,
        1,
    ));
    let even_odd = images(run(
        &star,
        &schedule,
        LayerFill::Winding(WindingRule::EvenOdd),
        panel,
        binary(),
        -20,
        1,
    ));
    let centre = (15 * 40 + 20) as usize;
    assert_eq!(
        (
            positive[0].to_pixels().unwrap()[centre],
            even_odd[0].to_pixels().unwrap()[centre]
        ),
        (255, 0)
    );
    assert_eq!(
        positive[0].winding(),
        Some(WindingRange {
            min: 0,
            max: 2,
            negative_samples: 0
        })
    );
    // Strict layers refuse every one of these sources.
    for mesh in [&overlapping, &touching, &star] {
        let strict = run(mesh, &schedule, LayerFill::Strict, panel, binary(), -20, 1);
        assert!(strict.iter().any(Result::is_err));
    }
}

#[test]
fn cavities_stay_empty_and_nested_bodies_are_material() {
    let schedule = plan(0.3, 0.65, 8);
    let panel = small_panel(Orientation::default());
    // Outer body, reversed cavity, and an outward body floating inside the cavity.
    let nested = bodies(&[
        cuboid([1., 1., 0.], [19., 14., 5.], false),
        cuboid([3., 3., 1.], [17., 12., 4.], true),
        cuboid([7., 6., 1.5], [12., 9., 3.5], false),
    ]);
    let counts = assert_matches_oracle(&nested, &schedule, &[WindingRule::Positive], panel);
    assert!(counts[0].0 > 9000 && counts[0].1 > 2000, "{counts:?}");
    // A hollow sphere: the reversed inner shell leaves the void empty under every rule, and the
    // masks equal the strict pipeline's.
    let hollow = bodies(&[
        sphere(7., [10., 7.5, 4.], false),
        sphere(5., [10., 7.5, 4.], true),
    ]);
    let strict = images(run(
        &hollow,
        &schedule,
        LayerFill::Strict,
        panel,
        binary(),
        -20,
        2,
    ));
    assert_matches_oracle(&hollow, &schedule, &RULES, panel);
    for rule in RULES {
        let union = images(run(
            &hollow,
            &schedule,
            LayerFill::Winding(rule),
            panel,
            binary(),
            -20,
            2,
        ));
        for (u, s) in union.iter().zip(&strict) {
            assert_eq!(u.mask(), s.mask());
        }
    }
}

#[test]
fn open_and_flipped_meshes_fail_with_the_offending_edges() {
    let schedule = plan(0.3, 0.65, 4);
    let panel = small_panel(Orientation::default());
    let mut open = cuboid([2., 2., 0.], [10., 8., 3.], false);
    // Remove one triangle of the y = 2 side (quad 0-1-5-4): two crossed edges become boundary.
    open.triangles.remove(4);
    let items = run(
        &from_fixture(&open),
        &schedule,
        LayerFill::Winding(WindingRule::Positive),
        panel,
        binary(),
        -20,
        2,
    );
    assert_eq!(items.len(), 1, "no mask is delivered");
    let LayerFailure::Unbalanced { crossings, details } = failure(&items) else {
        panic!("{:?}", failure(&items));
    };
    assert_eq!((*crossings, details.len()), (2, 2));
    assert!(details
        .iter()
        .all(karasu_core::slicing::winding::UnbalancedCrossing::open_boundary));
    let mut edges: Vec<[u32; 2]> = details.iter().map(|d| d.edge).collect();
    edges.sort_unstable();
    assert_eq!(edges, [[0, 5], [1, 5]]);
    assert!(failure(&items).to_string().contains("open boundary"));

    let mut flipped = cuboid([2., 2., 0.], [10., 8., 3.], false);
    flipped.triangles[4].swap(1, 2);
    let items = run(
        &from_fixture(&flipped),
        &schedule,
        LayerFill::Winding(WindingRule::NonZero),
        panel,
        binary(),
        -20,
        2,
    );
    let LayerFailure::Unbalanced { details, .. } = failure(&items) else {
        panic!("{:?}", failure(&items));
    };
    assert!(details
        .iter()
        .all(|d| !d.open_boundary() && d.excess.abs() == 2));
    assert!(failure(&items)
        .to_string()
        .contains("inconsistently oriented"));
}

#[test]
fn inside_out_sources_fail_the_positive_rule_and_fill_under_nonzero() {
    let schedule = plan(0.3, 0.65, 4);
    let panel = small_panel(Orientation::default());
    let outward = from_fixture(&cuboid([2., 2., 0.], [10., 8., 3.], false));
    let inward = from_fixture(&cuboid([2., 2., 0.], [10., 8., 3.], true));
    let items = run(
        &inward,
        &schedule,
        LayerFill::Winding(WindingRule::Positive),
        panel,
        binary(),
        -20,
        1,
    );
    assert!(matches!(
        items.last(),
        Some(Err(RasterStreamError::Raster(
            RasterError::NegativeWinding { .. }
        )))
    ));
    let nonzero = images(run(
        &inward,
        &schedule,
        LayerFill::Winding(WindingRule::NonZero),
        panel,
        binary(),
        -20,
        1,
    ));
    let strict = images(run(
        &outward,
        &schedule,
        LayerFill::Strict,
        panel,
        binary(),
        -20,
        1,
    ));
    for (n, s) in nonzero.iter().zip(&strict) {
        assert_eq!(n.mask(), s.mask());
        // Binary masks sample each pixel once, and every covered pixel winds -1.
        let covered = s.stats().nonzero_pixels as u64;
        assert_eq!(
            n.winding(),
            Some(WindingRange {
                min: -1,
                max: 0,
                negative_samples: covered
            })
        );
    }
    // The error names the layer and the first pixel found.
    let first = images(run(
        &inward,
        &schedule,
        LayerFill::Winding(WindingRule::NonZero),
        panel,
        binary(),
        -20,
        1,
    ));
    let [[x0, y0], _] = first[0].stats().bounds.unwrap();
    let Some(Err(RasterStreamError::Raster(RasterError::NegativeWinding { layer, pixel }))) =
        items.first()
    else {
        panic!("{:?}", items.first());
    };
    assert_eq!((*layer, *pixel), (0, [x0, y0]));
}

#[test]
fn the_bounded_positive_rule_tolerates_few_negative_samples_and_reports_them() {
    let schedule = plan(0.3, 0.65, 4);
    let panel = small_panel(Orientation::default());
    let outward = from_fixture(&cuboid([2., 2., 0.], [10., 8., 3.], false));
    let inward = from_fixture(&cuboid([2., 2., 0.], [10., 8., 3.], true));
    let positive = images(run(
        &outward,
        &schedule,
        LayerFill::Winding(WindingRule::Positive),
        panel,
        binary(),
        -20,
        1,
    ));
    let covered = positive[0].stats().nonzero_pixels as u64;
    let bounded = |max_negative_samples| {
        run(
            &inward,
            &schedule,
            LayerFill::Winding(WindingRule::PositiveBounded {
                max_negative_samples,
            }),
            panel,
            binary(),
            -20,
            1,
        )
    };
    // Within the bound the negative samples stay empty and are counted.
    for image in images(bounded(covered)) {
        assert_eq!(image.stats().nonzero_pixels, 0);
        assert_eq!(image.winding().unwrap().negative_samples, covered);
    }
    // One sample fewer fails, naming the count and the bound.
    let items = bounded(covered - 1);
    let Some(Err(RasterStreamError::Raster(RasterError::NegativeWindingAboveBound {
        samples,
        max_samples,
        ..
    }))) = items.first()
    else {
        panic!("{:?}", items.first());
    };
    assert_eq!((*samples, *max_samples), (covered, covered - 1));
    // An outward source gives the positive rule's masks.
    let outward_bounded = images(run(
        &outward,
        &schedule,
        LayerFill::Winding(WindingRule::PositiveBounded {
            max_negative_samples: 0,
        }),
        panel,
        binary(),
        -20,
        1,
    ));
    for (b, p) in outward_bounded.iter().zip(&positive) {
        assert_eq!(b.mask(), p.mask());
        assert_eq!(b.winding().unwrap().negative_samples, 0);
    }
}

#[test]
fn mirrored_panels_and_every_thread_count_give_the_strict_masks() {
    let schedule = plan(0.3, 0.65, 5);
    let mesh = bodies(&[
        sphere(5., [8., 7., 3.], false),
        cuboid([14., 2., 0.], [19., 6., 2.5], false),
    ]);
    for orientation in [
        Orientation::default(),
        Orientation {
            swap_xy: true,
            flip_x: false,
            flip_y: false,
        },
        Orientation {
            swap_xy: false,
            flip_x: true,
            flip_y: false,
        },
        Orientation {
            swap_xy: true,
            flip_x: true,
            flip_y: true,
        },
    ] {
        let panel = LcdProfile::try_new([40, 40], [0., 0.], [20., 20.], orientation).unwrap();
        let strict = images(run(
            &mesh,
            &schedule,
            LayerFill::Strict,
            panel,
            binary(),
            -20,
            1,
        ));
        for threads in [1, 2, 7] {
            let union = images(run(
                &mesh,
                &schedule,
                LayerFill::Winding(WindingRule::Positive),
                panel,
                binary(),
                -20,
                threads,
            ));
            for (u, s) in union.iter().zip(&strict) {
                assert_eq!(u.mask(), s.mask(), "{orientation:?} {threads}");
            }
        }
    }
}

/// A staircase whose step tops sit exactly on scheduled planes.
#[test]
fn planes_on_vertex_heights_never_fail() {
    let steps = bodies(&[
        cuboid([2., 2., 0.], [16., 12., 1.], false),
        cuboid([2., 2., 1.], [12., 12., 2.], false),
        cuboid([2., 2., 2.], [8., 12., 3.], false),
    ]);
    let schedule = plan(0.5, 0.5, 6);
    let panel = small_panel(Orientation::default());
    let strict = images(run(
        &steps,
        &schedule,
        LayerFill::Strict,
        panel,
        binary(),
        -20,
        2,
    ));
    for rule in RULES {
        let union = images(run(
            &steps,
            &schedule,
            LayerFill::Winding(rule),
            panel,
            binary(),
            -20,
            2,
        ));
        assert_eq!(union.len(), 6);
        for (u, s) in union.iter().zip(&strict) {
            assert_eq!(u.mask(), s.mask(), "{rule:?} {:?}", s.plane());
        }
    }
}

#[test]
fn random_overlapping_bodies_match_the_oracle() {
    let mut state = 0x853c_49e6_748f_ea9b_u64;
    let mut next = |range: f64| {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        f64::from(u32::try_from(state % 1_000_000).unwrap()) / 1_000_000. * range
    };
    let schedule = plan(0.37, 0.71, 4);
    let panel = small_panel(Orientation::default());
    for case in 0..12 {
        let mut parts = Vec::new();
        for _ in 0..3 {
            let (x, y, z) = (2. + next(12.), 2. + next(8.), next(1.));
            let size = [1. + next(5.), 1. + next(4.), 1. + next(2.)];
            let body = cuboid([x, y, z], [x + size[0], y + size[1], z + size[2]], false);
            parts.push(rotated(
                body,
                next(2. * PI),
                next(0.6),
                [x + size[0] / 2., y + size[1] / 2., z + size[2] / 2.],
            ));
        }
        parts.push(sphere(
            1. + next(3.),
            [5. + next(10.), 4. + next(7.), 1. + next(2.)],
            false,
        ));
        let mesh = bodies(&parts);
        for (compared, _) in assert_matches_oracle(&mesh, &schedule, &RULES, panel) {
            assert!(compared > 4000, "case {case}");
        }
    }
}
