//! The main window (M6.2): the workflow bar, the 3D viewport and the status bar.
//!
//! **Viewport controls:**
//! - drag with the primary button to orbit, with the secondary or middle button to pan, and
//!   scroll to zoom; "Fit view" frames the build volume and every object;
//! - click an object to select it, and drag its handles to move it (along X, Y or Z, or freely
//!   with the yellow square), turn it or scale it ([`crate::manipulate`]); only handles move
//!   objects. M, R and S choose the handles, Delete removes the object, Escape clears the
//!   selection;
//! - an object moved up off the plate stays there (slicing it will need supports, and the status
//!   bar says so) until "Drop to plate"; it never goes below the plate.
//!
//! **Messages:** the status bar shows the latest message in one line; the Messages window lists
//! them all with their details and fixes ([`crate::messages`]). The Repair menu sets the plate's
//! repairs ([`crate::repair_ui`]).
//!
//! **Colours:** the viewport's colours come from [`Appearance`], meant to be set by the user;
//! the interface chrome uses the brand palette ([`theme`]).
//!
//! **Files:** models and printer or resin profiles are dropped on the window (or named on the
//! command line); see [`crate::workspace`].
mod about;
mod models_export;
mod persist;
mod plates;
mod project_file;
mod settings_dialog;
mod tool_values;

use crate::{
    actions::{Action, Keymap},
    appearance::Appearance,
    camera::{Camera, Mat4, Ray},
    gpu,
    hollowing_ui::{HollowingUi, Part},
    i18n::tr,
    icons::{Icon, Icons},
    manipulate::{self, Drag, Mode, Space},
    messages::{Messages, Status},
    preparation_ui::PreparationUi,
    profiles_ui::ProfilesUi,
    section_ui::{self, SectionUi},
    sidebar,
    slicing_ui::SlicingUi,
    supports_ui::SupportsUi,
    theme,
    toasts::{self, Severity},
    transform_ui::{self, TransformUi},
    viewcube,
    viewport::{Draw, Outline, OverhangStyle, Viewport},
    visibility::Visibility,
    widgets,
    workflow::{Tool, Workflow},
    workspace::{plate_surface, volume_lines, FrontTab, Workspace},
};
use eframe::{egui, egui_wgpu, wgpu};
use karasu_core::profiles::PrinterProfile;
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

/// Frame timing of a measured run.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Timing {
    intervals: Vec<Duration>,
    last: Option<Instant>,
}

impl Timing {
    /// Record a frame.
    pub fn frame(&mut self) {
        let now = Instant::now();
        if let Some(last) = self.last.replace(now) {
            self.intervals.push(now - last);
        }
    }

    /// Mean and longest interval between frames, in milliseconds.
    #[must_use]
    pub fn summary(&self) -> (f64, f64) {
        let ms = |d: &Duration| d.as_secs_f64() * 1000.0;
        if self.intervals.is_empty() {
            return (0.0, 0.0);
        }
        #[allow(clippy::cast_precision_loss)] // A few hundred frames.
        let mean = self.intervals.iter().map(ms).sum::<f64>() / self.intervals.len() as f64;
        (mean, self.intervals.iter().map(ms).fold(0.0, f64::max))
    }
}

/// The main window's state.
#[allow(clippy::struct_excessive_bools)] // Independent states: dialogs open, answers awaited.
pub struct MainWindow {
    workspace: Workspace,
    camera: Camera,
    viewport: Option<Viewport>,
    /// Whether the volume lines were built, and for which printer.
    lines_built: bool,
    lines_printer: Option<PrinterProfile>,
    /// The colours and the language the plate's lines and front word were built with: they are
    /// built again when either changes (the settings), and only then.
    lines_look: Option<(Appearance, String)>,
    /// Fit the view once the next imports have arrived.
    fit_pending: bool,
    /// Viewport size in points, for fitting.
    aspect: f64,
    /// The selected object.
    selected: Option<usize>,
    /// The model the Delete key asked to remove, waiting for its confirmation.
    deleting: Option<usize>,
    /// The model right-clicked in the viewport, whose menu is open (owner, 2026-11-10).
    model_menu: Option<usize>,
    /// The models checked in the sidebar, by their places on the plate shown (owner,
    /// 2026-11-10).
    checked: std::collections::BTreeSet<usize>,
    /// The checked models' deletion waits for the user's answer.
    deleting_checked: bool,
    /// Help › About karasu-slicer is open (TD-145).
    about_open: bool,
    /// The tool whose "Delete all" was pressed, and its model, waiting for the confirmation.
    clearing: Option<(Tool, usize)>,
    /// The settings file, when the window keeps its settings (not in a smoke test).
    persistence: Option<persist::Persistence>,
    /// How the main window opens, followed as the user changes it.
    window_settings: crate::settings::WindowSettings,
    /// Which handles the selection shows.
    mode: Mode,
    /// The axes the handles move and turn along (M6.5); a user setting.
    space: Space,
    /// The Transform tool's locks and uniform scaling (M6.5); user settings.
    transform: TransformUi,
    /// The handle being dragged.
    drag: Option<Drag>,
    /// Viewport colours.
    appearance: Appearance,
    /// The actions' keyboard shortcuts (M6.5).
    keymap: Keymap,
    /// The workflow bar's step and tools (TD-098).
    workflow: Workflow,
    /// The interface's icons, uploaded on the first frame.
    icons: Option<Icons>,
    /// The settings dialog, while it is open (TD-097).
    settings_dialog: Option<settings_dialog::SettingsDialog>,
    /// The live log window (TD-096).
    log_window: crate::log_window::LogWindow,
    /// Whether the models' overhangs are shown, and the critical angle (TD-107).
    overhangs: Overhangs,
    /// The interface's language as the user chose it (`None`: the reference).
    language: Option<String>,
    /// How the application was started: `--dev`, and the language in use.
    launch: Launch,
    /// The sidebar's and the tool panel's state, and a pending quit.
    chrome: Chrome,
    /// Printer and resin selection and editing.
    profiles: ProfilesUi,
    /// Slicing, export and the layer view.
    slicing: SlicingUi,
    /// Every message of the session.
    messages: Messages,
    /// Live preparation previews.
    preparation: PreparationUi,
    /// The selected object's preparation panel.
    hollowing: HollowingUi,
    /// The Supports tool (TD-108).
    supports: SupportsUi,
    /// The Raft tool: the selected model's lift and raft (TD-108).
    raft: crate::raft_ui::RaftUi,
    /// The Islands tool: what the selected model's print starts in mid-air.
    islands: crate::islands_ui::IslandsUi,
    /// The Automatic tool: every support of the selected model planned at once (M6.4i).
    auto: crate::auto_ui::AutoUi,
    /// The Supports tool's Grid mode (TD-138).
    grid: crate::grid_ui::GridUi,
    /// The Orient tool (M6.4k, TD-119).
    orient: crate::orient_ui::OrientUi,
    /// The values the tools' new operations start from, set in the Settings dialog (owner,
    /// 2026-10-27).
    tool_defaults: crate::settings::ToolDefaults,
    /// The tool whose panel was open on the last frame: a tool opened anew starts from the
    /// defaults.
    opened_tool: Option<Tool>,
    /// The section panel at the viewport's right.
    section: SectionUi,
    /// The view before a cavity region was shown (TD-101), to go back to: the camera and the
    /// section's handles.
    region_view: Option<(Camera, [f64; 2])>,
    /// While the section follows the Layers panel (TD-103): the handles before, and the top it
    /// cut at last.
    followed: Option<([f64; 2], f64)>,
    /// Undo merge key of the current drag: one drag is one undo step.
    drag_key: u64,
    /// Workspace notices already copied into the messages.
    notices_taken: usize,
    /// The point a wheel gesture zooms at, where the pointer was, and when it last zoomed
    /// (M6.4f): picked once per gesture, not per notch.
    zoom_anchor: Option<([f64; 2], [f64; 3], std::time::Instant)>,
    /// A turn of the camera to a view the view cube or the home button asked for.
    turn: Option<viewcube::Turn>,
    /// The project file: where it is saved, unsaved changes, the recovery copy (TD-130).
    project_file: project_file::ProjectFile,
    /// The project's plates in the window: the plates bar and what showing another plate
    /// changes (TD-135).
    plates: plates::Plates,
    /// Export models… (TD-139).
    models_export: models_export::ModelsExport,
    /// The renderer's device and queue, for the plates' thumbnails drawn outside the viewport.
    render_state: Option<eframe::egui_wgpu::RenderState>,
}

/// How long a wheel gesture keeps its zoom anchor between notches, and how far the pointer may
/// move meanwhile, in points.
const ZOOM_GESTURE_SECONDS: f64 = 0.4;
/// `statuses` on one line, separated by dots, cut with an ellipsis at the space left; a spinner
/// first while work is in progress; the whole text and the details when the pointer rests on it.
fn status_line(ui: &mut egui::Ui, statuses: &[Status]) -> egui::Response {
    if statuses.iter().any(|s| s.busy) {
        ui.spinner();
    }
    let mut job = egui::text::LayoutJob::default();
    let font = egui::TextStyle::Body.resolve(ui.style());
    for (k, status) in statuses.iter().enumerate() {
        if k > 0 {
            let format = egui::TextFormat::simple(font.clone(), theme::BORDER.color());
            job.append("  ·  ", 0.0, format);
        }
        let format = egui::TextFormat::simple(font.clone(), status.colour);
        job.append(&status.text, 0.0, format);
    }
    let hover: Vec<String> = statuses
        .iter()
        .map(|s| match &s.detail {
            Some(detail) => format!("{}\n{detail}", s.text),
            None => s.text.clone(),
        })
        .collect();
    ui.add(egui::Label::new(job).truncate())
        .on_hover_text(hover.join("\n\n"))
}

/// A tool panel's body, scrolling, and its `footer` if any, fixed under it, filling the panel
/// down to `bottom`. `content` draws the body, or the footer when told so. The body takes the
/// height left less the footer's, measured on the frame before (kept under `id`): egui's
/// bottom-up layout does not know an area's fixed height.
fn body_and_footer(
    ui: &mut egui::Ui,
    bottom: f32,
    id: egui::Id,
    footer: bool,
    mut content: impl FnMut(&mut egui::Ui, bool),
) {
    let footer_height: f32 = if footer {
        ui.ctx()
            .data(|d| d.get_temp(id))
            .unwrap_or(PANEL_FOOTER_GUESS)
    } else {
        0.0
    };
    let scrolled = (bottom - ui.cursor().top() - footer_height).max(40.0);
    // The body has a fixed rectangle: content wider than the panel is cut, never widening it
    // (the panel's design rule, TD-098; panel rows wrap so nothing should be).
    let body =
        egui::Rect::from_min_size(ui.cursor().min, egui::vec2(ui.available_width(), scrolled));
    let mut inner = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(body)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    inner.set_clip_rect(body.intersect(ui.clip_rect()));
    crate::focus::scrolled(
        egui::ScrollArea::vertical()
            .id_salt(id)
            .auto_shrink([false, false])
            .max_height(scrolled)
            .min_scrolled_height(scrolled),
        &mut inner,
        |ui| content(ui, false),
    );
    ui.allocate_rect(body, egui::Sense::hover());
    if footer {
        let start = ui.cursor().top();
        ui.separator();
        content(ui, true);
        let used = ui.cursor().top() - start;
        if (used - footer_height).abs() > 0.5 {
            ui.ctx().data_mut(|d| d.insert_temp(id, used));
            ui.ctx().request_repaint();
        }
    }
}

/// The inner height of an open tool panel whose top is at `top`: down to the viewport's bottom,
/// less a margin and the panel's frame.
fn panel_height(viewport: egui::Rect, top: f32) -> f32 {
    (viewport.bottom() - top - PANEL_MARGIN - PANEL_FRAME).max(80.0)
}

/// The models' overhangs (TD-107): shown or not, and the critical angle in degrees from the
/// plate's plane; user settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Overhangs {
    pub shown: bool,
    pub angle_deg: f64,
}

impl Default for Overhangs {
    fn default() -> Self {
        Self {
            // Shown by default (owner, 2026-10-26).
            shown: true,
            angle_deg: crate::settings::DEFAULT_OVERHANG_ANGLE_DEG,
        }
    }
}

/// How the application was started.
#[derive(Clone, Debug)]
pub struct Launch {
    /// `--dev`: developer mode, which lists the pseudo-locale among the languages.
    pub dev: bool,
    /// `--locale` chose the language for this start: the setting does not change it.
    pub locale_fixed: bool,
    /// The language in use.
    pub locale: String,
    /// `--locale-dir`: catalogs added to the bundled ones.
    pub locale_dir: Option<std::path::PathBuf>,
    /// The session log, for the live log window (none: not installed).
    pub log: Option<karasu_project::logging::LogHandle>,
}

impl Default for Launch {
    fn default() -> Self {
        Self {
            dev: false,
            locale_fixed: false,
            locale: crate::i18n::REFERENCE.to_owned(),
            locale_dir: None,
            log: None,
        }
    }
}

/// The state of the window's chrome around the viewport (TD-098).
#[allow(clippy::struct_excessive_bools)] // Each a part of the chrome, open or not.
struct Chrome {
    /// The sidebar is open (else a narrow strip).
    sidebar_open: bool,
    /// The tool panel is expanded (else its header alone).
    panel_open: bool,
    /// The plates bar shows its thumbnails (else a row of tabs, TD-135).
    plates_bar_open: bool,
    /// Quit was asked for: the window closes at the frame's end.
    quit: bool,
}

impl Default for Chrome {
    fn default() -> Self {
        Self {
            sidebar_open: true,
            panel_open: true,
            plates_bar_open: true,
            quit: false,
        }
    }
}

/// The sidebar's width, open and closed, and the tool panel's width, in points (TD-098).
const SIDEBAR_WIDTH: f32 = 280.0;
/// The sidebar's plate section, by its collapsing state's identity.
const PLATE_SECTION: &str = "sidebar-plate-section";
/// The sidebar's models section, by its collapsing state's identity.
const MODELS_SECTION: &str = "sidebar-models-section";

/// Whether the sidebar's models section is open.
fn models_open(ctx: &egui::Context) -> bool {
    egui::collapsing_header::CollapsingState::load_with_default_open(
        ctx,
        egui::Id::new(MODELS_SECTION),
        true,
    )
    .is_open()
}
/// The closed sidebar's strip: its button (a control's height square) with the same 8-point
/// margin on every side (owner, 2026-11-10: the button was cut against the edge).
const SIDEBAR_CLOSED: f32 = 26.0 + 2.0 * SIDEBAR_CLOSED_MARGIN;
const SIDEBAR_CLOSED_MARGIN: f32 = 8.0;
const PANEL_WIDTH: f32 = 330.0;
/// The gap under an open tool panel, and its frame's margins and stroke, in points.
const PANEL_MARGIN: f32 = 12.0;
const PANEL_FRAME: f32 = 14.0;
/// A panel footer's height before it is first measured, in points.
const PANEL_FOOTER_GUESS: f32 = 72.0;
/// The period of the surface filters' stripes, in points.
const FILTER_STRIPE_POINTS: f32 = 12.0;
/// The selection outline's width beyond the silhouette, in points.
const SELECTION_OUTLINE_POINTS: f32 = 3.0;
const ZOOM_GESTURE_POINTS: f64 = 3.0;

/// Size of the handles on screen, in points.
const HANDLE_POINTS: f64 = 90.0;
/// How far from a handle a press still grabs it, in points.
const HANDLE_REACH: f64 = 8.0;
/// How long the plate's surface and grid take to fade out or in, in seconds.
const PLATE_FADE_SECONDS: f32 = 0.25;
/// How much the plate shows, from 1 to 0 over [`PLATE_FADE_SECONDS`] while the view at `eye`
/// goes below it (TD-088), back to 1 as it comes up: the plate's surface and grid fade by it,
/// and the supports' plate bases and rafts with them (owner R9, 2026-11-10). The same in every
/// call of a frame.
fn plate_fade(ctx: &egui::Context, eye: [f64; 3]) -> f32 {
    ctx.animate_value_with_time(
        egui::Id::new("plate-fade"),
        if eye[2] >= 0.0 { 1.0 } else { 0.0 },
        PLATE_FADE_SECONDS,
    )
}

/// Size at which the plate's front word is laid out, in points: large enough to stay sharp
/// close up.
const FRONT_LABEL_POINTS: f32 = 64.0;

impl MainWindow {
    /// An empty workspace, opening `files`: models and profiles into it, or else the first
    /// project file (TD-130).
    #[must_use]
    pub fn new(files: Vec<PathBuf>) -> Self {
        let (projects, files): (Vec<_>, Vec<_>) =
            files.into_iter().partition(|f| project_file::is_project(f));
        let mut workspace = Workspace::new();
        let fit_pending = !files.is_empty();
        for file in files {
            workspace.open(file);
        }
        let mut window = Self {
            workspace,
            camera: Camera::new([0.0; 3], 300.0),
            viewport: None,
            lines_built: false,
            lines_printer: None,
            lines_look: None,
            fit_pending,
            aspect: 1.6,
            selected: None,
            deleting: None,
            model_menu: None,
            checked: std::collections::BTreeSet::new(),
            deleting_checked: false,
            about_open: false,
            clearing: None,
            persistence: None,
            window_settings: crate::settings::WindowSettings::default(),
            mode: Mode::Move,
            space: Space::Global,
            transform: TransformUi::default(),
            drag: None,
            appearance: Appearance::default(),
            settings_dialog: None,
            log_window: crate::log_window::LogWindow::new(None),
            overhangs: Overhangs::default(),
            language: None,
            launch: Launch::default(),
            keymap: Keymap::default(),
            workflow: Workflow::default(),
            icons: None,
            chrome: Chrome::default(),
            profiles: ProfilesUi::new(),
            slicing: SlicingUi::new(),
            messages: Messages::default(),
            preparation: PreparationUi::default(),
            hollowing: HollowingUi::default(),
            supports: SupportsUi::new(),
            raft: crate::raft_ui::RaftUi::default(),
            islands: crate::islands_ui::IslandsUi::new(),
            auto: crate::auto_ui::AutoUi::new(),
            grid: crate::grid_ui::GridUi::new(),
            orient: crate::orient_ui::OrientUi::new(),
            tool_defaults: crate::settings::ToolDefaults::default(),
            opened_tool: None,
            section: SectionUi::default(),
            region_view: None,
            followed: None,
            drag_key: 0,
            notices_taken: 0,
            zoom_anchor: None,
            turn: None,
            project_file: project_file::ProjectFile::default(),
            plates: plates::Plates::default(),
            models_export: models_export::ModelsExport::default(),
            render_state: None,
        };
        if let Some(project) = projects.into_iter().next() {
            window.open_at_start(project);
        }
        window
    }

    /// Keep the recovery copies of unsaved projects in `root` (the application's data), and
    /// offer those other sessions left (TD-130).
    pub fn keep_recovery_in(&mut self, root: PathBuf) {
        self.project_file.keep_recovery_in(root);
    }

    /// Close without asking about unsaved changes, as a smoke test does.
    pub fn close_without_asking(&mut self) {
        self.project_file.ask = false;
    }

    /// How the application was started (before [`Self::load_settings`]).
    pub fn set_launch(&mut self, launch: Launch) {
        self.log_window = crate::log_window::LogWindow::new(launch.log.clone());
        self.launch = launch;
    }

    /// The workspace.
    #[must_use]
    pub fn workspace(&self) -> &Workspace {
        &self.workspace
    }

    /// Whether models are still being imported.
    #[must_use]
    pub fn busy(&self) -> bool {
        self.workspace.importing().next().is_some()
    }

    /// Turn the camera a little about the vertical, as a measured run does, with the first
    /// object selected, as when a user turns around the object they are preparing: its
    /// selection outline is part of the measured frames.
    pub fn spin(&mut self) {
        if self.selected.is_none() && !self.workspace.plate().objects.is_empty() {
            self.selected = Some(0);
        }
        self.camera.orbit(12.0, 0.0);
    }

    /// Samples per pixel of the viewport, once created.
    #[must_use]
    pub fn samples(&self) -> Option<u32> {
        self.viewport.as_ref().map(Viewport::samples)
    }

    fn fit(&mut self) {
        if let Some((min, max)) = self.workspace.bounds() {
            self.camera.fit(min, max, self.aspect);
        }
    }

    /// Zooms with the wheel at the surface under the pointer (M6.4f).
    fn wheel_zoom(&mut self, ui: &egui::Ui, pointer: Option<[f64; 2]>, size: [f64; 2]) {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll == 0.0 {
            return;
        }
        self.turn = None;
        let steps = f64::from(scroll) / 50.0;
        match pointer {
            Some(p) => {
                let anchor = self.zoom_anchor(p, size);
                self.camera.zoom_at(steps, anchor);
            }
            None => self.camera.zoom(steps),
        }
    }

    /// The selection outline of draw `draw`, the selected object's, for a `size` pixel image
    /// at `pixels` per point, kept to the object's bounds on screen.
    fn outline(
        &mut self,
        draw: usize,
        view_projection: &Mat4,
        size: [u32; 2],
        pixels: f32,
    ) -> Outline {
        let width = SELECTION_OUTLINE_POINTS * pixels;
        let bounds = self
            .selected
            .and_then(|index| self.workspace.placements().get(index).cloned())
            .and_then(Result::ok);
        Outline {
            draws: vec![draw],
            color: self.appearance.selection_outline.linear(),
            width,
            scissor: bounds.and_then(|p| {
                Outline::scissor_of(
                    view_projection,
                    [p.min.x, p.min.y, p.min.z],
                    [p.max.x, p.max.y, p.max.z],
                    size,
                    f64::from(width) + 1.0,
                )
            }),
        }
    }

    /// The viewport's settings for this frame: the section caps' colour, the plate's fade
    /// while the view is below it (TD-088), and the surface filters' build volume and stripes.
    fn configure_viewport(
        &mut self,
        ctx: &egui::Context,
        device: &wgpu::Device,
        eye: [f64; 3],
        pixels: f32,
    ) {
        #[allow(clippy::cast_possible_truncation)] // GPU precision for a display filter.
        let volume = self
            .workspace
            .build_volume()
            .map(|corners| corners.map(|c| c.map(|v| v as f32)));
        let Some(viewport) = self.viewport.as_mut() else {
            return;
        };
        viewport.set_cap_colors(
            device,
            [
                self.appearance.section_cap.linear(),
                self.appearance.support_section_cap.linear(),
            ],
        );
        viewport.set_plate_fade(plate_fade(ctx, eye));
        viewport.set_filters(volume, FILTER_STRIPE_POINTS * pixels);
        viewport.set_shading(self.appearance.shading());
        #[allow(clippy::cast_possible_truncation)] // Degrees in a small range.
        viewport.set_overhangs(self.overhangs.shown.then(|| OverhangStyle {
            angle_deg: self.overhangs.angle_deg as f32,
            limit: self.appearance.overhang_limit.linear(),
            level: self.appearance.overhang_level.linear(),
        }));
        // The contact's height line while a support is placed or its tip dragged (TD-137).
        let guide = (self.appearance.show_contact_line
            && self.workflow.tool() == Some(Tool::Supports))
        .then(|| self.supports.guide_height())
        .flatten()
        .map(|height_mm| crate::viewport::Guide {
            height_mm,
            colour: self.appearance.contact_line.linear(),
        });
        viewport.set_guide(guide);
    }

    /// Moves the camera along a turn the view cube started, and ends it once there.
    fn advance_turn(&mut self, ctx: &egui::Context) {
        if let Some(turn) = self.turn {
            let (camera, done) = turn.now();
            self.camera = camera;
            if done {
                self.turn = None;
            } else {
                ctx.request_repaint();
            }
        }
    }

    /// Shows the view cube and its home button, and starts the turn one of them asks for.
    fn view_cube(&mut self, ui: &mut egui::Ui, rect: egui::Rect) {
        let to = match viewcube::show(ui, rect, &self.camera) {
            Some(viewcube::Request::LookFrom(direction)) => {
                let mut to = self.camera;
                to.look_from(direction);
                to
            }
            Some(viewcube::Request::Home) => {
                self.perform(Action::HomeView);
                return;
            }
            None => return,
        };
        self.turn = Some(viewcube::Turn::new(self.camera, to));
    }

    /// Frame the region within `bounds` (world mm) and cut the models through its middle, so it
    /// shows from above: a cavity region (TD-101), a suction cup, or an island in its first layer.
    /// The view before the first region shown is kept for "Back to the whole model".
    fn show_region(&mut self, bounds: [[f64; 3]; 2]) {
        /// The least width framed, in mm: a pocket is seen in its surroundings.
        const CONTEXT_MM: f64 = 12.0;
        /// The least elevation of the eye: the cut is seen from above.
        const PITCH_DEG: f64 = 35.0;
        if self.region_view.is_none() {
            self.region_view = Some((self.camera, self.section.view()));
        }
        let [min, max] = bounds;
        let half = |k: usize| (max[k] - min[k]).max(CONTEXT_MM) / 2.0;
        let centre: [f64; 3] = std::array::from_fn(|k| f64::midpoint(min[k], max[k]));
        let mut to = self.camera;
        to.pitch = to.pitch.max(PITCH_DEG.to_radians());
        to.fit(
            std::array::from_fn(|k| centre[k] - half(k)),
            std::array::from_fn(|k| centre[k] + half(k)),
            self.aspect,
        );
        self.turn = Some(viewcube::Turn::new(self.camera, to));
        self.section.cut_above(&mut self.workspace, centre[2]);
    }

    /// Keep the supports' builds, the islands' search and their plans current; a repaint is
    /// asked for while any works.
    fn poll_supports(&mut self, ctx: &egui::Context) {
        // The selected model's supports are built first while the Supports, Islands or Braces
        // tool shows them; the Islands tool plans its supports on the model's surface for
        // routes, kept with them.
        let supporting = self.selected.filter(|_| {
            matches!(
                self.workflow.tool(),
                Some(Tool::Automatic | Tool::Supports | Tool::Islands | Tool::Braces)
            )
        });
        if self
            .supports
            .poll(&self.workspace, &self.preparation, supporting)
        {
            ctx.request_repaint_after(Duration::from_millis(50));
        }
        let islands = self
            .selected
            .filter(|_| self.workflow.tool() == Some(Tool::Islands));
        if self
            .islands
            .poll(&self.workspace, &self.preparation, &self.supports, islands)
        {
            ctx.request_repaint_after(Duration::from_millis(50));
        }
        self.islands.finish_here(&mut self.workspace, islands);
        if self.hollowing.cups.poll() {
            ctx.request_repaint_after(Duration::from_millis(50));
        }
        if self
            .auto
            .poll(&self.workspace, &self.preparation, &self.supports)
        {
            ctx.request_repaint_after(Duration::from_millis(50));
        }
        // The Grid mode plans on the model's surface for routes (asked for with the panel).
        if self.workflow.tool() == Some(Tool::Supports)
            && self
                .grid
                .poll(&self.workspace, &self.preparation, &self.supports)
        {
            ctx.request_repaint_after(Duration::from_millis(50));
        }
        // The Orient tool scores with the overhangs' critical angle.
        self.orient.angle_deg = self.overhangs.angle_deg;
        if self.orient.poll() {
            ctx.request_repaint_after(Duration::from_millis(50));
        }
        self.orient.confirm(ctx, &mut self.workspace);
    }

    /// The tool in use's list window, if it has one and it is open (owner, 2026-10-29); a
    /// double click on a row brings the view to its item.
    fn list_windows(&mut self, ctx: &egui::Context) {
        let appearance = self.appearance;
        let focus = match self.workflow.tool() {
            Some(Tool::Supports) => {
                self.supports
                    .list_window(ctx, &mut self.workspace, self.selected, &appearance)
            }
            Some(Tool::Braces) => {
                self.supports
                    .braces_window(ctx, &mut self.workspace, self.selected, &appearance)
            }
            Some(Tool::Hollow) => self.hollowing.blockers.list_window(
                ctx,
                &self.workspace,
                self.selected,
                &appearance,
            ),
            Some(Tool::DrainHoles) => self.hollowing.holes_window(
                ctx,
                &mut self.workspace,
                &mut self.preparation,
                self.selected,
                &appearance,
            ),
            Some(Tool::Islands) => self.islands.list_window(
                ctx,
                &self.workspace,
                &mut self.supports,
                self.selected,
                &appearance,
            ),
            Some(Tool::Automatic) => {
                self.auto
                    .list_window(ctx, &self.workspace, self.selected, &appearance)
            }
            _ => None,
        };
        // A place stepped to in the Automatic panel brings the view to it too.
        if let Some(bounds) = focus.or_else(|| self.auto.take_view()) {
            self.bring_into_view(bounds);
        }
        // An island stepped to, in the panel or the list, is framed with the section through its
        // first layer (owner, 2026-11-04), as a cavity region is shown.
        if let Some(bounds) = self.islands.take_view() {
            self.show_region(bounds);
        }
    }

    /// Turn the view to frame `bounds` (world mm), seen in its surroundings, the section as it
    /// is.
    fn bring_into_view(&mut self, bounds: [[f64; 3]; 2]) {
        /// The least width framed, in mm: an item is seen in its surroundings.
        const CONTEXT_MM: f64 = 12.0;
        let [min, max] = bounds;
        let half = |k: usize| (max[k] - min[k]).max(CONTEXT_MM) / 2.0;
        let centre: [f64; 3] = std::array::from_fn(|k| f64::midpoint(min[k], max[k]));
        let mut to = self.camera;
        to.fit(
            std::array::from_fn(|k| centre[k] - half(k)),
            std::array::from_fn(|k| centre[k] + half(k)),
            self.aspect,
        );
        self.turn = Some(viewcube::Turn::new(self.camera, to));
    }

    /// While a cavity region is shown, a button at the viewport's top left goes back to the view
    /// and the section before it.
    fn region_back(&mut self, ui: &mut egui::Ui, rect: egui::Rect) {
        let Some((camera, section)) = self.region_view else {
            return;
        };
        let back = egui::Area::new(egui::Id::new("region-back"))
            .fixed_pos(rect.min + egui::vec2(12.0, 12.0))
            .show(ui.ctx(), |ui| ui.button(tr!("region-back")))
            .inner;
        if back.clicked() {
            self.turn = Some(viewcube::Turn::new(self.camera, camera));
            self.section.set_view(section);
            self.region_view = None;
            self.hollowing.forget_region();
        }
    }

    /// With the Layers tool and Follow on, cut the models at the layer shown; the section's
    /// handles before are kept and put back once the tool or Follow is left.
    fn follow_layer(&mut self) {
        let following = self.workflow.tool() == Some(Tool::Layers) && self.chrome.panel_open;
        match (
            following.then(|| self.slicing.followed_top()).flatten(),
            self.followed,
        ) {
            (Some(top), followed) => {
                let saved = followed.map_or_else(|| self.section.view(), |(saved, _)| saved);
                #[allow(clippy::float_cmp)] // The same layer's top, unchanged.
                if followed.is_none_or(|(_, last)| last != top) {
                    self.section.cut_above(&mut self.workspace, top);
                }
                self.followed = Some((saved, top));
            }
            (None, Some((saved, _))) => {
                self.section.set_view(saved);
                self.followed = None;
            }
            (None, None) => {}
        }
    }

    /// The view the window opens with: the default direction, fitted to the plate.
    fn home_camera(&mut self) -> Camera {
        let mut camera = Camera::new([0.0; 3], 300.0);
        if let Some((min, max)) = self.workspace.bounds() {
            camera.fit(min, max, self.aspect);
        }
        camera
    }

    /// The point a wheel notch with the pointer at `pointer` zooms at: the surface under it, or
    /// the target's plane where there is none. Picked once per gesture.
    fn zoom_anchor(&mut self, pointer: [f64; 2], size: [f64; 2]) -> [f64; 3] {
        if let Some((at, anchor, when)) = &mut self.zoom_anchor {
            let moved = (at[0] - pointer[0]).hypot(at[1] - pointer[1]);
            if moved <= ZOOM_GESTURE_POINTS && when.elapsed().as_secs_f64() <= ZOOM_GESTURE_SECONDS
            {
                *when = std::time::Instant::now();
                return *anchor;
            }
        }
        let ray = self.camera.ray(pointer, size);
        let section = self.section.section(&mut self.workspace);
        let anchor = self
            .workspace
            .hit_in(&ray, section)
            .map_or_else(|| self.camera.focal_point(&ray), |hit| hit.point);
        self.zoom_anchor = Some((pointer, anchor, std::time::Instant::now()));
        anchor
    }

    /// Before an orbit or a pan: the camera's target brought, along the line of sight, to the
    /// depth of the surface seen at the first of `pixels` that meets one (see
    /// [`Camera::retarget`]). Over no surface the target stays.
    fn retarget<const N: usize>(&mut self, pixels: [[f64; 2]; N], size: [f64; 2]) {
        let section = self.section.section(&mut self.workspace);
        for pixel in pixels {
            let ray = self.camera.ray(pixel, size);
            if let Some(hit) = self.workspace.hit_in(&ray, section) {
                let depth = self.camera.depth_of(hit.point);
                if depth > 0.0 {
                    self.camera.retarget(depth);
                }
                return;
            }
        }
    }

    /// Show the window.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        frame: &eframe::Frame,
        adapter: Option<&gpu::Selected>,
    ) {
        let ctx = ui.ctx().clone();
        // The user's status colours, for every text and notification drawn this frame (TD-142).
        theme::set_status(self.appearance.status.map(|colour| colour.0));
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .collect()
        });
        if !dropped.is_empty() {
            self.open_files(dropped);
        }
        plates::keep_render_state(self, frame);
        self.follow_plate(&ctx);
        if self.workspace.poll() && self.fit_pending && !self.busy() {
            self.fit_pending = false;
            self.fit();
        }
        if self.busy() || self.workspace.diagnosing().next().is_some() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        if self.slicing.poll() {
            ctx.request_repaint();
        }
        self.advance_batch(&ctx);
        if self.preparation.poll(&self.workspace) {
            ctx.request_repaint_after(Duration::from_millis(50));
        }
        self.poll_supports(&ctx);
        self.collect_messages();
        self.menu_bar(ui);
        self.workflow_bar(ui);
        self.status_bar(ui, adapter);
        self.sidebar(ui);
        egui::Panel::right("section")
            .resizable(false)
            .exact_size(section_ui::WIDTH)
            .frame(
                egui::Frame::new()
                    .fill(theme::SURFACE.color())
                    .inner_margin(6.0),
            )
            .show(ui, |ui| self.section.panel(ui, &mut self.workspace));
        self.plates_bar(ui);
        let mut viewport_rect = egui::Rect::NOTHING;
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(theme::BACKGROUND.color()))
            .show(ui, |ui| {
                viewport_rect = ui.max_rect();
                self.viewport(ui, frame);
            });
        self.profiles
            .windows(&ctx, &mut self.workspace, self.icons.as_ref());
        self.confirm_deletions(&ctx);
        self.confirm_clearing(&ctx);
        self.rename_plate_dialog(&ctx);
        self.confirm_plate_deletion(&ctx);
        self.plates_overview(&ctx);
        self.plate_dialogs(&ctx);
        self.export_every_plate_dialog(&ctx);
        self.export_models_dialog(&ctx);
        self.settings_window(&ctx);
        // The strip at the left before the panel at the right: the Tab key follows the screen
        // (TD-142).
        self.tool_strip(&ctx, viewport_rect);
        self.tool_panel(&ctx, viewport_rect);
        // "Show" in the Repair section: look at the defect from where the camera is.
        if let Some(point) = self.hollowing.take_focus() {
            self.camera.target = point;
        }
        // A layer range's window, from the section bar's menu (TD-102).
        self.section.window(&ctx, &mut self.workspace);
        // A finding chosen in the Slice panel shows its layer in the Layers tool.
        if self.slicing.take_edit_setting() {
            self.profiles.edit_setting(&self.workspace);
        }
        if self.slicing.take_show_layers() {
            self.perform(Action::Tool(Tool::Layers));
        }
        // The Layers panel's larger window, and the section following the layer shown (TD-103).
        self.slicing.layer_window(&ctx, self.icons.as_ref());
        self.follow_layer();
        // "Show" in the Drainage section, or a cavity marker clicked.
        if let Some(bounds) = self.hollowing.take_region() {
            self.show_region(bounds);
        }
        self.list_windows(&ctx);
        plates::model_drag_hint(&ctx, &self.workspace);
        self.messages
            .toasts(&ctx, viewport_rect, self.icons.as_ref());
        // The settings: the window's size and place as the user leaves them, then saved a
        // moment after a change, or at once when the window closes.
        self.track_window(&ctx);
        self.project_frame(&ctx);
        let closing = self.chrome.quit || ctx.input(|i| i.viewport().close_requested());
        self.persist(closing);
        if std::mem::take(&mut self.chrome.quit) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if let Some(message) = self.log_window.window(&ctx, adapter) {
            self.messages.push(message);
        }
        if let Some(request) = self.messages.window(&ctx, &mut self.workspace) {
            request.fix.apply(&mut self.workspace);
            if request.reslice {
                self.slicing.start(&self.workspace);
            }
        }
        // Last, over every control drawn: the keyboard focus ring (TD-142).
        crate::focus::paint(&ctx);
    }

    /// Copy new workspace notices and slicing messages into the messages.
    fn collect_messages(&mut self) {
        let notices = self.workspace.notices();
        for notice in &notices[self.notices_taken.min(notices.len())..] {
            self.messages.push(notice.clone());
        }
        self.notices_taken = notices.len();
        for message in self.slicing.take_messages() {
            self.messages.push(message);
        }
        for message in self.take_plates_messages() {
            self.messages.push(message);
        }
        for message in self
            .hollowing
            .take_messages(&self.workspace, &self.preparation)
        {
            self.messages.push(message);
        }
        for message in self.section.take_messages() {
            self.messages.push(message);
        }
        for message in self.islands.take_messages() {
            self.messages.push(message);
        }
        for message in self.auto.take_messages() {
            self.messages.push(message);
        }
        for message in self.grid.take_messages() {
            self.messages.push(message);
        }
        for message in self.orient.take_messages() {
            self.messages.push(message);
        }
        for message in self.supports.take_messages() {
            self.messages.push(message);
        }
        for message in self.profiles.take_messages() {
            self.messages.push(message);
        }
    }

    /// The native dialog for opening models and profiles.
    fn open_dialog(&mut self) {
        let files = rfd::FileDialog::new()
            .set_title(tr!("dialog-open-title"))
            .add_filter(
                tr!("dialog-filter-all-openable"),
                &["karasu", "stl", "obj", "3mf", "json"],
            )
            .add_filter(tr!("dialog-filter-projects"), &["karasu"])
            .add_filter(tr!("dialog-filter-models"), &["stl", "obj", "3mf"])
            .add_filter(tr!("dialog-filter-profiles"), &["json"])
            .pick_files();
        self.open_files(files.unwrap_or_default());
    }

    /// The menu bar (TD-098): every entry is an action.
    fn menu_bar(&mut self, ui: &mut egui::Ui) {
        let mut chosen = None;
        let mut recent = None;
        egui::Panel::top("menu")
            .frame(
                egui::Frame::new()
                    .fill(theme::SURFACE.color())
                    .inner_margin(egui::Margin::symmetric(8, 4)),
            )
            .show(ui, |ui| {
                egui::MenuBar::new().ui(ui, |ui| {
                    let menus: [(String, &[Action]); 6] = [
                        (
                            tr!("menu-file"),
                            &[
                                Action::NewProject,
                                Action::Open,
                                Action::Save,
                                Action::SaveAs,
                                Action::ExportModels,
                                Action::OpenSettings,
                                Action::Quit,
                            ],
                        ),
                        (
                            tr!("menu-edit"),
                            &[
                                Action::Undo,
                                Action::Redo,
                                Action::Delete,
                                Action::RestoreDefaults,
                            ],
                        ),
                        (
                            tr!("menu-view"),
                            &[
                                Action::FitView,
                                Action::HomeView,
                                Action::ToggleTools,
                                Action::ToggleSidebar,
                                Action::Hide,
                                Action::Isolate,
                                Action::ShowAll,
                                Action::ToggleOverhangs,
                            ],
                        ),
                        (
                            tr!("menu-plate"),
                            &[
                                Action::NewPlate,
                                Action::DuplicatePlate,
                                Action::RenamePlate,
                                Action::DeletePlate,
                                Action::PreviousPlate,
                                Action::NextPlate,
                                Action::TogglePlatesBar,
                                Action::ArrangeModels,
                                Action::PlatesOverview,
                                Action::SliceEveryPlate,
                                Action::ExportEveryPlate,
                            ],
                        ),
                        (
                            tr!("menu-windows"),
                            &[
                                Action::ToggleMessages,
                                Action::ToggleLog,
                                Action::EditProfiles,
                            ],
                        ),
                        (tr!("menu-help"), &[Action::About]),
                    ];
                    for (name, actions) in menus {
                        ui.menu_button(name, |ui| {
                            for &action in actions {
                                if self.menu_item(ui, action) {
                                    chosen = Some(action);
                                }
                                // The recent projects, after Open (TD-130).
                                if action == Action::Open {
                                    self.recent_menu(ui, &mut recent);
                                }
                            }
                        });
                    }
                });
            });
        if let Some(action) = chosen {
            self.perform(action);
        }
        if let Some(path) = recent {
            self.open_recent(path);
        }
    }

    /// File › Open recent: the recent project files, the one clicked put in `chosen`.
    fn recent_menu(&self, ui: &mut egui::Ui, chosen: &mut Option<PathBuf>) {
        let recent = self.project_file.recent();
        ui.add_enabled_ui(!recent.is_empty() && !self.project_file.busy(), |ui| {
            ui.menu_button(tr!("menu-open-recent"), |ui| {
                for path in recent {
                    let name = path
                        .file_name()
                        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
                    if ui
                        .button(name)
                        .on_hover_text(path.display().to_string())
                        .clicked()
                    {
                        *chosen = Some(path.clone());
                    }
                }
            });
        });
    }

    /// A menu entry taking `action`, with its shortcut; returns whether it was clicked.
    fn menu_item(&self, ui: &mut egui::Ui, action: Action) -> bool {
        let mut button = egui::Button::new(action.label());
        if let Some(shortcut) = self.keymap.shortcuts(action).next() {
            button = button.shortcut_text(ui.ctx().format_shortcut(&shortcut));
        }
        let response = ui.add_enabled(self.enabled(action), button);
        let response = match action.hint() {
            Some(hint) => response.on_hover_text(hint),
            None => response,
        };
        response.clicked()
    }

    /// The workflow bar (TD-098): the steps, and the chosen step's tools.
    fn workflow_bar(&mut self, ui: &mut egui::Ui) {
        let hint = self.keymap.tooltip(ui.ctx(), Action::ToggleTools);
        let mut chosen = None;
        egui::Panel::top("workflow")
            .frame(
                // No border of its own: the panels' separator lines above and below it, as
                // around every other panel.
                egui::Frame::new()
                    .fill(theme::SURFACE.color())
                    .inner_margin(egui::Margin::symmetric(12, 8)),
            )
            .show(ui, |ui| {
                let icons = self.icons.get_or_insert_with(|| Icons::load(ui.ctx()));
                chosen = self.workflow.bar(ui, &hint, Some(icons));
            });
        if let Some(action) = chosen {
            self.perform(action);
        }
    }

    /// The sidebar (TD-098): the plate's printer and resin, and its objects. Closed, a narrow
    /// strip keeps the button that opens it.
    fn sidebar(&mut self, ui: &mut egui::Ui) {
        let frame = egui::Frame::new()
            .fill(theme::SURFACE.color())
            .inner_margin(10.0);
        if !self.chrome.sidebar_open {
            let mut open = false;
            egui::Panel::left("sidebar-closed")
                .resizable(false)
                .exact_size(SIDEBAR_CLOSED)
                .frame(frame.inner_margin(SIDEBAR_CLOSED_MARGIN))
                .show(ui, |ui| {
                    open = crate::widgets::icon_button(
                        ui,
                        self.icons.as_ref(),
                        crate::icons::Icon::ChevronRight,
                        false,
                        &tr!("sidebar-open"),
                    )
                    .clicked();
                });
            if open {
                self.perform(Action::ToggleSidebar);
            }
            return;
        }
        let mut request = None;
        egui::Panel::left("sidebar")
            .resizable(false)
            .exact_size(SIDEBAR_WIDTH)
            .frame(frame)
            .show(ui, |ui| {
                // Two sections, each collapsible: the plate's printer, resin and print setting,
                // then its models. The models' list scrolls on its own, so the plate's section
                // stays in view; it keeps a few rows, and the plate's section scrolls instead
                // when the window is too short for both.
                self.plate_section(ui, &mut request);
                ui.separator();
                egui::collapsing_header::CollapsingState::load_with_default_open(
                    ui.ctx(),
                    egui::Id::new(MODELS_SECTION),
                    true,
                )
                .show_header(ui, |ui| {
                    let title = sidebar::objects_title(&self.workspace);
                    widgets::name_id(ui.ctx(), egui::Id::new(MODELS_SECTION), &title);
                    ui.label(egui::RichText::new(title).strong());
                    if self.workspace.hidden_count() > 0 {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .link(tr!("action-show-all"))
                                .on_hover_text(tr!("action-visibility-hint"))
                                .clicked()
                            {
                                request = Some(sidebar::Request::ShowAll);
                            }
                        });
                    }
                })
                .body_unindented(|ui| {
                    // An undo or a change elsewhere may have taken checked models away.
                    let count = self.workspace.plate().objects.len();
                    self.checked.retain(|&k| k < count);
                    if let Some(r) = sidebar::checked_bar(
                        ui,
                        &self.workspace,
                        &self.checked,
                        self.icons.as_ref(),
                    ) {
                        request = Some(r);
                    }
                    crate::focus::scrolled(
                        egui::ScrollArea::vertical()
                            .id_salt("sidebar-models")
                            .auto_shrink([false, true]),
                        ui,
                        |ui| {
                            if let Some(r) = sidebar::objects(
                                ui,
                                &self.workspace,
                                (self.selected, &self.checked),
                                self.icons.as_ref(),
                            ) {
                                request = Some(r);
                            }
                        },
                    );
                });
            });
        if let Some(request) = request {
            self.sidebar_request(request);
        }
    }

    /// Do what the sidebar, or a model's menu in the viewport, asked.
    fn sidebar_request(&mut self, request: sidebar::Request) {
        match request {
            sidebar::Request::Close => self.perform(Action::ToggleSidebar),
            sidebar::Request::Select(index) => self.selected = Some(index),
            sidebar::Request::Delete(index) => self.delete_object(index),
            sidebar::Request::ToggleHidden(index) => {
                self.change_visibility(|v| v.toggle(index));
            }
            // Isolating a model from the list selects it too.
            sidebar::Request::Isolate(index) => {
                self.change_visibility(|v| v.isolate(index));
                if !self.workspace.is_hidden(index) {
                    self.selected = Some(index);
                }
            }
            sidebar::Request::ShowAll => self.change_visibility(Visibility::show_all),
            sidebar::Request::Transfer { index, to, copy } => {
                self.transfer_object(index, to, copy);
            }
            sidebar::Request::ExportModel(index) => self.open_export_models(Some(index)),
            sidebar::Request::Clone(index) => self.open_clone(index),
            sidebar::Request::Check(index, on) => {
                if on {
                    self.checked.insert(index);
                } else {
                    self.checked.remove(&index);
                }
            }
            sidebar::Request::CheckAll(on) => {
                self.checked = if on {
                    (0..self.workspace.plate().objects.len()).collect()
                } else {
                    std::collections::BTreeSet::new()
                };
            }
            sidebar::Request::HideChecked(hidden) => {
                let indices: Vec<usize> = self.checked.iter().copied().collect();
                self.change_visibility(|v| v.set_hidden(&indices, hidden));
            }
            sidebar::Request::DeleteChecked => self.deleting_checked = !self.checked.is_empty(),
        }
    }

    /// The model under viewport point `at` (in a viewport `size` pixels), if any.
    fn model_at(&mut self, at: [f64; 2], size: [f64; 2]) -> Option<usize> {
        self.workspace
            .hit(&self.camera.ray(at, size))
            .map(|h| h.object)
    }

    /// The menu of the model right-clicked in the viewport, the sidebar row's own (owner,
    /// 2026-11-10): a right click on a model selects it and opens its menu at the pointer; one
    /// elsewhere, or a left click, closes it. A right drag still pans.
    fn model_menu(&mut self, ui: &egui::Ui, response: &egui::Response, hit: Option<usize>) {
        let opened = response.secondary_clicked();
        if opened {
            self.model_menu = hit;
            if hit.is_some() {
                self.selected = hit;
            }
        }
        let set = if opened {
            Some(egui::SetOpenCommand::Bool(hit.is_some()))
        } else if response.clicked() {
            Some(egui::SetOpenCommand::Bool(false))
        } else {
            None
        };
        let mut chosen = None;
        let shown = egui::Popup::menu(response)
            .id(ui.id().with("model-menu"))
            .open_memory(set)
            .at_pointer_fixed()
            .show(|ui| {
                let Some(index) = self
                    .model_menu
                    .filter(|&i| i < self.workspace.plate().objects.len())
                else {
                    ui.close();
                    return;
                };
                let hidden = self.workspace.is_hidden(index);
                if let Some(request) = sidebar::row_menu(ui, &self.workspace, index, hidden) {
                    chosen = Some(request);
                    ui.close();
                }
            });
        if shown.is_none() {
            self.model_menu = None;
        }
        if let Some(request) = chosen {
            self.sidebar_request(request);
        }
    }

    /// The sidebar's plate section: collapsible, its close button in the header, and its
    /// printer, resin and print setting, scrolling when the window leaves the models' list too
    /// little room.
    fn plate_section(&mut self, ui: &mut egui::Ui, request: &mut Option<sidebar::Request>) {
        let plate = egui::collapsing_header::CollapsingState::load_with_default_open(
            ui.ctx(),
            egui::Id::new(PLATE_SECTION),
            true,
        );
        plate
            .show_header(ui, |ui| {
                widgets::name_id(
                    ui.ctx(),
                    egui::Id::new(PLATE_SECTION),
                    &tr!("sidebar-plate"),
                );
                ui.label(
                    egui::RichText::new(tr!("sidebar-plate"))
                        .strong()
                        .color(theme::ACCENT.color()),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if crate::widgets::icon_button(
                        ui,
                        self.icons.as_ref(),
                        crate::icons::Icon::ChevronLeft,
                        false,
                        &tr!("sidebar-close"),
                    )
                    .clicked()
                    {
                        *request = Some(sidebar::Request::Close);
                    }
                });
            })
            .body_unindented(|ui| {
                let models = models_open(ui.ctx());
                let header = 2.0 * ui.spacing().interact_size.y;
                let rows = if models {
                    sidebar::list_height(ui, 1) * sidebar::MIN_ROWS
                } else {
                    0.0
                };
                crate::focus::scrolled(
                    egui::ScrollArea::vertical()
                        .id_salt("sidebar-plate-body")
                        .max_height((ui.available_height() - header - rows).max(120.0)),
                    ui,
                    |ui| {
                        self.profiles.selectors(ui, &mut self.workspace);
                    },
                );
            });
    }

    /// The chosen tool's panel (TD-098), docked at the viewport's right below the view cube:
    /// not movable, collapsible, scrolling within the viewport's height.
    fn tool_panel(&mut self, ctx: &egui::Context, viewport: egui::Rect) {
        let open = self.workflow.tool().filter(|t| t.has_panel());
        // A tool opened anew keeps the values the user set (owner, 2026-11-02); only the
        // cavity view, a way of looking, is left.
        if open != self.opened_tool {
            self.opened_tool = open;
            if open.is_some() {
                self.supports.cavity_view = false;
            }
        }
        self.share_defaults();
        let Some(tool) = open else {
            self.idle_holes();
            self.supports.close();
            self.supports.close_braces();
            return;
        };
        let top = viewport.top() + viewcube::BOTTOM + 8.0;
        // Every panel reaches the viewport's bottom, less a margin (design rule, TD-098).
        let height = panel_height(viewport, top);
        egui::Area::new(egui::Id::new("tool-panel"))
            .pivot(egui::Align2::RIGHT_TOP)
            .fixed_pos(egui::pos2(viewport.right() - 12.0, top))
            .show(ctx, |ui| {
                egui::Frame::window(ui.style()).show(ui, |ui| {
                    // A fixed width: long titles and translations are cut or wrapped, never
                    // widening the panel (design rule, TD-098). An open panel also has a fixed
                    // height, set every frame: an egui area otherwise keeps the size it had for
                    // a shorter tool's content, and cut a longer one.
                    ui.set_width(PANEL_WIDTH);
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                    let top = ui.cursor().top();
                    if self.chrome.panel_open {
                        ui.set_height(height);
                    }
                    ui.horizontal(|ui| {
                        let hint = if self.chrome.panel_open {
                            tr!("panel-collapse")
                        } else {
                            tr!("panel-expand")
                        };
                        // egui's own collapse arrow, drawn rather than a glyph.
                        let (_, arrow) =
                            ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::click());
                        let openness = if self.chrome.panel_open { 1.0 } else { 0.0 };
                        egui::collapsing_header::paint_default_icon(ui, openness, &arrow);
                        widgets::name(&arrow, &hint);
                        if arrow.on_hover_text(hint).clicked() {
                            self.chrome.panel_open = !self.chrome.panel_open;
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            // The Advanced switch shows the settings for experienced users.
                            if let Some(advanced) = self.advanced_switch(tool) {
                                let switch = widgets::switch(ui, advanced);
                                widgets::name(&switch, &tr!("repair-advanced"));
                                ui.label(
                                    egui::RichText::new(tr!("repair-advanced"))
                                        .small()
                                        .color(theme::ACCENT.color()),
                                );
                            }
                            // The tool's name only, cut rather than widening the panel.
                            ui.with_layout(
                                egui::Layout::left_to_right(egui::Align::Center),
                                |ui| {
                                    ui.add(
                                        egui::Label::new(
                                            egui::RichText::new(tool.label()).strong(),
                                        )
                                        .truncate(),
                                    );
                                },
                            );
                        });
                    });
                    if self.chrome.panel_open {
                        ui.separator();
                        // A footer, when the tool has one, stays at the bottom while the rest
                        // scrolls above it: the scrolling part takes the height left under the
                        // header, less the footer's (measured on the frame before).
                        let footer = matches!(
                            tool,
                            Tool::Repair
                                | Tool::Orient
                                | Tool::Hollow
                                | Tool::DrainHoles
                                | Tool::Automatic
                                | Tool::Supports
                                | Tool::Islands
                                | Tool::Braces
                                | Tool::Raft
                                | Tool::Slice
                                | Tool::Layers
                        );
                        let id = egui::Id::new(("panel-footer", tool));
                        body_and_footer(ui, top + height, id, footer, |ui, in_footer| {
                            if in_footer {
                                self.panel_footer(ui, tool);
                            } else {
                                self.panel_body(ui, tool);
                            }
                        });
                    }
                });
            });
        self.leave_other_tools(tool);
    }

    /// The Advanced switch of `tool`'s panel, which shows the settings for experienced users;
    /// `None` for a tool without one.
    fn advanced_switch(&mut self, tool: Tool) -> Option<&mut bool> {
        match tool {
            Tool::Repair => Some(self.hollowing.repair_advanced()),
            Tool::Hollow => Some(self.hollowing.advanced(Part::Hollow)),
            Tool::DrainHoles => Some(self.hollowing.advanced(Part::Holes)),
            Tool::Supports => Some(&mut self.supports.advanced),
            Tool::Raft => Some(&mut self.raft.advanced),
            Tool::Slice => Some(&mut self.slicing.advanced),
            Tool::Islands => Some(&mut self.islands.advanced),
            Tool::Automatic => Some(&mut self.auto.advanced),
            Tool::Orient => Some(&mut self.orient.advanced),
            Tool::Select | Tool::Transform | Tool::Braces | Tool::Layers => None,
        }
    }

    /// Only the open tool, on a selected model, keeps what it is doing: the others' holes,
    /// supports and braces are left.
    fn leave_other_tools(&mut self, tool: Tool) {
        let open = self.chrome.panel_open && self.selected.is_some();
        if !open || tool != Tool::Orient {
            self.orient.close();
        }
        let preparing = matches!(tool, Tool::Hollow | Tool::DrainHoles);
        if !open || !preparing {
            self.idle_holes();
        }
        // The cavity view is the Supports, Islands and Braces tools' alike; the Islands tool adds
        // supports where they are routed.
        let supporting = matches!(
            tool,
            Tool::Automatic | Tool::Supports | Tool::Islands | Tool::Braces
        );
        // The Islands and Automatic tools, and the Supports tool's Grid mode (TD-138), plan on the
        // surface for routes.
        self.supports.route_wanted = open
            && (matches!(tool, Tool::Islands | Tool::Automatic)
                || (tool == Tool::Supports && self.grid.active));
        if (!open || tool != Tool::Supports) && self.grid.active {
            self.grid.toggle();
        }
        if !open || !supporting {
            self.supports.close();
        } else if tool != Tool::Supports {
            self.supports.stop_editing();
        }
        if !open || tool != Tool::Braces {
            self.supports.close_braces();
        }
    }

    /// How many supports, braces or drain holes `tool` would delete from model `index`, and the
    /// button's label; `None` for a tool that deletes none.
    fn deletable(&self, tool: Tool, index: Option<usize>) -> Option<(usize, String)> {
        let document = index.and_then(|i| self.workspace.supports(i));
        match tool {
            Tool::Supports => Some((
                document.map_or(0, |d| d.supports.len()),
                tr!("delete-all-supports"),
            )),
            Tool::Braces => Some((
                document.map_or(0, |d| d.braces.len()),
                tr!("delete-all-braces"),
            )),
            Tool::DrainHoles => Some((
                index.map_or(0, |i| self.hollowing.hole_count(&self.workspace, i)),
                tr!("delete-all-holes"),
            )),
            Tool::Islands => Some((
                document.map_or(0, |d| {
                    d.supports
                        .iter()
                        .filter(|s| s.origin == karasu_core::supports::SupportOrigin::Islands)
                        .count()
                }),
                tr!("delete-island-supports"),
            )),
            Tool::Automatic => Some((
                document.map_or(0, |d| {
                    d.supports
                        .iter()
                        .filter(|s| s.origin == karasu_core::supports::SupportOrigin::Automatic)
                        .count()
                }),
                tr!("delete-automatic-supports"),
            )),
            _ => None,
        }
    }

    /// The footer's "Delete all" of `tool` (owner, 2026-10-29): the selected model's supports,
    /// braces or drain holes, once confirmed.
    fn delete_all_button(&mut self, ui: &mut egui::Ui, tool: Tool) {
        let Some((count, label)) = self.deletable(tool, self.selected) else {
            return;
        };
        let size = egui::vec2(ui.available_width(), ui.spacing().interact_size.y);
        let enabled = count > 0 && self.selected.is_some();
        let clicked = ui
            .add_enabled(enabled, egui::Button::new(label).truncate().min_size(size))
            .on_hover_text(tr!("delete-all-hint"))
            .clicked();
        if let (true, Some(index)) = (clicked, self.selected) {
            self.clearing = Some((tool, index));
        }
        ui.add_space(4.0);
    }

    /// The confirmation of a tool's "Delete all": deleted on its button or Enter, as one
    /// undoable step; kept on Cancel or Escape, or when there is nothing left to delete.
    fn confirm_clearing(&mut self, ctx: &egui::Context) {
        let Some((tool, index)) = self.clearing else {
            return;
        };
        let count = self.deletable(tool, Some(index)).map_or(0, |(n, _)| n);
        if count == 0 || index >= self.workspace.plate().objects.len() {
            self.clearing = None;
            return;
        }
        let braces = self.workspace.supports(index).map_or(0, |d| d.braces.len());
        let (title, text) = match tool {
            Tool::Supports => (
                tr!("delete-all-supports-title"),
                tr!("delete-all-supports-text", count = count, braces = braces),
            ),
            Tool::Braces => (
                tr!("delete-all-braces-title"),
                tr!("delete-all-braces-text", count = count),
            ),
            Tool::Islands => (
                tr!("delete-island-supports-title"),
                tr!("delete-island-supports-text", count = count),
            ),
            Tool::Automatic => (
                tr!("delete-automatic-supports-title"),
                tr!("delete-automatic-supports-text", count = count),
            ),
            _ => (
                tr!("delete-all-holes-title"),
                tr!("delete-all-holes-text", count = count),
            ),
        };
        match crate::dialog::confirm(
            ctx,
            "delete-all",
            &title,
            &text,
            &tr!("delete-all-action"),
            true,
        ) {
            Some(true) => {
                self.clearing = None;
                match tool {
                    Tool::Supports => self
                        .supports
                        .delete_all_supports(&mut self.workspace, index),
                    Tool::Braces => self.supports.delete_all_braces(&mut self.workspace, index),
                    Tool::Islands => self
                        .supports
                        .delete_island_supports(&mut self.workspace, index),
                    Tool::Automatic => self
                        .supports
                        .delete_automatic_supports(&mut self.workspace, index),
                    _ => self.hollowing.delete_all_holes(&mut self.workspace, index),
                }
            }
            Some(false) => self.clearing = None,
            None => {}
        }
    }

    /// The footer of `tool`'s panel: its actions and the selected model's state.
    fn panel_footer(&mut self, ui: &mut egui::Ui, tool: Tool) {
        self.delete_all_button(ui, tool);
        let part = match tool {
            Tool::Repair => {
                self.hollowing.repair_footer(
                    ui,
                    &mut self.workspace,
                    &self.preparation,
                    self.selected,
                    self.icons.as_ref(),
                );
                return;
            }
            Tool::Hollow => Part::Hollow,
            Tool::DrainHoles => Part::Holes,
            Tool::Supports if self.grid.active => {
                self.grid
                    .footer(ui, &mut self.workspace, self.selected, self.icons.as_ref());
                return;
            }
            Tool::Supports => {
                self.supports.footer(
                    ui,
                    &mut self.workspace,
                    &self.preparation,
                    self.selected,
                    self.icons.as_ref(),
                );
                return;
            }
            Tool::Automatic => {
                self.auto.footer(
                    ui,
                    &mut self.workspace,
                    (self.selected, self.supports.automatic.lift_mm),
                    self.icons.as_ref(),
                );
                return;
            }
            Tool::Orient => {
                self.orient.footer(
                    ui,
                    &mut self.workspace,
                    &self.preparation,
                    self.selected,
                    self.icons.as_ref(),
                );
                return;
            }
            Tool::Islands => {
                self.islands.footer(
                    ui,
                    &mut self.workspace,
                    &self.preparation,
                    self.selected,
                    self.icons.as_ref(),
                );
                return;
            }
            Tool::Braces => {
                self.supports.braces_footer(
                    ui,
                    &mut self.workspace,
                    self.selected,
                    self.icons.as_ref(),
                );
                return;
            }
            Tool::Raft => {
                crate::raft_ui::RaftUi::footer(
                    ui,
                    &self.workspace,
                    self.selected,
                    self.icons.as_ref(),
                );
                return;
            }
            Tool::Slice => {
                self.slicing
                    .footer(ui, &self.workspace, self.icons.as_ref());
                return;
            }
            Tool::Layers => {
                self.slicing
                    .layers_footer(ui, &self.workspace, self.icons.as_ref());
                return;
            }
            Tool::Select | Tool::Transform => return,
        };
        let to_holes = self.hollowing.footer(
            ui,
            &mut self.workspace,
            &mut self.preparation,
            self.selected,
            part,
            self.icons.as_ref(),
        );
        if to_holes {
            self.perform(Action::Tool(Tool::DrainHoles));
        }
    }

    /// The content of `tool`'s panel.
    fn panel_body(&mut self, ui: &mut egui::Ui, tool: Tool) {
        match tool {
            // The plate's: it needs no selected model.
            Tool::Slice => {
                self.slicing
                    .panel(ui, &mut self.workspace, self.section.planner());
                // Never a refusal: what the last search found (owner mockup, 2026-10-31).
                self.hollowing.cups_slice_fact(ui, &self.workspace);
            }
            // The slice's layers: the plate's too.
            Tool::Layers => self.slicing.layers_panel(ui, self.icons.as_ref()),
            Tool::Transform => {
                if self.selected.is_none() {
                    self.no_model_notice(ui);
                }
                let appearance = self.appearance;
                let chosen = self.transform.panel(
                    ui,
                    &mut self.workspace,
                    self.selected,
                    self.mode,
                    self.space,
                    self.icons.as_ref(),
                    &appearance,
                );
                if let Some(action) = chosen {
                    self.perform(action);
                }
            }
            Tool::Repair => {
                if self.selected.is_none() {
                    self.no_model_notice(ui);
                }
                let appearance = self.appearance;
                self.hollowing.repair_panel(
                    ui,
                    &mut self.workspace,
                    &self.preparation,
                    self.selected,
                    &appearance,
                );
            }
            Tool::Hollow | Tool::DrainHoles => {
                if self.selected.is_none() {
                    self.no_model_notice(ui);
                }
                let part = if tool == Tool::Hollow {
                    Part::Hollow
                } else {
                    Part::Holes
                };
                self.hollowing.panel(
                    ui,
                    &mut self.workspace,
                    &mut self.preparation,
                    self.selected,
                    part,
                    self.icons.as_ref(),
                );
            }
            Tool::Supports => {
                if self.selected.is_none() {
                    self.no_model_notice(ui);
                }
                self.supports_panel(ui);
            }
            Tool::Automatic => self.auto_panel(ui),
            Tool::Orient => self.orient_panel(ui),
            Tool::Islands => {
                if self.selected.is_none() {
                    self.no_model_notice(ui);
                }
                self.islands.panel(
                    ui,
                    &mut self.workspace,
                    &self.preparation,
                    &mut self.supports,
                    self.selected,
                    self.icons.as_ref(),
                );
            }
            Tool::Braces => {
                if self.selected.is_none() {
                    self.no_model_notice(ui);
                }
                self.supports
                    .braces_panel(ui, &mut self.workspace, self.selected);
            }
            Tool::Raft => {
                if self.selected.is_none() {
                    self.no_model_notice(ui);
                }
                let defaults = self.tool_defaults.raft;
                self.raft.panel(
                    ui,
                    &mut self.workspace,
                    &self.supports,
                    self.selected,
                    defaults,
                );
            }
            Tool::Select => {}
        }
    }

    /// The Orient tool's panel body.
    fn orient_panel(&mut self, ui: &mut egui::Ui) {
        if self.selected.is_none() {
            self.no_model_notice(ui);
        }
        self.orient
            .panel(ui, &mut self.workspace, self.selected, self.icons.as_ref());
    }

    /// The Supports tool's panel body: the Grid mode's while it is on (TD-138).
    fn supports_panel(&mut self, ui: &mut egui::Ui) {
        if self.grid.active {
            self.grid
                .panel(ui, &self.workspace, &mut self.supports, self.selected);
        } else {
            self.supports.panel(ui, &mut self.workspace, self.selected);
        }
    }

    /// The Automatic tool's panel body.
    fn auto_panel(&mut self, ui: &mut egui::Ui) {
        if self.selected.is_none() {
            self.no_model_notice(ui);
        }
        self.auto
            .panel(ui, &self.workspace, &mut self.supports, self.selected);
    }

    /// The notice of a tool panel that needs a selected model and has none.
    fn no_model_notice(&self, ui: &mut egui::Ui) {
        toasts::notice(
            ui,
            Severity::Info,
            &tr!("panel-no-model"),
            self.icons.as_ref(),
        );
        ui.add_space(6.0);
    }

    /// Center model `index` on the plate, fit it to the plate, or reset its rotation or scale.
    fn transform_model(&mut self, action: Action, index: usize) {
        let fit = (action == Action::FitToPlate)
            .then(|| {
                let (_, placement) = self.workspace.anchor(index)?;
                let size = [
                    placement.max.x - placement.min.x,
                    placement.max.y - placement.min.y,
                    placement.max.z - placement.min.z,
                ];
                let volume = self.workspace.printer()?.build_size_mm;
                transform_ui::fit_factor(size, volume)
            })
            .flatten();
        self.workspace.edit(|plate| {
            let Some(object) = plate.objects.get_mut(index) else {
                return;
            };
            match action {
                Action::CenterModel => object.pose.center = true,
                Action::ResetRotation => object.pose.rotate_deg = [0.0; 3],
                Action::ResetScale => object.pose.scale = [1.0; 3],
                // Scaled to fit, then centred and rested, so it lies inside the volume.
                Action::FitToPlate => {
                    if let Some(factor) = fit {
                        for scale in &mut object.pose.scale {
                            *scale = (*scale * factor).clamp(0.01, 1000.0);
                        }
                        object.pose.center = true;
                        object.pose.drop = true;
                        object.pose.lift_mm = None;
                    }
                }
                _ => {}
            }
        });
    }

    /// No drain-hole tool: holes are neither placed nor dragged, and only pending holes keep
    /// their probes.
    fn idle_holes(&mut self) {
        self.hollowing.close();
        self.hollowing.blockers.close();
        self.preparation.set_probes(self.hollowing.probes(None));
    }

    /// Delete object `index`, ending any drag on it.
    /// Change which objects the scene hides. A hidden object cannot stay selected or dragged.
    fn change_visibility(&mut self, change: impl FnOnce(&mut Visibility)) {
        change(self.workspace.visibility());
        if self
            .selected
            .is_some_and(|index| self.workspace.is_hidden(index))
        {
            self.selected = None;
            self.drag = None;
        }
    }

    /// The confirmation of the model the Delete key asked to remove: deleted on Delete or Enter,
    /// kept on Cancel or Escape (and forgotten if the plate changed under it).
    fn confirm_deletion(&mut self, ctx: &egui::Context) {
        let Some(index) = self.deleting else {
            return;
        };
        let Some(name) = self
            .workspace
            .plate()
            .objects
            .get(index)
            .map(|o| o.name.clone())
        else {
            self.deleting = None;
            return;
        };
        match crate::dialog::confirm(
            ctx,
            "delete-model",
            &tr!("delete-model-title", name = &name),
            &tr!("delete-model-text"),
            &tr!("action-delete"),
            true,
        ) {
            Some(true) => {
                self.deleting = None;
                self.delete_object(index);
            }
            Some(false) => self.deleting = None,
            None => {}
        }
    }

    /// Forget object `index` of the plate shown, about to leave it: its drag, its visibility
    /// and the selection, the objects after it moving up one.
    fn forget_object(&mut self, index: usize) {
        self.drag = None;
        self.workspace.visibility().removed(index);
        // The checks after it move up with their models.
        self.checked = self
            .checked
            .iter()
            .filter(|&&k| k != index)
            .map(|&k| if k > index { k - 1 } else { k })
            .collect();
        if self.selected == Some(index) {
            self.selected = None;
        } else if let Some(selected) = self.selected.filter(|&s| s > index) {
            self.selected = Some(selected - 1);
        }
    }

    /// The deletions waiting for the user's answer: the model the Delete key asked for, and the
    /// checked models.
    fn confirm_deletions(&mut self, ctx: &egui::Context) {
        self.confirm_deletion(ctx);
        self.confirm_checked_deletion(ctx);
        self.about_dialog(ctx);
    }

    /// The checked models' deletion, once asked: confirmed, they leave the plate in one
    /// undoable step; refused, they stay checked.
    fn confirm_checked_deletion(&mut self, ctx: &egui::Context) {
        if !self.deleting_checked {
            return;
        }
        let count = self.workspace.plate().objects.len();
        let indices: Vec<usize> = self
            .checked
            .iter()
            .copied()
            .filter(|&k| k < count)
            .collect();
        if indices.is_empty() {
            self.deleting_checked = false;
            return;
        }
        match crate::dialog::confirm(
            ctx,
            "delete-checked",
            &tr!("delete-checked-title", count = indices.len()),
            &tr!("delete-checked-text"),
            &tr!("action-delete"),
            true,
        ) {
            Some(true) => {
                self.deleting_checked = false;
                self.delete_objects(&indices);
            }
            Some(false) => self.deleting_checked = false,
            None => {}
        }
    }

    /// Delete objects `indices` (ascending) in one undoable step.
    fn delete_objects(&mut self, indices: &[usize]) {
        for &index in indices.iter().rev() {
            self.forget_object(index);
        }
        self.workspace.edit(|plate| {
            for &index in indices.iter().rev() {
                if index < plate.objects.len() {
                    plate.objects.remove(index);
                }
            }
        });
    }

    fn delete_object(&mut self, index: usize) {
        self.forget_object(index);
        self.workspace.edit(|plate| {
            if index < plate.objects.len() {
                plate.objects.remove(index);
            }
        });
    }

    /// The status bar (owner, M6.5): the objects and triangles; then one line of short
    /// messages, cut with an ellipsis when too long; at the right the Messages button and the
    /// graphics adapter.
    fn status_bar(&mut self, ui: &mut egui::Ui, adapter: Option<&gpu::Selected>) {
        let objects = self.workspace.plate().objects.len();
        let triangles = self.workspace.triangles();
        let statuses = self.statuses();
        let samples = self.samples();
        egui::Panel::bottom("status")
            .frame(
                egui::Frame::new()
                    .fill(theme::SURFACE.color())
                    .inner_margin(6.0),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(format!(
                        "{} · {}",
                        tr!("status-objects", count = objects),
                        tr!("status-triangles", count = triangles)
                    ));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        match adapter {
                            Some(a) => ui.label(tr!(
                                "status-gpu",
                                adapter = &a.adapter,
                                backend = &a.backend,
                                samples = samples.unwrap_or(1),
                            )),
                            None => ui.label(tr!("status-gpu-none")),
                        };
                        ui.separator();
                        self.messages.status(ui);
                        // Right to left: the short messages take the space left over.
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            if !statuses.is_empty() {
                                ui.separator();
                                status_line(ui, &statuses);
                            }
                        });
                    });
                });
            });
    }

    /// The status bar's short messages, most urgent first: work in progress, then what is
    /// wrong, then the latest message.
    fn statuses(&mut self) -> Vec<Status> {
        let mut statuses = Vec::new();
        let busy = |text| Status {
            busy: true,
            ..Status::new(text, theme::TEXT_PRIMARY.color())
        };
        let importing: Vec<&str> = self.workspace.importing().collect();
        if !importing.is_empty() {
            statuses.push(busy(tr!("status-importing", names = importing.join(", "))));
        }
        let diagnosing: Vec<&str> = self.workspace.diagnosing().collect();
        if !diagnosing.is_empty() {
            statuses.push(busy(tr!("status-checking", names = diagnosing.join(", "))));
        }
        if let Some(batch) = self.batch_status() {
            statuses.push(busy(batch));
        } else if let Some(slicing) = self.slicing.running_status() {
            statuses.push(busy(slicing));
        }
        statuses.extend(self.preparation.statuses(&self.workspace));
        statuses.extend(self.supports.statuses());
        let outside = self.workspace.outside().len();
        if outside > 0 {
            let text = tr!("status-outside", count = outside);
            statuses.push(Status::new(text, theme::WARNING.color()));
        }
        let floating = self.workspace.floating().len();
        if floating > 0 {
            let text = tr!("status-floating", count = floating);
            statuses.push(Status::new(text, theme::TEXT_PRIMARY.color()));
        }
        statuses.extend(self.messages.latest_line());
        statuses
    }

    /// The scene's draws: the models and their previews, opened by the drain holes, and the
    /// holes themselves. Also the index of the selected object's draw, which is outlined.
    fn scene_draws(&mut self, state: &egui_wgpu::RenderState) -> (Vec<Draw>, Option<usize>) {
        let selected = self.selected;
        let colors = self.appearance;
        // The holes first: their cylinders open the models and cavities drawn next.
        let holes = match self.viewport.as_mut() {
            Some(viewport) => {
                // Holes are shown as edited only with the Drain holes tool, on the selected
                // model; elsewhere they are openings in the model's colour.
                let focus = selected.filter(|_| self.workflow.tool() == Some(Tool::DrainHoles));
                let holes = self.hollowing.draws(
                    viewport,
                    state,
                    &self.workspace,
                    &mut self.preparation,
                    &colors,
                    focus,
                    selected,
                );
                viewport.set_holes(state, self.hollowing.openings());
                holes
            }
            None => Vec::new(),
        };
        let cuts = |index| self.hollowing.cuts(index);
        // The selected model's supports, with the Supports or Braces tool: drawn as built there.
        let tool = self.supports_editing();
        let editing = selected.filter(|_| tool.is_some());
        let objects = self.workspace.draws(
            |index| {
                let color = if Some(index) == selected {
                    colors.selected
                } else {
                    colors.object
                };
                color.linear()
            },
            cuts,
        );
        let outlined = objects
            .iter()
            .position(|&(index, _)| Some(index) == selected);
        let inside = self.cavity_viewed();
        // The Orient tool's candidate hovered or chosen: the selected model drawn turned to it,
        // its overhangs tinted for that pose (TD-119).
        let orient_focus = selected.filter(|_| self.workflow.tool() == Some(Tool::Orient));
        let turned = self.orient.preview(&self.workspace, orient_focus);
        // Every model takes the outside filter: the shader applies it only where the surface
        // leaves the build volume.
        let mut draws: Vec<Draw> = objects
            .into_iter()
            .map(|(index, draw)| {
                let draw = match (&turned, Some(index) == orient_focus) {
                    (Some(turn), true) => Draw {
                        model: turn.mul(&draw.model),
                        ..draw
                    },
                    _ => draw,
                };
                let draw = Draw {
                    outside: Some(if Some(index) == selected {
                        colors.outside_filter_selected.filter()
                    } else {
                        colors.outside_filter.filter()
                    }),
                    ..draw
                };
                if Some(index) == inside {
                    Draw {
                        color: colors.model_ghost.linear(),
                        cap: crate::viewport::Cap::Off,
                        look: crate::viewport::Look::Ghost,
                        ..draw
                    }
                } else {
                    draw
                }
            })
            .collect();
        draws.extend(
            self.preparation
                .draws(&self.workspace, &colors, selected, inside, cuts),
        );
        draws.extend(holes);
        draws.extend(self.blocker_draws(state, selected));
        draws.extend(self.supports.draws(
            &self.workspace,
            selected,
            tool.filter(|_| editing.is_some()),
            &colors,
        ));
        draws.extend(self.drainage_draws(state, selected));
        draws.extend(self.island_draws(state, selected));
        draws.extend(self.auto_draws(state, selected));
        draws.extend(self.grid_draws(state, selected));
        draws.extend(self.orient_draws(state, selected));
        draws.extend(self.repair_draws(state, selected));
        // A project being opened shows all at once, when it is ready (owner, 2026-11-04): its
        // draws are made, and their meshes uploaded, but not drawn yet.
        if self.project_file.loading() {
            return (Vec::new(), None);
        }
        (draws, outlined)
    }

    /// The selected object's defects and repairs (TD-092 r8), a constant size on screen, with
    /// the Repair tool only (owner, M6.5).
    fn repair_draws(
        &mut self,
        state: &egui_wgpu::RenderState,
        selected: Option<usize>,
    ) -> Vec<Draw> {
        let repairing = self.workflow.tool() == Some(Tool::Repair);
        let Some(viewport) = self.viewport.as_mut().filter(|_| repairing) else {
            return Vec::new();
        };
        let eye = self.camera.eye();
        self.hollowing.repair_draws(
            viewport,
            state,
            &mut self.workspace,
            &self.preparation,
            selected,
            eye,
            &self.appearance,
        )
    }

    /// The `selected` model's hollowing blockers, with the Hollow tool (owner, 2026-11-02).
    fn blocker_draws(
        &mut self,
        state: &egui_wgpu::RenderState,
        selected: Option<usize>,
    ) -> Vec<Draw> {
        let colors = self.appearance;
        let focus = selected.filter(|_| self.workflow.tool() == Some(Tool::Hollow));
        match self.viewport.as_mut() {
            Some(viewport) => {
                self.hollowing
                    .blockers
                    .draws(viewport, state, &self.workspace, focus, &colors)
            }
            None => Vec::new(),
        }
    }

    /// The `selected` model's undrained cavity regions (TD-101) and suction cups (TD-114), with
    /// the Drain holes tool.
    fn drainage_draws(
        &mut self,
        state: &egui_wgpu::RenderState,
        selected: Option<usize>,
    ) -> Vec<Draw> {
        let colors = self.appearance;
        let eye = self.camera.eye();
        let focus = selected.filter(|_| self.workflow.tool() == Some(Tool::DrainHoles));
        let Some(viewport) = self.viewport.as_mut() else {
            return Vec::new();
        };
        let mut draws = self.hollowing.cavity_draws(
            viewport,
            state,
            &self.workspace,
            &self.preparation,
            focus,
            eye,
            &colors,
        );
        draws.extend(self.hollowing.cups.draws(
            viewport,
            state,
            &self.workspace,
            focus,
            eye,
            &colors,
        ));
        draws
    }

    /// The `selected` model's islands and local minima, with the Islands tool.
    fn island_draws(
        &mut self,
        state: &egui_wgpu::RenderState,
        selected: Option<usize>,
    ) -> Vec<Draw> {
        let colors = self.appearance;
        let eye = self.camera.eye();
        let focus = selected.filter(|_| self.workflow.tool() == Some(Tool::Islands));
        match self.viewport.as_mut() {
            Some(viewport) => {
                self.islands
                    .draws(viewport, state, &self.workspace, focus, eye, &colors)
            }
            None => Vec::new(),
        }
    }

    /// The `selected` model's planned supports and places left unsupported, with the Automatic
    /// tool.
    /// The Orient tool's protected points on the `selected` model.
    fn orient_draws(
        &mut self,
        state: &egui_wgpu::RenderState,
        selected: Option<usize>,
    ) -> Vec<Draw> {
        let colors = self.appearance;
        let eye = self.camera.eye();
        let focus = selected.filter(|_| self.workflow.tool() == Some(Tool::Orient));
        match self.viewport.as_mut() {
            Some(viewport) => {
                self.orient
                    .draws(viewport, state, &self.workspace, focus, eye, &colors)
            }
            None => Vec::new(),
        }
    }

    fn auto_draws(&mut self, state: &egui_wgpu::RenderState, selected: Option<usize>) -> Vec<Draw> {
        let colors = self.appearance;
        let eye = self.camera.eye();
        let focus = selected.filter(|_| self.workflow.tool() == Some(Tool::Automatic));
        match self.viewport.as_mut() {
            Some(viewport) => {
                self.auto
                    .draws(viewport, state, &self.workspace, focus, eye, &colors)
            }
            None => Vec::new(),
        }
    }

    fn grid_draws(&mut self, state: &egui_wgpu::RenderState, selected: Option<usize>) -> Vec<Draw> {
        let colors = self.appearance;
        let focus = selected.filter(|_| self.workflow.tool() == Some(Tool::Supports));
        match self.viewport.as_mut() {
            Some(viewport) => self
                .grid
                .draws(viewport, state, &self.workspace, focus, &colors),
            None => Vec::new(),
        }
    }

    /// A click at `at` in the viewport `rect` on a marker of the tool in use: a cavity region's
    /// shows it (TD-101), unless a hole is being placed; an island's chooses it. Returns whether
    /// it was on one.
    fn click_marker(&mut self, rect: egui::Rect, at: egui::Pos2) -> bool {
        match self.workflow.tool() {
            Some(Tool::DrainHoles) if !self.hollowing.tracking() => {
                self.hollowing.click_cavity_marker(&self.camera, rect, at)
                    || self.hollowing.cups.click_marker(
                        &self.workspace,
                        self.selected,
                        &self.camera,
                        rect,
                        at,
                    )
            }
            Some(Tool::Islands) => self.islands.click_marker(&self.camera, rect, at),
            _ => false,
        }
    }

    /// The model seen in the cavity view (owner, 2026-10-29): the selected hollowed model, its
    /// outside a ghost and its cavity seen from inside, with the Supports, Islands or Braces
    /// tool.
    fn cavity_viewed(&self) -> Option<usize> {
        let viewing = matches!(
            self.workflow.tool(),
            Some(Tool::Automatic | Tool::Supports | Tool::Islands | Tool::Braces)
        );
        self.selected
            .filter(|_| viewing && self.supports.cavity_view && self.selected_hollowed())
    }

    /// Which tool edits the selected model's supports, if one is in use.
    fn supports_editing(&self) -> Option<crate::supports_ui::Editing> {
        match self.workflow.tool() {
            Some(Tool::Supports) => Some(crate::supports_ui::Editing::Supports),
            Some(Tool::Braces) => Some(crate::supports_ui::Editing::Braces),
            _ => None,
        }
    }

    /// The handles of the selected hole or support over the viewport `rect`.
    fn paint_tool_handles(&mut self, ui: &egui::Ui, rect: egui::Rect) {
        let pointer = ui.input(|i| i.pointer.hover_pos());
        self.hollowing.paint_handles(
            ui.painter(),
            rect,
            &self.camera,
            &self.workspace,
            &mut self.preparation,
            pointer,
            &self.appearance,
        );
        match self.workflow.tool() {
            Some(Tool::Hollow) => {
                let handles = self.blocker_handles(rect);
                let size = [f64::from(rect.width()), f64::from(rect.height())];
                let local =
                    |p: egui::Pos2| [f64::from(p.x - rect.min.x), f64::from(p.y - rect.min.y)];
                let hovered = pointer
                    .map(local)
                    .and_then(|p| manipulate::hit(&handles, &self.camera, size, p, HANDLE_REACH));
                manipulate::draw(
                    ui.painter(),
                    rect,
                    &self.camera,
                    &handles,
                    hovered,
                    &self.appearance,
                    false,
                );
            }
            Some(Tool::DrainHoles) => {
                let rings = self.hole_rings(rect);
                let size = [f64::from(rect.width()), f64::from(rect.height())];
                let local =
                    |p: egui::Pos2| [f64::from(p.x - rect.min.x), f64::from(p.y - rect.min.y)];
                // The dragged ring stays lit; otherwise the one under the pointer, unless a drag
                // of another handle is under way.
                let hovered = match self.hollowing.turning() {
                    Some(k) => Some(k),
                    None if self.hollowing.dragging() => None,
                    None => pointer
                        .map(local)
                        .and_then(|p| manipulate::hit(&rings, &self.camera, size, p, HANDLE_REACH)),
                };
                manipulate::draw(
                    ui.painter(),
                    rect,
                    &self.camera,
                    &rings,
                    hovered,
                    &self.appearance,
                    false,
                );
            }
            Some(Tool::Supports) => self.supports.paint_handles(
                ui.painter(),
                rect,
                &self.camera,
                &self.workspace,
                self.selected,
                pointer,
                &self.appearance,
            ),
            Some(Tool::Braces) => self.supports.paint_brace_handles(
                ui.painter(),
                rect,
                &self.camera,
                &self.workspace,
                self.selected,
                pointer,
                &self.appearance,
            ),
            _ => {}
        }
    }

    /// Follow the pointer's `ray` with where a support would go while supports are placed.
    fn follow_supports(&mut self, ray: Option<&Ray>) {
        if self.workflow.tool() == Some(Tool::Braces) {
            if let Some(selected) = self.selected {
                self.supports.brace_hover(ray, selected);
            }
            return;
        }
        // Supports go only where the section shows the surface, as holes do. A new support's
        // foot follows the pointer as a dragged one does.
        if self.supports.following() {
            let section = self.section.section(&mut self.workspace);
            if let (Some(ray), Some(selected)) = (ray, self.selected) {
                // Found only for a support inside the cavity: the search scans the cavity.
                let cavity = if self.supports.dragging_inside() {
                    self.preparation
                        .cavity_hit(&self.workspace, selected, ray, section)
                } else {
                    None
                };
                // On the height Shift holds (TD-137).
                let held = if self.supports.holding() {
                    let inside = self.supports.dragging_inside();
                    let free = if inside {
                        cavity
                    } else {
                        self.workspace.hit_in(ray, section)
                    };
                    self.supports.tip_at(
                        &self.workspace,
                        &self.preparation,
                        (selected, ray, free),
                        section,
                        inside,
                    )
                } else {
                    crate::supports_ui::height::TipAt::Free
                };
                self.supports
                    .drag_to(&mut self.workspace, ray, (section, held), cavity);
            }
            return;
        }
        let (Some(selected), true) = (self.selected, self.supports.placing()) else {
            return;
        };
        // A branch's tip clicked: the shaft under the pointer is what it would join.
        if self.supports.branch_waiting() {
            self.supports.branch_hover(&self.workspace, ray, selected);
            return;
        }
        let hit = ray.and_then(|r| self.support_hit(r, selected));
        self.supports.hover(&self.workspace, hit, selected);
    }

    /// Where `ray` meets the `selected` model for a new support: its outside, or its cavity
    /// seen from inside when supports go there; only where the section shows the surface.
    fn support_hit(&mut self, ray: &Ray, selected: usize) -> Option<crate::workspace::SurfaceHit> {
        let section = self.section.section(&mut self.workspace);
        let cavity = self.supports.on_cavity;
        let free = if cavity {
            self.preparation
                .cavity_hit(&self.workspace, selected, ray, section)
        } else {
            self.workspace.hit_in(ray, section)
        };
        // On the height Shift holds (TD-137).
        match self.supports.tip_at(
            &self.workspace,
            &self.preparation,
            (selected, ray, free),
            section,
            cavity,
        ) {
            crate::supports_ui::height::TipAt::Held(hit) => hit,
            crate::supports_ui::height::TipAt::Free => free,
        }
    }

    /// A press on the viewport `rect` at `at`, along `ray`, for the tool in use on the
    /// `selected` model: a hole's or a support's handle, or a hole's solid, starts a drag.
    /// Returns whether the tool took it.
    fn tool_press(&mut self, rect: egui::Rect, at: egui::Pos2, ray: &Ray, selected: usize) -> bool {
        match self.workflow.tool() {
            Some(Tool::Hollow) => {
                let handles = self.blocker_handles(rect);
                let size = [f64::from(rect.width()), f64::from(rect.height())];
                let pointer = [f64::from(at.x - rect.min.x), f64::from(at.y - rect.min.y)];
                let hit = manipulate::hit(&handles, &self.camera, size, pointer, HANDLE_REACH);
                self.hollowing.blockers.press(
                    &self.workspace,
                    selected,
                    &handles,
                    hit,
                    ray,
                    self.space,
                )
            }
            Some(Tool::DrainHoles) => {
                let rings = self.hole_rings(rect);
                self.hollowing.press(
                    &self.workspace,
                    &mut self.preparation,
                    &self.camera,
                    rect,
                    at,
                    ray,
                    selected,
                    &rings,
                    self.space,
                )
            }
            // With Rectangle or Lasso, a drag draws the grid's shape (TD-138).
            Some(Tool::Supports) if self.grid.shaping() => {
                self.grid
                    .begin_shape([f64::from(at.x - rect.min.x), f64::from(at.y - rect.min.y)]);
                true
            }
            Some(Tool::Supports) => {
                self.supports
                    .press(&mut self.workspace, &self.camera, rect, at, selected)
            }
            Some(Tool::Braces) => {
                self.supports
                    .brace_press(&self.workspace, &self.camera, rect, at, selected)
            }
            _ => false,
        }
    }

    /// A click on the viewport along `ray`, for the tool in use on the `selected` model: a
    /// hole or a support placed or picked. Returns whether the tool took it.
    fn tool_click(&mut self, ray: &Ray, selected: usize) -> bool {
        match self.workflow.tool() {
            // Blockers go where the section shows the surface, as holes do.
            Some(Tool::Hollow) => {
                let section = self.section.section(&mut self.workspace);
                let hit = self.workspace.hit_in(ray, section);
                self.hollowing
                    .blockers
                    .click(&self.workspace, ray, hit, selected)
            }
            Some(Tool::DrainHoles) => {
                self.hollowing
                    .click(&mut self.workspace, &mut self.preparation, ray, selected)
            }
            // With the Grid mode on, a click picks the face to grid (TD-138).
            Some(Tool::Supports) if self.grid.active => {
                let hit = self.support_hit(ray, selected);
                let cavity = self.supports.on_cavity;
                self.grid.click(&self.workspace, hit, (selected, cavity))
            }
            Some(Tool::Supports) => {
                let hit = self.support_hit(ray, selected);
                self.supports.click(&mut self.workspace, ray, hit, selected)
            }
            Some(Tool::Braces) => self
                .supports
                .brace_click(&mut self.workspace, ray, selected),
            // With Protect, a click on the selected model protects the point there.
            Some(Tool::Orient) if self.orient.protecting => {
                match self.workspace.hit(ray).filter(|h| h.object == selected) {
                    Some(hit) => crate::orient_ui::OrientUi::add_point(
                        &mut self.workspace,
                        selected,
                        hit.point,
                        hit.normal,
                    ),
                    None => false,
                }
            }
            _ => false,
        }
    }

    /// Follow the pointer's `ray` with the hole placed or dragged, if any.
    fn follow_holes(&mut self, ray: Option<&Ray>) {
        let (Some(selected), true) = (self.selected, self.hollowing.tracking()) else {
            return;
        };
        // Holes go only where the section shows the surface.
        let section = self.section.section(&mut self.workspace);
        let hit = ray.and_then(|r| self.workspace.hit_in(r, section));
        let cavity = ray
            .filter(|_| self.hollowing.wants_cavity(&self.workspace))
            .and_then(|r| {
                self.preparation
                    .cavity_hit(&self.workspace, selected, r, section)
            });
        self.hollowing.hover(
            &self.workspace,
            &mut self.preparation,
            selected,
            ray,
            hit,
            cavity,
        );
    }

    fn viewport(&mut self, ui: &mut egui::Ui, frame: &eframe::Frame) {
        let rect = ui.max_rect();
        // Clicked and dragged, but no stop of the Tab key: the camera has no keys (owner,
        // 2026-11-10, TD-142).
        let response = ui.allocate_rect(rect, egui::Sense::CLICK | egui::Sense::DRAG);
        widgets::name(&response, &tr!("a11y-viewport"));
        self.aspect = f64::from(rect.width().max(1.0)) / f64::from(rect.height().max(1.0));
        self.keys(ui);
        let handles = self.handles(rect);
        let hovered = self.interact(ui, &response, &handles);
        self.advance_turn(ui.ctx());
        let Some(state) = frame.wgpu_render_state() else {
            return;
        };
        if self.viewport.is_none() {
            self.viewport = Some(Viewport::new(state));
        }
        self.sync(ui.ctx(), state);
        let pixels = ui.ctx().pixels_per_point();
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // Window pixels.
        let size = [
            (rect.width() * pixels).round() as u32,
            (rect.height() * pixels).round() as u32,
        ];
        // The supports' plate bases and the rafts fade with the plate (owner R9, 2026-11-10).
        self.supports.base_hidden = 1.0 - plate_fade(ui.ctx(), self.camera.eye());
        let (draws, outlined) = self.scene_draws(state);
        let colors = self.appearance;
        let view_projection = self.camera.view_projection(self.aspect);
        let outline = outlined
            .filter(|_| colors.show_selection_outline)
            .map(|draw| self.outline(draw, &view_projection, size, pixels));
        let eye = self.camera.eye();
        let target = self.camera.target;
        // A headlight: faces turned towards the viewer show the object's exact colour.
        #[allow(clippy::cast_possible_truncation)] // A direction.
        let light = std::array::from_fn(|k| (eye[k] - target[k]) as f32);
        // The target stores encoded bytes, so the background is cleared to its sRGB value.
        let clear = colors.background.encoded();
        let section = self.section.section(&mut self.workspace);
        self.configure_viewport(ui.ctx(), &state.device, eye, pixels);
        let Some(viewport) = self.viewport.as_mut() else {
            return;
        };
        if let Some(texture) = viewport.render(
            state,
            size,
            &view_projection,
            #[allow(clippy::cast_possible_truncation)] // GPU precision.
            eye.map(|c| c as f32),
            light,
            section,
            clear,
            &draws,
            outline.as_ref(),
        ) {
            ui.painter().image(
                texture,
                rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
        let pointer = ui.input(|i| i.pointer.hover_pos());
        self.paint_tool_handles(ui, rect);
        manipulate::draw(
            ui.painter(),
            rect,
            &self.camera,
            &handles,
            hovered,
            &self.appearance,
            self.transform.is_locked(self.mode),
        );
        self.view_cube(ui, rect);
        self.region_back(ui, rect);
        // A repair marker under the pointer says what it marks (TD-092 r8), and a cavity marker
        // the region it locates (TD-101).
        let label = match self.workflow.tool() {
            Some(Tool::Repair) => pointer
                .filter(|_| response.hovered())
                .and_then(|p| self.hollowing.repair_marker_at(&self.camera, rect, p)),
            Some(Tool::DrainHoles) => pointer
                .filter(|_| response.hovered())
                .and_then(|p| self.hollowing.cavity_marker_at(&self.camera, rect, p))
                .map(str::to_owned),
            _ => None,
        };
        if let Some(label) = label {
            response.on_hover_text_at_pointer(label);
        }
        self.paint_grid_shape(ui, rect);
        if self.workspace.plate().objects.is_empty() && !self.busy() {
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                tr!("viewport-empty-hint"),
                egui::FontId::proportional(18.0),
                theme::BORDER.color(),
            );
        }
    }

    /// The grid's shape as it is drawn (TD-138): a screen overlay, not scene content.
    fn paint_grid_shape(&self, ui: &egui::Ui, rect: egui::Rect) {
        if let Some(shape) = self.grid.shape() {
            #[allow(clippy::cast_possible_truncation)] // Pixels.
            let points: Vec<egui::Pos2> = shape
                .iter()
                .map(|p| rect.min + egui::vec2(p[0] as f32, p[1] as f32))
                .collect();
            ui.painter().add(egui::Shape::closed_line(
                points,
                egui::Stroke::new(1.5, theme::ACCENT.color()),
            ));
        }
    }

    /// Upload new meshes and rebuild the volume lines and the plate's front tab when the
    /// printer changed.
    fn sync(&mut self, ctx: &egui::Context, state: &egui_wgpu::RenderState) {
        let Some(viewport) = self.viewport.as_mut() else {
            return;
        };
        for loaded in self.workspace.meshes().values() {
            if !viewport.has_mesh(loaded.key) {
                viewport.upload(&state.device, loaded.key, &loaded.mesh, loaded.origin);
            }
        }
        self.preparation.sync(viewport, state);
        self.supports.sync(viewport, state);
        let printer = self.workspace.printer().cloned();
        let look_changed = self.lines_look.as_ref().is_none_or(|(colours, locale)| {
            *colours != self.appearance || *locale != self.launch.locale
        });
        if !self.lines_built || self.lines_printer != printer || look_changed {
            let colors = self.appearance;
            let (grid, edges) = printer.as_ref().map_or_else(Default::default, |p| {
                volume_lines(
                    p,
                    colors.grid.linear(),
                    colors.grid_major.linear(),
                    colors.outline.linear(),
                )
            });
            let (plate, marks) = printer.as_ref().map_or_else(Default::default, |p| {
                plate_surface(
                    p,
                    colors.plate.linear(),
                    colors.plate_border.linear(),
                    colors.front_tab.linear(),
                )
            });
            viewport.set_lines(&state.device, &grid, &edges);
            viewport.set_surface(&state.device, &plate, &marks);
            // The word is laid out large, so its glyphs stay sharp when the view comes close, and
            // the atlas is copied at once, while it holds them.
            let (label, atlas) = ctx.fonts_mut(|fonts| {
                let galley = fonts.layout_no_wrap(
                    tr!("plate-front"),
                    egui::FontId::proportional(FRONT_LABEL_POINTS),
                    egui::Color32::WHITE,
                );
                let tab = printer.as_ref().map(FrontTab::of);
                let label = tab.map_or_else(Vec::new, |tab| {
                    tab.label(
                        &galley,
                        fonts.font_image_size(),
                        colors.front_label.linear(),
                    )
                });
                (label, fonts.image())
            });
            viewport.set_label(state, &atlas, &label);
            if printer.is_some() && self.lines_printer.is_none() {
                self.fit_pending = true;
            }
            self.lines_built = true;
            self.lines_printer = printer;
            self.lines_look = Some((self.appearance, self.launch.locale.clone()));
            if !self.busy() && self.fit_pending {
                self.fit_pending = false;
                self.fit();
            }
        }
    }

    /// Undo the plate's last edit; a drag in progress ends.
    fn undo(&mut self) {
        self.drag = None;
        self.workspace.undo();
        self.keep_selection();
    }

    /// Redo the plate's last undone edit.
    fn redo(&mut self) {
        self.drag = None;
        self.workspace.redo();
        self.keep_selection();
    }

    /// Drop a selection that no longer names an object.
    fn keep_selection(&mut self) {
        if self
            .selected
            .is_some_and(|i| i >= self.workspace.plate().objects.len())
        {
            self.selected = None;
        }
    }

    /// Undo and redo, mode keys, Delete and Escape, unless a text field has the focus (text
    /// fields undo their own typing).
    /// The actions whose shortcuts were pressed, while no widget holds the keyboard focus (a
    /// text field or a value being typed).
    fn keys(&mut self, ui: &egui::Ui) {
        // Nor while a dialog is open: its keys are its own (Escape, Enter).
        if ui.ctx().egui_wants_keyboard_input()
            || ui.ctx().memory(|m| m.top_modal_layer().is_some())
        {
            return;
        }
        // The section's keys wait while another control holds the keyboard: one reached with
        // Tab, or the layer image, which takes the same keys (owner R10, 2026-11-10).
        let focused = ui.ctx().memory(egui::Memory::focused);
        let free = focused.is_none() || focused == self.section.slider_id();
        let pressed = ui.input_mut(|i| {
            self.keymap
                .pressed_where(i, |action| free || !matches!(action, Action::Section(_)))
        });
        for action in pressed {
            self.perform(action);
        }
    }

    /// Whether the tool in use has a list window (owner, 2026-10-29).
    fn tool_has_list(&self) -> bool {
        matches!(
            self.workflow.tool(),
            Some(
                Tool::Automatic
                    | Tool::Supports
                    | Tool::Islands
                    | Tool::Braces
                    | Tool::DrainHoles
                    | Tool::Hollow
            )
        )
    }

    /// Whether `action` can be taken now.
    #[allow(clippy::too_many_lines)] // One arm per action.
    fn enabled(&self, action: Action) -> bool {
        match action {
            Action::Undo => self.workspace.history().0,
            Action::Redo => self.workspace.history().1,
            Action::ExportModels => self.can_export_models(),
            Action::DropToPlate => {
                !self.transform.is_locked(Mode::Move)
                    && self.selected.is_some_and(|index| {
                        self.workspace
                            .plate()
                            .objects
                            .get(index)
                            .is_some_and(transform_ui::is_lifted)
                    })
            }
            Action::Step(step) => !step.tools().is_empty(),
            Action::CenterModel | Action::FitToPlate => {
                let lock = if action == Action::CenterModel {
                    Mode::Move
                } else {
                    Mode::Scale
                };
                self.selected.is_some()
                    && self.workspace.printer().is_some()
                    && !self.transform.is_locked(lock)
            }
            Action::ResetRotation => {
                self.selected.is_some() && !self.transform.is_locked(Mode::Rotate)
            }
            Action::ResetScale => self.selected.is_some() && !self.transform.is_locked(Mode::Scale),
            Action::Hide | Action::Isolate => self.selected.is_some(),
            Action::ShowAll => self.workspace.hidden_count() > 0,
            // The tool strip's (owner, 2026-10-29): with the tool that places, on a model.
            Action::Place => {
                let tool = self.workflow.tool();
                self.selected.is_some()
                    && (matches!(
                        tool,
                        Some(Tool::Supports | Tool::Braces | Tool::Hollow | Tool::Orient)
                    ) || (tool == Some(Tool::DrainHoles)
                        && self.selected.is_some_and(|i| {
                            self.preparation
                                .drainage_unavailable(&self.workspace, i)
                                .is_none()
                        })))
            }
            Action::ToggleSurface => {
                matches!(
                    self.workflow.tool(),
                    Some(Tool::Supports | Tool::DrainHoles)
                ) && self.selected_hollowed()
            }
            Action::ToggleAvoid | Action::ToggleKnee => {
                self.workflow.tool() == Some(Tool::Supports)
            }
            Action::ToggleCavityView => {
                matches!(
                    self.workflow.tool(),
                    Some(Tool::Automatic | Tool::Supports | Tool::Islands | Tool::Braces)
                ) && self.selected_hollowed()
            }
            Action::ToggleLeft => self.workflow.tool() == Some(Tool::Automatic),
            Action::ToggleIslands | Action::ToggleMinima => {
                self.workflow.tool() == Some(Tool::Islands)
            }
            Action::ShowCups => self.workflow.tool() == Some(Tool::DrainHoles),
            Action::SupportIslands => {
                self.workflow.tool() == Some(Tool::Islands) && self.selected.is_some()
            }
            Action::MergeTrunks | Action::GridSupports => {
                self.workflow.tool() == Some(Tool::Supports) && self.selected.is_some()
            }
            Action::JoinSupport => {
                self.workflow.tool() == Some(Tool::Supports)
                    && self.supports.support_selected(self.selected)
            }
            Action::ToggleTips | Action::ToggleMini => self.workflow.tool() == Some(Tool::Supports),
            Action::AutoBraces => {
                self.workflow.tool() == Some(Tool::Braces) && self.selected.is_some()
            }
            Action::ToggleList => self.tool_has_list(),
            Action::Show => matches!(
                self.workflow.tool(),
                Some(Tool::Supports | Tool::Braces | Tool::DrainHoles)
            ),
            Action::NewProject | Action::Open | Action::Save | Action::SaveAs => {
                !self.project_file.busy()
            }
            Action::NewPlate
            | Action::DuplicatePlate
            | Action::RenamePlate
            | Action::DeletePlate
            | Action::PreviousPlate
            | Action::NextPlate
            | Action::TogglePlatesBar
            | Action::PlatesOverview
            | Action::ArrangeModels
            | Action::SliceEveryPlate
            | Action::ExportEveryPlate => self.plate_action_enabled(action),
            Action::RestoreDefaults
            | Action::Section(_)
            | Action::Tool(_)
            | Action::ToggleTools
            | Action::ToggleAxes
            | Action::ToggleLock(_)
            | Action::ToggleUniform
            | Action::ToggleSidebar
            | Action::ToggleMessages
            | Action::ToggleLog
            | Action::ToggleOverhangs
            | Action::OpenSettings
            | Action::EditProfiles
            | Action::About
            | Action::Quit
            | Action::FitView
            | Action::HomeView
            | Action::MoveMode
            | Action::RotateMode
            | Action::ScaleMode
            | Action::Delete
            | Action::Cancel => true,
        }
    }

    /// The Supports tool's strip: placing (one click, or tip then foot), the surface, avoiding
    /// the model, the knee, the overhangs and Delete; `surfaces` labels the surface's options.
    fn supports_strip(
        &self,
        surfaces: &dyn Fn(bool) -> Vec<(String, bool)>,
    ) -> Vec<crate::tool_strip::Item> {
        use crate::tool_strip::Item;
        let cavity = self.supports.on_cavity;
        let two_clicks = self.supports.tip_then_foot;
        let branch = self.supports.branching;
        vec![
            Item::button(
                Action::Place,
                Icon::Supports,
                self.supports.placing(),
                self.enabled(Action::Place),
            )
            .with_options(vec![
                (tr!("supports-place-one-click"), !two_clicks && !branch),
                (tr!("supports-place-tip-then-foot"), two_clicks),
                (tr!("supports-place-branch"), branch),
            ]),
            self.grid_button(),
            Item::button(
                Action::ToggleMini,
                Icon::Mini,
                self.supports.mini,
                self.enabled(Action::ToggleMini),
            ),
            Item::button(
                Action::ToggleSurface,
                if cavity {
                    Icon::SurfaceCavity
                } else {
                    Icon::SurfaceOutside
                },
                cavity,
                self.enabled(Action::ToggleSurface),
            )
            .with_options(surfaces(cavity)),
            Item::button(
                Action::ToggleCavityView,
                Icon::CavityView,
                self.supports.cavity_view,
                self.enabled(Action::ToggleCavityView),
            ),
            Item::button(
                Action::ToggleTips,
                Icon::TipsOnly,
                self.supports.tips_only,
                true,
            ),
            Item::button(
                Action::ToggleAvoid,
                Icon::AvoidModel,
                !self.supports.straight_only,
                true,
            ),
            Item::button(
                Action::ToggleKnee,
                Icon::Knee,
                !self.supports.knee_stays,
                true,
            ),
            Item::Separator,
            Item::button(
                Action::MergeTrunks,
                Icon::MergeTrunks,
                self.supports.merging(),
                self.enabled(Action::MergeTrunks),
            ),
            Item::button(
                Action::JoinSupport,
                Icon::JoinSupport,
                self.supports.joining(),
                self.enabled(Action::JoinSupport),
            ),
            Item::Separator,
            Item::button(
                Action::ToggleOverhangs,
                Icon::Overhangs,
                self.overhangs.shown,
                true,
            ),
            Item::button(
                Action::ToggleList,
                Icon::List,
                self.supports.list_open,
                true,
            ),
            self.supports
                .shown
                .button(&crate::shown::Shown::SUPPORT_CHOICES),
            Item::button(
                Action::Delete,
                Icon::Delete,
                false,
                self.supports.support_selected(self.selected),
            ),
        ]
    }

    /// The Supports tool's Grid button, with how it picks an underside as its options (TD-138).
    fn grid_button(&self) -> crate::tool_strip::Item {
        use crate::grid_ui::plan::PickMode;
        crate::tool_strip::Item::button(
            Action::GridSupports,
            Icon::GridSupports,
            self.grid.active,
            self.enabled(Action::GridSupports),
        )
        .with_options(
            [
                (tr!("grid-pick-face"), PickMode::Face),
                (tr!("grid-pick-rectangle"), PickMode::Rectangle),
                (tr!("grid-pick-lasso"), PickMode::Lasso),
            ]
            .into_iter()
            .map(|(label, mode)| (label, self.grid.mode == mode))
            .collect(),
        )
    }

    /// The Braces tool's strip: placing (one by one or in a chain, as clicked or level), the
    /// automatic braces, the cavity view, the list and Delete.
    fn braces_strip(&self) -> Vec<crate::tool_strip::Item> {
        use crate::tool_strip::Item;
        let b = &self.supports.bracing;
        vec![
            Item::button(
                Action::Place,
                if b.chain {
                    Icon::BracesChain
                } else {
                    Icon::Braces
                },
                self.supports.brace_placing(),
                self.enabled(Action::Place),
            )
            .with_options(vec![
                (tr!("braces-place-single"), !b.chain),
                (tr!("braces-place-chain"), b.chain),
                (tr!("braces-place-as-clicked"), !b.level),
                (tr!("braces-place-level"), b.level),
            ]),
            Item::button(
                Action::AutoBraces,
                Icon::BracesAuto,
                self.supports.auto_bracing(),
                self.enabled(Action::AutoBraces),
            ),
            Item::Separator,
            Item::button(
                Action::ToggleCavityView,
                Icon::CavityView,
                self.supports.cavity_view,
                self.enabled(Action::ToggleCavityView),
            ),
            Item::button(Action::ToggleList, Icon::List, b.list_open, true),
            b.shown.button(&crate::shown::Shown::CHOICES),
            Item::button(
                Action::Delete,
                Icon::Delete,
                false,
                self.supports.brace_selected(self.selected),
            ),
        ]
    }

    /// The Islands tool's strip: the islands, the local minima and the overhangs shown, the
    /// cavity view and the list.
    fn islands_strip(&self) -> Vec<crate::tool_strip::Item> {
        use crate::tool_strip::Item;
        vec![
            Item::button(
                Action::ToggleIslands,
                Icon::Islands,
                self.islands.show_islands,
                true,
            ),
            Item::button(
                Action::ToggleMinima,
                Icon::Minima,
                self.islands.show_minima,
                true,
            ),
            Item::button(
                Action::ToggleOverhangs,
                Icon::Overhangs,
                self.overhangs.shown,
                true,
            ),
            Item::Separator,
            Item::button(
                Action::SupportIslands,
                Icon::SupportIslands,
                self.islands.supporting(),
                self.enabled(Action::SupportIslands),
            ),
            Item::Separator,
            Item::button(
                Action::ToggleCavityView,
                Icon::CavityView,
                self.supports.cavity_view,
                self.enabled(Action::ToggleCavityView),
            ),
            Item::button(Action::ToggleList, Icon::List, self.islands.list_open, true),
        ]
    }

    /// The Orient tool's strip (owner mockup, 2026-11-03): protecting points, removing the last
    /// one, and the overhangs.
    fn orient_strip(&self) -> Vec<crate::tool_strip::Item> {
        use crate::tool_strip::Item;
        let points = self
            .selected
            .is_some_and(|i| !crate::orient_ui::OrientUi::points_of(&self.workspace, i).is_empty());
        vec![
            Item::button(
                Action::Place,
                Icon::Protect,
                self.orient.protecting,
                self.enabled(Action::Place),
            ),
            Item::button(Action::Delete, Icon::Delete, false, points),
            Item::Separator,
            Item::button(
                Action::ToggleOverhangs,
                Icon::Overhangs,
                self.overhangs.shown,
                true,
            ),
        ]
    }

    /// The Automatic tool's strip: the overhangs, the places left unsupported, the cavity view.
    fn auto_strip(&self) -> Vec<crate::tool_strip::Item> {
        use crate::tool_strip::Item;
        vec![
            Item::button(
                Action::ToggleOverhangs,
                Icon::Overhangs,
                self.overhangs.shown,
                true,
            ),
            Item::button(
                Action::ToggleLeft,
                Icon::LeftUnsupported,
                self.auto.show_left,
                true,
            ),
            Item::Separator,
            Item::button(
                Action::ToggleCavityView,
                Icon::CavityView,
                self.supports.cavity_view,
                self.enabled(Action::ToggleCavityView),
            ),
            Item::button(Action::ToggleList, Icon::List, self.auto.list_open, true),
        ]
    }

    /// The Hollow tool's strip (owner mockup, 2026-11-02): placing blockers, moving, turning or
    /// scaling the selected one, the list and Delete.
    fn hollow_strip(&self) -> Vec<crate::tool_strip::Item> {
        use crate::tool_strip::Item;
        let blockers = &self.hollowing.blockers;
        let selected = blockers.has_selection();
        vec![
            Item::button(
                Action::Place,
                Icon::Blockers,
                blockers.placing,
                self.enabled(Action::Place),
            ),
            Item::Separator,
            Item::button(
                Action::MoveMode,
                Icon::Transform,
                selected && self.mode == Mode::Move,
                selected,
            ),
            // A sphere only moves and scales.
            Item::button(
                Action::RotateMode,
                Icon::Rotate,
                selected && self.mode == Mode::Rotate,
                blockers.selected_sphere() == Some(false),
            ),
            Item::button(
                Action::ScaleMode,
                Icon::Scale,
                selected && self.mode == Mode::Scale,
                selected,
            ),
            Item::Separator,
            Item::button(Action::ToggleList, Icon::List, blockers.list_open, true),
            Item::button(Action::Delete, Icon::Delete, false, selected),
        ]
    }

    /// The Drain holes tool's strip: placing, the surface holes start from, the axes the
    /// selected hole's rings turn about, the list, what is shown, Delete and the suction cups.
    fn holes_strip(&mut self) -> Vec<crate::tool_strip::Item> {
        use crate::tool_strip::Item;
        let cavity = self.on_cavity();
        vec![
            Item::button(
                Action::Place,
                Icon::DrainHoles,
                self.hollowing.is_placing(),
                self.enabled(Action::Place),
            ),
            Item::button(
                Action::ToggleSurface,
                if cavity {
                    Icon::SurfaceCavity
                } else {
                    Icon::SurfaceOutside
                },
                cavity,
                self.enabled(Action::ToggleSurface),
            )
            .with_options(vec![
                (tr!("prepare-start-outer"), !cavity),
                (tr!("prepare-start-cavity"), cavity),
            ]),
            // The axes the selected hole's rings turn it about.
            Item::button(
                Action::ToggleAxes,
                Icon::Rotate,
                self.space == Space::Local,
                true,
            )
            .with_options(vec![
                (tr!("transform-global"), self.space == Space::Global),
                (tr!("holes-axes-surface"), self.space == Space::Local),
            ]),
            Item::Separator,
            Item::button(
                Action::ToggleList,
                Icon::List,
                self.hollowing.holes_list_open,
                true,
            ),
            self.hollowing
                .holes_shown
                .button(&crate::shown::Shown::CHOICES),
            Item::button(
                Action::Delete,
                Icon::Delete,
                false,
                self.hollowing.hole_selected(),
            ),
            Item::Separator,
            Item::button(
                Action::ShowCups,
                Icon::SuctionCups,
                self.hollowing.cups.show,
                self.enabled(Action::ShowCups),
            ),
        ]
    }

    /// The tool strip at the viewport's left (owner, 2026-10-29), for the tool in use: its
    /// modes and switches, which the tool's panel no longer holds.
    fn tool_strip(&mut self, ctx: &egui::Context, viewport: egui::Rect) {
        use crate::tool_strip::Picked;
        let surfaces = |cavity: bool| {
            vec![
                (tr!("supports-surface-outer"), !cavity),
                (tr!("supports-surface-cavity"), cavity),
            ]
        };
        let items = match self.workflow.tool() {
            Some(Tool::Supports) => self.supports_strip(&surfaces),
            Some(Tool::Braces) => self.braces_strip(),
            Some(Tool::Islands) => self.islands_strip(),
            Some(Tool::Automatic) => self.auto_strip(),
            Some(Tool::Orient) => self.orient_strip(),
            Some(Tool::Hollow) => self.hollow_strip(),
            Some(Tool::DrainHoles) => self.holes_strip(),
            _ => Vec::new(),
        };
        let picked =
            crate::tool_strip::show(ctx, viewport, &items, &self.keymap, self.icons.as_ref());
        match picked {
            Some(Picked::Action(action)) => self.perform(action),
            // Choosing how braces are placed starts placing them that way: one by one or in a
            // chain, then as clicked or level.
            Some(Picked::Option(Action::Place, index))
                if self.workflow.tool() == Some(Tool::Braces) =>
            {
                if self.enabled(Action::Place) {
                    let (chain, level) = match index {
                        0 | 1 => (Some(index == 1), None),
                        _ => (None, Some(index == 3)),
                    };
                    self.supports.set_brace_options(chain, level);
                }
            }
            // Choosing how supports are placed starts placing them that way.
            Some(Picked::Option(Action::Place, index)) => {
                self.supports.set_tip_then_foot(index == 1);
                self.supports.set_branching(index == 2);
                if !self.supports.placing() && self.enabled(Action::Place) {
                    self.supports.toggle_placing();
                }
            }
            // Choosing how the grid picks an underside starts the Grid mode that way (TD-138).
            Some(Picked::Option(Action::GridSupports, index)) => {
                use crate::grid_ui::plan::PickMode;
                let mode = [PickMode::Face, PickMode::Rectangle, PickMode::Lasso]
                    .get(index)
                    .copied()
                    .unwrap_or_default();
                self.grid.set_mode(mode);
                if !self.grid.active && self.enabled(Action::GridSupports) {
                    self.perform(Action::GridSupports);
                }
            }
            Some(Picked::Option(Action::Show, index)) => {
                let choices: &[crate::shown::Shown] =
                    if self.workflow.tool() == Some(Tool::Supports) {
                        &crate::shown::Shown::SUPPORT_CHOICES
                    } else {
                        &crate::shown::Shown::CHOICES
                    };
                if let (Some(shown), Some(&choice)) = (self.shown_mut(), choices.get(index)) {
                    *shown = choice;
                }
            }
            Some(Picked::Option(Action::ToggleSurface, index)) => {
                if self.enabled(Action::ToggleSurface) {
                    self.set_surface(index == 1);
                }
            }
            Some(Picked::Option(Action::ToggleAxes, index)) => {
                self.space = if index == 1 {
                    Space::Local
                } else {
                    Space::Global
                };
            }
            Some(Picked::Option(..)) | None => {}
        }
    }

    /// The tool strip's Show clicked: every item again, or only those with errors.
    fn toggle_shown(&mut self) {
        if let Some(shown) = self.shown_mut() {
            *shown = shown.toggled();
        }
    }

    /// What the tool in use's Show keeps, for the tools that have one.
    fn shown_mut(&mut self) -> Option<&mut crate::shown::Shown> {
        match self.workflow.tool() {
            Some(Tool::Supports) => Some(&mut self.supports.shown),
            Some(Tool::Braces) => Some(&mut self.supports.bracing.shown),
            Some(Tool::DrainHoles) => Some(&mut self.hollowing.holes_shown),
            _ => None,
        }
    }

    /// Show or hide the tool in use's list window.
    fn toggle_list(&mut self) {
        match self.workflow.tool() {
            Some(Tool::Supports) => self.supports.list_open = !self.supports.list_open,
            Some(Tool::Braces) => {
                self.supports.bracing.list_open = !self.supports.bracing.list_open;
            }
            Some(Tool::Islands) => self.islands.list_open = !self.islands.list_open,
            Some(Tool::Automatic) => self.auto.list_open = !self.auto.list_open,
            Some(Tool::DrainHoles) => {
                self.hollowing.holes_list_open = !self.hollowing.holes_list_open;
            }
            Some(Tool::Hollow) => {
                self.hollowing.blockers.list_open = !self.hollowing.blockers.list_open;
            }
            _ => {}
        }
    }

    /// Whether the tool in use places in the cavity.
    fn on_cavity(&mut self) -> bool {
        match self.workflow.tool() {
            Some(Tool::Supports) => self.supports.on_cavity,
            Some(Tool::DrainHoles) => {
                *self.hollowing.next_hole().2
                    == karasu_core::preparation::sampled::HoleSurface::Cavity
            }
            _ => false,
        }
    }

    /// Place in the cavity, or on the outside, with the tool in use.
    fn set_surface(&mut self, cavity: bool) {
        use karasu_core::preparation::sampled::HoleSurface;
        match self.workflow.tool() {
            Some(Tool::Supports) => self.supports.set_cavity(cavity),
            Some(Tool::DrainHoles) => {
                *self.hollowing.next_hole().2 = if cavity {
                    HoleSurface::Cavity
                } else {
                    HoleSurface::Outer
                };
            }
            _ => {}
        }
    }

    /// Whether the selected model is hollowed: it has a cavity to place in.
    fn selected_hollowed(&self) -> bool {
        self.selected
            .and_then(|i| self.workspace.plate().objects.get(i))
            .is_some_and(|o| o.hollowing().is_some())
    }

    /// Take `action`, if it can be taken now; every action of the interface comes here.
    #[allow(clippy::too_many_lines)] // One arm per action.
    fn perform(&mut self, action: Action) {
        if !self.enabled(action) {
            return;
        }
        log::debug!(target: "app", "action {action:?}");
        match action {
            Action::NewProject => self.ask(project_file::Intent::New),
            Action::Open => self.open_dialog(),
            Action::Save => self.save(),
            Action::SaveAs => self.save_as(),
            Action::ExportModels => self.open_export_models(None),
            Action::NewPlate
            | Action::DuplicatePlate
            | Action::RenamePlate
            | Action::DeletePlate
            | Action::PreviousPlate
            | Action::NextPlate
            | Action::TogglePlatesBar
            | Action::PlatesOverview
            | Action::ArrangeModels
            | Action::SliceEveryPlate
            | Action::ExportEveryPlate => self.perform_plate(action),
            Action::Undo => self.undo(),
            Action::Redo => self.redo(),
            Action::FitView => self.fit(),
            Action::HomeView => {
                let to = self.home_camera();
                self.turn = Some(viewcube::Turn::new(self.camera, to));
            }
            Action::MoveMode => self.mode = Mode::Move,
            Action::RotateMode => self.mode = Mode::Rotate,
            Action::ScaleMode => self.mode = Mode::Scale,
            Action::DropToPlate => {
                if let Some(index) = self.selected {
                    self.workspace.drop_to_plate(index);
                }
            }
            // The selected drain hole, if any, before the object.
            Action::Delete => {
                let orient = self.workflow.tool() == Some(Tool::Orient)
                    && self.selected.is_some_and(|i| {
                        crate::orient_ui::OrientUi::remove_last(&mut self.workspace, i)
                    });
                if orient {
                    return;
                }
                let blocker =
                    self.workflow.tool() == Some(Tool::Hollow) && self.hollowing.blockers.delete();
                if blocker
                    || self.hollowing.delete(&mut self.workspace)
                    || self.supports.delete(&mut self.workspace)
                {
                    return;
                }
                // A model asks first (owner, 2026-10-26): the key may have been meant for a
                // hole that was not selected. The sidebar's bin deletes at once.
                self.deleting = self.selected;
            }
            Action::Hide => {
                if let Some(index) = self.selected {
                    self.change_visibility(|v| v.toggle(index));
                }
            }
            Action::Isolate => {
                if let Some(index) = self.selected {
                    self.change_visibility(|v| v.isolate(index));
                }
            }
            Action::ShowAll => self.change_visibility(Visibility::show_all),
            // First what the drain holes are doing, then the selection.
            Action::Cancel => {
                if self.workflow.tool() == Some(Tool::Orient) && self.orient.escape() {
                    return;
                }
                // Escape stops a shape being drawn, or ends the Grid mode, its plan with it.
                if self.workflow.tool() == Some(Tool::Supports) && self.grid.drawing() {
                    self.grid.cancel_shape();
                    return;
                }
                if self.workflow.tool() == Some(Tool::Supports) && self.grid.active {
                    self.grid.toggle();
                    return;
                }
                let blocker =
                    self.workflow.tool() == Some(Tool::Hollow) && self.hollowing.blockers.escape();
                if !blocker && !self.hollowing.escape() && !self.supports.escape() {
                    self.selected = None;
                }
            }
            Action::Section(step) => self.section.step(&mut self.workspace, step),
            Action::Step(step) => self.workflow.choose_step(step),
            Action::Tool(tool) => {
                self.workflow.choose_tool(tool);
                self.chrome.panel_open = true;
            }
            Action::ToggleTools => self.workflow.toggle_tools(),
            Action::ToggleLock(mode) => self.transform.toggle_lock(mode),
            Action::ToggleUniform => self.transform.uniform ^= true,
            Action::CenterModel
            | Action::FitToPlate
            | Action::ResetRotation
            | Action::ResetScale => {
                if let Some(index) = self.selected {
                    self.transform_model(action, index);
                }
            }
            Action::ToggleAxes => {
                self.space = match self.space {
                    Space::Global => Space::Local,
                    Space::Local => Space::Global,
                };
            }
            Action::ToggleSidebar => self.chrome.sidebar_open = !self.chrome.sidebar_open,
            Action::ToggleMessages => self.messages.toggle(),
            Action::ToggleLog => self.log_window.toggle(),
            Action::ToggleOverhangs => self.overhangs.shown = !self.overhangs.shown,
            Action::Place => match self.workflow.tool() {
                Some(Tool::Supports) => {
                    // Placing and the Grid mode take the clicks in turn.
                    if self.grid.active {
                        self.grid.toggle();
                    }
                    self.supports.toggle_placing();
                }
                Some(Tool::Braces) => self.supports.toggle_brace_placing(),
                Some(Tool::DrainHoles) => self.hollowing.toggle_placing(),
                Some(Tool::Hollow) => {
                    self.hollowing.blockers.placing = !self.hollowing.blockers.placing;
                }
                Some(Tool::Orient) => self.orient.protecting = !self.orient.protecting,
                _ => {}
            },
            Action::ToggleSurface => {
                let cavity = !self.on_cavity();
                self.set_surface(cavity);
            }
            Action::ToggleAvoid => self.supports.straight_only = !self.supports.straight_only,
            Action::ToggleKnee => self.supports.knee_stays = !self.supports.knee_stays,
            Action::ToggleCavityView => self.supports.toggle_cavity_view(),
            Action::ToggleTips => self.supports.tips_only = !self.supports.tips_only,
            Action::ToggleMini => self.supports.toggle_mini(),
            Action::AutoBraces => self.supports.toggle_auto_braces(),
            Action::ToggleIslands => self.islands.show_islands = !self.islands.show_islands,
            Action::ToggleMinima => self.islands.show_minima = !self.islands.show_minima,
            Action::SupportIslands => self.islands.toggle_supporting(),
            Action::ToggleLeft => self.auto.show_left = !self.auto.show_left,
            Action::MergeTrunks => self.supports.toggle_merging(self.selected),
            Action::GridSupports => {
                if self.supports.placing() {
                    self.supports.toggle_placing();
                }
                self.grid.toggle();
            }
            Action::JoinSupport => self.supports.toggle_joining(&self.workspace, self.selected),
            Action::ToggleList => self.toggle_list(),
            Action::Show => self.toggle_shown(),
            Action::ShowCups => self.hollowing.cups.show = !self.hollowing.cups.show,
            Action::RestoreDefaults => self.reset_tool_values(),
            Action::OpenSettings => self.open_settings(),
            Action::EditProfiles => self.profiles.open_library(),
            Action::About => self.about_open = true,
            Action::Quit => self.ask(project_file::Intent::Quit),
        }
    }

    /// The selected object's handles where the object is now, sized for the viewport `rect`.
    fn handles(&mut self, rect: egui::Rect) -> Vec<manipulate::Handle> {
        // The handles belong to the Transform tool (TD-098).
        let Some(index) = self
            .selected
            .filter(|_| self.workflow.tool() == Some(Tool::Transform))
        else {
            return Vec::new();
        };
        let Some((anchor, _)) = self.workspace.anchor(index) else {
            return Vec::new();
        };
        let rotate_deg = self
            .workspace
            .plate()
            .objects
            .get(index)
            .map_or([0.0; 3], |o| o.pose.rotate_deg);
        manipulate::handles(
            self.mode,
            anchor,
            self.handle_size(anchor, rect),
            rotate_deg,
            self.space,
        )
    }

    /// How long handles at `anchor` are, in mm, to show a fixed size in the viewport `rect`.
    fn handle_size(&self, anchor: [f64; 3], rect: egui::Rect) -> f64 {
        let eye = self.camera.eye();
        let distance = (0..3)
            .map(|k| (eye[k] - anchor[k]).powi(2))
            .sum::<f64>()
            .sqrt();
        let per_point =
            2.0 * distance * (self.camera.fov_y / 2.0).tan() / f64::from(rect.height().max(1.0));
        HANDLE_POINTS * per_point
    }

    /// The selected blocker's gizmo, with the Hollow tool, sized for the viewport `rect`.
    fn blocker_handles(&mut self, rect: egui::Rect) -> Vec<manipulate::Handle> {
        let Some(index) = self
            .selected
            .filter(|_| self.workflow.tool() == Some(Tool::Hollow))
        else {
            return Vec::new();
        };
        let blockers = &mut self.hollowing.blockers;
        let Some(anchor) = blockers.anchor(&self.workspace, index) else {
            return Vec::new();
        };
        let size = self.handle_size(anchor, rect);
        self.hollowing
            .blockers
            .handles(&self.workspace, index, self.mode, self.space, size)
    }

    /// The selected drain hole's turn gizmo, with the Drain holes tool, sized for the viewport
    /// `rect`.
    fn hole_rings(&mut self, rect: egui::Rect) -> Vec<manipulate::Handle> {
        if self.workflow.tool() != Some(Tool::DrainHoles) {
            return Vec::new();
        }
        let Some(anchor) = self
            .hollowing
            .ring_anchor(&self.workspace, &mut self.preparation)
        else {
            return Vec::new();
        };
        let size = self.handle_size(anchor, rect);
        self.hollowing
            .rings(&self.workspace, &mut self.preparation, size, self.space)
    }

    /// Start a drag pressed at `origin` on a handle of the selection. Elsewhere the drag orbits
    /// the camera: only handles move objects, so a drag across an object never moves it by
    /// accident.
    fn begin_drag(&mut self, handles: &[manipulate::Handle], origin: [f64; 2], size: [f64; 2]) {
        let ray = self.camera.ray(origin, size);
        let grabbed = manipulate::hit(handles, &self.camera, size, origin, HANDLE_REACH)
            .zip(self.selected)
            .and_then(|(handle, index)| {
                let (anchor, _) = self.workspace.anchor(index)?;
                Some((handles[handle].kind, index, anchor))
            });
        let Some((kind, index, anchor)) = grabbed else {
            return;
        };
        // A locked transform's handles do nothing.
        if self.transform.is_locked(self.mode) {
            return;
        }
        let Some((_, placement)) = self.workspace.anchor(index) else {
            return;
        };
        let Some(object) = self.workspace.plate().objects.get(index).cloned() else {
            return;
        };
        self.selected = Some(index);
        self.drag_key += 1;
        self.drag = Some(Drag {
            kind,
            object: index,
            anchor,
            start_ray: ray,
            start_pointer: origin,
            start: object,
            start_translation: placement.translation_mm,
            space: self.space,
        });
    }

    /// Select, drag objects and handles, orbit, pan and zoom from the pointer. Returns the
    /// handle under the pointer.
    fn interact(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        handles: &[manipulate::Handle],
    ) -> Option<usize> {
        let rect = response.rect;
        let size = [f64::from(rect.width()), f64::from(rect.height())];
        let local = |p: egui::Pos2| [f64::from(p.x - rect.min.x), f64::from(p.y - rect.min.y)];
        let pointer = response
            .hover_pos()
            .or_else(|| response.interact_pointer_pos())
            .map(local);
        let hovered =
            pointer.and_then(|p| manipulate::hit(handles, &self.camera, size, p, HANDLE_REACH));
        // Drain holes follow the pointer while one is placed or dragged.
        let ray = pointer.map(|p| self.camera.ray(p, size));
        self.hollowing.set_snap(ui.input(|i| i.modifiers.shift));
        self.supports.hold_height(ui.input(|i| i.modifiers.shift));
        self.follow_holes(ray.as_ref());
        self.follow_supports(ray.as_ref());
        // A shape drawn for the grid follows the pointer, and is picked on release (TD-138).
        if self.grid.drawing() {
            if let Some(at) = pointer {
                self.grid.extend_shape(at);
            }
            if !ui.input(|i| i.pointer.primary_down()) {
                match self.selected {
                    Some(selected) => self.grid.finish_shape(
                        &self.workspace,
                        (&self.camera, size),
                        (selected, self.supports.on_cavity),
                    ),
                    None => self.grid.cancel_shape(),
                }
            }
        }
        if let Some(ray) = ray.as_ref().filter(|_| self.hollowing.blockers.dragging()) {
            let snap = ui.input(|i| i.modifiers.shift);
            self.hollowing.blockers.drag_to(ray, snap);
        }
        if response.drag_started_by(egui::PointerButton::Primary) {
            let press = ui.input(|i| i.pointer.press_origin());
            let on_hole = match (self.selected, press) {
                (Some(selected), Some(at)) => {
                    let ray = self.camera.ray(local(at), size);
                    self.tool_press(rect, at, &ray, selected)
                }
                _ => false,
            };
            if !on_hole {
                if let Some(origin) = press.map(local) {
                    self.begin_drag(handles, origin, size);
                    // Orbiting: about what is seen in the middle, or else under the pointer.
                    if self.drag.is_none()
                        && !self.hollowing.dragging()
                        && !self.hollowing.blockers.dragging()
                        && !self.supports.dragging()
                    {
                        self.retarget([[size[0] / 2.0, size[1] / 2.0], origin], size);
                    }
                }
            }
        }
        if response.drag_started_by(egui::PointerButton::Secondary)
            || response.drag_started_by(egui::PointerButton::Middle)
        {
            // Panning: what is under the pointer follows it.
            if let Some(origin) = ui.input(|i| i.pointer.press_origin()).map(local) {
                self.retarget([origin], size);
            }
        }
        self.camera_drag(ui, response, size);
        let on_cube = response
            .interact_pointer_pos()
            .is_some_and(|p| viewcube::covers(rect, p));
        if response.clicked() && !on_cube {
            let ray = pointer.map(|p| self.camera.ray(p, size));
            let on_marker = response
                .interact_pointer_pos()
                .is_some_and(|at| self.click_marker(rect, at));
            // A support's handle clicked shows its gizmo, without a drag.
            let on_handle = self.workflow.tool() == Some(Tool::Supports)
                && self
                    .selected
                    .zip(response.interact_pointer_pos())
                    .is_some_and(|(selected, at)| {
                        self.supports
                            .touch(&self.workspace, &self.camera, rect, at, selected)
                    });
            let taken = match (self.selected, &ray) {
                _ if on_marker || on_handle => true,
                (Some(selected), Some(ray)) => self.tool_click(ray, selected),
                _ => false,
            };
            if let (false, Some(p)) = (taken, pointer) {
                let ray = self.camera.ray(p, size);
                self.selected = self.workspace.hit(&ray).map(|h| h.object);
            }
        }
        // A right click on a model opens its menu (the sidebar row's).
        let right_hit = (response.secondary_clicked() && !on_cube)
            .then(|| self.model_at(pointer?, size))
            .flatten();
        self.model_menu(ui, response, right_hit);
        if response.hovered() {
            self.wheel_zoom(ui, pointer, size);
        }
        hovered
    }

    /// Follow a drag of `response` over the `size` viewport: a grabbed handle moves its object,
    /// a tool's own item follows itself, and otherwise the camera orbits or pans. The drag ends
    /// when it stops.
    fn camera_drag(&mut self, ui: &egui::Ui, response: &egui::Response, size: [f64; 2]) {
        let rect = response.rect;
        let local = |p: egui::Pos2| [f64::from(p.x - rect.min.x), f64::from(p.y - rect.min.y)];
        let pointer = response
            .hover_pos()
            .or_else(|| response.interact_pointer_pos())
            .map(local);
        let delta = response.drag_delta();
        let (dx, dy) = (f64::from(delta.x), f64::from(delta.y));
        if response.dragged_by(egui::PointerButton::Primary) {
            match (&self.drag, pointer) {
                // A dragged hole or support handle follows the pointer (above); the camera
                // stays.
                _ if self.hollowing.dragging()
                    || self.hollowing.blockers.dragging()
                    || self.supports.dragging()
                    || self.grid.drawing() => {}
                (Some(drag), Some(p)) => {
                    let snap = ui.input(|i| i.modifiers.shift);
                    let ray = self.camera.ray(p, size);
                    if let Some(object) = manipulate::apply(drag, &self.camera, size, &ray, p, snap)
                    {
                        let index = drag.object;
                        self.workspace.place_object(
                            index,
                            object.translate_mm,
                            object.pose,
                            self.drag_key,
                        );
                        self.workspace.settle(index, self.drag_key);
                    }
                }
                (Some(_), None) => {}
                (None, _) => {
                    self.turn = None;
                    self.camera.orbit(dx, dy);
                }
            }
        } else if response.dragged_by(egui::PointerButton::Secondary)
            || response.dragged_by(egui::PointerButton::Middle)
        {
            self.turn = None;
            self.camera.pan(dx, dy, size[1].max(1.0));
        }
        if response.drag_stopped() {
            self.drag = None;
            self.hollowing.release(&mut self.workspace);
            self.hollowing.blockers.release();
            self.supports.release(&mut self.workspace);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::slicing_ui::SliceState;

    #[test]
    fn settings_applied_to_the_window_are_collected_back_as_they_were() {
        let ctx = egui::Context::default();
        let mut settings = crate::settings::Settings::default();
        settings.appearance.show_selection_outline = false;
        settings.appearance.brightness_percent = 120;
        settings.appearance.ambient_percent = 50;
        settings.repair_on_import = crate::settings::ImportRepair::Aggressive;
        settings.window = crate::settings::WindowSettings {
            open_as: crate::settings::OpenAs::Maximized,
            state: crate::settings::WindowState::Windowed,
            size: Some([1000.0, 700.0]),
            position: Some([40.0, 30.0]),
        };
        let layout = &mut settings.layout;
        layout.sidebar_open = false;
        layout.panel_open = false;
        layout.tools_shown = false;
        layout.sections.insert(PLATE_SECTION.to_owned(), false);
        layout.sections.insert(MODELS_SECTION.to_owned(), true);
        layout
            .dialogs
            .insert("library-window".to_owned(), [950.0, 610.0]);
        layout.windows.insert(
            "messages-window".to_owned(),
            crate::settings::WindowPlace {
                position: [10.0, 20.0],
                size: [500.0, 300.0],
            },
        );
        let tools = &mut settings.tools;
        tools.transform_local = true;
        tools.transform_locks = [true, false, true];
        tools.transform_uniform = false;
        tools.repair_advanced = true;
        tools.hollow_advanced = true;
        tools.hollow_wall_mm = 1.25;
        tools.hollow_cell_mm = Some(0.4);
        tools.holes_advanced = true;
        tools.hole_diameter_mm = 2.5;
        tools.hole_depth_mm = Some(4.0);
        tools.holes_from_cavity = true;
        tools.slice_advanced = true;
        tools.verify_walls = true;
        tools.layers_follow = false;
        tools.speeds_per_minute = true;
        tools.show_overhangs = false;
        tools.overhang_angle_deg = 30.0;
        tools.supports_advanced = true;
        tools.knee_follows_foot = false;
        tools.avoid_model = false;
        tools.support_presets = vec![crate::supports_ui::SupportPreset {
            name: "Fine".to_owned(),
            values: crate::supports_ui::SupportValues::of(crate::supports_ui::Weight::Light),
        }];
        tools.support_preset = Some("Fine".to_owned());
        tools.auto_cavity_preset = Some("Fine".to_owned());
        tools.auto_supports.cavity_own = true;
        tools.auto_supports.cavity_weight = crate::supports_ui::Weight::Light;
        tools.auto_supports.cavity_density_pct = 60.0;
        settings.appearance.show_contact_line = false;
        settings.appearance.contact_line = crate::appearance::Rgba::opaque([1, 2, 3]);
        tools.support_weight = crate::supports_ui::Weight::Heavy;
        tools.supports_list_open = false;
        tools.holes_list_open = false;
        tools.raft_advanced = true;
        tools.raft_thickness_mm = 1.5;
        tools.raft_margin_mm = 2.0;
        tools.raft_slope_deg = 45.0;
        tools.raft_fillet_diameter_mm = 2.5;
        tools.raft_fillet_height_mm = 0.0;
        tools.raft_join_mm = Some(15.0);
        tools.raft_shape = karasu_core::supports::RaftShape::Web;
        tools.raft_bar_width_mm = 0.8;
        tools.raft_bar_height_mm = 0.6;
        tools.raft_bar_longest_mm = 15.0;
        tools.supports_tips_only = true;
        settings.shortcuts.insert(
            Action::FitView.id(),
            vec![crate::actions::KeySetting {
                key: "D".into(),
                command: true,
                shift: false,
                alt: false,
            }],
        );
        settings.language = Some(crate::i18n::PSEUDO.to_owned());
        let mut window = MainWindow::new(Vec::new());
        window.apply_settings(&ctx, &settings);
        assert_eq!(window.collect_settings(&ctx), settings);
        // Defaults collect as defaults: a fresh window saves nothing new.
        let mut fresh = MainWindow::new(Vec::new());
        let ctx = egui::Context::default();
        assert_eq!(
            fresh.collect_settings(&ctx),
            crate::settings::Settings::default()
        );
    }

    #[test]
    fn showing_another_plate_starts_its_interfaces_afresh_and_keeps_each_plate_s_slicing() {
        // TD-135: the selection and the plate's interfaces follow the plate shown; the tools'
        // values stay; each plate's slicing waits for it.
        let ctx = egui::Context::default();
        let mut window = MainWindow::new(Vec::new());
        window.follow_plate(&ctx);
        let first = window.workspace.plate_id();
        *window.hollowing.next_hole().0 = 6.0;
        crate::slicing_ui::tests::fail_for_test(&mut window.slicing);
        window.selected = Some(2);
        assert!(!window.enabled(Action::DeletePlate));
        window.perform(Action::NewPlate);
        window.follow_plate(&ctx);
        let second = window.workspace.plate_id();
        assert_ne!(second, first);
        assert_eq!(
            window.workspace.plate().name,
            tr!("plate-default-name", number = 2usize)
        );
        assert_eq!(window.selected, None);
        assert_eq!(*window.hollowing.next_hole().0, 6.0);
        assert_eq!(window.slicing.slice_state(), SliceState::None);
        assert!(window.enabled(Action::DeletePlate));
        // Back to the first plate: its failed slice is said again.
        window.perform(Action::PreviousPlate);
        window.follow_plate(&ctx);
        assert_eq!(window.workspace.plate_id(), first);
        assert_eq!(window.slicing.slice_state(), SliceState::Failed);
        // Undo the new plate: the plate shown stays the first, and the second's slicing goes.
        window.perform(Action::Undo);
        window.follow_plate(&ctx);
        assert_eq!(window.workspace.plates().len(), 1);
        assert!(window.plates.slicings.is_empty());
        // Folding the bar is a setting kept across restarts.
        window.perform(Action::TogglePlatesBar);
        assert!(!window.collect_settings(&ctx).layout.plates_bar_open);
    }

    #[test]
    fn the_plates_bar_shows_every_plate_open_or_folded() {
        // TD-135: cards with the name, the models and the slice's state; folded, tabs.
        let ctx = egui::Context::default();
        let mut window = MainWindow::new(Vec::new());
        window.perform(Action::NewPlate);
        let drawn = |window: &mut MainWindow| -> Vec<String> {
            let mut texts = Vec::new();
            for _ in 0..2 {
                let mut output = ctx.run_ui(egui::RawInput::default(), |ui| window.plates_bar(ui));
                output.textures_delta.clear();
                texts = output
                    .shapes
                    .iter()
                    .filter_map(|s| match &s.shape {
                        egui::epaint::Shape::Text(t) => Some(t.galley.text().to_owned()),
                        _ => None,
                    })
                    .collect();
            }
            texts
        };
        let names: Vec<String> = window
            .workspace
            .plates()
            .iter()
            .map(|p| p.content().name.clone())
            .collect();
        let open = drawn(&mut window);
        for expected in names.iter().cloned().chain([
            tr!("plates-models", count = 0usize),
            tr!("plates-state-none"),
        ]) {
            assert!(open.contains(&expected), "{expected} in {open:?}");
        }
        window.perform(Action::TogglePlatesBar);
        let folded = drawn(&mut window);
        for name in &names {
            assert!(folded.contains(name), "{name} in {folded:?}");
        }
        assert!(!folded.contains(&tr!("plates-state-none")));
    }

    #[test]
    fn the_overview_shows_every_plate_with_its_last_slice_and_the_totals() {
        // TD-135, phase 2: one card per plate with its name, profiles and slice; the totals
        // count the plates sliced as they are, whichever plate is shown.
        let ctx = egui::Context::default();
        let folder =
            std::env::temp_dir().join(format!("karasu-window-overview-{}", std::process::id()));
        let mut window = MainWindow::new(Vec::new());
        window.workspace = crate::slicing_ui::tests::hollowed_cube(&folder);
        window.slicing = crate::slicing_ui::tests::slicing_into(&folder);
        window.follow_plate(&ctx);
        window.slicing.start(&window.workspace);
        crate::slicing_ui::tests::finish(&mut window.slicing);
        let layers = window.slicing.slice_facts().expect("sliced").layers;
        window.perform(Action::NewPlate);
        window.follow_plate(&ctx);
        window.perform(Action::PlatesOverview);
        let drawn = |window: &mut MainWindow| -> Vec<String> {
            let mut texts = Vec::new();
            for _ in 0..3 {
                let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                    window.plates_overview(&ui.ctx().clone());
                });
                output.textures_delta.clear();
                texts = output
                    .shapes
                    .iter()
                    .filter_map(|s| match &s.shape {
                        egui::epaint::Shape::Text(t) => Some(t.galley.text().to_owned()),
                        _ => None,
                    })
                    .collect();
            }
            texts
        };
        let texts = drawn(&mut window);
        let plates = window.workspace.plates();
        let first = plates[0].content();
        for expected in [
            tr!("plates-overview-title", count = 2usize),
            tr!("plates-overview-name", number = 1usize, name = &first.name),
            tr!(
                "plates-overview-name",
                number = 2usize,
                name = &plates[1].content().name
            ),
            tr!(
                "plates-overview-profiles",
                printer = &first.printer.as_ref().unwrap().name,
                resin = &first.resin.as_ref().unwrap().name,
                setting = &first.setting().unwrap().name
            ),
            tr!("plates-state-sliced"),
            tr!("plates-state-none"),
            Action::NewPlate.label(),
        ] {
            assert!(texts.contains(&expected), "{expected} in {texts:?}");
        }
        assert!(
            texts.iter().any(|t| t.starts_with(&format!("{layers} · "))),
            "{layers} layers in {texts:?}"
        );
        let totals = texts
            .iter()
            .find(|t| t.contains(" of 2 sliced"))
            .expect("the totals");
        assert!(totals.contains("1 model · 1 of 2 sliced · "), "{totals}");
        assert!(totals.ends_with(" ml"), "{totals}");
        // The first plate's slice is of the plate as it is: it can be exported from its card,
        // the new plate's cannot.
        let first = window.workspace.plates()[0].id();
        let second = window.workspace.plate_id();
        assert!(window.current_facts(first).is_some());
        assert!(window.current_facts(second).is_none());
        // An edit of the first plate leaves its slice out of date: no longer counted.
        window.workspace.show_plate(first);
        window.follow_plate(&ctx);
        window
            .workspace
            .edit(|plate| plate.objects[0].translate_mm[0] += 1.0);
        assert!(window.current_facts(first).is_none());
        let texts = drawn(&mut window);
        assert!(texts.contains(&tr!("plates-state-stale")), "{texts:?}");
        assert!(
            texts.iter().any(|t| t.contains("0 of 2 sliced")),
            "{texts:?}"
        );
        window.perform(Action::PlatesOverview);
        assert!(!window.plates.overview);
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_new_plate_takes_a_free_name_and_a_plate_with_models_asks_before_it_goes() {
        // Owner, 2026-11-05: deleting a plate and adding one gave two "Plate 2"; a plate with
        // models is deleted once confirmed, an empty one at once.
        let ctx = egui::Context::default();
        let folder =
            std::env::temp_dir().join(format!("karasu-window-plate-names-{}", std::process::id()));
        let mut window = MainWindow::new(Vec::new());
        window.workspace = crate::slicing_ui::tests::hollowed_cube(&folder);
        window.follow_plate(&ctx);
        let names = |window: &MainWindow| -> Vec<String> {
            window
                .workspace
                .plates()
                .iter()
                .map(|p| p.content().name.clone())
                .collect()
        };
        let first = window.workspace.plate_id();
        window.perform(Action::NewPlate);
        let second = window.workspace.plate_id();
        assert_eq!(
            names(&window)[1],
            tr!("plate-default-name", number = 2usize)
        );
        // The empty plate goes at once.
        window.perform(Action::DeletePlate);
        assert!(window.workspace.project().plate(second).is_none());
        assert!(window.plates.deleting.is_none());
        // A plate named as the next number would be is skipped.
        window
            .workspace
            .rename_plate(tr!("plate-default-name", number = 2usize));
        window.perform(Action::NewPlate);
        assert_eq!(
            names(&window)[1],
            tr!("plate-default-name", number = 3usize)
        );
        // The plate with a model waits for the answer; Escape keeps it.
        window.workspace.show_plate(first);
        window.perform(Action::DeletePlate);
        assert_eq!(window.plates.deleting, Some(first));
        assert_eq!(window.workspace.plates().len(), 2);
        let escape = egui::RawInput {
            events: vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            ..Default::default()
        };
        for input in [egui::RawInput::default(), escape] {
            let mut output = ctx.run_ui(input, |ui| window.confirm_plate_deletion(ui.ctx()));
            output.textures_delta.clear();
        }
        assert!(window.plates.deleting.is_none());
        assert!(window.workspace.project().plate(first).is_some());
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_model_moved_to_another_plate_leaves_the_selection_and_says_where_it_went() {
        // TD-135, phase 3: from the row's menu or a drop on a plate.
        let ctx = egui::Context::default();
        let folder =
            std::env::temp_dir().join(format!("karasu-window-transfer-{}", std::process::id()));
        let mut window = MainWindow::new(Vec::new());
        window.workspace = crate::slicing_ui::tests::hollowed_cube(&folder);
        window.follow_plate(&ctx);
        let first = window.workspace.plate_id();
        let model = window.workspace.plate().objects[0].name.clone();
        window.perform(Action::NewPlate);
        window.follow_plate(&ctx);
        let second = window.workspace.plate_id();
        let second_name = window.workspace.plate().name.clone();
        window.workspace.show_plate(first);
        window.follow_plate(&ctx);
        // A copy keeps the selection.
        window.selected = Some(0);
        window.transfer_object(0, Some(second), true);
        assert_eq!(window.selected, Some(0));
        assert_eq!(
            window.messages.latest().unwrap().text,
            tr!("plates-copied", model = &model, plate = &second_name)
        );
        // A move takes the object out of the selection; to a new plate too.
        window.transfer_object(0, None, false);
        assert_eq!(window.selected, None);
        assert_eq!(window.workspace.plate().objects.len(), 0);
        let third = &window.workspace.plates()[1];
        assert_eq!(third.content().objects.len(), 1);
        assert_eq!(
            window.messages.latest().unwrap().text,
            tr!(
                "plates-moved",
                model = &model,
                plate = &third.content().name
            )
        );
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn panel_values_stay_until_restored_and_untouched_ones_follow_new_defaults() {
        // Owner, 2026-11-02: a tool's values are the user's while they work, never saved as
        // defaults, back to the defaults on request; a changed default carries only to values
        // still at the old one.
        let ctx = egui::Context::default();
        let mut settings = crate::settings::Settings::default();
        settings.tools.hollow_wall_mm = 1.25;
        settings.tools.hole_diameter_mm = 2.5;
        settings.tools.support_weight = crate::supports_ui::Weight::Heavy;
        let mut window = MainWindow::new(Vec::new());
        window.apply_settings(&ctx, &settings);
        assert_eq!(window.hollowing.new_hollowing().wall_mm, 1.25);
        assert_eq!(*window.hollowing.next_hole().0, 2.5);
        // Changed in the panels: kept, and never saved as defaults.
        *window.hollowing.next_hole().0 = 6.0;
        window.supports.next =
            crate::supports_ui::NextSupport::of(crate::supports_ui::Weight::Light);
        assert_eq!(window.collect_settings(&ctx), settings);
        // Defaults changed in Settings: the wall, untouched, follows; the hole stays the user's.
        let old = window.default_tool_values();
        window.tool_defaults.wall_mm = 1.5;
        window.tool_defaults.hole_diameter_mm = 3.5;
        window.follow_defaults(&old);
        assert_eq!(window.hollowing.new_hollowing().wall_mm, 1.5);
        assert_eq!(*window.hollowing.next_hole().0, 6.0);
        // Restore all default values.
        window.perform(Action::RestoreDefaults);
        assert_eq!(*window.hollowing.next_hole().0, 3.5);
        assert_eq!(
            window.supports.next,
            crate::supports_ui::NextSupport::of(crate::supports_ui::Weight::Heavy)
        );
    }

    #[test]
    fn the_window_saves_its_settings_and_never_writes_over_a_damaged_file() {
        let folder =
            std::env::temp_dir().join(format!("karasu-window-settings-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        let path = folder.join(crate::settings::FILE_NAME);
        let ctx = egui::Context::default();
        let mut window = MainWindow::new(Vec::new());
        window.load_settings(&ctx, Some(path.clone()));
        assert!(window.messages.latest().is_none());
        window.chrome.sidebar_open = false;
        window.persist(true);
        let (_, read, _) = crate::settings::Store::open(path.clone());
        assert!(!read.layout.sidebar_open);
        // A new window starts as the last one closed.
        let mut next = MainWindow::new(Vec::new());
        next.load_settings(&egui::Context::default(), Some(path.clone()));
        assert!(!next.chrome.sidebar_open);
        // A damaged file: reported, the defaults apply, and it is left as it is.
        std::fs::write(&path, b"{ damaged").unwrap();
        let mut damaged = MainWindow::new(Vec::new());
        damaged.load_settings(&egui::Context::default(), Some(path.clone()));
        assert_eq!(
            damaged.messages.latest().map(|s| s.text),
            Some(tr!("settings-unreadable"))
        );
        assert!(damaged.chrome.sidebar_open);
        damaged.chrome.panel_open = false;
        damaged.persist(true);
        assert_eq!(std::fs::read(&path).unwrap(), b"{ damaged");
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn hiding_deselects_and_isolating_from_the_list_selects() {
        let folder =
            std::env::temp_dir().join(format!("karasu-window-hide-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let mut window = MainWindow::new(Vec::new());
        for name in ["a.stl", "b.stl"] {
            let model = folder.join(name);
            std::fs::write(&model, crate::workspace::tests::box_stl([0.0; 3], [8.0; 3])).unwrap();
            window.workspace.open(model);
            crate::workspace::tests::wait(&mut window.workspace);
        }
        // Nothing hidden: Show all is off; Hide needs a selection.
        assert!(!window.enabled(Action::ShowAll));
        assert!(!window.enabled(Action::Hide));
        window.selected = Some(0);
        window.perform(Action::Hide);
        assert!(window.workspace.is_hidden(0));
        assert_eq!(window.selected, None);
        window.perform(Action::ShowAll);
        assert_eq!(window.workspace.hidden_count(), 0);
        // Isolate from the list: the other hides and the isolated one is selected; the
        // isolated object's delete keeps the other's flag right.
        window.selected = None;
        window.change_visibility(|v| v.isolate(1));
        window.selected = Some(1);
        assert!(window.workspace.is_hidden(0) && !window.workspace.is_hidden(1));
        window.delete_object(1);
        assert_eq!(window.workspace.plate().objects.len(), 1);
        assert!(
            !window.workspace.is_hidden(0),
            "the isolation ended with its object"
        );
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn delete_all_asks_then_deletes_a_tools_items_as_one_step() {
        let folder =
            std::env::temp_dir().join(format!("karasu-window-clear-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let model = folder.join("cube.stl");
        std::fs::write(&model, crate::workspace::tests::box_stl([0.0; 3], [8.0; 3])).unwrap();
        let mut window = MainWindow::new(Vec::new());
        window.workspace.open(model);
        crate::workspace::tests::wait(&mut window.workspace);
        window.selected = Some(0);
        // Two supports and a brace between them, written as the tools write them.
        let mut document =
            karasu_core::supports::SupportDocument::new(karasu_core::supports::Preset::Medium);
        for id in [1, 2] {
            document
                .supports
                .push(karasu_core::supports::ManualSupport {
                    id,
                    tip_mm: [2.0 * f64::from(u8::try_from(id).unwrap()), 4.0, 0.0],
                    tip_axis: karasu_core::supports::AxisSpec::Normal,
                    bends: Vec::new(),
                    base: karasu_core::supports::BaseSpec::Vertical,
                    parameters: karasu_core::supports::Preset::Medium.parameters(),
                    accept_collision: false,
                    origin: karasu_core::supports::SupportOrigin::Placed,
                    kind: karasu_core::supports::SupportKind::Standard,
                });
        }
        document.braces.push(karasu_core::supports::Brace {
            id: 3,
            from: (1, 2.0),
            to: (2, 2.0),
            diameter_mm: Some(0.8),
            max_diameter_mm: None,
        });
        window.workspace.set_supports(0, Some(document), None);
        assert!(window.workspace.supports(0).is_some());
        let ctx = egui::Context::default();
        let key = |key: egui::Key| egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        let frame = |window: &mut MainWindow, events: Vec<egui::Event>| {
            let input = egui::RawInput {
                events,
                ..egui::RawInput::default()
            };
            let mut output = ctx.run_ui(input, |ui| window.confirm_clearing(ui.ctx()));
            output.textures_delta.clear();
        };
        // An emptied document is none: its counts are zero.
        let document = |window: &MainWindow| {
            window.workspace.supports(0).cloned().unwrap_or_else(|| {
                karasu_core::supports::SupportDocument::new(karasu_core::supports::Preset::Medium)
            })
        };
        assert_eq!(
            window.deletable(Tool::Braces, Some(0)).map(|(n, _)| n),
            Some(1)
        );
        // Escape keeps the braces; Enter deletes them, the supports kept.
        window.clearing = Some((Tool::Braces, 0));
        frame(&mut window, Vec::new());
        frame(&mut window, vec![key(egui::Key::Escape)]);
        assert_eq!((document(&window).braces.len(), window.clearing), (1, None));
        window.clearing = Some((Tool::Braces, 0));
        frame(&mut window, Vec::new());
        frame(&mut window, vec![key(egui::Key::Enter)]);
        assert_eq!(document(&window).braces.len(), 0);
        assert_eq!(document(&window).supports.len(), 2);
        // Undo brings them back; all supports go with their braces, as one step.
        window.workspace.undo();
        window.clearing = Some((Tool::Supports, 0));
        frame(&mut window, Vec::new());
        frame(&mut window, vec![key(egui::Key::Enter)]);
        assert_eq!(document(&window).supports.len(), 0);
        assert_eq!(document(&window).braces.len(), 0);
        window.workspace.undo();
        assert_eq!(document(&window).supports.len(), 2);
        assert_eq!(document(&window).braces.len(), 1);
        // Nothing to delete: no dialog waits.
        assert_eq!(
            window.deletable(Tool::DrainHoles, Some(0)).map(|(n, _)| n),
            Some(0)
        );
        window.clearing = Some((Tool::DrainHoles, 0));
        frame(&mut window, Vec::new());
        assert_eq!(window.clearing, None);
        assert_eq!(window.deletable(Tool::Raft, Some(0)), None);
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn the_section_keys_move_its_handle_without_the_slider_focused() {
        let folder =
            std::env::temp_dir().join(format!("karasu-window-section-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let model = folder.join("cube.stl");
        std::fs::write(&model, crate::workspace::tests::box_stl([0.0; 3], [8.0; 3])).unwrap();
        let mut window = MainWindow::new(Vec::new());
        window.workspace.open(model);
        crate::workspace::tests::wait(&mut window.workspace);
        let ctx = egui::Context::default();
        let key = |key: egui::Key| egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        let frame = |window: &mut MainWindow, events: Vec<egui::Event>, focus: Option<egui::Id>| {
            let input = egui::RawInput {
                events,
                ..egui::RawInput::default()
            };
            let mut output = ctx.run_ui(input, |ui| {
                if let Some(id) = focus {
                    ui.memory_mut(|m| m.request_focus(id));
                }
                window.keys(ui);
            });
            output.textures_delta.clear();
        };
        // Nothing holds the keyboard: Page Down takes the upper handle ten layers down, Page Up
        // back to the top.
        assert_eq!(window.section.view(), [0.0, 1.0]);
        frame(&mut window, vec![key(egui::Key::PageDown)], None);
        let [low, high] = window.section.view();
        assert!(low == 0.0 && high < 1.0, "{high}");
        frame(&mut window, vec![key(egui::Key::PageUp)], None);
        assert_eq!(window.section.view(), [0.0, 1.0]);
        // Another control holds it (reached with Tab): the keys are its own.
        let other = egui::Id::new("another control");
        frame(&mut window, Vec::new(), Some(other));
        frame(&mut window, vec![key(egui::Key::PageDown)], Some(other));
        assert_eq!(window.section.view(), [0.0, 1.0]);
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_right_click_on_a_model_selects_it_and_opens_its_menu() {
        let folder =
            std::env::temp_dir().join(format!("karasu-window-menu-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let model = folder.join("cube.stl");
        std::fs::write(&model, crate::workspace::tests::box_stl([0.0; 3], [8.0; 3])).unwrap();
        let mut window = MainWindow::new(Vec::new());
        window.workspace.open(model);
        crate::workspace::tests::wait(&mut window.workspace);
        let ctx = egui::Context::default();
        let at = egui::pos2(200.0, 200.0);
        let press = |button, pressed| {
            vec![
                egui::Event::PointerMoved(at),
                egui::Event::PointerButton {
                    pos: at,
                    button,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ]
        };
        // One frame of a viewport-sized area answering clicks, `hit` the model under the pointer.
        let frame = |window: &mut MainWindow, events: Vec<egui::Event>, hit: Option<usize>| {
            let mut menu_id = egui::Id::NULL;
            let input = egui::RawInput {
                events,
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(800.0, 600.0),
                )),
                ..egui::RawInput::default()
            };
            let mut output = ctx.run_ui(input, |ui| {
                let response = ui.allocate_rect(ui.max_rect(), egui::Sense::CLICK);
                menu_id = ui.id().with("model-menu");
                let hit = hit.filter(|_| response.secondary_clicked());
                window.model_menu(ui, &response, hit);
            });
            output.textures_delta.clear();
            menu_id
        };
        // The area is laid out and the pointer over it before the clicks.
        frame(&mut window, vec![egui::Event::PointerMoved(at)], None);
        // On the model: selected, its menu open.
        frame(
            &mut window,
            press(egui::PointerButton::Secondary, true),
            Some(0),
        );
        frame(
            &mut window,
            press(egui::PointerButton::Secondary, false),
            Some(0),
        );
        let menu_id = frame(&mut window, Vec::new(), None);
        assert_eq!(window.selected, Some(0));
        assert!(egui::Popup::is_id_open(&ctx, menu_id));
        // A left click closes it.
        frame(&mut window, press(egui::PointerButton::Primary, true), None);
        frame(
            &mut window,
            press(egui::PointerButton::Primary, false),
            None,
        );
        let menu_id = frame(&mut window, Vec::new(), None);
        assert!(!egui::Popup::is_id_open(&ctx, menu_id));
        // A right click beside every model opens nothing and keeps the selection.
        frame(
            &mut window,
            press(egui::PointerButton::Secondary, true),
            None,
        );
        frame(
            &mut window,
            press(egui::PointerButton::Secondary, false),
            None,
        );
        let menu_id = frame(&mut window, Vec::new(), None);
        assert!(!egui::Popup::is_id_open(&ctx, menu_id));
        assert_eq!(window.selected, Some(0));
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn checked_models_are_hidden_shown_and_deleted_together() {
        let folder =
            std::env::temp_dir().join(format!("karasu-window-checked-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let model = folder.join("cube.stl");
        std::fs::write(&model, crate::workspace::tests::box_stl([0.0; 3], [8.0; 3])).unwrap();
        let mut window = MainWindow::new(Vec::new());
        for _ in 0..4 {
            window.workspace.open(model.clone());
            crate::workspace::tests::wait(&mut window.workspace);
        }
        let ctx = egui::Context::default();
        // Every model checked, then the second cleared: 0, 2 and 3 checked.
        window.sidebar_request(sidebar::Request::CheckAll(true));
        window.sidebar_request(sidebar::Request::Check(1, false));
        assert_eq!(
            window.checked.iter().copied().collect::<Vec<_>>(),
            [0, 2, 3]
        );
        // Hidden together, then shown again; the second untouched.
        window.sidebar_request(sidebar::Request::HideChecked(true));
        let hidden: Vec<bool> = (0..4).map(|k| window.workspace.is_hidden(k)).collect();
        assert_eq!(hidden, [true, false, true, true]);
        window.sidebar_request(sidebar::Request::HideChecked(false));
        assert_eq!(window.workspace.hidden_count(), 0);
        // A model deleted on its own: the checks after it move up with their models.
        window.delete_object(0);
        assert_eq!(window.checked.iter().copied().collect::<Vec<_>>(), [1, 2]);
        // Deleted together once confirmed (Enter), in one step.
        window.sidebar_request(sidebar::Request::DeleteChecked);
        let key = |key: egui::Key| egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        for events in [Vec::new(), vec![key(egui::Key::Enter)]] {
            let input = egui::RawInput {
                events,
                ..egui::RawInput::default()
            };
            let mut output = ctx.run_ui(input, |ui| window.confirm_checked_deletion(ui.ctx()));
            output.textures_delta.clear();
        }
        assert_eq!(window.workspace.plate().objects.len(), 1);
        assert!(window.checked.is_empty() && !window.deleting_checked);
        assert!(window.workspace.undo());
        assert_eq!(window.workspace.plate().objects.len(), 3);
        // All cleared at once.
        window.sidebar_request(sidebar::Request::CheckAll(true));
        window.sidebar_request(sidebar::Request::CheckAll(false));
        assert!(window.checked.is_empty());
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn the_delete_key_asks_before_removing_a_model() {
        let folder =
            std::env::temp_dir().join(format!("karasu-window-delete-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let model = folder.join("cube.stl");
        std::fs::write(&model, crate::workspace::tests::box_stl([0.0; 3], [8.0; 3])).unwrap();
        let mut window = MainWindow::new(Vec::new());
        window.workspace.open(model);
        crate::workspace::tests::wait(&mut window.workspace);
        window.selected = Some(0);
        let ctx = egui::Context::default();
        let key = |key: egui::Key| egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        let frame = |window: &mut MainWindow, events: Vec<egui::Event>| {
            let input = egui::RawInput {
                events,
                ..egui::RawInput::default()
            };
            let mut output = ctx.run_ui(input, |ui| window.confirm_deletion(ui.ctx()));
            output.textures_delta.clear();
        };
        // Delete asks; Escape keeps the model.
        window.perform(Action::Delete);
        frame(&mut window, Vec::new());
        frame(&mut window, vec![key(egui::Key::Escape)]);
        assert_eq!(window.workspace.plate().objects.len(), 1);
        assert_eq!(window.deleting, None);
        // Delete, then Enter, removes it.
        window.perform(Action::Delete);
        frame(&mut window, Vec::new());
        frame(&mut window, vec![key(egui::Key::Enter)]);
        assert_eq!(window.workspace.plate().objects.len(), 0, "removed");
        assert_eq!(window.selected, None);
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_panel_footer_sits_at_the_bottom_under_a_body_filling_the_rest() {
        // A short body and a two-line footer, as the Repair panel: the footer ends at the
        // panel's bottom and the body fills the height above it.
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0));
        let id = egui::Id::new("footer-test");
        let mut rects = (egui::Rect::NOTHING, egui::Rect::NOTHING);
        for _ in 0..3 {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(screen),
                    ..Default::default()
                },
                |ui| {
                    egui::Area::new(egui::Id::new("panel"))
                        .fixed_pos(egui::pos2(100.0, 100.0))
                        .show(ui.ctx(), |ui| {
                            ui.set_width(PANEL_WIDTH);
                            let top = ui.cursor().top();
                            ui.set_height(800.0);
                            ui.horizontal(|ui| {
                                ui.label("Repair");
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| ui.label("Advanced"),
                                );
                            });
                            ui.separator();
                            body_and_footer(ui, top + 800.0, id, true, |ui, footer| {
                                let response = if footer {
                                    ui.label("Ready: slicing handles what remains");
                                    ui.button("Use Standard")
                                } else {
                                    ui.label("Exact (moves no vertex)")
                                };
                                if footer {
                                    rects.1 = response.rect;
                                } else {
                                    rects.0 = response.rect;
                                }
                            });
                        });
                },
            );
            output.textures_delta.clear();
        }
        let (body, footer) = rects;
        // The footer's last line ends within a few points of the panel's bottom (900).
        assert!(
            (900.0 - footer.bottom()).abs() <= 8.0,
            "footer ends at {}",
            footer.bottom()
        );
        // The body stays at the top, far above the footer.
        assert!(
            body.top() < 200.0 && footer.top() > 800.0,
            "{body:?} {footer:?}"
        );
    }

    #[test]
    fn a_row_wider_than_the_panel_is_cut_and_never_widens_it() {
        let ctx = egui::Context::default();
        let mut width = 0.0;
        for _ in 0..2 {
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                let area = egui::Area::new(egui::Id::new("wide-panel"))
                    .fixed_pos(egui::pos2(10.0, 10.0))
                    .show(ui.ctx(), |ui| {
                        ui.set_width(PANEL_WIDTH);
                        let top = ui.cursor().top();
                        body_and_footer(ui, top + 400.0, egui::Id::new("wide"), false, |ui, _| {
                            ui.horizontal(|ui| {
                                for k in 0..30 {
                                    ui.label(format!("label {k}"));
                                }
                            });
                        });
                    });
                width = area.response.rect.width();
            });
            output.textures_delta.clear();
        }
        assert!((width - PANEL_WIDTH).abs() < 1.0, "{width}");
    }

    #[test]
    fn a_tool_panel_keeps_its_full_height_whatever_it_held_before() {
        // A panel first holding little, then much, then in a shorter viewport: its area follows
        // the height set each frame (a shorter tool's size once cut the Repair panel).
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0));
        let mut heights = Vec::new();
        for (frame, bottom) in [
            (0, 1000.0),
            (1, 1000.0),
            (2, 1000.0),
            (3, 600.0),
            (4, 600.0),
        ] {
            let viewport =
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1500.0, bottom));
            let top = viewport.top() + viewcube::BOTTOM + 8.0;
            let lines = if frame == 0 { 3 } else { 80 };
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(screen),
                    ..Default::default()
                },
                |ui| {
                    let area = egui::Area::new(egui::Id::new("tool-panel"))
                        .pivot(egui::Align2::RIGHT_TOP)
                        .fixed_pos(egui::pos2(viewport.right() - 12.0, top))
                        .show(ui.ctx(), |ui| {
                            egui::Frame::window(ui.style()).show(ui, |ui| {
                                ui.set_width(PANEL_WIDTH);
                                ui.set_height(panel_height(viewport, top));
                                crate::focus::scrolled(
                                    egui::ScrollArea::vertical().auto_shrink([false, false]),
                                    ui,
                                    |ui| {
                                        for k in 0..lines {
                                            ui.label(format!("line {k}"));
                                        }
                                    },
                                );
                            });
                        });
                    heights.push(area.response.rect.bottom());
                },
            );
            output.textures_delta.clear();
        }
        // The panel ends a margin above the viewport's bottom, before and after the change.
        for (bottom, expected) in heights.iter().zip([1000.0, 1000.0, 1000.0, 600.0, 600.0]) {
            assert!(
                (expected - PANEL_MARGIN - bottom).abs() <= 2.0,
                "{bottom} for a viewport ending at {expected}"
            );
        }
    }

    #[test]
    fn the_status_line_keeps_to_its_width_and_cuts_long_messages() {
        let ctx = egui::Context::default();
        let long = "A message long enough to run past the space the status bar leaves for it";
        let statuses = [
            Status {
                busy: true,
                ..Status::new(
                    "Importing Base_01.stl".to_owned(),
                    theme::TEXT_PRIMARY.color(),
                )
            },
            Status::new(long.to_owned(), theme::WARNING.color()),
        ];
        let mut width = f32::INFINITY;
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.allocate_ui(egui::vec2(240.0, 24.0), |ui| {
                ui.horizontal(|ui| width = status_line(ui, &statuses).rect.width());
            });
        });
        output.textures_delta.clear();
        assert!(width <= 240.0, "{width}");
        assert!(width > 100.0, "{width}");
    }

    #[test]
    fn the_grid_mode_plans_a_face_clicked_in_the_window() {
        let (workspace, preparation, folder) = crate::grid_ui::tests::lifted_slab("window");
        let mut window = MainWindow::new(Vec::new());
        window.workspace = workspace;
        window.preparation = preparation;
        window.selected = Some(0);
        window.workflow.choose_tool(Tool::Supports);
        assert!(window.enabled(Action::GridSupports));
        window.perform(Action::GridSupports);
        assert!(window.grid.active);
        let placed = window.workspace.placement_of(0).unwrap();
        let (min, max) = (placed.min.coordinates(), placed.max.coordinates());
        let [cx, cy] = [0, 1].map(|k| f64::midpoint(min[k], max[k]));
        let under = crate::supports_ui::tests::ray(cx, cy, true);
        assert!(window.tool_click(&under, 0));
        let ctx = egui::Context::default();
        let started = std::time::Instant::now();
        while window
            .grid
            .planned_count(&window.workspace, Some(0))
            .is_none()
        {
            // Each frame: the polling, then the panel's tools (the owner's trial, 2026-11-06:
            // the panel asked for no surface, so the grid waited for one).
            window.poll_supports(&ctx);
            window.leave_other_tools(Tool::Supports);
            let messages = window.grid.take_messages();
            assert!(messages.is_empty(), "{messages:?}");
            assert!(started.elapsed() < Duration::from_secs(30), "no grid");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            window
                .grid
                .planned_count(&window.workspace, Some(0))
                .unwrap()
                > 0
        );
        let _ = std::fs::remove_dir_all(folder);
    }
}

// The accessibility checks of the window (TD-142), last: the messages' check reads each source
// only up to its first test attribute.
#[cfg(test)]
mod accessibility_tests;
