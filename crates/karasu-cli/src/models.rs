//! `export-models`: write the models of a `.karasu` project's plates as binary STL, 3MF or OBJ
//! (TD-139), as the print holds them: turned, scaled, hollowed, their drain holes cut, with
//! their supports and rafts by choice. The same export as the application's, through the shared
//! pipeline.
use super::progress::{Progress, ProgressMode};
use super::project::chosen;
use super::slice::{
    interrupted, on_interrupt, path_value, progress_mode, set, start_log, text_value, SliceFailure,
    INTERRUPTED,
};
use karasu_core::export::models::ModelFileFormat;
use karasu_project::{
    pipeline::{export_models, model_file_name, ModelExport, ModelFiles, ModelObjects, NameFields},
    project::{file, PlateId, Project},
};
use std::{
    collections::HashSet,
    ffi::OsString,
    fmt::Write as _,
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::Ordering,
};

/// Which plates to export.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Plates {
    /// One plate: by number or name, or the project's only plate.
    One(Option<String>),
    /// Every plate.
    Every,
}

/// Where the files go.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Output {
    /// One file, named in full.
    File(PathBuf),
    /// A folder, each file named by a pattern ([`model_file_name`]).
    Folder { folder: PathBuf, names: String },
}

/// Validated `export-models` arguments; parsing performs no filesystem access.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ExportArgs {
    pub(super) project: PathBuf,
    pub(super) plates: Plates,
    /// Only this model, 1-based in its plate.
    pub(super) model: Option<usize>,
    pub(super) format: ModelFileFormat,
    pub(super) files: ModelFiles,
    pub(super) supports: bool,
    pub(super) raft: bool,
    pub(super) replace: bool,
    /// One object per model, or model, supports and raft apart.
    pub(super) objects: ModelObjects,
    pub(super) output: Output,
    pub(super) progress: ProgressMode,
    pub(super) log: Option<PathBuf>,
}

/// The default file names in a folder: the plate's, or the plate's and the model's.
fn default_names(files: ModelFiles) -> &'static str {
    match files {
        ModelFiles::PerPlate => "{plate}",
        ModelFiles::PerModel => "{plate} {model}",
    }
}

/// Parse `export-models` options, in any order, each at most once.
pub(super) fn parse(mut arguments: impl Iterator<Item = OsString>) -> Result<ExportArgs, String> {
    let (mut project, mut plate, mut every, mut model) = (None, None, false, None);
    let (mut format, mut per_model, mut no_supports, mut no_raft, mut replace) =
        (None, false, false, false, false);
    let mut separate_objects = false;
    let (mut output, mut folder, mut names, mut progress, mut log) =
        (None, None, None::<String>, None, None);
    let flag = |slot: &mut bool, name: &str| {
        if std::mem::replace(slot, true) {
            Err(format!("{name} may be given only once"))
        } else {
            Ok(())
        }
    };
    while let Some(option) = arguments.next() {
        let name = option
            .to_str()
            .ok_or("export-models options must be Unicode")?
            .to_owned();
        match name.as_str() {
            "--project" => set(&mut project, &name, path_value(&name, arguments.next())?)?,
            "--project-plate" => set(&mut plate, &name, text_value(&name, arguments.next())?)?,
            "--every-plate" => flag(&mut every, &name)?,
            "--model" => {
                let text = text_value(&name, arguments.next())?;
                let n = text
                    .parse::<usize>()
                    .ok()
                    .filter(|&n| n > 0)
                    .ok_or_else(|| {
                        format!("--model requires a model number from 1, not {text:?}")
                    })?;
                set(&mut model, &name, n)?;
            }
            "--format" => {
                let text = text_value(&name, arguments.next())?;
                let value = ModelFileFormat::from_extension(&text)
                    .ok_or_else(|| format!("--format is stl, 3mf or obj, not {text:?}"))?;
                set(&mut format, &name, value)?;
            }
            "--per-model" => flag(&mut per_model, &name)?,
            "--no-supports" => flag(&mut no_supports, &name)?,
            "--no-raft" => flag(&mut no_raft, &name)?,
            "--replace" => flag(&mut replace, &name)?,
            "--separate-objects" => flag(&mut separate_objects, &name)?,
            "--output" => set(&mut output, &name, path_value(&name, arguments.next())?)?,
            "--folder" => set(&mut folder, &name, path_value(&name, arguments.next())?)?,
            "--names" => set(&mut names, &name, text_value(&name, arguments.next())?)?,
            "--progress" | "--no-progress" => {
                set(
                    &mut progress,
                    "--progress/--no-progress",
                    progress_mode(&name),
                )?;
            }
            "--log" => set(&mut log, &name, path_value(&name, arguments.next())?)?,
            _ => return Err(format!("unknown export-models option: {name}")),
        }
    }
    let project = project.ok_or("export-models requires --project <file.karasu>")?;
    if every && plate.is_some() {
        return Err("give --project-plate or --every-plate, not both".to_owned());
    }
    let format = format.ok_or("export-models requires --format stl, 3mf or obj")?;
    let files = if per_model {
        ModelFiles::PerModel
    } else {
        ModelFiles::PerPlate
    };
    // One file is named in full; several go into a folder.
    let several = every || (per_model && model.is_none());
    let output = output_of(output, folder, names, several, files)?;
    Ok(ExportArgs {
        project,
        plates: if every {
            Plates::Every
        } else {
            Plates::One(plate)
        },
        model,
        format,
        files,
        supports: !no_supports,
        raft: !no_raft,
        replace,
        objects: if separate_objects {
            ModelObjects::Separate
        } else {
            ModelObjects::OnePerModel
        },
        output,
        progress: progress.unwrap_or(ProgressMode::Auto),
        log,
    })
}

/// Where the files go: one file named in full, or a folder and its names' pattern.
fn output_of(
    output: Option<PathBuf>,
    folder: Option<PathBuf>,
    names: Option<String>,
    several: bool,
    files: ModelFiles,
) -> Result<Output, String> {
    match (output, folder) {
        (Some(_), Some(_)) => Err("give --output or --folder, not both".to_owned()),
        (Some(_), None) if several => Err(
            "this export writes several files: give --folder <folder> (and --names) instead of \
             --output"
                .to_owned(),
        ),
        (Some(_), None) if names.is_some() => Err("--names goes with --folder".to_owned()),
        (Some(path), None) => Ok(Output::File(path)),
        (None, Some(folder)) => Ok(Output::Folder {
            folder,
            names: names.unwrap_or_else(|| default_names(files).to_owned()),
        }),
        (None, None) => {
            Err("export-models requires --output <file> or --folder <folder>".to_owned())
        }
    }
}

/// The file of a plate (`model` `None`) or of one of its models, where `output` puts it.
fn destination(
    output: &Output,
    format: ModelFileFormat,
    project: &str,
    plate: &str,
    model: Option<(usize, &str)>,
) -> PathBuf {
    match output {
        Output::File(path) => path.clone(),
        Output::Folder { folder, names } => {
            let name = model_file_name(
                names,
                &NameFields {
                    project,
                    plate,
                    number: model.map(|(n, _)| n),
                    model: model.map(|(_, m)| m),
                },
            );
            folder.join(format!("{name}.{}", format.extension()))
        }
    }
}

/// Every file the export will write, so that two never share a name.
fn planned(args: &ExportArgs, project: &Project, plates: &[PlateId]) -> Result<(), String> {
    let mut seen = HashSet::new();
    for &id in plates {
        let Some(plate) = project.plates().iter().find(|p| p.id() == id) else {
            continue;
        };
        let content = plate.content();
        let labels: Vec<Option<(usize, &str)>> = match args.files {
            ModelFiles::PerPlate => vec![None],
            ModelFiles::PerModel => (1..)
                .zip(&content.objects)
                .filter(|(n, _)| args.model.is_none_or(|m| m == *n))
                .map(|(n, o)| Some((n, o.name.as_str())))
                .collect(),
        };
        for label in labels {
            let path = destination(
                &args.output,
                args.format,
                &project.name,
                &content.name,
                label,
            );
            if !seen.insert(path.clone()) {
                return Err(format!(
                    "two files would be named {}; add {{number}} or {{plate}} to --names",
                    path.display()
                ));
            }
        }
    }
    Ok(())
}

/// Open the project and export its plates' models.
pub(super) fn export(args: &ExportArgs, out: &mut impl Write) -> Result<(), SliceFailure> {
    if let Some(path) = &args.log {
        start_log(path)?;
    }
    let result = export_logged(args, out);
    if let Err(SliceFailure::Error(message)) = &result {
        log::error!(target: "export", "{message}");
    }
    log::logger().flush();
    result
}

fn export_logged(args: &ExportArgs, out: &mut impl Write) -> Result<(), SliceFailure> {
    let project = file::open(&args.project)
        .map_err(|e| format!("cannot open the project {}: {e}", args.project.display()))?;
    let plates: Vec<PlateId> = match &args.plates {
        Plates::Every => project
            .plates()
            .iter()
            .map(karasu_project::project::Plate::id)
            .collect(),
        Plates::One(choice) => vec![chosen(&project, choice.as_deref())?],
    };
    planned(args, &project, &plates)?;
    if let Output::Folder { folder, .. } = &args.output {
        if !folder.is_dir() {
            return Err(format!("the folder {} does not exist", folder.display()).into());
        }
    }
    let mut progress = Progress::new(args.progress);
    if let Err(e) = ctrlc::set_handler(on_interrupt) {
        progress.notice(&format!(
            "warning: Ctrl-C cancellation is unavailable ({e}); interrupting may leave a staging file"
        ));
    }
    let mut failed = 0;
    let mut report = String::new();
    for id in plates {
        let plate = project
            .plates()
            .iter()
            .find(|p| p.id() == id)
            .ok_or("the plate disappeared")?;
        let plate_name = plate.content().name.clone();
        let (request, _) = project
            .request(id, Path::new("unused").to_path_buf())
            .map_err(|e| format!("plate {plate_name:?}: {e}"))?;
        let export = ModelExport {
            format: args.format,
            files: args.files,
            supports: args.supports,
            raft: args.raft,
            object: args.model.map(|n| n - 1),
            replace: args.replace,
            objects: args.objects,
        };
        let name = |model: Option<(usize, &str)>| {
            destination(&args.output, args.format, &project.name, &plate_name, model)
        };
        let outcome = export_models(&request, &export, &name, None, &mut progress, &interrupted);
        let outcome = match outcome {
            Ok(outcome) => outcome,
            Err(_) if INTERRUPTED.load(Ordering::Acquire) => {
                progress.finish();
                return Err(SliceFailure::Interrupted);
            }
            Err(e) => {
                progress.finish();
                return Err(format!("plate {plate_name:?}: {}", e.message()).into());
            }
        };
        failed += report_plate(&outcome, &mut report);
    }
    progress.finish();
    out.write_all(report.as_bytes())
        .and_then(|()| out.flush())
        .map_err(|e| format!("cannot write the export summary: {e}"))?;
    if failed > 0 {
        return Err(format!("{failed} file(s) were not written").into());
    }
    Ok(())
}

/// Add to `report` what a plate's export did: each drilled model and each file. Returns the
/// number of files not written.
fn report_plate(
    outcome: &karasu_project::pipeline::ModelExportOutcome,
    report: &mut String,
) -> usize {
    let mut failed = 0;
    for drilled in &outcome.drilled {
        let left = if drilled.crossings + drilled.flat == 0 {
            "nothing crosses".to_owned()
        } else {
            format!(
                "{} crossing pair(s) and {} flat triangle(s) remain",
                drilled.crossings, drilled.flat
            )
        };
        let _ = writeln!(
            report,
            "{}: drain holes cut; {} round(s), {} vertices moved by at most {:.2} µm{}; {left}",
            drilled.model,
            drilled.rounds,
            drilled.moved,
            drilled.farthest_mm * 1000.0,
            {
                let mut extra = String::new();
                if drilled.specks > 0 {
                    let _ = write!(
                        extra,
                        ", {} speck(s) under 15.6 µm left out",
                        drilled.specks
                    );
                }
                if drilled.separated > 0 {
                    let _ = write!(
                        extra,
                        ", {} vertices moved 0.06 µm apart where bodies touched",
                        drilled.separated
                    );
                }
                extra
            }
        );
    }
    for file in &outcome.files {
        match &file.result {
            Ok(summary) => {
                let _ = writeln!(
                    report,
                    "wrote {}: {} body(ies) of {} ({} triangles, {} bytes)",
                    file.path.display(),
                    summary.bodies,
                    file.models.join(", "),
                    summary.triangles,
                    summary.bytes
                );
                if summary.shared_edges > 0 {
                    let _ = writeln!(
                        report,
                        "  note: {} edge(s) are still used by more than two triangles, where \
                         sheets of the model meet; 3MF readers may report them",
                        summary.shared_edges
                    );
                }
            }
            Err(e) => {
                failed += 1;
                let _ = writeln!(report, "not written: {}", e.message());
            }
        }
    }
    failed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(arguments: &[&str]) -> Result<ExportArgs, String> {
        parse(arguments.iter().map(OsString::from))
    }

    #[test]
    fn one_file_is_named_in_full_and_several_go_to_a_folder() {
        let one = parsed(&[
            "--project",
            "p.karasu",
            "--format",
            "3MF",
            "--output",
            "a.3mf",
        ])
        .unwrap();
        assert_eq!(one.format, ModelFileFormat::ThreeMf);
        assert_eq!(one.files, ModelFiles::PerPlate);
        assert_eq!(one.output, Output::File("a.3mf".into()));
        assert!(one.supports && one.raft && !one.replace);
        let several = parsed(&[
            "--project",
            "p.karasu",
            "--every-plate",
            "--per-model",
            "--format",
            "stl",
            "--folder",
            "out",
            "--no-raft",
        ])
        .unwrap();
        assert_eq!(several.plates, Plates::Every);
        assert_eq!(
            several.output,
            Output::Folder {
                folder: "out".into(),
                names: "{plate} {model}".into()
            }
        );
        assert!(!several.raft);
        // One model of one plate is one file.
        let model = parsed(&[
            "--project",
            "p.karasu",
            "--per-model",
            "--model",
            "2",
            "--format",
            "obj",
            "--output",
            "m.obj",
        ])
        .unwrap();
        assert_eq!(model.model, Some(2));
    }

    #[test]
    fn contradictions_and_missing_options_are_refused() {
        let refused = |a: &[&str]| parsed(a).unwrap_err();
        assert!(refused(&["--format", "stl", "--output", "a.stl"]).contains("--project"));
        assert!(refused(&["--project", "p", "--output", "a.stl"]).contains("--format"));
        assert!(
            refused(&["--project", "p", "--format", "ply", "--output", "a"]).contains("stl, 3mf")
        );
        assert!(refused(&["--project", "p", "--format", "stl"]).contains("--output"));
        assert!(refused(&[
            "--project",
            "p",
            "--format",
            "stl",
            "--every-plate",
            "--output",
            "a.stl"
        ])
        .contains("--folder"));
        assert!(refused(&[
            "--project",
            "p",
            "--format",
            "stl",
            "--project-plate",
            "1",
            "--every-plate",
            "--folder",
            "f"
        ])
        .contains("not both"));
        assert!(refused(&[
            "--project",
            "p",
            "--format",
            "stl",
            "--model",
            "0",
            "--output",
            "a"
        ])
        .contains("from 1"));
        assert!(
            refused(&["--project", "p", "--format", "stl", "--hollow", "2"]).contains("unknown")
        );
    }

    #[test]
    fn folder_names_follow_the_pattern() {
        let output = Output::Folder {
            folder: "out".into(),
            names: "{project} {plate} {number}".into(),
        };
        assert_eq!(
            destination(
                &output,
                ModelFileFormat::Stl,
                "Venom",
                "Arms",
                Some((3, "Hand"))
            ),
            PathBuf::from("out").join("Venom Arms 3.stl")
        );
    }
}
