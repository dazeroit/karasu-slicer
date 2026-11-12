//! Lean defect diagnosis and boundary rims (TD-054): every defect class counted exactly from one
//! sorted array of directed edge uses, without the full topology report's incidence structures.
//!
//! Degenerate faces (the `DegenerateTriangle` diagnostic) are excluded from every count. Edges are
//! keyed by source vertex indices; duplicate faces are compared through exact-coordinate vertex
//! identities, like the duplicate diagnostic.
//!
//! Beyond the counts (M6.4d r1, TD-092): degenerate faces split into needles and caps, rim loops
//! that lie on a line (cracks), pinch vertices, small components, and up to [`MAX_SAMPLES`]
//! positions per class, so an interface can show where each defect is.

use super::{util, TopologyError, TopologyLimits};
use crate::cancellation::{Cancellation, Checkpoint};
use crate::mesh::{diagnostics, TriangleMesh};

/// Exact defect counts of a mesh (see the defective-mesh specification, section 4).
#[derive(Clone, Debug, PartialEq)]
pub struct DefectReport {
    /// Faces contributing ordinary incidences.
    pub usable_triangles: usize,
    /// Degenerate faces, excluded from every other count.
    pub excluded_triangles: usize,
    /// Later faces with the same corner positions and the same cyclic orientation as an earlier one.
    pub same_orientation_duplicates: usize,
    /// Face pairs with the same corner positions and opposite orientation (they cancel exactly
    /// under the winding rule).
    pub opposite_duplicate_pairs: usize,
    /// Unique undirected usable edges.
    pub edges: usize,
    /// One-face edges.
    pub boundary_edges: usize,
    /// Two-face edges, regardless of winding.
    pub two_face_edges: usize,
    /// More-than-two-face edges.
    pub non_manifold_edges: usize,
    /// Non-manifold edges whose uses do not balance by direction (extra faces, fins).
    pub unbalanced_non_manifold_edges: usize,
    /// Same-direction two-face edges.
    pub winding_conflicts: usize,
    /// Boundary rims that close into loops (holes).
    pub rim_loops: usize,
    /// Boundary rims that end at non-manifold or inconsistently wound edges.
    pub rim_chains: usize,
    /// Boundary length of the longest rim (mm, source coordinates).
    pub longest_rim_mm: f64,
    /// Boundary edges of the rim with the most edges.
    pub largest_rim_edges: usize,
    /// Rims in ascending id order, at most [`MAX_RIM_DETAILS`].
    pub rims: Vec<RimDetail>,
    /// Face sets connected through two-face edges.
    pub components: usize,
    /// Components whose edges all balance by direction within the component.
    pub closed_components: usize,
    /// Components whose orientation constraints contradict each other (Möbius-like).
    pub non_orientable_components: usize,
    /// Degenerate faces with two corners at the same position (needles).
    pub needles: usize,
    /// Degenerate faces with three distinct corners on a line (caps), as exporters leave at
    /// T-junctions.
    pub caps: usize,
    /// Rim loops whose vertices all lie on one line: cracks of zero area (TD-091), counted among
    /// the rim loops too.
    pub cracks: usize,
    /// Vertices whose usable faces form more than one fan connected through edges at the vertex
    /// (pinch vertices).
    pub non_manifold_vertices: usize,
    /// Components with less than [`SMALL_COMPONENT_MM2`] of surface.
    pub small_components: usize,
    /// Surface of the smallest component (mm², source coordinates), 0 without components.
    pub smallest_component_mm2: f64,
    /// Where the defects are: at most [`MAX_SAMPLES`] per class, in source coordinates.
    pub samples: Vec<DefectSample>,
}

/// A class of defect, for [`DefectSample`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DefectClass {
    /// A boundary rim (its first vertex).
    Rim,
    /// A rim loop on a line (its first vertex).
    Crack,
    /// A non-manifold edge (its midpoint).
    NonManifoldEdge,
    /// A two-face edge with both uses in the same direction (its midpoint).
    WindingConflict,
    /// A needle face (its centroid).
    Needle,
    /// A cap face (its centroid).
    Cap,
    /// A same-orientation duplicate face (its centroid).
    Duplicate,
    /// A face and its reverse (their centroid).
    OppositePair,
    /// A pinch vertex.
    NonManifoldVertex,
    /// A component below [`SMALL_COMPONENT_MM2`] (its first face's centroid).
    SmallComponent,
    /// A non-orientable component (its first face's centroid).
    NonOrientable,
}

/// Where one defect is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DefectSample {
    /// What the defect is.
    pub class: DefectClass,
    /// Source coordinates, in mm.
    pub point: [f64; 3],
}

/// At most this many positions are kept per defect class.
pub const MAX_SAMPLES: usize = 16;

/// Components with less surface than this, in mm², are counted as small: a speck that prints as
/// nothing useful, as CHITUBOX's repair deletes (research CHI-MR-13). Diagnosis only.
pub const SMALL_COMPONENT_MM2: f64 = 1.0;

/// Keep `point` as a sample of `class` while the class has fewer than [`MAX_SAMPLES`].
fn sample(
    samples: &mut Vec<DefectSample>,
    class: DefectClass,
    point: [f64; 3],
) -> Result<(), TopologyError> {
    if samples.iter().filter(|s| s.class == class).count() < MAX_SAMPLES {
        util::push(samples, DefectSample { class, point })?;
    }
    Ok(())
}

/// The coordinates of vertex `v`.
fn coordinates(mesh: &TriangleMesh, v: u32) -> [f64; 3] {
    let p = mesh.vertices()[v as usize];
    [p.x, p.y, p.z]
}

/// The centroid of `face`.
fn centroid(mesh: &TriangleMesh, face: [u32; 3]) -> [f64; 3] {
    let [a, b, c] = face.map(|v| coordinates(mesh, v));
    std::array::from_fn(|k| (a[k] + b[k] + c[k]) / 3.0)
}

/// The midpoint of the edge `{a, b}`.
fn midpoint(mesh: &TriangleMesh, a: u32, b: u32) -> [f64; 3] {
    let (p, q) = (coordinates(mesh, a), coordinates(mesh, b));
    std::array::from_fn(|k| f64::midpoint(p[k], q[k]))
}

/// The cross product of `b - a` and `c - a`.
fn cross_at(a: [f64; 3], b: [f64; 3], c: [f64; 3]) -> [f64; 3] {
    let (ab, ac): ([f64; 3], [f64; 3]) = (
        std::array::from_fn(|k| b[k] - a[k]),
        std::array::from_fn(|k| c[k] - a[k]),
    );
    [
        ab[1] * ac[2] - ab[2] * ac[1],
        ab[2] * ac[0] - ab[0] * ac[2],
        ab[0] * ac[1] - ab[1] * ac[0],
    ]
}

/// The area of a triangle.
fn area(a: [f64; 3], b: [f64; 3], c: [f64; 3]) -> f64 {
    let n = cross_at(a, b, c);
    0.5 * (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt()
}

/// Whether `points` all lie on one line, in exact computed arithmetic: the cross product of every
/// offset from the first point with the first nonzero offset is zero. Points all at one position
/// count.
#[allow(clippy::float_cmp)] // Exact positions and exact zero products are the definition.
fn collinear(points: &[[f64; 3]]) -> bool {
    let Some(&origin) = points.first() else {
        return true;
    };
    let Some(&along) = points.iter().find(|&&p| p != origin) else {
        return true;
    };
    points
        .iter()
        .all(|&p| cross_at(origin, along, p) == [0.0; 3])
}

/// One boundary rim.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RimDetail {
    /// Source index of the first vertex of the rim's first boundary half-edge.
    pub first_vertex: u32,
    /// Boundary edges in the rim.
    pub edges: usize,
    /// Boundary length (mm, source coordinates).
    pub length_mm: f64,
    /// Whether the rim closes into a loop.
    pub closed: bool,
}

/// At most this many rims are detailed; the counts always cover all of them.
pub const MAX_RIM_DETAILS: usize = 16;

/// Boundary rims for section closure and hole filling: each boundary edge's rim and position
/// along it, and each rim's half-edges in order.
#[derive(Debug, Default)]
pub(crate) struct RimTable {
    /// `(undirected edge key, rim, position)`, sorted by key; boundary edges are unique.
    entries: Vec<(u64, u32, u32)>,
    /// Whether each rim is a loop.
    closed: Vec<bool>,
    /// Boundary half-edges `(from vertex, face)` of every rim in order, rims concatenated.
    order: Vec<(u32, u32)>,
    /// Start of each rim in `order`, plus a final end.
    starts: Vec<usize>,
}

impl RimTable {
    /// The rim and position of boundary edge `{a, b}`, or `None` for a non-boundary edge.
    pub(crate) fn lookup(&self, [a, b]: [u32; 2]) -> Option<(u32, u32)> {
        let key = undirected(u64::from(a) << 32 | u64::from(b));
        self.entries
            .binary_search_by_key(&key, |e| e.0)
            .ok()
            .map(|i| (self.entries[i].1, self.entries[i].2))
    }

    /// Whether `rim` is a loop.
    pub(crate) fn is_loop(&self, rim: u32) -> bool {
        self.closed.get(rim as usize).copied().unwrap_or(false)
    }

    /// Rims in id order: their boundary half-edges `(from vertex, face)` in rim order, and
    /// whether each is a loop. Half-edge `k` runs from `from[k]` to `from[k + 1]` (cyclically for
    /// a loop).
    pub(crate) fn rims(&self) -> impl Iterator<Item = (&[(u32, u32)], bool)> + '_ {
        self.closed
            .iter()
            .enumerate()
            .map(|(rim, &closed)| (&self.order[self.starts[rim]..self.starts[rim + 1]], closed))
    }
}

pub(in crate::mesh) fn undirected(directed: u64) -> u64 {
    let (from, to) = (directed >> 32, directed & 0xffff_ffff);
    from.min(to) << 32 | from.max(to)
}

const NONE: u32 = u32::MAX;

/// Union-find over faces with orientation parity relative to the parent.
pub(in crate::mesh) struct Classes {
    parent: Vec<u32>,
    parity: Vec<u8>,
    contradiction: Vec<bool>,
}

impl Classes {
    /// Every face in its own class.
    pub(in crate::mesh) fn new(faces: usize) -> Result<Self, TopologyError> {
        let count = u32::try_from(faces).map_err(|_| TopologyError::SizeOverflow)?;
        let mut parent = Vec::new();
        util::reserve(&mut parent, faces)?;
        parent.extend(0..count);
        Ok(Self {
            parent,
            parity: util::filled(faces, 0u8)?,
            contradiction: util::filled(faces, false)?,
        })
    }

    /// Whether the class rooted at `root` holds contradictory constraints (non-orientable).
    pub(in crate::mesh) fn contradictory(&self, root: u32) -> bool {
        self.contradiction[root as usize]
    }

    /// The class root of `face` (its smallest face index) and the face's parity relative to it.
    pub(in crate::mesh) fn find(&mut self, mut face: u32) -> (u32, u8) {
        let mut parity = 0;
        let mut path = face;
        while self.parent[face as usize] != face {
            parity ^= self.parity[face as usize];
            face = self.parent[face as usize];
        }
        // Path compression with accumulated parities.
        let root = face;
        let mut accumulated = parity;
        while self.parent[path as usize] != path {
            let next = self.parent[path as usize];
            let step = self.parity[path as usize];
            self.parent[path as usize] = root;
            self.parity[path as usize] = accumulated;
            accumulated ^= step;
            path = next;
        }
        (root, parity)
    }

    /// Require `a` and `b` to have relative orientation `relation` (0 same, 1 flipped).
    pub(in crate::mesh) fn join(&mut self, a: u32, b: u32, relation: u8) {
        let (ra, pa) = self.find(a);
        let (rb, pb) = self.find(b);
        if ra == rb {
            if pa ^ pb != relation {
                self.contradiction[ra as usize] = true;
            }
            return;
        }
        let (small, large) = if ra < rb { (ra, rb) } else { (rb, ra) };
        self.parent[large as usize] = small;
        self.parity[large as usize] = pa ^ pb ^ relation;
        let merged = self.contradiction[large as usize];
        self.contradiction[small as usize] |= merged;
    }
}

/// The face's vertex following `v` in its cyclic order.
fn after(face: [u32; 3], v: u32) -> u32 {
    let k = face.iter().position(|&x| x == v).unwrap_or(0);
    face[(k + 1) % 3]
}

/// Diagnose `mesh` and build its rim table.
pub(in crate::mesh) fn analyze(
    mesh: &TriangleMesh,
    limits: TopologyLimits,
    cancel: &mut Cancellation<'_>,
) -> Result<(DefectReport, RimTable), TopologyError> {
    cancel.check(Checkpoint::TopologyStart)?;
    if mesh.vertices().len() > limits.max_vertices {
        return Err(TopologyError::LimitExceeded {
            resource: super::TopologyResource::Vertices,
            limit: limits.max_vertices,
        });
    }
    if mesh.triangles().len() > limits.max_triangles {
        return Err(TopologyError::LimitExceeded {
            resource: super::TopologyResource::Triangles,
            limit: limits.max_triangles,
        });
    }
    let triangles = mesh.triangles();
    let faces = u32::try_from(triangles.len()).map_err(|_| TopologyError::SizeOverflow)?;
    // Directed uses `(from << 32 | to, face)` of usable faces.
    let mut uses: Vec<(u64, u32)> = Vec::new();
    util::reserve(
        &mut uses,
        triangles
            .len()
            .checked_mul(3)
            .ok_or(TopologyError::SizeOverflow)?,
    )?;
    cancel.check(Checkpoint::EdgeIncidence)?;
    let usable = diagnostics::usable_faces(mesh)?;
    let mut excluded_triangles = 0;
    for (face, &[a, b, c]) in (0..faces).zip(triangles) {
        cancel.poll(Checkpoint::EdgeIncidence, face as usize)?;
        if !usable[face as usize] {
            excluded_triangles += 1;
            continue;
        }
        for (from, to) in [(a, b), (b, c), (c, a)] {
            uses.push((u64::from(from) << 32 | u64::from(to), face));
        }
    }
    cancel.check(Checkpoint::EdgeSorting)?;
    // Usable faces have three distinct corners, so `(undirected, directed, face)` is unique.
    crate::parallel::sort_by_key(&mut uses, |&(d, f)| (undirected(d), d, f));
    cancel.check(Checkpoint::EdgeSorting)?;

    let mut report = DefectReport {
        usable_triangles: triangles.len() - excluded_triangles,
        excluded_triangles,
        same_orientation_duplicates: 0,
        opposite_duplicate_pairs: 0,
        edges: 0,
        boundary_edges: 0,
        two_face_edges: 0,
        non_manifold_edges: 0,
        unbalanced_non_manifold_edges: 0,
        winding_conflicts: 0,
        rim_loops: 0,
        rim_chains: 0,
        longest_rim_mm: 0.,
        largest_rim_edges: 0,
        rims: Vec::new(),
        components: 0,
        closed_components: 0,
        non_orientable_components: 0,
        needles: 0,
        caps: 0,
        cracks: 0,
        non_manifold_vertices: 0,
        small_components: 0,
        smallest_component_mm2: 0.,
        samples: Vec::new(),
    };
    degenerate(mesh, &usable, &mut report, cancel)?;
    let mut classes = Classes::new(triangles.len())?;
    let boundary = classify_edges(mesh, &uses, &mut report, &mut classes, cancel)?;
    count_components(mesh, &uses, &usable, &mut report, &mut classes, cancel)?;
    duplicates(mesh, &usable, &mut report, cancel)?;
    pinch_vertices(mesh, &usable, &mut report, cancel)?;
    drop(usable);
    let table = rims(mesh, &uses, &boundary, &mut report, cancel)?;
    cancel.check(Checkpoint::TopologyComplete)?;
    Ok((report, table))
}

/// Split the degenerate faces into needles (two corners at one position) and caps.
#[allow(clippy::float_cmp)] // A needle has two corners at exactly one position.
fn degenerate(
    mesh: &TriangleMesh,
    usable: &[bool],
    report: &mut DefectReport,
    cancel: &mut Cancellation<'_>,
) -> Result<(), TopologyError> {
    for (face, &corners) in mesh.triangles().iter().enumerate() {
        cancel.poll(Checkpoint::EdgeIncidence, face)?;
        if usable[face] {
            continue;
        }
        let [a, b, c] = corners.map(|v| coordinates(mesh, v));
        let class = if a == b || b == c || a == c {
            report.needles += 1;
            DefectClass::Needle
        } else {
            report.caps += 1;
            DefectClass::Cap
        };
        sample(&mut report.samples, class, centroid(mesh, corners))?;
    }
    Ok(())
}

/// The root of `x` in a small union-find forest, halving paths.
fn root(parent: &mut [u32], mut x: u32) -> u32 {
    while parent[x as usize] != x {
        let up = parent[parent[x as usize] as usize];
        parent[x as usize] = up;
        x = up;
    }
    x
}

/// Whether the `faces` around vertex `v` form more than one fan connected through the edges at
/// `v` (faces sharing another vertex share such an edge).
fn pinched(triangles: &[[u32; 3]], faces: &[u32], v: u32) -> bool {
    let count = faces.len();
    if count < 2 {
        return false;
    }
    let mut stack = [0u32; 64];
    let mut heap = Vec::new();
    let parent: &mut [u32] = if count <= stack.len() {
        &mut stack[..count]
    } else {
        heap.resize(count, 0);
        &mut heap
    };
    for (slot, x) in parent.iter_mut().zip(0u32..) {
        *slot = x;
    }
    let others = |face: u32| {
        let t = triangles[face as usize];
        let k = t.iter().position(|&w| w == v).unwrap_or(0);
        [t[(k + 1) % 3], t[(k + 2) % 3]]
    };
    for (i, &first) in faces.iter().enumerate() {
        let near = others(first);
        for (j, &second) in faces.iter().enumerate().skip(i + 1) {
            let far = others(second);
            if near.iter().any(|w| far.contains(w)) {
                let (x, y) = (
                    root(parent, u32::try_from(i).unwrap_or(0)),
                    root(parent, u32::try_from(j).unwrap_or(0)),
                );
                if x != y {
                    parent[x.max(y) as usize] = x.min(y);
                }
            }
        }
    }
    (0u32..)
        .take(count)
        .filter(|&x| root(parent, x) == x)
        .nth(1)
        .is_some()
}

/// Count the pinch vertices: a vertex whose usable faces do not form one fan connected through
/// the edges at the vertex.
fn pinch_vertices(
    mesh: &TriangleMesh,
    usable: &[bool],
    report: &mut DefectReport,
    cancel: &mut Cancellation<'_>,
) -> Result<(), TopologyError> {
    let triangles = mesh.triangles();
    let vertices = mesh.vertices().len();
    // The usable faces around each vertex, gathered by counting (no sort).
    let mut offsets = util::filled(vertices + 1, 0usize)?;
    for (face, t) in triangles.iter().enumerate() {
        cancel.poll(Checkpoint::EdgeIncidence, face)?;
        if usable[face] {
            for &v in t {
                offsets[v as usize + 1] += 1;
            }
        }
    }
    for v in 0..vertices {
        offsets[v + 1] += offsets[v];
    }
    let mut around = util::filled(offsets[vertices], 0u32)?;
    let mut next = offsets.clone();
    for (face, t) in triangles.iter().enumerate() {
        cancel.poll(Checkpoint::EdgeIncidence, face)?;
        if usable[face] {
            let face = u32::try_from(face).map_err(|_| TopologyError::SizeOverflow)?;
            for &v in t {
                around[next[v as usize]] = face;
                next[v as usize] += 1;
            }
        }
    }
    drop(next);
    cancel.check(Checkpoint::EdgeIncidence)?;
    let mut pinch = util::filled(vertices, false)?;
    crate::parallel::fill_with(&mut pinch, |v| {
        let faces = &around[offsets[v]..offsets[v + 1]];
        pinched(triangles, faces, u32::try_from(v).unwrap_or(u32::MAX))
    });
    cancel.check(Checkpoint::EdgeIncidence)?;
    for (v, _) in pinch.iter().enumerate().filter(|(_, &p)| p) {
        report.non_manifold_vertices += 1;
        let v = u32::try_from(v).map_err(|_| TopologyError::SizeOverflow)?;
        sample(
            &mut report.samples,
            DefectClass::NonManifoldVertex,
            coordinates(mesh, v),
        )?;
    }
    Ok(())
}

/// Classify every undirected edge, collect boundary half-edges and add orientation constraints.
fn classify_edges(
    mesh: &TriangleMesh,
    uses: &[(u64, u32)],
    report: &mut DefectReport,
    classes: &mut Classes,
    cancel: &mut Cancellation<'_>,
) -> Result<Vec<(u64, u32)>, TopologyError> {
    let mut boundary: Vec<(u64, u32)> = Vec::new();
    let mut start = 0;
    while start < uses.len() {
        cancel.poll(Checkpoint::EdgeIncidence, start)?;
        let key = undirected(uses[start].0);
        let mut end = start + 1;
        while end < uses.len() && undirected(uses[end].0) == key {
            end += 1;
        }
        report.edges += 1;
        match end - start {
            1 => {
                report.boundary_edges += 1;
                util::push(&mut boundary, uses[start])?;
            }
            2 => {
                report.two_face_edges += 1;
                let same = uses[start].0 == uses[start + 1].0;
                report.winding_conflicts += usize::from(same);
                if same {
                    let at = midpoint(mesh, (key >> 32) as u32, (key & 0xffff_ffff) as u32);
                    sample(&mut report.samples, DefectClass::WindingConflict, at)?;
                }
                classes.join(uses[start].1, uses[start + 1].1, u8::from(same));
            }
            _ => {
                report.non_manifold_edges += 1;
                let at = midpoint(mesh, (key >> 32) as u32, (key & 0xffff_ffff) as u32);
                sample(&mut report.samples, DefectClass::NonManifoldEdge, at)?;
                let forward = uses[start..end].iter().filter(|u| u.0 == key).count();
                if 2 * forward != end - start {
                    report.unbalanced_non_manifold_edges += 1;
                }
            }
        }
        start = end;
    }
    Ok(boundary)
}

/// Count components, closed components (every edge balanced within the component) and
/// non-orientable ones.
fn count_components(
    mesh: &TriangleMesh,
    uses: &[(u64, u32)],
    usable: &[bool],
    report: &mut DefectReport,
    classes: &mut Classes,
    cancel: &mut Cancellation<'_>,
) -> Result<(), TopologyError> {
    let mut open = util::filled(usable.len(), false)?;
    let mut start = 0;
    let mut tally: Vec<(u32, i64)> = Vec::new();
    while start < uses.len() {
        cancel.poll(Checkpoint::EdgeIncidence, start)?;
        let key = undirected(uses[start].0);
        let mut end = start + 1;
        while end < uses.len() && undirected(uses[end].0) == key {
            end += 1;
        }
        tally.clear();
        for &(d, f) in &uses[start..end] {
            let root = classes.find(f).0;
            util::push(&mut tally, (root, if d == key { 1 } else { -1 }))?;
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
        start = end;
    }
    // Each component's surface, gathered at its root.
    let faces = u32::try_from(usable.len()).map_err(|_| TopologyError::SizeOverflow)?;
    let mut surface = util::filled(usable.len(), 0f64)?;
    for face in 0..faces {
        cancel.poll(Checkpoint::EdgeIncidence, face as usize)?;
        if usable[face as usize] {
            let [a, b, c] = mesh.triangles()[face as usize].map(|v| coordinates(mesh, v));
            let root = classes.find(face).0;
            surface[root as usize] += area(a, b, c);
        }
    }
    let mut smallest = f64::INFINITY;
    for face in 0..faces {
        cancel.poll(Checkpoint::EdgeIncidence, face as usize)?;
        // After the pass above every path is compressed: a root is its own parent.
        if usable[face as usize] && classes.parent[face as usize] == face {
            report.components += 1;
            report.closed_components += usize::from(!open[face as usize]);
            let at = centroid(mesh, mesh.triangles()[face as usize]);
            if classes.contradiction[face as usize] {
                report.non_orientable_components += 1;
                sample(&mut report.samples, DefectClass::NonOrientable, at)?;
            }
            smallest = smallest.min(surface[face as usize]);
            if surface[face as usize] < SMALL_COMPONENT_MM2 {
                report.small_components += 1;
                sample(&mut report.samples, DefectClass::SmallComponent, at)?;
            }
        }
    }
    if smallest.is_finite() {
        report.smallest_component_mm2 = smallest;
    }
    Ok(())
}

/// Count same-orientation duplicates and opposite pairs among usable faces.
fn duplicates(
    mesh: &TriangleMesh,
    usable: &[bool],
    report: &mut DefectReport,
    cancel: &mut Cancellation<'_>,
) -> Result<(), TopologyError> {
    let canonical = diagnostics::canonical_vertices(mesh, cancel)?;
    let mut keys: Vec<([u32; 3], bool, u32)> = Vec::new();
    util::reserve(&mut keys, mesh.triangles().len())?;
    for (face, indices) in mesh.triangles().iter().enumerate() {
        cancel.poll(Checkpoint::EdgeIncidence, face)?;
        if !usable[face] {
            continue;
        }
        let ids = indices.map(|index| canonical[index as usize]);
        // Rotate the smallest identity first; the orientation is whether the rest ascend.
        let k = (0..3).min_by_key(|&k| ids[k]).unwrap_or(0);
        let rotated = [ids[k], ids[(k + 1) % 3], ids[(k + 2) % 3]];
        let mut sorted = ids;
        sorted.sort_unstable();
        let index = u32::try_from(face).map_err(|_| TopologyError::SizeOverflow)?;
        keys.push((sorted, rotated[1] < rotated[2], index));
    }
    drop(canonical);
    cancel.check(Checkpoint::EdgeSorting)?;
    // Keys are whole elements: equal keys are equal elements.
    crate::parallel::sort_by_key(&mut keys, |&key| key);
    let mut i = 0;
    while i < keys.len() {
        cancel.poll(Checkpoint::EdgeIncidence, i)?;
        let mut j = i;
        let (mut ascending, mut descending) = (0usize, 0usize);
        while j < keys.len() && keys[j].0 == keys[i].0 {
            if keys[j].1 {
                ascending += 1;
            } else {
                descending += 1;
            }
            j += 1;
        }
        let same = ascending.saturating_sub(1) + descending.saturating_sub(1);
        let opposite = ascending.min(descending);
        report.same_orientation_duplicates += same;
        report.opposite_duplicate_pairs += opposite;
        let at = || centroid(mesh, mesh.triangles()[keys[i].2 as usize]);
        if same > 0 {
            sample(&mut report.samples, DefectClass::Duplicate, at())?;
        }
        if opposite > 0 {
            sample(&mut report.samples, DefectClass::OppositePair, at())?;
        }
        i = j;
    }
    Ok(())
}

/// Chain boundary half-edges into rims and record each edge's rim and position.
#[allow(clippy::too_many_lines)] // One traversal: successors, chains, then loops.
fn rims(
    mesh: &TriangleMesh,
    uses: &[(u64, u32)],
    boundary: &[(u64, u32)],
    report: &mut DefectReport,
    cancel: &mut Cancellation<'_>,
) -> Result<RimTable, TopologyError> {
    let triangles = mesh.triangles();
    // Boundary half-edges sorted by directed key (from-major) for outgoing lookups.
    let mut by_from: Vec<u32> = Vec::new();
    util::reserve(&mut by_from, boundary.len())?;
    by_from.extend((0..boundary.len()).map(|i| u32::try_from(i).unwrap_or(NONE)));
    by_from.sort_unstable_by_key(|&i| boundary[i as usize].0);
    let outgoing = |v: u32| {
        let lo = by_from.partition_point(|&i| boundary[i as usize].0 >> 32 < u64::from(v));
        let hi = by_from.partition_point(|&i| boundary[i as usize].0 >> 32 <= u64::from(v));
        &by_from[lo..hi]
    };
    // The single use of a directed edge `from -> to`, if exactly one exists and its edge is
    // two-face and consistently wound.
    let manifold_use = |from: u32, to: u32| {
        let directed = u64::from(from) << 32 | u64::from(to);
        let key = undirected(directed);
        let lo = uses.partition_point(|u| undirected(u.0) < key);
        let hi = uses.partition_point(|u| undirected(u.0) <= key);
        let group = &uses[lo..hi];
        (group.len() == 2 && group[0].0 != group[1].0)
            .then(|| group.iter().find(|u| u.0 == directed).map(|u| u.1))
            .flatten()
    };
    let position_of = |directed: u64| {
        by_from
            .binary_search_by_key(&directed, |&i| boundary[i as usize].0)
            .ok()
            .map(|p| by_from[p])
    };
    // Successor of each boundary half-edge `u -> v`.
    let mut successor = util::filled(boundary.len(), NONE)?;
    let mut has_predecessor = util::filled(boundary.len(), false)?;
    for (h, &(directed, face)) in boundary.iter().enumerate() {
        cancel.poll(Checkpoint::EdgeIncidence, h)?;
        let v = (directed & 0xffff_ffff) as u32;
        let candidates = outgoing(v);
        let next = if candidates.len() == 1 {
            Some(candidates[0])
        } else if candidates.is_empty() {
            None
        } else {
            // Walk the fan around `v` from the face of `u -> v` across manifold edges.
            let mut current = face;
            let mut found = None;
            for _ in 0..uses.len() {
                let w = after(triangles[current as usize], v);
                if let Some(index) = position_of(u64::from(v) << 32 | u64::from(w)) {
                    found = Some(index);
                    break;
                }
                match manifold_use(w, v) {
                    Some(neighbour) => current = neighbour,
                    None => break,
                }
            }
            found
        };
        if let Some(next) = next {
            if !has_predecessor[next as usize] {
                has_predecessor[next as usize] = true;
                successor[h] = next;
            }
        }
    }
    // Trace chains from half-edges without a predecessor, then the remaining loops.
    let mut rim_of = util::filled(boundary.len(), NONE)?;
    let mut position = util::filled(boundary.len(), 0u32)?;
    let mut order = Vec::new();
    util::reserve(&mut order, boundary.len())?;
    let mut starts = vec![0usize];
    let mut closed = Vec::new();
    let mut lengths = Vec::new();
    let mut counts = Vec::new();
    let length = |directed: u64| {
        let a = mesh.vertices()[(directed >> 32) as usize];
        let b = mesh.vertices()[(directed & 0xffff_ffff) as usize];
        ((b.x - a.x).powi(2) + (b.y - a.y).powi(2) + (b.z - a.z).powi(2)).sqrt()
    };
    for pass in 0..2 {
        for start in 0..boundary.len() {
            cancel.poll(Checkpoint::EdgeIncidence, start)?;
            if rim_of[start] != NONE || (pass == 0 && has_predecessor[start]) {
                continue;
            }
            let rim = u32::try_from(closed.len()).map_err(|_| TopologyError::SizeOverflow)?;
            let (mut h, mut index, mut total) = (start, 0u32, 0.);
            loop {
                rim_of[h] = rim;
                position[h] = index;
                order.push(((boundary[h].0 >> 32) as u32, boundary[h].1));
                index += 1;
                total += length(boundary[h].0);
                let next = successor[h];
                if next == NONE || rim_of[next as usize] != NONE {
                    break;
                }
                h = next as usize;
            }
            util::push(&mut closed, successor[h] as usize == start)?;
            util::push(&mut starts, order.len())?;
            util::push(&mut lengths, (boundary[start].0 >> 32, total))?;
            util::push(&mut counts, index as usize)?;
        }
    }
    for (rim, &is_loop) in closed.iter().enumerate() {
        cancel.poll(Checkpoint::EdgeIncidence, rim)?;
        let vertices = &order[starts[rim]..starts[rim + 1]];
        let first = coordinates(mesh, vertices[0].0);
        if is_loop {
            report.rim_loops += 1;
            let points: Vec<[f64; 3]> = vertices
                .iter()
                .map(|&(v, _)| coordinates(mesh, v))
                .collect();
            if collinear(&points) {
                report.cracks += 1;
                sample(&mut report.samples, DefectClass::Crack, first)?;
            }
        } else {
            report.rim_chains += 1;
        }
        sample(&mut report.samples, DefectClass::Rim, first)?;
        report.longest_rim_mm = report.longest_rim_mm.max(lengths[rim].1);
        report.largest_rim_edges = report.largest_rim_edges.max(counts[rim]);
        if report.rims.len() < MAX_RIM_DETAILS {
            util::push(
                &mut report.rims,
                RimDetail {
                    first_vertex: u32::try_from(lengths[rim].0)
                        .map_err(|_| TopologyError::SizeOverflow)?,
                    edges: counts[rim],
                    length_mm: lengths[rim].1,
                    closed: is_loop,
                },
            )?;
        }
    }
    let mut entries = Vec::new();
    util::reserve(&mut entries, boundary.len())?;
    for (h, &(directed, _)) in boundary.iter().enumerate() {
        entries.push((undirected(directed), rim_of[h], position[h]));
    }
    entries.sort_unstable();
    Ok(RimTable {
        entries,
        closed,
        order,
        starts,
    })
}
