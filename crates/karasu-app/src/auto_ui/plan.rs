//! Every support of a model planned at once (M6.4i, owner mockup "Automatic supports tool",
//! 2026-10-30). The plan itself is headless ([`karasu_project::support_plan::auto`]); the
//! Automatic tool and the Grid mode make it on threads of their own ([`crate::auto_ui`],
//! [`crate::grid_ui`]).
pub use karasu_project::support_plan::auto::{
    plan, plan_tips, with_planned, AutoOutcome, AutoSupports, Cover, Input, Left, Place,
    CLEARANCE_MM, DEFAULT_AUTO_SUPPORTS, DENSITY_PCT, EDGE_INSET_MM, LIFT_MM, MINI_DENSITY_PCT,
    PASSES, SPACING_MM, WIDE_CLOSEST_PCT, WIDE_FROM_MM2,
};

#[cfg(test)]
mod tests;
