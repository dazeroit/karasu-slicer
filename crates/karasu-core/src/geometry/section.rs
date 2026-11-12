//! Local triangle–horizontal-plane intersection in one caller-selected coordinate space (mm).
//!
//! Finite represented `f64` values classify exactly, without subtraction or tolerance. The five
//! raw incidence events preserve source identity even for repeated positions or coincident outputs;
//! they do not certify positive area, exact affine rank, a contour, or a printable region.
//! Construction follows Sections 2–6 of `docs/M3-TRIANGLE-PLANE-CONTRACT.md`: separate nearest-even
//! binary64 operations with gradual underflow, no fused multiply-add or reassociation. Arbitrary
//! changes to the caller's floating-point environment are unsupported.
//!
//! The operation uses constant work/stack storage, no allocation, dependency, mesh traversal,
//! cancellation callback, welding, repair, shared-edge ownership policy, or layer schedule.
//!
//! ```
//! use karasu_core::geometry::{Point3, section::{intersect_triangle_z, TrianglePlaneEvent}};
//! let result = intersect_triangle_z([
//!     Point3::new(0.0, 0.0, -1.0),
//!     Point3::new(4.0, 0.0, 1.0),
//!     Point3::new(0.0, 4.0, 1.0),
//! ], 0.0)?;
//! if let TrianglePlaneEvent::Segment { endpoints, .. } = result.event {
//!     assert_eq!(endpoints[0].position, Point3::new(0.0, 2.0, 0.0));
//!     assert_eq!(endpoints[1].position, Point3::new(2.0, 0.0, 0.0));
//! } else {
//!     panic!("expected a segment");
//! }
//! # Ok::<(), karasu_core::geometry::section::TrianglePlaneError>(())
//! ```

use super::Point3;
use std::{cmp::Ordering, error::Error, fmt};

/// Exact relation of a finite represented vertex Z to the supplied plane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VertexPlaneSide {
    /// Vertex Z is strictly smaller than plane Z.
    Below,
    /// Vertex Z equals plane Z (including either sign of zero).
    On,
    /// Vertex Z is strictly larger than plane Z.
    Above,
}

/// Source corner identity in the supplied three-point array.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CornerId {
    /// Input corner 0.
    Corner0,
    /// Input corner 1.
    Corner1,
    /// Input corner 2.
    Corner2,
}

/// Local source edge, in the same 01/12/20 order as M2 triangle incidence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LocalEdge {
    /// Source corners 0 to 1.
    Edge01,
    /// Source corners 1 to 2.
    Edge12,
    /// Source corners 2 to 0.
    Edge20,
}

impl LocalEdge {
    fn indices(self) -> [usize; 2] {
        match self {
            Self::Edge01 => [0, 1],
            Self::Edge12 => [1, 2],
            Self::Edge20 => [2, 0],
        }
    }
}

/// Identity survives rounding or coincidence with another endpoint/source corner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EndpointOrigin {
    /// Exact on-plane source corner, apart from signed-zero normalization.
    Vertex(CornerId),
    /// A strict-crossing source edge, even if its result rounds to a corner.
    Edge(LocalEdge),
}

/// A finite, zero-normalized point and its local source identity.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SectionEndpoint {
    /// Computed or exact source point in the input coordinate space, in mm.
    pub position: Point3,
    /// Exact source incidence, not a coordinate-based identity.
    pub origin: EndpointOrigin,
}

/// One preserved corner of a raw coplanar triangle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SourceCorner {
    /// Identity in the original input array.
    pub corner: CornerId,
    /// Exact source position with numerical zeros normalized.
    pub position: Point3,
}

/// Equality of the two returned represented positions; not an exact-real rank test.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EndpointCoincidence {
    /// Returned positions differ exactly.
    Distinct,
    /// Returned positions agree exactly; both origins are still retained.
    CoincidentRepresented,
}

/// Qualified source-area observation, never a certified exact affine-rank predicate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TriangleAreaObservation {
    /// At least one pair of source positions is exactly equal.
    RepeatedPosition,
    /// The specified coordinate-ordered, scaled cross product computed zero.
    ZeroComputedCross,
    /// The specified computed cross product has a nonzero component (not an exact rank proof).
    NonzeroComputedCross,
    /// A finite edge difference or normalization could not be represented.
    UnavailableRange,
}

/// Source context remains available even for an empty intersection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TriangleContext {
    /// Exact equal-position flags in original local edge order 01, 12, 20.
    pub equal_corner_pairs: [bool; 3],
    /// Computed source-area context, independent of the intersection event.
    pub area_observation: TriangleAreaObservation,
}

/// Raw source incidence, not shared-face ownership or a filled-region assertion.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TrianglePlaneEvent {
    /// All three corners lie on one strict side of the plane.
    None,
    /// One on-plane corner, with both other corners on the same strict side.
    TangentPoint {
        /// Exact on-plane source corner.
        endpoint: SectionEndpoint,
    },
    /// Both strict sides occur, with zero or one on-plane corner.
    Segment {
        /// Canonically ordered endpoints; distinct origins survive coincidence.
        endpoints: [SectionEndpoint; 2],
        /// Exact equality of returned positions.
        coincidence: EndpointCoincidence,
    },
    /// Exactly two corners are on-plane, even if their positions agree.
    CoplanarEdge {
        /// Complete local source edge identity.
        edge: LocalEdge,
        /// Canonically ordered exact source endpoints.
        endpoints: [SectionEndpoint; 2],
        /// Exact equality of returned positions.
        coincidence: EndpointCoincidence,
    },
    /// All corners are on-plane; their convex hull may be a point, line, or area.
    CoplanarTriangle {
        /// All source corners, sorted by position then local corner ID.
        corners: [SourceCorner; 3],
    },
}

/// Fixed-size intersection result; guarantees describe kernel-produced values.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrianglePlaneIntersection {
    /// Exact sides in original corner order, not canonical coordinate order.
    pub sides: [VertexPlaneSide; 3],
    /// Independent source equality and computed-area context.
    pub context: TriangleContext,
    /// Raw bounded incidence event.
    pub event: TrianglePlaneEvent,
}

/// Invalid input or unrepresentable necessary construction; no partial result is returned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum TrianglePlaneError {
    /// A source coordinate is NaN or infinite (validated corner-first, then X/Y/Z).
    NonFiniteVertex {
        /// First invalid source corner.
        corner: CornerId,
        /// Coordinate index, X=0, Y=1, Z=2.
        axis: usize,
    },
    /// Plane Z is NaN or infinite; checked after every source coordinate.
    NonFinitePlane,
    /// A required positive plane-distance subtraction overflowed.
    DistanceOverflow {
        /// Strict-crossing local edge.
        edge: LocalEdge,
    },
    /// Required distances/weights violate the documented positive/normal/range domain.
    UnrepresentableParameter {
        /// Strict-crossing local edge.
        edge: LocalEdge,
    },
    /// A required interpolation intermediate/output is non-finite (guarded even if unreachable).
    CoordinateOverflow {
        /// Strict-crossing local edge.
        edge: LocalEdge,
        /// Interpolation coordinate, X=0 or Y=1.
        axis: usize,
    },
}

impl fmt::Display for TrianglePlaneError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonFiniteVertex { corner, axis } => {
                write!(f, "section vertex {corner:?}, axis {axis}: value must be finite")
            }
            Self::NonFinitePlane => write!(f, "section plane Z must be finite"),
            Self::DistanceOverflow { edge } => write!(f, "section edge {edge:?}: plane distance overflow"),
            Self::UnrepresentableParameter { edge } => write!(f, "section edge {edge:?}: interpolation parameter outside the supported normal-positive domain"),
            Self::CoordinateOverflow { edge, axis } => write!(f, "section edge {edge:?}, axis {axis}: coordinate construction overflow"),
        }
    }
}

impl Error for TrianglePlaneError {}

const CORNERS: [CornerId; 3] = [CornerId::Corner0, CornerId::Corner1, CornerId::Corner2];
const EDGES: [LocalEdge; 3] = [LocalEdge::Edge01, LocalEdge::Edge12, LocalEdge::Edge20];

/// Intersects one closed three-point convex hull with the explicit plane `z = plane_z` (mm).
///
/// Inputs and plane must share a coordinate space. No input is changed. The operation takes O(1)
/// work/stack space without allocation. Classification is exact for finite represented values;
/// construction is approximate, with coordinate-wise absolute error at most
/// `64 * 2^-53 * max(abs(edge_axis_endpoints)) + 16 * 2^-1074` mm under the module's arithmetic
/// assumptions. This is not an epsilon, correct-rounding or relative-error guarantee.
///
/// # Errors
///
/// Validates corners 0/1/2, each X/Y/Z, then the plane. Necessary strict crossings alone can fail:
/// non-finite distances, non-normal positive small weights, or guarded coordinate overflow.
/// Active edges are evaluated in geometric-key order, then X/Y, for deterministic error precedence.
/// Exact on-plane copies do not require interpolation. Source-area range failures remain context,
/// not intersection errors. No partial event or approximate fallback is returned.
pub fn intersect_triangle_z(
    vertices: [Point3; 3],
    plane_z: f64,
) -> Result<TrianglePlaneIntersection, TrianglePlaneError> {
    for (corner, point) in CORNERS.into_iter().zip(vertices) {
        for (axis, value) in point.coordinates().into_iter().enumerate() {
            if !value.is_finite() {
                return Err(TrianglePlaneError::NonFiniteVertex { corner, axis });
            }
        }
    }
    if !plane_z.is_finite() {
        return Err(TrianglePlaneError::NonFinitePlane);
    }
    let vertices = vertices.map(normalize_point);
    let plane_z = normalize_zero(plane_z);
    let sides = vertices.map(|point| classify(point.z, plane_z));
    Ok(TrianglePlaneIntersection {
        sides,
        context: observe(vertices),
        event: select_event(vertices, plane_z, sides)?,
    })
}

fn classify(z: f64, plane_z: f64) -> VertexPlaneSide {
    if z < plane_z {
        VertexPlaneSide::Below
    } else if z > plane_z {
        VertexPlaneSide::Above
    } else {
        VertexPlaneSide::On
    }
}

fn select_event(
    vertices: [Point3; 3],
    plane_z: f64,
    sides: [VertexPlaneSide; 3],
) -> Result<TrianglePlaneEvent, TrianglePlaneError> {
    let on_count = sides
        .iter()
        .filter(|&&side| side == VertexPlaneSide::On)
        .count();
    if on_count == 3 {
        let mut corners = CORNERS.map(|corner| SourceCorner {
            corner,
            position: vertices[corner as usize],
        });
        corners.sort_unstable_by(|a, b| {
            point_cmp(a.position, b.position)
                .then_with(|| (a.corner as usize).cmp(&(b.corner as usize)))
        });
        return Ok(TrianglePlaneEvent::CoplanarTriangle { corners });
    }
    if on_count == 2 {
        // Exactly one edge joins the two On corners, for all six ordered side words.
        for edge in EDGES {
            let [a, b] = edge.indices();
            if sides[a] == VertexPlaneSide::On && sides[b] == VertexPlaneSide::On {
                let mut endpoints = [vertex_endpoint(vertices, a), vertex_endpoint(vertices, b)];
                canonicalize(&mut endpoints, vertices);
                return Ok(TrianglePlaneEvent::CoplanarEdge {
                    edge,
                    endpoints,
                    coincidence: coincidence(endpoints),
                });
            }
        }
    }
    let crosses =
        sides.contains(&VertexPlaneSide::Below) && sides.contains(&VertexPlaneSide::Above);
    if !crosses {
        return Ok(
            match sides.iter().position(|&side| side == VertexPlaneSide::On) {
                Some(corner) => TrianglePlaneEvent::TangentPoint {
                    endpoint: vertex_endpoint(vertices, corner),
                },
                None => TrianglePlaneEvent::None,
            },
        );
    }
    let mut endpoints = [vertex_endpoint(vertices, 0); 2];
    let mut count = 0;
    if let Some(corner) = sides.iter().position(|&side| side == VertexPlaneSide::On) {
        endpoints[count] = vertex_endpoint(vertices, corner);
        count += 1;
    }
    let mut edges = EDGES;
    edges.sort_unstable_by(|&a, &b| edge_cmp(a, b, vertices));
    for edge in edges {
        let [a, b] = edge.indices();
        if matches!(
            (sides[a], sides[b]),
            (VertexPlaneSide::Below, VertexPlaneSide::Above)
                | (VertexPlaneSide::Above, VertexPlaneSide::Below)
        ) {
            endpoints[count] = construct(vertices, plane_z, edge)?;
            count += 1;
        }
    }
    // The exhaustive 27-state table gives exactly two records whenever both strict sides occur.
    debug_assert_eq!(count, 2);
    canonicalize(&mut endpoints, vertices);
    Ok(TrianglePlaneEvent::Segment {
        endpoints,
        coincidence: coincidence(endpoints),
    })
}

fn vertex_endpoint(vertices: [Point3; 3], corner: usize) -> SectionEndpoint {
    SectionEndpoint {
        position: vertices[corner],
        origin: EndpointOrigin::Vertex(CORNERS[corner]),
    }
}

fn construct(
    vertices: [Point3; 3],
    plane_z: f64,
    edge: LocalEdge,
) -> Result<SectionEndpoint, TrianglePlaneError> {
    let [a, b] = edge.indices();
    let (lo, hi) = if vertices[a].z < vertices[b].z {
        (vertices[a], vertices[b])
    } else {
        (vertices[b], vertices[a])
    };
    let d_lo = plane_z - lo.z;
    let d_hi = hi.z - plane_z;
    if !d_lo.is_finite() || !d_hi.is_finite() {
        return Err(TrianglePlaneError::DistanceOverflow { edge });
    }
    if d_lo <= 0.0 || d_hi <= 0.0 {
        return Err(TrianglePlaneError::UnrepresentableParameter { edge });
    }
    let (near, far, d_near, d_far) = if d_lo <= d_hi {
        (lo, hi, d_lo, d_hi)
    } else {
        (hi, lo, d_hi, d_lo)
    };
    let ratio = d_near / d_far;
    let denominator = 1.0 + ratio;
    let weight = ratio / denominator;
    if !normal_positive(ratio) || !normal_positive(weight) || weight > 0.5 {
        return Err(TrianglePlaneError::UnrepresentableParameter { edge });
    }
    let x = interpolate(near.x, far.x, weight, edge, 0)?;
    let y = interpolate(near.y, far.y, weight, edge, 1)?;
    Ok(SectionEndpoint {
        position: Point3::new(x, y, plane_z),
        origin: EndpointOrigin::Edge(edge),
    })
}

fn normal_positive(value: f64) -> bool {
    value.is_normal() && value > 0.0
}

fn coordinate_finite(value: f64, edge: LocalEdge, axis: usize) -> Result<f64, TrianglePlaneError> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(TrianglePlaneError::CoordinateOverflow { edge, axis })
    }
}

#[allow(clippy::float_cmp)] // Exact endpoint equality is required; no approximate snapping is allowed.
fn interpolate(
    near: f64,
    far: f64,
    weight: f64,
    edge: LocalEdge,
    axis: usize,
) -> Result<f64, TrianglePlaneError> {
    if near == far {
        return Ok(normalize_zero(near));
    }
    let value = if (near < 0.0 && far > 0.0) || (near > 0.0 && far < 0.0) {
        let complement = coordinate_finite(1.0 - weight, edge, axis)?;
        let first = coordinate_finite(complement * near, edge, axis)?;
        let second = coordinate_finite(weight * far, edge, axis)?;
        coordinate_finite(first + second, edge, axis)?
    } else {
        let delta = coordinate_finite(far - near, edge, axis)?;
        let offset = coordinate_finite(weight * delta, edge, axis)?;
        coordinate_finite(near + offset, edge, axis)?
    };
    // Projection to the source interval cannot increase error from the ideal convex interpolant.
    Ok(normalize_zero(value.clamp(near.min(far), near.max(far))))
}

fn observe(mut vertices: [Point3; 3]) -> TriangleContext {
    let equal_corner_pairs = EDGES.map(|edge| {
        let [a, b] = edge.indices();
        vertices[a] == vertices[b]
    });
    let area_observation = if equal_corner_pairs.contains(&true) {
        TriangleAreaObservation::RepeatedPosition
    } else {
        vertices.sort_unstable_by(|&a, &b| point_cmp(a, b));
        match (
            scaled_edge(vertices[0], vertices[1]),
            scaled_edge(vertices[0], vertices[2]),
        ) {
            (Some(a), Some(b)) => {
                let cross = [
                    a[1] * b[2] - a[2] * b[1],
                    a[2] * b[0] - a[0] * b[2],
                    a[0] * b[1] - a[1] * b[0],
                ];
                if !cross.iter().all(|v| v.is_finite()) {
                    TriangleAreaObservation::UnavailableRange
                } else if cross.iter().all(|&component| component == 0.0) {
                    TriangleAreaObservation::ZeroComputedCross
                } else {
                    TriangleAreaObservation::NonzeroComputedCross
                }
            }
            _ => TriangleAreaObservation::UnavailableRange,
        }
    };
    TriangleContext {
        equal_corner_pairs,
        area_observation,
    }
}

fn scaled_edge(a: Point3, b: Point3) -> Option<[f64; 3]> {
    let delta = [b.x - a.x, b.y - a.y, b.z - a.z];
    let scale = delta
        .iter()
        .fold(0.0_f64, |scale, value| scale.max(value.abs()));
    if scale == 0.0 || !delta.iter().all(|v| v.is_finite()) {
        return None;
    }
    let scaled = delta.map(|value| value / scale);
    scaled.iter().all(|v| v.is_finite()).then_some(scaled)
}

fn normalize_zero(value: f64) -> f64 {
    if value == 0.0 {
        0.0
    } else {
        value
    }
}

fn normalize_point(point: Point3) -> Point3 {
    let [x, y, z] = point.coordinates().map(normalize_zero);
    Point3::new(x, y, z)
}

fn point_cmp(a: Point3, b: Point3) -> Ordering {
    a.x.total_cmp(&b.x)
        .then_with(|| a.y.total_cmp(&b.y))
        .then_with(|| a.z.total_cmp(&b.z))
}

fn edge_points(edge: LocalEdge, vertices: [Point3; 3]) -> [Point3; 2] {
    let [a, b] = edge.indices();
    let mut points = [vertices[a], vertices[b]];
    if point_cmp(points[0], points[1]).is_gt() {
        points.swap(0, 1);
    }
    points
}

fn edge_cmp(a: LocalEdge, b: LocalEdge, vertices: [Point3; 3]) -> Ordering {
    let a_points = edge_points(a, vertices);
    let b_points = edge_points(b, vertices);
    point_cmp(a_points[0], b_points[0])
        .then_with(|| point_cmp(a_points[1], b_points[1]))
        .then_with(|| (a as usize).cmp(&(b as usize)))
}

fn origin_cmp(a: EndpointOrigin, b: EndpointOrigin, vertices: [Point3; 3]) -> Ordering {
    match (a, b) {
        (EndpointOrigin::Vertex(a), EndpointOrigin::Vertex(b)) => (a as usize).cmp(&(b as usize)),
        (EndpointOrigin::Vertex(_), EndpointOrigin::Edge(_)) => Ordering::Less,
        (EndpointOrigin::Edge(_), EndpointOrigin::Vertex(_)) => Ordering::Greater,
        (EndpointOrigin::Edge(a), EndpointOrigin::Edge(b)) => edge_cmp(a, b, vertices),
    }
}

fn canonicalize(endpoints: &mut [SectionEndpoint; 2], vertices: [Point3; 3]) {
    if point_cmp(endpoints[0].position, endpoints[1].position)
        .then_with(|| origin_cmp(endpoints[0].origin, endpoints[1].origin, vertices))
        .is_gt()
    {
        endpoints.swap(0, 1);
    }
}

fn coincidence(endpoints: [SectionEndpoint; 2]) -> EndpointCoincidence {
    if endpoints[0].position == endpoints[1].position {
        EndpointCoincidence::CoincidentRepresented
    } else {
        EndpointCoincidence::Distinct
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parameter_domain_rejects_zero_subnormal_negative_and_nonfinite_values() {
        for value in [
            0.0,
            -0.0,
            f64::from_bits(1),
            f64::from_bits(f64::MIN_POSITIVE.to_bits() - 1),
            -1.0,
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
        ] {
            assert!(!normal_positive(value));
        }
        assert!(normal_positive(f64::MIN_POSITIVE));
        assert!(normal_positive(0.5));
    }

    #[test]
    fn coordinate_guard_reports_each_nonfinite_value_and_axis() {
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            for edge in EDGES {
                for axis in 0..2 {
                    assert_eq!(
                        coordinate_finite(value, edge, axis),
                        Err(TrianglePlaneError::CoordinateOverflow { edge, axis })
                    );
                }
            }
        }
        assert_eq!(
            coordinate_finite(f64::MAX, LocalEdge::Edge01, 0),
            Ok(f64::MAX)
        );
    }

    #[test]
    fn private_classifier_does_not_subtract_extreme_coordinates() {
        assert_eq!(classify(-f64::MAX, f64::MAX), VertexPlaneSide::Below);
        assert_eq!(classify(f64::MAX, -f64::MAX), VertexPlaneSide::Above);
        assert_eq!(classify(-0.0, 0.0), VertexPlaneSide::On);
    }

    #[test]
    fn geometric_edge_order_precedes_local_id_and_ties_use_id() {
        let vertices = [
            Point3::new(2.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(0.0, 0.0, 0.0),
        ];
        assert!(edge_cmp(LocalEdge::Edge12, LocalEdge::Edge01, vertices).is_lt());
        assert!(edge_cmp(LocalEdge::Edge01, LocalEdge::Edge20, [vertices[0]; 3]).is_lt());
    }
}
