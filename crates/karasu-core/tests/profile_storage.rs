//! Profile schema, physical settings, directory separation and replacement regressions.
use karasu_core::profiles::{
    catalog::differences,
    store::{
        ProfileDocument, ProfileKind, ProfilePaths, ProfilePlatform, ProfileStore, SavePolicy,
    },
    validate_profile_id, CatalogRef, ImageSettings, LayerSettings, MotionStage,
    PrintSettingProfile, PrintSettings, PrinterCalibration, PrinterLimits, PrinterProfile,
    ResinKind, ResinMaterial, ResinProfile, SettingStatus, LIBRARY_SCHEMA_VERSION,
    MAX_PROFILE_BYTES,
};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

fn printer(id: &str) -> PrinterProfile {
    PrinterProfile {
        schema_version: 1,
        id: id.into(),
        name: "Independent test printer".into(),
        revision: 1,
        build_size_mm: [100., 60., 150.],
        resolution_px: [1000, 600],
        origin_xy_mm: [-50., -30.],
        swap_xy: false,
        mirror_x: false,
        mirror_y: false,
        format_ids: vec!["test-format-v1".into()],
        limits: PrinterLimits {
            min_layer_height_mm: 0.01,
            max_layer_height_mm: 0.2,
            max_exposure_s: 120.,
            max_speed_mm_s: 20.,
            max_lift_mm: 15.,
        },
        vendor: None,
        model: None,
        based_on: None,
        fixed_layer_time_s: None,
    }
}
fn layer() -> LayerSettings {
    LayerSettings {
        exposure_s: 2.,
        wait_before_exposure_s: 0.5,
        wait_after_exposure_s: 0.,
        wait_after_lift_s: 0.1,
        pwm: 255,
        lift: vec![
            MotionStage {
                distance_mm: 2.,
                speed_mm_s: 1.,
            },
            MotionStage {
                distance_mm: 4.,
                speed_mm_s: 3.,
            },
        ],
        retract: vec![MotionStage {
            distance_mm: 6.,
            speed_mm_s: 3.,
        }],
    }
}
fn settings() -> PrintSettings {
    PrintSettings {
        heating_temperature_c: None,
        layer_height_mm: 0.05,
        bottom_layer_count: 4,
        transition_layer_count: 3,
        bottom: LayerSettings {
            exposure_s: 30.,
            ..layer()
        },
        normal: layer(),
    }
}
fn resin() -> ResinProfile {
    ResinProfile {
        schema_version: 1,
        id: "test-resin".into(),
        name: "Independent test resin".into(),
        revision: 1,
        density_g_per_ml: Some(1.1),
        cost_per_liter: Some(20.),
        currency: Some("EUR".into()),
        calibrations: vec![PrinterCalibration {
            printer_id: "test-printer".into(),
            settings: settings(),
        }],
    }
}
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "karasu-profiles-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn store(&self) -> ProfileStore {
        ProfileStore::new(self.0.clone()).unwrap()
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn resin_usage_converts_volume_to_mass_and_cost() {
    let usage = resin().usage(2500.0);
    assert_eq!(usage.volume_ml, 2.5);
    assert!((usage.mass_g.unwrap() - 2.75).abs() < 1e-12);
    let (cost, currency) = usage.cost.unwrap();
    assert!((cost - 0.05).abs() < 1e-12);
    assert_eq!(currency, "EUR");
    let bare = ResinProfile {
        density_g_per_ml: None,
        cost_per_liter: None,
        currency: None,
        ..resin()
    };
    let usage = bare.usage(2500.0);
    assert_eq!((usage.mass_g, usage.cost), (None, None));
}
#[test]
fn typed_documents_round_trip_without_altering_values() {
    for p in [
        ProfileDocument::Printer(printer("test-printer")),
        ProfileDocument::Resin(resin()),
    ] {
        let bytes = p.to_json().unwrap();
        assert_eq!(ProfileDocument::from_json(&bytes).unwrap(), p);
        assert_eq!(bytes, p.to_json().unwrap());
    }
}
#[test]
fn future_unknown_duplicate_and_wrong_kind_are_rejected() {
    let valid = String::from_utf8(
        ProfileDocument::Printer(printer("test-printer"))
            .to_json()
            .unwrap(),
    )
    .unwrap();
    for changed in [
        // Printers read versions one and two (TD-106); a later one is refused.
        valid.replace("\"schema_version\": 1", "\"schema_version\": 3"),
        valid.replace(
            "\"schema_version\": 1",
            "\"schema_version\": 1, \"unknown\": true",
        ),
        valid.replace("\"revision\": 1", "\"revision\": 1, \"revision\": 1"),
        valid.replace("\"kind\": \"printer\"", "\"kind\": \"future\""),
    ] {
        assert!(
            ProfileDocument::from_json(changed.as_bytes()).is_err(),
            "{changed}"
        );
    }
}
#[test]
fn malformed_excessive_and_deep_inputs_fail() {
    assert!(ProfileDocument::from_json(&vec![b' '; MAX_PROFILE_BYTES + 1]).is_err());
    assert!(ProfileDocument::from_json(&[0xff]).is_err());
    assert!(ProfileDocument::from_json(
        format!("{}0{}", "[".repeat(33), "]".repeat(33)).as_bytes()
    )
    .is_err());
    assert!(ProfileDocument::from_json(b"{}").is_err());
}
#[test]
fn identifiers_reject_traversal_reserved_names_and_non_ascii() {
    for id in [
        "",
        "../printer",
        "foo/bar",
        "foo\\bar",
        "CON",
        "con",
        "com1",
        "lpt9",
        "-start",
        "end-",
        "é",
        "has space",
    ] {
        assert!(validate_profile_id(id).is_err(), "{id}");
    }
    assert!(validate_profile_id(&"a".repeat(65)).is_err());
    for id in ["printer-1", "123", "con-test"] {
        validate_profile_id(id).unwrap();
    }
}
#[test]
fn hardware_maps_explicit_origin_and_swapped_axes() {
    let mut p = printer("test-printer");
    p.swap_xy = true;
    p.mirror_x = true;
    p.validate().unwrap();
    let lcd = p.lcd_profile().unwrap();
    assert_eq!(lcd.origin_mm(), [-50., -30.]);
    assert_eq!(lcd.pitch_mm(), [100. / 600., 60. / 1000.]);
    assert!(lcd.orientation().flip_x);
    assert!(lcd.orientation().swap_xy);
}
#[test]
fn hardware_and_profile_metadata_fail_closed() {
    let good = printer("test-printer");
    for p in [
        PrinterProfile {
            resolution_px: [0, 600],
            ..good.clone()
        },
        PrinterProfile {
            build_size_mm: [f64::NAN, 60., 150.],
            ..good.clone()
        },
        PrinterProfile {
            origin_xy_mm: [f64::INFINITY, 0.],
            ..good.clone()
        },
        PrinterProfile {
            revision: 0,
            ..good.clone()
        },
        PrinterProfile {
            format_ids: vec!["x".into(), "x".into()],
            ..good.clone()
        },
        PrinterProfile {
            name: "\n".into(),
            ..good
        },
    ] {
        assert!(p.validate().is_err());
    }
}
#[test]
fn settings_reject_invalid_speeds_waits_and_unbalanced_travel() {
    for bad in [
        LayerSettings { pwm: 0, ..layer() },
        LayerSettings {
            exposure_s: f64::NAN,
            ..layer()
        },
        LayerSettings {
            wait_after_lift_s: -1.,
            ..layer()
        },
        LayerSettings {
            lift: vec![],
            ..layer()
        },
        LayerSettings {
            retract: vec![MotionStage {
                distance_mm: 5.,
                speed_mm_s: 1.,
            }],
            ..layer()
        },
        LayerSettings {
            lift: vec![MotionStage {
                distance_mm: 6.,
                speed_mm_s: 0.,
            }],
            ..layer()
        },
    ] {
        assert!(bad.validate(None).is_err());
    }
}
#[test]
fn printer_limits_and_overflowing_counts_are_enforced() {
    let p = printer("test-printer");
    settings().validate(Some(&p.limits)).unwrap();
    let mut s = settings();
    s.normal.exposure_s = 121.;
    assert!(s.validate(Some(&p.limits)).is_err());
    s = settings();
    s.layer_height_mm = 0.001;
    assert!(s.validate(Some(&p.limits)).is_err());
    s = settings();
    s.bottom_layer_count = u32::MAX;
    assert!(s.validate(None).is_err());
    s = settings();
    s.bottom_layer_count = 0;
    assert!(s.validate(None).is_err());
}
#[test]
fn calibration_requires_exact_printer_identity_and_uniqueness() {
    let mut r = resin();
    assert_eq!(r.calibration_for("test-printer").unwrap(), &settings());
    assert!(r.calibration_for("another-printer").is_err());
    r.calibrations.push(r.calibrations[0].clone());
    assert!(r.validate().is_err());
}
#[test]
fn resin_properties_are_explicit_and_finite() {
    for r in [
        ResinProfile {
            density_g_per_ml: Some(f64::INFINITY),
            ..resin()
        },
        ResinProfile {
            cost_per_liter: Some(-1.),
            ..resin()
        },
        ResinProfile {
            currency: None,
            ..resin()
        },
        ResinProfile {
            currency: Some("eur".into()),
            ..resin()
        },
    ] {
        assert!(r.validate().is_err());
    }
}
#[test]
fn linux_paths_follow_xdg_and_ignore_relative_values() {
    let lookup = |key: &str| match key {
        "HOME" => Some(PathBuf::from("/home/example")),
        "XDG_DATA_HOME" => Some(PathBuf::from("/custom/data")),
        "XDG_CONFIG_HOME" => Some(PathBuf::from("relative")),
        _ => None,
    };
    if cfg!(unix) {
        let p = ProfilePaths::from_environment(ProfilePlatform::Linux, lookup).unwrap();
        assert_eq!(p.data, PathBuf::from("/custom/data/karasu-slicer"));
        assert_eq!(
            p.config,
            PathBuf::from("/home/example/.config/karasu-slicer")
        );
        assert_ne!(p.config, p.data);
    }
}
#[test]
fn windows_discovery_uses_roaming_data_and_local_cache() {
    if cfg!(windows) {
        let paths = ProfilePaths::from_environment(ProfilePlatform::Windows, |key| match key {
            "APPDATA" => Some(PathBuf::from(r"C:\Users\Example\AppData\Roaming")),
            "LOCALAPPDATA" => Some(PathBuf::from(r"C:\Users\Example\AppData\Local")),
            _ => None,
        })
        .unwrap();
        assert_ne!(paths.data, paths.cache);
        assert!(paths.data.ends_with("karasu-slicer"));
    }
}
#[test]
fn missing_user_roots_and_relative_store_are_errors() {
    assert!(ProfilePaths::from_environment(ProfilePlatform::Linux, |_| None).is_err());
    assert!(ProfilePaths::from_environment(ProfilePlatform::Windows, |_| None).is_err());
    assert!(ProfileStore::new("relative").is_err());
}
#[test]
fn empty_library_has_no_creation_side_effect() {
    let t = Temp::new();
    let store = t.store();
    assert_eq!(store.list().unwrap(), Vec::<ProfileDocument>::new());
    assert!(!store.root().exists());
}
#[test]
fn create_load_catalog_import_and_export_preserve_schema() {
    let t = Temp::new();
    let store = t.store();
    store
        .save_printer(&printer("z-printer"), SavePolicy::CreateOnly)
        .unwrap();
    store
        .save_printer(&printer("a-printer"), SavePolicy::CreateOnly)
        .unwrap();
    let r = ProfileDocument::Resin(resin());
    store
        .import(&r.to_json().unwrap(), SavePolicy::CreateOnly)
        .unwrap();
    assert_eq!(
        store
            .list()
            .unwrap()
            .iter()
            .map(ProfileDocument::id)
            .collect::<Vec<_>>(),
        vec!["a-printer", "z-printer", "test-resin"]
    );
    assert_eq!(store.load_resin("test-resin").unwrap(), resin());
    assert_eq!(
        store.export(ProfileKind::Resin, "test-resin").unwrap(),
        r.to_json().unwrap()
    );
}
#[test]
fn replacement_preserves_previous_revision_and_rejects_stale_write() {
    let t = Temp::new();
    let store = t.store();
    let mut p = printer("test-printer");
    store.save_printer(&p, SavePolicy::CreateOnly).unwrap();
    let original = store.export(ProfileKind::Printer, &p.id).unwrap();
    p.revision = 2;
    p.name = "Changed printer".into();
    store
        .save_printer(
            &p,
            SavePolicy::Replace {
                expected_revision: 1,
            },
        )
        .unwrap();
    assert_eq!(store.load_printer(&p.id).unwrap(), p);
    assert_eq!(
        fs::read(store.root().join("history/printers/test-printer/1.json")).unwrap(),
        original
    );
    let new = store.export(ProfileKind::Printer, &p.id).unwrap();
    assert!(store
        .save_printer(
            &p,
            SavePolicy::Replace {
                expected_revision: 1
            }
        )
        .is_err());
    assert!(store.save_printer(&p, SavePolicy::CreateOnly).is_err());
    assert_eq!(store.export(ProfileKind::Printer, &p.id).unwrap(), new);
}
#[test]
fn failed_validation_and_lock_contention_retain_valid_bytes() {
    let t = Temp::new();
    let store = t.store();
    let p = printer("test-printer");
    store.save_printer(&p, SavePolicy::CreateOnly).unwrap();
    let original = store.export(ProfileKind::Printer, &p.id).unwrap();
    let mut invalid = p.clone();
    invalid.revision = 2;
    invalid.build_size_mm[0] = -1.;
    assert!(store
        .save_printer(
            &invalid,
            SavePolicy::Replace {
                expected_revision: 1
            }
        )
        .is_err());
    fs::write(store.root().join(".library.lock"), b"existing lock").unwrap();
    assert!(store
        .save_printer(&printer("another-printer"), SavePolicy::CreateOnly)
        .is_err());
    assert_eq!(
        fs::read(store.root().join(".library.lock")).unwrap(),
        b"existing lock"
    );
    assert_eq!(store.export(ProfileKind::Printer, &p.id).unwrap(), original);
}
#[test]
fn backup_failure_keeps_current_profile_and_releases_owned_lock() {
    let t = Temp::new();
    let store = t.store();
    let mut p = printer("test-printer");
    store.save_printer(&p, SavePolicy::CreateOnly).unwrap();
    let original = store.export(ProfileKind::Printer, &p.id).unwrap();
    fs::write(store.root().join("history"), b"not a directory").unwrap();
    p.revision = 2;
    assert!(store
        .save_printer(
            &p,
            SavePolicy::Replace {
                expected_revision: 1
            }
        )
        .is_err());
    assert_eq!(store.export(ProfileKind::Printer, &p.id).unwrap(), original);
    assert!(!store.root().join(".library.lock").exists());
}
#[test]
fn mismatched_filename_and_corrupt_profiles_are_not_hidden() {
    let t = Temp::new();
    let store = t.store();
    store
        .save_printer(&printer("test-printer"), SavePolicy::CreateOnly)
        .unwrap();
    fs::rename(
        store.root().join("printers/test-printer.json"),
        store.root().join("printers/wrong-id.json"),
    )
    .unwrap();
    assert!(store.list().is_err());
    assert!(store.load_printer("wrong-id").is_err());
}

#[test]
fn portable_import_preserves_noninitial_revision_in_a_fresh_library() {
    let t = Temp::new();
    let store = t.store();
    let mut p = printer("test-printer");
    p.revision = 2;
    let bytes = ProfileDocument::Printer(p.clone()).to_json().unwrap();
    store.import(&bytes, SavePolicy::CreateOnly).unwrap();
    assert_eq!(store.load_printer(&p.id).unwrap(), p);
    p.revision = 3;
    store
        .save_printer(
            &p,
            SavePolicy::Replace {
                expected_revision: 2,
            },
        )
        .unwrap();
    assert_eq!(store.load_printer(&p.id).unwrap(), p);
}

#[test]
fn difficult_binary64_calibration_values_survive_json_without_ulp_drift() {
    let mut state = 0xc21d_54ab_9e10_0011_u64;
    for index in 0..1024 {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let exponent = if index % 2 == 0 {
            0x3fe0_0000_0000_0000
        } else {
            0x3ff0_0000_0000_0000
        };
        let value = f64::from_bits(exponent | (state & 0x000f_ffff_ffff_ffff));
        let mut p = resin();
        let settings = &mut p.calibrations[0].settings;
        settings.normal.exposure_s = value;
        settings.normal.wait_before_exposure_s = value;
        settings.normal.lift = vec![MotionStage {
            distance_mm: value,
            speed_mm_s: value,
        }];
        settings.normal.retract = settings.normal.lift.clone();
        let document = ProfileDocument::Resin(p);
        let bytes = document.to_json().unwrap();
        let decoded = ProfileDocument::from_json(&bytes).unwrap();
        assert_eq!(decoded, document);
        let ProfileDocument::Resin(decoded) = decoded else {
            panic!("resin profile changed kind")
        };
        assert_eq!(
            decoded.calibrations[0].settings.normal.exposure_s.to_bits(),
            value.to_bits()
        );
        assert_eq!(
            decoded.calibrations[0].settings.normal.lift[0]
                .distance_mm
                .to_bits(),
            value.to_bits()
        );
        assert_eq!(ProfileDocument::Resin(decoded).to_json().unwrap(), bytes);
    }
}

fn material() -> ResinMaterial {
    ResinMaterial {
        colour: None,
        swatch: None,
        schema_version: LIBRARY_SCHEMA_VERSION,
        id: "test-material".into(),
        name: "Test material".into(),
        revision: 1,
        vendor: Some("anycubic".into()),
        kind: Some(ResinKind::WaterWashable),
        density_g_per_ml: Some(1.1),
        cost_per_liter: Some(30.),
        currency: Some("EUR".into()),
        based_on: Some(CatalogRef {
            id: "anycubic-standard-resin".into(),
            revision: 1,
        }),
    }
}
fn print_setting() -> PrintSettingProfile {
    PrintSettingProfile {
        schema_version: LIBRARY_SCHEMA_VERSION,
        id: "test-setting".into(),
        name: "Normal 0.05 mm".into(),
        revision: 1,
        printer_id: "test-printer".into(),
        resin_id: "test-material".into(),
        film: Some("NFEP".into()),
        status: SettingStatus::Official,
        source: Some("A manufacturer's table".into()),
        based_on: None,
        settings: settings(),
        image: ImageSettings {
            antialias: 4,
            min_gray: 20,
            xy_compensation_px: -1,
            bottom_xy_compensation_px: Some(-2),
        },
    }
}
#[test]
fn library_documents_round_trip_and_keep_their_kinds_apart() {
    let mut p = printer("test-printer");
    p.schema_version = LIBRARY_SCHEMA_VERSION;
    p.vendor = Some("anycubic".into());
    p.model = Some("Anycubic Photon P1".into());
    p.based_on = Some(CatalogRef {
        id: "anycubic-photon-p1".into(),
        revision: 2,
    });
    for document in [
        ProfileDocument::Printer(p),
        ProfileDocument::Material(material()),
        ProfileDocument::PrintSetting(print_setting()),
    ] {
        let bytes = document.to_json().unwrap();
        assert_eq!(ProfileDocument::from_json(&bytes).unwrap(), document);
    }
    // A version-one printer keeps its exact bytes: the new fields are written only when set.
    let bytes = ProfileDocument::Printer(printer("test-printer"))
        .to_json()
        .unwrap();
    let text = String::from_utf8(bytes).unwrap();
    assert!(!text.contains("vendor") && !text.contains("based_on"));
    // A library resin is not a version-one calibrated resin, and the reverse.
    let material =
        String::from_utf8(ProfileDocument::Material(material()).to_json().unwrap()).unwrap();
    assert!(
        ProfileDocument::from_json(material.replace("\"material\"", "\"resin\"").as_bytes())
            .is_err()
    );
    let calibrated = String::from_utf8(ProfileDocument::Resin(resin()).to_json().unwrap()).unwrap();
    assert!(
        ProfileDocument::from_json(calibrated.replace("\"resin\"", "\"material\"").as_bytes())
            .is_err()
    );
}
#[test]
fn a_resins_colour_and_a_settings_heating_are_optional_and_checked() {
    // Written only when set: a document from before them reads, and writes its bytes back.
    let plain = ProfileDocument::Material(material()).to_json().unwrap();
    assert!(!String::from_utf8_lossy(&plain).contains("colour"));
    let plain_setting = ProfileDocument::PrintSetting(print_setting())
        .to_json()
        .unwrap();
    assert!(!String::from_utf8_lossy(&plain_setting).contains("heating"));
    // Set, they round-trip.
    let mut grey = material();
    grey.colour = Some("Grey".into());
    grey.swatch = Some("#7A7A7A".into());
    grey.validate().unwrap();
    let document = ProfileDocument::Material(grey.clone());
    assert_eq!(
        ProfileDocument::from_json(&document.to_json().unwrap()).unwrap(),
        document
    );
    assert_eq!(
        karasu_core::profiles::swatch_rgb("#7A7A7A"),
        Some([0x7A, 0x7A, 0x7A])
    );
    let mut warm = print_setting();
    warm.settings.heating_temperature_c = Some(30.0);
    warm.validate().unwrap();
    // A swatch that is not #RRGGBB, a blank colour and a heating target out of range are refused.
    for swatch in ["7A7A7A", "#7A7A7", "#GGGGGG"] {
        let mut wrong = grey.clone();
        wrong.swatch = Some(swatch.into());
        assert!(wrong.validate().is_err(), "{swatch}");
    }
    let mut blank = grey.clone();
    blank.colour = Some("  ".into());
    assert!(blank.validate().is_err());
    for celsius in [5.0, 61.0, f64::NAN] {
        let mut wrong = warm.clone();
        wrong.settings.heating_temperature_c = Some(celsius);
        assert!(wrong.validate().is_err(), "{celsius}");
    }
}
#[test]
fn library_documents_fail_closed() {
    let mut m = material();
    m.schema_version = 1;
    assert!(m.validate().is_err());
    let mut m = material();
    m.vendor = Some("Any Cubic".into());
    assert!(m.validate().is_err());
    let mut m = material();
    m.currency = None;
    assert!(m.validate().is_err());
    let mut s = print_setting();
    s.image.antialias = 3;
    assert!(s.validate().is_err());
    let mut s = print_setting();
    s.image.antialias = 1;
    assert!(s.validate().is_err(), "a minimum gray needs antialiasing");
    let mut s = print_setting();
    s.image.xy_compensation_px = 65;
    assert!(s.validate().is_err());
    let mut s = print_setting();
    s.film = Some(String::new());
    assert!(s.validate().is_err());
    let mut s = print_setting();
    s.based_on = Some(CatalogRef {
        id: "x".into(),
        revision: 0,
    });
    assert!(s.validate().is_err());
    let mut r = resin();
    r.schema_version = LIBRARY_SCHEMA_VERSION;
    assert!(
        r.validate().is_err(),
        "a calibrated resin stays version one"
    );
}
#[test]
fn a_print_setting_and_its_resin_compose_the_jobs_calibrated_resin() {
    let job = print_setting().job_resin(&material()).unwrap();
    assert_eq!(job.id, "test-material");
    assert_eq!(job.calibration_for("test-printer").unwrap(), &settings());
    assert!(job.calibration_for("another-printer").is_err());
    assert_eq!(job.density_g_per_ml, Some(1.1));
    let mut other = material();
    other.id = "other-material".into();
    assert!(print_setting().job_resin(&other).is_err());
}
#[test]
fn the_library_store_is_separate_and_removal_keeps_history() {
    let t = Temp::new();
    let old = t.store();
    let library = ProfileStore::library(t.0.clone()).unwrap();
    assert_ne!(old.root(), library.root());
    assert!(library.root().ends_with("profiles/v2"));
    library
        .save(
            &ProfileDocument::Material(material()),
            SavePolicy::CreateOnly,
        )
        .unwrap();
    library
        .save(
            &ProfileDocument::PrintSetting(print_setting()),
            SavePolicy::CreateOnly,
        )
        .unwrap();
    assert_eq!(library.list().unwrap().len(), 2);
    assert_eq!(old.list().unwrap().len(), 0);
    assert!(library
        .remove(ProfileKind::Material, "test-material", 2)
        .is_err());
    library
        .remove(ProfileKind::Material, "test-material", 1)
        .unwrap();
    assert_eq!(library.list().unwrap().len(), 1);
    assert!(library
        .root()
        .join("history/materials/test-material/1.json")
        .exists());
    assert!(library
        .remove(ProfileKind::Material, "test-material", 1)
        .is_err());
    assert!(ProfileStore::in_directory("relative").is_err());
}
#[test]
fn catalog_differences_skip_identity_and_name_each_changed_field() {
    let ours = print_setting();
    let mut theirs = print_setting();
    theirs.id = "catalog-setting".into();
    theirs.name = "Renamed".into();
    theirs.revision = 7;
    theirs.printer_id = "catalog-printer".into();
    theirs.settings.normal.exposure_s = 2.5;
    theirs.image.antialias = 16;
    let changes = differences(&ours, &theirs).unwrap();
    let fields: Vec<&str> = changes.iter().map(|c| c.field.as_str()).collect();
    assert_eq!(fields, ["image.antialias", "settings.normal.exposure_s"]);
    assert_eq!(
        (changes[0].ours.as_str(), changes[0].theirs.as_str()),
        ("4", "16")
    );
    assert_eq!(
        differences(&ours, &ours).unwrap(),
        [] as [karasu_core::profiles::catalog::Difference; 0]
    );
}
