//! End-to-end streamed export contracts through independently generated M3/M4 masks.
use karasu_core::{
    cancellation::{Cancellation, Checkpoint},
    export::{
        export_machine, export_reference, ExportError, ExportLimits, ExportRegistry, ExportSession,
        MachineExporter, WriteSeek,
    },
    geometry::Point3,
    mesh::TriangleMesh,
    print_job::{JobLayer, JobLimits, PrintJob},
    profiles::{
        LayerSettings, MotionStage, PrintSettings, PrinterCalibration, PrinterLimits,
        PrinterProfile, ResinProfile,
    },
    raster::{
        rasterize_layer, Compensation, OutsidePolicy, RasterConfig, RasterImage, RasterLimits,
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
    fs,
    io::{self},
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "karasu-export-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn assert_empty(&self) {
        assert_eq!(fs::read_dir(&self.0).unwrap().count(), 0);
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn printer() -> PrinterProfile {
    PrinterProfile {
        schema_version: 1,
        id: "export-printer".into(),
        name: "Independent export printer".into(),
        revision: 1,
        build_size_mm: [8., 6., 10.],
        resolution_px: [8, 6],
        origin_xy_mm: [0., 0.],
        swap_xy: false,
        mirror_x: false,
        mirror_y: false,
        format_ids: vec!["toy-v1".into()],
        limits: PrinterLimits {
            min_layer_height_mm: 0.01,
            max_layer_height_mm: 1.,
            max_exposure_s: 100.,
            max_speed_mm_s: 20.,
            max_lift_mm: 5.,
        },
        vendor: None,
        model: None,
        based_on: None,
        fixed_layer_time_s: None,
    }
}
fn layer(exposure_s: f64) -> LayerSettings {
    LayerSettings {
        exposure_s,
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
            speed_mm_s: 2.,
        }],
    }
}
fn resin() -> ResinProfile {
    ResinProfile {
        schema_version: 1,
        id: "export-resin".into(),
        name: "Independent export resin".into(),
        revision: 1,
        density_g_per_ml: Some(1.),
        cost_per_liter: None,
        currency: None,
        calibrations: vec![PrinterCalibration {
            printer_id: "export-printer".into(),
            settings: PrintSettings {
                heating_temperature_c: None,
                layer_height_mm: 0.125,
                bottom_layer_count: 1,
                transition_layer_count: 1,
                bottom: layer(5.),
                normal: layer(1.),
            },
        }],
    }
}
fn schedule(first: f64, height: f64, count: u64) -> LayerSchedule {
    build_layer_schedule(
        UniformScheduleSpec::try_new(first, height, ScheduleEnd::Count(count)).unwrap(),
        ScheduleLimits {
            max_layers: 20,
            max_payload_bytes: 10_000,
        },
    )
    .unwrap()
}
fn job() -> PrintJob {
    PrintJob::new(
        "export-job",
        &printer(),
        &resin(),
        &schedule(0.0625, 0.125, 3),
        &[],
        JobLimits::default(),
    )
    .unwrap()
}
fn images(plan: &LayerSchedule, profile: &PrinterProfile) -> Vec<Result<RasterImage, io::Error>> {
    images_with_config(
        plan,
        profile,
        RasterConfig {
            samples_per_axis: 1,
            grayscale_levels: 256,
            minimum_gray: 0,
            outside: OutsidePolicy::Reject,
            compensation: Compensation::None,
            kept: karasu_core::raster::KeptSampling::Antialiased,
        },
    )
}
fn images_with_config(
    plan: &LayerSchedule,
    profile: &PrinterProfile,
    raster: RasterConfig,
) -> Vec<Result<RasterImage, io::Error>> {
    let mut points = Vec::new();
    for [x, y] in [[1., 1.], [3., 1.], [3., 3.], [1., 3.]] {
        points.extend([Point3::new(x, y, 0.), Point3::new(x, y, 1.)]);
    }
    let mut triangles = Vec::new();
    for i in 0_u32..4 {
        let a = 2 * i;
        let b = 2 * ((i + 1) % 4);
        triangles.extend([[a, b, b + 1], [a, b + 1, a + 1]]);
    }
    let mesh = TriangleMesh::try_new(points, triangles).unwrap();
    let config = LayerConfig {
        grid: BinaryGridSpec::try_new([0., 0.], -8).unwrap(),
        fill_rule: RegionFillRule::EvenOddStrict,
        stages: LayerStageLimits::default(),
        max_output_payload_bytes: 1 << 20,
    };
    LayerEngine::new(
        &mesh,
        plan,
        SectionTransform::Identity,
        config,
        Cancellation::never(),
        None,
    )
    .map(|layer| {
        Ok(rasterize_layer(
            &layer.unwrap(),
            profile.lcd_profile().unwrap(),
            raster,
            RasterLimits::default(),
        )
        .unwrap())
    })
    .collect()
}
fn valid_images() -> Vec<Result<RasterImage, io::Error>> {
    images(&schedule(0.0625, 0.125, 3), &printer())
}

struct Toy {
    id: &'static str,
    suffix: &'static str,
    fail_finish: bool,
}
impl Toy {
    fn good() -> Self {
        Self {
            id: "toy-v1",
            suffix: "toy",
            fail_finish: false,
        }
    }
}
struct ToySession<'a> {
    writer: &'a mut dyn WriteSeek,
    fail_finish: bool,
}
fn output_error(source: io::Error) -> ExportError {
    ExportError::Io {
        operation: "write test format",
        source,
    }
}
impl ExportSession for ToySession<'_> {
    fn write_layer(
        &mut self,
        layer: &JobLayer,
        image: &RasterImage,
        _: &mut Cancellation<'_>,
    ) -> Result<(), ExportError> {
        self.writer
            .write_all(&layer.index.to_le_bytes())
            .and_then(|()| self.writer.write_all(&image.to_pixels().unwrap()))
            .map_err(output_error)
    }
    fn finish(self: Box<Self>, _: &mut Cancellation<'_>) -> Result<(), ExportError> {
        if self.fail_finish {
            return Err(ExportError::Invalid("test finalization failure".into()));
        }
        self.writer.write_all(b"END").map_err(output_error)
    }
}
impl MachineExporter for Toy {
    fn id(&self) -> &str {
        self.id
    }
    fn extension(&self) -> &str {
        self.suffix
    }
    fn preflight(
        &self,
        _: &PrintJob,
        _: ExportLimits,
        _: &mut Cancellation<'_>,
    ) -> Result<(), ExportError> {
        Ok(())
    }
    fn begin<'a>(
        &self,
        writer: &'a mut dyn WriteSeek,
        _: &PrintJob,
        _: ExportLimits,
        _: &mut Cancellation<'_>,
    ) -> Result<Box<dyn ExportSession + 'a>, ExportError> {
        writer.write_all(b"TOY1").map_err(output_error)?;
        Ok(Box::new(ToySession {
            writer,
            fail_finish: self.fail_finish,
        }))
    }
}

#[test]
fn registry_extends_without_changing_job_or_profile_schema() {
    let mut registry = ExportRegistry::new();
    registry.register(Box::new(Toy::good())).unwrap();
    registry
        .register(Box::new(Toy {
            id: "another-v2",
            suffix: "abc",
            fail_finish: false,
        }))
        .unwrap();
    assert_eq!(registry.ids(), vec!["another-v2", "toy-v1"]);
    assert!(registry.get("toy-v1").is_some());
    assert!(registry.get("toy").is_none());
    assert!(registry.register(Box::new(Toy::good())).is_err());
    for format in [
        Toy {
            id: "Bad",
            suffix: "toy",
            fail_finish: false,
        },
        Toy {
            id: "bad-v1",
            suffix: "../bad",
            fail_finish: false,
        },
    ] {
        assert!(registry.register(Box::new(format)).is_err());
    }
}
#[test]
fn machine_export_streams_all_layers_and_publishes_complete_bytes() {
    let t = Temp::new();
    let destination = t.0.join("job.toy");
    let j = job();
    let result = export_machine(
        &destination,
        &j,
        &Toy::good(),
        valid_images(),
        ExportLimits::default(),
        &mut Cancellation::never(),
    )
    .unwrap();
    let bytes = fs::read(&destination).unwrap();
    assert!(bytes.starts_with(b"TOY1"));
    assert!(bytes.ends_with(b"END"));
    assert_eq!(result.layers, 3);
    assert_eq!(result.output_bytes, u64::try_from(bytes.len()).unwrap());
    assert!((result.coverage_volume_mm3 - 1.5).abs() < 1e-12);
    assert_eq!(fs::read_dir(&t.0).unwrap().count(), 1);
}
#[test]
fn reference_export_records_profiles_actual_planes_and_numbered_pngs() {
    let t = Temp::new();
    let destination = t.0.join("reference");
    let j = job();
    let summary = export_reference(
        &destination,
        &j,
        valid_images(),
        ExportLimits::default(),
        &mut Cancellation::never(),
    )
    .unwrap();
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(destination.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest["schema_version"], 1);
    assert_eq!(manifest["printer"]["id"], "export-printer");
    assert_eq!(manifest["resin"]["id"], "export-resin");
    assert_eq!(manifest["volume_kind"], "sampled_coverage_mm3");
    assert_eq!(
        manifest["duration_kind"],
        "nominal_without_firmware_acceleration_or_sensing"
    );
    assert!(manifest["summary"].get("output_bytes").is_none());
    let records = fs::read_to_string(destination.join("layers.jsonl")).unwrap();
    let records: Vec<serde_json::Value> = records
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect();
    assert_eq!(records.len(), 3);
    assert_eq!(records[0]["layer"]["sample_z_mm"], 0.0625);
    assert_eq!(records[0]["layer"]["position_z_mm"], 0.125);
    assert_eq!(records[0]["layer"]["settings"]["exposure_s"], 5.);
    assert_eq!(records[1]["layer"]["settings"]["exposure_s"], 3.);
    for i in 0..3 {
        let bytes = fs::read(destination.join(format!("layer-{i:08}.png"))).unwrap();
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
    }
    let actual: u64 = fs::read_dir(&destination)
        .unwrap()
        .map(|p| p.unwrap().metadata().unwrap().len())
        .sum();
    assert_eq!(summary.output_bytes, actual);
    assert_eq!(
        manifest["summary"]["payload_bytes_excluding_manifest"],
        actual
            - fs::metadata(destination.join("manifest.json"))
                .unwrap()
                .len()
    );
}
#[test]
fn count_source_and_finalize_failures_publish_nothing() {
    for variant in 0..5 {
        let t = Temp::new();
        let destination = t.0.join("job.toy");
        let mut source = valid_images();
        let mut format = Toy::good();
        match variant {
            0 => {
                source.pop();
            }
            1 => source.extend(valid_images()),
            2 => source[1] = Err(io::Error::other("synthetic layer generation failure")),
            3 => source.push(Err(io::Error::other("synthetic trailing source failure"))),
            _ => format.fail_finish = true,
        }
        let result = export_machine(
            &destination,
            &job(),
            &format,
            source,
            ExportLimits::default(),
            &mut Cancellation::never(),
        );
        assert!(result.is_err());
        assert!(!destination.exists());
        t.assert_empty();
    }
}
#[test]
fn reference_failures_remove_the_whole_partial_directory() {
    for variant in 0..3 {
        let t = Temp::new();
        let destination = t.0.join("reference");
        let mut source = valid_images();
        match variant {
            0 => {
                source.pop();
            }
            1 => source.push(Ok(valid_images().remove(0).unwrap())),
            _ => source[1] = Err(io::Error::other("synthetic reference source failure")),
        }
        assert!(export_reference(
            &destination,
            &job(),
            source,
            ExportLimits::default(),
            &mut Cancellation::never()
        )
        .is_err());
        t.assert_empty();
    }
}
#[test]
fn mismatched_raster_profiles_planes_indices_and_thickness_fail_atomically() {
    for variant in 0..4 {
        let t = Temp::new();
        let destination = t.0.join("job.toy");
        let mut profile = printer();
        let mut plan = schedule(0.0625, 0.125, 3);
        match variant {
            0 => profile.mirror_x = true,
            1 => plan = schedule(0.09375, 0.125, 3),
            2 => plan = schedule(0.0625, 0.25, 3),
            _ => {}
        }
        let mut source = images(&plan, &profile);
        if variant == 3 {
            source.swap(0, 1);
        }
        assert!(export_machine(
            &destination,
            &job(),
            &Toy::good(),
            source,
            ExportLimits::default(),
            &mut Cancellation::never()
        )
        .is_err());
        t.assert_empty();
    }
}
#[test]
fn export_cancellation_at_every_publication_phase_removes_staging() {
    for checkpoint in [
        Checkpoint::ExportStart,
        Checkpoint::ExportLayer,
        Checkpoint::ExportFinish,
        Checkpoint::ExportPublish,
    ] {
        for reference in [false, true] {
            let t = Temp::new();
            let destination = t.0.join(if reference { "reference" } else { "job.toy" });
            let mut predicate = |p| p == checkpoint;
            let mut cancel = Cancellation::new(&mut predicate);
            let result = if reference {
                export_reference(
                    &destination,
                    &job(),
                    valid_images(),
                    ExportLimits::default(),
                    &mut cancel,
                )
            } else {
                export_machine(
                    &destination,
                    &job(),
                    &Toy::good(),
                    valid_images(),
                    ExportLimits::default(),
                    &mut cancel,
                )
            };
            assert!(matches!(result, Err(ExportError::Cancelled(_))));
            t.assert_empty();
        }
    }
}
#[test]
fn existing_files_and_directories_are_unchanged() {
    let t = Temp::new();
    let destination = t.0.join("job.toy");
    fs::write(&destination, b"user data").unwrap();
    assert!(export_machine(
        &destination,
        &job(),
        &Toy::good(),
        valid_images(),
        ExportLimits::default(),
        &mut Cancellation::never()
    )
    .is_err());
    assert_eq!(fs::read(&destination).unwrap(), b"user data");
    let reference = t.0.join("reference");
    fs::create_dir(&reference).unwrap();
    fs::write(reference.join("user-file"), b"keep").unwrap();
    assert!(export_reference(
        &reference,
        &job(),
        valid_images(),
        ExportLimits::default(),
        &mut Cancellation::never()
    )
    .is_err());
    assert_eq!(fs::read(reference.join("user-file")).unwrap(), b"keep");
    assert_eq!(fs::read_dir(&t.0).unwrap().count(), 2);
}
#[test]
fn output_metadata_and_pixel_budgets_fail_without_publication() {
    for reference in [false, true] {
        for limits in [
            ExportLimits {
                max_layers: 2,
                ..ExportLimits::default()
            },
            ExportLimits {
                max_pixels: 47,
                ..ExportLimits::default()
            },
            ExportLimits {
                max_output_bytes: 1,
                ..ExportLimits::default()
            },
        ] {
            let t = Temp::new();
            let destination = t.0.join(if reference { "reference" } else { "job.toy" });
            let result = if reference {
                export_reference(
                    &destination,
                    &job(),
                    valid_images(),
                    limits,
                    &mut Cancellation::never(),
                )
            } else {
                export_machine(
                    &destination,
                    &job(),
                    &Toy::good(),
                    valid_images(),
                    limits,
                    &mut Cancellation::never(),
                )
            };
            assert!(result.is_err());
            t.assert_empty();
        }
    }
    for limits in [
        ExportLimits {
            max_metadata_bytes: 1,
            ..ExportLimits::default()
        },
        ExportLimits {
            max_layer_bytes: 1,
            ..ExportLimits::default()
        },
    ] {
        let t = Temp::new();
        assert!(export_reference(
            &t.0.join("reference"),
            &job(),
            valid_images(),
            limits,
            &mut Cancellation::never()
        )
        .is_err());
        t.assert_empty();
    }
}
#[test]
fn target_identity_and_suffix_require_explicit_matching_selection() {
    for (name, format) in [
        ("job.wrong", Toy::good()),
        (
            "job.toy",
            Toy {
                id: "unknown-v1",
                ..Toy::good()
            },
        ),
    ] {
        let t = Temp::new();
        assert!(export_machine(
            &t.0.join(name),
            &job(),
            &format,
            valid_images(),
            ExportLimits::default(),
            &mut Cancellation::never()
        )
        .is_err());
        t.assert_empty();
    }
}

#[test]
fn explicit_per_layer_raster_configuration_is_preserved_in_reference_metadata() {
    let t = Temp::new();
    let destination = t.0.join("reference");
    let mut source = valid_images();
    let mut alternate = images_with_config(
        &schedule(0.0625, 0.125, 3),
        &printer(),
        RasterConfig {
            samples_per_axis: 2,
            grayscale_levels: 16,
            minimum_gray: 0,
            outside: OutsidePolicy::Reject,
            compensation: Compensation::None,
            kept: karasu_core::raster::KeptSampling::Antialiased,
        },
    );
    source[1] = alternate.remove(1);
    export_reference(
        &destination,
        &job(),
        source,
        ExportLimits::default(),
        &mut Cancellation::never(),
    )
    .unwrap();
    let bytes = fs::read_to_string(destination.join("layers.jsonl")).unwrap();
    let records: Vec<serde_json::Value> = bytes
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect();
    assert_eq!(records[0]["raster"]["samples_per_axis"], 1);
    assert_eq!(records[0]["raster"]["grayscale_levels"], 256);
    assert_eq!(records[1]["raster"]["samples_per_axis"], 2);
    assert_eq!(records[1]["raster"]["grayscale_levels"], 16);
    assert_eq!(records[1]["raster"]["outside"], "reject");
    assert_eq!(records[1]["raster"]["compensation"]["kind"], "none");
    assert_eq!(
        records[1]["raster"]["compensation"]["radius_px"],
        serde_json::json!([0, 0])
    );
}

#[test]
fn checksum_matches_an_independent_bitwise_oracle_and_can_be_cancelled() {
    let t = Temp::new();
    let source = valid_images();
    let pixels = source[0].as_ref().unwrap().to_pixels().unwrap();
    let mut expected = u32::MAX;
    for &byte in &pixels {
        expected ^= u32::from(byte);
        for _ in 0..8 {
            expected = (expected >> 1) ^ if expected & 1 == 0 { 0 } else { 0xedb8_8320 };
        }
    }
    let expected = !expected;
    let destination = t.0.join("reference");
    export_reference(
        &destination,
        &job(),
        source,
        ExportLimits::default(),
        &mut Cancellation::never(),
    )
    .unwrap();
    let text = fs::read_to_string(destination.join("layers.jsonl")).unwrap();
    let record: serde_json::Value = serde_json::from_str(text.lines().next().unwrap()).unwrap();
    assert_eq!(record["mask_crc32"], expected);
    let t = Temp::new();
    let mut checks = 0;
    let mut predicate = |checkpoint| {
        if checkpoint == Checkpoint::ExportLayer {
            checks += 1;
        }
        checkpoint == Checkpoint::ExportLayer && checks == 2
    };
    let mut cancel = Cancellation::new(&mut predicate);
    assert!(matches!(
        export_reference(
            &t.0.join("cancelled"),
            &job(),
            valid_images(),
            ExportLimits::default(),
            &mut cancel
        ),
        Err(ExportError::Cancelled(_))
    ));
    t.assert_empty();
}

#[test]
fn large_override_manifest_obeys_streaming_metadata_limit_and_cleans_staging() {
    let t = Temp::new();
    let mut p = printer();
    p.build_size_mm[2] = 1000.;
    let schedule = build_layer_schedule(
        UniformScheduleSpec::try_new(0.0625, 0.125, ScheduleEnd::Count(2000)).unwrap(),
        ScheduleLimits {
            max_layers: 2000,
            max_payload_bytes: 100_000,
        },
    )
    .unwrap();
    let overrides: Vec<_> = (0..2000)
        .map(|index| karasu_core::print_job::LayerOverride {
            index,
            settings: layer(2.),
        })
        .collect();
    let j = PrintJob::new(
        "large-reference",
        &p,
        &resin(),
        &schedule,
        &overrides,
        JobLimits::default(),
    )
    .unwrap();
    let result = export_reference(
        &t.0.join("reference"),
        &j,
        images(&schedule, &p),
        ExportLimits {
            max_metadata_bytes: 1024,
            ..ExportLimits::default()
        },
        &mut Cancellation::never(),
    );
    assert!(result.is_err());
    t.assert_empty();
}

#[test]
fn all_builtin_dialects_publish_deterministic_complete_files() {
    use karasu_core::export::{BuiltinExporter, BuiltinFormat, PixelPolicy};
    for kind in [
        BuiltinFormat::CtbV2Plain,
        BuiltinFormat::GooV3,
        BuiltinFormat::AnycubicPhotonP1,
        BuiltinFormat::PhotonV1FirmwareMotion,
    ] {
        let exporter = BuiltinExporter::new(kind, PixelPolicy::Exact);
        let mut printer = printer();
        printer.format_ids = vec![exporter.id().into()];
        let plan = schedule(0.0625, 0.125, 3);
        let job = PrintJob::new(
            "builtin-job",
            &printer,
            &resin(),
            &plan,
            &[],
            JobLimits::default(),
        )
        .unwrap();
        let temp = Temp::new();
        let mut previous = None;
        for index in 0..2 {
            let path = temp.0.join(format!("job-{index}.{}", exporter.extension()));
            let summary = export_machine(
                &path,
                &job,
                &exporter,
                images(&plan, &printer),
                ExportLimits::default(),
                &mut Cancellation::never(),
            )
            .unwrap();
            let bytes = fs::read(&path).unwrap();
            assert_eq!(summary.output_bytes, u64::try_from(bytes.len()).unwrap());
            assert_eq!(summary.layers, 3);
            if let Some(prior) = previous {
                assert_eq!(bytes, prior, "deterministic {kind:?}");
            }
            previous = Some(bytes);
        }
        assert_eq!(fs::read_dir(&temp.0).unwrap().count(), 2);
    }
}

#[test]
fn swapped_panels_declare_bed_lengths_along_their_pixel_axes() {
    use karasu_core::export::{BuiltinExporter, BuiltinFormat, PixelPolicy};
    // World build 6 x 8 mm; with `swap_xy` the 8 pixel columns cover world Y (8 mm) and the 6
    // rows cover world X (6 mm): square 1 mm pixels. Headers pair lengths with pixel counts.
    let exporter = BuiltinExporter::new(BuiltinFormat::CtbV2Plain, PixelPolicy::Exact);
    let mut printer = printer();
    printer.build_size_mm = [6., 8., 10.];
    printer.swap_xy = true;
    printer.format_ids = vec![exporter.id().into()];
    let plan = schedule(0.0625, 0.125, 3);
    let job = PrintJob::new(
        "swap-job",
        &printer,
        &resin(),
        &plan,
        &[],
        JobLimits::default(),
    )
    .unwrap();
    let temp = Temp::new();
    let path = temp.0.join(format!("swap.{}", exporter.extension()));
    export_machine(
        &path,
        &job,
        &exporter,
        images(&plan, &printer),
        ExportLimits::default(),
        &mut Cancellation::never(),
    )
    .unwrap();
    let bytes = fs::read(&path).unwrap();
    let f32_at = |offset: usize| f32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
    let u32_at = |offset: usize| u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
    assert_eq!([u32_at(52), u32_at(56)], [8, 6]);
    assert_eq!([f32_at(8), f32_at(12), f32_at(16)], [8., 6., 10.]);
}

#[test]
fn goo_headers_declare_the_stored_images_mirroring() {
    use karasu_core::export::{BuiltinExporter, BuiltinFormat, PixelPolicy};
    // TD-104: the GOO header's x and y mirror bytes, after the layer count and the resolution,
    // describe the stored image against the view from above with the plate's back first. A
    // profile mirroring in X and Y (the ELEGOO Saturn 4 Ultra 16K's) stores the image ChiTuBox
    // does, and declares it as ChiTuBox does: X mirrored, Y not. Without mirroring the image is
    // Lychee's, declared as Lychee does: Y mirrored.
    let exporter = BuiltinExporter::new(BuiltinFormat::GooV3, PixelPolicy::Exact);
    let flags_at = 194 + 116 * 116 * 2 + 2 + 290 * 290 * 2 + 2 + 8;
    for ((mirror_x, mirror_y), declared) in [
        ((false, false), [0, 1]),
        ((true, true), [1, 0]),
        ((true, false), [1, 1]),
        ((false, true), [0, 0]),
    ] {
        let mut printer = printer();
        printer.mirror_x = mirror_x;
        printer.mirror_y = mirror_y;
        printer.format_ids = vec![exporter.id().into()];
        let plan = schedule(0.0625, 0.125, 3);
        let job = PrintJob::new(
            "mirror-job",
            &printer,
            &resin(),
            &plan,
            &[],
            JobLimits::default(),
        )
        .unwrap();
        let temp = Temp::new();
        let path = temp.0.join(format!("mirror.{}", exporter.extension()));
        export_machine(
            &path,
            &job,
            &exporter,
            images(&plan, &printer),
            ExportLimits::default(),
            &mut Cancellation::never(),
        )
        .unwrap();
        let bytes = fs::read(&path).unwrap();
        assert_eq!(
            bytes[flags_at..flags_at + 2],
            declared,
            "mirror x {mirror_x}, y {mirror_y}"
        );
    }
}

/// A written file's bytes as its readers see them: a ZIP's members inflated and joined in their
/// order (PP1 members are deflated, TD-133), any other file as it is.
fn readable(bytes: Vec<u8>) -> Vec<u8> {
    if !bytes.starts_with(b"PK\x03\x04") {
        return bytes;
    }
    let archive = karasu_core::archive::ArchiveReader::open(
        &bytes,
        karasu_core::archive::ArchiveLimits {
            max_entries: 1 << 16,
            max_entry_bytes: 1 << 30,
        },
    )
    .unwrap();
    let names: Vec<String> = archive.names().map(str::to_owned).collect();
    names
        .iter()
        .flat_map(|name| archive.read(name).unwrap().unwrap())
        .collect()
}

#[test]
fn pp1_bands_take_their_down_speed_from_the_retract_stage_that_covers_them() {
    use karasu_core::export::{BuiltinExporter, BuiltinFormat, PixelPolicy};
    // Photon Workshop's P1 standard settings: 4 mm at 3 mm/s, then 5 mm at 8 mm/s. The plate comes
    // back down the upper 5 mm first at 8 mm/s and the last 4 mm at 3 mm/s, so the lowest band's
    // down speed is the last retract stage's.
    let exporter = BuiltinExporter::new(BuiltinFormat::AnycubicPhotonP1, PixelPolicy::Exact);
    let stage = |distance_mm, speed_mm_s| MotionStage {
        distance_mm,
        speed_mm_s,
    };
    let mut printer = printer();
    printer.format_ids = vec![exporter.id().into()];
    printer.limits.max_lift_mm = 9.;
    let mut resin = resin();
    let normal = &mut resin.calibrations[0].settings.normal;
    normal.lift = vec![stage(4., 3.), stage(5., 8.)];
    normal.retract = vec![stage(5., 8.), stage(4., 3.)];
    let plan = schedule(0.0625, 0.125, 3);
    let job = PrintJob::new(
        "bands-job",
        &printer,
        &resin,
        &plan,
        &[],
        JobLimits::default(),
    )
    .unwrap();
    let temp = Temp::new();
    let path = temp.0.join(format!("bands.{}", exporter.extension()));
    export_machine(
        &path,
        &job,
        &exporter,
        images(&plan, &printer),
        ExportLimits::default(),
        &mut Cancellation::never(),
    )
    .unwrap();
    let bytes = readable(fs::read(&path).unwrap());
    for band in [
        r#""normal_0":{"down_speed":3.0,"height":4.0,"up_speed":3.0}"#,
        r#""normal_1":{"down_speed":8.0,"height":5.0,"up_speed":8.0}"#,
    ] {
        assert!(
            bytes.windows(band.len()).any(|w| w == band.as_bytes()),
            "{band}"
        );
    }
}

#[test]
fn pp1_files_declare_a_half_turn_and_refuse_one_axis_mirrors() {
    use karasu_core::export::{BuiltinExporter, BuiltinFormat, PixelPolicy};
    // TD-104: Photon Workshop and ChiTuBox store the P1's image turned half a turn from Lychee's
    // and declare `rotate_z` 180; karasu-slicer's profile mirroring in X and Y stores that image.
    let exporter = BuiltinExporter::new(BuiltinFormat::AnycubicPhotonP1, PixelPolicy::Exact);
    let contains =
        |bytes: &[u8], text: &str| bytes.windows(text.len()).any(|w| w == text.as_bytes());
    for ((mirror_x, mirror_y), declared) in [
        ((false, false), Some("\"rotate_z\":0.0")),
        ((true, true), Some("\"rotate_z\":180.0")),
        ((true, false), None),
        ((false, true), None),
    ] {
        let mut printer = printer();
        printer.mirror_x = mirror_x;
        printer.mirror_y = mirror_y;
        printer.format_ids = vec![exporter.id().into()];
        let plan = schedule(0.0625, 0.125, 3);
        let job = PrintJob::new(
            "turn-job",
            &printer,
            &resin(),
            &plan,
            &[],
            JobLimits::default(),
        )
        .unwrap();
        let temp = Temp::new();
        let path = temp.0.join(format!("turn.{}", exporter.extension()));
        let result = export_machine(
            &path,
            &job,
            &exporter,
            images(&plan, &printer),
            ExportLimits::default(),
            &mut Cancellation::never(),
        );
        if let Some(text) = declared {
            result.unwrap();
            let bytes = readable(fs::read(&path).unwrap());
            assert!(contains(&bytes, text), "mirror x {mirror_x}, y {mirror_y}");
        } else {
            // A mirror along one axis cannot be declared: nothing is published.
            assert!(result.is_err(), "mirror x {mirror_x}, y {mirror_y}");
            temp.assert_empty();
        }
    }
}

#[test]
fn tall_pp1_jobs_fit_the_default_metadata_budget() {
    use karasu_core::export::{BuiltinExporter, BuiltinFormat, PixelPolicy};
    // 20,000 layers: beyond the former 8 MiB budget (about 10,600 layers on the P1). The tall
    // printer only keeps the calibrated 0.125 mm layers inside the build height.
    // The source fails at once, so reaching it proves the metadata admission passed.
    let exporter = BuiltinExporter::new(BuiltinFormat::AnycubicPhotonP1, PixelPolicy::Exact);
    let mut printer = printer();
    printer.build_size_mm = [8., 6., 2600.];
    printer.format_ids = vec![exporter.id().into()];
    let plan = build_layer_schedule(
        UniformScheduleSpec::try_new(0.0625, 0.125, ScheduleEnd::Count(20_000)).unwrap(),
        ScheduleLimits {
            max_layers: 20_000,
            max_payload_bytes: 1 << 24,
        },
    )
    .unwrap();
    let job = PrintJob::new("tall", &printer, &resin(), &plan, &[], JobLimits::default()).unwrap();
    let temp = Temp::new();
    let path = temp.0.join(format!("tall.{}", exporter.extension()));
    let failing: Vec<Result<RasterImage, io::Error>> = vec![Err(io::Error::other("source"))];
    let error = export_machine(
        &path,
        &job,
        &exporter,
        failing,
        ExportLimits::default(),
        &mut Cancellation::never(),
    )
    .unwrap_err();
    assert!(matches!(error, ExportError::Source(_)), "{error}");
    assert!(!path.exists());
}

#[test]
fn builtin_capability_rejection_precedes_destination_creation() {
    use karasu_core::export::{BuiltinExporter, BuiltinFormat, PixelPolicy};
    for kind in [BuiltinFormat::CtbV2Plain, BuiltinFormat::AnycubicPhotonP1] {
        let exporter = BuiltinExporter::new(kind, PixelPolicy::Exact);
        let mut printer = printer();
        printer.format_ids = vec![exporter.id().into()];
        let mut resin = resin();
        resin.calibrations[0].settings.normal.wait_before_exposure_s = 1.;
        // CTB v2 has no separate rest semantics; PP1 lacks a verified power field.
        resin.calibrations[0].settings.normal.pwm = 128;
        let plan = schedule(0.0625, 0.125, 3);
        let job = PrintJob::new(
            "unsupported-job",
            &printer,
            &resin,
            &plan,
            &[],
            JobLimits::default(),
        )
        .unwrap();
        let temp = Temp::new();
        let path = temp.0.join(format!("job.{}", exporter.extension()));
        assert!(export_machine(
            &path,
            &job,
            &exporter,
            images(&plan, &printer),
            ExportLimits::default(),
            &mut Cancellation::never()
        )
        .is_err());
        temp.assert_empty();
    }
}

#[test]
fn native_grayscale_loss_requires_explicit_opt_in_in_each_reduced_dialect() {
    use karasu_core::export::{BuiltinExporter, BuiltinFormat, PixelPolicy};
    for kind in [
        BuiltinFormat::CtbV2Plain,
        BuiltinFormat::AnycubicPhotonP1,
        BuiltinFormat::PhotonV1FirmwareMotion,
    ] {
        let exact = BuiltinExporter::new(kind, PixelPolicy::Exact).with_antialiasing(4);
        let rounded = BuiltinExporter::new(kind, PixelPolicy::RoundToFormat).with_antialiasing(4);
        let mut printer = printer();
        printer.resolution_px = [7, 5];
        printer.format_ids = vec![exact.id().into()];
        let plan = schedule(0.0625, 0.125, 3);
        let job = PrintJob::new(
            "gray-job",
            &printer,
            &resin(),
            &plan,
            &[],
            JobLimits::default(),
        )
        .unwrap();
        let config = RasterConfig {
            samples_per_axis: 4,
            grayscale_levels: 256,
            minimum_gray: 0,
            outside: OutsidePolicy::Reject,
            compensation: Compensation::None,
            kept: karasu_core::raster::KeptSampling::Antialiased,
        };
        let source = images_with_config(&plan, &printer, config);
        assert!(source[0]
            .as_ref()
            .unwrap()
            .to_pixels()
            .unwrap()
            .iter()
            .any(|v| ![0, 255].contains(v)));
        let temp = Temp::new();
        let path = temp.0.join(format!("job.{}", exact.extension()));
        assert!(export_machine(
            &path,
            &job,
            &exact,
            source,
            ExportLimits::default(),
            &mut Cancellation::never()
        )
        .is_err());
        temp.assert_empty();
        export_machine(
            &path,
            &job,
            &rounded,
            images_with_config(&plan, &printer, config),
            ExportLimits::default(),
            &mut Cancellation::never(),
        )
        .unwrap();
        assert!(path.is_file());
        assert_eq!(fs::read_dir(&temp.0).unwrap().count(), 1);
    }
}

#[test]
fn antialiasing_is_declared_in_the_headers_and_checked_on_every_mask() {
    use karasu_core::export::{BuiltinExporter, BuiltinFormat, PixelPolicy};
    let config = |samples_per_axis| RasterConfig {
        samples_per_axis,
        grayscale_levels: 256,
        minimum_gray: 0,
        outside: OutsidePolicy::Reject,
        compensation: Compensation::None,
        kept: karasu_core::raster::KeptSampling::Antialiased,
    };
    let plan = schedule(0.0625, 0.125, 3);
    let export = |exporter: &BuiltinExporter, samples: u8| {
        let mut printer = printer();
        printer.format_ids = vec![exporter.id().into()];
        let job = PrintJob::new(
            "aa-job",
            &printer,
            &resin(),
            &plan,
            &[],
            JobLimits::default(),
        )
        .unwrap();
        let temp = Temp::new();
        let path = temp.0.join(format!("aa.{}", exporter.extension()));
        let result = export_machine(
            &path,
            &job,
            exporter,
            images_with_config(&plan, &printer, config(samples)),
            ExportLimits::default(),
            &mut Cancellation::never(),
        );
        let bytes = result.as_ref().ok().map(|_| fs::read(&path).unwrap());
        if bytes.is_none() {
            temp.assert_empty();
        }
        (result, bytes)
    };
    // GOO: big-endian u16 supersampling factor after the six identity strings (offset 188).
    for (samples, factor) in [(1, 1u16), (4, 4)] {
        let goo = BuiltinExporter::new(BuiltinFormat::GooV3, PixelPolicy::Exact)
            .with_antialiasing(samples);
        let (result, bytes) = export(&goo, samples);
        result.unwrap();
        let bytes = bytes.unwrap();
        assert_eq!(u16::from_be_bytes([bytes[188], bytes[189]]), factor);
        assert_eq!(&bytes[190..194], &[0, 0, 0, 0]);
    }
    // CTB: the slicer-information antialiasing flag (0x07 binary, 0x0F supersampled).
    for (samples, flag) in [(1, 0x07u32), (2, 0x0f)] {
        let ctb = BuiltinExporter::new(BuiltinFormat::CtbV2Plain, PixelPolicy::RoundToFormat)
            .with_antialiasing(samples);
        let (result, bytes) = export(&ctb, samples);
        result.unwrap();
        let bytes = bytes.unwrap();
        let slicer = u32::from_le_bytes(bytes[0x68..0x6c].try_into().unwrap()) as usize;
        assert_eq!(
            u32::from_le_bytes(bytes[slicer + 36..slicer + 40].try_into().unwrap()),
            flag
        );
    }
    // PP1: the resin's slice parameters record the level (members read inflated).
    for samples in [1, 4] {
        let pp1 = BuiltinExporter::new(BuiltinFormat::AnycubicPhotonP1, PixelPolicy::RoundToFormat)
            .with_antialiasing(samples);
        let (result, bytes) = export(&pp1, samples);
        result.unwrap();
        let text = String::from_utf8_lossy(&readable(bytes.unwrap())).into_owned();
        assert!(
            text.contains(&format!("\"anti_count\":{samples},")),
            "{samples}"
        );
    }
    // A mask whose supersampling differs from the declaration fails without publication.
    let declared =
        BuiltinExporter::new(BuiltinFormat::GooV3, PixelPolicy::Exact).with_antialiasing(4);
    let (result, _) = export(&declared, 1);
    assert!(
        matches!(&result, Err(ExportError::Invalid(m)) if m.contains("samples per axis")),
        "{result:?}"
    );
    // Unsupported levels are refused before any output.
    let odd = BuiltinExporter::new(BuiltinFormat::GooV3, PixelPolicy::Exact).with_antialiasing(3);
    assert!(export(&odd, 1).0.is_err());
}

#[test]
fn parallel_encoding_publishes_identical_files_for_every_builtin_format() {
    use karasu_core::export::{
        export_machine_parallel, BuiltinExporter, BuiltinFormat, ParallelEncoding, PixelPolicy,
    };
    fn never(_: Checkpoint) -> bool {
        false
    }
    let plan = schedule(0.0625, 0.125, 3);
    for kind in [
        BuiltinFormat::CtbV2Plain,
        BuiltinFormat::GooV3,
        BuiltinFormat::AnycubicPhotonP1,
        BuiltinFormat::PhotonV1FirmwareMotion,
    ] {
        for policy in [PixelPolicy::Exact, PixelPolicy::RoundToFormat] {
            let exporter = BuiltinExporter::new(kind, policy);
            let mut printer = printer();
            printer.format_ids = vec![exporter.id().into()];
            let job =
                PrintJob::new("p", &printer, &resin(), &plan, &[], JobLimits::default()).unwrap();
            let temp = Temp::new();
            let reference = temp.0.join(format!("seq.{}", exporter.extension()));
            let expected = export_machine(
                &reference,
                &job,
                &exporter,
                images(&plan, &printer),
                ExportLimits::default(),
                &mut Cancellation::never(),
            );
            let Ok(expected) = expected else {
                // A policy the format rejects must be rejected identically in parallel.
                let path = temp.0.join(format!("par.{}", exporter.extension()));
                let parallel = export_machine_parallel(
                    &path,
                    &job,
                    &exporter,
                    images(&plan, &printer),
                    ExportLimits::default(),
                    ParallelEncoding::default(),
                    &never,
                    &mut Cancellation::never(),
                );
                assert!(parallel.is_err() && !path.exists(), "{kind:?} {policy:?}");
                continue;
            };
            let bytes = fs::read(&reference).unwrap();
            for (index, encoding) in [
                ParallelEncoding {
                    threads: 1,
                    window_bytes: 1,
                },
                ParallelEncoding {
                    threads: 2,
                    window_bytes: 1 << 20,
                },
                ParallelEncoding {
                    threads: 8,
                    window_bytes: 1 << 30,
                },
            ]
            .into_iter()
            .enumerate()
            {
                let path = temp.0.join(format!("par{index}.{}", exporter.extension()));
                let summary = export_machine_parallel(
                    &path,
                    &job,
                    &exporter,
                    images(&plan, &printer),
                    ExportLimits::default(),
                    encoding,
                    &never,
                    &mut Cancellation::never(),
                )
                .unwrap();
                assert_eq!(
                    fs::read(&path).unwrap(),
                    bytes,
                    "{kind:?} {policy:?} {encoding:?}"
                );
                assert_eq!(summary.output_bytes, expected.output_bytes);
                assert_eq!(
                    summary.coverage_volume_mm3.to_bits(),
                    expected.coverage_volume_mm3.to_bits()
                );
            }
        }
    }
}

#[test]
fn parallel_encoding_reports_the_first_failure_in_layer_order() {
    use karasu_core::export::{
        export_machine_parallel, BuiltinExporter, BuiltinFormat, ParallelEncoding, PixelPolicy,
    };
    fn never(_: Checkpoint) -> bool {
        false
    }
    let exporter = BuiltinExporter::new(BuiltinFormat::GooV3, PixelPolicy::Exact);
    let mut printer = printer();
    printer.format_ids = vec![exporter.id().into()];
    let plan = schedule(0.0625, 0.125, 3);
    let job = PrintJob::new("p", &printer, &resin(), &plan, &[], JobLimits::default()).unwrap();
    let temp = Temp::new();
    let run = |source: Vec<Result<RasterImage, io::Error>>, name: &str| {
        let path = temp.0.join(format!("{name}.{}", exporter.extension()));
        let result = export_machine_parallel(
            &path,
            &job,
            &exporter,
            source,
            ExportLimits::default(),
            ParallelEncoding {
                threads: 4,
                window_bytes: 1 << 20,
            },
            &never,
            &mut Cancellation::never(),
        );
        assert!(!path.exists(), "{name}: nothing is published on failure");
        result.unwrap_err()
    };
    // A source failure at layer 1 is reported, after layer 0, as the source failure.
    let mut source = images(&plan, &printer);
    source[1] = Err(io::Error::other("layer 1"));
    assert!(matches!(run(source, "source"), ExportError::Source(_)));
    // Too few and too many layers.
    let mut short = images(&plan, &printer);
    short.pop();
    assert!(matches!(run(short, "short"), ExportError::Invalid(_)));
    let mut long = images(&plan, &printer);
    long.push(images(&plan, &printer).pop().unwrap());
    assert!(matches!(run(long, "long"), ExportError::Invalid(_)));
    // Cancellation from the encoding workers' predicate publishes nothing.
    let path = temp.0.join(format!("cancel.{}", exporter.extension()));
    let stop = |c: Checkpoint| c == Checkpoint::ExportLayer;
    let result = export_machine_parallel(
        &path,
        &job,
        &exporter,
        images(&plan, &printer),
        ExportLimits::default(),
        ParallelEncoding::default(),
        &stop,
        &mut Cancellation::never(),
    );
    assert!(
        matches!(result, Err(ExportError::Cancelled(_))),
        "{result:?}"
    );
    assert!(!path.exists());
}
