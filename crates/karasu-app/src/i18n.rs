//! Interface localisation with Project Fluent (M6.3b, TD-080).
//!
//! **Catalogs:** every interface text is a Fluent message in `assets/locales/<locale>/*.ftl`. Code
//! names messages by identifier through [`tr!`]; plurals, word order and grammar live in the
//! catalogs. `en-US` is the reference: every message exists in it, and a message missing from
//! another language falls back to it.
//!
//! **Sources:** the bundled catalogs are compiled into the executable. `--locale-dir <folder>`
//! adds `<folder>/<locale>/*.ftl` on top of them, overriding messages with the same identifier, so
//! a translator sees a change without rebuilding.
//!
//! **What is not translated:** the command line, the headless crates, and technical details shown
//! as message details (the pipeline's reason, the slice summary, file errors) stay in English.
//!
//! **Numbers:** Fluent selects plurals from numbers but does not apply locale conventions, so
//! decimals are formatted here with the catalog's `number-decimal-separator` ([`decimal`]).
//!
//! **Pseudo-locale:** `--locale pseudo` shows the reference texts with accented letters, about
//! 35 % longer and bracketed, to reveal texts that bypass the catalogs and layouts that cannot
//! take longer translations.
//!
//! **Threads:** the catalog in use belongs to the interface thread ([`install`]); other threads,
//! such as tests, use the reference catalog.
use fluent_bundle::{FluentArgs, FluentBundle, FluentResource, FluentValue};
use std::{borrow::Cow, cell::RefCell, fmt::Write as _, path::Path};
use unic_langid::LanguageIdentifier;

/// The reference locale.
pub const REFERENCE: &str = "en-US";
/// The pseudo-locale.
pub const PSEUDO: &str = "pseudo";
/// Catalogs compiled into the executable: locale and Fluent source.
pub const BUNDLED: &[(&str, &str)] = &[(
    REFERENCE,
    include_str!("../../../assets/locales/en-US/karasu.ftl"),
)];

/// Text with message `id` of the catalog in use, with named arguments.
///
/// `tr!("id")` or `tr!("id", name = value, …)`; a value is a count (an integer, which also
/// selects plurals) or text (anything else, already formatted).
macro_rules! tr {
    ($id:literal) => {
        $crate::i18n::text($id, &[])
    };
    ($id:literal, $($name:ident = $value:expr),+ $(,)?) => {
        $crate::i18n::text(
            $id,
            &[$((stringify!($name), $crate::i18n::Arg::from($value))),+],
        )
    };
}
pub(crate) use tr;

/// A message argument.
#[derive(Clone, Debug, PartialEq)]
pub enum Arg {
    /// A count: shown as an integer and used for plural selection.
    Count(i64),
    /// Text shown as it is.
    Text(String),
}

impl From<usize> for Arg {
    fn from(n: usize) -> Self {
        Self::Count(i64::try_from(n).unwrap_or(i64::MAX))
    }
}

impl From<u64> for Arg {
    fn from(n: u64) -> Self {
        Self::Count(i64::try_from(n).unwrap_or(i64::MAX))
    }
}

impl From<u32> for Arg {
    fn from(n: u32) -> Self {
        Self::Count(i64::from(n))
    }
}

impl From<&str> for Arg {
    fn from(text: &str) -> Self {
        Self::Text(text.to_owned())
    }
}

impl From<String> for Arg {
    fn from(text: String) -> Self {
        Self::Text(text)
    }
}

impl From<&String> for Arg {
    fn from(text: &String) -> Self {
        Self::Text(text.clone())
    }
}

/// The messages of one locale, falling back to the reference.
pub struct Catalog {
    primary: Option<FluentBundle<FluentResource>>,
    reference: FluentBundle<FluentResource>,
    pseudo: bool,
}

/// A new bundle for `locale` (no isolation marks: egui lays out left-to-right text only).
fn bundle(locale: &str) -> Result<FluentBundle<FluentResource>, String> {
    let id: LanguageIdentifier = locale
        .parse()
        .map_err(|_| format!("{locale:?} is not a language tag such as en-US or it-IT"))?;
    let mut bundle = FluentBundle::new(vec![id]);
    bundle.set_use_isolating(false);
    Ok(bundle)
}

/// Parse `source` (named `name` in errors).
fn resource(name: &str, source: String) -> Result<FluentResource, String> {
    FluentResource::try_new(source).map_err(|(_, errors)| {
        let first = errors.first().map(ToString::to_string).unwrap_or_default();
        format!("{name}: {} syntax error(s), first: {first}", errors.len())
    })
}

/// Add the bundled catalog of `locale`, then the files in `dir/<locale>`, overriding.
/// Returns whether any catalog exists for the locale.
fn load(
    bundle: &mut FluentBundle<FluentResource>,
    locale: &str,
    dir: Option<&Path>,
) -> Result<bool, String> {
    let mut found = false;
    for &(_, source) in BUNDLED.iter().filter(|(l, _)| *l == locale) {
        let parsed = resource(&format!("bundled {locale}"), source.to_owned())?;
        bundle.add_resource(parsed).map_err(|errors| {
            format!(
                "bundled {locale}: {} duplicate or invalid message(s)",
                errors.len()
            )
        })?;
        found = true;
    }
    let Some(folder) = dir.map(|d| d.join(locale)).filter(|f| f.is_dir()) else {
        return Ok(found);
    };
    let mut files: Vec<_> = std::fs::read_dir(&folder)
        .map_err(|e| format!("cannot read {}: {e}", folder.display()))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|e| e == "ftl"))
        .collect();
    files.sort();
    for path in files {
        let source = std::fs::read_to_string(&path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        bundle.add_resource_overriding(resource(&path.display().to_string(), source)?);
        found = true;
    }
    Ok(found)
}

/// Accented look-alikes of ASCII letters, for the pseudo-locale; every one has a glyph in the
/// interface font (a test checks it).
fn pseudo(text: &str) -> Cow<'_, str> {
    const FROM: &str = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";
    const TO: &str = "åƀçđéƒĝĥîĵķļмñöþǫŕšţûνŵхýžÅƁÇĐÉƑĜĤÎĴĶĻМÑÖÞǪŔŠŢÛѴŴХÝŽ";
    Cow::Owned(
        text.chars()
            .map(|c| FROM.find(c).and_then(|i| TO.chars().nth(i)).unwrap_or(c))
            .collect(),
    )
}

impl Catalog {
    /// The reference catalog alone.
    ///
    /// # Panics
    /// If the bundled reference catalog is invalid, a defect the tests catch.
    #[must_use]
    pub fn reference() -> Self {
        Self::open(REFERENCE, None).expect("the bundled reference catalog is valid")
    }

    /// The catalog of `locale` (or [`PSEUDO`]), with the files under `dir` added.
    ///
    /// # Errors
    /// A malformed language tag, no catalog for the locale, or an unreadable or invalid file.
    pub fn open(locale: &str, dir: Option<&Path>) -> Result<Self, String> {
        let mut reference = bundle(REFERENCE)?;
        load(&mut reference, REFERENCE, dir)?;
        let is_pseudo = locale == PSEUDO;
        if is_pseudo {
            reference.set_transform(Some(pseudo));
        }
        let primary = if is_pseudo || locale == REFERENCE {
            None
        } else {
            let mut primary = bundle(locale)?;
            if !load(&mut primary, locale, dir)? {
                return Err(format!(
                    "no interface catalog for {locale}; available: {}",
                    available(dir).join(", ")
                ));
            }
            Some(primary)
        };
        Ok(Self {
            primary,
            reference,
            pseudo: is_pseudo,
        })
    }

    /// Message `id` with `args`; its identifier when no catalog has it.
    #[must_use]
    pub fn format(&self, id: &str, args: &[(&str, Arg)]) -> String {
        let mut fluent = FluentArgs::new();
        for (name, value) in args {
            match value {
                Arg::Count(n) => fluent.set(*name, FluentValue::from(*n)),
                Arg::Text(text) => fluent.set(*name, FluentValue::from(text.as_str())),
            }
        }
        let args = (!args.is_empty()).then_some(&fluent);
        for bundle in self.primary.iter().chain(std::iter::once(&self.reference)) {
            let Some(pattern) = bundle.get_message(id).and_then(|m| m.value()) else {
                continue;
            };
            // Formatting errors (a missing argument) still give text naming the argument.
            let mut errors = Vec::new();
            let text = bundle
                .format_pattern(pattern, args, &mut errors)
                .into_owned();
            return if self.pseudo { expand(&text) } else { text };
        }
        id.to_owned()
    }
}

/// The pseudo-locale's frame: about 35 % longer, bracketed so truncation shows.
fn expand(text: &str) -> String {
    let extra = text.chars().count().div_ceil(3);
    let mut out = String::with_capacity(text.len() + extra + 2);
    let _ = write!(out, "[{text}{}]", "~".repeat(extra));
    out
}

/// The locales with a bundled catalog or files under `dir`, sorted, with the pseudo-locale.
#[must_use]
pub fn available(dir: Option<&Path>) -> Vec<String> {
    let mut locales: Vec<String> = BUNDLED.iter().map(|(l, _)| (*l).to_owned()).collect();
    if let Some(entries) = dir.and_then(|d| std::fs::read_dir(d).ok()) {
        for entry in entries.filter_map(Result::ok) {
            if entry.path().is_dir() {
                locales.push(entry.file_name().to_string_lossy().into_owned());
            }
        }
    }
    locales.push(PSEUDO.to_owned());
    locales.sort();
    locales.dedup();
    locales
}

thread_local! {
    static CATALOG: RefCell<Option<Catalog>> = const { RefCell::new(None) };
}

/// Use `catalog` for this thread's texts (the interface thread's, at start-up).
pub fn install(catalog: Catalog) {
    CATALOG.with(|c| *c.borrow_mut() = Some(catalog));
}

/// Message `id` of this thread's catalog; see [`tr!`].
#[must_use]
pub fn text(id: &str, args: &[(&str, Arg)]) -> String {
    CATALOG.with(|c| {
        c.borrow_mut()
            .get_or_insert_with(Catalog::reference)
            .format(id, args)
    })
}

/// `value` with `digits` decimals and the catalog's decimal separator.
#[must_use]
pub fn decimal(value: f64, digits: usize) -> String {
    let separator = text("number-decimal-separator", &[]);
    let separator = separator.trim_matches(['[', ']', '~']);
    format!("{value:.digits$}").replace('.', separator)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fluent_syntax::ast;
    use std::collections::{BTreeMap, BTreeSet};

    /// Each message of `source` and the variables it uses.
    fn variables(source: &str) -> BTreeMap<String, BTreeSet<String>> {
        fn walk(pattern: &ast::Pattern<&str>, out: &mut BTreeSet<String>) {
            for element in &pattern.elements {
                if let ast::PatternElement::Placeable { expression } = element {
                    expression_vars(expression, out);
                }
            }
        }
        fn inline_vars(inline: &ast::InlineExpression<&str>, out: &mut BTreeSet<String>) {
            match inline {
                ast::InlineExpression::VariableReference { id } => {
                    out.insert(id.name.to_owned());
                }
                ast::InlineExpression::Placeable { expression } => expression_vars(expression, out),
                _ => {}
            }
        }
        fn expression_vars(expression: &ast::Expression<&str>, out: &mut BTreeSet<String>) {
            match expression {
                ast::Expression::Inline(inline) => inline_vars(inline, out),
                ast::Expression::Select { selector, variants } => {
                    inline_vars(selector, out);
                    for variant in variants {
                        walk(&variant.value, out);
                    }
                }
            }
        }
        let parsed = fluent_syntax::parser::parse(source).expect("catalog parses");
        let mut messages = BTreeMap::new();
        for entry in parsed.body {
            if let ast::Entry::Message(message) = entry {
                let mut vars = BTreeSet::new();
                if let Some(value) = &message.value {
                    walk(value, &mut vars);
                }
                messages.insert(message.id.name.to_owned(), vars);
            }
        }
        messages
    }

    /// The crate's Rust sources, in its folders too: each file's path below `src` and text.
    fn sources() -> Vec<(String, String)> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut folders = vec![root.clone()];
        let mut found = Vec::new();
        while let Some(folder) = folders.pop() {
            for entry in std::fs::read_dir(folder).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    folders.push(path);
                } else if path.extension().is_some_and(|e| e == "rs") {
                    let name = path.strip_prefix(&root).unwrap().display().to_string();
                    found.push((name, std::fs::read_to_string(&path).unwrap()));
                }
            }
        }
        found.sort();
        found
    }

    /// Every `tr!` use in the crate's sources: identifier and argument names.
    fn uses() -> Vec<(String, BTreeSet<String>)> {
        let mut found = Vec::new();
        for (_, source) in sources() {
            // Skip this module's own documentation and macro definition.
            let source = source.split("#[cfg(test)]").next().unwrap_or_default();
            for (start, _) in source.match_indices("tr!(") {
                // Not the end of a longer macro name such as `include_str!`, nor documentation.
                let line = &source[source[..start].rfind('\n').map_or(0, |i| i + 1)..start];
                if source[..start].ends_with(|c: char| c.is_alphanumeric() || c == '_')
                    || line.trim_start().starts_with("//")
                {
                    continue;
                }
                // The identifier is the first argument, possibly on the next line.
                let Some(rest) = source[start + 4..].trim_start().strip_prefix('"') else {
                    continue;
                };
                let id = &rest[..rest.find('"').unwrap()];
                // The call ends at the parenthesis that closes `tr!(`.
                let mut depth = 1;
                let end = rest
                    .char_indices()
                    .find(|&(_, c)| {
                        depth += match c {
                            '(' => 1,
                            ')' => -1,
                            _ => 0,
                        };
                        depth == 0
                    })
                    .map_or(rest.len(), |(i, _)| i);
                let call = &rest[..end];
                let names = call
                    .split(',')
                    .skip(1)
                    .filter_map(|arg| arg.split_once('='))
                    .map(|(name, _)| name.trim().to_owned())
                    .collect();
                found.push((id.to_owned(), names));
            }
        }
        found
    }

    #[test]
    fn every_used_message_exists_with_its_arguments_and_every_message_is_used() {
        let reference = variables(BUNDLED[0].1);
        let uses = uses();
        assert!(uses.len() > 100, "{} uses", uses.len());
        for (id, names) in &uses {
            let vars = reference
                .get(id)
                .unwrap_or_else(|| panic!("{id} is not in the reference catalog"));
            assert_eq!(names, vars, "arguments of {id}");
        }
        let referenced: BTreeSet<&str> = uses.iter().map(|(id, _)| id.as_str()).collect();
        let unused: Vec<&String> = reference
            .keys()
            .filter(|id| {
                !referenced.contains(id.as_str()) && id.as_str() != "number-decimal-separator"
            })
            .collect();
        assert!(unused.is_empty(), "unused messages: {unused:?}");
    }

    #[test]
    fn every_catalog_parses_and_matches_the_reference_messages_and_variables() {
        let folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/locales");
        let reference = variables(BUNDLED[0].1);
        let mut locales = 0;
        for entry in std::fs::read_dir(&folder).unwrap() {
            let path = entry.unwrap().path();
            let locale = path.file_name().unwrap().to_string_lossy().into_owned();
            let mut messages = BTreeMap::new();
            for file in std::fs::read_dir(&path).unwrap() {
                let source = std::fs::read_to_string(file.unwrap().path()).unwrap();
                messages.extend(variables(&source));
            }
            // A translation may lack messages (they fall back) but never invent ones, and its
            // variables are the reference's.
            for (id, vars) in &messages {
                assert_eq!(reference.get(id), Some(vars), "{locale}: {id}");
            }
            // Every locale folder is bundled.
            assert!(BUNDLED.iter().any(|(l, _)| *l == locale), "{locale}");
            Catalog::open(&locale, None).unwrap();
            locales += 1;
        }
        assert_eq!(locales, BUNDLED.len());
    }

    #[test]
    fn plurals_fallback_overrides_and_the_pseudo_locale() {
        let english = Catalog::reference();
        let one = english.format("finding-inside-out", &[("count", Arg::from(1usize))]);
        let three = english.format("finding-inside-out", &[("count", Arg::from(3usize))]);
        assert_eq!(one, "1 closed body is inside out: its faces point inward");
        assert_eq!(
            three,
            "3 closed bodies are inside out: their faces point inward"
        );
        assert_eq!(english.format("no-such-message", &[]), "no-such-message");
        // A folder overrides one message and adds a language; the rest falls back.
        let dir = std::env::temp_dir().join(format!("karasu-i18n-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("en-US")).unwrap();
        std::fs::create_dir_all(dir.join("it-IT")).unwrap();
        std::fs::write(
            dir.join("en-US/override.ftl"),
            "action-fit-view = Frame all\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("it-IT/karasu.ftl"),
            "action-fit-view = Inquadra tutto\nnumber-decimal-separator = ,\n",
        )
        .unwrap();
        let overridden = Catalog::open(REFERENCE, Some(&dir)).unwrap();
        assert_eq!(overridden.format("action-fit-view", &[]), "Frame all");
        let italian = Catalog::open("it-IT", Some(&dir)).unwrap();
        assert_eq!(italian.format("action-fit-view", &[]), "Inquadra tutto");
        assert_eq!(italian.format("action-open", &[]), "Open");
        assert_eq!(
            available(Some(&dir)),
            ["en-US", "it-IT", "pseudo"].map(String::from)
        );
        assert!(Catalog::open("de-DE", Some(&dir))
            .err()
            .unwrap()
            .contains("no interface catalog for de-DE"));
        assert!(Catalog::open("not a tag", None).is_err());
        install(italian);
        assert_eq!(decimal(0.25, 2), "0,25");
        install(Catalog::reference());
        assert_eq!(decimal(0.25, 2), "0.25");
        let _ = std::fs::remove_dir_all(&dir);
        // Pseudo: accented, longer, bracketed; arguments are not transformed.
        let pseudo = Catalog::open(PSEUDO, None).unwrap();
        let text = pseudo.format("status-objects", &[("count", Arg::from(2usize))]);
        assert!(text.starts_with("[2 ") && text.ends_with("~]"), "{text}");
        assert!(
            !text[1..].contains(|c: char| c.is_ascii_alphabetic()),
            "{text}"
        );
        assert!(
            text.chars().count()
                > english
                    .format("status-objects", &[("count", Arg::from(2usize))])
                    .chars()
                    .count()
        );
    }

    #[test]
    fn the_interface_font_draws_every_catalog_and_pseudo_letter() {
        use eframe::egui::{
            epaint::text::{Fonts, TextOptions},
            FontDefinitions, FontId,
        };
        let mut fonts = Fonts::new(TextOptions::default(), FontDefinitions::default());
        let font = FontId::proportional(14.0);
        let mut text: String = BUNDLED.iter().map(|(_, source)| *source).collect();
        // Symbols written in the code (arrows, marks), outside comments and tests.
        for (_, source) in sources() {
            let code = source.split("#[cfg(test)]").next().unwrap_or_default();
            for line in code.lines().filter(|l| !l.trim_start().starts_with("//")) {
                text.extend(line.chars().filter(|c| !c.is_ascii()));
            }
        }
        text.push_str(&pseudo(
            "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ",
        ));
        let missing: BTreeSet<char> = text
            .chars()
            .filter(|c| !c.is_control() && !fonts.has_glyph(&font, *c))
            .collect();
        assert!(missing.is_empty(), "no glyph for {missing:?}");
    }

    #[test]
    fn interface_calls_take_catalog_texts_not_literals() {
        // A heuristic for single-line calls: widgets and messages given an English literal.
        let patterns = [
            ".label(\"",
            ".button(\"",
            "Button::new(\"",
            "RichText::new(\"",
            "hover_text(\"",
            "menu_button(\"",
            "Window::new(\"",
            "CollapsingHeader::new(\"",
            ".strong(\"",
            ".set_title(\"",
            ".add_filter(\"",
            ".text(\"",
            ", \"",
        ];
        let mut found = Vec::new();
        for (name, source) in sources() {
            let source = source.split("#[cfg(test)]").next().unwrap_or_default();
            for (number, line) in source.lines().enumerate() {
                let code = line.trim_start();
                if code.starts_with("//") {
                    continue;
                }
                for pattern in patterns {
                    let Some(at) = line.find(pattern) else {
                        continue;
                    };
                    let literal = &line[at + pattern.len()..];
                    let interface = pattern != ", \""
                        || line.contains("checkbox(")
                        || line.contains("selectable_label(")
                        || line.contains("Message::new(");
                    if interface && literal.starts_with(|c: char| c.is_ascii_alphabetic()) {
                        found.push(format!("{name}:{}: {}", number + 1, code));
                    }
                }
            }
        }
        assert!(
            found.is_empty(),
            "literal interface texts:\n{}",
            found.join("\n")
        );
    }
}
