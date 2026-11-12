//! Independent inclusion brackets for the represented convex cavity, not a Hausdorff claim.
use super::numeric::I;
use super::{
    cross, dot, normal, point, uncertain, vector, HollowCertificate, HollowError, Plane,
    VolumeBounds, Work,
};
use crate::{cancellation::Cancellation, geometry::Point3, mesh::TriangleMesh};
fn abs_upper(i: I) -> f64 {
    i.lo.abs().max(i.hi.abs())
}
fn volume(
    mesh: &TriangleMesh,
    origin: Point3,
    work: &mut Work,
    cancel: &mut Cancellation<'_>,
) -> Result<I, HollowError> {
    let mut volume = I::point(0.);
    for triangle in mesh.triangles() {
        work.charge(1, cancel)?;
        let [a, b, c] = triangle.map(|i| mesh.vertices()[i as usize]);
        volume = volume.add(dot(
            vector(a, origin),
            cross(vector(b, origin), vector(c, origin)),
        ));
    }
    let value = volume
        .div(I::point(6.))
        .filter(|v| v.is_finite() && v.lo > 0.)
        .ok_or_else(|| uncertain("positive volume interval"))?;
    Ok(value)
}
fn bounds(i: I) -> VolumeBounds {
    VolumeBounds {
        minimum_mm3: i.lo,
        maximum_mm3: i.hi,
    }
}
#[derive(Clone, Copy)]
struct Facet {
    normal: [I; 3],
    offset: I,
    anchor: Point3,
}
#[derive(Clone, Copy)]
struct Context {
    origin: Point3,
    radius: [f64; 3],
    thickness: f64,
}
fn determinant(first: [f64; 3], second: [f64; 3], third: [f64; 3]) -> f64 {
    first[0] * (second[1] * third[2] - second[2] * third[1])
        + first[1] * (second[2] * third[0] - second[0] * third[2])
        + first[2] * (second[0] * third[1] - second[1] * third[0])
}
// Any nonnegative weights are valid. The interval residual covers errors in the floating
// proposal solve; the solve itself is never treated as an exact normal-cone certificate.
fn dual_bound(
    facet: Facet,
    context: Context,
    planes: &[Plane; 3],
    weights: [f64; 3],
) -> Option<f64> {
    if weights.iter().any(|v| !v.is_finite() || *v < 0.) {
        return None;
    }
    let mut summed_normal = [I::point(0.); 3];
    let mut summed_offset = I::point(0.);
    let mut weight_sum = I::point(0.);
    for (plane, weight) in planes.iter().copied().zip(weights) {
        let weight = I::point(weight);
        weight_sum = weight_sum.add(weight);
        summed_offset = summed_offset.add(
            dot(plane.interval, vector(plane.anchor, context.origin))
                .sub(I::point(context.thickness))
                .mul(weight),
        );
        for (sum, normal) in summed_normal.iter_mut().zip(plane.interval) {
            *sum = sum.add(normal.mul(weight));
        }
    }
    if weight_sum.lo <= 0. {
        return None;
    }
    let mut drift = I::point(0.);
    for (index, sum) in summed_normal.into_iter().enumerate() {
        drift = drift.add(
            I::point(abs_upper(facet.normal[index].sub(sum))).mul(I::point(context.radius[index])),
        );
    }
    summed_offset
        .sub(facet.offset)
        .add(drift)
        .div(weight_sum)
        .filter(|v| v.is_finite())
        .map(|v| v.hi)
}
fn combined_bound(
    facet: Facet,
    context: Context,
    planes: &[Plane],
    work: &mut Work,
    cancel: &mut Cancellation<'_>,
) -> Result<f64, HollowError> {
    let mut ids = [0; 8];
    let mut scores = [f64::INFINITY; 8];
    let mut count = 0usize;
    for (index, plane) in planes.iter().enumerate() {
        work.charge(1, cancel)?;
        let delta = vector(facet.anchor, plane.anchor);
        let residual = dot(plane.interval, delta).add(I::point(context.thickness));
        let score = abs_upper(residual);
        if let Some(slot) = scores.iter().position(|old| score < *old) {
            for i in (slot + 1..8).rev() {
                scores[i] = scores[i - 1];
                ids[i] = ids[i - 1];
            }
            scores[slot] = score;
            ids[slot] = index;
            count = (count + 1).min(8);
        }
    }
    let target = facet.normal.map(|v| v.lo / 2. + v.hi / 2.);
    let mut best = f64::INFINITY;
    for first in 0..count {
        for second in first + 1..count {
            for third in second + 1..count {
                work.charge(1, cancel)?;
                let selected = [planes[ids[first]], planes[ids[second]], planes[ids[third]]];
                let [a, b, c] = selected.map(|p| p.normal);
                let det = determinant(a, b, c);
                if !det.is_finite() || det == 0. {
                    continue;
                }
                let weights = [
                    determinant(target, b, c) / det,
                    determinant(a, target, c) / det,
                    determinant(a, b, target) / det,
                ];
                if weights.iter().any(|v| !v.is_finite()) {
                    continue;
                }
                if let Some(bound) =
                    dual_bound(facet, context, &selected, weights.map(|v| v.max(0.)))
                {
                    best = best.min(bound);
                }
            }
        }
    }
    Ok(best)
}
pub(super) fn certify(
    source: &TriangleMesh,
    cavity: &TriangleMesh,
    planes: &[Plane],
    thickness: f64,
    error: f64,
    work: &mut Work,
    cancel: &mut Cancellation<'_>,
) -> Result<HollowCertificate, HollowError> {
    let origin = source.vertices()[0];
    let mut radius = [0.0_f64; 3];
    for &p in source.vertices() {
        work.charge(1, cancel)?;
        let delta = vector(p, origin);
        for i in 0..3 {
            radius[i] = radius[i].max(abs_upper(delta[i]));
        }
    }
    let mut achieved = 0.0_f64;
    for &p in cavity.vertices() {
        for plane in planes {
            work.charge(1, cancel)?;
            let residual = dot(plane.interval, vector(p, plane.anchor)).add(I::point(thickness));
            if !residual.is_finite() || residual.hi > error {
                return Err(uncertain("cavity violates requested outer erosion bracket"));
            }
            achieved = achieved.max(residual.hi);
        }
    }
    for triangle in cavity.triangles() {
        let [a, b, c] = triangle.map(|i| cavity.vertices()[i as usize]);
        let m = normal(a, b, c)?;
        let offset = dot(m, vector(a, origin));
        let mut best = f64::INFINITY;
        for plane in planes {
            work.charge(1, cancel)?;
            let drift = std::array::from_fn::<_, 3, _>(|i| {
                I::point(abs_upper(m[i].sub(plane.interval[i]))).mul(I::point(radius[i]))
            });
            let angular = drift[0].add(drift[1]).add(drift[2]);
            let required = dot(plane.interval, vector(plane.anchor, origin))
                .sub(I::point(thickness))
                .sub(offset)
                .add(angular);
            if required.is_finite() {
                best = best.min(required.hi);
            }
        }
        if best > error || !best.is_finite() {
            best = best.min(combined_bound(
                Facet {
                    normal: m,
                    offset,
                    anchor: a,
                },
                Context {
                    origin,
                    radius,
                    thickness,
                },
                planes,
                work,
                cancel,
            )?);
        }
        if best > error || !best.is_finite() {
            return Err(uncertain(
                "cavity facet cannot certify requested inner erosion bracket",
            ));
        }
        achieved = achieved.max(best);
    }
    if !achieved.is_finite() || achieved >= thickness {
        return Err(uncertain("wall offset error consumes thickness"));
    }
    let mut sum = [0.; 3];
    let count =
        u32::try_from(cavity.vertices().len()).map_err(|_| uncertain("cavity centroid count"))?;
    for &p in cavity.vertices() {
        work.charge(1, cancel)?;
        let p = point(p);
        for i in 0..3 {
            sum[i] += p[i] / f64::from(count);
        }
    }
    let center = Point3::new(sum[0], sum[1], sum[2]);
    for plane in planes {
        work.charge(1, cancel)?;
        let residual = dot(plane.interval, vector(center, plane.anchor))
            .add(I::point(thickness))
            .add(I::point(achieved));
        if !residual.is_finite() || residual.hi >= 0. {
            return Err(uncertain(
                "nonempty inner erosion bracket cannot be certified",
            ));
        }
    }
    let outer = volume(source, origin, work, cancel)?;
    let inner = volume(cavity, origin, work, cancel)?;
    let material = outer.sub(inner);
    if !material.is_finite() || material.lo <= 0. {
        return Err(uncertain("positive shell volume cannot be certified"));
    }
    Ok(HollowCertificate {
        wall_offset_error_mm: achieved,
        source_volume: bounds(outer),
        cavity_volume: bounds(inner),
        material_volume: bounds(material),
    })
}
