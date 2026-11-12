//! Where an object goes: its rotation, scale and mirroring about the source bounding-box centre,
//! then its translation, explicit or computed by centring and dropping (TD-061).
//!
//! The pipeline places every object through [`place`] and then checks the placement against the
//! printer. The desktop application uses the same function to show objects where they will
//! print.
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
