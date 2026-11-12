//! The tools' values for new operations and their defaults (owner, 2026-11-02).
//!
//! What a tool's panel sets for the next hollowing, hole, support, brace or plan is the user's
//! while they work: it stays when the tool closes and opens again, and is never saved as a new
//! default. The values go back to the defaults set in Settings at start-up and with "Restore all
//! default values" (and, later, with a new project). A default changed in Settings carries to
//! every value still at the old default; a value the user changed stays theirs, and its field's
//! reset button returns it to the new default.
use super::MainWindow;
use crate::{
    auto_ui::plan::AutoSupports, hollowing_ui::blockers::NewBlocker,
    islands_ui::plan::IslandSupports, supports_ui::braces::NewBraces, supports_ui::NextSupport,
};
use karasu_core::preparation::sampled::HoleSurface;

/// Every tool's values for new operations.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct ToolValues {
    wall_mm: f64,
    cell_mm: Option<f64>,
    hole_mm: f64,
    depth_mm: Option<f64>,
    start: HoleSurface,
    min_vent_mm2: f64,
    blocker: NewBlocker,
    next: NextSupport,
    braces: NewBraces,
    islands_angle_deg: f64,
    islands: IslandSupports,
    auto_angle_deg: f64,
    automatic: AutoSupports,
    grid: crate::grid_ui::plan::GridValues,
    orient: crate::orient_ui::OrientValues,
}

/// `current`'s field `$f` follows `old`'s to `new`'s when it was still at it.
macro_rules! follow {
    ($current:expr, $old:expr, $new:expr; $($f:ident),+) => {
        $(
            #[allow(clippy::float_cmp)] // Still exactly the old default: untouched.
            if $current.$f == $old.$f {
                $current.$f = $new.$f;
            }
        )+
    };
}

/// A preset chosen, as a weight and the user's own: they follow together.
fn follow_preset<T: PartialEq + Copy>(
    current: &mut (T, Option<usize>),
    old: (T, Option<usize>),
    new: (T, Option<usize>),
) {
    if *current == old {
        *current = new;
    }
}

impl MainWindow {
    /// The tools' values as the settings' defaults set them.
    pub(super) fn default_tool_values(&self) -> ToolValues {
        let defaults = self.tool_defaults;
        let supports = &self.supports;
        ToolValues {
            wall_mm: defaults.wall_mm,
            cell_mm: defaults.cell_mm,
            hole_mm: defaults.hole_diameter_mm,
            depth_mm: defaults.hole_depth_mm,
            start: if defaults.holes_from_cavity {
                HoleSurface::Cavity
            } else {
                HoleSurface::Outer
            },
            min_vent_mm2: defaults.min_vent_mm2,
            blocker: defaults.blocker,
            next: match supports.default_preset {
                Some(i) => NextSupport::of_preset(&supports.presets, i),
                None => NextSupport::of(defaults.support_weight),
            }
            .fitting(defaults.support_contact_fit_min_mm),
            braces: defaults.braces,
            islands_angle_deg: self.overhangs.angle_deg,
            islands: IslandSupports {
                mine: supports.islands_default_mine,
                ..defaults.island_supports
            },
            auto_angle_deg: self.overhangs.angle_deg,
            automatic: AutoSupports {
                mine: supports.automatic_default_mine,
                cavity_mine: supports.automatic_cavity_default_mine,
                ..defaults.auto_supports
            },
            grid: crate::grid_ui::plan::GridValues {
                mine: supports.grid_default_mine,
                ..defaults.grid
            },
            orient: defaults.orient,
        }
    }

    /// The tools' values now.
    pub(super) fn tool_values(&mut self) -> ToolValues {
        let hollowing = *self.hollowing.new_hollowing();
        let (hole_mm, depth_mm, start) = self.hollowing.next_hole();
        let (hole_mm, depth_mm, start) = (*hole_mm, *depth_mm, *start);
        ToolValues {
            wall_mm: hollowing.wall_mm,
            cell_mm: hollowing.cell_mm,
            hole_mm,
            depth_mm,
            start,
            min_vent_mm2: self.hollowing.cups.min_vent_mm2,
            blocker: self.hollowing.blockers.next,
            next: self.supports.next,
            braces: self.supports.bracing.new,
            islands_angle_deg: self.islands.angle_deg,
            islands: self.supports.islands,
            auto_angle_deg: self.auto.angle_deg,
            automatic: self.supports.automatic,
            grid: self.supports.grid,
            orient: self.orient.values,
        }
    }

    /// Set the tools' values to `values`.
    pub(super) fn set_tool_values(&mut self, values: &ToolValues) {
        let hollowing = self.hollowing.new_hollowing();
        hollowing.wall_mm = values.wall_mm;
        hollowing.cell_mm = values.cell_mm;
        let (hole_mm, depth_mm, start) = self.hollowing.next_hole();
        *hole_mm = values.hole_mm;
        *depth_mm = values.depth_mm;
        *start = values.start;
        self.hollowing.cups.min_vent_mm2 = values.min_vent_mm2;
        self.hollowing.blockers.next = values.blocker;
        self.supports.next = values.next;
        self.supports.bracing.new = values.braces;
        self.islands.angle_deg = values.islands_angle_deg;
        self.supports.islands = values.islands;
        self.auto.angle_deg = values.auto_angle_deg;
        self.supports.automatic = values.automatic;
        self.supports.grid = values.grid;
        self.orient.values = values.orient;
    }

    /// Every tool's values back to the defaults: at start-up, and with "Restore all default
    /// values". Values of applied operations belong to their models and stay.
    pub(super) fn reset_tool_values(&mut self) {
        let defaults = self.default_tool_values();
        self.set_tool_values(&defaults);
        self.islands.hide_under_mm2 = 0.0;
        self.supports.on_cavity = false;
        self.supports.cavity_view = false;
    }

    /// The defaults changed from `old`: every value still at its old default takes the new
    /// one; a value the user changed stays.
    pub(super) fn follow_defaults(&mut self, old: &ToolValues) {
        let new = self.default_tool_values();
        let mut current = self.tool_values();
        follow!(current, old, new; wall_mm, cell_mm, hole_mm, depth_mm, start, min_vent_mm2);
        follow!(current, old, new; next, islands_angle_deg, auto_angle_deg);
        follow!(current.blocker, old.blocker, new.blocker;
            sphere, bottom_mm, top_mm, height_mm, diameter_mm, base_mm);
        follow!(current.braces, old.braces, new.braces;
            auto_diameter, diameter_mm, max_distance_mm, lowest_mm, step_mm, pattern);
        let mut preset = (current.islands.weight, current.islands.mine);
        follow_preset(
            &mut preset,
            (old.islands.weight, old.islands.mine),
            (new.islands.weight, new.islands.mine),
        );
        (current.islands.weight, current.islands.mine) = preset;
        follow!(current.islands, old.islands, new.islands;
            join_mm, skip_under_mm2, foot_on_model, minis, branch_mm, per_trunk, merge_mm,
            per_group, levels, contact_fit_min_mm, sphere_contact);
        let mut preset = (current.automatic.weight, current.automatic.mine);
        follow_preset(
            &mut preset,
            (old.automatic.weight, old.automatic.mine),
            (new.automatic.weight, new.automatic.mine),
        );
        (current.automatic.weight, current.automatic.mine) = preset;
        let mut preset = (current.grid.weight, current.grid.mine);
        follow_preset(
            &mut preset,
            (old.grid.weight, old.grid.mine),
            (new.grid.weight, new.grid.mine),
        );
        (current.grid.weight, current.grid.mine) = preset;
        follow!(current.grid, old.grid, new.grid;
            spacing_mm, across_mm, along_longest, angle_deg, edges, inset_mm, flat_deg, branches,
            branch_mm, per_trunk);
        follow!(current.orient, old.orient, new.orient;
            axes, limited, limit_deg, keep_away_mm, priority, weights);
        follow!(current.automatic, old.automatic, new.automatic;
            cover, place, skip_under_mm2, spacing_mm, steep_spacing_mm, join_mm, feet_on_model,
            minis, branches, branch_mm, per_trunk, merge_mm, per_group, levels, braces, passes,
            contact_fit_min_mm, sphere_contact);
        self.set_tool_values(&current);
    }

    /// Give the tools their defaults, for the reset buttons of their panels' fields.
    pub(super) fn share_defaults(&mut self) {
        let defaults = self.default_tool_values();
        self.hollowing.tool_defaults = self.tool_defaults;
        self.supports.tool_defaults = self.tool_defaults;
        self.supports.next_default = defaults.next;
        self.islands.default_angle_deg = defaults.islands_angle_deg;
        self.auto.default_angle_deg = defaults.auto_angle_deg;
        self.orient.defaults = defaults.orient;
    }
}
