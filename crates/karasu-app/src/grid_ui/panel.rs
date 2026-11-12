//! The Grid mode's panel and footer (the mockup "Supports on a grid", TD-138): the face picked,
//! how the grid is made, then what it planned; the footer holds Apply and Discard and the state.
use super::{
    plan::{GridValues, ANGLE_DEG, BRANCH_MM, FLAT_DEG, INSET_MM, PER_TRUNK, SPACING_MM},
    GridUi,
};
use crate::{
    auto_ui::panel::length_row,
    i18n::{decimal, tr},
    icons::{Icon, Icons},
    islands_ui::panel::{count_row, preset_row},
    supports_ui::{SupportPreset, SupportsUi},
    theme,
    widgets::{action_pair, default_switch, fact_row, state_line, switch_default, Unit},
    workspace::Workspace,
};
use eframe::egui;

/// How the grid is made, in `ui`, with `advanced` the finer values: its preset, spacings, rows'
/// direction, the outline's tips and the branches. Shared by the panel and the settings'
/// defaults; in the panel, `defaults` gives each field its reset button. Returns whether any
/// changed.
pub fn grid_fields(
    ui: &mut egui::Ui,
    values: &mut GridValues,
    presets: &[SupportPreset],
    advanced: bool,
    defaults: Option<&GridValues>,
) -> bool {
    let mut chosen = (values.weight, values.mine);
    let mut edited = preset_row(
        ui,
        (tr!("islands-preset"), tr!("grid-preset-hint")),
        &mut chosen,
        presets,
        ("grid-preset", defaults.map(|d| (d.weight, d.mine))),
    );
    (values.weight, values.mine) = chosen;
    edited |= length_row(
        ui,
        (tr!("grid-spacing"), tr!("grid-spacing-hint")),
        &mut values.spacing_mm,
        (Unit::Millimetre, SPACING_MM),
        defaults.map(|d| d.spacing_mm),
    );
    edited |= length_row(
        ui,
        (tr!("grid-across"), tr!("grid-across-hint")),
        &mut values.across_mm,
        (Unit::Millimetre, SPACING_MM),
        defaults.map(|d| d.across_mm),
    );
    edited |= default_switch(
        ui,
        &mut values.along_longest,
        (tr!("grid-along"), tr!("grid-along-hint")),
        defaults.map(|d| switch_default(d.along_longest)).as_ref(),
    );
    let on = !values.along_longest;
    crate::widgets::depends(ui, on, |ui| {
        edited |= length_row(
            ui,
            (tr!("grid-angle"), tr!("grid-angle-hint")),
            &mut values.angle_deg,
            (Unit::Degree, ANGLE_DEG),
            defaults.map(|d| d.angle_deg),
        );
    });
    edited |= default_switch(
        ui,
        &mut values.edges,
        (tr!("grid-edges"), tr!("grid-edges-hint")),
        defaults.map(|d| switch_default(d.edges)).as_ref(),
    );
    let on = values.edges && advanced;
    crate::widgets::depends(ui, on, |ui| {
        edited |= length_row(
            ui,
            (tr!("grid-inset"), tr!("grid-inset-hint")),
            &mut values.inset_mm,
            (Unit::Millimetre, INSET_MM),
            defaults.map(|d| d.inset_mm),
        );
    });
    if advanced {
        edited |= length_row(
            ui,
            (tr!("grid-flat"), tr!("grid-flat-hint")),
            &mut values.flat_deg,
            (Unit::Degree, FLAT_DEG),
            defaults.map(|d| d.flat_deg),
        );
    }
    edited |= default_switch(
        ui,
        &mut values.branches,
        (tr!("grid-branches"), tr!("grid-branches-hint")),
        defaults.map(|d| switch_default(d.branches)).as_ref(),
    );
    let on = values.branches;
    crate::widgets::depends(ui, on, |ui| {
        edited |= length_row(
            ui,
            (tr!("islands-branch"), tr!("islands-branch-hint")),
            &mut values.branch_mm,
            (Unit::Millimetre, BRANCH_MM),
            defaults.map(|d| d.branch_mm),
        );
        edited |= count_row(
            ui,
            (tr!("islands-per-trunk"), tr!("islands-per-trunk-hint")),
            &mut values.per_trunk,
            (PER_TRUNK, true),
            defaults.map(|d| d.per_trunk),
        );
    });
    edited
}

impl GridUi {
    /// The panel's body: the face picked, the grid's values, then what the plan holds.
    pub fn panel(
        &mut self,
        ui: &mut egui::Ui,
        workspace: &Workspace,
        supports: &mut SupportsUi,
        index: Option<usize>,
    ) {
        let index = index.filter(|&i| i < workspace.plate().objects.len());
        crate::widgets::group(ui, tr!("grid-face"), |ui| {
            match self.plan_of(workspace, index) {
                Some((planned, _)) => {
                    fact_row(
                        ui,
                        &tr!("grid-face-area"),
                        &format!("{} mm²", decimal(planned.face.area_mm2, 0)),
                    );
                    fact_row(
                        ui,
                        &tr!("grid-face-tilt"),
                        &format!("{}°", decimal(planned.face.tilt_deg, 0)),
                    );
                }
                None => {
                    fact_row(ui, &tr!("grid-face-area"), "–");
                }
            }
        });
        crate::widgets::group(ui, tr!("grid-supports"), |ui| {
            let presets = supports.presets.clone();
            let defaults = GridValues {
                mine: supports.grid_default_mine,
                ..supports.tool_defaults.grid
            };
            let advanced = supports.advanced;
            ui.add_enabled_ui(index.is_some(), |ui| {
                grid_fields(ui, &mut supports.grid, &presets, advanced, Some(&defaults));
            });
        });
        crate::widgets::group(ui, tr!("grid-planned"), |ui| {
            let Some((planned, _)) = self.plan_of(workspace, index) else {
                fact_row(ui, &tr!("grid-fact-supports"), "–");
                return;
            };
            let tips = &planned.tips;
            let outcome = &planned.outcome;
            let count = outcome.plan.supports.len();
            for (name, value) in [
                (tr!("grid-fact-supports"), count.to_string()),
                (tr!("grid-fact-corners"), tips.corners.to_string()),
                (tr!("grid-fact-edges"), tips.edges.to_string()),
                (tr!("grid-fact-sides"), tips.sides.to_string()),
                (tr!("grid-fact-grid"), tips.grid.to_string()),
                (
                    tr!("grid-fact-plate"),
                    (count - outcome.plan.branches).to_string(),
                ),
                (tr!("grid-fact-left"), outcome.left.len().to_string()),
                (
                    tr!("grid-fact-time"),
                    format!("{} s", decimal(planned.seconds, 2)),
                ),
            ] {
                fact_row(ui, &name, &value);
            }
        });
    }

    /// The footer: Apply and Discard while a plan is shown, then the state.
    pub fn footer(
        &mut self,
        ui: &mut egui::Ui,
        workspace: &mut Workspace,
        index: Option<usize>,
        icons: Option<&Icons>,
    ) {
        let index = index.filter(|&i| i < workspace.plate().objects.len());
        if let Some(count) = self.planned_count(workspace, index) {
            let (apply, discard) = action_pair(
                ui,
                [tr!("grid-apply", count = count), tr!("grid-discard")],
                (count > 0, true),
            );
            if let (true, Some(i)) = (apply.clicked(), index) {
                self.apply(workspace, i);
            } else if discard.clicked() {
                self.discard();
            }
            ui.add_space(4.0);
        }
        let (text, colour, icon) = self.state(workspace, index);
        state_line(ui, icons, icon, &text, colour);
    }

    /// What the footer says: what to click, planning, a plan's outcome or out of date.
    fn state(
        &self,
        workspace: &Workspace,
        index: Option<usize>,
    ) -> (String, egui::Color32, Option<Icon>) {
        if index.is_none() {
            return ("–".to_owned(), theme::BORDER.color(), None);
        }
        if self.planning() {
            return (
                tr!("grid-planning"),
                theme::ACCENT.color(),
                Some(Icon::Info),
            );
        }
        match self.plan_of(workspace, index) {
            Some((_, false)) => (
                tr!("grid-out-of-date"),
                theme::ACCENT.color(),
                Some(Icon::Info),
            ),
            Some((planned, true)) => {
                let (count, left) = (
                    planned.outcome.plan.supports.len(),
                    planned.outcome.left.len(),
                );
                if left > 0 {
                    (
                        tr!("grid-state-planned-left", count = count, left = left),
                        theme::WARNING.color(),
                        Some(Icon::Warning),
                    )
                } else {
                    (
                        tr!("grid-state-planned", count = count),
                        theme::SUCCESS.color(),
                        Some(Icon::Success),
                    )
                }
            }
            None => (
                match self.mode {
                    super::plan::PickMode::Face => tr!("grid-state-pick"),
                    super::plan::PickMode::Rectangle => tr!("grid-state-pick-rectangle"),
                    super::plan::PickMode::Lasso => tr!("grid-state-pick-lasso"),
                },
                theme::ACCENT.color(),
                Some(Icon::Info),
            ),
        }
    }
}
