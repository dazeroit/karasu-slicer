//! The Islands tool (owner mockup "Islands tool", 2026-10-29): what the selected model's print
//! starts in mid-air, and which of those places a support holds.
//!
//! **Find islands** slices the selected model alone, without its supports, on a thread of its
//! own and in the plate's format as Slice does, and reads its layers' masks for the unsupported regions at the panel's
//! critical angle ([`karasu_core::analysis::unsupported`], TD-107): an island is a region that
//! starts in mid-air. The model is sliced without its supports so every island is found, held
//! or not; a support holds an island when its tip stands where the island starts
//! ([`plan::check::holds`]). The tool never searches by itself: a search reflects the
//! plate as it was, and once the plate changes its islands show as out of date until found
//! again.
//!
//! **Shown** as markers a constant size on screen, drawn on the GPU through the model as the
//! cavity markers are: red without a support, green held, faint when out of date, the chosen one
//! larger. The local minima (TD-107 level 2) show as small yellow markers, worked out on a
//! thread of their own for the prepared surface when asked. The panel steps through the islands
//! and tells the chosen one's layers, height, area, place and support; "Support it here" adds a
//! support under its lowest point with the Supports tool's values, in the cavity for an island
//! on the cavity's ceiling. A list window lists them all.
pub mod panel;
pub mod plan;

use crate::{
    appearance::Appearance,
    camera::{Camera, Mat4},
    i18n::{decimal, tr},
    messages::{Level, Message},
    preparation_ui::PreparationUi,
    repair_panel::{marker, MARKER_SIZE},
    supports_ui::SupportsUi,
    viewport::{Draw, Viewport},
    workspace::Workspace,
};
use eframe::{egui, egui_wgpu};
use karasu_core::{
    analysis::{minima::local_minima, unsupported::RegionKind},
    cancellation::Cancellation,
    cancellation::Checkpoint,
    geometry::Affine3,
    mesh::TriangleMesh,
    supports::SurfaceGrid,
};
use karasu_project::{
    pipeline::{slice_with, Observer, Preparation, Unsupported},
    request::Subject,
};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    time::Instant,
};

/// The viewport mesh key of the islands' and minima's marker.
const MARKER_KEY: u64 = 0x4953_4C41_4E44_4D4B; // "ISLANDMK"
/// The viewport mesh key of the supports planned under the islands.
const PLAN_KEY: u64 = 0x4953_4C41_4E44_504C; // "ISLANDPL"
/// How far beyond an island's footprint a support's tip still holds it, in mm: a contact's
/// radius and a pixel's rounding.
pub(crate) const HOLD_REACH_MM: f64 = 0.6;
/// How far under an island's first layer a ray starts to find the surface there, in mm.
pub(crate) const PROBE_MM: f64 = 1.0;
/// The room around an island the view is brought to, in mm.
const AROUND_MM: f64 = 4.0;
/// The critical angles the search takes, in degrees from the plate.
pub const ANGLE_DEG: std::ops::RangeInclusive<f64> = 1.0..=89.0;
/// The smallest islands' areas whose markers can be hidden, in mm².
pub const HIDE_UNDER_MM2: std::ops::RangeInclusive<f64> = 0.0..=100.0;

/// One island of a search, where a piece starts in mid-air (owner, 2026-10-30), in world
/// millimetres. The Automatic tool tells its regions the same way, over all their layers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Island {
    /// The centroid of its first layer, at that layer's height: where it starts.
    pub point: [f64; 3],
    /// Its first and last layers, from the plate up.
    pub layers: [usize; 2],
    /// The height of the layer under its first one (its underside lies between), and of its
    /// last layer.
    pub heights: [f64; 2],
    /// Its first layer's area, in mm² (a region's: over its layers).
    pub area_mm2: f64,
    /// Its first layer's footprint on the plate (a region's: over its layers): lowest and
    /// highest corners.
    pub footprint: [[f64; 2]; 2],
    /// It starts on the hollowed model's cavity ceiling, inside the model.
    pub inside: bool,
}

/// A finished search: the model, the plate's revision it describes and its islands, lowest
/// first.
struct Found {
    object: usize,
    revision: u64,
    islands: Vec<Island>,
}

/// What a search reports while it runs, and at its end.
enum Update {
    Progress(f32),
    Done(Result<Vec<Island>, String>),
}

/// A search running on its own thread.
struct Search {
    object: usize,
    revision: u64,
    started: Instant,
    progress: f32,
    stop: Arc<AtomicBool>,
    receiver: mpsc::Receiver<Update>,
}

impl Drop for Search {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// What a plan of the islands' supports is of: the model, the plate's revision, the values, the
/// one island Support it here asked for, and the surface for routes (by its identity).
#[derive(Clone, Debug, PartialEq)]
struct PlanKey {
    object: usize,
    revision: u64,
    values: plan::IslandSupports,
    only: Option<usize>,
    surface: usize,
}

/// A finished plan, and its preview's mesh and whether it is in the viewport.
struct Planned {
    key: PlanKey,
    outcome: plan::Outcome,
    preview: plan::Preview,
    uploaded: bool,
}

/// What the local minima are of: the model, its placement and its preparation's generation.
#[derive(Clone, Debug, PartialEq)]
struct MinimaKey {
    object: usize,
    placement: karasu_core::geometry::ObjectTransform,
    generation: u64,
}

/// Local minima worked out, or being worked out.
struct Minima {
    key: MinimaKey,
    points: Option<Vec<[f64; 3]>>,
    receiver: Option<mpsc::Receiver<Vec<[f64; 3]>>>,
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

/// The Islands tool's state.
#[allow(clippy::struct_excessive_bools)] // The tool's switches, each its own setting.
pub struct IslandsUi {
    /// The panel's Advanced switch (kept with the settings).
    pub advanced: bool,
    /// The islands' markers show (the strip's I, kept with the settings).
    pub show_islands: bool,
    /// The local minima's markers show (kept with the settings).
    pub show_minima: bool,
    /// The list window is open (kept with the settings).
    pub list_open: bool,
    /// The search's critical angle, in degrees from the plate (a tool value, from the
    /// overhangs' angle), and its default.
    pub angle_deg: f64,
    pub default_angle_deg: f64,
    /// Islands smaller than this are hidden, in mm² (a tool value, under Advanced): no marker,
    /// left out of the list, the stepping and the counts, which say how many are hidden. They
    /// stay found, and Support the islands still supports them unless its own "Skip islands
    /// under" leaves them out.
    pub hide_under_mm2: f64,
    /// The list window shows only the islands without a support.
    only_free: bool,
    /// Islands closer than this share a support, in mm: the Islands tool's value, as last
    /// polled; a support holds them alike.
    join_mm: f64,
    /// The list window's place: everywhere (0), outside (1), or in the cavity (2).
    place: usize,
    /// An island was chosen in the scene or the panel: the list scrolls to its row once.
    reveal: bool,
    /// Where searches write their slice; each removes its own file.
    cache: Option<PathBuf>,
    search: Option<Search>,
    found: Option<Found>,
    /// The chosen island, by its place in the found ones.
    at: usize,
    minima: Option<Minima>,
    uploaded: bool,
    /// The markers drawn last, for clicks: island places and their points.
    marks: Vec<(usize, [f64; 3])>,
    /// Where the view should go, once: the bounds to frame, flat at the height the section cuts
    /// the models at (owner, 2026-11-04: the island's first layer shows in section).
    view: Option<[[f64; 3]; 2]>,
    /// Support the islands: the supports planned under every island without one show, to
    /// apply or discard (the strip's U).
    auto: bool,
    /// The island Support it here asked for: its supports are added once planned.
    here: Option<usize>,
    planning: Option<plan::Job<PlanKey>>,
    planned: Option<Planned>,
    /// The preview in the viewport.
    preview_shown: bool,
    /// A plan waits for the model's surface for routes to be gridded.
    waiting: bool,
    /// Stepping to an island in the cavity turned the cavity view on: stepping to one outside
    /// turns it off again.
    cavity_turned_on: bool,
    outbox: Vec<Message>,
}

impl Default for IslandsUi {
    fn default() -> Self {
        Self::new()
    }
}

/// The application's folder for the islands' slices.
fn cache_folder() -> Option<PathBuf> {
    let paths = karasu_core::profiles::store::ProfilePaths::discover().ok()?;
    let folder = paths.cache.join("islands");
    std::fs::create_dir_all(&folder).ok()?;
    Some(folder)
}

/// A slice's name, starting `prefix`, no other search or plan uses, in this process or another
/// application running beside it: each removes its own file, and none empties the folder.
pub(crate) fn slice_name(prefix: &str) -> String {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    format!(
        "{prefix}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}

/// Whether an island at `point`, its layers' top at `top`, starts on the cavity's ceiling of the
/// hollowed `shell` (gridded by `grid`, its cavity triangles from `cavity_from`): the surface
/// straight above the air just under it is a cavity triangle. The same test places a support's
/// tip there, so the label and the support agree.
fn in_cavity(
    shell: &TriangleMesh,
    grid: &SurfaceGrid,
    cavity_from: usize,
    point: [f64; 3],
    top: f64,
) -> bool {
    grid.first_above(shell, point, top + PROBE_MM)
        .is_some_and(|(_, _, t)| t as usize >= cavity_from)
}

impl IslandsUi {
    /// The tool as it starts: islands and the list shown, the overhangs' default angle.
    #[must_use]
    pub fn new() -> Self {
        Self {
            advanced: false,
            show_islands: true,
            show_minima: false,
            list_open: true,
            angle_deg: crate::settings::DEFAULT_OVERHANG_ANGLE_DEG,
            default_angle_deg: crate::settings::DEFAULT_OVERHANG_ANGLE_DEG,
            hide_under_mm2: 0.0,
            only_free: false,
            join_mm: plan::DEFAULT_ISLAND_SUPPORTS.join_mm,
            place: 0,
            reveal: false,
            cache: None,
            search: None,
            found: None,
            at: 0,
            minima: None,
            uploaded: false,
            marks: Vec::new(),
            view: None,
            auto: false,
            here: None,
            planning: None,
            planned: None,
            preview_shown: false,
            waiting: false,
            cavity_turned_on: false,
            outbox: Vec::new(),
        }
    }

    /// The messages to show as notifications, taken.
    pub fn take_messages(&mut self) -> Vec<Message> {
        std::mem::take(&mut self.outbox)
    }

    /// Where the view should go, once: an island stepped to, framed with the section through its
    /// first layer.
    pub fn take_view(&mut self) -> Option<[[f64; 3]; 2]> {
        self.view.take()
    }

    /// Whether a search runs.
    #[cfg_attr(not(test), allow(dead_code))] // Tests wait on it; the footer reads the search.
    #[must_use]
    pub fn searching(&self) -> bool {
        self.search.is_some()
    }

    /// Model `index`'s islands as last found, and whether they still describe the plate.
    /// Whether `island` is shown: not smaller than [`Self::hide_under_mm2`].
    fn shows(&self, island: &Island) -> bool {
        island.area_mm2 >= self.hide_under_mm2
    }

    /// How many of `islands` are hidden, and the area they are smaller than, for the counts'
    /// note; `None` when none is.
    fn hidden_note(&self, islands: &[Island]) -> Option<String> {
        let hidden = islands.iter().filter(|i| !self.shows(i)).count();
        (hidden > 0).then(|| {
            tr!(
                "islands-hidden",
                count = hidden,
                area = decimal(self.hide_under_mm2, 2)
            )
        })
    }

    fn found(&self, workspace: &Workspace, index: Option<usize>) -> Option<(&[Island], bool)> {
        let found = self.found.as_ref().filter(|f| Some(f.object) == index)?;
        Some((&found.islands, found.revision == workspace.revision()))
    }

    /// Stop a running search: the islands found before stay.
    pub fn stop(&mut self) {
        self.search = None;
    }

    /// Find model `index`'s islands: slice it alone, without its supports, in the background.
    pub fn find(&mut self, workspace: &Workspace, preparation: &PreparationUi, index: usize) {
        if self.search.is_some() {
            return;
        }
        let Some((request, revision, output)) = self.request(workspace, index) else {
            return;
        };
        let Some(object) = workspace.plate().objects.get(index) else {
            return;
        };
        // A hollowed model's islands may start in its cavity: told by its prepared shell.
        let shell = preparation
            .shared(workspace, index)
            .map(|(shared, _)| shared)
            .filter(|_| object.hollowing().is_some());
        let pipeline = workspace.pipeline_cache();
        let stop = Arc::new(AtomicBool::new(false));
        let (sender, receiver) = mpsc::channel();
        let cancelled = Arc::clone(&stop);
        let spawned = std::thread::Builder::new()
            .name("islands".to_owned())
            .spawn(move || {
                let started = Instant::now();
                let mut observer = Progress {
                    sender: sender.clone(),
                    total: 0,
                };
                let stop = |_: Checkpoint| cancelled.load(Ordering::Relaxed);
                let result = slice_with(&request, Some(pipeline.as_ref()), &mut observer, &stop)
                    .map_err(|e| e.to_string())
                    .and_then(|outcome| {
                        let found = outcome
                            .unsupported
                            .ok_or_else(|| "the unsupported regions were not found".to_owned())?;
                        Ok(islands_of(&found, shell.as_deref(), index, &cancelled))
                    });
                let _ = std::fs::remove_file(&output);
                if let Ok(islands) = &result {
                    log::info!(
                        target: "supports",
                        "found {} island(s) in {:.2} s",
                        islands.len(),
                        started.elapsed().as_secs_f64()
                    );
                }
                let _ = sender.send(Update::Done(result));
            });
        match spawned {
            Ok(_) => {
                self.search = Some(Search {
                    object: index,
                    revision,
                    started: Instant::now(),
                    progress: 0.0,
                    stop,
                    receiver,
                });
            }
            Err(error) => log::warn!(target: "supports", "cannot start finding islands: {error}"),
        }
    }

    /// The request that finds model `index`'s islands, the plate revision it describes and the
    /// file it writes: the model alone, without its supports, so every island is found, held or
    /// not; a lifted model sliced where it stands, for the analysis only. It is written in the
    /// format Slice would write ([`crate::slicing_ui::plate_format`]): the reference format's
    /// images reach their size limit on a large print, and the printer's format is encoded on
    /// every core.
    fn request(
        &mut self,
        workspace: &Workspace,
        index: usize,
    ) -> Option<(karasu_project::request::SliceRequest, u64, PathBuf)> {
        if self.cache.is_none() {
            self.cache = cache_folder();
        }
        let Some(cache) = &self.cache else {
            self.outbox
                .push(Message::new(Level::Error, tr!("error-no-cache")));
            return None;
        };
        // The format Slice would write, and named as Slice names its output.
        let Some(format) = crate::slicing_ui::plate_format(workspace) else {
            self.outbox
                .push(Message::new(Level::Error, tr!("islands-no-format")));
            return None;
        };
        let name = slice_name("islands");
        let output = cache.join(match crate::slicing_ui::extension(&format) {
            Some(ext) => format!("{name}.{ext}"),
            None => format!("{name}-{}", karasu_project::request::REFERENCE_FORMAT),
        });
        match alone_request(workspace, index, output.clone(), format, self.angle_deg) {
            Ok(Some((request, revision))) => Some((request, revision, output)),
            Ok(None) => None,
            Err(error) => {
                self.outbox
                    .push(Message::new(Level::Error, tr!("islands-refused")).detail(error));
                None
            }
        }
    }

    /// Take a search's news and keep the local minima of model `selected` current while they
    /// show; `supports` tells which islands are held. Returns whether a repaint is needed soon.
    pub fn poll(
        &mut self,
        workspace: &Workspace,
        preparation: &PreparationUi,
        supports: &SupportsUi,
        selected: Option<usize>,
    ) -> bool {
        self.join_mm = supports.islands.join_mm;
        let searching = self.poll_search(workspace);
        let minima = self.poll_minima(workspace, preparation, selected);
        let planning = self.poll_plan(workspace, preparation, supports, selected);
        searching || minima || planning
    }

    /// Whether Support the islands shows its plan.
    #[must_use]
    pub fn supporting(&self) -> bool {
        self.auto
    }

    /// Show the supports planned under the islands, or leave them without adding any.
    pub fn toggle_supporting(&mut self) {
        self.auto = !self.auto;
        self.here = None;
        self.planning = None;
        self.planned = None;
    }

    /// Support it here: plan island `k`'s supports, added once planned.
    fn support_here(&mut self, k: usize) {
        self.auto = false;
        self.here = Some(k);
        self.planning = None;
        self.planned = None;
    }

    /// Keep the plan of model `selected`'s islands' supports current while Support the islands
    /// shows it, or Support it here waits for it: start one when the plate, the values or the
    /// surface for routes changed, and take a finished one. Returns whether a repaint is needed.
    fn poll_plan(
        &mut self,
        workspace: &Workspace,
        preparation: &PreparationUi,
        supports: &SupportsUi,
        selected: Option<usize>,
    ) -> bool {
        if !self.auto && self.here.is_none() {
            return false;
        }
        let Some(index) = selected else {
            return false;
        };
        if let Some(job) = &self.planning {
            match job.receiver.try_recv() {
                Ok((outcome, preview)) => {
                    let key = job.key.clone();
                    self.planning = None;
                    self.planned = Some(Planned {
                        key,
                        outcome,
                        preview,
                        uploaded: false,
                    });
                }
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => self.planning = None,
            }
        }
        let Some(surface) = supports.route_surface(workspace, preparation, index) else {
            // The surface is being gridded: the plan waits for it.
            self.waiting = true;
            return true;
        };
        self.waiting = false;
        let Some((islands, _)) = self.found(workspace, Some(index)) else {
            return false;
        };
        let values = supports.islands;
        let key = PlanKey {
            object: index,
            revision: workspace.revision(),
            values,
            only: self.here,
            surface: Arc::as_ptr(&surface) as usize,
        };
        if self.planned.as_ref().is_some_and(|p| p.key == key) {
            return false;
        }
        if self.planning.as_ref().is_some_and(|j| j.key == key) {
            return true;
        }
        let wanted = plan::wanted(
            workspace,
            index,
            islands,
            self.here,
            values.skip_under_mm2,
            values.join_mm,
        );
        let next = values.next(&supports.presets);
        let Some(printer) = workspace.printer() else {
            return false;
        };
        let volume = karasu_core::supports::BuildVolume {
            origin_xy_mm: printer.origin_xy_mm,
            size_mm: printer.build_size_mm,
        };
        // Checked as a slice builds them, against the model (owner, 2026-10-30).
        let checked = plan::check::model_input(workspace, preparation, supports, index).zip(
            workspace
                .model_to_world(index)
                .and_then(|to_world| to_world.inverse()),
        );
        self.planning = plan::start(
            key,
            wanted,
            surface,
            next,
            workspace.supports(index).cloned(),
            values,
            volume,
            checked,
        );
        true
    }

    /// The plan for model `index`, when it is current.
    fn plan_outcome(&self, index: Option<usize>) -> Option<&plan::Outcome> {
        self.planned
            .as_ref()
            .filter(|p| Some(p.key.object) == index && self.planning.is_none())
            .map(|p| &p.outcome)
    }

    /// Add the planned supports to model `index` as one undoable step: Apply, or Support it
    /// here's once planned. Returns how many were added.
    pub fn apply_plan(&mut self, workspace: &mut Workspace, index: usize) -> usize {
        let Some(planned) = self
            .planned
            .take()
            .filter(|p| p.key.object == index && p.key.revision == workspace.revision())
        else {
            return 0;
        };
        let ids = plan::apply(workspace, index, &planned.outcome);
        let outcome = &planned.outcome;
        let skipped = outcome.skipped.len();
        let text = match planned.key.only {
            Some(k) if ids.is_empty() => {
                let why = outcome
                    .skipped
                    .iter()
                    .find(|(island, _)| *island == k)
                    .map_or(plan::Skip::NoWay, |(_, why)| *why);
                // Too low: how much room there is, and how much more the model needs lifting.
                let island = self
                    .found
                    .as_ref()
                    .and_then(|f| f.islands.get(k))
                    .filter(|_| why == plan::Skip::TooLow);
                let detail = match island {
                    Some(island) => {
                        let room = island.point[2] - outcome.floor_mm;
                        tr!(
                            "islands-here-too-low",
                            room = decimal(room, 1),
                            needs = decimal(outcome.room_needed_mm, 1),
                            more = decimal((outcome.room_needed_mm - room).max(0.1), 1)
                        )
                    }
                    None => panel::reason(why),
                };
                self.outbox.push(
                    Message::new(Level::Warning, tr!("islands-here-none", number = k + 1))
                        .detail(detail),
                );
                self.auto = false;
                self.here = None;
                return 0;
            }
            Some(k) => tr!("islands-here-added", count = ids.len(), number = k + 1),
            None => tr!(
                "islands-supports-added",
                count = ids.len(),
                islands = outcome.held,
                skipped = skipped
            ),
        };
        // Islands left without a support are said, with why (the final check's too).
        let level = if skipped > 0 && planned.key.only.is_none() {
            Level::Warning
        } else {
            Level::Success
        };
        let mut message = Message::new(level, text);
        if let Some(hint) = (skipped > 0)
            .then(|| Self::skipped_of(&planned.outcome))
            .flatten()
        {
            message = message.detail(hint);
        }
        self.outbox.push(message);
        self.auto = false;
        self.here = None;
        ids.len()
    }

    /// Support it here's plan, once made: its supports are added to model `selected`.
    pub fn finish_here(&mut self, workspace: &mut Workspace, selected: Option<usize>) {
        let ready = self.here.is_some()
            && self.planning.is_none()
            && self
                .planned
                .as_ref()
                .is_some_and(|p| Some(p.key.object) == selected && p.key.only == self.here);
        if let (true, Some(index)) = (ready, selected) {
            self.apply_plan(workspace, index);
        }
    }

    fn poll_search(&mut self, workspace: &Workspace) -> bool {
        let Some(search) = &mut self.search else {
            return false;
        };
        let mut done = None;
        loop {
            match search.receiver.try_recv() {
                Ok(Update::Progress(share)) => search.progress = share,
                Ok(Update::Done(result)) => {
                    done = Some(result);
                    break;
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    done = Some(Err(tr!("islands-stopped")));
                    break;
                }
            }
        }
        let Some(result) = done else {
            return true;
        };
        let (object, revision, took) = (search.object, search.revision, search.started.elapsed());
        self.search = None;
        match result {
            Ok(islands) => {
                let found = Found {
                    object,
                    revision,
                    islands,
                };
                let count = found.islands.iter().filter(|i| self.shows(i)).count();
                let free = found
                    .islands
                    .iter()
                    .filter(|i| self.shows(i))
                    .filter(|i| holder(workspace, object, i, self.join_mm).is_none())
                    .count();
                let hidden = self.hidden_note(&found.islands);
                self.found = Some(found);
                self.at = 0;
                let (level, text) = if count == 0 {
                    (Level::Success, tr!("islands-found-none"))
                } else if free == 0 {
                    (Level::Success, tr!("islands-found-held", count = count))
                } else {
                    (
                        Level::Warning,
                        tr!("islands-found-free", count = count, free = free),
                    )
                };
                let took = tr!("islands-took", seconds = decimal(took.as_secs_f64(), 1));
                let detail = hidden.map_or_else(|| took.clone(), |h| format!("{h}\n{took}"));
                self.outbox.push(Message::new(level, text).detail(detail));
            }
            Err(reason) => {
                self.outbox
                    .push(Message::new(Level::Error, tr!("islands-failed")).detail(reason));
            }
        }
        true
    }

    /// Start or take the local minima of model `selected` while they show.
    fn poll_minima(
        &mut self,
        workspace: &Workspace,
        preparation: &PreparationUi,
        selected: Option<usize>,
    ) -> bool {
        let Some(index) = selected.filter(|_| self.show_minima) else {
            return false;
        };
        let (Some(placement), Some((shared, generation))) = (
            workspace.placement_of(index).map(|p| p.transform),
            preparation.shared(workspace, index),
        ) else {
            return false;
        };
        let key = MinimaKey {
            object: index,
            placement,
            generation,
        };
        if let Some(minima) = self.minima.as_mut().filter(|m| m.key == key) {
            if let Some(receiver) = &minima.receiver {
                match receiver.try_recv() {
                    Ok(points) => {
                        minima.points = Some(points);
                        minima.receiver = None;
                    }
                    Err(mpsc::TryRecvError::Empty) => return true,
                    Err(mpsc::TryRecvError::Disconnected) => minima.receiver = None,
                }
            }
            return false;
        }
        let Some(object) = workspace.plate().objects.get(index) else {
            return false;
        };
        let mesh = workspace
            .meshes()
            .get(&object.model)
            .map(|m| Arc::clone(&m.mesh));
        let (sender, receiver) = mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("minima".to_owned())
            .spawn(move || {
                let _ = sender.send(minima_of(&shared, index, mesh.as_deref(), &placement));
            });
        self.minima = spawned.ok().map(|_| Minima {
            key,
            points: None,
            receiver: Some(receiver),
        });
        true
    }

    /// The markers of model `focus`'s islands and minima, a constant size on screen as seen
    /// from `eye`, drawn through the model.
    #[allow(clippy::too_many_arguments)] // The viewport, the plate and the eye.
    pub fn draws(
        &mut self,
        viewport: &mut Viewport,
        state: &egui_wgpu::RenderState,
        workspace: &Workspace,
        focus: Option<usize>,
        eye: [f64; 3],
        appearance: &Appearance,
    ) -> Vec<Draw> {
        self.marks.clear();
        let Some(index) = focus else {
            return Vec::new();
        };
        if !self.uploaded {
            viewport.upload(&state.device, MARKER_KEY, &marker(), [0.0; 3]);
            self.uploaded = true;
        }
        let mut draws = Vec::new();
        draws.extend(self.plan_draw(viewport, state, index, appearance));
        // `through`: how much of the colour shows through the model in front of the marker.
        let mut mark = |point: [f64; 3], scale: f64, mut color: [f32; 4], through: f32| {
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
            let solid = color;
            color[3] *= through;
            for (color, overlay) in [(solid, false), (color, true)] {
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
        };
        if self.show_minima {
            let points = self
                .minima
                .as_ref()
                .filter(|m| m.key.object == index)
                .and_then(|m| m.points.as_ref());
            for &point in points.into_iter().flatten() {
                mark(point, 0.7, appearance.overhang_limit.linear(), 0.5);
            }
        }
        if self.show_islands {
            let mut marks = Vec::new();
            if let Some((islands, current)) = self.found(workspace, Some(index)) {
                for (k, island) in islands.iter().enumerate() {
                    // The chosen one is drawn last, over the others.
                    if k == self.at || !self.shows(island) {
                        continue;
                    }
                    let mut color = if holder(workspace, index, island, self.join_mm).is_some() {
                        appearance.hole_valid.linear()
                    } else {
                        appearance.support_error.linear()
                    };
                    if !current {
                        color[3] *= 0.45;
                    }
                    mark(island.point, 1.2, color, 0.5);
                    marks.push((k, island.point));
                }
                // The chosen island stands out among many (owner, 2026-10-29): the selection's
                // colour, larger, and whole through the model.
                if let Some(island) = islands.get(self.at).filter(|i| self.shows(i)) {
                    mark(island.point, 2.4, appearance.support_selected.linear(), 1.0);
                    marks.push((self.at, island.point));
                }
            }
            self.marks = marks;
        }
        draws
    }

    /// The supports planned under model `index`'s islands, while Support the islands shows
    /// them: uploaded once, and freed once gone.
    fn plan_draw(
        &mut self,
        viewport: &mut Viewport,
        state: &egui_wgpu::RenderState,
        index: usize,
        appearance: &Appearance,
    ) -> Option<Draw> {
        let planned = self
            .planned
            .as_mut()
            .filter(|p| p.key.object == index && p.key.only.is_none() && self.auto);
        match planned {
            Some(planned) => {
                if let Some((mesh, origin)) = &planned.preview {
                    if !planned.uploaded {
                        viewport.upload(&state.device, PLAN_KEY, mesh, *origin);
                        planned.uploaded = true;
                    }
                    self.preview_shown = true;
                    return Some(Draw {
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
        None
    }

    /// A click at `pointer` in the viewport `rect`: on an island's marker, it is chosen. Returns
    /// whether it was on one.
    pub fn click_marker(&mut self, camera: &Camera, rect: egui::Rect, pointer: egui::Pos2) -> bool {
        const REACH: f32 = 10.0;
        let size = [f64::from(rect.width()), f64::from(rect.height())];
        let nearest = self
            .marks
            .iter()
            .filter_map(|&(k, point)| {
                let [x, y] = camera.screen(point, size)?;
                #[allow(clippy::cast_possible_truncation)] // Screen positions.
                let at = rect.min + egui::vec2(x as f32, y as f32);
                let distance = at.distance(pointer);
                (distance <= REACH).then_some((distance, k))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0));
        let Some((_, k)) = nearest else {
            return false;
        };
        self.at = k;
        self.reveal = true;
        true
    }

    /// Choose island `k` of model `index`'s and bring the view to it; one in the cavity turns
    /// the cavity view on, and one outside turns it off again if stepping turned it on.
    fn go_to(&mut self, workspace: &Workspace, index: usize, k: usize, supports: &mut SupportsUi) {
        let Some((islands, _)) = self.found(workspace, Some(index)) else {
            return;
        };
        let Some(island) = islands.get(k).copied() else {
            return;
        };
        self.at = k;
        self.reveal = true;
        self.view = Some(around(&island));
        let hollowed = workspace
            .plate()
            .objects
            .get(index)
            .is_some_and(|o| o.hollowing().is_some());
        // The view stepping turned on goes off again for an island outside; one the user turned
        // on stays.
        if !supports.cavity_view {
            self.cavity_turned_on = false;
        }
        if island.inside && hollowed && !supports.cavity_view {
            supports.toggle_cavity_view();
            self.cavity_turned_on = true;
        } else if !island.inside && self.cavity_turned_on {
            supports.toggle_cavity_view();
            self.cavity_turned_on = false;
        }
    }
}

/// The request that slices model `index` alone, without its supports, into `output` in
/// `format`, finding its unsupported regions at `angle_deg`, and the plate revision it
/// describes: a lifted model is sliced where it stands, for the analysis only. `None` without
/// that model; an error when the plate cannot be sliced.
pub(crate) fn alone_request(
    workspace: &Workspace,
    index: usize,
    output: PathBuf,
    format: String,
    angle_deg: f64,
) -> Result<Option<(karasu_project::request::SliceRequest, u64)>, String> {
    let (mut request, revision) = workspace.request(output).map_err(|e| e.to_string())?;
    if let Subject::Plate(plate) = &mut request.subject {
        if index >= plate.objects.len() {
            return Ok(None);
        }
        let mut object = plate.objects.swap_remove(index);
        object.supports = None;
        plate.objects = vec![object];
    }
    request.allow_floating = true;
    request.format = format;
    request.find_unsupported = Some(angle_deg);
    Ok(Some((request, revision)))
}

/// The islands among `found`'s unsupported regions, lowest first: see [`found_of`].
fn islands_of(
    found: &Unsupported,
    shell: Option<&Preparation>,
    index: usize,
    stop: &AtomicBool,
) -> Vec<Island> {
    found_of(found, shell, index, stop).1
}

/// `found`'s unsupported regions with their kinds, each told as an [`Island`] is over all its
/// layers, and its islands, one where each piece starts in mid-air (owner, 2026-10-30: several
/// may grow into one region, each needing its support where it starts), lowest first. With
/// object `index`'s hollowed preparation in `shell`, a region or an island whose underside is a
/// cavity triangle starts on the cavity's ceiling ([`in_cavity`]); the shell is gridded once for
/// them all.
pub(crate) fn found_of(
    found: &Unsupported,
    shell: Option<&Preparation>,
    index: usize,
    stop: &AtomicBool,
) -> (Vec<(RegionKind, Island)>, Vec<Island>) {
    let hollow = shell
        .and_then(|s| s.objects.get(index))
        .and_then(|o| o.as_ref().ok())
        .and_then(|o| o.hollowed());
    let gridded = hollow.and_then(|model| {
        let mut stop = |_| stop.load(Ordering::Relaxed);
        let grid = SurfaceGrid::build(model.mesh(), &mut Cancellation::new(&mut stop)).ok()?;
        Some((model.mesh(), grid, model.exterior_faces()))
    });
    let inside = |point: [f64; 3], top: f64| {
        gridded
            .as_ref()
            .is_some_and(|(mesh, grid, from)| in_cavity(mesh, grid, *from, point, top))
    };
    let under_layer = |layer: usize| {
        layer
            .checked_sub(1)
            .and_then(|k| found.height(k))
            .unwrap_or(0.0)
    };
    let mut regions = Vec::new();
    let mut islands = Vec::new();
    for r in &found.report.regions {
        let point = found.world(r);
        let under = under_layer(r.first_layer);
        let top = found.height(r.last_layer).unwrap_or(point[2]);
        regions.push((
            r.kind,
            Island {
                point,
                layers: [r.first_layer, r.last_layer],
                heights: [under, top],
                area_mm2: found.area_mm2(r),
                footprint: found.bounds_mm(r),
                inside: inside([point[0], point[1], under - 1e-3], top),
            },
        ));
        for start in &r.starts {
            let point = found.start_world(start);
            let under = under_layer(start.layer);
            islands.push(Island {
                point,
                layers: [start.layer, r.last_layer],
                heights: [under, top],
                area_mm2: found.start_area_mm2(start),
                footprint: found.start_bounds_mm(start),
                inside: inside([point[0], point[1], under - 1e-3], top),
            });
        }
    }
    islands.sort_by(|a, b| {
        a.layers[0]
            .cmp(&b.layers[0])
            .then(a.point[0].total_cmp(&b.point[0]))
            .then(a.point[1].total_cmp(&b.point[1]))
    });
    (regions, islands)
}

/// The view's bounds around `island`, flat at the height the section cuts it: the top of its
/// first layer, where it starts in mid-air (owner, 2026-11-04), so that layer is the one seen.
/// The heights are the layers' planes, in their middles, and the section snaps to the nearest
/// layer boundary: half a layer above the first layer's plane is its top.
fn around(island: &Island) -> [[f64; 3]; 2] {
    let [[x0, y0], [x1, y1]] = island.footprint;
    let plane = island.point[2];
    let cut = plane + (plane - island.heights[0]) / 2.0;
    [
        [x0 - AROUND_MM, y0 - AROUND_MM, cut],
        [x1 + AROUND_MM, y1 + AROUND_MM, cut],
    ]
}

/// The support of model `index` holding `island` where it starts, islands closer than
/// `join_mm` sharing a support: the first whose tip does ([`plan::check::holds`]).
fn holder(workspace: &Workspace, index: usize, island: &Island, join_mm: f64) -> Option<u64> {
    let document = workspace.supports(index)?;
    let to_world = workspace.model_to_world(index)?;
    document
        .supports
        .iter()
        .find(|s| plan::check::holds(island, to_world.point(s.tip_mm), join_mm))
        .map(|s| s.id)
}

/// The local minima of model `index`'s prepared surface not resting on the plate, in world
/// millimetres: the hollowed shell's (its cavity's ceiling too), or `mesh` placed by `placement`.
fn minima_of(
    shared: &Preparation,
    index: usize,
    mesh: Option<&TriangleMesh>,
    placement: &karasu_core::geometry::ObjectTransform,
) -> Vec<[f64; 3]> {
    let hollow = shared
        .objects
        .get(index)
        .and_then(|o| o.as_ref().ok())
        .and_then(|o| o.hollowed());
    let found = match (hollow, mesh) {
        (Some(model), _) => local_minima(model.mesh(), &Affine3::identity()),
        (None, Some(mesh)) => match placement.to_affine() {
            Ok(affine) => local_minima(mesh, &affine),
            Err(_) => return Vec::new(),
        },
        (None, None) => return Vec::new(),
    };
    found.map_or_default(|minima| {
        minima
            .iter()
            .filter(|m| !m.on_plate)
            .map(|m| m.point.coordinates())
            .collect()
    })
}

#[cfg(test)]
mod tests;
