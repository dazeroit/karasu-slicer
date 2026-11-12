//! Analyses of a placed model for printing it: where its material starts in mid-air (TD-107),
//! and where its layers enclose resin as they are peeled (TD-114).
//!
//! - [`minima`]: the local minima of a model's surface in its placement, the points (and level
//!   edges or faces) lower than everything around them where material is above, so a layer of
//!   material starts there with nothing below it: each needs a support.
//! - [`unsupported`]: the print's unsupported regions on its layer masks, islands and
//!   overhangs, out of reach of the layer under them for the critical angle.
//! - [`orientation`]: tilts of a model scored for its supports, under the user's constraints
//!   (TD-119).
//! - [`suction`]: the print's suction cups on its layer masks, the resin each layer encloses
//!   while it is peeled from the film, capped towards the plate and not vented.
pub mod minima;
pub mod orientation;
mod runs;
pub mod suction;
pub mod unsupported;
