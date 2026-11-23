//! Where a new support can go clear of the model (TD-108, owner 2026-10-29).
//!
//! A support placed under an overhang may find more of the model below its tip: a shaft
//! straight down would go through it. [`find_route`] looks for a way, in this order (owner):
//! 1. **Straight down:** the shaft from the tip's join to the plate is clear.
//! 2. **A tilted tip** (owner, 2026-11-04: often a few degrees free a support where a mini one
//!    would otherwise be needed): the tip turned from the normal, ring by ring every
//!    [`TILT_STEP_DEG`] up to the axis limit, its shaft straight down from its join. On each
//!    ring the [`TILT_DIRECTIONS`] directions are tried steepest first, so the result is
//!    deterministic and the shaft as short as the ring allows.
//! 3. **Aside:** the nearest vertical column beside the tip that is clear from a knee down to
//!    the plate, the tip pointing at the knee, the slanted part at most 45° from vertical and
//!    the tip within the axis limit from the surface. Columns are tried ring by ring, nearest
//!    first, in a fixed order of directions, so the result is deterministic.
//! 4. **On the model:** the foot stands on the model where the shaft straight down first meets
//!    a surface facing up, the shaft above it being clear and at most 45° from vertical from the
//!    tip's join to the foot's. The foot points up, towards its shaft, as far as the axis limit
//!    from the surface's mean normal allows ([`foot_axis`], TD-132): along the normal on a steep
//!    surface, the foot would leave almost level, and the ball at the shaft's bottom would hang
//!    below it and start in mid-air.
//! 5. Otherwise none: the support goes straight down, and its build reports the collision.
//!
//! **A tip kept along the normal (TD-164).** The Islands and Automatic tools keep every tip along
//! the surface's normal ([`RouteQuery::keep_normal`]): a tilted tip touches more than it is set
//! to, off its centre (E-SUP-06, E-SUP-09). Way 2 is then left out, and way 3 bends the support
//! at the tip's join instead: the nearest vertical column beside the join that is clear from a
//! knee down to the plate, the part from the join to the knee at most 45° from vertical
//! ([`Route::Bent`]). A tip whose cone along the normal meets the model away from its contact,
//! or whose join is closer to the model than the support's widest radius and the clearance, is
//! [`Route::Crowded`]: every way starts at that join, so none is looked for, and the tip is never
//! tilted instead. The planner may move it along the surface, away from what crowds it.
//!
//! A mini support (owner, 2026-10-30) has one way when placed by hand: [`find_mini_foot`] stands
//! it on the first surface straight below its tip's ball, when that surface faces up, and its rod
//! may touch the model on its way. Placed by the Islands or Automatic tool (owner, 2026-11-04,
//! TD-126), it must keep off the model: its foot is looked for in a fan of directions, ring by
//! ring every [`TILT_STEP_DEG`] from straight down to 45°, and taken where its rod keeps off the
//! model away from its balls; with none, no mini support goes there. The tools also take a foot
//! only where its ball rests on the model (TD-169): on a steep surface the ball, centred along
//! the surface's normal, would hang off it and its bottom would start in mid-air. And they keep
//! the mini support off the model (TD-170): its rod enters the model nowhere past its balls and
//! keeps [`MINI_ROD_GAP_MM`] from it past their margins, and neither ball reaches the model
//! deeper than its penetration.
//!
//! "Clear" is a capsule around each straight part of the path, the widest radius of the support
//! plus the document's clearance, kept from every triangle of the surface: exact distances
//! between segments and triangles over the candidates a uniform grid gives. It guides placing;
//! the support's build still proves its clearance part by part (TD-062).
//!
//! **The tip's direction (TD-129).** A tip that points along the surface's normal is checked
//! that way. A tip that points towards its shaft ([`RouteQuery::towards_shaft`]) is built
//! pointing at the next point of its path, so ways 1 and 4, whose shaft goes straight down below
//! the tip, check a tip pointing straight down, within the axis limit; ways 2 and 3 give the
//! tip's direction themselves, so both kinds of tip check the same thing there.
//!
//! **The tip (TD-124).** The tip follows the surface's mean normal at its contact, as the build
//! measures it, and every way also needs the tip's cone to keep off the model away from its
//! contact (its reach, as the build checks it): a tip that would cross the surface beside it
//! straight down leans aside instead, and with no way at all the support is built straight and
//! reports it.
#![allow(clippy::many_single_char_names)] // Geometric notation: points, segments, normals.
use super::{
    index::{
        ball_rests, normal, point_keeps, proven_clear, segment_keeps, segment_meets_triangle, Grid,
    },
    shapes::{add, dot, frame, frustum, length, scale, sphere, sub, unit, V},
    tip::{surface_normal, TipCone},
    walls::is_wall,
};
use crate::{
    cancellation::{Cancellation, Cancelled},
    geometry::{plan_length, sin_cos_degrees, tan_degrees},
    mesh::TriangleMesh,
};

/// The steepest slanted part a route plans, in degrees from vertical: under the 45° the build
/// accepts, by a margin, so rounding never makes a part planned at the limit read as steeper.
const MAX_SLANT_DEG: f64 = 44.5;
/// How far a knee stays beyond the tip's join, in mm.
const PAST_JOIN_MM: f64 = 0.5;
/// How high above the plate (or the raft) a knee stays, in mm: room for the pad and its cone.
const KNEE_FLOOR_MM: f64 = 2.0;
/// The rings of columns tried aside: every this many mm, up to the farthest.
const RING_STEP_MM: f64 = 0.5;
/// The farthest column tried aside, in mm.
const MAX_ASIDE_MM: f64 = 15.0;
/// Directions tried on each ring.
const DIRECTIONS: u32 = 16;
/// The steps a tilted tip turns by from the normal, in degrees.
pub const TILT_STEP_DEG: f64 = 5.0;
/// Directions tried on each ring of tilted tips.
pub const TILT_DIRECTIONS: u32 = 16;
/// The longest piece a segment is walked in to find what it first meets, in mm.
pub const HIT_PIECE_MM: f64 = 2.0;
/// Rounding of the knee's height, in mm: far below a print's resolution.
const ROUNDING_MM: f64 = 1e-9;

/// A surface's grid, built once and kept with the surface: the routes of its supports are found
/// against it while they are placed.
pub struct SurfaceGrid {
    grid: Grid,
    triangles: usize,
}

impl SurfaceGrid {
    /// Grid every triangle of `mesh` (the placed model, or the hollowed shell).
    ///
    /// # Errors
    /// Cancellation.
    pub fn build(mesh: &TriangleMesh, cancel: &mut Cancellation<'_>) -> Result<Self, Cancelled> {
        Ok(Self {
            grid: Grid::build(mesh, cancel)?,
            triangles: mesh.triangles().len(),
        })
    }

    /// Whether the grid was built from a mesh with as many triangles as `mesh`.
    #[must_use]
    pub fn fits(&self, mesh: &TriangleMesh) -> bool {
        self.triangles == mesh.triangles().len()
    }

    fn corners(mesh: &TriangleMesh, t: u32) -> [V; 3] {
        mesh.triangles()[t as usize].map(|v| {
            let p = mesh.vertices()[v as usize];
            [p.x, p.y, p.z]
        })
    }

    /// Whether the capsule of `radius` around the segment `[p, q]` keeps clear of `mesh`: a
    /// route's parts, or a branch from its tip to the shaft it joins.
    #[must_use]
    pub fn clear(&self, mesh: &TriangleMesh, p: [f64; 3], q: [f64; 3], radius: f64) -> bool {
        let lo = [0, 1, 2].map(|k| p[k].min(q[k]) - radius);
        let hi = [0, 1, 2].map(|k| p[k].max(q[k]) + radius);
        let triangles = self
            .grid
            .candidates(mesh, lo, hi)
            .into_iter()
            .map(|t| Self::corners(mesh, t));
        segment_keeps(triangles, p, q, radius)
    }

    /// The surface's outward normal at `tip` (TD-124): the mean over the triangles of `mesh`
    /// within `radius` that face the `anchor` triangle's way.
    #[must_use]
    pub fn surface_normal(&self, mesh: &TriangleMesh, tip: V, anchor: V, radius: f64) -> V {
        let reach = [radius; 3];
        let near = self
            .grid
            .candidates(mesh, sub(tip, reach), add(tip, reach))
            .into_iter()
            .map(|t| Self::corners(mesh, t));
        surface_normal(near, tip, anchor, radius)
    }

    /// Whether `cone`, at `tip` along the unit `axis` on a surface of outward `normal` there,
    /// keeps off `mesh` away from its contact (TD-124): its reach meets no triangle.
    #[must_use]
    pub fn reach_clear(
        &self,
        mesh: &TriangleMesh,
        cone: &TipCone,
        tip: V,
        axis: V,
        normal: V,
    ) -> bool {
        let Some(reach) = cone.reach(tip, axis, normal) else {
            return true;
        };
        let [lo, hi] = reach.bounds();
        self.grid
            .candidates(mesh, lo, hi)
            .into_iter()
            .all(|t| !reach.meets(Self::corners(mesh, t)))
    }

    /// Whether the contact part of `cone`, at `tip` along the unit `axis` on a surface of unit
    /// outward `normal` there, keeps `clearance` from the walls beside it, as a document with
    /// `contact_clearance` proves it (TD-164): its cone, and its ball for a sphere contact.
    #[must_use]
    #[allow(clippy::too_many_arguments)] // The contact, its surface and its clearance.
    pub fn contact_clear(
        &self,
        mesh: &TriangleMesh,
        cone: &TipCone,
        tip: V,
        axis: V,
        normal: V,
        clearance: f64,
    ) -> bool {
        if cone.from_mm >= cone.length_mm {
            // No cone to build: the build refuses the support itself.
            return true;
        }
        let start = add(tip, scale(axis, cone.from_mm));
        let join = add(tip, scale(axis, cone.length_mm));
        let mut parts = vec![frustum(
            start,
            cone.radii_mm[0],
            join,
            cone.radii_mm[1],
            cone.sides,
        )];
        if let Some(ball) = cone.ball_mm {
            parts.push(sphere(start, ball, cone.sides));
        }
        parts.iter().all(|part| {
            let lo = [0, 1, 2].map(|k| {
                part.vertices
                    .iter()
                    .map(|v| v[k])
                    .fold(f64::INFINITY, f64::min)
                    - clearance
            });
            let hi = [0, 1, 2].map(|k| {
                part.vertices
                    .iter()
                    .map(|v| v[k])
                    .fold(f64::NEG_INFINITY, f64::max)
                    + clearance
            });
            self.grid.candidates(mesh, lo, hi).into_iter().all(|t| {
                let triangle = Self::corners(mesh, t);
                !is_wall(tip, normal, triangle, part) || proven_clear(part, triangle, clearance)
            })
        })
    }

    /// Where the segment straight up from `from` to the height `to_z` first meets `mesh`: the
    /// point, the outward normal of the triangle met there and its index in the mesh. Under an
    /// island, its underside; in a hollowed shell, a cavity triangle when the island starts on
    /// the cavity's ceiling.
    #[must_use]
    pub fn first_above(
        &self,
        mesh: &TriangleMesh,
        from: [f64; 3],
        to_z: f64,
    ) -> Option<([f64; 3], [f64; 3], u32)> {
        let to = [from[0], from[1], to_z];
        let mut best: Option<(V, V, u32)> = None;
        for t in self.grid.candidates(mesh, from, to) {
            let tri = Self::corners(mesh, t);
            if !segment_meets_triangle(from, to, tri) {
                continue;
            }
            let Some(n) = normal(tri) else {
                continue;
            };
            if n[2].abs() < 1e-12 {
                continue;
            }
            let z = from[2] - dot(sub(from, tri[0]), n) / n[2];
            if best.is_none_or(|(p, _, _)| z < p[2]) {
                best = Some(([from[0], from[1], z], n, t));
            }
        }
        best
    }

    /// Where the segment from `from` to `to` first meets `mesh`: the point, and the outward
    /// normal of the triangle met there. The segment is walked in pieces of at most
    /// [`HIT_PIECE_MM`], nearest first, so a long slanted one never gathers the triangles of its
    /// whole bounding box.
    #[must_use]
    pub fn first_hit(&self, mesh: &TriangleMesh, from: V, to: V) -> Option<(V, V)> {
        let total = length(sub(to, from));
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // A few hundred.
        let pieces = (total / HIT_PIECE_MM).ceil().max(1.0) as u32;
        (0..pieces).find_map(|k| {
            let at = |i: u32| add(from, scale(sub(to, from), f64::from(i) / f64::from(pieces)));
            self.first_hit_in(mesh, at(k), at(k + 1))
        })
    }

    /// [`Self::first_hit`] over one piece, its triangles gathered at once.
    fn first_hit_in(&self, mesh: &TriangleMesh, from: V, to: V) -> Option<(V, V)> {
        let along = sub(to, from);
        let lo = [0, 1, 2].map(|k| from[k].min(to[k]));
        let hi = [0, 1, 2].map(|k| from[k].max(to[k]));
        let mut best: Option<(f64, V)> = None;
        for t in self.grid.candidates(mesh, lo, hi) {
            let tri = Self::corners(mesh, t);
            if !segment_meets_triangle(from, to, tri) {
                continue;
            }
            let Some(n) = normal(tri) else {
                continue;
            };
            // The segment meets the triangle's plane where it meets the triangle.
            let facing = dot(n, along);
            if facing.abs() < 1e-12 {
                continue;
            }
            let at = (dot(n, sub(tri[0], from)) / facing).clamp(0.0, 1.0);
            if best.is_none_or(|(b, _)| at < b) {
                best = Some((at, n));
            }
        }
        best.map(|(at, n)| (add(from, scale(along, at)), n))
    }

    /// Whether a ball of `radius` centred at `centre` rests on `mesh` (TD-169): its lowest point
    /// lies inside the model, so its first layer lies on the model's (see
    /// [`super::index::ball_rests`]).
    #[must_use]
    pub fn ball_rests(&self, mesh: &TriangleMesh, centre: V, radius: f64) -> bool {
        let lowest = [centre[0], centre[1], centre[2] - radius];
        let triangles = self
            .grid
            .candidates(mesh, lowest, centre)
            .into_iter()
            .map(|t| Self::corners(mesh, t));
        ball_rests(triangles, centre, radius)
    }

    /// Whether a mini support's rod of `radius` along `path` (its balls' centres at the ends, its
    /// bends between) keeps off `mesh` away from its balls: farther than `radius` from it past
    /// [`MINI_BALL_MARGIN_MM`] beyond either ball, along the path (TD-126).
    ///
    /// With `keep_off` (TD-170, the tools), stricter: the rod enters the model nowhere past its
    /// balls' radius, and keeps [`MINI_ROD_GAP_MM`] from it past their margins.
    #[must_use]
    pub fn rod_clear(&self, mesh: &TriangleMesh, path: &[V], radius: f64, keep_off: bool) -> bool {
        rod_keeps_off(path, radius, keep_off, |p, q, distance| {
            self.clear(mesh, p, q, distance)
        })
    }

    /// Whether every triangle of `mesh` lies at least `distance` from `p`: a mini support's ball
    /// centred there reaches no surface deeper than its radius less `distance` (TD-170).
    #[must_use]
    pub fn point_clear(&self, mesh: &TriangleMesh, p: V, distance: f64) -> bool {
        let reach = [distance; 3];
        let triangles = self
            .grid
            .candidates(mesh, sub(p, reach), add(p, reach))
            .into_iter()
            .map(|t| Self::corners(mesh, t));
        point_keeps(triangles, p, distance)
    }

    /// Where the segment straight down from `from` to the height `to_z` first meets `mesh`,
    /// with the outward normal of the triangle met there.
    fn first_below(&self, mesh: &TriangleMesh, from: V, to_z: f64) -> Option<(V, V)> {
        let to = [from[0], from[1], to_z];
        let mut best: Option<(V, V)> = None;
        for t in self.grid.candidates(mesh, to, from) {
            let tri = Self::corners(mesh, t);
            if !segment_meets_triangle(from, to, tri) {
                continue;
            }
            let Some(n) = normal(tri) else {
                continue;
            };
            // The vertical line meets the triangle's plane where it meets the triangle.
            if n[2].abs() < 1e-12 {
                continue;
            }
            let z = from[2] - dot(sub(from, tri[0]), n) / n[2];
            if best.is_none_or(|(p, _)| z > p[2]) {
                best = Some(([from[0], from[1], z], n));
            }
        }
        best
    }
}

/// Whether a mini support's rod of `radius` along `path` (its balls' centres at the ends, its
/// bends between) keeps off the model, `keeps(p, q, distance)` saying whether the segment
/// `[p, q]` keeps `distance` from it: see [`SurfaceGrid::rod_clear`]. One rule for the tools'
/// search and the build's warning (TD-126, TD-170).
pub(super) fn rod_keeps_off(
    path: &[V],
    radius: f64,
    keep_off: bool,
    keeps: impl Fn(V, V, f64) -> bool,
) -> bool {
    let lengths: Vec<f64> = path.windows(2).map(|w| length(sub(w[1], w[0]))).collect();
    let total: f64 = lengths.iter().sum();
    // Whether the rod, past `away` from either end along the path, keeps `distance`.
    let past = |away: f64, distance: f64| {
        let mut run = 0.0;
        path.windows(2).zip(&lengths).all(|(w, &l)| {
            let (start, end) = (run, run + l);
            run = end;
            let (from, to) = (start.max(away), end.min(total - away));
            if to <= from || l <= 0.0 {
                return true;
            }
            let at = |t: f64| add(w[0], scale(sub(w[1], w[0]), (t - start) / l));
            keeps(at(from), at(to), distance)
        })
    };
    if !keep_off {
        return past(radius + MINI_BALL_MARGIN_MM, radius);
    }
    past(radius, radius) && past(radius + MINI_BALL_MARGIN_MM, radius + MINI_ROD_GAP_MM)
}

/// What a route is found for: a support's tip on the surface and its sizes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RouteQuery {
    /// The contact on the surface, in world mm.
    pub tip_mm: [f64; 3],
    /// The outward normal of the triangle the contact is on: the surface's normal is the mean
    /// around it (TD-124).
    pub normal: [f64; 3],
    /// The tip's cone: its join is its length along the tip's axis, and away from the contact it
    /// must not meet the model (TD-124).
    pub tip: TipCone,
    /// The widest radius of the support's parts along its path plus the clearance, in mm.
    pub radius_mm: f64,
    /// The height the support stands on: the plate, or the raft's top, in mm.
    pub floor_mm: f64,
    /// How far a model foot reaches out of the surface, in mm.
    pub foot_length_mm: f64,
    /// A model foot's contact radius, in mm: the surface's mean normal there is taken within
    /// it, as the build takes it (TD-124).
    pub foot_radius_mm: f64,
    /// How far the tip may turn from the surface's normal, in degrees.
    pub max_axis_deviation_deg: f64,
    /// The tip points towards its shaft (`AxisSpec::Straight`), not along the surface's normal:
    /// straight down when the shaft goes straight down below it, as the build makes it (TD-129).
    pub towards_shaft: bool,
    /// The tip stays along the surface's normal (the Islands and Automatic tools, TD-164): no
    /// tilted tip, and the support bends at the tip's join to go aside. Not with
    /// `towards_shaft`.
    pub keep_normal: bool,
    /// The clearance the contact part keeps from the walls beside it, in mm, when the document
    /// applies it (`contact_clearance`, TD-164): a tip that would not is no way.
    pub contact_clearance_mm: Option<f64>,
}

/// The way a support goes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Route {
    /// Straight down from the tip's join.
    Straight,
    /// The tip along this unit direction, turned from the normal within the axis limit; the
    /// shaft goes straight down from its join.
    Tilted([f64; 3]),
    /// The tip points at this knee, in world mm; the shaft goes straight down from it.
    Aside([f64; 3]),
    /// The tip along the normal; the shaft bends at the tip's join towards this knee, in world
    /// mm, at most 45° from vertical, and goes straight down from it (TD-164).
    Bent([f64; 3]),
    /// The foot stands on the model at this point, with this outward normal, along this axis.
    OnModel {
        /// The foot's contact, in world mm.
        point_mm: [f64; 3],
        /// The surface's outward normal there.
        normal: [f64; 3],
        /// The foot's unit axis, out of the surface towards its shaft (TD-132).
        axis: [f64; 3],
    },
    /// The tip kept along the normal (TD-164) meets the model beside its contact, or its join
    /// comes closer to the model than the support's widest radius and the clearance: no way
    /// starts there. Placed by hand it never arises; built anyway, the support goes straight
    /// down, as with [`Route::None`].
    Crowded,
    /// No way found: the support goes straight down, through the model.
    None,
}

/// The way a support with `query`'s tip can go clear of `mesh` (see the module).
#[must_use]
pub fn find_route(mesh: &TriangleMesh, grid: &SurfaceGrid, query: &RouteQuery) -> Route {
    let Some(anchor) = unit(query.normal) else {
        return Route::None;
    };
    let tip = query.tip_mm;
    let n = grid.surface_normal(mesh, tip, anchor, query.tip.footprint_mm);
    let r = query.radius_mm;
    let floor = query.floor_mm;
    let cos_limit = sin_cos_degrees(query.max_axis_deviation_deg).1;
    // The tip along `axis` keeps off the model away from its contact, and the clearance from
    // the walls beside it when the document asks.
    let tip_clear = |axis: V| {
        grid.reach_clear(mesh, &query.tip, tip, axis, n)
            && query
                .contact_clearance_mm
                .is_none_or(|c| grid.contact_clear(mesh, &query.tip, tip, axis, n, c))
    };
    // Ways 1 and 4 build the tip along the normal, or straight down when it points towards a
    // shaft below it: that tip must be within the axis limit and keep off the model (TD-129).
    let axis = if query.towards_shaft {
        [0.0, 0.0, -1.0]
    } else {
        n
    };
    let join = add(tip, scale(axis, query.tip.length_mm));
    let axis_clear = dot(axis, n) >= cos_limit && tip_clear(axis);
    // 1. Straight down from the join.
    if axis_clear && (join[2] <= floor || grid.clear(mesh, join, [join[0], join[1], floor], r)) {
        return Route::Straight;
    }
    let tan_slant = tan_degrees(MAX_SLANT_DEG);
    if query.keep_normal {
        // The tip along the normal or not at all (TD-164); every way starts at its join.
        if !axis_clear || !grid.clear(mesh, join, join, r) {
            return Route::Crowded;
        }
        if let Some(knee) = bent(mesh, grid, query, join, tan_slant) {
            return Route::Bent(knee);
        }
        return on_model(mesh, grid, query, join, tan_slant).unwrap_or(Route::None);
    }
    // 2. A tilted tip: ring by ring from the normal, steepest first on each.
    if let Some(axis) = tilted(mesh, grid, query, n, &tip_clear) {
        return Route::Tilted(axis);
    }
    // 3. Aside: ring by ring, nearest first.
    let reach = query.tip.length_mm + PAST_JOIN_MM;
    let rings = (MAX_ASIDE_MM / RING_STEP_MM).floor();
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // 30 rings.
    for ring in 1..=rings as u32 {
        let d = f64::from(ring) * RING_STEP_MM;
        // As high as 45° allows, and past the tip's join.
        let drop = (d / tan_slant).max((reach * reach - d * d).max(0.0).sqrt());
        // A knee exactly at the floor stays: the drop rounds just above or below it.
        let z = tip[2] - drop;
        if z < floor + KNEE_FLOOR_MM - ROUNDING_MM {
            break;
        }
        let z = z.max(floor + KNEE_FLOOR_MM);
        for k in 0..DIRECTIONS {
            let (sin_a, cos_a) = sin_cos_degrees(360.0 * f64::from(k) / f64::from(DIRECTIONS));
            let knee = [tip[0] + d * cos_a, tip[1] + d * sin_a, z];
            let Some(towards) = unit(sub(knee, tip)) else {
                continue;
            };
            if dot(towards, n) < cos_limit {
                continue;
            }
            // From the tip's join: the tip itself touches the surface by design, and keeps off
            // it elsewhere.
            let start = add(tip, scale(towards, query.tip.length_mm));
            if grid.clear(mesh, start, knee, r)
                && grid.clear(mesh, knee, [knee[0], knee[1], floor], r)
                && tip_clear(towards)
            {
                return Route::Aside(knee);
            }
        }
    }
    // 4. On the model.
    if !axis_clear {
        return Route::None;
    }
    on_model(mesh, grid, query, join, tan_slant).unwrap_or(Route::None)
}

/// Way 4: the foot on the model where the shaft straight down from the tip's `join` first meets
/// a surface facing up, the shaft from `join` to the foot's join clear and leaning no more than
/// `tan_slant` (see the module).
fn on_model(
    mesh: &TriangleMesh,
    grid: &SurfaceGrid,
    query: &RouteQuery,
    join: V,
    tan_slant: f64,
) -> Option<Route> {
    let (point, facing) = grid.first_below(mesh, join, query.floor_mm)?;
    let facing = unit(facing)?;
    // The foot towards its shaft (TD-132), and the way judged with it.
    let mean = grid.surface_normal(mesh, point, facing, query.foot_radius_mm);
    let foot = foot_axis(mean, query.max_axis_deviation_deg);
    let foot_join = add(point, scale(foot, query.foot_length_mm));
    let aside = plan_length(join[0] - foot_join[0], join[1] - foot_join[1]);
    (facing[2] > 0.0
        && foot_join[2] < join[2]
        && aside <= (join[2] - foot_join[2]) * tan_slant
        && grid.clear(mesh, join, foot_join, query.radius_mm))
    .then_some(Route::OnModel {
        point_mm: point,
        normal: facing,
        axis: foot,
    })
}

/// Way 3 for a tip kept along the normal (TD-164): the knee of the nearest vertical column beside
/// the tip's `join` that is clear from the join to the knee, leaning no more than `tan_slant`,
/// and from the knee down to the floor. Ring by ring every [`RING_STEP_MM`] up to
/// [`MAX_ASIDE_MM`], each knee as high as the slant allows, in a fixed order of directions.
fn bent(
    mesh: &TriangleMesh,
    grid: &SurfaceGrid,
    query: &RouteQuery,
    join: V,
    tan_slant: f64,
) -> Option<V> {
    let floor = query.floor_mm;
    let rings = (MAX_ASIDE_MM / RING_STEP_MM).floor();
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // 30 rings.
    for ring in 1..=rings as u32 {
        let d = f64::from(ring) * RING_STEP_MM;
        // A knee exactly at the floor stays: the drop rounds just above or below it.
        let z = join[2] - d / tan_slant;
        if z < floor + KNEE_FLOOR_MM - ROUNDING_MM {
            return None;
        }
        let z = z.max(floor + KNEE_FLOOR_MM);
        for k in 0..DIRECTIONS {
            let (sin_a, cos_a) = sin_cos_degrees(360.0 * f64::from(k) / f64::from(DIRECTIONS));
            let knee = [join[0] + d * cos_a, join[1] + d * sin_a, z];
            if grid.clear(mesh, join, knee, query.radius_mm)
                && grid.clear(mesh, knee, [knee[0], knee[1], floor], query.radius_mm)
            {
                return Some(knee);
            }
        }
    }
    None
}

/// How far short of the axis limit a foot's axis stays, in degrees: the build measures the
/// deviation from the same mean normal, and rounding must not make a foot at the limit read as
/// beyond it.
const FOOT_LIMIT_MARGIN_DEG: f64 = 0.5;

/// A model foot's unit axis on a surface of unit mean outward normal `normal` (TD-132):
/// straight up when that is within `max_deviation_deg` of the normal, otherwise the normal
/// turned towards straight up by that much less half a degree, a margin for rounding. The foot
/// then leaves the surface as steeply as allowed, so the shaft's bottom rests on it rather than
/// hanging beside it. A surface facing straight down has no way up: its normal is kept.
#[must_use]
pub fn foot_axis(normal: [f64; 3], max_deviation_deg: f64) -> [f64; 3] {
    let up = [0.0, 0.0, 1.0];
    let (sin, cos) = sin_cos_degrees((max_deviation_deg - FOOT_LIMIT_MARGIN_DEG).max(0.0));
    if dot(up, normal) >= cos {
        return up;
    }
    // The direction in the plane of the normal and straight up, square to the normal.
    let Some(towards) = unit(sub(up, scale(normal, dot(up, normal)))) else {
        return normal;
    };
    add(scale(normal, cos), scale(towards, sin))
}

/// The first tilted tip of `query` whose join is below the tip and whose reach and shaft straight
/// down are clear, on a surface of unit outward normal `n` (see the module's way 2).
fn tilted(
    mesh: &TriangleMesh,
    grid: &SurfaceGrid,
    query: &RouteQuery,
    n: V,
    tip_clear: &dyn Fn(V) -> bool,
) -> Option<V> {
    let tip = query.tip_mm;
    let (u, v) = frame(n);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // At most 18 rings.
    let rings = (query.max_axis_deviation_deg / TILT_STEP_DEG).floor() as u32;
    for ring in 1..=rings {
        let (sin_t, cos_t) = sin_cos_degrees(f64::from(ring) * TILT_STEP_DEG);
        let mut axes: Vec<V> = (0..TILT_DIRECTIONS)
            .map(|k| {
                let (sin_a, cos_a) =
                    sin_cos_degrees(360.0 * f64::from(k) / f64::from(TILT_DIRECTIONS));
                std::array::from_fn(|c| cos_t * n[c] + sin_t * (cos_a * u[c] + sin_a * v[c]))
            })
            .collect();
        // Steepest first: the shaft below is the shortest the ring allows. Ties keep their order.
        axes.sort_by(|a, b| a[2].total_cmp(&b[2]));
        for axis in axes {
            let join = add(tip, scale(axis, query.tip.length_mm));
            if join[2] >= tip[2] || join[2] <= query.floor_mm {
                continue;
            }
            let base = [join[0], join[1], query.floor_mm];
            if grid.clear(mesh, join, base, query.radius_mm) && tip_clear(axis) {
                return Some(axis);
            }
        }
    }
    None
}

/// What a mini support's foot is found for (owner, 2026-10-30): its tip on the surface and its
/// sizes, in millimetres.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MiniQuery {
    /// The contact on the surface, in world mm.
    pub tip_mm: [f64; 3],
    /// The surface's outward normal there.
    pub normal: [f64; 3],
    /// The mini support's diameter.
    pub diameter_mm: f64,
    /// How deep its ends reach below the surface.
    pub penetration_mm: f64,
    /// The height below which nothing is looked for: the plate, or the raft's top.
    pub floor_mm: f64,
    /// Placed by the Islands or Automatic tool: its rod must keep off the model away from its
    /// balls, and a fan of directions is tried (TD-126). By hand, straight down only, touching
    /// allowed.
    pub clear: bool,
    /// The longest its rod may be, between its balls' centres, in mm (TD-167): a foot farther
    /// away is no foot in that direction. `None`: no limit, as by hand.
    pub max_length_mm: Option<f64>,
    /// The least height its rod may rise, from its foot's ball's centre to its tip's, in mm
    /// (TD-167, owner 2026-11-20): a foot nearer below is no foot in that direction. `None`: no
    /// limit, as by hand.
    pub min_height_mm: Option<f64>,
    /// Its foot's ball must rest on the model (TD-169, owner 2026-11-23): its lowest point
    /// inside the model, so it never starts in mid-air beside a steep surface. A foot whose
    /// ball would hang is no foot in that direction. The Islands and Automatic tools ask it; by
    /// hand, `false`: the build reports a hanging foot instead.
    pub foot_rests: bool,
    /// It keeps off the model (TD-170, owner 2026-11-23): its rod enters the model nowhere past
    /// its balls and keeps [`MINI_ROD_GAP_MM`] from it past their margins (see
    /// [`SurfaceGrid::rod_clear`]), and neither ball reaches the model deeper than its
    /// penetration, within [`MINI_BALL_DEPTH_TOLERANCE_MM`]. The tools ask it, with `clear`;
    /// by hand, `false`: it may touch the model (owner, 2026-10-30).
    pub keep_off: bool,
}

/// How far from a mini support's ball centres its rod may touch the model, beyond its radius, in
/// mm: where the ball sits on the surface, the rod leaving it still grazes it (TD-126). Placed by
/// the tools, it still never enters the model there (TD-170).
pub const MINI_BALL_MARGIN_MM: f64 = 0.1;

/// The least distance the rod of a mini support placed by the tools keeps from the model past
/// its balls' margins, in mm (TD-170, owner 2026-11-23): touching it, the rod would print fused
/// to the model's surface along its way. About one or two pixels of a resin printer.
pub const MINI_ROD_GAP_MM: f64 = 0.05;

/// How much deeper than its penetration a ball of a mini support placed by the tools may reach
/// the model, in mm (TD-170): the facets of a curved surface under it, and rounding. A ball in a
/// crease, or beside a wall, reaches deeper and is not placed there.
pub const MINI_BALL_DEPTH_TOLERANCE_MM: f64 = 0.01;

/// Where a mini support with `query`'s tip stands on the model: the point and the outward normal
/// of the first surface straight below its tip's ball, if that surface faces up and the rod
/// between them (the ball centres, as the build places them along the normals) is at most 45°
/// from vertical, within `query`'s maximum length and minimum height when it has them
/// (TD-167), and, when `query` asks it, with the foot's ball resting on the model (TD-169) and
/// the support kept off the model (TD-170). By hand, the rod may touch the model on its way
/// (owner, 2026-10-30). `None` without one: a mini support then has nowhere to stand.
#[must_use]
pub fn find_mini_foot(
    mesh: &TriangleMesh,
    grid: &SurfaceGrid,
    query: &MiniQuery,
) -> Option<([f64; 3], [f64; 3])> {
    let r = query.diameter_mm / 2.0;
    let out = r - query.penetration_mm;
    // The balls sit along the surface's mean normals, as the build places them (TD-124).
    let n = grid.surface_normal(mesh, query.tip_mm, unit(query.normal)?, r);
    let top = add(query.tip_mm, scale(n, out));
    // Kept off the model (TD-170): a ball reaches no surface deeper than its penetration.
    let ball_clear = |centre: V| {
        !query.keep_off
            || grid.point_clear(mesh, centre, (out - MINI_BALL_DEPTH_TOLERANCE_MM).max(0.0))
    };
    if !ball_clear(top) {
        return None;
    }
    // A foot met going down from the top's ball along `down`: its point, its mean normal and
    // its ball's centre, when it faces up and the rod is at most 45° from vertical.
    let foot_along = |down: V| -> Option<(V, V, V)> {
        let mut reach = (top[2] - query.floor_mm) / -down[2];
        if let Some(longest) = query.max_length_mm {
            // The foot's ball sits `out` off the surface: a surface a little farther may still
            // give a rod within the limit.
            reach = reach.min(longest + out.abs());
        }
        let (point, facet) = grid.first_hit(mesh, top, add(top, scale(down, reach)))?;
        let facing = grid.surface_normal(mesh, point, unit(facet)?, r);
        if facing[2] <= 0.0 {
            return None;
        }
        let foot = add(point, scale(facing, out));
        let rise = top[2] - foot[2];
        let aside = plan_length(top[0] - foot[0], top[1] - foot[1]);
        let within = query
            .max_length_mm
            .is_none_or(|longest| length(sub(foot, top)) <= longest)
            && query.min_height_mm.is_none_or(|lowest| rise >= lowest);
        (rise > 0.0
            && aside <= rise * tan_degrees(MAX_SLANT_DEG)
            && within
            && (!query.foot_rests || grid.ball_rests(mesh, foot, r))
            && ball_clear(foot))
        .then_some((point, facing, foot))
    };
    if !query.clear {
        return foot_along([0.0, 0.0, -1.0]).map(|(point, facing, _)| (point, facing));
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // Ten rings.
    let rings = (MAX_SLANT_DEG / TILT_STEP_DEG).floor() as u32;
    for ring in 0..=rings {
        let (sin_t, cos_t) = sin_cos_degrees(f64::from(ring) * TILT_STEP_DEG);
        let directions = if ring == 0 { 1 } else { TILT_DIRECTIONS };
        for k in 0..directions {
            let (sin_a, cos_a) = sin_cos_degrees(360.0 * f64::from(k) / f64::from(TILT_DIRECTIONS));
            let down = [sin_t * cos_a, sin_t * sin_a, -cos_t];
            let Some((point, facing, foot)) = foot_along(down) else {
                continue;
            };
            if grid.rod_clear(mesh, &[top, foot], r, query.keep_off) {
                return Some((point, facing));
            }
        }
    }
    None
}
