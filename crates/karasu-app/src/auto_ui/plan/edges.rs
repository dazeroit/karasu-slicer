//! An underside's outline found on its probe grid, for the tips that hold it (TD-136).
//!
//! **Why.** Two test prints (`docs/research/SUPPORT-PRESETS-RESEARCH.md`, E-SUP-02 and
//! E-SUP-03) held flat tiles on the same contacts: with the outer tips 1.25 mm in from the edges
//! the tiles curled at their corners and half of them fell, with the outer tips 0.4 mm in and at
//! the corners every one held flat. The film peels a layer from its outline inwards, so the
//! outline takes tips of its own; the inside is covered as before.
//!
//! **The outline** lies between a grid point the plan wants a tip under and a neighbour it does
//! not (one step along X or Y). It is found by bisection along that step, to within a step's
//! 1/64. It counts as an **edge** only where the surface ends there: nothing is found beyond it,
//! the material is cut away (a drain hole), the surface beyond stands more than
//! [`EDGE_STEP_MM`] higher or lower, or it turns by more than [`EDGE_TURN_DEG`]. Where a smooth
//! surface only grows steeper than the critical angle there is no edge, and no tip for it.
//!
//! **Edge tips** stand [`super::EDGE_INSET_MM`] in from an edge along the grid's X or Y, from
//! each grid point next to it, and as far from an edge across that is nearer (so nearer an edge
//! the grid crosses aslant: 0.4 mm along X is 0.28 mm from an edge at 45°);
//! **corner tips** that far in from both edges of a grid point with an edge along X and one
//! along Y. They are thinned as every candidate is (see [`super::plan()`]), corners first.
//!
//! **A leading edge**, an edge the underside rises away from (more than [`LEADING_DEG`]), prints
//! first as a thin strip and bent on the tilted test tiles though tips stood 0.2 mm in from it
//! (owner, 2026-11-06: an edge standing on supports is also held on one side, and the tips go
//! close to it, as supports placed by hand are). Its edge tips stand the contact's radius at
//! the surface in from it, measured along the slope, so that their marks reach the edge. Beyond
//! it, the face beside the edge is probed outward every [`SIDE_STEP_MM`] up to
//! [`SIDE_REACH_MM`]: where it faces down, rising no more than [`SIDE_RISE_MM`], no steeper than
//! [`SIDE_STEEPEST_DEG`], a **side tip** goes the contact's radius up the face from the edge,
//! when the face is at least the contact's width. Those faces are steeper than the critical
//! angle, so the plan would not otherwise hold them.
//!
//! Everything here follows the grid's fixed order: it is deterministic.
use super::EDGE_INSET_MM;

/// Bisections that find an edge between two grid points: a step's 1/64.
const EDGE_BISECTIONS: u32 = 6;
/// A surface beyond the outline this much higher or lower than the underside ends it, in mm.
pub const EDGE_STEP_MM: f64 = 0.5;
/// A surface beyond the outline turned this much from the underside ends it, in degrees.
pub const EDGE_TURN_DEG: f64 = 30.0;
/// An edge the underside rises away from by more than this is a leading edge, in degrees.
pub const LEADING_DEG: f64 = 10.0;
/// The face beside a leading edge is probed every this many mm outward...
pub const SIDE_STEP_MM: f64 = 0.05;
/// ... up to this far beyond the edge, in mm...
pub const SIDE_REACH_MM: f64 = 2.0;
/// ... where it stands no higher than this above the edge, in mm...
pub const SIDE_RISE_MM: f64 = 3.0;
/// ... and faces down no steeper than this from straight down, in degrees.
pub const SIDE_STEEPEST_DEG: f64 = 75.0;

/// What a vertical line up meets first: the point, its outward normal and whether it lies on a
/// hollowed model's cavity.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Seen {
    pub point: [f64; 3],
    pub normal: [f64; 3],
    pub cavity: bool,
    /// A drain hole removes the material there.
    pub cut: bool,
}

/// How an underside is probed: what a vertical line meets, and which points want a tip.
pub trait Probe {
    /// What a vertical line up from `(x, y, from_z)` to `to_z` meets first.
    fn between(&self, x: f64, y: f64, from_z: f64, to_z: f64) -> Option<Seen>;
    /// What the line up at `(x, y)` within the region's heights meets first.
    fn look(&self, x: f64, y: f64) -> Option<Seen>;
    /// Whether `seen` wants a tip.
    fn wanted(&self, seen: &Seen) -> bool;
    /// Whether the side of the cavity's surface at `seen` is one the plan covers.
    fn holds(&self, seen: &Seen) -> bool;
}

/// How a tip's place was found: corners and edges are taken before the inside.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Rank {
    Corner,
    Edge,
    Inside,
}

/// The grid an underside is probed on: `nx` × `ny` points `step` apart from `start`, with what
/// each saw and whether it wants a tip.
pub struct Grid {
    pub start: [f64; 2],
    pub step: f64,
    pub nx: usize,
    pub ny: usize,
    pub seen: Vec<Option<Seen>>,
    pub wanted: Vec<bool>,
}

impl Grid {
    fn at(&self, i: usize, j: usize) -> [f64; 2] {
        #[allow(clippy::cast_precision_loss)] // A few hundred points per side.
        let (x, y) = (i as f64, j as f64);
        [self.start[0] + self.step * x, self.start[1] + self.step * y]
    }
}

/// The places along the outline: corner and edge tips, and side tips beside leading edges.
#[derive(Default)]
pub struct Outline {
    pub tips: Vec<(Seen, Rank)>,
    pub sides: Vec<Seen>,
}

/// The angle between two unit vectors, in radians.
fn between_normals(a: [f64; 3], b: [f64; 3]) -> f64 {
    (a[0] * b[0] + a[1] * b[1] + a[2] * b[2])
        .clamp(-1.0, 1.0)
        .acos()
}

/// Whether the surface ends between `inner`, which wants a tip, and `outer` beyond it.
fn ends(inner: &Seen, outer: Option<&Seen>) -> bool {
    outer.is_none_or(|o| {
        o.cut
            || (o.point[2] - inner.point[2]).abs() > EDGE_STEP_MM
            || between_normals(o.normal, inner.normal) > EDGE_TURN_DEG.to_radians()
    })
}

/// An edge found from a grid point along a direction: how far along it the underside still
/// wants a tip, and what is seen there.
struct Edge {
    along: f64,
    inner: Seen,
}

/// The edge between `from` (which wants a tip, seeing `inner`) and the point a step along `d`
/// (seeing `outer`), if the surface ends there.
fn edge_along(
    probe: &dyn Probe,
    from: [f64; 2],
    d: [f64; 2],
    step: f64,
    (mut inner, mut outer): (Seen, Option<Seen>),
) -> Option<Edge> {
    let (mut lo, mut hi) = (0.0, step);
    for _ in 0..EDGE_BISECTIONS {
        let mid = f64::midpoint(lo, hi);
        let seen = probe.look(from[0] + mid * d[0], from[1] + mid * d[1]);
        if let Some(wanted) = seen.filter(|s| probe.wanted(s)) {
            lo = mid;
            inner = wanted;
        } else {
            hi = mid;
            outer = seen;
        }
    }
    ends(&inner, outer.as_ref()).then_some(Edge { along: lo, inner })
}

/// The side tip beside a leading edge seen at `edge`, outward along `d`, `radius` up its face,
/// if the face holds a contact `radius` wide at the surface.
fn side_tip(probe: &dyn Probe, edge: &Seen, d: [f64; 2], radius: f64) -> Option<Seen> {
    let steepest = SIDE_STEEPEST_DEG.to_radians().cos();
    let [edge_x, edge_y, edge_z] = edge.point;
    let mut run: Vec<Seen> = Vec::new();
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // Forty samples.
    let samples = (SIDE_REACH_MM / SIDE_STEP_MM).round() as u32;
    for sample in 1..=samples {
        let out = f64::from(sample) * SIDE_STEP_MM;
        let Some(seen) = probe.between(
            edge_x + out * d[0],
            edge_y + out * d[1],
            edge_z - 1e-3,
            edge_z + SIDE_RISE_MM,
        ) else {
            break;
        };
        let rising = run.last().map_or(edge_z, |last| last.point[2]);
        if seen.normal[2] > -steepest
            || seen.point[2] < rising - 1e-6
            || seen.cut
            || !probe.holds(&seen)
        {
            break;
        }
        run.push(seen);
    }
    // Up the face from the edge, as the contact's mark would measure it.
    let up = |s: &Seen| {
        let v = [0, 1, 2].map(|k| s.point[k] - edge.point[k]);
        (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
    };
    if run.last().is_none_or(|last| up(last) < 2.0 * radius) {
        return None;
    }
    run.into_iter().find(|s| up(s) >= radius)
}

/// The outline of the underside probed on `grid`, for contacts `radius` wide at the surface:
/// see the module documentation.
pub fn outline(probe: &dyn Probe, grid: &Grid, radius: f64) -> Outline {
    let mut out = Outline::default();
    let index = |i: usize, j: usize| j * grid.nx + i;
    let leading = LEADING_DEG.to_radians().sin();
    for j in 0..grid.ny {
        for i in 0..grid.nx {
            let k = index(i, j);
            let Some(here) = grid.seen[k].filter(|_| grid.wanted[k]) else {
                continue;
            };
            let from = grid.at(i, j);
            // The edges along -X, +X, -Y and +Y, each with its distance from this point, its
            // direction and how far in its tips stand.
            let mut found: [Option<(f64, [f64; 2], f64)>; 4] = [None; 4];
            let directions: [(isize, isize, [f64; 2]); 4] = [
                (-1, 0, [-1.0, 0.0]),
                (1, 0, [1.0, 0.0]),
                (0, -1, [0.0, -1.0]),
                (0, 1, [0.0, 1.0]),
            ];
            for (slot, (di, dj, d)) in directions.into_iter().enumerate() {
                let (ni, nj) = (i.checked_add_signed(di), j.checked_add_signed(dj));
                let beyond = match (ni, nj) {
                    (Some(ni), Some(nj)) if ni < grid.nx && nj < grid.ny => {
                        let n = index(ni, nj);
                        if grid.wanted[n] {
                            continue;
                        }
                        grid.seen[n]
                    }
                    _ => {
                        let [x, y] = from;
                        probe.look(x + grid.step * d[0], y + grid.step * d[1])
                    }
                };
                if beyond.as_ref().is_some_and(|b| probe.wanted(b)) {
                    continue;
                }
                let Some(edge) = edge_along(probe, from, d, grid.step, (here, beyond)) else {
                    continue;
                };
                let n = edge.inner.normal;
                let lean = n[0] * d[0] + n[1] * d[1];
                let inset = if lean <= -leading {
                    if let Some(side) = side_tip(probe, &edge.inner, d, radius) {
                        out.sides.push(side);
                    }
                    // The radius along the slope, in plan.
                    radius * (-n[2]).clamp(0.0, 1.0)
                } else {
                    EDGE_INSET_MM
                };
                found[slot] = Some((edge.along, d, inset));
            }
            // From the edges found, a point the inset in from each: `shift` along X and along Y.
            let at = |shift: [Option<(f64, [f64; 2], f64)>; 2]| -> Option<Seen> {
                let mut point = from;
                for (k, edge) in shift.into_iter().enumerate() {
                    if let Some((along, d, inset)) = edge {
                        point[k] += (along - inset) * d[k];
                    }
                }
                probe.look(point[0], point[1]).filter(|s| probe.wanted(s))
            };
            // An edge nearer this point than the inset across: the tip is kept that far from
            // it too.
            let across = |edges: [Option<(f64, [f64; 2], f64)>; 2]| {
                edges
                    .into_iter()
                    .flatten()
                    .find(|(along, _, inset)| along < inset)
            };
            let (x_edges, y_edges) = ([found[0], found[1]], [found[2], found[3]]);
            // A corner first: an edge along X and one along Y. An edge tip at the same place is
            // the corner's.
            for x_edge in x_edges.into_iter().flatten() {
                for y_edge in y_edges.into_iter().flatten() {
                    if let Some(tip) = at([Some(x_edge), Some(y_edge)]) {
                        out.tips.push((tip, Rank::Corner));
                    }
                }
            }
            for x_edge in x_edges.into_iter().flatten() {
                if let Some(tip) = at([Some(x_edge), across(y_edges)]) {
                    out.tips.push((tip, Rank::Edge));
                }
            }
            for y_edge in y_edges.into_iter().flatten() {
                if let Some(tip) = at([across(x_edges), Some(y_edge)]) {
                    out.tips.push((tip, Rank::Edge));
                }
            }
        }
    }
    out
}
