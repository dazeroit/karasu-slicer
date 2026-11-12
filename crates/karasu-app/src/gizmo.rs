//! Picking and manipulation geometry for the viewport (M6.2, TD-073): ray tests against placed
//! meshes, and the drag math of the move, rotate and scale gizmos.
//!
//! Everything here is plain `f64` geometry in the world frame (millimetres, Z up), independent of
//! the GPU, so it is tested headless.
use crate::camera::Ray;
use karasu_core::mesh::TriangleMesh;

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// The ray parameter where it enters the box `min`..`max`, or `None` when it misses.
#[must_use]
pub fn ray_box(ray: &Ray, min: [f64; 3], max: [f64; 3]) -> Option<f64> {
    let (mut near, mut far) = (0.0f64, f64::INFINITY);
    for axis in 0..3 {
        let (o, d) = (ray.origin[axis], ray.direction[axis]);
        if d == 0.0 {
            if o < min[axis] || o > max[axis] {
                return None;
            }
            continue;
        }
        let (a, b) = ((min[axis] - o) / d, (max[axis] - o) / d);
        near = near.max(a.min(b));
        far = far.min(a.max(b));
    }
    (near <= far).then_some(near)
}

/// The ray parameter of its hit with the triangle `a`, `b`, `c` (either side), or `None`
/// (Möller–Trumbore).
#[must_use]
#[allow(clippy::many_single_char_names)] // The algorithm's usual names.
pub fn ray_triangle(
    origin: [f64; 3],
    direction: [f64; 3],
    [a, b, c]: [[f64; 3]; 3],
) -> Option<f64> {
    let (e1, e2) = (sub(b, a), sub(c, a));
    let p = cross(direction, e2);
    let det = dot(e1, p);
    if det == 0.0 {
        return None;
    }
    let s = sub(origin, a);
    let u = dot(s, p) / det;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = cross(s, e1);
    let v = dot(direction, q) / det;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let t = dot(e2, q) / det;
    (t >= 0.0).then_some(t)
}

/// An affine map `linear * p + translation`, and its inverse when it exists.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Affine {
    /// Rows of the linear part.
    pub linear: [[f64; 3]; 3],
    /// Translation.
    pub translation: [f64; 3],
}

impl Affine {
    /// The inverse map, or `None` for a singular one.
    #[must_use]
    pub fn inverse(&self) -> Option<Self> {
        let m = &self.linear;
        let cofactor = |r: usize, c: usize| {
            let (r1, r2) = ((r + 1) % 3, (r + 2) % 3);
            let (c1, c2) = ((c + 1) % 3, (c + 2) % 3);
            m[r1][c1] * m[r2][c2] - m[r1][c2] * m[r2][c1]
        };
        let det: f64 = (0..3).map(|c| m[0][c] * cofactor(0, c)).sum();
        if det == 0.0 || !det.is_finite() {
            return None;
        }
        // The inverse is the transposed cofactor matrix over the determinant.
        let linear: [[f64; 3]; 3] =
            std::array::from_fn(|r| std::array::from_fn(|c| cofactor(c, r) / det));
        let t = self.translation;
        let translation =
            std::array::from_fn(|r| -(0..3).map(|c| linear[r][c] * t[c]).sum::<f64>());
        Some(Self {
            linear,
            translation,
        })
    }

    /// The image of a point.
    #[must_use]
    pub fn point(&self, p: [f64; 3]) -> [f64; 3] {
        std::array::from_fn(|r| dot(self.linear[r], p) + self.translation[r])
    }

    /// The image of a direction.
    #[must_use]
    pub fn vector(&self, v: [f64; 3]) -> [f64; 3] {
        std::array::from_fn(|r| dot(self.linear[r], v))
    }
}

/// The nearest hit of `ray` with a mesh placed by `placement`, as a world ray parameter.
///
/// The ray is mapped into the mesh's source frame, where the parameter is the same, so the
/// vertices are never transformed.
#[must_use]
pub fn ray_mesh(ray: &Ray, placement: &Affine, mesh: &TriangleMesh) -> Option<(f64, usize)> {
    ray_mesh_where(ray, placement, mesh, |_| true)
}

/// [`ray_mesh`], counting only the hits whose ray parameter `keep` accepts: the parameter is the
/// same along the ray in world and model coordinates.
#[must_use]
pub fn ray_mesh_where(
    ray: &Ray,
    placement: &Affine,
    mesh: &TriangleMesh,
    keep: impl Fn(f64) -> bool,
) -> Option<(f64, usize)> {
    let inverse = placement.inverse()?;
    let origin = inverse.point(ray.origin);
    let direction = inverse.vector(ray.direction);
    let vertices = mesh.vertices();
    mesh.triangles()
        .iter()
        .enumerate()
        .filter_map(|(index, t)| {
            let corner = |k: usize| vertices.get(t[k] as usize).map(|p| [p.x, p.y, p.z]);
            ray_triangle(origin, direction, [corner(0)?, corner(1)?, corner(2)?])
                .filter(|&t| keep(t))
                .map(|t| (t, index))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
}

/// The outward unit normal, in world coordinates, of triangle `index` of `mesh` placed by
/// `placement`; `None` for a degenerate triangle. A reflecting placement reverses the winding,
/// so the normal is turned back outward, as slicing treats such a placement.
#[must_use]
pub fn world_normal(mesh: &TriangleMesh, index: usize, placement: &Affine) -> Option<[f64; 3]> {
    let triangle = mesh.triangles().get(index)?;
    let corner = |k: usize| {
        mesh.vertices()
            .get(triangle[k] as usize)
            .map(|p| placement.point([p.x, p.y, p.z]))
    };
    let [first, second, third] = [corner(0)?, corner(1)?, corner(2)?];
    let normal = cross(sub(second, first), sub(third, first));
    let rows = &placement.linear;
    let det = rows[0][0] * (rows[1][1] * rows[2][2] - rows[1][2] * rows[2][1])
        - rows[0][1] * (rows[1][0] * rows[2][2] - rows[1][2] * rows[2][0])
        + rows[0][2] * (rows[1][0] * rows[2][1] - rows[1][1] * rows[2][0]);
    let length = dot(normal, normal).sqrt();
    (length > 0.0 && det != 0.0).then(|| normal.map(|v| v / length * det.signum()))
}

/// The parameter along the line `origin + s * axis` (unit `axis`) of its point nearest `ray`, or
/// `None` when the ray is parallel to the line.
#[must_use]
#[allow(clippy::many_single_char_names)] // The closest-point formula's usual names.
pub fn along_axis(ray: &Ray, origin: [f64; 3], axis: [f64; 3]) -> Option<f64> {
    // Closest points of two lines: solve for the axis parameter.
    let w = sub(origin, ray.origin);
    let (a, b, c) = (
        dot(axis, axis),
        dot(axis, ray.direction),
        dot(ray.direction, ray.direction),
    );
    let (d, e) = (dot(axis, w), dot(ray.direction, w));
    let denominator = a * c - b * b;
    (denominator.abs() > 1e-12 * a * c).then(|| (b * e - c * d) / denominator)
}

/// Where `ray` meets the plane through `point` with `normal`, or `None` when parallel or behind.
#[must_use]
pub fn on_plane(ray: &Ray, point: [f64; 3], normal: [f64; 3]) -> Option<[f64; 3]> {
    let along = dot(ray.direction, normal);
    if along.abs() < 1e-12 {
        return None;
    }
    let t = dot(sub(point, ray.origin), normal) / along;
    (t >= 0.0).then(|| std::array::from_fn(|k| ray.origin[k] + ray.direction[k] * t))
}

/// The signed angle in degrees from `from` to `to` about `normal` (both measured from `centre`).
#[must_use]
pub fn angle_about(centre: [f64; 3], normal: [f64; 3], from: [f64; 3], to: [f64; 3]) -> f64 {
    let (a, b) = (sub(from, centre), sub(to, centre));
    dot(cross(a, b), normal).atan2(dot(a, b)).to_degrees()
}

/// A rotation matrix (rows) for fixed-axis X, then Y, then Z angles in degrees: `Rz * Ry * Rx`,
/// as placements apply them.
#[must_use]
pub fn euler_matrix([x, y, z]: [f64; 3]) -> [[f64; 3]; 3] {
    let (sx, cx) = x.to_radians().sin_cos();
    let (sy, cy) = y.to_radians().sin_cos();
    let (sz, cz) = z.to_radians().sin_cos();
    [
        [cz * cy, cz * sy * sx - sz * cx, cz * sy * cx + sz * sx],
        [sz * cy, sz * sy * sx + cz * cx, sz * sy * cx - cz * sx],
        [-sy, cy * sx, cy * cx],
    ]
}

/// The fixed-axis X, Y, Z angles in degrees of a rotation matrix (rows), with Y in -90..=90.
#[must_use]
pub fn matrix_euler(m: [[f64; 3]; 3]) -> [f64; 3] {
    let y = (-m[2][0]).clamp(-1.0, 1.0).asin();
    if y.cos() > 1e-9 {
        [
            m[2][1].atan2(m[2][2]).to_degrees(),
            y.to_degrees(),
            m[1][0].atan2(m[0][0]).to_degrees(),
        ]
    } else {
        // Gimbal lock: X and Z turn about the same axis; put it all in X.
        [(-m[1][2]).atan2(m[1][1]).to_degrees(), y.to_degrees(), 0.0]
    }
}

/// The Euler angles after turning by `degrees` about the world `axis` (0, 1, 2 for X, Y, Z).
///
/// A turn about Z is added to the Z angle exactly, since placements apply Z last; turns about X
/// or Y are composed and decomposed again.
#[must_use]
pub fn turned(angles: [f64; 3], axis: usize, degrees: f64) -> [f64; 3] {
    if axis == 2 {
        return [angles[0], angles[1], angles[2] + degrees];
    }
    let mut turn = [0.0; 3];
    turn[axis] = degrees;
    let (a, b) = (euler_matrix(turn), euler_matrix(angles));
    let product: [[f64; 3]; 3] =
        std::array::from_fn(|r| std::array::from_fn(|c| (0..3).map(|k| a[r][k] * b[k][c]).sum()));
    matrix_euler(product)
}

/// The Euler angles after turning by `degrees` about the object's own `axis` (its rotation's
/// column 0, 1 or 2): the turn applies before the object's rotation.
#[must_use]
pub fn turned_local(angles: [f64; 3], axis: usize, degrees: f64) -> [f64; 3] {
    let mut turn = [0.0; 3];
    turn[axis] = degrees;
    let (a, b) = (euler_matrix(angles), euler_matrix(turn));
    let product: [[f64; 3]; 3] =
        std::array::from_fn(|r| std::array::from_fn(|c| (0..3).map(|k| a[r][k] * b[k][c]).sum()));
    matrix_euler(product)
}

/// The distance in pixels from `point` to the segment `a`..`b`.
#[must_use]
pub fn segment_distance(point: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let length = dx * dx + dy * dy;
    let t = if length == 0.0 {
        0.0
    } else {
        (((point[0] - a[0]) * dx + (point[1] - a[1]) * dy) / length).clamp(0.0, 1.0)
    };
    (point[0] - (a[0] + t * dx)).hypot(point[1] - (a[1] + t * dy))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ray(origin: [f64; 3], direction: [f64; 3]) -> Ray {
        let n = dot(direction, direction).sqrt();
        Ray {
            origin,
            direction: direction.map(|d| d / n),
        }
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn rays_hit_boxes_triangles_and_placed_meshes() {
        let down = ray([1.0, 1.0, 10.0], [0.0, 0.0, -1.0]);
        assert_eq!(ray_box(&down, [0.0; 3], [2.0; 3]), Some(8.0));
        assert_eq!(ray_box(&down, [3.0, 0.0, 0.0], [4.0, 2.0, 2.0]), None);
        let triangle = [[0.0, 0.0, 2.0], [4.0, 0.0, 2.0], [0.0, 4.0, 2.0]];
        assert_eq!(
            ray_triangle(down.origin, down.direction, triangle),
            Some(8.0)
        );
        let outside = ray([5.0, 5.0, 10.0], [0.0, 0.0, -1.0]);
        assert_eq!(
            ray_triangle(outside.origin, outside.direction, triangle),
            None
        );
        // The same triangle placed 10 mm along X and scaled by 2: the ray parameter is in the
        // world.
        let placement = Affine {
            linear: [[2.0, 0.0, 0.0], [0.0, 2.0, 0.0], [0.0, 0.0, 2.0]],
            translation: [10.0, 0.0, 0.0],
        };
        let world = ray([12.0, 2.0, 10.0], [0.0, 0.0, -1.0]);
        let mesh = TriangleMesh::try_new(
            triangle
                .iter()
                .map(|&[x, y, z]| karasu_core::geometry::Point3::new(x, y, z))
                .collect(),
            vec![[0, 1, 2]],
        )
        .unwrap();
        assert_eq!(ray_mesh(&world, &placement, &mesh), Some((6.0, 0)));
        assert_eq!(world_normal(&mesh, 0, &placement), Some([0.0, 0.0, 1.0]));
        // Mirroring X reverses the winding; the normal stays outward.
        let mirrored = Affine {
            linear: [[-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            translation: [0.0; 3],
        };
        assert_eq!(world_normal(&mesh, 0, &mirrored), Some([0.0, 0.0, 1.0]));
        assert_eq!(world_normal(&mesh, 1, &placement), None);
        let inverse = placement.inverse().unwrap();
        assert_eq!(
            inverse.point(placement.point([1.0, 2.0, 3.0])),
            [1.0, 2.0, 3.0]
        );
        let singular = Affine {
            linear: [[1.0, 0.0, 0.0], [0.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
            translation: [0.0; 3],
        };
        assert_eq!(singular.inverse(), None);
    }

    #[test]
    fn drags_follow_axes_planes_and_angles() {
        // A ray passing over the X axis at x = 7 meets it there.
        let over = ray([7.0, -10.0, 10.0], [0.0, 1.0, -1.0]);
        assert!(close(
            along_axis(&over, [0.0; 3], [1.0, 0.0, 0.0]).unwrap(),
            7.0
        ));
        let parallel = ray([0.0, 1.0, 0.0], [1.0, 0.0, 0.0]);
        assert_eq!(along_axis(&parallel, [0.0; 3], [1.0, 0.0, 0.0]), None);
        let hit = on_plane(
            &ray([3.0, 4.0, 5.0], [0.0, 0.0, -1.0]),
            [0.0; 3],
            [0.0, 0.0, 1.0],
        );
        assert_eq!(hit, Some([3.0, 4.0, 0.0]));
        assert!(close(
            angle_about([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
            90.0
        ));
        assert!(close(
            angle_about([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, -1.0, 0.0]),
            -90.0
        ));
        assert!(close(
            segment_distance([1.0, 1.0], [0.0, 0.0], [2.0, 0.0]),
            1.0
        ));
        assert!(close(
            segment_distance([5.0, 0.0], [0.0, 0.0], [2.0, 0.0]),
            3.0
        ));
    }

    #[test]
    fn turns_compose_with_the_placement_rotation_order() {
        for angles in [[0.0, 0.0, 0.0], [10.0, 20.0, 30.0], [-45.0, 60.0, 170.0]] {
            let back = matrix_euler(euler_matrix(angles));
            for k in 0..3 {
                assert!((back[k] - angles[k]).abs() < 1e-9, "{angles:?} {back:?}");
            }
        }
        // About Z the angle simply adds.
        assert_eq!(turned([10.0, 20.0, 30.0], 2, 15.0), [10.0, 20.0, 45.0]);
        // About X from the identity: the X angle.
        let x = turned([0.0; 3], 0, 30.0);
        assert!(
            close(x[0], 30.0) && close(x[1], 0.0) && close(x[2], 0.0),
            "{x:?}"
        );
        // Composition: the world turn applies after the existing rotation.
        let angles = [0.0, 0.0, 90.0];
        let then = turned(angles, 0, 90.0);
        let expected = {
            let (a, b) = (euler_matrix([90.0, 0.0, 0.0]), euler_matrix(angles));
            let m: [[f64; 3]; 3] = std::array::from_fn(|r| {
                std::array::from_fn(|c| (0..3).map(|k| a[r][k] * b[k][c]).sum())
            });
            m
        };
        let got = euler_matrix(then);
        for r in 0..3 {
            for c in 0..3 {
                assert!((got[r][c] - expected[r][c]).abs() < 1e-9);
            }
        }
    }
}
