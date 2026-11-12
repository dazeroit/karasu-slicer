//! Immutable preparation plans/history and bounded convex hollowing, drainage and manual supports,
//! plus sampled-distance hollowing of arbitrary closed (non-convex) solids in [`sampled`].
//!
//! Parameter admission is not solid validation, hole connectivity or support certification.
//! Source digests are caller assertions; see `docs/M55-IMPLEMENTATION-SPEC.md` for trust and frames.
pub mod drainage;
pub mod hollow;
pub mod repair;
pub mod sampled;
pub mod session;
pub mod supports;

use crate::{
    cancellation::{Cancellation, Cancelled, Checkpoint},
    geometry::{Affine3, ObjectTransform},
};
use std::{error::Error, fmt, mem::size_of, sync::Arc};

/// Original-byte identity asserted by an asset loader, plus placement before world-space operations.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SourceBinding {
    /// SHA-256 of original asset bytes; correspondence with decoded geometry is not verified here.
    pub original_sha256: [u8; 32],
    /// Positive-scale placement; reflections and collapsed scales are rejected by plan admission.
    pub placement: ObjectTransform,
}
/// Explicit sampling request; no grid is allocated by plan construction.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SamplingSpec {
    /// World cell side in millimeters.
    pub cell_mm: f64,
    /// Positive requested distance-query error, at most cell/32.
    pub distance_error_mm: f64,
    /// Maximum cells allowed in a later evaluation.
    pub max_cells: u64,
}
/// World-space inward shell request.
#[derive(Clone, Debug, PartialEq)]
pub struct HollowSpec {
    /// Positive world wall thickness in millimeters.
    pub thickness_mm: f64,
    /// Regions left solid (owner, 2026-11-02): no cavity is cut in them; the wall round the
    /// rest of the cavity is guaranteed as without them. In the plan's source frame.
    pub blockers: Vec<HollowBlocker>,
    /// Guarantee the wall (TD-093): hollow at a raised offset and prove, face by face, that the
    /// certified minimum wall reaches `thickness_mm`, with further passes when it does not.
    /// `None` hollows at `thickness_mm` and reports the certified wall, as before.
    pub guarantee: Option<WallGuarantee>,
}

/// A region the hollowing leaves solid (owner, 2026-11-02), in the plan's source frame.
///
/// The cavity is the set of points deeper than the wall *and* outside every blocker. Each
/// blocker gives a signed distance bound ([`HollowBlocker::signed`]): 1-Lipschitz, negative
/// inside, zero exactly on its boundary. The sampled field takes the least of the cavity's
/// depth and the blockers' bounds, which keeps the field 1-Lipschitz, so its pruning and its
/// proof hold unchanged; blockers only take cavity away, so every cavity face keeps its wall.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HollowBlocker {
    /// A frustum: two rectangular bases, parallel and centred on one axis, `height_mm` apart; a
    /// box when they are alike, a truncated pyramid otherwise.
    Frustum {
        /// The point midway between the bases' centres.
        centre: [f64; 3],
        /// Its width, depth and height directions (rows): orthonormal.
        axes: [[f64; 3]; 3],
        /// The lower base's width and depth (along the first and second axes), in mm.
        bottom_mm: [f64; 2],
        /// The upper base's width and depth, in mm; zero makes a pyramid's apex.
        top_mm: [f64; 2],
        /// The distance between the bases, along the third axis, in mm.
        height_mm: f64,
    },
    /// A ball.
    Sphere {
        /// Its centre.
        centre: [f64; 3],
        /// Its radius, in mm.
        radius_mm: f64,
    },
}

impl HollowBlocker {
    /// A signed distance bound at `p`: negative inside, zero exactly on the boundary, positive
    /// outside, and 1-Lipschitz. A sphere's is exact; a frustum's is the largest of its six
    /// faces' plane distances, exact inside and along each face, under the true distance
    /// outside near its edges.
    #[must_use]
    pub fn signed(&self, p: [f64; 3]) -> f64 {
        let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
        match *self {
            Self::Sphere { centre, radius_mm } => {
                let d: [f64; 3] = std::array::from_fn(|k| p[k] - centre[k]);
                dot(d, d).sqrt() - radius_mm
            }
            Self::Frustum {
                centre,
                axes,
                bottom_mm,
                top_mm,
                height_mm,
            } => {
                let d: [f64; 3] = std::array::from_fn(|k| p[k] - centre[k]);
                let local = [dot(d, axes[0]), dot(d, axes[1]), dot(d, axes[2])];
                let half_height = height_mm / 2.0;
                let mut bound = (local[2].abs() - half_height).max(f64::NEG_INFINITY);
                // Each side leans from the lower base's half-size to the upper's: the plane
                // `u - (b + (t - b) (z + h) / 2h) = 0` in the (u, z) plane, normalised.
                for k in 0..2 {
                    let (b, t) = (bottom_mm[k] / 2.0, top_mm[k] / 2.0);
                    let slope = (t - b) / height_mm;
                    let at_centre = f64::midpoint(b, t);
                    let side = (local[k].abs() - at_centre - slope * local[2])
                        / (1.0 + slope * slope).sqrt();
                    bound = bound.max(side);
                }
                bound
            }
        }
    }
}

/// How the sampled backend guarantees the requested wall (TD-093).
///
/// The first pass hollows at `thickness + shortfall_coefficient · cell² / thickness`: next to a
/// reflex edge the cavity is a rounded surface of radius `thickness`, which a tetrahedron of
/// diameter `cell · √3` cuts by at most about `0.375 · cell² / thickness`. When the proven wall
/// still falls short, the offset grows by the shortfall; when a pass is refused or does not
/// converge, the cell is halved.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WallGuarantee {
    /// The predicted shortfall's coefficient, in `(0, 1]`.
    pub shortfall_coefficient: f64,
}
/// Certified simplification of the hollowing cavity (TD-066): the cavity surface may move by at
/// most `tolerance_mm` and never closer to the outside than the thinnest original wall.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SimplifySpec {
    /// Largest distance between the simplified and the original cavity surface, in mm.
    pub tolerance_mm: f64,
}
/// Default distance, in mm, of a surface hole's entry outside the surface and of its end past
/// the cavity wall (TD-066; `PrusaSlicer` also moves holes 1 mm out, Lychee 0.5 mm).
pub const SURFACE_HOLE_MARGIN_MM: f64 = 1.0;
/// Largest tilt of a surface hole's axis away from the surface normal, in degrees (TD-084).
pub const MAX_HOLE_TILT_DEG: f64 = 60.0;

/// Tilt of a surface hole's axis away from the inward surface normal (TD-084).
///
/// The tilt direction is measured in the tangent plane at the hole's foot, from a reference
/// tangent: world +Z projected onto the plane, or world +X where the normal is within about 26
/// degrees of vertical (its Z component above 0.9 in magnitude). The azimuth turns
/// counterclockwise seen from outside the surface. The axis leans from the inward normal towards
/// the tilt direction by `angle_deg`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HoleTilt {
    /// Angle between the axis and the inward normal, 0 to [`MAX_HOLE_TILT_DEG`].
    pub angle_deg: f64,
    /// Direction of the tilt around the normal, from the reference tangent, -360 to 360.
    pub azimuth_deg: f64,
}

/// Drain hole placed at a point of the model surface (TD-066, TD-084), resolved during drainage.
///
/// The hole axis is the inward area-weighted normal of the source surface within the hole's
/// radius of the surface point nearest `surface_mm`, tilted by `tilt` when given. The cut starts
/// `outside_mm` outside the surface. It ends `inside_mm` past the first cavity surface along the
/// axis, or at `depth_mm` from the surface when a fixed depth is given (the cut is then
/// certified like any other, and refused when it does not end inside the cavity).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SurfaceHoleSpec {
    /// A point on or near the source surface, in world millimeters.
    pub surface_mm: [f64; 3],
    /// Positive cut diameter.
    pub diameter_mm: f64,
    /// Positive distance of the entry cap outside the surface.
    pub outside_mm: f64,
    /// Positive depth of the end cap past the cavity surface (automatic depth).
    pub inside_mm: f64,
    /// Tilt of the axis from the inward normal, or `None` for a perpendicular hole.
    pub tilt: Option<HoleTilt>,
    /// Fixed depth from the surface to the end cap along the axis, or `None` for the automatic
    /// depth.
    pub depth_mm: Option<f64>,
}
/// Finite world-space cylindrical cut; raw direction is retained for stable replay.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HoleSpec {
    /// World cut entry in millimeters; remains world-fixed after placement changes.
    pub entry_mm: [f64; 3],
    /// Nonzero finite direction; orientation is preserved.
    pub direction: [f64; 3],
    /// Positive cut length in millimeters.
    pub length_mm: f64,
    /// Positive cylinder diameter in millimeters.
    pub diameter_mm: f64,
}
impl HoleSpec {
    /// Computes a unit direction with scaled arithmetic; raw parameters are never rewritten.
    ///
    /// # Errors
    /// Rejects zero or non-finite directions.
    pub fn unit_direction(&self) -> Result<[f64; 3], PreparationError> {
        let max = self.direction.iter().map(|v| v.abs()).fold(0.0, f64::max);
        if max == 0. || self.direction.iter().any(|v| !v.is_finite()) {
            return Err(invalid("cut direction", None));
        }
        let scaled = self.direction.map(|v| v / max);
        let norm = scaled.iter().map(|v| v * v).sum::<f64>().sqrt();
        Ok(scaled.map(|v| v / norm))
    }
}
/// Exact upstream model identity. Cloning shares immutable bytes, never a mutable cache entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SurfaceKey(Arc<[u8]>);
/// Manual world-space support parameters, not generated support geometry.
#[derive(Clone, Debug, PartialEq)]
pub struct SupportSpec {
    /// Exact source/placement/sampling/hollow/cut prefix selected for this contact.
    pub surface: SurfaceKey,
    /// World contact position, strictly above the platform.
    pub contact_mm: [f64; 3],
    /// Base center X/Y; platform Z is exactly zero.
    pub base_xy_mm: [f64; 2],
    /// Positive contact radius in millimeters.
    pub tip_radius_mm: f64,
    /// Positive intended contact penetration depth in millimeters.
    pub penetration_mm: f64,
    /// Positive shaft radius, at least the tip radius.
    pub shaft_radius_mm: f64,
    /// Positive base radius, at least the shaft radius.
    pub base_radius_mm: f64,
    /// Positive base height, below the contact.
    pub base_height_mm: f64,
}
/// Ordered operation request. No variant implies that its geometry has been evaluated.
#[derive(Clone, Debug, PartialEq)]
pub enum OperationKind {
    /// Explicit source repairs (TD-054), first and at most once: the operations after it apply
    /// to the repaired source.
    Repair(repair::RepairSpec),
    /// One shell operation, before any cuts or supports.
    Hollow(HollowSpec),
    /// Cavity simplification, directly after the hollow (TD-066).
    Simplify(SimplifySpec),
    /// A cut after hollowing and before supports.
    Hole(HoleSpec),
    /// A cut perpendicular to the surface, after hollowing and before supports (TD-066).
    SurfaceHole(SurfaceHoleSpec),
    /// A manually placed support bound to the exact model prefix.
    Support(SupportSpec),
}
/// Stable nonzero ID and complete immutable operation parameters.
#[derive(Clone, Debug, PartialEq)]
pub struct Operation {
    /// Nonzero ID unique within the plan.
    pub id: u64,
    /// Complete requested operation.
    pub kind: OperationKind,
}
/// Plan admission limits, separate from future geometry evaluation budgets.
#[derive(Clone, Copy, Debug)]
pub struct PlanLimits {
    /// Maximum ordered records.
    pub max_operations: usize,
    /// Conservative retained record, key and canonical-byte payload.
    pub max_payload_bytes: usize,
    /// Minimum admitted dimension in millimeters, not a geometric tolerance.
    pub min_feature_mm: f64,
    /// Maximum absolute coordinate or dimension in millimeters.
    pub max_extent_mm: f64,
}
impl Default for PlanLimits {
    fn default() -> Self {
        Self {
            max_operations: 4096,
            max_payload_bytes: 8 * 1024 * 1024,
            min_feature_mm: 0.000_001,
            max_extent_mm: 1_000_000.,
        }
    }
}
/// Structured plan/history failure without successful partial publication.
#[derive(Debug, PartialEq)]
pub enum PreparationError {
    /// Invalid parameter, operation order or duplicate ID.
    Invalid {
        /// Field or invariant that failed.
        field: &'static str,
        /// Relevant operation ID, if any.
        operation: Option<u64>,
    },
    /// Contact belongs to different upstream model content.
    StaleAnchor {
        /// Support operation requiring explicit rebinding.
        operation: u64,
    },
    /// Requested storage/count/generation or allocation limit.
    Resource {
        /// Resource that could not be admitted.
        resource: &'static str,
    },
    /// Cooperative cancellation before publication.
    Cancelled(Cancelled),
}
impl fmt::Display for PreparationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid { field, operation } => {
                write!(f, "invalid preparation {field} (operation {operation:?})")
            }
            Self::StaleAnchor { operation } => {
                write!(f, "support {operation} has a stale model surface binding")
            }
            Self::Resource { resource } => {
                write!(f, "preparation resource unavailable: {resource}")
            }
            Self::Cancelled(e) => e.fmt(f),
        }
    }
}
impl Error for PreparationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Cancelled(e) => Some(e),
            _ => None,
        }
    }
}
impl From<Cancelled> for PreparationError {
    fn from(value: Cancelled) -> Self {
        Self::Cancelled(value)
    }
}
fn invalid(field: &'static str, id: Option<u64>) -> PreparationError {
    PreparationError::Invalid {
        field,
        operation: id,
    }
}
fn resource(name: &'static str) -> PreparationError {
    PreparationError::Resource { resource: name }
}
fn append(bytes: &mut Vec<u8>, value: &[u8], limit: usize) -> Result<(), PreparationError> {
    if bytes
        .len()
        .checked_add(value.len())
        .is_none_or(|v| v > limit)
    {
        return Err(resource("canonical bytes"));
    }
    bytes
        .try_reserve(value.len())
        .map_err(|_| resource("canonical allocation"))?;
    bytes.extend_from_slice(value);
    Ok(())
}
fn integer(bytes: &mut Vec<u8>, value: u64, limit: usize) -> Result<(), PreparationError> {
    append(bytes, &value.to_le_bytes(), limit)
}
fn scalar(bytes: &mut Vec<u8>, value: f64, limit: usize) -> Result<(), PreparationError> {
    append(
        bytes,
        &(if value == 0. { 0.0 } else { value }).to_le_bytes(),
        limit,
    )
}
fn position(values: &[f64], limits: PlanLimits, id: Option<u64>) -> Result<(), PreparationError> {
    if values
        .iter()
        .any(|v| !v.is_finite() || v.abs() > limits.max_extent_mm)
    {
        return Err(invalid("world coordinate", id));
    }
    Ok(())
}
fn dimension(value: f64, limits: PlanLimits, id: Option<u64>) -> Result<(), PreparationError> {
    if !value.is_finite() || value < limits.min_feature_mm || value > limits.max_extent_mm {
        return Err(invalid("dimension", id));
    }
    Ok(())
}
#[derive(Debug)]
struct PlanData {
    source: SourceBinding,
    affine: Affine3,
    sampling: SamplingSpec,
    operations: Vec<Operation>,
    model: SurfaceKey,
    record: Vec<u8>,
    payload: usize,
}
/// Validated immutable parameters, explicitly not a prepared solid or sliceable result.
#[derive(Clone, Debug)]
pub struct PreparationPlan {
    inner: Arc<PlanData>,
}
impl PreparationPlan {
    /// Validates complete records and exact support bindings before publishing an immutable plan.
    /// Source digests are assertions; this does not inspect or modify an asset or mesh.
    ///
    /// # Errors
    /// Rejects invalid records/order/IDs, stale anchors, resource/allocation failure or cancellation.
    pub fn new(
        source: SourceBinding,
        sampling: SamplingSpec,
        operations: Vec<Operation>,
        limits: PlanLimits,
        cancel: &mut Cancellation<'_>,
    ) -> Result<Self, PreparationError> {
        cancel.check(Checkpoint::PreparationStart)?;
        if !limits.min_feature_mm.is_finite()
            || limits.min_feature_mm <= 0.
            || !limits.max_extent_mm.is_finite()
            || limits.max_extent_mm < limits.min_feature_mm
        {
            return Err(invalid("plan limits", None));
        }
        if operations.len() > limits.max_operations {
            return Err(resource("operation count"));
        }
        if source
            .placement
            .scale()
            .components()
            .iter()
            .any(|v| *v <= 0.)
        {
            return Err(invalid("positive placement scale", None));
        }
        dimension(sampling.cell_mm, limits, None)?;
        if !sampling.distance_error_mm.is_finite()
            || sampling.distance_error_mm <= 0.
            || sampling.distance_error_mm > sampling.cell_mm / 32.
            || sampling.max_cells == 0
        {
            return Err(invalid("sampling request", None));
        }
        let cap = limits.max_payload_bytes;
        let (mut bytes, affine) = encode_source(source, sampling, limits)?;
        validate_operations(&operations, limits, cancel)?;
        let model_count = operations
            .iter()
            .take_while(|o| !matches!(o.kind, OperationKind::Support(_)))
            .count();
        integer(
            &mut bytes,
            u64::try_from(model_count).map_err(|_| resource("model count"))?,
            cap,
        )?;
        for (index, op) in operations[..model_count].iter().enumerate() {
            cancel.poll(Checkpoint::PreparationRecords, index)?;
            encode_operation(&mut bytes, op, cap)?;
        }
        let model = SurfaceKey(Arc::from(bytes.as_slice()));
        integer(
            &mut bytes,
            u64::try_from(operations.len() - model_count).map_err(|_| resource("support count"))?,
            cap,
        )?;
        let mut anchor_payload = 0usize;
        for (index, op) in operations[model_count..].iter().enumerate() {
            cancel.poll(Checkpoint::PreparationRecords, index)?;
            if let OperationKind::Support(s) = &op.kind {
                if s.surface != model {
                    return Err(PreparationError::StaleAnchor { operation: op.id });
                }
                anchor_payload = anchor_payload
                    .checked_add(s.surface.0.len())
                    .ok_or_else(|| resource("anchor bytes"))?;
            }
            encode_operation(&mut bytes, op, cap)?;
        }
        let payload = operations
            .capacity()
            .checked_mul(size_of::<Operation>())
            .and_then(|v| v.checked_add(bytes.capacity()))
            .and_then(|v| v.checked_add(model.0.len()))
            .and_then(|v| v.checked_add(anchor_payload))
            .and_then(|v| v.checked_add(size_of::<PlanData>()))
            .ok_or_else(|| resource("plan payload"))?;
        if payload > cap {
            return Err(resource("plan payload"));
        }
        cancel.check(Checkpoint::PreparationPublish)?;
        Ok(Self {
            inner: Arc::new(PlanData {
                source,
                affine,
                sampling,
                operations,
                model,
                record: bytes,
                payload,
            }),
        })
    }
    /// Source assertion and placement snapshot.
    #[must_use]
    pub fn source(&self) -> SourceBinding {
        self.inner.source
    }
    /// Canonical flattened placement used by future evaluators; do not re-evaluate the raw transform.
    #[must_use]
    pub fn placement(&self) -> Affine3 {
        self.inner.affine
    }
    /// Explicit requested sampling settings.
    #[must_use]
    pub fn sampling(&self) -> SamplingSpec {
        self.inner.sampling
    }
    /// Immutable admitted operation records.
    #[must_use]
    pub fn operations(&self) -> &[Operation] {
        &self.inner.operations
    }
    /// Exact upstream model key for deliberately binding manual contacts.
    #[must_use]
    pub fn model_key(&self) -> SurfaceKey {
        self.inner.model.clone()
    }
    /// Version-one deterministic content identity; not a project-file loading format.
    #[must_use]
    pub fn canonical_record(&self) -> &[u8] {
        &self.inner.record
    }
    /// Conservative retained payload, with shared anchor data counted repeatedly.
    #[must_use]
    pub fn payload_bytes(&self) -> usize {
        self.inner.payload
    }
}
fn encode_source(
    source: SourceBinding,
    sampling: SamplingSpec,
    limits: PlanLimits,
) -> Result<(Vec<u8>, Affine3), PreparationError> {
    let mut bytes = Vec::new();
    let cap = limits.max_payload_bytes;
    append(&mut bytes, b"KPREP1", cap)?;
    append(&mut bytes, &source.original_sha256, cap)?;
    let affine = source
        .placement
        .to_affine()
        .map_err(|_| invalid("placement arithmetic", None))?;
    for v in affine
        .linear()
        .into_iter()
        .flatten()
        .chain(affine.translation().components())
    {
        scalar(&mut bytes, v, cap)?;
    }
    position(&affine.translation().components(), limits, None)?;
    scalar(&mut bytes, sampling.cell_mm, cap)?;
    scalar(&mut bytes, sampling.distance_error_mm, cap)?;
    integer(&mut bytes, sampling.max_cells, cap)?;

    Ok((bytes, affine))
}
#[allow(clippy::too_many_lines)] // One check per operation kind, in timeline order.
fn validate_operations(
    operations: &[Operation],
    limits: PlanLimits,
    cancel: &mut Cancellation<'_>,
) -> Result<(), PreparationError> {
    let mut stage = 0;
    let mut hollow = false;
    for index in 0..operations.len() {
        cancel.poll(Checkpoint::PreparationRecords, index)?;
        let id = operations[index].id;
        if id == 0 || operations[..index].iter().any(|o| o.id == id) {
            return Err(invalid("unique nonzero ID", Some(id)));
        }
        match &operations[index].kind {
            OperationKind::Repair(s) => {
                if index != 0 {
                    return Err(invalid("repair order", Some(id)));
                }
                if s.fill_holes.is_some_and(|f| f.max_hole_edges < 3) {
                    return Err(invalid("hole filling size bound", Some(id)));
                }
                s.check().map_err(|what| invalid(what, Some(id)))?;
            }
            OperationKind::Hollow(s) => {
                if hollow || stage != 0 {
                    return Err(invalid("hollow order", Some(id)));
                }
                dimension(s.thickness_mm, limits, Some(id))?;
                check_blockers(&s.blockers, limits, Some(id))?;
                if s.guarantee.is_some_and(|g| {
                    !(g.shortfall_coefficient > 0.0 && g.shortfall_coefficient <= 1.0)
                }) {
                    return Err(invalid("wall guarantee coefficient", Some(id)));
                }
                hollow = true;
            }
            OperationKind::Simplify(s) => {
                let after_hollow =
                    index > 0 && matches!(operations[index - 1].kind, OperationKind::Hollow(_));
                if !after_hollow {
                    return Err(invalid("simplification order", Some(id)));
                }
                dimension(s.tolerance_mm, limits, Some(id))?;
            }
            OperationKind::SurfaceHole(s) => {
                if !hollow || stage > 1 {
                    return Err(invalid("cut order", Some(id)));
                }
                stage = 1;
                position(&s.surface_mm, limits, Some(id))?;
                for v in [s.diameter_mm, s.outside_mm, s.inside_mm] {
                    dimension(v, limits, Some(id))?;
                }
                if let Some(tilt) = s.tilt {
                    if !(tilt.angle_deg.is_finite()
                        && (0.0..=MAX_HOLE_TILT_DEG).contains(&tilt.angle_deg)
                        && tilt.azimuth_deg.is_finite()
                        && tilt.azimuth_deg.abs() <= 360.0)
                    {
                        return Err(invalid("hole tilt", Some(id)));
                    }
                }
                if let Some(depth) = s.depth_mm {
                    dimension(depth, limits, Some(id))?;
                }
            }
            OperationKind::Hole(s) => {
                if !hollow || stage > 1 {
                    return Err(invalid("cut order", Some(id)));
                }
                stage = 1;
                dimension(s.length_mm, limits, Some(id))?;
                dimension(s.diameter_mm, limits, Some(id))?;
                position(&s.entry_mm, limits, Some(id))?;
                let direction = s
                    .unit_direction()
                    .map_err(|_| invalid("cut direction", Some(id)))?;
                position(
                    &std::array::from_fn::<_, 3, _>(|a| s.entry_mm[a] + direction[a] * s.length_mm),
                    limits,
                    Some(id),
                )?;
            }
            OperationKind::Support(s) => {
                stage = 2;
                position(&s.contact_mm, limits, Some(id))?;
                position(&s.base_xy_mm, limits, Some(id))?;
                for v in [
                    s.tip_radius_mm,
                    s.penetration_mm,
                    s.shaft_radius_mm,
                    s.base_radius_mm,
                    s.base_height_mm,
                ] {
                    dimension(v, limits, Some(id))?;
                }
                if s.contact_mm[2] <= s.base_height_mm
                    || s.tip_radius_mm > s.shaft_radius_mm
                    || s.shaft_radius_mm > s.base_radius_mm
                {
                    return Err(invalid("support proportions", Some(id)));
                }
            }
        }
    }
    Ok(())
}
/// A repair operation's record: the flags and hole bound, then the later stages' fields only
/// when set (TD-092), so earlier records are unchanged.
fn encode_repair(
    bytes: &mut Vec<u8>,
    s: &repair::RepairSpec,
    cap: usize,
) -> Result<(), PreparationError> {
    let flags = u8::from(s.duplicates)
        | u8::from(s.orientation) << 1
        | u8::from(s.outward) << 2
        | u8::from(s.fill_holes.is_some()) << 3
        | u8::from(s.allow_unfilled) << 4;
    append(bytes, &[3, flags], cap)?;
    let edges = s.fill_holes.map_or(0, |f| f.max_hole_edges);
    integer(
        bytes,
        u64::try_from(edges).map_err(|_| resource("hole size bound"))?,
        cap,
    )?;
    // Tolerant stages follow only when set, under markers no operation tag uses, so the
    // records of earlier repairs are unchanged (TD-092).
    for (marker, value) in [
        (0xF3, s.weld_mm),
        (0xF4, s.degenerate_mm),
        (0xF5, s.crack_mm),
    ] {
        if let Some(value) = value {
            append(bytes, &[marker], cap)?;
            scalar(bytes, value, cap)?;
        }
    }
    if let Some(fill) = s.fill_holes.filter(|f| f.split_rims || f.refine) {
        append(
            bytes,
            &[0xF7, u8::from(fill.split_rims) | u8::from(fill.refine) << 1],
            cap,
        )?;
    }
    if let Some(shells) = s.shells {
        append(bytes, &[0xF8], cap)?;
        scalar(bytes, shells.min_area_mm2, cap)?;
        scalar(bytes, shells.min_volume_mm3, cap)?;
        append(bytes, &[u8::from(shells.inner)], cap)?;
    }
    if s.visibility {
        append(bytes, &[0xF9], cap)?;
    }
    if let Some(resolution) = s.rebuild_mm {
        append(bytes, &[0xFA], cap)?;
        scalar(bytes, resolution, cap)?;
    }
    if let Some(mode) = s.non_manifold {
        let mode = match mode {
            repair::NonManifold::Fins => 1,
            repair::NonManifold::Aggressive => 2,
        };
        append(bytes, &[0xF6, mode], cap)?;
    }
    Ok(())
}

/// The most blockers one hollowing takes.
pub const MAX_HOLLOW_BLOCKERS: usize = 1024;

/// Refuse blockers out of the plan's ranges: a centre off the extent, a size not a dimension (a
/// frustum's upper base may be zero), axes not orthonormal, or too many.
fn check_blockers(
    blockers: &[HollowBlocker],
    limits: PlanLimits,
    id: Option<u64>,
) -> Result<(), PreparationError> {
    /// How far from orthonormal a frustum's axes may be: their rounding.
    const ORTHONORMAL: f64 = 1e-9;
    if blockers.len() > MAX_HOLLOW_BLOCKERS {
        return Err(invalid("hollow blocker count", id));
    }
    for blocker in blockers {
        match *blocker {
            HollowBlocker::Sphere { centre, radius_mm } => {
                position(&centre, limits, id)?;
                dimension(radius_mm, limits, id)?;
            }
            HollowBlocker::Frustum {
                centre,
                axes,
                bottom_mm,
                top_mm,
                height_mm,
            } => {
                position(&centre, limits, id)?;
                for v in bottom_mm.into_iter().chain([height_mm]) {
                    dimension(v, limits, id)?;
                }
                if top_mm
                    .iter()
                    .any(|&v| !(v.is_finite() && v >= 0.0 && v <= limits.max_extent_mm))
                {
                    return Err(invalid("dimension", id));
                }
                let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
                for i in 0..3 {
                    for j in 0..3 {
                        let expected = if i == j { 1.0 } else { 0.0 };
                        let off = (dot(axes[i], axes[j]) - expected).abs();
                        if off.is_nan() || off > ORTHONORMAL {
                            return Err(invalid("hollow blocker axes", id));
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

fn encode_operation(
    bytes: &mut Vec<u8>,
    op: &Operation,
    cap: usize,
) -> Result<(), PreparationError> {
    integer(bytes, op.id, cap)?;
    match &op.kind {
        OperationKind::Repair(s) => encode_repair(bytes, s, cap),
        OperationKind::Hollow(s) => {
            append(bytes, &[0], cap)?;
            scalar(bytes, s.thickness_mm, cap)?;
            // The guarantee follows only when requested, under a marker no operation tag uses, so
            // earlier records are unchanged (TD-093).
            if let Some(g) = s.guarantee {
                append(bytes, &[0xE1], cap)?;
                scalar(bytes, g.shortfall_coefficient, cap)?;
            }
            // Blockers follow likewise only when there are any (owner, 2026-11-02).
            for blocker in &s.blockers {
                match *blocker {
                    HollowBlocker::Frustum {
                        centre,
                        axes,
                        bottom_mm,
                        top_mm,
                        height_mm,
                    } => {
                        append(bytes, &[0xE2], cap)?;
                        for v in centre
                            .into_iter()
                            .chain(axes.into_iter().flatten())
                            .chain(bottom_mm)
                            .chain(top_mm)
                            .chain([height_mm])
                        {
                            scalar(bytes, v, cap)?;
                        }
                    }
                    HollowBlocker::Sphere { centre, radius_mm } => {
                        append(bytes, &[0xE3], cap)?;
                        for v in centre.into_iter().chain([radius_mm]) {
                            scalar(bytes, v, cap)?;
                        }
                    }
                }
            }
            Ok(())
        }
        OperationKind::Simplify(s) => {
            append(bytes, &[4], cap)?;
            scalar(bytes, s.tolerance_mm, cap)
        }
        OperationKind::SurfaceHole(s) => {
            append(bytes, &[5], cap)?;
            for v in s
                .surface_mm
                .into_iter()
                .chain([s.diameter_mm, s.outside_mm, s.inside_mm])
            {
                scalar(bytes, v, cap)?;
            }
            // Optional fields follow only when given, under markers no operation tag uses, so
            // the records of perpendicular holes with automatic depth are unchanged (TD-084).
            if let Some(tilt) = s.tilt {
                append(bytes, &[0xF1], cap)?;
                scalar(bytes, tilt.angle_deg, cap)?;
                scalar(bytes, tilt.azimuth_deg, cap)?;
            }
            if let Some(depth) = s.depth_mm {
                append(bytes, &[0xF2], cap)?;
                scalar(bytes, depth, cap)?;
            }
            Ok(())
        }
        OperationKind::Hole(s) => {
            append(bytes, &[1], cap)?;
            for v in s
                .entry_mm
                .into_iter()
                .chain(s.direction)
                .chain([s.length_mm, s.diameter_mm])
            {
                scalar(bytes, v, cap)?;
            }
            Ok(())
        }
        OperationKind::Support(s) => {
            append(bytes, &[2], cap)?;
            for v in s.contact_mm.into_iter().chain(s.base_xy_mm).chain([
                s.tip_radius_mm,
                s.penetration_mm,
                s.shaft_radius_mm,
                s.base_radius_mm,
                s.base_height_mm,
            ]) {
                scalar(bytes, v, cap)?;
            }
            Ok(())
        }
    }
}
/// Conservative limits for all retained history snapshots, including redo.
#[derive(Clone, Copy, Debug)]
pub struct HistoryLimits {
    /// Maximum current plus undo plus redo snapshots.
    pub max_snapshots: usize,
    /// Maximum summed retained plan payload; shared bytes are counted repeatedly.
    pub max_payload_bytes: usize,
}
impl Default for HistoryLimits {
    fn default() -> Self {
        Self {
            max_snapshots: 64,
            max_payload_bytes: 64 * 1024 * 1024,
        }
    }
}
/// Bounded transactional in-memory history; generation tokens are local to this history owner.
#[derive(Debug)]
pub struct PreparationHistory {
    states: Vec<PreparationPlan>,
    cursor: usize,
    generation: u64,
    limits: HistoryLimits,
}
impl PreparationHistory {
    /// Starts with one admitted snapshot; no geometry result or source is modified.
    ///
    /// # Errors
    /// Rejects count/payload limits or allocation failure.
    pub fn new(initial: PreparationPlan, limits: HistoryLimits) -> Result<Self, PreparationError> {
        if limits.max_snapshots == 0 || initial.payload_bytes() > limits.max_payload_bytes {
            return Err(resource("history limits"));
        }
        let mut states = Vec::new();
        states
            .try_reserve(1)
            .map_err(|_| resource("history allocation"))?;
        states.push(initial);
        Ok(Self {
            states,
            cursor: 0,
            generation: 0,
            limits,
        })
    }
    /// Current immutable plan.
    #[must_use]
    pub fn current(&self) -> &PreparationPlan {
        &self.states[self.cursor]
    }
    /// Monotonic generation, including undo/redo; do not compare across history owners.
    #[must_use]
    pub fn generation(&self) -> u64 {
        self.generation
    }
    /// Number of current, undo and redo snapshots retained.
    #[must_use]
    pub fn snapshot_count(&self) -> usize {
        self.states.len()
    }
    /// Tests both owner-local generation and exact content for stale background work admission.
    #[must_use]
    pub fn matches(&self, generation: u64, plan: &PreparationPlan) -> bool {
        generation == self.generation
            && plan.canonical_record() == self.current().canonical_record()
    }
    /// Commits a complete plan and drops redo only after all admission/cancellation checks succeed.
    ///
    /// # Errors
    /// Returns resource/allocation/generation exhaustion or cancellation without changing history.
    pub fn commit(
        &mut self,
        next: PreparationPlan,
        cancel: &mut Cancellation<'_>,
    ) -> Result<(), PreparationError> {
        cancel.check(Checkpoint::PreparationStart)?;
        let count = self
            .cursor
            .checked_add(2)
            .ok_or_else(|| resource("history count"))?;
        if count > self.limits.max_snapshots {
            return Err(resource("history snapshots"));
        }
        let mut total = next.payload_bytes();
        for (i, p) in self.states[..=self.cursor].iter().enumerate() {
            cancel.poll(Checkpoint::PreparationRecords, i)?;
            total = total
                .checked_add(p.payload_bytes())
                .ok_or_else(|| resource("history payload"))?;
        }
        if total > self.limits.max_payload_bytes {
            return Err(resource("history payload"));
        }
        let generation = self
            .generation
            .checked_add(1)
            .ok_or_else(|| resource("history generation"))?;
        self.states
            .try_reserve(1)
            .map_err(|_| resource("history allocation"))?;
        cancel.check(Checkpoint::PreparationPublish)?;
        self.states.truncate(self.cursor + 1);
        self.states.push(next);
        self.cursor += 1;
        self.generation = generation;
        Ok(())
    }
    /// Restores previous content, with a new generation. False means no previous snapshot.
    ///
    /// # Errors
    /// Returns generation exhaustion or cancellation without changing history.
    pub fn undo(&mut self, cancel: &mut Cancellation<'_>) -> Result<bool, PreparationError> {
        self.move_cursor(false, cancel)
    }
    /// Restores redo content, with a new generation. False means no redo snapshot.
    ///
    /// # Errors
    /// Returns generation exhaustion or cancellation without changing history.
    pub fn redo(&mut self, cancel: &mut Cancellation<'_>) -> Result<bool, PreparationError> {
        self.move_cursor(true, cancel)
    }
    fn move_cursor(
        &mut self,
        forward: bool,
        cancel: &mut Cancellation<'_>,
    ) -> Result<bool, PreparationError> {
        cancel.check(Checkpoint::PreparationStart)?;
        let next = if forward {
            self.cursor
                .checked_add(1)
                .filter(|v| *v < self.states.len())
        } else {
            self.cursor.checked_sub(1)
        };
        let Some(next) = next else {
            cancel.check(Checkpoint::PreparationPublish)?;
            return Ok(false);
        };
        let generation = self
            .generation
            .checked_add(1)
            .ok_or_else(|| resource("history generation"))?;
        cancel.check(Checkpoint::PreparationPublish)?;
        self.cursor = next;
        self.generation = generation;
        Ok(true)
    }
}
