//! Arrange by outline (TD-165): convex footprints packed with no-fit polygons.
//!
//! [`arrange`](super::arrange) packs rectangles. Here each footprint is a convex polygon, the
//! shape a band stretched round a model (its supports and raft included) has seen from above,
//! so round bases nest and triangles share their room.
//!
//! **Shapes.** Every polygon is convex and counter-clockwise, in world millimetres. A footprint
//! may be offered in several orientations (turns of the model about Z): the packer takes the one
//! that lands nearest the plate's middle. [`bounded`] keeps each to at most
//! [`MAX_OUTLINE_VERTICES`] vertices by a polygon that contains it, and the spacing grows each by
//! half its distance ([`grown`]), so two placed outlines stay at least the spacing apart.
//!
//! **No-fit polygons.** A footprint `B` moved by `t` overlaps a placed convex `A` exactly when `t`
//! lies inside their no-fit polygon, the Minkowski sum `A ⊕ (−B)` ([`minkowski`]), made by
//! merging the two polygons' edges by angle in linear time (Cuninghame-Green, 1989; Bennell and
//! Oliveira, 2008). The plate's room is a rectangle of moves (the inner-fit rectangle), as a convex
//! footprint lies within a rectangle exactly when its own rectangle does.
//!
//! **Placement** ([`arrange_outlines`]), centre-out as the rectangles are: each footprint, the
//! largest first, goes to the free move nearest the plate's middle among the move that centres
//! it, the no-fit polygons' vertices, the points of their edges nearest that move, and the
//! crossings of their edges with each other and with the inner-fit rectangle: where it touches
//! one or two footprints placed, or the plate's edge. A
//! move is free within [`OUTLINE_TOLERANCE_MM`] (1 µm: far below a pixel, above the rounding of
//! the moves).
//!
//! **When not all fit** from the middle, two fallbacks are tried, and the arrangement that leaves
//! fewest out is taken:
//! - **From the corner:** the same placement aiming at the front left corner (lowest, then
//!   leftmost, as bottom-left fill does; Burke, Hellier, Kendall and Whitwell, 2006), so rows
//!   along the plate's edges nest into each other, round bases in a honeycomb; centred again on
//!   the plate when nothing else is on it. A centred honeycomb loses rows at the plate's edges,
//!   so this fits up to a third more round bases than rectangles do on a full plate.
//! - **Rectangles:** a rectangle arrangement is also an outline arrangement, since each outline
//!   lies within its rectangle, so outlines never fit fewer than [`arrange`](super::arrange).
//!
//! The arithmetic is plain `f64` in a fixed order, so equal inputs give bitwise-equal results on
//! every platform.
use super::{arrange_with, convex_hull, Rect, Unplaced};
use std::{
    cmp::Ordering,
    collections::{BinaryHeap, HashMap},
};

/// The most vertices an outline keeps (TD-165): more are replaced by a polygon that contains them
/// ([`bounded`]), so each no-fit polygon has at most twice as many, with the spacing's.
pub const MAX_OUTLINE_VERTICES: usize = 32;

/// How far, in mm, two outlines may overlap and still count as clear, and a move may stray past the
/// plate's room (TD-165): 1 µm, far below a pixel and above the rounding of the moves.
pub const OUTLINE_TOLERANCE_MM: f64 = 1e-3;

/// The sides of the polygon round the circle that grows outlines by half the spacing.
const SPACING_SIDES: u32 = 8;

/// Most cells along each side of the grid that finds the no-fit polygons near a point.
const MAX_GRID_CELLS: usize = 256;

type Point = [f64; 2];

fn sub(a: Point, b: Point) -> Point {
    [a[0] - b[0], a[1] - b[1]]
}

fn add(a: Point, b: Point) -> Point {
    [a[0] + b[0], a[1] + b[1]]
}

fn cross(a: Point, b: Point) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}

/// The area of a counter-clockwise polygon, in mm² (negative for a clockwise one).
#[must_use]
pub fn area(polygon: &[[f64; 2]]) -> f64 {
    let n = polygon.len();
    (0..n)
        .map(|k| cross(polygon[k], polygon[(k + 1) % n]))
        .sum::<f64>()
        / 2.0
}

/// The rectangle around `polygon`; an empty polygon gives an empty rectangle at the origin.
#[must_use]
pub fn bounds(polygon: &[[f64; 2]]) -> Rect {
    if polygon.is_empty() {
        return Rect {
            min: [0.0; 2],
            max: [0.0; 2],
        };
    }
    let mut rect = Rect {
        min: [f64::INFINITY; 2],
        max: [f64::NEG_INFINITY; 2],
    };
    for p in polygon {
        for k in 0..2 {
            rect.min[k] = rect.min[k].min(p[k]);
            rect.max[k] = rect.max[k].max(p[k]);
        }
    }
    rect
}

/// `polygon` moved by `offset`.
#[must_use]
pub fn translated(polygon: &[[f64; 2]], offset: [f64; 2]) -> Vec<[f64; 2]> {
    polygon.iter().map(|&p| add(p, offset)).collect()
}

/// `polygon` turned `turn_deg` counter-clockwise about the origin. Quarter turns are exact.
#[must_use]
pub fn turned(polygon: &[[f64; 2]], turn_deg: f64) -> Vec<[f64; 2]> {
    let quarter = turn_deg.rem_euclid(360.0);
    #[allow(clippy::float_cmp)] // Exact quarter turns only.
    let exact: Option<fn(Point) -> Point> = if quarter == 0.0 {
        Some(|p| p)
    } else if quarter == 90.0 {
        Some(|[x, y]| [-y, x])
    } else if quarter == 180.0 {
        Some(|[x, y]| [-x, -y])
    } else if quarter == 270.0 {
        Some(|[x, y]| [y, -x])
    } else {
        None
    };
    if let Some(turn) = exact {
        return polygon.iter().map(|&p| turn(p)).collect();
    }
    let (sin, cos) = turn_deg.to_radians().sin_cos();
    polygon
        .iter()
        .map(|&[x, y]| [x * cos - y * sin, x * sin + y * cos])
        .collect()
}

/// A heap entry of [`bounded`]: the area that removing the edge starting at `vertex` adds, and the
/// vertex's version when it was pushed.
#[derive(PartialEq)]
struct Removal {
    added: f64,
    vertex: usize,
    version: u64,
}

impl Eq for Removal {}

impl PartialOrd for Removal {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Removal {
    /// The smallest area first, then the lowest vertex: a max-heap of reversed entries.
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .added
            .total_cmp(&self.added)
            .then(other.vertex.cmp(&self.vertex))
    }
}

/// A convex polygon with at most `max_vertices` vertices (at least 4) that contains the convex,
/// counter-clockwise `polygon` (TD-165), unchanged when it has no more.
///
/// Each step drops the edge whose removal adds the least area: its neighbours are extended until
/// they meet, which they do ahead of it when the two turn by less than half a turn together. A
/// convex polygon of five or more sides always has such an edge, since its turns add up to a whole
/// turn. This is a greedy approximation of the smallest circumscribing polygon (Aggarwal, Chang and
/// Yap, 1985); each edge's cost is kept in a heap, so it takes `O(n log n)`.
#[must_use]
pub fn bounded(polygon: &[[f64; 2]], max_vertices: usize) -> Vec<[f64; 2]> {
    let max_vertices = max_vertices.max(4);
    let n = polygon.len();
    if n <= max_vertices {
        return polygon.to_vec();
    }
    let mut points = polygon.to_vec();
    let mut next: Vec<usize> = (0..n).map(|k| (k + 1) % n).collect();
    let mut previous: Vec<usize> = (0..n).map(|k| (k + n - 1) % n).collect();
    let mut alive = vec![true; n];
    let mut version = vec![0u64; n];
    // Removing the edge from `v` to its next: its neighbours' lines meet at the point returned,
    // adding the area returned; `None` when they meet behind it.
    let removal = |points: &[Point], next: &[usize], previous: &[usize], v: usize| {
        let (a, b) = (v, next[v]);
        let (before, after) = (previous[a], next[b]);
        let incoming = sub(points[a], points[before]);
        let outgoing = sub(points[after], points[b]);
        let turn = cross(incoming, outgoing);
        if turn <= 0.0 {
            return None;
        }
        let s = cross(sub(points[b], points[a]), outgoing) / turn;
        if !(s.is_finite() && s >= 0.0) {
            return None;
        }
        let meet = add(points[a], [incoming[0] * s, incoming[1] * s]);
        let added = cross(sub(meet, points[a]), sub(points[b], points[a])).abs() / 2.0;
        Some((meet, added))
    };
    let mut heap = BinaryHeap::with_capacity(n);
    for v in 0..n {
        if let Some((_, added)) = removal(&points, &next, &previous, v) {
            heap.push(Removal {
                added,
                vertex: v,
                version: 0,
            });
        }
    }
    let mut count = n;
    while count > max_vertices {
        let Some(entry) = heap.pop() else {
            break;
        };
        let v = entry.vertex;
        if !alive[v] || entry.version != version[v] {
            continue;
        }
        let Some((meet, _)) = removal(&points, &next, &previous, v) else {
            continue;
        };
        // The edge from `v` to `w` goes: `v` moves to the meeting point and `w` leaves.
        let w = next[v];
        points[v] = meet;
        alive[w] = false;
        next[v] = next[w];
        previous[next[w]] = v;
        count -= 1;
        for u in [previous[v], v] {
            version[u] += 1;
            if let Some((_, added)) = removal(&points, &next, &previous, u) {
                heap.push(Removal {
                    added,
                    vertex: u,
                    version: version[u],
                });
            }
        }
    }
    // Counter-clockwise from the first vertex left.
    let first = (0..n).find(|&k| alive[k]).unwrap_or(0);
    let mut out = Vec::with_capacity(count);
    let mut v = first;
    loop {
        out.push(points[v]);
        v = next[v];
        if v == first || out.len() > n {
            break;
        }
    }
    out
}

/// The Minkowski sum of two convex, counter-clockwise polygons, counter-clockwise: their edges
/// merged by angle from their lowest vertices (then leftmost), in `O(n + m)`. A polygon of fewer
/// than three points (a point or a segment) is summed point by point and hulled.
#[must_use]
pub fn minkowski(a: &[[f64; 2]], b: &[[f64; 2]]) -> Vec<[f64; 2]> {
    if a.is_empty() || b.is_empty() {
        return Vec::new();
    }
    if a.len() < 3 || b.len() < 3 {
        let sums: Vec<Point> = a
            .iter()
            .flat_map(|&p| b.iter().map(move |&q| add(p, q)))
            .collect();
        return convex_hull(&sums);
    }
    let lowest = |p: &[Point]| {
        (0..p.len())
            .min_by(|&i, &j| {
                p[i][1]
                    .total_cmp(&p[j][1])
                    .then(p[i][0].total_cmp(&p[j][0]))
            })
            .unwrap_or(0)
    };
    let (a_len, b_len) = (a.len(), b.len());
    let (a_start, b_start) = (lowest(a), lowest(b));
    let pa = |i: usize| a[(a_start + i) % a_len];
    let pb = |j: usize| b[(b_start + j) % b_len];
    let mut out = Vec::with_capacity(a_len + b_len);
    let (mut i, mut j) = (0, 0);
    while i < a_len || j < b_len {
        out.push(add(pa(i), pb(j)));
        // The edges left of one polygon all turn further than those taken of the other.
        if i == a_len {
            j += 1;
            continue;
        }
        if j == b_len {
            i += 1;
            continue;
        }
        let turn = cross(sub(pa(i + 1), pa(i)), sub(pb(j + 1), pb(j)));
        if turn > 0.0 {
            i += 1;
        } else if turn < 0.0 {
            j += 1;
        } else {
            i += 1;
            j += 1;
        }
    }
    out
}

/// `polygon` grown by `radius`: its Minkowski sum with an octagon drawn
/// round the circle of that radius, so it contains every point within `radius` of `polygon`.
/// Unchanged for no radius.
#[must_use]
pub fn grown(polygon: &[[f64; 2]], radius: f64) -> Vec<[f64; 2]> {
    if radius <= 0.0 || polygon.is_empty() {
        return polygon.to_vec();
    }
    let step = std::f64::consts::TAU / f64::from(SPACING_SIDES);
    // Corners at r / cos(π / n), so the sides touch the circle.
    let outside = radius / (step / 2.0).cos();
    let round: Vec<Point> = (0..SPACING_SIDES)
        .map(|k| {
            let (sin, cos) = (step * f64::from(k)).sin_cos();
            [outside * cos, outside * sin]
        })
        .collect();
    minkowski(polygon, &round)
}

/// How far the projections of `a` and `b` overlap on the axis least overlapped among the X and
/// Y axes and every edge's normal: negative when they are apart. For convex polygons it is
/// positive exactly when their interiors overlap (separating axes).
fn penetration(a: &[[f64; 2]], b: &[[f64; 2]]) -> f64 {
    let mut least = f64::INFINITY;
    let mut axis = |normal: Point| {
        let length = normal[0].hypot(normal[1]);
        if length.is_nan() || length <= 0.0 {
            return;
        }
        let project = |polygon: &[Point]| {
            polygon
                .iter()
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| {
                    let d = (p[0] * normal[0] + p[1] * normal[1]) / length;
                    (lo.min(d), hi.max(d))
                })
        };
        let (a_low, a_high) = project(a);
        let (b_low, b_high) = project(b);
        least = least.min(a_high.min(b_high) - a_low.max(b_low));
    };
    axis([1.0, 0.0]);
    axis([0.0, 1.0]);
    for polygon in [a, b] {
        let n = polygon.len();
        if n < 2 {
            continue;
        }
        for k in 0..n {
            let edge = sub(polygon[(k + 1) % n], polygon[k]);
            axis([edge[1], -edge[0]]);
        }
    }
    least
}

/// Whether the interiors of convex outlines `a` and `b` overlap by more than
/// [`OUTLINE_TOLERANCE_MM`] (TD-165): the overlap checks of the status bar and the slice.
#[must_use]
pub fn outlines_overlap(a: &[[f64; 2]], b: &[[f64; 2]]) -> bool {
    !a.is_empty() && !b.is_empty() && penetration(a, b) > OUTLINE_TOLERANCE_MM
}

/// Pairs `(i, j)`, `i < j`, of convex `outlines` that overlap ([`outlines_overlap`]), in
/// increasing order. Their rectangles are compared first, so outlines far apart cost nothing.
#[must_use]
pub fn overlapping_outlines(outlines: &[Vec<[f64; 2]>]) -> Vec<(usize, usize)> {
    let rects: Vec<Rect> = outlines.iter().map(|o| bounds(o)).collect();
    super::overlapping_pairs(&rects)
        .into_iter()
        .filter(|&(i, j)| outlines_overlap(&outlines[i], &outlines[j]))
        .collect()
}

/// Where [`arrange_outlines`] put a footprint: which of its orientations, and the move applied to
/// that orientation's polygon.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlacedOutline {
    /// The orientation, an index into the footprint's orientations.
    pub orientation: usize,
    /// The move, in mm.
    pub offset: [f64; 2],
}

/// One orientation of a footprint, ready to place: its polygon bounded, with its area and
/// rectangle, and the polygon grown by half the spacing.
struct Form {
    area: f64,
    body: Rect,
    grown: Vec<Point>,
}

/// A no-fit polygon: its vertices, its rectangle, its edges with their rectangles, and each
/// edge's inward unit normal with the normal's offset, so a point's depth inside is the least
/// `n · p − c`.
struct NoFit {
    points: Vec<Point>,
    rect: Rect,
    segments: Vec<(Point, Point, Rect)>,
    edges: Vec<(Point, f64)>,
}

impl NoFit {
    fn new(points: Vec<Point>) -> Self {
        let n = points.len();
        let segments = if n < 2 {
            Vec::new()
        } else {
            (0..n)
                .map(|k| {
                    let (a, b) = (points[k], points[(k + 1) % n]);
                    (a, b, bounds(&[a, b]))
                })
                .collect()
        };
        let edges = (0..n)
            .filter_map(|k| {
                let (a, b) = (points[k], points[(k + 1) % n]);
                let e = sub(b, a);
                let length = e[0].hypot(e[1]);
                (length > 0.0).then(|| {
                    let normal = [-e[1] / length, e[0] / length];
                    (normal, normal[0] * a[0] + normal[1] * a[1])
                })
            })
            .collect();
        Self {
            rect: bounds(&points),
            points,
            segments,
            edges,
        }
    }

    /// How deep `p` lies inside: the least distance to an edge's line, negative outside. A
    /// polygon of fewer than three points has no inside.
    fn depth(&self, p: Point) -> f64 {
        if self.points.len() < 3 {
            return f64::NEG_INFINITY;
        }
        self.edges
            .iter()
            .map(|&(normal, c)| normal[0] * p[0] + normal[1] * p[1] - c)
            .fold(f64::INFINITY, f64::min)
    }
}

/// A grid of cells over a rectangle, each listing the no-fit polygons whose rectangles meet it.
struct Grid {
    origin: Point,
    cell: f64,
    columns: usize,
    rows: usize,
    cells: Vec<Vec<usize>>,
}

impl Grid {
    /// A grid over `room` of the polygons of `polygons` listed in `chosen`.
    fn new(room: Rect, polygons: &[NoFit], chosen: &[usize]) -> Self {
        let [width, depth] = room.size();
        // Cells about as large as the polygons, within the bound on their number.
        let mut sides: Vec<f64> = chosen
            .iter()
            .map(|&k| {
                let [w, d] = polygons[k].rect.size();
                w.max(d)
            })
            .collect();
        sides.sort_by(f64::total_cmp);
        let typical = sides.get(sides.len() / 2).copied().unwrap_or(1.0).max(1e-6);
        #[allow(clippy::cast_precision_loss)] // A small cell count.
        let bound = MAX_GRID_CELLS as f64;
        let cell = typical.max(width / bound).max(depth / bound).max(1e-6);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // Bounded above.
        let count = |extent: f64| ((extent / cell).floor() as usize + 1).min(MAX_GRID_CELLS);
        let (columns, rows) = (count(width.max(0.0)), count(depth.max(0.0)));
        let mut grid = Self {
            origin: room.min,
            cell,
            columns,
            rows,
            cells: vec![Vec::new(); columns * rows],
        };
        for &k in chosen {
            let p = &polygons[k];
            let ([c0, r0], [c1, r1]) = (grid.index(p.rect.min), grid.index(p.rect.max));
            for r in r0..=r1 {
                for c in c0..=c1 {
                    grid.cells[r * columns + c].push(k);
                }
            }
        }
        grid
    }

    /// The cell of `p`, clamped to the grid.
    fn index(&self, p: Point) -> [usize; 2] {
        let at = |v: f64, origin: f64, count: usize| {
            let k = ((v - origin) / self.cell).floor();
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // Clamped.
            let k = if k.is_finite() && k > 0.0 {
                k as usize
            } else {
                0
            };
            k.min(count - 1)
        };
        [
            at(p[0], self.origin[0], self.columns),
            at(p[1], self.origin[1], self.rows),
        ]
    }

    fn near(&self, p: Point) -> &[usize] {
        let [c, r] = self.index(p);
        &self.cells[r * self.columns + c]
    }
}

/// Whether two rectangles meet, within `slack`.
fn meet(a: &Rect, b: &Rect, slack: f64) -> bool {
    (0..2).all(|k| a.min[k] <= b.max[k] + slack && b.min[k] <= a.max[k] + slack)
}

/// The crossing of segments `p0 p1` and `q0 q1`, if they cross (ends included); parallel segments
/// give none, their ends being candidates of their own.
#[allow(clippy::float_cmp)] // Exactly parallel only.
fn crossing(p0: Point, p1: Point, q0: Point, q1: Point) -> Option<Point> {
    let (along_p, along_q) = (sub(p1, p0), sub(q1, q0));
    let denominator = cross(along_p, along_q);
    if denominator == 0.0 {
        return None;
    }
    let start = sub(q0, p0);
    let (at_p, at_q) = (
        cross(start, along_q) / denominator,
        cross(start, along_p) / denominator,
    );
    ((0.0..=1.0).contains(&at_p) && (0.0..=1.0).contains(&at_q))
        .then(|| add(p0, [along_p[0] * at_p, along_p[1] * at_p]))
}

/// The point of segment `p q` nearest `c`, when it lies strictly between the ends (the ends
/// being candidates of their own).
fn foot(c: Point, p: Point, q: Point) -> Option<Point> {
    let along = sub(q, p);
    let length = along[0] * along[0] + along[1] * along[1];
    if length <= 0.0 {
        return None;
    }
    let at = (sub(c, p)[0] * along[0] + sub(c, p)[1] * along[1]) / length;
    (at > 0.0 && at < 1.0).then(|| add(p, [along[0] * at, along[1] * at]))
}

/// The edges of `polygon` whose rectangles meet `within`.
fn edges_within(polygon: &NoFit, within: Rect) -> impl Iterator<Item = (Point, Point)> + '_ {
    polygon
        .segments
        .iter()
        .filter(move |(_, _, rect)| meet(rect, &within, OUTLINE_TOLERANCE_MM))
        .map(|&(p, q, _)| (p, q))
}

/// No live no-fit polygon: a candidate move's second source, or one that has none.
const NONE: usize = usize::MAX;

/// A candidate move, and the live no-fit polygons it lies on ([`NONE`] for none).
type Candidate = (Point, [usize; 2]);

/// The moves where a footprint might go: the centring `centred`, the corners of the inner-fit
/// rectangle `room`, and on each live no-fit polygon of `live` its vertices and its edges'
/// crossings with the edges of `room` and of the polygons near it in `grid` (a pair of live
/// polygons once). With `feet`, also the point of each live edge nearest `centred`, where the
/// free move nearest the middle may lie along an edge. Moves outside `room` are left out, those
/// within the tolerance moved onto it.
fn candidates(
    centred: Point,
    room: Rect,
    nofits: &[NoFit],
    (live, dead): (&[usize], &[bool]),
    grid: &Grid,
    feet: bool,
) -> Vec<Candidate> {
    let mut out: Vec<Candidate> = [
        centred,
        room.min,
        room.max,
        [room.min[0], room.max[1]],
        [room.max[0], room.min[1]],
    ]
    .into_iter()
    .map(|p| (p, [NONE; 2]))
    .collect();
    let walls = [
        (room.min, [room.max[0], room.min[1]]),
        ([room.max[0], room.min[1]], room.max),
        (room.max, [room.min[0], room.max[1]]),
        ([room.min[0], room.max[1]], room.min),
    ];
    let mut others: Vec<usize> = Vec::new();
    for &k in live {
        let a = &nofits[k];
        out.extend(a.points.iter().map(|&p| (p, [k, NONE])));
        for (p, q) in edges_within(a, room) {
            for &(w0, w1) in &walls {
                out.extend(crossing(p, q, w0, w1).map(|x| (x, [k, NONE])));
            }
            if feet {
                out.extend(foot(centred, p, q).map(|x| (x, [k, NONE])));
            }
        }
        // Its neighbours: every dead one, and the live ones after it, so a live pair is met once.
        others.clear();
        let ([c0, r0], [c1, r1]) = (grid.index(a.rect.min), grid.index(a.rect.max));
        for r in r0..=r1 {
            for c in c0..=c1 {
                others.extend(
                    grid.cells[r * grid.columns + c]
                        .iter()
                        .copied()
                        .filter(|&o| o != k && (dead[o] || o > k)),
                );
            }
        }
        others.sort_unstable();
        others.dedup();
        for &o in &others {
            let b = &nofits[o];
            if !meet(&a.rect, &b.rect, OUTLINE_TOLERANCE_MM) {
                continue;
            }
            let common = Rect {
                min: [
                    a.rect.min[0].max(b.rect.min[0]),
                    a.rect.min[1].max(b.rect.min[1]),
                ],
                max: [
                    a.rect.max[0].min(b.rect.max[0]),
                    a.rect.max[1].min(b.rect.max[1]),
                ],
            };
            let second = if dead[o] { NONE } else { o };
            for (p, q) in edges_within(a, common) {
                for (u, v) in edges_within(b, common) {
                    out.extend(crossing(p, q, u, v).map(|x| (x, [k, second])));
                }
            }
        }
    }
    let inside = |p: Point| {
        (0..2).all(|k| {
            p[k] >= room.min[k] - OUTLINE_TOLERANCE_MM && p[k] <= room.max[k] + OUTLINE_TOLERANCE_MM
        })
    };
    out.retain(|&(p, _)| inside(p));
    for (p, _) in &mut out {
        for k in 0..2 {
            p[k] = p[k].clamp(room.min[k], room.max[k]);
        }
    }
    out
}

/// Pack convex footprints into `area` (TD-165), at least `spacing_mm` from each other and from
/// every convex `obstacle`. Each footprint is given in one or more orientations, its convex,
/// counter-clockwise polygons, best first (the least turn): one of them is placed. Returns each
/// footprint's orientation and move, in input order.
///
/// **Order:** footprints by decreasing area (of their first orientation), then decreasing depth
/// and width of its rectangle, then input order.
///
/// **Choice:** for each footprint, each orientation's candidate moves (see the module) are taken
/// nearest the plate's middle first (then the lowest Y, then X), and the first free one is that
/// orientation's; the orientation landing nearest the middle wins, the first on a tie. A move is
/// free when the footprint stays within `area` and inside no footprint placed or obstacle, both
/// grown by half the spacing, within [`OUTLINE_TOLERANCE_MM`].
///
/// **Fallbacks:** when footprints are left out, the same placement from the front left corner
/// (centred again without obstacles), and their first orientations' rectangles packed by
/// [`arrange`](super::arrange); the one that leaves fewest out is taken, the middle's on a tie.
///
/// # Errors
/// [`Unplaced`] lists every footprint that fits nowhere; nothing is placed outside `area`.
pub fn arrange_outlines(
    shapes: &[Vec<Vec<[f64; 2]>>],
    area: Rect,
    obstacles: &[Vec<[f64; 2]>],
    spacing_mm: f64,
) -> Result<Vec<PlacedOutline>, Unplaced> {
    let forms = forms(shapes, spacing_mm);
    let (middle, left) = place(&forms, area, obstacles, spacing_mm, Aim::Middle, true);
    if left.is_empty() {
        return Ok(middle);
    }
    // From the front left corner, rows nest as tightly as they can: centred again on the plate
    // when nothing else is on it.
    let (corner, corner_left) = place(&forms, area, obstacles, spacing_mm, Aim::Corner, true);
    if corner_left.is_empty() {
        return Ok(if obstacles.is_empty() {
            centred(&forms, corner, area)
        } else {
            corner
        });
    }
    let fewest = if corner_left.len() < left.len() {
        corner_left
    } else {
        left
    };
    // The first orientations' rectangles: taken when they leave fewer out. A footprint with no
    // orientation fits nowhere either way.
    if forms.iter().any(Vec::is_empty) {
        return Err(Unplaced { items: fewest });
    }
    let sizes: Vec<[f64; 2]> = forms.iter().map(|f| f[0].body.size()).collect();
    let fixed: Vec<Rect> = obstacles.iter().map(|o| bounds(o)).collect();
    match arrange_with(&sizes, &vec![false; sizes.len()], area, &fixed, spacing_mm) {
        Ok(corners) => Ok(corners
            .into_iter()
            .zip(&forms)
            .map(|(p, form)| PlacedOutline {
                orientation: 0,
                offset: sub(p.corner, form[0].body.min),
            })
            .collect()),
        // Fewer left out by rectangles: the caller packs the others again without them.
        Err(boxes) if boxes.items.len() < fewest.len() => Err(boxes),
        Err(_) => Err(Unplaced { items: fewest }),
    }
}

/// `placed` moved as one so the rectangle around every footprint is centred on `area`.
fn centred(forms: &[Vec<Form>], placed: Vec<PlacedOutline>, area: Rect) -> Vec<PlacedOutline> {
    let mut around = Rect {
        min: [f64::INFINITY; 2],
        max: [f64::NEG_INFINITY; 2],
    };
    for (form, p) in forms.iter().zip(&placed) {
        let body = form[p.orientation].body;
        for k in 0..2 {
            around.min[k] = around.min[k].min(body.min[k] + p.offset[k]);
            around.max[k] = around.max[k].max(body.max[k] + p.offset[k]);
        }
    }
    if placed.is_empty() {
        return placed;
    }
    // Within the plate both ways, as the group fits it.
    let shift: Point = std::array::from_fn(|k| {
        let wanted =
            f64::midpoint(area.min[k], area.max[k]) - f64::midpoint(around.min[k], around.max[k]);
        wanted.clamp(area.min[k] - around.min[k], area.max[k] - around.max[k])
    });
    placed
        .into_iter()
        .map(|p| PlacedOutline {
            orientation: p.orientation,
            offset: add(p.offset, shift),
        })
        .collect()
}

/// Each footprint's orientations, ready to place.
fn forms(shapes: &[Vec<Vec<[f64; 2]>>], spacing_mm: f64) -> Vec<Vec<Form>> {
    shapes
        .iter()
        .map(|orientations| {
            orientations
                .iter()
                .map(|polygon| {
                    let body = bounded(polygon, MAX_OUTLINE_VERTICES);
                    Form {
                        area: area(&body),
                        body: bounds(&body),
                        grown: grown(&body, spacing_mm / 2.0),
                    }
                })
                .collect()
        })
        .collect()
}

/// What one form (a footprint's orientation) keeps between footprints of the same shape, as
/// copies are: its no-fit polygon with each footprint placed so far, and which of them are dead,
/// their whole boundary inside the others' so no move on them is ever free again. Footprints
/// placed only add to the others, so a dead polygon stays dead.
#[derive(Default)]
struct FormState {
    nofits: Vec<NoFit>,
    dead: Vec<bool>,
}

/// The bits of a form's rectangle and grown polygon: equal for footprints of the same shape.
fn form_key(form: &Form) -> Vec<u64> {
    form.body
        .min
        .iter()
        .chain(&form.body.max)
        .chain(form.grown.iter().flatten())
        .map(|v| v.to_bits())
        .collect()
}

/// Where [`place`] puts each footprint first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Aim {
    /// Nearest the plate's middle.
    Middle,
    /// Lowest, then leftmost, from the plate's front left corner: rows along the front edge,
    /// each nesting into the one before.
    Corner,
}

/// How much more a millimetre of height weighs than one of width in [`Aim::Corner`]: more than
/// any plate is wide, so the lowest always comes first.
const CORNER_WEIGHT: f64 = 1e6;

/// [`arrange_outlines`]' placement, without its fallbacks: each footprint's orientation and move,
/// and those left out, increasing. With `cache`, forms that recur keep their [`FormState`]; the
/// result is the same without it, only slower.
fn place(
    forms: &[Vec<Form>],
    area: Rect,
    obstacles: &[Vec<[f64; 2]>],
    spacing_mm: f64,
    aim: Aim,
    cache: bool,
) -> (Vec<PlacedOutline>, Vec<usize>) {
    let middle: Point = std::array::from_fn(|k| f64::midpoint(area.min[k], area.max[k]));
    let mut order: Vec<usize> = (0..forms.len()).collect();
    let first_area = |i: usize| forms[i].first().map_or(0.0, |f| f.area);
    let first_size = |i: usize| forms[i].first().map_or([0.0; 2], |f| f.body.size());
    order.sort_by(|&a, &b| {
        first_area(b)
            .total_cmp(&first_area(a))
            .then(first_size(b)[1].total_cmp(&first_size(a)[1]))
            .then(first_size(b)[0].total_cmp(&first_size(a)[0]))
            .then(a.cmp(&b))
    });
    // Forms that recur keep their state; one used once is worked out and dropped.
    let keys: Vec<Vec<Vec<u64>>> = forms
        .iter()
        .map(|orientations| orientations.iter().map(form_key).collect())
        .collect();
    let mut uses: HashMap<&[u64], usize> = HashMap::new();
    for key in keys.iter().flatten() {
        *uses.entry(key.as_slice()).or_default() += 1;
    }
    let mut states: HashMap<&[u64], FormState> = HashMap::new();
    // Obstacles and footprints placed, grown by half the spacing.
    let mut placed: Vec<Vec<Point>> = obstacles
        .iter()
        .map(|o| grown(&bounded(o, MAX_OUTLINE_VERTICES), spacing_mm / 2.0))
        .collect();
    let mut result = vec![
        PlacedOutline {
            orientation: 0,
            offset: [0.0; 2],
        };
        forms.len()
    ];
    let mut unplaced = Vec::new();
    for &i in &order {
        // The distance from the middle, the orientation and the move.
        let mut best: Option<(f64, usize, Point)> = None;
        for (j, form) in forms[i].iter().enumerate() {
            let key = keys[i][j].as_slice();
            let mut once = FormState::default();
            let state = if cache && uses[key] > 1 {
                states.entry(key).or_default()
            } else {
                &mut once
            };
            if let Some((d, t)) = nearest_free(form, state, area, middle, &placed, aim) {
                if best.is_none_or(|(b, _, _)| d < b) {
                    best = Some((d, j, t));
                }
            }
        }
        match best {
            Some((_, j, t)) => {
                result[i] = PlacedOutline {
                    orientation: j,
                    offset: t,
                };
                placed.push(translated(&forms[i][j].grown, t));
            }
            None => unplaced.push(i),
        }
    }
    unplaced.sort_unstable();
    (result, unplaced)
}

/// The free move of `form` nearest the plate's `middle` among its candidates, clear of the
/// grown polygons `placed`, with its squared distance; `None` when it fits nowhere. `state`
/// keeps the form's no-fit polygons and marks dead those whose every candidate was tried and
/// taken.
fn nearest_free(
    form: &Form,
    state: &mut FormState,
    area: Rect,
    middle: Point,
    placed: &[Vec<Point>],
    aim: Aim,
) -> Option<(f64, Point)> {
    let room = Rect {
        min: sub(area.min, form.body.min),
        max: sub(area.max, form.body.max),
    };
    if (0..2).any(|k| room.min[k] > room.max[k] + OUTLINE_TOLERANCE_MM) {
        return None;
    }
    let room = Rect {
        min: room.min,
        max: [room.max[0].max(room.min[0]), room.max[1].max(room.min[1])],
    };
    let centre = [
        f64::midpoint(form.body.min[0], form.body.max[0]),
        f64::midpoint(form.body.min[1], form.body.max[1]),
    ];
    if state.nofits.len() < placed.len() {
        let negated: Vec<Point> = form.grown.iter().map(|p| [-p[0], -p[1]]).collect();
        for p in &placed[state.nofits.len()..] {
            state.nofits.push(NoFit::new(minkowski(p, &negated)));
            state.dead.push(false);
        }
    }
    let nofits = &state.nofits;
    let near: Vec<usize> = (0..nofits.len())
        .filter(|&k| meet(&nofits[k].rect, &room, OUTLINE_TOLERANCE_MM))
        .collect();
    let live: Vec<usize> = near.iter().copied().filter(|&k| !state.dead[k]).collect();
    let grid = Grid::new(room, nofits, &near);
    let distance = |t: Point| match aim {
        Aim::Middle => {
            let [dx, dy] = sub(add(t, centre), middle);
            dx * dx + dy * dy
        }
        // The lowest first, then the leftmost: the footprint's lowest corner, its height above
        // the plate's front edge weighing far more than its distance from the left edge.
        Aim::Corner => {
            let low = add(t, form.body.min);
            (low[1] - area.min[1]) * CORNER_WEIGHT + (low[0] - area.min[0])
        }
    };
    let mut moves: Vec<(f64, Candidate)> = candidates(
        sub(middle, centre),
        room,
        nofits,
        (&live, &state.dead),
        &grid,
        aim == Aim::Middle,
    )
    .into_iter()
    .map(|c| (distance(c.0), c))
    .collect();
    moves.sort_unstable_by(|a, b| {
        a.0.total_cmp(&b.0)
            .then(a.1 .0[1].total_cmp(&b.1 .0[1]))
            .then(a.1 .0[0].total_cmp(&b.1 .0[0]))
    });
    // Each live polygon's candidates not yet tried and taken.
    let mut open: HashMap<usize, usize> = live.iter().map(|&k| (k, 0)).collect();
    for (_, (_, sources)) in &moves {
        for source in sources.iter().filter(|&&s| s != NONE) {
            *open.entry(*source).or_default() += 1;
        }
    }
    let mut found = None;
    for &(d, (t, sources)) in &moves {
        let free = grid.near(t).iter().all(|&k| {
            let n = &nofits[k];
            !meet(&n.rect, &Rect { min: t, max: t }, 0.0) || n.depth(t) <= OUTLINE_TOLERANCE_MM
        });
        if free {
            found = Some((d, t));
            break;
        }
        for source in sources.iter().filter(|&&s| s != NONE) {
            if let Some(n) = open.get_mut(source) {
                *n -= 1;
            }
        }
    }
    for (k, n) in open {
        if n == 0 {
            state.dead[k] = true;
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A regular polygon of `n` sides and circumradius `r` centred at `c`, counter-clockwise.
    fn regular(n: u32, r: f64, c: Point) -> Vec<Point> {
        (0..n)
            .map(|k| {
                let (sin, cos) = (std::f64::consts::TAU * f64::from(k) / f64::from(n)).sin_cos();
                [c[0] + r * cos, c[1] + r * sin]
            })
            .collect()
    }

    fn square(min: Point, side: f64) -> Vec<Point> {
        vec![
            min,
            [min[0] + side, min[1]],
            [min[0] + side, min[1] + side],
            [min[0], min[1] + side],
        ]
    }

    /// Whether convex `inner` lies within convex `outer`, within `slack`.
    fn contains(outer: &[Point], inner: &[Point], slack: f64) -> bool {
        let n = outer.len();
        inner.iter().all(|&p| {
            (0..n).all(|k| {
                let (a, b) = (outer[k], outer[(k + 1) % n]);
                let e = sub(b, a);
                cross(e, sub(p, a)) / e[0].hypot(e[1]) >= -slack
            })
        })
    }

    #[test]
    fn minkowski_sums_add_their_shapes() {
        let sum = minkowski(&square([0.0, 0.0], 2.0), &square([1.0, 1.0], 3.0));
        assert_eq!(
            bounds(&sum),
            Rect {
                min: [1.0, 1.0],
                max: [6.0, 6.0]
            }
        );
        assert!((area(&sum) - 25.0).abs() < 1e-12);
        // A triangle and its reflection make a hexagon of six times its area.
        let triangle = vec![[0.0, 0.0], [2.0, 0.0], [0.0, 2.0]];
        let reflected: Vec<Point> = triangle.iter().map(|p| [-p[0], -p[1]]).collect();
        let hexagon = minkowski(&triangle, &reflected);
        assert_eq!(hexagon.len(), 6);
        assert!((area(&hexagon) - 6.0 * area(&triangle)).abs() < 1e-12);
        // A segment sums point by point.
        let segment = minkowski(&square([0.0, 0.0], 1.0), &[[0.0, 0.0], [3.0, 0.0]]);
        assert!((area(&segment) - 4.0).abs() < 1e-12);
    }

    #[test]
    fn bounding_keeps_few_vertices_and_contains_the_polygon() {
        let circle = regular(400, 10.0, [3.0, -2.0]);
        let bound = bounded(&circle, MAX_OUTLINE_VERTICES);
        assert_eq!(bound.len(), MAX_OUTLINE_VERTICES);
        assert!(contains(&bound, &circle, 1e-9));
        assert!(area(&bound) > 0.0);
        // A regular 32-gon round the circle adds about 0.6 %; the greedy one stays near it.
        let added = area(&bound) / area(&circle) - 1.0;
        assert!(added < 0.015, "{added}");
        // Few vertices are kept as they are.
        let small = regular(12, 4.0, [0.0, 0.0]);
        assert_eq!(bounded(&small, MAX_OUTLINE_VERTICES), small);
    }

    #[test]
    fn growing_contains_every_point_within_the_radius() {
        let grown = grown(&square([0.0, 0.0], 4.0), 1.0);
        let circle = regular(720, 1.0, [0.0, 0.0]);
        for corner in square([0.0, 0.0], 4.0) {
            assert!(contains(&grown, &translated(&circle, corner), 1e-9));
        }
        assert_eq!(
            super::grown(&square([0.0, 0.0], 4.0), 0.0),
            square([0.0, 0.0], 4.0)
        );
    }

    #[test]
    fn overlaps_need_more_than_the_tolerance() {
        let a = square([0.0, 0.0], 2.0);
        assert!(outlines_overlap(&a, &square([1.0, 1.0], 2.0)));
        // Touching, or overlapping by less than the tolerance: clear.
        assert!(!outlines_overlap(&a, &square([2.0, 0.0], 2.0)));
        assert!(!outlines_overlap(&a, &square([2.0 - 0.5e-3, 0.0], 2.0)));
        assert!(outlines_overlap(&a, &square([2.0 - 2e-3, 0.0], 2.0)));
        // Rectangles that overlap while the triangles in them do not.
        let lower = vec![[0.0, 0.0], [4.0, 0.0], [0.0, 4.0]];
        let upper = vec![[4.0, 0.5], [4.0, 4.5], [0.5, 4.5]];
        assert!(!outlines_overlap(&lower, &upper));
        assert_eq!(
            overlapping_outlines(&[lower, upper, square([1.0, 1.0], 1.0)]),
            [(0, 2)]
        );
    }

    #[test]
    fn quarter_turns_are_exact() {
        let p = vec![[1.25, 0.5], [0.0, 3.0]];
        assert_eq!(turned(&p, 90.0), vec![[-0.5, 1.25], [-3.0, 0.0]]);
        assert_eq!(turned(&p, -90.0), vec![[0.5, -1.25], [3.0, 0.0]]);
        assert_eq!(turned(&p, 180.0), vec![[-1.25, -0.5], [-0.0, -3.0]]);
        let t = turned(&p, 30.0);
        assert!(
            (t[1][0] + 1.5).abs() < 1e-12
                && (t[1][1] - 3.0 * 30f64.to_radians().cos()).abs() < 1e-12
        );
    }

    /// Every placed outline within `area` and at least `spacing` from the others and the
    /// obstacles (within the tolerance), the polygons as given.
    fn check(
        shapes: &[Vec<Vec<Point>>],
        placed: &[PlacedOutline],
        area: Rect,
        obstacles: &[Vec<Point>],
        spacing: f64,
    ) {
        let outlines: Vec<Vec<Point>> = placed
            .iter()
            .zip(shapes)
            .map(|(p, s)| translated(&s[p.orientation], p.offset))
            .collect();
        for o in &outlines {
            let r = bounds(o);
            for k in 0..2 {
                assert!(r.min[k] >= area.min[k] - 1e-9 && r.max[k] <= area.max[k] + 1e-9);
            }
        }
        // Grown by an octagon within the circle of just under half the spacing (its sides at
        // `r cos(π / 8)`), two outlines that far apart must not overlap.
        let within = (spacing / 2.0 - OUTLINE_TOLERANCE_MM).max(0.0)
            * (std::f64::consts::PI / 8.0).cos()
            * (std::f64::consts::PI / 8.0).cos();
        let apart =
            |a: &[Point], b: &[Point]| !outlines_overlap(&grown(a, within), &grown(b, within));
        for (i, a) in outlines.iter().enumerate() {
            for b in &outlines[i + 1..] {
                assert!(apart(a, b));
            }
            for o in obstacles {
                assert!(apart(a, o));
            }
        }
    }

    fn plate() -> Rect {
        Rect {
            min: [0.0, 0.0],
            max: [100.0, 60.0],
        }
    }

    /// The footprints of `shapes` that `area` takes, packed as Arrange packs them: those left out
    /// dropped and the rest packed again until all fit. Returns their indices and placements.
    fn packed_all(
        shapes: &[Vec<Vec<Point>>],
        area: Rect,
        spacing: f64,
    ) -> (Vec<usize>, Vec<PlacedOutline>) {
        let mut keep: Vec<usize> = (0..shapes.len()).collect();
        loop {
            let wanted: Vec<Vec<Vec<Point>>> = keep.iter().map(|&i| shapes[i].clone()).collect();
            match arrange_outlines(&wanted, area, &[], spacing) {
                Ok(placed) => {
                    check(&wanted, &placed, area, &[], spacing);
                    return (keep, placed);
                }
                Err(e) => {
                    for &k in e.items.iter().rev() {
                        keep.remove(k);
                    }
                }
            }
        }
    }

    #[test]
    fn round_bases_nest_more_than_their_squares() {
        // 25 mm discs 2 mm apart on a 218 x 123 mm plate (a 12K panel's): 8 x 4 = 32 squares; from
        // the corner, rows of 8 and 7 nest five deep, 38.
        let area = Rect {
            min: [0.0, 0.0],
            max: [218.0, 123.0],
        };
        let disc = regular(96, 12.5, [0.0, 0.0]);
        let count = 45;
        let squares = vec![[25.0, 25.0]; count];
        let boxes = super::super::arrange(&squares, area, &[], 2.0)
            .err()
            .map_or(count, |e| count - e.items.len());
        assert_eq!(boxes, 32);
        let (kept, placed) = packed_all(&vec![vec![disc]; count], area, 2.0);
        assert_eq!(kept.len(), 38);
        // Centred on the plate again.
        let lows: Vec<f64> = placed.iter().map(|p| p.offset[0] - 12.5).collect();
        let left = lows.iter().copied().fold(f64::INFINITY, f64::min);
        let right = placed
            .iter()
            .map(|p| p.offset[0] + 12.5)
            .fold(f64::NEG_INFINITY, f64::max);
        assert!((left + right - 218.0).abs() < 1e-9, "{left} {right}");
        // When they all fit from the middle, they stay gathered there.
        let few = vec![vec![regular(96, 12.5, [0.0, 0.0])]; 3];
        let middle = arrange_outlines(&few, area, &[], 2.0).unwrap();
        assert!(middle.iter().all(|p| (p.offset[0] - 109.0).abs() < 30.0));
    }

    #[test]
    fn triangles_share_their_room_turned_half_round() {
        // Right triangles 19 mm on their sides on a 110 x 22 mm strip: one way, five fit across;
        // turned half round too, pairs share their squares, so nearly twice as many fit.
        let triangle = vec![[0.0, 0.0], [19.0, 0.0], [0.0, 19.0]];
        let turned_half = turned(&triangle, 180.0);
        let area = Rect {
            min: [0.0, 0.0],
            max: [110.0, 22.0],
        };
        let (one_way, _) = packed_all(&vec![vec![triangle.clone()]; 10], area, 1.0);
        assert_eq!(one_way.len(), 5);
        let both_ways = vec![vec![triangle.clone(), turned_half]; 10];
        let (kept, placed) = packed_all(&both_ways, area, 1.0);
        assert!(kept.len() >= 8, "{}", kept.len());
        assert!(placed.iter().any(|p| p.orientation == 1));
    }

    #[test]
    fn random_outlines_stay_apart_and_on_the_plate() {
        // A deterministic generator: convex hulls of a few points each.
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            #[allow(clippy::cast_precision_loss)]
            let v = (seed >> 11) as f64 / (1u64 << 53) as f64;
            v
        };
        let shapes: Vec<Vec<Vec<Point>>> = (0..40)
            .map(|_| {
                let points: Vec<Point> = (0..7).map(|_| [next() * 14.0, next() * 9.0]).collect();
                let hull = convex_hull(&points);
                vec![hull.clone(), turned(&hull, 90.0)]
            })
            .collect();
        let obstacles = vec![regular(20, 8.0, [50.0, 30.0])];
        let placed = arrange_outlines(&shapes, plate(), &obstacles, 2.0);
        let placed = match placed {
            Ok(p) => p,
            Err(e) => panic!("{} left out", e.items.len()),
        };
        check(&shapes, &placed, plate(), &obstacles, 2.0);
        // The same input, the same result.
        assert_eq!(
            arrange_outlines(&shapes, plate(), &obstacles, 2.0).unwrap(),
            placed
        );
    }

    #[test]
    fn never_fewer_than_rectangles() {
        // Squares packed edge to edge on a plate just their size: outlines and rectangles agree.
        let shapes = vec![vec![square([0.0, 0.0], 10.0)]; 6];
        let area = Rect {
            min: [0.0, 0.0],
            max: [32.0, 21.0],
        };
        let placed = arrange_outlines(&shapes, area, &[], 1.0).unwrap();
        check(&shapes, &placed, area, &[], 1.0);
        // One more than fits leaves exactly one out.
        let more = vec![vec![square([0.0, 0.0], 10.0)]; 7];
        assert_eq!(
            arrange_outlines(&more, area, &[], 1.0)
                .unwrap_err()
                .items
                .len(),
            1
        );
    }

    #[test]
    fn keeping_forms_changes_nothing() {
        // Copies of three shapes, two ways round each, with an obstacle: the dead polygons a form
        // skips held no free move.
        let shapes: Vec<Vec<Vec<Point>>> = (0..36)
            .map(|k| {
                let polygon = match k % 3 {
                    0 => regular(40, 4.0, [0.0, 0.0]),
                    1 => vec![[0.0, 0.0], [9.0, 0.0], [0.0, 7.0]],
                    _ => square([0.0, 0.0], 6.0),
                };
                vec![polygon.clone(), turned(&polygon, 180.0)]
            })
            .collect();
        let obstacles = vec![regular(12, 6.0, [40.0, 30.0])];
        let forms = forms(&shapes, 1.5);
        for aim in [Aim::Middle, Aim::Corner] {
            let kept = place(&forms, plate(), &obstacles, 1.5, aim, true);
            assert_eq!(kept, place(&forms, plate(), &obstacles, 1.5, aim, false));
            assert_eq!(kept.1, Vec::<usize>::new());
        }
    }

    #[test]
    #[ignore = "a measurement: run with --ignored --nocapture in release"]
    fn arrange_outlines_is_timed_on_many_footprints() {
        // A 218 x 123 mm plate (a 12K panel's), round and pointed footprints of mixed sizes.
        let area = Rect {
            min: [0.0, 0.0],
            max: [218.0, 123.0],
        };
        for (count, side) in [(50, 12.0), (100, 9.0), (200, 6.0), (400, 4.0)] {
            let shapes: Vec<Vec<Vec<Point>>> = (0..count)
                .map(|k: i32| {
                    let r = f64::midpoint(side, f64::from(k % 3));
                    let polygon = if k % 2 == 0 {
                        regular(48, r, [0.0, 0.0])
                    } else {
                        vec![[-r, -r], [r, -r], [0.0, r]]
                    };
                    vec![polygon.clone(), turned(&polygon, 180.0)]
                })
                .collect();
            let copies = vec![shapes[0].clone(); usize::try_from(count).unwrap_or(0)];
            let full =
                vec![vec![regular(48, 6.0, [0.0, 0.0])]; usize::try_from(count).unwrap_or(0)];
            for (kind, shapes) in [
                ("mixed", &shapes),
                ("copies", &copies),
                ("12 mm discs", &full),
            ] {
                let start = std::time::Instant::now();
                let result = arrange_outlines(shapes, area, &[], 2.0);
                eprintln!(
                    "{count} {kind} outlines of {side} mm: {:.1} ms, {} left out",
                    start.elapsed().as_secs_f64() * 1000.0,
                    result.err().map_or(0, |e| e.items.len())
                );
            }
        }
    }
}
