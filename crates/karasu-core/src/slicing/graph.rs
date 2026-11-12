//! Deterministic source-feature incidence graphs, not extracted or accepted contours (TD-021).
//!
//! Complete adjacency retains every normalized group, including unresolved and internal groups.
//! Candidate adjacency contains only locally eligible groups; neither view certifies filled material.
//! Multiplicity is borrowed losslessly from normalization rather than expanded into heap objects.
//!
//! ```
//! use karasu_core::{mesh::TriangleMesh, slicing::{section_mesh_z, SectionLimits, SectionTransform}};
//! use karasu_core::slicing::normalize::{normalize_section, NormalizationLimits};
//! use karasu_core::slicing::graph::{build_incidence_graph, GraphLimits};
//! let mesh = TriangleMesh::try_new(vec![], vec![])?;
//! let raw = section_mesh_z(&mesh, 0.0, SectionTransform::Identity, SectionLimits::default())?;
//! let normalized = normalize_section(&raw, NormalizationLimits::default())?;
//! let graph = build_incidence_graph(&normalized, GraphLimits::default())?;
//! assert_eq!(graph.summary().vertices, 0);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use super::normalize::{CoordinateAgreement, IncidenceIssues, NormalizedSection, Ownership};
use crate::cancellation::{Cancellation, Cancelled, Checkpoint};
use std::{error::Error, fmt, ops::Range};

/// Local graph eligibility, independent of contour/solid validity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EdgeEligibility {
    /// One M3.2-B crossing or verified local coplanar patch-boundary group.
    Candidate,
    /// Verified local patch-internal group, retained only in complete adjacency.
    InternalCoplanar,
    /// Unresolved ownership, collapse, numerical conflict or other M3.2-B ambiguity.
    Unresolved,
}

/// Complete counts at a source-feature vertex; loops have two incidence ends but one unique edge.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VertexDegrees {
    /// Different complete graph edge/group IDs, including internal and unresolved groups.
    pub unique_edges: usize,
    /// Different locally eligible edge/group IDs, not raw multiplicity.
    pub eligible_edges: usize,
    /// Candidate adjacency ends; a loop would count twice, not once.
    pub eligible_incidence_degree: usize,
    /// Sum of all raw occurrence ends, including both ends of a loop and all duplicates.
    pub raw_occurrence_ends: usize,
    /// Different unresolved edge/group IDs touching this vertex.
    pub unresolved_edges: usize,
}

/// Canonical vertex. Its ID is its index in `vertices()`, equal to the normalized feature-group ID.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GraphVertex {
    /// Index into `normalized().features()`; that record retains all positions and observations.
    pub feature_group: usize,
    /// Local degree and raw multiplicity information; degree two is not contour acceptance.
    pub degrees: VertexDegrees,
    complete: Range<usize>,
    candidates: Range<usize>,
}

/// One canonical feature-pair group with losslessly retained parallel raw incidence references.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GraphEdge {
    /// Index into `normalized().groups()`, also this graph edge's ID.
    pub group: usize,
    /// Vertex IDs in canonical source-feature order, including possible self-loops.
    pub endpoints: [usize; 2],
    /// Number of raw segment occurrences, not a parity or unique-edge count.
    pub multiplicity: usize,
    /// Explicit eligibility; unresolved groups never enter candidate adjacency.
    pub eligibility: EdgeEligibility,
}

/// One endpoint slot of a graph group. Two entries remain for a self-loop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AdjacentIncidence {
    /// Vertex owning this adjacency entry.
    pub vertex: usize,
    /// Graph edge/group ID, with raw occurrences available through `occurrences()`.
    pub edge: usize,
    /// Canonical endpoint slot, either 0 or 1, not source winding.
    pub endpoint: usize,
}

/// Structured diagnostics ordered by phase: feature conflicts, unresolved groups, then degrees.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GraphDiagnostic {
    /// No point representative or unambiguous connection is chosen for this source feature.
    CoordinateConflict {
        /// Canonical vertex ID; all conflicting observations remain in normalization.
        vertex: usize,
    },
    /// Inspect retained occurrences and the original ownership issues; not an ordinary candidate.
    UnresolvedEdge {
        /// Canonical graph edge/group ID.
        edge: usize,
        /// Unchanged independent M3.2-B issue bits.
        issues: IncidenceIssues,
    },
    /// Candidate degree one only; unresolved incidences may also touch this vertex.
    PotentialEndpoint {
        /// Canonical vertex ID.
        vertex: usize,
    },
    /// Candidate degree greater than two; no branch routing is selected.
    PotentialBranch {
        /// Canonical vertex ID.
        vertex: usize,
        /// Candidate incidence-end degree.
        degree: usize,
    },
}

/// Independent count and requested-buffer limits; borrowed input and allocator overhead are excluded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GraphLimits {
    /// All feature vertices, including tangent-only isolated ones.
    pub max_vertices: usize,
    /// All normalized feature-pair groups, not just candidates.
    pub max_edges: usize,
    /// Logical raw segment references retained by borrowing `segment_order()`.
    pub max_incidence_references: usize,
    /// Sum of complete and candidate adjacency entries (two ends per group per applicable view).
    pub max_adjacency_entries: usize,
    /// Actual structured diagnostic count, checked before each insertion.
    pub max_diagnostics: usize,
    /// Aggregate requested capacities of five owned buffers; not total RSS or borrowed data size.
    pub max_payload_bytes: usize,
}

impl Default for GraphLimits {
    fn default() -> Self {
        Self {
            max_vertices: 3_000_000,
            max_edges: 3_000_000,
            max_incidence_references: 3_000_000,
            max_adjacency_entries: 12_000_000,
            max_diagnostics: 9_000_000,
            max_payload_bytes: 512 * 1024 * 1024,
        }
    }
}

/// Resource whose configured graph cap was exceeded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GraphResource {
    /// Source-feature vertices.
    Vertices,
    /// Canonical complete edge/group records.
    Edges,
    /// Borrowed raw occurrence references.
    IncidenceReferences,
    /// Complete plus candidate adjacency ends.
    AdjacencyEntries,
    /// Actual structured diagnostic records.
    Diagnostics,
    /// Aggregate requested owned buffer capacities in bytes.
    PayloadBytes,
}

/// Fatal construction failure; representable geometry ambiguity and finite conflicts are diagnostics.
#[derive(Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum GraphError {
    /// Observed synchronous cancellation; no successful partial graph escapes.
    Cancelled(Cancelled),
    /// A count or aggregate byte cap was exceeded.
    LimitExceeded {
        /// Which cap failed.
        resource: GraphResource,
        /// Caller-configured maximum.
        limit: usize,
    },
    /// Count, capacity, or isize byte-range arithmetic cannot be represented.
    SizeOverflow,
    /// Fallible graph buffer reservation failed.
    Allocation,
}

impl fmt::Display for GraphError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled(error) => error.fmt(f),
            Self::LimitExceeded { resource, limit } => {
                write!(f, "incidence graph {resource:?} limit exceeded ({limit})")
            }
            Self::SizeOverflow => {
                write!(f, "incidence graph count or capacity arithmetic overflow")
            }
            Self::Allocation => write!(f, "unable to reserve incidence graph buffers"),
        }
    }
}

impl Error for GraphError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Cancelled(error) => Some(error),
            _ => None,
        }
    }
}

impl From<Cancelled> for GraphError {
    fn from(error: Cancelled) -> Self {
        Self::Cancelled(error)
    }
}

/// Deterministic group/degree totals, not extracted component, path, region or printability counts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GraphSummary {
    /// All source features, including isolated tangent points.
    pub vertices: usize,
    /// All complete feature-pair groups.
    pub edges: usize,
    /// Borrowed raw segment occurrences, including parallel/duplicate/internal contributions.
    pub occurrences: usize,
    /// Two endpoint entries per complete edge, including loops.
    pub complete_adjacency_entries: usize,
    /// Locally eligible groups only, not accepted contour edges.
    pub candidate_edges: usize,
    /// Two endpoint entries per candidate edge.
    pub candidate_adjacency_entries: usize,
    /// Verified internal coplanar groups retained outside candidate adjacency.
    pub internal_edges: usize,
    /// Groups with unresolved ownership or geometry.
    pub unresolved_edges: usize,
    /// Feature identities without an agreed coordinate.
    pub conflicted_vertices: usize,
    /// Vertices without any complete segment incidence (tangent-only observations).
    pub isolated_vertices: usize,
    /// Vertices with candidate incidence degree one, irrespective of other uncertainty.
    pub potential_endpoints: usize,
    /// Vertices with candidate incidence degree greater than two.
    pub potential_branches: usize,
    /// Actual structured diagnostics.
    pub diagnostics: usize,
    /// Aggregate requested capacities; excludes source/normalization/allocator overhead.
    pub requested_payload_bytes: usize,
}

/// Immutable graph borrowing normalized input, which in turn borrows the unchanged raw section.
/// IDs are result-local indices, not persistent serialized identities. No source mesh is rescanned.
#[derive(Debug, PartialEq)]
pub struct IncidenceGraph<'n, 'r> {
    normalized: &'n NormalizedSection<'r>,
    vertices: Vec<GraphVertex>,
    edges: Vec<GraphEdge>,
    complete: Vec<AdjacentIncidence>,
    candidates: Vec<AdjacentIncidence>,
    diagnostics: Vec<GraphDiagnostic>,
    summary: GraphSummary,
}

impl<'n, 'r> IncidenceGraph<'n, 'r> {
    /// Exactly the borrowed normalized input, including every observation and raw event.
    #[must_use]
    pub const fn normalized(&self) -> &'n NormalizedSection<'r> {
        self.normalized
    }
    /// Vertices in lexicographic source-feature order; coordinate equality does not join features.
    #[must_use]
    pub fn vertices(&self) -> &[GraphVertex] {
        &self.vertices
    }
    /// Complete group records in canonical endpoint-feature order, including excluded groups.
    #[must_use]
    pub fn edges(&self) -> &[GraphEdge] {
        &self.edges
    }
    /// Complete incident ends ordered by edge ID then endpoint slot, or None for an invalid vertex.
    #[must_use]
    pub fn complete_adjacency(&self, vertex: usize) -> Option<&[AdjacentIncidence]> {
        self.vertices
            .get(vertex)
            .map(|v| &self.complete[v.complete.clone()])
    }
    /// Locally eligible ends only, in the same order; never includes unresolved/internal groups.
    #[must_use]
    pub fn candidate_adjacency(&self, vertex: usize) -> Option<&[AdjacentIncidence]> {
        self.vertices
            .get(vertex)
            .map(|v| &self.candidates[v.candidates.clone()])
    }
    /// Borrowed normalized segment occurrence IDs for an edge. Resolve through `segments()` and
    /// its observation/raw-event IDs; duplicates, local origins and source winding remain intact.
    #[must_use]
    pub fn occurrences(&self, edge: usize) -> Option<&[usize]> {
        self.edges.get(edge).map(|e| {
            &self.normalized.segment_order()[self.normalized.groups()[e.group].occurrences.clone()]
        })
    }
    /// Ordered machine-readable issues and potential degree endpoints/branches.
    #[must_use]
    pub fn diagnostics(&self) -> &[GraphDiagnostic] {
        &self.diagnostics
    }
    /// Complete deterministic totals, including requested owned payload bytes.
    #[must_use]
    pub const fn summary(&self) -> GraphSummary {
        self.summary
    }
}

type Result<T> = std::result::Result<T, GraphError>;

/// Builds complete and locally eligible adjacency without welding, path selection or repair.
/// # Errors
/// Returns checked count/byte, allocation, or cancellation failure; finite conflicts remain data.
pub fn build_incidence_graph<'n, 'r>(
    normalized: &'n NormalizedSection<'r>,
    limits: GraphLimits,
) -> Result<IncidenceGraph<'n, 'r>> {
    build_incidence_graph_with_cancellation(normalized, limits, &mut Cancellation::never())
}

/// Builds canonical group arrays and flat sorted adjacency ranges (CSR-like, no per-incidence heap).
/// Only ordinary/on-plane crossing candidates and local coplanar patch boundaries enter candidate adjacency. Other incidence
/// and finite-coordinate conflicts remain explicit. Feature identities with conflicts have no chosen
/// point; a degree-two vertex with unresolved incidence is not an unambiguous contour connection.
///
/// For V features, G groups, C eligible groups, A=2G+2C adjacency entries and D diagnostics:
/// O(V+G log(V+1)+A log(A+1)+D) time, O(V+G+A+D) requested storage. Raw S occurrence references
/// remain borrowed; no O(S) copy, sort or mesh rescan. Entry, 256-record loops, reservation/sort
/// boundaries and final publication poll cancellation. Allocation, individual in-place sorts and
/// destruction are indivisible; this is not a progress, latency, serialization or RSS contract.
/// # Errors
/// Cancellation wins at its observed checkpoint. Otherwise preflight count/size caps precede byte
/// reservation, record/adjacency construction, actual diagnostic caps and final publication. No
/// successful partial graph escapes. Only private-constructor normalized results are accepted, so
/// no new deserialization validation/error domain or alternate ownership classifier is introduced.
pub fn build_incidence_graph_with_cancellation<'n, 'r>(
    normalized: &'n NormalizedSection<'r>,
    limits: GraphLimits,
    cancel: &mut Cancellation<'_>,
) -> Result<IncidenceGraph<'n, 'r>> {
    cancel.check(Checkpoint::GraphStart)?;
    let mut summary = preflight(normalized, limits, cancel)?;
    let diagnostic_capacity =
        add(mul(summary.vertices, 2)?, summary.unresolved_edges)?.min(limits.max_diagnostics);
    summary.requested_payload_bytes = [
        bytes::<GraphVertex>(summary.vertices)?,
        bytes::<GraphEdge>(summary.edges)?,
        bytes::<AdjacentIncidence>(summary.complete_adjacency_entries)?,
        bytes::<AdjacentIncidence>(summary.candidate_adjacency_entries)?,
        bytes::<GraphDiagnostic>(diagnostic_capacity)?,
    ]
    .into_iter()
    .try_fold(0, add)?;
    cap(
        summary.requested_payload_bytes,
        limits.max_payload_bytes,
        GraphResource::PayloadBytes,
    )?;
    let mut graph = IncidenceGraph {
        normalized,
        vertices: reserve(summary.vertices, cancel)?,
        edges: reserve(summary.edges, cancel)?,
        complete: reserve(summary.complete_adjacency_entries, cancel)?,
        candidates: reserve(summary.candidate_adjacency_entries, cancel)?,
        diagnostics: reserve(diagnostic_capacity, cancel)?,
        summary,
    };
    assemble(&mut graph, cancel)?;
    adjacency(&mut graph, cancel)?;
    diagnose(&mut graph, limits, cancel)?;
    cancel.check(Checkpoint::GraphComplete)?;
    Ok(graph)
}

fn eligibility(ownership: Ownership) -> EdgeEligibility {
    match ownership {
        Ownership::CrossingCandidate
        | Ownership::OnPlaneCrossingCandidate
        | Ownership::CoplanarPatchBoundary => EdgeEligibility::Candidate,
        Ownership::CoplanarPatchInternal => EdgeEligibility::InternalCoplanar,
        Ownership::Unresolved => EdgeEligibility::Unresolved,
    }
}

fn preflight(
    input: &NormalizedSection<'_>,
    limits: GraphLimits,
    cancel: &mut Cancellation<'_>,
) -> Result<GraphSummary> {
    let mut summary = GraphSummary {
        vertices: input.features().len(),
        edges: input.groups().len(),
        occurrences: input.segments().len(),
        ..GraphSummary::default()
    };
    cap(
        summary.vertices,
        limits.max_vertices,
        GraphResource::Vertices,
    )?;
    cap(summary.edges, limits.max_edges, GraphResource::Edges)?;
    cap(
        summary.occurrences,
        limits.max_incidence_references,
        GraphResource::IncidenceReferences,
    )?;
    for (i, group) in input.groups().iter().enumerate() {
        cancel.poll(Checkpoint::GraphRecords, i)?;
        let counter = match eligibility(group.ownership) {
            EdgeEligibility::Candidate => &mut summary.candidate_edges,
            EdgeEligibility::InternalCoplanar => &mut summary.internal_edges,
            EdgeEligibility::Unresolved => &mut summary.unresolved_edges,
        };
        *counter = add(*counter, 1)?;
    }
    summary.complete_adjacency_entries = mul(summary.edges, 2)?;
    summary.candidate_adjacency_entries = mul(summary.candidate_edges, 2)?;
    cap(
        add(
            summary.complete_adjacency_entries,
            summary.candidate_adjacency_entries,
        )?,
        limits.max_adjacency_entries,
        GraphResource::AdjacencyEntries,
    )?;
    Ok(summary)
}

fn assemble(graph: &mut IncidenceGraph<'_, '_>, cancel: &mut Cancellation<'_>) -> Result<()> {
    for (id, _) in graph.normalized.features().iter().enumerate() {
        cancel.poll(Checkpoint::GraphRecords, id)?;
        graph.vertices.push(GraphVertex {
            feature_group: id,
            degrees: VertexDegrees::default(),
            complete: 0..0,
            candidates: 0..0,
        });
    }
    for (id, group) in graph.normalized.groups().iter().enumerate() {
        cancel.poll(Checkpoint::GraphRecords, id)?;
        // Private normalized construction guarantees sorted feature groups and every endpoint key.
        let endpoints = group.endpoints.map(|feature| {
            graph
                .normalized
                .features()
                .binary_search_by_key(&feature, |g| g.feature)
                .expect("normalized group feature exists")
        });
        graph.edges.push(GraphEdge {
            group: id,
            endpoints,
            multiplicity: group.occurrences.len(),
            eligibility: eligibility(group.ownership),
        });
    }
    Ok(())
}

fn adjacency(graph: &mut IncidenceGraph<'_, '_>, cancel: &mut Cancellation<'_>) -> Result<()> {
    for (id, edge) in graph.edges.iter().enumerate() {
        cancel.poll(Checkpoint::GraphAdjacency, id)?;
        for endpoint in 0..2 {
            let entry = AdjacentIncidence {
                vertex: edge.endpoints[endpoint],
                edge: id,
                endpoint,
            };
            graph.complete.push(entry);
            if edge.eligibility == EdgeEligibility::Candidate {
                graph.candidates.push(entry);
            }
        }
    }
    for buffer in [&mut graph.complete, &mut graph.candidates] {
        cancel.check(Checkpoint::GraphSorting)?;
        buffer.sort_unstable_by_key(|entry| (entry.vertex, entry.edge, entry.endpoint));
        cancel.check(Checkpoint::GraphSorting)?;
    }
    let mut complete = 0;
    let mut candidates = 0;
    for (id, vertex) in graph.vertices.iter_mut().enumerate() {
        cancel.poll(Checkpoint::GraphAdjacency, id)?;
        let start = complete;
        let mut previous = None;
        while complete < graph.complete.len() && graph.complete[complete].vertex == id {
            cancel.poll(Checkpoint::GraphAdjacency, complete)?;
            let edge_id = graph.complete[complete].edge;
            let edge = &graph.edges[edge_id];
            vertex.degrees.raw_occurrence_ends =
                add(vertex.degrees.raw_occurrence_ends, edge.multiplicity)?;
            if previous != Some(edge_id) {
                vertex.degrees.unique_edges = add(vertex.degrees.unique_edges, 1)?;
                if edge.eligibility == EdgeEligibility::Candidate {
                    vertex.degrees.eligible_edges = add(vertex.degrees.eligible_edges, 1)?;
                }
                if edge.eligibility == EdgeEligibility::Unresolved {
                    vertex.degrees.unresolved_edges = add(vertex.degrees.unresolved_edges, 1)?;
                }
            }
            previous = Some(edge_id);
            complete += 1;
        }
        vertex.complete = start..complete;
        let start = candidates;
        while candidates < graph.candidates.len() && graph.candidates[candidates].vertex == id {
            cancel.poll(Checkpoint::GraphAdjacency, candidates)?;
            candidates += 1;
        }
        vertex.candidates = start..candidates;
        vertex.degrees.eligible_incidence_degree = candidates - start;
    }
    Ok(())
}

fn diagnose(
    graph: &mut IncidenceGraph<'_, '_>,
    limits: GraphLimits,
    cancel: &mut Cancellation<'_>,
) -> Result<()> {
    for id in 0..graph.vertices.len() {
        cancel.poll(Checkpoint::GraphDiagnostics, id)?;
        if matches!(
            graph.normalized.features()[id].coordinates,
            CoordinateAgreement::Conflict
        ) {
            graph.summary.conflicted_vertices = add(graph.summary.conflicted_vertices, 1)?;
            diagnostic(
                graph,
                GraphDiagnostic::CoordinateConflict { vertex: id },
                limits,
            )?;
        }
    }
    for id in 0..graph.edges.len() {
        cancel.poll(Checkpoint::GraphDiagnostics, id)?;
        if graph.edges[id].eligibility == EdgeEligibility::Unresolved {
            diagnostic(
                graph,
                GraphDiagnostic::UnresolvedEdge {
                    edge: id,
                    issues: graph.normalized.groups()[id].issues,
                },
                limits,
            )?;
        }
    }
    for id in 0..graph.vertices.len() {
        cancel.poll(Checkpoint::GraphDiagnostics, id)?;
        let degrees = graph.vertices[id].degrees;
        if degrees.unique_edges == 0 {
            graph.summary.isolated_vertices = add(graph.summary.isolated_vertices, 1)?;
        }
        if degrees.eligible_incidence_degree == 1 {
            graph.summary.potential_endpoints = add(graph.summary.potential_endpoints, 1)?;
            diagnostic(
                graph,
                GraphDiagnostic::PotentialEndpoint { vertex: id },
                limits,
            )?;
        } else if degrees.eligible_incidence_degree > 2 {
            graph.summary.potential_branches = add(graph.summary.potential_branches, 1)?;
            diagnostic(
                graph,
                GraphDiagnostic::PotentialBranch {
                    vertex: id,
                    degree: degrees.eligible_incidence_degree,
                },
                limits,
            )?;
        }
    }
    graph.summary.diagnostics = graph.diagnostics.len();
    Ok(())
}

fn diagnostic(
    graph: &mut IncidenceGraph<'_, '_>,
    record: GraphDiagnostic,
    limits: GraphLimits,
) -> Result<()> {
    cap(
        add(graph.diagnostics.len(), 1)?,
        limits.max_diagnostics,
        GraphResource::Diagnostics,
    )?;
    // Capacity is min(2V + unresolved groups, diagnostic cap); each vertex emits at most two.
    graph.diagnostics.push(record);
    Ok(())
}

fn add(a: usize, b: usize) -> Result<usize> {
    a.checked_add(b).ok_or(GraphError::SizeOverflow)
}
fn mul(a: usize, b: usize) -> Result<usize> {
    a.checked_mul(b).ok_or(GraphError::SizeOverflow)
}
fn bytes<T>(n: usize) -> Result<usize> {
    let value = mul(n, size_of::<T>())?;
    isize::try_from(value).map_err(|_| GraphError::SizeOverflow)?;
    Ok(value)
}
fn cap(value: usize, limit: usize, resource: GraphResource) -> Result<()> {
    if value > limit {
        Err(GraphError::LimitExceeded { resource, limit })
    } else {
        Ok(())
    }
}
fn reserve<T>(count: usize, cancel: &mut Cancellation<'_>) -> Result<Vec<T>> {
    let mut buffer = Vec::new();
    if count != 0 {
        cancel.check(Checkpoint::GraphStorage)?;
        buffer
            .try_reserve_exact(count)
            .map_err(|_| GraphError::Allocation)?;
        cancel.check(Checkpoint::GraphStorage)?;
    }
    Ok(buffer)
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
            normalize::{normalize_section, NormalizationLimits},
            section_mesh_z, SectionLimits, SectionTransform,
        },
    };

    #[test]
    fn finite_shared_feature_conflict_has_no_representative_or_candidate_connection() {
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
        assert_eq!(
            normalized.features()[0].coordinates,
            CoordinateAgreement::Conflict
        );
        assert_eq!(normalized.features()[0].observations.len(), 2);
        assert_eq!(graph.summary().conflicted_vertices, 1);
        assert_eq!(graph.summary().candidate_edges, 0);
        assert_eq!(graph.summary().unresolved_edges, 2);
        assert_eq!(graph.vertices()[0].degrees.unique_edges, 2);
        assert_eq!(graph.vertices()[0].degrees.eligible_incidence_degree, 0);
        assert_eq!(
            graph.diagnostics().first(),
            Some(&GraphDiagnostic::CoordinateConflict { vertex: 0 })
        );
        assert!(graph.diagnostics()[1..]
            .iter()
            .all(|d| matches!(d, GraphDiagnostic::UnresolvedEdge { .. })));
        assert_eq!(
            graph.occurrences(0).unwrap().len() + graph.occurrences(1).unwrap().len(),
            2
        );
        assert_eq!(raw.events, before);
    }

    #[test]
    fn checked_count_and_payload_arithmetic_rejects_overflow_without_allocation() {
        assert_eq!(add(usize::MAX, 1), Err(GraphError::SizeOverflow));
        assert_eq!(mul(usize::MAX, 2), Err(GraphError::SizeOverflow));
        assert_eq!(
            bytes::<GraphVertex>(usize::MAX),
            Err(GraphError::SizeOverflow)
        );
        assert_eq!(bytes::<u8>(usize::MAX), Err(GraphError::SizeOverflow));
        assert_eq!(
            bytes::<u64>(usize::try_from(isize::MAX).unwrap()),
            Err(GraphError::SizeOverflow)
        );
        assert_eq!(bytes::<GraphEdge>(0), Ok(0));
    }

    #[test]
    fn conflicting_shared_edge_interiors_retain_both_positions_and_source_faces() {
        use crate::slicing::normalize::SourceFeature;
        let mesh = TriangleMesh::try_new(
            vec![
                Point3::new(0., 0., -1.),
                Point3::new(4., 0., 1.),
                Point3::new(0., 4., 1.),
                Point3::new(0., -4., 1.),
            ],
            vec![[0, 1, 2], [1, 0, 3]],
        )
        .unwrap();
        let mut raw = section_mesh_z(
            &mesh,
            0.,
            SectionTransform::Identity,
            SectionLimits::default(),
        )
        .unwrap();
        let record = raw.events[1];
        let TrianglePlaneEvent::Segment {
            ref mut endpoints, ..
        } = raw.events[1].intersection.event
        else {
            panic!("segment")
        };
        let endpoint = endpoints
            .iter_mut()
            .find(|p| match p.origin {
                EndpointOrigin::Edge(edge) => {
                    let mut pair = record.source_edge(edge);
                    pair.sort_unstable();
                    pair == [0, 1]
                }
                EndpointOrigin::Vertex(_) => false,
            })
            .unwrap();
        endpoint.position.x = f64::from_bits(endpoint.position.x.to_bits() + 1);
        let normalized = normalize_section(&raw, NormalizationLimits::default()).unwrap();
        let graph = build_incidence_graph(&normalized, GraphLimits::default()).unwrap();
        let vertex = normalized
            .features()
            .iter()
            .position(|v| v.feature == SourceFeature::Edge([0, 1]))
            .unwrap();
        assert_eq!(
            normalized.features()[vertex].coordinates,
            CoordinateAgreement::Conflict
        );
        assert_eq!(graph.complete_adjacency(vertex).unwrap().len(), 2);
        assert_eq!(graph.candidate_adjacency(vertex), Some([].as_slice()));
        assert_eq!(graph.summary().occurrences, 2);
        assert_eq!(graph.summary().conflicted_vertices, 1);
    }

    #[test]
    fn impossible_reservation_is_typed_not_a_panic() {
        assert_eq!(
            reserve::<u8>(usize::MAX, &mut Cancellation::never()),
            Err(GraphError::Allocation)
        );
    }

    #[test]
    fn paths_reject_inconsistent_adjacency_without_changing_production_graph_construction() {
        use crate::slicing::paths::{extract_paths, PathError, PathLimits};
        let mesh = TriangleMesh::try_new(
            vec![
                Point3::new(0., 0., -1.),
                Point3::new(4., 0., 1.),
                Point3::new(0., 4., 1.),
            ],
            vec![[0, 1, 2]],
        )
        .unwrap();
        let raw = section_mesh_z(
            &mesh,
            0.,
            SectionTransform::Identity,
            SectionLimits::default(),
        )
        .unwrap();
        let normalized = normalize_section(&raw, NormalizationLimits::default()).unwrap();
        for corruption in 0..4 {
            let mut graph = build_incidence_graph(&normalized, GraphLimits::default()).unwrap();
            match corruption {
                0 => graph.complete[0].endpoint = usize::MAX,
                1 => graph.candidates[0].edge = usize::MAX,
                2 => graph.vertices[0].degrees.eligible_incidence_degree = usize::MAX,
                _ => graph.edges[0].endpoints[1] = usize::MAX,
            }
            assert!(matches!(
                extract_paths(&graph, PathLimits::default()),
                Err(PathError::InvalidGraph { .. })
            ));
        }
    }

    #[test]
    fn paths_do_not_accept_fabricated_parallel_candidate_or_self_loop_reclassification() {
        use crate::slicing::paths::{extract_paths, PathError, PathLimits};
        for faces in [vec![[0, 1, 2], [0, 1, 2]], vec![[0, 0, 1]]] {
            let mesh = TriangleMesh::try_new(
                vec![
                    Point3::new(0., 0., 0.),
                    Point3::new(4., 0., 0.),
                    Point3::new(0., 4., 0.),
                ],
                faces,
            )
            .unwrap();
            let raw = section_mesh_z(
                &mesh,
                0.,
                SectionTransform::Identity,
                SectionLimits::default(),
            )
            .unwrap();
            let normalized = normalize_section(&raw, NormalizationLimits::default()).unwrap();
            let mut graph = build_incidence_graph(&normalized, GraphLimits::default()).unwrap();
            graph.edges[0].eligibility = EdgeEligibility::Candidate;
            assert!(matches!(
                extract_paths(&graph, PathLimits::default()),
                Err(PathError::InvalidGraph { .. })
            ));
        }
    }
}
