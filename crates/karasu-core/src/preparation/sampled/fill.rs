//! Hole filling (TD-054 phase 3, specification section 5.4): every simple boundary loop up to a
//! size bound gets a triangulated cap that uses only the loop's own vertices.
//!
//! - **Triangulation:** minimum-weight triangulation of the loop polygon by dynamic programming
//!   (Liepa 2003), O(n³) time and O(n²) memory. The weight is lexicographic: first the largest fold
//!   between a cap triangle and its neighbours across its edges (one minus the cosine of the angle
//!   between unit normals; neighbours are the mesh faces at the rim and the other cap triangles),
//!   then the total area. Ties keep the smallest split vertex. No vertex is added, refined or moved.
//! - **Orientation:** caps run opposite to the rim's boundary half-edges, so every former boundary
//!   edge becomes a consistently oriented two-face edge.
//! - **Validation:** a hole is refused (and stays open) when its rim is a chain or not simple,
//!   exceeds the size bound, needs a degenerate cap triangle, would duplicate an existing mesh
//!   edge, or when a cap triangle meets the mesh or another cap anywhere other than the vertices
//!   and edges they share (exact contact over a BVH).
//! - **Cracks:** a rim whose every cap triangle has zero computed area lies on a line: it is the
//!   rim of zero-area faces, which topology leaves out, as exporters leave at T-junctions. It has
//!   no area to cap, so it is closed by splitting each face along it at the rim vertices that lie
//!   on its rim edge, which makes every refined rim segment meet its reverse. The split faces
//!   cover the region of the face they replace, no vertex is added or moved, and the zero-area
//!   faces inside the rim are removed. Where a split triangle coincides with an existing face of
//!   opposite orientation, a zero-volume fold, both are removed. Around the crack, every edge must
//!   then be used equally in both directions (M6.4d r2: a crack in a knot of touching faces closes
//!   into a balanced non-manifold edge); a crack that does not close this way is refused as
//!   degenerate, as before. With a crack tolerance (TD-092, stage R4), a rim within it of a line
//!   is zipped the same way, splitting at the rim vertices within it of each rim edge; the
//!   largest such distance is reported.
//!
//! The source mesh is never modified; the result is a derived mesh plus a report.

mod refine;

use super::{bvh::Bvh, contact::improper_contact, distance::Triangle};
use crate::{
    cancellation::{Cancellation, Cancelled, Checkpoint},
    mesh::{
        repair::{record, RepairChange, RepairSample},
        topology::{TopologyError, TopologyLimits},
        MeshError, TriangleMesh,
    },
};
use std::{error::Error, fmt};

/// Hole filling parameters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FillHoles {
    /// Largest rim (boundary edges) that is filled; larger holes are refused. `usize::MAX`
    /// fills holes of any size.
    pub max_hole_edges: usize,
    /// Split a rim that visits a vertex twice (at a pinch vertex) into simple loops, each capped
    /// on its own (TD-092 r5); otherwise such a hole is refused as not simple.
    pub split_rims: bool,
    /// Refine and fair caps with new vertices (TD-092 r5): cap triangles are subdivided until
    /// their size matches the edges around the hole, and the new vertices are placed on a smooth
    /// membrane spanning the rim. Holes beyond [`MAX_EXACT_CAP_EDGES`] need it.
    pub refine: bool,
}

/// Largest rim capped by the minimum-weight triangulation, whose cost grows with the cube of
/// the rim's edges; a larger rim is capped by a fan from its centroid, which only refinement
/// and fairing make well shaped.
pub const MAX_EXACT_CAP_EDGES: usize = 512;

/// The default bound of [`FillHoles::max_hole_edges`].
pub const DEFAULT_MAX_HOLE_EDGES: usize = 256;

impl Default for FillHoles {
    fn default() -> Self {
        Self {
            max_hole_edges: DEFAULT_MAX_HOLE_EDGES,
            split_rims: false,
            refine: false,
        }
    }
}

/// Why a hole was left open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HoleRefusal {
    /// The rim does not close (it ends at non-manifold or inconsistently wound edges).
    OpenChain,
    /// The rim has more boundary edges than the bound.
    TooLarge,
    /// The rim visits a vertex twice, or has fewer than three edges.
    NotSimple,
    /// Every triangulation needs a degenerate (zero-area) cap triangle.
    Degenerate,
    /// The cap would duplicate an existing mesh edge (a non-manifold result).
    ExistingEdge,
    /// The cap meets the mesh or another cap away from their shared vertices and edges.
    Intersects,
    /// Only cracks were to be closed (hollowing's tolerance of zero-area defects, TD-092), and
    /// this rim does not close as one.
    NotACrack,
}

/// One hole left open.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RefusedHole {
    /// Source index of the rim's first vertex.
    pub first_vertex: u32,
    /// Boundary edges of the rim.
    pub edges: usize,
    /// Why it was refused.
    pub reason: HoleRefusal,
}

/// At most this many refused holes are detailed; the count covers all of them.
pub const MAX_REFUSED_DETAILS: usize = 16;

/// What hole filling did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FillReport {
    /// Holes capped.
    pub holes_filled: usize,
    /// Cap triangles added.
    pub cap_triangles: usize,
    /// Total cap area (mm², source coordinates).
    pub cap_area_mm2: f64,
    /// Boundary edges of the largest filled hole.
    pub largest_filled_edges: usize,
    /// Area of the largest cap (mm², source coordinates).
    pub largest_cap_mm2: f64,
    /// Holes (rims) left open.
    pub holes_refused: usize,
    /// The first refused holes in rim order, at most [`MAX_REFUSED_DETAILS`].
    pub refused: Vec<RefusedHole>,
    /// Cracks closed: rims of zero-area faces, closed by splitting the faces along them.
    pub cracks_closed: usize,
    /// Faces split along cracks.
    pub faces_split: usize,
    /// Zero-area faces removed from inside closed cracks.
    pub slivers_removed: usize,
    /// Faces removed where a crack's split met them reversed (a zero-volume fold).
    pub folds_removed: usize,
    /// The largest distance of a vertex from the crack edge it split, in mm: 0 for exact cracks,
    /// at most the crack tolerance otherwise.
    pub largest_crack_offset_mm: f64,
    /// Vertices added by refined caps.
    pub vertices_added: usize,
    /// Rims through a vertex twice capped as their simple loops.
    pub rims_split: usize,
    /// Refined caps that met the mesh, capped without refinement instead.
    pub unrefined_caps: usize,
    /// Where holes were filled, cracks closed and holes refused: at most
    /// [`crate::mesh::repair::MAX_REPAIR_SAMPLES`] per kind, in source coordinates.
    pub samples: Vec<RepairSample>,
}

/// Why hole filling failed.
#[derive(Debug, PartialEq)]
#[non_exhaustive]
pub enum FillError {
    /// Cooperative cancellation.
    Cancelled(Cancelled),
    /// Rim extraction failed.
    Topology(TopologyError),
    /// The derived mesh could not be built.
    Mesh(MeshError),
    /// A bounded allocation failed.
    Resource(&'static str),
}

impl fmt::Display for FillError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled(e) => e.fmt(f),
            Self::Topology(e) => write!(f, "hole rims: {e}"),
            Self::Mesh(e) => write!(f, "filled mesh: {e}"),
            Self::Resource(what) => write!(f, "hole filling resource unavailable: {what}"),
        }
    }
}

impl Error for FillError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Cancelled(e) => Some(e),
            Self::Topology(e) => Some(e),
            Self::Mesh(e) => Some(e),
            Self::Resource(_) => None,
        }
    }
}

impl From<Cancelled> for FillError {
    fn from(e: Cancelled) -> Self {
        Self::Cancelled(e)
    }
}

fn reserve<T>(items: &mut Vec<T>, count: usize) -> Result<(), FillError> {
    items
        .try_reserve_exact(count)
        .map_err(|_| FillError::Resource("hole filling buffers"))
}

type Vec3 = [f64; 3];

fn sub(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(u: Vec3, v: Vec3) -> Vec3 {
    [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ]
}

fn length(v: Vec3) -> f64 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

/// Unit normal and area of a triangle, or `None` when its computed cross product is zero.
fn normal(a: Vec3, b: Vec3, c: Vec3) -> Option<(Vec3, f64)> {
    let product = cross(sub(b, a), sub(c, a));
    let norm = length(product);
    (norm > 0.0 && norm.is_finite()).then(|| {
        (
            [product[0] / norm, product[1] / norm, product[2] / norm],
            0.5 * norm,
        )
    })
}

/// Fold between two unit normals across a shared edge: 0 for a flat continuation, 2 for a full
/// reversal.
fn fold(n: Vec3, m: Vec3) -> f64 {
    1.0 - (n[0] * m[0] + n[1] * m[1] + n[2] * m[2])
}

fn undirected(a: u32, b: u32) -> u64 {
    u64::from(a.min(b)) << 32 | u64::from(a.max(b))
}

/// The minimum-weight cap of one polygon, as triangles of polygon positions, or a refusal.
///
/// `points` are the polygon's corners in cap order and `rim_normals[j]` the unit normal of the
/// mesh face across polygon edge `j -> j + 1` (cyclic), if that face has one.
#[allow(clippy::needless_range_loop)] // Dynamic programming over polygon index pairs.
fn triangulate(
    points: &[Vec3],
    rim_normals: &[Option<Vec3>],
    cancel: &mut Cancellation<'_>,
) -> Result<Result<Vec<[usize; 3]>, HoleRefusal>, FillError> {
    let n = points.len();
    let index = |i: usize, k: usize| i * n + k;
    // cost = (largest fold, area); apex = split vertex of sub-polygon i..k.
    let mut cost = Vec::new();
    reserve(&mut cost, n * n)?;
    cost.resize(n * n, (0.0f64, 0.0f64));
    let mut apex = Vec::new();
    reserve(&mut apex, n * n)?;
    apex.resize(n * n, usize::MAX);
    let tri = |i: usize, m: usize, k: usize| normal(points[i], points[m], points[k]);
    for span in 2..n {
        cancel.check(Checkpoint::PreparationRecords)?;
        for i in 0..n - span {
            let k = i + span;
            let mut best: Option<((f64, f64), usize)> = None;
            for m in i + 1..k {
                let Some((normal_imk, area)) = tri(i, m, k) else {
                    continue;
                };
                let left = cost[index(i, m)];
                let right = cost[index(m, k)];
                if (m > i + 1 && apex[index(i, m)] == usize::MAX)
                    || (k > m + 1 && apex[index(m, k)] == usize::MAX)
                {
                    continue;
                }
                // Neighbour across edge (i, m): the rim face or the sub-polygon's top triangle.
                let across = |a: usize, b: usize| {
                    if b == a + 1 {
                        rim_normals[a]
                    } else {
                        tri(a, apex[index(a, b)], b).map(|(normal, _)| normal)
                    }
                };
                let folds = [across(i, m), across(m, k)]
                    .into_iter()
                    .flatten()
                    .map(|neighbour| fold(normal_imk, neighbour))
                    .fold(0.0f64, f64::max);
                let mut weight = (left.0.max(right.0).max(folds), left.1 + right.1 + area);
                if i == 0 && k == n - 1 {
                    // The closing edge (n - 1 -> 0) borders the last rim face.
                    if let Some(neighbour) = rim_normals[n - 1] {
                        weight.0 = weight.0.max(fold(normal_imk, neighbour));
                    }
                }
                if best.is_none_or(|(w, _)| weight < w) {
                    best = Some((weight, m));
                }
            }
            if let Some((weight, m)) = best {
                cost[index(i, k)] = weight;
                apex[index(i, k)] = m;
            }
        }
    }
    if apex[index(0, n - 1)] == usize::MAX {
        return Ok(Err(HoleRefusal::Degenerate));
    }
    let mut triangles = Vec::new();
    reserve(&mut triangles, n - 2)?;
    let mut stack = vec![(0usize, n - 1)];
    while let Some((i, k)) = stack.pop() {
        if k < i + 2 {
            continue;
        }
        let m = apex[index(i, k)];
        triangles.push([i, m, k]);
        stack.push((i, m));
        stack.push((m, k));
    }
    triangles.sort_unstable();
    Ok(Ok(triangles))
}

/// How a crack is closed: faces replaced by the fans that split them (a fan may be empty when
/// all its triangles cancel), and the faces removed as one side of a cancelling pair.
struct Zip {
    rim: usize,
    splits: Vec<(u32, Vec<[u32; 3]>)>,
    cancelled: Vec<u32>,
    /// The largest distance of a split vertex from the rim edge it splits, in mm.
    offset: f64,
}

/// The faces with area around each vertex, for closing cracks.
struct Incidence {
    offsets: Vec<usize>,
    faces: Vec<u32>,
}

impl Incidence {
    fn build(mesh: &TriangleMesh, cancel: &mut Cancellation<'_>) -> Result<Self, FillError> {
        let vertex = |v: u32| mesh.vertices()[v as usize].coordinates();
        let usable = |t: &[u32; 3]| normal(vertex(t[0]), vertex(t[1]), vertex(t[2])).is_some();
        let count = mesh.vertices().len();
        let mut offsets = Vec::new();
        reserve(&mut offsets, count + 1)?;
        offsets.resize(count + 1, 0usize);
        for (i, t) in mesh.triangles().iter().enumerate() {
            cancel.poll(Checkpoint::PreparationRecords, i)?;
            if usable(t) {
                for &v in t {
                    offsets[v as usize + 1] += 1;
                }
            }
        }
        for v in 0..count {
            offsets[v + 1] += offsets[v];
        }
        let mut faces = Vec::new();
        reserve(&mut faces, offsets[count])?;
        faces.resize(offsets[count], 0u32);
        let mut next = offsets.clone();
        for (i, t) in mesh.triangles().iter().enumerate() {
            if usable(t) {
                let face = u32::try_from(i).map_err(|_| FillError::Resource("face index"))?;
                for &v in t {
                    faces[next[v as usize]] = face;
                    next[v as usize] += 1;
                }
            }
        }
        Ok(Self { offsets, faces })
    }

    fn around(&self, v: u32) -> &[u32] {
        &self.faces[self.offsets[v as usize]..self.offsets[v as usize + 1]]
    }
}

/// The same triangle, whatever its starting corner.
fn rotated(t: [u32; 3]) -> [u32; 3] {
    let k = (0..3).min_by_key(|&k| t[k]).unwrap_or(0);
    [t[k], t[(k + 1) % 3], t[(k + 2) % 3]]
}

/// Close a crack: the rim `half_edges` (each a rim vertex and the face whose edge runs from it to
/// the next rim vertex) when its vertices lie on one line, or within `tolerance` of it. Each face
/// along the rim is split at the rim vertices within `tolerance` of its rim edge and strictly
/// between its ends, in order. A split triangle that coincides with an existing face of opposite
/// orientation (a fold, of zero volume) cancels with it. The crack is closed when, around it,
/// every edge is then used equally in both directions; `None` otherwise. The returned rim index
/// is a placeholder for the caller.
fn zip_crack(
    mesh: &TriangleMesh,
    half_edges: &[(u32, u32)],
    incidence: &Incidence,
    tolerance: f64,
) -> Option<Zip> {
    let vertex = |v: u32| mesh.vertices()[v as usize].coordinates();
    let count = half_edges.len();
    let rim: Vec<u32> = half_edges.iter().map(|h| h.0).collect();
    let mut splits: Vec<(u32, Vec<[u32; 3]>)> = Vec::new();
    let mut largest = 0.0f64;
    for (h, &(u, face)) in half_edges.iter().enumerate() {
        let v = rim[(h + 1) % count];
        let (start, end) = (vertex(u), vertex(v));
        let direction = sub(end, start);
        let squared =
            direction[0] * direction[0] + direction[1] * direction[1] + direction[2] * direction[2];
        // Rim vertices on the segment (within the tolerance), strictly between its ends, by their
        // parameter along it, with their distance from its line.
        let reach = tolerance * squared.sqrt();
        let mut inside: Vec<(f64, u32, f64)> = rim
            .iter()
            .copied()
            .filter(|&x| x != u && x != v)
            .filter_map(|x| {
                let offset = sub(vertex(x), start);
                let t = (offset[0] * direction[0]
                    + offset[1] * direction[1]
                    + offset[2] * direction[2])
                    / squared;
                let off = length(cross(direction, offset));
                (off <= reach && t > 0.0 && t < 1.0).then(|| (t, x, off / squared.sqrt()))
            })
            .collect();
        if inside.is_empty() {
            continue;
        }
        largest = inside.iter().fold(largest, |m, p| m.max(p.2));
        inside.sort_by(|p, q| p.0.total_cmp(&q.0));
        // The face, turned to (u, v, w); a face split twice is left to the refusal.
        let triangle = mesh.triangles()[face as usize];
        let turn = (0..3).find(|&k| triangle[k] == u && triangle[(k + 1) % 3] == v)?;
        let w = triangle[(turn + 2) % 3];
        if splits.iter().any(|(f, _)| *f == face) {
            return None;
        }
        let path: Vec<u32> = std::iter::once(u)
            .chain(inside.iter().map(|&(_, x, _)| x))
            .chain(std::iter::once(v))
            .collect();
        splits.push((face, path.windows(2).map(|p| [p[0], p[1], w]).collect()));
    }
    if splits.is_empty() {
        return None;
    }
    // The faces around the crack as they would be: the faces with area at its vertices and at
    // the fans' apexes, the split faces replaced by their fans.
    let mut around: Vec<u32> = rim
        .iter()
        .copied()
        .chain(splits.iter().flat_map(|(_, fan)| fan.iter().map(|t| t[2])))
        .flat_map(|v| incidence.around(v).iter().copied())
        .collect();
    around.sort_unstable();
    around.dedup();
    let local: Vec<(Option<u32>, [u32; 3])> = around
        .iter()
        .filter(|f| !splits.iter().any(|(s, _)| s == *f))
        .map(|&f| (Some(f), mesh.triangles()[f as usize]))
        .chain(
            splits
                .iter()
                .flat_map(|(_, fan)| fan.iter().map(|&t| (None, t))),
        )
        .collect();
    let (cancelled_faces, cancelled_fan) = settle(local, &rim)?;
    for (_, fan) in &mut splits {
        fan.retain(|t| !cancelled_fan.contains(t));
    }
    Some(Zip {
        rim: 0,
        splits,
        cancelled: cancelled_faces,
        offset: largest,
    })
}

/// The existing faces and the fan triangles a crack's folds cancel.
type Folds = (Vec<u32>, Vec<[u32; 3]>);

/// Cancel the folds among the faces around a crack (`local`: existing faces by index, and new
/// fan triangles), then check that every edge at a `rim` vertex is used equally in both
/// directions. Returns what was cancelled, or `None` when the crack does not close.
///
/// Balance, not one face per side, is the test (M6.4d r2): splitting keeps the region of every
/// face and removes only zero-area ones, so the winding number is unchanged wherever it was
/// defined, and a balanced result defines it everywhere. A crack whose split edge coincides with
/// an existing edge (a knot of touching faces) then closes into a balanced non-manifold edge, as
/// where bodies touch.
fn settle(mut local: Vec<(Option<u32>, [u32; 3])>, rim: &[u32]) -> Option<Folds> {
    // Folds: a triangle and its reverse cancel.
    let mut cancelled_faces = Vec::new();
    let mut cancelled_fan: Vec<[u32; 3]> = Vec::new();
    let mut first = 0;
    while first < local.len() {
        let [p, q, r] = local[first].1;
        let reverse = rotated([p, r, q]);
        let Some(second) = (0..local.len()).find(|&k| k != first && rotated(local[k].1) == reverse)
        else {
            first += 1;
            continue;
        };
        for (face, triangle) in [local[first], local[second]] {
            match face {
                Some(f) => cancelled_faces.push(f),
                None => cancelled_fan.push(triangle),
            }
        }
        local.remove(first.max(second));
        local.remove(first.min(second));
        first = 0;
    }
    // Around the crack, every edge is used equally in both directions.
    let on_rim = |v: &u32| rim.contains(v);
    let mut directed: Vec<(u32, u32)> = local
        .iter()
        .flat_map(|&(_, [a, b, c])| [(a, b), (b, c), (c, a)])
        .filter(|(a, b)| on_rim(a) || on_rim(b))
        .collect();
    directed.sort_unstable();
    let uses = |edge: (u32, u32)| {
        directed.partition_point(|&d| d <= edge) - directed.partition_point(|&d| d < edge)
    };
    let balanced = directed.iter().all(|&(a, b)| uses((a, b)) == uses((b, a)));
    balanced.then_some((cancelled_faces, cancelled_fan))
}

/// One candidate cap: its rim, triangles and area. Vertex indices below the mesh's vertex count
/// are mesh vertices; the others are new vertices of [`Capper::extra`].
struct Cap {
    rim: usize,
    triangles: Vec<[u32; 3]>,
    area: f64,
    /// For a refined cap, the caps tried in order when it meets the mesh.
    alternatives: Vec<Alternative>,
}

/// Another cap of the same rim: the thin plate's of a cap kept flat at a crease, or the unrefined
/// triangulation of a refined cap.
struct Alternative {
    triangles: Vec<[u32; 3]>,
    area: f64,
    unrefined: bool,
}

/// Split a closed rim that visits a vertex twice into loops that each visit it once: at the
/// first repeated vertex, the half-edges between its two visits form one loop and the rest
/// another, recursively.
fn split_loops(half_edges: &[(u32, u32)]) -> Vec<Vec<(u32, u32)>> {
    let mut done = Vec::new();
    let mut pending = vec![half_edges.to_vec()];
    while let Some(rim) = pending.pop() {
        let repeat =
            (0..rim.len()).find_map(|j| (0..j).find(|&i| rim[i].0 == rim[j].0).map(|i| (i, j)));
        match repeat {
            Some((i, j)) => {
                let inner = rim[i..j].to_vec();
                let outer: Vec<(u32, u32)> = rim[j..].iter().chain(&rim[..i]).copied().collect();
                pending.push(outer);
                pending.push(inner);
            }
            None => done.push(rim),
        }
    }
    done
}

/// The alternatives of a rim capped as `parts`, one list per loop (see [`Capper::cap`]): none
/// when a loop has none.
fn combine(parts: &[Vec<Alternative>]) -> Result<Vec<Alternative>, FillError> {
    let count = parts.iter().map(Vec::len).max().unwrap_or(0);
    if parts.iter().any(Vec::is_empty) {
        return Ok(Vec::new());
    }
    let mut combined = Vec::new();
    for k in 0..count {
        let mut triangles = Vec::new();
        let mut area = 0.0;
        let mut unrefined = true;
        for part in parts {
            let alternative = &part[k.min(part.len() - 1)];
            reserve(&mut triangles, alternative.triangles.len())?;
            triangles.extend_from_slice(&alternative.triangles);
            area += alternative.area;
            unrefined &= alternative.unrefined;
        }
        reserve(&mut combined, 1)?;
        combined.push(Alternative {
            triangles,
            area,
            unrefined,
        });
    }
    Ok(combined)
}

/// One capped loop: its triangles and area, and for a refined cap its alternatives.
struct CapPart {
    triangles: Vec<[u32; 3]>,
    area: f64,
    alternatives: Vec<Alternative>,
}

/// Caps simple loops (see [`Capper::cap`]), keeping the new vertices of refined caps.
struct Capper<'a> {
    mesh: &'a TriangleMesh,
    /// The mesh's undirected edges, sorted.
    edges: &'a [u64],
    /// The mean length of the mesh edges at each vertex, computed the first time a cap is
    /// refined.
    scales: Option<Vec<f64>>,
    /// New vertices; vertex `mesh.vertices().len() + i` is `extra[i]`.
    extra: Vec<Vec3>,
    /// Each vertex's mesh neighbours (offsets and list), built the first time a cap is refined.
    adjacency: Option<(Vec<usize>, Vec<u32>)>,
}

impl Capper<'_> {
    /// The mesh neighbours of every vertex, from the sorted undirected edges.
    fn adjacency(&mut self) -> Result<(&[usize], &[u32]), FillError> {
        if self.adjacency.is_none() {
            let count = self.mesh.vertices().len();
            let mut offsets = Vec::new();
            reserve(&mut offsets, count + 1)?;
            offsets.resize(count + 1, 0usize);
            let ends = |edge: u64| {
                #[allow(clippy::cast_possible_truncation)] // Both halves are u32 vertex indices.
                ((edge >> 32) as u32, (edge & u64::from(u32::MAX)) as u32)
            };
            for &edge in self.edges {
                let (a, b) = ends(edge);
                offsets[a as usize + 1] += 1;
                offsets[b as usize + 1] += 1;
            }
            for v in 0..count {
                offsets[v + 1] += offsets[v];
            }
            let mut list = Vec::new();
            reserve(&mut list, offsets[count])?;
            list.resize(offsets[count], 0u32);
            let mut next = offsets.clone();
            for &edge in self.edges {
                let (a, b) = ends(edge);
                list[next[a as usize]] = b;
                next[a as usize] += 1;
                list[next[b as usize]] = a;
                next[b as usize] += 1;
            }
            self.adjacency = Some((offsets, list));
        }
        let (offsets, list) = self
            .adjacency
            .as_ref()
            .map_or((&[][..], &[][..]), |(o, l)| (o.as_slice(), l.as_slice()));
        Ok((offsets, list))
    }

    /// Each rim corner's mesh neighbours that are not rim corners.
    fn outside(&mut self, polygon: &[u32]) -> Result<Vec<Vec<Vec3>>, FillError> {
        let mesh = self.mesh;
        let mut corners = polygon.to_vec();
        corners.sort_unstable();
        let (offsets, list) = self.adjacency()?;
        Ok(polygon
            .iter()
            .map(|&v| {
                list[offsets[v as usize]..offsets[v as usize + 1]]
                    .iter()
                    .filter(|w| corners.binary_search(w).is_err())
                    .map(|&w| mesh.vertices()[w as usize].coordinates())
                    .collect()
            })
            .collect())
    }

    fn scales(&mut self) -> Result<&[f64], FillError> {
        if self.scales.is_none() {
            let count = self.mesh.vertices().len();
            let mut sums = Vec::new();
            reserve(&mut sums, count)?;
            sums.resize(count, (0.0f64, 0u32));
            for &edge in self.edges {
                #[allow(clippy::cast_possible_truncation)] // Both halves are u32 vertex indices.
                let (a, b) = ((edge >> 32) as usize, (edge & u64::from(u32::MAX)) as usize);
                let length = length(sub(
                    self.mesh.vertices()[a].coordinates(),
                    self.mesh.vertices()[b].coordinates(),
                ));
                for v in [a, b] {
                    sums[v].0 += length;
                    sums[v].1 += 1;
                }
            }
            self.scales = Some(
                sums.iter()
                    .map(|&(sum, n)| if n > 0 { sum / f64::from(n) } else { 0.0 })
                    .collect(),
            );
        }
        Ok(self.scales.as_deref().unwrap_or(&[]))
    }

    /// Cap the simple loop `half_edges`: the minimum-weight triangulation (`triangulated` when
    /// already computed), or a fan from the centroid for a loop too large for it when `fill`
    /// refines; then refined and faired when `fill` refines. Returns the triangles (with new
    /// vertices numbered after the mesh's) and the area, or why the loop is refused.
    fn cap(
        &mut self,
        half_edges: &[(u32, u32)],
        triangulated: Option<Result<Vec<[usize; 3]>, HoleRefusal>>,
        fill: FillHoles,
        cancel: &mut Cancellation<'_>,
    ) -> Result<Result<CapPart, HoleRefusal>, FillError> {
        let mesh = self.mesh;
        let vertex = |v: u32| mesh.vertices()[v as usize].coordinates();
        let n = half_edges.len();
        let polygon: Vec<u32> = (0..n).map(|j| half_edges[(n - j) % n].0).collect();
        let mut points: Vec<Vec3> = polygon.iter().map(|&v| vertex(v)).collect();
        let outcome = match triangulated {
            Some(outcome) => outcome,
            None if n <= MAX_EXACT_CAP_EDGES => {
                let rim_normals: Vec<Option<Vec3>> = (0..n)
                    .map(|j| {
                        let [a, b, c] =
                            mesh.triangles()[half_edges[n - 1 - j].1 as usize].map(vertex);
                        normal(a, b, c).map(|(normal, _)| normal)
                    })
                    .collect();
                triangulate(&points, &rim_normals, cancel)?
            }
            None if fill.refine => {
                // A fan from the centroid; refinement and fairing shape it.
                #[allow(clippy::cast_precision_loss)] // Rim sizes are far below 2^52.
                let centre =
                    std::array::from_fn(|k| points.iter().map(|p| p[k]).sum::<f64>() / n as f64);
                reserve(&mut points, 1)?;
                points.push(centre);
                Ok((0..n).map(|j| [j, (j + 1) % n, n]).collect())
            }
            None => Err(HoleRefusal::TooLarge),
        };
        let local = match outcome {
            Ok(local) => local,
            Err(reason) => return Ok(Err(reason)),
        };
        let edges = self.edges;
        let existing = |a: usize, b: usize| {
            edges
                .binary_search(&undirected(polygon[a], polygon[b]))
                .is_ok()
        };
        // Chords between non-adjacent rim corners must be new edges.
        let duplicates_edge = local.iter().any(|t| {
            (0..3).any(|k| {
                let (a, b) = (t[k].min(t[(k + 1) % 3]), t[k].max(t[(k + 1) % 3]));
                b < n && b != a + 1 && !(a == 0 && b == n - 1) && existing(a, b)
            })
        });
        if duplicates_edge {
            return Ok(Err(HoleRefusal::ExistingEdge));
        }
        // A refined cap keeps its minimum-weight triangulation (not a fan) to fall back on.
        let fallback = (fill.refine && points.len() == n).then(|| {
            let area = local
                .iter()
                .filter_map(|t| normal(points[t[0]], points[t[1]], points[t[2]]))
                .map(|(_, a)| a)
                .sum();
            (local.iter().map(|t| t.map(|v| polygon[v])).collect(), area)
        });
        let (positions, local, domed) = if fill.refine {
            let scales: Vec<f64> = {
                let all = self.scales()?;
                polygon.iter().map(|&v| all[v as usize]).collect()
            };
            let outside = self.outside(&polygon)?;
            let mut patch = refine::Patch::new(points, &scales, local, outside);
            let domed = patch.refine_and_fair(&existing, cancel)?;
            (patch.positions, patch.triangles, domed)
        } else {
            (points, local, None)
        };
        let (triangles, area) = self.place(mesh, &polygon, &positions, &local)?;
        let mut alternatives = Vec::new();
        if let Some(domed) = domed {
            let (triangles, area) = self.place(mesh, &polygon, &domed, &local)?;
            alternatives.push(Alternative {
                triangles,
                area,
                unrefined: false,
            });
        }
        alternatives.extend(fallback.map(|(triangles, area)| Alternative {
            triangles,
            area,
            unrefined: true,
        }));
        Ok(Ok(CapPart {
            triangles,
            area,
            alternatives,
        }))
    }

    /// The cap triangles over `positions` (the rim's `polygon` first, then new vertices, which
    /// are added to [`Capper::extra`]) and their area.
    fn place(
        &mut self,
        mesh: &TriangleMesh,
        polygon: &[u32],
        positions: &[Vec3],
        local: &[[usize; 3]],
    ) -> Result<(Vec<[u32; 3]>, f64), FillError> {
        let n = polygon.len();
        let base = u32::try_from(mesh.vertices().len() + self.extra.len())
            .map_err(|_| FillError::Resource("cap vertex index"))?;
        let global = |v: usize| -> Result<u32, FillError> {
            if v < n {
                Ok(polygon[v])
            } else {
                u32::try_from(v - n)
                    .ok()
                    .and_then(|i| base.checked_add(i))
                    .ok_or(FillError::Resource("cap vertex index"))
            }
        };
        let mut triangles = Vec::new();
        reserve(&mut triangles, local.len())?;
        let mut area = 0.0;
        for t in local {
            if let Some((_, a)) = normal(positions[t[0]], positions[t[1]], positions[t[2]]) {
                area += a;
            }
            triangles.push([global(t[0])?, global(t[1])?, global(t[2])?]);
        }
        reserve(&mut self.extra, positions.len() - n)?;
        self.extra.extend_from_slice(&positions[n..]);
        Ok((triangles, area))
    }
}

/// Whether every point lies within `tolerance` of the line through the first point and the point
/// farthest from it (all at one position counts).
fn near_line(points: &[Vec3], tolerance: f64) -> bool {
    let Some(&origin) = points.first() else {
        return true;
    };
    let far = points.iter().copied().fold(origin, |best, p| {
        if length(sub(p, origin)) > length(sub(best, origin)) {
            p
        } else {
            best
        }
    });
    let direction = sub(far, origin);
    let span = length(direction);
    span == 0.0
        || points
            .iter()
            .all(|&p| length(cross(direction, sub(p, origin))) <= tolerance * span)
}

/// Fill the holes of `mesh` (see the module documentation), or with `spec` `None` close only its
/// exact cracks, refusing every other rim. Returns the derived mesh, or `None` when no hole was
/// filled, and the report.
///
/// # Errors
/// Cancellation, rim extraction, allocation and mesh construction failures.
#[allow(clippy::too_many_lines)] // Rims, caps, then the intersection validation loop.
pub(in crate::preparation) fn fill_holes(
    mesh: &TriangleMesh,
    spec: Option<FillHoles>,
    crack_tolerance: f64,
    limits: TopologyLimits,
    cancel: &mut Cancellation<'_>,
) -> Result<(Option<TriangleMesh>, FillReport), FillError> {
    let (defects, rims) = mesh
        .rims_with_cancellation(limits, cancel)
        .map_err(FillError::Topology)?;
    let mut report = FillReport::default();
    if defects.boundary_edges == 0 {
        return Ok((None, report));
    }
    let vertex = |v: u32| mesh.vertices()[v as usize].coordinates();
    let face_normal = |f: u32| {
        let [a, b, c] = mesh.triangles()[f as usize].map(vertex);
        normal(a, b, c).map(|(n, _)| n)
    };
    // Existing undirected edges, for the non-manifold check of internal cap edges.
    let mut edges = Vec::new();
    reserve(&mut edges, mesh.triangles().len() * 3)?;
    for (i, &[a, b, c]) in mesh.triangles().iter().enumerate() {
        cancel.poll(Checkpoint::PreparationRecords, i)?;
        edges.extend([undirected(a, b), undirected(b, c), undirected(c, a)]);
    }
    edges.sort_unstable();
    edges.dedup();
    let mut refusals: Vec<(usize, u32, usize, HoleRefusal)> = Vec::new();
    let mut caps: Vec<Cap> = Vec::new();
    let mut capper = Capper {
        mesh,
        edges: &edges,
        scales: None,
        extra: Vec::new(),
        adjacency: None,
    };
    let mut zips: Vec<Zip> = Vec::new();
    // The faces with area around each vertex, built the first time a crack needs them.
    let mut incidence: Option<Incidence> = None;
    for (rim, (half_edges, closed)) in rims.rims().enumerate() {
        cancel.poll(Checkpoint::PreparationRecords, rim)?;
        let n = half_edges.len();
        let first = half_edges[0].0;
        let mut refuse = |reason| -> Result<(), FillError> {
            reserve(&mut refusals, 1)?;
            refusals.push((rim, first, n, reason));
            Ok(())
        };
        if !closed {
            refuse(HoleRefusal::OpenChain)?;
            continue;
        }
        if spec.is_some_and(|s| n > s.max_hole_edges) {
            refuse(HoleRefusal::TooLarge)?;
            continue;
        }
        let mut sorted: Vec<u32> = half_edges.iter().map(|h| h.0).collect();
        sorted.sort_unstable();
        if n < 3 || sorted.windows(2).any(|w| w[0] == w[1]) {
            // A rim through a vertex twice (a pinch vertex) is capped as its simple loops.
            let Some(fill) = spec.filter(|s| s.split_rims && n >= 3) else {
                refuse(HoleRefusal::NotSimple)?;
                continue;
            };
            let mut triangles = Vec::new();
            let mut area = 0.0;
            // Each loop's alternatives; the rim's k-th alternative takes each loop's k-th, or its
            // last when it has fewer, while every loop has one.
            let mut parts: Vec<Vec<Alternative>> = Vec::new();
            let mut failed = None;
            for part in split_loops(half_edges) {
                if part.len() < 3 {
                    failed = Some(HoleRefusal::NotSimple);
                    break;
                }
                match capper.cap(&part, None, fill, cancel)? {
                    Ok(loop_cap) => {
                        triangles.extend(loop_cap.triangles);
                        area += loop_cap.area;
                        reserve(&mut parts, 1)?;
                        parts.push(loop_cap.alternatives);
                    }
                    Err(reason) => {
                        failed = Some(reason);
                        break;
                    }
                }
            }
            if let Some(reason) = failed {
                refuse(reason)?;
            } else {
                report.rims_split += 1;
                reserve(&mut caps, 1)?;
                caps.push(Cap {
                    rim,
                    triangles,
                    area,
                    alternatives: combine(&parts)?,
                });
            }
            continue;
        }
        // Cap polygon: the rim reversed, q_j = v_(n - j) mod n. Polygon edge j -> j + 1 reverses
        // rim half-edge n - 1 - j, whose face is the neighbour across it.
        let polygon: Vec<u32> = (0..n).map(|j| half_edges[(n - j) % n].0).collect();
        let points: Vec<Vec3> = polygon.iter().map(|&v| vertex(v)).collect();
        let rim_normals: Vec<Option<Vec3>> = (0..n)
            .map(|j| face_normal(half_edges[n - 1 - j].1))
            .collect();
        // A rim within the crack tolerance of a line is zipped first; an exact crack is one whose
        // every cap triangle has zero area. A rim that does not zip is capped or refused as usual.
        // Rims too large for the minimum-weight triangulation are zipped when exactly on a line.
        let large = n > MAX_EXACT_CAP_EDGES;
        let near = spec.is_none() || (crack_tolerance > 0.0 && near_line(&points, crack_tolerance));
        let triangulated = if near || large {
            None
        } else {
            Some(triangulate(&points, &rim_normals, cancel)?)
        };
        let zip_first = near
            || matches!(triangulated, Some(Err(HoleRefusal::Degenerate)))
            || (large && near_line(&points, 0.0));
        if zip_first {
            if incidence.is_none() {
                incidence = Some(Incidence::build(mesh, cancel)?);
            }
            let zip = incidence
                .as_ref()
                .and_then(|incidence| zip_crack(mesh, half_edges, incidence, crack_tolerance));
            // Cracks are closed one at a time: one touching faces another changed is left.
            let touched = |z: &Zip| {
                z.splits
                    .iter()
                    .map(|(f, _)| *f)
                    .chain(z.cancelled.iter().copied())
                    .collect::<Vec<u32>>()
            };
            let taken: Vec<u32> = zips.iter().flat_map(touched).collect();
            if let Some(zip) = zip.filter(|z| !touched(z).iter().any(|f| taken.contains(f))) {
                reserve(&mut zips, 1)?;
                zips.push(Zip { rim, ..zip });
                continue;
            }
            if spec.is_none() {
                refuse(HoleRefusal::NotACrack)?;
                continue;
            }
        }
        let Some(fill) = spec else {
            continue;
        };
        match capper.cap(half_edges, triangulated, fill, cancel)? {
            Ok(rim_cap) => {
                reserve(&mut caps, 1)?;
                caps.push(Cap {
                    rim,
                    triangles: rim_cap.triangles,
                    area: rim_cap.area,
                    alternatives: rim_cap.alternatives,
                });
            }
            Err(reason) => refuse(reason)?,
        }
    }
    let extra = capper.extra;
    drop(edges);
    // Positions of mesh vertices and of the caps' new vertices.
    let base = mesh.vertices().len();
    let position = |v: u32| {
        let v = v as usize;
        if v < base {
            mesh.vertices()[v].coordinates()
        } else {
            extra[v - base]
        }
    };
    // Exact contact validation: refuse the hole of the first improper pair, until none remain.
    // Only pairs with a cap triangle count, and a mesh face can meet a cap triangle only where
    // their boxes overlap, so the index holds the caps and the mesh faces whose box overlaps a
    // cap's (TD-092 r9: a model with few caps no longer indexes all its faces).
    while !caps.is_empty() {
        cancel.check(Checkpoint::PreparationRecords)?;
        let boxes: Vec<(Vec3, Vec3)> = caps
            .iter()
            .map(|cap| {
                let mut low = [f64::INFINITY; 3];
                let mut high = [f64::NEG_INFINITY; 3];
                for p in cap.triangles.iter().flatten().map(|&v| position(v)) {
                    for k in 0..3 {
                        low[k] = low[k].min(p[k]);
                        high[k] = high[k].max(p[k]);
                    }
                }
                (low, high)
            })
            .collect();
        let overlaps = |a: &(Vec3, Vec3), b: &(Vec3, Vec3)| {
            (0..3).all(|k| a.0[k] <= b.1[k] && a.1[k] >= b.0[k])
        };
        let union = boxes.iter().fold(
            ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]),
            |(low, high), (l, h)| {
                (
                    std::array::from_fn(|k| low[k].min(l[k])),
                    std::array::from_fn(|k| high[k].max(h[k])),
                )
            },
        );
        let near = |face: &[u32; 3]| {
            let p = face.map(position);
            let face_box = (
                std::array::from_fn(|k| p[0][k].min(p[1][k]).min(p[2][k])),
                std::array::from_fn(|k| p[0][k].max(p[1][k]).max(p[2][k])),
            );
            overlaps(&face_box, &union) && boxes.iter().any(|b| overlaps(&face_box, b))
        };
        let mut owners = Vec::new();
        let mut faces = Vec::new();
        for (i, face) in mesh.triangles().iter().enumerate() {
            cancel.poll(Checkpoint::PreparationRecords, i)?;
            if near(face) {
                reserve(&mut faces, 1)?;
                faces.push(*face);
            }
        }
        let source_faces = faces.len();
        reserve(
            &mut faces,
            caps.iter().map(|c| c.triangles.len()).sum::<usize>(),
        )?;
        for (cap_index, cap) in caps.iter().enumerate() {
            for &t in &cap.triangles {
                faces.push(t);
                owners.push(cap_index);
            }
        }
        let mut triangles = Vec::new();
        reserve(&mut triangles, faces.len())?;
        triangles.extend(faces.iter().map(|f| Triangle::new(f.map(position))));
        let bvh =
            Bvh::build(triangles, cancel, Checkpoint::PreparationRecords).map_err(|e| match e {
                super::bvh::BuildFailure::Cancelled(c) => FillError::Cancelled(c),
                super::bvh::BuildFailure::Allocation => FillError::Resource("hole BVH"),
            })?;
        let corners = |i: usize| faces[i].map(position);
        let pair = bvh.find_pair(cancel, Checkpoint::PreparationRecords, |a, b| {
            let (a, b) = (a as usize, b as usize);
            (a >= source_faces || b >= source_faces)
                && improper_contact(faces[a], corners(a), faces[b], corners(b))
        })?;
        let Some((a, b)) = pair else {
            break;
        };
        // Of the caps in the pair (the higher index first), the first with an alternative is
        // replaced by it; with none, the cap of the higher index is refused. The rest are kept.
        let owner = |i: u32| (i as usize).checked_sub(source_faces).map(|i| owners[i]);
        let culprit = owners[(a.max(b) as usize) - source_faces];
        let replaced = [owner(a.max(b)), owner(a.min(b))]
            .into_iter()
            .flatten()
            .find(|&c| !caps[c].alternatives.is_empty());
        if let Some(culprit) = replaced {
            let next = caps[culprit].alternatives.remove(0);
            caps[culprit].triangles = next.triangles;
            caps[culprit].area = next.area;
            report.unrefined_caps += usize::from(next.unrefined);
            continue;
        }
        let cap = caps.remove(culprit);
        let (half_edges, _) = rims.rims().nth(cap.rim).unwrap_or((&[], false));
        reserve(&mut refusals, 1)?;
        refusals.push((
            cap.rim,
            half_edges.first().map_or(0, |h| h.0),
            half_edges.len(),
            HoleRefusal::Intersects,
        ));
    }
    refusals.sort_unstable_by_key(|r| r.0);
    let located = |samples: &mut Vec<RepairSample>, change, v: u32| {
        record(samples, change, vertex(v)).map_err(|_| FillError::Resource("repair samples"))
    };
    for &(_, first, _, _) in &refusals {
        located(&mut report.samples, RepairChange::HoleRefused, first)?;
    }
    report.holes_refused = refusals.len();
    report.refused = refusals
        .iter()
        .take(MAX_REFUSED_DETAILS)
        .map(|&(_, first_vertex, edges, reason)| RefusedHole {
            first_vertex,
            edges,
            reason,
        })
        .collect();
    if caps.is_empty() && zips.is_empty() {
        return Ok((None, report));
    }
    let source_faces = mesh.triangles().len();
    let mut faces = Vec::new();
    reserve(
        &mut faces,
        source_faces + caps.iter().map(|c| c.triangles.len()).sum::<usize>(),
    )?;
    faces.extend_from_slice(mesh.triangles());
    // Cracks: each split face is replaced by its fan, and the zero-area faces whose vertices are
    // all on a closed crack's rim are removed.
    let mut removed = vec![false; source_faces];
    for zip in &zips {
        let (half_edges, _) = rims.rims().nth(zip.rim).unwrap_or((&[], false));
        let mut rim: Vec<u32> = half_edges.iter().map(|h| h.0).collect();
        rim.sort_unstable();
        report.cracks_closed += 1;
        report.largest_crack_offset_mm = report.largest_crack_offset_mm.max(zip.offset);
        if let Some(&(first, _)) = half_edges.first() {
            located(&mut report.samples, RepairChange::CrackClosed, first)?;
        }
        for (face, fan) in &zip.splits {
            report.faces_split += 1;
            match fan.split_first() {
                Some((first, rest)) => {
                    faces[*face as usize] = *first;
                    faces.extend_from_slice(rest);
                }
                None => removed[*face as usize] = true,
            }
        }
        for &face in &zip.cancelled {
            if !removed[face as usize] {
                removed[face as usize] = true;
                report.folds_removed += 1;
            }
        }
        for (i, &[a, b, c]) in mesh.triangles().iter().enumerate() {
            cancel.poll(Checkpoint::PreparationRecords, i)?;
            let on_rim = [a, b, c].iter().all(|v| rim.binary_search(v).is_ok());
            if on_rim && !removed[i] && normal(vertex(a), vertex(b), vertex(c)).is_none() {
                removed[i] = true;
                report.slivers_removed += 1;
            }
        }
    }
    if removed.iter().any(|&r| r) {
        let mut index = 0;
        faces.retain(|_| {
            let keep = index >= source_faces || !removed[index];
            index += 1;
            keep
        });
    }
    // The surviving caps' new vertices, renumbered in order after the mesh's.
    let mut renumber = vec![u32::MAX; extra.len()];
    let mut vertices = Vec::new();
    reserve(&mut vertices, base + extra.len())?;
    vertices.extend_from_slice(mesh.vertices());
    for cap in &mut caps {
        for v in cap.triangles.iter_mut().flatten() {
            let index = *v as usize;
            if index >= base {
                if renumber[index - base] == u32::MAX {
                    renumber[index - base] = u32::try_from(vertices.len())
                        .map_err(|_| FillError::Resource("cap vertex index"))?;
                    let [x, y, z] = extra[index - base];
                    vertices.push(crate::geometry::Point3::new(x, y, z));
                    report.vertices_added += 1;
                }
                *v = renumber[index - base];
            }
        }
    }
    for cap in &caps {
        report.holes_filled += 1;
        report.cap_triangles += cap.triangles.len();
        report.cap_area_mm2 += cap.area;
        report.largest_cap_mm2 = report.largest_cap_mm2.max(cap.area);
        let (half_edges, _) = rims.rims().nth(cap.rim).unwrap_or((&[], false));
        report.largest_filled_edges = report.largest_filled_edges.max(half_edges.len());
        if let Some(&(first, _)) = half_edges.first() {
            located(&mut report.samples, RepairChange::HoleFilled, first)?;
        }
        faces.extend_from_slice(&cap.triangles);
    }
    let filled = TriangleMesh::try_new_with_cancellation(vertices, faces, cancel)
        .map_err(FillError::Mesh)?;
    Ok((Some(filled), report))
}
