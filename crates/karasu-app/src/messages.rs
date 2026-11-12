//! Messages for the user (M6.3): the latest as one short line in the status bar, all of them in
//! the Messages window with their details and the fixes they offer. Since M6.5 every message
//! also shows as a toast at the bottom of the viewport ([`crate::toasts`]), for as long as its
//! [`Lasting`] says.
//!
//! **Wording:** messages use the interface's words from the catalogs ([`crate::i18n`]), never
//! command-line options. A failed slice names its cause briefly; the pipeline's full reason, in
//! English, is the message's detail.
//!
//! **Fixes** change plate settings the user can also change in the Repair menu. A fix is applied
//! only when the user clicks it; nothing is repaired silently. A fix offered by a failed slice
//! also slices again.
use crate::{
    i18n::tr,
    icons::Icons,
    theme,
    toasts::{Severity, Toasts},
    workspace::Workspace,
};
use eframe::egui;
use karasu_core::preparation::repair::{FillHoles, RepairPreset};
use karasu_project::{
    diagnosis::{Diagnosis, Finding, Handling, Treatment},
    pipeline::Remedy,
    project::PlateSettings,
    request::{DEFAULT_CLOSE_HOLES_MM, DEFAULT_REVERSED_TOLERANCE_MM2},
};
use std::time::Duration;

/// One short message of the status bar's line (M6.5): what is happening or wrong now.
#[derive(Clone, Debug, PartialEq)]
pub struct Status {
    pub text: String,
    pub colour: egui::Color32,
    /// More, shown when the pointer rests on the line.
    pub detail: Option<String>,
    /// Work in progress: the line shows a spinner.
    pub busy: bool,
}

impl Status {
    /// A short message in `colour`, without detail.
    #[must_use]
    pub fn new(text: String, colour: egui::Color32) -> Self {
        Self {
            text,
            colour,
            detail: None,
            busy: false,
        }
    }
}

/// How serious a message is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    /// Something happened as asked.
    Info,
    /// Something asked for succeeded (a slice, an export, a verification).
    Success,
    /// Something may need attention.
    Warning,
    /// Something failed.
    Error,
}

impl Level {
    /// The level as a notification's severity: its name and icon.
    fn severity(self) -> Severity {
        match self {
            Self::Info => Severity::Info,
            Self::Success => Severity::Success,
            Self::Warning => Severity::Warning,
            Self::Error => Severity::Error,
        }
    }

    fn color(self) -> egui::Color32 {
        match self {
            Self::Info => theme::ACCENT.color(),
            Self::Success => theme::SUCCESS.color(),
            Self::Warning => theme::WARNING.color(),
            Self::Error => theme::ERROR.color(),
        }
    }
}

/// A plate setting change offered by a message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fix {
    /// Turn inside-out bodies outward.
    TurnOutward,
    /// Make face orientation consistent.
    Orientation,
    /// Merge duplicate faces.
    Duplicates,
    /// Close holes in each layer.
    CloseHoles,
    /// Leave small reversed slivers empty in each layer.
    TolerateReversed,
    /// Fill the model's holes.
    FillHoles,
    /// Slice despite mesh defects.
    AllowDefects,
    /// Rest every lifted object on the plate.
    DropToPlate,
    /// Repair every object at least as strongly as this preset (TD-092 r8).
    Repair(RepairPreset),
}

impl Fix {
    /// The fix for a pipeline remedy, when the interface offers one.
    #[must_use]
    pub fn for_remedy(remedy: Remedy) -> Option<Self> {
        Some(match remedy {
            Remedy::RepairOutward => Self::TurnOutward,
            Remedy::RepairOrientation => Self::Orientation,
            Remedy::FillHoles => Self::FillHoles,
            Remedy::CloseHoles => Self::CloseHoles,
            Remedy::TolerateReversed => Self::TolerateReversed,
            Remedy::AllowTopologyDefects => Self::AllowDefects,
            Remedy::RestOnPlate => Self::DropToPlate,
            // Moving objects is done by hand; the interface fills by union only and gives units
            // itself.
            _ => return None,
        })
    }

    /// The fix for a diagnosis finding, when one exists.
    #[must_use]
    pub fn for_finding(finding: Finding) -> Option<Self> {
        match finding {
            Finding::InsideOut { .. } => Some(Self::TurnOutward),
            Finding::InconsistentOrientation { .. } => Some(Self::Orientation),
            Finding::DuplicateFaces { .. } => Some(Self::Duplicates),
            Finding::Holes { .. } => Some(Self::CloseHoles),
            Finding::DroppedFacets { .. } => Some(Self::Repair(RepairPreset::Exact)),
            _ => None,
        }
    }

    /// The button's text.
    #[must_use]
    pub fn label(self) -> String {
        match self {
            Self::TurnOutward => tr!("repair-outward"),
            Self::Orientation => tr!("repair-orientation"),
            Self::Duplicates => tr!("repair-duplicates"),
            Self::CloseHoles => tr!("fix-close-holes"),
            Self::TolerateReversed => tr!("fix-tolerate-reversed"),
            Self::FillHoles => tr!("repair-fill-holes"),
            Self::AllowDefects => tr!("repair-allow-defects"),
            Self::DropToPlate => tr!("fix-drop-to-plate"),
            Self::Repair(preset) => tr!(
                "fix-repair",
                preset = crate::repair_panel::preset_name(Some(preset))
            ),
        }
    }

    /// Whether the plate already has this setting.
    #[must_use]
    pub fn applied(self, workspace: &mut Workspace) -> bool {
        let s = &workspace.plate().settings;
        match self {
            Self::TurnOutward => s.repair.outward,
            Self::Orientation => s.repair.orientation,
            Self::Duplicates => s.repair.duplicates,
            Self::CloseHoles => s.close_holes_mm.is_some(),
            Self::TolerateReversed => s.reversed_tolerance_mm2.is_some(),
            Self::FillHoles => s.repair.fill_holes.is_some(),
            Self::AllowDefects => s.allow_topology_defects,
            Self::DropToPlate => workspace.floating().is_empty(),
            Self::Repair(preset) => {
                let target = crate::repair_panel::rank(Some(preset.spec()));
                let non_finite = preset.spec().non_finite;
                workspace.plate().objects.iter().all(|o| {
                    crate::repair_panel::rank(o.repair()) >= target
                        && (!non_finite || o.repair().is_some_and(|r| r.non_finite))
                })
            }
        }
    }

    /// Change the plate's settings (or objects) as the fix says.
    pub fn apply(self, workspace: &mut Workspace) {
        if self == Self::DropToPlate {
            for index in workspace.floating() {
                workspace.drop_to_plate(index);
            }
            return;
        }
        if let Self::Repair(preset) = self {
            let target = crate::repair_panel::rank(Some(preset.spec()));
            workspace.edit(|plate| {
                for object in &mut plate.objects {
                    match object.repair() {
                        repair if crate::repair_panel::rank(repair) < target => {
                            object.set_repair(Some(preset.spec()));
                        }
                        // A custom repair as strong keeps its stages and gains the preset's
                        // acceptance of facets without a position (TD-092 r9).
                        Some(repair) if preset.spec().non_finite && !repair.non_finite => {
                            object.set_repair(Some(karasu_core::preparation::repair::RepairSpec {
                                non_finite: true,
                                ..repair
                            }));
                        }
                        _ => {}
                    }
                }
            });
            return;
        }
        workspace.edit(|plate| apply_setting(self, &mut plate.settings));
    }
}

/// The object repairs that resolve a failed slice's cause (TD-092 r8): holes, orientation and
/// fins by the standard preset, offered first; reversed slivers (of folds that cross themselves)
/// by rebuilding the solid, the last resort, offered `last`, after the plate's settings.
fn repair_fixes(remedies: &[Remedy], last: bool) -> Vec<Fix> {
    let mut fixes = Vec::new();
    if !last
        && remedies.iter().any(|r| {
            matches!(
                r,
                Remedy::RepairOrientation
                    | Remedy::FillHoles
                    | Remedy::CloseHoles
                    | Remedy::AllowTopologyDefects
                    | Remedy::StrictFill
                    | Remedy::RepairDefects
            )
        })
    {
        fixes.push(Fix::Repair(RepairPreset::Standard));
    }
    if !last && remedies.contains(&Remedy::DropNonFinite) {
        fixes.push(Fix::Repair(RepairPreset::Exact));
    }
    if last && remedies.contains(&Remedy::TolerateReversed) {
        fixes.push(Fix::Repair(RepairPreset::Rebuild));
    }
    fixes
}

fn apply_setting(fix: Fix, s: &mut PlateSettings) {
    match fix {
        Fix::TurnOutward => {
            // Outward orientation works on consistently oriented bodies.
            s.repair.orientation = true;
            s.repair.outward = true;
        }
        Fix::Orientation => s.repair.orientation = true,
        Fix::Duplicates => s.repair.duplicates = true,
        Fix::CloseHoles => {
            s.close_holes_mm.get_or_insert(DEFAULT_CLOSE_HOLES_MM);
        }
        Fix::FillHoles => {
            s.repair.fill_holes.get_or_insert_with(FillHoles::default);
        }
        Fix::TolerateReversed => {
            s.reversed_tolerance_mm2
                .get_or_insert(DEFAULT_REVERSED_TOLERANCE_MM2);
        }
        Fix::AllowDefects => s.allow_topology_defects = true,
        Fix::DropToPlate | Fix::Repair(_) => {}
    }
}

/// A message for the user.
#[derive(Clone, Debug, PartialEq)]
pub struct Message {
    /// How serious it is.
    pub level: Level,
    /// One short line.
    pub text: String,
    /// The full explanation, when there is more to say.
    pub detail: Option<String>,
    /// Setting changes that may help.
    pub fixes: Vec<Fix>,
    /// Whether applying a fix slices the plate again (the message reports a failed slice).
    pub reslice: bool,
    /// How long its toast shows.
    pub lasting: Lasting,
}

/// How long a message's toast shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lasting {
    /// As long as its level's default: errors until closed, others a few seconds.
    Default,
    /// This long.
    For(Duration),
    /// Until closed.
    UntilClosed,
}

impl Message {
    /// A message without detail or fixes.
    #[must_use]
    pub fn new(level: Level, text: impl Into<String>) -> Self {
        Self {
            level,
            text: text.into(),
            detail: None,
            fixes: Vec::new(),
            reslice: false,
            lasting: Lasting::Default,
        }
    }

    /// The same message, its toast showing as long as `lasting` says.
    #[must_use]
    pub fn lasting(mut self, lasting: Lasting) -> Self {
        self.lasting = lasting;
        self
    }

    /// The severity its toast shows with.
    fn severity(&self) -> Severity {
        self.level.severity()
    }

    /// The same message with `detail`.
    #[must_use]
    pub fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    /// The message for a failed slice: its cause in a few words, the pipeline's reason as the
    /// detail, and the fixes for its remedies.
    #[must_use]
    pub fn failed_slice(reason: &str, remedies: &[Remedy]) -> Self {
        let text = match remedies.first() {
            Some(Remedy::RepairOutward | Remedy::TolerateReversed) => {
                tr!("slice-failed-inside-out")
            }
            Some(Remedy::RepairOrientation) => tr!("slice-failed-orientation"),
            Some(Remedy::CloseHoles | Remedy::FillHoles) => tr!("slice-failed-holes"),
            Some(Remedy::AllowTopologyDefects | Remedy::StrictFill) => {
                tr!("slice-failed-defects")
            }
            Some(Remedy::RestOnPlate) => tr!("slice-failed-floating"),
            Some(Remedy::RepairDefects) => tr!("slice-failed-open"),
            Some(Remedy::DropNonFinite) => tr!("slice-failed-non-finite"),
            Some(Remedy::MoveOntoPlate) => tr!("slice-failed-outside"),
            _ => tr!("slice-failed"),
        };
        Self {
            level: Level::Error,
            text,
            detail: Some(reason.to_owned()),
            fixes: repair_fixes(remedies, false)
                .into_iter()
                .chain(remedies.iter().copied().filter_map(Fix::for_remedy))
                .chain(repair_fixes(remedies, true))
                .collect(),
            reslice: true,
            lasting: Lasting::Default,
        }
    }

    /// The message for an opened model: what its diagnosis found, judged against the plate's
    /// settings when it was opened. `note` is added to the detail (for example an assumed unit).
    #[must_use]
    pub fn opened(name: &str, diagnosis: &Diagnosis, treatment: Treatment, note: &str) -> Self {
        let findings = diagnosis.findings();
        let mut lines: Vec<String> = Vec::new();
        let mut fixes = Vec::new();
        let mut level = Level::Info;
        for &finding in &findings {
            let handling = finding.handling(treatment);
            if handling == Handling::Unhandled {
                level = Level::Warning;
                if let Some(fix) = Fix::for_finding(finding) {
                    fixes.push(fix);
                }
            }
            lines.push(tr!(
                "finding-line",
                finding = describe(finding),
                consequence = consequence(finding, handling),
            ));
        }
        if !note.is_empty() {
            lines.push(note.to_owned());
        }
        let text = if findings.is_empty() {
            tr!("opened-clean", name = name)
        } else {
            let names: Vec<String> = findings.iter().map(|&f| short(f)).collect();
            tr!("opened-findings", name = name, findings = names.join(", "))
        };
        Self {
            level,
            text,
            detail: (!lines.is_empty()).then(|| lines.join("\n")),
            // Fixes on offer stay in sight until the user closes the toast.
            lasting: if fixes.is_empty() {
                Lasting::Default
            } else {
                Lasting::UntilClosed
            },
            fixes,
            reslice: false,
        }
    }
}

/// A few words for the status line.
fn short(finding: Finding) -> String {
    match finding {
        Finding::InsideOut { .. } => tr!("finding-short-inside-out"),
        Finding::InconsistentOrientation { .. } => tr!("finding-short-orientation"),
        Finding::DuplicateFaces { .. } => tr!("finding-short-duplicates"),
        Finding::Holes { .. } => tr!("finding-short-holes"),
        Finding::NonManifold { .. } => tr!("finding-short-touching"),
        Finding::Fins { .. } => tr!("finding-short-fins"),
        Finding::NonOrientable { .. } => tr!("finding-short-non-orientable"),
        Finding::AmbiguousVolume { .. } => tr!("finding-short-flat"),
        Finding::Cracks { .. } => tr!("finding-short-cracks"),
        Finding::DegenerateFaces { .. } => tr!("finding-short-degenerate"),
        Finding::PinchedVertices { .. } => tr!("finding-short-pinched"),
        Finding::SmallBodies { .. } => tr!("finding-short-small"),
        Finding::DroppedFacets { .. } => tr!("finding-short-dropped"),
        _ => tr!("finding-short-empty"),
    }
}

/// What the finding is.
pub(crate) fn describe(finding: Finding) -> String {
    match finding {
        Finding::InsideOut { bodies } => tr!("finding-inside-out", count = bodies),
        Finding::InconsistentOrientation { bodies } => tr!("finding-orientation", count = bodies),
        Finding::DuplicateFaces { faces } => tr!("finding-duplicates", count = faces),
        Finding::Holes { holes, edges } => tr!("finding-holes", holes = holes, edges = edges),
        Finding::NonManifold { edges } => tr!("finding-touching", count = edges),
        Finding::Fins { edges } => tr!("finding-fins", count = edges),
        Finding::NonOrientable { bodies } => tr!("finding-non-orientable", count = bodies),
        Finding::AmbiguousVolume { bodies } => tr!("finding-flat", count = bodies),
        Finding::Cracks { cracks } => tr!("finding-cracks", count = cracks),
        Finding::DegenerateFaces { needles, caps } => tr!(
            "finding-degenerate",
            count = needles + caps,
            needles = needles,
            caps = caps
        ),
        Finding::PinchedVertices { vertices } => tr!("finding-pinched", count = vertices),
        Finding::SmallBodies { bodies } => tr!("finding-small", count = bodies),
        Finding::DroppedFacets { facets } => tr!("finding-dropped", count = facets),
        _ => tr!("finding-empty"),
    }
}

/// What slicing will do about it.
pub(crate) fn consequence(finding: Finding, handling: Handling) -> String {
    match (finding, handling) {
        (Finding::DegenerateFaces { .. }, _) => tr!("consequence-degenerate"),
        (Finding::SmallBodies { .. }, _) => tr!("consequence-small"),
        (Finding::DroppedFacets { .. }, Handling::Handled) => tr!("consequence-dropped-handled"),
        (Finding::DroppedFacets { .. }, _) => tr!("consequence-dropped"),
        (_, Handling::Harmless) => tr!("consequence-harmless"),
        (Finding::Cracks { .. }, Handling::Handled) => tr!("consequence-cracks-handled"),
        (Finding::Fins { .. }, Handling::Handled) => tr!("consequence-fins-handled"),
        (Finding::InsideOut { .. }, Handling::Handled) => tr!("consequence-outward-handled"),
        (Finding::InconsistentOrientation { .. }, Handling::Handled) => {
            tr!("consequence-orientation-handled")
        }
        (Finding::DuplicateFaces { .. }, Handling::Handled) => {
            tr!("consequence-duplicates-handled")
        }
        (Finding::Holes { .. }, Handling::Handled) => tr!("consequence-holes-handled"),
        (Finding::InsideOut { .. }, _) => tr!("consequence-inside-out"),
        (Finding::Holes { .. } | Finding::Cracks { .. }, _) => tr!("consequence-holes"),
        (Finding::NonOrientable { .. } | Finding::AmbiguousVolume { .. }, _) => {
            tr!("consequence-unfixable")
        }
        (Finding::Empty, _) => tr!("consequence-empty"),
        _ => tr!("consequence-repairable"),
    }
}

/// What the user asked of the Messages window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Request {
    /// The fix to apply.
    pub fix: Fix,
    /// Whether to slice again afterwards.
    pub reslice: bool,
}

/// Every message of the session, and the window that lists them.
#[derive(Default)]
pub struct Messages {
    list: Vec<Message>,
    open: bool,
    /// Messages already seen in the window.
    seen: usize,
    /// Each message's toast while it shows.
    toasts: Toasts,
}

impl Messages {
    /// Add a message, and record it in the session log (TD-096) with its detail.
    pub fn push(&mut self, message: Message) {
        let level = match message.level {
            Level::Info | Level::Success => log::Level::Info,
            Level::Warning => log::Level::Warn,
            Level::Error => log::Level::Error,
        };
        let severity = message.severity();
        let duration = match message.lasting {
            Lasting::Default => severity.default_duration(),
            Lasting::For(duration) => Some(duration),
            Lasting::UntilClosed => None,
        };
        let more = message.detail.is_some() || !message.fixes.is_empty();
        self.toasts
            .push(severity, message.text.clone(), more, duration);
        match &message.detail {
            Some(detail) => log::log!(target: "messages", level, "{}\n{detail}", message.text),
            None => log::log!(target: "messages", level, "{}", message.text),
        }
        self.list.push(message);
    }

    /// The toasts over `viewport`; a toast's link opens the window.
    pub fn toasts(&mut self, ctx: &egui::Context, viewport: egui::Rect, icons: Option<&Icons>) {
        if self.toasts.show(ctx, viewport, icons) {
            self.open = true;
        }
    }

    /// Open the Messages window, or close it.
    pub fn toggle(&mut self) {
        self.open = !self.open;
    }

    /// The latest message, for the status bar's line.
    #[must_use]
    pub fn latest(&self) -> Option<Status> {
        self.list.last().map(|last| Status {
            text: last.text.clone(),
            colour: last.level.color(),
            detail: last.detail.clone(),
            busy: false,
        })
    }

    /// The latest message as the status bar's line shows it: a warning or an error says so in
    /// words too, not by its colour alone (TD-142).
    #[must_use]
    pub fn latest_line(&self) -> Option<Status> {
        let level = self.list.last()?.level;
        self.latest().map(|status| Status {
            text: if level >= Level::Warning {
                tr!(
                    "messages-latest",
                    level = level.severity().label(),
                    text = status.text.as_str()
                )
            } else {
                status.text
            },
            ..status
        })
    }

    /// The button that opens the window, coloured by the worst unseen message.
    pub fn status(&mut self, ui: &mut egui::Ui) {
        let unseen = &self.list[self.seen.min(self.list.len())..];
        let worst = unseen.iter().map(|m| m.level).max();
        let label = match (unseen.len(), worst) {
            (0, _) => tr!("messages-button", count = self.list.len()),
            // The worst unseen warning or error is named, not only coloured (TD-142).
            (n, Some(level)) if level >= Level::Warning => tr!(
                "messages-button-new-worst",
                count = n,
                level = level.severity().label()
            ),
            (n, _) => tr!("messages-button-new", count = n),
        };
        let mut text = egui::RichText::new(label);
        if let Some(level) = worst.filter(|&l| l > Level::Info) {
            text = text.color(level.color()).strong();
        }
        if ui
            .selectable_label(self.open, text)
            .on_hover_text(tr!("messages-hover"))
            .clicked()
        {
            self.open = !self.open;
        }
    }

    /// The Messages window, when open. Returns the fix the user asked for.
    pub fn window(&mut self, ctx: &egui::Context, workspace: &mut Workspace) -> Option<Request> {
        if !self.open {
            return None;
        }
        self.seen = self.list.len();
        let mut request = None;
        let mut open = true;
        let mut clear = false;
        crate::dialog::tool_window(
            ctx,
            "messages-window",
            tr!("messages-title"),
            egui::vec2(560.0, 360.0),
        )
        .open(&mut open)
        .show(ctx, |ui| {
            crate::dialog::remember_tool_window(ui, "messages-window");
            if self.list.is_empty() {
                ui.label(tr!("messages-none"));
            } else if ui.button(tr!("action-clear")).clicked() {
                clear = true;
            }
            crate::focus::scrolled(egui::ScrollArea::vertical(), ui, |ui| {
                // Newest first.
                for (index, message) in self.list.iter().enumerate().rev() {
                    ui.separator();
                    if let Some(fix) = show(ui, index, message, workspace) {
                        request = Some(Request {
                            fix,
                            reslice: message.reslice,
                        });
                    }
                }
            });
        });
        if clear {
            self.list.clear();
            self.seen = 0;
        }
        self.open = open;
        request
    }
}

/// One message in the window; returns the fix clicked.
fn show(
    ui: &mut egui::Ui,
    index: usize,
    message: &Message,
    workspace: &mut Workspace,
) -> Option<Fix> {
    // The level's icon and name with its colour (TD-142).
    let icons = Icons::shared(ui.ctx());
    let severity = message.level.severity();
    crate::widgets::state_line(
        ui,
        icons.as_deref(),
        Some(severity.icon()),
        &tr!(
            "messages-latest",
            level = severity.label(),
            text = message.text.as_str()
        ),
        message.level.color(),
    );
    if let Some(detail) = &message.detail {
        egui::CollapsingHeader::new(tr!("messages-details"))
            .id_salt(("message", index))
            .default_open(message.level > Level::Info)
            .show(ui, |ui| {
                ui.add(egui::Label::new(detail).selectable(true).wrap());
            });
    }
    let mut clicked = None;
    if !message.fixes.is_empty() {
        ui.horizontal_wrapped(|ui| {
            for &fix in &message.fixes {
                let label = if message.reslice {
                    tr!("fix-and-slice", fix = fix.label())
                } else {
                    fix.label()
                };
                if fix.applied(workspace) {
                    ui.add_enabled(
                        false,
                        egui::Button::new(tr!("fix-applied", fix = fix.label())),
                    );
                } else if ui.button(label).clicked() {
                    clicked = Some(fix);
                }
            }
        });
    }
    clicked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_warning_or_an_error_is_named_in_words_not_by_colour_alone() {
        let mut messages = Messages::default();
        messages.push(Message::new(Level::Info, "Opened"));
        assert_eq!(messages.latest_line().unwrap().text, "Opened");
        messages.push(Message::new(Level::Error, "Failed"));
        assert_eq!(
            messages.latest_line().unwrap().text,
            tr!(
                "messages-latest",
                level = tr!("toast-error"),
                text = "Failed"
            )
        );
        // The plain text stays the message's.
        assert_eq!(messages.latest().unwrap().text, "Failed");
    }

    #[test]
    fn every_message_shows_as_a_toast_for_its_lasting() {
        let mut messages = Messages::default();
        messages.push(Message::new(Level::Info, "Opened"));
        messages.push(
            Message::new(Level::Success, "Sliced").lasting(Lasting::For(Duration::from_secs(6))),
        );
        messages.push(Message::new(Level::Warning, "Thin").lasting(Lasting::UntilClosed));
        messages.push(Message::new(Level::Error, "Failed"));
        assert_eq!(
            messages.toasts.shown(),
            [
                (Severity::Info, Some(Duration::from_secs(5))),
                (Severity::Success, Some(Duration::from_secs(6))),
                (Severity::Warning, None),
                (Severity::Error, None),
            ]
        );
        // And every one stays in the Messages window.
        assert_eq!(messages.list.len(), 4);
    }
    use karasu_project::request::DEFAULT_REPAIR;

    #[test]
    fn failed_slices_name_the_cause_and_offer_its_fixes() {
        let message = Message::failed_slice(
            "cannot export \"x.goo\": negative winding",
            &[Remedy::RepairOutward],
        );
        assert_eq!(message.level, Level::Error);
        assert_eq!(
            message.text,
            "Slicing failed: part of the model is inside out"
        );
        assert_eq!(message.fixes, [Fix::TurnOutward]);
        let sliver = Message::failed_slice(
            "negative winding",
            &[Remedy::RepairOutward, Remedy::TolerateReversed],
        );
        assert_eq!(
            sliver.fixes,
            [
                Fix::TurnOutward,
                Fix::TolerateReversed,
                Fix::Repair(RepairPreset::Rebuild)
            ]
        );
        assert!(message.reslice);
        let unknown = Message::failed_slice("disk full", &[]);
        assert_eq!(unknown.fixes, []);
        assert_eq!(unknown.detail.as_deref(), Some("disk full"));
        let topology = Message::failed_slice(
            "model cannot be sliced as a union",
            &[
                Remedy::RepairOrientation,
                Remedy::StrictFill,
                Remedy::AllowTopologyDefects,
            ],
        );
        assert_eq!(
            topology.fixes,
            [
                Fix::Repair(RepairPreset::Standard),
                Fix::Orientation,
                Fix::AllowDefects
            ]
        );
        assert!(!topology.text.contains("--"));
        // A layer that does not close: the standard repair (TD-092 r8).
        let open = Message::failed_slice("does not close", &[Remedy::RepairDefects]);
        assert_eq!(open.fixes, [Fix::Repair(RepairPreset::Standard)]);
        // Facets without a position: the exact repair accepts leaving them out (TD-092 r9).
        let lost = Message::failed_slice("coordinate must be finite", &[Remedy::DropNonFinite]);
        assert_eq!(lost.fixes, [Fix::Repair(RepairPreset::Exact)]);
        assert_eq!(lost.text, tr!("slice-failed-non-finite"));
        let finding = Finding::DroppedFacets { facets: 1440 };
        assert_eq!(
            Fix::for_finding(finding),
            Some(Fix::Repair(RepairPreset::Exact))
        );
        assert!(describe(finding).contains("1440"));
    }

    #[test]
    fn fixes_change_only_their_settings() {
        let mut settings = PlateSettings::new("goo-v3");
        settings.repair.orientation = false;
        apply_setting(Fix::TurnOutward, &mut settings);
        assert!(settings.repair.outward && settings.repair.orientation);
        settings.close_holes_mm = Some(0.5);
        apply_setting(Fix::CloseHoles, &mut settings);
        assert_eq!(settings.close_holes_mm, Some(0.5));
        settings.close_holes_mm = None;
        apply_setting(Fix::CloseHoles, &mut settings);
        assert_eq!(settings.close_holes_mm, Some(DEFAULT_CLOSE_HOLES_MM));
        apply_setting(Fix::FillHoles, &mut settings);
        assert_eq!(settings.repair.fill_holes, Some(FillHoles::default()));
        apply_setting(Fix::TolerateReversed, &mut settings);
        assert_eq!(
            settings.reversed_tolerance_mm2,
            Some(DEFAULT_REVERSED_TOLERANCE_MM2)
        );
        let mut expected = PlateSettings::new("goo-v3");
        expected.repair.outward = true;
        expected.repair.fill_holes = Some(FillHoles::default());
        expected.reversed_tolerance_mm2 = Some(DEFAULT_REVERSED_TOLERANCE_MM2);
        assert_eq!(settings, expected);
    }

    #[test]
    fn opened_models_report_what_the_plate_does_not_handle() {
        use karasu_core::{
            cancellation::Cancellation,
            mesh::{formats::LengthUnit, repair::RepairReport},
        };
        use karasu_project::{diagnosis::diagnose, pipeline::import_mesh, request::ModelSource};
        // A closed box's defect report, with a repair report that turns one body outward.
        let source = ModelSource::Embedded {
            name: "box.stl".into(),
            bytes: crate::workspace::tests::box_stl([0.0; 3], [1.0; 3])
                .into_bytes()
                .into(),
        };
        let mesh = import_mesh(&source, Some(LengthUnit::Millimeter), &|_| false)
            .unwrap()
            .mesh;
        let mut diagnosis = diagnose(&mesh, &mut Cancellation::never()).unwrap();
        diagnosis.repairs.outward_flipped_components = 1;
        let treatment = Treatment {
            repair: DEFAULT_REPAIR,
            close_holes: true,
        };
        let message = Message::opened("model.stl", &diagnosis, treatment, "");
        assert_eq!(message.level, Level::Warning);
        assert_eq!(message.text, "model.stl: inside-out bodies");
        assert_eq!(message.fixes, [Fix::TurnOutward]);
        let detail = message.detail.unwrap();
        assert!(detail.contains("1 closed body is inside out"), "{detail}");
        // Once the plate turns bodies outward, the same finding is only information.
        let mut outward = treatment;
        outward.repair.outward = true;
        let message = Message::opened("model.stl", &diagnosis, outward, "Read in millimetres");
        assert_eq!(message.level, Level::Info);
        assert_eq!(message.fixes, []);
        assert!(message.detail.unwrap().ends_with("Read in millimetres"));
        // A clean model.
        let clean = Diagnosis {
            repairs: RepairReport::default(),
            fins: 0,
            ..diagnosis
        };
        let message = Message::opened("box.stl", &clean, treatment, "");
        assert_eq!(message.text, "Opened box.stl: no mesh defects found");
        assert_eq!(message.detail, None);
    }
}
