//! Contacts fitted to the surface (TD-116): a support that asks for it narrows its contact until
//! the contact's footprint lies on the model, never widening it.
//!
//! **The surface's normal.** The room is measured along the surface's normal at the tip, `n`:
//! the area-weighted mean of the normals of the triangles within the widest footprint's radius
//! of the tip that face the anchor triangle's way (less than 90° from its normal). A tip on a
//! vertex of a tessellated curve sees the curve, not the one facet its anchor happens to be on;
//! a wall's other side, facing away, does not count. The tip's axis is not involved.
//!
//! **Room for a footprint.** A footprint of radius `r` with penetration `d` has room at a tip `p`
//! when, with `e = max(d / 2, 2τ)` (`τ` the anchor tolerance):
//! - the point `e` below the tip is inside the model: going back out along `n`, the first
//!   surface crossed is left, not entered;
//! - the column below it, down to `d + τ`, crosses no surface (no thin wall under the tip);
//! - each of [`SPOKES`] spokes of length `r`, perpendicular to `n`, from the column at depths `e`
//!   and `d`, crosses no surface.
//!
//! So the whole footprint keeps at least half its penetration (and at least `2τ`) in material:
//! the surface may fall away from the tip's tangent plane by less than that within the
//! footprint. A sharp edge, a fin narrower than the footprint, a tip too close to a border or a
//! wall thinner than the penetration narrows the contact; a concave corner does not. Crossings
//! are found with exact orientation predicates; the order of crossings along the column, used
//! only for the first test, is by rounded plane intersections.
//!
//! **Narrowing.** The contact, its ball and its penetration narrow together by one factor, so a
//! smaller contact keeps the shape of the larger one: from `contact_diameter_mm` down by
//! [`CONTACT_FIT_STEP_MM`] to `contact_fit_min_mm`, the first that has room is built. When none
//! has, the smallest is built and reported.
#![allow(clippy::many_single_char_names)] // Geometric notation: points, normals, radii.
use super::{
    document::{SupportParameters, CONTACT_FIT_STEP_MM},
    index::{normal, segment_meets_triangle, TriangleIndex},
    shapes::{add, dot, frame, scale, sub, V},
    tip::surface_normal,
};
use crate::geometry::sin_cos_degrees;

/// Spokes of a footprint tested for room, evenly around the normal.
pub(super) const SPOKES: u16 = 16;

/// The parameters a support is built with when its contact fits the surface: `None` when the
/// support does not ask for it; else the narrowed parameters and whether they have room.
/// `anchor` is the outward normal of the triangle the tip is anchored on.
pub(super) fn fitted(
    index: &TriangleIndex<'_>,
    tip: V,
    anchor: V,
    parameters: &SupportParameters,
    tolerance: f64,
) -> Option<(SupportParameters, bool)> {
    let least = parameters.contact_fit_min_mm?;
    let widest = parameters.contact_diameter_mm;
    let radius = parameters.footprint_radius_mm();
    let reach = [radius; 3];
    let near = index
        .candidates(sub(tip, reach), add(tip, reach))
        .into_iter()
        .map(|t| index.corners(t));
    let normal = surface_normal(near, tip, anchor, radius);
    let mut step = 0.0;
    loop {
        let diameter = widest - step * CONTACT_FIT_STEP_MM;
        // The last step is the smallest itself, not the grid's value below it.
        if diameter <= least + 1e-9 {
            let narrowed = narrowed(parameters, least);
            let room = has_room(index, tip, normal, &narrowed, tolerance);
            return Some((narrowed, room));
        }
        let narrowed = narrowed(parameters, diameter);
        if has_room(index, tip, normal, &narrowed, tolerance) {
            return Some((narrowed, true));
        }
        step += 1.0;
    }
}

/// `parameters` with a contact of `diameter`, its ball and penetration narrowed alike.
fn narrowed(parameters: &SupportParameters, diameter: f64) -> SupportParameters {
    let factor = diameter / parameters.contact_diameter_mm;
    SupportParameters {
        contact_diameter_mm: diameter,
        contact_sphere_diameter_mm: parameters.contact_sphere_diameter_mm * factor,
        penetration_mm: parameters.penetration_mm * factor,
        ..parameters.clone()
    }
}

/// Whether the contact of `p` has room at `tip` (see the module's documentation).
fn has_room(
    index: &TriangleIndex<'_>,
    tip: V,
    n: V,
    p: &SupportParameters,
    tolerance: f64,
) -> bool {
    let r = p.footprint_radius_mm();
    let d = p.penetration_mm;
    let e = (d / 2.0).max(2.0 * tolerance);
    let below = |depth: f64| sub(tip, scale(n, depth));
    if !leaves_first(index, below(e), add(tip, scale(n, 2.0 * tolerance))) {
        return false;
    }
    if d + tolerance > e && meets(index, below(e), below(d + tolerance)) {
        return false;
    }
    if r <= 0.0 {
        return true;
    }
    let (u, v) = frame(n);
    (0..SPOKES).all(|k| {
        let (sine, cosine) = sin_cos_degrees(360.0 * f64::from(k) / f64::from(SPOKES));
        let spoke = add(scale(u, r * cosine), scale(v, r * sine));
        let clear = |depth: f64| !meets(index, below(depth), add(below(depth), spoke));
        clear(e) && (d <= e || clear(d))
    })
}

/// The triangles that may meet the segment `[a, b]`.
fn near(index: &TriangleIndex<'_>, a: V, b: V) -> Vec<u32> {
    index.candidates(
        [0, 1, 2].map(|k| a[k].min(b[k])),
        [0, 1, 2].map(|k| a[k].max(b[k])),
    )
}

/// Whether the segment `[a, b]` meets the surface.
fn meets(index: &TriangleIndex<'_>, a: V, b: V) -> bool {
    near(index, a, b)
        .into_iter()
        .any(|t| segment_meets_triangle(a, b, index.corners(t)))
}

/// Whether going from `a` to `b`, the first surface crossed is left (its outward normal along
/// the way): `a` is then inside the model. No crossing, or a crossing in the plane of a
/// triangle, is not.
fn leaves_first(index: &TriangleIndex<'_>, a: V, b: V) -> bool {
    let way = sub(b, a);
    let mut first: Option<(f64, bool)> = None;
    for t in near(index, a, b) {
        let corners = index.corners(t);
        if !segment_meets_triangle(a, b, corners) {
            continue;
        }
        let Some(m) = normal(corners) else {
            return false;
        };
        let along = dot(m, way);
        if along == 0.0 {
            return false;
        }
        let at = dot(m, sub(corners[0], a)) / along;
        first = match first {
            // A tie (a shared edge) leaves only if every crossing there does.
            Some((best, leaves)) if (at - best).abs() <= 1e-12 => {
                Some((best, leaves && along > 0.0))
            }
            Some((best, _)) if at > best => first,
            _ => Some((at, along > 0.0)),
        };
    }
    first.is_some_and(|(_, leaves)| leaves)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::supports::document::ContactShape;
    use crate::{
        cancellation::Cancellation, geometry::Point3, mesh::TriangleMesh, supports::Preset,
    };

    /// A prism of the polygon `outline` (counter-clockwise in X and Z, a fan from its first
    /// corner) from Y = 0 to Y = 10.
    fn prism(outline: &[[f64; 2]]) -> TriangleMesh {
        let n = u32::try_from(outline.len()).unwrap();
        let mut vertices = Vec::new();
        for y in [0.0, 10.0] {
            vertices.extend(outline.iter().map(|&[x, z]| Point3::new(x, y, z)));
        }
        let mut triangles = Vec::new();
        for k in 1..n - 1 {
            // Seen from -Y the outline runs counter-clockwise in X and Z: that cap faces -Y.
            triangles.push([0, k, k + 1]);
            triangles.push([n, n + k + 1, n + k]);
        }
        for k in 0..n {
            let j = (k + 1) % n;
            triangles.push([k, n + k, n + j]);
            triangles.push([k, n + j, j]);
        }
        TriangleMesh::try_new(vertices, triangles).unwrap()
    }

    fn heavy_fitted(least: f64) -> SupportParameters {
        SupportParameters {
            contact_fit_min_mm: Some(least),
            ..Preset::Heavy.parameters()
        }
    }

    fn fit(mesh: &TriangleMesh, tip: V, p: &SupportParameters) -> (SupportParameters, bool) {
        let index = TriangleIndex::build(mesh, &mut Cancellation::never()).unwrap();
        fitted(&index, tip, [0.0, 0.0, 1.0], p, 0.05).unwrap()
    }

    /// An L: a tall arm on the left (X 0 to 4, Z 0 to 10) and a low one on the right (X 4 to
    /// 10, Z 0 to 4), whose top meets the tall arm in a concave corner at X 4.
    fn ell() -> TriangleMesh {
        let ell = prism(&[
            [0.0, 0.0],
            [10.0, 0.0],
            [10.0, 4.0],
            [4.0, 4.0],
            [4.0, 10.0],
            [0.0, 10.0],
        ]);
        // The fan of X and Z is counter-clockwise seen from +Y; the caps' winding is checked by
        // their normals.
        let index = TriangleIndex::build(&ell, &mut Cancellation::never()).unwrap();
        assert!(normal(index.corners(0)).unwrap()[1] < 0.0);
        ell
    }

    #[test]
    fn a_contact_on_open_surface_or_in_a_concave_corner_keeps_its_diameter() {
        let ell = ell();
        let p = heavy_fitted(0.2);
        for tip in [[7.0, 5.0, 4.0], [4.05, 5.0, 4.0], [2.0, 5.0, 10.0]] {
            let (built, room) = fit(&ell, tip, &p);
            assert!(room);
            assert_eq!(built, p, "{tip:?}");
        }
        // Without the request, nothing is fitted.
        let index = TriangleIndex::build(&ell, &mut Cancellation::never()).unwrap();
        let plain = Preset::Heavy.parameters();
        assert!(fitted(&index, [9.9, 5.0, 4.0], [0.0, 0.0, 1.0], &plain, 0.05).is_none());
    }

    #[test]
    fn a_contact_near_an_edge_narrows_until_its_footprint_lies_on_the_model() {
        // 0.23 mm from the edge at X 10: 0.6, 0.55 and 0.5 reach past it; 0.45 does not.
        let (built, room) = fit(&ell(), [9.77, 5.0, 4.0], &heavy_fitted(0.2));
        assert!(room);
        assert!((built.contact_diameter_mm - 0.45).abs() < 1e-12);
        let factor = 0.45 / 0.6;
        let heavy = Preset::Heavy.parameters();
        assert!((built.penetration_mm - heavy.penetration_mm * factor).abs() < 1e-12);
        assert!(
            (built.contact_sphere_diameter_mm - heavy.contact_sphere_diameter_mm * factor).abs()
                < 1e-12
        );
        assert_eq!(
            SupportParameters {
                contact_diameter_mm: heavy.contact_diameter_mm,
                contact_sphere_diameter_mm: heavy.contact_sphere_diameter_mm,
                penetration_mm: heavy.penetration_mm,
                contact_fit_min_mm: None,
                ..built
            },
            heavy,
            "nothing else narrows"
        );
        // Closer than the smallest allows: the smallest, reported as without room.
        let (built, room) = fit(&ell(), [9.95, 5.0, 4.0], &heavy_fitted(0.2));
        assert!(!room);
        assert!((built.contact_diameter_mm - 0.2).abs() < 1e-12);
    }

    #[test]
    fn a_wall_thinner_than_the_penetration_narrows_the_contact_and_its_penetration() {
        // A slab 0.21 mm thick: Heavy's contact sunk 0.3 mm (and the tolerance) would pass
        // through; at 0.3 mm the penetration is 0.15 mm and the column ends at 0.2 mm.
        let slab = prism(&[[0.0, 0.0], [10.0, 0.0], [10.0, 0.21], [0.0, 0.21]]);
        let deep = SupportParameters {
            penetration_mm: 0.3,
            ..heavy_fitted(0.2)
        };
        let (built, room) = fit(&slab, [5.0, 5.0, 0.21], &deep);
        assert!(room);
        assert!((built.contact_diameter_mm - 0.3).abs() < 1e-12);
        assert!((built.penetration_mm - 0.15).abs() < 1e-12);
    }

    #[test]
    fn a_ball_contact_fits_by_the_circle_where_it_meets_the_surface() {
        let p = SupportParameters {
            contact_shape: ContactShape::Sphere,
            penetration_mm: 0.3,
            ..heavy_fitted(0.2)
        };
        // Heavy's ball (0.8 mm) sunk 0.3 mm meets the surface on a circle of radius
        // √(0.3 · 0.5) ≈ 0.387 mm: 0.3 mm from the edge, it narrows; far from it, it does not.
        assert!((p.footprint_radius_mm() - 0.15f64.sqrt()).abs() < 1e-12);
        let (built, room) = fit(&ell(), [7.0, 5.0, 4.0], &p);
        assert!(room && built == p);
        let (built, room) = fit(&ell(), [9.7, 5.0, 4.0], &p);
        assert!(room);
        assert!(built.contact_diameter_mm < p.contact_diameter_mm);
        assert!(built.footprint_radius_mm() < 0.3);
    }
}
