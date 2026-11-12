//! Slice every plate and Export every plate (TD-135, phase 4, from the owner-approved mockup
//! "Export every plate").
//!
//! **A batch** takes plates one after another, in project order: a plate whose last slice is
//! of the plate as it is goes on at once; another is sliced by its own slicing (the plate
//! shown's, or the one kept for it), off the interface thread, one slice at a time. Exporting,
//! each plate's file is then written whole or not at all, beside nothing else. A plate edited
//! while it slices, deleted, or whose slice fails is reported, never exported. Cancelling stops
//! the slice running and leaves the plates not reached.
//!
//! **Export every plate…** is a modal dialog: the folder, the file names' pattern
//! (`{project} {number} {plate}`, each plate's format giving the extension), a row per plate
//! with its check box, file, format and state, and the switch that slices first the plates not
//! sliced, or out of date (off, they are left out and listed in the notification). An existing
//! file is never replaced without asking. The folder, the pattern and the switch are kept
//! across restarts. While it works, its footer shows the plate being sliced and Cancel.
//!
//! **Slice every plate** slices the plates not sliced as they are, without a dialog: the status
//! bar and the overview's footer show its progress, the overview's footer can stop it, and a
//! notification ends it.
use super::Shown;
use crate::{
    dialog::{self, Part, Size},
    i18n::{decimal, tr},
    messages::{Level, Message},
    settings::DEFAULT_EXPORT_PATTERN,
    slicing_ui::{extension, format_label, format_of, switch_row, SlicingUi},
    widgets::text_field,
    window::MainWindow,
};
use eframe::egui;
use karasu_project::{project::PlateId, request::REFERENCE_FORMAT};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

/// The dialog's identity, under which its size is remembered.
const ID: &str = "export-every-plate";

/// Where a batch's plate stands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::window) enum Step {
    /// Its turn has not come.
    Waiting,
    /// It is being sliced.
    Slicing,
    /// Sliced as it is and, exporting, its file written, of this many bytes.
    Done(Option<u64>),
    /// Not sliced as it is, and the batch does not slice it.
    LeftOut,
    /// Its slice or its file failed, with why.
    Failed(String),
}

impl Step {
    /// Whether the plate is finished with, one way or another.
    fn finished(&self) -> bool {
        !matches!(self, Self::Waiting | Self::Slicing)
    }
}

/// A batch's plate.
#[derive(Clone, Debug)]
pub(in crate::window) struct Row {
    pub id: PlateId,
    pub name: String,
    /// Exporting, where its file goes.
    pub file: Option<PathBuf>,
    pub step: Step,
}

/// Plates sliced, and perhaps exported, one after another.
#[derive(Clone, Debug)]
pub(in crate::window) struct Batch {
    pub rows: Vec<Row>,
    /// Plates not sliced as they are are sliced first (else left out).
    pub slice_first: bool,
    /// Exporting, the folder the files go to (else slicing only).
    pub folder: Option<PathBuf>,
}

impl Batch {
    /// The row being sliced, its number among the rows and its share done.
    fn slicing(&self) -> Option<(usize, &Row)> {
        self.rows
            .iter()
            .enumerate()
            .find(|(_, r)| r.step == Step::Slicing)
            .map(|(k, r)| (k + 1, r))
    }
}

/// The Export every plate dialog's own state while it is open.
#[derive(Clone, Debug, Default)]
pub(in crate::window) struct ExportForm {
    /// Plates whose check box is cleared.
    unchecked: HashSet<PlateId>,
    /// The files that exist already, waiting for the user's answer to replace them.
    replacing: Option<Vec<PathBuf>>,
}

/// A plate's row in the dialog: what the batch would do with it.
pub(in crate::window) struct Planned {
    id: PlateId,
    number: usize,
    name: String,
    pub file: String,
    format: String,
    shown: Shown,
}

/// `pattern` with `{project}`, `{number}` and `{plate}` replaced, made a file name every system
/// accepts: no separator or reserved character, no control character, no dot or space at the
/// end, never empty.
pub(in crate::window) fn file_name(
    pattern: &str,
    project: &str,
    number: usize,
    plate: &str,
) -> String {
    let named = pattern
        .replace("{project}", project)
        .replace("{number}", &number.to_string())
        .replace("{plate}", plate);
    let clean: String = named
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') {
                '_'
            } else {
                c
            }
        })
        .collect();
    let clean = clean.trim().trim_end_matches(['.', ' ']).to_owned();
    if clean.is_empty() {
        format!("{number}")
    } else {
        clean
    }
}

impl MainWindow {
    /// The project's name for the files: its file's, else its own, else "Untitled".
    pub(in crate::window) fn project_label(&self) -> String {
        self.project_file
            .path
            .as_ref()
            .and_then(|p| p.file_stem())
            .map(|s| s.to_string_lossy().into_owned())
            .or_else(|| {
                let name = self.workspace.project().name.trim();
                (!name.is_empty()).then(|| name.to_owned())
            })
            .unwrap_or_else(|| tr!("project-untitled"))
    }

    /// Every plate's row, as the dialog's choices name its file, the same name never twice.
    pub(in crate::window) fn planned(&self) -> Vec<Planned> {
        let project = self.project_label();
        let pattern = &self.plates.choices.export_pattern;
        let mut taken = HashSet::new();
        self.workspace
            .plates()
            .iter()
            .enumerate()
            .map(|(k, plate)| {
                let content = plate.content();
                let format = &if content.settings.format == REFERENCE_FORMAT {
                    content.settings.format.clone()
                } else {
                    format_of(content).unwrap_or_else(|| content.settings.format.clone())
                };
                let base = file_name(pattern, &project, k + 1, &content.name);
                let with = |stem: &str| match extension(format) {
                    Some(ext) => format!("{stem}.{ext}"),
                    None => stem.to_owned(),
                };
                let mut file = with(&base);
                let mut copy = 2;
                while !taken.insert(file.to_lowercase()) {
                    file = with(&format!("{base} ({copy})"));
                    copy += 1;
                }
                Planned {
                    id: plate.id(),
                    number: k + 1,
                    name: content.name.clone(),
                    file,
                    format: format_label(format),
                    shown: self.plate_slice(plate.id()),
                }
            })
            .collect()
    }

    /// Open Export every plate…, every plate with a model checked.
    pub(super) fn open_export_every_plate(&mut self) {
        if self.plates.batch.is_none() {
            let unchecked = self
                .workspace
                .plates()
                .iter()
                .filter(|p| p.content().objects.is_empty())
                .map(karasu_project::project::Plate::id)
                .collect();
            self.plates.exporting = Some(ExportForm {
                unchecked,
                replacing: None,
            });
        }
    }

    /// Slice every plate not sliced as it is, one after another; say so when there is none.
    pub(super) fn slice_every_plate(&mut self) {
        if self.plates.batch.is_some() {
            return;
        }
        let rows: Vec<Row> = self
            .workspace
            .plates()
            .iter()
            .filter(|p| !p.content().objects.is_empty())
            .filter(|p| self.current_facts(p.id()).is_none())
            .map(|p| Row {
                id: p.id(),
                name: p.content().name.clone(),
                file: None,
                step: Step::Waiting,
            })
            .collect();
        if rows.is_empty() {
            self.messages
                .push(Message::new(Level::Info, tr!("batch-all-sliced")));
            return;
        }
        log::info!(target: "app", "slicing {} plates", rows.len());
        self.plates.batch = Some(Batch {
            rows,
            slice_first: true,
            folder: None,
        });
    }

    /// Stop the batch: the slice running is cancelled, the plates not reached are left.
    pub(in crate::window) fn cancel_batch(&mut self) {
        let Some(batch) = self.plates.batch.take() else {
            return;
        };
        if let Some((_, row)) = batch.slicing() {
            if let Some(slicing) = self.slicing_of(row.id) {
                slicing.cancel();
            }
        }
        self.plates.exporting = None;
        self.messages.push(summary(&batch, true));
    }

    /// The batch's line in the status bar while it runs: the plate being sliced, its number and
    /// its share done.
    pub(in crate::window) fn batch_status(&self) -> Option<String> {
        let batch = self.plates.batch.as_ref()?;
        let (number, row) = batch.slicing()?;
        let percent = self
            .slicing_of(row.id)
            .and_then(SlicingUi::progress)
            .map_or_else(String::new, |p| format!(" · {} %", decimal(p * 100.0, 0)));
        Some(tr!(
            "batch-status",
            number = number,
            count = batch.rows.len(),
            plate = &row.name,
            percent = percent
        ))
    }

    /// Take the batch a step further: start the next plate's slice, export a plate sliced,
    /// note a plate that failed; end it, with its notification, when every plate is done.
    /// Called every frame after the slicings are polled.
    pub(in crate::window) fn advance_batch(&mut self, ctx: &egui::Context) {
        let Some(mut batch) = self.plates.batch.take() else {
            return;
        };
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
        for k in 0..batch.rows.len() {
            if batch.rows[k].step.finished() {
                continue;
            }
            let id = batch.rows[k].id;
            if self.workspace.project().plate(id).is_none() {
                batch.rows[k].step = Step::Failed(tr!("batch-plate-deleted"));
                continue;
            }
            if self.current_facts(id).is_some() {
                // Sliced as it is: exported, or done.
                batch.rows[k].step = match &batch.rows[k].file {
                    Some(file) => self.write_plate(id, file),
                    None => Step::Done(None),
                };
                continue;
            }
            let slicing = self.slicing_of(id).is_some_and(SlicingUi::running);
            match batch.rows[k].step {
                Step::Waiting if slicing => {
                    // Already slicing (the Slice panel started it): its end is awaited.
                    batch.rows[k].step = Step::Slicing;
                    break;
                }
                Step::Waiting if !batch.slice_first => batch.rows[k].step = Step::LeftOut,
                Step::Waiting => {
                    let name = batch.rows[k].name.clone();
                    let slicing = if id == self.workspace.plate_id() {
                        &mut self.slicing
                    } else {
                        let sibling = || self.slicing.sibling();
                        self.plates.slicings.entry(id).or_insert_with(sibling)
                    };
                    slicing.start_plate(&self.workspace, id, &name);
                    batch.rows[k].step = if self.slicing_of(id).is_some_and(SlicingUi::running) {
                        Step::Slicing
                    } else {
                        Step::Failed(tr!("batch-not-started"))
                    };
                    break;
                }
                Step::Slicing if slicing => break,
                _ => {
                    // The slice ended, but not with the plate as it is.
                    let why = self
                        .slicing_of(id)
                        .and_then(SlicingUi::failure)
                        .map_or_else(|| tr!("batch-changed"), str::to_owned);
                    batch.rows[k].step = Step::Failed(why);
                }
            }
        }
        if batch.rows.iter().all(|r| r.step.finished()) {
            log::info!(target: "app", "batch of {} plates ended", batch.rows.len());
            self.plates.exporting = None;
            self.messages.push(summary(&batch, false));
        } else {
            self.plates.batch = Some(batch);
        }
    }

    /// Write plate `id`'s slice to `file`.
    fn write_plate(&self, id: PlateId, file: &Path) -> Step {
        match self.slicing_of(id).map(|s| s.write_to(file)) {
            Some(Ok(bytes)) => Step::Done(Some(bytes)),
            Some(Err(error)) => Step::Failed(error.to_string()),
            None => Step::Failed(tr!("batch-changed")),
        }
    }

    /// Start exporting the checked plates to the dialog's folder, unless a file there would be
    /// replaced: then the user is asked first.
    pub(in crate::window) fn start_export(&mut self, planned: &[Planned], replace: bool) {
        let Some(folder) = self.plates.choices.export_folder.clone() else {
            return;
        };
        let Some(form) = self.plates.exporting.as_mut() else {
            return;
        };
        let rows: Vec<Row> = planned
            .iter()
            .filter(|p| !form.unchecked.contains(&p.id))
            .map(|p| Row {
                id: p.id,
                name: p.name.clone(),
                file: Some(folder.join(&p.file)),
                step: Step::Waiting,
            })
            .collect();
        if !replace {
            let existing: Vec<PathBuf> = rows
                .iter()
                .filter_map(|r| r.file.clone())
                .filter(|f| f.exists())
                .collect();
            if !existing.is_empty() {
                form.replacing = Some(existing);
                return;
            }
        }
        if let Err(error) = std::fs::create_dir_all(&folder) {
            self.messages.push(
                Message::new(Level::Error, tr!("export-all-no-folder"))
                    .detail(format!("cannot create {}: {error}", folder.display())),
            );
            return;
        }
        log::info!(target: "app", "exporting {} plates to {}", rows.len(), folder.display());
        self.plates.batch = Some(Batch {
            rows,
            slice_first: self.plates.choices.export_slice_first,
            folder: Some(folder),
        });
    }

    /// Export every plate…, while it is open.
    pub(in crate::window) fn export_every_plate_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut form) = self.plates.exporting.take() else {
            return;
        };
        let planned = self.planned();
        let running = self.plates.batch.clone();
        let choices = &mut self.plates.choices;
        let mut folder = choices
            .export_folder
            .as_ref()
            .map_or_else(String::new, |f| f.display().to_string());
        let (mut pattern, mut slice_first) =
            (choices.export_pattern.clone(), choices.export_slice_first);
        let checked = planned
            .iter()
            .filter(|p| !form.unchecked.contains(&p.id))
            .count();
        let mut pick = false;
        let mut go = false;
        let mut cancel = false;
        let status = self.batch_status();
        let size = Size::Resizable {
            default: egui::vec2(720.0, 440.0),
            min: egui::vec2(560.0, 320.0),
        };
        let response = dialog::show(
            ctx,
            ID,
            &tr!("export-all-title"),
            size,
            |ui, part| match part {
                Part::Body => {
                    ui.add_enabled_ui(running.is_none(), |ui| {
                        form_fields(ui, &mut folder, &mut pattern, &mut pick);
                    });
                    ui.add_space(6.0);
                    plate_table(ui, &planned, &mut form.unchecked, running.as_ref());
                    ui.add_space(6.0);
                    ui.add_enabled_ui(running.is_none(), |ui| {
                        switch_row(
                            ui,
                            &mut slice_first,
                            tr!("export-all-slice-first"),
                            tr!("export-all-slice-first-hint"),
                        );
                    });
                    ui.add_space(4.0);
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(tr!("export-all-note"))
                                .small()
                                .color(ui.visuals().weak_text_color()),
                        )
                        .wrap(),
                    );
                }
                Part::Footer { enter } => {
                    let ready = running.is_none() && checked > 0 && !folder.trim().is_empty();
                    let label = tr!("export-all-action", count = checked);
                    if dialog::main_action(ui, &label, ready, enter) {
                        go = true;
                    }
                    if dialog::cancel_action(ui) {
                        cancel = true;
                    }
                    if let Some(status) = &status {
                        ui.add(egui::Label::new(status.as_str()).truncate());
                    }
                }
            },
        );
        keep_choices(
            &mut self.plates.choices,
            &folder,
            pattern,
            slice_first,
            pick,
        );
        if cancel || response.cancel {
            if running.is_some() {
                self.cancel_batch();
            }
            return;
        }
        let replacing = form.replacing.take();
        self.plates.exporting = Some(form);
        if go {
            self.start_export(&planned, false);
        } else if let Some(files) = replacing {
            self.ask_replace(ctx, &planned, files);
        }
    }

    /// Ask whether to replace `files`, which exist already, and export once the user agrees.
    fn ask_replace(&mut self, ctx: &egui::Context, planned: &[Planned], files: Vec<PathBuf>) {
        let names: Vec<String> = files
            .iter()
            .filter_map(|f| f.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .collect();
        match dialog::confirm(
            ctx,
            "export-all-replace",
            &tr!("export-all-replace-title", count = files.len()),
            &tr!("export-all-replace-text", files = names.join("\n")),
            &tr!("export-all-replace-action"),
            true,
        ) {
            Some(true) => self.start_export(planned, true),
            Some(false) => {}
            None => {
                if let Some(form) = self.plates.exporting.as_mut() {
                    form.replacing = Some(files);
                }
            }
        }
    }
}

/// Keep the dialog's `folder`, `pattern` (the default when cleared) and `slice_first` in
/// `choices`; with `pick`, ask for the folder first.
fn keep_choices(
    choices: &mut crate::settings::PlateSettings,
    folder: &str,
    pattern: String,
    slice_first: bool,
    pick: bool,
) {
    let folder = folder.trim();
    choices.export_folder = (!folder.is_empty()).then(|| PathBuf::from(folder));
    choices.export_pattern = if pattern.trim().is_empty() {
        DEFAULT_EXPORT_PATTERN.to_owned()
    } else {
        pattern
    };
    choices.export_slice_first = slice_first;
    if pick {
        let mut dialog = rfd::FileDialog::new().set_title(tr!("export-all-folder-title"));
        if let Some(folder) = &choices.export_folder {
            dialog = dialog.set_directory(folder);
        }
        if let Some(folder) = dialog.pick_folder() {
            choices.export_folder = Some(folder);
        }
    }
}

/// The folder with Choose…, and the file names' pattern.
fn form_fields(ui: &mut egui::Ui, folder: &mut String, pattern: &mut String, pick: &mut bool) {
    egui::Grid::new("export-all-fields")
        .num_columns(2)
        .spacing([10.0, 6.0])
        .show(ui, |ui| {
            ui.label(tr!("export-all-folder"));
            ui.horizontal(|ui| {
                let choose = tr!("export-all-choose");
                let room = ui.available_width() - 90.0;
                let field = ui.add(text_field(folder, room.max(120.0)));
                crate::widgets::name(&field, &tr!("export-all-folder"));
                *pick |= ui.button(choose).clicked();
            });
            ui.end_row();
            ui.label(tr!("export-all-names"));
            ui.horizontal(|ui| {
                let field = ui.add(text_field(pattern, 260.0));
                crate::widgets::name(&field, &tr!("export-all-names"));
                ui.label(
                    egui::RichText::new(tr!("export-all-names-hint"))
                        .color(ui.visuals().weak_text_color()),
                );
            });
            ui.end_row();
        });
}

/// The plates' table: each one's check box, number and name, file, format and state; while a
/// batch runs, the batch's state of its plates.
fn plate_table(
    ui: &mut egui::Ui,
    planned: &[Planned],
    unchecked: &mut HashSet<PlateId>,
    running: Option<&Batch>,
) {
    let weak = ui.visuals().weak_text_color();
    crate::focus::scrolled(
        egui::ScrollArea::vertical()
            .id_salt((ID, "plates"))
            .max_height(ui.available_height() - 90.0)
            .auto_shrink([false, true]),
        ui,
        |ui| {
            egui::Grid::new((ID, "table"))
                .num_columns(5)
                .striped(true)
                .spacing([12.0, 6.0])
                .show(ui, |ui| {
                    ui.label("");
                    for title in [
                        tr!("export-all-plate"),
                        tr!("export-all-file"),
                        tr!("export-all-format"),
                        tr!("export-all-state"),
                    ] {
                        ui.label(egui::RichText::new(title).color(weak));
                    }
                    ui.end_row();
                    for plate in planned {
                        let mut on = !unchecked.contains(&plate.id);
                        let check = ui
                            .add_enabled(running.is_none(), egui::Checkbox::without_text(&mut on));
                        crate::widgets::name(&check, &format!("{} · {}", plate.number, plate.name));
                        let changed = check.changed();
                        if changed {
                            if on {
                                unchecked.remove(&plate.id);
                            } else {
                                unchecked.insert(plate.id);
                            }
                        }
                        ui.label(format!("{} · {}", plate.number, plate.name));
                        ui.label(&plate.file);
                        ui.label(egui::RichText::new(&plate.format).color(weak));
                        let step = running
                            .and_then(|b| b.rows.iter().find(|r| r.id == plate.id))
                            .map(|r| &r.step);
                        let (text, colour) = state(step, plate.shown, on, weak);
                        ui.label(egui::RichText::new(text).small().color(colour));
                        ui.end_row();
                    }
                });
        },
    );
}

/// A row's state as the table says it: the batch's step while it runs, else where the plate's
/// slice stands.
fn state(
    step: Option<&Step>,
    shown: Shown,
    checked: bool,
    weak: egui::Color32,
) -> (String, egui::Color32) {
    use crate::theme;
    match step {
        Some(Step::Waiting) => (tr!("export-all-waiting"), weak),
        Some(Step::Slicing) => (tr!("export-all-slicing"), theme::ACCENT.color()),
        Some(Step::Done(Some(bytes))) => (
            tr!("export-all-exported", size = megabytes(*bytes)),
            theme::SUCCESS.color(),
        ),
        Some(Step::Done(None)) => (Shown::Sliced.label(), theme::SUCCESS.color()),
        Some(Step::LeftOut) => (tr!("export-all-left-out"), theme::WARNING.color()),
        Some(Step::Failed(_)) => (tr!("export-all-failed"), theme::ERROR.color()),
        None if !checked => (tr!("export-all-unchecked"), weak),
        None if shown == Shown::NotSliced => (shown.label(), weak),
        None => (shown.label(), shown.color()),
    }
}

/// `bytes` in megabytes, as the table shows a file's size.
fn megabytes(bytes: u64) -> String {
    #[allow(clippy::cast_precision_loss)] // A file's size, shown to one decimal.
    let mb = bytes as f64 / 1_000_000.0;
    decimal(mb, usize::from(mb < 10.0))
}

/// The notification at a batch's end: how many plates were sliced or exported (and where),
/// then those left out and those that failed, with why; `cancelled` when the user stopped it.
fn summary(batch: &Batch, cancelled: bool) -> Message {
    let done = batch
        .rows
        .iter()
        .filter(|r| matches!(r.step, Step::Done(_)))
        .count();
    let left: Vec<&str> = batch
        .rows
        .iter()
        .filter(|r| r.step == Step::LeftOut)
        .map(|r| r.name.as_str())
        .collect();
    let failed: Vec<String> = batch
        .rows
        .iter()
        .filter_map(|r| match &r.step {
            Step::Failed(why) => Some(format!("{}: {why}", r.name)),
            _ => None,
        })
        .collect();
    let not_reached = batch.rows.iter().filter(|r| !r.step.finished()).count();
    let text = match (&batch.folder, cancelled) {
        (Some(folder), false) => tr!(
            "batch-exported",
            count = done,
            folder = folder.display().to_string()
        ),
        (None, false) => tr!("batch-sliced", count = done),
        (Some(_), true) => tr!("batch-export-cancelled", count = done),
        (None, true) => tr!("batch-slice-cancelled", count = done),
    };
    let mut detail = Vec::new();
    if !left.is_empty() {
        detail.push(tr!("batch-left-out", plates = left.join(", ")));
    }
    if not_reached > 0 && cancelled {
        detail.push(tr!("batch-not-reached", count = not_reached));
    }
    detail.extend(failed.iter().cloned());
    let level = if !failed.is_empty() {
        Level::Error
    } else if !left.is_empty() || cancelled {
        Level::Warning
    } else {
        Level::Success
    };
    let message = Message::new(level, text);
    if detail.is_empty() {
        message
    } else {
        message.detail(detail.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::{file_name, Step};
    use crate::{actions::Action, i18n::tr, window::MainWindow};
    use eframe::egui;
    use std::time::{Duration, Instant};

    /// Run the window's batch to its end, as the frames do.
    fn run_batch(window: &mut MainWindow, ctx: &egui::Context) {
        let started = Instant::now();
        while window.plates.batch.is_some() {
            window.follow_plate(ctx);
            window.slicing.poll();
            window.advance_batch(ctx);
            assert!(started.elapsed() < Duration::from_secs(600), "no end");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn every_plate_is_sliced_then_exported_one_after_another() {
        // TD-135, phase 4: plates not shown are sliced by their own slicing, one at a time;
        // exporting writes each plate's file, asks before replacing one, and leaves out the
        // plates not sliced as they are when the switch is off.
        let ctx = egui::Context::default();
        let folder =
            std::env::temp_dir().join(format!("karasu-window-batch-{}", std::process::id()));
        let mut window = MainWindow::new(Vec::new());
        window.workspace = crate::slicing_ui::tests::hollowed_cube(&folder);
        window.slicing = crate::slicing_ui::tests::slicing_into(&folder);
        window.follow_plate(&ctx);
        window.perform(Action::DuplicatePlate);
        window.follow_plate(&ctx);
        let ids: Vec<_> = window
            .workspace
            .plates()
            .iter()
            .map(karasu_project::project::Plate::id)
            .collect();
        assert_eq!(ids.len(), 2);
        // Slice every plate: both, the one not shown too.
        assert!(window.enabled(Action::SliceEveryPlate));
        window.perform(Action::SliceEveryPlate);
        assert!(!window.enabled(Action::SliceEveryPlate));
        assert!(!window.enabled(Action::ExportEveryPlate));
        window.advance_batch(&ctx);
        let status = window.batch_status().expect("a plate slicing");
        assert!(status.starts_with(&tr!(
            "batch-status",
            number = 1usize,
            count = 2usize,
            plate = &window.workspace.plates()[0].content().name,
            percent = ""
        )));
        run_batch(&mut window, &ctx);
        for id in &ids {
            assert!(window.current_facts(*id).is_some(), "{id:?} sliced");
        }
        let latest = window.messages.latest().unwrap();
        assert_eq!(latest.text, tr!("batch-sliced", count = 2usize));
        // Nothing left to slice.
        window.perform(Action::SliceEveryPlate);
        assert!(window.plates.batch.is_none());
        assert_eq!(
            window.messages.latest().unwrap().text,
            tr!("batch-all-sliced")
        );
        // Export every plate: the plate shown, edited, is sliced again first.
        window
            .workspace
            .edit(|plate| plate.objects[0].translate_mm[0] += 1.0);
        let out = folder.join("out");
        window.plates.choices.export_folder = Some(out.clone());
        window.perform(Action::ExportEveryPlate);
        assert!(window.plates.exporting.is_some());
        let planned = window.planned();
        assert_eq!(planned.len(), 2);
        assert_ne!(planned[0].file, planned[1].file);
        window.start_export(&planned, false);
        run_batch(&mut window, &ctx);
        assert!(window.plates.exporting.is_none());
        for plate in &planned {
            let file = out.join(&plate.file);
            assert!(std::fs::metadata(&file).unwrap().len() > 0, "{file:?}");
        }
        assert_eq!(
            window.messages.latest().unwrap().text,
            tr!(
                "batch-exported",
                count = 2usize,
                folder = out.display().to_string()
            )
        );
        assert!(window.current_facts(window.workspace.plate_id()).is_some());
        // Again: the files are there, so the user is asked first.
        window.perform(Action::ExportEveryPlate);
        window.start_export(&planned, false);
        assert!(window.plates.batch.is_none());
        let asked = window.plates.exporting.as_ref().unwrap().replacing.clone();
        assert_eq!(asked.map(|files| files.len()), Some(2));
        // Replaced, the plate shown out of date and not sliced first: it is left out.
        window
            .workspace
            .edit(|plate| plate.objects[0].translate_mm[0] += 1.0);
        window.plates.choices.export_slice_first = false;
        window.start_export(&planned, true);
        let batch = window.plates.batch.clone().unwrap();
        run_batch(&mut window, &ctx);
        assert_eq!(batch.rows.len(), 2);
        let latest = window.messages.latest().unwrap();
        assert_eq!(
            latest.text,
            tr!(
                "batch-exported",
                count = 1usize,
                folder = out.display().to_string()
            )
        );
        let shown = window.workspace.plate().name.clone();
        assert_eq!(
            latest.detail,
            Some(tr!("batch-left-out", plates = shown.as_str()))
        );
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn the_dialog_lists_every_plate_with_its_file_and_state() {
        let ctx = egui::Context::default();
        let folder =
            std::env::temp_dir().join(format!("karasu-window-batch-draw-{}", std::process::id()));
        let mut window = MainWindow::new(Vec::new());
        window.workspace = crate::slicing_ui::tests::hollowed_cube(&folder);
        window.follow_plate(&ctx);
        window.perform(Action::NewPlate);
        window.follow_plate(&ctx);
        window.plates.choices.export_folder = Some(folder.clone());
        window.perform(Action::ExportEveryPlate);
        let planned = window.planned();
        let mut texts = Vec::new();
        for _ in 0..2 {
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                window.export_every_plate_dialog(&ui.ctx().clone());
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
        for expected in [
            tr!("export-all-title"),
            tr!("export-all-folder"),
            tr!("export-all-names"),
            tr!("export-all-file"),
            tr!("export-all-slice-first"),
            tr!("export-all-action", count = 1usize),
            tr!("export-all-unchecked"),
            planned[0].file.clone(),
            planned[1].file.clone(),
            tr!("plates-state-none"),
        ] {
            assert!(texts.contains(&expected), "{expected} in {texts:?}");
        }
        // The new plate, empty, starts unchecked.
        assert!(window.plates.exporting.is_some());
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_plate_the_slice_panel_never_settled_takes_its_printer_s_format() {
        // Owner's trial, 2026-11-07: Slice every plate wrote GOO for two Photon P1 plates,
        // whose format the Slice panel had never settled; the printer refused it.
        let ctx = egui::Context::default();
        let folder =
            std::env::temp_dir().join(format!("karasu-window-batch-format-{}", std::process::id()));
        let mut window = MainWindow::new(Vec::new());
        window.workspace = crate::slicing_ui::tests::hollowed_cube(&folder);
        window.slicing = crate::slicing_ui::tests::slicing_into(&folder);
        window.follow_plate(&ctx);
        let settled = window.workspace.plate().settings.format.clone();
        window
            .workspace
            .edit(|plate| plate.settings.format = "goo-v3".to_owned());
        assert_ne!(settled, "goo-v3");
        let ext = super::extension(&settled).unwrap();
        assert!(window.planned()[0].file.ends_with(&ext));
        window.perform(Action::SliceEveryPlate);
        run_batch(&mut window, &ctx);
        assert!(window.current_facts(window.workspace.plate_id()).is_some());
        assert_eq!(
            window.messages.latest().unwrap().text,
            tr!("batch-sliced", count = 1usize)
        );
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_batch_cancelled_stops_the_slice_running_and_says_so() {
        let ctx = egui::Context::default();
        let folder =
            std::env::temp_dir().join(format!("karasu-window-batch-stop-{}", std::process::id()));
        let mut window = MainWindow::new(Vec::new());
        window.workspace = crate::slicing_ui::tests::hollowed_cube(&folder);
        window.slicing = crate::slicing_ui::tests::slicing_into(&folder);
        window.follow_plate(&ctx);
        window.perform(Action::DuplicatePlate);
        window.follow_plate(&ctx);
        window.perform(Action::SliceEveryPlate);
        window.advance_batch(&ctx);
        let first = window.workspace.plates()[0].id();
        assert_eq!(
            window.plates.batch.as_ref().unwrap().rows[0].step,
            Step::Slicing
        );
        window.cancel_batch();
        assert!(window.plates.batch.is_none());
        assert!(window.enabled(Action::SliceEveryPlate));
        let latest = window.messages.latest().unwrap();
        assert_eq!(latest.text, tr!("batch-slice-cancelled", count = 0usize));
        assert_eq!(
            latest.detail,
            Some(tr!("batch-not-reached", count = 2usize))
        );
        // The slice stops, without a message of its own.
        let slicing = window.plates.slicings.get_mut(&first).unwrap();
        crate::slicing_ui::tests::finish(slicing);
        assert_eq!(slicing.take_messages().len(), 0);
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn file_names_follow_the_pattern_and_every_system_accepts_them() {
        let pattern = "{project} {number} {plate}";
        assert_eq!(
            file_name(pattern, "venom_figure", 2, "Torso"),
            "venom_figure 2 Torso"
        );
        assert_eq!(file_name(pattern, "a/b", 1, "c:d?"), "a_b 1 c_d_");
        assert_eq!(file_name("{plate}. ", "p", 3, "Arms"), "Arms");
        assert_eq!(file_name("{plate}", "p", 4, " "), "4");
        assert_eq!(file_name("{plate}", "p", 5, "tab\there"), "tab_here");
    }
}
