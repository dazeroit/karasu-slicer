//! The profile library (TD-106): the built-in catalog and the user's own printers, resins and
//! print settings.
//!
//! **The catalog** ([`Catalog`]) is the JSON documents under `assets/catalog`, grouped by vendor
//! and compiled into the executable: read-only, and checked whole when the library opens, so a
//! broken catalog fails the tests, not the user.
//!
//! **The user's library** lives in the core's [`ProfileStore`] (`profiles/v2` in the data
//! folder). It holds only what the user added: copies of catalog entries, each recording the
//! catalog revision it was copied from, and custom profiles. A newer catalog revision is offered
//! ([`Library::update`]), never applied on its own. Printers have unique names, so several
//! printers of one model can be told apart.
//!
//! **Print settings** belong to one of the user's printers and one of their resins. Adding a
//! resin copies the catalog's official settings for the printer; without one, a setting is made
//! only by an explicit [`Library::start_from`], marked as a starting point.
//!
//! **Version one:** a library from before TD-106 (`profiles/v1`) is converted once, on first
//! opening, and left in place ([`MigrationReport`]).
//!
//! **Order:** lists are sorted by name, then identity, so they are deterministic.
mod catalog;
mod catalog_files;
mod migration;

pub use catalog::{Catalog, CatalogMaterial, CatalogPrinter, CatalogSetting};
pub use migration::MigrationReport;

use karasu_core::profiles::{
    catalog::{differences, Difference},
    store::{ProfileDocument, ProfileKind, ProfileStore, SavePolicy},
    CatalogRef, PrintSettingProfile, PrinterProfile, ProfileError, ResinMaterial, SettingStatus,
    LIBRARY_SCHEMA_VERSION,
};
use std::{fmt, path::Path};

/// A refused library operation.
#[derive(Debug)]
pub enum LibraryError {
    /// A catalog file is invalid (a defect the tests catch).
    Catalog {
        /// The file, under `assets/catalog`.
        file: String,
        /// Why.
        reason: String,
    },
    /// The user library refused the operation.
    Store(ProfileError),
    /// There is no user library (no data folder).
    NoUserLibrary,
    /// No profile of this kind has this identity.
    Unknown {
        /// The kind of profile.
        kind: ProfileKind,
        /// The identity.
        id: String,
    },
    /// Another profile of the same kind already has this name.
    NameTaken(String),
    /// The operation does not apply, and why.
    Refused(String),
}

impl fmt::Display for LibraryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Catalog { file, reason } => write!(f, "built-in catalog {file}: {reason}"),
            Self::Store(error) => write!(f, "profile library: {error}"),
            Self::NoUserLibrary => f.write_str("there is no user profile library on this system"),
            Self::Unknown { kind, id } => write!(f, "no {} has identity {id}", noun(*kind)),
            Self::NameTaken(name) => write!(f, "the name {name:?} is already in use"),
            Self::Refused(reason) => f.write_str(reason),
        }
    }
}

impl std::error::Error for LibraryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Store(error) => Some(error),
            _ => None,
        }
    }
}

const fn noun(kind: ProfileKind) -> &'static str {
    match kind {
        ProfileKind::Printer => "printer",
        ProfileKind::Resin => "version-one resin",
        ProfileKind::Material => "resin",
        ProfileKind::PrintSetting => "print setting",
    }
}

/// A newer catalog revision of a library profile, and what it would change.
#[derive(Clone, Debug, PartialEq)]
pub struct Update {
    /// The catalog revision.
    pub revision: u64,
    /// The settings it changes, in field order.
    pub changes: Vec<Difference>,
}

/// What adding a resin added.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AddedResin {
    /// The resin's identity in the library (an existing copy of the same catalog resin is
    /// reused).
    pub resin: String,
    /// The print settings added for the printer.
    pub settings: Vec<String>,
}

/// What importing a profile file added and left out.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ImportReport {
    /// Names of the profiles added.
    pub added: Vec<String>,
    /// What was left out, and why.
    pub skipped: Vec<String>,
}

/// The catalog and, when there is a data folder, the user's library.
#[derive(Debug)]
pub struct Library {
    catalog: Catalog,
    store: Option<ProfileStore>,
    printers: Vec<PrinterProfile>,
    materials: Vec<ResinMaterial>,
    settings: Vec<PrintSettingProfile>,
    migration: Option<MigrationReport>,
}

impl Library {
    /// The catalog, and the user's library under the data folder `data_root` (read now). A
    /// version-one library there is converted first, once.
    ///
    /// # Errors
    /// An invalid catalog, a failed conversion, or a user library that cannot be listed.
    /// Without a data folder only the catalog is available.
    pub fn open(data_root: Option<&Path>) -> Result<Self, LibraryError> {
        let catalog = Catalog::built_in()?;
        let (store, migration) = match data_root {
            None => (None, None),
            Some(root) => {
                let migration = migration::migrate(root, &catalog)?;
                (
                    Some(ProfileStore::library(root).map_err(LibraryError::Store)?),
                    migration,
                )
            }
        };
        let mut library = Self {
            catalog,
            store,
            printers: Vec::new(),
            materials: Vec::new(),
            settings: Vec::new(),
            migration,
        };
        library.reload()?;
        Ok(library)
    }

    /// Read the user library again.
    ///
    /// # Errors
    /// A user library that exists but cannot be listed.
    pub fn reload(&mut self) -> Result<(), LibraryError> {
        self.printers.clear();
        self.materials.clear();
        self.settings.clear();
        let Some(store) = &self.store else {
            return Ok(());
        };
        if !store.root().exists() {
            return Ok(());
        }
        for document in store.list().map_err(LibraryError::Store)? {
            match document {
                ProfileDocument::Printer(p) => self.printers.push(p),
                ProfileDocument::Material(m) => self.materials.push(m),
                ProfileDocument::PrintSetting(s) => self.settings.push(s),
                // The library never stores version-one resins.
                ProfileDocument::Resin(_) => {}
            }
        }
        self.printers
            .sort_by(|a, b| (a.name.to_lowercase(), &a.id).cmp(&(b.name.to_lowercase(), &b.id)));
        self.materials
            .sort_by(|a, b| (a.name.to_lowercase(), &a.id).cmp(&(b.name.to_lowercase(), &b.id)));
        self.settings.sort_by(|a, b| {
            (&a.printer_id, &a.resin_id, a.name.to_lowercase(), &a.id).cmp(&(
                &b.printer_id,
                &b.resin_id,
                b.name.to_lowercase(),
                &b.id,
            ))
        });
        Ok(())
    }

    /// The built-in catalog.
    #[must_use]
    pub const fn catalog(&self) -> &Catalog {
        &self.catalog
    }

    /// What opening converted from a version-one library, once.
    #[must_use]
    pub const fn migration(&self) -> Option<&MigrationReport> {
        self.migration.as_ref()
    }

    /// Whether there is a user library to add to.
    #[must_use]
    pub const fn has_user_library(&self) -> bool {
        self.store.is_some()
    }

    /// The user's printers.
    #[must_use]
    pub fn printers(&self) -> &[PrinterProfile] {
        &self.printers
    }

    /// A user printer by identity.
    #[must_use]
    pub fn printer(&self, id: &str) -> Option<&PrinterProfile> {
        self.printers.iter().find(|p| p.id == id)
    }

    /// The user's resins.
    #[must_use]
    pub fn materials(&self) -> &[ResinMaterial] {
        &self.materials
    }

    /// A user resin by identity.
    #[must_use]
    pub fn material(&self, id: &str) -> Option<&ResinMaterial> {
        self.materials.iter().find(|m| m.id == id)
    }

    /// The user's print settings.
    #[must_use]
    pub fn settings(&self) -> &[PrintSettingProfile] {
        &self.settings
    }

    /// A user print setting by identity.
    #[must_use]
    pub fn setting(&self, id: &str) -> Option<&PrintSettingProfile> {
        self.settings.iter().find(|s| s.id == id)
    }

    /// The print settings for a printer and a resin of the user's.
    #[must_use]
    pub fn settings_for(&self, printer_id: &str, resin_id: &str) -> Vec<&PrintSettingProfile> {
        self.settings
            .iter()
            .filter(|s| s.printer_id == printer_id && s.resin_id == resin_id)
            .collect()
    }

    /// The user's resins with a print setting for `printer_id`, then those without one.
    #[must_use]
    pub fn resins_for(&self, printer_id: &str) -> (Vec<&ResinMaterial>, Vec<&ResinMaterial>) {
        self.materials.iter().partition(|m| {
            self.settings
                .iter()
                .any(|s| s.printer_id == printer_id && s.resin_id == m.id)
        })
    }

    /// The printer's verification facts from the catalog entry it is based on, when its panel
    /// orientation still matches that entry's.
    #[must_use]
    pub fn orientation_verified(&self, printer: &PrinterProfile) -> bool {
        printer
            .based_on
            .as_ref()
            .and_then(|b| self.catalog.printer(&b.id))
            .is_some_and(|c| {
                c.orientation_verified
                    && (c.profile.swap_xy, c.profile.mirror_x, c.profile.mirror_y)
                        == (printer.swap_xy, printer.mirror_x, printer.mirror_y)
            })
    }

    /// Whether another profile of `kind` than `except` is named `name` (ignoring case and
    /// surrounding spaces).
    #[must_use]
    pub fn name_taken(&self, kind: ProfileKind, name: &str, except: Option<&str>) -> bool {
        let name = name.trim().to_lowercase();
        let taken =
            |id: &str, other: &str| Some(id) != except && other.trim().to_lowercase() == name;
        match kind {
            ProfileKind::Printer => self.printers.iter().any(|p| taken(&p.id, &p.name)),
            ProfileKind::Material => self.materials.iter().any(|m| taken(&m.id, &m.name)),
            ProfileKind::PrintSetting | ProfileKind::Resin => false,
        }
    }

    /// A name for a new profile of `kind`: `base`, or `base (2)`, `base (3)` and so on.
    #[must_use]
    pub fn unique_name(&self, kind: ProfileKind, base: &str) -> String {
        let base = base.trim();
        let mut candidate = base.to_owned();
        let mut n = 1u64;
        while self.name_taken(kind, &candidate, None) {
            n += 1;
            candidate = format!("{base} ({n})");
        }
        candidate
    }

    fn taken_id(&self, kind: ProfileKind, id: &str) -> bool {
        match kind {
            ProfileKind::Printer => self.printer(id).is_some(),
            ProfileKind::Material => self.material(id).is_some(),
            ProfileKind::PrintSetting => self.setting(id).is_some(),
            ProfileKind::Resin => false,
        }
    }

    /// An identity not yet taken by a profile of `kind`, from the slug of `base`: the slug, then
    /// the slug with `-2`, `-3` and so on.
    #[must_use]
    pub fn free_id(&self, kind: ProfileKind, base: &str) -> String {
        let slug = slug(base);
        let mut candidate = slug.clone();
        let mut n = 1u64;
        while self.taken_id(kind, &candidate) {
            n += 1;
            candidate = format!("{slug}-{n}");
        }
        candidate
    }

    fn store(&self) -> Result<&ProfileStore, LibraryError> {
        self.store.as_ref().ok_or(LibraryError::NoUserLibrary)
    }

    fn create(&self, document: &ProfileDocument) -> Result<(), LibraryError> {
        self.store()?
            .save(document, SavePolicy::CreateOnly)
            .map_err(LibraryError::Store)
    }

    fn replace(
        &self,
        document: &ProfileDocument,
        expected_revision: u64,
    ) -> Result<(), LibraryError> {
        self.store()?
            .save(document, SavePolicy::Replace { expected_revision })
            .map_err(LibraryError::Store)
    }

    fn unique(
        &self,
        kind: ProfileKind,
        name: &str,
        except: Option<&str>,
    ) -> Result<(), LibraryError> {
        if self.name_taken(kind, name, except) {
            return Err(LibraryError::NameTaken(name.trim().to_owned()));
        }
        Ok(())
    }

    /// Add a copy of the catalog printer `catalog_id` named `name`, with copies of the catalog
    /// resins `resins` and of their catalog print settings for it; a resin without a print
    /// setting for the printer is not copied ([`Catalog::resins_for`] lists those that have one).
    ///
    /// # Errors
    /// No user library, an unknown catalog printer, a name in use, or the store's refusal.
    pub fn add_printer_from_catalog(
        &mut self,
        catalog_id: &str,
        name: &str,
        resins: &[&str],
    ) -> Result<String, LibraryError> {
        self.store()?;
        let entry = self
            .catalog
            .printer(catalog_id)
            .ok_or_else(|| unknown(ProfileKind::Printer, catalog_id))?;
        self.unique(ProfileKind::Printer, name, None)?;
        let mut printer = entry.profile.clone();
        printer.id = self.free_id(ProfileKind::Printer, catalog_id);
        name.trim().clone_into(&mut printer.name);
        printer.schema_version = LIBRARY_SCHEMA_VERSION;
        printer.revision = 1;
        printer.based_on = Some(CatalogRef {
            id: entry.profile.id.clone(),
            revision: entry.profile.revision,
        });
        let id = printer.id.clone();
        self.create(&ProfileDocument::Printer(printer))?;
        self.reload()?;
        let settings: Vec<CatalogSetting> = self
            .catalog
            .settings()
            .iter()
            .filter(|s| {
                s.profile.printer_id == catalog_id && resins.contains(&s.profile.resin_id.as_str())
            })
            .cloned()
            .collect();
        for setting in settings {
            let resin = self.material_copy(&setting.profile.resin_id)?;
            self.copy_setting(&setting, &id, &resin)?;
        }
        Ok(id)
    }

    /// Add a custom printer; its identity comes from its name, and its revision starts at one.
    ///
    /// # Errors
    /// No user library, a name in use, an invalid printer, or the store's refusal.
    pub fn add_custom_printer(
        &mut self,
        mut printer: PrinterProfile,
    ) -> Result<String, LibraryError> {
        self.store()?;
        self.unique(ProfileKind::Printer, &printer.name, None)?;
        trim(&mut printer.name);
        printer.id = self.free_id(ProfileKind::Printer, &printer.name);
        printer.schema_version = LIBRARY_SCHEMA_VERSION;
        printer.revision = 1;
        printer.based_on = None;
        let id = printer.id.clone();
        self.create(&ProfileDocument::Printer(printer))?;
        self.reload()?;
        Ok(id)
    }

    /// The user's copy of the catalog resin `catalog_id`, added when there is none yet.
    fn material_copy(&mut self, catalog_id: &str) -> Result<String, LibraryError> {
        if let Some(existing) = self
            .materials
            .iter()
            .find(|m| m.based_on.as_ref().is_some_and(|b| b.id == catalog_id))
        {
            return Ok(existing.id.clone());
        }
        let entry = self
            .catalog
            .material(catalog_id)
            .ok_or_else(|| unknown(ProfileKind::Material, catalog_id))?;
        let mut material = entry.profile.clone();
        material.id = self.free_id(ProfileKind::Material, catalog_id);
        material.name = self.unique_name(ProfileKind::Material, &material.name);
        material.revision = 1;
        material.based_on = Some(CatalogRef {
            id: entry.profile.id.clone(),
            revision: entry.profile.revision,
        });
        let id = material.id.clone();
        self.create(&ProfileDocument::Material(material))?;
        self.reload()?;
        Ok(id)
    }

    /// Copy a catalog print setting for the user's printer and resin, unless one based on it is
    /// already there.
    fn copy_setting(
        &mut self,
        setting: &CatalogSetting,
        printer_id: &str,
        resin_id: &str,
    ) -> Result<Option<String>, LibraryError> {
        let catalog_id = &setting.profile.id;
        if self.settings.iter().any(|s| {
            s.printer_id == printer_id
                && s.resin_id == resin_id
                && s.based_on.as_ref().is_some_and(|b| &b.id == catalog_id)
        }) {
            return Ok(None);
        }
        let mut copy = setting.profile.clone();
        copy.id = self.free_id(
            ProfileKind::PrintSetting,
            &format!("{printer_id}-{resin_id}"),
        );
        copy.revision = 1;
        printer_id.clone_into(&mut copy.printer_id);
        resin_id.clone_into(&mut copy.resin_id);
        copy.based_on = Some(CatalogRef {
            id: catalog_id.clone(),
            revision: setting.profile.revision,
        });
        let id = copy.id.clone();
        self.create(&ProfileDocument::PrintSetting(copy))?;
        self.reload()?;
        Ok(Some(id))
    }

    /// Add a copy of the catalog resin `catalog_id` (or reuse the user's copy of it). For the
    /// user's printer `for_printer`, the catalog's official settings for that printer's model
    /// are copied; when there are none, `start_from` (a setting of that printer) makes a
    /// starting point, as [`Self::start_from`] does.
    ///
    /// # Errors
    /// No user library, an unknown printer, resin or setting, or the store's refusal.
    pub fn add_material_from_catalog(
        &mut self,
        catalog_id: &str,
        for_printer: Option<&str>,
        start_from: Option<&str>,
    ) -> Result<AddedResin, LibraryError> {
        self.store()?;
        if let Some(printer) = for_printer {
            self.printer(printer)
                .ok_or_else(|| unknown(ProfileKind::Printer, printer))?;
        }
        let resin = self.material_copy(catalog_id)?;
        let mut added = AddedResin {
            resin: resin.clone(),
            settings: Vec::new(),
        };
        let Some(printer_id) = for_printer else {
            return Ok(added);
        };
        let model = self
            .printer(printer_id)
            .and_then(|p| p.based_on.as_ref())
            .map(|b| b.id.clone());
        let official: Vec<CatalogSetting> = model
            .map(|model| {
                self.catalog
                    .settings_for(&model, catalog_id)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        for setting in &official {
            if let Some(id) = self.copy_setting(setting, printer_id, &resin)? {
                added.settings.push(id);
            }
        }
        if official.is_empty() && self.settings_for(printer_id, &resin).is_empty() {
            if let Some(from) = start_from {
                let name = self
                    .setting(from)
                    .map(|s| s.name.clone())
                    .ok_or_else(|| unknown(ProfileKind::PrintSetting, from))?;
                added
                    .settings
                    .push(self.start_from(printer_id, &resin, from, &name)?);
            }
        }
        Ok(added)
    }

    /// Add a custom resin; its identity comes from its name. For `for_printer`, `start_from`
    /// makes a starting point as [`Self::start_from`] does.
    ///
    /// # Errors
    /// No user library, a name in use, an invalid resin, an unknown printer or setting, or the
    /// store's refusal.
    pub fn add_custom_material(
        &mut self,
        mut material: ResinMaterial,
        for_printer: Option<&str>,
        start_from: Option<&str>,
    ) -> Result<AddedResin, LibraryError> {
        self.store()?;
        self.unique(ProfileKind::Material, &material.name, None)?;
        trim(&mut material.name);
        material.id = self.free_id(ProfileKind::Material, &material.name);
        material.schema_version = LIBRARY_SCHEMA_VERSION;
        material.revision = 1;
        material.based_on = None;
        let resin = material.id.clone();
        self.create(&ProfileDocument::Material(material))?;
        self.reload()?;
        let mut added = AddedResin {
            resin: resin.clone(),
            settings: Vec::new(),
        };
        if let (Some(printer), Some(from)) = (for_printer, start_from) {
            let name = self
                .setting(from)
                .map(|s| s.name.clone())
                .ok_or_else(|| unknown(ProfileKind::PrintSetting, from))?;
            added
                .settings
                .push(self.start_from(printer, &resin, from, &name)?);
        }
        Ok(added)
    }

    /// A starting point for the user's printer and resin: a copy of the printer's setting
    /// `from` (for another resin), named `name` and marked to be calibrated.
    ///
    /// # Errors
    /// No user library, an unknown printer, resin or setting, a setting of another printer, or
    /// the store's refusal.
    pub fn start_from(
        &mut self,
        printer_id: &str,
        resin_id: &str,
        from: &str,
        name: &str,
    ) -> Result<String, LibraryError> {
        self.store()?;
        self.printer(printer_id)
            .ok_or_else(|| unknown(ProfileKind::Printer, printer_id))?;
        self.material(resin_id)
            .ok_or_else(|| unknown(ProfileKind::Material, resin_id))?;
        let source = self
            .setting(from)
            .ok_or_else(|| unknown(ProfileKind::PrintSetting, from))?;
        if source.printer_id != printer_id {
            return Err(LibraryError::Refused(format!(
                "print setting {from} is for another printer: a starting point copies a setting \
                 of the same printer"
            )));
        }
        let from_resin = self
            .material(&source.resin_id)
            .map_or_else(|| source.resin_id.clone(), |m| m.name.clone());
        let mut copy = source.clone();
        copy.id = self.free_id(
            ProfileKind::PrintSetting,
            &format!("{printer_id}-{resin_id}"),
        );
        name.trim().clone_into(&mut copy.name);
        copy.revision = 1;
        resin_id.clone_into(&mut copy.resin_id);
        copy.status = SettingStatus::StartingPoint;
        copy.source = Some(format!("From {from_resin} · {}", source.name));
        copy.based_on = None;
        let id = copy.id.clone();
        self.create(&ProfileDocument::PrintSetting(copy))?;
        self.reload()?;
        Ok(id)
    }

    /// A print setting entered by the user: `draft`'s printer, resin, name, film, settings and
    /// image settings, as the user's own with a new identity.
    ///
    /// # Errors
    /// No user library, an unknown printer or resin, invalid settings, or the store's refusal.
    pub fn new_setting(&mut self, draft: &PrintSettingProfile) -> Result<String, LibraryError> {
        self.store()?;
        let printer = self
            .printer(&draft.printer_id)
            .ok_or_else(|| unknown(ProfileKind::Printer, &draft.printer_id))?;
        draft
            .settings
            .validate(Some(&printer.limits))
            .map_err(LibraryError::Store)?;
        self.material(&draft.resin_id)
            .ok_or_else(|| unknown(ProfileKind::Material, &draft.resin_id))?;
        let setting = PrintSettingProfile {
            schema_version: LIBRARY_SCHEMA_VERSION,
            id: self.free_id(
                ProfileKind::PrintSetting,
                &format!("{}-{}", draft.printer_id, draft.resin_id),
            ),
            name: draft.name.trim().to_owned(),
            revision: 1,
            status: SettingStatus::User,
            source: None,
            based_on: None,
            ..draft.clone()
        };
        let id = setting.id.clone();
        self.create(&ProfileDocument::PrintSetting(setting))?;
        self.reload()?;
        Ok(id)
    }

    /// Save an edited printer as its next revision (its name must stay unique).
    ///
    /// # Errors
    /// No user library, an unknown printer, a name in use, an invalid printer, or the store's
    /// refusal (such as a revision changed elsewhere).
    pub fn save_printer(&mut self, mut printer: PrinterProfile) -> Result<(), LibraryError> {
        let current = self
            .printer(&printer.id)
            .ok_or_else(|| unknown(ProfileKind::Printer, &printer.id))?
            .revision;
        self.unique(ProfileKind::Printer, &printer.name, Some(&printer.id))?;
        trim(&mut printer.name);
        printer.schema_version = LIBRARY_SCHEMA_VERSION;
        printer.revision = current + 1;
        self.replace(&ProfileDocument::Printer(printer), current)?;
        self.reload()
    }

    /// Save an edited resin as its next revision (its name must stay unique).
    ///
    /// # Errors
    /// As [`Self::save_printer`].
    pub fn save_material(&mut self, mut material: ResinMaterial) -> Result<(), LibraryError> {
        let current = self
            .material(&material.id)
            .ok_or_else(|| unknown(ProfileKind::Material, &material.id))?
            .revision;
        self.unique(ProfileKind::Material, &material.name, Some(&material.id))?;
        trim(&mut material.name);
        material.revision = current + 1;
        self.replace(&ProfileDocument::Material(material), current)?;
        self.reload()
    }

    /// Save an edited print setting as its next revision. A changed official setting or
    /// starting point becomes the user's own; its origin is kept.
    ///
    /// # Errors
    /// No user library, an unknown setting, settings beyond the printer's limits, or the
    /// store's refusal.
    pub fn save_setting(&mut self, mut setting: PrintSettingProfile) -> Result<(), LibraryError> {
        let current = self
            .setting(&setting.id)
            .ok_or_else(|| unknown(ProfileKind::PrintSetting, &setting.id))?
            .clone();
        if let Some(printer) = self.printer(&setting.printer_id) {
            setting
                .settings
                .validate(Some(&printer.limits))
                .map_err(LibraryError::Store)?;
        }
        let changed = current.settings != setting.settings
            || current.image != setting.image
            || current.film != setting.film;
        if changed {
            setting.status = SettingStatus::User;
        }
        trim(&mut setting.name);
        setting.printer_id = current.printer_id;
        setting.resin_id = current.resin_id;
        setting.revision = current.revision + 1;
        self.replace(&ProfileDocument::PrintSetting(setting), current.revision)?;
        self.reload()
    }

    /// Copy the user's printer `id` with its print settings, named `name`.
    ///
    /// # Errors
    /// No user library, an unknown printer, a name in use, or the store's refusal.
    pub fn duplicate_printer(&mut self, id: &str, name: &str) -> Result<String, LibraryError> {
        self.store()?;
        let mut printer = self
            .printer(id)
            .ok_or_else(|| unknown(ProfileKind::Printer, id))?
            .clone();
        self.unique(ProfileKind::Printer, name, None)?;
        name.trim().clone_into(&mut printer.name);
        printer.id = self.free_id(ProfileKind::Printer, &printer.name);
        printer.revision = 1;
        let copy = printer.id.clone();
        self.create(&ProfileDocument::Printer(printer))?;
        self.reload()?;
        let settings: Vec<PrintSettingProfile> = self
            .settings
            .iter()
            .filter(|s| s.printer_id == id)
            .cloned()
            .collect();
        for mut setting in settings {
            setting.id = self.free_id(
                ProfileKind::PrintSetting,
                &format!("{copy}-{}", setting.resin_id),
            );
            setting.printer_id.clone_from(&copy);
            setting.revision = 1;
            self.create(&ProfileDocument::PrintSetting(setting))?;
            self.reload()?;
        }
        Ok(copy)
    }

    /// Remove a profile of the user's; a printer or a resin takes its print settings with it.
    /// Removed revisions stay in the store's history.
    ///
    /// # Errors
    /// No user library, an unknown profile, or the store's refusal.
    pub fn remove(&mut self, kind: ProfileKind, id: &str) -> Result<(), LibraryError> {
        let store = self.store()?.clone();
        let revision = self.revision(kind, id)?;
        let dependent: Vec<(String, u64)> = self
            .settings
            .iter()
            .filter(|s| match kind {
                ProfileKind::Printer => s.printer_id == id,
                ProfileKind::Material => s.resin_id == id,
                _ => false,
            })
            .map(|s| (s.id.clone(), s.revision))
            .collect();
        for (setting, revision) in dependent {
            store
                .remove(ProfileKind::PrintSetting, &setting, revision)
                .map_err(LibraryError::Store)?;
        }
        store
            .remove(kind, id, revision)
            .map_err(LibraryError::Store)?;
        self.reload()
    }

    fn revision(&self, kind: ProfileKind, id: &str) -> Result<u64, LibraryError> {
        let revision = match kind {
            ProfileKind::Printer => self.printer(id).map(|p| p.revision),
            ProfileKind::Material => self.material(id).map(|m| m.revision),
            ProfileKind::PrintSetting => self.setting(id).map(|s| s.revision),
            ProfileKind::Resin => None,
        };
        revision.ok_or_else(|| unknown(kind, id))
    }

    /// The newer catalog revision of the user's profile `id`, when there is one that changes
    /// something.
    #[must_use]
    pub fn update(&self, kind: ProfileKind, id: &str) -> Option<Update> {
        let (revision, changes) = match kind {
            ProfileKind::Printer => {
                let ours = self.printer(id)?;
                let based = ours.based_on.as_ref()?;
                let theirs = &self.catalog.printer(&based.id)?.profile;
                (theirs.revision > based.revision)
                    .then(|| (theirs.revision, differences(ours, theirs)))?
            }
            ProfileKind::Material => {
                let ours = self.material(id)?;
                let based = ours.based_on.as_ref()?;
                let theirs = &self.catalog.material(&based.id)?.profile;
                (theirs.revision > based.revision)
                    .then(|| (theirs.revision, differences(ours, theirs)))?
            }
            ProfileKind::PrintSetting => {
                let ours = self.setting(id)?;
                let based = ours.based_on.as_ref()?;
                let theirs = &self.catalog.setting(&based.id)?.profile;
                (theirs.revision > based.revision).then(|| {
                    (
                        theirs.revision,
                        differences(ours, theirs).map(|d| {
                            d.into_iter()
                                .filter(|d| d.field != "source")
                                .collect::<Vec<_>>()
                        }),
                    )
                })?
            }
            ProfileKind::Resin => return None,
        };
        let changes = changes.ok()?;
        (!changes.is_empty()).then_some(Update { revision, changes })
    }

    /// Take the catalog's newer revision of the user's profile `id`, keeping its identity and
    /// name, as its next revision; with `apply` false, only record that the user has seen it.
    ///
    /// # Errors
    /// No user library, a profile with no newer catalog revision, or the store's refusal.
    pub fn take_update(
        &mut self,
        kind: ProfileKind,
        id: &str,
        apply: bool,
    ) -> Result<(), LibraryError> {
        let none = || LibraryError::Refused(format!("{id} has no newer catalog revision"));
        let update = self.update(kind, id).ok_or_else(none)?;
        let based = |revision| {
            move |b: &CatalogRef| CatalogRef {
                id: b.id.clone(),
                revision,
            }
        };
        match kind {
            ProfileKind::Printer => {
                let ours = self.printer(id).ok_or_else(none)?.clone();
                let mut next = if apply {
                    let mut theirs = self
                        .catalog
                        .printer(&ours.based_on.as_ref().ok_or_else(none)?.id)
                        .ok_or_else(none)?
                        .profile
                        .clone();
                    theirs.id.clone_from(&ours.id);
                    theirs.name.clone_from(&ours.name);
                    theirs
                } else {
                    ours.clone()
                };
                next.based_on = ours.based_on.as_ref().map(based(update.revision));
                self.save_printer(next)
            }
            ProfileKind::Material => {
                let ours = self.material(id).ok_or_else(none)?.clone();
                let mut next = if apply {
                    let mut theirs = self
                        .catalog
                        .material(&ours.based_on.as_ref().ok_or_else(none)?.id)
                        .ok_or_else(none)?
                        .profile
                        .clone();
                    theirs.id.clone_from(&ours.id);
                    theirs.name.clone_from(&ours.name);
                    theirs
                } else {
                    ours.clone()
                };
                next.based_on = ours.based_on.as_ref().map(based(update.revision));
                self.save_material(next)
            }
            ProfileKind::PrintSetting => {
                let ours = self.setting(id).ok_or_else(none)?.clone();
                let current = ours.revision;
                let mut next = if apply {
                    let mut theirs = self
                        .catalog
                        .setting(&ours.based_on.as_ref().ok_or_else(none)?.id)
                        .ok_or_else(none)?
                        .profile
                        .clone();
                    theirs.id.clone_from(&ours.id);
                    theirs.name.clone_from(&ours.name);
                    theirs.printer_id.clone_from(&ours.printer_id);
                    theirs.resin_id.clone_from(&ours.resin_id);
                    theirs
                } else {
                    ours.clone()
                };
                next.based_on = ours.based_on.as_ref().map(based(update.revision));
                next.revision = current + 1;
                // The catalog's status and source come with its values; save_setting would
                // mark them as the user's.
                self.replace(&ProfileDocument::PrintSetting(next), current)?;
                self.reload()
            }
            ProfileKind::Resin => Err(none()),
        }
    }

    /// The deterministic JSON of the user's profile `id`, for a file.
    ///
    /// # Errors
    /// No user library, or the store's refusal.
    pub fn export(&self, kind: ProfileKind, id: &str) -> Result<Vec<u8>, LibraryError> {
        self.store()?.export(kind, id).map_err(LibraryError::Store)
    }

    /// Import a profile file into the user library. A printer or resin gets a free identity and,
    /// when its name is in use, a numbered one; a print setting needs its printer and resin in
    /// the library. A version-one resin becomes a resin and a print setting for each of its
    /// calibrations whose printer is in the library; the others are reported as left out.
    ///
    /// # Errors
    /// Invalid JSON or profile, a print setting whose printer or resin is missing, no user
    /// library, or the store's refusal.
    pub fn import(&mut self, bytes: &[u8]) -> Result<ImportReport, LibraryError> {
        self.store()?;
        let document = ProfileDocument::from_json(bytes).map_err(LibraryError::Store)?;
        let mut report = ImportReport::default();
        match document {
            ProfileDocument::Printer(mut printer) => {
                printer.name = self.unique_name(ProfileKind::Printer, &printer.name);
                printer.id = self.free_id(ProfileKind::Printer, &printer.id);
                printer.schema_version = LIBRARY_SCHEMA_VERSION;
                printer.revision = 1;
                report.added.push(printer.name.clone());
                self.create(&ProfileDocument::Printer(printer))?;
            }
            ProfileDocument::Material(mut material) => {
                material.name = self.unique_name(ProfileKind::Material, &material.name);
                material.id = self.free_id(ProfileKind::Material, &material.id);
                material.revision = 1;
                report.added.push(material.name.clone());
                self.create(&ProfileDocument::Material(material))?;
            }
            ProfileDocument::PrintSetting(mut setting) => {
                if self.printer(&setting.printer_id).is_none()
                    || self.material(&setting.resin_id).is_none()
                {
                    return Err(LibraryError::Refused(format!(
                        "print setting {:?} is for printer {} and resin {}: add them to the \
                         library first",
                        setting.name, setting.printer_id, setting.resin_id
                    )));
                }
                setting.id = self.free_id(ProfileKind::PrintSetting, &setting.id);
                setting.revision = 1;
                report.added.push(setting.name.clone());
                self.create(&ProfileDocument::PrintSetting(setting))?;
            }
            ProfileDocument::Resin(resin) => {
                let (material, settings) = migration::split(&resin);
                let mut material = material;
                material.name = self.unique_name(ProfileKind::Material, &material.name);
                material.id = self.free_id(ProfileKind::Material, &material.id);
                let resin_id = material.id.clone();
                report.added.push(material.name.clone());
                self.create(&ProfileDocument::Material(material))?;
                self.reload()?;
                for mut setting in settings {
                    if self.printer(&setting.printer_id).is_none() {
                        report.skipped.push(format!(
                            "the calibration for printer {}, which is not in the library",
                            setting.printer_id
                        ));
                        continue;
                    }
                    setting.resin_id.clone_from(&resin_id);
                    setting.id = self.free_id(
                        ProfileKind::PrintSetting,
                        &format!("{}-{resin_id}", setting.printer_id),
                    );
                    report.added.push(setting.name.clone());
                    self.create(&ProfileDocument::PrintSetting(setting))?;
                    self.reload()?;
                }
            }
        }
        self.reload()?;
        Ok(report)
    }
}

fn unknown(kind: ProfileKind, id: &str) -> LibraryError {
    LibraryError::Unknown {
        kind,
        id: id.to_owned(),
    }
}

/// Remove surrounding white space from `text` in place.
fn trim(text: &mut String) {
    let end = text.trim_end().len();
    text.truncate(end);
    let start = text.len() - text.trim_start().len();
    text.drain(..start);
}

/// A portable identity from display text: lowercase ASCII letters and digits joined by single
/// hyphens, at most 56 bytes so numbered variants stay within the 64-byte limit.
fn slug(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    out.truncate(56);
    let out = out.trim_end_matches('-').to_owned();
    if out.is_empty() || karasu_core::profiles::validate_profile_id(&out).is_err() {
        "profile".to_owned()
    } else {
        out
    }
}
