//! Conservative ordered paths over the borrowed M3.3-A graph (TD-022).
//!
//! Closed cycle candidates are graph-simple only: no XY simplicity, area, orientation, nesting,
//! filled-boundary or printability claim. Branches and unresolved neighborhoods stop continuity.
//! Complete-only groups retain their original provenance and are never walked as candidate edges.
//!
//! ```
//! use karasu_core::{mesh::TriangleMesh, slicing::{section_mesh_z, SectionLimits, SectionTransform}};
//! use karasu_core::slicing::normalize::{normalize_section, NormalizationLimits};
//! use karasu_core::slicing::graph::{build_incidence_graph, GraphLimits};
//! use karasu_core::slicing::paths::{extract_paths, PathLimits};
//! let mesh = TriangleMesh::try_new(vec![], vec![])?;
//! let raw = section_mesh_z(&mesh, 0.0, SectionTransform::Identity, SectionLimits::default())?;
//! let normalized = normalize_section(&raw, NormalizationLimits::default())?;
//! let graph = build_incidence_graph(&normalized, GraphLimits::default())?;
//! let result = extract_paths(&graph, PathLimits::default())?;
//! assert_eq!(result.paths(), []);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use super::graph::{AdjacentIncidence, EdgeEligibility, GraphEdge, IncidenceGraph};
use super::normalize::{CoordinateAgreement, IncidenceIssues, Ownership};
use crate::cancellation::{Cancellation, Cancelled, Checkpoint};
use std::{error::Error, fmt, ops::Range};

/// Graph-theoretic classification only, never a valid polygon or material boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathKind {
    /// Maximal clean chain between degree-one endpoints; no ownership ambiguity touches it.
    OpenChain,
    /// At least three distinct graph vertices, all clean degree two; XY validity is unassessed.
    ClosedCycleCandidate,
    /// Maximal nonbranching trail stopped at uncertainty/branches, possibly graph-closed.
    AmbiguousTrail,
}

/// Independent reason not to treat a path or component as clean graph continuity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathIssue {
    /// More than two candidate ends meet; no continuation is chosen through the vertex.
    Branch,
    /// At least one complete-only unresolved group touches the vertex.
    UnresolvedNeighborhood,
    /// The source feature has incompatible positions; no point is selected.
    CoordinateConflict,
    /// A graph-closed trail has fewer than three graph vertices, not a nondegenerate contour.
    DegenerateCycle,
}

/// Compact graph-ambiguity flags; geometric simplicity/area are not assessed even if empty.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PathIssues(u8);

impl PathIssues {
    /// Whether an independent reason is present.
    #[must_use]
    pub const fn contains(self, issue: PathIssue) -> bool {
        self.0 & (1 << (issue as u8)) != 0
    }
    /// No graph-level uncertainty found along this trail; not polygon validity.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
    fn add(&mut self, issue: PathIssue) {
        self.0 |= 1 << (issue as u8);
    }
    fn merge(&mut self, other: Self) {
        self.0 |= other.0;
    }
}

/// Immutable ordered path metadata; query its flat vertex/edge slices from the owning result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContourPath {
    /// Candidate-connected component ID, ordered by its minimum vertex ID.
    pub component: usize,
    /// Graph-level taxonomy, explicitly provisional with respect to XY geometry.
    pub kind: PathKind,
    /// Actual graph return to the start, not artificial closing or polygon certification.
    pub closed: bool,
    /// Reasons that stopped clean continuity, including either endpoint's boundary issues.
    pub issues: PathIssues,
    /// Canonically ordered endpoint IDs for an open trail; None for any closed walk.
    pub endpoints: Option<[usize; 2]>,
    vertices: Range<usize>,
    edges: Range<usize>,
}

/// Candidate-connected component, not a component of complete adjacency or a polygon region.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PathComponent {
    /// Smallest canonical graph vertex ID in this component.
    pub minimum_vertex: usize,
    /// Candidate-incident vertex count; tangent-only/complete-only vertices are not components.
    pub vertices: usize,
    /// Candidate graph groups in this component, not raw occurrences.
    pub edges: usize,
    /// Extracted trails/cycles, possibly several terminating at a shared branch.
    pub paths: usize,
    /// Union of graph uncertainty found on component vertices/paths.
    pub issues: PathIssues,
}

/// Exactly one outcome per complete graph group; raw occurrences remain accessible through `graph()`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EdgeOutcome {
    /// This candidate group is consumed once at the given offset, never once per raw occurrence.
    Path {
        /// Final canonical path ID.
        path: usize,
        /// Edge offset within that path.
        offset: usize,
    },
    /// Verified local internal coplanar group, excluded only by unchanged TD-019.
    InternalCoplanar,
    /// Complete-only unresolved/nontraversable group, with original issues and multiplicity intact.
    Unresolved,
}

/// Ordered diagnostic records; original graph diagnostics remain available separately via `graph()`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathDiagnostic {
    /// Candidate routing boundary or isolated tangent, in canonical vertex order.
    Vertex {
        /// Graph vertex ID; inspect all original coordinate observations there.
        vertex: usize,
        /// Branch, unresolved neighborhood or coordinate conflict, possibly empty for a tangent.
        issues: PathIssues,
        /// No complete segment incidence, rather than merely zero candidate degree.
        isolated: bool,
    },
    /// Complete-only group retains all original ambiguity/collapse/multiplicity reasons.
    UnresolvedEdge {
        /// Graph group ID.
        edge: usize,
        /// Original independent M3.2-B issue bits.
        issues: IncidenceIssues,
    },
}

/// Independent input work, actual output counts and aggregate requested payload caps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PathLimits {
    /// Maximum graph vertices inspected, including complete-only/tangent vertices.
    pub max_graph_vertices: usize,
    /// Maximum complete graph group records inspected.
    pub max_graph_edges: usize,
    /// Maximum actual path records, not an a priori candidate count requirement.
    pub max_paths: usize,
    /// Maximum actual flat vertex references; closed paths omit the repeated terminal vertex.
    pub max_path_vertices: usize,
    /// Maximum actual flat graph-edge references.
    pub max_path_edges: usize,
    /// Maximum actual candidate-connected component records.
    pub max_components: usize,
    /// Maximum actual new diagnostics; borrowed graph diagnostics are not copied.
    pub max_diagnostics: usize,
    /// Aggregate requested output plus scratch capacity bytes; not whole-process RSS.
    pub max_payload_bytes: usize,
}

impl Default for PathLimits {
    fn default() -> Self {
        Self {
            max_graph_vertices: 3_000_000,
            max_graph_edges: 3_000_000,
            max_paths: 3_000_000,
            max_path_vertices: 6_000_000,
            max_path_edges: 3_000_000,
            max_components: 3_000_000,
            max_diagnostics: 6_000_000,
            max_payload_bytes: 512 * 1024 * 1024,
        }
    }
}

/// Specific path-processing cap.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathResource {
    /// Input graph vertices.
    GraphVertices,
    /// Input complete graph groups.
    GraphEdges,
    /// Actual path records.
    Paths,
    /// Actual path vertex references.
    PathVertices,
    /// Actual path edge references.
    PathEdges,
    /// Actual candidate components.
    Components,
    /// Actual new diagnostics.
    Diagnostics,
    /// Aggregate requested owned output/scratch payload bytes.
    PayloadBytes,
}

/// Operational failure, distinct from successfully reported representable geometric ambiguity.
#[derive(Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum PathError {
    /// Synchronous cancellation at the observed checkpoint; no partial success.
    Cancelled(Cancelled),
    /// A configured count or requested-buffer cap was exceeded.
    LimitExceeded {
        /// Which cap failed.
        resource: PathResource,
        /// Configured maximum.
        limit: usize,
    },
    /// Checked arithmetic or isize allocation-range overflow.
    SizeOverflow,
    /// Fallible reservation failed.
    Allocation,
    /// Inconsistent graph identity, adjacency, degree or final consumption accounting.
    InvalidGraph {
        /// Relevant vertex when known.
        vertex: Option<usize>,
        /// Relevant group when known.
        edge: Option<usize>,
    },
}

impl fmt::Display for PathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled(error) => error.fmt(f),
            Self::LimitExceeded { resource, limit } => {
                write!(f, "contour path {resource:?} limit exceeded ({limit})")
            }
            Self::SizeOverflow => write!(f, "contour path count or capacity arithmetic overflow"),
            Self::Allocation => write!(f, "unable to reserve contour path buffers"),
            Self::InvalidGraph { vertex, edge } => write!(
                f,
                "inconsistent contour incidence graph at vertex {vertex:?}, edge {edge:?}"
            ),
        }
    }
}
impl Error for PathError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Cancelled(error) => Some(error),
            _ => None,
        }
    }
}
impl From<Cancelled> for PathError {
    fn from(error: Cancelled) -> Self {
        Self::Cancelled(error)
    }
}

/// Deterministic conservation and taxonomy totals, not polygon/region/printability counts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PathSummary {
    /// All complete graph groups.
    pub graph_edges: usize,
    /// Candidate groups, each consumed exactly once on success.
    pub candidate_edges: usize,
    /// Sum of path edge references, equal to `candidate_edges` on success.
    pub consumed_edges: usize,
    /// Borrowed raw segment occurrences across all complete groups, not path-edge count.
    pub raw_occurrences: usize,
    /// Retained complete-only local internal coplanar groups.
    pub internal_edges: usize,
    /// Retained complete-only ambiguous/nontraversable groups.
    pub unresolved_edges: usize,
    /// Candidate-connected components.
    pub components: usize,
    /// Components touched by graph ambiguity.
    pub ambiguous_components: usize,
    /// All ordered path records.
    pub paths: usize,
    /// Clean graph-open chains.
    pub open_chains: usize,
    /// Clean graph-simple cycle candidates; XY validity unassessed.
    pub cycle_candidates: usize,
    /// Boundary-stopped trails, possibly graph-closed.
    pub ambiguous_trails: usize,
    /// All actual closed walks, including ambiguous returns to a branch/uncertain vertex.
    pub closed_walks: usize,
    /// Flat vertex reference count; closed terminal vertices are omitted.
    pub path_vertices: usize,
    /// New vertex/unresolved-edge diagnostics.
    pub diagnostics: usize,
    /// Aggregate requested capacities including scratch later released; not actual RSS.
    pub requested_payload_bytes: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Assignment {
    Pending,
    Path { path: usize, offset: usize },
    Internal,
    Unresolved,
}

/// Immutable ordered paths borrowing the original graph and all original provenance transitively.
#[derive(Debug, PartialEq)]
pub struct SectionPaths<'g, 'n, 'r> {
    graph: &'g IncidenceGraph<'n, 'r>,
    paths: Vec<ContourPath>,
    vertices: Vec<usize>,
    edges: Vec<usize>,
    components: Vec<PathComponent>,
    outcomes: Vec<Assignment>,
    diagnostics: Vec<PathDiagnostic>,
    summary: PathSummary,
}

impl<'g, 'n, 'r> SectionPaths<'g, 'n, 'r> {
    /// Same immutable graph, including complete-only groups and every raw occurrence reference.
    #[must_use]
    pub const fn graph(&self) -> &'g IncidenceGraph<'n, 'r> {
        self.graph
    }
    /// Paths ordered by canonical (first vertex, first edge), not geometric winding.
    #[must_use]
    pub fn paths(&self) -> &[ContourPath] {
        &self.paths
    }
    /// Ordered vertex IDs; a closed path does not repeat its first vertex at the end.
    #[must_use]
    pub fn path_vertices(&self, path: usize) -> Option<&[usize]> {
        self.paths
            .get(path)
            .map(|p| &self.vertices[p.vertices.clone()])
    }
    /// Ordered graph group IDs; resolve raw provenance and multiplicity through `graph()`.
    #[must_use]
    pub fn path_edges(&self, path: usize) -> Option<&[usize]> {
        self.paths.get(path).map(|p| &self.edges[p.edges.clone()])
    }
    /// Candidate components in ascending minimum-vertex order; paths carry their component ID.
    #[must_use]
    pub fn components(&self) -> &[PathComponent] {
        &self.components
    }
    /// Exactly one outcome for every valid complete graph group; None only for an invalid ID.
    #[must_use]
    pub fn edge_outcome(&self, edge: usize) -> Option<EdgeOutcome> {
        match self.outcomes.get(edge)? {
            Assignment::Path { path, offset } => Some(EdgeOutcome::Path {
                path: *path,
                offset: *offset,
            }),
            Assignment::Internal => Some(EdgeOutcome::InternalCoplanar),
            Assignment::Unresolved => Some(EdgeOutcome::Unresolved),
            Assignment::Pending => None, // Final accounting rejects this private construction state.
        }
    }
    /// Vertex diagnostics first, then complete-only unresolved groups, in canonical ID order.
    #[must_use]
    pub fn diagnostics(&self) -> &[PathDiagnostic] {
        &self.diagnostics
    }
    /// Complete counts and conservation totals.
    #[must_use]
    pub const fn summary(&self) -> PathSummary {
        self.summary
    }
}

type Result<T> = std::result::Result<T, PathError>;
const UNASSIGNED: usize = usize::MAX;

#[derive(Clone, Copy)]
struct VertexState {
    component: usize,
    last_path: usize,
    issues: PathIssues,
}

struct Builder<'g, 'n, 'r> {
    result: SectionPaths<'g, 'n, 'r>,
    states: Vec<VertexState>,
    stack: Vec<usize>,
    limits: PathLimits,
}

/// Extracts maximal nonbranching trails and provisional closed cycles without repair.
/// # Errors
/// Returns structured cancellation, count/byte, capacity, allocation or graph-consistency failure.
pub fn extract_paths<'g, 'n, 'r>(
    graph: &'g IncidenceGraph<'n, 'r>,
    limits: PathLimits,
) -> Result<SectionPaths<'g, 'n, 'r>> {
    extract_paths_with_cancellation(graph, limits, &mut Cancellation::never())
}

/// Consumes each candidate group once, regardless of raw multiplicity. Stop at degree !=2 or a
/// conflict/unresolved neighborhood. Branches are boundaries, never routing choices. Complete-only
/// internal/unresolved groups have explicit outcomes and stay borrowed, including loops/duplicates.
/// Open starts use the smaller endpoint ID. Closed walks use their unique minimum vertex, then the
/// smaller incident edge ID. Canonical direction is not clockwise/counterclockwise. Paths are sorted
/// by (first vertex, first edge), a unique total key because groups are consumed once.
///
/// For V vertices, G complete groups, A adjacency ends, C candidates, P paths and D diagnostics:
/// O(V+G+A+C+P log(P+1)+D) time, O(V+G+C+P+D) requested storage. No raw occurrence/point copies,
/// repeated front insertion, planar certification, components via unresolved edges or progress API.
/// Allocation, individual rotations/reversals, sort and drop are indivisible; surrounding checks
/// and 256-record loop polls do not promise wall-clock latency or whole-process RSS.
/// # Errors
/// Entry cancellation precedes input caps; graph consistency/counts precede byte reservations, then
/// actual component/path/diagnostic caps and final accounting/publication. Cancellation wins where
/// observed; no successful partial result escapes. A privately constructed graph is required, not an
/// external graph deserializer. Defensive consistency failures are not silently converted to geometry.
pub fn extract_paths_with_cancellation<'g, 'n, 'r>(
    graph: &'g IncidenceGraph<'n, 'r>,
    limits: PathLimits,
    cancel: &mut Cancellation<'_>,
) -> Result<SectionPaths<'g, 'n, 'r>> {
    cancel.check(Checkpoint::PathsStart)?;
    let summary = inspect(graph, limits, cancel)?;
    let mut builder = Builder::new(graph, limits, summary, cancel)?;
    builder.discover(cancel)?;
    builder.decompose(cancel)?;
    builder.finish(cancel)?;
    cancel.check(Checkpoint::PathsComplete)?;
    Ok(builder.result)
}

fn invalid(vertex: Option<usize>, edge: Option<usize>) -> PathError {
    PathError::InvalidGraph { vertex, edge }
}
fn add(a: usize, b: usize) -> Result<usize> {
    a.checked_add(b).ok_or(PathError::SizeOverflow)
}
fn mul(a: usize, b: usize) -> Result<usize> {
    a.checked_mul(b).ok_or(PathError::SizeOverflow)
}
fn bytes<T>(count: usize) -> Result<usize> {
    let value = mul(count, size_of::<T>())?;
    isize::try_from(value).map_err(|_| PathError::SizeOverflow)?;
    Ok(value)
}
fn cap(value: usize, limit: usize, resource: PathResource) -> Result<()> {
    if value > limit {
        Err(PathError::LimitExceeded { resource, limit })
    } else {
        Ok(())
    }
}
fn reserve<T>(count: usize, cancel: &mut Cancellation<'_>) -> Result<Vec<T>> {
    let mut buffer = Vec::new();
    if count != 0 {
        cancel.check(Checkpoint::PathsStorage)?;
        buffer
            .try_reserve_exact(count)
            .map_err(|_| PathError::Allocation)?;
        cancel.check(Checkpoint::PathsStorage)?;
    }
    Ok(buffer)
}
fn push<T>(buffer: &mut Vec<T>, value: T, limit: usize, resource: PathResource) -> Result<()> {
    cap(add(buffer.len(), 1)?, limit, resource)?;
    // Every buffer has a checked conservative capacity, intersected with this actual-count cap.
    if buffer.len() == buffer.capacity() {
        return Err(PathError::SizeOverflow);
    }
    buffer.push(value);
    Ok(())
}

fn inspect(
    graph: &IncidenceGraph<'_, '_>,
    limits: PathLimits,
    cancel: &mut Cancellation<'_>,
) -> Result<PathSummary> {
    let vertices = graph.vertices().len();
    let edges = graph.edges().len();
    cap(
        vertices,
        limits.max_graph_vertices,
        PathResource::GraphVertices,
    )?;
    cap(edges, limits.max_graph_edges, PathResource::GraphEdges)?;
    let normalized = graph.normalized();
    if normalized.features().len() != vertices || normalized.groups().len() != edges {
        return Err(invalid(None, None));
    }
    let mut summary = PathSummary {
        graph_edges: edges,
        ..PathSummary::default()
    };
    for (id, edge) in graph.edges().iter().enumerate() {
        cancel.poll(Checkpoint::PathsInput, id)?;
        let group = &normalized.groups()[id];
        if edge.group != id
            || edge.endpoints.iter().any(|&v| v >= vertices)
            || edge.endpoints.map(|v| normalized.features()[v].feature) != group.endpoints
            || edge.multiplicity != group.occurrences.len()
        {
            return Err(invalid(None, Some(id)));
        }
        let expected = match group.ownership {
            Ownership::CrossingCandidate
            | Ownership::OnPlaneCrossingCandidate
            | Ownership::CoplanarPatchBoundary => EdgeEligibility::Candidate,
            Ownership::CoplanarPatchInternal => EdgeEligibility::InternalCoplanar,
            Ownership::Unresolved => EdgeEligibility::Unresolved,
        };
        if edge.eligibility != expected {
            return Err(invalid(None, Some(id)));
        }
        match edge.eligibility {
            EdgeEligibility::Candidate => {
                let expected_multiplicity = if group.ownership
                    == Ownership::OnPlaneCrossingCandidate
                    || (group.ownership == Ownership::CoplanarPatchBoundary
                        && group.occurrences.len() == 2)
                {
                    2
                } else {
                    1
                };
                if edge.multiplicity != expected_multiplicity
                    || edge.endpoints[0] >= edge.endpoints[1]
                    || !group.issues.is_empty()
                {
                    return Err(invalid(None, Some(id)));
                }
                summary.candidate_edges = add(summary.candidate_edges, 1)?;
            }
            EdgeEligibility::InternalCoplanar => {
                summary.internal_edges = add(summary.internal_edges, 1)?;
            }
            EdgeEligibility::Unresolved => {
                summary.unresolved_edges = add(summary.unresolved_edges, 1)?;
            }
        }
        summary.raw_occurrences = add(summary.raw_occurrences, edge.multiplicity)?;
    }
    let (complete_count, candidate_count) = inspect_adjacency(graph, cancel)?;
    let graph_summary = graph.summary();
    if complete_count != mul(edges, 2)?
        || candidate_count != mul(summary.candidate_edges, 2)?
        || graph_summary.vertices != vertices
        || graph_summary.edges != edges
        || graph_summary.candidate_edges != summary.candidate_edges
        || graph_summary.occurrences != summary.raw_occurrences
    {
        return Err(invalid(None, None));
    }
    Ok(summary)
}

fn inspect_adjacency(
    graph: &IncidenceGraph<'_, '_>,
    cancel: &mut Cancellation<'_>,
) -> Result<(usize, usize)> {
    let mut complete_count = 0;
    let mut candidate_count = 0;
    for (id, vertex) in graph.vertices().iter().enumerate() {
        cancel.poll(Checkpoint::PathsInput, id)?;
        if vertex.feature_group != id {
            return Err(invalid(Some(id), None));
        }
        let complete = graph
            .complete_adjacency(id)
            .ok_or_else(|| invalid(Some(id), None))?;
        let candidates = graph
            .candidate_adjacency(id)
            .ok_or_else(|| invalid(Some(id), None))?;
        let mut previous = None;
        let mut unique = 0;
        let mut unresolved = 0;
        let mut raw_ends = 0;
        let mut eligible = 0;
        for (index, entry) in complete.iter().enumerate() {
            cancel.poll(Checkpoint::PathsInput, index)?;
            validate_entry(graph.edges(), id, *entry, previous, false)?;
            let edge = graph.edges()[entry.edge];
            if previous.map(|key: (usize, usize)| key.0) != Some(entry.edge) {
                unique = add(unique, 1)?;
                if edge.eligibility == EdgeEligibility::Unresolved {
                    unresolved = add(unresolved, 1)?;
                }
                if edge.eligibility == EdgeEligibility::Candidate {
                    eligible = add(eligible, 1)?;
                }
            }
            previous = Some((entry.edge, entry.endpoint));
            raw_ends = add(raw_ends, edge.multiplicity)?;
        }
        previous = None;
        for (index, entry) in candidates.iter().enumerate() {
            cancel.poll(Checkpoint::PathsInput, index)?;
            validate_entry(graph.edges(), id, *entry, previous, true)?;
            previous = Some((entry.edge, entry.endpoint));
        }
        let degrees = vertex.degrees;
        if degrees.unique_edges != unique
            || degrees.unresolved_edges != unresolved
            || degrees.raw_occurrence_ends != raw_ends
            || degrees.eligible_edges != eligible
            || degrees.eligible_incidence_degree != candidates.len()
        {
            return Err(invalid(Some(id), None));
        }
        complete_count = add(complete_count, complete.len())?;
        candidate_count = add(candidate_count, candidates.len())?;
    }
    Ok((complete_count, candidate_count))
}

fn validate_entry(
    edges: &[GraphEdge],
    vertex: usize,
    entry: AdjacentIncidence,
    previous: Option<(usize, usize)>,
    candidate: bool,
) -> Result<()> {
    let Some(edge) = edges.get(entry.edge) else {
        return Err(invalid(Some(vertex), Some(entry.edge)));
    };
    if entry.vertex != vertex
        || entry.endpoint >= 2
        || edge.endpoints[entry.endpoint] != vertex
        || previous.is_some_and(|key| key >= (entry.edge, entry.endpoint))
        || (candidate && edge.eligibility != EdgeEligibility::Candidate)
    {
        return Err(invalid(Some(vertex), Some(entry.edge)));
    }
    Ok(())
}

impl<'g, 'n, 'r> Builder<'g, 'n, 'r> {
    fn new(
        graph: &'g IncidenceGraph<'n, 'r>,
        limits: PathLimits,
        mut summary: PathSummary,
        cancel: &mut Cancellation<'_>,
    ) -> Result<Self> {
        let vertices = graph.vertices().len();
        let edges = graph.edges().len();
        let candidates = summary.candidate_edges;
        let paths = candidates.min(limits.max_paths);
        let refs = mul(candidates, 2)?.min(limits.max_path_vertices);
        let edge_refs = candidates.min(limits.max_path_edges);
        let components = vertices.min(candidates).min(limits.max_components);
        let diagnostics = add(vertices, edges)?.min(limits.max_diagnostics);
        summary.requested_payload_bytes = [
            bytes::<ContourPath>(paths)?,
            bytes::<usize>(refs)?,
            bytes::<usize>(edge_refs)?,
            bytes::<PathComponent>(components)?,
            bytes::<Assignment>(edges)?,
            bytes::<PathDiagnostic>(diagnostics)?,
            bytes::<VertexState>(vertices)?,
            bytes::<usize>(vertices)?,
        ]
        .into_iter()
        .try_fold(0, add)?;
        cap(
            summary.requested_payload_bytes,
            limits.max_payload_bytes,
            PathResource::PayloadBytes,
        )?;
        let mut result = SectionPaths {
            graph,
            paths: reserve(paths, cancel)?,
            vertices: reserve(refs, cancel)?,
            edges: reserve(edge_refs, cancel)?,
            components: reserve(components, cancel)?,
            outcomes: reserve(edges, cancel)?,
            diagnostics: reserve(diagnostics, cancel)?,
            summary,
        };
        let mut states = reserve(vertices, cancel)?;
        let stack = reserve(vertices, cancel)?;
        for (id, vertex) in graph.vertices().iter().enumerate() {
            cancel.poll(Checkpoint::PathsInput, id)?;
            let mut issues = PathIssues::default();
            if vertex.degrees.eligible_incidence_degree > 2 {
                issues.add(PathIssue::Branch);
            }
            if vertex.degrees.unresolved_edges > 0 {
                issues.add(PathIssue::UnresolvedNeighborhood);
            }
            if matches!(
                graph.normalized().features()[id].coordinates,
                CoordinateAgreement::Conflict
            ) {
                issues.add(PathIssue::CoordinateConflict);
            }
            states.push(VertexState {
                component: UNASSIGNED,
                last_path: UNASSIGNED,
                issues,
            });
        }
        for (id, edge) in graph.edges().iter().enumerate() {
            cancel.poll(Checkpoint::PathsInput, id)?;
            result.outcomes.push(match edge.eligibility {
                EdgeEligibility::Candidate => Assignment::Pending,
                EdgeEligibility::InternalCoplanar => Assignment::Internal,
                EdgeEligibility::Unresolved => Assignment::Unresolved,
            });
        }
        Ok(Self {
            result,
            states,
            stack,
            limits,
        })
    }

    fn discover(&mut self, cancel: &mut Cancellation<'_>) -> Result<()> {
        let graph = self.result.graph;
        for start in 0..self.states.len() {
            cancel.poll(Checkpoint::PathsComponents, start)?;
            if self.states[start].component != UNASSIGNED
                || graph.vertices()[start].degrees.eligible_incidence_degree == 0
            {
                continue;
            }
            let id = self.result.components.len();
            cap(
                add(id, 1)?,
                self.limits.max_components,
                PathResource::Components,
            )?;
            let mut component = PathComponent {
                minimum_vertex: start,
                vertices: 0,
                edges: 0,
                paths: 0,
                issues: PathIssues::default(),
            };
            self.states[start].component = id;
            self.stack.push(start);
            while let Some(vertex) = self.stack.pop() {
                cancel.poll(Checkpoint::PathsComponents, component.vertices)?;
                component.vertices = add(component.vertices, 1)?;
                component.issues.merge(self.states[vertex].issues);
                for (index, entry) in graph
                    .candidate_adjacency(vertex)
                    .ok_or_else(|| invalid(Some(vertex), None))?
                    .iter()
                    .enumerate()
                {
                    cancel.poll(Checkpoint::PathsComponents, index)?;
                    let edge = graph.edges()[entry.edge];
                    if entry.endpoint == 0 {
                        component.edges = add(component.edges, 1)?;
                    }
                    let other = edge.endpoints[1 - entry.endpoint];
                    if self.states[other].component == UNASSIGNED {
                        self.states[other].component = id;
                        self.stack.push(other);
                    }
                }
            }
            push(
                &mut self.result.components,
                component,
                self.limits.max_components,
                PathResource::Components,
            )?;
        }
        Ok(())
    }

    fn boundary(&self, vertex: usize) -> bool {
        self.result.graph.vertices()[vertex]
            .degrees
            .eligible_incidence_degree
            != 2
            || !self.states[vertex].issues.is_empty()
    }

    fn decompose(&mut self, cancel: &mut Cancellation<'_>) -> Result<()> {
        let graph = self.result.graph;
        for vertex in 0..self.states.len() {
            cancel.poll(Checkpoint::PathsTraversal, vertex)?;
            if !self.boundary(vertex) {
                continue;
            }
            for (index, entry) in graph
                .candidate_adjacency(vertex)
                .ok_or_else(|| invalid(Some(vertex), None))?
                .iter()
                .enumerate()
            {
                cancel.poll(Checkpoint::PathsTraversal, index)?;
                if self.result.outcomes[entry.edge] == Assignment::Pending {
                    self.walk(vertex, entry.edge, cancel)?;
                }
            }
        }
        // Any remaining group belongs to a wholly clean degree-two component. No route is chosen
        // through a branch here: all boundary incidences were consumed by the first pass.
        for (id, edge) in graph.edges().iter().enumerate() {
            cancel.poll(Checkpoint::PathsTraversal, id)?;
            if self.result.outcomes[id] == Assignment::Pending {
                self.walk(edge.endpoints[0], id, cancel)?;
            }
        }
        Ok(())
    }

    fn walk(&mut self, start: usize, first: usize, cancel: &mut Cancellation<'_>) -> Result<()> {
        let graph = self.result.graph;
        let path_id = self.result.paths.len();
        cap(add(path_id, 1)?, self.limits.max_paths, PathResource::Paths)?;
        let vertex_start = self.result.vertices.len();
        let edge_start = self.result.edges.len();
        push(
            &mut self.result.vertices,
            start,
            self.limits.max_path_vertices,
            PathResource::PathVertices,
        )?;
        self.states[start].last_path = path_id;
        let mut vertex = start;
        let mut edge_id = first;
        let mut issues = self.states[start].issues;
        let closed;
        loop {
            cancel.poll(
                Checkpoint::PathsTraversal,
                self.result.edges.len() - edge_start,
            )?;
            let edge = *graph
                .edges()
                .get(edge_id)
                .ok_or_else(|| invalid(Some(vertex), Some(edge_id)))?;
            if self.result.outcomes[edge_id] != Assignment::Pending {
                return Err(invalid(Some(vertex), Some(edge_id)));
            }
            let next = other_endpoint(edge, vertex, edge_id)?;
            self.result.outcomes[edge_id] = Assignment::Path {
                path: path_id,
                offset: self.result.edges.len() - edge_start,
            };
            push(
                &mut self.result.edges,
                edge_id,
                self.limits.max_path_edges,
                PathResource::PathEdges,
            )?;
            issues.merge(self.states[next].issues);
            if next == start {
                closed = true;
                break;
            }
            if self.states[next].last_path == path_id {
                return Err(invalid(Some(next), Some(edge_id)));
            }
            self.states[next].last_path = path_id;
            push(
                &mut self.result.vertices,
                next,
                self.limits.max_path_vertices,
                PathResource::PathVertices,
            )?;
            if self.boundary(next) {
                closed = false;
                break;
            }
            let adjacent = graph
                .candidate_adjacency(next)
                .ok_or_else(|| invalid(Some(next), None))?;
            if adjacent.len() != 2 {
                return Err(invalid(Some(next), None));
            }
            edge_id = if adjacent[0].edge == edge_id {
                adjacent[1].edge
            } else if adjacent[1].edge == edge_id {
                adjacent[0].edge
            } else {
                return Err(invalid(Some(next), Some(edge_id)));
            };
            vertex = next;
        }
        self.publish_path(start, vertex_start, edge_start, closed, issues, cancel)
    }

    fn publish_path(
        &mut self,
        start: usize,
        vertex_start: usize,
        edge_start: usize,
        closed: bool,
        mut issues: PathIssues,
        cancel: &mut Cancellation<'_>,
    ) -> Result<()> {
        let vertex_end = self.result.vertices.len();
        let edge_end = self.result.edges.len();
        canonicalize(
            &mut self.result.vertices[vertex_start..vertex_end],
            &mut self.result.edges[edge_start..edge_end],
            closed,
            cancel,
        )?;
        if closed && edge_end - edge_start < 3 {
            issues.add(PathIssue::DegenerateCycle);
        }
        let kind = if !issues.is_empty() {
            PathKind::AmbiguousTrail
        } else if closed {
            PathKind::ClosedCycleCandidate
        } else {
            PathKind::OpenChain
        };
        let component = self.states[start].component;
        let record = self
            .result
            .components
            .get_mut(component)
            .ok_or_else(|| invalid(Some(start), None))?;
        record.paths = add(record.paths, 1)?;
        record.issues.merge(issues);
        let endpoints = if closed {
            None
        } else {
            Some([
                self.result.vertices[vertex_start],
                self.result.vertices[vertex_end - 1],
            ])
        };
        push(
            &mut self.result.paths,
            ContourPath {
                component,
                kind,
                closed,
                issues,
                endpoints,
                vertices: vertex_start..vertex_end,
                edges: edge_start..edge_end,
            },
            self.limits.max_paths,
            PathResource::Paths,
        )
    }

    fn finish(&mut self, cancel: &mut Cancellation<'_>) -> Result<()> {
        cancel.check(Checkpoint::PathsSorting)?;
        self.result.paths.sort_unstable_by_key(|p| {
            (
                self.result.vertices[p.vertices.start],
                self.result.edges[p.edges.start],
            )
        });
        cancel.check(Checkpoint::PathsSorting)?;
        for (id, path) in self.result.paths.iter().enumerate() {
            cancel.poll(Checkpoint::PathsTraversal, id)?;
            for (offset, &edge) in self.result.edges[path.edges.clone()].iter().enumerate() {
                cancel.poll(Checkpoint::PathsTraversal, offset)?;
                self.result.outcomes[edge] = Assignment::Path { path: id, offset };
            }
            let count = match path.kind {
                PathKind::OpenChain => &mut self.result.summary.open_chains,
                PathKind::ClosedCycleCandidate => &mut self.result.summary.cycle_candidates,
                PathKind::AmbiguousTrail => &mut self.result.summary.ambiguous_trails,
            };
            *count = add(*count, 1)?;
            if path.closed {
                self.result.summary.closed_walks = add(self.result.summary.closed_walks, 1)?;
            }
        }
        for (id, state) in self.states.iter().enumerate() {
            cancel.poll(Checkpoint::PathsDiagnostics, id)?;
            let isolated = self.result.graph.vertices()[id].degrees.unique_edges == 0;
            if isolated || !state.issues.is_empty() {
                push(
                    &mut self.result.diagnostics,
                    PathDiagnostic::Vertex {
                        vertex: id,
                        issues: state.issues,
                        isolated,
                    },
                    self.limits.max_diagnostics,
                    PathResource::Diagnostics,
                )?;
            }
        }
        for (id, outcome) in self.result.outcomes.iter().enumerate() {
            cancel.poll(Checkpoint::PathsDiagnostics, id)?;
            if *outcome == Assignment::Pending {
                return Err(invalid(None, Some(id)));
            }
            if *outcome == Assignment::Unresolved {
                push(
                    &mut self.result.diagnostics,
                    PathDiagnostic::UnresolvedEdge {
                        edge: id,
                        issues: self.result.graph.normalized().groups()[id].issues,
                    },
                    self.limits.max_diagnostics,
                    PathResource::Diagnostics,
                )?;
            }
        }
        if self.result.edges.len() != self.result.summary.candidate_edges {
            return Err(invalid(None, None));
        }
        for (id, component) in self.result.components.iter().enumerate() {
            cancel.poll(Checkpoint::PathsDiagnostics, id)?;
            if !component.issues.is_empty() {
                self.result.summary.ambiguous_components =
                    add(self.result.summary.ambiguous_components, 1)?;
            }
        }
        self.result.summary.consumed_edges = self.result.edges.len();
        self.result.summary.paths = self.result.paths.len();
        self.result.summary.path_vertices = self.result.vertices.len();
        self.result.summary.components = self.result.components.len();
        self.result.summary.diagnostics = self.result.diagnostics.len();
        Ok(())
    }
}

fn other_endpoint(edge: GraphEdge, vertex: usize, id: usize) -> Result<usize> {
    if edge.endpoints[0] == vertex {
        Ok(edge.endpoints[1])
    } else if edge.endpoints[1] == vertex {
        Ok(edge.endpoints[0])
    } else {
        Err(invalid(Some(vertex), Some(id)))
    }
}

fn canonicalize(
    vertices: &mut [usize],
    edges: &mut [usize],
    closed: bool,
    cancel: &mut Cancellation<'_>,
) -> Result<()> {
    cancel.check(Checkpoint::PathsCanonicalization)?;
    if edges.is_empty() || vertices.len() != add(edges.len(), usize::from(!closed))? {
        return Err(invalid(None, None));
    }
    if closed {
        let mut minimum = 0;
        for index in 0..vertices.len() {
            cancel.poll(Checkpoint::PathsCanonicalization, index)?;
            if vertices[index] < vertices[minimum] {
                minimum = index;
            }
        }
        cancel.check(Checkpoint::PathsCanonicalization)?;
        vertices.rotate_left(minimum);
        edges.rotate_left(minimum);
        cancel.check(Checkpoint::PathsCanonicalization)?;
        if edges[0] > edges[edges.len() - 1] {
            vertices[1..].reverse();
            edges.reverse();
        }
    } else if vertices[0] > vertices[vertices.len() - 1] {
        vertices.reverse();
        edges.reverse();
    }
    cancel.check(Checkpoint::PathsCanonicalization)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        geometry::{
            section::{EndpointOrigin, TrianglePlaneEvent},
            Point3,
        },
        mesh::TriangleMesh,
        slicing::{
            graph::{build_incidence_graph, GraphLimits},
            normalize::{normalize_section, NormalizationLimits},
            section_mesh_z, SectionLimits, SectionTransform,
        },
    };

    #[test]
    fn cyclic_rotations_and_reverse_directions_have_one_independent_expected_sequence() {
        for shift in 0..4 {
            for reverse in [false, true] {
                let mut vertices = [0, 1, 2, 3];
                let mut edges = [0, 3, 4, 2];
                vertices.rotate_left(shift);
                edges.rotate_left(shift);
                if reverse {
                    vertices[1..].reverse();
                    edges.reverse();
                }
                canonicalize(&mut vertices, &mut edges, true, &mut Cancellation::never()).unwrap();
                assert_eq!(vertices, [0, 1, 2, 3]);
                assert_eq!(edges, [0, 3, 4, 2]);
            }
        }
    }

    #[test]
    fn open_reversal_changes_both_vertex_and_edge_sequences_without_area_tests() {
        let mut vertices = [7, 5, 3, 1];
        let mut edges = [8, 4, 2];
        canonicalize(&mut vertices, &mut edges, false, &mut Cancellation::never()).unwrap();
        assert_eq!(vertices, [1, 3, 5, 7]);
        assert_eq!(edges, [2, 4, 8]);
    }

    #[test]
    fn coordinate_conflict_retains_raw_positions_and_complete_only_outcomes() {
        let mesh = TriangleMesh::try_new(
            vec![
                Point3::new(0., 0., 0.),
                Point3::new(4., 2., -1.),
                Point3::new(4., 2., 1.),
                Point3::new(-4., 2., -1.),
                Point3::new(-4., 2., 1.),
            ],
            vec![[0, 1, 2], [0, 3, 4]],
        )
        .unwrap();
        let mut raw = section_mesh_z(
            &mesh,
            0.,
            SectionTransform::Identity,
            SectionLimits::default(),
        )
        .unwrap();
        let TrianglePlaneEvent::Segment {
            ref mut endpoints, ..
        } = raw.events[1].intersection.event
        else {
            panic!("segment")
        };
        endpoints
            .iter_mut()
            .find(|p| matches!(p.origin, EndpointOrigin::Vertex(_)))
            .unwrap()
            .position
            .x = f64::from_bits(1);
        let before = raw.events.clone();
        let normalized = normalize_section(&raw, NormalizationLimits::default()).unwrap();
        let graph = build_incidence_graph(&normalized, GraphLimits::default()).unwrap();
        let result = extract_paths(&graph, PathLimits::default()).unwrap();
        assert_eq!(result.paths(), []);
        assert_eq!(result.summary().unresolved_edges, 2);
        assert_eq!(result.summary().raw_occurrences, 2);
        assert!(result.diagnostics().iter().any(|d|matches!(d,PathDiagnostic::Vertex {issues,..} if issues.contains(PathIssue::CoordinateConflict))));
        assert_eq!(
            normalized.features()[0].coordinates,
            CoordinateAgreement::Conflict
        );
        assert_eq!(normalized.features()[0].observations.len(), 2);
        assert_eq!(raw.events, before);
    }

    #[test]
    fn arithmetic_and_isize_boundaries_fail_without_allocating() {
        assert_eq!(add(usize::MAX, 1), Err(PathError::SizeOverflow));
        assert_eq!(mul(usize::MAX, 2), Err(PathError::SizeOverflow));
        assert_eq!(bytes::<usize>(usize::MAX), Err(PathError::SizeOverflow));
        assert_eq!(bytes::<u8>(usize::MAX), Err(PathError::SizeOverflow));
        assert_eq!(bytes::<usize>(0), Ok(0));
        assert_eq!(
            reserve::<u8>(usize::MAX, &mut Cancellation::never()),
            Err(PathError::Allocation)
        );
    }

    #[test]
    fn invalid_adjacency_indices_and_order_are_distinct_from_geometry_diagnostics() {
        let edge = GraphEdge {
            group: 0,
            endpoints: [0, 1],
            multiplicity: 1,
            eligibility: EdgeEligibility::Candidate,
        };
        let entry = AdjacentIncidence {
            vertex: 0,
            edge: 0,
            endpoint: 0,
        };
        for bad in [
            AdjacentIncidence {
                edge: usize::MAX,
                ..entry
            },
            AdjacentIncidence {
                endpoint: 2,
                ..entry
            },
            AdjacentIncidence { vertex: 1, ..entry },
        ] {
            assert!(matches!(
                validate_entry(&[edge], 0, bad, None, true),
                Err(PathError::InvalidGraph { .. })
            ));
        }
        assert!(matches!(
            validate_entry(&[edge], 0, entry, Some((0, 0)), true),
            Err(PathError::InvalidGraph { .. })
        ));
        assert!(matches!(
            other_endpoint(edge, 2, 0),
            Err(PathError::InvalidGraph { .. })
        ));
        let excluded = GraphEdge {
            eligibility: EdgeEligibility::Unresolved,
            ..edge
        };
        assert!(matches!(
            validate_entry(&[excluded], 0, entry, None, true),
            Err(PathError::InvalidGraph { .. })
        ));
    }

    #[test]
    fn inconsistent_path_buffer_shapes_are_typed_failures() {
        assert!(matches!(
            canonicalize(&mut [], &mut [], true, &mut Cancellation::never()),
            Err(PathError::InvalidGraph { .. })
        ));
        assert!(matches!(
            canonicalize(&mut [0, 1], &mut [0, 1], false, &mut Cancellation::never()),
            Err(PathError::InvalidGraph { .. })
        ));
    }

    #[test]
    fn fallible_push_never_grows_beyond_checked_preallocation() {
        assert_eq!(
            push(&mut Vec::new(), 0, 1, PathResource::PathEdges),
            Err(PathError::SizeOverflow)
        );
        assert_eq!(
            push(&mut Vec::new(), 0, 0, PathResource::PathEdges),
            Err(PathError::LimitExceeded {
                resource: PathResource::PathEdges,
                limit: 0
            })
        );
    }
}
