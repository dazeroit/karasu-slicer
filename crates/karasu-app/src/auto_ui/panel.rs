//! The Automatic tool's panel and footer (owner mockup, 2026-10-30). The panel holds what the
//! plan covers, how its supports are made, its trees and braces, then what it planned or added
//! and the places it left; the footer holds Apply and Discard over Generate (or Stop), and the
//! state.
use super::{
    plan::{
        AutoSupports, Cover, Place, DENSITY_PCT, LIFT_MM, PASSES, SPACING_MM, WIDE_CLOSEST_PCT,
        WIDE_FROM_MM2,
    },
    AutoUi, Stage,
};
use crate::{
    appearance::Appearance,
    i18n::{decimal, tr},
    icons::{Icon, Icons},
    islands_ui::{
        panel::{count_row, preset_row, reason},
        plan::{BRANCH_MM, JOIN_MM, LEVELS, MERGE_MM, PER_GROUP, PER_TRUNK, SKIP_UNDER_MM2},
        ANGLE_DEG,
    },
    list_window::{self, Event, Row, Spec},
    supports_ui::{
        braces::{brace_fields, NewBraces},
        panel::{fit_fields, shape_field},
        SupportPreset, SupportsUi,
    },
    theme,
    widgets::{
        action_pair, default_choice, default_row, default_switch, fact_row, length_default,
        range_slider, state_line, switch_default, DefaultValue, Unit, UnitField,
    },
    workspace::Workspace,
};
use eframe::egui;
use karasu_core::supports::SupportKind;

/// A length's row within `range`, stepped by a tenth, with a reset to `default` when given.
/// Returns whether it changed.
pub(crate) fn length_row(
    ui: &mut egui::Ui,
    (label, hint): (String, String),
    value: &mut f64,
    (unit, range): (Unit, std::ops::RangeInclusive<f64>),
    default: Option<f64>,
) -> bool {
    let default = default.map(|d| length_default(d, 1, unit));
    default_row(ui, label, value, default.as_ref(), |ui, value| {
        ui.add(UnitField::new(value, unit).range(range).step(0.1))
            .on_hover_text(hint)
    })
}

/// A share as a whole percentage.
fn percent([part, whole]: [usize; 2]) -> String {
    #[allow(clippy::cast_precision_loss)] // Counts of points.
    let share = if whole == 0 {
        100.0
    } else {
        100.0 * part as f64 / whole as f64
    };
    decimal(share.floor(), 0)
}

/// What the plan covers, in `ui`: islands and overhangs or islands only, where (offered when
/// `hollowed`: everywhere, outside or the cavity), and with `advanced` the smallest islands left
/// out; with `defaults`, each field's reset button. Returns whether any changed.
fn cover_fields(
    ui: &mut egui::Ui,
    values: &mut AutoSupports,
    (hollowed, advanced): (bool, bool),
    defaults: Option<&AutoSupports>,
) -> bool {
    let covers = [tr!("auto-cover-all"), tr!("auto-cover-islands")];
    let cover_of = |cover: Cover| usize::from(cover == Cover::IslandsOnly);
    let mut chosen = cover_of(values.cover);
    let default =
        defaults.map(|d| DefaultValue::new(cover_of(d.cover), covers[cover_of(d.cover)].clone()));
    let mut edited = default_choice(ui, &mut chosen, &covers, default.as_ref())
        .on_hover_text(tr!("auto-cover-hint"))
        .changed();
    values.cover = if chosen == 1 {
        Cover::IslandsOnly
    } else {
        Cover::All
    };
    let places = [Place::Everywhere, Place::Outside, Place::Cavity];
    let labels = [
        tr!("auto-place-everywhere"),
        tr!("auto-place-outside"),
        tr!("auto-place-cavity"),
    ];
    let place_of = |place: Place| places.iter().position(|&p| p == place).unwrap_or(0);
    let mut place = place_of(values.place);
    let default =
        defaults.map(|d| DefaultValue::new(place_of(d.place), labels[place_of(d.place)].clone()));
    ui.add_enabled_ui(hollowed, |ui| {
        edited |= default_choice(ui, &mut place, &labels, default.as_ref())
            .on_hover_text(tr!("auto-place-hint"))
            .on_disabled_hover_text(tr!("auto-place-not-hollowed"))
            .changed();
    });
    values.place = places[place];
    if advanced {
        edited |= length_row(
            ui,
            (tr!("islands-skip-under"), tr!("auto-skip-under-hint")),
            &mut values.skip_under_mm2,
            (Unit::SquareMillimetre, SKIP_UNDER_MM2),
            defaults.map(|d| d.skip_under_mm2),
        );
    }
    edited
}

/// Every value a plan starts from, in `ui`, as the settings set them: what it covers, how its
/// supports are made and its trees and braces. Returns whether any changed.
pub fn default_fields(
    ui: &mut egui::Ui,
    values: &mut AutoSupports,
    presets: &[SupportPreset],
) -> bool {
    let mut edited = cover_fields(ui, values, (true, true), None);
    edited |= supports_fields(ui, values, presets, (true, true), None);
    edited |= trees_fields(ui, values, true, None);
    edited
}

/// How the plan's supports are made, in `ui`, with `advanced` the finer values: their preset,
/// spacing, density, for a `hollowed` model the cavity's own, and feet on the model. Shared by
/// the panel and the settings' defaults; in the panel, `defaults` gives each field its reset
/// button. Returns whether any changed.
pub fn supports_fields(
    ui: &mut egui::Ui,
    values: &mut AutoSupports,
    presets: &[SupportPreset],
    (advanced, hollowed): (bool, bool),
    defaults: Option<&AutoSupports>,
) -> bool {
    let mut chosen = (values.weight, values.mine);
    let mut edited = preset_row(
        ui,
        (tr!("islands-preset"), tr!("islands-preset-hint")),
        &mut chosen,
        presets,
        ("automatic-preset", defaults.map(|d| (d.weight, d.mine))),
    );
    (values.weight, values.mine) = chosen;
    // The model's lift, which Generate makes (owner, 2026-11-04, TD-128).
    edited |= length_row(
        ui,
        (tr!("auto-lift"), tr!("auto-lift-hint")),
        &mut values.lift_mm,
        (Unit::Millimetre, LIFT_MM),
        defaults.map(|d| d.lift_mm),
    );
    edited |= shape_field(
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
    edited |= length_row(
        ui,
        (tr!("auto-spacing"), tr!("auto-spacing-hint")),
        &mut values.spacing_mm,
        (Unit::Millimetre, SPACING_MM),
        defaults.map(|d| d.spacing_mm),
    );
    if advanced {
        edited |= length_row(
            ui,
            (tr!("auto-steep-spacing"), tr!("auto-steep-spacing-hint")),
            &mut values.steep_spacing_mm,
            (Unit::Millimetre, values.spacing_mm..=*SPACING_MM.end()),
            defaults.map(|d| d.steep_spacing_mm),
        );
        edited |= length_row(
            ui,
            (tr!("islands-join"), tr!("islands-join-hint")),
            &mut values.join_mm,
            (Unit::Millimetre, JOIN_MM),
            defaults.map(|d| d.join_mm),
        );
        edited |= count_row(
            ui,
            (tr!("auto-passes"), tr!("auto-passes-hint")),
            &mut values.passes,
            (PASSES, true),
            defaults.map(|d| d.passes),
        );
    }
    values.steep_spacing_mm = values.steep_spacing_mm.max(values.spacing_mm);
    edited |= density_fields(ui, values, defaults);
    if hollowed {
        edited |= cavity_fields(ui, values, presets, defaults);
    }
    edited |= default_switch(
        ui,
        &mut values.feet_on_model,
        (tr!("auto-feet"), tr!("auto-feet-hint")),
        defaults.map(|d| switch_default(d.feet_on_model)).as_ref(),
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

/// The tips' density and how much closer they stand under wide undersides (TD-127), in `ui`;
/// with `defaults`, each field's reset button. Returns whether any changed.
fn density_fields(
    ui: &mut egui::Ui,
    values: &mut AutoSupports,
    defaults: Option<&AutoSupports>,
) -> bool {
    let mut edited = density_row(
        ui,
        (tr!("auto-density"), tr!("auto-density-hint")),
        &mut values.density_pct,
        defaults.map(|d| d.density_pct),
    );
    let scale = values.density_scale();
    ui.add(
        egui::Label::new(
            egui::RichText::new(tr!(
                "auto-density-spacing",
                level = decimal(values.spacing_mm * scale, 2),
                steep = decimal(values.steep_spacing_mm * scale, 2)
            ))
            .small(),
        )
        .wrap(),
    );
    edited |= default_switch(
        ui,
        &mut values.wide,
        (tr!("auto-wide"), tr!("auto-wide-hint")),
        defaults.map(|d| switch_default(d.wide)).as_ref(),
    );
    let on = values.wide;
    crate::widgets::depends(ui, on, |ui| {
        edited |= length_row(
            ui,
            (tr!("auto-wide-from"), tr!("auto-wide-from-hint")),
            &mut values.wide_from_mm2,
            (Unit::SquareMillimetre, WIDE_FROM_MM2),
            defaults.map(|d| d.wide_from_mm2),
        );
        let default = defaults.map(|d| length_default(d.wide_closest_pct, 0, Unit::Percent));
        edited |= default_row(
            ui,
            tr!("auto-wide-closest"),
            &mut values.wide_closest_pct,
            default.as_ref(),
            |ui, value| {
                ui.add(
                    UnitField::new(value, Unit::Percent)
                        .range(WIDE_CLOSEST_PCT)
                        .step(5.0)
                        .decimals(0),
                )
                .on_hover_text(tr!("auto-wide-closest-hint"))
            },
        );
    });
    edited
}

/// A density's row in `ui`, `label` and `hint` its own: its field, with the app's slider under
/// it (owner rule: never egui's), in whole percent; with `default`, a reset button. Returns
/// whether it changed.
fn density_row(
    ui: &mut egui::Ui,
    (label, hint): (String, String),
    density_pct: &mut f64,
    default: Option<f64>,
) -> bool {
    let default = default.map(|d| length_default(d, 0, Unit::Percent));
    let mut edited = default_row(ui, &label, density_pct, default.as_ref(), |ui, value| {
        ui.add(
            UnitField::new(value, Unit::Percent)
                .range(DENSITY_PCT)
                .step(10.0)
                .decimals(0),
        )
        .on_hover_text(hint.clone())
    });
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // 50 to 200.
    let mut slid = density_pct.round() as usize;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // 50 to 200.
    let span = *DENSITY_PCT.start() as usize..=*DENSITY_PCT.end() as usize;
    let width = ui.available_width();
    let slider = range_slider(ui, &mut slid, span, width).on_hover_text(hint);
    crate::widgets::name(&slider, &label);
    if slider.changed() {
        #[allow(clippy::cast_precision_loss)] // 50 to 200.
        {
            *density_pct = slid as f64;
        }
        edited = true;
    }
    edited
}

/// The cavity's own preset and density (owner, 2026-11-06, TD-137), in `ui`: the switch, then
/// with it on the two values; with `defaults`, each field's reset button. Returns whether any
/// changed. Switched on, the cavity starts from the outside's values.
fn cavity_fields(
    ui: &mut egui::Ui,
    values: &mut AutoSupports,
    presets: &[SupportPreset],
    defaults: Option<&AutoSupports>,
) -> bool {
    let was = values.cavity_own;
    let mut edited = default_switch(
        ui,
        &mut values.cavity_own,
        (tr!("auto-cavity-own"), tr!("auto-cavity-own-hint")),
        defaults.map(|d| switch_default(d.cavity_own)).as_ref(),
    );
    if values.cavity_own && !was {
        values.cavity_weight = values.weight;
        values.cavity_mine = values.mine;
        values.cavity_density_pct = values.density_pct;
    }
    let on = values.cavity_own;
    crate::widgets::depends(ui, on, |ui| {
        let mut chosen = (values.cavity_weight, values.cavity_mine);
        edited |= preset_row(
            ui,
            (tr!("auto-cavity-preset"), tr!("auto-cavity-preset-hint")),
            &mut chosen,
            presets,
            (
                "automatic-cavity-preset",
                defaults.map(|d| (d.cavity_weight, d.cavity_mine)),
            ),
        );
        (values.cavity_weight, values.cavity_mine) = chosen;
        edited |= density_row(
            ui,
            (tr!("auto-cavity-density"), tr!("auto-cavity-density-hint")),
            &mut values.cavity_density_pct,
            defaults.map(|d| d.cavity_density_pct),
        );
    });
    edited
}

/// The plan's trees and braces, in `ui`, with `advanced` the merging; with `defaults`, each
/// field's reset button. Returns whether any changed.
pub fn trees_fields(
    ui: &mut egui::Ui,
    values: &mut AutoSupports,
    advanced: bool,
    defaults: Option<&AutoSupports>,
) -> bool {
    let mut edited = default_switch(
        ui,
        &mut values.branches,
        (tr!("auto-branches"), tr!("auto-branches-hint")),
        defaults.map(|d| switch_default(d.branches)).as_ref(),
    );
    let on = values.branches;
    crate::widgets::depends(ui, on, |ui| {
        edited |= length_row(
            ui,
            (tr!("islands-branch"), tr!("islands-branch-hint")),
            &mut values.branch_mm,
            (
                Unit::Millimetre,
                *BRANCH_MM.start() + 0.5..=*BRANCH_MM.end(),
            ),
            defaults.map(|d| d.branch_mm),
        );
        edited |= count_row(
            ui,
            (tr!("islands-per-trunk"), tr!("islands-per-trunk-hint")),
            &mut values.per_trunk,
            (PER_TRUNK, true),
            defaults.map(|d| d.per_trunk),
        );
        if advanced {
            edited |= length_row(
                ui,
                (tr!("islands-merge"), tr!("islands-merge-hint")),
                &mut values.merge_mm,
                (Unit::Millimetre, MERGE_MM),
                defaults.map(|d| d.merge_mm),
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
        }
    });
    edited |= default_switch(
        ui,
        &mut values.braces,
        (tr!("auto-braces"), tr!("auto-braces-hint")),
        defaults.map(|d| switch_default(d.braces)).as_ref(),
    );
    edited
}

/// The braces' values in `ui`, the Braces tool's own (owner, 2026-10-30): with `advanced` every
/// one, as the Braces tool and the settings show them, with `defaults`' reset buttons;
/// otherwise a line saying them.
fn braces_values(
    ui: &mut egui::Ui,
    braces: &mut NewBraces,
    advanced: bool,
    defaults: Option<&NewBraces>,
) {
    if advanced {
        brace_fields(ui, braces, defaults);
    } else {
        let diameter = if braces.auto_diameter {
            tr!(
                "auto-braces-diameter-auto",
                diameter = decimal(braces.diameter_mm, 2)
            )
        } else {
            tr!(
                "auto-braces-diameter",
                diameter = decimal(braces.diameter_mm, 2)
            )
        };
        let line = tr!(
            "auto-braces-summary",
            pattern = braces.pattern.label(),
            lowest = decimal(braces.lowest_mm, 1),
            step = decimal(braces.step_mm, 1),
            distance = decimal(braces.max_distance_mm, 1),
            diameter = diameter
        );
        ui.add(
            egui::Label::new(egui::RichText::new(line).color(ui.visuals().weak_text_color()))
                .wrap(),
        )
        .on_hover_text(tr!("auto-braces-summary-hint"));
    }
}

/// What a plan or Apply reports in the panel.
struct Facts {
    supports: usize,
    /// The mini supports among them (owner, 2026-10-30).
    minis: usize,
    branches: usize,
    braces: usize,
    islands: [usize; 2],
    overhangs: [usize; 2],
    cover: Cover,
    passes: Option<(u32, usize)>,
    seconds: Option<f64>,
}

impl AutoUi {
    /// The panel for model `index`: what is covered, how supports are made (`supports` holds
    /// the values, with the presets they name), trees and braces, then the plan's or Apply's
    /// facts and the places left.
    pub fn panel(
        &mut self,
        ui: &mut egui::Ui,
        workspace: &Workspace,
        supports: &mut SupportsUi,
        index: Option<usize>,
    ) {
        let presets = supports.presets.clone();
        let defaults = AutoSupports {
            mine: supports.automatic_default_mine,
            cavity_mine: supports.automatic_cavity_default_mine,
            ..supports.tool_defaults.auto_supports
        };
        let braces_default = supports.tool_defaults.braces;
        let (values, braces) = (&mut supports.automatic, &mut supports.bracing.new);
        let index = index.filter(|&i| i < workspace.plate().objects.len());
        let hollowed = index
            .and_then(|i| workspace.plate().objects.get(i))
            .is_some_and(|o| o.hollowing().is_some());
        let editable = index.is_some() && !self.planning();
        let advanced = self.advanced;
        let angle_deg = &mut self.angle_deg;
        let default_angle = self.default_angle_deg;
        ui.add_enabled_ui(editable, |ui| {
            crate::widgets::group(ui, tr!("auto-cover"), |ui| {
                cover_fields(ui, values, (hollowed, advanced), Some(&defaults));
                let angle = length_default(default_angle, 0, Unit::Degree);
                default_row(
                    ui,
                    tr!("islands-angle"),
                    angle_deg,
                    Some(&angle),
                    |ui, angle| {
                        ui.add(
                            UnitField::new(angle, Unit::Degree)
                                .range(ANGLE_DEG)
                                .step(1.0)
                                .decimals(0),
                        )
                        .on_hover_text(tr!("auto-angle-hint"))
                    },
                );
            });
            crate::widgets::group(ui, tr!("auto-supports"), |ui| {
                supports_fields(ui, values, &presets, (advanced, hollowed), Some(&defaults));
            });
            crate::widgets::group(ui, tr!("auto-trees"), |ui| {
                trees_fields(ui, values, advanced, Some(&defaults));
                let on = values.braces;
                crate::widgets::depends(ui, on, |ui| {
                    braces_values(ui, braces, advanced, Some(&braces_default));
                });
            });
        });
        self.result(ui, workspace, index);
    }

    /// The plan's facts (or Apply's), and the places left unsupported with Show.
    fn result(&mut self, ui: &mut egui::Ui, workspace: &Workspace, index: Option<usize>) {
        let (title, facts, left) = if let Some((planned, _)) = self
            .plan_of(workspace, index)
            .filter(|(_, current)| *current)
        {
            let o = &planned.outcome;
            (
                tr!("auto-planned"),
                Facts {
                    supports: o.plan.supports.len(),
                    minis: o
                        .plan
                        .supports
                        .iter()
                        .filter(|p| p.routed.support.kind == SupportKind::Mini)
                        .count(),
                    branches: o.plan.branches,
                    braces: o.braces.len(),
                    islands: o.islands,
                    overhangs: o.overhangs,
                    cover: planned.key.values.cover,
                    passes: Some((o.passes, o.added)),
                    seconds: Some(planned.seconds),
                },
                o.left.clone(),
            )
        } else if let Some(applied) = self.applied_of(workspace, index) {
            (
                tr!("auto-applied"),
                Facts {
                    supports: applied.supports,
                    minis: applied.minis,
                    branches: applied.branches,
                    braces: applied.braces,
                    islands: applied.islands,
                    overhangs: applied.overhangs,
                    cover: Cover::All,
                    passes: None,
                    seconds: None,
                },
                applied.left.clone(),
            )
        } else {
            return;
        };
        crate::widgets::group(ui, title, |ui| {
            fact_row(ui, &tr!("auto-supports-count"), &facts.supports.to_string());
            if facts.minis > 0 {
                fact_row(ui, &tr!("auto-minis-count"), &facts.minis.to_string());
            }
            fact_row(ui, &tr!("auto-branches-count"), &facts.branches.to_string());
            fact_row(ui, &tr!("auto-braces-count"), &facts.braces.to_string());
            let [held, wanted] = facts.islands;
            fact_row(
                ui,
                &tr!("auto-islands-count"),
                &tr!("auto-of", part = held, whole = wanted),
            );
            let overhangs = if facts.cover == Cover::IslandsOnly {
                tr!("auto-overhangs-not-covered")
            } else {
                tr!("auto-overhangs-covered", percent = percent(facts.overhangs))
            };
            fact_row(ui, &tr!("auto-overhangs-count"), &overhangs);
            if let Some((passes, added)) = facts.passes {
                fact_row(
                    ui,
                    &tr!("auto-check"),
                    &tr!("auto-check-passes", passes = passes, added = added),
                );
            }
            if let Some(seconds) = facts.seconds {
                fact_row(
                    ui,
                    &tr!("auto-took"),
                    &tr!("auto-seconds", seconds = decimal(seconds, 1)),
                );
            }
            // The places left are counted here and listed in their window.
            let places = if left.is_empty() {
                tr!("auto-left-none")
            } else {
                tr!("auto-left-count", count = left.len())
            };
            let line = fact_row(ui, &tr!("auto-left"), &places);
            if !left.is_empty() {
                line.on_hover_text(tr!("auto-left-hint"));
            }
        });
    }

    /// The list window of the places left on model `index` (owner, 2026-10-30, as the other
    /// tools' lists): each place, what it is, its height and why; a double click brings the view
    /// to it. Returns where the view should go.
    pub fn list_window(
        &mut self,
        ctx: &egui::Context,
        workspace: &Workspace,
        index: Option<usize>,
        appearance: &Appearance,
    ) -> Option<[[f64; 3]; 2]> {
        if !self.list_open {
            return None;
        }
        let index = index.filter(|&i| i < workspace.plate().objects.len());
        let left = self.left_of(workspace, index).to_vec();
        let islands = left.iter().filter(|l| l.island).count();
        let items: Vec<usize> = (0..left.len())
            .filter(|&k| !self.only_islands || left[k].island)
            .collect();
        let planned = self.plan_of(workspace, index).is_some_and(|(_, c)| c)
            || self.applied_of(workspace, index).is_some();
        let spec = Spec {
            id: "automatic-left-list",
            title: tr!("auto-list-title", count = left.len()),
            fact: tr!("auto-list-fact"),
            state: tr!("auto-list-state"),
            filters: vec![tr!("list-all"), tr!("auto-list-islands", count = islands)],
            summary: if index.is_none() {
                tr!("panel-no-model")
            } else if planned {
                tr!("auto-list-current")
            } else {
                tr!("auto-list-none")
            },
            tabs: Vec::new(),
            places: Vec::new(),
            remove: None,
        };
        let at = self.at;
        let row = |k: usize| {
            let place = left[k];
            let height = decimal(place.point[2], 2);
            Row {
                dot: if Some(k) == at {
                    appearance.support_selected.color()
                } else {
                    appearance.marker_hollowing.color()
                },
                name: tr!("list-number", number = k + 1),
                fact: if place.island {
                    tr!("auto-list-island", height = height)
                } else {
                    tr!("auto-list-overhang", height = height)
                },
                state: reason(place.why),
                state_colour: theme::WARNING.color(),
                hint: tr!("list-row-hint"),
                selected: Some(k) == at,
            }
        };
        let mut filter = usize::from(self.only_islands);
        let event = list_window::show(
            ctx,
            &spec,
            &mut self.list_open,
            &mut 0,
            &mut filter,
            &mut 0,
            &items,
            None,
            row,
        );
        self.only_islands = filter == 1;
        match event? {
            Event::Picked(k) => {
                self.at = Some(k);
                None
            }
            Event::Focused(k) => {
                self.at = Some(k);
                self.show(&left[k]);
                self.view.take()
            }
            Event::Removed(_) => None,
        }
    }

    /// The footer for model `index`: Apply and Discard while a plan shows, Generate (again) or
    /// Stop, the progress, then the state.
    pub fn footer(
        &mut self,
        ui: &mut egui::Ui,
        workspace: &mut Workspace,
        (index, lift_mm): (Option<usize>, f64),
        icons: Option<&Icons>,
    ) {
        let index = index.filter(|&i| i < workspace.plate().objects.len());
        if let Some((planned, current)) = self.plan_of(workspace, index) {
            let count = planned.outcome.plan.supports.len();
            let (apply, discard) = action_pair(
                ui,
                [tr!("auto-apply", count = count), tr!("auto-discard")],
                (current && count > 0, true),
            );
            if let (true, Some(i)) = (apply.on_hover_text(tr!("auto-apply-hint")).clicked(), index)
            {
                self.apply(workspace, i);
            } else if discard.clicked() {
                self.discard();
            }
            ui.add_space(4.0);
        }
        let automatic = index.and_then(|i| workspace.supports(i)).is_some_and(|d| {
            d.supports
                .iter()
                .any(|s| s.origin == karasu_core::supports::SupportOrigin::Automatic)
        });
        let (label, hint) = if self.planning() {
            (tr!("auto-stop"), tr!("auto-stop-hint"))
        } else if automatic {
            (tr!("auto-generate-again"), tr!("auto-generate-again-hint"))
        } else {
            (tr!("auto-generate"), tr!("auto-generate-hint"))
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
            self.generate_lifted(workspace, i, lift_mm);
        }
        if let Some(job) = &self.job {
            ui.add_space(4.0);
            ui.add(egui::ProgressBar::new(job.progress).desired_height(6.0));
        }
        ui.add_space(4.0);
        let (text, colour, icon) = self.state(workspace, index);
        state_line(ui, icons, icon, &text, colour);
    }

    /// What the footer says: planning and at what, a plan's outcome, out of date, what Apply
    /// added, or nothing yet.
    pub(super) fn state(
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
        if self.waiting.is_some() {
            return (tr!("auto-waiting"), theme::ACCENT.color(), Some(Icon::Info));
        }
        if let Some(job) = &self.job {
            let stage = match job.stage {
                Stage::Finding => tr!("auto-stage-finding"),
                Stage::Placing => tr!("auto-stage-placing"),
                Stage::Growing => tr!("auto-stage-growing"),
                Stage::Checking => tr!("auto-stage-checking"),
                Stage::Verifying => tr!("auto-stage-verifying"),
                Stage::Bracing => tr!("auto-stage-bracing"),
            };
            return (stage, theme::ACCENT.color(), Some(Icon::Info));
        }
        let warn = |text| (text, theme::WARNING.color(), Some(Icon::Warning));
        let ok = |text| (text, theme::SUCCESS.color(), Some(Icon::Success));
        if let Some((planned, current)) = self.plan_of(workspace, index) {
            if !current {
                return (
                    tr!("auto-out-of-date"),
                    theme::ACCENT.color(),
                    Some(Icon::Info),
                );
            }
            let (count, left) = (
                planned.outcome.plan.supports.len(),
                planned.outcome.left.len(),
            );
            return if left > 0 {
                warn(tr!("auto-state-planned-left", count = count, left = left))
            } else {
                ok(tr!("auto-state-planned", count = count))
            };
        }
        if let Some(applied) = self.applied_of(workspace, index) {
            return if applied.left.is_empty() {
                ok(tr!("auto-state-applied", count = applied.supports))
            } else {
                warn(tr!(
                    "auto-state-applied-left",
                    count = applied.supports,
                    left = applied.left.len()
                ))
            };
        }
        (tr!("auto-state-none"), theme::BORDER.color(), None)
    }
}
