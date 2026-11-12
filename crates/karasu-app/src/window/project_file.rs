//! The project file in the window (TD-130): New, Open, Open recent, Save and Save as; the
//! unsaved-changes question; the window's title; and the recovery copy.
//!
//! **Unsaved changes.** The project is unsaved while any plate's revision differs from the one
//! it was saved at or opened with (revisions only grow, so an edit undone still counts). An
//! untitled project with no model holds nothing to lose and is never unsaved. Closing, New and
//! Open then ask, in a modal dialog, to save, discard or cancel.
//!
//! **Off the interface thread.** Saving takes the project as it stands (its plates' revisions
//! recorded) and writes it on a thread of its own; opening reads and checks the file on one, and
//! the models are imported again in the background as when a model is opened.
//!
//! **The recovery copy.** While the project is unsaved, it is kept every few minutes (a setting,
//! 0 to turn it off) in a folder of this session under the application's data, unpacked: the
//! document rewritten each time, each model written once. The session holds the folder's lock
//! file; the folder is emptied when the project is saved and removed when the application closes
//! normally. At start, the folders whose lock no session holds are offered: restore one as an
//! unsaved project, or discard them.
use super::MainWindow;
use crate::{
    dialog::{self, Part, Size},
    i18n::tr,
    messages::{Level, Message},
    settings::{ProfileChoice, ProjectSettings, RECENT_PROJECTS, RECOVERY_MINUTES},
    workspace::Workspace,
};
use eframe::egui;
use karasu_project::project::{
    file::{self, EXTENSION},
    PlateId, Project,
};
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    path::{Path, PathBuf},
    sync::mpsc,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

/// What waits on the unsaved-changes question, or on a save that answers it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Intent {
    /// Start a new project.
    New,
    /// Open this project file, or the one the file dialog chooses.
    Open(Option<PathBuf>),
    /// Restore the recovery copy in this folder.
    Restore(PathBuf),
    /// Close the application.
    Quit,
}

/// A save running on its thread.
struct Saving {
    path: PathBuf,
    /// The project's state as it is saved.
    state: Vec<(PlateId, u64)>,
    result: mpsc::Receiver<Result<(), String>>,
}

/// An open running on its thread: of a project file, or of a recovery copy.
struct Opening {
    /// The project file, or where a recovered project was saved last.
    path: Option<PathBuf>,
    /// The recovery copy's folder, removed once it is restored.
    recovered: Option<PathBuf>,
    result: mpsc::Receiver<Result<Project, String>>,
}

/// A project being opened, shown in a dialog until all of it is ready to show at once: read,
/// its models imported, prepared, and its supports built (owner, 2026-11-04).
struct Loading {
    /// The file's name, or the recovered project's.
    name: String,
    /// Said once it is shown: opened, or restored.
    done: Option<Message>,
}

/// What a recovery folder says about its project.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
struct SessionNote {
    /// The project's name.
    name: String,
    /// Where it was saved last, if it was.
    path: Option<PathBuf>,
    /// When the copy was written, in seconds since 1970.
    written_s: u64,
}

/// A recovery copy left by a session that did not end normally.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Orphan {
    folder: PathBuf,
    note: SessionNote,
}

/// The recovery folder's files: the lock, the note, and the project unpacked.
const LOCK: &str = "lock";
const NOTE: &str = "session.json";
const PROJECT: &str = "project";

/// This session's recovery copy.
#[derive(Default)]
struct Recovery {
    /// The recovery folders' root, when the application keeps settings (not in tests).
    root: Option<PathBuf>,
    /// This session's folder and the lock held on it, once a copy was written.
    session: Option<(PathBuf, File)>,
    /// When the last copy was written, and the project's state then.
    written: Option<(Instant, Vec<(PlateId, u64)>)>,
    /// A copy being written.
    writing: Option<mpsc::Receiver<Result<(), String>>>,
    /// A failure was reported: later ones are logged only.
    failure_reported: bool,
    /// The copies other sessions left, offered at start.
    orphans: Vec<Orphan>,
    /// Whether the orphans were looked for.
    searched: bool,
}

/// The window's project file.
pub(super) struct ProjectFile {
    /// Where the project is saved, once it is.
    pub(super) path: Option<PathBuf>,
    /// The project's state as saved or opened.
    saved: Vec<(PlateId, u64)>,
    saving: Option<Saving>,
    opening: Option<Opening>,
    /// What waits on the unsaved-changes question.
    asking: Option<Intent>,
    /// What runs on the next frame, once nothing needs asking.
    queued: Option<Intent>,
    /// What runs once the save in progress succeeds.
    after_save: Option<Intent>,
    /// The recent project files, newest first.
    recent: Vec<PathBuf>,
    /// How often the recovery copy is written, in minutes; 0 never.
    recovery_minutes: u32,
    recovery: Recovery,
    /// The application may close: the question was answered.
    closing: bool,
    /// A project being opened, until all of it shows.
    loading: Option<Loading>,
    /// What the title last set says: unsaved, the project's name and its file.
    title: Option<(bool, String, Option<PathBuf>)>,
    /// Ask before discarding unsaved changes; a smoke test closes without asking.
    pub(super) ask: bool,
}

impl Default for ProjectFile {
    fn default() -> Self {
        Self {
            path: None,
            saved: Vec::new(),
            saving: None,
            opening: None,
            asking: None,
            queued: None,
            after_save: None,
            recent: Vec::new(),
            recovery_minutes: ProjectSettings::default().recovery_minutes,
            recovery: Recovery::default(),
            closing: false,
            loading: None,
            title: None,
            ask: true,
        }
    }
}

impl ProjectFile {
    /// The settings it keeps.
    pub(super) fn settings(&self) -> ProjectSettings {
        ProjectSettings {
            recent: self.recent.clone(),
            recovery_minutes: self.recovery_minutes,
        }
    }

    /// Take `settings` up.
    pub(super) fn apply(&mut self, settings: &ProjectSettings) {
        self.recent.clone_from(&settings.recent);
        self.recent.truncate(RECENT_PROJECTS);
        self.recovery_minutes = if RECOVERY_MINUTES.contains(&settings.recovery_minutes) {
            settings.recovery_minutes
        } else {
            ProjectSettings::default().recovery_minutes
        };
    }

    /// The recovery copy's interval, in minutes, for the settings dialog.
    pub(super) fn recovery_minutes(&mut self) -> &mut u32 {
        &mut self.recovery_minutes
    }

    /// Keep recovery copies under `root` (the application's data), and look for those left.
    pub(super) fn keep_recovery_in(&mut self, root: PathBuf) {
        self.recovery.root = Some(root);
    }

    /// The recent project files, newest first.
    pub(super) fn recent(&self) -> &[PathBuf] {
        &self.recent
    }

    /// Put `path` first among the recent files.
    fn remember(&mut self, path: &Path) {
        self.recent.retain(|p| p != path);
        self.recent.insert(0, path.to_path_buf());
        self.recent.truncate(RECENT_PROJECTS);
    }

    /// Whether a save or an open is running.
    pub(super) fn busy(&self) -> bool {
        self.saving.is_some() || self.opening.is_some()
    }

    /// Whether a project being opened is not ready to show yet: the scene draws no model.
    pub(super) fn loading(&self) -> bool {
        self.loading.is_some()
    }
}

/// The path with the project file's extension, added when it has another or none.
fn with_extension(path: PathBuf) -> PathBuf {
    if path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case(EXTENSION))
    {
        path
    } else {
        let mut name = path.into_os_string();
        name.push(".");
        name.push(EXTENSION);
        PathBuf::from(name)
    }
}

/// Whether `path` names a project file.
pub(super) fn is_project(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case(EXTENSION))
}

/// Seconds since 1970, now.
fn now_s() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// How long ago `written_s` was, in words.
fn ago(written_s: u64) -> String {
    let minutes = now_s().saturating_sub(written_s) / 60;
    match minutes {
        0 => tr!("recovery-just-now"),
        1..=59 => tr!("recovery-minutes-ago", count = minutes),
        60..=2879 => tr!("recovery-hours-ago", count = minutes / 60),
        _ => tr!("recovery-days-ago", count = minutes / 1440),
    }
}

/// The recovery folders under `root` no running session holds, newest first.
fn orphans(root: &Path) -> Vec<Orphan> {
    let mut found: Vec<Orphan> = std::fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|folder| folder.is_dir())
        .filter(|folder| {
            // A lock no session holds: its session ended without removing the folder.
            File::options()
                .write(true)
                .create(true)
                .truncate(false)
                .open(folder.join(LOCK))
                .is_ok_and(|lock| lock.try_lock().is_ok())
        })
        .filter_map(|folder| {
            let note = std::fs::read(folder.join(NOTE))
                .ok()
                .and_then(|bytes| serde_json::from_slice::<SessionNote>(&bytes).ok());
            match note {
                Some(note) if folder.join(PROJECT).is_dir() => Some(Orphan { folder, note }),
                // A folder with nothing to restore is removed.
                _ => {
                    let _ = std::fs::remove_dir_all(&folder);
                    None
                }
            }
        })
        .collect();
    found.sort_by_key(|o| std::cmp::Reverse(o.note.written_s));
    found
}

impl MainWindow {
    /// Whether the project has changes to lose: edited since saved or opened, and holding
    /// something (a file, or a model).
    pub(super) fn unsaved(&self) -> bool {
        let file = &self.project_file;
        let holds = file.path.is_some()
            || self
                .workspace
                .project()
                .plates()
                .iter()
                .any(|p| !p.content().objects.is_empty());
        holds && !self.workspace.is_state(&file.saved)
    }

    /// Do `intent`, once unsaved changes are saved or discarded.
    pub(super) fn ask(&mut self, intent: Intent) {
        // A save in progress finishes first: closing waits for it.
        if self.project_file.saving.is_some() {
            if intent == Intent::Quit {
                self.project_file.after_save = Some(Intent::Quit);
            }
            return;
        }
        if self.project_file.busy() && intent != Intent::Quit {
            return;
        }
        if self.unsaved() && self.project_file.ask {
            self.project_file.asking = Some(intent);
        } else {
            self.project_file.queued = Some(intent);
        }
    }

    /// The file dialog's project, opened once unsaved changes are answered; models and
    /// profiles chosen beside it are opened into the current project first.
    pub(super) fn open_files(&mut self, files: Vec<PathBuf>) {
        let (projects, others): (Vec<_>, Vec<_>) = files.into_iter().partition(|f| is_project(f));
        for file in others {
            self.workspace.open(file);
            self.fit_pending = true;
        }
        let mut projects = projects.into_iter();
        if let Some(project) = projects.next() {
            if projects.next().is_some() {
                self.messages
                    .push(Message::new(Level::Warning, tr!("project-one-at-a-time")));
            }
            self.ask(Intent::Open(Some(project)));
        }
    }

    /// Save where the project was saved or opened, or else where the user chooses.
    pub(super) fn save(&mut self) {
        match self.project_file.path.clone() {
            Some(path) => self.start_save(path),
            None => self.save_as(),
        }
    }

    /// Save where the user chooses now.
    pub(super) fn save_as(&mut self) {
        let name = self.workspace.project().name.clone();
        let mut dialog = rfd::FileDialog::new()
            .set_title(tr!("dialog-save-title"))
            .add_filter(tr!("dialog-filter-projects"), &[EXTENSION])
            .set_file_name(format!("{name}.{EXTENSION}"));
        if let Some(folder) = self.project_file.path.as_ref().and_then(|p| p.parent()) {
            dialog = dialog.set_directory(folder);
        }
        match dialog.save_file() {
            Some(path) => self.start_save(with_extension(path)),
            // Cancelled: what waited on the save does not happen.
            None => self.project_file.after_save = None,
        }
    }

    /// Save the project as it stands to `path`, on a thread of its own.
    fn start_save(&mut self, path: PathBuf) {
        if self.project_file.saving.is_some() {
            return;
        }
        if let Some(stem) = path.file_stem() {
            self.workspace
                .set_project_name(stem.to_string_lossy().into_owned());
        }
        let project = self.workspace.project().clone();
        let state = self.workspace.state();
        let (sender, result) = mpsc::channel();
        let target = path.clone();
        log::info!(target: "project", "saving {}", path.display());
        thread::spawn(move || {
            let started = Instant::now();
            let saved = file::save(&project, &target).map_err(|e| e.to_string());
            log::info!(
                target: "project",
                "saved {} in {:.2} s",
                target.display(),
                started.elapsed().as_secs_f64()
            );
            // The receiver is gone only if the window was dropped.
            let _ = sender.send(saved);
        });
        self.project_file.saving = Some(Saving {
            path,
            state,
            result,
        });
    }

    /// Open the project file `path`, or the recovery copy in `recovered`, on a thread.
    fn start_open(&mut self, path: Option<PathBuf>, recovered: Option<PathBuf>) {
        let (sender, result) = mpsc::channel();
        let source = recovered.clone().map_or_else(
            || path.clone().map(|p| (p, false)),
            |folder| Some((folder.join(PROJECT), true)),
        );
        let Some((source, folder)) = source else {
            return;
        };
        log::info!(target: "project", "opening {}", source.display());
        let name = path.as_ref().and_then(|p| p.file_name()).map_or_else(
            || tr!("project-untitled"),
            |n| n.to_string_lossy().into_owned(),
        );
        self.project_file.loading = Some(Loading { name, done: None });
        thread::spawn(move || {
            let opened = if folder {
                file::open_folder(&source)
            } else {
                file::open(&source)
            };
            // The receiver is gone only if the window was dropped.
            let _ = sender.send(opened.map_err(|e| e.to_string()));
        });
        self.project_file.opening = Some(Opening {
            path,
            recovered,
            result,
        });
    }

    /// Open the project files named on the command line: the first, as the session starts.
    pub(super) fn open_at_start(&mut self, path: PathBuf) {
        self.start_open(Some(path), None);
    }

    /// Put `workspace` in place of the current one: the tools start over on it, with the
    /// user's settings and their values at the defaults, as with a new project (TD-097), and
    /// the printer, resin and print setting last used when `restore_profiles` (a new project;
    /// an opened one keeps its own).
    fn replace_workspace(
        &mut self,
        ctx: &egui::Context,
        workspace: Workspace,
        restore_profiles: bool,
    ) {
        let mut settings = self.collect_settings(ctx);
        if !restore_profiles {
            settings.profiles = ProfileChoice::default();
        }
        // The plates' identities and revisions start over in the new project: nothing kept for
        // the last project's plates may be taken for its own.
        self.forget_plates();
        self.workspace = workspace;
        self.selected = None;
        self.deleting = None;
        self.clearing = None;
        self.drag = None;
        self.preparation = crate::preparation_ui::PreparationUi::default();
        self.hollowing = crate::hollowing_ui::HollowingUi::default();
        self.supports = crate::supports_ui::SupportsUi::new();
        self.raft = crate::raft_ui::RaftUi::default();
        self.islands = crate::islands_ui::IslandsUi::new();
        self.auto = crate::auto_ui::AutoUi::new();
        self.grid = crate::grid_ui::GridUi::new();
        self.orient = crate::orient_ui::OrientUi::new();
        self.slicing = crate::slicing_ui::SlicingUi::new();
        self.section = crate::section_ui::SectionUi::default();
        self.region_view = None;
        self.followed = None;
        self.notices_taken = 0;
        self.fit_pending = true;
        // The viewport starts over: what the last project uploaded goes with it.
        self.viewport = None;
        self.lines_built = false;
        self.lines_printer = None;
        self.lines_look = None;
        self.apply_settings(ctx, &settings);
        self.project_file.saved = self.workspace.state();
    }

    /// Run what was asked for, now that nothing needs asking.
    fn run(&mut self, ctx: &egui::Context, intent: Intent) {
        match intent {
            Intent::New => {
                self.replace_workspace(ctx, Workspace::new(), true);
                self.project_file.path = None;
                self.project_file.saved = self.workspace.state();
                self.clear_recovery();
            }
            Intent::Open(Some(path)) => self.start_open(Some(path), None),
            Intent::Open(None) => {
                let mut dialog = rfd::FileDialog::new()
                    .set_title(tr!("dialog-open-project-title"))
                    .add_filter(tr!("dialog-filter-projects"), &[EXTENSION]);
                if let Some(folder) = self.project_file.recent.first().and_then(|p| p.parent()) {
                    dialog = dialog.set_directory(folder);
                }
                if let Some(path) = dialog.pick_file() {
                    self.start_open(Some(path), None);
                }
            }
            Intent::Restore(folder) => {
                let path = self
                    .project_file
                    .recovery
                    .orphans
                    .iter()
                    .find(|o| o.folder == folder)
                    .and_then(|o| o.note.path.clone());
                self.project_file
                    .recovery
                    .orphans
                    .retain(|o| o.folder != folder);
                self.start_open(path, Some(folder));
            }
            Intent::Quit => {
                self.project_file.closing = true;
                self.chrome.quit = true;
            }
        }
    }

    /// Each frame: saves and opens that finished, what was asked for, the questions, the
    /// window's title and the recovery copy.
    pub(super) fn project_frame(&mut self, ctx: &egui::Context) {
        self.poll_save();
        self.poll_open(ctx);
        // Closing the window with unsaved changes asks first, and waits for a save running.
        if ctx.input(|i| i.viewport().close_requested())
            && !self.project_file.closing
            && ((self.project_file.ask && self.unsaved()) || self.project_file.saving.is_some())
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.ask(Intent::Quit);
        }
        if let Some(intent) = self.project_file.queued.take() {
            self.run(ctx, intent);
        }
        self.loading_dialog(ctx);
        self.unsaved_question(ctx);
        self.recovery_question(ctx);
        self.keep_title(ctx);
        self.keep_recovery();
        if self.project_file.busy()
            || self.project_file.loading()
            || self.project_file.recovery.writing.is_some()
        {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }

    fn poll_save(&mut self) {
        let Some(saving) = &self.project_file.saving else {
            return;
        };
        let result = match saving.result.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => Err("the save stopped unexpectedly".into()),
        };
        let Some(saving) = self.project_file.saving.take() else {
            return;
        };
        let name = saving
            .path
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        match result {
            Ok(()) => {
                self.project_file.remember(&saving.path);
                self.project_file.path = Some(saving.path);
                self.project_file.saved = saving.state;
                self.messages.push(Message::new(
                    Level::Success,
                    tr!("project-saved", name = &name),
                ));
                self.clear_recovery();
                if let Some(intent) = self.project_file.after_save.take() {
                    self.project_file.queued = Some(intent);
                }
            }
            Err(error) => {
                log::warn!(target: "project", "not saved: {error}");
                self.project_file.after_save = None;
                self.messages.push(
                    Message::new(Level::Error, tr!("project-not-saved", name = &name))
                        .detail(error),
                );
            }
        }
    }

    fn poll_open(&mut self, ctx: &egui::Context) {
        let Some(opening) = &self.project_file.opening else {
            return;
        };
        let result = match opening.result.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => Err("opening stopped unexpectedly".into()),
        };
        let Some(opening) = self.project_file.opening.take() else {
            return;
        };
        let shown = opening
            .path
            .as_ref()
            .and_then(|p| p.file_name())
            .map_or_else(
                || tr!("project-untitled"),
                |n| n.to_string_lossy().into_owned(),
            );
        match result {
            Ok(project) => {
                let name = project.name.clone();
                self.replace_workspace(ctx, Workspace::with_project(project), false);
                // Said once all of it shows.
                let done = if let Some(folder) = &opening.recovered {
                    // Restored: unsaved until it is saved again, where it was saved before.
                    self.project_file.saved = Vec::new();
                    self.project_file.path = opening.path;
                    let _ = std::fs::remove_dir_all(folder);
                    Message::new(Level::Success, tr!("recovery-restored", name = &name))
                } else {
                    if let Some(path) = &opening.path {
                        self.project_file.remember(path);
                    }
                    self.project_file.path = opening.path;
                    self.clear_recovery();
                    Message::new(Level::Success, tr!("project-opened", name = &shown))
                };
                self.project_file.loading = Some(Loading {
                    name: shown,
                    done: Some(done),
                });
            }
            Err(error) => {
                self.project_file.loading = None;
                log::warn!(target: "project", "not opened: {error}");
                self.messages.push(
                    Message::new(Level::Error, tr!("project-not-opened", name = &shown))
                        .detail(error),
                );
            }
        }
    }

    /// Open a recent project file, or say it is no longer there and forget it.
    pub(super) fn open_recent(&mut self, path: PathBuf) {
        if path.is_file() {
            self.ask(Intent::Open(Some(path)));
        } else {
            self.project_file.recent.retain(|p| *p != path);
            self.messages.push(
                Message::new(Level::Warning, tr!("project-recent-missing"))
                    .detail(path.display().to_string()),
            );
        }
    }

    /// What a project being opened is doing, or `None` once all of it is ready to show.
    fn loading_stage(&self) -> Option<String> {
        if self.project_file.opening.is_some() {
            return Some(tr!("loading-reading"));
        }
        let importing: Vec<&str> = self.workspace.importing().collect();
        if !importing.is_empty() {
            return Some(tr!("status-importing", names = importing.join(", ")));
        }
        if !self.preparation.settled(&self.workspace) {
            return Some(self.preparation.running_phase().map_or_else(
                || tr!("loading-preparing"),
                |phase| tr!("preparation-running", phase = phase),
            ));
        }
        if !self.supports.built(&self.workspace, &self.preparation) {
            return Some(tr!("loading-supports"));
        }
        None
    }

    /// The dialog of a project being opened: what it is doing, until all of it shows at once;
    /// "Show now" (or Escape) shows it as it comes.
    fn loading_dialog(&mut self, ctx: &egui::Context) {
        let Some(loading) = &self.project_file.loading else {
            return;
        };
        let title = tr!("loading-title", name = &loading.name);
        let Some(stage) = self.loading_stage() else {
            if let Some(done) = self.project_file.loading.take().and_then(|l| l.done) {
                self.messages.push(done);
            }
            return;
        };
        let mut show_now = false;
        let response = dialog::show(
            ctx,
            "project-loading",
            &title,
            Size::Fixed(420.0),
            |ui, part| match part {
                Part::Body => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.add(egui::Label::new(stage.as_str()).wrap());
                    });
                }
                Part::Footer { enter } => {
                    if dialog::main_action(ui, &tr!("loading-show-now"), true, enter) {
                        show_now = true;
                    }
                }
            },
        );
        if show_now || response.cancel {
            // Shown as it comes; said when it was opened.
            if let Some(done) = self.project_file.loading.take().and_then(|l| l.done) {
                self.messages.push(done);
            }
        }
    }

    /// The unsaved-changes question: save, discard, or cancel what was asked.
    fn unsaved_question(&mut self, ctx: &egui::Context) {
        let Some(intent) = self.project_file.asking.clone() else {
            return;
        };
        let name = self.workspace.project().name.clone();
        let mut answer = None;
        let response = dialog::show(
            ctx,
            "unsaved-changes",
            &tr!("unsaved-title"),
            Size::Fixed(420.0),
            |ui, part| match part {
                Part::Body => {
                    ui.add(egui::Label::new(tr!("unsaved-text", name = &name)).wrap());
                }
                Part::Footer { enter } => {
                    if dialog::main_action(ui, &tr!("action-save"), true, enter) {
                        answer = Some(true);
                    }
                    if ui.button(tr!("unsaved-discard")).clicked() {
                        answer = Some(false);
                    }
                    if dialog::cancel_action(ui) {
                        self.project_file.asking = None;
                    }
                }
            },
        );
        if response.cancel {
            self.project_file.asking = None;
        }
        match answer {
            Some(true) => {
                self.project_file.asking = None;
                self.project_file.after_save = Some(intent);
                self.save();
            }
            Some(false) => {
                self.project_file.asking = None;
                self.project_file.queued = Some(intent);
            }
            None => {}
        }
    }

    /// The window's title: the project's name, marked while it has unsaved changes.
    fn keep_title(&mut self, ctx: &egui::Context) {
        let unsaved = self.unsaved();
        let project = &self.workspace.project().name;
        let path = &self.project_file.path;
        // Compared without allocating: the title is set again only when it changes.
        if self
            .project_file
            .title
            .as_ref()
            .is_some_and(|(u, n, p)| *u == unsaved && n == project && p == path)
        {
            return;
        }
        self.project_file.title = Some((unsaved, project.clone(), path.clone()));
        let name = self.project_file.path.as_ref().map_or_else(
            || self.workspace.project().name.clone(),
            |p| {
                p.file_name()
                    .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
            },
        );
        let mark = if unsaved { "• " } else { "" };
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(format!(
            "{mark}{name} — {} {}",
            karasu_core::APPLICATION_NAME,
            env!("CARGO_PKG_VERSION")
        )));
    }

    /// Write the recovery copy when it is due, or empty it once nothing is unsaved.
    fn keep_recovery(&mut self) {
        let unsaved = self.unsaved();
        let minutes = self.project_file.recovery_minutes;
        let recovery = &mut self.project_file.recovery;
        if let Some(writing) = &recovery.writing {
            match writing.try_recv() {
                Ok(Ok(())) | Err(mpsc::TryRecvError::Disconnected) => recovery.writing = None,
                Ok(Err(error)) => {
                    recovery.writing = None;
                    log::warn!(target: "project", "recovery copy not written: {error}");
                    if !recovery.failure_reported {
                        recovery.failure_reported = true;
                        self.messages.push(
                            Message::new(Level::Warning, tr!("recovery-not-written")).detail(error),
                        );
                    }
                }
                Err(mpsc::TryRecvError::Empty) => return,
            }
        }
        let Some(root) = recovery.root.clone() else {
            return;
        };
        if !unsaved || minutes == 0 {
            return;
        }
        // The first copy waits a whole interval after the project became unsaved.
        let Some((at, written)) = &recovery.written else {
            recovery.written = Some((Instant::now(), Vec::new()));
            return;
        };
        if at.elapsed() < Duration::from_secs(u64::from(minutes) * 60)
            || self.workspace.is_state(written)
        {
            return;
        }
        let state = self.workspace.state();
        if recovery.session.is_none() {
            let folder = root.join(format!("{}-{}", std::process::id(), now_s()));
            let lock = std::fs::create_dir_all(&folder).and_then(|()| {
                let lock = File::create(folder.join(LOCK))?;
                lock.lock()?;
                Ok(lock)
            });
            match lock {
                Ok(lock) => recovery.session = Some((folder, lock)),
                Err(error) => {
                    log::warn!(target: "project", "no recovery folder: {error}");
                    recovery.root = None;
                    return;
                }
            }
        }
        let Some((folder, _)) = &recovery.session else {
            return;
        };
        let folder = folder.clone();
        let project = self.workspace.project().clone();
        let note = SessionNote {
            name: project.name.clone(),
            path: self.project_file.path.clone(),
            written_s: now_s(),
        };
        let (sender, result) = mpsc::channel();
        thread::spawn(move || {
            let written = file::save_folder(&project, &folder.join(PROJECT))
                .map_err(|e| e.to_string())
                .and_then(|()| {
                    let bytes = serde_json::to_vec(&note).map_err(|e| e.to_string())?;
                    std::fs::write(folder.join(NOTE), bytes).map_err(|e| e.to_string())
                });
            // The receiver is gone only if the window was dropped.
            let _ = sender.send(written);
        });
        recovery.writing = Some(result);
        recovery.written = Some((Instant::now(), state));
    }

    /// Empty this session's recovery copy: nothing is unsaved.
    fn clear_recovery(&mut self) {
        let recovery = &mut self.project_file.recovery;
        recovery.written = None;
        if let Some((folder, _)) = &recovery.session {
            let _ = std::fs::remove_dir_all(folder.join(PROJECT));
            let _ = std::fs::remove_file(folder.join(NOTE));
        }
    }

    /// The application closes normally: this session's recovery folder and sliced files go.
    pub fn end_session(&mut self) {
        if let Some((folder, lock)) = self.project_file.recovery.session.take() {
            drop(lock);
            let _ = std::fs::remove_dir_all(folder);
        }
        crate::slicing_ui::end_session();
    }

    /// At start, the recovery copies other sessions left: restore one, or discard them.
    fn recovery_question(&mut self, ctx: &egui::Context) {
        let recovery = &mut self.project_file.recovery;
        if !recovery.searched {
            recovery.searched = true;
            if let Some(root) = &recovery.root {
                recovery.orphans = orphans(root);
            }
        }
        if recovery.orphans.is_empty() || self.project_file.asking.is_some() {
            return;
        }
        let orphans = recovery.orphans.clone();
        let mut chosen = None;
        let mut discard = false;
        let mut later = false;
        let response = dialog::show(
            ctx,
            "recovery",
            &tr!("recovery-title"),
            Size::Fixed(460.0),
            |ui, part| match part {
                Part::Body => {
                    ui.add(egui::Label::new(tr!("recovery-text")).wrap());
                    ui.add_space(6.0);
                    for orphan in &orphans {
                        ui.horizontal(|ui| {
                            let name = if orphan.note.name.is_empty() {
                                tr!("project-untitled")
                            } else {
                                orphan.note.name.clone()
                            };
                            ui.label(egui::RichText::new(name).strong());
                            ui.label(
                                egui::RichText::new(ago(orphan.note.written_s))
                                    .color(ui.visuals().weak_text_color()),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui.button(tr!("recovery-restore")).clicked() {
                                        chosen = Some(orphan.folder.clone());
                                    }
                                },
                            );
                        });
                    }
                }
                Part::Footer { .. } => {
                    if ui.button(tr!("recovery-discard")).clicked() {
                        discard = true;
                    }
                    if ui.button(tr!("recovery-later")).clicked() {
                        later = true;
                    }
                }
            },
        );
        if let Some(folder) = chosen {
            self.ask(Intent::Restore(folder));
        } else if discard {
            for orphan in std::mem::take(&mut self.project_file.recovery.orphans) {
                let _ = std::fs::remove_dir_all(orphan.folder);
            }
        } else if later || response.cancel {
            // Kept for the next start.
            self.project_file.recovery.orphans.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_saved_path_takes_the_project_extension() {
        assert_eq!(
            with_extension(PathBuf::from("/a/b/plate")),
            PathBuf::from("/a/b/plate.karasu")
        );
        assert_eq!(
            with_extension(PathBuf::from("plate.KARASU")),
            PathBuf::from("plate.KARASU")
        );
        assert_eq!(
            with_extension(PathBuf::from("plate.stl")),
            PathBuf::from("plate.stl.karasu")
        );
        assert!(is_project(Path::new("x.Karasu")));
        assert!(!is_project(Path::new("x.stl")));
    }

    #[test]
    fn recent_files_keep_the_newest_first_without_repeats() {
        let mut file = ProjectFile::default();
        for k in 0..12 {
            file.remember(Path::new(&format!("p{k}.karasu")));
        }
        file.remember(Path::new("p5.karasu"));
        assert_eq!(file.recent().len(), RECENT_PROJECTS);
        assert_eq!(file.recent()[0], PathBuf::from("p5.karasu"));
        assert_eq!(
            file.recent()
                .iter()
                .filter(|p| p.as_path() == Path::new("p5.karasu"))
                .count(),
            1
        );
    }

    /// Run `window`'s project frame until `done` holds, or fail; `events` arrive on the second
    /// frame, once a dialog the first one opened is the topmost.
    fn until(
        window: &mut MainWindow,
        ctx: &egui::Context,
        events: Vec<egui::Event>,
        done: impl Fn(&MainWindow) -> bool,
    ) {
        let mut events = Some(events);
        for frame in 0..2000 {
            let input = egui::RawInput {
                events: if frame == 1 {
                    events.take().unwrap_or_default()
                } else {
                    Vec::new()
                },
                ..egui::RawInput::default()
            };
            let mut output = ctx.run_ui(input, |ui| window.project_frame(ui.ctx()));
            output.textures_delta.clear();
            // As the window's frame does: imports, previews and supports go on.
            window.workspace.poll();
            window.preparation.poll(&window.workspace);
            window
                .supports
                .poll(&window.workspace, &window.preparation, None);
            if done(window) {
                return;
            }
            thread::sleep(Duration::from_millis(5));
        }
        panic!("the project frame never got there");
    }

    #[test]
    fn another_project_keeps_nothing_of_the_last_one_s_plates() {
        // Plate identities and revisions start over in a new project: a slice kept for the
        // last project's plate 1 must not be taken for the new project's, and its file goes.
        let ctx = egui::Context::default();
        let folder =
            std::env::temp_dir().join(format!("karasu-window-forget-{}", std::process::id()));
        let mut window = MainWindow::new(Vec::new());
        window.workspace = crate::slicing_ui::tests::hollowed_cube(&folder);
        window.slicing = crate::slicing_ui::tests::slicing_into(&folder);
        window.follow_plate(&ctx);
        let first = window.workspace.plate_id();
        window.slicing.start(&window.workspace);
        crate::slicing_ui::tests::finish(&mut window.slicing);
        let file = crate::slicing_ui::tests::output(&window.slicing).unwrap();
        window.perform(crate::actions::Action::NewPlate);
        window.follow_plate(&ctx);
        assert!(window.plates.slicings.contains_key(&first));
        assert!(file.exists());
        window.replace_workspace(&ctx, Workspace::new(), true);
        window.follow_plate(&ctx);
        assert!(window.plates.slicings.is_empty());
        assert!(!file.exists());
        assert_eq!(window.workspace.plate_id(), first);
        assert!(window.current_facts(first).is_none());
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_saved_project_asks_before_its_changes_are_lost_and_opens_again() {
        let folder =
            std::env::temp_dir().join(format!("karasu-window-project-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let model = folder.join("cube.stl");
        std::fs::write(&model, crate::workspace::tests::box_stl([0.0; 3], [8.0; 3])).unwrap();
        let mut window = MainWindow::new(Vec::new());
        let ctx = egui::Context::default();
        // An untitled project without a model holds nothing to lose.
        assert!(!window.unsaved());
        window.workspace.open(model);
        crate::workspace::tests::wait(&mut window.workspace);
        assert!(window.unsaved());
        // Saved: named after its file, remembered, nothing unsaved.
        let path = folder.join("cube.karasu");
        window.start_save(path.clone());
        until(&mut window, &ctx, Vec::new(), |w| !w.project_file.busy());
        assert!(!window.unsaved());
        assert_eq!(window.workspace.project().name, "cube");
        assert_eq!(window.project_file.recent(), std::slice::from_ref(&path));
        // An edit is unsaved; New asks, and Escape keeps the project as it is.
        window
            .workspace
            .edit(|p| p.objects[0].translate_mm = [1.0, 2.0, 0.0]);
        assert!(window.unsaved());
        window.ask(Intent::New);
        assert_eq!(window.project_file.asking, Some(Intent::New));
        let key = |key: egui::Key| egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        until(&mut window, &ctx, vec![key(egui::Key::Escape)], |w| {
            w.project_file.asking.is_none()
        });
        assert_eq!(window.workspace.plate().objects.len(), 1);
        // Opening it again asks; Enter saves first, then opens what was saved.
        window.ask(Intent::Open(Some(path.clone())));
        until(&mut window, &ctx, Vec::new(), |w| {
            w.project_file.asking.is_some()
        });
        until(&mut window, &ctx, vec![key(egui::Key::Enter)], |w| {
            w.project_file.asking.is_none()
                && !w.project_file.busy()
                && w.project_file.queued.is_none()
                && w.project_file.loading()
        });
        // Shown all at once, once its model is imported and prepared: the scene draws nothing
        // and nothing is said until then.
        let opened = tr!("project-opened", name = "cube.karasu");
        let said = |w: &MainWindow| w.messages.latest().is_some_and(|s| s.text == opened);
        assert!(!said(&window));
        until(&mut window, &ctx, Vec::new(), |w| !w.project_file.loading());
        assert!(said(&window));
        assert!(!window.workspace.meshes().is_empty());
        assert!(!window.unsaved());
        let object = &window.workspace.plate().objects[0];
        assert_eq!(object.translate_mm, [1.0, 2.0, 0.0]);
        assert_eq!(window.workspace.revision(), 1);
        // Placed and drawn once its mesh arrived, though the plate did not change since.
        assert!(window.workspace.placement_of(0).is_some());
        assert_eq!(window.workspace.draws(|_| [1.0; 4], |_| [0, 0]).len(), 1);
        // New, with nothing unsaved, does not ask: an empty untitled project.
        window.ask(Intent::New);
        until(&mut window, &ctx, Vec::new(), |w| {
            w.project_file.queued.is_none()
        });
        assert_eq!(window.workspace.plate().objects.len(), 0);
        assert!(!window.unsaved());
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_recovery_copy_outlives_a_crash_and_restores_the_unsaved_project() {
        let folder =
            std::env::temp_dir().join(format!("karasu-window-recovery-{}", std::process::id()));
        let root = folder.join("recovery");
        std::fs::create_dir_all(&folder).unwrap();
        let model = folder.join("cube.stl");
        std::fs::write(&model, crate::workspace::tests::box_stl([0.0; 3], [8.0; 3])).unwrap();
        let ctx = egui::Context::default();
        let mut window = MainWindow::new(Vec::new());
        window.keep_recovery_in(root.clone());
        window.workspace.open(model);
        crate::workspace::tests::wait(&mut window.workspace);
        // The first copy waits an interval after the project became unsaved: here, past it.
        until(&mut window, &ctx, Vec::new(), |w| {
            w.project_file.recovery.written.is_some()
        });
        window.project_file.recovery.written = Some((
            Instant::now()
                .checked_sub(Duration::from_secs(3600))
                .unwrap(),
            Vec::new(),
        ));
        until(&mut window, &ctx, Vec::new(), |w| {
            w.project_file.recovery.writing.is_none() && w.project_file.recovery.session.is_some()
        });
        let (session, lock) = window.project_file.recovery.session.take().unwrap();
        assert!(session.join(PROJECT).join(file::DOCUMENT_ENTRY).is_file());
        // A crash: the lock goes with the process, the folder stays.
        drop(lock);
        drop(window);
        let mut next = MainWindow::new(Vec::new());
        next.keep_recovery_in(root.clone());
        until(&mut next, &ctx, Vec::new(), |w| {
            w.project_file.recovery.searched
        });
        assert_eq!(next.project_file.recovery.orphans.len(), 1);
        next.ask(Intent::Restore(session.clone()));
        until(&mut next, &ctx, Vec::new(), |w| {
            !w.project_file.busy()
                && w.project_file.queued.is_none()
                && !w.workspace.plate().objects.is_empty()
        });
        // Restored as unsaved, its copy gone; this session keeps its own from now on.
        assert!(next.unsaved());
        assert!(!session.exists());
        next.end_session();
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn only_copies_no_session_holds_are_offered() {
        let root = std::env::temp_dir().join(format!("karasu-recovery-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let write = |name: &str, written_s: u64| {
            let folder = root.join(name);
            std::fs::create_dir_all(folder.join(PROJECT)).unwrap();
            let note = SessionNote {
                name: name.to_owned(),
                path: None,
                written_s,
            };
            std::fs::write(folder.join(NOTE), serde_json::to_vec(&note).unwrap()).unwrap();
            folder
        };
        write("older", 100);
        write("newer", 200);
        // A running session holds its lock: its copy is not offered.
        let held = write("held", 300);
        let lock = File::create(held.join(LOCK)).unwrap();
        lock.lock().unwrap();
        // A folder with nothing to restore is removed.
        std::fs::create_dir_all(root.join("empty")).unwrap();
        let found: Vec<String> = orphans(&root).into_iter().map(|o| o.note.name).collect();
        assert_eq!(found, ["newer", "older"]);
        assert!(!root.join("empty").exists());
        drop(lock);
        let _ = std::fs::remove_dir_all(&root);
    }
}
