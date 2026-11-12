//! Drain holes cut into a model's mesh (TD-139, TD-140 phases 4 and 5): the drilled mesh is
//! closed and on the grid, and slicing it gives the layers slicing the model with its holes' cuts
//! gives, but for pixels on a boundary, which snapping, rounding and resolving (at most 7.8 µm)
//! may move.

use karasu_core::{
    cancellation::{Cancellation, Checkpoint},
    geometry::{ObjectTransform, Point3},
    mesh::TriangleMesh,
    preparation::{
        sampled::{
            hollow_mesh_sampled, DrillError, DrillInput, HoleCuts, SampledHollowLimits,
            SampledHollowedModel, MAX_HOLES,
        },
        HoleSpec, HollowSpec, Operation, OperationKind, PlanLimits, PreparationPlan, SamplingSpec,
        SourceBinding,
    },
    raster::{
        parallel::{with_parallel_raster, LayerFill, ParallelRaster},
        Compensation, KeptSampling, LayerCut, LcdProfile, Orientation, OutsidePolicy, RasterConfig,
        RasterLimits,
    },
    slicing::{
        engine::{LayerConfig, LayerStageLimits},
        grid::BinaryGridSpec,
        regions::RegionFillRule,
        schedule::{
            build_layer_schedule, LayerSchedule, ScheduleEnd, ScheduleLimits, UniformScheduleSpec,
        },
        winding::WindingRule,
        SectionTransform,
    },
};
use std::f64::consts::{PI, TAU};

/// A closed, outward box.
fn cube(min: [f64; 3], max: [f64; 3]) -> TriangleMesh {
    let points = (0..8)
        .map(|i| {
            Point3::new(
                if i & 1 == 0 { min[0] } else { max[0] },
                if i & 2 == 0 { min[1] } else { max[1] },
                if i & 4 == 0 { min[2] } else { max[2] },
            )
        })
        .collect();
    let mut triangles = Vec::new();
    for q in [
        [0, 2, 3, 1],
        [4, 5, 7, 6],
        [0, 1, 5, 4],
        [2, 6, 7, 3],
        [0, 4, 6, 2],
        [1, 3, 7, 5],
    ] {
        triangles.push([q[0], q[1], q[2]]);
        triangles.push([q[0], q[2], q[3]]);
    }
    TriangleMesh::try_new(points, triangles).unwrap()
}

/// A closed, outward sphere of `radius` around `centre`: `rings` bands of `sectors` quads.
fn sphere(centre: [f64; 3], radius: f64, rings: u32, sectors: u32) -> TriangleMesh {
    let mut points = vec![Point3::new(centre[0], centre[1], centre[2] - radius)];
    for i in 1..rings {
        let polar = PI * f64::from(i) / f64::from(rings);
        for j in 0..sectors {
            let azimuth = TAU * f64::from(j) / f64::from(sectors);
            points.push(Point3::new(
                centre[0] + radius * polar.sin() * azimuth.cos(),
                centre[1] + radius * polar.sin() * azimuth.sin(),
                centre[2] - radius * polar.cos(),
            ));
        }
    }
    points.push(Point3::new(centre[0], centre[1], centre[2] + radius));
    let top = u32::try_from(points.len() - 1).unwrap();
    let at = |ring: u32, j: u32| 1 + (ring - 1) * sectors + j % sectors;
    let mut triangles = Vec::new();
    for j in 0..sectors {
        triangles.push([0, at(1, j + 1), at(1, j)]);
        triangles.push([top, at(rings - 1, j), at(rings - 1, j + 1)]);
    }
    for ring in 1..rings - 1 {
        for j in 0..sectors {
            let (a, b) = (at(ring, j), at(ring, j + 1));
            let (c, d) = (at(ring + 1, j + 1), at(ring + 1, j));
            triangles.push([a, b, c]);
            triangles.push([a, c, d]);
        }
    }
    TriangleMesh::try_new(points, triangles).unwrap()
}

/// `source` hollowed with a 2 mm wall on a 0.5 mm grid.
fn hollowed(source: &TriangleMesh) -> SampledHollowedModel {
    let plan = PreparationPlan::new(
        SourceBinding {
            original_sha256: [7; 32],
            placement: ObjectTransform::identity(),
        },
        SamplingSpec {
            cell_mm: 0.5,
            distance_error_mm: 1e-6,
            max_cells: u64::MAX,
        },
        vec![Operation {
            id: 1,
            kind: OperationKind::Hollow(HollowSpec {
                blockers: Vec::new(),
                thickness_mm: 2.0,
                guarantee: None,
            }),
        }],
        PlanLimits::default(),
        &mut Cancellation::never(),
    )
    .unwrap();
    hollow_mesh_sampled(
        source,
        &plan,
        SampledHollowLimits::default(),
        &mut Cancellation::never(),
    )
    .unwrap()
}

fn hole(entry: [f64; 3], direction: [f64; 3], length: f64, diameter: f64) -> HoleSpec {
    HoleSpec {
        entry_mm: entry,
        direction,
        length_mm: length,
        diameter_mm: diameter,
    }
}

fn cuts(holes: &[HoleSpec]) -> HoleCuts {
    HoleCuts::build(holes, 1e-3, MAX_HOLES, 4096, &mut Cancellation::never()).unwrap()
}

/// The enclosed volume of a closed outward mesh.
fn volume(mesh: &TriangleMesh) -> f64 {
    let v = mesh.vertices();
    mesh.triangles()
        .iter()
        .map(|t| {
            let [a, b, c] = t.map(|i| v[i as usize]);
            (a.x * (b.y * c.z - b.z * c.y) - a.y * (b.x * c.z - b.z * c.x)
                + a.z * (b.x * c.y - b.y * c.x))
                / 6.0
        })
        .sum()
}

/// No edge on one face, none on more than two, none used twice in one direction.
fn assert_closed(mesh: &TriangleMesh) {
    let report = mesh.analyze_topology().unwrap();
    let summary = report.summary();
    assert_eq!(
        (
            summary.boundary_edges,
            summary.non_manifold_edges,
            summary.winding_conflicts
        ),
        (0, 0, 0),
        "{summary:?}"
    );
}

const PIXELS: usize = 260;

/// Layers of 0.1 mm over 26 mm, pixels of 0.1 mm from the origin, one sample per pixel.
fn layers(mesh: &TriangleMesh, cut: Option<&(dyn LayerCut + Sync)>) -> Vec<Vec<u8>> {
    let schedule: LayerSchedule = build_layer_schedule(
        UniformScheduleSpec::try_new(0.05, 0.1, ScheduleEnd::Count(260)).unwrap(),
        ScheduleLimits {
            max_layers: 260,
            max_payload_bytes: 1 << 20,
        },
    )
    .unwrap();
    let side = u32::try_from(PIXELS).unwrap();
    let spec = ParallelRaster {
        mesh,
        schedule: &schedule,
        transform: SectionTransform::Identity,
        layer_config: LayerConfig {
            grid: BinaryGridSpec::try_new([0., 0.], -20).unwrap(),
            fill_rule: RegionFillRule::EvenOddStrict,
            stages: LayerStageLimits::default(),
            max_output_payload_bytes: 1 << 26,
        },
        represented_coordinates: true,
        face_index_bytes: Some(1 << 24),
        vertex_avoiding_bytes: Some(1 << 24),
        profile: LcdProfile::try_new([side, side], [-1., -1.], [26., 26.], Orientation::default())
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
        threads: 4,
        window_bytes: ParallelRaster::DEFAULT_WINDOW_BYTES,
    };
    let never = |_: Checkpoint| false;
    with_parallel_raster(&spec, &never, |images| {
        images
            .map(|image| image.unwrap().to_pixels().unwrap())
            .collect()
    })
}

/// Whether pixel `i` of `layer` has a neighbour of another value: it lies on a boundary.
fn on_boundary(layer: &[u8], i: usize) -> bool {
    let (x, y) = (i % PIXELS, i / PIXELS);
    (x.saturating_sub(1)..=(x + 1).min(PIXELS - 1)).any(|nx| {
        (y.saturating_sub(1)..=(y + 1).min(PIXELS - 1))
            .any(|ny| layer[ny * PIXELS + nx] != layer[i])
    })
}

/// The pixels where the two stacks differ, after checking each lies on a boundary of both.
fn differences(expected: &[Vec<u8>], actual: &[Vec<u8>]) -> usize {
    assert_eq!(expected.len(), actual.len());
    let mut count = 0;
    for (k, (e, a)) in expected.iter().zip(actual).enumerate() {
        for i in 0..e.len() {
            if e[i] != a[i] {
                assert!(
                    on_boundary(e, i) && on_boundary(a, i),
                    "layer {k}, pixel {i}: {} became {}, away from any boundary",
                    e[i],
                    a[i]
                );
                count += 1;
            }
        }
    }
    count
}

#[test]
fn a_drilled_hollow_sphere_slices_as_its_cuts_do() {
    let source = sphere([12.0, 12.0, 12.0], 10.0, 24, 48);
    let model = hollowed(&source);
    let shell = model.mesh();
    let holes = cuts(&[
        // From above into the cavity, and a narrower one overlapping it.
        hole([12.0, 12.0, 23.0], [0.0, 0.0, -1.0], 4.0, 3.0),
        hole([13.0, 12.0, 23.0], [0.0, 0.0, -1.0], 4.5, 2.0),
        // Along X through both walls and the cavity.
        hole([0.0, 11.0, 9.0], [1.0, 0.0, 0.0], 24.0, 2.5),
        // Slanted into the cavity from below.
        hole([5.0, 5.0, 4.0], [1.0, 1.0, 1.0], 5.0, 2.0),
        // Ending in the wall, and wholly outside the model.
        hole([12.0, 1.0, 12.0], [0.0, 1.0, 0.0], 2.0, 1.5),
        hole([1.0, 1.0, 1.0], [0.0, 0.0, 1.0], 2.0, 1.0),
    ]);
    let drilled = holes.drill(shell, &mut Cancellation::never()).unwrap();
    assert_closed(&drilled.mesh);
    // Where the holes meet the curved shell, the points made are not grid points: rounding them
    // folds slivers where the slanted hole grazes the sphere, and a round moving the vertices
    // near them to their cells' centres resolves them. Nothing is left flat or crossing, and no
    // vertex moved more than half a cell.
    assert!(drilled.rounded > 0, "{}", drilled.rounded);
    assert!(
        (1..=2).contains(&drilled.rounds) && drilled.moved > 0,
        "{} rounds, {} moved",
        drilled.rounds,
        drilled.moved
    );
    assert!(
        drilled.farthest_mm <= 256.0 / 32_768.0,
        "{}",
        drilled.farthest_mm
    );
    assert_eq!((drilled.flat, drilled.crossings), (0, 0));
    // Every vertex is an f32 value: a model file of any format stores the mesh exactly.
    for p in drilled.mesh.vertices() {
        #[allow(clippy::cast_possible_truncation)]
        let stored = [p.x, p.y, p.z].map(|c| f64::from(c as f32));
        assert_eq!(stored, [p.x, p.y, p.z]);
    }
    // Material is only removed, and the holes remove some.
    let (before, after) = (volume(shell), volume(&drilled.mesh));
    assert!(
        after < before - 10.0 && after > before - 200.0,
        "{before} {after}"
    );
    // The same layers as the shell with its holes' cuts, but for pixels on a boundary.
    let expected = layers(shell, Some(&holes));
    let actual = layers(&drilled.mesh, None);
    let material: usize = expected
        .iter()
        .map(|l| l.iter().filter(|&&p| p > 0).count())
        .sum();
    let different = differences(&expected, &actual);
    assert!(different * 10_000 <= material, "{different} of {material}");
    // The holes are there: without their cuts the shell's layers differ.
    let uncut = layers(shell, None);
    let removed = expected
        .iter()
        .zip(&uncut)
        .map(|(e, u)| e.iter().zip(u).filter(|(e, u)| e != u).count())
        .sum::<usize>();
    assert!(removed > 100 * different.max(1), "{removed} {different}");
}

#[test]
fn a_solid_block_takes_a_blind_hole_and_a_hole_through() {
    // On the grid: the block's own vertices do not move.
    let block = cube([0.0; 3], [10.0, 10.0, 8.0]);
    let blind = hole([3.0, 3.0, 9.0], [0.0, 0.0, -1.0], 5.0, 2.0);
    let through = hole([-1.0, 7.0, 4.0], [1.0, 0.0, 0.0], 12.0, 2.0);
    let holes = cuts(&[blind, through]);
    let drilled = holes.drill(&block, &mut Cancellation::never()).unwrap();
    assert_closed(&drilled.mesh);
    // Every point the cut made lies on the block's faces and the cylinders' lateral edges,
    // whole grid steps: nothing is rounded.
    let report = (
        drilled.rounded,
        drilled.rounds,
        drilled.flat,
        drilled.crossings,
    );
    assert_eq!(report, (0, 0, 0, 0));
    // The blind hole goes 4 mm into the block, the other through its 10 mm; each removes its
    // polygon's area (within the cylinder's area) along its length in the block.
    let removed = volume(&block) - volume(&drilled.mesh);
    let area = PI * 1.0_f64.powi(2);
    assert!(
        removed <= 14.0 * area && removed > 14.0 * area * 0.99,
        "{removed}"
    );
    // The block's corners stay where they were.
    for corner in block.vertices() {
        assert!(drilled.mesh.vertices().contains(corner), "{corner:?}");
    }
    // The same mesh again: the cut is deterministic.
    let again = holes.drill(&block, &mut Cancellation::never()).unwrap();
    assert_eq!(again.mesh.vertices(), drilled.mesh.vertices());
    assert_eq!(again.mesh.triangles(), drilled.mesh.triangles());
}

#[test]
fn holes_beside_the_model_leave_it_as_it_was() {
    let block = cube([0.0; 3], [10.0, 10.0, 8.0]);
    let holes = cuts(&[hole([20.0, 5.0, 9.0], [0.0, 0.0, -1.0], 10.0, 2.0)]);
    let drilled = holes.drill(&block, &mut Cancellation::never()).unwrap();
    assert_eq!(drilled.mesh.triangles().len(), 12);
    // The cylinder's vertices are snapped; nothing the cut made is rounded.
    assert_eq!(drilled.rounded, 0);
    assert!((volume(&drilled.mesh) - 800.0).abs() < 1e-9);
}

#[test]
fn an_open_model_or_one_beyond_the_range_is_refused() {
    let block = cube([0.0; 3], [10.0, 10.0, 8.0]);
    let holes = cuts(&[hole([3.0, 3.0, 9.0], [0.0, 0.0, -1.0], 5.0, 2.0)]);
    let open =
        TriangleMesh::try_new(block.vertices().to_vec(), block.triangles()[1..].to_vec()).unwrap();
    match holes.drill(&open, &mut Cancellation::never()) {
        Err(DrillError::Open {
            input: DrillInput::Model,
            edges: 3,
        }) => {}
        other => panic!("{other:?}"),
    }
    let far = cube([0.0; 3], [10.0, 10.0, 20_000.0]);
    match holes.drill(&far, &mut Cancellation::never()) {
        Err(DrillError::Coordinate {
            input: DrillInput::Model,
            ..
        }) => {}
        other => panic!("{other:?}"),
    }
    // Cancelled at the first check.
    let mut stop = |_: Checkpoint| true;
    assert!(matches!(
        holes.drill(&block, &mut Cancellation::new(&mut stop)),
        Err(DrillError::Cancelled(_))
    ));
}
