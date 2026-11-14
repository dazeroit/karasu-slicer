//! Slicing requests: what one job slices, with which profiles, into which output, and how.
//!
//! A [`SliceRequest`] describes a single model (with its placement, repair, hollowing, drain holes,
//! supports and copies) or a plate document of several objects (TD-065). Paths are only recorded
//! here; they are read when the [pipeline](crate::pipeline) runs. [`SliceRequest::checked`]
//! refuses option combinations the pipeline cannot honour, before any work starts.
//!
//! Messages name the command-line options of `karasu-cli`, which is today the only caller; the
//! desktop application maps them to its controls when it builds requests (M6.3).
use karasu_core::{
    cancellation::Cancellation,
    export::{AnycubicWorkshop, BuiltinExporter, BuiltinFormat, MachineExporter, PixelPolicy},
    layer_plan::LayerPlanDocument,
    mesh::formats::LengthUnit,
    preparation::{
        drainage::DrainageError,
        repair::RepairSpec,
        sampled::{map_tilt, HoleFrame, HoleProbe, HoleSurface},
        HoleSpec, HoleTilt, HollowBlocker, SurfaceHoleSpec,
    },
    profiles::{PrinterProfile, ResinProfile},
    supports::SupportDocument,
};
use std::{
    fmt,
    path::{Path, PathBuf},
    sync::Arc,
};

/// A path as messages show it: quoted and escaped, exactly as typed.
pub(crate) struct QuotedPath<'a>(pub(crate) &'a Path);

impl fmt::Display for QuotedPath<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.0.as_os_str(), f)
    }
}

/// Output selector that writes numbered PNGs plus JSON manifests instead of a machine file.
pub const REFERENCE_FORMAT: &str = "reference";

/// Machine dialects that can be selected besides the Anycubic Photon Workshop targets.
const MACHINE_FORMATS: [BuiltinFormat; 10] = [
    BuiltinFormat::CtbV2Plain,
    BuiltinFormat::CtbV3Plain,
    BuiltinFormat::CtbV4Plain,
    BuiltinFormat::CtbV5Plain,
    BuiltinFormat::GooV3,
    BuiltinFormat::AnycubicPhotonP1,
    BuiltinFormat::AnycubicPhotonP1Max,
    BuiltinFormat::PhotonV1FirmwareMotion,
    BuiltinFormat::PrusaSl1FirmwareMotion,
    BuiltinFormat::PrusaSl1sFirmwareMotion,
];

/// Every selectable machine dialect, in stable help/diagnostic order.
fn machine_formats() -> impl Iterator<Item = BuiltinFormat> {
    MACHINE_FORMATS.into_iter().chain(
        AnycubicWorkshop::ALL
            .into_iter()
            .map(BuiltinFormat::AnycubicWorkshop),
    )
}

/// Default gap bound of section closure at holes (TD-054): the `PrusaSlicer` bound, far above
/// tessellation gaps and below any deliberate opening worth printing as solid.
pub const DEFAULT_CLOSE_HOLES_MM: f64 = 2.0;

/// The reversed-sliver tolerance the desktop application gives new plates (TD-079), in mm² per
/// layer: far above the slivers that self-intersecting folds leave (a few pixels) and far below
/// the section of a body that is inside out as a whole. The command line has no default.
pub const DEFAULT_REVERSED_TOLERANCE_MM2: f64 = 0.1;

/// Largest reversed-sliver tolerance accepted, in mm² per layer.
pub const MAX_REVERSED_TOLERANCE_MM2: f64 = 100.0;

/// Default print-job identity recorded in exported metadata.
pub const DEFAULT_JOB_ID: &str = "slice";

/// Default repairs (TD-054): duplicate collapse and consistent orientation, exact and reported.
pub const DEFAULT_REPAIR: RepairSpec = RepairSpec {
    duplicates: true,
    orientation: true,
    outward: false,
    fill_holes: None,
    allow_unfilled: false,
    weld_mm: None,
    degenerate_mm: None,
    crack_mm: None,
    non_manifold: None,
    visibility: false,
    shells: None,
    rebuild_mm: None,
    // The command line refuses a model with facets without a position unless asked.
    non_finite: false,
};

/// A region the hollowing leaves solid (owner, 2026-11-02), in the object's model frame: it
/// follows every move, turn, scale and mirror of the object, as drain holes do.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Blocker {
    /// Two rectangular bases, parallel and centred on one axis: a box when they are alike, a
    /// truncated pyramid otherwise.
    Frustum {
        /// The point midway between the bases' centres.
        centre_mm: [f64; 3],
        /// Its turn about the model's X, then Y, then Z axes (fixed), in degrees; unturned, its
        /// width runs along X, its depth along Y and its height along Z.
        rotation_deg: [f64; 3],
        /// The lower base's width and depth.
        bottom_mm: [f64; 2],
        /// The upper base's width and depth; zero makes a pyramid's apex.
        top_mm: [f64; 2],
        /// The distance between the bases.
        height_mm: f64,
    },
    /// A ball.
    Sphere {
        /// Its centre.
        centre_mm: [f64; 3],
        /// Its diameter.
        diameter_mm: f64,
    },
}

impl Blocker {
    /// The blocker as the hollowing takes it: a frustum's turn as its axes.
    #[must_use]
    pub fn to_core(&self) -> HollowBlocker {
        match *self {
            Self::Sphere {
                centre_mm,
                diameter_mm,
            } => HollowBlocker::Sphere {
                centre: centre_mm,
                radius_mm: diameter_mm / 2.0,
            },
            Self::Frustum {
                centre_mm,
                rotation_deg,
                bottom_mm,
                top_mm,
                height_mm,
            } => HollowBlocker::Frustum {
                centre: centre_mm,
                axes: blocker_axes(rotation_deg),
                bottom_mm,
                top_mm,
                height_mm,
            },
        }
    }

    /// Its centre, in the model frame.
    #[must_use]
    pub const fn centre_mm(&self) -> [f64; 3] {
        match *self {
            Self::Frustum { centre_mm, .. } | Self::Sphere { centre_mm, .. } => centre_mm,
        }
    }
}

/// The width, depth and height directions of a frustum turned by `rotation_deg` about the fixed
/// X, then Y, then Z axes: the columns of `Rz · Ry · Rx`.
#[must_use]
pub fn blocker_axes(rotation_deg: [f64; 3]) -> [[f64; 3]; 3] {
    let [(sx, cx), (sy, cy), (sz, cz)] = rotation_deg.map(|a| a.to_radians().sin_cos());
    let rx = [[1.0, 0.0, 0.0], [0.0, cx, -sx], [0.0, sx, cx]];
    let ry = [[cy, 0.0, sy], [0.0, 1.0, 0.0], [-sy, 0.0, cy]];
    let rz = [[cz, -sz, 0.0], [sz, cz, 0.0], [0.0, 0.0, 1.0]];
    let product = |a: [[f64; 3]; 3], b: [[f64; 3]; 3]| -> [[f64; 3]; 3] {
        std::array::from_fn(|r| std::array::from_fn(|c| (0..3).map(|k| a[r][k] * b[k][c]).sum()))
    };
    let m = product(rz, product(ry, rx));
    std::array::from_fn(|axis| std::array::from_fn(|k| m[k][axis]))
}

/// A drain cut request, in world millimetres after placement.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DrainHole {
    /// An explicit cylinder along any direction (TD-066; cardinal before).
    Axis {
        /// Centre of the entry cap, outside the model.
        entry_mm: [f64; 3],
        /// Cut direction (any nonzero vector).
        direction: [f64; 3],
        /// Cut length along the direction.
        length_mm: f64,
        /// Cylinder diameter.
        diameter_mm: f64,
    },
    /// Through the wall at a point of a surface (TD-066, TD-084, TD-089).
    Surface {
        /// Point near the surface where the hole opens.
        surface_mm: [f64; 3],
        /// Cylinder diameter.
        diameter_mm: f64,
        /// Distance of the entry cap outside the surface.
        outside_mm: f64,
        /// Distance the cut reaches past the cavity wall (automatic depth).
        inside_mm: f64,
        /// Tilt of the axis from the surface normal, or `None` for a perpendicular hole.
        tilt: Option<HoleTilt>,
        /// Fixed depth from the surface, or `None` to reach `inside_mm` past the cavity wall
        /// (an outer hole of a hollowed object only).
        depth_mm: Option<f64>,
        /// The surface the hole starts from: the outer one, cutting inward, or the cavity's,
        /// cutting out through the wall (TD-089).
        surface: HoleSurface,
    },
}

impl DrainHole {
    /// The cylinder diameter in mm.
    #[must_use]
    pub const fn diameter_mm(&self) -> f64 {
        match self {
            Self::Axis { diameter_mm, .. } | Self::Surface { diameter_mm, .. } => *diameter_mm,
        }
    }

    /// This hole as a cut (TD-089): an explicit cylinder as given, a surface hole resolved by
    /// `probe`, with the frame it opens in.
    ///
    /// # Errors
    /// The probe's refusals of a surface hole (see [`HoleProbe::resolve`]) and cancellation.
    pub fn resolve(
        &self,
        probe: &HoleProbe,
        cancel: &mut Cancellation<'_>,
    ) -> Result<(HoleSpec, Option<HoleFrame>), DrainageError> {
        match *self {
            Self::Axis {
                entry_mm,
                direction,
                length_mm,
                diameter_mm,
            } => Ok((
                HoleSpec {
                    entry_mm,
                    direction,
                    length_mm,
                    diameter_mm,
                },
                None,
            )),
            Self::Surface {
                surface_mm,
                diameter_mm,
                outside_mm,
                inside_mm,
                tilt,
                depth_mm,
                surface,
            } => {
                let request = SurfaceHoleSpec {
                    surface_mm,
                    diameter_mm,
                    outside_mm,
                    inside_mm,
                    tilt,
                    depth_mm,
                };
                let resolved = probe.resolve(request, surface, cancel)?;
                Ok((resolved.spec, Some(resolved.frame)))
            }
        }
    }

    /// The unit normal a surface hole's tilt is measured from, as `probe` resolves it: the mean
    /// over the hole's radius at its foot, towards the free side. It does not depend on the
    /// tilt or the depth, so a short perpendicular hole finds it even where an automatic depth
    /// would find no cavity; but it changes with the diameter (TD-121). `None` for an axis hole.
    ///
    /// # Errors
    /// The probe's refusals of the hole without its tilt, and cancellation.
    pub fn outward(
        self,
        probe: &HoleProbe,
        cancel: &mut Cancellation<'_>,
    ) -> Result<Option<[f64; 3]>, DrainageError> {
        let Self::Surface {
            surface_mm,
            diameter_mm,
            outside_mm,
            inside_mm,
            surface,
            ..
        } = self
        else {
            return Ok(None);
        };
        let upright = Self::Surface {
            surface_mm,
            diameter_mm,
            outside_mm,
            inside_mm,
            tilt: None,
            depth_mm: Some(diameter_mm.max(1.0)),
            surface,
        };
        Ok(upright.resolve(probe, cancel)?.1.map(|frame| frame.outward))
    }

    /// This hole, given in its object's model frame and already moved to world coordinates by a
    /// placement whose linear part is `linear` (rows), with its tilt carried from the model frame
    /// into the world one, so the hole leans the same way on the turned model (TD-089). The
    /// world normal at the hole is `probe`'s, over the placed object. A hole without a tilt is
    /// returned as it is.
    ///
    /// # Errors
    /// The probe's refusals of the hole without its tilt, a singular placement
    /// ([`DrainageError::Unsupported`]), and cancellation.
    pub fn with_world_tilt(
        self,
        linear: [[f64; 3]; 3],
        probe: &HoleProbe,
        cancel: &mut Cancellation<'_>,
    ) -> Result<Self, DrainageError> {
        let Self::Surface {
            surface_mm,
            diameter_mm,
            outside_mm,
            inside_mm,
            tilt: Some(tilt),
            depth_mm,
            surface,
        } = self
        else {
            return Ok(self);
        };
        let Some(world) = self.outward(probe, cancel)? else {
            return Ok(self);
        };
        // Normals go back to the model frame by the transposed linear part.
        let back: [f64; 3] = std::array::from_fn(|c| (0..3).map(|r| linear[r][c] * world[r]).sum());
        let length = back.iter().map(|c| c * c).sum::<f64>().sqrt();
        let invalid = || DrainageError::Unsupported {
            reason: "the object's placement cannot carry the hole's tilt",
        };
        if !(length.is_finite() && length > 0.0) {
            return Err(invalid());
        }
        let model = back.map(|c| c / length);
        let tilt = map_tilt(tilt, model, linear, world).ok_or_else(invalid)?;
        Ok(Self::Surface {
            surface_mm,
            diameter_mm,
            outside_mm,
            inside_mm,
            tilt: Some(tilt),
            depth_mm,
            surface,
        })
    }
}

/// The coordinates drain holes (TD-089) and supports (TD-108) are given in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Coordinates {
    /// World millimetres at the object's design placement: the command line and plate
    /// documents.
    #[default]
    World,
    /// The object's model frame: millimetres of the imported model, before its placement. Holes
    /// and supports follow every move, turn, scale and mirror; diameters, depths and heights on
    /// shafts stay in millimetres (see [`SupportDocument::placed`]).
    Model,
}

/// The cavity simplification tolerance (TD-066).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Simplify {
    /// An explicit tolerance in mm.
    Millimeters(f64),
    /// Half the smaller pixel pitch, at most half the layer height.
    Auto,
}

impl Simplify {
    /// The tolerance in mm for this printer and calibrated layer height.
    ///
    /// # Errors
    /// A message when `Auto` needs the LCD pitch and the printer's LCD is invalid.
    pub fn resolve(self, printer: &PrinterProfile, layer_mm: f64) -> Result<f64, String> {
        match self {
            Self::Millimeters(mm) => Ok(mm),
            Self::Auto => {
                let pitch = printer
                    .lcd_profile()
                    .map_err(|e| format!("invalid printer LCD: {e}"))?
                    .pitch_mm();
                Ok((pitch[0].min(pitch[1]) / 2.0).min(layer_mm / 2.0))
            }
        }
    }
}

/// How layer sections become material.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fill {
    /// Positive winding number: overlapping and touching bodies merge, reversed shells subtract.
    Union,
    /// The strict even-odd pipeline: every layer must be an embedded set of contours.
    Strict,
}

impl Fill {
    /// The identifier shown in summaries.
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::Union => "union",
            Self::Strict => "strict",
        }
    }
}

/// A document the job needs: a file read and validated when the job runs, or a value already in
/// memory (such as a project's profiles).
#[derive(Clone, Debug, PartialEq)]
pub enum Input<T> {
    /// A file, read when the job runs.
    File(PathBuf),
    /// A validated value, with the name messages and summaries show for it.
    Loaded {
        /// Display name.
        name: String,
        /// The document.
        value: T,
    },
}

impl<T> Input<T> {
    /// How messages name the input: the quoted path or name.
    #[must_use]
    pub fn label(&self) -> String {
        match self {
            Self::File(path) => QuotedPath(path).to_string(),
            Self::Loaded { name, .. } => format!("{name:?}"),
        }
    }
}

/// The bytes of one model: a file, or a model embedded in a project.
#[derive(Clone, Debug, PartialEq)]
pub enum ModelSource {
    /// A model file, read when the job runs; its extension selects the format.
    File(PathBuf),
    /// Model bytes held in memory. `name` is the original file name: its extension selects the
    /// format, and messages show it.
    Embedded {
        /// Original file name.
        name: String,
        /// The file's bytes, shared without copying.
        bytes: Arc<[u8]>,
    },
}

impl ModelSource {
    /// How messages name the model: the quoted path or name.
    #[must_use]
    pub fn label(&self) -> String {
        match self {
            Self::File(path) => QuotedPath(path).to_string(),
            Self::Embedded { name, .. } => format!("{name:?}"),
        }
    }

    /// The plain path or name, as a default object name.
    #[must_use]
    pub fn display_name(&self) -> String {
        match self {
            Self::File(path) => path.display().to_string(),
            Self::Embedded { name, .. } => name.clone(),
        }
    }
}

/// What a job prints.
#[derive(Clone, Debug, PartialEq)]
pub enum Subject {
    /// One model, with the request's own placement, preparation and copies.
    Model(ModelSource),
    /// A `karasu-plate` document (TD-065). Its objects carry their own model, placement and
    /// preparation; their paths are relative to the document.
    PlateFile(PathBuf),
    /// A plate held in memory, such as a project plate.
    Plate(PlateSpec),
}

/// A plate held in memory: named objects, each with its copies.
#[derive(Clone, Debug, PartialEq)]
pub struct PlateSpec {
    /// Display name.
    pub name: String,
    /// Gap between arranged footprints, in mm.
    pub spacing_mm: f64,
    /// Objects in plate order.
    pub objects: Vec<ObjectSpec>,
}

/// One object of a plate: its model, design placement, preparation and copies.
///
/// The design placement and preparation have the meaning of the single-model request fields of
/// the same names (TD-065). The job-wide fields (profiles, output, masks, fill, repair) come from
/// the request.
#[derive(Clone, Debug, PartialEq)]
pub struct ObjectSpec {
    /// Display name.
    pub name: String,
    /// The model.
    pub model: ModelSource,
    /// Unit for model formats that declare none.
    pub units: Option<LengthUnit>,
    /// Explicit design translation in mm.
    pub translate_mm: [f64; 3],
    /// Rotation, scale, mirroring, centring, dropping and lift.
    pub pose: Pose,
    /// Wall thickness for hollowing, in mm.
    pub hollow_mm: Option<f64>,
    /// Explicit hollowing grid cell, in mm.
    pub hollow_cell_mm: Option<f64>,
    /// Drain cuts, in the coordinates `hole_coordinates` names.
    pub drain_holes: Vec<DrainHole>,
    /// The coordinates the drain holes are given in.
    pub hole_coordinates: Coordinates,
    /// Certified cavity simplification.
    pub hollow_simplify: Option<Simplify>,
    /// Regions the hollowing leaves solid, in the model frame (owner, 2026-11-02).
    pub hollow_blockers: Vec<Blocker>,
    /// Manual supports, in the coordinates `support_coordinates` names.
    pub supports: Option<Input<SupportDocument>>,
    /// The coordinates the supports are given in.
    pub support_coordinates: Coordinates,
    /// Accept every support collision with the model (still reported).
    pub accept_support_collisions: bool,
    /// Number of instances (at least one).
    pub copies: u32,
    /// Whether the arranger places the instances; otherwise the object stays where it is.
    pub arrange: bool,
    /// The object's own source repairs, used instead of the job's (M6.4d, TD-092).
    pub repair: Option<RepairSpec>,
}

/// One slicing job: what it prints, the profiles, the output and every processing option.
///
/// Files are read when the job runs; building a request performs no filesystem access.
#[derive(Clone, Debug, PartialEq)]
#[allow(clippy::struct_excessive_bools)] // Independent switches.
pub struct SliceRequest {
    /// A model, or a plate of objects; with a plate, the per-object fields below keep their
    /// defaults and each object carries its own.
    pub subject: Subject,
    /// Unit assumed for model formats that declare none (`--units`, or millimetres with
    /// `--assume-mm`).
    pub units: Option<LengthUnit>,
    /// The model was given with the legacy `--assume-mm`, which reads files without a known
    /// model extension as STL.
    pub legacy_stl: bool,
    /// Printer profile.
    pub printer: Input<PrinterProfile>,
    /// Resin profile, with a calibration for the printer.
    pub resin: Input<ResinProfile>,
    /// Output format identifier (see [`format_ids`]).
    pub format: String,
    /// Output file, or directory for the reference format; created only on success.
    pub output: PathBuf,
    /// Explicit world translation in mm (zero when computed by the pose).
    pub translate_mm: [f64; 3],
    /// Rotation, scale, mirroring, centring and dropping applied with the translation.
    pub pose: Pose,
    /// Print-job identity recorded in exported metadata.
    pub job_id: String,
    /// Slice a model with topology defects anyway (every layer must still close).
    pub allow_topology_defects: bool,
    /// How layer sections become material.
    pub fill: Fill,
    /// Mask samples per pixel axis (1: binary masks).
    pub antialias: u8,
    /// Smallest gray of a partially covered pixel (0 keeps gray proportional to coverage).
    pub min_gray: u8,
    /// Anti-alias the supports with the model; otherwise they are binary, whole pixels at their
    /// centres, and only the model is anti-aliased (TD-134).
    pub antialias_supports: bool,
    /// Signed XY compensation of every layer, in pixels (positive grows material).
    pub xy_compensation_px: i16,
    /// Signed XY compensation of the bottom layers instead, in pixels, when given.
    pub bottom_xy_compensation_px: Option<i16>,
    /// Gap bound of section closure at holes (mm), or `None` when disabled (union fill only).
    pub close_holes_mm: Option<f64>,
    /// Area per layer (mm²) of negative winding left empty instead of failing the layer, or
    /// `None` to refuse any (union fill only, TD-079). Negative winding inside that bound is
    /// reported in the summary.
    pub reversed_tolerance_mm2: Option<f64>,
    /// Explicit source repairs applied to a derived mesh before slicing or hollowing.
    pub repair: RepairSpec,
    /// Wall thickness for hollowing, in mm.
    pub hollow_mm: Option<f64>,
    /// Explicit hollowing grid cell, in mm.
    pub hollow_cell_mm: Option<f64>,
    /// Drain cuts in command-line order, in the coordinates `hole_coordinates` names.
    pub drain_holes: Vec<DrainHole>,
    /// The coordinates the drain holes are given in.
    pub hole_coordinates: Coordinates,
    /// Certified cavity simplification (requires hollowing, TD-066).
    pub hollow_simplify: Option<Simplify>,
    /// Regions the hollowing leaves solid, in the model frame (owner, 2026-11-02).
    pub hollow_blockers: Vec<Blocker>,
    /// Manual support document (TD-062), in the coordinates `support_coordinates` names.
    pub supports: Option<Input<SupportDocument>>,
    /// The coordinates the supports are given in.
    pub support_coordinates: Coordinates,
    /// Accept every support collision with the model (still reported).
    pub accept_support_collisions: bool,
    /// Variable or adaptive layer heights (karasu-layers, TD-064).
    pub layer_plan: Option<Input<LayerPlanDocument>>,
    /// Instances of the model (at least one); more than one are arranged.
    pub copies: u32,
    /// Whether the arranger places the model's instances in XY.
    pub arrange: bool,
    /// Gap between arranged footprints, in mm.
    pub spacing_mm: f64,
    /// Bytes the hollowing, and each index of drain holes, may request (TD-094), or `None` for
    /// [`crate::pipeline::DEFAULT_MEMORY_BUDGET_BYTES`]. A cavity no hole index could take is
    /// refused when hollowed, so what can be hollowed can be drilled.
    pub memory_budget_bytes: Option<u64>,
    /// Verify the printed walls on the sliced layers (`--verify-walls`, TD-095): the job then
    /// keeps its masks and rasterizes its drain holes and cavities alone.
    pub verify_walls: bool,
    /// Find the print's unsupported regions on the sliced layers for this critical angle, in
    /// degrees from the plate's plane (`--unsupported`, TD-107): the job then keeps its masks.
    pub find_unsupported: Option<f64>,
    /// Find the print's suction cups on the sliced layers, with this minimum vent area in mm²
    /// (`--suction-cups`, TD-114): the job then keeps its masks.
    pub find_suction_cups: Option<f64>,
    /// Slice supports with error findings as they are built, with a warning, instead of
    /// refusing the job (the interface's policy, owner 2026-10-27, TD-108): a support that
    /// could not be built at all is left out and reported. The command line refuses them.
    pub slice_support_errors: bool,
    /// Slice a model lifted above the plate where it stands although nothing holds it: for an
    /// analysis of the print only, never an export (the interface's islands search slices a
    /// model without its supports to find every island, TD-107). Off, such a model is refused.
    pub allow_floating: bool,
}

/// The model's orientation and size, and which parts of its translation are computed.
#[derive(Clone, Debug, PartialEq)]
pub struct Pose {
    /// Fixed-axis X, then Y, then Z rotation, in degrees, about the source bounding-box centre.
    pub rotate_deg: [f64; 3],
    /// Positive per-axis scale factors, about the same centre.
    pub scale: [f64; 3],
    /// Axes reflected (negative scale), about the same centre.
    pub mirror: [bool; 3],
    /// Centre the placed model's XY extent on the build plate.
    pub center: bool,
    /// Lower or raise the placed model to rest exactly on Z = 0.
    pub drop: bool,
    /// With `drop`, the height of the model's lowest point above the plate (for supports).
    pub lift_mm: Option<f64>,
}

impl Default for Pose {
    fn default() -> Self {
        Self {
            rotate_deg: [0.; 3],
            scale: [1.; 3],
            mirror: [false; 3],
            center: false,
            drop: false,
            lift_mm: None,
        }
    }
}

impl Pose {
    /// Whether rotation, scale and mirroring leave the model unchanged.
    #[allow(clippy::float_cmp)] // Only the exact defaults keep the translation-only evaluation.
    #[must_use]
    pub fn is_rigid_identity(&self) -> bool {
        self.rotate_deg == [0.; 3] && self.scale == [1.; 3] && self.mirror == [false; 3]
    }
}

impl SliceRequest {
    /// The request for one object of a plate: the object's own fields with the job-wide ones.
    #[must_use]
    pub fn for_object(&self, object: &ObjectSpec) -> Self {
        Self {
            subject: Subject::Model(object.model.clone()),
            units: object.units,
            legacy_stl: false,
            translate_mm: object.translate_mm,
            pose: object.pose.clone(),
            hollow_mm: object.hollow_mm,
            hollow_cell_mm: object.hollow_cell_mm,
            drain_holes: object.drain_holes.clone(),
            hole_coordinates: object.hole_coordinates,
            hollow_simplify: object.hollow_simplify,
            hollow_blockers: object.hollow_blockers.clone(),
            supports: object.supports.clone(),
            support_coordinates: object.support_coordinates,
            accept_support_collisions: object.accept_support_collisions,
            copies: object.copies,
            arrange: object.arrange,
            printer: self.printer.clone(),
            resin: self.resin.clone(),
            format: self.format.clone(),
            output: self.output.clone(),
            job_id: self.job_id.clone(),
            allow_topology_defects: self.allow_topology_defects,
            fill: self.fill,
            antialias: self.antialias,
            min_gray: self.min_gray,
            antialias_supports: self.antialias_supports,
            xy_compensation_px: self.xy_compensation_px,
            bottom_xy_compensation_px: self.bottom_xy_compensation_px,
            close_holes_mm: self.close_holes_mm,
            reversed_tolerance_mm2: self.reversed_tolerance_mm2,
            repair: object.repair.unwrap_or(self.repair),
            layer_plan: self.layer_plan.clone(),
            spacing_mm: self.spacing_mm,
            memory_budget_bytes: self.memory_budget_bytes,
            verify_walls: self.verify_walls,
            find_unsupported: self.find_unsupported,
            find_suction_cups: self.find_suction_cups,
            slice_support_errors: self.slice_support_errors,
            allow_floating: self.allow_floating,
        }
    }

    /// The model of a single-model request.
    #[must_use]
    pub fn model(&self) -> Option<&ModelSource> {
        match &self.subject {
            Subject::Model(model) => Some(model),
            Subject::PlateFile(_) | Subject::Plate(_) => None,
        }
    }

    /// Check the requirements between options.
    ///
    /// # Errors
    /// A message naming the conflicting options, or the unsupported format.
    pub fn checked(self) -> Result<Self, String> {
        if self.supports.is_some() && self.fill == Fill::Strict {
            return Err(
                "--supports needs --fill union: supports and the model merge by winding union"
                    .to_owned(),
            );
        }
        if self.accept_support_collisions && self.supports.is_none() {
            return Err("--accept-support-collisions requires --supports <file.json>".to_owned());
        }
        // A project lifts a model before its supports are placed (TD-108), so only the command
        // line refuses the option at once; slicing refuses a model left above the plate.
        if self.pose.lift_mm.is_some_and(|lift| lift > 0.)
            && self.supports.is_none()
            && self.support_coordinates == Coordinates::World
        {
            return Err(
                "--lift raises the model off the plate, which requires --supports".to_owned(),
            );
        }
        if self.hollow_cell_mm.is_some() && self.hollow_mm.is_none() {
            return Err("--hollow-cell requires --hollow <wall thickness>".to_owned());
        }
        if self.hollow_simplify.is_some() && self.hollow_mm.is_none() {
            return Err("--hollow-simplify requires --hollow <wall thickness>".to_owned());
        }
        // TD-089: holes are independent of hollowing, but a surface hole without a depth
        // reaches the cavity, and one from the cavity starts on it: only hollowing makes it.
        let automatic =
            |hole: &DrainHole| matches!(hole, DrainHole::Surface { depth_mm: None, .. });
        let from_cavity = |hole: &DrainHole| {
            matches!(
                hole,
                DrainHole::Surface {
                    surface: HoleSurface::Cavity,
                    ..
                }
            )
        };
        if self.hollow_mm.is_none() {
            if self.drain_holes.iter().any(from_cavity) {
                return Err("--hole-from cavity requires --hollow <wall thickness>".to_owned());
            }
            if self.drain_holes.iter().any(automatic) {
                return Err(
                    "--surface-drain-hole requires --hollow <wall thickness>, or --hole-depth \
                     <mm>: without a depth it reaches the cavity"
                        .to_owned(),
                );
            }
        }
        if self.min_gray > 0 && self.antialias == 1 {
            return Err(
                "--min-gray applies to antialiased edges; it requires --antialias 2, 4, 8 or 16"
                    .to_owned(),
            );
        }
        if let Some(area) = self.reversed_tolerance_mm2 {
            if !(area.is_finite() && area > 0. && area <= MAX_REVERSED_TOLERANCE_MM2) {
                return Err(format!(
                    "--reversed-tolerance requires a finite positive area of at most \
                     {MAX_REVERSED_TOLERANCE_MM2} mm^2"
                ));
            }
            if self.fill == Fill::Strict {
                return Err(
                    "--reversed-tolerance applies to --fill union: strict filling has no winding"
                        .to_owned(),
                );
            }
        }
        machine_exporter_with(&self.format, self.antialias)?;
        Ok(self)
    }
}

/// The exporter for `format` with binary masks; `None` for the reference format.
///
/// # Errors
/// A message listing the supported identifiers when `format` is unknown.
pub fn machine_exporter(format: &str) -> Result<Option<BuiltinExporter>, String> {
    machine_exporter_with(format, 1)
}

/// The exporter for `format` with masks supersampled `antialias` times per axis.
///
/// Binary masks are exactly representable in every dialect, so any loss is an error. Antialiased
/// masks are generated at each dialect's native depth (8-bit GOO, 16-level PP1), except CTB, whose
/// seven-bit samples receive the declared nearest-level rounding; legacy Photon stores binary
/// masks only and is refused.
///
/// # Errors
/// A message when `format` is unknown or cannot store antialiased masks.
pub fn machine_exporter_with(
    format: &str,
    antialias: u8,
) -> Result<Option<BuiltinExporter>, String> {
    if format == REFERENCE_FORMAT {
        return Ok(None);
    }
    let selected = machine_formats()
        .find(|&f| BuiltinExporter::new(f, PixelPolicy::Exact).id() == format)
        .ok_or_else(|| {
            format!(
                "unsupported --format {format:?}; supported: {}",
                format_ids()
            )
        })?;
    if antialias == 1 {
        return Ok(Some(BuiltinExporter::new(selected, PixelPolicy::Exact)));
    }
    let policy = match selected {
        BuiltinFormat::PhotonV1FirmwareMotion => {
            return Err(format!(
                "--format {format} stores binary masks; --antialias needs a CTB, GOO, Anycubic \
                 or {REFERENCE_FORMAT} format"
            ))
        }
        BuiltinFormat::CtbV2Plain
        | BuiltinFormat::CtbV3Plain
        | BuiltinFormat::CtbV4Plain
        | BuiltinFormat::CtbV5Plain => PixelPolicy::RoundToFormat,
        // Bit-planes store fewer grays than the masks: round to the nearest.
        BuiltinFormat::AnycubicWorkshop(target) if target.bitplanes() => PixelPolicy::RoundToFormat,
        BuiltinFormat::GooV3
        | BuiltinFormat::AnycubicPhotonP1
        | BuiltinFormat::AnycubicPhotonP1Max
        | BuiltinFormat::AnycubicWorkshop(_)
        | BuiltinFormat::PrusaSl1FirmwareMotion
        | BuiltinFormat::PrusaSl1sFirmwareMotion => PixelPolicy::Exact,
    };
    Ok(Some(
        BuiltinExporter::new(selected, policy).with_antialiasing(antialias),
    ))
}

/// A machine format as the interface names it (TD-106): its family, its name and its file
/// extension, for the format's identifier.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FormatName {
    /// The format identifier (see [`format_ids`]).
    pub id: String,
    /// Who defines the format, such as "CHITUBOX" or "Anycubic Photon Workshop".
    pub family: &'static str,
    /// The format's name within its family, such as "CTB version 4, unencrypted" (owner,
    /// 2026-11-11: karasu writes no encrypted CTB, and the name says so) or "Photon Mono X".
    pub name: String,
    /// The file extension, without its dot.
    pub extension: String,
}

/// Every machine format this build writes, named, in the order of [`format_ids`]. The reference
/// format is not a printer's and is not listed.
#[must_use]
pub fn format_names() -> Vec<FormatName> {
    machine_formats()
        .map(|format| {
            let exporter = BuiltinExporter::new(format, PixelPolicy::Exact);
            let (family, name) = match format {
                BuiltinFormat::CtbV2Plain => ("CHITUBOX", "CTB version 2, unencrypted".to_owned()),
                BuiltinFormat::CtbV3Plain => ("CHITUBOX", "CTB version 3, unencrypted".to_owned()),
                BuiltinFormat::CtbV4Plain => ("CHITUBOX", "CTB version 4, unencrypted".to_owned()),
                BuiltinFormat::CtbV5Plain => ("CHITUBOX", "CTB version 5, unencrypted".to_owned()),
                BuiltinFormat::GooV3 => ("ELEGOO", "GOO".to_owned()),
                BuiltinFormat::AnycubicPhotonP1 => ("Anycubic", "Photon P1".to_owned()),
                BuiltinFormat::AnycubicPhotonP1Max => ("Anycubic", "Photon P1 Max".to_owned()),
                BuiltinFormat::AnycubicWorkshop(target) => {
                    ("Anycubic Photon Workshop", target.models().to_owned())
                }
                BuiltinFormat::PhotonV1FirmwareMotion => ("Anycubic", "Photon".to_owned()),
                BuiltinFormat::PrusaSl1FirmwareMotion => ("Prusa", "SL1".to_owned()),
                BuiltinFormat::PrusaSl1sFirmwareMotion => ("Prusa", "SL1S".to_owned()),
            };
            FormatName {
                id: exporter.id().to_owned(),
                family,
                name,
                extension: exporter.extension().to_owned(),
            }
        })
        .collect()
}

/// The name of the machine format `id`, or `None` for the reference format or an unknown one.
#[must_use]
pub fn format_name(id: &str) -> Option<FormatName> {
    format_names().into_iter().find(|f| f.id == id)
}

/// Supported format identifiers, for help and diagnostics.
#[must_use]
pub fn format_ids() -> String {
    machine_formats()
        .map(|f| BuiltinExporter::new(f, PixelPolicy::Exact).id().to_owned())
        .chain([REFERENCE_FORMAT.to_owned()])
        .collect::<Vec<_>>()
        .join(", ")
}
