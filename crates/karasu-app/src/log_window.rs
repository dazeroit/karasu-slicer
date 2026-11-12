//! The live log window (TD-096, M6.5): the session's recent records as they come, like a
//! terminal, filtered by level, area and text; and the diagnostics a user sends with a problem:
//! the logs folder, and a diagnostic report.
//!
//! **Cost.** While the window is open it asks the logger's ring only for the records after the
//! last one it has ([`karasu_project::logging::Ring::since`]), four times a second, and draws only the rows in view, so
//! ten thousand records cost no more than a screenful. Closed, it reads nothing.
//!
//! **The report** is plain text the user saves and sends: the application, the system, the
//! graphics adapter, then the session's log file (or the records in memory when there is no
//! file). It can hide the home folder, where the user's name appears in paths.
use crate::{
    dialog::{remember_tool_window, tool_window},
    gpu,
    i18n::tr,
    messages::{Level as MessageLevel, Message},
    theme,
    widgets::{switch, text_field},
};
use eframe::egui;
use karasu_project::logging::{LogHandle, Record, RING_RECORDS};
use log::Level;
use std::{
    collections::{BTreeSet, VecDeque},
    fmt::Write as _,
};

/// How often an open window looks for new records.
const POLL: std::time::Duration = std::time::Duration::from_millis(250);

/// Which levels the window shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Shown {
    /// Every record kept.
    #[default]
    Everything,
    /// Warnings and errors.
    Warnings,
    /// Errors only.
    Errors,
}

impl Shown {
    fn keeps(self, level: Level) -> bool {
        match self {
            Self::Everything => true,
            Self::Warnings => level <= Level::Warn,
            Self::Errors => level == Level::Error,
        }
    }

    fn label(self) -> String {
        match self {
            Self::Everything => tr!("log-shown-everything"),
            Self::Warnings => tr!("log-shown-warnings"),
            Self::Errors => tr!("log-shown-errors"),
        }
    }
}

/// One row shown: a line of a record, by the record's place since the window started (so rows
/// stay valid as the oldest records are let go).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Row {
    record: usize,
    line: usize,
}

/// The records the window has, and the rows its filters show.
#[derive(Debug, Default)]
pub struct LogView {
    records: VecDeque<Record>,
    /// How many records were let go from the front.
    base: usize,
    /// The next record's sequence number to ask for.
    next: u64,
    /// Records before this sequence number are hidden ("Clear the view").
    cleared: u64,
    rows: Vec<Row>,
    /// The areas seen, for the area menu.
    areas: BTreeSet<String>,
    shown: Shown,
    area: Option<String>,
    search: String,
}

impl LogView {
    /// Take `records` (the ring's, oldest first), keeping at most as many as the ring.
    pub fn add(&mut self, records: Vec<Record>) {
        for record in records {
            self.next = record.sequence + 1;
            self.areas.insert(record.target.clone());
            let index = self.base + self.records.len();
            if self.matches(&record) {
                let lines = record.message.lines().count().max(1);
                self.rows.extend((0..lines).map(|line| Row {
                    record: index,
                    line,
                }));
            }
            self.records.push_back(record);
        }
        let excess = self.records.len().saturating_sub(RING_RECORDS);
        if excess > 0 {
            self.records.drain(..excess);
            self.base += excess;
            let first = self.rows.partition_point(|row| row.record < self.base);
            self.rows.drain(..first);
        }
    }

    /// The sequence number to ask the ring for next.
    #[must_use]
    pub fn next(&self) -> u64 {
        self.next
    }

    /// Show `shown`, of `area` (all with `None`), containing `search`.
    pub fn filter(&mut self, shown: Shown, area: Option<String>, search: &str) {
        let search = search.trim().to_lowercase();
        if (shown, &area, &search) == (self.shown, &self.area, &self.search) {
            return;
        }
        self.shown = shown;
        self.area = area;
        self.search = search;
        self.rebuild();
    }

    /// Hide every record so far.
    pub fn clear(&mut self) {
        self.cleared = self.next;
        self.rebuild();
    }

    fn rebuild(&mut self) {
        self.rows.clear();
        for (k, record) in self.records.iter().enumerate() {
            if self.matches(record) {
                let lines = record.message.lines().count().max(1);
                self.rows.extend((0..lines).map(|line| Row {
                    record: self.base + k,
                    line,
                }));
            }
        }
    }

    fn matches(&self, record: &Record) -> bool {
        record.sequence >= self.cleared
            && self.shown.keeps(record.level)
            && self.area.as_ref().is_none_or(|a| *a == record.target)
            && (self.search.is_empty()
                || record.target.to_lowercase().contains(&self.search)
                || record.message.to_lowercase().contains(&self.search))
    }

    /// How many rows the filters show.
    #[must_use]
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Whether the filters show nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// Row `k`'s text and level: a record's first line with its time since the start, level and
    /// area; its other lines indented under it.
    #[must_use]
    pub fn row(&self, k: usize) -> Option<(String, Level)> {
        let row = self.rows.get(k)?;
        let record = self.records.get(row.record - self.base)?;
        let line = record.message.lines().nth(row.line).unwrap_or("");
        let text = if row.line == 0 {
            format!(
                "+{:>5}.{:03}s {:<5} {}: {line}",
                record.elapsed_ms / 1000,
                record.elapsed_ms % 1000,
                record.level,
                record.target
            )
        } else {
            format!("{:28}{line}", "")
        };
        Some((text, record.level))
    }

    /// Every row the filters show, one per line, to copy.
    #[must_use]
    pub fn text(&self) -> String {
        (0..self.len())
            .filter_map(|k| self.row(k).map(|(text, _)| text))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Every record the window has, as the log file writes them.
    #[must_use]
    pub fn records_text(&self) -> String {
        self.records
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The areas seen so far.
    pub fn areas(&self) -> impl Iterator<Item = &String> {
        self.areas.iter()
    }
}

/// A diagnostic report: the `facts` as `name: value` lines, then the session's `log`; with
/// `home`, every occurrence of the home folder becomes `~`, so the user's name does not show.
#[must_use]
pub fn report(facts: &[(String, String)], log: &str, home: Option<&str>) -> String {
    let mut text = format!("{} diagnostic report\n\n", karasu_core::APPLICATION_NAME);
    for (name, value) in facts {
        let _ = writeln!(text, "{name}: {value}");
    }
    text.push_str("\nSession log\n\n");
    text.push_str(log);
    if !text.ends_with('\n') {
        text.push('\n');
    }
    match home.filter(|h| h.len() > 1) {
        Some(home) => {
            let mut hidden = text.replace(home, "~");
            // The same folder written with the other separator.
            let other = if home.contains('\\') {
                home.replace('\\', "/")
            } else {
                home.replace('/', "\\")
            };
            if other != home {
                hidden = hidden.replace(&other, "~");
            }
            hidden
        }
        None => text,
    }
}

/// The window and what it shows.
#[derive(Debug, Default)]
pub struct LogWindow {
    open: bool,
    view: LogView,
    /// The logger's file and ring, when one was installed.
    handle: Option<LogHandle>,
    shown: Shown,
    area: Option<String>,
    search: String,
    /// The view follows new records, scrolled to the end.
    follow: bool,
    /// The report hides the home folder.
    hide_paths: bool,
}

impl LogWindow {
    /// A window over the logger `handle` (none: the window says the log is not kept).
    #[must_use]
    pub fn new(handle: Option<LogHandle>) -> Self {
        Self {
            handle,
            follow: true,
            hide_paths: true,
            ..Self::default()
        }
    }

    /// Open the window, or close it (Windows menu).
    pub fn toggle(&mut self) {
        self.open = !self.open;
    }

    /// The window while it is open; a notification for what its actions did. `adapter` is the
    /// graphics adapter, for the report.
    pub fn window(
        &mut self,
        ctx: &egui::Context,
        adapter: Option<&gpu::Selected>,
    ) -> Option<Message> {
        if !self.open {
            return None;
        }
        if let Some(handle) = &self.handle {
            let records = handle.ring().since(self.view.next());
            self.view.add(records);
        }
        ctx.request_repaint_after(POLL);
        let mut open = true;
        let mut outcome = None;
        tool_window(
            ctx,
            "log-window",
            tr!("log-title"),
            egui::vec2(900.0, 440.0),
        )
        .open(&mut open)
        .show(ctx, |ui| {
            remember_tool_window(ui, "log-window");
            self.toolbar(ui);
            ui.separator();
            self.rows(ui);
            ui.separator();
            outcome = self.diagnostics(ui, adapter);
        });
        self.open = open;
        outcome
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            let shown = egui::ComboBox::from_id_salt("log-shown")
                .selected_text(self.shown.label())
                .show_ui(ui, |ui| {
                    for shown in [Shown::Everything, Shown::Warnings, Shown::Errors] {
                        ui.selectable_value(&mut self.shown, shown, shown.label());
                    }
                });
            crate::widgets::name(&shown.response, &tr!("a11y-log-shown"));
            let area = self.area.clone().unwrap_or_else(|| tr!("log-all-areas"));
            let areas = egui::ComboBox::from_id_salt("log-area")
                .selected_text(area)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.area, None, tr!("log-all-areas"));
                    for area in self.view.areas() {
                        ui.selectable_value(&mut self.area, Some(area.clone()), area);
                    }
                });
            crate::widgets::name(&areas.response, &tr!("a11y-log-area"));
            let search = ui.add(text_field(&mut self.search, 200.0).hint_text(tr!("log-search")));
            crate::widgets::name(&search, &tr!("log-search"));
            let follow = switch(ui, &mut self.follow).on_hover_text(tr!("log-follow-hint"));
            crate::widgets::name(&follow, &tr!("log-follow"));
            ui.label(tr!("log-follow"));
            if ui.button(tr!("log-copy")).clicked() {
                ui.ctx().copy_text(self.view.text());
            }
            if ui
                .button(tr!("log-clear"))
                .on_hover_text(tr!("log-clear-hint"))
                .clicked()
            {
                self.view.clear();
            }
        });
        self.view
            .filter(self.shown, self.area.clone(), &self.search);
    }

    /// The rows in view only: ten thousand records cost a screenful.
    fn rows(&mut self, ui: &mut egui::Ui) {
        let font = egui::TextStyle::Monospace.resolve(ui.style());
        let height = ui.fonts_mut(|f| f.row_height(&font));
        let footer = 2.0 * ui.spacing().interact_size.y + 12.0;
        let weak = ui.visuals().weak_text_color();
        let text = ui.visuals().text_color();
        if self.handle.is_none() {
            ui.label(tr!("log-not-kept"));
        } else if self.view.is_empty() {
            ui.label(egui::RichText::new(tr!("log-empty")).color(weak));
        }
        egui::ScrollArea::both()
            .id_salt("log-rows")
            .auto_shrink([false, false])
            .max_height((ui.available_height() - footer).max(80.0))
            .stick_to_bottom(self.follow)
            .show_rows(ui, height, self.view.len(), |ui, range| {
                for k in range {
                    let Some((line, level)) = self.view.row(k) else {
                        continue;
                    };
                    let colour = match level {
                        Level::Error => theme::ERROR.color(),
                        Level::Warn => theme::WARNING.color(),
                        Level::Info => text,
                        Level::Debug | Level::Trace => weak,
                    };
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(line).font(font.clone()).color(colour),
                        )
                        .extend()
                        .selectable(true),
                    );
                }
            });
    }

    /// The logs folder and the diagnostic report.
    fn diagnostics(
        &mut self,
        ui: &mut egui::Ui,
        adapter: Option<&gpu::Selected>,
    ) -> Option<Message> {
        let mut outcome = None;
        let file = self
            .handle
            .as_ref()
            .and_then(|h| h.path())
            .map(ToOwned::to_owned);
        ui.horizontal_wrapped(|ui| {
            let folder = file
                .as_ref()
                .and_then(|f| f.parent())
                .map(ToOwned::to_owned);
            if ui
                .add_enabled(folder.is_some(), egui::Button::new(tr!("log-open-folder")))
                .clicked()
            {
                if let Some(Err(error)) = folder.map(|f| crate::desktop::open_folder(&f)) {
                    outcome = Some(
                        Message::new(MessageLevel::Warning, tr!("settings-file-open-failed"))
                            .detail(error.to_string()),
                    );
                }
            }
            if ui.button(tr!("log-report")).clicked() {
                outcome = self.save_report(file.as_deref(), adapter);
            }
            ui.checkbox(&mut self.hide_paths, tr!("log-hide-paths"))
                .on_hover_text(tr!("log-hide-paths-hint"));
            if let Some(file) = &file {
                ui.label(
                    egui::RichText::new(file.display().to_string())
                        .small()
                        .color(ui.visuals().weak_text_color()),
                );
            }
        });
        outcome
    }

    /// Ask where to save the report, and save it; what happened, as a notification.
    fn save_report(
        &self,
        file: Option<&std::path::Path>,
        adapter: Option<&gpu::Selected>,
    ) -> Option<Message> {
        // What is buffered reaches the file first.
        log::logger().flush();
        let log = file.and_then(|f| std::fs::read(f).ok()).map_or_else(
            || self.view.records_text(),
            |bytes| String::from_utf8_lossy(&bytes).into_owned(),
        );
        let home = std::env::var(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).ok();
        let text = report(
            &facts(adapter, file),
            &log,
            home.as_deref().filter(|_| self.hide_paths),
        );
        let now = karasu_project::logging::utc(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(0)),
        );
        let name = format!("karasu-diagnostic-{}.txt", now[..19].replace(':', "-"));
        let path = rfd::FileDialog::new()
            .set_title(tr!("log-report-title"))
            .add_filter(tr!("log-report-filter"), &["txt"])
            .set_file_name(name)
            .save_file()?;
        Some(match std::fs::write(&path, text) {
            Ok(()) => Message::new(
                MessageLevel::Success,
                tr!("log-report-saved", path = path.display().to_string()),
            ),
            Err(error) => Message::new(MessageLevel::Error, tr!("log-report-failed"))
                .detail(error.to_string()),
        })
    }
}

/// What the report says of the application, the system and the graphics adapter.
fn facts(adapter: Option<&gpu::Selected>, file: Option<&std::path::Path>) -> Vec<(String, String)> {
    use sysinfo::{MemoryRefreshKind, RefreshKind, System};
    let memory = System::new_with_specifics(
        RefreshKind::nothing().with_memory(MemoryRefreshKind::nothing().with_ram()),
    )
    .total_memory();
    #[allow(clippy::cast_precision_loss)] // Gigabytes to one decimal.
    let gigabytes = memory as f64 / 1e9;
    let mut facts = vec![
        (
            "Application".to_owned(),
            format!(
                "{} {} ({})",
                karasu_core::APPLICATION_NAME,
                env!("CARGO_PKG_VERSION"),
                if cfg!(debug_assertions) {
                    "debug"
                } else {
                    "release"
                }
            ),
        ),
        (
            "System".to_owned(),
            format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
        ),
        (
            "Threads".to_owned(),
            std::thread::available_parallelism()
                .map_or(1, usize::from)
                .to_string(),
        ),
        ("Memory".to_owned(), format!("{gigabytes:.1} GB")),
    ];
    if let Some(a) = adapter {
        facts.push((
            "Graphics".to_owned(),
            format!(
                "{} on {} ({}), driver {}",
                a.backend, a.adapter, a.device_type, a.driver
            ),
        ));
    }
    facts.push((
        "Log file".to_owned(),
        file.map_or_else(
            || "none (memory only)".to_owned(),
            |f| f.display().to_string(),
        ),
    ));
    facts
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(sequence: u64, level: Level, target: &str, message: &str) -> Record {
        Record {
            sequence,
            unix_ms: 1_793_000_000_000,
            elapsed_ms: 1234 + sequence,
            level,
            target: target.to_owned(),
            message: message.to_owned(),
        }
    }

    #[test]
    fn the_view_follows_new_records_and_filters_them() {
        let mut view = LogView::default();
        view.add(vec![
            record(0, Level::Info, "app", "opened box.stl"),
            record(1, Level::Warn, "profiles", "catalog update\nfor Photon P1"),
            record(2, Level::Error, "panic", "boom"),
        ]);
        assert_eq!(view.next(), 3);
        // A record's other lines follow it, indented.
        assert_eq!(view.len(), 4);
        let (first, level) = view.row(1).unwrap();
        assert_eq!(level, Level::Warn);
        assert!(
            first.starts_with("+    1.235s WARN  profiles: catalog update"),
            "{first}"
        );
        assert!(view.row(2).unwrap().0.ends_with("for Photon P1"));
        view.filter(Shown::Warnings, None, "");
        assert_eq!(view.len(), 3);
        view.filter(Shown::Everything, Some("app".into()), "");
        assert_eq!(view.len(), 1);
        view.filter(Shown::Everything, None, "PHOTON");
        assert_eq!(view.len(), 2);
        // New records join the rows the filters show.
        view.add(vec![record(3, Level::Info, "app", "Photon chosen")]);
        assert_eq!(view.len(), 3);
        assert_eq!(view.areas().count(), 3);
        // Clearing hides what is there, not what comes.
        view.filter(Shown::Everything, None, "");
        view.clear();
        assert!(view.is_empty());
        view.add(vec![record(4, Level::Info, "app", "after")]);
        assert_eq!(view.len(), 1);
    }

    #[test]
    fn the_view_keeps_as_many_records_as_the_ring() {
        let mut view = LogView::default();
        let total = RING_RECORDS + 25;
        view.add(
            (0..total as u64)
                .map(|k| record(k, Level::Info, "app", &format!("record {k}")))
                .collect(),
        );
        assert_eq!(view.len(), RING_RECORDS);
        assert!(view.row(0).unwrap().0.ends_with("record 25"));
        assert!(view
            .row(RING_RECORDS - 1)
            .unwrap()
            .0
            .ends_with(&format!("record {}", total - 1)));
    }

    #[test]
    fn the_window_draws_ten_thousand_records_inside_it() {
        let mut window = LogWindow::new(None);
        window.open = true;
        window.view.add(
            (0..RING_RECORDS as u64)
                .map(|k| {
                    record(
                        k,
                        Level::Info,
                        "app",
                        &format!("a long record {k} {}", "x".repeat(200)),
                    )
                })
                .collect(),
        );
        let ctx = egui::Context::default();
        let mut shapes = 0;
        for _ in 0..3 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1600.0, 1000.0),
                )),
                ..egui::RawInput::default()
            };
            let mut output = ctx.run_ui(input, |ui| {
                assert!(window.window(ui.ctx(), None).is_none());
            });
            output.textures_delta.clear();
            shapes = output.shapes.len();
        }
        let area = ctx
            .memory(|m| m.area_rect(egui::Id::new("log-window")))
            .unwrap();
        assert!(
            area.width() <= 900.0 + 40.0 && area.height() <= 440.0 + 80.0,
            "{area:?}"
        );
        // Only the rows in view are drawn: far fewer shapes than records.
        assert!(shapes < 400, "{shapes}");
    }

    #[test]
    fn a_report_lists_the_facts_then_the_log_and_can_hide_the_home_folder() {
        let facts = vec![("System".to_owned(), "windows x86_64".to_owned())];
        let log = "settings C:\\Users\\ivan\\AppData\\karasu\nread C:/Users/ivan/model.stl";
        let shown = report(&facts, log, None);
        assert!(shown.starts_with("karasu-slicer diagnostic report"));
        assert!(shown.contains("System: windows x86_64\n"));
        assert!(shown.contains("C:\\Users\\ivan\\AppData"));
        let hidden = report(&facts, log, Some("C:\\Users\\ivan"));
        assert!(!hidden.contains("ivan"), "{hidden}");
        assert!(hidden.contains("~\\AppData") && hidden.contains("~/model.stl"));
    }
}
