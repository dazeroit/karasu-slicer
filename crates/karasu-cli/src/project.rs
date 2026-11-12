//! `slice --project`: slice one plate of a `.karasu` project (TD-130) through the shared
//! pipeline, with the plate's own printer, resin, print setting, settings and objects, so a
//! project saved by the application slices the same from the command line.
use super::progress::ProgressMode;
use super::slice::{self, path_value, progress_mode, set, text_value, SliceArgs, SliceFailure};
use karasu_project::project::{file, Project};
use std::{ffi::OsString, io::Write, path::PathBuf};

/// Validated `slice --project` arguments; parsing performs no filesystem access.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ProjectArgs {
    /// The project file.
    pub(super) path: PathBuf,
    /// The plate: its 1-based position or its name; `None` for a project of one plate.
    pub(super) plate: Option<String>,
    /// The print file to write.
    pub(super) output: PathBuf,
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
    while let Some(option) = arguments.next() {
        let name = option
            .to_str()
            .ok_or("slice options must be Unicode")?
            .to_owned();
        match name.as_str() {
            "--project" => set(&mut path, &name, path_value(&name, arguments.next())?)?,
            "--project-plate" => set(&mut plate, &name, text_value(&name, arguments.next())?)?,
            "--output" => set(&mut output, &name, path_value(&name, arguments.next())?)?,
            "--progress" | "--no-progress" => {
                set(
                    &mut progress,
                    "--progress/--no-progress",
                    progress_mode(&name),
                )?;
            }
            "--log" => set(&mut log, &name, path_value(&name, arguments.next())?)?,
            _ => {
                return Err(format!(
                    "{name} cannot be given with --project: the project holds the printer, \
                     resin, print setting, settings and objects; give only --project-plate, \
                     --output, --progress or --no-progress, and --log"
                ))
            }
        }
    }
    Ok(ProjectArgs {
        path: path.ok_or("--project requires a path")?,
        plate,
        output: output.ok_or("--project requires --output <path>")?,
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

/// Open the project, take its plate's request and slice it as `slice` does.
pub(super) fn slice(args: &ProjectArgs, out: &mut impl Write) -> Result<(), SliceFailure> {
    let project = file::open(&args.path)
        .map_err(|e| format!("cannot open the project {}: {e}", args.path.display()))?;
    let plate = chosen(&project, args.plate.as_deref())?;
    let (request, _) = project
        .request(plate, args.output.clone())
        .map_err(|e| e.to_string())?;
    let request = request.checked()?;
    slice::slice(
        &SliceArgs {
            request,
            progress: args.progress,
            log: args.log.clone(),
            print_setting: None,
        },
        out,
    )
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
        assert_eq!(args.plate.as_deref(), Some("Second"));
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
