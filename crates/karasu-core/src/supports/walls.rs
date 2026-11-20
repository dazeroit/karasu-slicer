//! The walls beside a contact (TD-164, owner 2026-11-20), which its part keeps the clearance
//! from in a document with `contact_clearance`.
//!
//! A contact touches the surface it holds, so the clearance cannot apply to every triangle near
//! it. A triangle is a **wall** beside a contact held at point `P` with unit outward normal `n`
//! (the surface's mean normal there, TD-124) when all three hold:
//! - it reaches more than [`WALL_BEYOND_MM`] beyond the contact's tangent plane, on the
//!   support's side: `max over its corners of (v − P)·n > 0.05 mm`;
//! - its normal turns from `n` by more than 45°;
//! - the part is in front of it: some point of the part's axis capsule lies on the side its
//!   outward normal faces.
//!
//! **Why each condition (E-SUP-10, measured on another slicer's exported supports):** 45° alone
//! takes the far side of a convex rib for a wall; the tangent plane alone takes a gently concave
//! surface around the contact for one. Together they find the wall of a concave corner, the side
//! of a groove or a pin hanging beside the contact, and leave out the surface the contact holds,
//! however it curves. A triangle with no normal (zero area) beyond the plane counts as a wall:
//! it is tested rather than ignored.
use super::shapes::{cross, dot, sub, unit, Part, V};

/// How far beyond a contact's tangent plane, towards the support, a surface must reach to be a
/// wall beside it, in mm: above the anchor tolerance and the rounding of a curved surface's
/// facets around the contact.
pub const WALL_BEYOND_MM: f64 = 0.05;

/// Whether `triangle` is a wall beside the contact held at `point` with unit outward `normal`,
/// for `part` (see the module).
pub(super) fn is_wall(point: V, normal: V, triangle: [V; 3], part: &Part) -> bool {
    let beyond = triangle
        .iter()
        .map(|&v| dot(sub(v, point), normal))
        .fold(f64::NEG_INFINITY, f64::max);
    if beyond <= WALL_BEYOND_MM {
        return false;
    }
    let Some(facing) = unit(cross(
        sub(triangle[1], triangle[0]),
        sub(triangle[2], triangle[0]),
    )) else {
        return true;
    };
    // cos 45°.
    if dot(facing, normal) >= std::f64::consts::FRAC_1_SQRT_2 {
        return false;
    }
    part.axis
        .iter()
        .any(|&end| dot(sub(end, triangle[0]), facing) + part.radius > 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::supports::shapes::frustum;

    #[test]
    fn a_wall_reaches_beyond_the_plane_turns_more_than_45_degrees_and_faces_the_part() {
        // A contact under a surface facing down at the origin, its cone 2 mm down.
        let (point, normal) = ([0.0, 0.0, 0.0], [0.0, 0.0, -1.0]);
        let part = frustum([0.0, 0.0, 0.1], 0.15, [0.0, 0.0, -2.0], 0.5, 16);
        // The wall of a concave corner 1 mm away, facing the contact: a wall.
        let wall = [[1.0, 0.0, 0.0], [1.0, 0.0, -3.0], [1.0, 1.0, -3.0]];
        let wall = if dot(
            unit(cross(sub(wall[1], wall[0]), sub(wall[2], wall[0]))).unwrap(),
            [-1.0, 0.0, 0.0],
        ) > 0.0
        {
            wall
        } else {
            [wall[0], wall[2], wall[1]]
        };
        assert!(is_wall(point, normal, wall, &part));
        // The same wall facing away: the part is behind it.
        assert!(!is_wall(point, normal, [wall[0], wall[2], wall[1]], &part));
        // A gently concave surface rising 30° beside the contact: beyond the plane, but within
        // 45° of the held surface.
        let (s, c) = (0.5, 0.75_f64.sqrt());
        let gentle = [[1.0, 0.0, 0.0], [1.0 + c, 0.0, -s], [1.0 + c, 1.0, -s]];
        let gentle = if dot(
            cross(sub(gentle[1], gentle[0]), sub(gentle[2], gentle[0])),
            normal,
        ) > 0.0
        {
            gentle
        } else {
            [gentle[0], gentle[2], gentle[1]]
        };
        assert!(!is_wall(point, normal, gentle, &part));
        // The far side of a convex rib turning 60° away: behind the plane.
        let rib = [[1.0, 0.0, 0.0], [1.5, 0.0, 0.866], [1.5, 1.0, 0.866]];
        assert!(!is_wall(point, normal, rib, &part));
        assert!(!is_wall(point, normal, [rib[0], rib[2], rib[1]], &part));
        // Reaching no more than 0.05 mm beyond the plane: no wall, whatever it faces.
        let low = [[1.0, 0.0, 0.0], [1.0, 0.0, -0.05], [1.0, 1.0, -0.05]];
        assert!(!is_wall(point, normal, low, &part));
        assert!(!is_wall(point, normal, [low[0], low[2], low[1]], &part));
    }
}
