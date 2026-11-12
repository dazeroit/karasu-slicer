//! Converting a version-one library (TD-077) into the library of TD-106, once.
//!
//! The version-one library (`profiles/v1`) held printers and resins with one calibration per
//! printer. On first opening, every readable profile is converted into a staging folder that
//! becomes `profiles/v2` only when complete, so an interrupted conversion is redone and never
//! leaves half a library. The version-one files are left untouched, as the backup. A file that
//! cannot be read or converted is reported, never dropped silently.
use super::{Catalog, LibraryError};
use karasu_core::profiles::{
    store::{ProfileDocument, ProfileStore, SavePolicy},
    CatalogRef, ImageSettings, PrintSettingProfile, PrinterProfile, ResinMaterial, ResinProfile,
    SettingStatus, LIBRARY_SCHEMA_VERSION,
};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

/// What converting a version-one library did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MigrationReport {
    /// Where the version-one files remain, untouched.
    pub kept_in: PathBuf,
    /// Printers converted.
    pub printers: usize,
    /// Resins converted into materials.
    pub resins: usize,
    /// Print settings made from resin calibrations.
    pub settings: usize,
    /// Printers added because a calibration named a former built-in preset, by name.
    pub added_printers: Vec<String>,
    /// Files or calibrations left out, and why.
    pub failures: Vec<String>,
}

fn store_error(error: karasu_core::profiles::ProfileError) -> LibraryError {
    LibraryError::Store(error)
}

fn io(operation: &str, path: &Path, error: &std::io::Error) -> LibraryError {
    LibraryError::Refused(format!(
        "profile library conversion: {operation} {}: {error}",
        path.display()
    ))
}

/// Convert `data_root`'s version-one library when there is one and no converted library yet.
pub(super) fn migrate(
    data_root: &Path,
    catalog: &Catalog,
) -> Result<Option<MigrationReport>, LibraryError> {
    let v1 = data_root.join("profiles/v1");
    let v2 = data_root.join("profiles/v2");
    if v2.exists() || !v1.exists() {
        return Ok(None);
    }
    let staging = data_root.join("profiles/v2.converting");
    if staging.exists() {
        fs::remove_dir_all(&staging).map_err(|e| io("remove the unfinished", &staging, &e))?;
    }
    let store = ProfileStore::in_directory(&staging).map_err(store_error)?;
    let mut report = MigrationReport {
        kept_in: v1.clone(),
        ..MigrationReport::default()
    };
    let printers = read(&v1.join("printers"), &mut report);
    let resins = read(&v1.join("resins"), &mut report);
    let mut ids: BTreeSet<String> = BTreeSet::new();
    let mut names: BTreeSet<String> = BTreeSet::new();
    for document in &printers {
        let ProfileDocument::Printer(printer) = document else {
            continue;
        };
        let printer = converted_printer(printer, catalog);
        ids.insert(printer.id.clone());
        names.insert(printer.name.to_lowercase());
        store
            .save(&ProfileDocument::Printer(printer), SavePolicy::CreateOnly)
            .map_err(store_error)?;
        report.printers += 1;
    }
    let mut setting_ids = BTreeSet::new();
    for document in &resins {
        let ProfileDocument::Resin(resin) = document else {
            continue;
        };
        let (mut material, settings) = split(resin);
        if let Some(entry) = catalog
            .materials()
            .iter()
            .find(|m| copy_of(&resin.id, &m.profile.id))
        {
            material.vendor.clone_from(&entry.profile.vendor);
            material.kind = entry.profile.kind;
            material.based_on = Some(CatalogRef {
                id: entry.profile.id.clone(),
                revision: 1,
            });
        }
        store
            .save(&ProfileDocument::Material(material), SavePolicy::CreateOnly)
            .map_err(store_error)?;
        report.resins += 1;
        for mut setting in settings {
            if !ids.contains(&setting.printer_id) {
                let Some(entry) = catalog.printer(&setting.printer_id) else {
                    report.failures.push(format!(
                        "resin {:?}: the calibration for printer {} was left out: that printer \
                         is neither yours nor in the catalog",
                        resin.name, setting.printer_id
                    ));
                    continue;
                };
                // The calibration was for a built-in preset, which the library now holds only
                // as the user's own copy.
                let mut printer = entry.profile.clone();
                printer.revision = 1;
                printer.based_on = Some(CatalogRef {
                    id: entry.profile.id.clone(),
                    revision: entry.profile.revision,
                });
                let mut name = printer.name.clone();
                let mut n = 1;
                while names.contains(&name.to_lowercase()) {
                    n += 1;
                    name = format!("{} ({n})", printer.name);
                }
                printer.name = name;
                names.insert(printer.name.to_lowercase());
                ids.insert(printer.id.clone());
                report.added_printers.push(printer.name.clone());
                store
                    .save(&ProfileDocument::Printer(printer), SavePolicy::CreateOnly)
                    .map_err(store_error)?;
            }
            setting.id = free(&mut setting_ids, &setting.id);
            store
                .save(
                    &ProfileDocument::PrintSetting(setting),
                    SavePolicy::CreateOnly,
                )
                .map_err(store_error)?;
            report.settings += 1;
        }
    }
    fs::rename(&staging, &v2).map_err(|e| io("publish", &v2, &e))?;
    Ok(Some(report))
}

/// Every readable version-one profile in `directory`, by file name; the others are reported.
fn read(directory: &Path, report: &mut MigrationReport) -> Vec<ProfileDocument> {
    let Ok(entries) = fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect();
    paths.sort();
    let mut documents = Vec::new();
    for path in paths {
        let parsed = fs::read(&path)
            .map_err(|e| e.to_string())
            .and_then(|bytes| ProfileDocument::from_json(&bytes).map_err(|e| e.to_string()));
        match parsed {
            Ok(document) => documents.push(document),
            Err(reason) => report
                .failures
                .push(format!("{} was left out: {reason}", path.display())),
        }
    }
    documents
}

/// Whether `id` is a version-one copy of the catalog entry `catalog_id`: the saved copy of a
/// preset was `<preset>-custom`, numbered `-2`, `-3`… when taken (TD-077).
fn copy_of(id: &str, catalog_id: &str) -> bool {
    id.strip_prefix(catalog_id)
        .and_then(|rest| rest.strip_prefix("-custom"))
        .is_some_and(|rest| {
            rest.is_empty()
                || rest
                    .strip_prefix('-')
                    .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        })
}

fn converted_printer(printer: &PrinterProfile, catalog: &Catalog) -> PrinterProfile {
    let mut printer = printer.clone();
    printer.schema_version = LIBRARY_SCHEMA_VERSION;
    if let Some(entry) = catalog
        .printers()
        .iter()
        .find(|c| copy_of(&printer.id, &c.profile.id))
    {
        printer.vendor.clone_from(&entry.profile.vendor);
        printer.model.clone_from(&entry.profile.model);
        // Which revision was copied is unknown: the first one, so a later revision that
        // changes anything is offered.
        printer.based_on = Some(CatalogRef {
            id: entry.profile.id.clone(),
            revision: 1,
        });
    }
    printer
}

/// An identity not in `taken`, from `base` (numbered as the library numbers them), recorded.
fn free(taken: &mut BTreeSet<String>, base: &str) -> String {
    let mut candidate = base.to_owned();
    let mut n = 1;
    while taken.contains(&candidate) {
        n += 1;
        candidate = format!("{base}-{n}");
    }
    taken.insert(candidate.clone());
    candidate
}

/// A version-one resin as a material and one user print setting per calibration, identified
/// `<printer>-<resin>` (shortened to fit an identity). Image settings are the command line's
/// defaults, as plates had them.
pub(super) fn split(resin: &ResinProfile) -> (ResinMaterial, Vec<PrintSettingProfile>) {
    let material = ResinMaterial {
        colour: None,
        swatch: None,
        schema_version: LIBRARY_SCHEMA_VERSION,
        id: resin.id.clone(),
        name: resin.name.clone(),
        revision: resin.revision,
        vendor: None,
        kind: None,
        density_g_per_ml: resin.density_g_per_ml,
        cost_per_liter: resin.cost_per_liter,
        currency: resin.currency.clone(),
        based_on: None,
    };
    let settings = resin
        .calibrations
        .iter()
        .map(|c| {
            let mut id = format!("{}-{}", c.printer_id, resin.id);
            id.truncate(56);
            PrintSettingProfile {
                schema_version: LIBRARY_SCHEMA_VERSION,
                id: id.trim_end_matches('-').to_owned(),
                name: format!("{} mm", c.settings.layer_height_mm),
                revision: 1,
                printer_id: c.printer_id.clone(),
                resin_id: resin.id.clone(),
                film: None,
                status: SettingStatus::User,
                source: Some(format!("Converted from your resin {:?}", resin.name)),
                based_on: None,
                settings: c.settings.clone(),
                image: ImageSettings::default(),
            }
        })
        .collect();
    (material, settings)
}

#[cfg(test)]
mod tests {
    use super::copy_of;

    #[test]
    fn version_one_copies_are_recognised_by_their_suffix() {
        assert!(copy_of("anycubic-photon-custom", "anycubic-photon"));
        assert!(copy_of("anycubic-photon-custom-3", "anycubic-photon"));
        assert!(!copy_of("anycubic-photon-s-custom", "anycubic-photon"));
        assert!(!copy_of("anycubic-photon-custom-x", "anycubic-photon"));
        assert!(!copy_of("anycubic-photon", "anycubic-photon"));
    }
}
