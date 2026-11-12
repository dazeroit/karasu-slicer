//! Small and inner shells (stage R8) and the orientation of open parts by visibility (stage R6),
//! M6.4d r6, TD-092.
//!
//! **Shells** are faces joined through shared edges (any number of faces on an edge). A shell is
//! closed when every one of its edges is used equally in both directions; its signed volume is
//! then that of its interior, positive when it faces outward.
//!
//! - **Small shells:** a shell with less surface than an area bound, or a closed shell with less
//!   volume than a volume bound, is removed: a speck that prints as nothing useful.
//! - **Inner shells:** a closed outward shell that crosses no other shell and lies where the other
//!   shells' winding number is at least one is removed: it is material inside material, which the
//!   union of the bodies already fills. The winding number is the sum of the orientations of the
//!   closed shells whose interior contains one of its vertices, by exact ray parity per shell;
//!   a vertex on another shell, or inside an open one, decides nothing, and the shell is kept.
//!   Reversed shells (cavities) and bodies floating in a cavity are never removed.
//! - **Visibility:** a part joined through consistently oriented two-face edges that is open (a
//!   sheet, a surface with holes) has no inside to orient it by. Rays leave sample faces along
//!   their normal and against it; when the reverse side reaches the outside of the model's box
//!   unobstructed more often, the part is turned. A tie leaves it as it is.
//!
//! No vertex moves. Results do not depend on the thread count.

use super::{
    bvh::{BuildFailure, Bvh, ParityFailure},
    contact::improper_contact,
    distance::Triangle,
};
use crate::{
    cancellation::{Cancellation, Checkpoint},
    mesh::{
        repair::{record, RepairChange, RepairSample},
        topology::TopologyError,
        FaceNormal, MeshError, TriangleMesh,
    },
    preparation::repair::RepairError,
};

/// Which shells to remove.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shells {
    /// Remove shells with less surface than this, in mm²; 0 removes none by area.
    pub min_area_mm2: f64,
    /// Remove closed shells with less volume than this, in mm³; 0 removes none by volume.
    pub min_volume_mm3: f64,
    /// Remove closed outward shells inside other bodies.
    pub inner: bool,
}

/// What the shell stage did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ShellReport {
    /// Shells removed for their small area or volume.
    pub small_removed: usize,
    /// Their faces and area (mm², source coordinates).
    pub small_faces: usize,
    /// Their area.
    pub small_area_mm2: f64,
    /// Inner shells removed.
    pub inner_removed: usize,
    /// Their faces.
    pub inner_faces: usize,
    /// Their volume (mm³).
    pub inner_volume_mm3: f64,
    /// Where: the first face of up to [`crate::mesh::repair::MAX_REPAIR_SAMPLES`] shells per kind.
    pub samples: Vec<RepairSample>,
}

/// What the visibility stage did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct VisibilityReport {
    /// Open parts examined.
    pub open_parts: usize,
    /// Parts turned.
    pub parts_flipped: usize,
    /// Their faces.
    pub faces_flipped: usize,
    /// Parts whose two sides were equally visible, left as they were.
    pub undecided: usize,
    /// Where: the first face of up to [`crate::mesh::repair::MAX_REPAIR_SAMPLES`] turned parts.
    pub samples: Vec<RepairSample>,
}

/// Faces sampled per open part, spread evenly over its faces.
const VISIBILITY_SAMPLES: usize = 32;
/// Vertices of a shell tried for the inner test.
const INNER_PROBES: usize = 8;

type Vec3 = [f64; 3];

fn allocation(context: &'static str) -> RepairError {
    RepairError::Topology(TopologyError::Mesh(MeshError::Allocation { context }))
}

fn root(parent: &mut [u32], mut x: u32) -> u32 {
    while parent[x as usize] != x {
        let up = parent[parent[x as usize] as usize];
        parent[x as usize] = up;
        x = up;
    }
    x
}

fn cross(u: Vec3, v: Vec3) -> Vec3 {
    [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ]
}

fn sub(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn length(v: Vec3) -> f64 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

/// Directed edge uses `(undirected key, +1 or -1, face)` of the usable faces, sorted.
fn edge_uses(
    mesh: &TriangleMesh,
    usable: &[bool],
    cancel: &mut Cancellation<'_>,
) -> Result<Vec<(u64, i8, u32)>, RepairError> {
    let mut uses = Vec::new();
    uses.try_reserve_exact(mesh.triangles().len() * 3)
        .map_err(|_| allocation("shell edge uses"))?;
    for (face, t) in mesh.triangles().iter().enumerate() {
        cancel.poll(Checkpoint::PreparationRecords, face)?;
        if !usable[face] {
            continue;
        }
        let face = u32::try_from(face).map_err(|_| allocation("face index"))?;
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            uses.push((
                u64::from(a.min(b)) << 32 | u64::from(a.max(b)),
                if a < b { 1 } else { -1 },
                face,
            ));
        }
    }
    crate::parallel::sort_by_key(&mut uses, |&u| u);
    cancel.check(Checkpoint::PreparationRecords)?;
    Ok(uses)
}

/// Runs of equal edges in sorted uses.
fn runs(uses: &[(u64, i8, u32)]) -> impl Iterator<Item = &[(u64, i8, u32)]> {
    let mut start = 0;
    std::iter::from_fn(move || {
        (start < uses.len()).then(|| {
            let end = start + uses[start..].partition_point(|u| u.0 == uses[start].0);
            let run = &uses[start..end];
            start = end;
            run
        })
    })
}

fn usable_faces(mesh: &TriangleMesh) -> Vec<bool> {
    (0..mesh.triangles().len())
        .map(|f| matches!(mesh.triangle_normal(f), Some(FaceNormal::Unit(_))))
        .collect()
}

/// `mesh` without the faces not `kept`, vertices unchanged.
fn without(
    mesh: &TriangleMesh,
    kept: &[bool],
    cancel: &mut Cancellation<'_>,
) -> Result<TriangleMesh, RepairError> {
    let mut faces = Vec::new();
    faces
        .try_reserve_exact(mesh.triangles().len())
        .map_err(|_| allocation("shell faces"))?;
    faces.extend(
        mesh.triangles()
            .iter()
            .zip(kept)
            .filter(|(_, &k)| k)
            .map(|(t, _)| *t),
    );
    let mut vertices = Vec::new();
    vertices
        .try_reserve_exact(mesh.vertices().len())
        .map_err(|_| allocation("shell vertices"))?;
    vertices.extend_from_slice(mesh.vertices());
    TriangleMesh::try_new_with_cancellation(vertices, faces, cancel)
        .map_err(|e| RepairError::Topology(e.into()))
}

/// One shell: its faces, area, closedness and signed volume (six times, with its error bound).
struct Shell {
    faces: Vec<u32>,
    area: f64,
    closed: bool,
    volume6: f64,
    bound: f64,
}

/// The shells of the usable faces, in order of their first face.
fn shells(
    mesh: &TriangleMesh,
    usable: &[bool],
    uses: &[(u64, i8, u32)],
    cancel: &mut Cancellation<'_>,
) -> Result<(Vec<Shell>, Vec<u32>), RepairError> {
    let count = mesh.triangles().len();
    let mut parent: Vec<u32> =
        (0..u32::try_from(count).map_err(|_| allocation("shells"))?).collect();
    let mut open_face = vec![false; count];
    for run in runs(uses) {
        let first = run[0].2;
        for u in &run[1..] {
            let (a, b) = (root(&mut parent, first), root(&mut parent, u.2));
            if a != b {
                parent[a.max(b) as usize] = a.min(b);
            }
        }
        if run.iter().map(|u| i64::from(u.1)).sum::<i64>() != 0 {
            for u in run {
                open_face[u.2 as usize] = true;
            }
        }
    }
    let mut slot = vec![u32::MAX; count];
    let mut list: Vec<Shell> = Vec::new();
    let mut shell_of = vec![u32::MAX; count];
    let corners = |f: usize| mesh.triangles()[f].map(|v| mesh.vertices()[v as usize].coordinates());
    for face in 0..count {
        cancel.poll(Checkpoint::PreparationRecords, face)?;
        if !usable[face] {
            continue;
        }
        let r = root(&mut parent, u32::try_from(face).unwrap_or(0)) as usize;
        if slot[r] == u32::MAX {
            slot[r] = u32::try_from(list.len()).map_err(|_| allocation("shells"))?;
            list.push(Shell {
                faces: Vec::new(),
                area: 0.0,
                closed: true,
                volume6: 0.0,
                bound: 0.0,
            });
        }
        let index = slot[r];
        shell_of[face] = index;
        let shell = &mut list[index as usize];
        shell.faces.push(u32::try_from(face).unwrap_or(0));
        let [a, b, c] = corners(face);
        shell.area += 0.5 * length(cross(sub(b, a), sub(c, a)));
        shell.closed &= !open_face[face];
        // Six times the signed volume against the origin, with an absolute error bound.
        let det = a[0] * (b[1] * c[2] - b[2] * c[1])
            + a[1] * (b[2] * c[0] - b[0] * c[2])
            + a[2] * (b[0] * c[1] - b[1] * c[0]);
        let magnitude = a[0].abs() * ((b[1] * c[2]).abs() + (b[2] * c[1]).abs())
            + a[1].abs() * ((b[2] * c[0]).abs() + (b[0] * c[2]).abs())
            + a[2].abs() * ((b[0] * c[1]).abs() + (b[1] * c[0]).abs());
        shell.volume6 += det;
        shell.bound += magnitude;
    }
    #[allow(clippy::cast_precision_loss)] // A generous bound; the count's precision is irrelevant.
    let length = count as f64 + 16.0;
    for shell in &mut list {
        shell.bound *= length * f64::EPSILON;
    }
    Ok((list, shell_of))
}

/// Remove small and inner shells (see the module documentation).
///
/// # Errors
/// Cancellation, allocation and mesh construction failures.
#[allow(clippy::too_many_lines)] // Shells, small ones, inner ones, then the mesh.
pub(in crate::preparation) fn remove_shells(
    mesh: &TriangleMesh,
    spec: Shells,
    cancel: &mut Cancellation<'_>,
) -> Result<(Option<TriangleMesh>, ShellReport), RepairError> {
    let mut report = ShellReport::default();
    let usable = usable_faces(mesh);
    let uses = edge_uses(mesh, &usable, cancel)?;
    let (list, shell_of) = shells(mesh, &usable, &uses, cancel)?;
    drop(uses);
    let centroid = |f: u32| {
        let p = mesh.triangles()[f as usize].map(|v| mesh.vertices()[v as usize].coordinates());
        std::array::from_fn(|k| (p[0][k] + p[1][k] + p[2][k]) / 3.0)
    };
    let mut removed = vec![false; list.len()];
    for (index, shell) in list.iter().enumerate() {
        let volume = shell.volume6.abs() / 6.0;
        let small =
            shell.area < spec.min_area_mm2 || (shell.closed && volume < spec.min_volume_mm3);
        if small {
            removed[index] = true;
            report.small_removed += 1;
            report.small_faces += shell.faces.len();
            report.small_area_mm2 += shell.area;
            record(
                &mut report.samples,
                RepairChange::SmallShellRemoved,
                centroid(shell.faces[0]),
            )
            .map_err(|_| allocation("shell samples"))?;
        }
    }
    if spec.inner && list.len() > 1 {
        // Every usable face, labelled by its shell.
        let mut triangles = Vec::new();
        let mut labels = Vec::new();
        let mut faces = Vec::new();
        for (face, t) in mesh.triangles().iter().enumerate() {
            if usable[face] {
                triangles.push(Triangle::new(
                    t.map(|v| mesh.vertices()[v as usize].coordinates()),
                ));
                labels.push(shell_of[face]);
                faces.push(*t);
            }
        }
        let bvh = Bvh::build_labeled(triangles, &labels, cancel, Checkpoint::PreparationRecords)
            .map_err(|e| match e {
                BuildFailure::Cancelled(c) => RepairError::Cancelled(c),
                BuildFailure::Allocation => allocation("shell index"),
            })?;
        // Shells that cross another shell.
        let mut crossing = vec![false; list.len()];
        let corners = |i: usize| faces[i].map(|v| mesh.vertices()[v as usize].coordinates());
        bvh.find_pair(cancel, Checkpoint::PreparationRecords, |a, b| {
            let (a, b) = (a as usize, b as usize);
            let (sa, sb) = (labels[a], labels[b]);
            if sa != sb && improper_contact(faces[a], corners(a), faces[b], corners(b)) {
                crossing[sa as usize] = true;
                crossing[sb as usize] = true;
            }
            false
        })?;
        let sign = |s: &Shell| {
            if s.volume6 > s.bound {
                1
            } else if s.volume6 < -s.bound {
                -1
            } else {
                0
            }
        };
        for (index, shell) in list.iter().enumerate() {
            cancel.check(Checkpoint::PreparationRecords)?;
            if removed[index] || crossing[index] || !shell.closed || sign(shell) != 1 {
                continue;
            }
            let label = u32::try_from(index).map_err(|_| allocation("shell label"))?;
            // The other shells' winding number at one of its vertices, when one decides it.
            let mut probes = shell
                .faces
                .iter()
                .flat_map(|&f| mesh.triangles()[f as usize])
                .take(INNER_PROBES * 3)
                .collect::<Vec<u32>>();
            probes.dedup();
            let mut winding = None;
            for v in probes.into_iter().take(INNER_PROBES) {
                let point = mesh.vertices()[v as usize].coordinates();
                match bvh.vertical_odd_labels(point, Some(label), None) {
                    Ok(odd) => {
                        let decided = odd.iter().all(|&l| list[l as usize].closed);
                        if decided {
                            winding =
                                Some(odd.iter().map(|&l| sign(&list[l as usize])).sum::<i64>());
                        }
                        break;
                    }
                    Err(ParityFailure::OnSurface) => {}
                    Err(ParityFailure::Allocation) => return Err(allocation("shell parity")),
                }
            }
            if winding.is_some_and(|w| w >= 1) {
                removed[index] = true;
                report.inner_removed += 1;
                report.inner_faces += shell.faces.len();
                report.inner_volume_mm3 += shell.volume6 / 6.0;
                record(
                    &mut report.samples,
                    RepairChange::InnerShellRemoved,
                    centroid(shell.faces[0]),
                )
                .map_err(|_| allocation("shell samples"))?;
            }
        }
    }
    if report.small_removed + report.inner_removed == 0 {
        return Ok((None, report));
    }
    let kept: Vec<bool> = (0..mesh.triangles().len())
        .map(|f| shell_of[f] == u32::MAX || !removed[shell_of[f] as usize])
        .collect();
    Ok((Some(without(mesh, &kept, cancel)?), report))
}

/// Turn open parts whose reverse side is more visible from outside (see the module
/// documentation).
///
/// # Errors
/// Cancellation, allocation and mesh construction failures.
#[allow(clippy::too_many_lines)] // Parts, sampled rays, then the mesh.
pub(in crate::preparation) fn orient_by_visibility(
    mesh: &TriangleMesh,
    cancel: &mut Cancellation<'_>,
) -> Result<(Option<TriangleMesh>, VisibilityReport), RepairError> {
    let mut report = VisibilityReport::default();
    let usable = usable_faces(mesh);
    let uses = edge_uses(mesh, &usable, cancel)?;
    let count = mesh.triangles().len();
    // Parts: faces joined through consistently oriented two-face edges; open when one of their
    // edges is unbalanced.
    let mut parent: Vec<u32> =
        (0..u32::try_from(count).map_err(|_| allocation("parts"))?).collect();
    let mut open_face = vec![false; count];
    for run in runs(&uses) {
        if let [first, second] = run {
            if first.1 != second.1 {
                let (a, b) = (root(&mut parent, first.2), root(&mut parent, second.2));
                if a != b {
                    parent[a.max(b) as usize] = a.min(b);
                }
            }
        }
        if run.iter().map(|u| i64::from(u.1)).sum::<i64>() != 0 {
            for u in run {
                open_face[u.2 as usize] = true;
            }
        }
    }
    drop(uses);
    let mut members: Vec<(u32, u32)> = (0..count)
        .filter(|&f| usable[f])
        .map(|f| {
            (
                root(&mut parent, u32::try_from(f).unwrap_or(0)),
                u32::try_from(f).unwrap_or(0),
            )
        })
        .collect();
    members.sort_unstable();
    // Parts that have an open face.
    let mut open_parts: Vec<&[(u32, u32)]> = Vec::new();
    let mut start = 0;
    while start < members.len() {
        let end = start + members[start..].partition_point(|m| m.0 == members[start].0);
        let part = &members[start..end];
        if part.iter().any(|&(_, f)| open_face[f as usize]) {
            open_parts.push(part);
        }
        start = end;
    }
    report.open_parts = open_parts.len();
    if open_parts.is_empty() {
        return Ok((None, report));
    }
    let triangles: Vec<Triangle> = mesh
        .triangles()
        .iter()
        .enumerate()
        .filter(|&(f, _)| usable[f])
        .map(|(_, t)| Triangle::new(t.map(|v| mesh.vertices()[v as usize].coordinates())))
        .collect();
    let bvh =
        Bvh::build(triangles, cancel, Checkpoint::PreparationRecords).map_err(|e| match e {
            BuildFailure::Cancelled(c) => RepairError::Cancelled(c),
            BuildFailure::Allocation => allocation("visibility index"),
        })?;
    let bounds = mesh.statistics().source_bounds;
    let diagonal = bounds.map_or(1.0, |b| {
        let d = b.dimensions();
        (d.x * d.x + d.y * d.y + d.z * d.z).sqrt().max(1e-9)
    });
    let (reach, lift) = (2.0 * diagonal, 1e-7 * diagonal);
    let mut flip = vec![false; count];
    for part in open_parts {
        cancel.check(Checkpoint::PreparationRecords)?;
        let step = part.len().div_ceil(VISIBILITY_SAMPLES).max(1);
        let (mut forward, mut backward) = (0usize, 0usize);
        for &(_, face) in part.iter().step_by(step) {
            let p =
                mesh.triangles()[face as usize].map(|v| mesh.vertices()[v as usize].coordinates());
            let n = cross(sub(p[1], p[0]), sub(p[2], p[0]));
            let norm = length(n);
            if norm == 0.0 {
                continue;
            }
            let n = [n[0] / norm, n[1] / norm, n[2] / norm];
            let centre: Vec3 = std::array::from_fn(|k| (p[0][k] + p[1][k] + p[2][k]) / 3.0);
            let ray = |sense: f64| {
                let from = std::array::from_fn(|k| centre[k] + sense * lift * n[k]);
                let to = std::array::from_fn(|k| centre[k] + sense * reach * n[k]);
                !bvh.segment_meets(from, to)
            };
            forward += usize::from(ray(1.0));
            backward += usize::from(ray(-1.0));
        }
        if backward > forward {
            report.parts_flipped += 1;
            report.faces_flipped += part.len();
            let first = part.iter().map(|m| m.1).min().unwrap_or(0);
            let p =
                mesh.triangles()[first as usize].map(|v| mesh.vertices()[v as usize].coordinates());
            record(
                &mut report.samples,
                RepairChange::VisibilityFlipped,
                std::array::from_fn(|k| (p[0][k] + p[1][k] + p[2][k]) / 3.0),
            )
            .map_err(|_| allocation("visibility samples"))?;
            for &(_, f) in part {
                flip[f as usize] = true;
            }
        } else if backward == forward {
            report.undecided += 1;
        }
    }
    if report.parts_flipped == 0 {
        return Ok((None, report));
    }
    let mut faces = Vec::new();
    faces
        .try_reserve_exact(count)
        .map_err(|_| allocation("visibility faces"))?;
    faces.extend(
        mesh.triangles()
            .iter()
            .zip(&flip)
            .map(|(&[a, b, c], &f)| if f { [a, c, b] } else { [a, b, c] }),
    );
    let mut vertices = Vec::new();
    vertices
        .try_reserve_exact(mesh.vertices().len())
        .map_err(|_| allocation("visibility vertices"))?;
    vertices.extend_from_slice(mesh.vertices());
    let turned = TriangleMesh::try_new_with_cancellation(vertices, faces, cancel)
        .map_err(|e| RepairError::Topology(e.into()))?;
    Ok((Some(turned), report))
}
