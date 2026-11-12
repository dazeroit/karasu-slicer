//! Non-manifold edges (M6.4d r4, TD-092, stage R5).
//!
//! An edge used unequally in its two directions by more than two faces is unbalanced: no winding
//! number is defined across it. Faces are grouped into parts connected through two-face edges, as
//! the topology's components are; a closed part uses every edge equally in both directions, so an
//! imbalance always comes from an open part.
//!
//! - **Fins (exact):** an open part is removed when it causes the imbalance and is surplus there:
//!   at some unbalanced non-manifold edge its own uses are unbalanced in the edge's direction of
//!   surplus; removing it makes no edge more unbalanced and some edge less; and it is not the
//!   largest part (by area) on that edge. A flap folded onto a face, or a sheet hanging from an
//!   edge, is removed whole; the body it hangs from never is. Parts are examined from the smallest
//!   area (then the smallest first face), and the edges' balance is updated after each removal.
//!   No vertex moves and no face changes; removed faces enclose no volume of their own.
//! - **Aggressive:** after the fins, every face on an edge that is still unbalanced and
//!   non-manifold is removed, leaving holes for hole filling (stage R7) to close.
//!
//! Faces with zero computed area are ignored, as topology ignores them. Results do not depend on
//! the thread count.

use super::{weld::allocation, RepairError};
use crate::{
    cancellation::{Cancellation, Checkpoint},
    mesh::{
        repair::{record, RepairChange, RepairSample},
        FaceNormal, TriangleMesh,
    },
};

/// How non-manifold edges are repaired.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NonManifold {
    /// Remove the open parts that cause an imbalance (fins), nothing else.
    Fins,
    /// Fins, then every face on an edge still unbalanced and non-manifold.
    Aggressive,
}

/// What the non-manifold stage did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NonManifoldReport {
    /// Open parts removed as fins.
    pub fins_removed: usize,
    /// Their faces.
    pub fin_faces: usize,
    /// Their area (mm², source coordinates).
    pub fin_area_mm2: f64,
    /// Faces removed by the aggressive mode from edges still unbalanced.
    pub faces_cut: usize,
    /// Their area (mm², source coordinates).
    pub cut_area_mm2: f64,
    /// Unbalanced non-manifold edges left.
    pub edges_left: usize,
    /// Where: the first face of up to [`crate::mesh::repair::MAX_REPAIR_SAMPLES`] fins and cuts.
    pub samples: Vec<RepairSample>,
}

type Vec3 = [f64; 3];

fn area(p: [Vec3; 3]) -> f64 {
    let (u, w): (Vec3, Vec3) = (
        std::array::from_fn(|k| p[1][k] - p[0][k]),
        std::array::from_fn(|k| p[2][k] - p[0][k]),
    );
    let cross = [
        u[1] * w[2] - u[2] * w[1],
        u[2] * w[0] - u[0] * w[2],
        u[0] * w[1] - u[1] * w[0],
    ];
    0.5 * cross.iter().map(|c| c * c).sum::<f64>().sqrt()
}

fn root(parent: &mut [u32], mut x: u32) -> u32 {
    while parent[x as usize] != x {
        let up = parent[parent[x as usize] as usize];
        parent[x as usize] = up;
        x = up;
    }
    x
}

/// One use of an edge: the undirected key, +1 when it runs from the smaller vertex, the face.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Use {
    edge: u64,
    sign: i8,
    face: u32,
}

/// The faces' undirected edges, sorted, with the start of every edge's run.
struct Edges {
    uses: Vec<Use>,
    starts: Vec<usize>,
}

impl Edges {
    fn build(
        mesh: &TriangleMesh,
        alive: &[bool],
        cancel: &mut Cancellation<'_>,
    ) -> Result<Self, RepairError> {
        let mut uses = Vec::new();
        uses.try_reserve_exact(mesh.triangles().len() * 3)
            .map_err(|_| allocation("non-manifold edge uses"))?;
        for (face, t) in mesh.triangles().iter().enumerate() {
            cancel.poll(Checkpoint::PreparationRecords, face)?;
            if !alive[face] {
                continue;
            }
            let face = u32::try_from(face).map_err(|_| allocation("face index"))?;
            for k in 0..3 {
                let (a, b) = (t[k], t[(k + 1) % 3]);
                uses.push(Use {
                    edge: u64::from(a.min(b)) << 32 | u64::from(a.max(b)),
                    sign: if a < b { 1 } else { -1 },
                    face,
                });
            }
        }
        crate::parallel::sort_by_key(&mut uses, |&u| u);
        cancel.check(Checkpoint::PreparationRecords)?;
        let mut starts = Vec::new();
        for (i, u) in uses.iter().enumerate() {
            if i == 0 || uses[i - 1].edge != u.edge {
                starts.push(i);
            }
        }
        starts.push(uses.len());
        Ok(Self { uses, starts })
    }

    fn runs(&self) -> impl Iterator<Item = &[Use]> {
        self.starts.windows(2).map(|w| &self.uses[w[0]..w[1]])
    }
}

/// One part's uses of one edge, when they do not balance.
#[derive(Clone, Copy)]
struct Entry {
    part: u32,
    edge: usize,
    net: i64,
}

/// Repair the non-manifold edges of `mesh` (see the module documentation). Returns the derived
/// mesh, or `None` when nothing changed, and the report.
///
/// # Errors
/// Cancellation, allocation and mesh construction failures.
#[allow(clippy::too_many_lines)] // Edges, parts, fins, then the aggressive cut and the mesh.
pub(super) fn repair_non_manifold(
    mesh: &TriangleMesh,
    mode: NonManifold,
    cancel: &mut Cancellation<'_>,
) -> Result<(Option<TriangleMesh>, NonManifoldReport), RepairError> {
    let mut report = NonManifoldReport::default();
    let count = mesh.triangles().len();
    let corners = |f: usize| mesh.triangles()[f].map(|v| mesh.vertices()[v as usize].coordinates());
    let mut alive = vec![false; count];
    for (face, slot) in alive.iter_mut().enumerate() {
        cancel.poll(Checkpoint::PreparationRecords, face)?;
        *slot = matches!(mesh.triangle_normal(face), Some(FaceNormal::Unit(_)));
    }
    let edges = Edges::build(mesh, &alive, cancel)?;
    // Parts: faces joined through two-face edges.
    let mut parent: Vec<u32> =
        (0..u32::try_from(count).map_err(|_| allocation("parts"))?).collect();
    for run in edges.runs() {
        if let [first, second] = run {
            let (a, b) = (
                root(&mut parent, first.face),
                root(&mut parent, second.face),
            );
            if a != b {
                parent[a.max(b) as usize] = a.min(b);
            }
        }
    }
    let part: Vec<u32> = (0..count)
        .map(|f| root(&mut parent, u32::try_from(f).unwrap_or(0)))
        .collect();
    let mut part_area = vec![0.0f64; count];
    for face in (0..count).filter(|&f| alive[f]) {
        part_area[part[face] as usize] += area(corners(face));
    }
    // Every edge's balance, and each part's unbalanced uses of it.
    let mut net: Vec<i64> = Vec::new();
    let mut non_manifold: Vec<bool> = Vec::new();
    let mut entries: Vec<Entry> = Vec::new();
    let mut by_part: Vec<(u32, i64)> = Vec::new();
    for (index, run) in edges.runs().enumerate() {
        cancel.poll(Checkpoint::PreparationRecords, index)?;
        net.push(run.iter().map(|u| i64::from(u.sign)).sum());
        non_manifold.push(run.len() > 2);
        by_part.clear();
        by_part.extend(
            run.iter()
                .map(|u| (part[u.face as usize], i64::from(u.sign))),
        );
        by_part.sort_unstable();
        let mut i = 0;
        while i < by_part.len() {
            let mut j = i;
            let mut sum = 0;
            while j < by_part.len() && by_part[j].0 == by_part[i].0 {
                sum += by_part[j].1;
                j += 1;
            }
            if sum != 0 {
                entries.push(Entry {
                    part: by_part[i].0,
                    edge: index,
                    net: sum,
                });
            }
            i = j;
        }
    }
    entries.sort_unstable_by_key(|e| (e.part, e.edge));
    let mut removed_part = vec![false; count];
    // Candidate fins: parts surplus at an unbalanced non-manifold edge, smallest first.
    let surplus = |e: &Entry, net: &[i64]| {
        non_manifold[e.edge] && net[e.edge] != 0 && net[e.edge].signum() == e.net.signum()
    };
    let mut candidates: Vec<u32> = entries
        .iter()
        .filter(|e| surplus(e, &net))
        .map(|e| e.part)
        .collect();
    candidates.sort_unstable();
    candidates.dedup();
    candidates.sort_by(|a, b| {
        part_area[*a as usize]
            .total_cmp(&part_area[*b as usize])
            .then(a.cmp(b))
    });
    let range = |p: u32| {
        let start = entries.partition_point(|e| e.part < p);
        let end = entries.partition_point(|e| e.part <= p);
        start..end
    };
    for candidate in candidates {
        cancel.check(Checkpoint::PreparationRecords)?;
        let own = &entries[range(candidate)];
        if !own.iter().any(|e| surplus(e, &net)) {
            continue;
        }
        // No edge more unbalanced, some edge less.
        let worse = own
            .iter()
            .any(|e| (net[e.edge] - e.net).abs() > net[e.edge].abs());
        let better = own
            .iter()
            .any(|e| (net[e.edge] - e.net).abs() < net[e.edge].abs());
        if worse || !better {
            continue;
        }
        // Never the largest part on an edge where it is surplus.
        let largest = own.iter().filter(|e| surplus(e, &net)).any(|e| {
            let run = &edges.uses[edges.starts[e.edge]..edges.starts[e.edge + 1]];
            run.iter()
                .map(|u| part[u.face as usize])
                .filter(|&p| p != candidate && !removed_part[p as usize])
                .all(|p| part_area[p as usize] < part_area[candidate as usize])
        });
        if largest {
            continue;
        }
        for e in own {
            net[e.edge] -= e.net;
        }
        removed_part[candidate as usize] = true;
        report.fins_removed += 1;
        report.fin_area_mm2 += part_area[candidate as usize];
        let first = (0..count)
            .find(|&f| alive[f] && part[f] == candidate)
            .unwrap_or(0);
        let p = corners(first);
        record(
            &mut report.samples,
            RepairChange::FinRemoved,
            std::array::from_fn(|k| (p[0][k] + p[1][k] + p[2][k]) / 3.0),
        )
        .map_err(|_| allocation("non-manifold samples"))?;
    }
    let mut keep: Vec<bool> = (0..count)
        .map(|f| !(alive[f] && removed_part[part[f] as usize]))
        .collect();
    report.fin_faces = keep.iter().filter(|&&k| !k).count();
    // Aggressive: cut every face still on an unbalanced non-manifold edge.
    for (index, run) in edges.runs().enumerate() {
        if !(non_manifold[index] && net[index] != 0) {
            continue;
        }
        if mode == NonManifold::Aggressive {
            for u in run {
                let face = u.face as usize;
                if keep[face] {
                    keep[face] = false;
                    report.faces_cut += 1;
                    report.cut_area_mm2 += area(corners(face));
                    let p = corners(face);
                    record(
                        &mut report.samples,
                        RepairChange::FacesCut,
                        std::array::from_fn(|k| (p[0][k] + p[1][k] + p[2][k]) / 3.0),
                    )
                    .map_err(|_| allocation("non-manifold samples"))?;
                }
            }
        } else if run.iter().any(|u| keep[u.face as usize]) {
            report.edges_left += 1;
        }
    }
    if report.fins_removed + report.faces_cut == 0 {
        return Ok((None, report));
    }
    let mut faces = Vec::new();
    faces
        .try_reserve_exact(count)
        .map_err(|_| allocation("non-manifold faces"))?;
    faces.extend(
        mesh.triangles()
            .iter()
            .zip(&keep)
            .filter(|(_, &k)| k)
            .map(|(t, _)| *t),
    );
    let mut vertices = Vec::new();
    vertices
        .try_reserve_exact(mesh.vertices().len())
        .map_err(|_| allocation("non-manifold vertices"))?;
    vertices.extend_from_slice(mesh.vertices());
    let repaired = TriangleMesh::try_new_with_cancellation(vertices, faces, cancel)
        .map_err(|e| RepairError::Topology(e.into()))?;
    Ok((Some(repaired), report))
}
