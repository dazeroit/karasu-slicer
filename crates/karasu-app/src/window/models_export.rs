//! Export models… (TD-139), from the owner-approved mockup "karasu model export mockup".
//!
//! **What.** The model of a row (its menu's Export model…), the plate shown, or every plate, as
//! binary STL, 3MF or OBJ: each model as it stands on the plate, in millimetres (turned, scaled,
//! hollowed, its drain holes cut), with its supports and raft when their switches are on.
//!
//! **Files.** One file per plate, or one per model. A single file (a model, or a plate as one
//! file) is named by Export…, which asks where, offering the project's and the plate's or
//! model's name. Several go into a folder, named by a pattern (`{project}`, `{number}`,
//! `{plate}`, `{model}`), listed in a table with their state. An existing file is never replaced
//! without asking; each file is written whole or not at all.
//!
//! **Running.** The export runs off the interface thread ([`ModelExportJob`]); the dialog shows
//! the step running and Cancel, which stops it (files already written stay). A notification ends
//! it: the files written, what cutting each model's holes left, and any file that failed.
//!
//! The choices (format, files, switches, folder, pattern) are kept across restarts.
use crate::{
    dialog::{self, Part, Size},
    i18n::{decimal, tr},
    messages::{Level, Message},
    settings::{ModelExportSettings, DEFAULT_MODEL_PATTERN},
    slicing_ui::switch_row,
    widgets::{choice, choice_among, text_field},
    window::MainWindow,
};
use eframe::egui;
use karasu_core::export::models::ModelFileFormat;
use karasu_project::{
    jobs::{ModelExportEvent, ModelExportJob, PlateExport, PlateFiles},
    pipeline::{model_file_name, ModelExport, ModelFiles, ModelObjects, NameFields},
    project::PlateId,
};
use std::{collections::HashSet, path::PathBuf};

/// The dialog's identity, under which its size is remembered.
const ID: &str = "export-models";

/// What the dialog exports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::window) enum Scope {
    /// One model of the plate shown.
    Model,
    /// The plate shown.
    Plate,
    /// Every plate with a model.
    Every,
}

/// The dialog's own state while it is open.
#[derive(Clone, Debug)]
pub(in crate::window) struct Form {
    pub scope: Scope,
    /// The model it opened on (its index in the plate shown), if any.
    pub model: Option<usize>,
    /// The files that exist already, waiting for the user's answer to replace them.
    replacing: Option<Vec<PathBuf>>,
}

/// One file of the export, as the table shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::window) struct Planned {
    /// Its plate.
    pub plate: PlateId,
    /// What it holds: the plate's name, or the model's with its number.
    pub label: String,
    /// Its object (0-based in its plate), for a file per model.
    pub object: Option<usize>,
    /// Where it goes.
    pub file: PathBuf,
}

/// A file's state while the export runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::window) enum State {
    Waiting,
    Working,
    Written(u64),
    Failed(String),
}

/// An export running.
pub(in crate::window) struct Running {
    job: ModelExportJob,
    /// The plates, in the job's order.
    plates: Vec<(PlateId, String)>,
    pub files: Vec<(Planned, State)>,
    /// The step running: its plate's name and the step.
    phase: Option<String>,
    /// What cutting each model's holes left, for the notification.
    drilled: Vec<String>,
    /// Whether some drilled model was left with crossings or flat triangles.
    left: bool,
    /// Plates that failed as a whole, with why.
    failed: Vec<String>,
}

/// Export models' state in the window.
#[derive(Default)]
pub(in crate::window) struct ModelsExport {
    /// The choices kept across restarts.
    pub choices: ModelExportSettings,
    pub form: Option<Form>,
    pub running: Option<Running>,
}

impl ModelsExport {
    /// Whether an export is running.
    pub(in crate::window) fn busy(&self) -> bool {
        self.running.is_some()
    }

    fn format(&self) -> ModelFileFormat {
        ModelFileFormat::from_extension(&self.choices.format).unwrap_or(ModelFileFormat::Stl)
    }

    fn files(&self) -> ModelFiles {
        if self.choices.per_model {
            ModelFiles::PerModel
        } else {
            ModelFiles::PerPlate
        }
    }
}

impl MainWindow {
    /// Open Export models… on the model at `model` of the plate shown, or on the plate.
    pub(in crate::window) fn open_export_models(&mut self, model: Option<usize>) {
        if self.models_export.busy() {
            return;
        }
        self.models_export.form = Some(Form {
            scope: if model.is_some() {
                Scope::Model
            } else {
                Scope::Plate
            },
            model,
            replacing: None,
        });
    }

    /// Whether Export models… can open: some plate has a model, and no export runs.
    pub(in crate::window) fn can_export_models(&self) -> bool {
        !self.models_export.busy()
            && self
                .workspace
                .plates()
                .iter()
                .any(|p| !p.content().objects.is_empty())
    }

    /// Whether `form` writes one file, named by the user, rather than several in a folder.
    fn single_file(&self, form: &Form) -> bool {
        match form.scope {
            Scope::Model => true,
            Scope::Plate => {
                !self.models_export.choices.per_model || self.workspace.plate().objects.len() <= 1
            }
            Scope::Every => false,
        }
    }

    /// The files `form` would write into `folder`, the same name never twice.
    pub(in crate::window) fn planned_models(
        &self,
        form: &Form,
        folder: &std::path::Path,
    ) -> Vec<Planned> {
        let project = self.project_label();
        let choices = &self.models_export.choices;
        let format = self.models_export.format();
        let shown = self.workspace.plate_id();
        let mut taken = HashSet::new();
        let mut out = Vec::new();
        for (k, plate) in self.workspace.plates().iter().enumerate() {
            let content = plate.content();
            let wanted = match form.scope {
                Scope::Every => !content.objects.is_empty(),
                Scope::Plate | Scope::Model => plate.id() == shown,
            };
            if !wanted {
                continue;
            }
            let objects: Vec<Option<usize>> = match (form.scope, choices.per_model) {
                (Scope::Model, _) => vec![form.model],
                (_, true) => (0..content.objects.len()).map(Some).collect(),
                (_, false) => vec![None],
            };
            for object in objects {
                let model = object.and_then(|i| content.objects.get(i));
                let fields = NameFields {
                    project: &project,
                    plate: &content.name,
                    number: object.map(|i| i + 1),
                    model: model.map(|o| o.name.as_str()),
                };
                let base = model_file_name(&choices.pattern, &fields);
                let mut name = format!("{base}.{}", format.extension());
                let mut copy = 2;
                while !taken.insert(name.to_lowercase()) {
                    name = format!("{base} ({copy}).{}", format.extension());
                    copy += 1;
                }
                let label = match model {
                    Some(o) => tr!(
                        "plates-overview-name",
                        number = object.unwrap_or(0) + 1,
                        name = &o.name
                    ),
                    None => tr!("plates-overview-name", number = k + 1, name = &content.name),
                };
                out.push(Planned {
                    plate: plate.id(),
                    label,
                    object,
                    file: folder.join(name),
                });
            }
        }
        out
    }

    /// The file name Export… offers for a single file: the project's name and the plate's, or
    /// the model's.
    fn offered_name(&self, form: &Form) -> String {
        let project = self.project_label();
        let content = self.workspace.plate();
        let model = form
            .model
            .filter(|_| form.scope == Scope::Model)
            .and_then(|i| content.objects.get(i));
        let fields = NameFields {
            project: &project,
            plate: &content.name,
            number: None,
            model: model.map(|o| o.name.as_str()),
        };
        let pattern = if model.is_some() {
            "{project} {model}"
        } else {
            "{project} {plate}"
        };
        format!(
            "{}.{}",
            model_file_name(pattern, &fields),
            self.models_export.format().extension()
        )
    }

    /// Start writing `planned`, unless a file there would be replaced and `replace` is off: then
    /// the user is asked first.
    #[allow(clippy::too_many_lines)] // The check, each plate's request and files, then the job.
    pub(in crate::window) fn start_model_export(&mut self, planned: Vec<Planned>, replace: bool) {
        if planned.is_empty() || self.models_export.busy() {
            return;
        }
        if !replace {
            let existing: Vec<PathBuf> = planned
                .iter()
                .map(|p| p.file.clone())
                .filter(|f| f.exists())
                .collect();
            if !existing.is_empty() {
                if let Some(form) = self.models_export.form.as_mut() {
                    form.replacing = Some(existing);
                }
                return;
            }
        }
        let scope = self.models_export.form.as_ref().map(|f| f.scope);
        // The plates, in order, each with its files.
        let mut plates: Vec<(PlateId, String)> = Vec::new();
        let mut exports = Vec::new();
        for file in &planned {
            if plates.iter().any(|(id, _)| *id == file.plate) {
                continue;
            }
            let Some(plate) = self.workspace.project().plate(file.plate) else {
                continue;
            };
            let name = plate.content().name.clone();
            let request = match self
                .workspace
                .project()
                .request(file.plate, PathBuf::from("unused"))
            {
                Ok((request, _)) => request,
                Err(error) => {
                    self.messages.push(
                        Message::new(Level::Error, tr!("export-models-failed"))
                            .detail(format!("{name}: {error}")),
                    );
                    return;
                }
            };
            let mine: Vec<&Planned> = planned.iter().filter(|p| p.plate == file.plate).collect();
            // A plate's one file, or a model's alone: one file, the export's `object` choosing.
            let files = if scope == Some(Scope::Model) || mine.iter().all(|p| p.object.is_none()) {
                PlateFiles::One(mine[0].file.clone())
            } else {
                PlateFiles::PerModel(
                    mine.iter()
                        .filter_map(|p| p.object.map(|o| (o, p.file.clone())))
                        .collect(),
                )
            };
            plates.push((file.plate, name.clone()));
            exports.push(PlateExport {
                request,
                name,
                files,
            });
        }
        let choices = &self.models_export.choices;
        let object = match scope {
            Some(Scope::Model) => planned[0].object,
            _ => None,
        };
        let export = ModelExport {
            format: self.models_export.format(),
            files: self.models_export.files(),
            supports: choices.supports,
            raft: choices.raft,
            object,
            replace,
            objects: if choices.separate_objects {
                ModelObjects::Separate
            } else {
                ModelObjects::OnePerModel
            },
        };
        if let Some(folder) = planned[0].file.parent() {
            if let Err(error) = std::fs::create_dir_all(folder) {
                self.messages.push(
                    Message::new(Level::Error, tr!("export-models-failed"))
                        .detail(format!("cannot create {}: {error}", folder.display())),
                );
                return;
            }
        }
        log::info!(
            target: "app",
            "exporting {} model file(s) as {}",
            planned.len(),
            export.format.extension()
        );
        let job = ModelExportJob::start(exports, export, Some(self.workspace.pipeline_cache()));
        self.models_export.running = Some(Running {
            job,
            plates,
            files: planned.into_iter().map(|p| (p, State::Waiting)).collect(),
            phase: None,
            drilled: Vec::new(),
            left: false,
            failed: Vec::new(),
        });
    }

    /// Take the running export's events: the step running, each plate's files, its end.
    pub(in crate::window) fn follow_model_export(&mut self, ctx: &egui::Context) {
        let Some(mut running) = self.models_export.running.take() else {
            return;
        };
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
        let mut finished = None;
        for event in running.job.poll() {
            match event {
                ModelExportEvent::Phase { plate, name } => {
                    let id = running.plates.get(plate).map(|p| p.0);
                    for (file, state) in &mut running.files {
                        if Some(file.plate) == id && *state == State::Waiting {
                            *state = State::Working;
                        }
                    }
                    let plate = running.plates.get(plate).map_or("", |p| p.1.as_str());
                    running.phase = Some(tr!("export-models-phase", plate = plate, step = name));
                }
                ModelExportEvent::Plate { plate, result } => {
                    let Some(&(id, ref name)) = running.plates.get(plate) else {
                        continue;
                    };
                    match result {
                        Ok(outcome) => {
                            for drilled in &outcome.drilled {
                                running.drilled.push(drilled_line(drilled));
                                running.left |= drilled.crossings + drilled.flat > 0;
                            }
                            for written in outcome.files {
                                if let Some((_, state)) = running
                                    .files
                                    .iter_mut()
                                    .find(|(p, _)| p.plate == id && p.file == written.path)
                                {
                                    *state = match written.result {
                                        Ok(summary) => State::Written(summary.bytes),
                                        Err(error) => State::Failed(error.message().to_owned()),
                                    };
                                }
                            }
                        }
                        Err(error) => {
                            let why = match error {
                                karasu_project::jobs::JobError::Failed(e) => e.message().to_owned(),
                                other => format!("{other:?}"),
                            };
                            running.failed.push(format!("{name}: {why}"));
                            for (file, state) in &mut running.files {
                                if file.plate == id && !matches!(state, State::Written(_)) {
                                    *state = State::Failed(why.clone());
                                }
                            }
                        }
                    }
                }
                ModelExportEvent::Finished { cancelled } => finished = Some(cancelled),
            }
        }
        match finished {
            Some(cancelled) => {
                log::info!(target: "app", "model export ended{}", if cancelled { ", cancelled" } else { "" });
                self.messages.push(export_summary(&running, cancelled));
                self.models_export.form = None;
            }
            None => self.models_export.running = Some(running),
        }
    }

    /// Stop the running export: the file being written is not published, those written stay.
    pub(in crate::window) fn cancel_model_export(&mut self) {
        if let Some(running) = &self.models_export.running {
            running.job.cancel();
        }
    }

    /// Export models…, while it is open.
    #[allow(clippy::too_many_lines)] // One dialog, its parts in order.
    pub(in crate::window) fn export_models_dialog(&mut self, ctx: &egui::Context) {
        self.follow_model_export(ctx);
        let Some(mut form) = self.models_export.form.take() else {
            return;
        };
        let running = self.models_export.running.is_some();
        let plate = self.workspace.plate().clone();
        let plates_with_models = self
            .workspace
            .plates()
            .iter()
            .filter(|p| !p.content().objects.is_empty())
            .count();
        // A model chosen for the Model scope: the one the dialog opened on, else the selection.
        if form.model.is_none() {
            form.model = self.selected.filter(|&i| i < plate.objects.len());
        }
        let model_name = form
            .model
            .and_then(|i| plate.objects.get(i))
            .map(|o| o.name.clone());
        let single = self.single_file(&form);
        let mut folder = self
            .models_export
            .choices
            .folder
            .as_ref()
            .map_or_else(String::new, |f| f.display().to_string());
        let mut pattern = self.models_export.choices.pattern.clone();
        let planned = if single {
            Vec::new()
        } else {
            self.planned_models(&form, std::path::Path::new(folder.trim()))
        };
        let states: Vec<State> = match &self.models_export.running {
            Some(r) => planned
                .iter()
                .map(|p| {
                    r.files
                        .iter()
                        .find(|(f, _)| f.file == p.file)
                        .map_or(State::Waiting, |(_, s)| s.clone())
                })
                .collect(),
            None => Vec::new(),
        };
        let phase = self
            .models_export
            .running
            .as_ref()
            .and_then(|r| r.phase.clone());
        let choices = &mut self.models_export.choices;
        let mut scope = match form.scope {
            Scope::Model => 0,
            Scope::Plate => 1,
            Scope::Every => 2,
        };
        let mut format = ModelFileFormat::ALL
            .iter()
            .position(|f| f.extension() == choices.format)
            .unwrap_or(0);
        let mut files = usize::from(choices.per_model);
        let (mut supports, mut raft) = (choices.supports, choices.raft);
        let mut objects = usize::from(choices.separate_objects);
        let (mut pick, mut go, mut cancel) = (false, false, false);
        let size = Size::Resizable {
            default: egui::vec2(640.0, 520.0),
            min: egui::vec2(520.0, 380.0),
        };
        let response =
            dialog::show(
                ctx,
                ID,
                &tr!("export-models-title"),
                size,
                |ui, part| match part {
                    Part::Body => {
                        ui.add_enabled_ui(!running, |ui| {
                            ui.label(tr!("export-models-scope"));
                            let labels = [
                                model_name
                                    .clone()
                                    .unwrap_or_else(|| tr!("export-models-no-model")),
                                tr!("export-models-plate", plate = &plate.name),
                                tr!("export-models-every", count = plates_with_models),
                            ];
                            let unavailable: Vec<usize> = if model_name.is_none() {
                                vec![0]
                            } else {
                                Vec::new()
                            };
                            choice_among(ui, &mut scope, &labels, &unavailable);
                            ui.add_space(8.0);
                            ui.label(tr!("export-models-format"));
                            let names: Vec<String> =
                                ["STL", "3MF", "OBJ"].map(str::to_owned).to_vec();
                            choice(ui, &mut format, &names);
                            ui.add_space(8.0);
                            ui.label(tr!("export-models-files"));
                            choice(
                                ui,
                                &mut files,
                                &[
                                    tr!("export-models-per-plate"),
                                    tr!("export-models-per-model"),
                                ],
                            );
                            // 3MF and OBJ name their objects: one per model, or its parts.
                            if format != 0 {
                                ui.add_space(8.0);
                                ui.label(tr!("export-models-objects"));
                                choice(
                                    ui,
                                    &mut objects,
                                    &[
                                        tr!("export-models-objects-joined"),
                                        tr!("export-models-objects-separate"),
                                    ],
                                );
                            }
                            ui.add_space(8.0);
                            switch_row(
                                ui,
                                &mut supports,
                                tr!("export-models-supports"),
                                tr!("export-models-supports-hint"),
                            );
                            switch_row(
                                ui,
                                &mut raft,
                                tr!("export-models-raft"),
                                tr!("export-models-raft-hint"),
                            );
                        });
                        ui.add_space(6.0);
                        let content = if format != 0 && objects == 1 {
                            tr!("export-models-content-objects")
                        } else {
                            tr!("export-models-content-joined")
                        };
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(content)
                                    .small()
                                    .color(ui.visuals().weak_text_color()),
                            )
                            .wrap(),
                        );
                        if !single {
                            ui.add_space(8.0);
                            ui.add_enabled_ui(!running, |ui| {
                                folder_fields(ui, &mut folder, &mut pattern, &mut pick);
                            });
                            ui.add_space(6.0);
                            file_table(ui, &planned, &states);
                        }
                    }
                    Part::Footer { enter } => {
                        let ready = !running
                            && (single || (!planned.is_empty() && !folder.trim().is_empty()));
                        let label = if single {
                            tr!("export-models-action-one")
                        } else {
                            tr!("export-models-action", count = planned.len())
                        };
                        if dialog::main_action(ui, &label, ready, enter) {
                            go = true;
                        }
                        if dialog::cancel_action(ui) {
                            cancel = true;
                        }
                        if let Some(phase) = &phase {
                            ui.add(egui::Label::new(phase.as_str()).truncate());
                        }
                    }
                },
            );
        // The choices, kept.
        form.scope = match scope {
            0 if model_name.is_some() => Scope::Model,
            2 => Scope::Every,
            _ => Scope::Plate,
        };
        ModelFileFormat::ALL[format.min(2)]
            .extension()
            .clone_into(&mut choices.format);
        choices.per_model = files == 1;
        choices.supports = supports;
        choices.raft = raft;
        choices.separate_objects = objects == 1;
        let trimmed = folder.trim();
        if !single {
            choices.folder = (!trimmed.is_empty()).then(|| PathBuf::from(trimmed));
        }
        choices.pattern = if pattern.trim().is_empty() {
            DEFAULT_MODEL_PATTERN.to_owned()
        } else {
            pattern
        };
        if pick {
            let mut picker = rfd::FileDialog::new().set_title(tr!("export-models-folder-title"));
            if let Some(folder) = &choices.folder {
                picker = picker.set_directory(folder);
            }
            if let Some(folder) = picker.pick_folder() {
                choices.folder = Some(folder);
            }
        }
        if cancel || response.cancel {
            if running {
                self.cancel_model_export();
                self.models_export.form = Some(form);
            }
            return;
        }
        let replacing = form.replacing.take();
        self.models_export.form = Some(form.clone());
        if go {
            if single {
                if let Some(file) = self.ask_model_file(&form) {
                    let plan = self.single_plan(&form, file);
                    // The system's save dialog asked before replacing a file.
                    self.start_model_export(plan, true);
                }
            } else {
                let planned = self.planned_models(&form, std::path::Path::new(folder.trim()));
                self.start_model_export(planned, false);
            }
        } else if let Some(files) = replacing {
            self.ask_model_replace(ctx, &form, files);
        }
    }

    /// The one file of a single-file export, at `file`.
    fn single_plan(&self, form: &Form, file: PathBuf) -> Vec<Planned> {
        let content = self.workspace.plate();
        let object = form.model.filter(|_| form.scope == Scope::Model);
        vec![Planned {
            plate: self.workspace.plate_id(),
            label: object
                .and_then(|i| content.objects.get(i))
                .map_or_else(|| content.name.clone(), |o| o.name.clone()),
            object,
            file,
        }]
    }

    /// Ask where the single file goes, offering its name in the folder used last.
    fn ask_model_file(&mut self, form: &Form) -> Option<PathBuf> {
        let format = self.models_export.format();
        let mut picker = rfd::FileDialog::new()
            .set_title(tr!("export-models-title"))
            .set_file_name(self.offered_name(form))
            .add_filter(format.extension().to_uppercase(), &[format.extension()]);
        if let Some(folder) = &self.models_export.choices.folder {
            picker = picker.set_directory(folder);
        }
        let mut file = picker.save_file()?;
        if file
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            != Some(format.extension().to_owned())
        {
            file.set_extension(format.extension());
        }
        self.models_export.choices.folder = file.parent().map(std::path::Path::to_path_buf);
        Some(file)
    }

    /// Ask whether to replace `files`, which exist already, and export once the user agrees.
    fn ask_model_replace(&mut self, ctx: &egui::Context, form: &Form, files: Vec<PathBuf>) {
        let names: Vec<String> = files
            .iter()
            .filter_map(|f| f.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .collect();
        match dialog::confirm(
            ctx,
            "export-models-replace",
            &tr!("export-all-replace-title", count = files.len()),
            &tr!("export-all-replace-text", files = names.join("\n")),
            &tr!("export-all-replace-action"),
            true,
        ) {
            Some(true) => {
                let folder = self
                    .models_export
                    .choices
                    .folder
                    .clone()
                    .unwrap_or_default();
                let planned = self.planned_models(form, &folder);
                self.start_model_export(planned, true);
            }
            Some(false) => {}
            None => {
                if let Some(form) = self.models_export.form.as_mut() {
                    form.replacing = Some(files);
                }
            }
        }
    }
}

/// The folder with Choose…, and the file names' pattern.
fn folder_fields(ui: &mut egui::Ui, folder: &mut String, pattern: &mut String, pick: &mut bool) {
    egui::Grid::new("export-models-fields")
        .num_columns(2)
        .spacing([10.0, 6.0])
        .show(ui, |ui| {
            ui.label(tr!("export-all-folder"));
            ui.horizontal(|ui| {
                let room = ui.available_width() - 90.0;
                ui.add(text_field(folder, room.max(120.0)));
                *pick |= ui.button(tr!("export-all-choose")).clicked();
            });
            ui.end_row();
            ui.label(tr!("export-all-names"));
            ui.horizontal(|ui| {
                ui.add(text_field(pattern, 220.0));
                ui.label(
                    egui::RichText::new(tr!("export-models-names-hint"))
                        .color(ui.visuals().weak_text_color()),
                );
            });
            ui.end_row();
        });
}

/// The files' table: what each holds, its file and its state.
fn file_table(ui: &mut egui::Ui, planned: &[Planned], states: &[State]) {
    use crate::theme;
    let weak = ui.visuals().weak_text_color();
    crate::focus::scrolled(
        egui::ScrollArea::vertical()
            .id_salt((ID, "files"))
            .max_height(ui.available_height().max(80.0))
            .auto_shrink([false, true]),
        ui,
        |ui| {
            egui::Grid::new((ID, "table"))
                .num_columns(3)
                .striped(true)
                .spacing([12.0, 6.0])
                .show(ui, |ui| {
                    for title in [
                        tr!("export-models-model"),
                        tr!("export-all-file"),
                        tr!("export-all-state"),
                    ] {
                        ui.label(egui::RichText::new(title).color(weak));
                    }
                    ui.end_row();
                    for (k, file) in planned.iter().enumerate() {
                        ui.label(&file.label);
                        let name = file
                            .file
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        ui.label(name);
                        let (text, colour) = match states.get(k) {
                            None | Some(State::Waiting) => (tr!("export-all-waiting"), weak),
                            Some(State::Working) => {
                                (tr!("export-models-working"), theme::ACCENT.color())
                            }
                            Some(State::Written(bytes)) => (
                                tr!("export-all-exported", size = megabytes(*bytes)),
                                theme::SUCCESS.color(),
                            ),
                            Some(State::Failed(_)) => {
                                (tr!("export-all-failed"), theme::ERROR.color())
                            }
                        };
                        ui.label(egui::RichText::new(text).small().color(colour));
                        ui.end_row();
                    }
                });
        },
    );
}

/// `bytes` in megabytes, as the table shows a file's size.
fn megabytes(bytes: u64) -> String {
    #[allow(clippy::cast_precision_loss)] // A file's size, shown to one decimal.
    let mb = bytes as f64 / 1_000_000.0;
    decimal(mb, usize::from(mb < 10.0))
}

/// What cutting a model's holes left, as the notification says it.
fn drilled_line(drilled: &karasu_project::pipeline::DrilledReport) -> String {
    let moved = decimal(drilled.farthest_mm * 1000.0, 1);
    let mut line = drilled_cut(drilled, &moved);
    if drilled.specks > 0 {
        line = format!(
            "{line}; {}",
            tr!("export-models-specks", count = drilled.specks)
        );
    }
    if drilled.separated > 0 {
        line = format!(
            "{line}; {}",
            tr!("export-models-separated", count = drilled.separated)
        );
    }
    line
}

/// What cutting a model's holes did, and what crossings or flat triangles remain.
fn drilled_cut(drilled: &karasu_project::pipeline::DrilledReport, moved: &str) -> String {
    if drilled.crossings + drilled.flat == 0 {
        tr!(
            "export-models-drilled",
            model = &drilled.model,
            moved = drilled.moved,
            distance = moved
        )
    } else {
        tr!(
            "export-models-drilled-left",
            model = &drilled.model,
            moved = drilled.moved,
            distance = moved,
            crossings = drilled.crossings,
            flat = drilled.flat
        )
    }
}

/// The notification at an export's end: the files written, what drilling left, the failures.
fn export_summary(running: &Running, cancelled: bool) -> Message {
    let written: Vec<&Planned> = running
        .files
        .iter()
        .filter(|(_, s)| matches!(s, State::Written(_)))
        .map(|(p, _)| p)
        .collect();
    let failed: Vec<String> = running
        .files
        .iter()
        .filter_map(|(p, s)| match s {
            State::Failed(why) => Some(format!("{}: {why}", p.label)),
            _ => None,
        })
        .chain(running.failed.iter().cloned())
        .collect();
    let left = running.left;
    let text = if cancelled {
        tr!("export-models-cancelled", count = written.len())
    } else {
        match written.as_slice() {
            [one] => tr!(
                "export-models-done-one",
                file = one.file.display().to_string()
            ),
            _ => tr!("export-models-done", count = written.len()),
        }
    };
    let mut detail: Vec<String> = written
        .iter()
        .map(|p| p.file.display().to_string())
        .collect();
    detail.extend(running.drilled.iter().cloned());
    detail.extend(failed.iter().cloned());
    let level = if !failed.is_empty() {
        Level::Error
    } else if cancelled || left {
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
    use super::{Scope, State};
    use crate::{actions::Action, i18n::tr, window::MainWindow};
    use eframe::egui;
    use std::time::{Duration, Instant};

    /// Follow the window's model export to its end, as the frames do.
    fn run_export(window: &mut MainWindow, ctx: &egui::Context) {
        let started = Instant::now();
        while window.models_export.running.is_some() {
            window.follow_model_export(ctx);
            assert!(started.elapsed() < Duration::from_secs(600), "no end");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// The texts a frame of the dialog draws.
    fn drawn(window: &mut MainWindow, ctx: &egui::Context) -> Vec<String> {
        let mut texts = Vec::new();
        for _ in 0..2 {
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                window.export_models_dialog(&ui.ctx().clone());
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
    }

    #[test]
    fn the_plate_s_models_go_to_a_folder_one_file_each_then_ask_before_replacing() {
        let ctx = egui::Context::default();
        let folder =
            std::env::temp_dir().join(format!("karasu-window-models-{}", std::process::id()));
        let mut window = MainWindow::new(Vec::new());
        window.workspace = crate::slicing_ui::tests::hollowed_cube(&folder);
        window.follow_plate(&ctx);
        assert!(window.enabled(Action::ExportModels));
        window.perform(Action::ExportModels);
        let form = window.models_export.form.clone().unwrap();
        assert_eq!(form.scope, Scope::Plate);
        // One file per plate of one model: a single file, named where Export… asks.
        let texts = drawn(&mut window, &ctx);
        for expected in [
            tr!("export-models-title"),
            tr!("export-models-format"),
            tr!("export-models-per-model"),
            tr!("export-models-supports"),
            tr!("export-models-action-one"),
        ] {
            assert!(texts.contains(&expected), "{expected} in {texts:?}");
        }
        // Every plate, one file per model, into a folder.
        let out = folder.join("models");
        window.models_export.choices.per_model = true;
        window.models_export.choices.format = "3mf".to_owned();
        let mut form = window.models_export.form.clone().unwrap();
        form.scope = Scope::Every;
        window.models_export.form = Some(form.clone());
        let planned = window.planned_models(&form, &out);
        assert_eq!(planned.len(), 1);
        assert!(planned[0].file.to_string_lossy().ends_with(".3mf"));
        window.start_model_export(planned.clone(), false);
        assert!(!window.enabled(Action::ExportModels));
        run_export(&mut window, &ctx);
        assert!(std::fs::metadata(&planned[0].file).unwrap().len() > 0);
        let latest = window.messages.latest().unwrap();
        assert_eq!(
            latest.text,
            tr!(
                "export-models-done-one",
                file = planned[0].file.display().to_string()
            )
        );
        assert!(window.models_export.form.is_none());
        // Again: the file is there, so the user is asked first; replaced once they agree.
        window.models_export.form = Some(form.clone());
        window.start_model_export(planned.clone(), false);
        assert!(window.models_export.running.is_none());
        assert!(window
            .models_export
            .form
            .as_ref()
            .unwrap()
            .replacing
            .is_some());
        window.start_model_export(planned.clone(), true);
        let files: Vec<State> = window
            .models_export
            .running
            .as_ref()
            .unwrap()
            .files
            .iter()
            .map(|(_, s)| s.clone())
            .collect();
        assert_eq!(files, [State::Waiting]);
        run_export(&mut window, &ctx);
        assert_eq!(
            window.messages.latest().unwrap().colour,
            crate::theme::SUCCESS.color()
        );
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_model_s_row_opens_the_dialog_on_it_and_one_file_holds_it() {
        let ctx = egui::Context::default();
        let folder =
            std::env::temp_dir().join(format!("karasu-window-models-one-{}", std::process::id()));
        let mut window = MainWindow::new(Vec::new());
        window.workspace = crate::slicing_ui::tests::hollowed_cube(&folder);
        window.follow_plate(&ctx);
        window.open_export_models(Some(0));
        let form = window.models_export.form.clone().unwrap();
        assert_eq!((form.scope, form.model), (Scope::Model, Some(0)));
        let name = window.workspace.plate().objects[0].name.clone();
        assert!(drawn(&mut window, &ctx).contains(&name));
        let file = folder.join("one.stl");
        let plan = window.single_plan(&form, file.clone());
        window.start_model_export(plan, true);
        run_export(&mut window, &ctx);
        assert!(std::fs::metadata(&file).unwrap().len() > 0);
        let _ = std::fs::remove_dir_all(&folder);
    }
}
