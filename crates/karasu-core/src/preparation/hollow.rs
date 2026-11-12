//! Bounded inward hollowing of certified convex polyhedral source solids.
use super::{OperationKind, PreparationPlan};
use crate::{
    cancellation::{Cancellation, Cancelled, Checkpoint},
    geometry::Point3,
    mesh::{topology::TopologyError, MeshError, TriangleMesh},
};
use std::{error::Error, fmt, mem::size_of};
mod certificate;
mod clipping;
mod hull;
pub(crate) mod numeric;
mod validation;
use numeric::I;
/// Independent geometry admission and cumulative requested-allocation limits.
#[derive(Clone, Copy, Debug)]
pub struct HollowLimits {
    /// Original vertex cap.
    pub max_input_vertices: usize,
    /// Original triangle cap.
    pub max_input_triangles: usize,
    /// Unique support planes.
    pub max_planes: usize,
    /// Maximum retained construction or output vertices.
    pub max_output_vertices: usize,
    /// Maximum construction or output triangle/face count.
    pub max_output_triangles: usize,
    /// Charged traversal/predicate units.
    pub max_work: usize,
    /// Sum of requested geometry/analysis allocation bytes, excluding allocator metadata.
    pub max_payload_bytes: usize,
}
impl Default for HollowLimits {
    fn default() -> Self {
        Self {
            max_input_vertices: 2048,
            max_input_triangles: 4096,
            max_planes: 512,
            max_output_vertices: 20_000,
            max_output_triangles: 40_000,
            max_work: 50_000_000,
            max_payload_bytes: 256 * 1024 * 1024,
        }
    }
}
/// Atomic hollowing failure; no original geometry is repaired or mutated.
#[derive(Debug)]
pub enum HollowError {
    /// Cooperative cancellation.
    Cancelled(Cancelled),
    /// Invalid source or placement, with a relevant triangle when available.
    InvalidInput {
        /// Violated geometric/input invariant.
        reason: &'static str,
        /// Source triangle context.
        triangle: Option<usize>,
    },
    /// Unsupported recipe or nonconvex/multiple-solid domain.
    Unsupported {
        /// Explicit unsupported condition.
        reason: &'static str,
    },
    /// Checked count, requested storage, work or allocation failure.
    Resource {
        /// Resource or allocation that failed.
        resource: &'static str,
    },
    /// Construction cannot certify the requested accuracy or a nonempty cavity.
    Uncertain {
        /// Certification or construction failure.
        reason: &'static str,
    },
    /// Underlying checked mesh construction failure.
    Mesh(MeshError),
    /// Underlying topology analysis failure.
    Topology(TopologyError),
    /// The plan's source repair failed (TD-054).
    Repair(Box<super::repair::RepairError>),
    /// The wall guarantee (TD-093) did not reach the requested wall within its passes.
    WallNotReached {
        /// The requested wall, in mm.
        requested_mm: f64,
        /// The thickest wall certified by any pass, in mm (zero when no pass was certified).
        certified_mm: f64,
        /// Passes made.
        passes: u32,
    },
}
impl fmt::Display for HollowError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled(e) => e.fmt(f),
            Self::InvalidInput { reason, triangle } => write!(
                f,
                "invalid hollowing input: {reason} (triangle {triangle:?})"
            ),
            Self::Unsupported { reason } => write!(f, "unsupported hollowing input: {reason}"),
            Self::Resource { resource } => write!(f, "hollowing resource unavailable: {resource}"),
            Self::Uncertain { reason } => write!(f, "uncertified hollowing result: {reason}"),
            Self::Mesh(e) => e.fmt(f),
            Self::Topology(e) => e.fmt(f),
            Self::Repair(e) => write!(f, "source repair: {e}"),
            Self::WallNotReached {
                requested_mm,
                certified_mm,
                passes,
            } => write!(
                f,
                "the requested {requested_mm} mm wall was not certified in {passes} passes; the \
                 thickest certified wall was {certified_mm} mm"
            ),
        }
    }
}
impl Error for HollowError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Cancelled(e) => Some(e),
            Self::Mesh(e) => Some(e),
            Self::Topology(e) => Some(e),
            Self::Repair(e) => Some(e.as_ref()),
            _ => None,
        }
    }
}
impl From<Cancelled> for HollowError {
    fn from(e: Cancelled) -> Self {
        Self::Cancelled(e)
    }
}
impl From<MeshError> for HollowError {
    fn from(e: MeshError) -> Self {
        Self::Mesh(e)
    }
}
impl From<TopologyError> for HollowError {
    fn from(e: TopologyError) -> Self {
        Self::Topology(e)
    }
}
fn resource(resource: &'static str) -> HollowError {
    HollowError::Resource { resource }
}
fn uncertain(reason: &'static str) -> HollowError {
    HollowError::Uncertain { reason }
}
fn invalid(reason: &'static str, triangle: Option<usize>) -> HollowError {
    HollowError::InvalidInput { reason, triangle }
}
struct Work {
    limits: HollowLimits,
    used: usize,
    requested: usize,
}
impl Work {
    fn charge(&mut self, count: usize, cancel: &mut Cancellation<'_>) -> Result<(), HollowError> {
        let next = self
            .used
            .checked_add(count)
            .ok_or_else(|| resource("work arithmetic"))?;
        if next > self.limits.max_work {
            return Err(resource("work"));
        }
        if self.used / 256 != next / 256 || self.used == 0 {
            cancel.check(Checkpoint::HollowWork)?;
        }
        self.used = next;
        Ok(())
    }
    fn account(&mut self, bytes: usize) -> Result<(), HollowError> {
        self.requested = self
            .requested
            .checked_add(bytes)
            .ok_or_else(|| resource("payload arithmetic"))?;
        if self.requested > self.limits.max_payload_bytes {
            return Err(resource("requested payload"));
        }
        Ok(())
    }
    fn reserve<T>(&mut self, vec: &mut Vec<T>, additional: usize) -> Result<(), HollowError> {
        let needed = vec
            .len()
            .checked_add(additional)
            .ok_or_else(|| resource("vector arithmetic"))?;
        if needed > vec.capacity() {
            self.account(
                (needed - vec.capacity())
                    .checked_mul(size_of::<T>())
                    .ok_or_else(|| resource("vector bytes"))?,
            )?;
            vec.try_reserve_exact(additional)
                .map_err(|_| resource("allocation"))?;
        }
        Ok(())
    }
    fn push<T>(&mut self, vec: &mut Vec<T>, value: T) -> Result<(), HollowError> {
        self.reserve(vec, 1)?;
        vec.push(value);
        Ok(())
    }
}
#[derive(Clone, Copy)]
struct Plane {
    anchor: Point3,
    normal: [f64; 3],
    interval: [I; 3],
}
fn point(p: Point3) -> [f64; 3] {
    p.coordinates()
}
fn vector(a: Point3, b: Point3) -> [I; 3] {
    std::array::from_fn(|i| I::point(point(a)[i]).sub(I::point(point(b)[i])))
}
fn cross(a: [I; 3], b: [I; 3]) -> [I; 3] {
    [
        a[1].mul(b[2]).sub(a[2].mul(b[1])),
        a[2].mul(b[0]).sub(a[0].mul(b[2])),
        a[0].mul(b[1]).sub(a[1].mul(b[0])),
    ]
}
fn dot(a: [I; 3], b: [I; 3]) -> I {
    a[0].mul(b[0]).add(a[1].mul(b[1])).add(a[2].mul(b[2]))
}
fn normal(a: Point3, b: Point3, c: Point3) -> Result<[I; 3], HollowError> {
    let n = cross(vector(b, a), vector(c, a));
    let length = dot(n, n)
        .sqrt()
        .ok_or_else(|| uncertain("face normal length"))?;
    let mut result = [I::point(0.); 3];
    for i in 0..3 {
        result[i] = n[i]
            .div(length)
            .filter(|v| v.is_finite())
            .ok_or_else(|| uncertain("face normal interval"))?;
    }
    Ok(result)
}
/// Outward certified volume interval for represented triangle geometry, in cubic millimeters.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VolumeBounds {
    /// Lower volume bound.
    pub minimum_mm3: f64,
    /// Upper volume bound.
    pub maximum_mm3: f64,
}
/// Certified convex erosion bracket; not a global surface-distance or printability certificate.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HollowCertificate {
    /// Delta with K eroded by t+delta inside cavity, inside K eroded by t-delta.
    pub wall_offset_error_mm: f64,
    /// Volume interval of the represented placed exterior.
    pub source_volume: VolumeBounds,
    /// Volume interval of the emitted represented cavity.
    pub cavity_volume: VolumeBounds,
    /// Exterior minus cavity volume interval.
    pub material_volume: VolumeBounds,
}
/// Complete hollowed geometry with original placed exterior and reversed cavity boundary.
#[derive(Debug)]
pub struct HollowedModel {
    mesh: TriangleMesh,
    cavity: TriangleMesh,
    certificate: HollowCertificate,
    plan: Vec<u8>,
    work_units: usize,
    requested_bytes: usize,
}
impl HollowedModel {
    /// Sliceable shell mesh, preserving exterior vertex/face order before appended cavity geometry.
    #[must_use]
    pub fn mesh(&self) -> &TriangleMesh {
        &self.mesh
    }
    /// Outward-oriented cavity surface for headless cavity previews.
    #[must_use]
    pub fn cavity(&self) -> &TriangleMesh {
        &self.cavity
    }
    /// Geometric inclusion/volume certificate for this represented output.
    #[must_use]
    pub fn certificate(&self) -> HollowCertificate {
        self.certificate
    }
    /// Exact parameter record associated with this result; source digest remains a loader assertion.
    #[must_use]
    pub fn plan_record(&self) -> &[u8] {
        &self.plan
    }
    /// Algorithm identity distinct from the operation-plan schema.
    #[must_use]
    pub const fn backend_id(&self) -> &'static str {
        "convex-halfspaces-v1"
    }
    /// Charged work, excluding indivisible sorts/allocations and subordinate topology traversal.
    #[must_use]
    pub fn work_units(&self) -> usize {
        self.work_units
    }
    /// Cumulative requested allocation bytes including explicit subordinate analysis allowances.
    #[must_use]
    pub fn requested_bytes(&self) -> usize {
        self.requested_bytes
    }
}
fn placed_source(
    source: &TriangleMesh,
    plan: &PreparationPlan,
    work: &mut Work,
    cancel: &mut Cancellation<'_>,
) -> Result<TriangleMesh, HollowError> {
    if source.vertices().len() > work.limits.max_input_vertices
        || source.triangles().len() > work.limits.max_input_triangles
    {
        return Err(resource("source count"));
    }
    if source.vertices().len() > work.limits.max_output_vertices
        || source.triangles().len() > work.limits.max_output_triangles
    {
        return Err(resource("minimum shell output count"));
    }
    if source.vertices().is_empty() || source.triangles().is_empty() {
        return Err(invalid("empty source", None));
    }
    let mut points = Vec::new();
    work.reserve(&mut points, source.vertices().len())?;
    for &p in source.vertices() {
        work.charge(1, cancel)?;
        let p = plan
            .placement()
            .transform_point(p)
            .map_err(|_| invalid("placement arithmetic", None))?;
        if p.coordinates().iter().any(|v| v.abs() > 10_000.) {
            return Err(HollowError::Unsupported {
                reason: "placed coordinate exceeds +/-10000 mm",
            });
        }
        points.push(p);
    }
    let mut faces = Vec::new();
    work.reserve(&mut faces, source.triangles().len())?;
    faces.extend_from_slice(source.triangles());
    Ok(TriangleMesh::try_new_with_cancellation(
        points, faces, cancel,
    )?)
}
fn planes_for(
    source: &TriangleMesh,
    ids: &[usize],
    work: &mut Work,
    cancel: &mut Cancellation<'_>,
) -> Result<Vec<Plane>, HollowError> {
    let mut planes = Vec::new();
    work.reserve(&mut planes, ids.len())?;
    for &id in ids {
        work.charge(1, cancel)?;
        let [a, b, c] = source.triangles()[id].map(|i| source.vertices()[i as usize]);
        let interval = normal(a, b, c)?;
        let normal = interval.map(|v| v.lo / 2. + v.hi / 2.);
        planes.push(Plane {
            anchor: a,
            normal,
            interval,
        });
    }
    Ok(planes)
}
fn assemble(
    source: &TriangleMesh,
    cavity: &TriangleMesh,
    work: &mut Work,
    cancel: &mut Cancellation<'_>,
) -> Result<TriangleMesh, HollowError> {
    let mut vertices = Vec::new();
    let mut triangles = Vec::new();
    let nv = source
        .vertices()
        .len()
        .checked_add(cavity.vertices().len())
        .ok_or_else(|| resource("output vertices"))?;
    let nt = source
        .triangles()
        .len()
        .checked_add(cavity.triangles().len())
        .ok_or_else(|| resource("output triangles"))?;
    if nv > work.limits.max_output_vertices || nt > work.limits.max_output_triangles {
        return Err(resource("shell output count"));
    }
    work.reserve(&mut vertices, nv)?;
    vertices.extend_from_slice(source.vertices());
    vertices.extend_from_slice(cavity.vertices());
    work.reserve(&mut triangles, nt)?;
    triangles.extend_from_slice(source.triangles());
    let offset =
        u32::try_from(source.vertices().len()).map_err(|_| resource("output vertex index"))?;
    for &[a, b, c] in cavity.triangles() {
        work.charge(1, cancel)?;
        triangles.push([
            a.checked_add(offset)
                .ok_or_else(|| resource("output vertex index"))?,
            c.checked_add(offset)
                .ok_or_else(|| resource("output vertex index"))?,
            b.checked_add(offset)
                .ok_or_else(|| resource("output vertex index"))?,
        ]);
    }
    Ok(TriangleMesh::try_new_with_cancellation(
        vertices, triangles, cancel,
    )?)
}
/// Named deterministic phases, separate from cancellation polling.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum HollowStage {
    /// Source placement, topology and exact convex admission.
    Source,
    /// Inward halfspace clipping proposal.
    Construction,
    /// Exact convex hull and embedded boundary validation.
    Reconstruction,
    /// Independent interval erosion/volume certificate.
    Certification,
    /// Combined shell construction and final publication checks.
    Assembly,
    /// Final publication check passed; later cancellation need not prevent return.
    Published,
}
/// Phase-level advisory progress; no partially successful geometry is exposed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HollowProgress {
    /// Named stage about to start, or final publication notification.
    pub stage: HollowStage,
    /// Completed stages, monotonically increasing from zero through five.
    pub completed_stages: u8,
    /// Fixed total for this algorithm version.
    pub total_stages: u8,
}
fn notify(progress: &mut dyn FnMut(HollowProgress), stage: HollowStage, completed_stages: u8) {
    progress(HollowProgress {
        stage,
        completed_stages,
        total_stages: 5,
    });
}
/// Constructs and certifies one convex inward shell without changing the original source.
/// The plan must contain exactly one hollow operation; cuts/supports belong to subsequent steps.
/// The source SHA assertion must be verified against source bytes by the asset owner.
///
/// # Errors
/// Rejects unsupported recipes/nonconvex solids, invalid surfaces, unresolvable cavity/error bounds,
/// numeric range, work/storage limits, allocation failure or cooperative cancellation. No partial
/// successful geometry escapes and no thickness/resolution is silently clamped.
pub fn hollow_mesh(
    source: &TriangleMesh,
    plan: &PreparationPlan,
    limits: HollowLimits,
    cancel: &mut Cancellation<'_>,
) -> Result<HollowedModel, HollowError> {
    hollow_mesh_with_progress(source, plan, limits, cancel, &mut |_| {})
}
/// Runs hollowing with deterministic phase-level progress on the caller's thread.
/// Callbacks must return promptly and must not panic. They are advisory, not a result or certificate;
/// requests after the final publication check need not prevent success.
///
/// # Errors
/// Returns the same structured atomic failures as [`hollow_mesh`].
pub fn hollow_mesh_with_progress(
    source: &TriangleMesh,
    plan: &PreparationPlan,
    limits: HollowLimits,
    cancel: &mut Cancellation<'_>,
    progress: &mut dyn FnMut(HollowProgress),
) -> Result<HollowedModel, HollowError> {
    cancel.check(Checkpoint::HollowStart)?;
    let [operation] = plan.operations() else {
        return Err(HollowError::Unsupported {
            reason: "exactly one hollow operation is required",
        });
    };
    let OperationKind::Hollow(spec) = &operation.kind else {
        return Err(HollowError::Unsupported {
            reason: "cuts and supports are not implemented by hollowing",
        });
    };
    if !spec.blockers.is_empty() {
        return Err(HollowError::Unsupported {
            reason: "hollow blockers are implemented by sampled hollowing only",
        });
    }
    let t = spec.thickness_mm;
    let error = plan.sampling().distance_error_mm;
    if error >= t / 4. {
        return Err(invalid(
            "requested offset error must be less than thickness/4",
            None,
        ));
    }
    let mut work = Work {
        limits,
        used: 0,
        requested: 0,
    };
    notify(progress, HollowStage::Source, 0);
    let world = placed_source(source, plan, &mut work, cancel)?;
    let ids = validation::validate(&world, &mut work, cancel)?;
    notify(progress, HollowStage::Construction, 1);
    let planes = planes_for(&world, &ids, &mut work, cancel)?;
    let proposal = clipping::propose(&world, &planes, t, error, &mut work, cancel)?;
    notify(progress, HollowStage::Reconstruction, 2);
    let cavity = hull::build(proposal, &mut work, cancel)?;
    // The same geometric proof validates generated cavity faces under output, rather than input, caps.
    let input = work.limits;
    work.limits.max_input_vertices = limits.max_output_vertices;
    work.limits.max_input_triangles = limits.max_output_triangles;
    work.limits.max_planes = limits.max_output_triangles;
    let checked = validation::validate(&cavity, &mut work, cancel);
    work.limits = input;
    checked?;
    notify(progress, HollowStage::Certification, 3);
    cancel.check(Checkpoint::HollowCertificate)?;
    let certificate = certificate::certify(&world, &cavity, &planes, t, error, &mut work, cancel)?;
    notify(progress, HollowStage::Assembly, 4);
    let mesh = assemble(&world, &cavity, &mut work, cancel)?;
    let mut record = Vec::new();
    work.reserve(&mut record, plan.canonical_record().len())?;
    record.extend_from_slice(plan.canonical_record());
    cancel.check(Checkpoint::HollowPublish)?;
    notify(progress, HollowStage::Published, 5);
    Ok(HollowedModel {
        mesh,
        cavity,
        certificate,
        plan: record,
        work_units: work.used,
        requested_bytes: work.requested,
    })
}

// Support lofts are explicitly defined as convex hulls of adjacent represented profile rings.
// Reuse the existing independently validated hull, never silently repair an input source mesh.
pub(crate) fn convex_component(
    points: Vec<Point3>,
    limits: HollowLimits,
    cancel: &mut Cancellation<'_>,
) -> Result<TriangleMesh, HollowError> {
    let mut work = Work {
        limits,
        used: 0,
        requested: 0,
    };
    let mesh = hull::build(points, &mut work, cancel)?;
    // Validate the generated hull under output caps, as for a generated cavity: a loft between
    // two independently rotated 256-gons has up to 514 face planes.
    work.limits.max_input_vertices = limits.max_output_vertices;
    work.limits.max_input_triangles = limits.max_output_triangles;
    work.limits.max_planes = limits.max_output_triangles;
    validation::validate(&mesh, &mut work, cancel)?;
    Ok(mesh)
}

// Solid support sources use the same source admission as hollowing without inventing a cavity.
pub(crate) fn convex_source(
    source: &TriangleMesh,
    plan: &PreparationPlan,
    limits: HollowLimits,
    cancel: &mut Cancellation<'_>,
) -> Result<TriangleMesh, HollowError> {
    let mut work = Work {
        limits,
        used: 0,
        requested: 0,
    };
    let world = placed_source(source, plan, &mut work, cancel)?;
    validation::validate(&world, &mut work, cancel)?;
    Ok(world)
}

#[cfg(test)]
mod tests {
    use super::{convex_component, HollowLimits};
    use crate::{cancellation::Cancellation, geometry::Point3};

    #[test]
    fn twisted_lofts_with_more_faces_than_the_plane_cap_are_admitted() {
        // Two 64-gons, the upper one rotated by half a step: the hull is an antiprism with 128
        // side faces and two caps (130 planes), above a 128-plane input cap. Generated hulls are
        // validated under output caps (with the 256-vertex profile maximum and the default
        // 512-plane support component cap, the same shape has 514 planes).
        let ring = |z: f64, offset: f64| {
            (0..64).map(move |k| {
                let angle = std::f64::consts::TAU * (f64::from(k) + offset) / 64.0;
                Point3::new(10.0 * angle.cos(), 10.0 * angle.sin(), z)
            })
        };
        let points: Vec<Point3> = ring(0.0, 0.0).chain(ring(5.0, 0.5)).collect();
        let limits = HollowLimits {
            max_input_vertices: 128,
            max_input_triangles: 512,
            max_planes: 128,
            max_output_vertices: 128,
            max_output_triangles: 512,
            max_work: 20_000_000,
            max_payload_bytes: 16 * 1024 * 1024,
        };
        let mesh = convex_component(points, limits, &mut Cancellation::never()).unwrap();
        assert_eq!(mesh.vertices().len(), 128);
        assert_eq!(mesh.triangles().len(), 128 + 2 * 62);
    }
}
