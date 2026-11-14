//! The profile library (TD-106): the built-in catalog, the user's printers, resins and print
//! settings, catalog revisions, imports and the conversion of a version-one library.
mod common;

use common::{print_setting, printer, resin, TempDirectory};
use karasu_core::profiles::{
    store::{ProfileDocument, ProfileKind, ProfileStore, SavePolicy},
    SettingStatus,
};
use karasu_project::library::{Catalog, Library, LibraryError};
use std::{collections::BTreeSet, fs, path::Path};

/// Every JSON file under `folder`, as a path relative to it.
fn json_files(folder: &Path, prefix: &str, out: &mut BTreeSet<String>) {
    for entry in fs::read_dir(folder).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name().into_string().unwrap();
        let relative = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        if entry.file_type().unwrap().is_dir() {
            json_files(&entry.path(), &relative, out);
        } else if Path::new(&name)
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("json"))
        {
            out.insert(relative);
        }
    }
}

#[test]
fn the_catalog_is_complete_valid_and_grouped_by_vendor() {
    let catalog = Catalog::built_in().unwrap();
    // Every JSON file under assets/catalog is compiled in, and nothing else.
    let folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/catalog");
    let mut on_disk = BTreeSet::new();
    json_files(&folder, "", &mut on_disk);
    let compiled: BTreeSet<String> = Catalog::files().map(str::to_owned).collect();
    assert_eq!(on_disk, compiled);
    assert!(!catalog.printers().is_empty() && !catalog.materials().is_empty());
    // Each document sits in its vendor's folder.
    for printer in catalog.printers() {
        let vendor = printer.profile.vendor.as_deref().unwrap();
        assert!(catalog.vendor(vendor).is_some());
        assert!(printer.profile.id.starts_with(&format!("{vendor}-")));
        assert!(printer.profile.model.is_some());
    }
    for setting in catalog.settings() {
        assert!(catalog.printer(&setting.profile.printer_id).is_some());
        assert!(catalog.material(&setting.profile.resin_id).is_some());
    }
    // The manufacturer's P1 default carries anti-aliasing (TD-106 point 8).
    let p1 = catalog
        .settings_for("anycubic-photon-p1", "anycubic-standard-resin")
        .next()
        .unwrap();
    assert_eq!(p1.profile.image.antialias, 16);
    assert_eq!(p1.profile.status, SettingStatus::Official);
}

#[test]
fn the_catalog_holds_each_makers_resins_and_offers_its_standard_one_first() {
    let catalog = Catalog::built_in().unwrap();
    let of = |vendor: &str| {
        catalog
            .materials()
            .iter()
            .filter(|m| m.profile.vendor.as_deref() == Some(vendor))
            .count()
    };
    // One entry per product (owner, 2026-11-11).
    assert_eq!((of("anycubic"), of("elegoo"), of("sunlu")), (16, 23, 16));
    for m in catalog.materials() {
        assert!(m
            .profile
            .density_g_per_ml
            .is_none_or(|d| (0.9..1.4).contains(&d)));
    }
    // SUNLU publishes no settings for any printer: its resins are materials only.
    assert!(catalog
        .settings()
        .iter()
        .all(|s| !s.profile.resin_id.starts_with("sunlu-")));
    assert_eq!(
        catalog
            .standard_resin_for("elegoo-saturn-3")
            .map(|m| m.profile.id.as_str()),
        Some("elegoo-standard-resin")
    );
    assert!(catalog.resins_for("elegoo-saturn-3").len() > 20);
    // No resin table covers the first Photon, nor bottom layers the Mars 4 Max.
    assert!(catalog.standard_resin_for("anycubic-photon").is_none());
    assert_eq!(catalog.resins_for("elegoo-mars-4-max").len(), 0);
    // ELEGOO publishes no 8K settings for the Jupiter SE.
    assert!(catalog
        .resins_for("elegoo-jupiter-se")
        .iter()
        .all(|m| !m.profile.id.starts_with("elegoo-8k-")));
    // A range's middle makes a starting point; the tilting printers' lift is karasu-slicer's.
    let tilt = catalog
        .settings_for("elegoo-saturn-4-ultra-16k", "elegoo-standard-resin")
        .next()
        .unwrap();
    assert_eq!(tilt.profile.status, SettingStatus::StartingPoint);
    assert!((tilt.profile.settings.normal.exposure_s - 2.55).abs() < 1e-9);
}

#[test]
fn printers_are_added_from_the_catalog_under_unique_names() {
    let temp = TempDirectory::new();
    let mut library = Library::open(Some(&temp.0)).unwrap();
    assert!(library.migration().is_none());
    assert_eq!(library.printers(), []);
    let first = library
        .add_printer_from_catalog(
            "anycubic-photon-p1",
            "Photon P1",
            &["anycubic-standard-resin"],
        )
        .unwrap();
    assert_eq!(first, "anycubic-photon-p1");
    let printer = library.printer(&first).unwrap();
    assert_eq!(printer.based_on.as_ref().unwrap().id, "anycubic-photon-p1");
    assert!(library.orientation_verified(printer));
    // The official resin and its setting came with it.
    let (ready, other) = library.resins_for(&first);
    assert_eq!((ready.len(), other.len()), (1, 0));
    let resin = ready[0].id.clone();
    assert_eq!(library.settings_for(&first, &resin).len(), 1);
    // Several printers of one model, each named apart.
    assert!(matches!(
        library.add_printer_from_catalog(
            "anycubic-photon-p1",
            " photon p1 ",
            &["anycubic-standard-resin"]
        ),
        Err(LibraryError::NameTaken(_))
    ));
    let name = library.unique_name(ProfileKind::Printer, "Photon P1");
    assert_eq!(name, "Photon P1 (2)");
    let second = library
        .add_printer_from_catalog("anycubic-photon-p1", &name, &["anycubic-standard-resin"])
        .unwrap();
    assert_eq!(second, "anycubic-photon-p1-2");
    // The resin copy is shared; each printer has its own setting.
    assert_eq!(library.materials().len(), 1);
    assert_eq!(library.settings_for(&second, &resin).len(), 1);
    // Reopening reads the same library.
    let reopened = Library::open(Some(&temp.0)).unwrap();
    assert_eq!(reopened.printers().len(), 2);
    assert_eq!(reopened.settings().len(), 2);
}

#[test]
fn a_resin_without_an_official_setting_starts_from_another_and_becomes_the_users() {
    let temp = TempDirectory::new();
    let mut library = Library::open(Some(&temp.0)).unwrap();
    let p1 = library
        .add_printer_from_catalog(
            "anycubic-photon-p1",
            "Photon P1",
            &["anycubic-standard-resin"],
        )
        .unwrap();
    let official = library.settings()[0].id.clone();
    // No official ELEGOO setting for the P1: nothing is invented without a "start from".
    let bare = library
        .add_material_from_catalog("elegoo-standard-resin", Some(&p1), None)
        .unwrap();
    assert_eq!(bare.settings, [] as [std::string::String; 0]);
    library.remove(ProfileKind::Material, &bare.resin).unwrap();
    let added = library
        .add_material_from_catalog("elegoo-standard-resin", Some(&p1), Some(&official))
        .unwrap();
    assert_eq!(added.settings.len(), 1);
    let start = library.setting(&added.settings[0]).unwrap().clone();
    assert_eq!(start.status, SettingStatus::StartingPoint);
    assert_eq!(start.resin_id, added.resin);
    assert!(start
        .source
        .as_deref()
        .unwrap()
        .contains("Anycubic Standard Resin"));
    // Saving it unchanged keeps it a starting point; a change makes it the user's.
    library.save_setting(start.clone()).unwrap();
    assert_eq!(
        library.setting(&start.id).unwrap().status,
        SettingStatus::StartingPoint
    );
    let mut edited = library.setting(&start.id).unwrap().clone();
    edited.settings.normal.exposure_s = 2.2;
    library.save_setting(edited).unwrap();
    let saved = library.setting(&start.id).unwrap();
    assert_eq!((saved.status, saved.revision), (SettingStatus::User, 3));
    // A starting point copies a setting of the same printer only.
    let photon = library
        .add_printer_from_catalog("anycubic-photon", "Photon", &[])
        .unwrap();
    assert!(matches!(
        library.start_from(&photon, &added.resin, &official, "Copy"),
        Err(LibraryError::Refused(_))
    ));
    // A custom resin, set up from the same official setting.
    let custom = library
        .add_custom_material(
            karasu_core::profiles::ResinMaterial {
                name: "Water Washable".into(),
                ..common::material()
            },
            Some(&p1),
            Some(&official),
        )
        .unwrap();
    assert_eq!(custom.resin, "water-washable");
    assert_eq!(custom.settings.len(), 1);
}

#[test]
fn copies_and_removals_carry_their_print_settings() {
    let temp = TempDirectory::new();
    let mut library = Library::open(Some(&temp.0)).unwrap();
    let p1 = library
        .add_printer_from_catalog(
            "anycubic-photon-p1",
            "Photon P1",
            &["anycubic-standard-resin"],
        )
        .unwrap();
    let copy = library.duplicate_printer(&p1, "P1 shelf 2").unwrap();
    assert_eq!(copy, "p1-shelf-2");
    assert_eq!(library.settings().len(), 2);
    assert!(library.settings().iter().any(|s| s.printer_id == copy));
    library.remove(ProfileKind::Printer, &p1).unwrap();
    assert_eq!(library.printers().len(), 1);
    assert_eq!(library.settings().len(), 1);
    assert_eq!(library.materials().len(), 1);
    let resin = library.materials()[0].id.clone();
    library.remove(ProfileKind::Material, &resin).unwrap();
    assert_eq!(library.settings(), []);
    // Removed revisions stay in the store's history.
    let history = temp
        .0
        .join("profiles/v2/history/printers")
        .join(&p1)
        .join("1.json");
    assert!(history.exists());
}

#[test]
fn a_newer_catalog_revision_is_offered_never_applied_on_its_own() {
    let temp = TempDirectory::new();
    let mut library = Library::open(Some(&temp.0)).unwrap();
    let saturn = library
        .add_printer_from_catalog("elegoo-saturn-3-ultra", "Saturn", &[])
        .unwrap();
    assert!(library.update(ProfileKind::Printer, &saturn).is_none());
    // A copy of revision 1, before the panel was mirrored as ELEGOO SatelLite's image (TD-153).
    let mut old = library.printer(&saturn).unwrap().clone();
    old.mirror_x = false;
    old.mirror_y = false;
    old.based_on.as_mut().unwrap().revision = 1;
    library.save_printer(old).unwrap();
    assert!(!library.orientation_verified(library.printer(&saturn).unwrap()));
    let update = library.update(ProfileKind::Printer, &saturn).unwrap();
    assert_eq!(update.revision, 2);
    let fields: Vec<&str> = update.changes.iter().map(|c| c.field.as_str()).collect();
    assert_eq!(fields, ["mirror_x", "mirror_y"]);
    // Dismissed: recorded as seen, nothing changed.
    let mut seen = Library::open(Some(&temp.0)).unwrap();
    seen.take_update(ProfileKind::Printer, &saturn, false)
        .unwrap();
    assert!(seen.update(ProfileKind::Printer, &saturn).is_none());
    assert!(!seen.printer(&saturn).unwrap().mirror_x);
    // Applied on another copy: the catalog's values, the user's name.
    let mut other = library.printer(&saturn).unwrap().clone();
    other.name = "Saturn".into();
    let mut library = Library::open(Some(&temp.0)).unwrap();
    other.based_on.as_mut().unwrap().revision = 1;
    other.revision = library.printer(&saturn).unwrap().revision;
    library.save_printer(other).unwrap();
    library
        .take_update(ProfileKind::Printer, &saturn, true)
        .unwrap();
    let applied = library.printer(&saturn).unwrap();
    assert!(applied.mirror_x && applied.mirror_y);
    assert_eq!(applied.name, "Saturn");
    assert!(library.update(ProfileKind::Printer, &saturn).is_none());
}

#[test]
fn a_p1_copied_before_its_fixed_layer_time_is_offered_it() {
    // TD-122: revision 2 of the catalog's P1 adds the 0.45 s it spends on each layer.
    let temp = TempDirectory::new();
    let mut library = Library::open(Some(&temp.0)).unwrap();
    let p1 = library
        .add_printer_from_catalog("anycubic-photon-p1", "P1", &[])
        .unwrap();
    assert_eq!(library.printer(&p1).unwrap().fixed_layer_time_s, Some(0.45));
    let mut old = library.printer(&p1).unwrap().clone();
    old.fixed_layer_time_s = None;
    old.based_on.as_mut().unwrap().revision = 1;
    library.save_printer(old).unwrap();
    let update = library.update(ProfileKind::Printer, &p1).unwrap();
    assert_eq!(update.revision, 2);
    let fields: Vec<&str> = update.changes.iter().map(|c| c.field.as_str()).collect();
    assert_eq!(fields, ["fixed_layer_time_s"]);
}

#[test]
fn a_version_one_library_is_converted_once_and_kept() {
    let temp = TempDirectory::new();
    let old = ProfileStore::new(&temp.0).unwrap();
    old.save_printer(&printer(), SavePolicy::CreateOnly)
        .unwrap();
    // A copy of a former preset, recognised by its suffix.
    let mut preset_copy = Catalog::built_in()
        .unwrap()
        .printer("anycubic-photon")
        .unwrap()
        .profile
        .clone();
    preset_copy.schema_version = 1;
    preset_copy.id = "anycubic-photon-custom".into();
    preset_copy.name = "My Photon".into();
    preset_copy.vendor = None;
    preset_copy.model = None;
    old.save_printer(&preset_copy, SavePolicy::CreateOnly)
        .unwrap();
    let mut calibrated = resin();
    let mut for_preset = calibrated.calibrations[0].clone();
    for_preset.printer_id = "anycubic-photon-p1".into();
    for_preset.settings.layer_height_mm = 0.05;
    let mut for_ghost = for_preset.clone();
    for_ghost.printer_id = "ghost-printer".into();
    calibrated.calibrations.push(for_preset);
    calibrated.calibrations.push(for_ghost);
    old.save_resin(&calibrated, SavePolicy::CreateOnly).unwrap();
    fs::write(old.root().join("printers/broken.json"), b"{").unwrap();
    let before = fs::read(old.root().join("resins/test-resin.json")).unwrap();

    let library = Library::open(Some(&temp.0)).unwrap();
    let report = library.migration().unwrap().clone();
    assert_eq!((report.printers, report.resins, report.settings), (2, 1, 2));
    assert_eq!(report.added_printers, ["Photon P1"]);
    assert_eq!(report.failures.len(), 2, "{:?}", report.failures);
    assert!(report.failures.iter().any(|f| f.contains("ghost-printer")));
    assert!(report.failures.iter().any(|f| f.contains("broken.json")));
    let mine = library.printer("anycubic-photon-custom").unwrap();
    assert_eq!(mine.based_on.as_ref().unwrap().id, "anycubic-photon");
    assert_eq!(mine.vendor.as_deref(), Some("anycubic"));
    assert_eq!(library.settings_for("test-printer", "test-resin").len(), 1);
    assert_eq!(
        library
            .settings_for("anycubic-photon-p1", "test-resin")
            .len(),
        1
    );
    // The version-one files are kept as they were; a second opening converts nothing.
    assert_eq!(
        fs::read(old.root().join("resins/test-resin.json")).unwrap(),
        before
    );
    assert!(Library::open(Some(&temp.0)).unwrap().migration().is_none());
}

#[test]
fn imports_take_free_identities_and_need_their_references() {
    let temp = TempDirectory::new();
    let mut library = Library::open(Some(&temp.0)).unwrap();
    let json = ProfileDocument::Printer(printer()).to_json().unwrap();
    library.import(&json).unwrap();
    let again = library.import(&json).unwrap();
    assert_eq!(again.added, ["Test printer (2)"]);
    assert!(library.printer("test-printer-2").is_some());
    // A print setting needs its printer and resin in the library.
    let setting = ProfileDocument::PrintSetting(print_setting())
        .to_json()
        .unwrap();
    assert!(matches!(
        library.import(&setting),
        Err(LibraryError::Refused(_))
    ));
    // A version-one resin: a resin and a setting per calibration whose printer is here.
    let mut calibrated = resin();
    let mut elsewhere = calibrated.calibrations[0].clone();
    elsewhere.printer_id = "unknown-printer".into();
    calibrated.calibrations.push(elsewhere);
    let report = library
        .import(&ProfileDocument::Resin(calibrated).to_json().unwrap())
        .unwrap();
    assert_eq!(report.added.len(), 2);
    assert_eq!(report.skipped.len(), 1);
    assert_eq!(library.settings_for("test-printer", "test-resin").len(), 1);
    library.import(&setting).unwrap();
    assert_eq!(library.settings_for("test-printer", "test-resin").len(), 2);
    // Export gives back the stored document.
    let exported = library
        .export(ProfileKind::Printer, "test-printer")
        .unwrap();
    assert!(ProfileDocument::from_json(&exported).is_ok());
}

#[test]
fn without_a_data_folder_only_the_catalog_is_available() {
    let mut library = Library::open(None).unwrap();
    assert!(!library.has_user_library());
    assert!(matches!(
        library.add_printer_from_catalog(
            "anycubic-photon-p1",
            "Photon P1",
            &["anycubic-standard-resin"]
        ),
        Err(LibraryError::NoUserLibrary)
    ));
    assert!(matches!(
        library.new_setting(&print_setting()),
        Err(LibraryError::NoUserLibrary)
    ));
}

#[test]
fn the_readme_lists_every_catalog_printer_with_its_state() {
    // The README's table of supported printers is generated with the catalog
    // (tools/assets/make_catalog.py); it must name every catalog printer once.
    let readme =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../README.md"))
            .unwrap();
    let start = readme
        .find("<!-- supported-printers: generated by tools/assets/make_catalog.py -->")
        .expect("the table's start marker");
    let end = readme[start..]
        .find("<!-- /supported-printers -->")
        .expect("the table's end marker")
        + start;
    let rows: Vec<&str> = readme[start..end]
        .lines()
        .filter(|l| l.starts_with("| ") && !l.starts_with("| Vendor") && !l.starts_with("| ---"))
        .collect();
    let catalog = Catalog::built_in().unwrap();
    assert_eq!(
        rows.len(),
        catalog.printers().len(),
        "regenerate the catalog"
    );
    for printer in catalog.printers() {
        let name = &printer.profile.name;
        assert_eq!(
            rows.iter()
                .filter(|row| row.split(" | ").nth(1) == Some(name.as_str()))
                .count(),
            1,
            "{name} in the README's table of supported printers"
        );
    }
}

#[test]
fn every_anycubic_catalog_printer_has_a_format_made_for_it() {
    use karasu_core::export::AnycubicWorkshop;
    let catalog = Catalog::built_in().unwrap();
    let mut checked = 0;
    for printer in catalog.printers() {
        let printer = &printer.profile;
        for format in &printer.format_ids {
            let Some(target) = AnycubicWorkshop::ALL.iter().find(|t| t.id() == format) else {
                continue;
            };
            // The format's models, "Photon Mono X 6K / M3 Plus", each named in full.
            let models: Vec<String> = target
                .models()
                .trim_end_matches(" (bit-planes)")
                .split(" / ")
                .map(|m| {
                    if m.starts_with("Photon") {
                        m.to_owned()
                    } else {
                        format!("Photon {m}")
                    }
                })
                .collect();
            assert!(
                models.contains(&printer.name),
                "{} is given {format}, made for {models:?}",
                printer.name
            );
            checked += 1;
        }
    }
    assert!(checked >= 20, "{checked}");
}

#[test]
fn every_catalog_printer_slices_with_each_of_its_formats() {
    use karasu_core::profiles::ResinMaterial;
    use karasu_project::{
        pipeline::{slice, Silent},
        project::{PlateContent, PlateObject, PlateSettings, Project},
    };
    let catalog = Catalog::built_in().unwrap();
    let temp = TempDirectory::new();
    let mut project = Project::new("Catalog");
    // A 4 x 4 x 1.25 mm box: 25 layers on every panel, more than any setting's bottom and
    // transition layers together (the Photon P1's 1 + 15).
    let model = project.add_model(
        "box.stl",
        common::box_stl([0.0; 3], [4.0, 4.0, 1.25]).into_bytes(),
    );
    for printer in catalog.printers() {
        let printer = &printer.profile;
        let (resin, setting): (ResinMaterial, _) = catalog
            .settings()
            .iter()
            .find(|s| s.profile.printer_id == printer.id)
            .map_or_else(
                || {
                    // A printer without a catalog setting still slices with a stand-in.
                    let mut setting = print_setting();
                    setting.printer_id.clone_from(&printer.id);
                    setting.settings.layer_height_mm = 0.05;
                    (common::material(), setting)
                },
                |s| {
                    (
                        catalog
                            .material(&s.profile.resin_id)
                            .unwrap()
                            .profile
                            .clone(),
                        s.profile.clone(),
                    )
                },
            );
        for format in &printer.format_ids {
            let mut object = PlateObject::new("box", model);
            object.pose.center = true;
            object.arrange = false;
            let plate = project.add_plate(PlateContent {
                name: format!("{} {format}", printer.id),
                printer: Some(printer.clone()),
                resin: Some(resin.clone()),
                print_setting: Some(setting.clone()),
                settings: PlateSettings::new(format.clone()),
                objects: vec![object],
            });
            let extension = karasu_project::request::machine_exporter(format)
                .unwrap()
                .map(|e| karasu_core::export::MachineExporter::extension(&e).to_owned())
                .unwrap();
            let output = temp.path(&format!("{}.{extension}", printer.id));
            let (request, _) = project.request(plate, output.clone()).unwrap();
            assert_eq!(request.antialias, setting.image.antialias);
            let outcome = slice(&request, &mut Silent, &|_| false)
                .unwrap_or_else(|e| panic!("{} {format}: {e}", printer.id));
            assert_eq!(outcome.export.layers, 25, "{} {format}", printer.id);
            assert!(output.exists(), "{} {format}", printer.id);
        }
    }
}

#[test]
fn a_plate_slices_only_with_a_print_setting_for_its_printer_and_resin() {
    use karasu_project::project::{
        PlateContent, PlateObject, PlateSettings, Project, ProjectError,
    };
    let temp = TempDirectory::new();
    let mut project = Project::new("Plate");
    let model = project.add_model(
        "box.stl",
        common::box_stl([0.0; 3], [4.0, 4.0, 2.0]).into_bytes(),
    );
    let mut setting = print_setting();
    setting.printer_id = "another-printer".into();
    let plate = project.add_plate(PlateContent {
        name: "plate".into(),
        printer: Some(printer()),
        resin: Some(common::material()),
        print_setting: Some(setting),
        settings: PlateSettings::new("goo-v3"),
        objects: vec![PlateObject::new("box", model)],
    });
    assert_eq!(
        project.request(plate, temp.path("x.goo")).unwrap_err(),
        ProjectError::NoPrintSetting(plate)
    );
    project
        .edit_plate(plate, |p| p.print_setting = Some(print_setting()))
        .unwrap();
    let (request, _) = project.request(plate, temp.path("x.goo")).unwrap();
    let karasu_project::request::Input::Loaded { value: resin, .. } = &request.resin else {
        panic!("a plate's resin is embedded");
    };
    assert_eq!(resin.calibrations.len(), 1);
    assert_eq!(resin.calibrations[0].printer_id, printer().id);
}
