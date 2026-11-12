//! Non-destructive normalization of complete M3.2-A raw sections; see TD-019.
//! Source identity, exact coordinate coincidence, multiplicity and ownership remain separate.
//! No contour, filled region, global embedding, outwardness or printability is inferred.
//!
//! ```
//! use karasu_core::{mesh::TriangleMesh, slicing::{section_mesh_z, SectionLimits, SectionTransform}};
//! use karasu_core::slicing::normalize::{normalize_section, NormalizationLimits};
//! let mesh = TriangleMesh::try_new(vec![], vec![])?;
//! let raw = section_mesh_z(&mesh, 0.0, SectionTransform::Identity, SectionLimits::default())?;
//! let normalized = normalize_section(&raw, NormalizationLimits::default())?;
//! assert!(std::ptr::eq(normalized.raw(), std::ptr::from_ref(&raw)));
//! assert_eq!(normalized.groups(), []);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use super::exact;

use super::{FaceSection, MeshSection};
use crate::cancellation::{Cancellation, Cancelled, Checkpoint};
use crate::geometry::section::{
    EndpointOrigin, LocalEdge, TriangleAreaObservation, TrianglePlaneEvent, VertexPlaneSide,
};
use crate::geometry::Point3;
use std::{cmp::Ordering, error::Error, fmt, ops::Range};

/// Identity in the raw section's source mesh, independent of transformed coordinate equality.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SourceFeature {
    /// Indexed source vertex, even when transformed onto a different feature.
    Vertex(u32),
    /// Canonical unordered source pair (including collapsed pairs in raw provenance).
    Edge([u32; 2]),
}

/// One coordinate observation, retaining the raw occurrence and original local identity.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EndpointObservation {
    /// Source-feature identity; not a coordinate merge key.
    pub feature: SourceFeature,
    /// Finite world position on the requested plane, numerical zeros normalized.
    pub position: Point3,
    /// Index into `raw().events()`; source face is accessible there.
    pub event: usize,
    /// Original local origin, even when multiple corners share one source index.
    pub origin: EndpointOrigin,
    /// Number of segment ends using this record (two for a coplanar triangle corner).
    pub segment_ends: usize,
}

/// Agreement for one identity. Conflict never silently selects a coordinate representative.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CoordinateAgreement {
    /// Every observation is exactly equal, after signed-zero normalization.
    Agreed(Point3),
    /// Inspect all observation IDs; no canonical point has been chosen.
    Conflict,
}

/// Source-feature group, ordered by identity, with all observations recoverable.
#[derive(Clone, Debug, PartialEq)]
pub struct FeatureGroup {
    /// Shared source identity.
    pub feature: SourceFeature,
    /// Range in `feature_order()`, whose elements index `observations()`.
    pub observations: Range<usize>,
    /// Exact coordinate agreement or explicit numerical inconsistency.
    pub coordinates: CoordinateAgreement,
    /// Raw segment-end multiplicity, not a manifold or contour-degree certificate.
    pub segment_ends: usize,
}

/// Independent exact coordinate group. Equal coordinates never redefine source identity.
#[derive(Clone, Debug, PartialEq)]
pub struct CoordinateGroup {
    /// Exact normalized represented position.
    pub position: Point3,
    /// Range in `coordinate_order()`, whose elements index `observations()`.
    pub observations: Range<usize>,
    /// Number of different source features observed at this position.
    pub distinct_features: usize,
}

/// Which preserved raw contribution produced a normalized segment occurrence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContributionKind {
    /// M3.1 Segment, including represented zero length.
    Crossing,
    /// M3.1 `CoplanarEdge` from a noncoplanar source triangle.
    OnEdge(LocalEdge),
    /// One of three original directed edges of an M3.1 `CoplanarTriangle`.
    CoplanarFaceEdge(LocalEdge),
}

/// Canonical feature-pair incidence; coordinates and opposite directions do not erase occurrences.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SegmentOccurrence {
    /// Raw event index; original source face, winding and all context remain accessible there.
    pub event: usize,
    /// Contribution class and original local edge where applicable.
    pub kind: ContributionKind,
    /// Observation IDs in ascending source-feature order (input-order tie break for equal keys).
    pub endpoints: [usize; 2],
    /// Whether canonicalization reversed incoming payload order, not an inferred surface winding.
    pub reversed: bool,
}

/// Conservative interpretation of one canonical feature-pair group, never a filled-region claim.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ownership {
    /// One ordinary noncollapsed crossing contribution; not a certified contour edge.
    CrossingCandidate,
    /// Exactly two coherent noncoplanar faces cross the plane through their shared source edge.
    OnPlaneCrossingCandidate,
    /// One verified coplanar patch edge, optionally with one coherent attached noncoplanar face.
    /// This is local patch exterior only; all original occurrences remain retained.
    CoplanarPatchBoundary,
    /// Exactly two verified triangles form a local patch across this edge; occurrences retained.
    CoplanarPatchInternal,
    /// Inspect structured issues; not automatically a boundary or an error/repair operation.
    Unresolved,
}

/// Independent machine-readable issue bits; several may coexist on one group.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IncidenceIssue {
    /// One source identity has disagreeing finite coordinates.
    CoordinateConflict,
    /// Represented segment endpoints coincide, without erasing source identities.
    Collapsed,
    /// Repeated/zero/unavailable context or exact coplanar rank loss prevents certification.
    DegenerateOrUnassessed,
    /// Multiple contributions cannot be reduced to one ordinary crossing.
    MultipleContributions,
    /// Two contributing faces have identical unordered source index triples.
    DuplicateFace,
    /// More than two distinct faces contribute to this on-plane source edge.
    NonManifoldEdge,
    /// Directed shared-edge uses agree rather than oppose.
    WindingConflict,
    /// Coplanar third vertices lie on the same strict side of their shared edge.
    OverlappingCoplanar,
    /// Coplanar-face and noncoplanar on-edge contributions lack a coherent verified patch perimeter.
    MixedCoplanar,
    /// Noncoplanar on-plane edge ownership remains explicitly unresolved.
    OnEdgeOwnership,
}

/// Compact issue set. No text log or per-occurrence diagnostic allocation is used.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IncidenceIssues(u16);

impl IncidenceIssues {
    /// Whether this independently reportable issue is present.
    #[must_use]
    pub const fn contains(self, issue: IncidenceIssue) -> bool {
        self.0 & (1 << (issue as u8)) != 0
    }
    /// Whether no issue was recorded (not a global validity certificate).
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
    fn add(&mut self, issue: IncidenceIssue) {
        self.0 |= 1 << (issue as u8);
    }
}

/// Segment group in lexicographic source-feature order. No occurrence has been suppressed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SegmentGroup {
    /// Canonical endpoint feature pair.
    pub endpoints: [SourceFeature; 2],
    /// Range in `segment_order()`, whose elements index `segments()`; its length is multiplicity.
    pub occurrences: Range<usize>,
    /// Count of distinct original source faces contributing (not occurrence parity).
    pub source_faces: usize,
    /// Local classification with explicit certification limits.
    pub ownership: Ownership,
    /// Structured ambiguity/numerical/degeneracy reasons.
    pub issues: IncidenceIssues,
}

/// Flat normalization limits, independent of M3.2-A struct literals and topology caps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NormalizationLimits {
    /// Maximum retained raw events traversed (None events were already omitted).
    pub max_events: usize,
    /// Maximum endpoint observations and, independently, segment occurrences.
    pub max_incidences: usize,
    /// Sum of requested capacities of all output/index buffers, excluding borrowed raw data,
    /// allocator overhead/over-allocation and bounded exact-predicate stack; not a total RSS cap.
    pub max_payload_bytes: usize,
}

impl Default for NormalizationLimits {
    fn default() -> Self {
        Self {
            max_events: 1_000_000,
            max_incidences: 3_000_000,
            max_payload_bytes: 512 * 1024 * 1024,
        }
    }
}

/// Distinct normalization resource cap.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NormalizationResource {
    /// Input retained events.
    Events,
    /// Endpoint observation or segment occurrence count.
    Incidences,
    /// Sum of requested output/scratch buffer capacity bytes.
    PayloadBytes,
}

/// Fatal normalization failure. Finite coordinate conflicts and ambiguous geometry instead survive
/// as `CoordinateAgreement::Conflict` / `Ownership::Unresolved` with complete observations.
#[derive(Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum NormalizationError {
    /// Synchronous observed cancellation; no successful partial result.
    Cancelled(Cancelled),
    /// Resource count or aggregate requested bytes exceed a caller cap.
    LimitExceeded {
        /// Kind of resource.
        resource: NormalizationResource,
        /// Configured maximum.
        limit: usize,
    },
    /// Checked count, byte or isize capacity arithmetic overflow.
    SizeOverflow,
    /// Fallible normalization buffer reservation failed.
    Allocation,
    /// Raw section summary, face ordering or local source provenance is inconsistent.
    InvalidProvenance {
        /// Raw event index; None means the section-wide summary failed validation.
        event: Option<usize>,
    },
    /// Nonfinite or off-plane coordinate observation (including an invalid section plane).
    InvalidCoordinate {
        /// Raw event index; None means the section-wide plane failed validation.
        event: Option<usize>,
    },
}

impl fmt::Display for NormalizationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled(e) => e.fmt(f),
            Self::LimitExceeded { resource, limit } => {
                write!(f, "normalization {resource:?} limit exceeded ({limit})")
            }
            Self::SizeOverflow => write!(f, "normalization count or capacity arithmetic overflow"),
            Self::Allocation => write!(f, "unable to reserve normalization buffers"),
            Self::InvalidProvenance { event } => write!(
                f,
                "inconsistent normalization source provenance at raw event {event:?}"
            ),
            Self::InvalidCoordinate { event } => write!(
                f,
                "nonfinite or off-plane normalization coordinate at raw event {event:?}"
            ),
        }
    }
}

impl Error for NormalizationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Cancelled(e) => Some(e),
            _ => None,
        }
    }
}
impl From<Cancelled> for NormalizationError {
    fn from(e: Cancelled) -> Self {
        Self::Cancelled(e)
    }
}

/// Complete normalization summary. No contour topology is inferred from these incidence counts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NormalizationSummary {
    /// Retained raw events, including tangents and coplanar faces.
    pub raw_events: usize,
    /// Point observations, including every raw local corner/edge identity.
    pub observations: usize,
    /// All contributed segment occurrences, including internal/ambiguous/duplicate ones.
    pub segment_occurrences: usize,
    /// Different source-feature identities.
    pub features: usize,
    /// Canonical source-feature pair groups.
    pub segment_groups: usize,
    /// Raw tangent-point events, not suppressed or converted to segments.
    pub tangent_events: usize,
    /// Raw coplanar triangle events, including rank loss.
    pub coplanar_faces: usize,
    /// Source features with inconsistent finite coordinates.
    pub coordinate_conflicts: usize,
    /// Exact coordinate groups containing more than one feature identity.
    pub coincident_feature_positions: usize,
    /// Certified local coplanar internal-edge groups, not deleted occurrences.
    pub internal_patch_edges: usize,
    /// Segment groups requiring explicit later policy or defective-input handling.
    pub unresolved_groups: usize,
}

/// Immutable normalized view borrowing the original complete raw event stream unchanged.
/// Buffer indices/ranges are local to this result, not serialized identifiers.
#[derive(Debug, PartialEq)]
pub struct NormalizedSection<'a> {
    raw: &'a MeshSection,
    observations: Vec<EndpointObservation>,
    feature_order: Vec<usize>,
    features: Vec<FeatureGroup>,
    coordinate_order: Vec<usize>,
    coordinates: Vec<CoordinateGroup>,
    segments: Vec<SegmentOccurrence>,
    segment_order: Vec<usize>,
    groups: Vec<SegmentGroup>,
    summary: NormalizationSummary,
}

impl NormalizedSection<'_> {
    /// Original raw section; no mutation, suppression or copied raw geometry.
    #[must_use]
    pub const fn raw(&self) -> &MeshSection {
        self.raw
    }
    /// Explicit world plane Z (mm), numerical zero normalized.
    #[must_use]
    pub fn plane_z(&self) -> f64 {
        zero(self.raw.plane_z())
    }
    /// Observations in raw event/corner order. Segment endpoints refer to these IDs.
    #[must_use]
    pub fn observations(&self) -> &[EndpointObservation] {
        &self.observations
    }
    /// Observation IDs grouped by source feature, then original observation order.
    #[must_use]
    pub fn feature_order(&self) -> &[usize] {
        &self.feature_order
    }
    /// Feature groups in lexicographic identity order, with no conflicting point selected.
    #[must_use]
    pub fn features(&self) -> &[FeatureGroup] {
        &self.features
    }
    /// Observation IDs ordered by exact coordinate, feature, then original observation order.
    #[must_use]
    pub fn coordinate_order(&self) -> &[usize] {
        &self.coordinate_order
    }
    /// Exact-coordinate coincidences independent of feature grouping.
    #[must_use]
    pub fn coordinates(&self) -> &[CoordinateGroup] {
        &self.coordinates
    }
    /// All segment contributions in raw event order (face edges in local 01/12/20 order).
    #[must_use]
    pub fn segments(&self) -> &[SegmentOccurrence] {
        &self.segments
    }
    /// Segment IDs grouped by canonical feature pair, then original occurrence order.
    #[must_use]
    pub fn segment_order(&self) -> &[usize] {
        &self.segment_order
    }
    /// Segment groups; internal occurrences remain in `segments()` and `raw()`.
    #[must_use]
    pub fn groups(&self) -> &[SegmentGroup] {
        &self.groups
    }
    /// Complete processing totals.
    #[must_use]
    pub const fn summary(&self) -> NormalizationSummary {
        self.summary
    }
}

type Result<T> = std::result::Result<T, NormalizationError>;
const EDGES: [LocalEdge; 3] = [LocalEdge::Edge01, LocalEdge::Edge12, LocalEdge::Edge20];

/// Normalizes complete raw events without choosing contour or filled-region semantics.
/// See TD-019 and [`normalize_section_with_cancellation`] for local ownership invariants.
/// # Errors
/// Returns typed resource, capacity, allocation, provenance or coordinate-domain failure.
pub fn normalize_section(
    raw: &MeshSection,
    limits: NormalizationLimits,
) -> Result<NormalizedSection<'_>> {
    normalize_section_with_cancellation(raw, limits, &mut Cancellation::never())
}

/// Borrows raw input and preserves every contribution in flat buffers. Feature identities group
/// indexed vertices/unordered source edges; exact coordinate conflicts never choose a representative.
/// A local patch-internal edge requires exactly two distinct nonduplicate coplanar triangles,
/// agreed coordinates, opposite winding uses and exact strictly opposite third-vertex XY sides.
/// TD-032 also admits exactly two coherent noncoplanar on-edge faces whose third vertices have
/// opposite strict Z signs. Both occurrences remain retained in one candidate edge.
/// This is not global embedding/solid validation. All other ambiguous groups remain unresolved.
///
/// With E raw events, P<=3E observations and S<=3E segment occurrences: O(E+(P+S)log(P+S)) time,
/// O(P+S) flat storage and bounded exact-sign stack. Preflight checks aggregate requested capacities
/// before allocating eight fallible buffers. Group inner loops poll every 256 records. Entry/final,
/// both event passes and each allocation/sort boundary poll. Allocation, in-place unstable sort,
/// bounded exact orientation and drop are indivisible, not latency/progress/RSS guarantees.
/// # Errors
/// Cancellation wins at its observed checkpoint. Otherwise input/event caps and preflight validation
/// precede aggregate byte caps/reservation, extraction, sorting/grouping and final publication.
/// No successful partial normalized result escapes. Finite conflicting observations are diagnostics,
/// not fatal errors; nonfinite/off-plane coordinates and inconsistent provenance are fatal.
pub fn normalize_section_with_cancellation<'a>(
    raw: &'a MeshSection,
    limits: NormalizationLimits,
    cancel: &mut Cancellation<'_>,
) -> Result<NormalizedSection<'a>> {
    cancel.check(Checkpoint::NormalizationStart)?;
    let (points, segments) = preflight(raw, limits, cancel)?;
    let mut view = NormalizedSection {
        raw,
        observations: Vec::new(),
        feature_order: Vec::new(),
        features: Vec::new(),
        coordinate_order: Vec::new(),
        coordinates: Vec::new(),
        segments: Vec::new(),
        segment_order: Vec::new(),
        groups: Vec::new(),
        summary: NormalizationSummary::default(),
    };
    allocate(&mut view, points, segments, limits, cancel)?;
    extract(&mut view, cancel)?;
    build_features(&mut view, cancel)?;
    build_coordinates(&mut view, cancel)?;
    build_segments(&mut view, cancel)?;
    view.summary.raw_events = raw.events().len();
    view.summary.observations = view.observations.len();
    view.summary.segment_occurrences = view.segments.len();
    view.summary.features = view.features.len();
    view.summary.segment_groups = view.groups.len();
    cancel.check(Checkpoint::NormalizationComplete)?;
    Ok(view)
}

fn add(a: usize, b: usize) -> Result<usize> {
    a.checked_add(b).ok_or(NormalizationError::SizeOverflow)
}
fn count(value: &mut usize) -> Result<()> {
    *value = add(*value, 1)?;
    Ok(())
}
fn cap(value: usize, limit: usize, resource: NormalizationResource) -> Result<()> {
    if value > limit {
        Err(NormalizationError::LimitExceeded { resource, limit })
    } else {
        Ok(())
    }
}
fn bytes<T>(n: usize) -> Result<usize> {
    n.checked_mul(size_of::<T>())
        .filter(|&v| isize::try_from(v).is_ok())
        .ok_or(NormalizationError::SizeOverflow)
}
fn zero(x: f64) -> f64 {
    if x == 0.0 {
        0.0
    } else {
        x
    }
}
fn point_cmp(a: Point3, b: Point3) -> Ordering {
    a.x.total_cmp(&b.x)
        .then_with(|| a.y.total_cmp(&b.y))
        .then_with(|| a.z.total_cmp(&b.z))
}
fn edge_corners(edge: LocalEdge) -> [usize; 2] {
    match edge {
        LocalEdge::Edge01 => [0, 1],
        LocalEdge::Edge12 => [1, 2],
        LocalEdge::Edge20 => [2, 0],
    }
}
fn feature(record: &FaceSection, origin: EndpointOrigin) -> SourceFeature {
    match origin {
        EndpointOrigin::Vertex(c) => SourceFeature::Vertex(record.source_vertex(c)),
        EndpointOrigin::Edge(e) => {
            let mut pair = record.source_edge(e);
            pair.sort_unstable();
            SourceFeature::Edge(pair)
        }
    }
}

fn preflight(
    raw: &MeshSection,
    limits: NormalizationLimits,
    cancel: &mut Cancellation<'_>,
) -> Result<(usize, usize)> {
    cap(
        raw.events().len(),
        limits.max_events,
        NormalizationResource::Events,
    )?;
    if !raw.plane_z().is_finite() {
        return Err(NormalizationError::InvalidCoordinate { event: None });
    }
    let summary = raw.summary();
    if summary.retained_events != raw.events().len()
        || add(summary.non_intersecting_faces, summary.retained_events)? != summary.source_faces
    {
        return Err(NormalizationError::InvalidProvenance { event: None });
    }
    let mut points = 0;
    let mut segments = 0;
    let mut previous = None;
    for (index, record) in raw.events().iter().enumerate() {
        cancel.poll(Checkpoint::NormalizationEvents, index)?;
        if record.source_face >= summary.source_faces
            || previous.is_some_and(|p| p >= record.source_face)
        {
            return Err(NormalizationError::InvalidProvenance { event: Some(index) });
        }
        previous = Some(record.source_face);
        validate(record, index, raw.plane_z())?;
        let (p, s) = match record.intersection.event {
            TrianglePlaneEvent::None => {
                return Err(NormalizationError::InvalidProvenance { event: Some(index) })
            }
            TrianglePlaneEvent::TangentPoint { .. } => (1, 0),
            TrianglePlaneEvent::Segment { .. } | TrianglePlaneEvent::CoplanarEdge { .. } => (2, 1),
            TrianglePlaneEvent::CoplanarTriangle { .. } => (3, 3),
        };
        points = add(points, p)?;
        segments = add(segments, s)?;
        cap(
            points,
            limits.max_incidences,
            NormalizationResource::Incidences,
        )?;
        cap(
            segments,
            limits.max_incidences,
            NormalizationResource::Incidences,
        )?;
    }
    Ok((points, segments))
}

#[allow(clippy::float_cmp)] // Raw points must equal the represented plane exactly, not approximately.
fn validate(record: &FaceSection, event: usize, plane: f64) -> Result<()> {
    let invalid = || NormalizationError::InvalidProvenance { event: Some(event) };
    let coordinate = |p: Point3| {
        if p.coordinates().iter().all(|x| x.is_finite()) && p.z == plane {
            Ok(())
        } else {
            Err(NormalizationError::InvalidCoordinate { event: Some(event) })
        }
    };
    let endpoint = |p: crate::geometry::section::SectionEndpoint| -> Result<()> {
        coordinate(p.position)?;
        let sides = record.intersection.sides;
        let valid = match p.origin {
            EndpointOrigin::Vertex(c) => sides[c as usize] == VertexPlaneSide::On,
            EndpointOrigin::Edge(e) => {
                let [a, b] = edge_corners(e);
                matches!(
                    (sides[a], sides[b]),
                    (VertexPlaneSide::Below, VertexPlaneSide::Above)
                        | (VertexPlaneSide::Above, VertexPlaneSide::Below)
                )
            }
        };
        if valid {
            Ok(())
        } else {
            Err(invalid())
        }
    };
    for [a, b] in [[0, 1], [1, 2], [2, 0]] {
        if record.source_vertices[a] == record.source_vertices[b]
            && record.intersection.sides[a] != record.intersection.sides[b]
        {
            return Err(invalid());
        }
    }
    let on = record
        .intersection
        .sides
        .iter()
        .filter(|&&s| s == VertexPlaneSide::On)
        .count();
    match record.intersection.event {
        TrianglePlaneEvent::None => Err(invalid()),
        TrianglePlaneEvent::TangentPoint { endpoint: p } => {
            if on != 1
                || record.intersection.sides.contains(&VertexPlaneSide::Below)
                    && record.intersection.sides.contains(&VertexPlaneSide::Above)
            {
                return Err(invalid());
            }
            endpoint(p)
        }
        TrianglePlaneEvent::Segment { endpoints, .. } => {
            if !record.intersection.sides.contains(&VertexPlaneSide::Below)
                || !record.intersection.sides.contains(&VertexPlaneSide::Above)
                || endpoints[0].origin == endpoints[1].origin
            {
                return Err(invalid());
            }
            for p in endpoints {
                endpoint(p)?;
            }
            Ok(())
        }
        TrianglePlaneEvent::CoplanarEdge {
            edge, endpoints, ..
        } => {
            let [a, b] = edge_corners(edge);
            if on != 2
                || record.intersection.sides[a] != VertexPlaneSide::On
                || record.intersection.sides[b] != VertexPlaneSide::On
                || endpoints[0].origin == endpoints[1].origin
            {
                return Err(invalid());
            }
            for p in endpoints {
                endpoint(p)?;
                if !matches!(p.origin, EndpointOrigin::Vertex(_)) {
                    return Err(invalid());
                }
            }
            Ok(())
        }
        TrianglePlaneEvent::CoplanarTriangle { corners } => {
            if on != 3 {
                return Err(invalid());
            }
            let mut seen = [false; 3];
            for p in corners {
                coordinate(p.position)?;
                if seen[p.corner as usize] {
                    return Err(invalid());
                }
                seen[p.corner as usize] = true;
            }
            Ok(())
        }
    }
}

fn allocate(
    view: &mut NormalizedSection<'_>,
    p: usize,
    s: usize,
    limits: NormalizationLimits,
    cancel: &mut Cancellation<'_>,
) -> Result<()> {
    let sizes = [
        bytes::<EndpointObservation>(p)?,
        bytes::<usize>(p)?,
        bytes::<FeatureGroup>(p)?,
        bytes::<usize>(p)?,
        bytes::<CoordinateGroup>(p)?,
        bytes::<SegmentOccurrence>(s)?,
        bytes::<usize>(s)?,
        bytes::<SegmentGroup>(s)?,
    ];
    let total = sizes.into_iter().try_fold(0, add)?;
    cap(
        total,
        limits.max_payload_bytes,
        NormalizationResource::PayloadBytes,
    )?;
    reserve(&mut view.observations, p, cancel)?;
    reserve(&mut view.feature_order, p, cancel)?;
    reserve(&mut view.features, p, cancel)?;
    reserve(&mut view.coordinate_order, p, cancel)?;
    reserve(&mut view.coordinates, p, cancel)?;
    reserve(&mut view.segments, s, cancel)?;
    reserve(&mut view.segment_order, s, cancel)?;
    reserve(&mut view.groups, s, cancel)
}
fn reserve<T>(buffer: &mut Vec<T>, capacity: usize, cancel: &mut Cancellation<'_>) -> Result<()> {
    if capacity != 0 {
        cancel.check(Checkpoint::NormalizationStorage)?;
        buffer
            .try_reserve_exact(capacity)
            .map_err(|_| NormalizationError::Allocation)?;
        cancel.check(Checkpoint::NormalizationStorage)?;
    }
    Ok(())
}

fn observation(
    view: &mut NormalizedSection<'_>,
    event: usize,
    origin: EndpointOrigin,
    position: Point3,
    segment_ends: usize,
) -> usize {
    let index = view.observations.len();
    let [x, y, z] = position.coordinates().map(zero);
    view.observations.push(EndpointObservation {
        feature: feature(&view.raw.events()[event], origin),
        position: Point3::new(x, y, z),
        event,
        origin,
        segment_ends,
    });
    index
}
fn occurrence(
    view: &mut NormalizedSection<'_>,
    event: usize,
    kind: ContributionKind,
    mut endpoints: [usize; 2],
) {
    let reversed =
        view.observations[endpoints[0]].feature > view.observations[endpoints[1]].feature;
    if reversed {
        endpoints.swap(0, 1);
    }
    view.segments.push(SegmentOccurrence {
        event,
        kind,
        endpoints,
        reversed,
    });
}
fn extract(view: &mut NormalizedSection<'_>, cancel: &mut Cancellation<'_>) -> Result<()> {
    for (event, record) in view.raw.events().iter().enumerate() {
        cancel.poll(Checkpoint::NormalizationEvents, event)?;
        match record.intersection.event {
            TrianglePlaneEvent::None => {
                return Err(NormalizationError::InvalidProvenance { event: Some(event) })
            }
            TrianglePlaneEvent::TangentPoint { endpoint } => {
                observation(view, event, endpoint.origin, endpoint.position, 0);
                count(&mut view.summary.tangent_events)?;
            }
            TrianglePlaneEvent::Segment { endpoints, .. }
            | TrianglePlaneEvent::CoplanarEdge { endpoints, .. } => {
                let ids = endpoints.map(|p| observation(view, event, p.origin, p.position, 1));
                let kind = if let TrianglePlaneEvent::CoplanarEdge { edge, .. } =
                    record.intersection.event
                {
                    ContributionKind::OnEdge(edge)
                } else {
                    ContributionKind::Crossing
                };
                occurrence(view, event, kind, ids);
            }
            TrianglePlaneEvent::CoplanarTriangle { corners } => {
                let mut ids = [0; 3];
                for p in corners {
                    ids[p.corner as usize] =
                        observation(view, event, EndpointOrigin::Vertex(p.corner), p.position, 2);
                }
                for edge in EDGES {
                    let [a, b] = edge_corners(edge);
                    occurrence(
                        view,
                        event,
                        ContributionKind::CoplanarFaceEdge(edge),
                        [ids[a], ids[b]],
                    );
                }
                count(&mut view.summary.coplanar_faces)?;
            }
        }
    }
    view.feature_order.extend(0..view.observations.len());
    view.coordinate_order.extend(0..view.observations.len());
    view.segment_order.extend(0..view.segments.len());
    Ok(())
}

fn build_features(view: &mut NormalizedSection<'_>, cancel: &mut Cancellation<'_>) -> Result<()> {
    cancel.check(Checkpoint::NormalizationSorting)?;
    view.feature_order
        .sort_unstable_by_key(|&i| (view.observations[i].feature, i));
    cancel.check(Checkpoint::NormalizationSorting)?;
    let mut start = 0;
    while start < view.feature_order.len() {
        let first = view.observations[view.feature_order[start]];
        let mut end = start;
        let mut agreement = CoordinateAgreement::Agreed(first.position);
        let mut uses = 0;
        while end < view.feature_order.len() {
            cancel.poll(Checkpoint::NormalizationGroups, end)?;
            let obs = view.observations[view.feature_order[end]];
            if obs.feature != first.feature {
                break;
            }
            if obs.position != first.position {
                agreement = CoordinateAgreement::Conflict;
            }
            uses = add(uses, obs.segment_ends)?;
            end += 1;
        }
        if matches!(agreement, CoordinateAgreement::Conflict) {
            count(&mut view.summary.coordinate_conflicts)?;
        }
        view.features.push(FeatureGroup {
            feature: first.feature,
            observations: start..end,
            coordinates: agreement,
            segment_ends: uses,
        });
        start = end;
    }
    Ok(())
}

fn build_coordinates(
    view: &mut NormalizedSection<'_>,
    cancel: &mut Cancellation<'_>,
) -> Result<()> {
    cancel.check(Checkpoint::NormalizationSorting)?;
    view.coordinate_order.sort_unstable_by(|&a, &b| {
        point_cmp(view.observations[a].position, view.observations[b].position)
            .then_with(|| {
                view.observations[a]
                    .feature
                    .cmp(&view.observations[b].feature)
            })
            .then(a.cmp(&b))
    });
    cancel.check(Checkpoint::NormalizationSorting)?;
    let mut start = 0;
    while start < view.coordinate_order.len() {
        let position = view.observations[view.coordinate_order[start]].position;
        let mut end = start;
        let mut previous = None;
        let mut distinct = 0;
        while end < view.coordinate_order.len() {
            cancel.poll(Checkpoint::NormalizationGroups, end)?;
            let obs = view.observations[view.coordinate_order[end]];
            if obs.position != position {
                break;
            }
            if previous != Some(obs.feature) {
                count(&mut distinct)?;
                previous = Some(obs.feature);
            }
            end += 1;
        }
        if distinct > 1 {
            count(&mut view.summary.coincident_feature_positions)?;
        }
        view.coordinates.push(CoordinateGroup {
            position,
            observations: start..end,
            distinct_features: distinct,
        });
        start = end;
    }
    Ok(())
}

fn key(view: &NormalizedSection<'_>, segment: usize) -> [SourceFeature; 2] {
    view.segments[segment]
        .endpoints
        .map(|i| view.observations[i].feature)
}
fn conflict(view: &NormalizedSection<'_>, feature: SourceFeature) -> bool {
    view.features
        .binary_search_by_key(&feature, |g| g.feature)
        .is_ok_and(|i| matches!(view.features[i].coordinates, CoordinateAgreement::Conflict))
}

fn build_segments(view: &mut NormalizedSection<'_>, cancel: &mut Cancellation<'_>) -> Result<()> {
    cancel.check(Checkpoint::NormalizationSorting)?;
    let observations = &view.observations;
    let segments = &view.segments;
    view.segment_order
        .sort_unstable_by_key(|&i| (segments[i].endpoints.map(|j| observations[j].feature), i));
    cancel.check(Checkpoint::NormalizationSorting)?;
    let mut start = 0;
    while start < view.segment_order.len() {
        let endpoints = key(view, view.segment_order[start]);
        let mut end = start;
        while end < view.segment_order.len() {
            cancel.poll(Checkpoint::NormalizationGroups, end)?;
            if key(view, view.segment_order[end]) != endpoints {
                break;
            }
            end += 1;
        }
        let group = classify(view, start..end, endpoints, cancel)?;
        if group.ownership == Ownership::Unresolved {
            count(&mut view.summary.unresolved_groups)?;
        }
        if group.ownership == Ownership::CoplanarPatchInternal {
            count(&mut view.summary.internal_patch_edges)?;
        }
        view.groups.push(group);
        start = end;
    }
    Ok(())
}

fn classify(
    view: &NormalizedSection<'_>,
    range: Range<usize>,
    endpoints: [SourceFeature; 2],
    cancel: &mut Cancellation<'_>,
) -> Result<SegmentGroup> {
    let mut issues = IncidenceIssues::default();
    let mut faces = 0;
    let mut previous = None;
    let mut coplanar = 0;
    let mut crossing = 0;
    for index in range.clone() {
        cancel.poll(Checkpoint::NormalizationGroups, index)?;
        let segment = view.segments[view.segment_order[index]];
        let record = &view.raw.events()[segment.event];
        if previous != Some(record.source_face) {
            count(&mut faces)?;
            previous = Some(record.source_face);
        }
        if !matches!(segment.kind, ContributionKind::CoplanarFaceEdge(_))
            && record.intersection.context.area_observation
                != TriangleAreaObservation::NonzeroComputedCross
        {
            issues.add(IncidenceIssue::DegenerateOrUnassessed);
        }
        let [a, b] = segment.endpoints.map(|i| view.observations[i]);
        if a.position == b.position {
            issues.add(IncidenceIssue::Collapsed);
        }
        if conflict(view, a.feature) || conflict(view, b.feature) {
            issues.add(IncidenceIssue::CoordinateConflict);
        }
        match segment.kind {
            ContributionKind::Crossing => count(&mut crossing)?,
            ContributionKind::OnEdge(_) => {}
            ContributionKind::CoplanarFaceEdge(_) => {
                count(&mut coplanar)?;
                for v in record.source_vertices {
                    if conflict(view, SourceFeature::Vertex(v)) {
                        issues.add(IncidenceIssue::CoordinateConflict);
                    }
                }
                let points = coplanar_points(record);
                if exact::orientation(points[0], points[1], points[2]) == Ordering::Equal {
                    issues.add(IncidenceIssue::DegenerateOrUnassessed);
                }
            }
        }
    }
    let first = view.segments[view.segment_order[range.start]];
    if range.len() == 2 {
        let second = view.segments[view.segment_order[range.start + 1]];
        check_duplicate_faces(view, first, second, &mut issues);
    }
    if faces > 2
        && crossing == 0
        && matches!(endpoints,[SourceFeature::Vertex(a),SourceFeature::Vertex(b)] if a!=b)
    {
        issues.add(IncidenceIssue::NonManifoldEdge);
    }
    if faces != range.len() {
        issues.add(IncidenceIssue::DegenerateOrUnassessed);
    }
    let mut ownership = Ownership::Unresolved;
    if crossing == range.len() {
        if range.len() == 1 {
            ownership = Ownership::CrossingCandidate;
        } else {
            issues.add(IncidenceIssue::MultipleContributions);
        }
    } else if coplanar == range.len() {
        if range.len() == 1 {
            ownership = Ownership::CoplanarPatchBoundary;
        } else if range.len() == 2 {
            verify_pair(view, &range, &mut issues);
            if issues.is_empty() {
                ownership = Ownership::CoplanarPatchInternal;
            }
        } else {
            issues.add(IncidenceIssue::MultipleContributions);
        }
    } else if coplanar == 1 && crossing == 0 && range.len() == 2 {
        let second = view.segments[view.segment_order[range.start + 1]];
        ownership = mixed_cap_ownership(view, first, second, &mut issues);
    } else if coplanar == 0 && crossing == 0 && range.len() == 2 {
        let second = view.segments[view.segment_order[range.start + 1]];
        ownership = on_plane_ownership(view, first, second, &mut issues);
    } else {
        issues.add(IncidenceIssue::OnEdgeOwnership);
        if coplanar > 0 {
            issues.add(IncidenceIssue::MixedCoplanar);
        }
    }
    if !issues.is_empty() {
        ownership = Ownership::Unresolved;
    }
    Ok(SegmentGroup {
        endpoints,
        occurrences: range,
        source_faces: faces,
        ownership,
        issues,
    })
}

fn on_plane_ownership(
    view: &NormalizedSection<'_>,
    a: SegmentOccurrence,
    b: SegmentOccurrence,
    issues: &mut IncidenceIssues,
) -> Ownership {
    if verify_on_plane_crossing(view, a, b, issues) && issues.is_empty() {
        Ownership::OnPlaneCrossingCandidate
    } else {
        issues.add(IncidenceIssue::OnEdgeOwnership);
        Ownership::Unresolved
    }
}

// A strict third-vertex sign is exact M3.1 classification, not a geometric tolerance.
// Two retained opposite-side contributions describe one local section edge; source occurrences
// are preserved and downstream degree/contact checks still decide whether a simple cycle exists.
fn verify_on_plane_crossing(
    view: &NormalizedSection<'_>,
    a: SegmentOccurrence,
    b: SegmentOccurrence,
    issues: &mut IncidenceIssues,
) -> bool {
    let (ContributionKind::OnEdge(ae), ContributionKind::OnEdge(be)) = (a.kind, b.kind) else {
        return false;
    };
    let ar = &view.raw.events()[a.event];
    let br = &view.raw.events()[b.event];
    if ar.source_face == br.source_face {
        return false;
    }
    let ap = ar.source_edge(ae);
    let bp = br.source_edge(be);
    if ap != [bp[1], bp[0]] {
        issues.add(IncidenceIssue::WindingConflict);
        return false;
    }
    let [ai, aj] = edge_corners(ae);
    let [bi, bj] = edge_corners(be);
    matches!(
        (
            ar.intersection.sides[3 - ai - aj],
            br.intersection.sides[3 - bi - bj]
        ),
        (VertexPlaneSide::Above, VertexPlaneSide::Below)
            | (VertexPlaneSide::Below, VertexPlaneSide::Above)
    )
}

fn check_duplicate_faces(
    view: &NormalizedSection<'_>,
    first: SegmentOccurrence,
    second: SegmentOccurrence,
    issues: &mut IncidenceIssues,
) {
    let mut a = view.raw.events()[first.event].source_vertices;
    let mut b = view.raw.events()[second.event].source_vertices;
    a.sort_unstable();
    b.sort_unstable();
    if first.event != second.event && a == b {
        issues.add(IncidenceIssue::DuplicateFace);
    }
}

fn mixed_cap_ownership(
    view: &NormalizedSection<'_>,
    a: SegmentOccurrence,
    b: SegmentOccurrence,
    issues: &mut IncidenceIssues,
) -> Ownership {
    // A coherent attached face does not erase a verified coplanar patch
    // perimeter. Its occurrence remains context, never a duplicate edge.
    if verify_mixed_cap_boundary(view, a, b, issues) {
        Ownership::CoplanarPatchBoundary
    } else {
        issues.add(IncidenceIssue::MixedCoplanar);
        issues.add(IncidenceIssue::OnEdgeOwnership);
        Ownership::Unresolved
    }
}

fn verify_mixed_cap_boundary(
    view: &NormalizedSection<'_>,
    a: SegmentOccurrence,
    b: SegmentOccurrence,
    issues: &mut IncidenceIssues,
) -> bool {
    let (cap, cap_edge, side, side_edge) = match (a.kind, b.kind) {
        (ContributionKind::CoplanarFaceEdge(ae), ContributionKind::OnEdge(be)) => (a, ae, b, be),
        (ContributionKind::OnEdge(ae), ContributionKind::CoplanarFaceEdge(be)) => (b, be, a, ae),
        _ => return false,
    };
    let cap = &view.raw.events()[cap.event];
    let side = &view.raw.events()[side.event];
    if cap.source_face == side.source_face {
        return false;
    }
    let [i, j] = edge_corners(side_edge);
    if !matches!(
        side.intersection.sides[3 - i - j],
        VertexPlaneSide::Above | VertexPlaneSide::Below
    ) {
        return false;
    }
    let edge = side.source_edge(side_edge);
    if cap.source_edge(cap_edge) != [edge[1], edge[0]] {
        issues.add(IncidenceIssue::WindingConflict);
        return false;
    }
    issues.is_empty()
}

fn coplanar_points(record: &FaceSection) -> [Point3; 3] {
    let mut points = [Point3::new(0.0, 0.0, 0.0); 3];
    if let TrianglePlaneEvent::CoplanarTriangle { corners } = record.intersection.event {
        for p in corners {
            points[p.corner as usize] = p.position;
        }
    }
    points
}
fn verify_pair(view: &NormalizedSection<'_>, range: &Range<usize>, issues: &mut IncidenceIssues) {
    let a = view.segments[view.segment_order[range.start]];
    let b = view.segments[view.segment_order[range.start + 1]];
    let (ContributionKind::CoplanarFaceEdge(ae), ContributionKind::CoplanarFaceEdge(be)) =
        (a.kind, b.kind)
    else {
        return;
    };
    let ar = &view.raw.events()[a.event];
    let br = &view.raw.events()[b.event];
    let ap = ar.source_edge(ae);
    let bp = br.source_edge(be);
    if ap != [bp[1], bp[0]] {
        issues.add(IncidenceIssue::WindingConflict);
    }
    let [ai, aj] = edge_corners(ae);
    let [bi, bj] = edge_corners(be);
    let apts = coplanar_points(ar);
    let bpts = coplanar_points(br);
    let third_a = apts[3 - ai - aj];
    let third_b = bpts[3 - bi - bj];
    let sa = exact::orientation(apts[ai], apts[aj], third_a);
    let sb = exact::orientation(apts[ai], apts[aj], third_b);
    if sa == Ordering::Equal || sb == Ordering::Equal {
        issues.add(IncidenceIssue::DegenerateOrUnassessed);
    } else if sa == sb {
        issues.add(IncidenceIssue::OverlappingCoplanar);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        mesh::TriangleMesh,
        slicing::{section_mesh_z, SectionLimits, SectionTransform},
    };

    fn shared_edge_raw() -> MeshSection {
        let mesh = TriangleMesh::try_new(
            vec![
                Point3::new(0.0, 0.0, -1.0),
                Point3::new(4.0, 0.0, 1.0),
                Point3::new(0.0, 4.0, 1.0),
                Point3::new(0.0, -4.0, 1.0),
            ],
            vec![[0, 1, 2], [1, 0, 3]],
        )
        .unwrap();
        section_mesh_z(
            &mesh,
            0.0,
            SectionTransform::Identity,
            SectionLimits::default(),
        )
        .unwrap()
    }

    #[test]
    fn conflicting_shared_edge_observations_are_retained_without_a_representative() {
        let mut raw = shared_edge_raw();
        let record = raw.events[1];
        let TrianglePlaneEvent::Segment {
            ref mut endpoints, ..
        } = raw.events[1].intersection.event
        else {
            panic!("segment")
        };
        let endpoint = endpoints
            .iter_mut()
            .find(|p| feature(&record, p.origin) == SourceFeature::Edge([0, 1]))
            .unwrap();
        endpoint.position.x = f64::from_bits(endpoint.position.x.to_bits() + 1);
        let before = raw.events.clone();
        let view = normalize_section(&raw, NormalizationLimits::default()).unwrap();
        let group = view
            .features()
            .iter()
            .find(|g| g.feature == SourceFeature::Edge([0, 1]))
            .unwrap();
        assert_eq!(group.coordinates, CoordinateAgreement::Conflict);
        assert_eq!(group.observations.len(), 2);
        assert_eq!(view.summary().coordinate_conflicts, 1);
        assert!(view
            .groups()
            .iter()
            .all(|g| g.ownership == Ownership::Unresolved
                && g.issues.contains(IncidenceIssue::CoordinateConflict)));
        assert_eq!(raw.events, before);
    }

    #[test]
    fn corrupted_provenance_and_coordinates_are_separate_typed_errors() {
        let mut raw = shared_edge_raw();
        raw.events[1].source_face = 0;
        assert!(matches!(
            normalize_section(&raw, NormalizationLimits::default()),
            Err(NormalizationError::InvalidProvenance { event: Some(1) })
        ));
        let mut raw = shared_edge_raw();
        let TrianglePlaneEvent::Segment {
            ref mut endpoints, ..
        } = raw.events[0].intersection.event
        else {
            panic!("segment")
        };
        endpoints[0].position.x = f64::NAN;
        assert!(matches!(
            normalize_section(&raw, NormalizationLimits::default()),
            Err(NormalizationError::InvalidCoordinate { event: Some(0) })
        ));
        let mut raw = shared_edge_raw();
        let TrianglePlaneEvent::Segment {
            ref mut endpoints, ..
        } = raw.events[0].intersection.event
        else {
            panic!("segment")
        };
        endpoints[1].origin = endpoints[0].origin;
        assert!(matches!(
            normalize_section(&raw, NormalizationLimits::default()),
            Err(NormalizationError::InvalidProvenance { event: Some(0) })
        ));
    }

    #[test]
    fn checked_sizes_and_counters_fail_before_allocating() {
        assert_eq!(
            bytes::<EndpointObservation>(usize::MAX),
            Err(NormalizationError::SizeOverflow)
        );
        assert_eq!(
            bytes::<u64>(usize::try_from(isize::MAX).unwrap()),
            Err(NormalizationError::SizeOverflow)
        );
        assert_eq!(add(usize::MAX, 1), Err(NormalizationError::SizeOverflow));
    }
}
