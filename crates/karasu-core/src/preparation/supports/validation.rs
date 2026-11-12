//! Conservative whole-component support contact and separating-halfspace proofs.
use super::{
    SupportCertificate, SupportComponent, SupportContact, SupportError, SupportLimits,
    SupportRecipe, Work,
};
use crate::{
    cancellation::Cancellation,
    geometry::Point3,
    mesh::TriangleMesh,
    preparation::hollow::numeric::{orient3d, I},
};
use std::{cmp::Ordering, ops::Range};
#[derive(Clone, Copy)]
struct Plane {
    anchor: Point3,
    normal: [I; 3],
}
fn vector(a: Point3, b: Point3) -> [I; 3] {
    std::array::from_fn(|i| I::point(a.coordinates()[i]).sub(I::point(b.coordinates()[i])))
}
fn dot(a: [I; 3], b: [I; 3]) -> I {
    a[0].mul(b[0]).add(a[1].mul(b[1])).add(a[2].mul(b[2]))
}
fn plane(mesh: &TriangleMesh, face: [u32; 3]) -> Result<Plane, SupportError> {
    let [origin, first, second] = face.map(|i| mesh.vertices()[i as usize]);
    let edge = vector(first, origin);
    let other = vector(second, origin);
    let normal = [
        edge[1].mul(other[2]).sub(edge[2].mul(other[1])),
        edge[2].mul(other[0]).sub(edge[0].mul(other[2])),
        edge[0].mul(other[1]).sub(edge[1].mul(other[0])),
    ];
    let norm = dot(normal, normal).sqrt().ok_or(SupportError::Uncertain {
        reason: "support obstacle normal",
    })?;
    let mut unit = [I::point(0.); 3];
    for i in 0..3 {
        unit[i] = normal[i]
            .div(norm)
            .filter(|v| v.is_finite())
            .ok_or(SupportError::Uncertain {
                reason: "support obstacle normal interval",
            })?;
    }
    Ok(Plane {
        anchor: origin,
        normal: unit,
    })
}
fn distance(plane: Plane, point: Point3) -> I {
    dot(plane.normal, vector(point, plane.anchor))
}
fn contact_error(id: u64, reason: &'static str) -> SupportError {
    SupportError::Contact {
        support: id,
        reason,
    }
}
fn same_plane(mesh: &TriangleMesh, selected: [u32; 3], candidate: [u32; 3]) -> bool {
    let [a, b, c] = selected.map(|i| mesh.vertices()[i as usize]);
    candidate
        .iter()
        .all(|&i| orient3d(a, b, c, mesh.vertices()[i as usize]) == Ordering::Equal)
}
fn all_inside(
    mesh: &TriangleMesh,
    range: Range<usize>,
    points: &[Point3],
    margin: f64,
    work: &mut Work,
    cancel: &mut Cancellation<'_>,
) -> Result<bool, SupportError> {
    for &face in &mesh.triangles()[range] {
        let bound = plane(mesh, face)?;
        for &p in points {
            work.tick(cancel)?;
            if distance(bound, p).hi >= -margin {
                return Ok(false);
            }
        }
    }
    Ok(true)
}
fn separated(
    mesh: &TriangleMesh,
    range: Range<usize>,
    points: &[Point3],
    margin: f64,
    work: &mut Work,
    cancel: &mut Cancellation<'_>,
) -> Result<bool, SupportError> {
    for &face in &mesh.triangles()[range] {
        let bound = plane(mesh, face)?;
        let mut outside = true;
        for &p in points {
            work.tick(cancel)?;
            if distance(bound, p).lo <= margin {
                outside = false;
                break;
            }
        }
        if outside {
            return Ok(true);
        }
    }
    Ok(false)
}
fn bounds_points(
    mesh: &TriangleMesh,
    range: Range<usize>,
    work: &mut Work,
    cancel: &mut Cancellation<'_>,
) -> Result<[[f64; 2]; 3], SupportError> {
    let mut result = [[f64::INFINITY, f64::NEG_INFINITY]; 3];
    for face in &mesh.triangles()[range] {
        work.tick(cancel)?;
        for &id in face {
            for (i, value) in mesh.vertices()[id as usize]
                .coordinates()
                .into_iter()
                .enumerate()
            {
                result[i][0] = result[i][0].min(value);
                result[i][1] = result[i][1].max(value);
            }
        }
    }
    Ok(result)
}
fn boxes_separated(first: [[f64; 2]; 3], second: [[f64; 2]; 3], margin: f64) -> bool {
    (0..3).any(|i| {
        I::point(first[i][1]).add(I::point(margin)).hi < second[i][0]
            || I::point(second[i][1]).add(I::point(margin)).hi < first[i][0]
    })
}
/// A sufficient separating-facet or coordinate-axis certificate, not a complete SAT intersection test.
pub(super) fn meshes_separated(
    first: &TriangleMesh,
    second: &TriangleMesh,
    margin: f64,
    work: &mut Work,
    cancel: &mut Cancellation<'_>,
) -> Result<bool, SupportError> {
    work.tick(cancel)?;
    if boxes_separated(
        bounds_points(first, 0..first.triangles().len(), work, cancel)?,
        bounds_points(second, 0..second.triangles().len(), work, cancel)?,
        margin,
    ) {
        return Ok(true);
    }
    Ok(separated(
        first,
        0..first.triangles().len(),
        second.vertices(),
        margin,
        work,
        cancel,
    )? || separated(
        second,
        0..second.triangles().len(),
        first.vertices(),
        margin,
        work,
        cancel,
    )?)
}
fn cuts_clear(
    model: &super::scene::PreparedBase,
    mesh: &TriangleMesh,
    margin: f64,
    work: &mut Work,
    cancel: &mut Cancellation<'_>,
) -> Result<bool, SupportError> {
    let (boundary, labels) = model.boundary_scene();
    let mut start = 0;
    while start < labels.len() {
        work.tick(cancel)?;
        let label = labels[start];
        let mut end = start + 1;
        while end < labels.len() && labels[end] == label {
            work.tick(cancel)?;
            end += 1;
        }
        if label != 0
            && !boxes_separated(
                bounds_points(boundary, start..end, work, cancel)?,
                bounds_points(mesh, 0..mesh.triangles().len(), work, cancel)?,
                margin,
            )
            && !separated(boundary, start..end, mesh.vertices(), margin, work, cancel)?
        {
            return Ok(false);
        }
        start = end;
    }
    Ok(true)
}
fn certify_tip(
    model: &super::scene::PreparedBase,
    recipe: &SupportRecipe,
    tip: &TriangleMesh,
    sections: [&[Point3]; 2],
    limits: SupportLimits,
    work: &mut Work,
    cancel: &mut Cancellation<'_>,
) -> Result<(f64, f64), SupportError> {
    let (source, count) = model.exterior();
    let [start, end] = sections;
    let SupportContact {
        source_triangle,
        position_mm,
        maximum_penetration_mm,
    } = recipe.contact;
    let Some(&face) = source.triangles()[..count].get(source_triangle) else {
        return Err(contact_error(
            recipe.id,
            "source triangle is outside the placed exterior",
        ));
    };
    let selected = plane(source, face)?;
    let anchor = Point3::new(position_mm[0], position_mm[1], position_mm[2]);
    let offset = distance(selected, anchor);
    if offset.lo < -limits.contact_tolerance_mm || offset.hi > limits.contact_tolerance_mm {
        return Err(contact_error(
            recipe.id,
            "contact position is not certified on its selected exterior plane",
        ));
    }
    if !all_inside(
        tip,
        0..tip.triangles().len(),
        &[anchor],
        limits.contact_tolerance_mm,
        work,
        cancel,
    )? {
        return Err(contact_error(
            recipe.id,
            "contact position lacks strict interior margin within the penetrating tip",
        ));
    }
    for &p in start {
        work.tick(cancel)?;
        if distance(selected, p).lo <= limits.clearance_mm {
            return Err(contact_error(
                recipe.id,
                "neck section is not entirely exterior to the selected contact face",
            ));
        }
    }
    let mut minimum = f64::INFINITY;
    let mut maximum: f64 = 0.;
    for &p in end {
        work.tick(cancel)?;
        let depth = distance(selected, p);
        minimum = minimum.min(-depth.hi);
        maximum = maximum.max(-depth.lo);
    }
    if minimum <= limits.clearance_mm || maximum > maximum_penetration_mm {
        return Err(contact_error(
            recipe.id,
            "terminal section lacks bounded strict penetration",
        ));
    }
    for &candidate in &source.triangles()[..count] {
        work.tick(cancel)?;
        if same_plane(source, face, candidate) {
            continue;
        }
        let bound = plane(source, candidate)?;
        for &p in tip.vertices() {
            work.tick(cancel)?;
            if distance(bound, p).hi >= -limits.clearance_mm {
                return Err(contact_error(
                    recipe.id,
                    "tip is not certified confined to one exterior facet",
                ));
            }
        }
    }
    Ok((minimum, maximum))
}
pub(super) fn certify(
    model: &super::scene::PreparedBase,
    recipe: &SupportRecipe,
    components: &[SupportComponent],
    rings: &[Vec<Point3>],
    limits: SupportLimits,
    work: &mut Work,
    cancel: &mut Cancellation<'_>,
) -> Result<SupportCertificate, SupportError> {
    let (source, exterior) = model.exterior();
    if rings[0].len() < 3 || rings[0].iter().any(|p| p.z != 0.) {
        return Err(contact_error(
            recipe.id,
            "base must have positive-area platform section at world Z=0",
        ));
    }
    let mut penetration = None;
    for (index, component) in components.iter().enumerate() {
        let mesh = &component.mesh;
        if mesh.vertices().iter().any(|p| p.z < 0.) {
            return Err(contact_error(
                recipe.id,
                "support extends below the platform",
            ));
        }
        if index + 1 == components.len() {
            penetration = Some(certify_tip(
                model,
                recipe,
                mesh,
                [&rings[index], &rings[index + 1]],
                limits,
                work,
                cancel,
            )?);
        } else if !separated(
            source,
            0..exterior,
            mesh.vertices(),
            limits.clearance_mm,
            work,
            cancel,
        )? {
            return Err(SupportError::Collision {
                support: recipe.id,
                component: index,
                obstacle: "unintended model contact or uncertain separation",
            });
        }
        if let Some(cavity) = model.cavity() {
            if !separated(
                cavity,
                0..cavity.triangles().len(),
                mesh.vertices(),
                limits.clearance_mm,
                work,
                cancel,
            )? {
                return Err(SupportError::Collision {
                    support: recipe.id,
                    component: index,
                    obstacle: "cavity",
                });
            }
        }
        if !cuts_clear(model, mesh, limits.clearance_mm, work, cancel)? {
            return Err(SupportError::Collision {
                support: recipe.id,
                component: index,
                obstacle: "drainage cutter",
            });
        }
    }
    let (minimum, maximum) =
        penetration.ok_or(contact_error(recipe.id, "missing tip component"))?;
    Ok(SupportCertificate {
        support_id: recipe.id,
        minimum_penetration_mm: minimum,
        maximum_penetration_mm: maximum,
        clearance_mm: limits.clearance_mm,
    })
}
