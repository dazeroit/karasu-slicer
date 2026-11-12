//! Profile dialogs: a print setting's editor, a printer's and a resin's forms, a resin's set-up
//! from another setting, and a newer catalog revision's review (TD-106; "Windows" in
//! `docs/UI-DESIGN.md`). Closing an editor with unsaved changes asks first.
use super::{
    forms::{form_grid, form_row, PrinterForm, ResinForm, WIDE_FIELD},
    label_of, status, Dialog, Outcome,
};
use crate::{
    dialog::{self, Part, Remembered, Size},
    i18n::tr,
    messages::{Level, Message},
    theme,
    widgets::{switch, text_field, value_row, Unit, UnitField},
};
use eframe::egui;
use karasu_core::profiles::{
    store::ProfileKind, ImageSettings, LayerSettings, MotionStage, PrintSettingProfile,
    PrintSettings, PrinterProfile, ResinMaterial, SettingStatus, LIBRARY_SCHEMA_VERSION,
    MAX_COMPENSATION_PX,
};
use karasu_project::library::Library;

/// Ask whether to discard unsaved changes while `asking`; closes the dialog on yes.
fn discard_prompt(ctx: &egui::Context, asking: &mut bool, out: &mut Outcome) {
    if !*asking {
        return;
    }
    match dialog::confirm(
        ctx,
        "discard-changes",
        &tr!("discard-title"),
        &tr!("discard-text"),
        &tr!("action-discard"),
        true,
    ) {
        Some(true) => out.close = true,
        Some(false) => *asking = false,
        None => {}
    }
}

/// An edited printer's dialog.
pub(super) struct PrinterEditor {
    form: PrinterForm,
    discard: bool,
}

impl PrinterEditor {
    pub(super) fn new(printer: &PrinterProfile) -> Self {
        Self {
            form: PrinterForm::edit(printer),
            discard: false,
        }
    }

    pub(super) fn show(&mut self, ctx: &egui::Context, library: &mut Library, out: &mut Outcome) {
        let (mut save, mut cancel) = (false, false);
        let size = Size::Resizable {
            default: egui::vec2(800.0, 560.0),
            min: egui::vec2(760.0, 480.0),
        };
        let response = dialog::show(
            ctx,
            "printer-editor",
            &tr!("printer-edit-title"),
            size,
            |ui, part| match part {
                Part::Body => {
                    crate::focus::scrolled(
                        egui::ScrollArea::vertical()
                            .id_salt("printer-editor-body")
                            .auto_shrink([false, false]),
                        ui,
                        |ui| self.form.ui(ui, library),
                    );
                }
                Part::Footer { enter } => {
                    let can = self.form.changed() && self.form.complete();
                    save = dialog::main_action(ui, &tr!("action-save"), can, enter);
                    cancel = dialog::cancel_action(ui);
                }
            },
        );
        if save {
            match library.save_printer(self.form.printer()) {
                Ok(()) => {
                    out.messages.push(Message::new(
                        Level::Success,
                        tr!("library-saved", name = self.form.name.trim()),
                    ));
                    out.changed = true;
                    out.close = true;
                }
                Err(error) => out.fail(&error),
            }
        } else if response.cancel || cancel {
            if self.form.changed() {
                self.discard = true;
            } else {
                out.close = true;
            }
        }
        discard_prompt(ctx, &mut self.discard, out);
    }
}

/// An edited resin's dialog.
pub(super) struct ResinEditor {
    form: ResinForm,
    discard: bool,
}

impl ResinEditor {
    pub(super) fn new(resin: &ResinMaterial) -> Self {
        Self {
            form: ResinForm::edit(resin),
            discard: false,
        }
    }

    pub(super) fn show(&mut self, ctx: &egui::Context, library: &mut Library, out: &mut Outcome) {
        let (mut save, mut cancel) = (false, false);
        let response = dialog::show(
            ctx,
            "resin-editor",
            &tr!("resin-edit-title"),
            Size::Fixed(440.0),
            |ui, part| match part {
                Part::Body => self.form.ui(ui, library),
                Part::Footer { enter } => {
                    let named = !self.form.material().name.is_empty();
                    let can = self.form.changed() && named;
                    save = dialog::main_action(ui, &tr!("action-save"), can, enter);
                    cancel = dialog::cancel_action(ui);
                }
            },
        );
        if save {
            let resin = self.form.material();
            match library.save_material(resin.clone()) {
                Ok(()) => {
                    out.messages.push(Message::new(
                        Level::Success,
                        tr!("library-saved", name = &resin.name),
                    ));
                    out.changed = true;
                    out.close = true;
                }
                Err(error) => out.fail(&error),
            }
        } else if response.cancel || cancel {
            if self.form.changed() {
                self.discard = true;
            } else {
                out.close = true;
            }
        }
        discard_prompt(ctx, &mut self.discard, out);
    }
}

/// A new print setting for a printer and a resin (TD-106 point 4): a copy of one of the
/// printer's settings, marked a starting point, or every value entered in the editor. From the
/// resin menu it sets a resin up for the plate's printer; from the library or the print setting
/// menu the printer and the resin are chosen here.
pub(super) struct SetUp {
    printer: String,
    resin: String,
    /// The printer and the resin are chosen in the dialog.
    choose: bool,
    /// The setting copied, or `None` to enter every value.
    from: Option<String>,
    /// The printer `from` was last given its default for.
    defaulted: Option<String>,
    name: String,
}

impl SetUp {
    /// Set `resin` up for `printer`.
    pub(super) fn new(printer: String, resin: String) -> Self {
        Self {
            printer,
            resin,
            choose: false,
            from: None,
            defaulted: None,
            name: String::new(),
        }
    }

    /// A new print setting, `printer` and `resin` chosen first.
    pub(super) fn choosing(printer: String, resin: String) -> Self {
        Self {
            choose: true,
            ..Self::new(printer, resin)
        }
    }

    #[allow(clippy::too_many_lines)] // The choices, then the outcome.
    pub(super) fn show(&mut self, ctx: &egui::Context, library: &mut Library, out: &mut Outcome) {
        let sources: Vec<PrintSettingProfile> = library
            .settings()
            .iter()
            .filter(|s| s.printer_id == self.printer)
            .cloned()
            .collect();
        // The printer's first setting is offered, again whenever another printer is chosen.
        if self.defaulted.as_ref() != Some(&self.printer) {
            self.defaulted = Some(self.printer.clone());
            self.from = sources.first().map(|s| s.id.clone());
            self.name = sources.first().map_or_else(String::new, |s| s.name.clone());
        }
        let printer = library
            .printer(&self.printer)
            .map_or_else(String::new, |p| p.name.clone());
        let resin = library
            .material(&self.resin)
            .map_or_else(String::new, |r| r.name.clone());
        // Each source described, read before the dialog may change the library.
        let described: Vec<String> = sources
            .iter()
            .map(|s| {
                let resin = library
                    .material(&s.resin_id)
                    .map_or_else(|| s.resin_id.clone(), |m| m.name.clone());
                format!("{resin} · {} ({})", label_of(s), status(s.status).0)
            })
            .collect();
        let printers: Vec<(String, String)> = library
            .printers()
            .iter()
            .map(|p| (p.id.clone(), p.name.clone()))
            .collect();
        let resins: Vec<(String, String)> = library
            .materials()
            .iter()
            .map(|m| (m.id.clone(), m.name.clone()))
            .collect();
        let title = if self.choose {
            tr!("setting-new-title")
        } else {
            tr!("setup-title", resin = &resin, printer = &printer)
        };
        let (mut create, mut cancel) = (false, false);
        let response = dialog::show(
            ctx,
            "setup",
            &title,
            Size::Fixed(480.0),
            |ui, part| match part {
                Part::Body => {
                    let explain = if self.choose {
                        tr!("setting-new-explain")
                    } else if sources.is_empty() {
                        tr!("setup-none", printer = &printer)
                    } else {
                        tr!("setup-explain")
                    };
                    ui.add(egui::Label::new(explain).wrap());
                    ui.add_space(6.0);
                    form_grid(ui, "setup-form", |ui| {
                        if self.choose {
                            form_row(ui, tr!("profiles-printer"), |ui| {
                                choice(ui, "setup-printer", &printers, &mut self.printer)
                            });
                            form_row(ui, tr!("profiles-resin"), |ui| {
                                choice(ui, "setup-resin", &resins, &mut self.resin)
                            });
                        }
                        form_row(ui, tr!("setup-from"), |ui| {
                            let chosen = sources
                                .iter()
                                .position(|s| Some(&s.id) == self.from.as_ref());
                            egui::ComboBox::from_id_salt("setup-from")
                                .selected_text(chosen.map_or_else(
                                    || tr!("setup-from-nothing"),
                                    |k| described[k].clone(),
                                ))
                                .width(WIDE_FIELD * 1.4)
                                .show_ui(ui, |ui| {
                                    for (k, s) in sources.iter().enumerate() {
                                        if ui
                                            .selectable_label(chosen == Some(k), &described[k])
                                            .clicked()
                                        {
                                            self.from = Some(s.id.clone());
                                            self.name.clone_from(&s.name);
                                        }
                                    }
                                    if ui
                                        .selectable_label(
                                            self.from.is_none(),
                                            tr!("setup-from-nothing"),
                                        )
                                        .clicked()
                                    {
                                        self.from = None;
                                    }
                                })
                        });
                        if self.from.is_some() {
                            form_row(ui, tr!("field-name"), |ui| {
                                ui.add(text_field(&mut self.name, WIDE_FIELD * 1.4))
                            });
                        }
                    });
                }
                Part::Footer { enter } => {
                    let label = if self.from.is_none() {
                        tr!("action-new-print-setting")
                    } else {
                        tr!("action-create")
                    };
                    let can = !printer.is_empty()
                        && !resin.is_empty()
                        && (self.from.is_none() || !self.name.trim().is_empty());
                    create = dialog::main_action(ui, &label, can, enter);
                    cancel = dialog::cancel_action(ui);
                }
            },
        );
        if response.cancel || cancel {
            out.close = true;
            return;
        }
        if !create {
            return;
        }
        let Some(from) = self.from.clone() else {
            out.replace = Some(Dialog::Setting(SettingEditor::new(
                &self.printer,
                &self.resin,
            )));
            return;
        };
        match library.start_from(&self.printer, &self.resin, &from, &self.name) {
            Ok(id) => {
                out.messages.push(Message::new(
                    Level::Warning,
                    tr!("setup-created", resin = &resin),
                ));
                out.changed = true;
                if self.choose {
                    // Opened from the library: the copy opens in the editor, to calibrate.
                    out.replace = library
                        .setting(&id)
                        .map(|s| Dialog::Setting(SettingEditor::edit(s.clone())));
                    out.close = out.replace.is_none();
                } else {
                    out.use_resin = Some((self.resin.clone(), Some(id)));
                    out.close = true;
                }
            }
            Err(error) => out.fail(&error),
        }
    }
}

/// A menu choosing one of `options`, `(id, name)`, into `chosen`.
fn choice(
    ui: &mut egui::Ui,
    id: &str,
    options: &[(String, String)],
    chosen: &mut String,
) -> egui::InnerResponse<Option<()>> {
    let current = options
        .iter()
        .find(|(o, _)| o == chosen)
        .map_or_else(String::new, |(_, name)| name.clone());
    egui::ComboBox::from_id_salt(id)
        .selected_text(current)
        .width(WIDE_FIELD * 1.4)
        .show_ui(ui, |ui| {
            for (option, name) in options {
                ui.selectable_value(chosen, option.clone(), name);
            }
        })
}

/// A print setting's band of motion: from the plate up, its height, lift speed, and the
/// retract's distance and speed over it.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Band {
    distance: f64,
    up: f64,
    retract: f64,
    down: f64,
}

/// A layer's motion as bands: lifting covers them in order, retracting in reverse (TD-105).
fn bands(layer: &LayerSettings) -> Vec<Band> {
    let retract: Vec<&MotionStage> = layer.retract.iter().rev().collect();
    layer
        .lift
        .iter()
        .enumerate()
        .map(|(k, stage)| Band {
            distance: stage.distance_mm,
            up: stage.speed_mm_s,
            retract: retract.get(k).map_or(stage.distance_mm, |r| r.distance_mm),
            down: retract.get(k).map_or(stage.speed_mm_s, |r| r.speed_mm_s),
        })
        .collect()
}

fn set_bands(layer: &mut LayerSettings, bands: &[Band]) {
    layer.lift = bands
        .iter()
        .map(|b| MotionStage {
            distance_mm: b.distance,
            speed_mm_s: b.up,
        })
        .collect();
    layer.retract = bands
        .iter()
        .rev()
        .map(|b| MotionStage {
            distance_mm: b.retract,
            speed_mm_s: b.down,
        })
        .collect();
}

/// A neutral setting to enter values in, when nothing can be started from; the user enters
/// every value (the editor says so).
fn blank(printer: &str, resin: &str) -> PrintSettingProfile {
    let layer = |exposure_s| LayerSettings {
        exposure_s,
        wait_before_exposure_s: 1.0,
        wait_after_exposure_s: 0.0,
        wait_after_lift_s: 0.0,
        pwm: 255,
        lift: vec![MotionStage {
            distance_mm: 6.0,
            speed_mm_s: 3.0,
        }],
        retract: vec![MotionStage {
            distance_mm: 6.0,
            speed_mm_s: 3.0,
        }],
    };
    PrintSettingProfile {
        schema_version: LIBRARY_SCHEMA_VERSION,
        id: "new-setting".into(),
        name: "Normal 0.05 mm".into(),
        revision: 1,
        printer_id: printer.to_owned(),
        resin_id: resin.to_owned(),
        film: None,
        status: SettingStatus::User,
        source: None,
        based_on: None,
        settings: PrintSettings {
            heating_temperature_c: Some(karasu_core::profiles::DEFAULT_HEATING_C),
            layer_height_mm: 0.05,
            bottom_layer_count: 4,
            transition_layer_count: 0,
            bottom: layer(30.0),
            normal: layer(2.5),
        },
        image: ImageSettings::default(),
    }
}

const FILMS: [&str; 3] = ["NFEP", "ACF", "FEP"];
/// The width of a table's row labels.
const ROW_LABEL: f32 = 170.0;
/// The width of a distance or rest field in a table.
const SHORT_FIELD: f32 = 100.0;
/// The width of a speed field in a table.
const SPEED_FIELD: f32 = 120.0;

/// One row of a table: cells of `widths`, each filled by `add` with its index.
fn table_row(ui: &mut egui::Ui, widths: &[f32], mut add: impl FnMut(usize, &mut egui::Ui)) {
    ui.horizontal(|ui| {
        for (k, &width) in widths.iter().enumerate() {
            ui.allocate_ui_with_layout(
                egui::vec2(width, ui.spacing().interact_size.y),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.set_width(width);
                    add(k, ui);
                },
            );
        }
    });
}

/// A print setting's editor (TD-106): exposure, rests and motion, and the image.
pub(super) struct SettingEditor {
    draft: PrintSettingProfile,
    /// The saved setting, or `None` for a new one.
    original: Option<PrintSettingProfile>,
    tab: usize,
    /// The two stages of the normal and the bottom layers while two stages are off, to bring
    /// back when they are turned on again.
    split: [Option<Vec<Band>>; 2],
    discard: bool,
}

impl SettingEditor {
    pub(super) fn edit(setting: PrintSettingProfile) -> Self {
        Self {
            draft: setting.clone(),
            original: Some(setting),
            tab: 0,
            split: [None, None],
            discard: false,
        }
    }

    pub(super) fn new(printer: &str, resin: &str) -> Self {
        Self {
            draft: blank(printer, resin),
            original: None,
            tab: 0,
            split: [None, None],
            discard: false,
        }
    }

    fn dirty(&self) -> bool {
        self.original.as_ref() != Some(&self.draft)
    }

    pub(super) fn show(&mut self, ctx: &egui::Context, library: &mut Library, out: &mut Outcome) {
        let printer = library
            .printer(&self.draft.printer_id)
            .map_or_else(|| self.draft.printer_id.clone(), |p| p.name.clone());
        let resin = library
            .material(&self.draft.resin_id)
            .map_or_else(|| self.draft.resin_id.clone(), |r| r.name.clone());
        let (mut save, mut revert, mut cancel) = (false, false, false);
        let size = Size::Resizable {
            default: egui::vec2(720.0, 560.0),
            min: egui::vec2(640.0, 440.0),
        };
        let response = dialog::show(
            ctx,
            "setting-editor",
            &tr!("setting-title"),
            size,
            |ui, part| match part {
                Part::Body => self.body(ui, &printer, &resin),
                Part::Footer { enter } => {
                    let can = self.dirty() && !self.draft.name.trim().is_empty();
                    save = dialog::main_action(ui, &tr!("action-save"), can, enter);
                    revert = ui
                        .add_enabled(
                            self.dirty() && self.original.is_some(),
                            egui::Button::new(tr!("action-revert")),
                        )
                        .clicked();
                    cancel = dialog::cancel_action(ui);
                    let (text, colour) = if self.dirty() {
                        (tr!("setting-unsaved"), theme::WARNING.color())
                    } else {
                        (tr!("setting-no-changes"), ui.visuals().weak_text_color())
                    };
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.add(
                            egui::Label::new(egui::RichText::new(text).color(colour)).truncate(),
                        );
                    });
                }
            },
        );
        if save {
            self.save(library, out);
        } else if revert {
            if let Some(original) = &self.original {
                self.draft = original.clone();
            }
        } else if response.cancel || cancel {
            if self.dirty() {
                self.discard = true;
            } else {
                out.close = true;
            }
        }
        discard_prompt(ctx, &mut self.discard, out);
    }

    fn body(&mut self, ui: &mut egui::Ui, printer: &str, resin: &str) {
        ui.horizontal(|ui| {
            ui.label(tr!("setting-for", printer = printer, resin = resin));
            // The saved status: an edit becomes the user's only when saved.
            let saved = self
                .original
                .as_ref()
                .map_or(SettingStatus::User, |o| o.status);
            let (text, colour) = status(saved);
            ui.label(egui::RichText::new(text).color(colour).strong());
        });
        if let Some(source) = &self.draft.source {
            ui.label(
                egui::RichText::new(source)
                    .small()
                    .color(ui.visuals().weak_text_color()),
            );
        }
        if self.original.is_none() {
            ui.add(
                egui::Label::new(
                    egui::RichText::new(tr!("setting-enter-every-value"))
                        .color(theme::WARNING.color()),
                )
                .wrap(),
            );
        }
        form_grid(ui, "setting-header", |ui| {
            form_row(ui, tr!("field-name"), |ui| {
                ui.add(text_field(&mut self.draft.name, WIDE_FIELD * 1.4))
            });
            form_row(ui, tr!("field-film"), |ui| {
                let label = self
                    .draft
                    .film
                    .clone()
                    .unwrap_or_else(|| tr!("film-not-stated"));
                egui::ComboBox::from_id_salt("setting-film")
                    .selected_text(label)
                    .width(WIDE_FIELD)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.draft.film, None, tr!("film-not-stated"));
                        for film in FILMS {
                            ui.selectable_value(&mut self.draft.film, Some(film.to_owned()), film);
                        }
                    })
            });
        });
        ui.separator();
        ui.horizontal(|ui| {
            for (k, label) in [
                tr!("setting-tab-exposure"),
                tr!("setting-tab-motion"),
                tr!("setting-tab-image"),
            ]
            .into_iter()
            .enumerate()
            {
                if ui.selectable_label(self.tab == k, label).clicked() {
                    self.tab = k;
                }
            }
        });
        ui.separator();
        crate::focus::scrolled(
            egui::ScrollArea::vertical()
                .id_salt("setting-editor-tab")
                .auto_shrink([false, false]),
            ui,
            |ui| match self.tab {
                0 => self.exposure(ui),
                1 => self.motion(ui),
                _ => self.image(ui),
            },
        );
    }

    fn save(&mut self, library: &mut Library, out: &mut Outcome) {
        let result = match &self.original {
            Some(_) => library
                .save_setting(self.draft.clone())
                .map(|()| self.draft.id.clone()),
            None => library.new_setting(&self.draft),
        };
        match result {
            Ok(id) => {
                out.messages.push(Message::new(
                    Level::Success,
                    tr!("library-saved", name = self.draft.name.trim()),
                ));
                out.changed = true;
                if self.original.is_none() {
                    out.use_resin = Some((self.draft.resin_id.clone(), Some(id.clone())));
                }
                if let Some(saved) = library.setting(&id) {
                    let tab = self.tab;
                    *self = Self::edit(saved.clone());
                    self.tab = tab;
                }
            }
            Err(error) => out.fail(&error),
        }
    }

    fn exposure(&mut self, ui: &mut egui::Ui) {
        let s = &mut self.draft.settings;
        value_row(ui, tr!("field-layer-height"), |ui| {
            ui.add(
                UnitField::new(&mut s.layer_height_mm, Unit::Millimetre)
                    .range(0.01..=0.3)
                    .step(0.005)
                    .decimals(3),
            )
        });
        count_row(
            ui,
            &tr!("field-bottom-layers"),
            &mut s.bottom_layer_count,
            0..=200,
        );
        value_row(ui, tr!("field-bottom-exposure"), |ui| {
            ui.add(
                UnitField::new(&mut s.bottom.exposure_s, Unit::Second)
                    .range(0.1..=300.0)
                    .decimals(2),
            )
        });
        count_row(
            ui,
            &tr!("field-transition-layers"),
            &mut s.transition_layer_count,
            0..=200,
        )
        .on_hover_text(tr!("field-transition-layers-hint"));
        value_row(ui, tr!("field-exposure"), |ui| {
            ui.add(
                UnitField::new(&mut s.normal.exposure_s, Unit::Second)
                    .range(0.1..=300.0)
                    .decimals(2),
            )
        });
        pwm_row(ui, &tr!("field-bottom-light-power"), &mut s.bottom.pwm);
        pwm_row(ui, &tr!("field-light-power"), &mut s.normal.pwm);
        // A setting that states none heats to the default, as the file formats write it.
        let mut celsius = s
            .heating_temperature_c
            .unwrap_or(karasu_core::profiles::DEFAULT_HEATING_C);
        let changed = value_row(ui, tr!("field-heating"), |ui| {
            ui.add(
                UnitField::new(&mut celsius, Unit::Celsius)
                    .range(karasu_core::profiles::HEATING_C)
                    .step(0.5)
                    .decimals(1),
            )
        })
        .on_hover_text(tr!("field-heating-hint"))
        .changed();
        if changed {
            s.heating_temperature_c = Some(celsius);
        }
    }

    /// Turn two stages on or off for both layer classes, keeping the two stages to restore.
    fn set_two_stages(&mut self, two: bool) {
        let s = &mut self.draft.settings;
        for (k, layer) in [&mut s.normal, &mut s.bottom].into_iter().enumerate() {
            let mut b = bands(layer);
            if two && b.len() == 1 {
                // The stages the user had, while the total travel is still theirs; otherwise
                // two halves of the single stage.
                let total = b[0].distance;
                b = match self.split[k].take() {
                    Some(saved)
                        if (saved.iter().map(|x| x.distance).sum::<f64>() - total).abs() < 1e-9 =>
                    {
                        saved
                    }
                    _ => {
                        let half = Band {
                            distance: b[0].distance / 2.0,
                            retract: b[0].retract / 2.0,
                            ..b[0]
                        };
                        vec![half, half]
                    }
                };
            } else if !two && b.len() == 2 {
                self.split[k] = Some(b.clone());
                b = vec![Band {
                    distance: b[0].distance + b[1].distance,
                    retract: b[0].retract + b[1].retract,
                    ..b[0]
                }];
            }
            set_bands(layer, &b);
        }
    }

    #[allow(clippy::too_many_lines)] // The rests, then the motion table.
    fn motion(&mut self, ui: &mut egui::Ui) {
        crate::widgets::group(ui, tr!("motion-rests"), |ui| {
            let rests = [ROW_LABEL, SHORT_FIELD + 20.0, SHORT_FIELD + 20.0];
            table_row(ui, &rests, |k, ui| {
                let text = match k {
                    1 => tr!("motion-normal"),
                    2 => tr!("motion-bottom"),
                    _ => String::new(),
                };
                ui.label(egui::RichText::new(text).small());
            });
            let s = &mut self.draft.settings;
            for (row, (label, aka)) in [
                (tr!("rest-after-retract"), tr!("rest-after-retract-aka")),
                (tr!("rest-before-lift"), tr!("rest-before-lift-aka")),
                (tr!("rest-after-lift"), tr!("rest-after-lift-aka")),
            ]
            .into_iter()
            .enumerate()
            {
                table_row(ui, &rests, |k, ui| {
                    if k == 0 {
                        ui.add(egui::Label::new(label.clone()).truncate())
                            .on_hover_text(aka.clone());
                        return;
                    }
                    let layer = if k == 1 { &mut s.normal } else { &mut s.bottom };
                    let value = match row {
                        0 => &mut layer.wait_before_exposure_s,
                        1 => &mut layer.wait_after_exposure_s,
                        _ => &mut layer.wait_after_lift_s,
                    };
                    ui.add(
                        UnitField::new(value, Unit::Second)
                            .range(0.0..=300.0)
                            .decimals(2)
                            .width(SHORT_FIELD),
                    );
                });
            }
        });
        ui.separator();
        let s = &mut self.draft.settings;
        let mut two = s.normal.lift.len() == 2 || s.bottom.lift.len() == 2;
        let mut toggled = false;
        // Speeds in mm/min or mm/s: a choice kept for every editor and across restarts.
        let per_minute = Remembered::of(ui.ctx()).speeds_per_minute;
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(tr!("motion-title")).strong());
            ui.add_space(12.0);
            let stages = switch(ui, &mut two);
            crate::widgets::name(&stages, &tr!("motion-two-stages"));
            toggled = stages.changed();
            ui.label(tr!("motion-two-stages"));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .selectable_label(per_minute, tr!("unit-mm-per-min"))
                    .clicked()
                {
                    Remembered::change(ui.ctx(), |r| r.speeds_per_minute = true);
                }
                if ui
                    .selectable_label(!per_minute, tr!("unit-mm-per-s"))
                    .clicked()
                {
                    Remembered::change(ui.ctx(), |r| r.speeds_per_minute = false);
                }
            });
        });
        if toggled {
            self.set_two_stages(two);
        }
        let (unit, factor) = if per_minute {
            (Unit::MillimetrePerMinute, 60.0)
        } else {
            (Unit::MillimetrePerSecond, 1.0)
        };
        let columns = [
            ROW_LABEL,
            SHORT_FIELD,
            SPEED_FIELD,
            SHORT_FIELD,
            SPEED_FIELD,
        ];
        table_row(ui, &columns, |k, ui| {
            let text = match k {
                1 => tr!("motion-distance"),
                2 => tr!("motion-lift-speed"),
                3 => tr!("motion-retract-distance"),
                4 => tr!("motion-retract-speed"),
                _ => String::new(),
            };
            ui.add(egui::Label::new(egui::RichText::new(text).small()).truncate());
        });
        let s = &mut self.draft.settings;
        for (class, layer) in [
            (tr!("motion-normal"), &mut s.normal),
            (tr!("motion-bottom"), &mut s.bottom),
        ] {
            let mut b = bands(layer);
            let count = b.len();
            if count > 1 {
                ui.label(egui::RichText::new(class.clone()).strong());
            }
            for (i, band) in b.iter_mut().enumerate() {
                let label = match (count, i) {
                    (1, _) => class.clone(),
                    (_, 0) => tr!("motion-near-film"),
                    _ => tr!("motion-away-from-film"),
                };
                table_row(ui, &columns, |k, ui| match k {
                    0 => {
                        ui.add(egui::Label::new(label.clone()).truncate());
                    }
                    1 | 3 => {
                        let value = if k == 1 {
                            &mut band.distance
                        } else {
                            &mut band.retract
                        };
                        ui.add(
                            UnitField::new(value, Unit::Millimetre)
                                .range(0.01..=20.0)
                                .width(SHORT_FIELD),
                        );
                    }
                    _ => {
                        let value = if k == 2 { &mut band.up } else { &mut band.down };
                        let mut shown = *value * factor;
                        ui.add(
                            UnitField::new(&mut shown, unit)
                                .range(0.01 * factor..=20.0 * factor)
                                .width(SPEED_FIELD),
                        );
                        *value = shown / factor;
                    }
                });
            }
            set_bands(layer, &b);
        }
        ui.add(egui::Label::new(egui::RichText::new(tr!("motion-hint")).small()).wrap());
    }

    fn image(&mut self, ui: &mut egui::Ui) {
        let image = &mut self.draft.image;
        value_row(ui, tr!("field-antialias"), |ui| {
            egui::ComboBox::from_id_salt("setting-antialias")
                .selected_text(super::antialias_label(image.antialias))
                .width(crate::widgets::FIELD_WIDTH)
                .show_ui(ui, |ui| {
                    for samples in super::ANTIALIAS_LEVELS {
                        ui.selectable_value(
                            &mut image.antialias,
                            samples,
                            super::antialias_label(samples),
                        );
                    }
                })
                .response
        })
        .on_hover_text(tr!("field-antialias-hint"));
        if image.antialias == 1 {
            image.min_gray = 0;
        }
        ui.add_enabled_ui(image.antialias > 1, |ui| {
            let mut gray = f64::from(image.min_gray);
            value_row(ui, tr!("field-min-gray"), |ui| {
                ui.add(
                    UnitField::new(&mut gray, Unit::Count)
                        .range(0.0..=255.0)
                        .step(1.0)
                        .decimals(0),
                )
            })
            .on_hover_text(tr!("field-min-gray-hint"));
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // Within 0..=255.
            {
                image.min_gray = gray.round() as u8;
            }
        });
        let max = f64::from(MAX_COMPENSATION_PX);
        let mut xy = f64::from(image.xy_compensation_px);
        value_row(ui, tr!("field-xy-compensation"), |ui| {
            ui.add(
                UnitField::new(&mut xy, Unit::Count)
                    .range(-max..=max)
                    .step(1.0)
                    .decimals(0),
            )
        })
        .on_hover_text(tr!("field-xy-compensation-hint"));
        #[allow(clippy::cast_possible_truncation)] // Within ±64.
        {
            image.xy_compensation_px = xy.round() as i16;
        }
        let mut bottom_on = image.bottom_xy_compensation_px.is_some();
        let mut bottom = f64::from(image.bottom_xy_compensation_px.unwrap_or(0));
        crate::widgets::check_value_row(
            ui,
            &mut bottom_on,
            tr!("field-bottom-xy-compensation"),
            |ui| {
                ui.add(
                    UnitField::new(&mut bottom, Unit::Count)
                        .range(-max..=max)
                        .step(1.0)
                        .decimals(0),
                )
            },
        );
        #[allow(clippy::cast_possible_truncation)] // Within ±64.
        {
            image.bottom_xy_compensation_px = bottom_on.then(|| bottom.round() as i16);
        }
    }
}

/// A whole count in `range`, in a value field.
fn count_row(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut u32,
    range: std::ops::RangeInclusive<u32>,
) -> egui::Response {
    let mut shown = f64::from(*value);
    let response = value_row(ui, label, |ui| {
        ui.add(
            UnitField::new(&mut shown, Unit::Count)
                .range(f64::from(*range.start())..=f64::from(*range.end()))
                .step(1.0)
                .decimals(0),
        )
    });
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // Within `range`.
    {
        *value = shown.round() as u32;
    }
    response
}

/// A light power from 1 to 255.
fn pwm_row(ui: &mut egui::Ui, label: &str, value: &mut u8) {
    let mut shown = f64::from(*value);
    value_row(ui, label, |ui| {
        ui.add(
            UnitField::new(&mut shown, Unit::Count)
                .range(1.0..=255.0)
                .step(1.0)
                .decimals(0),
        )
    })
    .on_hover_text(tr!("field-light-power-hint"));
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // Within 1..=255.
    {
        *value = shown.round() as u8;
    }
}

/// A newer catalog revision's changes, applied or dismissed by the user (TD-106 point 5).
pub(super) struct UpdateReview {
    kind: ProfileKind,
    id: String,
}

impl UpdateReview {
    pub(super) const fn new(kind: ProfileKind, id: String) -> Self {
        Self { kind, id }
    }

    pub(super) fn show(&mut self, ctx: &egui::Context, library: &mut Library, out: &mut Outcome) {
        let Some(update) = library.update(self.kind, &self.id) else {
            out.close = true;
            return;
        };
        let name = match self.kind {
            ProfileKind::Printer => library.printer(&self.id).map(|p| p.name.clone()),
            ProfileKind::Material => library.material(&self.id).map(|m| m.name.clone()),
            _ => library.setting(&self.id).map(|s| s.name.clone()),
        }
        .unwrap_or_default();
        let (mut apply, mut dismiss) = (false, false);
        let response = dialog::show(
            ctx,
            "update-review",
            &tr!("update-title", name = &name),
            Size::Fixed(560.0),
            |ui, part| match part {
                Part::Body => {
                    ui.add(
                        egui::Label::new(tr!("update-explain", revision = update.revision)).wrap(),
                    );
                    egui::Grid::new("update-changes")
                        .num_columns(3)
                        .striped(true)
                        .spacing([16.0, 4.0])
                        .show(ui, |ui| {
                            ui.label(egui::RichText::new(tr!("update-setting")).strong());
                            ui.label(egui::RichText::new(tr!("update-yours")).strong());
                            ui.label(egui::RichText::new(tr!("update-catalog")).strong());
                            ui.end_row();
                            for change in &update.changes {
                                ui.label(&change.field);
                                ui.label(&change.ours);
                                ui.label(
                                    egui::RichText::new(&change.theirs)
                                        .color(theme::ACCENT.color()),
                                );
                                ui.end_row();
                            }
                        });
                    ui.add(
                        egui::Label::new(egui::RichText::new(tr!("update-projects")).small())
                            .wrap(),
                    );
                }
                Part::Footer { enter } => {
                    apply = dialog::main_action(ui, &tr!("action-apply"), true, enter);
                    dismiss = ui.button(tr!("action-dismiss")).clicked();
                }
            },
        );
        if response.cancel {
            out.close = true;
        } else if apply || dismiss {
            match library.take_update(self.kind, &self.id, apply) {
                Ok(()) => {
                    let (level, text) = if apply {
                        (Level::Success, tr!("update-applied", name = &name))
                    } else {
                        (Level::Info, tr!("update-dismissed", name = &name))
                    };
                    out.messages.push(Message::new(level, text));
                    out.changed = true;
                    out.close = true;
                }
                Err(error) => out.fail(&error),
            }
        }
    }
}

#[cfg(test)]
impl SettingEditor {
    /// The same editor on its tab `tab`.
    pub(super) const fn on_tab(mut self, tab: usize) -> Self {
        self.tab = tab;
        self
    }

    /// Turn two stages on or off, as the switch does.
    pub(super) fn two_stages(&mut self, two: bool) {
        self.set_two_stages(two);
    }

    /// The draft's normal-layer lift stages, as (distance, speed).
    pub(super) fn normal_lift(&self) -> Vec<(f64, f64)> {
        self.draft
            .settings
            .normal
            .lift
            .iter()
            .map(|s| (s.distance_mm, s.speed_mm_s))
            .collect()
    }
}
