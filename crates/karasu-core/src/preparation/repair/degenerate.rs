//! Degenerate faces (M6.4d r2, TD-092, stage R2).
//!
//! - **Needles with a repeated vertex** are always removed. Such a face is a segment: its two uses
//!   of the edge between its distinct vertices cancel, so removing it changes no other face's
//!   neighbourhood and no winding number. (Needles of distinct vertices at one position need
//!   welding first.)
//! - With a positive tolerance `t`, faces are examined in index order:
//!   - **Short edges:** a face whose shortest edge is no longer than `t` has that edge collapsed.
//!     The edge's larger-index vertex moves onto the other (so by at most `t`) and the two faces
//!     on the edge are removed. Only when the edge has exactly two faces, in opposite directions;
//!     the two vertices' common neighbours are exactly those faces' apexes (the link condition, so
//!     no edge gains a third face); and no other face around the moved vertex degenerates or
//!     turns over.
//!   - **Flat faces (caps):** a face whose apex lies within `t` of its longest edge is flipped
//!     with the face across that edge: the two become the faces on the other diagonal of their
//!     quadrilateral, and no vertex moves. Only when the edge has exactly these two faces, in
//!     opposite directions; the other diagonal is not an edge yet; and both new faces have a
//!     normal facing like the neighbour's and an apex farther than `t` from their longest edge,
//!     so a flipped pair is never flipped back.
//!
//!   An operation waits for the next pass when it would read a vertex that another operation
//!   changed in the same pass, so results are deterministic. Collapses remove vertices and flips
//!   remove flat faces without creating any, so passes end; [`MAX_PASSES`] bounds them anyway.
//!   Faces still within the tolerance are counted as left.
//!
//! Exactly collinear caps that cannot be flipped are left to crack closing (TD-091), which splits
//! the neighbour along the whole crack instead.

use super::{weld::allocation, RepairError};
use crate::{
    cancellation::{Cancellation, Checkpoint},
    mesh::{
        repair::{record, RepairChange, RepairSample},
        TriangleMesh,
    },
};

/// Passes over the faces at most.
pub const MAX_PASSES: usize = 64;

/// What the degenerate-face stage did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DegenerateReport {
    /// Faces with a repeated vertex removed.
    pub needles_removed: usize,
    /// Short edges collapsed (each removes its two faces and one vertex).
    pub edges_collapsed: usize,
    /// Flat faces flipped with their neighbour.
    pub caps_flipped: usize,
    /// The largest distance a vertex moved, in mm (at most the tolerance).
    pub max_displacement_mm: f64,
    /// The largest distance of a flipped face's apex from its longest edge, in mm.
    pub max_cap_height_mm: f64,
    /// Faces still within the tolerance (a short edge or a flat apex) that no operation could
    /// change.
    pub left: usize,
    /// Where: up to [`crate::mesh::repair::MAX_REPAIR_SAMPLES`] per kind of change.
    pub samples: Vec<RepairSample>,
}

type Vec3 = [f64; 3];

fn sub(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: Vec3, b: Vec3) -> Vec3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot(a: Vec3, b: Vec3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn length(a: Vec3) -> f64 {
    dot(a, a).sqrt()
}

/// Twice the area vector of a triangle.
fn normal(p: [Vec3; 3]) -> Vec3 {
    cross(sub(p[1], p[0]), sub(p[2], p[0]))
}

/// The face's shortest edge `(k, length)` (from corner `k` to `k + 1`), and the distance of the
/// apex from its longest edge with that edge's index.
fn measure(p: [Vec3; 3]) -> ((usize, f64), (usize, f64)) {
    let lengths: [f64; 3] = std::array::from_fn(|k| length(sub(p[(k + 1) % 3], p[k])));
    let shortest = (0..3)
        .min_by(|&a, &b| lengths[a].total_cmp(&lengths[b]))
        .unwrap_or(0);
    let longest = (0..3)
        .max_by(|&a, &b| lengths[a].total_cmp(&lengths[b]).then(b.cmp(&a)))
        .unwrap_or(0);
    let height = if lengths[longest] > 0.0 {
        length(normal(p)) / lengths[longest]
    } else {
        0.0
    };
    ((shortest, lengths[shortest]), (longest, height))
}

/// The alive faces around each vertex.
struct Incidence {
    offsets: Vec<usize>,
    faces: Vec<u32>,
}

impl Incidence {
    fn build(faces: &[[u32; 3]], alive: &[bool], vertices: usize) -> Result<Self, RepairError> {
        let mut offsets = Vec::new();
        offsets
            .try_reserve_exact(vertices + 1)
            .map_err(|_| allocation("degenerate face incidence"))?;
        offsets.resize(vertices + 1, 0usize);
        for (t, _) in faces.iter().zip(alive).filter(|(_, &a)| a) {
            for &v in t {
                offsets[v as usize + 1] += 1;
            }
        }
        for v in 0..vertices {
            offsets[v + 1] += offsets[v];
        }
        let mut list = Vec::new();
        list.try_reserve_exact(offsets[vertices])
            .map_err(|_| allocation("degenerate face incidence"))?;
        list.resize(offsets[vertices], 0u32);
        let mut next = offsets.clone();
        for (face, t) in faces.iter().enumerate() {
            if alive[face] {
                for &v in t {
                    list[next[v as usize]] = u32::try_from(face).unwrap_or(u32::MAX);
                    next[v as usize] += 1;
                }
            }
        }
        Ok(Self {
            offsets,
            faces: list,
        })
    }

    fn around(&self, v: u32) -> &[u32] {
        &self.faces[self.offsets[v as usize]..self.offsets[v as usize + 1]]
    }
}

/// The working state of the stage.
struct Work<'a> {
    mesh: &'a TriangleMesh,
    faces: Vec<[u32; 3]>,
    alive: Vec<bool>,
    removed: Vec<bool>,
    tolerance: f64,
}

impl Work<'_> {
    fn position(&self, v: u32) -> Vec3 {
        self.mesh.vertices()[v as usize].coordinates()
    }

    fn corners(&self, face: u32) -> [Vec3; 3] {
        self.faces[face as usize].map(|v| self.position(v))
    }

    /// Collapse the edge `{p, q}` if it is valid; returns the faces read (to mark) on success.
    fn collapse(
        &mut self,
        incidence: &Incidence,
        dirty: &[bool],
        p: u32,
        q: u32,
    ) -> Option<(Vec<u32>, f64)> {
        let (keep, gone) = (p.min(q), p.max(q));
        let alive = |f: &u32| self.alive[*f as usize];
        let read: Vec<u32> = incidence
            .around(keep)
            .iter()
            .chain(incidence.around(gone))
            .copied()
            .filter(alive)
            .collect();
        if read
            .iter()
            .any(|&f| self.faces[f as usize].iter().any(|&v| dirty[v as usize]))
        {
            return None;
        }
        let on_edge: Vec<u32> = incidence
            .around(keep)
            .iter()
            .copied()
            .filter(alive)
            .filter(|&f| self.faces[f as usize].contains(&gone))
            .collect();
        let directed = |f: u32, a: u32, b: u32| {
            let t = self.faces[f as usize];
            (0..3).any(|k| t[k] == a && t[(k + 1) % 3] == b)
        };
        let [first, second] = on_edge[..] else {
            return None;
        };
        if directed(first, keep, gone) == directed(second, keep, gone) {
            return None;
        }
        let apex = |f: u32| {
            let t = self.faces[f as usize];
            t.into_iter().find(|&v| v != keep && v != gone)
        };
        let mut apexes = [apex(first)?, apex(second)?];
        apexes.sort_unstable();
        if apexes[0] == apexes[1] {
            return None;
        }
        // Link condition: the common neighbours are exactly the two apexes.
        let neighbours = |v: u32| {
            let mut n: Vec<u32> = incidence
                .around(v)
                .iter()
                .copied()
                .filter(alive)
                .flat_map(|f| self.faces[f as usize])
                .filter(|&w| w != keep && w != gone)
                .collect();
            n.sort_unstable();
            n.dedup();
            n
        };
        let (near_keep, near_gone) = (neighbours(keep), neighbours(gone));
        let common: Vec<u32> = near_keep
            .iter()
            .copied()
            .filter(|w| near_gone.binary_search(w).is_ok())
            .collect();
        if common != apexes {
            return None;
        }
        // No face around the moved vertex degenerates or turns over.
        let target = self.position(keep);
        for f in incidence.around(gone).iter().copied().filter(alive) {
            if f == first || f == second {
                continue;
            }
            let before = normal(self.corners(f));
            let after = normal(self.faces[f as usize].map(|v| {
                if v == gone {
                    target
                } else {
                    self.position(v)
                }
            }));
            if dot(after, after) <= 0.0 || dot(before, after) <= 0.0 {
                return None;
            }
        }
        for f in incidence.around(gone).iter().copied().filter(alive) {
            for v in &mut self.faces[f as usize] {
                if *v == gone {
                    *v = keep;
                }
            }
        }
        self.alive[first as usize] = false;
        self.alive[second as usize] = false;
        self.removed[gone as usize] = true;
        let moved = length(sub(self.position(gone), target));
        Some((read, moved))
    }

    /// Flip the face `face`'s longest edge (corners `k`, `k + 1`) with the face across it; returns
    /// the vertices changed and the apex height on success.
    fn flip(
        &mut self,
        incidence: &Incidence,
        dirty: &[bool],
        face: u32,
        edge: usize,
    ) -> Option<([u32; 4], f64)> {
        let corners = self.faces[face as usize];
        let (start, end, apex) = (
            corners[edge],
            corners[(edge + 1) % 3],
            corners[(edge + 2) % 3],
        );
        if [start, end, apex].iter().any(|&v| dirty[v as usize]) {
            return None;
        }
        let alive = |f: &u32| self.alive[*f as usize];
        let across: Vec<u32> = incidence
            .around(start)
            .iter()
            .copied()
            .filter(alive)
            .filter(|&f| f != face && self.faces[f as usize].contains(&end))
            .collect();
        let [other] = across[..] else {
            return None;
        };
        let neighbour = self.faces[other as usize];
        let turn = (0..3).find(|&at| neighbour[at] == end && neighbour[(at + 1) % 3] == start)?;
        let opposite = neighbour[(turn + 2) % 3];
        if opposite == apex || dirty[opposite as usize] {
            return None;
        }
        // The other diagonal must not be an edge yet.
        if incidence
            .around(apex)
            .iter()
            .copied()
            .filter(alive)
            .any(|f| self.faces[f as usize].contains(&opposite))
        {
            return None;
        }
        let first = [start, opposite, apex];
        let second = [opposite, end, apex];
        let reference = normal(self.corners(other));
        for new in [first, second] {
            let corners = new.map(|v| self.position(v));
            let area = normal(corners);
            let (_, (_, height)) = measure(corners);
            if dot(area, area) <= 0.0 || dot(area, reference) <= 0.0 || height <= self.tolerance {
                return None;
            }
        }
        let (_, (_, height)) = measure(self.corners(face));
        self.faces[face as usize] = first;
        self.faces[other as usize] = second;
        Some(([start, end, apex, opposite], height))
    }
}

/// Remove and repair degenerate faces of `mesh` at `tolerance` (see the module documentation).
/// Returns the derived mesh, or `None` when nothing changed, and the report.
///
/// # Errors
/// Cancellation, allocation and mesh construction failures.
#[allow(clippy::too_many_lines)] // Needles, the passes, then the compacted mesh.
pub(super) fn repair_degenerate(
    mesh: &TriangleMesh,
    tolerance: f64,
    cancel: &mut Cancellation<'_>,
) -> Result<(Option<TriangleMesh>, DegenerateReport), RepairError> {
    let mut report = DegenerateReport::default();
    let count = mesh.triangles().len();
    let mut faces = Vec::new();
    faces
        .try_reserve_exact(count)
        .map_err(|_| allocation("degenerate faces"))?;
    faces.extend_from_slice(mesh.triangles());
    let mut work = Work {
        mesh,
        faces,
        alive: vec![true; count],
        removed: vec![false; mesh.vertices().len()],
        tolerance,
    };
    let sample = |samples: &mut Vec<RepairSample>, change, point| {
        record(samples, change, point).map_err(|_| allocation("degenerate face samples"))
    };
    for face in 0..count {
        cancel.poll(Checkpoint::PreparationRecords, face)?;
        let [a, b, c] = work.faces[face];
        if a == b || b == c || a == c {
            work.alive[face] = false;
            report.needles_removed += 1;
            let p = work.corners(u32::try_from(face).unwrap_or(0));
            let centroid = std::array::from_fn(|k| (p[0][k] + p[1][k] + p[2][k]) / 3.0);
            sample(&mut report.samples, RepairChange::NeedleRemoved, centroid)?;
        }
    }
    if tolerance > 0.0 {
        for _ in 0..MAX_PASSES {
            let incidence = Incidence::build(&work.faces, &work.alive, mesh.vertices().len())?;
            let mut dirty = vec![false; mesh.vertices().len()];
            let mut changed = false;
            for face in 0..count {
                cancel.poll(Checkpoint::PreparationRecords, face)?;
                if !work.alive[face] {
                    continue;
                }
                let index = u32::try_from(face).map_err(|_| allocation("face index"))?;
                let t = work.faces[face];
                if t.iter().any(|&v| dirty[v as usize]) {
                    continue;
                }
                let ((short, shortest), (long, height)) = measure(work.corners(index));
                if shortest <= tolerance {
                    let Some((read, moved)) =
                        work.collapse(&incidence, &dirty, t[short], t[(short + 1) % 3])
                    else {
                        continue;
                    };
                    for f in read {
                        for v in work.faces[f as usize] {
                            dirty[v as usize] = true;
                        }
                    }
                    for v in t {
                        dirty[v as usize] = true;
                    }
                    report.edges_collapsed += 1;
                    report.max_displacement_mm = report.max_displacement_mm.max(moved);
                    let kept = t[short].min(t[(short + 1) % 3]);
                    sample(
                        &mut report.samples,
                        RepairChange::EdgeCollapsed,
                        work.position(kept),
                    )?;
                    changed = true;
                } else if height <= tolerance {
                    let Some((touched, height)) = work.flip(&incidence, &dirty, index, long) else {
                        continue;
                    };
                    for v in touched {
                        dirty[v as usize] = true;
                    }
                    report.caps_flipped += 1;
                    report.max_cap_height_mm = report.max_cap_height_mm.max(height);
                    sample(
                        &mut report.samples,
                        RepairChange::CapFlipped,
                        work.position(touched[2]),
                    )?;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        for face in 0..count {
            if work.alive[face] {
                let index = u32::try_from(face).map_err(|_| allocation("face index"))?;
                let ((_, shortest), (_, height)) = measure(work.corners(index));
                if shortest <= tolerance || height <= tolerance {
                    report.left += 1;
                }
            }
        }
    }
    if report.needles_removed + report.edges_collapsed + report.caps_flipped == 0 {
        return Ok((None, report));
    }
    // Drop the vertices collapsed away; every other vertex keeps its order.
    let mut index = vec![u32::MAX; mesh.vertices().len()];
    let mut vertices = Vec::new();
    vertices
        .try_reserve_exact(mesh.vertices().len())
        .map_err(|_| allocation("degenerate face vertices"))?;
    for (v, point) in mesh.vertices().iter().enumerate() {
        if !work.removed[v] {
            index[v] = u32::try_from(vertices.len()).map_err(|_| allocation("vertex index"))?;
            vertices.push(*point);
        }
    }
    let mut kept = Vec::new();
    kept.try_reserve_exact(count)
        .map_err(|_| allocation("degenerate face list"))?;
    kept.extend(
        work.faces
            .iter()
            .zip(&work.alive)
            .filter(|(_, &a)| a)
            .map(|(t, _)| t.map(|v| index[v as usize])),
    );
    let repaired = TriangleMesh::try_new_with_cancellation(vertices, kept, cancel)
        .map_err(|e| RepairError::Topology(e.into()))?;
    Ok((Some(repaired), report))
}
