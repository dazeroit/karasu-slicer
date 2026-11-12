//! The Braces tool's panel, footer and list window (owner mockup, 2026-10-29). The panel keeps
//! values: the new braces' diameter, the automatic braces' distance, heights and pattern, and
//! the selected brace's own values. The footer says what the tool is doing, and with the
//! automatic braces shown, holds Apply and Discard.
use super::{
    super::{describe, errors, list::short, SupportsUi},
    auto::Outcome,
    NewBraces, Pattern, DIAMETER_MM, DISTANCE_MM, LOWEST_MM, STEP_MM,
};
use crate::{
    appearance::Appearance,
    i18n::tr,
    icons::{Icon, Icons},
    list_window::{self, Event, Row, Spec},
    theme,
    widgets::{
        action_pair, default_choice, default_row, default_switch, fact_row, on_off, state_line,
        value_row, with_unit, DefaultValue, Unit, UnitField,
    },
    workspace::Workspace,
};
use eframe::egui;
use karasu_core::supports::{Brace, Severity, SupportFinding, SupportScene};

/// A diameter field for a brace, with a reset to `default` when given.
fn diameter_row(
    ui: &mut egui::Ui,
    label: String,
    diameter: &mut f64,
    hint: String,
    default: Option<&DefaultValue<f64>>,
) -> bool {
    default_row(ui, label, diameter, default, |ui, diameter| {
        ui.add(
            UnitField::new(diameter, Unit::Millimetre)
                .range(DIAMETER_MM)
                .step(0.05),
        )
        .on_hover_text(hint)
    })
}

/// The switch "Automatic diameter" and the diameter's field. With the switch on, the field is
/// the largest diameter an automatic brace takes (TD-136), and `shown`, the diameter worked out,
/// is shown below it when known. Returns whether either changed.
fn diameter_choice(
    ui: &mut egui::Ui,
    (auto, diameter): (&mut bool, &mut f64),
    shown: Option<f64>,
    hint: String,
    defaults: Option<&NewBraces>,
) -> bool {
    let switched = default_switch(
        ui,
        auto,
        (
            tr!("braces-auto-diameter"),
            tr!("braces-auto-diameter-hint"),
        ),
        defaults
            .map(|d| DefaultValue::new(d.auto_diameter, on_off(d.auto_diameter)))
            .as_ref(),
    );
    let default = defaults
        .map(|d| DefaultValue::new(d.diameter_mm, with_unit(d.diameter_mm, 2, Unit::Millimetre)));
    let label = if *auto {
        tr!("braces-largest-diameter")
    } else {
        tr!("braces-diameter")
    };
    let typed = diameter_row(ui, label, diameter, hint, default.as_ref());
    if let Some(built) = shown.filter(|_| *auto) {
        fact_row(
            ui,
            &tr!("braces-built-diameter"),
            &with_unit(built, 2, Unit::Millimetre),
        );
    }
    switched || typed
}

/// New braces' values in `ui`: the diameter, then the automatic braces' distance, lowest height,
/// step and pattern. Shared by the panel and the settings' defaults; in the panel, `defaults`
/// gives each field its reset button. Returns whether any changed.
pub fn brace_fields(ui: &mut egui::Ui, new: &mut NewBraces, defaults: Option<&NewBraces>) -> bool {
    let mut edited = diameter_choice(
        ui,
        (&mut new.auto_diameter, &mut new.diameter_mm),
        None,
        tr!("braces-diameter-hint"),
        defaults,
    );
    crate::widgets::group(ui, tr!("braces-auto"), |ui| {
        let millimetres = |v: f64| DefaultValue::new(v, with_unit(v, 1, Unit::Millimetre));
        for (label, hint, value, range, default) in [
            (
                tr!("braces-distance"),
                tr!("braces-distance-hint"),
                &mut new.max_distance_mm,
                DISTANCE_MM,
                defaults.map(|d| millimetres(d.max_distance_mm)),
            ),
            (
                tr!("braces-lowest"),
                tr!("braces-lowest-hint"),
                &mut new.lowest_mm,
                LOWEST_MM,
                defaults.map(|d| millimetres(d.lowest_mm)),
            ),
            (
                tr!("braces-step"),
                tr!("braces-step-hint"),
                &mut new.step_mm,
                STEP_MM,
                defaults.map(|d| millimetres(d.step_mm)),
            ),
        ] {
            edited |= default_row(ui, label, value, default.as_ref(), |ui, value| {
                ui.add(
                    UnitField::new(value, Unit::Millimetre)
                        .range(range)
                        .step(0.5),
                )
                .on_hover_text(hint)
            });
        }
        ui.label(tr!("braces-pattern"));
        let labels = Pattern::ALL.map(Pattern::label);
        let place = |pattern: Pattern| Pattern::ALL.iter().position(|&p| p == pattern).unwrap_or(1);
        let mut chosen = place(new.pattern);
        let default = defaults.map(|d| DefaultValue::new(place(d.pattern), d.pattern.label()));
        if default_choice(ui, &mut chosen, &labels, default.as_ref())
            .on_hover_text(tr!("braces-pattern-hint"))
            .changed()
        {
            new.pattern = Pattern::ALL[chosen];
            edited = true;
        }
        edited
    })
}

/// Brace `id`'s findings in `scene`.
fn findings(scene: Option<&SupportScene>, id: u64) -> Vec<&SupportFinding> {
    scene.map_or_else(Vec::new, |s| {
        s.findings().iter().filter(|f| f.entity == id).collect()
    })
}

/// Brace `brace`'s row: the supports it joins, its state from `scene`'s findings, its dot in the
/// colour the scene draws it (`selected` the selected brace's identity).
fn row(
    scene: Option<&SupportScene>,
    appearance: &Appearance,
    selected: Option<u64>,
    brace: &Brace,
) -> Row {
    let found = findings(scene, brace.id);
    let error = found.iter().find(|f| f.severity == Severity::Error);
    let (state, state_colour) = match error {
        Some(f) => (short(f.kind), theme::ERROR.color()),
        None => (tr!("list-ok"), theme::TEXT_PRIMARY.color()),
    };
    let chosen = Some(brace.id) == selected;
    let dot = match (error.is_some(), chosen) {
        (true, true) => appearance.support_error_selected,
        (true, false) => appearance.support_error,
        (false, true) => appearance.support_selected,
        (false, false) => appearance.support,
    };
    let hint = if found.is_empty() {
        tr!("list-row-hint")
    } else {
        found
            .iter()
            .map(|f| describe(f))
            .collect::<Vec<_>>()
            .join("\n")
    };
    Row {
        dot: dot.color(),
        name: tr!("list-number", number = brace.id),
        fact: tr!("braces-joins", from = brace.from.0, to = brace.to.0),
        state,
        state_colour,
        hint,
        selected: chosen,
    }
}

/// Where brace `id` of `scene` lies, its lowest and highest corners in world coordinates.
fn bounds(scene: &SupportScene, id: u64) -> Option<[[f64; 3]; 2]> {
    let mesh = scene.mesh();
    let (mut low, mut high) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
    for range in scene.part_ranges().iter().filter(|r| r.entity == id) {
        for triangle in &mesh.triangles()[range.triangles.clone()] {
            for &v in triangle {
                let p = mesh.vertices()[v as usize];
                for (k, c) in [p.x, p.y, p.z].into_iter().enumerate() {
                    low[k] = low[k].min(c);
                    high[k] = high[k].max(c);
                }
            }
        }
    }
    (low[0] <= high[0]).then_some([low, high])
}

/// What the footer says of the automatic braces' plan: being made, failed, or what Apply would
/// add and what was skipped, and why, in its tooltip.
fn auto_state(outcome: Outcome) -> (String, egui::Color32, Option<Icon>, Option<String>) {
    match outcome {
        Outcome::Waiting | Outcome::Planning => (
            tr!("braces-auto-planning"),
            theme::ACCENT.color(),
            Some(Icon::Info),
            None,
        ),
        Outcome::Failed(reason) => (
            tr!("braces-auto-failed"),
            theme::ERROR.color(),
            Some(Icon::Error),
            Some(reason),
        ),
        Outcome::Ready { kept, skipped } => {
            let total: usize = skipped.iter().sum();
            let why: Vec<String> = [
                tr!("braces-skipped-model", count = skipped[0]),
                tr!("braces-skipped-hole", count = skipped[1]),
                tr!("braces-skipped-joined", count = skipped[2]),
            ]
            .into_iter()
            .zip(skipped)
            .filter(|(_, n)| *n > 0)
            .map(|(text, _)| text)
            .collect();
            (
                tr!("braces-auto-state", count = kept, skipped = total),
                if total > 0 {
                    theme::WARNING.color()
                } else {
                    theme::ACCENT.color()
                },
                Some(if total > 0 { Icon::Warning } else { Icon::Info }),
                (!why.is_empty()).then(|| why.join("\n")),
            )
        }
    }
}

impl SupportsUi {
    /// The Braces tool's panel for model `index`: the new braces' values, then the selected
    /// brace's.
    pub fn braces_panel(
        &mut self,
        ui: &mut egui::Ui,
        workspace: &mut Workspace,
        index: Option<usize>,
    ) {
        let index = index.filter(|&i| i < workspace.plate().objects.len());
        if index.is_none() {
            self.close_braces();
        }
        ui.add_enabled_ui(index.is_some(), |ui| {
            crate::widgets::group(ui, tr!("braces-new"), |ui| {
                let defaults = self.tool_defaults.braces;
                brace_fields(ui, &mut self.bracing.new, Some(&defaults));
            });
        });
        ui.add_space(4.0);
        self.selected_brace(ui, workspace, index);
    }

    /// The selected brace of model `index`: the supports it joins and its values, which change
    /// it alone. Without one, the same fields, disabled.
    fn selected_brace(
        &mut self,
        ui: &mut egui::Ui,
        workspace: &mut Workspace,
        index: Option<usize>,
    ) {
        let chosen = self
            .bracing
            .selected
            .filter(|&(object, _)| Some(object) == index)
            .and_then(|(object, id)| {
                let brace = workspace
                    .supports(object)?
                    .braces
                    .iter()
                    .find(|b| b.id == id)?;
                Some((object, *brace))
            });
        let title = match chosen {
            Some((_, brace)) => tr!("braces-selected-title", number = brace.id),
            None => tr!("braces-selected"),
        };
        crate::widgets::group(ui, title, |ui| {
            let mut brace =
                chosen.map_or(self.bracing.new.brace(0, (0, 0.0), (0, 0.0)), |(_, b)| b);
            let worked_out = chosen.and_then(|(object, b)| self.brace_diameter(object, &b));
            let mut edited = false;
            ui.add_enabled_ui(chosen.is_some(), |ui| {
                let joins = chosen.map_or_else(
                    || "–".to_owned(),
                    |(_, b)| tr!("braces-joins", from = b.from.0, to = b.to.0),
                );
                fact_row(ui, &tr!("braces-joins-label"), &joins);
                let mut auto = brace.diameter_mm.is_none();
                // An automatic brace written before TD-136 has no largest diameter: its own is
                // shown until one is typed.
                let mut diameter = brace
                    .diameter_mm
                    .or(brace.max_diameter_mm)
                    .or(worked_out)
                    .unwrap_or(self.bracing.new.diameter_mm);
                if diameter_choice(
                    ui,
                    (&mut auto, &mut diameter),
                    worked_out,
                    tr!("braces-selected-hint"),
                    None,
                ) {
                    brace.diameter_mm = (!auto).then_some(diameter);
                    brace.max_diameter_mm = auto.then_some(diameter);
                    edited = true;
                }
                for (end, which) in [(&mut brace.from, true), (&mut brace.to, false)] {
                    let label = if chosen.is_some() {
                        tr!("braces-height-on", number = end.0)
                    } else if which {
                        tr!("braces-height-first")
                    } else {
                        tr!("braces-height-second")
                    };
                    edited |= value_row(ui, label, |ui| {
                        ui.add(
                            UnitField::new(&mut end.1, Unit::Millimetre)
                                .range(0.0..=1000.0)
                                .step(0.1),
                        )
                        .on_hover_text(tr!("braces-height-hint"))
                    })
                    .changed();
                }
                if let Some(scene) = chosen.and_then(|(object, _)| self.scene(object)) {
                    for finding in findings(Some(scene), brace.id) {
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(describe(finding)).color(theme::ERROR.color()),
                            )
                            .wrap(),
                        );
                    }
                }
            });
            if let (true, Some((object, _))) = (edited, chosen) {
                Self::replace_brace(workspace, object, brace, true);
            }
        });
    }

    /// The Braces tool's footer for model `index`: with the automatic braces shown, Apply and
    /// Discard and what they would add; otherwise what the tool is doing, or the braces' state.
    pub fn braces_footer(
        &mut self,
        ui: &mut egui::Ui,
        workspace: &mut Workspace,
        index: Option<usize>,
        icons: Option<&Icons>,
    ) {
        let index = index.filter(|&i| i < workspace.plate().objects.len());
        if let Some(outcome) = self.auto_outcome(index) {
            let kept = match outcome {
                Outcome::Ready { kept, .. } => kept,
                _ => 0,
            };
            let (apply, discard) = action_pair(
                ui,
                [
                    tr!("braces-auto-apply", count = kept),
                    tr!("braces-auto-discard"),
                ],
                (kept > 0 && index.is_some(), true),
            );
            if let (true, Some(i)) = (apply.clicked(), index) {
                self.apply_auto(workspace, i);
            } else if discard.clicked() {
                self.toggle_auto_braces();
            }
            ui.add_space(4.0);
        }
        let (text, colour, icon, detail) = self.braces_state(workspace, index);
        let line = state_line(ui, icons, icon, &text, colour);
        if let Some(detail) = detail {
            line.on_hover_text(detail);
        }
    }

    /// What the footer says: the automatic braces' plan, placing, or how many braces model
    /// `index` has and how many have errors.
    fn braces_state(
        &self,
        workspace: &Workspace,
        index: Option<usize>,
    ) -> (String, egui::Color32, Option<Icon>, Option<String>) {
        let Some(index) = index else {
            return ("–".to_owned(), theme::BORDER.color(), None, None);
        };
        if workspace.plate().setting().is_none() {
            return (
                tr!("prepare-needs-profiles"),
                theme::WARNING.color(),
                Some(Icon::Warning),
                None,
            );
        }
        let supported = workspace
            .supports(index)
            .is_some_and(|d| d.supports.len() >= 2);
        if !supported {
            return (
                tr!("braces-need-supports"),
                theme::BORDER.color(),
                Some(Icon::Info),
                None,
            );
        }
        if let Some(outcome) = self.auto_outcome(Some(index)) {
            return auto_state(outcome);
        }
        if self.bracing.placing {
            let (text, hint) = match (self.bracing.first.is_some(), self.bracing.chain) {
                (false, _) => (tr!("braces-placing"), tr!("braces-placing-hint")),
                (true, true) => (
                    tr!("braces-placing-chain"),
                    tr!("braces-placing-chain-hint"),
                ),
                (true, false) => (
                    tr!("braces-placing-second"),
                    tr!("braces-placing-second-hint"),
                ),
            };
            return (text, theme::ACCENT.color(), Some(Icon::Info), Some(hint));
        }
        let count = workspace.supports(index).map_or(0, |d| d.braces.len());
        let scene = self.scene(index);
        let wrong = scene.map_or_else(Vec::new, |s| {
            let ids: Vec<u64> = workspace
                .supports(index)
                .map_or_default(|d| d.braces.iter().map(|b| b.id).collect());
            errors(s.findings())
                .into_iter()
                .filter(|id| ids.contains(id))
                .collect()
        });
        if count == 0 {
            (tr!("braces-none"), theme::BORDER.color(), None, None)
        } else if wrong.is_empty() {
            (
                tr!("braces-state", count = count),
                theme::SUCCESS.color(),
                Some(Icon::Success),
                None,
            )
        } else {
            (
                tr!("braces-state-errors", count = count, errors = wrong.len()),
                theme::WARNING.color(),
                Some(Icon::Warning),
                Some(tr!("braces-state-errors-hint")),
            )
        }
    }

    /// The list window of model `index`'s braces, while open. Returns where the view should
    /// go, after a double click on a row.
    pub fn braces_window(
        &mut self,
        ctx: &egui::Context,
        workspace: &mut Workspace,
        index: Option<usize>,
        appearance: &Appearance,
    ) -> Option<[[f64; 3]; 2]> {
        if !self.bracing.list_open {
            return None;
        }
        let index = index.filter(|&i| i < workspace.plate().objects.len());
        let braces: Vec<Brace> = index
            .and_then(|i| workspace.supports(i))
            .map_or_else(Vec::new, |d| d.braces.clone());
        // The build's fields alone, so the window's own state can be lent beside it.
        let scene = index
            .and_then(|i| self.builds.get(&i))
            .and_then(|b| b.scene.as_ref().ok());
        let wrong: Vec<u64> = scene.map_or_else(Vec::new, |s| errors(s.findings()));
        let wrong_braces = braces
            .iter()
            .filter(|b| wrong.binary_search(&b.id).is_ok())
            .count();
        let items: Vec<u64> = braces
            .iter()
            .map(|b| b.id)
            .filter(|id| !self.bracing.only_errors || wrong.binary_search(id).is_ok())
            .collect();
        let selected = self
            .bracing
            .selected
            .filter(|&(object, _)| Some(object) == index)
            .map(|(_, id)| id);
        let reveal = std::mem::take(&mut self.bracing.reveal)
            .then_some(selected)
            .flatten();
        let spec = Spec {
            id: "braces-list",
            title: tr!("braces-list-title", count = braces.len()),
            fact: tr!("braces-joins-label"),
            state: tr!("list-state"),
            filters: vec![
                tr!("list-all"),
                tr!("supports-list-errors", count = wrong_braces),
            ],
            summary: if index.is_some() {
                tr!(
                    "braces-list-summary",
                    count = braces.len(),
                    errors = wrong_braces
                )
            } else {
                tr!("panel-no-model")
            },
            tabs: Vec::new(),
            places: Vec::new(),
            remove: Some(tr!("braces-remove")),
        };
        let gone = Brace {
            id: 0,
            from: (0, 0.0),
            to: (0, 0.0),
            diameter_mm: None,
            max_diameter_mm: None,
        };
        let row = |id: u64| {
            let brace = braces.iter().find(|b| b.id == id).unwrap_or(&gone);
            row(scene, appearance, selected, brace)
        };
        let mut filter = usize::from(self.bracing.only_errors);
        let event = list_window::show(
            ctx,
            &spec,
            &mut self.bracing.list_open,
            &mut 0,
            &mut filter,
            &mut 0,
            &items,
            reveal,
            row,
        );
        self.bracing.only_errors = filter == 1;
        let object = index?;
        match event? {
            Event::Picked(id) => {
                self.bracing.selected = Some((object, id));
                None
            }
            Event::Focused(id) => {
                self.bracing.selected = Some((object, id));
                self.scene(object).and_then(|s| bounds(s, id))
            }
            Event::Removed(id) => {
                Self::remove_brace(workspace, object, id);
                if self.bracing.selected == Some((object, id)) {
                    self.bracing.selected = None;
                }
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_braces_row_says_what_it_joins_and_its_first_error() {
        let appearance = Appearance::default();
        let brace = Brace {
            id: 7,
            from: (1, 4.0),
            to: (2, 6.0),
            diameter_mm: Some(0.8),
            max_diameter_mm: None,
        };
        let shown = row(None, &appearance, None, &brace);
        assert_eq!(shown.name, tr!("list-number", number = 7_u64));
        assert_eq!(shown.fact, tr!("braces-joins", from = 1_u64, to = 2_u64));
        assert_eq!(shown.state, tr!("list-ok"));
        assert_eq!(shown.dot, appearance.support.color());
        let chosen = row(None, &appearance, Some(7), &brace);
        assert!(chosen.selected);
        assert_eq!(chosen.dot, appearance.support_selected.color());
    }
}
