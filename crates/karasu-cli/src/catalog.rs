//! The built-in catalog on the command line (TD-155): `--printer`, `--resin` and
//! `--print-setting` take a catalog id where a profile file is not given, and `catalog` lists
//! what the catalog holds.
use karasu_project::library::{Catalog, CatalogMaterial, CatalogPrinter};
use std::{
    ffi::OsString,
    fmt::Write as _,
    io::Write,
    path::{Path, PathBuf},
};

/// Which catalog section an option names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Printer,
    Resin,
    PrintSetting,
}

impl Kind {
    const fn option(self) -> &'static str {
        match self {
            Self::Printer => "--printer",
            Self::Resin => "--resin",
            Self::PrintSetting => "--print-setting",
        }
    }

    const fn noun(self) -> &'static str {
        match self {
            Self::Printer => "printer",
            Self::Resin => "resin",
            Self::PrintSetting => "print setting",
        }
    }
}

/// Where a profile comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Source {
    /// A profile file.
    File(PathBuf),
    /// A built-in catalog entry, by id.
    Catalog(String),
}

/// The value as a catalog id when it could be one: no path separator, no extension dot.
/// Catalog ids are lowercase words joined by hyphens, so a path never reads as one by accident.
fn as_id(value: &Path) -> Option<&str> {
    let text = value.to_str()?;
    let id = !text.is_empty()
        && text
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    id.then_some(text)
}

/// Whether `value` reads as a catalog id, so the catalog is worth reading.
pub(super) fn reads_as_id(value: &Path) -> bool {
    as_id(value).is_some()
}

/// The ids of the catalog's entries of `kind`.
fn ids(catalog: &Catalog, kind: Kind) -> Vec<&str> {
    match kind {
        Kind::Printer => catalog
            .printers()
            .iter()
            .map(|p| p.profile.id.as_str())
            .collect(),
        Kind::Resin => catalog
            .materials()
            .iter()
            .map(|m| m.profile.id.as_str())
            .collect(),
        Kind::PrintSetting => catalog
            .settings()
            .iter()
            .map(|s| s.profile.id.as_str())
            .collect(),
    }
}

/// Where `value` comes from: a file when it is one, a catalog entry when it reads as an id of
/// `kind`, an error naming both possibilities otherwise. A value that is both an existing file
/// and a catalog id is refused rather than guessed: `./value` names the file.
pub(super) fn source(
    value: &Path,
    kind: Kind,
    catalog: &Catalog,
    exists: impl Fn(&Path) -> bool,
) -> Result<Source, String> {
    let Some(id) = as_id(value) else {
        return Ok(Source::File(value.to_owned()));
    };
    let known = ids(catalog, kind).contains(&id);
    match (exists(value), known) {
        (true, true) => Err(format!(
            "{option} {id} names both a file and a catalog {noun}: write ./{id} for the file",
            option = kind.option(),
            noun = kind.noun()
        )),
        (true, false) => Ok(Source::File(value.to_owned())),
        (false, true) => Ok(Source::Catalog(id.to_owned())),
        (false, false) => Err(format!(
            "{option} {id}: no such file and no catalog {noun} with that id{hint}",
            option = kind.option(),
            noun = kind.noun(),
            hint = suggestions(&ids(catalog, kind), id)
        )),
    }
}

/// Up to five ids that contain `text`, or share its words, for an error message.
fn suggestions(ids: &[&str], text: &str) -> String {
    let words: Vec<&str> = text.split('-').filter(|w| w.len() > 1).collect();
    let mut close: Vec<&str> = ids.iter().copied().filter(|id| id.contains(text)).collect();
    if close.is_empty() {
        let mut scored: Vec<(usize, &str)> = ids
            .iter()
            .map(|id| {
                (
                    words
                        .iter()
                        .filter(|w| id.split('-').any(|p| p == **w))
                        .count(),
                    *id,
                )
            })
            .filter(|(score, _)| *score > 0)
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(b.1)));
        close = scored.into_iter().map(|(_, id)| id).collect();
    }
    close.truncate(5);
    if close.is_empty() {
        "; `karasu-cli catalog` lists them".to_owned()
    } else {
        format!(
            "; did you mean {}? `karasu-cli catalog` lists them",
            close.join(", ")
        )
    }
}

/// The catalog compiled into this build.
pub(super) fn built_in() -> Result<Catalog, String> {
    Catalog::built_in().map_err(|e| format!("the built-in catalog cannot be read: {e}"))
}

/// What `catalog` lists.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum CatalogArgs {
    /// Every printer.
    Printers,
    /// One printer and the resins with print settings for it.
    Printer(String),
    /// Every resin.
    Resins,
}

/// Parse the options after `catalog`: none, `--printer <id>` or `--resins`.
pub(super) fn parse(arguments: impl Iterator<Item = OsString>) -> Result<CatalogArgs, String> {
    let arguments: Vec<String> = arguments
        .map(|a| {
            a.into_string()
                .map_err(|_| "catalog options must be Unicode".to_owned())
        })
        .collect::<Result<_, _>>()?;
    match arguments
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        [] => Ok(CatalogArgs::Printers),
        ["--printer", id] if !id.starts_with('-') => Ok(CatalogArgs::Printer((*id).to_owned())),
        ["--printer"] | ["--printer", _] => {
            Err("--printer requires a catalog printer id".to_owned())
        }
        ["--resins"] => Ok(CatalogArgs::Resins),
        _ => Err(format!(
            "catalog takes no option, --printer <id> or --resins, not {}",
            arguments.join(" ")
        )),
    }
}

/// Write what `args` asks for.
pub(super) fn list(args: &CatalogArgs, out: &mut impl Write) -> Result<(), String> {
    let catalog = built_in()?;
    let text = match args {
        CatalogArgs::Printers => printers(&catalog),
        CatalogArgs::Printer(id) => {
            let printer = catalog.printer(id).ok_or_else(|| {
                format!(
                    "no catalog printer {id}{}",
                    suggestions(&ids(&catalog, Kind::Printer), id)
                )
            })?;
            printer_details(&catalog, printer)
        }
        CatalogArgs::Resins => resins(&catalog),
    };
    out.write_all(text.as_bytes())
        .map_err(|e| format!("cannot write the catalog: {e}"))
}

fn orientation(printer: &CatalogPrinter) -> &'static str {
    if printer.orientation_verified {
        "verified"
    } else {
        "not verified"
    }
}

fn printers(catalog: &Catalog) -> String {
    let mut text = format!(
        "Printers of the built-in catalog ({}). Slice with --printer <id>; \
         `karasu-cli catalog --printer <id>` lists a printer's resins.\n\n\
         {:<34} {:<34} {:<12} {:>6}  FORMATS\n",
        catalog.printers().len(),
        "ID",
        "PRINTER",
        "ORIENTATION",
        "RESINS"
    );
    for printer in catalog.printers() {
        let p = &printer.profile;
        let _ = writeln!(
            text,
            "{:<34} {:<34} {:<12} {:>6}  {}",
            p.id,
            p.model.as_deref().unwrap_or(&p.name),
            orientation(printer),
            catalog.resins_for(&p.id).len(),
            p.format_ids.join(", ")
        );
    }
    text
}

fn printer_details(catalog: &Catalog, printer: &CatalogPrinter) -> String {
    let p = &printer.profile;
    let [x, y, z] = p.build_size_mm;
    let mut text = format!(
        "{} ({})\nPanel: {} x {} px; build {x} x {y} x {z} mm\nFormats: {}\n\
         Image orientation: {}\n",
        p.model.as_deref().unwrap_or(&p.name),
        p.id,
        p.resolution_px[0],
        p.resolution_px[1],
        p.format_ids.join(", "),
        orientation(printer)
    );
    let resins = catalog.resins_for(&p.id);
    if resins.is_empty() {
        text.push_str(
            "\nNo resin has a print setting for this printer in the catalog: give --resin and \
             --print-setting files of your own.\n",
        );
        return text;
    }
    let _ = write!(
        text,
        "\nResins with print settings ({}); slice with --printer {} --resin <id>:\n\n\
         {:<38} {:<40} {:>8}  PRINT SETTING\n",
        resins.len(),
        p.id,
        "RESIN ID",
        "RESIN",
        "LAYER"
    );
    for resin in resins {
        for setting in catalog.settings_for(&p.id, &resin.profile.id) {
            let _ = writeln!(
                text,
                "{:<38} {:<40} {:>5} mm  {}",
                resin.profile.id,
                resin.profile.name,
                setting.profile.settings.layer_height_mm,
                setting.profile.id
            );
        }
    }
    text
}

fn resins(catalog: &Catalog) -> String {
    let printers_with = |resin: &CatalogMaterial| {
        catalog
            .printers()
            .iter()
            .filter(|p| {
                catalog
                    .settings_for(&p.profile.id, &resin.profile.id)
                    .next()
                    .is_some()
            })
            .count()
    };
    let mut text = format!(
        "Resins of the built-in catalog ({}). PRINTERS counts the printers with a print setting \
         for the resin; a resin without one needs a print setting file of your own.\n\n\
         {:<38} {:<40} {:>8}\n",
        catalog.materials().len(),
        "ID",
        "RESIN",
        "PRINTERS"
    );
    for resin in catalog.materials() {
        let _ = writeln!(
            text,
            "{:<38} {:<40} {:>8}",
            resin.profile.id,
            resin.profile.name,
            printers_with(resin)
        );
    }
    text
}

#[cfg(test)]
mod tests {
    use super::{as_id, parse, source, suggestions, CatalogArgs, Kind, Source};
    use std::path::Path;

    fn catalog() -> karasu_project::library::Catalog {
        super::built_in().unwrap()
    }

    #[test]
    fn only_lowercase_hyphenated_words_read_as_ids() {
        assert_eq!(as_id(Path::new("elegoo-mars-5")), Some("elegoo-mars-5"));
        for path in [
            "printer.json",
            "./elegoo-mars-5",
            "profiles/mars",
            "Mars",
            "",
        ] {
            assert_eq!(as_id(Path::new(path)), None, "{path}");
        }
    }

    #[test]
    fn a_catalog_id_is_used_when_no_file_has_its_name() {
        let catalog = catalog();
        let missing = |_: &Path| false;
        assert_eq!(
            source(Path::new("elegoo-mars-5"), Kind::Printer, &catalog, missing),
            Ok(Source::Catalog("elegoo-mars-5".into()))
        );
        assert_eq!(
            source(Path::new("printer.json"), Kind::Printer, &catalog, missing),
            Ok(Source::File("printer.json".into()))
        );
    }

    #[test]
    fn a_file_named_as_a_catalog_id_is_refused_rather_than_guessed() {
        let catalog = catalog();
        let error = source(Path::new("elegoo-mars-5"), Kind::Printer, &catalog, |_| {
            true
        })
        .unwrap_err();
        assert!(error.contains("./elegoo-mars-5"), "{error}");
        // A file whose name is not a catalog id stays a file.
        assert_eq!(
            source(Path::new("my-printer"), Kind::Printer, &catalog, |_| true),
            Ok(Source::File("my-printer".into()))
        );
    }

    #[test]
    fn an_unknown_id_suggests_close_ones() {
        let catalog = catalog();
        let error = source(Path::new("saturn-4"), Kind::Printer, &catalog, |_| false).unwrap_err();
        assert!(error.contains("elegoo-saturn-4"), "{error}");
        assert!(error.contains("karasu-cli catalog"), "{error}");
        // Ids are looked up in the option's own section.
        assert!(source(Path::new("elegoo-mars-5"), Kind::Resin, &catalog, |_| false).is_err());
    }

    #[test]
    fn suggestions_fall_back_to_shared_words() {
        let ids = ["elegoo-mars-5", "elegoo-mars-5-ultra", "anycubic-photon"];
        assert!(suggestions(&ids, "mars-5-pro").contains("elegoo-mars-5"));
        assert!(!suggestions(&ids, "zzz").contains("did you mean"));
    }

    #[test]
    fn catalog_options_parse() {
        let parsed = |args: &[&str]| parse(args.iter().map(Into::into));
        assert_eq!(parsed(&[]), Ok(CatalogArgs::Printers));
        assert_eq!(parsed(&["--resins"]), Ok(CatalogArgs::Resins));
        assert_eq!(
            parsed(&["--printer", "elegoo-mars-5"]),
            Ok(CatalogArgs::Printer("elegoo-mars-5".into()))
        );
        assert!(parsed(&["--printer"]).is_err());
        assert!(parsed(&["--printer", "--resins"]).is_err());
        assert!(parsed(&["--everything"]).is_err());
    }
}
