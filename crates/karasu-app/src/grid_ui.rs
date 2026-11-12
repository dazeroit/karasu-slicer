//! Supports on a grid (owner, 2026-11-06, TD-138; the mockup "Supports on a grid"): the
//! Supports tool's Grid mode.
//!
//! **Pick, then Apply.** With the mode on (the strip's Grid), a click on the selected model's
//! underside picks a flat face there ([`plan::pick`]); with the button's Rectangle or Lasso, a
//! rectangle dragged or a shape drawn on the screen picks the triangles facing down it holds
//! ([`plan::select`]), while the drag draws the shape instead of turning the view. Its tips ([`plan::tips`]) are grown into
//! supports on a thread of their own, as the Automatic tool grows its tips
//! ([`crate::auto_ui::plan::plan_tips`]): straight down to the plate, or with branches when
//! asked, then built against the model, those a slice would build with an error going. The
//! supports show in the viewport in the colour of a valid hole, as the Automatic tool's plan;
//! Apply adds them as one undoable step, Discard drops them. Clicking another face replaces
//! the plan.
//!
//! **Values change, the plan follows.** A clicked face is kept as its point in the model's
//! frame, a shape as the triangles it selected once planned: a value changed in the panel, or
//! the model moved, plans the same face again. A model whose surface changed (prepared again)
//! drops a shape's triangles: it is selected again. A plan for an earlier plate is
//! out of date and cannot be applied.
//!
//! **Nothing left is hidden.** Tips whose support could not be grown are counted in the panel
//! and the footer, and said by Apply's notification.
pub mod panel;
pub mod plan;

use crate::{
    appearance::Appearance,
    auto_ui::plan::{plan_tips, with_planned, AutoOutcome, Input},
    camera::{Camera, Mat4},
    gizmo::Affine,
    i18n::tr,
    islands_ui::plan::{check, Preview},
    messages::{Level, Message},
    preparation_ui::PreparationUi,
    supports_ui::{NextSupport, SupportsUi},
    viewport::{Draw, Viewport},
    workspace::{SurfaceHit, Workspace},
};
use eframe::egui_wgpu;
use karasu_core::supports::{BuildVolume, SupportOrigin};
use plan::{GridTips, GridValues, PickMode};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    time::Instant,
};

/// The viewport mesh key of the planned supports.
const PLAN_KEY: u64 = 0x4752_4944_504C_414E; // "GRIDPLAN"

/// What is picked: its model, the side of a hollowed model's surface it is on, and where.
#[derive(Clone, Debug, PartialEq)]
struct Pick {
    object: usize,
    cavity: bool,
    at: At,
}

/// A shape drawn on a `size` pixel viewport seen through `view_projection`, its corners, and
/// the model's frame then (model to world), so that it keeps to the model when the model moves.
#[derive(Clone, Debug, PartialEq)]
struct Clip {
    view_projection: Mat4,
    size: [f64; 2],
    polygon: Vec<[f64; 2]>,
    to_world: Affine,
}

impl Clip {
    /// Whether the shape holds a point of the model, now at `p` (world mm) through `to_model`.
    fn holds(&self, to_model: &Affine, p: [f64; 3]) -> bool {
        self.view_projection
            .project(self.to_world.point(to_model.point(p)))
            .is_some_and(|[x, y, _]| {
                plan::inside_polygon(
                    &self.polygon,
                    [
                        f64::midpoint(x, 1.0) * self.size[0],
                        f64::midpoint(1.0, -y) * self.size[1],
                    ],
                )
            })
    }
}

/// Where a pick is.
#[derive(Clone, Debug, PartialEq)]
enum At {
    /// A clicked face: a point on it, in the model's frame.
    Face([f64; 3]),
    /// A shape drawn, its triangles still to select.
    Shape(Arc<Clip>),
    /// The triangles a shape selected, of a surface of `count` triangles, and the shape.
    Triangles {
        triangles: Arc<Vec<u32>>,
        count: usize,
        clip: Arc<Clip>,
    },
}

/// What a plan is of: the face picked, the plate's revision, the values and the supports'
/// preset.
#[derive(Clone, Debug, PartialEq)]
struct Key {
    pick: Pick,
    revision: u64,
    values: GridValues,
    next: NextSupport,
}

/// The face a plan found: its area and tilt.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FaceFacts {
    pub area_mm2: f64,
    pub tilt_deg: f64,
}

/// A finished plan.
struct Planned {
    key: Key,
    face: FaceFacts,
    tips: GridTips,
    outcome: AutoOutcome,
    preview: Preview,
    uploaded: bool,
    seconds: f64,
}

/// Why a face could not be gridded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Failure {
    /// The surface for routes is not there.
    NoSurface,
    /// No underside lies where the face was picked.
    NoFace,
    /// The face is too small for one tip.
    TooSmall,
    /// The shape holds no triangle facing down.
    NoSelection,
    /// The surface changed since its triangles were selected.
    Stale,
}

impl Failure {
    fn text(self) -> String {
        match self {
            Self::NoSurface => tr!("grid-no-surface"),
            Self::NoFace => tr!("grid-no-face"),
            Self::TooSmall => tr!("grid-too-small"),
            Self::NoSelection => tr!("grid-no-selection"),
            Self::Stale => tr!("grid-stale"),
        }
    }
}

/// What a shape selected: its triangles, of a surface of so many.
type Selected = Option<(Arc<Vec<u32>>, usize)>;

/// What a plan's thread sends back: the face, its tips, the supports grown, their preview, and
/// for a shape the triangles it selected.
type Done = Result<(FaceFacts, GridTips, AutoOutcome, Preview, Selected), Failure>;

/// A plan running on its own thread.
struct Job {
    key: Key,
    started: Instant,
    stop: Arc<AtomicBool>,
    receiver: mpsc::Receiver<Done>,
}

impl Drop for Job {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// The Grid mode's state.
#[derive(Default)]
pub struct GridUi {
    /// The mode is on: a click picks a face, or a drag a shape's.
    pub active: bool,
    /// How an underside is picked (the strip's options, kept with the settings).
    pub mode: PickMode,
    /// The shape being drawn, in viewport pixels: a rectangle's two corners, or a lasso's path.
    drawing: Option<Vec<[f64; 2]>>,
    pick: Option<Pick>,
    job: Option<Job>,
    planned: Option<Planned>,
    preview_shown: bool,
    outbox: Vec<Message>,
}

impl GridUi {
    /// The mode as it starts: off, nothing picked.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The messages to show as notifications, taken.
    pub fn take_messages(&mut self) -> Vec<Message> {
        std::mem::take(&mut self.outbox)
    }

    /// Turn the mode on or off; off, the face and its plan are dropped.
    pub fn toggle(&mut self) {
        self.active = !self.active;
        if !self.active {
            self.discard();
        }
    }

    /// Drop the face picked and its plan.
    pub fn discard(&mut self) {
        self.pick = None;
        self.job = None;
        self.planned = None;
        self.drawing = None;
    }

    /// Pick undersides with `mode` from now on.
    pub fn set_mode(&mut self, mode: PickMode) {
        self.mode = mode;
        self.drawing = None;
    }

    /// Whether a primary drag draws a shape: the mode on with Rectangle or Lasso.
    #[must_use]
    pub fn shaping(&self) -> bool {
        self.active && self.mode != PickMode::Face
    }

    /// Whether a shape is being drawn.
    #[must_use]
    pub fn drawing(&self) -> bool {
        self.drawing.is_some()
    }

    /// Start drawing a shape at `at` (viewport pixels).
    pub fn begin_shape(&mut self, at: [f64; 2]) {
        self.drawing = Some(vec![at]);
    }

    /// The pointer at `at` while drawing: a rectangle's far corner, or a lasso's next point.
    pub fn extend_shape(&mut self, at: [f64; 2]) {
        let mode = self.mode;
        if let Some(points) = &mut self.drawing {
            if mode == PickMode::Rectangle {
                points.truncate(1);
                points.push(at);
            } else if points
                .last()
                .is_none_or(|p| (p[0] - at[0]).hypot(p[1] - at[1]) >= 2.0)
            {
                // A point every couple of pixels is enough for a freehand path.
                points.push(at);
            }
        }
    }

    /// Stop drawing without a pick.
    pub fn cancel_shape(&mut self) {
        self.drawing = None;
    }

    /// The shape as drawn, closed: a rectangle's four corners or a lasso's path.
    #[must_use]
    pub fn shape(&self) -> Option<Vec<[f64; 2]>> {
        let points = self.drawing.as_ref()?;
        if self.mode == PickMode::Rectangle {
            let (a, b) = (points.first()?, points.last()?);
            Some(vec![*a, [b[0], a[1]], *b, [a[0], b[1]]])
        } else {
            Some(points.clone())
        }
    }

    /// The shape drawn, released on model `index` (its cavity's side, `cavity`) as `camera`
    /// shows it in a `size` pixel viewport: picked, to be planned. A shape too small to hold
    /// anything is dropped.
    pub fn finish_shape(
        &mut self,
        workspace: &Workspace,
        (camera, size): (&Camera, [f64; 2]),
        (index, cavity): (usize, bool),
    ) {
        let polygon = self.shape();
        self.drawing = None;
        let Some(polygon) = polygon else {
            return;
        };
        let (mut low, mut high) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
        for p in &polygon {
            for k in 0..2 {
                low[k] = low[k].min(p[k]);
                high[k] = high[k].max(p[k]);
            }
        }
        let Some(to_world) = workspace.model_to_world(index) else {
            return;
        };
        if polygon.len() < 3 || high[0] - low[0] < 3.0 || high[1] - low[1] < 3.0 {
            return;
        }
        self.pick = Some(Pick {
            object: index,
            cavity,
            at: At::Shape(Arc::new(Clip {
                view_projection: camera.view_projection(size[0] / size[1]),
                size,
                polygon,
                to_world,
            })),
        });
        self.planned = None;
    }

    /// Whether a plan is being made.
    #[must_use]
    pub fn planning(&self) -> bool {
        self.job.is_some()
    }

    /// A click on model `index` meeting its surface at `hit`, on its cavity's side (`cavity`)
    /// or its outside: the face there is picked, to be planned. Returns whether the mode took
    /// the click.
    pub fn click(
        &mut self,
        workspace: &Workspace,
        hit: Option<SurfaceHit>,
        (index, cavity): (usize, bool),
    ) -> bool {
        if !self.active {
            return false;
        }
        // With a shape to draw, a click alone picks nothing.
        if self.mode != PickMode::Face {
            return true;
        }
        let Some(hit) = hit.filter(|h| h.object == index) else {
            return true;
        };
        if hit.normal[2] >= 0.0 {
            self.outbox
                .push(Message::new(Level::Warning, tr!("grid-facing-up")));
            return true;
        }
        let Some(to_model) = workspace.model_to_world(index).and_then(|m| m.inverse()) else {
            return true;
        };
        self.pick = Some(Pick {
            object: index,
            cavity,
            at: At::Face(to_model.point(hit.point)),
        });
        self.planned = None;
        true
    }

    /// The plan wanted now: the face picked, the plate as it is and the values set.
    fn wanted(&self, workspace: &Workspace, supports: &SupportsUi) -> Option<Key> {
        let pick = self.pick.clone()?;
        (pick.object < workspace.plate().objects.len()).then(|| Key {
            pick,
            revision: workspace.revision(),
            values: supports.grid,
            next: supports.grid.next(&supports.presets),
        })
    }

    /// Plan the face picked when what is wanted changed, once the surface for routes is ready,
    /// and take a running plan's result. Returns whether a repaint is needed soon.
    pub fn poll(
        &mut self,
        workspace: &Workspace,
        preparation: &PreparationUi,
        supports: &SupportsUi,
    ) -> bool {
        let wanted = self.wanted(workspace, supports).filter(|_| self.active);
        let current = |key: &Key| Some(key) == wanted.as_ref();
        let busy = self.job.as_ref().is_some_and(|j| current(&j.key));
        let done = self.planned.as_ref().is_some_and(|p| current(&p.key));
        if let (Some(key), false, false) = (wanted.clone(), busy, done) {
            self.job = None;
            if supports
                .route_surface(workspace, preparation, key.pick.object)
                .is_none()
            {
                return true;
            }
            self.start(workspace, preparation, supports, key);
        }
        let Some(job) = &self.job else {
            return false;
        };
        let result = match job.receiver.try_recv() {
            Ok(result) => result.map_err(Failure::text),
            Err(mpsc::TryRecvError::Empty) => return true,
            Err(mpsc::TryRecvError::Disconnected) => Err(tr!("grid-stopped")),
        };
        let (key, seconds) = (job.key.clone(), job.started.elapsed().as_secs_f64());
        self.job = None;
        match result {
            Ok((face, tips, outcome, preview, selected)) => {
                // A shape is kept as the triangles it selected, the plan as of them.
                let mut key = key;
                if let (Some((triangles, count)), At::Shape(clip)) = (selected, &key.pick.at) {
                    key.pick.at = At::Triangles {
                        triangles,
                        count,
                        clip: Arc::clone(clip),
                    };
                    self.pick = Some(key.pick.clone());
                }
                // A plan that grows nothing says why, never only in the footer.
                if outcome.plan.supports.is_empty() {
                    if let Some(why) = most_common(&outcome.left) {
                        self.outbox
                            .push(Message::new(Level::Warning, tr!("grid-none")).detail(
                                if why == crate::islands_ui::plan::Skip::TooLow {
                                    tr!("grid-too-low")
                                } else {
                                    crate::islands_ui::panel::reason(why)
                                },
                            ));
                    }
                }
                log::info!(
                    target: "supports",
                    "planned {} supports on a grid ({} tips, {} left) in {seconds:.2} s",
                    outcome.plan.supports.len(),
                    tips.tips.len(),
                    outcome.left.len()
                );
                self.planned = Some(Planned {
                    key,
                    face,
                    tips,
                    outcome,
                    preview,
                    uploaded: false,
                    seconds,
                });
            }
            Err(reason) => {
                self.pick = None;
                self.outbox
                    .push(Message::new(Level::Warning, tr!("grid-failed")).detail(reason));
            }
        }
        true
    }

    /// Plan `key`'s face on a thread of its own: see the module.
    fn start(
        &mut self,
        workspace: &Workspace,
        preparation: &PreparationUi,
        supports: &SupportsUi,
        key: Key,
    ) {
        let index = key.pick.object;
        let Some(surface) = supports.route_surface(workspace, preparation, index) else {
            return;
        };
        let (Some(printer), Some(to_world)) =
            (workspace.printer(), workspace.model_to_world(index))
        else {
            return;
        };
        let Some(to_model) = to_world.inverse() else {
            return;
        };
        let values = key.values;
        let automatic = values.automatic();
        let input = Input {
            regions: Vec::new(),
            islands: Vec::new(),
            document: workspace.supports(index).cloned(),
            kept_tips: Vec::new(),
            surface,
            next: key.next,
            cavity_next: None,
            values: automatic,
            angle_deg: crate::settings::DEFAULT_OVERHANG_ANGLE_DEG,
            volume: BuildVolume {
                origin_xy_mm: printer.origin_xy_mm,
                size_mm: printer.build_size_mm,
            },
            to_model,
            model: check::model_input(workspace, preparation, supports, index),
        };
        // A clicked face's point where the model now stands.
        let at = match &key.pick.at {
            At::Face(point) => At::Face(to_world.point(*point)),
            other => other.clone(),
        };
        let cavity = key.pick.cavity;
        let stop = Arc::new(AtomicBool::new(false));
        let cancelled = Arc::clone(&stop);
        let (sender, receiver) = mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("grid-supports".to_owned())
            .spawn(move || {
                let result = plan_face(&input, (&values, &at, cavity), &cancelled);
                let _ = sender.send(result);
            });
        match spawned {
            Ok(_) => {
                self.job = Some(Job {
                    key,
                    started: Instant::now(),
                    stop,
                    receiver,
                });
            }
            Err(error) => {
                log::warn!(target: "supports", "cannot start planning the grid: {error}");
            }
        }
    }

    /// How many supports the plan for model `index` holds, while it describes the plate as it
    /// is.
    #[must_use]
    pub fn planned_count(&self, workspace: &Workspace, index: Option<usize>) -> Option<usize> {
        self.plan_of(workspace, index)
            .filter(|(_, current)| *current)
            .map(|(p, _)| p.outcome.plan.supports.len())
    }

    /// The plan for model `index`, and whether it describes the plate as it is.
    fn plan_of(&self, workspace: &Workspace, index: Option<usize>) -> Option<(&Planned, bool)> {
        let planned = self
            .planned
            .as_ref()
            .filter(|p| Some(p.key.pick.object) == index)?;
        Some((planned, planned.key.revision == workspace.revision()))
    }

    /// Apply: the plan's supports added to model `index` as one undoable step. Returns how many.
    pub fn apply(&mut self, workspace: &mut Workspace, index: usize) -> usize {
        let Some(planned) = self
            .planned
            .take()
            .filter(|p| p.key.pick.object == index && p.key.revision == workspace.revision())
        else {
            return 0;
        };
        let Some(to_model) = workspace.model_to_world(index).and_then(|m| m.inverse()) else {
            return 0;
        };
        let outcome = &planned.outcome;
        let document = with_planned(workspace.supports(index), &to_model, outcome);
        let count = outcome.plan.supports.len();
        workspace.set_supports(index, Some(document), None);
        self.pick = None;
        let left = outcome.left.len();
        self.outbox.push(if left > 0 {
            Message::new(
                Level::Warning,
                tr!("grid-added-left", count = count, left = left),
            )
        } else {
            Message::new(Level::Success, tr!("grid-added", count = count))
        });
        count
    }

    /// The planned supports' preview, for model `focus` while the mode is on.
    pub fn draws(
        &mut self,
        viewport: &mut Viewport,
        state: &egui_wgpu::RenderState,
        workspace: &Workspace,
        focus: Option<usize>,
        appearance: &Appearance,
    ) -> Vec<Draw> {
        let active = self.active;
        let shown = self.planned.as_mut().filter(|p| {
            active && Some(p.key.pick.object) == focus && p.key.revision == workspace.revision()
        });
        if let Some(((mesh, origin), uploaded)) =
            shown.and_then(|p| p.preview.as_ref().map(|preview| (preview, &mut p.uploaded)))
        {
            if !*uploaded {
                viewport.upload(&state.device, PLAN_KEY, mesh, *origin);
                *uploaded = true;
            }
            self.preview_shown = true;
            return vec![Draw {
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
            }];
        }
        if self.preview_shown {
            viewport.remove(PLAN_KEY);
            self.preview_shown = false;
        }
        Vec::new()
    }
}

/// The reason most of `left` were left unsupported for, if any were.
fn most_common(left: &[crate::auto_ui::plan::Left]) -> Option<crate::islands_ui::plan::Skip> {
    let mut counts: Vec<(crate::islands_ui::plan::Skip, usize)> = Vec::new();
    for l in left {
        match counts.iter_mut().find(|(why, _)| *why == l.why) {
            Some((_, n)) => *n += 1,
            None => counts.push((l.why, 1)),
        }
    }
    // The first of the most counted, in the order met.
    counts
        .into_iter()
        .fold(None, |best: Option<(_, usize)>, (why, n)| match best {
            Some((_, m)) if m >= n => best,
            _ => Some((why, n)),
        })
        .map(|(why, _)| why)
}

/// The face `at` (a point in world mm, a shape, or triangles) on `input`'s surface, on its
/// cavity's side or its outside, gridded with `values` and its supports grown: see the module.
fn plan_face(
    input: &Input,
    (values, at, cavity): (&GridValues, &At, bool),
    stop: &AtomicBool,
) -> Done {
    let surface = input.surface.as_ref();
    let mesh = surface.mesh().ok_or(Failure::NoSurface)?;
    let side = |t: u32| surface.is_cavity(t) == cavity;
    let count = mesh.triangles().len();
    let (face, selected, clip) = match at {
        At::Face([x, y, z]) => {
            let probe = crate::islands_ui::PROBE_MM;
            let (_, _, seed) = surface
                .grid()
                .first_above(mesh, [*x, *y, z - probe], z + probe)
                .ok_or(Failure::NoFace)?;
            let face = plan::pick(mesh, seed, values.flat_deg, &side).ok_or(Failure::NoFace)?;
            (face, None, None)
        }
        At::Shape(clip) => {
            // Drawn where the model stands still: the shape's eye sees it where it was.
            let triangles = plan::select(
                mesh,
                (&clip.view_projection, clip.size),
                &clip.polygon,
                &side,
            );
            let kept = Arc::new(triangles.clone());
            let face = plan::region(mesh, triangles).ok_or(Failure::NoSelection)?;
            (face, Some((kept, count)), Some(clip))
        }
        At::Triangles {
            triangles,
            count: of,
            clip,
        } => {
            if *of != count || triangles.iter().any(|&t| t as usize >= count) {
                return Err(Failure::Stale);
            }
            let face =
                plan::region(mesh, triangles.as_ref().clone()).ok_or(Failure::NoSelection)?;
            (face, None, Some(clip))
        }
    };
    // A shape's tips are those it holds, as the model stood when it was drawn.
    let within = |p: [f64; 3]| clip.is_none_or(|c| c.holds(&input.to_model, p));
    let radius = input.next.parameters().surface_width_mm() / 2.0;
    let tips = plan::tips(
        mesh,
        &face,
        values,
        radius,
        (&|t| surface.is_cavity(t), &within),
    );
    if tips.tips.is_empty() {
        return Err(Failure::TooSmall);
    }
    let outcome = plan_tips(
        input,
        (&input.values, &input.next),
        (&tips.tips, SupportOrigin::Placed),
        stop,
    );
    let preview = crate::islands_ui::plan::preview(&outcome.plan, input.volume);
    let facts = FaceFacts {
        area_mm2: face.area_mm2,
        tilt_deg: face.tilt_deg,
    };
    Ok((facts, tips, outcome, preview, selected))
}

#[cfg(test)]
pub(crate) mod tests;
