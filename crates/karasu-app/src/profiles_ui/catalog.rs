//! Adding a printer or a resin to the user's library: from the catalog, by vendor, or as a
//! custom one (TD-106). Both are resizable dialogs whose three columns fill their height and
//! scroll on their own.
use super::{
    forms::{catalog_facts, kind_label, PrinterForm, ResinForm},
    label_of, status, Outcome,
};
use crate::{
    dialog::{self, Part, Size},
    i18n::tr,
    messages::{Level, Message},
    theme,
    widgets::{fact_row, text_field},
};
use eframe::egui;
use karasu_core::profiles::{store::ProfileKind, SettingStatus};
use karasu_project::library::{AddedResin, CatalogMaterial, CatalogPrinter, Library};

/// The vendors column's width.
const VENDORS_WIDTH: f32 = 170.0;
/// The details column's width.
const DETAILS_WIDTH: f32 = 320.0;
/// The space a rule between columns takes.
const RULE_WIDTH: f32 = 9.0;

/// A dialog's size, as the window rules give it (UI-DESIGN, "Windows").
const SIZE: Size = Size::Resizable {
    default: egui::vec2(900.0, 600.0),
    min: egui::vec2(760.0, 480.0),
};

/// A column `width` wide and `height` high, scrolling on its own.
fn column(ui: &mut egui::Ui, id: &str, width: f32, height: f32, add: impl FnOnce(&mut egui::Ui)) {
    ui.allocate_ui_with_layout(
        egui::vec2(width, height),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            ui.set_width(width);
            ui.set_height(height);
            crate::focus::scrolled(
                egui::ScrollArea::vertical()
                    .id_salt(id)
                    .auto_shrink([false, false]),
                ui,
                add,
            );
        },
    );
}

/// A vertical rule between two columns, `height` high.
fn rule(ui: &mut egui::Ui, height: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(RULE_WIDTH, height), egui::Sense::hover());
    ui.painter().vline(
        rect.center().x,
        rect.y_range(),
        ui.visuals().widgets.noninteractive.bg_stroke,
    );
}

/// Three columns filling `ui`: the vendors, the list and the details.
fn columns(ui: &mut egui::Ui, id: &str, mut add: impl FnMut(usize, &mut egui::Ui)) {
    let height = ui.available_height();
    // Five items side by side: three columns and two rules, with the spacing between them.
    let spacing = ui.spacing().item_spacing.x * 4.0;
    let list = (ui.available_width() - VENDORS_WIDTH - DETAILS_WIDTH - 2.0 * RULE_WIDTH - spacing)
        .max(200.0);
    ui.horizontal_top(|ui| {
        column(ui, &format!("{id}-vendors"), VENDORS_WIDTH, height, |ui| {
            add(0, ui);
        });
        rule(ui, height);
        column(ui, &format!("{id}-list"), list, height, |ui| add(1, ui));
        rule(ui, height);
        column(ui, &format!("{id}-details"), DETAILS_WIDTH, height, |ui| {
            add(2, ui);
        });
    });
}

/// A list entry: selectable, as wide as its column, its text at the left.
fn list_entry(ui: &mut egui::Ui, selected: bool, text: impl Into<egui::WidgetText>) -> bool {
    let width = ui.available_width();
    ui.add(
        egui::Button::selectable(selected, (text.into(), egui::Atom::grow()))
            .min_size(egui::vec2(width, 0.0)),
    )
    .clicked()
}

/// The search field and the vendor filter, each vendor with its matching count.
fn vendors(
    ui: &mut egui::Ui,
    library: &Library,
    vendor: &mut Option<String>,
    query: &mut String,
    count: impl Fn(Option<&str>) -> usize,
) {
    let search = ui.add(text_field(query, VENDORS_WIDTH - 16.0).hint_text(tr!("catalog-search")));
    crate::widgets::name(&search, &tr!("catalog-search"));
    ui.add_space(6.0);
    ui.label(
        egui::RichText::new(tr!("catalog-vendor"))
            .small()
            .color(ui.visuals().weak_text_color()),
    );
    let all = format!("{} ({})", tr!("catalog-all"), count(None));
    if list_entry(ui, vendor.is_none(), all) {
        *vendor = None;
    }
    for v in library.catalog().vendors() {
        let n = count(Some(&v.id));
        if n > 0 {
            let selected = vendor.as_deref() == Some(v.id.as_str());
            if list_entry(ui, selected, format!("{} ({n})", v.name)) {
                *vendor = Some(v.id.clone());
            }
        }
    }
}

/// A list entry's text: its name, then a smaller line of facts.
fn entry_text(
    ui: &egui::Ui,
    name: &str,
    facts: &str,
    note: Option<String>,
) -> egui::text::LayoutJob {
    let mut text = egui::text::LayoutJob::default();
    text.append(
        name,
        0.0,
        egui::TextFormat {
            font_id: egui::FontId::proportional(14.0),
            color: ui.visuals().strong_text_color(),
            ..Default::default()
        },
    );
    text.append(
        &format!("\n{facts}"),
        0.0,
        egui::TextFormat {
            font_id: egui::FontId::proportional(11.5),
            color: ui.visuals().weak_text_color(),
            ..Default::default()
        },
    );
    if let Some(note) = note {
        text.append(
            &format!("  · {note}"),
            0.0,
            egui::TextFormat {
                font_id: egui::FontId::proportional(11.5),
                color: theme::ACCENT.color(),
                ..Default::default()
            },
        );
    }
    text
}

fn tabs(ui: &mut egui::Ui, custom: &mut bool, own: &str) {
    ui.horizontal(|ui| {
        if ui.selectable_label(!*custom, tr!("catalog-tab")).clicked() {
            *custom = false;
        }
        if ui.selectable_label(*custom, own).clicked() {
            *custom = true;
        }
    });
    ui.separator();
}

/// The resins chosen at first when the catalog printer `id` is added: its vendor's standard resin,
/// when it has print settings for the printer.
fn default_resins(library: &Library, id: &str) -> Vec<String> {
    library
        .catalog()
        .standard_resin_for(id)
        .map_or_default(|m| vec![m.profile.id.clone()])
}

/// Choose every resin `offered`, or none.
fn choose_all(chosen: &mut Vec<String>, offered: &[&CatalogMaterial], all: bool) {
    chosen.clear();
    if all {
        chosen.extend(offered.iter().map(|m| m.profile.id.clone()));
    }
}

/// The "Add printer" dialog.
pub(super) struct AddPrinter {
    custom: bool,
    vendor: Option<String>,
    query: String,
    picked: Option<String>,
    /// The name typed for the picked printer, or `None` for its free default.
    name: Option<String>,
    /// The catalog resins chosen to come with the picked printer, with their print settings for
    /// it, or `None` for the default: its vendor's standard resin (owner, 2026-11-11).
    resins: Option<Vec<String>>,
    form: PrinterForm,
}

impl AddPrinter {
    pub(super) fn new(library: &Library) -> Self {
        Self {
            custom: false,
            vendor: None,
            query: String::new(),
            picked: library
                .catalog()
                .printers()
                .first()
                .map(|p| p.profile.id.clone()),
            name: None,
            resins: None,
            form: PrinterForm::new(
                library.unique_name(ProfileKind::Printer, &tr!("printer-new-name")),
            ),
        }
    }

    fn matches(&self, vendor: Option<&str>, p: &CatalogPrinter) -> bool {
        let query = self.query.trim().to_lowercase();
        vendor.is_none_or(|v| p.profile.vendor.as_deref() == Some(v))
            && (query.is_empty()
                || p.profile
                    .model
                    .as_deref()
                    .unwrap_or(&p.profile.name)
                    .to_lowercase()
                    .contains(&query))
    }

    fn list(&mut self, ui: &mut egui::Ui, library: &Library) {
        let shown: Vec<&CatalogPrinter> = library
            .catalog()
            .printers()
            .iter()
            .filter(|p| self.matches(self.vendor.as_deref(), p))
            .collect();
        if shown.is_empty() {
            ui.add(egui::Label::new(tr!("catalog-no-match")).wrap());
        }
        for entry in shown {
            let p = &entry.profile;
            let owned = library
                .printers()
                .iter()
                .any(|u| u.based_on.as_ref().is_some_and(|b| b.id == p.id));
            let facts = format!(
                "{} × {} · {}",
                p.resolution_px[0],
                p.resolution_px[1],
                p.format_ids
                    .iter()
                    .map(|id| super::forms::format_text(id))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            let text = entry_text(
                ui,
                p.model.as_deref().unwrap_or(&p.name),
                &facts,
                owned.then(|| tr!("catalog-in-library")),
            );
            let selected = self.picked.as_deref() == Some(p.id.as_str());
            if list_entry(ui, selected, text) {
                self.picked = Some(p.id.clone());
                self.name = None;
                self.resins = None;
            }
        }
    }

    fn details(&mut self, ui: &mut egui::Ui, library: &Library) {
        let Some(id) = self.picked.clone() else {
            return;
        };
        let Some(entry) = library.catalog().printer(&id) else {
            return;
        };
        ui.label(
            egui::RichText::new(
                entry
                    .profile
                    .model
                    .as_deref()
                    .unwrap_or(&entry.profile.name),
            )
            .strong(),
        );
        catalog_facts(ui, library, &id);
        ui.separator();
        let default = library.unique_name(ProfileKind::Printer, &entry.profile.name);
        let name = self.name.get_or_insert(default);
        let taken = library.name_taken(ProfileKind::Printer, name, None);
        ui.label(tr!("field-name-in-library"));
        let edit = text_field(name, ui.available_width() - 4.0);
        let field = ui.add(if taken {
            edit.text_color(theme::ERROR.color())
        } else {
            edit
        });
        crate::widgets::name(&field, &tr!("field-name-in-library"));
        // A name in use is said in words, not by the red text alone (TD-142).
        if taken {
            let icons = crate::icons::Icons::shared(ui.ctx());
            crate::widgets::state_line(
                ui,
                icons.as_deref(),
                Some(crate::icons::Icon::Error),
                &tr!("library-name-taken", name = name.as_str()),
                theme::ERROR.color(),
            );
        }
        ui.add(egui::Label::new(egui::RichText::new(tr!("name-unique-hint")).small()).wrap());
        let offered = library.catalog().resins_for(&id);
        if offered.is_empty() {
            ui.add(
                egui::Label::new(egui::RichText::new(tr!("catalog-no-settings")).small()).wrap(),
            );
            return;
        }
        ui.add_space(4.0);
        ui.add(egui::Label::new(tr!("catalog-resins-to-add")).wrap());
        let chosen = self
            .resins
            .get_or_insert_with(|| default_resins(library, &id));
        // A box that ticks every resin, or clears them (mixed while only some are), as the
        // sidebar's for models (owner, 2026-11-11).
        let some = offered
            .iter()
            .filter(|m| chosen.contains(&m.profile.id))
            .count();
        ui.horizontal(|ui| {
            let mut all = some == offered.len();
            let response = ui
                .add(
                    egui::Checkbox::new(&mut all, tr!("catalog-resins-all"))
                        .indeterminate(some > 0 && some < offered.len()),
                )
                .on_hover_text(tr!("catalog-resins-all-hint"));
            crate::widgets::name(&response, &tr!("catalog-resins-all"));
            if response.changed() {
                // Mixed or none: tick them all; all: clear them.
                choose_all(chosen, &offered, some < offered.len());
            }
            ui.label(
                egui::RichText::new(tr!(
                    "catalog-resins-chosen",
                    count = some,
                    total = offered.len()
                ))
                .color(ui.visuals().weak_text_color()),
            );
        });
        ui.separator();
        for m in offered {
            let mut on = chosen.contains(&m.profile.id);
            if ui.checkbox(&mut on, &m.profile.name).changed() {
                if on {
                    chosen.push(m.profile.id.clone());
                } else {
                    chosen.retain(|r| r != &m.profile.id);
                }
            }
        }
    }

    fn can_add(&self) -> bool {
        if self.custom {
            self.form.complete()
        } else {
            self.picked.is_some() && self.name.as_ref().is_some_and(|n| !n.trim().is_empty())
        }
    }

    fn add(&self, library: &mut Library, out: &mut Outcome) {
        let result = if self.custom {
            library
                .add_custom_printer(self.form.printer())
                .map(|id| (id, self.form.name.trim().to_owned()))
        } else {
            let id = self.picked.clone().unwrap_or_default();
            let name = self.name.clone().unwrap_or_default();
            let resins = self
                .resins
                .clone()
                .unwrap_or_else(|| default_resins(library, &id));
            let resins: Vec<&str> = resins.iter().map(String::as_str).collect();
            library
                .add_printer_from_catalog(&id, &name, &resins)
                .map(|added| (added, name.trim().to_owned()))
        };
        match result {
            Ok((id, name)) => {
                out.messages.push(Message::new(
                    Level::Success,
                    tr!("library-added", name = &name),
                ));
                out.use_printer = Some(id);
                out.changed = true;
                out.close = true;
            }
            Err(error) => out.fail(&error),
        }
    }

    pub(super) fn show(&mut self, ctx: &egui::Context, library: &mut Library, out: &mut Outcome) {
        let (mut add, mut cancel) = (false, false);
        let response = dialog::show(
            ctx,
            "add-printer",
            &tr!("add-printer-title"),
            SIZE,
            |ui, part| match part {
                Part::Body => {
                    tabs(ui, &mut self.custom, &tr!("custom-printer-tab"));
                    if self.custom {
                        crate::focus::scrolled(
                            egui::ScrollArea::vertical()
                                .id_salt("add-printer-custom")
                                .auto_shrink([false, false]),
                            ui,
                            |ui| {
                                self.form.ui(ui, library);
                                ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(tr!("custom-printer-note")).small(),
                                    )
                                    .wrap(),
                                );
                            },
                        );
                    } else {
                        let printers = library.catalog().printers();
                        columns(ui, "add-printer", |k, ui| match k {
                            0 => {
                                let counts: Vec<(Option<String>, usize)> = std::iter::once(None)
                                    .chain(
                                        library
                                            .catalog()
                                            .vendors()
                                            .iter()
                                            .map(|v| Some(v.id.clone())),
                                    )
                                    .map(|v| {
                                        let n = printers
                                            .iter()
                                            .filter(|p| self.matches(v.as_deref(), p))
                                            .count();
                                        (v, n)
                                    })
                                    .collect();
                                vendors(ui, library, &mut self.vendor, &mut self.query, |v| {
                                    counts
                                        .iter()
                                        .find(|(id, _)| id.as_deref() == v)
                                        .map_or(0, |(_, n)| *n)
                                });
                            }
                            1 => self.list(ui, library),
                            _ => self.details(ui, library),
                        });
                    }
                }
                Part::Footer { enter } => {
                    add = dialog::main_action(
                        ui,
                        &tr!("action-add-printer-short"),
                        self.can_add(),
                        enter,
                    );
                    cancel = dialog::cancel_action(ui);
                }
            },
        );
        if response.cancel || cancel {
            out.close = true;
        } else if add {
            self.add(library, out);
        }
    }
}

/// The "Add resin" dialog, for one of the user's printers.
pub(super) struct AddResin {
    printer: Option<String>,
    custom: bool,
    vendor: Option<String>,
    query: String,
    picked: Option<String>,
    /// The printer's setting to start from, or `None` to add the resin without one.
    from: Option<String>,
    from_chosen: bool,
    form: ResinForm,
}

impl AddResin {
    pub(super) fn new(printer: Option<String>) -> Self {
        Self {
            printer,
            custom: false,
            vendor: None,
            query: String::new(),
            picked: None,
            from: None,
            from_chosen: false,
            form: ResinForm::new(),
        }
    }

    /// The catalog identity of the printer's model, when the printer is a catalog copy.
    fn model(&self, library: &Library) -> Option<String> {
        self.printer
            .as_ref()
            .and_then(|p| library.printer(p))
            .and_then(|p| p.based_on.as_ref())
            .map(|b| b.id.clone())
    }

    fn official(&self, library: &Library, resin: &str) -> bool {
        self.model(library)
            .is_some_and(|m| library.catalog().settings_for(&m, resin).next().is_some())
    }

    fn matches(&self, vendor: Option<&str>, m: &CatalogMaterial) -> bool {
        let query = self.query.trim().to_lowercase();
        vendor.is_none_or(|v| m.profile.vendor.as_deref() == Some(v))
            && (query.is_empty() || m.profile.name.to_lowercase().contains(&query))
    }

    /// The start-from choice for the printer: its settings, or adding without one.
    fn start_from(&mut self, ui: &mut egui::Ui, library: &Library, printer: &str) {
        let sources: Vec<_> = library
            .settings()
            .iter()
            .filter(|s| s.printer_id == printer)
            .collect();
        if !self.from_chosen {
            self.from = sources.first().map(|s| s.id.clone());
            self.from_chosen = true;
        }
        if sources.is_empty() {
            ui.add(egui::Label::new(tr!("add-resin-no-source")).wrap());
            return;
        }
        ui.add(egui::Label::new(tr!("add-resin-start-from")).wrap());
        let resin_of = |id: &str| {
            library
                .material(id)
                .map_or_else(|| id.to_owned(), |m| m.name.clone())
        };
        let label = sources
            .iter()
            .find(|s| Some(&s.id) == self.from.as_ref())
            .map_or_else(
                || tr!("add-resin-without-setting"),
                |s| format!("{} · {}", resin_of(&s.resin_id), label_of(s)),
            );
        egui::ComboBox::from_id_salt("add-resin-from")
            .selected_text(label)
            .width(ui.available_width() - 8.0)
            .show_ui(ui, |ui| {
                for s in &sources {
                    let text = format!(
                        "{} · {} ({})",
                        resin_of(&s.resin_id),
                        label_of(s),
                        status(s.status).0
                    );
                    if ui
                        .selectable_label(self.from.as_ref() == Some(&s.id), text)
                        .clicked()
                    {
                        self.from = Some(s.id.clone());
                    }
                }
                if ui
                    .selectable_label(self.from.is_none(), tr!("add-resin-without-setting"))
                    .clicked()
                {
                    self.from = None;
                }
            });
    }

    fn list(&mut self, ui: &mut egui::Ui, library: &Library) {
        let printer_name = self
            .printer
            .as_ref()
            .and_then(|p| library.printer(p))
            .map_or_else(String::new, |p| p.name.clone());
        let shown: Vec<&CatalogMaterial> = library
            .catalog()
            .materials()
            .iter()
            .filter(|m| self.matches(self.vendor.as_deref(), m))
            .collect();
        for (ready, title) in [
            (true, tr!("add-resin-official", printer = &printer_name)),
            (false, tr!("add-resin-others")),
        ] {
            let group: Vec<&&CatalogMaterial> = shown
                .iter()
                .filter(|m| self.official(library, &m.profile.id) == ready)
                .collect();
            if group.is_empty() || (ready && self.printer.is_none()) {
                continue;
            }
            ui.label(
                egui::RichText::new(title)
                    .small()
                    .color(ui.visuals().weak_text_color()),
            );
            for entry in group {
                let m = &entry.profile;
                let owned = library
                    .materials()
                    .iter()
                    .any(|u| u.based_on.as_ref().is_some_and(|b| b.id == m.id));
                let vendor = m
                    .vendor
                    .as_ref()
                    .and_then(|v| library.catalog().vendor(v))
                    .map_or_else(String::new, |v| v.name.clone());
                let text = entry_text(
                    ui,
                    &m.name,
                    &format!("{vendor} · {}", kind_label(m.kind)),
                    owned.then(|| tr!("catalog-in-library")),
                );
                let selected = self.picked.as_deref() == Some(m.id.as_str());
                if list_entry(ui, selected, text) {
                    self.picked = Some(m.id.clone());
                }
            }
        }
    }

    fn details(&mut self, ui: &mut egui::Ui, library: &Library) {
        let Some(entry) = self
            .picked
            .as_ref()
            .and_then(|id| library.catalog().material(id))
        else {
            return;
        };
        let m = &entry.profile;
        ui.label(egui::RichText::new(&m.name).strong());
        fact_row(ui, &tr!("field-kind"), &kind_label(m.kind));
        let printers: Vec<String> = library
            .catalog()
            .settings()
            .iter()
            .filter(|s| s.profile.resin_id == m.id)
            .filter_map(|s| library.catalog().printer(&s.profile.printer_id))
            .map(|p| p.profile.name.clone())
            .collect();
        let official_for = if printers.is_empty() {
            "–".to_owned()
        } else {
            printers.join(", ")
        };
        fact_row(ui, &tr!("add-resin-official-for"), &official_for);
        fact_row(ui, &tr!("field-sources"), &entry.sources);
        ui.separator();
        let Some(printer) = self.printer.clone() else {
            ui.add(egui::Label::new(tr!("add-resin-material-only")).wrap());
            return;
        };
        let id = m.id.clone();
        if self.official(library, &id) {
            ui.label(tr!("add-resin-settings-added"));
            if let Some(model) = self.model(library) {
                for s in library.catalog().settings_for(&model, &id) {
                    let (text, colour) = status(s.profile.status);
                    ui.horizontal_wrapped(|ui| {
                        ui.label(label_of(&s.profile));
                        ui.label(egui::RichText::new(text).small().color(colour));
                    });
                }
            }
        } else if library.materials().iter().any(|u| {
            u.based_on.as_ref().is_some_and(|b| b.id == id)
                && !library.settings_for(&printer, &u.id).is_empty()
        }) {
            ui.add(egui::Label::new(tr!("add-resin-already-set-up")).wrap());
        } else {
            self.start_from(ui, library, &printer);
        }
    }

    fn report(&self, library: &Library, added: &AddedResin, name: &str, out: &mut Outcome) {
        let printer = self
            .printer
            .as_ref()
            .and_then(|p| library.printer(p))
            .map(|p| p.name.clone());
        let starting = added
            .settings
            .iter()
            .filter_map(|id| library.setting(id))
            .any(|s| s.status == SettingStatus::StartingPoint);
        let message = match (&printer, added.settings.is_empty(), starting) {
            (Some(printer), true, _) => Message::new(
                Level::Info,
                tr!("add-resin-no-setting-added", name = name, printer = printer),
            ),
            (Some(_), false, true) => {
                Message::new(Level::Warning, tr!("setup-created", resin = name))
            }
            _ => Message::new(Level::Success, tr!("library-added", name = name)),
        };
        out.messages.push(message);
        out.use_resin = Some((added.resin.clone(), added.settings.first().cloned()));
        out.changed = true;
        out.close = true;
    }

    fn add(&self, library: &mut Library, out: &mut Outcome) {
        let printer = self.printer.clone();
        if self.custom {
            let material = self.form.material();
            let from = printer.as_ref().and(self.from.clone());
            match library.add_custom_material(material.clone(), printer.as_deref(), from.as_deref())
            {
                Ok(added) => self.report(library, &added, &material.name, out),
                Err(error) => out.fail(&error),
            }
            return;
        }
        let id = self.picked.clone().unwrap_or_default();
        let name = library
            .catalog()
            .material(&id)
            .map_or_default(|m| m.profile.name.clone());
        let from = if self.official(library, &id) {
            None
        } else {
            self.from.clone()
        };
        match library.add_material_from_catalog(&id, printer.as_deref(), from.as_deref()) {
            Ok(added) => self.report(library, &added, &name, out),
            Err(error) => out.fail(&error),
        }
    }

    fn printer_choice(&mut self, ui: &mut egui::Ui, library: &Library) {
        ui.horizontal(|ui| {
            ui.label(tr!("add-resin-for"));
            let label = self
                .printer
                .as_ref()
                .and_then(|p| library.printer(p))
                .map_or_else(|| tr!("add-resin-no-printer"), |p| p.name.clone());
            let combo = egui::ComboBox::from_id_salt("add-resin-printer")
                .selected_text(label)
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(self.printer.is_none(), tr!("add-resin-no-printer"))
                        .clicked()
                    {
                        self.printer = None;
                        self.from_chosen = false;
                    }
                    for p in library.printers() {
                        if ui
                            .selectable_label(self.printer.as_ref() == Some(&p.id), &p.name)
                            .clicked()
                        {
                            self.printer = Some(p.id.clone());
                            self.from_chosen = false;
                        }
                    }
                });
            crate::widgets::name(&combo.response, &tr!("add-resin-for"));
        });
    }

    pub(super) fn show(&mut self, ctx: &egui::Context, library: &mut Library, out: &mut Outcome) {
        if self.picked.is_none() {
            let materials = library.catalog().materials();
            self.picked = materials
                .iter()
                .find(|m| self.official(library, &m.profile.id))
                .or_else(|| materials.first())
                .map(|m| m.profile.id.clone());
        }
        let (mut add, mut cancel) = (false, false);
        let response = dialog::show(
            ctx,
            "add-resin",
            &tr!("add-resin-title"),
            SIZE,
            |ui, part| match part {
                Part::Body => {
                    self.printer_choice(ui, library);
                    tabs(ui, &mut self.custom, &tr!("custom-resin-tab"));
                    if self.custom {
                        crate::focus::scrolled(
                            egui::ScrollArea::vertical()
                                .id_salt("add-resin-custom")
                                .auto_shrink([false, false]),
                            ui,
                            |ui| {
                                ui.horizontal_top(|ui| {
                                    ui.allocate_ui_with_layout(
                                        egui::vec2(380.0, 0.0),
                                        egui::Layout::top_down(egui::Align::Min),
                                        |ui| {
                                            ui.set_width(380.0);
                                            self.form.ui(ui, library);
                                        },
                                    );
                                    ui.add_space(16.0);
                                    if let Some(printer) = self.printer.clone() {
                                        ui.allocate_ui_with_layout(
                                            egui::vec2(380.0, 0.0),
                                            egui::Layout::top_down(egui::Align::Min),
                                            |ui| {
                                                ui.set_width(380.0);
                                                self.start_from(ui, library, &printer);
                                            },
                                        );
                                    }
                                });
                            },
                        );
                    } else {
                        let materials = library.catalog().materials();
                        columns(ui, "add-resin", |k, ui| match k {
                            0 => {
                                let counts: Vec<(Option<String>, usize)> = std::iter::once(None)
                                    .chain(
                                        library
                                            .catalog()
                                            .vendors()
                                            .iter()
                                            .map(|v| Some(v.id.clone())),
                                    )
                                    .map(|v| {
                                        let n = materials
                                            .iter()
                                            .filter(|m| self.matches(v.as_deref(), m))
                                            .count();
                                        (v, n)
                                    })
                                    .collect();
                                vendors(ui, library, &mut self.vendor, &mut self.query, |v| {
                                    counts
                                        .iter()
                                        .find(|(id, _)| id.as_deref() == v)
                                        .map_or(0, |(_, n)| *n)
                                });
                            }
                            1 => self.list(ui, library),
                            _ => self.details(ui, library),
                        });
                    }
                }
                Part::Footer { enter } => {
                    let can = if self.custom {
                        !self.form.material().name.is_empty()
                    } else {
                        self.picked.is_some()
                    };
                    add = dialog::main_action(ui, &tr!("action-add-resin-short"), can, enter);
                    cancel = dialog::cancel_action(ui);
                }
            },
        );
        if response.cancel || cancel {
            out.close = true;
        } else if add {
            self.add(library, out);
        }
    }
}

#[cfg(test)]
impl AddPrinter {
    /// The same dialog on its custom tab.
    pub(super) const fn custom(mut self) -> Self {
        self.custom = true;
        self
    }

    /// The same dialog with the catalog printer `id` picked.
    pub(super) fn picking(mut self, id: &str) -> Self {
        self.picked = Some(id.to_owned());
        self.name = None;
        self.resins = None;
        self
    }

    /// The resins chosen so far, `None` before the details showed the picked printer's.
    pub(super) fn chosen(&self) -> Option<&[String]> {
        self.resins.as_deref()
    }

    /// Tick every resin offered for the picked printer, or clear them, as the box over the list
    /// does.
    pub(super) fn tick_all(&mut self, library: &Library, all: bool) {
        let picked = self.picked.as_deref().unwrap_or_default();
        let offered = library.catalog().resins_for(picked);
        choose_all(self.resins.get_or_insert_with(Vec::new), &offered, all);
    }

    /// Tick the box of the catalog resin `id`, as a click on it does.
    pub(super) fn tick(&mut self, id: &str) {
        self.resins.get_or_insert_with(Vec::new).push(id.to_owned());
    }

    /// Add the picked printer, under its default name unless one was typed, as the dialog's main
    /// action does.
    pub(super) fn confirm(&mut self, library: &mut Library, out: &mut Outcome) {
        if self.name.is_none() {
            let picked = self.picked.as_deref().unwrap_or_default();
            let model = library
                .catalog()
                .printer(picked)
                .map(|p| p.profile.name.clone());
            self.name = model.map(|m| library.unique_name(ProfileKind::Printer, &m));
        }
        self.add(library, out);
    }
}

#[cfg(test)]
impl AddResin {
    /// The same dialog on its custom tab.
    pub(super) const fn custom(mut self) -> Self {
        self.custom = true;
        self
    }
}
