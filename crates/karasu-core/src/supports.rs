//! Manual supports on any closed model, solid or hollowed (TD-062; the specification is
//! `docs/SUPPORTS-SPEC.md`).
//!
//! A [`SupportDocument`] lists individually parameterised supports. Each one has:
//! - a tip on the model;
//! - an optionally bent shaft;
//! - a foot on the plate, on the model or on another support.
//!
//! The document can also hold braces and a raft.
//!
//! [`build_supports`] turns a document into closed, outward, convex part meshes (deterministic,
//! bit-identical on every platform) and validates them against the actual model surface: anchors
//! on the surface, axis limits, no piercing, proven clearance, drain holes and the build volume.
//! Findings are reported with their support, part and numbers; nothing is repaired, clamped or
//! dropped. Slicing unions the parts with the model under the positive winding rule, so tips that
//! penetrate the model, supports inside cavities and overlapping supports merge into one material.
//!
//! These supports are independent of the convex certified KSUP1 plans of
//! [`crate::preparation::supports`].
mod bracing;
mod build;
mod document;
mod fit;
mod index;
mod route;
mod shapes;
mod tip;

pub use bracing::{brace_mesh, plan_braces, BracePattern, BracePlan, PlannedBrace, Skipped};
pub use build::{
    build_supports, BaseKind, BuildVolume, BuiltSupport, DrainCylinder, ScenePart, SupportOptions,
    SupportScene, SupportSurface,
};
pub use document::{
    AxisSpec, BaseSpec, Binding, BoundPose, Brace, ContactShape, ManualSupport, PlanarMotion,
    Preset, RaftBars, RaftLayout, RaftShape, RaftSpec, SupportDocument, SupportKind, SupportOrigin,
    SupportParameters, CONTACT_FIT_STEP_MM, DEFAULT_ANCHOR_TOLERANCE_MM,
    DEFAULT_FILLET_DIAMETER_MM, DEFAULT_FILLET_HEIGHT_MM, FORMAT, MAX_DOCUMENT_BYTES,
    SKATE_MIN_PLATE_RADIUS_MM, VERSION,
};
pub use route::{
    find_mini_foot, find_route, foot_axis, MiniQuery, Route, RouteQuery, SurfaceGrid, HIT_PIECE_MM,
    MINI_BALL_MARGIN_MM, TILT_DIRECTIONS, TILT_STEP_DEG,
};
pub use tip::TipCone;

use crate::cancellation::Cancelled;
use std::fmt;

/// The part of a support a finding is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SupportPart {
    /// Its parameters.
    Parameters,
    /// The tip and contact on the model.
    Contact,
    /// A joint sphere at the tip join, a bend or the base join.
    Joint,
    /// A shaft segment.
    Shaft,
    /// The plate pad.
    Pad,
    /// The transition cone above the pad.
    Cone,
    /// The foot standing on the model.
    Foot,
    /// The junction on a parent support.
    Junction,
    /// A brace.
    Brace,
    /// The raft.
    Raft,
}

/// What a finding reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FindingKind {
    /// A value outside its recommended (soft) range.
    SoftRange,
    /// An anchor farther from the model surface than the tolerance.
    AnchorOffSurface,
    /// A tip or foot axis beyond the allowed deviation from the surface normal.
    AxisDeviation,
    /// Penetration reaching another surface (a thin wall or a cavity).
    Piercing,
    /// A contact fitted to the surface whose footprint does not lie on the model even at its
    /// smallest diameter (TD-116): it is built that small, with this warning.
    ContactFit,
    /// A part closer to the model than the clearance (or not proven clear).
    Collision,
    /// A part entering a drain hole's channel through the model, which it may partly plug
    /// (TD-123). A warning (TD-131): holding an island the hole made comes first. Drain holes
    /// cut only the model: a support crossing a hole's margins outside it is whole.
    DrainHole,
    /// A foot standing on the model where a drain hole cuts it away (TD-131): nothing is under
    /// it once the hole is cut.
    FootInHole,
    /// A part outside the printer's build volume.
    BuildVolume,
    /// A plate base that is not below the shaft above it, or a degenerate path.
    InvalidPath,
    /// A shaft segment more than 45° from vertical.
    SteepShaft,
    /// A base or brace on a support that is invalid or does not reach the height.
    InvalidParent,
    /// A raft without plate supports.
    EmptyRaft,
}

/// How serious a finding is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// Informational; slicing proceeds.
    Warning,
    /// A collision the document or caller accepted; slicing proceeds and it stays reported.
    Accepted,
    /// The supports cannot be sliced.
    Error,
}

/// One validation finding.
#[derive(Clone, Debug, PartialEq)]
pub struct SupportFinding {
    /// Support or brace ID; zero for the document or the raft.
    pub entity: u64,
    /// The part concerned.
    pub part: SupportPart,
    /// What was found.
    pub kind: FindingKind,
    /// How serious it is.
    pub severity: Severity,
    /// Numbers and context, in English.
    pub detail: String,
}

impl fmt::Display for SupportFinding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let severity = match self.severity {
            Severity::Warning => "warning",
            Severity::Accepted => "accepted",
            Severity::Error => "error",
        };
        let what = if self.entity == 0 {
            "document".to_owned()
        } else {
            format!("support {}", self.entity)
        };
        write!(f, "{severity}: {what}, {:?}: {}", self.part, self.detail)
    }
}

/// A support document or build that cannot proceed.
#[derive(Debug)]
pub enum SupportError {
    /// Malformed JSON or an unknown field.
    Json(String),
    /// A value, identifier or reference that violates the schema.
    Invalid {
        /// Support or brace ID; zero for document-level values.
        entity: u64,
        /// What is wrong.
        reason: String,
    },
    /// A raft hull that could not be built.
    Geometry(String),
    /// A count or allocation beyond the limits.
    Resource(&'static str),
    /// Cooperative cancellation.
    Cancelled(Cancelled),
}

impl fmt::Display for SupportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(e) => write!(f, "invalid support document: {e}"),
            Self::Invalid { entity: 0, reason } => write!(f, "invalid support document: {reason}"),
            Self::Invalid { entity, reason } => write!(f, "invalid support {entity}: {reason}"),
            Self::Geometry(e) => write!(f, "support geometry failed: {e}"),
            Self::Resource(what) => write!(f, "support limit exceeded: {what}"),
            Self::Cancelled(c) => write!(f, "{c}"),
        }
    }
}

impl std::error::Error for SupportError {}

impl From<Cancelled> for SupportError {
    fn from(c: Cancelled) -> Self {
        Self::Cancelled(c)
    }
}
