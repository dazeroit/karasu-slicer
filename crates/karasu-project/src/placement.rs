//! Where an object goes: its rotation, scale and mirroring about the source bounding-box centre,
//! then its translation, explicit or computed by centring and dropping (TD-061).
//!
//! The pipeline places every object through [`place`] and then checks the placement against the
//! printer. The desktop application uses the same function to show objects where they will
//! print. [`Affine`] maps points and directions between a model's frame and the world, as the
//! support planners and the desktop application's picking do.
//!
//! **Exactness:** dropping negates the lowest placed Z, so a dropped model rests exactly on
//! Z = 0. The identity pose keeps the exact translation-only evaluation.
use crate::request::Pose;
use karasu_core::{
    cancellation::Cancellation,
    geometry::{ObjectTransform, Point3, Rotation3, TransformError, Vector3},
    mesh::TriangleMesh,
    profiles::PrinterProfile,
};

/// The centre of a printer's plate, where centring places objects.
#[must_use]
pub fn plate_centre(printer: &PrinterProfile) -> [f64; 2] {
    let [ox, oy] = printer.origin_xy_mm;
    let [width, depth, _] = printer.build_size_mm;
    [ox + width / 2., oy + depth / 2.]
}

/// A placed object.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    /// Source to world.
    pub transform: ObjectTransform,
    /// The translation used: the explicit one, with the centred XY and the dropped Z.
    pub translation_mm: [f64; 3],
    /// World bounding box of the placed model.
    pub min: Point3,
    /// World bounding box of the placed model.
    pub max: Point3,
    /// The Z translation that rests the model exactly on the plate.
    pub resting_dz: f64,
}

/// Place `mesh` under `pose`, with the explicit translation `translate` (whose X and Y centring,
/// and whose Z dropping, replace).
///
/// `pose.center` centres the XY extent on `plate_centre`; `pose.drop` rests the lowest point on
/// Z = 0, or `lift_mm` above it. Nothing is checked against a printer here.
///
/// # Errors
/// An invalid pose (non-finite values, zero scale), a mesh without vertices, or cancellation.
pub fn place(
    mesh: &TriangleMesh,
    translate: [f64; 3],
    pose: &Pose,
    plate_centre: [f64; 2],
    cancel: &mut Cancellation<'_>,
) -> Result<Placement, String> {
    let shape = Shape::new(mesh, pose.rotate_deg, pose.scale, pose.mirror)?;
    let bounds_at = |placement: &ObjectTransform, cancel: &mut Cancellation<'_>| {
        mesh.world_bounds_with_cancellation(placement, cancel)
            .map_err(|e| format!("cannot place model: {e}"))?
            .ok_or_else(|| "model contains no vertices".to_owned())
    };
    // Bounds before translation: they decide the computed translation and the resting hint.
    let free = bounds_at(&shape.at([0.; 3])?, cancel)?;
    let mut offset = translate;
    if pose.center {
        offset[0] = plate_centre[0] - f64::midpoint(free.min().x, free.max().x);
        offset[1] = plate_centre[1] - f64::midpoint(free.min().y, free.max().y);
    }
    // Negating the lowest placed Z rests the model exactly on Z = 0 (an exact zero stays +0).
    let resting_dz = if free.min().z == 0. {
        0.
    } else {
        -free.min().z
    };
    if pose.drop {
        // A lift keeps the lowest point that far above the plate (exact zero without one).
        offset[2] = match pose.lift_mm {
            Some(lift) if lift > 0. => resting_dz + lift,
            _ => resting_dz,
        };
    }
    let transform = shape.at(offset)?;
    let bounds = bounds_at(&transform, cancel)?;
    Ok(Placement {
        transform,
        translation_mm: offset,
        min: bounds.min(),
        max: bounds.max(),
        resting_dz,
    })
}

/// A model's rotation, scale and mirroring about its source bounding-box centre, to be placed at
/// any translation.
pub(crate) struct Shape {
    rotation: Rotation3,
    scale: Vector3,
    pivot: Point3,
}

impl Shape {
    pub(crate) fn new(
        mesh: &TriangleMesh,
        rotate_deg: [f64; 3],
        scale: [f64; 3],
        mirror: [bool; 3],
    ) -> Result<Self, String> {
        let invalid = |e: TransformError| format!("invalid placement: {e}");
        let pose = Pose {
            rotate_deg,
            scale,
            mirror,
            ..Pose::default()
        };
        if pose.is_rigid_identity() {
            // The identity keeps the exact translation-only evaluation.
            return Ok(Self {
                rotation: Rotation3::identity(),
                scale: Vector3::new(1., 1., 1.),
                pivot: Point3::new(0., 0., 0.),
            });
        }
        let [rx, ry, rz] = rotate_deg;
        let rotation =
            Rotation3::from_euler_xyz_degrees(Vector3::new(rx, ry, rz)).map_err(invalid)?;
        let [sx, sy, sz] = std::array::from_fn(|axis| {
            if mirror[axis] {
                -scale[axis]
            } else {
                scale[axis]
            }
        });
        Ok(Self {
            rotation,
            scale: Vector3::new(sx, sy, sz),
            pivot: centre(mesh).ok_or("model contains no vertices")?,
        })
    }

    /// The placement at translation `offset`.
    pub(crate) fn at(&self, [x, y, z]: [f64; 3]) -> Result<ObjectTransform, String> {
        ObjectTransform::new(Vector3::new(x, y, z), self.rotation, self.scale, self.pivot)
            .map_err(|e| format!("invalid placement: {e}"))
    }
}

/// The source bounding-box centre (the pivot of rotation and scale), or `None` for a mesh
/// without vertices.
#[must_use]
pub fn centre(mesh: &TriangleMesh) -> Option<Point3> {
    let (min, max) = source_bounds(mesh)?;
    Some(Point3::new(
        f64::midpoint(min.x, max.x),
        f64::midpoint(min.y, max.y),
        f64::midpoint(min.z, max.z),
    ))
}

/// The source vertices' bounding box, or `None` for a mesh without vertices.
#[must_use]
pub fn source_bounds(mesh: &TriangleMesh) -> Option<(Point3, Point3)> {
    let mut vertices = mesh.vertices().iter();
    let first = *vertices.next()?;
    Some(vertices.fold((first, first), |(min, max), p| {
        (
            Point3::new(min.x.min(p.x), min.y.min(p.y), min.z.min(p.z)),
            Point3::new(max.x.max(p.x), max.y.max(p.y), max.z.max(p.z)),
        )
    }))
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

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// The map from a placed object's model frame to world coordinates, or `None` for a placement
/// that has no affine form.
#[must_use]
pub fn affine(placement: &Placement) -> Option<Affine> {
    let affine = placement.transform.to_affine().ok()?;
    Some(Affine {
        linear: affine.linear(),
        translation: affine.translation().components(),
    })
}

/// The model frame's `[x, y]` that `to_world` carries, at Z 0, to the world's `[wx, wy]` (a foot
/// on the plate, see `SupportDocument::placed`), or `None` for a placement standing the frame's
/// plane on edge.
#[must_use]
pub fn plate_point(to_world: &Affine, [wx, wy]: [f64; 2]) -> Option<[f64; 2]> {
    let l = to_world.linear;
    let det = l[0][0] * l[1][1] - l[0][1] * l[1][0];
    if det.abs() < 1e-12 || !det.is_finite() {
        return None;
    }
    let (rx, ry) = (wx - to_world.translation[0], wy - to_world.translation[1]);
    Some([
        (l[1][1] * rx - l[0][1] * ry) / det,
        (l[0][0] * ry - l[1][0] * rx) / det,
    ])
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_foot_on_the_plate_comes_back_to_the_model_frame() {
        // Turned a quarter about Z, scaled twice in X and moved: the world point maps back to
        // the model frame's point the placement carries there.
        let to_world = Affine {
            linear: [[0.0, -1.0, 0.0], [2.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
            translation: [10.0, 20.0, 5.0],
        };
        let model = plate_point(&to_world, [7.0, 26.0]).unwrap();
        let back = to_world.point([model[0], model[1], 0.0]);
        assert!((back[0] - 7.0).abs() < 1e-12 && (back[1] - 26.0).abs() < 1e-12);
        // A frame stood on edge has no point on the plate's plane to give.
        let edge = Affine {
            linear: [[1.0, 0.0, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]],
            translation: [0.0; 3],
        };
        assert!(plate_point(&edge, [1.0, 1.0]).is_none());
    }

    #[test]
    fn euler_angles_give_the_placement_rotation_and_come_back() {
        for angles in [
            [0.0, 0.0, 0.0],
            [10.0, 20.0, 30.0],
            [-45.0, 60.0, 170.0],
            [80.0, 0.0, 0.0],
        ] {
            // The matrix turns as the placement's rotation does.
            let m = euler_matrix(angles);
            let r =
                Rotation3::from_euler_xyz_degrees(Vector3::new(angles[0], angles[1], angles[2]))
                    .unwrap()
                    .matrix();
            for (row, expected) in m.iter().zip(r) {
                for (a, b) in row.iter().zip(expected) {
                    assert!((a - b).abs() < 1e-12, "{angles:?}: {m:?} {r:?}");
                }
            }
            let back = matrix_euler(m);
            for k in 0..3 {
                assert!((back[k] - angles[k]).abs() < 1e-9, "{angles:?} {back:?}");
            }
        }
        // Gimbal lock: Y at 90 degrees puts the turn about X and Z all in X.
        let locked = matrix_euler(euler_matrix([30.0, 90.0, 20.0]));
        assert!(
            (locked[1] - 90.0).abs() < 1e-6 && locked[2] == 0.0,
            "{locked:?}"
        );
        let (a, b) = (euler_matrix(locked), euler_matrix([30.0, 90.0, 20.0]));
        for r in 0..3 {
            for c in 0..3 {
                assert!((a[r][c] - b[r][c]).abs() < 1e-6);
            }
        }
    }
}
