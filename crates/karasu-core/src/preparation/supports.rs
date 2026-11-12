//! Parametric manual supports with immutable revision identity and certified geometric contacts.
//!
//! Polygonal sections, arbitrary section orientations and center paths define convex loft segments.
//! Adjacent segments and explicitly permitted support intersections are material unions, not a
//! Boolean triangle-mesh export. Geometry/contact validity does not certify mechanical strength.
use super::{
    drainage::{DrainageError, DrainageLimits},
    hollow::{HollowError, HollowLimits},
    SurfaceKey,
};
use crate::{
    cancellation::{Cancellation, Cancelled, Checkpoint},
    geometry::{Rotation3, TransformError},
    mesh::{MeshError, TriangleMesh},
};
use std::{error::Error, fmt, sync::Arc};
mod geometry;
mod plan;
mod profile;
mod scene;
mod validation;
pub use scene::{generate_supports, generate_supports_with_progress, SupportedModel};

/// An explicit convex polygonal profile. Smooth curve accuracy is not inferred from its name.
#[derive(Clone, Debug, PartialEq)]
pub enum SupportProfile {
    /// Inscribed elliptical polygon; equal radii provide a circular polygonal section.
    EllipsePolygon {
        /// Positive local X/Y semiaxes in millimeters.
        radii_mm: [f64; 2],
        /// Explicit vertex count, at least three.
        sides: u16,
    },
    /// Centered local rectangle, counterclockwise without repeated closing vertex.
    Rectangle {
        /// Positive full width/height in millimeters.
        size_mm: [f64; 2],
    },
    /// Regular polygon with a vertex on positive local X.
    RegularPolygon {
        /// Positive circumradius in millimeters.
        radius_mm: f64,
        /// Explicit vertex count, at least three.
        sides: u16,
    },
    /// Strictly convex counterclockwise polygon in local XY millimeters; no implicit repair.
    ConvexPolygon {
        /// Closing vertex is implicit; the local origin must lie strictly inside.
        vertices_mm: Vec<[f64; 2]>,
    },
    /// A sharp endpoint, admitted only at the final penetrating station.
    Point,
}
/// One section of a world-space support; neighboring sections define a convex-hull segment.
#[derive(Clone, Debug, PartialEq)]
pub struct SupportStation {
    /// World center in millimeters. Changing centers controls shaft direction and bends.
    pub center_mm: [f64; 3],
    /// Full right-handed local-XY section orientation, including tilt and roll.
    pub rotation: Rotation3,
    /// Independent section dimensions and shape; no requirement to match adjacent vertex counts.
    pub profile: SupportProfile,
}
/// Explicit intended surface contact; no nearest-surface projection is performed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SupportContact {
    /// Preserved placed-exterior source triangle defining the intended contact plane.
    pub source_triangle: usize,
    /// World contact point, admitted within the explicit contact tolerance of that plane.
    pub position_mm: [f64; 3],
    /// Maximum permitted inward depth of the penetrating end section in millimeters.
    pub maximum_penetration_mm: f64,
}
/// Full ordered manual recipe; stations zero/one define the base and the final pair the tip.
#[derive(Clone, Debug, PartialEq)]
pub struct SupportRecipe {
    /// Stable nonzero ID, unique within the support plan.
    pub id: u64,
    /// Exact source/placement/hollow/cut prefix; changed model revisions reject stale contacts.
    pub surface: SurfaceKey,
    /// Declared intended contact and bounded penetration.
    pub contact: SupportContact,
    /// At least four stations: platform, base top, neck, penetrating end. Additional stations bend
    /// or vary the shaft, including horizontal or returning segments above the platform.
    pub stations: Vec<SupportStation>,
}
/// Intentional intersection policy between different support recipes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SupportIntersectionPolicy {
    /// Require conservative geometric separation between every pair of components.
    RejectUncertified,
    /// Explicitly permit overlaps as material union; no strength/brace certificate is implied.
    Union,
}
/// Explicit plan, geometry, numerical and conservative memory/work limits.
#[derive(Clone, Copy, Debug)]
pub struct SupportLimits {
    /// Underlying convex hollow/drainage domain.
    pub drainage: DrainageLimits,
    /// Maximum support recipes.
    pub max_supports: usize,
    /// Maximum sections per recipe.
    pub max_stations: usize,
    /// Maximum points per nonterminal section, hard-capped at256.
    pub max_profile_vertices: usize,
    /// Independent hull/validation limits per convex loft segment.
    pub component: HollowLimits,
    /// Charged admission/profile/collision/assembly records, including subordinate allowances.
    pub max_work: usize,
    /// Conservative aggregate requested-capacity allowance, not allocator traffic or RSS.
    pub max_payload_bytes: usize,
    /// Strict exterior/cavity/cutter separation and terminal penetration admission margin, in mm.
    pub clearance_mm: f64,
    /// Maximum selected-plane contact-position error, in mm; also required contact interior margin.
    pub contact_tolerance_mm: f64,
}
impl Default for SupportLimits {
    fn default() -> Self {
        Self {
            drainage: DrainageLimits::default(),
            max_supports: 32,
            max_stations: 32,
            max_profile_vertices: 128,
            component: HollowLimits {
                max_input_vertices: 512,
                max_input_triangles: 2048,
                max_planes: 512,
                max_output_vertices: 512,
                max_output_triangles: 2048,
                max_work: 2_000_000,
                max_payload_bytes: 4 * 1024 * 1024,
            },
            max_work: 200_000_000,
            max_payload_bytes: 512 * 1024 * 1024,
            clearance_mm: 1e-6,
            contact_tolerance_mm: 1e-5,
        }
    }
}
/// Structured atomic support failure; no failed scene is published as valid.
#[derive(Debug)]
pub enum SupportError {
    /// Cooperative cancellation.
    Cancelled(Cancelled),
    /// Invalid parameter or source/section relationship.
    Invalid {
        /// Contextual invariant.
        reason: &'static str,
    },
    /// Resource/checked-capacity/allocation failure.
    Resource {
        /// Resource context.
        resource: &'static str,
    },
    /// A conservative geometric proof could not establish acceptance.
    Uncertain {
        /// Failed proof context.
        reason: &'static str,
    },
    /// Contact bound to another exact upstream revision.
    StaleAnchor {
        /// Stable support ID.
        support: u64,
    },
    /// Intended contact or penetration cannot be certified.
    Contact {
        /// Stable support ID.
        support: u64,
        /// Failed contact invariant.
        reason: &'static str,
    },
    /// A support component is not proven clear of an unintended obstacle.
    Collision {
        /// Stable support ID.
        support: u64,
        /// Ordered segment index.
        component: usize,
        /// Obstacle context; uncertainty is not a claim of proved intersection.
        obstacle: &'static str,
    },
    /// Source/model construction failure.
    Drainage(DrainageError),
    /// Convex component construction/validation failure.
    Geometry(HollowError),
    /// Checked mesh construction failure.
    Mesh(MeshError),
    /// Section placement failure.
    Transform(TransformError),
}
impl fmt::Display for SupportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled(e) => e.fmt(f),
            Self::Drainage(e) => e.fmt(f),
            Self::Geometry(e) => e.fmt(f),
            Self::Mesh(e) => e.fmt(f),
            Self::Transform(e) => e.fmt(f),
            Self::Invalid { reason } => write!(f, "invalid support: {reason}"),
            Self::Resource { resource } => write!(f, "support resource unavailable: {resource}"),
            Self::Uncertain { reason } => write!(f, "uncertified support: {reason}"),
            Self::StaleAnchor { support } => {
                write!(f, "support {support} targets an obsolete model revision")
            }
            Self::Contact { support, reason } => write!(f, "support {support} contact: {reason}"),
            Self::Collision {
                support,
                component,
                obstacle,
            } => write!(
                f,
                "support {support} component {component} is not certified clear of {obstacle}"
            ),
        }
    }
}
impl Error for SupportError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Cancelled(e) => Some(e),
            Self::Drainage(e) => Some(e),
            Self::Geometry(e) => Some(e),
            Self::Mesh(e) => Some(e),
            Self::Transform(e) => Some(e),
            _ => None,
        }
    }
}
impl From<Cancelled> for SupportError {
    fn from(e: Cancelled) -> Self {
        Self::Cancelled(e)
    }
}
impl From<MeshError> for SupportError {
    fn from(e: MeshError) -> Self {
        Self::Mesh(e)
    }
}
impl From<TransformError> for SupportError {
    fn from(e: TransformError) -> Self {
        Self::Transform(e)
    }
}
/// Immutable, separately versioned parametric recipe plan. Historical KPREP1 remains unchanged.
#[derive(Clone, Debug)]
pub struct SupportPlan {
    inner: Arc<plan::PlanData>,
}
/// Geometric contact certificate for one accepted recipe; no strength/printability claim.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SupportCertificate {
    /// Stable support identity.
    pub support_id: u64,
    /// Strict penetration lower bound of the terminal section center/vertices, in mm.
    pub minimum_penetration_mm: f64,
    /// Certified upper bound of the maximum terminal penetration, in mm.
    pub maximum_penetration_mm: f64,
    /// Minimum conservative clearance from unintended cavity/cutter/model contact, in mm.
    pub clearance_mm: f64,
}

pub use plan::SupportHistory;
/// One independently validated convex segment of an accepted support.
#[derive(Debug)]
pub struct SupportComponent {
    support_id: u64,
    segment_index: usize,
    mesh: TriangleMesh,
}
impl SupportComponent {
    /// Original recipe identity.
    #[must_use]
    pub const fn support_id(&self) -> u64 {
        self.support_id
    }
    /// Ordered segment between stations index and index+1.
    #[must_use]
    pub const fn segment_index(&self) -> usize {
        self.segment_index
    }
    /// Explicit outward convex component mesh for previews/export as a component.
    /// This is not a final Boolean union boundary.
    #[must_use]
    pub fn mesh(&self) -> &TriangleMesh {
        &self.mesh
    }
}
struct Work {
    used: usize,
    limit: usize,
}
impl Work {
    fn charge(&mut self, count: usize, cancel: &mut Cancellation<'_>) -> Result<(), SupportError> {
        self.used = self.used.checked_add(count).ok_or(SupportError::Resource {
            resource: "support work arithmetic",
        })?;
        if self.used > self.limit {
            return Err(SupportError::Resource {
                resource: "support work",
            });
        }
        cancel.check(Checkpoint::SupportWork)?;
        Ok(())
    }
    fn tick(&mut self, cancel: &mut Cancellation<'_>) -> Result<(), SupportError> {
        self.charge(1, cancel)
    }
}

/// Advisory snapshot-generation phase, separate from session acceptance and cancellation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum SupportStage {
    /// Validating aggregate resource admission.
    Admission,
    /// Validating/constructing the placed solid, cavity and drainage scene.
    Source,
    /// Constructing one recipe's independently oriented profile rings.
    Profiles,
    /// Constructing one closed convex loft component.
    Components,
    /// Certifying contact, connection and conservative separation.
    Validation,
    /// Assembling explicit prepared-material boundaries.
    Assembly,
    /// Fully computed immutable snapshot; a session must still accept its revision ticket.
    Complete,
}
/// Deterministic phase/count observation; not a percent, latency estimate or current-export grant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SupportProgress {
    /// Current named phase.
    pub stage: SupportStage,
    /// Recipes completely constructed and certified before the phase starts.
    pub completed_supports: usize,
    /// Number of requested recipes, including explicitly zero recipes.
    pub total_supports: usize,
    /// Stable recipe ID when processing one recipe.
    pub support_id: Option<u64>,
    /// Zero-based adjacent-section segment when constructing one component.
    pub component_index: Option<usize>,
}
