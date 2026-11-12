//! Command-line entry point for karasu-slicer.

use std::env;
use std::ffi::OsString;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;

mod inspect;
mod models;
mod progress;
mod project;
mod slice;

use karasu_core::{
    APPLICATION_DESCRIPTION, APPLICATION_NAME, COPYRIGHT, LICENCE, LICENCE_URL, SOURCE_URL,
};

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, PartialEq)]
enum Command {
    Help,
    Version,
    Inspect(inspect::InspectArgs),
    Slice(Box<slice::SliceArgs>),
    SliceProject(project::ProjectArgs),
    ExportModels(Box<models::ExportArgs>),
}

fn parse_command(
    arguments: impl IntoIterator<Item = impl Into<OsString>>,
) -> Result<Command, String> {
    let mut arguments = arguments.into_iter().map(Into::into);
    let first = arguments.next();
    if first.as_deref() == Some(std::ffi::OsStr::new("inspect")) {
        return parse_inspect(arguments);
    }
    if first.as_deref() == Some(std::ffi::OsStr::new("export-models")) {
        return models::parse(arguments).map(|args| Command::ExportModels(Box::new(args)));
    }
    if first.as_deref() == Some(std::ffi::OsStr::new("slice")) {
        let arguments: Vec<OsString> = arguments.collect();
        if project::names_project(&arguments) {
            return project::parse(arguments.into_iter()).map(Command::SliceProject);
        }
        return slice::parse(arguments.into_iter()).map(|args| Command::Slice(Box::new(args)));
    }

    match (
        first.as_deref().and_then(std::ffi::OsStr::to_str),
        arguments.next(),
    ) {
        (None, None) if first.is_some() => Err("unknown non-Unicode argument".to_owned()),
        (None | Some("-h" | "--help"), None) => Ok(Command::Help),
        (Some("-V" | "--version"), None) => Ok(Command::Version),
        (Some(argument), None) => Err(format!("unknown argument: {argument}")),
        (_, Some(_)) => Err("too many arguments".to_owned()),
    }
}

fn parse_inspect(mut arguments: impl Iterator<Item = OsString>) -> Result<Command, String> {
    let (mut path, mut units, mut legacy_stl, mut diagnosis) =
        (None::<PathBuf>, None, false, false);
    let (mut minima, mut rotate) = (false, None::<[f64; 3]>);
    while let Some(option) = arguments.next() {
        match option.to_str() {
            Some(name @ ("--assume-mm" | "--model")) => {
                let value = arguments
                    .next()
                    .ok_or("inspect requires exactly one input path")?;
                if value.is_empty() || value.to_string_lossy().starts_with('-') {
                    return Err("inspect requires a nonempty input path, not an option; prefix a leading '-' filename with './'".to_owned());
                }
                if path.replace(value.into()).is_some() {
                    return Err(
                        "unexpected arguments: inspect accepts exactly one input path".to_owned(),
                    );
                }
                if name == "--assume-mm" {
                    if units
                        .replace(karasu_core::mesh::formats::LengthUnit::Millimeter)
                        .is_some()
                    {
                        return Err(
                            "--assume-mm already sets millimeters; use --model with --units"
                                .to_owned(),
                        );
                    }
                    legacy_stl = true;
                }
            }
            Some("--diagnosis") => {
                if std::mem::replace(&mut diagnosis, true) {
                    return Err("--diagnosis is given only once".to_owned());
                }
            }
            Some("--minima") => {
                if std::mem::replace(&mut minima, true) {
                    return Err("--minima is given only once".to_owned());
                }
            }
            Some("--rotate") => {
                let mut angles = [0.0; 3];
                for (axis, value) in ["X", "Y", "Z"].into_iter().zip(&mut angles) {
                    *value = arguments
                        .next()
                        .and_then(|v| v.to_str().and_then(|v| v.parse::<f64>().ok()))
                        .filter(|v| v.is_finite())
                        .ok_or_else(|| {
                            format!("--rotate requires a finite {axis} angle in degrees")
                        })?;
                }
                if rotate.replace(angles).is_some() {
                    return Err("--rotate is given only once".to_owned());
                }
            }
            Some("--units") => {
                let unit = slice::unit_value("--units", arguments.next())?;
                if units.replace(unit).is_some() {
                    return Err(
                        "--assume-mm already sets millimeters; use --model with --units".to_owned(),
                    );
                }
            }
            _ if path.is_some() => {
                return Err(
                    "unexpected arguments: inspect accepts exactly one input path".to_owned(),
                )
            }
            _ => return Err(
                "inspect requires explicit --assume-mm (or --model) before exactly one input path"
                    .to_owned(),
            ),
        }
    }
    let path = path.ok_or(
        "inspect requires explicit --assume-mm (or --model) before exactly one input path",
    )?;
    if rotate.is_some() && !minima {
        return Err("--rotate is used only with --minima".to_owned());
    }
    if minima && diagnosis {
        return Err("--minima is a report of its own: give it without --diagnosis".to_owned());
    }
    Ok(Command::Inspect(inspect::InspectArgs {
        path,
        units,
        legacy_stl,
        diagnosis,
        minima: minima.then_some(rotate.unwrap_or([0.0; 3])),
    }))
}

/// The version, with the copyright and the licence's notice (owner, 2026-11-11, TD-145).
fn version_text() -> String {
    format!(
        "{APPLICATION_NAME} {VERSION}\n{COPYRIGHT}\n\
         License {LICENCE}: GNU Affero GPL version 3 or later <{LICENCE_URL}>.\n\
         This is free software: you are free to change and redistribute it.\n\
         There is NO WARRANTY, to the extent permitted by law.\n\
         Source: <{SOURCE_URL}>. The name karasu-slicer and its logo are trademarks."
    )
}

#[allow(clippy::too_many_lines)] // One block of help text.
fn help_text() -> String {
    format!(
        "{APPLICATION_NAME} {VERSION}\n{APPLICATION_DESCRIPTION}\n\n\
         Usage: karasu-cli [OPTIONS]\n       karasu-cli inspect (--assume-mm <path> | --model <path> [--units <unit>]) [--diagnosis | --minima [--rotate <x> <y> <z>]]\n       \
         karasu-cli slice --project <project.karasu> [--project-plate <number or name>] --output <path>\n         \
         \x20 [--progress | --no-progress] [--log <file>]\n       \
         karasu-cli export-models --project <project.karasu> [--project-plate <number or name> | --every-plate]\n         \
         \x20 [--model <n>] --format stl|3mf|obj [--per-model] [--no-supports] [--no-raft] [--separate-objects] [--replace]\n         \
         \x20 (--output <file> | --folder <folder> [--names <pattern>]) [--progress | --no-progress] [--log <file>]\n       \
         karasu-cli slice (--assume-mm <model> | --model <model> [--units <unit>] | --plate <plate.json>)\n         \
         --printer <printer.json> \
         --resin <resin.json> \
         --format <id> --output <path>\n         [--translate <x> <y> <z> | [--center] [--drop]] \
         [--rotate <x> <y> <z>] [--scale <x> <y> <z>]\n         [--mirror <axes>] [--job-id <id>] \
         [--fill union|strict] [--close-holes <mm> | --no-close-holes]\n         \
         [--reversed-tolerance <mm2>]\n         \
         [--repair none|off|auto|exact|standard|aggressive|rebuild|<list>] [--antialias 1|2|4|8|16 [--min-gray <0-255>] [--antialias-supports]]\n         \
         [--xy-compensation <px>] [--bottom-xy-compensation <px>]\n         \
         [--supports <file.json> [--accept-support-collisions]] [--drop --lift <mm>]\n         \
         [--layer-plan <file.json>] [--copies <n>] [--arrange] [--spacing <mm>]\n         \
         [--allow-topology-defects] \
         [--progress | --no-progress]\n         \
         [--hollow <wall mm> [--hollow-cell <mm>]] [--drain-hole <x> <y> <z> <axis> <length> <diameter>]...\n         \
         [--surface-drain-hole <x> <y> <z> <diameter> [--hole-depth <mm>] [--hole-from outer|cavity]\n         \
         \x20 [--hole-tilt <angle> <direction>]]...\n         \
         [--verify-walls] [--unsupported [--overhang-angle <deg>]]\n         \
         [--suction-cups [--min-vent <mm2>]] [--log <file>]\n\n\
         Options:\n  -h, --help       Print help\n  -V, --version    Print version\n\n\
         Commands:\n  inspect --assume-mm <path>\n    Inspect binary/ASCII STL in source-space millimeters without repair or placement.\n\
         \x20   With --model, inspect any supported model (STL, OBJ, 3MF) in millimeters; --units\n\
         \x20   (mm, cm, m, um, in, ft) is required for STL and OBJ, while 3MF declares its unit.\n\
         \x20   --diagnosis adds the extended diagnosis (zero-area faces, cracks, pinch vertices,\n\
         \x20   small bodies, self-intersections, where each defect is), the printability state,\n\
         \x20   and what the exact repair preset would change.\n\
         \x20   --minima reports instead the local minima (TD-107): where material starts in\n\
         \x20   mid-air and needs a support, with the model turned by --rotate (degrees, X then\n\
         \x20   Y then Z) and its lowest point on the plate.\n\
         \n  slice --project <project.karasu> --output <path>\n\
         \x20   Slice one plate of a karasu-slicer project (TD-130) with its own printer, resin,\n\
         \x20   print setting, settings and objects: placement, repair, hollowing, drain holes,\n\
         \x20   blockers and supports, as the application slices it. --project-plate chooses the\n\
         \x20   plate by its number (from 1) or its name; a project of one plate needs none. Every\n\
         \x20   other option belongs in the project and is refused beside it.\n\
         \n  export-models --project <project.karasu> --format stl|3mf|obj (--output <file> | --folder <folder>)\n\
         \x20   Write the models of a project's plate (TD-139) as they stand on it, in millimetres:\n\
         \x20   turned, scaled, hollowed, their drain holes cut into the mesh (TD-140), with their\n\
         \x20   supports and rafts unless --no-supports or --no-raft. STL holds one solid of\n\
         \x20   overlapping shells; 3MF and OBJ hold each model with its supports and raft as one\n\
         \x20   object, or with --separate-objects the model, its supports and its raft apart.\n\
         \x20   One file for the plate, or with --per-model one for each model (--model <n> for one\n\
         \x20   model only); --every-plate exports every plate. Several files go into --folder,\n\
         \x20   named by --names: {{project}}, {{plate}}, {{number}} and {{model}} (\"{{plate}}\", or\n\
         \x20   \"{{plate}} {{model}}\" with --per-model). An existing file is never replaced without\n\
         \x20   --replace; each file is written whole or not at all. Each drilled model reports what\n\
         \x20   rounding to the kernel's grid moved and anything left crossing. Exit status 1 when\n\
         \x20   any file was not written.\n\
         Reports show full-precision bounds and at most 100 details per diagnostic section,\n\
         with accurate totals and omitted counts. Closed topology does not prove printability.\n\
         \n  slice (--assume-mm <model> | --model <model> [--units <unit>]) --printer <printer.json> --resin <resin.json> [--print-setting <setting.json>] --format <id> --output <path>\n\
         \x20   Slice an STL, OBJ or 3MF model (chosen by extension) and export one new print file (or\n\
         \x20   reference directory). --assume-mm means --units mm and reads unknown extensions as STL;\n\
         \x20   3MF declares its unit, and a contradicting --units is an error.\n\
         \x20   Profiles are typed JSON documents; the resin needs a calibration for the printer ID.\n\
         \x20   With --print-setting (a library print setting, kind print_setting), --resin is a\n\
         \x20   library resin (kind material): the setting supplies the calibration, anti-aliasing,\n\
         \x20   minimum gray and XY compensation, so --antialias, --min-gray and the compensations\n\
         \x20   are refused beside it.\n\
         \x20   --rotate (degrees, fixed X then Y then Z), --scale (positive factors) and --mirror\n\
         \x20   (axes such as x or xy) act about the source bounding-box centre. The model is then\n\
         \x20   translated by --translate (default 0 0 0), or --center centres it on the plate and\n\
         \x20   --drop rests it exactly on Z = 0. It must rest exactly on Z = 0 and lie within the\n\
         \x20   LCD area; it is never repaired silently or clipped.\n\
         \x20   --fill union (default) fills where the oriented surface's winding number is positive:\n\
         \x20   overlapping and touching bodies merge, reversed shells stay empty; layers that do not\n\
         \x20   close fail with the offending edges. --fill strict requires embedded contours; both\n\
         \x20   give identical files when strict succeeds. Mesh defects are reported. With union,\n\
         \x20   open sections at holes are closed in each layer by the chord across the hole, paired\n\
         \x20   along the hole's rim, up to --close-holes mm (default 2; --no-close-holes refuses\n\
         \x20   holes); longer chords fail. A negative winding number (an inside-out body, or the\n\
         \x20   reversed sliver of a self-intersecting fold) fails the layer, unless\n\
         \x20   --reversed-tolerance A leaves up to A mm^2 of it empty in each layer and reports\n\
         \x20   it; more still fails. --repair (default auto = duplicates,orientation) works\n\
         \x20   on a derived mesh: duplicate faces collapse to their net orientation, orientation is\n\
         \x20   made consistent per component, fill-holes (opt-in) caps simple closed hole rims of up\n\
         \x20   to 256 edges with flat-as-possible triangles (no new vertices; refused holes are\n\
         \x20   reported), and outward (opt-in) turns outermost inside-out closed bodies outward;\n\
         \x20   weld[=mm] merges vertices within mm (equal positions without it) into groups no\n\
         \x20   wider than mm, degenerate[=mm] removes faces with a repeated vertex and, with mm,\n\
         \x20   collapses edges up to mm and flips faces whose apex is within mm of their longest\n\
         \x20   edge, and cracks=mm (with fill-holes) closes cracks within mm of a line; mm is at\n\
         \x20   most 1. fins removes open parts that make an edge non-manifold (a flap or a\n\
         \x20   hanging sheet, never the body they hang from); cut-non-manifold also removes every\n\
         \x20   face on an edge still non-manifold, for fill-holes to close. With fill-holes,\n\
         \x20   split-rims caps a rim through a vertex twice as its simple loops, refine-holes adds\n\
         \x20   vertices to caps (refined to the size of the faces around, faired as a membrane;\n\
         \x20   needed past 512 edges) and unbounded-holes lifts the 256-edge bound. exact is all of\n\
         \x20   them without tolerances, fins, refinement or the bound, refused holes left open and\n\
         \x20   reported; visibility turns open parts to face where they are seen from outside;\n\
         \x20   small-shells=mm2 and small-volume=mm3 remove smaller shells, inner-shells closed\n\
         \x20   outward shells inside other bodies (never cavities). standard adds weld, degenerate\n\
         \x20   and cracks at 0.001 mm, fins and refine-holes; aggressive adds cut-non-manifold,\n\
         \x20   unbounded-holes, visibility and small-shells=1. rebuild[=mm] (default 0.05) rebuilds\n\
         \x20   the model as a solid on a grid of that cell, last: closed and manifold, within the\n\
         \x20   reported deviation; the rebuild preset is aggressive plus rebuild. non-finite\n\
         \x20   accepts an STL whose facets with a NaN or infinite coordinate are left out (they have\n\
         \x20   no position; the summary lists them); exact and the stronger presets include it.\n\
         \x20   Every change is reported. Meshes with winding conflicts left\n\
         \x20   (union), or that are not closed manifold surfaces (strict), are refused unless\n\
         \x20   --allow-topology-defects is given; every layer must still close or resolve.\n\
         \x20   Layers are sampled at slab midpoints, moved just below any that meets a\n\
         \x20   vertex height. Masks are binary unless --antialias N samples each pixel on an\n\
         \x20   N x N grid and stores its covered fraction as grey (8-bit GOO, 16-level PP1, CTB\n\
         \x20   rounded to its 7-bit levels; refused for photon-v1-firmware-motion); the level is\n\
         \x20   recorded in the file. --min-gray G maps partial coverage linearly onto grays\n\
         \x20   G..255 (empty pixels stay 0), for resins that do not cure below gray G. The\n\
         \x20   supports are binary (TD-134): a pixel whose centre lies in a support is full, and\n\
         \x20   only the model is anti-aliased; --antialias-supports anti-aliases them too.\n\
         \x20   --xy-compensation N grows (N > 0) or shrinks (N < 0) every layer by N pixels (holes\n\
         \x20   change the opposite way); --bottom-xy-compensation N replaces it on the resin's\n\
         \x20   bottom layers, such as -1 against elephant's foot.\n\
         \x20   --supports reads manual supports (karasu-supports JSON): tips on the model surface\n\
         \x20   (outer or cavity), feet on the plate, the model or another support, braces, raft.\n\
         \x20   Clearance is proven; collisions, piercing and drain-hole crossings fail unless\n\
         \x20   collisions are accepted. --lift raises a dropped model for its supports.\n\
         \x20   --layer-plan reads Z ranges (karasu-layers JSON) with fixed or adaptive layer\n\
         \x20   heights; layers that differ from the calibration take their range's exposure or\n\
         \x20   the document's height-to-exposure table, never a guess. Bottom and transition\n\
         \x20   layers keep the calibrated height; SL1 and SL1S store one height and refuse it.\n\
         \x20   --copies N prints N instances of the model and --arrange places them (or one)\n\
         \x20   automatically: footprints with supports and drain holes, packed in shelves at\n\
         \x20   --spacing mm (default 2), centred, never rotated; instances that do not fit fail.\n\
         \x20   --plate reads a karasu-plate JSON document instead of one model: objects with\n\
         \x20   their own model, placement, hollowing, drain holes, supports and copies, fixed or\n\
         \x20   arranged around fixed ones. Overlapping objects print as their union.\n\
         \x20   Formats: {formats}.\n\
         \x20   The output must not exist.\n\
         \x20   --hollow removes the interior deeper than the given wall thickness (exact distance)\n\
         \x20   and guarantees that wall: every cavity face is proven at least that far from the\n\
         \x20   surface, with further passes when needed; the summary gives the guaranteed and the\n\
         \x20   thickest wall. The grid cell follows the wall and the model's surface (a 0.05 mm\n\
         \x20   over-thickness, at most about 10 million cavity triangles); --hollow-cell sets it\n\
         \x20   instead. The source must be closed and consistently oriented after --repair (add\n\
         \x20   fill-holes for open models, which must then fill every hole); touching and\n\
         \x20   overlapping bodies are hollowed as their union. Without --drain-hole the cavity\n\
         \x20   stays enclosed.\n\
         \x20   --drain-hole cuts a cylinder from world point x y z along axis +x -x +y -y +z -z\n\
         \x20   (repeatable, up to 1024, with or without --hollow). Its path is never refused, and\n\
         \x20   holes may overlap: the summary says whether each opens to the outside, reaches a\n\
         \x20   cavity, passes through the model or ends in material.\n\
         \x20   --surface-drain-hole cuts a hole of the given diameter where the surface is nearest\n\
         \x20   world point x y z, along its normal; without --hole-depth it reaches 1 mm past the\n\
         \x20   cavity wall (needs --hollow). The options after it shape it: --hole-depth sets a\n\
         \x20   fixed depth from the surface, --hole-from cavity starts it on the cavity's surface\n\
         \x20   and cuts out through the wall (needs --hollow), --hole-tilt leans its axis by up to\n\
         \x20   60 degrees towards a direction measured around the normal.\n\
         \x20   --verify-walls checks the sliced layers (TD-095): the thinnest printed wall between\n\
         \x20   the cavity and the outside, by exact distance on the print's pixels and layers, wall\n\
         \x20   breaches (cavity reaching the outside other than through a drain hole) and pockets\n\
         \x20   that do not drain. A breach, or a printed wall under the certified one by more than\n\
         \x20   one voxel, fails the job (exit 1) after writing its output, for inspection.\n\
         \x20   --unsupported finds the print's unsupported regions on the sliced layers (TD-107):\n\
         \x20   islands, which start in mid-air, and overhangs, which hang out from material that\n\
         \x20   holds them, beyond the self-supporting distance of the layer height over the\n\
         \x20   tangent of --overhang-angle (degrees from the plate, 45 by default). Supports in the\n\
         \x20   job count as material. It reports and never fails the job.\n\
         \x20   --suction-cups finds the print's suction cups on the sliced layers (TD-114): resin a\n\
         \x20   layer encloses while it is peeled from the film, capped towards the plate (by the\n\
         \x20   print, its raft or the plate) and not vented. An opening vents only where a circle\n\
         \x20   of --min-vent mm^2 (1 by default; 0 for any empty pixel) fits through it, less one\n\
         \x20   pixel. Each cup gives its layers, its cap, its widest area and volume, and whether\n\
         \x20   it closes over trapped resin. It reports and never fails the job.\n\
         \x20   --log writes the job's log to the file (TD-096), replaced, at most 20 MB: the\n\
         \x20   command line, the phases and their durations, the hollowing and holes, the\n\
         \x20   verification, the summary and any error. Without it nothing is logged to disk.\n\
         \x20   Progress goes to stderr: live on a terminal by default, --progress forces one line\n\
         \x20   per percent elsewhere, --no-progress disables it. Ctrl-C cancels cooperatively\n\
         \x20   without publishing output (exit 130); a second Ctrl-C exits immediately.\n\n\
         Quote paths containing spaces; prefix leading '-' filenames with './'.\n\
         Exit status: 0 completed (inspect includes defective meshes), 1 processing/output failure,\n\
         2 usage error, 130 slice interrupted by Ctrl-C.",
        formats = slice::format_ids()
    )
}

pub(crate) fn main() -> ExitCode {
    let command = match parse_command(env::args_os().skip(1)) {
        Ok(command) => command,
        Err(message) => {
            let _ = writeln!(
                io::stderr().lock(),
                "error: {message}\n\nTry '--help' for usage information."
            );
            return ExitCode::from(2);
        }
    };
    let result = match command {
        Command::Help => Ok(writeln!(io::stdout().lock(), "{}", help_text())
            .map_err(|e| format!("cannot write help: {e}"))),
        Command::Version => Ok(writeln!(io::stdout().lock(), "{}", version_text())
            .map_err(|e| format!("cannot write version: {e}"))),
        Command::Inspect(args) => Ok(inspect::inspect_model(&args, &mut io::stdout().lock())),
        Command::Slice(args) => sliced(slice::slice(&args, &mut io::stdout().lock())),
        Command::SliceProject(args) => sliced(project::slice(&args, &mut io::stdout().lock())),
        Command::ExportModels(args) => sliced(models::export(&args, &mut io::stdout().lock())),
    };
    let result = match result {
        Ok(result) => result,
        Err(status) => return status,
    };
    finish(result)
}

/// A slice's result, or the exit status of an interrupted one.
fn sliced(result: Result<(), slice::SliceFailure>) -> Result<Result<(), String>, ExitCode> {
    match result {
        Ok(()) => Ok(Ok(())),
        Err(slice::SliceFailure::Error(message)) => Ok(Err(message)),
        Err(slice::SliceFailure::Interrupted) => {
            let _ = writeln!(
                io::stderr().lock(),
                "error: interrupted by Ctrl-C; no output was published"
            );
            Err(ExitCode::from(slice::INTERRUPTED_EXIT_STATUS))
        }
    }
}

/// The exit status of a command's result; a failure is reported on standard error.
fn finish(result: Result<(), String>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            let _ = writeln!(io::stderr().lock(), "error: {message}");
            ExitCode::from(1)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{help_text, parse_command, Command, VERSION};

    fn arguments(values: &[&str]) -> Vec<String> {
        values.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn no_arguments_selects_help() {
        assert_eq!(parse_command(arguments(&[])), Ok(Command::Help));
    }

    #[test]
    fn recognized_options_select_the_expected_command() {
        assert_eq!(parse_command(arguments(&["--help"])), Ok(Command::Help));
        assert_eq!(parse_command(arguments(&["-V"])), Ok(Command::Version));
    }

    #[test]
    fn unsupported_arguments_are_rejected() {
        let error = parse_command(arguments(&["--slice"]))
            .expect_err("unsupported functionality must not be advertised");
        assert!(error.contains("unknown argument"));
    }

    #[test]
    fn help_identifies_the_application_and_version() {
        let help = help_text();
        assert!(help.contains("karasu-slicer"));
        assert!(help.contains(VERSION));
        assert!(help.contains("--help"));
    }
}
