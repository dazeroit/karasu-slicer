//! Deterministic convex support primitives: frustums and spheres as closed, outward triangle
//! meshes (TD-062). Angles come from the platform-independent degree trigonometry, so every
//! vertex is bit-identical on every platform.
use crate::geometry::sin_cos_degrees;

pub(super) type V = [f64; 3];

pub(super) fn sub(a: V, b: V) -> V {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
pub(super) fn add(a: V, b: V) -> V {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
pub(super) fn scale(a: V, s: f64) -> V {
    [a[0] * s, a[1] * s, a[2] * s]
}
pub(super) fn dot(a: V, b: V) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
pub(super) fn cross(a: V, b: V) -> V {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
pub(super) fn length(a: V) -> f64 {
    dot(a, a).sqrt()
}
/// The unit vector of `a`, or `None` for a zero or non-finite vector.
pub(super) fn unit(a: V) -> Option<V> {
    let l = length(a);
    (l > 0.0 && l.is_finite()).then(|| scale(a, 1.0 / l))
}

/// A right-handed frame `(e, f)` perpendicular to the unit axis `u`, with `e × f = u`.
/// The reference is the world axis least aligned with `u` (ties in X, Y, Z order).
pub(super) fn frame(u: V) -> (V, V) {
    let magnitudes = u.map(f64::abs);
    let mut reference = [0.0; 3];
    let mut least = 0;
    for axis in 1..3 {
        if magnitudes[axis] < magnitudes[least] {
            least = axis;
        }
    }
    reference[least] = 1.0;
    let e = unit(cross(u, reference)).expect("a unit axis is never parallel to its least axis");
    (e, cross(u, e))
}

/// One support part: a closed convex triangle mesh plus its axis capsule (for fast tests).
#[derive(Clone, Debug)]
pub(super) struct Part {
    pub vertices: Vec<V>,
    pub triangles: Vec<[u32; 3]>,
    /// Axis segment and the largest distance of any vertex from it: a capsule containing the part.
    pub axis: [V; 2],
    pub radius: f64,
    /// The part's distance from its axis at each end, varying linearly between: a tapered
    /// capsule that also contains it, tighter for a cone (TD-123). Both are `radius` for a part
    /// of one width.
    pub taper: [f64; 2],
}

fn ring(centre: V, u: V, radius: f64, sides: u16, out: &mut Vec<V>) {
    ring_from(centre, u, radius, sides, 0.0, out);
}

/// [`ring`], its first vertex `phase` sides around (0 or a half side).
fn ring_from(centre: V, u: V, radius: f64, sides: u16, phase: f64, out: &mut Vec<V>) {
    let (e, f) = frame(u);
    for k in 0..sides {
        let (sine, cosine) = sin_cos_degrees(360.0 * (f64::from(k) + phase) / f64::from(sides));
        out.push(add(
            centre,
            add(scale(e, radius * cosine), scale(f, radius * sine)),
        ));
    }
}

/// A frustum from the circle `(c0, r0)` to `(c1, r1)` with `sides` sides and flat caps. A zero
/// radius gives a cone apex. `c0 ≠ c1` and at least one radius is positive.
pub(super) fn frustum(c0: V, r0: f64, c1: V, r1: f64, sides: u16) -> Part {
    let u = unit(sub(c1, c0)).expect("frustum ends must differ");
    let n = u32::from(sides);
    let mut vertices = Vec::new();
    let mut triangles = Vec::new();
    // Bottom ring (or apex), then top ring (or apex).
    let bottom: Vec<u32> = if r0 > 0.0 {
        ring(c0, u, r0, sides, &mut vertices);
        (0..n).collect()
    } else {
        vertices.push(c0);
        vec![0; n as usize]
    };
    let offset = u32::try_from(vertices.len()).expect("small part");
    let top: Vec<u32> = if r1 > 0.0 {
        ring(c1, u, r1, sides, &mut vertices);
        (offset..offset + n).collect()
    } else {
        vertices.push(c1);
        vec![offset; n as usize]
    };
    for k in 0..n as usize {
        let next = (k + 1) % n as usize;
        // Side faces wind counterclockwise seen from outside: tangential, then axial.
        if r0 > 0.0 {
            triangles.push([bottom[k], bottom[next], top[next]]);
        }
        if r1 > 0.0 {
            triangles.push([bottom[k], top[next], top[k]]);
        }
    }
    for k in 1..n as usize - 1 {
        if r0 > 0.0 {
            triangles.push([bottom[0], bottom[k + 1], bottom[k]]);
        }
        if r1 > 0.0 {
            triangles.push([top[0], top[k], top[k + 1]]);
        }
    }
    Part {
        vertices,
        triangles,
        axis: [c0, c1],
        radius: r0.max(r1),
        taper: [r0, r1],
    }
}

/// A latitude–longitude sphere with `sides` meridians and `max(2, sides / 2)` bands; every
/// vertex lies on the sphere. Its meridians are half a side round from a frustum's: where a
/// shaft enters a joint of its radius, their rings never share a vertex (shells sharing a ring of
/// vertices read as edges of four faces once a file's vertices are welded; crossing does not).
pub(super) fn sphere(centre: V, radius: f64, sides: u16) -> Part {
    let bands = (sides / 2).max(2);
    let n = u32::from(sides);
    let mut vertices = vec![add(centre, [0.0, 0.0, radius])];
    for j in 1..bands {
        let (s, c) = sin_cos_degrees(180.0 * f64::from(j) / f64::from(bands));
        let level = add(centre, [0.0, 0.0, radius * c]);
        ring_from(
            level,
            [0.0, 0.0, 1.0],
            radius * s,
            sides,
            0.5,
            &mut vertices,
        );
    }
    vertices.push(add(centre, [0.0, 0.0, -radius]));
    let south = u32::try_from(vertices.len() - 1).expect("small part");
    let at = |band: u32, k: u32| 1 + band * n + k % n;
    let mut triangles = Vec::new();
    for k in 0..n {
        triangles.push([0, at(0, k), at(0, k + 1)]);
    }
    for band in 0..u32::from(bands) - 2 {
        for k in 0..n {
            let (u0, u1) = (at(band, k), at(band, k + 1));
            let (l0, l1) = (at(band + 1, k), at(band + 1, k + 1));
            triangles.push([u0, l0, l1]);
            triangles.push([u0, l1, u1]);
        }
    }
    let last = u32::from(bands) - 2;
    for k in 0..n {
        triangles.push([south, at(last, k + 1), at(last, k)]);
    }
    Part {
        vertices,
        triangles,
        axis: [centre, centre],
        radius,
        taper: [radius; 2],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{geometry::Point3, mesh::TriangleMesh};

    fn mesh(part: &Part) -> TriangleMesh {
        TriangleMesh::try_new(
            part.vertices
                .iter()
                .map(|&[x, y, z]| Point3::new(x, y, z))
                .collect(),
            part.triangles.clone(),
        )
        .unwrap()
    }

    /// Signed volume by the divergence theorem; positive for an outward closed mesh.
    fn volume(part: &Part) -> f64 {
        part.triangles
            .iter()
            .map(|t| {
                let [a, b, c] = t.map(|i| part.vertices[i as usize]);
                dot(a, cross(b, c)) / 6.0
            })
            .sum()
    }

    fn assert_closed(part: &Part) {
        let report = mesh(part)
            .defect_report_with_cancellation(
                crate::mesh::topology::TopologyLimits::default(),
                &mut crate::cancellation::Cancellation::never(),
            )
            .unwrap();
        assert_eq!(report.boundary_edges, 0, "{report:?}");
        assert_eq!(report.winding_conflicts, 0, "{report:?}");
        assert_eq!(report.non_manifold_edges, 0, "{report:?}");
    }

    #[test]
    fn frustums_and_cones_are_closed_outward_and_converge_to_their_volumes() {
        for (c0, c1) in [
            ([0.0, 0.0, 0.0], [0.0, 0.0, 5.0]),
            ([1.0, 2.0, 3.0], [4.0, -1.0, 9.0]),
            ([0.0, 0.0, 5.0], [0.0, 0.0, 0.0]),
        ] {
            let h = length(sub(c1, c0));
            for (r0, r1) in [(1.0, 1.0), (0.5, 1.5), (0.0, 1.0), (1.0, 0.0)] {
                let part = frustum(c0, r0, c1, r1, 64);
                assert_closed(&part);
                let exact = std::f64::consts::PI * h / 3.0 * (r0 * r0 + r0 * r1 + r1 * r1);
                let v = volume(&part);
                assert!(v > 0.0 && (v - exact).abs() / exact < 0.005, "{v} {exact}");
            }
        }
        // A square section: four sides, area 2 r² for an inscribed square.
        let square = frustum([0.0; 3], 1.0, [0.0, 0.0, 2.0], 1.0, 4);
        assert_closed(&square);
        assert!((volume(&square) - 4.0).abs() < 1e-12);
    }

    #[test]
    fn spheres_are_closed_outward_and_converge_to_their_volume() {
        for sides in [3u16, 4, 8, 16, 64] {
            let part = sphere([1.0, 2.0, 3.0], 2.0, sides);
            assert_closed(&part);
            assert!(volume(&part) > 0.0);
        }
        let exact = 4.0 / 3.0 * std::f64::consts::PI * 8.0;
        let v = volume(&sphere([0.0; 3], 2.0, 64));
        assert!((v - exact).abs() / exact < 0.01, "{v}");
    }

    #[test]
    fn rings_are_deterministic_and_axis_frames_right_handed() {
        for u in [
            [0.0, 0.0, 1.0],
            [0.0, 0.0, -1.0],
            [1.0, 0.0, 0.0],
            unit([1.0, -2.0, 0.5]).unwrap(),
        ] {
            let (e, f) = frame(u);
            let w = cross(e, f);
            assert!(length(sub(w, u)) < 1e-15);
            assert!(dot(e, u).abs() < 1e-15 && dot(f, u).abs() < 1e-15);
        }
        let a = frustum([0.0; 3], 0.5, [0.0, 0.0, 3.0], 0.7, 16);
        let b = frustum([0.0; 3], 0.5, [0.0, 0.0, 3.0], 0.7, 16);
        assert_eq!(a.vertices, b.vertices);
        // Right-angle samples are exact: the first ring vertex lies on e, the fifth on f.
        assert_eq!(a.vertices[0], [0.0, 0.5, 0.0]);
    }
}
