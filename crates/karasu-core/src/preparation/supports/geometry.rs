//! Adjacent station convex hulls define the represented support components.
use super::SupportError;
use crate::{
    cancellation::{Cancellation, Checkpoint},
    geometry::Point3,
    mesh::TriangleMesh,
    preparation::hollow::{convex_component, HollowLimits},
};

pub(super) fn loft(
    first: &[Point3],
    second: &[Point3],
    limits: HollowLimits,
    cancel: &mut Cancellation<'_>,
) -> Result<TriangleMesh, SupportError> {
    cancel.check(Checkpoint::SupportWork)?;
    let count = first
        .len()
        .checked_add(second.len())
        .ok_or(SupportError::Resource {
            resource: "support component vertex count",
        })?;
    if count > limits.max_input_vertices {
        return Err(SupportError::Resource {
            resource: "support component vertices",
        });
    }
    let mut points = Vec::new();
    points
        .try_reserve_exact(count)
        .map_err(|_| SupportError::Resource {
            resource: "support component point allocation",
        })?;
    for &p in first.iter().chain(second) {
        cancel.check(Checkpoint::SupportWork)?;
        points.push(p);
    }
    convex_component(points, limits, cancel).map_err(SupportError::Geometry)
}
