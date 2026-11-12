//! The built-in catalog (TD-106): vendors, printers, resins and print settings, read-only and
//! compiled into the executable from `assets/catalog`.
use super::{catalog_files::FILES, LibraryError};
use crate::request::format_names;
use karasu_core::profiles::{
    catalog::{CatalogIndex, Release, Vendor},
    store::ProfileDocument,
    PrintSettingProfile, PrinterProfile, ResinMaterial,
};
use std::collections::BTreeSet;

/// A catalog printer and what has been verified about it.
#[derive(Clone, Debug, PartialEq)]
pub struct CatalogPrinter {
    /// The printer.
    pub profile: PrinterProfile,
    /// How it releases each layer.
    pub release: Release,
    /// Whether its panel orientation was compared with the manufacturer's slicer output.
    pub orientation_verified: bool,
    /// Source keys of the catalog's provenance table.
    pub sources: String,
}

/// A catalog resin (material).
#[derive(Clone, Debug, PartialEq)]
pub struct CatalogMaterial {
    /// The resin.
    pub profile: ResinMaterial,
    /// Source keys of the catalog's provenance table.
    pub sources: String,
}

/// A catalog print setting.
#[derive(Clone, Debug, PartialEq)]
pub struct CatalogSetting {
    /// The print setting, for a catalog printer and a catalog resin.
    pub profile: PrintSettingProfile,
    /// Source keys of the catalog's provenance table.
    pub sources: String,
}

/// The built-in catalog, in the index's order.
#[derive(Clone, Debug, PartialEq)]
pub struct Catalog {
    vendors: Vec<Vendor>,
    printers: Vec<CatalogPrinter>,
    materials: Vec<CatalogMaterial>,
    settings: Vec<CatalogSetting>,
}

/// The index file's path under `assets/catalog`.
const INDEX: &str = "catalog.json";

fn defect(file: &str, reason: impl Into<String>) -> LibraryError {
    LibraryError::Catalog {
        file: file.to_owned(),
        reason: reason.into(),
    }
}

fn document(file: &str) -> Result<ProfileDocument, LibraryError> {
    let (_, json) = FILES
        .iter()
        .find(|(path, _)| *path == file)
        .ok_or_else(|| defect(file, "listed in the index but not compiled in"))?;
    ProfileDocument::from_json(json.as_bytes()).map_err(|e| defect(file, e.to_string()))
}

impl Catalog {
    /// The catalog compiled into the executable, checked whole: every file listed once, every
    /// document of its section's kind, unique identities, known vendors whose formats this build
    /// writes and include their printers' formats, and print settings for catalog printers and
    /// resins within the printer's limits.
    ///
    /// # Errors
    /// The first defect, by file (a defect the tests catch before release).
    pub fn built_in() -> Result<Self, LibraryError> {
        let (_, index) = FILES
            .iter()
            .find(|(path, _)| *path == INDEX)
            .ok_or_else(|| defect(INDEX, "the catalog index is not compiled in"))?;
        let index =
            CatalogIndex::from_json(index.as_bytes()).map_err(|e| defect(INDEX, e.to_string()))?;
        let listed: Vec<&str> = std::iter::once(INDEX)
            .chain(index.printers.iter().map(|e| e.file.as_str()))
            .chain(index.materials.iter().map(|e| e.file.as_str()))
            .chain(index.print_settings.iter().map(|e| e.file.as_str()))
            .collect();
        let mut seen = BTreeSet::new();
        for file in &listed {
            if !seen.insert(*file) {
                return Err(defect(file, "listed twice in the index"));
            }
        }
        if let Some((file, _)) = FILES.iter().find(|(path, _)| !seen.contains(path)) {
            return Err(defect(file, "compiled in but not listed in the index"));
        }
        let vendor = |file: &str, id: Option<&String>| match id {
            Some(id) if !index.vendors.iter().any(|v| &v.id == id) => {
                Err(defect(file, format!("unknown vendor {id}")))
            }
            _ => Ok(()),
        };
        let known: Vec<String> = format_names().into_iter().map(|f| f.id).collect();
        for v in &index.vendors {
            if let Some(format) = v.formats.iter().find(|f| !known.contains(f)) {
                return Err(defect(
                    INDEX,
                    format!("vendor {} lists unknown format {format}", v.id),
                ));
            }
        }
        let mut printers = Vec::new();
        for entry in &index.printers {
            let ProfileDocument::Printer(profile) = document(&entry.file)? else {
                return Err(defect(&entry.file, "not a printer"));
            };
            vendor(&entry.file, profile.vendor.as_ref())?;
            let listed = profile
                .vendor
                .as_ref()
                .and_then(|id| index.vendors.iter().find(|v| &v.id == id))
                .map_or(&[][..], |v| v.formats.as_slice());
            if let Some(format) = profile.format_ids.iter().find(|f| !listed.contains(f)) {
                return Err(defect(
                    &entry.file,
                    format!("format {format} is not among its vendor's formats"),
                ));
            }
            printers.push(CatalogPrinter {
                profile,
                release: entry.release,
                orientation_verified: entry.orientation_verified,
                sources: entry.sources.clone(),
            });
        }
        let mut materials = Vec::new();
        for entry in &index.materials {
            let ProfileDocument::Material(profile) = document(&entry.file)? else {
                return Err(defect(&entry.file, "not a resin material"));
            };
            vendor(&entry.file, profile.vendor.as_ref())?;
            materials.push(CatalogMaterial {
                profile,
                sources: entry.sources.clone(),
            });
        }
        let mut settings = Vec::new();
        for entry in &index.print_settings {
            let ProfileDocument::PrintSetting(profile) = document(&entry.file)? else {
                return Err(defect(&entry.file, "not a print setting"));
            };
            let printer = printers
                .iter()
                .find(|p| p.profile.id == profile.printer_id)
                .ok_or_else(|| defect(&entry.file, "its printer is not in the catalog"))?;
            if !materials.iter().any(|m| m.profile.id == profile.resin_id) {
                return Err(defect(&entry.file, "its resin is not in the catalog"));
            }
            profile
                .settings
                .validate(Some(&printer.profile.limits))
                .map_err(|e| defect(&entry.file, e.to_string()))?;
            settings.push(CatalogSetting {
                profile,
                sources: entry.sources.clone(),
            });
        }
        let catalog = Self {
            vendors: index.vendors,
            printers,
            materials,
            settings,
        };
        catalog.check_identities()?;
        catalog.check_standard_resins()?;
        Ok(catalog)
    }

    /// Each vendor's standard resin, when it names one, is a catalog resin of that vendor.
    fn check_standard_resins(&self) -> Result<(), LibraryError> {
        for v in &self.vendors {
            if let Some(resin) = &v.standard_resin {
                if self
                    .material(resin)
                    .is_none_or(|m| m.profile.vendor.as_ref() != Some(&v.id))
                {
                    return Err(defect(
                        INDEX,
                        format!("vendor {}'s standard resin {resin} is not its resin", v.id),
                    ));
                }
            }
        }
        Ok(())
    }

    fn check_identities(&self) -> Result<(), LibraryError> {
        let unique = |kind: &str, ids: Vec<&str>| {
            let mut seen = BTreeSet::new();
            for id in ids {
                if !seen.insert(id) {
                    return Err(defect(INDEX, format!("{kind} {id} appears twice")));
                }
            }
            Ok(())
        };
        unique(
            "printer",
            self.printers
                .iter()
                .map(|p| p.profile.id.as_str())
                .collect(),
        )?;
        unique(
            "resin",
            self.materials
                .iter()
                .map(|m| m.profile.id.as_str())
                .collect(),
        )?;
        unique(
            "print setting",
            self.settings
                .iter()
                .map(|s| s.profile.id.as_str())
                .collect(),
        )
    }

    /// Every compiled-in file's path under `assets/catalog`.
    pub fn files() -> impl Iterator<Item = &'static str> {
        FILES.iter().map(|(path, _)| *path)
    }

    /// Vendors, in the index's order.
    #[must_use]
    pub fn vendors(&self) -> &[Vendor] {
        &self.vendors
    }

    /// A vendor by identity.
    #[must_use]
    pub fn vendor(&self, id: &str) -> Option<&Vendor> {
        self.vendors.iter().find(|v| v.id == id)
    }

    /// Printers, in the index's order.
    #[must_use]
    pub fn printers(&self) -> &[CatalogPrinter] {
        &self.printers
    }

    /// A printer by identity.
    #[must_use]
    pub fn printer(&self, id: &str) -> Option<&CatalogPrinter> {
        self.printers.iter().find(|p| p.profile.id == id)
    }

    /// Resins, in the index's order.
    #[must_use]
    pub fn materials(&self) -> &[CatalogMaterial] {
        &self.materials
    }

    /// A resin by identity.
    #[must_use]
    pub fn material(&self, id: &str) -> Option<&CatalogMaterial> {
        self.materials.iter().find(|m| m.profile.id == id)
    }

    /// Print settings, in the index's order.
    #[must_use]
    pub fn settings(&self) -> &[CatalogSetting] {
        &self.settings
    }

    /// A print setting by identity.
    #[must_use]
    pub fn setting(&self, id: &str) -> Option<&CatalogSetting> {
        self.settings.iter().find(|s| s.profile.id == id)
    }

    /// The print settings for a catalog printer and resin.
    pub fn settings_for<'a>(
        &'a self,
        printer_id: &'a str,
        resin_id: &'a str,
    ) -> impl Iterator<Item = &'a CatalogSetting> + 'a {
        self.settings
            .iter()
            .filter(move |s| s.profile.printer_id == printer_id && s.profile.resin_id == resin_id)
    }

    /// The resins with print settings for a catalog printer, in the index's order.
    #[must_use]
    pub fn resins_for(&self, printer_id: &str) -> Vec<&CatalogMaterial> {
        self.materials
            .iter()
            .filter(|m| {
                self.settings.iter().any(|s| {
                    s.profile.printer_id == printer_id && s.profile.resin_id == m.profile.id
                })
            })
            .collect()
    }

    /// The standard resin of a catalog printer's vendor when it has print settings for the
    /// printer: the resin offered already chosen when the printer is added.
    #[must_use]
    pub fn standard_resin_for(&self, printer_id: &str) -> Option<&CatalogMaterial> {
        let vendor = self.printer(printer_id)?.profile.vendor.as_deref()?;
        let resin = self.vendor(vendor)?.standard_resin.as_deref()?;
        self.resins_for(printer_id)
            .into_iter()
            .find(|m| m.profile.id == resin)
    }
}
