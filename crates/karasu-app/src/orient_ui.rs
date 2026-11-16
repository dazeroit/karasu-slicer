//! The Orient tool (M6.4k, TD-119; owner mockup "Automatic orientation in the Orient tool",
//! 2026-11-03): tilts of the selected model proposed under the user's constraints, previewed,
//! and applied as one step.
//!
//! **Find orientations** runs [`karasu_project::orient::search`] on a thread of its own (TD-158):
//! - the rough pass ([`karasu_core::analysis::orientation::rough`]) scores every tilt the
//!   constraints allow from the prepared mesh (the hollowed shell when hollowed);
//! - the best [`CHECKED`](karasu_project::orient::CHECKED) distinct tilts are checked as a slice
//!   would: the model alone is sliced in that pose, as the Islands tool's search does, for its
//!   islands and, hollowed, its suction cups; its largest layer comes from the mesh's sections;
//! - the checked ones are ranked again with every measure, and the best
//!   [`SHOWN`](karasu_project::orient::SHOWN) proposed.
//!
//! The search, the tool's values and the candidates live in [`karasu_project::orient`]; this
//! module keeps the thread, the preview, the protected points and Apply.
//!
//! **Preview.** Hovering a candidate, or else the chosen one, draws the model turned to it, in
//! place of the model, its overhangs tinted when they are shown; the scene is not changed.
//!
//! **Apply** turns the model to the chosen candidate as one undoable step: about its centre,
//! resting where it rested. A model with supports asks first: they were placed for the pose
//! before, and are removed with the turn.
//!
//! **Protected points** are kept in the model's frame, with their surface's normal: they follow
//! every move and turn. With Protect (P), a click on the selected model adds one.
pub mod panel;

use crate::{
    camera::Mat4,
    i18n::tr,
    islands_ui::alone_request,
    messages::{Level, Message},
    preparation_ui::PreparationUi,
    repair_panel::{marker, MARKER_SIZE},
    viewport::{Draw, Viewport},
    workspace::Workspace,
};
use eframe::egui_wgpu;
use karasu_core::analysis::orientation::{Protected, Tilt};
pub use karasu_project::orient::{
    Axes, Candidate, OrientValues, Priority, Stage, DEFAULT_ORIENT, KEEP_AWAY_MM, LIMIT_DEG, WEIGHT,
};
use karasu_project::{
    orient::{search, turn_rows, Found, Inputs, Report, SearchError},
    project::ProtectedPoint,
};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    time::Instant,
};

/// The viewport mesh key of the protected points' markers.
const MARKER_KEY: u64 = 0x4F52_4945_4E54_4D4B; // "ORIENTMK"

/// What a search is of: the model, the plate's revision, the values, the critical angle and the
/// protected points. A search for another is out of date.
#[derive(Clone, Debug, PartialEq)]
struct Key {
    object: usize,
    revision: u64,
    values: OrientValues,
    angle_deg: f64,
    points: Vec<ProtectedPoint>,
}

enum Update {
    Stage(Stage),
    Progress(f32),
    Done(Box<Result<Found, String>>),
}

struct Job {
    key: Key,
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

/// A finished search.
struct Searched {
    key: Key,
    found: Found,
    seconds: f64,
}

/// Sends a search's stages and progress to the tool.
struct Sent<'a>(&'a mpsc::Sender<Update>);

impl Report for Sent<'_> {
    fn stage(&mut self, stage: Stage) {
        let _ = self.0.send(Update::Stage(stage));
    }

    fn progress(&mut self, share: f32) {
        let _ = self.0.send(Update::Progress(share));
    }
}

/// The Orient tool's state.
pub struct OrientUi {
    /// The panel's Advanced switch (kept with the settings).
    pub advanced: bool,
    /// The values for the next search.
    pub values: OrientValues,
    /// The defaults set in Settings, for the fields' reset buttons.
    pub defaults: OrientValues,
    /// A click on the selected model adds a protected point.
    pub protecting: bool,
    /// The critical angle, the overhangs' (set each frame by the window).
    pub angle_deg: f64,
    job: Option<Job>,
    searched: Option<Searched>,
    /// The candidate chosen, and the one hovered (previewed).
    pub(crate) chosen: usize,
    pub(crate) hovered: Option<usize>,
    /// Apply waits for the user to confirm removing the supports.
    confirming: Option<usize>,
    cache: Option<PathBuf>,
    marker_uploaded: bool,
    outbox: Vec<Message>,
}

impl Default for OrientUi {
    fn default() -> Self {
        Self::new()
    }
}

/// The application's folder for the searches' slices; each search removes its own files.
fn cache_folder() -> Option<PathBuf> {
    let paths = karasu_core::profiles::store::ProfilePaths::discover().ok()?;
    let folder = paths.cache.join("orientation");
    std::fs::create_dir_all(&folder).ok()?;
    Some(folder)
}

/// The world map of the turn: about `anchor` by `tilt`, then `dz` up.
fn turn_matrix(tilt: &Tilt, anchor: [f64; 3], dz: f64) -> Mat4 {
    Mat4(turn_rows(tilt, anchor, dz))
}

impl OrientUi {
    /// The tool as it starts: its built-in values.
    #[must_use]
    pub fn new() -> Self {
        Self {
            advanced: false,
            values: DEFAULT_ORIENT,
            defaults: DEFAULT_ORIENT,
            protecting: false,
            angle_deg: crate::settings::DEFAULT_OVERHANG_ANGLE_DEG,
            job: None,
            searched: None,
            chosen: 0,
            hovered: None,
            confirming: None,
            cache: None,
            marker_uploaded: false,
            outbox: Vec::new(),
        }
    }

    /// The messages to show as notifications, taken.
    pub fn take_messages(&mut self) -> Vec<Message> {
        std::mem::take(&mut self.outbox)
    }

    /// Whether a search runs.
    #[must_use]
    pub fn searching(&self) -> bool {
        self.job.is_some()
    }

    /// Stop protecting, and drop the hovered preview: the tool is left.
    pub fn close(&mut self) {
        self.protecting = false;
        self.hovered = None;
    }

    /// Escape: stop protecting first. Returns whether it did anything.
    /// Escape: stop protecting first, then drop the candidates. Returns whether it did
    /// anything.
    pub fn escape(&mut self) -> bool {
        if std::mem::replace(&mut self.protecting, false) {
            return true;
        }
        if self.searched.is_some() && self.job.is_none() {
            self.discard();
            return true;
        }
        false
    }

    /// Discard: drop the candidates, so the model shows as it is.
    pub fn discard(&mut self) {
        self.searched = None;
        self.hovered = None;
        self.chosen = 0;
    }

    /// Object `index`'s protected points, in its frame: kept with the object (TD-130), so they
    /// follow it, are undone with it and saved with the project.
    pub fn points_of(workspace: &Workspace, index: usize) -> &[ProtectedPoint] {
        workspace
            .plate()
            .objects
            .get(index)
            .map_or(&[], |o| o.protected.as_slice())
    }

    /// Remove object `index`'s protected point `k`, as one plate edit.
    pub fn remove_point(workspace: &mut Workspace, index: usize, k: usize) {
        if k < Self::points_of(workspace, index).len() {
            workspace.edit(|plate| {
                if let Some(object) = plate.objects.get_mut(index) {
                    object.protected.remove(k);
                }
            });
        }
    }

    /// Delete: object `index`'s last protected point. Returns whether there was one.
    pub fn remove_last(workspace: &mut Workspace, index: usize) -> bool {
        let count = Self::points_of(workspace, index).len();
        if count == 0 {
            return false;
        }
        Self::remove_point(workspace, index, count - 1);
        true
    }

    /// A click on object `index`'s surface at `point` (world) facing `normal`, while protecting:
    /// a protected point there, in the model's frame, added as one plate edit.
    pub fn add_point(
        workspace: &mut Workspace,
        index: usize,
        point: [f64; 3],
        normal: [f64; 3],
    ) -> bool {
        let Some(map) = workspace.model_to_world(index) else {
            return false;
        };
        let Some(back) = map.inverse() else {
            return false;
        };
        // Normals go back by the placement's transpose.
        let l = map.linear;
        let n: [f64; 3] = std::array::from_fn(|c| (0..3).map(|r| l[r][c] * normal[r]).sum());
        let length = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        if length <= 0.0 || !length.is_finite() {
            return false;
        }
        let protected = ProtectedPoint {
            point_mm: back.point(point),
            normal: n.map(|c| c / length),
        };
        workspace.edit(|plate| {
            if let Some(object) = plate.objects.get_mut(index) {
                object.protected.push(protected);
            }
        });
        true
    }

    /// Object `index`'s protected points in the world, with their normals.
    fn protected_world(workspace: &Workspace, index: usize) -> Option<Vec<Protected>> {
        let map = workspace.model_to_world(index)?;
        let back = map.inverse()?;
        // Normals go out by the inverse's transpose.
        let l = back.linear;
        Some(
            Self::points_of(workspace, index)
                .iter()
                .filter_map(|p| {
                    let n: [f64; 3] =
                        std::array::from_fn(|c| (0..3).map(|r| l[r][c] * p.normal[r]).sum());
                    let length = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
                    (length > 0.0 && length.is_finite()).then(|| Protected {
                        point: map.point(p.point_mm),
                        normal: n.map(|c| c / length),
                    })
                })
                .collect(),
        )
    }

    fn key(&self, workspace: &Workspace, index: usize) -> Key {
        Key {
            object: index,
            revision: workspace.revision(),
            values: self.values,
            angle_deg: self.angle_deg,
            points: Self::points_of(workspace, index).to_vec(),
        }
    }

    /// Find orientations for object `index`; or Stop while searching.
    pub fn find(&mut self, workspace: &Workspace, preparation: &PreparationUi, index: usize) {
        if self.job.take().is_some() {
            return;
        }
        self.searched = None;
        self.chosen = 0;
        self.hovered = None;
        self.start(workspace, preparation, index);
    }

    fn start(&mut self, workspace: &Workspace, preparation: &PreparationUi, index: usize) {
        let Some(object) = workspace.plate().objects.get(index) else {
            return;
        };
        let (Some(placement), Some(loaded)) = (
            workspace.placement_of(index),
            workspace.meshes().get(&object.model),
        ) else {
            return;
        };
        let Some(centre) = karasu_project::placement::centre(&loaded.mesh)
            .and_then(|c| placement.transform.transform_point(c).ok())
        else {
            return;
        };
        let Some(protected) = Self::protected_world(workspace, index) else {
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
        let name = crate::islands_ui::slice_name("orientation");
        let output = cache.join(match crate::slicing_ui::extension(&format) {
            Some(ext) => format!("{name}.{ext}"),
            None => format!("{name}-{}", karasu_project::request::REFERENCE_FORMAT),
        });
        let (request, _) =
            match alone_request(workspace, index, output.clone(), format, self.angle_deg) {
                Ok(Some(made)) => made,
                Ok(None) => return,
                Err(error) => {
                    self.outbox
                        .push(Message::new(Level::Error, tr!("orient-refused")).detail(error));
                    return;
                }
            };
        let shell = preparation
            .shared(workspace, index)
            .map(|(shared, _)| shared)
            .filter(|_| object.hollowing().is_some());
        if object.hollowing().is_some() && shell.is_none() {
            // The shell is still being prepared: the search waits for it.
            self.outbox
                .push(Message::new(Level::Warning, tr!("orient-preparing")));
            return;
        }
        let inputs = Inputs {
            index,
            source: Arc::clone(&loaded.mesh),
            placement: placement.transform,
            shell,
            protected,
            request,
            output,
            values: self.values,
            angle_deg: self.angle_deg,
            anchor: [centre.x, centre.y, centre.z],
            min_z: placement.min.z,
            rotate_deg: object.pose.rotate_deg,
            drop: object.pose.drop,
        };
        let key = self.key(workspace, index);
        let stop = Arc::new(AtomicBool::new(false));
        let cancelled = Arc::clone(&stop);
        let (sender, receiver) = mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("orientation".to_owned())
            .spawn(move || {
                let result = search(&inputs, &mut Sent(&sender), &cancelled).map_err(|e| match e {
                    SearchError::Stopped => tr!("orient-stopped"),
                    SearchError::Failed(cause) => cause,
                });
                let _ = sender.send(Update::Done(Box::new(result)));
            });
        match spawned {
            Ok(_) => {
                self.job = Some(Job {
                    key,
                    started: Instant::now(),
                    stage: Stage::Rough,
                    progress: 0.0,
                    stop,
                    receiver,
                });
            }
            Err(error) => log::warn!(target: "orientation", "cannot start the search: {error}"),
        }
    }

    /// Take a running search's news. Returns whether a repaint is needed soon.
    pub fn poll(&mut self) -> bool {
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
                    done = Some(Err(tr!("orient-stopped")));
                    break;
                }
            }
        }
        let Some(result) = done else {
            return true;
        };
        let (key, seconds) = (job.key.clone(), job.started.elapsed().as_secs_f64());
        self.job = None;
        match result {
            Ok(found) => {
                log::info!(
                    target: "orientation",
                    "scored {} tilts, proposed {} in {seconds:.2} s",
                    found.scored,
                    found.candidates.len()
                );
                self.outbox.push(Message::new(
                    Level::Success,
                    tr!(
                        "orient-found",
                        scored = found.scored,
                        count = found.candidates.len(),
                        seconds = crate::i18n::decimal(seconds, 1)
                    ),
                ));
                self.chosen = 0;
                self.searched = Some(Searched {
                    key,
                    found,
                    seconds,
                });
            }
            Err(reason) => self
                .outbox
                .push(Message::new(Level::Error, tr!("orient-failed")).detail(reason)),
        }
        true
    }

    /// The candidates found for object `index`, and whether they describe the model as it is
    /// (the plate, the values and the protected points unchanged since).
    pub(crate) fn found_of(
        &self,
        workspace: &Workspace,
        index: Option<usize>,
    ) -> Option<(&[Candidate], bool)> {
        let index = index?;
        let searched = self.searched.as_ref().filter(|s| s.key.object == index)?;
        Some((
            &searched.found.candidates,
            searched.key == self.key(workspace, index),
        ))
    }

    /// How long the last search took, in seconds.
    pub(crate) fn seconds(&self) -> Option<f64> {
        self.searched.as_ref().map(|s| s.seconds)
    }

    /// What a running search does, and its progress.
    pub(crate) fn stage(&self) -> Option<(Stage, f32)> {
        self.job.as_ref().map(|j| (j.stage, j.progress))
    }

    /// Apply: turn object `index` to the chosen candidate as one step; a model with supports
    /// asks first.
    pub fn apply(&mut self, workspace: &mut Workspace, index: usize) {
        let has_supports = workspace
            .supports(index)
            .is_some_and(|d| !d.supports.is_empty());
        if has_supports {
            self.confirming = Some(index);
        } else {
            self.turn(workspace, index, false);
        }
    }

    fn turn(&mut self, workspace: &mut Workspace, index: usize, remove_supports: bool) {
        let Some((candidates, true)) = self.found_of(workspace, Some(index)) else {
            return;
        };
        let Some(c) = candidates.get(self.chosen).copied() else {
            return;
        };
        workspace.edit(|plate| {
            if let Some(object) = plate.objects.get_mut(index) {
                object.pose.rotate_deg = c.rotate_deg;
                if !object.pose.drop {
                    object.translate_mm[2] += c.dz;
                }
                if remove_supports {
                    object.supports = None;
                }
            }
        });
        self.searched = None;
        self.hovered = None;
        self.outbox.push(Message::new(
            Level::Success,
            if remove_supports {
                tr!("orient-turned-supports", tilt = describe(&c.tilt))
            } else {
                tr!("orient-turned", tilt = describe(&c.tilt))
            },
        ));
    }

    /// The confirmation before supports are removed by a turn.
    pub fn confirm(&mut self, ctx: &eframe::egui::Context, workspace: &mut Workspace) {
        let Some(index) = self.confirming else {
            return;
        };
        let count = workspace.supports(index).map_or(0, |d| d.supports.len());
        match crate::dialog::confirm(
            ctx,
            "orient-confirm",
            &tr!("orient-confirm-title"),
            &tr!("orient-confirm-text", count = count),
            &tr!("orient-confirm-action"),
            true,
        ) {
            Some(true) => {
                self.confirming = None;
                self.turn(workspace, index, true);
            }
            Some(false) => self.confirming = None,
            None => {}
        }
    }

    /// The world map previewing the hovered (else chosen) candidate of object `focus`, when the
    /// candidates describe the model as it is.
    /// Never while protecting: a click must land on the model as it is (owner, 2026-11-03).
    pub fn preview(&self, workspace: &Workspace, focus: Option<usize>) -> Option<Mat4> {
        if self.protecting {
            return None;
        }
        let (candidates, true) = self.found_of(workspace, focus)? else {
            return None;
        };
        let c = candidates.get(self.hovered.unwrap_or(self.chosen))?;
        let anchor = self.searched.as_ref()?.found.anchor;
        Some(turn_matrix(&c.tilt, anchor, c.dz))
    }

    /// The protected points' markers on object `focus`, a constant size on screen.
    pub fn draws(
        &mut self,
        viewport: &mut Viewport,
        state: &egui_wgpu::RenderState,
        workspace: &Workspace,
        focus: Option<usize>,
        eye: [f64; 3],
        appearance: &crate::appearance::Appearance,
    ) -> Vec<Draw> {
        let Some(index) = focus else {
            return Vec::new();
        };
        let Some(map) = workspace.model_to_world(index) else {
            return Vec::new();
        };
        // Previewed, the markers turn with the model.
        let turn = self.preview(workspace, focus);
        let points: Vec<[f64; 3]> = Self::points_of(workspace, index)
            .iter()
            .map(|p| {
                let w = map.point(p.point_mm);
                turn.as_ref().map_or(w, |m| {
                    std::array::from_fn(|r| {
                        m.0[r][0] * w[0] + m.0[r][1] * w[1] + m.0[r][2] * w[2] + m.0[r][3]
                    })
                })
            })
            .collect();
        if points.is_empty() {
            return Vec::new();
        }
        if !self.marker_uploaded {
            viewport.upload(&state.device, MARKER_KEY, &marker(), [0.0; 3]);
            self.marker_uploaded = true;
        }
        let color = appearance.protected.linear();
        let mut through = color;
        through[3] *= 0.5;
        let mut draws = Vec::new();
        for point in points {
            let distance = (0..3)
                .map(|k| (point[k] - eye[k]).powi(2))
                .sum::<f64>()
                .sqrt();
            let size = MARKER_SIZE * 1.6 * distance;
            let model = Mat4([
                [size, 0.0, 0.0, point[0]],
                [0.0, size, 0.0, point[1]],
                [0.0, 0.0, size, point[2]],
                [0.0, 0.0, 0.0, 1.0],
            ]);
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

/// A tilt in words: how far, and about which plate axis.
#[must_use]
pub fn describe(tilt: &Tilt) -> String {
    let angle = tilt.angle_deg();
    if angle < 0.5 {
        return tr!("orient-tilt-none");
    }
    let [x, y, _] = tilt.axis.map(|c| c * angle);
    let round = |v: f64| crate::i18n::decimal(v, 0);
    if y.abs() < 0.5 {
        tr!("orient-tilt-x", x = round(x))
    } else if x.abs() < 0.5 {
        tr!("orient-tilt-y", y = round(y))
    } else {
        tr!("orient-tilt-xy", x = round(x), y = round(y))
    }
}

#[cfg(test)]
mod tests;
