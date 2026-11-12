//! The Slice and export tool panel (M6.5, TD-098), drawn from the owner-approved mockup.
//!
//! The panel is the plate's, not a model's: it needs no selection. Its body holds the output
//! (the printer, the resin and its layer height as facts, the file format, the plate's masks:
//! anti-aliasing, smallest gray and its supports' (TD-134), and whether slices verify the
//! printed walls), the plate's slicing options behind Advanced (its repairs and
//! tolerances, design rule 7), and the last slice's result with the printed walls' findings when
//! verified. Its fixed footer holds Slice and Export…, or the running slice's progress with
//! Cancel, then the slice's state. What a slice or an export did comes as a toast.
use super::{duration, facts, walls, SlicingUi};
use crate::{
    i18n::{decimal, tr},
    icons::{Icon, Icons},
    layer_plan_ui::Planner,
    repair_ui, theme,
    widgets::{action_pair, fact_row, state_line},
    workspace::Workspace,
};
use eframe::egui;
use karasu_project::{
    project::Freshness,
    request::{format_ids, format_name, REFERENCE_FORMAT},
};

/// The formats the plate's printer lists that this build can write, in the printer's order;
/// with `reference`, then the reference format, for checking a slice (TD-106: it is no
/// printer's format, so only the Advanced options offer it).
fn formats(workspace: &Workspace, reference: bool) -> Vec<String> {
    printer_formats(workspace.printer(), reference)
}

/// The formats `printer` lists that this build can write, in its order; with `reference`, then
/// the reference format.
fn printer_formats(
    printer: Option<&karasu_core::profiles::PrinterProfile>,
    reference: bool,
) -> Vec<String> {
    let supported: Vec<String> = format_ids().split(", ").map(str::to_owned).collect();
    let mut formats: Vec<String> = printer.map_or_default(|printer| {
        printer
            .format_ids
            .iter()
            .filter(|f| supported.contains(f) && f.as_str() != REFERENCE_FORMAT)
            .cloned()
            .collect()
    });
    if reference && printer.is_some() {
        formats.push(REFERENCE_FORMAT.to_owned());
    }
    formats
}

/// The format a slice of the plate is written in: the plate's own when its printer writes it,
/// else the first its printer writes, as the Slice panel chooses it; `None` without one.
pub(crate) fn plate_format(workspace: &Workspace) -> Option<String> {
    format_of(workspace.plate())
}

/// [`plate_format`] for `plate`, shown or not (TD-135, phase 4: a plate not shown has not had
/// its format settled by the Slice panel): its own when its printer writes it, else the first
/// its printer writes; `None` without one.
pub(crate) fn format_of(plate: &karasu_project::project::PlateContent) -> Option<String> {
    let formats = printer_formats(plate.printer.as_ref(), false);
    let current = &plate.settings.format;
    if formats.contains(current) {
        Some(current.clone())
    } else {
        formats.into_iter().next()
    }
}

/// A format as the panel names it: its readable name and extension, or the reference format's
/// purpose.
pub(crate) fn format_label(id: &str) -> String {
    if id == REFERENCE_FORMAT {
        return tr!("format-reference");
    }
    format_name(id).map_or_else(
        || id.to_owned(),
        |f| format!("{} (.{})", f.name, f.extension),
    )
}

/// A switch with its label and a weak hint under it, both wrapping left of the switch.
pub(crate) fn switch_row(ui: &mut egui::Ui, on: &mut bool, label: String, hint: String) -> bool {
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
            let switch = crate::widgets::switch(ui, on);
            crate::widgets::name(&switch, &label);
            let changed = switch.changed();
            ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                crate::widgets::switch_label(ui, label);
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(hint)
                            .small()
                            .color(ui.visuals().weak_text_color()),
                    )
                    .wrap(),
                );
            });
            changed
        })
        .inner
    })
    .inner
}

impl SlicingUi {
    /// Whether the last slice is of the plate as it is now.
    pub(crate) fn current(&self, workspace: &Workspace) -> bool {
        self.sliced
            .as_ref()
            .is_some_and(|s| workspace.freshness(s.revision) == Freshness::Current)
    }

    /// Whether the plate can be sliced now: models, a printer, a resin and a format, and no
    /// slice running.
    fn ready(&self, workspace: &Workspace) -> bool {
        !workspace.plate().objects.is_empty()
            && workspace.plate().setting().is_some()
            && !formats(workspace, false).is_empty()
            && self.running.is_none()
    }

    /// The Slice panel's body: the output, the slicing options with Advanced, and the result.
    #[allow(clippy::too_many_lines)] // The panel's groups in order, as the owner's mockup.
    pub fn panel(&mut self, ui: &mut egui::Ui, workspace: &mut Workspace, planner: &Planner) {
        let formats = formats(workspace, self.advanced);
        // The plate keeps a format its printer lists.
        let current = workspace.plate().settings.format.clone();
        if !formats.contains(&current) {
            if let Some(first) = formats.first().cloned() {
                workspace.edit(|plate| plate.settings.format = first);
            }
        }
        let profiles = workspace.plate().setting().is_some();
        ui.add_enabled_ui(profiles, |ui| {
            crate::widgets::group(ui, tr!("slice-output"), |ui| {
                let plate = workspace.plate();
                let name = |name: Option<&String>| name.cloned().unwrap_or_else(|| "–".to_owned());
                fact_row(
                    ui,
                    &tr!("slice-printer"),
                    &name(plate.printer.as_ref().map(|p| &p.name)),
                );
                fact_row(
                    ui,
                    &tr!("slice-resin"),
                    &name(plate.resin.as_ref().map(|r| &r.name)),
                );
                if let Some(setting) = plate.setting() {
                    setting_facts(ui, setting);
                    if ui.link(tr!("action-edit-print-setting")).clicked() {
                        self.edit_setting = true;
                    }
                }
                // The calibrated height, then the plate's layer ranges (TD-102).
                fact_row(
                    ui,
                    &tr!("slice-layer-height"),
                    &crate::layer_plan_ui::summary(workspace),
                )
                .on_hover_text(tr!("slice-layer-height-hint"));
                let layers = match (planner.error(), planner.count()) {
                    (Some(_), _) | (None, None) => "–".to_owned(),
                    (None, Some((count, false))) => count.to_string(),
                    (None, Some((count, true))) => tr!("slice-layers-about", count = count),
                };
                let row = fact_row(ui, &tr!("slice-layers"), &layers);
                match planner.error() {
                    Some(error) => {
                        row.on_hover_text(tr!("slice-plan-error", reason = error));
                    }
                    None if planner.count().is_some_and(|(_, about)| about) => {
                        row.on_hover_text(tr!("slice-layers-about-hint"));
                    }
                    None => {}
                }
                let current = workspace.plate().settings.format.clone();
                let mut chosen = None;
                // A format's name can be long: the menu takes the panel's width under its label.
                ui.label(tr!("slice-format"));
                let combo = egui::ComboBox::from_id_salt("format")
                    .selected_text(format_label(&current))
                    .width(ui.available_width())
                    .show_ui(ui, |ui| {
                        if formats.is_empty() {
                            ui.label(tr!("format-none-supported"));
                        }
                        for format in &formats {
                            let label = format_label(format);
                            let item = ui.selectable_label(*format == current, label);
                            let family = format_name(format).map(|f| f.family);
                            if item.on_hover_text(family.unwrap_or_default()).clicked() {
                                chosen = Some(format.clone());
                            }
                        }
                    })
                    .response
                    .on_hover_text(&current);
                crate::widgets::name(&combo, &tr!("slice-format"));
                if let Some(format) = chosen {
                    workspace.edit(|plate| plate.settings.format = format);
                }
                // The plate's masks, over its print setting's (TD-134).
                super::masks::rows(ui, workspace);
                switch_row(
                    ui,
                    &mut self.verify,
                    tr!("slice-verify"),
                    tr!("slice-verify-hint"),
                );
            });
        });
        if self.advanced {
            ui.add_space(4.0);
            egui::CollapsingHeader::new(tr!("slice-options"))
                .id_salt("slice-options")
                .default_open(true)
                .show(ui, |ui| repair_ui::plate_settings(ui, workspace));
        }
        ui.add_space(4.0);
        let stale = self.sliced.is_some() && !self.current(workspace);
        egui::CollapsingHeader::new(tr!("prepare-result"))
            .id_salt("slice-result")
            .default_open(true)
            .show(ui, |ui| {
                // A result older than the plate shows faded: it is not what would be printed.
                if stale {
                    ui.set_opacity(0.55);
                }
                self.result(ui);
            });
    }

    /// The last slice's facts, and the printed walls' findings when it verified them.
    fn result(&mut self, ui: &mut egui::Ui) {
        let dash = || "–".to_owned();
        let sliced = self.sliced.as_ref();
        let export = sliced.map(|s| &s.outcome.export);
        fact_row(
            ui,
            &tr!("slice-layers"),
            &export.map_or_else(dash, |e| e.layers.to_string()),
        );
        fact_row(
            ui,
            &tr!("slice-height"),
            &sliced.and_then(|s| s.masks.last()).map_or_else(dash, |m| {
                tr!("prepare-length-mm", length = decimal(m.z_mm, 2))
            }),
        );
        fact_row(
            ui,
            &tr!("slice-time"),
            &export.map_or_else(dash, |e| duration(e.estimated_time_s)),
        );
        fact_row(
            ui,
            &tr!("slice-resin"),
            &sliced.map_or_else(dash, |s| {
                tr!(
                    "slice-volume-ml",
                    volume = decimal(s.outcome.resin.volume_ml, 1)
                )
            }),
        );
        fact_row(
            ui,
            &tr!("slice-format"),
            &sliced.map_or_else(dash, |s| {
                if s.extension.is_empty() {
                    karasu_project::request::REFERENCE_FORMAT.to_owned()
                } else {
                    s.extension.clone()
                }
            }),
        );
        let Some(verification) = sliced.and_then(|s| s.outcome.verification.as_ref()) else {
            return;
        };
        ui.add_space(4.0);
        ui.separator();
        ui.strong(tr!("walls-title"));
        // A finding is shown on its layer, in the Layers tool.
        if let Some(focus) = walls(ui, verification) {
            self.layer = focus.layer;
            self.focus = Some(focus);
            self.show_layers = true;
        }
    }

    /// The Slice panel's footer: Slice and Export…, or the running slice's progress with
    /// Cancel, then the slice's state.
    pub fn footer(&mut self, ui: &mut egui::Ui, workspace: &Workspace, icons: Option<&Icons>) {
        if let Some(running) = &self.running {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(50));
            let (fraction, text) = running.layers.map_or_else(
                || {
                    let seconds = running.started.elapsed().as_secs();
                    (0.0, tr!("progress-seconds", seconds = seconds))
                },
                |(done, total)| {
                    #[allow(clippy::cast_precision_loss)] // A progress bar.
                    let f = done as f32 / total.max(1) as f32;
                    (f, tr!("progress-layers", done = done, total = total))
                },
            );
            ui.add(
                egui::Label::new(
                    egui::RichText::new(&running.phase)
                        .small()
                        .color(ui.visuals().weak_text_color()),
                )
                .truncate(),
            );
            ui.add(egui::ProgressBar::new(fraction).text(text));
            if ui
                .add_sized(
                    [ui.available_width(), ui.spacing().interact_size.y],
                    egui::Button::new(tr!("action-cancel")),
                )
                .clicked()
            {
                running.job.cancel();
            }
        } else {
            // Hiding is a way of looking: hidden models print all the same, and the footer says
            // so before slicing (owner, 2026-10-26).
            let hidden = workspace.hidden_count();
            if hidden > 0 {
                state_line(
                    ui,
                    icons,
                    Some(Icon::Warning),
                    &tr!("slice-hidden-printed", count = hidden),
                    theme::WARNING.color(),
                )
                .on_hover_text(tr!("action-visibility-hint"));
                ui.add_space(4.0);
            }
            let (slice, export) = action_pair(
                ui,
                [tr!("action-slice"), tr!("action-export")],
                (self.ready(workspace), self.current(workspace)),
            );
            let slice = slice.on_disabled_hover_text(tr!("action-slice-disabled"));
            let export = export.on_disabled_hover_text(tr!("action-export-disabled"));
            if slice.clicked() {
                self.start(workspace);
            } else if export.clicked() {
                self.export(workspace);
            }
        }
        ui.add_space(4.0);
        let (text, colour, icon, detail) = self.state(workspace);
        let response = state_line(ui, icons, icon, &text, colour);
        if let Some(detail) = detail {
            response.on_hover_text(detail);
        }
    }

    /// What the footer says of the plate's slice, most important first: running, missing
    /// profiles, models or format, a failure, then the last slice's freshness.
    fn state(
        &self,
        workspace: &Workspace,
    ) -> (String, egui::Color32, Option<Icon>, Option<String>) {
        let plate = workspace.plate();
        if self.running.is_some() {
            return (
                tr!("slice-state-running"),
                theme::ACCENT.color(),
                Some(Icon::Info),
                None,
            );
        }
        if plate.setting().is_none() {
            return (
                tr!("slice-state-profiles"),
                theme::WARNING.color(),
                Some(Icon::Warning),
                None,
            );
        }
        if plate.objects.is_empty() {
            return (
                tr!("slice-state-no-models"),
                theme::BORDER.color(),
                None,
                None,
            );
        }
        if formats(workspace, false).is_empty() {
            return (
                tr!("format-none-supported"),
                theme::ERROR.color(),
                Some(Icon::Error),
                None,
            );
        }
        if let Some(failure) = &self.failed {
            return (
                failure.clone(),
                theme::ERROR.color(),
                Some(Icon::Error),
                Some(tr!("slice-state-failed-hint")),
            );
        }
        match &self.sliced {
            None => (tr!("slice-state-none"), theme::BORDER.color(), None, None),
            Some(_) if !self.current(workspace) => (
                tr!("slice-state-stale"),
                theme::WARNING.color(),
                Some(Icon::Warning),
                None,
            ),
            Some(sliced) => (
                tr!("slice-state-done", facts = facts(&sliced.outcome)),
                theme::SUCCESS.color(),
                Some(Icon::Success),
                None,
            ),
        }
    }
}

/// The plate's print setting: its name, exposure, compensation and a starting point's warning
/// (TD-106). Its anti-aliasing is a row of its own, which the plate may override (TD-134).
fn setting_facts(ui: &mut egui::Ui, setting: &karasu_core::profiles::PrintSettingProfile) {
    let image = &setting.image;
    fact_row(ui, &tr!("slice-print-setting"), &setting.name)
        .on_hover_text(setting.source.clone().unwrap_or_default());
    let s = &setting.settings;
    fact_row(
        ui,
        &tr!("field-exposure"),
        &tr!(
            "slice-exposure-value",
            normal = s.normal.exposure_s.to_string(),
            bottom = s.bottom.exposure_s.to_string(),
            count = s.bottom_layer_count
        ),
    );
    fact_row(
        ui,
        &tr!("field-xy-compensation"),
        &tr!("slice-pixels", value = image.xy_compensation_px.to_string()),
    );
    if setting.status == karasu_core::profiles::SettingStatus::StartingPoint {
        ui.label(
            egui::RichText::new(tr!("setting-starting-point"))
                .small()
                .color(theme::WARNING.color()),
        )
        .on_hover_text(tr!("slice-starting-point-hint"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::slicing_ui::tests::{finish, hollowed_cube, slicing_into};

    /// The width a panel's body has: the panel, less its frame.
    const WIDTH: f32 = 316.0;

    /// Draw the panel and its footer, `WIDTH` wide, over two frames: the texts drawn, and the
    /// width used.
    fn draw(slicing: &mut SlicingUi, workspace: &mut Workspace) -> (Vec<String>, f32) {
        let ctx = egui::Context::default();
        let mut texts = Vec::new();
        let mut used = 0.0;
        for _ in 0..2 {
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                ui.allocate_ui(egui::vec2(WIDTH, 4000.0), |ui| {
                    ui.set_width(WIDTH);
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                    slicing.panel(ui, workspace, &Planner::default());
                    slicing.footer(ui, workspace, None);
                    used = ui.min_rect().width();
                });
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
        (texts, used)
    }

    #[test]
    fn without_profiles_the_panel_keeps_its_structure_and_says_what_is_missing() {
        let folder =
            std::env::temp_dir().join(format!("karasu-slice-panel-{}", std::process::id()));
        let mut slicing = slicing_into(&folder);
        let mut workspace = Workspace::new();
        let before = workspace.plate().clone();
        let (texts, _) = draw(&mut slicing, &mut workspace);
        for expected in [
            // A group's title is written in capitals on its frame.
            tr!("slice-output").to_uppercase(),
            tr!("slice-format"),
            tr!("slice-verify"),
            tr!("prepare-result"),
            tr!("slice-layers"),
            tr!("action-slice"),
            tr!("action-export"),
            tr!("slice-state-profiles"),
        ] {
            assert!(texts.contains(&expected), "{expected} in {texts:?}");
        }
        assert_eq!(*workspace.plate(), before);
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn the_footer_follows_a_slice_from_none_to_done_to_out_of_date() {
        let folder =
            std::env::temp_dir().join(format!("karasu-slice-state-{}", std::process::id()));
        let mut workspace = hollowed_cube(&folder);
        let mut slicing = slicing_into(&folder);
        slicing.advanced = true;
        assert_eq!(slicing.state(&workspace).0, tr!("slice-state-none"));
        // A failure is said until a slice ends.
        slicing.failed = Some("failed".to_owned());
        assert_eq!(slicing.state(&workspace).0, "failed");
        slicing.failed = None;
        slicing.start(&workspace);
        assert_eq!(slicing.state(&workspace).0, tr!("slice-state-running"));
        finish(&mut slicing);
        let (text, _, icon, _) = slicing.state(&workspace);
        let done = facts(&slicing.sliced.as_ref().unwrap().outcome);
        assert_eq!(text, tr!("slice-state-done", facts = done));
        assert_eq!(icon, Some(Icon::Success));
        // Every setting shown, also in a longer language, within the panel's width.
        for locale in ["en-US", "pseudo"] {
            crate::i18n::install(crate::i18n::Catalog::open(locale, None).unwrap());
            let (_, used) = draw(&mut slicing, &mut workspace);
            assert!(used <= WIDTH + 0.5, "{locale}: {used} wide");
        }
        crate::i18n::install(crate::i18n::Catalog::reference());
        workspace.edit(|plate| plate.objects[0].translate_mm[0] += 1.0);
        assert_eq!(slicing.state(&workspace).0, tr!("slice-state-stale"));
        assert!(!slicing.current(&workspace));
        let _ = std::fs::remove_dir_all(&folder);
    }
}

#[cfg(test)]
mod plan_tests {
    use crate::layer_plan_ui::{self, Kind, Planner, Range};
    use crate::slicing_ui::tests::{finish, hollowed_cube, slicing_into};

    #[test]
    fn a_fixed_range_set_from_the_interface_slices_the_layers_it_planned() {
        // TD-102: the planned count is exact for fixed ranges, as the slice shows.
        let folder = std::env::temp_dir().join(format!("karasu-plan-slice-{}", std::process::id()));
        let mut workspace = hollowed_cube(&folder);
        let calibration = layer_plan_ui::calibration(&workspace).unwrap();
        let fixed = Range {
            from_mm: (layer_plan_ui::base_mm(&calibration) + 1.0).max(2.0),
            kind: Kind::Fixed {
                height_mm: 0.025,
                exposure_s: Some(1.6),
            },
        };
        layer_plan_ui::set(&mut workspace, &[fixed], &[]);
        let mut planner = Planner::default();
        planner.update(&mut workspace);
        let (count, about) = planner.count().unwrap();
        assert!(!about);
        let mut slicing = slicing_into(&folder);
        slicing.start(&workspace);
        finish(&mut slicing);
        let sliced = slicing
            .sliced
            .as_ref()
            .expect("the plate slices with its plan");
        assert_eq!(sliced.outcome.export.layers, count);
        // The job's layers, as resolved: within the range, its height and own exposure.
        let layers = &sliced.outcome.layers;
        assert_eq!(layers.len(), count);
        assert!(layers.windows(2).all(|w| w[1].top_mm > w[0].top_mm));
        let inside = layers
            .iter()
            .find(|l| l.top_mm > fixed.from_mm + 0.1)
            .unwrap();
        assert!((inside.thickness_mm - 0.025).abs() < 1e-9, "{inside:?}");
        assert!((inside.exposure_s - 1.6).abs() < 1e-9, "{inside:?}");
        assert_eq!(inside.phase, karasu_core::print_job::LayerPhase::Override);
        let _ = std::fs::remove_dir_all(&folder);
    }
}
