//! Nonconstructive cycle area and current traversal orientation (TD-024).
//!
//! Only existing locally `SimpleCycle` embeddings are measured. Exactness concerns represented
//! finite binary64 XY in mm, not ideal pre-rounding geometry. Canonical order, source geometry,
//! cross-path contacts and complete-only uncertainty remain borrowed and unchanged. Neither sign
//! nor a finite magnitude assigns hole/material roles. No closure, grid, repair or rewinding occurs.
//!
//! ```
//! use karasu_core::{geometry::Point3, mesh::TriangleMesh};
//! use karasu_core::slicing::{section_mesh_z, SectionLimits, SectionTransform};
//! use karasu_core::slicing::normalize::{normalize_section, NormalizationLimits};
//! use karasu_core::slicing::graph::{build_incidence_graph, GraphLimits};
//! use karasu_core::slicing::paths::{extract_paths, PathLimits};
//! use karasu_core::slicing::validity::{inspect_planar_paths, ValidityLimits};
//! use karasu_core::slicing::area::{measure_cycle_areas, AreaLimits, AreaMagnitude, PathAreaOutcome};
//! let mesh = TriangleMesh::try_new(vec![Point3::new(0.,0.,0.),
//!     Point3::new(2.,0.,0.), Point3::new(0.,2.,0.)], vec![[0,1,2]])?;
//! let raw = section_mesh_z(&mesh, 0., SectionTransform::Identity, SectionLimits::default())?;
//! let normalized = normalize_section(&raw, NormalizationLimits::default())?;
//! let graph = build_incidence_graph(&normalized, GraphLimits::default())?;
//! let paths = extract_paths(&graph, PathLimits::default())?;
//! let inspection = inspect_planar_paths(&paths, ValidityLimits::default())?;
//! let areas = measure_cycle_areas(&inspection, AreaLimits::default())?;
//! assert!(matches!(areas.outcomes()[0], PathAreaOutcome::Measured {
//!     magnitude: AreaMagnitude::Finite(2.0), .. }));
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use super::normalize::CoordinateAgreement;
use super::paths::{ContourPath, EdgeOutcome, PathKind};
use super::validity::{EmbeddingStatus, PathEmbedding, PlanarInspection};
use crate::cancellation::{Cancellation, Cancelled, Checkpoint};
use crate::geometry::Point3;
use std::{cmp::Ordering, error::Error, fmt};

/// Exact current traversal direction viewed from +Z, not an outer/hole role.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CycleOrientation {
    /// Negative exact signed shoelace area.
    Clockwise,
    /// Positive exact signed shoelace area.
    Counterclockwise,
}

/// One nearest-even conversion of the positive exact represented area, in mm^2.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AreaMagnitude {
    /// Strictly positive finite magnitude, including subnormals; not necessarily exact.
    Finite(f64),
    /// Positive exact area rounds to zero; not geometric degeneracy or lost orientation.
    RoundedToZero,
    /// Nearest-even conversion would overflow; no infinity scalar is returned.
    Overflow,
}

/// Why a path is retained without algebraic area or fabricated orientation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AreaSkipReason {
    /// Open graph chain, never automatically closed.
    OpenPath,
    /// Ambiguous graph trail, even when graph-closed or coordinate-closed.
    AmbiguousPath,
    /// Closed candidate with forbidden self-contact or collapsed edges.
    NonSimpleCycle,
}

/// Exactly one complete outcome per original path ID; no material-area total.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PathAreaOutcome {
    /// Ineligible geometry remains explicit and accessible through the inspection.
    Skipped(AreaSkipReason),
    /// Existing `SimpleCycle` measured without modifying its canonical traversal.
    Measured {
        /// Exact orientation, independent of magnitude representability.
        orientation: CycleOrientation,
        /// Correctly rounded magnitude or explicit representation-range outcome.
        magnitude: AreaMagnitude,
    },
}

/// Configurable safety policy, not measured throughput or a total-RSS guarantee.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AreaLimits {
    /// All original paths, including skipped paths.
    pub max_paths: usize,
    /// All original path vertex references, including skipped paths.
    pub max_path_vertices: usize,
    /// All original path edge references, including skipped paths.
    pub max_path_edges: usize,
    /// Actually assessed closing edges, summed across `SimpleCycle` paths.
    pub max_terms: usize,
    /// Requested owned outcome capacity, excluding input, stack and allocator overhead.
    pub max_payload_bytes: usize,
}
impl Default for AreaLimits {
    fn default() -> Self {
        Self {
            max_paths: 3_000_000,
            max_path_vertices: 6_000_000,
            max_path_edges: 3_000_000,
            max_terms: 3_000_000,
            max_payload_bytes: 512 * 1024 * 1024,
        }
    }
}

/// Independent count/work/requested-byte resource.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AreaResource {
    /// Original path records.
    Paths,
    /// Original path vertex references, including skipped input.
    PathVertices,
    /// Original path edge references, including skipped input.
    PathEdges,
    /// Assessed closing edges.
    Terms,
    /// Requested outcome-buffer capacity, not RSS.
    PayloadBytes,
}

/// Structured defensive failure of a privately constructed upstream certificate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AreaInputIssue {
    /// Embedding/path counts or eligibility metadata disagree.
    Eligibility,
    /// Missing slices, inconsistent closure, issues or edge/vertex counts.
    CycleShape,
    /// Graph vertex/edge/path assignment does not agree with the stored walk.
    Association,
    /// Coordinate agreement is missing or nonfinite.
    Coordinates,
    /// Exact zero area contradicts a claimed simple closed noncollapsed cycle.
    ZeroAreaSimpleCycle,
}

/// Operational failure; skipped paths and scalar range outcomes are complete result data instead.
#[derive(Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum AreaError {
    /// Cancellation observed; no successful partial report is published.
    Cancelled(Cancelled),
    /// A configured cap failed before publication.
    LimitExceeded {
        /// Resource that exceeded the cap.
        resource: AreaResource,
        /// Configured maximum.
        limit: usize,
    },
    /// Checked count/capacity arithmetic, isize range or fixed arithmetic domain failed.
    SizeOverflow,
    /// Fallible outcome reservation failed.
    Allocation,
    /// Privately constructed inspection invariants were inconsistent.
    InvalidInspection {
        /// Original path ID, when known.
        path: Option<usize>,
        /// Original graph vertex ID, when known.
        vertex: Option<usize>,
        /// Structured consistency reason.
        reason: AreaInputIssue,
    },
}
impl fmt::Display for AreaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled(error) => error.fmt(f),
            Self::LimitExceeded { resource, limit } => {
                write!(f, "cycle area {resource:?} limit exceeded ({limit})")
            }
            Self::SizeOverflow => write!(f, "cycle area count or capacity arithmetic overflow"),
            Self::Allocation => write!(f, "unable to reserve cycle area outcomes"),
            Self::InvalidInspection {
                path,
                vertex,
                reason,
            } => write!(
                f,
                "inconsistent cycle area input at path {path:?}, vertex {vertex:?}: {reason:?}"
            ),
        }
    }
}
impl Error for AreaError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Cancelled(error) => Some(error),
            _ => None,
        }
    }
}
impl From<Cancelled> for AreaError {
    fn from(error: Cancelled) -> Self {
        Self::Cancelled(error)
    }
}

/// Deterministic accounting, never accepted material or summed union area.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AreaSummary {
    /// All original paths; equals measured plus three skipped counts.
    pub paths: usize,
    /// All path vertex references, including skipped input.
    pub path_vertices: usize,
    /// All path edge references, including skipped input.
    pub path_edges: usize,
    /// Measured `SimpleCycle` paths; equals finite plus rounded-to-zero plus overflow counts.
    pub measured: usize,
    /// Explicitly skipped open paths.
    pub skipped_open: usize,
    /// Explicitly skipped ambiguous trails.
    pub skipped_ambiguous: usize,
    /// Explicitly skipped non-simple candidates.
    pub skipped_non_simple: usize,
    /// Positive finite scalar magnitudes.
    pub finite: usize,
    /// Exact positive magnitudes that round to zero.
    pub rounded_to_zero: usize,
    /// Magnitudes whose conversion overflows.
    pub overflow: usize,
    /// Actually assessed closing edges, not arithmetic product count.
    pub assessed_terms: usize,
    /// Requested outcome capacity only, not actual allocator capacity or RSS.
    pub requested_payload_bytes: usize,
}

/// Immutable area report borrowing all upstream geometry, diagnostics and uncertainty.
#[derive(Debug, PartialEq)]
pub struct SectionAreas<'i, 'p, 'g, 'n, 'r> {
    input: &'i PlanarInspection<'p, 'g, 'n, 'r>,
    outcomes: Vec<PathAreaOutcome>,
    summary: AreaSummary,
}
impl<'i, 'p, 'g, 'n, 'r> SectionAreas<'i, 'p, 'g, 'n, 'r> {
    /// Same inspection, retaining cross-path findings and unassessed context unchanged.
    #[must_use]
    pub const fn inspection(&self) -> &'i PlanarInspection<'p, 'g, 'n, 'r> {
        self.input
    }
    /// Outcomes in original path-ID order; no rewinding or area sorting.
    #[must_use]
    pub fn outcomes(&self) -> &[PathAreaOutcome] {
        &self.outcomes
    }
    /// Complete count and requested-capacity accounting, not material totals.
    #[must_use]
    pub const fn summary(&self) -> AreaSummary {
        self.summary
    }
}

type Result<T> = std::result::Result<T, AreaError>;
fn invalid(path: Option<usize>, vertex: Option<usize>, reason: AreaInputIssue) -> AreaError {
    AreaError::InvalidInspection {
        path,
        vertex,
        reason,
    }
}
fn add(a: usize, b: usize) -> Result<usize> {
    a.checked_add(b).ok_or(AreaError::SizeOverflow)
}
fn payload(n: usize) -> Result<usize> {
    let bytes = n
        .checked_mul(size_of::<PathAreaOutcome>())
        .ok_or(AreaError::SizeOverflow)?;
    isize::try_from(bytes).map_err(|_| AreaError::SizeOverflow)?;
    Ok(bytes)
}
fn cap(n: usize, limit: usize, resource: AreaResource) -> Result<()> {
    if n > limit {
        Err(AreaError::LimitExceeded { resource, limit })
    } else {
        Ok(())
    }
}

fn eligibility(
    path: &ContourPath,
    embedding: PathEmbedding,
    id: usize,
    vertices: usize,
    edges: usize,
) -> Result<Option<AreaSkipReason>> {
    if edges == 0 || vertices != add(edges, usize::from(!path.closed))? {
        return Err(invalid(Some(id), None, AreaInputIssue::CycleShape));
    }
    let candidate = path.kind == PathKind::ClosedCycleCandidate;
    if candidate
        && (!path.closed || !path.issues.is_empty() || vertices < 3 || path.endpoints.is_some())
    {
        return Err(invalid(Some(id), None, AreaInputIssue::CycleShape));
    }
    let defects = embedding.collapsed_edges != 0 || embedding.self_contacts != 0;
    match (path.kind, embedding.status, defects) {
        (PathKind::ClosedCycleCandidate, EmbeddingStatus::SimpleCycle, false) => Ok(None),
        (PathKind::ClosedCycleCandidate, EmbeddingStatus::NonSimpleCycle, true) => {
            Ok(Some(AreaSkipReason::NonSimpleCycle))
        }
        (PathKind::OpenChain, EmbeddingStatus::Ineligible, _) if !path.closed => {
            Ok(Some(AreaSkipReason::OpenPath))
        }
        (PathKind::AmbiguousTrail, EmbeddingStatus::Ineligible, _) => {
            Ok(Some(AreaSkipReason::AmbiguousPath))
        }
        _ => Err(invalid(Some(id), None, AreaInputIssue::Eligibility)),
    }
}

fn preflight(
    input: &PlanarInspection<'_, '_, '_, '_>,
    limits: AreaLimits,
    cancel: &mut Cancellation<'_>,
) -> Result<AreaSummary> {
    let paths = input.paths();
    let mut summary = AreaSummary {
        paths: paths.paths().len(),
        ..AreaSummary::default()
    };
    cap(summary.paths, limits.max_paths, AreaResource::Paths)?;
    if input.embeddings().len() != summary.paths {
        return Err(invalid(None, None, AreaInputIssue::Eligibility));
    }
    for (id, path) in paths.paths().iter().enumerate() {
        cancel.poll(Checkpoint::AreaInput, id)?;
        let vertices =
            paths
                .path_vertices(id)
                .ok_or(invalid(Some(id), None, AreaInputIssue::CycleShape))?;
        let edges =
            paths
                .path_edges(id)
                .ok_or(invalid(Some(id), None, AreaInputIssue::CycleShape))?;
        summary.path_vertices = add(summary.path_vertices, vertices.len())?;
        summary.path_edges = add(summary.path_edges, edges.len())?;
        if eligibility(
            path,
            input.embeddings()[id],
            id,
            vertices.len(),
            edges.len(),
        )?
        .is_none()
        {
            // The proof is valid through u64::MAX even on a future wider-usize host.
            u64::try_from(edges.len()).map_err(|_| AreaError::SizeOverflow)?;
            summary.assessed_terms = add(summary.assessed_terms, edges.len())?;
        }
    }
    for (n, limit, resource) in [
        (
            summary.path_vertices,
            limits.max_path_vertices,
            AreaResource::PathVertices,
        ),
        (
            summary.path_edges,
            limits.max_path_edges,
            AreaResource::PathEdges,
        ),
        (
            summary.assessed_terms,
            limits.max_terms,
            AreaResource::Terms,
        ),
    ] {
        cap(n, limit, resource)?;
    }
    summary.requested_payload_bytes = payload(summary.paths)?;
    cap(
        summary.requested_payload_bytes,
        limits.max_payload_bytes,
        AreaResource::PayloadBytes,
    )?;
    Ok(summary)
}

fn point(input: &PlanarInspection<'_, '_, '_, '_>, path: usize, vertex: usize) -> Result<Point3> {
    let graph = input.paths().graph();
    let feature = graph
        .vertices()
        .get(vertex)
        .and_then(|v| graph.normalized().features().get(v.feature_group))
        .ok_or(invalid(
            Some(path),
            Some(vertex),
            AreaInputIssue::Association,
        ))?;
    agreed_point(feature.coordinates, path, vertex)
}
fn agreed_point(coordinates: CoordinateAgreement, path: usize, vertex: usize) -> Result<Point3> {
    match coordinates {
        CoordinateAgreement::Agreed(p) if p.x.is_finite() && p.y.is_finite() && p.z.is_finite() => {
            Ok(p)
        }
        _ => Err(invalid(
            Some(path),
            Some(vertex),
            AreaInputIssue::Coordinates,
        )),
    }
}

/// Measure existing `SimpleCycle` paths, without assigning material roles or modifying geometry.
/// # Errors
/// Returns typed limits, count/capacity/domain overflow, allocation, consistency or cancellation.
pub fn measure_cycle_areas<'i, 'p, 'g, 'n, 'r>(
    input: &'i PlanarInspection<'p, 'g, 'n, 'r>,
    limits: AreaLimits,
) -> Result<SectionAreas<'i, 'p, 'g, 'n, 'r>> {
    measure_cycle_areas_with_cancellation(input, limits, &mut Cancellation::never())
}

/// Exact represented-XY sign and one nearest-even mm^2 magnitude conversion per eligible cycle.
///
/// O(P+134*T) work for P paths/T assessed edges, O(P) requested outcomes and fixed stack scratch.
/// No contact scan, coordinate copies, physical grid, thread or dependency is introduced. Input and
/// work caps precede fallible reservation. Skipped input still counts against metadata limits.
/// Loops poll every 256 from zero; term indices span cycles. Allocation and fixed per-cycle rounding
/// have checks on both sides; final publication checks even empty/all-skipped input. Fixed arithmetic,
/// allocation and cleanup are indivisible; no latency, progress or RSS guarantee is made.
/// Exactness is not a bound on geometry before upstream rounding. Magnitude range outcomes are
/// complete data, not zero/infinity scalars or operational failure. Earlier errors retain precedence.
/// # Errors
/// Returns no successful partial report on cancellation, limits, overflow, allocation or inconsistent
/// privately constructed input. An exact-zero `SimpleCycle` is a consistency error, never a valid ring.
pub fn measure_cycle_areas_with_cancellation<'i, 'p, 'g, 'n, 'r>(
    input: &'i PlanarInspection<'p, 'g, 'n, 'r>,
    limits: AreaLimits,
    cancel: &mut Cancellation<'_>,
) -> Result<SectionAreas<'i, 'p, 'g, 'n, 'r>> {
    cancel.check(Checkpoint::AreaStart)?;
    let mut summary = preflight(input, limits, cancel)?;
    let mut outcomes = Vec::new();
    if summary.paths != 0 {
        cancel.check(Checkpoint::AreaStorage)?;
        outcomes
            .try_reserve_exact(summary.paths)
            .map_err(|_| AreaError::Allocation)?;
        cancel.check(Checkpoint::AreaStorage)?;
    }
    let paths = input.paths();
    let mut term = 0;
    for (id, path) in paths.paths().iter().enumerate() {
        cancel.poll(Checkpoint::AreaRecords, id)?;
        let vertices =
            paths
                .path_vertices(id)
                .ok_or(invalid(Some(id), None, AreaInputIssue::CycleShape))?;
        let edges =
            paths
                .path_edges(id)
                .ok_or(invalid(Some(id), None, AreaInputIssue::CycleShape))?;
        let outcome = if let Some(reason) = eligibility(
            path,
            input.embeddings()[id],
            id,
            vertices.len(),
            edges.len(),
        )? {
            match reason {
                AreaSkipReason::OpenPath => summary.skipped_open += 1,
                AreaSkipReason::AmbiguousPath => summary.skipped_ambiguous += 1,
                AreaSkipReason::NonSimpleCycle => summary.skipped_non_simple += 1,
            }
            PathAreaOutcome::Skipped(reason)
        } else {
            let mut sum = Accumulator::zero();
            for (offset, &edge_id) in edges.iter().enumerate() {
                cancel.poll(Checkpoint::AreaTerms, term)?;
                term = add(term, 1)?;
                let endpoints = [vertices[offset], vertices[(offset + 1) % vertices.len()]];
                let edge = paths.graph().edges().get(edge_id).ok_or(invalid(
                    Some(id),
                    None,
                    AreaInputIssue::Association,
                ))?;
                if (edge.endpoints != endpoints && edge.endpoints != [endpoints[1], endpoints[0]])
                    || paths.edge_outcome(edge_id) != Some(EdgeOutcome::Path { path: id, offset })
                {
                    return Err(invalid(Some(id), None, AreaInputIssue::Association));
                }
                let a = point(input, id, endpoints[0])?;
                let b = point(input, id, endpoints[1])?;
                sum.product(a.x, b.y, false)?;
                sum.product(a.y, b.x, true)?;
            }
            cancel.check(Checkpoint::AreaRounding)?;
            let orientation = cycle_orientation(&sum, id)?;
            let magnitude = sum.magnitude();
            cancel.check(Checkpoint::AreaRounding)?;
            summary.measured += 1;
            match magnitude {
                AreaMagnitude::Finite(_) => summary.finite += 1,
                AreaMagnitude::RoundedToZero => summary.rounded_to_zero += 1,
                AreaMagnitude::Overflow => summary.overflow += 1,
            }
            PathAreaOutcome::Measured {
                orientation,
                magnitude,
            }
        };
        outcomes.push(outcome);
    }
    cancel.check(Checkpoint::AreaComplete)?;
    Ok(SectionAreas {
        input,
        outcomes,
        summary,
    })
}

fn cycle_orientation(sum: &Accumulator, path: usize) -> Result<CycleOrientation> {
    match sum.sign() {
        Ordering::Greater => Ok(CycleOrientation::Counterclockwise),
        Ordering::Less => Ok(CycleOrientation::Clockwise),
        Ordering::Equal => Err(invalid(
            Some(path),
            None,
            AreaInputIssue::ZeroAreaSimpleCycle,
        )),
    }
}

// With eta=2^-1074, |X|<(2^2098). Every partial signed-product sum for N edges is
// <N*2^4197. N<=u64::MAX needs <=4261 magnitude bits; 134 u32 limbs provide 4288.
// This is exact encoding, not physical quantization, and does not change the 132-limb predicate.
const WORDS: usize = 134;
#[cfg(test)]
mod tests;
#[derive(Clone)]
struct Accumulator {
    words: [u32; WORDS],
    negative: bool,
}
impl Accumulator {
    fn zero() -> Self {
        Self {
            words: [0; WORDS],
            negative: false,
        }
    }
    fn sign(&self) -> Ordering {
        if self.words.iter().all(|&w| w == 0) {
            Ordering::Equal
        } else if self.negative {
            Ordering::Less
        } else {
            Ordering::Greater
        }
    }
    fn product(&mut self, a: f64, b: f64, subtract: bool) -> Result<()> {
        // Callers resolve and validate finite coordinates first; shifts are therefore <=2045.
        let decode = |value: f64| {
            let bits = value.to_bits();
            let exponent = usize::try_from((bits >> 52) & 0x7ff).unwrap();
            let fraction = bits & ((1_u64 << 52) - 1);
            (
                if exponent == 0 {
                    fraction
                } else {
                    fraction | (1_u64 << 52)
                },
                exponent.saturating_sub(1),
                bits >> 63 != 0,
            )
        };
        let (ma, sa, na) = decode(a);
        let (mb, sb, nb) = decode(b);
        let mut value = Self::zero();
        value.negative = na ^ nb ^ subtract;
        let product = u128::from(ma) * u128::from(mb);
        let shift = sa + sb;
        // Split before alignment: shifting the full 106-bit product by up to 31 would overflow u128.
        for i in 0..4 {
            let piece = u64::try_from((product >> (32 * i)) & 0xffff_ffff).unwrap() << (shift % 32);
            let index = shift / 32 + i;
            value.words[index] |= u32::try_from(piece & 0xffff_ffff).unwrap();
            value.words[index + 1] |= u32::try_from(piece >> 32).unwrap();
        }
        self.combine(&value)
    }
    fn combine(&mut self, value: &Self) -> Result<()> {
        if self.negative == value.negative {
            let mut carry = 0_u64;
            for (a, &b) in self.words.iter_mut().zip(&value.words) {
                let sum = u64::from(*a) + u64::from(b) + carry;
                *a = u32::try_from(sum & 0xffff_ffff).unwrap();
                carry = sum >> 32;
            }
            if carry != 0 {
                return Err(AreaError::SizeOverflow);
            }
        } else {
            let order = self.words.iter().rev().cmp(value.words.iter().rev());
            let mut borrow = false;
            for (a, &b) in self.words.iter_mut().zip(&value.words) {
                let (large, small) = if order.is_lt() { (b, *a) } else { (*a, b) };
                let (difference, first) = large.overflowing_sub(small);
                let (difference, second) = difference.overflowing_sub(u32::from(borrow));
                *a = difference;
                borrow = first || second;
            }
            if borrow {
                return Err(AreaError::SizeOverflow);
            }
            if order.is_lt() {
                self.negative = value.negative;
            }
        }
        if self.words.iter().all(|&w| w == 0) {
            self.negative = false;
        }
        Ok(())
    }
    fn bit(&self, index: usize) -> bool {
        self.words
            .get(index / 32)
            .is_some_and(|w| w & (1_u32 << (index % 32)) != 0)
    }
    fn magnitude(&self) -> AreaMagnitude {
        let length = self.words.iter().rposition(|&w| w != 0).map_or(0, |i| {
            32 * i + 32 - usize::try_from(self.words[i].leading_zeros()).unwrap()
        });
        let mut shift = length.saturating_sub(53).max(1075);
        let mut q = 0_u64;
        for bit in shift..length {
            q |= u64::from(self.bit(bit)) << (bit - shift);
        }
        let sticky = (0..shift - 1).any(|bit| self.bit(bit));
        if self.bit(shift - 1) && (sticky || q & 1 != 0) {
            q += 1;
        }
        if q == 1_u64 << 53 {
            q >>= 1;
            shift += 1;
        }
        if q == 0 {
            return AreaMagnitude::RoundedToZero;
        }
        if q < 1_u64 << 52 {
            return AreaMagnitude::Finite(f64::from_bits(q));
        }
        // For normal results the biased exponent is shift-1074; 2047 means overflow.
        if shift >= 3121 {
            return AreaMagnitude::Overflow;
        }
        AreaMagnitude::Finite(f64::from_bits(
            (u64::try_from(shift - 1074).unwrap() << 52) | (q - (1_u64 << 52)),
        ))
    }
}
