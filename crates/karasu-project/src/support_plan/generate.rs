//! The Automatic tool's Generate and Apply without the desktop application (TD-157), for the
//! one model of a slicing request: what the command line's `--auto-supports` runs.
//!
//! **As Generate does.** [`generate`] lifts the model to the plan's lift first (TD-128); prepares
//! it as a preview does (its hollowed shell and drain holes) and grids its surface for routes;
//! slices it alone, without its supports, in the job's format, into a file of the scratch folder
//! that is removed whatever happens ([`alone`]); tells the regions and islands found
//! ([`super::found`]); and plans with [`super::auto::plan`] from the input the application
//! builds ([`super::auto::input_for`]). Apply's document follows ([`super::auto::with_planned`]):
//! the given supports kept, the automatic ones of the place covered replaced, the planned ones
//! and their braces added.
//!
//! **The model as the application holds it.** The planners place the model imported for display
//! (`import_mesh`, without the job's repairs) by the request's pose, as the application places
//! its models; the slice places the repaired model, which the exact repairs leave where it was.
//!
//! **Frames.** The application keeps a model's supports in its model frame (TD-108), and so do
//! the planners: a document given in world coordinates (the command line's `--supports`) is
//! taken to the model frame for the plan, with the binding and motion the job would apply. The
//! result is given in both frames; the world document, placed as the job places the model,
//! keeps the given supports exactly as the job would build them, and is what the job slices.
//!
//! **Determinism.** Equal requests, values and files give equal documents: the analysis slice,
//! the preparation and the plan are deterministic, and nothing depends on the thread count.
use super::{
    auto::{self, input_for, with_planned, AutoOutcome, AutoSupports},
    braces::NewBraces,
    found::found_of,
    islands::check::ModelInput,
    route::{axis_to_model, RouteSurface},
    settings::AutoSettings,
};
use crate::{
    pipeline::{
        self, plan_world_braces, prepare_with, slice_with, ObjectPreparation, Observer,
        PipelineCache, Preparation,
    },
    placement::{self, plate_point, Affine, Placement},
    request::{format_extension, Coordinates, Input, SliceRequest, Subject, REFERENCE_FORMAT},
};
use karasu_core::{
    cancellation::{Cancellation, Checkpoint},
    geometry::ObjectTransform,
    supports::{BaseSpec, BuildVolume, ManualSupport, SupportDocument},
};
use std::{
    cell::RefCell,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::Instant,
};

/// How far a model's lift may differ from the plan's before Generate lifts it, in mm (TD-128).
pub const LIFT_TOLERANCE_MM: f64 = 1e-6;

/// The name a job's messages give the document [`generate`] and [`given`] make.
const DOCUMENT_NAME: &str = "automatic supports";

/// What a run reports to and is stopped by, and where its slice of the model alone goes.
pub struct Run<'a> {
    /// Cavities and models kept between the runs of one job (TD-089): the preparation, the slice
    /// of the model alone and the job's own slice then hollow a model once.
    pub cache: Option<&'a PipelineCache>,
    /// The folder the slice of the model alone is written to, and removed from.
    pub scratch: &'a Path,
    /// Where progress goes: the stages, prefixed `Automatic supports: `, and the slice's layers.
    pub observer: &'a mut dyn Observer,
    /// The caller's cancellation flag, observed at every checkpoint (the command line's Ctrl-C).
    pub stop: &'a AtomicBool,
}

/// Automatic supports planned for the model of a request.
pub struct Generated {
    /// The request with the model where it was planned (lifted) and [`Self::document`] as its
    /// supports, in world coordinates; nothing else changed.
    pub request: SliceRequest,
    /// The support document in world coordinates at the job's placement: the given supports as
    /// the job builds them, without the automatic ones the plan replaced, with the planned
    /// supports and braces added. `None` when it holds nothing (no support and no raft).
    pub document: Option<SupportDocument>,
    /// The same document in the model's frame, as the application's Apply stores it.
    pub model_document: Option<SupportDocument>,
    /// What the plan made, covered and left.
    pub outcome: AutoOutcome,
    /// The values planned with: a model that is not hollowed is planned everywhere.
    pub values: AutoSupports,
    /// The critical angle planned at, in degrees from the plate.
    pub angle_deg: f64,
    /// The height of the model's lowest point above the plate when planned, in mm.
    pub lift_mm: f64,
    /// The supports given with the request that the document keeps.
    pub kept: usize,
    /// The automatic supports given with the request that the plan replaced.
    pub replaced: usize,
}

/// The supports a request gives, as its job builds them, with the braces planned between them.
pub struct Given {
    /// The request with [`Self::document`] as its supports when braces were added; otherwise as
    /// it was.
    pub request: SliceRequest,
    /// The support document in world coordinates at the job's placement, as the job builds it
    /// (a binding checked and its motion applied), with the braces added.
    pub document: SupportDocument,
    /// The braces added.
    pub braces: usize,
}

/// Plan the automatic supports of `request`'s model as the Automatic tool's Generate plans them
/// with `settings`, and make Apply's document: see the module. `request` slices one model,
/// placed by its own options and dropped; its supports, if any, are kept around the planned
/// ones, those planned automatically before in the place covered replaced.
///
/// # Errors
/// A request of a plate or of copies, unreadable inputs, a model that cannot be imported, placed,
/// prepared or sliced, given supports that cannot be built or taken to the model's frame, a
/// placement without an inverse, and cancellation.
#[allow(clippy::too_many_lines)] // Generate's stages, in order.
pub fn generate(
    request: &SliceRequest,
    settings: &AutoSettings,
    run: Run<'_>,
) -> Result<Generated, String> {
    let Run {
        cache,
        scratch,
        observer,
        stop,
    } = run;
    let halt = |_: Checkpoint| stop.load(Ordering::Relaxed);
    let stopped = || stop.load(Ordering::Relaxed);
    one_model(request)?;
    let printer = pipeline::read_printer(&request.printer)?;
    let mut observer = Prefixed(observer);
    observer.phase("Importing model");
    let mesh = Arc::new(pipeline::import_request_mesh(request, &halt)?);
    let centre = placement::plate_centre(&printer);
    let place = |request: &SliceRequest| {
        let mut predicate = |checkpoint| halt(checkpoint);
        placement::place(
            &mesh,
            request.translate_mm,
            &request.pose,
            centre,
            &mut Cancellation::new(&mut predicate),
        )
    };
    // The lift first, as Generate lifts the model (TD-128).
    let mut request = request.clone();
    let mut placed = place(&request)?;
    let lift_mm = settings.values.lift_mm;
    if (lift_of(&request, &placed) - lift_mm).abs() > LIFT_TOLERANCE_MM {
        request.pose.rest_at(lift_mm);
        placed = place(&request)?;
    }
    let preparation = Arc::new(
        prepare_with(&request, cache, &[], &mut observer, &halt)
            .map_err(|e| e.message().to_owned())?,
    );
    let prepared = prepared(&preparation)?;
    let to_world = placement::affine(&placed).ok_or("the model's placement has no affine form")?;
    let job_placement = *prepared.placement();
    let job_to_world = transform_affine(&job_placement)?;
    // The given supports: as the job builds them, and in the model's frame for the plan.
    let given_world = prepared.support_document().cloned();
    let given_model = match (&request.supports, request.support_coordinates) {
        (Some(input), Coordinates::Model) => Some(pipeline::read_supports(input)?),
        (Some(_), Coordinates::World) => given_world
            .as_ref()
            .map(|d| model_frame(d, &job_to_world))
            .transpose()?,
        (None, _) => None,
    };
    observer.phase("Gridding the model's surface");
    let mut predicate = |checkpoint| halt(checkpoint);
    let surface = RouteSurface::build(
        &mesh,
        &placed.transform,
        Arc::clone(&preparation),
        0,
        &mut Cancellation::new(&mut predicate),
    )?;
    let surface = match surface {
        Some(surface) => Arc::new(surface),
        None if stopped() => return Err(cancelled()),
        None => return Err("the model's surface could not be gridded for routes".to_owned()),
    };
    let hollowed = request.hollow_mm.is_some();
    let model = ModelInput {
        mesh: Arc::clone(&mesh),
        placement: placed.transform,
        preparation: Arc::clone(&preparation),
        index: 0,
        printer: printer.clone(),
        braces: settings.braces.plan(),
    };
    let volume = BuildVolume {
        origin_xy_mm: printer.origin_xy_mm,
        size_mm: printer.build_size_mm,
    };
    let (mut input, replaced) = input_for(
        surface,
        given_model.as_ref(),
        &to_world,
        hollowed,
        (settings.values, &settings.presets),
        settings.angle_deg,
        volume,
        Some(model),
    )
    .ok_or("the model's placement cannot be inverted")?;
    // The model sliced alone, in the job's format, into a file removed whatever happens.
    let output = scratch_output(scratch, &request.format);
    let alone_request = alone(
        request.clone(),
        0,
        output.clone(),
        request.format.clone(),
        settings.angle_deg,
    )
    .ok_or("the request has no single model to slice")?;
    observer.phase("Finding islands and overhangs");
    let sliced = slice_with(&alone_request, cache, &mut observer, &halt);
    remove_output(&output);
    let sliced = sliced.map_err(|e| {
        if stopped() {
            cancelled()
        } else {
            format!("the model sliced alone: {}", e.message())
        }
    })?;
    let found = sliced
        .unsupported
        .ok_or("the unsupported regions were not found")?;
    let shell = hollowed.then(|| Arc::clone(&preparation));
    (input.regions, input.islands) = found_of(&found, shell.as_deref(), 0, stop);
    // The plan, its stages told as the application's footer tells them.
    let started = Instant::now();
    let observer = RefCell::new(observer);
    let stage = |k: u32| {
        observer.borrow_mut().phase(match k {
            0 => "Placing the tips",
            1 => "Growing the supports",
            2 => "Closing check",
            3 => "Final check",
            _ => "Planning braces",
        });
    };
    let outcome = auto::plan(&input, &stage, stop);
    if stopped() {
        return Err(cancelled());
    }
    log::info!(
        target: "supports",
        "planned {} automatic supports ({} left unsupported) in {:.2} s",
        outcome.plan.supports.len(),
        outcome.left.len(),
        started.elapsed().as_secs_f64()
    );
    // Apply: the kept supports, without those replaced, with the planned ones.
    let kept_model = given_model.as_ref().map(|d| auto::without(d, &replaced));
    let model_document = Some(with_planned(kept_model.as_ref(), &input.to_model, &outcome))
        .filter(|d| !d.supports.is_empty() || d.raft.is_some());
    let document = model_document
        .as_ref()
        .map(|d| world_document(d, given_world.as_ref(), &job_placement))
        .transpose()?;
    let kept = kept_model.as_ref().map_or(0, |d| d.supports.len());
    request.supports = document.clone().map(|value| Input::Loaded {
        name: DOCUMENT_NAME.to_owned(),
        value,
    });
    request.support_coordinates = Coordinates::World;
    Ok(Generated {
        lift_mm: lift_of(&request, &placed),
        request,
        document,
        model_document,
        values: input.values,
        angle_deg: settings.angle_deg,
        outcome,
        kept,
        replaced: replaced.len(),
    })
}

/// The supports `request` gives, as its job builds them, with the automatic braces `braces`
/// plan between them when given, as the Braces tool's automatic braces are planned (only the
/// braces kept, none skipped). `request` slices one model with supports.
///
/// # Errors
/// A request of a plate or of copies, or without supports; unreadable inputs, a model that
/// cannot be imported, placed or prepared, supports that cannot be built, and cancellation.
pub fn given(
    request: &SliceRequest,
    braces: Option<&NewBraces>,
    run: Run<'_>,
) -> Result<Given, String> {
    let Run {
        cache,
        observer,
        stop,
        ..
    } = run;
    let halt = |_: Checkpoint| stop.load(Ordering::Relaxed);
    one_model(request)?;
    if request.supports.is_none() {
        return Err("the request gives no supports".to_owned());
    }
    let mut observer = Prefixed(observer);
    let preparation = prepare_with(request, cache, &[], &mut observer, &halt)
        .map_err(|e| e.message().to_owned())?;
    let prepared = prepared(&preparation)?;
    let mut document = prepared
        .support_document()
        .cloned()
        .ok_or("the supports were not built")?;
    let mut request = request.clone();
    let Some(braces) = braces else {
        return Ok(Given {
            request,
            document,
            braces: 0,
        });
    };
    // Planned against the surface the application plans them against: the model as imported,
    // placed by the request's pose, or its hollowed shell.
    let printer = pipeline::read_printer(&request.printer)?;
    let mesh = pipeline::import_request_mesh(&request, &halt)?;
    let mut predicate = |checkpoint| halt(checkpoint);
    let placed = placement::place(
        &mesh,
        request.translate_mm,
        &request.pose,
        placement::plate_centre(&printer),
        &mut Cancellation::new(&mut predicate),
    )?;
    observer.phase("Planning braces");
    let planned = plan_world_braces(
        &document,
        &mesh,
        &placed.transform,
        prepared,
        &braces.plan(),
        &halt,
    )?;
    let added: Vec<_> = planned
        .into_iter()
        .filter(|p| p.skipped.is_none())
        .map(|p| p.brace)
        .collect();
    let count = added.len();
    if count > 0 {
        document.braces.extend(added);
        document = document.validated().map_err(|e| e.to_string())?;
        request.supports = Some(Input::Loaded {
            name: DOCUMENT_NAME.to_owned(),
            value: document.clone(),
        });
        request.support_coordinates = Coordinates::World;
    }
    Ok(Given {
        request,
        document,
        braces: count,
    })
}

/// The request that slices object `index` of `request` alone, without its supports, into
/// `output` in `format`, finding its unsupported regions at `angle_deg`: a lifted model is
/// sliced where it stands, for the analysis only. Wall verification and suction cups are not
/// asked for. `None` without that object, and for a plate file, whose objects are not read here.
#[must_use]
pub fn alone(
    mut request: SliceRequest,
    index: usize,
    output: PathBuf,
    format: String,
    angle_deg: f64,
) -> Option<SliceRequest> {
    match &mut request.subject {
        Subject::Plate(plate) => {
            if index >= plate.objects.len() {
                return None;
            }
            let mut object = plate.objects.swap_remove(index);
            object.supports = None;
            plate.objects = vec![object];
        }
        Subject::Model(_) if index == 0 => request.supports = None,
        Subject::Model(_) | Subject::PlateFile(_) => return None,
    }
    request.output = output;
    request.allow_floating = true;
    request.format = format;
    request.find_unsupported = Some(angle_deg);
    request.verify_walls = false;
    request.find_suction_cups = None;
    Some(request)
}

/// `document`, in world coordinates at a placement `to_world` carries the model frame by, in the
/// model's frame (TD-108): tips, bends, feet on the model and explicit axes carried back, a foot
/// on the plate taken to the model frame's point at Z 0 that the placement sets there. Placing
/// the result gives the document back, to rounding. Unbound, as a model-frame document is.
///
/// # Errors
/// A placement without an inverse, and a foot on the plate under a placement that stands the
/// model frame's plane on edge, where no such point exists.
pub fn model_frame(
    document: &SupportDocument,
    to_world: &Affine,
) -> Result<SupportDocument, String> {
    let to_model = to_world
        .inverse()
        .ok_or("the model's placement cannot be inverted")?;
    let supports = document
        .supports
        .iter()
        .map(|s| {
            let base = match s.base {
                BaseSpec::Platform(xy) => {
                    BaseSpec::Platform(plate_point(to_world, xy).ok_or_else(|| {
                        format!(
                            "support {} stands on the plate at {xy:?}, which the model's frame \
                             cannot hold under this placement (its plane stands on edge)",
                            s.id
                        )
                    })?)
                }
                BaseSpec::Model { point_mm, axis } => BaseSpec::Model {
                    point_mm: to_model.point(point_mm),
                    axis: axis_to_model(axis, &to_model),
                },
                other @ (BaseSpec::Vertical | BaseSpec::Support { .. }) => other,
            };
            Ok(ManualSupport {
                tip_mm: to_model.point(s.tip_mm),
                tip_axis: axis_to_model(s.tip_axis, &to_model),
                bends: s.bends.iter().map(|&b| to_model.point(b)).collect(),
                base,
                ..s.clone()
            })
        })
        .collect::<Result<_, String>>()?;
    Ok(SupportDocument {
        binding: None,
        supports,
        ..document.clone()
    })
}

/// `document`, in the model frame, placed as the job places the model (`placement`), with the
/// supports of `given` (world coordinates, as the job builds them) kept exactly as given: placing
/// their model-frame form back would move them by rounding. Checked as a document read is.
fn world_document(
    document: &SupportDocument,
    given: Option<&SupportDocument>,
    placement: &ObjectTransform,
) -> Result<SupportDocument, String> {
    let affine = placement
        .to_affine()
        .map_err(|e| format!("cannot place the supports: {e}"))?;
    let mut world = document
        .placed(&affine)
        .map_err(|e| format!("cannot place the supports: {e}"))?;
    if let Some(given) = given {
        for support in &mut world.supports {
            if let Some(kept) = given.supports.iter().find(|g| g.id == support.id) {
                support.clone_from(kept);
            }
        }
    }
    world
        .validated()
        .map_err(|e| format!("the automatic supports: {e}"))
}

/// The height of the model's lowest point above the plate: its lift when it rests at one, else
/// where it was put.
fn lift_of(request: &SliceRequest, placed: &Placement) -> f64 {
    if request.pose.drop {
        request.pose.lift_mm.unwrap_or(0.0)
    } else {
        placed.min.coordinates()[2].max(0.0)
    }
}

/// Refuse a request that is not one model printed once.
fn one_model(request: &SliceRequest) -> Result<(), String> {
    if request.model().is_none() {
        return Err("automatic supports are planned for one model, not a plate".to_owned());
    }
    if request.copies != 1 || request.arrange {
        return Err(
            "automatic supports are planned for one model where it stands, not for arranged \
             copies"
                .to_owned(),
        );
    }
    Ok(())
}

/// The one object of a single-model preparation.
fn prepared(preparation: &Preparation) -> Result<&ObjectPreparation, String> {
    preparation
        .objects
        .first()
        .ok_or("the request prepared no model")?
        .as_ref()
        .map_err(|e| e.message().to_owned())
}

/// A placement's map from the model frame to the world.
fn transform_affine(placement: &ObjectTransform) -> Result<Affine, String> {
    let affine = placement
        .to_affine()
        .map_err(|e| format!("the model's placement has no affine form: {e}"))?;
    Ok(Affine {
        linear: affine.linear(),
        translation: affine.translation().components(),
    })
}

/// A path in `scratch` no other run of this process or another uses, for the slice of the model
/// alone in `format`: a file of the format's extension, or a reference directory.
fn scratch_output(scratch: &Path, format: &str) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let name = format!(
        "karasu-automatic-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    );
    scratch.join(match format_extension(format) {
        Some(extension) => format!("{name}.{extension}"),
        None => format!("{name}-{REFERENCE_FORMAT}"),
    })
}

/// Remove the slice of the model alone, a file or a reference directory, if it was written.
fn remove_output(output: &Path) {
    if output.is_dir() {
        let _ = std::fs::remove_dir_all(output);
    } else {
        let _ = std::fs::remove_file(output);
    }
}

/// Why a run stopped at the caller's request.
fn cancelled() -> String {
    "automatic supports cancelled".to_owned()
}

/// An observer that tells its stages as the automatic supports' own.
struct Prefixed<'a>(&'a mut dyn Observer);

impl Observer for Prefixed<'_> {
    fn phase(&mut self, name: &str) {
        self.0.phase(&format!("Automatic supports: {name}"));
    }

    fn begin_layers(&mut self, total: usize) {
        self.0.begin_layers(total);
    }

    fn layers(&mut self, completed: usize) {
        self.0.layers(completed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use karasu_core::supports::{AxisSpec, Preset, SupportKind, SupportOrigin};

    fn support(id: u64, tip: [f64; 3], base: BaseSpec) -> ManualSupport {
        ManualSupport {
            id,
            tip_mm: tip,
            tip_axis: AxisSpec::Direction([0.2, 0.0, -1.0]),
            bends: vec![[tip[0] + 1.0, tip[1], tip[2] - 2.0]],
            base,
            parameters: Preset::Medium.parameters(),
            accept_collision: false,
            origin: SupportOrigin::Placed,
            kind: SupportKind::Standard,
        }
    }

    #[test]
    fn a_world_document_comes_back_from_the_model_frame_it_is_taken_to() {
        // A placement turned 30 degrees about X and 20 about Z, scaled and moved: the model
        // frame's document placed again is the world one, to rounding.
        let (sx, cx) = 30f64.to_radians().sin_cos();
        let (sz, cz) = 20f64.to_radians().sin_cos();
        let to_world = Affine {
            linear: [
                [1.5 * cz, -sz * cx, sz * sx],
                [1.5 * sz, cz * cx, -cz * sx],
                [0.0, sx, cx],
            ],
            translation: [40.0, 25.0, 6.0],
        };
        let mut world = SupportDocument::new(Preset::Medium);
        world.supports = vec![
            support(1, [41.0, 26.0, 7.0], BaseSpec::Platform([44.0, 27.0])),
            support(
                2,
                [43.0, 24.0, 9.0],
                BaseSpec::Model {
                    point_mm: [43.5, 24.0, 6.5],
                    axis: AxisSpec::Normal,
                },
            ),
            support(3, [42.0, 25.0, 8.0], BaseSpec::Vertical),
        ];
        let model = model_frame(&world, &to_world).unwrap();
        let placement = karasu_core::geometry::Affine3::from_parts(
            to_world.linear,
            karasu_core::geometry::Vector3::new(40.0, 25.0, 6.0),
        )
        .unwrap();
        let back = model.placed(&placement).unwrap();
        let near = |a: [f64; 3], b: [f64; 3]| (0..3).all(|k| (a[k] - b[k]).abs() < 1e-9);
        for (before, after) in world.supports.iter().zip(&back.supports) {
            assert!(near(before.tip_mm, after.tip_mm), "{after:?}");
            assert!(near(before.bends[0], after.bends[0]), "{after:?}");
            match (before.base, after.base) {
                (BaseSpec::Platform(a), BaseSpec::Platform(b)) => {
                    assert!(near([a[0], a[1], 0.0], [b[0], b[1], 0.0]), "{b:?}");
                }
                (BaseSpec::Model { point_mm: a, .. }, BaseSpec::Model { point_mm: b, .. }) => {
                    assert!(near(a, b), "{b:?}");
                }
                (a, b) => assert_eq!(a, b),
            }
            let (AxisSpec::Direction(a), AxisSpec::Direction(b)) =
                (before.tip_axis, after.tip_axis)
            else {
                panic!("{:?}", after.tip_axis);
            };
            assert!(near(a, b), "{b:?}");
        }
        // A frame stood on edge holds no foot on the plate.
        let edge = Affine {
            linear: [[1.0, 0.0, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]],
            translation: [0.0; 3],
        };
        let refused = model_frame(&world, &edge).unwrap_err();
        assert!(
            refused.contains("support 1 stands on the plate"),
            "{refused}"
        );
    }
}
