//! Exact mesh Booleans (TD-140): difference, union and intersection of closed meshes, and any
//! combination of several, decided by the vector of their winding numbers.
//!
//! **The design** (TD-140): every input vertex
//! is snapped once to a fixed grid of 2⁻¹⁵ mm ([`grid`]); polygons are planes with integer
//! coefficients and vertices the exact meeting points of three planes ([`plane`]), in fixed-width
//! integers ([`int`]); no decision uses floating point. Fragments are classified by exact tracing
//! from reference points of known winding numbers, never by propagating winding numbers radially
//! around intersection curves (TD-140).
//!
//! **Parts:** the grid, the integers, the planes and their meeting points ([`plane`]); convex
//! polygons ([`polygon`]); exact walks of winding numbers ([`trace`]); the localized arrangement
//! ([`arrange`]); the result as a closed mesh ([`output`]); and [`solve`], from meshes in
//! millimetres to the result on the grid, the crossings rounding makes resolved (phase 5). Its
//! first use is cutting drain holes into a model's mesh for export
//! ([`crate::preparation::sampled::HoleCuts::drill`], TD-139, phase 4).
#![allow(clippy::many_single_char_names)] // Geometric notation: points, planes and vectors.

pub(crate) mod arrange;
pub(crate) mod grid;
pub(crate) mod int;
pub(crate) mod output;
pub(crate) mod plane;
pub(crate) mod polygon;
pub(crate) mod trace;

use crate::{
    cancellation::Cancellation,
    geometry::Point3,
    mesh::{MeshError, TriangleMesh},
};
use std::fmt;

/// The cells of the snap grid that resolves rounding's crossings, in grid steps: 2⁹ steps of
/// 2⁻¹⁵ mm, 15.6 µm (TD-140 phase 5). A snapped vertex moves at most half a cell on each axis,
/// 7.8 µm, below any resin pixel; the cell is 2⁹ times the grid the made points are rounded to,
/// the gap that keeps rounding from folding what the snap made (Valque and Lazard).
pub(crate) const SNAP_CELL: i64 = 512;

/// How far [`separate`] moves a touching vertex's copies into their own bodies, in grid steps:
/// two steps of 2⁻¹⁵ mm, about 0.06 µm: far below any pixel, and at least two units in the last
/// place of `f32` within ±512 mm, so the copies stay apart in a file. Farther does not help:
/// on the owner's torso 1 µm left as many crossings, which come from the slivers along the
/// seams turning, not from the distance.
pub(crate) const APART: f64 = 2.0;

/// Rounds of resolving rounding's crossings before what remains is reported.
pub(crate) const SNAP_ROUNDS: u32 = 12;

/// What a Boolean made: the result on the grid, and what snapping, rounding and resolving did.
#[derive(Debug)]
pub(crate) struct Solved {
    /// The result, closed and facing out, every vertex a grid point: exact in `f64`, and in
    /// `f32` within ±512 mm.
    pub(crate) mesh: TriangleMesh,
    /// Input vertices moved by snapping to the grid (at most 2⁻¹⁶ mm on each axis).
    pub(crate) snapped: usize,
    /// Points the Boolean made that are not grid points, rounded to the nearest (at most
    /// 2⁻¹⁶ mm on each axis), counted in the last round.
    pub(crate) rounded: usize,
    /// Rounds of resolving what rounding made cross or flat (0: rounding made nothing so).
    pub(crate) rounds: u32,
    /// Vertices moved to the centre of their snap cell, over every round.
    pub(crate) moved: usize,
    /// The farthest a vertex was moved to a snap cell's centre on one axis, in millimetres.
    pub(crate) farthest_mm: f64,
    /// Separate bodies no larger than a snap cell on any axis, left out ([`specks`]).
    pub(crate) specks: usize,
    /// Vertex copies moved [`APART`] into their own body where bodies touched ([`separate`]).
    pub(crate) separated: usize,
    /// Triangles without area that remain after the last round.
    pub(crate) flat: usize,
    /// Pairs of triangles that meet other than along shared vertices and edges, remaining
    /// after the last round.
    pub(crate) crossings: usize,
}

/// Why a Boolean was refused or stopped.
#[derive(Debug)]
pub(crate) enum BooleanError {
    /// An input vertex cannot be snapped to the grid.
    Coordinate {
        input: usize,
        vertex: usize,
        error: grid::GridError,
    },
    /// An input is not closed: this many edges are not used as often in each direction.
    Open { input: usize, edges: usize },
    /// The arrangement stopped.
    Arrange(arrange::ArrangeError),
    /// The result could not be made a mesh.
    Mesh(MeshError),
    /// The result came out not closed: this many edges, by position, are not used as often in
    /// each direction. A defect of the kernel, never written as a model.
    NotClosed { edges: usize },
}

impl fmt::Display for BooleanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Coordinate {
                input,
                vertex,
                error,
            } => write!(f, "input {input}, vertex {vertex}: {error}"),
            Self::Open { input, edges } => write!(
                f,
                "input {input} is not closed: {edges} edges are not used as often in each direction"
            ),
            Self::Arrange(error) => error.fmt(f),
            Self::Mesh(error) => write!(f, "the result is not a valid mesh: {error}"),
            Self::NotClosed { edges } => write!(
                f,
                "the result is not closed ({edges} edges are not used as often in each \
                 direction); this is a defect of the Boolean kernel"
            ),
        }
    }
}

impl std::error::Error for BooleanError {}

/// The Boolean `keep` of `inputs` (meshes in millimetres) on the grid, the crossings rounding
/// makes resolved (TD-140 phase 5).
///
/// The inputs are snapped to the grid, checked closed and arranged; the exact result is
/// rounded to the grid. Rounding can fold a sliver: the pairs of triangles that then cross, and
/// the triangles left without area, are found exactly. Every vertex lying in a cell of the
/// fixed snap grid ([`SNAP_CELL`]) that holds a vertex of such a triangle is moved to the
/// cell's centre, which makes the slivers collapse, and the mesh is united with itself by the
/// exact kernel, which resolves what still crosses; the new result is rounded again. After
/// [`SNAP_ROUNDS`] rounds what remains is counted, never hidden. This is Valque and Lazard's
/// method (SGP 2025), with the kernel's grid as the target precision: it is not proved to end,
/// and on the owner's torso it ended in 8 rounds.
///
/// Deterministic: the same result for any number of threads.
///
/// # Errors
/// [`BooleanError`]: a coordinate beyond the grid's range, an input not closed, cancellation or
/// exhausted memory, a result that cannot be a mesh, or one that comes out not closed.
pub(crate) fn solve(
    inputs: &[&TriangleMesh],
    keep: &(dyn Fn(&[bool]) -> bool + Sync),
    cancel: &mut Cancellation<'_>,
) -> Result<Solved, BooleanError> {
    let (meshes, snapped) = snap(inputs)?;
    let faces = arrange::arrange(&meshes, keep, cancel).map_err(BooleanError::Arrange)?;
    // Each stage's input is let go once the next holds what it needs: the peak is one stage's.
    drop(meshes);
    let mut exact = output::exact(faces, cancel).map_err(BooleanError::Arrange)?;
    let (mut moved, mut farthest) = (0, 0_i64);
    let mut round = 0;
    loop {
        let out = output::round(&exact);
        drop(exact);
        let doubles = output::grid_doubles(&out);
        let pairs = output::crossing_pairs(&doubles, cancel).map_err(BooleanError::Arrange)?;
        let flat: Vec<usize> = (0..out.triangles.len())
            .filter(|&t| flat_on_grid(&out, t))
            .collect();
        if (pairs.is_empty() && flat.is_empty()) || round == SNAP_ROUNDS {
            let edges = output::unbalanced(&out.triangles);
            if edges > 0 {
                return Err(BooleanError::NotClosed { edges });
            }
            let (kept, specks) = specks(&out);
            let (apart, separated) = separate(&out, &kept);
            let doubles = output::grid_doubles(&apart);
            let crossings = output::crossing_pairs(&doubles, cancel)
                .map_err(BooleanError::Arrange)?
                .len();
            let flat = (0..apart.triangles.len())
                .filter(|&t| flat_on_grid(&apart, t))
                .count();
            let mesh = kept_mesh(&doubles, &vec![true; apart.triangles.len()])?;
            #[allow(clippy::cast_precision_loss)] // Half a snap cell, in steps.
            let farthest_mm = farthest as f64 / grid::STEPS_PER_MM;
            return Ok(Solved {
                mesh,
                snapped,
                rounded: out.moved,
                rounds: round,
                moved,
                farthest_mm,
                specks,
                separated,
                flat,
                crossings,
            });
        }
        round += 1;
        // Every vertex in a cell that holds a vertex of an offending triangle, to that cell's
        // centre (Valque and Lazard snap whole cells, not only the offending vertices).
        let cell = |g: grid::GridPoint| g.coordinates().map(|x| x.div_euclid(SNAP_CELL));
        let mut cells = std::collections::HashSet::new();
        for t in pairs
            .iter()
            .flat_map(|&(a, b)| [a as usize, b as usize])
            .chain(flat)
        {
            for v in out.triangles[t] {
                cells.insert(cell(out.vertices[v as usize]));
            }
        }
        let mut vertices = out.vertices;
        for v in &mut vertices {
            let c = cell(*v);
            if !cells.contains(&c) {
                continue;
            }
            let centre = c.map(|i| i * SNAP_CELL + SNAP_CELL / 2);
            let old = v.coordinates();
            if centre != old {
                moved += 1;
                farthest = (0..3)
                    .map(|k| (centre[k] - old[k]).abs())
                    .fold(farthest, i64::max);
                *v = grid::GridPoint {
                    x: centre[0],
                    y: centre[1],
                    z: centre[2],
                };
            }
        }
        // Triangles the snap collapsed add nothing: their edges cancel.
        let triangles: Vec<[u32; 3]> = out
            .triangles
            .into_iter()
            .filter(|t| {
                let p = t.map(|v| vertices[v as usize]);
                p[0] != p[1] && p[1] != p[2] && p[2] != p[0]
            })
            .collect();
        let snapped_mesh = arrange::Mesh {
            vertices,
            triangles,
        };
        let faces = arrange::arrange(&[snapped_mesh], &|m: &[bool]| m[0], cancel)
            .map_err(BooleanError::Arrange)?;
        exact = output::exact(faces, cancel).map_err(BooleanError::Arrange)?;
    }
}

/// Which triangles of `out` to keep, and how many bodies are left out: a separate closed body
/// (its triangles connected through shared vertices) whose extent on every axis is at most a
/// snap cell ([`SNAP_CELL`], 15.6 µm). Rounding and its rounds can leave such a speck where a
/// sliver was resolved; it lies within the move the rounds already allow, below any pixel, and
/// removing a whole closed body keeps the rest closed. Each one left out is counted.
fn specks(out: &output::Output) -> (Vec<bool>, usize) {
    let mut parent: Vec<u32> = (0..u32::try_from(out.vertices.len()).unwrap_or(u32::MAX)).collect();
    for t in &out.triangles {
        let a = root(&mut parent, t[0]);
        for &v in &t[1..] {
            let b = root(&mut parent, v);
            if a != b {
                parent[b as usize] = a;
            }
        }
    }
    // Each body's box, by its root.
    let mut boxes: std::collections::HashMap<u32, ([i64; 3], [i64; 3])> =
        std::collections::HashMap::new();
    for v in 0..out.vertices.len() {
        let r = root(&mut parent, u32::try_from(v).unwrap_or(u32::MAX));
        let c = out.vertices[v].coordinates();
        let (lo, hi) = boxes.entry(r).or_insert((c, c));
        for ((lo, hi), &value) in lo.iter_mut().zip(hi.iter_mut()).zip(&c) {
            *lo = (*lo).min(value);
            *hi = (*hi).max(value);
        }
    }
    let speck: std::collections::HashSet<u32> = boxes
        .iter()
        .filter(|(_, (lo, hi))| (0..3).all(|k| hi[k] - lo[k] <= SNAP_CELL))
        .map(|(&r, _)| r)
        .collect();
    // Never the whole result: a body that small alone is the result.
    if speck.len() == boxes.len() {
        return (vec![true; out.triangles.len()], 0);
    }
    let kept = out
        .triangles
        .iter()
        .map(|t| !speck.contains(&root(&mut parent, t[0])))
        .collect();
    (kept, speck.len())
}

/// The triangles of `out` that `kept` marks, where bodies touch moved apart: the result's sheets
/// that meet along an edge of four faces, or at a vertex alone (bodies of a sculpt touching after
/// the union), each get their own copies of the vertices there ([`manifold_edges`]'s pairing,
/// each pair of faces bounding one wedge of solid), and each copy moves [`APART`] into its own
/// body, along the mean inward normal of its faces, rounded to the grid. A program welding a
/// file's equal points then finds two separate sheets rather than an edge of four faces. Returns
/// the result and the copies moved; the crossings this could make are counted by the caller.
///
/// [`manifold_edges`]: crate::export::models::manifold_edges
fn separate(out: &output::Output, kept: &[bool]) -> (output::Output, usize) {
    // The kept triangles on their own vertices, as `f32` (exact for grid points within ±512 mm).
    let mut index = vec![u32::MAX; out.vertices.len()];
    let mut grid_points: Vec<grid::GridPoint> = Vec::new();
    // Whether rounding moved each: its crossings are counted with the separation's.
    let mut rounded: Vec<bool> = Vec::new();
    let mut triangles: Vec<[u32; 3]> = Vec::new();
    for (&t, _) in out.triangles.iter().zip(kept).filter(|(_, &k)| k) {
        triangles.push(t.map(|v| {
            if index[v as usize] == u32::MAX {
                index[v as usize] = u32::try_from(grid_points.len()).unwrap_or(u32::MAX);
                grid_points.push(out.vertices[v as usize]);
                rounded.push(out.moved_at[v as usize]);
            }
            index[v as usize]
        }));
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)] // Grid points: exact.
    let points: Vec<[f32; 3]> = grid_points
        .iter()
        .map(|g| {
            g.coordinates()
                .map(|c| (c as f64 / grid::STEPS_PER_MM) as f32)
        })
        .collect();
    let (copies, faces, _) = crate::export::models::manifold_edges(&points, &triangles);
    // Each copy's grid point: copies of one vertex share its point.
    let by_point: std::collections::HashMap<[u32; 3], usize> = points
        .iter()
        .enumerate()
        .map(|(k, p)| (p.map(f32::to_bits), k))
        .collect();
    let original: Vec<usize> = copies
        .iter()
        .map(|p| by_point[&p.map(f32::to_bits)])
        .collect();
    let mut vertices: Vec<grid::GridPoint> = original.iter().map(|&k| grid_points[k]).collect();
    let mut copies_of: std::collections::HashMap<grid::GridPoint, Vec<usize>> =
        std::collections::HashMap::new();
    for (k, &g) in vertices.iter().enumerate() {
        copies_of.entry(g).or_default().push(k);
    }
    // Each touching copy's outward normal: the sum of its faces' (twice their area).
    let mut normal = vec![[0_i128; 3]; vertices.len()];
    let touching: Vec<bool> = vertices.iter().map(|g| copies_of[g].len() > 1).collect();
    for t in &faces {
        if !t.iter().any(|&v| touching[v as usize]) {
            continue;
        }
        let [a, b, c] = t.map(|v| vertices[v as usize].coordinates().map(i128::from));
        let (u, w) = (
            [0, 1, 2].map(|k| b[k] - a[k]),
            [0, 1, 2].map(|k| c[k] - a[k]),
        );
        let n = [
            u[1] * w[2] - u[2] * w[1],
            u[2] * w[0] - u[0] * w[2],
            u[0] * w[1] - u[1] * w[0],
        ];
        for &v in t {
            for k in 0..3 {
                normal[v as usize][k] += n[k];
            }
        }
    }
    let mut moved_at: Vec<bool> = original.iter().map(|&k| rounded[k]).collect();
    let mut separated = 0;
    for (v, g) in vertices.iter_mut().enumerate() {
        if !touching[v] {
            continue;
        }
        #[allow(clippy::cast_precision_loss)] // A direction.
        let n = normal[v].map(|c| c as f64);
        let length = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        if length == 0.0 || !length.is_finite() {
            continue;
        }
        // Inward: against the outward normal.
        #[allow(clippy::cast_possible_truncation)] // A few grid steps.
        let step = n.map(|c| (-c / length * APART).round() as i64);
        let c = g.coordinates();
        *g = grid::GridPoint {
            x: c[0] + step[0],
            y: c[1] + step[1],
            z: c[2] + step[2],
        };
        moved_at[v] = true;
        separated += 1;
    }
    let mut faces = faces;
    separated += split_shared_edges(&mut vertices, &mut faces, &mut moved_at);
    (
        output::Output {
            moved: moved_at.iter().filter(|&&m| m).count(),
            vertices,
            triangles: faces,
            dropped: 0,
            flat: 0,
            moved_at,
        },
        separated,
    )
}

/// An edge, by its ends in increasing order, and its faces: (triangle, corner its use starts at).
type EdgeUses = ((u32, u32), Vec<(u32, u8)>);

/// Edges [`separate`] left with four faces or more where a reader welds equal points (sheets
/// meeting around both ends, or copies that could not move): each pair of faces bounding one
/// wedge of solid gets its own point in the middle of the edge, moved [`APART`] into that wedge,
/// and both faces are split there. The edge is gone; the sheets share at most its ends. Returns
/// the points added.
fn split_shared_edges(
    vertices: &mut Vec<grid::GridPoint>,
    faces: &mut Vec<[u32; 3]>,
    moved_at: &mut Vec<bool>,
) -> usize {
    use crate::export::models::{around_edge, pair_faces};
    // Each vertex as the first one at its point.
    let mut first: std::collections::HashMap<grid::GridPoint, u32> =
        std::collections::HashMap::new();
    let welded: Vec<u32> = (0_u32..)
        .zip(vertices.iter())
        .map(|(i, g)| *first.entry(*g).or_insert(i))
        .collect();
    let weld = |t: [u32; 3]| t.map(|v| welded[v as usize]);
    let welded_faces: Vec<[u32; 3]> = faces.iter().map(|&t| weld(t)).collect();
    let mut uses: std::collections::HashMap<(u32, u32), Vec<(u32, u8)>> =
        std::collections::HashMap::new();
    for (t, triangle) in (0_u32..).zip(&welded_faces) {
        for k in 0..3_u8 {
            let (a, b) = (triangle[k as usize], triangle[(k as usize + 1) % 3]);
            uses.entry((a.min(b), a.max(b))).or_default().push((t, k));
        }
    }
    let mut shared: Vec<EdgeUses> = uses.into_iter().filter(|(_, u)| u.len() > 2).collect();
    if shared.is_empty() {
        return 0;
    }
    shared.sort_unstable_by_key(|(edge, _)| *edge);
    #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)] // Grid points.
    let points: Vec<[f32; 3]> = vertices
        .iter()
        .map(|g| {
            g.coordinates()
                .map(|c| (c as f64 / grid::STEPS_PER_MM) as f32)
        })
        .collect();
    let mut added = 0;
    let mut split: Vec<(u32, u32, u32, (u32, u32))> = Vec::new();
    for ((u, v), list) in &shared {
        let around = around_edge(&points, &welded_faces, (*u, *v), list);
        for (a, b) in pair_faces(&around, false) {
            // The pair's mean outward normal, twice its area.
            let mut normal = [0_i128; 3];
            for t in [a, b] {
                let [p, q, r] =
                    faces[t as usize].map(|w| vertices[w as usize].coordinates().map(i128::from));
                let (e, f) = (
                    [0, 1, 2].map(|k| q[k] - p[k]),
                    [0, 1, 2].map(|k| r[k] - p[k]),
                );
                normal[0] += e[1] * f[2] - e[2] * f[1];
                normal[1] += e[2] * f[0] - e[0] * f[2];
                normal[2] += e[0] * f[1] - e[1] * f[0];
            }
            #[allow(clippy::cast_precision_loss)] // A direction.
            let n = normal.map(|c| c as f64);
            let length = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            if length == 0.0 || !length.is_finite() {
                continue;
            }
            let (cu, cv) = (
                vertices[*u as usize].coordinates(),
                vertices[*v as usize].coordinates(),
            );
            #[allow(clippy::cast_possible_truncation)] // A few grid steps.
            let middle: [i64; 3] = std::array::from_fn(|k| {
                (cu[k] + cv[k]).div_euclid(2) + (-n[k] / length * APART).round() as i64
            });
            let m = u32::try_from(vertices.len()).unwrap_or(u32::MAX);
            vertices.push(grid::GridPoint {
                x: middle[0],
                y: middle[1],
                z: middle[2],
            });
            moved_at.push(true);
            added += 1;
            split.push((a, b, m, (*u, *v)));
        }
    }
    // Each face of a pair, split at its pair's point on the shared edge, its turn kept.
    for (a, b, m, (u, v)) in split {
        for t in [a, b] {
            let face = faces[t as usize];
            let w = weld(face);
            let Some(k) = (0..3).find(|&k| {
                let (p, q) = (w[k], w[(k + 1) % 3]);
                (p.min(q), p.max(q)) == (u, v)
            }) else {
                continue;
            };
            let (p, q, r) = (face[k], face[(k + 1) % 3], face[(k + 2) % 3]);
            faces[t as usize] = [p, m, r];
            faces.push([m, q, r]);
        }
    }
    added
}

/// The triangles of `doubles` that `kept` marks, as a mesh of the vertices they use.
fn kept_mesh(doubles: &output::Doubles, kept: &[bool]) -> Result<TriangleMesh, BooleanError> {
    let mut index = vec![u32::MAX; doubles.vertices.len()];
    let mut vertices = Vec::new();
    let mut triangles = Vec::with_capacity(doubles.triangles.len());
    for (&triangle, _) in doubles.triangles.iter().zip(kept).filter(|(_, &k)| k) {
        triangles.push(triangle.map(|v| {
            if index[v as usize] == u32::MAX {
                index[v as usize] = u32::try_from(vertices.len()).unwrap_or(u32::MAX);
                let [x, y, z] = doubles.vertices[v as usize];
                vertices.push(Point3::new(x, y, z));
            }
            index[v as usize]
        }));
    }
    TriangleMesh::try_new(vertices, triangles).map_err(BooleanError::Mesh)
}

/// The union-find root of `x`, halving the path.
fn root(parent: &mut [u32], mut x: u32) -> u32 {
    while parent[x as usize] != x {
        parent[x as usize] = parent[parent[x as usize] as usize];
        x = parent[x as usize];
    }
    x
}

/// Whether triangle `t` of `out` has its three grid corners on one line.
fn flat_on_grid(out: &output::Output, t: usize) -> bool {
    let [a, b, c] =
        out.triangles[t].map(|v| out.vertices[v as usize].coordinates().map(i128::from));
    let (u, v) = (
        [0, 1, 2].map(|k| b[k] - a[k]),
        [0, 1, 2].map(|k| c[k] - a[k]),
    );
    u[1] * v[2] == u[2] * v[1] && u[2] * v[0] == u[0] * v[2] && u[0] * v[1] == u[1] * v[0]
}

/// Each input snapped to the grid and checked closed, with the number of vertices snapping moved.
fn snap(inputs: &[&TriangleMesh]) -> Result<(Vec<arrange::Mesh>, usize), BooleanError> {
    let mut meshes = Vec::with_capacity(inputs.len());
    let mut snapped = 0;
    for (input, mesh) in inputs.iter().enumerate() {
        let mut vertices = Vec::with_capacity(mesh.vertices().len());
        for (vertex, &p) in mesh.vertices().iter().enumerate() {
            let g = grid::GridPoint::snap(p).map_err(|error| BooleanError::Coordinate {
                input,
                vertex,
                error,
            })?;
            snapped += usize::from(g.point() != p);
            vertices.push(g);
        }
        // Closed on the snapped vertices, welded by position.
        let mut index = std::collections::HashMap::new();
        let welded: Vec<[u32; 3]> = mesh
            .triangles()
            .iter()
            .map(|t| {
                t.map(|v| {
                    let next = u32::try_from(index.len()).unwrap_or(u32::MAX);
                    *index.entry(vertices[v as usize]).or_insert(next)
                })
            })
            .collect();
        let edges = output::unbalanced(&welded);
        if edges > 0 {
            return Err(BooleanError::Open { input, edges });
        }
        meshes.push(arrange::Mesh {
            vertices,
            triangles: mesh.triangles().to_vec(),
        });
    }
    Ok((meshes, snapped))
}

#[cfg(test)]
mod tests {
    use super::{solve, BooleanError};
    use crate::{
        boolean::arrange::tests::{box_triangles, difference, union, Triangle},
        cancellation::Cancellation,
        geometry::Point3,
        mesh::TriangleMesh,
    };

    /// Grid triangles as a mesh in millimetres.
    fn mm(triangles: &[Triangle]) -> TriangleMesh {
        let vertices: Vec<Point3> = triangles.iter().flatten().map(|g| g.point()).collect();
        let faces = (0..triangles.len())
            .map(|i| {
                let i = u32::try_from(3 * i).unwrap();
                [i, i + 1, i + 2]
            })
            .collect();
        TriangleMesh::try_new(vertices, faces).unwrap()
    }

    #[test]
    fn a_box_drilled_through_is_a_closed_mesh_on_the_grid() {
        let block = mm(&box_triangles([0, 0, 0], [4_096, 4_096, 4_096]));
        let drill = mm(&box_triangles([1_000, 1_000, -100], [2_000, 2_000, 5_000]));
        let solved = solve(&[&block, &drill], &difference, &mut Cancellation::never()).unwrap();
        // 8 outside, 8 in the hole and 8 on each pierced face: the pieces cell walls and the
        // drill's planes cut stay where the region around the hole is not convex, but every
        // vertex they made lies in one plane or on a straight edge and is taken out again, and so
        // are the points where the drill's diagonals cross the block's faces. Every vertex is a
        // corner of an input: nothing is rounded.
        let count = solved.mesh.triangles().len();
        assert_eq!((count, solved.rounded), (32, 0));
        assert_eq!(
            (
                solved.snapped,
                solved.rounds,
                solved.moved,
                solved.flat,
                solved.crossings
            ),
            (0, 0, 0, 0, 0)
        );
        assert_eq!(super::output::unbalanced(solved.mesh.triangles()), 0);
    }

    #[test]
    #[ignore = "a measurement: cargo test --release -p karasu-core boolean::tests -- --ignored --nocapture"]
    fn a_fine_shell_drilled_by_four_holes_from_start_to_end() {
        use crate::boolean::arrange::tests::fine_box;
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
        let (shell, holes) = (mm(&shell), mm(&holes));
        let started = std::time::Instant::now();
        let solved = solve(&[&shell, &holes], &difference, &mut Cancellation::never()).unwrap();
        println!(
            "{} triangles in, {} out in {:.2} s: {} rounded, {} rounds, {} moved, {} flat, {} \
             crossings",
            shell.triangles().len() + holes.triangles().len(),
            solved.mesh.triangles().len(),
            started.elapsed().as_secs_f64(),
            solved.rounded,
            solved.rounds,
            solved.moved,
            solved.flat,
            solved.crossings
        );
    }

    #[test]
    fn a_body_no_larger_than_a_snap_cell_is_left_out_and_counted() {
        // A block and, apart from it, a cube of 300 steps (9 µm): the cube is a speck.
        let block = mm(&box_triangles([0, 0, 0], [40_000, 40_000, 40_000]));
        let speck = mm(&box_triangles([50_000, 0, 0], [50_300, 300, 300]));
        let solved = solve(&[&block, &speck], &union, &mut Cancellation::never()).unwrap();
        assert_eq!((solved.mesh.triangles().len(), solved.specks), (12, 1));
        // One cell wide, it stays: the rule is a cell on every axis, no more.
        let bar = mm(&box_triangles([50_000, 0, 0], [50_300, 300, 2_000]));
        let solved = solve(&[&block, &bar], &union, &mut Cancellation::never()).unwrap();
        assert_eq!((solved.mesh.triangles().len(), solved.specks), (24, 0));
        // A speck alone is the whole result, and kept.
        let solved = solve(&[&speck], &union, &mut Cancellation::never()).unwrap();
        assert_eq!((solved.mesh.triangles().len(), solved.specks), (12, 0));
    }

    #[test]
    fn bodies_touching_along_an_edge_are_moved_apart() {
        // Two blocks meeting along the edge x = y = 4096: united, the edge has four faces. Each
        // block's copies of its two ends move into it, and no point is shared any more.
        let a = mm(&box_triangles([0, 0, 0], [4_096, 4_096, 4_096]));
        let b = mm(&box_triangles([4_096, 4_096, 0], [8_192, 8_192, 4_096]));
        let solved = solve(&[&a, &b], &union, &mut Cancellation::never()).unwrap();
        assert_eq!(solved.separated, 4);
        assert_eq!((solved.crossings, solved.flat), (0, 0));
        // Welded by position, as a file's reader does: every edge has two faces.
        let mut index = std::collections::HashMap::new();
        let mut edges = std::collections::HashMap::new();
        for t in solved.mesh.triangles() {
            let w = t.map(|v| {
                let p = solved.mesh.vertices()[v as usize];
                let next = index.len();
                *index
                    .entry([p.x, p.y, p.z].map(f64::to_bits))
                    .or_insert(next)
            });
            for k in 0..3 {
                let (p, q) = (w[k], w[(k + 1) % 3]);
                *edges.entry((p.min(q), p.max(q))).or_insert(0) += 1;
            }
        }
        assert!(edges.values().all(|&n| n == 2), "{edges:?}");
        assert_eq!(index.len(), 16);
    }

    #[test]
    fn every_stage_can_be_cancelled() {
        use crate::cancellation::Checkpoint;
        let block = mm(&box_triangles([0, 0, 0], [4_096, 4_096, 4_096]));
        let drill = mm(&box_triangles([1_000, 1_000, -100], [2_000, 2_000, 5_000]));
        for stage in [
            Checkpoint::BooleanContacts,
            Checkpoint::BooleanCells,
            Checkpoint::BooleanMesh,
        ] {
            let mut predicate = |c| c == stage;
            let result = solve(
                &[&block, &drill],
                &difference,
                &mut Cancellation::new(&mut predicate),
            );
            assert!(
                matches!(
                    &result,
                    Err(BooleanError::Arrange(super::arrange::ArrangeError::Cancelled(c)))
                        if c.checkpoint == stage
                ),
                "{stage:?}: {result:?}"
            );
        }
    }

    #[test]
    fn inputs_off_the_grid_are_snapped_and_said() {
        // A box a third of a step off the grid.
        let third = 1.0 / (3.0 * super::grid::STEPS_PER_MM);
        let triangles = box_triangles([0, 0, 0], [100, 100, 100]);
        let vertices: Vec<Point3> = triangles
            .iter()
            .flatten()
            .map(|g| {
                let p = g.point();
                Point3::new(p.x + third, p.y, p.z)
            })
            .collect();
        let faces = (0..triangles.len())
            .map(|i| {
                let i = u32::try_from(3 * i).unwrap();
                [i, i + 1, i + 2]
            })
            .collect();
        let shifted = TriangleMesh::try_new(vertices, faces).unwrap();
        let solved = solve(&[&shifted], &|m: &[bool]| m[0], &mut Cancellation::never()).unwrap();
        assert_eq!(solved.snapped, 36);
        assert_eq!(solved.mesh.triangles().len(), 12);
    }

    #[test]
    fn open_inputs_and_coordinates_beyond_the_range_are_refused() {
        let mut open = box_triangles([0, 0, 0], [10, 10, 10]);
        open.pop();
        let result = solve(&[&mm(&open)], &union, &mut Cancellation::never());
        assert!(
            matches!(result, Err(BooleanError::Open { input: 0, edges: 3 })),
            "{result:?}"
        );
        let far = TriangleMesh::try_new(
            vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(20_000.0, 0.0, 0.0),
                Point3::new(0.0, 1.0, 0.0),
            ],
            vec![[0, 1, 2], [0, 2, 1]],
        )
        .unwrap();
        let result = solve(&[&far], &union, &mut Cancellation::never());
        assert!(
            matches!(
                result,
                Err(BooleanError::Coordinate {
                    input: 0,
                    vertex: 1,
                    ..
                })
            ),
            "{result:?}"
        );
        let message = result.unwrap_err().to_string();
        assert!(message.contains("beyond the admitted"), "{message}");
    }
}
