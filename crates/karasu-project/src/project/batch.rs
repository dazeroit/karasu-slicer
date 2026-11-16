//! Plates sliced one after another (TD-135, phase 4; TD-159): the format each plate is sliced
//! in, and the names of their print files, as the application's Export every plate and the
//! command line's `slice --project --every-plate` give them.
//!
//! **Format.** A plate is sliced in its own format when its printer writes it, else in the first
//! its printer writes ([`plate_format`]): a plate whose printer changed may still hold the format
//! of the one before. The reference format, chosen to check a slice, stays ([`slice_format`]).
//!
//! **Names.** [`plate_file_name`] fills a pattern with the project's name, the plate's 1-based
//! number and its name, made a name every file system accepts; [`plate_files`] gives every
//! plate's file with its format's extension, the same name never twice whatever the case (a
//! later plate's gets " (2)", then " (3)", and so on).
use super::{PlateContent, PlateId, Project};
use crate::request::{format_extension, format_ids, REFERENCE_FORMAT};
use std::collections::HashSet;

/// The pattern of the plates' file names until the user sets another.
pub const DEFAULT_PLATE_FILES: &str = "{project} {number} {plate}";

/// The formats `printer` lists that this build can write, in its order; with `reference`, then
/// the reference format.
#[must_use]
pub fn printer_formats(
    printer: Option<&karasu_core::profiles::PrinterProfile>,
    reference: bool,
) -> Vec<String> {
    let supported: Vec<String> = format_ids().split(", ").map(str::to_owned).collect();
    let mut formats: Vec<String> = printer.map_or_default(|printer| {
        printer
            .format_ids
            .iter()
            .filter(|f| supported.contains(f) && f.as_str() != REFERENCE_FORMAT)
            .cloned()
            .collect()
    });
    if reference && printer.is_some() {
        formats.push(REFERENCE_FORMAT.to_owned());
    }
    formats
}

/// The format a slice of `plate` is written in, as the application's Slice panel settles it,
/// shown or not (TD-135, phase 4: a plate not shown has not had its format settled by the
/// panel): its own when its printer writes it, else the first its printer writes; `None`
/// without one.
#[must_use]
pub fn plate_format(plate: &PlateContent) -> Option<String> {
    let formats = printer_formats(plate.printer.as_ref(), false);
    let current = &plate.settings.format;
    if formats.contains(current) {
        Some(current.clone())
    } else {
        formats.into_iter().next()
    }
}

/// The format `plate` is sliced in: the reference format when chosen, else [`plate_format`],
/// else the plate's own.
#[must_use]
pub fn slice_format(plate: &PlateContent) -> String {
    if plate.settings.format == REFERENCE_FORMAT {
        plate.settings.format.clone()
    } else {
        plate_format(plate).unwrap_or_else(|| plate.settings.format.clone())
    }
}

/// `pattern` with `{project}`, `{number}` and `{plate}` replaced, made a file name every system
/// accepts: no separator or reserved character, no control character, no dot or space at the
/// end, never empty.
#[must_use]
pub fn plate_file_name(pattern: &str, project: &str, number: usize, plate: &str) -> String {
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

/// A plate's print file, as [`plate_files`] names it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlateFile {
    /// The plate.
    pub id: PlateId,
    /// Its 1-based place in the project.
    pub number: usize,
    /// Its name.
    pub name: String,
    /// The format it is sliced in ([`slice_format`]).
    pub format: String,
    /// The file's name: the pattern filled, with the format's extension (none for the reference
    /// format's folder).
    pub file: String,
}

/// Every plate's print file, in project order, named by `pattern` with `label` as the project's
/// name: the same name never twice whatever the case.
#[must_use]
pub fn plate_files(project: &Project, label: &str, pattern: &str) -> Vec<PlateFile> {
    let mut taken = HashSet::new();
    project
        .plates()
        .iter()
        .enumerate()
        .map(|(k, plate)| {
            let content = plate.content();
            let format = slice_format(content);
            let base = plate_file_name(pattern, label, k + 1, &content.name);
            let with = |stem: &str| match format_extension(&format) {
                Some(ext) => format!("{stem}.{ext}"),
                None => stem.to_owned(),
            };
            let mut file = with(&base);
            let mut copy = 2;
            while !taken.insert(file.to_lowercase()) {
                file = with(&format!("{base} ({copy})"));
                copy += 1;
            }
            PlateFile {
                id: plate.id(),
                number: k + 1,
                name: content.name.clone(),
                format,
                file,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_follow_the_pattern_and_every_system_accepts_them() {
        let pattern = "{project} {number} {plate}";
        assert_eq!(
            plate_file_name(pattern, "venom_figure", 2, "Torso"),
            "venom_figure 2 Torso"
        );
        assert_eq!(plate_file_name(pattern, "a/b", 1, "c:d?"), "a_b 1 c_d_");
        assert_eq!(plate_file_name("{plate}. ", "p", 3, "Arms"), "Arms");
        assert_eq!(plate_file_name("{plate}", "p", 4, " "), "4");
        assert_eq!(plate_file_name("{plate}", "p", 5, "tab\there"), "tab_here");
    }
}
