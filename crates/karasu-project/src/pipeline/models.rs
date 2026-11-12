//! Model export (TD-139): a plate's models as they stand on it, written as binary STL, 3MF or
//! OBJ, with their supports and rafts by choice, hollowed models with their drain holes cut.
//!
//! **Preparation.** Every object is prepared exactly as slicing prepares it (import, repair,
//! placement, hollowing, drain holes, supports) and arranged on the plate the same way, so a
//! file holds what the print holds. Each instance is its object's geometry moved rigidly in XY,
//! the move rounded to the Boolean kernel's grid (2⁻¹⁵ mm), so a drilled model stays exact in
//! the file ([`karasu_core::export::models::grid_offset`]).
//!
//! **Bodies.** For each instance:
//! - the model: the hollowed shell, or the placed model, its drain holes cut into it by the
//!   exact kernel (TD-140); an object is drilled once, whatever its copies;
//! - its supports (braces included), when asked;
//! - its raft, when asked.
//!
//! A raft belongs to its model (each model's supports build their own), so it goes with its model
//! in one file per model too.
//!
//! **Files.** One file per plate holds every chosen object's instances. One file per model
//! holds one object's instances. The caller names each file ([`ModelFiles`]). Each file is
//! written whole or not at all, and a failure in one file leaves the others. Every file reports
//! what drilling left (rounds, moves, crossings), never hidden.
use super::{
    placed_mesh, plate, prepare_object, read_printer, read_resin, read_supports, Context, Failure,
    Observer, PipelineCache, PipelineError, PreparedObject, Stop,
};
use crate::request::SliceRequest;
use karasu_core::{
    cancellation::{Cancellation, Checkpoint},
    export::models::{
        grid_offset, replace_model_file, tidy_shells, write_model_file, BodyKind, ModelBody,
        ModelFileError, ModelFileFormat, ModelFileSummary,
    },
    geometry::Point3,
    mesh::TriangleMesh,
    preparation::sampled::DrillError,
};
use std::{
    collections::HashMap,
    ops::Range,
    path::{Path, PathBuf},
};

/// How a plate's models are divided into files.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelFiles {
    /// One file for the plate.
    PerPlate,
    /// One file for each model (object), with its copies.
    PerModel,
}

/// What to export from a plate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModelExport {
    /// The file format.
    pub format: ModelFileFormat,
    /// One file for the plate or one per model.
    pub files: ModelFiles,
    /// With the supports (braces included).
    pub supports: bool,
    /// With the rafts.
    pub raft: bool,
    /// Only this object (its 0-based position in the plate), or every object.
    pub object: Option<usize>,
    /// Replace files that exist: the user confirmed it. Otherwise an existing file is refused.
    pub replace: bool,
    /// One body per model with its supports and raft, or the three apart (3MF objects, OBJ
    /// groups).
    pub objects: ModelObjects,
}

/// How a model, its supports and its raft are divided into a file's bodies (3MF objects, OBJ
/// groups; STL is one solid either way).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelObjects {
    /// Each model with its supports and raft as one body: readers that rest each 3MF object on
    /// the plate on their own (Lychee) keep the supports under their model (owner, 2026-11-09).
    OnePerModel,
    /// The model, its supports and its raft as bodies of their own.
    Separate,
}

/// One file the export wrote, or tried to.
#[derive(Debug)]
pub struct ExportedModelFile {
    /// Where.
    pub path: PathBuf,
    /// The models it holds, by name.
    pub models: Vec<String>,
    /// What it holds, or why it was not written (the folder is then as it was).
    pub result: Result<ModelFileSummary, PipelineError>,
}

/// What cutting a model's drain holes left (TD-140 phase 5).
#[derive(Clone, Debug, PartialEq)]
pub struct DrilledReport {
    /// The model's name.
    pub model: String,
    /// Rounds of resolving rounding's crossings.
    pub rounds: u32,
    /// Vertices moved to a snap cell's centre.
    pub moved: usize,
    /// The farthest such a move on one axis, in millimetres.
    pub farthest_mm: f64,
    /// Specks no larger than 15.6 µm that rounding left, left out.
    pub specks: usize,
    /// Vertex copies moved about 0.06 µm apart where the model's bodies touched.
    pub separated: usize,
    /// Triangles without area left.
    pub flat: usize,
    /// Pairs of triangles crossing left.
    pub crossings: usize,
}

/// The export of one plate.
#[derive(Debug)]
pub struct ModelExportOutcome {
    /// Every file, in order.
    pub files: Vec<ExportedModelFile>,
    /// Every drilled model, in object order.
    pub drilled: Vec<DrilledReport>,
}

/// Which file of a plate a destination names: `None` for the plate's one file, `Some((n,
/// name))` for its model `n` (1-based) of that name.
pub type ModelLabel<'a> = Option<(usize, &'a str)>;

/// What names a file: the fields of a file name pattern ([`model_file_name`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NameFields<'a> {
    /// The project's name.
    pub project: &'a str,
    /// The plate's name.
    pub plate: &'a str,
    /// The model's 1-based number in its plate, for a file per model.
    pub number: Option<usize>,
    /// The model's name, for a file per model.
    pub model: Option<&'a str>,
}

/// A file name from `pattern`: `{project}`, `{plate}`, `{number}` and `{model}` replaced by
/// their fields (empty when the field is absent), characters no file system takes (`\ / : * ?
/// " < > |` and control characters) replaced by `_`, runs of spaces collapsed and the ends
/// trimmed. Without an extension; never empty (`model` then).
#[must_use]
pub fn model_file_name(pattern: &str, fields: &NameFields<'_>) -> String {
    let number = fields.number.map_or_default(|n| n.to_string());
    let text = pattern
        .replace("{project}", fields.project)
        .replace("{plate}", fields.plate)
        .replace("{number}", &number)
        .replace("{model}", fields.model.unwrap_or(""));
    let cleaned: String = text
        .chars()
        .map(|c| {
            if c.is_control() || r#"\/:*?"<>|"#.contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    let name = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    // A name of dots alone is no file name on Windows.
    let name = name.trim_matches('.').to_owned();
    if name.is_empty() {
        "model".to_owned()
    } else {
        name
    }
}

/// A mesh made for the export, or one the preparation holds.
enum Body<'a> {
    Owned(TriangleMesh),
    Borrowed(&'a TriangleMesh),
}

impl std::ops::Deref for Body<'_> {
    type Target = TriangleMesh;
    fn deref(&self) -> &TriangleMesh {
        match self {
            Self::Owned(mesh) => mesh,
            Self::Borrowed(mesh) => mesh,
        }
    }
}

/// One object made ready for export: its model, supports and raft, and its instances.
struct Exported<'a> {
    name: String,
    model: Body<'a>,
    supports: Option<TriangleMesh>,
    raft: Option<TriangleMesh>,
    /// Each instance's move, on the grid.
    offsets: Vec<[f64; 3]>,
}

/// Export the models of `request`'s plate as `export` says, each file at the path `destination`
/// gives: for one file per plate, `destination(None)`; for one per model, `destination(Some((n,
/// name)))` with its 1-based number and name. Objects are prepared as slicing prepares them,
/// reusing `cache`.
///
/// # Errors
/// Failures that concern the whole plate: unreadable profiles, an object that cannot be
/// prepared, a plate whose objects do not fit, cancellation. A file that cannot be written (a
/// model whose holes cannot be cut, an existing file, a full disk) is reported in its own
/// result, and the other files are still written.
pub fn export_models(
    request: &SliceRequest,
    export: &ModelExport,
    destination: &dyn Fn(ModelLabel<'_>) -> PathBuf,
    cache: Option<&PipelineCache>,
    observer: &mut dyn Observer,
    stop: &(dyn Fn(Checkpoint) -> bool + Sync),
) -> Result<ModelExportOutcome, PipelineError> {
    let mut ctx = Context {
        observer,
        stop: Stop(stop),
        preview: false,
        probe: false,
        cache,
        running: None,
    };
    let started = std::time::Instant::now();
    let result = run(request, export, destination, &mut ctx);
    ctx.close_phase();
    match &result {
        Ok(outcome) => {
            let written = outcome.files.iter().filter(|f| f.result.is_ok()).count();
            log::info!(
                target: "export",
                "exported {written} of {} model file(s) as {} in {:.2} s",
                outcome.files.len(),
                export.format.extension(),
                started.elapsed().as_secs_f64()
            );
            for file in &outcome.files {
                if let Err(error) = &file.result {
                    log::warn!(target: "export", "{}: {}", file.path.display(), error.message());
                }
            }
        }
        Err(failure) => log::warn!(target: "export", "model export failed: {}", failure.message),
    }
    result.map_err(PipelineError::from)
}

fn run(
    args: &SliceRequest,
    export: &ModelExport,
    destination: &dyn Fn(ModelLabel<'_>) -> PathBuf,
    ctx: &mut Context<'_>,
) -> Result<ModelExportOutcome, Failure> {
    ctx.phase("Reading profiles");
    let printer = read_printer(&args.printer)?;
    let resin = read_resin(&args.resin)?;
    let settings = resin
        .calibration_for(&printer.id)
        .map_err(|e| format!("resin profile cannot be used with this printer: {e}"))?;
    let (objects, names, spacing_mm) = plate::objects(args)?;
    if let Some(object) = export.object {
        if object >= objects.len() {
            return Err(format!(
                "the plate has {} object(s); there is no object {}",
                objects.len(),
                object + 1
            )
            .into());
        }
    }
    let documents = objects
        .iter()
        .map(|o| o.supports.as_ref().map(read_supports).transpose())
        .collect::<Result<Vec<_>, _>>()?;
    // Every object, even when one is exported: the arrangement places them all together.
    let mut prepared = Vec::with_capacity(objects.len());
    for (object, document) in objects.into_iter().zip(&documents) {
        prepared.push(prepare_object(
            object,
            document.as_ref(),
            &printer,
            settings.layer_height_mm,
            ctx,
        )?);
    }
    let layout = (args.model().is_none() || args.arrange)
        .then(|| plate::Layout::new(&prepared, names.clone(), &printer, spacing_mm))
        .transpose()?;
    let mut offsets: Vec<Vec<[f64; 3]>> = vec![Vec::new(); prepared.len()];
    match &layout {
        None => offsets[0].push([0.0; 3]),
        Some(layout) => {
            for (object, _, [dx, dy]) in layout.placed() {
                offsets[object].push(grid_offset([dx, dy, 0.0]));
            }
        }
    }
    let chosen: Vec<usize> = (0..prepared.len())
        .filter(|&i| export.object.is_none_or(|o| o == i))
        .collect();
    // Each chosen object's bodies; a model whose holes cannot be cut fails its files.
    let mut drilled = Vec::new();
    let mut ready: HashMap<usize, Result<Exported<'_>, String>> = HashMap::new();
    for &i in &chosen {
        let result = match bodies(
            &prepared[i],
            &names[i],
            std::mem::take(&mut offsets[i]),
            export,
            &mut drilled,
            ctx,
        ) {
            Err(Stopped::Cancelled) => return Err(cancelled()),
            Err(Stopped::Failed(message)) => Err(message),
            Ok(exported) => Ok(exported),
        };
        ready.insert(i, result);
    }
    let groups: Vec<(ModelLabel<'_>, Vec<usize>)> = match export.files {
        ModelFiles::PerPlate => vec![(None, chosen.clone())],
        ModelFiles::PerModel => chosen
            .iter()
            .map(|&i| (Some((i + 1, names[i].as_str())), vec![i]))
            .collect(),
    };
    let mut files = Vec::with_capacity(groups.len());
    for (label, members) in groups {
        let path = destination(label);
        let models: Vec<String> = members.iter().map(|&i| names[i].clone()).collect();
        let result = match write(&path, &members, &ready, export, ctx) {
            Err(Stopped::Cancelled) => return Err(cancelled()),
            Err(Stopped::Failed(message)) => Err(PipelineError::from(Failure::from(message))),
            Ok(summary) => Ok(summary),
        };
        files.push(ExportedModelFile {
            path,
            models,
            result,
        });
    }
    Ok(ModelExportOutcome { files, drilled })
}

/// Why an object or a file was not made.
enum Stopped {
    /// The caller cancelled: the whole export stops.
    Cancelled,
    /// This object or file only.
    Failed(String),
}

impl From<String> for Stopped {
    fn from(message: String) -> Self {
        Self::Failed(message)
    }
}

/// The failure of a cancelled export: the files written before it stay, the rest are not
/// begun.
fn cancelled() -> Failure {
    Failure::from("the model export was cancelled; no further file was written")
}

/// One object's bodies: its model (drilled when it has holes), supports and raft.
fn bodies<'a>(
    object: &'a PreparedObject,
    name: &str,
    offsets: Vec<[f64; 3]>,
    export: &ModelExport,
    drilled: &mut Vec<DrilledReport>,
    ctx: &mut Context<'_>,
) -> Result<Exported<'a>, Stopped> {
    let stop = ctx.stop;
    let surface = match object.prepared.hollow() {
        Some(model) => Body::Borrowed(model.mesh()),
        None => Body::Owned(placed_mesh(&object.mesh, &object.placement)?),
    };
    let model = match &object.holes {
        Some(holes) => {
            ctx.phase(&format!("Cutting drain holes: {name}"));
            let started = std::time::Instant::now();
            let cut = holes
                .cuts
                .drill(&surface, &mut Cancellation::new(&mut stop.predicate()))
                .map_err(|e| match e {
                    DrillError::Cancelled(_) => Stopped::Cancelled,
                    e => Stopped::Failed(format!("cannot cut the drain holes into {name}: {e}")),
                })?;
            log::info!(
                target: "export",
                "{name}: {} holes cut in {:.2} s, {} rounds, {} vertices moved (at most {:.2} \
                 µm), {} specks left out, {} flat, {} crossings left",
                holes.specs.len(),
                started.elapsed().as_secs_f64(),
                cut.rounds,
                cut.moved,
                cut.farthest_mm * 1000.0,
                cut.specks,
                cut.flat,
                cut.crossings
            );
            drilled.push(DrilledReport {
                model: name.to_owned(),
                rounds: cut.rounds,
                moved: cut.moved,
                farthest_mm: cut.farthest_mm,
                specks: cut.specks,
                separated: cut.separated,
                flat: cut.flat,
                crossings: cut.crossings,
            });
            Body::Owned(cut.mesh)
        }
        None => surface,
    };
    let (supports, raft) = match &object.supported {
        Some(supported) => {
            let all = supported.model_faces..supported.mesh.triangles().len();
            let raft = &supported.raft_faces;
            let support_faces = without(all, raft);
            // Supports and raft tidied for a file: each repeated shell once, caps back to back
            // gone (karasu_core::export::models::tidy_shells).
            let tidy = |ranges: &[Range<usize>]| {
                part(&supported.mesh, ranges).map(|mesh| {
                    let tidied = tidy_shells(&mesh);
                    log::info!(
                        target: "export",
                        "{name}: {} repeated support shells written once, {} face pairs back to \
                         back left out",
                        tidied.repeated,
                        tidied.cancelled
                    );
                    tidied.mesh
                })
            };
            (
                export.supports.then(|| tidy(&support_faces)).flatten(),
                export.raft.then(|| tidy(raft)).flatten(),
            )
        }
        None => (None, None),
    };
    Ok(Exported {
        name: name.to_owned(),
        model,
        supports,
        raft,
        offsets,
    })
}

/// `all` without the ranges in `removed` (sorted, inside it).
fn without(all: Range<usize>, removed: &[Range<usize>]) -> Vec<Range<usize>> {
    let mut sorted = removed.to_vec();
    sorted.sort_by_key(|r| r.start);
    let mut out = Vec::new();
    let mut at = all.start;
    for r in sorted {
        if r.start > at {
            out.push(at..r.start);
        }
        at = at.max(r.end);
    }
    if at < all.end {
        out.push(at..all.end);
    }
    out
}

/// The triangles of `mesh` in `ranges` as a mesh of their own; `None` when there are none.
fn part(mesh: &TriangleMesh, ranges: &[Range<usize>]) -> Option<TriangleMesh> {
    let mut index: HashMap<u32, u32> = HashMap::new();
    let mut vertices: Vec<Point3> = Vec::new();
    let mut triangles = Vec::new();
    for range in ranges {
        for t in &mesh.triangles()[range.clone()] {
            triangles.push(t.map(|v| {
                *index.entry(v).or_insert_with(|| {
                    vertices.push(mesh.vertices()[v as usize]);
                    u32::try_from(vertices.len() - 1).unwrap_or(u32::MAX)
                })
            }));
        }
    }
    if triangles.is_empty() {
        return None;
    }
    TriangleMesh::try_new(vertices, triangles).ok()
}

/// `meshes` as one mesh, their triangles in order on their own vertices.
fn join(meshes: &[&TriangleMesh]) -> TriangleMesh {
    let mut vertices: Vec<Point3> = Vec::new();
    let mut triangles: Vec<[u32; 3]> = Vec::new();
    for mesh in meshes {
        let base = u32::try_from(vertices.len()).unwrap_or(u32::MAX);
        vertices.extend_from_slice(mesh.vertices());
        triangles.extend(mesh.triangles().iter().map(|t| t.map(|v| v + base)));
    }
    TriangleMesh::try_new(vertices, triangles).expect("valid meshes side by side make a valid mesh")
}

/// Write one file of the objects `members`.
fn write(
    path: &Path,
    members: &[usize],
    ready: &HashMap<usize, Result<Exported<'_>, String>>,
    export: &ModelExport,
    ctx: &mut Context<'_>,
) -> Result<ModelFileSummary, Stopped> {
    let stop = ctx.stop;
    let mut names: Vec<String> = Vec::new();
    let mut parts: Vec<(usize, BodyKind, &TriangleMesh, [f64; 3])> = Vec::new();
    // Each model with its supports and raft as one body, unless kept apart.
    let mut objects = Vec::with_capacity(members.len());
    for &i in members {
        objects.push(
            ready[&i]
                .as_ref()
                .map_err(|message| Stopped::Failed(message.clone()))?,
        );
    }
    let joined: Vec<Option<TriangleMesh>> = objects
        .iter()
        .map(|object| {
            (export.objects == ModelObjects::OnePerModel
                && (object.supports.is_some() || object.raft.is_some()))
            .then(|| {
                let meshes: Vec<&TriangleMesh> = [Some(&*object.model)]
                    .into_iter()
                    .chain([object.supports.as_ref(), object.raft.as_ref()])
                    .flatten()
                    .collect();
                join(&meshes)
            })
        })
        .collect();
    for (object, joined) in objects.iter().zip(&joined) {
        let copies = object.offsets.len();
        for (copy, &offset) in (1..).zip(&object.offsets) {
            let base = if copies > 1 {
                format!("{} {copy}", object.name)
            } else {
                object.name.clone()
            };
            if let Some(mesh) = joined {
                names.push(base);
                parts.push((names.len() - 1, BodyKind::Model, mesh, offset));
                continue;
            }
            for (kind, mesh, suffix) in [
                (BodyKind::Model, Some(&*object.model), ""),
                (BodyKind::Supports, object.supports.as_ref(), " supports"),
                (BodyKind::Raft, object.raft.as_ref(), " raft"),
            ] {
                if let Some(mesh) = mesh {
                    names.push(format!("{base}{suffix}"));
                    parts.push((names.len() - 1, kind, mesh, offset));
                }
            }
        }
    }
    let bodies: Vec<ModelBody<'_>> = parts
        .iter()
        .map(|&(n, kind, mesh, offset)| ModelBody {
            name: &names[n],
            kind,
            mesh,
            offset,
        })
        .collect();
    let file = path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    ctx.phase(&format!("Writing {file}"));
    let mut cancel = stop.predicate();
    let mut cancel = Cancellation::new(&mut cancel);
    let written = if export.replace {
        replace_model_file(path, export.format, &bodies, &mut cancel)
    } else {
        write_model_file(path, export.format, &bodies, &mut cancel)
    };
    written.map_err(|e| match e {
        ModelFileError::Cancelled(_) => Stopped::Cancelled,
        e => Stopped::Failed(format!("cannot write {}: {e}", path.display())),
    })
}

#[cfg(test)]
mod tests {
    use super::{model_file_name, without, NameFields};

    #[test]
    fn file_names_fill_the_pattern_and_keep_to_what_file_systems_take() {
        let fields = NameFields {
            project: "Venom",
            plate: "Arms: left/right",
            number: Some(3),
            model: Some("Hand?"),
        };
        assert_eq!(
            model_file_name("{project} {plate} {number} {model}", &fields),
            "Venom Arms_ left_right 3 Hand_"
        );
        let plate = NameFields {
            number: None,
            model: None,
            ..fields
        };
        assert_eq!(
            model_file_name("{plate}  {model}", &plate),
            "Arms_ left_right"
        );
        assert_eq!(model_file_name("{model}", &plate), "model");
        assert_eq!(model_file_name("..", &plate), "model");
    }

    #[test]
    fn ranges_without_others_keep_the_rest_in_order() {
        assert_eq!(
            without(10..30, &[20..25, 12..14]),
            vec![10..12, 14..20, 25..30]
        );
        assert_eq!(without(10..30, &[]), vec![10..30]);
        assert_eq!(
            without(10..30, &[std::ops::Range { start: 10, end: 30 }]),
            Vec::<std::ops::Range<usize>>::new()
        );
    }
}
