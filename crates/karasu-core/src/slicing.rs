//! Deterministic raw sections at one explicit world-space Z plane (mm).
//!
//! This reference full scan preserves source-face multiplicity and the unchanged M3.1 event model.
//! It does not assign shared-edge ownership, weld coordinates, deduplicate segments, repair input,
//! reconstruct contours, certify solids, or choose layer planes. Source geometry stays immutable.
//! [`normalize`] adds a separate non-destructive incidence view with conservative local coplanar
//! ownership. The raw traversal functions and event semantics remain unchanged.
//! [`graph`] derives complete and locally eligible adjacency from normalization without choosing paths.
//! [`paths`] extracts conservative ordered trails/cycle candidates from that graph, not filled regions.
//!
//! ```
//! use karasu_core::{geometry::Point3, mesh::TriangleMesh};
//! use karasu_core::slicing::{section_mesh_z, SectionLimits, SectionTransform};
//! let mesh = TriangleMesh::try_new(vec![
//!     Point3::new(0.0, 0.0, -1.0), Point3::new(4.0, 0.0, 1.0),
//!     Point3::new(0.0, 4.0, 1.0)], vec![[0, 1, 2]])?;
//! let section = section_mesh_z(&mesh, 0.0, SectionTransform::Identity, SectionLimits::default())?;
//! assert_eq!(section.events().len(), 1);
//! assert_eq!(section.events()[0].source_face, 0);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use std::{error::Error, fmt};

use crate::cancellation::{Cancellation, Cancelled, Checkpoint};
use crate::geometry::section::{
    intersect_triangle_z, CornerId, EndpointOrigin, LocalEdge, TriangleAreaObservation,
    TrianglePlaneError, TrianglePlaneEvent, TrianglePlaneIntersection,
};
use crate::geometry::{Affine3, ObjectTransform, Point3, TransformError};
use crate::mesh::{Triangle, TriangleMesh};

pub mod area;
pub mod construct;
pub mod engine;
pub(crate) mod exact;
pub mod graph;
pub mod grid;
pub mod normalize;
pub mod paths;
pub mod regions;
pub mod schedule;
pub mod validity;
pub mod winding;

/// Explicit source-to-world mapping. Object evaluation is direct, never silently flattened.
/// Negative determinants and singular maps are accepted without reversing source winding.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SectionTransform<'a> {
    /// Source coordinates already name world space; no arithmetic is needed.
    Identity,
    /// Existing M2 local-pivot order: subtract pivot, scale, rotate, restore pivot, translate.
    Object(&'a ObjectTransform),
    /// Existing M2 checked affine evaluation, including caller-composed shear or singularity.
    Affine(&'a Affine3),
}

impl SectionTransform<'_> {
    fn point(self, point: Point3) -> Result<Point3, TransformError> {
        match self {
            Self::Identity => Ok(point),
            Self::Object(transform) => transform.transform_point(point),
            Self::Affine(transform) => transform.transform_point(point),
        }
    }
}

/// Independent M3 caps, not topology limits or a total-process memory guarantee.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SectionLimits {
    /// Maximum source faces scanned, including non-intersecting/degenerate/duplicate faces.
    pub max_faces: usize,
    /// Maximum stored non-None events (one at most per source face).
    pub max_events: usize,
    /// Maximum requested vector payload capacity in bytes, using `size_of::<FaceSection>()`.
    /// Allocator overhead, transient old/new allocations during growth, and source storage are
    /// excluded. `try_reserve_exact` may allocate more than requested; this is not a hard RSS cap.
    pub max_event_bytes: usize,
}

impl Default for SectionLimits {
    fn default() -> Self {
        Self {
            max_faces: 1_000_000,
            max_events: 1_000_000,
            max_event_bytes: 256 * 1024 * 1024,
        }
    }
}

/// Source-mesh incidence, retaining local identity even for repeated vertex indices or coordinates.
/// The owning [`FaceSection::source_face`] disambiguates separate uses of a shared feature.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeshEndpointOrigin {
    /// Original face corner and its mesh vertex-buffer index.
    Vertex {
        /// Corner in original source winding order.
        corner: CornerId,
        /// Source vertex-buffer index, never a coordinate-derived identity.
        vertex: u32,
    },
    /// Original local edge and its directed source index pair; collapsed pairs remain valid here.
    Edge {
        /// Edge in original local order 01, 12, 20.
        edge: LocalEdge,
        /// Directed mesh vertex pair in that local order, not endpoint output order.
        vertices: [u32; 2],
    },
}

/// One non-None raw event. No source coordinates are duplicated beyond the event's point payloads.
/// Guarantees apply to traversal-produced records, not caller-fabricated public field values.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FaceSection {
    /// Zero-based face-buffer index in the borrowed source mesh (not a persistent asset ID).
    pub source_face: usize,
    /// Original indexed face in winding order; sufficient to resolve every local source identity.
    pub source_vertices: Triangle,
    /// Unchanged M3.1 sides, context and event for the three computed world points.
    /// Context observes post-transform positions, not the original source-space rank.
    pub intersection: TrianglePlaneIntersection,
}

impl FaceSection {
    /// Resolves a local corner without coordinate lookup or merging repeated indices.
    #[must_use]
    pub fn source_vertex(&self, corner: CornerId) -> u32 {
        self.source_vertices[corner as usize]
    }

    /// Resolves a local edge to its directed source indices, including collapsed pairs.
    /// This is incidence, not a topology `EdgeKey` or a shared-edge ownership decision.
    #[must_use]
    pub fn source_edge(&self, edge: LocalEdge) -> [u32; 2] {
        let [a, b] = match edge {
            LocalEdge::Edge01 => [0, 1],
            LocalEdge::Edge12 => [1, 2],
            LocalEdge::Edge20 => [2, 0],
        };
        [self.source_vertices[a], self.source_vertices[b]]
    }

    /// Promotes an endpoint's local identity to indexed mesh incidence. Keep `source_face` with it:
    /// coordinate equality, shared index pairs, or local reordering do not erase separate face uses.
    #[must_use]
    pub fn endpoint_origin(&self, origin: EndpointOrigin) -> MeshEndpointOrigin {
        match origin {
            EndpointOrigin::Vertex(corner) => MeshEndpointOrigin::Vertex {
                corner,
                vertex: self.source_vertex(corner),
            },
            EndpointOrigin::Edge(edge) => MeshEndpointOrigin::Edge {
                edge,
                vertices: self.source_edge(edge),
            },
        }
    }
}

/// Bounded totals for every scanned face, including omitted None events. No diagnostic detail log.
/// Area observations are qualified floating-point context, never exact-rank/printability proofs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SectionSummary {
    /// Every source face is accounted for once on successful publication: evaluated, or proven
    /// non-intersecting by an opt-in Z index (see `prefiltered_faces`).
    pub source_faces: usize,
    /// None events omitted from storage, including prefiltered faces.
    pub non_intersecting_faces: usize,
    /// Faces whose computed world Z range excludes the plane, skipped by an opt-in Z index without
    /// kernel evaluation (zero for a full scan). Area observations count evaluated faces only.
    pub prefiltered_faces: usize,
    /// Non-None records stored, including duplicates and all degeneracies.
    pub retained_events: usize,
    /// Faces observed with at least one equal pair of computed world positions.
    pub repeated_position_faces: usize,
    /// Faces with zero computed cross (not a certified exact-collinearity count).
    pub zero_computed_cross_faces: usize,
    /// Faces with a nonzero computed cross (not an exact-rank certificate).
    pub nonzero_computed_cross_faces: usize,
    /// Faces whose area observation was outside the kernel's representable range.
    pub unavailable_area_faces: usize,
}

impl SectionSummary {
    fn observe(&mut self, intersection: TrianglePlaneIntersection) -> Result<(), SectionError> {
        increment(&mut self.source_faces)?;
        let counter = match intersection.context.area_observation {
            TriangleAreaObservation::RepeatedPosition => &mut self.repeated_position_faces,
            TriangleAreaObservation::ZeroComputedCross => &mut self.zero_computed_cross_faces,
            TriangleAreaObservation::NonzeroComputedCross => &mut self.nonzero_computed_cross_faces,
            TriangleAreaObservation::UnavailableRange => &mut self.unavailable_area_faces,
        };
        increment(counter)?;
        if matches!(intersection.event, TrianglePlaneEvent::None) {
            increment(&mut self.non_intersecting_faces)
        } else {
            increment(&mut self.retained_events)
        }
    }
}

/// Complete immutable derived result. Source IDs refer to the input mesh; no mesh is copied or
/// retained by ownership. Keep the corresponding source asset if later interpreting those IDs.
#[derive(Debug, PartialEq)]
pub struct MeshSection {
    plane_z: f64,
    events: Vec<FaceSection>,
    summary: SectionSummary,
}

impl MeshSection {
    pub(crate) fn event_capacity(&self) -> usize {
        self.events.capacity()
    }

    /// Explicit requested world-space plane Z (mm), with numerical zero normalized to +0.
    #[must_use]
    pub const fn plane_z(&self) -> f64 {
        self.plane_z
    }

    /// All non-None raw events in strictly ascending source-face order. No duplicate suppression.
    #[must_use]
    pub fn events(&self) -> &[FaceSection] {
        &self.events
    }

    /// Complete processing totals, including contexts of non-intersecting faces.
    #[must_use]
    pub const fn summary(&self) -> SectionSummary {
        self.summary
    }
}

/// A separately bounded mesh-section resource.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SectionResource {
    /// Original faces, even if no events would be emitted.
    Faces,
    /// Retained non-None event count.
    Events,
    /// Requested event-vector payload bytes.
    EventBytes,
    /// Requested bytes of an opt-in face Z index.
    IndexBytes,
}

/// Failed raw mesh section; no successful partial result escapes.
#[derive(Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum SectionError {
    /// Cooperative cancellation at the observed phase checkpoint.
    Cancelled(Cancelled),
    /// Requested world-space Z is NaN or infinite, even for an empty mesh.
    NonFinitePlane,
    /// A configured work/storage cap was exceeded.
    LimitExceeded {
        /// Resource whose cap was exceeded.
        resource: SectionResource,
        /// Caller-configured maximum.
        limit: usize,
    },
    /// Checked count, byte-size or capacity arithmetic cannot fit the platform.
    SizeOverflow,
    /// Fallible event-buffer reservation failed; not invalid geometry or cancellation.
    Allocation,
    /// M2 point evaluation failed on this face/corner, before local intersection.
    Transform {
        /// First failing source face in traversal order.
        face: usize,
        /// First failing corner in original winding order.
        corner: CornerId,
        /// Source vertex-buffer index.
        vertex: u32,
        /// Underlying M2 transform error, unchanged.
        source: TransformError,
    },
    /// Unchanged M3.1 typed error with the original indexed face for source-edge resolution.
    Intersection {
        /// First failing source face in traversal order.
        face: usize,
        /// Source index triple in winding order.
        vertices: Triangle,
        /// Local numerical failure; its edge/corner maps through `vertices`.
        source: TrianglePlaneError,
    },
}

impl fmt::Display for SectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled(cause) => cause.fmt(f),
            Self::NonFinitePlane => write!(f, "mesh section world plane Z must be finite"),
            Self::LimitExceeded { resource, limit } => {
                write!(f, "mesh section {resource:?} limit exceeded ({limit})")
            }
            Self::SizeOverflow => write!(f, "mesh section count or capacity arithmetic overflow"),
            Self::Allocation => write!(f, "unable to reserve mesh section event storage"),
            Self::Transform {
                face,
                corner,
                vertex,
                source,
            } => write!(
                f,
                "mesh section face {face}, corner {corner:?}, vertex {vertex}: {source}"
            ),
            Self::Intersection { face, source, .. } => {
                write!(f, "mesh section face {face}: {source}")
            }
        }
    }
}

impl Error for SectionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Cancelled(cause) => Some(cause),
            Self::Transform { source, .. } => Some(source),
            Self::Intersection { source, .. } => Some(source),
            _ => None,
        }
    }
}

impl From<Cancelled> for SectionError {
    fn from(value: Cancelled) -> Self {
        Self::Cancelled(value)
    }
}

/// Scans an immutable mesh at one finite world-space Z (mm) with explicit mapping and M3 limits.
/// See [`section_mesh_z_with_cancellation`] for ordering, storage, and numerical semantics.
///
/// # Errors
/// Returns a typed invalid-plane, resource, allocation, transform, or kernel failure.
pub fn section_mesh_z(
    mesh: &TriangleMesh,
    plane_z: f64,
    transform: SectionTransform<'_>,
    limits: SectionLimits,
) -> Result<MeshSection, SectionError> {
    section_mesh_z_with_cancellation(mesh, plane_z, transform, limits, &mut Cancellation::never())
}

/// Full source-face scan: three checked world-point evaluations then the unchanged M3.1 kernel.
/// No source-Z rejection, transformed cache, topology exclusion, sorting or geometry repair occurs.
/// Unused vertices are not evaluated. Reflections/singular maps preserve source incidence; kernel
/// context describes computed world geometry, including new rank loss and rounding coincidence.
/// Local payload ordering is preserved, not re-sorted by promoted mesh IDs.
///
/// Cost is O(F) point/kernel work, amortized O(S) vector growth, O(S) requested output capacity and
/// O(1) scratch, for F faces/S retained events. Fallible geometric growth requests at most the
/// smaller of F, `max_events` and `max_event_bytes` / record size; counts/byte arithmetic are checked.
/// No geometry/diagnostic-detail scratch buffer or initial worst-case reservation is allocated.
///
/// Checkpoints: `SectionStart`; `SectionFaces` at 0,256,512,...; `SectionStorage` before/after each vector
/// growth; `SectionComplete` before publishing success (also for empty/no-hit input). An allocation,
/// reallocation/copy, bounded individual point/kernel call and cleanup cannot be interrupted inside.
/// These are synchronous work checkpoints, not progress or wall-clock latency promises. No partial
/// section escapes cancellation, resource or numerical failure. Exact sequences may change by release.
///
/// # Errors
/// Observed cancellation wins at its checkpoint. Otherwise precedence is finite plane, face cap,
/// then faces in source order: corners 0/1/2 under the selected transform, local kernel, totals and
/// event storage caps/reservation. Ordinary errors before the next poll remain ordinary errors.
/// The first failure is returned without fallback or a second polling pass.
pub fn section_mesh_z_with_cancellation(
    mesh: &TriangleMesh,
    plane_z: f64,
    transform: SectionTransform<'_>,
    limits: SectionLimits,
    cancellation: &mut Cancellation<'_>,
) -> Result<MeshSection, SectionError> {
    cancellation.check(Checkpoint::SectionStart)?;
    if !plane_z.is_finite() {
        return Err(SectionError::NonFinitePlane);
    }
    enforce(
        mesh.triangles().len(),
        limits.max_faces,
        SectionResource::Faces,
    )?;
    let plane_z = if plane_z == 0.0 { 0.0 } else { plane_z };
    let mut result = MeshSection {
        plane_z,
        events: Vec::new(),
        summary: SectionSummary::default(),
    };
    for (face, &vertices) in mesh.triangles().iter().enumerate() {
        cancellation.poll(Checkpoint::SectionFaces, face)?;
        scan_face(
            mesh,
            face,
            vertices,
            plane_z,
            transform,
            limits,
            &mut result,
            cancellation,
        )?;
    }
    cancellation.check(Checkpoint::SectionComplete)?;
    Ok(result)
}

/// Sweep index of per-face world Z ranges for strictly increasing section planes.
///
/// Ranges are the extrema of exactly the world points the full scan evaluates, so a face outside
/// `[z_min, z_max]` has all corners strictly above or below the plane and the kernel would report
/// no event for it. Candidates are returned in ascending source order, so an indexed section is
/// identical to a full scan apart from its summary's prefilter accounting.
#[derive(Debug)]
pub(crate) struct FaceZSweep {
    index: FaceZIndex,
    cursor: FaceZCursor,
}

/// Immutable part of the sweep index: per-face world Z ranges and faces sorted by lower bound.
/// Shareable between threads; each sweep position lives in its own [`FaceZCursor`].
#[derive(Debug)]
pub(crate) struct FaceZIndex {
    ranges: Vec<[f64; 2]>,
    by_min: Vec<u32>,
}

/// Mutable sweep position over a [`FaceZIndex`]: efficient for increasing planes, restarting on a
/// non-increasing one.
#[derive(Debug)]
pub(crate) struct FaceZCursor {
    next: usize,
    active: Vec<u32>,
    last: f64,
}

impl FaceZSweep {
    /// Evaluates every face's world corners once, in source order, with full-scan error context.
    pub(crate) fn build(
        mesh: &TriangleMesh,
        transform: SectionTransform<'_>,
        max_bytes: usize,
        cancellation: &mut Cancellation<'_>,
    ) -> Result<Self, SectionError> {
        let index = FaceZIndex::build(mesh, transform, max_bytes, cancellation)?;
        let cursor = FaceZCursor::new(&index)?;
        Ok(Self { index, cursor })
    }

    /// The shared index and this sweep's cursor.
    pub(crate) fn parts(&mut self) -> (&FaceZIndex, &mut FaceZCursor) {
        (&self.index, &mut self.cursor)
    }
}

impl FaceZCursor {
    /// A cursor before the first plane, with a small fallible initial active-list reservation.
    pub(crate) fn new(index: &FaceZIndex) -> Result<Self, SectionError> {
        let mut active = Vec::new();
        active
            .try_reserve_exact(index.ranges.len().min(1024))
            .map_err(|_| SectionError::Allocation)?;
        Ok(Self {
            next: 0,
            active,
            last: f64::NEG_INFINITY,
        })
    }

    /// Ascending faces whose Z range contains `z`. Non-increasing queries restart the sweep.
    fn advance(&mut self, index: &FaceZIndex, z: f64) -> Result<Vec<u32>, SectionError> {
        // Planes are finite; a non-increasing plane restarts the sweep from the first face.
        if z <= self.last {
            self.next = 0;
            self.active.clear();
        }
        self.last = z;
        let ranges = &index.ranges;
        self.active.retain(|&f| ranges[f as usize][1] >= z);
        while let Some(&face) = index.by_min.get(self.next) {
            let [low, high] = ranges[face as usize];
            if low > z {
                break;
            }
            if high >= z {
                self.active
                    .try_reserve(1)
                    .map_err(|_| SectionError::Allocation)?;
                self.active.push(face);
            }
            self.next += 1;
        }
        let mut candidates = Vec::new();
        candidates
            .try_reserve_exact(self.active.len())
            .map_err(|_| SectionError::Allocation)?;
        candidates.extend_from_slice(&self.active);
        candidates.sort_unstable();
        Ok(candidates)
    }
}

impl FaceZIndex {
    /// Requested index bytes per face: one range, one ordering entry, and either the parallel
    /// sort's merge entry or, later, one active slot (the two never coexist).
    const BYTES_PER_FACE: usize = size_of::<[f64; 2]>() + 2 * size_of::<u32>();

    /// Evaluates every face's world corners once, in source order, with full-scan error context.
    pub(crate) fn build(
        mesh: &TriangleMesh,
        transform: SectionTransform<'_>,
        max_bytes: usize,
        cancellation: &mut Cancellation<'_>,
    ) -> Result<Self, SectionError> {
        let faces = mesh.triangles().len();
        if u32::try_from(faces).is_err() {
            return Err(SectionError::SizeOverflow);
        }
        let bytes = faces
            .checked_mul(Self::BYTES_PER_FACE)
            .ok_or(SectionError::SizeOverflow)?;
        enforce(bytes, max_bytes, SectionResource::IndexBytes)?;
        let mut ranges = Vec::new();
        ranges
            .try_reserve_exact(faces)
            .map_err(|_| SectionError::Allocation)?;
        for (face, &vertices) in mesh.triangles().iter().enumerate() {
            cancellation.poll(Checkpoint::SectionFaces, face)?;
            let mut range = [f64::INFINITY, f64::NEG_INFINITY];
            for (index, corner) in [CornerId::Corner0, CornerId::Corner1, CornerId::Corner2]
                .into_iter()
                .enumerate()
            {
                let vertex = vertices[index];
                let z = transform
                    .point(mesh.vertices()[vertex as usize])
                    .map_err(|source| SectionError::Transform {
                        face,
                        corner,
                        vertex,
                        source,
                    })?
                    .z;
                range = [range[0].min(z), range[1].max(z)];
            }
            ranges.push(range);
        }
        let mut by_min = Vec::new();
        by_min
            .try_reserve_exact(faces)
            .map_err(|_| SectionError::Allocation)?;
        let count = u32::try_from(faces).map_err(|_| SectionError::SizeOverflow)?;
        by_min.extend(0..count);
        cancellation.check(Checkpoint::SectionStorage)?;
        // On every core: the keys are unique, so the order is the sequential one for any
        // thread count.
        crate::parallel::sort_by_key(&mut by_min, |&face| {
            (total_order_key(ranges[face as usize][0]), face)
        });
        cancellation.check(Checkpoint::SectionStorage)?;
        Ok(Self { ranges, by_min })
    }
}

/// An integer key that orders values exactly as [`f64::total_cmp`] does.
fn total_order_key(value: f64) -> i64 {
    #[allow(clippy::cast_possible_wrap)] // The bits, reinterpreted.
    let bits = value.to_bits() as i64;
    // Negative values have their magnitude bits flipped, so they order backwards.
    #[allow(clippy::cast_sign_loss, clippy::cast_possible_wrap)]
    let flip = (((bits >> 63) as u64) >> 1) as i64;
    bits ^ flip
}

/// Sorted, distinct world Z of every face corner, exactly as the section scan evaluates them
/// (negative zero canonicalized to zero), for planes that must avoid every vertex height.
///
/// Corners are visited in face order but each vertex is evaluated once, at its first use, so a
/// transform failure carries the same first failing face/corner context as the scan; unused
/// vertices are never evaluated. At most `max_bytes` are requested (8 per vertex plus one bit per
/// vertex of visit marks); every reservation is fallible.
pub(crate) fn vertex_heights(
    mesh: &TriangleMesh,
    transform: SectionTransform<'_>,
    max_bytes: usize,
    cancellation: &mut Cancellation<'_>,
) -> Result<Vec<f64>, SectionError> {
    let vertices = mesh.vertices().len();
    let words = vertices.div_ceil(64);
    let bytes = vertices
        .checked_mul(size_of::<f64>())
        .and_then(|n| n.checked_add(words.checked_mul(size_of::<u64>())?))
        .ok_or(SectionError::SizeOverflow)?;
    enforce(bytes, max_bytes, SectionResource::IndexBytes)?;
    let mut seen = Vec::new();
    seen.try_reserve_exact(words)
        .map_err(|_| SectionError::Allocation)?;
    seen.resize(words, 0u64);
    let mut heights = Vec::new();
    heights
        .try_reserve_exact(vertices)
        .map_err(|_| SectionError::Allocation)?;
    for (face, &corners) in mesh.triangles().iter().enumerate() {
        cancellation.poll(Checkpoint::SectionFaces, face)?;
        for (index, corner) in [CornerId::Corner0, CornerId::Corner1, CornerId::Corner2]
            .into_iter()
            .enumerate()
        {
            let vertex = corners[index];
            let (word, bit) = (vertex as usize / 64, 1u64 << (vertex % 64));
            if seen[word] & bit != 0 {
                continue;
            }
            seen[word] |= bit;
            let z = transform
                .point(mesh.vertices()[vertex as usize])
                .map_err(|source| SectionError::Transform {
                    face,
                    corner,
                    vertex,
                    source,
                })?
                .z;
            heights.push(if z == 0.0 { 0.0 } else { z });
        }
    }
    drop(seen);
    cancellation.check(Checkpoint::SectionStorage)?;
    heights.sort_unstable_by(f64::total_cmp);
    heights.dedup();
    heights.shrink_to_fit();
    cancellation.check(Checkpoint::SectionStorage)?;
    Ok(heights)
}

/// A plane in the same slab as `z` that equals no entry of the sorted, distinct `heights`.
///
/// `z` itself when it is no vertex height. Otherwise the middle of the vertex-free interval just
/// below `z`, which reaches down to the next lower vertex height or to `z - half`, whichever is
/// higher: no vertex lies between the result and `z`, so the section has the combinatorics of the
/// section just below `z`, while its crossing points stay as far from the vertex heights as that
/// interval allows. Only when the interval holds no binary64 value strictly inside does the plane
/// step down one representable value at a time instead.
pub(crate) fn below_vertex_heights(z: f64, half: f64, heights: &[f64]) -> f64 {
    let z = if z == 0.0 { 0.0 } else { z };
    let is_height = |value: f64| heights.binary_search_by(|h| h.total_cmp(&value)).is_ok();
    let Ok(position) = heights.binary_search_by(|h| h.total_cmp(&z)) else {
        return z;
    };
    let floor = z - half;
    let lower = position
        .checked_sub(1)
        .map_or(floor, |below| heights[below].max(floor));
    let middle = f64::midpoint(lower, z);
    if lower < middle && middle < z {
        return middle;
    }
    let mut plane = z;
    while is_height(plane) {
        plane = plane.next_down();
    }
    plane
}

/// Indexed variant of [`section_mesh_z_with_cancellation`]: identical events in identical order,
/// evaluating only faces whose world Z range contains the plane. `max_faces` bounds evaluated
/// candidates. Transform failures of any face surface when the index is built.
pub(crate) fn section_mesh_z_indexed_with_cancellation(
    mesh: &TriangleMesh,
    plane_z: f64,
    transform: SectionTransform<'_>,
    (index, cursor): (&FaceZIndex, &mut FaceZCursor),
    limits: SectionLimits,
    cancellation: &mut Cancellation<'_>,
) -> Result<MeshSection, SectionError> {
    cancellation.check(Checkpoint::SectionStart)?;
    if !plane_z.is_finite() {
        return Err(SectionError::NonFinitePlane);
    }
    let plane_z = if plane_z == 0.0 { 0.0 } else { plane_z };
    let candidates = cursor.advance(index, plane_z)?;
    enforce(candidates.len(), limits.max_faces, SectionResource::Faces)?;
    let mut result = MeshSection {
        plane_z,
        events: Vec::new(),
        summary: SectionSummary::default(),
    };
    for (position, &face) in candidates.iter().enumerate() {
        cancellation.poll(Checkpoint::SectionFaces, position)?;
        let face = face as usize;
        let vertices = mesh.triangles()[face];
        scan_face(
            mesh,
            face,
            vertices,
            plane_z,
            transform,
            limits,
            &mut result,
            cancellation,
        )?;
    }
    let skipped = mesh.triangles().len() - candidates.len();
    let summary = &mut result.summary;
    summary.prefiltered_faces = skipped;
    summary.source_faces = summary
        .source_faces
        .checked_add(skipped)
        .ok_or(SectionError::SizeOverflow)?;
    summary.non_intersecting_faces = summary
        .non_intersecting_faces
        .checked_add(skipped)
        .ok_or(SectionError::SizeOverflow)?;
    cancellation.check(Checkpoint::SectionComplete)?;
    Ok(result)
}

#[allow(clippy::too_many_arguments)] // One face of the shared full/indexed scan body.
fn scan_face(
    mesh: &TriangleMesh,
    face: usize,
    vertices: [u32; 3],
    plane_z: f64,
    transform: SectionTransform<'_>,
    limits: SectionLimits,
    result: &mut MeshSection,
    cancellation: &mut Cancellation<'_>,
) -> Result<(), SectionError> {
    let mut world = [Point3::new(0.0, 0.0, 0.0); 3];
    for (index, corner) in [CornerId::Corner0, CornerId::Corner1, CornerId::Corner2]
        .into_iter()
        .enumerate()
    {
        let vertex = vertices[index];
        // TriangleMesh validates finite source coordinates and every index at construction.
        world[index] = transform
            .point(mesh.vertices()[vertex as usize])
            .map_err(|source| SectionError::Transform {
                face,
                corner,
                vertex,
                source,
            })?;
    }
    let intersection =
        intersect_triangle_z(world, plane_z).map_err(|source| SectionError::Intersection {
            face,
            vertices,
            source,
        })?;
    result.summary.observe(intersection)?;
    if !matches!(intersection.event, TrianglePlaneEvent::None) {
        reserve_event(
            &mut result.events,
            mesh.triangles().len(),
            limits,
            cancellation,
        )?;
        result.events.push(FaceSection {
            source_face: face,
            source_vertices: vertices,
            intersection,
        });
    }
    Ok(())
}

fn increment(value: &mut usize) -> Result<(), SectionError> {
    *value = value.checked_add(1).ok_or(SectionError::SizeOverflow)?;
    Ok(())
}

fn enforce(value: usize, limit: usize, resource: SectionResource) -> Result<(), SectionError> {
    if value > limit {
        Err(SectionError::LimitExceeded { resource, limit })
    } else {
        Ok(())
    }
}

fn event_bytes(count: usize) -> Result<usize, SectionError> {
    count
        .checked_mul(size_of::<FaceSection>())
        .filter(|&bytes| isize::try_from(bytes).is_ok())
        .ok_or(SectionError::SizeOverflow)
}

fn reserve_event(
    events: &mut Vec<FaceSection>,
    faces: usize,
    limits: SectionLimits,
    cancellation: &mut Cancellation<'_>,
) -> Result<(), SectionError> {
    let next = events
        .len()
        .checked_add(1)
        .ok_or(SectionError::SizeOverflow)?;
    enforce(next, limits.max_events, SectionResource::Events)?;
    enforce(
        event_bytes(next)?,
        limits.max_event_bytes,
        SectionResource::EventBytes,
    )?;
    if next > events.capacity() {
        let ceiling = faces
            .min(limits.max_events)
            .min(limits.max_event_bytes / size_of::<FaceSection>());
        // Clamp before doubling so even extremely large caller caps cannot overflow growth.
        let capacity = events
            .capacity()
            .min(ceiling / 2)
            .checked_mul(2)
            .ok_or(SectionError::SizeOverflow)?
            .max(next);
        event_bytes(capacity)?;
        cancellation.check(Checkpoint::SectionStorage)?;
        events
            .try_reserve_exact(capacity - events.len())
            .map_err(|_| SectionError::Allocation)?;
        cancellation.check(Checkpoint::SectionStorage)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_total_order_key_orders_as_total_cmp() {
        let values = [
            f64::NEG_INFINITY,
            -1e300,
            -1.5,
            -f64::MIN_POSITIVE,
            -0.0,
            0.0,
            f64::MIN_POSITIVE,
            1.5,
            1e300,
            f64::INFINITY,
            f64::NAN,
            -f64::NAN,
        ];
        for a in values {
            for b in values {
                assert_eq!(
                    total_order_key(a).cmp(&total_order_key(b)),
                    a.total_cmp(&b),
                    "{a} {b}"
                );
            }
        }
    }

    #[test]
    fn unrepresentable_payload_counts_and_counter_overflow_are_typed() {
        assert_eq!(event_bytes(usize::MAX), Err(SectionError::SizeOverflow));
        let count = usize::try_from(isize::MAX).unwrap() / size_of::<FaceSection>() + 1;
        assert_eq!(event_bytes(count), Err(SectionError::SizeOverflow));
        let mut total = usize::MAX;
        assert_eq!(increment(&mut total), Err(SectionError::SizeOverflow));
        assert_eq!(total, usize::MAX);
    }

    #[test]
    fn planes_leave_vertex_heights_for_the_middle_of_the_free_interval_below() {
        use super::below_vertex_heights;
        let heights = [0.0, 0.25, 0.375, 1.0];
        // Not a height: unchanged (negative zero canonicalized).
        assert_eq!(below_vertex_heights(0.5, 0.125, &heights), 0.5);
        assert_eq!(
            below_vertex_heights(-0.0, 0.125, &[0.25]).to_bits(),
            0.0f64.to_bits()
        );
        // A height with the next lower one inside the slab: middle of the gap between them.
        assert_eq!(below_vertex_heights(0.375, 0.125, &heights), 0.3125);
        // A height whose slab bottom is higher than the next lower height: middle above it.
        assert_eq!(below_vertex_heights(1.0, 0.125, &heights), 0.9375);
        // The lowest height: the slab bottom bounds the interval.
        assert_eq!(below_vertex_heights(0.0, 0.5, &heights), -0.25);
        // Adjacent binary64 heights leave no room inside: step down past all of them.
        let z = 2.0f64;
        let tight = [z.next_down().next_down(), z.next_down(), z];
        let plane = below_vertex_heights(z, 0.0, &tight);
        assert!(plane < tight[0] && !tight.contains(&plane), "{plane}");
    }
}
