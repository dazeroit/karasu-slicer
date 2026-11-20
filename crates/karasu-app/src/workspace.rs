//! The open project and what the viewport shows of it (M6.2, TD-073).
//!
//! **Opening files:** models and profiles are opened by dropping them on the window or naming
//! them on the command line.
//! - A `karasu` printer or resin profile (JSON) becomes the plate's printer or resin.
//! - Any other file is imported as a model on a background thread, then embedded in the
//!   project's library and placed on the plate.
//! - A model file that declares no unit (STL) is read in millimetres, and a message says so.
//! - Every imported model is then diagnosed on another background thread
//!   ([`karasu_project::diagnosis`]), so a large model appears before its diagnosis ends; a
//!   message reports what was found and what the plate's settings do about it. The model itself
//!   is never changed.
//!
//! **Placement:** a new object is centred on the plate (when a printer is selected) and dropped
//! onto it. Placements are computed with the pipeline's own [`placement::place`], so the viewport
//! shows objects exactly where they will print. They are recomputed only when the plate's
//! revision changes.
use crate::{
    camera::{Mat4, Ray},
    gizmo::{ray_box, ray_mesh_where, world_normal, Affine},
    i18n::tr,
    messages::{Level, Message},
    viewport::{Draw, LabelVertex, LineVertex, Section},
    visibility::Visibility,
};
use karasu_core::{
    cancellation::Cancellation,
    mesh::{formats::LengthUnit, TriangleMesh},
    plate::{arrange, convex_hull, overlapping_pairs, Rect},
    preparation::repair::{RepairPreset, RepairSpec},
    profiles::{store::ProfileDocument, PrinterProfile, MAX_PROFILE_BYTES},
    supports::{BaseSpec, SupportDocument},
};
use karasu_project::{
    diagnosis::{diagnose, Diagnosis, Treatment},
    pipeline::{import_mesh, ImportError, PipelineCache},
    placement::{self, Placement},
    project::{
        Hollowing, ModelId, OperationKind, PlateContent, PlateId, PlateObject, PlateSettings,
        Project,
    },
    request::{Blocker, DrainHole, ModelSource, Pose, DEFAULT_REVERSED_TOLERANCE_MM2},
};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{mpsc, Arc},
    thread,
};

pub use crate::arranging::{
    ArrangeInput, ArrangeJob, ArrangeOptions, ArrangeScope, ArrangeStage, Arranged, LeftOut,
    Planned, PlateChoice,
};

/// Largest model file read, matching the core importers' input limit.
fn model_limit() -> usize {
    karasu_core::mesh::stl::ImportLimits::default().max_input_bytes
}

/// A model read and imported on a background thread.
struct Imported {
    name: String,
    bytes: Arc<[u8]>,
    mesh: TriangleMesh,
    /// The unit given because the file declares none.
    assumed: Option<LengthUnit>,
    /// Facets the import left out for a non-finite coordinate (TD-092 r9).
    dropped_facets: usize,
    /// The mesh's surface area, in mm², which predicts its cavity (TD-094).
    area_mm2: f64,
}

/// A background diagnosis of an opened model.
struct Diagnosing {
    name: String,
    model: ModelId,
    /// The repairs the new object gets: its own (the auto-repair preset) or the plate's.
    repair: RepairSpec,
    /// Said about the model besides the diagnosis (an assumed unit, the auto-repair).
    note: String,
    result: mpsc::Receiver<Result<Diagnosis, String>>,
}

/// A background import.
struct Pending {
    name: String,
    /// A file opened: a new object of the plate; otherwise a model of the project opened, whose
    /// objects are already there (TD-130).
    adds_object: bool,
    result: mpsc::Receiver<Result<Imported, String>>,
}

/// A library model's mesh, ready to draw.
pub struct LoadedMesh {
    /// The imported mesh, in millimetres.
    pub mesh: Arc<TriangleMesh>,
    /// The origin its GPU vertices are relative to: the source bounding-box centre.
    pub origin: [f64; 3],
    /// The source bounding box's size along the model's own axes, in mm: measured once, so the
    /// Transform tool's sizes never scan the mesh (issue #40).
    pub size_mm: [f64; 3],
    /// The viewport's key for it.
    pub key: u64,
    /// Its surface area, in mm², which predicts its cavity (TD-094).
    pub area_mm2: f64,
}

/// Where a ray meets an object's surface, in world coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SurfaceHit {
    /// The plate object.
    pub object: usize,
    /// The point on the surface, in mm.
    pub point: [f64; 3],
    /// The surface's outward unit normal there.
    pub normal: [f64; 3],
}

/// The overlapping footprints of a plate (its pairs of object indices) at a revision.
type Overlaps = ((PlateId, u64), Vec<(usize, usize)>);

/// The project being edited, the plate shown, and the meshes and placements to draw it.
pub struct Workspace {
    project: Project,
    plate: PlateId,
    meshes: HashMap<ModelId, LoadedMesh>,
    pending: Vec<Pending>,
    diagnosing: Vec<Diagnosing>,
    /// Placements of the plate's objects, and the revision they were computed for.
    placements: (u64, Vec<Result<Placement, String>>),
    notices: Vec<Message>,
    /// Profile files opened, as `(name, bytes)`, for the profile library to import (TD-106).
    profile_files: Vec<(String, Vec<u8>)>,
    /// Cavities and imported models shared by previews and slicing (TD-089).
    pipeline_cache: Arc<PipelineCache>,
    /// The repairs every opened model gets as its own (TD-092 r8), or `None` to diagnose only.
    auto_repair: Option<RepairPreset>,
    /// Each library model's diagnosis as imported.
    diagnoses: HashMap<ModelId, Diagnosis>,
    /// The memory hollowing and hole indexes may request (TD-094), or `None` for the
    /// pipeline's default.
    memory_budget: Option<u64>,
    /// Which objects the scene hides: a state of the view, not of the plate.
    visibility: Visibility,
    /// The overlapping footprints of a plate at a revision, worked out when it changed.
    overlaps: Option<Overlaps>,
}

impl Default for Workspace {
    fn default() -> Self {
        Self::new()
    }
}

/// The viewport key of a model: the first bytes of its identity.
fn mesh_key(id: ModelId) -> u64 {
    let mut bytes = [0; 8];
    bytes.copy_from_slice(&id.0[..8]);
    u64::from_le_bytes(bytes)
}

/// Read a profile document's bytes, checked to be one.
fn read_profile(path: &Path) -> Result<Vec<u8>, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    if bytes.len() > MAX_PROFILE_BYTES {
        return Err(format!(
            "{} exceeds the {MAX_PROFILE_BYTES}-byte profile limit",
            path.display()
        ));
    }
    ProfileDocument::from_json(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(bytes)
}

/// Read and import a model file, in millimetres when it declares no unit.
/// Share of the installed memory one preparation may use (TD-094).
const MEMORY_SHARE: f64 = 0.5;
/// The application's measured peak before a cavity, and per cavity triangle (TD-094, measured
/// in release on `Base_01.stl` hollowed and drilled at 0.5, 0.35 and 0.3 mm: 3.2, 5.5 and
/// 7.0 GB for 6.4, 10.6 and 15.5 M cavity triangles).
const BASE_BYTES: f64 = 0.75 * 1_073_741_824.0;
const BYTES_PER_CAVITY_TRIANGLE: f64 = 410.0;
/// The fewest cavity triangles a budget allows, however little memory is installed.
const MIN_CAVITY_TRIANGLES: f64 = 1e6;

/// The memory budget of this machine (TD-094): the hole-index payload of the largest cavity
/// whose whole preparation is predicted to fit in [`MEMORY_SHARE`] of the installed memory.
/// `None`, for the pipeline's default, when the memory cannot be read.
fn machine_memory_budget() -> Option<u64> {
    use sysinfo::{MemoryRefreshKind, RefreshKind, System};
    let system = System::new_with_specifics(
        RefreshKind::nothing().with_memory(MemoryRefreshKind::nothing().with_ram()),
    );
    #[allow(clippy::cast_precision_loss)] // Bytes far below 2^52 matter only to a few digits.
    let total = system.total_memory() as f64;
    if total <= 0.0 {
        return None;
    }
    let triangles =
        ((total * MEMORY_SHARE - BASE_BYTES) / BYTES_PER_CAVITY_TRIANGLE).max(MIN_CAVITY_TRIANGLES);
    log::info!(
        target: "session",
        "installed memory {:.1} GiB: up to {:.1} M cavity triangles",
        total / 1_073_741_824.0,
        triangles / 1e6
    );
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )] // Positive, and far below u64::MAX.
    Some((triangles * karasu_core::preparation::sampled::ProbeLimits::BYTES_PER_FACE as f64) as u64)
}

fn import(path: &Path) -> Result<Imported, String> {
    let name = path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    let bytes = std::fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    if bytes.len() > model_limit() {
        return Err(format!(
            "{name} exceeds the {}-byte import limit",
            model_limit()
        ));
    }
    import_bytes(name, bytes.into(), None)
}

/// Import a model's bytes, named `name` (its extension selects the format), in `units` when
/// given; a format that needs a unit and gets none is read in millimetres, and said.
fn import_bytes(
    name: String,
    bytes: Arc<[u8]>,
    units: Option<LengthUnit>,
) -> Result<Imported, String> {
    let source = ModelSource::Embedded {
        name: name.clone(),
        bytes: Arc::clone(&bytes),
    };
    let never = |_| false;
    let (imported, assumed) = match import_mesh(&source, units, &never) {
        Err(ImportError::UnitsRequired) => {
            let unit = LengthUnit::Millimeter;
            (import_mesh(&source, Some(unit), &never), Some(unit))
        }
        other => (other, None),
    };
    let imported = imported.map_err(|e| e.to_string())?;
    Ok(Imported {
        name,
        bytes,
        dropped_facets: imported.dropped.map_or(0, |d| d.facets),
        // Computed here, off the interface thread.
        area_mm2: karasu_core::preparation::sampled::surface_area(&imported.mesh),
        mesh: imported.mesh,
        assumed,
    })
}

impl Workspace {
    /// A project with one empty plate, without printer or resin.
    #[must_use]
    pub fn new() -> Self {
        let mut project = Project::new(tr!("project-untitled"));
        let plate = project.add_plate(PlateContent {
            name: tr!("plate-default-name", number = 1usize),
            printer: None,
            resin: None,
            print_setting: None,
            settings: PlateSettings {
                // The owner's choice for the application (TD-079): tolerate the reversed
                // slivers of self-intersecting folds; the command line has no default.
                reversed_tolerance_mm2: Some(DEFAULT_REVERSED_TOLERANCE_MM2),
                ..PlateSettings::new("goo-v3")
            },
            objects: Vec::new(),
        });
        Self {
            project,
            plate,
            meshes: HashMap::new(),
            pending: Vec::new(),
            diagnosing: Vec::new(),
            placements: (0, Vec::new()),
            notices: Vec::new(),
            profile_files: Vec::new(),
            pipeline_cache: Arc::new(PipelineCache::default()),
            // The owner's choice (TD-092): exact repairs, which move no vertex, on import.
            auto_repair: Some(RepairPreset::Exact),
            diagnoses: HashMap::new(),
            memory_budget: machine_memory_budget(),
            visibility: Visibility::default(),
            overlaps: None,
        }
    }

    /// The repairs every opened model gets, or `None` to diagnose only.
    #[must_use]
    pub fn auto_repair(&self) -> Option<RepairPreset> {
        self.auto_repair
    }

    /// Set the repairs models opened from now on get.
    pub fn set_auto_repair(&mut self, preset: Option<RepairPreset>) {
        self.auto_repair = preset;
    }

    /// Library model `model`'s diagnosis as imported, once it is known.
    #[must_use]
    pub fn diagnosis(&self, model: ModelId) -> Option<&Diagnosis> {
        self.diagnoses.get(&model)
    }

    /// Give object `index` its own repairs, or return it to the plate's with `None`, as one
    /// plate edit (TD-092: the repair is the first operation of its timeline).
    pub fn set_repair(&mut self, index: usize, repair: Option<RepairSpec>) {
        self.edit(|plate| {
            if let Some(object) = plate.objects.get_mut(index) {
                object.set_repair(repair);
            }
        });
    }

    /// The cavities and imported models shared by previews and slicing, so neither imports,
    /// repairs or hollows a model again while what it depends on is unchanged (TD-089).
    #[must_use]
    pub fn pipeline_cache(&self) -> Arc<PipelineCache> {
        Arc::clone(&self.pipeline_cache)
    }

    /// Whether object `index` is hidden from the scene (still printed).
    #[must_use]
    pub fn is_hidden(&self, index: usize) -> bool {
        self.plate()
            .objects
            .get(index)
            .is_some_and(|object| self.visibility.is_hidden(index, object.model))
    }

    /// The objects' visibility, following the plate's objects as they are now.
    pub fn visibility(&mut self) -> &mut Visibility {
        let models: Vec<ModelId> = self.plate().objects.iter().map(|o| o.model).collect();
        self.visibility.sync(&models);
        &mut self.visibility
    }

    /// The object shown alone, while one is isolated.
    #[must_use]
    pub fn isolated(&self) -> Option<usize> {
        self.visibility.isolated()
    }

    /// How many of the plate's objects the scene hides.
    #[must_use]
    pub fn hidden_count(&self) -> usize {
        (0..self.plate().objects.len())
            .filter(|&index| self.is_hidden(index))
            .count()
    }

    /// The plate shown.
    #[must_use]
    pub fn plate(&self) -> &PlateContent {
        self.project
            .plate(self.plate)
            .map(karasu_project::project::Plate::content)
            .expect("the workspace's plate is never removed")
    }

    /// The slicing request of the plate, writing `output`, and the revision it describes.
    ///
    /// # Errors
    /// The project's refusal: no objects, printer or resin, or invalid settings or objects.
    pub fn request(
        &self,
        output: PathBuf,
    ) -> Result<(karasu_project::request::SliceRequest, u64), karasu_project::project::ProjectError>
    {
        let (mut request, revision) = self.project.request(self.plate, output)?;
        request.memory_budget_bytes = self.memory_budget;
        Ok((request, revision))
    }

    /// The slice request of plate `id`, shown or not (TD-135, phase 4), with the plate's
    /// revision it describes.
    ///
    /// # Errors
    /// The project's refusal: no such plate, or a plate that cannot be sliced as it is.
    pub fn request_of(
        &self,
        id: PlateId,
        output: PathBuf,
    ) -> Result<(karasu_project::request::SliceRequest, u64), karasu_project::project::ProjectError>
    {
        let (mut request, revision) = self.project.request(id, output)?;
        request.memory_budget_bytes = self.memory_budget;
        Ok((request, revision))
    }

    /// Set the memory hollowing and hole indexes may request in previews and slicing (TD-094).
    #[cfg_attr(not(test), allow(dead_code))] // Tests and a future settings override.
    pub fn set_memory_budget(&mut self, bytes: Option<u64>) {
        self.memory_budget = bytes;
    }

    /// The memory hollowing and hole indexes may request, or `None` for the pipeline's default.
    #[must_use]
    pub fn memory_budget(&self) -> Option<u64> {
        self.memory_budget
    }

    /// Whether a result computed from `revision` still describes the plate.
    #[must_use]
    pub fn freshness(&self, revision: u64) -> karasu_project::project::Freshness {
        self.project.freshness(self.plate, revision)
    }

    /// The plate's current revision.
    #[must_use]
    pub fn revision(&self) -> u64 {
        self.project
            .plate(self.plate)
            .map_or(0, karasu_project::project::Plate::revision)
    }

    /// The plate's identity.
    #[must_use]
    pub fn plate_id(&self) -> PlateId {
        self.plate
    }

    /// The plate's printer, if one is selected.
    #[must_use]
    pub fn printer(&self) -> Option<&PrinterProfile> {
        self.plate().printer.as_ref()
    }

    /// Messages for the user, oldest first.
    #[must_use]
    pub fn notices(&self) -> &[Message] {
        &self.notices
    }

    /// Names of the models still being imported.
    pub fn importing(&self) -> impl Iterator<Item = &str> {
        self.pending.iter().map(|p| p.name.as_str())
    }

    /// Open a dropped or named file: a profile, or a model imported in the background.
    pub fn open(&mut self, path: PathBuf) {
        log::info!(target: "models", "opening {}", path.display());
        let is_json = path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("json"));
        if is_json {
            match read_profile(&path) {
                Ok(bytes) => self.profile_files.push((path.display().to_string(), bytes)),
                Err(text) => self.notices.push(
                    Message::new(Level::Error, tr!("notice-cannot-open-profile")).detail(text),
                ),
            }
            return;
        }
        let (sender, result) = mpsc::channel();
        let name = path.display().to_string();
        thread::spawn(move || {
            // The receiver is gone only if the workspace was dropped.
            let _ = sender.send(import(&path));
        });
        self.pending.push(Pending {
            name,
            adds_object: true,
            result,
        });
    }

    /// A workspace on `project`, as it was opened (TD-130): its first plate (an empty one when
    /// it has none), its models imported again in the background, in the units their objects
    /// give; their objects are already on the plate.
    #[must_use]
    pub fn with_project(mut project: Project) -> Self {
        let mut workspace = Self::new();
        let plate = match project.plates().first() {
            Some(plate) => plate.id(),
            None => project.add_plate(workspace.plate().clone()),
        };
        workspace.project = project;
        workspace.plate = plate;
        let models: Vec<_> = workspace.project.models().to_vec();
        for model in models {
            let units = workspace
                .project
                .plates()
                .iter()
                .flat_map(|p| &p.content().objects)
                .find(|o| o.model == model.id)
                .and_then(|o| o.units);
            let (sender, result) = mpsc::channel();
            let name = model.name.clone();
            thread::spawn(move || {
                // The receiver is gone only if the workspace was dropped.
                let _ = sender.send(import_bytes(model.name, model.bytes, units));
            });
            workspace.pending.push(Pending {
                name,
                adds_object: false,
                result,
            });
        }
        workspace
    }

    /// The project the workspace edits.
    #[must_use]
    pub fn project(&self) -> &Project {
        &self.project
    }

    /// Name the project, as it is saved; not a plate edit.
    pub fn set_project_name(&mut self, name: String) {
        self.project.name = name;
    }

    /// Whether the project's state is `state` (see [`Self::state`]), compared without
    /// allocating: it is asked every frame.
    #[must_use]
    pub fn is_state(&self, state: &[(PlateId, u64)]) -> bool {
        let plates = self.project.plates();
        plates.len() == state.len()
            && plates
                .iter()
                .zip(state)
                .all(|(p, &(id, revision))| p.id() == id && p.revision() == revision)
    }

    /// Every plate's identity and revision: the project's state, to tell whether it changed
    /// since it was saved. Revisions only grow, so any edit since, undone or not, counts.
    #[must_use]
    pub fn state(&self) -> Vec<(PlateId, u64)> {
        self.project
            .plates()
            .iter()
            .map(|p| (p.id(), p.revision()))
            .collect()
    }

    /// The profile files opened since the last call, for the profile library to import.
    pub fn take_profile_files(&mut self) -> Vec<(String, Vec<u8>)> {
        std::mem::take(&mut self.profile_files)
    }

    /// Forget the plate's undo and redo steps: what the window set up as it opened (the last
    /// session's profiles) is no step for the user to undo.
    pub fn forget_history(&mut self) {
        // The workspace's plate always exists.
        let _ = self.project.clear_plate_history(self.plate);
    }

    /// Edit the plate; its revision advances.
    pub fn edit(&mut self, edit: impl FnOnce(&mut PlateContent)) {
        self.apply(None, edit);
    }

    /// Edit the plate, merging consecutive edits with the same `key` into one undo step (a
    /// drag, or a value dragged in a field).
    pub fn edit_merging(&mut self, key: u64, edit: impl FnOnce(&mut PlateContent)) {
        self.apply(Some(key), edit);
    }

    /// Apply an edit. Drain holes are stored in their objects' model frames (TD-089), so they
    /// follow every move without being touched.
    fn apply(&mut self, key: Option<u64>, edit: impl FnOnce(&mut PlateContent)) {
        let mut after = self.plate().clone();
        edit(&mut after);
        let new = self.place_all(&after);
        // The workspace's plate always exists, so the edit cannot fail.
        let revision = match key {
            Some(key) => self
                .project
                .edit_plate_merging(self.plate, key, |c| *c = after),
            None => self.project.edit_plate(self.plate, |c| *c = after),
        };
        // The placements just computed describe the new revision: a drag places once a frame.
        if let Ok(revision) = revision {
            self.placements = (revision, new);
        }
    }

    /// Set object `index`'s translation and pose (a handle drag), merging with `key`. Only the
    /// placement changes: the object's drain holes, in its model frame, go along with it.
    pub fn place_object(&mut self, index: usize, translate_mm: [f64; 3], pose: Pose, key: u64) {
        self.edit_merging(key, |plate| {
            if let Some(object) = plate.objects.get_mut(index) {
                object.translate_mm = translate_mm;
                object.pose = pose;
            }
        });
    }

    /// Undo the project's newest step: the plate's own, or a change of the plates themselves,
    /// whichever is newer (TD-135). A plate it brings back is shown; one it takes away leaves
    /// another shown. Returns whether there was a step.
    pub fn undo(&mut self) -> bool {
        let change = self.project.undo(self.plate);
        self.follow(&change)
    }

    /// Redo the project's most recently undone step (TD-135), as [`Self::undo`] does.
    pub fn redo(&mut self) -> bool {
        let change = self.project.redo(self.plate);
        self.follow(&change)
    }

    /// Show what an undo or redo `change` leaves to show; whether it changed anything.
    fn follow(
        &mut self,
        change: &Result<
            Option<karasu_project::project::Change>,
            karasu_project::project::ProjectError,
        >,
    ) -> bool {
        use karasu_project::project::Change;
        match change {
            Ok(Some(Change::Structure(shown))) => {
                match *shown {
                    Some(id) => self.plate = id,
                    None => self.keep_a_plate(),
                }
                true
            }
            // A plate edited after a step between plates is undone first: it is shown.
            Ok(Some(Change::Plate(id, _))) => {
                self.plate = *id;
                true
            }
            _ => false,
        }
    }

    /// Whether the project has a step to undo, and an undone step to redo, from this plate.
    #[must_use]
    pub fn history(&self) -> (bool, bool) {
        self.project.history(self.plate)
    }

    /// The project's plates, in order (TD-135).
    #[must_use]
    pub fn plates(&self) -> &[karasu_project::project::Plate] {
        self.project.plates()
    }

    /// The shown plate's position among the project's plates.
    #[must_use]
    pub fn plate_index(&self) -> usize {
        self.project
            .plates()
            .iter()
            .position(|p| p.id() == self.plate)
            .unwrap_or(0)
    }

    /// Show plate `id`; an unknown plate leaves the shown one. Returns whether it changed.
    pub fn show_plate(&mut self, id: PlateId) -> bool {
        if id == self.plate || self.project.plate(id).is_none() {
            return false;
        }
        self.plate = id;
        true
    }

    /// Show the plate `step` places after the shown one in project order (negative: before),
    /// wrapping round. Returns whether it changed.
    pub fn step_plate(&mut self, step: isize) -> bool {
        let plates = self.project.plates();
        let count = plates.len().cast_signed();
        if count < 2 {
            return false;
        }
        let at = (self.plate_index().cast_signed() + step).rem_euclid(count);
        let id = plates[at.cast_unsigned()].id();
        self.show_plate(id)
    }

    /// Add a plate after the shown one, with its printer, resin, print setting and settings
    /// and no model, named `name`, and show it: one undoable step (TD-135).
    pub fn new_plate(&mut self, name: String) {
        let content = PlateContent {
            name,
            objects: Vec::new(),
            ..self.plate().clone()
        };
        let at = self.plate_index() + 1;
        self.plate = self.project.insert_plate(at, content);
    }

    /// Add a copy of plate `id` after it, its models and their preparation too, named `name`,
    /// and show it: one undoable step (TD-135). An unknown plate adds nothing.
    pub fn duplicate_plate(&mut self, id: PlateId, name: String) {
        let Some(at) = self.project.plates().iter().position(|p| p.id() == id) else {
            return;
        };
        let content = PlateContent {
            name,
            ..self.project.plates()[at].content().clone()
        };
        self.plate = self.project.insert_plate(at + 1, content);
    }

    /// Name the shown plate: an edit of it.
    pub fn rename_plate(&mut self, name: String) {
        self.edit(|plate| plate.name = name);
    }

    /// Delete plate `id`: one undoable step (TD-135). Deleting the shown plate shows the one
    /// after it (or before, for the last); another plate leaves the shown one. The last plate
    /// of the project stays; returns whether it went.
    pub fn delete_plate(&mut self, id: PlateId) -> bool {
        let plates = self.project.plates();
        let Some(index) = plates.iter().position(|p| p.id() == id) else {
            return false;
        };
        if plates.len() < 2 {
            return false;
        }
        let next = plates[if index + 1 < plates.len() {
            index + 1
        } else {
            index - 1
        }]
        .id();
        let gone = self.project.delete_plate(id).is_ok();
        if gone && id == self.plate {
            self.plate = next;
        }
        gone
    }

    /// Move plate `id` to position `to` among the plates: one undoable step (TD-135).
    pub fn move_plate(&mut self, id: PlateId, to: usize) {
        let _ = self.project.move_plate(id, to);
    }

    /// Move object `index` of the shown plate to plate `to` (a new plate after the shown one,
    /// named `name`, with its printer, resin, print setting and settings, for `None`), or copy
    /// it there with `copy`: one undoable step of the project (TD-135, phase 3). The object goes
    /// whole: its source, transform and preparation (repair, hollowing, drain holes, blockers,
    /// supports, protected points, copies).
    ///
    /// It lands where it is when its footprint (the model with its supports and raft) lies on
    /// the target's plate clear of the models there by the target's spacing, otherwise beside
    /// them as the packer places it (centred on an empty plate). With no room for it, it lands
    /// in the middle of the target's plate all the same, over the models there or beyond the
    /// build volume, for the user to place (owner, 2026-11-19, TD-163). The shown plate stays.
    ///
    /// # Errors
    /// [`Refused`]: no such object or plate, or a model still loading, with nothing changed.
    pub fn transfer(
        &mut self,
        index: usize,
        to: Option<PlateId>,
        copy: bool,
        name: String,
    ) -> Result<Landing, Refused> {
        let source = self.plate().clone();
        let object = source.objects.get(index).cloned().ok_or(Refused::Unknown)?;
        if to == Some(self.plate) {
            return Err(Refused::Unknown);
        }
        let mut target = match to {
            Some(id) => self
                .project
                .plate(id)
                .ok_or(Refused::Unknown)?
                .content()
                .clone(),
            None => PlateContent {
                name,
                objects: Vec::new(),
                ..source.clone()
            },
        };
        let mut placed = object;
        let landing = match &target.printer {
            // Without a printer there is no plate to fit on: the object keeps its place.
            None => Landing::Kept,
            Some(printer) => {
                let centre = placement::plate_centre(printer);
                let footprint = |o: &PlateObject| self.footprint(centre, o).map(|(rect, _)| rect);
                let (rect, translation) =
                    self.footprint(centre, &placed).ok_or(Refused::Loading)?;
                let others = target
                    .objects
                    .iter()
                    .map(footprint)
                    .collect::<Option<Vec<_>>>()
                    .ok_or(Refused::Loading)?;
                let [ox, oy] = printer.origin_xy_mm;
                let [width, depth, _] = printer.build_size_mm;
                let area = Rect {
                    min: [ox, oy],
                    max: [ox + width, oy + depth],
                };
                let spacing = target.settings.spacing_mm;
                let on_plate =
                    (0..2).all(|k| rect.min[k] >= area.min[k] && rect.max[k] <= area.max[k]);
                if on_plate && others.iter().all(|o| !rect.within(o, spacing)) {
                    Landing::Kept
                } else {
                    let (corner, landing) = arrange(&[rect.size()], area, &others, spacing)
                        .map_or_else(
                            |_| (middle_corner(area, rect.size()), Landing::Middle),
                            |corners| (corners[0], Landing::Beside),
                        );
                    // The place it was given, no longer centred on the plate.
                    placed.translate_mm[0] = translation[0] + corner[0] - rect.min[0];
                    placed.translate_mm[1] = translation[1] + corner[1] - rect.min[1];
                    placed.pose.center = false;
                    landing
                }
            }
        };
        target.objects.push(placed);
        let mut contents = Vec::new();
        if !copy {
            let mut left = source;
            left.objects.remove(index);
            contents.push((self.plate, left));
        }
        let added = match to {
            Some(id) => {
                contents.push((id, target));
                Vec::new()
            }
            None => vec![(self.plate_index() + 1, target)],
        };
        self.project
            .edit_plates(contents, added)
            .map_err(|_| Refused::Unknown)?;
        Ok(landing)
    }

    /// The plates Arrange works on with `scope` and `choice` (TD-162), in the project's order,
    /// and those "Every plate" leaves out with why. "This plate" takes the plate shown.
    #[must_use]
    pub fn arranged_plates(
        &self,
        scope: ArrangeScope,
        choice: PlateChoice,
    ) -> (Vec<PlateId>, Vec<(PlateId, LeftOut)>) {
        if scope == ArrangeScope::ThisPlate {
            return (vec![self.plate], Vec::new());
        }
        let printer = self.plate().printer.as_ref().map(|p| p.id.clone());
        let (mut taken, mut left) = (Vec::new(), Vec::new());
        for plate in self.project.plates() {
            let id = plate.id();
            match (&plate.content().printer, &printer) {
                (None, _) => left.push((id, LeftOut::NoPrinter)),
                (Some(p), Some(shown)) if choice == PlateChoice::SamePrinter && &p.id != shown => {
                    left.push((id, LeftOut::AnotherPrinter));
                }
                _ => taken.push(id),
            }
        }
        (taken, left)
    }

    /// What Arrange works from with `options`, taken now: the plates arranged (the plate shown,
    /// or every plate `options` takes), their models' meshes (shared, not copied), the plates'
    /// names and order, and each plate's revision, so the planning can run on another thread.
    ///
    /// # Errors
    /// [`Unarranged`]: the plate shown has no printer, or a model is still loading.
    pub fn arrange_input(&mut self, options: ArrangeOptions) -> Result<ArrangeInput, Unarranged> {
        if self.plate().printer.is_none() {
            return Err(Unarranged::NoPrinter);
        }
        let (ids, _) = self.arranged_plates(options.scope, options.plates);
        let mut plates = Vec::with_capacity(ids.len());
        let mut meshes = HashMap::new();
        for &id in &ids {
            let plate = self.project.plate(id).ok_or(Unarranged::Changed)?;
            for object in &plate.content().objects {
                let mesh = self.meshes.get(&object.model).ok_or(Unarranged::Loading)?;
                meshes.insert(object.model, Arc::clone(&mesh.mesh));
            }
            plates.push((id, plate.content().clone(), plate.revision()));
        }
        let shown = ids
            .iter()
            .position(|&id| id == self.plate)
            .ok_or(Unarranged::Changed)?;
        let order: Vec<PlateId> = self
            .project
            .plates()
            .iter()
            .map(karasu_project::project::Plate::id)
            .collect();
        // Plates added go after the last plate arranged.
        let at = ids
            .iter()
            .filter_map(|id| order.iter().position(|o| o == id))
            .max()
            .map_or(order.len(), |last| last + 1);
        Ok(ArrangeInput {
            options,
            plates,
            shown,
            meshes,
            taken: self
                .project
                .plates()
                .iter()
                .map(|p| p.content().name.clone())
                .collect(),
            order,
            at,
        })
    }

    /// Make the arrangement `planned`, as one undoable step of the project's structure: the
    /// plates arranged take their new contents, plates are added, and those left empty go when
    /// asked. Only while the project's plates and the revisions of those arranged are as they
    /// were when the planning started; a plate shown that goes leaves another shown.
    ///
    /// # Errors
    /// [`Unarranged::Changed`] when a plate changed meanwhile, with nothing changed.
    pub fn finish_arrange(&mut self, planned: Planned) -> Result<Arranged, Unarranged> {
        let order: Vec<PlateId> = self
            .project
            .plates()
            .iter()
            .map(karasu_project::project::Plate::id)
            .collect();
        let unchanged = order == planned.order
            && planned.revisions.iter().all(|&(id, revision)| {
                self.project
                    .plate(id)
                    .map(karasu_project::project::Plate::revision)
                    == Some(revision)
            });
        if !unchanged {
            return Err(Unarranged::Changed);
        }
        self.project
            .rearrange_plates(planned.contents, planned.added, &planned.removed)
            .map_err(|_| Unarranged::Changed)?;
        self.keep_a_plate();
        Ok(planned.out)
    }

    /// Add `copies` copies of object `index` of the plate shown to plate `to`, the plate shown
    /// itself included, or to a new plate after the plate shown named `name` (`None`), with its
    /// profiles and settings (owner, 2026-11-10). Arranged, the copies are placed from the
    /// plate's middle outwards around the models already there, which stay where they are, the
    /// plate's spacing apart; on the model's own plate the model is arranged with them, the
    /// first in the middle. Those that do not fit go to new plates after it, named after it
    /// ("Arms (2)") and arranged the same way. Not arranged, every copy goes to the middle of
    /// the plate, its footprint (supports and raft included) centred there; so do they, arranged
    /// or not, when the model fits not even an empty plate (TD-163), and [`Cloned::too_large`]
    /// says so. Without a printer there is no plate to place on: the copies keep the model's
    /// place. One undoable step.
    ///
    /// # Errors
    /// [`Refused::Unknown`] for an unknown model or plate, [`Refused::Loading`] while a model
    /// loads; nothing is changed then.
    pub fn clone_object(
        &mut self,
        index: usize,
        copies: usize,
        to: Option<PlateId>,
        arranged: bool,
        name: String,
    ) -> Result<Cloned, Refused> {
        let source = self.plate().clone();
        let object = source.objects.get(index).cloned().ok_or(Refused::Unknown)?;
        let mut target = match to {
            Some(id) => self
                .project
                .plate(id)
                .ok_or(Refused::Unknown)?
                .content()
                .clone(),
            None => PlateContent {
                name,
                objects: Vec::new(),
                ..source.clone()
            },
        };
        let mut out = Cloned::default();
        let mut added_contents = Vec::new();
        match target.printer.clone() {
            None => {
                target.objects.extend((0..copies).map(|_| object.clone()));
                out.on_plate = copies;
            }
            Some(printer) => {
                let centre = placement::plate_centre(&printer);
                let (rect, translation) =
                    self.footprint(centre, &object).ok_or(Refused::Loading)?;
                // The copy with its footprint's lowest corner at `corner`.
                let at = |corner: [f64; 2]| {
                    let mut copy = object.clone();
                    copy.translate_mm[0] = translation[0] + corner[0] - rect.min[0];
                    copy.translate_mm[1] = translation[1] + corner[1] - rect.min[1];
                    copy.pose.center = false;
                    copy
                };
                let [ox, oy] = printer.origin_xy_mm;
                let [width, depth, _] = printer.build_size_mm;
                let area = Rect {
                    min: [ox, oy],
                    max: [ox + width, oy + depth],
                };
                // A model larger than an empty plate goes to its middle, as when not arranged.
                out.too_large = arrange(&[rect.size()], area, &[], 0.0).is_err();
                if arranged && !out.too_large {
                    // On the model's own plate the model is packed with its copies, item 0, as
                    // Arrange models would (owner, 2026-11-10); the other models stay.
                    let here = to == Some(self.plate);
                    let footprint = |o: &PlateObject| self.footprint(centre, o).map(|(r, _)| r);
                    let others = target
                        .objects
                        .iter()
                        .enumerate()
                        .filter(|&(k, _)| !here || k != index)
                        .map(|(_, o)| footprint(o))
                        .collect::<Option<Vec<_>>>()
                        .ok_or(Refused::Loading)?;
                    let spacing = target.settings.spacing_mm;
                    let count = copies + usize::from(here);
                    let sizes = vec![rect.size(); count];
                    let all: Vec<usize> = (0..count).collect();
                    let (placed, mut left) = pack(&all, &sizes, area, &others, spacing);
                    for (item, corner) in placed {
                        if here && item == 0 {
                            target.objects[index] = at(corner);
                        } else {
                            target.objects.push(at(corner));
                            out.on_plate += 1;
                        }
                    }
                    // The model itself, when even it has no room, stays where it was.
                    left.retain(|&item| !here || item != 0);
                    added_contents = self.overflow_plates(
                        &target,
                        (left, &sizes),
                        (area, spacing),
                        at,
                        &mut out,
                    );
                } else {
                    let corner = middle_corner(area, rect.size());
                    target.objects.extend((0..copies).map(|_| at(corner)));
                    out.on_plate = copies;
                }
            }
        }
        self.add_clones(to, target, added_contents)?;
        Ok(out)
    }

    /// New plates after `target`, as many as needed, holding the copies `left` (of `sizes`)
    /// placed by `at`, each packed on `area` `spacing` apart as an empty plate and named after
    /// `target` ("Arms (2)"); counted in `out`. Each copy fits an empty plate: the caller
    /// checked the model does.
    fn overflow_plates(
        &self,
        target: &PlateContent,
        (mut left, sizes): (Vec<usize>, &[[f64; 2]]),
        (area, spacing): (Rect, f64),
        at: impl Fn([f64; 2]) -> PlateObject,
        out: &mut Cloned,
    ) -> Vec<PlateContent> {
        let taken: Vec<String> = self
            .project
            .plates()
            .iter()
            .map(|p| p.content().name.clone())
            .chain(std::iter::once(target.name.clone()))
            .collect();
        let mut plates = Vec::new();
        let mut number = 2;
        while !left.is_empty() {
            let (on_new, remaining) = pack(&left, sizes, area, &[], spacing);
            // Never empty, each copy fitting an empty plate; stopping is the safe end all the
            // same, the copies left uncounted in `out`.
            if on_new.is_empty() {
                break;
            }
            let plate = fresh_name(&target.name, &mut number, |name| {
                taken.iter().chain(&out.new_plates).any(|t| t == name)
            });
            out.moved += on_new.len();
            plates.push(PlateContent {
                name: plate.clone(),
                objects: on_new.into_iter().map(|(_, corner)| at(corner)).collect(),
                ..target.clone()
            });
            out.new_plates.push(plate);
            left = remaining;
        }
        plates
    }

    /// Plate `to` (a new one after the plate shown for `None`) as `target`, and the plates
    /// `added` after it: one undoable step.
    fn add_clones(
        &mut self,
        to: Option<PlateId>,
        target: PlateContent,
        mut added: Vec<PlateContent>,
    ) -> Result<(), Refused> {
        let (contents, at) = if let Some(id) = to {
            let at = self
                .project
                .plates()
                .iter()
                .position(|p| p.id() == id)
                .ok_or(Refused::Unknown)?
                + 1;
            (vec![(id, target)], at)
        } else {
            added.insert(0, target);
            (Vec::new(), self.plate_index() + 1)
        };
        let added = added
            .into_iter()
            .enumerate()
            .map(|(k, content)| (at + k, content))
            .collect();
        self.project
            .edit_plates(contents, added)
            .map_err(|_| Refused::Unknown)?;
        Ok(())
    }

    /// The XY rectangle object `object` covers on a plate centred at `centre`, in world mm:
    /// the placed model, its supports' tips, bends and feet with their pads, and the raft's
    /// margin and slope beyond the pads (drain-hole cutters, inside the model, are left out).
    /// With it, the translation the placement used. `None` while its model is loading, or when
    /// it cannot be placed.
    fn footprint(&self, centre: [f64; 2], object: &PlateObject) -> Option<(Rect, [f64; 3])> {
        footprint_of(&self.meshes.get(&object.model)?.mesh, centre, object)
    }

    /// The pairs of objects on the plate shown, in increasing order, whose footprints (supports
    /// and raft included) overlap as rectangles: the test a slice applies, which prints them
    /// joined (TD-065); drain holes still cut only their own model (TD-166). The status bar
    /// warns of them (TD-163). Worked out from the placements when the plate changes, never
    /// per frame.
    pub fn overlapping(&mut self) -> Vec<(usize, usize)> {
        let revision = self
            .project
            .plate(self.plate)
            .map_or(0, karasu_project::project::Plate::revision);
        let key = (self.plate, revision);
        if let Some((cached, pairs)) = &self.overlaps {
            if *cached == key {
                return pairs.clone();
            }
        }
        self.placements();
        let rects: Vec<(usize, Rect)> = self
            .placements
            .1
            .iter()
            .zip(&self.plate().objects)
            .enumerate()
            .filter_map(|(index, (placed, object))| {
                let placed = placed.as_ref().ok()?;
                Some((index, placed_footprint(placed, object)?))
            })
            .collect();
        let only: Vec<Rect> = rects.iter().map(|&(_, rect)| rect).collect();
        let pairs: Vec<(usize, usize)> = overlapping_pairs(&only)
            .into_iter()
            .map(|(a, b)| (rects[a].0, rects[b].0))
            .collect();
        self.overlaps = Some((key, pairs.clone()));
        pairs
    }
}

/// The XY rectangle `object` covers on a plate centred at `centre`, `mesh` being its model's, in
/// world mm, with the translation its placement used (see [`placed_footprint`]); `None` when it
/// cannot be placed.
fn footprint_of(
    mesh: &TriangleMesh,
    centre: [f64; 2],
    object: &PlateObject,
) -> Option<(Rect, [f64; 3])> {
    let placement = placement::place(
        mesh,
        object.translate_mm,
        &object.pose,
        centre,
        &mut Cancellation::never(),
    )
    .ok()?;
    Some((
        placed_footprint(&placement, object)?,
        placement.translation_mm,
    ))
}

/// The XY rectangle `object`, placed by `placement`, covers, in world mm: the placed model, its
/// supports' tips, bends and feet with their pads, and the raft's margin and slope beyond the
/// pads (drain-hole cutters, inside the model, are left out). `None` when its supports cannot
/// be placed.
pub(crate) fn placed_footprint(placement: &Placement, object: &PlateObject) -> Option<Rect> {
    let mut rect = Rect {
        min: [placement.min.x, placement.min.y],
        max: [placement.max.x, placement.max.y],
    };
    for ([x, y], radius) in support_circles(placement, object)? {
        rect.min = [rect.min[0].min(x - radius), rect.min[1].min(y - radius)];
        rect.max = [rect.max[0].max(x + radius), rect.max[1].max(y + radius)];
    }
    Some(rect)
}

/// The circles, centre and radius in world mm, that `object`'s supports, placed by `placement`,
/// cover seen from above: tips, bends, feet with their pads, and the raft's margin and slope
/// beyond the pads. `None` when they cannot be placed.
fn support_circles(placement: &Placement, object: &PlateObject) -> Option<Vec<([f64; 2], f64)>> {
    let Some(document) = &object.supports else {
        return Some(Vec::new());
    };
    let affine = placement.transform.to_affine().ok()?;
    let placed = document.placed(&affine).ok()?;
    let raft = placed.raft.as_ref().map_or(0.0, |raft| {
        let run = raft.thickness_mm / raft.slope_deg.to_radians().tan().max(1e-6);
        raft.fillet_diameter_mm / 2.0 + raft.margin_mm + run
    });
    let mut circles = Vec::new();
    for support in &placed.supports {
        let p = &support.parameters;
        let at = |[x, y, _]: [f64; 3]| [x, y];
        circles.push((at(support.tip_mm), p.tip_body_diameter_mm / 2.0));
        for &bend in &support.bends {
            circles.push((at(bend), p.shaft_diameter_mm / 2.0));
        }
        let foot = (p.pad_diameter_mm / 2.0).max(raft);
        match support.base {
            BaseSpec::Platform(xy) => circles.push((xy, foot)),
            BaseSpec::Vertical => {
                let last = support.bends.last().copied().unwrap_or(support.tip_mm);
                circles.push((at(last), foot));
            }
            BaseSpec::Model { point_mm, .. } => {
                circles.push((at(point_mm), p.foot_body_diameter_mm / 2.0));
            }
            // Within its parent's footprint.
            BaseSpec::Support { .. } => {}
        }
    }
    Some(circles)
}

/// The sides of the polygon that stands for a support's circle in a convex footprint: drawn
/// round the circle, so the footprint never falls inside it.
const CIRCLE_SIDES: u32 = 16;

/// The convex footprint of `mesh` placed by `placement`, with `object`'s supports and raft,
/// seen from above in world mm (TD-162): the hull of the placed model's vertices and of a
/// polygon round each support circle. `None` when it cannot be placed.
pub(crate) fn convex_footprint(
    mesh: &TriangleMesh,
    placement: &Placement,
    object: &PlateObject,
) -> Option<Vec<[f64; 2]>> {
    let affine = placement.transform.to_affine().ok()?;
    let mut points: Vec<[f64; 2]> = Vec::with_capacity(mesh.vertices().len());
    for &vertex in mesh.vertices() {
        let p = affine.transform_point(vertex).ok()?;
        points.push([p.x, p.y]);
    }
    // A polygon round the circle: its corners at r / cos(π / n).
    let step = std::f64::consts::TAU / f64::from(CIRCLE_SIDES);
    let outside = 1.0 / (step / 2.0).cos();
    for ([x, y], radius) in support_circles(placement, object)? {
        for k in 0..CIRCLE_SIDES {
            let (sin, cos) = (step * f64::from(k)).sin_cos();
            points.push([x + radius * outside * cos, y + radius * outside * sin]);
        }
    }
    Some(convex_hull(&points))
}

impl Workspace {
    /// Show a plate that still exists: the shown one, or the first.
    fn keep_a_plate(&mut self) {
        if self.project.plate(self.plate).is_none() {
            if let Some(first) = self.project.plates().first() {
                self.plate = first.id();
            }
        }
    }

    /// Take finished imports into the project. Returns whether anything arrived.
    pub fn poll(&mut self) -> bool {
        let mut arrived = Vec::new();
        self.pending
            .retain(|pending| match pending.result.try_recv() {
                Ok(result) => {
                    arrived.push((pending.adds_object, result));
                    false
                }
                Err(mpsc::TryRecvError::Empty) => true,
                Err(mpsc::TryRecvError::Disconnected) => {
                    arrived.push((
                        pending.adds_object,
                        Err(format!("importing {} stopped unexpectedly", pending.name)),
                    ));
                    false
                }
            });
        let changed = !arrived.is_empty();
        for (adds_object, result) in arrived {
            match result {
                Ok(imported) if adds_object => self.add(imported),
                Ok(imported) => {
                    self.load(imported, None);
                }
                Err(text) => self
                    .notices
                    .push(Message::new(Level::Error, tr!("notice-cannot-open-model")).detail(text)),
            }
        }
        self.poll_diagnoses();
        changed
    }

    /// Report finished diagnoses, judged against the plate's settings now.
    fn poll_diagnoses(&mut self) {
        let mut finished = Vec::new();
        self.diagnosing
            .retain(|diagnosing| match diagnosing.result.try_recv() {
                Ok(result) => {
                    finished.push((
                        diagnosing.name.clone(),
                        diagnosing.model,
                        diagnosing.repair,
                        diagnosing.note.clone(),
                        result,
                    ));
                    false
                }
                Err(mpsc::TryRecvError::Empty) => true,
                Err(mpsc::TryRecvError::Disconnected) => {
                    finished.push((
                        diagnosing.name.clone(),
                        diagnosing.model,
                        diagnosing.repair,
                        diagnosing.note.clone(),
                        Err("the diagnosis stopped unexpectedly".to_owned()),
                    ));
                    false
                }
            });
        let close_holes = self.plate().settings.close_holes_mm.is_some();
        for (name, model, repair, note, result) in finished {
            let treatment = Treatment {
                repair,
                close_holes,
            };
            self.notices.push(match result {
                Ok(diagnosis) => {
                    let message = Message::opened(&name, &diagnosis, treatment, &note);
                    self.diagnoses.insert(model, diagnosis);
                    message
                }
                Err(error) => {
                    Message::new(Level::Warning, tr!("notice-cannot-check", name = &name))
                        .detail(format!("{error}\n{note}").trim_end().to_owned())
                }
            });
        }
    }

    /// Names of the models still being diagnosed.
    pub fn diagnosing(&self) -> impl Iterator<Item = &str> {
        self.diagnosing.iter().map(|d| d.name.as_str())
    }

    fn add(&mut self, imported: Imported) {
        let name = imported.name.clone();
        let assumed = imported.assumed;
        let id = self.load(imported, self.auto_repair.map(RepairPreset::spec));
        let centred = self.printer().is_some();
        let mut object = PlateObject::new(name, id);
        object.units = assumed;
        object.arrange = false;
        object.pose.center = centred;
        object.set_repair(self.auto_repair.map(RepairPreset::spec));
        self.edit(|plate| plate.objects.push(object));
    }

    /// Keep an imported model's mesh for drawing and diagnose it in the background; `repair` is
    /// what its new object gets, or `None` for a model of an opened project, judged with its
    /// object's own repairs. Returns its identity.
    fn load(&mut self, imported: Imported, repair: Option<RepairSpec>) -> ModelId {
        let id = self.project.add_model(&imported.name, imported.bytes);
        // The objects of this model could not be placed without its mesh: placed again now,
        // though the plate's revision stays (a project opened, TD-130).
        self.placements.0 = 0;
        let (origin, size_mm) =
            placement::source_bounds(&imported.mesh).map_or(([0.0; 3], [0.0; 3]), |(min, max)| {
                (
                    [
                        f64::midpoint(min.x, max.x),
                        f64::midpoint(min.y, max.y),
                        f64::midpoint(min.z, max.z),
                    ],
                    [max.x - min.x, max.y - min.y, max.z - min.z],
                )
            });
        let mesh = Arc::clone(
            &self
                .meshes
                .entry(id)
                .or_insert_with(|| LoadedMesh {
                    area_mm2: imported.area_mm2,
                    mesh: Arc::new(imported.mesh),
                    origin,
                    size_mm,
                    key: mesh_key(id),
                })
                .mesh,
        );
        let (sender, result) = mpsc::channel();
        let dropped_facets = imported.dropped_facets;
        thread::spawn(move || {
            let diagnosis = diagnose(&mesh, &mut Cancellation::never())
                .map(|d| Diagnosis {
                    dropped_facets,
                    ..d
                })
                .map_err(|e| e.to_string());
            // The receiver is gone only if the workspace was dropped.
            let _ = sender.send(diagnosis);
        });
        let mut note = imported.assumed.map_or_else(String::new, |unit| {
            tr!(
                "notice-assumed-unit",
                name = &imported.name,
                unit = unit.to_string(),
            )
        });
        // A file opened now gets the auto-repair; a project's model keeps its object's repairs.
        if let Some(preset) = self.auto_repair.filter(|_| repair.is_some()) {
            if !note.is_empty() {
                note.push('\n');
            }
            note.push_str(&tr!(
                "notice-auto-repair",
                preset = crate::repair_panel::preset_name(Some(preset))
            ));
        }
        let own = || {
            self.plate()
                .objects
                .iter()
                .find(|o| o.model == id)
                .and_then(PlateObject::repair)
        };
        let repair = repair.or_else(own).unwrap_or(self.plate().settings.repair);
        self.diagnosing.push(Diagnosing {
            name: imported.name,
            model: id,
            repair,
            note,
            result,
        });
        id
    }

    /// The size of object `index`'s model along its own axes, scale applied, in mm (mirroring
    /// keeps it): what the Transform tool shows beside the scale (issue #40). `None` for no
    /// such object or a model still loading.
    #[must_use]
    pub fn own_size(&self, index: usize) -> Option<[f64; 3]> {
        let object = self.plate().objects.get(index)?;
        let size = self.meshes.get(&object.model)?.size_mm;
        Some(std::array::from_fn(|k| size[k] * object.pose.scale[k]))
    }

    /// The loaded meshes, by library model.
    #[must_use]
    pub fn meshes(&self) -> &HashMap<ModelId, LoadedMesh> {
        &self.meshes
    }

    /// The placement of every object, recomputed when the plate changed.
    pub fn placements(&mut self) -> &[Result<Placement, String>] {
        let revision = self
            .project
            .plate(self.plate)
            .map_or(0, karasu_project::project::Plate::revision);
        if self.placements.0 != revision {
            let placements = self.place_all(self.plate());
            self.placements = (revision, placements);
        }
        &self.placements.1
    }

    /// The placement of object `index`: the cached one while the plate is unchanged, otherwise
    /// computed for that object alone.
    #[must_use]
    pub fn placement_of(&self, index: usize) -> Option<Placement> {
        let revision = self
            .project
            .plate(self.plate)
            .map_or(0, karasu_project::project::Plate::revision);
        if self.placements.0 == revision {
            return self.placements.1.get(index)?.as_ref().ok().copied();
        }
        let content = self.plate();
        let object = content.objects.get(index)?;
        let centre = content
            .printer
            .as_ref()
            .map_or([0.0; 2], placement::plate_centre);
        let mesh = self.meshes.get(&object.model)?;
        placement::place(
            &mesh.mesh,
            object.translate_mm,
            &object.pose,
            centre,
            &mut Cancellation::never(),
        )
        .ok()
    }

    /// Object `index`'s enabled drain holes in its model frame (TD-089), in timeline order.
    #[must_use]
    pub fn holes(&self, index: usize) -> Vec<DrainHole> {
        self.plate()
            .objects
            .get(index)
            .map_or_else(Vec::new, |object| object.drain_holes().copied().collect())
    }

    /// The map from object `index`'s model frame to world coordinates at its placement.
    #[must_use]
    pub fn model_to_world(&self, index: usize) -> Option<Affine> {
        self.placement_of(index).as_ref().and_then(affine)
    }

    /// The operation positions of object `index`'s enabled drain holes, in timeline order.
    fn hole_operations(object: &PlateObject) -> Vec<usize> {
        object
            .operations
            .iter()
            .enumerate()
            .filter(|(_, o)| o.enabled && matches!(o.kind, OperationKind::DrainHole(_)))
            .map(|(i, _)| i)
            .collect()
    }

    /// Replace drain hole `position` (among the enabled ones) of object `index` with `hole`, in
    /// its model frame, as one plate edit merged under `key` when given. The hole keeps its
    /// operation identity.
    pub fn set_hole(&mut self, index: usize, position: usize, hole: DrainHole, key: Option<u64>) {
        let apply = move |plate: &mut PlateContent| {
            let Some(object) = plate.objects.get_mut(index) else {
                return;
            };
            if let Some(&at) = Self::hole_operations(object).get(position) {
                object.operations[at].kind = OperationKind::DrainHole(hole);
            }
        };
        match key {
            Some(key) => self.edit_merging(key, apply),
            None => self.edit(apply),
        }
    }

    /// Remove drain hole `position` (among the enabled ones) of object `index`.
    pub fn remove_hole(&mut self, index: usize, position: usize) {
        self.edit(|plate| {
            let Some(object) = plate.objects.get_mut(index) else {
                return;
            };
            if let Some(&at) = Self::hole_operations(object).get(position) {
                let id = object.operations[at].id;
                object.remove_operation(id);
            }
        });
    }

    /// Remove every drain hole of object `index` as one plate edit.
    pub fn remove_holes(&mut self, index: usize) {
        self.edit(|plate| {
            let Some(object) = plate.objects.get_mut(index) else {
                return;
            };
            let ids: Vec<_> = Self::hole_operations(object)
                .into_iter()
                .map(|at| object.operations[at].id)
                .collect();
            for id in ids {
                object.remove_operation(id);
            }
        });
    }

    /// Append `holes`, in object `index`'s model frame, to its timeline as one plate edit.
    pub fn add_holes(&mut self, index: usize, holes: Vec<DrainHole>) {
        self.edit(|plate| {
            if let Some(object) = plate.objects.get_mut(index) {
                for hole in holes {
                    object.push_operation(OperationKind::DrainHole(hole));
                }
            }
        });
    }

    /// The operation positions of object `index`'s enabled blockers, in timeline order.
    fn blocker_operations(object: &PlateObject) -> Vec<usize> {
        object
            .operations
            .iter()
            .enumerate()
            .filter(|(_, o)| o.enabled && matches!(o.kind, OperationKind::Blocker(_)))
            .map(|(i, _)| i)
            .collect()
    }

    /// Hollow object `index` with `hollowing`, or make it solid with `None`, and give it
    /// `blockers` (its model frame) in place of its enabled ones, as one plate edit: Apply and
    /// Remove hollowing (owner, 2026-11-02).
    pub fn hollow_with(
        &mut self,
        index: usize,
        hollowing: Option<Hollowing>,
        blockers: Vec<Blocker>,
    ) {
        self.edit(|plate| {
            let Some(object) = plate.objects.get_mut(index) else {
                return;
            };
            object.set_hollowing(hollowing);
            let ids: Vec<_> = Self::blocker_operations(object)
                .into_iter()
                .map(|at| object.operations[at].id)
                .collect();
            for id in ids {
                object.remove_operation(id);
            }
            for blocker in blockers {
                object.push_operation(OperationKind::Blocker(blocker));
            }
        });
    }

    /// Object `index`'s supports, in its model frame (TD-108), if it has any.
    #[must_use]
    pub fn supports(&self, index: usize) -> Option<&SupportDocument> {
        self.plate().objects.get(index)?.supports.as_ref()
    }

    /// Give object `index` the supports `document`, in its model frame, or none with `None` (an
    /// empty document is none), as one plate edit merged under `key` when given.
    pub fn set_supports(
        &mut self,
        index: usize,
        document: Option<SupportDocument>,
        key: Option<u64>,
    ) {
        let document = document.filter(|d| !d.supports.is_empty() || d.raft.is_some());
        let apply = move |plate: &mut PlateContent| {
            if let Some(object) = plate.objects.get_mut(index) {
                object.supports = document;
            }
        };
        match key {
            Some(key) => self.edit_merging(key, apply),
            None => self.edit(apply),
        }
    }

    /// Apply `hollowing` to object `index`, or remove it with `None`, as one plate edit; its
    /// drain holes stay (TD-089: they are independent of hollowing).
    #[cfg(test)]
    pub fn set_hollowing(&mut self, index: usize, hollowing: Option<Hollowing>) {
        self.edit(|plate| {
            if let Some(object) = plate.objects.get_mut(index) {
                object.set_hollowing(hollowing);
            }
        });
    }

    /// Where object `index` of `content` (an earlier state of the plate) is placed.
    #[must_use]
    pub fn placement_in(&self, content: &PlateContent, index: usize) -> Option<Placement> {
        let object = content.objects.get(index)?;
        let centre = content
            .printer
            .as_ref()
            .map_or([0.0; 2], placement::plate_centre);
        let mesh = self.meshes.get(&object.model)?;
        placement::place(
            &mesh.mesh,
            object.translate_mm,
            &object.pose,
            centre,
            &mut Cancellation::never(),
        )
        .ok()
    }

    /// Where every object of `content` is placed.
    fn place_all(&self, content: &PlateContent) -> Vec<Result<Placement, String>> {
        let centre = content
            .printer
            .as_ref()
            .map_or([0.0; 2], placement::plate_centre);
        content
            .objects
            .iter()
            .map(|object| {
                let mesh = self
                    .meshes
                    .get(&object.model)
                    .ok_or("the model is not loaded")?;
                placement::place(
                    &mesh.mesh,
                    object.translate_mm,
                    &object.pose,
                    centre,
                    &mut Cancellation::never(),
                )
            })
            .collect()
    }

    /// One draw per placed object the scene shows, with the object's index, in the colour `color` gives for
    /// that index (linear RGB and alpha), opened by the holes `holes` gives for it
    /// ([`Draw::holes`]).
    pub fn draws(
        &mut self,
        color: impl Fn(usize) -> [f32; 4],
        holes: impl Fn(usize) -> [u32; 2],
    ) -> Vec<(usize, Draw)> {
        let placements = self.placements().to_vec();
        self.plate()
            .objects
            .iter()
            .zip(placements)
            .enumerate()
            .filter(|(index, _)| !self.is_hidden(*index))
            .filter_map(|(index, (object, placement))| {
                let mesh = self.meshes.get(&object.model)?;
                let placement = placement.ok()?;
                let draw = Draw {
                    mesh: mesh.key,
                    model: model_matrix(&placement, mesh.origin)?,
                    color: color(index),
                    overlay: false,
                    cap: crate::viewport::Cap::Model,
                    holes: holes(index),
                    bore: None,
                    outside: None,
                    triangles: None,
                    look: crate::viewport::Look::Solid,
                };
                Some((index, draw))
            })
            .collect()
    }

    /// What plate `id`'s thumbnail draws (TD-135): its models in `color`, placed as on the
    /// plate, its printer's build volume and the models' footprint. `None` without a printer,
    /// or while one of its models is still loading, so that no thumbnail misses one. Placing is
    /// a pass over each model: asked only when the plate's revision changes.
    #[must_use]
    pub fn plate_draws(&self, id: PlateId, color: [f32; 4]) -> Option<PlateDraws> {
        let content = self.project.plate(id)?.content();
        let printer = content.printer.as_ref()?;
        // Cheap checks first: placing is the costly part.
        if content
            .objects
            .iter()
            .any(|o| !self.meshes.contains_key(&o.model))
        {
            return None;
        }
        let [ox, oy] = printer.origin_xy_mm;
        let [w, d, h] = printer.build_size_mm;
        let mut draws = Vec::with_capacity(content.objects.len());
        let mut footprint: Option<[[f64; 2]; 2]> = None;
        for (object, placement) in content.objects.iter().zip(self.place_all(content)) {
            let mesh = self.meshes.get(&object.model)?;
            let Ok(placement) = placement else {
                continue;
            };
            let (low, high) = (
                [placement.min.x, placement.min.y],
                [placement.max.x, placement.max.y],
            );
            footprint = Some(footprint.map_or([low, high], |[l, h]| {
                [
                    [l[0].min(low[0]), l[1].min(low[1])],
                    [h[0].max(high[0]), h[1].max(high[1])],
                ]
            }));
            draws.push(Draw {
                mesh: mesh.key,
                model: model_matrix(&placement, mesh.origin)?,
                color,
                overlay: false,
                cap: crate::viewport::Cap::Off,
                holes: [0, 0],
                bore: None,
                outside: None,
                triangles: None,
                look: crate::viewport::Look::Solid,
            });
        }
        Some(PlateDraws {
            draws,
            volume: [[ox, oy, 0.0], [ox + w, oy + d, h]],
            footprint,
        })
    }

    /// The printer's build volume in world mm, its low and high corners (none without a
    /// printer): from the plate at its origin up to the build height.
    #[must_use]
    pub fn build_volume(&self) -> Option<[[f64; 3]; 2]> {
        let printer = self.printer()?;
        let [ox, oy] = printer.origin_xy_mm;
        let [w, d, h] = printer.build_size_mm;
        Some([[ox, oy, 0.0], [ox + w, oy + d, h]])
    }

    /// The objects that do not fit in the printer's build volume (none without a printer).
    pub fn outside(&mut self) -> Vec<usize> {
        let Some([low, high]) = self.build_volume() else {
            return Vec::new();
        };
        self.placements()
            .iter()
            .enumerate()
            .filter(|(_, p)| {
                p.as_ref().is_ok_and(|p| {
                    p.min.x < low[0]
                        || p.min.y < low[1]
                        || p.min.z < low[2]
                        || p.max.x > high[0]
                        || p.max.y > high[1]
                        || p.max.z > high[2]
                })
            })
            .map(|(index, _)| index)
            .collect()
    }

    /// Where `ray` first meets an object's surface.
    pub fn hit(&mut self, ray: &Ray) -> Option<SurfaceHit> {
        self.hit_in(ray, Section::default())
    }

    /// Where `ray` first meets an object's surface where the `section` shows it: what it cuts
    /// away cannot be hit, nor a hidden object.
    pub fn hit_in(&mut self, ray: &Ray, section: Section) -> Option<SurfaceHit> {
        let kept = |t: f64| {
            let z = ray.origin[2] + ray.direction[2] * t;
            section.below.is_none_or(|b| z >= b) && section.above.is_none_or(|a| z <= a)
        };
        let placements = self.placements().to_vec();
        self.plate()
            .objects
            .iter()
            .zip(placements)
            .enumerate()
            .filter(|(index, _)| !self.is_hidden(*index))
            .filter_map(|(index, (object, placement))| {
                let placement = placement.ok()?;
                let mesh = self.meshes.get(&object.model)?;
                let (min, max) = (placement.min, placement.max);
                ray_box(ray, [min.x, min.y, min.z], [max.x, max.y, max.z])?;
                let placed = affine(&placement)?;
                let (t, triangle) = ray_mesh_where(ray, &placed, &mesh.mesh, kept)?;
                Some((t, index, world_normal(&mesh.mesh, triangle, &placed)?))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(t, object, normal)| SurfaceHit {
                object,
                point: std::array::from_fn(|k| ray.origin[k] + ray.direction[k] * t),
                normal,
            })
    }

    /// Keep object `index` from going below the plate: an object lifted off it (not resting)
    /// is raised until its lowest point is on the plate.
    pub fn settle(&mut self, index: usize, key: u64) {
        let Some(Ok(placement)) = self.placements().get(index).cloned() else {
            return;
        };
        if placement.min.z < 0.0 {
            let rise = -placement.min.z;
            self.edit_merging(key, |plate| {
                if let Some(object) = plate.objects.get_mut(index) {
                    if !object.pose.drop {
                        object.translate_mm[2] += rise;
                    }
                }
            });
        }
    }

    /// Rest object `index` on the plate again.
    pub fn drop_to_plate(&mut self, index: usize) {
        self.edit(|plate| {
            if let Some(object) = plate.objects.get_mut(index) {
                object.pose.drop = true;
                object.pose.lift_mm = None;
            }
        });
    }

    /// The objects lifted off the plate: slicing them needs supports.
    pub fn floating(&mut self) -> Vec<usize> {
        self.placements()
            .iter()
            .enumerate()
            .filter(|(_, p)| p.as_ref().is_ok_and(|p| p.min.z > 0.0))
            .map(|(index, _)| index)
            .collect()
    }

    /// The world point an object turns and scales about (its source bounding-box centre, placed),
    /// and its placement.
    pub fn anchor(&mut self, index: usize) -> Option<([f64; 3], Placement)> {
        let placement = self.placements().get(index)?.clone().ok()?;
        let object = self.plate().objects.get(index)?;
        let mesh = self.meshes.get(&object.model)?;
        let centre = placement::centre(&mesh.mesh)?;
        let point = placement.transform.transform_point(centre).ok()?;
        Some(([point.x, point.y, point.z], placement))
    }

    /// The box holding the build volume and every placed object, for fitting the view.
    pub fn bounds(&mut self) -> Option<([f64; 3], [f64; 3])> {
        let mut corners: Vec<[f64; 3]> = self
            .placements()
            .iter()
            .flatten()
            .flat_map(|p| [[p.min.x, p.min.y, p.min.z], [p.max.x, p.max.y, p.max.z]])
            .collect();
        if let Some(printer) = self.printer() {
            let [ox, oy] = printer.origin_xy_mm;
            let [w, d, h] = printer.build_size_mm;
            corners.extend([[ox, oy, 0.0], [ox + w, oy + d, h]]);
        }
        let first = *corners.first()?;
        Some(corners.iter().fold((first, first), |(min, max), p| {
            (
                std::array::from_fn(|k| min[k].min(p[k])),
                std::array::from_fn(|k| max[k].max(p[k])),
            )
        }))
    }

    /// Triangles of every placed object.
    #[must_use]
    pub fn triangles(&self) -> usize {
        self.plate()
            .objects
            .iter()
            .filter_map(|o| self.meshes.get(&o.model))
            .map(|m| m.mesh.triangles().len())
            .sum()
    }
}

/// Where an object moved or copied to another plate landed (TD-135, phase 3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Landing {
    /// Where it was: it lies on the target's plate, clear of the models there.
    Kept,
    /// Beside the target's models, as the packer places it.
    Beside,
    /// In the middle of the target's plate: there was no room beside its models, so it may lie
    /// over them or beyond the build volume (TD-163).
    Middle,
}

/// Why an object could not be moved or copied to another plate (TD-135, phase 3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refused {
    /// No such object, or no such plate (or the plate shown itself).
    Unknown,
    /// Its model or a model of the target is still loading.
    Loading,
}

/// What Arrange does with the models that do not fit on the plate (TD-135, phase 5).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Overflow {
    /// They go to new plates after it, as many as needed.
    #[default]
    NewPlates,
    /// They stay where they are.
    Stay,
}

/// Where the copies of a model went (owner, 2026-11-10).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Cloned {
    /// Copies on the plate chosen.
    pub on_plate: usize,
    /// Copies on new plates after it, and those plates' names.
    pub moved: usize,
    pub new_plates: Vec<String>,
    /// The model fits not even an empty plate: its copies are in the middle of the plate
    /// chosen, arranged or not (TD-163).
    pub too_large: bool,
}

/// Why a plate could not be arranged (TD-135, phase 5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unarranged {
    /// The plate has no printer, so no plate to arrange on.
    NoPrinter,
    /// A model is still loading.
    Loading,
    /// A plate changed while they were being arranged (TD-162): nothing was changed.
    Changed,
    /// The arrangement was cancelled (TD-162): nothing was changed.
    Cancelled,
}

/// `items` (indices into `sizes`) packed on `area` `spacing` apart and clear of `obstacles`:
/// those placed with their lowest corner, and those left, in order. The packer lists what
/// does not fit; those are left out and the rest packed again, until all that remain fit.
fn pack(
    items: &[usize],
    sizes: &[[f64; 2]],
    area: Rect,
    obstacles: &[Rect],
    spacing: f64,
) -> (Vec<(usize, [f64; 2])>, Vec<usize>) {
    let mut keep = items.to_vec();
    let mut left = Vec::new();
    while !keep.is_empty() {
        let wanted: Vec<[f64; 2]> = keep.iter().map(|&i| sizes[i]).collect();
        match arrange(&wanted, area, obstacles, spacing) {
            Ok(corners) => return (keep.into_iter().zip(corners).collect(), sorted(left)),
            Err(unplaced) => {
                for &k in unplaced.items.iter().rev() {
                    left.push(keep.remove(k));
                }
            }
        }
    }
    (Vec::new(), sorted(left))
}

/// The lowest corner that centres a footprint of `size` on `area`, whether it fits or not.
fn middle_corner(area: Rect, size: [f64; 2]) -> [f64; 2] {
    std::array::from_fn(|k| f64::midpoint(area.min[k], area.max[k]) - size[k] / 2.0)
}

/// `base` followed by the first number from `number` on whose name `taken` refuses: "Arms (2)",
/// "Arms (3)". `number` moves past the name given.
pub(crate) fn fresh_name(base: &str, number: &mut usize, taken: impl Fn(&str) -> bool) -> String {
    let mut name = format!("{base} ({number})");
    while taken(&name) {
        *number += 1;
        name = format!("{base} ({number})");
    }
    *number += 1;
    name
}

/// `items`, increasing.
fn sorted(mut items: Vec<usize>) -> Vec<usize> {
    items.sort_unstable();
    items
}

/// What a plate's thumbnail draws, from [`Workspace::plate_draws`] (TD-135).
pub struct PlateDraws {
    /// The plate's placed models.
    pub draws: Vec<Draw>,
    /// The printer's build volume, low and high corners in world mm.
    pub volume: [[f64; 3]; 2],
    /// The box the placed models cover on the plate, low and high corners in world mm (the
    /// models alone, without supports or raft); `None` without a placed model.
    pub footprint: Option<[[f64; 2]; 2]>,
}

/// `hole` mapped by `map`: its anchor or entry as a point, an explicit direction as a vector;
/// diameters, depths and lengths stay in millimetres (TD-089). A tilt is left as it is: it is
/// measured about the surface normal, which only a probe knows (see
/// [`DrainHole::with_world_tilt`]).
#[must_use]
pub fn map_hole(hole: DrainHole, map: &Affine) -> DrainHole {
    match hole {
        DrainHole::Axis {
            entry_mm,
            direction,
            length_mm,
            diameter_mm,
        } => DrainHole::Axis {
            entry_mm: map.point(entry_mm),
            direction: map.vector(direction),
            length_mm,
            diameter_mm,
        },
        DrainHole::Surface {
            surface_mm,
            diameter_mm,
            outside_mm,
            inside_mm,
            tilt,
            depth_mm,
            surface,
        } => DrainHole::Surface {
            surface_mm: map.point(surface_mm),
            diameter_mm,
            outside_mm,
            inside_mm,
            tilt,
            depth_mm,
            surface,
        },
    }
}

/// The placement as an affine map (shared with the command line's automatic supports, TD-157).
pub use karasu_project::placement::affine;

/// The matrix from GPU vertices (relative to `origin`) to world: the placement after the
/// translation by `origin`.
fn model_matrix(placement: &Placement, origin: [f64; 3]) -> Option<Mat4> {
    let affine = placement.transform.to_affine().ok()?;
    let linear = affine.linear();
    let t = affine.translation().components();
    Some(Mat4(std::array::from_fn(|row| {
        if row == 3 {
            [0.0, 0.0, 0.0, 1.0]
        } else {
            let l = linear[row];
            [
                l[0],
                l[1],
                l[2],
                t[row] + l[0] * origin[0] + l[1] * origin[1] + l[2] * origin[2],
            ]
        }
    })))
}

/// The width of the border around the plate, in mm: about 1 % of its larger side.
#[must_use]
pub fn plate_border_mm(printer: &PrinterProfile) -> f64 {
    let [w, d, _] = printer.build_size_mm;
    (w.max(d) * 0.012).clamp(0.8, 3.0)
}

/// The plate's triangles at Z = 0: its surface in `color`, which fades when the view goes below
/// the plate, then the marks that stay, the border around it in `border` and the front tab in
/// `tab` (TD-088).
#[must_use]
pub fn plate_surface(
    printer: &PrinterProfile,
    color: [f32; 4],
    border: [f32; 4],
    tab: [f32; 4],
) -> (Vec<LineVertex>, Vec<LineVertex>) {
    let [ox, oy] = printer.origin_xy_mm;
    let [w, d, _] = printer.build_size_mm;
    #[allow(clippy::cast_possible_truncation)] // GPU precision.
    let v = |[x, y]: [f64; 2], color| LineVertex {
        position: [x as f32, y as f32, 0.0],
        color,
    };
    let quad = |corners: [[f64; 2]; 4], color| [0, 1, 2, 0, 2, 3].map(|k| v(corners[k], color));
    let inner = [[ox, oy], [ox + w, oy], [ox + w, oy + d], [ox, oy + d]];
    let rim = plate_border_mm(printer);
    let outer = [
        [ox - rim, oy - rim],
        [ox + w + rim, oy - rim],
        [ox + w + rim, oy + d + rim],
        [ox - rim, oy + d + rim],
    ];
    let mut marks = Vec::new();
    // The border: one trapezoid per side, between the plate's edge and the outer outline.
    for i in 0..4 {
        let j = (i + 1) % 4;
        marks.extend(quad([inner[i], inner[j], outer[j], outer[i]], border));
    }
    marks.extend(quad(FrontTab::of(printer).corners(), tab));
    (quad(inner, color).to_vec(), marks)
}

/// The tab marking the plate's front, the edge at the lowest Y (`docs/ARCHITECTURE.md`): a
/// trapezoid on the plate's plane, outside that edge and centred on it, which carries the word
/// "Front".
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrontTab {
    /// The middle of the tab, in world mm on the plate's plane.
    pub center: [f64; 2],
    /// The length along the plate's edge, in mm.
    pub long: f64,
    /// The length of the outer side, in mm.
    pub short: f64,
    /// How far the tab reaches out from the edge, in mm.
    pub depth: f64,
}

impl FrontTab {
    /// The tab of `printer`'s plate, sized to the plate's width, beyond the plate's border.
    #[must_use]
    pub fn of(printer: &PrinterProfile) -> Self {
        let [ox, oy] = printer.origin_xy_mm;
        let width = printer.build_size_mm[0];
        let long = (width * 0.36).min(120.0);
        let depth = (long * 0.2).clamp(4.0, 16.0);
        let edge = oy - plate_border_mm(printer);
        Self {
            center: [ox + width / 2.0, edge - depth / 2.0],
            long,
            short: long * 0.72,
            depth,
        }
    }

    /// The glyph triangles of `galley` (laid out by egui, its UVs in texels of an atlas of
    /// `atlas_size`) lying flat in the tab, centred and as large as the tab allows. Read from
    /// the front, the text runs along +X with its top towards the plate (+Y).
    #[must_use]
    pub fn label(
        &self,
        galley: &eframe::egui::Galley,
        atlas_size: [usize; 2],
        color: [f32; 4],
    ) -> Vec<LabelVertex> {
        let bounds = galley
            .rows
            .iter()
            .filter(|row| !row.visuals.mesh.is_empty())
            .map(|row| row.visuals.mesh_bounds.translate(row.pos.to_vec2()))
            .reduce(eframe::egui::Rect::union);
        let Some(bounds) = bounds.filter(|b| b.width() > 0.0 && b.height() > 0.0) else {
            return Vec::new();
        };
        // The ink fills 55 percent of the tab's depth and stays within 85 percent of its outer
        // side.
        let scale = (self.depth * 0.55 / f64::from(bounds.height()))
            .min(self.short * 0.85 / f64::from(bounds.width()));
        let middle = bounds.center();
        #[allow(clippy::cast_precision_loss)] // Atlas sizes.
        let texel = [1.0 / atlas_size[0] as f32, 1.0 / atlas_size[1] as f32];
        let mut vertices = Vec::new();
        for row in &galley.rows {
            let mesh = &row.visuals.mesh;
            for &index in &mesh.indices {
                let vertex = mesh.vertices[index as usize];
                let at = vertex.pos + row.pos.to_vec2() - middle;
                #[allow(clippy::cast_possible_truncation)] // GPU precision.
                vertices.push(LabelVertex {
                    position: [
                        (self.center[0] + f64::from(at.x) * scale) as f32,
                        (self.center[1] - f64::from(at.y) * scale) as f32,
                        0.0,
                    ],
                    uv: [vertex.uv.x * texel[0], vertex.uv.y * texel[1]],
                    color,
                });
            }
        }
        vertices
    }

    /// The corners: along the plate's edge left to right, then the outer side right to left.
    #[must_use]
    pub fn corners(&self) -> [[f64; 2]; 4] {
        let [cx, cy] = self.center;
        let (edge, outer) = (cy + self.depth / 2.0, cy - self.depth / 2.0);
        [
            [cx - self.long / 2.0, edge],
            [cx + self.long / 2.0, edge],
            [cx + self.short / 2.0, outer],
            [cx - self.short / 2.0, outer],
        ]
    }
}

/// The plate grid every 10 mm in `grid` and every 50 mm in `major`, which fades when the view
/// goes below the plate; then the plate's outline and the build volume's edges in `edge`.
#[must_use]
#[allow(clippy::many_single_char_names)] // Coordinates and the build size.
pub fn volume_lines(
    printer: &PrinterProfile,
    grid: [f32; 4],
    major: [f32; 4],
    edge: [f32; 4],
) -> (Vec<LineVertex>, Vec<LineVertex>) {
    #[allow(clippy::cast_possible_truncation)] // GPU precision.
    let v = |x: f64, y: f64, z: f64, color| LineVertex {
        position: [x as f32, y as f32, z as f32],
        color,
    };
    let [ox, oy] = printer.origin_xy_mm;
    let [w, d, h] = printer.build_size_mm;
    let mut lines = Vec::new();
    let mark = |k: u32| if k.is_multiple_of(5) { major } else { grid };
    let mut k = 1;
    while f64::from(k) * 10.0 < w {
        let x = ox + f64::from(k) * 10.0;
        lines.extend([v(x, oy, 0.0, mark(k)), v(x, oy + d, 0.0, mark(k))]);
        k += 1;
    }
    let mut k = 1;
    while f64::from(k) * 10.0 < d {
        let y = oy + f64::from(k) * 10.0;
        lines.extend([v(ox, y, 0.0, mark(k)), v(ox + w, y, 0.0, mark(k))]);
        k += 1;
    }
    let mut edges = Vec::new();
    let corners = [[ox, oy], [ox + w, oy], [ox + w, oy + d], [ox, oy + d]];
    for i in 0..4 {
        let ([x0, y0], [x1, y1]) = (corners[i], corners[(i + 1) % 4]);
        for z in [0.0, h] {
            edges.extend([v(x0, y0, z, edge), v(x1, y1, z, edge)]);
        }
        edges.extend([v(x0, y0, 0.0, edge), v(x0, y0, h, edge)]);
    }
    (lines, edges)
}

#[cfg(test)]
pub(crate) mod tests {
    use crate::arranging::{plan_arrangement, turn_of, ArrangeProgress};
    use karasu_core::print_job::uniform_time_s;

    impl Workspace {
        /// Arrange the shown plate on this thread: [`Self::arrange_input`], [`plan_arrangement`] and
        /// [`Self::finish_arrange`] in turn. The application plans on a thread of its own
        /// ([`ArrangeJob`]); the tests use this.
        ///
        /// # Errors
        /// [`Unarranged`]: no printer, or a model still loading, with nothing changed.
        pub(crate) fn arrange(&mut self, options: ArrangeOptions) -> Result<Arranged, Unarranged> {
            let input = self.arrange_input(options)?;
            let planned = plan_arrangement(&input, &ArrangeProgress::default())?;
            self.finish_arrange(planned)
        }
    }

    use super::*;
    use std::fmt::Write as _;

    #[test]
    fn an_opened_project_places_its_models_once_their_meshes_arrive() {
        // The owner's marshal reopened showed its cavity alone (2026-11-04): its object was
        // placed before its mesh arrived, and that placement was kept for the revision.
        let mut project = Project::new("Opened");
        let model = project.add_model("cube.stl", box_stl([0.0; 3], [8.0; 3]).into_bytes());
        project.add_plate(PlateContent {
            name: "Plate".into(),
            printer: None,
            resin: None,
            print_setting: None,
            settings: PlateSettings::new("goo-v3"),
            objects: vec![PlateObject::new("cube", model)],
        });
        let mut workspace = Workspace::with_project(project);
        // The scene asks for the placements at once, before the import arrives.
        assert!(workspace.placements()[0].is_err());
        wait(&mut workspace);
        assert!(workspace.placements()[0].is_ok());
        assert_eq!(workspace.draws(|_| [1.0; 4], |_| [0, 0]).len(), 1);
        assert_eq!(workspace.revision(), 1);
    }

    #[test]
    fn the_memory_budget_follows_the_installed_memory() {
        // TD-094: read on every supported system, and never under a million cavity triangles.
        use karasu_core::preparation::sampled::ProbeLimits;
        let budget = machine_memory_budget().expect("installed memory");
        assert!(
            budget >= ProbeLimits::BYTES_PER_FACE * 1_000_000,
            "{budget}"
        );
        assert_eq!(Workspace::new().memory_budget(), Some(budget));
    }

    #[test]
    fn plates_are_added_shown_moved_deleted_and_undone_as_steps_of_the_project() {
        // TD-135: plates come and go as undoable steps, newest first with each plate's edits.
        let mut workspace = Workspace::new();
        let names = |w: &Workspace| -> Vec<String> {
            w.plates()
                .iter()
                .map(|p| p.content().name.clone())
                .collect()
        };
        let first = workspace.plate_id();
        workspace.rename_plate("First".into());
        workspace.new_plate("Second".into());
        let second = workspace.plate_id();
        assert_ne!(first, second);
        assert_eq!(workspace.plate_index(), 1);
        assert_eq!(workspace.plate().objects.len(), 0);
        workspace.duplicate_plate(workspace.plate_id(), "Second (copy)".into());
        let third = workspace.plate_id();
        assert_eq!(names(&workspace), ["First", "Second", "Second (copy)"]);
        // Previous and Next wrap round.
        assert!(workspace.step_plate(1));
        assert_eq!(workspace.plate_id(), first);
        assert!(workspace.step_plate(-1));
        assert_eq!(workspace.plate_id(), third);
        workspace.move_plate(third, 0);
        assert_eq!(names(&workspace), ["Second (copy)", "First", "Second"]);
        // Deleting the plate shown shows the next one.
        assert!(workspace.delete_plate(workspace.plate_id()));
        assert_eq!(workspace.plate_id(), first);
        assert_eq!(names(&workspace), ["First", "Second"]);
        // Undo brings the plate back and shows it, then the move, the copy, the new plate and
        // the rename, newest first.
        assert!(workspace.undo());
        assert_eq!(workspace.plate_id(), third);
        assert_eq!(names(&workspace), ["Second (copy)", "First", "Second"]);
        assert!(workspace.undo());
        assert_eq!(names(&workspace), ["First", "Second", "Second (copy)"]);
        assert!(workspace.undo());
        assert_eq!(names(&workspace), ["First", "Second"]);
        assert!(workspace.project().plate(workspace.plate_id()).is_some());
        workspace.show_plate(first);
        assert!(workspace.undo());
        assert_eq!(names(&workspace), ["First"]);
        assert!(workspace.undo());
        assert_ne!(names(&workspace), ["First"]);
        assert!(!workspace.undo());
        // The last plate stays.
        assert!(!workspace.delete_plate(workspace.plate_id()));
        assert_eq!(workspace.plates().len(), 1);
    }

    #[test]
    fn another_plate_is_duplicated_and_deleted_from_the_overview() {
        // TD-135, the overview: a card acts on its own plate, not on the plate shown.
        let mut workspace = Workspace::new();
        let first = workspace.plate_id();
        workspace.new_plate("Second".into());
        let second = workspace.plate_id();
        workspace.show_plate(first);
        // Deleting a plate not shown keeps the plate shown.
        assert!(workspace.delete_plate(second));
        assert_eq!(workspace.plate_id(), first);
        assert_eq!(workspace.plates().len(), 1);
        assert!(!workspace.delete_plate(second));
        assert!(workspace.undo());
        assert!(workspace.project().plate(second).is_some());
        // Duplicating a plate puts its copy after it and shows the copy.
        workspace.show_plate(first);
        workspace.duplicate_plate(second, "Second (copy)".into());
        assert_eq!(workspace.plate_index(), 2);
        assert_eq!(workspace.plate().name, "Second (copy)");
    }

    #[test]
    fn clones_go_around_the_models_there_or_to_the_middle_and_overflow_to_new_plates() {
        // Owner, 2026-11-10: a slab of 60 × 30 mm in the middle of a 192 × 120 mm plate, cloned.
        let folder =
            std::env::temp_dir().join(format!("karasu-workspace-clone-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let slab = folder.join("slab.stl");
        std::fs::write(&slab, box_stl([0.0; 3], [60.0, 30.0, 3.0])).unwrap();
        let mut workspace = Workspace::new();
        let (printer, resin, setting) = catalog_profiles("anycubic-photon-mono-x");
        workspace.edit(|plate| {
            plate.name = "Arms".into();
            plate.printer = Some(printer);
            plate.resin = Some(resin);
            plate.print_setting = Some(setting);
        });
        workspace.open(slab.clone());
        wait(&mut workspace);
        let first = workspace.plate_id();
        let original = workspace.plate().objects[0].clone();
        let spacing = workspace.plate().settings.spacing_mm;
        let rects = |workspace: &Workspace| -> Vec<Rect> {
            let content = workspace.plate().clone();
            let centre = placement::plate_centre(content.printer.as_ref().unwrap());
            content
                .objects
                .iter()
                .map(|o| workspace.footprint(centre, o).unwrap().0)
                .collect()
        };
        // Arranged, alone on its plate: the slab and its copies arranged together from the
        // middle, as Arrange models arranges a plate (owner, 2026-11-10).
        let cloned = workspace
            .clone_object(0, 3, Some(first), true, "unused".into())
            .unwrap();
        assert_eq!((cloned.on_plate, cloned.moved), (3, 0));
        assert_ne!(workspace.plate().objects[0], original);
        let placed = rects(&workspace);
        let printer = workspace.plate().printer.clone().unwrap();
        let middle = std::array::from_fn::<f64, 2, _>(|k| {
            printer.origin_xy_mm[k] + printer.build_size_mm[k] / 2.0
        });
        // The model in the middle (the arrangement's first), its copies around it, apart.
        for (k, m) in middle.iter().enumerate() {
            let r = &placed[0];
            assert!(
                (f64::midpoint(r.min[k], r.max[k]) - m).abs() < 1e-6,
                "{placed:?}"
            );
        }
        for (k, r) in placed.iter().enumerate() {
            for q in &placed[k + 1..] {
                assert!(!r.within(q, spacing - 1e-6), "{r:?} {q:?}");
            }
        }
        assert!(workspace.undo());
        assert_eq!(workspace.plate().objects[0], original);
        // Many: those left over go to a new plate after it, the slab staying on its plate.
        let cloned = workspace
            .clone_object(0, 12, Some(first), true, "unused".into())
            .unwrap();
        assert_eq!(cloned.on_plate + cloned.moved, 12);
        assert!(cloned.on_plate >= 4 && cloned.moved >= 1, "{cloned:?}");
        assert_eq!(cloned.new_plates, vec!["Arms (2)".to_owned()]);
        assert_eq!(workspace.plate().objects.len(), 1 + cloned.on_plate);
        let placed = rects(&workspace);
        for (k, r) in placed.iter().enumerate() {
            for q in &placed[k + 1..] {
                assert!(!r.within(q, spacing - 1e-6), "{r:?} {q:?}");
            }
        }
        let plates = workspace.project().plates();
        assert_eq!(plates[1].content().name, "Arms (2)");
        assert_eq!(plates[1].content().objects.len(), cloned.moved);
        assert_eq!(workspace.plate_id(), first);
        // One step: undone whole.
        assert!(workspace.undo());
        assert_eq!(workspace.project().plates().len(), 1);
        assert_eq!(workspace.plate().objects.len(), 1);
        // Not arranged: every copy in the middle of the plate, nothing elsewhere.
        let cloned = workspace
            .clone_object(0, 2, Some(first), false, "unused".into())
            .unwrap();
        assert_eq!((cloned.on_plate, cloned.moved), (2, 0));
        let placed = rects(&workspace);
        for r in &placed[1..] {
            for (k, m) in middle.iter().enumerate() {
                assert!((f64::midpoint(r.min[k], r.max[k]) - m).abs() < 1e-6);
            }
        }
        assert!(workspace.undo());
        // To a new plate: after the plate shown, with its profiles, the copies on it.
        let cloned = workspace
            .clone_object(0, 2, None, true, "Plate 2".into())
            .unwrap();
        assert_eq!((cloned.on_plate, cloned.moved), (2, 0));
        let plates = workspace.project().plates();
        assert_eq!(plates.len(), 2);
        assert_eq!(plates[1].content().name, "Plate 2");
        assert_eq!(plates[1].content().objects.len(), 2);
        assert!(plates[1].content().printer.is_some());
        assert_eq!(workspace.plate().objects.len(), 1);
        // An unknown model: nothing changes.
        assert_eq!(
            workspace.clone_object(9, 1, Some(first), true, String::new()),
            Err(Refused::Unknown)
        );
        let _ = std::fs::remove_dir_all(folder);
    }

    /// Arrange's options at a 2 mm spacing.
    fn options(overflow: Overflow, by_height: bool, turn: bool) -> ArrangeOptions {
        ArrangeOptions {
            spacing_mm: 2.0,
            overflow,
            by_height,
            turn,
            scope: ArrangeScope::ThisPlate,
            plates: PlateChoice::SamePrinter,
            remove_empty: false,
        }
    }

    /// A workspace on a 192 × 120 mm plate (a Photon Mono X's) holding one box of `size`, its
    /// folder removed by the caller.
    fn one_box(name: &str, size: [f64; 3]) -> (Workspace, std::path::PathBuf) {
        let folder = std::env::temp_dir().join(format!("{name}-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let model = folder.join("box.stl");
        std::fs::write(&model, box_stl([0.0; 3], size)).unwrap();
        let mut workspace = Workspace::new();
        let (printer, resin, setting) = catalog_profiles("anycubic-photon-mono-x");
        workspace.edit(|plate| {
            plate.printer = Some(printer);
            plate.resin = Some(resin);
            plate.print_setting = Some(setting);
        });
        workspace.open(model);
        wait(&mut workspace);
        (workspace, folder)
    }

    /// The footprint of the plate shown's object `index`.
    fn rect_of(workspace: &Workspace, index: usize) -> Rect {
        let content = workspace.plate().clone();
        let centre = placement::plate_centre(content.printer.as_ref().unwrap());
        workspace
            .footprint(centre, &content.objects[index])
            .unwrap()
            .0
    }

    #[test]
    fn arrange_turns_a_model_lying_aslant_back_to_its_smallest_room() {
        // TD-162: a 100 × 10 mm bar turned 30° takes 92 × 59 mm; turned back, 100 × 10.
        let (mut workspace, folder) = one_box("karasu-arrange-aslant", [100.0, 10.0, 5.0]);
        workspace.edit(|plate| plate.objects[0].pose.rotate_deg = [0.0, 0.0, 30.0]);
        let aslant = rect_of(&workspace, 0).size();
        assert!(aslant[0] * aslant[1] > 5000.0, "{aslant:?}");
        // Not asked: it keeps its turn.
        let kept = workspace
            .arrange(options(Overflow::NewPlates, true, false))
            .unwrap();
        assert_eq!(kept.turned, 0);
        assert!((workspace.plate().objects[0].pose.rotate_deg[2] - 30.0).abs() < 1e-9);
        assert!(workspace.undo());
        // Asked: turned back, along X or Y, and said so.
        let turned = workspace
            .arrange(options(Overflow::NewPlates, true, true))
            .unwrap();
        assert_eq!((turned.turned, turned.on_plate), (1, 1));
        let z = workspace.plate().objects[0].pose.rotate_deg[2];
        assert!((z / 90.0 - (z / 90.0).round()).abs() < 1e-9, "{z}");
        let [w, d] = rect_of(&workspace, 0).size();
        assert!((w * d - 1000.0).abs() < 1e-6, "{w} {d}");
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn arrange_turns_a_model_that_fits_only_turned_or_along_the_diagonal() {
        // TD-162: 110 × 150 mm fits the 192 × 120 mm plate only turned a quarter.
        let (mut workspace, folder) = one_box("karasu-arrange-quarter", [110.0, 150.0, 5.0]);
        let stays = workspace
            .arrange(options(Overflow::NewPlates, true, false))
            .unwrap();
        assert_eq!(stays.stayed, 1);
        assert!(workspace.undo());
        let fits = workspace
            .arrange(options(Overflow::NewPlates, true, true))
            .unwrap();
        assert_eq!((fits.on_plate, fits.stayed, fits.turned), (1, 0, 1));
        let [w, d] = rect_of(&workspace, 0).size();
        assert!(w <= 192.0 && d <= 120.0, "{w} {d}");
        let _ = std::fs::remove_dir_all(&folder);
        // A 205 × 8 mm bar is longer than both sides; along the diagonal it fits.
        let (mut workspace, folder) = one_box("karasu-arrange-diagonal", [205.0, 8.0, 5.0]);
        let fits = workspace
            .arrange(options(Overflow::NewPlates, true, true))
            .unwrap();
        assert_eq!((fits.on_plate, fits.stayed, fits.turned), (1, 0, 1));
        let rect = rect_of(&workspace, 0);
        assert!(rect.max[0] - rect.min[0] <= 192.0 && rect.max[1] - rect.min[1] <= 120.0);
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_turned_model_takes_its_supports_with_it() {
        // TD-162: supports are kept in the model's frame (TD-108), so a turn about Z carries
        // them; the footprint, with them, still lies on the plate.
        use karasu_core::supports::{AxisSpec, ManualSupport, Preset, SupportKind, SupportOrigin};
        let (mut workspace, folder) = one_box("karasu-arrange-supported", [60.0, 10.0, 10.0]);
        let mut document = SupportDocument::new(Preset::Medium);
        document.supports.push(ManualSupport {
            id: 1,
            tip_mm: [60.0, 5.0, 5.0],
            tip_axis: AxisSpec::Normal,
            bends: Vec::new(),
            base: BaseSpec::Platform([75.0, 5.0]),
            parameters: Preset::Medium.parameters(),
            accept_collision: true,
            origin: SupportOrigin::Placed,
            kind: SupportKind::Standard,
        });
        workspace.edit(|plate| {
            plate.objects[0].supports = Some(document.clone());
            plate.objects[0].pose.rotate_deg = [0.0, 0.0, 40.0];
        });
        let turned = workspace
            .arrange(options(Overflow::NewPlates, false, true))
            .unwrap();
        assert_eq!(turned.turned, 1);
        let object = &workspace.plate().objects[0];
        assert_eq!(object.supports.as_ref(), Some(&document));
        let rect = rect_of(&workspace, 0);
        let [w, d] = rect.size();
        // The bar and the support beyond its end, about 80 mm along one axis once turned back.
        assert!(w.max(d) > 75.0 && w.min(d) < 20.0, "{w} {d}");
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    #[ignore = "times a local large model; set KARASU_INTERFACE_MODEL"]
    fn the_turn_of_a_local_model_is_timed() {
        // TD-162: the convex footprint's cost on a large model.
        let Ok(path) = std::env::var("KARASU_INTERFACE_MODEL") else {
            eprintln!("KARASU_INTERFACE_MODEL is not set: nothing to time");
            return;
        };
        let mut workspace = Workspace::new();
        let (printer, _, _) = catalog_profiles("anycubic-photon-mono-x");
        workspace.edit(|plate| plate.printer = Some(printer.clone()));
        workspace.open(std::path::PathBuf::from(path));
        wait(&mut workspace);
        let object = workspace.plate().objects[0].clone();
        let centre = placement::plate_centre(&printer);
        let triangles = workspace.meshes()[&object.model].mesh.triangles().len();
        let start = std::time::Instant::now();
        let mesh = Arc::clone(&workspace.meshes()[&object.model].mesh);
        let turn = turn_of(&mesh, centre, &object, [192.0, 120.0]);
        eprintln!(
            "{triangles} triangles: turn {turn:?} in {:.0} ms",
            start.elapsed().as_secs_f64() * 1000.0
        );
        // The whole Arrange, as packed today and with turns.
        for turn in [false, true] {
            let start = std::time::Instant::now();
            let arranged = workspace.arrange(options(Overflow::NewPlates, true, turn));
            eprintln!(
                "Arrange, turn {turn}: {:.0} ms, {arranged:?}",
                start.elapsed().as_secs_f64() * 1000.0
            );
            workspace.undo();
        }
    }

    #[test]
    fn an_arrangement_planned_on_its_own_thread_is_the_same_cancels_and_spots_a_changed_plate() {
        // Owner, 2026-11-19: Arrange works beside the window; done, cancelled, or the plate
        // changed meanwhile.
        let (mut workspace, folder) = one_box("karasu-arrange-job", [90.0, 50.0, 5.0]);
        workspace.edit(|plate| {
            let object = plate.objects[0].clone();
            for k in 1..6 {
                let mut copy = object.clone();
                copy.translate_mm = [f64::from(k) * 3.0, 0.0, 0.0];
                plate.objects.push(copy);
            }
        });
        let options = options(Overflow::NewPlates, true, true);
        let before = workspace.plate().clone();
        // On a thread of its own: the same arrangement as on this one.
        let job = ArrangeJob::start(workspace.arrange_input(options).unwrap());
        let started = std::time::Instant::now();
        let result = loop {
            if let Some(result) = job.poll() {
                break result;
            }
            assert!(started.elapsed().as_secs() < 60, "{:?}", job.progress());
            std::thread::sleep(std::time::Duration::from_millis(5));
        };
        let threaded = workspace.finish_arrange(result.unwrap()).unwrap();
        let shown = workspace.plate().clone();
        let plates = workspace.project().plates().len();
        assert!(workspace.undo());
        assert_eq!(workspace.plate(), &before);
        let here = workspace.arrange(options).unwrap();
        assert_eq!(threaded, here);
        assert_eq!(workspace.plate(), &shown);
        assert_eq!(workspace.project().plates().len(), plates);
        assert!(workspace.undo());
        // Asked to stop: nothing planned.
        let progress = ArrangeProgress::default();
        progress.stop();
        let input = workspace.arrange_input(options).unwrap();
        assert!(matches!(
            plan_arrangement(&input, &progress),
            Err(Unarranged::Cancelled)
        ));
        // The plate changed meanwhile: refused, the plate as the user left it.
        let planned = plan_arrangement(&input, &ArrangeProgress::default()).unwrap();
        workspace.edit(|plate| plate.objects[0].translate_mm = [1.0, 2.0, 0.0]);
        let edited = workspace.plate().clone();
        assert_eq!(workspace.finish_arrange(planned), Err(Unarranged::Changed));
        assert_eq!(workspace.plate(), &edited);
        assert_eq!(workspace.project().plates().len(), 1);
        let _ = std::fs::remove_dir_all(&folder);
    }

    /// A plate for [`plates_with`]: its name, its catalog printer (or none), and its boxes.
    type PlateOf<'a> = (&'a str, Option<&'a str>, &'a [[f64; 3]]);

    /// A workspace of plates, each named, with a catalog printer (or none) and boxes of the
    /// sizes given; the plates' identities and the folder, removed by the caller.
    fn plates_with(
        name: &str,
        plates: &[PlateOf<'_>],
    ) -> (Workspace, Vec<PlateId>, std::path::PathBuf) {
        let folder = std::env::temp_dir().join(format!("{name}-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let mut workspace = Workspace::new();
        let mut ids = Vec::new();
        for (k, &(plate_name, printer, boxes)) in plates.iter().enumerate() {
            if k > 0 {
                workspace.new_plate(plate_name.into());
            }
            let profiles = printer.map(catalog_profiles);
            workspace.edit(|plate| {
                plate.name = plate_name.into();
                plate.printer = profiles.as_ref().map(|p| p.0.clone());
                plate.resin = profiles.as_ref().map(|p| p.1.clone());
                plate.print_setting = profiles.as_ref().map(|p| p.2.clone());
            });
            for (j, &size) in boxes.iter().enumerate() {
                let file = folder.join(format!("box-{k}-{j}.stl"));
                std::fs::write(&file, box_stl([0.0; 3], size)).unwrap();
                workspace.open(file);
                wait(&mut workspace);
            }
            ids.push(workspace.plate_id());
        }
        (workspace, ids, folder)
    }

    /// Every plate's options: which plates, removing those left empty, and the overflow.
    fn every(plates: PlateChoice, remove_empty: bool, overflow: Overflow) -> ArrangeOptions {
        ArrangeOptions {
            scope: ArrangeScope::EveryPlate,
            plates,
            remove_empty,
            ..options(overflow, true, false)
        }
    }

    /// The tops of plate `id`'s models, to the micrometre.
    fn tops_on(workspace: &mut Workspace, id: PlateId) -> Vec<f64> {
        workspace.show_plate(id);
        workspace
            .placements()
            .iter()
            .map(|p| (p.as_ref().unwrap().max.z * 1e3).round() / 1e3)
            .collect()
    }

    #[test]
    fn every_plate_groups_the_models_of_several_plates_by_height() {
        // TD-162, phase 3: two plates of tall and short slabs, four to a plate; each keeps its
        // name and place, the first taking the tall ones.
        let tall = [90.0, 50.0, 100.0];
        let short = [90.0, 50.0, 20.0];
        let mixed: &[[f64; 3]] = &[tall, short, tall, short];
        let (mut workspace, ids, folder) = plates_with(
            "karasu-every-height",
            &[
                ("Left", Some("anycubic-photon-mono-x"), mixed),
                ("Right", Some("anycubic-photon-mono-x"), mixed),
            ],
        );
        workspace.show_plate(ids[0]);
        let options = every(PlateChoice::SamePrinter, false, Overflow::NewPlates);
        let arranged = workspace.arrange(options).unwrap();
        assert!(arranged.grouped, "{arranged:?}");
        assert_eq!(arranged.every, Some((8, 2, Vec::new())));
        assert_eq!(arranged.new_plates, Vec::<String>::new());
        assert_eq!(tops_on(&mut workspace, ids[0]), vec![100.0; 4]);
        assert_eq!(tops_on(&mut workspace, ids[1]), vec![20.0; 4]);
        let names: Vec<String> = workspace
            .plates()
            .iter()
            .map(|p| p.content().name.clone())
            .collect();
        assert_eq!(names, ["Left", "Right"]);
        // One step: undone, both plates are as they were.
        assert!(workspace.undo());
        assert_eq!(
            tops_on(&mut workspace, ids[0]),
            vec![100.0, 20.0, 100.0, 20.0]
        );
        assert_eq!(
            tops_on(&mut workspace, ids[1]),
            vec![100.0, 20.0, 100.0, 20.0]
        );
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn every_plate_takes_the_plates_chosen_and_each_model_where_its_printer_fits_it() {
        // TD-162, phase 3: a 200 mm tall box stands on an Elegoo Mars 4 plate (175 mm high);
        // a Photon Mono X plate (245 mm) follows, then a plate without a printer.
        let small = [20.0, 20.0, 10.0];
        let (mut workspace, ids, folder) = plates_with(
            "karasu-every-printers",
            &[
                ("Mars", Some("elegoo-mars-4"), &[[30.0, 30.0, 200.0], small]),
                ("Mono", Some("anycubic-photon-mono-x"), &[small]),
                ("Bare", None, &[small]),
            ],
        );
        workspace.show_plate(ids[1]);
        let (taken, left) =
            workspace.arranged_plates(ArrangeScope::EveryPlate, PlateChoice::SamePrinter);
        assert_eq!(taken, vec![ids[1]]);
        assert_eq!(
            left,
            vec![
                (ids[0], LeftOut::AnotherPrinter),
                (ids[2], LeftOut::NoPrinter)
            ]
        );
        let (taken, left) = workspace.arranged_plates(ArrangeScope::EveryPlate, PlateChoice::All);
        assert_eq!(taken, vec![ids[0], ids[1]]);
        assert_eq!(left, vec![(ids[2], LeftOut::NoPrinter)]);
        // All plates: the Mars plate, first, takes what it fits; the tall box goes to the Mono.
        let mut options = every(PlateChoice::All, false, Overflow::NewPlates);
        options.by_height = false;
        workspace.arrange(options).unwrap();
        assert_eq!(tops_on(&mut workspace, ids[0]), vec![10.0, 10.0]);
        assert_eq!(tops_on(&mut workspace, ids[1]), vec![200.0]);
        assert_eq!(tops_on(&mut workspace, ids[2]), vec![10.0]);
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn every_plate_removes_the_plates_it_empties_only_when_asked() {
        // TD-162, phase 3: three plates of one small box each, all of which fit the first.
        let one: &[[f64; 3]] = &[[30.0, 30.0, 10.0]];
        let printer = Some("anycubic-photon-mono-x");
        let (mut workspace, ids, folder) = plates_with(
            "karasu-every-empty",
            &[
                ("P1", printer, one),
                ("P2", printer, one),
                ("P3", printer, one),
            ],
        );
        workspace.show_plate(ids[0]);
        let kept = workspace
            .arrange(every(PlateChoice::SamePrinter, false, Overflow::NewPlates))
            .unwrap();
        assert_eq!(kept.every, Some((3, 3, Vec::new())));
        let counts = |w: &Workspace| -> Vec<usize> {
            w.plates()
                .iter()
                .map(|p| p.content().objects.len())
                .collect()
        };
        assert_eq!(counts(&workspace), vec![3, 0, 0]);
        assert!(workspace.undo());
        let removed = workspace
            .arrange(every(PlateChoice::SamePrinter, true, Overflow::NewPlates))
            .unwrap();
        assert_eq!(
            removed.every,
            Some((3, 3, vec!["P2".to_owned(), "P3".to_owned()]))
        );
        assert_eq!(counts(&workspace), vec![3]);
        // One step: undone, the plates come back with their models.
        assert!(workspace.undo());
        assert_eq!(counts(&workspace), vec![1, 1, 1]);
        assert_eq!(
            workspace
                .plates()
                .iter()
                .map(karasu_project::project::Plate::id)
                .collect::<Vec<_>>(),
            ids
        );
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn every_plate_keeps_a_model_that_fits_nowhere_and_packs_its_plate_around_it() {
        // TD-162, phase 3, "Stay where they are": a 250 mm slab fits no 192 mm plate; it stays
        // on its plate, and the boxes packed there keep clear of it.
        let small = [30.0, 30.0, 10.0];
        let printer = Some("anycubic-photon-mono-x");
        let (mut workspace, ids, folder) = plates_with(
            "karasu-every-stay",
            &[
                ("First", printer, &[[250.0, 40.0, 5.0], small, small]),
                ("Second", printer, &[small, small]),
            ],
        );
        workspace.show_plate(ids[0]);
        let slab = workspace.plate().objects[0].clone();
        let mut options = every(PlateChoice::SamePrinter, false, Overflow::Stay);
        options.by_height = false;
        let arranged = workspace.arrange(options).unwrap();
        assert_eq!(arranged.stayed, 1);
        workspace.show_plate(ids[0]);
        assert!(workspace.plate().objects.contains(&slab));
        assert_eq!(workspace.overlapping(), Vec::new());
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn arrange_groups_models_by_height_when_the_plates_print_sooner() {
        // TD-162: four slabs 100 mm tall and four 20 mm tall, 90 × 50 mm each, four to a
        // 192 × 120 mm plate, opened tall and short in turn.
        let folder =
            std::env::temp_dir().join(format!("karasu-workspace-height-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let tall = folder.join("tall.stl");
        std::fs::write(&tall, box_stl([0.0; 3], [90.0, 50.0, 100.0])).unwrap();
        let short = folder.join("short.stl");
        std::fs::write(&short, box_stl([0.0; 3], [90.0, 50.0, 20.0])).unwrap();
        let mut workspace = Workspace::new();
        let (printer, resin, setting) = catalog_profiles("anycubic-photon-mono-x");
        workspace.edit(|plate| {
            plate.name = "Arms".into();
            plate.printer = Some(printer);
            plate.resin = Some(resin);
            plate.print_setting = Some(setting);
        });
        for _ in 0..4 {
            for model in [&tall, &short] {
                workspace.open(model.clone());
                wait(&mut workspace);
            }
        }
        let tops = |w: &mut Workspace, id: PlateId| -> Vec<f64> {
            w.show_plate(id);
            let tops = w
                .placements()
                .iter()
                .map(|p| (p.as_ref().unwrap().max.z * 1e6).round() / 1e6)
                .collect();
            tops
        };
        let first = workspace.plate_id();
        // As packed today: by footprint, so tall and short share both plates.
        let packed = workspace
            .arrange(options(Overflow::NewPlates, false, false))
            .unwrap();
        assert_eq!(
            (packed.on_plate, packed.moved, packed.grouped),
            (4, 4, false)
        );
        let second = workspace.project().plates()[1].id();
        assert!(tops(&mut workspace, second).contains(&100.0));
        workspace.show_plate(first);
        assert!(workspace.undo());
        // Grouped: the tall ones on the plate shown, the short ones on the new plate.
        let grouped = workspace
            .arrange(options(Overflow::NewPlates, true, false))
            .unwrap();
        assert_eq!((grouped.on_plate, grouped.moved), (4, 4));
        assert!(grouped.grouped);
        assert_eq!(tops(&mut workspace, first), vec![100.0; 4]);
        let second = workspace.project().plates()[1].id();
        assert_eq!(tops(&mut workspace, second), vec![20.0; 4]);
        // What it saves: one plate of 100 mm printed as one of 20 mm.
        workspace.show_plate(first);
        let printer = workspace.plate().printer.clone().unwrap();
        let settings = workspace.plate().calibration().cloned().unwrap();
        let layers = |mm: f64| {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let layers = (mm / settings.layer_height_mm).ceil() as u64;
            layers
        };
        let saved = uniform_time_s(&printer, &settings, layers(100.0))
            - uniform_time_s(&printer, &settings, layers(20.0));
        #[allow(clippy::cast_precision_loss)]
        let reported = grouped.saved_s as f64;
        assert!((reported - saved).abs() <= 1.0, "{reported} {saved}");
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn arrange_packs_the_plate_and_takes_the_overflow_to_new_plates_or_leaves_it() {
        // TD-135, phase 5: six slabs of 90 × 50 mm on a 192 × 120 mm plate, four fit; a slab
        // larger than the plate fits nowhere.
        let folder =
            std::env::temp_dir().join(format!("karasu-workspace-arrange-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let slab = folder.join("slab.stl");
        std::fs::write(&slab, box_stl([0.0; 3], [90.0, 50.0, 3.0])).unwrap();
        let huge = folder.join("huge.stl");
        std::fs::write(&huge, box_stl([0.0; 3], [250.0, 50.0, 3.0])).unwrap();
        let mut workspace = Workspace::new();
        let (printer, resin, setting) = catalog_profiles("anycubic-photon-mono-x");
        workspace.edit(|plate| {
            plate.name = "Arms".into();
            plate.printer = Some(printer);
            plate.resin = Some(resin);
            plate.print_setting = Some(setting);
        });
        for _ in 0..6 {
            workspace.open(slab.clone());
            wait(&mut workspace);
        }
        workspace.open(huge.clone());
        wait(&mut workspace);
        let first = workspace.plate_id();
        assert_eq!(workspace.plate().objects.len(), 7);
        let revision = workspace.revision();
        let arranged = workspace
            .arrange(options(Overflow::NewPlates, false, false))
            .unwrap();
        assert_eq!(
            arranged,
            Arranged {
                on_plate: 4,
                moved: 2,
                new_plates: vec!["Arms (2)".into()],
                stayed: 1,
                removed: arranged.removed.clone(),
                grouped: false,
                saved_s: 0,
                turned: 0,
                every: None,
            }
        );
        assert_eq!(arranged.removed.len(), 2);
        assert!(arranged.removed.windows(2).all(|w| w[0] < w[1]));
        assert_eq!(workspace.plate_id(), first);
        assert_ne!(workspace.revision(), revision);
        let plates = workspace.project().plates();
        assert_eq!(plates.len(), 2);
        assert_eq!(plates[1].content().name, "Arms (2)");
        assert_eq!(plates[1].content().objects.len(), 2);
        assert_eq!(workspace.plate().objects.len(), 5);
        // The arranged slabs lie on the plate, the spacing apart.
        let content = workspace.plate().clone();
        let printer = content.printer.as_ref().unwrap();
        let centre = placement::plate_centre(printer);
        let rects: Vec<Rect> = content
            .objects
            .iter()
            .filter(|o| !o.pose.center)
            .map(|o| workspace.footprint(centre, o).unwrap().0)
            .collect();
        assert_eq!(rects.len(), 4);
        let [ox, oy] = printer.origin_xy_mm;
        for (k, r) in rects.iter().enumerate() {
            assert!(r.min[0] >= ox - 1e-6 && r.min[1] >= oy - 1e-6, "{r:?}");
            assert!(r.max[0] <= ox + printer.build_size_mm[0] + 1e-6, "{r:?}");
            assert!(r.max[1] <= oy + printer.build_size_mm[1] + 1e-6, "{r:?}");
            for q in &rects[k + 1..] {
                assert!(!r.within(q, 2.0 - 1e-6), "{r:?} {q:?}");
            }
        }
        // One step: undone whole.
        assert!(workspace.undo());
        assert_eq!(workspace.project().plates().len(), 1);
        assert_eq!(workspace.plate().objects.len(), 7);
        // Slabs all of one height: grouping by height saves nothing, so the packing is the
        // same (TD-162).
        let same = workspace
            .arrange(options(Overflow::NewPlates, true, false))
            .unwrap();
        assert_eq!(
            (same.on_plate, same.moved, same.stayed, same.grouped),
            (4, 2, 1, false)
        );
        assert!(workspace.undo());
        // Left where they are: no new plate, three stay.
        let arranged = workspace
            .arrange(options(Overflow::Stay, true, false))
            .unwrap();
        assert_eq!(
            (arranged.on_plate, arranged.moved, arranged.stayed),
            (4, 0, 3)
        );
        assert_eq!(arranged.new_plates.len(), 0);
        assert_eq!(workspace.project().plates().len(), 1);
        assert_eq!(workspace.plate().objects.len(), 7);
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn a_model_moves_or_copies_to_another_plate_where_it_is_or_beside_the_others() {
        // TD-135, phase 3: the object goes whole, at its place when that is free, beside the
        // models there otherwise; one undoable step. With no room, see the next test (TD-163).
        let folder =
            std::env::temp_dir().join(format!("karasu-workspace-transfer-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let cube = folder.join("cube.stl");
        std::fs::write(&cube, box_stl([0.0; 3], [10.0; 3])).unwrap();
        let huge = folder.join("huge.stl");
        std::fs::write(&huge, box_stl([0.0; 3], [100.0, 100.0, 5.0])).unwrap();
        let mut workspace = Workspace::new();
        let (printer, resin, setting) = catalog_profiles("anycubic-photon-mono-x");
        workspace.edit(|plate| {
            plate.printer = Some(printer);
            plate.resin = Some(resin);
            plate.print_setting = Some(setting);
        });
        workspace.open(cube.clone());
        wait(&mut workspace);
        let first = workspace.plate_id();
        workspace.new_plate("Second".into());
        let second = workspace.plate_id();
        workspace.open(cube.clone());
        wait(&mut workspace);
        workspace.show_plate(first);
        let footprint = |w: &Workspace, id: PlateId, index: usize| {
            let content = w.project().plate(id).unwrap().content();
            let centre = placement::plate_centre(content.printer.as_ref().unwrap());
            w.footprint(centre, &content.objects[index]).unwrap().0
        };
        let objects =
            |w: &Workspace, id: PlateId| w.project().plate(id).unwrap().content().objects.len();
        // A copy onto the second plate, whose cube is where this one is: beside it.
        assert_eq!(
            workspace.transfer(0, Some(second), true, String::new()),
            Ok(Landing::Beside)
        );
        assert_eq!(
            (objects(&workspace, first), objects(&workspace, second)),
            (1, 2)
        );
        let spacing = workspace.plate().settings.spacing_mm;
        let (there, landed) = (
            footprint(&workspace, second, 0),
            footprint(&workspace, second, 1),
        );
        assert!(!there.within(&landed, spacing), "{there:?} {landed:?}");
        let volume = workspace.build_volume().unwrap();
        assert!(landed.min[0] >= volume[0][0] && landed.max[0] <= volume[1][0]);
        assert!(landed.min[1] >= volume[0][1] && landed.max[1] <= volume[1][1]);
        // The object went whole: its name and pose, placed where it landed rather than
        // centred.
        let copied = &workspace.project().plate(second).unwrap().content().objects[1];
        let original = &workspace.plate().objects[0];
        assert_eq!(copied.name, original.name);
        assert_eq!(
            (
                copied.pose.rotate_deg,
                copied.pose.scale,
                copied.pose.center
            ),
            (original.pose.rotate_deg, original.pose.scale, false)
        );
        // Moved to a new plate: there at its own place, gone from the first plate, which stays
        // shown; undone, it is back.
        let before = footprint(&workspace, first, 0);
        assert_eq!(
            workspace.transfer(0, None, false, "Third".into()),
            Ok(Landing::Kept)
        );
        assert_eq!(workspace.plate_id(), first);
        assert_eq!(objects(&workspace, first), 0);
        let third = workspace.plates()[1].id();
        assert_eq!(workspace.plates()[1].content().name, "Third");
        assert_eq!(footprint(&workspace, third, 0), before);
        assert!(workspace.undo());
        assert_eq!(objects(&workspace, first), 1);
        assert!(workspace.project().plate(third).is_none());
        workspace.show_plate(second);
        assert_eq!(
            workspace.transfer(0, Some(second), false, String::new()),
            Err(Refused::Unknown)
        );
        let _ = std::fs::remove_dir_all(&folder);
    }

    /// A workspace whose plate has a 10 mm cube in its middle, and a second plate after it with
    /// a model wider than the plate: those plates and the folder, removed by the caller.
    fn crowded_plates(name: &str) -> (Workspace, PlateId, PlateId, std::path::PathBuf) {
        let folder = std::env::temp_dir().join(format!("{name}-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let cube = folder.join("cube.stl");
        std::fs::write(&cube, box_stl([0.0; 3], [10.0; 3])).unwrap();
        let mut workspace = Workspace::new();
        let (printer, resin, setting) = catalog_profiles("anycubic-photon-mono-x");
        workspace.edit(|plate| {
            plate.printer = Some(printer);
            plate.resin = Some(resin);
            plate.print_setting = Some(setting);
        });
        workspace.open(cube.clone());
        wait(&mut workspace);
        let first = workspace.plate_id();
        workspace.new_plate("Second".into());
        let second = workspace.plate_id();
        workspace.open(cube);
        wait(&mut workspace);
        workspace.edit(|plate| {
            let width = plate.printer.as_ref().unwrap().build_size_mm[0];
            plate.objects[0].pose.scale = [width / 8.0; 3];
        });
        (workspace, first, second, folder)
    }

    /// The middle of the plate shown's build area, and the centre of object `index`'s footprint.
    fn middle_and_centre(workspace: &Workspace, index: usize) -> ([f64; 2], [f64; 2]) {
        let printer = workspace.plate().printer.clone().unwrap();
        let [ox, oy] = printer.origin_xy_mm;
        let [width, depth, _] = printer.build_size_mm;
        let centre = placement::plate_centre(&printer);
        let object = workspace.plate().objects[index].clone();
        let (rect, _) = workspace.footprint(centre, &object).unwrap();
        (
            [ox + width / 2.0, oy + depth / 2.0],
            std::array::from_fn(|k| f64::midpoint(rect.min[k], rect.max[k])),
        )
    }

    #[test]
    fn a_model_with_no_room_lands_in_the_middle_over_the_models_there() {
        // TD-163 (owner, 2026-11-19): never refused; in the middle, the overlap found, one step.
        let (mut workspace, first, second, folder) = crowded_plates("karasu-workspace-middle");
        assert_eq!(
            workspace.transfer(0, Some(first), false, String::new()),
            Ok(Landing::Middle)
        );
        assert_eq!(workspace.plate().objects.len(), 0);
        workspace.show_plate(first);
        assert_eq!(workspace.plate().objects.len(), 2);
        let (middle, centre) = middle_and_centre(&workspace, 1);
        assert!(
            (0..2).all(|k| (centre[k] - middle[k]).abs() < 1e-6),
            "{centre:?}"
        );
        assert_eq!(workspace.overlapping(), vec![(0, 1)]);
        assert!(workspace.undo());
        workspace.show_plate(first);
        assert_eq!(workspace.plate().objects.len(), 1);
        assert_eq!(workspace.overlapping(), Vec::new());
        workspace.show_plate(second);
        assert_eq!(workspace.plate().objects.len(), 1);
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_clone_of_a_model_larger_than_an_empty_plate_lands_in_its_middle() {
        // TD-163: no longer refused, arranged or not; every copy in the middle, and said so.
        let (mut workspace, _, second, folder) = crowded_plates("karasu-workspace-big-clone");
        let cloned = workspace
            .clone_object(0, 2, Some(second), true, "unused".into())
            .unwrap();
        assert!(cloned.too_large);
        assert_eq!((cloned.on_plate, cloned.moved), (2, 0));
        assert_eq!(workspace.project().plates().len(), 2);
        assert_eq!(workspace.plate().objects.len(), 3);
        for index in 1..3 {
            let (middle, centre) = middle_and_centre(&workspace, index);
            assert!(
                (0..2).all(|k| (centre[k] - middle[k]).abs() < 1e-6),
                "{centre:?}"
            );
        }
        assert!(workspace.undo());
        assert_eq!(workspace.plate().objects.len(), 1);
        let _ = std::fs::remove_dir_all(&folder);
    }
    #[test]
    fn a_footprint_holds_the_supports_pads_beyond_the_model() {
        // TD-135, phase 3: what lands clear of the others is the whole print of the object.
        use karasu_core::supports::{AxisSpec, ManualSupport, Preset, SupportKind, SupportOrigin};
        let folder =
            std::env::temp_dir().join(format!("karasu-workspace-footprint-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let cube = folder.join("cube.stl");
        std::fs::write(&cube, box_stl([0.0; 3], [10.0; 3])).unwrap();
        let mut workspace = Workspace::new();
        workspace.open(cube);
        wait(&mut workspace);
        let object = workspace.plate().objects[0].clone();
        let (bare, _) = workspace.footprint([0.0; 2], &object).unwrap();
        assert_eq!(bare.size(), [10.0, 10.0]);
        // One support from the cube's side down to the plate 20 mm beyond it, in the model's
        // frame.
        let mut document = SupportDocument::new(Preset::Medium);
        let parameters = Preset::Medium.parameters();
        let pad = parameters.pad_diameter_mm / 2.0;
        document.supports.push(ManualSupport {
            id: 1,
            tip_mm: [10.0, 5.0, 5.0],
            tip_axis: AxisSpec::Normal,
            bends: Vec::new(),
            base: BaseSpec::Platform([30.0, 5.0]),
            parameters,
            accept_collision: true,
            origin: SupportOrigin::Placed,
            kind: SupportKind::Standard,
        });
        let supported = PlateObject {
            supports: Some(document),
            ..object
        };
        let (rect, _) = workspace.footprint([0.0; 2], &supported).unwrap();
        assert_eq!(rect.min, bare.min);
        assert!(
            (rect.max[0] - (bare.min[0] + 30.0 + pad)).abs() < 1e-9,
            "{rect:?}"
        );
        let _ = std::fs::remove_dir_all(&folder);
    }

    pub(crate) fn box_stl(min: [f64; 3], max: [f64; 3]) -> String {
        let corner = |i: usize| -> [f64; 3] {
            std::array::from_fn(|a| if i >> a & 1 == 1 { max[a] } else { min[a] })
        };
        let mut text = String::from("solid box\n");
        for q in [
            [0, 2, 3, 1],
            [4, 5, 7, 6],
            [0, 1, 5, 4],
            [2, 6, 7, 3],
            [0, 4, 6, 2],
            [1, 3, 7, 5],
        ] {
            for t in [[q[0], q[1], q[2]], [q[0], q[2], q[3]]] {
                text.push_str("facet normal 0 0 0\nouter loop\n");
                for i in t {
                    let [x, y, z] = corner(i);
                    writeln!(text, "vertex {x} {y} {z}").unwrap();
                }
                text.push_str("endloop\nendfacet\n");
            }
        }
        text + "endsolid box\n"
    }

    #[test]
    fn the_build_volume_runs_from_the_plate_at_its_origin_to_the_build_height() {
        let mut printer = test_printer();
        printer.origin_xy_mm = [-5.0, 10.0];
        printer.build_size_mm = [100.0, 60.0, 40.0];
        let mut workspace = Workspace::new();
        workspace.edit(|plate| plate.printer = Some(printer));
        assert_eq!(
            workspace.build_volume(),
            Some([[-5.0, 10.0, 0.0], [95.0, 70.0, 40.0]])
        );
    }

    #[test]
    #[allow(clippy::float_cmp)] // Colours are passed through unchanged.
    fn the_plate_is_drawn_from_the_printer() {
        let mut printer: PrinterProfile = test_printer();
        printer.origin_xy_mm = [-5.0, 10.0];
        printer.build_size_mm = [100.0, 60.0, 40.0];
        let (surface, marks) = plate_surface(&printer, [0.5; 4], [0.6; 4], [0.7; 4]);
        assert_eq!((surface.len(), marks.len()), (6, 30));
        assert!(surface.iter().chain(&marks).all(|v| v.position[2] == 0.0));
        let xs: Vec<f32> = surface.iter().map(|v| v.position[0]).collect();
        assert_eq!(xs.iter().copied().fold(f32::INFINITY, f32::min), -5.0);
        assert_eq!(xs.iter().copied().fold(f32::NEG_INFINITY, f32::max), 95.0);
        // The border (1.2 mm for a 100 mm plate) surrounds the plate, outside it.
        let border = plate_border_mm(&printer);
        assert!((border - 1.2).abs() < 1e-12);
        let ring = &marks[..24];
        assert!(ring.iter().all(|v| v.color == [0.6; 4]));
        let left = ring
            .iter()
            .map(|v| v.position[0])
            .fold(f32::INFINITY, f32::min);
        let back = ring
            .iter()
            .map(|v| v.position[1])
            .fold(f32::NEG_INFINITY, f32::max);
        assert!((left + 6.2).abs() < 1e-5 && (back - 71.2).abs() < 1e-5);
        // The front tab lies beyond the border at the front edge (Y = 10), centred on it, in its
        // own colour.
        let tab = &marks[24..];
        assert!(tab
            .iter()
            .all(|v| v.color == [0.7; 4] && v.position[1] <= 8.81));
        let front = FrontTab::of(&printer);
        assert_eq!(front.center[0], 45.0);
        assert!((front.long - 36.0).abs() < 1e-9 && front.short < front.long);
        assert!((front.corners()[0][0] - 27.0).abs() < 1e-12);
        assert!((front.corners()[1][1] - 8.8).abs() < 1e-12);
        // The word lies inside the tab, on the plate's plane, with valid atlas coordinates.
        let ctx = eframe::egui::Context::default();
        // A first pass loads the fonts; its texture changes go nowhere here.
        ctx.run_ui(eframe::egui::RawInput::default(), |_| {})
            .textures_delta
            .clear();
        let (label, size) = ctx.fonts_mut(|fonts| {
            let galley = fonts.layout_no_wrap(
                "Front".to_owned(),
                eframe::egui::FontId::proportional(64.0),
                eframe::egui::Color32::WHITE,
            );
            (
                front.label(&galley, fonts.font_image_size(), [1.0; 4]),
                fonts.font_image_size(),
            )
        });
        assert!(!label.is_empty() && label.len() % 3 == 0, "{size:?}");
        let [x0, y0] = front.corners()[3];
        let [x1, y1] = front.corners()[1];
        for v in &label {
            let [x, y, z] = v.position.map(f64::from);
            assert!(x > x0 && x < x1 && y > y0 && y < y1 && z == 0.0, "{v:?}");
            assert!(v.uv.iter().all(|t| (0.0..=1.0).contains(t)), "{v:?}");
        }
        let (grid, edges) = volume_lines(&printer, [0.1; 4], [0.5; 4], [0.9; 4]);
        // Nine X and five Y grid lines (X = 45 and Y = 60 are major), and twelve volume edges.
        assert_eq!((grid.len(), edges.len()), (2 * (9 + 5), 2 * 12));
        let majors: Vec<[f32; 3]> = grid
            .iter()
            .filter(|v| v.color == [0.5; 4])
            .map(|v| v.position)
            .collect();
        assert_eq!(
            majors,
            [
                [45.0, 10.0, 0.0],
                [45.0, 70.0, 0.0],
                [-5.0, 60.0, 0.0],
                [95.0, 60.0, 0.0]
            ]
        );
        assert!(edges.iter().all(|v| v.color == [0.9; 4]));
    }

    /// A printer profile for drawing tests.
    fn test_printer() -> PrinterProfile {
        PrinterProfile {
            schema_version: 1,
            id: "p".into(),
            name: "Printer".into(),
            revision: 1,
            build_size_mm: [10.0, 10.0, 10.0],
            resolution_px: [10, 10],
            origin_xy_mm: [0.0, 0.0],
            swap_xy: false,
            mirror_x: false,
            mirror_y: false,
            format_ids: Vec::new(),
            limits: karasu_core::profiles::PrinterLimits {
                min_layer_height_mm: 0.01,
                max_layer_height_mm: 0.3,
                max_exposure_s: 120.0,
                max_speed_mm_s: 100.0,
                max_lift_mm: 20.0,
            },
            vendor: None,
            model: None,
            based_on: None,
            fixed_layer_time_s: None,
        }
    }

    /// A catalog printer with its first catalog resin and print setting (TD-106).
    pub(crate) fn catalog_profiles(
        printer: &str,
    ) -> (
        karasu_core::profiles::PrinterProfile,
        karasu_core::profiles::ResinMaterial,
        karasu_core::profiles::PrintSettingProfile,
    ) {
        let catalog = karasu_project::library::Catalog::built_in().unwrap();
        let setting = catalog
            .settings()
            .iter()
            .find(|s| s.profile.printer_id == printer)
            .unwrap()
            .profile
            .clone();
        (
            catalog.printer(printer).unwrap().profile.clone(),
            catalog.material(&setting.resin_id).unwrap().profile.clone(),
            setting,
        )
    }

    pub(crate) fn wait(workspace: &mut Workspace) {
        for _ in 0..2000 {
            workspace.poll();
            if workspace.importing().next().is_none() && workspace.diagnosing().next().is_none() {
                return;
            }
            thread::sleep(std::time::Duration::from_millis(5));
        }
        panic!("import did not finish");
    }

    #[test]
    fn a_hidden_model_is_not_drawn_nor_picked_but_stays_on_the_plate() {
        let directory =
            std::env::temp_dir().join(format!("karasu-app-hidden-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let mut workspace = Workspace::new();
        for (name, x) in [("left.stl", -20.0), ("right.stl", 20.0)] {
            let path = directory.join(name);
            std::fs::write(&path, box_stl([x, 0.0, 0.0], [x + 10.0, 10.0, 10.0])).unwrap();
            workspace.open(path);
            wait(&mut workspace);
        }
        let down = |x: f64| Ray {
            origin: [x, 5.0, 50.0],
            direction: [0.0, 0.0, -1.0],
        };
        let drawn = |workspace: &mut Workspace| -> Vec<usize> {
            workspace
                .draws(|_| [1.0; 4], |_| [0, 0])
                .into_iter()
                .map(|(index, _)| index)
                .collect()
        };
        assert_eq!(drawn(&mut workspace), [0, 1]);
        let (history, revision) = (workspace.history(), workspace.revision());
        workspace.visibility().toggle(0);
        assert!(workspace.is_hidden(0) && !workspace.is_hidden(1));
        assert_eq!(workspace.hidden_count(), 1);
        assert_eq!(drawn(&mut workspace), [1]);
        assert_eq!(workspace.hit(&down(-15.0)), None);
        assert_eq!(workspace.hit(&down(25.0)).map(|h| h.object), Some(1));
        // Still on the plate, so still sliced; and hiding is no edit to undo.
        assert_eq!(workspace.plate().objects.len(), 2);
        assert_eq!(workspace.history(), history);
        assert_eq!(workspace.revision(), revision);
        // Isolating the hidden one swaps them; isolating it again restores.
        workspace.visibility().isolate(0);
        assert_eq!(drawn(&mut workspace), [0]);
        workspace.visibility().isolate(0);
        assert_eq!(drawn(&mut workspace), [1]);
        workspace.visibility().show_all();
        assert_eq!(drawn(&mut workspace), [0, 1]);
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn opened_models_are_embedded_placed_and_drawn() {
        let directory = std::env::temp_dir().join(format!("karasu-app-ws-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("box.stl");
        std::fs::write(&path, box_stl([-5.0, -5.0, 3.0], [5.0, 15.0, 13.0])).unwrap();
        let mut workspace = Workspace::new();
        workspace.open(path.clone());
        wait(&mut workspace);
        assert_eq!(workspace.plate().objects.len(), 1);
        assert_eq!(workspace.triangles(), 12);
        // STL declares no unit: millimetres, with a notice.
        assert_eq!(
            workspace.plate().objects[0].units,
            Some(LengthUnit::Millimeter)
        );
        let opened = &workspace.notices()[0];
        assert_eq!(opened.text, "Opened box.stl: no mesh defects found");
        assert!(opened.detail.as_ref().unwrap().contains("declares no unit"));
        // Without a printer the object is dropped onto the plate where it is.
        let placed = workspace.placements()[0].clone().unwrap();
        assert_eq!([placed.min.z, placed.max.z], [0.0, 10.0]);
        assert_eq!([placed.min.x, placed.max.y], [-5.0, 15.0]);
        // The draw matrix maps the GPU vertices (relative to the box centre) to the world.
        let (index, draw) = workspace.draws(|_| [1.0; 4], |_| [0, 0])[0];
        assert_eq!(index, 0);
        // A ray down onto the box picks it; one beside it picks nothing.
        let down = |x: f64, y: f64| Ray {
            origin: [x, y, 50.0],
            direction: [0.0, 0.0, -1.0],
        };
        assert_eq!(workspace.hit(&down(0.0, 5.0)).map(|h| h.object), Some(0));
        assert_eq!(workspace.hit(&down(20.0, 5.0)), None);
        // It turns about its centre, placed: (0, 5, 5).
        assert_eq!(workspace.anchor(0).unwrap().0, [0.0, 5.0, 5.0]);
        // Without a printer nothing is outside, and there is no volume to filter against.
        assert_eq!(workspace.outside(), Vec::<usize>::new());
        assert_eq!(workspace.build_volume(), None);
        // Lifted 4 mm, the box floats; pushed below the plate, it settles on it; dropped, it
        // rests again.
        workspace.edit(|plate| {
            let object = &mut plate.objects[0];
            object.pose.drop = false;
            object.translate_mm = [0.0, 0.0, 1.0];
        });
        assert_eq!(workspace.floating(), [0]);
        workspace.edit(|plate| plate.objects[0].translate_mm = [0.0, 0.0, -7.0]);
        workspace.settle(0, 1);
        assert_eq!(workspace.placements()[0].clone().unwrap().min.z, 0.0);
        workspace.edit(|plate| plate.objects[0].translate_mm = [0.0, 0.0, 9.0]);
        workspace.drop_to_plate(0);
        assert_eq!(workspace.floating(), Vec::<usize>::new());
        assert_eq!(
            workspace.hit(&down(0.0, 5.0)).map(|h| (h.point, h.normal)),
            Some(([0.0, 5.0, 10.0], [0.0, 0.0, 1.0]))
        );
        let corner = draw.model.project([5.0, 10.0, 5.0]).unwrap();
        assert_eq!(corner, [5.0, 15.0, 10.0]);
        let (min, max) = workspace.bounds().unwrap();
        assert_eq!((min, max), ([-5.0, -5.0, 0.0], [5.0, 15.0, 10.0]));
        // Opening the same bytes again embeds them once, as a second object.
        workspace.open(path.clone());
        wait(&mut workspace);
        assert_eq!(workspace.plate().objects.len(), 2);
        assert_eq!(workspace.meshes().len(), 1);
        // A broken profile is reported, not applied.
        let broken = directory.join("printer.json");
        std::fs::write(&broken, "{").unwrap();
        workspace.open(broken);
        assert_eq!(workspace.notices().last().unwrap().level, Level::Error);
        assert!(workspace.printer().is_none());
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn drain_holes_follow_their_object_and_no_other() {
        // TD-089: holes are stored in the model frame, so they follow without being touched.
        let directory =
            std::env::temp_dir().join(format!("karasu-app-holes-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("block.stl");
        std::fs::write(&path, box_stl([0.0; 3], [10.0, 10.0, 10.0])).unwrap();
        let mut workspace = Workspace::new();
        workspace.open(path.clone());
        workspace.open(path);
        wait(&mut workspace);
        let hole = |point| DrainHole::Surface {
            surface_mm: point,
            diameter_mm: 3.0,
            outside_mm: 1.0,
            inside_mm: 1.0,
            tilt: None,
            depth_mm: None,
            surface: karasu_core::preparation::sampled::HoleSurface::Outer,
        };
        // The second object, moved 20 mm along X, has a hole on its +X face, given in world
        // coordinates as the interface gives it.
        workspace.edit(|plate| {
            plate.objects[1].translate_mm = [20.0, 0.0, 0.0];
        });
        let model = workspace
            .model_to_world(1)
            .and_then(|map| map.inverse())
            .unwrap();
        workspace.add_holes(1, vec![map_hole(hole([30.0, 5.0, 5.0]), &model)]);
        let point = |w: &Workspace, i: usize| {
            let world = map_hole(w.holes(i)[0], &w.model_to_world(i).unwrap());
            match world {
                DrainHole::Surface { surface_mm, .. } => surface_mm,
                DrainHole::Axis { .. } => unreachable!(),
            }
        };
        let near = |a: [f64; 3], b: [f64; 3]| (0..3).all(|k| (a[k] - b[k]).abs() < 1e-9);
        // Stored in the model frame: the box's own +X face, at its origin.
        match workspace.plate().objects[1].drain_holes().next() {
            Some(DrainHole::Surface { surface_mm, .. }) => {
                assert!(near(*surface_mm, [10.0, 5.0, 5.0]), "{surface_mm:?}");
            }
            _ => unreachable!(),
        }
        assert!(near(point(&workspace, 1), [30.0, 5.0, 5.0]));
        // Moved by 5 mm and turned a quarter about Z: the hole stays on the same face.
        workspace.edit(|plate| {
            plate.objects[1].translate_mm[1] += 5.0;
            plate.objects[1].pose.rotate_deg[2] = 90.0;
        });
        let moved = point(&workspace, 1);
        let placement = workspace.placements()[1].clone().unwrap();
        // The +X face turned to +Y: the hole is on the placed box's +Y face, at mid-height.
        assert!(
            (moved[1] - placement.max.y).abs() < 1e-9,
            "{moved:?} {placement:?}"
        );
        assert!((moved[2] - 5.0).abs() < 1e-9);
        // A drag places the object frame by frame from the pose it had when the drag began;
        // the hole follows every frame and ends where the object ends.
        let start = workspace.plate().objects[1].clone();
        for step in 1..=5 {
            let mut pose = start.pose.clone();
            pose.rotate_deg[2] = 90.0;
            let mut translate = start.translate_mm;
            translate[0] += f64::from(step);
            workspace.place_object(1, translate, pose, 42);
        }
        let dragged = point(&workspace, 1);
        assert!(
            near(dragged, [moved[0] + 5.0, moved[1], moved[2]]),
            "{dragged:?}"
        );
        workspace.undo();
        assert!(near(point(&workspace, 1), moved));
        // Removing the first object shifts the second down the list; its hole does not move.
        workspace.edit(|plate| {
            plate.objects.remove(0);
        });
        assert!(near(point(&workspace, 0), moved));
        // Undo restores both in one step.
        workspace.undo();
        assert!(near(point(&workspace, 1), moved));
        // Every hole of a model goes in one step, the other model's kept; undo brings them back.
        workspace.add_holes(1, vec![map_hole(hole([30.0, 5.0, 2.0]), &model)]);
        workspace.add_holes(0, vec![hole([10.0, 5.0, 5.0])]);
        assert_eq!(workspace.holes(1).len(), 2);
        workspace.remove_holes(1);
        assert_eq!((workspace.holes(1).len(), workspace.holes(0).len()), (0, 1));
        workspace.undo();
        assert_eq!(workspace.holes(1).len(), 2);
        let _ = std::fs::remove_dir_all(&directory);
    }
}
