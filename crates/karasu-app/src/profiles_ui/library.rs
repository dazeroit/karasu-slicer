//! The library dialog (TD-106): the user's printers, resins and print settings, with their
//! state against the catalog, and editing, duplicating, exporting, importing and deleting.
//! Editors and the catalog dialogs open over it and return to it.
use super::{
    catalog::{AddPrinter, AddResin},
    editors::{PrinterEditor, ResinEditor, SetUp, SettingEditor, UpdateReview},
    forms::{format_text, kind_label},
    label_of, status, Dialog, Outcome,
};
use crate::{
    dialog::{self, Part, Size},
    i18n::tr,
    icons::{Icon, Icons},
    messages::{Level, Message},
    theme,
    widgets::outlined_icon_button,
};
use eframe::egui;
use karasu_core::profiles::store::ProfileKind;
use karasu_project::library::Library;

/// One row of the library's table.
struct Row {
    id: String,
    name: String,
    /// A resin's swatch, from its material (TD-118).
    resin: Option<karasu_core::profiles::ResinMaterial>,
    detail: String,
    based_on: String,
    state: (String, egui::Color32),
    update: bool,
}

/// A deletion waiting for confirmation.
#[derive(Clone)]
struct Deletion {
    kind: ProfileKind,
    id: String,
    name: String,
    text: String,
}

/// The library dialog's tab and pending deletion.
#[derive(Default)]
pub(super) struct LibraryWindow {
    tab: usize,
    deleting: Option<Deletion>,
}

const KINDS: [ProfileKind; 3] = [
    ProfileKind::Printer,
    ProfileKind::Material,
    ProfileKind::PrintSetting,
];

/// The state column's width.
const STATE_COLUMN: f32 = 150.0;
/// The space between columns, and around a row's content.
const GAP: f32 = 12.0;
/// A row's inner margin, across and down.
const ROW_MARGIN: [i8; 2] = [8, 6];

/// The widths of the table's columns, name, origin, state and actions, sharing `width`: the
/// state and the four action buttons keep theirs, the name and the origin share the rest.
fn columns(ui: &egui::Ui, width: f32) -> [f32; 4] {
    let actions = 4.0 * ui.spacing().interact_size.y + 3.0 * ui.spacing().item_spacing.x;
    let rest =
        (width - 2.0 * f32::from(ROW_MARGIN[0]) - actions - STATE_COLUMN - 3.0 * GAP).max(200.0);
    [rest * 0.55, rest * 0.45, STATE_COLUMN, actions]
}

/// A resin's facts in the library: its vendor, its kind, and its colour when it has one.
fn resin_detail(vendor: &str, m: &karasu_core::profiles::ResinMaterial) -> String {
    match &m.colour {
        Some(colour) => format!("{vendor} · {} · {colour}", kind_label(m.kind)),
        None => format!("{vendor} · {}", kind_label(m.kind)),
    }
}

fn rows(library: &Library, kind: ProfileKind) -> Vec<Row> {
    let based =
        |b: Option<&karasu_core::profiles::CatalogRef>, catalog_name: Option<String>| match b {
            Some(b) => tr!(
                "library-based-on",
                name = catalog_name.unwrap_or_else(|| b.id.clone()),
                revision = b.revision
            ),
            None => tr!("library-custom"),
        };
    let state = |update: bool, custom: bool| {
        if update {
            (tr!("library-state-update"), theme::ACCENT.color())
        } else if custom {
            (tr!("library-custom"), egui::Color32::GRAY)
        } else {
            (tr!("library-state-current"), theme::SUCCESS.color())
        }
    };
    match kind {
        ProfileKind::Printer => library
            .printers()
            .iter()
            .map(|p| {
                let update = library.update(kind, &p.id).is_some();
                let catalog = p
                    .based_on
                    .as_ref()
                    .and_then(|b| library.catalog().printer(&b.id))
                    .and_then(|c| c.profile.model.clone());
                let formats: Vec<String> = p.format_ids.iter().map(|f| format_text(f)).collect();
                Row {
                    id: p.id.clone(),
                    name: p.name.clone(),
                    resin: None,
                    detail: format!(
                        "{} × {} · {}",
                        p.resolution_px[0],
                        p.resolution_px[1],
                        formats.join(", ")
                    ),
                    based_on: based(p.based_on.as_ref(), catalog),
                    state: state(update, p.based_on.is_none()),
                    update,
                }
            })
            .collect(),
        ProfileKind::Material => library
            .materials()
            .iter()
            .map(|m| {
                let update = library.update(kind, &m.id).is_some();
                let catalog = m
                    .based_on
                    .as_ref()
                    .and_then(|b| library.catalog().material(&b.id))
                    .map(|c| c.profile.name.clone());
                let vendor = m
                    .vendor
                    .as_ref()
                    .and_then(|v| library.catalog().vendor(v))
                    .map_or_else(String::new, |v| v.name.clone());
                Row {
                    id: m.id.clone(),
                    name: m.name.clone(),
                    resin: Some(m.clone()),
                    detail: resin_detail(&vendor, m),
                    based_on: based(m.based_on.as_ref(), catalog),
                    state: state(update, m.based_on.is_none()),
                    update,
                }
            })
            .collect(),
        _ => library
            .settings()
            .iter()
            .map(|s| {
                let update = library.update(kind, &s.id).is_some();
                let printer = library
                    .printer(&s.printer_id)
                    .map_or_else(|| s.printer_id.clone(), |p| p.name.clone());
                let resin = library
                    .material(&s.resin_id)
                    .map_or_else(|| s.resin_id.clone(), |m| m.name.clone());
                Row {
                    id: s.id.clone(),
                    name: label_of(s),
                    resin: None,
                    detail: format!("{printer} · {resin}"),
                    based_on: s.source.clone().unwrap_or_else(|| tr!("library-custom")),
                    state: if update {
                        (tr!("library-state-update"), theme::ACCENT.color())
                    } else {
                        status(s.status)
                    },
                    update,
                }
            })
            .collect(),
    }
}

/// A table cell `width` wide whose text wraps, so the table keeps its width.
fn cell(ui: &mut egui::Ui, width: f32, add: impl FnOnce(&mut egui::Ui)) {
    ui.allocate_ui_with_layout(
        egui::vec2(width, 0.0),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            ui.set_width(width);
            add(ui);
        },
    );
}

impl LibraryWindow {
    pub(super) fn show(
        &mut self,
        ctx: &egui::Context,
        library: &mut Library,
        icons: Option<&Icons>,
        out: &mut Outcome,
    ) {
        let kind = KINDS[self.tab.min(2)];
        let mut close = false;
        let size = Size::Resizable {
            default: egui::vec2(900.0, 560.0),
            min: egui::vec2(760.0, 400.0),
        };
        let response = dialog::show(
            ctx,
            "library-window",
            &tr!("library-title"),
            size,
            |ui, part| match part {
                Part::Body => self.body(ui, library, kind, icons, out),
                Part::Footer { enter } => {
                    close = dialog::main_action(ui, &tr!("action-close"), true, enter);
                    if ui.button(tr!("action-import")).clicked() {
                        import(library, out);
                    }
                    let first_printer = library.printers().first().map(|p| p.id.clone());
                    let (add, can) = match kind {
                        ProfileKind::Printer => (tr!("action-add-printer"), true),
                        ProfileKind::Material => (tr!("action-add-resin"), true),
                        // A print setting needs a printer and a resin.
                        _ => (
                            tr!("action-new-setting"),
                            first_printer.is_some() && !library.materials().is_empty(),
                        ),
                    };
                    if ui.add_enabled(can, egui::Button::new(add)).clicked() {
                        out.push = Some(match kind {
                            ProfileKind::Printer => Dialog::AddPrinter(AddPrinter::new(library)),
                            ProfileKind::Material => Dialog::AddResin(AddResin::new(first_printer)),
                            _ => Dialog::SetUp(SetUp::choosing(
                                first_printer.unwrap_or_default(),
                                library.materials()[0].id.clone(),
                            )),
                        });
                    }
                }
            },
        );
        if response.cancel || close {
            out.close = true;
        }
        if let Some(deletion) = self.deleting.clone() {
            match dialog::confirm(
                ctx,
                "library-delete",
                &tr!("library-delete-title", name = &deletion.name),
                &format!("{}\n{}", deletion.text, tr!("library-delete-projects")),
                &tr!("action-delete-profile"),
                true,
            ) {
                Some(true) => {
                    match library.remove(deletion.kind, &deletion.id) {
                        Ok(()) => {
                            out.messages.push(Message::new(
                                Level::Info,
                                tr!("library-deleted", name = &deletion.name),
                            ));
                            out.changed = true;
                        }
                        Err(error) => out.fail(&error),
                    }
                    self.deleting = None;
                }
                Some(false) => self.deleting = None,
                None => {}
            }
        }
    }

    fn body(
        &mut self,
        ui: &mut egui::Ui,
        library: &mut Library,
        kind: ProfileKind,
        icons: Option<&Icons>,
        out: &mut Outcome,
    ) {
        ui.horizontal(|ui| {
            for (k, label) in [
                tr!("library-tab-printers"),
                tr!("library-tab-resins"),
                tr!("library-tab-settings"),
            ]
            .into_iter()
            .enumerate()
            {
                let count = match KINDS[k] {
                    ProfileKind::Printer => library.printers().len(),
                    ProfileKind::Material => library.materials().len(),
                    _ => library.settings().len(),
                };
                if ui
                    .selectable_label(self.tab == k, format!("{label} ({count})"))
                    .clicked()
                {
                    self.tab = k;
                }
            }
        });
        ui.separator();
        let rows = rows(library, kind);
        // The scroll bar's room is kept so the columns do not move when it appears.
        let width = ui.available_width() - ui.spacing().scroll.allocated_width();
        let widths = columns(ui, width);
        header(ui, &widths);
        if rows.is_empty() {
            ui.add(egui::Label::new(tr!("library-tab-empty")).wrap());
        }
        let footer_note = 30.0;
        crate::focus::scrolled(
            egui::ScrollArea::vertical()
                .id_salt("library-rows")
                .max_height((ui.available_height() - footer_note).max(80.0))
                .auto_shrink([false, false]),
            ui,
            |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for (k, row) in rows.iter().enumerate() {
                    self.row(ui, library, kind, (k, row), (width, &widths), icons, out);
                }
            },
        );
        ui.add(egui::Label::new(egui::RichText::new(tr!("library-footer")).small()).wrap());
    }

    /// One row: its background striped, and lit under the pointer; then its cells.
    #[allow(clippy::too_many_arguments)] // The row, its place and the actions' context.
    fn row(
        &mut self,
        ui: &mut egui::Ui,
        library: &mut Library,
        kind: ProfileKind,
        (index, row): (usize, &Row),
        (width, widths): (f32, &[f32; 4]),
        icons: Option<&Icons>,
        out: &mut Outcome,
    ) {
        let background = ui.painter().add(egui::Shape::Noop);
        let rect = egui::Frame::NONE
            .inner_margin(egui::Margin::symmetric(ROW_MARGIN[0], ROW_MARGIN[1]))
            .show(ui, |ui| {
                ui.set_width(width - 2.0 * f32::from(ROW_MARGIN[0]));
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = GAP;
                    self.cells(ui, library, kind, row, widths, icons, out);
                });
            })
            .response
            .rect;
        let fill = if ui.rect_contains_pointer(rect) {
            ui.visuals().widgets.hovered.weak_bg_fill
        } else if index % 2 == 1 {
            ui.visuals().faint_bg_color
        } else {
            egui::Color32::TRANSPARENT
        };
        ui.painter()
            .set(background, egui::Shape::rect_filled(rect, 4.0, fill));
    }

    #[allow(clippy::too_many_arguments)] // The row and the actions' context.
    fn cells(
        &mut self,
        ui: &mut egui::Ui,
        library: &mut Library,
        kind: ProfileKind,
        row: &Row,
        widths: &[f32; 4],
        icons: Option<&Icons>,
        out: &mut Outcome,
    ) {
        cell(ui, widths[0], |ui| {
            if row.resin.is_some() {
                ui.horizontal(|ui| {
                    super::swatch(ui, row.resin.as_ref());
                    ui.add(egui::Label::new(egui::RichText::new(&row.name).strong()).wrap());
                });
            } else {
                ui.add(egui::Label::new(egui::RichText::new(&row.name).strong()).wrap());
            }
            ui.add(
                egui::Label::new(
                    egui::RichText::new(&row.detail)
                        .small()
                        .color(ui.visuals().weak_text_color()),
                )
                .wrap(),
            );
        });
        cell(ui, widths[1], |ui| {
            ui.add(egui::Label::new(egui::RichText::new(&row.based_on).small()).wrap());
        });
        cell(ui, widths[2], |ui| {
            ui.add(
                egui::Label::new(egui::RichText::new(&row.state.0).small().color(row.state.1))
                    .wrap(),
            );
            if row.update && ui.small_button(tr!("action-review")).clicked() {
                out.push = Some(Dialog::Update(UpdateReview::new(kind, row.id.clone())));
            }
        });
        cell(ui, widths[3], |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                if outlined_icon_button(ui, icons, Icon::Edit, &tr!("library-action-edit"))
                    .clicked()
                {
                    out.push = match kind {
                        ProfileKind::Printer => library
                            .printer(&row.id)
                            .map(|p| Dialog::Printer(PrinterEditor::new(p))),
                        ProfileKind::Material => library
                            .material(&row.id)
                            .map(|m| Dialog::Resin(ResinEditor::new(m))),
                        _ => library
                            .setting(&row.id)
                            .map(|s| Dialog::Setting(SettingEditor::edit(s.clone()))),
                    };
                }
                // Only a printer is duplicated (with its print settings); the other rows keep
                // the place empty so their buttons stay in columns.
                if kind == ProfileKind::Printer {
                    let hint = tr!("library-action-duplicate");
                    if outlined_icon_button(ui, icons, Icon::Duplicate, &hint).clicked() {
                        duplicate(library, row, out);
                    }
                } else {
                    ui.add_space(ui.spacing().interact_size.y);
                }
                if outlined_icon_button(ui, icons, Icon::Export, &tr!("library-action-export"))
                    .clicked()
                {
                    export(library, kind, &row.id, out);
                }
                if outlined_icon_button(ui, icons, Icon::Delete, &tr!("library-action-delete"))
                    .clicked()
                {
                    self.deleting = Some(deletion(library, kind, row));
                }
            });
        });
    }
}

/// Duplicate the printer of `row`, with its print settings, under a free name.
fn duplicate(library: &mut Library, row: &Row, out: &mut Outcome) {
    let name = library.unique_name(ProfileKind::Printer, &row.name);
    match library.duplicate_printer(&row.id, &name) {
        Ok(_) => {
            out.messages.push(Message::new(
                Level::Success,
                tr!("library-duplicated", name = &name),
            ));
            out.changed = true;
        }
        Err(error) => out.fail(&error),
    }
}

/// The columns' names over the rows.
fn header(ui: &mut egui::Ui, widths: &[f32; 4]) {
    egui::Frame::NONE
        .inner_margin(egui::Margin::symmetric(ROW_MARGIN[0], 2))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = GAP;
                for (width, name) in widths.iter().zip([
                    tr!("library-column-name"),
                    tr!("library-column-based-on"),
                    tr!("library-column-state"),
                    String::new(),
                ]) {
                    cell(ui, *width, |ui| {
                        ui.label(
                            egui::RichText::new(name)
                                .small()
                                .color(ui.visuals().weak_text_color()),
                        );
                    });
                }
            });
        });
    ui.separator();
}

/// The deletion of `row` to confirm: what goes with it.
fn deletion(library: &Library, kind: ProfileKind, row: &Row) -> Deletion {
    let with = match kind {
        ProfileKind::Printer => library
            .settings()
            .iter()
            .filter(|s| s.printer_id == row.id)
            .count(),
        ProfileKind::Material => library
            .settings()
            .iter()
            .filter(|s| s.resin_id == row.id)
            .count(),
        _ => 0,
    };
    let text = match kind {
        ProfileKind::PrintSetting => tr!("library-delete-setting"),
        _ => tr!("library-delete-with", count = with),
    };
    Deletion {
        kind,
        id: row.id.clone(),
        name: row.name.clone(),
        text,
    }
}

/// Save a profile of the library to a file the user chooses.
fn export(library: &Library, kind: ProfileKind, id: &str, out: &mut Outcome) {
    let bytes = match library.export(kind, id) {
        Ok(bytes) => bytes,
        Err(error) => return out.fail(&error),
    };
    let Some(path) = rfd::FileDialog::new()
        .set_title(tr!("library-export-title"))
        .add_filter(tr!("dialog-filter-profiles"), &["json"])
        .set_file_name(format!("{id}.json"))
        .save_file()
    else {
        return;
    };
    match std::fs::write(&path, bytes) {
        Ok(()) => out.messages.push(Message::new(
            Level::Success,
            tr!("library-exported", path = path.display().to_string()),
        )),
        Err(error) => out.messages.push(
            Message::new(Level::Error, tr!("library-export-failed")).detail(error.to_string()),
        ),
    }
}

/// Import profile files the user chooses into the library.
fn import(library: &mut Library, out: &mut Outcome) {
    let files = rfd::FileDialog::new()
        .set_title(tr!("library-import-title"))
        .add_filter(tr!("dialog-filter-profiles"), &["json"])
        .pick_files()
        .unwrap_or_default();
    for path in files {
        let name = path.display().to_string();
        let result = std::fs::read(&path)
            .map_err(|e| e.to_string())
            .and_then(|bytes| library.import(&bytes).map_err(|e| e.to_string()));
        match result {
            Ok(report) => {
                let mut message = Message::new(
                    Level::Success,
                    tr!("library-imported", names = report.added.join(", ")),
                );
                if !report.skipped.is_empty() {
                    message.level = Level::Warning;
                    message = message.detail(report.skipped.join("\n"));
                }
                out.messages.push(message);
                out.changed = true;
            }
            Err(error) => out.messages.push(
                Message::new(Level::Error, tr!("library-import-failed", name = name)).detail(error),
            ),
        }
    }
}

#[cfg(test)]
impl LibraryWindow {
    /// The dialog showing tab `tab`: printers, resins, print settings.
    pub(super) fn on_tab(tab: usize) -> Self {
        Self {
            tab,
            deleting: None,
        }
    }
}
