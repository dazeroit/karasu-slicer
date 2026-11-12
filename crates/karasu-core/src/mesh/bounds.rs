use super::MeshError;
use crate::cancellation::{Cancellation, Cancelled, Checkpoint};
use crate::geometry::TransformError;
use crate::geometry::{Point3, Vector3};
use std::{error::Error, fmt};

/// Finite bounds in the coordinate space named by the producing API (`source_bounds`,
/// `world_bounds`, or `transformed_bounds`). Empty geometry uses `None`, not sentinel infinities.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aabb {
    min: Point3,
    max: Point3,
}

impl Aabb {
    pub(crate) fn from_vertices(
        vertices: &[Point3],
        cancellation: &mut Cancellation<'_>,
    ) -> Result<Option<Self>, MeshError> {
        cancellation.check(Checkpoint::SourceBounds)?;
        let Some(&first) = vertices.first() else {
            return Ok(None);
        };
        let mut min = first.coordinates();
        let mut max = min;
        for (index, point) in vertices.iter().skip(1).enumerate() {
            cancellation.poll(Checkpoint::SourceBounds, index)?;
            for (axis, value) in point.coordinates().into_iter().enumerate() {
                min[axis] = min[axis].min(value);
                max[axis] = max[axis].max(value);
            }
        }
        Self::from_extrema(
            Point3::new(min[0], min[1], min[2]),
            Point3::new(max[0], max[1], max[2]),
        )
        .map(Some)
    }

    // Callers provide finite ordered extrema from validated vertices, never public raw endpoints.
    pub(super) fn from_extrema(min: Point3, max: Point3) -> Result<Self, MeshError> {
        if min
            .coordinates()
            .into_iter()
            .zip(max.coordinates())
            .any(|(low, high)| !(high - low).is_finite())
        {
            return Err(MeshError::UnrepresentableBounds);
        }
        Ok(Self { min, max })
    }

    /// Minimum coordinates in the producing API's space, in millimeters.
    #[must_use]
    pub const fn min(self) -> Point3 {
        self.min
    }

    /// Maximum coordinates in the producing API's space, in millimeters.
    #[must_use]
    pub const fn max(self) -> Point3 {
        self.max
    }

    /// Finite nonnegative X/Y/Z extents in millimeters.
    #[must_use]
    pub fn dimensions(self) -> Vector3 {
        Vector3::new(
            self.max.x - self.min.x,
            self.max.y - self.min.y,
            self.max.z - self.min.z,
        )
    }

    /// Midpoint in the producing API's space, without adding potentially overflowing endpoints.
    #[must_use]
    pub fn center(self) -> Point3 {
        let d = self.dimensions();
        Point3::new(
            self.min.x + d.x * 0.5,
            self.min.y + d.y * 0.5,
            self.min.z + d.z * 0.5,
        )
    }
}

/// Failure to bound transformed vertices. Original mesh/metadata remain unchanged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum TransformedBoundsError {
    /// Caller-requested cancellation; no partial bounds are published.
    Cancelled(Cancelled),
    /// A point could not be transformed using finite arithmetic.
    Vertex {
        /// Original vertex-buffer index, including unused manual vertices.
        vertex: usize,
        /// Underlying transform failure.
        source: TransformError,
    },
    /// Individual transformed coordinates are finite, but a world extent overflows `f64`.
    UnrepresentableExtent,
}

impl fmt::Display for TransformedBoundsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled(cancelled) => cancelled.fmt(formatter),
            Self::Vertex { vertex, source } => {
                write!(formatter, "transformed bounds at vertex {vertex}: {source}")
            }
            Self::UnrepresentableExtent => write!(
                formatter,
                "transformed bounds exceed finite f64 extent range"
            ),
        }
    }
}

impl Error for TransformedBoundsError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Cancelled(cause) => Some(cause),
            Self::Vertex { source, .. } => Some(source),
            Self::UnrepresentableExtent => None,
        }
    }
}

impl From<Cancelled> for TransformedBoundsError {
    fn from(value: Cancelled) -> Self {
        Self::Cancelled(value)
    }
}
