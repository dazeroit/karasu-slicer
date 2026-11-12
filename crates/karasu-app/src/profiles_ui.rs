//! The plate's printer, resin and print setting, and the user's library (TD-106).
//!
//! **Selection (sidebar):** the printer menu lists only the user's printers, then "Add printer…"
//! (the catalog, or a custom printer) and "Manage…" (the library). The resin menu lists the
//! user's resins with a print setting for the plate's printer; the others are offered under
//! "Not set up", through an explicit "start from". A print setting menu appears when the printer
//! and resin have more than one. The plate keeps copies of the three; when the library's copy
//! changes, the plate takes it.
//!
//! **Windows:** adding a printer or a resin from the catalog or as a custom one, setting a resin
//! up from another setting, editing a print setting, a printer or a resin, the library, and a
//! newer catalog revision's changes. Outcomes go to the notifications (TD-099).
//!
//! **Version one:** the first opening converts a library from before TD-106 and says so.
use crate::settings::ProfileChoice;
use crate::{
    i18n::tr,
    messages::{Level, Message},
    theme,
    widgets::fact_row,
    workspace::Workspace,
};
use eframe::egui;
use karasu_core::profiles::{
    store::{ProfileKind, ProfilePaths},
    swatch_rgb, PrintSettingProfile, ResinMaterial, SettingStatus,
};
use karasu_project::library::{Library, LibraryError};

/// The anti-aliasing levels a print setting or a plate can take, in samples per pixel side; 1
/// gives binary masks.
pub(crate) const ANTIALIAS_LEVELS: [u8; 5] = [1, 2, 4, 8, 16];

/// An anti-aliasing level as every view names it (TD-134): Off for binary masks (1 sample per
/// pixel side), otherwise its samples per pixel side.
pub(crate) fn antialias_label(samples: u8) -> String {
    if samples == 1 {
        tr!("field-antialias-off")
    } else {
        samples.to_string()
    }
}

mod catalog;
mod editors;
mod forms;
mod library;
mod menu;

/// An open profile window.
#[allow(clippy::large_enum_variant)] // One window at a time, held while it is open.
enum Dialog {
    AddPrinter(catalog::AddPrinter),
    AddResin(catalog::AddResin),
    SetUp(editors::SetUp),
    Setting(editors::SettingEditor),
    Printer(editors::PrinterEditor),
    Resin(editors::ResinEditor),
    Update(editors::UpdateReview),
    Library(library::LibraryWindow),
}

/// The library, the open window and the notifications to send.
pub struct ProfilesUi {
    library: Option<Library>,
    /// Why there is no library.
    error: Option<String>,
    /// The open dialogs, the topmost last: one opened from another stacks over it.
    stack: Vec<Dialog>,
    outbox: Vec<Message>,
}

impl Default for ProfilesUi {
    fn default() -> Self {
        Self::new()
    }
}

/// The name and colour of a print setting's status.
fn status(status: SettingStatus) -> (String, egui::Color32) {
    match status {
        SettingStatus::Official => (tr!("setting-official"), theme::SUCCESS.color()),
        SettingStatus::StartingPoint => (tr!("setting-starting-point"), theme::WARNING.color()),
        SettingStatus::User => (tr!("setting-user"), theme::ACCENT.color()),
    }
}

/// A library refusal as a notification.
fn refusal(error: &LibraryError) -> Message {
    let text = match error {
        LibraryError::NameTaken(name) => tr!("library-name-taken", name = name),
        _ => tr!("library-failed"),
    };
    Message::new(Level::Error, text).detail(error.to_string())
}

impl ProfilesUi {
    /// Open the catalog and the user library in the platform's data folder, converting a
    /// version-one library first.
    #[must_use]
    pub fn new() -> Self {
        let data = ProfilePaths::discover().ok().map(|paths| paths.data);
        Self::open(data.as_deref())
    }

    /// The library under `data` (or the catalog alone).
    #[must_use]
    pub fn open(data: Option<&std::path::Path>) -> Self {
        let mut outbox = Vec::new();
        match Library::open(data) {
            Ok(library) => {
                if let Some(report) = library.migration() {
                    let mut message = Message::new(
                        Level::Info,
                        tr!(
                            "library-converted",
                            printers = report.printers,
                            resins = report.resins,
                            settings = report.settings
                        ),
                    );
                    let mut detail = vec![tr!(
                        "library-converted-kept",
                        path = report.kept_in.display().to_string()
                    )];
                    detail.extend(report.failures.iter().cloned());
                    message = message.detail(detail.join("\n"));
                    if !report.failures.is_empty() {
                        message.level = Level::Warning;
                    }
                    outbox.push(message);
                }
                Self {
                    library: Some(library),
                    error: None,
                    stack: Vec::new(),
                    outbox,
                }
            }
            Err(error) => {
                log::error!(target: "profiles", "the profile library cannot be opened: {error}");
                Self {
                    library: None,
                    error: Some(error.to_string()),
                    stack: Vec::new(),
                    outbox,
                }
            }
        }
    }

    /// The notifications since the last call.
    pub fn take_messages(&mut self) -> Vec<Message> {
        std::mem::take(&mut self.outbox)
    }

    /// Open the editor of the plate's print setting (the Slice panel's link, and the action).
    pub fn edit_setting(&mut self, workspace: &Workspace) {
        if let Some(setting) = workspace.plate().setting() {
            self.open_alone(Dialog::Setting(editors::SettingEditor::edit(
                setting.clone(),
            )));
        }
    }

    /// Open the library window.
    pub fn open_library(&mut self) {
        self.open_alone(Dialog::Library(library::LibraryWindow::default()));
    }

    /// Profile files opened or dropped in the window: each is imported into the library.
    fn import_opened(&mut self, workspace: &mut Workspace) {
        let files = workspace.take_profile_files();
        let Some(library) = &mut self.library else {
            return;
        };
        for (name, bytes) in files {
            match library.import(&bytes) {
                Ok(report) => {
                    let mut message = Message::new(
                        Level::Success,
                        tr!("library-imported", names = report.added.join(", ")),
                    );
                    if !report.skipped.is_empty() {
                        message.level = Level::Warning;
                        message = message.detail(report.skipped.join("\n"));
                    }
                    self.outbox.push(message);
                }
                Err(error) => self.outbox.push(
                    Message::new(Level::Error, tr!("library-import-failed", name = name))
                        .detail(error.to_string()),
                ),
            }
        }
    }

    /// The plate takes the library's current copies of its printer, resin and print setting.
    fn sync_plate(&self, workspace: &mut Workspace) {
        let Some(library) = &self.library else {
            return;
        };
        let plate = workspace.plate();
        let printer = plate
            .printer
            .as_ref()
            .and_then(|p| library.printer(&p.id))
            .filter(|p| Some(*p) != plate.printer.as_ref())
            .cloned();
        let resin = plate
            .resin
            .as_ref()
            .and_then(|r| library.material(&r.id))
            .filter(|r| Some(*r) != plate.resin.as_ref())
            .cloned();
        let setting = plate
            .print_setting
            .as_ref()
            .and_then(|s| library.setting(&s.id))
            .filter(|s| Some(*s) != plate.print_setting.as_ref())
            .cloned();
        if printer.is_some() || resin.is_some() || setting.is_some() {
            workspace.edit(|plate| {
                if let Some(p) = printer {
                    plate.printer = Some(p);
                }
                if let Some(r) = resin {
                    plate.resin = Some(r);
                }
                if let Some(s) = setting {
                    plate.print_setting = Some(s);
                }
            });
        }
    }

    /// Choose `printer_id` for the plate, keeping the resin and setting when they fit it, else
    /// taking its first set-up resin and setting.
    fn choose_printer(&self, workspace: &mut Workspace, printer_id: &str) {
        let Some(library) = &self.library else {
            return;
        };
        let Some(printer) = library.printer(printer_id).cloned() else {
            return;
        };
        let plate = workspace.plate();
        let resin = plate
            .resin
            .as_ref()
            .filter(|r| !library.settings_for(printer_id, &r.id).is_empty())
            .cloned()
            .or_else(|| {
                library
                    .resins_for(printer_id)
                    .0
                    .first()
                    .map(|r| (*r).clone())
            });
        let setting = resin.as_ref().and_then(|r| {
            let settings = library.settings_for(printer_id, &r.id);
            plate
                .print_setting
                .as_ref()
                .and_then(|s| settings.iter().find(|x| x.id == s.id))
                .or_else(|| settings.first())
                .map(|s| (*s).clone())
        });
        workspace.edit(|plate| {
            plate.printer = Some(printer);
            plate.resin = resin;
            plate.print_setting = setting;
        });
    }

    /// Start the plate with the printer, resin and print setting of `choice` (the last session's),
    /// when the plate has no printer yet: the printer if it is still in the library, the resin
    /// if it is still set up for it (else the printer's first), the setting if it is still one
    /// of theirs (else their first). Returns whether the plate changed.
    pub fn restore(&self, workspace: &mut Workspace, choice: &ProfileChoice) -> bool {
        let Some(library) = &self.library else {
            return false;
        };
        let Some(printer) = choice
            .printer
            .as_deref()
            .filter(|id| library.printer(id).is_some())
        else {
            return false;
        };
        if workspace.printer().is_some() {
            return false;
        }
        self.choose_printer(workspace, printer);
        if let Some(resin) = choice
            .resin
            .as_deref()
            .filter(|r| !library.settings_for(printer, r).is_empty())
        {
            self.choose_resin(workspace, resin);
            let setting = choice
                .print_setting
                .as_deref()
                .and_then(|id| {
                    library
                        .settings_for(printer, resin)
                        .into_iter()
                        .find(|s| s.id == id)
                })
                .cloned();
            if let Some(setting) = setting {
                workspace.edit(|plate| plate.print_setting = Some(setting));
            }
        }
        true
    }

    /// The printer, resin and print setting the plate uses now, by identity.
    #[must_use]
    pub fn choice(workspace: &Workspace) -> ProfileChoice {
        let plate = workspace.plate();
        ProfileChoice {
            printer: plate.printer.as_ref().map(|p| p.id.clone()),
            resin: plate.resin.as_ref().map(|r| r.id.clone()),
            print_setting: plate.print_setting.as_ref().map(|s| s.id.clone()),
        }
    }

    /// Choose the user's resin `resin_id` for the plate's printer, with its first setting.
    fn choose_resin(&self, workspace: &mut Workspace, resin_id: &str) {
        let Some(library) = &self.library else {
            return;
        };
        let Some(printer) = workspace.printer().map(|p| p.id.clone()) else {
            return;
        };
        let resin = library.material(resin_id).cloned();
        let setting = library
            .settings_for(&printer, resin_id)
            .first()
            .map(|s| (*s).clone());
        workspace.edit(|plate| {
            plate.resin = resin;
            plate.print_setting = setting;
        });
    }

    /// The printer, resin and print setting cards for the sidebar (TD-098).
    #[allow(clippy::too_many_lines)] // The three cards, in order.
    pub fn selectors(&mut self, ui: &mut egui::Ui, workspace: &mut Workspace) {
        self.import_opened(workspace);
        self.sync_plate(workspace);
        let Some(library) = &self.library else {
            ui.label(egui::RichText::new(tr!("library-unavailable")).color(theme::WARNING.color()))
                .on_hover_text(self.error.clone().unwrap_or_default());
            return;
        };
        let mut request = None;
        ui.label(egui::RichText::new(tr!("profiles-printer")).strong());
        if library.printers().is_empty() {
            ui.add(egui::Label::new(tr!("library-empty")).wrap());
            if ui.button(tr!("action-add-your-printer")).clicked() {
                request = Some(Request::AddPrinter);
            }
            self.handle(request, workspace);
            return;
        }
        let plate_printer = workspace.printer().cloned();
        let label = plate_printer
            .as_ref()
            .map_or_else(|| tr!("printer-choose"), |p| p.name.clone());
        let picked = std::cell::Cell::new(None);
        let combo = menu::show(
            ui,
            "printer",
            label,
            library.printers().len(),
            |ui, query| {
                let mut shown = 0;
                for printer in library.printers() {
                    let text = match &printer.model {
                        Some(model) if model != &printer.name => {
                            format!("{} · {model}", printer.name)
                        }
                        _ => printer.name.clone(),
                    };
                    if !query.matches(&text) {
                        continue;
                    }
                    shown += 1;
                    let selected = plate_printer.as_ref().is_some_and(|p| p.id == printer.id);
                    if ui.selectable_label(selected, text).clicked() {
                        picked.set(Some(Request::Printer(printer.id.clone())));
                        ui.close();
                    }
                }
                shown
            },
            |ui| {
                action(ui, &tr!("action-add-printer"), &picked, Request::AddPrinter);
                action(ui, &tr!("action-manage-library"), &picked, Request::Library);
            },
        );
        if let Some(chosen) = picked.into_inner() {
            request = Some(chosen);
        }
        crate::widgets::name(&combo, &tr!("profiles-printer"));
        let Some(printer) = plate_printer else {
            self.handle(request, workspace);
            return;
        };
        if library.update(ProfileKind::Printer, &printer.id).is_some()
            && ui
                .small_button(tr!("library-update-available"))
                .on_hover_text(tr!("library-update-hint"))
                .clicked()
        {
            request = Some(Request::Update(ProfileKind::Printer, printer.id.clone()));
        }
        let [width, depth, height] = printer.build_size_mm;
        let [columns, rows] = printer.resolution_px;
        fact_row(ui, &tr!("field-panel"), &format!("{columns} × {rows}"));
        fact_row(
            ui,
            &tr!("field-build-volume"),
            &format!("{} × {} × {height} mm", round1(width), round1(depth)),
        );
        let verified = library.orientation_verified(&printer);
        fact_row(
            ui,
            &tr!("field-orientation"),
            &if verified {
                tr!("orientation-verified")
            } else {
                tr!("orientation-not-verified")
            },
        )
        .on_hover_text(tr!("orientation-hint"));
        ui.add_space(8.0);
        ui.label(egui::RichText::new(tr!("profiles-resin")).strong());
        let (ready, other) = library.resins_for(&printer.id);
        let plate_resin = workspace
            .plate()
            .resin
            .as_ref()
            .filter(|r| ready.iter().any(|x| x.id == r.id))
            .cloned();
        let label = plate_resin
            .as_ref()
            .map_or_else(|| tr!("resin-choose"), |r| r.name.clone());
        ui.horizontal(|ui| {
            swatch(ui, plate_resin.as_ref());
            let picked = std::cell::Cell::new(None);
            let combo = menu::show(
                ui,
                "resin",
                label,
                ready.len() + other.len(),
                |ui, query| {
                    if ready.is_empty() {
                        ui.label(tr!("resin-none-set-up", printer = &printer.name));
                    }
                    let mut shown = 0;
                    for resin in ready.iter().filter(|r| query.matches(&r.name)) {
                        shown += 1;
                        let selected = plate_resin.as_ref().is_some_and(|r| r.id == resin.id);
                        let clicked = ui
                            .horizontal(|ui| {
                                swatch(ui, Some(resin));
                                ui.selectable_label(selected, &resin.name).clicked()
                            })
                            .inner;
                        if clicked {
                            picked.set(Some(Request::Resin(resin.id.clone())));
                            ui.close();
                        }
                    }
                    let unset: Vec<_> = other.iter().filter(|r| query.matches(&r.name)).collect();
                    if !unset.is_empty() {
                        ui.separator();
                        ui.label(
                            egui::RichText::new(tr!("resin-not-set-up", printer = &printer.name))
                                .small()
                                .color(ui.visuals().weak_text_color()),
                        );
                        for resin in unset {
                            shown += 1;
                            if ui
                                .selectable_label(false, tr!("resin-set-up", name = &resin.name))
                                .clicked()
                            {
                                picked.set(Some(Request::SetUp(resin.id.clone())));
                                ui.close();
                            }
                        }
                    }
                    shown
                },
                |ui| {
                    action(ui, &tr!("action-add-resin"), &picked, Request::AddResin);
                    action(ui, &tr!("action-manage-library"), &picked, Request::Library);
                },
            );
            if let Some(chosen) = picked.into_inner() {
                request = Some(chosen);
            }
            crate::widgets::name(&combo, &tr!("profiles-resin"));
        });
        if let Some(resin) = &plate_resin {
            let settings = library.settings_for(&printer.id, &resin.id);
            let current = workspace.plate().setting().cloned();
            ui.add_space(8.0);
            ui.label(egui::RichText::new(tr!("profiles-print-setting")).strong());
            // The resin's print settings on this printer, each with its status, then new ones.
            let picked = std::cell::Cell::new(None);
            let combo = menu::show(
                ui,
                "print-setting",
                current
                    .as_ref()
                    .map_or_else(|| tr!("setting-choose").into(), setting_text),
                settings.len(),
                |ui, query| {
                    let mut shown = 0;
                    for setting in settings.iter().filter(|s| query.matches(&label_of(s))) {
                        shown += 1;
                        let selected = current.as_ref().is_some_and(|c| c.id == setting.id);
                        if ui
                            .selectable_label(selected, setting_text(setting))
                            .clicked()
                        {
                            picked.set(Some(Request::Setting(setting.id.clone())));
                            ui.close();
                        }
                    }
                    shown
                },
                |ui| {
                    let new = Request::NewSetting(resin.id.clone());
                    action(ui, &tr!("action-new-setting"), &picked, new);
                    action(ui, &tr!("action-manage-library"), &picked, Request::Library);
                },
            );
            if let Some(chosen) = picked.into_inner() {
                request = Some(chosen);
            }
            crate::widgets::name(&combo, &tr!("profiles-print-setting"));
            if let Some(setting) = &current {
                setting_facts(ui, setting);
                if ui.button(tr!("action-edit-print-setting")).clicked() {
                    request = Some(Request::EditSetting);
                }
            }
        } else if ready.is_empty() && ui.button(tr!("action-add-resin")).clicked() {
            request = Some(Request::AddResin);
        }
        self.handle(request, workspace);
    }

    fn handle(&mut self, request: Option<Request>, workspace: &mut Workspace) {
        let Some(request) = request else {
            return;
        };
        let printer = workspace.printer().map(|p| p.id.clone());
        match request {
            Request::Printer(id) => self.choose_printer(workspace, &id),
            Request::Resin(id) => self.choose_resin(workspace, &id),
            Request::Setting(id) => {
                if let Some(setting) = self.library.as_ref().and_then(|l| l.setting(&id)) {
                    let setting = setting.clone();
                    workspace.edit(|plate| plate.print_setting = Some(setting));
                }
            }
            Request::AddPrinter => {
                if let Some(library) = &self.library {
                    self.open_alone(Dialog::AddPrinter(catalog::AddPrinter::new(library)));
                }
            }
            Request::AddResin => {
                self.open_alone(Dialog::AddResin(catalog::AddResin::new(printer)));
            }
            Request::SetUp(resin) => {
                if let Some(printer) = printer {
                    self.open_alone(Dialog::SetUp(editors::SetUp::new(printer, resin)));
                }
            }
            Request::NewSetting(resin) => {
                if let Some(printer) = printer {
                    self.open_alone(Dialog::SetUp(editors::SetUp::choosing(printer, resin)));
                }
            }
            Request::EditSetting => self.edit_setting(workspace),
            Request::Library => self.open_library(),
            Request::Update(kind, id) => {
                self.open_alone(Dialog::Update(editors::UpdateReview::new(kind, id)));
            }
        }
    }

    /// The open profile window, if any.
    /// Open `dialog` alone: a dialog from the sidebar or a menu.
    fn open_alone(&mut self, dialog: Dialog) {
        self.stack = vec![dialog];
    }

    /// The open dialogs, the topmost last; only the topmost acts.
    pub fn windows(
        &mut self,
        ctx: &egui::Context,
        workspace: &mut Workspace,
        icons: Option<&crate::icons::Icons>,
    ) {
        let Some(library) = &mut self.library else {
            return;
        };
        let top = self.stack.len();
        let mut out = Outcome::default();
        for (k, dialog) in self.stack.iter_mut().enumerate() {
            let mut outcome = Outcome::default();
            dialog.show(ctx, library, icons, &mut outcome);
            if k + 1 == top {
                out = outcome;
            }
        }
        self.outbox.append(&mut out.messages);
        if out.close || out.replace.is_some() {
            self.stack.pop();
        }
        if let Some(next) = out.replace.take().or(out.push.take()) {
            self.stack.push(next);
        }
        if let Some(printer) = out.use_printer {
            self.choose_printer(workspace, &printer);
        }
        if let Some((resin, setting)) = out.use_resin {
            self.choose_resin(workspace, &resin);
            if let Some(setting) =
                setting.and_then(|id| self.library.as_ref().and_then(|l| l.setting(&id)).cloned())
            {
                workspace.edit(|plate| plate.print_setting = Some(setting));
            }
        }
        if out.changed {
            self.sync_plate(workspace);
        }
    }
}

impl Dialog {
    fn show(
        &mut self,
        ctx: &egui::Context,
        library: &mut Library,
        icons: Option<&crate::icons::Icons>,
        out: &mut Outcome,
    ) {
        match self {
            Self::AddPrinter(d) => d.show(ctx, library, out),
            Self::AddResin(d) => d.show(ctx, library, out),
            Self::SetUp(d) => d.show(ctx, library, out),
            Self::Setting(d) => d.show(ctx, library, out),
            Self::Printer(d) => d.show(ctx, library, out),
            Self::Resin(d) => d.show(ctx, library, out),
            Self::Update(d) => d.show(ctx, library, out),
            Self::Library(d) => d.show(ctx, library, icons, out),
        }
    }
}

/// What a sidebar control asked for.
enum Request {
    Printer(String),
    Resin(String),
    Setting(String),
    AddPrinter,
    AddResin,
    SetUp(String),
    /// A new print setting for the plate's printer and this resin.
    NewSetting(String),
    EditSetting,
    Library,
    Update(ProfileKind, String),
}

/// What a window did this frame.
#[derive(Default)]
struct Outcome {
    messages: Vec<Message>,
    /// Close the window.
    close: bool,
    /// Open this dialog over the current one, which it returns to.
    push: Option<Dialog>,
    /// Open this dialog in the current one's place.
    replace: Option<Dialog>,
    /// Put this printer on the plate.
    use_printer: Option<String>,
    /// Put this resin, and this setting of it, on the plate.
    use_resin: Option<(String, Option<String>)>,
    /// The library changed.
    changed: bool,
}

impl Outcome {
    fn fail(&mut self, error: &LibraryError) {
        self.messages.push(refusal(error));
    }
}

/// An action at the foot of a profile menu: choosing it asks for `request` and closes the menu.
fn action(
    ui: &mut egui::Ui,
    text: &str,
    picked: &std::cell::Cell<Option<Request>>,
    request: Request,
) {
    if ui.selectable_label(false, text).clicked() {
        picked.set(Some(request));
        ui.close();
    }
}

/// A print setting's name, with its film.
fn label_of(setting: &PrintSettingProfile) -> String {
    match &setting.film {
        Some(film) => format!("{} · {film}", setting.name),
        None => setting.name.clone(),
    }
}

fn round1(value: f64) -> String {
    let text = format!("{value:.1}");
    text.strip_suffix(".0").map_or(text.clone(), str::to_owned)
}

/// A print setting's name and, in its colour, its status: an entry of the print setting menu.
fn setting_text(setting: &PrintSettingProfile) -> egui::WidgetText {
    let (text, colour) = status(setting.status);
    let mut job = egui::text::LayoutJob::default();
    let style = egui::Style::default();
    egui::RichText::new(label_of(setting)).append_to(
        &mut job,
        &style,
        egui::FontSelection::Default,
        egui::Align::Center,
    );
    egui::RichText::new(format!("  {text}"))
        .small()
        .color(colour)
        .append_to(
            &mut job,
            &style,
            egui::FontSelection::Default,
            egui::Align::Center,
        );
    job.into()
}

/// The print setting's main values and where they come from, read only.
fn setting_facts(ui: &mut egui::Ui, setting: &PrintSettingProfile) {
    let s = &setting.settings;
    fact_row(
        ui,
        &tr!("field-layer-height"),
        &format!("{} mm", s.layer_height_mm),
    );
    fact_row(
        ui,
        &tr!("field-exposure"),
        &format!("{} s", s.normal.exposure_s),
    );
    fact_row(
        ui,
        &tr!("field-bottom-exposure"),
        &format!(
            "{} s · {}",
            s.bottom.exposure_s,
            tr!("sidebar-bottom-layers", count = s.bottom_layer_count)
        ),
    );
    fact_row(
        ui,
        &tr!("field-transition-layers"),
        &s.transition_layer_count.to_string(),
    );
    fact_row(
        ui,
        &tr!("field-antialias"),
        &antialias_label(setting.image.antialias),
    );
    if let Some(source) = &setting.source {
        fact_row(ui, &tr!("field-source"), source).on_hover_text(source);
    }
}

/// A resin's swatch (TD-118): a small square of its colour beside its name. Without one, the
/// same room, empty, so the names stay aligned.
pub(crate) fn swatch(ui: &mut egui::Ui, resin: Option<&ResinMaterial>) {
    let side = ui.spacing().interact_size.y * 0.6;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::hover());
    if let Some([r, g, b]) = resin.and_then(|r| r.swatch.as_deref()).and_then(swatch_rgb) {
        ui.painter().rect(
            rect,
            2.0,
            egui::Color32::from_rgb(r, g, b),
            egui::Stroke::new(1.0, ui.visuals().weak_text_color()),
            egui::StrokeKind::Inside,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{catalog, editors, library, Dialog, ProfilesUi};
    use crate::workspace::Workspace;
    use eframe::egui;
    use karasu_core::profiles::store::ProfileKind;
    use std::path::PathBuf;

    /// A fresh data folder under the system's temporary directory.
    fn folder(name: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("karasu-profiles-ui-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn the_plate_takes_a_printer_with_its_resin_and_setting_and_follows_the_library() {
        let data = folder("plate");
        let mut ui = ProfilesUi::open(Some(&data));
        let mut workspace = Workspace::new();
        let library = ui.library.as_mut().unwrap();
        let p1 = library
            .add_printer_from_catalog(
                "anycubic-photon-p1",
                "Photon P1",
                &["anycubic-standard-resin"],
            )
            .unwrap();
        let photon = library
            .add_printer_from_catalog("anycubic-photon", "Photon", &["anycubic-standard-resin"])
            .unwrap();
        ui.choose_printer(&mut workspace, &p1);
        let setting = workspace.plate().setting().unwrap().clone();
        assert_eq!(setting.printer_id, p1);
        // The library's edit reaches the plate.
        let mut edited = setting.clone();
        edited.settings.normal.exposure_s = 2.4;
        ui.library.as_mut().unwrap().save_setting(edited).unwrap();
        ui.sync_plate(&mut workspace);
        assert!((workspace.plate().calibration().unwrap().normal.exposure_s - 2.4).abs() < 1e-12);
        // A printer with no resin set up leaves the plate without resin or setting.
        ui.choose_printer(&mut workspace, &photon);
        assert!(workspace.plate().resin.is_none() && workspace.plate().setting().is_none());
        ui.choose_printer(&mut workspace, &p1);
        assert!(workspace.plate().setting().is_some());
        let _ = std::fs::remove_dir_all(&data);
    }

    #[test]
    fn the_last_sessions_profiles_come_back_while_they_fit() {
        use crate::settings::ProfileChoice;
        let data = folder("restore");
        let mut ui = ProfilesUi::open(Some(&data));
        let library = ui.library.as_mut().unwrap();
        let p1 = library
            .add_printer_from_catalog(
                "anycubic-photon-p1",
                "Photon P1",
                &["anycubic-standard-resin"],
            )
            .unwrap();
        let first = library.settings()[0].clone();
        let mut draft = first.clone();
        draft.name = "Fine".into();
        let fine = library.new_setting(&draft).unwrap();
        let resin = first.resin_id.clone();
        // The pair's first setting as the library lists them, which a fallback takes, and the
        // other one.
        let listed = library.settings_for(&p1, &resin)[0].id.clone();
        let other = if listed == fine {
            first.id.clone()
        } else {
            fine
        };
        let choice = |printer: &str, resin: &str, setting: &str| ProfileChoice {
            printer: Some(printer.to_owned()),
            resin: Some(resin.to_owned()),
            print_setting: Some(setting.to_owned()),
        };
        // All three, the setting other than the pair's first included.
        let mut workspace = Workspace::new();
        assert!(ui.restore(&mut workspace, &choice(&p1, &resin, &other)));
        assert_eq!(ProfilesUi::choice(&workspace), choice(&p1, &resin, &other));
        // Already chosen: the plate keeps what it has.
        assert!(!ui.restore(&mut workspace, &choice(&p1, &resin, &listed)));
        assert_eq!(workspace.plate().setting().unwrap().id, other);
        // A resin no longer set up: the printer's first resin and setting.
        let mut workspace = Workspace::new();
        assert!(ui.restore(&mut workspace, &choice(&p1, "gone-resin", &other)));
        assert_eq!(ProfilesUi::choice(&workspace), choice(&p1, &resin, &listed));
        // A printer no longer in the library: nothing.
        let mut workspace = Workspace::new();
        assert!(!ui.restore(&mut workspace, &choice("gone-printer", &resin, &other)));
        assert_eq!(ProfilesUi::choice(&workspace), ProfileChoice::default());
        let _ = std::fs::remove_dir_all(&data);
    }

    #[test]
    fn every_profile_window_draws_without_a_library_change() {
        let data = folder("windows");
        let mut ui = ProfilesUi::open(Some(&data));
        let mut workspace = Workspace::new();
        let ctx = egui::Context::default();
        let frame = |ui: &mut ProfilesUi, workspace: &mut Workspace| {
            let mut output = ctx.run_ui(egui::RawInput::default(), |inner| {
                ui.selectors(inner, workspace);
                ui.windows(inner.ctx(), workspace, None);
            });
            output.textures_delta.clear();
        };
        // The empty library's invitation.
        frame(&mut ui, &mut workspace);
        let library = ui.library.as_mut().unwrap();
        let p1 = library
            .add_printer_from_catalog(
                "anycubic-photon-p1",
                "Photon P1",
                &["anycubic-standard-resin"],
            )
            .unwrap();
        let resin = library.materials()[0].id.clone();
        let setting = library.settings()[0].clone();
        ui.choose_printer(&mut workspace, &p1);
        let printers = ui.library.as_ref().unwrap().printers().len();
        let dialogs = |ui: &ProfilesUi| {
            let library = ui.library.as_ref().unwrap();
            vec![
                Dialog::AddPrinter(catalog::AddPrinter::new(library)),
                Dialog::AddResin(catalog::AddResin::new(Some(p1.clone()))),
                Dialog::SetUp(editors::SetUp::new(p1.clone(), resin.clone())),
                Dialog::Setting(editors::SettingEditor::edit(setting.clone())),
                Dialog::Setting(editors::SettingEditor::new(&p1, &resin)),
                Dialog::Printer(editors::PrinterEditor::new(library.printer(&p1).unwrap())),
                Dialog::Resin(editors::ResinEditor::new(library.material(&resin).unwrap())),
                Dialog::Update(editors::UpdateReview::new(ProfileKind::Printer, p1.clone())),
                Dialog::Library(library::LibraryWindow::default()),
            ]
        };
        for dialog in dialogs(&ui) {
            ui.stack = vec![dialog];
            frame(&mut ui, &mut workspace);
        }
        assert_eq!(ui.library.as_ref().unwrap().printers().len(), printers);
        assert_eq!(ui.take_messages(), Vec::new());
        let _ = std::fs::remove_dir_all(&data);
    }

    #[test]
    fn a_catalog_printer_comes_with_its_vendors_standard_resin_and_the_resins_ticked() {
        let data = folder("add-printer-resins");
        let mut ui = ProfilesUi::open(Some(&data));
        let mut workspace = Workspace::new();
        let ctx = egui::Context::default();
        let library = ui.library.as_ref().unwrap();
        let offered = library.catalog().resins_for("elegoo-saturn-3").len();
        assert!(offered > 10, "{offered}");
        let open = catalog::AddPrinter::new(library).picking("elegoo-saturn-3");
        ui.stack = vec![Dialog::AddPrinter(open)];
        let mut output = ctx.run_ui(egui::RawInput::default(), |inner| {
            ui.windows(inner.ctx(), &mut workspace, None);
        });
        output.textures_delta.clear();
        let [Dialog::AddPrinter(dialog)] = ui.stack.as_mut_slice() else {
            panic!("the dialog closed");
        };
        // Only the vendor's standard resin is ticked at first.
        assert_eq!(
            dialog.chosen(),
            Some(&["elegoo-standard-resin".to_owned()][..])
        );
        dialog.tick("elegoo-tough-resin");
        let mut dialog = std::mem::replace(
            dialog,
            catalog::AddPrinter::new(ui.library.as_ref().unwrap()),
        );
        let library = ui.library.as_mut().unwrap();
        let mut out = super::Outcome::default();
        dialog.confirm(library, &mut out);
        assert!(out.close);
        let mut resins: Vec<_> = library
            .materials()
            .iter()
            .map(|m| m.based_on.as_ref().unwrap().id.clone())
            .collect();
        resins.sort();
        assert_eq!(resins, ["elegoo-standard-resin", "elegoo-tough-resin"]);
        assert_eq!(library.settings().len(), 2);
        // Every resin of a printer, then none: the printer alone.
        let saturn = library.catalog().resins_for("elegoo-saturn-3").len();
        dialog = dialog.picking("elegoo-saturn-3");
        dialog.tick_all(library, true);
        assert_eq!(dialog.chosen().map(<[String]>::len), Some(saturn));
        dialog.tick_all(library, false);
        assert_eq!(dialog.chosen(), Some(&[][..]));
        let mut out = super::Outcome::default();
        dialog.confirm(library, &mut out);
        assert!(out.close);
        assert_eq!((library.printers().len(), library.settings().len()), (2, 2));
        // Another printer with only a ticked resin, whose copy the library already holds.
        dialog = dialog.picking("elegoo-mars-4");
        dialog.tick("elegoo-tough-resin");
        let mut out = super::Outcome::default();
        dialog.confirm(library, &mut out);
        assert!(out.close);
        assert_eq!(library.printers().len(), 3);
        assert_eq!(library.materials().len(), 2);
        assert_eq!(library.settings().len(), 3);
        let _ = std::fs::remove_dir_all(&data);
    }

    #[test]
    fn every_profile_dialog_and_the_plate_selectors_name_their_controls() {
        let data = folder("a11y");
        let mut ui = ProfilesUi::open(Some(&data));
        let mut workspace = Workspace::new();
        let library = ui.library.as_mut().unwrap();
        let p1 = library
            .add_printer_from_catalog(
                "anycubic-photon-p1",
                "Photon P1",
                &["anycubic-standard-resin"],
            )
            .unwrap();
        let resin = library.materials()[0].id.clone();
        let setting = library.settings()[0].clone();
        ui.choose_printer(&mut workspace, &p1);
        let mut report = Vec::new();
        // The sidebar's printer, resin and print setting.
        report.extend(crate::accessibility_check::failures(|inner| {
            ui.selectors(inner, &mut workspace);
            crate::focus::paint(inner.ctx());
        }));
        let library = ui.library.as_ref().unwrap();
        let dialogs = vec![
            Dialog::AddPrinter(catalog::AddPrinter::new(library)),
            // A printer whose resins are offered, each with its box.
            Dialog::AddPrinter(catalog::AddPrinter::new(library).picking("elegoo-saturn-3")),
            Dialog::AddResin(catalog::AddResin::new(Some(p1.clone()))),
            Dialog::SetUp(editors::SetUp::new(p1.clone(), resin.clone())),
            Dialog::Setting(editors::SettingEditor::edit(setting.clone())),
            Dialog::Printer(editors::PrinterEditor::new(library.printer(&p1).unwrap())),
            Dialog::Resin(editors::ResinEditor::new(library.material(&resin).unwrap())),
            Dialog::Update(editors::UpdateReview::new(ProfileKind::Printer, p1.clone())),
            Dialog::Library(library::LibraryWindow::default()),
        ];
        for dialog in dialogs {
            let kind = format!("{:?}", std::mem::discriminant(&dialog));
            ui.stack = vec![dialog];
            report.extend(
                crate::accessibility_check::failures(|inner| {
                    ui.windows(inner.ctx(), &mut workspace, None);
                    crate::focus::paint(inner.ctx());
                })
                .into_iter()
                .map(|f| format!("{kind}: {f}")),
            );
        }
        let _ = std::fs::remove_dir_all(&data);
        assert!(report.is_empty(), "{}", report.join("\n"));
    }

    /// What `shape` draws, without shadows and backdrops (outside a window's frame by design).
    fn bounds(shape: &egui::Shape, clip: egui::Rect, into: &mut egui::Rect) {
        match shape {
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    bounds(shape, clip, into);
                }
            }
            // Shadows, and a modal dialog's backdrop over the whole screen, by design.
            egui::Shape::Rect(r) if r.blur_width > 0.0 || r.rect.width() >= 1600.0 => {}
            shape => {
                let r = shape.visual_bounding_rect().intersect(clip);
                if r.is_positive() && r.is_finite() {
                    *into = into.union(r);
                }
            }
        }
    }

    /// Everything a frame drew that can be seen, as one rectangle.
    fn drawn(output: &egui::FullOutput) -> egui::Rect {
        let mut rect = egui::Rect::NOTHING;
        for c in &output.shapes {
            bounds(&c.shape, c.clip_rect, &mut rect);
        }
        rect
    }

    #[test]
    #[allow(clippy::too_many_lines)] // Every window, then its checks.
    fn every_profile_window_keeps_its_content_inside_it_and_on_the_screen() {
        let data = folder("layout");
        let mut ui = ProfilesUi::open(Some(&data));
        let mut workspace = Workspace::new();
        let library = ui.library.as_mut().unwrap();
        let p1 = library
            .add_printer_from_catalog(
                "anycubic-photon-p1",
                "Photon P1",
                &["anycubic-standard-resin"],
            )
            .unwrap();
        library
            .add_printer_from_catalog(
                "anycubic-photon-p1",
                "Photon P1 on the second shelf",
                &["anycubic-standard-resin"],
            )
            .unwrap();
        let resin = library.materials()[0].id.clone();
        let setting = library.settings()[0].clone();
        ui.choose_printer(&mut workspace, &p1);
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0));
        let windows: Vec<(&str, f32, Dialog)> = {
            let library = ui.library.as_ref().unwrap();
            vec![
                (
                    "add-printer",
                    900.0,
                    Dialog::AddPrinter(catalog::AddPrinter::new(library)),
                ),
                (
                    "add-printer",
                    900.0,
                    Dialog::AddPrinter(catalog::AddPrinter::new(library).custom()),
                ),
                (
                    "add-resin",
                    900.0,
                    Dialog::AddResin(catalog::AddResin::new(Some(p1.clone()))),
                ),
                (
                    "add-resin",
                    900.0,
                    Dialog::AddResin(catalog::AddResin::new(Some(p1.clone())).custom()),
                ),
                (
                    "setup",
                    480.0,
                    Dialog::SetUp(editors::SetUp::new(p1.clone(), resin.clone())),
                ),
                (
                    "setup",
                    480.0,
                    Dialog::SetUp(editors::SetUp::choosing(p1.clone(), resin.clone())),
                ),
                (
                    "setting-editor",
                    720.0,
                    Dialog::Setting(editors::SettingEditor::edit(setting.clone())),
                ),
                (
                    "setting-editor",
                    720.0,
                    Dialog::Setting(editors::SettingEditor::edit(setting.clone()).on_tab(1)),
                ),
                (
                    "setting-editor",
                    720.0,
                    Dialog::Setting(editors::SettingEditor::edit(setting.clone()).on_tab(2)),
                ),
                (
                    "printer-editor",
                    800.0,
                    Dialog::Printer(editors::PrinterEditor::new(library.printer(&p1).unwrap())),
                ),
                (
                    "resin-editor",
                    440.0,
                    Dialog::Resin(editors::ResinEditor::new(library.material(&resin).unwrap())),
                ),
                (
                    "library-window",
                    900.0,
                    Dialog::Library(library::LibraryWindow::default()),
                ),
            ]
        };
        for (id, width, dialog) in windows {
            let ctx = egui::Context::default();
            ui.stack = vec![dialog];
            let mut last = None;
            let mut settled = None;
            // A window takes its size over the first frames, then keeps it however long the
            // pointer moves over it.
            for frame in 0..16_u8 {
                let input = egui::RawInput {
                    screen_rect: Some(screen),
                    events: vec![egui::Event::PointerMoved(egui::pos2(
                        800.0,
                        300.0 + f32::from(frame),
                    ))],
                    ..egui::RawInput::default()
                };
                let mut output =
                    ctx.run_ui(input, |inner| ui.windows(inner.ctx(), &mut workspace, None));
                output.textures_delta.clear();
                last = Some(output);
                if frame == 3 {
                    settled = ctx.memory(|m| m.area_rect(egui::Id::new(id)));
                }
            }
            let window = ctx.memory(|m| m.area_rect(egui::Id::new(id))).unwrap();
            assert_eq!(
                Some(window),
                settled,
                "{id}: the window changed size by itself"
            );
            let drawn = drawn(last.as_ref().unwrap());
            assert!(
                screen.contains_rect(window),
                "{id}: {window:?} leaves the screen"
            );
            // The frame's margins come on top of the width the window is given.
            assert!(
                window.width() <= width + 24.0,
                "{id}: {} wide, more than its {width}",
                window.width()
            );
            assert!(
                window.expand(2.0).contains_rect(drawn),
                "{id}: content {drawn:?} overflows the window {window:?}"
            );
        }
        let _ = std::fs::remove_dir_all(&data);
    }

    /// One frame of the profile dialogs on a 1600 × 1000 screen, with `events`.
    fn dialog_frame(
        ctx: &egui::Context,
        ui: &mut ProfilesUi,
        workspace: &mut Workspace,
        events: Vec<egui::Event>,
    ) {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1600.0, 1000.0),
            )),
            events,
            ..egui::RawInput::default()
        };
        let mut output = ctx.run_ui(input, |inner| ui.windows(inner.ctx(), workspace, None));
        output.textures_delta.clear();
    }

    fn escape() -> egui::Event {
        egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }
    }

    #[test]
    fn escape_closes_the_topmost_dialog_and_returns_to_the_one_below() {
        let data = folder("stack");
        let mut ui = ProfilesUi::open(Some(&data));
        let mut workspace = Workspace::new();
        let library = ui.library.as_mut().unwrap();
        let p1 = library
            .add_printer_from_catalog(
                "anycubic-photon-p1",
                "Photon P1",
                &["anycubic-standard-resin"],
            )
            .unwrap();
        let printer = library.printer(&p1).unwrap().clone();
        let setting = library.settings()[0].clone();
        ui.stack = vec![
            Dialog::Library(library::LibraryWindow::default()),
            Dialog::Printer(editors::PrinterEditor::new(&printer)),
        ];
        let ctx = egui::Context::default();
        dialog_frame(&ctx, &mut ui, &mut workspace, Vec::new());
        dialog_frame(&ctx, &mut ui, &mut workspace, vec![escape()]);
        assert!(matches!(ui.stack.as_slice(), [Dialog::Library(_)]));
        // An editor with unsaved changes asks first: Escape opens the question, a second
        // Escape keeps editing.
        let mut editor = editors::SettingEditor::edit(setting);
        editor.two_stages(false);
        ui.stack.push(Dialog::Setting(editor));
        dialog_frame(&ctx, &mut ui, &mut workspace, Vec::new());
        dialog_frame(&ctx, &mut ui, &mut workspace, vec![escape()]);
        dialog_frame(&ctx, &mut ui, &mut workspace, Vec::new());
        dialog_frame(&ctx, &mut ui, &mut workspace, vec![escape()]);
        dialog_frame(&ctx, &mut ui, &mut workspace, Vec::new());
        assert_eq!(ui.stack.len(), 2);
        let _ = std::fs::remove_dir_all(&data);
    }

    #[test]
    fn every_library_row_can_be_edited_exported_and_deleted() {
        let data = folder("rows");
        let mut ui = ProfilesUi::open(Some(&data));
        let mut workspace = Workspace::new();
        let library = ui.library.as_mut().unwrap();
        for (id, name) in [
            ("anycubic-photon-mono-4k", "Photon Mono 4K"),
            ("anycubic-photon-p1", "Photon P1"),
            ("elegoo-saturn-4-ultra-16k", "Saturn 4 Ultra 16K"),
        ] {
            library
                .add_printer_from_catalog(
                    id,
                    name,
                    &["anycubic-standard-resin", "elegoo-standard-resin"],
                )
                .unwrap();
        }
        let counts = [
            library.printers().len(),
            library.materials().len(),
            library.settings().len(),
        ];
        assert!(counts.iter().all(|n| *n >= 2), "{counts:?}");
        for (tab, rows) in counts.into_iter().enumerate() {
            let ctx = egui::Context::default();
            ctx.enable_accesskit();
            ui.stack = vec![Dialog::Library(library::LibraryWindow::on_tab(tab))];
            let mut labels = Vec::new();
            for _ in 0..3 {
                let input = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1600.0, 1000.0),
                    )),
                    ..egui::RawInput::default()
                };
                let mut output =
                    ctx.run_ui(input, |inner| ui.windows(inner.ctx(), &mut workspace, None));
                output.textures_delta.clear();
                labels = output
                    .platform_output
                    .accesskit_update
                    .map_or_default(|update| {
                        update
                            .nodes
                            .iter()
                            .filter_map(|(_, node)| node.label().map(str::to_owned))
                            .collect()
                    });
            }
            let count = |label: String| labels.iter().filter(|l| **l == label).count();
            assert_eq!(
                count(crate::i18n::tr!("library-action-edit")),
                rows,
                "tab {tab}"
            );
            assert_eq!(
                count(crate::i18n::tr!("library-action-export")),
                rows,
                "tab {tab}"
            );
            assert_eq!(
                count(crate::i18n::tr!("library-action-delete")),
                rows,
                "tab {tab}"
            );
            let duplicates = if tab == 0 { rows } else { 0 };
            assert_eq!(
                count(crate::i18n::tr!("library-action-duplicate")),
                duplicates,
                "tab {tab}"
            );
        }
        let _ = std::fs::remove_dir_all(&data);
    }

    #[test]
    fn a_new_print_setting_from_the_library_starts_from_one_and_opens_its_editor() {
        let data = folder("new-setting");
        let mut ui = ProfilesUi::open(Some(&data));
        let mut workspace = Workspace::new();
        let library = ui.library.as_mut().unwrap();
        let p1 = library
            .add_printer_from_catalog(
                "anycubic-photon-p1",
                "Photon P1",
                &["anycubic-standard-resin"],
            )
            .unwrap();
        let resin = library.materials()[0].id.clone();
        let before = library.settings().len();
        ui.stack = vec![
            Dialog::Library(library::LibraryWindow::on_tab(2)),
            Dialog::SetUp(editors::SetUp::choosing(p1, resin)),
        ];
        let ctx = egui::Context::default();
        let enter = egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        dialog_frame(&ctx, &mut ui, &mut workspace, Vec::new());
        dialog_frame(&ctx, &mut ui, &mut workspace, vec![enter]);
        assert_eq!(ui.library.as_ref().unwrap().settings().len(), before + 1);
        assert!(matches!(
            ui.stack.as_slice(),
            [Dialog::Library(_), Dialog::Setting(_)]
        ));
        let _ = std::fs::remove_dir_all(&data);
    }

    #[test]
    fn two_stages_come_back_as_they_were_and_never_grow() {
        let catalog = karasu_project::library::Catalog::built_in().unwrap();
        let setting = |printer: &str| {
            catalog
                .settings_for(printer, "anycubic-standard-resin")
                .next()
                .unwrap()
                .profile
                .clone()
        };
        let mut two = editors::SettingEditor::edit(setting("anycubic-photon-p1"));
        assert_eq!(two.normal_lift(), [(4.0, 3.0), (5.0, 8.0)]);
        for _ in 0..3 {
            two.two_stages(false);
            assert_eq!(two.normal_lift(), [(9.0, 3.0)]);
            two.two_stages(true);
            assert_eq!(two.normal_lift(), [(4.0, 3.0), (5.0, 8.0)]);
        }
        // A single stage turns into two halves of it, and back.
        let mut one = editors::SettingEditor::edit(setting("anycubic-photon-mono-x"));
        assert_eq!(one.normal_lift(), [(8.0, 2.0)]);
        one.two_stages(true);
        assert_eq!(one.normal_lift(), [(4.0, 2.0), (4.0, 2.0)]);
        one.two_stages(false);
        assert_eq!(one.normal_lift(), [(8.0, 2.0)]);
    }
}
