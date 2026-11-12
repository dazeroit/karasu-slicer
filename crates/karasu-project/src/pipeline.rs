//! The slicing pipeline shared by the command line and the desktop application (M6.1, TD-070):
//! import, repair, placement, hollowing, drain holes, supports, plate arrangement, layer planning,
//! rasterization and export, then the summary.
//!
//! **No geometry policy of its own:** source coordinates are millimetres by explicit request,
//! placement is an explicit translation or pose, and the model must rest exactly on the build
//! plate (world Z = 0) inside the LCD panel unless supports hold it. Unsupported or ambiguous
//! geometry fails through the core's structured errors; no repair, automatic placement or
//! clipping happens unless requested.
//!
//! **Filling:** layers are filled by the winding-number union (TD-050) unless the request selects
//! the strict even-odd pipeline. Union filling refuses a mesh with boundary edges or winding
//! conflicts, and strict filling one that is not a closed combinatorial surface, unless the
//! request accepts topology defects; every sampled layer must then still close (union) or
//! resolve completely (strict).
//!
//! **Masks:** `antialias` samples every pixel on an N x N grid (TD-052); the masks store the
//! covered fraction as grey at the format's native depth. `min_gray` raises every partially
//! covered pixel to at least that gray (TD-060).
//!
//! **Hollowing:** the placed model is hollowed by the core's sampled-distance backend (TD-045);
//! the shell (exterior plus reversed cavity) is sliced in world coordinates. Drain holes are
//! certified finite cuts (TD-047, TD-066), removed from every layer analytically.
//!
//! **Cancellation and progress:** the caller's predicate is observed at every core cancellation
//! checkpoint; a cancelled job fails through its ordinary error path, so export staging data is
//! removed and nothing is published. Progress goes to an [`Observer`], never to the output.
//!
//! **Determinism:** the outputs and the summary depend only on the request and the files it
//! names, never on the thread count.

mod models;
mod plate;
mod suction;
mod summary;
mod unsupported;
mod verification;

pub use models::{
    export_models, model_file_name, DrilledReport, ExportedModelFile, ModelExport,
    ModelExportOutcome, ModelFiles, ModelLabel, ModelObjects, NameFields,
};
pub use suction::SuctionCups;
pub use unsupported::Unsupported;
pub use verification::{Verification, Walls, UNHOLLOWED_WALL_MM};

use crate::placement::{self, centre, Shape};
use crate::request::{
    machine_exporter_with, Blocker, Coordinates, DrainHole, Fill, Input, ModelSource, Pose,
    QuotedPath, SliceRequest, REFERENCE_FORMAT,
};
use karasu_core::{
    cancellation::{Cancellation, Checkpoint},
    digest::sha256,
    export::{
        export_machine_parallel, export_reference, AnycubicWorkshop, BuiltinExporter,
        BuiltinFormat, ExportError, ExportLimits, ExportSummary, MachineExporter, ParallelEncoding,
        PixelPolicy,
    },
    geometry::{Affine3, ObjectTransform, Point3},
    layer_plan::{
        plan_layers as plan_variable, LayerPlanDocument, MAX_DOCUMENT_BYTES as MAX_LAYER_PLAN_BYTES,
    },
    mesh::{
        formats::{
            FormatRegistry, ImportNote, LengthUnit, ModelImportError, ModelImportOptions,
            StlFormat, UnitOrigin,
        },
        stl::{DroppedFacets, ImportLimits, NonFinitePolicy},
        topology::{DefectReport, TopologyLimits},
        TriangleMesh,
    },
    preparation::{
        drainage::DrainageError,
        repair::{repair_source, RepairSpec, SourceRepairReport},
        sampled::{
            guaranteed_cell, hollow_mesh_sampled_with_progress, predicted_cavity_triangles,
            surface_area, CavityRegion, HoleCuts, HoleFindings, HoleProbe, ProbeLimits,
            SampledDrainageLimits, SampledHollowLimits, SampledHollowProgress, SampledHollowStage,
            SampledHollowedModel, DEFAULT_CAVITY_TRIANGLE_BUDGET, DEFAULT_OVER_THICKNESS_MM,
            DEFAULT_SHORTFALL_COEFFICIENT, MAX_HOLES,
        },
        HoleSpec, HollowSpec, Operation, OperationKind, PlanLimits, PreparationPlan, SamplingSpec,
        SimplifySpec, SourceBinding, WallGuarantee,
    },
    preview::{render_preview, Previews},
    print_job::{JobLimits, LayerOverride, PrintJob},
    profiles::{
        store::ProfileDocument, PrintSettings, PrinterProfile, ResinProfile, ResinUsage,
        MAX_PROFILE_BYTES,
    },
    raster::{
        parallel::{with_parallel_raster, BottomRaster, LayerFill, ParallelRaster},
        Compensation, LayerCut, LcdProfile, OutsidePolicy, RasterConfig, RasterError, RasterImage,
        RasterLimits,
    },
    slicing::{
        engine::{LayerConfig, LayerStageLimits},
        grid::BinaryGridSpec,
        regions::RegionFillRule,
        schedule::{
            build_layer_schedule, build_slab_schedule, LayerSchedule, ScheduleEnd, ScheduleLimits,
            UniformScheduleSpec,
        },
        validity::ValidityLimits,
        winding::{SectionClosure, WindingRule},
        SectionLimits, SectionTransform,
    },
    supports::{
        build_supports, plan_braces, BaseKind, BoundPose, BracePlan, BuildVolume, DrainCylinder,
        PlanarMotion, PlannedBrace, Severity, SupportDocument, SupportKind, SupportOptions,
        SupportScene, SupportSurface, MAX_DOCUMENT_BYTES,
    },
};
use std::{
    borrow::Cow,
    fmt,
    fs::File,
    io::Read,
    ops::Range,
    path::Path,
    sync::{Arc, Mutex},
};

/// Receives the progress of a running job. Purely advisory: it never changes the outputs.
pub trait Observer {
    /// A named stage begins (for example `Importing model` or `Hollowing: distance field 40%`).
    fn phase(&mut self, name: &str);
    /// Layer generation begins, with `total` layers to produce.
    fn begin_layers(&mut self, total: usize);
    /// `completed` layers have been produced and handed to the exporter, in order.
    fn layers(&mut self, completed: usize);
    /// A layer's mask, exactly as the exporter receives it, just before [`Self::layers`] counts
    /// it. Masks arrive in layer order. The default ignores them; an interface keeps them to show
    /// the layers that were exported (M6.3).
    fn mask(&mut self, image: &RasterImage) {
        let _ = image;
    }
}

/// An observer that ignores progress.
#[derive(Clone, Copy, Debug, Default)]
pub struct Silent;

impl Observer for Silent {
    fn phase(&mut self, _: &str) {}
    fn begin_layers(&mut self, _: usize) {}
    fn layers(&mut self, _: usize) {}
}

/// A request change that may let a refused job succeed. The command line names each one by its
/// option; an interface offers its own control for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Remedy {
    /// Turn inside-out closed bodies outward ([`RepairSpec::outward`]).
    RepairOutward,
    /// Make face orientation consistent ([`RepairSpec::orientation`]).
    RepairOrientation,
    /// Fill the source's holes ([`RepairSpec::fill_holes`]): hollowing needs a closed surface.
    FillHoles,
    /// Close holes in each layer (the request's `close_holes_mm`).
    CloseHoles,
    /// Leave small reversed slivers empty in each layer (the request's
    /// `reversed_tolerance_mm2`, TD-079).
    TolerateReversed,
    /// Fill layers by the strict even-odd rule instead of the winding union.
    StrictFill,
    /// Slice despite topology defects; every layer must still close.
    AllowTopologyDefects,
    /// Lower or raise the model so it rests on the build plate.
    RestOnPlate,
    /// Move the model inside the LCD area, for example by centring it.
    MoveOntoPlate,
    /// Give the length unit of a file that declares none.
    GiveUnits,
    /// Give no unit, or the unit the file declares.
    MatchDeclaredUnits,
    /// Repair the model's holes and non-manifold edges, for example with the standard preset
    /// (TD-092): a layer did not close.
    RepairDefects,
    /// Accept leaving out the facets without a position ([`RepairSpec::non_finite`], included
    /// in the exact preset, TD-092 r9): the model has NaN or infinite coordinates.
    DropNonFinite,
}

/// A failed job: the reason, naming the inputs involved, and the remedies it suggests.
///
/// A job cancelled by the caller's predicate fails with whatever error its interrupted stage
/// reports; the caller, which owns the predicate, knows that it asked for the cancellation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PipelineError {
    message: String,
    reason: String,
    remedies: Vec<Remedy>,
}

impl PipelineError {
    /// The command line's message: the reason followed by the options that may help.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// The reason alone, without command-line options.
    #[must_use]
    pub fn reason(&self) -> &str {
        &self.reason
    }

    /// The request changes that may help, most specific first; empty when none is known.
    #[must_use]
    pub fn remedies(&self) -> &[Remedy] {
        &self.remedies
    }
}

impl From<Failure> for PipelineError {
    fn from(failure: Failure) -> Self {
        Self {
            reason: failure.reason.unwrap_or_else(|| failure.message.clone()),
            message: failure.message,
            remedies: failure.remedies,
        }
    }
}

/// A failure inside the pipeline. Most stages report only a message; the stages that name
/// command-line options also keep the reason without them, and the remedies those options stand
/// for.
#[derive(Debug)]
struct Failure {
    message: String,
    reason: Option<String>,
    remedies: Vec<Remedy>,
}

impl Failure {
    /// `reason` followed by `hint`, which names the command-line options for `remedies`.
    fn with_remedies(reason: String, hint: &str, remedies: Vec<Remedy>) -> Self {
        Self {
            message: format!("{reason}{hint}"),
            reason: Some(reason),
            remedies,
        }
    }
}

impl From<String> for Failure {
    fn from(message: String) -> Self {
        Self {
            message,
            reason: None,
            remedies: Vec::new(),
        }
    }
}

impl From<&str> for Failure {
    fn from(message: &str) -> Self {
        message.to_owned().into()
    }
}

impl fmt::Display for PipelineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for PipelineError {}

/// A completed job: its output is published.
#[derive(Clone, Debug)]
pub struct SliceOutcome {
    /// The human-readable summary, one `Key: value` line per fact, ending in a newline.
    pub summary: String,
    /// What the exporter wrote.
    pub export: ExportSummary,
    /// Resin needed for the sampled mask volume.
    pub resin: ResinUsage,
    /// The printed walls' verification, when asked for (`verify_walls`, TD-095).
    pub verification: Option<Box<Verification>>,
    /// The print's unsupported regions, when asked for (`find_unsupported`, TD-107).
    pub unsupported: Option<Box<Unsupported>>,
    /// The print's suction cups, when asked for (`find_suction_cups`, TD-114).
    pub suction_cups: Option<Box<SuctionCups>>,
    /// Each layer as the print job resolved it, from the plate up (for display, TD-103).
    pub layers: Vec<PrintedLayer>,
}

/// One printed layer as the print job resolved it: where the build platform stops, how thick it
/// cures, where its settings came from and how long it is exposed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PrintedLayer {
    /// The printer's build position after the layer, in mm: the layer's top.
    pub top_mm: f64,
    /// The cured thickness, in mm.
    pub thickness_mm: f64,
    /// Bottom, transition, normal or an explicit override (a layer plan's).
    pub phase: karasu_core::print_job::LayerPhase,
    /// The exposure, in seconds.
    pub exposure_s: f64,
}

/// Run one job to completion and publish its output, or publish nothing.
///
/// `stop` is polled at every cancellation checkpoint, from several threads; returning `true`
/// cancels the job. Progress is reported to `observer`.
///
/// # Errors
/// The first failure: unreadable or invalid inputs, refused geometry or placement, failed
/// preparation, layer generation or export, or cancellation.
pub fn slice(
    request: &SliceRequest,
    observer: &mut dyn Observer,
    stop: &(dyn Fn(Checkpoint) -> bool + Sync),
) -> Result<SliceOutcome, PipelineError> {
    slice_with(request, None, observer, stop)
}

/// [`slice()`], reusing and keeping cavities in `cache` (TD-089). The outputs are the same with
/// or without a cache; only the time differs.
///
/// # Errors
/// The failures of [`slice()`].
pub fn slice_with(
    request: &SliceRequest,
    cache: Option<&PipelineCache>,
    observer: &mut dyn Observer,
    stop: &(dyn Fn(Checkpoint) -> bool + Sync),
) -> Result<SliceOutcome, PipelineError> {
    let mut ctx = Context {
        observer,
        stop: Stop(stop),
        preview: false,
        probe: false,
        cache,
        running: None,
    };
    let started = std::time::Instant::now();
    let result = run(request, &mut ctx);
    ctx.close_phase();
    match &result {
        Ok(outcome) => {
            log::info!(
                target: "slice",
                "sliced {} layers to {} in {:.2} s",
                outcome.export.layers,
                request.output.display(),
                started.elapsed().as_secs_f64()
            );
            log::info!(target: "slice", "summary:\n{}", outcome.summary.trim_end());
        }
        Err(failure) => log::warn!(target: "slice", "slicing failed: {}", failure.message),
    }
    result.map_err(PipelineError::from)
}

/// One object prepared for display at its design placement (M6.4a, TD-082): its hollowed
/// shell, its drain holes and its built supports, in world millimetres.
#[derive(Debug)]
pub struct ObjectPreparation {
    prepared: Prepared,
    holes: Option<PreparedHoles>,
    holes_error: Option<PipelineError>,
    drainage_unavailable: Option<PipelineError>,
    supports: Option<SupportScene>,
    probe: Option<Arc<HoleProbe>>,
    repair: Option<RepairOutcome>,
}

impl ObjectPreparation {
    /// The object's drain holes as cut, in world millimetres (resolved surface holes
    /// included): what its supports are checked against, and where automatic supports place
    /// no tip (TD-131). Empty without holes.
    #[must_use]
    pub fn drain_cylinders(&self) -> Vec<DrainCylinder> {
        drain_cylinders(self.holes.as_ref())
    }

    /// What the object's repairs did and the state they leave (TD-092 r8).
    #[must_use]
    pub fn repair(&self) -> Option<&RepairOutcome> {
        self.repair.as_ref()
    }

    /// The hollowed model (shell, cavity and wall certificate), when the object is hollowed.
    #[must_use]
    pub fn hollowed(&self) -> Option<&SampledHollowedModel> {
        self.prepared.hollow()
    }

    /// The object's drain holes as cut, with their findings, when it has any (TD-089).
    #[must_use]
    pub fn holes(&self) -> Option<&PreparedHoles> {
        self.holes.as_ref()
    }

    /// The built supports with every finding, when the object has supports. Unlike slicing,
    /// preparation keeps supports with errors, so the findings can be shown and fixed.
    #[must_use]
    pub fn supports(&self) -> Option<&SupportScene> {
        self.supports.as_ref()
    }

    /// Why the object's drain holes could not be cut, when they could not (a hole too narrow
    /// to print, or one that cannot be resolved). The object is then shown without them.
    #[must_use]
    pub fn drainage_error(&self) -> Option<&PipelineError> {
        self.holes_error.as_ref()
    }

    /// Why no drain hole can be cut into the object at all, when none can: its surfaces are too
    /// detailed to index. Known as soon as the object is prepared, before any hole is placed;
    /// the object is shown hollowed all the same, without its holes.
    #[must_use]
    pub fn drainage_unavailable(&self) -> Option<&PipelineError> {
        self.drainage_unavailable.as_ref()
    }

    /// The object's hole probe (TD-089): resolves a hole and reports its findings, as
    /// preparation does, so holes can be checked while they are placed. Built for objects with
    /// holes and objects holes are being placed on (see [`prepare_with`]).
    #[must_use]
    pub fn probe(&self) -> Option<&HoleProbe> {
        self.probe.as_deref()
    }
}

/// An object's drain holes as cut (TD-089): the resolved cylinders, what each does, and the
/// cavity regions they open.
#[derive(Debug)]
pub struct PreparedHoles {
    cuts: HoleCuts,
    specs: Vec<HoleSpec>,
    findings: Vec<HoleFindings>,
    regions: Vec<CavityRegion>,
}

impl PreparedHoles {
    /// The resolved cylinders, in hole order.
    #[must_use]
    pub fn specs(&self) -> &[HoleSpec] {
        &self.specs
    }

    /// What each hole does, in hole order.
    #[must_use]
    pub fn findings(&self) -> &[HoleFindings] {
        &self.findings
    }

    /// The cavity regions, each with the holes (1-based) that open it to the outside.
    #[must_use]
    pub fn regions(&self) -> &[CavityRegion] {
        &self.regions
    }

    /// Cavity regions no hole opens to the outside.
    #[must_use]
    pub fn enclosed(&self) -> usize {
        self.regions
            .iter()
            .filter(|r| r.drained_by.is_empty())
            .count()
    }

    /// The cylinders as built, for display.
    #[must_use]
    pub fn cutters(&self) -> &TriangleMesh {
        self.cuts.cutters()
    }

    /// The cuts slicing removes from every layer; [`HoleCuts::drill`] cuts them into a mesh.
    #[must_use]
    pub fn cuts(&self) -> &HoleCuts {
        &self.cuts
    }
}

/// The preparation of every object of a request, in plate order: each object has its own
/// result, so one refused object does not hide the others.
#[derive(Debug)]
pub struct Preparation {
    /// One result per plate object (one for a single-model request).
    pub objects: Vec<Result<ObjectPreparation, PipelineError>>,
}

/// Prepare every object of `request` for display, without slicing or exporting anything
/// (M6.4a, TD-082): import, repair, placement, hollowing, drain holes and supports, exactly as
/// [`slice()`] prepares them.
///
/// Some things differ from slicing, because a preview must show what is wrong: support findings
/// of error severity, a model above the plate without a support standing on it, and a model out
/// of the build volume (beside the LCD area or above the build height) are kept for display
/// instead of refusing the object. Slicing still refuses them.
///
/// # Errors
/// Failures that concern the whole request: unreadable profiles, an incompatible resin, or
/// invalid plate objects. Each object's own failure is in its result; after a cancellation
/// the remaining objects fail at their first checkpoint, and the caller, which owns the
/// predicate, knows that it asked for it.
pub fn prepare(
    request: &SliceRequest,
    observer: &mut dyn Observer,
    stop: &(dyn Fn(Checkpoint) -> bool + Sync),
) -> Result<Preparation, PipelineError> {
    prepare_with(request, None, &[], observer, stop)
}

/// [`prepare`], reusing and keeping cavities and imported models in `cache` (TD-089), so a moved,
/// turned or scaled object, or one whose holes changed, is not imported or hollowed again.
///
/// An object's hole probe (see [`ObjectPreparation::probe`]) is built when it has drain holes,
/// which need it, or when it is listed in `probes`, by its index in the plate: drain holes are
/// being placed on it. Building it costs about as much as the rest of a preview after a move
/// (0.46 s on a 700,000-triangle model), so other objects go without.
///
/// # Errors
/// The failures of [`prepare`].
pub fn prepare_with(
    request: &SliceRequest,
    cache: Option<&PipelineCache>,
    probes: &[usize],
    observer: &mut dyn Observer,
    stop: &(dyn Fn(Checkpoint) -> bool + Sync),
) -> Result<Preparation, PipelineError> {
    let mut ctx = Context {
        observer,
        stop: Stop(stop),
        preview: true,
        probe: false,
        cache,
        running: None,
    };
    let started = std::time::Instant::now();
    let result = prepare_all(request, probes, &mut ctx);
    ctx.close_phase();
    match &result {
        Ok(preparation) => {
            for (number, object) in (1usize..).zip(&preparation.objects) {
                if let Err(error) = object {
                    log::warn!(target: "prepare", "object {number}: {}", error.reason());
                }
            }
            log::info!(
                target: "prepare",
                "prepared {} object(s) in {:.2} s",
                preparation.objects.len(),
                started.elapsed().as_secs_f64()
            );
        }
        Err(failure) => log::warn!(target: "prepare", "preparation failed: {}", failure.message),
    }
    result.map_err(PipelineError::from)
}

fn prepare_all(
    args: &SliceRequest,
    probes: &[usize],
    ctx: &mut Context<'_>,
) -> Result<Preparation, Failure> {
    ctx.phase("Reading profiles");
    let printer = read_printer(&args.printer)?;
    let resin = read_resin(&args.resin)?;
    let settings = resin
        .calibration_for(&printer.id)
        .map_err(|e| format!("resin profile cannot be used with this printer: {e}"))?;
    let (objects, _, _) = plate::objects(args)?;
    let mut prepared = Vec::with_capacity(objects.len());
    for (index, object) in objects.into_iter().enumerate() {
        ctx.probe = probes.contains(&index);
        let result = object
            .supports
            .as_ref()
            .map(read_supports)
            .transpose()
            .map_err(Failure::from)
            .and_then(|document| {
                prepare_object(
                    object,
                    document.as_ref(),
                    &printer,
                    settings.layer_height_mm,
                    ctx,
                )
            })
            .map(|object| ObjectPreparation {
                prepared: object.prepared,
                holes: object.holes,
                holes_error: object.holes_error,
                drainage_unavailable: object.drainage_unavailable,
                repair: object.diagnosis.outcome.clone(),
                supports: object.supported.and_then(|s| s.scene),
                probe: object.probe,
            })
            .map_err(PipelineError::from);
        prepared.push(result);
    }
    Ok(Preparation { objects: prepared })
}

/// A model imported for display.
#[derive(Debug)]
pub struct ImportedMesh {
    /// The source as imported, in millimetres, without the repairs a job applies.
    pub mesh: TriangleMesh,
    /// The unit of the file's coordinates.
    pub unit: LengthUnit,
    /// Whether the file declared the unit (otherwise it was the `units` given).
    pub declared: bool,
    /// Facets left out for a non-finite coordinate (TD-092 r9); a job refuses the model unless
    /// its repairs accept them ([`RepairSpec::non_finite`]).
    pub dropped: Option<DroppedFacets>,
}

/// Why a model could not be imported for display.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImportError {
    /// The format declares no unit and none was given.
    UnitsRequired,
    /// Any other failure.
    Failed(PipelineError),
}

impl fmt::Display for ImportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnitsRequired => f.write_str("the model file declares no length unit"),
            Self::Failed(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for ImportError {}

/// Import a model for display: the format its name selects, in `units` when it declares none.
///
/// # Errors
/// [`ImportError::UnitsRequired`] when the file declares no unit and `units` is `None`; otherwise
/// an unreadable or oversized file, an unknown format, a unit conflict, malformed content, or
/// cancellation.
pub fn import_mesh(
    model: &ModelSource,
    units: Option<LengthUnit>,
    stop: &(dyn Fn(Checkpoint) -> bool + Sync),
) -> Result<ImportedMesh, ImportError> {
    let failed = |failure: Failure| ImportError::Failed(failure.into());
    let bytes = read_model(model).map_err(|message| failed(message.into()))?;
    match import_model(units, false, model, &bytes, Stop(stop)) {
        Ok(imported) => Ok(ImportedMesh {
            dropped: dropped_facets(&imported.notes).cloned(),
            mesh: imported.mesh,
            unit: imported.unit,
            declared: imported.unit_origin == UnitOrigin::Declared,
        }),
        // Only a file without a declared unit asks for one.
        Err(failure) if failure.remedies == [Remedy::GiveUnits] => Err(ImportError::UnitsRequired),
        Err(failure) => Err(failed(failure)),
    }
}

/// The caller's cancellation predicate, shared by every stage and worker thread.
#[derive(Clone, Copy)]
struct Stop<'a>(&'a (dyn Fn(Checkpoint) -> bool + Sync));

impl<'a> Stop<'a> {
    /// A predicate for one [`Cancellation`] controller.
    fn predicate(self) -> impl FnMut(Checkpoint) -> bool + 'a {
        move |checkpoint| (self.0)(checkpoint)
    }
}

/// What every stage receives: the observer and the cancellation predicate.
struct Context<'a> {
    observer: &'a mut dyn Observer,
    stop: Stop<'a>,
    /// Preparing for display: keep support findings instead of refusing (see [`prepare`]).
    preview: bool,
    /// Preparing for display an object drain holes are being placed on: build its hole probe
    /// even without holes (see [`prepare_with`]).
    probe: bool,
    /// Cavities and models computed earlier, reused while their inputs are the same (TD-089).
    cache: Option<&'a PipelineCache>,
    /// The phase running and when it started, for the session log (TD-096).
    running: Option<(String, std::time::Instant)>,
}

/// What previews and slices keep between runs (TD-089): cavities, and models as imported.
///
/// **Cavities:** a hollow is computed in its object's model frame, so it depends only on the
/// model and the hollowing settings, never on where the object lies. Its key is the hollow plan's
/// canonical record (the source digest, the repairs, the wall, the grid, the simplification and
/// the query error) with the format and unit the model was read in.
///
/// **Models:** a model held in memory (a project's, [`ModelSource::Embedded`]) is kept as
/// imported, with its defects, its digest once computed, and its repairs for each repair
/// setting asked for. Its key is the identity of its bytes, which the entry keeps alive, with
/// the file name and the unit choices. Models read from files are not kept: a file may change.
///
/// Every step kept is deterministic, so what the cache gives is what a new computation would
/// give, bit for bit: the cache changes timing only. The least recently used entries of each
/// kind are dropped beyond `max_bytes` of their meshes; the latest one is always kept.
pub struct PipelineCache {
    cavities: Mutex<Vec<CachedHollow>>,
    probes: Mutex<Vec<CachedProbe>>,
    models: Mutex<Vec<CachedModel>>,
    max_bytes: u64,
}

struct CachedHollow {
    key: Vec<u8>,
    model: Arc<SampledHollowedModel>,
    bytes: u64,
}

/// A hollowed object's hole probe, under its cavity's key and its exact placement (TD-094).
struct CachedProbe {
    key: Vec<u8>,
    probe: Arc<HoleProbe>,
    bytes: u64,
}

/// A model as imported and checked.
struct CachedModel {
    /// The bytes it was read from, held so that their identity, the key, stays unique.
    bytes: Arc<[u8]>,
    name: String,
    units: Option<LengthUnit>,
    legacy_stl: bool,
    import: Arc<ImportedSource>,
    /// The SHA-256 of the bytes, once a step needed it.
    digest: Option<[u8; 32]>,
    /// The repairs for each repair setting asked for.
    repairs: Vec<(RepairSpec, Arc<RepairedSource>)>,
}

/// A model as imported, with the defects found in it.
struct ImportedSource {
    mesh: Arc<TriangleMesh>,
    source: ImportFacts,
    found: DefectReport,
}

/// The repairs made to an imported model: the repaired mesh (`None` when nothing changed), the
/// report, and the defects that remain.
struct RepairedSource {
    mesh: Option<Arc<TriangleMesh>>,
    report: SourceRepairReport,
    remaining: Option<DefectReport>,
    /// The full diagnosis of the repaired mesh, made the first time a preview needs it.
    state: std::sync::OnceLock<crate::diagnosis::Diagnosis>,
}

/// A model as the cache or a new import gives it: imported, its digest when computed, and its
/// repairs for the settings asked for, when they are known.
struct ModelState {
    import: Arc<ImportedSource>,
    digest: Option<[u8; 32]>,
    repaired: Option<Arc<RepairedSource>>,
}

/// The bytes of a mesh's vertices and faces.
fn mesh_bytes(mesh: &TriangleMesh) -> u64 {
    (std::mem::size_of_val(mesh.vertices()) + std::mem::size_of_val(mesh.triangles())) as u64
}

impl CachedModel {
    fn matches(&self, bytes: &Arc<[u8]>, name: &str, args: &SliceRequest) -> bool {
        Arc::ptr_eq(&self.bytes, bytes)
            && self.name == name
            && self.units == args.units
            && self.legacy_stl == args.legacy_stl
    }

    fn size(&self) -> u64 {
        mesh_bytes(&self.import.mesh)
            + self
                .repairs
                .iter()
                .filter_map(|(_, r)| r.mesh.as_deref())
                .map(mesh_bytes)
                .sum::<u64>()
    }
}

impl fmt::Debug for PipelineCache {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PipelineCache")
            .field("cavities", &self.cavities())
            .field("models", &self.models())
            .field("probes", &self.probes())
            .field("max_bytes", &self.max_bytes)
            .finish()
    }
}

impl Default for PipelineCache {
    /// Up to 1 GiB each of cavity meshes, model meshes and hole probes.
    fn default() -> Self {
        Self::new(1 << 30)
    }
}

impl PipelineCache {
    /// An empty cache keeping up to `max_bytes` of cavity meshes, and as much of model meshes
    /// and of hole probes.
    #[must_use]
    pub fn new(max_bytes: u64) -> Self {
        Self {
            cavities: Mutex::new(Vec::new()),
            probes: Mutex::new(Vec::new()),
            models: Mutex::new(Vec::new()),
            max_bytes,
        }
    }

    /// Number of cavities kept.
    #[must_use]
    pub fn cavities(&self) -> usize {
        self.cavities.lock().map_or(0, |e| e.len())
    }

    /// Number of hole probes kept.
    #[must_use]
    pub fn probes(&self) -> usize {
        self.probes.lock().map_or(0, |e| e.len())
    }

    /// Number of models kept.
    #[must_use]
    pub fn models(&self) -> usize {
        self.models.lock().map_or(0, |e| e.len())
    }

    /// The cavity kept under `key`, marked as the most recently used.
    fn cavity(&self, key: &[u8]) -> Option<Arc<SampledHollowedModel>> {
        let mut entries = self.cavities.lock().ok()?;
        let index = entries.iter().position(|e| e.key == key)?;
        let entry = entries.remove(index);
        let model = Arc::clone(&entry.model);
        entries.push(entry);
        Some(model)
    }

    /// The hole probe kept under `key`, marked as the most recently used.
    fn probe(&self, key: &[u8]) -> Option<Arc<HoleProbe>> {
        let mut entries = self.probes.lock().ok()?;
        let index = entries.iter().position(|e| e.key == key)?;
        let entry = entries.remove(index);
        let probe = Arc::clone(&entry.probe);
        entries.push(entry);
        Some(probe)
    }

    /// Keep `probe` under `key`, dropping the least recently used beyond the byte bound. It is
    /// shared with the preparation that built it, so keeping the latest costs nothing more.
    fn insert_probe(&self, key: Vec<u8>, probe: Arc<HoleProbe>) {
        let Ok(mut entries) = self.probes.lock() else {
            return;
        };
        let bytes = probe.retained_bytes();
        entries.retain(|e| e.key != key);
        entries.push(CachedProbe { key, probe, bytes });
        let mut total: u64 = entries.iter().map(|e| e.bytes).sum();
        while total > self.max_bytes && entries.len() > 1 {
            total -= entries.remove(0).bytes;
        }
    }

    /// Keep `model` under `key`, dropping the least recently used beyond the byte bound.
    fn insert_cavity(&self, key: Vec<u8>, model: Arc<SampledHollowedModel>) {
        let Ok(mut entries) = self.cavities.lock() else {
            return;
        };
        let bytes = mesh_bytes(model.mesh()) + mesh_bytes(model.cavity());
        entries.retain(|e| e.key != key);
        entries.push(CachedHollow { key, model, bytes });
        let mut total: u64 = entries.iter().map(|e| e.bytes).sum();
        while total > self.max_bytes && entries.len() > 1 {
            total -= entries.remove(0).bytes;
        }
    }

    /// The model kept for `bytes` read as `name` with `args`' unit choices, its digest and its
    /// repairs for `spec`, marked as the most recently used.
    fn model(
        &self,
        bytes: &Arc<[u8]>,
        name: &str,
        args: &SliceRequest,
        spec: RepairSpec,
    ) -> Option<ModelState> {
        let mut entries = self.models.lock().ok()?;
        let index = entries.iter().position(|e| e.matches(bytes, name, args))?;
        let entry = entries.remove(index);
        let found = ModelState {
            import: Arc::clone(&entry.import),
            digest: entry.digest,
            repaired: entry
                .repairs
                .iter()
                .find(|(s, _)| *s == spec)
                .map(|(_, r)| Arc::clone(r)),
        };
        entries.push(entry);
        Some(found)
    }

    /// Keep what was computed for a model: its import, and its digest and repairs when given.
    fn insert_model(
        &self,
        bytes: &Arc<[u8]>,
        name: &str,
        args: &SliceRequest,
        import: Arc<ImportedSource>,
        digest: Option<[u8; 32]>,
        repair: (RepairSpec, Arc<RepairedSource>),
    ) {
        let Ok(mut entries) = self.models.lock() else {
            return;
        };
        let mut entry = match entries.iter().position(|e| e.matches(bytes, name, args)) {
            Some(index) => entries.remove(index),
            None => CachedModel {
                bytes: Arc::clone(bytes),
                name: name.to_owned(),
                units: args.units,
                legacy_stl: args.legacy_stl,
                import,
                digest: None,
                repairs: Vec::new(),
            },
        };
        entry.digest = entry.digest.or(digest);
        if !entry.repairs.iter().any(|(s, _)| *s == repair.0) {
            entry.repairs.push(repair);
        }
        entries.push(entry);
        let mut total: u64 = entries.iter().map(CachedModel::size).sum();
        while total > self.max_bytes && entries.len() > 1 {
            total -= entries.remove(0).size();
        }
    }
}

impl Context<'_> {
    fn phase(&mut self, name: &str) {
        // Progress within one phase (a percentage, a pass, a count) is the same phase.
        let same = self
            .running
            .as_ref()
            .is_some_and(|(running, _)| phase_key(running) == phase_key(name));
        if !same {
            self.close_phase();
            self.running = Some((name.to_owned(), std::time::Instant::now()));
        }
        self.observer.phase(name);
    }

    /// Log the running phase's duration (TD-096): from a tenth of a second at `info`, shorter
    /// ones at `debug`, so frequent previews stay brief.
    fn close_phase(&mut self) {
        let Some((name, started)) = self.running.take() else {
            return;
        };
        let seconds = started.elapsed().as_secs_f64();
        let target = if self.preview { "prepare" } else { "slice" };
        let level = if seconds >= 0.1 {
            log::Level::Info
        } else {
            log::Level::Debug
        };
        log::log!(target: target, level, "{}: {seconds:.2} s", phase_key(&name).trim_end());
    }

    fn begin_layers(&mut self, total: usize) {
        self.observer.begin_layers(total);
    }
}

/// A phase's name without its progress: up to its first digit.
fn phase_key(name: &str) -> &str {
    name.find(|c: char| c.is_ascii_digit())
        .map_or(name, |at| &name[..at])
}

/// Reports each successfully produced layer to the observer.
struct CountLayers<'o, I> {
    inner: I,
    observer: &'o mut dyn Observer,
    completed: usize,
}

impl<E, I: Iterator<Item = Result<RasterImage, E>>> Iterator for CountLayers<'_, I> {
    type Item = Result<RasterImage, E>;

    fn next(&mut self) -> Option<Self::Item> {
        let item = self.inner.next();
        if let Some(Ok(image)) = &item {
            self.completed += 1;
            self.observer.mask(image);
            self.observer.layers(self.completed);
        }
        item
    }
}

/// Upper bound on scheduled layers; far above any supported printer's Z range at its minimum height.
const MAX_LAYERS: u64 = 1_000_000;

/// Per-layer owned geometry cap, matching the layer engine's reviewed examples.
const MAX_LAYER_PAYLOAD_BYTES: usize = 64 * 1024 * 1024;

/// Source faces scanned per plane: the M3 closure's validated large-input budget.
const MAX_SECTION_FACES: usize = 2_000_000;

/// Exact planar-contact pair work per layer: the M3 closure's validated large-input budget.
/// The pair check is quadratic in a layer's contour segments, so dense models can still exceed it.
const MAX_VALIDITY_PAIRS: usize = 40_000_000;

/// Requested bytes of the per-face Z index (24 per face): bounds models to ~89 million faces.
const MAX_FACE_INDEX_BYTES: usize = 2 << 30;

/// Plan distance error without drain holes: the hollowing query margin only. Far above the
/// binary64 query budget for any admitted model, far below any meaningful wall tolerance.
const HOLLOW_QUERY_ERROR_MM: f64 = 1e-6;

/// Plan distance error with drain holes: also the cylinder approximation budget. One micrometre
/// is far below any LCD pixel and keeps cylinders to a few hundred sides. Plans also require it
/// to be at most cell/32.
const DRAIN_DISTANCE_ERROR_MM: f64 = 1e-3;

/// Plan ID of the source repair operation; hollowing is 1 and drain holes count from 2.
const REPAIR_OPERATION_ID: u64 = u64::MAX;

/// Plan ID of the cavity simplification (TD-066), after the hollow.
const SIMPLIFY_OPERATION_ID: u64 = u64::MAX - 1;

/// How messages name the model of a single-model or per-object request.
fn model_label(args: &SliceRequest) -> String {
    args.model().map_or_else(String::new, ModelSource::label)
}

/// Model geometry handed to slicing.
#[derive(Debug)]
enum Prepared {
    Solid,
    Hollow(Box<SampledHollowedModel>),
}

impl Prepared {
    fn hollow(&self) -> Option<&SampledHollowedModel> {
        match self {
            Self::Solid => None,
            Self::Hollow(model) => Some(model),
        }
    }
}

/// One object prepared at its design placement. Every instance of it on a plate is this geometry
/// moved rigidly in XY (TD-065).
struct PreparedObject {
    /// The object's options (the command line, or one plate object).
    args: SliceRequest,
    /// The imported (and possibly repaired) source mesh, shared with the cache.
    mesh: Arc<TriangleMesh>,
    diagnosis: Diagnosis,
    placement: ObjectTransform,
    /// Highest point of the model and its supports.
    top_z: f64,
    prepared: Prepared,
    /// The drain holes as cut (TD-089), when the object has any.
    holes: Option<PreparedHoles>,
    /// Preparing for display only: why the holes could not be cut.
    holes_error: Option<PipelineError>,
    /// Preparing for display only: why no hole can be cut at all.
    drainage_unavailable: Option<PipelineError>,
    /// The hole probe, kept for display (shared with the cache).
    probe: Option<Arc<HoleProbe>>,
    supported: Option<Supported>,
}

impl PreparedObject {
    /// The surface that is sliced and its transform: the model with its supports, the hollowed
    /// shell (already in world coordinates), or the source mesh under its placement.
    fn surface(&self) -> (&TriangleMesh, SectionTransform<'_>) {
        match (&self.supported, self.prepared.hollow()) {
            (Some(supported), _) => (&supported.mesh, SectionTransform::Identity),
            (None, Some(model)) => (model.mesh(), SectionTransform::Identity),
            (None, None) => (&self.mesh, SectionTransform::Object(&self.placement)),
        }
    }

    /// Drain cutters, removed from every layer analytically.
    fn cut(&self) -> Option<&HoleCuts> {
        self.holes.as_ref().map(|h| &h.cuts)
    }

    /// The faces of [`Self::surface`] that are supports: drain cutters leave their material
    /// (TD-123). `None` without supports.
    fn kept_faces(&self) -> Option<Range<usize>> {
        self.supported
            .as_ref()
            .map(|s| s.model_faces..s.mesh.triangles().len())
    }
}

/// Import, place, hollow and support one object. `arranged` objects are checked against the
/// plate only after arrangement.
fn prepare_object(
    args: SliceRequest,
    document: Option<&SupportDocument>,
    printer: &PrinterProfile,
    layer_height_mm: f64,
    ctx: &mut Context<'_>,
) -> Result<PreparedObject, Failure> {
    let bound_digest = document
        .as_ref()
        .and_then(|d| d.binding.as_ref())
        .is_some_and(|b| b.model_sha256.is_some());
    let stop = ctx.stop;
    let (mesh, diagnosis, digest) = load_model(&args, bound_digest, ctx)?;
    let (placement, top_z) = place(
        &mesh,
        args.translate_mm,
        &args.pose,
        printer,
        Checks {
            supported: args.supports.is_some() || args.allow_floating,
            // A preview prepares an object out of the build volume all the same; slicing
            // refuses it.
            plate_xy: !args.arrange && !ctx.preview,
            height: !ctx.preview,
            resting: !ctx.preview,
        },
        &mut Cancellation::new(&mut stop.predicate()),
    )?;
    check_drain_resolution(&args.drain_holes, printer, layer_height_mm)?;
    let simplify_mm = args
        .hollow_simplify
        .map(|s| s.resolve(printer, layer_height_mm))
        .transpose()?;
    let (prepared, cavity) = match (args.hollow_mm, digest) {
        (Some(thickness), Some(digest)) => {
            let read_as = format!("{} {:?}", diagnosis.source.format, diagnosis.source.unit);
            let settings = HollowSettings {
                digest,
                thickness,
                simplify_mm,
                read_as: &read_as,
            };
            let (prepared, key) = hollow_object(&args, &mesh, &settings, placement, ctx)?;
            (prepared, Some(key))
        }
        _ => (Prepared::Solid, None),
    };
    // TD-089: holes are cut into the hollowed shell or the solid alike. A preview shows the
    // object without holes that cannot be cut. A hollowed object's probe is kept under its
    // cavity and exact placement, so editing holes does not index it again.
    let probe_key = cavity.map(|mut key| {
        key.extend_from_slice(b"\0probe\0");
        key.extend_from_slice(format!("{placement:?}").as_bytes());
        key
    });
    let (probe, drainage_unavailable) =
        probe_for(&args, &prepared, &mesh, &placement, probe_key, ctx)?;
    let (holes, holes_error) = match &probe {
        Some(probe) if !args.drain_holes.is_empty() => {
            match drill(
                &args,
                probe,
                &prepared,
                &placement,
                printer,
                layer_height_mm,
                ctx,
            ) {
                Ok(holes) => (Some(holes), None),
                Err(message) if ctx.preview => {
                    log::warn!(target: "holes", "{message}");
                    (None, Some(PipelineError::from(Failure::from(message))))
                }
                Err(message) => return Err(message.into()),
            }
        }
        _ => (None, None),
    };
    let supported = match (&args.supports, document) {
        (Some(source), Some(document)) => {
            let input = SupportInputs {
                document,
                args: &args,
                mesh: &mesh,
                placement: &placement,
                prepared: &prepared,
                holes: holes.as_ref(),
                digest,
                printer,
            };
            Some(apply_supports(&input, &source.label(), ctx)?)
        }
        _ => None,
    };
    let top_z = supported.as_ref().map_or(top_z, |s| s.top_z.max(top_z));
    Ok(PreparedObject {
        args,
        mesh,
        diagnosis,
        placement,
        top_z,
        prepared,
        holes,
        holes_error,
        drainage_unavailable: drainage_unavailable
            .map(|message| PipelineError::from(Failure::from(message))),
        probe: ctx.preview.then_some(probe).flatten(),
        supported,
    })
}

#[allow(clippy::too_many_lines)] // The pipeline's stages, in order.
fn run(args: &SliceRequest, ctx: &mut Context<'_>) -> Result<SliceOutcome, Failure> {
    let stop = ctx.stop;
    // Layer workers share the caller's predicate; the exporter holds its own controller.
    let mut export_cancel = stop.predicate();
    ctx.phase("Reading profiles");
    let printer = read_printer(&args.printer)?;
    let resin = read_resin(&args.resin)?;
    let settings = resin
        .calibration_for(&printer.id)
        .map_err(|e| format!("resin profile cannot be used with this printer: {e}"))?;
    let machine = machine_exporter_with(&args.format, args.antialias)?;
    log::info!(
        target: "slice",
        "slicing to {} for {} with {}{}",
        args.format,
        printer.name,
        resin.name,
        if args.verify_walls {
            ", verifying the walls"
        } else {
            ""
        }
    );
    let (objects, names, spacing_mm) = plate::objects(args)?;
    // Support documents are read first, so their errors come before any heavy work.
    let documents = objects
        .iter()
        .map(|o| o.supports.as_ref().map(read_supports).transpose())
        .collect::<Result<Vec<_>, _>>()?;
    let layer_plan = args.layer_plan.as_ref().map(read_layer_plan).transpose()?;

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
    // Plates and arranged models are united in world coordinates; a single placed model keeps
    // its exact placement transform.
    let layout = (args.model().is_none() || args.arrange)
        .then(|| plate::Layout::new(&prepared, names, &printer, spacing_mm))
        .transpose()?;
    let united;
    let plate_cut;
    // The supports, which drain cutters leave (TD-123).
    let kept_faces: Vec<Range<usize>>;
    let (slice_mesh, transform, cut, top_z): (_, _, Option<&(dyn LayerCut + Sync)>, _) =
        match &layout {
            None => {
                let object = &prepared[0];
                let (mesh, transform) = object.surface();
                let cut = object.cut().map(|m| m as &(dyn LayerCut + Sync));
                kept_faces = object.kept_faces().into_iter().collect();
                (mesh, transform, cut, object.top_z)
            }
            Some(layout) => {
                let (mesh, kept) = layout.united(&prepared)?;
                united = mesh;
                kept_faces = kept;
                plate_cut = layout.cut(&prepared);
                let cut = plate_cut.as_ref().map(|c| c as &(dyn LayerCut + Sync));
                (&united, SectionTransform::Identity, cut, layout.top_z)
            }
        };
    // Thumbnails show the sliced surface: the repaired model, or the hollowed shell.
    let machine = with_previews(machine, slice_mesh, transform, ctx)?;

    ctx.phase("Planning layers");
    let (plan, overrides) = match &layer_plan {
        Some(document) => {
            // Adaptive heights follow the models' slopes, never the supports'.
            let surface = document
                .is_adaptive()
                .then(|| match &layout {
                    None => placed_mesh(&prepared[0].mesh, &prepared[0].placement),
                    Some(layout) => layout.models(&prepared),
                })
                .transpose()?;
            plan_layer_document(document, settings, surface.as_ref(), top_z, stop)?
        }
        None => (plan_layers(settings.layer_height_mm, top_z)?, Vec::new()),
    };
    // SL1 archives store one layer height and exposure; refuse before rasterizing.
    if !overrides.is_empty() && args.format.starts_with("prusa-sl1") {
        return Err(format!(
            "{} stores one layer height and exposure, but the layer plan gives {} layers their own",
            args.format,
            overrides.len()
        )
        .into());
    }
    let job = print_job(&args.job_id, &printer, &resin, &plan, &overrides, stop)?;
    let layers = (0..job.layer_count())
        .map(|index| {
            job.layer(index).map(|layer| PrintedLayer {
                top_mm: layer.position_z_mm,
                thickness_mm: layer.thickness_mm,
                phase: layer.phase,
                exposure_s: layer.settings.exposure_s,
            })
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("cannot resolve the job's layers: {e}"))?;

    let (lcd, raster, bottom) = raster_setup(&printer, args, settings.bottom_layer_count)?;
    let sample_mm2 = sample_area_mm2(&printer, args.antialias);
    // Layers are generated on every core and delivered in order (TD-050); the masks are identical
    // to sequential generation for any thread count.
    let spec = ParallelRaster {
        mesh: slice_mesh,
        schedule: &plan,
        transform,
        layer_config: layer_config()?,
        represented_coordinates: true,
        face_index_bytes: Some(MAX_FACE_INDEX_BYTES),
        // A layer plane exactly through a horizontal face (such as the top of a step) would yield
        // that face instead of the material on either side; planes avoid every vertex height.
        vertex_avoiding_bytes: Some(MAX_FACE_INDEX_BYTES),
        profile: lcd,
        raster,
        bottom,
        limits: RasterLimits::default(),
        // Cutter sections are analytic per plane; only the shell is sliced.
        cut,
        // Strict layers cannot tell supports apart; supported models slice as winding layers.
        kept_faces: match args.fill {
            Fill::Union => &kept_faces,
            Fill::Strict => &[],
        },
        fill: match (args.fill, args.reversed_tolerance_mm2) {
            (Fill::Union, None) => LayerFill::Winding(WindingRule::Positive),
            (Fill::Union, Some(area)) => LayerFill::Winding(WindingRule::PositiveBounded {
                max_negative_samples: samples_within(area, sample_mm2),
            }),
            (Fill::Strict, _) => LayerFill::Strict,
        },
        closure: args
            .close_holes_mm
            .map(|max_gap_mm| SectionClosure { max_gap_mm }),
        threads: 0,
        window_bytes: ParallelRaster::DEFAULT_WINDOW_BYTES,
    };
    ctx.begin_layers(plan.planes().len());
    let limits = ExportLimits::default();
    let mut cancel = Cancellation::new(&mut export_cancel);
    let mut facts = LayerFacts {
        sample_mm2,
        ..LayerFacts::default()
    };
    // TD-095: the verification reads the exported masks again.
    let mut kept: Vec<karasu_core::raster::mask::SpanMask> = Vec::new();
    let observer = &mut *ctx.observer;
    let export = with_parallel_raster(&spec, stop.0, |masks| {
        let masks = CountLayers {
            inner: masks,
            observer,
            completed: 0,
        }
        .inspect(|item| {
            if let Ok(image) = item {
                facts.observe(image);
                if args.verify_walls
                    || args.find_unsupported.is_some()
                    || args.find_suction_cups.is_some()
                {
                    kept.push(image.mask().clone());
                }
            }
        });
        match &machine {
            // Layers are also encoded on all cores and written in order (identical bytes).
            Some(exporter) => export_machine_parallel(
                &args.output,
                &job,
                exporter,
                masks,
                limits,
                ParallelEncoding::default(),
                stop.0,
                &mut cancel,
            ),
            None => export_reference(&args.output, &job, masks, limits, &mut cancel),
        }
    })
    .map_err(|e| export_failure(args, &e))?;
    let totals = Totals {
        export: &export,
        facts: &facts,
        resin: resin.usage(export.coverage_volume_mm3),
        pixel_mm: std::array::from_fn(|axis| {
            printer.build_size_mm[axis] / f64::from(printer.resolution_px[axis])
        }),
        bottom_layers: settings.bottom_layer_count,
        layer_heights: plan.slabs().map(|slabs| {
            let heights = slabs.iter().map(|slab| slab.height_mm);
            (
                [
                    heights.clone().fold(f64::INFINITY, f64::min),
                    heights.fold(0., f64::max),
                ],
                overrides.len(),
            )
        }),
    };
    let verification = args
        .verify_walls
        .then(|| {
            ctx.phase("Verifying walls");
            let cavities = match &layout {
                None => Ok(None),
                Some(layout) => layout.cavities(&prepared),
            }?;
            let cavity = match &layout {
                None => prepared[0]
                    .prepared
                    .hollow()
                    .map(SampledHollowedModel::cavity),
                Some(_) => cavities.as_ref(),
            };
            verification::verify(
                &verification::Job {
                    masks: &kept,
                    spec: &spec,
                    cavity,
                    walls: walls(&prepared),
                },
                stop.0,
            )
            .map(Box::new)
        })
        .transpose()?;
    let unsupported = args
        .find_unsupported
        .map(|angle| {
            ctx.phase("Finding unsupported regions");
            unsupported::find(&kept, &spec, angle, stop.0).map(Box::new)
        })
        .transpose()?;
    let suction_cups = args
        .find_suction_cups
        .map(|min_vent_mm2| {
            ctx.phase("Finding suction cups");
            suction::find(&kept, &spec, min_vent_mm2, stop.0).map(Box::new)
        })
        .transpose()?;
    let machine = machine.as_ref();
    let mut text = String::new();
    match &layout {
        None => summary::write(&mut text, args, machine, &totals, &prepared[0]),
        Some(layout) => plate::write_summary(&mut text, args, machine, &totals, &prepared, layout),
    }
    .and_then(|()| {
        verification
            .as_ref()
            .map_or(Ok(()), |v| verification::write(&mut text, v))
    })
    .and_then(|()| {
        unsupported
            .as_ref()
            .map_or(Ok(()), |u| unsupported::write(&mut text, u))
    })
    .and_then(|()| {
        suction_cups
            .as_ref()
            .map_or(Ok(()), |c| suction::write(&mut text, c))
    })
    .map_err(|_| "cannot format the slice summary".to_owned())?;
    Ok(SliceOutcome {
        summary: text,
        resin: totals.resin,
        export,
        verification,
        unsupported,
        suction_cups,
        layers,
    })
}

/// The thinnest requested and certified walls of the hollowed objects as placed (TD-095).
fn walls(prepared: &[PreparedObject]) -> Walls {
    let certificates: Vec<_> = prepared
        .iter()
        .filter_map(|o| o.prepared.hollow().map(SampledHollowedModel::certificate))
        .collect();
    let requested = certificates
        .iter()
        .map(|c| c.thickness_mm * c.placement_scale[0])
        .fold(f64::INFINITY, f64::min);
    let certified = certificates
        .iter()
        .map(|c| c.surface_distance_mm[0])
        .fold(f64::INFINITY, f64::min);
    Walls {
        requested_mm: if requested.is_finite() {
            requested
        } else {
            UNHOLLOWED_WALL_MM
        },
        certified_mm: certified.is_finite().then_some(certified),
    }
}

/// Measured results of a completed slice, for the summary.
struct Totals<'a> {
    export: &'a ExportSummary,
    facts: &'a LayerFacts,
    /// Resin for the sampled mask volume.
    resin: ResinUsage,
    /// LCD pixel size in X and Y, in mm.
    pixel_mm: [f64; 2],
    /// Bottom layers of the job.
    bottom_layers: u32,
    /// With a layer plan: the thinnest and thickest layer (mm) and the layers with explicit
    /// settings.
    layer_heights: Option<([f64; 2], usize)>,
}

/// What the union fill did across the job's layers.
#[derive(Default)]
struct LayerFacts {
    /// Layers whose fill merged overlapping material.
    merged: usize,
    /// Layers where section closure added chords.
    closed_layers: usize,
    /// Chords added across the job.
    chords: usize,
    /// Longest chord (mm) and its layer index.
    longest: Option<(f64, u64)>,
    /// Area of one raster sample, in mm².
    sample_mm2: f64,
    /// Layers with negative winding left empty under the reversed-sliver tolerance.
    reversed_layers: usize,
    /// Negative samples across the job.
    reversed_samples: u64,
    /// Most negative samples in one layer, and that layer's index.
    largest_reversed: Option<(u64, u64)>,
}

impl LayerFacts {
    fn observe(&mut self, image: &RasterImage) {
        if image.winding().is_some_and(|range| range.max > 1) {
            self.merged += 1;
        }
        if let Some(samples) = image
            .winding()
            .map(|range| range.negative_samples)
            .filter(|&n| n > 0)
        {
            self.reversed_layers += 1;
            self.reversed_samples += samples;
            if self.largest_reversed.is_none_or(|(most, _)| samples > most) {
                self.largest_reversed = Some((samples, image.plane().index));
            }
        }
        if let Some(closure) = image.closure().filter(|c| c.chords > 0) {
            self.closed_layers += 1;
            self.chords += closure.chords;
            if self.longest.is_none_or(|(mm, _)| closure.longest_mm > mm) {
                self.longest = Some((closure.longest_mm, image.plane().index));
            }
        }
    }
}

/// The defects of `mesh`.
fn defects(mesh: &TriangleMesh, stop: Stop<'_>) -> Result<DefectReport, String> {
    mesh.defect_report_with_cancellation(
        TopologyLimits::default(),
        &mut Cancellation::new(&mut stop.predicate()),
    )
    .map_err(|e| format!("cannot analyze model topology: {e}"))
}

/// The repairs `spec` makes to `mesh`, and the defects that remain after them.
fn repair(mesh: &TriangleMesh, spec: RepairSpec, stop: Stop<'_>) -> Result<RepairedSource, String> {
    let (repaired, report) = repair_source(
        mesh,
        spec,
        TopologyLimits::default(),
        &mut Cancellation::new(&mut stop.predicate()),
    )
    .map_err(|e| format!("cannot repair the model: {e}"))?;
    let remaining = repaired.as_ref().map(|m| defects(m, stop)).transpose()?;
    Ok(RepairedSource {
        mesh: repaired.map(Arc::new),
        report,
        remaining,
        state: std::sync::OnceLock::new(),
    })
}

/// Import the model, diagnose its defects and admit it for the selected fill.
///
/// Only a preparation plan binds the source digest. The file bytes, facet metadata and import
/// diagnostics are released right after import: large models need that memory later.
/// The mesh to prepare, its diagnosis, and its digest when hollowing or a bound document needs
/// it.
type LoadedModel = (Arc<TriangleMesh>, Diagnosis, Option<[u8; 32]>);

/// Read, import, check and repair a model that the cache does not hold.
fn import_fresh(
    args: &SliceRequest,
    model: &ModelSource,
    spec: RepairSpec,
    need_digest: bool,
    ctx: &mut Context<'_>,
) -> Result<ModelState, Failure> {
    let stop = ctx.stop;
    let bytes = read_model(model)?;
    let digest = need_digest.then(|| sha256(&bytes));
    let imported = import_model(args.units, args.legacy_stl, model, &bytes, stop)?;
    drop(bytes);
    let source = ImportFacts {
        format: imported.format,
        unit: imported.unit,
        origin: imported.unit_origin,
        objects: imported.objects,
        notes: imported.notes,
    };
    let mesh = imported.mesh;
    drop(imported.diagnostics);
    log::info!(
        target: "models",
        "imported {}: {} triangles",
        model_label(args),
        mesh.triangles().len()
    );
    ctx.phase("Checking topology");
    // Diagnosis and repair both read the imported mesh only, so they run concurrently; a
    // diagnosis failure is still reported first.
    let (found, repaired) = std::thread::scope(|scope| {
        let repaired = scope.spawn(|| repair(&mesh, spec, stop));
        let found = defects(&mesh, stop);
        (found, repaired.join())
    });
    let found = found?;
    let repaired = repaired.map_err(|_| "model repair stopped unexpectedly".to_string())??;
    Ok(ModelState {
        import: Arc::new(ImportedSource {
            mesh: Arc::new(mesh),
            source,
            found,
        }),
        digest,
        repaired: Some(Arc::new(repaired)),
    })
}

fn load_model(
    args: &SliceRequest,
    bound_digest: bool,
    ctx: &mut Context<'_>,
) -> Result<LoadedModel, Failure> {
    ctx.phase("Importing model");
    let stop = ctx.stop;
    let model = args.model().ok_or("a plate object has no model")?;
    let need_digest = args.hollow_mm.is_some() || bound_digest;
    // Slicing alone closes holes that filling refuses in each layer; hollowing needs them all
    // filled, so its plan refuses any hole left open.
    let spec = RepairSpec {
        allow_unfilled: args.hollow_mm.is_none(),
        ..args.repair
    };
    // A model held in memory is imported, checked and repaired once (see `PipelineCache`).
    let held = match model {
        ModelSource::Embedded { name, bytes } => Some((bytes, name.as_str())),
        ModelSource::File(_) => None,
    };
    let cached = held
        .zip(ctx.cache)
        .and_then(|((bytes, name), cache)| cache.model(bytes, name, args, spec));
    let ModelState {
        import,
        digest,
        repaired,
    } = if let Some(state) = cached {
        ctx.phase("Importing model: reused");
        let digest = match (state.digest, held) {
            (None, Some((bytes, _))) if need_digest => Some(sha256(bytes)),
            (digest, _) => digest,
        };
        ModelState { digest, ..state }
    } else {
        import_fresh(args, model, spec, need_digest, ctx)?
    };
    refuse_non_finite(model, &import.source, spec)?;
    // Repairs not asked for before are made on the kept import.
    let repaired = if let Some(repaired) = repaired {
        repaired
    } else {
        ctx.phase("Checking topology");
        Arc::new(repair(&import.mesh, spec, stop)?)
    };
    if let (Some((bytes, name)), Some(cache)) = (held, ctx.cache) {
        let repair = (spec, Arc::clone(&repaired));
        cache.insert_model(bytes, name, args, Arc::clone(&import), digest, repair);
    }
    // Hollowing repairs the bound source itself through its plan; slicing uses the derived mesh.
    let mesh = match &repaired.mesh {
        Some(repaired) if args.hollow_mm.is_none() => Arc::clone(repaired),
        _ => Arc::clone(&import.mesh),
    };
    // The facets the import left out belong to the repairs' report (accepted above).
    let report = SourceRepairReport {
        non_finite: dropped_facets(&import.source.notes).cloned(),
        ..repaired.report.clone()
    };
    // A preview also diagnoses the repaired mesh in full, once per repairs (the cache keeps it).
    let outcome = if ctx.preview {
        if repaired.state.get().is_none() {
            let target = repaired.mesh.as_deref().unwrap_or(&import.mesh);
            let state =
                crate::diagnosis::diagnose(target, &mut Cancellation::new(&mut stop.predicate()))
                    .map_err(|e| format!("cannot diagnose the repaired model: {e}"))?;
            let _ = repaired.state.set(state);
        }
        repaired.state.get().map(|state| RepairOutcome {
            spec: args.repair,
            found: import.found.clone(),
            report: report.clone(),
            state: crate::diagnosis::Diagnosis {
                dropped_facets: report.non_finite.as_ref().map_or(0, |d| d.facets),
                ..state.clone()
            },
        })
    } else {
        None
    };
    let diagnosis = Diagnosis {
        source: import.source.clone(),
        found: import.found.clone(),
        repairs: report,
        remaining: repaired.remaining.clone(),
        outcome,
    };
    // Hollowing closes zero-area cracks itself (TD-092 H1); other open edges need hole filling.
    let current = diagnosis.current();
    let open_beyond_cracks = current.rim_chains > 0 || current.rim_loops > current.cracks;
    if args.hollow_mm.is_some() && args.repair.fill_holes.is_none() && open_beyond_cracks {
        return Err(Failure::with_remedies(
            format!(
                "hollowing needs a closed surface, but the model has {} boundary edges (hole \
                 rims: {})",
                diagnosis.current().boundary_edges,
                diagnosis.current().rim_loops
            ),
            "; add fill-holes to --repair (for example --repair \
             duplicates,orientation,fill-holes)",
            vec![Remedy::FillHoles],
        ));
    }
    // A preview shows the defects instead (see `RepairOutcome`); slicing still refuses them.
    if !args.allow_topology_defects && args.hollow_mm.is_none() && !ctx.preview {
        check_topology(
            &mesh,
            diagnosis.current(),
            args,
            &mut Cancellation::new(&mut stop.predicate()),
        )?;
    }
    Ok((mesh, diagnosis, digest))
}

/// The format, units and import notes of the model file.
#[derive(Clone)]
struct ImportFacts {
    format: &'static str,
    unit: LengthUnit,
    origin: UnitOrigin,
    objects: usize,
    notes: Vec<ImportNote>,
}

/// Import the model with the format its extension selects (STL for unknown extensions with the
/// legacy `--assume-mm`), resolving its unit.
fn import_model(
    units: Option<LengthUnit>,
    legacy_stl: bool,
    model: &ModelSource,
    bytes: &[u8],
    stop: Stop<'_>,
) -> Result<karasu_core::mesh::formats::ImportedModel, Failure> {
    let registry = FormatRegistry::builtin();
    let path = match model {
        ModelSource::File(path) => path.as_path(),
        ModelSource::Embedded { name, .. } => Path::new(name),
    };
    let format = match registry.for_path(path) {
        Ok(format) => format,
        Err(ModelImportError::UnknownFormat { .. }) if legacy_stl => &StlFormat,
        Err(e) => {
            let names: Vec<&str> = registry
                .formats()
                .flat_map(|f| f.extensions().iter().copied())
                .collect();
            return Err(format!(
                "cannot import {}: {e} (supported: .{})",
                model.label(),
                names.join(", .")
            )
            .into());
        }
    };
    // Facets without a position are always dropped and noted here, so the import does not
    // depend on the repairs; a job without `RepairSpec::non_finite` refuses them afterwards
    // with the import's own message (see `refuse_non_finite`).
    let options = ModelImportOptions {
        non_finite: NonFinitePolicy::Drop,
        ..ModelImportOptions::new(units)
    };
    registry
        .import_bytes(
            format,
            bytes,
            &options,
            &mut Cancellation::new(&mut stop.predicate()),
        )
        .map_err(|e| {
            let (hint, remedies) = match e {
                ModelImportError::UnitsRequired { .. } => (
                    "; give it with --units (for example --units mm)",
                    vec![Remedy::GiveUnits],
                ),
                ModelImportError::UnitConflict { .. } => (
                    "; drop --units/--assume-mm, or give the declared unit",
                    vec![Remedy::MatchDeclaredUnits],
                ),
                _ => ("", Vec::new()),
            };
            let reason = format!("cannot import {} {}: {e}", format.name(), model.label());
            Failure::with_remedies(reason, hint, remedies)
        })
}

/// The facets an import left out for a non-finite coordinate, from its notes.
fn dropped_facets(notes: &[ImportNote]) -> Option<&DroppedFacets> {
    notes.iter().find_map(|note| match note {
        ImportNote::NonFiniteFacetsDropped(dropped) => Some(dropped),
        _ => None,
    })
}

/// Refuse a model whose import dropped facets without a position unless `spec` accepts it, with
/// the message the import gives when it refuses them itself, then the remedy.
fn refuse_non_finite(
    model: &ModelSource,
    source: &ImportFacts,
    spec: RepairSpec,
) -> Result<(), Failure> {
    let Some(refusal) = dropped_facets(&source.notes)
        .filter(|_| !spec.non_finite)
        .and_then(DroppedFacets::refusal)
    else {
        return Ok(());
    };
    let format = FormatRegistry::builtin()
        .by_id(source.format)
        .map_or(source.format, |f| f.name());
    Err(Failure::with_remedies(
        format!("cannot import {format} {}: {refusal}", model.label()),
        "; leave those facets out with --repair non-finite (or --repair exact)",
        vec![Remedy::DropNonFinite],
    ))
}

/// Defects found at import, the repairs applied, and the defects that remain after them.
struct Diagnosis {
    /// Where the mesh came from.
    source: ImportFacts,
    found: DefectReport,
    repairs: SourceRepairReport,
    /// Defects of the repaired mesh, or `None` when the repairs changed nothing.
    remaining: Option<DefectReport>,
    /// For a preview, what the repairs did and the state they leave.
    outcome: Option<RepairOutcome>,
}

/// What an object's repairs did and the state they leave (TD-092 r8), for an interface.
#[derive(Clone, Debug)]
pub struct RepairOutcome {
    /// The repairs applied: the object's own, or the plate's.
    pub spec: RepairSpec,
    /// The defects of the model as imported.
    pub found: DefectReport,
    /// What the repairs did, where.
    pub report: SourceRepairReport,
    /// The diagnosis of the mesh that is sliced (the repaired one, or the model as imported when
    /// the repairs changed nothing): its findings, their locations and its printability.
    pub state: crate::diagnosis::Diagnosis,
}

impl Diagnosis {
    /// The defects of the mesh that is sliced.
    fn current(&self) -> &DefectReport {
        self.remaining.as_ref().unwrap_or(&self.found)
    }
}

/// Validated print job over the full schedule, with the layer plan's explicit settings.
fn print_job(
    id: &str,
    printer: &PrinterProfile,
    resin: &ResinProfile,
    plan: &LayerSchedule,
    overrides: &[LayerOverride],
    stop: Stop<'_>,
) -> Result<PrintJob, String> {
    PrintJob::new_with_cancellation(
        id,
        printer,
        resin,
        plan,
        overrides,
        JobLimits::default(),
        &mut Cancellation::new(&mut stop.predicate()),
    )
    .map_err(|e| format!("invalid print job: {e}"))
}

/// Printer LCD and the mask configuration: `antialias` x `antialias` samples per pixel (one gives
/// binary masks), no compensation. PP1 masks use the format's sixteen native levels, so they are
/// stored exactly; the other grayscale formats use 256 levels.
fn raster_setup(
    printer: &PrinterProfile,
    args: &SliceRequest,
    bottom_layers: u32,
) -> Result<(LcdProfile, RasterConfig, Option<BottomRaster>), String> {
    let lcd = printer
        .lcd_profile()
        .map_err(|e| format!("invalid printer LCD: {e}"))?;
    let pp1 = BuiltinExporter::new(BuiltinFormat::AnycubicPhotonP1, PixelPolicy::Exact);
    let raster = RasterConfig {
        samples_per_axis: args.antialias,
        // PW0 (PP1 and the Photon Workshop family) stores 16 exact gray levels.
        grayscale_levels: if args.format == pp1.id()
            || AnycubicWorkshop::ALL
                .iter()
                .any(|t| !t.bitplanes() && t.id() == args.format)
        {
            16
        } else {
            256
        },
        minimum_gray: args.min_gray,
        outside: OutsidePolicy::Reject,
        compensation: compensation(args.xy_compensation_px),
        // The supports binary unless asked (TD-134).
        kept: if args.antialias_supports {
            karasu_core::raster::KeptSampling::Antialiased
        } else {
            karasu_core::raster::KeptSampling::Binary
        },
    };
    // The bottom layers' own compensation (elephant-foot correction), when given.
    if args.bottom_xy_compensation_px.is_some() && bottom_layers == 0 {
        return Err(
            "--bottom-xy-compensation needs bottom layers, but the resin calibration has none"
                .to_owned(),
        );
    }
    let bottom = args.bottom_xy_compensation_px.map(|pixels| BottomRaster {
        layers: u64::from(bottom_layers),
        raster: RasterConfig {
            compensation: compensation(pixels),
            ..raster
        },
    });
    Ok((lcd, raster, bottom))
}

/// Signed pixel compensation: positive grows material (and shrinks holes), negative shrinks it.
fn compensation(pixels: i16) -> Compensation {
    let radius = pixels.unsigned_abs();
    match pixels.signum() {
        1 => Compensation::Dilate([radius; 2]),
        -1 => Compensation::Erode([radius; 2]),
        _ => Compensation::None,
    }
}

/// Sample each layer slab `[i*h, (i+1)*h]` at its midpoint, through the model's top.
fn plan_layers(height: f64, top_z: f64) -> Result<LayerSchedule, String> {
    let plan = build_layer_schedule(
        UniformScheduleSpec::try_new(height / 2., height, ScheduleEnd::Through(top_z))
            .map_err(|e| format!("invalid layer schedule: {e}"))?,
        ScheduleLimits {
            max_layers: MAX_LAYERS,
            max_payload_bytes: 64 * 1024 * 1024,
        },
    )
    .map_err(|e| format!("cannot schedule layers: {e}"))?;
    if plan.planes().is_empty() {
        return Err(format!(
            "model height {top_z} mm is below half of one {height} mm layer; nothing to print"
        ));
    }
    Ok(plan)
}

/// Plan the layers of a `karasu-layers` document (TD-064): slab heights from the plate up and the
/// explicit settings of every layer that differs from the calibration.
///
/// Adaptive ranges read the slopes of `surface`: the placed models, never their supports.
fn plan_layer_document(
    document: &LayerPlanDocument,
    settings: &PrintSettings,
    surface: Option<&TriangleMesh>,
    top_z: f64,
    stop: Stop<'_>,
) -> Result<(LayerSchedule, Vec<LayerOverride>), String> {
    let planned = plan_variable(
        document,
        settings,
        surface,
        top_z,
        usize::try_from(MAX_LAYERS).unwrap_or(usize::MAX),
        &mut Cancellation::new(&mut stop.predicate()),
    )
    .map_err(|e| format!("cannot plan layers: {e}"))?;
    let schedule = build_slab_schedule(
        &planned.heights_nm,
        ScheduleLimits {
            max_layers: MAX_LAYERS,
            max_payload_bytes: 64 * 1024 * 1024,
        },
        &mut Cancellation::new(&mut stop.predicate()),
    )
    .map_err(|e| format!("cannot schedule layers: {e}"))?;
    Ok((schedule, planned.overrides))
}

/// Read and validate a layer-plan document.
fn read_layer_plan(input: &Input<LayerPlanDocument>) -> Result<LayerPlanDocument, String> {
    let path = match input {
        Input::File(path) => path,
        Input::Loaded { value, .. } => return Ok(value.clone()),
    };
    let quoted = QuotedPath(path);
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|file| {
            file.take(u64::try_from(MAX_LAYER_PLAN_BYTES).unwrap_or(u64::MAX) + 1)
                .read_to_end(&mut bytes)
        })
        .map_err(|e| format!("cannot read layer plan {quoted}: {e}"))?;
    if bytes.len() > MAX_LAYER_PLAN_BYTES {
        return Err(format!(
            "layer plan {quoted} exceeds {MAX_LAYER_PLAN_BYTES} bytes"
        ));
    }
    LayerPlanDocument::from_json(&bytes).map_err(|e| format!("{quoted}: {e}"))
}

/// Read the model bytes once, bounded like the importers, for both digest and import. Embedded
/// bytes are borrowed, never copied.
fn read_model(model: &ModelSource) -> Result<Cow<'_, [u8]>, String> {
    let limit = ImportLimits::default().max_input_bytes;
    let path = match model {
        ModelSource::File(path) => path,
        ModelSource::Embedded { bytes, .. } if bytes.len() > limit => {
            return Err(format!(
                "cannot import model {}: file exceeds the {limit}-byte import limit",
                model.label()
            ));
        }
        ModelSource::Embedded { bytes, .. } => return Ok(Cow::Borrowed(bytes)),
    };
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|file| {
            file.take(u64::try_from(limit).unwrap_or(u64::MAX) + 1)
                .read_to_end(&mut bytes)
        })
        .map_err(|e| format!("cannot read model {}: {e}", QuotedPath(path)))?;
    if bytes.len() > limit {
        return Err(format!(
            "cannot import model {}: file exceeds the {limit}-byte import limit",
            QuotedPath(path)
        ));
    }
    Ok(Cow::Owned(bytes))
}

/// What an object is hollowed with, besides its request: the source digest, the wall and the
/// resolved simplification, and how its model was read (format and unit), for the cache key.
struct HollowSettings<'a> {
    digest: [u8; 32],
    thickness: f64,
    simplify_mm: Option<f64>,
    read_as: &'a str,
}

/// Hollow the model in its model frame, or take its cavity from the cache, and place the shell
/// in world coordinates (TD-089).
fn hollow_object(
    args: &SliceRequest,
    mesh: &TriangleMesh,
    settings: &HollowSettings<'_>,
    placement: ObjectTransform,
    ctx: &mut Context<'_>,
) -> Result<(Prepared, Vec<u8>), String> {
    let HollowSettings {
        digest,
        thickness,
        simplify_mm,
        read_as,
    } = *settings;
    let stop = ctx.stop;
    // TD-089: the source is hollowed in its model frame, so the cavity does not depend on where
    // the object lies, and the result is then placed. Drain holes are cut afterwards, on the
    // placed shell, without hollowing again.
    let cell = hollow_cell(args, thickness, surface_area(mesh));
    let hollow_plan = preparation_plan(
        args,
        digest,
        ObjectTransform::identity(),
        [thickness, cell],
        simplify_mm,
        stop,
    )?;
    let mut key = hollow_plan.canonical_record().to_vec();
    key.extend_from_slice(read_as.as_bytes());
    if let Some(model) = ctx.cache.and_then(|c| c.cavity(&key)) {
        log::debug!(target: "hollow", "{}: cavity reused", model_label(args));
        ctx.phase("Hollowing: cavity reused");
        let placed = model
            .placed(placement, &mut Cancellation::new(&mut stop.predicate()))
            .map_err(|e| format!("cannot place the hollowed {}: {e}", model_label(args)))?;
        return Ok((Prepared::Hollow(Box::new(placed)), key));
    }
    ctx.phase("Hollowing");
    let mut last_percent = None;
    let mut report = |p: SampledHollowProgress| match p.stage {
        SampledHollowStage::Field if p.total_items > 0 => {
            let percent = p.completed_items * 100 / p.total_items;
            if last_percent != Some(percent) {
                last_percent = Some(percent);
                ctx.phase(&format!("Hollowing: distance field {percent}%"));
            }
        }
        SampledHollowStage::Extraction => ctx.phase("Hollowing: extracting cavity"),
        SampledHollowStage::Certification => ctx.phase("Hollowing: certifying walls"),
        SampledHollowStage::Simplification => ctx.phase("Hollowing: simplifying cavity"),
        SampledHollowStage::Pass => ctx.phase(&format!(
            "Hollowing: pass {} of at most {}, raising the wall",
            p.completed_items, p.total_items
        )),
        SampledHollowStage::Drainage => ctx.phase(&format!(
            "Draining: certifying holes {}/{}",
            p.completed_items, p.total_items
        )),
        _ => {}
    };
    let model = hollow_mesh_sampled_with_progress(
        mesh,
        &hollow_plan,
        hollow_limits(args),
        &mut Cancellation::new(&mut stop.predicate()),
        &mut report,
    )
    .map_err(|e| {
        use karasu_core::preparation::hollow::HollowError;
        let hint = match e {
            HollowError::WallNotReached { .. } => {
                "; a thicker wall or a finer --hollow-cell may reach it"
            }
            // TD-094: within the memory budget, a thicker wall makes a coarser, smaller cavity.
            HollowError::Resource { .. } => "; a thicker wall makes the cavity smaller",
            _ => "",
        };
        format!("cannot hollow {}: {e}{hint}", model_label(args))
    })?;
    let c = model.certificate();
    log::info!(
        target: "hollow",
        "hollowed {}: wall {} mm on a {:.4} mm cell at offset {:.4} mm, {} pass(es), guaranteed \
         {:.4} mm, {} cavity triangles",
        model_label(args),
        c.thickness_mm,
        c.cell_mm,
        c.offset_mm,
        c.passes,
        c.surface_distance_mm[0],
        model.cavity().triangles().len()
    );
    let placed = model
        .placed(placement, &mut Cancellation::new(&mut stop.predicate()))
        .map_err(|e| format!("cannot place the hollowed {}: {e}", model_label(args)))?;
    if let Some(cache) = ctx.cache {
        cache.insert_cavity(key.clone(), Arc::new(model));
    }
    Ok((Prepared::Hollow(Box::new(placed)), key))
}

/// Bytes the hollowing and each drain-hole index may request when the request names no budget
/// (TD-094).
pub const DEFAULT_MEMORY_BUDGET_BYTES: u64 = 4 << 30;

/// The request's memory budget (TD-094).
fn memory_budget(args: &SliceRequest) -> u64 {
    args.memory_budget_bytes
        .unwrap_or(DEFAULT_MEMORY_BUDGET_BYTES)
}

/// The hole probe's limits under the request's budget.
fn probe_limits(args: &SliceRequest) -> ProbeLimits {
    ProbeLimits {
        max_payload_bytes: memory_budget(args),
    }
}

/// The most cavity triangles a hollowing may emit under a request's memory budget (TD-094):
/// no more than a hole index can take, so a model that can be hollowed can be drilled. Without
/// a budget, the hollowing's default limit, which the default index takes.
#[must_use]
pub fn cavity_triangle_cap(memory_budget_bytes: Option<u64>) -> u64 {
    match memory_budget_bytes {
        Some(bytes) => ProbeLimits {
            max_payload_bytes: bytes,
        }
        .max_faces(),
        None => SampledHollowLimits::default()
            .max_cavity_triangles
            .min(ProbeLimits::default().max_faces()),
    }
}

/// How much more cavity than TD-093's law predicts a model may make: the law fell up to 7 %
/// short on the twenty models of its campaign (TD-094).
const PREDICTION_MARGIN: f64 = 1.1;

/// Whether a source of `area_mm2` hollowed on `cell_mm` is predicted, with a margin, to fit
/// [`cavity_triangle_cap`] (TD-094).
#[must_use]
pub fn cavity_predicted_to_fit(
    area_mm2: f64,
    cell_mm: f64,
    memory_budget_bytes: Option<u64>,
) -> bool {
    #[allow(clippy::cast_precision_loss)] // Triangle counts far below 2^52.
    let cap = cavity_triangle_cap(memory_budget_bytes) as f64;
    predicted_cavity_triangles(area_mm2, cell_mm) * PREDICTION_MARGIN <= cap
}

/// The thinnest wall, in mm, whose cavity is predicted to fit for a source of `area_mm2`
/// hollowed on the automatic cell (TD-094, see [`cavity_predicted_to_fit`]). A prediction:
/// sources whose thin parts leave no cavity often go thinner, so the interface warns below it
/// and does not refuse.
#[must_use]
pub fn predicted_minimum_wall_mm(area_mm2: f64, memory_budget_bytes: Option<u64>) -> f64 {
    let fits = |wall: f64| {
        cavity_predicted_to_fit(
            area_mm2,
            automatic_cell(wall, area_mm2),
            memory_budget_bytes,
        )
    };
    // The prediction falls as the wall grows: bisect between a wall of a few query errors and
    // the thickest the interface takes.
    let (mut thin, mut thick) = (4.0 * HOLLOW_QUERY_ERROR_MM, 20.0);
    if fits(thin) {
        return thin;
    }
    if !fits(thick) {
        return thick;
    }
    for _ in 0..60 {
        let mid = f64::midpoint(thin, thick);
        if fits(mid) {
            thick = mid;
        } else {
            thin = mid;
        }
    }
    thick
}

/// The hollowing's limits under the request's budget (TD-094).
fn hollow_limits(args: &SliceRequest) -> SampledHollowLimits {
    SampledHollowLimits {
        max_payload_bytes: memory_budget(args),
        max_cavity_triangles: cavity_triangle_cap(args.memory_budget_bytes),
        ..SampledHollowLimits::default()
    }
}

/// The object's hole probe, when it is needed, and in a preview why none can be built. A preview
/// keeps the probe of an object holes are being placed on, to check them as they move. It also
/// says at once when the object cannot be indexed for holes at all (TD-094), and then keeps it,
/// hollowed, without them; slicing refuses it.
fn probe_for(
    args: &SliceRequest,
    prepared: &Prepared,
    mesh: &TriangleMesh,
    placement: &ObjectTransform,
    key: Option<Vec<u8>>,
    ctx: &mut Context<'_>,
) -> Result<(Option<Arc<HoleProbe>>, Option<String>), Failure> {
    let unavailable = ctx
        .preview
        .then(|| admit_probe(args, prepared, mesh).err())
        .flatten();
    if let Some(reason) = &unavailable {
        log::warn!(target: "holes", "{reason}");
    }
    if unavailable.is_some() || (args.drain_holes.is_empty() && !(ctx.preview && ctx.probe)) {
        return Ok((None, unavailable));
    }
    let cache = ctx.cache.zip(key);
    if let Some(probe) = cache.as_ref().and_then(|(cache, key)| cache.probe(key)) {
        log::debug!(target: "holes", "{}: drain-hole index reused", model_label(args));
        ctx.phase("Drain holes: index reused");
        return Ok((Some(probe), None));
    }
    ctx.phase("Drain holes: indexing");
    let started = std::time::Instant::now();
    match hole_probe(args, prepared, mesh, placement, ctx) {
        Ok(probe) => {
            log::info!(
                target: "holes",
                "{}: drain-hole index built in {:.2} s ({} bytes)",
                model_label(args),
                started.elapsed().as_secs_f64(),
                probe.retained_bytes()
            );
            let probe = Arc::new(probe);
            if let Some((cache, key)) = cache {
                cache.insert_probe(key, Arc::clone(&probe));
            }
            Ok((Some(probe), None))
        }
        Err(message) if ctx.preview => Ok((None, Some(message))),
        Err(message) => Err(message.into()),
    }
}

/// The hole probe of a prepared object (TD-089): over the placed shell when hollowed, over the
/// placed source otherwise.
fn hole_probe(
    args: &SliceRequest,
    prepared: &Prepared,
    mesh: &TriangleMesh,
    placement: &ObjectTransform,
    ctx: &mut Context<'_>,
) -> Result<HoleProbe, String> {
    let mut predicate = ctx.stop.predicate();
    let mut cancel = Cancellation::new(&mut predicate);
    let probe = match prepared.hollow() {
        Some(model) => HoleProbe::hollow(model, probe_limits(args), &mut cancel),
        None => HoleProbe::solid(
            &placed_mesh(mesh, placement)?,
            probe_limits(args),
            &mut cancel,
        ),
    };
    probe.map_err(|e| probe_failure(args, prepared, &e))
}

/// Whether the object's hole probe would be admitted, from its size alone (see
/// [`HoleProbe::admit_hollow`]), with [`hole_probe`]'s message when it would not.
fn admit_probe(
    args: &SliceRequest,
    prepared: &Prepared,
    mesh: &TriangleMesh,
) -> Result<(), String> {
    match prepared.hollow() {
        Some(model) => HoleProbe::admit_hollow(model, probe_limits(args)),
        None => HoleProbe::admit_solid(mesh, probe_limits(args)),
    }
    .map_err(|e| probe_failure(args, prepared, &e))
}

/// Why an object cannot be indexed for drain holes.
fn probe_failure(args: &SliceRequest, prepared: &Prepared, e: &DrainageError) -> String {
    // A cavity too detailed to index: say how detailed, and what makes it coarser.
    let hint = match (e, prepared.hollow()) {
        (DrainageError::Resource { .. }, Some(model)) => format!(
            "; the cavity has {} triangles: a thicker wall or a coarser hollowing cell makes \
             fewer",
            model.cavity().triangles().len()
        ),
        _ => String::new(),
    };
    format!(
        "cannot index {} for drain holes: {e}{hint}",
        model_label(args)
    )
}

/// Resolve, check and cut an object's drain holes (TD-089). Nothing about a hole's path is
/// refused: what each does is kept as findings. A hole too narrow to open in the print, or one
/// that cannot be resolved, is refused.
#[allow(clippy::too_many_arguments)] // One object's holes and everything they are cut into.
fn drill(
    args: &SliceRequest,
    probe: &HoleProbe,
    prepared: &Prepared,
    placement: &ObjectTransform,
    printer: &PrinterProfile,
    layer_mm: f64,
    ctx: &mut Context<'_>,
) -> Result<PreparedHoles, String> {
    ctx.phase("Cutting drain holes");
    let mut predicate = ctx.stop.predicate();
    let mut cancel = Cancellation::new(&mut predicate);
    let pixel = largest_pitch(printer)?;
    let mut specs = Vec::with_capacity(args.drain_holes.len());
    for (index, hole) in (1..).zip(&args.drain_holes) {
        let hole = match args.hole_coordinates {
            Coordinates::World => *hole,
            Coordinates::Model => {
                let linear = placement
                    .to_affine()
                    .map_err(|_| format!("drain hole {index}: its placement is not finite"))?
                    .linear();
                world_hole(hole, placement)
                    .ok_or_else(|| format!("drain hole {index}: its placement is not finite"))?
                    .with_world_tilt(linear, probe, &mut cancel)
                    .map_err(|e| format!("drain hole {index}: {}", drain_message(&e)))?
            }
        };
        let (spec, _) = hole
            .resolve(probe, &mut cancel)
            .map_err(|e| format!("drain hole {index}: {}", drain_message(&e)))?;
        let direction = spec
            .unit_direction()
            .map_err(|_| format!("drain hole {index}: invalid direction"))?;
        too_small(
            index,
            spec.diameter_mm,
            minimum_drain_diameter(direction, pixel, layer_mm),
        )?;
        specs.push(spec);
    }
    let findings = specs
        .iter()
        .map(|spec| probe.findings(spec, &mut cancel))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("cannot check the drain holes of {}: {e}", model_label(args)))?;
    let regions = probe.drained_regions(&findings);
    // Cuts are sampled within a 32nd of the hollowing cell (TD-066), the cell the shell used.
    let error = match prepared.hollow() {
        Some(model) => DRAIN_DISTANCE_ERROR_MM.min(model.certificate().cell_mm / 32.0),
        None => DRAIN_DISTANCE_ERROR_MM,
    };
    let cuts = HoleCuts::build(
        &specs,
        error,
        MAX_HOLES,
        SampledDrainageLimits::default().max_segments,
        &mut cancel,
    )
    .map_err(|e| {
        // Cut errors name holes by number (1-based), not by plan operation.
        let detail = match e {
            DrainageError::Cut {
                operation_id,
                reason,
            } => format!("drain hole {operation_id}: cannot build its cylinder ({reason})"),
            other => other.to_string(),
        };
        format!(
            "cannot cut the drain holes of {}: {detail}",
            model_label(args)
        )
    })?;
    Ok(PreparedHoles {
        cuts,
        specs,
        findings,
        regions,
    })
}

/// The hollowing grid cell of a request, in mm: the explicit cell, or the wall guarantee's first
/// cell for a source of `area_mm2` (TD-093).
fn hollow_cell(args: &SliceRequest, thickness: f64, area_mm2: f64) -> f64 {
    args.hollow_cell_mm
        .unwrap_or_else(|| automatic_cell(thickness, area_mm2))
}

/// The wall guarantee's first cell for a wall of `thickness` mm on a source of `area_mm2`.
fn automatic_cell(thickness: f64, area_mm2: f64) -> f64 {
    guaranteed_cell(
        area_mm2,
        thickness,
        HOLLOW_QUERY_ERROR_MM,
        DEFAULT_OVER_THICKNESS_MM,
        DEFAULT_CAVITY_TRIANGLE_BUDGET,
    )
}

/// Hollow preparation plan bound to the source digest and the placement (TD-089: the identity,
/// so the source is hollowed in its model frame), sampled with the hollowing query error, with
/// the wall guaranteed (TD-093).
fn preparation_plan(
    args: &SliceRequest,
    digest: [u8; 32],
    placement: ObjectTransform,
    [thickness, cell]: [f64; 2],
    simplify_mm: Option<f64>,
    stop: Stop<'_>,
) -> Result<PreparationPlan, String> {
    let mut operations = Vec::new();
    if !args.repair.is_empty() {
        operations.push(Operation {
            id: REPAIR_OPERATION_ID,
            kind: OperationKind::Repair(RepairSpec {
                allow_unfilled: false,
                ..args.repair
            }),
        });
    }
    operations.push(Operation {
        id: 1,
        kind: OperationKind::Hollow(HollowSpec {
            // In the model frame, as the hollowing (TD-089).
            blockers: args.hollow_blockers.iter().map(Blocker::to_core).collect(),
            thickness_mm: thickness,
            guarantee: Some(WallGuarantee {
                shortfall_coefficient: DEFAULT_SHORTFALL_COEFFICIENT,
            }),
        }),
    });
    if let Some(tolerance_mm) = simplify_mm {
        operations.push(Operation {
            id: SIMPLIFY_OPERATION_ID,
            kind: OperationKind::Simplify(SimplifySpec { tolerance_mm }),
        });
    }
    PreparationPlan::new(
        SourceBinding {
            original_sha256: digest,
            placement,
        },
        SamplingSpec {
            cell_mm: cell,
            distance_error_mm: HOLLOW_QUERY_ERROR_MM,
            max_cells: u64::MAX,
        },
        operations,
        PlanLimits::default(),
        &mut Cancellation::new(&mut stop.predicate()),
    )
    .map_err(|e| format!("invalid hollowing request: {e}"))
}

/// Drainage failure text naming holes by command-line position (operation IDs start at 2).
fn drain_message(error: &karasu_core::preparation::drainage::DrainageError) -> String {
    use karasu_core::preparation::drainage::DrainageError;
    match error {
        DrainageError::Opening {
            operation_id,
            issue,
        } => format!("drain hole {}: {}", operation_id - 1, describe(*issue)),
        DrainageError::PotentialOverlap { first, second } => format!(
            "drain holes {} and {} are not certified separated",
            first - 1,
            second - 1
        ),
        DrainageError::Cut {
            operation_id,
            reason,
        } => format!(
            "drain hole {}: cannot build its cylinder ({reason})",
            operation_id - 1
        ),
        other => other.to_string(),
    }
}

/// Smallest diameter that opens in the sampled print along `direction`, and why.
///
/// Every layer plane cuts the cylinder in an ellipse at least the diameter wide, so two pixel
/// pitches always leave a pixel centre inside on every layer it crosses. Across the layers the
/// opening spans `diameter / sin(tilt)` of height, where the tilt is measured from vertical:
/// a vertical hole needs nothing more, a horizontal one two layer heights, and a tilted one two
/// layer heights times the sine of its tilt.
fn minimum_drain_diameter(direction: [f64; 3], pixel: f64, layer_mm: f64) -> (f64, &'static str) {
    let norm = direction.iter().map(|c| c * c).sum::<f64>().sqrt();
    let across = direction[0].hypot(direction[1]) / norm;
    if across == 0.0 {
        (2.0 * pixel, "two pixel pitches")
    } else if direction[2] == 0.0 {
        (
            2.0 * pixel.max(layer_mm),
            "two pixel pitches and two layer heights",
        )
    } else {
        (
            2.0 * pixel.max(layer_mm * across),
            "two pixel pitches, and two layer heights times the sine of the tilt",
        )
    }
}

fn largest_pitch(printer: &PrinterProfile) -> Result<f64, String> {
    let pitch = printer
        .lcd_profile()
        .map_err(|e| format!("invalid printer LCD: {e}"))?
        .pitch_mm();
    Ok(pitch[0].max(pitch[1]))
}

fn too_small(index: usize, diameter: f64, (minimum, why): (f64, &str)) -> Result<(), String> {
    if diameter < minimum {
        return Err(format!(
            "drain hole {index}: diameter {diameter} mm is below {minimum} mm ({why}) and would \
             not open in the print"
        ));
    }
    Ok(())
}

/// Refuse holes too small to open in the sampled print (see [`minimum_drain_diameter`]).
/// Surface holes are checked here for the pixel pitch only, and along their axis once resolved.
fn check_drain_resolution(
    holes: &[DrainHole],
    printer: &PrinterProfile,
    layer_mm: f64,
) -> Result<(), String> {
    if holes.is_empty() {
        return Ok(());
    }
    let pixel = largest_pitch(printer)?;
    for (index, hole) in (1..).zip(holes) {
        let rule = match hole {
            DrainHole::Axis { direction, .. } => {
                minimum_drain_diameter(*direction, pixel, layer_mm)
            }
            DrainHole::Surface { .. } => (2.0 * pixel, "two pixel pitches"),
        };
        too_small(index, hole.diameter_mm(), rule)?;
    }
    Ok(())
}

/// A model-frame hole at the object's `placement` (TD-089): its anchor or entry is placed, an
/// explicit direction turned with the model; diameters, depths and lengths stay in millimetres.
/// A tilt needs the surface normal, so the caller carries it with
/// [`DrainHole::with_world_tilt`].
fn world_hole(hole: &DrainHole, placement: &ObjectTransform) -> Option<DrainHole> {
    let point = |p: [f64; 3]| {
        placement
            .transform_point(Point3::new(p[0], p[1], p[2]))
            .ok()
            .map(Point3::coordinates)
    };
    Some(match *hole {
        DrainHole::Axis {
            entry_mm,
            direction,
            length_mm,
            diameter_mm,
        } => DrainHole::Axis {
            entry_mm: point(entry_mm)?,
            direction: placement
                .transform_vector(karasu_core::geometry::Vector3::new(
                    direction[0],
                    direction[1],
                    direction[2],
                ))
                .ok()?
                .components(),
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
            surface_mm: point(surface_mm)?,
            diameter_mm,
            outside_mm,
            inside_mm,
            tilt,
            depth_mm,
            surface,
        },
    })
}

/// The drain cylinders as cut (resolved surface holes included), for supports and footprints.
fn drain_cylinders(holes: Option<&PreparedHoles>) -> Vec<DrainCylinder> {
    holes.map_or_else(Vec::new, |holes| {
        holes
            .specs
            .iter()
            .filter_map(|spec| {
                let axis = spec.unit_direction().ok()?;
                Some(DrainCylinder {
                    start_mm: spec.entry_mm,
                    end_mm: std::array::from_fn(|k| spec.entry_mm[k] + axis[k] * spec.length_mm),
                    radius_mm: spec.diameter_mm / 2.,
                })
            })
            .collect()
    })
}

/// User-facing explanation of an uncertified opening.
fn describe(issue: karasu_core::preparation::drainage::OpeningIssue) -> &'static str {
    use karasu_core::preparation::drainage::OpeningIssue;
    match issue {
        OpeningIssue::EntryNotExterior => {
            "the entry cap is not entirely outside the model (move the entry further out)"
        }
        OpeningIssue::MissedModel => "the cut ends outside the model",
        OpeningIssue::Overshoot => {
            "the cut passes through the cavity and ends in or on the far wall (shorten it)"
        }
        OpeningIssue::Shallow => "the cut ends before reaching the cavity (lengthen it)",
        OpeningIssue::InsufficientClearance => {
            "the end cap is not entirely inside the cavity (move or narrow the hole)"
        }
        OpeningIssue::SourceNotEmbedded => {
            "no straight path along an axis or a diagonal leads from the entry out of the \
             model's bounding box, and the model's bodies touch, overlap or self-intersect, so \
             the entry cannot be certified (move the entry where such a path leads out)"
        }
    }
}

/// The export failure, naming the remedies when negative winding is the cause: an inside-out
/// body is the usual one, and a self-intersecting fold's reversed sliver the other (TD-079).
fn export_failure(args: &SliceRequest, e: &ExportError) -> Failure {
    let reason = format!("cannot export {}: {e}", QuotedPath(&args.output));
    // TD-092 r8: a layer that does not close (a fin, or a hole wider than layer closure) names
    // the repair that resolves it.
    if caused_by_open_section(e) {
        return Failure::with_remedies(
            reason,
            "; repair the model's holes and non-manifold edges, for example with --repair \
             standard",
            vec![Remedy::RepairDefects],
        );
    }
    if !caused_by_negative_winding(e) {
        return reason.into();
    }
    let outward = !args.repair.outward;
    let tolerate = args.reversed_tolerance_mm2.is_none();
    let hint = match (outward, tolerate) {
        (true, true) => {
            "; use --repair outward to turn inside-out closed bodies outward, or \
             --reversed-tolerance <mm2> to leave small reversed slivers empty"
        }
        (true, false) => "; use --repair outward to turn inside-out closed bodies outward",
        (false, true) => "; use --reversed-tolerance <mm2> to leave small reversed slivers empty",
        (false, false) => "",
    };
    let remedies = [
        (outward, Remedy::RepairOutward),
        (tolerate, Remedy::TolerateReversed),
    ]
    .into_iter()
    .filter_map(|(offered, remedy)| offered.then_some(remedy))
    .collect();
    Failure::with_remedies(reason, hint, remedies)
}

/// Area of one raster sample, in mm²: a pixel's area over the samples it holds.
fn sample_area_mm2(printer: &PrinterProfile, antialias: u8) -> f64 {
    let [width, depth, _] = printer.build_size_mm;
    let [columns, rows] = printer.resolution_px;
    let per_axis = f64::from(antialias);
    width / f64::from(columns) * (depth / f64::from(rows)) / (per_axis * per_axis)
}

/// Whole samples within `area_mm2`, rounded down so the tolerance is never exceeded.
fn samples_within(area_mm2: f64, sample_mm2: f64) -> u64 {
    // Both are finite and positive (validated profile and request), and the quotient is bounded
    // by the tolerance limit over the smallest sample.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let samples = (area_mm2 / sample_mm2).floor() as u64;
    samples
}

/// What manual supports need from the run.
struct SupportInputs<'a> {
    document: &'a SupportDocument,
    args: &'a SliceRequest,
    mesh: &'a TriangleMesh,
    placement: &'a ObjectTransform,
    prepared: &'a Prepared,
    holes: Option<&'a PreparedHoles>,
    digest: Option<[u8; 32]>,
    printer: &'a PrinterProfile,
}

/// The model surface with its built supports, ready to slice.
struct Supported {
    /// The model surface and every support part, in world millimetres.
    mesh: TriangleMesh,
    /// The model surface's faces, which come first in `mesh`; the supports' follow.
    model_faces: usize,
    /// The raft's faces in `mesh`, among the supports' (an exported model's raft is a body of
    /// its own, TD-139).
    raft_faces: Vec<Range<usize>>,
    /// The highest support point.
    top_z: f64,
    /// Summary lines.
    lines: Vec<String>,
    /// The built scene, kept only when preparing for display.
    scene: Option<SupportScene>,
}

/// The placed model as a world mesh. A reflecting placement reverses every face, keeping the
/// surface outward exactly as the winding layers treat a reflected transform. The interface uses
/// it to plan adaptive layers as slicing does (TD-102).
///
/// # Errors
/// A placed vertex that is not finite, and a mesh the placed vertices cannot form.
pub fn placed_mesh(
    mesh: &TriangleMesh,
    placement: &ObjectTransform,
) -> Result<TriangleMesh, String> {
    let vertices = mesh
        .vertices()
        .iter()
        .map(|&p| placement.transform_point(p))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("cannot place model for supports: {e}"))?;
    let reflects = placement
        .scale()
        .components()
        .iter()
        .filter(|&&s| s < 0.)
        .count()
        % 2
        == 1;
    let triangles = mesh
        .triangles()
        .iter()
        .map(|&[a, b, c]| if reflects { [a, c, b] } else { [a, b, c] })
        .collect();
    TriangleMesh::try_new(vertices, triangles)
        .map_err(|e| format!("cannot place model for supports: {e}"))
}

/// Read and validate a support document.
fn read_supports(input: &Input<SupportDocument>) -> Result<SupportDocument, String> {
    let path = match input {
        Input::File(path) => path,
        Input::Loaded { value, .. } => return Ok(value.clone()),
    };
    let quoted = QuotedPath(path);
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|file| {
            file.take(u64::try_from(MAX_DOCUMENT_BYTES).unwrap_or(u64::MAX) + 1)
                .read_to_end(&mut bytes)
        })
        .map_err(|e| format!("cannot read support document {quoted}: {e}"))?;
    SupportDocument::from_json(&bytes).map_err(|e| format!("{quoted}: {e}"))
}

/// The document's recorded model and translation must match this run exactly.
///
/// A binding with a recorded pose (TD-072) admits any placement reached by moving the model on the
/// plate and turning it about the vertical axis, and returns the motion that carries the supports
/// along; `None` when the supports stay where they are.
fn check_binding(input: &SupportInputs<'_>, quoted: &str) -> Result<Option<PlanarMotion>, String> {
    use std::fmt::Write as _;
    let Some(binding) = &input.document.binding else {
        return Ok(None);
    };
    if let (Some(expected), Some(digest)) = (&binding.model_sha256, input.digest) {
        let actual = digest.iter().fold(String::new(), |mut text, byte| {
            let _ = write!(text, "{byte:02x}");
            text
        });
        if *expected != actual {
            return Err(format!(
                "{quoted} was written for model SHA-256 {expected}, but this model is {actual}"
            ));
        }
    }
    match (binding.translation_mm, binding.pose) {
        (Some(translation), Some(bound)) => follow(input, quoted, translation, &bound),
        (Some(expected), None) => {
            let actual = input.placement.translation().components();
            // An exact match: the document was written for exactly this placement.
            #[allow(clippy::float_cmp)]
            let same = expected == actual;
            if !same {
                return Err(format!(
                    "{quoted} was written for translation {expected:?} mm, but the placement is \
                     {actual:?} mm"
                ));
            }
            Ok(None)
        }
        (None, _) => Ok(None),
    }
}

/// The motion from the placement a document's supports were placed for to the current one, or
/// why the supports cannot follow (TD-072).
///
/// Supports stand on the plate and their shafts are vertical, so they follow the model exactly
/// only through moves on the plate and turns about the vertical axis. Any other change (the
/// rotation about X or Y, the scale, the mirroring) is refused. So is a height change beyond the
/// document's anchor tolerance; within it, anchoring still checks every contact on the surface.
fn follow(
    input: &SupportInputs<'_>,
    quoted: &str,
    translation: [f64; 3],
    bound: &BoundPose,
) -> Result<Option<PlanarMotion>, String> {
    let pose = &input.args.pose;
    #[allow(clippy::float_cmp)] // Recorded options compare exactly.
    let same_shape = bound.rotate_deg[..2] == pose.rotate_deg[..2]
        && bound.scale == pose.scale
        && bound.mirror == pose.mirror;
    if !same_shape {
        return Err(format!(
            "{quoted} was placed for the model rotated {:?} degrees, scaled {:?} and mirrored \
             {:?}, but it is rotated {:?} degrees, scaled {:?} and mirrored {:?}; supports follow \
             moves on the plate and turns about the vertical axis only",
            bound.rotate_deg, bound.scale, bound.mirror, pose.rotate_deg, pose.scale, pose.mirror
        ));
    }
    let recorded =
        Shape::new(input.mesh, bound.rotate_deg, bound.scale, bound.mirror)?.at(translation)?;
    let reference = centre(input.mesh).ok_or("model contains no vertices")?;
    let place = |placement: &ObjectTransform| {
        placement
            .transform_point(reference)
            .map_err(|e| format!("invalid placement: {e}"))
    };
    let (before, after) = (place(&recorded)?, place(input.placement)?);
    let rise = after.z - before.z;
    if rise.abs() > input.document.anchor_tolerance_mm {
        return Err(format!(
            "{quoted} was placed with the model {} mm {} than it is now, beyond the anchor \
             tolerance of {} mm; supports follow moves on the plate and turns about the vertical \
             axis only",
            rise.abs(),
            if rise > 0.0 { "lower" } else { "higher" },
            input.document.anchor_tolerance_mm
        ));
    }
    let motion = PlanarMotion {
        turn_deg: pose.rotate_deg[2] - bound.rotate_deg[2],
        from_mm: [before.x, before.y],
        to_mm: [after.x, after.y],
    };
    Ok((!motion.is_identity()).then_some(motion))
}

/// Why a support document in a model frame cannot carry a binding: it records no placement.
const MODEL_FRAME_BINDING: &str =
    "supports in the model's frame follow it everywhere and take no binding to a placement";

/// Build an object's supports, written in its model frame (TD-108), against its prepared
/// surface: the hollowed shell when it is hollowed, otherwise `mesh` carried by `placement`.
/// Drain holes as cut and the printer's build volume are checked as when slicing. The interface
/// builds an object's supports with it while they are edited, drafts included; nothing is
/// refused, so every finding can be shown.
///
/// # Errors
/// A document or placement that cannot be built (see [`SupportDocument::placed`]), and
/// cancellation.
pub fn build_model_supports(
    document: &SupportDocument,
    mesh: &TriangleMesh,
    placement: &ObjectTransform,
    preparation: &ObjectPreparation,
    printer: &PrinterProfile,
    stop: &(dyn Fn(Checkpoint) -> bool + Sync),
) -> Result<SupportScene, String> {
    if document.binding.is_some() {
        return Err(MODEL_FRAME_BINDING.to_owned());
    }
    let affine = placement
        .to_affine()
        .map_err(|e| format!("cannot place the supports: {e}"))?;
    // Built in code by the interface: checked as a document read would be.
    let doc = document
        .clone()
        .validated()
        .and_then(|d| d.placed(&affine))
        .map_err(|e| e.to_string())?;
    let world;
    let (surface, interior_from) = if let Some(model) = preparation.prepared.hollow() {
        (model.mesh(), Some(model.exterior_faces()))
    } else {
        world = placed_mesh(mesh, placement)?;
        (&world, None)
    };
    let mut predicate = |checkpoint| stop(checkpoint);
    build_supports(
        &doc,
        SupportSurface {
            mesh: surface,
            interior_from,
        },
        &drain_cylinders(preparation.holes.as_ref()),
        BuildVolume {
            origin_xy_mm: printer.origin_xy_mm,
            size_mm: printer.build_size_mm,
        },
        SupportOptions::default(),
        &mut Cancellation::new(&mut predicate),
    )
    .map_err(|e| format!("cannot build the supports: {e}"))
}

/// Plan braces between an object's neighbouring supports, its document in its model frame
/// (TD-108), against its prepared surface and drain holes as [`build_model_supports`] builds
/// them: see [`plan_braces`]. Brace heights and ends are world values, as braces keep them.
///
/// # Errors
/// A document, placement or plan that cannot be used, and cancellation.
pub fn plan_model_braces(
    document: &SupportDocument,
    mesh: &TriangleMesh,
    placement: &ObjectTransform,
    preparation: &ObjectPreparation,
    plan: &BracePlan,
    stop: &(dyn Fn(Checkpoint) -> bool + Sync),
) -> Result<Vec<PlannedBrace>, String> {
    if document.binding.is_some() {
        return Err(MODEL_FRAME_BINDING.to_owned());
    }
    let affine = placement
        .to_affine()
        .map_err(|e| format!("cannot place the supports: {e}"))?;
    let doc = document
        .clone()
        .validated()
        .and_then(|d| d.placed(&affine))
        .map_err(|e| e.to_string())?;
    let world;
    let (surface, interior_from) = if let Some(model) = preparation.prepared.hollow() {
        (model.mesh(), Some(model.exterior_faces()))
    } else {
        world = placed_mesh(mesh, placement)?;
        (&world, None)
    };
    let mut predicate = |checkpoint| stop(checkpoint);
    plan_braces(
        &doc,
        SupportSurface {
            mesh: surface,
            interior_from,
        },
        &drain_cylinders(preparation.holes.as_ref()),
        plan,
        &mut Cancellation::new(&mut predicate),
    )
    .map_err(|e| format!("cannot plan the braces: {e}"))
}

/// The document in world coordinates for this placement, with the motion that carried a bound
/// one (TD-072); `None` when it is used as it is.
fn world_supports(
    input: &SupportInputs<'_>,
    quoted: &str,
) -> Result<(Option<PlanarMotion>, Option<SupportDocument>), String> {
    match input.args.support_coordinates {
        Coordinates::World => {
            let motion = check_binding(input, quoted)?;
            Ok((motion, motion.map(|m| input.document.moved(&m))))
        }
        Coordinates::Model => {
            if input.document.binding.is_some() {
                return Err(format!("{quoted}: {MODEL_FRAME_BINDING}"));
            }
            let affine = input
                .placement
                .to_affine()
                .map_err(|e| format!("cannot place the supports of {quoted}: {e}"))?;
            // A project's document is built in code: checked as a document read would be.
            let placed = input
                .document
                .clone()
                .validated()
                .and_then(|d| d.placed(&affine))
                .map_err(|e| format!("cannot place the supports of {quoted}: {e}"))?;
            Ok((None, Some(placed)))
        }
    }
}

/// Build the supports and join them with the model surface.
fn apply_supports(
    input: &SupportInputs<'_>,
    quoted: &str,
    ctx: &mut Context<'_>,
) -> Result<Supported, String> {
    ctx.phase("Building supports");
    let stop = ctx.stop;
    let args = input.args;
    let (motion, placed) = world_supports(input, quoted)?;
    let doc = placed.as_ref().unwrap_or(input.document);
    let world;
    let (surface, interior_from) = if let Some(model) = input.prepared.hollow() {
        (model.mesh(), Some(model.exterior_faces()))
    } else {
        world = placed_mesh(input.mesh, input.placement)?;
        (&world, None)
    };
    let drains = drain_cylinders(input.holes);
    let volume = BuildVolume {
        origin_xy_mm: input.printer.origin_xy_mm,
        size_mm: input.printer.build_size_mm,
    };
    let scene = build_supports(
        doc,
        SupportSurface {
            mesh: surface,
            interior_from,
        },
        &drains,
        volume,
        SupportOptions {
            accept_all_collisions: args.accept_support_collisions,
        },
        &mut Cancellation::new(&mut stop.predicate()),
    )
    .map_err(|e| format!("cannot build supports from {quoted}: {e}"))?;
    check_support_errors(&scene, args.slice_support_errors || ctx.preview, quoted)?;
    let lowest = surface
        .vertices()
        .iter()
        .map(|p| p.z)
        .fold(f64::INFINITY, f64::min);
    let reaches_plate = scene.raft()
        || scene
            .supports()
            .iter()
            .any(|s| s.parts > 0 && s.base == BaseKind::Platform);
    if lowest > 0. && !reaches_plate && !ctx.preview {
        // The supports left out say why (M6.4h): the refusal alone gives no way forward.
        let errors: Vec<String> = scene
            .findings()
            .iter()
            .filter(|f| f.severity == Severity::Error)
            .take(MAX_FINDINGS_SHOWN)
            .map(ToString::to_string)
            .collect();
        let why = if errors.is_empty() {
            String::new()
        } else {
            format!(
                "; supports that could not be built:\n  {}",
                errors.join("\n  ")
            )
        };
        return Err(format!(
            "the model is {lowest} mm above the plate, but no support in {quoted} stands on the plate{why}"
        ));
    }
    let lines = support_lines(&scene, motion);
    let mut vertices = surface.vertices().to_vec();
    let offset = u32::try_from(vertices.len()).map_err(|_| "too many vertices with supports")?;
    vertices.extend_from_slice(scene.mesh().vertices());
    let mut triangles = surface.triangles().to_vec();
    triangles.extend(
        scene
            .mesh()
            .triangles()
            .iter()
            .map(|t| t.map(|v| v + offset)),
    );
    let top_z = scene
        .mesh()
        .vertices()
        .iter()
        .map(|p| p.z)
        .fold(f64::NEG_INFINITY, f64::max);
    let model_faces = surface.triangles().len();
    let raft_faces = scene
        .part_ranges()
        .iter()
        .filter(|p| p.part == karasu_core::supports::SupportPart::Raft)
        .map(|p| model_faces + p.triangles.start..model_faces + p.triangles.end)
        .collect();
    Ok(Supported {
        mesh: TriangleMesh::try_new(vertices, triangles)
            .map_err(|e| format!("cannot join supports with the model: {e}"))?,
        model_faces,
        raft_faces,
        top_z,
        lines,
        scene: ctx.preview.then_some(scene),
    })
}

/// Refuse supports with error findings, unless the job slices them as they are built
/// (`slice`: a preview, or the interface's policy, TD-108), which a slicing job warns of.
fn check_support_errors(scene: &SupportScene, slice: bool, quoted: &str) -> Result<(), String> {
    let errors: Vec<String> = scene
        .findings()
        .iter()
        .filter(|f| f.severity == Severity::Error)
        .map(ToString::to_string)
        .collect();
    if errors.is_empty() {
        return Ok(());
    }
    if slice {
        log::warn!(
            target: "supports",
            "{quoted}: supports with {} error findings are used as they are built",
            errors.len()
        );
        return Ok(());
    }
    let shown = errors.len().min(MAX_FINDINGS_SHOWN);
    Err(format!(
        "supports in {quoted} cannot be printed ({} errors{}):\n  {}",
        errors.len(),
        if shown < errors.len() {
            format!(", first {shown} shown")
        } else {
            String::new()
        },
        errors[..shown].join("\n  ")
    ))
}

/// Most support findings listed in a failure or the summary.
const MAX_FINDINGS_SHOWN: usize = 20;

/// The summary lines of built supports, and of the motion that carried them, if any.
fn support_lines(scene: &SupportScene, motion: Option<PlanarMotion>) -> Vec<String> {
    let count = |base: BaseKind| scene.supports().iter().filter(|s| s.base == base).count();
    let interior = scene.supports().iter().filter(|s| s.interior_tip).count();
    let minis = scene
        .supports()
        .iter()
        .filter(|s| s.kind == SupportKind::Mini)
        .count();
    let mut lines = vec![format!(
        "Supports: {} (plate {}, model {}, on supports {}; {}interior {}), braces {}, raft {}, parts {}",
        scene.supports().len(),
        count(BaseKind::Platform),
        count(BaseKind::Model),
        count(BaseKind::Support),
        if minis > 0 {
            format!("mini {minis}; ")
        } else {
            String::new()
        },
        interior,
        scene.braces(),
        if scene.raft() { "yes" } else { "no" },
        scene.parts()
    )];
    let warnings = scene
        .findings()
        .iter()
        .filter(|f| f.severity == Severity::Warning)
        .count();
    let accepted = scene
        .findings()
        .iter()
        .filter(|f| f.severity == Severity::Accepted)
        .count();
    let errors = scene
        .findings()
        .iter()
        .filter(|f| f.severity == Severity::Error)
        .count();
    if let Some(m) = motion {
        let ([fx, fy], [tx, ty]) = (m.from_mm, m.to_mm);
        lines.push(format!(
            "Supports follow the model: turned {} degrees, from ({fx}, {fy}) to ({tx}, {ty}) mm",
            m.turn_deg
        ));
    }
    lines.push(format!(
        "Support findings: {warnings} warnings, {accepted} accepted collisions"
    ));
    // Only a job that slices supports with errors gets here with any (TD-108).
    if errors > 0 {
        let left_out = scene.supports().iter().filter(|s| s.parts == 0).count();
        lines.push(format!(
            "Supports sliced with errors: {errors} errors; {left_out} supports could not be built and are left out"
        ));
    }
    lines.extend(
        scene
            .findings()
            .iter()
            .take(MAX_FINDINGS_SHOWN)
            .map(|f| format!("Support finding: {f}")),
    );
    lines
}

/// The exporter with rendered previews of `mesh` placed by `transform`.
fn with_previews(
    machine: Option<BuiltinExporter>,
    mesh: &TriangleMesh,
    transform: SectionTransform<'_>,
    ctx: &mut Context<'_>,
) -> Result<Option<BuiltinExporter>, String> {
    machine
        .map(|exporter| {
            ctx.phase("Rendering previews");
            let previews = render_previews(mesh, transform, exporter.preview_sizes(), ctx.stop)?;
            Ok(exporter.with_previews(previews))
        })
        .transpose()
}

/// Render the `[small, large]` previews of `mesh` placed by `transform`, both at once.
fn render_previews(
    mesh: &TriangleMesh,
    transform: SectionTransform<'_>,
    sizes: [[u32; 2]; 2],
    stop: Stop<'_>,
) -> Result<Previews, String> {
    let placement = match transform {
        SectionTransform::Identity => Affine3::identity(),
        SectionTransform::Affine(affine) => *affine,
        SectionTransform::Object(placement) => placement
            .to_affine()
            .map_err(|e| format!("cannot render previews: {e}"))?,
    };
    let render = |[width, height]: [u32; 2]| {
        let mut predicate = stop.predicate();
        render_preview(
            mesh,
            placement,
            width,
            height,
            &mut Cancellation::new(&mut predicate),
        )
        .map_err(|e| format!("cannot render {width}x{height} preview: {e}"))
    };
    let (small, large) = std::thread::scope(|scope| {
        let small = scope.spawn(|| render(sizes[0]));
        let large = render(sizes[1]);
        (small.join(), large)
    });
    let small = small.map_err(|_| "preview rendering failed unexpectedly")??;
    Ok(Previews {
        small,
        large: large?,
    })
}

/// Compose the placement (rotation, scale and mirroring about the source bounding-box centre,
/// then the translation) and return it with the placed model's top world Z.
///
/// `--center` and `--drop` compute the translation from the bounds placed without one. Dropping
/// is exact: the translation is the negated lowest placed Z, and adding a value's negation gives
/// exactly zero, so the model rests on Z = 0 with every vertex at or above it.
/// What a placement must satisfy.
#[derive(Clone, Copy)]
#[allow(clippy::struct_excessive_bools)] // Independent checks, each switched by the caller.
struct Checks {
    /// Supports hold the model, so it may float above the plate.
    supported: bool,
    /// The model must lie within the LCD area (false for models the arranger moves later, and
    /// in previews).
    plate_xy: bool,
    /// The model must fit the printer's build height (false in previews).
    height: bool,
    /// The model must rest on the plate unless supported (false in previews: a lifted object is
    /// prepared where it is, and slicing refuses it).
    resting: bool,
}

fn place(
    mesh: &TriangleMesh,
    translate: [f64; 3],
    pose: &Pose,
    printer: &PrinterProfile,
    checks: Checks,
    cancel: &mut Cancellation<'_>,
) -> Result<(ObjectTransform, f64), Failure> {
    let centre = placement::plate_centre(printer);
    let placed = placement::place(mesh, translate, pose, centre, cancel)?;
    let [x, y, _] = placed.translation_mm;
    check_placement(
        placed.min,
        placed.max,
        [x, y, placed.resting_dz],
        printer,
        checks,
    )?;
    Ok((placed.transform, placed.max.z))
}

/// Strict even-odd interpretation of represented binary64 coordinates, with no snapping.
fn layer_config() -> Result<LayerConfig, String> {
    Ok(LayerConfig {
        // Inactive under represented coordinates; validated metadata only.
        grid: BinaryGridSpec::try_new([0., 0.], -20).map_err(|e| e.to_string())?,
        fill_rule: RegionFillRule::EvenOddStrict,
        stages: LayerStageLimits {
            section: SectionLimits {
                max_faces: MAX_SECTION_FACES,
                ..SectionLimits::default()
            },
            validity: ValidityLimits {
                max_pairs: MAX_VALIDITY_PAIRS,
                ..ValidityLimits::default()
            },
            ..LayerStageLimits::default()
        },
        max_output_payload_bytes: MAX_LAYER_PAYLOAD_BYTES,
    })
}

fn check_topology(
    mesh: &TriangleMesh,
    defects: &DefectReport,
    args: &SliceRequest,
    cancel: &mut Cancellation<'_>,
) -> Result<(), Failure> {
    if args.fill == Fill::Union {
        // Touching bodies make non-manifold edges and vertices, which the winding number fills
        // correctly. Holes are closed per layer when section closure is enabled; orientation
        // conflicts would leave layers that cannot close.
        let holes = defects.boundary_edges > 0 && args.close_holes_mm.is_none();
        if defects.usable_triangles > 0 && defects.winding_conflicts == 0 && !holes {
            return Ok(());
        }
        let (first, mut remedies) = match (holes, defects.winding_conflicts > 0) {
            (true, _) => (
                "enable --close-holes <mm> to close holes in each layer, ",
                vec![Remedy::CloseHoles],
            ),
            (false, true) if !args.repair.orientation => (
                "enable --repair orientation to make orientation consistent, ",
                vec![Remedy::RepairOrientation],
            ),
            _ => ("", Vec::new()),
        };
        remedies.extend([Remedy::StrictFill, Remedy::AllowTopologyDefects]);
        return Err(Failure::with_remedies(
            format!(
                "model cannot be sliced as a union (mesh defects: {})",
                describe_defects(defects)
            ),
            &format!(
                "; {first}run 'inspect' for details, use --fill strict, or pass \
                 --allow-topology-defects to slice anyway with every layer still required to \
                 close exactly"
            ),
            remedies,
        ));
    }
    let topology = mesh
        .analyze_topology_with_cancellation(TopologyLimits::default(), cancel)
        .map_err(|e| format!("cannot analyze model topology: {e}"))?;
    let s = topology.summary();
    // Even-odd filling does not depend on winding, so winding conflicts alone are not refused.
    if s.closed_combinatorial_surface {
        return Ok(());
    }
    Err(Failure::with_remedies(
        format!(
            "model is not a closed manifold surface (boundary edges: {}, non-manifold edges: {}, \
             non-manifold vertices: {}, excluded degenerate triangles: {}, isolated vertices: {})",
            s.boundary_edges,
            s.non_manifold_edges,
            s.non_manifold_vertices,
            s.excluded_triangles,
            s.isolated_vertices
        ),
        "; run 'inspect' for details, or pass --allow-topology-defects to slice anyway with \
         every layer still required to resolve completely",
        vec![Remedy::AllowTopologyDefects],
    ))
}

/// Whether a layer failed because its section did not close: an unbalanced crossing (an open
/// boundary or a fin) or a gap wider than layer closure.
fn caused_by_open_section(error: &(dyn std::error::Error + 'static)) -> bool {
    let mut cause = Some(error);
    while let Some(e) = cause {
        if matches!(
            e.downcast_ref::<karasu_core::slicing::engine::LayerFailure>(),
            Some(
                karasu_core::slicing::engine::LayerFailure::Unbalanced { .. }
                    | karasu_core::slicing::engine::LayerFailure::OpenSectionGap { .. }
            )
        ) {
            return true;
        }
        cause = e.source();
    }
    false
}

/// Whether a raster layer failed for negative winding under the positive rule.
fn caused_by_negative_winding(error: &(dyn std::error::Error + 'static)) -> bool {
    let mut cause = Some(error);
    while let Some(e) = cause {
        if matches!(
            e.downcast_ref::<RasterError>(),
            Some(
                RasterError::NegativeWinding { .. } | RasterError::NegativeWindingAboveBound { .. }
            )
        ) {
            return true;
        }
        cause = e.source();
    }
    false
}

/// The tolerant and non-manifold stages' parts of the repairs line (TD-092): welding and degenerate
/// faces first, as they run, then non-manifold edges; each only when the stage changed something,
/// so earlier summaries are unchanged.
fn describe_stages(report: &SourceRepairReport, parts: &mut Vec<String>) {
    if let Some(weld) = report.weld.as_ref().filter(|w| w.clusters > 0) {
        parts.push(format!(
            "{} vertices welded into {} (largest move {} mm; {} faces collapsed, {} mm^2)",
            weld.vertices_merged,
            weld.clusters,
            weld.max_displacement_mm,
            weld.faces_collapsed,
            weld.collapsed_area_mm2
        ));
    }
    if let Some(d) = &report.degenerate {
        if d.needles_removed > 0 {
            parts.push(format!("{} needle faces removed", d.needles_removed));
        }
        if d.edges_collapsed > 0 {
            parts.push(format!(
                "{} short edges collapsed (largest move {} mm)",
                d.edges_collapsed, d.max_displacement_mm
            ));
        }
        if d.caps_flipped > 0 {
            parts.push(format!(
                "{} flat faces flipped (largest apex distance {} mm)",
                d.caps_flipped, d.max_cap_height_mm
            ));
        }
        if d.left > 0 {
            parts.push(format!("{} near-degenerate faces left", d.left));
        }
    }
    if let Some(n) = &report.non_manifold {
        if n.fins_removed > 0 {
            parts.push(format!(
                "{} fins removed ({} faces, {} mm^2)",
                n.fins_removed, n.fin_faces, n.fin_area_mm2
            ));
        }
        if n.faces_cut > 0 {
            parts.push(format!(
                "{} faces cut from non-manifold edges ({} mm^2)",
                n.faces_cut, n.cut_area_mm2
            ));
        }
        if n.edges_left > 0 {
            parts.push(format!("{} non-manifold edges left", n.edges_left));
        }
    }
    if let Some(v) = report.visibility.as_ref().filter(|v| v.parts_flipped > 0) {
        parts.push(format!(
            "{} open parts turned by visibility ({} faces)",
            v.parts_flipped, v.faces_flipped
        ));
    }
    if let Some(s) = &report.shells {
        if s.small_removed > 0 {
            parts.push(format!(
                "{} small shells removed ({} faces, {} mm^2)",
                s.small_removed, s.small_faces, s.small_area_mm2
            ));
        }
        if s.inner_removed > 0 {
            parts.push(format!(
                "{} inner shells removed ({} faces, {} mm^3)",
                s.inner_removed, s.inner_faces, s.inner_volume_mm3
            ));
        }
    }
    if let Some(r) = &report.rebuild {
        parts.push(format!(
            "rebuilt as a solid at {} mm ({} -> {} triangles, {} surfaces; rebuilt surface within \
             {} mm of the source, source vertices within {} mm of it; volume {} -> {} mm^3)",
            r.resolution_mm,
            r.triangles[0],
            r.triangles[1],
            r.surfaces,
            r.max_deviation_mm,
            r.source_deviation_mm,
            r.volume_mm3[0],
            r.volume_mm3[1]
        ));
    }
}

/// The repairs applied, or "none".
fn describe_repairs(report: &SourceRepairReport) -> String {
    let r = &report.mesh;
    let mut parts = Vec::new();
    describe_stages(report, &mut parts);
    if let Some(fill) = &report.fill {
        if fill.holes_filled > 0 {
            parts.push(format!(
                "{} holes filled ({} cap triangles, {} mm^2, largest {} edges)",
                fill.holes_filled, fill.cap_triangles, fill.cap_area_mm2, fill.largest_filled_edges
            ));
            // TD-092 r5: only when caps add vertices or split rims, so earlier lines are unchanged.
            if fill.vertices_added > 0 {
                parts.push(format!("{} cap vertices added", fill.vertices_added));
            }
            if fill.rims_split > 0 {
                parts.push(format!("{} rims split at pinch vertices", fill.rims_split));
            }
            if fill.unrefined_caps > 0 {
                parts.push(format!(
                    "{} refined caps met the mesh and were capped unrefined",
                    fill.unrefined_caps
                ));
            }
        }
        if fill.cracks_closed > 0 {
            parts.push(format!(
                "{} cracks closed ({} faces split, {} zero-area faces and {} folded faces \
                 removed)",
                fill.cracks_closed, fill.faces_split, fill.slivers_removed, fill.folds_removed
            ));
            if fill.largest_crack_offset_mm > 0.0 {
                parts.push(format!(
                    "largest crack offset {} mm",
                    fill.largest_crack_offset_mm
                ));
            }
        }
        if let Some(first) = fill.refused.first() {
            parts.push(format!(
                "{} holes left open (first: {} edges at vertex {}, {:?})",
                fill.holes_refused, first.edges, first.first_vertex, first.reason
            ));
        }
    }
    if r.dropped_duplicate_faces > 0 {
        parts.push(format!(
            "{} duplicate faces dropped ({} cancelling groups)",
            r.dropped_duplicate_faces, r.cancelled_duplicate_groups
        ));
    }
    if r.flipped_faces > 0 {
        parts.push(format!(
            "{} faces flipped ({} mm^2; {} components reoriented, {} turned outward)",
            r.flipped_faces,
            r.flipped_area_mm2,
            r.reoriented_components,
            r.outward_flipped_components
        ));
    }
    for (count, what) in [
        (
            r.non_orientable_components,
            "non-orientable components left unchanged",
        ),
        (
            r.nested_reversed_components,
            "nested reversed components left unchanged",
        ),
        (
            r.ambiguous_volume_components,
            "components with an ambiguous volume sign",
        ),
    ] {
        if count > 0 {
            parts.push(format!("{count} {what}"));
        }
    }
    if parts.is_empty() {
        "none".to_owned()
    } else {
        parts.join("; ")
    }
}

/// The defect classes present, or "none".
fn describe_defects(d: &DefectReport) -> String {
    let mut parts = Vec::new();
    if d.boundary_edges > 0 {
        parts.push(format!(
            "{} boundary edges (hole rims: {}, open rim chains: {}, longest rim {} mm)",
            d.boundary_edges, d.rim_loops, d.rim_chains, d.longest_rim_mm
        ));
    }
    for (count, what) in [
        (d.non_manifold_edges, "non-manifold edges"),
        (
            d.unbalanced_non_manifold_edges,
            "unbalanced non-manifold edges",
        ),
        (d.winding_conflicts, "winding conflicts"),
        (
            d.same_orientation_duplicates,
            "same-orientation duplicate faces",
        ),
        (d.opposite_duplicate_pairs, "opposite duplicate face pairs"),
        (d.excluded_triangles, "degenerate faces"),
        (d.non_orientable_components, "non-orientable components"),
    ] {
        if count > 0 {
            parts.push(format!("{count} {what}"));
        }
    }
    if parts.is_empty() {
        "none".to_owned()
    } else {
        parts.join("; ")
    }
}

/// `resting` is the full `--translate` that keeps the given X/Y and rests the model on Z = 0.
/// `supported` admits a model lifted above the plate (its supports must then reach the plate).
fn check_placement(
    min: Point3,
    max: Point3,
    [rx, ry, rz]: [f64; 3],
    printer: &PrinterProfile,
    checks: Checks,
) -> Result<(), Failure> {
    // Exact zero is reachable: translating by the negated source minimum yields +0.
    if checks.resting && (min.z < 0. || (min.z > 0. && !checks.supported)) {
        return Err(Failure::with_remedies(
            format!(
                "model must rest on the build plate: world minimum Z is {} mm, expected 0",
                min.z
            ),
            &format!("; use --drop, or --translate {rx} {ry} {rz}"),
            vec![Remedy::RestOnPlate],
        ));
    }
    let [ox, oy] = printer.origin_xy_mm;
    let [width, depth, height] = printer.build_size_mm;
    if checks.plate_xy && (min.x < ox || min.y < oy || max.x > ox + width || max.y > oy + depth) {
        return Err(Failure::with_remedies(
            format!(
                "model XY extent [{}, {}] x [{}, {}] mm lies outside the LCD area [{ox}, {}] x \
                 [{oy}, {}] mm",
                min.x,
                max.x,
                min.y,
                max.y,
                ox + width,
                oy + depth
            ),
            "; adjust --translate or use --center",
            vec![Remedy::MoveOntoPlate],
        ));
    }
    if checks.height && max.z > height {
        return Err(format!(
            "model height {} mm exceeds the printer Z build size {height} mm",
            max.z
        )
        .into());
    }
    Ok(())
}

fn read_profile(path: &Path, what: &str) -> Result<ProfileDocument, String> {
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|file| {
            // One byte beyond the bound lets the core report oversize input explicitly.
            file.take(u64::try_from(MAX_PROFILE_BYTES).unwrap_or(u64::MAX) + 1)
                .read_to_end(&mut bytes)
        })
        .map_err(|e| format!("cannot read {what} profile {}: {e}", QuotedPath(path)))?;
    ProfileDocument::from_json(&bytes)
        .map_err(|e| format!("invalid {what} profile {}: {e}", QuotedPath(path)))
}

fn read_printer(input: &Input<PrinterProfile>) -> Result<PrinterProfile, String> {
    let path = match input {
        Input::File(path) => path,
        Input::Loaded { value, .. } => return Ok(value.clone()),
    };
    match read_profile(path, "printer")? {
        ProfileDocument::Printer(p) => Ok(p),
        _ => Err(format!(
            "{} is not a printer profile; --printer requires kind \"printer\"",
            QuotedPath(path)
        )),
    }
}

fn read_resin(input: &Input<ResinProfile>) -> Result<ResinProfile, String> {
    let path = match input {
        Input::File(path) => path,
        Input::Loaded { value, .. } => return Ok(value.clone()),
    };
    match read_profile(path, "resin")? {
        ProfileDocument::Resin(r) => Ok(r),
        ProfileDocument::Material(_) => Err(format!(
            "{} is a library resin without exposure settings; give its print setting with \
             --print-setting <file>",
            QuotedPath(path)
        )),
        _ => Err(format!(
            "{} is not a resin profile; --resin requires kind \"resin\"",
            QuotedPath(path)
        )),
    }
}
