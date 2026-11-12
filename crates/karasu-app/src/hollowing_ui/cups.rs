//! Suction cups in the Drain holes tool (M6.4j, TD-114; owner mockup "Suction cups in the Drain
//! holes tool", 2026-10-31).
//!
//! **Find suction cups** slices the selected model as it will be exported, with its supports,
//! raft and drain holes, on a thread of its own and in the plate's format as Slice does, and
//! reads its layers' masks for the resin each layer encloses while it is peeled
//! ([`karasu_core::analysis::suction`]). The tool never searches by itself: a search reflects the
//! plate as it was, and once the plate changes its cups show as out of date until found again.
//!
//! **Shown:** only the cups at least as wide and as tall as the panel's filter, widest first;
//! the smaller ones stay found and counted, and the list window shows them all on demand.
//! Markers stand at each cup's cap, a constant size on screen, drawn through the model: one
//! colour without an opening, another with openings narrower than the minimum vent, faint when
//! out of date, the chosen one larger in the selection's colour. Choosing a cup frames it and
//! cuts the section at the top of the layer where its vent goes, so that layer shows.
//!
//! **Add a vent here** makes a new hole under the chosen cup's cap: from the outside straight
//! towards the plate, or, when that way ends on the plate (a model resting on it), sideways
//! through the nearest wall just above the cap. It is a new hole like a placed one, checked and
//! cut by Apply holes.
use super::{HoleRef, HollowingUi, Pending, HOLE_MARGIN_MM};
use crate::{
    appearance::Appearance,
    camera::{Camera, Mat4, Ray},
    i18n::{decimal, tr},
    messages::{Level, Message},
    preparation_ui::PreparationUi,
    repair_panel::{marker, MARKER_SIZE},
    viewport::{Draw, Viewport},
    workspace::Workspace,
};
use eframe::{egui, egui_wgpu};
use karasu_core::{
    analysis::suction::Opening,
    cancellation::Checkpoint,
    preparation::{
        sampled::{axis_tilt, surface_hole_outward, HoleSurface},
        MAX_HOLE_TILT_DEG,
    },
};
use karasu_project::{
    pipeline::{slice_with, Observer, SuctionCups},
    request::{DrainHole, Subject},
};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    time::Instant,
};

/// The viewport mesh key of the cups' marker.
const MARKER_KEY: u64 = 0x5355_4354_494F_4E4D; // "SUCTIONM"
/// The smallest vents the search takes, in mm².
pub const MIN_VENT_MM2: std::ops::RangeInclusive<f64> = 0.0..=100.0;
/// The smallest vent a search credits until the user sets one, in mm²: a circle about 1.1 mm
/// across (TD-114).
pub const DEFAULT_MIN_VENT_MM2: f64 = 1.0;
/// The display filter's widths, in mm², and heights, in mm.
pub const SHOW_FROM_MM2: std::ops::RangeInclusive<f64> = 0.0..=1000.0;
pub const SHOW_TALLER_MM: std::ops::RangeInclusive<f64> = 0.0..=100.0;
/// The display filter until the user sets one (owner, 2026-10-31): on the owner's hollowed
/// `HillGiant_32mm.stl` it shows 9 of 1319 cups.
pub const DEFAULT_SHOW_FROM_MM2: f64 = 1.0;
pub const DEFAULT_SHOW_TALLER_MM: f64 = 0.5;
/// How far a vent reaches past the cap into its cup, in mm.
const INTO_CUP_MM: f64 = 0.5;
/// How far above a cup's first layer's bottom a vent's way starts, in mm: inside its resin.
const ABOVE_CAP_MM: f64 = 0.01;
/// How close to a resting model's lowest point a way down ends on the plate, in mm.
const ON_PLATE_MM: f64 = 0.05;
/// The directions tried sideways, round the vertical.
const SIDES: usize = 16;
/// How far a vent's way is followed, in mm.
const REACH_MM: f64 = 200.0;

/// A separately capped part of a cup, in world millimetres: each needs its own vent (owner,
/// 2026-10-31).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Part {
    /// The centroid of its first layer, at that layer's bottom: its cap.
    pub cap: [f64; 3],
    /// Where a vent opens into it wide enough: where a circle of the minimum vent first fits in
    /// it, at that layer's bottom. Above a narrow tip, a vent at the cap itself would open into
    /// it only through a passage too small.
    pub vent: [f64; 3],
    /// The top of the layer its vent goes in, in mm: where the section cuts it, so that layer is
    /// the one seen (the section snaps to the nearest layer boundary; owner, 2026-11-04).
    pub vent_top_mm: f64,
    /// Its first layer, and its last as a part of its own.
    pub layers: [usize; 2],
    /// The bottom of its first layer and the top of its last.
    pub heights: [f64; 2],
    /// Its widest layer's area as a part of its own, in mm².
    pub area_mm2: f64,
    /// Its volume as a part of its own, in mm³.
    pub volume_mm3: f64,
    /// Its first layer's footprint: lowest and highest corners.
    pub footprint: [[f64; 2]; 2],
}

impl Part {
    /// How tall it is as a part of its own, in mm.
    #[must_use]
    pub fn height_mm(&self) -> f64 {
        self.heights[1] - self.heights[0]
    }

    /// The height the section cuts it at: the top of the layer where its vent goes, so that
    /// layer shows, near the cup's bottom, where it is plainly closed (owner, 2026-11-02: higher
    /// up, a cup whose openings are too narrow already shows them, and looks open).
    fn cut_mm(&self) -> f64 {
        self.vent_top_mm.min(self.heights[1])
    }

    /// What the view frames to show it: its first layer, the section through the layer where
    /// its vent goes.
    fn around(&self) -> [[f64; 3]; 2] {
        let [[x0, y0], [x1, y1]] = self.footprint;
        let z = self.cut_mm();
        [[x0, y0, z], [x1, y1, z]]
    }
}

/// One cup of a search, in world millimetres.
#[derive(Clone, Debug, PartialEq)]
pub struct Cup {
    /// The centroid of its first layer, at that layer's bottom: its cap, where a vent relieves
    /// it.
    pub cap: [f64; 3],
    /// Its first and last layers, from the plate up.
    pub layers: [usize; 2],
    /// The bottom of its first layer and the top of its last.
    pub heights: [f64; 2],
    /// The height of its widest layer's plane: where the section cuts it.
    pub widest_mm: f64,
    /// Its widest layer's area, in mm²: what the film pulls on.
    pub area_mm2: f64,
    /// Its volume, in mm³.
    pub volume_mm3: f64,
    /// Its openings.
    pub opening: Opening,
    /// The layers close over it, with its resin trapped.
    pub closes: bool,
    /// Its footprint on the plate over its layers: lowest and highest corners.
    pub footprint: [[f64; 2]; 2],
    /// Its separately capped parts, oldest first: the first is its own cap's.
    pub parts: Vec<Part>,
}

impl Cup {
    /// How tall it is, in mm.
    #[must_use]
    pub fn height_mm(&self) -> f64 {
        self.heights[1] - self.heights[0]
    }

    /// What the view frames to show it: its footprint, the section through the layer where its
    /// own part's vent goes.
    fn around(&self) -> [[f64; 3]; 2] {
        let [[x0, y0], [x1, y1]] = self.footprint;
        let z = self.parts.first().map_or(self.cap[2], Part::cut_mm);
        [[x0, y0, z], [x1, y1, z]]
    }
}

/// A finished search: the model, the plate's revision it describes and its cups, widest first.
struct Found {
    object: usize,
    revision: u64,
    cups: Vec<Cup>,
}

/// What a search reports while it runs, and at its end.
enum Update {
    Progress(f32),
    Done(Result<Vec<Cup>, String>),
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

/// The suction cups' state in the Drain holes tool.
#[allow(clippy::struct_excessive_bools)] // Its switches, each its own setting or state.
pub struct CupsUi {
    /// The markers show (the strip's suction cups, kept with the settings).
    pub show: bool,
    /// The smallest opening that vents a cup, in mm² (a tool value: from the settings each time
    /// the tool opens).
    pub min_vent_mm2: f64,
    /// The display filter: the cups shown are at least this wide, in mm², and this tall, in mm
    /// (kept with the settings).
    pub show_from_mm2: f64,
    pub show_taller_mm: f64,
    /// The list window lists every cup, not only those the filter shows.
    pub(super) all: bool,
    /// The chosen cup, by its place among the found ones.
    pub(super) at: usize,
    /// The chosen cup's chosen part, by its place among its parts.
    pub(super) part: usize,
    /// A cup was chosen in the scene or the panel: the list scrolls to its row once.
    pub(super) reveal: bool,
    /// Where searches write their slice; each removes its own file.
    cache: Option<PathBuf>,
    search: Option<Search>,
    found: Option<Found>,
    uploaded: bool,
    /// The markers drawn last, for clicks: cup and part places and their points.
    marks: Vec<(usize, usize, [f64; 3])>,
    /// A cup to frame and cut through, once.
    region: Option<[[f64; 3]; 2]>,
    pub(super) outbox: Vec<Message>,
}

impl Default for CupsUi {
    fn default() -> Self {
        Self {
            show: true,
            min_vent_mm2: DEFAULT_MIN_VENT_MM2,
            show_from_mm2: DEFAULT_SHOW_FROM_MM2,
            show_taller_mm: DEFAULT_SHOW_TALLER_MM,
            all: false,
            at: 0,
            part: 0,
            reveal: false,
            cache: None,
            search: None,
            found: None,
            uploaded: false,
            marks: Vec::new(),
            region: None,
            outbox: Vec::new(),
        }
    }
}

/// The application's folder for the searches' slices.
fn cache_folder() -> Option<PathBuf> {
    let paths = karasu_core::profiles::store::ProfilePaths::discover().ok()?;
    let folder = paths.cache.join("suction");
    std::fs::create_dir_all(&folder).ok()?;
    Some(folder)
}

/// The cups of `found`, widest first.
fn cups_of(found: &SuctionCups) -> Vec<Cup> {
    let mut cups: Vec<Cup> = found
        .report
        .cups
        .iter()
        .map(|c| {
            let bottom = found.slab(c.first_layer).map_or(f64::NAN, |s| s[0]);
            let [x, y, _] = found.cap_world(c);
            Cup {
                cap: [x, y, bottom],
                layers: [c.first_layer, c.last_layer],
                heights: [bottom, found.slab(c.last_layer).map_or(f64::NAN, |s| s[1])],
                widest_mm: found.height(c.largest_layer).unwrap_or(bottom),
                area_mm2: found.largest_area_mm2(c),
                volume_mm3: c.volume_mm3,
                opening: c.opening,
                closes: c.closes,
                footprint: found.bounds_mm(c),
                parts: c
                    .caps
                    .iter()
                    .map(|p| {
                        let cap = found.cap_point(p);
                        #[allow(clippy::cast_precision_loss)] // Pixel counts.
                        let area_mm2 = p.largest_pixels as f64 * found.pixel_mm2;
                        Part {
                            cap,
                            vent: found.vent_point(p),
                            vent_top_mm: found.slab(p.vent_layer).map_or(f64::NAN, |s| s[1]),
                            layers: [p.layer, p.last_layer],
                            heights: [cap[2], found.slab(p.last_layer).map_or(f64::NAN, |s| s[1])],
                            area_mm2,
                            volume_mm3: p.volume_mm3,
                            footprint: found.cap_bounds_mm(p),
                        }
                    })
                    .collect(),
            }
        })
        .collect();
    cups.sort_by(|a, b| {
        b.area_mm2
            .total_cmp(&a.area_mm2)
            .then(a.layers[0].cmp(&b.layers[0]))
    });
    cups
}

impl CupsUi {
    /// Model `index`'s cups as last found, and whether they still describe the plate.
    pub(super) fn found(
        &self,
        workspace: &Workspace,
        index: Option<usize>,
    ) -> Option<(&[Cup], bool)> {
        let found = self.found.as_ref().filter(|f| Some(f.object) == index)?;
        Some((&found.cups, found.revision == workspace.revision()))
    }

    /// What the last search found: how many cups the filter shows, and whether it still
    /// describes the plate.
    pub(super) fn last(&self, workspace: &Workspace) -> Option<(usize, bool)> {
        let found = self.found.as_ref()?;
        Some((
            self.shown(&found.cups).len(),
            found.revision == workspace.revision(),
        ))
    }

    /// The places of the cups the filter shows, widest first.
    pub(super) fn shown(&self, cups: &[Cup]) -> Vec<usize> {
        (0..cups.len())
            .filter(|&k| {
                cups[k].area_mm2 >= self.show_from_mm2 && cups[k].height_mm() >= self.show_taller_mm
            })
            .collect()
    }

    /// The places of `cup`'s parts that need a vent: its own cap's, and those at least as wide
    /// and as tall as the filter.
    pub(super) fn shown_parts(&self, cup: &Cup) -> Vec<usize> {
        (0..cup.parts.len())
            .filter(|&k| {
                k == 0
                    || (cup.parts[k].area_mm2 >= self.show_from_mm2
                        && cup.parts[k].height_mm() >= self.show_taller_mm)
            })
            .collect()
    }

    /// Whether a search runs, and its share done for model `index`.
    pub(super) fn progress(&self, index: Option<usize>) -> Option<f32> {
        self.search
            .as_ref()
            .filter(|s| Some(s.object) == index)
            .map(|s| s.progress)
    }

    /// Whether a search runs.
    #[cfg_attr(not(test), allow(dead_code))] // Tests wait on it; the footer reads the search.
    #[must_use]
    pub fn searching(&self) -> bool {
        self.search.is_some()
    }

    /// Stop a running search: the cups found before stay.
    pub fn stop(&mut self) {
        self.search = None;
    }

    /// The cup to frame and cut through, once.
    pub fn take_region(&mut self) -> Option<[[f64; 3]; 2]> {
        self.region.take()
    }

    /// Find model `index`'s cups: slice it as it will be exported, in the background.
    pub(super) fn find(&mut self, workspace: &Workspace, index: usize) {
        if self.search.is_some() {
            return;
        }
        let Some((request, revision, output)) = self.request(workspace, index) else {
            return;
        };
        let pipeline = workspace.pipeline_cache();
        let stop = Arc::new(AtomicBool::new(false));
        let (sender, receiver) = mpsc::channel();
        let cancelled = Arc::clone(&stop);
        let spawned = std::thread::Builder::new()
            .name("suction".to_owned())
            .spawn(move || {
                let mut observer = Progress {
                    sender: sender.clone(),
                    total: 0,
                };
                let stop = |_: Checkpoint| cancelled.load(Ordering::Relaxed);
                let result = slice_with(&request, Some(pipeline.as_ref()), &mut observer, &stop)
                    .map_err(|e| e.to_string())
                    .and_then(|outcome| {
                        let found = outcome
                            .suction_cups
                            .ok_or_else(|| "the suction cups were not found".to_owned())?;
                        Ok(cups_of(&found))
                    });
                let _ = std::fs::remove_file(&output);
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
            Err(error) => log::warn!(target: "suction", "cannot start finding cups: {error}"),
        }
    }

    /// The request that finds model `index`'s cups, the plate revision it describes and the
    /// file it writes: the model as it will be exported, with its supports, raft and holes,
    /// in the format Slice would write. A model lifted without supports is sliced where it
    /// stands, for the analysis only.
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
        let Some(format) = crate::slicing_ui::plate_format(workspace) else {
            self.outbox
                .push(Message::new(Level::Error, tr!("islands-no-format")));
            return None;
        };
        let name = crate::islands_ui::slice_name("suction");
        let output = cache.join(match crate::slicing_ui::extension(&format) {
            Some(ext) => format!("{name}.{ext}"),
            None => format!("{name}-{}", karasu_project::request::REFERENCE_FORMAT),
        });
        let (mut request, revision) = match workspace.request(output.clone()) {
            Ok(made) => made,
            Err(error) => {
                self.outbox.push(
                    Message::new(Level::Error, tr!("cups-refused")).detail(error.to_string()),
                );
                return None;
            }
        };
        if let Subject::Plate(plate) = &mut request.subject {
            if index >= plate.objects.len() {
                return None;
            }
            let object = plate.objects.swap_remove(index);
            plate.objects = vec![object];
        }
        request.allow_floating = true;
        request.format = format;
        request.find_suction_cups = Some(self.min_vent_mm2);
        Some((request, revision, output))
    }

    /// Take a search's news. Returns whether a repaint is needed soon.
    pub fn poll(&mut self) -> bool {
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
                    done = Some(Err(tr!("cups-stopped")));
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
            Ok(cups) => {
                let shown = self.shown(&cups).len();
                let total = cups.len();
                log::info!(
                    target: "suction",
                    "found {total} suction cup(s), {shown} shown, in {:.2} s",
                    took.as_secs_f64()
                );
                let (level, text) = if shown == 0 {
                    (Level::Success, tr!("cups-found-none", count = total))
                } else {
                    (
                        Level::Warning,
                        tr!("cups-found", count = total, shown = shown),
                    )
                };
                self.outbox.push(Message::new(level, text).detail(tr!(
                    "islands-took",
                    seconds = decimal(took.as_secs_f64(), 1)
                )));
                self.at = self.shown(&cups).first().copied().unwrap_or(0);
                self.part = 0;
                self.found = Some(Found {
                    object,
                    revision,
                    cups,
                });
            }
            Err(reason) => {
                self.outbox
                    .push(Message::new(Level::Error, tr!("cups-failed")).detail(reason));
            }
        }
        true
    }

    /// Choose cup `k` of model `index` and frame it, the section through its widest layer.
    pub(super) fn go_to(&mut self, workspace: &Workspace, index: usize, k: usize) {
        let Some(region) = self
            .found(workspace, Some(index))
            .and_then(|(cups, _)| cups.get(k).map(Cup::around))
        else {
            return;
        };
        self.at = k;
        self.part = 0;
        self.reveal = true;
        self.region = Some(region);
    }

    /// Choose part `part` of cup `k` of model `index` and frame it, the section just above its
    /// cap.
    pub(super) fn go_to_part(
        &mut self,
        workspace: &Workspace,
        index: usize,
        k: usize,
        part: usize,
    ) {
        let Some(region) = self
            .found(workspace, Some(index))
            .and_then(|(cups, _)| cups.get(k)?.parts.get(part).map(Part::around))
        else {
            return;
        };
        self.at = k;
        self.part = part;
        self.reveal = true;
        self.region = Some(region);
    }

    /// The markers of model `focus`'s shown cups, a constant size on screen as seen from `eye`,
    /// drawn through the model.
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
        let Some(index) = focus.filter(|_| self.show) else {
            return Vec::new();
        };
        if !self.uploaded {
            viewport.upload(&state.device, MARKER_KEY, &marker(), [0.0; 3]);
            self.uploaded = true;
        }
        let Some((cups, current)) = self.found(workspace, Some(index)) else {
            return Vec::new();
        };
        let mut shown = self.shown(cups);
        // A cup chosen in the list under the filter shows while it is chosen.
        if self.at < cups.len() && !shown.contains(&self.at) {
            shown.push(self.at);
        }
        let points: Vec<(usize, &Cup)> = shown.iter().map(|&k| (k, &cups[k])).collect();
        let mut draws = Vec::new();
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
        let colour = |cup: &Cup| {
            let mut color = match cup.opening {
                Opening::None => appearance.suction_cup.linear(),
                Opening::TooSmall => appearance.suction_cup_narrow.linear(),
            };
            if !current {
                color[3] *= 0.45;
            }
            color
        };
        let mut marks = Vec::new();
        // Each marker stands where a vent goes: its part's vent point.
        for &(k, cup) in points.iter().filter(|(k, _)| *k != self.at) {
            let at = cup.parts.first().map_or(cup.cap, |p| p.vent);
            mark(at, 1.2, colour(cup), 0.5);
            marks.push((k, 0, at));
        }
        // The chosen cup stands out among many: each of its parts that needs a vent in its
        // colour, and the chosen part in the selection's colour, larger, whole through the
        // model; drawn last, over the others.
        if let Some(&(k, cup)) = points.iter().find(|(k, _)| *k == self.at) {
            let parts = self.shown_parts(cup);
            for &p in parts.iter().filter(|&&p| p != self.part) {
                mark(cup.parts[p].vent, 1.6, colour(cup), 1.0);
                marks.push((k, p, cup.parts[p].vent));
            }
            if let Some(part) = cup.parts.get(self.part) {
                mark(part.vent, 2.4, appearance.support_selected.linear(), 1.0);
                marks.push((k, self.part, part.vent));
            }
        }
        self.marks = marks;
        draws
    }

    /// A click at `pointer` in the viewport `rect`: on a marker of model `index`'s cups, its cup
    /// and part are chosen and framed. Returns whether it was on one.
    pub fn click_marker(
        &mut self,
        workspace: &Workspace,
        index: Option<usize>,
        camera: &Camera,
        rect: egui::Rect,
        pointer: egui::Pos2,
    ) -> bool {
        const REACH: f32 = 10.0;
        let size = [f64::from(rect.width()), f64::from(rect.height())];
        let nearest = self
            .marks
            .iter()
            .filter_map(|&(k, part, point)| {
                let [x, y] = camera.screen(point, size)?;
                #[allow(clippy::cast_possible_truncation)] // Screen positions.
                let at = rect.min + egui::vec2(x as f32, y as f32);
                let distance = at.distance(pointer);
                (distance <= REACH).then_some((distance, k, part))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0));
        let (Some((_, k, part)), Some(index)) = (nearest, index) else {
            return false;
        };
        if part == 0 {
            self.go_to(workspace, index, k);
        } else {
            self.go_to_part(workspace, index, k, part);
        }
        true
    }
}

/// A vent's way out: where it leaves the model, the way's direction (from the cup out) and how
/// deep the hole goes, in world millimetres.
#[derive(Clone, Copy, Debug)]
struct Way {
    exit: [f64; 3],
    direction: [f64; 3],
    depth: f64,
}

/// Where `direction` from `origin` first leaves model `index`'s surface (its outward normal
/// along the way) within [`REACH_MM`], how far that is and the normal there: hits entering the
/// model, or on other models, are passed through.
fn way_out(
    workspace: &mut Workspace,
    index: usize,
    origin: [f64; 3],
    direction: [f64; 3],
) -> Option<([f64; 3], f64, [f64; 3])> {
    /// How far past a hit the next search starts, in mm.
    const PAST_MM: f64 = 1e-4;
    let mut from = origin;
    for _ in 0..64 {
        let hit = workspace.hit(&Ray {
            origin: from,
            direction,
        })?;
        let distance = (0..3)
            .map(|k| (hit.point[k] - origin[k]).powi(2))
            .sum::<f64>()
            .sqrt();
        if distance > REACH_MM {
            return None;
        }
        let along: f64 = (0..3).map(|k| hit.normal[k] * direction[k]).sum();
        if hit.object == index && along > 0.0 {
            return Some((hit.point, distance, hit.normal));
        }
        from = std::array::from_fn(|k| hit.point[k] + direction[k] * PAST_MM);
    }
    None
}

/// The ways out for a vent of `part` in model `index`, best first, for a hole `diameter` mm
/// across, from where a vent opens into the part wide enough.
///
/// The ways tried: straight down, then down at 45° and level, each round the vertical in
/// [`SIDES`] directions, the slanted and level ones from a radius above that point (or half the
/// part's height left, when lower). Each is followed to where it leaves the model past any
/// material on the way: from a cavity, through its wall; from a crevice of the surface, through
/// the crevice's wall. A way that ends on the plate under a resting model is left out. The
/// shortest goes first, straight down taking a quarter off its length (owner, 2026-11-02:
/// straight down was taken however long, through the whole model under a crevice); ways whose
/// face leans past [`MAX_HOLE_TILT_DEG`] from them go last, since their hole may not turn
/// along them.
fn ways(workspace: &mut Workspace, index: usize, part: &Part, diameter: f64) -> Vec<Way> {
    /// The share of its length a way straight down counts for: it is preferred.
    const DOWN_SHARE: f64 = 0.75;
    let Some(lowest) = workspace.placement_of(index).map(|p| p.min.z) else {
        return Vec::new();
    };
    let on_plate = |exit: [f64; 3]| lowest <= ON_PLATE_MM && exit[2] <= lowest + ON_PLATE_MM;
    let start = [part.vent[0], part.vent[1], part.vent[2] + ABOVE_CAP_MM];
    let rise = (diameter / 2.0)
        .min((part.heights[1] - part.vent[2]) / 2.0)
        .max(ABOVE_CAP_MM);
    let centre = [part.vent[0], part.vent[1], part.vent[2] + rise];
    let slant = std::f64::consts::FRAC_1_SQRT_2;
    let mut tries = vec![(start, [0.0, 0.0, -1.0], ABOVE_CAP_MM)];
    for (level, drop) in [(slant, -slant), (1.0, 0.0)] {
        for side in 0..SIDES {
            #[allow(clippy::cast_precision_loss)] // A few directions.
            let angle = std::f64::consts::TAU * side as f64 / SIDES as f64;
            tries.push((
                centre,
                [level * angle.cos(), level * angle.sin(), drop],
                0.0,
            ));
        }
    }
    let mut ways: Vec<(bool, f64, Way)> = tries
        .into_iter()
        .enumerate()
        .filter_map(|(k, (from, direction, above))| {
            let (exit, distance, normal) = way_out(workspace, index, from, direction)?;
            if on_plate(exit) {
                return None;
            }
            let along: f64 = (0..3).map(|c| normal[c] * direction[c]).sum();
            let leans = along.clamp(-1.0, 1.0).acos().to_degrees() > MAX_HOLE_TILT_DEG;
            let depth = distance + above + INTO_CUP_MM;
            let rank = if k == 0 { depth * DOWN_SHARE } else { depth };
            Some((
                leans,
                rank,
                Way {
                    exit,
                    direction,
                    depth,
                },
            ))
        })
        .collect();
    ways.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
    ways.into_iter().map(|(_, _, way)| way).collect()
}

/// The surface hole that cuts along `way` into model `index` (placed by `map`), `diameter` mm
/// across, in its model frame, or `None` when its hole cannot turn along it. The hole's axis
/// follows the area-weighted normal of the faces within its radius where it starts, as
/// drainage resolves it, turned by a tilt onto the way: a single face's normal may lean far
/// from it on a sculpted surface, and the hole then misses the cup.
fn hole_along(
    workspace: &Workspace,
    index: usize,
    map: &crate::gizmo::Affine,
    way: &Way,
    diameter: f64,
) -> Option<DrainHole> {
    let back = map.inverse()?;
    let foot = back.point(way.exit);
    let object = workspace.plate().objects.get(index)?;
    let mesh = &workspace.meshes().get(&object.model)?.mesh;
    let outward = surface_hole_outward(mesh, foot, diameter / 2.0)?;
    let inward = back.vector(way.direction.map(|c| -c));
    let tilt = axis_tilt(outward, inward)?;
    if tilt.angle_deg > MAX_HOLE_TILT_DEG {
        return None;
    }
    Some(DrainHole::Surface {
        surface_mm: foot,
        diameter_mm: diameter,
        outside_mm: HOLE_MARGIN_MM,
        inside_mm: HOLE_MARGIN_MM,
        tilt: (tilt.angle_deg > 1e-6).then_some(tilt),
        depth_mm: Some(way.depth),
        surface: HoleSurface::Outer,
    })
}

impl HollowingUi {
    /// Add a vent under the chosen part of model `index`'s chosen cup, or under each of its
    /// parts that need one when `every`: new holes, the last selected, checked like placed
    /// ones. Says how many parts have no way out for one.
    pub(super) fn add_vents(
        &mut self,
        workspace: &mut Workspace,
        preparation: &PreparationUi,
        index: usize,
        every: bool,
    ) {
        let Some(cup) = self
            .cups
            .found(workspace, Some(index))
            .and_then(|(cups, current)| cups.get(self.cups.at).filter(|_| current).cloned())
        else {
            return;
        };
        let parts = if every {
            self.cups.shown_parts(&cup)
        } else {
            vec![self.cups.part]
        };
        let mut missed = 0_usize;
        for p in parts {
            let Some(part) = cup.parts.get(p) else {
                continue;
            };
            if !self.add_vent(workspace, preparation, index, part) {
                missed += 1;
            }
        }
        if missed > 0 {
            self.cups.outbox.push(Message::new(
                Level::Warning,
                tr!("cups-no-vent", count = missed),
            ));
        }
    }

    /// Add a vent under `part` of model `index`: a new hole, selected. Returns whether there
    /// was a way out for one.
    fn add_vent(
        &mut self,
        workspace: &mut Workspace,
        preparation: &PreparationUi,
        index: usize,
        part: &Part,
    ) -> bool {
        let diameter = self.hole_mm;
        let Some(map) = workspace.model_to_world(index) else {
            return false;
        };
        let made = ways(workspace, index, part, diameter)
            .iter()
            .find_map(|way| hole_along(workspace, index, &map, way, diameter));
        let Some(hole) = made else {
            return false;
        };
        self.check_pending(workspace);
        self.pending.push(Pending {
            object: index,
            hole,
            verdict: None,
            mesh: None,
            solid: None,
        });
        let position = self.pending.len() - 1;
        self.pending_among = workspace.plate().objects.len();
        self.recheck(workspace, preparation, position);
        self.selected = Some((index, HoleRef::New(position)));
        self.reveal_hole = true;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn a_closed_cavity_is_found_framed_and_vented_through_its_side_on_the_plate() {
        let folder = std::env::temp_dir().join(format!("karasu-app-cups-{}", std::process::id()));
        // An 8 mm cube hollowed at 1.5 mm, resting on the plate, without a hole.
        let mut workspace = crate::slicing_ui::tests::hollowed_cube(&folder);
        let preparation = PreparationUi::default();
        let mut ui = HollowingUi::default();
        ui.cups.cache = Some(folder.clone());
        ui.cups.find(&workspace, 0);
        assert!(ui.cups.searching());
        let started = Instant::now();
        while ui.cups.searching() {
            ui.cups.poll();
            assert!(started.elapsed() < Duration::from_secs(300), "no cups");
            std::thread::sleep(Duration::from_millis(20));
        }
        let (cups, current) = ui
            .cups
            .found(&workspace, Some(0))
            .unwrap_or_else(|| panic!("no cups: {:?}", ui.cups.outbox));
        assert!(current);
        // The cavity, 5 mm across and tall over the 1.5 mm floor, is one cup that closes.
        let cavity = cups[0].clone();
        assert!((cavity.area_mm2 - 25.0).abs() < 1.5, "{cavity:?}");
        assert!((cavity.heights[0] - 1.5).abs() < 0.1, "{cavity:?}");
        assert!((cavity.height_mm() - 5.0).abs() < 0.2, "{cavity:?}");
        assert_eq!(cavity.opening, Opening::None);
        assert!(cavity.closes);
        assert_eq!(ui.cups.shown(cups), vec![0]);
        // Chosen, it is framed with the section through the layer where its vent goes.
        ui.cups.go_to(&workspace, 0, 0);
        let region = ui.cups.take_region().expect("framed");
        assert_eq!(region[0][2], cavity.parts[0].cut_mm());
        // At the top of that layer, so the section, snapped to the nearest layer boundary,
        // shows it rather than the one under it (owner, 2026-11-04).
        let mut section = crate::section_ui::SectionUi::default();
        section.cut_above(&mut workspace, region[0][2]);
        let above = section.section(&mut workspace).above.unwrap();
        assert!(above > cavity.parts[0].vent[2] + 1e-6, "{above} {cavity:?}");
        assert!(
            (above - cavity.parts[0].vent_top_mm).abs() < 1e-9,
            "{above} {cavity:?}"
        );
        // Its vent: down ends on the plate, so it goes sideways through the nearest wall, a
        // radius above the cavity's floor, 4 mm from the cap and half a millimetre into it.
        let placement = workspace.placement_of(0).expect("placed");
        ui.hole_mm = 2.0;
        ui.add_vents(&mut workspace, &preparation, 0, false);
        assert_eq!(ui.pending.len(), 1, "{:?}", ui.cups.outbox);
        let map = workspace.model_to_world(0).expect("placed");
        let DrainHole::Surface {
            surface_mm,
            depth_mm: Some(depth),
            surface: HoleSurface::Outer,
            ..
        } = ui.pending[0].hole
        else {
            panic!("{:?}", ui.pending[0].hole);
        };
        let at = map.point(surface_mm);
        let sides = [
            (at[0], placement.min.x, placement.max.x),
            (at[1], placement.min.y, placement.max.y),
        ];
        let on_side = sides
            .iter()
            .any(|&(v, low, high)| (v - low).abs() < 1e-3 || (v - high).abs() < 1e-3);
        assert!(on_side, "{at:?}");
        assert!((at[2] - (cavity.cap[2] + 1.0)).abs() < 1e-6, "{at:?}");
        assert!((depth - 4.5).abs() < 0.1, "{depth}");
        assert_eq!(ui.selected, Some((0, HoleRef::New(0))));
        let _ = std::fs::remove_dir_all(folder);
    }

    /// Search `ui`'s cups on model 0 and print them, the shown ones in full.
    fn report(ui: &mut HollowingUi, workspace: &Workspace, title: &str) {
        ui.cups.find(workspace, 0);
        let started = Instant::now();
        while ui.cups.searching() {
            ui.cups.poll();
            std::thread::sleep(Duration::from_millis(20));
        }
        let (cups, _) = ui.cups.found(workspace, Some(0)).expect("cups");
        let shown = ui.cups.shown(cups);
        eprintln!(
            "{title}: {} cups, {} shown, {:.1} s",
            cups.len(),
            shown.len(),
            started.elapsed().as_secs_f64()
        );
        for &k in &shown {
            let c = &cups[k];
            eprintln!(
                "  #{k}: layers {}-{} cap ({:.2}, {:.2}, {:.2}) {:.2} mm2 {:.2} mm3 {:?} closes {}",
                c.layers[0],
                c.layers[1],
                c.cap[0],
                c.cap[1],
                c.cap[2],
                c.area_mm2,
                c.volume_mm3,
                c.opening,
                c.closes
            );
            for &p in ui.cups.shown_parts(c).iter().skip(1) {
                let part = c.parts[p];
                eprintln!(
                    "      part {p}: layers {}-{} cap ({:.2}, {:.2}, {:.2}) {:.2} mm2",
                    part.layers[0],
                    part.layers[1],
                    part.cap[0],
                    part.cap[1],
                    part.cap[2],
                    part.area_mm2
                );
            }
        }
    }

    #[test]
    #[ignore = "searches a local large model; set KARASU_INTERFACE_MODEL"]
    fn a_local_model_s_cups_are_found_and_vented() {
        let Ok(path) = std::env::var("KARASU_INTERFACE_MODEL") else {
            eprintln!("KARASU_INTERFACE_MODEL is not set: no cups to find");
            return;
        };
        let limit = Duration::from_secs(600);
        let hollowing = karasu_project::project::Hollowing {
            wall_mm: 2.0,
            ..crate::hollowing_ui::DEFAULT_HOLLOWING
        };
        let (mut workspace, mut preparation) =
            crate::hollowing_ui::tests::placed(path.into(), Some(hollowing), limit);
        let rotate =
            std::env::var("KARASU_INTERFACE_ROTATE_X").map_or(80.0, |r| r.parse().unwrap());
        workspace.edit(|plate| {
            plate.objects[0].pose.rotate_deg = [rotate, 0.0, 0.0];
            plate.objects[0].pose.center = true;
            plate.objects[0].pose.drop = true;
            plate.objects[0].pose.lift_mm = Some(5.0);
        });
        crate::hollowing_ui::tests::prepared_within(&workspace, &mut preparation, limit);
        let folder =
            std::env::temp_dir().join(format!("karasu-app-cups-local-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let mut ui = HollowingUi::default();
        ui.cups.cache = Some(folder.clone());
        report(&mut ui, &workspace, "before");
        // KARASU_INTERFACE_CUP chooses which of the shown cups is vented, the widest by default.
        let which = std::env::var("KARASU_INTERFACE_CUP").map_or(0, |c| c.parse().unwrap());
        let first = ui
            .cups
            .found(&workspace, Some(0))
            .map(|(c, _)| ui.cups.shown(c)[which])
            .unwrap();
        ui.cups.at = first;
        let every = std::env::var("KARASU_INTERFACE_EVERY").is_ok_and(|v| v == "1");
        ui.add_vents(&mut workspace, &preparation, 0, every);
        eprintln!(
            "vents: {:?} {:?}",
            ui.pending.iter().map(|p| p.hole).collect::<Vec<_>>(),
            ui.cups.outbox
        );
        for pending in &ui.pending {
            let verdict = pending
                .verdict
                .as_ref()
                .map(|(_, v)| (&v.refusal, &v.findings));
            eprintln!("verdict: {verdict:?}");
        }
        ui.apply_pending(&mut workspace, 0);
        report(&mut ui, &workspace, "vented");
        eprintln!("messages: {:?}", ui.cups.outbox);
        let _ = std::fs::remove_dir_all(folder);
    }
}
