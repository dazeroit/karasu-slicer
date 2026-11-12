//! Analytic drainage-to-mask fixture; output is not a calibrated printer file.
//! Usage: `drain_reference NEW_OUTPUT_DIRECTORY`.
use karasu_core::{
    cancellation::Cancellation,
    geometry::{ObjectTransform, Point3},
    mesh::TriangleMesh,
    preparation::{
        drainage::{drain_model, DrainageLimits},
        HoleSpec, HollowSpec, Operation, OperationKind, PlanLimits, PreparationPlan, SamplingSpec,
        SourceBinding,
    },
    raster::{
        rasterize_layer, Compensation, LcdProfile, Orientation, OutsidePolicy, RasterConfig,
        RasterLimits,
    },
    slicing::{
        construct::ConstructLimits,
        engine::{LayerConfig, LayerStageLimits},
        grid::BinaryGridSpec,
        regions::RegionFillRule,
        schedule::{build_layer_schedule, ScheduleEnd, ScheduleLimits, UniformScheduleSpec},
    },
};
use std::{
    error::Error,
    io::{self, Write},
    path::PathBuf,
    time::Instant,
};
fn cube(side: f64) -> TriangleMesh {
    let points = [
        [0., 0., 0.],
        [side, 0., 0.],
        [side, side, 0.],
        [0., side, 0.],
        [0., 0., side],
        [side, 0., side],
        [side, side, side],
        [0., side, side],
    ];
    TriangleMesh::try_new(
        points.map(|[x, y, z]| Point3::new(x, y, z)).to_vec(),
        vec![
            [0, 2, 1],
            [0, 3, 2],
            [4, 5, 6],
            [4, 6, 7],
            [0, 1, 5],
            [0, 5, 4],
            [1, 2, 6],
            [1, 6, 5],
            [2, 3, 7],
            [2, 7, 6],
            [3, 0, 4],
            [3, 4, 7],
        ],
    )
    .unwrap()
}

fn recipe() -> Result<PreparationPlan, Box<dyn Error>> {
    Ok(PreparationPlan::new(
        SourceBinding {
            original_sha256: [61; 32],
            placement: ObjectTransform::identity(),
        },
        SamplingSpec {
            cell_mm: 0.1,
            distance_error_mm: 0.001,
            max_cells: 1,
        },
        vec![
            Operation {
                id: 1,
                kind: OperationKind::Hollow(HollowSpec {
                    blockers: Vec::new(),
                    thickness_mm: 2.,
                    guarantee: None,
                }),
            },
            Operation {
                id: 2,
                kind: OperationKind::Hole(HoleSpec {
                    entry_mm: [10., 10., -1.],
                    direction: [0., 0., 1.],
                    length_mm: 4.,
                    diameter_mm: 2.,
                }),
            },
        ],
        PlanLimits::default(),
        &mut Cancellation::never(),
    )?)
}
fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args_os().skip(1);
    let destination = PathBuf::from(args.next().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "usage: drain_reference NEW_OUTPUT_DIRECTORY",
        )
    })?);
    if args.next().is_some() || destination.exists() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "exactly one absent output directory is required",
        )
        .into());
    }
    let source = cube(20.);
    let plan = recipe()?;
    let config = LayerConfig {
        grid: BinaryGridSpec::try_new([0., 0.], 0)?,
        fill_rule: RegionFillRule::EvenOddStrict,
        stages: LayerStageLimits::default(),
        max_output_payload_bytes: 64 * 1024 * 1024,
    };
    let profile = LcdProfile::try_new([200, 200], [0., 0.], [20., 20.], Orientation::default())?;
    let raster = RasterConfig {
        samples_per_axis: 1,
        grayscale_levels: 2,
        minimum_gray: 0,
        outside: OutsidePolicy::Reject,
        compensation: Compensation::None,
        kept: karasu_core::raster::KeptSampling::Antialiased,
    };
    let planes = [
        (1., "z1.pgm"),
        (2.5, "z2_5.pgm"),
        (3., "z3.pgm"),
        (19., "z19.pgm"),
    ];
    let schedules = planes
        .iter()
        .map(|&(z, _)| -> Result<_, Box<dyn Error>> {
            Ok(build_layer_schedule(
                UniformScheduleSpec::try_new(z, 1., ScheduleEnd::Count(1))?,
                ScheduleLimits {
                    max_layers: 1,
                    max_payload_bytes: 1024,
                },
            )?)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let start = Instant::now();
    let drained = drain_model(
        &source,
        &plan,
        DrainageLimits::default(),
        &mut Cancellation::never(),
    )?;
    let mut images = Vec::new();
    for schedule in &schedules {
        let mut engine = drained.layer_engine(
            schedule,
            config,
            ConstructLimits::default(),
            Cancellation::never(),
            None,
        );
        let layer = engine
            .next()
            .ok_or_else(|| io::Error::other("missing scheduled layer"))??;
        if engine.next().is_some() {
            return Err(io::Error::other("unexpected additional layer").into());
        }
        images.push(rasterize_layer(
            &layer,
            profile,
            raster,
            RasterLimits::default(),
        )?);
    }
    let elapsed_ms = start.elapsed().as_secs_f64() * 1000.;
    let report = report(&drained, &plan, &planes, elapsed_ms)?;
    std::fs::create_dir(&destination)?;
    for ((_, filename), image) in planes.iter().zip(images) {
        let mut file = std::fs::File::create(destination.join(filename))?;
        file.write_all(b"P5\n200 200\n255\n")?;
        file.write_all(&image.to_pixels()?)?;
    }
    std::fs::write(
        destination.join("report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{}", destination.display());
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    bytes
        .iter()
        .flat_map(|&byte| {
            [
                char::from(DIGITS[usize::from(byte >> 4)]),
                char::from(DIGITS[usize::from(byte & 15)]),
            ]
        })
        .collect()
}

fn report(
    drained: &karasu_core::preparation::drainage::DrainedModel,
    plan: &PreparationPlan,
    planes: &[(f64, &str)],
    elapsed_ms: f64,
) -> Result<serde_json::Value, io::Error> {
    let opening = drained
        .openings()
        .first()
        .ok_or_else(|| io::Error::other("missing certified opening"))?;
    Ok(serde_json::json!({
        "schema": 1,
        "fixture": "cube20-bottom-cut-v1",
        "backend": drained.backend_id(),
        "elapsed_ms": elapsed_ms,
        "panel_dimensions": [200, 200],
        "panel_extent_mm": [20, 20],
        "samples_per_axis": 1,
        "grayscale_levels": 2,
        "compensation": "none",
        "source_stamp": "synthetic caller assertion, not an asset hash",
        "plan_record_hex": hex(plan.canonical_record()),
        "enclosed_cavities": drained.enclosed_cavities(),
        "radial_error_mm": opening.radial_error_mm,
        "axial_error_mm": opening.axial_error_mm,
        "hollow_offset_error_mm": drained.hollowed_model().certificate().wall_offset_error_mm,
        "segments": opening.segments,
        "planes": planes.iter().map(|&(z, file)|
            serde_json::json!({"z_mm": z, "file": file})).collect::<Vec<_>>()
    }))
}
