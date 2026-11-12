//! Deterministic, non-destructive source-index topology. No tolerance-based welding is performed.
//!
//! Geometrically degenerate faces remain in the mesh and vertex incidence index, but are excluded
//! from edges, links, and components. Every affected vertex is indeterminate. Duplicate faces are
//! retained in all usable incidences. Exact-coordinate STL deduplication establishes index identity;
//! manual meshes retain their supplied identity, even for coincident coordinates.
//!
//! A closed combinatorial surface is not a verified solid: self-intersections, outwardness,
//! orientability of a conflicting winding, nesting, volume, thickness, and printability are untested.
//!
//! ```
//! use karasu_core::geometry::Point3;
//! use karasu_core::mesh::TriangleMesh;
//! use karasu_core::mesh::topology::CheckStatus;
//!
//! let mesh = TriangleMesh::try_new(
//!     vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0),
//!          Point3::new(0.0, 1.0, 0.0)],
//!     vec![[0, 1, 2]],
//! )?;
//! let report = mesh.analyze_topology()?;
//! assert_eq!(report.summary().boundary_edges, 3);
//! assert_eq!(report.summary().vertex_manifold, CheckStatus::Satisfied);
//! assert!(!report.summary().closed_combinatorial_surface);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

mod analysis;
mod components;
mod defects;
mod util;
mod vertex;

use super::{Degeneracy, MeshDiagnostic, MeshError};
use crate::cancellation::Cancelled;
use std::{error::Error, fmt, ops::Range};

pub(super) use analysis::{analyze, edge_summary};
pub(super) use defects::analyze as defects;
pub(crate) use defects::RimTable;
pub(in crate::mesh) use defects::{undirected, Classes};
pub use defects::{
    DefectClass, DefectReport, DefectSample, RimDetail, MAX_RIM_DETAILS, MAX_SAMPLES,
    SMALL_COMPONENT_MM2,
};

/// Canonical undirected identity, ordered lexicographically by source vertex indices.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct EdgeKey([u32; 2]);

impl EdgeKey {
    /// Constructs `(min(a,b), max(a,b))`; a collapsed index pair has no ordinary edge identity.
    #[must_use]
    pub fn new(a: u32, b: u32) -> Option<Self> {
        (a != b).then_some(Self([a.min(b), a.max(b)]))
    }

    /// Ascending source vertex indices.
    #[must_use]
    pub const fn vertices(self) -> [u32; 2] {
        self.0
    }
}

/// Original directed use of an edge by one usable source triangle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DirectedEdge {
    /// Source triangle index.
    pub triangle: usize,
    /// Source corner at the start of this edge: 0, 1, or 2.
    pub local_edge: u8,
    /// Source vertex at the start.
    pub from: u32,
    /// Source vertex at the end.
    pub to: u32,
}

impl DirectedEdge {
    fn key(self) -> EdgeKey {
        EdgeKey([self.from.min(self.to), self.from.max(self.to)])
    }
}

/// Face incidence classification; duplicates count as separate faces.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EdgeKind {
    /// One usable incident face.
    Boundary,
    /// Exactly two usable incident faces; this alone says nothing about vertex links or winding.
    TwoFace,
    /// More than two usable incident faces.
    NonManifold,
}

/// One sorted edge and its range in the report's flat directed incidence buffer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EdgeIncidence {
    /// Canonical source-index identity.
    pub key: EdgeKey,
    /// Incidence class.
    pub kind: EdgeKind,
    /// True only for two-face edges whose directions agree rather than oppose.
    pub winding_conflict: bool,
    uses: Range<usize>,
}

/// A failed local link condition. Link edges retain multiplicity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VertexDefect {
    /// More than one connected fan touches the vertex.
    DisconnectedFans,
    /// Repeated opposite-vertex pair, including a duplicate incident face.
    DuplicateLinkEdge,
    /// The link is neither a simple cycle nor a simple path.
    BranchedLink,
}

/// Whole source-vertex neighborhood classification, independent of face orientation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VertexKind {
    /// No source triangle references this vertex.
    Isolated,
    /// A single simple cycle of at least three link nodes.
    InteriorManifold,
    /// A single simple path with two degree-one endpoints.
    BoundaryManifold,
    /// Complete neighborhood fails a link condition.
    NonManifold(VertexDefect),
    /// At least one incident source face was excluded; the usable link is incomplete.
    Indeterminate,
}

/// Vertex record; its position in `TopologyReport::vertices()` is the source vertex index.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VertexIncidence {
    /// Classification of the complete source neighborhood, not just a usable subset.
    pub kind: VertexKind,
    /// Number of usable incident faces.
    pub usable_faces: usize,
    /// Number of excluded incident faces.
    pub excluded_faces: usize,
    /// Connected component count of the usable link, zero for an empty link.
    pub usable_link_components: usize,
    faces: Range<usize>,
}

/// A separately scoped finding. `Indeterminate` never means a successful check.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckStatus {
    /// The documented check was established.
    Satisfied,
    /// A counterexample was found.
    Failed,
    /// Exclusions or unsupported incidence prevent a complete affirmative check.
    Indeterminate,
    /// There is no surface on which to perform the check.
    NotApplicable,
}

/// Independent counts and whole-mesh findings. Counts refer to usable faces unless specified.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TopologySummary {
    /// Entire original vertex buffer, including unused entries.
    pub source_vertices: usize,
    /// Entire original triangle buffer, including excluded and duplicate entries.
    pub source_triangles: usize,
    /// Faces contributing ordinary incidences.
    pub usable_triangles: usize,
    /// Degenerate faces excluded under the existing computed-normal policy.
    pub excluded_triangles: usize,
    /// Exact-coordinate duplicate diagnostics, including any excluded duplicate faces.
    pub duplicate_triangles: usize,
    /// Number of unique undirected usable edges.
    pub edges: usize,
    /// One-face edges.
    pub boundary_edges: usize,
    /// Two-face edges, regardless of winding.
    pub two_face_edges: usize,
    /// More-than-two-face edges.
    pub non_manifold_edges: usize,
    /// Same-direction two-face edges.
    pub winding_conflicts: usize,
    /// Vertices with a complete cycle link.
    pub interior_vertices: usize,
    /// Vertices with a complete path link.
    pub boundary_vertices: usize,
    /// Vertices with a complete but invalid link.
    pub non_manifold_vertices: usize,
    /// Vertices touching excluded faces.
    pub indeterminate_vertices: usize,
    /// Vertices with no source incidence.
    pub isolated_vertices: usize,
    /// Edge-connected usable triangle components.
    pub edge_components: usize,
    /// Vertex-connected usable triangle components.
    pub vertex_components: usize,
    /// No observed usable boundary edges. This can be vacuously true; not a closedness claim.
    pub no_boundary_edges: bool,
    /// All usable edges have at most two faces; exclusions prevent an affirmative whole-mesh result.
    pub edge_manifold: CheckStatus,
    /// All source vertices have one complete simple cycle/path link; isolated vertices fail.
    pub vertex_manifold: CheckStatus,
    /// All two-face directions oppose; non-manifold edges/exclusions make success indeterminate.
    pub local_winding: CheckStatus,
    /// Nonempty, no exclusions or isolated vertices, edge/vertex manifold, and no boundary.
    /// Independent of local winding; does not prove an embedded, oriented, or printable solid.
    pub closed_combinatorial_surface: bool,
}

/// Stable detailed finding. Edge references index the sorted report edge buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TopologyIssue {
    /// Source face excluded from ordinary incidences.
    ExcludedTriangle {
        /// Source triangle index.
        triangle: usize,
        /// Existing geometric degeneracy classification.
        reason: Degeneracy,
    },
    /// One usable incident face.
    BoundaryEdge {
        /// Report edge index.
        edge: usize,
    },
    /// More than two usable incident faces.
    NonManifoldEdge {
        /// Report edge index.
        edge: usize,
    },
    /// Two usable faces traverse their common edge in the same direction.
    WindingConflict {
        /// Report edge index.
        edge: usize,
        /// Source triangles, in ascending order.
        triangles: [usize; 2],
    },
    /// Complete link fails a manifold condition.
    NonManifoldVertex {
        /// Source vertex index.
        vertex: usize,
        /// Deterministically selected defect: disconnected, duplicate, then branched.
        reason: VertexDefect,
    },
    /// Complete link cannot be established because incident faces were excluded.
    IndeterminateVertex {
        /// Source vertex index.
        vertex: usize,
    },
    /// No source face uses the vertex.
    IsolatedVertex {
        /// Source vertex index.
        vertex: usize,
    },
}

/// One usable triangle component. Connectivity alone implies neither closedness nor a solid.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConnectedComponent {
    /// Number of source triangles in this component.
    pub triangle_count: usize,
    /// Distinct source vertex identities in this component.
    pub vertex_count: usize,
    /// Unique usable edges.
    pub edge_count: usize,
    /// One-face edges.
    pub boundary_edges: usize,
    /// Two-face edges.
    pub two_face_edges: usize,
    /// More-than-two-face edges.
    pub non_manifold_edges: usize,
    /// Same-direction two-face edges.
    pub winding_conflicts: usize,
    triangles: Range<usize>,
    vertices: Range<usize>,
}

/// Flat component partition: ascending members, components ordered by minimum source triangle.
/// Excluded faces and isolated vertices do not belong to either triangle partition.
#[derive(Debug, PartialEq, Eq)]
pub struct ComponentPartition {
    components: Vec<ConnectedComponent>,
    triangles: Vec<usize>,
    vertices: Vec<u32>,
}

impl ComponentPartition {
    /// Component summaries in deterministic order.
    #[must_use]
    pub fn components(&self) -> &[ConnectedComponent] {
        &self.components
    }

    /// Ascending source triangle members, or `None` for an absent component index.
    #[must_use]
    pub fn triangles(&self, component: usize) -> Option<&[usize]> {
        self.components
            .get(component)
            .map(|c| &self.triangles[c.triangles.clone()])
    }

    /// Ascending distinct source vertex members, or `None` for an absent component index.
    #[must_use]
    pub fn vertices(&self, component: usize) -> Option<&[u32]> {
        self.components
            .get(component)
            .map(|c| &self.vertices[c.vertices.clone()])
    }
}

/// Complete derived report. Source geometry, metadata, and scene transforms are never owned here.
#[derive(Debug, PartialEq, Eq)]
pub struct TopologyReport {
    summary: TopologySummary,
    edges: Vec<EdgeIncidence>,
    uses: Vec<DirectedEdge>,
    vertices: Vec<VertexIncidence>,
    incident_faces: Vec<usize>,
    issues: Vec<TopologyIssue>,
    geometry_diagnostics: Vec<MeshDiagnostic>,
    edge_components: ComponentPartition,
    vertex_components: ComponentPartition,
}

impl TopologyReport {
    /// Independent counts and narrowly defined findings.
    #[must_use]
    pub const fn summary(&self) -> &TopologySummary {
        &self.summary
    }
    /// Edges in ascending canonical key order.
    #[must_use]
    pub fn edges(&self) -> &[EdgeIncidence] {
        &self.edges
    }
    /// Directed uses ordered by source triangle then local edge, or `None` for an absent edge.
    #[must_use]
    pub fn incidents(&self, edge: usize) -> Option<&[DirectedEdge]> {
        self.edges.get(edge).map(|e| &self.uses[e.uses.clone()])
    }
    /// One record per original vertex, in source order.
    #[must_use]
    pub fn vertices(&self) -> &[VertexIncidence] {
        &self.vertices
    }
    /// All incident source faces, including excluded faces, ascending and once per face.
    #[must_use]
    pub fn incident_faces(&self, vertex: usize) -> Option<&[usize]> {
        self.vertices
            .get(vertex)
            .map(|v| &self.incident_faces[v.faces.clone()])
    }
    /// Exclusions in source face order, edge findings in key order, vertex findings in source order.
    #[must_use]
    pub fn issues(&self) -> &[TopologyIssue] {
        &self.issues
    }
    /// Existing mesh geometric diagnostics, unchanged, including exact-coordinate duplicates.
    /// STL stored-normal/attribute diagnostics remain on the separately owned import result.
    #[must_use]
    pub fn geometry_diagnostics(&self) -> &[MeshDiagnostic] {
        &self.geometry_diagnostics
    }
    /// Faces connected through a shared usable edge, including non-manifold edges.
    #[must_use]
    pub const fn edge_components(&self) -> &ComponentPartition {
        &self.edge_components
    }
    /// Faces connected through any shared source vertex; may join otherwise separate edge fans.
    #[must_use]
    pub const fn vertex_components(&self) -> &ComponentPartition {
        &self.vertex_components
    }
}

/// Edge closure and orientation counts of a mesh, without the full topology report.
///
/// Every field equals the same-named [`TopologySummary`] field: degenerate triangles are excluded,
/// and edges are keyed by their two vertex indices. Scratch is one word per directed edge use,
/// so very large meshes can be admitted where only these facts matter (winding-union slicing).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EdgeSummary {
    /// Faces contributing ordinary incidences.
    pub usable_triangles: usize,
    /// Degenerate faces excluded under the existing computed-normal policy.
    pub excluded_triangles: usize,
    /// Unique undirected usable edges.
    pub edges: usize,
    /// One-face edges.
    pub boundary_edges: usize,
    /// Two-face edges, regardless of winding.
    pub two_face_edges: usize,
    /// More-than-two-face edges.
    pub non_manifold_edges: usize,
    /// Same-direction two-face edges.
    pub winding_conflicts: usize,
}

/// Caller-selected input caps, not a peak-memory/RSS guarantee.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TopologyLimits {
    /// Maximum original vertex count, including unused vertices.
    pub max_vertices: usize,
    /// Maximum original face count, including degenerate faces.
    pub max_triangles: usize,
}

impl Default for TopologyLimits {
    fn default() -> Self {
        let limits = super::stl::ImportLimits::default();
        Self {
            max_vertices: limits.max_vertices,
            max_triangles: limits.max_triangles,
        }
    }
}

/// Input resource bounded by the topology analyzer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TopologyResource {
    /// Source vertices.
    Vertices,
    /// Source triangles.
    Triangles,
}

/// Controlled analysis failure. No partial successful manifold classification is published.
#[derive(Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum TopologyError {
    /// Caller cancellation, including subordinate geometric diagnostics.
    Cancelled(Cancelled),
    /// Input exceeds a configured cap.
    LimitExceeded {
        /// Bounded resource.
        resource: TopologyResource,
        /// Configured maximum.
        limit: usize,
    },
    /// Input-derived count or byte capacity cannot be represented.
    SizeOverflow,
    /// Existing geometric diagnostic or allocation failure, preserving its cause.
    Mesh(MeshError),
}

impl fmt::Display for TopologyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled(cancelled) => cancelled.fmt(f),
            Self::LimitExceeded { resource, limit } => {
                write!(f, "topology {resource:?} limit exceeded: {limit}")
            }
            Self::SizeOverflow => write!(f, "topology count or capacity exceeds platform size"),
            Self::Mesh(error) => write!(f, "topology analysis: {error}"),
        }
    }
}

impl Error for TopologyError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Cancelled(cause) => Some(cause),
            Self::Mesh(error) => Some(error),
            _ => None,
        }
    }
}

impl From<MeshError> for TopologyError {
    fn from(error: MeshError) -> Self {
        match error {
            MeshError::Cancelled(cause) => Self::Cancelled(cause),
            other => Self::Mesh(other),
        }
    }
}

impl From<Cancelled> for TopologyError {
    fn from(value: Cancelled) -> Self {
        Self::Cancelled(value)
    }
}
