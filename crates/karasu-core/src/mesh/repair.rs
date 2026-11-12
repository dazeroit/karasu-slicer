//! Explicit, reported mesh repairs (TD-054, phase 2) producing a derived mesh. The source mesh is
//! never modified, and every change is counted in the [`RepairReport`].
//!
//! - **Duplicates:** faces with the same corner positions (exact-coordinate vertex identities) form
//!   a group whose winding contribution is the face times the group's net orientation count. Each
//!   group collapses to that contribution: the earliest face with the net orientation, or none
//!   when opposite faces cancel. A stacked group (net count 2 or more) collapses to one face only
//!   where that leaves every edge balanced that was balanced (a stray duplicate); otherwise it
//!   keeps as many faces as its net count (a face two overlapping bodies share on the same side,
//!   TD-092 r9). The winding number is unchanged wherever it was defined, and coincident double
//!   walls between touching bodies disappear.
//! - **Orientation:** within each component of faces connected through two-face edges, faces are
//!   flipped so that every such edge has opposite uses. The orientation class covering more face
//!   area is kept (on an exact tie, the class of the component's smallest face index).
//!   Non-orientable components are left unchanged and counted.
//! - **Outward:** a closed component that lies outside the bounds of every other closed component
//!   (its lexicographically smallest vertex is outside their bounding boxes) must enclose positive
//!   volume. It is flipped as a whole when its signed volume is negative beyond the evaluation's
//!   error bound. A nested component (inside the bounds of another) turns with the outermost
//!   components around it when all of them turn, so an inside-out model keeps its cavities;
//!   otherwise it is left unchanged (reversed ones are counted), because a reversed nested shell
//!   is usually a cavity.
//!
//! Degenerate faces are never changed. No vertex is moved, added or merged.
//!
//! Every change is also located (M6.4d r1, TD-092): up to [`MAX_REPAIR_SAMPLES`] positions per kind
//! of change, so an interface can show where the model was changed.

use super::{
    diagnostics,
    topology::{undirected, Classes, TopologyError, TopologyLimits, TopologyResource},
    TriangleMesh,
};
use crate::cancellation::{Cancellation, Checkpoint};
use crate::geometry::Point3;

/// Which repairs to apply.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RepairPolicy {
    /// Collapse duplicate faces to their net orientation.
    pub duplicates: bool,
    /// Make orientation consistent within each component.
    pub orientation: bool,
    /// Flip outermost closed components that enclose negative volume (requires `orientation`).
    pub outward: bool,
}

/// What the repairs changed.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RepairReport {
    /// Faces removed by duplicate collapse.
    pub dropped_duplicate_faces: usize,
    /// Duplicate groups removed entirely because their faces cancel.
    pub cancelled_duplicate_groups: usize,
    /// Faces flipped (consistency and outward flips).
    pub flipped_faces: usize,
    /// Area of the flipped faces (mm², source coordinates).
    pub flipped_area_mm2: f64,
    /// Components in which consistency flipped at least one face.
    pub reoriented_components: usize,
    /// Components left unchanged because their orientation constraints contradict.
    pub non_orientable_components: usize,
    /// Outermost closed components flipped whole to enclose positive volume.
    pub outward_flipped_components: usize,
    /// Nested closed components with negative volume, left unchanged (cavities or unresolved).
    pub nested_reversed_components: usize,
    /// Outermost closed components whose volume sign is within the evaluation error, unchanged.
    pub ambiguous_volume_components: usize,
}

/// A kind of change a repair made, or declined to make, for [`RepairSample`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RepairChange {
    /// Duplicate faces collapsed to one, or cancelled (the centroid of the group's face).
    Duplicates,
    /// Faces flipped to make a component's orientation consistent (the centroid of its first
    /// flipped face).
    Reoriented,
    /// A closed component turned outward whole (its lexicographically smallest vertex).
    TurnedOutward,
    /// An outermost closed component left unchanged because the sign of its volume is within
    /// the evaluation error (its lexicographically smallest vertex).
    AmbiguousVolume,
    /// A hole capped (the first vertex of its rim).
    HoleFilled,
    /// A crack of zero area closed by splitting the faces along it (the first vertex of its rim).
    CrackClosed,
    /// A hole left open (the first vertex of its rim).
    HoleRefused,
    /// Vertices welded into one (the kept vertex).
    Welded,
    /// A face with a repeated vertex removed (its centroid).
    NeedleRemoved,
    /// A short edge collapsed (the kept vertex).
    EdgeCollapsed,
    /// A flat face flipped with its neighbour (its apex).
    CapFlipped,
    /// An open part causing a non-manifold edge removed (its first face's centroid).
    FinRemoved,
    /// A face on an edge left non-manifold removed by the aggressive mode (its centroid).
    FacesCut,
    /// A shell below the area or volume bound removed (its first face's centroid).
    SmallShellRemoved,
    /// A closed shell inside another body removed (its first face's centroid).
    InnerShellRemoved,
    /// An open part turned to face where it is visible from outside (its first face's
    /// centroid).
    VisibilityFlipped,
    /// The source vertex farthest from a rebuilt surface (where a feature was lost).
    RebuildDeviation,
}

/// Where one repair changed the model.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RepairSample {
    /// What changed there.
    pub change: RepairChange,
    /// Source coordinates, in mm.
    pub point: [f64; 3],
}

/// At most this many positions are kept per kind of change.
pub const MAX_REPAIR_SAMPLES: usize = 16;

/// Keep `point` as a sample of `change` while that kind has fewer than [`MAX_REPAIR_SAMPLES`].
///
/// # Errors
/// The allocation of one more sample.
pub(crate) fn record(
    samples: &mut Vec<RepairSample>,
    change: RepairChange,
    point: [f64; 3],
) -> Result<(), std::collections::TryReserveError> {
    if samples.iter().filter(|s| s.change == change).count() < MAX_REPAIR_SAMPLES {
        samples.try_reserve(1)?;
        samples.push(RepairSample { change, point });
    }
    Ok(())
}

fn located(
    samples: &mut Vec<RepairSample>,
    change: RepairChange,
    point: [f64; 3],
) -> Result<(), TopologyError> {
    record(samples, change, point).map_err(|_| {
        TopologyError::Mesh(super::MeshError::Allocation {
            context: "mesh repair samples",
        })
    })
}

fn centroid(mesh: &TriangleMesh, face: [u32; 3]) -> [f64; 3] {
    let [a, b, c] = face.map(|v| mesh.vertices()[v as usize].coordinates());
    std::array::from_fn(|k| (a[k] + b[k] + c[k]) / 3.0)
}

impl RepairReport {
    /// Whether any face was removed or flipped.
    #[must_use]
    pub fn changed(&self) -> bool {
        self.dropped_duplicate_faces > 0 || self.flipped_faces > 0
    }
}

fn area(points: [Point3; 3]) -> f64 {
    let [origin, first, second] = points;
    let edge = |p: Point3| [p.x - origin.x, p.y - origin.y, p.z - origin.z];
    let (e1, e2) = (edge(first), edge(second));
    let cross = [
        e1[1] * e2[2] - e1[2] * e2[1],
        e1[2] * e2[0] - e1[0] * e2[2],
        e1[0] * e2[1] - e1[1] * e2[0],
    ];
    0.5 * (cross[0] * cross[0] + cross[1] * cross[1] + cross[2] * cross[2]).sqrt()
}

fn reserve<T>(items: &mut Vec<T>, count: usize) -> Result<(), TopologyError> {
    items.try_reserve_exact(count).map_err(|_| {
        TopologyError::Mesh(super::MeshError::Allocation {
            context: "mesh repair buffers",
        })
    })
}

fn filled<T: Clone>(count: usize, value: T) -> Result<Vec<T>, TopologyError> {
    let mut items = Vec::new();
    reserve(&mut items, count)?;
    items.resize(count, value);
    Ok(items)
}

/// Apply `policy` to `mesh`. Returns the derived mesh, or `None` when nothing changed, and the
/// report.
///
/// # Errors
/// Cancellation, the topology limits, and allocation or size failures; no partial mesh escapes.
pub(super) fn repair(
    mesh: &TriangleMesh,
    policy: RepairPolicy,
    limits: TopologyLimits,
    samples: &mut Vec<RepairSample>,
    cancel: &mut Cancellation<'_>,
) -> Result<(Option<TriangleMesh>, RepairReport), TopologyError> {
    cancel.check(Checkpoint::TopologyStart)?;
    let mut report = RepairReport::default();
    if !(policy.duplicates || policy.orientation || policy.outward) {
        return Ok((None, report));
    }
    for (count, limit, resource) in [
        (
            mesh.vertices().len(),
            limits.max_vertices,
            TopologyResource::Vertices,
        ),
        (
            mesh.triangles().len(),
            limits.max_triangles,
            TopologyResource::Triangles,
        ),
    ] {
        if count > limit {
            return Err(TopologyError::LimitExceeded { resource, limit });
        }
    }
    let mut faces = Vec::new();
    reserve(&mut faces, mesh.triangles().len())?;
    faces.extend_from_slice(mesh.triangles());
    cancel.check(Checkpoint::EdgeIncidence)?;
    let usable = diagnostics::usable_faces(mesh)?;
    let mut keep = filled(faces.len(), true)?;
    if policy.duplicates {
        collapse_duplicates(mesh, &usable, &mut keep, &mut report, samples, cancel)?;
    }
    if policy.orientation || policy.outward {
        let classes = orient(
            mesh,
            &mut faces,
            &usable,
            &keep,
            &mut report,
            samples,
            cancel,
        )?;
        if policy.outward {
            outward(
                mesh,
                &mut faces,
                &usable,
                &keep,
                classes,
                &mut report,
                samples,
                cancel,
            )?;
        }
    }
    if !report.changed() {
        return Ok((None, report));
    }
    let mut kept = Vec::new();
    reserve(&mut kept, faces.len() - report.dropped_duplicate_faces)?;
    kept.extend(faces.iter().zip(&keep).filter(|(_, &k)| k).map(|(&f, _)| f));
    let mut vertices = Vec::new();
    reserve(&mut vertices, mesh.vertices().len())?;
    vertices.extend_from_slice(mesh.vertices());
    let derived = TriangleMesh::try_new_with_cancellation(vertices, kept, cancel)?;
    Ok((Some(derived), report))
}

/// Collapse each group of faces with equal corner positions to its net orientation (see the
/// module documentation for stacked groups).
#[allow(clippy::too_many_lines)] // Groups, edge balance, stacked groups, then the drops.
fn collapse_duplicates(
    mesh: &TriangleMesh,
    usable: &[bool],
    keep: &mut [bool],
    report: &mut RepairReport,
    samples: &mut Vec<RepairSample>,
    cancel: &mut Cancellation<'_>,
) -> Result<(), TopologyError> {
    let canonical = diagnostics::canonical_vertices(mesh, cancel)?;
    let mut keys: Vec<([u32; 3], bool, u32)> = Vec::new();
    reserve(&mut keys, mesh.triangles().len())?;
    for (face, indices) in mesh.triangles().iter().enumerate() {
        cancel.poll(Checkpoint::EdgeIncidence, face)?;
        if !usable[face] {
            continue;
        }
        let ids = indices.map(|index| canonical[index as usize]);
        let k = (0..3).min_by_key(|&k| ids[k]).unwrap_or(0);
        let ascending = ids[(k + 1) % 3] < ids[(k + 2) % 3];
        let mut sorted = ids;
        sorted.sort_unstable();
        let face = u32::try_from(face).map_err(|_| TopologyError::SizeOverflow)?;
        keys.push((sorted, ascending, face));
    }
    cancel.check(Checkpoint::EdgeSorting)?;
    // Keys end with the face index, so they are unique.
    crate::parallel::sort_by_key(&mut keys, |&key| key);
    // Groups: (start, end, net, ascending wins).
    let mut groups = Vec::new();
    let mut i = 0;
    while i < keys.len() {
        cancel.poll(Checkpoint::EdgeIncidence, i)?;
        let mut j = i;
        while j < keys.len() && keys[j].0 == keys[i].0 {
            j += 1;
        }
        if j - i > 1 {
            let ascending = keys[i..j].iter().filter(|k| k.1).count();
            let descending = j - i - ascending;
            reserve(&mut groups, 1)?;
            groups.push((i, j, ascending.abs_diff(descending), ascending > descending));
        }
        i = j;
    }
    // Each edge's net use count, zero where the winding number is defined across it: needed
    // only for stacked groups, which most meshes have none of.
    let before = if groups.iter().any(|g| g.2 >= 2) {
        edge_balance(mesh, usable, &canonical, cancel)?
    } else {
        Vec::new()
    };
    drop(canonical);
    // A stacked group (net count 2 or more) collapses to one face unless that unbalances an edge
    // that was balanced: then it keeps its net count, as a face two overlapping bodies share on
    // the same side. Collapses are tried together and undone one by one until no balanced edge
    // is left unbalanced.
    let edge_of = |edge: (u32, u32)| before.binary_search_by_key(&edge, |e| e.0).ok();
    let mut after = Vec::new();
    reserve(&mut after, before.len())?;
    after.extend(before.iter().map(|e| e.1));
    let mut collapsed = filled(groups.len(), false)?;
    for (g, &(start, _, net, ascending)) in groups.iter().enumerate() {
        if net >= 2 {
            collapsed[g] = true;
            for (edge, sign) in group_edges(&keys[start].0, ascending) {
                if let Some(e) = edge_of(edge) {
                    #[allow(clippy::cast_possible_wrap)] // Group sizes are far below 2^63.
                    let change = (net - 1) as i64;
                    after[e] -= sign * change;
                }
            }
        }
    }
    loop {
        cancel.check(Checkpoint::EdgeIncidence)?;
        let mut undone = false;
        for (g, &(start, _, net, ascending)) in groups.iter().enumerate() {
            if !collapsed[g] {
                continue;
            }
            let edges = group_edges(&keys[start].0, ascending);
            let broken = edges
                .iter()
                .filter_map(|&(edge, _)| edge_of(edge))
                .any(|e| before[e].1 == 0 && after[e] != 0);
            if broken {
                collapsed[g] = false;
                undone = true;
                for (edge, sign) in edges {
                    if let Some(e) = edge_of(edge) {
                        #[allow(clippy::cast_possible_wrap)] // Group sizes are far below 2^63.
                        let change = (net - 1) as i64;
                        after[e] += sign * change;
                    }
                }
            }
        }
        if !undone {
            break;
        }
    }
    for (g, &(start, end, net, ascending)) in groups.iter().enumerate() {
        cancel.poll(Checkpoint::EdgeIncidence, g)?;
        let group = &keys[start..end];
        let kept = if collapsed[g] { 1 } else { net };
        if group.len() == kept {
            continue;
        }
        if net == 0 {
            report.cancelled_duplicate_groups += 1;
        }
        // The earliest faces with the net orientation survive.
        let mut survivors: Vec<u32> = group
            .iter()
            .filter(|k| k.1 == ascending)
            .map(|k| k.2)
            .collect();
        survivors.sort_unstable();
        survivors.truncate(kept);
        let first = group.iter().map(|k| k.2).min().unwrap_or(0);
        located(
            samples,
            RepairChange::Duplicates,
            centroid(mesh, mesh.triangles()[first as usize]),
        )?;
        for key in group {
            if !survivors.contains(&key.2) {
                keep[key.2 as usize] = false;
                report.dropped_duplicate_faces += 1;
            }
        }
    }
    Ok(())
}

/// An edge over positions `(lower, higher)` with a use count, +1 per use from lower to higher.
type EdgeCount = ((u32, u32), i64);

/// Each edge with its net use count, sorted.
fn edge_balance(
    mesh: &TriangleMesh,
    usable: &[bool],
    canonical: &[u32],
    cancel: &mut Cancellation<'_>,
) -> Result<Vec<EdgeCount>, TopologyError> {
    let mut uses: Vec<EdgeCount> = Vec::new();
    reserve(&mut uses, 3 * mesh.triangles().len())?;
    for (face, indices) in mesh.triangles().iter().enumerate() {
        cancel.poll(Checkpoint::EdgeIncidence, face)?;
        if usable[face] {
            let ids = indices.map(|index| canonical[index as usize]);
            for k in 0..3 {
                uses.push(directed(ids[k], ids[(k + 1) % 3]));
            }
        }
    }
    cancel.check(Checkpoint::EdgeSorting)?;
    crate::parallel::sort_by_key(&mut uses, |&key| key);
    let mut balance: Vec<EdgeCount> = Vec::new();
    for (i, &(edge, sign)) in uses.iter().enumerate() {
        cancel.poll(Checkpoint::EdgeIncidence, i)?;
        match balance.last_mut() {
            Some(last) if last.0 == edge => last.1 += sign,
            _ => {
                reserve(&mut balance, 1)?;
                balance.push((edge, sign));
            }
        }
    }
    Ok(balance)
}

/// An edge between positions `a` and `b` as `(lower, higher)`, with +1 when used lower to higher.
fn directed(a: u32, b: u32) -> EdgeCount {
    if a < b {
        ((a, b), 1)
    } else {
        ((b, a), -1)
    }
}

/// The directed edges of a face over the sorted positions `sorted`, wound ascending (the cyclic
/// order `sorted[0], sorted[1], sorted[2]`) or descending.
fn group_edges(sorted: &[u32; 3], ascending: bool) -> [EdgeCount; 3] {
    let [a, b, c] = if ascending {
        *sorted
    } else {
        [sorted[0], sorted[2], sorted[1]]
    };
    [directed(a, b), directed(b, c), directed(c, a)]
}

/// Directed uses `(from << 32 | to, face)` of kept usable faces, sorted by undirected edge.
fn sorted_uses(
    faces: &[[u32; 3]],
    usable: &[bool],
    keep: &[bool],
    cancel: &mut Cancellation<'_>,
) -> Result<Vec<(u64, u32)>, TopologyError> {
    let mut uses = Vec::new();
    reserve(
        &mut uses,
        faces
            .len()
            .checked_mul(3)
            .ok_or(TopologyError::SizeOverflow)?,
    )?;
    for (face, &[a, b, c]) in faces.iter().enumerate() {
        cancel.poll(Checkpoint::EdgeIncidence, face)?;
        if usable[face] && keep[face] {
            let face = u32::try_from(face).map_err(|_| TopologyError::SizeOverflow)?;
            for (from, to) in [(a, b), (b, c), (c, a)] {
                uses.push((u64::from(from) << 32 | u64::from(to), face));
            }
        }
    }
    cancel.check(Checkpoint::EdgeSorting)?;
    // Usable faces have three distinct corners, so `(undirected, directed, face)` is unique.
    crate::parallel::sort_by_key(&mut uses, |&(d, f)| (undirected(d), d, f));
    cancel.check(Checkpoint::EdgeSorting)?;
    Ok(uses)
}

/// The `[start, end)` ranges of equal undirected edges in sorted uses.
fn groups(uses: &[(u64, u32)]) -> impl Iterator<Item = (usize, usize)> + '_ {
    let mut start = 0;
    std::iter::from_fn(move || {
        (start < uses.len()).then(|| {
            let key = undirected(uses[start].0);
            let mut end = start + 1;
            while end < uses.len() && undirected(uses[end].0) == key {
                end += 1;
            }
            let range = (start, end);
            start = end;
            range
        })
    })
}

fn flip(face: &mut [u32; 3]) {
    face.swap(1, 2);
}

/// Consistent orientation per component; returns the components (union-find classes).
fn orient(
    mesh: &TriangleMesh,
    faces: &mut [[u32; 3]],
    usable: &[bool],
    keep: &[bool],
    report: &mut RepairReport,
    samples: &mut Vec<RepairSample>,
    cancel: &mut Cancellation<'_>,
) -> Result<Classes, TopologyError> {
    let uses = sorted_uses(faces, usable, keep, cancel)?;
    let mut classes = Classes::new(faces.len())?;
    for (index, (start, end)) in groups(&uses).enumerate() {
        cancel.poll(Checkpoint::EdgeIncidence, index)?;
        if end - start == 2 {
            let same = uses[start].0 == uses[start + 1].0;
            classes.join(uses[start].1, uses[start + 1].1, u8::from(same));
        }
    }
    drop(uses);
    // Area per orientation class, accumulated at each component's root.
    let mut areas = filled(faces.len(), [0f64; 2])?;
    let count = u32::try_from(faces.len()).map_err(|_| TopologyError::SizeOverflow)?;
    for face in 0..count {
        cancel.poll(Checkpoint::EdgeIncidence, face as usize)?;
        if usable[face as usize] && keep[face as usize] {
            let (root, parity) = classes.find(face);
            let a = area(faces[face as usize].map(|v| mesh.vertices()[v as usize]));
            areas[root as usize][usize::from(parity)] += a;
        }
    }
    let mut touched = filled(faces.len(), false)?;
    for face in 0..count {
        cancel.poll(Checkpoint::EdgeIncidence, face as usize)?;
        if !(usable[face as usize] && keep[face as usize]) {
            continue;
        }
        let (root, parity) = classes.find(face);
        if classes.contradictory(root) {
            continue;
        }
        let [kept_area, other_area] = areas[root as usize];
        // The root's class (parity 0) wins exact ties.
        let keep_parity = u8::from(other_area > kept_area);
        if parity != keep_parity {
            let a = area(faces[face as usize].map(|v| mesh.vertices()[v as usize]));
            if !touched[root as usize] {
                located(
                    samples,
                    RepairChange::Reoriented,
                    centroid(mesh, faces[face as usize]),
                )?;
            }
            flip(&mut faces[face as usize]);
            report.flipped_faces += 1;
            report.flipped_area_mm2 += a;
            touched[root as usize] = true;
        }
    }
    for face in 0..count {
        cancel.poll(Checkpoint::EdgeIncidence, face as usize)?;
        if usable[face as usize] && keep[face as usize] && classes.find(face).0 == face {
            report.reoriented_components += usize::from(touched[face as usize]);
            report.non_orientable_components += usize::from(classes.contradictory(face));
        }
    }
    Ok(classes)
}

/// One closed component's bounds, reference vertex and signed volume (six times, with bound).
struct Shell {
    root: u32,
    min: [f64; 3],
    max: [f64; 3],
    reference: [f64; 3],
    volume6: f64,
    bound: f64,
}

/// Whether shell `i` lies outside the bounds of every other shell: its reference vertex is
/// outside their bounding boxes.
fn outermost(shells: &[Shell], i: usize) -> bool {
    let p = shells[i].reference;
    shells.iter().enumerate().all(|(j, other)| {
        j == i || (0..3).any(|axis| p[axis] < other.min[axis] || p[axis] > other.max[axis])
    })
}

/// Flip outermost closed components that enclose negative volume.
#[allow(clippy::too_many_lines)] // Closedness, shells, the outermost test, nested shells, flips.
#[allow(clippy::too_many_arguments)] // The repair's shared state, as `orient` takes it, plus classes.
fn outward(
    mesh: &TriangleMesh,
    faces: &mut [[u32; 3]],
    usable: &[bool],
    keep: &[bool],
    mut classes: Classes,
    report: &mut RepairReport,
    samples: &mut Vec<RepairSample>,
    cancel: &mut Cancellation<'_>,
) -> Result<(), TopologyError> {
    // Closedness within each component, on the consistently oriented faces.
    let uses = sorted_uses(faces, usable, keep, cancel)?;
    let mut open = filled(faces.len(), false)?;
    let mut tally: Vec<(u32, i64)> = Vec::new();
    for (index, (start, end)) in groups(&uses).enumerate() {
        cancel.poll(Checkpoint::EdgeIncidence, index)?;
        let key = undirected(uses[start].0);
        tally.clear();
        reserve(&mut tally, end - start)?;
        for &(d, f) in &uses[start..end] {
            tally.push((classes.find(f).0, if d == key { 1 } else { -1 }));
        }
        tally.sort_unstable();
        let mut i = 0;
        while i < tally.len() {
            let mut j = i;
            let mut net = 0;
            while j < tally.len() && tally[j].0 == tally[i].0 {
                net += tally[j].1;
                j += 1;
            }
            if net != 0 {
                open[tally[i].0 as usize] = true;
            }
            i = j;
        }
    }
    drop(uses);
    // Shells: bounds and reference vertex first, then the volume relative to the reference.
    let count = u32::try_from(faces.len()).map_err(|_| TopologyError::SizeOverflow)?;
    let mut slot = filled(faces.len(), u32::MAX)?;
    let mut shells: Vec<Shell> = Vec::new();
    let eligible = |classes: &mut Classes, face: u32| {
        let (root, _) = classes.find(face);
        usable[face as usize]
            && keep[face as usize]
            && !open[root as usize]
            && !classes.contradictory(root)
    };
    for face in 0..count {
        cancel.poll(Checkpoint::EdgeIncidence, face as usize)?;
        if !eligible(&mut classes, face) {
            continue;
        }
        let root = classes.find(face).0;
        if slot[root as usize] == u32::MAX {
            slot[root as usize] =
                u32::try_from(shells.len()).map_err(|_| TopologyError::SizeOverflow)?;
            reserve(&mut shells, 1)?;
            shells.push(Shell {
                root,
                min: [f64::INFINITY; 3],
                max: [f64::NEG_INFINITY; 3],
                reference: [f64::INFINITY; 3],
                volume6: 0.,
                bound: 0.,
            });
        }
        let shell = &mut shells[slot[root as usize] as usize];
        for v in faces[face as usize] {
            let p = mesh.vertices()[v as usize].coordinates();
            for (axis, &value) in p.iter().enumerate() {
                shell.min[axis] = shell.min[axis].min(value);
                shell.max[axis] = shell.max[axis].max(value);
            }
            if p.partial_cmp(&shell.reference) == Some(std::cmp::Ordering::Less) {
                shell.reference = p;
            }
        }
    }
    let mut magnitude = filled(shells.len(), 0f64)?;
    for face in 0..count {
        cancel.poll(Checkpoint::EdgeIncidence, face as usize)?;
        if !eligible(&mut classes, face) {
            continue;
        }
        let index = slot[classes.find(face).0 as usize] as usize;
        let r = shells[index].reference;
        let [a, b, c] = faces[face as usize].map(|v| {
            let p = mesh.vertices()[v as usize].coordinates();
            [p[0] - r[0], p[1] - r[1], p[2] - r[2]]
        });
        shells[index].volume6 += a[0] * (b[1] * c[2] - b[2] * c[1])
            + a[1] * (b[2] * c[0] - b[0] * c[2])
            + a[2] * (b[0] * c[1] - b[1] * c[0]);
        // Absolute products bound every rounding in the determinant and the running sum.
        magnitude[index] += a[0].abs() * ((b[1] * c[2]).abs() + (b[2] * c[1]).abs())
            + a[1].abs() * ((b[2] * c[0]).abs() + (b[0] * c[2]).abs())
            + a[2].abs() * ((b[0] * c[1]).abs() + (b[1] * c[0]).abs());
    }
    // Forward error bound of the determinant terms and their summation (the total face count
    // bounds every shell's summation length).
    #[allow(clippy::cast_precision_loss)] // A generous bound; precision of the count is irrelevant.
    let length = faces.len() as f64;
    for (shell, &m) in shells.iter_mut().zip(&magnitude) {
        shell.bound = (length + 16.) * f64::EPSILON * m;
    }
    let mut outermost_shells = filled(shells.len(), false)?;
    for (i, slot) in outermost_shells.iter_mut().enumerate() {
        cancel.poll(Checkpoint::EdgeIncidence, i)?;
        *slot = outermost(&shells, i);
    }
    let mut flip_root = filled(faces.len(), false)?;
    for (i, shell) in shells.iter().enumerate() {
        cancel.poll(Checkpoint::EdgeIncidence, i)?;
        let p = shell.reference;
        let outermost = outermost_shells[i];
        let negative = shell.volume6 < -shell.bound;
        if shell.volume6.abs() <= shell.bound {
            if outermost {
                report.ambiguous_volume_components += 1;
                located(samples, RepairChange::AmbiguousVolume, p)?;
            }
        } else if negative && outermost {
            flip_root[shell.root as usize] = true;
            report.outward_flipped_components += 1;
            located(samples, RepairChange::TurnedOutward, p)?;
        } else if negative {
            report.nested_reversed_components += 1;
        }
    }
    // A nested shell turns with the outermost shells around it when all of them turn: an
    // inside-out model is inside out as a whole, and its cavities and inner bodies keep their
    // orientation relative to it (TD-092 r9).
    let around = |inner: &Shell, outer: &Shell| {
        (0..3).all(|axis| {
            inner.reference[axis] >= outer.min[axis] && inner.reference[axis] <= outer.max[axis]
        })
    };
    for (i, shell) in shells.iter().enumerate() {
        cancel.poll(Checkpoint::EdgeIncidence, i)?;
        if outermost_shells[i] {
            continue;
        }
        let mut outers = shells
            .iter()
            .zip(&outermost_shells)
            .filter(|&(outer, &o)| o && around(shell, outer))
            .peekable();
        if outers.peek().is_some() && outers.all(|(outer, _)| flip_root[outer.root as usize]) {
            if shell.volume6 < -shell.bound {
                report.nested_reversed_components -= 1;
            }
            flip_root[shell.root as usize] = true;
            report.outward_flipped_components += 1;
            located(samples, RepairChange::TurnedOutward, shell.reference)?;
        }
    }
    for face in 0..count {
        cancel.poll(Checkpoint::EdgeIncidence, face as usize)?;
        if eligible(&mut classes, face) && flip_root[classes.find(face).0 as usize] {
            let a = area(faces[face as usize].map(|v| mesh.vertices()[v as usize]));
            flip(&mut faces[face as usize]);
            report.flipped_faces += 1;
            report.flipped_area_mm2 += a;
        }
    }
    Ok(())
}
