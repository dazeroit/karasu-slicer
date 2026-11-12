//! `ExactOnly` world-XY integer coordinates, not polygon construction or material acceptance (TD-025).
//!
//! An explicit binary step/origin maps agreed represented f64 points without movement or rounding.
//! Off-grid and range errors reject the whole view. Every graph vertex retains its original ID;
//! coordinate conflicts remain unresolved, with all observations/complete-only context borrowed.
//! No coordinate welding, inverse decoding, layer sampling, repair or integer area is performed.
//!
//! TD-031 also supports explicit represented-coordinate retention through
//! [`retain_represented_vertices_with_cancellation`]. That mode performs no conversion, labels
//! outcomes honestly, and makes caller grid metadata inactive; see the view's mode getter.
//!
//! ```
//! use karasu_core::{geometry::Point3, mesh::TriangleMesh};
//! use karasu_core::slicing::{section_mesh_z, SectionLimits, SectionTransform};
//! use karasu_core::slicing::normalize::{normalize_section, NormalizationLimits};
//! use karasu_core::slicing::graph::{build_incidence_graph, GraphLimits};
//! use karasu_core::slicing::paths::{extract_paths, PathLimits};
//! use karasu_core::slicing::validity::{inspect_planar_paths, ValidityLimits};
//! use karasu_core::slicing::grid::{map_exact_grid_vertices, BinaryGridSpec, GridLimits};
//! let mesh = TriangleMesh::try_new(vec![Point3::new(0.,0.,0.),
//!     Point3::new(2.,0.,0.), Point3::new(0.,2.,0.)], vec![[0,1,2]])?;
//! let raw = section_mesh_z(&mesh, 0., SectionTransform::Identity, SectionLimits::default())?;
//! let normalized = normalize_section(&raw, NormalizationLimits::default())?;
//! let graph = build_incidence_graph(&normalized, GraphLimits::default())?;
//! let paths = extract_paths(&graph, PathLimits::default())?;
//! let inspection = inspect_planar_paths(&paths, ValidityLimits::default())?;
//! let view = map_exact_grid_vertices(&inspection, BinaryGridSpec::try_new([0.,0.], -1)?,
//!     GridLimits::default())?;
//! assert_eq!(view.summary().exact_vertices, 3);
//! assert_eq!(view.inspection(), &inspection);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use super::graph::GraphVertex;
use super::normalize::{CoordinateAgreement, FeatureGroup};
use super::validity::PlanarInspection;
use crate::cancellation::{Cancellation, Cancelled, Checkpoint};
use std::{cmp::Ordering, error::Error, fmt};

const BOUND: i64 = 1 << 52;
const WORDS: usize = 66;
const MASK: u64 = (1_u64 << 32) - 1;

/// Axis in deterministic world-XY traversal/error order, not a raster axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GridAxis {
    /// World X in millimeters.
    X,
    /// World Y in millimeters.
    Y,
}

/// Validated version-1 `ExactOnly` metadata; no default origin, scale or lossy mode.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BinaryGridSpec {
    origin_xy_mm: [f64; 2],
    step_exponent: i16,
}
impl BinaryGridSpec {
    /// Validate explicit finite origins and h=2^k mm for -1074<=k<=1023.
    ///
    /// Metadata signed zeros become +0. This does not move or normalize source geometry.
    /// # Errors
    /// Checks origin X, origin Y, then exponent, independently of mapping cancellation.
    pub fn try_new(
        mut origin_xy_mm: [f64; 2],
        step_exponent: i16,
    ) -> std::result::Result<Self, GridSpecError> {
        for (value, axis) in origin_xy_mm.iter_mut().zip([GridAxis::X, GridAxis::Y]) {
            if !value.is_finite() {
                return Err(GridSpecError::NonFiniteOrigin { axis });
            }
            if *value == 0.0 {
                *value = 0.0;
            }
        }
        if !(-1074..=1023).contains(&step_exponent) {
            return Err(GridSpecError::StepExponentOutOfRange {
                exponent: step_exponent,
            });
        }
        Ok(Self {
            origin_xy_mm,
            step_exponent,
        })
    }
    /// Explicit world-XY origin in mm, interpreted as exact represented binary64 values.
    #[must_use]
    pub const fn origin_xy_mm(&self) -> [f64; 2] {
        self.origin_xy_mm
    }
    /// Exact uniform binary step exponent k; no floating reconstruction is performed.
    #[must_use]
    pub const fn step_exponent(&self) -> i16 {
        self.step_exponent
    }
    /// Metadata semantic version, including world XY/mm and `ExactOnly` behavior.
    #[must_use]
    pub const fn version(&self) -> u32 {
        1
    }
    /// Inclusive absolute integer bound, independent of resource limits.
    #[must_use]
    pub const fn max_abs_coordinate(&self) -> i64 {
        BOUND
    }
}

/// Invalid metadata, reported before any mapping operation is invoked.
#[derive(Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum GridSpecError {
    /// An explicit origin is NaN or infinite.
    NonFiniteOrigin {
        /// First nonfinite origin axis.
        axis: GridAxis,
    },
    /// Binary step exponent is outside the supported finite-power-of-two domain.
    StepExponentOutOfRange {
        /// Requested k, not a rounded scale.
        exponent: i16,
    },
}
impl fmt::Display for GridSpecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonFiniteOrigin { axis } => {
                write!(f, "exact grid {axis:?} origin must be finite")
            }
            Self::StepExponentOutOfRange { exponent } => {
                write!(
                    f,
                    "exact grid step exponent {exponent} is outside -1074..=1023"
                )
            }
        }
    }
}
impl Error for GridSpecError {}

/// One complete coordinate outcome per original graph vertex, not a detached validity certificate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GridVertexOutcome {
    /// Both agreed coordinates exactly equal origin+q*2^k, with abs(q)<=2^52.
    Exact {
        /// World X/Y integer indices under this view's spec; never a coordinate merge key.
        xy: [i64; 2],
    },
    /// Agreed represented coordinates are retained without requesting integer conversion.
    Represented,
    /// Upstream full-coordinate conflict; no representative or index pair is selected.
    UnresolvedCoordinates,
}

/// Independent input/work/requested-payload caps, not physical or process-RSS limits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GridLimits {
    /// Every graph vertex, including conflicts, isolated and complete-only features.
    pub max_vertices: usize,
    /// Planned coordinate conversions: two per agreed feature, including complete-only context.
    pub max_axis_conversions: usize,
    /// Requested flat outcome capacity; excludes input, stack and allocator overhead/over-allocation.
    pub max_payload_bytes: usize,
}
impl Default for GridLimits {
    fn default() -> Self {
        Self {
            max_vertices: 3_000_000,
            max_axis_conversions: 6_000_000,
            max_payload_bytes: 512 * 1024 * 1024,
        }
    }
}

/// Resource whose configured cap failed before reservation/publication.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GridResource {
    /// All original graph vertices.
    Vertices,
    /// Two planned axes per agreed feature.
    AxisConversions,
    /// Requested outcome payload, not actual allocator capacity or RSS.
    PayloadBytes,
}

/// Defensive inconsistencies in the upstream metadata actually read by mapping.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GridInputIssue {
    /// Graph vertex and normalized feature counts differ.
    FeatureCount,
    /// Borrowed world plane is not finite.
    Plane,
    /// Graph vertex ID does not equal its normalized feature-group ID.
    Association,
    /// Source-feature identities are not strictly ascending.
    FeatureOrder,
    /// Feature observation range is empty or outside the borrowed order buffer.
    ObservationRange,
    /// Agreed XYZ is nonfinite or its Z differs from the borrowed plane.
    Coordinates,
}

/// Atomic mapping failure; no partial coordinate view is returned.
#[derive(Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum GridError {
    /// Cancellation observed at its sourced checkpoint.
    Cancelled(Cancelled),
    /// A configured cap failed in deterministic preflight order.
    LimitExceeded {
        /// Resource that exceeded its limit.
        resource: GridResource,
        /// Caller-configured maximum.
        limit: usize,
    },
    /// Checked count, byte-size or isize capacity arithmetic failed.
    SizeOverflow,
    /// Fallible outcome reservation failed.
    Allocation,
    /// Privately constructed input metadata was inconsistent.
    InvalidInspection {
        /// Original graph vertex ID, when known.
        vertex: Option<usize>,
        /// Specific read-metadata inconsistency.
        reason: GridInputIssue,
    },
    /// Exact coordinate division has a nonzero residual, even if its quotient would also overflow.
    OffGrid {
        /// Original graph vertex ID, not a coordinate-sorted index.
        vertex: usize,
        /// First failing axis, X before Y.
        axis: GridAxis,
    },
    /// Exact quotient has magnitude greater than 2^52.
    CoordinateOutOfRange {
        /// Original graph vertex ID.
        vertex: usize,
        /// First failing axis.
        axis: GridAxis,
    },
    /// Defensive fixed-helper capacity failure, unreachable in the proved valid domain.
    ArithmeticOverflow {
        /// Original graph vertex ID.
        vertex: usize,
        /// Axis being converted.
        axis: GridAxis,
    },
}
impl fmt::Display for GridError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled(cause) => cause.fmt(f),
            Self::LimitExceeded { resource, limit } => {
                write!(f, "exact grid {resource:?} limit exceeded ({limit})")
            }
            Self::SizeOverflow => write!(f, "exact grid count or capacity arithmetic overflow"),
            Self::Allocation => write!(f, "unable to reserve exact grid outcomes"),
            Self::InvalidInspection { vertex, reason } => {
                write!(
                    f,
                    "inconsistent exact grid input at vertex {vertex:?}: {reason:?}"
                )
            }
            Self::OffGrid { vertex, axis } => {
                write!(
                    f,
                    "exact grid vertex {vertex} {axis:?} coordinate is off-grid"
                )
            }
            Self::CoordinateOutOfRange { vertex, axis } => {
                write!(
                    f,
                    "exact grid vertex {vertex} {axis:?} coordinate exceeds the integer domain"
                )
            }
            Self::ArithmeticOverflow { vertex, axis } => {
                write!(
                    f,
                    "exact grid arithmetic overflow at vertex {vertex}, axis {axis:?}"
                )
            }
        }
    }
}
impl Error for GridError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Cancelled(cause) => Some(cause),
            _ => None,
        }
    }
}
impl From<Cancelled> for GridError {
    fn from(cause: Cancelled) -> Self {
        Self::Cancelled(cause)
    }
}

/// Complete deterministic counts, not polygon or material acceptance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GridSummary {
    /// Agreed vertices retained in millimeters without integer conversion.
    pub represented_vertices: usize,
    /// All graph vertices; equals exact plus represented plus unresolved.
    pub vertices: usize,
    /// Agreed vertices successfully converted on both axes.
    pub exact_vertices: usize,
    /// Preserved upstream coordinate conflicts.
    pub unresolved_vertices: usize,
    /// Actual successful axis conversions; equals twice `exact_vertices`.
    pub axis_conversions: usize,
    /// Requested outcome payload only, excluding stack/input/allocator overhead.
    pub requested_payload_bytes: usize,
}

/// Complete coordinate view borrowing all unchanged inspection/source context.
#[derive(Debug, PartialEq)]
pub struct ExactGridView<'i, 'p, 'g, 'n, 'r> {
    input: &'i PlanarInspection<'p, 'g, 'n, 'r>,
    spec: BinaryGridSpec,
    represented: bool,
    outcomes: Vec<GridVertexOutcome>,
    summary: GridSummary,
}
impl<'i, 'p, 'g, 'n, 'r> ExactGridView<'i, 'p, 'g, 'n, 'r> {
    /// Whether coordinates were retained in millimeters and grid metadata is inactive.
    #[must_use]
    pub const fn uses_represented_coordinates(&self) -> bool {
        self.represented
    }

    /// The original inspection, including paths, complete-only context and every observation.
    #[must_use]
    pub const fn inspection(&self) -> &'i PlanarInspection<'p, 'g, 'n, 'r> {
        self.input
    }
    /// Caller grid metadata; inactive when `uses_represented_coordinates()` is true.
    /// Index pairs from different active specs are not world-coordinate comparisons.
    #[must_use]
    pub const fn spec(&self) -> BinaryGridSpec {
        self.spec
    }
    /// One outcome per original graph vertex in ascending ID order.
    #[must_use]
    pub fn outcomes(&self) -> &[GridVertexOutcome] {
        &self.outcomes
    }
    /// Copied outcome, or None for an out-of-range vertex ID.
    #[must_use]
    pub fn vertex(&self, id: usize) -> Option<GridVertexOutcome> {
        self.outcomes.get(id).copied()
    }
    /// Deterministic totals and requested payload, not memory/performance measurements.
    #[must_use]
    pub const fn summary(&self) -> GridSummary {
        self.summary
    }
}

type Result<T> = std::result::Result<T, GridError>;
fn invalid(vertex: Option<usize>, reason: GridInputIssue) -> GridError {
    GridError::InvalidInspection { vertex, reason }
}
fn cap(count: usize, limit: usize, resource: GridResource) -> Result<()> {
    if count > limit {
        Err(GridError::LimitExceeded { resource, limit })
    } else {
        Ok(())
    }
}
fn payload(count: usize) -> Result<usize> {
    let bytes = count
        .checked_mul(size_of::<GridVertexOutcome>())
        .ok_or(GridError::SizeOverflow)?;
    isize::try_from(bytes).map_err(|_| GridError::SizeOverflow)?;
    Ok(bytes)
}

fn preflight(
    vertices: &[GraphVertex],
    features: &[FeatureGroup],
    observation_order_len: usize,
    plane: f64,
    limits: GridLimits,
    cancel: &mut Cancellation<'_>,
) -> Result<GridSummary> {
    cap(vertices.len(), limits.max_vertices, GridResource::Vertices)?;
    if features.len() != vertices.len() {
        return Err(invalid(None, GridInputIssue::FeatureCount));
    }
    if !plane.is_finite() {
        return Err(invalid(None, GridInputIssue::Plane));
    }
    let mut exact_vertices = 0_usize;
    let mut unresolved_vertices = 0_usize;
    let mut previous = None;
    for (id, (vertex, feature)) in vertices.iter().zip(features).enumerate() {
        cancel.poll(Checkpoint::GridInput, id)?;
        if vertex.feature_group != id {
            return Err(invalid(Some(id), GridInputIssue::Association));
        }
        if previous.is_some_and(|p| p >= feature.feature) {
            return Err(invalid(Some(id), GridInputIssue::FeatureOrder));
        }
        previous = Some(feature.feature);
        if feature.observations.start >= feature.observations.end
            || feature.observations.end > observation_order_len
        {
            return Err(invalid(Some(id), GridInputIssue::ObservationRange));
        }
        match feature.coordinates {
            CoordinateAgreement::Agreed(point) => {
                if !point.x.is_finite()
                    || !point.y.is_finite()
                    || !point.z.is_finite()
                    || !(point.z.to_bits() == plane.to_bits() || (point.z == 0.0 && plane == 0.0))
                {
                    return Err(invalid(Some(id), GridInputIssue::Coordinates));
                }
                exact_vertices = exact_vertices
                    .checked_add(1)
                    .ok_or(GridError::SizeOverflow)?;
            }
            CoordinateAgreement::Conflict => {
                unresolved_vertices = unresolved_vertices
                    .checked_add(1)
                    .ok_or(GridError::SizeOverflow)?;
            }
        }
    }
    let axis_conversions = exact_vertices
        .checked_mul(2)
        .ok_or(GridError::SizeOverflow)?;
    cap(
        axis_conversions,
        limits.max_axis_conversions,
        GridResource::AxisConversions,
    )?;
    let requested_payload_bytes = payload(vertices.len())?;
    cap(
        requested_payload_bytes,
        limits.max_payload_bytes,
        GridResource::PayloadBytes,
    )?;
    Ok(GridSummary {
        represented_vertices: 0,
        vertices: vertices.len(),
        exact_vertices,
        unresolved_vertices,
        axis_conversions,
        requested_payload_bytes,
    })
}

/// Map every graph vertex with no rounding, geometry repair, source mutation or area prerequisite.
/// # Errors
/// Returns structured input, cap, storage, exact divisibility/range or cancellation failure.
pub fn map_exact_grid_vertices<'i, 'p, 'g, 'n, 'r>(
    input: &'i PlanarInspection<'p, 'g, 'n, 'r>,
    spec: BinaryGridSpec,
    limits: GridLimits,
) -> Result<ExactGridView<'i, 'p, 'g, 'n, 'r>> {
    map_exact_grid_vertices_with_cancellation(input, spec, limits, &mut Cancellation::never())
}

/// `ExactOnly` coordinate view with atomic TD-014 cancellation and complete retained context.
///
/// Preflight checks vertex cap, feature count, plane, per-ID metadata/coordinates, planned axis cap,
/// then checked requested payload. One fallible reservation precedes conversion in ID/X/Y order.
/// Off-grid residual wins before quotient range. Conflicts remain unresolved, never represented.
/// `GridInput`/`GridRecords` poll every 256 IDs; `GridCoordinates` uses a global axis count, also every 256.
/// `GridStart` precedes caps, `GridStorage` checks both sides of nonempty reservation, `GridComplete`
/// precedes publication even for empty/all-conflict input. Fixed per-axis arithmetic, allocation
/// and cleanup are indivisible, not latency/progress/RSS guarantees. Errors before the next check
/// retain precedence. Cost O(V+66*A), O(V) output and two fixed 66-word magnitudes plus scalars.
/// # Errors
/// No successful partial view escapes any error. Cancellation preserves its checkpoint/source;
/// limits, capacity/allocation, inconsistent read metadata, off-grid and range remain distinct.
pub fn map_exact_grid_vertices_with_cancellation<'i, 'p, 'g, 'n, 'r>(
    input: &'i PlanarInspection<'p, 'g, 'n, 'r>,
    spec: BinaryGridSpec,
    limits: GridLimits,
    cancel: &mut Cancellation<'_>,
) -> Result<ExactGridView<'i, 'p, 'g, 'n, 'r>> {
    cancel.check(Checkpoint::GridStart)?;
    let graph = input.paths().graph();
    let normalized = graph.normalized();
    let summary = preflight(
        graph.vertices(),
        normalized.features(),
        normalized.feature_order().len(),
        normalized.plane_z(),
        limits,
        cancel,
    )?;
    let mut outcomes = Vec::new();
    if summary.vertices > 0 {
        cancel.check(Checkpoint::GridStorage)?;
        outcomes
            .try_reserve_exact(summary.vertices)
            .map_err(|_| GridError::Allocation)?;
        cancel.check(Checkpoint::GridStorage)?;
    }
    let mut scratch = Scratch::new();
    let mut axes = 0_usize;
    for (vertex, feature) in normalized.features().iter().enumerate() {
        cancel.poll(Checkpoint::GridRecords, vertex)?;
        let outcome = match feature.coordinates {
            CoordinateAgreement::Conflict => GridVertexOutcome::UnresolvedCoordinates,
            CoordinateAgreement::Agreed(point) => {
                let mut xy = [0; 2];
                for (index, (value, axis)) in [point.x, point.y]
                    .into_iter()
                    .zip([GridAxis::X, GridAxis::Y])
                    .enumerate()
                {
                    cancel.poll(Checkpoint::GridCoordinates, axes)?;
                    xy[index] = scratch
                        .convert(value, spec.origin_xy_mm[index], spec.step_exponent)
                        .map_err(|error| error.context(vertex, axis))?;
                    axes = axes.checked_add(1).ok_or(GridError::SizeOverflow)?;
                }
                GridVertexOutcome::Exact { xy }
            }
        };
        outcomes.push(outcome);
    }
    // The immutable preflight and one-emission-per-feature loop guarantee publication accounting.
    debug_assert_eq!(outcomes.len(), summary.vertices);
    debug_assert_eq!(axes, summary.axis_conversions);
    cancel.check(Checkpoint::GridComplete)?;
    Ok(ExactGridView {
        input,
        spec,
        represented: false,
        outcomes,
        summary,
    })
}

/// Retain agreed binary64 millimeter coordinates without imposing an integer grid.
///
/// `spec` is retained solely as inactive caller metadata; every agreed outcome is
/// `Represented`, never `Exact`. Upstream conflicts remain unresolved. Vertex and payload
/// caps, cancellation, finite-coordinate validation and atomic publication match exact mapping.
/// No axis conversion is performed or charged against `max_axis_conversions`.
/// # Errors
/// Returns structured input, vertex/payload limit, storage or cancellation errors.
pub fn retain_represented_vertices_with_cancellation<'i, 'p, 'g, 'n, 'r>(
    input: &'i PlanarInspection<'p, 'g, 'n, 'r>,
    spec: BinaryGridSpec,
    mut limits: GridLimits,
    cancel: &mut Cancellation<'_>,
) -> Result<ExactGridView<'i, 'p, 'g, 'n, 'r>> {
    cancel.check(Checkpoint::GridStart)?;
    let graph = input.paths().graph();
    let normalized = graph.normalized();
    limits.max_axis_conversions = usize::MAX;
    let mut summary = preflight(
        graph.vertices(),
        normalized.features(),
        normalized.feature_order().len(),
        normalized.plane_z(),
        limits,
        cancel,
    )?;
    summary.represented_vertices = summary.exact_vertices;
    summary.exact_vertices = 0;
    summary.axis_conversions = 0;
    let mut outcomes = Vec::new();
    if summary.vertices != 0 {
        cancel.check(Checkpoint::GridStorage)?;
        outcomes
            .try_reserve_exact(summary.vertices)
            .map_err(|_| GridError::Allocation)?;
        cancel.check(Checkpoint::GridStorage)?;
    }
    for (id, feature) in normalized.features().iter().enumerate() {
        cancel.poll(Checkpoint::GridRecords, id)?;
        outcomes.push(match feature.coordinates {
            CoordinateAgreement::Agreed(_) => GridVertexOutcome::Represented,
            CoordinateAgreement::Conflict => GridVertexOutcome::UnresolvedCoordinates,
        });
    }
    cancel.check(Checkpoint::GridComplete)?;
    Ok(ExactGridView {
        input,
        spec,
        represented: true,
        outcomes,
        summary,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AxisFailure {
    OffGrid,
    OutOfRange,
    Overflow,
}
impl AxisFailure {
    fn context(self, vertex: usize, axis: GridAxis) -> GridError {
        match self {
            Self::OffGrid => GridError::OffGrid { vertex, axis },
            Self::OutOfRange => GridError::CoordinateOutOfRange { vertex, axis },
            Self::Overflow => GridError::ArithmeticOverflow { vertex, axis },
        }
    }
}
type AxisResult<T> = std::result::Result<T, AxisFailure>;

// Two magnitudes cover |X-O|<2^2099 eta units; 66*32=2112. No public arithmetic API.
struct Scratch {
    left: [u32; WORDS],
    right: [u32; WORDS],
}
impl Scratch {
    const fn new() -> Self {
        Self {
            left: [0; WORDS],
            right: [0; WORDS],
        }
    }

    fn decode(value: f64, words: &mut [u32; WORDS]) -> AxisResult<bool> {
        words.fill(0);
        let bits = value.to_bits();
        let exponent = usize::try_from((bits >> 52) & 2047).map_err(|_| AxisFailure::Overflow)?;
        if exponent == 2047 {
            return Err(AxisFailure::Overflow);
        }
        let fraction = bits & ((1_u64 << 52) - 1);
        let mantissa = if exponent == 0 {
            fraction
        } else {
            (1 << 52) | fraction
        };
        let shift = exponent.saturating_sub(1);
        for (offset, fragment) in [mantissa & MASK, mantissa >> 32].into_iter().enumerate() {
            // Shift a <=32-bit fragment by <=31, not an entire 53-bit mantissa in u64.
            let value = fragment << (shift % 32);
            let index = shift / 32 + offset;
            *words.get_mut(index).ok_or(AxisFailure::Overflow)? |=
                u32::try_from(value & MASK).map_err(|_| AxisFailure::Overflow)?;
            if value >> 32 != 0 {
                *words.get_mut(index + 1).ok_or(AxisFailure::Overflow)? |=
                    u32::try_from(value >> 32).map_err(|_| AxisFailure::Overflow)?;
            }
        }
        Ok(bits >> 63 != 0 && mantissa != 0)
    }

    fn difference(&mut self, negative_x: bool, negative_origin: bool) -> AxisResult<bool> {
        if negative_x != negative_origin {
            let mut carry = 0;
            for (left, &right) in self.left.iter_mut().zip(self.right.iter()) {
                let sum = u64::from(*left) + u64::from(right) + carry;
                *left = u32::try_from(sum & MASK).map_err(|_| AxisFailure::Overflow)?;
                carry = sum >> 32;
            }
            if carry != 0 {
                return Err(AxisFailure::Overflow);
            }
            return Ok(negative_x);
        }
        let reverse = self.left.iter().rev().cmp(self.right.iter().rev()) == Ordering::Less;
        let mut borrow = 0;
        for (left, &right) in self.left.iter_mut().zip(self.right.iter()) {
            let (a, b) = if reverse {
                (u64::from(right), u64::from(*left))
            } else {
                (u64::from(*left), u64::from(right))
            };
            let subtrahend = b + borrow;
            let word = if a < subtrahend {
                borrow = 1;
                a + (1_u64 << 32) - subtrahend
            } else {
                borrow = 0;
                a - subtrahend
            };
            *left = u32::try_from(word).map_err(|_| AxisFailure::Overflow)?;
        }
        if borrow != 0 {
            return Err(AxisFailure::Overflow);
        }
        Ok((negative_x != reverse) && self.left.iter().any(|&word| word != 0))
    }

    fn quotient(&self, shift: usize, negative: bool) -> AxisResult<i64> {
        if shift > 2097 {
            return Err(AxisFailure::Overflow);
        }
        let whole = shift / 32;
        let tail = shift % 32;
        if self.left[..whole].iter().any(|&word| word != 0)
            || (tail != 0 && self.left[whole] & ((1_u32 << tail) - 1) != 0)
        {
            return Err(AxisFailure::OffGrid);
        }
        let Some(index) = self.left.iter().rposition(|&word| word != 0) else {
            return Ok(0);
        };
        let width = usize::try_from(32 - self.left[index].leading_zeros())
            .map_err(|_| AxisFailure::Overflow)?;
        let bits = (index * 32 + width)
            .checked_sub(shift)
            .ok_or(AxisFailure::Overflow)?;
        if bits > 53 {
            return Err(AxisFailure::OutOfRange);
        }
        let mut quotient = 0_u64;
        for offset in 0..bits {
            let bit = shift + offset;
            let word = self.left.get(bit / 32).ok_or(AxisFailure::Overflow)?;
            quotient |= u64::from((word >> (bit % 32)) & 1) << offset;
        }
        if quotient > (1_u64 << 52) {
            return Err(AxisFailure::OutOfRange);
        }
        let value = i64::try_from(quotient).map_err(|_| AxisFailure::Overflow)?;
        Ok(if negative { -value } else { value })
    }

    fn convert(&mut self, value: f64, origin: f64, exponent: i16) -> AxisResult<i64> {
        if !(-1074..=1023).contains(&exponent) {
            return Err(AxisFailure::Overflow);
        }
        let negative_x = Self::decode(value, &mut self.left)?;
        let negative_origin = Self::decode(origin, &mut self.right)?;
        let negative = self.difference(negative_x, negative_origin)?;
        let shift =
            usize::try_from(i32::from(exponent) + 1074).map_err(|_| AxisFailure::Overflow)?;
        self.quotient(shift, negative)
    }
}

#[cfg(test)]
mod tests;
