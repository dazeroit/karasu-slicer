//! `slice --project`: slice one plate of a `.karasu` project (TD-130), or every plate into a
//! folder (TD-159), through the shared pipeline, with each plate's own printer, resin, print
//! setting, settings and objects, in the format the application slices it in, so a project
//! saved by the application slices the same from the command line.
//!
//! **Every plate** (`--every-plate`), as the application's Export every plate does: the plates
//! with a model, in project order, each into its own file of the folder, named by the pattern
//! (`{project} {number} {plate}` unless `--names` gives another) with its format's extension,
//! the same name never twice ([`batch::plate_files`]). Files that exist already are refused
//! before any plate is sliced, unless `--replace`: each is then sliced beside it and renamed
//! into place. A plate whose slice fails is reported and the next one sliced; the run then ends
//! with exit status 1. Ctrl-C stops the plate being sliced and leaves those not reached.
use super::progress::{Progress, ProgressMode};
use super::slice::{
    self, interrupted, on_interrupt, path_value, progress_mode, set, start_log, text_value,
    SliceArgs, SliceFailure, INTERRUPTED,
};
use karasu_project::{
    pipeline,
    project::{
        batch::{self, PlateFile, DEFAULT_PLATE_FILES},
        file, Project,
    },
};
use std::{
    ffi::OsString,
    fmt::Write as _,
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::Ordering,
};

/// Which plates are sliced, and where their files go.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Plates {
    /// One plate, into one file.
    One {
        /// The plate: its 1-based position or its name; `None` for a project of one plate.
        plate: Option<String>,
        /// The print file to write.
        output: PathBuf,
    },
    /// Every plate with a model, each into its own file of `folder`.
    Every {
        /// The folder the files go into; it must exist.
        folder: PathBuf,
        /// The files' names' pattern ([`batch::plate_file_name`]).
        names: String,
        /// Files that exist already are replaced.
        replace: bool,
    },
}

/// Validated `slice --project` arguments; parsing performs no filesystem access.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ProjectArgs {
    /// The project file.
    pub(super) path: PathBuf,
    /// The plates and their files.
    pub(super) plates: Plates,
    /// How progress is reported on standard error.
    pub(super) progress: ProgressMode,
    /// The session log's file, or `None` for no log.
    pub(super) log: Option<PathBuf>,
}

/// Whether `slice`'s options name a project.
pub(super) fn names_project(arguments: &[OsString]) -> bool {
    arguments.iter().any(|a| a == "--project")
}

/// Parse `slice --project` options, in any order, each at most once. The project holds
/// everything else, so any other option is refused.
pub(super) fn parse(mut arguments: impl Iterator<Item = OsString>) -> Result<ProjectArgs, String> {
    let (mut path, mut plate, mut output, mut progress, mut log) = (None, None, None, None, None);
    let (mut every, mut folder, mut names, mut replace) = (false, None, None, false);
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
            .ok_or("slice options must be Unicode")?
            .to_owned();
        match name.as_str() {
            "--project" => set(&mut path, &name, path_value(&name, arguments.next())?)?,
            "--project-plate" => set(&mut plate, &name, text_value(&name, arguments.next())?)?,
            "--output" => set(&mut output, &name, path_value(&name, arguments.next())?)?,
            "--every-plate" => flag(&mut every, &name)?,
            "--folder" => set(&mut folder, &name, path_value(&name, arguments.next())?)?,
            "--names" => set(&mut names, &name, text_value(&name, arguments.next())?)?,
            "--replace" => flag(&mut replace, &name)?,
            "--progress" | "--no-progress" => {
                set(
                    &mut progress,
                    "--progress/--no-progress",
                    progress_mode(&name),
                )?;
            }
            "--log" => set(&mut log, &name, path_value(&name, arguments.next())?)?,
            "--auto-supports" | "--auto-braces" | "--write-supports" | "--auto-settings"
            | "--support-weight" | "--auto-density" | "--auto-cover" | "--auto-place"
            | "--auto-minis" => {
                return Err(format!(
                    "{name} works on one model (--model): a project's plate holds its objects' \
                     supports, which the application plans"
                ))
            }
            _ => {
                return Err(format!(
                    "{name} cannot be given with --project: the project holds the printer, \
                     resin, print setting, settings and objects; give only --project-plate and \
                     --output, or --every-plate with --folder, --names and --replace, and \
                     --progress or --no-progress, and --log"
                ))
            }
        }
    }
    let path = path.ok_or("--project requires a path")?;
    let plates = if every {
        if plate.is_some() {
            return Err("give --project-plate or --every-plate, not both".to_owned());
        }
        if output.is_some() {
            return Err(
                "--every-plate writes a file for each plate: give --folder <folder> \
                        (and --names) instead of --output"
                    .to_owned(),
            );
        }
        Plates::Every {
            folder: folder.ok_or("--every-plate requires --folder <folder>")?,
            names: names.unwrap_or_else(|| DEFAULT_PLATE_FILES.to_owned()),
            replace,
        }
    } else {
        for (given, option) in [
            (folder.is_some(), "--folder"),
            (names.is_some(), "--names"),
            (replace, "--replace"),
        ] {
            if given {
                return Err(format!("{option} goes with --every-plate"));
            }
        }
        Plates::One {
            plate,
            output: output.ok_or("--project requires --output <path>")?,
        }
    };
    Ok(ProjectArgs {
        path,
        plates,
        progress: progress.unwrap_or(ProgressMode::Auto),
        log,
    })
}

/// The plate `choice` names in `project`: a 1-based position, or a name no other plate has;
/// without a choice, the project's only plate.
pub(super) fn chosen(
    project: &Project,
    choice: Option<&str>,
) -> Result<karasu_project::project::PlateId, String> {
    let plates = project.plates();
    let listed = || {
        (1..)
            .zip(plates)
            .map(|(n, p)| format!("{n} {:?}", p.content().name))
            .collect::<Vec<_>>()
            .join(", ")
    };
    match choice {
        None if plates.len() == 1 => Ok(plates[0].id()),
        None => Err(format!(
            "the project has {} plates ({}); choose one with --project-plate <number or name>",
            plates.len(),
            listed()
        )),
        Some(choice) => {
            if let Ok(n) = choice.parse::<usize>() {
                return n
                    .checked_sub(1)
                    .and_then(|i| plates.get(i))
                    .map(karasu_project::project::Plate::id)
                    .ok_or_else(|| {
                        format!("the project has no plate {n}; its plates: {}", listed())
                    });
            }
            let named: Vec<_> = plates
                .iter()
                .filter(|p| p.content().name == choice)
                .collect();
            match named.as_slice() {
                [plate] => Ok(plate.id()),
                [] => Err(format!(
                    "the project has no plate named {choice:?}; its plates: {}",
                    listed()
                )),
                _ => Err(format!(
                    "{} plates are named {choice:?}; choose one by number: {}",
                    named.len(),
                    listed()
                )),
            }
        }
    }
}

/// Open the project and slice the plate, or every plate, as the arguments say.
pub(super) fn slice(args: &ProjectArgs, out: &mut impl Write) -> Result<(), SliceFailure> {
    match &args.plates {
        Plates::One { plate, output } => slice_one(args, plate.as_deref(), output, out),
        Plates::Every {
            folder,
            names,
            replace,
        } => {
            if let Some(path) = &args.log {
                start_log(path)?;
            }
            let result = slice_every(args, folder, names, *replace, out);
            if let Err(SliceFailure::Error(message)) = &result {
                log::error!(target: "slice", "{message}");
            }
            log::logger().flush();
            result
        }
    }
}

fn open(path: &Path) -> Result<Project, String> {
    file::open(path).map_err(|e| format!("cannot open the project {}: {e}", path.display()))
}

/// Take the plate's request and slice it as `slice` does, in the format the application
/// slices it in.
fn slice_one(
    args: &ProjectArgs,
    choice: Option<&str>,
    output: &Path,
    out: &mut impl Write,
) -> Result<(), SliceFailure> {
    let project = open(&args.path)?;
    let plate = chosen(&project, choice)?;
    let (mut request, _) = project
        .request(plate, output.to_path_buf())
        .map_err(|e| e.to_string())?;
    if let Some(plate) = project.plate(plate) {
        request.format = batch::slice_format(plate.content());
    }
    let request = request.checked()?;
    slice::slice(
        &SliceArgs {
            request,
            progress: args.progress,
            log: args.log.clone(),
            print_setting: None,
            image_options: false,
            supports: super::supports::SupportJob::default(),
        },
        out,
    )
}

/// The project's name in the files' names: its file's, else its own, else "Untitled", as the
/// application names it.
fn label(path: &Path, project: &Project) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .filter(|s| !s.trim().is_empty())
        .or_else(|| {
            let name = project.name.trim();
            (!name.is_empty()).then(|| name.to_owned())
        })
        .unwrap_or_else(|| "Untitled".to_owned())
}

/// Slice every plate with a model into `folder` (see the module's documentation).
fn slice_every(
    args: &ProjectArgs,
    folder: &Path,
    names: &str,
    replace: bool,
    out: &mut impl Write,
) -> Result<(), SliceFailure> {
    let project = open(&args.path)?;
    if !folder.is_dir() {
        return Err(format!("the folder {} does not exist", folder.display()).into());
    }
    let files = batch::plate_files(&project, &label(&args.path, &project), names);
    // Plates without a model are left out, as the application leaves them unchecked.
    let (files, empty): (Vec<PlateFile>, Vec<PlateFile>) = files.into_iter().partition(|f| {
        project
            .plate(f.id)
            .is_some_and(|p| !p.content().objects.is_empty())
    });
    if files.is_empty() {
        return Err("the project has no plate with a model".to_owned().into());
    }
    let existing: Vec<&str> = files
        .iter()
        .filter(|f| folder.join(&f.file).exists())
        .map(|f| f.file.as_str())
        .collect();
    if !existing.is_empty() && !replace {
        return Err(format!(
            "{} file(s) exist already in {}: {}; give --replace to replace them",
            existing.len(),
            folder.display(),
            existing.join(", ")
        )
        .into());
    }
    let mut progress = Progress::new(args.progress);
    if let Err(e) = ctrlc::set_handler(on_interrupt) {
        progress.notice(&format!(
            "warning: Ctrl-C cancellation is unavailable ({e}); interrupting may leave a staging file"
        ));
    }
    let total = files.len();
    let (mut report, mut sliced, mut failed) = (String::new(), 0, Vec::new());
    for plate in &files {
        progress.phase(&format!(
            "Plate {} of {} ({:?})",
            plate.number,
            project.plates().len(),
            plate.name
        ));
        match slice_plate(&project, plate, &folder.join(&plate.file), &mut progress) {
            Ok(summary) => {
                sliced += 1;
                let _ = writeln!(report, "Plate number: {}", plate.number);
                report.push_str(&summary);
                report.push('\n');
            }
            Err(SliceFailure::Error(reason)) => {
                log::error!(target: "slice", "plate {} {:?}: {reason}", plate.number, plate.name);
                failed.push(format!("plate {} {:?}: {reason}", plate.number, plate.name));
            }
            Err(_) => {
                progress.finish();
                write_report(out, &report)?;
                return Err(SliceFailure::InterruptedAfter(format!(
                    "{sliced} of {total} plates were sliced and published before it; the plate \
                     being sliced was not"
                )));
            }
        }
    }
    progress.finish();
    let _ = writeln!(report, "Plates sliced: {sliced} of {total}");
    if !empty.is_empty() {
        let left: Vec<String> = empty
            .iter()
            .map(|p| format!("{} {:?}", p.number, p.name))
            .collect();
        let _ = writeln!(report, "Plates without a model: {}", left.join(", "));
    }
    write_report(out, &report)?;
    if failed.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "{} of {total} plates were not sliced: {}",
            failed.len(),
            failed.join("; ")
        )
        .into())
    }
}

/// Slice `plate` into `target`, as the application slices it, and give its summary. A file
/// there already is replaced: the plate is sliced into a sibling, renamed into place once whole.
fn slice_plate(
    project: &Project,
    plate: &PlateFile,
    target: &Path,
    progress: &mut Progress,
) -> Result<String, SliceFailure> {
    let staging = target.exists().then(|| staging_of(target));
    let output = staging.clone().unwrap_or_else(|| target.to_path_buf());
    let (mut request, _) = project
        .request(plate.id, output.clone())
        .map_err(|e| e.to_string())?;
    request.format.clone_from(&plate.format);
    let request = request.checked()?;
    let outcome = match pipeline::slice(&request, progress, &interrupted) {
        Ok(outcome) => outcome,
        // A failure after an interrupt request is the interruption it caused.
        Err(_) if INTERRUPTED.load(Ordering::Acquire) => return Err(SliceFailure::Interrupted),
        Err(e) => return Err(SliceFailure::Error(e.message().to_owned())),
    };
    if let Some(staging) = &staging {
        publish(staging, target)
            .map_err(|e| format!("cannot replace {}: {e}", target.display()))?;
    }
    // TD-095: the output is kept for inspection, but the plate fails.
    if let Some(reason) = outcome.verification.and_then(|v| v.failure) {
        return Err(format!(
            "wall verification failed: {reason}; the output was written for inspection"
        )
        .into());
    }
    // The summary names the file where it now is: the staging name is the target's with a
    // marker before its extension.
    Ok(match staging {
        Some(_) => outcome.summary.replacen(&staging_marker(), "", 1),
        None => outcome.summary,
    })
}

/// What [`staging_of`] puts before a file's extension.
fn staging_marker() -> String {
    format!(".karasu-partial-{}", std::process::id())
}

/// A sibling of `target` no other run uses, for slicing beside a file being replaced: its name
/// with a marker before the extension, which the format's exporter checks.
fn staging_of(target: &Path) -> PathBuf {
    let mut name = target.file_stem().unwrap_or_default().to_os_string();
    name.push(staging_marker());
    if let Some(extension) = target.extension() {
        name.push(".");
        name.push(extension);
    }
    target.with_file_name(name)
}

/// Put `staging`, a whole slice, in the place of `target`: a file is renamed over it; the
/// reference format's folder replaces the old one, which is removed first.
fn publish(staging: &Path, target: &Path) -> std::io::Result<()> {
    if target.is_dir() {
        std::fs::remove_dir_all(target)?;
    }
    std::fs::rename(staging, target).inspect_err(|_| {
        let _ = if staging.is_dir() {
            std::fs::remove_dir_all(staging)
        } else {
            std::fs::remove_file(staging)
        };
    })
}

fn write_report(out: &mut impl Write, report: &str) -> Result<(), SliceFailure> {
    out.write_all(report.as_bytes())
        .and_then(|()| out.flush())
        .map_err(|e| SliceFailure::from(format!("cannot write the slice summary: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(arguments: &[&str]) -> Result<ProjectArgs, String> {
        parse(arguments.iter().map(OsString::from))
    }

    #[test]
    fn a_project_takes_its_plate_and_output_and_nothing_else() {
        let args = parsed(&[
            "--project",
            "a.karasu",
            "--output",
            "a.goo",
            "--project-plate",
            "Second",
            "--no-progress",
        ])
        .unwrap();
        assert_eq!(args.path, PathBuf::from("a.karasu"));
        assert_eq!(
            args.plates,
            Plates::One {
                plate: Some("Second".into()),
                output: "a.goo".into()
            }
        );
        assert_eq!(args.progress, ProgressMode::Never);
        assert!(parsed(&["--project", "a.karasu"])
            .unwrap_err()
            .contains("--output"));
        let refused = parsed(&[
            "--project",
            "a.karasu",
            "--output",
            "a.goo",
            "--hollow",
            "2",
        ])
        .unwrap_err();
        assert!(refused.contains("--hollow cannot be given"), "{refused}");
        assert!(
            parsed(&["--project", "a", "--project", "b", "--output", "c"])
                .unwrap_err()
                .contains("only once")
        );
    }

    #[test]
    fn every_plate_goes_into_a_folder_named_by_a_pattern() {
        // TD-159: the application's pattern unless --names gives another.
        let args = parsed(&["--project", "a.karasu", "--every-plate", "--folder", "out"]).unwrap();
        assert_eq!(
            args.plates,
            Plates::Every {
                folder: "out".into(),
                names: "{project} {number} {plate}".into(),
                replace: false
            }
        );
        let args = parsed(&[
            "--project",
            "a.karasu",
            "--every-plate",
            "--folder",
            "out",
            "--names",
            "{plate}",
            "--replace",
        ])
        .unwrap();
        assert_eq!(
            args.plates,
            Plates::Every {
                folder: "out".into(),
                names: "{plate}".into(),
                replace: true
            }
        );
        for (arguments, refused) in [
            (&["--every-plate"][..], "--folder"),
            (
                &["--every-plate", "--folder", "o", "--output", "a.goo"],
                "instead of --output",
            ),
            (
                &["--every-plate", "--folder", "o", "--project-plate", "1"],
                "not both",
            ),
            (
                &["--output", "a.goo", "--folder", "o"],
                "--folder goes with --every-plate",
            ),
            (
                &["--output", "a.goo", "--names", "{plate}"],
                "--names goes with",
            ),
            (&["--output", "a.goo", "--replace"], "--replace goes with"),
            (
                &["--every-plate", "--every-plate", "--folder", "o"],
                "only once",
            ),
        ] {
            let mut all = vec!["--project", "a.karasu"];
            all.extend(arguments);
            let error = parsed(&all).unwrap_err();
            assert!(error.contains(refused), "{arguments:?}: {error}");
        }
    }

    #[test]
    fn a_plate_is_chosen_by_number_or_name() {
        use karasu_project::project::{PlateContent, PlateSettings};
        let mut project = Project::new("Plates");
        let plate = |name: &str| PlateContent {
            name: name.to_owned(),
            printer: None,
            resin: None,
            print_setting: None,
            settings: PlateSettings::new("goo-v3"),
            objects: Vec::new(),
        };
        let first = project.add_plate(plate("First"));
        assert_eq!(chosen(&project, None), Ok(first));
        let second = project.add_plate(plate("Twin"));
        let third = project.add_plate(plate("Twin"));
        assert!(chosen(&project, None).unwrap_err().contains("3 plates"));
        assert_eq!(chosen(&project, Some("1")), Ok(first));
        assert_eq!(chosen(&project, Some("3")), Ok(third));
        assert_eq!(chosen(&project, Some("First")), Ok(first));
        assert!(chosen(&project, Some("Twin"))
            .unwrap_err()
            .contains("by number"));
        assert!(chosen(&project, Some("4"))
            .unwrap_err()
            .contains("no plate 4"));
        assert!(chosen(&project, Some("Other"))
            .unwrap_err()
            .contains("no plate named"));
        let _ = second;
    }
}
