//! The Orient tool's panel (owner mockup, 2026-11-03): Constraints, Priority and the
//! Orientations found, then the footer's actions and state.
use super::{
    describe, Axes, Candidate, OrientUi, OrientValues, Priority, Stage, KEEP_AWAY_MM, LIMIT_DEG,
    WEIGHT,
};
use crate::{
    i18n::{decimal, tr},
    icons::{Icon, Icons},
    preparation_ui::PreparationUi,
    theme,
    widgets::{
        action_pair, default_choice, default_row, default_switch, icon_button, length_default,
        state_line, switch_default, DefaultValue, Unit, UnitField,
    },
    workspace::Workspace,
};
use eframe::egui;

/// A value's row in `unit` within `range`, stepped by `step`, with a reset to `default` when
/// given. Returns whether it changed.
fn number_row(
    ui: &mut egui::Ui,
    (label, hint): (String, String),
    value: &mut f64,
    (unit, range, step): (Unit, std::ops::RangeInclusive<f64>, f64),
    default: Option<f64>,
) -> bool {
    let default = default.map(|d| length_default(d, 2, unit));
    default_row(ui, label, value, default.as_ref(), |ui, value| {
        ui.add(UnitField::new(value, unit).range(range).step(step))
            .on_hover_text(hint)
    })
}

/// The tool's values in `ui`: the constraints and the priority, with `advanced` the distance
/// kept round protected points and the weights. Shared by the panel and the settings' defaults;
/// in the panel, `defaults` gives each field its reset button. Returns whether any changed.
pub fn orient_fields(
    ui: &mut egui::Ui,
    values: &mut OrientValues,
    advanced: bool,
    defaults: Option<&OrientValues>,
) -> bool {
    let mut edited = false;
    ui.label(egui::RichText::new(tr!("orient-axes")).color(ui.visuals().weak_text_color()));
    let labels = [
        tr!("orient-axes-any"),
        tr!("orient-axes-x"),
        tr!("orient-axes-y"),
    ];
    let place = |a: Axes| Axes::ALL.iter().position(|&x| x == a).unwrap_or(0);
    let mut chosen = place(values.axes);
    let default = defaults.map(|d| DefaultValue::new(place(d.axes), labels[place(d.axes)].clone()));
    if default_choice(ui, &mut chosen, &labels, default.as_ref())
        .on_hover_text(tr!("orient-axes-hint"))
        .changed()
    {
        values.axes = Axes::ALL[chosen];
        edited = true;
    }
    edited |= default_switch(
        ui,
        &mut values.limited,
        (tr!("orient-limit"), tr!("orient-limit-hint")),
        defaults.map(|d| switch_default(d.limited)).as_ref(),
    );
    let on = values.limited;
    crate::widgets::depends(ui, on, |ui| {
        edited |= number_row(
            ui,
            (tr!("orient-limit-angle"), tr!("orient-limit-hint")),
            &mut values.limit_deg,
            (Unit::Degree, LIMIT_DEG, 1.0),
            defaults.map(|d| d.limit_deg),
        );
    });
    if advanced {
        edited |= number_row(
            ui,
            (tr!("orient-keep-away"), tr!("orient-keep-away-hint")),
            &mut values.keep_away_mm,
            (Unit::Millimetre, KEEP_AWAY_MM, 0.5),
            defaults.map(|d| d.keep_away_mm),
        );
    }
    ui.add_space(4.0);
    edited | priority_fields(ui, values, advanced, defaults)
}

/// The priority, with `advanced` the weights it sets; with `defaults`, their reset buttons.
fn priority_fields(
    ui: &mut egui::Ui,
    values: &mut OrientValues,
    advanced: bool,
    defaults: Option<&OrientValues>,
) -> bool {
    let mut edited = false;
    ui.label(egui::RichText::new(tr!("orient-priority")).color(ui.visuals().weak_text_color()));
    let labels = [
        tr!("orient-priority-fewer"),
        tr!("orient-priority-balanced"),
        tr!("orient-priority-faster"),
    ];
    let place = |p: Priority| Priority::ALL.iter().position(|&x| x == p).unwrap_or(1);
    let mut chosen = place(values.priority);
    let default =
        defaults.map(|d| DefaultValue::new(place(d.priority), labels[place(d.priority)].clone()));
    if default_choice(ui, &mut chosen, &labels, default.as_ref())
        .on_hover_text(tr!("orient-priority-hint"))
        .changed()
    {
        values.priority = Priority::ALL[chosen];
        values.weights = values.priority.weights();
        edited = true;
    }
    if advanced {
        // The weights have no unit: each measure is divided by its largest among the
        // candidates, then multiplied by its weight (TD-119).
        ui.add(
            egui::Label::new(
                egui::RichText::new(tr!("orient-weights-note"))
                    .small()
                    .color(ui.visuals().weak_text_color()),
            )
            .wrap(),
        );
        let preset = values.priority.weights();
        let w = &mut values.weights;
        for (label, hint, value, default) in [
            (
                tr!("orient-weight-facing"),
                tr!("orient-weight-facing-hint"),
                &mut w.facing,
                preset.facing,
            ),
            (
                tr!("orient-weight-islands"),
                tr!("orient-weight-islands-hint"),
                &mut w.islands,
                preset.islands,
            ),
            (
                tr!("orient-weight-cups"),
                tr!("orient-weight-cups-hint"),
                &mut w.cups,
                preset.cups,
            ),
            (
                tr!("orient-weight-height"),
                tr!("orient-weight-height-hint"),
                &mut w.height,
                preset.height,
            ),
            (
                tr!("orient-weight-largest"),
                tr!("orient-weight-largest-hint"),
                &mut w.largest,
                preset.largest,
            ),
            (
                tr!("orient-weight-protected"),
                tr!("orient-weight-protected-hint"),
                &mut w.protected,
                preset.protected,
            ),
        ] {
            edited |= number_row(
                ui,
                (label, hint),
                value,
                (Unit::Ratio, WEIGHT, 0.25),
                defaults.map(|_| default),
            );
        }
    }
    edited
}

impl OrientUi {
    /// The panel's body for model `index`.
    pub fn panel(
        &mut self,
        ui: &mut egui::Ui,
        workspace: &mut Workspace,
        index: Option<usize>,
        icons: Option<&Icons>,
    ) {
        let index = index.filter(|&i| i < workspace.plate().objects.len());
        if index.is_none() {
            self.close();
        }
        ui.add_enabled_ui(index.is_some(), |ui| {
            crate::widgets::group(ui, tr!("orient-constraints"), |ui| {
                let defaults = self.defaults;
                let advanced = self.advanced;
                orient_fields(ui, &mut self.values, advanced, Some(&defaults));
                ui.add_space(6.0);
                Self::points_section(ui, workspace, index, icons);
            });
        });
        crate::widgets::group(ui, tr!("orient-orientations"), |ui| {
            self.candidates_section(ui, workspace, index);
        });
    }

    /// The protected points of model `index`, each with its bin.
    fn points_section(
        ui: &mut egui::Ui,
        workspace: &mut Workspace,
        index: Option<usize>,
        icons: Option<&Icons>,
    ) {
        let count = index.map_or(0, |i| Self::points_of(workspace, i).len());
        ui.horizontal(|ui| {
            ui.label(tr!("orient-points"))
                .on_hover_text(tr!("orient-points-hint"));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new(count.to_string()).color(ui.visuals().weak_text_color()),
                );
            });
        });
        let Some(index) = index else {
            return;
        };
        if count == 0 {
            ui.add(
                egui::Label::new(
                    egui::RichText::new(tr!("orient-points-none"))
                        .small()
                        .color(ui.visuals().weak_text_color()),
                )
                .wrap(),
            );
            return;
        }
        let mut removed = None;
        for k in 0..count {
            ui.horizontal(|ui| {
                ui.label(tr!("orient-point", number = k + 1));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if icon_button(ui, icons, Icon::Delete, false, &tr!("orient-point-remove"))
                        .clicked()
                    {
                        removed = Some(k);
                    }
                });
            });
        }
        if let Some(k) = removed {
            Self::remove_point(workspace, index, k);
        }
    }

    /// The candidates found, each a card: chosen on a click, previewed while hovered.
    fn candidates_section(
        &mut self,
        ui: &mut egui::Ui,
        workspace: &Workspace,
        index: Option<usize>,
    ) {
        let found = self
            .found_of(workspace, index)
            .map(|(c, current)| (c.to_vec(), current));
        let Some((candidates, current)) = found else {
            ui.add(
                egui::Label::new(
                    egui::RichText::new(tr!("orient-none-yet"))
                        .small()
                        .color(ui.visuals().weak_text_color()),
                )
                .wrap(),
            );
            return;
        };
        if candidates.is_empty() {
            ui.add(egui::Label::new(tr!("orient-none-fits")).wrap());
            return;
        }
        if !current {
            ui.add(
                egui::Label::new(
                    egui::RichText::new(tr!("orient-out-of-date")).color(theme::WARNING.color()),
                )
                .wrap(),
            );
        }
        let tags = best_tags(&candidates);
        let mut hovered = None;
        for (k, c) in candidates.iter().enumerate() {
            let chosen = k == self.chosen;
            let stroke = if chosen {
                ui.visuals().selection.stroke
            } else {
                ui.visuals().widgets.noninteractive.bg_stroke
            };
            let frame = egui::Frame::group(ui.style())
                .stroke(stroke)
                .inner_margin(egui::Margin::same(8));
            let response = frame
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(format!("{}. {}", k + 1, describe(&c.tilt)))
                                .strong(),
                        );
                        if let Some(tag) = &tags[k] {
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(
                                        egui::RichText::new(tag)
                                            .small()
                                            .color(theme::ACCENT.color()),
                                    );
                                },
                            );
                        }
                    });
                    facts(ui, c);
                })
                .response
                .interact(egui::Sense::click());
            if current && response.hovered() {
                hovered = Some(k);
            }
            if current && response.clicked() {
                self.chosen = k;
            }
            ui.add_space(4.0);
        }
        self.hovered = hovered;
    }

    /// The panel's footer: what turning means for this model, Find orientations (Stop while
    /// searching) and Apply, the progress and the state.
    pub fn footer(
        &mut self,
        ui: &mut egui::Ui,
        workspace: &mut Workspace,
        preparation: &PreparationUi,
        index: Option<usize>,
        icons: Option<&Icons>,
    ) {
        let index = index.filter(|&i| i < workspace.plate().objects.len());
        if let Some(i) = index {
            let object = &workspace.plate().objects[i];
            let supports = object
                .supports
                .as_ref()
                .is_some_and(|d| !d.supports.is_empty());
            for (shown, text) in [
                (supports, tr!("orient-warn-supports")),
                (object.hollowing().is_some(), tr!("orient-warn-hollowed")),
            ] {
                if shown {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(text)
                                .small()
                                .color(theme::WARNING.color()),
                        )
                        .wrap(),
                    );
                }
            }
        }
        // Candidates found: Apply the chosen one, or Discard them and see the model as it is
        // (owner, 2026-11-03), as the Automatic tool's plan.
        if let Some((candidates, current)) = self.found_of(workspace, index) {
            let applicable = current && !candidates.is_empty() && !self.searching();
            let (apply, discard) = action_pair(
                ui,
                [tr!("orient-apply"), tr!("orient-discard")],
                (applicable, true),
            );
            if let (true, Some(i)) = (
                apply.on_hover_text(tr!("orient-apply-hint")).clicked(),
                index,
            ) {
                self.apply(workspace, i);
            } else if discard.on_hover_text(tr!("orient-discard-hint")).clicked() {
                self.discard();
            }
            ui.add_space(4.0);
        }
        let (label, hint) = if self.searching() {
            (tr!("orient-stop"), tr!("orient-stop-hint"))
        } else {
            (tr!("orient-find"), tr!("orient-find-hint"))
        };
        let size = egui::vec2(ui.available_width(), ui.spacing().interact_size.y);
        let clicked = ui
            .add_enabled(
                index.is_some() && workspace.plate().setting().is_some(),
                egui::Button::new(label).min_size(size),
            )
            .on_hover_text(hint)
            .clicked();
        if let (true, Some(i)) = (clicked, index) {
            self.find(workspace, preparation, i);
        }
        if let Some((_, progress)) = self.stage() {
            ui.add_space(4.0);
            ui.add(egui::ProgressBar::new(progress).desired_height(6.0));
        }
        ui.add_space(4.0);
        let (text, colour, icon) = self.state(workspace, index);
        state_line(ui, icons, icon, &text, colour);
    }

    /// What the footer says.
    fn state(
        &self,
        workspace: &Workspace,
        index: Option<usize>,
    ) -> (String, egui::Color32, Option<Icon>) {
        if index.is_none() {
            return ("–".to_owned(), theme::BORDER.color(), None);
        }
        if workspace.plate().setting().is_none() {
            return (
                tr!("prepare-needs-profiles"),
                theme::WARNING.color(),
                Some(Icon::Warning),
            );
        }
        let info = |text| (text, theme::ACCENT.color(), Some(Icon::Info));
        if let Some((stage, _)) = self.stage() {
            return info(match stage {
                Stage::Rough => tr!("orient-stage-rough"),
                Stage::Checking(k, n) => tr!("orient-stage-checking", number = k, count = n),
            });
        }
        if self.protecting {
            return info(tr!("orient-protecting"));
        }
        match self.found_of(workspace, index) {
            Some((_, false)) => (
                tr!("orient-out-of-date"),
                theme::WARNING.color(),
                Some(Icon::Warning),
            ),
            Some(([], true)) => (
                tr!("orient-none-fits"),
                theme::WARNING.color(),
                Some(Icon::Warning),
            ),
            Some((c, true)) => info(tr!(
                "orient-state-found",
                count = c.len(),
                seconds = decimal(self.seconds().unwrap_or(0.0), 1)
            )),
            None => (tr!("orient-state-idle"), theme::BORDER.color(), None),
        }
    }
}

/// A candidate's measures, in a grid of two columns of facts.
fn facts(ui: &mut egui::Ui, c: &Candidate) {
    let weak = ui.visuals().weak_text_color();
    let rows = [
        (
            tr!("orient-fact-facing"),
            format!("{} mm²", decimal(c.facing_mm2, 0)),
        ),
        (tr!("orient-fact-islands"), c.islands.to_string()),
        (
            tr!("orient-fact-cups"),
            c.cups.map_or_else(|| "–".to_owned(), |n| n.to_string()),
        ),
        (
            tr!("orient-fact-height"),
            format!("{} mm", decimal(c.height_mm, 1)),
        ),
        (
            tr!("orient-fact-largest"),
            format!("{} mm²", decimal(c.largest_mm2, 0)),
        ),
        (
            tr!("orient-fact-protected"),
            if c.protected_mm2 > 0.0 || c.protected_islands > 0 {
                tr!("orient-protected-near")
            } else {
                tr!("orient-protected-clear")
            },
        ),
    ];
    egui::Grid::new(ui.next_auto_id())
        .num_columns(4)
        .spacing([8.0, 2.0])
        .show(ui, |ui| {
            for pair in rows.chunks(2) {
                for (name, value) in pair {
                    ui.label(egui::RichText::new(name).small().color(weak));
                    ui.label(egui::RichText::new(value).small());
                }
                ui.end_row();
            }
        });
}

/// Each candidate's tag: the best at fewest supports, fewest islands, fewest cups or lowest.
fn best_tags(candidates: &[Candidate]) -> Vec<Option<String>> {
    let mut tags = vec![None; candidates.len()];
    let least = |key: &dyn Fn(&Candidate) -> f64| {
        (0..candidates.len()).min_by(|&a, &b| key(&candidates[a]).total_cmp(&key(&candidates[b])))
    };
    #[allow(clippy::cast_precision_loss)] // Counts.
    let picks: [(Option<usize>, String); 4] = [
        (least(&|c| c.facing_mm2), tr!("orient-tag-fewest-supports")),
        (
            least(&|c| c.islands as f64),
            tr!("orient-tag-fewest-islands"),
        ),
        (
            least(&|c| c.cups.map_or(f64::INFINITY, |n| n as f64))
                .filter(|&k| candidates[k].cups.is_some()),
            tr!("orient-tag-fewest-cups"),
        ),
        (least(&|c| c.height_mm), tr!("orient-tag-lowest")),
    ];
    for (k, tag) in picks {
        if let Some(k) = k {
            if tags[k].is_none() {
                tags[k] = Some(tag);
            }
        }
    }
    tags
}
