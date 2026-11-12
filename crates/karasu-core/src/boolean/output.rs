//! The result as a closed triangle mesh (TD-140, phases 3 to 5).
//!
//! The arrangement's faces are convex polygons with exact vertices ([`super::arrange`]). They
//! become a triangle mesh in these steps:
//! 1. **One vertex per point.** Vertices are grouped by their nearest grid point and compared
//!    exactly within a group, so two faces' vertices are the same vertex exactly when they are the
//!    same point.
//! 2. **Cuts undone.** Two faces in one plane facing one way (cut from one input triangle by a
//!    cell wall or by a plane that crossed it where no surface does, or from two input triangles
//!    in one plane), with an edge between them, are joined while their union is convex.
//! 3. **No T-junction.** A vertex of one face may lie inside another face's edge (where a face was
//!    cut and its neighbour was not, or not there). Only an edge used more often one way than the
//!    other can hold one, and only a vertex at an end of such an edge can lie inside one: a BVH
//!    of those edges and vertices pairs each edge with the vertices within two steps of it, each
//!    tested exactly (on the edge's two planes, strictly between its ends) and inserted in order.
//!    Afterwards every edge is used once in each direction. The faces are joined again where
//!    their edges now match, and a vertex lying on a straight side of every face it belongs to is
//!    left out of all of them.
//! 4. **Triangles.** A face, convex with some vertices on its straight sides, is cut into
//!    triangles by clipping strictly convex corners, those next to a vertex on a side first, so no
//!    triangle is flat and every boundary vertex is used.
//! 5. **Vertices only cells made, taken out** ([`decimate`]): a point the Boolean made whose
//!    faces all lie in one plane, or in two planes either side of a straight line through it, is
//!    taken out and its polygon triangulated again in the same plane: the surface is the same, and
//!    such a point (where a cell wall met a face around a hole) is never rounded.
//! 6. **Rounding** ([`round`]). Each point the Boolean made goes to its nearest grid point.
//!    Points rounding to the same grid point become one vertex; a triangle whose corners round to
//!    fewer than three points is dropped, which keeps every edge balanced. Rounding can fold a
//!    sliver: the pairs of triangles that then meet where they should not are found exactly
//!    ([`crossing_pairs`]), and [`super::solve`] resolves them (phase 5).
mod decimate;

use super::{
    arrange::{ArrangeError, Arranged, Face},
    grid::GridPoint,
    int::Int,
    plane::{Plane, Point},
};
use crate::{
    cancellation::{Cancellation, Cancelled, Checkpoint},
    preparation::sampled::{
        bvh::{BuildFailure, Bvh},
        distance::Triangle,
    },
};
use std::{cmp::Ordering, collections::HashMap};

/// The result rounded to the grid, with what rounding did.
#[derive(Clone, Debug, Default)]
pub(crate) struct Output {
    pub(crate) vertices: Vec<GridPoint>,
    pub(crate) triangles: Vec<[u32; 3]>,
    /// Exact vertices not on the grid, moved to their nearest grid point.
    pub(crate) moved: usize,
    /// Triangles whose corners rounded to fewer than three points, left out.
    pub(crate) dropped: usize,
    /// Triangles left with three collinear corners after rounding.
    pub(crate) flat: usize,
    /// Whether each vertex was moved by rounding.
    pub(crate) moved_at: Vec<bool>,
}

/// The exact triangulation before rounding: the distinct points and the triangles on them,
/// facing out of the result.
pub(crate) struct Exact {
    pub(crate) points: Vec<Point>,
    /// Each point's nearest grid point.
    pub(crate) rounded: Vec<GridPoint>,
    pub(crate) triangles: Vec<[u32; 3]>,
}

/// A face as a ring of point indices, with the plane of each of its edges.
#[derive(Clone, Debug)]
struct Ring {
    ids: Vec<u32>,
    /// Edge `i` runs from `ids[i]` to `ids[i + 1]` on `edges[i]`.
    edges: Vec<Plane>,
    support: Plane,
    untouched: bool,
    source: u32,
}

impl Ring {
    /// The axis the support's normal leans on most: projecting along it keeps the plane's lines.
    fn axis(&self) -> usize {
        (0..3)
            .max_by_key(|&k| self.support.n[k].unsigned_abs())
            .unwrap_or(0)
    }
}

/// The faces as a closed triangle mesh on the grid.
#[cfg(test)]
pub(crate) fn mesh(faces: Arranged) -> Output {
    round(&exact(faces, &mut Cancellation::never()).expect("never cancelled"))
}

/// A triangle's vertex indices and its corners in millimetres.
type Corners = ([u32; 3], [[f64; 3]; 3]);

/// A mesh in millimetres for the crossing search ([`crossing_pairs`]): the rounded result's
/// grid points ([`grid_doubles`]), exact.
#[derive(Clone, Debug, Default)]
pub(crate) struct Doubles {
    pub(crate) vertices: Vec<[f64; 3]>,
    pub(crate) triangles: Vec<[u32; 3]>,
    /// Vertices rounding moved.
    pub(crate) rounded: usize,
    /// Whether each vertex was moved by rounding.
    pub(crate) rounded_at: Vec<bool>,
}

/// Rings a thread triangulates in one run.
const RUN: usize = 4_096;

/// A run's triangles, and each ring's plane with the number of its triangles.
type Run = (Vec<[u32; 3]>, Vec<(Plane, usize)>);

/// The distinct points, each one's nearest grid point, and the faces as rings on them.
type Identified = (Vec<Point>, Vec<GridPoint>, Vec<Ring>);

/// The faces triangulated exactly: shared points identified, walls undone, T-junctions resolved.
/// The arrangement is let go as soon as its points are identified: from there on, the rings hold
/// all that is needed.
///
/// # Errors
/// Cancellation, and a failed reservation in the search for T-junctions.
pub(crate) fn exact(faces: Arranged, cancel: &mut Cancellation<'_>) -> Result<Exact, ArrangeError> {
    let (points, rounded, mut rings) = identify(&faces, cancel)?;
    drop(faces);
    join_cut_faces(&points, &mut rings, cancel)?;
    resolve_junctions(&points, &rounded, &mut rings, cancel)?;
    // Edges that only now match vertex for vertex can join further.
    join_cut_faces(&points, &mut rings, cancel)?;
    drop_flat_vertices(&points, &mut rings);
    // Triangulated in runs of rings on every core, each ring's triangles with its plane in lowest
    // terms; then joined in ring order, the planes numbered in order of first use.
    let runs = rings.len().div_ceil(RUN);
    let mut made: Vec<Run> = vec![Default::default(); runs];
    crate::parallel::fill_costly_with(&mut made, |run| {
        let (mut triangles, mut planes) = (Vec::new(), Vec::new());
        for ring in rings.iter().skip(run * RUN).take(RUN) {
            let before = triangles.len();
            triangulate(ring, &points, &mut triangles);
            planes.push((canonical(&ring.support), triangles.len() - before));
        }
        (triangles, planes)
    });
    drop(rings);
    let mut triangles = Vec::with_capacity(made.iter().map(|m| m.0.len()).sum());
    // Each triangle's plane by index into `supports`.
    let mut planes = Vec::with_capacity(triangles.capacity());
    let mut supports = Vec::new();
    let mut index: HashMap<Plane, u32> = HashMap::new();
    for (run, ring_planes) in made {
        triangles.extend(run);
        for (plane, count) in ring_planes {
            let plane = *index.entry(plane).or_insert_with(|| {
                supports.push(plane);
                u32::try_from(supports.len() - 1).expect("fewer than 2^32 planes")
            });
            planes.extend(std::iter::repeat_n(plane, count));
        }
    }
    decimate::decimate(&points, &supports, &mut triangles, &mut planes);
    Ok(Exact {
        points,
        rounded,
        triangles,
    })
}

/// Every face as a ring of distinct points' indices, each point with its nearest grid point.
///
/// # Errors
/// Cancellation.
fn identify(arranged: &Arranged, cancel: &mut Cancellation<'_>) -> Result<Identified, Cancelled> {
    let mut points: Vec<Point> = Vec::new();
    let mut rounded = Vec::new();
    let mut made_on_grid = Vec::new();
    let mut groups: HashMap<GridPoint, Vec<u32>> = HashMap::new();
    let mut rings = Vec::with_capacity(arranged.faces.len());
    for (n, face) in arranged.faces.iter().enumerate() {
        cancel.poll(Checkpoint::BooleanMesh, n)?;
        let corners;
        let (vertices, edges, support, source): (&[Point], &[Plane], Plane, u32) = match face {
            Face::Part(polygon) => (
                &polygon.vertices,
                &polygon.edges,
                polygon.support,
                polygon.source,
            ),
            Face::Whole { index, flipped } => {
                let triangle = &arranged.triangles[*index as usize];
                corners = triangle.vertices();
                let support = if *flipped {
                    triangle.support.flipped()
                } else {
                    triangle.support
                };
                (&corners, &triangle.edges, support, *index)
            }
        };
        let mut ids = Vec::with_capacity(vertices.len());
        for vertex in vertices {
            let grid = vertex.as_grid();
            let key = grid.unwrap_or_else(|| vertex.rounded());
            let group = groups.entry(key).or_default();
            // Two points made as the group's grid point are that point; otherwise compare.
            let found = group.iter().find(|&&id| {
                (grid.is_some() && made_on_grid[id as usize]) || points[id as usize].same(vertex)
            });
            let id = if let Some(&id) = found {
                id
            } else {
                let id = u32::try_from(points.len()).expect("fewer than 2^32 vertices");
                points.push(*vertex);
                rounded.push(key);
                made_on_grid.push(grid.is_some());
                group.push(id);
                id
            };
            ids.push(id);
        }
        rings.push(Ring {
            ids,
            edges: edges.to_vec(),
            support,
            untouched: matches!(face, Face::Whole { .. }),
            source,
        });
    }
    Ok((points, rounded, rings))
}

/// The plane of `support` in lowest terms, its normal's direction kept: the same key for every
/// polygon in one plane facing one way, whatever triangle it comes from.
fn canonical(support: &Plane) -> Plane {
    fn gcd(mut a: u128, mut b: u128) -> u128 {
        while b != 0 {
            (a, b) = (b, a % b);
        }
        a
    }
    let g = support
        .n
        .iter()
        .map(|&v| u128::from(v.unsigned_abs()))
        .fold(support.d.unsigned_abs(), gcd);
    if g <= 1 {
        return *support;
    }
    // The normal is not zero, so neither is `g`; each quotient is no larger than its term.
    let g64 = i64::try_from(g).unwrap_or(i64::MAX);
    Plane {
        n: support.n.map(|v| v / g64),
        d: support.d / i128::try_from(g).unwrap_or(i128::MAX),
    }
}

/// Join again faces cut from one plane of the inputs (by a cell wall, by a plane that crossed
/// them where nothing does, or because two input triangles share it): the same plane facing the
/// same way, an edge shared in opposite directions, and a convex union. Repeated until no pair
/// joins; the joined face keeps the first's place. Whole untouched triangles are left as they
/// are.
///
/// Faces in different planes never join, so each plane's faces are joined on their own, the
/// planes shared among the cores ([`join_plane`]): the same joins as one queue over every face.
///
/// # Errors
/// Cancellation.
fn join_cut_faces(
    points: &[Point],
    rings: &mut Vec<Ring>,
    cancel: &mut Cancellation<'_>,
) -> Result<(), Cancelled> {
    // The cut rings of each plane, in index order.
    let mut planes: HashMap<Plane, usize> = HashMap::new();
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for (f, ring) in rings.iter().enumerate() {
        cancel.poll(Checkpoint::BooleanMesh, f)?;
        if ring.untouched {
            continue;
        }
        let next = groups.len();
        let group = *planes.entry(canonical(&ring.support)).or_insert(next);
        if group == next {
            groups.push(Vec::new());
        }
        groups[group].push(f);
    }
    drop(planes);
    groups.retain(|group| group.len() > 1);
    let mut joined: Vec<Vec<(usize, Option<Ring>)>> = vec![Vec::new(); groups.len()];
    crate::parallel::fill_balanced_cancellable(
        &mut joined,
        cancel,
        Checkpoint::BooleanMesh,
        |g, halted| join_plane(points, rings, &groups[g], halted),
    )?;
    let mut dead = vec![false; rings.len()];
    for (f, ring) in joined.into_iter().flatten() {
        match ring {
            Some(ring) => rings[f] = ring,
            None => dead[f] = true,
        }
    }
    let mut keep = dead.iter().map(|d| !d);
    rings.retain(|_| keep.next().unwrap_or(true));
    Ok(())
}

/// The joins among the rings `members` of one plane (indices into `rings`, ascending): each
/// ring a union replaced, with its union, and each ring joined into another, with `None`. Rings
/// in index order, a union queued again to try its new edges: the same joins on every run. Cut
/// short, to be discarded, once `halted` is raised.
fn join_plane(
    points: &[Point],
    rings: &[Ring],
    members: &[usize],
    halted: &std::sync::atomic::AtomicBool,
) -> Vec<(usize, Option<Ring>)> {
    let mut local: Vec<Ring> = members.iter().map(|&f| rings[f].clone()).collect();
    let mut changed = vec![false; local.len()];
    let mut dead = vec![false; local.len()];
    let key = |u: u32, v: u32| (u.min(v), u.max(v));
    // The live rings holding each edge.
    let mut holders: HashMap<(u32, u32), Vec<usize>> = HashMap::new();
    for (f, ring) in local.iter().enumerate() {
        let ids = &ring.ids;
        for i in 0..ids.len() {
            holders
                .entry(key(ids[i], ids[(i + 1) % ids.len()]))
                .or_default()
                .push(f);
        }
    }
    let mut queue: std::collections::VecDeque<usize> = (0..local.len()).collect();
    while let Some(f) = queue.pop_front() {
        if halted.load(std::sync::atomic::Ordering::Relaxed) {
            break;
        }
        if dead[f] {
            continue;
        }
        let Some((g, union)) = (0..local[f].ids.len()).find_map(|i| {
            let ids = &local[f].ids;
            let (u, v) = (ids[i], ids[(i + 1) % ids.len()]);
            holders.get(&key(u, v))?.iter().find_map(|&g| {
                if g == f || dead[g] {
                    return None;
                }
                let other = &local[g].ids;
                let j = (0..other.len())
                    .find(|&j| other[j] == v && other[(j + 1) % other.len()] == u)?;
                // The union takes the lower index's place.
                if g < f {
                    join(points, &local[g], j, &local[f], i).map(|r| (g, r))
                } else {
                    join(points, &local[f], i, &local[g], j).map(|r| (g, r))
                }
            })
        }) else {
            continue;
        };
        let (kept, gone) = (f.min(g), f.max(g));
        for ring in [kept, gone] {
            let ids = &local[ring].ids;
            for i in 0..ids.len() {
                if let Some(list) = holders.get_mut(&key(ids[i], ids[(i + 1) % ids.len()])) {
                    list.retain(|&h| h != ring);
                }
            }
        }
        local[kept] = union;
        changed[kept] = true;
        dead[gone] = true;
        let ids = &local[kept].ids;
        for i in 0..ids.len() {
            holders
                .entry(key(ids[i], ids[(i + 1) % ids.len()]))
                .or_default()
                .push(kept);
        }
        queue.push_back(kept);
    }
    local
        .into_iter()
        .enumerate()
        .filter(|&(f, _)| dead[f] || changed[f])
        .map(|(f, ring)| (members[f], (!dead[f]).then_some(ring)))
        .collect()
}

/// The union of ring `a`, whose edge `i` runs `u → v`, and ring `b`, whose edge `j` runs
/// `v → u`, when it is convex.
fn join(points: &[Point], a: &Ring, i: usize, b: &Ring, j: usize) -> Option<Ring> {
    let (na, nb) = (a.ids.len(), b.ids.len());
    // a from v round to u, then b from u round to v, the shared edge left out.
    let mut ids = Vec::with_capacity(na + nb - 2);
    let mut edges = Vec::with_capacity(na + nb - 2);
    for k in 1..na {
        ids.push(a.ids[(i + k) % na]);
        edges.push(a.edges[(i + k) % na]);
    }
    for k in 1..nb {
        ids.push(b.ids[(j + k) % nb]);
        edges.push(b.edges[(j + k) % nb]);
    }
    // ids: v .. u (from a), then u's successor in b .. v's predecessor; edges[k] from ids[k].
    // When the two shared more than that edge (a run along the wall), the union goes out and
    // back along the rest: `x → y → x` is closed up, `y` and the second `x` left out.
    loop {
        let n = ids.len();
        let Some(k) = (0..n).find(|&k| n > 3 && ids[(k + n - 1) % n] == ids[(k + 1) % n]) else {
            break;
        };
        // Remove y (at k) and the repeated x (at k + 1); x keeps the edge leaving the second x.
        let after = (k + 1) % n;
        let leaving = edges[after];
        let before = (k + n - 1) % n;
        edges[before] = leaving;
        let (first, second) = if k < after { (k, after) } else { (after, k) };
        ids.remove(second);
        edges.remove(second);
        ids.remove(first);
        edges.remove(first);
    }
    let mut seen = ids.clone();
    seen.sort_unstable();
    if seen.windows(2).any(|w| w[0] == w[1]) || ids.len() < 3 {
        return None;
    }
    let joined = Ring {
        ids,
        edges,
        support: a.support,
        untouched: a.untouched && b.untouched,
        source: a.source,
    };
    let axis = joined.axis();
    let n = joined.ids.len();
    let orientation = (0..n)
        .map(|k| {
            turn(
                points,
                joined.ids[(k + n - 1) % n],
                joined.ids[k],
                joined.ids[(k + 1) % n],
                axis,
            )
        })
        .collect::<Vec<_>>();
    let sign = orientation
        .iter()
        .copied()
        .find(|t| *t != Ordering::Equal)?;
    // Convex: every corner turns one way or goes straight on, never back (a spike, where two
    // pieces met along a run whose vertices did not yet match).
    (0..n)
        .all(|k| {
            orientation[k] == sign
                || orientation[k] == Ordering::Equal
                    && !reverses(
                        &points[joined.ids[(k + n - 1) % n] as usize],
                        &points[joined.ids[k] as usize],
                        &points[joined.ids[(k + 1) % n] as usize],
                    )
        })
        .then_some(joined)
}

/// Whether the path `a → b → c`, along one line, turns back at `b`: `b` is not strictly between
/// `a` and `c` on an axis where they differ.
fn reverses(a: &Point, b: &Point, c: &Point) -> bool {
    let Some(axis) = (0..3).find(|&k| a.compare_on(c, k) != Ordering::Equal) else {
        return true;
    };
    let forward = a.compare_on(c, axis);
    !(a.compare_on(b, axis) == forward && b.compare_on(c, axis) == forward)
}

/// For each edge without its reverse in `open`, the vertices at an end of such an edge that lie
/// within two steps of it ([`close`]): the candidates for lying inside it. Found through a BVH of
/// the edges' and the points' boxes, so a dense cluster of vertices costs only the edges near it.
///
/// # Errors
/// Cancellation, and a failed reservation for the BVH.
fn candidates(
    open: &HashMap<(u32, u32), i64>,
    rounded: &[GridPoint],
    cancel: &mut Cancellation<'_>,
) -> Result<HashMap<(u32, u32), Vec<u32>>, ArrangeError> {
    let mut edges: Vec<(u32, u32)> = open
        .iter()
        .filter(|(_, &balance)| balance != 0)
        .map(|(&key, _)| key)
        .collect();
    edges.sort_unstable();
    let mut points: Vec<u32> = edges.iter().flat_map(|&(u, v)| [u, v]).collect();
    points.sort_unstable();
    points.dedup();
    #[allow(clippy::cast_precision_loss)] // Grid coordinates below 2^26: exact in f64.
    let at = |id: u32| rounded[id as usize].coordinates().map(|c| c as f64);
    // Each edge's own box; each point's widened by two steps on every axis.
    let items: Vec<Triangle> = edges
        .iter()
        .map(|&(u, v)| Triangle::new([at(u), at(v), at(v)]))
        .chain(points.iter().map(|&p| {
            let c = at(p);
            Triangle::new([c.map(|x| x - 2.0), c.map(|x| x + 2.0), c])
        }))
        .collect();
    let bvh = Bvh::build(items, cancel, Checkpoint::BooleanMesh).map_err(|e| match e {
        BuildFailure::Cancelled(c) => ArrangeError::Cancelled(c),
        BuildFailure::Allocation => ArrangeError::Resource,
    })?;
    let count = edges.len();
    let near = bvh.pairs(cancel, Checkpoint::BooleanMesh, |x, y| {
        // The lower index first: an edge's is below `count`, a point's not.
        let (x, y) = (x as usize, y as usize);
        x < count && y >= count && {
            let (u, v) = edges[x];
            let p = points[y - count];
            p != u && p != v && close(rounded[p as usize], at(u), at(v))
        }
    })?;
    let mut pairs: Vec<(usize, u32)> = near
        .into_iter()
        .map(|(x, y)| (x as usize, points[y as usize - count]))
        .collect();
    pairs.sort_unstable();
    let mut found: HashMap<(u32, u32), Vec<u32>> = HashMap::new();
    for (edge, p) in pairs {
        found.entry(edges[edge]).or_default().push(p);
    }
    Ok(found)
}

/// The points among `near` strictly inside the edge `a → b` of `ring`, on its plane `edge`, in
/// order from `a`.
fn on_edge(
    points: &[Point],
    ring: &Ring,
    edge: &Plane,
    a: u32,
    b: u32,
    near: Vec<u32>,
) -> Vec<u32> {
    let (start, end) = (&points[a as usize], &points[b as usize]);
    // Order along the axis the edge runs along most, among those where its ends differ exactly
    // (rounded, a short edge may not differ at all).
    let (ea, eb) = (
        start.approximate().coordinates(),
        end.approximate().coordinates(),
    );
    let axis = (0..3)
        .filter(|&k| start.compare_on(end, k) != Ordering::Equal)
        .max_by(|&j, &k| (eb[j] - ea[j]).abs().total_cmp(&(eb[k] - ea[k]).abs()))
        .expect("an edge joins two distinct points");
    let forward = start.compare_on(end, axis);
    let mut inside: Vec<u32> = near
        .into_iter()
        .filter(|&id| {
            let p = &points[id as usize];
            ring.support.side(p) == Ordering::Equal
                && edge.side(p) == Ordering::Equal
                && start.compare_on(p, axis) == forward
                && p.compare_on(end, axis) == forward
        })
        .collect();
    inside.sort_by(|&x, &y| {
        let order = points[x as usize].compare_on(&points[y as usize], axis);
        if forward == Ordering::Less {
            order
        } else {
            order.reverse()
        }
    });
    inside
}

/// Whether grid point `p` lies within two steps of the segment from `a` to `b` (grid
/// coordinates). A point exactly on an exact edge rounds to within one step, on each axis, of the
/// segment between the edge's rounded ends, so this only ever passes over points off the edge;
/// the decision is [`on_edge`]'s, exact.
#[allow(clippy::cast_precision_loss)] // Grid coordinates below 2^26: exact in f64.
fn close(p: GridPoint, a: [f64; 3], b: [f64; 3]) -> bool {
    let p = p.coordinates().map(|c| c as f64);
    let d = [0, 1, 2].map(|k| b[k] - a[k]);
    let w = [0, 1, 2].map(|k| p[k] - a[k]);
    let length = d.iter().map(|v| v * v).sum::<f64>();
    let t = if length > 0.0 {
        (w.iter().zip(&d).map(|(w, d)| w * d).sum::<f64>() / length).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let distance = (0..3).map(|k| (w[k] - t * d[k]).powi(2)).sum::<f64>();
    // At most √3 steps away; four is far beyond that and any rounding of these sums.
    distance <= 16.0
}

/// Insert into every face's edges the vertices of cut faces that lie strictly inside them.
///
/// Only an edge without its reverse can hold one, and only a vertex at an end of such an edge can
/// lie inside one: where a vertex lies inside an edge, the faces holding it have edges along that
/// line which no face has the other way.
fn resolve_junctions(
    points: &[Point],
    rounded: &[GridPoint],
    rings: &mut [Ring],
    cancel: &mut Cancellation<'_>,
) -> Result<(), ArrangeError> {
    let mut open: HashMap<(u32, u32), i64> = HashMap::new();
    for ring in rings.iter() {
        let count = ring.ids.len();
        for i in 0..count {
            let (u, v) = (ring.ids[i], ring.ids[(i + 1) % count]);
            let (key, sign) = if u < v { ((u, v), 1) } else { ((v, u), -1) };
            *open.entry(key).or_default() += sign;
        }
    }
    let candidates = candidates(&open, rounded, cancel)?;
    if candidates.is_empty() {
        return Ok(());
    }
    for (n, ring) in rings.iter_mut().enumerate() {
        cancel.poll(Checkpoint::BooleanMesh, n)?;
        let count = ring.ids.len();
        let mut ids = Vec::new();
        let mut edges = Vec::new();
        let mut grew = false;
        for i in 0..count {
            let (a, b) = (ring.ids[i], ring.ids[(i + 1) % count]);
            if grew {
                ids.push(a);
                edges.push(ring.edges[i]);
            }
            let Some(near) = candidates.get(&(a.min(b), a.max(b))) else {
                continue;
            };
            let edge = ring.edges[i];
            let inside = on_edge(points, ring, &edge, a, b, near.clone());
            if inside.is_empty() {
                continue;
            }
            if !grew {
                // The first insertion: the ring so far, up to and including vertex i.
                grew = true;
                ids.extend_from_slice(&ring.ids[..=i]);
                edges.extend_from_slice(&ring.edges[..=i]);
            }
            for id in inside {
                ids.push(id);
                edges.push(edge);
            }
        }
        if grew {
            ring.ids = ids;
            ring.edges = edges;
        }
    }
    Ok(())
}

/// Leave out the vertices that lie on a straight side of every face they belong to. Every face
/// holding such a vertex holds it between the same two neighbours on one line, so leaving it out
/// of all of them keeps every edge matched.
fn drop_flat_vertices(points: &[Point], rings: &mut [Ring]) {
    let mut corner = vec![false; points.len()];
    for ring in rings.iter() {
        let count = ring.ids.len();
        if count == 3 {
            for &v in &ring.ids {
                corner[v as usize] = true;
            }
            continue;
        }
        let axis = ring.axis();
        for i in 0..count {
            let (previous, v, next) = (
                ring.ids[(i + count - 1) % count],
                ring.ids[i],
                ring.ids[(i + 1) % count],
            );
            if !corner[v as usize] && turn(points, previous, v, next, axis) != Ordering::Equal {
                corner[v as usize] = true;
            }
        }
    }
    for ring in rings.iter_mut() {
        if ring.ids.iter().all(|&id| corner[id as usize]) {
            continue;
        }
        let ids = std::mem::take(&mut ring.ids);
        let edges = std::mem::take(&mut ring.edges);
        for (id, edge) in ids.into_iter().zip(edges) {
            if corner[id as usize] {
                ring.ids.push(id);
                ring.edges.push(edge);
            }
        }
    }
}

/// The orientation of three points of a face, in the projection along `axis`: the sign of the
/// turn `a → b → c` (`Equal` when collinear).
fn turn(points: &[Point], a: u32, b: u32, c: u32, axis: usize) -> Ordering {
    let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
    // Three grid points: plain integers.
    if let (Some(pa), Some(pb), Some(pc)) = (
        points[a as usize].as_grid(),
        points[b as usize].as_grid(),
        points[c as usize].as_grid(),
    ) {
        let (pa, pb, pc) = (pa.coordinates(), pb.coordinates(), pc.coordinates());
        let d = |p: [i64; 3], k: usize| i128::from(p[k] - pa[k]);
        return (d(pb, u) * d(pc, v) - d(pb, v) * d(pc, u)).cmp(&0);
    }
    let [pa, pb, pc] = [a, b, c].map(|i| points[i as usize].homogeneous());
    // (xb wa − xa wb)(yc wa − ya wc) − (yb wa − ya wb)(xc wa − xa wc): the turn times the
    // positive wa² wb wc.
    let difference = |p: &([Int<4>; 3], Int<3>), k: usize| -> Int<7> {
        Int::<7>::product(&p.0[k], &pa.1).sub(&Int::product(&pa.0[k], &p.1))
    };
    let left: Int<14> = Int::product(&difference(&pb, u), &difference(&pc, v));
    let right: Int<14> = Int::product(&difference(&pb, v), &difference(&pc, u));
    left.compare(&right)
}

/// Triangulate one ring into `out`, facing as its support does.
fn triangulate(ring: &Ring, points: &[Point], out: &mut Vec<[u32; 3]>) {
    let axis = ring.axis();
    // A ring turning counterclockwise about the support's normal turns as the normal's component
    // along `axis` in the projection.
    let facing = ring.support.n[axis].cmp(&0);
    let emit = |out: &mut Vec<[u32; 3]>, t: [u32; 3], orientation: Ordering| {
        out.push(if orientation == facing {
            t
        } else {
            [t[0], t[2], t[1]]
        });
    };
    let ids = &ring.ids;
    if ids.len() == 3 {
        let orientation = turn(points, ids[0], ids[1], ids[2], axis);
        emit(out, [ids[0], ids[1], ids[2]], orientation);
        return;
    }
    let mut ids = ids.clone();
    let count = ids.len();
    let orientation = (0..count)
        .map(|i| {
            turn(
                points,
                ids[i],
                ids[(i + 1) % count],
                ids[(i + 2) % count],
                axis,
            )
        })
        .find(|t| *t != Ordering::Equal)
        .expect("a face has area");
    while ids.len() > 3 {
        let n = ids.len();
        let at = |i: usize| ids[i % n];
        let bend = |i: usize| turn(points, at(i + n - 1), at(i), at(i + 1), axis);
        // A strictly convex corner next to a vertex on a side first: clipping it never leaves a
        // collinear remainder.
        let chosen = (0..n)
            .find(|&i| {
                bend(i) == orientation
                    && (bend(i + n - 1) == Ordering::Equal || bend(i + 1) == Ordering::Equal)
            })
            .or_else(|| (0..n).find(|&i| bend(i) == orientation))
            .expect("a convex face has a strictly convex corner");
        emit(
            out,
            [at(chosen + n - 1), at(chosen), at(chosen + 1)],
            orientation,
        );
        ids.remove(chosen);
    }
    emit(out, [ids[0], ids[1], ids[2]], orientation);
}

/// The exact triangulation rounded to the grid.
pub(crate) fn round(exact: &Exact) -> Output {
    let mut index: HashMap<GridPoint, u32> = HashMap::new();
    let mut output = Output::default();
    let mut used = vec![false; exact.points.len()];
    for t in &exact.triangles {
        for &v in t {
            used[v as usize] = true;
        }
    }
    let mut map = vec![u32::MAX; exact.points.len()];
    for (i, (point, &g)) in exact.points.iter().zip(&exact.rounded).enumerate() {
        if !used[i] {
            continue;
        }
        let moved = point.as_grid().is_none() && !point.on_grid();
        output.moved += usize::from(moved);
        let next = u32::try_from(output.vertices.len()).expect("fewer than 2^32 vertices");
        map[i] = *index.entry(g).or_insert_with(|| {
            output.vertices.push(g);
            output.moved_at.push(false);
            next
        });
        output.moved_at[map[i] as usize] |= moved;
    }
    for t in &exact.triangles {
        let r = t.map(|v| map[v as usize]);
        if r[0] == r[1] || r[1] == r[2] || r[2] == r[0] {
            output.dropped += 1;
            continue;
        }
        let [a, b, c] = r.map(|v| output.vertices[v as usize].coordinates().map(i128::from));
        let (u, v) = (
            [0, 1, 2].map(|k| b[k] - a[k]),
            [0, 1, 2].map(|k| c[k] - a[k]),
        );
        let cross = [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ];
        if cross == [0; 3] {
            output.flat += 1;
        }
        output.triangles.push(r);
    }
    output
}

/// The pairs of triangles that meet other than along the vertices they share, among those with
/// a rounded vertex: what rounding may have made cross. Exact, on the grid points; as indices
/// into `out.triangles`, lower first, in an order fixed by the mesh.
///
/// # Errors
/// Cancellation, and a failed reservation in the search.
pub(crate) fn crossing_pairs(
    out: &Doubles,
    cancel: &mut crate::cancellation::Cancellation<'_>,
) -> Result<Vec<(u32, u32)>, super::arrange::ArrangeError> {
    use super::arrange::ArrangeError;
    use crate::{
        cancellation::Checkpoint,
        preparation::sampled::{
            bvh::{BuildFailure, Bvh},
            contact::{improper_contact, orient_projected},
            distance::Triangle,
        },
    };
    if out.rounded == 0 {
        return Ok(Vec::new());
    }
    // Flat triangles have no area to cross with (the caller counts them apart).
    let (faces, index): (Vec<Corners>, Vec<u32>) = (0_u32..)
        .zip(&out.triangles)
        .map(|(i, t)| ((*t, t.map(|v| out.vertices[v as usize])), i))
        .filter(|((_, [a, b, c]), _)| {
            (0..3).any(|k| orient_projected(*a, *b, *c, k) != Ordering::Equal)
        })
        .unzip();
    let triangles: Vec<Triangle> = faces.iter().map(|f| Triangle::new(f.1)).collect();
    let bvh = Bvh::build(triangles, cancel, Checkpoint::BooleanMesh).map_err(|e| match e {
        BuildFailure::Cancelled(c) => ArrangeError::Cancelled(c),
        BuildFailure::Allocation => ArrangeError::Resource,
    })?;
    let rounded = |i: usize| faces[i].0.iter().any(|&v| out.rounded_at[v as usize]);
    let crossing = bvh.pairs(cancel, Checkpoint::BooleanMesh, |a, b| {
        let (a, b) = (a as usize, b as usize);
        (rounded(a) || rounded(b))
            && improper_contact(faces[a].0, faces[a].1, faces[b].0, faces[b].1)
    })?;
    Ok(crossing
        .into_iter()
        .map(|(a, b)| (index[a as usize], index[b as usize]))
        .collect())
}

/// `out`'s grid points in millimetres (exact: whole steps below 2²⁶ over a power of two), each
/// vertex rounding moved marked.
pub(crate) fn grid_doubles(out: &Output) -> Doubles {
    #[allow(clippy::cast_precision_loss)] // Whole steps below 2^26: exact.
    let mm = |g: &GridPoint| {
        g.coordinates()
            .map(|c| c as f64 / super::grid::STEPS_PER_MM)
    };
    Doubles {
        vertices: out.vertices.iter().map(mm).collect(),
        triangles: out.triangles.clone(),
        rounded: out.moved,
        rounded_at: out.moved_at.clone(),
    }
}

/// The directed edges not matched by their reverse: zero for a closed mesh.
pub(crate) fn unbalanced(triangles: &[[u32; 3]]) -> usize {
    let mut balance: HashMap<(u32, u32), i64> = HashMap::new();
    for t in triangles {
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            if a == b {
                continue;
            }
            let (key, sign) = if a < b { ((a, b), 1) } else { ((b, a), -1) };
            *balance.entry(key).or_default() += sign;
        }
    }
    balance.values().filter(|&&v| v != 0).count()
}

#[cfg(test)]
mod tests {
    use super::{exact, mesh, unbalanced};
    use crate::{
        boolean::{
            arrange::{
                arrange,
                tests::{
                    box_triangles, difference, fine_box, intersection, mesh as input, oracle,
                    tetrahedron, union, Triangle,
                },
            },
            grid::GridPoint,
            int::tests::Rng,
        },
        cancellation::Cancellation,
    };

    /// The result's triangles as grid triangles, for the oracle.
    fn triangles(out: &super::Output) -> Vec<Triangle> {
        out.triangles
            .iter()
            .map(|t| t.map(|v| out.vertices[v as usize]))
            .collect()
    }

    /// Arrange, triangulate and round; check the exact triangulation and the rounded mesh are
    /// closed, and return the mesh with how many of `samples` random points (eighths of a step in
    /// `[-span, span]`) its membership differs from the inputs' result.
    fn run(
        inputs: &[Vec<Triangle>],
        keep: &(dyn Fn(&[bool]) -> bool + Sync),
        span: i64,
        samples: usize,
        seed: u64,
    ) -> (super::Output, usize) {
        let meshes: Vec<_> = inputs.iter().map(|t| input(t)).collect();
        let faces = arrange(&meshes, keep, &mut Cancellation::never()).unwrap();
        let exact = exact(faces, &mut Cancellation::never()).unwrap();
        assert_eq!(
            unbalanced(&exact.triangles),
            0,
            "the exact triangulation is closed"
        );
        let out = super::round(&exact);
        assert_eq!(unbalanced(&out.triangles), 0, "the rounded mesh is closed");
        let result = triangles(&out);
        let mut rng = Rng::new(seed);
        let (mut checked, mut wrong) = (0, 0);
        while checked < samples {
            let mut c = || {
                i64::try_from(rng.next() % u64::try_from(16 * span).unwrap()).unwrap() - 8 * span
            };
            let q = [c(), c(), c()];
            let Some(memberships) = inputs
                .iter()
                .map(|t| oracle(t, q).map(|w| w > 0))
                .collect::<Option<Vec<bool>>>()
            else {
                continue;
            };
            let Some(winding) = oracle(&result, q) else {
                continue;
            };
            assert!((0..=1).contains(&winding), "winding {winding} at {q:?}");
            wrong += usize::from((winding == 1) != keep(&memberships));
            checked += 1;
        }
        (out, wrong)
    }

    #[test]
    fn boxes_combine_into_closed_meshes_exactly_on_the_grid() {
        let inputs = [
            box_triangles([0, 0, 0], [10, 10, 10]),
            box_triangles([4, 3, 5], [17, 12, 14]),
        ];
        for keep in [union as fn(&[bool]) -> bool, intersection, difference] {
            let (out, wrong) = run(&inputs, &keep, 20, 400, 1);
            // Vertices where cell walls cross cut edges round to the grid along their straight,
            // axis-parallel lines: nothing bends, nothing goes flat.
            eprintln!(
                "moved {} dropped {} flat {}",
                out.moved, out.dropped, out.flat
            );
            assert_eq!((out.dropped, out.flat, wrong), (0, 0, 0));
        }
        // Coplanar faces partly overlapping: T-junctions where one face is cut and the other not.
        let stepped = [
            box_triangles([0, 0, 0], [10, 10, 10]),
            box_triangles([5, 5, 0], [15, 15, 6]),
        ];
        for keep in [union as fn(&[bool]) -> bool, intersection, difference] {
            let (out, wrong) = run(&stepped, &keep, 18, 300, 2);
            eprintln!(
                "moved {} dropped {} flat {}",
                out.moved, out.dropped, out.flat
            );
            assert_eq!((out.dropped, out.flat, wrong), (0, 0, 0));
        }
    }

    #[test]
    fn a_hollow_box_drilled_by_square_and_slanted_holes() {
        let mut shell = fine_box([0, 0, 0], [60, 60, 60], 6);
        shell.extend(
            fine_box([6, 6, 6], [54, 54, 54], 4)
                .into_iter()
                .map(|[a, b, c]| [a, c, b]),
        );
        // A square hole through the floor, and a tetrahedral spike through a wall at a slant.
        let mut holes = box_triangles([20, 21, -5], [29, 30, 12]);
        holes.extend(tetrahedron([
            GridPoint {
                x: -7,
                y: 25,
                z: 30,
            },
            GridPoint {
                x: 13,
                y: 35,
                z: 22,
            },
            GridPoint {
                x: 11,
                y: 18,
                z: 41,
            },
            GridPoint {
                x: 15,
                y: 31,
                z: 37,
            },
        ]));
        let (out, wrong) = run(&[shell, holes], &difference, 70, 600, 3);
        assert!(out.triangles.len() > 300, "{}", out.triangles.len());
        assert!(
            out.moved > 0,
            "the slanted hole makes vertices off the grid"
        );
        // Rounding moves a vertex by half a step at most: only points that close to the
        // surface can change sides.
        assert!(wrong <= 3, "{wrong} of 600");
    }

    #[test]
    fn random_tetrahedra_give_closed_meshes() {
        let mut rng = Rng::new(8);
        let mut g = || {
            let mut c = || i64::try_from(rng.next() % 41).unwrap() - 20;
            GridPoint {
                x: c(),
                y: c(),
                z: c(),
            }
        };
        let mut wrong = 0;
        for round in 0..40 {
            let inputs: Vec<Vec<Triangle>> =
                (0..3).map(|_| tetrahedron([g(), g(), g(), g()])).collect();
            let keep: fn(&[bool]) -> bool = [union, intersection, difference][round % 3];
            wrong += run(&inputs, &keep, 22, 60, 100 + round as u64).1;
        }
        assert!(wrong <= 24, "{wrong} of 2,400");
    }

    #[test]
    #[ignore = "a measurement: cargo test --release -p karasu-core boolean::output -- --ignored --nocapture"]
    fn a_fine_shell_drilled_by_four_holes_as_a_mesh() {
        let n = 150;
        let mut shell = fine_box([0, 0, 0], [300_000, 300_000, 300_000], n);
        shell.extend(
            fine_box([12_000, 12_000, 12_000], [288_000, 288_000, 288_000], n)
                .into_iter()
                .map(|[a, b, c]| [a, c, b]),
        );
        let mut holes = Vec::new();
        for k in 0..4 {
            let x = 40_000 + 60_000 * k;
            holes.extend(box_triangles(
                [x, 140_000, -5_000],
                [x + 8_192, 148_192, 20_000],
            ));
        }
        let inputs = [input(&shell), input(&holes)];
        let started = std::time::Instant::now();
        let faces = arrange(&inputs, &difference, &mut Cancellation::never()).unwrap();
        let arranged = started.elapsed();
        let out = mesh(faces);
        println!(
            "arranged in {:.2} s, meshed in {:.2} s: {} triangles, {} moved, {} unbalanced",
            arranged.as_secs_f64(),
            started.elapsed().saturating_sub(arranged).as_secs_f64(),
            out.triangles.len(),
            out.moved,
            unbalanced(&out.triangles)
        );
    }
}
