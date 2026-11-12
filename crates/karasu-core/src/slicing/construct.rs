//! Exact material composition: source parity with union or body-minus-cutters classification.
//!
//! Original simple integer rings are split into a rational arrangement. Symbolic infinitesimal
//! classification preserves arbitrarily small features without rounding or a physical epsilon.
//! Contact vertices remain shared by edge-disjoint cycles; strict source diagnostics are separate.
//!
//! ```
//! use karasu_core::slicing::construct::{MaterialRing, ConstructLimits, union_material_rings};
//! let first = [[0,0],[4,0],[4,4],[0,4]];
//! let second = [[2,0],[6,0],[6,4],[2,4]];
//! let material = union_material_rings(&[
//!     MaterialRing { vertices: &first, material: 0, source_path: 0 },
//!     MaterialRing { vertices: &second, material: 1, source_path: 1 },
//! ], ConstructLimits::default())?;
//! assert_eq!(material.regions().len(), 1);
//! assert_eq!(material.rings().len(), 1);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use super::{area::CycleOrientation, regions::BoundaryRole};
use crate::cancellation::{Cancellation, Cancelled, Checkpoint};
use std::{cmp::Ordering, error::Error, fmt, ops::Range};
mod exact;
mod represented;
pub use exact::ExactPoint;
use exact::{intersection, on_segment_integer, orient_integer, SymbolicPoint};
pub use represented::{
    difference_represented_material_rings, difference_represented_material_rings_with_cancellation,
    prepared_represented_material_rings, prepared_represented_material_rings_with_cancellation,
    union_represented_material_rings, union_represented_material_rings_with_cancellation,
    RepresentedMaterialRing,
};

/// One individually simple closed ring; the closing vertex is implicit and never repeated.
#[derive(Clone, Copy, Debug)]
pub struct MaterialRing<'a> {
    /// Exact integer world-XY grid coordinates, each within +/-2^52.
    pub vertices: &'a [[i64; 2]],
    /// Explicit source: parity within this ID; composition selects union or body/cutter difference.
    pub material: u64,
    /// Unique caller identity preserved in constructed edge provenance.
    pub source_path: usize,
}
/// Independent input, arrangement, work and aggregate requested-capacity budgets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConstructLimits {
    /// Sum of all original ring edges.
    pub max_input_edges: usize,
    /// Unordered original segment pairs, including within-ring validation.
    pub max_pairs: usize,
    /// Exact arrangement vertices, including internal/cancelled context.
    pub max_vertices: usize,
    /// Distinct geometric atomic segments, not raw support count.
    pub max_atoms: usize,
    /// Charged inner-loop records/predicates; sorting and allocation are indivisible.
    pub max_work: usize,
    /// Aggregate requested output/scratch vector capacity increments; excludes allocator overhead.
    pub max_payload_bytes: usize,
}
impl Default for ConstructLimits {
    fn default() -> Self {
        Self {
            max_input_edges: 4096,
            max_pairs: 1_000_000,
            max_vertices: 100_000,
            max_atoms: 100_000,
            max_work: 20_000_000,
            max_payload_bytes: 256 * 1024 * 1024,
        }
    }
}
/// Resource whose independent configured cap failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConstructResource {
    /// Original edges.
    InputEdges,
    /// Original unordered segment pairs.
    Pairs,
    /// Exact arrangement vertices.
    Vertices,
    /// Distinct split segments.
    Atoms,
    /// Charged primitive work.
    Work,
    /// Aggregate requested vector capacity.
    PayloadBytes,
}
/// Invalid input is reported rather than repaired.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConstructInputIssue {
    /// A closed ring needs at least three vertices.
    TooFewVertices,
    /// An integer coordinate exceeds the approved domain.
    CoordinateRange,
    /// A represented coordinate is NaN or infinite.
    NonFiniteCoordinate,
    /// Exact common dyadic encoding exceeds the supported 60-bit magnitude domain.
    DyadicRange,
    /// Equal adjacent vertices, including an explicitly repeated closing vertex.
    CollapsedEdge,
    /// A ring touches, crosses or overlaps itself beyond prescribed adjacent endpoints.
    SelfContact,
    /// Source-path identities must be unique.
    DuplicatePath,
}
/// Atomic constructive failure; successful partial arrangements never escape.
#[derive(Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ConstructError {
    /// Observed cooperative cancellation.
    Cancelled(Cancelled),
    /// An explicit independent cap failed.
    LimitExceeded {
        /// Resource whose cap failed.
        resource: ConstructResource,
        /// Caller-selected maximum.
        limit: usize,
    },
    /// Invalid source ring with its caller identity.
    InvalidRing {
        /// Caller source-path ID.
        path: usize,
        /// Structured reason.
        reason: ConstructInputIssue,
    },
    /// Checked count/byte/host-capacity arithmetic failed.
    SizeOverflow,
    /// A fallible buffer reservation failed.
    Allocation,
    /// Defensive fixed exact arithmetic overflow.
    ArithmeticRange,
    /// Arrangement boundary/forest invariants were inconsistent.
    Topology,
}
impl fmt::Display for ConstructError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled(e) => e.fmt(f),
            Self::LimitExceeded { resource, limit } => {
                write!(f, "constructive {resource:?} limit exceeded ({limit})")
            }
            Self::InvalidRing { path, reason } => {
                write!(f, "invalid constructive source path {path}: {reason:?}")
            }
            Self::SizeOverflow => f.write_str("constructive count or capacity overflow"),
            Self::Allocation => f.write_str("unable to reserve constructive buffers"),
            Self::ArithmeticRange => f.write_str("constructive exact arithmetic range exceeded"),
            Self::Topology => f.write_str("inconsistent constructive boundary topology"),
        }
    }
}
impl Error for ConstructError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        if let Self::Cancelled(e) = self {
            Some(e)
        } else {
            None
        }
    }
}
impl From<Cancelled> for ConstructError {
    fn from(e: Cancelled) -> Self {
        Self::Cancelled(e)
    }
}
type Result<T> = std::result::Result<T, ConstructError>;

/// Exact original support identity; constructed coordinates never impersonate source vertices.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceEdge {
    /// Caller source-path identity.
    pub path: usize,
    /// Zero-based directed edge offset in that original ring.
    pub edge: usize,
}
/// A geometric split segment, retaining all coincident source supports and both side outcomes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConstructedAtom {
    /// Lexicographically increasing exact endpoints in the arrangement vertex array.
    pub endpoints: [usize; 2],
    /// Union material immediately to the left of this canonical direction.
    pub left_material: bool,
    /// Union material immediately to its right.
    pub right_material: bool,
    /// Directed boundary-edge ID, or None when explicit composition makes this atom internal.
    pub boundary: Option<usize>,
    supports: Range<usize>,
}
/// Directed material-left boundary edge referencing its full atomic provenance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConstructedEdge {
    /// Directed exact vertex IDs; no repeated or rounded endpoint construction.
    pub endpoints: [usize; 2],
    /// Complete atomic record, including coincident supports and side classification.
    pub atom: usize,
}
/// Edge-disjoint simple cycle; contact vertices may be shared with other cycles.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConstructedRing {
    /// Immediate containing cycle, allowing contact vertices under symbolic interior membership.
    pub parent: Option<usize>,
    /// Number of containing cycles.
    pub depth: usize,
    /// Material role; unlike strict boundaries, shared contact vertices are permitted.
    pub role: BoundaryRole,
    /// Exact traversal orientation from local supporting directions, not rational area summation.
    pub orientation: CycleOrientation,
    edges: Range<usize>,
}
/// One material outer cycle and its direct holes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConstructedRegion {
    outer: usize,
    holes: Range<usize>,
}
impl ConstructedRegion {
    /// Outer cycle index in the constructed ring array.
    #[must_use]
    pub const fn outer_boundary(&self) -> usize {
        self.outer
    }
}
/// Deterministic counts and charged work, not an area or a measured performance certificate.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ConstructedSummary {
    /// Supplied source rings.
    pub input_rings: usize,
    /// Original segment count.
    pub input_edges: usize,
    /// Original unordered segment pairs visited.
    pub pairs: usize,
    /// Exact vertices, including internal context.
    pub vertices: usize,
    /// Distinct atomic segments.
    pub atoms: usize,
    /// Directed composed-material boundary segments.
    pub boundary_edges: usize,
    /// Constructed simple cycles, possibly sharing contact vertices.
    pub rings: usize,
    /// Material outer regions.
    pub regions: usize,
    /// Direct hole references.
    pub holes: usize,
    /// Charged primitive work.
    pub work: usize,
    /// Aggregate requested output/scratch vector capacity increments.
    pub requested_payload_bytes: usize,
}
/// Complete exact constructed material with caller grid units or an explicit dyadic millimeter scale.
#[derive(Debug, PartialEq, Eq)]
pub struct ConstructedMaterial {
    dyadic_exponent_mm: Option<i16>,
    vertices: Vec<ExactPoint>,
    atoms: Vec<ConstructedAtom>,
    edges: Vec<ConstructedEdge>,
    supports: Vec<SourceEdge>,
    rings: Vec<ConstructedRing>,
    ring_edges: Vec<usize>,
    regions: Vec<ConstructedRegion>,
    holes: Vec<usize>,
    summary: ConstructedSummary,
    payload_bytes: usize,
}
impl ConstructedMaterial {
    /// Exact physical scale for represented-coordinate construction: multiply rational XY by
    /// `2^exponent` millimeters. `None` denotes caller-defined integer grid units.
    #[must_use]
    pub const fn dyadic_exponent_mm(&self) -> Option<i16> {
        self.dyadic_exponent_mm
    }

    /// All exact arrangement points, including internal/cancelled context.
    #[must_use]
    pub fn vertices(&self) -> &[ExactPoint] {
        &self.vertices
    }
    /// Every atom with explicit side/material disposition.
    #[must_use]
    pub fn atoms(&self) -> &[ConstructedAtom] {
        &self.atoms
    }
    /// Directed material-left boundary edges.
    #[must_use]
    pub fn edges(&self) -> &[ConstructedEdge] {
        &self.edges
    }
    /// Constructed cycles in deterministic canonical start/edge order.
    #[must_use]
    pub fn rings(&self) -> &[ConstructedRing] {
        &self.rings
    }
    /// Material regions in outer-cycle order.
    #[must_use]
    pub fn regions(&self) -> &[ConstructedRegion] {
        &self.regions
    }
    /// Complete original supports for a valid atom ID.
    #[must_use]
    pub fn atom_supports(&self, id: usize) -> Option<&[SourceEdge]> {
        self.supports.get(self.atoms.get(id)?.supports.clone())
    }
    /// Complete original supports for a valid directed boundary-edge ID.
    #[must_use]
    pub fn edge_supports(&self, id: usize) -> Option<&[SourceEdge]> {
        self.atom_supports(self.edges.get(id)?.atom)
    }
    /// Directed edge IDs of a cycle, with implicit closure.
    #[must_use]
    pub fn ring_edges(&self, id: usize) -> Option<&[usize]> {
        self.ring_edges.get(self.rings.get(id)?.edges.clone())
    }
    /// Direct hole-cycle indices of a material region.
    #[must_use]
    pub fn region_holes(&self, id: usize) -> Option<&[usize]> {
        self.holes.get(self.regions.get(id)?.holes.clone())
    }
    /// Deterministic accounting.
    #[must_use]
    pub const fn summary(&self) -> ConstructedSummary {
        self.summary
    }
    /// Retained vector element capacities only, excluding headers, scratch and allocator overhead.
    #[must_use]
    pub const fn payload_bytes(&self) -> usize {
        self.payload_bytes
    }
}

#[derive(Clone, Copy)]
struct Original {
    a: [i64; 2],
    b: [i64; 2],
    ring: usize,
    source: SourceEdge,
    material: u64,
}
#[derive(Clone, Copy)]
struct Split {
    edge: usize,
    vertex: usize,
}
#[derive(Clone, Copy)]
struct Atom {
    ends: [usize; 2],
    original: usize,
}
#[derive(Clone, Copy)]
struct Support {
    atom: usize,
    original: usize,
}
struct Arrangement {
    originals: Vec<Original>,
    points: Vec<ExactPoint>,
    splits: Vec<Split>,
    atoms: Vec<Atom>,
    supports: Vec<Support>,
}
struct Work<'a, 'c> {
    cancel: &'a mut Cancellation<'c>,
    limits: ConstructLimits,
    steps: usize,
    bytes: usize,
}
impl Work<'_, '_> {
    fn tick(&mut self, phase: Checkpoint) -> Result<()> {
        self.cancel.poll(phase, self.steps)?;
        if self.steps >= self.limits.max_work {
            return Err(ConstructError::LimitExceeded {
                resource: ConstructResource::Work,
                limit: self.limits.max_work,
            });
        }
        self.steps = self
            .steps
            .checked_add(1)
            .ok_or(ConstructError::SizeOverflow)?;
        Ok(())
    }
    fn push<T>(&mut self, vec: &mut Vec<T>, value: T) -> Result<()> {
        if vec.len() == vec.capacity() {
            self.cancel.check(Checkpoint::ConstructStorage)?;
            let next = vec
                .capacity()
                .checked_mul(2)
                .ok_or(ConstructError::SizeOverflow)?
                .max(1);
            let bytes = next
                .checked_sub(vec.capacity())
                .and_then(|n| n.checked_mul(size_of::<T>()))
                .ok_or(ConstructError::SizeOverflow)?;
            let capacity_bytes = next
                .checked_mul(size_of::<T>())
                .ok_or(ConstructError::SizeOverflow)?;
            isize::try_from(capacity_bytes).map_err(|_| ConstructError::SizeOverflow)?;
            self.bytes = self
                .bytes
                .checked_add(bytes)
                .ok_or(ConstructError::SizeOverflow)?;
            cap(
                self.bytes,
                self.limits.max_payload_bytes,
                ConstructResource::PayloadBytes,
            )?;
            vec.try_reserve_exact(next - vec.len())
                .map_err(|_| ConstructError::Allocation)?;
            self.cancel.check(Checkpoint::ConstructStorage)?;
        }
        vec.push(value);
        Ok(())
    }
}
fn cap(value: usize, limit: usize, resource: ConstructResource) -> Result<()> {
    if value > limit {
        Err(ConstructError::LimitExceeded { resource, limit })
    } else {
        Ok(())
    }
}
fn invalid(path: usize, reason: ConstructInputIssue) -> ConstructError {
    ConstructError::InvalidRing { path, reason }
}
fn arith<T>(value: std::result::Result<T, ()>) -> Result<T> {
    value.map_err(|()| ConstructError::ArithmeticRange)
}
fn point_id(a: &mut Arrangement, p: ExactPoint, w: &mut Work<'_, '_>) -> Result<usize> {
    for (id, existing) in a.points.iter().enumerate() {
        w.tick(Checkpoint::ConstructSplit)?;
        if *existing == p {
            return Ok(id);
        }
    }
    cap(
        a.points
            .len()
            .checked_add(1)
            .ok_or(ConstructError::SizeOverflow)?,
        w.limits.max_vertices,
        ConstructResource::Vertices,
    )?;
    let id = a.points.len();
    w.push(&mut a.points, p)?;
    Ok(id)
}
fn input(rings: &[MaterialRing<'_>], bound: i64, w: &mut Work<'_, '_>) -> Result<Arrangement> {
    w.cancel.check(Checkpoint::ConstructInput)?;
    let mut count = 0_usize;
    for (id, ring) in rings.iter().enumerate() {
        w.tick(Checkpoint::ConstructInput)?;
        if ring.vertices.len() < 3 {
            return Err(invalid(
                ring.source_path,
                ConstructInputIssue::TooFewVertices,
            ));
        }
        for prior in &rings[..id] {
            w.tick(Checkpoint::ConstructInput)?;
            if prior.source_path == ring.source_path {
                return Err(invalid(
                    ring.source_path,
                    ConstructInputIssue::DuplicatePath,
                ));
            }
        }
        count = count
            .checked_add(ring.vertices.len())
            .ok_or(ConstructError::SizeOverflow)?;
    }
    cap(
        count,
        w.limits.max_input_edges,
        ConstructResource::InputEdges,
    )?;
    let pairs = if count < 2 {
        Some(0)
    } else if count.is_multiple_of(2) {
        (count / 2).checked_mul(count - 1)
    } else {
        count.checked_mul((count - 1) / 2)
    }
    .ok_or(ConstructError::SizeOverflow)?;
    cap(pairs, w.limits.max_pairs, ConstructResource::Pairs)?;
    let mut a = Arrangement {
        originals: Vec::new(),
        points: Vec::new(),
        splits: Vec::new(),
        atoms: Vec::new(),
        supports: Vec::new(),
    };
    for (ring_id, ring) in rings.iter().enumerate() {
        for (offset, &p) in ring.vertices.iter().enumerate() {
            w.tick(Checkpoint::ConstructInput)?;
            if p.iter().any(|&q| !(-bound..=bound).contains(&q)) {
                return Err(invalid(
                    ring.source_path,
                    ConstructInputIssue::CoordinateRange,
                ));
            }
            let q = ring.vertices[(offset + 1) % ring.vertices.len()];
            if p == q {
                return Err(invalid(
                    ring.source_path,
                    ConstructInputIssue::CollapsedEdge,
                ));
            }
            w.push(
                &mut a.originals,
                Original {
                    a: p,
                    b: q,
                    ring: ring_id,
                    source: SourceEdge {
                        path: ring.source_path,
                        edge: offset,
                    },
                    material: ring.material,
                },
            )?;
        }
    }
    for edge in 0..a.originals.len() {
        let original = a.originals[edge];
        for p in [original.a, original.b] {
            let vertex = point_id(&mut a, ExactPoint::integer(p), w)?;
            w.push(&mut a.splits, Split { edge, vertex })?;
        }
    }
    Ok(a)
}
fn contacts(a: Original, b: Original) -> Result<[Option<ExactPoint>; 4]> {
    let mut result = [None; 4];
    if orient_integer(a.a, a.b, b.a) == 0 && orient_integer(a.a, a.b, b.b) == 0 {
        for (slot, p) in [a.a, a.b, b.a, b.b].into_iter().enumerate() {
            if on_segment_integer(p, a.a, a.b) && on_segment_integer(p, b.a, b.b) {
                result[slot] = Some(ExactPoint::integer(p));
            }
        }
    } else {
        result[0] = arith(intersection(a.a, a.b, b.a, b.b))?;
    }
    Ok(result)
}
fn split_pairs(
    a: &mut Arrangement,
    rings: &[MaterialRing<'_>],
    w: &mut Work<'_, '_>,
) -> Result<usize> {
    w.cancel.check(Checkpoint::ConstructPairs)?;
    let mut pairs = 0;
    for left in 0..a.originals.len() {
        for right in left + 1..a.originals.len() {
            w.tick(Checkpoint::ConstructPairs)?;
            pairs += 1;
            let (x, y) = (a.originals[left], a.originals[right]);
            let found = contacts(x, y)?;
            if x.ring == y.ring && found.iter().any(Option::is_some) {
                let ring_len = rings[x.ring].vertices.len();
                let adjacent = (x.source.edge + 1) % ring_len == y.source.edge
                    || (y.source.edge + 1) % ring_len == x.source.edge;
                let shared = if x.b == y.a { x.b } else { x.a };
                if !adjacent
                    || found
                        .iter()
                        .flatten()
                        .any(|p| *p != ExactPoint::integer(shared))
                {
                    return Err(invalid(x.source.path, ConstructInputIssue::SelfContact));
                }
            }
            for p in found.into_iter().flatten() {
                let vertex = point_id(a, p, w)?;
                for edge in [left, right] {
                    w.push(&mut a.splits, Split { edge, vertex })?;
                }
            }
        }
    }
    Ok(pairs)
}
fn atoms(a: &mut Arrangement, w: &mut Work<'_, '_>) -> Result<()> {
    w.cancel.check(Checkpoint::ConstructSplit)?;
    a.splits.sort_unstable_by(|x, y| {
        x.edge
            .cmp(&y.edge)
            .then_with(|| a.points[x.vertex].cmp(&a.points[y.vertex]))
    });
    w.cancel.check(Checkpoint::ConstructSplit)?;
    let mut previous: Option<Split> = None;
    for index in 0..a.splits.len() {
        w.tick(Checkpoint::ConstructSplit)?;
        let current = a.splits[index];
        if let Some(last) = previous {
            if last.edge == current.edge && last.vertex != current.vertex {
                let ends = [last.vertex, current.vertex];
                let mut found = None;
                for (id, atom) in a.atoms.iter().enumerate() {
                    w.tick(Checkpoint::ConstructSplit)?;
                    if atom.ends == ends {
                        found = Some(id);
                        break;
                    }
                }
                let atom = if let Some(id) = found {
                    id
                } else {
                    cap(
                        a.atoms
                            .len()
                            .checked_add(1)
                            .ok_or(ConstructError::SizeOverflow)?,
                        w.limits.max_atoms,
                        ConstructResource::Atoms,
                    )?;
                    let id = a.atoms.len();
                    w.push(
                        &mut a.atoms,
                        Atom {
                            ends,
                            original: current.edge,
                        },
                    )?;
                    id
                };
                w.push(
                    &mut a.supports,
                    Support {
                        atom,
                        original: current.edge,
                    },
                )?;
            }
        }
        previous = Some(current);
    }
    a.supports.sort_unstable_by_key(|s| (s.atom, s.original));
    w.cancel.check(Checkpoint::ConstructSplit)?;
    Ok(())
}
fn original_direction(o: Original) -> [i64; 2] {
    if o.a < o.b {
        [o.b[0] - o.a[0], o.b[1] - o.a[1]]
    } else {
        [o.a[0] - o.b[0], o.a[1] - o.b[1]]
    }
}
#[derive(Clone, Copy)]
enum Composition {
    Union,
    Difference,
    Prepared,
}

fn material(
    query: &SymbolicPoint,
    a: &Arrangement,
    groups: &[u64],
    composition: Composition,
    w: &mut Work<'_, '_>,
) -> Result<bool> {
    if matches!(composition, Composition::Difference) && groups.first() != Some(&0) {
        return Ok(false);
    }
    let mut retained_body = false;
    for &group in groups {
        let mut inside = false;
        for original in &a.originals {
            w.tick(Checkpoint::ConstructClassify)?;
            if original.material != group {
                continue;
            }
            let ya = arith(query.compare_y(&ExactPoint::integer(original.a)))?;
            let yb = arith(query.compare_y(&ExactPoint::integer(original.b)))?;
            if (ya != Ordering::Less && yb == Ordering::Less)
                || (yb != Ordering::Less && ya == Ordering::Less)
            {
                let side = arith(query.side_of_original_line(original.a, original.b))?;
                if (original.b[1] > original.a[1] && side == Ordering::Greater)
                    || (original.b[1] < original.a[1] && side == Ordering::Less)
                {
                    inside = !inside;
                }
            }
        }
        match composition {
            Composition::Union if inside => return Ok(true),
            Composition::Difference if (group == 0 && !inside) || (group != 0 && inside) => {
                return Ok(false);
            }
            Composition::Prepared => {
                if group == 0 {
                    retained_body = inside;
                } else if group < PREPARED_ADDITIVE_ID_START {
                    retained_body &= !inside;
                } else if inside {
                    return Ok(true);
                }
            }
            _ => {}
        }
    }
    Ok(match composition {
        Composition::Union => false,
        Composition::Difference => true,
        Composition::Prepared => retained_body,
    })
}
fn classify(
    a: &Arrangement,
    composition: Composition,
    w: &mut Work<'_, '_>,
) -> Result<ConstructedMaterial> {
    w.cancel.check(Checkpoint::ConstructClassify)?;
    let mut groups = Vec::new();
    for original in &a.originals {
        w.tick(Checkpoint::ConstructClassify)?;
        w.push(&mut groups, original.material)?;
    }
    groups.sort_unstable();
    groups.dedup();
    let mut result = ConstructedMaterial {
        dyadic_exponent_mm: None,
        vertices: Vec::new(),
        atoms: Vec::new(),
        edges: Vec::new(),
        supports: Vec::new(),
        rings: Vec::new(),
        ring_edges: Vec::new(),
        regions: Vec::new(),
        holes: Vec::new(),
        summary: ConstructedSummary::default(),
        payload_bytes: 0,
    };
    for &point in &a.points {
        w.tick(Checkpoint::ConstructClassify)?;
        w.push(&mut result.vertices, point)?;
    }
    let mut support_cursor = 0;
    for (id, atom) in a.atoms.iter().enumerate() {
        w.tick(Checkpoint::ConstructClassify)?;
        let direction = original_direction(a.originals[atom.original]);
        let left = arith(SymbolicPoint::midpoint_normal(
            &a.points[atom.ends[0]],
            &a.points[atom.ends[1]],
            [-direction[1], direction[0]],
        ))?;
        let right = arith(SymbolicPoint::midpoint_normal(
            &a.points[atom.ends[0]],
            &a.points[atom.ends[1]],
            [direction[1], -direction[0]],
        ))?;
        let (l, r) = (
            material(&left, a, &groups, composition, w)?,
            material(&right, a, &groups, composition, w)?,
        );
        let start = result.supports.len();
        while support_cursor < a.supports.len() && a.supports[support_cursor].atom == id {
            w.tick(Checkpoint::ConstructClassify)?;
            let support = a.supports[support_cursor];
            w.push(&mut result.supports, a.originals[support.original].source)?;
            support_cursor += 1;
        }
        let boundary = if l == r {
            None
        } else {
            let index = result.edges.len();
            let endpoints = if l {
                atom.ends
            } else {
                [atom.ends[1], atom.ends[0]]
            };
            w.push(
                &mut result.edges,
                ConstructedEdge {
                    endpoints,
                    atom: id,
                },
            )?;
            Some(index)
        };
        let end = result.supports.len();
        w.push(
            &mut result.atoms,
            ConstructedAtom {
                endpoints: atom.ends,
                left_material: l,
                right_material: r,
                boundary,
                supports: start..end,
            },
        )?;
    }
    Ok(result)
}
fn direction(edge: ConstructedEdge, a: &Arrangement) -> [i64; 2] {
    let atom = a.atoms[edge.atom];
    let d = original_direction(a.originals[atom.original]);
    if edge.endpoints == atom.ends {
        d
    } else {
        [-d[0], -d[1]]
    }
}
fn cross(a: [i64; 2], b: [i64; 2]) -> i128 {
    i128::from(a[0]) * i128::from(b[1]) - i128::from(a[1]) * i128::from(b[0])
}
fn angle(a: [i64; 2], b: [i64; 2]) -> Ordering {
    let half = |d: [i64; 2]| d[1] < 0 || (d[1] == 0 && d[0] < 0);
    half(a).cmp(&half(b)).then_with(|| cross(b, a).cmp(&0))
}
fn successors(
    out: &ConstructedMaterial,
    a: &Arrangement,
    w: &mut Work<'_, '_>,
) -> Result<Vec<usize>> {
    let mut next = Vec::new();
    let mut incoming = Vec::new();
    for _ in &out.edges {
        w.tick(Checkpoint::ConstructTopology)?;
        w.push(&mut incoming, 0_usize)?;
    }
    for edge in &out.edges {
        let d = direction(*edge, a);
        let reverse = [-d[0], -d[1]];
        let (mut before, mut largest): (Option<usize>, Option<usize>) = (None, None);
        for (id, candidate) in out.edges.iter().enumerate() {
            w.tick(Checkpoint::ConstructTopology)?;
            if candidate.endpoints[0] != edge.endpoints[1] {
                continue;
            }
            let candidate_dir = direction(*candidate, a);
            if largest.is_none_or(|old| angle(candidate_dir, direction(out.edges[old], a)).is_gt())
            {
                largest = Some(id);
            }
            if angle(candidate_dir, reverse).is_lt()
                && before
                    .is_none_or(|old| angle(candidate_dir, direction(out.edges[old], a)).is_gt())
            {
                before = Some(id);
            }
        }
        let id = before.or(largest).ok_or(ConstructError::Topology)?;
        incoming[id] = incoming[id]
            .checked_add(1)
            .ok_or(ConstructError::SizeOverflow)?;
        w.push(&mut next, id)?;
    }
    for count in incoming {
        w.tick(Checkpoint::ConstructTopology)?;
        if count != 1 {
            return Err(ConstructError::Topology);
        }
    }
    Ok(next)
}
fn emit_cycle(
    out: &mut ConstructedMaterial,
    edges: &[usize],
    a: &Arrangement,
    w: &mut Work<'_, '_>,
) -> Result<()> {
    if edges.len() < 3 {
        return Err(ConstructError::Topology);
    }
    let mut minimum = 0;
    for index in 1..edges.len() {
        w.tick(Checkpoint::ConstructTopology)?;
        if out.vertices[out.edges[edges[index]].endpoints[0]]
            < out.vertices[out.edges[edges[minimum]].endpoints[0]]
        {
            minimum = index;
        }
    }
    let incoming = direction(
        out.edges[edges[(minimum + edges.len() - 1) % edges.len()]],
        a,
    );
    let outgoing = direction(out.edges[edges[minimum]], a);
    let turn = cross(incoming, outgoing);
    if turn == 0 {
        return Err(ConstructError::Topology);
    }
    let orientation = if turn > 0 {
        CycleOrientation::Counterclockwise
    } else {
        CycleOrientation::Clockwise
    };
    let role = if turn > 0 {
        BoundaryRole::Outer
    } else {
        BoundaryRole::Hole
    };
    let mut canonical = 0;
    for index in 1..edges.len() {
        w.tick(Checkpoint::ConstructTopology)?;
        let key = |offset: usize| (out.edges[edges[offset]].endpoints[0], edges[offset]);
        if key(index) < key(canonical) {
            canonical = index;
        }
    }
    let start = out.ring_edges.len();
    for offset in 0..edges.len() {
        w.tick(Checkpoint::ConstructTopology)?;
        w.push(
            &mut out.ring_edges,
            edges[(canonical + offset) % edges.len()],
        )?;
    }
    let end = out.ring_edges.len();
    w.push(
        &mut out.rings,
        ConstructedRing {
            parent: None,
            depth: 0,
            role,
            orientation,
            edges: start..end,
        },
    )?;
    Ok(())
}
fn cycles(out: &mut ConstructedMaterial, a: &Arrangement, w: &mut Work<'_, '_>) -> Result<()> {
    w.cancel.check(Checkpoint::ConstructTopology)?;
    let next = successors(out, a, w)?;
    let mut visited = Vec::new();
    for _ in &out.edges {
        w.tick(Checkpoint::ConstructTopology)?;
        w.push(&mut visited, false)?;
    }
    let mut vertices = Vec::new();
    let mut stack = Vec::new();
    for first in 0..out.edges.len() {
        w.tick(Checkpoint::ConstructTopology)?;
        if visited[first] {
            continue;
        }
        vertices.clear();
        stack.clear();
        w.push(&mut vertices, out.edges[first].endpoints[0])?;
        let mut current = first;
        loop {
            w.tick(Checkpoint::ConstructTopology)?;
            if visited[current] {
                return Err(ConstructError::Topology);
            }
            visited[current] = true;
            w.push(&mut stack, current)?;
            let end = out.edges[current].endpoints[1];
            let mut position = None;
            for (index, &vertex) in vertices.iter().enumerate() {
                w.tick(Checkpoint::ConstructTopology)?;
                if vertex == end {
                    position = Some(index);
                    break;
                }
            }
            if let Some(position) = position {
                emit_cycle(out, &stack[position..], a, w)?;
                stack.truncate(position);
                vertices.truncate(position + 1);
            } else {
                w.push(&mut vertices, end)?;
            }
            current = next[current];
            if current == first {
                break;
            }
        }
        if !stack.is_empty() {
            return Err(ConstructError::Topology);
        }
    }
    out.rings.sort_unstable_by_key(|r| {
        let e = out.ring_edges[r.edges.start];
        (out.edges[e].endpoints[0], e)
    });
    w.cancel.check(Checkpoint::ConstructTopology)?;
    Ok(())
}
fn contains(
    out: &ConstructedMaterial,
    ring: usize,
    query: &SymbolicPoint,
    a: &Arrangement,
    w: &mut Work<'_, '_>,
) -> Result<bool> {
    let mut inside = false;
    for &id in &out.ring_edges[out.rings[ring].edges.clone()] {
        w.tick(Checkpoint::ConstructTopology)?;
        let edge = out.edges[id];
        let d = direction(edge, a);
        let (start, end) = (
            &out.vertices[edge.endpoints[0]],
            &out.vertices[edge.endpoints[1]],
        );
        let ya = arith(query.compare_y(start))?;
        let yb = arith(query.compare_y(end))?;
        if (ya != Ordering::Less && yb == Ordering::Less)
            || (yb != Ordering::Less && ya == Ordering::Less)
        {
            let side = arith(query.side_of_supported_line(start, d))?;
            if (d[1] > 0 && side.is_gt()) || (d[1] < 0 && side.is_lt()) {
                inside = !inside;
            }
        }
    }
    Ok(inside)
}
fn hierarchy(out: &mut ConstructedMaterial, a: &Arrangement, w: &mut Work<'_, '_>) -> Result<()> {
    let mut relations = Vec::new();
    for child in 0..out.rings.len() {
        let first = out.edges[out.ring_edges[out.rings[child].edges.start]];
        let d = direction(first, a);
        let normal = if out.rings[child].orientation == CycleOrientation::Counterclockwise {
            [-d[1], d[0]]
        } else {
            [d[1], -d[0]]
        };
        let query = arith(SymbolicPoint::midpoint_normal(
            &out.vertices[first.endpoints[0]],
            &out.vertices[first.endpoints[1]],
            normal,
        ))?;
        for parent in 0..out.rings.len() {
            w.tick(Checkpoint::ConstructTopology)?;
            if parent != child && contains(out, parent, &query, a, w)? {
                w.push(&mut relations, (parent, child))?;
                out.rings[child].depth += 1;
            }
        }
    }
    for &(parent, child) in &relations {
        w.tick(Checkpoint::ConstructTopology)?;
        if out.rings[child]
            .parent
            .is_none_or(|old| out.rings[parent].depth > out.rings[old].depth)
        {
            out.rings[child].parent = Some(parent);
        }
    }
    for id in 0..out.rings.len() {
        w.tick(Checkpoint::ConstructTopology)?;
        let ring = &out.rings[id];
        if ring.depth.is_multiple_of(2) != (ring.role == BoundaryRole::Outer)
            || ring
                .parent
                .is_some_and(|p| out.rings[p].depth + 1 != ring.depth)
            || (ring.depth > 0 && ring.parent.is_none())
        {
            return Err(ConstructError::Topology);
        }
        if ring.role == BoundaryRole::Outer {
            let start = out.holes.len();
            for hole in 0..out.rings.len() {
                w.tick(Checkpoint::ConstructTopology)?;
                if out.rings[hole].parent == Some(id) && out.rings[hole].role == BoundaryRole::Hole
                {
                    w.push(&mut out.holes, hole)?;
                }
            }
            let end = out.holes.len();
            w.push(
                &mut out.regions,
                ConstructedRegion {
                    outer: id,
                    holes: start..end,
                },
            )?;
        }
    }
    Ok(())
}
fn retained_bytes(out: &ConstructedMaterial) -> Result<usize> {
    let sizes = [
        (out.vertices.capacity(), size_of::<ExactPoint>()),
        (out.atoms.capacity(), size_of::<ConstructedAtom>()),
        (out.edges.capacity(), size_of::<ConstructedEdge>()),
        (out.supports.capacity(), size_of::<SourceEdge>()),
        (out.rings.capacity(), size_of::<ConstructedRing>()),
        (out.ring_edges.capacity(), size_of::<usize>()),
        (out.regions.capacity(), size_of::<ConstructedRegion>()),
        (out.holes.capacity(), size_of::<usize>()),
    ];
    sizes.into_iter().try_fold(0_usize, |sum, (n, size)| {
        sum.checked_add(n.checked_mul(size).ok_or(ConstructError::SizeOverflow)?)
            .ok_or(ConstructError::SizeOverflow)
    })
}

/// Construct exact material without modifying or rounding source rings.
/// # Errors
/// Invalid rings, explicit resource caps, arithmetic/capacity failures and inconsistent topology.
pub fn union_material_rings(
    rings: &[MaterialRing<'_>],
    limits: ConstructLimits,
) -> Result<ConstructedMaterial> {
    union_material_rings_with_cancellation(rings, limits, &mut Cancellation::never())
}
/// Construct exact material atomically with bounded TD-014 phase/work cancellation.
/// # Errors
/// Observed cancellation preserves its checkpoint; no successful partial material is returned.
pub fn union_material_rings_with_cancellation(
    rings: &[MaterialRing<'_>],
    limits: ConstructLimits,
    cancel: &mut Cancellation<'_>,
) -> Result<ConstructedMaterial> {
    compose_bounded(rings, limits, cancel, 1_i64 << 52, Composition::Union)
}

/// Subtract the union of cutter sources from body source zero, without rounding.
///
/// Rings sharing an ID use even-odd parity regardless of winding. ID zero is the body;
/// every nonzero ID is a distinct cutter, and overlapping cutter sources are unioned.
/// An absent body produces empty material. Input validation still covers every ring.
/// Boundary-only contact removes no area. Source provenance includes body and cutter edges.
/// # Errors
/// Returns structured input, budget, arithmetic, allocation and topology failures.
pub fn difference_material_rings(
    rings: &[MaterialRing<'_>],
    limits: ConstructLimits,
) -> Result<ConstructedMaterial> {
    difference_material_rings_with_cancellation(rings, limits, &mut Cancellation::never())
}

/// Cancellable exact body-minus-cutters composition with the same semantics as
/// [`difference_material_rings`]. No partial material is published.
/// # Errors
/// Also preserves observed cancellation checkpoints.
pub fn difference_material_rings_with_cancellation(
    rings: &[MaterialRing<'_>],
    limits: ConstructLimits,
    cancel: &mut Cancellation<'_>,
) -> Result<ConstructedMaterial> {
    compose_bounded(rings, limits, cancel, 1_i64 << 52, Composition::Difference)
}

/// First additive component ID reserved by prepared-material composition.
///
/// Only the prepared APIs assign this high-bit meaning. Union and difference retain their
/// existing interpretation of every ID. Source ownership must be supplied explicitly by callers.
pub const PREPARED_ADDITIVE_ID_START: u64 = 1_u64 << 63;

/// Construct `(body minus cutters) union additive components` without rounding.
///
/// ID zero is the body, IDs `1..2^63` are cutters, and IDs `2^63..=u64::MAX` are additive
/// components. Each individual ID uses even-odd parity; distinct cutters and additives use union.
/// Additive material survives absent body and overlapping cutters. No ownership is inferred from
/// winding, connectivity or nesting. Boundary-only contacts contribute no area.
/// # Errors
/// Returns structured input, budget, arithmetic, allocation and topology failures.
pub fn prepared_material_rings(
    rings: &[MaterialRing<'_>],
    limits: ConstructLimits,
) -> Result<ConstructedMaterial> {
    prepared_material_rings_with_cancellation(rings, limits, &mut Cancellation::never())
}

/// Cancellable exact prepared composition with the semantics of [`prepared_material_rings`].
/// # Errors
/// Also preserves observed cancellation checkpoints; no partial material is returned.
pub fn prepared_material_rings_with_cancellation(
    rings: &[MaterialRing<'_>],
    limits: ConstructLimits,
    cancel: &mut Cancellation<'_>,
) -> Result<ConstructedMaterial> {
    compose_bounded(rings, limits, cancel, 1_i64 << 52, Composition::Prepared)
}

fn compose_bounded(
    rings: &[MaterialRing<'_>],
    limits: ConstructLimits,
    cancel: &mut Cancellation<'_>,
    bound: i64,
    composition: Composition,
) -> Result<ConstructedMaterial> {
    cancel.check(Checkpoint::ConstructStart)?;
    let mut w = Work {
        cancel,
        limits,
        steps: 0,
        bytes: 0,
    };
    let mut a = input(rings, bound, &mut w)?;
    let pairs = split_pairs(&mut a, rings, &mut w)?;
    atoms(&mut a, &mut w)?;
    let mut out = classify(&a, composition, &mut w)?;
    cycles(&mut out, &a, &mut w)?;
    hierarchy(&mut out, &a, &mut w)?;
    out.payload_bytes = retained_bytes(&out)?;
    out.summary = ConstructedSummary {
        input_rings: rings.len(),
        input_edges: a.originals.len(),
        pairs,
        vertices: out.vertices.len(),
        atoms: out.atoms.len(),
        boundary_edges: out.edges.len(),
        rings: out.rings.len(),
        regions: out.regions.len(),
        holes: out.holes.len(),
        work: w.steps,
        requested_payload_bytes: w.bytes,
    };
    w.cancel.check(Checkpoint::ConstructComplete)?;
    Ok(out)
}
