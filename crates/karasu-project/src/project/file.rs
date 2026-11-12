//! The project file (TD-130): a project saved as a `.karasu` package and opened again.
//!
//! **Package.** A ZIP archive ([`karasu_core::archive`]) recognised by its content:
//! - [`DOCUMENT_ENTRY`], the first entry: the project document, JSON of [`FORMAT`] version
//!   [`VERSION`];
//! - `models/<sha256>.<ext>`: each model file the plates use, byte for byte as it was opened,
//!   `<ext>` the lower-case extension of its original name. Models no plate uses are not written.
//!
//! **Document.** The project's name and its plates in order, each with its identity, name, the
//! snapshots of its printer, resin and print setting (their profile JSON), its settings and its
//! objects: name, model, units, translation, pose, operations with their identities and switches,
//! supports (their `karasu-supports` JSON), collision acceptance, copies, arrangement and the
//! Orient tool's protected points.
//!
//! **Not saved:** each plate's undo history (each plate opened starts at a new revision with none), and
//! everything computed from the project: diagnoses, cavities, previews, slices, which the
//! application computes again, and the view.
//!
//! **Deterministic.** Fields in a fixed order, numbers in their shortest round-trip form, the
//! models in the project's order and fixed archive timestamps: the same project saves to the same
//! bytes.
//!
//! **Opening is checked, never repaired.** Unknown fields and entries are refused; a newer
//! version is refused as such; each model must hash to the identity its entry and the document
//! give, and be used; every value is checked as an edit checks it, and the first one refused is
//! reported with its plate, object and field. Nothing is extracted to disk.
use super::{
    check_operations, History, Hollowing, ImageOverride, LibraryModel, ModelId, ObjectOperation,
    OperationKind, Plate, PlateContent, PlateId, PlateObject, PlateSettings, Project,
    ProtectedPoint,
};
use crate::request::{
    machine_exporter, machine_exporter_with, Blocker, DrainHole, Fill, Pose, Simplify,
    MAX_REVERSED_TOLERANCE_MM2,
};
use karasu_core::{
    archive::{ArchiveError, ArchiveLimits, ArchiveReader, ArchiveWriter},
    digest::sha256,
    layer_plan::LayerPlanDocument,
    mesh::formats::LengthUnit,
    plate::MAX_COPIES,
    preparation::{
        repair::{FillHoles, NonManifold, RepairSpec, Shells},
        sampled::HoleSurface,
        HoleTilt, MAX_HOLE_TILT_DEG,
    },
    profiles::{PrintSettingProfile, PrinterProfile, ResinMaterial},
    supports::SupportDocument,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeSet,
    fmt,
    fs::File,
    io::{self, BufWriter, Write},
    path::{Path, PathBuf},
    sync::{atomic::AtomicU64, Arc},
};

/// The project file's extension, without its dot.
pub const EXTENSION: &str = "karasu";
/// The document's format name.
pub const FORMAT: &str = "karasu-project";
/// The document version this build writes and reads.
pub const VERSION: u32 = 1;
/// The project document's entry.
pub const DOCUMENT_ENTRY: &str = "karasu-project.json";
/// Largest project document, in bytes.
pub const MAX_DOCUMENT_BYTES: u64 = 512 * 1024 * 1024;
/// Largest model file, in bytes.
pub const MAX_MODEL_BYTES: u64 = 8 * 1024 * 1024 * 1024;
/// Most models in one project.
pub const MAX_MODELS: usize = 4096;
/// Largest project file opened, in bytes: it is read into memory whole.
pub const MAX_PACKAGE_BYTES: u64 = 16 * 1024 * 1024 * 1024;
/// Coordinates and translations must lie within this many millimetres of the origin.
const DOMAIN_MM: f64 = 10_000.0;

/// Why a project could not be saved or opened.
#[derive(Debug)]
pub enum ProjectFileError {
    /// The file could not be read or written.
    Io {
        /// The file.
        path: PathBuf,
        /// The cause.
        error: io::Error,
    },
    /// The package is not a readable ZIP archive.
    Archive(ArchiveError),
    /// The file is not a karasu-slicer project.
    NotAProject(String),
    /// A newer karasu-slicer wrote the file.
    Newer {
        /// The document's version.
        version: u32,
    },
    /// The document is not well-formed JSON of the project's schema.
    Json(String),
    /// A value is refused, where it is.
    Invalid {
        /// Where: the plate, object and field.
        at: String,
        /// Why.
        reason: String,
    },
}

impl fmt::Display for ProjectFileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, error } => write!(f, "{}: {error}", path.display()),
            Self::Archive(e) => write!(f, "the project file cannot be read: {e}"),
            Self::NotAProject(reason) => {
                write!(f, "not a karasu-slicer project: {reason}")
            }
            Self::Newer { version } => write!(
                f,
                "the project was saved by a newer karasu-slicer (project version {version}; this \
                 version reads {VERSION})"
            ),
            Self::Json(e) => write!(f, "the project document is invalid: {e}"),
            Self::Invalid { at, reason } => write!(f, "{at}: {reason}"),
        }
    }
}

impl std::error::Error for ProjectFileError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { error, .. } => Some(error),
            Self::Archive(e) => Some(e),
            _ => None,
        }
    }
}

fn invalid(at: &str, reason: impl Into<String>) -> ProjectFileError {
    ProjectFileError::Invalid {
        at: at.to_owned(),
        reason: reason.into(),
    }
}

// ---------------------------------------------------------------------------------------------
// The document's schema.
// ---------------------------------------------------------------------------------------------

/// Just the format and version, read first so a newer document is refused as such.
#[derive(Deserialize)]
struct Header {
    format: Option<String>,
    version: Option<u32>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    format: String,
    version: u32,
    name: String,
    /// The identity the next new plate takes.
    next_plate: u64,
    models: Vec<ModelEntry>,
    plates: Vec<PlateEntry>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ModelEntry {
    sha256: String,
    /// The original file name; its extension selects the import format.
    name: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PlateEntry {
    id: u64,
    name: String,
    printer: Option<PrinterProfile>,
    resin: Option<ResinMaterial>,
    print_setting: Option<PrintSettingProfile>,
    settings: SettingsEntry,
    objects: Vec<ObjectEntry>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SettingsEntry {
    format: String,
    job_id: String,
    fill: FillEntry,
    close_holes_mm: Option<f64>,
    reversed_tolerance_mm2: Option<f64>,
    repair: RepairEntry,
    allow_topology_defects: bool,
    /// A `karasu-layers` document.
    layer_plan: Option<Value>,
    spacing_mm: f64,
    /// The plate's own mask settings (TD-134), written only when it has any, so a project
    /// without them is written as before.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    image: Option<ImageEntry>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ImageEntry {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    antialias: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    min_gray: Option<u8>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    antialias_supports: bool,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum FillEntry {
    Union,
    Strict,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(clippy::struct_excessive_bools)] // The repair's own switches.
struct RepairEntry {
    duplicates: bool,
    orientation: bool,
    outward: bool,
    fill_holes: Option<FillHolesEntry>,
    allow_unfilled: bool,
    weld_mm: Option<f64>,
    degenerate_mm: Option<f64>,
    crack_mm: Option<f64>,
    non_manifold: Option<NonManifoldEntry>,
    visibility: bool,
    shells: Option<ShellsEntry>,
    rebuild_mm: Option<f64>,
    non_finite: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FillHolesEntry {
    max_hole_edges: u64,
    split_rims: bool,
    refine: bool,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum NonManifoldEntry {
    Fins,
    Aggressive,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ShellsEntry {
    min_area_mm2: f64,
    min_volume_mm3: f64,
    inner: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ObjectEntry {
    name: String,
    model: String,
    units: Option<UnitEntry>,
    translate_mm: [f64; 3],
    pose: PoseEntry,
    operations: Vec<OperationEntry>,
    /// A `karasu-supports` document.
    supports: Option<Value>,
    accept_support_collisions: bool,
    copies: u32,
    arrange: bool,
    protected: Vec<ProtectedEntry>,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum UnitEntry {
    Micrometer,
    Millimeter,
    Centimeter,
    Inch,
    Foot,
    Meter,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PoseEntry {
    rotate_deg: [f64; 3],
    scale: [f64; 3],
    mirror: [bool; 3],
    center: bool,
    drop: bool,
    lift_mm: Option<f64>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct OperationEntry {
    id: u64,
    enabled: bool,
    #[serde(rename = "do")]
    kind: OperationKindEntry,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
enum OperationKindEntry {
    Repair(RepairEntry),
    Hollow(HollowEntry),
    DrainHole(HoleEntry),
    Blocker(BlockerEntry),
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct HollowEntry {
    wall_mm: f64,
    cell_mm: Option<f64>,
    simplify: Option<SimplifyEntry>,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
enum SimplifyEntry {
    Auto,
    Mm(f64),
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
enum HoleEntry {
    Axis {
        entry_mm: [f64; 3],
        direction: [f64; 3],
        length_mm: f64,
        diameter_mm: f64,
    },
    Surface {
        surface_mm: [f64; 3],
        diameter_mm: f64,
        outside_mm: f64,
        inside_mm: f64,
        tilt: Option<TiltEntry>,
        depth_mm: Option<f64>,
        from: SurfaceEntry,
    },
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TiltEntry {
    angle_deg: f64,
    azimuth_deg: f64,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SurfaceEntry {
    Outer,
    Cavity,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
enum BlockerEntry {
    Frustum {
        centre_mm: [f64; 3],
        rotation_deg: [f64; 3],
        bottom_mm: [f64; 2],
        top_mm: [f64; 2],
        height_mm: f64,
    },
    Sphere {
        centre_mm: [f64; 3],
        diameter_mm: f64,
    },
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProtectedEntry {
    point_mm: [f64; 3],
    normal: [f64; 3],
}

// ---------------------------------------------------------------------------------------------
// Writing.
// ---------------------------------------------------------------------------------------------

/// The hexadecimal form of a model's identity.
fn hex(id: ModelId) -> String {
    id.to_string()
}

/// The entry of a model: its identity and the lower-case extension of its original name, when
/// it has a plain one.
fn model_entry_name(model: &LibraryModel) -> String {
    let extension = Path::new(&model.name)
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .filter(|e| !e.is_empty() && e.len() <= 8 && e.bytes().all(|b| b.is_ascii_alphanumeric()));
    match extension {
        Some(extension) => format!("models/{}.{extension}", hex(model.id)),
        None => format!("models/{}", hex(model.id)),
    }
}

/// A document of the core as a JSON value, nested in the project's.
fn nested(json: &[u8]) -> Value {
    // The core's writers produce valid JSON: reading it back cannot fail.
    serde_json::from_slice(json).expect("a core document is valid JSON")
}

const fn fill_entry(fill: Fill) -> FillEntry {
    match fill {
        Fill::Union => FillEntry::Union,
        Fill::Strict => FillEntry::Strict,
    }
}

fn repair_entry(spec: &RepairSpec) -> RepairEntry {
    let RepairSpec {
        duplicates,
        orientation,
        outward,
        fill_holes,
        allow_unfilled,
        weld_mm,
        degenerate_mm,
        crack_mm,
        non_manifold,
        visibility,
        shells,
        rebuild_mm,
        non_finite,
    } = *spec;
    RepairEntry {
        duplicates,
        orientation,
        outward,
        fill_holes: fill_holes.map(|f| {
            let FillHoles {
                max_hole_edges,
                split_rims,
                refine,
            } = f;
            FillHolesEntry {
                max_hole_edges: max_hole_edges as u64,
                split_rims,
                refine,
            }
        }),
        allow_unfilled,
        weld_mm,
        degenerate_mm,
        crack_mm,
        non_manifold: non_manifold.map(|n| match n {
            NonManifold::Fins => NonManifoldEntry::Fins,
            NonManifold::Aggressive => NonManifoldEntry::Aggressive,
        }),
        visibility,
        shells: shells.map(|s| {
            let Shells {
                min_area_mm2,
                min_volume_mm3,
                inner,
            } = s;
            ShellsEntry {
                min_area_mm2,
                min_volume_mm3,
                inner,
            }
        }),
        rebuild_mm,
        non_finite,
    }
}

const fn unit_entry(unit: LengthUnit) -> UnitEntry {
    match unit {
        LengthUnit::Micrometer => UnitEntry::Micrometer,
        LengthUnit::Millimeter => UnitEntry::Millimeter,
        LengthUnit::Centimeter => UnitEntry::Centimeter,
        LengthUnit::Inch => UnitEntry::Inch,
        LengthUnit::Foot => UnitEntry::Foot,
        LengthUnit::Meter => UnitEntry::Meter,
    }
}

fn operation_entry(operation: &ObjectOperation) -> OperationEntry {
    let kind = match &operation.kind {
        OperationKind::Repair(spec) => OperationKindEntry::Repair(repair_entry(spec)),
        OperationKind::Hollow(h) => OperationKindEntry::Hollow(HollowEntry {
            wall_mm: h.wall_mm,
            cell_mm: h.cell_mm,
            simplify: h.simplify.map(|s| match s {
                Simplify::Auto => SimplifyEntry::Auto,
                Simplify::Millimeters(mm) => SimplifyEntry::Mm(mm),
            }),
        }),
        OperationKind::DrainHole(hole) => OperationKindEntry::DrainHole(match *hole {
            DrainHole::Axis {
                entry_mm,
                direction,
                length_mm,
                diameter_mm,
            } => HoleEntry::Axis {
                entry_mm,
                direction,
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
            } => HoleEntry::Surface {
                surface_mm,
                diameter_mm,
                outside_mm,
                inside_mm,
                tilt: tilt.map(|t| TiltEntry {
                    angle_deg: t.angle_deg,
                    azimuth_deg: t.azimuth_deg,
                }),
                depth_mm,
                from: match surface {
                    HoleSurface::Outer => SurfaceEntry::Outer,
                    HoleSurface::Cavity => SurfaceEntry::Cavity,
                },
            },
        }),
        OperationKind::Blocker(blocker) => OperationKindEntry::Blocker(match *blocker {
            Blocker::Frustum {
                centre_mm,
                rotation_deg,
                bottom_mm,
                top_mm,
                height_mm,
            } => BlockerEntry::Frustum {
                centre_mm,
                rotation_deg,
                bottom_mm,
                top_mm,
                height_mm,
            },
            Blocker::Sphere {
                centre_mm,
                diameter_mm,
            } => BlockerEntry::Sphere {
                centre_mm,
                diameter_mm,
            },
        }),
    };
    OperationEntry {
        id: operation.id,
        enabled: operation.enabled,
        kind,
    }
}

fn object_entry(object: &PlateObject) -> ObjectEntry {
    let Pose {
        rotate_deg,
        scale,
        mirror,
        center,
        drop,
        lift_mm,
    } = object.pose;
    ObjectEntry {
        name: object.name.clone(),
        model: hex(object.model),
        units: object.units.map(unit_entry),
        translate_mm: object.translate_mm,
        pose: PoseEntry {
            rotate_deg,
            scale,
            mirror,
            center,
            drop,
            lift_mm,
        },
        operations: object.operations.iter().map(operation_entry).collect(),
        supports: object.supports.as_ref().map(|s| nested(&s.to_json())),
        accept_support_collisions: object.accept_support_collisions,
        copies: object.copies,
        arrange: object.arrange,
        protected: object
            .protected
            .iter()
            .map(|p| ProtectedEntry {
                point_mm: p.point_mm,
                normal: p.normal,
            })
            .collect(),
    }
}

fn plate_entry(plate: &Plate) -> PlateEntry {
    let content = &plate.content;
    let s = &content.settings;
    PlateEntry {
        id: plate.id.0,
        name: content.name.clone(),
        printer: content.printer.clone(),
        resin: content.resin.clone(),
        print_setting: content.print_setting.clone(),
        settings: SettingsEntry {
            format: s.format.clone(),
            job_id: s.job_id.clone(),
            fill: fill_entry(s.fill),
            close_holes_mm: s.close_holes_mm,
            reversed_tolerance_mm2: s.reversed_tolerance_mm2,
            repair: repair_entry(&s.repair),
            allow_topology_defects: s.allow_topology_defects,
            layer_plan: s.layer_plan.as_ref().map(|p| nested(&p.to_json())),
            spacing_mm: s.spacing_mm,
            image: (s.image != ImageOverride::default()).then_some(ImageEntry {
                antialias: s.image.antialias,
                min_gray: s.image.min_gray,
                antialias_supports: s.image.antialias_supports,
            }),
        },
        objects: content.objects.iter().map(object_entry).collect(),
    }
}

/// The models the plates use, in the project's order.
fn used_models(project: &Project) -> Vec<&LibraryModel> {
    let used: BTreeSet<ModelId> = project
        .plates
        .iter()
        .flat_map(|p| p.content.objects.iter().map(|o| o.model))
        .collect();
    project
        .models
        .iter()
        .filter(|m| used.contains(&m.id))
        .collect()
}

/// The project's document, as it is written.
fn document_bytes(project: &Project, models: &[&LibraryModel]) -> Vec<u8> {
    let document = Document {
        format: FORMAT.to_owned(),
        version: VERSION,
        name: project.name.clone(),
        next_plate: project.next_plate,
        models: models
            .iter()
            .map(|m| ModelEntry {
                sha256: hex(m.id),
                name: m.name.clone(),
            })
            .collect(),
        plates: project.plates.iter().map(plate_entry).collect(),
    };
    // Strings, numbers and the core's documents: serialising them cannot fail.
    serde_json::to_vec(&document).expect("a project document always serialises")
}

/// Write `project` as a package to `out`, and return it.
///
/// # Errors
/// [`ProjectFileError::Io`] when writing fails (its path is empty: the caller knows it).
pub fn write<W: Write>(project: &Project, out: W) -> Result<W, ProjectFileError> {
    let io = |error| ProjectFileError::Io {
        path: PathBuf::new(),
        error,
    };
    let models = used_models(project);
    let mut archive = ArchiveWriter::new(out);
    archive
        .add(DOCUMENT_ENTRY, &document_bytes(project, &models))
        .map_err(io)?;
    for model in models {
        archive
            .add(&model_entry_name(model), &model.bytes)
            .map_err(io)?;
    }
    archive.finish().map_err(io)
}

/// The package of `project`, in memory.
///
/// # Panics
/// Never: writing to memory cannot fail.
#[must_use]
pub fn to_bytes(project: &Project) -> Vec<u8> {
    // Writing to memory cannot fail.
    write(project, Vec::new()).expect("writing to memory cannot fail")
}

/// Temporary files of saves in progress, told apart within one process.
static TEMPORARY: AtomicU64 = AtomicU64::new(0);

/// Save `project` to `path`, atomically: it is written to a temporary file beside `path`, then
/// renamed over it, so a failed or interrupted save leaves the previous file whole.
///
/// # Errors
/// [`ProjectFileError::Io`] naming the file that could not be written or replaced.
pub fn save(project: &Project, path: &Path) -> Result<(), ProjectFileError> {
    let io = |path: &Path| {
        let path = path.to_path_buf();
        move |error| ProjectFileError::Io { path, error }
    };
    let name = path
        .file_name()
        .ok_or_else(|| ProjectFileError::Io {
            path: path.to_path_buf(),
            error: io::Error::new(io::ErrorKind::InvalidInput, "not a file name"),
        })?
        .to_string_lossy();
    let counter = TEMPORARY.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let temporary = path.with_file_name(format!(".{name}.{}-{counter}.tmp", std::process::id()));
    let result = (|| {
        let file = File::create(&temporary).map_err(io(&temporary))?;
        let mut out = write(project, BufWriter::new(file)).map_err(|e| match e {
            ProjectFileError::Io { error, .. } => ProjectFileError::Io {
                path: temporary.clone(),
                error,
            },
            other => other,
        })?;
        out.flush().map_err(io(&temporary))?;
        let file = out.into_inner().map_err(|e| ProjectFileError::Io {
            path: temporary.clone(),
            error: e.into_error(),
        })?;
        file.sync_all().map_err(io(&temporary))?;
        drop(file);
        std::fs::rename(&temporary, path).map_err(io(path))
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

// ---------------------------------------------------------------------------------------------
// Reading.
// ---------------------------------------------------------------------------------------------

/// A project's entries unpacked in a folder: the document, and the models under `models/`.
struct Folder<'a>(&'a Path);

impl Folder<'_> {
    fn path(&self, name: &str) -> PathBuf {
        name.split('/')
            .fold(self.0.to_path_buf(), |p, part| p.join(part))
    }
}

impl Entries for Folder<'_> {
    fn names(&self) -> Vec<String> {
        let files = |dir: &Path| -> Vec<String> {
            std::fs::read_dir(dir)
                .into_iter()
                .flatten()
                .flatten()
                .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        };
        let mut names = files(self.0);
        names.extend(
            files(&self.0.join("models"))
                .into_iter()
                .map(|n| format!("models/{n}")),
        );
        names
    }

    fn read(&self, name: &str) -> Result<Option<Vec<u8>>, ProjectFileError> {
        let path = self.path(name);
        match std::fs::read(&path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(ProjectFileError::Io { path, error }),
        }
    }
}

/// Write `bytes` to `path` through a temporary file beside it, renamed over it.
fn replace_file(path: &Path, bytes: &[u8]) -> Result<(), ProjectFileError> {
    let io = |path: &Path| {
        let path = path.to_path_buf();
        move |error| ProjectFileError::Io { path, error }
    };
    let counter = TEMPORARY.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let temporary = path.with_extension(format!("{}-{counter}.tmp", std::process::id()));
    let result = std::fs::write(&temporary, bytes)
        .map_err(io(&temporary))
        .and_then(|()| std::fs::rename(&temporary, path).map_err(io(path)));
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

/// Keep `project` unpacked in the folder `dir`, as a recovery copy does (TD-130): the document
/// rewritten each time, each model written once, as it is not there yet, and the models the
/// project no longer uses removed. Each file is replaced whole, so an interrupted write leaves
/// the previous copy readable.
///
/// # Errors
/// [`ProjectFileError::Io`] naming the file that could not be written.
pub fn save_folder(project: &Project, dir: &Path) -> Result<(), ProjectFileError> {
    let folder = Folder(dir);
    let models_dir = dir.join("models");
    std::fs::create_dir_all(&models_dir).map_err(|error| ProjectFileError::Io {
        path: models_dir.clone(),
        error,
    })?;
    let models = used_models(project);
    let names: BTreeSet<String> = models.iter().map(|m| model_entry_name(m)).collect();
    for model in &models {
        let path = folder.path(&model_entry_name(model));
        if !path.is_file() {
            replace_file(&path, &model.bytes)?;
        }
    }
    for name in folder.names() {
        if name.starts_with("models/") && !names.contains(&name) {
            let _ = std::fs::remove_file(folder.path(&name));
        }
    }
    replace_file(
        &folder.path(DOCUMENT_ENTRY),
        &document_bytes(project, &models),
    )
}

/// The project kept unpacked in the folder `dir` (see [`save_folder`]), checked as a package is.
///
/// # Errors
/// The errors of [`from_bytes`], and [`ProjectFileError::Io`] for a file that cannot be read.
pub fn open_folder(dir: &Path) -> Result<Project, ProjectFileError> {
    read_entries(&Folder(dir))
}

/// Read `path` and open the project it holds (see [`from_bytes`]).
///
/// # Errors
/// [`ProjectFileError::Io`] when the file cannot be read or exceeds [`MAX_PACKAGE_BYTES`], and
/// every error of [`from_bytes`].
pub fn open(path: &Path) -> Result<Project, ProjectFileError> {
    let io = |error| ProjectFileError::Io {
        path: path.to_path_buf(),
        error,
    };
    let size = std::fs::metadata(path).map_err(io)?.len();
    if size > MAX_PACKAGE_BYTES {
        return Err(io(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("the file exceeds {MAX_PACKAGE_BYTES} bytes"),
        )));
    }
    from_bytes(&std::fs::read(path).map_err(io)?)
}

/// The project a package holds, checked (see the module).
///
/// # Errors
/// [`ProjectFileError::Archive`] for a package that is not a readable archive,
/// [`ProjectFileError::NotAProject`] for one without a project document,
/// [`ProjectFileError::Newer`] for a document of a newer version, [`ProjectFileError::Json`] for
/// a document that does not follow the schema, and [`ProjectFileError::Invalid`] for the first
/// value, model or entry refused.
pub fn from_bytes(bytes: &[u8]) -> Result<Project, ProjectFileError> {
    let limits = ArchiveLimits {
        max_entries: MAX_MODELS + 1,
        max_entry_bytes: MAX_MODEL_BYTES.max(MAX_DOCUMENT_BYTES),
    };
    let archive = ArchiveReader::open(bytes, limits).map_err(|e| match e {
        ArchiveError::NotAnArchive => {
            ProjectFileError::NotAProject("it is not a ZIP package".to_owned())
        }
        other => ProjectFileError::Archive(other),
    })?;
    read_entries(&archive)
}

/// Where a project's entries are read from: a package, or a recovery folder.
trait Entries {
    /// Every entry's name.
    fn names(&self) -> Vec<String>;
    /// The entry named exactly `name`, if there is one.
    fn read(&self, name: &str) -> Result<Option<Vec<u8>>, ProjectFileError>;
}

impl Entries for ArchiveReader<'_> {
    fn names(&self) -> Vec<String> {
        ArchiveReader::names(self).map(str::to_owned).collect()
    }

    fn read(&self, name: &str) -> Result<Option<Vec<u8>>, ProjectFileError> {
        ArchiveReader::read(self, name).map_err(ProjectFileError::Archive)
    }
}

/// The project `entries` hold, checked (see the module).
fn read_entries(entries: &dyn Entries) -> Result<Project, ProjectFileError> {
    let document = entries
        .read(DOCUMENT_ENTRY)?
        .ok_or_else(|| ProjectFileError::NotAProject(format!("it has no {DOCUMENT_ENTRY}")))?;
    if document.len() as u64 > MAX_DOCUMENT_BYTES {
        return Err(invalid(
            DOCUMENT_ENTRY,
            format!("the document exceeds {MAX_DOCUMENT_BYTES} bytes"),
        ));
    }
    let header: Header =
        serde_json::from_slice(&document).map_err(|e| ProjectFileError::Json(e.to_string()))?;
    if header.format.as_deref() != Some(FORMAT) {
        return Err(ProjectFileError::NotAProject(format!(
            "its document is not of the {FORMAT:?} format"
        )));
    }
    match header.version {
        Some(VERSION) => {}
        Some(version) if version > VERSION => return Err(ProjectFileError::Newer { version }),
        other => {
            return Err(invalid(
                DOCUMENT_ENTRY,
                format!("unknown document version {other:?}"),
            ))
        }
    }
    let document: Document =
        serde_json::from_slice(&document).map_err(|e| ProjectFileError::Json(e.to_string()))?;
    let models = read_models(entries, &document)?;
    let mut project = Project {
        name: document.name,
        models,
        plates: Vec::with_capacity(document.plates.len()),
        next_plate: document.next_plate,
        revision: 0,
        structure: super::Structure::default(),
    };
    let mut ids = BTreeSet::new();
    for (index, entry) in (1..).zip(document.plates) {
        let at = format!("plate {index} ({:?})", entry.name);
        if entry.id == 0 || entry.id >= project.next_plate || !ids.insert(entry.id) {
            return Err(invalid(
                &at,
                format!(
                    "its identity {} must be nonzero, unique and below next_plate {}",
                    entry.id, project.next_plate
                ),
            ));
        }
        let id = PlateId(entry.id);
        let content = read_plate(entry, &at, &project)?;
        let revision = project.next_revision();
        project.plates.push(Plate {
            id,
            revision,
            content,
            history: History::default(),
        });
    }
    let used: BTreeSet<ModelId> = project
        .plates
        .iter()
        .flat_map(|p| p.content.objects.iter().map(|o| o.model))
        .collect();
    if let Some(unused) = project.models.iter().find(|m| !used.contains(&m.id)) {
        return Err(invalid(
            &model_entry_name(unused),
            "no plate uses this model",
        ));
    }
    Ok(project)
}

/// The models the document lists, each read from its entry and checked against its identity;
/// any other entry is refused.
fn read_models(
    archive: &dyn Entries,
    document: &Document,
) -> Result<Vec<LibraryModel>, ProjectFileError> {
    let mut models = Vec::with_capacity(document.models.len());
    let mut entries = BTreeSet::from([DOCUMENT_ENTRY.to_owned()]);
    for listed in &document.models {
        let at = format!("model {:?}", listed.name);
        let id = parse_model_id(&listed.sha256)
            .ok_or_else(|| invalid(&at, "its sha256 must be 64 lower-case hexadecimal digits"))?;
        if models.iter().any(|m: &LibraryModel| m.id == id) {
            return Err(invalid(&at, "the model is listed twice"));
        }
        if listed.name.trim().is_empty() {
            return Err(invalid(&at, "a model needs its file name"));
        }
        let mut model = LibraryModel {
            id,
            name: listed.name.clone(),
            bytes: Arc::from(Vec::new()),
        };
        let entry = model_entry_name(&model);
        let bytes = archive
            .read(&entry)?
            .ok_or_else(|| invalid(&entry, "the package does not hold this model"))?;
        if sha256(&bytes) != id.0 {
            return Err(invalid(
                &entry,
                "the model's bytes do not match its identity: the file was altered",
            ));
        }
        model.bytes = Arc::from(bytes);
        entries.insert(entry);
        models.push(model);
    }
    if let Some(extra) = archive.names().iter().find(|name| !entries.contains(*name)) {
        return Err(invalid(
            extra,
            "the package holds an entry the project does not list",
        ));
    }
    Ok(models)
}

fn parse_model_id(text: &str) -> Option<ModelId> {
    if text.len() != 64 || !text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
        return None;
    }
    let mut id = [0_u8; 32];
    for (k, byte) in id.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[2 * k..2 * k + 2], 16).ok()?;
    }
    Some(ModelId(id))
}

/// Values finite and within the domain.
fn coordinates(at: &str, what: &str, values: &[f64]) -> Result<(), ProjectFileError> {
    if values.iter().all(|v| v.is_finite() && v.abs() <= DOMAIN_MM) {
        Ok(())
    } else {
        Err(invalid(
            at,
            format!("{what} must be finite and within {DOMAIN_MM} mm of the origin"),
        ))
    }
}

/// A finite value above zero.
fn positive(at: &str, what: &str, value: f64) -> Result<(), ProjectFileError> {
    if value.is_finite() && value > 0.0 {
        Ok(())
    } else {
        Err(invalid(
            at,
            format!("{what} is {value}; it must be positive"),
        ))
    }
}

/// A finite value at least zero.
fn non_negative(at: &str, what: &str, value: f64) -> Result<(), ProjectFileError> {
    if value.is_finite() && value >= 0.0 {
        Ok(())
    } else {
        Err(invalid(
            at,
            format!("{what} is {value}; it must be zero or more"),
        ))
    }
}

fn read_plate(
    entry: PlateEntry,
    at: &str,
    project: &Project,
) -> Result<PlateContent, ProjectFileError> {
    if let Some(printer) = &entry.printer {
        printer
            .validate()
            .map_err(|e| invalid(&format!("{at} printer"), e.to_string()))?;
    }
    if let Some(resin) = &entry.resin {
        resin
            .validate()
            .map_err(|e| invalid(&format!("{at} resin"), e.to_string()))?;
    }
    if let Some(setting) = &entry.print_setting {
        setting
            .validate()
            .map_err(|e| invalid(&format!("{at} print setting"), e.to_string()))?;
    }
    let settings = read_settings(entry.settings, &format!("{at} settings"))?;
    let objects = (1..)
        .zip(entry.objects)
        .map(|(index, object)| {
            let at = format!("{at} object {index} ({:?})", object.name);
            read_object(object, &at, project)
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(PlateContent {
        name: entry.name,
        printer: entry.printer,
        resin: entry.resin,
        print_setting: entry.print_setting,
        settings,
        objects,
    })
}

fn read_settings(entry: SettingsEntry, at: &str) -> Result<PlateSettings, ProjectFileError> {
    machine_exporter(&entry.format).map_err(|e| invalid(at, e))?;
    if entry.job_id.trim().is_empty() {
        return Err(invalid(at, "job_id must not be empty"));
    }
    if let Some(gap) = entry.close_holes_mm {
        positive(at, "close_holes_mm", gap)?;
    }
    if let Some(area) = entry.reversed_tolerance_mm2 {
        if !(area.is_finite() && area > 0.0 && area <= MAX_REVERSED_TOLERANCE_MM2) {
            return Err(invalid(
                at,
                format!(
                    "reversed_tolerance_mm2 is {area}; it must be positive and at most \
                     {MAX_REVERSED_TOLERANCE_MM2}"
                ),
            ));
        }
    }
    non_negative(at, "spacing_mm", entry.spacing_mm)?;
    let image = entry
        .image
        .map_or_else(ImageOverride::default, |i| ImageOverride {
            antialias: i.antialias,
            min_gray: i.min_gray,
            antialias_supports: i.antialias_supports,
        });
    if let Some(samples) = image.antialias {
        if !matches!(samples, 1 | 2 | 4 | 8 | 16) {
            return Err(invalid(
                at,
                format!("image antialias is {samples}; it must be 1, 2, 4, 8 or 16"),
            ));
        }
        machine_exporter_with(&entry.format, samples).map_err(|e| invalid(at, e))?;
    }
    let repair = read_repair(entry.repair, &format!("{at} repair"))?;
    let layer_plan = entry
        .layer_plan
        .map(|plan| {
            // A value read from JSON writes back as JSON.
            let bytes = serde_json::to_vec(&plan).expect("a JSON value always serialises");
            LayerPlanDocument::from_json(&bytes)
                .map_err(|e| invalid(&format!("{at} layer_plan"), e.to_string()))
        })
        .transpose()?;
    Ok(PlateSettings {
        format: entry.format,
        job_id: entry.job_id,
        fill: match entry.fill {
            FillEntry::Union => Fill::Union,
            FillEntry::Strict => Fill::Strict,
        },
        close_holes_mm: entry.close_holes_mm,
        reversed_tolerance_mm2: entry.reversed_tolerance_mm2,
        repair,
        allow_topology_defects: entry.allow_topology_defects,
        layer_plan,
        spacing_mm: entry.spacing_mm,
        image,
    })
}

fn read_repair(entry: RepairEntry, at: &str) -> Result<RepairSpec, ProjectFileError> {
    let fill_holes = entry
        .fill_holes
        .map(|f| {
            Ok::<_, ProjectFileError>(FillHoles {
                max_hole_edges: usize::try_from(f.max_hole_edges).unwrap_or(usize::MAX),
                split_rims: f.split_rims,
                refine: f.refine,
            })
        })
        .transpose()?;
    if let Some(s) = &entry.shells {
        non_negative(at, "shells min_area_mm2", s.min_area_mm2)?;
        non_negative(at, "shells min_volume_mm3", s.min_volume_mm3)?;
    }
    let spec = RepairSpec {
        duplicates: entry.duplicates,
        orientation: entry.orientation,
        outward: entry.outward,
        fill_holes,
        allow_unfilled: entry.allow_unfilled,
        weld_mm: entry.weld_mm,
        degenerate_mm: entry.degenerate_mm,
        crack_mm: entry.crack_mm,
        non_manifold: entry.non_manifold.map(|n| match n {
            NonManifoldEntry::Fins => NonManifold::Fins,
            NonManifoldEntry::Aggressive => NonManifold::Aggressive,
        }),
        visibility: entry.visibility,
        shells: entry.shells.map(|s| Shells {
            min_area_mm2: s.min_area_mm2,
            min_volume_mm3: s.min_volume_mm3,
            inner: s.inner,
        }),
        rebuild_mm: entry.rebuild_mm,
        non_finite: entry.non_finite,
    };
    spec.check().map_err(|reason| invalid(at, reason))?;
    Ok(spec)
}

fn read_object(
    entry: ObjectEntry,
    at: &str,
    project: &Project,
) -> Result<PlateObject, ProjectFileError> {
    let model = parse_model_id(&entry.model)
        .filter(|id| project.model(*id).is_some())
        .ok_or_else(|| invalid(at, "its model is not one the project lists"))?;
    coordinates(at, "translate_mm", &entry.translate_mm)?;
    let p = entry.pose;
    if !p.rotate_deg.iter().all(|r| r.is_finite()) {
        return Err(invalid(at, "rotate_deg must be finite"));
    }
    for s in p.scale {
        positive(at, "a scale factor", s)?;
    }
    if let Some(lift) = p.lift_mm {
        non_negative(at, "lift_mm", lift)?;
        coordinates(at, "lift_mm", &[lift])?;
    }
    if !(1..=MAX_COPIES).contains(&entry.copies) {
        return Err(invalid(
            at,
            format!("copies is {}; it must be 1 to {MAX_COPIES}", entry.copies),
        ));
    }
    let mut ids = BTreeSet::new();
    let operations = entry
        .operations
        .into_iter()
        .map(|o| {
            let at = format!("{at} operation {}", o.id);
            if o.id == 0 || !ids.insert(o.id) {
                return Err(invalid(
                    &at,
                    "operation identities must be nonzero and unique",
                ));
            }
            Ok(ObjectOperation {
                id: o.id,
                enabled: o.enabled,
                kind: read_operation(o.kind, &at)?,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let supports = entry
        .supports
        .map(|doc| {
            // A value read from JSON writes back as JSON.
            let bytes = serde_json::to_vec(&doc).expect("a JSON value always serialises");
            SupportDocument::from_json(&bytes)
                .map_err(|e| invalid(&format!("{at} supports"), e.to_string()))
        })
        .transpose()?;
    let protected = entry
        .protected
        .into_iter()
        .map(|p| {
            coordinates(at, "a protected point", &p.point_mm)?;
            let length = p.normal.iter().map(|c| c * c).sum::<f64>().sqrt();
            if !(length.is_finite() && length > 0.0) {
                return Err(invalid(
                    at,
                    "a protected point's normal must be a direction",
                ));
            }
            Ok(ProtectedPoint {
                point_mm: p.point_mm,
                normal: p.normal,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let object = PlateObject {
        name: entry.name,
        model,
        units: entry.units.map(|u| match u {
            UnitEntry::Micrometer => LengthUnit::Micrometer,
            UnitEntry::Millimeter => LengthUnit::Millimeter,
            UnitEntry::Centimeter => LengthUnit::Centimeter,
            UnitEntry::Inch => LengthUnit::Inch,
            UnitEntry::Foot => LengthUnit::Foot,
            UnitEntry::Meter => LengthUnit::Meter,
        }),
        translate_mm: entry.translate_mm,
        pose: Pose {
            rotate_deg: p.rotate_deg,
            scale: p.scale,
            mirror: p.mirror,
            center: p.center,
            drop: p.drop,
            lift_mm: p.lift_mm,
        },
        operations,
        supports,
        accept_support_collisions: entry.accept_support_collisions,
        copies: entry.copies,
        arrange: entry.arrange,
        protected,
    };
    check_operations(&object).map_err(|reason| invalid(at, format!("the object {reason}")))?;
    Ok(object)
}

fn read_operation(kind: OperationKindEntry, at: &str) -> Result<OperationKind, ProjectFileError> {
    Ok(match kind {
        OperationKindEntry::Repair(spec) => OperationKind::Repair(read_repair(spec, at)?),
        OperationKindEntry::Hollow(h) => {
            positive(at, "wall_mm", h.wall_mm)?;
            if let Some(cell) = h.cell_mm {
                positive(at, "cell_mm", cell)?;
            }
            let simplify = h
                .simplify
                .map(|s| match s {
                    SimplifyEntry::Auto => Ok(Simplify::Auto),
                    SimplifyEntry::Mm(mm) => {
                        positive(at, "simplify mm", mm).map(|()| Simplify::Millimeters(mm))
                    }
                })
                .transpose()?;
            OperationKind::Hollow(Hollowing {
                wall_mm: h.wall_mm,
                cell_mm: h.cell_mm,
                simplify,
            })
        }
        OperationKindEntry::DrainHole(hole) => OperationKind::DrainHole(read_hole(hole, at)?),
        OperationKindEntry::Blocker(blocker) => OperationKind::Blocker(read_blocker(blocker, at)?),
    })
}

fn read_hole(hole: HoleEntry, at: &str) -> Result<DrainHole, ProjectFileError> {
    Ok(match hole {
        HoleEntry::Axis {
            entry_mm,
            direction,
            length_mm,
            diameter_mm,
        } => {
            coordinates(at, "entry_mm", &entry_mm)?;
            let length = direction.iter().map(|c| c * c).sum::<f64>().sqrt();
            if !(length.is_finite() && length > 0.0) {
                return Err(invalid(at, "a hole's direction must be a direction"));
            }
            positive(at, "length_mm", length_mm)?;
            positive(at, "diameter_mm", diameter_mm)?;
            DrainHole::Axis {
                entry_mm,
                direction,
                length_mm,
                diameter_mm,
            }
        }
        HoleEntry::Surface {
            surface_mm,
            diameter_mm,
            outside_mm,
            inside_mm,
            tilt,
            depth_mm,
            from,
        } => {
            coordinates(at, "surface_mm", &surface_mm)?;
            positive(at, "diameter_mm", diameter_mm)?;
            non_negative(at, "outside_mm", outside_mm)?;
            non_negative(at, "inside_mm", inside_mm)?;
            if let Some(depth) = depth_mm {
                positive(at, "depth_mm", depth)?;
            }
            let tilt = tilt
                .map(|t| {
                    if !(t.angle_deg.is_finite()
                        && (0.0..=MAX_HOLE_TILT_DEG).contains(&t.angle_deg)
                        && t.azimuth_deg.is_finite()
                        && (-360.0..=360.0).contains(&t.azimuth_deg))
                    {
                        return Err(invalid(
                            at,
                            format!(
                                "a hole's tilt must be 0 to {MAX_HOLE_TILT_DEG}°, its \
                                     azimuth -360 to 360°"
                            ),
                        ));
                    }
                    Ok(HoleTilt {
                        angle_deg: t.angle_deg,
                        azimuth_deg: t.azimuth_deg,
                    })
                })
                .transpose()?;
            DrainHole::Surface {
                surface_mm,
                diameter_mm,
                outside_mm,
                inside_mm,
                tilt,
                depth_mm,
                surface: match from {
                    SurfaceEntry::Outer => HoleSurface::Outer,
                    SurfaceEntry::Cavity => HoleSurface::Cavity,
                },
            }
        }
    })
}

fn read_blocker(blocker: BlockerEntry, at: &str) -> Result<Blocker, ProjectFileError> {
    Ok(match blocker {
        BlockerEntry::Frustum {
            centre_mm,
            rotation_deg,
            bottom_mm,
            top_mm,
            height_mm,
        } => {
            coordinates(at, "centre_mm", &centre_mm)?;
            if !rotation_deg.iter().all(|r| r.is_finite()) {
                return Err(invalid(at, "rotation_deg must be finite"));
            }
            for v in bottom_mm.into_iter().chain(top_mm) {
                non_negative(at, "a blocker's size", v)?;
            }
            positive(at, "height_mm", height_mm)?;
            Blocker::Frustum {
                centre_mm,
                rotation_deg,
                bottom_mm,
                top_mm,
                height_mm,
            }
        }
        BlockerEntry::Sphere {
            centre_mm,
            diameter_mm,
        } => {
            coordinates(at, "centre_mm", &centre_mm)?;
            positive(at, "diameter_mm", diameter_mm)?;
            Blocker::Sphere {
                centre_mm,
                diameter_mm,
            }
        }
    })
}
