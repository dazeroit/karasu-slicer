//! Supports on a grid (owner, 2026-11-06, TD-138): the tips a flat underside takes, in rows.
//!
//! **The face.** A click picks a triangle of the model's surface facing down; the face is
//! every triangle joined to it, edge to edge, whose normal turns from the clicked one's by less
//! than "Flat within" ([`pick`]). A rectangle or a lasso drawn on the screen picks instead every
//! triangle facing down the shape touches, seen through the model ([`select`]), and the tips
//! are kept to the shape. The face's normal is its triangles' area-weighted mean ([`region`]).
//!
//! **The grid** lies in the face's plane, so a tilted face is gridded as a level one: a frame
//! (u along the rows, v across them, the normal n with u × v = n) has its rows along the face's
//! longest edge, the direction the outline's length runs most (within 2°), or at the angle typed
//! from the plane's own X. Points `spacing` apart along the rows and `across` between them, the
//! lattice centred on the face, each on the face where the surface lies under it.
//!
//! **Edges first** (TD-136): with the switch on, the outline's loops are cut at their corners
//! (turning more than [`CORNER_DEG`]); a corner takes a tip the inset in from both its sides,
//! and each side tips every `spacing` along it, the inset in. A side the face rises away from
//! (a leading edge, more than [`LEADING_DEG`]) takes its tips the contact's radius in, and the
//! face beside it, facing down no steeper than [`SIDE_STEEPEST_DEG`], a side tip the contact's
//! radius from the edge between each two of them. A grid point nearer an outline tip than half
//! the smaller spacing is left out, and none stands nearer the outline than the inset (the
//! contact's radius with the switch off), so every contact lies on the face.
//!
//! Everything follows the mesh's and the loops' order: it is deterministic.
#![allow(clippy::many_single_char_names, clippy::similar_names)] // Geometric notation.
use crate::{
    auto_ui::plan::{AutoSupports, EDGE_INSET_MM},
    camera::Mat4,
    islands_ui::plan::Tip,
    supports_ui::{NextSupport, SupportPreset, Weight},
};
use karasu_core::mesh::TriangleMesh;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    ops::RangeInclusive,
};

/// The spacings the panel accepts, in mm.
pub const SPACING_MM: RangeInclusive<f64> = 1.0..=20.0;
/// The edge insets the panel accepts, in mm.
pub const INSET_MM: RangeInclusive<f64> = 0.1..=3.0;
/// How far a face's triangles may turn from the one clicked, the range accepted, in degrees.
pub const FLAT_DEG: RangeInclusive<f64> = 0.5..=30.0;
/// The rows' angles the panel accepts, in degrees.
pub const ANGLE_DEG: RangeInclusive<f64> = 0.0..=180.0;
/// The branch distances the panel accepts, in mm.
pub const BRANCH_MM: RangeInclusive<f64> = 0.5..=20.0;
/// The branches per trunk the panel accepts.
pub const PER_TRUNK: RangeInclusive<f64> = 1.0..=10.0;
/// An outline turning more than this at a vertex has a corner there, in degrees.
pub const CORNER_DEG: f64 = 30.0;
/// A side the face rises away from by more than this is a leading edge, in degrees.
pub const LEADING_DEG: f64 = 10.0;
/// The face beside a leading edge takes side tips facing down no steeper than this, in degrees
/// from straight down.
pub const SIDE_STEEPEST_DEG: f64 = 75.0;
/// A border whose triangle beside turns from the face by more than this is an edge, as the
/// Automatic tool's outline counts one (TD-136), in degrees.
pub const EDGE_TURN_DEG: f64 = 30.0;
/// Edge directions this close count as one when the longest edge is found, in degrees.
const ALONG_TOLERANCE_DEG: f64 = 2.0;

/// How an underside is picked for the grid (owner, 2026-11-06): the strip's Grid button's
/// options, kept with the settings.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PickMode {
    /// A click on a flat face.
    #[default]
    Face,
    /// A rectangle dragged on the screen.
    Rectangle,
    /// A shape drawn freehand on the screen.
    Lasso,
}

/// How the grid is made: tool values, from Settings › Supports each time the tool opens.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
#[allow(clippy::struct_excessive_bools)] // The panel's switches, each its own value.
pub struct GridValues {
    /// The preset: a weight, or the user's preset at this place (never kept).
    pub weight: Weight,
    #[serde(skip)]
    pub mine: Option<usize>,
    /// Tips stand this far apart along the rows...
    pub spacing_mm: f64,
    /// ... and the rows this far apart, in mm.
    pub across_mm: f64,
    /// The rows run along the face's longest edge; otherwise at `angle_deg`.
    pub along_longest: bool,
    /// The rows' angle from the face plane's own X, in degrees.
    pub angle_deg: f64,
    /// The outline takes its own tips (TD-136).
    pub edges: bool,
    /// How far in from the outline its tips stand, in mm.
    pub inset_mm: f64,
    /// How far a face's triangles may turn from the one clicked, in degrees.
    pub flat_deg: f64,
    /// Tips close together share trunks, as the Automatic tool grows them.
    pub branches: bool,
    /// Tips closer than this in plan share a trunk, in mm.
    pub branch_mm: f64,
    /// The branches a trunk takes at most.
    pub per_trunk: u32,
}

/// The values the Grid mode starts from when the settings hold none (the owner's mockup).
pub const DEFAULT_GRID: GridValues = GridValues {
    weight: Weight::Medium,
    mine: None,
    spacing_mm: 3.0,
    across_mm: 3.0,
    along_longest: true,
    angle_deg: 0.0,
    edges: true,
    inset_mm: EDGE_INSET_MM,
    flat_deg: 5.0,
    branches: false,
    branch_mm: 5.0,
    per_trunk: 4,
};

impl Default for GridValues {
    fn default() -> Self {
        DEFAULT_GRID
    }
}

impl GridValues {
    /// These values if every one is in its range, else the built-in ones.
    #[must_use]
    pub fn valid(self) -> Self {
        if SPACING_MM.contains(&self.spacing_mm)
            && SPACING_MM.contains(&self.across_mm)
            && ANGLE_DEG.contains(&self.angle_deg)
            && INSET_MM.contains(&self.inset_mm)
            && FLAT_DEG.contains(&self.flat_deg)
            && BRANCH_MM.contains(&self.branch_mm)
            && PER_TRUNK.contains(&f64::from(self.per_trunk))
        {
            self
        } else {
            DEFAULT_GRID
        }
    }

    /// The values the supports are grown with: the preset, the branches when asked, feet on the
    /// model and mini supports where a support does not fit, as the Automatic tool's defaults.
    #[must_use]
    pub fn automatic(&self) -> AutoSupports {
        AutoSupports {
            weight: self.weight,
            mine: self.mine,
            branches: self.branches,
            branch_mm: self.branch_mm,
            per_trunk: self.per_trunk,
            merge_mm: 0.0,
            braces: false,
            ..AutoSupports::default()
        }
    }

    /// What the supports are made with: the preset's values.
    #[must_use]
    pub fn next(&self, presets: &[SupportPreset]) -> NextSupport {
        self.automatic().next(presets)
    }
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn scale(a: [f64; 3], s: f64) -> [f64; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn unit(a: [f64; 3]) -> Option<[f64; 3]> {
    let length = dot(a, a).sqrt();
    (length > 1e-12).then(|| scale(a, 1.0 / length))
}

/// Triangle `t`'s corners.
fn corners(mesh: &TriangleMesh, t: u32) -> [[f64; 3]; 3] {
    mesh.triangles()[t as usize].map(|v| {
        let p = mesh.vertices()[v as usize];
        [p.x, p.y, p.z]
    })
}

/// Triangle `t`'s outward normal and area; `None` for a degenerate one.
fn facet(mesh: &TriangleMesh, t: u32) -> Option<([f64; 3], f64)> {
    let [a, b, c] = corners(mesh, t);
    let n = cross(sub(b, a), sub(c, a));
    let length = dot(n, n).sqrt();
    (length > 1e-18).then(|| (scale(n, 1.0 / length), length / 2.0))
}

/// An edge's key: its two vertices, the lower first.
fn key(a: u32, b: u32) -> u64 {
    (u64::from(a.min(b)) << 32) | u64::from(a.max(b))
}

/// A flat underside picked on the surface: see the module documentation.
#[derive(Clone, Debug, PartialEq)]
pub struct Face {
    /// Its triangles, from the one clicked outwards.
    pub triangles: Vec<u32>,
    /// Their area-weighted mean outward normal.
    pub normal: [f64; 3],
    /// Their area, in mm².
    pub area_mm2: f64,
    /// How far it is tilted from level, in degrees.
    pub tilt_deg: f64,
}

/// The face of `mesh` (world mm) around triangle `seed`: see the module documentation. Only
/// triangles `keep` accepts (one side of a hollowed model) join it. `None` when the seed faces
/// up, or sideways (a support cannot stand under it), or is degenerate.
pub fn pick(
    mesh: &TriangleMesh,
    seed: u32,
    flat_deg: f64,
    keep: &dyn Fn(u32) -> bool,
) -> Option<Face> {
    let (n0, _) = facet(mesh, seed)?;
    if n0[2] >= 0.0 {
        return None;
    }
    let least = flat_deg.to_radians().cos();
    // The triangles that may join: turned from the seed's by less than the angle.
    let triangles = mesh.triangles();
    let mut near: HashMap<u64, Vec<u32>> = HashMap::new();
    for (t, triangle) in triangles.iter().enumerate() {
        let t = u32::try_from(t).ok()?;
        let Some((n, _)) = facet(mesh, t) else {
            continue;
        };
        if dot(n, n0) < least || !keep(t) {
            continue;
        }
        for (a, b) in [
            (triangle[0], triangle[1]),
            (triangle[1], triangle[2]),
            (triangle[2], triangle[0]),
        ] {
            near.entry(key(a, b)).or_default().push(t);
        }
    }
    // Those joined to the seed, edge to edge.
    let mut seen = HashSet::from([seed]);
    let mut queue = VecDeque::from([seed]);
    let mut out = Vec::new();
    while let Some(t) = queue.pop_front() {
        out.push(t);
        let triangle = triangles[t as usize];
        for (a, b) in [
            (triangle[0], triangle[1]),
            (triangle[1], triangle[2]),
            (triangle[2], triangle[0]),
        ] {
            for &other in near.get(&key(a, b)).map_or(&[][..], Vec::as_slice) {
                if seen.insert(other) {
                    queue.push_back(other);
                }
            }
        }
    }
    region(mesh, out)
}

/// The face `triangles` of `mesh` make: their area-weighted mean normal, area and tilt. `None`
/// for none, or when the mean does not face down.
#[must_use]
pub fn region(mesh: &TriangleMesh, triangles: Vec<u32>) -> Option<Face> {
    let (mut sum, mut area) = ([0.0; 3], 0.0);
    for &t in &triangles {
        if let Some((n, a)) = facet(mesh, t) {
            sum = add(sum, scale(n, a));
            area += a;
        }
    }
    let normal = unit(sum).filter(|n| n[2] < 0.0)?;
    Some(Face {
        triangles,
        normal,
        area_mm2: area,
        tilt_deg: (-normal[2]).clamp(-1.0, 1.0).acos().to_degrees(),
    })
}

/// Whether `polygon` (closed, even-odd) holds `q`.
#[must_use]
pub fn inside_polygon(polygon: &[[f64; 2]], q: [f64; 2]) -> bool {
    let mut crossings = false;
    for (k, a) in polygon.iter().enumerate() {
        let b = polygon[(k + 1) % polygon.len()];
        if (a[1] > q[1]) != (b[1] > q[1]) {
            let x = a[0] + (q[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0]);
            if q[0] < x {
                crossings = !crossings;
            }
        }
    }
    crossings
}

/// The triangles of `mesh` (world mm) facing down, of those `keep` accepts, that a `size` pixel
/// viewport seen through `view_projection` shows touched by `polygon` (pixels, origin top-left;
/// a rectangle's four corners or a lasso's path, closed): a corner or the centre inside it, or
/// its first point inside the triangle. In the mesh's order. The tips are then kept to the
/// shape itself ([`inside_polygon`]).
#[must_use]
pub fn select(
    mesh: &TriangleMesh,
    (view_projection, size): (&Mat4, [f64; 2]),
    polygon: &[[f64; 2]],
    keep: &dyn Fn(u32) -> bool,
) -> Vec<u32> {
    if polygon.len() < 3 {
        return Vec::new();
    }
    let inside = |q: [f64; 2]| inside_polygon(polygon, q);
    let mut out = Vec::new();
    for t in 0..mesh.triangles().len() {
        let Ok(t) = u32::try_from(t) else { break };
        let Some((n, _)) = facet(mesh, t) else {
            continue;
        };
        if n[2] >= 0.0 || !keep(t) {
            continue;
        }
        let [a, b, c] = corners(mesh, t);
        let centre = scale(add(a, add(b, c)), 1.0 / 3.0);
        let on_screen = |p: [f64; 3]| {
            view_projection.project(p).map(|[x, y, _]| {
                [
                    f64::midpoint(x, 1.0) * size[0],
                    f64::midpoint(1.0, -y) * size[1],
                ]
            })
        };
        let (Some(qa), Some(qb), Some(qc), Some(qm)) =
            (on_screen(a), on_screen(b), on_screen(c), on_screen(centre))
        else {
            continue;
        };
        // Touched: a corner or its centre in the shape, or the shape's first point in it.
        let held = |q: [f64; 2]| {
            let side = |p: [f64; 2], r: [f64; 2]| {
                (r[0] - p[0]) * (q[1] - p[1]) - (r[1] - p[1]) * (q[0] - p[0])
            };
            let (d1, d2, d3) = (side(qa, qb), side(qb, qc), side(qc, qa));
            !((d1 < 0.0 || d2 < 0.0 || d3 < 0.0) && (d1 > 0.0 || d2 > 0.0 || d3 > 0.0))
        };
        if [qa, qb, qc, qm].into_iter().any(&inside) || held(polygon[0]) {
            out.push(t);
        }
    }
    out
}

/// The tips a grid gives a face, and how many of each kind.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GridTips {
    /// Corners, then the sides' tips, then the side tips beside leading edges, then the grid's.
    pub tips: Vec<Tip>,
    pub corners: usize,
    pub edges: usize,
    pub sides: usize,
    pub grid: usize,
}

/// A face's outline edge: its ends (in the triangle's winding) and the triangle it bounds.
#[derive(Clone, Copy)]
struct Border {
    ends: [u32; 2],
    triangle: u32,
}

/// The face in its plane's frame: 2D points (u, v) and back to the surface.
struct Plane<'a> {
    mesh: &'a TriangleMesh,
    origin: [f64; 3],
    u: [f64; 3],
    v: [f64; 3],
    /// Buckets of the face's triangles and of the outline's segments (2D ends and the triangle
    /// they bound), `cell` mm square.
    cell: f64,
    triangles: HashMap<[i64; 2], Vec<u32>>,
    segments: HashMap<[i64; 2], Vec<usize>>,
    /// Each border's 2D ends, its triangle, and whether the surface ends there.
    outline: Vec<([[f64; 2]; 2], u32, bool)>,
}

impl Plane<'_> {
    fn to_2d(&self, p: [f64; 3]) -> [f64; 2] {
        let d = sub(p, self.origin);
        [dot(d, self.u), dot(d, self.v)]
    }

    fn up(&self, q: [f64; 2]) -> [f64; 3] {
        add(self.origin, add(scale(self.u, q[0]), scale(self.v, q[1])))
    }

    #[allow(clippy::cast_possible_truncation)] // Cells of a model's size.
    fn cell_of(&self, q: [f64; 2]) -> [i64; 2] {
        [
            (q[0] / self.cell).floor() as i64,
            (q[1] / self.cell).floor() as i64,
        ]
    }

    /// Where the face's surface lies under `q`: the point, its triangle's normal and the
    /// triangle; none off the face.
    fn surface_at(&self, q: [f64; 2]) -> Option<([f64; 3], [f64; 3], u32)> {
        for &t in self.triangles.get(&self.cell_of(q))? {
            let [a, b, c] = corners(self.mesh, t);
            let [a2, b2, c2] = [a, b, c].map(|p| self.to_2d(p));
            let det = (b2[0] - a2[0]) * (c2[1] - a2[1]) - (c2[0] - a2[0]) * (b2[1] - a2[1]);
            if det.abs() < 1e-18 {
                continue;
            }
            let s = ((q[0] - a2[0]) * (c2[1] - a2[1]) - (c2[0] - a2[0]) * (q[1] - a2[1])) / det;
            let r = ((b2[0] - a2[0]) * (q[1] - a2[1]) - (q[0] - a2[0]) * (b2[1] - a2[1])) / det;
            if s >= -1e-9 && r >= -1e-9 && s + r <= 1.0 + 1e-9 {
                let point = add(a, add(scale(sub(b, a), s), scale(sub(c, a), r)));
                let normal = facet(self.mesh, t).map_or([0.0, 0.0, -1.0], |f| f.0);
                return Some((point, normal, t));
            }
        }
        None
    }

    /// Whether the outline lies within `d` of `q`.
    fn near_outline(&self, q: [f64; 2], d: f64) -> bool {
        #[allow(clippy::cast_possible_truncation)] // A few cells.
        let reach = (d / self.cell).ceil() as i64 + 1;
        let [cx, cy] = self.cell_of(q);
        for dx in -reach..=reach {
            for dy in -reach..=reach {
                for &k in self
                    .segments
                    .get(&[cx + dx, cy + dy])
                    .map_or(&[][..], Vec::as_slice)
                {
                    let ([a, b], _, edge) = self.outline[k];
                    if edge && segment_distance(q, a, b) < d {
                        return true;
                    }
                }
            }
        }
        false
    }
}

/// The distance from `q` to the segment `[a, b]`.
fn segment_distance(q: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let length = dx * dx + dy * dy;
    let s = if length > 1e-18 {
        (((q[0] - a[0]) * dx + (q[1] - a[1]) * dy) / length).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (q[0] - a[0] - dx * s).hypot(q[1] - a[1] - dy * s)
}

/// The cells a 2D box covers.
#[allow(clippy::cast_possible_truncation)] // Cells of a model's size.
fn cells(low: [f64; 2], high: [f64; 2], cell: f64) -> impl Iterator<Item = [i64; 2]> {
    let (x0, y0) = (
        (low[0] / cell).floor() as i64,
        (low[1] / cell).floor() as i64,
    );
    let (x1, y1) = (
        (high[0] / cell).floor() as i64,
        (high[1] / cell).floor() as i64,
    );
    (x0..=x1).flat_map(move |x| (y0..=y1).map(move |y| [x, y]))
}

/// The face's outline: the edges only one of its triangles has, in their triangle's winding.
fn borders(mesh: &TriangleMesh, face: &Face) -> Vec<Border> {
    let mut count: HashMap<u64, (usize, Border)> = HashMap::new();
    let mut order = Vec::new();
    for &t in &face.triangles {
        let triangle = mesh.triangles()[t as usize];
        for (a, b) in [
            (triangle[0], triangle[1]),
            (triangle[1], triangle[2]),
            (triangle[2], triangle[0]),
        ] {
            let entry = count.entry(key(a, b)).or_insert_with(|| {
                order.push(key(a, b));
                (
                    0,
                    Border {
                        ends: [a, b],
                        triangle: t,
                    },
                )
            });
            entry.0 += 1;
        }
    }
    order
        .into_iter()
        .filter_map(|k| count.get(&k).filter(|(n, _)| *n == 1).map(|(_, b)| *b))
        .collect()
}

/// The outline's loops, each its borders in order (by their place in `borders`).
fn loops(borders: &[Border]) -> Vec<Vec<usize>> {
    let mut from: HashMap<u32, usize> = HashMap::new();
    for (k, b) in borders.iter().enumerate() {
        from.entry(b.ends[0]).or_insert(k);
    }
    let mut used = vec![false; borders.len()];
    let mut out = Vec::new();
    for start in 0..borders.len() {
        if used[start] {
            continue;
        }
        let mut chain = Vec::new();
        let mut k = start;
        while !used[k] {
            used[k] = true;
            chain.push(k);
            match from.get(&borders[k].ends[1]) {
                Some(&next) => k = next,
                None => break,
            }
        }
        out.push(chain);
    }
    out
}

/// The rows' direction in the plane (u0, v0), as an angle from u0: the direction the outline's
/// length runs most, within [`ALONG_TOLERANCE_DEG`] (1° bins, a 5° window).
fn longest_direction(segments: &[[[f64; 2]; 2]]) -> f64 {
    let mut bins = [0.0f64; 180];
    for [a, b] in segments {
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let length = dx.hypot(dy);
        if length < 1e-12 {
            continue;
        }
        let angle = dy.atan2(dx).to_degrees().rem_euclid(180.0);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // 0 to 179.
        let bin = (angle.round() as usize) % 180;
        bins[bin] += length;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // A few bins.
    let half = ALONG_TOLERANCE_DEG.round() as usize;
    let score = |k: usize| -> f64 {
        (0..=2 * half)
            .map(|d| bins[(k + 180 + d - half) % 180])
            .sum()
    };
    let best = (0..180).max_by(|&a, &b| score(a).total_cmp(&score(b)).then(b.cmp(&a)));
    #[allow(clippy::cast_precision_loss)] // 0 to 179.
    best.map_or(0.0, |k| (k as f64).to_radians())
}

/// What tells a tip's triangle on a hollowed model's cavity, and whether a tip's world point
/// is kept (a shape's).
pub type Filters<'a> = (&'a dyn Fn(u32) -> bool, &'a dyn Fn([f64; 3]) -> bool);

/// The tips `values` give `face` on `mesh` (world mm), for contacts `radius` wide at the
/// surface, of those `within` keeps (a shape's, by their world points): see the module
/// documentation. `cavity` tells a hollowed model's cavity triangles. Only the borders where
/// the surface ends, or turns by more than [`EDGE_TURN_DEG`], are edges: a shape's own border
/// across a face that goes on takes no edge tip.
#[allow(clippy::too_many_lines)] // The outline, then the grid, in order.
pub fn tips(
    mesh: &TriangleMesh,
    face: &Face,
    values: &GridValues,
    radius: f64,
    (cavity, within): Filters<'_>,
) -> GridTips {
    let n = face.normal;
    let mut out = GridTips::default();
    let Some(origin) = face.triangles.first().map(|&t| {
        let [a, b, c] = corners(mesh, t);
        scale(add(a, add(b, c)), 1.0 / 3.0)
    }) else {
        return out;
    };
    // The plane's own frame: its X, or its Y where X runs along the normal.
    let x = if n[0].abs() < 0.9 {
        [1.0, 0.0, 0.0]
    } else {
        [0.0, 1.0, 0.0]
    };
    let Some(u0) = unit(sub(x, scale(n, dot(x, n)))) else {
        return out;
    };
    let v0 = cross(n, u0);
    let borders = borders(mesh, face);
    let point = |v: u32| {
        let p = mesh.vertices()[v as usize];
        [p.x, p.y, p.z]
    };
    let flat = |p: [f64; 3]| {
        let d = sub(p, origin);
        [dot(d, u0), dot(d, v0)]
    };
    let angle = if values.along_longest {
        let segments: Vec<[[f64; 2]; 2]> = borders
            .iter()
            .map(|b| b.ends.map(|v| flat(point(v))))
            .collect();
        longest_direction(&segments)
    } else {
        values.angle_deg.to_radians()
    };
    let (sin, cos) = angle.sin_cos();
    let u = add(scale(u0, cos), scale(v0, sin));
    let v = cross(n, u);
    let cell = values.spacing_mm.min(values.across_mm).max(0.5);
    let mut plane = Plane {
        mesh,
        origin,
        u,
        v,
        cell,
        triangles: HashMap::new(),
        segments: HashMap::new(),
        outline: Vec::new(),
    };
    let (mut low, mut high) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
    for &t in &face.triangles {
        let c = corners(mesh, t).map(|p| plane.to_2d(p));
        let lo = [0, 1].map(|k| c[0][k].min(c[1][k]).min(c[2][k]));
        let hi = [0, 1].map(|k| c[0][k].max(c[1][k]).max(c[2][k]));
        for k in 0..2 {
            low[k] = low[k].min(lo[k]);
            high[k] = high[k].max(hi[k]);
        }
        for at in cells(lo, hi, cell) {
            plane.triangles.entry(at).or_default().push(t);
        }
    }
    // The triangles beside the outline: each border's other one, for its side tips and to tell
    // an edge (none beside, or one turned away) from a border across a surface that goes on.
    let wanted: HashMap<u64, usize> = borders
        .iter()
        .enumerate()
        .map(|(k, b)| (key(b.ends[0], b.ends[1]), k))
        .collect();
    let in_face: HashSet<u32> = face.triangles.iter().copied().collect();
    let mut beside: Vec<Option<u32>> = vec![None; borders.len()];
    for (t, triangle) in mesh.triangles().iter().enumerate() {
        let Ok(t) = u32::try_from(t) else { break };
        if in_face.contains(&t) {
            continue;
        }
        for (a, b) in [
            (triangle[0], triangle[1]),
            (triangle[1], triangle[2]),
            (triangle[2], triangle[0]),
        ] {
            if let Some(&k) = wanted.get(&key(a, b)) {
                beside[k].get_or_insert(t);
            }
        }
    }
    let turned = EDGE_TURN_DEG.to_radians().cos();
    let edge: Vec<bool> = beside
        .iter()
        .map(|b| {
            b.and_then(|t| facet(mesh, t))
                .is_none_or(|(m, _)| dot(m, n) < turned)
        })
        .collect();
    for (b, &is_edge) in borders.iter().zip(&edge) {
        let ends = b.ends.map(|v| plane.to_2d(point(v)));
        let k = plane.outline.len();
        plane.outline.push((ends, b.triangle, is_edge));
        let lo = [0, 1].map(|j| ends[0][j].min(ends[1][j]));
        let hi = [0, 1].map(|j| ends[0][j].max(ends[1][j]));
        for at in cells(lo, hi, cell) {
            plane.segments.entry(at).or_default().push(k);
        }
    }
    let mut taken: Vec<[f64; 2]> = Vec::new();
    // The tip at `q` on the face, no nearer the outline than `keep_from`, if there is one.
    let place = |q: [f64; 2], keep_from: f64| -> Option<Tip> {
        let (p, normal, t) = plane.surface_at(q)?;
        (!plane.near_outline(q, keep_from) && normal[2] < 0.0 && within(p))
            .then(|| (p, normal, cavity(t)))
    };
    let leading = LEADING_DEG.to_radians().sin();
    // How far a side's tips stand in: the contact's radius along a leading edge.
    let inset_of = |inward: [f64; 2]| -> (f64, bool) {
        let rise = add(scale(u, inward[0]), scale(v, inward[1]))[2];
        if rise >= leading {
            (radius, true)
        } else {
            (values.inset_mm, false)
        }
    };
    let mut side_points: Vec<([f64; 3], [f64; 3], u32)> = Vec::new();
    if values.edges {
        for chain in loops(&borders) {
            let ends: Vec<[[f64; 2]; 2]> = chain.iter().map(|&k| plane.outline[k].0).collect();
            let dir = |e: [[f64; 2]; 2]| {
                let (dx, dy) = (e[1][0] - e[0][0], e[1][1] - e[0][1]);
                let l = dx.hypot(dy).max(1e-12);
                [dx / l, dy / l]
            };
            // The face lies on each border's left (u × v = n, triangles wound about n).
            let inward = |e: [[f64; 2]; 2]| {
                let d = dir(e);
                [-d[1], d[0]]
            };
            let m = ends.len();
            let closed =
                chain.len() > 2 && borders[chain[m - 1]].ends[1] == borders[chain[0]].ends[0];
            // Corners: where the outline turns more than the corner angle.
            let turn = |i: usize| {
                let (a, b) = (dir(ends[(i + m - 1) % m]), dir(ends[i]));
                (a[0] * b[0] + a[1] * b[1])
                    .clamp(-1.0, 1.0)
                    .acos()
                    .to_degrees()
            };
            // Corners between two edges; a border that is no edge takes no tip.
            let is_edge = |i: usize| edge[chain[i]];
            let corner_at: Vec<usize> = (0..m)
                .filter(|&i| (closed || i > 0) && turn(i) > CORNER_DEG)
                .collect();
            for &i in &corner_at {
                if !is_edge(i) || !is_edge((i + m - 1) % m) {
                    continue;
                }
                let (e1, e2) = (ends[(i + m - 1) % m], ends[i]);
                let (n1, n2) = (inward(e1), inward(e2));
                let ((i1, _), (i2, _)) = (inset_of(n1), inset_of(n2));
                let vertex = e2[0];
                // The point i1 in from the first side and i2 from the second.
                let det = n1[0] * n2[1] - n1[1] * n2[0];
                if det.abs() < 1e-6 {
                    continue;
                }
                let x = (i1 * n2[1] - i2 * n1[1]) / det;
                let y = (n1[0] * i2 - n2[0] * i1) / det;
                if x.hypot(y) > 4.0 * i1.max(i2) {
                    continue;
                }
                let q = [vertex[0] + x, vertex[1] + y];
                if let Some(tip) = place(q, i1.min(i2) * 0.9) {
                    out.tips.push(tip);
                    taken.push(q);
                    out.corners += 1;
                }
            }
            // The sides between corners (a loop without one is one side, all round).
            let starts: Vec<usize> = if corner_at.is_empty() {
                vec![0]
            } else {
                corner_at.clone()
            };
            for (s, &first) in starts.iter().enumerate() {
                let last = if corner_at.is_empty() {
                    first + m
                } else if s + 1 < starts.len() {
                    starts[s + 1]
                } else if closed {
                    starts[0] + m
                } else {
                    m
                };
                let side: Vec<usize> = (first..last).map(|i| i % m).collect();
                let length: f64 = side
                    .iter()
                    .map(|&i| {
                        let e = ends[i];
                        (e[1][0] - e[0][0]).hypot(e[1][1] - e[0][1])
                    })
                    .sum();
                if length < 1e-9 {
                    continue;
                }
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // A few.
                let count = ((length / values.spacing_mm).round() as usize).max(1);
                #[allow(clippy::cast_precision_loss)] // A few.
                let step = length / count as f64;
                // Its tips every step, past its corners; on a loop without corners, all round.
                let ring = corner_at.is_empty();
                let positions: Vec<f64> = if ring {
                    #[allow(clippy::cast_precision_loss)] // A few.
                    (0..count).map(|k| step * k as f64).collect()
                } else {
                    #[allow(clippy::cast_precision_loss)] // A few.
                    (1..count).map(|k| step * k as f64).collect()
                };
                let at = |position: f64| -> Option<(usize, [f64; 2])> {
                    let mut walked = 0.0;
                    for &i in &side {
                        let e = ends[i];
                        let l = (e[1][0] - e[0][0]).hypot(e[1][1] - e[0][1]);
                        if walked + l >= position || i == side[side.len() - 1] {
                            let t = ((position - walked) / l.max(1e-12)).clamp(0.0, 1.0);
                            return Some((
                                i,
                                [
                                    e[0][0] + (e[1][0] - e[0][0]) * t,
                                    e[0][1] + (e[1][1] - e[0][1]) * t,
                                ],
                            ));
                        }
                        walked += l;
                    }
                    None
                };
                let mut led: Vec<f64> = Vec::new();
                for &position in &positions {
                    let Some((i, q)) = at(position).filter(|(i, _)| is_edge(*i)) else {
                        continue;
                    };
                    let normal = inward(ends[i]);
                    let (inset, lead) = inset_of(normal);
                    let into = [q[0] + normal[0] * inset, q[1] + normal[1] * inset];
                    if let Some(tip) = place(into, inset * 0.9) {
                        out.tips.push(tip);
                        taken.push(into);
                        out.edges += 1;
                        if lead {
                            led.push(position);
                        }
                    }
                }
                // Between each two leading tips (and past the last on a ring), a side tip on the
                // face beside the edge.
                let mut between: Vec<f64> =
                    led.windows(2).map(|w| f64::midpoint(w[0], w[1])).collect();
                if ring && led.len() > 1 {
                    between.push((led[led.len() - 1] + led[0] + length) / 2.0 % length);
                }
                for position in between {
                    let Some((i, q)) = at(position) else {
                        continue;
                    };
                    let k = chain[i];
                    let Some(t) = beside[k] else {
                        continue;
                    };
                    if let Some(side) = side_tip(mesh, &plane, (q, inward(ends[i])), t, radius) {
                        if cavity(t) == cavity(borders[k].triangle) && within(side.0) {
                            side_points.push(side);
                        }
                    }
                }
            }
        }
    }
    for (p, normal, t) in side_points {
        out.tips.push((p, normal, cavity(t)));
        out.sides += 1;
    }
    // The grid, centred on the face, kept clear of the outline and of its tips.
    let clear = if values.edges {
        values.inset_mm
    } else {
        radius
    };
    let apart = values.spacing_mm.min(values.across_mm) / 2.0;
    let centre = [
        f64::midpoint(low[0], high[0]),
        f64::midpoint(low[1], high[1]),
    ];
    #[allow(clippy::cast_possible_truncation)] // A model's size over a spacing.
    let reach = |half: f64, step: f64| (half / step).floor() as i64 + 1;
    let (nx, ny) = (
        reach((high[0] - low[0]) / 2.0, values.spacing_mm),
        reach((high[1] - low[1]) / 2.0, values.across_mm),
    );
    for j in -ny..=ny {
        for i in -nx..=nx {
            #[allow(clippy::cast_precision_loss)] // A few hundred.
            let q = [
                centre[0] + values.spacing_mm * i as f64,
                centre[1] + values.across_mm * j as f64,
            ];
            if taken
                .iter()
                .any(|o| (o[0] - q[0]).hypot(o[1] - q[1]) < apart)
            {
                continue;
            }
            if let Some(tip) = place(q, clear) {
                out.tips.push(tip);
                out.grid += 1;
            }
        }
    }
    out
}

/// A side tip on triangle `t` beside a leading edge at `q` (the face's plane), whose inward
/// direction there is `inward`: the contact's `radius` from the edge, away from the face, on
/// `t`; none where `t` faces up or steeper than [`SIDE_STEEPEST_DEG`], or the point is off it.
fn side_tip(
    mesh: &TriangleMesh,
    plane: &Plane<'_>,
    (q, inward): ([f64; 2], [f64; 2]),
    t: u32,
    radius: f64,
) -> Option<([f64; 3], [f64; 3], u32)> {
    let (n, _) = facet(mesh, t)?;
    if n[2] > -SIDE_STEEPEST_DEG.to_radians().cos() {
        return None;
    }
    let edge_point = plane.up(q);
    let into = add(scale(plane.u, inward[0]), scale(plane.v, inward[1]));
    // Away from the face, along t's plane, square to the edge: of the two ways, the one whose
    // point lies on t (both only on a sliver: the one turned from the face).
    let along = unit(cross(into, plane_normal(plane)))?;
    let away = unit(cross(n, along))?;
    let [a, b, c] = corners(mesh, t);
    let on_t = |point: [f64; 3]| -> Option<[f64; 3]> {
        let (ab, ac, ap) = (sub(b, a), sub(c, a), sub(point, a));
        let (d00, d01, d11) = (dot(ab, ab), dot(ab, ac), dot(ac, ac));
        let (d20, d21) = (dot(ap, ab), dot(ap, ac));
        let det = d00 * d11 - d01 * d01;
        if det.abs() < 1e-18 {
            return None;
        }
        let s = (d11 * d20 - d01 * d21) / det;
        let r = (d00 * d21 - d01 * d20) / det;
        (s >= -1e-6 && r >= -1e-6 && s + r <= 1.0 + 1e-6)
            .then(|| add(a, add(scale(ab, s), scale(ac, r))))
    };
    let mut ways = [away, scale(away, -1.0)];
    if dot(away, into) > 0.0 {
        ways.swap(0, 1);
    }
    ways.into_iter()
        .find_map(|way| on_t(add(edge_point, scale(way, radius))))
        .map(|on| (on, n, t))
}

/// The plane's normal: u × v.
fn plane_normal(plane: &Plane<'_>) -> [f64; 3] {
    cross(plane.u, plane.v)
}

#[cfg(test)]
mod tests;
