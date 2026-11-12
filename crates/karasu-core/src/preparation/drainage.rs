//! Certified finite drainage openings evaluated by exact layer-domain subtraction.
//!
//! The scene stores separate shell/cutter boundaries, not a Boolean triangle mesh. Only the
//! material difference produced by [`DrainedModel::layer_engine`] represents the drained solid.
//! The initial backend admits axis-parallel cuts of one certified convex hollow model.
use super::{
    hollow::{hollow_mesh, numeric::I, HollowError, HollowLimits, HollowedModel},
    OperationKind, PlanLimits, PreparationError, PreparationPlan,
};
use crate::{
    cancellation::{Cancellation, Cancelled, Checkpoint},
    geometry::Point3,
    mesh::{MeshError, TriangleMesh},
    slicing::{
        construct::ConstructLimits,
        engine::{LayerConfig, LayerEngine, LayerProgress},
        schedule::LayerSchedule,
        SectionTransform,
    },
};
use std::{error::Error, fmt};
pub(crate) mod cylinder;

/// Independent drainage policy; subordinate hollowing and layer construction retain their own caps.
#[derive(Clone, Copy, Debug)]
pub struct DrainageLimits {
    /// Source/cavity construction limits.
    pub hollow: HollowLimits,
    /// Maximum finite cuts in a scene.
    pub max_holes: usize,
    /// Maximum sides per polygonal cylinder.
    pub max_segments: usize,
    /// Charged containment and overlap traversal units.
    pub max_work: usize,
    /// Conservative aggregate allocation admission, including subordinate allowances.
    pub max_payload_bytes: usize,
    /// Required strict end-cap/exterior and inter-cutter separation in millimeters.
    pub minimum_clearance_mm: f64,
}
impl Default for DrainageLimits {
    fn default() -> Self {
        Self {
            hollow: HollowLimits::default(),
            max_holes: 32,
            max_segments: 4096,
            max_work: 50_000_000,
            max_payload_bytes: 256 * 1024 * 1024,
            minimum_clearance_mm: 1e-6,
        }
    }
}
/// A cut that cannot certify the requested geometric connection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpeningIssue {
    /// The entry cap is not certified to lie entirely in the exterior (convex backend: beyond
    /// one supporting plane; sampled backend: disjoint, clear and in the unbounded exterior).
    EntryNotExterior,
    /// End-cap center is outside the model (the convex backend also reports a center on its
    /// boundary here).
    MissedModel,
    /// End-cap center is in the material (sampled backend: or exactly on the model surface) and
    /// the cut does not reach a cavity before it.
    Shallow,
    /// The whole terminal cap lacks the required certified cavity clearance (sampled backend: its
    /// center is in the cavity or exactly on the cavity surface).
    InsufficientClearance,
    /// End-cap center is in the material, or exactly on the model surface, after the cut passed
    /// through a cavity: the cut is too long. Reported by the sampled backend only.
    Overshoot,
    /// None of the 26 axis and diagonal escape segments joins the entry to the outside of the
    /// model's bounding box, and the fallback certificate cannot apply because the source
    /// surfaces touch, overlap or self-intersect. Reported by the sampled backend only.
    SourceNotEmbedded,
}
/// Atomic drainage failure; no successful partially cut scene is returned.
#[derive(Debug)]
pub enum DrainageError {
    /// Observed cooperative cancellation.
    Cancelled(Cancelled),
    /// Immutable plan admission failure.
    Plan(PreparationError),
    /// Source/cavity construction failure.
    Hollow(HollowError),
    /// Checked mesh boundary assembly failed.
    Mesh(MeshError),
    /// Recipe or geometric domain is unsupported.
    Unsupported {
        /// Domain restriction.
        reason: &'static str,
    },
    /// An independent resource limit or fallible allocation failed.
    Resource {
        /// Resource context.
        resource: &'static str,
    },
    /// Numerical bounds cannot certify the requested approximation.
    Uncertain {
        /// Failed proof.
        reason: &'static str,
    },
    /// A particular operation lacks a certified opening.
    Opening {
        /// Original stable operation ID.
        operation_id: u64,
        /// Geometric failure category.
        issue: OpeningIssue,
    },
    /// The polygonal cylinder for one cut cannot be built as requested (axis, size or error).
    /// Reported by the sampled backend, which names the operation.
    Cut {
        /// Original stable operation ID.
        operation_id: u64,
        /// Underlying cylinder construction failure.
        reason: &'static str,
    },
    /// Cutters are not proven disjoint by the conservative bounding-box policy.
    PotentialOverlap {
        /// Earlier operation ID.
        first: u64,
        /// Later operation ID.
        second: u64,
    },
}
impl fmt::Display for DrainageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled(e) => e.fmt(f),
            Self::Plan(e) => e.fmt(f),
            Self::Hollow(e) => e.fmt(f),
            Self::Mesh(e) => e.fmt(f),
            Self::Unsupported { reason } => write!(f, "unsupported drainage: {reason}"),
            Self::Resource { resource } => write!(f, "drainage resource unavailable: {resource}"),
            Self::Uncertain { reason } => write!(f, "uncertified drainage: {reason}"),
            Self::Opening {
                operation_id,
                issue,
            } => write!(f, "drainage operation {operation_id}: {issue:?}"),
            Self::Cut {
                operation_id,
                reason,
            } => write!(f, "drainage operation {operation_id}: {reason}"),
            Self::PotentialOverlap { first, second } => write!(
                f,
                "drainage operations {first} and {second} are not certified separated"
            ),
        }
    }
}
impl Error for DrainageError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Cancelled(e) => Some(e),
            Self::Plan(e) => Some(e),
            Self::Hollow(e) => Some(e),
            Self::Mesh(e) => Some(e),
            _ => None,
        }
    }
}
impl From<Cancelled> for DrainageError {
    fn from(e: Cancelled) -> Self {
        Self::Cancelled(e)
    }
}
impl From<MeshError> for DrainageError {
    fn from(e: MeshError) -> Self {
        Self::Mesh(e)
    }
}
fn resource(resource: &'static str) -> DrainageError {
    DrainageError::Resource { resource }
}

/// A proven positive-area connection to the single connected convex cavity.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DrainageOpening {
    /// Stable source operation ID, in original plan order.
    pub operation_id: u64,
    /// Number of sides in the represented polygonal cut.
    pub segments: usize,
    /// Independently bounded radial deviation from the requested cylinder, in mm.
    pub radial_error_mm: f64,
    /// Independently bounded axial endpoint deviation, in mm.
    pub axial_error_mm: f64,
    /// Certified minimum terminal polygon-cap clearance within the cavity, in mm.
    pub terminal_clearance_mm: f64,
}
/// Immutable prepared CSG scene. Its boundary input mesh is deliberately not exposed as final geometry.
#[derive(Debug)]
pub struct DrainedModel {
    hollow: HollowedModel,
    boundaries: TriangleMesh,
    labels: Vec<u64>,
    openings: Vec<DrainageOpening>,
    plan: Vec<u8>,
}
impl DrainedModel {
    pub(crate) fn boundary_scene(&self) -> (&TriangleMesh, &[u64]) {
        (&self.boundaries, &self.labels)
    }
    /// Original uncut hollowing result for comparison/previews, not drained output.
    #[must_use]
    pub fn hollowed_model(&self) -> &HollowedModel {
        &self.hollow
    }
    /// Original plan identity including all ordered cuts; digests remain loader assertions.
    #[must_use]
    pub fn plan_record(&self) -> &[u8] {
        &self.plan
    }
    /// Certified openings in operation order.
    #[must_use]
    pub fn openings(&self) -> &[DrainageOpening] {
        &self.openings
    }
    /// Number of geometrically enclosed cavity components. No physical drainage guarantee.
    #[must_use]
    pub fn enclosed_cavities(&self) -> usize {
        usize::from(self.openings.is_empty())
    }
    /// Backend identity distinct from the canonical plan format.
    #[must_use]
    pub const fn backend_id(&self) -> &'static str {
        "convex-cardinal-layer-difference-v1"
    }
    /// Evaluate actual drained material with existing section ownership and exact planar difference.
    ///
    /// Coordinates are retained as represented f64 values; planar intersection construction is exact
    /// for those represented sections. Per-layer failures and stream state follow [`LayerEngine`].
    #[must_use]
    pub fn layer_engine<'a, 'c, 'o>(
        &'a self,
        schedule: &'a LayerSchedule,
        config: LayerConfig,
        construct: ConstructLimits,
        cancel: Cancellation<'c>,
        observer: Option<&'o mut dyn FnMut(LayerProgress)>,
    ) -> LayerEngine<'a, 'c, 'o> {
        LayerEngine::new(
            &self.boundaries,
            schedule,
            SectionTransform::Identity,
            config,
            cancel,
            observer,
        )
        .with_represented_coordinates()
        .with_material_difference(&self.labels, construct)
    }
}
struct Work {
    used: usize,
    limit: usize,
}
impl Work {
    fn tick(&mut self, cancel: &mut Cancellation<'_>) -> Result<(), DrainageError> {
        self.used = self
            .used
            .checked_add(1)
            .ok_or_else(|| resource("work arithmetic"))?;
        if self.used > self.limit {
            return Err(resource("containment work"));
        }
        cancel.poll(Checkpoint::DrainageWork, self.used)?;
        Ok(())
    }
}
fn vec(a: Point3, b: Point3) -> [I; 3] {
    std::array::from_fn(|i| I::point(a.coordinates()[i]).sub(I::point(b.coordinates()[i])))
}
fn dot(a: [I; 3], b: [I; 3]) -> I {
    a[0].mul(b[0]).add(a[1].mul(b[1])).add(a[2].mul(b[2]))
}
fn plane(mesh: &TriangleMesh, face: [u32; 3], query: Point3) -> Result<I, DrainageError> {
    let [origin, first, second] = face.map(|vertex| mesh.vertices()[vertex as usize]);
    let edge = vec(first, origin);
    let other = vec(second, origin);
    let normal = [
        edge[1].mul(other[2]).sub(edge[2].mul(other[1])),
        edge[2].mul(other[0]).sub(edge[0].mul(other[2])),
        edge[0].mul(other[1]).sub(edge[1].mul(other[0])),
    ];
    let norm = dot(normal, normal).sqrt().ok_or(DrainageError::Uncertain {
        reason: "support normal length",
    })?;
    dot(normal, vec(query, origin))
        .div(norm)
        .filter(|value| value.is_finite())
        .ok_or(DrainageError::Uncertain {
            reason: "support distance",
        })
}
fn clearance(
    mesh: &TriangleMesh,
    faces: &[[u32; 3]],
    points: &[Point3],
    work: &mut Work,
    cancel: &mut Cancellation<'_>,
) -> Result<f64, DrainageError> {
    let mut result = f64::INFINITY;
    for &face in faces {
        for &p in points {
            work.tick(cancel)?;
            result = result.min(-plane(mesh, face, p)?.hi);
        }
    }
    Ok(result)
}
fn outside(
    mesh: &TriangleMesh,
    faces: &[[u32; 3]],
    points: &[Point3],
    margin: f64,
    work: &mut Work,
    cancel: &mut Cancellation<'_>,
) -> Result<bool, DrainageError> {
    for &face in faces {
        let mut all = true;
        for &p in points {
            work.tick(cancel)?;
            if plane(mesh, face, p)?.lo <= margin {
                all = false;
                break;
            }
        }
        if all {
            return Ok(true);
        }
    }
    Ok(false)
}
fn bounds(points: &[Point3]) -> [[f64; 2]; 3] {
    std::array::from_fn(|i| {
        points
            .iter()
            .fold([f64::INFINITY, f64::NEG_INFINITY], |b, p| {
                [b[0].min(p.coordinates()[i]), b[1].max(p.coordinates()[i])]
            })
    })
}
fn reserve<T>(v: &mut Vec<T>, n: usize) -> Result<(), DrainageError> {
    v.try_reserve_exact(n).map_err(|_| resource("allocation"))
}

/// Construct a complete immutable drained scene from a hollow-first plan followed by zero or more cuts.
///
/// Cuts must be axis-parallel, have an entirely exterior entry cap and terminate wholly inside the
/// convex cavity with explicit clearance. Potentially overlapping cutter boxes are rejected, even
/// if a more general solver could separate their solids. Edits/removal require recomputing the new
/// immutable plan; old results retain their original identity. No cached success is relabelled.
/// # Errors
/// Returns structured recipe, source, numerical, opening, overlap, resource and cancellation errors.
pub fn drain_model(
    source: &TriangleMesh,
    plan: &PreparationPlan,
    limits: DrainageLimits,
    cancel: &mut Cancellation<'_>,
) -> Result<DrainedModel, DrainageError> {
    cancel.check(Checkpoint::DrainageStart)?;
    if !limits.minimum_clearance_mm.is_finite() || limits.minimum_clearance_mm <= 0. {
        return Err(DrainageError::Unsupported {
            reason: "clearance must be positive finite millimeters",
        });
    }
    let Some(first) = plan.operations().first() else {
        return Err(DrainageError::Unsupported {
            reason: "hollow-first recipe required",
        });
    };
    if !matches!(first.kind, OperationKind::Hollow(_))
        || plan.operations()[1..]
            .iter()
            .any(|o| !matches!(o.kind, OperationKind::Hole(_)))
    {
        return Err(DrainageError::Unsupported {
            reason: "only hollow followed by drainage cuts is supported",
        });
    }
    let count = plan.operations().len() - 1;
    if count > limits.max_holes || limits.max_segments > 4096 {
        return Err(resource("hole or segment limit"));
    }
    // Reserve a conservative envelope for all cutter proposals, cap lists, merged buffers and records.
    let shell_copy = limits
        .hollow
        .max_output_vertices
        .checked_mul(24)
        .and_then(|n| {
            limits
                .hollow
                .max_output_triangles
                .checked_mul(20)
                .and_then(|m| n.checked_add(m))
        })
        .ok_or_else(|| resource("shell copy admission"))?;
    let allowance = count
        .checked_mul(limits.max_segments)
        .and_then(|n| n.checked_mul(1024))
        .and_then(|n| n.checked_add(plan.canonical_record().len()))
        .and_then(|n| n.checked_add(shell_copy))
        .and_then(|n| count.checked_mul(4096).and_then(|m| n.checked_add(m)))
        .ok_or_else(|| resource("payload arithmetic"))?;
    if allowance > limits.max_payload_bytes {
        return Err(resource("payload admission"));
    }
    let mut hollow_limits = limits.hollow;
    hollow_limits.max_payload_bytes = hollow_limits
        .max_payload_bytes
        .min(limits.max_payload_bytes - allowance);
    let mut prefix_operations = Vec::new();
    reserve(&mut prefix_operations, 1)?;
    prefix_operations.push(first.clone());
    let prefix = PreparationPlan::new(
        plan.source(),
        plan.sampling(),
        prefix_operations,
        PlanLimits::default(),
        cancel,
    )
    .map_err(DrainageError::Plan)?;
    let hollow =
        hollow_mesh(source, &prefix, hollow_limits, cancel).map_err(DrainageError::Hollow)?;
    let cylinder_work = count
        .checked_mul(limits.max_segments)
        .and_then(|n| n.checked_mul(100))
        .ok_or_else(|| resource("cylinder work admission"))?;
    if cylinder_work > limits.max_work {
        return Err(resource("cylinder work admission"));
    }
    let mut work = Work {
        used: cylinder_work,
        limit: limits.max_work,
    };
    let (cutters, openings) = evaluate_cuts(&hollow, plan, limits, &mut work, cancel)?;
    let (boundaries, labels) = assemble_scene(&hollow, &cutters, &mut work, cancel)?;
    let mut record = Vec::new();
    reserve(&mut record, plan.canonical_record().len())?;
    record.extend_from_slice(plan.canonical_record());
    cancel.check(Checkpoint::DrainagePublish)?;
    Ok(DrainedModel {
        hollow,
        boundaries,
        labels,
        openings,
        plan: record,
    })
}

fn assemble_scene(
    hollow: &HollowedModel,
    cutters: &[TriangleMesh],
    work: &mut Work,
    cancel: &mut Cancellation<'_>,
) -> Result<(TriangleMesh, Vec<u64>), DrainageError> {
    let mut vertices = Vec::new();
    let mut triangles = Vec::new();
    let mut labels = Vec::new();
    let nv = cutters
        .iter()
        .try_fold(hollow.mesh().vertices().len(), |n, m| {
            n.checked_add(m.vertices().len())
        })
        .ok_or_else(|| resource("scene vertex count"))?;
    let nf = cutters
        .iter()
        .try_fold(hollow.mesh().triangles().len(), |n, m| {
            n.checked_add(m.triangles().len())
        })
        .ok_or_else(|| resource("scene triangle count"))?;
    reserve(&mut vertices, nv)?;
    reserve(&mut triangles, nf)?;
    reserve(&mut labels, nf)?;
    for (i, mesh) in std::iter::once(hollow.mesh())
        .chain(cutters.iter())
        .enumerate()
    {
        let offset = u32::try_from(vertices.len()).map_err(|_| resource("scene vertex indices"))?;
        for &v in mesh.vertices() {
            work.tick(cancel)?;
            vertices.push(v);
        }
        for &f in mesh.triangles() {
            work.tick(cancel)?;
            let mut face = [0; 3];
            for k in 0..3 {
                face[k] = f[k]
                    .checked_add(offset)
                    .ok_or_else(|| resource("scene vertex indices"))?;
            }
            triangles.push(face);
            labels.push(i as u64);
        }
    }
    let boundaries = TriangleMesh::try_new_with_cancellation(vertices, triangles, cancel)?;
    Ok((boundaries, labels))
}

fn evaluate_cuts(
    hollow: &HollowedModel,
    plan: &PreparationPlan,
    limits: DrainageLimits,
    work: &mut Work,
    cancel: &mut Cancellation<'_>,
) -> Result<(Vec<TriangleMesh>, Vec<DrainageOpening>), DrainageError> {
    let exterior_count = hollow.mesh().triangles().len() - hollow.cavity().triangles().len();
    let exterior = &hollow.mesh().triangles()[..exterior_count];
    let count = plan.operations().len() - 1;
    let mut cutters = Vec::new();
    reserve(&mut cutters, count)?;
    let mut openings = Vec::new();
    reserve(&mut openings, count)?;
    let mut boxes: Vec<(u64, [[f64; 2]; 3])> = Vec::new();
    reserve(&mut boxes, count)?;
    for operation in &plan.operations()[1..] {
        work.tick(cancel)?;
        let OperationKind::Hole(spec) = operation.kind else {
            unreachable!("recipe checked before geometry");
        };
        let cut = cylinder::cylinder(
            spec,
            plan.sampling().distance_error_mm,
            limits.max_segments,
            cancel,
        )?;
        if !outside(
            hollow.mesh(),
            exterior,
            &cut.entry,
            limits.minimum_clearance_mm,
            work,
            cancel,
        )? {
            return Err(DrainageError::Opening {
                operation_id: operation.id,
                issue: OpeningIssue::EntryNotExterior,
            });
        }
        let terminal = clearance(
            hollow.cavity(),
            hollow.cavity().triangles(),
            &cut.end,
            work,
            cancel,
        )?;
        if terminal <= limits.minimum_clearance_mm {
            let direction = spec.unit_direction().map_err(DrainageError::Plan)?;
            let p = Point3::new(
                spec.entry_mm[0] + direction[0] * spec.length_mm,
                spec.entry_mm[1] + direction[1] * spec.length_mm,
                spec.entry_mm[2] + direction[2] * spec.length_mm,
            );
            let center_cavity = clearance(
                hollow.cavity(),
                hollow.cavity().triangles(),
                &[p],
                work,
                cancel,
            )?;
            let center_source = clearance(hollow.mesh(), exterior, &[p], work, cancel)?;
            let issue = if center_source <= 0. {
                OpeningIssue::MissedModel
            } else if center_cavity <= 0. {
                OpeningIssue::Shallow
            } else {
                OpeningIssue::InsufficientClearance
            };
            return Err(DrainageError::Opening {
                operation_id: operation.id,
                issue,
            });
        }
        let bbox = bounds(cut.mesh.vertices());
        for &(id, other) in &boxes {
            work.tick(cancel)?;
            if !(0..3).any(|i| {
                I::point(bbox[i][1])
                    .add(I::point(limits.minimum_clearance_mm))
                    .hi
                    < other[i][0]
                    || I::point(other[i][1])
                        .add(I::point(limits.minimum_clearance_mm))
                        .hi
                        < bbox[i][0]
            }) {
                return Err(DrainageError::PotentialOverlap {
                    first: id,
                    second: operation.id,
                });
            }
        }
        boxes.push((operation.id, bbox));
        openings.push(DrainageOpening {
            operation_id: operation.id,
            segments: cut.entry.len(),
            radial_error_mm: cut.radial_error_mm,
            axial_error_mm: cut.axial_error_mm,
            terminal_clearance_mm: terminal,
        });
        cutters.push(cut.mesh);
    }
    Ok((cutters, openings))
}
