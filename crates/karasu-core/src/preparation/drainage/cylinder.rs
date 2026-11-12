//! Certified represented polygonal cylinders for cardinal world-space cut axes.
// Exact comparisons establish represented coordinate identity; all integer casts are bounded by
// the hard 4096-segment cap. Keep the complete construction and its checks together for audit.
#![allow(
    clippy::float_cmp,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::too_many_lines
)]
use super::DrainageError;
use crate::{
    cancellation::{Cancellation, Checkpoint},
    geometry::Point3,
    mesh::TriangleMesh,
    preparation::{hollow::numeric::I, HoleSpec},
};

pub(crate) struct Cutter {
    pub(crate) mesh: TriangleMesh,
    pub(crate) entry: Vec<Point3>,
    pub(crate) end: Vec<Point3>,
    pub(crate) radial_error_mm: f64,
    pub(crate) axial_error_mm: f64,
}

fn reserved<T>(count: usize) -> Result<Vec<T>, DrainageError> {
    let mut result = Vec::new();
    result
        .try_reserve_exact(count)
        .map_err(|_| DrainageError::Resource {
            resource: "cylinder allocation",
        })?;
    Ok(result)
}

fn uncertain() -> DrainageError {
    DrainageError::Uncertain {
        reason: "cylinder radial or axial bounds cannot meet the requested error",
    }
}

fn radial_bounds(
    ring: &[Point3],
    center: [f64; 3],
    axes: [usize; 2],
    cancel: &mut Cancellation<'_>,
) -> Result<(f64, f64), DrainageError> {
    let delta = |point: Point3| {
        let values = point.coordinates();
        axes.map(|axis| I::point(values[axis]).sub(I::point(center[axis])))
    };
    let mut outer: f64 = 0.0;
    let mut inner = f64::INFINITY;
    let quadrant = |point: Point3| {
        let values = point.coordinates();
        match (
            values[axes[0]] >= center[axes[0]],
            values[axes[1]] >= center[axes[1]],
        ) {
            (true, true) => 0,
            (false, true) => 1,
            (false, false) => 2,
            (true, false) => 3,
        }
    };
    let first = ring[0].coordinates();
    if first[axes[0]] <= center[axes[0]] || first[axes[1]] != center[axes[1]] {
        return Err(uncertain());
    }
    let mut previous_quadrant = 0;
    for (index, &point) in ring.iter().enumerate() {
        cancel
            .check(Checkpoint::DrainageWork)
            .map_err(DrainageError::Cancelled)?;
        let a = delta(point);
        let b = delta(ring[(index + 1) % ring.len()]);
        let radius = a[0]
            .mul(a[0])
            .add(a[1].mul(a[1]))
            .sqrt()
            .ok_or_else(uncertain)?;
        let edge = [b[0].sub(a[0]), b[1].sub(a[1])];
        let length = edge[0]
            .mul(edge[0])
            .add(edge[1].mul(edge[1]))
            .sqrt()
            .ok_or_else(uncertain)?;
        let cross = a[0].mul(b[1]).sub(a[1].mul(b[0]));
        let distance = cross.div(length).ok_or_else(uncertain)?;
        if !radius.is_finite() || !distance.is_finite() || distance.lo <= 0.0 {
            return Err(uncertain());
        }
        outer = outer.max(radius.hi);
        inner = inner.min(distance.lo);
        // Exact quadrant order plus strictly positive neighboring radial determinants proves
        // one simple winding, with each edge in its disjoint angular sector. Positive local
        // turns then establish convexity. No trigonometric accuracy is assumed.
        let current_quadrant = quadrant(point);
        if current_quadrant < previous_quadrant || cross.lo <= 0.0 {
            return Err(uncertain());
        }
        previous_quadrant = current_quadrant;
        let c = delta(ring[(index + 2) % ring.len()]);
        let next = [c[0].sub(b[0]), c[1].sub(b[1])];
        let turn = edge[0].mul(next[1]).sub(edge[1].mul(next[0]));
        if !turn.is_finite() || turn.lo <= 0.0 {
            return Err(uncertain());
        }
    }
    if previous_quadrant != 3 {
        return Err(uncertain());
    }
    Ok((inner, outer))
}

pub(crate) fn cylinder(
    spec: HoleSpec,
    error: f64,
    max_segments: usize,
    cancel: &mut Cancellation<'_>,
) -> Result<Cutter, DrainageError> {
    cancel
        .check(Checkpoint::DrainageWork)
        .map_err(DrainageError::Cancelled)?;
    let nonzero = spec.direction.iter().filter(|value| **value != 0.0).count();
    if nonzero != 1 || spec.direction.iter().any(|value| !value.is_finite()) {
        return Err(DrainageError::Unsupported {
            reason: "cylinder axis must be a cardinal world direction",
        });
    }
    if !error.is_finite()
        || error <= 0.0
        || !spec.diameter_mm.is_finite()
        || spec.diameter_mm <= 0.0
        || !spec.length_mm.is_finite()
        || spec.length_mm <= 0.0
        || spec.entry_mm.iter().any(|v| !v.is_finite())
    {
        return Err(uncertain());
    }
    let axis = spec
        .direction
        .iter()
        .position(|value| *value != 0.0)
        .ok_or_else(uncertain)?;
    let sign = spec.direction[axis].signum();
    let radial_axes = [(axis + 1) % 3, (axis + 2) % 3];
    // The certificate always uses positive planar winding; mesh orientation is reversed below
    // for a negative extrusion direction.
    let radius = spec.diameter_mm * 0.5;
    let exact_radius = I::point(spec.diameter_mm).mul(I::point(0.5));
    if radius == 0.0 {
        return Err(uncertain());
    }
    let mut end_center = spec.entry_mm;
    end_center[axis] += sign * spec.length_mm;
    let exact_end = I::point(spec.entry_mm[axis]).add(I::point(sign * spec.length_mm));
    let axial = I::point(end_center[axis]).sub(exact_end);
    let axial_error = axial.lo.abs().max(axial.hi.abs());
    if !axial_error.is_finite() || axial_error > error || end_center[axis] == spec.entry_mm[axis] {
        return Err(uncertain());
    }
    let limit = max_segments.min(4096);
    if limit < 8 {
        return Err(DrainageError::Resource {
            resource: "cylinder segments",
        });
    }
    let mut count = 8usize;
    let (mut entry, radial_error) = loop {
        let mut ring = reserved(count)?;
        for index in 0..count {
            cancel
                .poll(Checkpoint::DrainageWork, index)
                .map_err(DrainageError::Cancelled)?;
            let angle = std::f64::consts::TAU * index as f64 / count as f64;
            let mut coordinates = spec.entry_mm;
            coordinates[radial_axes[0]] += radius * angle.cos();
            coordinates[radial_axes[1]] += radius * angle.sin();
            if coordinates.iter().any(|v| !v.is_finite()) {
                return Err(uncertain());
            }
            ring.push(Point3::new(coordinates[0], coordinates[1], coordinates[2]));
        }
        let (inner, outer) = radial_bounds(&ring, spec.entry_mm, radial_axes, cancel)?;
        let bound = exact_radius
            .sub(I::point(inner))
            .hi
            .max(I::point(outer).sub(exact_radius).hi)
            .max(0.0);
        if I::point(bound).add(I::point(axial_error)).hi <= error {
            break (ring, bound);
        }
        if count == limit {
            return Err(DrainageError::Resource {
                resource: "cylinder segments",
            });
        }
        count = count.saturating_mul(2).min(limit);
    };
    if sign < 0.0 {
        entry.reverse();
    }
    let mut end = reserved(count)?;
    for &point in &entry {
        let mut values = point.coordinates();
        values[axis] = end_center[axis];
        end.push(Point3::new(values[0], values[1], values[2]));
    }
    let mut vertices = reserved(count * 2)?;
    vertices.extend_from_slice(&entry);
    vertices.extend_from_slice(&end);
    let mut triangles = reserved(count * 4 - 4)?;
    let n = count as u32;
    for index in 0..count {
        let a = index as u32;
        let b = ((index + 1) % count) as u32;
        triangles.push([a, b, n + b]);
        triangles.push([a, n + b, n + a]);
    }
    for index in 1..count - 1 {
        let b = index as u32;
        triangles.push([0, b + 1, b]);
        triangles.push([n, n + b, n + b + 1]);
    }
    let mesh = TriangleMesh::try_new_with_cancellation(vertices, triangles, cancel)
        .map_err(DrainageError::Mesh)?;
    Ok(Cutter {
        mesh,
        entry,
        end,
        radial_error_mm: radial_error,
        axial_error_mm: axial_error,
    })
}

/// Interval of the dot product of two interval vectors.
fn dot(a: [I; 3], b: [I; 3]) -> I {
    a[0].mul(b[0]).add(a[1].mul(b[1])).add(a[2].mul(b[2]))
}

/// Interval of the cross product of two interval vectors.
fn cross(a: [I; 3], b: [I; 3]) -> [I; 3] {
    [
        a[1].mul(b[2]).sub(a[2].mul(b[1])),
        a[2].mul(b[0]).sub(a[0].mul(b[2])),
        a[0].mul(b[1]).sub(a[1].mul(b[0])),
    ]
}

/// Interval of a vector's length. A sum of squares is never negative, so the outward-rounded
/// lower end is clamped at zero before the square root.
fn magnitude(v: [I; 3]) -> Option<I> {
    let mut sum = dot(v, v);
    sum.lo = sum.lo.max(0.0);
    sum.sqrt()
}

fn interval(v: [f64; 3]) -> [I; 3] {
    v.map(I::point)
}

/// Certified represented polygonal cylinder along any direction (TD-066).
///
/// The ring lies in the plane through `entry_mm` spanned by a computed orthonormal basis
/// `(u, v)` across the unit axis `w`; the end ring is the entry ring moved by `length · w`. The
/// certificate bounds, with outward intervals, the represented geometry against the requested
/// cylinder (axis through `entry_mm` along the raw `direction`):
/// - **axial error:** the distance between the represented and the requested end centre;
/// - **radial error:** every vertex's distance from the requested axis, the chord sagitta
///   (`sqrt(rho_min² − (chord/2)²)`) and the lateral skew between matching vertices of the two
///   rings, perpendicular to the axis;
/// - **winding:** projected onto `(u, v)`, every step turns strictly left by less than a right
///   angle and the steps add up to less than two turns, so the ring winds exactly once.
///
/// Cardinal directions keep using [`cylinder`], whose rings are exactly planar.
#[allow(clippy::many_single_char_names)] // Axis w, basis u and v, axis index k, entry e.
pub(crate) fn oblique_cylinder(
    spec: HoleSpec,
    error: f64,
    max_segments: usize,
    cancel: &mut Cancellation<'_>,
) -> Result<Cutter, DrainageError> {
    cancel
        .check(Checkpoint::DrainageWork)
        .map_err(DrainageError::Cancelled)?;
    if !error.is_finite()
        || error <= 0.0
        || !spec.diameter_mm.is_finite()
        || spec.diameter_mm <= 0.0
        || !spec.length_mm.is_finite()
        || spec.length_mm <= 0.0
        || spec.entry_mm.iter().any(|v| !v.is_finite())
    {
        return Err(uncertain());
    }
    let w = spec
        .unit_direction()
        .map_err(|_| DrainageError::Unsupported {
            reason: "cut direction must be finite and nonzero",
        })?;
    // Basis across the axis: the world axis least aligned with w, made orthogonal to it.
    let k = (0..3)
        .min_by(|&a, &b| w[a].abs().total_cmp(&w[b].abs()).then(a.cmp(&b)))
        .unwrap_or(0);
    let mut u = w.map(|c| -w[k] * c);
    u[k] += 1.0;
    let norm = u.iter().map(|c| c * c).sum::<f64>().sqrt();
    if !(norm.is_finite() && norm > 0.0) {
        return Err(uncertain());
    }
    let u = u.map(|c| c / norm);
    let v = [
        w[1] * u[2] - w[2] * u[1],
        w[2] * u[0] - w[0] * u[2],
        w[0] * u[1] - w[1] * u[0],
    ];
    let radius = spec.diameter_mm * 0.5;
    let exact_radius = I::point(spec.diameter_mm).mul(I::point(0.5));
    let length = spec.length_mm;
    let entry_center = spec.entry_mm;
    let end_center: [f64; 3] = std::array::from_fn(|c| entry_center[c] + length * w[c]);
    if end_center == entry_center || end_center.iter().any(|c| !c.is_finite()) {
        return Err(uncertain());
    }
    // Requested axis: through the entry along the raw direction, of norm `axis_norm`.
    let direction = interval(spec.direction);
    let axis_norm = magnitude(direction).ok_or_else(uncertain)?;
    let e = interval(entry_center);
    let mut axial_sq = I::point(0.0);
    for c in 0..3 {
        let target = e[c].add(
            I::point(length)
                .mul(direction[c])
                .div(axis_norm)
                .ok_or_else(uncertain)?,
        );
        let gap = I::point(end_center[c]).sub(target);
        let worst = I::point(gap.lo.abs().max(gap.hi.abs()));
        axial_sq = axial_sq.add(worst.mul(worst));
    }
    let axial_error = axial_sq.sqrt().ok_or_else(uncertain)?.hi;
    if !axial_error.is_finite() || axial_error > error {
        return Err(uncertain());
    }
    // Distance interval of a point from the requested axis.
    let from_axis = |p: [f64; 3]| -> Option<I> {
        let rel = [0, 1, 2].map(|c| I::point(p[c]).sub(e[c]));
        magnitude(cross(rel, direction))?.div(axis_norm)
    };
    let limit = max_segments.min(4096);
    if limit < 8 {
        return Err(DrainageError::Resource {
            resource: "cylinder segments",
        });
    }
    let mut count = 8usize;
    let (entry, end, radial_error) = loop {
        let mut entry = reserved(count)?;
        let mut end = reserved(count)?;
        for index in 0..count {
            cancel
                .poll(Checkpoint::DrainageWork, index)
                .map_err(DrainageError::Cancelled)?;
            let angle = std::f64::consts::TAU * index as f64 / count as f64;
            let (sin, cos) = angle.sin_cos();
            let p: [f64; 3] =
                std::array::from_fn(|c| entry_center[c] + radius * (cos * u[c] + sin * v[c]));
            let q: [f64; 3] = std::array::from_fn(|c| p[c] + length * w[c]);
            if p.iter().chain(&q).any(|c| !c.is_finite()) {
                return Err(uncertain());
            }
            entry.push(p);
            end.push(q);
        }
        match oblique_bounds(
            &entry,
            &end,
            entry_center,
            [u, v],
            &from_axis,
            direction,
            axis_norm,
        ) {
            Some((inner, outer)) => {
                let bound = exact_radius
                    .sub(I::point(inner))
                    .hi
                    .max(I::point(outer).sub(exact_radius).hi)
                    .max(0.0);
                if I::point(bound).add(I::point(axial_error)).hi <= error {
                    break (entry, end, bound);
                }
            }
            None if count == limit => return Err(uncertain()),
            None => {}
        }
        if count == limit {
            return Err(DrainageError::Resource {
                resource: "cylinder segments",
            });
        }
        count = count.saturating_mul(2).min(limit);
    };
    let to_points = |ring: &[[f64; 3]]| -> Vec<Point3> {
        ring.iter().map(|p| Point3::new(p[0], p[1], p[2])).collect()
    };
    let (entry, end) = (to_points(&entry), to_points(&end));
    let mut vertices = reserved(count * 2)?;
    vertices.extend_from_slice(&entry);
    vertices.extend_from_slice(&end);
    let mut triangles = reserved(count * 4 - 4)?;
    let n = count as u32;
    // The ring winds positively about w, the extrusion direction: the same pattern as a cardinal
    // cut in the positive direction gives an outward closed prism.
    for index in 0..count {
        let a = index as u32;
        let b = ((index + 1) % count) as u32;
        triangles.push([a, b, n + b]);
        triangles.push([a, n + b, n + a]);
    }
    for index in 1..count - 1 {
        let b = index as u32;
        triangles.push([0, b + 1, b]);
        triangles.push([n, n + b, n + b + 1]);
    }
    let mesh = TriangleMesh::try_new_with_cancellation(vertices, triangles, cancel)
        .map_err(DrainageError::Mesh)?;
    Ok(Cutter {
        mesh,
        entry,
        end,
        radial_error_mm: radial_error,
        axial_error_mm: axial_error,
    })
}

/// Certified inner and outer distance of an oblique prism's surface from the requested axis, or
/// `None` when the winding or a bound cannot be proven.
#[allow(clippy::many_single_char_names)] // Basis u, v; centre c; ring points a, b, p, q.
fn oblique_bounds(
    entry: &[[f64; 3]],
    end: &[[f64; 3]],
    center: [f64; 3],
    [u, v]: [[f64; 3]; 2],
    from_axis: &dyn Fn([f64; 3]) -> Option<I>,
    direction: [I; 3],
    axis_norm: I,
) -> Option<(f64, f64)> {
    let count = entry.len();
    let c = interval(center);
    // Projected ring, for the winding proof.
    let projected: Vec<[I; 2]> = entry
        .iter()
        .map(|p| {
            let rel = [0, 1, 2].map(|k| I::point(p[k]).sub(c[k]));
            [dot(rel, interval(u)), dot(rel, interval(v))]
        })
        .collect();
    let mut turns = I::point(0.0);
    let (mut inner, mut outer) = (f64::INFINITY, 0.0f64);
    let (mut chord, mut skew) = (0.0f64, 0.0f64);
    for i in 0..count {
        let (a, b) = (projected[i], projected[(i + 1) % count]);
        let step_cross = a[0].mul(b[1]).sub(a[1].mul(b[0]));
        let step_dot = a[0].mul(b[0]).add(a[1].mul(b[1]));
        let planar = |p: [I; 2]| magnitude([p[0], p[1], I::point(0.0)]);
        let lengths = planar(a)?.mul(planar(b)?);
        if step_cross.lo <= 0.0 || step_dot.lo <= 0.0 || lengths.lo <= 0.0 {
            return None;
        }
        // sin of the step angle; a step below a right angle is at most (pi/2) sin.
        turns = turns.add(I::point(step_cross.hi).div(I::point(lengths.lo))?);
        for (p, q) in [
            (entry[i], entry[(i + 1) % count]),
            (end[i], end[(i + 1) % count]),
        ] {
            let gap = [0, 1, 2].map(|k| I::point(q[k]).sub(I::point(p[k])));
            chord = chord.max(magnitude(gap)?.hi);
        }
        for p in [entry[i], end[i]] {
            let d = from_axis(p)?;
            inner = inner.min(d.lo);
            outer = outer.max(d.hi);
        }
        // Lateral edge: its extent across the axis.
        let rel = [0, 1, 2].map(|k| I::point(end[i][k]).sub(I::point(entry[i][k])));
        skew = skew.max(magnitude(cross(rel, direction))?.div(axis_norm)?.hi);
    }
    // Total turning below two full turns (pi/2 * sum(sin) < 4 pi): the ring winds exactly once.
    if turns.hi >= 8.0 {
        return None;
    }
    let half = I::point(chord).mul(I::point(0.5));
    let sagitta_free = I::point(inner).mul(I::point(inner)).sub(half.mul(half));
    if sagitta_free.lo <= 0.0 {
        return None;
    }
    let inner = sagitta_free.sqrt()?.sub(I::point(skew)).lo;
    (inner > 0.0 && outer.is_finite()).then_some((inner, outer))
}

/// Whether a direction is parallel to a world axis (the exact cardinal construction applies).
pub(crate) fn is_cardinal(direction: [f64; 3]) -> bool {
    direction.iter().filter(|value| **value != 0.0).count() == 1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(axis: usize, sign: f64) -> HoleSpec {
        let mut direction = [0.0; 3];
        direction[axis] = sign * 7.0;
        HoleSpec {
            entry_mm: [3.0, -2.0, 1.0],
            direction,
            length_mm: 5.0,
            diameter_mm: 2.0,
        }
    }

    #[test]
    fn all_cardinal_directions_have_positive_outward_volume() {
        for axis in 0..3 {
            for sign in [-1.0, 1.0] {
                let cut =
                    cylinder(spec(axis, sign), 0.001, 512, &mut Cancellation::never()).unwrap();
                assert!(cut.radial_error_mm <= 0.001);
                assert!(cut.axial_error_mm <= 0.001);
                assert_eq!(cut.entry.len(), cut.end.len());
                let mut volume6 = 0.0;
                for face in cut.mesh.triangles() {
                    let a = cut.mesh.vertices()[face[0] as usize].coordinates();
                    let b = cut.mesh.vertices()[face[1] as usize].coordinates();
                    let c = cut.mesh.vertices()[face[2] as usize].coordinates();
                    volume6 += a[0] * (b[1] * c[2] - b[2] * c[1])
                        - a[1] * (b[0] * c[2] - b[2] * c[0])
                        + a[2] * (b[0] * c[1] - b[1] * c[0]);
                }
                let volume = volume6 / 6.0;
                assert!(volume > 15.6 && volume < std::f64::consts::PI * 5.0);
                assert_eq!(
                    cut.end[0].coordinates()[axis] - cut.entry[0].coordinates()[axis],
                    sign * 5.0
                );
            }
        }
    }

    #[test]
    fn rejects_unresolved_axes_precision_and_resource_limits() {
        let mut request = spec(2, 1.0);
        request.direction = [1.0, 1.0, 0.0];
        assert!(matches!(
            cylinder(request, 0.01, 512, &mut Cancellation::never()),
            Err(DrainageError::Unsupported { .. })
        ));
        assert!(matches!(
            cylinder(spec(2, 1.0), 1e-12, 8, &mut Cancellation::never()),
            Err(DrainageError::Resource { .. })
        ));
        request = spec(2, 1.0);
        request.entry_mm = [1e20; 3];
        assert!(matches!(
            cylinder(request, 0.001, 512, &mut Cancellation::never()),
            Err(DrainageError::Uncertain { .. })
        ));
        assert!(cylinder(spec(2, 1.0), f64::NAN, 512, &mut Cancellation::never()).is_err());
    }

    #[test]
    fn cancellation_prevents_publication() {
        let mut predicate = |_| true;
        let mut cancel = Cancellation::new(&mut predicate);
        assert!(matches!(
            cylinder(spec(2, 1.0), 0.001, 512, &mut cancel),
            Err(DrainageError::Cancelled(_))
        ));
    }

    #[test]
    fn certificate_rejects_crossed_and_nonconvex_ring_proposals() {
        let convex = [
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(0.0, 1.0, 0.0),
            Point3::new(-1.0, 0.0, 0.0),
            Point3::new(0.0, -1.0, 0.0),
        ];
        assert!(radial_bounds(&convex, [0.0; 3], [0, 1], &mut Cancellation::never()).is_ok());
        let mut crossed = convex;
        crossed.swap(1, 2);
        assert!(radial_bounds(&crossed, [0.0; 3], [0, 1], &mut Cancellation::never()).is_err());
        let indented = [
            convex[0],
            Point3::new(0.1, 0.1, 0.0),
            convex[1],
            convex[2],
            convex[3],
        ];
        assert!(radial_bounds(&indented, [0.0; 3], [0, 1], &mut Cancellation::never()).is_err());
    }

    #[test]
    fn translated_rings_meet_combined_error_and_are_deterministic() {
        let mut request = spec(1, -1.0);
        request.entry_mm = [123.456_789, -456.12345, 789.987_654];
        request.length_mm = 5.123_456_789;
        let a = cylinder(request, 1e-6, 4096, &mut Cancellation::never()).unwrap();
        let b = cylinder(request, 1e-6, 4096, &mut Cancellation::never()).unwrap();
        assert!(
            I::point(a.radial_error_mm)
                .add(I::point(a.axial_error_mm))
                .hi
                <= 1e-6
        );
        assert_eq!(a.mesh.vertices(), b.mesh.vertices());
        assert_eq!(a.mesh.triangles(), b.mesh.triangles());
    }

    #[test]
    fn oblique_cylinders_are_closed_outward_and_within_the_error_budget() {
        for direction in [
            [1.0, 0.0, 1.0],
            [0.3, -0.4, -0.866],
            [1e-3, 1.0, 0.0],
            [-2.0, 3.0, 5.0],
        ] {
            let request = HoleSpec {
                entry_mm: [12.5, -3.25, 7.0],
                direction,
                length_mm: 6.0,
                diameter_mm: 3.0,
            };
            let cut = oblique_cylinder(request, 1e-4, 4096, &mut Cancellation::never()).unwrap();
            assert!(cut.radial_error_mm + cut.axial_error_mm <= 1e-4);
            let mut volume6 = 0.0;
            for face in cut.mesh.triangles() {
                let [a, b, c] = face.map(|v| cut.mesh.vertices()[v as usize].coordinates());
                volume6 += a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
                    + a[2] * (b[0] * c[1] - b[1] * c[0]);
            }
            let analytic = std::f64::consts::PI * 1.5 * 1.5 * 6.0;
            assert!(
                (volume6 / 6.0 - analytic).abs() < 1e-3 * analytic,
                "{direction:?}"
            );
            // The end ring is the entry ring moved along the axis.
            let w = request.unit_direction().unwrap();
            for (p, q) in cut.entry.iter().zip(&cut.end) {
                let (p, q) = (p.coordinates(), q.coordinates());
                for ((a, b), c) in p.iter().zip(q).zip(w) {
                    assert!((b - a - 6.0 * c).abs() < 1e-12);
                }
            }
            let again = oblique_cylinder(request, 1e-4, 4096, &mut Cancellation::never()).unwrap();
            assert_eq!(cut.mesh.vertices(), again.mesh.vertices());
        }
        assert!(is_cardinal([0.0, -2.0, 0.0]) && !is_cardinal([0.0, 1.0, 1.0]));
        let zero = HoleSpec {
            entry_mm: [0.0; 3],
            direction: [0.0; 3],
            length_mm: 1.0,
            diameter_mm: 1.0,
        };
        assert!(oblique_cylinder(zero, 1e-4, 512, &mut Cancellation::never()).is_err());
    }
}
