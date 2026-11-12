//! Job scheduling, explicit physical settings and immutable profile snapshot regressions.
use karasu_core::{
    cancellation::{Cancellation, Checkpoint},
    print_job::{JobError, JobLimits, LayerOverride, LayerPhase, PrintJob},
    profiles::{
        LayerSettings, MotionStage, PrintSettings, PrinterCalibration, PrinterLimits,
        PrinterProfile, ResinProfile,
    },
    slicing::schedule::{
        build_layer_schedule, build_slab_schedule, LayerSchedule, ScheduleEnd, ScheduleLimits,
        UniformScheduleSpec,
    },
};

fn printer() -> PrinterProfile {
    PrinterProfile {
        schema_version: 1,
        id: "job-printer".into(),
        name: "Job test printer".into(),
        revision: 1,
        build_size_mm: [10., 10., 20.],
        resolution_px: [100, 100],
        origin_xy_mm: [0., 0.],
        swap_xy: false,
        mirror_x: false,
        mirror_y: false,
        format_ids: vec!["test-v1".into()],
        limits: PrinterLimits {
            min_layer_height_mm: 0.01,
            max_layer_height_mm: 1.,
            max_exposure_s: 100.,
            max_speed_mm_s: 20.,
            max_lift_mm: 10.,
        },
        vendor: None,
        model: None,
        based_on: None,
        fixed_layer_time_s: None,
    }
}
fn layer(exposure_s: f64, speed_mm_s: f64) -> LayerSettings {
    LayerSettings {
        exposure_s,
        wait_before_exposure_s: 0.2,
        wait_after_exposure_s: 0.1,
        wait_after_lift_s: 0.3,
        pwm: 255,
        lift: vec![MotionStage {
            distance_mm: 5.,
            speed_mm_s,
        }],
        retract: vec![MotionStage {
            distance_mm: 5.,
            speed_mm_s: 5.,
        }],
    }
}
fn resin() -> ResinProfile {
    ResinProfile {
        schema_version: 1,
        id: "job-resin".into(),
        name: "Job test resin".into(),
        revision: 1,
        density_g_per_ml: None,
        cost_per_liter: None,
        currency: None,
        calibrations: vec![PrinterCalibration {
            printer_id: "job-printer".into(),
            settings: PrintSettings {
                heating_temperature_c: None,
                layer_height_mm: 0.1,
                bottom_layer_count: 2,
                transition_layer_count: 3,
                bottom: LayerSettings {
                    pwm: 220,
                    ..layer(22., 1.)
                },
                normal: layer(2., 2.),
            },
        }],
    }
}
fn schedule(first: f64, height: f64, count: u64) -> LayerSchedule {
    build_layer_schedule(
        UniformScheduleSpec::try_new(first, height, ScheduleEnd::Count(count)).unwrap(),
        ScheduleLimits {
            max_layers: 1000,
            max_payload_bytes: 100_000,
        },
    )
    .unwrap()
}
fn job() -> PrintJob {
    PrintJob::new(
        "job-a",
        &printer(),
        &resin(),
        &schedule(0.05, 0.1, 7),
        &[],
        JobLimits::default(),
    )
    .unwrap()
}

#[test]
fn sample_planes_are_distinct_from_print_positions() {
    let j = job();
    let first = j.layer(0).unwrap();
    assert_eq!(first.sample_z_mm, 0.05);
    assert_eq!(first.position_z_mm, 0.1);
    assert_eq!(first.thickness_mm, 0.1);
    let last = j.layer(6).unwrap();
    assert_eq!(last.position_z_mm, 0.700_000_000_000_000_1);
    assert_eq!(last.sample_z_mm, j.sample_planes()[6].z_mm);
    assert!(j.layer(7).is_err());
}
#[test]
fn transition_interpolates_exposure_only_and_reaches_normal_after_block() {
    let j = job();
    assert_eq!(j.layer(0).unwrap().phase, LayerPhase::Bottom);
    assert_eq!(j.layer(1).unwrap().settings.exposure_s, 22.);
    for (index, expected) in [(2, 17.), (3, 12.), (4, 7.)] {
        let transition = j.layer(index).unwrap();
        assert_eq!(transition.phase, LayerPhase::Transition);
        assert_eq!(transition.settings.exposure_s, expected);
        assert_eq!(
            transition.settings.lift,
            resin().calibrations[0].settings.normal.lift
        );
        assert_eq!(transition.settings.pwm, 255);
        assert_eq!(transition.settings.wait_after_lift_s, 0.3);
    }
    assert_eq!(j.layer(5).unwrap().phase, LayerPhase::Normal);
    assert_eq!(j.layer(5).unwrap().settings.exposure_s, 2.);
}
#[test]
fn ascending_exposure_transitions_are_explicitly_supported() {
    let mut r = resin();
    r.calibrations[0].settings.bottom.exposure_s = 2.;
    r.calibrations[0].settings.normal.exposure_s = 22.;
    let j = PrintJob::new(
        "job-a",
        &printer(),
        &r,
        &schedule(0.05, 0.1, 7),
        &[],
        JobLimits::default(),
    )
    .unwrap();
    assert_eq!(j.layer(2).unwrap().settings.exposure_s, 7.);
    assert_eq!(j.layer(4).unwrap().settings.exposure_s, 17.);
}
#[test]
fn overrides_are_complete_sorted_and_do_not_mutate_calibration() {
    let mut special = layer(9., 3.);
    special.pwm = 128;
    let values = vec![
        LayerOverride {
            index: 5,
            settings: special.clone(),
        },
        LayerOverride {
            index: 0,
            settings: layer(40., 1.),
        },
    ];
    let j = PrintJob::new(
        "job-a",
        &printer(),
        &resin(),
        &schedule(0.05, 0.1, 7),
        &values,
        JobLimits::default(),
    )
    .unwrap();
    assert_eq!(
        j.overrides().iter().map(|o| o.index).collect::<Vec<_>>(),
        vec![0, 5]
    );
    assert_eq!(j.layer(5).unwrap().settings, special);
    assert_eq!(j.layer(5).unwrap().phase, LayerPhase::Override);
    assert_eq!(j.settings().normal.exposure_s, 2.);
    assert_eq!(j.layer(6).unwrap().settings.exposure_s, 2.);
}
#[test]
fn duplicate_out_of_range_and_invalid_overrides_fail_before_job_publication() {
    for values in [
        vec![
            LayerOverride {
                index: 0,
                settings: layer(4., 1.),
            },
            LayerOverride {
                index: 0,
                settings: layer(5., 1.),
            },
        ],
        vec![LayerOverride {
            index: 7,
            settings: layer(4., 1.),
        }],
        vec![LayerOverride {
            index: 1,
            settings: layer(101., 1.),
        }],
    ] {
        assert!(PrintJob::new(
            "job-a",
            &printer(),
            &resin(),
            &schedule(0.05, 0.1, 7),
            &values,
            JobLimits::default()
        )
        .is_err());
    }
}
#[test]
fn count_metadata_calibration_and_thickness_mismatches_fail() {
    let p = printer();
    let r = resin();
    for (s, limits) in [
        (schedule(0.05, 0.1, 0), JobLimits::default()),
        (schedule(0.05, 0.1, 4), JobLimits::default()),
        (schedule(0.05, 0.2, 7), JobLimits::default()),
        (
            schedule(0.05, 0.1, 7),
            JobLimits {
                max_layers: 6,
                ..JobLimits::default()
            },
        ),
        (
            schedule(0.05, 0.1, 7),
            JobLimits {
                max_metadata_bytes: 0,
                ..JobLimits::default()
            },
        ),
    ] {
        assert!(PrintJob::new("job-a", &p, &r, &s, &[], limits).is_err());
    }
    let mut other = p;
    other.id = "other-printer".into();
    assert!(PrintJob::new(
        "job-a",
        &other,
        &r,
        &schedule(0.05, 0.1, 7),
        &[],
        JobLimits::default()
    )
    .is_err());
}
#[test]
fn sample_outside_printed_slab_is_not_silently_repositioned() {
    for first in [-0.01, 0.11, 5.] {
        assert!(PrintJob::new(
            "job-a",
            &printer(),
            &resin(),
            &schedule(first, 0.1, 7),
            &[],
            JobLimits::default()
        )
        .is_err());
    }
    for first in [0., 0.1] {
        PrintJob::new(
            "job-a",
            &printer(),
            &resin(),
            &schedule(first, 0.1, 7),
            &[],
            JobLimits::default(),
        )
        .unwrap();
    }
}
#[test]
fn travel_must_fit_within_printer_z_even_when_layer_position_fits() {
    let mut p = printer();
    p.build_size_mm[2] = 5.6;
    p.limits.max_lift_mm = 5.5;
    assert!(PrintJob::new(
        "job-a",
        &p,
        &resin(),
        &schedule(0.05, 0.1, 7),
        &[],
        JobLimits::default()
    )
    .is_err());
    p.build_size_mm[2] = 5.7;
    PrintJob::new(
        "job-a",
        &p,
        &resin(),
        &schedule(0.05, 0.1, 7),
        &[],
        JobLimits::default(),
    )
    .unwrap();
}
#[test]
fn jobs_retain_immutable_profile_settings_and_override_snapshots() {
    let mut p = printer();
    let mut r = resin();
    let mut overrides = vec![LayerOverride {
        index: 6,
        settings: layer(8., 1.),
    }];
    let j = PrintJob::new(
        "job-a",
        &p,
        &r,
        &schedule(0.05, 0.1, 7),
        &overrides,
        JobLimits::default(),
    )
    .unwrap();
    p.name = "Edited after job creation".into();
    r.calibrations[0].settings.normal.exposure_s = 77.;
    overrides[0].settings.exposure_s = 66.;
    assert_eq!(j.printer().name, "Job test printer");
    assert_eq!(
        j.resin()
            .calibration_for("job-printer")
            .unwrap()
            .normal
            .exposure_s,
        2.
    );
    assert_eq!(j.layer(6).unwrap().settings.exposure_s, 8.);
}
#[test]
fn time_estimate_includes_exposure_waits_and_each_motion_stage() {
    let j = job();
    let expected: f64 = (0..j.layer_count())
        .map(|i| {
            let s = j.layer(i).unwrap().settings;
            s.exposure_s
                + 0.6
                + s.lift
                    .iter()
                    .chain(&s.retract)
                    .map(|m| m.distance_mm / m.speed_mm_s)
                    .sum::<f64>()
        })
        .sum();
    assert!((j.estimated_time_s() - expected).abs() < 1e-12);
}
#[test]
fn a_printers_fixed_time_is_added_to_every_layer_and_bounded() {
    // TD-122: the P1 catalog's 0.45 s, on each of the seven layers.
    let mut printer = printer();
    printer.fixed_layer_time_s = Some(0.45);
    let fixed = PrintJob::new(
        "job-a",
        &printer,
        &resin(),
        &schedule(0.05, 0.1, 7),
        &[],
        JobLimits::default(),
    )
    .unwrap();
    let added = fixed.estimated_time_s() - job().estimated_time_s();
    assert!((added - 7.0 * 0.45).abs() < 1e-9, "{added}");
    for seconds in [-0.1, 10.5, f64::NAN] {
        printer.fixed_layer_time_s = Some(seconds);
        assert!(printer.validate().is_err(), "{seconds}");
    }
}
#[test]
fn cancellation_is_reported_at_start_layer_and_finish_without_partial_success() {
    for checkpoint in [
        Checkpoint::ExportStart,
        Checkpoint::ExportLayer,
        Checkpoint::ExportFinish,
    ] {
        let mut predicate = |p| p == checkpoint;
        let mut cancel = Cancellation::new(&mut predicate);
        let result = PrintJob::new_with_cancellation(
            "job-a",
            &printer(),
            &resin(),
            &schedule(0.05, 0.1, 7),
            &[],
            JobLimits::default(),
            &mut cancel,
        );
        assert!(matches!(result, Err(JobError::Cancelled(_))));
    }
}

#[test]
fn variable_slabs_need_calibrated_bottom_layers_and_explicit_settings() {
    // Five calibrated bottom/transition layers, then 0.05 and 0.2 mm layers (TD-064).
    let heights = [100_000, 100_000, 100_000, 100_000, 100_000, 50_000, 200_000];
    let slabs = |heights: &[u64]| {
        build_slab_schedule(
            heights,
            ScheduleLimits {
                max_layers: 1000,
                max_payload_bytes: 100_000,
            },
            &mut Cancellation::never(),
        )
        .unwrap()
    };
    let explicit = |index, exposure_s| LayerOverride {
        index,
        settings: layer(exposure_s, 2.),
    };
    let overrides = [explicit(5, 1.5), explicit(6, 4.)];
    let make = |heights: &[u64], overrides: &[LayerOverride], printer: &PrinterProfile| {
        PrintJob::new(
            "job-v",
            printer,
            &resin(),
            &slabs(heights),
            overrides,
            JobLimits::default(),
        )
    };
    let j = make(&heights, &overrides, &printer()).unwrap();
    let thin = j.layer(5).unwrap();
    assert_eq!((thin.position_z_mm, thin.thickness_mm), (0.55, 0.05));
    assert_eq!(thin.sample_z_mm, 0.525);
    assert_eq!(thin.phase, LayerPhase::Override);
    let thick = j.layer(6).unwrap();
    assert_eq!((thick.position_z_mm, thick.thickness_mm), (0.75, 0.2));
    assert_eq!(thick.settings.exposure_s, 4.);
    // An uncalibrated layer without explicit settings has no exposure to use.
    let error = make(&heights, &overrides[..1], &printer()).unwrap_err();
    assert!(
        error.to_string().contains("no explicit settings"),
        "{error}"
    );
    // Bottom and transition layers keep the calibrated height.
    let mut early = heights;
    early[4] = 50_000;
    let error = make(&early, &overrides, &printer()).unwrap_err();
    assert!(
        error.to_string().contains("bottom or transition"),
        "{error}"
    );
    // Heights stay within the printer's limits.
    let mut narrow = printer();
    narrow.limits.max_layer_height_mm = 0.15;
    let error = make(&heights, &overrides, &narrow).unwrap_err();
    assert!(
        error.to_string().contains("outside the printer's"),
        "{error}"
    );
}
