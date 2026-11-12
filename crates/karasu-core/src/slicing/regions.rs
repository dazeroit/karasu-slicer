//! Strict exact containment hierarchy over already reviewed section paths (TD-026).
//!
//! This nonconstructive stage accepts one matching planar inspection, exact-cycle-area report and
//! coordinate view. It treats non-contacting `SimpleCycle` paths with agreed represented
//! coordinates or exact grid vertices as boundaries (TD-026, TD-031). Open, ambiguous, non-simple, coordinate-conflicted and contacting paths
//! remain one explicit non-boundary outcome each. Existing source geometry, canonical path order,
//! raw events, ownership and diagnostics remain borrowed and unchanged.
//!
//! ```
//! use karasu_core::{geometry::Point3, mesh::TriangleMesh};
//! use karasu_core::slicing::{section_mesh_z, SectionLimits, SectionTransform};
//! use karasu_core::slicing::area::{measure_cycle_areas, AreaLimits};
//! use karasu_core::slicing::graph::{build_incidence_graph, GraphLimits};
//! use karasu_core::slicing::grid::{map_exact_grid_vertices, BinaryGridSpec, GridLimits};
//! use karasu_core::slicing::normalize::{normalize_section, NormalizationLimits};
//! use karasu_core::slicing::paths::{extract_paths, PathLimits};
//! use karasu_core::slicing::regions::{assemble_strict_regions, RegionCompleteness, RegionLimits};
//! use karasu_core::slicing::validity::{inspect_planar_paths, ValidityLimits};
//! let mesh = TriangleMesh::try_new(vec![Point3::new(0., 0., 0.),
//!     Point3::new(2., 0., 0.), Point3::new(0., 2., 0.)], vec![[0, 1, 2]])?;
//! let raw = section_mesh_z(&mesh, 0., SectionTransform::Identity, SectionLimits::default())?;
//! let normalized = normalize_section(&raw, NormalizationLimits::default())?;
//! let graph = build_incidence_graph(&normalized, GraphLimits::default())?;
//! let paths = extract_paths(&graph, PathLimits::default())?;
//! let inspection = inspect_planar_paths(&paths, ValidityLimits::default())?;
//! let areas = measure_cycle_areas(&inspection, AreaLimits::default())?;
//! let grid = map_exact_grid_vertices(&inspection, BinaryGridSpec::try_new([0., 0.], 0)?,
//!     GridLimits::default())?;
//! let regions = assemble_strict_regions(&inspection, &areas, &grid, RegionLimits::default())?;
//! assert_eq!(regions.completeness(), RegionCompleteness::Complete);
//! assert_eq!(regions.regions().len(), 1);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use super::area::{CycleOrientation, PathAreaOutcome, SectionAreas};
use super::grid::{ExactGridView, GridVertexOutcome};
use super::normalize::CoordinateAgreement;
use super::paths::PathKind;
use super::validity::{
    ContactKind, EmbeddingStatus, PathEdgeRef, PlanarDiagnostic, PlanarInspection,
};
use crate::cancellation::{Cancellation, Cancelled, Checkpoint};
use crate::geometry::Point3;
use std::cmp::Ordering;
use std::{error::Error, fmt, ops::Range, ptr};

const NO_INDEX: usize = usize::MAX;

/// The sole M3.5-A material interpretation, valid only for strict non-contacting cycles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegionFillRule {
    /// Even-depth cycles are material outers and their direct odd-depth children are holes.
    EvenOddStrict,
}

/// Whether all retained section context is resolved enough for this strict hierarchy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegionCompleteness {
    /// Every retained contribution has a reviewed boundary or non-material interpretation.
    Complete,
    /// Retained open, ambiguous, conflicting or contacting context prevents a whole-layer claim.
    GeometricallyUnresolved,
}

/// The role derived solely from `EvenOddStrict` containment depth, not traversal winding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoundaryRole {
    /// An even-depth boundary starts one material region.
    Outer,
    /// An odd-depth boundary is a direct hole of its even-depth parent.
    Hole,
}

/// Why an original path remains explicit but cannot enter the strict region hierarchy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NonBoundaryReason {
    /// A clean graph chain is not automatically closed.
    OpenChain,
    /// A graph trail stopped at a branch or unresolved neighborhood.
    AmbiguousTrail,
    /// A closed candidate has a collapsed edge or forbidden self-contact.
    NonSimpleCycle,
    /// A referenced graph vertex has unresolved coordinates in the selected coordinate mode.
    UnresolvedCoordinates,
    /// A retained cross-path contact involves this otherwise eligible cycle.
    CrossPathContact,
}

/// One deterministic outcome per original path ID.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathRegionOutcome {
    /// Index into [`SectionRegions::boundaries`] in original path-ID order.
    Boundary {
        /// Stable index of this path's strict boundary record.
        boundary: usize,
    },
    /// The original path is retained but not used as a strict boundary.
    NonBoundary(NonBoundaryReason),
}

/// One reviewed cycle in the containment forest, retaining original path identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegionBoundary {
    /// Original canonical path ID.
    pub path: usize,
    /// Original canonical path ID of the immediate containing cycle, if any.
    pub parent: Option<usize>,
    /// Number of strict containing boundaries.
    pub depth: usize,
    /// Existing exact traversal orientation; it does not determine `role`.
    pub orientation: CycleOrientation,
    /// Material role derived from the explicit fill rule and `depth` parity.
    pub role: BoundaryRole,
    /// Whether callers must reverse the borrowed canonical path to view this role conventionally.
    pub reversed_view: bool,
}

/// One material outer and its direct hole-boundary index range in [`SectionRegions`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MaterialRegion {
    outer: usize,
    holes: Range<usize>,
}
impl MaterialRegion {
    /// Index into [`SectionRegions::boundaries`] for this region's even-depth outer boundary.
    #[must_use]
    pub const fn outer_boundary(&self) -> usize {
        self.outer
    }
}

/// Explicit retained ambiguity that is not silently converted into a boundary or construction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegionDiagnostic {
    /// Complete-only unresolved graph groups remain outside strict material interpretation.
    UnresolvedCompleteOnly {
        /// Number of retained unresolved complete groups from the path stage.
        edges: usize,
    },
    /// Graph vertices with incompatible coordinate observations remain unresolved.
    UnresolvedGridVertices {
        /// Number of original graph vertices with unresolved coordinates.
        vertices: usize,
    },
    /// An exact contact copied from the matching planar inspection in its established order.
    CrossPathContact {
        /// Earlier canonical path-edge reference.
        first: PathEdgeRef,
        /// Later canonical path-edge reference.
        second: PathEdgeRef,
        /// Exact retained contact classification.
        kind: ContactKind,
    },
}

/// Independent strict-region input, work, output and requested-capacity limits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegionLimits {
    /// Original path records, including every non-boundary outcome.
    pub max_paths: usize,
    /// Flat vertex references across closed-cycle candidates before strict eligibility filtering.
    pub max_cycle_vertices: usize,
    /// Unordered candidate-boundary pairs classified for containment.
    pub max_pairs: usize,
    /// Exact ray-edge comparisons used by pair classification.
    pub max_ray_tests: usize,
    /// Actual strict-containment relationships, not disjoint pair classifications.
    pub max_relations: usize,
    /// Actual strict boundary records.
    pub max_boundaries: usize,
    /// Actual even-depth material region records.
    pub max_regions: usize,
    /// Actual derived ambiguity diagnostics; borrowed upstream diagnostics are not copied wholesale.
    pub max_diagnostics: usize,
    /// Aggregate requested output and scratch capacities, excluding borrowed input and allocator overhead.
    pub max_payload_bytes: usize,
}
impl Default for RegionLimits {
    fn default() -> Self {
        Self {
            max_paths: 3_000_000,
            max_cycle_vertices: 6_000_000,
            max_pairs: 1_000_000,
            max_ray_tests: 12_000_000,
            max_relations: 1_000_000,
            max_boundaries: 3_000_000,
            max_regions: 3_000_000,
            max_diagnostics: 1_000_000,
            max_payload_bytes: 512 * 1024 * 1024,
        }
    }
}

/// The independently capped resource whose configured maximum was exceeded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegionResource {
    /// Original path records.
    Paths,
    /// Closed-cycle candidate vertex references.
    CycleVertices,
    /// Unordered candidate-boundary pair classifications.
    Pairs,
    /// Exact ray-edge tests performed while classifying pairs.
    RayTests,
    /// Actual strict-containment relationships.
    Relations,
    /// Actual strict boundary records.
    Boundaries,
    /// Actual material region records.
    Regions,
    /// Actual derived diagnostics.
    Diagnostics,
    /// Aggregate requested output/scratch capacities.
    PayloadBytes,
}

/// Defensive inconsistency in privately constructed upstream reports or strict forest derivation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegionInputIssue {
    /// Grid or area report borrows a different planar inspection.
    Provenance,
    /// A matching report does not have one outcome per original path or graph vertex.
    OutcomeCount,
    /// A path's retained slices or closed-cycle shape disagree with its metadata.
    PathShape,
    /// Area, embedding or graph-kind taxonomy disagrees with the shared inspection.
    Eligibility,
    /// A referenced graph vertex has no corresponding grid outcome.
    Vertex,
    /// A retained contact refers outside the matching path-edge ranges.
    Diagnostic,
    /// Pair classification says both non-contacting simple cycles contain each other.
    ContainmentContradiction,
    /// Strict containment relations do not form one laminar immediate-parent forest.
    ContainmentForest,
}

/// Operational failure; successful geometric ambiguity remains result data instead.
#[derive(Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum RegionError {
    /// Cancellation observed at a named TD-014 checkpoint; no successful partial report escapes.
    Cancelled(Cancelled),
    /// A configured independent cap failed.
    LimitExceeded {
        /// Resource that exceeded its caller-provided maximum.
        resource: RegionResource,
        /// Caller-provided maximum.
        limit: usize,
    },
    /// Checked count, byte-size, index or capacity arithmetic failed.
    SizeOverflow,
    /// A fallible exact reservation failed.
    Allocation,
    /// Matching immutable input reports were internally inconsistent.
    InvalidInput {
        /// Original path ID when known.
        path: Option<usize>,
        /// Original graph vertex ID when known.
        vertex: Option<usize>,
        /// Specific inconsistency.
        reason: RegionInputIssue,
    },
}
impl fmt::Display for RegionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled(error) => error.fmt(f),
            Self::LimitExceeded { resource, limit } => {
                write!(f, "strict region {resource:?} limit exceeded ({limit})")
            }
            Self::SizeOverflow => write!(f, "strict region count or capacity arithmetic overflow"),
            Self::Allocation => write!(f, "unable to reserve strict region buffers"),
            Self::InvalidInput {
                path,
                vertex,
                reason,
            } => write!(
                f,
                "inconsistent strict region input at path {path:?}, vertex {vertex:?}: {reason:?}"
            ),
        }
    }
}
impl Error for RegionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Cancelled(error) => Some(error),
            _ => None,
        }
    }
}
impl From<Cancelled> for RegionError {
    fn from(error: Cancelled) -> Self {
        Self::Cancelled(error)
    }
}

/// Deterministic counts and requested-capacity accounting, not a material-area or printability claim.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RegionSummary {
    /// Original path count, equal to `path_outcomes().len()`.
    pub paths: usize,
    /// Closed-cycle candidates examined before strict eligibility filtering.
    pub cycle_candidates: usize,
    /// Closed-cycle candidate vertex references.
    pub cycle_vertices: usize,
    /// Strict boundaries accepted after all prerequisites.
    pub boundaries: usize,
    /// Explicit non-boundary path outcomes.
    pub non_boundaries: usize,
    /// Unordered strict-boundary pairs classified.
    pub pairs: usize,
    /// Exact ray-edge comparisons actually performed.
    pub ray_tests: usize,
    /// Strict-containment relationships retained in the forest.
    pub relations: usize,
    /// Even-depth material outer records.
    pub regions: usize,
    /// Direct odd-depth holes across all material regions.
    pub holes: usize,
    /// Derived ambiguity diagnostics.
    pub diagnostics: usize,
    /// Aggregate requested vector capacities, not actual allocator capacity or process RSS.
    pub requested_payload_bytes: usize,
}

/// Immutable strict hierarchy borrowing its reviewed inspection, grid view and area report.
#[derive(Debug, PartialEq)]
pub struct SectionRegions<'a, 'i, 'p, 'g, 'n, 'r> {
    input: &'i PlanarInspection<'p, 'g, 'n, 'r>,
    areas: &'a SectionAreas<'i, 'p, 'g, 'n, 'r>,
    grid: &'a ExactGridView<'i, 'p, 'g, 'n, 'r>,
    boundaries: Vec<RegionBoundary>,
    regions: Vec<MaterialRegion>,
    holes: Vec<usize>,
    path_outcomes: Vec<PathRegionOutcome>,
    diagnostics: Vec<RegionDiagnostic>,
    completeness: RegionCompleteness,
    summary: RegionSummary,
}
impl<'a, 'i, 'p, 'g, 'n, 'r> SectionRegions<'a, 'i, 'p, 'g, 'n, 'r> {
    /// The immutable matching planar inspection, including all raw and complete-only context.
    #[must_use]
    pub const fn inspection(&self) -> &'i PlanarInspection<'p, 'g, 'n, 'r> {
        self.input
    }
    /// Matching immutable area report that supplied each strict boundary's traversal orientation.
    #[must_use]
    pub const fn areas(&self) -> &'a SectionAreas<'i, 'p, 'g, 'n, 'r> {
        self.areas
    }
    /// Matching coordinate view; represented mode uses original millimeter coordinates.
    #[must_use]
    pub const fn grid(&self) -> &'a ExactGridView<'i, 'p, 'g, 'n, 'r> {
        self.grid
    }
    /// The sole explicit material interpretation used by this result.
    #[must_use]
    pub const fn fill_rule(&self) -> RegionFillRule {
        RegionFillRule::EvenOddStrict
    }
    /// Strict boundaries in original path-ID order.
    #[must_use]
    pub fn boundaries(&self) -> &[RegionBoundary] {
        &self.boundaries
    }
    /// Material regions in ascending outer path-ID order.
    #[must_use]
    pub fn regions(&self) -> &[MaterialRegion] {
        &self.regions
    }
    /// Direct hole-boundary indices for one material region, in ascending path-ID order.
    #[must_use]
    pub fn region_holes(&self, region: usize) -> Option<&[usize]> {
        self.regions
            .get(region)
            .map(|record| &self.holes[record.holes.clone()])
    }
    /// One stable outcome per original path ID, in that path-ID order.
    #[must_use]
    pub fn path_outcomes(&self) -> &[PathRegionOutcome] {
        &self.path_outcomes
    }
    /// Derived unresolved-context diagnostics in their documented stable order.
    #[must_use]
    pub fn diagnostics(&self) -> &[RegionDiagnostic] {
        &self.diagnostics
    }
    /// Whether retained context prevents a claim that the whole layer is geometrically resolved.
    #[must_use]
    pub const fn completeness(&self) -> RegionCompleteness {
        self.completeness
    }
    /// Deterministic accounting, not a material-union area or memory measurement.
    #[must_use]
    pub const fn summary(&self) -> RegionSummary {
        self.summary
    }
}

type Result<T> = std::result::Result<T, RegionError>;

#[derive(Clone)]
struct Cycle {
    path: usize,
    vertices: Range<usize>,
    bounds: [f64; 4],
}

#[derive(Clone, Copy)]
struct Relation {
    parent: usize,
    child: usize,
}

#[derive(Clone, Copy)]
struct Plan {
    paths: usize,
    candidates: usize,
    cycle_vertices: usize,
    boundary_vertices: usize,
    pairs: usize,
    diagnostics: usize,
    requested_payload_bytes: usize,
}

struct ContactContext {
    contacted: Vec<bool>,
    unresolved_complete: usize,
    unresolved_grid: usize,
    diagnostic_count: usize,
}

struct CycleBuffers {
    candidate_for_path: Vec<usize>,
    cycles: Vec<Cycle>,
    coordinates: Vec<[f64; 2]>,
}

struct PairResults {
    relations: Vec<Relation>,
    ray_tests: usize,
}

struct Forest {
    depths: Vec<usize>,
    parents: Vec<Option<usize>>,
}

struct BoundaryOutput {
    outcomes: Vec<PathRegionOutcome>,
    boundaries: Vec<RegionBoundary>,
    geometrically_unresolved: bool,
}

struct BoundaryInputs<'a> {
    plan: Plan,
    cycles: &'a [Cycle],
    candidate_for_path: &'a [usize],
    forest: &'a Forest,
}

struct RegionParts {
    regions: Vec<MaterialRegion>,
    holes: Vec<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Eligibility {
    Candidate,
    NonBoundary(NonBoundaryReason),
}

fn invalid(path: Option<usize>, vertex: Option<usize>, reason: RegionInputIssue) -> RegionError {
    RegionError::InvalidInput {
        path,
        vertex,
        reason,
    }
}
fn add(left: usize, right: usize) -> Result<usize> {
    left.checked_add(right).ok_or(RegionError::SizeOverflow)
}
fn bytes<T>(count: usize) -> Result<usize> {
    let value = count
        .checked_mul(size_of::<T>())
        .ok_or(RegionError::SizeOverflow)?;
    isize::try_from(value).map_err(|_| RegionError::SizeOverflow)?;
    Ok(value)
}
fn cap(count: usize, limit: usize, resource: RegionResource) -> Result<()> {
    if count > limit {
        Err(RegionError::LimitExceeded { resource, limit })
    } else {
        Ok(())
    }
}
fn pairs(count: usize) -> Result<usize> {
    if count < 2 {
        return Ok(0);
    }
    let (left, right) = if count.is_multiple_of(2) {
        (count / 2, count - 1)
    } else {
        (count, (count - 1) / 2)
    };
    left.checked_mul(right).ok_or(RegionError::SizeOverflow)
}
fn reserve<T>(out: &mut Vec<T>, count: usize, cancel: &mut Cancellation<'_>) -> Result<()> {
    if count != 0 {
        cancel.check(Checkpoint::RegionsStorage)?;
        out.try_reserve_exact(count)
            .map_err(|_| RegionError::Allocation)?;
        cancel.check(Checkpoint::RegionsStorage)?;
    }
    Ok(())
}

fn requested_payload(
    paths: usize,
    candidates: usize,
    vertices: usize,
    pairs: usize,
    diagnostics: usize,
    limits: RegionLimits,
) -> Result<usize> {
    let relation_capacity = pairs.min(limits.max_relations);
    let mut total = 0;
    for value in [
        bytes::<PathRegionOutcome>(paths)?,
        bytes::<bool>(paths)?,
        bytes::<usize>(paths)?,
        bytes::<Cycle>(candidates)?,
        bytes::<[f64; 2]>(vertices)?,
        bytes::<Relation>(relation_capacity)?,
        bytes::<usize>(candidates)?,
        bytes::<Option<usize>>(candidates)?,
        bytes::<RegionBoundary>(candidates)?,
        bytes::<MaterialRegion>(candidates)?,
        bytes::<usize>(candidates)?,
        bytes::<usize>(candidates)?,
        bytes::<usize>(candidates)?,
        bytes::<usize>(candidates)?,
        bytes::<RegionDiagnostic>(diagnostics)?,
    ] {
        total = add(total, value)?;
    }
    Ok(total)
}

fn validate_contact(
    input: &PlanarInspection<'_, '_, '_, '_>,
    reference: PathEdgeRef,
) -> Result<()> {
    let edges = input.paths().path_edges(reference.path).ok_or(invalid(
        None,
        None,
        RegionInputIssue::Diagnostic,
    ))?;
    if reference.offset >= edges.len() {
        return Err(invalid(
            Some(reference.path),
            None,
            RegionInputIssue::Diagnostic,
        ));
    }
    Ok(())
}

fn eligibility(
    input: &PlanarInspection<'_, '_, '_, '_>,
    areas: &SectionAreas<'_, '_, '_, '_, '_>,
    grid: &ExactGridView<'_, '_, '_, '_, '_>,
    path_id: usize,
    contacted: bool,
) -> Result<Eligibility> {
    let paths = input.paths();
    let path = paths.paths().get(path_id).ok_or(invalid(
        Some(path_id),
        None,
        RegionInputIssue::PathShape,
    ))?;
    let embedding = *input.embeddings().get(path_id).ok_or(invalid(
        Some(path_id),
        None,
        RegionInputIssue::OutcomeCount,
    ))?;
    let area = *areas.outcomes().get(path_id).ok_or(invalid(
        Some(path_id),
        None,
        RegionInputIssue::OutcomeCount,
    ))?;
    let vertices = paths.path_vertices(path_id).ok_or(invalid(
        Some(path_id),
        None,
        RegionInputIssue::PathShape,
    ))?;
    let edges = paths.path_edges(path_id).ok_or(invalid(
        Some(path_id),
        None,
        RegionInputIssue::PathShape,
    ))?;

    match path.kind {
        PathKind::OpenChain => {
            if path.closed
                || embedding.status != EmbeddingStatus::Ineligible
                || !matches!(area, PathAreaOutcome::Skipped(_))
            {
                return Err(invalid(Some(path_id), None, RegionInputIssue::Eligibility));
            }
            return Ok(Eligibility::NonBoundary(NonBoundaryReason::OpenChain));
        }
        PathKind::AmbiguousTrail => {
            if embedding.status != EmbeddingStatus::Ineligible
                || !matches!(area, PathAreaOutcome::Skipped(_))
            {
                return Err(invalid(Some(path_id), None, RegionInputIssue::Eligibility));
            }
            return Ok(Eligibility::NonBoundary(NonBoundaryReason::AmbiguousTrail));
        }
        PathKind::ClosedCycleCandidate => {}
    }

    if !path.closed || vertices.len() < 3 || vertices.len() != edges.len() {
        return Err(invalid(Some(path_id), None, RegionInputIssue::PathShape));
    }
    match embedding.status {
        EmbeddingStatus::Ineligible => {
            Err(invalid(Some(path_id), None, RegionInputIssue::Eligibility))
        }
        EmbeddingStatus::NonSimpleCycle => {
            if !matches!(area, PathAreaOutcome::Skipped(_)) {
                return Err(invalid(Some(path_id), None, RegionInputIssue::Eligibility));
            }
            Ok(Eligibility::NonBoundary(NonBoundaryReason::NonSimpleCycle))
        }
        EmbeddingStatus::SimpleCycle => {
            if !matches!(area, PathAreaOutcome::Measured { .. }) {
                return Err(invalid(Some(path_id), None, RegionInputIssue::Eligibility));
            }
            for &vertex in vertices {
                match grid.vertex(vertex) {
                    Some(GridVertexOutcome::Exact { .. } | GridVertexOutcome::Represented) => {}
                    Some(GridVertexOutcome::UnresolvedCoordinates) => {
                        return Ok(Eligibility::NonBoundary(
                            NonBoundaryReason::UnresolvedCoordinates,
                        ));
                    }
                    None => {
                        return Err(invalid(
                            Some(path_id),
                            Some(vertex),
                            RegionInputIssue::Vertex,
                        ))
                    }
                }
            }
            if contacted {
                Ok(Eligibility::NonBoundary(
                    NonBoundaryReason::CrossPathContact,
                ))
            } else {
                Ok(Eligibility::Candidate)
            }
        }
    }
}

fn preflight(
    input: &PlanarInspection<'_, '_, '_, '_>,
    areas: &SectionAreas<'_, '_, '_, '_, '_>,
    grid: &ExactGridView<'_, '_, '_, '_, '_>,
    limits: RegionLimits,
    contacted: &[bool],
    diagnostics: usize,
    cancel: &mut Cancellation<'_>,
) -> Result<Plan> {
    let paths = input.paths().paths();
    cap(paths.len(), limits.max_paths, RegionResource::Paths)?;
    if input.embeddings().len() != paths.len()
        || areas.outcomes().len() != paths.len()
        || contacted.len() != paths.len()
        || grid.outcomes().len() != input.paths().graph().vertices().len()
    {
        return Err(invalid(None, None, RegionInputIssue::OutcomeCount));
    }
    let mut candidates = 0;
    let mut cycle_vertices = 0;
    let mut boundary_vertices = 0;
    for path_id in 0..paths.len() {
        cancel.poll(Checkpoint::RegionsInput, path_id)?;
        if paths[path_id].kind == PathKind::ClosedCycleCandidate {
            cycle_vertices = add(
                cycle_vertices,
                input
                    .paths()
                    .path_vertices(path_id)
                    .ok_or(invalid(Some(path_id), None, RegionInputIssue::PathShape))?
                    .len(),
            )?;
        }
        if eligibility(input, areas, grid, path_id, contacted[path_id])? == Eligibility::Candidate {
            candidates = add(candidates, 1)?;
            boundary_vertices = add(
                boundary_vertices,
                input
                    .paths()
                    .path_vertices(path_id)
                    .ok_or(invalid(Some(path_id), None, RegionInputIssue::PathShape))?
                    .len(),
            )?;
        }
    }
    cap(
        cycle_vertices,
        limits.max_cycle_vertices,
        RegionResource::CycleVertices,
    )?;
    cap(
        candidates,
        limits.max_boundaries,
        RegionResource::Boundaries,
    )?;
    cap(
        diagnostics,
        limits.max_diagnostics,
        RegionResource::Diagnostics,
    )?;
    let pair_count = pairs(candidates)?;
    cap(pair_count, limits.max_pairs, RegionResource::Pairs)?;
    let payload = requested_payload(
        paths.len(),
        candidates,
        boundary_vertices,
        pair_count,
        diagnostics,
        limits,
    )?;
    cap(
        payload,
        limits.max_payload_bytes,
        RegionResource::PayloadBytes,
    )?;
    Ok(Plan {
        paths: paths.len(),
        candidates,
        cycle_vertices,
        boundary_vertices,
        pairs: pair_count,
        diagnostics,
        requested_payload_bytes: payload,
    })
}

#[allow(clippy::cast_precision_loss)] // TD-025 integer coordinates are exactly representable binary64.
fn integer_xy(xy: [i64; 2]) -> [f64; 2] {
    [xy[0] as f64, xy[1] as f64]
}

fn bounds(points: &[[f64; 2]]) -> [f64; 4] {
    let mut result = [points[0][0], points[0][1], points[0][0], points[0][1]];
    for &[x, y] in &points[1..] {
        result[0] = result[0].min(x);
        result[1] = result[1].min(y);
        result[2] = result[2].max(x);
        result[3] = result[3].max(y);
    }
    result
}
fn may_contain(outer: [f64; 4], inner: [f64; 4]) -> bool {
    outer[0] <= inner[0] && outer[1] <= inner[1] && outer[2] >= inner[2] && outer[3] >= inner[3]
}
fn contains(
    polygon: &[[f64; 2]],
    point: [f64; 2],
    ray_tests: &mut usize,
    limits: RegionLimits,
    cancel: &mut Cancellation<'_>,
) -> Result<bool> {
    let mut inside = false;
    for (index, &a) in polygon.iter().enumerate() {
        cancel.poll(Checkpoint::RegionsPairs, *ray_tests)?;
        if *ray_tests >= limits.max_ray_tests {
            return Err(RegionError::LimitExceeded {
                resource: RegionResource::RayTests,
                limit: limits.max_ray_tests,
            });
        }
        *ray_tests = add(*ray_tests, 1)?;
        let b = polygon[(index + 1) % polygon.len()];
        let upward = b[1] > a[1];
        let downward = b[1] < a[1];
        // Both endpoints bound the half-open interval. Direction alone also admits edges
        // entirely below the ray, which can double-toggle subdivided polygon sides.
        if !((a[1] <= point[1] && point[1] < b[1]) || (b[1] <= point[1] && point[1] < a[1])) {
            continue;
        }
        let side = super::exact::orientation(
            Point3::new(a[0], a[1], 0.0),
            Point3::new(b[0], b[1], 0.0),
            Point3::new(point[0], point[1], 0.0),
        );
        if (upward && side == Ordering::Greater) || (downward && side == Ordering::Less) {
            inside = !inside;
        }
    }
    Ok(inside)
}

fn validate_reports(
    input: &PlanarInspection<'_, '_, '_, '_>,
    areas: &SectionAreas<'_, '_, '_, '_, '_>,
    grid: &ExactGridView<'_, '_, '_, '_, '_>,
    limits: RegionLimits,
) -> Result<()> {
    if !ptr::eq(input, areas.inspection()) || !ptr::eq(input, grid.inspection()) {
        return Err(invalid(None, None, RegionInputIssue::Provenance));
    }
    let path_count = input.paths().paths().len();
    cap(path_count, limits.max_paths, RegionResource::Paths)?;
    if input.embeddings().len() != path_count
        || areas.outcomes().len() != path_count
        || grid.outcomes().len() != input.paths().graph().vertices().len()
    {
        return Err(invalid(None, None, RegionInputIssue::OutcomeCount));
    }
    Ok(())
}

fn contact_context(
    input: &PlanarInspection<'_, '_, '_, '_>,
    grid: &ExactGridView<'_, '_, '_, '_, '_>,
    cancel: &mut Cancellation<'_>,
) -> Result<ContactContext> {
    let mut contacted = Vec::new();
    reserve(&mut contacted, input.paths().paths().len(), cancel)?;
    contacted.resize(input.paths().paths().len(), false);
    let mut cross_diagnostics = 0;
    for (index, diagnostic) in input.diagnostics().iter().enumerate() {
        cancel.poll(Checkpoint::RegionsInput, index)?;
        let PlanarDiagnostic::Contact { first, second, .. } = diagnostic else {
            continue;
        };
        validate_contact(input, *first)?;
        validate_contact(input, *second)?;
        if first.path != second.path {
            let Some(first_path) = contacted.get_mut(first.path) else {
                return Err(invalid(None, None, RegionInputIssue::Diagnostic));
            };
            *first_path = true;
            let Some(second_path) = contacted.get_mut(second.path) else {
                return Err(invalid(None, None, RegionInputIssue::Diagnostic));
            };
            *second_path = true;
            cross_diagnostics = add(cross_diagnostics, 1)?;
        }
    }
    let unresolved_complete = input.paths().summary().unresolved_edges;
    let unresolved_grid = grid.summary().unresolved_vertices;
    let diagnostic_count = add(
        cross_diagnostics,
        usize::from(unresolved_complete != 0) + usize::from(unresolved_grid != 0),
    )?;
    Ok(ContactContext {
        contacted,
        unresolved_complete,
        unresolved_grid,
        diagnostic_count,
    })
}

fn build_cycles(
    input: &PlanarInspection<'_, '_, '_, '_>,
    areas: &SectionAreas<'_, '_, '_, '_, '_>,
    grid: &ExactGridView<'_, '_, '_, '_, '_>,
    plan: Plan,
    contacted: &[bool],
    cancel: &mut Cancellation<'_>,
) -> Result<CycleBuffers> {
    let mut candidate_for_path = Vec::new();
    let mut cycles = Vec::new();
    let mut coordinates = Vec::new();
    reserve(&mut candidate_for_path, plan.paths, cancel)?;
    reserve(&mut cycles, plan.candidates, cancel)?;
    reserve(&mut coordinates, plan.boundary_vertices, cancel)?;
    candidate_for_path.resize(plan.paths, NO_INDEX);
    for path_id in 0..plan.paths {
        cancel.poll(Checkpoint::RegionsBounds, path_id)?;
        if eligibility(input, areas, grid, path_id, contacted[path_id])? != Eligibility::Candidate {
            continue;
        }
        let vertices = input.paths().path_vertices(path_id).ok_or(invalid(
            Some(path_id),
            None,
            RegionInputIssue::PathShape,
        ))?;
        let start = coordinates.len();
        for &vertex in vertices {
            let xy = match grid.vertex(vertex) {
                Some(GridVertexOutcome::Exact { xy }) => integer_xy(xy),
                Some(GridVertexOutcome::Represented) => {
                    let feature = &input.paths().graph().normalized().features()[vertex];
                    let CoordinateAgreement::Agreed(point) = feature.coordinates else {
                        return Err(invalid(
                            Some(path_id),
                            Some(vertex),
                            RegionInputIssue::Vertex,
                        ));
                    };
                    [point.x, point.y]
                }
                _ => {
                    return Err(invalid(
                        Some(path_id),
                        Some(vertex),
                        RegionInputIssue::Vertex,
                    ))
                }
            };
            coordinates.push(xy);
        }
        let end = coordinates.len();
        let cycle_id = cycles.len();
        candidate_for_path[path_id] = cycle_id;
        cycles.push(Cycle {
            path: path_id,
            vertices: start..end,
            bounds: bounds(&coordinates[start..end]),
        });
    }
    if cycles.len() != plan.candidates || coordinates.len() != plan.boundary_vertices {
        return Err(invalid(None, None, RegionInputIssue::Eligibility));
    }
    Ok(CycleBuffers {
        candidate_for_path,
        cycles,
        coordinates,
    })
}

fn classify_pairs(
    cycles: &[Cycle],
    coordinates: &[[f64; 2]],
    plan: Plan,
    limits: RegionLimits,
    cancel: &mut Cancellation<'_>,
) -> Result<PairResults> {
    let mut relations = Vec::new();
    reserve(&mut relations, plan.pairs.min(limits.max_relations), cancel)?;
    let mut ray_tests = 0;
    let mut pair_count = 0;
    for left in 0..cycles.len() {
        for right in left + 1..cycles.len() {
            cancel.poll(Checkpoint::RegionsPairs, pair_count)?;
            pair_count = add(pair_count, 1)?;
            let first = &cycles[left];
            let second = &cycles[right];
            let first_contains_second = may_contain(first.bounds, second.bounds)
                && contains(
                    &coordinates[first.vertices.clone()],
                    coordinates[second.vertices.start],
                    &mut ray_tests,
                    limits,
                    cancel,
                )?;
            let second_contains_first = may_contain(second.bounds, first.bounds)
                && contains(
                    &coordinates[second.vertices.clone()],
                    coordinates[first.vertices.start],
                    &mut ray_tests,
                    limits,
                    cancel,
                )?;
            match (first_contains_second, second_contains_first) {
                (false, false) => {}
                (true, false) => relations.push(Relation {
                    parent: left,
                    child: right,
                }),
                (false, true) => relations.push(Relation {
                    parent: right,
                    child: left,
                }),
                (true, true) => {
                    return Err(invalid(
                        Some(first.path),
                        None,
                        RegionInputIssue::ContainmentContradiction,
                    ));
                }
            }
            if relations.len() > limits.max_relations {
                return Err(RegionError::LimitExceeded {
                    resource: RegionResource::Relations,
                    limit: limits.max_relations,
                });
            }
        }
    }
    if pair_count != plan.pairs {
        return Err(RegionError::SizeOverflow);
    }
    Ok(PairResults {
        relations,
        ray_tests,
    })
}

fn build_forest(
    cycles: &[Cycle],
    relations: &[Relation],
    cancel: &mut Cancellation<'_>,
) -> Result<Forest> {
    let mut depths = Vec::new();
    let mut parents = Vec::new();
    reserve(&mut depths, cycles.len(), cancel)?;
    reserve(&mut parents, cycles.len(), cancel)?;
    depths.resize(cycles.len(), 0);
    parents.resize(cycles.len(), None);
    for (index, relation) in relations.iter().enumerate() {
        cancel.poll(Checkpoint::RegionsForest, index)?;
        depths[relation.child] = add(depths[relation.child], 1)?;
    }
    for (index, relation) in relations.iter().enumerate() {
        cancel.poll(Checkpoint::RegionsForest, index)?;
        let current = parents[relation.child];
        if current.is_none_or(|parent| depths[relation.parent] > depths[parent]) {
            parents[relation.child] = Some(relation.parent);
        } else if let Some(parent) = current {
            if depths[relation.parent] == depths[parent] && parent != relation.parent {
                return Err(invalid(
                    Some(cycles[relation.child].path),
                    None,
                    RegionInputIssue::ContainmentForest,
                ));
            }
        }
    }
    for cycle in 0..cycles.len() {
        cancel.poll(Checkpoint::RegionsForest, relations.len() + cycle)?;
        let valid = match parents[cycle] {
            Some(parent) => depths[parent].checked_add(1) == Some(depths[cycle]),
            None => depths[cycle] == 0,
        };
        if !valid {
            return Err(invalid(
                Some(cycles[cycle].path),
                None,
                RegionInputIssue::ContainmentForest,
            ));
        }
    }
    Ok(Forest { depths, parents })
}

fn copy_diagnostics(
    input: &PlanarInspection<'_, '_, '_, '_>,
    context: &ContactContext,
    plan: Plan,
    cancel: &mut Cancellation<'_>,
) -> Result<Vec<RegionDiagnostic>> {
    let mut diagnostics = Vec::new();
    reserve(&mut diagnostics, plan.diagnostics, cancel)?;
    if context.unresolved_complete != 0 {
        diagnostics.push(RegionDiagnostic::UnresolvedCompleteOnly {
            edges: context.unresolved_complete,
        });
    }
    if context.unresolved_grid != 0 {
        diagnostics.push(RegionDiagnostic::UnresolvedGridVertices {
            vertices: context.unresolved_grid,
        });
    }
    for (index, diagnostic) in input.diagnostics().iter().enumerate() {
        cancel.poll(Checkpoint::RegionsOutput, index)?;
        if let PlanarDiagnostic::Contact {
            first,
            second,
            kind,
        } = diagnostic
        {
            if first.path != second.path {
                diagnostics.push(RegionDiagnostic::CrossPathContact {
                    first: *first,
                    second: *second,
                    kind: *kind,
                });
            }
        }
    }
    if diagnostics.len() != plan.diagnostics {
        return Err(invalid(None, None, RegionInputIssue::Diagnostic));
    }
    Ok(diagnostics)
}

fn assemble_boundaries(
    input: &PlanarInspection<'_, '_, '_, '_>,
    areas: &SectionAreas<'_, '_, '_, '_, '_>,
    grid: &ExactGridView<'_, '_, '_, '_, '_>,
    context: &ContactContext,
    inputs: &BoundaryInputs<'_>,
    cancel: &mut Cancellation<'_>,
) -> Result<BoundaryOutput> {
    let plan = inputs.plan;
    let mut outcomes = Vec::new();
    let mut boundaries = Vec::new();
    reserve(&mut outcomes, plan.paths, cancel)?;
    reserve(&mut boundaries, plan.candidates, cancel)?;
    let mut geometrically_unresolved =
        context.unresolved_complete != 0 || context.unresolved_grid != 0;
    for (path_id, &cycle) in inputs.candidate_for_path.iter().enumerate() {
        cancel.poll(Checkpoint::RegionsOutput, path_id)?;
        if cycle == NO_INDEX {
            let Eligibility::NonBoundary(reason) =
                eligibility(input, areas, grid, path_id, context.contacted[path_id])?
            else {
                return Err(invalid(Some(path_id), None, RegionInputIssue::Eligibility));
            };
            geometrically_unresolved = true;
            outcomes.push(PathRegionOutcome::NonBoundary(reason));
            continue;
        }
        let Some(PathAreaOutcome::Measured { orientation, .. }) = areas.outcomes().get(path_id)
        else {
            return Err(invalid(Some(path_id), None, RegionInputIssue::Eligibility));
        };
        let orientation = *orientation;
        let depth = *inputs.forest.depths.get(cycle).ok_or(invalid(
            Some(path_id),
            None,
            RegionInputIssue::ContainmentForest,
        ))?;
        let role = if depth.is_multiple_of(2) {
            BoundaryRole::Outer
        } else {
            BoundaryRole::Hole
        };
        let reversed_view = matches!(
            (role, orientation),
            (BoundaryRole::Outer, CycleOrientation::Clockwise)
                | (BoundaryRole::Hole, CycleOrientation::Counterclockwise)
        );
        let boundary = boundaries.len();
        boundaries.push(RegionBoundary {
            path: path_id,
            parent: inputs.forest.parents[cycle].map(|parent| inputs.cycles[parent].path),
            depth,
            orientation,
            role,
            reversed_view,
        });
        outcomes.push(PathRegionOutcome::Boundary { boundary });
    }
    if boundaries.len() != plan.candidates || outcomes.len() != plan.paths {
        return Err(invalid(None, None, RegionInputIssue::Eligibility));
    }
    Ok(BoundaryOutput {
        outcomes,
        boundaries,
        geometrically_unresolved,
    })
}

fn parent_boundary(boundary: &RegionBoundary, candidate_for_path: &[usize]) -> Result<usize> {
    boundary
        .parent
        .and_then(|path| candidate_for_path.get(path).copied())
        .filter(|&id| id != NO_INDEX)
        .ok_or(invalid(
            Some(boundary.path),
            None,
            RegionInputIssue::ContainmentForest,
        ))
}

fn count_holes(
    boundaries: &[RegionBoundary],
    candidate_for_path: &[usize],
    cancel: &mut Cancellation<'_>,
) -> Result<(Vec<usize>, usize)> {
    let mut counts = Vec::new();
    reserve(&mut counts, boundaries.len(), cancel)?;
    counts.resize(boundaries.len(), 0);
    let mut holes = 0;
    for (index, boundary) in boundaries.iter().enumerate() {
        cancel.poll(Checkpoint::RegionsOutput, index)?;
        if boundary.role == BoundaryRole::Hole {
            let parent = parent_boundary(boundary, candidate_for_path)?;
            if boundaries[parent].role != BoundaryRole::Outer {
                return Err(invalid(
                    Some(boundary.path),
                    None,
                    RegionInputIssue::ContainmentForest,
                ));
            }
            counts[parent] = add(counts[parent], 1)?;
            holes = add(holes, 1)?;
        }
    }
    Ok((counts, holes))
}

fn assemble_material_regions(
    boundaries: &[RegionBoundary],
    candidate_for_path: &[usize],
    hole_counts: &[usize],
    holes: usize,
    limits: RegionLimits,
    cancel: &mut Cancellation<'_>,
) -> Result<RegionParts> {
    let region_count = boundaries
        .iter()
        .filter(|record| record.role == BoundaryRole::Outer)
        .count();
    cap(region_count, limits.max_regions, RegionResource::Regions)?;
    let mut regions = Vec::new();
    let mut region_for_boundary = Vec::new();
    let mut hole_positions = Vec::new();
    let mut region_holes = Vec::new();
    reserve(&mut regions, region_count, cancel)?;
    reserve(&mut region_for_boundary, boundaries.len(), cancel)?;
    reserve(&mut hole_positions, boundaries.len(), cancel)?;
    reserve(&mut region_holes, holes, cancel)?;
    region_for_boundary.resize(boundaries.len(), NO_INDEX);
    hole_positions.resize(boundaries.len(), 0);
    region_holes.resize(holes, 0);
    let mut next_hole = 0;
    for (index, boundary) in boundaries.iter().enumerate() {
        cancel.poll(Checkpoint::RegionsOutput, index)?;
        if boundary.role == BoundaryRole::Outer {
            let end = add(next_hole, hole_counts[index])?;
            region_for_boundary[index] = regions.len();
            hole_positions[index] = next_hole;
            regions.push(MaterialRegion {
                outer: index,
                holes: next_hole..end,
            });
            next_hole = end;
        }
    }
    if next_hole != holes {
        return Err(RegionError::SizeOverflow);
    }
    fill_holes(
        boundaries,
        candidate_for_path,
        &region_for_boundary,
        &mut hole_positions,
        &regions,
        &mut region_holes,
        cancel,
    )?;
    Ok(RegionParts {
        regions,
        holes: region_holes,
    })
}

fn fill_holes(
    boundaries: &[RegionBoundary],
    candidate_for_path: &[usize],
    region_for_boundary: &[usize],
    hole_positions: &mut [usize],
    regions: &[MaterialRegion],
    region_holes: &mut [usize],
    cancel: &mut Cancellation<'_>,
) -> Result<()> {
    for (index, boundary) in boundaries.iter().enumerate() {
        cancel.poll(Checkpoint::RegionsOutput, index)?;
        if boundary.role != BoundaryRole::Hole {
            continue;
        }
        let parent = parent_boundary(boundary, candidate_for_path)?;
        let region = *region_for_boundary.get(parent).ok_or(invalid(
            Some(boundary.path),
            None,
            RegionInputIssue::ContainmentForest,
        ))?;
        let record = regions.get(region).ok_or(invalid(
            Some(boundary.path),
            None,
            RegionInputIssue::ContainmentForest,
        ))?;
        let position = hole_positions[parent];
        if region == NO_INDEX || position >= record.holes.end {
            return Err(invalid(
                Some(boundary.path),
                None,
                RegionInputIssue::ContainmentForest,
            ));
        }
        region_holes[position] = index;
        hole_positions[parent] = add(position, 1)?;
    }
    Ok(())
}

/// Assemble the exact nonconstructive `EvenOddStrict` containment hierarchy.
///
/// Inputs must borrow the same `PlanarInspection`; a mismatched grid or area report is rejected.
/// Input and storage caps precede every fallible reservation. Candidate paths are classified in
/// original ID order, then exact bounds/pairs, the laminar forest, and final records are assembled
/// deterministically. The positive-X ray uses the half-open Y rule and checked-domain `i128` cross
/// products; no floating arithmetic, epsilon, winding rewrite, closure, welding, construction,
/// repair, thread or dependency is introduced. Direct ray-edge work has its own cap because a
/// point-in-cycle query examines each retained edge. `RegionsInput`, `RegionsBounds`,
/// `RegionsPairs`, `RegionsForest` and `RegionsOutput` poll every 256 records from zero;
/// `RegionsStorage` checks both sides of every nonempty reservation and `RegionsComplete` precedes
/// publication. Cancellation and operational errors publish no partial hierarchy.
/// # Errors
/// Returns typed provenance, consistency, cap, allocation, arithmetic or cancellation failures.
/// Open/ambiguous/contacting geometry and unresolved complete-only context are successful explicit
/// outcomes with `GeometricallyUnresolved`, never silently repaired or converted to an error.
pub fn assemble_strict_regions<'a, 'i, 'p, 'g, 'n, 'r>(
    input: &'i PlanarInspection<'p, 'g, 'n, 'r>,
    areas: &'a SectionAreas<'i, 'p, 'g, 'n, 'r>,
    grid: &'a ExactGridView<'i, 'p, 'g, 'n, 'r>,
    limits: RegionLimits,
) -> Result<SectionRegions<'a, 'i, 'p, 'g, 'n, 'r>> {
    assemble_strict_regions_with_cancellation(
        input,
        areas,
        grid,
        limits,
        &mut Cancellation::never(),
    )
}

/// `EvenOddStrict` assembly with synchronous TD-014 cooperative cancellation.
///
/// This is the cancellation-aware form of [`assemble_strict_regions`]. The predicate stays on the
/// caller's thread; no partial `SectionRegions` escapes after cancellation at any documented phase.
/// # Errors
/// Uses the same typed outcomes and failures as [`assemble_strict_regions`].
pub fn assemble_strict_regions_with_cancellation<'a, 'i, 'p, 'g, 'n, 'r>(
    input: &'i PlanarInspection<'p, 'g, 'n, 'r>,
    areas: &'a SectionAreas<'i, 'p, 'g, 'n, 'r>,
    grid: &'a ExactGridView<'i, 'p, 'g, 'n, 'r>,
    limits: RegionLimits,
    cancel: &mut Cancellation<'_>,
) -> Result<SectionRegions<'a, 'i, 'p, 'g, 'n, 'r>> {
    cancel.check(Checkpoint::RegionsStart)?;
    validate_reports(input, areas, grid, limits)?;
    let context = contact_context(input, grid, cancel)?;
    let plan = preflight(
        input,
        areas,
        grid,
        limits,
        &context.contacted,
        context.diagnostic_count,
        cancel,
    )?;

    let buffers = build_cycles(input, areas, grid, plan, &context.contacted, cancel)?;
    let pairs = classify_pairs(&buffers.cycles, &buffers.coordinates, plan, limits, cancel)?;
    let forest = build_forest(&buffers.cycles, &pairs.relations, cancel)?;
    let diagnostics = copy_diagnostics(input, &context, plan, cancel)?;
    let output = assemble_boundaries(
        input,
        areas,
        grid,
        &context,
        &BoundaryInputs {
            plan,
            cycles: &buffers.cycles,
            candidate_for_path: &buffers.candidate_for_path,
            forest: &forest,
        },
        cancel,
    )?;
    let (hole_counts, holes) =
        count_holes(&output.boundaries, &buffers.candidate_for_path, cancel)?;
    let parts = assemble_material_regions(
        &output.boundaries,
        &buffers.candidate_for_path,
        &hole_counts,
        holes,
        limits,
        cancel,
    )?;
    let region_count = parts.regions.len();

    cancel.check(Checkpoint::RegionsComplete)?;
    Ok(SectionRegions {
        input,
        areas,
        grid,
        boundaries: output.boundaries,
        regions: parts.regions,
        holes: parts.holes,
        path_outcomes: output.outcomes,
        diagnostics,
        completeness: if output.geometrically_unresolved {
            RegionCompleteness::GeometricallyUnresolved
        } else {
            RegionCompleteness::Complete
        },
        summary: RegionSummary {
            paths: plan.paths,
            cycle_candidates: input.paths().summary().cycle_candidates,
            cycle_vertices: plan.cycle_vertices,
            boundaries: plan.candidates,
            non_boundaries: plan.paths - plan.candidates,
            pairs: plan.pairs,
            ray_tests: pairs.ray_tests,
            relations: pairs.relations.len(),
            regions: region_count,
            holes,
            diagnostics: plan.diagnostics,
            requested_payload_bytes: plan.requested_payload_bytes,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subdividing_collinear_sides_preserves_analytic_rectangle_membership() {
        let polygon = [
            [0.0, 0.0],
            [4.0, 0.0],
            [4.0, 1.0],
            [4.0, 2.0],
            [4.0, 3.0],
            [4.0, 4.0],
            [0.0, 4.0],
            [0.0, 3.0],
            [0.0, 2.0],
            [0.0, 1.0],
        ];
        for reverse in [false, true] {
            for rotation in 0..polygon.len() {
                let mut reordered = polygon;
                if reverse {
                    reordered.reverse();
                }
                reordered.rotate_left(rotation);
                for x in -1..=5 {
                    for y in -1..=5 {
                        // The containment caller already excludes boundary contacts.
                        if x == 0 || x == 4 || y == 0 || y == 4 {
                            continue;
                        }
                        let expected = 0 < x && x < 4 && 0 < y && y < 4;
                        assert_eq!(
                            contains(
                                &reordered,
                                [f64::from(x), f64::from(y)],
                                &mut 0,
                                RegionLimits::default(),
                                &mut Cancellation::never()
                            )
                            .unwrap(),
                            expected
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn positive_ray_uses_one_half_open_y_endpoint_at_each_vertex() {
        let square = [[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0]];
        let mut cancellation = Cancellation::never();
        let mut checks = 0;
        assert!(!contains(
            &square,
            [-1.0, 0.0],
            &mut checks,
            RegionLimits::default(),
            &mut cancellation,
        )
        .unwrap());
        assert!(!contains(
            &square,
            [-1.0, 4.0],
            &mut checks,
            RegionLimits::default(),
            &mut cancellation,
        )
        .unwrap());
        assert!(contains(
            &square,
            [2.0, 2.0],
            &mut checks,
            RegionLimits::default(),
            &mut cancellation,
        )
        .unwrap());
        assert!(!contains(
            &square,
            [5.0, 2.0],
            &mut checks,
            RegionLimits::default(),
            &mut cancellation,
        )
        .unwrap());
        assert_eq!(checks, 16);
    }

    #[test]
    fn td_025_extremes_preserve_membership_in_shared_exact_predicates() {
        let limit = 2_f64.powi(52);
        let square = [
            [-limit, -limit],
            [limit, -limit],
            [limit, limit],
            [-limit, limit],
        ];
        let mut cancellation = Cancellation::never();
        let mut checks = 0;
        assert!(contains(
            &square,
            [0.0, 0.0],
            &mut checks,
            RegionLimits::default(),
            &mut cancellation,
        )
        .unwrap());
        assert_eq!(checks, 4);
        assert!(may_contain(
            [-limit, -limit, limit, limit],
            [-1.0, -1.0, 1.0, 1.0]
        ));
    }
}
