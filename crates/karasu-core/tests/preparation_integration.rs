//! Parametric manual-support regressions with independently integrated layer sections.
#![allow(clippy::cast_precision_loss)] // Independent test-only rational decoding for area checks.
use karasu_core::{
    cancellation::{Cancellation, Checkpoint},
    geometry::{ObjectTransform, Point3, Rotation3, Vector3},
    mesh::TriangleMesh,
    preparation::{
        supports::*, HoleSpec, HollowSpec, Operation, OperationKind, PlanLimits, PreparationPlan,
        SamplingSpec, SourceBinding,
    },
    profiles::{
        LayerSettings, MotionStage, PrintSettings, PrinterCalibration, PrinterLimits,
        PrinterProfile, ResinProfile,
    },
    slicing::{
        construct::{ConstructLimits, ConstructedMaterial, ExactPoint},
        engine::{LayerConfig, LayerStageLimits},
        grid::BinaryGridSpec,
        regions::RegionFillRule,
        schedule::*,
    },
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
                    entry_mm: [5., 5., 9.],
                    direction: [0., 0., 1.],
                    length_mm: 4.,
                    diameter_mm: 2.,
                }),
            },
        ],
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
fn make_plan(
    model: PreparationPlan,
    recipes: Vec<SupportRecipe>,
    policy: SupportIntersectionPolicy,
) -> Result<SupportPlan, SupportError> {
    SupportPlan::new(
        model,
        recipes,
        policy,
        SupportLimits::default(),
        &mut Cancellation::never(),
    )
}
fn run_recipe(recipe: SupportRecipe) -> Result<SupportedModel, SupportError> {
    let p = make_plan(
        model_plan(),
        vec![recipe],
        SupportIntersectionPolicy::RejectUncertified,
    )?;
    generate_supports(
        &cube(20.),
        &p,
        SupportLimits::default(),
        &mut Cancellation::never(),
    )
}
fn run() -> SupportedModel {
    run_recipe(recipe(&model_plan())).unwrap()
}
fn with_section<T>(model: &SupportedModel, z: f64, f: impl FnOnce(&ConstructedMaterial) -> T) -> T {
    let schedule = build_layer_schedule(
        UniformScheduleSpec::try_new(z, 1., ScheduleEnd::Count(1)).unwrap(),
        ScheduleLimits {
            max_layers: 1,
            max_payload_bytes: 1024,
        },
    )
    .unwrap();
    let config = LayerConfig {
        grid: BinaryGridSpec::try_new([0., 0.], -16).unwrap(),
        fill_rule: RegionFillRule::EvenOddStrict,
        stages: LayerStageLimits::default(),
        max_output_payload_bytes: 64 * 1024 * 1024,
    };
    let mut engine = model.layer_engine(
        &schedule,
        config,
        ConstructLimits::default(),
        Cancellation::never(),
        None,
    );
    let layer = engine.next().unwrap().unwrap();
    assert!(engine.next().is_none());
    f(layer.material_union().unwrap())
}
fn magnitude(words: [u64; 8]) -> f64 {
    words
        .iter()
        .rev()
        .fold(0., |n, &word| n * 18_446_744_073_709_551_616. + word as f64)
}
fn xy(p: ExactPoint, exp: i16) -> [f64; 2] {
    let den = magnitude(p.denominator());
    [p.numerator_x(), p.numerator_y()].map(|(negative, words)| {
        let x = magnitude(words) / den * 2_f64.powi(i32::from(exp));
        if negative {
            -x
        } else {
            x
        }
    })
}
fn area(m: &ConstructedMaterial) -> f64 {
    let exp = m.dyadic_exponent_mm().unwrap();
    m.edges()
        .iter()
        .map(|e| {
            let a = xy(m.vertices()[e.endpoints[0]], exp);
            let b = xy(m.vertices()[e.endpoints[1]], exp);
            (a[0] * b[1] - a[1] * b[0]) * 0.5
        })
        .sum()
}
fn contains(m: &ConstructedMaterial, q: [f64; 2]) -> bool {
    let exp = m.dyadic_exponent_mm().unwrap();
    let mut result = false;
    for edge in m.edges() {
        let a = xy(m.vertices()[edge.endpoints[0]], exp);
        let b = xy(m.vertices()[edge.endpoints[1]], exp);
        if (a[1] > q[1]) != (b[1] > q[1])
            && q[0] < (b[0] - a[0]) * (q[1] - a[1]) / (b[1] - a[1]) + a[0]
        {
            result = !result;
        }
    }
    result
}

#[test]
fn combined_boundary_sections_preserve_support_union_drain_and_cavity() {
    let m = run();
    for (z, expected) in [
        (0., 16.),
        (0.5, 16.),
        (1., 16.),
        (5., 6.25),
        (9., 1.),
        (12., 144.),
        (12.5, 144.),
        (13., 144.),
        (14., 144.),
    ] {
        with_section(&m, z, |section| {
            assert!((area(section) - expected).abs() < 0.001, "z={z}");
        });
    }
    for z in [10., 11.] {
        with_section(&m, z, |section| {
            assert!(!contains(section, [5., 5.]));
            assert!(contains(section, [12., 8.]));
            assert!(contains(section, [1., 1.]));
            assert!((area(section) - (400. - std::f64::consts::PI)).abs() < 0.02);
        });
    }
    assert_eq!(m.drained_model().unwrap().enclosed_cavities(), 0);
}
#[test]
fn combined_repeated_generation_preserves_inputs_and_exact_output() {
    let source = cube(20.);
    let vertices = source.vertices().to_vec();
    let triangles = source.triangles().to_vec();
    let model = model_plan();
    let p = make_plan(
        model.clone(),
        vec![recipe(&model)],
        SupportIntersectionPolicy::RejectUncertified,
    )
    .unwrap();
    let record = p.canonical_record().to_vec();
    let a = generate_supports(
        &source,
        &p,
        SupportLimits::default(),
        &mut Cancellation::never(),
    )
    .unwrap();
    let b = generate_supports(
        &source,
        &p,
        SupportLimits::default(),
        &mut Cancellation::never(),
    )
    .unwrap();
    assert_eq!(source.vertices(), vertices);
    assert_eq!(source.triangles(), triangles);
    assert_eq!(p.canonical_record(), record);
    assert_eq!(a.plan_record(), b.plan_record());
    for (a, b) in a.components().iter().zip(b.components()) {
        assert_eq!(a.mesh().vertices(), b.mesh().vertices());
        assert_eq!(a.mesh().triangles(), b.mesh().triangles());
    }
    for z in [0.5, 10., 12., 13.] {
        let first = with_section(&a, z, area);
        let second = with_section(&b, z, area);
        assert_eq!(first.to_bits(), second.to_bits());
    }
}
#[test]
fn combined_cut_edit_invalidates_contact_until_explicit_rebind() {
    let old = model_plan();
    let stale = recipe(&old);
    let mut operations = old.operations().to_vec();
    if let OperationKind::Hole(h) = &mut operations[1].kind {
        h.entry_mm[0] = 6.;
    }
    let changed = PreparationPlan::new(
        old.source(),
        old.sampling(),
        operations,
        PlanLimits::default(),
        &mut Cancellation::never(),
    )
    .unwrap();
    assert!(make_plan(
        changed.clone(),
        vec![stale.clone()],
        SupportIntersectionPolicy::RejectUncertified
    )
    .is_err());
    let mut rebound = stale;
    rebound.surface = changed.model_key();
    let plan = make_plan(
        changed,
        vec![rebound],
        SupportIntersectionPolicy::RejectUncertified,
    )
    .unwrap();
    let generated = generate_supports(
        &cube(20.),
        &plan,
        SupportLimits::default(),
        &mut Cancellation::never(),
    )
    .unwrap();
    with_section(&generated, 11., |section| {
        assert!(!contains(section, [6., 5.]));
        assert!(contains(section, [4.5, 5.]));
    });
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
                layer_height_mm: 0.5,
                bottom_layer_count: 1,
                transition_layer_count: 1,
                bottom: layer(10.),
                normal: layer(2.),
            },
        }],
    };
    (printer, resin)
}

#[test]
fn combined_export_cancellation_removes_staging_at_every_publication_phase() {
    use karasu_core::{
        export::{
            export_machine, export_reference, BuiltinExporter, BuiltinFormat, ExportError,
            ExportLimits, PixelPolicy,
        },
        print_job::{JobLimits, PrintJob},
        raster::{stream::RasterStream, Compensation, OutsidePolicy, RasterConfig, RasterLimits},
    };
    let supported = run();
    let (mut printer, mut resin) = profiles();
    printer.limits.max_layer_height_mm = 11.;
    resin.calibrations[0].settings.layer_height_mm = 11.;
    let schedule = build_layer_schedule(
        UniformScheduleSpec::try_new(11., 11., ScheduleEnd::Count(2)).unwrap(),
        ScheduleLimits {
            max_layers: 2,
            max_payload_bytes: 1024,
        },
    )
    .unwrap();
    // Synthetic wide slabs reach model material while keeping cancellation runs small.
    let job = PrintJob::new(
        "combined-cancellation",
        &printer,
        &resin,
        &schedule,
        &[],
        JobLimits::default(),
    )
    .unwrap();
    let config = export_layer_config();
    let root = std::env::temp_dir().join(format!("karasu-combined-cancel-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    for format in [
        None,
        Some(BuiltinFormat::CtbV2Plain),
        Some(BuiltinFormat::GooV3),
        Some(BuiltinFormat::PhotonV1FirmwareMotion),
        Some(BuiltinFormat::AnycubicPhotonP1),
    ] {
        for checkpoint in [
            Checkpoint::ExportStart,
            Checkpoint::ExportLayer,
            Checkpoint::ExportFinish,
            Checkpoint::ExportPublish,
        ] {
            let layers = supported.layer_engine(
                &schedule,
                config,
                ConstructLimits::default(),
                Cancellation::never(),
                None,
            );
            let masks = RasterStream::new(
                layers,
                printer.lcd_profile().unwrap(),
                RasterConfig {
                    samples_per_axis: 1,
                    grayscale_levels: 2,
                    minimum_gray: 0,
                    outside: OutsidePolicy::Reject,
                    compensation: Compensation::None,
                    kept: karasu_core::raster::KeptSampling::Antialiased,
                },
                RasterLimits::default(),
                Cancellation::never(),
            );
            let mut predicate = |p| p == checkpoint;
            let result = if let Some(format) = format {
                export_machine(
                    &root.join(match format {
                        BuiltinFormat::CtbV2Plain => "machine.ctb",
                        BuiltinFormat::GooV3 => "machine.goo",
                        BuiltinFormat::PhotonV1FirmwareMotion => "machine.photon",
                        BuiltinFormat::AnycubicPhotonP1 => "machine.pp1",
                        _ => unreachable!("this test exports the four original formats"),
                    }),
                    &job,
                    &BuiltinExporter::new(format, PixelPolicy::Exact),
                    masks,
                    ExportLimits::default(),
                    &mut Cancellation::new(&mut predicate),
                )
            } else {
                export_reference(
                    &root.join("reference"),
                    &job,
                    masks,
                    ExportLimits::default(),
                    &mut Cancellation::new(&mut predicate),
                )
            };
            assert!(
                matches!(result, Err(ExportError::Cancelled(_))),
                "{checkpoint:?}: {result:?}"
            );
            assert_eq!(std::fs::read_dir(&root).unwrap().count(), 0);
        }
    }
    std::fs::remove_dir(&root).unwrap();
}

fn export_layer_config() -> LayerConfig {
    LayerConfig {
        grid: BinaryGridSpec::try_new([0., 0.], -16).unwrap(),
        fill_rule: RegionFillRule::EvenOddStrict,
        stages: LayerStageLimits::default(),
        max_output_payload_bytes: 64 * 1024 * 1024,
    }
}
