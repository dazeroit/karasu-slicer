//! A support's ends near the model (TD-124): the surface's normal at a contact, and the stretch
//! of a contact's cone that must not meet the model.
//!
//! **The surface's normal.** At a contact, the area-weighted mean of the normals of the
//! triangles within the contact's footprint radius that face the anchor triangle's way (less
//! than 90° from its normal), as contacts fitted to the surface measure it (TD-116). A tip on a
//! vertex of a tessellated curve sees the curve, not the one facet it happens to be on; a wall's
//! other side, facing away, does not count. It orients a tip that follows the normal, and the
//! axis limit is measured from it.
//!
//! **The reach.** A contact's cone touches the model by design where it is anchored, so it is
//! exempt from the clearance (TD-062). Away from its contact it must not enter the model: on a
//! crease, the cone can cross the neighbouring surface. With the cone's axis at `θ` from the
//! normal `n`, its side at the distance `s` along the axis, of radius `r(s)`, stays above the
//! contact's tangent plane while `s cos θ ≥ r(s) sin θ + m` (`m` a small margin): from the
//! smallest such `s` to the cone's wide end, the *reach*, the cone must keep off every surface.
//! A flat or convex surface around the contact never meets the reach; a surface rising beside
//! it does. Near the contact, within [`NEAR_FOOTPRINTS`] footprint radii of it, the reach is
//! narrower than the cone by the contact's penetration: an intrusion there no deeper than the one
//! the contact makes by design is not counted, so the small bumps of a sculpted surface around a
//! contact are not taken for the surface beside it. Farther, the whole cone must keep off the
//! surface: a fold it grazes there is another part of the model.

use super::{
    document::{ContactShape, SupportParameters},
    index::{point_triangle_distance, proven_clear},
    shapes::{add, cross, dot, frustum, scale, sub, unit, Part, V},
};

/// How far from a contact, in its footprint radii, the reach is narrowed by the penetration.
pub(super) const NEAR_FOOTPRINTS: f64 = 3.0;

/// The stretch of a contact's cone that must not meet the model (see the module's
/// documentation).
pub(super) struct Reach {
    /// The cone over the stretch.
    pub(super) whole: Part,
    /// The same stretch narrowed by the allowance, near the contact; `None` when nothing of it
    /// is left.
    pub(super) narrow: Option<Part>,
    /// The contact, and the radius around it where the narrowed stretch is tested.
    tip: V,
    near: f64,
}

impl Reach {
    /// Whether the reach meets the triangle `tri`: the narrowed stretch near the contact, the
    /// whole one farther, each by the separating-axis proof without clearance.
    pub(super) fn meets(&self, tri: [V; 3]) -> bool {
        if point_triangle_distance(self.tip, tri) <= self.near {
            self.narrow
                .as_ref()
                .is_some_and(|narrow| !proven_clear(narrow, tri, 0.0))
        } else {
            !proven_clear(&self.whole, tri, 0.0)
        }
    }

    /// The box around the whole stretch: every triangle it may meet overlaps it.
    pub(super) fn bounds(&self) -> [V; 2] {
        let lo = [0, 1, 2].map(|k| {
            self.whole
                .vertices
                .iter()
                .map(|v| v[k])
                .fold(f64::INFINITY, f64::min)
        });
        let hi = [0, 1, 2].map(|k| {
            self.whole
                .vertices
                .iter()
                .map(|v| v[k])
                .fold(f64::NEG_INFINITY, f64::max)
        });
        [lo, hi]
    }
}

/// The cone of a support's tip, as its reach and the surface's normal at its contact need it
/// (TD-124): the build and the search for a route measure it alike.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TipCone {
    /// Where its narrow end is along the tip's axis from the contact, in mm: below the surface
    /// by the penetration for a cone, a ball's centre above it.
    pub from_mm: f64,
    /// Its radius at the narrow end and at the join with the shaft, in mm.
    pub radii_mm: [f64; 2],
    /// The tip's length, from the contact to the join, in mm.
    pub length_mm: f64,
    /// The contact's footprint radius, in mm: the surface's normal is the mean over it.
    pub footprint_mm: f64,
    /// How far above the contact's tangent plane the reach starts, in mm: twice the anchor
    /// tolerance.
    pub margin_mm: f64,
    /// How much narrower than the cone the reach is, in mm: the contact's penetration.
    pub allowance_mm: f64,
    /// Sides of the cone's polygon.
    pub sides: u16,
}

impl TipCone {
    /// The tip of a support of `parameters`, in a document of `anchor_tolerance_mm`.
    #[must_use]
    pub fn of(parameters: &SupportParameters, anchor_tolerance_mm: f64) -> Self {
        let p = parameters;
        Self {
            from_mm: match p.contact_shape {
                ContactShape::Cone => -p.penetration_mm,
                ContactShape::Sphere => p.contact_sphere_diameter_mm / 2.0 - p.penetration_mm,
            },
            radii_mm: [p.contact_diameter_mm / 2.0, p.tip_body_diameter_mm / 2.0],
            length_mm: p.tip_length_mm,
            footprint_mm: p.footprint_radius_mm(),
            margin_mm: 2.0 * anchor_tolerance_mm,
            allowance_mm: p.penetration_mm,
            sides: p.sides,
        }
    }

    /// The reach of this tip at `tip` along the unit `axis`, on a surface of unit outward
    /// `normal` there (see the module's documentation).
    pub(super) fn reach(&self, tip: V, axis: V, normal: V) -> Option<Reach> {
        reach(
            tip,
            axis,
            normal,
            (self.from_mm, self.radii_mm[0]),
            (self.length_mm, self.radii_mm[1]),
            self.margin_mm,
            (self.allowance_mm, NEAR_FOOTPRINTS * self.footprint_mm),
            self.sides,
        )
    }
}

/// The surface's outward normal at `tip` over `triangles` (see the module's documentation):
/// `anchor`, the anchor triangle's normal, when none is within `radius` or the mean cancels out.
pub(super) fn surface_normal(
    triangles: impl IntoIterator<Item = [V; 3]>,
    tip: V,
    anchor: V,
    radius: f64,
) -> V {
    let mut sum = [0.0; 3];
    for [a, b, c] in triangles {
        // Twice the area along the normal.
        let weighted = cross(sub(b, a), sub(c, a));
        if dot(weighted, anchor) > 0.0
            && super::index::point_triangle_distance(tip, [a, b, c]) <= radius
        {
            sum = add(sum, weighted);
        }
    }
    unit(sum).unwrap_or(anchor)
}

/// The reach of a contact's cone (see the module's documentation): the cone from `tip` along the
/// unit `axis`, of radius `from.1` at the distance `from.0` along it to `to.1` at `to.0`, cut
/// where its side rises `margin` above the tangent plane of the unit outward `normal`; within
/// `near.1` of the tip, narrowed by `near.0`. `None` when no stretch of it does: a cone along the
/// surface, or a tip into it.
#[allow(clippy::too_many_arguments)] // The cone, its contact and its tolerances.
pub(super) fn reach(
    tip: V,
    axis: V,
    normal: V,
    from: (f64, f64),
    to: (f64, f64),
    margin: f64,
    near: (f64, f64),
    sides: u16,
) -> Option<Reach> {
    let (allowance, near) = near;
    let ((s0, r0), (s1, r1)) = (from, to);
    if s1 <= s0 {
        return None;
    }
    let cos = dot(axis, normal);
    if cos <= 0.0 {
        return None;
    }
    let sin = (1.0 - cos * cos).max(0.0).sqrt();
    let slope = (r1 - r0) / (s1 - s0);
    let rising = cos - slope * sin;
    if rising <= 0.0 {
        return None;
    }
    let start = (((r0 - slope * s0) * sin + margin) / rising).max(s0);
    // Too short a stretch is no reach: the cone stays along its own contact.
    if start >= s1 - 1e-9 {
        return None;
    }
    let (a, b) = (add(tip, scale(axis, start)), add(tip, scale(axis, s1)));
    let radius = r0 + slope * (start - s0);
    let narrow = (radius - allowance > 0.0 || r1 - allowance > 0.0).then(|| {
        frustum(
            a,
            (radius - allowance).max(0.0),
            b,
            (r1 - allowance).max(0.0),
            sides,
        )
    });
    Some(Reach {
        whole: frustum(a, radius, b, r1, sides),
        narrow,
        tip,
        near,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reach_starts_where_the_cone_leaves_the_tangent_plane() {
        let up = [0.0, 0.0, 1.0];
        // Along the normal, a cone rises at once: its reach starts at the margin.
        let straight = reach(
            [0.0; 3],
            up,
            up,
            (0.0, 0.2),
            (2.0, 0.6),
            0.01,
            (0.0, 0.0),
            8,
        )
        .unwrap()
        .whole;
        assert!((straight.axis[0][2] - 0.01).abs() < 1e-12);
        // At 60°, it starts where s cos 60° = r(s) sin 60° + m: r(s) = 0.2 + 0.2 s, so
        // s = (0.2 sin + m) / (cos - 0.2 sin).
        let (sin, cos) = 60f64.to_radians().sin_cos();
        let tilted = [sin, 0.0, cos];
        let part = reach(
            [0.0; 3],
            tilted,
            up,
            (0.0, 0.2),
            (2.0, 0.6),
            0.01,
            (0.0, 0.0),
            8,
        )
        .unwrap()
        .whole;
        let s = (0.2 * sin + 0.01) / (cos - 0.2 * sin);
        let start = part.axis[0];
        assert!((start[0] - s * sin).abs() < 1e-12 && (start[2] - s * cos).abs() < 1e-12);
        assert!((part.taper[0] - (0.2 + 0.2 * s)).abs() < 1e-12);
        // Narrowed by an allowance: the same stretch, thinner.
        let narrowed = reach(
            [0.0; 3],
            tilted,
            up,
            (0.0, 0.2),
            (2.0, 0.6),
            0.01,
            (0.1, 0.5),
            8,
        )
        .unwrap()
        .narrow
        .unwrap();
        assert_eq!(narrowed.axis, part.axis);
        assert!((narrowed.taper[1] - 0.5).abs() < 1e-12);
        // Along the surface, or into it: no reach.
        assert!(reach(
            [0.0; 3],
            [1.0, 0.0, 0.0],
            up,
            (0.0, 0.2),
            (2.0, 0.6),
            0.01,
            (0.0, 0.0),
            8
        )
        .is_none());
        assert!(reach(
            [0.0; 3],
            [0.0, 0.0, -1.0],
            up,
            (0.0, 0.2),
            (2.0, 0.6),
            0.01,
            (0.0, 0.0),
            8
        )
        .is_none());
    }

    #[test]
    fn a_reach_tolerates_a_shallow_intrusion_only_near_its_contact() {
        // A cone straight up, 0.5 mm wide at 1.5 mm, narrowed by 0.1 mm; a wall at X 0.45 around
        // that height grazes its whole side (0.05 mm in) but not the narrowed one.
        let up = [0.0, 0.0, 1.0];
        let wall = [[0.45, -1.0, 1.4], [0.45, 1.0, 1.4], [0.45, 0.0, 1.6]];
        let cone = |near| {
            reach(
                [0.0; 3],
                up,
                up,
                (0.0, 0.2),
                (2.0, 0.6),
                0.01,
                (0.1, near),
                8,
            )
            .unwrap()
        };
        // Within the near radius of the contact: the contact's own allowance.
        assert!(!cone(2.0).meets(wall));
        // Farther: another part of the model, which the cone must keep off.
        assert!(cone(1.0).meets(wall));
        // A wall clear of the whole cone is clear either way.
        let away = [[0.7, -1.0, 1.4], [0.7, 1.0, 1.4], [0.7, 0.0, 1.6]];
        assert!(!cone(1.0).meets(away) && !cone(2.0).meets(away));
    }

    #[test]
    fn the_normal_is_the_mean_of_the_facets_around_the_tip() {
        // A ridge: two facets meeting at the tip, their normals 30° either side of vertical.
        let drop = -(1.0 / 3f64.sqrt());
        let left = [[-1.0, -1.0, drop], [0.0, -1.0, 0.0], [0.0, 1.0, 0.0]];
        let right = [[0.0, -1.0, 0.0], [1.0, -1.0, drop], [0.0, 1.0, 0.0]];
        let anchor = super::super::index::normal(left).unwrap();
        let n = surface_normal([left, right], [0.0; 3], anchor, 0.5);
        assert!((n[2] - 1.0).abs() < 1e-12, "{n:?}");
        // Nothing near: the anchor's own.
        assert_eq!(surface_normal([], [0.0; 3], anchor, 0.5), anchor);
    }
}
