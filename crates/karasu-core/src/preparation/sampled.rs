//! Sampled-distance inward hollowing of arbitrary closed (including non-convex) source solids.
//!
//! Backend `sampled-distance-tetra-v1` (TD-045). The cavity is the set of source-interior points
//! deeper than the wall thickness `t`: `{p inside S : d(p, S) > t}`, with inside/outside defined by
//! the exact winding number of the balanced source surface (TD-054): inside is a positive winding
//! number. Touching bodies are admitted, and since TD-092 (H2) so are overlapping bodies, hollowed
//! as their union: `d` is the distance to every source face, so every surface keeps its wall and
//! a void never contains a source face, which makes the winding number constant in each void.
//! A negative winding number (a reversed shell outside every body) is refused.
//! It is reconstructed on a uniform world grid of the plan's `cell_mm`:
//!
//! 1. Admission: one hollow operation, a balanced source once its zero-area faces and exact
//!    zero-area cracks are left out (TD-092 H1, [`MeasureZeroDefects`]), placed coordinates within
//!    +/-10000 mm, and `cell * sqrt(3) + 2 * error < t`.
//! 2. Field: exact binary64 point-triangle distances from a BVH, Lipschitz-pruned adaptive blocks
//!    and exact simulation-of-simplicity ray winding (see `field`). Only blocks straddling the
//!    iso-level are sampled; the work is proportional to the cavity area, not the bounding volume.
//! 3. Extraction: marching tetrahedra on the Kuhn subdivision with iso-vertices keyed by grid edge,
//!    giving a closed, edge-manifold, embedded cavity surface by construction (see `tetra`).
//! 4. Certification: the cavity surface is re-validated as a closed manifold, and every cavity face
//!    is proven farther from the source than the certified minimum wall by exact tests (TD-093,
//!    see [`SampledHollowCertificate`]).
//! 5. Assembly: placed exterior plus reversed cavity, each cavity surface once per body its void
//!    lies in (its winding number, [`CavityOverlap`]), so the shell's winding number is zero in
//!    every void and unchanged elsewhere; a void inside one body is the even-odd shell as before.
//!
//! **Model frame (TD-089):** a plan bound to the identity placement hollows the source in its
//! model frame. [`SampledHollowedModel::placed`] then maps that result by an object's placement
//! without hollowing again: moving, turning, scaling or mirroring an object reuses its cavity.
//!
//! Original meshes are never modified, repaired or re-welded; the shell's exterior is the source
//! with its zero-measure defects left out, which changes no point's inside/outside state. Results are identical for every
//! thread count. No dependency or foreign geometry kernel is used.

pub(crate) mod bvh;
pub(crate) mod contact;
pub(crate) mod distance;
mod drain;
mod drill;
mod field;
pub(super) mod fill;
mod holes;
pub(super) mod intersections;
mod rebuild;
pub(super) mod shells;
mod simplify;
mod tetra;

use super::{
    hollow::{HollowError, VolumeBounds},
    repair::{repair_source, RepairError, RepairSpec},
    Operation, OperationKind, PreparationPlan, SimplifySpec, WallGuarantee,
};
use crate::{
    cancellation::{Cancellation, Checkpoint},
    geometry::{Affine3, ObjectTransform, Point3},
    mesh::{topology::TopologyLimits, FaceNormal, TriangleMesh},
};
use bvh::{BuildFailure, Bvh};
use distance::{Triangle, QUERY_RELATIVE_ERROR};
pub use drain::{
    axis_tilt, drain_hollowed_sampled, drain_model_sampled, hole_tangents, map_tilt,
    surface_hole_outward, tilted_axis, CavityRegion, DrainageProbe, HoleFrame, ProbedHole,
    RefusedHole, SampledDrainageLimits, SampledDrainageOpening, SampledDrainedModel,
    NOMINAL_REFUSED_DEPTH_MM, PROBE_OPERATION_ID,
};
pub use drill::{DrillError, DrillInput, DrilledModel};
use field::{FieldError, FieldOutcome, FieldSpec, Grid};
pub use holes::{
    HoleCuts, HoleFindings, HoleProbe, HoleSurface, ProbeLimits, ResolvedHole, MAX_HOLES,
};
pub use rebuild::{
    rebuild_solid, RebuildReport, DEFAULT_REBUILD_MM, MAX_REBUILD_MM, MIN_REBUILD_MM,
};
use std::collections::HashMap;

/// Backend identity recorded with every result.
pub const SAMPLED_BACKEND_ID: &str = "sampled-distance-tetra-v1";

/// Fractional grid offset (2 - golden ratio) applied on every axis. Grid planes then sit at an
/// irrational-looking fraction of the cell from the source minimum, so they practically never
/// coincide with layer sampling planes (`(i + 1/2) h`) or with axis-aligned source faces. A
/// coincidence would not be wrong, only reported by the slicer as ambiguous on-plane geometry.
const GRID_OFFSET_FRACTION: f64 = 0.381_966_011_250_105_1;

/// Maximum admitted absolute placed coordinate, in millimeters.
const MAX_COORDINATE_MM: f64 = 10_000.0;

/// Independent admission limits; none is a peak-RSS guarantee.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SampledHollowLimits {
    /// Maximum source vertices.
    pub max_source_vertices: usize,
    /// Maximum source triangles.
    pub max_source_triangles: usize,
    /// Maximum grid cells in the padded bounding box; also bounded by the plan's `max_cells`.
    pub max_grid_cells: u64,
    /// Maximum exact point-to-surface distance queries during field evaluation.
    pub max_distance_queries: u64,
    /// Maximum emitted cavity triangles.
    pub max_cavity_triangles: u64,
    /// Requested bytes for the BVH, item fragments and merged cavity, excluding allocator overhead.
    pub max_payload_bytes: u64,
    /// Worker threads including the caller's; zero selects the available parallelism.
    pub threads: usize,
}

impl Default for SampledHollowLimits {
    fn default() -> Self {
        Self {
            max_source_vertices: 3_000_000,
            max_source_triangles: 4_000_000,
            max_grid_cells: 1 << 36,
            max_distance_queries: 2_000_000_000,
            max_cavity_triangles: 20_000_000,
            max_payload_bytes: 4 << 30,
            threads: 0,
        }
    }
}

/// Geometric guarantees of one sampled shell, in world millimeters.
///
/// Let `d(p)` be the distance from `p` to the placed source surface and `I` its interior (a
/// positive winding number).
/// With `band = cell * sqrt(3) + 2 * error`:
/// - every point of the cavity lies in `I` with `d >= thickness - band` (a priori), and
/// - every point of `I` with `d > thickness + band` lies in the cavity (a priori).
///
/// `surface_distance_mm` is an a posteriori enclosure of `d` over the whole emitted cavity
/// surface. Its lower end is the certified minimum wall thickness of the represented shell, proven
/// face by face with exact tests (TD-093): it equals the smallest vertex distance within a
/// micrometre on every measured model, where widening vertex distances by each face's size (the
/// certificate before TD-093) lost up to a cell. Its upper end widens the exact vertex distances
/// by each face's size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SampledHollowCertificate {
    /// Requested wall thickness.
    pub thickness_mm: f64,
    /// The level the cavity was extracted at: the requested thickness, or under a wall guarantee
    /// (TD-093) the raised offset of the last pass.
    pub offset_mm: f64,
    /// Extractions made: 1, or more when a wall guarantee needed further passes (TD-093).
    pub passes: u32,
    /// Grid cell side.
    pub cell_mm: f64,
    /// Decision margin for distance queries (the plan's requested error, at least the query budget).
    pub query_error_mm: f64,
    /// A priori band half-width around `thickness_mm`.
    pub band_mm: f64,
    /// A posteriori `[min, max]` of `d` over the cavity surface: the lower end proven per face.
    pub surface_distance_mm: [f64; 2],
    /// Observed `[min, max]` of `d` at cavity vertices (diagnostic, not an enclosure).
    pub vertex_distance_mm: [f64; 2],
    /// Edge-connected closed surfaces of the cavity. A void with an internal island (such as a
    /// preserved inner void) is bounded by more than one surface.
    pub cavity_surface_components: usize,
    /// Signed-volume estimate of the placed source (binary64 divergence sum).
    pub source_volume_mm3: f64,
    /// Signed-volume estimate of the cavity.
    pub cavity_volume_mm3: f64,
    /// The cavity simplification, when the plan requested one (TD-066).
    pub simplification: Option<SimplificationCertificate>,
    /// Smallest and largest absolute scale factor of the placement this shell was mapped by
    /// ([`SampledHollowedModel::placed`]); `[1, 1]` for a shell hollowed where it lies.
    ///
    /// The distance enclosures and volumes describe this shell: under a placement, the model
    /// frame's distances are multiplied by these bounds (a linear map with singular values in
    /// `[s, S]` keeps every point-to-surface distance within `[s d, S d]`), and the volumes are
    /// recomputed from the placed surfaces. The requested thickness, the cell, the query error,
    /// the band and the simplification remain the model frame's.
    pub placement_scale: [f64; 2],
}

/// What a certified cavity simplification did and guarantees (TD-066).
///
/// Every simplified face lies within its certified deviation (at most `max_deviation_mm`, never
/// above `tolerance_mm`) of the original cavity surface and vice versa, and is proven farther
/// than `wall_floor_mm` from the source surface. Untouched faces keep their original bounds.
/// The a priori completeness band widens by the tolerance: every point of the interior with
/// `d > thickness + band + tolerance` still lies in the cavity.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SimplificationCertificate {
    /// Requested tolerance.
    pub tolerance_mm: f64,
    /// Largest certified face deviation actually used.
    pub max_deviation_mm: f64,
    /// Proven minimum distance of every simplified face from the source surface: the thinnest
    /// original wall at the cavity vertices (or the thickness, if smaller), less two query errors.
    pub wall_floor_mm: f64,
    /// Cavity triangles before and after.
    pub triangles: [usize; 2],
    /// Cavity volume estimate before simplification.
    pub cavity_volume_before_mm3: f64,
}

impl SampledHollowCertificate {
    /// Remaining material volume estimate.
    #[must_use]
    pub fn material_volume_mm3(&self) -> f64 {
        self.source_volume_mm3 - self.cavity_volume_mm3
    }

    /// Volume estimates as degenerate intervals, for callers sharing convex-backend reporting.
    #[must_use]
    pub fn cavity_volume(&self) -> VolumeBounds {
        VolumeBounds {
            minimum_mm3: self.cavity_volume_mm3,
            maximum_mm3: self.cavity_volume_mm3,
        }
    }
}

/// Deterministic work statistics of one run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SampledHollowStats {
    /// Grid vertices per axis.
    pub grid_vertices: [u64; 3],
    /// Leaf block side in cells.
    pub leaf_cells: u64,
    /// Work-item block side in cells.
    pub item_cells: u64,
    /// Number of work items.
    pub work_items: u64,
    /// Exact distance queries during field evaluation.
    pub distance_queries: u64,
    /// Exact parity queries.
    pub parity_queries: u64,
    /// Sampled leaf blocks.
    pub leaves: u64,
}

/// Cavity surfaces inside overlapping bodies (TD-092 H2).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CavityOverlap {
    /// Cavity surfaces whose void lies inside two or more bodies.
    pub surfaces: usize,
    /// The most bodies any cavity's void lies in (its winding number); 0 without a cavity.
    pub deepest: u32,
}

/// Complete sampled shell: placed exterior plus reversed cavity, and the outward cavity surface.
#[derive(Debug)]
pub struct SampledHollowedModel {
    /// Leading faces of `mesh` that are the placed source's.
    exterior_faces: usize,
    mesh: TriangleMesh,
    cavity: TriangleMesh,
    measure_zero: MeasureZeroDefects,
    overlap: CavityOverlap,
    certificate: SampledHollowCertificate,
    plan: Vec<u8>,
    stats: SampledHollowStats,
    /// The placement applied after hollowing by [`Self::placed`], if any.
    placed_by: Option<ObjectTransform>,
}

impl SampledHollowedModel {
    /// Sliceable shell: placed source faces in original order (zero-area faces and cracks left
    /// out, see [`Self::measure_zero`]), then the reversed cavity faces, each cavity surface once
    /// per body its void lies in (see [`Self::overlap`]).
    #[must_use]
    pub fn mesh(&self) -> &TriangleMesh {
        &self.mesh
    }

    /// The number of leading faces of [`Self::mesh`] that are the placed source's.
    #[must_use]
    pub fn exterior_faces(&self) -> usize {
        self.exterior_faces
    }

    /// What hollowing left out of the source as defects of zero measure (H1).
    #[must_use]
    pub fn measure_zero(&self) -> MeasureZeroDefects {
        self.measure_zero
    }

    /// The cavity surfaces inside overlapping bodies (H2).
    #[must_use]
    pub fn overlap(&self) -> CavityOverlap {
        self.overlap
    }

    /// Cavity surface oriented outward from the void (towards the material), for previews.
    #[must_use]
    pub fn cavity(&self) -> &TriangleMesh {
        &self.cavity
    }

    /// Geometric guarantees of this result.
    #[must_use]
    pub fn certificate(&self) -> SampledHollowCertificate {
        self.certificate
    }

    /// Exact plan record this result was computed from.
    #[must_use]
    pub fn plan_record(&self) -> &[u8] {
        &self.plan
    }

    /// Algorithm identity, distinct from the plan schema.
    #[must_use]
    pub const fn backend_id(&self) -> &'static str {
        SAMPLED_BACKEND_ID
    }

    /// Work statistics.
    #[must_use]
    pub fn stats(&self) -> SampledHollowStats {
        self.stats
    }

    /// The placement this shell was mapped by after hollowing ([`Self::placed`]), or `None` for
    /// a shell hollowed where it lies.
    #[must_use]
    pub fn placed_by(&self) -> Option<ObjectTransform> {
        self.placed_by
    }

    /// This shell, hollowed in its model frame, mapped by an object's `placement` (TD-089).
    ///
    /// Every vertex is mapped exactly as hollowing maps the source of a placed plan (the
    /// placement's flattened [`Affine3`]), so the exterior is bit-identical to the placed
    /// source. A reflecting placement reverses every face, so the exterior still faces outward
    /// and the cavity still faces the material. Nothing is hollowed again: the cavity is the
    /// model frame's, mapped. The certificate's distances are scaled by the placement's
    /// smallest and largest absolute scale factors and its volumes are recomputed
    /// ([`SampledHollowCertificate::placement_scale`]); the plan record and statistics stay
    /// those of the model frame.
    ///
    /// # Errors
    /// Rejects a shell that is already placed, a placement with a zero scale factor (it would
    /// collapse the shell), arithmetic that leaves the finite range, resource exhaustion and
    /// cancellation.
    pub fn placed(
        &self,
        placement: ObjectTransform,
        cancel: &mut Cancellation<'_>,
    ) -> Result<Self, HollowError> {
        if self.placed_by.is_some() {
            return Err(unsupported("a placed shell cannot be placed again"));
        }
        let scale = placement.scale().components().map(f64::abs);
        if scale.contains(&0.0) {
            return Err(unsupported(
                "the placement collapses the shell (a zero scale factor)",
            ));
        }
        let reflects = placement
            .scale()
            .components()
            .iter()
            .filter(|s| **s < 0.0)
            .count()
            % 2
            == 1;
        // The flattened affine map, as hollowing maps a placed plan's source.
        let affine = placement
            .to_affine()
            .map_err(|_| invalid("placement arithmetic"))?;
        let mesh = place_mesh(&self.mesh, affine, reflects, cancel)?;
        let cavity = place_mesh(&self.cavity, affine, reflects, cancel)?;
        let exterior_faces = self.exterior_faces;
        let low = scale[0].min(scale[1]).min(scale[2]);
        let high = scale[0].max(scale[1]).max(scale[2]);
        let model = self.certificate;
        let [d_lo, d_hi] = model.surface_distance_mm;
        let [v_lo, v_hi] = model.vertex_distance_mm;
        let certificate = SampledHollowCertificate {
            surface_distance_mm: [d_lo * low, d_hi * high],
            vertex_distance_mm: [v_lo * low, v_hi * high],
            source_volume_mm3: signed_volume_of(&mesh, &mesh.triangles()[..exterior_faces]),
            cavity_volume_mm3: signed_volume(&cavity),
            placement_scale: [low, high],
            ..model
        };
        let mut plan = Vec::new();
        plan.try_reserve_exact(self.plan.len())
            .map_err(|_| resource("plan record"))?;
        plan.extend_from_slice(&self.plan);
        Ok(Self {
            exterior_faces,
            mesh,
            cavity,
            measure_zero: self.measure_zero,
            overlap: self.overlap,
            certificate,
            plan,
            stats: self.stats,
            placed_by: Some(placement),
        })
    }
}

/// `mesh` with every vertex mapped by `placement`, faces reversed when it `reflects`.
fn place_mesh(
    mesh: &TriangleMesh,
    placement: Affine3,
    reflects: bool,
    cancel: &mut Cancellation<'_>,
) -> Result<TriangleMesh, HollowError> {
    let mut vertices = Vec::new();
    vertices
        .try_reserve_exact(mesh.vertices().len())
        .map_err(|_| resource("placed shell vertices"))?;
    for (index, &p) in mesh.vertices().iter().enumerate() {
        cancel.poll(Checkpoint::HollowWork, index)?;
        vertices.push(
            placement
                .transform_point(p)
                .map_err(|_| invalid("placement arithmetic"))?,
        );
    }
    let mut triangles = Vec::new();
    triangles
        .try_reserve_exact(mesh.triangles().len())
        .map_err(|_| resource("placed shell triangles"))?;
    triangles.extend(
        mesh.triangles()
            .iter()
            .map(|&[a, b, c]| if reflects { [a, c, b] } else { [a, b, c] }),
    );
    Ok(TriangleMesh::try_new_with_cancellation(
        vertices, triangles, cancel,
    )?)
}

/// Named deterministic phases, separate from cancellation polling.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum SampledHollowStage {
    /// Placement, topology admission and acceleration structure.
    Source,
    /// Adaptive distance-field evaluation; item counts advance.
    Field,
    /// Iso-surface welding and cavity validation.
    Extraction,
    /// A posteriori distance certificate.
    Certification,
    /// Shell assembly.
    Assembly,
    /// Final publication check passed.
    Published,
    /// Drainage cut certification after hollowing (drained models only). Item counts are the
    /// processed cuts out of the requested cuts.
    Drainage,
    /// Certified cavity simplification (TD-066), between extraction and certification.
    Simplification,
    /// A further pass of the wall guarantee (TD-093): `completed_items` is the pass number and
    /// `total_items` the most passes.
    Pass,
}

/// Advisory progress; no partial geometry is exposed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SampledHollowProgress {
    /// Current stage.
    pub stage: SampledHollowStage,
    /// Completed field work items (meaningful during and after `Field`).
    pub completed_items: u64,
    /// Total field work items, zero before the grid is known.
    pub total_items: u64,
}

fn invalid(reason: &'static str) -> HollowError {
    HollowError::InvalidInput {
        reason,
        triangle: None,
    }
}

fn unsupported(reason: &'static str) -> HollowError {
    HollowError::Unsupported { reason }
}

fn resource(resource: &'static str) -> HollowError {
    HollowError::Resource { resource }
}

fn uncertain(reason: &'static str) -> HollowError {
    HollowError::Uncertain { reason }
}

/// Hollows one closed source solid with the sampled backend.
///
/// # Errors
/// See [`hollow_mesh_sampled_with_progress`].
pub fn hollow_mesh_sampled(
    source: &TriangleMesh,
    plan: &PreparationPlan,
    limits: SampledHollowLimits,
    cancel: &mut Cancellation<'_>,
) -> Result<SampledHollowedModel, HollowError> {
    hollow_mesh_sampled_with_progress(source, plan, limits, cancel, &mut |_| {})
}

/// The hollowing error for a failed field evaluation.
fn field_failure(outcome: &FieldOutcome) -> HollowError {
    match outcome {
        FieldOutcome::Cancelled(e) => HollowError::Cancelled(*e),
        FieldOutcome::Failed(FieldError::Resource(name)) => resource(name),
        FieldOutcome::Failed(FieldError::Uncertain(reason)) => uncertain(reason),
        FieldOutcome::Failed(FieldError::Stopped) => uncertain("field evaluation stopped"),
        FieldOutcome::Failed(FieldError::NegativeWinding) => unsupported(
            "the source encloses negative winding (an inside-out body or an unenclosed reversed \
             shell); repair its orientation first",
        ),
    }
}

/// Apply a plan's leading source repair; `None` when it changes nothing.
fn repair_plan_source(
    source: &TriangleMesh,
    spec: RepairSpec,
    limits: SampledHollowLimits,
    cancel: &mut Cancellation<'_>,
) -> Result<Option<TriangleMesh>, HollowError> {
    let topology = TopologyLimits {
        max_vertices: limits.max_source_vertices,
        max_triangles: limits.max_source_triangles,
    };
    repair_source(source, spec, topology, cancel)
        .map(|(mesh, _)| mesh)
        .map_err(|e| match e {
            RepairError::Cancelled(c) => HollowError::Cancelled(c),
            e => HollowError::Repair(Box::new(e)),
        })
}

/// The predicted shortfall's law: next to a reflex edge the cavity is a rounded surface of radius
/// `thickness`, which a tetrahedron of diameter `cell · √3` cuts with a chord whose sagitta is
/// `0.375 · cell² / thickness`; 0.39 is the median measured on 20 models (TD-093).
const SHORTFALL_LAW: f64 = 0.39;

/// Cavity triangles per square cell of cavity surface, measured on `Base_01.stl` (TD-093).
const CAVITY_TRIANGLES_PER_CELL_AREA: f64 = 7.9;

/// The accepted over-thickness of the guarantee's cell rule, in mm (TD-093).
pub const DEFAULT_OVER_THICKNESS_MM: f64 = 0.05;

/// The guarantee's cavity triangle budget: under the drain-hole index's capacity (TD-093).
pub const DEFAULT_CAVITY_TRIANGLE_BUDGET: f64 = 10_000_000.0;

/// The guarantee's predicted shortfall coefficient: above every measured ratio (0.37), with
/// further passes for models that exceed it (TD-093).
pub const DEFAULT_SHORTFALL_COEFFICIENT: f64 = 0.40;

/// Most passes of the wall guarantee.
pub const MAX_GUARANTEE_PASSES: u32 = 6;

/// Passes at one cell before it is halved.
const PASSES_PER_CELL: u32 = 3;

/// A pass that falls short raises the offset by this multiple of its shortfall.
const CORRECTION_MARGIN: f64 = 1.25;

/// The area of a mesh's faces, in mm².
#[must_use]
pub fn surface_area(mesh: &TriangleMesh) -> f64 {
    mesh.triangles()
        .iter()
        .map(|t| {
            let [first, second, third] = t.map(|v| mesh.vertices()[v as usize].coordinates());
            let edge = |to: [f64; 3]| std::array::from_fn::<f64, 3, _>(|k| to[k] - first[k]);
            let (along, across) = (edge(second), edge(third));
            let normal: [f64; 3] = std::array::from_fn(|k| {
                along[(k + 1) % 3] * across[(k + 2) % 3] - along[(k + 2) % 3] * across[(k + 1) % 3]
            });
            0.5 * normal.iter().map(|v| v * v).sum::<f64>().sqrt()
        })
        .sum()
}

/// The cavity triangles predicted for a source of `area_mm2` hollowed on `cell_mm`, by TD-093's
/// measured law (about 7.9 per cell² of source area). It over-predicts sources whose thin parts
/// leave no cavity, so it is a warning, never a refusal (TD-094).
#[must_use]
pub fn predicted_cavity_triangles(area_mm2: f64, cell_mm: f64) -> f64 {
    CAVITY_TRIANGLES_PER_CELL_AREA * area_mm2 / (cell_mm * cell_mm)
}

/// The guarantee's first cell for a source of `area_mm2` (TD-093): the larger of the cell whose
/// predicted shortfall is `over_thickness_mm` and the cell whose cavity fits `budget` triangles,
/// kept below `(thickness − 2 · error) / √3` (by 5 %) so the grid admits it.
#[must_use]
pub fn guaranteed_cell(
    area_mm2: f64,
    thickness_mm: f64,
    error_mm: f64,
    over_thickness_mm: f64,
    budget: f64,
) -> f64 {
    let precise = (over_thickness_mm * thickness_mm / SHORTFALL_LAW).sqrt();
    let bounded = (CAVITY_TRIANGLES_PER_CELL_AREA * area_mm2 / budget).sqrt();
    let admissible = (thickness_mm - 2.0 * error_mm) / 3f64.sqrt() * 0.95;
    precise.max(bounded).min(admissible)
}

/// The smallest and largest corner coordinates of the placed source.
type Bounds = ([f64; 3], [f64; 3]);

/// One extraction at one cell and offset, certified.
struct Pass {
    cavity: TriangleMesh,
    certificate: SampledHollowCertificate,
    stats: SampledHollowStats,
    total_items: u64,
}

/// What every pass of one run shares.
struct PassContext<'a> {
    world: &'a TriangleMesh,
    bvh: &'a Bvh,
    /// The regions left solid, in world space (owner, 2026-11-02).
    blockers: &'a field::Blockers,
    bounds: Bounds,
    /// The requested wall.
    thickness: f64,
    error: f64,
    max_cells: u64,
    simplification: Option<SimplifySpec>,
    limits: SampledHollowLimits,
    threads: usize,
}

/// Hollows one closed source solid, reporting advisory progress on the caller's thread.
///
/// The plan must contain exactly one hollow operation, optionally followed by a simplification.
/// Its sampling `cell_mm` is the grid pitch (the first pass's, under a guarantee),
/// `distance_error_mm` the decision margin (it must cover the binary64 query budget) and
/// `max_cells` an additional cap on padded grid cells.
///
/// Without a [`WallGuarantee`] the cavity is extracted once at the requested thickness and its
/// certified wall reported. With one (TD-093) it is extracted at a raised offset and proven, in
/// further passes when it falls short, until the certified wall reaches the requested thickness
/// within twice the query error; the certificate reports the offset and the passes.
///
/// # Errors
/// Rejects unsupported plans, open/non-manifold/degenerate sources, cells too coarse for the
/// thickness, resource exhaustion, cancellation, an empty cavity, any failed post-validation
/// of the cavity surface, and [`HollowError::WallNotReached`] when the guarantee's passes run
/// out. No partial geometry is published and nothing is silently clamped.
pub fn hollow_mesh_sampled_with_progress(
    source: &TriangleMesh,
    plan: &PreparationPlan,
    limits: SampledHollowLimits,
    cancel: &mut Cancellation<'_>,
    progress: &mut dyn FnMut(SampledHollowProgress),
) -> Result<SampledHollowedModel, HollowError> {
    cancel.check(Checkpoint::HollowStart)?;
    let (repair, operations) = split_repair(plan.operations());
    let (spec, simplification) = match operations {
        [Operation {
            kind: OperationKind::Hollow(spec),
            ..
        }] => (spec.clone(), None),
        [Operation {
            kind: OperationKind::Hollow(spec),
            ..
        }, Operation {
            kind: OperationKind::Simplify(simplify),
            ..
        }] => (spec.clone(), Some(*simplify)),
        [_] | [_, _] => {
            return Err(unsupported(
                "cuts and supports are not implemented by hollowing",
            ))
        }
        _ => return Err(unsupported("exactly one hollow operation is required")),
    };
    let thickness = spec.thickness_mm;
    let sampling = plan.sampling();
    let (cell, error) = (sampling.cell_mm, sampling.distance_error_mm);
    let positive = |v: f64| v.is_finite() && v > 0.0;
    if !(positive(thickness) && positive(cell) && positive(error)) {
        return Err(invalid(
            "thickness, cell and error must be finite and positive",
        ));
    }
    let mut notify = |stage, completed_items, total_items| {
        progress(SampledHollowProgress {
            stage,
            completed_items,
            total_items,
        });
    };
    notify(SampledHollowStage::Source, 0, 0);

    // The plan's source repair (TD-054) runs first, on a derived mesh.
    let repaired = match repair {
        Some(spec) => repair_plan_source(source, spec, limits, cancel)?,
        None => None,
    };
    let source = repaired.as_ref().unwrap_or(source);
    let (world, measure_zero) = admit_source(source, plan, limits, cancel)?;
    let (triangles, bounds) = source_triangles(&world, cancel)?;
    let mut bvh = distance_index(triangles, cancel)?;
    if bvh.retained_bytes() as u64 > limits.max_payload_bytes {
        return Err(resource("requested payload"));
    }
    // The plane index speeds up the exact clearance tests of simplification and of the wall proof.
    bvh.index_planes()
        .map_err(|_| resource("source plane index"))?;
    let placement = plan.placement();
    let blockers = field::Blockers::new(
        spec.blockers.clone(),
        placement.linear(),
        placement.translation().components(),
    )
    .ok_or_else(|| invalid("singular placement for hollow blockers"))?;
    let context = PassContext {
        world: &world,
        bvh: &bvh,
        blockers: &blockers,
        bounds,
        thickness,
        error,
        max_cells: sampling.max_cells,
        simplification,
        limits,
        threads: worker_threads(limits.threads),
    };
    let pass = match spec.guarantee {
        None => hollow_pass(&context, cell, thickness, cancel, &mut notify)?,
        Some(guarantee) => guaranteed_pass(&context, guarantee, cell, cancel, &mut notify)?,
    };
    let total_items = pass.total_items;

    // Assembly: each cavity surface is subtracted once per body its void lies in (H2).
    notify(SampledHollowStage::Assembly, total_items, total_items);
    let multiplicity = cavity_multiplicity(&bvh, &pass.cavity, cancel)?;
    let mesh = assemble(&world, &pass.cavity, &multiplicity, cancel)?;
    let overlap = CavityOverlap {
        surfaces: multiplicity.surfaces.iter().filter(|&&w| w > 1).count(),
        deepest: multiplicity.surfaces.iter().copied().max().unwrap_or(0),
    };
    let mut record = Vec::new();
    record
        .try_reserve_exact(plan.canonical_record().len())
        .map_err(|_| resource("plan record"))?;
    record.extend_from_slice(plan.canonical_record());
    cancel.check(Checkpoint::HollowPublish)?;
    notify(SampledHollowStage::Published, total_items, total_items);
    Ok(SampledHollowedModel {
        exterior_faces: world.triangles().len(),
        mesh,
        cavity: pass.cavity,
        measure_zero,
        overlap,
        certificate: pass.certificate,
        plan: record,
        stats: pass.stats,
        placed_by: None,
    })
}

/// Passes of the wall guarantee (TD-093), from `cell`, until the certified wall reaches the
/// requested thickness within twice the query error.
fn guaranteed_pass(
    context: &PassContext<'_>,
    guarantee: WallGuarantee,
    cell: f64,
    cancel: &mut Cancellation<'_>,
    notify: &mut dyn FnMut(SampledHollowStage, u64, u64),
) -> Result<Pass, HollowError> {
    let requested = context.thickness;
    let raise = |cell: f64| requested + guarantee.shortfall_coefficient * cell * cell / requested;
    let (mut cell, mut offset) = (cell, raise(cell));
    let (mut best, mut at_cell) = (0.0f64, 0);
    for pass in 1..=MAX_GUARANTEE_PASSES {
        if pass > 1 {
            notify(
                SampledHollowStage::Pass,
                u64::from(pass),
                u64::from(MAX_GUARANTEE_PASSES),
            );
        }
        match hollow_pass(context, cell, offset, cancel, notify) {
            Ok(mut result) => {
                let wall = result.certificate.surface_distance_mm[0];
                best = best.max(wall);
                if wall + 2.0 * context.error >= requested {
                    result.certificate.passes = pass;
                    return Ok(result);
                }
                at_cell += 1;
                if at_cell < PASSES_PER_CELL {
                    offset += CORRECTION_MARGIN * (requested - wall);
                    continue;
                }
            }
            // A cavity the proof could not separate from the source: a finer cell may.
            Err(HollowError::Uncertain { .. }) => {}
            Err(e) => return Err(e),
        }
        cell /= 2.0;
        offset = raise(cell);
        at_cell = 0;
    }
    Err(HollowError::WallNotReached {
        requested_mm: requested,
        certified_mm: best,
        passes: MAX_GUARANTEE_PASSES,
    })
}

/// One extraction at `cell` and `offset`: grid, field, welding, validation, the optional
/// simplification and the certificate.
fn hollow_pass(
    context: &PassContext<'_>,
    cell: f64,
    offset: f64,
    cancel: &mut Cancellation<'_>,
    notify: &mut dyn FnMut(SampledHollowStage, u64, u64),
) -> Result<Pass, HollowError> {
    let PassContext {
        world,
        bvh,
        blockers,
        bounds,
        thickness,
        error,
        limits,
        threads,
        ..
    } = *context;
    let (grid, leaf) = grid_for(bounds, offset, cell, error, context.max_cells, limits)?;
    let item = leaf * 8;
    let bvh_bytes = bvh.retained_bytes() as u64;
    let field_spec = FieldSpec {
        thickness: offset,
        margin: error,
        leaf,
        item,
        max_queries: limits.max_distance_queries,
        max_triangles: limits.max_cavity_triangles,
        max_payload_bytes: limits.max_payload_bytes.saturating_sub(bvh_bytes),
        rebuild: false,
    };

    // Field evaluation.
    notify(SampledHollowStage::Field, 0, 0);
    let (outputs, field_stats) = field::evaluate(
        bvh,
        grid,
        field_spec,
        blockers,
        threads,
        cancel,
        &mut |done, total| notify(SampledHollowStage::Field, done, total),
    )
    .map_err(|outcome| field_failure(&outcome))?;
    let total_items = field_stats.items;

    // Extraction: weld item fragments by global grid-edge key, in item order.
    notify(SampledHollowStage::Extraction, total_items, total_items);
    let cavity = weld(outputs, cancel)?;
    let cavity_surface_components = validate_closed_oriented(&cavity, cancel)?;
    let distances = vertex_distances(bvh, &cavity, threads);

    // Optional certified simplification (TD-066), never below the requested wall's floor.
    let (cavity, distances, simplified) = match context.simplification {
        None => (cavity, distances, None),
        Some(request) => {
            notify(SampledHollowStage::Simplification, total_items, total_items);
            simplify_cavity(
                &cavity,
                &distances,
                bvh,
                [thickness, error, request.tolerance_mm],
                cavity_surface_components,
                threads,
                cancel,
            )?
        }
    };

    // Certification.
    notify(SampledHollowStage::Certification, total_items, total_items);
    cancel.check(Checkpoint::HollowCertificate)?;
    let mut certificate = certify(
        world,
        bvh,
        &cavity,
        &distances,
        simplified.as_ref(),
        [thickness, cell, error],
        cavity_surface_components,
        threads,
        cancel,
    )?;
    certificate.offset_mm = offset;
    Ok(Pass {
        cavity,
        certificate,
        stats: SampledHollowStats {
            grid_vertices: grid.vertices,
            leaf_cells: leaf,
            item_cells: item,
            work_items: total_items,
            distance_queries: field_stats.distance_queries,
            parity_queries: field_stats.parity_queries,
            leaves: field_stats.leaves,
        },
        total_items,
    })
}

/// Requested worker count, or the available parallelism for zero.
fn worker_threads(requested: usize) -> usize {
    if requested == 0 {
        std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get)
    } else {
        requested
    }
}

/// Build the distance index over the placed source triangles.
fn distance_index(
    triangles: Vec<Triangle>,
    cancel: &mut Cancellation<'_>,
) -> Result<Bvh, HollowError> {
    Bvh::build(triangles, cancel, Checkpoint::HollowWork).map_err(|failure| match failure {
        BuildFailure::Cancelled(cancelled) => HollowError::Cancelled(cancelled),
        BuildFailure::Allocation => resource("distance index"),
    })
}

/// A plan's optional leading source repair and the operations after it.
pub(super) fn split_repair(operations: &[Operation]) -> (Option<RepairSpec>, &[Operation]) {
    match operations.split_first() {
        Some((first, rest)) => match first.kind {
            OperationKind::Repair(spec) => (Some(spec), rest),
            _ => (None, operations),
        },
        None => (None, operations),
    }
}

/// The defects of zero measure hollowing left out of its source (M6.4d r3, TD-092, H1): they do
/// not change which points are inside, nor any distance to material.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MeasureZeroDefects {
    /// Cracks of zero area closed by splitting the faces along them, as the exact repair does
    /// (TD-091); no vertex moves and no face changes its region.
    pub cracks_closed: usize,
    /// Faces of zero computed area left out (including those inside the closed cracks).
    pub zero_area_faces: usize,
}

impl MeasureZeroDefects {
    /// Whether anything was left out.
    #[must_use]
    pub fn any(&self) -> bool {
        self.cracks_closed + self.zero_area_faces > 0
    }
}

/// Close the exact cracks of `world` and leave out its faces of zero computed area. Faces of
/// zero area enclose no volume and lie in the closure of the other faces' union only where they
/// touch them, so the winding number is unchanged wherever it was defined; and splitting along
/// a crack keeps every face's region (TD-091).
fn leave_out_measure_zero(
    world: TriangleMesh,
    topology: TopologyLimits,
    cancel: &mut Cancellation<'_>,
) -> Result<(TriangleMesh, MeasureZeroDefects), HollowError> {
    let mut left_out = MeasureZeroDefects::default();
    let (zipped, report) =
        fill::fill_holes(&world, None, 0.0, topology, cancel).map_err(|e| match e {
            fill::FillError::Cancelled(c) => HollowError::Cancelled(c),
            fill::FillError::Topology(e) => HollowError::Topology(e),
            fill::FillError::Mesh(e) => HollowError::Mesh(e),
            fill::FillError::Resource(what) => resource(what),
        })?;
    left_out.cracks_closed = report.cracks_closed;
    left_out.zero_area_faces = report.slivers_removed;
    let world = zipped.unwrap_or(world);
    let mut faces = Vec::new();
    faces
        .try_reserve_exact(world.triangles().len())
        .map_err(|_| resource("source face allocation"))?;
    for (index, &face) in world.triangles().iter().enumerate() {
        cancel.poll(Checkpoint::HollowWork, index)?;
        if matches!(world.triangle_normal(index), Some(FaceNormal::Unit(_))) {
            faces.push(face);
        } else {
            left_out.zero_area_faces += 1;
        }
    }
    if faces.len() == world.triangles().len() {
        return Ok((world, left_out));
    }
    let mut vertices = Vec::new();
    vertices
        .try_reserve_exact(world.vertices().len())
        .map_err(|_| resource("source vertex allocation"))?;
    vertices.extend_from_slice(world.vertices());
    Ok((
        TriangleMesh::try_new_with_cancellation(vertices, faces, cancel)?,
        left_out,
    ))
}

/// Place the source in world millimeters and admit only balanced surfaces, once its defects of
/// zero measure are left out (see [`MeasureZeroDefects`]).
fn admit_source(
    source: &TriangleMesh,
    plan: &PreparationPlan,
    limits: SampledHollowLimits,
    cancel: &mut Cancellation<'_>,
) -> Result<(TriangleMesh, MeasureZeroDefects), HollowError> {
    if source.vertices().len() > limits.max_source_vertices
        || source.triangles().len() > limits.max_source_triangles
    {
        return Err(resource("source count"));
    }
    if source.triangles().is_empty() {
        return Err(invalid("empty source"));
    }
    let placement = plan.placement();
    let mut points = Vec::new();
    points
        .try_reserve_exact(source.vertices().len())
        .map_err(|_| resource("placed vertex allocation"))?;
    for (index, &p) in source.vertices().iter().enumerate() {
        cancel.poll(Checkpoint::HollowWork, index)?;
        let p = placement
            .transform_point(p)
            .map_err(|_| invalid("placement arithmetic"))?;
        if p.coordinates().iter().any(|v| v.abs() > MAX_COORDINATE_MM) {
            return Err(unsupported("placed coordinate exceeds +/-10000 mm"));
        }
        points.push(p);
    }
    let mut faces = Vec::new();
    faces
        .try_reserve_exact(source.triangles().len())
        .map_err(|_| resource("placed face allocation"))?;
    faces.extend_from_slice(source.triangles());
    let world = TriangleMesh::try_new_with_cancellation(points, faces, cancel)?;
    // A balanced surface (every edge used equally in both directions) defines a winding number
    // everywhere; touching bodies and non-manifold contacts are admitted, open boundaries and
    // inconsistent orientation are not. Zero-area faces and the cracks they leave are left out
    // first (H1). The field then requires a positive winding number inside the cavity.
    let topology = TopologyLimits {
        max_vertices: limits.max_source_vertices,
        max_triangles: limits.max_source_triangles,
    };
    let mut defects = world.defect_report_with_cancellation(topology, cancel)?;
    let mut world = world;
    let mut left_out = MeasureZeroDefects::default();
    if defects.excluded_triangles > 0 || defects.cracks > 0 {
        (world, left_out) = leave_out_measure_zero(world, topology, cancel)?;
        defects = world.defect_report_with_cancellation(topology, cancel)?;
    }
    if defects.boundary_edges > 0
        || defects.winding_conflicts > 0
        || defects.unbalanced_non_manifold_edges > 0
    {
        return Err(unsupported(
            "source must be a closed, consistently oriented surface (no boundary edges, winding \
             conflicts or unbalanced non-manifold edges); repair or fill it first",
        ));
    }
    Ok((world, left_out))
}

/// The placed source's triangles for the distance index, and the bounds of their corners.
fn source_triangles(
    world: &TriangleMesh,
    cancel: &mut Cancellation<'_>,
) -> Result<(Vec<Triangle>, Bounds), HollowError> {
    let mut min = [f64::INFINITY; 3];
    let mut max = [f64::NEG_INFINITY; 3];
    let mut triangles = Vec::new();
    triangles
        .try_reserve_exact(world.triangles().len())
        .map_err(|_| resource("triangle allocation"))?;
    for (index, face) in world.triangles().iter().enumerate() {
        cancel.poll(Checkpoint::HollowWork, index)?;
        let corners = face.map(|v| world.vertices()[v as usize].coordinates());
        for c in corners {
            for axis in 0..3 {
                min[axis] = min[axis].min(c[axis]);
                max[axis] = max[axis].max(c[axis]);
            }
        }
        triangles.push(Triangle::new(corners));
    }
    Ok((triangles, (min, max)))
}

/// The grid of one pass over the source `bounds`, and its leaf side in cells.
fn grid_for(
    (min, max): Bounds,
    thickness: f64,
    cell: f64,
    error: f64,
    max_cells: u64,
    limits: SampledHollowLimits,
) -> Result<(Grid, u64), HollowError> {
    let magnitude = min
        .iter()
        .chain(max.iter())
        .map(|v| v.abs())
        .fold(0.0, f64::max)
        + 4.0 * cell;
    let budget = QUERY_RELATIVE_ERROR * (1.0 + magnitude);
    if error < budget {
        return Err(unsupported(
            "requested distance error is below the binary64 query error budget",
        ));
    }
    let leaf = [8u8, 4, 2, 1]
        .into_iter()
        .find(|&l| f64::from(l) * cell * 3f64.sqrt() + 2.0 * error < thickness)
        .map(u64::from)
        .ok_or_else(|| invalid("cell too coarse: need cell * sqrt(3) + 2 * error < thickness"))?;

    // Grid: at least two cells of outside padding on every side, offset by a fixed fraction.
    let origin: [f64; 3] = std::array::from_fn(|a| min[a] - (2.0 + GRID_OFFSET_FRACTION) * cell);
    let mut vertices = [0u64; 3];
    for axis in 0..3 {
        let span = ((max[axis] - origin[axis]) / cell).ceil() + 3.0;
        if !(span.is_finite() && span < 9.0e15) {
            return Err(resource("grid extent"));
        }
        // A finite value in [3, 9e15) is a nonnegative integer below 2^53: the cast is exact.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let span = span as u64;
        vertices[axis] = span;
    }
    let cells = (vertices[0] - 1)
        .checked_mul(vertices[1] - 1)
        .and_then(|v| v.checked_mul(vertices[2] - 1))
        .ok_or_else(|| resource("grid cells"))?;
    if cells > limits.max_grid_cells.min(max_cells) {
        return Err(resource("grid cells"));
    }
    let grid = Grid {
        origin,
        cell,
        vertices,
    };
    Ok((grid, leaf))
}

/// Weld item fragments by global grid-edge key, in item order.
fn weld(
    outputs: Vec<field::ItemOutput>,
    cancel: &mut Cancellation<'_>,
) -> Result<TriangleMesh, HollowError> {
    let vertex_estimate: usize = outputs.iter().map(|o| o.vertices.len()).sum();
    let triangle_count: usize = outputs.iter().map(|o| o.triangles.len()).sum();
    if triangle_count == 0 {
        return Err(unsupported(
            "no interior point is deeper than the wall thickness; nothing to hollow",
        ));
    }
    let mut keys: HashMap<u64, u32> = HashMap::new();
    keys.try_reserve(vertex_estimate)
        .map_err(|_| resource("cavity vertex map"))?;
    let mut cavity_vertices = Vec::new();
    cavity_vertices
        .try_reserve(vertex_estimate)
        .map_err(|_| resource("cavity vertices"))?;
    let mut cavity_triangles = Vec::new();
    cavity_triangles
        .try_reserve_exact(triangle_count)
        .map_err(|_| resource("cavity triangles"))?;
    let mut remap = Vec::new();
    for (index, output) in outputs.into_iter().enumerate() {
        cancel.poll(Checkpoint::HollowWork, index)?;
        remap.clear();
        for (key, p) in output.vertices {
            let next = u32::try_from(cavity_vertices.len())
                .map_err(|_| resource("cavity vertex index"))?;
            let id = *keys.entry(key).or_insert_with(|| {
                cavity_vertices.push(Point3::new(p[0], p[1], p[2]));
                next
            });
            remap.push(id);
        }
        for t in output.triangles {
            cavity_triangles.push(t.map(|v| remap[v as usize]));
        }
    }
    drop(keys);
    Ok(TriangleMesh::try_new_with_cancellation(
        cavity_vertices,
        cavity_triangles,
        cancel,
    )?)
}

/// A simplified cavity's per-face deviations and its certificate (TD-066).
struct SimplifiedCavity {
    deviation: Vec<f64>,
    /// Upper end of the original cavity's a posteriori enclosure.
    original_upper: f64,
    certificate: SimplificationCertificate,
}

/// Simplify the cavity within `tolerance` and return it with its remaining vertex distances.
fn simplify_cavity(
    cavity: &TriangleMesh,
    distances: &[f64],
    bvh: &Bvh,
    [thickness, error, tolerance]: [f64; 3],
    components: usize,
    threads: usize,
    cancel: &mut Cancellation<'_>,
) -> Result<(TriangleMesh, Vec<f64>, Option<SimplifiedCavity>), HollowError> {
    if !(tolerance.is_finite() && tolerance > 0.0 && tolerance < thickness) {
        return Err(invalid(
            "simplification tolerance must be positive and below the wall thickness",
        ));
    }
    let vertex_lo = distances.iter().copied().fold(f64::INFINITY, f64::min);
    let floor = vertex_lo.min(thickness) - 2.0 * error;
    if !(floor.is_finite() && floor > tolerance) {
        return Err(uncertain(
            "the cavity is too close to the source for the simplification tolerance",
        ));
    }
    let (_, original_upper) = enclosure(cavity, distances, error, cancel)?;
    let result = simplify::simplify(
        cavity,
        bvh,
        simplify::Request {
            tolerance,
            floor,
            threads,
        },
        cancel,
    )?;
    if validate_closed_oriented(&result.cavity, cancel)? != components {
        return Err(uncertain("cavity simplification changed the surface count"));
    }
    let remaining: Vec<f64> = result
        .original_vertex
        .iter()
        .map(|&v| distances[v as usize])
        .collect();
    let certificate = SimplificationCertificate {
        tolerance_mm: tolerance,
        max_deviation_mm: result.deviation.iter().copied().fold(0.0, f64::max),
        wall_floor_mm: floor,
        triangles: [cavity.triangles().len(), result.cavity.triangles().len()],
        cavity_volume_before_mm3: signed_volume(cavity),
    };
    Ok((
        result.cavity,
        remaining,
        Some(SimplifiedCavity {
            deviation: result.deviation,
            original_upper,
            certificate,
        }),
    ))
}

/// A posteriori `[min, max]` of `d` over a cavity surface: exact vertex distances widened per
/// triangle by its largest vertex-to-point gap, then by the query error.
fn enclosure(
    cavity: &TriangleMesh,
    distances: &[f64],
    error: f64,
    cancel: &mut Cancellation<'_>,
) -> Result<(f64, f64), HollowError> {
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    for (index, face) in cavity.triangles().iter().enumerate() {
        cancel.poll(Checkpoint::HollowWork, index)?;
        let p = face.map(|v| cavity.vertices()[v as usize].coordinates());
        let d = face.map(|v| distances[v as usize]);
        let gap = vertex_gap(p);
        lo = lo.min(d[0].min(d[1]).min(d[2]) - gap);
        hi = hi.max(d[0].max(d[1]).max(d[2]) + gap);
    }
    Ok((lo - error, hi + error))
}

/// Below the cavity's smallest vertex distance by this much, every cavity face is first proven
/// farther from the source by the exact clearance test (TD-093). A performance choice only: a
/// face that fails gets its own certified bound, so the certificate never depends on it. Faces
/// lie below their vertices by under a micrometre on every measured model.
const PROOF_SLACK_MM: f64 = 1e-3;

/// Cavity faces proven between two cancellation checks.
const PROOF_BLOCK: usize = 1 << 20;

/// Cavity faces a worker takes at a time: faces that need a full bound cost far more than the
/// rest, so workers take small batches from a shared counter instead of fixed shares.
const PROOF_BATCH: usize = 256;

/// The certified distance enclosure of the cavity surface and the volume estimates (TD-093).
///
/// **Lower end, exact per face:** each cavity face is bounded below by
/// - its proof when it was simplified (TD-066): the wall floor;
/// - otherwise its smallest vertex distance less its vertex gap, when that already reaches the
///   proof threshold (the smallest vertex distance less [`PROOF_SLACK_MM`]);
/// - otherwise the threshold, when [`Bvh::clearance`] proves the face farther than it from every
///   source face;
/// - otherwise the face's own certified [`Bvh::lower_bound`].
///
/// The minimum over the faces, less the query error, is the certified minimum wall. A face that
/// meets the source bounds it at zero, which refuses the cavity.
///
/// **Upper end:** exact vertex distances widened per face by its vertex gap, as before, or for a
/// simplified face its original enclosure plus its certified deviation.
#[allow(clippy::too_many_arguments)] // The run's inputs, as the hollowing loop holds them.
fn certify(
    world: &TriangleMesh,
    bvh: &Bvh,
    cavity: &TriangleMesh,
    vertex_distances: &[f64],
    simplified: Option<&SimplifiedCavity>,
    [thickness, cell, error]: [f64; 3],
    cavity_surface_components: usize,
    threads: usize,
    cancel: &mut Cancellation<'_>,
) -> Result<SampledHollowCertificate, HollowError> {
    let vertex_lo = vertex_distances
        .iter()
        .copied()
        .fold(f64::INFINITY, f64::min);
    let vertex_hi = vertex_distances
        .iter()
        .copied()
        .fold(f64::NEG_INFINITY, f64::max);
    let threshold = vertex_lo - PROOF_SLACK_MM;
    let faces = cavity.triangles();
    // Per face: (certified lower bound, upper bound).
    let bound = |index: usize| -> (f64, f64) {
        let deviation = simplified.map_or(0.0, |s| s.deviation[index]);
        if let Some(s) = simplified.filter(|_| deviation > 0.0) {
            return (
                s.certificate.wall_floor_mm + error,
                s.original_upper + deviation - error,
            );
        }
        let face = faces[index];
        let p = face.map(|v| cavity.vertices()[v as usize].coordinates());
        let d = face.map(|v| vertex_distances[v as usize]);
        let gap = vertex_gap(p);
        let upper = d[0].max(d[1]).max(d[2]) + gap;
        let widened = d[0].min(d[1]).min(d[2]) - gap;
        if widened >= threshold {
            return (widened, upper);
        }
        let triangle = Triangle::new(p);
        let mut never = Cancellation::never();
        if threshold > 0.0 {
            match bvh.clearance(
                &triangle,
                threshold,
                &mut never,
                Checkpoint::HollowCertificate,
            ) {
                Ok(bvh::Clearance::Beyond) => return (threshold, upper),
                Ok(bvh::Clearance::Meets) => return (0.0, upper),
                _ => {}
            }
        }
        let lower = bvh
            .lower_bound(&triangle, &mut never, Checkpoint::HollowCertificate)
            .unwrap_or(0.0);
        (lower, upper)
    };
    let (mut surface_lo, mut surface_hi) = face_extrema(faces.len(), &bound, threads, cancel)?;
    surface_lo -= error;
    surface_hi += error;
    if surface_lo.is_nan() || surface_lo <= 0.0 {
        return Err(uncertain(
            "cavity surface is not certified to be separated from the source",
        ));
    }
    Ok(SampledHollowCertificate {
        thickness_mm: thickness,
        offset_mm: thickness,
        passes: 1,
        cell_mm: cell,
        query_error_mm: error,
        band_mm: cell * 3f64.sqrt() + 2.0 * error,
        surface_distance_mm: [surface_lo, surface_hi],
        vertex_distance_mm: [vertex_lo, vertex_hi],
        cavity_surface_components,
        source_volume_mm3: signed_volume(world),
        cavity_volume_mm3: signed_volume(cavity),
        simplification: simplified.map(|s| s.certificate),
        placement_scale: [1.0, 1.0],
    })
}

/// The smallest lower and largest upper of `bound` over faces `0..count`, on `threads` workers
/// taking batches from a shared counter, with a cancellation check every [`PROOF_BLOCK`] faces.
/// Minimum and maximum are exact and order-free, so any thread count and any batch order give the
/// same result. A NaN lower bound proves nothing and counts as touching (zero); a failed worker
/// gives NaN, which refuses the certificate.
fn face_extrema(
    count: usize,
    bound: &(dyn Fn(usize) -> (f64, f64) + Sync),
    threads: usize,
    cancel: &mut Cancellation<'_>,
) -> Result<(f64, f64), HollowError> {
    let (mut surface_lo, mut surface_hi) = (f64::INFINITY, f64::NEG_INFINITY);
    for (block, start) in (0..count).step_by(PROOF_BLOCK).enumerate() {
        cancel.poll(Checkpoint::HollowCertificate, block)?;
        let end = (start + PROOF_BLOCK).min(count);
        let next = std::sync::atomic::AtomicUsize::new(start);
        let parts: Vec<(f64, f64)> = std::thread::scope(|scope| {
            let workers: Vec<_> = (0..threads.max(1))
                .map(|_| {
                    let next = &next;
                    scope.spawn(move || {
                        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
                        loop {
                            let from =
                                next.fetch_add(PROOF_BATCH, std::sync::atomic::Ordering::Relaxed);
                            if from >= end {
                                break (lo, hi);
                            }
                            for index in from..(from + PROOF_BATCH).min(end) {
                                let (l, h) = bound(index);
                                lo = lo.min(if l.is_nan() { 0.0 } else { l });
                                hi = hi.max(h);
                            }
                        }
                    })
                })
                .collect();
            workers
                .into_iter()
                .map(|w| w.join().unwrap_or((f64::NAN, f64::NAN)))
                .collect()
        });
        for (lo, hi) in parts {
            surface_lo = if lo.is_nan() {
                f64::NAN
            } else {
                surface_lo.min(lo)
            };
            surface_hi = surface_hi.max(hi);
        }
    }
    Ok((surface_lo, surface_hi))
}

/// Independent check of the extraction guarantee: every undirected edge is used by exactly two
/// triangles with opposite directions (closed, edge-manifold, consistently oriented), and no
/// triangle has a zero computed cross product. Returns the number of edge-connected surfaces.
///
/// The marching-tetrahedra construction implies this; the check guards the implementation with
/// O(T log T) work instead of the general topology report.
fn validate_closed_oriented(
    cavity: &TriangleMesh,
    cancel: &mut Cancellation<'_>,
) -> Result<usize, HollowError> {
    let failed = || uncertain("cavity surface failed closed oriented manifold validation");
    let vertices = cavity.vertices();
    // Edge keys pack two vertex indices below 2^31 into one u64.
    if u32::try_from(vertices.len()).map_or(true, |n| n >= 1 << 31) {
        return Err(resource("cavity vertex index"));
    }
    let mut keys: Vec<u64> = Vec::new();
    keys.try_reserve_exact(cavity.triangles().len() * 3)
        .map_err(|_| resource("cavity edge keys"))?;
    for (index, t) in cavity.triangles().iter().enumerate() {
        cancel.poll(Checkpoint::HollowWork, index)?;
        let [origin, first, second] = t.map(|v| vertices[v as usize].coordinates());
        let e1 = [0, 1, 2].map(|i| first[i] - origin[i]);
        let e2 = [0, 1, 2].map(|i| second[i] - origin[i]);
        let cross = [
            e1[1] * e2[2] - e1[2] * e2[1],
            e1[2] * e2[0] - e1[0] * e2[2],
            e1[0] * e2[1] - e1[1] * e2[0],
        ];
        // An exactly zero computed cross product: the same degeneracy rule as M2 exclusion.
        #[allow(clippy::float_cmp)]
        let degenerate = cross == [0.0; 3];
        if degenerate {
            return Err(uncertain("cavity surface contains a degenerate triangle"));
        }
        for k in 0..3 {
            let (from, to) = (u64::from(t[k]), u64::from(t[(k + 1) % 3]));
            if from == to {
                return Err(failed());
            }
            keys.push(from.min(to) << 32 | to.max(from) << 1 | u64::from(from > to));
        }
    }
    cancel.check(Checkpoint::HollowWork)?;
    crate::parallel::sort_by_key(&mut keys, |&key| key);
    cancel.check(Checkpoint::HollowWork)?;
    if !keys.len().is_multiple_of(2) {
        return Err(failed());
    }
    for (index, pair) in keys.as_chunks::<2>().0.iter().enumerate() {
        cancel.poll(Checkpoint::HollowWork, index)?;
        let same_edge = pair[0] >> 1 == pair[1] >> 1;
        let opposite = pair[0] & 1 == 0 && pair[1] & 1 == 1;
        let unique = keys
            .get(2 * index + 2)
            .is_none_or(|next| next >> 1 != pair[0] >> 1);
        if !(same_edge && opposite && unique) {
            return Err(failed());
        }
    }
    drop(keys);
    let (_, components) = triangle_components(cavity, cancel)?;
    Ok(components)
}

/// Connected-component label of every triangle (vertex-connected; for the closed manifold
/// surfaces produced here this equals edge connectivity), numbered in order of first appearance,
/// and the component count.
pub(super) fn triangle_components(
    mesh: &TriangleMesh,
    cancel: &mut Cancellation<'_>,
) -> Result<(Vec<u32>, usize), HollowError> {
    let count = u32::try_from(mesh.vertices().len())
        .ok()
        .filter(|&n| n < 1 << 31)
        .ok_or_else(|| resource("component vertex index"))?;
    let mut parent: Vec<u32> = Vec::new();
    parent
        .try_reserve_exact(mesh.vertices().len())
        .map_err(|_| resource("components"))?;
    parent.extend(0..count);
    for (index, t) in mesh.triangles().iter().enumerate() {
        cancel.poll(Checkpoint::HollowWork, index)?;
        for k in 1..3 {
            let (a, b) = (root(&mut parent, t[0]), root(&mut parent, t[k]));
            if a != b {
                parent[a.max(b) as usize] = a.min(b);
            }
        }
    }
    let mut label_of_root: Vec<u32> = Vec::new();
    label_of_root
        .try_reserve_exact(mesh.vertices().len())
        .map_err(|_| resource("component roots"))?;
    label_of_root.resize(mesh.vertices().len(), u32::MAX);
    let mut labels = Vec::new();
    labels
        .try_reserve_exact(mesh.triangles().len())
        .map_err(|_| resource("component labels"))?;
    let mut next = 0u32;
    for t in mesh.triangles() {
        let r = root(&mut parent, t[0]) as usize;
        if label_of_root[r] == u32::MAX {
            label_of_root[r] = next;
            next += 1;
        }
        labels.push(label_of_root[r]);
    }
    Ok((labels, next as usize))
}

/// Edge-connected component label of every triangle (triangles sharing an undirected edge are
/// connected; a shared vertex alone is not), numbered in order of first appearance, and the count.
///
/// Shells that touch only at a vertex stay separate, which per-component parity relies on.
pub(super) fn edge_components(
    faces: &[[u32; 3]],
    cancel: &mut Cancellation<'_>,
) -> Result<(Vec<u32>, usize), HollowError> {
    let count = u32::try_from(faces.len()).map_err(|_| resource("component face index"))?;
    let mut keys: Vec<(u64, u32)> = Vec::new();
    keys.try_reserve_exact(faces.len() * 3)
        .map_err(|_| resource("component edge keys"))?;
    for (index, t) in faces.iter().enumerate() {
        cancel.poll(Checkpoint::HollowWork, index)?;
        let face = u32::try_from(index).map_err(|_| resource("component face index"))?;
        for k in 0..3 {
            let (a, b) = (u64::from(t[k]), u64::from(t[(k + 1) % 3]));
            keys.push((a.min(b) << 32 | a.max(b), face));
        }
    }
    cancel.check(Checkpoint::HollowWork)?;
    crate::parallel::sort_by_key(&mut keys, |&key| key);
    cancel.check(Checkpoint::HollowWork)?;
    let mut parent: Vec<u32> = Vec::new();
    parent
        .try_reserve_exact(faces.len())
        .map_err(|_| resource("components"))?;
    parent.extend(0..count);
    for (index, pair) in keys.windows(2).enumerate() {
        cancel.poll(Checkpoint::HollowWork, index)?;
        if pair[0].0 == pair[1].0 {
            let (a, b) = (root(&mut parent, pair[0].1), root(&mut parent, pair[1].1));
            if a != b {
                parent[a.max(b) as usize] = a.min(b);
            }
        }
    }
    let mut label_of_root = Vec::new();
    label_of_root
        .try_reserve_exact(faces.len())
        .map_err(|_| resource("component roots"))?;
    label_of_root.resize(faces.len(), u32::MAX);
    let mut labels = Vec::new();
    labels
        .try_reserve_exact(faces.len())
        .map_err(|_| resource("component labels"))?;
    let mut next = 0u32;
    for face in 0..count {
        let r = root(&mut parent, face) as usize;
        if label_of_root[r] == u32::MAX {
            label_of_root[r] = next;
            next += 1;
        }
        labels.push(label_of_root[r]);
    }
    Ok((labels, next as usize))
}

/// Union-find root with path halving.
fn root(parent: &mut [u32], mut v: u32) -> u32 {
    while parent[v as usize] != v {
        let next = parent[parent[v as usize] as usize];
        parent[v as usize] = next;
        v = next;
    }
    v
}

/// Exact-query distances of every cavity vertex, computed in fixed contiguous chunks.
fn vertex_distances(bvh: &Bvh, cavity: &TriangleMesh, threads: usize) -> Vec<f64> {
    let points = cavity.vertices();
    let mut out = vec![0.0; points.len()];
    let chunk = points.len().div_ceil(threads.max(1)).max(1);
    std::thread::scope(|scope| {
        for (slice, values) in points.chunks(chunk).zip(out.chunks_mut(chunk)) {
            scope.spawn(move || {
                let mut hint = 0;
                for (p, value) in slice.iter().zip(values.iter_mut()) {
                    let (squared, slot) = bvh.nearest(p.coordinates(), hint);
                    hint = slot;
                    *value = squared.sqrt();
                }
            });
        }
    });
    out
}

/// Upper bound on the distance from any point of a triangle to its nearest vertex.
///
/// Every point of a triangle is within its circumradius of some vertex (the farthest point from
/// all vertices is the circumcentre or lies on the boundary), and trivially within its longest
/// edge. The smaller is used, widened by a relative margin for rounding.
#[allow(clippy::many_single_char_names)] // Side lengths a, b, c and edge vectors u, v, w.
fn vertex_gap(p: [[f64; 3]; 3]) -> f64 {
    let sub = |a: [f64; 3], b: [f64; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let norm = |v: [f64; 3]| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    let (u, v, w) = (sub(p[1], p[0]), sub(p[2], p[0]), sub(p[2], p[1]));
    let (a, b, c) = (norm(w), norm(v), norm(u));
    let longest = a.max(b).max(c);
    let cross = [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ];
    let area2 = norm(cross);
    let circumradius = if area2 > 0.0 {
        a * b * c / (2.0 * area2)
    } else {
        f64::INFINITY
    };
    longest.min(circumradius) * (1.0 + 1e-9)
}

fn signed_volume(mesh: &TriangleMesh) -> f64 {
    signed_volume_of(mesh, mesh.triangles())
}

/// The divergence-sum volume of `faces` of `mesh`.
fn signed_volume_of(mesh: &TriangleMesh, faces: &[[u32; 3]]) -> f64 {
    faces
        .iter()
        .map(|t| {
            let [a, b, c] = t.map(|v| mesh.vertices()[v as usize]);
            (a.x * (b.y * c.z - b.z * c.y)
                + a.y * (b.z * c.x - b.x * c.z)
                + a.z * (b.x * c.y - b.y * c.x))
                / 6.0
        })
        .sum()
}

/// The winding number of each cavity surface's void, per surface and per cavity face.
struct Multiplicity {
    surfaces: Vec<u32>,
    faces: Vec<u32>,
}

/// The source's winding number in each cavity surface's void. No source face lies within the
/// wall band of a cavity vertex, so the winding number at any vertex of a surface is the one of
/// the void it bounds; it is positive by construction of the field.
fn cavity_multiplicity(
    bvh: &Bvh,
    cavity: &TriangleMesh,
    cancel: &mut Cancellation<'_>,
) -> Result<Multiplicity, HollowError> {
    let (labels, count) = edge_components(cavity.triangles(), cancel)?;
    let mut surfaces = Vec::new();
    surfaces
        .try_reserve_exact(count)
        .map_err(|_| resource("cavity multiplicity"))?;
    surfaces.resize(count, 0u32);
    for (face, &label) in cavity.triangles().iter().zip(&labels) {
        let slot = &mut surfaces[label as usize];
        if *slot == 0 {
            let point = cavity.vertices()[face[0] as usize].coordinates();
            let winding = bvh
                .vertical_winding(point)
                .ok_or_else(|| uncertain("cavity vertex on the source surface"))?;
            *slot = u32::try_from(winding)
                .ok()
                .filter(|&w| w > 0)
                .ok_or_else(|| uncertain("cavity void outside the source"))?;
        }
    }
    let faces = labels.iter().map(|&l| surfaces[l as usize]).collect();
    Ok(Multiplicity { surfaces, faces })
}

fn assemble(
    world: &TriangleMesh,
    cavity: &TriangleMesh,
    multiplicity: &Multiplicity,
    cancel: &mut Cancellation<'_>,
) -> Result<TriangleMesh, HollowError> {
    let mut vertices = Vec::new();
    vertices
        .try_reserve_exact(world.vertices().len() + cavity.vertices().len())
        .map_err(|_| resource("shell vertices"))?;
    vertices.extend_from_slice(world.vertices());
    vertices.extend_from_slice(cavity.vertices());
    let offset =
        u32::try_from(world.vertices().len()).map_err(|_| resource("shell vertex index"))?;
    if u32::try_from(vertices.len()).is_err() {
        return Err(resource("shell vertex index"));
    }
    let copies: usize = multiplicity.faces.iter().map(|&w| w as usize).sum();
    let mut triangles = Vec::new();
    triangles
        .try_reserve_exact(world.triangles().len() + copies)
        .map_err(|_| resource("shell triangles"))?;
    triangles.extend_from_slice(world.triangles());
    // One copy of every reversed cavity face first, then the extra copies of faces in overlaps,
    // so a cavity inside one body keeps the earlier face order.
    let reversed = |&[a, b, c]: &[u32; 3]| [a + offset, c + offset, b + offset];
    for (index, face) in cavity.triangles().iter().enumerate() {
        cancel.poll(Checkpoint::HollowWork, index)?;
        triangles.push(reversed(face));
    }
    for round in 2..=multiplicity.surfaces.iter().copied().max().unwrap_or(0) {
        for (face, &w) in cavity.triangles().iter().zip(&multiplicity.faces) {
            if w >= round {
                triangles.push(reversed(face));
            }
        }
    }
    Ok(TriangleMesh::try_new_with_cancellation(
        vertices, triangles, cancel,
    )?)
}

#[cfg(test)]
mod tests {
    use super::{distance::disjoint_triangles_closest, *};
    use crate::{
        geometry::{Rotation3, Vector3},
        preparation::{
            HollowSpec, Operation, OperationKind, PlanLimits, SamplingSpec, SourceBinding,
        },
    };

    /// A closed L-shaped prism, 20 mm wide and 10 mm high: its reflex edge rounds the cavity,
    /// where a coarse cell's faces come closest to the source.
    fn l_prism() -> TriangleMesh {
        let outline = [
            [0.0, 0.0],
            [20.0, 0.0],
            [20.0, 8.0],
            [8.0, 8.0],
            [8.0, 20.0],
            [0.0, 20.0],
        ];
        let mut points = Vec::new();
        for z in [0.0, 10.0] {
            points.extend(outline.iter().map(|&[x, y]| Point3::new(x, y, z)));
        }
        let mut faces = Vec::new();
        // The outline is star-shaped from its first corner: fans for the bottom and the top.
        for i in 1..5u32 {
            faces.push([0, i + 1, i]);
            faces.push([6, 6 + i, 7 + i]);
        }
        for i in 0..6u32 {
            let j = (i + 1) % 6;
            faces.push([i, j, 6 + j]);
            faces.push([i, 6 + j, 6 + i]);
        }
        TriangleMesh::try_new(points, faces).unwrap()
    }

    fn hollow(source: &TriangleMesh, thickness: f64, cell: f64) -> SampledHollowedModel {
        let plan = PreparationPlan::new(
            SourceBinding {
                original_sha256: [0; 32],
                placement: ObjectTransform::new(
                    Vector3::new(0.0, 0.0, 0.0),
                    Rotation3::identity(),
                    Vector3::new(1.0, 1.0, 1.0),
                    Point3::new(0.0, 0.0, 0.0),
                )
                .unwrap(),
            },
            SamplingSpec {
                cell_mm: cell,
                distance_error_mm: 1e-6,
                max_cells: u64::MAX,
            },
            vec![Operation {
                id: 1,
                kind: OperationKind::Hollow(HollowSpec {
                    blockers: Vec::new(),
                    thickness_mm: thickness,
                    guarantee: None,
                }),
            }],
            PlanLimits::default(),
            &mut Cancellation::never(),
        )
        .unwrap();
        hollow_mesh_sampled(
            source,
            &plan,
            SampledHollowLimits::default(),
            &mut Cancellation::never(),
        )
        .unwrap()
    }

    /// The smallest distance between any cavity face and any source face, by brute force.
    fn brute_force_wall(model: &SampledHollowedModel) -> f64 {
        let corners = |mesh: &TriangleMesh, t: [u32; 3]| {
            Triangle::new(t.map(|v| mesh.vertices()[v as usize].coordinates()))
        };
        let shell = model.mesh();
        let source: Vec<Triangle> = shell.triangles()[..model.exterior_faces()]
            .iter()
            .map(|&t| corners(shell, t))
            .collect();
        let cavity = model.cavity();
        cavity
            .triangles()
            .iter()
            .map(|&t| {
                let face = corners(cavity, t);
                source
                    .iter()
                    .map(|s| disjoint_triangles_closest(&face, s).0.sqrt())
                    .fold(f64::INFINITY, f64::min)
            })
            .fold(f64::INFINITY, f64::min)
    }

    #[test]
    fn the_certified_wall_is_the_brute_force_minimum() {
        let source = l_prism();
        // A fine and a coarse cell, the coarse one near the `cell * sqrt(3) < wall` limit.
        for cell in [0.5, 1.1] {
            let model = hollow(&source, 2.0, cell);
            let c = model.certificate();
            let brute = brute_force_wall(&model);
            let [lo, _] = c.surface_distance_mm;
            // Sound: never above the true minimum.
            assert!(lo <= brute + 1e-9, "cell {cell}: {lo} > {brute}");
            // Tight: below it by at most the proof slack and the query error.
            assert!(
                lo >= brute - PROOF_SLACK_MM - 2.0 * c.query_error_mm,
                "cell {cell}: {lo} < {brute}"
            );
            // Faces may come closer than their vertices: by 0.044 mm here at the coarse cell, next
            // to the reflex edge. The proof sees it; vertex distances alone would not.
            assert!(lo <= c.vertex_distance_mm[0], "cell {cell}: {c:?}");
        }
    }
}
