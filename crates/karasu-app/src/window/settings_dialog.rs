//! The settings dialog (TD-097, M6.5 u3; the owner's mockup of 2026-10-26): the colours, the
//! keyboard shortcuts, the repair of opened models, the window, the language and the settings
//! file. Changes apply at once and are kept across restarts, so the footer only closes it.
use super::{persist::SettingsFile, MainWindow};
use crate::{
    actions::{shortcut, Action},
    appearance::{Appearance, FilterStyle, Rgba},
    dialog::{self, Part, Remembered, Size},
    i18n::tr,
    icons::Icon,
    manipulate::Mode,
    messages::{Level, Message},
    section_ui::SectionStep,
    settings::OpenAs,
    theme,
    viewport::Pattern,
    widgets::{icon_button, range_slider, switch, text_field, Unit, UnitField},
    workflow::{Step, Tool},
};
use eframe::egui::{self, KeyboardShortcut};
use karasu_core::preparation::repair::RepairPreset;

/// The dialog's sections, in the order the side list shows them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Section {
    #[default]
    Appearance,
    Shortcuts,
    Hollowing,
    Supports,
    Models,
    Window,
    Language,
    File,
}

impl Section {
    const ALL: [Self; 8] = [
        Self::Appearance,
        Self::Shortcuts,
        Self::Hollowing,
        Self::Supports,
        Self::Models,
        Self::Window,
        Self::Language,
        Self::File,
    ];

    fn label(self) -> String {
        match self {
            Self::Appearance => tr!("settings-appearance"),
            Self::Shortcuts => tr!("settings-shortcuts"),
            Self::Hollowing => tr!("settings-hollowing"),
            Self::Supports => tr!("settings-supports"),
            Self::Models => tr!("settings-models"),
            Self::Window => tr!("settings-window"),
            Self::Language => tr!("settings-language"),
            Self::File => tr!("settings-file"),
        }
    }
}

/// The dialog's own state while it is open.
#[derive(Debug, Default)]
pub(super) struct SettingsDialog {
    section: Section,
    /// The colours beyond the main ones show.
    more_colours: bool,
    /// The action whose new shortcut is awaited.
    capturing: Option<Action>,
    /// A shortcut another action has, waiting for the user's yes: the action it is for, the
    /// shortcut, and the action that has it.
    conflict: Option<(Action, KeyboardShortcut, Action)>,
    /// The shortcuts' search.
    search: String,
    /// The hexadecimal field being typed in, by colour, with its text.
    hex: Option<(&'static str, String)>,
    /// Starting over, waiting for the user's yes.
    starting_over: bool,
    /// The dialogs' and tool windows' sizes and places were forgotten.
    places_forgotten: bool,
}

/// The dialog's size (design rule, `docs/UI-DESIGN.md`, "Windows").
const SIZE: Size = Size::Resizable {
    default: egui::vec2(920.0, 640.0),
    min: egui::vec2(760.0, 480.0),
};
/// The side list's width.
const NAV_WIDTH: f32 = 180.0;
/// A colour's hexadecimal field's width: `#RRGGBBAA` in the monospace font, with its margins.
const HEX_FIELD: f32 = 96.0;
/// A percentage's slider's width, beside its value field.
const PERCENT_SLIDER: f32 = 200.0;

/// The colours under a heading, by identity (see [`colour`]).
const GROUPS: [(&str, &[&str]); 9] = [
    (
        "models",
        &[
            "object",
            "selected",
            "selection_outline",
            "section_cap",
            "model_ghost",
        ],
    ),
    (
        "scene",
        &[
            "background",
            "plate",
            "grid",
            "grid_major",
            "plate_border",
            "front_tab",
        ],
    ),
    (
        "handles",
        &["axis_x", "axis_y", "axis_z", "uniform", "hovered"],
    ),
    (
        "overhangs",
        &["overhang_limit", "overhang_level", "protected"],
    ),
    (
        "holes",
        &[
            "drain_hole",
            "hole_valid",
            "suction_cup",
            "suction_cup_narrow",
        ],
    ),
    ("blockers", &["blocker", "blocker_new", "blocker_selected"]),
    (
        "supports",
        &[
            "support",
            "support_selected",
            "support_error",
            "support_error_selected",
            "support_section_cap",
            "filtered_ghost",
            "contact_line",
        ],
    ),
    (
        "markers",
        &[
            "marker_fails",
            "marker_hollowing",
            "marker_harmless",
            "marker_repaired",
        ],
    ),
    // The interface's status colours (TD-142), in the order of `theme::STATUS_TOKENS`.
    (
        "status",
        &[
            "status_warning",
            "status_error",
            "status_success",
            "status_info_fill",
            "status_success_fill",
            "status_warning_fill",
            "status_error_fill",
        ],
    ),
];
/// The colours shown on request.
const MORE: [&str; 8] = [
    "outline",
    "front_label",
    "free_move",
    "free_move_hovered",
    "cavity",
    "cavity_selected",
    "hole_selected",
    "hole_unchecked",
];

/// The colour of identity `id` in `a`.
fn colour<'a>(a: &'a mut Appearance, id: &str) -> Option<&'a mut Rgba> {
    Some(match id {
        "object" => &mut a.object,
        "selected" => &mut a.selected,
        "selection_outline" => &mut a.selection_outline,
        "section_cap" => &mut a.section_cap,
        "background" => &mut a.background,
        "plate" => &mut a.plate,
        "grid" => &mut a.grid,
        "grid_major" => &mut a.grid_major,
        "plate_border" => &mut a.plate_border,
        "front_tab" => &mut a.front_tab,
        "axis_x" => &mut a.axes[0],
        "axis_y" => &mut a.axes[1],
        "axis_z" => &mut a.axes[2],
        "hovered" => &mut a.hovered,
        "drain_hole" => &mut a.drain_hole,
        "hole_valid" => &mut a.hole_valid,
        "support" => &mut a.support,
        "support_error" => &mut a.support_error,
        "support_selected" => &mut a.support_selected,
        "support_error_selected" => &mut a.support_error_selected,
        "support_section_cap" => &mut a.support_section_cap,
        "filtered_ghost" => &mut a.filtered_ghost,
        "contact_line" => &mut a.contact_line,
        "suction_cup" => &mut a.suction_cup,
        "suction_cup_narrow" => &mut a.suction_cup_narrow,
        "blocker" => &mut a.blocker,
        "blocker_new" => &mut a.blocker_new,
        "blocker_selected" => &mut a.blocker_selected,
        "marker_fails" => &mut a.marker_fails,
        "marker_hollowing" => &mut a.marker_hollowing,
        "marker_harmless" => &mut a.marker_harmless,
        "marker_repaired" => &mut a.marker_repaired,
        "outline" => &mut a.outline,
        "front_label" => &mut a.front_label,
        "uniform" => &mut a.uniform,
        "free_move" => &mut a.free_move,
        "free_move_hovered" => &mut a.free_move_hovered,
        "cavity" => &mut a.cavity,
        "cavity_selected" => &mut a.cavity_selected,
        "model_ghost" => &mut a.model_ghost,
        "hole_selected" => &mut a.hole_selected,
        "hole_unchecked" => &mut a.hole_unchecked,
        "overhang_limit" => &mut a.overhang_limit,
        "overhang_level" => &mut a.overhang_level,
        "protected" => &mut a.protected,
        "status_warning" => &mut a.status[0],
        "status_error" => &mut a.status[1],
        "status_success" => &mut a.status[2],
        "status_info_fill" => &mut a.status[3],
        "status_success_fill" => &mut a.status[4],
        "status_warning_fill" => &mut a.status[5],
        "status_error_fill" => &mut a.status[6],
        _ => return None,
    })
}

/// The colour's name in the interface.
fn colour_label(id: &str) -> String {
    match id {
        "object" => tr!("settings-colour-object"),
        "selected" => tr!("settings-colour-selected"),
        "selection_outline" => tr!("settings-colour-selection-outline"),
        "section_cap" => tr!("settings-colour-section-cap"),
        "background" => tr!("settings-colour-background"),
        "plate" => tr!("settings-colour-plate"),
        "grid" => tr!("settings-colour-grid"),
        "grid_major" => tr!("settings-colour-grid-major"),
        "plate_border" => tr!("settings-colour-plate-border"),
        "front_tab" => tr!("settings-colour-front-tab"),
        "axis_x" => tr!("settings-colour-axis-x"),
        "axis_y" => tr!("settings-colour-axis-y"),
        "axis_z" => tr!("settings-colour-axis-z"),
        "hovered" => tr!("settings-colour-hovered"),
        "drain_hole" => tr!("settings-colour-drain-hole"),
        "hole_valid" => tr!("settings-colour-hole-valid"),
        "support" => tr!("settings-colour-support"),
        "support_error" => tr!("settings-colour-support-error"),
        "support_selected" => tr!("settings-colour-support-selected"),
        "support_error_selected" => tr!("settings-colour-support-error-selected"),
        "support_section_cap" => tr!("settings-colour-support-section-cap"),
        "filtered_ghost" => tr!("settings-colour-filtered-ghost"),
        "contact_line" => tr!("settings-colour-contact-line"),
        "suction_cup" => tr!("settings-colour-suction-cup"),
        "suction_cup_narrow" => tr!("settings-colour-suction-cup-narrow"),
        "blocker" => tr!("settings-colour-blocker"),
        "blocker_new" => tr!("settings-colour-blocker-new"),
        "blocker_selected" => tr!("settings-colour-blocker-selected"),
        "marker_fails" => tr!("settings-colour-marker-fails"),
        "marker_hollowing" => tr!("settings-colour-marker-hollowing"),
        "marker_harmless" => tr!("settings-colour-marker-harmless"),
        "marker_repaired" => tr!("settings-colour-marker-repaired"),
        "outline" => tr!("settings-colour-outline"),
        "front_label" => tr!("settings-colour-front-label"),
        "uniform" => tr!("settings-colour-uniform"),
        "free_move" => tr!("settings-colour-free-move"),
        "free_move_hovered" => tr!("settings-colour-free-move-hovered"),
        "cavity" => tr!("settings-colour-cavity"),
        "cavity_selected" => tr!("settings-colour-cavity-selected"),
        "model_ghost" => tr!("settings-colour-model-ghost"),
        "hole_selected" => tr!("settings-colour-hole-selected"),
        "overhang_limit" => tr!("settings-colour-overhang-limit"),
        "overhang_level" => tr!("settings-colour-overhang-level"),
        "protected" => tr!("settings-colour-protected"),
        "status_warning" => tr!("settings-colour-status-warning"),
        "status_error" => tr!("settings-colour-status-error"),
        "status_success" => tr!("settings-colour-status-success"),
        "status_info_fill" => tr!("settings-colour-status-info-fill"),
        "status_success_fill" => tr!("settings-colour-status-success-fill"),
        "status_warning_fill" => tr!("settings-colour-status-warning-fill"),
        "status_error_fill" => tr!("settings-colour-status-error-fill"),
        _ => tr!("settings-colour-hole-unchecked"),
    }
}

fn group_label(id: &str) -> String {
    match id {
        "models" => tr!("settings-colours-models"),
        "scene" => tr!("settings-colours-scene"),
        "handles" => tr!("settings-colours-handles"),
        "overhangs" => tr!("settings-colours-overhangs"),
        "holes" => tr!("settings-colours-holes"),
        "blockers" => tr!("settings-colours-blockers"),
        "supports" => tr!("settings-colours-supports"),
        "status" => tr!("settings-colours-status"),
        _ => tr!("settings-colours-markers"),
    }
}

/// Under the status colours, the ones that read poorly (TD-142): a text colour under 4.5:1 on the
/// panels, or a notification fill under 4.5:1 behind the primary text. Nothing when all read.
fn status_contrast(ui: &mut egui::Ui, status: &[Rgba; theme::STATUS_COUNT]) {
    const ENOUGH: f64 = 4.5;
    let panel = theme::SURFACE.rgba;
    let text = theme::TEXT_PRIMARY.rgba;
    let ids = [
        "status_warning",
        "status_error",
        "status_success",
        "status_info_fill",
        "status_success_fill",
        "status_warning_fill",
        "status_error_fill",
    ];
    let poor: Vec<String> = ids
        .iter()
        .zip(status)
        .enumerate()
        .filter_map(|(k, (id, colour))| {
            // The first three are text on the panels; the fills are behind the primary text.
            let ratio = if k < 3 {
                theme::contrast(colour.0, panel)
            } else {
                let mut fill = colour.0;
                fill[3] = 0xFF;
                theme::contrast(text, fill)
            };
            (ratio < ENOUGH).then(|| {
                tr!(
                    "settings-status-contrast-item",
                    colour = colour_label(id),
                    ratio = crate::i18n::decimal(ratio, 1)
                )
            })
        })
        .collect();
    if poor.is_empty() {
        return;
    }
    let icons = crate::icons::Icons::shared(ui.ctx());
    crate::widgets::state_line(
        ui,
        icons.as_deref(),
        Some(crate::icons::Icon::Warning),
        &tr!("settings-status-contrast", colours = poor.join(", ")),
        theme::WARNING.color(),
    );
}

/// `colour` as `#RRGGBB`, or `#RRGGBBAA` when not opaque.
#[must_use]
pub(super) fn hex(colour: Rgba) -> String {
    let [r, g, b, a] = colour.0;
    if a == 0xFF {
        format!("#{r:02X}{g:02X}{b:02X}")
    } else {
        format!("#{r:02X}{g:02X}{b:02X}{a:02X}")
    }
}

/// The colour `text` writes as six or eight hexadecimal digits, with or without `#`.
#[must_use]
pub(super) fn parse_hex(text: &str) -> Option<Rgba> {
    let digits = text.trim().trim_start_matches('#');
    if !matches!(digits.len(), 6 | 8) || !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let byte = |k: usize| u8::from_str_radix(&digits[k..k + 2], 16).ok();
    let alpha = if digits.len() == 8 { byte(6)? } else { 0xFF };
    Some(Rgba([byte(0)?, byte(2)?, byte(4)?, alpha]))
}

/// An action's name in the shortcuts list, at the left of its cell, its hint under the pointer.
fn action_name(ui: &mut egui::Ui, action: Action) {
    let size = egui::vec2(250.0, 26.0);
    let name = ui
        .allocate_ui_with_layout(
            size,
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.set_min_size(size);
                ui.add(egui::Label::new(action.label()).truncate())
            },
        )
        .inner;
    if let Some(hint) = action.hint() {
        name.on_hover_text(hint);
    }
}

/// The actions under a heading, as the shortcuts list groups them: the menus' first, in their
/// order, then the handles, the section, the tool strip, the steps and the tools. Every action
/// is listed once (a test checks it), so every shortcut the application has can be changed.
#[allow(clippy::too_many_lines)] // A list of data, one action a line.
fn shortcut_groups() -> Vec<(String, Vec<Action>)> {
    vec![
        (
            tr!("menu-file"),
            vec![
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
            vec![
                Action::Undo,
                Action::Redo,
                Action::Delete,
                Action::Cancel,
                Action::RestoreDefaults,
            ],
        ),
        (
            tr!("menu-view"),
            vec![
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
            vec![
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
            vec![
                Action::ToggleMessages,
                Action::ToggleLog,
                Action::EditProfiles,
            ],
        ),
        (tr!("menu-help"), vec![Action::About]),
        (
            tr!("tool-transform"),
            vec![
                Action::MoveMode,
                Action::RotateMode,
                Action::ScaleMode,
                Action::DropToPlate,
                Action::CenterModel,
                Action::FitToPlate,
                Action::ResetRotation,
                Action::ResetScale,
                Action::ToggleAxes,
                Action::ToggleLock(Mode::Move),
                Action::ToggleLock(Mode::Rotate),
                Action::ToggleLock(Mode::Scale),
                Action::ToggleUniform,
            ],
        ),
        (
            tr!("settings-shortcuts-section"),
            SectionStep::ALL.map(Action::Section).to_vec(),
        ),
        (
            tr!("settings-shortcuts-strip"),
            vec![
                Action::Place,
                Action::ToggleSurface,
                Action::ToggleAvoid,
                Action::ToggleKnee,
                Action::ToggleMini,
                Action::JoinSupport,
                Action::GridSupports,
                Action::MergeTrunks,
                Action::ToggleCavityView,
                Action::ToggleList,
                Action::ToggleTips,
                Action::Show,
                Action::AutoBraces,
                Action::ToggleIslands,
                Action::ToggleMinima,
                Action::SupportIslands,
                Action::ToggleLeft,
                Action::ShowCups,
            ],
        ),
        (
            tr!("settings-shortcuts-steps"),
            Step::ALL.map(Action::Step).to_vec(),
        ),
        (
            tr!("settings-shortcuts-tools"),
            Tool::ALL.map(Action::Tool).to_vec(),
        ),
    ]
}

impl MainWindow {
    /// Open the settings dialog (File › Settings…).
    pub(super) fn open_settings(&mut self) {
        self.settings_dialog
            .get_or_insert_with(SettingsDialog::default);
    }

    /// The settings dialog, while it is open.
    pub(super) fn settings_window(&mut self, ctx: &egui::Context) {
        let Some(mut dialog) = self.settings_dialog.take() else {
            return;
        };
        if dialog.show(ctx, self) {
            self.settings_dialog = Some(dialog);
        }
    }
}

impl SettingsDialog {
    /// Show the dialog over `window`; returns whether it stays open.
    fn show(&mut self, ctx: &egui::Context, window: &mut MainWindow) -> bool {
        // The keys go to the shortcut being changed before anything else sees them: Escape
        // cancels it rather than closing the dialog.
        self.capture(ctx, window);
        let file = window.settings_file();
        let mut close = false;
        let response =
            dialog::show(
                ctx,
                "settings",
                &tr!("settings-title"),
                SIZE,
                |ui, part| match part {
                    Part::Body => {
                        banner(ui, file.as_ref());
                        ui.horizontal_top(|ui| {
                            self.nav(ui);
                            ui.separator();
                            crate::focus::scrolled(
                                egui::ScrollArea::vertical()
                                    .id_salt(("settings", self.section as u8))
                                    .auto_shrink([false, false]),
                                ui,
                                |ui| {
                                    // Top-down, whatever the row around it lays out.
                                    let down = egui::Layout::top_down(egui::Align::Min);
                                    ui.with_layout(down, |ui| {
                                        ui.set_width(ui.available_width());
                                        self.section(ui, window, file.as_ref());
                                    });
                                },
                            );
                        });
                    }
                    Part::Footer { enter } => {
                        close = dialog::main_action(ui, &tr!("action-close"), true, enter);
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            ui.label(
                                egui::RichText::new(tr!("settings-applied"))
                                    .small()
                                    .color(ui.visuals().weak_text_color()),
                            );
                        });
                    }
                },
            );
        if self.starting_over {
            self.confirm_start_over(ctx, window, file.as_ref());
        }
        !(close || response.cancel)
    }

    fn nav(&mut self, ui: &mut egui::Ui) {
        ui.allocate_ui_with_layout(
            egui::vec2(NAV_WIDTH, ui.available_height()),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.set_width(NAV_WIDTH);
                for section in Section::ALL {
                    let entry = egui::Button::selectable(
                        self.section == section,
                        (section.label(), egui::Atom::grow()),
                    );
                    if ui.add_sized([NAV_WIDTH, 28.0], entry).clicked() {
                        self.section = section;
                        self.capturing = None;
                        self.conflict = None;
                    }
                }
            },
        );
    }

    fn section(&mut self, ui: &mut egui::Ui, window: &mut MainWindow, file: Option<&SettingsFile>) {
        ui.label(
            egui::RichText::new(self.section.label())
                .strong()
                .size(14.0),
        );
        ui.add_space(6.0);
        match self.section {
            Section::Appearance => self.appearance(ui, &mut window.appearance),
            Section::Shortcuts => self.shortcuts(ui, window),
            Section::Hollowing => hollowing(ui, window),
            Section::Supports => supports(ui, window),
            Section::Models => models(ui, window),
            Section::Window => self.window(ui, window),
            Section::Language => language(ui, window),
            Section::File => self.file(ui, window, file),
        }
    }

    fn appearance(&mut self, ui: &mut egui::Ui, appearance: &mut Appearance) {
        note(ui, &tr!("settings-appearance-note"));
        crate::widgets::group(ui, tr!("settings-lighting"), |ui| {
            egui::Grid::new("settings-lighting")
                .num_columns(2)
                .spacing([10.0, 6.0])
                .show(ui, |ui| {
                    percent_row(
                        ui,
                        &tr!("settings-brightness"),
                        &tr!("settings-brightness-hint"),
                        &mut appearance.brightness_percent,
                        &crate::appearance::BRIGHTNESS_RANGE,
                    );
                    percent_row(
                        ui,
                        &tr!("settings-ambient"),
                        &tr!("settings-ambient-hint"),
                        &mut appearance.ambient_percent,
                        &crate::appearance::AMBIENT_RANGE,
                    );
                });
        });
        for (group, ids) in GROUPS {
            crate::widgets::group(ui, group_label(group), |ui| {
                self.colours(ui, group, ids, appearance);
                if group == "status" {
                    status_contrast(ui, &appearance.status);
                }
            });
        }
        ui.add_space(6.0);
        let more = egui::collapsing_header::CollapsingState::load_with_default_open(
            ui.ctx(),
            egui::Id::new("settings-more-colours"),
            self.more_colours,
        );
        more.show_header(ui, |ui| {
            let title = tr!("settings-colours-more", count = MORE.len());
            crate::widgets::name_id(ui.ctx(), egui::Id::new("settings-more-colours"), &title);
            ui.label(egui::RichText::new(title).strong());
        })
        .body_unindented(|ui| self.colours(ui, "more", &MORE, appearance));
        crate::widgets::group(ui, tr!("settings-display"), |ui| {
            ui.horizontal(|ui| {
                let outline = switch(ui, &mut appearance.show_selection_outline);
                crate::widgets::name(&outline, &tr!("settings-outline"));
                ui.label(tr!("settings-outline"));
            });
            ui.horizontal(|ui| {
                let line = switch(ui, &mut appearance.show_contact_line);
                crate::widgets::name(&line, &tr!("settings-contact-line"));
                ui.label(tr!("settings-contact-line"))
                    .on_hover_text(tr!("settings-contact-line-hint"));
            });
            ui.add_space(4.0);
            ui.label(tr!("settings-outside"));
            egui::Grid::new("settings-outside")
                .num_columns(3)
                .spacing([12.0, 6.0])
                .show(ui, |ui| {
                    for (name, style) in [
                        (
                            tr!("settings-colour-object"),
                            &mut appearance.outside_filter,
                        ),
                        (
                            tr!("settings-colour-selected"),
                            &mut appearance.outside_filter_selected,
                        ),
                    ] {
                        filter_row(ui, &name, style);
                    }
                });
        });
        ui.add_space(10.0);
        if ui.button(tr!("settings-colours-reset")).clicked() {
            *appearance = Appearance::default();
        }
    }

    /// Rows of colours, two to a line: each its name, its picker and its hexadecimal value.
    fn colours(
        &mut self,
        ui: &mut egui::Ui,
        group: &str,
        ids: &[&'static str],
        a: &mut Appearance,
    ) {
        egui::Grid::new(("settings-colours", group))
            .num_columns(6)
            .spacing([10.0, 6.0])
            .show(ui, |ui| {
                for pair in ids.chunks(2) {
                    for &id in pair {
                        if let Some(colour) = colour(a, id) {
                            ui.label(colour_label(id));
                            self.colour_row(ui, id, colour);
                        }
                    }
                    ui.end_row();
                }
            });
    }

    fn colour_row(&mut self, ui: &mut egui::Ui, id: &'static str, colour: &mut Rgba) {
        // egui's picker passes the colour through HSV every frame, which moves a translucent
        // colour's channels by one untouched: written back only when the user changes it.
        let mut edited = colour.0;
        let name = colour_label(id);
        let well = ui.color_edit_button_srgba_unmultiplied(&mut edited);
        crate::widgets::name(&well, &tr!("a11y-colour", name = name.as_str()));
        if well.changed() {
            colour.0 = edited;
        }
        let typing = self.hex.as_ref().is_some_and(|(field, _)| *field == id);
        let mut text = if typing {
            self.hex
                .as_ref()
                .map(|(_, t)| t.clone())
                .unwrap_or_default()
        } else {
            hex(*colour)
        };
        // Its own width, whatever the grid's column was last frame: a field that takes the
        // space available narrows a little each frame and cuts the value.
        let size = egui::vec2(HEX_FIELD, ui.spacing().interact_size.y);
        let field = ui.add_sized(
            size,
            text_field(&mut text, HEX_FIELD).font(egui::TextStyle::Monospace),
        );
        crate::widgets::name(&field, &tr!("a11y-hex", name = name.as_str()));
        if field.gained_focus() || (field.has_focus() && !typing) {
            self.hex = Some((id, text.clone()));
        }
        if field.changed() {
            if let Some(parsed) = parse_hex(&text) {
                *colour = parsed;
            }
            self.hex = Some((id, text));
        }
        if field.lost_focus() && typing {
            self.hex = None;
        }
    }

    /// The key pressed for the shortcut being changed, if any: Escape alone cancels; a
    /// shortcut another action has asks first.
    fn capture(&mut self, ctx: &egui::Context, window: &mut MainWindow) {
        let Some(action) = self.capturing else {
            return;
        };
        let pressed = ctx.input_mut(|input| {
            let mut found = None;
            input.events.retain(|event| match event {
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } if found.is_none() => {
                    found = Some((*key, *modifiers));
                    false
                }
                _ => true,
            });
            found
        });
        let Some((key, modifiers)) = pressed else {
            return;
        };
        self.capturing = None;
        if key == egui::Key::Escape && modifiers.is_none() {
            return;
        }
        let new = shortcut(key, modifiers.command, modifiers.shift, modifiers.alt);
        match window.keymap.owner(new).filter(|&owner| owner != action) {
            Some(owner) => self.conflict = Some((action, new, owner)),
            None => {
                window.keymap.set(action, new);
            }
        }
    }

    /// The shortcut another action has, waiting for the user's yes.
    fn conflict_box(&mut self, ui: &mut egui::Ui, window: &mut MainWindow) {
        let ctx = ui.ctx().clone();
        if let Some((action, new, owner)) = self.conflict {
            ui.add_space(6.0);
            egui::Frame::new()
                .fill(theme::WARNING_FILL.color())
                .corner_radius(6.0)
                .inner_margin(10.0)
                .show(ui, |ui| {
                    ui.add(
                        egui::Label::new(tr!(
                            "settings-shortcuts-conflict",
                            keys = ctx.format_shortcut(&new),
                            owner = owner.label(),
                            action = action.label()
                        ))
                        .wrap(),
                    );
                    ui.horizontal(|ui| {
                        if ui.button(tr!("settings-shortcuts-use-here")).clicked() {
                            window.keymap.set(action, new);
                            self.conflict = None;
                        }
                        if ui.button(tr!("action-cancel")).clicked() {
                            self.conflict = None;
                        }
                    });
                });
        }
    }

    fn shortcuts(&mut self, ui: &mut egui::Ui, window: &mut MainWindow) {
        note(ui, &tr!("settings-shortcuts-note"));
        ui.horizontal(|ui| {
            let search = ui.add(
                text_field(&mut self.search, 240.0).hint_text(tr!("settings-shortcuts-search")),
            );
            crate::widgets::name(&search, &tr!("settings-shortcuts-search"));
            if ui.button(tr!("settings-shortcuts-reset-all")).clicked() {
                window.keymap = crate::actions::Keymap::default();
                self.capturing = None;
                self.conflict = None;
            }
        });
        let ctx = ui.ctx().clone();
        self.conflict_box(ui, window);
        let query = self.search.trim().to_lowercase();
        let defaults = crate::actions::Keymap::default();
        let icons = crate::icons::Icons::shared(&ctx);
        for (heading, actions) in shortcut_groups() {
            let rows: Vec<(Action, String)> = actions
                .into_iter()
                .map(|action| {
                    let keys: Vec<String> = window
                        .keymap
                        .shortcuts(action)
                        .map(|s| ctx.format_shortcut(&s))
                        .collect();
                    (action, keys.join(" · "))
                })
                .filter(|(action, keys)| {
                    query.is_empty()
                        || action.label().to_lowercase().contains(&query)
                        || keys.to_lowercase().contains(&query)
                })
                .collect();
            if rows.is_empty() {
                continue;
            }
            // A group as the other settings' sections (R3), the action's name at the left.
            crate::widgets::group(ui, heading, |ui| {
                egui::Grid::new(("settings-keys", rows[0].0.id()))
                    .num_columns(3)
                    .spacing([12.0, 4.0])
                    .min_col_width(0.0)
                    .show(ui, |ui| {
                        for (action, keys) in rows {
                            action_name(ui, action);
                            let capturing = self.capturing == Some(action);
                            let text = if capturing {
                                egui::RichText::new(tr!("settings-shortcuts-press"))
                                    .color(theme::ACCENT.color())
                            } else if keys.is_empty() {
                                egui::RichText::new(tr!("settings-shortcuts-none"))
                                    .color(ui.visuals().weak_text_color())
                            } else {
                                egui::RichText::new(keys).monospace()
                            };
                            let button =
                                egui::Button::selectable(capturing, (text, egui::Atom::grow()));
                            if ui
                                .add_sized([220.0, 26.0], button)
                                .on_hover_text(tr!("settings-shortcuts-change"))
                                .clicked()
                            {
                                self.capturing = Some(action);
                                self.conflict = None;
                            }
                            let default: Vec<_> = defaults.shortcuts(action).collect();
                            let mut now: Vec<_> = window.keymap.shortcuts(action).collect();
                            now.retain(|s| !default.contains(s));
                            let changed = !now.is_empty()
                                || window.keymap.shortcuts(action).count() != default.len();
                            let reset = ui
                                .add_enabled_ui(changed, |ui| {
                                    icon_button(
                                        ui,
                                        icons.as_deref(),
                                        Icon::Reset,
                                        false,
                                        &tr!("settings-shortcuts-reset"),
                                    )
                                })
                                .inner;
                            if reset.clicked() {
                                window.keymap.reset(action);
                            }
                            ui.end_row();
                        }
                    });
            });
        }
    }

    fn window(&mut self, ui: &mut egui::Ui, window: &mut MainWindow) {
        let open_as = &mut window.window_settings.open_as;
        ui.horizontal(|ui| {
            ui.label(tr!("settings-window-opens"));
            let combo = egui::ComboBox::from_id_salt("settings-open-as")
                .selected_text(open_as_label(*open_as))
                .width(220.0)
                .show_ui(ui, |ui| {
                    for choice in [
                        OpenAs::AsClosed,
                        OpenAs::Maximized,
                        OpenAs::Fullscreen,
                        OpenAs::Windowed,
                    ] {
                        ui.selectable_value(open_as, choice, open_as_label(choice));
                    }
                });
            crate::widgets::name(&combo.response, &tr!("settings-window-opens"));
        });
        note(ui, &tr!("settings-window-option"));
        ui.add_space(8.0);
        ui.separator();
        ui.add(egui::Label::new(tr!("settings-window-places")).wrap());
        if ui.button(tr!("settings-window-forget")).clicked() {
            Remembered::change(ui.ctx(), |r| {
                r.dialogs.clear();
                r.windows.clear();
            });
            self.places_forgotten = true;
        }
        if self.places_forgotten {
            note(ui, &tr!("settings-window-forgotten"));
        }
    }

    fn file(&mut self, ui: &mut egui::Ui, window: &mut MainWindow, file: Option<&SettingsFile>) {
        let Some(file) = file else {
            note(ui, &tr!("settings-file-none"));
            return;
        };
        egui::Grid::new("settings-file")
            .num_columns(2)
            .spacing([12.0, 6.0])
            .show(ui, |ui| {
                ui.label(tr!("settings-file-path"));
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(file.path.display().to_string()).monospace(),
                    )
                    .wrap(),
                );
                ui.end_row();
                ui.label(tr!("settings-file-state"));
                let (text, colour) = file_state(file);
                ui.add(egui::Label::new(egui::RichText::new(text).color(colour)).wrap());
                ui.end_row();
            });
        ui.add_space(6.0);
        note(ui, &file_note(file));
        ui.horizontal(|ui| {
            if ui.button(tr!("settings-file-open-folder")).clicked() {
                if let Some(folder) = file.path.parent() {
                    open_folder(folder, window);
                }
            }
            let reset =
                egui::RichText::new(tr!("settings-file-start-over")).color(theme::ERROR.color());
            if ui.button(reset).clicked() {
                self.starting_over = true;
            }
        });
    }

    fn confirm_start_over(
        &mut self,
        ctx: &egui::Context,
        window: &mut MainWindow,
        file: Option<&SettingsFile>,
    ) {
        let text = match file.map(|f| &f.opened) {
            Some(crate::settings::Opened::Unreadable(_)) => tr!("settings-start-over-unreadable"),
            Some(crate::settings::Opened::Newer(_)) => tr!("settings-start-over-newer"),
            _ => tr!("settings-start-over-text"),
        };
        match dialog::confirm(
            ctx,
            "settings-start-over",
            &tr!("settings-start-over-title"),
            &text,
            &tr!("settings-start-over-action"),
            true,
        ) {
            Some(true) => {
                self.starting_over = false;
                self.capturing = None;
                self.conflict = None;
                window.start_over_settings(ctx);
            }
            Some(false) => self.starting_over = false,
            None => {}
        }
    }
}

fn filter_row(ui: &mut egui::Ui, name: &str, style: &mut FilterStyle) {
    ui.label(name);
    let mut edited = style.color.0;
    let well = ui.color_edit_button_srgba_unmultiplied(&mut edited);
    crate::widgets::name(&well, &tr!("a11y-colour", name = name));
    if well.changed() {
        style.color.0 = edited;
    }
    let combo = egui::ComboBox::from_id_salt(("settings-pattern", name))
        .selected_text(pattern_label(style.pattern))
        .show_ui(ui, |ui| {
            for pattern in [Pattern::Solid, Pattern::Stripes] {
                ui.selectable_value(&mut style.pattern, pattern, pattern_label(pattern));
            }
        });
    crate::widgets::name(&combo.response, &tr!("a11y-pattern", name = name));
    ui.end_row();
}

fn pattern_label(pattern: Pattern) -> String {
    match pattern {
        Pattern::Solid => tr!("settings-pattern-solid"),
        Pattern::Stripes => tr!("settings-pattern-stripes"),
    }
}

fn open_as_label(open_as: OpenAs) -> String {
    match open_as {
        OpenAs::AsClosed => tr!("settings-open-as-closed"),
        OpenAs::Maximized => tr!("settings-open-maximized"),
        OpenAs::Fullscreen => tr!("settings-open-fullscreen"),
        OpenAs::Windowed => tr!("settings-open-windowed"),
    }
}

/// The supports' settings (TD-107): the critical angle, and whether the models' overhangs show.
/// What the Orient tool's searches start from (TD-119).
fn orient_defaults(ui: &mut egui::Ui, window: &mut MainWindow) {
    crate::widgets::group(ui, tr!("settings-orient"), |ui| {
        note(ui, &tr!("settings-defaults-note"));
        crate::orient_ui::panel::orient_fields(ui, &mut window.tool_defaults.orient, true, None);
        ui.add_space(8.0);
    });
}

#[allow(clippy::too_many_lines)] // One group of defaults per supports tool, in order.
fn supports(ui: &mut egui::Ui, window: &mut MainWindow) {
    let old = window.default_tool_values();
    note(ui, &tr!("settings-supports-note"));
    crate::widgets::group(ui, tr!("settings-colours-overhangs"), |ui| {
        ui.horizontal(|ui| {
            ui.label(tr!("settings-overhang-angle"));
            let angle = ui
                .add(
                    crate::widgets::UnitField::new(
                        &mut window.overhangs.angle_deg,
                        crate::widgets::Unit::Degree,
                    )
                    .range(crate::settings::OVERHANG_ANGLE_RANGE)
                    .step(1.0)
                    .decimals(0),
                )
                .on_hover_text(tr!("settings-overhang-angle-hint"));
            crate::widgets::name(&angle, &tr!("settings-overhang-angle"));
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let shown = switch(ui, &mut window.overhangs.shown);
            crate::widgets::name(&shown, &tr!("settings-overhang-show"));
            ui.label(tr!("settings-overhang-show"));
        });
        note(ui, &tr!("settings-overhang-show-note"));
    });
    orient_defaults(ui, window);
    crate::widgets::group(ui, tr!("settings-new-supports"), |ui| {
        note(ui, &tr!("settings-defaults-note"));
        ui.label(tr!("supports-preset"));
        let label = window
            .supports
            .default_preset
            .and_then(|i| window.supports.presets.get(i))
            .map_or_else(
                || window.tool_defaults.support_weight.label(),
                |p| p.name.clone(),
            );
        let combo = egui::ComboBox::from_id_salt("settings-support-preset")
            .width(220.0)
            .truncate()
            .selected_text(label)
            .show_ui(ui, |ui| {
                for weight in crate::supports_ui::Weight::ALL {
                    let chosen = window.supports.default_preset.is_none()
                        && window.tool_defaults.support_weight == weight;
                    if ui.selectable_label(chosen, weight.label()).clicked() {
                        window.tool_defaults.support_weight = weight;
                        window.supports.default_preset = None;
                    }
                }
                for i in 0..window.supports.presets.len() {
                    let name = window.supports.presets[i].name.clone();
                    let chosen = window.supports.default_preset == Some(i);
                    if ui.selectable_label(chosen, name).clicked() {
                        window.supports.default_preset = Some(i);
                    }
                }
            })
            .response
            .on_hover_text(tr!("settings-support-weight-hint"));
        crate::widgets::name(&combo, &tr!("supports-preset"));
        // Whether new supports' contacts fit the surface (TD-116), at most the preset's contact.
        let contact = window
            .supports
            .default_preset
            .and_then(|i| window.supports.presets.get(i))
            .map_or_else(
                || crate::supports_ui::SupportValues::of(window.tool_defaults.support_weight),
                |p| p.values,
            )
            .contact_diameter_mm;
        crate::supports_ui::panel::fit_fields(
            ui,
            &mut window.tool_defaults.support_contact_fit_min_mm,
            contact,
            None,
        );
    });
    // What a raft turned on starts from; a model's raft keeps its own values.
    crate::widgets::group(ui, tr!("settings-new-raft"), |ui| {
        crate::raft_ui::raft_fields(ui, &mut window.tool_defaults.raft, true);
    });
    let presets = window.supports.presets.clone();
    // What the islands' supports start from.
    crate::widgets::group(ui, tr!("settings-island-supports"), |ui| {
        let mut island = window.tool_defaults.island_supports;
        island.mine = window.supports.islands_default_mine;
        if crate::islands_ui::panel::island_fields(ui, &mut island, &presets, true, None) {
            window.supports.islands_default_mine = island.mine;
            island.mine = None;
            window.tool_defaults.island_supports = island;
        }
    });
    // What the Automatic tool's plans start from.
    crate::widgets::group(ui, tr!("settings-auto-supports"), |ui| {
        let mut auto = window.tool_defaults.auto_supports;
        auto.mine = window.supports.automatic_default_mine;
        auto.cavity_mine = window.supports.automatic_cavity_default_mine;
        if crate::auto_ui::panel::default_fields(ui, &mut auto, &presets) {
            window.supports.automatic_default_mine = auto.mine;
            window.supports.automatic_cavity_default_mine = auto.cavity_mine;
            auto.mine = None;
            auto.cavity_mine = None;
            window.tool_defaults.auto_supports = auto;
        }
    });
    // What the Grid mode starts from (TD-138).
    crate::widgets::group(ui, tr!("settings-grid-supports"), |ui| {
        let mut grid = window.tool_defaults.grid;
        grid.mine = window.supports.grid_default_mine;
        if crate::grid_ui::panel::grid_fields(ui, &mut grid, &presets, true, None) {
            window.supports.grid_default_mine = grid.mine;
            grid.mine = None;
            window.tool_defaults.grid = grid;
        }
    });
    // What new braces start from; a placed brace keeps its own diameter.
    crate::widgets::group(ui, tr!("settings-new-braces"), |ui| {
        crate::supports_ui::braces::brace_fields(ui, &mut window.tool_defaults.braces, None);
    });
    // Values still at their old defaults take the new ones; those the user changed stay.
    if window.default_tool_values() != old {
        window.follow_defaults(&old);
    }
}

/// The values new hollowings and drain holes start from (owner, 2026-10-27): every tool opens
/// with them, and what is changed in a tool's panel is for the model at hand.
fn hollowing(ui: &mut egui::Ui, window: &mut MainWindow) {
    note(ui, &tr!("settings-defaults-note"));
    ui.add_space(6.0);
    let old = window.default_tool_values();
    let defaults = &mut window.tool_defaults;
    crate::widgets::group(ui, tr!("tool-hollow"), |ui| {
        crate::widgets::value_row(ui, tr!("settings-wall"), |ui| {
            ui.add(
                UnitField::new(&mut defaults.wall_mm, Unit::Millimetre)
                    .range(0.1..=20.0)
                    .step(0.05),
            )
        });
        let mut auto = defaults.cell_mm.is_none();
        let mut cell = defaults.cell_mm.unwrap_or(0.25);
        if crate::widgets::auto_value_row(ui, tr!("settings-cell"), &mut auto, |ui| {
            ui.add(
                UnitField::new(&mut cell, Unit::Millimetre)
                    .range(0.02..=1.0)
                    .step(0.01),
            )
        }) {
            defaults.cell_mm = (!auto).then_some(cell);
        }
        blocker_defaults(ui, &mut defaults.blocker);
    });
    crate::widgets::group(ui, tr!("tool-drain-holes"), |ui| {
        crate::widgets::value_row(ui, tr!("hole-diameter"), |ui| {
            ui.add(
                UnitField::new(&mut defaults.hole_diameter_mm, Unit::Millimetre)
                    .range(0.5..=20.0)
                    .step(0.05)
                    .decimals(1),
            )
        });
        let mut auto = defaults.hole_depth_mm.is_none();
        let mut depth = defaults.hole_depth_mm.unwrap_or(3.0);
        if crate::widgets::auto_value_row(ui, tr!("hole-depth"), &mut auto, |ui| {
            ui.add(
                UnitField::new(&mut depth, Unit::Millimetre)
                    .range(0.1..=100.0)
                    .step(0.05),
            )
        }) {
            defaults.hole_depth_mm = (!auto).then_some(depth);
        }
        ui.label(tr!("prepare-start"));
        let mut chosen = usize::from(defaults.holes_from_cavity);
        let labels = [tr!("prepare-start-outer"), tr!("prepare-start-cavity")];
        if crate::widgets::choice(ui, &mut chosen, &labels).changed() {
            defaults.holes_from_cavity = chosen == 1;
        }
        crate::widgets::value_row(ui, tr!("cups-min-vent"), |ui| {
            ui.add(
                UnitField::new(&mut defaults.min_vent_mm2, Unit::SquareMillimetre)
                    .range(crate::hollowing_ui::cups::MIN_VENT_MM2)
                    .step(0.05),
            )
            .on_hover_text(tr!("cups-min-vent-settings-hint"))
        });
    });
    if window.default_tool_values() != old {
        window.follow_defaults(&old);
    }
}

/// What new hollowing blockers take (owner, 2026-11-02).
fn blocker_defaults(ui: &mut egui::Ui, blocker: &mut crate::hollowing_ui::blockers::NewBlocker) {
    use crate::hollowing_ui::blockers::{BASE_MM, SIZE_MM, TOP_MM};
    ui.add_space(4.0);
    ui.label(tr!("blockers-next"));
    let mut chosen = usize::from(blocker.sphere);
    let labels = [tr!("blockers-box"), tr!("blockers-sphere")];
    if crate::widgets::choice(ui, &mut chosen, &labels).changed() {
        blocker.sphere = chosen == 1;
    }
    let row = |ui: &mut egui::Ui, label: String, value: &mut f64, range| {
        crate::widgets::value_row(ui, label, |ui| {
            ui.add(
                UnitField::new(value, Unit::Millimetre)
                    .range(range)
                    .step(0.1),
            )
        });
    };
    if blocker.sphere {
        row(
            ui,
            tr!("blockers-diameter"),
            &mut blocker.diameter_mm,
            SIZE_MM,
        );
    } else {
        row(
            ui,
            tr!("blockers-bottom-width"),
            &mut blocker.bottom_mm[0],
            SIZE_MM,
        );
        row(
            ui,
            tr!("blockers-bottom-depth"),
            &mut blocker.bottom_mm[1],
            SIZE_MM,
        );
        row(
            ui,
            tr!("blockers-top-width"),
            &mut blocker.top_mm[0],
            TOP_MM,
        );
        row(
            ui,
            tr!("blockers-top-depth"),
            &mut blocker.top_mm[1],
            TOP_MM,
        );
        row(ui, tr!("blockers-height"), &mut blocker.height_mm, SIZE_MM);
    }
    row(ui, tr!("blockers-base"), &mut blocker.base_mm, BASE_MM);
}

/// A grid row for a percentage: its name, the app's slider and its value field, both setting it.
fn percent_row(
    ui: &mut egui::Ui,
    label: &str,
    hint: &str,
    value: &mut u16,
    range: &std::ops::RangeInclusive<u16>,
) {
    ui.label(label).on_hover_text(hint);
    ui.horizontal(|ui| {
        let mut slid = usize::from(*value);
        let span = usize::from(*range.start())..=usize::from(*range.end());
        let slider = range_slider(ui, &mut slid, span, PERCENT_SLIDER).on_hover_text(hint);
        crate::widgets::name(&slider, label);
        if slider.changed() {
            *value = u16::try_from(slid).unwrap_or(*range.end());
        }
        let mut typed = f64::from(*value);
        let field = ui.add(
            UnitField::new(&mut typed, Unit::Percent)
                .range(f64::from(*range.start())..=f64::from(*range.end()))
                .step(1.0)
                .decimals(0),
        );
        crate::widgets::name(&field, label);
        if field.changed() {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // In range.
            {
                *value = typed.round() as u16;
            }
        }
    });
    ui.end_row();
}

/// The repairs every opened model gets, one choice each with what it does.
fn models(ui: &mut egui::Ui, window: &mut MainWindow) {
    note(ui, &tr!("settings-models-note"));
    let chosen = window.workspace.auto_repair();
    for (preset, note_text) in [
        (None, tr!("settings-repair-none")),
        (Some(RepairPreset::Exact), tr!("settings-repair-exact")),
        (
            Some(RepairPreset::Standard),
            tr!("settings-repair-standard"),
        ),
        (
            Some(RepairPreset::Aggressive),
            tr!("settings-repair-aggressive"),
        ),
    ] {
        let name = preset.map_or_else(
            || tr!("settings-repair-none-name"),
            |p| crate::repair_panel::preset_name(Some(p)),
        );
        ui.add_space(4.0);
        if ui
            .radio(chosen == preset, egui::RichText::new(name).strong())
            .clicked()
        {
            window.workspace.set_auto_repair(preset);
        }
        ui.indent(("settings-repair", note_text.len()), |ui| {
            note(ui, &note_text);
        });
    }
    // The recovery copy of a project with unsaved changes (TD-130).
    crate::widgets::group(ui, tr!("settings-projects"), |ui| {
        let minutes = window.project_file.recovery_minutes();
        let mut value = f64::from(*minutes);
        crate::widgets::value_row(ui, tr!("settings-recovery-minutes"), |ui| {
            ui.add(
                UnitField::new(&mut value, Unit::Count)
                    .range(0.0..=f64::from(*crate::settings::RECOVERY_MINUTES.end()))
                    .step(1.0)
                    .decimals(0),
            )
        });
        // A whole number of minutes within the range the field keeps.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        {
            *minutes = value.round() as u32;
        }
        note(ui, &tr!("settings-recovery-note"));
    });
}

/// The interface's languages: the bundled ones, and the pseudo-locale in developer mode.
fn language(ui: &mut egui::Ui, window: &mut MainWindow) {
    let mut locales: Vec<String> = crate::i18n::available(window.launch.locale_dir.as_deref())
        .into_iter()
        .filter(|l| l != crate::i18n::PSEUDO || window.launch.dev)
        .collect();
    locales.sort();
    let current = window
        .language
        .clone()
        .unwrap_or_else(|| crate::i18n::REFERENCE.to_owned());
    for locale in locales {
        let name = match locale.as_str() {
            crate::i18n::REFERENCE => tr!("settings-language-en-us"),
            crate::i18n::PSEUDO => tr!("settings-language-pseudo"),
            other => other.to_owned(),
        };
        if ui.radio(current == locale, name).clicked() && current != locale {
            let chosen = (locale != crate::i18n::REFERENCE).then_some(locale);
            window.set_language(chosen);
        }
    }
    ui.add_space(6.0);
    note(ui, &tr!("settings-language-note"));
    if window.launch.locale_fixed {
        note(
            ui,
            &tr!(
                "settings-language-fixed",
                locale = window.launch.locale.clone()
            ),
        );
    }
}

/// The settings file's state, and its colour.
fn file_state(file: &SettingsFile) -> (String, egui::Color32) {
    match &file.opened {
        crate::settings::Opened::Missing | crate::settings::Opened::Read => {
            (tr!("settings-file-ok"), theme::SUCCESS.color())
        }
        crate::settings::Opened::Unreadable(reason) => (
            tr!("settings-file-unreadable", reason = reason.clone()),
            theme::ERROR.color(),
        ),
        crate::settings::Opened::Newer(schema) => (
            tr!("settings-file-newer", schema = *schema),
            theme::WARNING.color(),
        ),
    }
}

fn file_note(file: &SettingsFile) -> String {
    match &file.opened {
        crate::settings::Opened::Missing | crate::settings::Opened::Read => {
            tr!("settings-file-note-ok")
        }
        crate::settings::Opened::Unreadable(_) => tr!("settings-file-note-unreadable"),
        crate::settings::Opened::Newer(_) => tr!("settings-file-note-newer"),
    }
}

/// The strip over the sections while the file is not saved to: why.
fn banner(ui: &mut egui::Ui, file: Option<&SettingsFile>) {
    let (text, background) = match file.map(|f| &f.opened) {
        Some(crate::settings::Opened::Unreadable(_)) => {
            (tr!("settings-unreadable"), theme::ERROR_FILL.color())
        }
        Some(crate::settings::Opened::Newer(_)) => {
            (tr!("settings-newer"), theme::WARNING_FILL.color())
        }
        _ => return,
    };
    egui::Frame::new()
        .fill(background)
        .corner_radius(6.0)
        .inner_margin(8.0)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.add(egui::Label::new(text).wrap());
        });
    ui.add_space(6.0);
}

/// Small weak text that wraps: what a choice means.
fn note(ui: &mut egui::Ui, text: &str) {
    ui.add(
        egui::Label::new(
            egui::RichText::new(text)
                .small()
                .color(ui.visuals().weak_text_color()),
        )
        .wrap(),
    );
}

/// Show `folder` in the system's file manager; a failure is a notification.
fn open_folder(folder: &std::path::Path, window: &mut MainWindow) {
    if let Err(error) = crate::desktop::open_folder(folder) {
        window.messages.push(
            Message::new(Level::Warning, tr!("settings-file-open-failed"))
                .detail(error.to_string()),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_action_is_listed_once_among_the_shortcuts() {
        let listed: Vec<Action> = shortcut_groups()
            .into_iter()
            .flat_map(|(_, actions)| actions)
            .collect();
        let mut sorted = listed.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), listed.len(), "an action is listed twice");
        let missing: Vec<Action> = Action::all()
            .into_iter()
            .filter(|action| !listed.contains(action))
            .collect();
        assert_eq!(missing, Vec::new());
        // Every default shortcut, Ctrl+Page Up's previous plate among them, can be changed.
        assert!(listed.contains(&Action::PreviousPlate) && listed.contains(&Action::NextPlate));
        assert_eq!(listed.len(), Action::all().len());
    }

    fn key(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        }
    }

    /// One frame of the dialog over `window`, with `events`; the text drawn and the dialog's
    /// area. Returns whether it stays open.
    fn frame(
        ctx: &egui::Context,
        dialog: &mut SettingsDialog,
        window: &mut MainWindow,
        events: Vec<egui::Event>,
    ) -> (bool, Vec<egui::Rect>) {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1600.0, 1000.0),
            )),
            events,
            ..egui::RawInput::default()
        };
        let mut open = true;
        let mut output = ctx.run_ui(input, |ui| open = dialog.show(ui.ctx(), window));
        output.textures_delta.clear();
        let texts = output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text) => Some(
                    text.galley
                        .rect
                        .translate(text.pos.to_vec2())
                        .intersect(clipped.clip_rect),
                ),
                _ => None,
            })
            .filter(egui::Rect::is_positive)
            .collect();
        (open, texts)
    }

    #[test]
    fn every_section_draws_inside_the_dialog() {
        let mut window = MainWindow::new(Vec::new());
        window.launch.dev = true;
        for section in Section::ALL {
            let ctx = egui::Context::default();
            let mut dialog = SettingsDialog {
                section,
                more_colours: true,
                ..SettingsDialog::default()
            };
            let mut texts = Vec::new();
            for _ in 0..4 {
                texts = frame(&ctx, &mut dialog, &mut window, Vec::new()).1;
            }
            let area = ctx
                .memory(|m| m.area_rect(egui::Id::new("settings")))
                .unwrap();
            assert!(area.width() <= 920.0 + 24.0, "{section:?}: {area:?}");
            for text in texts {
                assert!(
                    area.expand(1.0).contains_rect(text),
                    "{section:?}: {text:?} outside {area:?}"
                );
            }
        }
    }

    #[test]
    fn every_colour_shows_its_whole_hexadecimal_value() {
        let mut window = MainWindow::new(Vec::new());
        let ctx = egui::Context::default();
        let mut dialog = SettingsDialog {
            more_colours: true,
            ..SettingsDialog::default()
        };
        let mut hexes = Vec::new();
        for _ in 0..4 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1600.0, 1000.0),
                )),
                ..egui::RawInput::default()
            };
            let mut output = ctx.run_ui(input, |ui| {
                dialog.show(ui.ctx(), &mut window);
            });
            output.textures_delta.clear();
            hexes = output
                .shapes
                .iter()
                .filter_map(|clipped| match &clipped.shape {
                    egui::Shape::Text(text) if text.galley.text().starts_with('#') => Some((
                        text.galley.text().to_owned(),
                        text.galley.rect.translate(text.pos.to_vec2()),
                        clipped.clip_rect,
                    )),
                    _ => None,
                })
                .collect();
        }
        // The groups' frames and gaps leave fewer rows in the first view than before.
        assert!(hexes.len() >= 15, "{}", hexes.len());
        // Across: a field cut at the scroll area's top or bottom is only out of view.
        for (text, rect, clip) in hexes {
            assert!(
                rect.min.x >= clip.min.x - 0.5 && rect.max.x <= clip.max.x + 0.5,
                "{text} is cut: {rect:?} in {clip:?}"
            );
        }
    }

    #[test]
    fn every_settings_page_names_its_controls_and_keeps_the_focus() {
        let mut report = Vec::new();
        for section in Section::ALL {
            let mut window = MainWindow::new(Vec::new());
            window.settings_dialog = Some(SettingsDialog {
                section,
                ..SettingsDialog::default()
            });
            let frame = eframe::Frame::_new_kittest();
            report.extend(
                crate::accessibility_check::failures(|ui| window.show(ui, &frame, None))
                    .into_iter()
                    .map(|f| format!("{section:?}: {f}")),
            );
        }
        assert!(report.is_empty(), "{}", report.join("\n"));
    }

    #[test]
    fn a_new_shortcut_is_taken_from_the_keys_and_another_actions_asks_first() {
        let ctx = egui::Context::default();
        let mut window = MainWindow::new(Vec::new());
        let mut dialog = SettingsDialog {
            section: Section::Shortcuts,
            capturing: Some(Action::FitView),
            ..SettingsDialog::default()
        };
        frame(&ctx, &mut dialog, &mut window, Vec::new());
        // Ctrl+D is free: Fit view takes it.
        let ctrl = egui::Modifiers::COMMAND;
        frame(
            &ctx,
            &mut dialog,
            &mut window,
            vec![key(egui::Key::D, ctrl)],
        );
        let ctrl_d = shortcut(egui::Key::D, true, false, false);
        assert_eq!(window.keymap.owner(ctrl_d), Some(Action::FitView));
        assert_eq!(dialog.capturing, None);
        // H is Hide's: asked first, nothing changed yet.
        dialog.capturing = Some(Action::Isolate);
        let none = egui::Modifiers::NONE;
        frame(
            &ctx,
            &mut dialog,
            &mut window,
            vec![key(egui::Key::H, none)],
        );
        let h = shortcut(egui::Key::H, false, false, false);
        assert_eq!(dialog.conflict, Some((Action::Isolate, h, Action::Hide)));
        assert_eq!(window.keymap.owner(h), Some(Action::Hide));
        // Escape while waiting for keys cancels them, and the dialog stays.
        dialog.conflict = None;
        dialog.capturing = Some(Action::Isolate);
        let (open, _) = frame(
            &ctx,
            &mut dialog,
            &mut window,
            vec![key(egui::Key::Escape, none)],
        );
        assert!(open);
        assert_eq!(dialog.capturing, None);
        // Escape then closes it, as any dialog.
        let (open, _) = frame(
            &ctx,
            &mut dialog,
            &mut window,
            vec![key(egui::Key::Escape, none)],
        );
        assert!(!open);
    }

    #[test]
    fn colours_read_and_write_as_hexadecimal() {
        assert_eq!(hex(Rgba([0xB8, 0xB0, 0xA8, 0xFF])), "#B8B0A8");
        assert_eq!(hex(Rgba([0x1C, 0x1C, 0x22, 0x33])), "#1C1C2233");
        assert_eq!(parse_hex("#b8b0a8"), Some(Rgba([0xB8, 0xB0, 0xA8, 0xFF])));
        assert_eq!(
            parse_hex(" 1c1c2233 "),
            Some(Rgba([0x1C, 0x1C, 0x22, 0x33]))
        );
        for bad in ["", "#12345", "#1234567", "#GGGGGG", "#B8B0A8FF00"] {
            assert_eq!(parse_hex(bad), None, "{bad}");
        }
        // Every colour named in the dialog is one of the appearance's.
        let mut appearance = Appearance::default();
        for id in GROUPS
            .iter()
            .flat_map(|(_, ids)| ids.iter())
            .chain(MORE.iter())
        {
            assert!(colour(&mut appearance, id).is_some(), "{id}");
        }
    }

    #[test]
    fn starting_over_keeps_the_damaged_file_and_saves_the_defaults() {
        let folder =
            std::env::temp_dir().join(format!("karasu-settings-dialog-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join(crate::settings::FILE_NAME);
        std::fs::write(&path, b"{ damaged").unwrap();
        let ctx = egui::Context::default();
        let mut window = MainWindow::new(Vec::new());
        window.load_settings(&ctx, Some(path.clone()));
        window
            .keymap
            .set(Action::FitView, shortcut(egui::Key::D, true, false, false));
        window.start_over_settings(&ctx);
        assert_eq!(window.keymap, crate::actions::Keymap::default());
        assert_eq!(
            std::fs::read(folder.join("settings.json.unreadable")).unwrap(),
            b"{ damaged"
        );
        let (_, _, opened) = crate::settings::Store::open(path);
        assert_eq!(opened, crate::settings::Opened::Read);
        let _ = std::fs::remove_dir_all(&folder);
    }
}
