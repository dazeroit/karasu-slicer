//! Tolerant vertex welding (M6.4d r2, TD-092, stage R1).
//!
//! **Rule:** vertices on open edges (edges with one face) that lie within the tolerance `t` of
//! each other (computed Euclidean distance) merge into clusters in which every two members are
//! within `t`, so no vertex moves farther than `t` and no two points farther apart than `t` ever
//! merge. Each cluster keeps the position of its smallest vertex index; the other members move
//! onto it. `t = 0` merges vertices at equal positions only.
//!
//! **Only open edges:** welding closes gaps, which only open edges have. A vertex inside a closed
//! region is never moved, so welding cannot pinch or fold a closed surface where two parts pass
//! close to each other (as welding every vertex does on the Yorick miniature at 1 µm: three
//! non-manifold edges and a pinch vertex). Short edges inside closed regions are the
//! degenerate-face stage's, which checks that collapsing them keeps the surface manifold.
//!
//! **Clusters** are grown from the unmerged vertices in increasing index: the seed's unmerged
//! neighbours within `t` join in increasing distance (then index) while they stay within `t` of
//! every member. A neighbour left out may start or join a later cluster. Unlike rounding to a
//! grid, as the reference slicers weld (repair comparison, "Welding everywhere"), points closer
//! than `t` across a cell border are compared, and no point moves more than `t`.
//!
//! **Faces** whose corners merge are removed and reported with their area; faces that become
//! duplicates are left to duplicate collapse. Merged vertices are dropped; every other vertex
//! keeps its order, unused ones included.
//!
//! **Method:** vertices sorted by their cell of side `t` (their exact coordinates when `t = 0`);
//! which vertices have a neighbour is decided on every core, then clusters are grown
//! sequentially, so results do not depend on the thread count.

use super::RepairError;
use crate::{
    cancellation::{Cancellation, Checkpoint},
    mesh::{
        repair::{record, RepairChange, RepairSample},
        MeshError, TriangleMesh,
    },
};

/// What welding did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WeldReport {
    /// Groups of vertices merged into one.
    pub clusters: usize,
    /// Vertices merged into another (moved and dropped).
    pub vertices_merged: usize,
    /// The largest distance a vertex moved, in mm (at most the tolerance).
    pub max_displacement_mm: f64,
    /// Faces removed because two or three of their corners merged.
    pub faces_collapsed: usize,
    /// Their area before welding (mm², source coordinates).
    pub collapsed_area_mm2: f64,
    /// Where vertices merged: the kept vertex of up to
    /// [`crate::mesh::repair::MAX_REPAIR_SAMPLES`] clusters.
    pub samples: Vec<RepairSample>,
}

/// No vertex assigned yet.
const NONE: u32 = u32::MAX;

/// Cell coordinates beyond this magnitude would lose integer precision.
const MAX_CELL: f64 = 4_503_599_627_370_496.0; // 2^52

pub(super) fn allocation(context: &'static str) -> RepairError {
    RepairError::Topology(crate::mesh::topology::TopologyError::Mesh(
        MeshError::Allocation { context },
    ))
}

fn filled<T: Clone>(count: usize, value: T, context: &'static str) -> Result<Vec<T>, RepairError> {
    let mut items = Vec::new();
    items
        .try_reserve_exact(count)
        .map_err(|_| allocation(context))?;
    items.resize(count, value);
    Ok(items)
}

fn squared_distance(p: [f64; 3], q: [f64; 3]) -> f64 {
    (0..3).map(|k| (p[k] - q[k]) * (p[k] - q[k])).sum()
}

fn area([origin, first, second]: [[f64; 3]; 3]) -> f64 {
    let (along, across): ([f64; 3], [f64; 3]) = (
        std::array::from_fn(|k| first[k] - origin[k]),
        std::array::from_fn(|k| second[k] - origin[k]),
    );
    let normal = [
        along[1] * across[2] - along[2] * across[1],
        along[2] * across[0] - along[0] * across[2],
        along[0] * across[1] - along[1] * across[0],
    ];
    0.5 * normal.iter().map(|c| c * c).sum::<f64>().sqrt()
}

/// The sorted cells of the used vertices, and the search span around a cell (0 when exact).
struct Grid {
    keyed: Vec<([i64; 3], u32)>,
    span: i64,
}

impl Grid {
    fn build(
        mesh: &TriangleMesh,
        used: &[bool],
        tolerance: f64,
        cancel: &mut Cancellation<'_>,
    ) -> Result<Self, RepairError> {
        let mut keyed = Vec::new();
        keyed
            .try_reserve_exact(used.iter().filter(|&&u| u).count())
            .map_err(|_| allocation("weld grid"))?;
        for (v, point) in mesh.vertices().iter().enumerate() {
            cancel.poll(Checkpoint::PreparationRecords, v)?;
            if !used[v] {
                continue;
            }
            let key = if tolerance == 0.0 {
                // Exact positions: the coordinate bits (signed zero normalized) as the cell.
                point
                    .key()
                    .map(|bits| i64::from_ne_bytes(bits.to_ne_bytes()))
            } else {
                let mut cell = [0i64; 3];
                for (slot, value) in cell.iter_mut().zip(point.coordinates()) {
                    let scaled = (value / tolerance).floor();
                    if scaled.is_nan() || scaled.abs() >= MAX_CELL {
                        return Err(RepairError::Invalid(
                            "weld tolerance too small for the model's coordinates",
                        ));
                    }
                    #[allow(clippy::cast_possible_truncation)] // Bounded by MAX_CELL above.
                    {
                        *slot = scaled as i64;
                    }
                }
                cell
            };
            let index = u32::try_from(v).map_err(|_| allocation("weld vertex index"))?;
            keyed.push((key, index));
        }
        cancel.check(Checkpoint::PreparationRecords)?;
        crate::parallel::sort_by_key(&mut keyed, |&k| k);
        Ok(Self {
            keyed,
            span: i64::from(tolerance != 0.0),
        })
    }

    /// Visit the vertices in the cells around `v`'s cell `key`; stops when `visit` returns true.
    fn around(&self, key: [i64; 3], mut visit: impl FnMut(u32) -> bool) -> bool {
        for dx in -self.span..=self.span {
            for dy in -self.span..=self.span {
                let low = ([key[0] + dx, key[1] + dy, key[2] - self.span], 0);
                let high = ([key[0] + dx, key[1] + dy, key[2] + self.span], u32::MAX);
                let start = self.keyed.partition_point(|&k| k < low);
                let end = self.keyed.partition_point(|&k| k <= high);
                if self.keyed[start..end].iter().any(|&(_, w)| visit(w)) {
                    return true;
                }
            }
        }
        false
    }
}

/// The vertices of edges used by exactly one face (faces with a repeated vertex left out).
fn open_vertices(
    mesh: &TriangleMesh,
    cancel: &mut Cancellation<'_>,
) -> Result<Vec<bool>, RepairError> {
    let mut edges: Vec<u64> = Vec::new();
    edges
        .try_reserve_exact(mesh.triangles().len() * 3)
        .map_err(|_| allocation("weld edges"))?;
    for (i, &[a, b, c]) in mesh.triangles().iter().enumerate() {
        cancel.poll(Checkpoint::PreparationRecords, i)?;
        if a != b && b != c && a != c {
            for (p, q) in [(a, b), (b, c), (c, a)] {
                edges.push(u64::from(p.min(q)) << 32 | u64::from(p.max(q)));
            }
        }
    }
    crate::parallel::sort_by_key(&mut edges, |&e| e);
    cancel.check(Checkpoint::PreparationRecords)?;
    let mut open = filled(mesh.vertices().len(), false, "weld vertex flags")?;
    let mut i = 0;
    while i < edges.len() {
        let mut j = i + 1;
        while j < edges.len() && edges[j] == edges[i] {
            j += 1;
        }
        if j - i == 1 {
            #[allow(clippy::cast_possible_truncation)] // Both halves are u32 vertex indices.
            for v in [edges[i] >> 32, edges[i] & u64::from(u32::MAX)] {
                open[v as usize] = true;
            }
        }
        i = j;
    }
    Ok(open)
}

/// Weld `mesh` at `tolerance` (see the module documentation). Returns the derived mesh, or `None`
/// when no vertex merged, and the report.
///
/// # Errors
/// Cancellation, allocation and mesh construction failures, and a tolerance so small that the
/// model's coordinates exceed the welding grid.
pub(super) fn weld(
    mesh: &TriangleMesh,
    tolerance: f64,
    cancel: &mut Cancellation<'_>,
) -> Result<(Option<TriangleMesh>, WeldReport), RepairError> {
    let mut report = WeldReport::default();
    let count = mesh.vertices().len();
    let position = |v: u32| mesh.vertices()[v as usize].coordinates();
    let limit = tolerance * tolerance;
    let close = |a: u32, b: u32| squared_distance(position(a), position(b)) <= limit;
    let open = open_vertices(mesh, cancel)?;
    let grid = Grid::build(mesh, &open, tolerance, cancel)?;
    // Which vertices have a neighbour within the tolerance, on every core.
    let mut near = filled(grid.keyed.len(), false, "weld neighbour flags")?;
    crate::parallel::fill_with(&mut near, |i| {
        let (key, v) = grid.keyed[i];
        grid.around(key, |w| w != v && close(v, w))
    });
    cancel.check(Checkpoint::PreparationRecords)?;
    let mut key_of = filled(count, None, "weld cells")?;
    for (&(key, v), &near) in grid.keyed.iter().zip(&near) {
        if near {
            key_of[v as usize] = Some(key);
        }
    }
    drop(near);
    // Clusters, grown sequentially from the smallest unmerged vertex.
    let mut kept = filled(count, NONE, "weld representatives")?;
    let mut candidates: Vec<(f64, u32)> = Vec::new();
    let mut members: Vec<u32> = Vec::new();
    for seed in 0..count {
        cancel.poll(Checkpoint::PreparationRecords, seed)?;
        let Some(key) = key_of[seed] else { continue };
        if kept[seed] != NONE {
            continue;
        }
        let seed = u32::try_from(seed).map_err(|_| allocation("weld vertex index"))?;
        candidates.clear();
        grid.around(key, |w| {
            if w != seed && kept[w as usize] == NONE && close(seed, w) {
                candidates.push((squared_distance(position(seed), position(w)), w));
            }
            false
        });
        candidates.sort_by(|p, q| p.0.total_cmp(&q.0).then(p.1.cmp(&q.1)));
        members.clear();
        members.push(seed);
        for &(_, w) in &candidates {
            if members.iter().all(|&m| close(m, w)) {
                members.push(w);
            }
        }
        if members.len() < 2 {
            continue;
        }
        report.clusters += 1;
        report.vertices_merged += members.len() - 1;
        for &m in &members {
            kept[m as usize] = seed;
            report.max_displacement_mm = report
                .max_displacement_mm
                .max(squared_distance(position(seed), position(m)).sqrt());
        }
        record(&mut report.samples, RepairChange::Welded, position(seed))
            .map_err(|_| allocation("weld samples"))?;
    }
    if report.clusters == 0 {
        return Ok((None, report));
    }
    // New indices: merged vertices dropped, the others in order.
    let mut index = filled(count, NONE, "weld index map")?;
    let mut vertices = Vec::new();
    vertices
        .try_reserve_exact(count - report.vertices_merged)
        .map_err(|_| allocation("welded vertices"))?;
    for (v, point) in mesh.vertices().iter().enumerate() {
        if kept[v] == NONE || kept[v] as usize == v {
            index[v] = u32::try_from(vertices.len()).map_err(|_| allocation("weld index"))?;
            vertices.push(*point);
        }
    }
    let target = |v: u32| {
        let k = kept[v as usize];
        index[if k == NONE { v } else { k } as usize]
    };
    let mut faces = Vec::new();
    faces
        .try_reserve_exact(mesh.triangles().len())
        .map_err(|_| allocation("welded faces"))?;
    for (i, face) in mesh.triangles().iter().enumerate() {
        cancel.poll(Checkpoint::PreparationRecords, i)?;
        let mapped = face.map(target);
        let repeated = |f: [u32; 3]| f[0] == f[1] || f[1] == f[2] || f[0] == f[2];
        if !repeated(*face) && repeated(mapped) {
            report.faces_collapsed += 1;
            report.collapsed_area_mm2 += area(face.map(position));
        } else {
            faces.push(mapped);
        }
    }
    let welded = TriangleMesh::try_new_with_cancellation(vertices, faces, cancel)
        .map_err(|e| RepairError::Topology(e.into()))?;
    Ok((Some(welded), report))
}
