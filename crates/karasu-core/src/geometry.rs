//! Point/vector values and checked rotations/object/affine transforms. Geometry containers and
//! transform operations validate finite inputs; no printer or UI policy is embedded here.

mod rotation;
pub mod section;
mod transform;
mod transform_error;

pub(crate) use rotation::sin_cos_degrees;
pub use rotation::Rotation3;
pub use transform::{Affine3, ObjectTransform};
pub use transform_error::TransformError;

/// A position in a named coordinate space, measured in millimeters for mesh vertices.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point3 {
    /// X coordinate.
    pub x: f64,
    /// Y coordinate.
    pub y: f64,
    /// Z coordinate.
    pub z: f64,
}

impl Point3 {
    /// Creates a value. Mesh construction separately rejects non-finite coordinates.
    #[must_use]
    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    /// Returns coordinates in X/Y/Z order.
    #[must_use]
    pub const fn coordinates(self) -> [f64; 3] {
        [self.x, self.y, self.z]
    }

    pub(crate) fn key(self) -> [u64; 3] {
        self.coordinates()
            .map(|v| if v == 0.0 { 0 } else { v.to_bits() })
    }
}

/// A displacement or direction, distinct from a position.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vector3 {
    /// X component.
    pub x: f64,
    /// Y component.
    pub y: f64,
    /// Z component.
    pub z: f64,
}

impl Vector3 {
    /// Creates a vector without silently normalizing it.
    #[must_use]
    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    /// Returns components in X/Y/Z order.
    #[must_use]
    pub const fn components(self) -> [f64; 3] {
        [self.x, self.y, self.z]
    }

    // Direction-only scaling keeps at least one component at magnitude one. It avoids computing
    // edge lengths before cross products and bounds the later sum of squares to [1, 3].
    pub(crate) fn scaled_direction(self) -> Option<Self> {
        let scale = self.x.abs().max(self.y.abs()).max(self.z.abs());
        if scale == 0.0 || !self.components().iter().all(|v| v.is_finite()) {
            return None;
        }
        Some(Self::new(self.x / scale, self.y / scale, self.z / scale))
    }

    pub(crate) fn unit(self) -> Option<Self> {
        let Self { x, y, z } = self.scaled_direction()?;
        let length = (x * x + y * y + z * z).sqrt();
        Some(Self::new(x / length, y / length, z / length))
    }
}
