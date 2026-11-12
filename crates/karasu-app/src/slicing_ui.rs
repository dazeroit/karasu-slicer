//! Slicing, export and the layer view (M6.3, TD-077).
//!
//! **Slicing** runs the plate's request as a background job ([`karasu_project::jobs`]) into the
//! application's cache folder. The Slice panel ([`panel`], M6.5) starts it and shows its
//! progress with Cancel in its footer; the status bar says it runs once it has run for
//! [`BUSY_DELAY`], so quick jobs never flash it. The result records the plate revision it was
//! sliced from; once the plate changes it is shown as out of date and cannot be exported.
//!
//! **Export** copies the sliced file to a place the user picks in the native save dialog. It
//! writes a temporary file beside the destination and renames it, so a failed copy never leaves
//! a partial file.
//!
//! **Messages:** failures, cancellations, finished slices and the repairs they applied become
//! [`Message`]s, which the main window collects; a failed slice offers the fixes its remedies
//! name.
//!
//! **Layers:** the job keeps every exported layer's mask, exactly as written to the file. The
//! layer panel shows the chosen layer, reduced to the panel's size straight from the mask's row
//! spans, and turned as the plate is seen from above with its front at the bottom: the printer's
//! panel orientation (axis swap and mirrors) is undone, so the layer matches the model's place
//! on the plate whatever the printer.
//!
//! **Wall verification (TD-095):** with the Slice panel's "Verify the printed walls" on, slices
//! run with the verifier (the same file, since slicing is deterministic). The panel's Result
//! then lists the thinnest printed wall, breaches and undrained pockets, each taking the layer
//! view to its layer, and the layer image marks them in the theme's colours. The marks are
//! painted into the layer's image when it is built, so drawing a layer costs nothing more.
use crate::{
    i18n::{decimal, tr},
    messages::{Lasting, Level, Message},
    theme,
    workspace::Workspace,
};
use eframe::egui;
use karasu_core::raster::{
    mask::SpanMask,
    verify::{LayerMarks, VoxelAt},
    Orientation,
};
use karasu_project::{
    jobs::{Event, Job, JobError, LayerMask, Progress},
    pipeline::{SliceOutcome, Verification},
    request::{machine_exporter, REFERENCE_FORMAT},
};
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

mod layers;
mod masks;
mod panel;

pub(crate) use panel::{format_label, switch_row};

pub(crate) use panel::{format_of, plate_format};

/// How long the toast of a finished slice or export shows: its facts or path take a moment to
/// read.
const READING_TIME: std::time::Duration = std::time::Duration::from_secs(6);

/// How long a job runs before the status bar says so.
pub const BUSY_DELAY: Duration = Duration::from_millis(150);
/// Largest side of the layer image shown, in pixels.
const LAYER_IMAGE_MAX: u32 = 1024;

/// A running job and what it last reported.
struct Running {
    job: Job,
    started: Instant,
    phase: String,
    layers: Option<(usize, usize)>,
    output: PathBuf,
    /// The printer panel's orientation, which the masks are written in.
    orientation: Orientation,
    /// The area of one of the printer's pixels, in mm².
    pixel_mm2: f64,
    /// Whether the job verifies the printed walls (TD-095).
    verify: bool,
    /// The plate's name when Slice or Export every plate started the job (TD-135, phase 4):
    /// its messages then name the plate, and its plain success and cancellation are left to
    /// the batch's own notification.
    batch: Option<String>,
}

/// A finished slice.
struct Sliced {
    revision: u64,
    output: PathBuf,
    extension: String,
    outcome: SliceOutcome,
    masks: Vec<LayerMask>,
    /// The printer panel's orientation, which the masks are written in.
    orientation: Orientation,
    /// The area of one of the printer's pixels, in mm².
    pixel_mm2: f64,
}

/// Where a plate's slicing stands (TD-135).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SliceState {
    /// Never sliced, or not since it was opened.
    None,
    /// A slice is running.
    Running,
    /// The last slice failed.
    Failed,
    /// Sliced at this revision of the plate.
    Sliced(u64),
}

/// What a finished slice made, as the plates' overview shows it (TD-135).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SliceFacts {
    /// The plate's revision the slice describes.
    pub revision: u64,
    /// The printed layers.
    pub layers: usize,
    /// The top of the last layer above the plate, in mm.
    pub height_mm: f64,
    /// The estimated print time, in seconds.
    pub time_s: f64,
    /// The resin the layers take, in ml.
    pub resin_ml: f64,
}

/// The number of the next slice's file, shared by every plate's slicing (TD-135).
static NEXT_SLICE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// The slicing state of the main window.
#[allow(clippy::struct_excessive_bools)] // Independent states and one-shot requests.
pub struct SlicingUi {
    cache: Option<PathBuf>,
    running: Option<Running>,
    sliced: Option<Sliced>,
    /// Messages not yet taken by the main window.
    outbox: Vec<Message>,
    /// The layer shown, by position in the masks.
    layer: usize,
    /// The verification finding chosen in the panel, marked on its layer.
    focus: Option<Focus>,
    /// The layer image, and the layer and focus it shows.
    texture: Option<((usize, Option<Focus>), egui::TextureHandle)>,
    /// The Slice panel's Advanced switch: the slicing options show.
    pub advanced: bool,
    /// Slices verify the printed walls (a setting of the tool, TD-095).
    verify: bool,
    /// Why the last slice failed, while no slice has ended since.
    failed: Option<String>,
    /// How the Layers panel and its window show the layer (TD-103).
    view: layers::View,
    /// A finding was chosen in the Slice panel: the window opens the Layers tool on it.
    show_layers: bool,
    /// The output's "edit the print setting" link was clicked.
    edit_setting: bool,
}

/// A verification finding to point at: a voxel, and the pocket it belongs to when it is one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Focus {
    layer: usize,
    pixel: [u32; 2],
    pocket: Option<usize>,
}

impl Default for SlicingUi {
    fn default() -> Self {
        Self::new()
    }
}

/// The lock file in a session's folder of slices, held while the session runs.
const SESSION_LOCK: &str = "lock";

/// This session's folder for sliced files, under the application's `slices` folder, made by the
/// first slicing the application makes; every plate's slicing shares it (TD-135). The session
/// holds the lock file in it while it runs, so another application started beside it leaves it
/// alone; the folders of sessions that ended, and loose files an earlier version left, are
/// removed then.
fn cache_folder() -> Option<PathBuf> {
    static FOLDER: std::sync::OnceLock<Option<(PathBuf, std::fs::File)>> =
        std::sync::OnceLock::new();
    FOLDER
        .get_or_init(|| {
            let paths = karasu_core::profiles::store::ProfilePaths::discover().ok()?;
            let root = paths.cache.join("slices");
            std::fs::create_dir_all(&root).ok()?;
            remove_ended_sessions(&root);
            let folder = root.join(std::process::id().to_string());
            std::fs::create_dir_all(&folder).ok()?;
            let lock = std::fs::File::create(folder.join(SESSION_LOCK)).ok()?;
            lock.lock().ok()?;
            Some((folder, lock))
        })
        .as_ref()
        .map(|(folder, _)| folder.clone())
}

/// Remove what `root` holds that no running session holds: the folders whose lock is free (or
/// missing), and loose files.
fn remove_ended_sessions(root: &Path) {
    for entry in std::fs::read_dir(root).into_iter().flatten().flatten() {
        let path = entry.path();
        if !path.is_dir() {
            let _ = std::fs::remove_file(&path);
            continue;
        }
        let free = std::fs::File::options()
            .write(true)
            .create(true)
            .truncate(false)
            .open(path.join(SESSION_LOCK))
            .is_ok_and(|lock| lock.try_lock().is_ok());
        // The lock taken to test it is closed above: Windows removes no open file.
        if free {
            let _ = std::fs::remove_dir_all(&path);
        }
    }
}

/// Remove this session's sliced files as the application closes, its lock kept until the
/// process ends (the next session removes the folder).
pub fn end_session() {
    let Some(folder) = cache_folder() else {
        return;
    };
    for entry in std::fs::read_dir(&folder).into_iter().flatten().flatten() {
        if entry.file_name() != SESSION_LOCK {
            remove(&entry.path());
        }
    }
}

/// The extension of a format's file, or `None` for the reference format (a folder).
pub(crate) fn extension(format: &str) -> Option<String> {
    machine_exporter(format)
        .ok()
        .flatten()
        .map(|exporter| karasu_core::export::MachineExporter::extension(&exporter).to_owned())
}

impl SlicingUi {
    /// Whether every slice verifies the printed walls (a setting of the tool, TD-095).
    pub fn verify_walls(&mut self) -> &mut bool {
        &mut self.verify
    }

    /// Whether the section follows the layer the Layers tool shows (a user setting).
    pub fn follow_layer(&mut self) -> &mut bool {
        &mut self.view.follow
    }

    /// No slice yet; sliced files go to the application's cache folder.
    #[must_use]
    pub fn new() -> Self {
        Self {
            cache: cache_folder(),
            running: None,
            sliced: None,
            outbox: Vec::new(),
            layer: 0,
            focus: None,
            texture: None,
            advanced: false,
            verify: false,
            failed: None,
            view: layers::View::default(),
            show_layers: false,
            edit_setting: false,
        }
    }

    /// A slicing for another plate (TD-135, phase 4): no slice yet, writing where this one
    /// writes and verifying the walls as it does.
    #[must_use]
    pub fn sibling(&self) -> Self {
        let mut sibling = Self::new();
        sibling.cache.clone_from(&self.cache);
        sibling.verify = self.verify;
        sibling
    }

    /// Whether a finding chosen in the Slice panel asks for the Layers tool, once.
    pub fn take_show_layers(&mut self) -> bool {
        std::mem::take(&mut self.show_layers)
    }

    /// Whether the output's "edit the print setting" link was clicked since the last call.
    pub fn take_edit_setting(&mut self) -> bool {
        std::mem::take(&mut self.edit_setting)
    }

    /// Messages produced since the last call, oldest first.
    pub fn take_messages(&mut self) -> Vec<Message> {
        std::mem::take(&mut self.outbox)
    }

    /// Slice the plate shown in the background, verifying its printed walls when the tool says
    /// so, unless a job is already running.
    pub fn start(&mut self, workspace: &Workspace) {
        self.start_job(workspace, workspace.plate_id(), self.verify, None);
    }

    /// Slice plate `id`, shown or not, in the background for Slice every plate or Export every
    /// plate (TD-135, phase 4), unless a job is already running. Its messages name the plate
    /// (`name`); its plain success and a cancellation are left to the batch.
    pub fn start_plate(
        &mut self,
        workspace: &Workspace,
        id: karasu_project::project::PlateId,
        name: &str,
    ) {
        self.start_job(workspace, id, self.verify, Some(name.to_owned()));
    }

    /// Whether a slice is running.
    #[must_use]
    pub fn running(&self) -> bool {
        self.running.is_some()
    }

    /// The running slice's share done, from 0 to 1, once its layers are counted.
    #[must_use]
    pub fn progress(&self) -> Option<f64> {
        let (done, total) = self.running.as_ref()?.layers?;
        #[allow(clippy::cast_precision_loss)] // Layer counts are far below 2^52.
        Some(done as f64 / total.max(1) as f64)
    }

    /// Ask the running slice to stop; [`Self::poll`] then reports it cancelled.
    pub fn cancel(&self) {
        if let Some(running) = &self.running {
            running.job.cancel();
        }
    }

    /// Why the last slice failed, when it did.
    #[must_use]
    pub fn failure(&self) -> Option<&str> {
        self.failed.as_deref()
    }

    /// Slice plate `id` in the background, verifying its printed walls when `verify`.
    fn start_job(
        &mut self,
        workspace: &Workspace,
        id: karasu_project::project::PlateId,
        verify: bool,
        batch: Option<String>,
    ) {
        if self.running.is_some() {
            return;
        }
        let Some(plate) = workspace
            .project()
            .plate(id)
            .map(karasu_project::project::Plate::content)
        else {
            return;
        };
        let Some(cache) = &self.cache else {
            self.outbox
                .push(Message::new(Level::Error, tr!("error-no-cache")));
            return;
        };
        // The format the Slice panel would settle on: a plate not shown may still hold one its
        // printer does not write (TD-135, phase 4). The reference format, chosen in the
        // Advanced options, stays.
        let format = if plate.settings.format == REFERENCE_FORMAT {
            plate.settings.format.clone()
        } else {
            format_of(plate).unwrap_or_else(|| plate.settings.format.clone())
        };
        // Unique across every plate's slicing (TD-135): they share the session's folder, and
        // a slice removes its previous file when the next ends.
        let number = NEXT_SLICE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let name = match extension(&format) {
            Some(ext) => format!("slice-{number}.{ext}"),
            None => format!("slice-{number}-{REFERENCE_FORMAT}"),
        };
        let output = cache.join(name);
        let orientation = plate
            .printer
            .as_ref()
            .map_or_else(Orientation::default, |p| Orientation {
                swap_xy: p.swap_xy,
                flip_x: p.mirror_x,
                flip_y: p.mirror_y,
            });
        // One LCD pixel's share of the build area, whichever way the panel is turned.
        let pixel_mm2 = plate.printer.as_ref().map_or(0.0, |p| {
            let [w, h] = p.resolution_px;
            p.build_size_mm[0] * p.build_size_mm[1] / (f64::from(w) * f64::from(h)).max(1.0)
        });
        match workspace.request_of(id, output.clone()) {
            Ok((mut request, revision)) => {
                request.verify_walls = verify;
                request.format = format;
                self.running = Some(Running {
                    job: Job::start_with(
                        id,
                        revision,
                        request,
                        true,
                        Some(workspace.pipeline_cache()),
                    ),
                    started: Instant::now(),
                    phase: tr!("progress-starting"),
                    layers: None,
                    output,
                    orientation,
                    pixel_mm2,
                    verify,
                    batch,
                });
            }
            Err(error) => self.outbox.push(
                Message::new(Level::Error, tr!("slice-plate-refused")).detail(error.to_string()),
            ),
        }
    }

    /// Take the running job's events. Returns whether a repaint is needed.
    /// Where this plate's slicing stands, for the plates bar (TD-135): slicing, failed, or the
    /// revision its last slice describes.
    pub(crate) fn slice_state(&self) -> SliceState {
        if self.running.is_some() {
            SliceState::Running
        } else if self.failed.is_some() {
            SliceState::Failed
        } else {
            self.sliced
                .as_ref()
                .map_or(SliceState::None, |s| SliceState::Sliced(s.revision))
        }
    }

    /// The last slice's facts, for the plates' overview (TD-135); `None` before a slice ends.
    pub(crate) fn slice_facts(&self) -> Option<SliceFacts> {
        let sliced = self.sliced.as_ref()?;
        Some(SliceFacts {
            revision: sliced.revision,
            layers: sliced.outcome.export.layers,
            height_mm: sliced.masks.last().map_or(0.0, |m| m.z_mm),
            time_s: sliced.outcome.export.estimated_time_s,
            resin_ml: sliced.outcome.resin.volume_ml,
        })
    }

    pub fn poll(&mut self) -> bool {
        let Some(running) = &mut self.running else {
            return false;
        };
        let mut finished = None;
        for event in running.job.poll() {
            match event {
                Event::Progress(Progress::Phase(name)) => running.phase = name,
                Event::Progress(Progress::Layers { completed, total }) => {
                    running.phase = tr!("progress-slicing-layers");
                    running.layers = Some((completed, total));
                }
                Event::Finished(f) => finished = Some(f),
            }
        }
        let Some(finished) = finished else {
            return true;
        };
        let (output, orientation, verify, pixel_mm2) = (
            running.output.clone(),
            running.orientation,
            running.verify,
            running.pixel_mm2,
        );
        let batch = running.batch.take();
        let start = self.outbox.len();
        self.running = None;
        self.failed = None;
        match finished.result {
            Ok(outcome) => {
                if let Some(previous) = self.sliced.take() {
                    remove(&previous.output);
                }
                let extension = output
                    .extension()
                    .map_or_else(String::new, |e| e.to_string_lossy().into_owned());
                if !verify {
                    self.layer = 0;
                }
                self.focus = None;
                self.texture = None;
                match &outcome.verification {
                    Some(verification) => self.outbox.push(verified_message(verification)),
                    None => self.outbox.extend(finished_messages(&outcome)),
                }
                self.sliced = Some(Sliced {
                    revision: finished.revision,
                    output,
                    extension,
                    outcome,
                    masks: finished.masks,
                    orientation,
                    pixel_mm2,
                });
            }
            Err(JobError::Cancelled) => {
                if batch.is_none() {
                    self.outbox
                        .push(Message::new(Level::Info, tr!("slice-cancelled")));
                }
            }
            Err(JobError::Failed(error)) => {
                let message = Message::failed_slice(error.reason(), error.remedies());
                self.failed = Some(message.text.clone());
                self.outbox.push(message);
            }
            Err(JobError::Lost) => {
                self.failed = Some(tr!("slice-lost"));
                self.outbox
                    .push(Message::new(Level::Error, tr!("slice-lost")));
            }
        }
        if let Some(plate) = batch {
            name_plate(&mut self.outbox, start, &plate);
        }
        true
    }

    fn export(&mut self, workspace: &Workspace) {
        self.export_as(&workspace.plate().name);
    }

    /// Ask where to export the last slice, offering plate `plate`'s name for the file, and
    /// export it there.
    pub(crate) fn export_as(&mut self, plate: &str) {
        let Some(sliced) = &self.sliced else {
            return;
        };
        let name = format!("{plate}.{}", sliced.extension);
        let mut dialog = rfd::FileDialog::new()
            .set_title(tr!("dialog-export-title"))
            .set_file_name(name);
        if !sliced.extension.is_empty() {
            dialog = dialog.add_filter(&sliced.extension, &[sliced.extension.as_str()]);
        }
        if let Some(destination) = dialog.save_file() {
            self.export_to(&destination);
        }
    }

    /// Export the last slice to `destination`, saying how it went. Returns whether it was
    /// written.
    pub(crate) fn export_to(&mut self, destination: &Path) -> bool {
        let Some(sliced) = &self.sliced else {
            return false;
        };
        let written = publish(&sliced.output, destination);
        let ok = written.is_ok();
        self.outbox.push(match written {
            // The path takes a moment to read.
            Ok(()) => Message::new(
                Level::Success,
                tr!("export-done", path = destination.display().to_string()),
            )
            .lasting(Lasting::For(READING_TIME)),
            Err(error) => Message::new(Level::Error, tr!("export-failed"))
                .detail(format!("cannot write {}: {error}", destination.display())),
        });
        ok
    }

    /// Write the last slice to `destination` whole or not at all, without a message: Export
    /// every plate says how its files went (TD-135, phase 4). Returns the bytes written.
    ///
    /// # Errors
    /// No slice to write, or the file system's error.
    pub(crate) fn write_to(&self, destination: &Path) -> std::io::Result<u64> {
        let sliced = self
            .sliced
            .as_ref()
            .ok_or_else(|| std::io::Error::other("no slice"))?;
        publish(&sliced.output, destination)?;
        Ok(size_of(destination))
    }

    /// The status bar's line while a job runs past [`BUSY_DELAY`]: its phase and progress.
    #[must_use]
    pub fn running_status(&self) -> Option<String> {
        let running = self.running.as_ref()?;
        (running.started.elapsed() >= BUSY_DELAY).then(|| {
            let progress = running.layers.map_or_else(
                || {
                    tr!(
                        "progress-seconds",
                        seconds = running.started.elapsed().as_secs()
                    )
                },
                |(done, total)| tr!("progress-layers", done = done, total = total),
            );
            format!("{}: {}, {progress}", tr!("progress-title"), running.phase)
        })
    }
}

/// Pockets listed in the panel; more are counted.
const LISTED_POCKETS: usize = 8;

/// The colours of the map's marks: thin walls, breaches and undrained pockets.
const THIN: theme::Token = theme::WARNING;
const BREACH: theme::Token = theme::ERROR;
const POCKET: theme::Token = theme::INTERACTION;
/// The focused finding: neither material (gray) nor another mark.
const SELECTED: theme::Token = theme::SELECTION;
/// Half-width of the ring around a focused finding, and the length of its cross's arms, in
/// pixels of the reduced layer image.
const FOCUS_RING: usize = 6;
const FOCUS_ARM: usize = 16;

/// The verification's findings, each with a link to it; returns the finding clicked.
fn walls(ui: &mut egui::Ui, v: &Verification) -> Option<Focus> {
    let report = &v.report;
    let mut go = None;
    let mut link = |ui: &mut egui::Ui, text: String, focus: Focus, colour: theme::Token| {
        if ui
            .link(egui::RichText::new(text).color(colour.color()))
            .on_hover_text(tr!("walls-go", layer = focus.layer + 1))
            .clicked()
        {
            go = Some(focus);
        }
    };
    let at = |voxel: VoxelAt| Focus {
        layer: voxel.layer,
        pixel: voxel.pixel,
        pocket: None,
    };
    match &v.failure {
        Some(reason) => {
            ui.label(egui::RichText::new(tr!("walls-failed")).color(theme::ERROR.color()))
                .on_hover_text(reason);
        }
        None => {
            ui.label(egui::RichText::new(tr!("walls-passed")).color(theme::ACCENT.color()));
        }
    }
    match report.thinnest {
        Some(t) => link(
            ui,
            tr!(
                "walls-thinnest",
                measured = decimal(t.measured_mm, 3),
                guaranteed = decimal(t.guaranteed_mm, 3)
            ),
            at(t.at),
            theme::TEXT_PRIMARY,
        ),
        None if report.cavity_voxels == 0 => {
            ui.label(tr!("walls-no-cavity"));
        }
        None => {
            ui.label(tr!("walls-thick", wall = decimal(v.walls.requested_mm, 2)));
        }
    }
    if report.thin_voxels > 0 {
        ui.label(
            egui::RichText::new(tr!(
                "walls-thin",
                count = report.thin_voxels,
                wall = decimal(v.walls.requested_mm, 2)
            ))
            .color(THIN.color()),
        );
    }
    match report.breaches.first {
        Some(first) => link(
            ui,
            tr!("walls-breaches", count = report.breaches.voxels),
            at(first),
            BREACH,
        ),
        None => {
            ui.label(tr!("walls-no-breach"));
        }
    }
    if report.pockets.is_empty() {
        ui.label(tr!("walls-drained"));
    } else {
        ui.label(
            egui::RichText::new(tr!("walls-pockets", count = report.pockets.len()))
                .color(POCKET.color()),
        );
        for (index, pocket) in report.pockets.iter().enumerate().take(LISTED_POCKETS) {
            link(
                ui,
                tr!(
                    "walls-pocket",
                    number = index + 1,
                    volume = decimal(pocket.volume_mm3, 3),
                    layer = pocket.widest.layer + 1
                ),
                Focus {
                    pocket: Some(index),
                    ..at(pocket.widest)
                },
                POCKET,
            );
        }
        if report.pockets.len() > LISTED_POCKETS {
            ui.label(tr!(
                "walls-more-pockets",
                count = report.pockets.len() - LISTED_POCKETS
            ));
        }
    }
    go
}

/// The map's colours, explained.
fn legend(ui: &mut egui::Ui) {
    ui.horizontal_wrapped(|ui| {
        for (colour, text) in [
            (THIN, tr!("walls-legend-thin")),
            (BREACH, tr!("walls-legend-breach")),
            (POCKET, tr!("walls-legend-pocket")),
        ] {
            let (swatch, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
            ui.painter().rect_filled(swatch, 2.0, colour.color());
            ui.label(text);
        }
    });
}

/// Paint a layer's marks into its image reduced from `mask` by [`reduced`] with `max`, over its
/// material, which stays white: pockets, then thin walls, then breaches, each over the one
/// before. The focused pocket is painted in
/// the selection colour, and a focus on this layer gets a ring and a cross around it, so a
/// finding a few pixels wide is found at any reduction.
fn mark(
    image: &mut egui::ColorImage,
    mask: &SpanMask,
    marks: &LayerMarks,
    focus: Option<Focus>,
    max: u32,
) {
    let step = mask.width().max(mask.height()).div_ceil(max).max(1);
    // The material stays white, as exported (owner, M6.5): the marks are painted over it.
    let [width, height] = image.size;
    let mut paint = |row: u32, start: u32, end: u32, colour: egui::Color32| {
        let line = (row / step) as usize * width;
        for x in (start / step)..=((end - 1) / step) {
            if let Some(pixel) = image.pixels.get_mut(line + x as usize) {
                *pixel = colour;
            }
        }
    };
    for &[row, start, end, pocket] in &marks.pocket {
        let selected = focus.and_then(|f| f.pocket) == Some(pocket as usize);
        paint(
            row,
            start,
            end,
            if selected { SELECTED } else { POCKET }.color(),
        );
    }
    for (runs, colour) in [(&marks.thin, THIN), (&marks.breach, BREACH)] {
        for &[row, start, end] in runs {
            paint(row, start, end, colour.color());
        }
    }
    let Some(focus) = focus else {
        return;
    };
    let (cx, cy) = (
        (focus.pixel[0] / step) as usize,
        (focus.pixel[1] / step) as usize,
    );
    let colour = SELECTED.color();
    let mut dot = |x: usize, y: usize| {
        if x < width && y < height {
            image.pixels[y * width + x] = colour;
        }
    };
    for d in 0..=FOCUS_ARM {
        // The ring.
        if d <= FOCUS_RING {
            for (x, y) in [
                (cx + d, cy + FOCUS_RING),
                (cx + d, cy.wrapping_sub(FOCUS_RING)),
                (cx.wrapping_sub(d), cy + FOCUS_RING),
                (cx.wrapping_sub(d), cy.wrapping_sub(FOCUS_RING)),
                (cx + FOCUS_RING, cy + d),
                (cx.wrapping_sub(FOCUS_RING), cy + d),
                (cx + FOCUS_RING, cy.wrapping_sub(d)),
                (cx.wrapping_sub(FOCUS_RING), cy.wrapping_sub(d)),
            ] {
                dot(x, y);
            }
        }
        // The cross, outside the ring.
        if d > FOCUS_RING {
            for (x, y) in [
                (cx + d, cy),
                (cx.wrapping_sub(d), cy),
                (cx, cy + d),
                (cx, cy.wrapping_sub(d)),
            ] {
                dot(x, y);
            }
        }
    }
}

/// The message of a finished wall verification: passed, or failed with why.
fn verified_message(v: &Verification) -> Message {
    match &v.failure {
        Some(reason) => Message::new(Level::Error, tr!("walls-failed")).detail(reason.clone()),
        None => Message::new(Level::Success, tr!("walls-passed")),
    }
}

/// A mask reduced so its larger side is at most `max` pixels: each shown pixel takes the
/// brightest mask pixel it covers, so thin features stay visible.
#[must_use]
pub fn reduced(mask: &SpanMask, max: u32) -> egui::ColorImage {
    let (width, height) = (mask.width(), mask.height());
    let step = width.max(height).div_ceil(max).max(1);
    let (out_w, out_h) = (width.div_ceil(step), height.div_ceil(step));
    let mut pixels = vec![0u8; out_w as usize * out_h as usize];
    for (y, row) in mask.rows().enumerate() {
        #[allow(clippy::cast_possible_truncation)] // Rows fit u32: the mask's height does.
        let out_row = (y as u32 / step) as usize * out_w as usize;
        for span in row {
            for x in (span.start() / step)..=((span.end() - 1) / step) {
                let slot = &mut pixels[out_row + x as usize];
                *slot = (*slot).max(span.value());
            }
        }
    }
    egui::ColorImage::from_gray([out_w as usize, out_h as usize], &pixels)
}

/// A panel image turned to the plate seen from above: columns run along world X, rows along
/// world Y with the front (lowest Y) at the bottom. The panel's mirrors and axis swap are undone
/// in the reverse of the order the raster applies them (`karasu_core::raster::LcdProfile`).
#[must_use]
pub fn plate_view(panel: &egui::ColorImage, orientation: Orientation) -> egui::ColorImage {
    let [width, height] = panel.size;
    let [world_w, world_h] = if orientation.swap_xy {
        [height, width]
    } else {
        [width, height]
    };
    let mut pixels = vec![egui::Color32::BLACK; width * height];
    for (row, line) in panel.pixels.chunks_exact(width.max(1)).enumerate() {
        for (column, &pixel) in line.iter().enumerate() {
            let x = if orientation.flip_x {
                width - 1 - column
            } else {
                column
            };
            let y = if orientation.flip_y {
                height - 1 - row
            } else {
                row
            };
            let (world_x, world_y) = if orientation.swap_xy { (y, x) } else { (x, y) };
            pixels[(world_h - 1 - world_y) * world_w + world_x] = pixel;
        }
    }
    egui::ColorImage::new([world_w, world_h], pixels)
}

/// Layers, print time and resin of a slice, in one line.
fn facts(outcome: &SliceOutcome) -> String {
    let e = &outcome.export;
    tr!(
        "slice-facts",
        layers = e.layers,
        time = duration(e.estimated_time_s),
        volume = decimal(outcome.resin.volume_ml, 1),
    )
}

/// The messages of a finished slice: its facts, with the summary as detail, and the repairs it
/// applied, when any.
fn finished_messages(outcome: &SliceOutcome) -> Vec<Message> {
    let mut messages =
        vec![
            Message::new(Level::Success, tr!("slice-done", facts = facts(outcome)))
                .detail(outcome.summary.trim_end())
                .lasting(Lasting::For(READING_TIME)),
        ];
    let repairs: Vec<&str> = outcome
        .summary
        .lines()
        .filter_map(|line| line.strip_prefix("Repairs: "))
        .filter(|&repairs| repairs != "none")
        .collect();
    if !repairs.is_empty() {
        messages.push(
            Message::new(Level::Info, tr!("slice-repairs-applied")).detail(repairs.join("\n")),
        );
    }
    let slivers = outcome
        .summary
        .lines()
        .filter_map(|line| line.strip_prefix("Reversed slivers left empty: "))
        .find(|value| !value.starts_with("none"));
    if let Some(slivers) = slivers {
        messages.push(
            Message::new(Level::Info, tr!("slice-slivers-left"))
                .detail(tr!("slice-slivers-detail", slivers = slivers)),
        );
    }
    // A cavity no hole opens prints, but traps uncured resin (M6.4h): said, never a plain
    // success.
    let enclosed = outcome
        .summary
        .lines()
        .find(|line| line.starts_with("Drainage: ") && line.contains("trap uncured resin"));
    if let Some(line) = enclosed {
        messages.push(
            Message::new(Level::Warning, tr!("slice-cavity-enclosed"))
                .detail(line.trim_start_matches("Drainage: ")),
        );
    }
    // Supports with errors print as they are built (owner, 2026-10-27, TD-108): said, not
    // refused.
    let support_errors: Vec<&str> = outcome
        .summary
        .lines()
        .filter_map(|line| line.strip_prefix("Supports sliced with errors: "))
        .collect();
    if !support_errors.is_empty() {
        let findings: Vec<&str> = outcome
            .summary
            .lines()
            .filter_map(|line| line.strip_prefix("Support finding: error: "))
            .collect();
        messages.push(
            Message::new(Level::Warning, tr!("slice-support-errors")).detail(
                support_errors
                    .into_iter()
                    .chain(findings)
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
        );
    }
    messages
}

/// Copy `from` (a file, or the reference format's folder) to `to` through a temporary sibling
/// renamed into place, so a failure never leaves a partial result.
fn publish(from: &Path, to: &Path) -> std::io::Result<()> {
    let staging = to.with_extension("karasu-partial");
    let result = if from.is_dir() {
        copy_folder(from, &staging)
    } else {
        std::fs::copy(from, &staging).map(|_| ())
    };
    if let Err(error) = result {
        remove(&staging);
        return Err(error);
    }
    if to.exists() {
        // The save dialog has confirmed replacing it.
        remove(to);
    }
    std::fs::rename(&staging, to).inspect_err(|_| remove(&staging))
}

/// A slicing let go (its plate deleted, another project opened, the application closing) takes
/// its files with it: the last slice's, and a running job's output should it be there already
/// (the job, cancelled, publishes nothing new).
impl Drop for SlicingUi {
    fn drop(&mut self) {
        if let Some(running) = self.running.take() {
            let output = running.output.clone();
            drop(running);
            remove(&output);
        }
        if let Some(sliced) = self.sliced.take() {
            remove(&sliced.output);
        }
    }
}

/// The messages of a batch's slice of plate `plate`, from index `start` of `outbox` on: its
/// plain success left out, the others naming the plate.
fn name_plate(outbox: &mut Vec<Message>, start: usize, plate: &str) {
    let mut kept = outbox.split_off(start);
    if kept.first().is_some_and(|m| m.level == Level::Success) {
        kept.remove(0);
    }
    for message in &mut kept {
        message.text = tr!("batch-plate-message", plate = plate, text = &message.text);
    }
    outbox.extend(kept);
}

/// The bytes of the file or folder at `path`; 0 for what cannot be read.
fn size_of(path: &Path) -> u64 {
    if path.is_dir() {
        std::fs::read_dir(path).map_or(0, |entries| {
            entries
                .filter_map(Result::ok)
                .map(|entry| size_of(&entry.path()))
                .sum()
        })
    } else {
        std::fs::metadata(path).map_or(0, |m| m.len())
    }
}

fn copy_folder(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_folder(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

fn remove(path: &Path) {
    let _ = if path.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    };
}

/// Seconds as `h:mm:ss` or `m:ss`.
pub(crate) fn duration(seconds: f64) -> String {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // Print times.
    let s = seconds.max(0.0).round() as u64;
    let (h, m, s) = (s / 3600, s / 60 % 60, s % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

#[cfg(test)]
pub(crate) mod tests {
    /// Mark `slicing`'s last slice as failed, as a test needs a plate's slicing to tell apart.
    pub(crate) fn fail_for_test(slicing: &mut super::SlicingUi) {
        slicing.failed = Some("failed".to_owned());
    }

    use super::*;
    use karasu_project::request::format_ids;

    #[test]
    fn durations_and_reduced_masks() {
        assert_eq!(duration(75.4), "1:15");
        assert_eq!(duration(3_725.0), "1:02:05");
        // A 10 x 4 mask with one lit pixel at (7, 1), reduced by 4: the pixel survives.
        let mut pixels = vec![0u8; 40];
        pixels[10 + 7] = 200;
        let mask = SpanMask::from_pixels(10, 4, &pixels).unwrap();
        let image = reduced(&mask, 3);
        assert_eq!(image.size, [3, 1]);
        assert_eq!(image.pixels[1], egui::Color32::from_gray(200));
        assert_eq!(image.pixels[0], egui::Color32::from_gray(0));
    }

    #[test]
    fn layers_are_shown_as_the_plate_seen_from_above() {
        // A 3 x 2 panel image with one lit pixel at column 0, row 0: world X and Y minimum, the
        // front-left corner when the panel is not turned.
        let lit = egui::Color32::from_gray(255);
        let mut panel = egui::ColorImage::new([3, 2], vec![egui::Color32::BLACK; 6]);
        panel.pixels[0] = lit;
        let lit_at = |image: &egui::ColorImage| {
            let index = image.pixels.iter().position(|&p| p == lit).unwrap();
            (image.size, index % image.size[0], index / image.size[0])
        };
        let turned = |swap_xy, flip_x, flip_y| Orientation {
            swap_xy,
            flip_x,
            flip_y,
        };
        // The front-left corner is shown at the bottom left.
        assert_eq!(
            lit_at(&plate_view(&panel, turned(false, false, false))),
            ([3, 2], 0, 1)
        );
        // Mirrored columns: the pixel is world X maximum, the right.
        assert_eq!(
            lit_at(&plate_view(&panel, turned(false, true, false))),
            ([3, 2], 2, 1)
        );
        // Mirrored rows: world Y maximum, the back, at the top.
        assert_eq!(
            lit_at(&plate_view(&panel, turned(false, false, true))),
            ([3, 2], 0, 0)
        );
        // Swapped axes: panel columns run along world Y, so the plate is 2 wide and 3 deep.
        assert_eq!(
            lit_at(&plate_view(&panel, turned(true, false, false))),
            ([2, 3], 0, 2)
        );
        // Panel column 2, row 0 with swapped axes is world X 0, Y 2: the back-left corner.
        let mut panel = egui::ColorImage::new([3, 2], vec![egui::Color32::BLACK; 6]);
        panel.pixels[2] = lit;
        assert_eq!(
            lit_at(&plate_view(&panel, turned(true, false, false))),
            ([2, 3], 0, 0)
        );
    }

    #[test]
    fn verification_marks_are_painted_over_the_reduced_layer() {
        // A 10 x 4 mask reduced by 4: pockets, then thin walls, then breaches on top.
        let mask = SpanMask::from_pixels(10, 4, &[255; 40]).unwrap();
        let mut image = reduced(&mask, 3);
        let marks = LayerMarks {
            thin: vec![[1, 5, 6]],
            breach: vec![[2, 4, 5]],
            pocket: vec![[1, 0, 2, 0], [2, 4, 9, 1]],
        };
        mark(&mut image, &mask, &marks, None, 3);
        assert_eq!(image.size, [3, 1]);
        assert_eq!(image.pixels[0], POCKET.color());
        assert_eq!(image.pixels[1], BREACH.color());
        assert_eq!(image.pixels[2], POCKET.color());
    }

    #[test]
    fn the_focused_pocket_is_selected_and_ringed() {
        // A 400 x 400 empty layer reduced by 1, with two pockets; the second is focused.
        let mask = SpanMask::from_pixels(400, 400, &vec![0; 160_000]).unwrap();
        let mut image = reduced(&mask, 400);
        let marks = LayerMarks {
            pocket: vec![[100, 100, 103, 0], [200, 200, 201, 1]],
            ..LayerMarks::default()
        };
        let focus = Focus {
            layer: 0,
            pixel: [200, 200],
            pocket: Some(1),
        };
        mark(&mut image, &mask, &marks, Some(focus), 400);
        let at = |x: usize, y: usize| image.pixels[y * 400 + x];
        assert_eq!(at(100, 100), POCKET.color());
        assert_eq!(at(200, 200), SELECTED.color());
        // The ring's corner and the cross's arm.
        assert_eq!(at(200 + FOCUS_RING, 200 + FOCUS_RING), SELECTED.color());
        assert_eq!(at(200 + FOCUS_ARM, 200), SELECTED.color());
        // Away from it, nothing.
        assert_eq!(at(300, 300), egui::Color32::from_gray(0));
        // Material under the map stays white, as exported (owner, M6.5).
        let solid = SpanMask::from_pixels(4, 1, &[255; 4]).unwrap();
        let mut image = reduced(&solid, 4);
        mark(&mut image, &solid, &LayerMarks::default(), None, 4);
        assert_eq!(image.pixels[0], egui::Color32::from_gray(255));
    }

    /// A workspace with an 8 mm cube hollowed at 1.5 mm on a preset printer, in `folder`.
    pub(crate) fn hollowed_cube(folder: &Path) -> Workspace {
        use karasu_project::project::Hollowing;
        std::fs::create_dir_all(folder).unwrap();
        let model = folder.join("cube.stl");
        let stl = crate::workspace::tests::box_stl([0.0; 3], [8.0; 3]);
        std::fs::write(&model, stl).unwrap();
        let mut workspace = Workspace::new();
        workspace.open(model);
        crate::workspace::tests::wait(&mut workspace);
        let (printer, resin, setting) =
            crate::workspace::tests::catalog_profiles("anycubic-photon-mono-x");
        let supported: Vec<String> = format_ids().split(", ").map(str::to_owned).collect();
        let format = printer
            .format_ids
            .iter()
            .find(|f| supported.contains(f))
            .unwrap()
            .clone();
        workspace.edit(|plate| {
            plate.printer = Some(printer);
            plate.resin = Some(resin);
            plate.print_setting = Some(setting);
            plate.settings.format = format;
            plate.objects[0].set_hollowing(Some(Hollowing {
                wall_mm: 1.5,
                cell_mm: None,
                simplify: None,
            }));
        });
        workspace
    }

    /// Slicing for tests, writing into `folder`.
    pub(crate) fn slicing_into(folder: &Path) -> SlicingUi {
        SlicingUi {
            cache: Some(folder.to_owned()),
            running: None,
            sliced: None,
            outbox: Vec::new(),
            layer: 0,
            focus: None,
            texture: None,
            advanced: false,
            verify: false,
            failed: None,
            view: layers::View::default(),
            show_layers: false,
            edit_setting: false,
        }
    }

    /// The last slice's file in the cache.
    pub(crate) fn output(slicing: &SlicingUi) -> Option<PathBuf> {
        slicing.sliced.as_ref().map(|s| s.output.clone())
    }

    /// The last slice's summary.
    pub(crate) fn summary(slicing: &SlicingUi) -> Option<&str> {
        slicing.sliced.as_ref().map(|s| s.outcome.summary.as_str())
    }

    /// Poll until the running job ends.
    pub(crate) fn finish(slicing: &mut SlicingUi) {
        let started = Instant::now();
        while slicing.running.is_some() {
            slicing.poll();
            assert!(started.elapsed() < Duration::from_secs(300), "no slice");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn two_plates_slicings_keep_their_own_files() {
        // TD-135: every plate's slicing writes into the same folder; one plate's slice must
        // never take another's file, and a new slicing must not empty the folder.
        let folder =
            std::env::temp_dir().join(format!("karasu-app-sibling-{}", std::process::id()));
        let workspace = hollowed_cube(&folder);
        let mut first = slicing_into(&folder);
        first.start(&workspace);
        finish(&mut first);
        let mut second = first.sibling();
        let _ = SlicingUi::new();
        second.start(&workspace);
        finish(&mut second);
        let (a, b) = (
            &first.sliced.as_ref().unwrap().output,
            &second.sliced.as_ref().unwrap().output,
        );
        assert_ne!(a, b);
        assert!(a.exists() && b.exists(), "{a:?} {b:?}");
        // A slicing let go takes its file with it, and only its own.
        let (a, b) = (a.clone(), b.clone());
        drop(second);
        assert!(a.exists());
        assert!(!b.exists());
        drop(first);
        assert!(!a.exists());
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn only_the_slices_of_sessions_that_ended_are_removed() {
        // Another application running beside this one keeps its folder; an ended session's
        // folder, one without a lock and an earlier version's loose file go.
        let root = std::env::temp_dir().join(format!("karasu-app-sessions-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for name in ["running", "ended", "unlocked"] {
            std::fs::create_dir_all(root.join(name)).unwrap();
            std::fs::write(root.join(name).join("slice-1.pp1"), b"x").unwrap();
        }
        let running = std::fs::File::create(root.join("running").join(SESSION_LOCK)).unwrap();
        running.lock().unwrap();
        std::fs::File::create(root.join("ended").join(SESSION_LOCK)).unwrap();
        std::fs::write(root.join("slice-3.goo"), b"x").unwrap();
        remove_ended_sessions(&root);
        let mut left: Vec<String> = std::fs::read_dir(&root)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(left, ["running"]);
        assert!(root.join("running").join("slice-1.pp1").exists());
        drop(running);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_session_log_describes_a_hollowed_drained_verified_slice() {
        // TD-096: the only test of this binary that installs the process's logger, in memory.
        use karasu_project::{
            logging::{install, Destination, LogConfig},
            request::DrainHole,
        };
        let log = install(LogConfig::new(Destination::Memory)).unwrap();
        let folder = std::env::temp_dir().join(format!("karasu-app-log-{}", std::process::id()));
        let mut workspace = hollowed_cube(&folder);
        workspace.add_holes(
            0,
            vec![DrainHole::Surface {
                surface_mm: [4.0, 4.0, 8.0],
                diameter_mm: 2.0,
                outside_mm: 1.0,
                inside_mm: 1.0,
                tilt: None,
                depth_mm: None,
                surface: karasu_core::preparation::sampled::HoleSurface::Outer,
            }],
        );
        let mut slicing = slicing_into(&folder);
        slicing.start_job(&workspace, workspace.plate_id(), true, None);
        finish(&mut slicing);
        // As the main window does: messages shown are recorded too.
        let mut messages = crate::messages::Messages::default();
        for message in slicing.take_messages() {
            messages.push(message);
        }
        let records: Vec<String> = log
            .ring()
            .since(0)
            .into_iter()
            .map(|r| format!("{} {}", r.target, r.message))
            .collect();
        let has = |prefix: &str, text: &str| {
            records
                .iter()
                .any(|r| r.starts_with(prefix) && r.contains(text))
        };
        assert!(has("models opening", "cube.stl"), "{records:#?}");
        assert!(has("models imported", "12 triangles"), "{records:#?}");
        assert!(
            has("slice slicing to", "verifying the walls"),
            "{records:#?}"
        );
        assert!(has("hollow hollowed", "guaranteed"), "{records:#?}");
        assert!(has("holes ", "drain-hole index built"), "{records:#?}");
        assert!(
            has("verify printed wall", "0 breached voxels"),
            "{records:#?}"
        );
        assert!(has("slice Verifying walls:", " s"), "{records:#?}");
        assert!(has("slice sliced", "layers to"), "{records:#?}");
        assert!(
            has("slice summary:", "Wall verification: passed"),
            "{records:#?}"
        );
        assert!(has("messages ", "Walls verified"), "{records:#?}");
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn verifying_walls_slices_again_and_keeps_the_layer_shown() {
        let folder = std::env::temp_dir().join(format!("karasu-app-verify-{}", std::process::id()));
        let workspace = hollowed_cube(&folder);
        let mut slicing = slicing_into(&folder);
        slicing.start(&workspace);
        finish(&mut slicing);
        let messages = slicing.take_messages();
        assert!(slicing.sliced.is_some(), "{messages:?}");
        assert!(slicing
            .sliced
            .as_ref()
            .unwrap()
            .outcome
            .verification
            .is_none());
        slicing.layer = 40;
        slicing.take_messages();
        slicing.start_job(&workspace, workspace.plate_id(), true, None);
        finish(&mut slicing);
        let sliced = slicing.sliced.as_ref().unwrap();
        let verification = sliced.outcome.verification.as_ref().unwrap();
        assert_eq!(verification.failure, None, "{}", sliced.outcome.summary);
        assert_eq!(verification.report.pockets.len(), 1);
        assert_eq!(verification.report.marks.len(), sliced.masks.len());
        assert_eq!(slicing.layer, 40);
        let messages = slicing.take_messages();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].level, Level::Success);
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn publishing_replaces_atomically() {
        let folder =
            std::env::temp_dir().join(format!("karasu-app-publish-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let (from, to) = (folder.join("a.goo"), folder.join("b.goo"));
        std::fs::write(&from, b"new").unwrap();
        std::fs::write(&to, b"old").unwrap();
        publish(&from, &to).unwrap();
        assert_eq!(std::fs::read(&to).unwrap(), b"new");
        assert!(!to.with_extension("karasu-partial").exists());
        let _ = std::fs::remove_dir_all(&folder);
    }
}
