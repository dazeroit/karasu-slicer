//! The `slice` command: option parsing, terminal progress and Ctrl-C handling around the shared
//! pipeline of `karasu-project` (M6.1), which does all the work and writes the summary.
//!
//! The first Ctrl-C (SIGINT, or the Windows console equivalent) sets a flag that every core
//! cancellation checkpoint observes; the pipeline then fails through its ordinary error path, so
//! export staging data is removed and nothing is published. A second Ctrl-C exits immediately.
use super::progress::{Progress, ProgressMode};
use karasu_core::{
    cancellation::Checkpoint,
    mesh::formats::LengthUnit,
    preparation::{
        repair::{
            FillHoles, NonManifold, RepairPreset, RepairSpec, Shells, DEFAULT_REBUILD_MM,
            MAX_REBUILD_MM, MAX_TOLERANCE_MM, MIN_REBUILD_MM,
        },
        sampled::HoleSurface,
        HoleTilt, MAX_HOLE_TILT_DEG,
    },
    profiles::{store::ProfileDocument, MAX_PROFILE_BYTES},
};
use karasu_project::{
    pipeline,
    request::{
        machine_exporter, Coordinates, DrainHole, Fill, Input, ModelSource, Pose, Simplify,
        SliceRequest, Subject, DEFAULT_CLOSE_HOLES_MM, DEFAULT_JOB_ID, DEFAULT_REPAIR,
    },
};
use std::{
    ffi::{OsStr, OsString},
    fs::File,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

pub(super) use karasu_project::request::format_ids;

/// The critical angle `--unsupported` uses without `--overhang-angle`, in degrees from the plate's
/// plane (TD-107).
const DEFAULT_OVERHANG_ANGLE_DEG: f64 = 45.0;

/// The smallest vent `--suction-cups` credits without `--min-vent`, in mm² (TD-114): a circle
/// about 1.1 mm across.
const DEFAULT_MIN_VENT_MM2: f64 = 1.0;

/// Set by the first Ctrl-C; read by every cancellation predicate of the pipeline.
pub(super) static INTERRUPTED: AtomicBool = AtomicBool::new(false);

/// Conventional exit status for termination by SIGINT (128 + 2), used on every platform.
pub(super) const INTERRUPTED_EXIT_STATUS: u8 = 130;

/// Cancellation predicate shared by all pipeline stages.
pub(super) fn interrupted(_: Checkpoint) -> bool {
    INTERRUPTED.load(Ordering::Acquire)
}

/// Terminal outcome of a failed slice; interrupted runs never publish output.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum SliceFailure {
    /// The user pressed Ctrl-C and the pipeline stopped at a cancellation checkpoint.
    Interrupted,
    /// Ordinary processing, validation, I/O or export failure.
    Error(String),
}

impl From<String> for SliceFailure {
    fn from(message: String) -> Self {
        Self::Error(message)
    }
}

impl From<&str> for SliceFailure {
    fn from(message: &str) -> Self {
        Self::Error(message.to_owned())
    }
}

/// Validated `slice` arguments; parsing performs no filesystem access.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct SliceArgs {
    /// The job the pipeline runs.
    pub(super) request: SliceRequest,
    /// How progress is reported on standard error.
    pub(super) progress: ProgressMode,
    /// The session log's file (`--log`, TD-096), or `None` for no log.
    pub(super) log: Option<PathBuf>,
    /// A library print setting (`--print-setting`, TD-106): `--resin` then names a library
    /// resin, and the setting supplies the calibration, masks and compensation.
    pub(super) print_setting: Option<PathBuf>,
}

/// Default gap between arranged footprints (TD-065 owner decision).
const DEFAULT_SPACING_MM: f64 = karasu_core::plate::DEFAULT_SPACING_MM;
/// Most instances of a single model.
const MAX_COPIES: u32 = karasu_core::plate::MAX_COPIES;

/// Default distance of a surface hole's entry outside the surface, and of its end past the
/// cavity wall (TD-066).
const SURFACE_HOLE_MARGIN_MM: f64 = 1.0;

/// Maximum `--drain-hole` options, matching the core's bound on holes (TD-089).
const MAX_DRAIN_HOLES: usize = karasu_core::preparation::sampled::MAX_HOLES;

/// Placement options, each given at most once.
#[derive(Default)]
struct PlacementOptions {
    translate: Option<[f64; 3]>,
    pose: Pose,
    given: Vec<&'static str>,
}

impl PlacementOptions {
    /// The explicit translation (default zero) and the pose.
    fn finish(self) -> Result<([f64; 3], Pose), String> {
        if self.translate.is_some() && (self.pose.center || self.pose.drop) {
            return Err(
                "--translate sets the position explicitly; it cannot be combined with --center or \
                 --drop"
                    .to_owned(),
            );
        }
        if self.pose.lift_mm.is_some() && !self.pose.drop {
            return Err(
                "--lift sets the height that --drop rests the model at; add --drop".to_owned(),
            );
        }
        Ok((self.translate.unwrap_or([0.; 3]), self.pose))
    }

    /// Consume `name` and its values if it is a placement option.
    fn apply(
        &mut self,
        name: &str,
        arguments: &mut impl Iterator<Item = OsString>,
    ) -> Result<bool, String> {
        let pose = &mut self.pose;
        let option: &'static str = match name {
            "--translate" => {
                let mut offset = [0.; 3];
                for (axis, value) in ["X", "Y", "Z"].into_iter().zip(&mut offset) {
                    *value = millimeters(name, axis, arguments.next().as_deref())?;
                }
                self.translate = Some(offset);
                "--translate"
            }
            "--rotate" => {
                for (axis, value) in ["X", "Y", "Z"].into_iter().zip(&mut pose.rotate_deg) {
                    *value = finite_value(name, "degree", axis, arguments.next().as_deref())?;
                }
                "--rotate"
            }
            "--scale" => {
                for (axis, value) in ["X", "Y", "Z"].into_iter().zip(&mut pose.scale) {
                    *value =
                        finite_value(name, "positive factor", axis, arguments.next().as_deref())?;
                    if *value <= 0. {
                        return Err(format!(
                            "--scale factors must be positive ({axis} is {value}); use --mirror to reflect"
                        ));
                    }
                }
                "--scale"
            }
            "--mirror" => {
                let axes = text_value(name, arguments.next())?;
                for axis in axes.chars() {
                    let index = match axis {
                        'x' | 'X' => 0,
                        'y' | 'Y' => 1,
                        'z' | 'Z' => 2,
                        _ => {
                            return Err(format!(
                                "--mirror takes the axes to reflect (x, y, z, such as xy), not {axes:?}"
                            ))
                        }
                    };
                    if std::mem::replace(&mut pose.mirror[index], true) {
                        return Err(format!("--mirror names axis {axis} twice"));
                    }
                }
                "--mirror"
            }
            "--center" => {
                pose.center = true;
                "--center"
            }
            "--drop" => {
                pose.drop = true;
                "--drop"
            }
            "--lift" => {
                pose.lift_mm = Some(
                    finite_value(name, "millimeter", "lift", arguments.next().as_deref())
                        .ok()
                        .filter(|v| *v >= 0.)
                        .ok_or("--lift requires one nonnegative finite height in millimeters")?,
                );
                "--lift"
            }
            _ => return Ok(false),
        };
        if self.given.contains(&option) {
            return Err(format!("{option} may be given only once"));
        }
        self.given.push(option);
        Ok(true)
    }
}

/// Parse options after the `slice` subcommand, in any order. Each option appears at most once,
/// except `--drain-hole`, which may be repeated.
#[allow(clippy::too_many_lines)] // Every option of the command, matched in one place.
pub(super) fn parse(mut arguments: impl Iterator<Item = OsString>) -> Result<SliceArgs, String> {
    let mut model = ModelOptions::default();
    let mut printer = None;
    let mut resin = None;
    let mut print_setting = None;
    let mut format = None;
    let mut output = None;
    let mut job_id = None;
    let mut allow_topology_defects = false;
    let mut processing = Processing::default();
    let mut hollowing = Hollowing::default();
    let mut placement = PlacementOptions::default();
    let mut plate = PlateOptions::default();
    while let Some(option) = arguments.next() {
        let name = option
            .to_str()
            .ok_or("slice options must be Unicode")?
            .to_owned();
        match name.as_str() {
            "--printer" => set(&mut printer, &name, path_value(&name, arguments.next())?)?,
            "--resin" => set(&mut resin, &name, path_value(&name, arguments.next())?)?,
            "--print-setting" => set(
                &mut print_setting,
                &name,
                path_value(&name, arguments.next())?,
            )?,
            "--output" => set(&mut output, &name, path_value(&name, arguments.next())?)?,
            "--format" => {
                let value = text_value(&name, arguments.next())?;
                machine_exporter(&value)?;
                set(&mut format, &name, value)?;
            }
            "--job-id" => set(&mut job_id, &name, text_value(&name, arguments.next())?)?,
            "--allow-topology-defects" if !allow_topology_defects => allow_topology_defects = true,
            "--allow-topology-defects" => return Err(format!("{name} may be given only once")),
            _ if hollowing.apply(&name, &mut arguments)? => {}
            _ if processing.apply(&name, &mut arguments)? => {}
            _ if plate.apply(&name, &mut arguments)? => {}
            _ if placement.apply(&name, &mut arguments)? => {}
            _ if model.apply(&name, &mut arguments)? => {}
            _ => return Err(format!("unknown slice option: {name}")),
        }
    }
    let per_object = model.path.is_some()
        || model.units.is_some()
        || !placement.given.is_empty()
        || hollowing != Hollowing::default()
        || processing.supports.is_some()
        || processing.accept_support_collisions.is_some();
    let arranged = plate.check(per_object, placement.pose.center)?;
    let (translate_mm, pose) = placement.finish()?;
    let Processing {
        fill,
        antialias,
        min_gray,
        xy_compensation,
        bottom_xy_compensation,
        supports,
        accept_support_collisions,
        antialias_supports,
        layer_plan,
        progress,
        close_holes,
        reversed_tolerance,
        repair,
        verify_walls,
        unsupported,
        overhang_angle,
        suction_cups,
        min_vent,
        log,
    } = processing;
    if overhang_angle.is_some() && unsupported.is_none() {
        return Err("--overhang-angle is used only with --unsupported".to_owned());
    }
    if min_vent.is_some() && suction_cups.is_none() {
        return Err("--min-vent is used only with --suction-cups".to_owned());
    }
    if print_setting.is_some()
        && (antialias.is_some()
            || min_gray.is_some()
            || xy_compensation.is_some()
            || bottom_xy_compensation.is_some())
    {
        return Err(
            "--print-setting holds the anti-aliasing, minimum gray and XY compensation: edit \
             the print setting instead of giving --antialias, --min-gray or --xy-compensation"
                .to_owned(),
        );
    }
    SliceRequest {
        subject: match plate.plate {
            Some(path) => Subject::PlateFile(path),
            None => Subject::Model(ModelSource::File(required(
                model.path,
                "--model <path> (or --assume-mm <path>), or --plate <file>",
            )?)),
        },
        units: model.units,
        legacy_stl: model.legacy_stl,
        printer: Input::File(required(printer, "--printer <printer.json>")?),
        resin: Input::File(required(resin, "--resin <resin.json>")?),
        format: required(format, "--format <id>")?,
        output: required(output, "--output <path>")?,
        translate_mm,
        pose,
        job_id: job_id.unwrap_or_else(|| DEFAULT_JOB_ID.to_owned()),
        allow_topology_defects,
        fill: fill.unwrap_or(Fill::Union),
        antialias: antialias.unwrap_or(1),
        min_gray: min_gray.unwrap_or(0),
        antialias_supports: antialias_supports.unwrap_or(false),
        xy_compensation_px: xy_compensation.unwrap_or(0),
        bottom_xy_compensation_px: bottom_xy_compensation,
        close_holes_mm: close_holes.map_or(Some(DEFAULT_CLOSE_HOLES_MM), Closure::bound),
        reversed_tolerance_mm2: reversed_tolerance,
        repair: repair.unwrap_or(DEFAULT_REPAIR),
        hollow_mm: hollowing.wall,
        hollow_cell_mm: hollowing.cell,
        drain_holes: hollowing.drains,
        hole_coordinates: Coordinates::World,
        support_coordinates: Coordinates::World,
        hollow_simplify: hollowing.simplify,
        hollow_blockers: Vec::new(),
        supports: supports.map(Input::File),
        accept_support_collisions: accept_support_collisions.unwrap_or(false),
        layer_plan: layer_plan.map(Input::File),
        copies: plate.copies.unwrap_or(1),
        arrange: arranged,
        spacing_mm: plate.spacing.unwrap_or(DEFAULT_SPACING_MM),
        memory_budget_bytes: None,
        verify_walls: verify_walls.unwrap_or(false),
        find_unsupported: unsupported.map(|_| overhang_angle.unwrap_or(DEFAULT_OVERHANG_ANGLE_DEG)),
        find_suction_cups: suction_cups.map(|_| min_vent.unwrap_or(DEFAULT_MIN_VENT_MM2)),
        slice_support_errors: false,
        allow_floating: false,
    }
    .checked()
    .map(|request| SliceArgs {
        request,
        progress: progress.unwrap_or(ProgressMode::Auto),
        log,
        print_setting,
    })
}

/// The request with the library print setting in `path` (TD-106): `--resin`'s library resin
/// with that setting becomes the job's calibrated resin, and the setting's image settings
/// replace the command line's defaults.
fn with_print_setting(request: &SliceRequest, path: &Path) -> Result<SliceRequest, String> {
    let read = |path: &Path, what: &str| {
        let mut bytes = Vec::new();
        File::open(path)
            .and_then(|file| {
                file.take(u64::try_from(MAX_PROFILE_BYTES).unwrap_or(u64::MAX) + 1)
                    .read_to_end(&mut bytes)
            })
            .map_err(|e| format!("cannot read {what} {}: {e}", path.display()))?;
        ProfileDocument::from_json(&bytes)
            .map_err(|e| format!("invalid {what} {}: {e}", path.display()))
    };
    let ProfileDocument::PrintSetting(setting) = read(path, "print setting")? else {
        return Err(format!(
            "{} is not a print setting; --print-setting requires kind \"print_setting\"",
            path.display()
        ));
    };
    let Input::File(resin_path) = &request.resin else {
        return Err("--print-setting needs --resin <resin.json>".to_owned());
    };
    let ProfileDocument::Material(material) = read(resin_path, "resin")? else {
        return Err(format!(
            "{} is not a library resin; with --print-setting, --resin requires kind \"material\"",
            resin_path.display()
        ));
    };
    let resin = setting.job_resin(&material).map_err(|e| e.to_string())?;
    SliceRequest {
        resin: Input::Loaded {
            name: material.name.clone(),
            value: resin,
        },
        antialias: setting.image.antialias,
        min_gray: setting.image.min_gray,
        xy_compensation_px: setting.image.xy_compensation_px,
        bottom_xy_compensation_px: setting.image.bottom_xy_compensation_px,
        ..request.clone()
    }
    .checked()
}

/// Hollowing options: the wall and grid cell at most once, drain holes repeatable.
#[derive(Default, PartialEq)]
struct Hollowing {
    wall: Option<f64>,
    cell: Option<f64>,
    drains: Vec<DrainHole>,
    simplify: Option<Simplify>,
    /// The hole modifiers given for the last `--surface-drain-hole`, each at most once.
    modifiers: Vec<&'static str>,
}

impl Hollowing {
    /// `--hole-depth <mm>`, `--hole-from outer|cavity` and `--hole-tilt <angle> <azimuth>`: they
    /// shape the `--surface-drain-hole` just before them (TD-089), each at most once per hole.
    fn modify(
        &mut self,
        name: &str,
        arguments: &mut impl Iterator<Item = OsString>,
    ) -> Result<(), String> {
        let Some(DrainHole::Surface {
            depth_mm,
            surface,
            tilt,
            ..
        }) = self.drains.last_mut()
        else {
            return Err(format!(
                "{name} applies to the --surface-drain-hole just before it"
            ));
        };
        let option = match name {
            "--hole-depth" => "--hole-depth",
            "--hole-from" => "--hole-from",
            _ => "--hole-tilt",
        };
        if self.modifiers.contains(&option) {
            return Err(format!(
                "{name} may be given only once per --surface-drain-hole"
            ));
        }
        self.modifiers.push(option);
        match option {
            "--hole-depth" => *depth_mm = Some(positive_mm(name, arguments.next().as_deref())?),
            "--hole-from" => {
                *surface = match text_value(name, arguments.next())?.as_str() {
                    "outer" => HoleSurface::Outer,
                    "cavity" => HoleSurface::Cavity,
                    other => return Err(format!("{name} takes outer or cavity, not {other:?}")),
                }
            }
            _ => {
                let mut value = || {
                    arguments
                        .next()
                        .as_deref()
                        .and_then(OsStr::to_str)
                        .and_then(|v| v.parse::<f64>().ok())
                        .filter(|v| v.is_finite())
                };
                let (angle_deg, azimuth_deg) = (value(), value());
                match (angle_deg, azimuth_deg) {
                    (Some(angle_deg), Some(azimuth_deg))
                        if (0.0..=MAX_HOLE_TILT_DEG).contains(&angle_deg)
                            && azimuth_deg.abs() <= 360.0 =>
                    {
                        // No tilt is a perpendicular hole, recorded as such.
                        *tilt = (angle_deg > 0.0).then_some(HoleTilt {
                            angle_deg,
                            azimuth_deg,
                        });
                    }
                    _ => {
                        return Err(format!(
                            "{name} requires an angle from 0 to {MAX_HOLE_TILT_DEG} degrees and \
                             a direction from -360 to 360 degrees"
                        ))
                    }
                }
            }
        }
        Ok(())
    }

    /// Consume `name` and its values if it is a hollowing option.
    fn apply(
        &mut self,
        name: &str,
        arguments: &mut impl Iterator<Item = OsString>,
    ) -> Result<bool, String> {
        match name {
            "--hollow" => set(
                &mut self.wall,
                name,
                positive_mm(name, arguments.next().as_deref())?,
            )?,
            "--hollow-cell" => set(
                &mut self.cell,
                name,
                positive_mm(name, arguments.next().as_deref())?,
            )?,
            "--drain-hole" | "--surface-drain-hole" => {
                if self.drains.len() == MAX_DRAIN_HOLES {
                    return Err(format!(
                        "at most {MAX_DRAIN_HOLES} --drain-hole and --surface-drain-hole options \
                         are supported"
                    ));
                }
                self.drains.push(if name == "--drain-hole" {
                    drain_hole(name, arguments)?
                } else {
                    surface_drain_hole(name, arguments)?
                });
                self.modifiers.clear();
            }
            "--hole-depth" | "--hole-from" | "--hole-tilt" => self.modify(name, arguments)?,
            "--hollow-simplify" => {
                let text = text_value(name, arguments.next())?;
                let value = if text == "auto" {
                    Simplify::Auto
                } else {
                    Simplify::Millimeters(
                        text.parse::<f64>()
                            .ok()
                            .filter(|v| v.is_finite() && *v > 0.0)
                            .ok_or_else(|| {
                                format!(
                                    "{name} takes a positive tolerance in mm or auto, not {text:?}"
                                )
                            })?,
                    )
                };
                set(&mut self.simplify, name, value)?;
            }
            _ => return Ok(false),
        }
        Ok(true)
    }
}

/// Plate and arrangement options (TD-065), each given at most once.
#[derive(Default)]
struct PlateOptions {
    plate: Option<PathBuf>,
    copies: Option<u32>,
    arrange: bool,
    spacing: Option<f64>,
}

impl PlateOptions {
    /// Whether the model is arranged; `per_object` tells whether any per-object option was
    /// given, and `center` whether `--center` was.
    fn check(&self, per_object: bool, center: bool) -> Result<bool, String> {
        if self.plate.is_some() && per_object {
            return Err(
                "--plate describes every object: model, placement, hollowing and \
                        support options belong in the plate document"
                    .to_owned(),
            );
        }
        let arranged = self.copies.is_some_and(|n| n > 1) || self.arrange;
        if self.spacing.is_some() && !arranged {
            return Err(
                "--spacing applies to arranged models; add --copies <n> or --arrange".to_owned(),
            );
        }
        if arranged && center {
            return Err(
                "--center places the model itself; --copies and --arrange place it by arrangement"
                    .to_owned(),
            );
        }
        Ok(arranged)
    }

    /// Consume `name` and its value if it is a plate option.
    fn apply(
        &mut self,
        name: &str,
        arguments: &mut impl Iterator<Item = OsString>,
    ) -> Result<bool, String> {
        match name {
            "--plate" => set(&mut self.plate, name, path_value(name, arguments.next())?)?,
            "--copies" => {
                let text = text_value(name, arguments.next())?;
                let copies = text
                    .parse::<u32>()
                    .ok()
                    .filter(|n| (1..=MAX_COPIES).contains(n))
                    .ok_or_else(|| {
                        format!(
                            "{name} must be a whole number from 1 to {MAX_COPIES}, not {text:?}"
                        )
                    })?;
                set(&mut self.copies, name, copies)?;
            }
            "--arrange" if !self.arrange => self.arrange = true,
            "--arrange" => return Err(format!("{name} may be given only once")),
            "--spacing" => {
                let text = text_value(name, arguments.next())?;
                let spacing = text
                    .parse::<f64>()
                    .ok()
                    .filter(|v| (0. ..=100.).contains(v))
                    .ok_or_else(|| {
                        format!("{name} must be a distance from 0 to 100 mm, not {text:?}")
                    })?;
                set(&mut self.spacing, name, spacing)?;
            }
            _ => return Ok(false),
        }
        Ok(true)
    }
}

/// The `--close-holes <mm>` / `--no-close-holes` choice.
#[derive(Clone, Copy)]
enum Closure {
    Bound(f64),
    Disabled,
}

impl Closure {
    /// The gap bound in mm, or `None` when closure is disabled.
    fn bound(self) -> Option<f64> {
        match self {
            Self::Bound(mm) => Some(mm),
            Self::Disabled => None,
        }
    }
}

/// Options selecting how the model becomes masks, each given at most once.
#[derive(Default)]
struct Processing {
    fill: Option<Fill>,
    antialias: Option<u8>,
    min_gray: Option<u8>,
    xy_compensation: Option<i16>,
    bottom_xy_compensation: Option<i16>,
    supports: Option<PathBuf>,
    accept_support_collisions: Option<bool>,
    antialias_supports: Option<bool>,
    layer_plan: Option<PathBuf>,
    progress: Option<ProgressMode>,
    close_holes: Option<Closure>,
    reversed_tolerance: Option<f64>,
    repair: Option<RepairSpec>,
    verify_walls: Option<bool>,
    unsupported: Option<bool>,
    overhang_angle: Option<f64>,
    suction_cups: Option<bool>,
    min_vent: Option<f64>,
    log: Option<PathBuf>,
}

impl Processing {
    /// Consume `name` and its value if it is a processing option.
    fn apply(
        &mut self,
        name: &str,
        arguments: &mut impl Iterator<Item = OsString>,
    ) -> Result<bool, String> {
        match name {
            "--fill" => set(&mut self.fill, name, fill_value(name, arguments.next())?)?,
            "--verify-walls" => set(&mut self.verify_walls, name, true)?,
            "--unsupported" => set(&mut self.unsupported, name, true)?,
            "--overhang-angle" => {
                let angle = arguments
                    .next()
                    .as_deref()
                    .and_then(OsStr::to_str)
                    .and_then(|v| v.parse::<f64>().ok())
                    .filter(|v| v.is_finite() && *v > 0.0 && *v < 90.0)
                    .ok_or("--overhang-angle requires an angle in degrees between 0 and 90")?;
                set(&mut self.overhang_angle, name, angle)?;
            }
            "--suction-cups" => set(&mut self.suction_cups, name, true)?,
            "--min-vent" => {
                let area = arguments
                    .next()
                    .as_deref()
                    .and_then(OsStr::to_str)
                    .and_then(|v| v.parse::<f64>().ok())
                    .filter(|v| v.is_finite() && *v >= 0.0)
                    .ok_or("--min-vent requires an area in mm^2, zero or more")?;
                set(&mut self.min_vent, name, area)?;
            }
            "--log" => set(&mut self.log, name, path_value(name, arguments.next())?)?,
            "--close-holes" | "--no-close-holes" => {
                let bound = close_holes_value(name, arguments)?;
                set(
                    &mut self.close_holes,
                    "--close-holes/--no-close-holes",
                    bound,
                )?;
            }
            "--reversed-tolerance" => set(
                &mut self.reversed_tolerance,
                name,
                positive_area(name, arguments.next().as_deref())?,
            )?,
            "--repair" => set(
                &mut self.repair,
                name,
                repair_value(name, arguments.next())?,
            )?,
            "--antialias" => set(
                &mut self.antialias,
                name,
                antialias_value(name, arguments.next())?,
            )?,
            "--min-gray" => set(
                &mut self.min_gray,
                name,
                min_gray_value(name, arguments.next())?,
            )?,
            "--progress" | "--no-progress" => {
                set(
                    &mut self.progress,
                    "--progress/--no-progress",
                    progress_mode(name),
                )?;
            }
            "--supports" => set(
                &mut self.supports,
                name,
                path_value(name, arguments.next())?,
            )?,
            "--accept-support-collisions" => set(&mut self.accept_support_collisions, name, true)?,
            "--antialias-supports" => set(&mut self.antialias_supports, name, true)?,
            "--layer-plan" => set(
                &mut self.layer_plan,
                name,
                path_value(name, arguments.next())?,
            )?,
            "--xy-compensation" => set(
                &mut self.xy_compensation,
                name,
                compensation_value(name, arguments.next())?,
            )?,
            "--bottom-xy-compensation" => set(
                &mut self.bottom_xy_compensation,
                name,
                compensation_value(name, arguments.next())?,
            )?,
            _ => return Ok(false),
        }
        Ok(true)
    }
}

/// `--surface-drain-hole <x> <y> <z> <diameter>`: perpendicular to the surface nearest the point.
fn surface_drain_hole(
    option: &str,
    arguments: &mut impl Iterator<Item = OsString>,
) -> Result<DrainHole, String> {
    let mut surface_mm = [0.0; 3];
    for (axis, value) in ["X", "Y", "Z"].into_iter().zip(&mut surface_mm) {
        *value = millimeters(option, axis, arguments.next().as_deref())?;
    }
    let diameter_mm = positive_mm(option, arguments.next().as_deref())?;
    Ok(DrainHole::Surface {
        surface_mm,
        diameter_mm,
        outside_mm: SURFACE_HOLE_MARGIN_MM,
        inside_mm: SURFACE_HOLE_MARGIN_MM,
        tilt: None,
        depth_mm: None,
        surface: HoleSurface::Outer,
    })
}

/// `--drain-hole <x> <y> <z> <axis> <length> <diameter>`, axis one of +x -x +y -y +z -z or a
/// direction `dx,dy,dz` (TD-066).
fn drain_hole(
    option: &str,
    arguments: &mut impl Iterator<Item = OsString>,
) -> Result<DrainHole, String> {
    let mut entry_mm = [0.0; 3];
    for (axis, value) in ["X", "Y", "Z"].into_iter().zip(&mut entry_mm) {
        *value = millimeters(option, axis, arguments.next().as_deref())?;
    }
    let token = arguments.next();
    let direction = match token.as_deref().and_then(OsStr::to_str) {
        Some("+x" | "x") => [1.0, 0.0, 0.0],
        Some("-x") => [-1.0, 0.0, 0.0],
        Some("+y" | "y") => [0.0, 1.0, 0.0],
        Some("-y") => [0.0, -1.0, 0.0],
        Some("+z" | "z") => [0.0, 0.0, 1.0],
        Some("-z") => [0.0, 0.0, -1.0],
        Some(text) if text.contains(',') => {
            let values: Vec<f64> = text
                .split(',')
                .filter_map(|v| v.trim().parse::<f64>().ok())
                .filter(|v| v.is_finite())
                .collect();
            match values[..] {
                [x, y, z] if [x, y, z].iter().any(|&c| c != 0.0) => [x, y, z],
                _ => {
                    return Err(format!(
                        "{option} direction must be three finite numbers dx,dy,dz, not all zero"
                    ))
                }
            }
        }
        _ => {
            return Err(format!(
                "{option} axis must be one of +x -x +y -y +z -z or a direction dx,dy,dz"
            ))
        }
    };
    let length_mm = positive_mm(option, arguments.next().as_deref())?;
    let diameter_mm = positive_mm(option, arguments.next().as_deref())?;
    Ok(DrainHole::Axis {
        entry_mm,
        direction,
        length_mm,
        diameter_mm,
    })
}

/// `--repair none|off|auto|exact|standard|aggressive|<list>`: `none` and `off` repair nothing,
/// `auto` is duplicates and orientation, `exact`, `standard` and `aggressive` the presets
/// (TD-092); the list names any of `duplicates`, `orientation`, `fill-holes`, `outward` (which
/// implies `orientation`), `weld[=mm]`, `degenerate[=mm]` (exact without a value), `cracks=mm`
/// (with `fill-holes`), `fins` or `cut-non-manifold`, and `non-finite` (accept dropping facets
/// without a position at import).
fn repair_value(option: &str, value: Option<OsString>) -> Result<RepairSpec, String> {
    let text = text_value(option, value)?;
    match text.as_str() {
        "none" | "off" => return Ok(RepairPreset::Off.spec()),
        "auto" => return Ok(DEFAULT_REPAIR),
        "exact" => return Ok(RepairPreset::Exact.spec()),
        "standard" => return Ok(RepairPreset::Standard.spec()),
        "aggressive" => return Ok(RepairPreset::Aggressive.spec()),
        "rebuild" => return Ok(RepairPreset::Rebuild.spec()),
        _ => {}
    }
    let mut spec = RepairSpec::default();
    let mut holes = HoleItems::default();
    for item in text.split(',') {
        // A rebuild resolution and shell bounds are sizes, not tolerances.
        if item == "rebuild" || item.starts_with("rebuild=") {
            rebuild_item(option, item, &mut spec)?;
            continue;
        }
        if let Some((name @ ("small-shells" | "small-volume"), value)) = item.split_once('=') {
            shell_bound(option, name, value, &mut spec)?;
            continue;
        }
        let (name, tolerance) = match item.split_once('=') {
            Some((name, value)) => {
                let tolerance = value
                    .parse::<f64>()
                    .ok()
                    .filter(|t| (0.0..=MAX_TOLERANCE_MM).contains(t))
                    .ok_or_else(|| {
                        format!(
                            "{option} gives {name} a tolerance from 0 to {MAX_TOLERANCE_MM} mm, \
                             not {value:?}"
                        )
                    })?;
                (name, Some(tolerance))
            }
            None => (item, None),
        };
        let repeated = match (name, tolerance) {
            ("weld", t) => spec.weld_mm.replace(t.unwrap_or(0.0)).is_some(),
            ("degenerate", t) => spec.degenerate_mm.replace(t.unwrap_or(0.0)).is_some(),
            ("cracks", Some(t)) => spec.crack_mm.replace(t).is_some(),
            (_, Some(_)) => {
                return Err(format!(
                    "{option} takes a tolerance only for weld, degenerate and cracks, not {item:?}"
                ))
            }
            _ => match item {
                "duplicates" => std::mem::replace(&mut spec.duplicates, true),
                "orientation" => std::mem::replace(&mut spec.orientation, true) && !spec.outward,
                "outward" => {
                    spec.orientation = true;
                    std::mem::replace(&mut spec.outward, true)
                }
                "fill-holes" => std::mem::replace(&mut holes.fill, true),
                "split-rims" => std::mem::replace(&mut holes.split, true),
                "refine-holes" => std::mem::replace(&mut holes.refine, true),
                "unbounded-holes" => std::mem::replace(&mut holes.unbounded, true),
                "fins" => spec.non_manifold.replace(NonManifold::Fins).is_some(),
                "visibility" => std::mem::replace(&mut spec.visibility, true),
                "non-finite" => std::mem::replace(&mut spec.non_finite, true),
                "inner-shells" => {
                    let shells = spec.shells.get_or_insert(Shells {
                        min_area_mm2: 0.0,
                        min_volume_mm3: 0.0,
                        inner: false,
                    });
                    std::mem::replace(&mut shells.inner, true)
                }
                "cut-non-manifold" => spec.non_manifold.replace(NonManifold::Aggressive).is_some(),
                _ => {
                    return Err(format!(
                    "{option} takes none, off, auto, exact, standard, aggressive or a comma list \
                     of duplicates, orientation, fill-holes (with split-rims, refine-holes, \
                     unbounded-holes), outward, weld[=mm], degenerate[=mm], cracks=mm, fins, \
                     cut-non-manifold, visibility, small-shells=mm2, small-volume=mm3, \
                     inner-shells, rebuild[=mm] and non-finite, not {text:?}"
                ))
                }
            },
        };
        if repeated {
            return Err(format!("{option} lists {name} twice"));
        }
    }
    holes.apply(option, &mut spec)?;
    spec.check()
        .map_err(|what| format!("{option}: {what} (cracks=mm needs fill-holes)"))?;
    Ok(spec)
}

/// `rebuild[=mm]` in a `--repair` list.
fn rebuild_item(option: &str, item: &str, spec: &mut RepairSpec) -> Result<(), String> {
    let resolution = match item.split_once('=') {
        Some((_, value)) => value
            .parse::<f64>()
            .ok()
            .filter(|r| (MIN_REBUILD_MM..=MAX_REBUILD_MM).contains(r))
            .ok_or_else(|| {
                format!(
                    "{option} gives rebuild a resolution from {MIN_REBUILD_MM} to \
                     {MAX_REBUILD_MM} mm, not {value:?}"
                )
            })?,
        None => DEFAULT_REBUILD_MM,
    };
    if spec.rebuild_mm.replace(resolution).is_some() {
        return Err(format!("{option} lists rebuild twice"));
    }
    Ok(())
}

/// `small-shells=<mm²>` or `small-volume=<mm³>` in a `--repair` list.
fn shell_bound(option: &str, name: &str, value: &str, spec: &mut RepairSpec) -> Result<(), String> {
    let bound = value
        .parse::<f64>()
        .ok()
        .filter(|b| b.is_finite() && *b >= 0.0)
        .ok_or_else(|| {
            format!("{option} gives {name} a finite bound of at least 0, not {value:?}")
        })?;
    let shells = spec.shells.get_or_insert(Shells {
        min_area_mm2: 0.0,
        min_volume_mm3: 0.0,
        inner: false,
    });
    let slot = if name == "small-shells" {
        &mut shells.min_area_mm2
    } else {
        &mut shells.min_volume_mm3
    };
    if std::mem::replace(slot, bound) != 0.0 {
        return Err(format!("{option} lists {name} twice"));
    }
    Ok(())
}

/// The hole-filling items of a `--repair` list.
#[derive(Default)]
#[allow(clippy::struct_excessive_bools)] // Independent list items.
struct HoleItems {
    fill: bool,
    split: bool,
    refine: bool,
    unbounded: bool,
}

impl HoleItems {
    /// Set `spec`'s hole filling from the items; the others need `fill-holes`.
    fn apply(&self, option: &str, spec: &mut RepairSpec) -> Result<(), String> {
        if self.fill {
            spec.fill_holes = Some(FillHoles {
                max_hole_edges: if self.unbounded {
                    usize::MAX
                } else {
                    FillHoles::default().max_hole_edges
                },
                split_rims: self.split,
                refine: self.refine,
            });
        } else if self.split || self.refine || self.unbounded {
            return Err(format!(
                "{option}: split-rims, refine-holes and unbounded-holes need fill-holes"
            ));
        }
        Ok(())
    }
}

/// `--progress` forces progress reports; `--no-progress` disables them.
pub(super) fn progress_mode(option: &str) -> ProgressMode {
    if option == "--progress" {
        ProgressMode::Always
    } else {
        ProgressMode::Never
    }
}

/// `--close-holes <mm>` gives a bound; `--no-close-holes` disables closure.
fn close_holes_value(
    option: &str,
    arguments: &mut impl Iterator<Item = OsString>,
) -> Result<Closure, String> {
    if option == "--no-close-holes" {
        return Ok(Closure::Disabled);
    }
    positive_mm(option, arguments.next().as_deref()).map(Closure::Bound)
}

fn antialias_value(option: &str, value: Option<OsString>) -> Result<u8, String> {
    let text = text_value(option, value)?;
    match text.parse::<u8>() {
        Ok(n @ (1 | 2 | 4 | 8 | 16)) => Ok(n),
        _ => Err(format!("{option} must be 1, 2, 4, 8 or 16, not {text:?}")),
    }
}

fn min_gray_value(option: &str, value: Option<OsString>) -> Result<u8, String> {
    let text = text_value(option, value)?;
    text.parse::<u8>()
        .map_err(|_| format!("{option} must be a gray level from 0 to 255, not {text:?}"))
}

/// Largest accepted compensation radius, in pixels.
const MAX_COMPENSATION_PX: i16 = 64;

fn compensation_value(option: &str, value: Option<OsString>) -> Result<i16, String> {
    // Negative values are expected here, so a leading '-' is part of the number.
    let text = value
        .and_then(|v| v.into_string().ok())
        .ok_or_else(|| format!("{option} requires a number of pixels"))?;
    text.parse::<i16>()
        .ok()
        .filter(|pixels| pixels.abs() <= MAX_COMPENSATION_PX)
        .ok_or_else(|| {
            format!(
                "{option} must be a whole number of pixels from -{MAX_COMPENSATION_PX} to \
                 {MAX_COMPENSATION_PX}, not {text:?}"
            )
        })
}

fn fill_value(option: &str, value: Option<OsString>) -> Result<Fill, String> {
    match text_value(option, value)?.as_str() {
        "union" => Ok(Fill::Union),
        "strict" => Ok(Fill::Strict),
        other => Err(format!("{option} must be union or strict, not {other:?}")),
    }
}

/// A finite positive area in mm² (its upper bound is the request's).
fn positive_area(option: &str, value: Option<&OsStr>) -> Result<f64, String> {
    value
        .and_then(OsStr::to_str)
        .and_then(|v| v.parse::<f64>().ok())
        .filter(|v| v.is_finite() && *v > 0.0)
        .ok_or_else(|| format!("{option} requires a finite positive area in mm^2"))
}

fn positive_mm(option: &str, value: Option<&OsStr>) -> Result<f64, String> {
    value
        .and_then(OsStr::to_str)
        .and_then(|v| v.parse::<f64>().ok())
        .filter(|v| v.is_finite() && *v > 0.0)
        .ok_or_else(|| format!("{option} requires a finite positive millimeter value"))
}

/// The model path and its unit choice, each given at most once.
#[derive(Default)]
struct ModelOptions {
    path: Option<PathBuf>,
    units: Option<LengthUnit>,
    legacy_stl: bool,
}

impl ModelOptions {
    /// Consume `name` if it is a model option; `Ok(false)` otherwise.
    fn apply(
        &mut self,
        name: &str,
        arguments: &mut impl Iterator<Item = OsString>,
    ) -> Result<bool, String> {
        match name {
            "--assume-mm" => {
                set(
                    &mut self.path,
                    "--assume-mm/--model",
                    path_value(name, arguments.next())?,
                )?;
                set(
                    &mut self.units,
                    "--assume-mm/--units",
                    LengthUnit::Millimeter,
                )?;
                self.legacy_stl = true;
            }
            "--model" => set(
                &mut self.path,
                "--assume-mm/--model",
                path_value(name, arguments.next())?,
            )?,
            "--units" => set(
                &mut self.units,
                "--assume-mm/--units",
                unit_value(name, arguments.next())?,
            )?,
            _ => return Ok(false),
        }
        Ok(true)
    }
}

/// A `--units` value: a symbol (`mm`, `cm`, `m`, `um`, `in`, `ft`) or a 3MF unit name.
pub(super) fn unit_value(option: &str, value: Option<OsString>) -> Result<LengthUnit, String> {
    let value = text_value(option, value)?;
    LengthUnit::parse(&value).ok_or_else(|| {
        let symbols: Vec<_> = LengthUnit::ALL.iter().map(|u| u.symbol()).collect();
        format!(
            "{option} must be one of {} (or a 3MF unit name), not '{value}'",
            symbols.join(", ")
        )
    })
}

fn required<T>(value: Option<T>, option: &str) -> Result<T, String> {
    value.ok_or_else(|| format!("slice requires {option}"))
}

pub(super) fn set<T>(slot: &mut Option<T>, option: &str, value: T) -> Result<(), String> {
    if slot.replace(value).is_some() {
        return Err(format!("{option} may be given only once"));
    }
    Ok(())
}

pub(super) fn path_value(option: &str, value: Option<OsString>) -> Result<PathBuf, String> {
    let value = value.ok_or_else(|| format!("{option} requires a path"))?;
    if value.is_empty() || value.to_string_lossy().starts_with('-') {
        return Err(format!(
            "{option} requires a nonempty path, not an option; prefix a leading '-' filename with './'"
        ));
    }
    Ok(value.into())
}

pub(super) fn text_value(option: &str, value: Option<OsString>) -> Result<String, String> {
    value
        .and_then(|v| v.into_string().ok())
        .filter(|v| !v.is_empty() && !v.starts_with('-'))
        .ok_or_else(|| format!("{option} requires a nonempty Unicode value"))
}

fn finite_value(
    option: &str,
    kind: &str,
    axis: &str,
    value: Option<&OsStr>,
) -> Result<f64, String> {
    value
        .and_then(OsStr::to_str)
        .and_then(|v| v.parse::<f64>().ok())
        .filter(|v| v.is_finite())
        .ok_or_else(|| format!("{option} requires three finite {kind} values; {axis} is invalid"))
}

fn millimeters(option: &str, axis: &str, value: Option<&OsStr>) -> Result<f64, String> {
    // Rust's shortest round-trip float text, as printed by `inspect`, parses back to the same value.
    value
        .and_then(OsStr::to_str)
        .and_then(|v| v.parse::<f64>().ok())
        .filter(|v| v.is_finite())
        .ok_or_else(|| {
            format!("{option} requires three finite millimeter values; {axis} is invalid")
        })
}

/// Run the complete pipeline and publish one create-only artifact; nothing partial is published.
///
/// A pipeline failure means nothing was published, since publishing is its last fallible step.
pub(super) fn slice(args: &SliceArgs, out: &mut impl Write) -> Result<(), SliceFailure> {
    if let Some(path) = &args.log {
        start_log(path)?;
    }
    let resolved;
    let args = match &args.print_setting {
        None => args,
        Some(path) => {
            resolved = SliceArgs {
                request: with_print_setting(&args.request, path).map_err(SliceFailure::Error)?,
                print_setting: None,
                ..args.clone()
            };
            &resolved
        }
    };
    let result = slice_logged(args, out);
    if let Err(SliceFailure::Error(message)) = &result {
        log::error!(target: "slice", "{message}");
    }
    log::logger().flush();
    result
}

/// Start the session log in `path` (TD-096), with the session's facts and the command line.
pub(super) fn start_log(path: &Path) -> Result<(), SliceFailure> {
    use karasu_project::logging::{install, Destination, LogConfig};
    let handle = install(LogConfig::new(Destination::File(path.to_owned())))
        .map_err(|e| SliceFailure::Error(e.to_string()))?;
    log::info!(
        target: "session",
        "{} {} on {} {}, {} threads, log {}",
        karasu_core::APPLICATION_NAME,
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
        std::thread::available_parallelism().map_or(1, usize::from),
        handle.path().map_or_else(String::new, |p| p.display().to_string())
    );
    let command: Vec<String> = std::env::args_os()
        .map(|a| format!("{:?}", a.to_string_lossy()))
        .collect();
    log::info!(target: "session", "command: {}", command.join(" "));
    Ok(())
}

fn slice_logged(args: &SliceArgs, out: &mut impl Write) -> Result<(), SliceFailure> {
    let mut progress = Progress::new(args.progress);
    if let Err(e) = ctrlc::set_handler(on_interrupt) {
        // Without a handler Ctrl-C still terminates the process, only without staging cleanup.
        progress.notice(&format!(
            "warning: Ctrl-C cancellation is unavailable ({e}); interrupting may leave a staging file"
        ));
    }
    let result = pipeline::slice(&args.request, &mut progress, &interrupted);
    progress.finish();
    match result {
        Ok(outcome) => {
            out.write_all(outcome.summary.as_bytes())
                .and_then(|()| out.flush())
                .map_err(|e| SliceFailure::from(format!("cannot write slice summary: {e}")))?;
            // TD-095: the output is kept for inspection, but the job fails.
            match outcome.verification.and_then(|v| v.failure) {
                Some(reason) => Err(SliceFailure::Error(format!(
                    "wall verification failed: {reason}; the output was written for inspection"
                ))),
                None => Ok(()),
            }
        }
        // A failure after an interrupt request is the interruption it caused.
        Err(_) if INTERRUPTED.load(Ordering::Acquire) => Err(SliceFailure::Interrupted),
        Err(e) => Err(SliceFailure::Error(e.message().to_owned())),
    }
}

pub(super) fn on_interrupt() {
    if INTERRUPTED.swap(true, Ordering::AcqRel) {
        std::process::exit(i32::from(INTERRUPTED_EXIT_STATUS));
    }
    let _ = writeln!(
        io::stderr().lock(),
        "\nInterrupt received: cancelling; press Ctrl-C again to exit immediately without cleanup"
    );
}

#[cfg(test)]
mod tests {
    use super::{
        parse, Fill, Input, ModelSource, ProgressMode, SliceArgs, SliceRequest, Subject,
        DEFAULT_JOB_ID,
    };
    use std::{ffi::OsString, path::PathBuf};

    fn arguments(values: &[&str]) -> impl Iterator<Item = OsString> {
        values
            .iter()
            .map(OsString::from)
            .collect::<Vec<_>>()
            .into_iter()
    }

    #[test]
    fn options_are_order_independent_with_explicit_defaults() {
        let parsed = parse(arguments(&[
            "--output",
            "o.goo",
            "--format",
            "goo-v3",
            "--resin",
            "r.json",
            "--printer",
            "p.json",
            "--assume-mm",
            "m.stl",
        ]))
        .unwrap();
        assert_eq!(
            parsed,
            SliceArgs {
                request: SliceRequest {
                    subject: Subject::Model(ModelSource::File(PathBuf::from("m.stl"))),
                    units: Some(karasu_core::mesh::formats::LengthUnit::Millimeter),
                    legacy_stl: true,
                    printer: Input::File(PathBuf::from("p.json")),
                    resin: Input::File(PathBuf::from("r.json")),
                    format: "goo-v3".into(),
                    output: PathBuf::from("o.goo"),
                    translate_mm: [0.; 3],
                    pose: super::Pose::default(),
                    job_id: DEFAULT_JOB_ID.into(),
                    allow_topology_defects: false,
                    fill: Fill::Union,
                    close_holes_mm: Some(2.0),
                    reversed_tolerance_mm2: None,
                    repair: super::DEFAULT_REPAIR,
                    antialias: 1,
                    min_gray: 0,
                    antialias_supports: false,
                    xy_compensation_px: 0,
                    bottom_xy_compensation_px: None,
                    hollow_mm: None,
                    hollow_cell_mm: None,
                    drain_holes: Vec::new(),
                    hole_coordinates: karasu_project::request::Coordinates::World,
                    hollow_simplify: None,
                    hollow_blockers: Vec::new(),
                    supports: None,
                    support_coordinates: karasu_project::request::Coordinates::World,
                    accept_support_collisions: false,
                    layer_plan: None,
                    copies: 1,
                    arrange: false,
                    spacing_mm: 2.0,
                    memory_budget_bytes: None,
                    verify_walls: false,
                    find_unsupported: None,
                    find_suction_cups: None,
                    slice_support_errors: false,
                    allow_floating: false,
                },
                progress: ProgressMode::Auto,
                log: None,
                print_setting: None,
            }
        );
    }

    #[test]
    fn progress_flags_are_explicit_and_mutually_exclusive() {
        let base = [
            "--assume-mm",
            "m.stl",
            "--printer",
            "p.json",
            "--resin",
            "r.json",
            "--format",
            "reference",
            "--output",
            "out",
        ];
        let with = |extra: &[&str]| parse(arguments(&[&base[..], extra].concat()));
        assert_eq!(
            with(&["--progress"]).unwrap().progress,
            ProgressMode::Always
        );
        assert_eq!(
            with(&["--no-progress"]).unwrap().progress,
            ProgressMode::Never
        );
        for conflict in [
            ["--progress", "--no-progress"],
            ["--progress", "--progress"],
        ] {
            assert!(with(&conflict).unwrap_err().contains("only once"));
        }
    }

    #[test]
    fn repair_defaults_to_duplicates_and_orientation() {
        use karasu_core::preparation::repair::{FillHoles, RepairSpec};
        let base = [
            "--assume-mm",
            "m.stl",
            "--printer",
            "p.json",
            "--resin",
            "r.json",
            "--format",
            "reference",
            "--output",
            "out",
        ];
        let with = |extra: &[&str]| parse(arguments(&[&base[..], extra].concat()));
        let spec = |d, o, w, fill: bool| RepairSpec {
            duplicates: d,
            orientation: o,
            outward: w,
            fill_holes: fill.then(FillHoles::default),
            allow_unfilled: false,
            weld_mm: None,
            degenerate_mm: None,
            crack_mm: None,
            non_manifold: None,
            visibility: false,
            shells: None,
            rebuild_mm: None,
            non_finite: false,
        };
        assert_eq!(
            with(&[]).unwrap().request.repair,
            spec(true, true, false, false)
        );
        assert_eq!(
            with(&["--repair", "auto"]).unwrap().request.repair,
            spec(true, true, false, false)
        );
        assert_eq!(
            with(&["--repair", "none"]).unwrap().request.repair,
            spec(false, false, false, false)
        );
        assert_eq!(
            with(&["--repair", "off"]).unwrap().request.repair,
            spec(false, false, false, false)
        );
        assert_eq!(
            with(&["--repair", "exact"]).unwrap().request.repair,
            RepairSpec {
                allow_unfilled: true,
                weld_mm: Some(0.0),
                degenerate_mm: Some(0.0),
                fill_holes: Some(FillHoles {
                    split_rims: true,
                    ..FillHoles::default()
                }),
                non_finite: true,
                ..spec(true, true, true, true)
            }
        );
        assert_eq!(
            with(&["--repair", "duplicates,non-finite"])
                .unwrap()
                .request
                .repair,
            RepairSpec {
                non_finite: true,
                ..spec(true, false, false, false)
            }
        );
        assert!(with(&["--repair", "non-finite,non-finite"]).is_err());
        assert_eq!(
            with(&["--repair", "duplicates,fill-holes"])
                .unwrap()
                .request
                .repair,
            spec(true, false, false, true)
        );
        assert_eq!(
            with(&["--repair", "duplicates,outward"])
                .unwrap()
                .request
                .repair,
            spec(true, true, true, false)
        );
        for bad in [
            "holes",
            "duplicates,duplicates",
            "fill-holes,fill-holes",
            "auto,outward",
            "exact,outward",
            "Exact",
            "",
        ] {
            assert!(with(&["--repair", bad]).is_err(), "{bad}");
        }
        assert!(with(&["--repair", "none", "--repair", "auto"])
            .unwrap_err()
            .contains("only once"));
    }

    #[test]
    fn repair_presets_and_tolerances_are_parsed_and_checked() {
        use karasu_core::preparation::repair::{FillHoles, NonManifold, RepairSpec};
        let base = [
            "--assume-mm",
            "m.stl",
            "--printer",
            "p.json",
            "--resin",
            "r.json",
            "--format",
            "reference",
            "--output",
            "out",
        ];
        let with = |extra: &[&str]| parse(arguments(&[&base[..], extra].concat()));
        let spec = |d, o, w, fill: bool| RepairSpec {
            duplicates: d,
            orientation: o,
            outward: w,
            fill_holes: fill.then(FillHoles::default),
            allow_unfilled: false,
            weld_mm: None,
            degenerate_mm: None,
            crack_mm: None,
            non_manifold: None,
            visibility: false,
            shells: None,
            rebuild_mm: None,
            non_finite: false,
        };
        assert_eq!(
            with(&["--repair", "standard"]).unwrap().request.repair,
            RepairSpec {
                allow_unfilled: true,
                weld_mm: Some(0.001),
                degenerate_mm: Some(0.001),
                crack_mm: Some(0.001),
                non_manifold: Some(NonManifold::Fins),
                fill_holes: Some(FillHoles {
                    split_rims: true,
                    refine: true,
                    ..FillHoles::default()
                }),
                non_finite: true,
                ..spec(true, true, true, true)
            }
        );
        assert_eq!(
            with(&["--repair", "aggressive"])
                .unwrap()
                .request
                .repair
                .non_manifold,
            Some(NonManifold::Aggressive)
        );
        assert_eq!(
            with(&["--repair", "duplicates,cut-non-manifold"])
                .unwrap()
                .request
                .repair,
            RepairSpec {
                non_manifold: Some(NonManifold::Aggressive),
                ..spec(true, false, false, false)
            }
        );
        assert_eq!(
            with(&["--repair", "weld=0.002,degenerate,fill-holes,cracks=0.01"])
                .unwrap()
                .request
                .repair,
            RepairSpec {
                weld_mm: Some(0.002),
                degenerate_mm: Some(0.0),
                crack_mm: Some(0.01),
                ..spec(false, false, false, true)
            }
        );
        for bad in [
            "weld=-1",
            "weld=2",
            "weld=nan",
            "weld,weld=0.1",
            "cracks=0.001",
            "cracks",
            "duplicates=0.1",
            "fins,cut-non-manifold",
            "fins=1",
        ] {
            assert!(with(&["--repair", bad]).is_err(), "{bad}");
        }
    }

    #[test]
    fn hole_filling_items_need_fill_holes() {
        use karasu_core::preparation::repair::FillHoles;
        let base = [
            "--assume-mm",
            "m.stl",
            "--printer",
            "p.json",
            "--resin",
            "r.json",
            "--format",
            "reference",
            "--output",
            "out",
        ];
        let with = |extra: &[&str]| parse(arguments(&[&base[..], extra].concat()));
        let items = "fill-holes,split-rims,refine-holes,unbounded-holes";
        assert_eq!(
            with(&["--repair", items])
                .unwrap()
                .request
                .repair
                .fill_holes,
            Some(FillHoles {
                max_hole_edges: usize::MAX,
                split_rims: true,
                refine: true,
            })
        );
        let shells = with(&[
            "--repair",
            "small-shells=2.5,small-volume=0.5,inner-shells,visibility",
        ])
        .unwrap()
        .request
        .repair;
        assert!(shells.visibility);
        assert_eq!(
            shells.shells,
            Some(karasu_core::preparation::repair::Shells {
                min_area_mm2: 2.5,
                min_volume_mm3: 0.5,
                inner: true,
            })
        );
        assert_eq!(
            with(&["--repair", "rebuild"])
                .unwrap()
                .request
                .repair
                .rebuild_mm,
            Some(0.05)
        );
        assert_eq!(
            with(&["--repair", "duplicates,rebuild=0.2"])
                .unwrap()
                .request
                .repair
                .rebuild_mm,
            Some(0.2)
        );
        for bad in [
            "refine-holes",
            "split-rims,fill-holes,split-rims",
            "rebuild=0",
            "rebuild=20",
            "rebuild,rebuild=0.1",
            "small-shells=-1",
            "small-shells=1,small-shells=2",
            "visibility,visibility",
        ] {
            assert!(with(&["--repair", bad]).is_err(), "{bad}");
        }
    }

    #[test]
    fn hole_closure_defaults_to_two_millimetres_and_can_be_disabled() {
        let base = [
            "--assume-mm",
            "m.stl",
            "--printer",
            "p.json",
            "--resin",
            "r.json",
            "--format",
            "reference",
            "--output",
            "out",
        ];
        let with = |extra: &[&str]| parse(arguments(&[&base[..], extra].concat()));
        assert_eq!(with(&[]).unwrap().request.close_holes_mm, Some(2.0));
        assert_eq!(
            with(&["--close-holes", "0.5"])
                .unwrap()
                .request
                .close_holes_mm,
            Some(0.5)
        );
        assert_eq!(
            with(&["--no-close-holes"]).unwrap().request.close_holes_mm,
            None
        );
        for bad in ["0", "-1", "inf", "x"] {
            assert!(with(&["--close-holes", bad]).is_err(), "{bad}");
        }
        assert!(with(&["--close-holes", "1", "--no-close-holes"])
            .unwrap_err()
            .contains("only once"));
        // The reversed-sliver tolerance has no default and is bounded (TD-079).
        assert_eq!(with(&[]).unwrap().request.reversed_tolerance_mm2, None);
        assert_eq!(
            with(&["--reversed-tolerance", "0.25"])
                .unwrap()
                .request
                .reversed_tolerance_mm2,
            Some(0.25)
        );
        for bad in ["0", "-1", "inf", "x", "101"] {
            assert!(with(&["--reversed-tolerance", bad]).is_err(), "{bad}");
        }
        assert!(with(&["--reversed-tolerance", "1", "--fill", "strict"])
            .unwrap_err()
            .contains("--fill union"));
    }

    #[test]
    fn antialias_accepts_supported_levels_once() {
        let base = [
            "--assume-mm",
            "m.stl",
            "--printer",
            "p.json",
            "--resin",
            "r.json",
            "--format",
            "goo-v3",
            "--output",
            "o.goo",
        ];
        let with = |extra: &[&str]| parse(arguments(&[&base[..], extra].concat()));
        assert_eq!(with(&[]).unwrap().request.antialias, 1);
        for level in [1u8, 2, 4, 8, 16] {
            let text = level.to_string();
            assert_eq!(
                with(&["--antialias", &text]).unwrap().request.antialias,
                level
            );
        }
        for bad in ["0", "3", "32", "x"] {
            assert!(with(&["--antialias", bad])
                .unwrap_err()
                .contains("1, 2, 4, 8 or 16"));
        }
        assert!(with(&["--antialias", "2", "--antialias", "4"])
            .unwrap_err()
            .contains("only once"));
    }

    #[test]
    fn min_gray_needs_antialiasing_and_a_gray_level() {
        let base = [
            "--assume-mm",
            "m.stl",
            "--printer",
            "p.json",
            "--resin",
            "r.json",
            "--format",
            "goo-v3",
            "--output",
            "o.goo",
        ];
        let with = |extra: &[&str]| parse(arguments(&[&base[..], extra].concat()));
        assert_eq!(with(&[]).unwrap().request.min_gray, 0);
        assert_eq!(with(&["--min-gray", "0"]).unwrap().request.min_gray, 0);
        for level in [1u8, 77, 255] {
            let text = level.to_string();
            let args = with(&["--antialias", "4", "--min-gray", &text]).unwrap();
            assert_eq!(args.request.min_gray, level);
        }
        // A leading '-' reads as a missing value.
        assert!(with(&["--antialias", "4", "--min-gray", "-1"]).is_err());
        for bad in ["256", "30%", "x"] {
            assert!(with(&["--antialias", "4", "--min-gray", bad])
                .unwrap_err()
                .contains("from 0 to 255"));
        }
        assert!(with(&["--min-gray", "50"])
            .unwrap_err()
            .contains("requires --antialias"));
        assert!(
            with(&["--antialias", "4", "--min-gray", "1", "--min-gray", "2"])
                .unwrap_err()
                .contains("only once")
        );
    }

    #[test]
    fn fill_defaults_to_union_and_accepts_strict_once() {
        let base = [
            "--assume-mm",
            "m.stl",
            "--printer",
            "p.json",
            "--resin",
            "r.json",
            "--format",
            "reference",
            "--output",
            "out",
        ];
        let with = |extra: &[&str]| parse(arguments(&[&base[..], extra].concat()));
        assert_eq!(with(&[]).unwrap().request.fill, Fill::Union);
        assert_eq!(
            with(&["--fill", "union"]).unwrap().request.fill,
            Fill::Union
        );
        assert_eq!(
            with(&["--fill", "strict"]).unwrap().request.fill,
            Fill::Strict
        );
        assert!(with(&["--fill", "evenodd"])
            .unwrap_err()
            .contains("union or strict"));
        assert!(with(&["--fill", "union", "--fill", "strict"])
            .unwrap_err()
            .contains("only once"));
    }

    #[test]
    fn translation_round_trips_shortest_float_text_and_rejects_non_finite_values() {
        let base = [
            "--assume-mm",
            "m.stl",
            "--printer",
            "p.json",
            "--resin",
            "r.json",
            "--format",
            "reference",
            "--output",
            "out",
            "--translate",
        ];
        let with = |xyz: [&str; 3]| parse(arguments(&[&base[..], &xyz[..]].concat()));
        let value = 0.1_f64 + 0.2;
        let text = (-value).to_string();
        assert_eq!(
            with(["1.5", "-2", &text]).unwrap().request.translate_mm,
            [1.5, -2., -value]
        );
        for bad in ["inf", "-inf", "NaN", "1e400", "x"] {
            assert!(with(["0", "0", bad]).unwrap_err().contains("Z is invalid"));
        }
    }
}
