//! The project model (M6.1b, TD-071): a library of embedded models and the plates that print
//! them, each with its own printer, resin, print setting, settings and objects.
//!
//! **Library:** every model file is embedded once, identified by the SHA-256 of its bytes; adding
//! the same bytes again returns the existing entry. Plates refer to models by that identity, so
//! copies on several plates share one set of bytes.
//!
//! **Plates and revisions:** every plate edit goes through [`Project::edit_plate`], which
//! increments the plate's revision. A job records the revision it was built from, and its result
//! is current only while the plate still has that revision ([`Project::freshness`]); an older
//! result never replaces a newer edit (ROADMAP M6, TD-041).
//!
//! **Jobs:** [`Project::request`] turns a plate into a [`SliceRequest`] over embedded data, so
//! a plate is sliced by exactly the pipeline the command line runs.
//!
//! **Persistence:** [`file`](mod@file) saves a project as a `.karasu` package and opens one
//! (TD-130).
use crate::request::{
    Blocker, Coordinates, DrainHole, Fill, Input, ModelSource, ObjectSpec, PlateSpec, Pose,
    Simplify, SliceRequest, Subject, DEFAULT_CLOSE_HOLES_MM, DEFAULT_JOB_ID, DEFAULT_REPAIR,
};
use karasu_core::{
    digest::sha256,
    layer_plan::LayerPlanDocument,
    mesh::formats::LengthUnit,
    plate::DEFAULT_SPACING_MM,
    preparation::repair::RepairSpec,
    profiles::{PrintSettingProfile, PrintSettings, PrinterProfile, ResinMaterial, ResinProfile},
    supports::SupportDocument,
};
use std::{fmt, path::PathBuf, sync::Arc};

pub mod file;

/// A library model's identity: the SHA-256 of its file bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ModelId(pub [u8; 32]);

impl fmt::Display for ModelId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.iter().try_for_each(|byte| write!(f, "{byte:02x}"))
    }
}

/// One embedded model file.
#[derive(Clone, Debug, PartialEq)]
pub struct LibraryModel {
    /// Content identity.
    pub id: ModelId,
    /// Original file name; its extension selects the import format.
    pub name: String,
    /// The file's bytes.
    pub bytes: Arc<[u8]>,
}

/// A plate's identity within its project; never reused after removal.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PlateId(pub u64);

/// Job-wide settings of a plate: output format, fill and repair. Masks and compensation belong
/// to the print setting (TD-106).
#[derive(Clone, Debug, PartialEq)]
pub struct PlateSettings {
    /// Output format identifier (see [`crate::request::format_ids`]).
    pub format: String,
    /// Print-job identity recorded in exported metadata.
    pub job_id: String,
    /// How layer sections become material.
    pub fill: Fill,
    /// Gap bound of section closure at holes (mm), or `None` when disabled.
    pub close_holes_mm: Option<f64>,
    /// Negative-winding area per layer (mm²) left empty instead of failing, or `None` to refuse
    /// any (TD-079).
    pub reversed_tolerance_mm2: Option<f64>,
    /// Source repairs applied before slicing or hollowing.
    pub repair: RepairSpec,
    /// Slice models with topology defects anyway.
    pub allow_topology_defects: bool,
    /// Variable or adaptive layer heights (TD-064).
    pub layer_plan: Option<LayerPlanDocument>,
    /// Gap between arranged footprints, in mm.
    pub spacing_mm: f64,
    /// What the plate's masks take instead of its print setting's (TD-134).
    pub image: ImageOverride,
}

/// A plate's own mask settings (TD-134): each value given replaces its print setting's for this
/// plate only.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ImageOverride {
    /// Samples per pixel axis (1, 2, 4, 8 or 16; 1 turns anti-aliasing off), or `None` for the
    /// print setting's.
    pub antialias: Option<u8>,
    /// Smallest gray of a partially covered pixel, or `None` for the print setting's. With
    /// anti-aliasing off it is 0 whatever is given.
    pub min_gray: Option<u8>,
    /// Anti-alias the supports with the model; off, they are binary (the default).
    pub antialias_supports: bool,
}

impl ImageOverride {
    /// The anti-aliasing and smallest gray the plate slices with, over `setting`'s.
    #[must_use]
    pub fn resolve(&self, setting: &karasu_core::profiles::ImageSettings) -> (u8, u8) {
        let antialias = self.antialias.unwrap_or(setting.antialias);
        let min_gray = if antialias == 1 {
            0
        } else {
            self.min_gray.unwrap_or(setting.min_gray)
        };
        (antialias, min_gray)
    }
}

impl PlateSettings {
    /// The command line's defaults, writing `format`.
    #[must_use]
    pub fn new(format: impl Into<String>) -> Self {
        Self {
            format: format.into(),
            job_id: DEFAULT_JOB_ID.to_owned(),
            fill: Fill::Union,
            close_holes_mm: Some(DEFAULT_CLOSE_HOLES_MM),
            reversed_tolerance_mm2: None,
            repair: DEFAULT_REPAIR,
            allow_topology_defects: false,
            layer_plan: None,
            spacing_mm: DEFAULT_SPACING_MM,
            image: ImageOverride::default(),
        }
    }
}

/// Hollowing of one object (TD-089): computed in its model frame, so it follows the object.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hollowing {
    /// Wall thickness, in mm.
    pub wall_mm: f64,
    /// Explicit grid cell, in mm, or `None` for the default.
    pub cell_mm: Option<f64>,
    /// Certified cavity simplification, if any.
    pub simplify: Option<Simplify>,
}

/// What an operation does to its object (TD-089, TD-090).
#[derive(Clone, Debug, PartialEq)]
pub enum OperationKind {
    /// Repair the model with these repairs instead of the plate's (M6.4d, TD-092). An object is
    /// repaired at most once, before every other operation.
    Repair(RepairSpec),
    /// Hollow the model. An object is hollowed at most once.
    Hollow(Hollowing),
    /// Cut a drain hole, in the object's model frame: it follows every move, turn, scale and
    /// mirror of the object. Its diameter and depth stay in millimetres.
    DrainHole(DrainHole),
    /// Leave a region solid when the object is hollowed (owner, 2026-11-02), in its model frame.
    Blocker(Blocker),
}

/// One operation applied to an object, in its timeline (TD-090).
///
/// Operations never change the object's model: they are evaluated in order on the untouched
/// source, so removing or suspending one returns exactly to the object without it.
#[derive(Clone, Debug, PartialEq)]
pub struct ObjectOperation {
    /// Stable identity within the object; never reused while the object exists.
    pub id: u64,
    /// Whether the operation applies; a suspended operation is kept but ignored.
    pub enabled: bool,
    /// What it does.
    pub kind: OperationKind,
}

/// One object of a plate: a library model with its placement, preparation and copies.
#[derive(Clone, Debug, PartialEq)]
pub struct PlateObject {
    /// Display name.
    pub name: String,
    /// The library model.
    pub model: ModelId,
    /// Unit for model formats that declare none.
    pub units: Option<LengthUnit>,
    /// Explicit design translation, in mm.
    pub translate_mm: [f64; 3],
    /// Rotation, scale, mirroring, centring, dropping and lift.
    pub pose: Pose,
    /// The operations applied to the model, in timeline order (TD-090): hollowing and drain
    /// holes.
    pub operations: Vec<ObjectOperation>,
    /// Manual supports, in the object's model frame (TD-108): they follow every move, turn,
    /// scale and mirror, as drain holes do.
    pub supports: Option<SupportDocument>,
    /// Accept every support collision (still reported).
    pub accept_support_collisions: bool,
    /// Number of instances (at least one).
    pub copies: u32,
    /// Whether the arranger places the instances; otherwise the object stays where it is.
    pub arrange: bool,
    /// The points the Orient tool keeps from facing the plate (TD-119), in the model's frame:
    /// kept with the object, so they follow it and are saved with it (TD-130).
    pub protected: Vec<ProtectedPoint>,
}

/// A point of a model the Orient tool protects (TD-119): a tilt is refused when its surface
/// would face the plate past the critical angle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProtectedPoint {
    /// Its place on the surface, in the model's frame, in mm.
    pub point_mm: [f64; 3],
    /// The surface's outward unit normal there, in the model's frame.
    pub normal: [f64; 3],
}

impl PlateObject {
    /// The enabled hollowing, if any.
    #[must_use]
    pub fn hollowing(&self) -> Option<&Hollowing> {
        self.operations.iter().find_map(|o| match &o.kind {
            OperationKind::Hollow(h) if o.enabled => Some(h),
            _ => None,
        })
    }

    /// The object's enabled repairs, if it has its own.
    #[must_use]
    pub fn repair(&self) -> Option<RepairSpec> {
        self.operations.iter().find_map(|o| match &o.kind {
            OperationKind::Repair(spec) if o.enabled => Some(*spec),
            _ => None,
        })
    }

    /// Give the object its own repairs, replacing earlier ones in place (keeping their identity),
    /// or return it to the plate's repairs with `None`. A new repair comes first in the timeline.
    pub fn set_repair(&mut self, repair: Option<RepairSpec>) {
        let existing = self
            .operations
            .iter()
            .position(|o| matches!(o.kind, OperationKind::Repair(_)));
        match (existing, repair) {
            (Some(index), Some(spec)) => {
                self.operations[index].kind = OperationKind::Repair(spec);
                self.operations[index].enabled = true;
            }
            (Some(index), None) => {
                self.operations.remove(index);
            }
            (None, Some(spec)) => {
                let id = self.next_operation_id();
                self.operations.insert(
                    0,
                    ObjectOperation {
                        id,
                        enabled: true,
                        kind: OperationKind::Repair(spec),
                    },
                );
            }
            (None, None) => {}
        }
    }

    /// The enabled drain holes, in timeline order, in the model frame.
    pub fn drain_holes(&self) -> impl Iterator<Item = &DrainHole> {
        self.operations.iter().filter_map(|o| match &o.kind {
            OperationKind::DrainHole(hole) if o.enabled => Some(hole),
            _ => None,
        })
    }

    /// The enabled hollowing blockers, in timeline order.
    pub fn blockers(&self) -> impl Iterator<Item = &Blocker> {
        self.operations.iter().filter_map(|o| match &o.kind {
            OperationKind::Blocker(blocker) if o.enabled => Some(blocker),
            _ => None,
        })
    }

    /// The enabled drain holes with their operation identities, in timeline order.
    pub fn drain_holes_mut(&mut self) -> impl Iterator<Item = (u64, &mut DrainHole)> {
        self.operations
            .iter_mut()
            .filter_map(|o| match &mut o.kind {
                OperationKind::DrainHole(hole) if o.enabled => Some((o.id, hole)),
                _ => None,
            })
    }

    /// The identity the next operation gets.
    #[must_use]
    pub fn next_operation_id(&self) -> u64 {
        self.operations
            .iter()
            .map(|o| o.id)
            .max()
            .map_or(1, |id| id + 1)
    }

    /// Append an enabled operation; returns its identity.
    pub fn push_operation(&mut self, kind: OperationKind) -> u64 {
        let id = self.next_operation_id();
        self.operations.push(ObjectOperation {
            id,
            enabled: true,
            kind,
        });
        id
    }

    /// Apply `hollowing`, replacing the object's hollowing in place (keeping its identity and
    /// place in the timeline), or remove it with `None`.
    pub fn set_hollowing(&mut self, hollowing: Option<Hollowing>) {
        let existing = self
            .operations
            .iter()
            .position(|o| matches!(o.kind, OperationKind::Hollow(_)));
        match (existing, hollowing) {
            (Some(index), Some(h)) => {
                self.operations[index].kind = OperationKind::Hollow(h);
                self.operations[index].enabled = true;
            }
            (Some(index), None) => {
                self.operations.remove(index);
            }
            (None, Some(h)) => {
                // Hollowing comes first after a repair: holes are evaluated on the hollowed
                // model.
                let id = self.next_operation_id();
                let first = usize::from(
                    self.operations
                        .first()
                        .is_some_and(|o| matches!(o.kind, OperationKind::Repair(_))),
                );
                self.operations.insert(
                    first,
                    ObjectOperation {
                        id,
                        enabled: true,
                        kind: OperationKind::Hollow(h),
                    },
                );
            }
            (None, None) => {}
        }
    }

    /// Remove the operation `id`, if present.
    pub fn remove_operation(&mut self, id: u64) {
        self.operations.retain(|o| o.id != id);
    }

    /// One arranged instance of `model` in millimetres, resting on the plate, without
    /// preparation.
    #[must_use]
    pub fn new(name: impl Into<String>, model: ModelId) -> Self {
        Self {
            name: name.into(),
            model,
            units: Some(LengthUnit::Millimeter),
            translate_mm: [0.; 3],
            pose: Pose {
                drop: true,
                ..Pose::default()
            },
            operations: Vec::new(),
            supports: None,
            accept_support_collisions: false,
            copies: 1,
            arrange: true,
            protected: Vec::new(),
        }
    }
}

/// What a plate holds; edited through [`Project::edit_plate`].
#[derive(Clone, Debug, PartialEq)]
pub struct PlateContent {
    /// Display name.
    pub name: String,
    /// The printer the plate prints on; slicing needs one.
    pub printer: Option<PrinterProfile>,
    /// The resin; slicing needs one.
    pub resin: Option<ResinMaterial>,
    /// The print setting for the printer and the resin (TD-106); slicing needs one.
    pub print_setting: Option<PrintSettingProfile>,
    /// Job-wide settings.
    pub settings: PlateSettings,
    /// Objects in plate order.
    pub objects: Vec<PlateObject>,
}

impl PlateContent {
    /// The print setting, when it is for the plate's printer and resin: a setting left from
    /// another printer or resin does not count.
    #[must_use]
    pub fn setting(&self) -> Option<&PrintSettingProfile> {
        let (printer, resin, setting) = (
            self.printer.as_ref()?,
            self.resin.as_ref()?,
            self.print_setting.as_ref()?,
        );
        (setting.printer_id == printer.id && setting.resin_id == resin.id).then_some(setting)
    }

    /// The exposure, rests and motion the plate prints with, from its print setting.
    #[must_use]
    pub fn calibration(&self) -> Option<&PrintSettings> {
        self.setting().map(|s| &s.settings)
    }

    /// The calibrated resin a print job takes (the resin with its print setting for the
    /// printer), or `None` without a print setting for the printer and resin.
    ///
    /// The inner error explains an invalid resin or setting.
    #[must_use]
    pub fn job_resin(&self) -> Option<Result<ResinProfile, String>> {
        let setting = self.setting()?;
        let resin = self.resin.as_ref()?;
        Some(setting.job_resin(resin).map_err(|e| e.to_string()))
    }
}

/// Most undo steps kept per plate; the oldest are dropped first.
pub const MAX_UNDO_STEPS: usize = 200;

/// The edit history of one plate (TD-081): the contents before each step, newest last, each
/// with the project revision the step was made at (TD-135), so that a project's undo takes the
/// newest step of a plate or of its structure.
///
/// Undo and redo restore earlier contents as new revisions, so results computed before stay
/// stale even when the content matches them again: revisions only grow.
#[derive(Clone, Debug, Default, PartialEq)]
struct History {
    undo: Vec<(PlateContent, u64)>,
    redo: Vec<(PlateContent, u64)>,
    /// The merge key of the last step, while further edits with it extend that step.
    merging: Option<u64>,
}

/// One change of a project's plates themselves (TD-135), as its undo keeps it.
#[derive(Clone, Debug, PartialEq)]
enum StructureStep {
    /// The plate was added at this position.
    Added { index: usize, plate: Plate },
    /// The plate was removed from this position.
    Removed { index: usize, plate: Plate },
    /// The plate at `from` was moved to `to`.
    Moved { from: usize, to: usize },
    /// The contents of these plates were replaced at once (objects moved or copied between
    /// plates): each holds the content the plate does not have now, before the step when it is
    /// undoable, after it when it is undone.
    Contents {
        plates: Vec<(PlateId, PlateContent)>,
    },
    /// These steps, made in order as one.
    Several(Vec<StructureStep>),
}

impl StructureStep {
    /// The plates whose content the step replaces.
    fn touched(&self) -> Vec<PlateId> {
        match self {
            Self::Contents { plates } => plates.iter().map(|(id, _)| *id).collect(),
            Self::Several(steps) => steps.iter().flat_map(Self::touched).collect(),
            _ => Vec::new(),
        }
    }
}

/// The history of a project's structure (TD-135): its steps with the project revision each was
/// made at, newest last.
#[derive(Clone, Debug, Default, PartialEq)]
struct Structure {
    undo: Vec<(StructureStep, u64)>,
    redo: Vec<(StructureStep, u64)>,
}

/// What a project's undo or redo changed (TD-135).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    /// A plate's content, at its new revision.
    Plate(PlateId, u64),
    /// The plates themselves: one was added or restored (the plate to show), removed or moved.
    Structure(Option<PlateId>),
}

/// A plate: its content, the revision of that content and its edit history.
#[derive(Clone, Debug, PartialEq)]
pub struct Plate {
    id: PlateId,
    revision: u64,
    content: PlateContent,
    history: History,
}

impl Plate {
    /// The plate's identity.
    #[must_use]
    pub fn id(&self) -> PlateId {
        self.id
    }

    /// Raised by every edit of the plate, to a value no other plate of the project has had
    /// (TD-135), so that a result computed for one plate never describes another.
    #[must_use]
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// What the plate holds.
    #[must_use]
    pub fn content(&self) -> &PlateContent {
        &self.content
    }

    /// Whether an edit can be undone.
    #[must_use]
    pub fn can_undo(&self) -> bool {
        !self.history.undo.is_empty()
    }

    /// Whether an undone edit can be redone.
    #[must_use]
    pub fn can_redo(&self) -> bool {
        !self.history.redo.is_empty()
    }
}

/// Whether a result computed from a plate revision still describes the plate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Freshness {
    /// The plate still has the revision the result was computed from.
    Current,
    /// The plate was edited since; the result must not replace the current state.
    Stale {
        /// The plate's revision now.
        current: u64,
    },
    /// The plate no longer exists.
    Removed,
}

/// A refused project operation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProjectError {
    /// No plate has this identity.
    UnknownPlate(PlateId),
    /// An object refers to a model the library does not hold.
    UnknownModel {
        /// The plate.
        plate: PlateId,
        /// 1-based object position.
        object: usize,
        /// The missing model.
        model: ModelId,
    },
    /// The model is still used by these plates.
    ModelInUse {
        /// The model.
        model: ModelId,
        /// Plates that place it.
        plates: Vec<PlateId>,
    },
    /// The plate has no object to print.
    EmptyPlate(PlateId),
    /// The plate has no printer selected.
    NoPrinter(PlateId),
    /// The plate has no resin selected.
    NoResin(PlateId),
    /// The plate has no print setting for its printer and resin.
    NoPrintSetting(PlateId),
    /// The plate's settings or an object are refused by the job checks.
    Invalid {
        /// The plate.
        plate: PlateId,
        /// The reason, naming the object when one is at fault.
        reason: String,
    },
}

impl fmt::Display for ProjectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownPlate(plate) => write!(f, "plate {} does not exist", plate.0),
            Self::UnknownModel {
                plate,
                object,
                model,
            } => write!(
                f,
                "plate {} object {object} uses model {model}, which the library does not hold",
                plate.0
            ),
            Self::ModelInUse { model, plates } => {
                let ids: Vec<String> = plates.iter().map(|p| p.0.to_string()).collect();
                write!(f, "model {model} is used by plates {}", ids.join(", "))
            }
            Self::EmptyPlate(plate) => write!(f, "plate {} has no objects", plate.0),
            Self::NoPrinter(plate) => write!(f, "plate {} has no printer selected", plate.0),
            Self::NoResin(plate) => write!(f, "plate {} has no resin selected", plate.0),
            Self::NoPrintSetting(plate) => write!(
                f,
                "plate {} has no print setting for its printer and resin",
                plate.0
            ),
            Self::Invalid { plate, reason } => write!(f, "plate {}: {reason}", plate.0),
        }
    }
}

impl std::error::Error for ProjectError {}

/// A project: the model library and the plates.
#[derive(Clone, Debug, PartialEq)]
pub struct Project {
    /// Display name.
    pub name: String,
    models: Vec<LibraryModel>,
    plates: Vec<Plate>,
    next_plate: u64,
    /// The last revision given to any plate (TD-135).
    revision: u64,
    /// The undo and redo of the plates themselves (TD-135).
    structure: Structure,
}

impl Project {
    /// An empty project.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            models: Vec::new(),
            plates: Vec::new(),
            next_plate: 1,
            revision: 0,
            structure: Structure::default(),
        }
    }

    /// A revision no plate of the project has had yet.
    fn next_revision(&mut self) -> u64 {
        self.revision += 1;
        self.revision
    }

    /// Embed a model file and return its identity. The same bytes are stored once: adding them
    /// again returns the existing identity and keeps the first name.
    pub fn add_model(&mut self, name: impl Into<String>, bytes: impl Into<Arc<[u8]>>) -> ModelId {
        let bytes = bytes.into();
        let id = ModelId(sha256(&bytes));
        if self.model(id).is_none() {
            self.models.push(LibraryModel {
                id,
                name: name.into(),
                bytes,
            });
        }
        id
    }

    /// The library model with this identity.
    #[must_use]
    pub fn model(&self, id: ModelId) -> Option<&LibraryModel> {
        self.models.iter().find(|m| m.id == id)
    }

    /// Library models, in the order they were first added.
    #[must_use]
    pub fn models(&self) -> &[LibraryModel] {
        &self.models
    }

    /// Remove a model that no plate uses.
    ///
    /// # Errors
    /// [`ProjectError::ModelInUse`] naming the plates that still place it.
    pub fn remove_model(&mut self, id: ModelId) -> Result<Option<LibraryModel>, ProjectError> {
        let plates: Vec<PlateId> = self
            .plates
            .iter()
            .filter(|p| p.content.objects.iter().any(|o| o.model == id))
            .map(|p| p.id)
            .collect();
        if !plates.is_empty() {
            return Err(ProjectError::ModelInUse { model: id, plates });
        }
        let index = self.models.iter().position(|m| m.id == id);
        Ok(index.map(|index| self.models.remove(index)))
    }

    /// Add a plate at the end, with no undo step, and return its identity: a project set up in
    /// code or read from a file.
    pub fn add_plate(&mut self, content: PlateContent) -> PlateId {
        let id = self.new_plate(content);
        self.plates.push(id.1);
        id.0
    }

    /// A plate holding `content`, at a new revision, with a new identity.
    fn new_plate(&mut self, content: PlateContent) -> (PlateId, Plate) {
        let id = PlateId(self.next_plate);
        self.next_plate += 1;
        let revision = self.next_revision();
        (
            id,
            Plate {
                id,
                revision,
                content,
                history: History::default(),
            },
        )
    }

    /// Insert a plate holding `content` at `index` (at most the number of plates) as one undo
    /// step of the project's structure (TD-135), and return its identity.
    pub fn insert_plate(&mut self, index: usize, content: PlateContent) -> PlateId {
        let (id, plate) = self.new_plate(content);
        let index = index.min(self.plates.len());
        self.plates.insert(index, plate.clone());
        self.record(StructureStep::Added { index, plate });
        id
    }

    /// Delete a plate as one undo step of the project's structure (TD-135): undoing it brings
    /// the plate back where it was, with its content and its own history.
    ///
    /// # Errors
    /// [`ProjectError::UnknownPlate`].
    pub fn delete_plate(&mut self, id: PlateId) -> Result<(), ProjectError> {
        let index = self
            .plates
            .iter()
            .position(|p| p.id == id)
            .ok_or(ProjectError::UnknownPlate(id))?;
        let plate = self.plates.remove(index);
        self.record(StructureStep::Removed { index, plate });
        Ok(())
    }

    /// Move a plate to position `to` (clamped to the last) as one undo step of the project's
    /// structure (TD-135). Moving it where it is records nothing.
    ///
    /// # Errors
    /// [`ProjectError::UnknownPlate`].
    pub fn move_plate(&mut self, id: PlateId, to: usize) -> Result<(), ProjectError> {
        let from = self
            .plates
            .iter()
            .position(|p| p.id == id)
            .ok_or(ProjectError::UnknownPlate(id))?;
        let to = to.min(self.plates.len() - 1);
        if from != to {
            let plate = self.plates.remove(from);
            self.plates.insert(to, plate);
            self.record(StructureStep::Moved { from, to });
        }
        Ok(())
    }

    /// Replace the contents of several plates at once, and add plates holding `added` at their
    /// positions (each counted after the ones before are in), as one undo step of the project's
    /// structure (TD-135): objects moved or copied between plates, to new plates too, and
    /// Arrange's overflow. Each plate takes a new revision; their own histories are kept.
    /// Returns the added plates' identities, in order.
    ///
    /// # Errors
    /// [`ProjectError::UnknownPlate`], before anything changes.
    pub fn edit_plates(
        &mut self,
        contents: Vec<(PlateId, PlateContent)>,
        added: Vec<(usize, PlateContent)>,
    ) -> Result<Vec<PlateId>, ProjectError> {
        if let Some((id, _)) = contents.iter().find(|(id, _)| self.plate(*id).is_none()) {
            return Err(ProjectError::UnknownPlate(*id));
        }
        let mut contents = StructureStep::Contents { plates: contents };
        self.replay(&mut contents, true);
        let mut steps = vec![contents];
        let mut added_ids = Vec::new();
        for (index, content) in added {
            let (id, plate) = self.new_plate(content);
            let index = index.min(self.plates.len());
            self.plates.insert(index, plate.clone());
            steps.push(StructureStep::Added { index, plate });
            added_ids.push(id);
        }
        let step = if steps.len() == 1 {
            steps.remove(0)
        } else {
            StructureStep::Several(steps)
        };
        self.record(step);
        Ok(added_ids)
    }

    /// Keep `step` as the newest of the structure's undo steps; it clears their redo steps.
    fn record(&mut self, step: StructureStep) {
        let at = self.next_revision();
        let structure = &mut self.structure;
        structure.undo.push((step, at));
        if structure.undo.len() > MAX_UNDO_STEPS {
            structure.undo.remove(0);
        }
        structure.redo.clear();
    }

    /// Apply `step` backwards (undo) or forwards, and say which plate it leaves to show. A plate
    /// it takes out is kept in the step as it is then, its own history too, so that putting it
    /// back restores it whole.
    fn replay(&mut self, step: &mut StructureStep, forwards: bool) -> Option<PlateId> {
        match (step, forwards) {
            (StructureStep::Added { index, plate }, true)
            | (StructureStep::Removed { index, plate }, false) => {
                let index = (*index).min(self.plates.len());
                self.plates.insert(index, plate.clone());
                Some(plate.id)
            }
            (StructureStep::Added { plate, .. }, false)
            | (StructureStep::Removed { plate, .. }, true) => {
                let index = self.plates.iter().position(|p| p.id == plate.id)?;
                *plate = self.plates.remove(index);
                None
            }
            (StructureStep::Contents { plates }, _) => {
                for (id, content) in plates.iter_mut() {
                    let revision = self.next_revision();
                    if let Some(plate) = self.plates.iter_mut().find(|p| p.id == *id) {
                        std::mem::swap(&mut plate.content, content);
                        plate.revision = revision;
                        plate.history.merging = None;
                    }
                }
                plates.first().map(|(id, _)| *id)
            }
            (StructureStep::Several(steps), true) => {
                let mut shown = None;
                for step in steps.iter_mut() {
                    shown = self.replay(step, true).or(shown);
                }
                shown
            }
            (StructureStep::Several(steps), false) => {
                let mut shown = None;
                for step in steps.iter_mut().rev() {
                    shown = self.replay(step, false).or(shown);
                }
                shown
            }
            (&mut StructureStep::Moved { from, to }, forwards) => {
                let (from, to) = if forwards { (from, to) } else { (to, from) };
                if from < self.plates.len() && to < self.plates.len() {
                    let moved = self.plates.remove(from);
                    let id = moved.id;
                    self.plates.insert(to, moved);
                    Some(id)
                } else {
                    None
                }
            }
        }
    }

    /// Undo the project's newest step (TD-135): the newest of plate `id`'s own steps and of the
    /// structure's. Returns what changed, or `None` with nothing to undo.
    ///
    /// # Errors
    /// [`ProjectError::UnknownPlate`].
    pub fn undo(&mut self, id: PlateId) -> Result<Option<Change>, ProjectError> {
        let plate_at = self.plate(id).ok_or(ProjectError::UnknownPlate(id))?;
        let plate_at = plate_at.history.undo.last().map(|s| s.1);
        let structure_at = self.structure.undo.last().map(|s| s.1);
        if let (Some((step, s)), true) = (
            self.structure.undo.last(),
            structure_at.is_some_and(|s| plate_at.is_none_or(|p| s > p)),
        ) {
            // A plate the step changed and edited since is undone first, newest first: the
            // step must find the contents it left.
            let newer = step
                .touched()
                .into_iter()
                .filter_map(|t| Some((t, self.plate(t)?.history.undo.last()?.1)))
                .filter(|&(_, at)| at > *s)
                .max_by_key(|&(_, at)| at);
            if let Some((other, _)) = newer {
                return Ok(self
                    .undo_plate(other)?
                    .map(|revision| Change::Plate(other, revision)));
            }
        }
        if structure_at.is_some_and(|s| plate_at.is_none_or(|p| s > p)) {
            if let Some((mut step, at)) = self.structure.undo.pop() {
                let shown = self.replay(&mut step, false);
                self.structure.redo.push((step, at));
                return Ok(Some(Change::Structure(shown)));
            }
        }
        Ok(self
            .undo_plate(id)?
            .map(|revision| Change::Plate(id, revision)))
    }

    /// Redo the project's most recently undone step (TD-135): of plate `id`'s and the
    /// structure's undone steps, the one made first. Returns what changed, or `None`.
    ///
    /// # Errors
    /// [`ProjectError::UnknownPlate`].
    pub fn redo(&mut self, id: PlateId) -> Result<Option<Change>, ProjectError> {
        let plate_at = self.plate(id).ok_or(ProjectError::UnknownPlate(id))?;
        let plate_at = plate_at.history.redo.last().map(|s| s.1);
        let structure_at = self.structure.redo.last().map(|s| s.1);
        if let (Some((step, s)), true) = (
            self.structure.redo.last(),
            structure_at.is_some_and(|s| plate_at.is_none_or(|p| s < p)),
        ) {
            // A plate the step changed with an undone edit made before it is redone first.
            let older = step
                .touched()
                .into_iter()
                .filter_map(|t| Some((t, self.plate(t)?.history.redo.last()?.1)))
                .filter(|&(_, at)| at < *s)
                .min_by_key(|&(_, at)| at);
            if let Some((other, _)) = older {
                return Ok(self
                    .redo_plate(other)?
                    .map(|revision| Change::Plate(other, revision)));
            }
        }
        if structure_at.is_some_and(|s| plate_at.is_none_or(|p| s < p)) {
            if let Some((mut step, at)) = self.structure.redo.pop() {
                let shown = self.replay(&mut step, true);
                self.structure.undo.push((step, at));
                return Ok(Some(Change::Structure(shown)));
            }
        }
        Ok(self
            .redo_plate(id)?
            .map(|revision| Change::Plate(id, revision)))
    }

    /// Whether [`Self::undo`] and [`Self::redo`] have a step for plate `id`.
    #[must_use]
    pub fn history(&self, id: PlateId) -> (bool, bool) {
        let (undo, redo) = self
            .plate(id)
            .map_or((false, false), |p| (p.can_undo(), p.can_redo()));
        (
            undo || !self.structure.undo.is_empty(),
            redo || !self.structure.redo.is_empty(),
        )
    }

    /// The plate with this identity.
    #[must_use]
    pub fn plate(&self, id: PlateId) -> Option<&Plate> {
        self.plates.iter().find(|p| p.id == id)
    }

    /// Plates in project order.
    #[must_use]
    pub fn plates(&self) -> &[Plate] {
        &self.plates
    }

    /// Remove a plate, with no undo step; results computed from it become
    /// [`Freshness::Removed`].
    pub fn remove_plate(&mut self, id: PlateId) -> Option<Plate> {
        let index = self.plates.iter().position(|p| p.id == id)?;
        Some(self.plates.remove(index))
    }

    fn plate_mut(&mut self, id: PlateId) -> Result<&mut Plate, ProjectError> {
        self.plates
            .iter_mut()
            .find(|p| p.id == id)
            .ok_or(ProjectError::UnknownPlate(id))
    }

    /// Edit a plate's content and return its new revision. Every edit counts, even one that
    /// leaves the content equal: results are recomputed rather than trusted. An edit that
    /// changes the content is one undo step; it clears the redo steps.
    ///
    /// # Errors
    /// [`ProjectError::UnknownPlate`].
    pub fn edit_plate(
        &mut self,
        id: PlateId,
        edit: impl FnOnce(&mut PlateContent),
    ) -> Result<u64, ProjectError> {
        self.edit_with(id, None, edit)
    }

    /// [`Self::edit_plate`], merging consecutive edits with the same `key` into one undo step,
    /// as a drag or a value typed digit by digit is one change for the user. Any other edit,
    /// undo or redo ends the step; a step that returns to its starting content is dropped.
    ///
    /// # Errors
    /// [`ProjectError::UnknownPlate`].
    pub fn edit_plate_merging(
        &mut self,
        id: PlateId,
        key: u64,
        edit: impl FnOnce(&mut PlateContent),
    ) -> Result<u64, ProjectError> {
        self.edit_with(id, Some(key), edit)
    }

    fn edit_with(
        &mut self,
        id: PlateId,
        key: Option<u64>,
        edit: impl FnOnce(&mut PlateContent),
    ) -> Result<u64, ProjectError> {
        self.plate_mut(id)?;
        let revision = self.next_revision();
        let plate = self.plate_mut(id)?;
        let before = plate.content.clone();
        edit(&mut plate.content);
        plate.revision = revision;
        if plate.content != before {
            let history = &mut plate.history;
            let extends = key.is_some() && history.merging == key && !history.undo.is_empty();
            if !extends {
                history.undo.push((before, revision));
                if history.undo.len() > MAX_UNDO_STEPS {
                    history.undo.remove(0);
                }
            } else if history.undo.last().map(|s| &s.0) == Some(&plate.content) {
                // The merged step came back to where it started: it is no step at all.
                history.undo.pop();
            }
            history.redo.clear();
            history.merging = key;
            let revision = plate.revision;
            // A step between plates undone before this edit would no longer find the plate as
            // it left it: it cannot be redone.
            self.structure
                .redo
                .retain(|(step, _)| !step.touched().contains(&id));
            return Ok(revision);
        }
        Ok(plate.revision)
    }

    /// Undo the last edit step of a plate, as a new revision. Returns the new revision, or
    /// `None` when there is nothing to undo.
    ///
    /// # Errors
    /// [`ProjectError::UnknownPlate`].
    pub fn undo_plate(&mut self, id: PlateId) -> Result<Option<u64>, ProjectError> {
        if !self.plate_mut(id)?.can_undo() {
            return Ok(None);
        }
        let revision = self.next_revision();
        let plate = self.plate_mut(id)?;
        let Some((previous, at)) = plate.history.undo.pop() else {
            return Ok(None);
        };
        let current = std::mem::replace(&mut plate.content, previous);
        plate.history.redo.push((current, at));
        plate.history.merging = None;
        plate.revision = revision;
        Ok(Some(plate.revision))
    }

    /// Redo the last undone step of a plate, as a new revision. Returns the new revision, or
    /// `None` when there is nothing to redo.
    ///
    /// # Errors
    /// [`ProjectError::UnknownPlate`].
    pub fn redo_plate(&mut self, id: PlateId) -> Result<Option<u64>, ProjectError> {
        if !self.plate_mut(id)?.can_redo() {
            return Ok(None);
        }
        let revision = self.next_revision();
        let plate = self.plate_mut(id)?;
        let Some((next, at)) = plate.history.redo.pop() else {
            return Ok(None);
        };
        let current = std::mem::replace(&mut plate.content, next);
        plate.history.undo.push((current, at));
        plate.history.merging = None;
        plate.revision = revision;
        Ok(Some(plate.revision))
    }

    /// Forget a plate's undo and redo steps, keeping its content: what the application set up
    /// before the user did anything (such as the profiles the last session used) is no step
    /// to undo.
    ///
    /// # Errors
    /// [`ProjectError::UnknownPlate`].
    pub fn clear_plate_history(&mut self, id: PlateId) -> Result<(), ProjectError> {
        let plate = self.plate_mut(id)?;
        plate.history.undo.clear();
        plate.history.redo.clear();
        plate.history.merging = None;
        Ok(())
    }

    /// Whether a result computed from `revision` of plate `id` still describes it.
    #[must_use]
    pub fn freshness(&self, id: PlateId, revision: u64) -> Freshness {
        match self.plate(id) {
            None => Freshness::Removed,
            Some(plate) if plate.revision == revision => Freshness::Current,
            Some(plate) => Freshness::Stale {
                current: plate.revision,
            },
        }
    }

    /// The slicing request of a plate, writing `output`, and the revision it describes.
    ///
    /// The request embeds the plate's models, profiles and documents, so it stays valid however
    /// the project changes afterwards. Every object is checked as the pipeline will check it.
    ///
    /// # Errors
    /// An unknown plate or model, an empty plate, or settings or objects the job checks refuse.
    pub fn request(
        &self,
        id: PlateId,
        output: PathBuf,
    ) -> Result<(SliceRequest, u64), ProjectError> {
        let plate = self.plate(id).ok_or(ProjectError::UnknownPlate(id))?;
        let content = &plate.content;
        if content.objects.is_empty() {
            return Err(ProjectError::EmptyPlate(id));
        }
        let printer = content
            .printer
            .as_ref()
            .ok_or(ProjectError::NoPrinter(id))?;
        content.resin.as_ref().ok_or(ProjectError::NoResin(id))?;
        let setting = content.setting().ok_or(ProjectError::NoPrintSetting(id))?;
        let resin = content
            .job_resin()
            .ok_or(ProjectError::NoPrintSetting(id))?
            .map_err(|reason| ProjectError::Invalid { plate: id, reason })?;
        let objects = (1..)
            .zip(&content.objects)
            .map(|(index, object)| {
                let model = self.model(object.model).ok_or(ProjectError::UnknownModel {
                    plate: id,
                    object: index,
                    model: object.model,
                })?;
                check_operations(object).map_err(|reason| ProjectError::Invalid {
                    plate: id,
                    reason: format!("object {index} ({:?}) {reason}", object.name),
                })?;
                Ok(object_spec(object, model))
            })
            .collect::<Result<Vec<_>, ProjectError>>()?;
        let settings = &content.settings;
        let (antialias, min_gray) = settings.image.resolve(&setting.image);
        let invalid = |reason: String| ProjectError::Invalid { plate: id, reason };
        let request = SliceRequest {
            subject: Subject::Plate(PlateSpec {
                name: content.name.clone(),
                spacing_mm: settings.spacing_mm,
                objects,
            }),
            units: None,
            legacy_stl: false,
            printer: Input::Loaded {
                name: printer.name.clone(),
                value: printer.clone(),
            },
            resin: Input::Loaded {
                name: resin.name.clone(),
                value: resin.clone(),
            },
            format: settings.format.clone(),
            output,
            translate_mm: [0.; 3],
            pose: Pose::default(),
            job_id: settings.job_id.clone(),
            allow_topology_defects: settings.allow_topology_defects,
            fill: settings.fill,
            antialias,
            min_gray,
            antialias_supports: settings.image.antialias_supports,
            xy_compensation_px: setting.image.xy_compensation_px,
            bottom_xy_compensation_px: setting.image.bottom_xy_compensation_px,
            close_holes_mm: settings.close_holes_mm,
            reversed_tolerance_mm2: settings.reversed_tolerance_mm2,
            repair: settings.repair,
            hollow_mm: None,
            hollow_cell_mm: None,
            drain_holes: Vec::new(),
            hole_coordinates: Coordinates::Model,
            hollow_simplify: None,
            hollow_blockers: Vec::new(),
            supports: None,
            support_coordinates: Coordinates::Model,
            accept_support_collisions: false,
            layer_plan: settings.layer_plan.clone().map(|value| Input::Loaded {
                name: format!("{} layer plan", content.name),
                value,
            }),
            copies: 1,
            arrange: false,
            spacing_mm: settings.spacing_mm,
            memory_budget_bytes: None,
            verify_walls: false,
            find_unsupported: None,
            find_suction_cups: None,
            // Owner, 2026-10-27: supports with errors print, with a warning (TD-108).
            slice_support_errors: true,
            allow_floating: false,
        }
        .checked()
        .map_err(invalid)?;
        if let Subject::Plate(spec) = &request.subject {
            for (index, object) in (1..).zip(&spec.objects) {
                request
                    .for_object(object)
                    .checked()
                    .map_err(|e| invalid(format!("object {index} ({:?}): {e}", object.name)))?;
            }
        }
        Ok((request, plate.revision))
    }
}

/// Refuse an object repaired more than once or after another operation, or hollowed more than
/// once; the reason follows the object's name.
fn check_operations(object: &PlateObject) -> Result<(), String> {
    let enabled = || object.operations.iter().filter(|o| o.enabled);
    let hollowings = enabled()
        .filter(|o| matches!(o.kind, OperationKind::Hollow(_)))
        .count();
    let repairs = enabled()
        .filter(|o| matches!(o.kind, OperationKind::Repair(_)))
        .count();
    let repaired_first = enabled()
        .next()
        .is_some_and(|o| matches!(o.kind, OperationKind::Repair(_)));
    if repairs > 1 || (repairs == 1 && !repaired_first) {
        return Err(format!(
            "has {repairs} repair operations; it can be repaired once, before its other operations"
        ));
    }
    if hollowings > 1 {
        return Err(format!(
            "is hollowed {hollowings} times; it can be hollowed once"
        ));
    }
    Ok(())
}

/// The request form of one object, over the library model's bytes.
fn object_spec(object: &PlateObject, model: &LibraryModel) -> ObjectSpec {
    let hollow = object.hollowing();
    ObjectSpec {
        name: object.name.clone(),
        model: ModelSource::Embedded {
            name: model.name.clone(),
            bytes: Arc::clone(&model.bytes),
        },
        units: object.units,
        translate_mm: object.translate_mm,
        pose: object.pose.clone(),
        hollow_mm: hollow.map(|h| h.wall_mm),
        hollow_cell_mm: hollow.and_then(|h| h.cell_mm),
        drain_holes: object.drain_holes().copied().collect(),
        hole_coordinates: Coordinates::Model,
        hollow_simplify: hollow.and_then(|h| h.simplify),
        hollow_blockers: object.blockers().copied().collect(),
        supports: object.supports.clone().map(|value| Input::Loaded {
            name: format!("{} supports", object.name),
            value,
        }),
        support_coordinates: Coordinates::Model,
        accept_support_collisions: object.accept_support_collisions,
        copies: object.copies,
        arrange: object.arrange,
        repair: object.repair(),
    }
}
