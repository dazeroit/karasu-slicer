//! Independently owned M5 sample jobs, streamed through M3/M4 and versioned exporters.
//!
//! Usage: `print_reference OUTPUT_DIRECTORY [--p1] [LAYER_COUNT]`.
//! The output directory must not exist. Default generates small CTB/GOO/Photon/PP1/reference jobs.
//! --p1 uses publicly documented P1 envelope/resolution, explicit uncalibrated origin/orientation,
//! and emits PP1 plus reference PNGs. It is a format/size fixture, not a calibrated print preset.
use karasu_core::{
    cancellation::Cancellation,
    export::{
        export_machine, export_reference, BuiltinExporter, BuiltinFormat, ExportLimits, PixelPolicy,
    },
    geometry::Point3,
    mesh::TriangleMesh,
    print_job::{JobLimits, PrintJob},
    profiles::{
        LayerSettings, MotionStage, PrintSettings, PrinterCalibration, PrinterLimits,
        PrinterProfile, ResinProfile,
    },
    raster::{stream::RasterStream, Compensation, OutsidePolicy, RasterConfig, RasterLimits},
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
use std::{error::Error, io, path::PathBuf, time::Instant};
fn mesh() -> Result<TriangleMesh, Box<dyn Error>> {
    let mut vertices = Vec::new();
    let mut triangles = Vec::new();
    for ring in [
        [[5., 5.], [95., 5.], [95., 55.], [5., 55.]],
        [[25., 15.], [35., 15.], [35., 25.], [25., 25.]],
    ] {
        let base = u32::try_from(vertices.len())?;
        for [x, y] in ring {
            vertices.extend([Point3::new(x, y, 0.), Point3::new(x, y, 200.)]);
        }
        for i in 0..4u32 {
            let a = base + 2 * i;
            let b = base + 2 * ((i + 1) % 4);
            triangles.extend([[a, b, b + 1], [a, b + 1, a + 1]]);
        }
    }
    Ok(TriangleMesh::try_new(vertices, triangles)?)
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
fn source<'a>(
    mesh: &'a TriangleMesh,
    plan: &'a LayerSchedule,
    printer: &PrinterProfile,
) -> Result<RasterStream<'static, LayerEngine<'a, 'static, 'static>>, Box<dyn Error>> {
    let layers = LayerEngine::new(
        mesh,
        plan,
        SectionTransform::Identity,
        LayerConfig {
            grid: BinaryGridSpec::try_new([0., 0.], -20)?,
            fill_rule: RegionFillRule::EvenOddStrict,
            stages: LayerStageLimits::default(),
            max_output_payload_bytes: 64 * 1024 * 1024,
        },
        Cancellation::never(),
        None,
    )
    .with_represented_coordinates();
    Ok(RasterStream::new(
        layers,
        printer.lcd_profile()?,
        RasterConfig {
            samples_per_axis: 1,
            grayscale_levels: 256,
            minimum_gray: 0,
            outside: OutsidePolicy::Reject,
            compensation: Compensation::None,
            kept: karasu_core::raster::KeptSampling::Antialiased,
        },
        RasterLimits {
            max_pixels: 128 * 1024 * 1024,
            ..RasterLimits::default()
        },
        Cancellation::never(),
    ))
}
fn profiles(p1: bool) -> (PrinterProfile, ResinProfile) {
    let printer = PrinterProfile {
        schema_version: 1,
        id: if p1 {
            "anycubic-photon-p1-sample".into()
        } else {
            "karasu-sample".into()
        },
        name: if p1 {
            "Anycubic Photon P1".into()
        } else {
            "Karasu analytic sample".into()
        },
        revision: 1,
        build_size_mm: if p1 {
            [223., 126., 230.]
        } else {
            [100., 60., 230.]
        },
        resolution_px: if p1 { [13312, 5120] } else { [64, 40] },
        origin_xy_mm: [0., 0.],
        swap_xy: false,
        mirror_x: false,
        mirror_y: false,
        format_ids: if p1 {
            vec!["anycubic-pp1-v3".into()]
        } else {
            vec![
                "ctb-v2-plain".into(),
                "goo-v3".into(),
                "anycubic-pp1-v3".into(),
                "photon-v1-firmware-motion".into(),
            ]
        },
        limits: PrinterLimits {
            min_layer_height_mm: 0.02,
            max_layer_height_mm: 0.15,
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
        id: "uncalibrated-test-resin".into(),
        name: "Uncalibrated format test resin".into(),
        revision: 1,
        density_g_per_ml: None,
        cost_per_liter: None,
        currency: None,
        calibrations: vec![PrinterCalibration {
            printer_id: printer.id.clone(),
            settings: PrintSettings {
                heating_temperature_c: None,
                layer_height_mm: 0.05,
                bottom_layer_count: 1,
                transition_layer_count: 1,
                bottom: layer(10.),
                normal: layer(2.),
            },
        }],
    };
    (printer, resin)
}
fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args_os().skip(1);
    let destination = PathBuf::from(args.next().ok_or_else(|| {
        io::Error::other("usage: print_reference OUTPUT_DIRECTORY [--p1] [LAYER_COUNT]")
    })?);
    let options: Vec<_> = args.collect();
    let p1 = options.first().is_some_and(|s| s == "--p1");
    let count_index = usize::from(p1);
    if options.len() > count_index + 1 {
        return Err(io::Error::other("unexpected sample job arguments").into());
    }
    let count: u32 = options.get(count_index).map_or(Ok(3), |v| {
        v.to_str()
            .ok_or_else(|| io::Error::other("layer count must be Unicode"))
            .and_then(|v| v.parse().map_err(io::Error::other))
    })?;
    if !(3..=2000).contains(&count) {
        return Err(io::Error::other("sample layer count must be 3..2000").into());
    }
    std::fs::create_dir(&destination)?;
    let (printer, resin) = profiles(p1);
    let plan = build_layer_schedule(
        UniformScheduleSpec::try_new(0.025, 0.05, ScheduleEnd::Count(u64::from(count)))?,
        ScheduleLimits {
            max_layers: u64::from(count),
            max_payload_bytes: 1024 * 1024,
        },
    )?;
    let job = PrintJob::new(
        "analytic-m5-sample",
        &printer,
        &resin,
        &plan,
        &[],
        JobLimits::default(),
    )?;
    let mesh = mesh()?;
    let limits = ExportLimits::default();
    let formats = if p1 {
        vec![(BuiltinFormat::AnycubicPhotonP1, "sample.pp1")]
    } else {
        vec![
            (BuiltinFormat::CtbV2Plain, "sample.ctb"),
            (BuiltinFormat::GooV3, "sample.goo"),
            (BuiltinFormat::PhotonV1FirmwareMotion, "sample.photon"),
            (BuiltinFormat::AnycubicPhotonP1, "sample.pp1"),
        ]
    };
    for (format, name) in formats {
        let start = Instant::now();
        let summary = export_machine(
            &destination.join(name),
            &job,
            &BuiltinExporter::new(format, PixelPolicy::Exact),
            source(&mesh, &plan, &printer)?,
            limits,
            &mut Cancellation::never(),
        )?;
        println!(
            "{name} layers={} bytes={} source_coverage_mm3={} elapsed_ms={:.3}",
            summary.layers,
            summary.output_bytes,
            summary.coverage_volume_mm3,
            start.elapsed().as_secs_f64() * 1000.
        );
    }
    let start = Instant::now();
    let summary = export_reference(
        &destination.join("reference"),
        &job,
        source(&mesh, &plan, &printer)?,
        limits,
        &mut Cancellation::never(),
    )?;
    println!(
        "reference layers={} bytes={} elapsed_ms={:.3}",
        summary.layers,
        summary.output_bytes,
        start.elapsed().as_secs_f64() * 1000.
    );
    Ok(())
}
