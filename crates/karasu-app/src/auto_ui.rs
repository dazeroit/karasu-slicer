//! The Automatic tool (M6.4i, TD-112; owner mockup "Automatic supports tool", 2026-10-30): every
//! support of the selected model planned at once, previewed, and applied as one step.
//!
//! **Generate** slices the selected model alone, without its supports, on a thread of its own
//! and in the plate's format as Slice does, as the Islands tool's search does, and plans the
//! supports for the regions found ([`plan`]) on the Supports tool's surface for routes. The plan
//! keeps the supports placed by hand and by the Islands tool, and replaces the automatic ones:
//! generating again starts from the model without them.
//!
//! **Preview, then Apply.** The planned supports show in the viewport, in the colour of a valid
//! hole, as Support the islands' do; Apply adds them, their braces too, as one undoable step;
//! Discard drops them. A plan made for an earlier plate is out of date and cannot be applied.
//!
//! **Nothing left is hidden.** Places the plan leaves unsupported are counted in the panel,
//! listed with why in a list window of their own (owner, 2026-10-30, as the other tools' lists),
//! shown as markers in the viewport (the strip's switch; the chosen one larger, in the
//! selection's colour), and said in the footer and the notification, never behind a plain
//! success.
pub mod panel;
pub mod plan;

use crate::{
    appearance::Appearance,
    camera::Mat4,
    gizmo::Affine,
    i18n::tr,
    islands_ui::{alone_request, found_of, plan::Preview},
    messages::{Level, Message},
    preparation_ui::PreparationUi,
    repair_panel::{marker, MARKER_SIZE},
    supports_ui::{branches, route::RouteSurface, SupportsUi},
    viewport::{Draw, Viewport},
    workspace::Workspace,
};
use eframe::egui_wgpu;
use karasu_core::{
    cancellation::Checkpoint,
    supports::{BuildVolume, SupportDocument, SupportOrigin},
};
use karasu_project::pipeline::{slice_with, Observer};
use plan::{AutoOutcome, AutoSupports, Input, Left, Place};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    time::Instant,
};

/// The viewport mesh key of the planned supports.
const PLAN_KEY: u64 = 0x4155_544F_504C_414E; // "AUTOPLAN"
/// The viewport mesh key of the markers of places left unsupported.
const MARKER_KEY: u64 = 0x4155_544F_4D41_524B; // "AUTOMARK"
/// The room around a place left unsupported the view is brought to, in mm.
const AROUND_MM: f64 = 4.0;

/// What a plan is of: the model, the plate's revision, the values and the critical angle.
#[derive(Clone, Debug, PartialEq)]
struct Key {
    object: usize,
    revision: u64,
    values: AutoSupports,
    angle_deg: f64,
}

/// How far a model's lift may differ from the tool's before Generate lifts it, in mm.
const LIFT_TOLERANCE_MM: f64 = 1e-6;
/// Merge key of a lift Generate makes, with the model's index.
const LIFT_KEY: u64 = 0x4C49_4654_0000_0000; // "LIFT"

/// Rest model `index` with its lowest point `lift` mm above the plate, merged with the next
/// lifts into one undoable step.
pub(crate) fn set_lift(workspace: &mut Workspace, index: usize, lift: f64) {
    workspace.edit_merging(LIFT_KEY | index as u64, |plate| {
        if let Some(object) = plate.objects.get_mut(index) {
            object.pose.drop = true;
            object.pose.lift_mm = (lift > 1e-9).then_some(lift);
        }
    });
}

/// The height of model `index`'s lowest point above the plate: its lift when it rests at one,
/// else where it was put.
pub(crate) fn lift_of(workspace: &Workspace, index: usize) -> f64 {
    let object = &workspace.plate().objects[index];
    if object.pose.drop {
        object.pose.lift_mm.unwrap_or(0.0)
    } else {
        workspace
            .placement_of(index)
            .map_or(0.0, |p| p.min.coordinates()[2].max(0.0))
    }
}

/// What a plan is doing, in the order it does it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// Slicing the model alone, and finding what needs support.
    Finding,
    /// Placing the tips.
    Placing,
    /// Routing the supports and growing trees.
    Growing,
    /// The closing check's passes.
    Checking,
    /// The final check: the supports built against the model.
    Verifying,
    /// Planning braces.
    Bracing,
}

/// What a plan reports while it runs, and at its end.
enum Update {
    Stage(Stage),
    Progress(f32),
    Done(Box<Result<(AutoOutcome, Preview), String>>),
}

/// A plan running on its own thread.
struct Job {
    key: Key,
    /// The automatic supports the plan replaces: those of the place it covers.
    replaced: Vec<u64>,
    started: Instant,
    stage: Stage,
    progress: f32,
    stop: Arc<AtomicBool>,
    receiver: mpsc::Receiver<Update>,
}

impl Drop for Job {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// A finished plan, its preview and whether that is in the viewport.
struct Planned {
    key: Key,
    /// The automatic supports Apply replaces: those of the place the plan covers.
    replaced: Vec<u64>,
    outcome: AutoOutcome,
    preview: Preview,
    uploaded: bool,
    seconds: f64,
}

/// What Apply added, for the panel's Applied facts while the plate stays as it left it.
struct Applied {
    object: usize,
    revision: u64,
    supports: usize,
    minis: usize,
    branches: usize,
    braces: usize,
    islands: [usize; 2],
    overhangs: [usize; 2],
    left: Vec<Left>,
}

/// Turns a slice's progress into a share of the work done.
struct Progress {
    sender: mpsc::Sender<Update>,
    total: usize,
}

impl Observer for Progress {
    fn phase(&mut self, _: &str) {}

    fn begin_layers(&mut self, total: usize) {
        self.total = total;
    }

    fn layers(&mut self, completed: usize) {
        #[allow(clippy::cast_precision_loss)] // Layer counts.
        let share = completed as f32 / self.total.max(1) as f32;
        let _ = self.sender.send(Update::Progress(share));
    }
}

/// The Automatic tool's state.
#[allow(clippy::struct_excessive_bools)] // The tool's switches and what is in the viewport.
pub struct AutoUi {
    /// The panel's Advanced switch (kept with the settings).
    pub advanced: bool,
    /// The critical angle's default, the overhangs' (for its field's reset button).
    pub default_angle_deg: f64,
    /// The places left unsupported show as markers (the strip's switch, kept with the settings).
    pub show_left: bool,
    /// The list window of the places left is open (kept with the settings).
    pub list_open: bool,
    /// The list shows only the islands left.
    only_islands: bool,
    /// The place left chosen in the list, by its place there.
    at: Option<usize>,
    /// The critical angle, in degrees from the plate (the overhangs' angle each time the tool
    /// opens).
    pub angle_deg: f64,
    /// Where plans write their slice.
    cache: Option<PathBuf>,
    /// Generate was asked for this model: it starts once the surface for routes is gridded.
    waiting: Option<usize>,
    job: Option<Job>,
    planned: Option<Planned>,
    applied: Option<Applied>,
    marker_uploaded: bool,
    preview_shown: bool,
    view: Option<[[f64; 3]; 2]>,
    outbox: Vec<Message>,
}

impl Default for AutoUi {
    fn default() -> Self {
        Self::new()
    }
}

/// The application's folder for the plans' slices; each plan removes its own file.
fn cache_folder() -> Option<PathBuf> {
    let paths = karasu_core::profiles::store::ProfilePaths::discover().ok()?;
    let folder = paths.cache.join("automatic");
    std::fs::create_dir_all(&folder).ok()?;
    Some(folder)
}

/// What model `index`'s plan is made from, its regions still to find, its hollowed shell's
/// preparation to tell the cavity's regions, and the automatic supports it replaces: see
/// [`plan`]. `None` without the surface for routes, a printer or a placement.
fn plan_input(
    workspace: &Workspace,
    preparation: &PreparationUi,
    supports: &SupportsUi,
    index: usize,
    angle_deg: f64,
) -> Option<(
    Input,
    Option<Arc<karasu_project::pipeline::Preparation>>,
    Vec<u64>,
)> {
    let surface = supports.route_surface(workspace, preparation, index)?;
    let object = workspace.plate().objects.get(index)?;
    let printer = workspace.printer()?;
    let to_world = workspace.model_to_world(index)?;
    let to_model = to_world.inverse()?;
    let mut values = supports.automatic;
    // A model that is not hollowed has no cavity to choose.
    if object.hollowing().is_none() {
        values.place = Place::Everywhere;
    }
    // The supports kept: placed by hand or by the Islands tool, and the automatic ones of the
    // place the plan leaves as it is.
    let replaced = workspace
        .supports(index)
        .map_or_default(|d| replaced(d, &surface, &to_world, values.place));
    let document = workspace.supports(index).map(|d| without(d, &replaced));
    let kept_tips = document.as_ref().map_or_default(|d| {
        d.supports
            .iter()
            .map(|s| to_world.point(s.tip_mm))
            .collect()
    });
    let shared = preparation
        .shared(workspace, index)
        .map(|(shared, _)| shared);
    let shell = shared.clone().filter(|_| object.hollowing().is_some());
    let model =
        crate::islands_ui::plan::check::model_input(workspace, preparation, supports, index);
    let input = Input {
        regions: Vec::new(),
        islands: Vec::new(),
        document,
        kept_tips,
        surface,
        next: values.next(&supports.presets),
        cavity_next: values
            .cavity()
            .filter(|_| object.hollowing().is_some())
            .map(|cavity| cavity.next(&supports.presets)),
        values,
        angle_deg,
        volume: BuildVolume {
            origin_xy_mm: printer.origin_xy_mm,
            size_mm: printer.build_size_mm,
        },
        to_model,
        model,
    };
    Some((input, shell, replaced))
}

/// The automatic supports of `document` that a plan covering `place` replaces: every one, or
/// those whose tip (`to_world` from the model frame) touches the cavity's surface, or the
/// outside's. A tip over no surface within [`crate::islands_ui::PROBE_MM`] counts as outside.
pub(crate) fn replaced(
    document: &SupportDocument,
    surface: &RouteSurface,
    to_world: &Affine,
    place: Place,
) -> Vec<u64> {
    let probe = crate::islands_ui::PROBE_MM;
    let in_cavity = |tip: [f64; 3]| {
        let [x, y, z] = to_world.point(tip);
        surface.mesh().is_some_and(|mesh| {
            surface
                .grid()
                .first_above(mesh, [x, y, z - probe], z + probe)
                .is_some_and(|(_, _, t)| surface.is_cavity(t))
        })
    };
    document
        .supports
        .iter()
        .filter(|s| {
            s.origin == SupportOrigin::Automatic
                && (place == Place::Everywhere || place.holds(in_cavity(s.tip_mm)))
        })
        .map(|s| s.id)
        .collect()
}

/// `document` without the supports `ids`, the branches and braces on them too.
pub(crate) fn without(document: &SupportDocument, ids: &[u64]) -> SupportDocument {
    let mut kept = document.clone();
    branches::without(&mut kept, ids);
    kept
}

impl AutoUi {
    /// The tool as it starts: its built-in values and the overhangs' default angle.
    #[must_use]
    pub fn new() -> Self {
        Self {
            advanced: false,
            default_angle_deg: crate::settings::DEFAULT_OVERHANG_ANGLE_DEG,
            show_left: true,
            list_open: true,
            only_islands: false,
            at: None,
            angle_deg: crate::settings::DEFAULT_OVERHANG_ANGLE_DEG,
            cache: None,
            waiting: None,
            job: None,
            planned: None,
            applied: None,
            marker_uploaded: false,
            preview_shown: false,
            view: None,
            outbox: Vec::new(),
        }
    }

    /// The messages to show as notifications, taken.
    pub fn take_messages(&mut self) -> Vec<Message> {
        std::mem::take(&mut self.outbox)
    }

    /// Where the view should go, once: a place left unsupported, shown.
    pub fn take_view(&mut self) -> Option<[[f64; 3]; 2]> {
        self.view.take()
    }

    /// Whether a plan is being made, or waits to start.
    #[must_use]
    pub fn planning(&self) -> bool {
        self.job.is_some() || self.waiting.is_some()
    }

    /// Generate: plan model `index`'s supports with the panel's values; or Stop while planning.
    pub fn generate(&mut self, index: usize) {
        if self.planning() {
            self.job = None;
            self.waiting = None;
            return;
        }
        self.planned = None;
        self.at = None;
        self.waiting = Some(index);
    }

    /// Generate for model `index` with `values`: the model lifted to their lift first, when it
    /// stands elsewhere (owner, 2026-11-04, TD-128), as one undoable step; the plan waits for it
    /// to be prepared there.
    pub fn generate_lifted(&mut self, workspace: &mut Workspace, index: usize, lift_mm: f64) {
        if !self.planning() && (lift_of(workspace, index) - lift_mm).abs() > LIFT_TOLERANCE_MM {
            set_lift(workspace, index, lift_mm);
        }
        self.generate(index);
    }

    /// Drop the plan shown.
    pub fn discard(&mut self) {
        self.planned = None;
    }

    /// Bring the view to `left`.
    fn show(&mut self, left: &Left) {
        let [low, high] = left.bounds;
        self.view = Some([low.map(|c| c - AROUND_MM), high.map(|c| c + AROUND_MM)]);
    }

    /// Start a waiting plan once the surface for routes is ready, and take a running one's
    /// news. Returns whether a repaint is needed soon.
    pub fn poll(
        &mut self,
        workspace: &Workspace,
        preparation: &PreparationUi,
        supports: &SupportsUi,
    ) -> bool {
        if let Some(index) = self.waiting {
            if index >= workspace.plate().objects.len() {
                self.waiting = None;
            } else if supports
                .route_surface(workspace, preparation, index)
                .is_some()
            {
                self.waiting = None;
                self.start(workspace, preparation, supports, index);
            } else {
                return true;
            }
        }
        let Some(job) = &mut self.job else {
            return false;
        };
        let mut done = None;
        loop {
            match job.receiver.try_recv() {
                Ok(Update::Stage(stage)) => {
                    job.stage = stage;
                    job.progress = 0.0;
                }
                Ok(Update::Progress(share)) => job.progress = share,
                Ok(Update::Done(result)) => {
                    done = Some(*result);
                    break;
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    done = Some(Err(tr!("auto-stopped")));
                    break;
                }
            }
        }
        let Some(result) = done else {
            return true;
        };
        let (key, seconds) = (job.key.clone(), job.started.elapsed().as_secs_f64());
        let replaced = std::mem::take(&mut job.replaced);
        self.job = None;
        match result {
            Ok((outcome, preview)) => {
                log::info!(
                    target: "supports",
                    "planned {} automatic supports ({} left unsupported) in {seconds:.2} s",
                    outcome.plan.supports.len(),
                    outcome.left.len()
                );
                self.planned = Some(Planned {
                    key,
                    replaced,
                    outcome,
                    preview,
                    uploaded: false,
                    seconds,
                });
            }
            Err(reason) => self
                .outbox
                .push(Message::new(Level::Error, tr!("auto-failed")).detail(reason)),
        }
        true
    }

    /// Plan model `index`'s supports on a thread of its own: see the module.
    fn start(
        &mut self,
        workspace: &Workspace,
        preparation: &PreparationUi,
        supports: &SupportsUi,
        index: usize,
    ) {
        let Some((input, shell, replaced)) =
            plan_input(workspace, preparation, supports, index, self.angle_deg)
        else {
            return;
        };
        if self.cache.is_none() {
            self.cache = cache_folder();
        }
        let Some(cache) = &self.cache else {
            self.outbox
                .push(Message::new(Level::Error, tr!("error-no-cache")));
            return;
        };
        let Some(format) = crate::slicing_ui::plate_format(workspace) else {
            self.outbox
                .push(Message::new(Level::Error, tr!("islands-no-format")));
            return;
        };
        let name = crate::islands_ui::slice_name("automatic");
        let output = cache.join(match crate::slicing_ui::extension(&format) {
            Some(ext) => format!("{name}.{ext}"),
            None => format!("{name}-{}", karasu_project::request::REFERENCE_FORMAT),
        });
        let (request, revision) =
            match alone_request(workspace, index, output.clone(), format, self.angle_deg) {
                Ok(Some(made)) => made,
                Ok(None) => return,
                Err(error) => {
                    self.outbox
                        .push(Message::new(Level::Error, tr!("auto-refused")).detail(error));
                    return;
                }
            };
        let key = Key {
            object: index,
            revision,
            values: input.values,
            angle_deg: self.angle_deg,
        };
        let pipeline = workspace.pipeline_cache();
        let stop = Arc::new(AtomicBool::new(false));
        let cancelled = Arc::clone(&stop);
        let (sender, receiver) = mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("automatic-supports".to_owned())
            .spawn(move || {
                let mut input = input;
                let mut observer = Progress {
                    sender: sender.clone(),
                    total: 0,
                };
                let halt = |_: Checkpoint| cancelled.load(Ordering::Relaxed);
                let found = slice_with(&request, Some(pipeline.as_ref()), &mut observer, &halt)
                    .map_err(|e| e.to_string())
                    .and_then(|outcome| {
                        outcome
                            .unsupported
                            .ok_or_else(|| "the unsupported regions were not found".to_owned())
                    });
                let _ = std::fs::remove_file(&output);
                let result = found.map(|found| {
                    (input.regions, input.islands) =
                        found_of(&found, shell.as_deref(), index, &cancelled);
                    let stage = |k: u32| {
                        let stage = match k {
                            0 => Stage::Placing,
                            1 => Stage::Growing,
                            2 => Stage::Checking,
                            3 => Stage::Verifying,
                            _ => Stage::Bracing,
                        };
                        let _ = sender.send(Update::Stage(stage));
                    };
                    let outcome = plan::plan(&input, &stage, &cancelled);
                    let preview = crate::islands_ui::plan::preview(&outcome.plan, input.volume);
                    (outcome, preview)
                });
                let _ = sender.send(Update::Done(Box::new(result)));
            });
        match spawned {
            Ok(_) => {
                self.job = Some(Job {
                    key,
                    replaced,
                    started: Instant::now(),
                    stage: Stage::Finding,
                    progress: 0.0,
                    stop,
                    receiver,
                });
            }
            Err(error) => {
                log::warn!(target: "supports", "cannot start planning the supports: {error}");
            }
        }
    }

    /// The plan for model `index`, and whether it describes the plate as it is.
    fn plan_of(&self, workspace: &Workspace, index: Option<usize>) -> Option<(&Planned, bool)> {
        let planned = self
            .planned
            .as_ref()
            .filter(|p| Some(p.key.object) == index)?;
        Some((planned, planned.key.revision == workspace.revision()))
    }

    /// The places left for model `index`: the current plan's, or else Apply's.
    fn left_of(&self, workspace: &Workspace, index: Option<usize>) -> &[Left] {
        match (
            self.plan_of(workspace, index),
            self.applied_of(workspace, index),
        ) {
            (Some((planned, true)), _) => &planned.outcome.left,
            (_, Some(applied)) => &applied.left,
            _ => &[],
        }
    }

    /// What Apply added to model `index`, while the plate is as Apply left it.
    fn applied_of(&self, workspace: &Workspace, index: Option<usize>) -> Option<&Applied> {
        self.applied
            .as_ref()
            .filter(|a| Some(a.object) == index && a.revision == workspace.revision())
    }

    /// Apply: model `index`'s automatic supports replaced by the plan's, its braces added, as one
    /// undoable step. Returns how many supports it added.
    pub fn apply(&mut self, workspace: &mut Workspace, index: usize) -> usize {
        let Some(planned) = self
            .planned
            .take()
            .filter(|p| p.key.object == index && p.key.revision == workspace.revision())
        else {
            return 0;
        };
        let Some(to_model) = workspace.model_to_world(index).and_then(|m| m.inverse()) else {
            return 0;
        };
        let outcome = &planned.outcome;
        let kept = workspace
            .supports(index)
            .map(|d| without(d, &planned.replaced));
        let document = plan::with_planned(kept.as_ref(), &to_model, outcome);
        let count = outcome.plan.supports.len();
        workspace.set_supports(index, Some(document), None);
        let left = outcome.left.len();
        let text = tr!(
            "auto-added",
            count = count,
            braces = outcome.braces.len(),
            left = left
        );
        self.outbox.push(Message::new(
            if left > 0 {
                Level::Warning
            } else {
                Level::Success
            },
            text,
        ));
        self.applied = Some(Applied {
            object: index,
            revision: workspace.revision(),
            supports: count,
            minis: outcome
                .plan
                .supports
                .iter()
                .filter(|p| p.routed.support.kind == karasu_core::supports::SupportKind::Mini)
                .count(),
            branches: outcome.plan.branches,
            braces: outcome.braces.len(),
            islands: outcome.islands,
            overhangs: outcome.overhangs,
            left: outcome.left.clone(),
        });
        count
    }

    /// The planned supports' preview and the markers of places left unsupported, for model
    /// `index` while the tool is in use (`focus`), from `eye`.
    pub fn draws(
        &mut self,
        viewport: &mut Viewport,
        state: &egui_wgpu::RenderState,
        workspace: &Workspace,
        focus: Option<usize>,
        eye: [f64; 3],
        appearance: &Appearance,
    ) -> Vec<Draw> {
        let mut draws = Vec::new();
        let shown = self
            .planned
            .as_mut()
            .filter(|p| Some(p.key.object) == focus && p.key.revision == workspace.revision());
        match shown {
            Some(planned) => {
                if let Some((mesh, origin)) = &planned.preview {
                    if !planned.uploaded {
                        viewport.upload(&state.device, PLAN_KEY, mesh, *origin);
                        planned.uploaded = true;
                    }
                    self.preview_shown = true;
                    draws.push(Draw {
                        mesh: PLAN_KEY,
                        model: crate::preparation_ui::translation(*origin),
                        color: appearance.hole_valid.linear(),
                        overlay: false,
                        cap: crate::viewport::Cap::Support,
                        holes: [0, 0],
                        bore: None,
                        outside: None,
                        triangles: None,
                        look: crate::viewport::Look::Solid,
                    });
                }
            }
            None if self.preview_shown => {
                viewport.remove(PLAN_KEY);
                self.preview_shown = false;
            }
            None => {}
        }
        let left: Vec<[f64; 3]> = self
            .left_of(workspace, focus)
            .iter()
            .map(|l| l.point)
            .collect();
        if !self.show_left || left.is_empty() {
            return draws;
        }
        if !self.marker_uploaded {
            viewport.upload(&state.device, MARKER_KEY, &marker(), [0.0; 3]);
            self.marker_uploaded = true;
        }
        let at = self.at;
        for (k, point) in left.into_iter().enumerate() {
            // The chosen one stands out among many, as a chosen island does.
            let (color, scale) = if Some(k) == at {
                (appearance.support_selected.linear(), 2.4)
            } else {
                (appearance.marker_hollowing.linear(), 1.4)
            };
            let distance = (0..3)
                .map(|k| (point[k] - eye[k]).powi(2))
                .sum::<f64>()
                .sqrt();
            let size = MARKER_SIZE * scale * distance;
            let model = Mat4([
                [size, 0.0, 0.0, point[0]],
                [0.0, size, 0.0, point[1]],
                [0.0, 0.0, size, point[2]],
                [0.0, 0.0, 0.0, 1.0],
            ]);
            // Whole in front of the model, half through it.
            let mut through = color;
            through[3] *= 0.5;
            for (color, overlay) in [(color, false), (through, true)] {
                draws.push(Draw {
                    mesh: MARKER_KEY,
                    model,
                    color,
                    overlay,
                    cap: crate::viewport::Cap::Off,
                    holes: [0, 0],
                    bore: None,
                    outside: None,
                    triangles: None,
                    look: crate::viewport::Look::Solid,
                });
            }
        }
        draws
    }
}

#[cfg(test)]
mod tests;
