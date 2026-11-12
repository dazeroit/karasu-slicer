//! Winding-number layers (TD-050): the oriented plane section of the source surface, filled under
//! an explicit [`WindingRule`] instead of strict even-odd regions.
//!
//! Winding layers are always sectioned on vertex-avoiding planes (TD-048), so every crossed face
//! meets the plane in one segment between crossing points on two of its edges. A crossing point
//! depends only on its edge's two world vertices, so faces sharing an edge obtain bit-identical
//! endpoints. Each segment's direction comes from the face's cyclic vertex order and the side of
//! its lone vertex, not from arithmetic, so the material of an outward-oriented surface lies on
//! its left.
//!
//! Nothing is moved, merged, added or dropped. Instead the section must balance exactly: at every
//! crossing point as many segments leave as arrive. A closed surface always balances, including
//! overlapping bodies, self-intersecting shells and non-manifold edges; only an open boundary or
//! inconsistently oriented faces can unbalance it, and such a layer fails with the crossings
//! involved. On a balanced section the winding number of every point off the section is well
//! defined, and the rule turns it into material.

use super::{
    engine::LayerFailure, schedule::LayerPlane, FaceSection, MeshSection, SectionTransform,
};
use crate::mesh::topology::RimTable;
use crate::{
    cancellation::{Cancellation, Checkpoint},
    geometry::{
        section::{EndpointOrigin, LocalEdge, TrianglePlaneEvent, VertexPlaneSide},
        Point3,
    },
    preparation::hollow::numeric::orient3d,
};
use std::{cmp::Ordering, mem::size_of, ops::Range};

/// How a winding number selects material.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindingRule {
    /// Material where the winding number is positive: the union of outward bodies, with reversed
    /// (inward) shells such as hollowing cavities subtracting. A negative sampled winding number
    /// means an inside-out or unenclosed reversed shell and fails the layer's raster.
    Positive,
    /// [`Self::Positive`], except that up to `max_negative_samples` raster samples per layer may
    /// have a negative winding number: they stay empty, as under [`Self::Positive`], and the layer
    /// reports how many there were. More fail the layer. This is an explicit policy for the tiny
    /// reversed slivers that self-intersecting folds leave in some models (TD-079); a body that is
    /// inside out as a whole still fails.
    PositiveBounded {
        /// Largest number of negative samples a layer may have.
        max_negative_samples: u64,
    },
    /// Material where the winding number is nonzero, for sources oriented inside out as a whole.
    NonZero,
    /// Material where the winding number is odd: the strict pipeline's even-odd semantics.
    EvenOdd,
}

impl WindingRule {
    /// Whether a point with winding number `winding` is material.
    #[must_use]
    pub const fn fills(self, winding: i64) -> bool {
        match self {
            Self::Positive | Self::PositiveBounded { .. } => winding > 0,
            Self::NonZero => winding != 0,
            Self::EvenOdd => winding % 2 != 0,
        }
    }
}

/// Closure of open sections at holes (TD-054): open chains at a hole are joined by the chord
/// across the hole, paired along the hole's boundary rim, never by proximity.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SectionClosure {
    /// Longest admitted chord (mm, world XY); a longer one fails the layer.
    pub max_gap_mm: f64,
}

/// What section closure added to one layer.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ClosureReport {
    /// Chords added.
    pub chords: usize,
    /// Longest chord (mm), zero without chords.
    pub longest_mm: f64,
    /// Rims crossed by the plane more than twice, where the shorter of two matchings was chosen.
    pub ambiguous_rims: usize,
}

/// Section closure with the job's rim table.
#[derive(Clone, Copy)]
pub(crate) struct Closure<'a> {
    pub(crate) rims: &'a RimTable,
    pub(crate) max_gap_mm: f64,
}

/// One balanced oriented layer section, ready for winding-rule rasterization.
#[derive(Debug, PartialEq)]
pub struct WindingLayer {
    plane: LayerPlane,
    sampled_z_mm: f64,
    segments: Vec<[[f64; 2]; 2]>,
    /// Copies of the segments of the faces a cut leaves (TD-123), in the same order.
    kept: Vec<[[f64; 2]; 2]>,
    /// The other segments, closure chords included, in the same order: the model alone, for
    /// binary supports (TD-134). Empty when nothing is kept.
    model: Vec<[[f64; 2]; 2]>,
    closure: ClosureReport,
}

impl WindingLayer {
    /// The scheduled plane this layer represents.
    #[must_use]
    pub const fn plane(&self) -> LayerPlane {
        self.plane
    }

    /// The world Z actually sectioned (mm): the scheduled Z moved off every vertex height.
    #[must_use]
    pub const fn sampled_z_mm(&self) -> f64 {
        self.sampled_z_mm
    }

    /// Directed world-XY segments `[from, to]` (mm) with the material of an outward-oriented
    /// surface on their left, in ascending source-face order, followed by closure chords.
    #[must_use]
    pub fn segments(&self) -> &[[[f64; 2]; 2]] {
        &self.segments
    }

    /// The segments of the faces a cut leaves whole (the supports, TD-123), in segment order:
    /// a subset of [`Self::segments`], without closure chords. Closed bodies of their own, they
    /// bound where the cut does not apply. Empty unless the kernel was given such faces.
    #[must_use]
    pub fn kept_segments(&self) -> &[[[f64; 2]; 2]] {
        &self.kept
    }

    /// The segments of the faces not kept, closure chords included, in segment order: the model
    /// without its supports (TD-134). Empty unless the kernel was given kept faces.
    #[must_use]
    pub fn model_segments(&self) -> &[[[f64; 2]; 2]] {
        &self.model
    }

    /// Chords added by section closure.
    #[must_use]
    pub const fn closure(&self) -> ClosureReport {
        self.closure
    }
}

/// A section crossing point where leaving and arriving segments do not balance.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UnbalancedCrossing {
    /// World XY of the crossing point (mm).
    pub position_mm: [f64; 2],
    /// Source vertex indices (ascending) of one mesh edge crossing the plane there.
    pub edge: [u32; 2],
    /// Leaving minus arriving segments. Odd: an odd number of faces use the edge (an open
    /// boundary). Even: faces sharing the edge are inconsistently oriented.
    pub excess: i64,
}

impl UnbalancedCrossing {
    /// Whether the imbalance comes from an odd number of faces, such as an open boundary edge.
    #[must_use]
    pub const fn open_boundary(&self) -> bool {
        self.excess % 2 != 0
    }
}

/// At most this many unbalanced crossings are listed in a failure; the total is always given.
pub const MAX_UNBALANCED_DETAILS: usize = 16;

/// Whether `transform` reverses orientation (its linear part has a negative determinant), so the
/// combinatorial segment directions must be reversed to keep the material on their left.
///
/// # Errors
/// [`LayerFailure::SingularTransform`] for a singular linear part: winding numbers of a collapsed
/// image are undefined.
pub(crate) fn reflects(transform: SectionTransform<'_>) -> Result<bool, LayerFailure> {
    let sign = match transform {
        SectionTransform::Identity => Ordering::Greater,
        // A rotation is proper, so only the local scale can reflect.
        SectionTransform::Object(object) => {
            let scale = object.scale().components();
            if scale.contains(&0.0) {
                Ordering::Equal
            } else if scale.iter().filter(|&&s| s < 0.0).count() % 2 == 1 {
                Ordering::Less
            } else {
                Ordering::Greater
            }
        }
        // Exact sign of the determinant: the orientation of the three image columns.
        SectionTransform::Affine(affine) => {
            let m = affine.linear();
            let column = |j: usize| Point3::new(m[0][j], m[1][j], m[2][j]);
            orient3d(Point3::new(0., 0., 0.), column(0), column(1), column(2))
        }
    };
    match sign {
        Ordering::Greater => Ok(false),
        Ordering::Less => Ok(true),
        Ordering::Equal => Err(LayerFailure::SingularTransform),
    }
}

/// A directed segment `[from, to]` in world XY (mm).
type Segment = [[f64; 2]; 2];

/// The local edge joining corner `a` to the next corner in winding order, and to the previous one.
const fn corner_edges(lone: usize) -> [LocalEdge; 2] {
    match lone {
        0 => [LocalEdge::Edge01, LocalEdge::Edge20],
        1 => [LocalEdge::Edge12, LocalEdge::Edge01],
        _ => [LocalEdge::Edge20, LocalEdge::Edge12],
    }
}

/// The directed segment of one crossed face, and the source edges of its two endpoints.
///
/// With the face's lone corner `k` above the plane, the segment runs from the crossing on edge
/// `(k, k+1)` to the one on edge `(k, k-1)`; below, the other way. For a face wound
/// counterclockwise seen from outside, this keeps the inside on the segment's left.
fn directed(event: &FaceSection, reflect: bool) -> Option<(Segment, [[u32; 2]; 2])> {
    let TrianglePlaneEvent::Segment { endpoints, .. } = event.intersection.event else {
        return None;
    };
    let sides = event.intersection.sides;
    if sides.contains(&VertexPlaneSide::On) {
        return None;
    }
    let lone =
        (0..3).find(|&k| sides[k] != sides[(k + 1) % 3] && sides[k] != sides[(k + 2) % 3])?;
    let point = |edge: LocalEdge| {
        endpoints
            .iter()
            .find(|e| e.origin == EndpointOrigin::Edge(edge))
            .map(|e| [e.position.x, e.position.y])
    };
    let key = |edge: LocalEdge| {
        let [a, b] = event.source_edge(edge);
        [a.min(b), a.max(b)]
    };
    let [next, previous] = corner_edges(lone);
    let (from, to) = (point(next)?, point(previous)?);
    let forward = (sides[lone] == VertexPlaneSide::Above) != reflect;
    Some(if forward {
        ([from, to], [key(next), key(previous)])
    } else {
        ([to, from], [key(previous), key(next)])
    })
}

/// One segment endpoint for the balance check: position, crossed edge, +1 leaving, -1 arriving.
struct Endpoint {
    position: [f64; 2],
    edge: [u32; 2],
    sign: i8,
}

/// Directed segments of a section on a vertex-avoiding plane, after the exact balance check.
///
/// `max_bytes` bounds the segment buffer plus the balance scratch.
///
/// # Errors
/// [`LayerFailure::Unbalanced`] for an open or inconsistently oriented section,
/// [`LayerFailure::InvalidProjection`] for an event touching the plane at a vertex (excluded by
/// vertex-avoiding planes), and resource, allocation and cancellation failures.
pub(crate) fn segments(
    section: &MeshSection,
    reflect: bool,
    max_bytes: usize,
    closure: Option<Closure<'_>>,
    cancel: &mut Cancellation<'_>,
) -> Result<(Vec<Segment>, ClosureReport), LayerFailure> {
    let events = section.events();
    let per_segment = size_of::<[[f64; 2]; 2]>() + 2 * size_of::<Endpoint>();
    let bytes = events
        .len()
        .checked_mul(per_segment)
        .ok_or(LayerFailure::SizeOverflow)?;
    if bytes > max_bytes {
        return Err(LayerFailure::OutputLimit { limit: max_bytes });
    }
    let mut segments = Vec::new();
    segments
        .try_reserve_exact(events.len())
        .map_err(|_| LayerFailure::Allocation)?;
    let mut ends = Vec::new();
    ends.try_reserve_exact(2 * events.len())
        .map_err(|_| LayerFailure::Allocation)?;
    for (i, event) in events.iter().enumerate() {
        cancel.poll(Checkpoint::LayerOutput, i)?;
        let (segment, edges) = directed(event, reflect).ok_or(LayerFailure::InvalidProjection)?;
        segments.push(segment);
        for (k, sign) in [(0, 1), (1, -1)] {
            ends.push(Endpoint {
                position: segment[k],
                edge: edges[k],
                sign,
            });
        }
    }
    cancel.check(Checkpoint::LayerStorage)?;
    ends.sort_unstable_by(|a, b| {
        a.position[0]
            .total_cmp(&b.position[0])
            .then(a.position[1].total_cmp(&b.position[1]))
            .then(a.edge.cmp(&b.edge))
    });
    cancel.check(Checkpoint::LayerStorage)?;
    let mut open = Vec::new();
    let mut start = 0;
    while start < ends.len() {
        cancel.poll(Checkpoint::LayerOutput, start)?;
        let position = ends[start].position;
        let mut end = start;
        let mut excess = 0i64;
        // The sort's total order: -0 and +0 differ, but section points never carry -0.
        let same = |p: [f64; 2]| {
            p[0].total_cmp(&position[0]).is_eq() && p[1].total_cmp(&position[1]).is_eq()
        };
        while end < ends.len() && same(ends[end].position) {
            excess += i64::from(ends[end].sign);
            end += 1;
        }
        if excess != 0 {
            open.try_reserve(1).map_err(|_| LayerFailure::Allocation)?;
            open.push(UnbalancedCrossing {
                position_mm: position,
                edge: ends[start].edge,
                excess,
            });
        }
        start = end;
    }
    drop(ends);
    let report = match closure {
        None if open.is_empty() => ClosureReport::default(),
        None => return Err(unbalanced(open.len(), &open)),
        Some(closure) => close(&mut segments, &open, closure, cancel)?,
    };
    Ok((segments, report))
}

fn unbalanced(crossings: usize, details: &[UnbalancedCrossing]) -> LayerFailure {
    LayerFailure::Unbalanced {
        crossings,
        details: details
            .iter()
            .take(MAX_UNBALANCED_DETAILS)
            .copied()
            .collect(),
    }
}

/// Close the open section chains at holes by chords paired along each hole's rim
/// (specification section 5.3), appending the chords to `segments`.
///
/// A crossing point on a boundary edge where one chain arrives (excess -1) is an *end*, one where a
/// chain leaves (+1) a *start*. Along a rim they alternate: an upward boundary half-edge yields an
/// end and a downward one a start. Each end is joined to a start of the same rim by a straight
/// chord; on a loop with several pairs, the cyclic matching with the smaller total length wins.
fn close(
    segments: &mut Vec<Segment>,
    open: &[UnbalancedCrossing],
    closure: Closure<'_>,
    cancel: &mut Cancellation<'_>,
) -> Result<ClosureReport, LayerFailure> {
    let mut report = ClosureReport::default();
    if open.is_empty() {
        return Ok(report);
    }
    let mut crossings = Vec::new();
    crossings
        .try_reserve_exact(open.len())
        .map_err(|_| LayerFailure::Allocation)?;
    let mut stray = Vec::new();
    for crossing in open {
        let rim = (crossing.excess.abs() == 1)
            .then(|| closure.rims.lookup(crossing.edge))
            .flatten();
        if let Some((rim, position)) = rim {
            crossings.push((rim, position, *crossing));
        } else {
            stray.try_reserve(1).map_err(|_| LayerFailure::Allocation)?;
            stray.push(*crossing);
        }
    }
    // An imbalance off the rims (duplicate faces, fins) is not a hole.
    if !stray.is_empty() {
        return Err(unbalanced(open.len(), &stray));
    }
    crossings.sort_unstable_by_key(|c| (c.0, c.1));
    cancel.check(Checkpoint::LayerOutput)?;
    let mut i = 0;
    while i < crossings.len() {
        cancel.poll(Checkpoint::LayerOutput, i)?;
        let mut j = i;
        while j < crossings.len() && crossings[j].0 == crossings[i].0 {
            j += 1;
        }
        let group: Vec<UnbalancedCrossing> = crossings[i..j].iter().map(|c| c.2).collect();
        let is_end = |c: &UnbalancedCrossing| c.excess < 0;
        let m = group.len();
        let alternating = group.windows(2).all(|w| is_end(&w[0]) != is_end(&w[1]));
        let pairs: Vec<(usize, usize)> = if closure.rims.is_loop(crossings[i].0) {
            if !alternating || !m.is_multiple_of(2) || is_end(&group[0]) == is_end(&group[m - 1]) {
                return Err(unbalanced(open.len(), &group));
            }
            let ends = (0..m).filter(|&k| is_end(&group[k]));
            let next: Vec<_> = ends.clone().map(|k| (k, (k + 1) % m)).collect();
            if m == 2 {
                next
            } else {
                let previous: Vec<_> = ends.map(|k| (k, (k + m - 1) % m)).collect();
                let total = |pairs: &[(usize, usize)]| {
                    pairs
                        .iter()
                        .map(|&(a, b)| chord(group[a].position_mm, group[b].position_mm))
                        .sum::<f64>()
                };
                report.ambiguous_rims += 1;
                if total(&previous) < total(&next) {
                    previous
                } else {
                    next
                }
            }
        } else {
            if !alternating || !is_end(&group[0]) || is_end(&group[m - 1]) {
                return Err(unbalanced(open.len(), &group));
            }
            (0..m).step_by(2).map(|k| (k, k + 1)).collect()
        };
        for (a, b) in pairs {
            let (from, to) = (group[a].position_mm, group[b].position_mm);
            let length = chord(from, to);
            if length > closure.max_gap_mm {
                return Err(LayerFailure::OpenSectionGap {
                    length_mm: length,
                    from_mm: from,
                    to_mm: to,
                    max_gap_mm: closure.max_gap_mm,
                });
            }
            segments
                .try_reserve(1)
                .map_err(|_| LayerFailure::Allocation)?;
            segments.push([from, to]);
            report.chords += 1;
            report.longest_mm = report.longest_mm.max(length);
        }
        i = j;
    }
    Ok(report)
}

fn chord(a: [f64; 2], b: [f64; 2]) -> f64 {
    (b[0] - a[0]).hypot(b[1] - a[1])
}

/// Assemble a winding layer from a section at its vertex-avoiding plane. The segments of faces in
/// `kept` (sorted, disjoint face ranges) are also listed apart (TD-123).
pub(crate) fn layer(
    plane: LayerPlane,
    section: &MeshSection,
    transform: SectionTransform<'_>,
    max_bytes: usize,
    closure: Option<Closure<'_>>,
    kept: &[Range<usize>],
    cancel: &mut Cancellation<'_>,
) -> Result<WindingLayer, LayerFailure> {
    let reflect = reflects(transform)?;
    let (segments, closure) = segments(section, reflect, max_bytes, closure, cancel)?;
    // Before closure chords, the segments follow the section's events one for one.
    let mut kept_segments = Vec::new();
    let mut model_segments = Vec::new();
    if !kept.is_empty() {
        let is_kept = |face: usize| {
            let at = kept.partition_point(|r| r.end <= face);
            kept.get(at).is_some_and(|r| r.contains(&face))
        };
        let count = section
            .events()
            .iter()
            .filter(|e| is_kept(e.source_face))
            .count();
        kept_segments
            .try_reserve_exact(count)
            .map_err(|_| LayerFailure::Allocation)?;
        model_segments
            .try_reserve_exact(segments.len() - count)
            .map_err(|_| LayerFailure::Allocation)?;
        let events = section.events();
        for (i, segment) in segments.iter().enumerate() {
            cancel.poll(Checkpoint::LayerOutput, i)?;
            // Past the events come the closure chords, which close the model's sections.
            if events.get(i).is_some_and(|e| is_kept(e.source_face)) {
                kept_segments.push(*segment);
            } else {
                model_segments.push(*segment);
            }
        }
    }
    Ok(WindingLayer {
        plane,
        sampled_z_mm: section.plane_z(),
        segments,
        kept: kept_segments,
        model: model_segments,
        closure,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        geometry::{Affine3, ObjectTransform, Rotation3, Vector3},
        mesh::TriangleMesh,
        slicing::{section_mesh_z, SectionLimits},
    };

    fn plane() -> LayerPlane {
        LayerPlane {
            index: 0,
            z_mm: 0.5,
            nominal_height_mm: 1.0,
        }
    }

    /// An axis-aligned box with outward faces: its two quads per side, counterclockwise outside.
    fn cube(min: [f64; 3], max: [f64; 3]) -> (Vec<Point3>, Vec<[u32; 3]>) {
        let points = (0..8)
            .map(|i| {
                Point3::new(
                    if i & 1 == 0 { min[0] } else { max[0] },
                    if i & 2 == 0 { min[1] } else { max[1] },
                    if i & 4 == 0 { min[2] } else { max[2] },
                )
            })
            .collect();
        let quads = [
            [0, 2, 3, 1],
            [4, 5, 7, 6],
            [0, 1, 5, 4],
            [2, 6, 7, 3],
            [0, 4, 6, 2],
            [1, 3, 7, 5],
        ];
        let triangles = quads
            .iter()
            .flat_map(|q| [[q[0], q[1], q[2]], [q[0], q[2], q[3]]])
            .collect();
        (points, triangles)
    }

    fn section(mesh: &TriangleMesh, z: f64) -> MeshSection {
        section_mesh_z(
            mesh,
            z,
            SectionTransform::Identity,
            SectionLimits::default(),
        )
        .unwrap()
    }

    /// Twice the signed area of the closed directed segments (positive counterclockwise).
    fn doubled_area(segments: &[[[f64; 2]; 2]]) -> f64 {
        segments
            .iter()
            .map(|[a, b]| a[0] * b[1] - b[0] * a[1])
            .sum()
    }

    #[test]
    fn outward_faces_give_counterclockwise_balanced_sections() {
        let (points, triangles) = cube([0., 0., 0.], [2., 3., 1.]);
        let mesh = TriangleMesh::try_new(points, triangles).unwrap();
        let layer = layer(
            plane(),
            &section(&mesh, 0.5),
            SectionTransform::Identity,
            1 << 20,
            None,
            &[],
            &mut Cancellation::never(),
        )
        .unwrap();
        assert_eq!(layer.segments().len(), 8);
        assert_eq!(doubled_area(layer.segments()), 12.0);
        assert_eq!(layer.sampled_z_mm(), 0.5);
    }

    #[test]
    fn the_segments_of_kept_faces_are_listed_apart() {
        // TD-123: a 2 x 3 box and, after its twelve faces, a 1 x 1 box whose faces are kept.
        let (mut points, mut triangles) = cube([0., 0., 0.], [2., 3., 1.]);
        let (more, faces) = cube([5., 0., 0.], [6., 1., 1.]);
        let base = u32::try_from(points.len()).unwrap();
        points.extend(more);
        triangles.extend(faces.iter().map(|t| t.map(|v| v + base)));
        let mesh = TriangleMesh::try_new(points, triangles).unwrap();
        let section = section(&mesh, 0.5);
        let layer = |kept: &[Range<usize>]| {
            super::layer(
                plane(),
                &section,
                SectionTransform::Identity,
                1 << 20,
                None,
                kept,
                &mut Cancellation::never(),
            )
            .unwrap()
        };
        let kept = std::iter::once(12..24).collect::<Vec<_>>();
        let all = layer(&kept);
        assert_eq!(all.segments().len(), 16);
        assert_eq!(all.kept_segments().len(), 8);
        assert_eq!(doubled_area(all.kept_segments()), 2.0);
        // TD-134: the rest is the model alone, the 2 x 3 box.
        assert_eq!(all.model_segments().len(), 8);
        assert_eq!(doubled_area(all.model_segments()), 12.0);
        assert_eq!(layer(&[]).model_segments(), &[] as &[[[f64; 2]; 2]]);
        assert_eq!(layer(&[]).kept_segments(), &[] as &[[[f64; 2]; 2]]);
    }

    #[test]
    fn reflecting_transforms_keep_material_on_the_left_and_singular_ones_fail() {
        let (points, triangles) = cube([0., 0., 0.], [2., 3., 1.]);
        let mesh = TriangleMesh::try_new(points, triangles).unwrap();
        let mirror = ObjectTransform::new(
            Vector3::new(0., 0., 0.),
            Rotation3::identity(),
            Vector3::new(-1., 1., 1.),
            Point3::new(0., 0., 0.),
        )
        .unwrap();
        let transform = SectionTransform::Object(&mirror);
        let section = section_mesh_z(&mesh, 0.5, transform, SectionLimits::default()).unwrap();
        let layer = layer(
            plane(),
            &section,
            transform,
            1 << 20,
            None,
            &[],
            &mut Cancellation::never(),
        )
        .unwrap();
        assert_eq!(doubled_area(layer.segments()), 12.0);
        let affine = Affine3::from_scale(Vector3::new(1., -2., 1.)).unwrap();
        assert!(reflects(SectionTransform::Affine(&affine)).unwrap());
        let flat = Affine3::from_scale(Vector3::new(1., 1., 0.)).unwrap();
        assert_eq!(
            reflects(SectionTransform::Affine(&flat)),
            Err(LayerFailure::SingularTransform)
        );
    }

    #[test]
    fn open_and_flipped_sections_fail_with_their_crossings() {
        let (points, mut triangles) = cube([0., 0., 0.], [2., 3., 1.]);
        // Drop one side triangle: an open boundary.
        let mut open = triangles.clone();
        open.remove(4);
        let mesh = TriangleMesh::try_new(points.clone(), open).unwrap();
        let Err(LayerFailure::Unbalanced { crossings, details }) = segments(
            &section(&mesh, 0.5),
            false,
            1 << 20,
            None,
            &mut Cancellation::never(),
        ) else {
            panic!("open section accepted");
        };
        assert_eq!(crossings, 2);
        assert!(details.iter().all(UnbalancedCrossing::open_boundary));
        // Flip one side triangle: an orientation conflict on its crossed edges.
        triangles[4].swap(1, 2);
        let mesh = TriangleMesh::try_new(points, triangles).unwrap();
        let Err(LayerFailure::Unbalanced { details, .. }) = segments(
            &section(&mesh, 0.5),
            false,
            1 << 20,
            None,
            &mut Cancellation::never(),
        ) else {
            panic!("flipped section accepted");
        };
        assert!(details
            .iter()
            .all(|d| !d.open_boundary() && d.excess.abs() == 2));
    }

    #[test]
    fn overlapping_bodies_balance_and_vertex_planes_are_refused() {
        let (mut points, mut triangles) = cube([0., 0., 0.], [2., 2., 1.]);
        let (more, faces) = cube([1., 1., 0.], [3., 3., 1.]);
        triangles.extend(faces.iter().map(|f| f.map(|v| v + 8)));
        points.extend(more);
        let mesh = TriangleMesh::try_new(points, triangles).unwrap();
        let (segments, _) = segments(
            &section(&mesh, 0.5),
            false,
            1 << 20,
            None,
            &mut Cancellation::never(),
        )
        .unwrap();
        assert_eq!(segments.len(), 16);
        // A plane through vertex heights is never produced by vertex-avoiding planes.
        assert_eq!(
            super::segments(
                &section(&mesh, 1.0),
                false,
                1 << 20,
                None,
                &mut Cancellation::never()
            ),
            Err(LayerFailure::InvalidProjection)
        );
        assert!(matches!(
            super::segments(
                &section(&mesh, 0.5),
                false,
                64,
                None,
                &mut Cancellation::never()
            ),
            Err(LayerFailure::OutputLimit { limit: 64 })
        ));
    }

    #[test]
    fn rules_select_material_from_winding_numbers() {
        let filled = |rule: WindingRule| (-2..=2).map(|w| rule.fills(w)).collect::<Vec<_>>();
        assert_eq!(
            filled(WindingRule::Positive),
            [false, false, false, true, true]
        );
        assert_eq!(
            filled(WindingRule::NonZero),
            [true, true, false, true, true]
        );
        assert_eq!(
            filled(WindingRule::EvenOdd),
            [false, true, false, true, false]
        );
    }
}
