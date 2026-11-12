//! Floating-point proposal construction only; the final hull requires an independent certificate.
use super::numeric::orient3d;
use super::{invalid, point, resource, uncertain, HollowError, Plane, Work};
use crate::{cancellation::Cancellation, geometry::Point3, mesh::TriangleMesh};
use std::cmp::Ordering;

struct Poly {
    points: Vec<Point3>,
    faces: Vec<Vec<u32>>,
}
fn distance(plane: Plane, p: Point3, t: f64) -> f64 {
    let p = point(p);
    let a = point(plane.anchor);
    ((plane.normal[0] * (p[0] - a[0]) + plane.normal[1] * (p[1] - a[1]))
        + plane.normal[2] * (p[2] - a[2]))
        + t
}
fn intersection(
    poly: &mut Poly,
    edge: [u32; 2],
    distances: [f64; 2],
    cache: &mut Vec<([u32; 2], u32)>,
    work: &mut Work,
    cancel: &mut Cancellation<'_>,
) -> Result<u32, HollowError> {
    let [a, b] = edge;
    let [da, db] = distances;
    if da == 0. {
        return Ok(a);
    }
    if db == 0. {
        return Ok(b);
    }
    let key = [a.min(b), a.max(b)];
    for &(old, id) in cache.iter() {
        work.charge(1, cancel)?;
        if old == key {
            return Ok(id);
        }
    }
    let fraction = da / (da - db);
    if !fraction.is_finite() || !(0. ..=1.).contains(&fraction) {
        return Err(uncertain("clipping intersection parameter"));
    }
    let pa = point(poly.points[a as usize]);
    let pb = point(poly.points[b as usize]);
    let v = std::array::from_fn::<_, 3, _>(|i| pa[i] + fraction * (pb[i] - pa[i]));
    if v.iter().any(|x| !x.is_finite()) {
        return Err(uncertain("clipping intersection coordinate"));
    }
    if poly.points.len() >= work.limits.max_output_vertices {
        return Err(resource("construction vertices"));
    }
    let id = u32::try_from(poly.points.len()).map_err(|_| resource("vertex index"))?;
    work.push(&mut poly.points, Point3::new(v[0], v[1], v[2]))?;
    work.push(cache, (key, id))?;
    Ok(id)
}
fn projected(p: Point3, axis: usize) -> Point3 {
    let p = point(p);
    Point3::new(p[(axis + 1) % 3], p[(axis + 2) % 3], 0.)
}
fn turn(points: &[Point3], a: u32, b: u32, c: u32, axis: usize) -> Ordering {
    orient3d(
        projected(points[a as usize], axis),
        projected(points[b as usize], axis),
        projected(points[c as usize], axis),
        Point3::new(0., 0., 1.),
    )
}
fn cap(
    points: &[Point3],
    mut ids: Vec<u32>,
    plane: Plane,
    work: &mut Work,
    cancel: &mut Cancellation<'_>,
) -> Result<Vec<u32>, HollowError> {
    let axis = (0..3)
        .max_by(|&a, &b| plane.normal[a].abs().total_cmp(&plane.normal[b].abs()))
        .ok_or_else(|| uncertain("cap axis"))?;
    work.charge(ids.len(), cancel)?;
    ids.sort_unstable_by(|&a, &b| {
        let a = projected(points[a as usize], axis);
        let b = projected(points[b as usize], axis);
        a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y))
    });
    ids.dedup();
    let mut hull = Vec::new();
    work.reserve(
        &mut hull,
        ids.len()
            .checked_mul(2)
            .ok_or_else(|| resource("cap size"))?,
    )?;
    for &id in &ids {
        while hull.len() >= 2 {
            work.charge(1, cancel)?;
            if turn(points, hull[hull.len() - 2], hull[hull.len() - 1], id, axis)
                == Ordering::Greater
            {
                break;
            }
            hull.pop();
        }
        hull.push(id);
    }
    let lower = hull.len();
    for &id in ids.iter().rev().skip(1) {
        while hull.len() > lower {
            work.charge(1, cancel)?;
            if turn(points, hull[hull.len() - 2], hull[hull.len() - 1], id, axis)
                == Ordering::Greater
            {
                break;
            }
            hull.pop();
        }
        hull.push(id);
    }
    hull.pop();
    if hull.len() < 3 {
        return Err(uncertain("cavity collapsed during clipping"));
    }
    if plane.normal[axis] < 0. {
        hull.reverse();
    }
    Ok(hull)
}
fn clip(
    poly: &mut Poly,
    plane: Plane,
    t: f64,
    work: &mut Work,
    cancel: &mut Cancellation<'_>,
) -> Result<(), HollowError> {
    let mut next = Vec::new();
    work.reserve(&mut next, poly.faces.len() + 1)?;
    let mut cuts = Vec::new();
    let mut cache = Vec::new();
    let old = std::mem::take(&mut poly.faces);
    for face in old {
        let mut output = Vec::new();
        work.reserve(&mut output, face.len() + 2)?;
        for i in 0..face.len() {
            work.charge(1, cancel)?;
            let a = face[i];
            let b = face[(i + 1) % face.len()];
            let da = distance(plane, poly.points[a as usize], t);
            let db = distance(plane, poly.points[b as usize], t);
            if !da.is_finite() || !db.is_finite() {
                return Err(uncertain("clipping distance"));
            }
            if (da <= 0.) != (db <= 0.) {
                let id = intersection(poly, [a, b], [da, db], &mut cache, work, cancel)?;
                work.push(&mut output, id)?;
                work.push(&mut cuts, id)?;
            }
            if db <= 0. {
                work.push(&mut output, b)?;
            }
        }
        output.dedup();
        if output.len() > 1 && output.first() == output.last() {
            output.pop();
        }
        if output.len() >= 3 {
            next.push(output);
        }
    }
    if next.is_empty() {
        return Err(uncertain(
            "requested thickness leaves no constructed cavity",
        ));
    }
    if !cuts.is_empty() {
        next.push(cap(&poly.points, cuts, plane, work, cancel)?);
    }
    if next.len() > work.limits.max_output_triangles {
        return Err(resource("construction faces"));
    }
    poly.faces = next;
    Ok(())
}
fn snap(value: f64, pitch: f64) -> Result<f64, HollowError> {
    let ticks = value / pitch;
    if !ticks.is_finite() || ticks.abs() > 1_125_899_906_842_624.0 {
        return Err(uncertain("cavity construction grid range"));
    }
    let result = ticks.round() * pitch;
    Ok(if result == 0. { 0. } else { result })
}
pub(super) fn propose(
    source: &TriangleMesh,
    planes: &[Plane],
    t: f64,
    error: f64,
    work: &mut Work,
    cancel: &mut Cancellation<'_>,
) -> Result<Vec<Point3>, HollowError> {
    let mut poly = Poly {
        points: Vec::new(),
        faces: Vec::new(),
    };
    work.reserve(&mut poly.points, source.vertices().len())?;
    poly.points.extend_from_slice(source.vertices());
    work.reserve(&mut poly.faces, source.triangles().len())?;
    for &triangle in source.triangles() {
        let mut face = Vec::new();
        work.reserve(&mut face, 3)?;
        face.extend_from_slice(&triangle);
        poly.faces.push(face);
    }
    for &plane in planes {
        work.charge(1, cancel)?;
        clip(&mut poly, plane, t, work, cancel)?;
    }
    let target = error / 64.;
    if !target.is_normal() {
        return Err(invalid("construction error is below numerical range", None));
    }
    let bits = target.to_bits();
    let pitch = f64::from_bits(bits & (!((1_u64 << 52) - 1)));
    if pitch <= 0. {
        return Err(uncertain("construction grid pitch"));
    }
    let mut ids = Vec::new();
    for face in &poly.faces {
        work.reserve(&mut ids, face.len())?;
        ids.extend_from_slice(face);
    }
    work.charge(ids.len(), cancel)?;
    ids.sort_unstable();
    ids.dedup();
    let mut result = Vec::new();
    work.reserve(&mut result, ids.len())?;
    for id in ids {
        work.charge(1, cancel)?;
        let p = point(poly.points[id as usize]);
        result.push(Point3::new(
            snap(p[0], pitch)?,
            snap(p[1], pitch)?,
            snap(p[2], pitch)?,
        ));
    }
    Ok(result)
}
