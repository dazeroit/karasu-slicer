//! The Islands tool's panel, footer and list window (owner mockup, 2026-10-29). The panel holds
//! the search's values and steps through the islands; the footer holds Find islands (or Stop)
//! and what the search found.
use super::{
    holder,
    plan::{
        IslandSupports, Skip, BRANCH_MM, JOIN_MM, LEVELS, MERGE_MM, PER_GROUP, PER_TRUNK,
        SKIP_UNDER_MM2,
    },
    Island, IslandsUi, ANGLE_DEG, HIDE_UNDER_MM2,
};
use crate::{
    appearance::Appearance,
    i18n::{decimal, tr},
    icons::{Icon, Icons},
    list_window::{self, Event, Row, Spec},
    preparation_ui::PreparationUi,
    supports_ui::{
        panel::{fit_fields, shape_field},
        SupportPreset, SupportsUi, Weight,
    },
    theme,
    widgets::{
        action_pair, default_row, default_switch, fact_row, icon_button, length_default,
        state_line, switch_default, DefaultValue, Unit, UnitField,
    },
    workspace::Workspace,
};
use eframe::egui;

/// Why a support was not planned, in a few words: shared by the Islands and Automatic tools.
pub(crate) fn reason(skip: Skip) -> String {
    match skip {
        Skip::NoSurface => tr!("auto-left-no-surface"),
        Skip::OutOfReach => tr!("auto-left-out-of-reach"),
        Skip::NoWay => tr!("auto-left-no-way"),
        Skip::Unbuildable => tr!("auto-left-unbuildable"),
        Skip::TooLow => tr!("auto-left-too-low"),
        Skip::DrainHole => tr!("auto-left-drain-hole"),
        Skip::FootInHole => tr!("auto-left-foot-in-hole"),
        Skip::Collides => tr!("auto-left-collides"),
        Skip::Pierces => tr!("auto-left-pierces"),
    }
}

/// A preset's name: the user's own, or the weight's.
fn preset_name((weight, mine): (Weight, Option<usize>), presets: &[SupportPreset]) -> String {
    match mine.and_then(|i| presets.get(i)) {
        Some(preset) => preset.name.clone(),
        None => weight.label(),
    }
}

/// A preset's row in `ui`, `label` and `hint` its own: a weight, or one of the user's
/// `presets`, as `chosen` holds it; `salt` tells its menu from another's in the same place;
/// with `default`, a reset button while another is chosen. Returns whether it changed.
pub(crate) fn preset_row(
    ui: &mut egui::Ui,
    (label, hint): (String, String),
    chosen: &mut (Weight, Option<usize>),
    presets: &[SupportPreset],
    (salt, default): (&str, Option<(Weight, Option<usize>)>),
) -> bool {
    let default = default.map(|d| DefaultValue::new(d, preset_name(d, presets)));
    default_row(ui, label, chosen, default.as_ref(), |ui, chosen| {
        let mut picked = false;
        let mut response = egui::ComboBox::from_id_salt(ui.id().with(salt))
            .width(ui.available_width())
            .truncate()
            .selected_text(preset_name(*chosen, presets))
            .show_ui(ui, |ui| {
                for weight in Weight::ALL {
                    let on = chosen.1.is_none() && chosen.0 == weight;
                    if ui.selectable_label(on, weight.label()).clicked() {
                        *chosen = (weight, None);
                        picked = true;
                    }
                }
                for (i, preset) in presets.iter().enumerate() {
                    if ui
                        .selectable_label(chosen.1 == Some(i), &preset.name)
                        .clicked()
                    {
                        chosen.1 = Some(i);
                        picked = true;
                    }
                }
            })
            .response
            .on_hover_text(hint.clone());
        if picked {
            response.mark_changed();
        }
        response
    })
}

/// A count's row: its label and hint, its field within `range`, typed only when `enabled`;
/// with `default`, a reset button while it differs. Returns whether it changed.
pub(crate) fn count_row(
    ui: &mut egui::Ui,
    (label, hint): (String, String),
    count: &mut u32,
    (range, enabled): (std::ops::RangeInclusive<f64>, bool),
    default: Option<u32>,
) -> bool {
    let mut value = f64::from(*count);
    let default = default.map(|d| DefaultValue::new(f64::from(d), d.to_string()));
    let changed = default_row(ui, label, &mut value, default.as_ref(), |ui, value| {
        ui.add_enabled(
            enabled,
            UnitField::new(value, Unit::Count)
                .range(range)
                .step(1.0)
                .decimals(0),
        )
        .on_hover_text(hint)
    });
    if changed {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // A few, in range.
        let rounded = value.round() as u32;
        *count = rounded;
    }
    changed
}

/// The contact's shape over the preset's, and whether it fits the surface; with `defaults`,
/// each one's reset button. Returns whether any changed.
fn contact_fields(
    ui: &mut egui::Ui,
    values: &mut IslandSupports,
    presets: &[SupportPreset],
    defaults: Option<&IslandSupports>,
) -> bool {
    let mut edited = shape_field(
        ui,
        &mut values.sphere_contact,
        defaults.map(|d| d.sphere_contact),
    );
    let contact = values.next(presets).values.contact_diameter_mm;
    edited |= fit_fields(
        ui,
        &mut values.contact_fit_min_mm,
        contact,
        defaults.map(|d| d.contact_fit_min_mm),
    );
    edited
}

/// How the islands' supports are made, in `ui`: their preset (a weight or one of the user's
/// `presets`), the trees, with `advanced` the join distance and the smallest islands
/// left out, and whether a foot may stand on the model. Shared by the panel and the settings'
/// defaults; in the panel, `defaults` gives each field its reset button. Returns whether any
/// changed.
pub fn island_fields(
    ui: &mut egui::Ui,
    values: &mut IslandSupports,
    presets: &[SupportPreset],
    advanced: bool,
    defaults: Option<&IslandSupports>,
) -> bool {
    let mut chosen = (values.weight, values.mine);
    let mut edited = preset_row(
        ui,
        (tr!("islands-preset"), tr!("islands-preset-hint")),
        &mut chosen,
        presets,
        ("islands-preset", defaults.map(|d| (d.weight, d.mine))),
    );
    (values.weight, values.mine) = chosen;
    edited |= contact_fields(ui, values, presets, defaults);
    let length = |d: Option<f64>, unit| d.map(|d| length_default(d, 1, unit));
    let field = |ui: &mut egui::Ui,
                 (label, hint): (String, String),
                 value: &mut f64,
                 (unit, range): (Unit, std::ops::RangeInclusive<f64>),
                 default: Option<DefaultValue<f64>>| {
        default_row(ui, label, value, default.as_ref(), |ui, value| {
            ui.add(UnitField::new(value, unit).range(range).step(0.1))
                .on_hover_text(hint)
        })
    };
    edited |= field(
        ui,
        (tr!("islands-branch"), tr!("islands-branch-hint")),
        &mut values.branch_mm,
        (Unit::Millimetre, BRANCH_MM),
        length(defaults.map(|d| d.branch_mm), Unit::Millimetre),
    );
    edited |= count_row(
        ui,
        (tr!("islands-per-trunk"), tr!("islands-per-trunk-hint")),
        &mut values.per_trunk,
        (PER_TRUNK, values.branch_mm > 0.0),
        defaults.map(|d| d.per_trunk),
    );
    edited |= field(
        ui,
        (tr!("islands-merge"), tr!("islands-merge-hint")),
        &mut values.merge_mm,
        (Unit::Millimetre, MERGE_MM),
        length(defaults.map(|d| d.merge_mm), Unit::Millimetre),
    );
    let merging = values.merge_mm > 0.0;
    edited |= count_row(
        ui,
        (tr!("islands-per-group"), tr!("islands-per-group-hint")),
        &mut values.per_group,
        (PER_GROUP, merging),
        defaults.map(|d| d.per_group),
    );
    edited |= count_row(
        ui,
        (tr!("islands-levels"), tr!("islands-levels-hint")),
        &mut values.levels,
        (LEVELS, merging),
        defaults.map(|d| d.levels),
    );
    if advanced {
        // Islands share a support only when asked (owner, 2026-10-30: none by default).
        edited |= field(
            ui,
            (tr!("islands-join"), tr!("islands-join-hint")),
            &mut values.join_mm,
            (Unit::Millimetre, JOIN_MM),
            length(defaults.map(|d| d.join_mm), Unit::Millimetre),
        );
        edited |= field(
            ui,
            (tr!("islands-skip-under"), tr!("islands-skip-under-hint")),
            &mut values.skip_under_mm2,
            (Unit::SquareMillimetre, SKIP_UNDER_MM2),
            length(defaults.map(|d| d.skip_under_mm2), Unit::SquareMillimetre),
        );
    }
    edited |= default_switch(
        ui,
        &mut values.foot_on_model,
        (
            tr!("islands-foot-on-model"),
            tr!("islands-foot-on-model-hint"),
        ),
        defaults.map(|d| switch_default(d.foot_on_model)).as_ref(),
    );
    edited |= default_switch(
        ui,
        &mut values.minis,
        (
            tr!("supports-minis-fallback"),
            tr!("supports-minis-fallback-hint"),
        ),
        defaults.map(|d| switch_default(d.minis)).as_ref(),
    );
    edited
}

/// An island's layers in a few words: one layer, or the first and last.
fn layers(island: &Island) -> String {
    let [first, last] = island.layers.map(|k| k + 1);
    if first == last {
        first.to_string()
    } else {
        tr!("islands-layer-span", first = first, last = last)
    }
}

/// An island's support in a few words, and its colour.
fn held(
    workspace: &Workspace,
    index: usize,
    island: &Island,
    join_mm: f64,
) -> (String, egui::Color32) {
    match holder(workspace, index, island, join_mm) {
        Some(id) => (tr!("islands-held", support = id), theme::SUCCESS.color()),
        None => (tr!("islands-free"), theme::ERROR.color()),
    }
}

impl IslandsUi {
    /// The panel for model `index`: the search's values, then the chosen island's facts.
    #[allow(clippy::too_many_arguments)] // The panel's whole context.
    pub fn panel(
        &mut self,
        ui: &mut egui::Ui,
        workspace: &mut Workspace,
        preparation: &PreparationUi,
        supports: &mut SupportsUi,
        index: Option<usize>,
        icons: Option<&Icons>,
    ) {
        let index = index.filter(|&i| i < workspace.plate().objects.len());
        ui.add_enabled_ui(index.is_some(), |ui| {
            crate::widgets::group(ui, tr!("islands-search"), |ui| {
                let angle = length_default(self.default_angle_deg, 0, Unit::Degree);
                default_row(
                    ui,
                    tr!("islands-angle"),
                    &mut self.angle_deg,
                    Some(&angle),
                    |ui, angle| {
                        ui.add(
                            UnitField::new(angle, Unit::Degree)
                                .range(ANGLE_DEG)
                                .step(1.0)
                                .decimals(0),
                        )
                        .on_hover_text(tr!("islands-angle-hint"))
                    },
                );
                if self.advanced {
                    let none = length_default(0.0, 2, Unit::SquareMillimetre);
                    default_row(
                        ui,
                        tr!("islands-hide-under"),
                        &mut self.hide_under_mm2,
                        Some(&none),
                        |ui, area| {
                            ui.add(
                                UnitField::new(area, Unit::SquareMillimetre)
                                    .range(HIDE_UNDER_MM2)
                                    .step(0.01),
                            )
                            .on_hover_text(tr!("islands-hide-under-hint"))
                        },
                    );
                }
            });
        });
        ui.add_space(4.0);
        ui.add_enabled_ui(index.is_some(), |ui| {
            crate::widgets::group(ui, tr!("islands-supports"), |ui| {
                let advanced = self.advanced;
                let presets = supports.presets.clone();
                let defaults = IslandSupports {
                    mine: supports.islands_default_mine,
                    ..supports.tool_defaults.island_supports
                };
                island_fields(
                    ui,
                    &mut supports.islands,
                    &presets,
                    advanced,
                    Some(&defaults),
                );
            });
        });
        ui.add_space(4.0);
        self.navigator(ui, workspace, preparation, supports, index, icons);
    }

    /// The places of `islands` stepping goes through, those hidden as too small skipped, and
    /// the chosen one's position among them: the chosen island, or the next one shown when it
    /// is hidden, which becomes the chosen one.
    fn stepping(&mut self, islands: Option<&[Island]>) -> (Vec<usize>, usize) {
        let shown: Vec<usize> = islands.map_or_else(Vec::new, |islands| {
            (0..islands.len())
                .filter(|&k| self.shows(&islands[k]))
                .collect()
        });
        let position = shown.iter().position(|&k| k == self.at).unwrap_or_else(|| {
            shown
                .partition_point(|&k| k < self.at)
                .min(shown.len().saturating_sub(1))
        });
        if let Some(&k) = shown.get(position) {
            self.at = k;
        }
        (shown, position)
    }

    /// The chosen island: previous and next, its facts, and "Support it here". Without islands,
    /// the same rows, disabled.
    fn navigator(
        &mut self,
        ui: &mut egui::Ui,
        workspace: &mut Workspace,
        preparation: &PreparationUi,
        supports: &mut SupportsUi,
        index: Option<usize>,
        icons: Option<&Icons>,
    ) {
        let found = self
            .found(workspace, index)
            .map(|(islands, current)| (islands.to_vec(), current));
        let (shown, position) = self.stepping(found.as_ref().map(|(i, _)| i.as_slice()));
        let count = shown.len();
        let island = shown
            .get(position)
            .and_then(|&k| found.as_ref().and_then(|(i, _)| i.get(k)))
            .copied();
        let mut go = None;
        ui.horizontal(|ui| {
            ui.add_enabled_ui(count > 1, |ui| {
                if icon_button(
                    ui,
                    icons,
                    Icon::ChevronLeft,
                    false,
                    &tr!("islands-previous"),
                )
                .clicked()
                {
                    go = Some(shown[(position + count - 1) % count]);
                }
            });
            let title = if count > 0 {
                tr!("islands-of", number = position + 1, count = count)
            } else {
                tr!("islands-island")
            };
            ui.add(egui::Label::new(egui::RichText::new(title).strong()).truncate());
            ui.add_enabled_ui(count > 1, |ui| {
                if icon_button(ui, icons, Icon::ChevronRight, false, &tr!("islands-next")).clicked()
                {
                    go = Some(shown[(position + 1) % count]);
                }
            });
        });
        if let (Some(k), Some(i)) = (go, index) {
            self.go_to(workspace, i, k, supports);
        }
        let dash = || "–".to_owned();
        let facts = island.zip(index).map(|(island, i)| {
            let (state, colour) = held(workspace, i, &island, self.join_mm);
            (island, state, colour)
        });
        let mut add = false;
        ui.add_enabled_ui(facts.is_some(), |ui| {
            let (layer, height, area, place) = facts.as_ref().map_or_else(
                || (dash(), dash(), dash(), dash()),
                |(island, _, _)| {
                    (
                        layers(island),
                        tr!("islands-height-mm", height = decimal(island.point[2], 2)),
                        tr!("islands-area-mm2", area = decimal(island.area_mm2, 2)),
                        if island.inside {
                            tr!("islands-inside")
                        } else {
                            tr!("islands-outside")
                        },
                    )
                },
            );
            fact_row(ui, &tr!("islands-layers"), &layer);
            fact_row(ui, &tr!("islands-height"), &height);
            fact_row(ui, &tr!("islands-area"), &area);
            fact_row(ui, &tr!("islands-where"), &place).on_hover_text(tr!("islands-where-hint"));
            let (state, colour) = facts.as_ref().map_or_else(
                || (dash(), theme::TEXT_PRIMARY.color()),
                |f| (f.1.clone(), f.2),
            );
            ui.horizontal(|ui| {
                ui.label(tr!("islands-support"));
                ui.label(egui::RichText::new(state).color(colour));
            });
            let free = facts.as_ref().is_some_and(|(island, _, _)| {
                index.is_some_and(|i| holder(workspace, i, island, self.join_mm).is_none())
            });
            let hint = if facts.as_ref().is_some_and(|f| f.0.inside) {
                tr!("islands-support-here-cavity-hint")
            } else {
                tr!("islands-support-here-hint")
            };
            let size = egui::vec2(ui.available_width(), ui.spacing().interact_size.y);
            add = ui
                .add_enabled(
                    free,
                    egui::Button::new(tr!("islands-support-here")).min_size(size),
                )
                .on_hover_text(hint)
                .clicked();
        });
        if let (true, Some(i)) = (add, index) {
            let _ = (i, preparation, &supports);
            self.support_here(self.at);
        }
    }

    /// The footer for model `index` (its preparation in `preparation`): Find islands (again), or
    /// Stop while searching, then what the search found.
    pub fn footer(
        &mut self,
        ui: &mut egui::Ui,
        workspace: &mut Workspace,
        preparation: &PreparationUi,
        index: Option<usize>,
        icons: Option<&Icons>,
    ) {
        let index = index.filter(|&i| i < workspace.plate().objects.len());
        if self.auto {
            let count = self.plan_outcome(index).map_or(0, |o| o.supports.len());
            let (apply, discard) = action_pair(
                ui,
                [tr!("islands-apply", count = count), tr!("islands-discard")],
                (count > 0, true),
            );
            if let (true, Some(i)) = (
                apply.on_hover_text(tr!("islands-apply-hint")).clicked(),
                index,
            ) {
                self.apply_plan(workspace, i);
            } else if discard.clicked() {
                self.toggle_supporting();
            }
            ui.add_space(4.0);
        }
        let (label, hint) = if self.search.is_some() {
            (tr!("islands-stop"), tr!("islands-stop-hint"))
        } else if self.found(workspace, index).is_some() {
            (tr!("islands-find-again"), tr!("islands-find-hint"))
        } else {
            (tr!("islands-find"), tr!("islands-find-hint"))
        };
        let size = egui::vec2(ui.available_width(), ui.spacing().interact_size.y);
        let clicked = ui
            .add_enabled(
                index.is_some() && workspace.plate().setting().is_some(),
                egui::Button::new(label).min_size(size),
            )
            .on_hover_text(hint)
            .clicked();
        if clicked {
            match (self.search.is_some(), index) {
                (true, _) => self.stop(),
                (false, Some(i)) => {
                    self.auto = false;
                    self.find(workspace, preparation, i);
                }
                (false, None) => {}
            }
        }
        ui.add_space(4.0);
        let (text, colour, icon) = self.state(workspace, index);
        let line = state_line(ui, icons, icon, &text, colour);
        if let Some(hint) = self.skipped_hint(index).filter(|_| self.auto) {
            line.on_hover_text(hint);
        }
    }

    /// What the footer says: the search running, not searched, out of date, islands without a
    /// support, or every island held.
    fn state(
        &self,
        workspace: &Workspace,
        index: Option<usize>,
    ) -> (String, egui::Color32, Option<Icon>) {
        let Some(index) = index else {
            return ("–".to_owned(), theme::BORDER.color(), None);
        };
        if workspace.plate().setting().is_none() {
            return (
                tr!("prepare-needs-profiles"),
                theme::WARNING.color(),
                Some(Icon::Warning),
            );
        }
        if let Some(search) = self.search.as_ref().filter(|s| s.object == index) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // A percentage.
            let percent = (search.progress * 100.0).round() as u32;
            return (
                tr!("islands-finding", percent = percent),
                theme::ACCENT.color(),
                Some(Icon::Info),
            );
        }
        if self.here.is_some() || self.auto {
            return self.plan_state(Some(index));
        }
        let Some((islands, current)) = self.found(workspace, Some(index)) else {
            return (tr!("islands-not-searched"), theme::BORDER.color(), None);
        };
        if !current {
            return (
                tr!("islands-out-of-date"),
                theme::ACCENT.color(),
                Some(Icon::Info),
            );
        }
        let free = islands
            .iter()
            .filter(|i| self.shows(i))
            .filter(|i| holder(workspace, index, i, self.join_mm).is_none())
            .count();
        if free > 0 {
            (
                tr!("islands-state-free", count = free),
                theme::WARNING.color(),
                Some(Icon::Warning),
            )
        } else {
            (
                tr!("islands-state-held"),
                theme::SUCCESS.color(),
                Some(Icon::Success),
            )
        }
    }

    /// What the footer says of the islands' supports being planned: the surface or the plan
    /// being made, or what Apply would add and what was skipped.
    fn plan_state(&self, index: Option<usize>) -> (String, egui::Color32, Option<Icon>) {
        let Some(outcome) = self.plan_outcome(index) else {
            let text = if self.waiting {
                tr!("islands-preparing-surface")
            } else {
                tr!("islands-planning")
            };
            return (text, theme::ACCENT.color(), Some(Icon::Info));
        };
        let skipped = outcome.skipped.len();
        if skipped == 0 {
            (
                tr!(
                    "islands-plan",
                    count = outcome.supports.len(),
                    branches = outcome.branches,
                    islands = outcome.held
                ),
                theme::ACCENT.color(),
                Some(Icon::Info),
            )
        } else {
            (
                tr!(
                    "islands-plan-skipped",
                    count = outcome.supports.len(),
                    branches = outcome.branches,
                    islands = outcome.held,
                    skipped = skipped
                ),
                theme::WARNING.color(),
                Some(Icon::Warning),
            )
        }
    }

    /// Why the islands of the plan for model `index` were skipped, a line per reason with its
    /// count, for the footer's tooltip.
    pub(super) fn skipped_hint(&self, index: Option<usize>) -> Option<String> {
        Self::skipped_of(self.plan_outcome(index)?)
    }

    /// Why `outcome`'s islands were skipped, a line per reason with its count.
    pub(super) fn skipped_of(outcome: &super::plan::Outcome) -> Option<String> {
        let count = |why: Skip| outcome.skipped.iter().filter(|(_, w)| *w == why).count();
        let lines: Vec<String> = [
            (
                Skip::OutOfReach,
                tr!("islands-skip-out-of-reach", count = count(Skip::OutOfReach)),
            ),
            (
                Skip::NoWay,
                tr!("islands-skip-no-way", count = count(Skip::NoWay)),
            ),
            (
                Skip::NoSurface,
                tr!("islands-skip-no-surface", count = count(Skip::NoSurface)),
            ),
            (
                Skip::Unbuildable,
                tr!("islands-skip-unbuildable", count = count(Skip::Unbuildable)),
            ),
            (
                Skip::TooLow,
                tr!("islands-skip-too-low", count = count(Skip::TooLow)),
            ),
            (
                Skip::DrainHole,
                tr!("islands-skip-drain-hole", count = count(Skip::DrainHole)),
            ),
            (
                Skip::FootInHole,
                tr!("islands-skip-foot-in-hole", count = count(Skip::FootInHole)),
            ),
            (
                Skip::Collides,
                tr!("islands-skip-collides", count = count(Skip::Collides)),
            ),
            (
                Skip::Pierces,
                tr!("islands-skip-pierces", count = count(Skip::Pierces)),
            ),
        ]
        .into_iter()
        .filter(|(why, _)| count(*why) > 0)
        .map(|(_, line)| line)
        .collect();
        (!lines.is_empty()).then(|| lines.join("\n"))
    }

    /// The places of `islands` the list shows: not hidden as too small, without a support (by
    /// `holders`) when asked, and in the place chosen.
    pub(super) fn shown(&self, islands: &[Island], holders: &[Option<u64>]) -> Vec<usize> {
        (0..islands.len())
            .filter(|&k| self.shows(&islands[k]))
            .filter(|&k| !self.only_free || holders[k].is_none())
            .filter(|&k| match self.place {
                1 => !islands[k].inside,
                2 => islands[k].inside,
                _ => true,
            })
            .collect()
    }

    /// The list window's summary: whether model `index`'s islands were searched and are
    /// `current`, and how many are hidden as too small (`hidden`).
    fn list_summary(
        &self,
        workspace: &Workspace,
        index: Option<usize>,
        current: bool,
        hidden: Option<String>,
    ) -> String {
        if index.is_none() {
            tr!("panel-no-model")
        } else if self.found(workspace, index).is_none() {
            tr!("islands-list-not-searched")
        } else {
            let state = if current {
                tr!("islands-list-current")
            } else {
                tr!("islands-out-of-date")
            };
            hidden.map_or_else(|| state.clone(), |h| format!("{state} · {h}"))
        }
    }

    /// The list window of model `index`'s islands, while open, by place: outside, or on the
    /// cavity's ceiling, where too the resin cures in the vat. Returns where the view should
    /// go, after a double click on a row.
    pub fn list_window(
        &mut self,
        ctx: &egui::Context,
        workspace: &Workspace,
        supports: &mut SupportsUi,
        index: Option<usize>,
        appearance: &Appearance,
    ) -> Option<[[f64; 3]; 2]> {
        if !self.list_open {
            return None;
        }
        let index = index.filter(|&i| i < workspace.plate().objects.len());
        let (islands, current) = self
            .found(workspace, index)
            .map_or((Vec::new(), true), |(i, c)| (i.to_vec(), c));
        let holders: Vec<Option<u64>> = islands
            .iter()
            .map(|i| index.and_then(|o| holder(workspace, o, i, self.join_mm)))
            .collect();
        // The counts are of the islands shown; the summary says how many are hidden.
        let visible: Vec<usize> = (0..islands.len())
            .filter(|&k| self.shows(&islands[k]))
            .collect();
        let free = visible.iter().filter(|&&k| holders[k].is_none()).count();
        let inside = visible.iter().filter(|&&k| islands[k].inside).count();
        let hidden = self.hidden_note(&islands);
        let items = self.shown(&islands, &holders);
        let reveal = std::mem::take(&mut self.reveal)
            .then_some(self.at)
            .filter(|&k| k < islands.len());
        let spec = Spec {
            id: "islands-list",
            title: tr!("islands-list-title", count = visible.len()),
            fact: tr!("islands-list-fact"),
            state: tr!("list-state"),
            tabs: Vec::new(),
            filters: vec![tr!("list-all"), tr!("islands-list-free", count = free)],
            places: vec![
                tr!("islands-list-everywhere"),
                tr!("islands-list-outside", count = visible.len() - inside),
                tr!("islands-list-cavity", count = inside),
            ],
            summary: self.list_summary(workspace, index, current, hidden),
            remove: None,
        };
        let at = self.at;
        let row = |k: usize| {
            let island = islands[k];
            let held = holders[k];
            let place = if island.inside {
                tr!("islands-list-inside")
            } else {
                String::new()
            };
            Row {
                // In the colour the scene draws it: the chosen one in the selection's.
                dot: if k == at {
                    appearance.support_selected.color()
                } else if held.is_some() {
                    appearance.hole_valid.color()
                } else {
                    appearance.support_error.color()
                },
                name: tr!("list-number", number = k + 1),
                fact: tr!(
                    "islands-list-row",
                    layer = island.layers[0] + 1,
                    area = decimal(island.area_mm2, 2),
                    place = place
                ),
                state: match held {
                    Some(id) => tr!("islands-held", support = id),
                    None => tr!("islands-free"),
                },
                state_colour: if held.is_some() {
                    theme::SUCCESS.color()
                } else {
                    theme::ERROR.color()
                },
                hint: tr!("list-row-hint"),
                selected: k == at,
            }
        };
        let mut filter = usize::from(self.only_free);
        let event = list_window::show(
            ctx,
            &spec,
            &mut self.list_open,
            &mut 0,
            &mut filter,
            &mut self.place,
            &items,
            reveal,
            row,
        );
        self.only_free = filter == 1;
        let object = index?;
        match event? {
            Event::Picked(k) => {
                self.at = k;
                None
            }
            // Framed with the section through it, as stepping to it does: the window takes it.
            Event::Focused(k) => {
                self.go_to(workspace, object, k, supports);
                None
            }
            Event::Removed(_) => None,
        }
    }
}
