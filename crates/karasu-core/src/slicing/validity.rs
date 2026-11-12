//! Detection-only planar embedding inspection (TD-023), not filled-region acceptance.
//!
//! Exact decisions concern represented finite binary64 XY endpoints in millimeters, not ideal
//! geometry before intersection rounding. No epsilon, grid, new intersections, welding, repair,
//! closure, area, winding, nesting or material is introduced. Open/ambiguous paths remain ineligible;
//! their edges still participate in contact testing. Complete-only groups and tangent-only vertices
//! remain explicitly unassessed and accessible through the borrowed input.
//!
//! ```
//! use karasu_core::mesh::TriangleMesh;
//! use karasu_core::slicing::{section_mesh_z, SectionLimits, SectionTransform};
//! use karasu_core::slicing::normalize::{normalize_section, NormalizationLimits};
//! use karasu_core::slicing::graph::{build_incidence_graph, GraphLimits};
//! use karasu_core::slicing::paths::{extract_paths, PathLimits};
//! use karasu_core::slicing::validity::{inspect_planar_paths, ValidityLimits};
//! let mesh = TriangleMesh::try_new(vec![], vec![])?;
//! let raw = section_mesh_z(&mesh, 0.0, SectionTransform::Identity, SectionLimits::default())?;
//! let normalized = normalize_section(&raw, NormalizationLimits::default())?;
//! let graph = build_incidence_graph(&normalized, GraphLimits::default())?;
//! let paths = extract_paths(&graph, PathLimits::default())?;
//! let report = inspect_planar_paths(&paths, ValidityLimits::default())?;
//! assert_eq!(report.embeddings(), []);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use super::exact::orientation;
use super::normalize::CoordinateAgreement;
use super::paths::{EdgeOutcome, PathKind, SectionPaths};
use crate::cancellation::{Cancellation, Cancelled, Checkpoint};
use crate::geometry::Point3;
use std::{cmp::Ordering, error::Error, fmt};

/// Stable reference into the original canonical path; resolve graph/source provenance there.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct PathEdgeRef {
    /// Original path ID.
    pub path: usize,
    /// Original edge offset in that path.
    pub offset: usize,
}

/// Where a singleton contact lies on an oriented path edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContactPosition {
    /// First endpoint in canonical path traversal direction.
    Start,
    /// Second endpoint in canonical path traversal direction.
    End,
    /// Strict interior of a noncollapsed segment.
    Interior,
    /// Both endpoints have identical represented XY.
    Collapsed,
}

/// Exact closed-segment contact; disjoint pairs have no finding. No point is constructed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContactKind {
    /// Noncollinear intersection strictly interior to both segments.
    ProperCrossing,
    /// Noncollinear endpoint/endpoint contact, in first/second edge order.
    EndpointContact([ContactPosition; 2]),
    /// Noncollinear endpoint/interior contact, in first/second edge order.
    EndpointInterior([ContactPosition; 2]),
    /// Collinear singleton, in first/second edge order.
    CollinearPoint([ContactPosition; 2]),
    /// Collinear intersection has positive length but unequal unordered endpoints.
    PositiveOverlap,
    /// Equal unordered endpoint positions, including reversed direction.
    Coincident,
    /// Point/segment or point/point contact; collapsed inputs remain explicit.
    Collapsed([ContactPosition; 2]),
}

/// Findings are ordered: collapsed edges first, then unordered pairs, each lexicographically.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlanarDiagnostic {
    /// A retained path edge has identical XY endpoints; never silently deleted.
    CollapsedEdge(PathEdgeRef),
    /// Every cross-path contact, or a forbidden within-path contact.
    Contact {
        /// Earlier edge reference.
        first: PathEdgeRef,
        /// Later edge reference.
        second: PathEdgeRef,
        /// Exact relation; cross-path contacts do not change per-path simplicity.
        kind: ContactKind,
    },
}

/// Local embedding only, never a polygon-set/material/printability certificate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EmbeddingStatus {
    /// Open or graph-ambiguous input remains ineligible, even if positions happen to close.
    Ineligible,
    /// A graph cycle has collapsed edges or forbidden self-contacts.
    NonSimpleCycle,
    /// A graph cycle has noncollapsed edges and only prescribed adjacent singleton contacts.
    SimpleCycle,
}

/// Per-path findings, indexed by original path ID. Cross-path contacts are intentionally separate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PathEmbedding {
    /// Local status, not material validity.
    pub status: EmbeddingStatus,
    /// Number of collapsed retained edges.
    pub collapsed_edges: usize,
    /// Forbidden unordered self-contact pairs, not intersection-point count.
    pub self_contacts: usize,
}

/// Independent work, output and aggregate requested-capacity caps; no total-RSS promise.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ValidityLimits {
    /// All graph feature vertices, including isolated/complete-only context.
    pub max_graph_vertices: usize,
    /// All complete graph groups.
    pub max_graph_edges: usize,
    /// Original path records.
    pub max_paths: usize,
    /// Sum of original path vertex references.
    pub max_path_vertices: usize,
    /// Sum of original path edge references.
    pub max_path_edges: usize,
    /// Exact contact tests: unordered pairs whose closed represented-XY boxes overlap. Pairs with
    /// disjoint boxes are proven contact-free without a test and do not count.
    pub max_pairs: usize,
    /// Actual collapsed-edge plus forbidden-self/all-cross-path contact findings.
    pub max_diagnostics: usize,
    /// Aggregate requested output and scratch buffer capacities; excludes borrowed input/stack.
    pub max_payload_bytes: usize,
}

impl Default for ValidityLimits {
    fn default() -> Self {
        Self {
            max_graph_vertices: 3_000_000,
            max_graph_edges: 3_000_000,
            max_paths: 3_000_000,
            max_path_vertices: 6_000_000,
            max_path_edges: 3_000_000,
            max_pairs: 1_000_000,
            max_diagnostics: 1_000_000,
            max_payload_bytes: 512 * 1024 * 1024,
        }
    }
}

/// Resource whose configured cap failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValidityResource {
    /// Input graph vertices.
    GraphVertices,
    /// Input complete graph groups.
    GraphEdges,
    /// Input paths.
    Paths,
    /// Input path vertex references.
    PathVertices,
    /// Input path edge references.
    PathEdges,
    /// Potential unordered-pair work.
    Pairs,
    /// Actual finding count.
    Diagnostics,
    /// Aggregate requested capacities, including edge-index scratch.
    PayloadBytes,
}

/// Operational error, distinct from geometric invalidity returned as complete diagnostic data.
#[derive(Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ValidityError {
    /// Observed cancellation; no successful partial report.
    Cancelled(Cancelled),
    /// A configured cap failed.
    LimitExceeded {
        /// Resource that exceeded its cap.
        resource: ValidityResource,
        /// Configured maximum.
        limit: usize,
    },
    /// Checked count/capacity arithmetic or isize allocation range failed.
    SizeOverflow,
    /// Fallible buffer reservation failed.
    Allocation,
    /// Privately constructed path/graph invariants or agreed finite positions were inconsistent.
    InvalidPaths {
        /// Original path ID when known.
        path: Option<usize>,
        /// Original graph group ID when known.
        edge: Option<usize>,
    },
}

impl fmt::Display for ValidityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled(error) => error.fmt(f),
            Self::LimitExceeded { resource, limit } => {
                write!(f, "planar inspection {resource:?} limit exceeded ({limit})")
            }
            Self::SizeOverflow => {
                write!(f, "planar inspection count or capacity arithmetic overflow")
            }
            Self::Allocation => write!(f, "unable to reserve planar inspection buffers"),
            Self::InvalidPaths { path, edge } => write!(
                f,
                "inconsistent planar input at path {path:?}, graph edge {edge:?}"
            ),
        }
    }
}
impl Error for ValidityError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Cancelled(error) => Some(error),
            _ => None,
        }
    }
}
impl From<Cancelled> for ValidityError {
    fn from(error: Cancelled) -> Self {
        Self::Cancelled(error)
    }
}

/// Complete accounting, not accepted material-region counts or progress.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ValiditySummary {
    /// Retained path edges inspected.
    pub path_edges: usize,
    /// All unordered pairs resolved, including allowed adjacency and disjoint pairs.
    pub examined_pairs: usize,
    /// Pairs resolved by an exact contact test because their closed XY boxes overlap; the rest
    /// are proven disjoint by box separation.
    pub tested_pairs: usize,
    /// Actual retained geometric findings.
    pub diagnostics: usize,
    /// Cross-path contact pairs; not intersections after union or repair.
    pub cross_path_contacts: usize,
    /// Complete-only internal groups, not assessed geometrically here.
    pub unassessed_internal_edges: usize,
    /// Complete-only unresolved groups, not assessed geometrically here.
    pub unassessed_unresolved_edges: usize,
    /// Graph vertices with no complete segment incidence, not assessed as path edges.
    pub unassessed_isolated_vertices: usize,
    /// Requested output plus scratch capacities, even after scratch release; not actual RSS.
    pub requested_payload_bytes: usize,
}

/// Immutable report borrowing all path/graph/source provenance without modifying it.
#[derive(Debug, PartialEq)]
pub struct PlanarInspection<'p, 'g, 'n, 'r> {
    input: &'p SectionPaths<'g, 'n, 'r>,
    embeddings: Vec<PathEmbedding>,
    diagnostics: Vec<PlanarDiagnostic>,
    summary: ValiditySummary,
}
impl<'p, 'g, 'n, 'r> PlanarInspection<'p, 'g, 'n, 'r> {
    /// Original immutable paths, graph, complete-only context and raw provenance.
    #[must_use]
    pub const fn paths(&self) -> &'p SectionPaths<'g, 'n, 'r> {
        self.input
    }
    /// Local embedding results in original path-ID order.
    #[must_use]
    pub fn embeddings(&self) -> &[PathEmbedding] {
        &self.embeddings
    }
    /// Collapses then lexicographic pair findings; no silently truncated success.
    #[must_use]
    pub fn diagnostics(&self) -> &[PlanarDiagnostic] {
        &self.diagnostics
    }
    /// Counts, inherited unassessed context and requested storage accounting.
    #[must_use]
    pub const fn summary(&self) -> ValiditySummary {
        self.summary
    }
}

type Result<T> = std::result::Result<T, ValidityError>;
#[derive(Clone, Copy)]
struct Edge {
    reference: PathEdgeRef,
    vertices: [usize; 2],
    points: [Point3; 2],
}
fn invalid(path: Option<usize>, edge: Option<usize>) -> ValidityError {
    ValidityError::InvalidPaths { path, edge }
}
fn add(a: usize, b: usize) -> Result<usize> {
    a.checked_add(b).ok_or(ValidityError::SizeOverflow)
}
fn bytes<T>(count: usize) -> Result<usize> {
    let n = count
        .checked_mul(size_of::<T>())
        .ok_or(ValidityError::SizeOverflow)?;
    isize::try_from(n).map_err(|_| ValidityError::SizeOverflow)?;
    Ok(n)
}
fn pairs(n: usize) -> Result<usize> {
    if n < 2 {
        return Ok(0);
    }
    let (a, b) = if n.is_multiple_of(2) {
        (n / 2, n - 1)
    } else {
        (n, (n - 1) / 2)
    };
    a.checked_mul(b).ok_or(ValidityError::SizeOverflow)
}
fn cap(n: usize, limit: usize, resource: ValidityResource) -> Result<()> {
    if n > limit {
        Err(ValidityError::LimitExceeded { resource, limit })
    } else {
        Ok(())
    }
}
fn reserve<T>(n: usize, cancel: &mut Cancellation<'_>) -> Result<Vec<T>> {
    let mut out = Vec::new();
    if n != 0 {
        cancel.check(Checkpoint::ValidityStorage)?;
        out.try_reserve_exact(n)
            .map_err(|_| ValidityError::Allocation)?;
        cancel.check(Checkpoint::ValidityStorage)?;
    }
    Ok(out)
}

/// Inspect path embeddings without modifying them or interpreting filled material.
/// # Errors
/// Returns typed count/work/byte, arithmetic, allocation, consistency or cancellation failure.
pub fn inspect_planar_paths<'p, 'g, 'n, 'r>(
    input: &'p SectionPaths<'g, 'n, 'r>,
    limits: ValidityLimits,
) -> Result<PlanarInspection<'p, 'g, 'n, 'r>> {
    inspect_planar_paths_with_cancellation(input, limits, &mut Cancellation::never())
}

/// Exact represented-XY detection with canonical pair order and atomic publication.
///
/// Entry cancellation precedes caps. Input caps, coarse reference checks and checked potential-pair count precede
/// reservations; finite endpoint consistency is checked while indexing. Diagnostic caps precede insertion. All pairs count, including disjoint and
/// permitted adjacent pairs. Consecutive edges may share only their prescribed graph endpoint;
/// first/last edges are adjacent only on graph-closed paths. Cross-path contacts never assign roles.
///
/// For graph vertices V/groups G, paths P, references E, box-overlapping pairs K and findings D:
/// O(V+G+P+E log E+S+K log K) time, where S <= E^2 is the X-sweep overlap work, and O(P+E+K+D)
/// requested storage. Defaults are configurable safety policy, not measured performance.
/// Loops poll every 256 records/pairs from zero, including inner pair loops, plus allocation,
/// finding and publication boundaries. Fixed-size exact predicates, allocations and cleanup are
/// indivisible; no progress/latency/RSS guarantee. Complete-only geometry is explicitly unassessed.
/// # Errors
/// No successful partial report escapes. Ordinary errors before the next observed checkpoint retain
/// precedence. Accepts privately constructed paths, not an untrusted path deserialization format.
pub fn inspect_planar_paths_with_cancellation<'p, 'g, 'n, 'r>(
    input: &'p SectionPaths<'g, 'n, 'r>,
    limits: ValidityLimits,
    cancel: &mut Cancellation<'_>,
) -> Result<PlanarInspection<'p, 'g, 'n, 'r>> {
    cancel.check(Checkpoint::ValidityStart)?;
    let mut summary = inspect_input(input, limits, cancel)?;
    let (edges, mut embeddings) = index_edges(input, summary.path_edges, cancel)?;
    let mut findings = Findings {
        records: Vec::new(),
        requested: 0,
        limits,
    };
    for (i, edge) in edges.iter().enumerate() {
        cancel.poll(Checkpoint::ValidityPairs, i)?;
        if equal(edge.points[0], edge.points[1]) {
            let embedding = &mut embeddings[edge.reference.path];
            embedding.collapsed_edges = add(embedding.collapsed_edges, 1)?;
            mark_nonsimple(embedding);
            findings.push(
                PlanarDiagnostic::CollapsedEdge(edge.reference),
                &mut summary,
                cancel,
            )?;
        }
    }
    let candidates = candidate_pairs(&edges, limits, &mut summary, cancel)?;
    for (n, &(i, j)) in candidates.iter().enumerate() {
        cancel.poll(Checkpoint::ValidityPairs, n)?;
        let (first, second) = (&edges[i as usize], &edges[j as usize]);
        {
            if let Some(kind) = contact(first.points, second.points) {
                let same = first.reference.path == second.reference.path;
                if same && allowed_adjacent(input, first, second, kind) {
                    continue;
                }
                if same {
                    let embedding = &mut embeddings[first.reference.path];
                    embedding.self_contacts = add(embedding.self_contacts, 1)?;
                    mark_nonsimple(embedding);
                } else {
                    summary.cross_path_contacts = add(summary.cross_path_contacts, 1)?;
                }
                findings.push(
                    PlanarDiagnostic::Contact {
                        first: first.reference,
                        second: second.reference,
                        kind,
                    },
                    &mut summary,
                    cancel,
                )?;
            }
        }
    }
    summary.examined_pairs = pairs(edges.len())?;
    summary.diagnostics = findings.records.len();
    drop(candidates);
    drop(edges);
    cancel.check(Checkpoint::ValidityComplete)?;
    Ok(PlanarInspection {
        input,
        embeddings,
        diagnostics: findings.records,
        summary,
    })
}

fn inspect_input(
    input: &SectionPaths<'_, '_, '_>,
    limits: ValidityLimits,
    cancel: &mut Cancellation<'_>,
) -> Result<ValiditySummary> {
    let graph = input.graph();
    cap(
        graph.vertices().len(),
        limits.max_graph_vertices,
        ValidityResource::GraphVertices,
    )?;
    cap(
        graph.edges().len(),
        limits.max_graph_edges,
        ValidityResource::GraphEdges,
    )?;
    cap(
        input.paths().len(),
        limits.max_paths,
        ValidityResource::Paths,
    )?;
    let mut summary = ValiditySummary::default();
    for (i, v) in graph.vertices().iter().enumerate() {
        cancel.poll(Checkpoint::ValidityInput, i)?;
        if v.degrees.unique_edges == 0 {
            summary.unassessed_isolated_vertices = add(summary.unassessed_isolated_vertices, 1)?;
        }
    }
    for i in 0..graph.edges().len() {
        cancel.poll(Checkpoint::ValidityInput, i)?;
        match input.edge_outcome(i).ok_or(invalid(None, Some(i)))? {
            EdgeOutcome::InternalCoplanar => {
                summary.unassessed_internal_edges = add(summary.unassessed_internal_edges, 1)?;
            }
            EdgeOutcome::Unresolved => {
                summary.unassessed_unresolved_edges = add(summary.unassessed_unresolved_edges, 1)?;
            }
            EdgeOutcome::Path { path, offset } => {
                if input.path_edges(path).and_then(|e| e.get(offset)) != Some(&i) {
                    return Err(invalid(None, Some(i)));
                }
            }
        }
    }
    let mut vertex_count = 0;
    for (id, path) in input.paths().iter().enumerate() {
        cancel.poll(Checkpoint::ValidityInput, id)?;
        let es = input.path_edges(id).ok_or(invalid(Some(id), None))?;
        let vs = input.path_vertices(id).ok_or(invalid(Some(id), None))?;
        if es.is_empty() || vs.len() != add(es.len(), usize::from(!path.closed))? {
            return Err(invalid(Some(id), None));
        }
        summary.path_edges = add(summary.path_edges, es.len())?;
        vertex_count = add(vertex_count, vs.len())?;
    }
    cap(
        vertex_count,
        limits.max_path_vertices,
        ValidityResource::PathVertices,
    )?;
    cap(
        summary.path_edges,
        limits.max_path_edges,
        ValidityResource::PathEdges,
    )?;
    // Every pair is accounted for; the exact-test budget is enforced while generating candidates.
    pairs(summary.path_edges)?;
    summary.requested_payload_bytes = add(
        bytes::<Edge>(summary.path_edges)?,
        bytes::<PathEmbedding>(input.paths().len())?,
    )?;
    cap(
        summary.requested_payload_bytes,
        limits.max_payload_bytes,
        ValidityResource::PayloadBytes,
    )?;
    Ok(summary)
}

type IndexedEdges = (Vec<Edge>, Vec<PathEmbedding>);

/// Unordered edge pairs `(i, j)`, `i < j`, whose closed represented-XY bounding boxes overlap,
/// in lexicographic order. Closed segments with disjoint boxes cannot touch, so every possible
/// contact is among these pairs and processing them in this order reproduces the exhaustive scan.
/// Generated by a sweep over boxes sorted by minimum X.
fn candidate_pairs(
    edges: &[Edge],
    limits: ValidityLimits,
    summary: &mut ValiditySummary,
    cancel: &mut Cancellation<'_>,
) -> Result<Vec<(u32, u32)>> {
    let count = u32::try_from(edges.len()).map_err(|_| ValidityError::SizeOverflow)?;
    let mut boxes = reserve::<[f64; 4]>(edges.len(), cancel)?;
    boxes.extend(edges.iter().map(|e| {
        let [a, b] = e.points;
        [a.x.min(b.x), a.x.max(b.x), a.y.min(b.y), a.y.max(b.y)]
    }));
    let mut order = reserve::<u32>(edges.len(), cancel)?;
    order.extend(0..count);
    order.sort_unstable_by(|&a, &b| {
        boxes[a as usize][0]
            .total_cmp(&boxes[b as usize][0])
            .then(a.cmp(&b))
    });
    let mut pairs_found: Vec<(u32, u32)> = Vec::new();
    for (position, &a) in order.iter().enumerate() {
        cancel.poll(Checkpoint::ValidityPairs, position)?;
        let ba = boxes[a as usize];
        for &b in &order[position + 1..] {
            let bb = boxes[b as usize];
            if bb[0] > ba[1] {
                break;
            }
            if bb[2] > ba[3] || bb[3] < ba[2] {
                continue;
            }
            let next = add(pairs_found.len(), 1)?;
            cap(next, limits.max_pairs, ValidityResource::Pairs)?;
            if next > pairs_found.capacity() {
                let desired = next.saturating_mul(2).min(limits.max_pairs);
                let total = add(
                    summary.requested_payload_bytes - bytes::<(u32, u32)>(pairs_found.capacity())?,
                    bytes::<(u32, u32)>(desired)?,
                )?;
                cap(
                    total,
                    limits.max_payload_bytes,
                    ValidityResource::PayloadBytes,
                )?;
                pairs_found
                    .try_reserve_exact(desired - pairs_found.len())
                    .map_err(|_| ValidityError::Allocation)?;
                summary.requested_payload_bytes = add(
                    total - bytes::<(u32, u32)>(desired)?,
                    bytes::<(u32, u32)>(pairs_found.capacity())?,
                )?;
            }
            pairs_found.push((a.min(b), a.max(b)));
        }
    }
    pairs_found.sort_unstable();
    summary.tested_pairs = pairs_found.len();
    Ok(pairs_found)
}
fn index_edges(
    input: &SectionPaths<'_, '_, '_>,
    count: usize,
    cancel: &mut Cancellation<'_>,
) -> Result<IndexedEdges> {
    let graph = input.graph();
    let mut edges = reserve::<Edge>(count, cancel)?;
    let mut embeddings = reserve(input.paths().len(), cancel)?;
    for (id, path) in input.paths().iter().enumerate() {
        cancel.poll(Checkpoint::ValidityInput, id)?;
        embeddings.push(PathEmbedding {
            status: if path.kind == PathKind::ClosedCycleCandidate {
                EmbeddingStatus::SimpleCycle
            } else {
                EmbeddingStatus::Ineligible
            },
            collapsed_edges: 0,
            self_contacts: 0,
        });
        let es = input.path_edges(id).ok_or(invalid(Some(id), None))?;
        let vs = input.path_vertices(id).ok_or(invalid(Some(id), None))?;
        for (offset, &edge_id) in es.iter().enumerate() {
            cancel.poll(Checkpoint::ValidityInput, offset)?;
            let endpoints = [vs[offset], vs[(offset + 1) % vs.len()]];
            let edge = graph
                .edges()
                .get(edge_id)
                .ok_or(invalid(Some(id), Some(edge_id)))?;
            if !((edge.endpoints == endpoints) || (edge.endpoints == [endpoints[1], endpoints[0]]))
                || input.edge_outcome(edge_id) != Some(EdgeOutcome::Path { path: id, offset })
            {
                return Err(invalid(Some(id), Some(edge_id)));
            }
            let mut points = [Point3::new(0., 0., 0.); 2];
            for k in 0..2 {
                let v = graph
                    .vertices()
                    .get(endpoints[k])
                    .ok_or(invalid(Some(id), Some(edge_id)))?;
                let feature = graph
                    .normalized()
                    .features()
                    .get(v.feature_group)
                    .ok_or(invalid(Some(id), Some(edge_id)))?;
                let CoordinateAgreement::Agreed(p) = feature.coordinates else {
                    return Err(invalid(Some(id), Some(edge_id)));
                };
                if !p.x.is_finite() || !p.y.is_finite() || !p.z.is_finite() {
                    return Err(invalid(Some(id), Some(edge_id)));
                }
                points[k] = p;
            }
            edges.push(Edge {
                reference: PathEdgeRef { path: id, offset },
                vertices: endpoints,
                points,
            });
        }
    }
    Ok((edges, embeddings))
}

fn mark_nonsimple(path: &mut PathEmbedding) {
    if path.status == EmbeddingStatus::SimpleCycle {
        path.status = EmbeddingStatus::NonSimpleCycle;
    }
}
struct Findings {
    records: Vec<PlanarDiagnostic>,
    requested: usize,
    limits: ValidityLimits,
}
impl Findings {
    fn push(
        &mut self,
        record: PlanarDiagnostic,
        summary: &mut ValiditySummary,
        cancel: &mut Cancellation<'_>,
    ) -> Result<()> {
        cancel.check(Checkpoint::ValidityDiagnostics)?;
        let required = add(self.records.len(), 1)?;
        cap(
            required,
            self.limits.max_diagnostics,
            ValidityResource::Diagnostics,
        )?;
        if required > self.requested {
            let old = bytes::<PlanarDiagnostic>(self.requested)?;
            let base = summary.requested_payload_bytes - old;
            let available = (self.limits.max_payload_bytes - base) / size_of::<PlanarDiagnostic>();
            let desired = self
                .requested
                .saturating_mul(2)
                .max(1)
                .min(self.limits.max_diagnostics)
                .min(available);
            if desired < required {
                return Err(ValidityError::LimitExceeded {
                    resource: ValidityResource::PayloadBytes,
                    limit: self.limits.max_payload_bytes,
                });
            }
            let total = add(base, bytes::<PlanarDiagnostic>(desired)?)?;
            cancel.check(Checkpoint::ValidityStorage)?;
            self.records
                .try_reserve_exact(desired - self.records.len())
                .map_err(|_| ValidityError::Allocation)?;
            cancel.check(Checkpoint::ValidityStorage)?;
            self.requested = desired;
            summary.requested_payload_bytes = total;
        }
        self.records.push(record);
        Ok(())
    }
}

#[allow(clippy::float_cmp)] // Equality/ordering of represented XY, deliberately not proximity.
fn equal(a: Point3, b: Point3) -> bool {
    a.x == b.x && a.y == b.y
}
fn on(p: Point3, segment: [Point3; 2]) -> bool {
    let [a, b] = segment;
    orientation(a, b, p) == Ordering::Equal
        && p.x >= a.x.min(b.x)
        && p.x <= a.x.max(b.x)
        && p.y >= a.y.min(b.y)
        && p.y <= a.y.max(b.y)
}
fn position(p: Point3, [a, b]: [Point3; 2]) -> ContactPosition {
    if equal(a, b) {
        ContactPosition::Collapsed
    } else if equal(p, a) {
        ContactPosition::Start
    } else if equal(p, b) {
        ContactPosition::End
    } else {
        ContactPosition::Interior
    }
}
fn contact(first: [Point3; 2], second: [Point3; 2]) -> Option<ContactKind> {
    let [a, b] = first;
    let [c, d] = second;
    if equal(a, b) || equal(c, d) {
        let point = if equal(a, b) { a } else { c };
        return (on(point, first) && on(point, second))
            .then(|| ContactKind::Collapsed([position(point, first), position(point, second)]));
    }
    // Strict closed-AABB rejection uses only finite endpoint comparisons, never rounded differences.
    if a.x.max(b.x) < c.x.min(d.x)
        || c.x.max(d.x) < a.x.min(b.x)
        || a.y.max(b.y) < c.y.min(d.y)
        || c.y.max(d.y) < a.y.min(b.y)
    {
        return None;
    }
    let signs = [
        orientation(a, b, c),
        orientation(a, b, d),
        orientation(c, d, a),
        orientation(c, d, b),
    ];
    let collinear = signs.iter().all(|s| *s == Ordering::Equal);
    if collinear {
        let [aa, bb, cc, dd] = if a.x.partial_cmp(&b.x) == Some(Ordering::Equal) {
            [a.y, b.y, c.y, d.y]
        } else {
            [a.x, b.x, c.x, d.x]
        };
        let low = aa.min(bb).max(cc.min(dd));
        let high = aa.max(bb).min(cc.max(dd));
        if low > high {
            return None;
        }
        if low < high {
            return Some(
                if (equal(a, c) && equal(b, d)) || (equal(a, d) && equal(b, c)) {
                    ContactKind::Coincident
                } else {
                    ContactKind::PositiveOverlap
                },
            );
        }
    } else if signs[0] != Ordering::Equal
        && signs[1] != Ordering::Equal
        && signs[0] != signs[1]
        && signs[2] != Ordering::Equal
        && signs[3] != Ordering::Equal
        && signs[2] != signs[3]
    {
        return Some(ContactKind::ProperCrossing);
    }
    for point in [a, b, c, d] {
        if on(point, first) && on(point, second) {
            let roles = [position(point, first), position(point, second)];
            return Some(if collinear {
                ContactKind::CollinearPoint(roles)
            } else if roles.contains(&ContactPosition::Interior) {
                ContactKind::EndpointInterior(roles)
            } else {
                ContactKind::EndpointContact(roles)
            });
        }
    }
    None
}
fn allowed_adjacent(
    input: &SectionPaths<'_, '_, '_>,
    a: &Edge,
    b: &Edge,
    kind: ContactKind,
) -> bool {
    let count = input.path_edges(a.reference.path).map_or(0, <[usize]>::len);
    let wrap = input.paths()[a.reference.path].closed
        && a.reference.offset == 0
        && b.reference.offset + 1 == count;
    let successive = a.reference.offset + 1 == b.reference.offset;
    let (ContactKind::EndpointContact(roles) | ContactKind::CollinearPoint(roles)) = kind else {
        return false;
    };
    (successive
        && a.vertices[1] == b.vertices[0]
        && roles == [ContactPosition::End, ContactPosition::Start])
        || (wrap
            && a.vertices[0] == b.vertices[1]
            && roles == [ContactPosition::Start, ContactPosition::End])
}

#[cfg(test)]
mod tests {
    use super::*;
    use ContactPosition::{Collapsed, End, Interior, Start};

    fn segment(a: [f64; 2], b: [f64; 2]) -> [Point3; 2] {
        [Point3::new(a[0], a[1], 0.), Point3::new(b[0], b[1], 0.)]
    }

    #[test]
    fn exact_contact_taxonomy_and_endpoint_roles() {
        let base = segment([0., 0.], [4., 0.]);
        let cases = [
            (
                segment([2., -1.], [2., 1.]),
                Some(ContactKind::ProperCrossing),
            ),
            (
                segment([4., 0.], [4., 2.]),
                Some(ContactKind::EndpointContact([End, Start])),
            ),
            (
                segment([2., 0.], [2., 2.]),
                Some(ContactKind::EndpointInterior([Interior, Start])),
            ),
            (
                segment([4., 0.], [6., 0.]),
                Some(ContactKind::CollinearPoint([End, Start])),
            ),
            (
                segment([1., 0.], [3., 0.]),
                Some(ContactKind::PositiveOverlap),
            ),
            (segment([4., 0.], [0., 0.]), Some(ContactKind::Coincident)),
            (
                segment([2., 0.], [2., 0.]),
                Some(ContactKind::Collapsed([Interior, Collapsed])),
            ),
            (
                segment([0., 0.], [0., 0.]),
                Some(ContactKind::Collapsed([Start, Collapsed])),
            ),
            (segment([5., 0.], [5., 0.]), None),
            (segment([0., 1.], [4., 1.]), None),
        ];
        for (other, expected) in cases {
            assert_eq!(contact(base, other), expected);
        }
        assert_eq!(
            contact(segment([-0., 0.], [0., -0.]), segment([0., 0.], [0., 0.])),
            Some(ContactKind::Collapsed([Collapsed, Collapsed]))
        );
    }

    // Parametric rational line oracle: small integers keep every i128 intermediate exact.
    // Classification codes deliberately do not call orientation/on/contact from production.
    fn oracle(coords: [[i128; 2]; 4]) -> u8 {
        let [aa, bb, cc, dd] = coords;
        let delta = |a: [i128; 2], b: [i128; 2]| [a[0] - b[0], a[1] - b[1]];
        let cross = |a: [i128; 2], b: [i128; 2]| a[0] * b[1] - a[1] * b[0];
        let rr = delta(bb, aa);
        let ss = delta(dd, cc);
        let qq = delta(cc, aa);
        let within = |p: [i128; 2], a: [i128; 2], b: [i128; 2]| {
            cross(delta(p, a), delta(b, a)) == 0
                && (0..2).all(|k| p[k] >= a[k].min(b[k]) && p[k] <= a[k].max(b[k]))
        };
        if aa == bb {
            return u8::from(within(aa, cc, dd));
        }
        if cc == dd {
            return u8::from(within(cc, aa, bb));
        }
        let det = cross(rr, ss);
        if det != 0 {
            let sign = det.signum();
            let denominator = det.abs();
            let t = cross(qq, ss) * sign;
            let u = cross(qq, rr) * sign;
            if t < 0 || t > denominator || u < 0 || u > denominator {
                0
            } else if t > 0 && t < denominator && u > 0 && u < denominator {
                2
            } else {
                1
            }
        } else if cross(qq, rr) != 0 {
            0
        } else {
            let k = usize::from(rr[0] == 0);
            let low = aa[k].min(bb[k]).max(cc[k].min(dd[k]));
            let high = aa[k].max(bb[k]).min(cc[k].max(dd[k]));
            match low.cmp(&high) {
                Ordering::Greater => 0,
                Ordering::Less => 3,
                Ordering::Equal => 1,
            }
        }
    }
    fn class(kind: Option<ContactKind>) -> u8 {
        match kind {
            None => 0,
            Some(ContactKind::ProperCrossing) => 2,
            Some(ContactKind::PositiveOverlap | ContactKind::Coincident) => 3,
            _ => 1,
        }
    }
    #[test]
    fn exhaustive_small_grid_matches_independent_parametric_oracle_and_symmetries() {
        let grid = [-1_i32, 0, 1]
            .into_iter()
            .flat_map(|x| [-1, 0, 1].map(|y| [x, y]))
            .collect::<Vec<_>>();
        let mut count = 0;
        for &aa in &grid {
            for &bb in &grid {
                for &cc in &grid {
                    for &dd in &grid {
                        let expected = oracle([aa, bb, cc, dd].map(|p| p.map(i128::from)));
                        let [a, b, c, d] = [aa, bb, cc, dd].map(|p| p.map(f64::from));
                        for (first, second) in [
                            (segment(a, b), segment(c, d)),
                            (segment(b, a), segment(c, d)),
                            (segment(a, b), segment(d, c)),
                            (segment(c, d), segment(a, b)),
                        ] {
                            assert_eq!(class(contact(first, second)), expected);
                            count += 1;
                        }
                    }
                }
            }
        }
        assert_eq!(count, 26244);
    }
    #[test]
    fn extreme_subnormal_and_adjacent_values_do_not_use_float_differences() {
        let maximum = f64::MAX;
        let eta = f64::from_bits(1);
        assert_eq!(
            contact(
                segment([-maximum, 0.], [maximum, 0.]),
                segment([0., -maximum], [0., maximum])
            ),
            Some(ContactKind::ProperCrossing)
        );
        assert_eq!(
            contact(segment([0., 0.], [eta, eta]), segment([0., eta], [eta, 0.])),
            Some(ContactKind::ProperCrossing)
        );
        assert_eq!(
            contact(
                segment([0., 0.], [1., 0.]),
                segment([f64::from_bits(1_f64.to_bits() + 1), 0.], [2., 0.])
            ),
            None
        );
    }
    #[test]
    fn lossless_dyadic_maps_preserve_relations_and_contact_roles() {
        let first = segment([0., 0.], [4., 0.]);
        let second = segment([2., 0.], [2., 3.]);
        for scale in [0.125, 1., 8.] {
            for shift in [[0., 0.], [4.25, -1.125]] {
                let transform = |ps: [Point3; 2]| {
                    ps.map(|p| Point3::new(p.x * scale + shift[0], p.y * scale + shift[1], 0.))
                };
                assert_eq!(
                    contact(transform(first), transform(second)),
                    contact(first, second)
                );
            }
        }
    }
    #[test]
    fn checked_pair_and_buffer_bounds_do_not_overflow_in_the_guard() {
        assert_eq!(pairs(0), Ok(0));
        assert_eq!(pairs(1), Ok(0));
        assert_eq!(pairs(5), Ok(10));
        assert_eq!(pairs(usize::MAX), Err(ValidityError::SizeOverflow));
        assert_eq!(bytes::<Edge>(usize::MAX), Err(ValidityError::SizeOverflow));
        let mut findings = Findings {
            records: Vec::new(),
            requested: 0,
            limits: ValidityLimits {
                max_payload_bytes: 0,
                ..ValidityLimits::default()
            },
        };
        assert!(matches!(
            findings.push(
                PlanarDiagnostic::CollapsedEdge(PathEdgeRef { path: 0, offset: 0 }),
                &mut ValiditySummary::default(),
                &mut Cancellation::never()
            ),
            Err(ValidityError::LimitExceeded {
                resource: ValidityResource::PayloadBytes,
                ..
            })
        ));
    }
}
