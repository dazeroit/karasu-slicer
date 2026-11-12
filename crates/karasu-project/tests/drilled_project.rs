//! Drain holes cut into a real project's hollowed models (TD-139, TD-140 phase 4): a
//! measurement on a local project, not part of the default run.

use karasu_core::{
    cancellation::{Cancellation, Checkpoint},
    mesh::TriangleMesh,
    raster::{
        parallel::{with_parallel_raster, LayerFill, ParallelRaster},
        Compensation, KeptSampling, LayerCut, LcdProfile, Orientation, OutsidePolicy, RasterConfig,
        RasterLimits,
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
use karasu_project::{
    pipeline::{prepare, PreparedHoles, Silent},
    project::file,
};
use std::time::Instant;

/// `f` given the material pixels of `mesh` (minus `cut`) layer by layer, as they are made, on
/// layers of 0.05 mm from `low` to `high`, on pixels of 0.05 mm over the box `min`–`max`, one
/// sample each, and the layers' width: no more than a window of layers is held at once.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)] // A few thousand pixels and layers.
fn with_layers<R>(
    mesh: &TriangleMesh,
    cut: Option<&(dyn LayerCut + Sync)>,
    (min, max): ([f64; 2], [f64; 2]),
    (low, high): (f64, f64),
    f: impl FnOnce(&mut dyn Iterator<Item = Vec<u8>>, usize) -> R,
) -> R {
    let pitch = 0.05;
    let count = ((high - low) / pitch).ceil() as u64;
    let schedule = build_layer_schedule(
        UniformScheduleSpec::try_new(low + pitch / 2.0, pitch, ScheduleEnd::Count(count)).unwrap(),
        ScheduleLimits {
            max_layers: count,
            max_payload_bytes: 1 << 24,
        },
    )
    .unwrap();
    let width = ((max[0] - min[0]) / pitch).ceil() as u32;
    let height = ((max[1] - min[1]) / pitch).ceil() as u32;
    let spec = ParallelRaster {
        mesh,
        schedule: &schedule,
        transform: SectionTransform::Identity,
        layer_config: LayerConfig {
            grid: BinaryGridSpec::try_new([0., 0.], -20).unwrap(),
            fill_rule: RegionFillRule::EvenOddStrict,
            stages: LayerStageLimits::default(),
            max_output_payload_bytes: 1 << 28,
        },
        represented_coordinates: true,
        face_index_bytes: Some(1 << 28),
        vertex_avoiding_bytes: Some(1 << 28),
        profile: LcdProfile::try_new(
            [width, height],
            min,
            [f64::from(width) * pitch, f64::from(height) * pitch],
            Orientation::default(),
        )
        .unwrap(),
        raster: RasterConfig {
            samples_per_axis: 1,
            grayscale_levels: 2,
            minimum_gray: 0,
            outside: OutsidePolicy::Clip,
            compensation: Compensation::None,
            kept: KeptSampling::Antialiased,
        },
        bottom: None,
        limits: RasterLimits::default(),
        cut,
        kept_faces: &[],
        fill: LayerFill::Winding(WindingRule::Positive),
        closure: None,
        threads: std::thread::available_parallelism().map_or(4, usize::from),
        window_bytes: ParallelRaster::DEFAULT_WINDOW_BYTES,
    };
    let never = |_: Checkpoint| false;
    with_parallel_raster(&spec, &never, |images| {
        let mut pixels = images.map(|image| image.unwrap().to_pixels().unwrap());
        f(&mut pixels, width as usize)
    })
}

/// The distance from `p` to the nearest triangle of `mesh` (mm).
#[allow(clippy::many_single_char_names)] // Points of a triangle and their dot products.
fn nearest(mesh: &TriangleMesh, p: [f64; 3]) -> f64 {
    let v = mesh.vertices();
    let sub = |a: [f64; 3], b: [f64; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let mut best = f64::INFINITY;
    for t in mesh.triangles() {
        let [a, b, c] = t.map(|i| {
            let q = v[i as usize];
            [q.x, q.y, q.z]
        });
        // Only the triangles near the point.
        if (0..3)
            .any(|k| a[k].min(b[k]).min(c[k]) > p[k] + 1.0 || a[k].max(b[k]).max(c[k]) < p[k] - 1.0)
        {
            continue;
        }
        // The closest point by barycentric regions (Ericson, Real-Time Collision Detection).
        let (ab, ac, ap) = (sub(b, a), sub(c, a), sub(p, a));
        let (d1, d2) = (dot(ab, ap), dot(ac, ap));
        let bp = sub(p, b);
        let (d3, d4) = (dot(ab, bp), dot(ac, bp));
        let cp = sub(p, c);
        let (d5, d6) = (dot(ab, cp), dot(ac, cp));
        let (va, vb, vc) = (d3 * d6 - d5 * d4, d5 * d2 - d1 * d6, d1 * d4 - d3 * d2);
        let along = |s: [f64; 3], e: [f64; 3], t: f64| [0, 1, 2].map(|k| s[k] + t * (e[k] - s[k]));
        let q = if d1 <= 0.0 && d2 <= 0.0 {
            a
        } else if d3 >= 0.0 && d4 <= d3 {
            b
        } else if d6 >= 0.0 && d5 <= d6 {
            c
        } else if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
            along(a, b, d1 / (d1 - d3))
        } else if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
            along(a, c, d2 / (d2 - d6))
        } else if va <= 0.0 && d4 - d3 >= 0.0 && d5 - d6 >= 0.0 {
            along(b, c, (d4 - d3) / ((d4 - d3) + (d5 - d6)))
        } else {
            let sum = va + vb + vc;
            [0, 1, 2].map(|k| a[k] + ab[k] * vb / sum + ac[k] * vc / sum)
        };
        best = best.min(dot(sub(p, q), sub(p, q)).sqrt());
    }
    best
}

/// Whether pixel `i` of a layer `width` wide has a neighbour of another value.
fn on_boundary(layer: &[u8], width: usize, i: usize) -> bool {
    let (x, y) = (i % width, i / width);
    let rows = layer.len() / width;
    (x.saturating_sub(1)..=(x + 1).min(width - 1)).any(|nx| {
        (y.saturating_sub(1)..=(y + 1).min(rows - 1)).any(|ny| layer[ny * width + nx] != layer[i])
    })
}

#[test]
#[ignore = "drills a local project's models; set KARASU_DRILL_PROJECT to a .karasu file"]
fn a_project_s_hollowed_models_drilled_slice_as_their_cuts_do() {
    // KARASU_DRILL_PROJECT=<project.karasu> cargo test --release -p karasu-project --test
    // drilled_project -- --ignored --nocapture (KARASU_DRILL_COMPARE=0: no layer comparison)
    let Ok(path) = std::env::var("KARASU_DRILL_PROJECT") else {
        return;
    };
    let project = file::open(std::path::Path::new(&path)).unwrap();
    for plate in project.plates() {
        let (request, _) = project
            .request(plate.id(), std::env::temp_dir().join("unused.pp1"))
            .unwrap();
        let never = |_: Checkpoint| false;
        let preparation = prepare(&request, &mut Silent, &never).unwrap();
        for (number, object) in (1..).zip(preparation.objects) {
            let object = object.unwrap();
            let (Some(model), Some(holes)) = (object.hollowed(), object.holes()) else {
                continue;
            };
            let shell = model.mesh();
            let started = Instant::now();
            let drilled = holes
                .cuts()
                .drill(shell, &mut Cancellation::never())
                .unwrap();
            let seconds = started.elapsed().as_secs_f64();
            let topology = drilled.mesh.analyze_topology().unwrap();
            let summary = topology.summary();
            println!(
                "object {number}: {} triangles and {} holes in, {} out in {seconds:.2} s; \
                 {} snapped, {} rounded, {} rounds, {} moved (farthest {:.2} µm), {} specks, \
                 {} separated, {} flat, {} crossings; \
                 {} boundary, {} non-manifold edges, {} winding conflicts",
                shell.triangles().len(),
                holes.specs().len(),
                drilled.mesh.triangles().len(),
                drilled.snapped,
                drilled.rounded,
                drilled.rounds,
                drilled.moved,
                drilled.farthest_mm * 1000.0,
                drilled.specks,
                drilled.separated,
                drilled.flat,
                drilled.crossings,
                summary.boundary_edges,
                summary.non_manifold_edges,
                summary.winding_conflicts,
            );
            assert_eq!(summary.boundary_edges, 0);
            // KARASU_DRILL_COMPARE=0 measures the cut alone (the comparison keeps every layer).
            if std::env::var("KARASU_DRILL_COMPARE").is_ok_and(|v| v == "0") {
                continue;
            }
            // A differing pixel away from a boundary lies within the farthest move (on three axes)
            // and the grid's half step of the model's surface.
            // Plus the 1 µm by which touching bodies are moved apart.
            let reach = drilled.farthest_mm * 3.0_f64.sqrt() + 0.0005 + 0.001;
            assert_eq!(compare(shell, holes, &drilled.mesh, reach), 0);
        }
    }
}

/// The layers through the holes of `shell` sliced with their cuts and of `drilled`, compared:
/// the number of differing pixels whose neighbours agree and whose sample lies farther than
/// `reach` (mm) from the model's surface and the holes' walls (a feature thinner than a pixel
/// explains the others).
fn compare(
    shell: &TriangleMesh,
    holes: &PreparedHoles,
    drilled: &TriangleMesh,
    reach: f64,
) -> usize {
    // The layers through the holes, with the cuts and drilled.
    let cutters = holes.cutters().vertices();
    let fold = |f: fn(f64, f64) -> f64, start: f64, k: usize| {
        cutters.iter().map(|p| [p.x, p.y, p.z][k]).fold(start, f)
    };
    let bounds = shell.vertices().iter().fold(
        ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]),
        |(lo, hi), p| {
            (
                [lo[0].min(p.x), lo[1].min(p.y), lo[2].min(p.z)],
                [hi[0].max(p.x), hi[1].max(p.y), hi[2].max(p.z)],
            )
        },
    );
    let low = fold(f64::min, f64::INFINITY, 2).max(bounds.0[2]);
    let high = fold(f64::max, f64::NEG_INFINITY, 2).min(bounds.1[2]);
    let (min, max) = (
        [bounds.0[0] - 0.5, bounds.0[1] - 0.5],
        [bounds.1[0] + 0.5, bounds.1[1] + 0.5],
    );
    let started = Instant::now();
    let (mut different, mut inner, mut material, mut count, mut rows) = (0, 0, 0, 0, 0);
    with_layers(
        shell,
        Some(holes.cuts()),
        (min, max),
        (low, high),
        |expected, width| {
            with_layers(drilled, None, (min, max), (low, high), |actual, _| {
                for (layer, (e, a)) in expected.zip(actual).enumerate() {
                    count += 1;
                    rows = e.len() / width;
                    material += e.iter().filter(|&&p| p > 0).count();
                    for i in 0..e.len() {
                        if e[i] == a[i] {
                            continue;
                        }
                        different += 1;
                        if on_boundary(&e, width, i) && on_boundary(&a, width, i) {
                            continue;
                        }
                        // A pixel whose neighbours agree: its sample must lie within `reach` of
                        // the model's surface (a feature thinner than a pixel).
                        #[allow(clippy::cast_precision_loss)] // Pixel and layer numbers.
                        let at = |k: usize| k as f64 * 0.05 + 0.025;
                        let sample = [
                            min[0] + at(i % width),
                            min[1] + at(i / width),
                            low + at(layer),
                        ];
                        // The drilled surface is the shell's and the holes' walls.
                        let distance = nearest(shell, sample).min(nearest(holes.cutters(), sample));
                        println!(
                            "  {sample:.3?}: {} became {}, {:.3} µm from the model's surface or a hole's",
                            e[i],
                            a[i],
                            distance * 1000.0
                        );
                        if distance > reach {
                            inner += 1;
                        }
                    }
                }
            });
        },
    );
    let width = ((max[0] - min[0]) / 0.05).ceil();
    println!(
        "  {count} layers of {width} x {rows} pixels compared in {:.1} s: {different} of \
         {material} material pixels differ, {inner} of them away from the surface",
        started.elapsed().as_secs_f64()
    );
    inner
}
