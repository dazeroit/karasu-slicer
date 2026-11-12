//! Deterministic parametric-support geometry and layer-mask fixture; no printer calibration claim.
//! Usage: `support_reference NEW_OUTPUT_DIRECTORY`.
//! Also streams five synthetic layers to CTB/GOO/Photon/PP1 and reference PNGs.
//! The 4.5 mm layer spacing intentionally traverses support/model sections; it is not a print preset.
use karasu_core::{
    cancellation::Cancellation,
    export::{
        export_machine, export_reference, BuiltinExporter, BuiltinFormat, ExportLimits, PixelPolicy,
    },
    geometry::{ObjectTransform, Point3, Rotation3, Vector3},
    mesh::TriangleMesh,
    preparation::{
        supports::{
            generate_supports, SupportContact, SupportIntersectionPolicy, SupportLimits,
            SupportPlan, SupportProfile, SupportRecipe, SupportStation, SupportedModel,
        },
        HollowSpec, Operation, OperationKind, PlanLimits, PreparationPlan, SamplingSpec,
        SourceBinding,
    },
    print_job::{JobLimits, PrintJob},
    profiles::{
        LayerSettings, MotionStage, PrintSettings, PrinterCalibration, PrinterLimits,
        PrinterProfile, ResinProfile,
    },
    raster::{
        rasterize_layer, stream::RasterStream, Compensation, LcdProfile, Orientation,
        OutsidePolicy, RasterConfig, RasterLimits,
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

fn model_plan() -> PreparationPlan {
    PreparationPlan::new(
        SourceBinding {
            original_sha256: [91; 32],
            placement: ObjectTransform::new(
                Vector3::new(0., 0., 10.),
                Rotation3::identity(),
                Vector3::new(1., 1., 1.),
                Point3::new(0., 0., 0.),
            )
            .unwrap(),
        },
        SamplingSpec {
            cell_mm: 0.1,
            distance_error_mm: 0.001,
            max_cells: 1,
        },
        vec![Operation {
            id: 1,
            kind: OperationKind::Hollow(HollowSpec {
                blockers: Vec::new(),
                thickness_mm: 2.,
                guarantee: None,
            }),
        }],
        PlanLimits::default(),
        &mut Cancellation::never(),
    )
    .unwrap()
}
fn station(z: f64, size: f64) -> SupportStation {
    SupportStation {
        center_mm: [12., 8., z],
        rotation: Rotation3::identity(),
        profile: SupportProfile::Rectangle {
            size_mm: [size, size],
        },
    }
}
fn recipe(model: &PreparationPlan) -> SupportRecipe {
    SupportRecipe {
        id: 1,
        surface: model.model_key(),
        contact: SupportContact {
            source_triangle: 0,
            position_mm: [12., 8., 10.],
            maximum_penetration_mm: 0.5,
        },
        stations: vec![
            station(0., 4.),
            station(1., 4.),
            station(9., 1.),
            station(10.3, 0.4),
        ],
    }
}

fn mesh_json(mesh: &TriangleMesh) -> serde_json::Value {
    serde_json::json!({"vertices":mesh.vertices().iter().map(|p|p.coordinates()).collect::<Vec<_>>(),"triangles":mesh.triangles()})
}
fn layer(exposure: f64) -> LayerSettings {
    LayerSettings {
        exposure_s: exposure,
        wait_before_exposure_s: 0.,
        wait_after_exposure_s: 0.,
        wait_after_lift_s: 0.,
        pwm: 255,
        lift: vec![MotionStage {
            distance_mm: 1.,
            speed_mm_s: 1.,
        }],
        retract: vec![MotionStage {
            distance_mm: 1.,
            speed_mm_s: 1.,
        }],
    }
}
fn profiles() -> (PrinterProfile, ResinProfile) {
    let printer = PrinterProfile {
        schema_version: 1,
        id: "karasu-support-format-fixture".into(),
        name: "Uncalibrated support fixture".into(),
        revision: 1,
        build_size_mm: [20., 20., 40.],
        resolution_px: [200, 200],
        origin_xy_mm: [0., 0.],
        swap_xy: false,
        mirror_x: false,
        mirror_y: false,
        format_ids: vec![
            "ctb-v2-plain".into(),
            "goo-v3".into(),
            "anycubic-pp1-v3".into(),
            "photon-v1-firmware-motion".into(),
        ],
        limits: PrinterLimits {
            min_layer_height_mm: 0.02,
            max_layer_height_mm: 5.,
            max_exposure_s: 120.,
            max_speed_mm_s: 100.,
            max_lift_mm: 20.,
        },
        vendor: None,
        model: None,
        based_on: None,
        fixed_layer_time_s: None,
    };
    let resin = ResinProfile {
        schema_version: 1,
        id: "uncalibrated-support-fixture".into(),
        name: "Uncalibrated format test resin".into(),
        revision: 1,
        density_g_per_ml: None,
        cost_per_liter: None,
        currency: None,
        calibrations: vec![PrinterCalibration {
            printer_id: printer.id.clone(),
            settings: PrintSettings {
                heating_temperature_c: None,
                layer_height_mm: 4.5,
                bottom_layer_count: 1,
                transition_layer_count: 1,
                bottom: layer(10.),
                normal: layer(2.),
            },
        }],
    };
    (printer, resin)
}
fn export_jobs(
    destination: &std::path::Path,
    supported: &SupportedModel,
    config: &LayerConfig,
) -> Result<(), Box<dyn Error>> {
    let (printer, resin) = profiles();
    let schedule = build_layer_schedule(
        UniformScheduleSpec::try_new(0.5, 4.5, ScheduleEnd::Count(5))?,
        ScheduleLimits {
            max_layers: 5,
            max_payload_bytes: 4096,
        },
    )?;
    let job = PrintJob::new(
        "parametric-support-fixture",
        &printer,
        &resin,
        &schedule,
        &[],
        JobLimits::default(),
    )?;
    let lcd = printer.lcd_profile()?;
    let raster = RasterConfig {
        samples_per_axis: 1,
        grayscale_levels: 2,
        minimum_gray: 0,
        outside: OutsidePolicy::Reject,
        compensation: Compensation::None,
        kept: karasu_core::raster::KeptSampling::Antialiased,
    };
    for (format, name) in [
        (BuiltinFormat::CtbV2Plain, "support.ctb"),
        (BuiltinFormat::GooV3, "support.goo"),
        (BuiltinFormat::PhotonV1FirmwareMotion, "support.photon"),
        (BuiltinFormat::AnycubicPhotonP1, "support.pp1"),
    ] {
        let layers = supported.layer_engine(
            &schedule,
            *config,
            ConstructLimits::default(),
            Cancellation::never(),
            None,
        );
        let source = RasterStream::new(
            layers,
            lcd,
            raster,
            RasterLimits::default(),
            Cancellation::never(),
        );
        export_machine(
            &destination.join(name),
            &job,
            &BuiltinExporter::new(format, PixelPolicy::Exact),
            source,
            ExportLimits::default(),
            &mut Cancellation::never(),
        )?;
    }
    let layers = supported.layer_engine(
        &schedule,
        *config,
        ConstructLimits::default(),
        Cancellation::never(),
        None,
    );
    let source = RasterStream::new(
        layers,
        lcd,
        raster,
        RasterLimits::default(),
        Cancellation::never(),
    );
    export_reference(
        &destination.join("reference"),
        &job,
        source,
        ExportLimits::default(),
        &mut Cancellation::never(),
    )?;
    Ok(())
}
fn destination() -> Result<PathBuf, Box<dyn Error>> {
    let mut args = std::env::args_os().skip(1);
    let destination = PathBuf::from(args.next().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "usage: support_reference NEW_OUTPUT_DIRECTORY",
        )
    })?);
    if args.next().is_some() || destination.exists() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "exactly one absent output directory is required",
        )
        .into());
    }
    Ok(destination)
}
fn main() -> Result<(), Box<dyn Error>> {
    let destination = destination()?;
    let source = cube(20.);
    let model = model_plan();
    let support = recipe(&model);
    let authored=support.stations.iter().map(|s|serde_json::json!({"center_mm":s.center_mm,"rectangle_full_size_mm":match &s.profile{SupportProfile::Rectangle{size_mm}=>*size_mm,_=>unreachable!("owned fixture has rectangular sections")}})).collect::<Vec<_>>();
    let plan = SupportPlan::new(
        model,
        vec![support],
        SupportIntersectionPolicy::RejectUncertified,
        SupportLimits::default(),
        &mut Cancellation::never(),
    )?;
    let start = Instant::now();
    let supported = generate_supports(
        &source,
        &plan,
        SupportLimits::default(),
        &mut Cancellation::never(),
    )?;
    let geometry_elapsed_ms = start.elapsed().as_secs_f64() * 1000.;
    let config = LayerConfig {
        grid: BinaryGridSpec::try_new([0., 0.], -16)?,
        fill_rule: RegionFillRule::EvenOddStrict,
        stages: LayerStageLimits::default(),
        max_output_payload_bytes: 64 * 1024 * 1024,
    };
    let panel = LcdProfile::try_new([200, 200], [0., 0.], [20., 20.], Orientation::default())?;
    let raster = RasterConfig {
        samples_per_axis: 1,
        grayscale_levels: 2,
        minimum_gray: 0,
        outside: OutsidePolicy::Reject,
        compensation: Compensation::None,
        kept: karasu_core::raster::KeptSampling::Antialiased,
    };
    let planes = [
        (0.5, "z0_5.pgm"),
        (5., "z5.pgm"),
        (9.5, "z9_5.pgm"),
        (11., "z11.pgm"),
        (13., "z13.pgm"),
    ];
    let mut images = Vec::new();
    for (z, _) in planes {
        let schedule = build_layer_schedule(
            UniformScheduleSpec::try_new(z, 1., ScheduleEnd::Count(1))?,
            ScheduleLimits {
                max_layers: 1,
                max_payload_bytes: 1024,
            },
        )?;
        let mut engine = supported.layer_engine(
            &schedule,
            config,
            ConstructLimits::default(),
            Cancellation::never(),
            None,
        );
        let layer = engine
            .next()
            .ok_or_else(|| io::Error::other("missing layer"))??;
        if engine.next().is_some() {
            return Err(io::Error::other("unexpected layer").into());
        }
        images.push(rasterize_layer(
            &layer,
            panel,
            raster,
            RasterLimits::default(),
        )?);
    }
    let certificates=supported.certificates().iter().map(|c|serde_json::json!({"support_id":c.support_id,"minimum_penetration_mm":c.minimum_penetration_mm,"maximum_penetration_mm":c.maximum_penetration_mm,"clearance_mm":c.clearance_mm})).collect::<Vec<_>>();
    let geometry = serde_json::json!({"schema":1,"fixture":"cube20-z10-rectangular-support-v1","source":mesh_json(&source),"stations":authored,"components":supported.components().iter().map(|c|mesh_json(c.mesh())).collect::<Vec<_>>()});
    let report = serde_json::json!({"schema":1,"fixture":"cube20-z10-rectangular-support-v1","geometry_elapsed_ms":geometry_elapsed_ms,"source_stamp":"synthetic caller assertion, not an asset hash","panel_dimensions":[200,200],"panel_extent_mm":[20.,20.],"grid_exponent_mm":-16,"samples_per_axis":1,"grayscale_levels":2,"certificates":certificates,"plan_record":supported.plan_record(),"planes":planes.iter().map(|(z,file)|serde_json::json!({"z_mm":z,"file":file})).collect::<Vec<_>>()});
    std::fs::create_dir(&destination)?;
    for ((_, name), image) in planes.iter().zip(images) {
        let mut file = std::fs::File::create(destination.join(name))?;
        file.write_all(b"P5\n200 200\n255\n")?;
        file.write_all(&image.to_pixels()?)?;
    }
    std::fs::write(
        destination.join("geometry.json"),
        serde_json::to_vec_pretty(&geometry)?,
    )?;
    std::fs::write(
        destination.join("report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    export_jobs(&destination, &supported, &config)?;
    println!("{}", destination.display());
    Ok(())
}
