//! Explicit represented polygon profiles and checked world-space station placement.
use super::{SupportError, SupportProfile, SupportStation};
use crate::{
    cancellation::{Cancellation, Checkpoint},
    geometry::Point3,
    preparation::hollow::numeric::orient3d,
};
use std::cmp::Ordering;

fn invalid(reason: &'static str) -> SupportError {
    SupportError::Invalid { reason }
}
fn positive(value: f64) -> bool {
    value.is_finite() && value > 0.0 && value <= 1000.0
}
fn allocate<T>(count: usize) -> Result<Vec<T>, SupportError> {
    let mut result = Vec::new();
    result
        .try_reserve_exact(count)
        .map_err(|_| SupportError::Resource {
            resource: "support profile allocation",
        })?;
    Ok(result)
}

pub(super) fn ring(
    station: &SupportStation,
    max_vertices: usize,
    cancel: &mut Cancellation<'_>,
) -> Result<Vec<Point3>, SupportError> {
    cancel.check(Checkpoint::SupportWork)?;
    if station
        .center_mm
        .iter()
        .any(|v| !v.is_finite() || v.abs() > 10_000.0)
    {
        return Err(invalid("support station coordinate domain"));
    }
    let count = match &station.profile {
        SupportProfile::EllipsePolygon { sides, .. }
        | SupportProfile::RegularPolygon { sides, .. } => usize::from(*sides),
        SupportProfile::Rectangle { .. } => 4,
        SupportProfile::ConvexPolygon { vertices_mm } => vertices_mm.len(),
        SupportProfile::Point => 1,
    };
    if count > max_vertices {
        return Err(SupportError::Resource {
            resource: "support profile vertices",
        });
    }
    if !matches!(station.profile, SupportProfile::Point) && count < 3 {
        return Err(invalid("support polygon needs at least three vertices"));
    }
    let mut local = allocate(count)?;
    match &station.profile {
        SupportProfile::EllipsePolygon { radii_mm, .. } => {
            if !radii_mm.iter().all(|&v| positive(v)) {
                return Err(invalid("support ellipse radii"));
            }
            regular(&mut local, count, *radii_mm, cancel)?;
        }
        SupportProfile::RegularPolygon { radius_mm, .. } => {
            if !positive(*radius_mm) {
                return Err(invalid("support polygon radius"));
            }
            regular(&mut local, count, [*radius_mm; 2], cancel)?;
        }
        SupportProfile::Rectangle { size_mm } => {
            if !size_mm.iter().all(|&v| positive(v)) {
                return Err(invalid("support rectangle dimensions"));
            }
            let [x, y] = size_mm.map(|v| v * 0.5);
            local.extend([[-x, -y], [x, -y], [x, y], [-x, y]]);
        }
        SupportProfile::ConvexPolygon { vertices_mm } => {
            local.extend_from_slice(vertices_mm);
        }
        SupportProfile::Point => local.push([0.0, 0.0]),
    }
    if local
        .iter()
        .flatten()
        .any(|v| !v.is_finite() || v.abs() > 1000.0)
    {
        return Err(invalid("support local profile coordinate domain"));
    }
    if count != 1 {
        // Every nonincident vertex must lie strictly left of every edge. This checks both
        // winding and strict convexity, including star orderings and repeated coordinates.
        for i in 0..count {
            let j = (i + 1) % count;
            let a = Point3::new(local[i][0], local[i][1], 0.0);
            let b = Point3::new(local[j][0], local[j][1], 0.0);
            let lifted = Point3::new(a.x, a.y, 1.0);
            if orient3d(a, b, Point3::new(0.0, 0.0, 0.0), lifted) != Ordering::Greater {
                return Err(invalid("support profile origin must be strictly interior"));
            }
            for (k, p) in local.iter().enumerate() {
                cancel.check(Checkpoint::SupportWork)?;
                if k != i && k != j {
                    let p = Point3::new(p[0], p[1], 0.0);
                    if orient3d(a, b, p, lifted) != Ordering::Greater {
                        return Err(invalid(
                            "support profile must be strictly convex counterclockwise",
                        ));
                    }
                }
            }
        }
    }
    let matrix = station.rotation.matrix();
    let mut world = allocate(count)?;
    for p in local {
        cancel.check(Checkpoint::SupportWork)?;
        let xyz = std::array::from_fn::<_, 3, _>(|axis| {
            station.center_mm[axis] + matrix[axis][0] * p[0] + matrix[axis][1] * p[1]
        });
        if xyz.iter().any(|v| !v.is_finite() || v.abs() > 10_000.0) {
            return Err(invalid("transformed support profile coordinate domain"));
        }
        world.push(Point3::new(xyz[0], xyz[1], xyz[2]));
    }
    if count != 1 {
        validate_area(&world, cancel)?;
    }
    Ok(world)
}

fn validate_area(world: &[Point3], cancel: &mut Cancellation<'_>) -> Result<(), SupportError> {
    let a = world[0];
    let Some(&b) = world.iter().find(|&&p| p != a) else {
        return Err(invalid("transformed support profile has collapsed area"));
    };
    let mut noncollinear = false;
    for &p in world {
        cancel.check(Checkpoint::SupportWork)?;
        for axis in 0..3 {
            let mut lifted = a.coordinates();
            // The checked coordinate domain makes a unit increment representably distinct.
            lifted[axis] += 1.0;
            let lifted = Point3::new(lifted[0], lifted[1], lifted[2]);
            if orient3d(a, b, p, lifted) != Ordering::Equal {
                noncollinear = true;
                break;
            }
        }
        if noncollinear {
            break;
        }
    }
    if !noncollinear {
        return Err(invalid("transformed support profile has collapsed area"));
    }
    Ok(())
}

fn regular(
    vertices: &mut Vec<[f64; 2]>,
    count: usize,
    radii: [f64; 2],
    cancel: &mut Cancellation<'_>,
) -> Result<(), SupportError> {
    let sides = u16::try_from(count).map_err(|_| SupportError::Resource {
        resource: "support polygon side count",
    })?;
    for i in 0..count {
        cancel.check(Checkpoint::SupportWork)?;
        let index = u16::try_from(i).map_err(|_| SupportError::Resource {
            resource: "support polygon vertex index",
        })?;
        let angle = std::f64::consts::TAU * f64::from(index) / f64::from(sides);
        let (sin, cos) = angle.sin_cos();
        vertices.push([radii[0] * cos, radii[1] * sin]);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Rotation3;

    fn station(profile: SupportProfile) -> SupportStation {
        SupportStation {
            center_mm: [2.0, 3.0, 4.0],
            rotation: Rotation3::identity(),
            profile,
        }
    }

    #[test]
    fn rectangle_has_explicit_dimensions_and_full_station_rotation() {
        let mut request = station(SupportProfile::Rectangle {
            size_mm: [4.0, 2.0],
        });
        request.rotation = Rotation3::around_x_radians(std::f64::consts::FRAC_PI_2).unwrap();
        let points = ring(&request, 4, &mut Cancellation::never()).unwrap();
        assert_eq!(points.len(), 4);
        assert_eq!(points[0].x, 0.0);
        assert!((points[0].y - 3.0).abs() <= 4.0 * f64::EPSILON);
        assert!((points[0].z - 3.0).abs() <= 4.0 * f64::EPSILON);
    }

    #[test]
    fn custom_profiles_reject_clockwise_concavity_duplicates_and_crossing() {
        let cases = [
            vec![[0.0, 0.0], [0.0, 1.0], [1.0, 0.0]],
            vec![[0.0, 0.0], [2.0, 0.0], [0.5, 0.5], [0.0, 2.0]],
            vec![[0.0, 0.0], [1.0, 0.0], [1.0, 0.0], [0.0, 1.0]],
            vec![[0.0, 0.0], [1.0, 1.0], [0.0, 1.0], [1.0, 0.0]],
        ];
        for vertices_mm in cases {
            let request = station(SupportProfile::ConvexPolygon { vertices_mm });
            assert!(matches!(
                ring(&request, 8, &mut Cancellation::never()),
                Err(SupportError::Invalid { .. })
            ));
        }
    }

    #[test]
    fn polygon_count_is_authoritative_and_resource_limit_precedes_allocation() {
        let request = station(SupportProfile::EllipsePolygon {
            radii_mm: [2.0, 0.5],
            sides: 12,
        });
        let points = ring(&request, 12, &mut Cancellation::never()).unwrap();
        assert_eq!(points.len(), 12);
        assert_eq!(points[0], Point3::new(4.0, 3.0, 4.0));
        assert!(matches!(
            ring(&request, 11, &mut Cancellation::never()),
            Err(SupportError::Resource { .. })
        ));
    }

    #[test]
    fn custom_profile_must_strictly_contain_station_origin_without_recentering() {
        for vertices_mm in [
            vec![[1.0, 1.0], [2.0, 1.0], [2.0, 2.0], [1.0, 2.0]],
            vec![[0.0, -1.0], [1.0, -1.0], [1.0, 1.0], [0.0, 1.0]],
        ] {
            let request = station(SupportProfile::ConvexPolygon { vertices_mm });
            assert!(matches!(
                ring(&request, 4, &mut Cancellation::never()),
                Err(SupportError::Invalid { .. })
            ));
        }
        let request = station(SupportProfile::ConvexPolygon {
            vertices_mm: vec![[-1.0, -2.0], [3.0, -2.0], [3.0, 1.0], [-1.0, 1.0]],
        });
        assert_eq!(
            ring(&request, 4, &mut Cancellation::never()).unwrap()[0],
            Point3::new(1.0, 1.0, 4.0)
        );
    }

    #[test]
    fn represented_profile_must_retain_area_after_station_placement() {
        let mut request = station(SupportProfile::Rectangle {
            size_mm: [1e-20, 1e-20],
        });
        request.center_mm = [1.0, 1.0, 1.0];
        assert!(matches!(
            ring(&request, 4, &mut Cancellation::never()),
            Err(SupportError::Invalid { .. })
        ));
        request.profile = SupportProfile::Rectangle {
            size_mm: [1.0, 1e-20],
        };
        assert!(matches!(
            ring(&request, 4, &mut Cancellation::never()),
            Err(SupportError::Invalid { .. })
        ));
        request.center_mm = [0.0, 0.0, 0.0];
        request.profile = SupportProfile::Rectangle {
            size_mm: [1e-20, 1e-20],
        };
        assert!(ring(&request, 4, &mut Cancellation::never()).is_ok());
    }
}
