//! The main window's settings kept across restarts (TD-097): applied when the window starts,
//! collected from it as they are now, and saved a moment after they change and when the
//! application closes.
use super::{MainWindow, MODELS_SECTION, PLATE_SECTION};
use crate::{actions::Keymap, profiles_ui::ProfilesUi};
use crate::{
    dialog::Remembered,
    i18n::tr,
    manipulate::Space,
    messages::{Level, Message},
    settings::{
        ImportRepair, LayoutSettings, Opened, Settings, Store, WindowSettings, WindowState,
    },
};
use eframe::egui;
use karasu_core::preparation::repair::RepairPreset;
use std::time::{Duration, Instant};

/// How long settings stay unsaved after a change: dragging a window's edge or a colour saves
/// once, when it rests.
const SAVE_AFTER: Duration = Duration::from_secs(1);

/// How often the settings are compared with what was saved: not every frame, which would
/// allocate in the frame loop for nothing.
const CHECK_EVERY: Duration = Duration::from_millis(250);

/// The collapsible sections whose state is kept.
const SECTIONS: [&str; 2] = [PLATE_SECTION, MODELS_SECTION];

/// The settings store and its saving.
pub(super) struct Persistence {
    store: Store,
    /// The context the layout's state is kept in, to collect it when the application closes.
    ctx: egui::Context,
    /// When the settings first differed from what was saved, while they do.
    changed_at: Option<Instant>,
    /// A save failed and was reported: later failures are logged only.
    failure_reported: bool,
    /// When the settings were last compared.
    checked_at: Instant,
}

/// The settings file as the settings dialog shows it.
#[derive(Clone, Debug)]
pub(super) struct SettingsFile {
    /// Where it is.
    pub path: std::path::PathBuf,
    /// What opening it found (`Missing` once started over).
    pub opened: Opened,
}

impl MainWindow {
    /// The settings file, when the window keeps its settings.
    pub(super) fn settings_file(&self) -> Option<SettingsFile> {
        self.persistence.as_ref().map(|p| SettingsFile {
            path: p.store.path().to_path_buf(),
            opened: p.store.opened().clone(),
        })
    }

    /// Start over with the default settings: a file this session did not save over is kept
    /// beside (said in a notification), and the defaults apply and are saved at once.
    pub(super) fn start_over_settings(&mut self, ctx: &egui::Context) {
        let kept = match self.persistence.as_mut().map(|p| p.store.start_over()) {
            Some(Ok(kept)) => kept,
            Some(Err(error)) => {
                self.messages.push(
                    Message::new(Level::Error, tr!("settings-start-over-failed")).detail(error),
                );
                return;
            }
            None => None,
        };
        let defaults = Settings::default();
        self.apply_settings(ctx, &defaults);
        for name in SECTIONS {
            egui::collapsing_header::CollapsingState::load_with_default_open(
                ctx,
                egui::Id::new(name),
                true,
            )
            .store(ctx);
        }
        self.persist(true);
        let mut message = Message::new(Level::Success, tr!("settings-started-over"));
        if let Some(kept) = kept {
            message = message.detail(tr!("settings-kept-as", path = kept.display().to_string()));
        }
        self.messages.push(message);
    }

    /// Open the settings file at `path` and apply what it holds; what it could not read, or a
    /// newer version's file, is reported and left as it is. Without a path (a smoke test), the
    /// window keeps its defaults and saves nothing.
    pub fn load_settings(&mut self, ctx: &egui::Context, path: Option<std::path::PathBuf>) {
        let Some(path) = path else {
            return;
        };
        let (store, settings, opened) = Store::open(path);
        let file = store.path().display().to_string();
        match opened {
            Opened::Missing | Opened::Read => {}
            Opened::Unreadable(error) => {
                log::warn!(target: "app", "settings {file}: {error}");
                self.messages.push(
                    Message::new(Level::Error, tr!("settings-unreadable"))
                        .detail(format!("{file}\n{error}")),
                );
            }
            Opened::Newer(version) => {
                log::warn!(target: "app", "settings {file}: schema {version} is newer");
                self.messages
                    .push(Message::new(Level::Warning, tr!("settings-newer")).detail(file.clone()));
            }
        }
        self.apply_settings(ctx, &settings);
        self.persistence = Some(Persistence {
            store,
            ctx: ctx.clone(),
            changed_at: None,
            failure_reported: false,
            checked_at: Instant::now(),
        });
    }

    /// The Islands and Automatic tools' switches and windows `tools` keeps, and the suction
    /// cups' (TD-114).
    fn apply_island_tools(&mut self, tools: &crate::settings::ToolSettings) {
        self.hollowing.cups.show = tools.cups_shown;
        if crate::hollowing_ui::cups::SHOW_FROM_MM2.contains(&tools.cups_show_from_mm2) {
            self.hollowing.cups.show_from_mm2 = tools.cups_show_from_mm2;
        }
        if crate::hollowing_ui::cups::SHOW_TALLER_MM.contains(&tools.cups_show_taller_mm) {
            self.hollowing.cups.show_taller_mm = tools.cups_show_taller_mm;
        }
        self.islands.advanced = tools.islands_advanced;
        self.islands.show_islands = tools.islands_shown;
        self.islands.show_minima = tools.minima_shown;
        self.islands.list_open = tools.islands_list_open;
        self.auto.advanced = tools.auto_advanced;
        self.orient.advanced = tools.orient_advanced;
        self.auto.show_left = tools.auto_left_shown;
        self.auto.list_open = tools.auto_list_open;
    }

    /// The support presets `tools` keeps, and those each tool starts from.
    fn apply_support_presets(&mut self, tools: &crate::settings::ToolSettings) {
        // Presets a settings file keeps with values a support would refuse are left out.
        self.supports.presets = tools
            .support_presets
            .iter()
            .filter(|p| p.values.is_valid())
            .cloned()
            .collect();
        self.supports.default_preset = tools
            .support_preset
            .as_ref()
            .and_then(|name| self.supports.presets.iter().position(|p| p.name == *name));
        self.supports.islands_default_mine = tools
            .island_preset
            .as_ref()
            .and_then(|name| self.supports.presets.iter().position(|p| p.name == *name));
        self.supports.automatic_default_mine = tools
            .auto_preset
            .as_ref()
            .and_then(|name| self.supports.presets.iter().position(|p| p.name == *name));
        self.supports.automatic_cavity_default_mine = tools
            .auto_cavity_preset
            .as_ref()
            .and_then(|name| self.supports.presets.iter().position(|p| p.name == *name));
        self.supports.grid_default_mine = tools
            .grid_preset
            .as_ref()
            .and_then(|name| self.supports.presets.iter().position(|p| p.name == *name));
    }

    /// Apply `settings` to the window and to `ctx` (the layout egui keeps).
    pub(super) fn apply_settings(&mut self, ctx: &egui::Context, settings: &Settings) {
        self.appearance = settings.appearance;
        let (keymap, problems) = Keymap::with_overrides(&settings.shortcuts);
        self.keymap = keymap;
        if !problems.is_empty() {
            log::warn!(target: "app", "shortcuts left out: {}", problems.join("; "));
            self.messages.push(
                Message::new(Level::Warning, tr!("settings-shortcuts-skipped"))
                    .detail(problems.join("\n")),
            );
        }
        self.set_language(settings.language.clone());
        self.workspace
            .set_auto_repair(match settings.repair_on_import {
                ImportRepair::Off => None,
                ImportRepair::Exact => Some(RepairPreset::Exact),
                ImportRepair::Standard => Some(RepairPreset::Standard),
                ImportRepair::Aggressive => Some(RepairPreset::Aggressive),
            });
        self.window_settings = settings.window;
        self.project_file.apply(&settings.projects);
        self.plates.choices = settings.plates.clone();
        self.models_export.choices = settings.models.clone();
        let layout = &settings.layout;
        self.chrome.sidebar_open = layout.sidebar_open;
        self.chrome.panel_open = layout.panel_open;
        self.chrome.plates_bar_open = layout.plates_bar_open;
        *self.workflow.tools_shown() = layout.tools_shown;
        for (name, open) in &layout.sections {
            let mut state = egui::collapsing_header::CollapsingState::load_with_default_open(
                ctx,
                egui::Id::new(name.as_str()),
                *open,
            );
            state.set_open(*open);
            state.store(ctx);
        }
        Remembered {
            dialogs: layout.dialogs.clone(),
            windows: layout.windows.clone(),
            speeds_per_minute: settings.tools.speeds_per_minute,
        }
        .keep(ctx);
        let tools = &settings.tools;
        self.space = if tools.transform_local {
            Space::Local
        } else {
            Space::Global
        };
        self.transform.locked = tools.transform_locks;
        self.transform.uniform = tools.transform_uniform;
        *self.hollowing.repair_advanced() = tools.repair_advanced;
        *self.hollowing.advanced(crate::hollowing_ui::Part::Hollow) = tools.hollow_advanced;
        *self.hollowing.advanced(crate::hollowing_ui::Part::Holes) = tools.holes_advanced;
        self.slicing.advanced = tools.slice_advanced;
        *self.slicing.verify_walls() = tools.verify_walls;
        *self.slicing.follow_layer() = tools.layers_follow;
        self.overhangs.shown = tools.show_overhangs;
        if crate::settings::OVERHANG_ANGLE_RANGE.contains(&tools.overhang_angle_deg) {
            self.overhangs.angle_deg = tools.overhang_angle_deg;
        }
        self.supports.advanced = tools.supports_advanced;
        self.supports.knee_stays = !tools.knee_follows_foot;
        self.supports.straight_only = !tools.avoid_model;
        self.supports.tip_then_foot = tools.tip_then_foot;
        self.grid.set_mode(tools.grid_pick);
        self.supports.branching = tools.place_branch && !tools.tip_then_foot;
        self.supports.mini = tools.place_mini;
        self.supports.shown = tools.supports_shown;
        self.supports.bracing.shown = tools.braces_shown;
        self.hollowing.holes_shown = tools.holes_shown;
        self.raft.advanced = tools.raft_advanced;
        self.supports.list_open = tools.supports_list_open;
        self.supports.tips_only = tools.supports_tips_only;
        self.hollowing.holes_list_open = tools.holes_list_open;
        self.hollowing.blockers.list_open = tools.blockers_list_open;
        self.apply_island_tools(tools);
        self.supports.bracing.chain = tools.braces_chain;
        self.supports.bracing.level = tools.braces_level;
        self.supports.bracing.list_open = tools.braces_list_open;
        self.apply_support_presets(tools);
        // The tools' defaults: every tool opens with them (owner, 2026-10-27).
        self.tool_defaults = tools.defaults();
        self.reset_tool_values();
        // The last session's profiles, set up before the user does anything: no undo step.
        if self
            .profiles
            .restore(&mut self.workspace, &settings.profiles)
        {
            self.workspace.forget_history();
        }
    }

    /// The settings as the window and `ctx` hold them now.
    pub(super) fn collect_settings(&mut self, ctx: &egui::Context) -> Settings {
        let remembered = Remembered::of(ctx);
        let sections = SECTIONS
            .iter()
            .filter_map(|&name| {
                egui::collapsing_header::CollapsingState::load(ctx, egui::Id::new(name))
                    .map(|state| (name.to_owned(), state.is_open()))
            })
            .collect();
        let mut settings = Settings {
            appearance: self.appearance,
            repair_on_import: match self.workspace.auto_repair() {
                None | Some(RepairPreset::Off | RepairPreset::Rebuild) => ImportRepair::Off,
                Some(RepairPreset::Exact) => ImportRepair::Exact,
                Some(RepairPreset::Standard) => ImportRepair::Standard,
                Some(RepairPreset::Aggressive) => ImportRepair::Aggressive,
            },
            window: self.window_settings,
            shortcuts: self.keymap.overrides(),
            language: self.language.clone(),
            projects: self.project_file.settings(),
            plates: self.plates.choices.clone(),
            models: self.models_export.choices.clone(),
            layout: LayoutSettings {
                sidebar_open: self.chrome.sidebar_open,
                panel_open: self.chrome.panel_open,
                tools_shown: *self.workflow.tools_shown(),
                plates_bar_open: self.chrome.plates_bar_open,
                sections,
                dialogs: remembered.dialogs,
                windows: remembered.windows,
            },
            ..Settings::default()
        };
        let tools = &mut settings.tools;
        tools.transform_local = self.space == Space::Local;
        tools.transform_locks = self.transform.locked;
        tools.transform_uniform = self.transform.uniform;
        tools.repair_advanced = *self.hollowing.repair_advanced();
        tools.hollow_advanced = *self.hollowing.advanced(crate::hollowing_ui::Part::Hollow);
        tools.holes_advanced = *self.hollowing.advanced(crate::hollowing_ui::Part::Holes);
        tools.set_defaults(&self.tool_defaults);
        tools.slice_advanced = self.slicing.advanced;
        tools.verify_walls = *self.slicing.verify_walls();
        tools.layers_follow = *self.slicing.follow_layer();
        tools.speeds_per_minute = remembered.speeds_per_minute;
        tools.show_overhangs = self.overhangs.shown;
        tools.overhang_angle_deg = self.overhangs.angle_deg;
        tools.supports_advanced = self.supports.advanced;
        tools.knee_follows_foot = !self.supports.knee_stays;
        tools.avoid_model = !self.supports.straight_only;
        tools.tip_then_foot = self.supports.tip_then_foot;
        tools.grid_pick = self.grid.mode;
        tools.place_branch = self.supports.branching;
        tools.place_mini = self.supports.mini;
        tools.supports_shown = self.supports.shown;
        tools.braces_shown = self.supports.bracing.shown;
        tools.holes_shown = self.hollowing.holes_shown;
        tools.cups_shown = self.hollowing.cups.show;
        tools.cups_show_from_mm2 = self.hollowing.cups.show_from_mm2;
        tools.cups_show_taller_mm = self.hollowing.cups.show_taller_mm;
        tools.raft_advanced = self.raft.advanced;
        tools.supports_list_open = self.supports.list_open;
        tools.supports_tips_only = self.supports.tips_only;
        tools.holes_list_open = self.hollowing.holes_list_open;
        tools.blockers_list_open = self.hollowing.blockers.list_open;
        tools.islands_advanced = self.islands.advanced;
        tools.islands_shown = self.islands.show_islands;
        tools.minima_shown = self.islands.show_minima;
        tools.islands_list_open = self.islands.list_open;
        tools.auto_advanced = self.auto.advanced;
        tools.orient_advanced = self.orient.advanced;
        tools.auto_left_shown = self.auto.show_left;
        tools.auto_list_open = self.auto.list_open;
        tools.braces_chain = self.supports.bracing.chain;
        tools.braces_level = self.supports.bracing.level;
        tools.braces_list_open = self.supports.bracing.list_open;
        tools.support_presets.clone_from(&self.supports.presets);
        // The user's presets the tools start from, by name.
        let named = |place: Option<usize>| {
            place
                .and_then(|i| self.supports.presets.get(i))
                .map(|p| p.name.clone())
        };
        tools.support_preset = named(self.supports.default_preset);
        tools.island_preset = named(self.supports.islands_default_mine);
        tools.auto_preset = named(self.supports.automatic_default_mine);
        tools.auto_cavity_preset = named(self.supports.automatic_cavity_default_mine);
        tools.grid_preset = named(self.supports.grid_default_mine);
        settings.profiles = ProfilesUi::choice(&self.workspace);
        settings
    }

    /// Use `language` for the interface (`None`: the reference), unless the command line chose
    /// one for this start; a language without a catalog keeps the reference, said in a
    /// notification.
    pub(super) fn set_language(&mut self, language: Option<String>) {
        self.language = language;
        if self.launch.locale_fixed {
            return;
        }
        let locale = self
            .language
            .clone()
            .unwrap_or_else(|| crate::i18n::REFERENCE.to_owned());
        if locale == self.launch.locale {
            return;
        }
        match crate::i18n::Catalog::open(&locale, self.launch.locale_dir.as_deref()) {
            Ok(catalog) => {
                crate::i18n::install(catalog);
                self.launch.locale = locale;
            }
            Err(error) => {
                log::warn!(target: "app", "language {locale}: {error}");
                self.messages.push(
                    Message::new(Level::Warning, tr!("settings-language-missing")).detail(error),
                );
            }
        }
    }

    /// How the main window should open: its state, and its size and place as a window.
    #[must_use]
    pub fn window_settings(&self) -> WindowSettings {
        self.window_settings
    }

    /// Follow the main window's state, size and place as the user leaves them: the size and
    /// place only while it is a plain window, so a maximized window restores to them.
    pub fn track_window(&mut self, ctx: &egui::Context) {
        let (maximized, fullscreen, minimized, inner, outer) = ctx.input(|i| {
            let viewport = i.viewport();
            (
                viewport.maximized.unwrap_or(false),
                viewport.fullscreen.unwrap_or(false),
                viewport.minimized.unwrap_or(false),
                viewport.inner_rect,
                viewport.outer_rect,
            )
        });
        if minimized {
            return;
        }
        let window = &mut self.window_settings;
        window.state = if fullscreen {
            WindowState::Fullscreen
        } else if maximized {
            WindowState::Maximized
        } else {
            WindowState::Windowed
        };
        if window.state == WindowState::Windowed {
            if let (Some(inner), Some(outer)) = (inner, outer) {
                window.size = Some([inner.width().round(), inner.height().round()]);
                window.position = Some([outer.min.x.round(), outer.min.y.round()]);
            }
        }
    }

    /// Save the settings a moment after they changed, or at once when `now` (the application
    /// closing). A failed save is reported once, and the settings stay to save again.
    pub fn persist(&mut self, now: bool) {
        let Some(persistence) = self.persistence.as_mut() else {
            return;
        };
        let since_check = persistence.checked_at.elapsed();
        if !now && since_check < CHECK_EVERY {
            // A frame comes at the next check, so a change made just now is not left unsaved.
            persistence
                .ctx
                .request_repaint_after(CHECK_EVERY.saturating_sub(since_check));
            return;
        }
        persistence.checked_at = Instant::now();
        let ctx = persistence.ctx.clone();
        let settings = self.collect_settings(&ctx);
        let Some(persistence) = self.persistence.as_mut() else {
            return;
        };
        if !persistence.store.changed(&settings) {
            persistence.changed_at = None;
            return;
        }
        let since = *persistence.changed_at.get_or_insert_with(Instant::now);
        if !now && since.elapsed() < SAVE_AFTER {
            ctx.request_repaint_after(SAVE_AFTER);
            return;
        }
        match persistence.store.save(&settings) {
            Ok(()) => persistence.changed_at = None,
            Err(error) => {
                log::warn!(target: "app", "settings not saved: {error}");
                // Tried again after the delay, without repeating the report.
                persistence.changed_at = Some(Instant::now());
                if !persistence.failure_reported {
                    persistence.failure_reported = true;
                    self.messages.push(
                        Message::new(Level::Warning, tr!("settings-not-saved")).detail(error),
                    );
                }
            }
        }
    }
}
