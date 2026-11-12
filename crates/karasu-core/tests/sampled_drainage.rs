//! Drainage cuts into sampled (non-convex) cavities: certified openings, cavity regions and the
//! drained solid rasterized as shell minus cutters.

use karasu_core::{
    cancellation::{Cancellation, Checkpoint},
    geometry::{ObjectTransform, Point3, Rotation3, Vector3},
    mesh::TriangleMesh,
    preparation::{
        drainage::{DrainageError, OpeningIssue},
        sampled::{
            drain_hollowed_sampled, drain_model_sampled, hollow_mesh_sampled,
            SampledDrainageLimits, SampledDrainedModel, SampledHollowLimits, SampledHollowStage,
        },
        HoleSpec, HollowSpec, Operation, OperationKind, PlanLimits, PreparationPlan, SamplingSpec,
        SourceBinding, SurfaceHoleSpec,
    },
    raster::{
        stream::DifferenceRasterStream, Compensation, LcdProfile, Orientation, OutsidePolicy,
        RasterConfig, RasterLimits,
    },
    slicing::{
        engine::{LayerConfig, LayerStageLimits},
        grid::BinaryGridSpec,
        regions::RegionFillRule,
        schedule::{build_layer_schedule, ScheduleEnd, ScheduleLimits, UniformScheduleSpec},
    },
};
use std::f64::consts::PI;

fn mesh(points: Vec<[f64; 3]>, triangles: Vec<[u32; 3]>) -> TriangleMesh {
    TriangleMesh::try_new(
        points
            .into_iter()
            .map(|[x, y, z]| Point3::new(x, y, z))
            .collect(),
        triangles,
    )
    .unwrap()
}

fn cuboid(
    min: [f64; 3],
    max: [f64; 3],
    inward: bool,
    offset: u32,
) -> (Vec<[f64; 3]>, Vec<[u32; 3]>) {
    let points = (0..8)
        .map(|i| {
            [
                if i & 1 == 0 { min[0] } else { max[0] },
                if i & 2 == 0 { min[1] } else { max[1] },
                if i & 4 == 0 { min[2] } else { max[2] },
            ]
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
        for mut t in [[q[0], q[1], q[2]], [q[0], q[2], q[3]]] {
            if inward {
                t.swap(1, 2);
            }
            triangles.push(t.map(|v| v + offset));
        }
    }
    (points, triangles)
}

fn boxes(specs: &[([f64; 3], [f64; 3], bool)]) -> TriangleMesh {
    let mut points = Vec::new();
    let mut triangles = Vec::new();
    for &(min, max, inward) in specs {
        let offset = u32::try_from(points.len()).unwrap();
        let (p, t) = cuboid(min, max, inward, offset);
        points.extend(p);
        triangles.extend(t);
    }
    mesh(points, triangles)
}

#[allow(clippy::many_single_char_names)] // Parametric torus notation.
fn torus(big: f64, small: f64, u: u32, v: u32, center: [f64; 3]) -> TriangleMesh {
    let mut points = Vec::new();
    for i in 0..u {
        let a = 2.0 * PI * f64::from(i) / f64::from(u);
        for j in 0..v {
            let b = 2.0 * PI * f64::from(j) / f64::from(v);
            let r = big + small * b.cos();
            points.push([
                center[0] + r * a.cos(),
                center[1] + r * a.sin(),
                center[2] + small * b.sin(),
            ]);
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
    mesh(points, triangles)
}

fn hole(entry: [f64; 3], direction: [f64; 3], length: f64, diameter: f64) -> HoleSpec {
    HoleSpec {
        entry_mm: entry,
        direction,
        length_mm: length,
        diameter_mm: diameter,
    }
}

fn plan(thickness: f64, cell: f64, holes: &[HoleSpec]) -> PreparationPlan {
    let mut operations = vec![Operation {
        id: 1,
        kind: OperationKind::Hollow(HollowSpec {
            blockers: Vec::new(),
            thickness_mm: thickness,
            guarantee: None,
        }),
    }];
    for (i, &spec) in holes.iter().enumerate() {
        operations.push(Operation {
            id: 10 + u64::try_from(i).unwrap(),
            kind: OperationKind::Hole(spec),
        });
    }
    PreparationPlan::new(
        SourceBinding {
            original_sha256: [3; 32],
            placement: ObjectTransform::new(
                Vector3::new(0.0, 0.0, 0.0),
                Rotation3::identity(),
                Vector3::new(1.0, 1.0, 1.0),
                Point3::new(0.0, 0.0, 0.0),
            )
            .unwrap(),
        },
        SamplingSpec {
            cell_mm: cell,
            distance_error_mm: 1e-4,
            max_cells: u64::MAX,
        },
        operations,
        PlanLimits::default(),
        &mut Cancellation::never(),
    )
    .unwrap()
}

fn drain(
    source: &TriangleMesh,
    plan: &PreparationPlan,
) -> Result<SampledDrainedModel, DrainageError> {
    drain_model_sampled(
        source,
        plan,
        SampledDrainageLimits::default(),
        &mut Cancellation::never(),
        &mut |_| {},
    )
}

/// Total sampled material volume of the drained solid on a 0.1 mm panel, in mm³.
fn drained_volume(
    model: &SampledDrainedModel,
    origin: [f64; 2],
    size: [f64; 2],
    layers: u64,
    height: f64,
) -> f64 {
    let schedule = build_layer_schedule(
        UniformScheduleSpec::try_new(height / 2.0, height, ScheduleEnd::Count(layers)).unwrap(),
        ScheduleLimits {
            max_layers: layers,
            max_payload_bytes: 1 << 24,
        },
    )
    .unwrap();
    let config = LayerConfig {
        grid: BinaryGridSpec::try_new([0.0, 0.0], -20).unwrap(),
        fill_rule: RegionFillRule::EvenOddStrict,
        stages: LayerStageLimits::default(),
        max_output_payload_bytes: 64 << 20,
    };
    let pitch = 0.1;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let resolution = [
        (size[0] / pitch).round() as u32,
        (size[1] / pitch).round() as u32,
    ];
    let profile = LcdProfile::try_new(resolution, origin, size, Orientation::default()).unwrap();
    let raster = RasterConfig {
        samples_per_axis: 1,
        grayscale_levels: 256,
        minimum_gray: 0,
        outside: OutsidePolicy::Reject,
        compensation: Compensation::None,
        kept: karasu_core::raster::KeptSampling::Antialiased,
    };
    let body = model
        .body_layers(&schedule, config, Cancellation::never(), None)
        .with_face_z_index(1 << 28);
    let stream = DifferenceRasterStream::new(
        body,
        model,
        profile,
        raster,
        RasterLimits::default(),
        Cancellation::never(),
    );
    let mut volume = 0.0;
    for image in stream {
        let image = image.unwrap();
        #[allow(clippy::naive_bytecount)] // Test oracle; no byte-counting dependency.
        let lit = image
            .to_pixels()
            .unwrap()
            .iter()
            .filter(|&&p| p == 255)
            .count();
        volume += f64::from(u32::try_from(lit).unwrap()) * pitch * pitch * height;
    }
    volume
}

#[test]
fn bottom_hole_drains_the_cube_cavity_and_removes_only_the_wall_plug() {
    let source = boxes(&[([0.0; 3], [20.0; 3], false)]);
    let model = drain(
        &source,
        &plan(
            2.0,
            0.25,
            &[hole([10.0, 10.0, -1.0], [0.0, 0.0, 1.0], 4.0, 3.0)],
        ),
    )
    .unwrap();
    assert_eq!(model.openings().len(), 1);
    let opening = model.openings()[0];
    assert_eq!(opening.operation_id, 10);
    assert_eq!(opening.cavity_region, 0);
    // Entry cap at Z -1 is 1 mm below the base; the end cap at Z 3 is about 1 mm above the floor.
    assert!(
        (opening.entry_clearance_mm - 1.0).abs() < 1e-9,
        "{opening:?}"
    );
    assert!(
        opening.terminal_clearance_mm > 0.9 && opening.terminal_clearance_mm < 1.1,
        "{opening:?}"
    );
    assert_eq!(model.cavity_regions().len(), 1);
    assert_eq!(model.cavity_regions()[0].drained_by, [10]);
    assert_eq!(model.enclosed_cavities(), 0);
    // Rasterized: 8000 - 16^3 - (plug of radius 1.5 through the 2 mm floor).
    let volume = drained_volume(&model, [0.0, 0.0], [20.0, 20.0], 80, 0.25);
    let analytic = 8000.0 - 4096.0 - PI * 1.5 * 1.5 * 2.0;
    assert!(
        (volume - analytic).abs() < 0.01 * analytic,
        "{volume} vs {analytic}"
    );
    let solid = drained_volume(
        &drain(&source, &plan(2.0, 0.25, &[])).unwrap(),
        [0.0, 0.0],
        [20.0, 20.0],
        80,
        0.25,
    );
    assert!(solid > volume + 10.0, "{solid} vs {volume}");
}

#[test]
fn non_convex_torus_cavity_is_drained_from_above() {
    // Tube radius 5 around a 12 mm circle at Z 10; a 1.5 mm wall leaves a 3.5 mm tube cavity
    // whose top is at Z 13.5. A 1.5 mm hole enters at Z 16 and ends at Z 12.5, inside it.
    let source = torus(12.0, 5.0, 96, 48, [0.0, 0.0, 10.0]);
    let model = drain(
        &source,
        &plan(
            1.5,
            0.2,
            &[hole([12.0, 0.0, 16.0], [0.0, 0.0, -1.0], 3.5, 1.5)],
        ),
    )
    .unwrap();
    assert_eq!(model.cavity_regions().len(), 1);
    assert_eq!(model.enclosed_cavities(), 0);
    let opening = model.openings()[0];
    assert!(opening.entry_clearance_mm > 0.5, "{opening:?}");
    assert!(opening.terminal_clearance_mm > 0.2, "{opening:?}");
}

#[test]
fn only_reached_regions_are_drained_and_sealed_voids_are_not_exterior() {
    // Two separate cubes: one hole drains one of the two cavities.
    let source = boxes(&[
        ([0.0; 3], [20.0; 3], false),
        ([30.0, 0.0, 0.0], [50.0, 20.0, 20.0], false),
    ]);
    let model = drain(
        &source,
        &plan(
            2.0,
            0.25,
            &[hole([10.0, 10.0, -1.0], [0.0, 0.0, 1.0], 4.0, 3.0)],
        ),
    )
    .unwrap();
    assert_eq!(model.cavity_regions().len(), 2);
    assert_eq!(model.enclosed_cavities(), 1);
    // Each region is located (TD-101): its point is on its own surface, inside its bounds,
    // which lie within its cube less the wall.
    let mut located: Vec<([f64; 3], [[f64; 3]; 2])> = model
        .cavity_regions()
        .iter()
        .map(|r| (r.point_mm, r.bounds_mm))
        .collect();
    located.sort_by(|a, b| a.1[0][0].total_cmp(&b.1[0][0]));
    for ((point, [min, max]), x0) in located.into_iter().zip([0.0, 30.0]) {
        assert!((0..3).all(|k| min[k] <= point[k] && point[k] <= max[k]));
        let cube_min = [x0 + 2.0, 2.0, 2.0];
        let cube_max = [x0 + 18.0, 18.0, 18.0];
        // The sampled cavity follows the offset surface within a cell.
        assert!(
            (0..3)
                .all(|k| (min[k] - cube_min[k]).abs() < 0.5 && (max[k] - cube_max[k]).abs() < 0.5),
            "{min:?} {max:?}"
        );
    }
    // An entry inside a sealed source void is outside by parity but not exterior.
    let sealed = boxes(&[([0.0; 3], [40.0; 3], false), ([15.0; 3], [25.0; 3], true)]);
    let error = drain(
        &sealed,
        &plan(
            2.0,
            0.5,
            &[hole([20.0, 20.0, 20.0], [0.0, 0.0, 1.0], 9.0, 2.0)],
        ),
    )
    .unwrap_err();
    assert!(
        matches!(
            error,
            DrainageError::Opening {
                operation_id: 10,
                issue: OpeningIssue::EntryNotExterior
            }
        ),
        "{error}"
    );
}

#[test]
fn preserved_inner_void_gives_one_region_with_an_island() {
    let source = boxes(&[([0.0; 3], [30.0; 3], false), ([10.0; 3], [20.0; 3], true)]);
    let model = drain(
        &source,
        &plan(
            2.0,
            0.25,
            &[hole([5.0, 5.0, -1.0], [0.0, 0.0, 1.0], 4.0, 2.0)],
        ),
    )
    .unwrap();
    let regions = model.cavity_regions();
    assert_eq!(regions.len(), 1);
    assert_eq!(regions[0].surfaces, 2);
    let certificate = model.hollowed_model().certificate();
    assert!((regions[0].volume_mm3 - certificate.cavity_volume_mm3).abs() < 1e-6);
    assert_eq!(model.enclosed_cavities(), 0);
}

#[test]
fn a_column_of_preserved_voids_gives_one_region_with_every_island() {
    // Twelve 1 mm voids stacked in one column inside a 6 x 6 x 40 mm box: each island ray must
    // find the one outer surface without crossing the islands above it.
    let mut specs = vec![([0.0; 3], [6.0, 6.0, 40.0], false)];
    for k in 0..12 {
        let z = 4.0 + 2.75 * f64::from(k);
        specs.push(([2.5, 2.5, z], [3.5, 3.5, z + 1.0], true));
    }
    let source = boxes(&specs);
    let model = drain(
        &source,
        &plan(
            0.5,
            0.25,
            &[hole([1.2, 1.2, -1.0], [0.0, 0.0, 1.0], 2.0, 0.8)],
        ),
    )
    .unwrap();
    let regions = model.cavity_regions();
    assert_eq!(regions.len(), 1);
    assert_eq!(regions[0].surfaces, 13);
    let certificate = model.hollowed_model().certificate();
    assert!((regions[0].volume_mm3 - certificate.cavity_volume_mm3).abs() < 1e-6);
    assert_eq!(regions[0].drained_by, [10]);
}

#[test]
fn recipes_and_limits_outside_the_contract_are_refused() {
    let source = boxes(&[([0.0; 3], [20.0; 3], false)]);
    let one_hole = plan(
        2.0,
        0.5,
        &[hole([10.0, 10.0, -1.0], [0.0, 0.0, 1.0], 4.0, 3.0)],
    );
    let run = |plan: &PreparationPlan, limits: SampledDrainageLimits| {
        drain_model_sampled(
            &source,
            plan,
            limits,
            &mut Cancellation::never(),
            &mut |_| {},
        )
    };
    for limits in [
        SampledDrainageLimits {
            max_holes: 0,
            ..SampledDrainageLimits::default()
        },
        SampledDrainageLimits {
            max_segments: 4097,
            ..SampledDrainageLimits::default()
        },
        SampledDrainageLimits {
            max_segments: 7,
            ..SampledDrainageLimits::default()
        },
    ] {
        let error = run(&one_hole, limits).unwrap_err();
        assert!(matches!(error, DrainageError::Resource { .. }), "{error}");
    }
    for clearance in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        let limits = SampledDrainageLimits {
            minimum_clearance_mm: clearance,
            ..SampledDrainageLimits::default()
        };
        let error = run(&one_hole, limits).unwrap_err();
        assert!(
            matches!(error, DrainageError::Unsupported { .. }),
            "{error}"
        );
    }
    // The plan itself orders operations (hollow, then cuts, then supports); a plan with no
    // hollow operation is valid for the plan but not a drainage recipe.
    let empty = PreparationPlan::new(
        one_hole.source(),
        one_hole.sampling(),
        Vec::new(),
        PlanLimits::default(),
        &mut Cancellation::never(),
    )
    .unwrap();
    let error = run(&empty, SampledDrainageLimits::default()).unwrap_err();
    assert!(
        matches!(
            error,
            DrainageError::Unsupported {
                reason: "hollow-first recipe required"
            }
        ),
        "{error}"
    );
}

#[test]
fn every_failed_opening_is_classified() {
    let source = boxes(&[([0.0; 3], [20.0; 3], false)]);
    let cases = [
        // Entry cap inside the base material, and entry cap lying on the base face (contact).
        (
            hole([10.0, 10.0, 0.5], [0.0, 0.0, 1.0], 4.0, 3.0),
            OpeningIssue::EntryNotExterior,
            true,
        ),
        (
            hole([10.0, 10.0, 0.0], [0.0, 0.0, 1.0], 4.0, 3.0),
            OpeningIssue::EntryNotExterior,
            true,
        ),
        // Ends inside the wall.
        (
            hole([10.0, 10.0, -1.0], [0.0, 0.0, 1.0], 2.5, 3.0),
            OpeningIssue::Shallow,
            false,
        ),
        // Points away from the model.
        (
            hole([10.0, 10.0, -1.0], [0.0, 0.0, -1.0], 2.0, 3.0),
            OpeningIssue::MissedModel,
            false,
        ),
        // End-cap centre exactly on the top or bottom face: too short from outside, too long
        // after crossing the cavity.
        (
            hole([10.0, 10.0, 25.0], [0.0, 0.0, -1.0], 5.0, 3.0),
            OpeningIssue::Shallow,
            false,
        ),
        (
            hole([10.0, 10.0, -1.0], [0.0, 0.0, 1.0], 1.0, 3.0),
            OpeningIssue::Shallow,
            false,
        ),
        (
            hole([10.0, 10.0, -1.0], [0.0, 0.0, 1.0], 21.0, 3.0),
            OpeningIssue::Overshoot,
            false,
        ),
        // The same on vertical faces, decided by an exact point query whatever the side: the far
        // wall after crossing the cavity, the near wall from outside, a wall plane reached along
        // that wall, and a vertical edge.
        (
            hole([-5.0, 10.0, 10.0], [1.0, 0.0, 0.0], 25.0, 3.0),
            OpeningIssue::Overshoot,
            false,
        ),
        (
            hole([10.0, -5.0, 10.0], [0.0, 1.0, 0.0], 25.0, 3.0),
            OpeningIssue::Overshoot,
            false,
        ),
        (
            hole([25.0, 10.0, 10.0], [-1.0, 0.0, 0.0], 5.0, 3.0),
            OpeningIssue::Shallow,
            false,
        ),
        (
            hole([20.0, 10.0, -5.0], [0.0, 0.0, 1.0], 15.0, 3.0),
            OpeningIssue::Shallow,
            false,
        ),
        (
            hole([25.0, 20.0, 10.0], [-1.0, 0.0, 0.0], 5.0, 3.0),
            OpeningIssue::Shallow,
            false,
        ),
        // Through the whole cavity into the top wall; centred end cap crossing the side wall.
        (
            hole([10.0, 10.0, -1.0], [0.0, 0.0, 1.0], 20.0, 3.0),
            OpeningIssue::Overshoot,
            false,
        ),
        (
            hole([17.5, 10.0, -1.0], [0.0, 0.0, 1.0], 11.0, 2.0),
            OpeningIssue::InsufficientClearance,
            false,
        ),
        (
            hole([2.5, 10.0, -1.0], [0.0, 0.0, 1.0], 11.0, 2.0),
            OpeningIssue::InsufficientClearance,
            false,
        ),
        // End cap wider than the clearance next to the cavity side wall.
        (
            hole([2.5, 10.0, -1.0], [0.0, 0.0, 1.0], 4.0, 2.0),
            OpeningIssue::InsufficientClearance,
            false,
        ),
    ];
    for (spec, issue, entry_case) in cases {
        let error = match drain(&source, &plan(2.0, 0.25, &[spec])) {
            Ok(model) => panic!("{spec:?} unexpectedly certified: {:?}", model.openings()),
            Err(error) => error,
        };
        match error {
            DrainageError::Opening {
                operation_id: 10,
                issue: found,
            } => {
                assert_eq!(found, issue, "{spec:?}");
            }
            other => panic!("{spec:?}: {other} (entry case {entry_case})"),
        }
    }
}

#[test]
fn overlapping_cutters_bad_axes_and_cancellation_are_rejected() {
    let source = boxes(&[([0.0; 3], [20.0; 3], false)]);
    let error = drain(
        &source,
        &plan(
            2.0,
            0.25,
            &[
                hole([10.0, 10.0, -1.0], [0.0, 0.0, 1.0], 4.0, 3.0),
                hole([11.0, 10.0, -1.0], [0.0, 0.0, 1.0], 4.0, 3.0),
            ],
        ),
    )
    .unwrap_err();
    assert!(
        matches!(
            error,
            DrainageError::PotentialOverlap {
                first: 10,
                second: 11
            }
        ),
        "{error}"
    );
    // An oblique cut is accepted (TD-066), but this tilted entry cap reaches into the floor.
    let error = drain(
        &source,
        &plan(
            2.0,
            0.25,
            &[hole([10.0, 10.0, -1.0], [0.0, 1.0, 1.0], 4.0, 3.0)],
        ),
    )
    .unwrap_err();
    assert!(
        matches!(
            error,
            DrainageError::Opening {
                operation_id: 10,
                issue: OpeningIssue::EntryNotExterior,
            }
        ),
        "{error}"
    );
    let mut predicate = |c| c == Checkpoint::DrainagePublish;
    let error = drain_model_sampled(
        &source,
        &plan(
            2.0,
            0.25,
            &[hole([10.0, 10.0, -1.0], [0.0, 0.0, 1.0], 4.0, 3.0)],
        ),
        SampledDrainageLimits::default(),
        &mut Cancellation::new(&mut predicate),
        &mut |_| {},
    )
    .unwrap_err();
    assert!(matches!(error, DrainageError::Cancelled(_)), "{error}");
}

#[test]
fn drainage_reports_progress_per_hole_and_stops_when_cancelled() {
    let source = boxes(&[([0.0; 3], [20.0; 3], false)]);
    let holes = [
        hole([10.0, 10.0, -1.0], [0.0, 0.0, 1.0], 4.0, 3.0),
        hole([10.0, 10.0, 21.0], [0.0, 0.0, -1.0], 4.0, 3.0),
    ];
    let mut seen = Vec::new();
    let model = drain_model_sampled(
        &source,
        &plan(2.0, 0.5, &holes),
        SampledDrainageLimits::default(),
        &mut Cancellation::never(),
        &mut |p| {
            if p.stage == SampledHollowStage::Drainage {
                seen.push((p.completed_items, p.total_items));
            }
        },
    )
    .unwrap();
    assert_eq!(model.openings().len(), 2);
    assert_eq!(seen, [(0, 2), (1, 2), (2, 2)]);
    let mut predicate = |c| c == Checkpoint::DrainageWork;
    let error = drain_model_sampled(
        &source,
        &plan(2.0, 0.5, &holes),
        SampledDrainageLimits::default(),
        &mut Cancellation::new(&mut predicate),
        &mut |_| {},
    )
    .unwrap_err();
    assert!(matches!(error, DrainageError::Cancelled(_)), "{error}");
}

#[test]
fn clearances_are_certified_down_to_subnormal_gaps_and_use_the_disk_rim() {
    let source = boxes(&[([0.0; 3], [20.0; 3], false)]);
    let limits = |minimum_clearance_mm| SampledDrainageLimits {
        minimum_clearance_mm,
        ..SampledDrainageLimits::default()
    };
    let run = |gap: f64, required: f64| {
        drain_model_sampled(
            &source,
            &plan(
                2.0,
                0.5,
                &[hole([10.0, 10.0, -gap], [0.0, 0.0, 1.0], 10.0, 4.0)],
            ),
            limits(required),
            &mut Cancellation::never(),
            &mut |_| {},
        )
    };
    // Review counterexample: a 1.6e-162 mm gap is not certified as 2e-162 mm of clearance, and
    // a smaller requirement reports no more than the gap.
    let refused = run(1.6e-162, 2e-162).unwrap_err();
    assert!(
        matches!(
            refused,
            DrainageError::Opening {
                operation_id: 10,
                issue: OpeningIssue::EntryNotExterior
            }
        ),
        "{refused}"
    );
    let model = run(1.6e-162, 1e-200).unwrap();
    let reported = model.openings()[0].entry_clearance_mm;
    assert!((1e-200..=1.6e-162).contains(&reported), "{reported}");
    // A separate post beside the entry cap crosses its plane inside the cap's square but
    // outside its disk: the radial bound still reports most of the 0.13 mm gap.
    let mut points = Vec::new();
    let mut triangles = Vec::new();
    for (min, max) in [([0.0; 3], [10.0; 3]), ([5.8, 5.8, -2.0], [6.5, 6.5, -0.5])] {
        let offset = u32::try_from(points.len()).unwrap();
        let (p, t) = cuboid(min, max, false, offset);
        points.extend(p);
        triangles.extend(t);
    }
    let source = mesh(points, triangles);
    let model = drain(
        &source,
        &plan(
            2.0,
            0.5,
            &[hole([5.0, 5.0, -1.0], [0.0, 0.0, 1.0], 4.0, 2.0)],
        ),
    )
    .unwrap();
    let reported = model.openings()[0].entry_clearance_mm;
    let true_distance = 0.8f64.hypot(0.8) - 1.0;
    assert!(
        reported <= true_distance && reported > 0.5 * true_distance,
        "{reported} vs {true_distance}"
    );
}

#[test]
fn touching_bodies_refuse_only_entries_without_a_free_escape() {
    // Review counterexample: an open-top cup with a blind recess and a separate lid resting on
    // its rim. The recess is outside each body individually but sealed by the pair.
    let mut points = Vec::new();
    let mut triangles = Vec::new();
    // Cup: 30 x 30 x 20 box with a 14 x 14 recess from the top down to Z 10, as one closed shell
    // built from an outer box and an inward recess box sharing the top face ring.
    let outer = [
        [0.0, 0.0, 0.0],
        [30.0, 0.0, 0.0],
        [30.0, 30.0, 0.0],
        [0.0, 30.0, 0.0],
    ];
    let top = [
        [0.0, 0.0, 20.0],
        [30.0, 0.0, 20.0],
        [30.0, 30.0, 20.0],
        [0.0, 30.0, 20.0],
    ];
    let rim = [
        [8.0, 8.0, 20.0],
        [22.0, 8.0, 20.0],
        [22.0, 22.0, 20.0],
        [8.0, 22.0, 20.0],
    ];
    let floor = [
        [8.0, 8.0, 10.0],
        [22.0, 8.0, 10.0],
        [22.0, 22.0, 10.0],
        [8.0, 22.0, 10.0],
    ];
    for ring in [outer, top, rim, floor] {
        points.extend(ring);
    }
    let quad = |t: &mut Vec<[u32; 3]>, a: u32, b: u32, c: u32, d: u32| {
        t.push([a, b, c]);
        t.push([a, c, d]);
    };
    // Bottom (outward -Z), outer walls, top annulus (outward +Z), recess walls and recess floor.
    quad(&mut triangles, 0, 3, 2, 1);
    for k in 0..4u32 {
        let n = (k + 1) % 4;
        quad(&mut triangles, k, n, 4 + n, 4 + k);
        quad(&mut triangles, 4 + k, 4 + n, 8 + n, 8 + k);
        quad(&mut triangles, 8 + k, 8 + n, 12 + n, 12 + k);
    }
    quad(&mut triangles, 12, 13, 14, 15);
    let (lid_points, lid_triangles) = cuboid([-2.0, -2.0, 20.0], [32.0, 32.0, 24.0], false, 16);
    points.extend(lid_points);
    triangles.extend(lid_triangles);
    let source = mesh(points, triangles);
    let error = drain(
        &source,
        &plan(
            1.0,
            0.25,
            &[hole([15.0, 15.0, 14.0], [0.0, 0.0, -1.0], 9.0, 2.0)],
        ),
    )
    .unwrap_err();
    assert!(
        matches!(
            error,
            DrainageError::Opening {
                operation_id: 10,
                issue: OpeningIssue::SourceNotEmbedded
            }
        ),
        "{error}"
    );
    // An entry joined to the outside by a free segment below the cup is certified directly,
    // without the embedded-source admission that the touching pair fails.
    let model = drain(
        &source,
        &plan(
            1.0,
            0.25,
            &[hole([4.0, 4.0, -1.0], [0.0, 0.0, 1.0], 3.0, 2.0)],
        ),
    )
    .unwrap();
    assert_eq!(model.openings().len(), 1);
    assert_eq!(model.enclosed_cavities(), model.cavity_regions().len() - 1);
}

#[test]
fn tiny_pockets_far_from_the_origin_are_regions_not_errors() {
    // Review counterexample: one grid vertex lies barely inside the small cube's deep zone,
    // producing a minute pocket whose absolute-coordinate signed volume was rounding noise.
    let t = 1.061_803_398_874_989_5 - 1e-5;
    let side = 2.0 * t + 2e-5;
    let source = boxes(&[
        ([100.0, 90.0, 80.0], [120.0, 110.0, 100.0], false),
        (
            [130.0, 90.0, 80.0],
            [130.0 + side, 90.0 + side, 80.0 + side],
            false,
        ),
    ]);
    let model = drain(
        &source,
        &plan(
            t,
            0.1,
            &[hole([110.0, 100.0, 79.0], [0.0, 0.0, 1.0], 3.0, 3.0)],
        ),
    )
    .unwrap();
    let regions = model.cavity_regions();
    assert!(regions.len() >= 2, "{regions:?}");
    assert!(regions.iter().all(|r| r.volume_mm3 > 0.0), "{regions:?}");
    assert_eq!(model.enclosed_cavities(), regions.len() - 1);
}

#[test]
fn horizontal_holes_tangent_to_layer_planes_and_past_the_panel_slice_cleanly() {
    // 0.05 mm layers sample at 0.025 + 0.05 k, so a 1.25 mm hole centred at Z 10 has its top and
    // bottom prism edges exactly on the planes 10.625 and 9.375. The entry also lies outside the
    // 20 mm panel that starts at the model's X = 0 face.
    let source = boxes(&[([0.0; 3], [20.0; 3], false)]);
    let model = drain(
        &source,
        &plan(
            2.0,
            0.25,
            &[hole([-1.0, 10.0, 10.0], [1.0, 0.0, 0.0], 4.0, 1.25)],
        ),
    )
    .unwrap();
    let volume = drained_volume(&model, [0.0, 0.0], [20.0, 20.0], 400, 0.05);
    let analytic = 8000.0 - 4096.0 - PI * 0.625 * 0.625 * 2.0;
    assert!(
        (volume - analytic).abs() < 0.01 * analytic,
        "{volume} vs {analytic}"
    );
}

#[test]
fn oblique_holes_cross_the_wall_at_their_angle() {
    // A 3 mm hole at 45 degrees in XZ enters the cube's 2 mm side wall at Z 9 and ends inside.
    let source = boxes(&[([0.0; 3], [20.0; 3], false)]);
    let model = drain(
        &source,
        &plan(
            2.0,
            0.25,
            &[hole([-2.0, 10.0, 7.0], [1.0, 0.0, 1.0], 8.0, 3.0)],
        ),
    )
    .unwrap();
    let opening = model.openings()[0];
    assert_eq!(model.enclosed_cavities(), 0);
    assert!(
        opening.radial_error_mm + opening.axial_error_mm <= 1e-4,
        "{opening:?}"
    );
    // The entry cap's nearest point is 2 - 1.5 cos 45 = 0.94 mm from the wall.
    assert!(
        opening.entry_clearance_mm > 0.9 && opening.entry_clearance_mm < 0.95,
        "{opening:?}"
    );
    assert!(opening.terminal_clearance_mm > 0.3, "{opening:?}");
    // The plug removed from the wall: a 45-degree cylinder through 2 mm, pi r^2 2 / cos 45.
    let solid = drained_volume(
        &drain(&source, &plan(2.0, 0.25, &[])).unwrap(),
        [0.0, 0.0],
        [20.0, 20.0],
        80,
        0.25,
    );
    let volume = drained_volume(&model, [0.0, 0.0], [20.0, 20.0], 80, 0.25);
    let plug = PI * 1.5 * 1.5 * 2.0 * 2f64.sqrt();
    assert!(
        (solid - volume - plug).abs() < 0.1 * plug,
        "{} vs {plug}",
        solid - volume
    );
    // Identical inputs give identical cutters.
    let again = drain(
        &source,
        &plan(
            2.0,
            0.25,
            &[hole([-2.0, 10.0, 7.0], [1.0, 0.0, 1.0], 8.0, 3.0)],
        ),
    )
    .unwrap();
    assert_eq!(model.cutters().vertices(), again.cutters().vertices());
}

fn surface_plan(thickness: f64, cell: f64, holes: &[SurfaceHoleSpec]) -> PreparationPlan {
    let base = plan(thickness, cell, &[]);
    let mut operations = base.operations().to_vec();
    for (i, &spec) in holes.iter().enumerate() {
        operations.push(Operation {
            id: 10 + u64::try_from(i).unwrap(),
            kind: OperationKind::SurfaceHole(spec),
        });
    }
    PreparationPlan::new(
        base.source(),
        base.sampling(),
        operations,
        PlanLimits::default(),
        &mut Cancellation::never(),
    )
    .unwrap()
}

fn surface_hole(point: [f64; 3], diameter: f64) -> SurfaceHoleSpec {
    SurfaceHoleSpec {
        surface_mm: point,
        diameter_mm: diameter,
        outside_mm: 1.0,
        inside_mm: 1.0,
        tilt: None,
        depth_mm: None,
    }
}

#[test]
fn surface_holes_follow_the_normal_and_reach_the_cavity() {
    // A point near the cube's -X face: the hole runs along +X from 1 mm outside to 1 mm past the
    // cavity wall at X 2.
    let source = boxes(&[([0.0; 3], [20.0; 3], false)]);
    let model = drain(
        &source,
        &surface_plan(2.0, 0.25, &[surface_hole([-0.4, 10.0, 10.0], 3.0)]),
    )
    .unwrap();
    let opening = model.openings()[0];
    assert_eq!(opening.entry_mm, [-1.0, 10.0, 10.0]);
    assert_eq!(opening.direction.map(f64::abs), [1.0, 0.0, 0.0]);
    assert!(opening.direction[0] > 0.0);
    assert!((opening.length_mm - 4.0).abs() < 0.01, "{opening:?}");
    assert_eq!(model.enclosed_cavities(), 0);
    // On a torus the normal is averaged over the faces within the hole's radius.
    let torus = torus(12.0, 5.0, 96, 48, [0.0, 0.0, 10.0]);
    let model = drain(
        &torus,
        &surface_plan(1.5, 0.2, &[surface_hole([12.0, 0.0, 15.2], 1.5)]),
    )
    .unwrap();
    let opening = model.openings()[0];
    assert!(opening.direction[2] < -0.99, "{opening:?}");
    assert!(
        (opening.length_mm - (1.0 + 1.5 + 1.0)).abs() < 0.05,
        "{opening:?}"
    );
    assert_eq!(model.enclosed_cavities(), 0);
    // A cavity only 1 mm tall cannot hold an end cap 1 mm past its floor: refused, not moved.
    let error = drain(
        &boxes(&[([0.0; 3], [20.0, 20.0, 3.0], false)]),
        &surface_plan(1.0, 0.1, &[surface_hole([10.0, 10.0, -0.5], 1.0)]),
    )
    .unwrap_err();
    assert!(
        matches!(
            error,
            DrainageError::Opening {
                operation_id: 10,
                ..
            }
        ),
        "{error}"
    );
}

#[test]
fn surface_holes_tilt_and_take_a_fixed_depth() {
    use karasu_core::preparation::HoleTilt;
    let source = boxes(&[([0.0; 3], [20.0; 3], false)]);
    // The -X face's normal is horizontal: the reference tangent is +Z, so a 30 degree tilt at
    // azimuth 0 leans the +X axis up towards +Z.
    let tilted = SurfaceHoleSpec {
        tilt: Some(HoleTilt {
            angle_deg: 30.0,
            azimuth_deg: 0.0,
        }),
        ..surface_hole([-0.4, 10.0, 10.0], 3.0)
    };
    let model = drain(&source, &surface_plan(2.0, 0.25, &[tilted])).unwrap();
    let d = model.openings()[0].direction;
    let expected = [30f64.to_radians().cos(), 0.0, 30f64.to_radians().sin()];
    for k in 0..3 {
        assert!((d[k] - expected[k]).abs() < 1e-12, "{d:?}");
    }
    assert_eq!(model.enclosed_cavities(), 0);
    // A fixed depth that stops in the wall is refused; one past the wall drains, at that depth.
    let fixed = |depth| SurfaceHoleSpec {
        depth_mm: Some(depth),
        ..surface_hole([-0.4, 10.0, 10.0], 3.0)
    };
    let error = drain(&source, &surface_plan(2.0, 0.25, &[fixed(1.0)])).unwrap_err();
    assert!(
        matches!(
            error,
            DrainageError::Opening {
                operation_id: 10,
                ..
            }
        ),
        "{error}"
    );
    let model = drain(&source, &surface_plan(2.0, 0.25, &[fixed(3.5)])).unwrap();
    assert_eq!(model.openings()[0].length_mm, 1.0 + 3.5);
    // Tilts beyond the bound are refused by the plan.
    let steep = SurfaceHoleSpec {
        tilt: Some(HoleTilt {
            angle_deg: 61.0,
            azimuth_deg: 0.0,
        }),
        ..surface_hole([-0.4, 10.0, 10.0], 3.0)
    };
    let base = plan(2.0, 0.25, &[]);
    let mut operations = base.operations().to_vec();
    operations.push(Operation {
        id: 10,
        kind: OperationKind::SurfaceHole(steep),
    });
    assert!(PreparationPlan::new(
        base.source(),
        base.sampling(),
        operations,
        PlanLimits::default(),
        &mut Cancellation::never(),
    )
    .is_err());
    // The options are recorded only when given: a tilt adds its marker and two values to the
    // record of a perpendicular hole, which is otherwise unchanged (the command-line comparison
    // suite checks the bytes).
    let plain = surface_plan(2.0, 0.25, &[surface_hole([-0.4, 10.0, 10.0], 3.0)]);
    let with_tilt = surface_plan(2.0, 0.25, &[tilted]);
    assert_eq!(
        with_tilt.canonical_record().len() - plain.canonical_record().len(),
        1 + 2 * 8
    );
    // The 17 bytes are inserted where the hole's record ends; everything else is identical.
    let (a, b) = (plain.canonical_record(), with_tilt.canonical_record());
    let at = a.iter().zip(b).position(|(x, y)| x != y).unwrap();
    assert_eq!(b[at], 0xF1);
    assert_eq!(a[..at], b[..at]);
    assert_eq!(a[at..], b[at + 1 + 2 * 8..]);
}

#[test]
fn a_probe_certifies_one_hole_exactly_as_drainage_does() {
    use karasu_core::preparation::sampled::{DrainageProbe, PROBE_OPERATION_ID};
    let source = boxes(&[([0.0; 3], [20.0; 3], false)]);
    let centre = surface_hole([-0.4, 10.0, 10.0], 3.0);
    let plan = surface_plan(2.0, 0.25, &[centre]);
    let model = drain(&source, &plan).unwrap();
    let mut probe = DrainageProbe::new(
        model.hollowed_model(),
        SampledDrainageLimits::default(),
        plan.sampling().distance_error_mm,
        &mut Cancellation::never(),
    )
    .unwrap();
    // The same hole, probed alone, is the opening drainage certified.
    let probed = probe
        .check(
            &OperationKind::SurfaceHole(centre),
            &[],
            &mut Cancellation::never(),
        )
        .unwrap();
    let mut expected = model.openings()[0];
    expected.operation_id = PROBE_OPERATION_ID;
    assert_eq!(probed.opening, expected);
    assert_eq!(probed.cutter.vertices(), model.cutters().vertices());
    // The probe also gives the hole's frame: its foot on the -X face and the outward normal.
    let frame = probed.frame.unwrap();
    assert_eq!(frame.foot_mm, [0.0, 10.0, 10.0]);
    assert_eq!(frame.outward, [-1.0, 0.0, 0.0]);
    // A hole at the cube's edge cannot end inside the cavity; its shape is still given.
    let edge = surface_hole([-0.4, 0.5, 10.0], 3.0);
    let refused = probe
        .check(
            &OperationKind::SurfaceHole(edge),
            &[],
            &mut Cancellation::never(),
        )
        .unwrap_err();
    assert!(matches!(refused.error, DrainageError::Opening { .. }));
    assert!(refused.spec.is_some() && refused.frame.is_some() && refused.cutter.is_some());
    // A hole next to an existing one is refused as a potential overlap.
    let near = surface_hole([-0.4, 11.0, 10.0], 3.0);
    let refused = probe
        .check(
            &OperationKind::SurfaceHole(near),
            &[probed.bounds],
            &mut Cancellation::never(),
        )
        .unwrap_err();
    assert!(matches!(
        refused.error,
        DrainageError::PotentialOverlap { .. }
    ));
    // A fixed depth that stops in the wall is refused with the cutter it would have.
    let shallow = SurfaceHoleSpec {
        depth_mm: Some(1.0),
        ..centre
    };
    let refused = probe
        .check(
            &OperationKind::SurfaceHole(shallow),
            &[],
            &mut Cancellation::never(),
        )
        .unwrap_err();
    assert_eq!(refused.spec.unwrap().length_mm, 2.0);
    assert!(refused.cutter.is_some());
}

#[test]
fn a_hollowed_model_is_drained_without_hollowing_again() {
    // TD-089: the same cuts certified on an existing hollow as on a fresh one.
    let source = boxes(&[([0.0; 3], [20.0; 3], false)]);
    let holes = [hole([10.0, 10.0, -1.0], [0.0, 0.0, 1.0], 4.0, 3.0)];
    let fresh = drain(&source, &plan(2.0, 0.25, &holes)).unwrap();
    let hollow = || {
        hollow_mesh_sampled(
            &source,
            &plan(2.0, 0.25, &[]),
            SampledHollowLimits::default(),
            &mut Cancellation::never(),
        )
        .unwrap()
    };
    let reused = drain_hollowed_sampled(
        hollow(),
        &plan(2.0, 0.25, &holes),
        SampledDrainageLimits::default(),
        &mut Cancellation::never(),
        &mut |_| {},
    )
    .unwrap();
    assert_eq!(reused.openings(), fresh.openings());
    assert_eq!(reused.cutters(), fresh.cutters());
    assert_eq!(reused.cavity_regions(), fresh.cavity_regions());
    assert_eq!(
        reused.hollowed_model().mesh(),
        fresh.hollowed_model().mesh()
    );
    // A hollow of another thickness does not match the plan.
    assert!(matches!(
        drain_hollowed_sampled(
            hollow(),
            &plan(2.5, 0.25, &holes),
            SampledDrainageLimits::default(),
            &mut Cancellation::never(),
            &mut |_| {},
        ),
        Err(DrainageError::Unsupported { .. })
    ));
}

#[test]
fn a_cavity_inside_overlapping_bodies_is_drained_like_any_other() {
    // Boxes [0, 20] and [10, 30] along x overlap; with 2 mm walls the overlap's void lies inside
    // both (TD-092 H2). A hole from the top reaches it; the other two voids stay enclosed.
    let source = boxes(&[
        ([0.0; 3], [20.0; 3], false),
        ([10.0, 0.0, 0.0], [30.0, 20.0, 20.0], false),
    ]);
    let model = drain(
        &source,
        &plan(
            2.0,
            0.25,
            &[hole([15.0, 10.0, 21.0], [0.0, 0.0, -1.0], 4.0, 2.0)],
        ),
    )
    .unwrap();
    assert_eq!(model.openings().len(), 1);
    assert_eq!(model.cavity_regions().len(), 3);
    assert_eq!(model.enclosed_cavities(), 2);
}
