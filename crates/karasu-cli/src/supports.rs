//! `slice`'s automatic supports (TD-157): `--auto-supports` plans the supports of the one model
//! as the application's Automatic tool plans them with Generate, `--auto-braces` adds the Braces
//! tool's automatic braces, and `--write-supports` writes the job's support document. The work
//! is `karasu_project::support_plan::generate`'s; this parses the options, reads the value set,
//! runs it before the job's slice and says what it planned in the summary.
use super::slice::{path_value, set, text_value, SliceFailure, INTERRUPTED};
use karasu_core::supports::{SupportDocument, SupportKind};
use karasu_project::{
    pipeline::{Observer, PipelineCache},
    request::SliceRequest,
    support_plan::{
        auto::{Cover, Place, CLEARANCE_MM, DENSITY_PCT},
        generate::{generate, given, Generated, Run},
        islands::Skip,
        settings::AutoSettings,
        values::Weight,
    },
};
use std::{
    ffi::OsString,
    fmt::Write as _,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

/// The largest `--auto-settings` file read, in bytes: an application settings file with many
/// presets is far smaller.
const MAX_SETTINGS_BYTES: u64 = 16 * 1024 * 1024;
/// Places left unsupported listed at most in the summary; the count covers every one.
const LISTED: usize = 20;

/// What the job does with its supports besides reading `--supports`: `Default` when none of the
/// options is given.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct SupportJob {
    /// `--auto-supports` and the values given beside it.
    pub(super) automatic: Option<AutoArgs>,
    /// `--auto-braces`: the Braces tool's automatic braces are added.
    pub(super) braces: bool,
    /// `--auto-settings <file.json>`: the Automatic tool's values, presets and brace values.
    pub(super) settings: Option<PathBuf>,
    /// `--write-supports <file.json>`: where the job's support document is written.
    pub(super) write: Option<PathBuf>,
}

impl SupportJob {
    /// Whether any of the options is given.
    pub(super) fn any(&self) -> bool {
        *self != Self::default()
    }
}

/// `--auto-supports` and the values given beside it, which override the value set's.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct AutoArgs {
    /// `--support-weight`: the preset's weight (a user's preset named by the settings is then
    /// not taken).
    pub(super) weight: Option<Weight>,
    /// `--auto-density`, in percent.
    pub(super) density_pct: Option<f64>,
    /// `--auto-cover`.
    pub(super) cover: Option<Cover>,
    /// `--auto-place`.
    pub(super) place: Option<Place>,
    /// `--auto-minis`.
    pub(super) minis: Option<bool>,
    /// `--auto-clearance`, in mm: the clearance the plan keeps, the contacts' too (TD-164).
    pub(super) clearance_mm: Option<f64>,
    /// `--lift`: the model rests there instead of at the plan's lift.
    pub(super) lift_mm: Option<f64>,
    /// `--overhang-angle`: the critical angle, in degrees.
    pub(super) angle_deg: Option<f64>,
    /// `--accept-support-collisions` without `--supports`, held here: the request has no
    /// supports until they are planned.
    pub(super) accept_collisions: bool,
}

/// The options as given, each at most once.
#[derive(Default)]
pub(super) struct SupportOptions {
    automatic: bool,
    weight: Option<Weight>,
    density_pct: Option<f64>,
    cover: Option<Cover>,
    place: Option<Place>,
    minis: Option<bool>,
    clearance_mm: Option<f64>,
    settings: Option<PathBuf>,
    braces: bool,
    write: Option<PathBuf>,
}

/// What the rest of the command line says that the options depend on.
#[allow(clippy::struct_excessive_bools)] // Each an option given or not.
pub(super) struct Context {
    /// `--plate` was given.
    pub(super) plate: bool,
    /// `--copies` or `--arrange` was given.
    pub(super) copies: bool,
    /// `--supports` was given.
    pub(super) supports: bool,
    /// `--drop` was given.
    pub(super) drop: bool,
    /// `--fill strict` was given.
    pub(super) strict: bool,
}

impl SupportOptions {
    /// Whether `--auto-supports` was given.
    pub(super) fn automatic(&self) -> bool {
        self.automatic
    }

    /// Consume `name` and its value if it is one of these options.
    pub(super) fn apply(
        &mut self,
        name: &str,
        arguments: &mut impl Iterator<Item = OsString>,
    ) -> Result<bool, String> {
        let once = |given: bool| {
            if given {
                Err(format!("{name} may be given only once"))
            } else {
                Ok(true)
            }
        };
        match name {
            "--auto-supports" => self.automatic = once(self.automatic)?,
            "--auto-braces" => self.braces = once(self.braces)?,
            "--support-weight" => {
                let value = text_value(name, arguments.next())?;
                let weight = match value.as_str() {
                    "light" => Weight::Light,
                    "medium" => Weight::Medium,
                    "heavy" => Weight::Heavy,
                    _ => {
                        return Err(format!(
                            "{name} takes light, medium or heavy, not {value:?}"
                        ))
                    }
                };
                set(&mut self.weight, name, weight)?;
            }
            "--auto-density" => {
                let value = text_value(name, arguments.next())?;
                let density = value
                    .parse::<f64>()
                    .ok()
                    .filter(|v| DENSITY_PCT.contains(v))
                    .ok_or_else(|| {
                        format!(
                            "{name} takes a density from {} to {} percent, not {value:?}",
                            DENSITY_PCT.start(),
                            DENSITY_PCT.end()
                        )
                    })?;
                set(&mut self.density_pct, name, density)?;
            }
            "--auto-cover" => {
                let value = text_value(name, arguments.next())?;
                let cover = match value.as_str() {
                    "all" => Cover::All,
                    "islands" => Cover::IslandsOnly,
                    _ => return Err(format!("{name} takes all or islands, not {value:?}")),
                };
                set(&mut self.cover, name, cover)?;
            }
            "--auto-place" => {
                let value = text_value(name, arguments.next())?;
                let place = match value.as_str() {
                    "everywhere" => Place::Everywhere,
                    "outside" => Place::Outside,
                    "cavity" => Place::Cavity,
                    _ => {
                        return Err(format!(
                            "{name} takes everywhere, outside or cavity, not {value:?}"
                        ))
                    }
                };
                set(&mut self.place, name, place)?;
            }
            "--auto-minis" => {
                let value = text_value(name, arguments.next())?;
                let minis = match value.as_str() {
                    "on" => true,
                    "off" => false,
                    _ => return Err(format!("{name} takes on or off, not {value:?}")),
                };
                set(&mut self.minis, name, minis)?;
            }
            "--auto-clearance" => {
                let value = text_value(name, arguments.next())?;
                let clearance = value
                    .parse::<f64>()
                    .ok()
                    .filter(|v| CLEARANCE_MM.contains(v))
                    .ok_or_else(|| {
                        format!(
                            "{name} takes a clearance from {} to {} mm, not {value:?}",
                            CLEARANCE_MM.start(),
                            CLEARANCE_MM.end()
                        )
                    })?;
                set(&mut self.clearance_mm, name, clearance)?;
            }
            "--auto-settings" => {
                set(
                    &mut self.settings,
                    name,
                    path_value(name, arguments.next())?,
                )?;
            }
            "--write-supports" => {
                set(&mut self.write, name, path_value(name, arguments.next())?)?;
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    /// The options checked against each other and the rest of the command line: see the module
    /// and `docs/CLI.md`.
    pub(super) fn check(self, context: &Context) -> Result<SupportJob, String> {
        let values = [
            ("--support-weight", self.weight.is_some()),
            ("--auto-density", self.density_pct.is_some()),
            ("--auto-cover", self.cover.is_some()),
            ("--auto-place", self.place.is_some()),
            ("--auto-minis", self.minis.is_some()),
            ("--auto-clearance", self.clearance_mm.is_some()),
        ];
        if !self.automatic {
            if let Some((name, _)) = values.iter().find(|(_, given)| *given) {
                return Err(format!("{name} is used only with --auto-supports"));
            }
            if self.settings.is_some() && !self.braces {
                return Err(
                    "--auto-settings is used only with --auto-supports or --auto-braces".to_owned(),
                );
            }
        }
        let given = [
            ("--auto-supports", self.automatic),
            ("--auto-braces", self.braces),
            ("--write-supports", self.write.is_some()),
        ];
        let Some((first, _)) = given.iter().find(|(_, given)| *given) else {
            return Ok(SupportJob::default());
        };
        if context.plate {
            return Err(format!(
                "{first} works on one model (--model): a plate document holds each object's \
                 supports"
            ));
        }
        if context.copies {
            return Err(format!(
                "{first} works on one model where it stands: not with --copies or --arrange"
            ));
        }
        if !self.automatic && !context.supports {
            return Err(format!(
                "{first} works on the job's supports: give --auto-supports or --supports"
            ));
        }
        if self.automatic && !context.drop {
            return Err(
                "--auto-supports lifts the model as the Automatic tool does: add --drop (and \
                 --lift <mm> for a height of your own)"
                    .to_owned(),
            );
        }
        if self.automatic && context.strict {
            return Err(
                "--auto-supports needs --fill union: supports and the model merge by winding \
                 union"
                    .to_owned(),
            );
        }
        Ok(SupportJob {
            automatic: self.automatic.then(|| AutoArgs {
                weight: self.weight,
                density_pct: self.density_pct,
                cover: self.cover,
                place: self.place,
                minis: self.minis,
                clearance_mm: self.clearance_mm,
                ..AutoArgs::default()
            }),
            braces: self.braces,
            settings: self.settings,
            write: self.write,
        })
    }
}

/// The job's request with its supports planned, braced or read as `job` asks, the summary's
/// lines about them, and the document to write. `cache` is shared with the job's own slice, so
/// a hollowed model is hollowed once; `observer` gets the progress.
pub(super) fn prepare(
    request: &SliceRequest,
    job: &SupportJob,
    cache: &PipelineCache,
    observer: &mut dyn Observer,
) -> Result<(SliceRequest, String, Option<SupportDocument>), SliceFailure> {
    if let Some(path) = &job.write {
        if path.try_exists().unwrap_or(true) {
            return Err(SliceFailure::Error(format!(
                "--write-supports {}: the file exists; supports are never written over a file",
                path.display()
            )));
        }
    }
    let settings = match &job.settings {
        Some(path) => read_settings(path)?,
        None => AutoSettings::default(),
    };
    let scratch = std::env::temp_dir();
    let run = Run {
        cache: Some(cache),
        scratch: &scratch,
        observer,
        stop: &INTERRUPTED,
    };
    let failed = |message: String| {
        if INTERRUPTED.load(Ordering::Acquire) {
            SliceFailure::Interrupted
        } else {
            SliceFailure::Error(message)
        }
    };
    if let Some(auto) = &job.automatic {
        let mut settings = settings;
        let values = &mut settings.values;
        if let Some(weight) = auto.weight {
            values.weight = weight;
            values.mine = None;
        }
        if let Some(density) = auto.density_pct {
            values.density_pct = density;
        }
        if let Some(cover) = auto.cover {
            values.cover = cover;
        }
        if let Some(place) = auto.place {
            values.place = place;
        }
        if let Some(minis) = auto.minis {
            values.minis = minis;
        }
        if let Some(clearance) = auto.clearance_mm {
            values.clearance_mm = clearance;
        }
        if let Some(lift) = auto.lift_mm {
            values.lift_mm = lift;
        }
        if job.braces {
            values.braces = true;
        }
        if let Some(angle) = auto.angle_deg {
            settings.angle_deg = angle;
        }
        let generated = generate(request, &settings, run).map_err(failed)?;
        let lines = automatic_lines(&generated, &settings);
        let mut request = generated.request.clone();
        if auto.accept_collisions && request.supports.is_some() {
            request.accept_support_collisions = true;
        }
        if generated.document.is_none() && generated.lift_mm > 0.0 {
            return Err(SliceFailure::Error(format!(
                "the automatic supports planned no support, so nothing holds the model {} mm \
                 above the plate:\n{}",
                generated.lift_mm,
                lines.trim_end()
            )));
        }
        return Ok((request, lines, generated.document));
    }
    let braces = job.braces.then_some(&settings.braces);
    let given = given(request, braces, run).map_err(failed)?;
    let lines = if job.braces {
        format!(
            "Automatic braces: {} added between the given supports\n",
            given.braces
        )
    } else {
        String::new()
    };
    Ok((given.request, lines, Some(given.document)))
}

/// The value set `--auto-settings` names.
fn read_settings(path: &Path) -> Result<AutoSettings, SliceFailure> {
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|file| file.take(MAX_SETTINGS_BYTES + 1).read_to_end(&mut bytes))
        .map_err(|e| {
            SliceFailure::Error(format!(
                "cannot read --auto-settings {}: {e}",
                path.display()
            ))
        })?;
    if bytes.len() as u64 > MAX_SETTINGS_BYTES {
        return Err(SliceFailure::Error(format!(
            "--auto-settings {} exceeds {MAX_SETTINGS_BYTES} bytes",
            path.display()
        )));
    }
    AutoSettings::from_json(&bytes)
        .map_err(|e| SliceFailure::Error(format!("--auto-settings {}: {e}", path.display())))
}

/// The summary's lines about `generated`, planned with `settings`, one fact per line.
fn automatic_lines(generated: &Generated, settings: &AutoSettings) -> String {
    let outcome = &generated.outcome;
    let values = &generated.values;
    let supports = &outcome.plan.supports;
    let minis = supports
        .iter()
        .filter(|p| p.routed.support.kind == SupportKind::Mini)
        .count();
    let mut text = format!(
        "Automatic supports: {} (mini {minis}, branches {}), braces {}\n",
        supports.len(),
        outcome.plan.branches,
        outcome.braces.len()
    );
    let preset = values
        .mine
        .and_then(|k| settings.presets.get(k))
        .map_or_else(
            || weight_name(values.weight).to_owned(),
            |p| format!("{:?}", p.name),
        );
    let _ = writeln!(
        text,
        "Automatic supports values: {preset}, density {}%, {}, {}, minis {}, branches {}, \
         clearance {} mm, lift {} mm, critical angle {} degrees",
        values.density_pct,
        match values.cover {
            Cover::All => "islands and overhangs",
            Cover::IslandsOnly => "islands only",
        },
        match values.place {
            Place::Everywhere => "everywhere",
            Place::Outside => "outside",
            Place::Cavity => "cavity only",
        },
        on_off(values.minis),
        on_off(values.branches),
        values.clearance_mm,
        generated.lift_mm,
        generated.angle_deg
    );
    let [held, islands] = outcome.islands;
    let [covered, points] = outcome.overhangs;
    let overhangs = if values.cover == Cover::IslandsOnly {
        "overhangs not covered (islands only)".to_owned()
    } else if points == 0 {
        "no overhang".to_owned()
    } else {
        #[allow(clippy::cast_precision_loss)] // Point counts.
        let share = 100.0 * covered as f64 / points as f64;
        format!("overhangs {covered} of {points} points ({share:.1}%)")
    };
    let _ = writeln!(
        text,
        "Automatic supports held: islands {held} of {islands}, {overhangs}; closing check {} \
         passes, {} tips added",
        outcome.passes, outcome.added
    );
    if generated.kept > 0 || generated.replaced > 0 {
        let _ = writeln!(
            text,
            "Automatic supports kept: {} given, {} automatic replaced",
            generated.kept, generated.replaced
        );
    }
    let _ = writeln!(
        text,
        "Automatic supports left unsupported: {}",
        outcome.left.len()
    );
    for left in outcome.left.iter().take(LISTED) {
        let [x, y, z] = left.point;
        let _ = writeln!(
            text,
            "Left unsupported: {} at (x {x:.3}, y {y:.3}, z {z:.3} mm): {}",
            if left.island { "island" } else { "overhang" },
            why(left.why)
        );
    }
    text
}

/// A weight's name, as the options take it.
fn weight_name(weight: Weight) -> &'static str {
    match weight {
        Weight::Light => "light",
        Weight::Medium => "medium",
        Weight::Heavy => "heavy",
    }
}

fn on_off(on: bool) -> &'static str {
    if on {
        "on"
    } else {
        "off"
    }
}

/// Why a place was left unsupported, as the application's list says it.
fn why(skip: Skip) -> &'static str {
    match skip {
        Skip::NoSurface => "no underside was found there",
        Skip::OutOfReach => "no way down reaches the plate, and feet on the model are off",
        Skip::NoWay => "no way down clear of the model was found",
        Skip::Unbuildable => "its support cannot be built there with the preset",
        Skip::DrainHole => "a drain hole cuts the material away there: nothing to hold",
        Skip::FootInHole => "its support's foot would stand where a drain hole cuts the model away",
        Skip::TooLow => {
            "too close to the plate for the preset: raise --lift, or use a lighter --support-weight"
        }
        Skip::Collides => "its support would come closer to the model than the clearance",
        Skip::Pierces => "its tip would reach through a thin part of the model",
        Skip::Crowded => {
            "its tip, along the surface, would come too close to the model beside it (a wall)"
        }
    }
}

/// Write `document` to `path` as `karasu-supports` JSON, whole or not at all, never over an
/// existing file: staged beside it, then linked into place; where the file system has no hard
/// links, created exclusively and copied into (not atomic there, as the print files' fallback).
pub(super) fn write_document(path: &Path, document: &SupportDocument) -> Result<(), String> {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let quoted = path.display();
    let bytes = document.to_json();
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let name = path.file_name().map_or_else(
        || "supports".to_owned(),
        |n| n.to_string_lossy().into_owned(),
    );
    let stage = parent.join(format!(
        ".{name}.{}-{}.partial",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let staged = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&stage)
        .and_then(|mut file| {
            file.write_all(&bytes)?;
            file.sync_all()
        });
    if let Err(e) = staged {
        let _ = fs::remove_file(&stage);
        return Err(format!("cannot write --write-supports {quoted}: {e}"));
    }
    let linked = fs::hard_link(&stage, path);
    let result = match linked {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Err(format!(
            "--write-supports {quoted}: the file exists; supports are never written over a file"
        )),
        Err(_) => copy_exclusive(&bytes, path)
            .map_err(|e| format!("cannot write --write-supports {quoted}: {e}")),
    };
    let _ = fs::remove_file(&stage);
    result
}

/// `bytes` into a new file at `path`, removed again if they cannot all be written.
fn copy_exclusive(bytes: &[u8], path: &Path) -> io::Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    let written = file.write_all(bytes).and_then(|()| file.sync_all());
    if written.is_err() {
        drop(file);
        // Created exclusively above: this operation's own file.
        let _ = fs::remove_file(path);
    }
    written
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context() -> Context {
        Context {
            plate: false,
            copies: false,
            supports: false,
            drop: true,
            strict: false,
        }
    }

    fn parsed(values: &[&str], context: &Context) -> Result<SupportJob, String> {
        let mut options = SupportOptions::default();
        let mut arguments = values
            .iter()
            .map(OsString::from)
            .collect::<Vec<_>>()
            .into_iter();
        while let Some(name) = arguments.next() {
            let name = name.into_string().unwrap();
            assert!(options.apply(&name, &mut arguments)?, "{name}");
        }
        options.check(context)
    }

    #[test]
    fn values_are_read_and_misuse_is_refused() {
        let job = parsed(
            &[
                "--auto-supports",
                "--support-weight",
                "heavy",
                "--auto-density",
                "150",
                "--auto-cover",
                "islands",
                "--auto-place",
                "outside",
                "--auto-minis",
                "off",
                "--auto-clearance",
                "0.4",
                "--auto-braces",
            ],
            &context(),
        )
        .unwrap();
        let auto = job.automatic.unwrap();
        assert_eq!(auto.weight, Some(Weight::Heavy));
        assert_eq!(auto.density_pct, Some(150.0));
        assert_eq!(auto.cover, Some(Cover::IslandsOnly));
        assert_eq!(auto.place, Some(Place::Outside));
        assert_eq!(auto.minis, Some(false));
        assert_eq!(auto.clearance_mm, Some(0.4));
        assert!(job.braces);
        assert_eq!(parsed(&[], &context()).unwrap(), SupportJob::default());
        let refused = |values: &[&str], context: &Context, says: &str| {
            let error = parsed(values, context).unwrap_err();
            assert!(error.contains(says), "{error}");
        };
        refused(
            &["--auto-density", "120"],
            &context(),
            "only with --auto-supports",
        );
        refused(
            &["--auto-settings", "s.json"],
            &context(),
            "only with --auto-supports",
        );
        refused(
            &["--auto-supports", "--auto-density", "20"],
            &context(),
            "from 50 to 200",
        );
        refused(
            &["--auto-supports", "--auto-clearance", "6"],
            &context(),
            "from 0 to 5 mm",
        );
        refused(
            &["--auto-clearance", "0.3"],
            &context(),
            "only with --auto-supports",
        );
        refused(
            &["--auto-supports", "--support-weight", "x"],
            &context(),
            "light",
        );
        refused(
            &["--auto-supports", "--auto-supports"],
            &context(),
            "only once",
        );
        refused(
            &["--auto-braces"],
            &context(),
            "give --auto-supports or --supports",
        );
        refused(&["--write-supports", "w.json"], &context(), "--supports");
        let plate = Context {
            plate: true,
            ..context()
        };
        refused(&["--auto-supports"], &plate, "one model");
        let copies = Context {
            copies: true,
            ..context()
        };
        refused(&["--auto-supports"], &copies, "--copies or --arrange");
        let floating = Context {
            drop: false,
            ..context()
        };
        refused(&["--auto-supports"], &floating, "add --drop");
        let strict = Context {
            strict: true,
            ..context()
        };
        refused(&["--auto-supports"], &strict, "--fill union");
        // With supports given, braces and writing need no planning.
        let given = Context {
            supports: true,
            ..context()
        };
        let job = parsed(&["--auto-braces", "--write-supports", "w.json"], &given).unwrap();
        assert!(job.automatic.is_none() && job.braces && job.write.is_some());
    }
}
