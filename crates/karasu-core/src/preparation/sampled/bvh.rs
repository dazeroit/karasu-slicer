//! Bounding-volume hierarchy over placed source triangles for distance and vertical-ray queries.
//!
//! The tree is built once from immutable world triangles with a deterministic median split on
//! the longest centroid axis (ties broken by source index), so its shape depends only on input
//! order and coordinates. Boxes are tight binary64 extrema of the exact represented coordinates;
//! all box tests are inclusive, so pruning never excludes a triangle that could matter.

use super::{
    contact::{segment_meets_triangle, triangles_meet},
    distance::{
        disk_box_gap, disk_gap, interval_dot, point_triangle_distance_sq, projection_gap_exceeds,
        separation, triangle_normal, Triangle,
    },
};
use crate::{
    cancellation::{Cancellation, Cancelled, Checkpoint},
    geometry::Point3,
    mesh::TriangleMesh,
    preparation::hollow::numeric::{orient3d, I},
};
use std::{
    cmp::{Ordering, Reverse},
    collections::BinaryHeap,
    sync::atomic::{AtomicBool, Ordering as AtomicOrdering},
};

/// Triangles per leaf; small leaves keep nearest-distance pruning tight.
const LEAF_SIZE: usize = 4;

/// Ranges at least this large check cancellation before they are split.
const LARGE_RANGE: usize = 1 << 12;

/// Why a labeled parity query has no answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ParityFailure {
    /// The query point lies exactly on a counted stabbed triangle; no side exists.
    OnSurface,
    /// A fallible reservation failed.
    Allocation,
}

/// Why a hierarchy could not be built.
#[derive(Debug)]
pub(crate) enum BuildFailure {
    /// The caller's predicate requested cancellation.
    Cancelled(Cancelled),
    /// A fallible reservation failed.
    Allocation,
}

impl From<Cancelled> for BuildFailure {
    fn from(cancelled: Cancelled) -> Self {
        Self::Cancelled(cancelled)
    }
}

/// A triangle's centroid and its input index.
type Centroid = ([f64; 3], usize);

/// Triangles, labels and original indices in leaf-contiguous slot order.
type Slots = (Vec<Triangle>, Vec<u32>, Vec<u32>);

/// Reorder triangles and their labels into the slot order of `centroids`, reserving fallibly.
///
/// The triangles are permuted in place, cycle by cycle, so the largest buffer is never copied
/// (TD-094): one bit per triangle marks the slots already filled.
fn reorder(
    mut triangles: Vec<Triangle>,
    centroids: &[([f64; 3], usize)],
    source_labels: &[u32],
    cancel: &mut Cancellation<'_>,
    checkpoint: Checkpoint,
) -> Result<Slots, BuildFailure> {
    let mut labels = Vec::new();
    let mut ids = Vec::new();
    let mut filled: Vec<u64> = Vec::new();
    filled
        .try_reserve_exact(triangles.len().div_ceil(64))
        .map_err(|_| BuildFailure::Allocation)?;
    filled.resize(triangles.len().div_ceil(64), 0);
    labels
        .try_reserve_exact(triangles.len())
        .map_err(|_| BuildFailure::Allocation)?;
    ids.try_reserve_exact(triangles.len())
        .map_err(|_| BuildFailure::Allocation)?;
    for (position, &(_, index)) in centroids.iter().enumerate() {
        cancel.poll(checkpoint, position)?;
        labels.push(source_labels.get(index).copied().unwrap_or(0));
        ids.push(u32::try_from(index).unwrap_or(u32::MAX));
    }
    // Slot `p` takes the triangle at `centroids[p].1`: follow each cycle of that permutation
    // from its first slot, which is overwritten last with the saved triangle.
    let is_filled = |filled: &[u64], slot: usize| filled[slot / 64] >> (slot % 64) & 1 == 1;
    for start in 0..triangles.len() {
        if is_filled(&filled, start) {
            continue;
        }
        cancel.poll(checkpoint, start)?;
        let first = triangles[start];
        let mut slot = start;
        loop {
            filled[slot / 64] |= 1 << (slot % 64);
            let source = centroids[slot].1;
            if source == start {
                triangles[slot] = first;
                break;
            }
            triangles[slot] = triangles[source];
            slot = source;
        }
    }
    Ok((triangles, labels, ids))
}

/// Ranges at least this large hand half of their subtree to another thread.
const PARALLEL_RANGE: usize = 1 << 16;

/// What every thread building a hierarchy shares.
struct Shared<'a> {
    /// The input triangle at an index.
    triangle: &'a (dyn Fn(usize) -> Triangle + Sync),
    /// Set by the caller's thread when it is cancelled, so the others stop.
    halted: &'a AtomicBool,
}

/// Why a subtree was not built.
enum Stop {
    /// The caller's cancellation, seen on its own thread.
    Cancelled(Cancelled),
    /// Another thread was told to stop.
    Halted,
}

/// Whether a subtree may be built: the caller's thread checks its cancellation (and halts the
/// others when cancelled), the other threads whether they were halted.
fn keep_going(
    shared: &Shared<'_>,
    len: usize,
    slot: usize,
    cancel: &mut Option<(&mut Cancellation<'_>, Checkpoint)>,
) -> Result<(), Stop> {
    match cancel {
        Some((cancel, checkpoint)) => {
            let checked = if len >= LARGE_RANGE {
                cancel.check(*checkpoint)
            } else {
                cancel.poll(*checkpoint, slot)
            };
            checked.map_err(|cancelled| {
                shared.halted.store(true, AtomicOrdering::Relaxed);
                Stop::Cancelled(cancelled)
            })
        }
        None if shared.halted.load(AtomicOrdering::Relaxed) => Err(Stop::Halted),
        None => Ok(()),
    }
}

/// The bounding box of the triangles of `centroids`.
fn range_bounds(
    triangle: &(dyn Fn(usize) -> Triangle + Sync),
    centroids: &[([f64; 3], usize)],
) -> ([f64; 3], [f64; 3]) {
    let (mut min, mut max) = bounds(&triangle(centroids[0].1));
    for &(_, index) in &centroids[1..] {
        let (a, b) = bounds(&triangle(index));
        for axis in 0..3 {
            min[axis] = min[axis].min(a[axis]);
            max[axis] = max[axis].max(b[axis]);
        }
    }
    (min, max)
}

/// The axis along which the centroids spread most (the lower axis on ties).
fn split_axis(centroids: &[([f64; 3], usize)]) -> usize {
    let mut cmin = centroids[0].0;
    let mut cmax = cmin;
    for (c, _) in &centroids[1..] {
        for axis in 0..3 {
            cmin[axis] = cmin[axis].min(c[axis]);
            cmax[axis] = cmax[axis].max(c[axis]);
        }
    }
    (0..3)
        .max_by(|&a, &b| {
            (cmax[a] - cmin[a])
                .total_cmp(&(cmax[b] - cmin[b]))
                .then(b.cmp(&a))
        })
        .unwrap_or(0)
}

/// Build the subtree over `centroids`, the input slots `start..start + centroids.len()`, into
/// `nodes`, exactly its [`node_count`] slots, the first being `slot`.
///
/// The node at `slot` is followed by its left subtree, then its right one, whose slot is known
/// from the left one's size: the layout is the sequential pre-order whoever builds each part.
/// Returns the subtree's bounding box, which its parent joins with its sibling's.
/// While `spawn` is positive, large ranges hand their left half to a new thread. Only the
/// caller's thread holds `cancel`; it checks it and halts the others.
fn build_subtree(
    shared: &Shared<'_>,
    centroids: &mut [([f64; 3], usize)],
    start: usize,
    nodes: &mut [Node],
    slot: usize,
    spawn: u32,
    cancel: &mut Option<(&mut Cancellation<'_>, Checkpoint)>,
) -> Result<([f64; 3], [f64; 3]), Stop> {
    let len = centroids.len();
    keep_going(shared, len, slot, cancel)?;
    if len <= LEAF_SIZE {
        let (min, max) = range_bounds(shared.triangle, centroids);
        nodes[0] = Node {
            min,
            max,
            start_or_right: u32::try_from(start).unwrap_or(u32::MAX),
            count: u32::try_from(len).unwrap_or(u32::MAX),
        };
        return Ok((min, max));
    }
    let axis = split_axis(centroids);
    let mid = len / 2;
    centroids.select_nth_unstable_by(mid, |a, b| {
        a.0[axis].total_cmp(&b.0[axis]).then(a.1.cmp(&b.1))
    });
    let left_count = node_count(mid);
    let right_slot = slot + 1 + left_count;
    let (left, right) = centroids.split_at_mut(mid);
    let (node, children) = nodes.split_at_mut(1);
    let (left_nodes, right_nodes) = children.split_at_mut(left_count);
    let (low, high) = if spawn == 0 || len < PARALLEL_RANGE {
        let low = build_subtree(shared, left, start, left_nodes, slot + 1, 0, cancel)?;
        let high = build_subtree(
            shared,
            right,
            start + mid,
            right_nodes,
            right_slot,
            0,
            cancel,
        )?;
        (low, high)
    } else {
        std::thread::scope(|scope| {
            let worker = scope.spawn(|| {
                build_subtree(
                    shared,
                    left,
                    start,
                    left_nodes,
                    slot + 1,
                    spawn - 1,
                    &mut None,
                )
            });
            let high = build_subtree(
                shared,
                right,
                start + mid,
                right_nodes,
                right_slot,
                spawn - 1,
                cancel,
            );
            let low = worker
                .join()
                .unwrap_or_else(|panic| std::panic::resume_unwind(panic));
            // The caller's cancellation first: it explains a halted worker.
            match (low, high) {
                (Err(stop @ Stop::Cancelled(_)), _) | (_, Err(stop @ Stop::Cancelled(_))) => {
                    Err(stop)
                }
                (Err(Stop::Halted), _) | (_, Err(Stop::Halted)) => Err(Stop::Halted),
                (Ok(low), Ok(high)) => Ok((low, high)),
            }
        })?
    };
    // A node's box is its children's, joined: every triangle is read once, at its leaf.
    let min = std::array::from_fn(|axis| low.0[axis].min(high.0[axis]));
    let max = std::array::from_fn(|axis| low.1[axis].max(high.1[axis]));
    node[0] = Node {
        min,
        max,
        start_or_right: u32::try_from(right_slot).unwrap_or(u32::MAX),
        count: 0,
    };
    Ok((min, max))
}

/// The nodes of the hierarchy over `count` triangles (`triangle` gives each), in `nodes`
/// (reserved at [`node_count`]), and the centroids in slot order.
///
/// Subtrees fill disjoint, known slots, so they are built on every core (TD-094) into the same
/// nodes, in the same pre-order, as one thread builds them.
fn hierarchy(
    count: usize,
    triangle: &(dyn Fn(usize) -> Triangle + Sync),
    mut nodes: Vec<Node>,
    cancel: &mut Cancellation<'_>,
    checkpoint: Checkpoint,
) -> Result<(Vec<Node>, Vec<Centroid>), BuildFailure> {
    let mut centroids: Vec<([f64; 3], usize)> = Vec::new();
    centroids
        .try_reserve_exact(count)
        .map_err(|_| BuildFailure::Allocation)?;
    centroids.extend((0..count).map(|index| {
        let t = triangle(index);
        let c = std::array::from_fn(|axis| {
            (t.corners[0][axis] + t.corners[1][axis] + t.corners[2][axis]) / 3.0
        });
        (c, index)
    }));
    nodes.resize(
        nodes.capacity(),
        Node {
            min: [0.0; 3],
            max: [0.0; 3],
            start_or_right: 0,
            count: 0,
        },
    );
    let threads = std::thread::available_parallelism().map_or(1, usize::from);
    let halted = AtomicBool::new(false);
    let mut caller = Some((&mut *cancel, checkpoint));
    let built = build_subtree(
        &Shared {
            triangle,
            halted: &halted,
        },
        &mut centroids,
        0,
        &mut nodes,
        0,
        threads.next_power_of_two().trailing_zeros(),
        &mut caller,
    );
    match built {
        Ok(_) => {}
        Err(Stop::Cancelled(cancelled)) => return Err(BuildFailure::Cancelled(cancelled)),
        // Only the caller halts the others, and it reports its own cancellation.
        Err(Stop::Halted) => unreachable!("a worker halted without a cancellation"),
    }
    debug_assert_eq!(nodes.len(), nodes.capacity());
    Ok((nodes, centroids))
}

/// Exact number of nodes the median split builds over `triangles` triangles.
///
/// A range of more than [`LEAF_SIZE`] triangles splits into `n / 2` and `n - n / 2`, so the
/// ranges at one depth take at most two adjacent sizes; counting ranges per size and depth gives
/// the total in logarithmic time.
fn node_count(triangles: usize) -> usize {
    if triangles == 0 {
        return 0;
    }
    let mut total = 0usize;
    // At most two (size, multiplicity) pairs per depth.
    let mut level: Vec<(usize, usize)> = vec![(triangles, 1)];
    while !level.is_empty() {
        let mut next: Vec<(usize, usize)> = Vec::with_capacity(2);
        for (size, multiplicity) in level {
            total += multiplicity;
            if size > LEAF_SIZE {
                for half in [size / 2, size - size / 2] {
                    match next.iter_mut().find(|(s, _)| *s == half) {
                        Some((_, m)) => *m += multiplicity,
                        None => next.push((half, multiplicity)),
                    }
                }
            }
        }
        level = next;
    }
    total
}

#[derive(Clone, Copy, Debug)]
struct Node {
    min: [f64; 3],
    max: [f64; 3],
    /// Leaf: first triangle slot. Internal: index of the second child (first child is next).
    start_or_right: u32,
    /// Leaf triangle count; zero marks an internal node.
    count: u32,
}

/// Immutable acceleration structure; triangle slots are reordered copies of the source.
pub(crate) struct Bvh {
    nodes: Vec<Node>,
    triangles: Vec<Triangle>,
    /// Caller label (for example a surface component) of each reordered triangle slot.
    labels: Vec<u32>,
    /// Original input index of each reordered triangle slot.
    ids: Vec<u32>,
    /// Optional per-slot planes for fast clearance rejection (see [`Self::index_planes`]).
    planes: Vec<Plane>,
}

/// A triangle's computed normal `n`, the outward interval of its vertices' projections onto
/// `n`, and an upper bound on `|n|²`.
#[derive(Clone, Copy, Debug)]
pub(super) struct Plane {
    normal: [f64; 3],
    low: f64,
    high: f64,
    norm_sq: f64,
}

/// Exact relation between a query triangle and the indexed surface within a threshold.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Clearance {
    /// The query triangle shares at least one point with an indexed triangle (exact test).
    Meets,
    /// Disjoint, but not proven farther than the threshold from the indexed surface.
    Within,
    /// Disjoint, and every indexed triangle is proven farther than the threshold.
    Beyond,
}

/// Outcome of the exact vertical-ray parity query.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Parity {
    /// Even number of crossings above the point.
    Outside,
    /// Odd number of crossings above the point.
    Inside,
    /// The point lies exactly on a triangle that the ray stabs; no side exists.
    OnSurface,
}

fn bounds(triangle: &Triangle) -> ([f64; 3], [f64; 3]) {
    let mut min = triangle.corners[0];
    let mut max = triangle.corners[0];
    for corner in &triangle.corners[1..] {
        for axis in 0..3 {
            min[axis] = min[axis].min(corner[axis]);
            max[axis] = max[axis].max(corner[axis]);
        }
    }
    (min, max)
}

fn box_distance_sq(p: [f64; 3], min: [f64; 3], max: [f64; 3]) -> f64 {
    let mut sum = 0.0;
    for axis in 0..3 {
        let d = if p[axis] < min[axis] {
            min[axis] - p[axis]
        } else if p[axis] > max[axis] {
            p[axis] - max[axis]
        } else {
            0.0
        };
        sum += d * d;
    }
    sum
}

impl Bvh {
    /// Builds the hierarchy. Requires fewer than 2^31 triangles (checked by the caller's limits).
    ///
    /// # Errors
    /// [`BuildFailure`] on cancellation at `checkpoint` or a failed allocation.
    pub(crate) fn build(
        triangles: Vec<Triangle>,
        cancel: &mut Cancellation<'_>,
        checkpoint: Checkpoint,
    ) -> Result<Self, BuildFailure> {
        let mut labels = Vec::new();
        labels
            .try_reserve_exact(triangles.len())
            .map_err(|_| BuildFailure::Allocation)?;
        labels.resize(triangles.len(), 0);
        Self::build_labeled(triangles, &labels, cancel, checkpoint)
    }

    /// Peak bytes [`Self::build_labeled`] allocates per triangle beyond its input: centroids,
    /// labels, ids, nodes (fewer nodes than triangles) and the reordering's bit, rounded up to a
    /// byte. The triangles are reordered in place (TD-094).
    pub(super) const BUILD_BYTES_PER_TRIANGLE: usize = std::mem::size_of::<([f64; 3], usize)>()
        + 2 * std::mem::size_of::<u32>()
        + std::mem::size_of::<Node>()
        + 1;

    /// Builds the hierarchy with one caller label per triangle (same order as `triangles`).
    ///
    /// Every buffer is reserved fallibly at its exact final size ([`node_count`] for the nodes)
    /// before it is filled, so construction never grows a vector. Cancellation is checked before
    /// every split of a large range and polled otherwise.
    ///
    /// # Errors
    /// [`BuildFailure`] on cancellation at `checkpoint` or a failed allocation.
    pub(super) fn build_labeled(
        triangles: Vec<Triangle>,
        source_labels: &[u32],
        cancel: &mut Cancellation<'_>,
        checkpoint: Checkpoint,
    ) -> Result<Self, BuildFailure> {
        let mut nodes = Vec::new();
        nodes
            .try_reserve_exact(node_count(triangles.len()))
            .map_err(|_| BuildFailure::Allocation)?;
        if triangles.is_empty() {
            return Ok(Self {
                nodes,
                triangles,
                labels: Vec::new(),
                ids: Vec::new(),
                planes: Vec::new(),
            });
        }
        let (nodes, centroids) = hierarchy(
            triangles.len(),
            &|index| triangles[index],
            nodes,
            cancel,
            checkpoint,
        )?;
        let (triangles, labels, ids) =
            reorder(triangles, &centroids, source_labels, cancel, checkpoint)?;
        Ok(Self {
            nodes,
            triangles,
            labels,
            ids,
            planes: Vec::new(),
        })
    }

    /// [`Self::build_labeled`] over `faces` of `mesh`, without a copy of the triangles to
    /// reorder: they are written once, in slot order, on every core (TD-094). The hierarchy is
    /// the one [`Self::build_labeled`] builds from the same triangles. Peak bytes per face
    /// beyond the input: [`Self::BUILD_BYTES_PER_TRIANGLE`] and the indexed triangle.
    ///
    /// # Errors
    /// [`BuildFailure`] on cancellation at `checkpoint` or a failed allocation.
    pub(super) fn build_indexed(
        mesh: &TriangleMesh,
        faces: &[[u32; 3]],
        source_labels: &[u32],
        cancel: &mut Cancellation<'_>,
        checkpoint: Checkpoint,
    ) -> Result<Self, BuildFailure> {
        let vertices = mesh.vertices();
        let triangle =
            |index: usize| Triangle::new(faces[index].map(|v| vertices[v as usize].coordinates()));
        let mut nodes = Vec::new();
        nodes
            .try_reserve_exact(node_count(faces.len()))
            .map_err(|_| BuildFailure::Allocation)?;
        if faces.is_empty() {
            return Ok(Self {
                nodes,
                triangles: Vec::new(),
                labels: Vec::new(),
                ids: Vec::new(),
                planes: Vec::new(),
            });
        }
        let (nodes, centroids) = hierarchy(faces.len(), &triangle, nodes, cancel, checkpoint)?;
        let mut triangles = Vec::new();
        let mut labels = Vec::new();
        let mut ids = Vec::new();
        triangles
            .try_reserve_exact(faces.len())
            .map_err(|_| BuildFailure::Allocation)?;
        labels
            .try_reserve_exact(faces.len())
            .map_err(|_| BuildFailure::Allocation)?;
        ids.try_reserve_exact(faces.len())
            .map_err(|_| BuildFailure::Allocation)?;
        triangles.resize(faces.len(), Triangle::new([[0.0; 3]; 3]));
        labels.resize(faces.len(), 0);
        ids.resize(faces.len(), 0);
        cancel.check(checkpoint)?;
        crate::parallel::fill_with(&mut triangles, |slot| triangle(centroids[slot].1));
        crate::parallel::fill_with(&mut labels, |slot| {
            source_labels.get(centroids[slot].1).copied().unwrap_or(0)
        });
        crate::parallel::fill_with(&mut ids, |slot| {
            u32::try_from(centroids[slot].1).unwrap_or(u32::MAX)
        });
        cancel.check(checkpoint)?;
        Ok(Self {
            nodes,
            triangles,
            labels,
            ids,
            planes: Vec::new(),
        })
    }

    /// Index every triangle's plane, so that [`Self::clearance`] rejects most nearby triangles
    /// with three projections (cavity simplification, TD-066).
    ///
    /// # Errors
    /// [`BuildFailure::Allocation`] when the buffer cannot be reserved.
    pub(super) fn index_planes(&mut self) -> Result<(), BuildFailure> {
        let mut planes = Vec::new();
        planes
            .try_reserve_exact(self.triangles.len())
            .map_err(|_| BuildFailure::Allocation)?;
        for t in &self.triangles {
            let normal = triangle_normal(t);
            let (mut low, mut high) = (f64::INFINITY, f64::NEG_INFINITY);
            for &v in &t.corners {
                let p = interval_dot(normal, v);
                low = low.min(p.lo);
                high = high.max(p.hi);
            }
            planes.push(Plane {
                normal,
                low,
                high,
                norm_sq: interval_dot(normal, normal).hi,
            });
        }
        self.planes = planes;
        Ok(())
    }

    /// Retained bytes of every buffer at its capacity, for resource admission.
    pub(super) fn retained_bytes(&self) -> usize {
        self.nodes.capacity() * std::mem::size_of::<Node>()
            + self.triangles.capacity() * std::mem::size_of::<Triangle>()
            + (self.labels.capacity() + self.ids.capacity()) * std::mem::size_of::<u32>()
            + self.planes.capacity() * std::mem::size_of::<Plane>()
    }

    /// Squared distance to the nearest triangle and that triangle's slot.
    ///
    /// `hint` is a slot whose distance seeds the upper bound (any valid slot is correct; a nearby
    /// one makes pruning effective). Returns `(f64::INFINITY, 0)` for an empty tree.
    /// The indexed triangle in `slot` (as returned by [`Self::nearest`]).
    pub(super) fn triangle(&self, slot: usize) -> Option<&Triangle> {
        self.triangles.get(slot)
    }

    /// The original input index of every triangle within squared distance `r2` of `p`
    /// (inclusive), with its slot, in increasing input order: a sum over them adds the same
    /// terms in the same order as a scan of the input. Boxes are lower bounds of their
    /// triangles' distances, so no triangle within the distance is missed.
    pub(super) fn within(&self, p: [f64; 3], r2: f64) -> Vec<(u32, usize)> {
        let mut found = Vec::new();
        if self.nodes.is_empty() {
            return found;
        }
        let mut stack = vec![0usize];
        while let Some(index) = stack.pop() {
            let node = self.nodes[index];
            if box_distance_sq(p, node.min, node.max) > r2 {
                continue;
            }
            if node.count > 0 {
                let start = node.start_or_right as usize;
                for slot in start..start + node.count as usize {
                    if point_triangle_distance_sq(p, &self.triangles[slot]) <= r2 {
                        found.push((self.ids[slot], slot));
                    }
                }
            } else {
                stack.push(index + 1);
                stack.push(node.start_or_right as usize);
            }
        }
        found.sort_unstable();
        found
    }

    pub(super) fn nearest(&self, p: [f64; 3], hint: usize) -> (f64, usize) {
        let mut best = f64::INFINITY;
        let mut best_slot = 0;
        if let Some(t) = self.triangles.get(hint) {
            best = point_triangle_distance_sq(p, t);
            best_slot = hint;
        }
        if self.nodes.is_empty() {
            return (best, best_slot);
        }
        let mut stack: [usize; 128] = [0; 128];
        let mut depth = 1;
        while depth > 0 {
            depth -= 1;
            let node = self.nodes[stack[depth]];
            if box_distance_sq(p, node.min, node.max) >= best {
                continue;
            }
            if node.count > 0 {
                let start = node.start_or_right as usize;
                for slot in start..start + node.count as usize {
                    let d = point_triangle_distance_sq(p, &self.triangles[slot]);
                    // Exact ties break on the lower slot so the result is order independent.
                    #[allow(clippy::float_cmp)]
                    let tie = d == best && slot < best_slot;
                    if d < best || tie {
                        best = d;
                        best_slot = slot;
                    }
                }
                continue;
            }
            let left = stack[depth] + 1;
            let right = node.start_or_right as usize;
            let dl = box_distance_sq(p, self.nodes[left].min, self.nodes[left].max);
            let dr = box_distance_sq(p, self.nodes[right].min, self.nodes[right].max);
            // Visit the nearer child first: push it last.
            let (near, far, dfar) = if dl <= dr {
                (left, right, dr)
            } else {
                (right, left, dl)
            };
            if dfar < best && depth < stack.len() {
                stack[depth] = far;
                depth += 1;
            }
            if depth < stack.len() {
                stack[depth] = near;
                depth += 1;
            }
        }
        (best, best_slot)
    }

    /// Exact parity of the closed surface along the upward vertical ray from `p`.
    ///
    /// The query point is symbolically perturbed to `(x + e, y + e^2)` for an infinitesimal
    /// `e > 0` (simulation of simplicity), so each projected edge/vertex contact is assigned to
    /// exactly one side and every triangle is either stabbed or not. Triangles whose projection
    /// has zero area are never stabbed. For a closed edge-manifold surface the count of stabbed
    /// triangles strictly above `p` is odd exactly for points inside its even-odd interior.
    pub(super) fn vertical_parity(&self, p: [f64; 3]) -> Parity {
        if self.nodes.is_empty() {
            return Parity::Outside;
        }
        let query = Point3::new(p[0], p[1], p[2]);
        let mut odd = false;
        let mut stack = vec![0usize];
        while let Some(index) = stack.pop() {
            let node = self.nodes[index];
            if p[0] < node.min[0]
                || p[0] > node.max[0]
                || p[1] < node.min[1]
                || p[1] > node.max[1]
                || p[2] > node.max[2]
            {
                continue;
            }
            if node.count == 0 {
                stack.push(node.start_or_right as usize);
                stack.push(index + 1);
                continue;
            }
            let start = node.start_or_right as usize;
            for t in &self.triangles[start..start + node.count as usize] {
                match stab_above(t, query) {
                    Some(Ordering::Greater) => odd = !odd,
                    Some(Ordering::Equal) => return Parity::OnSurface,
                    _ => {}
                }
            }
        }
        if odd {
            Parity::Inside
        } else {
            Parity::Outside
        }
    }
}

impl Bvh {
    /// Exact winding number of the surface at `p`: the stabbed triangles strictly above `p` along
    /// the upward vertical ray, each counted +1 when its normal points up (counterclockwise
    /// projection) and -1 when it points down. With the same symbolic perturbation as
    /// [`Self::vertical_parity`], so its parity agrees with that query.
    ///
    /// For a balanced surface (every edge used equally in both directions) the value does not
    /// depend on the ray, and it is the number of outward shells enclosing `p` minus the number of
    /// reversed ones. `None` when `p` lies on a stabbed triangle.
    pub(super) fn vertical_winding(&self, p: [f64; 3]) -> Option<i64> {
        let query = Point3::new(p[0], p[1], p[2]);
        let mut winding = 0i64;
        let mut stack = if self.nodes.is_empty() {
            Vec::new()
        } else {
            vec![0usize]
        };
        while let Some(index) = stack.pop() {
            let node = self.nodes[index];
            if p[0] < node.min[0]
                || p[0] > node.max[0]
                || p[1] < node.min[1]
                || p[1] > node.max[1]
                || p[2] > node.max[2]
            {
                continue;
            }
            if node.count == 0 {
                stack.push(node.start_or_right as usize);
                stack.push(index + 1);
                continue;
            }
            let start = node.start_or_right as usize;
            for t in &self.triangles[start..start + node.count as usize] {
                match stab_above(t, query) {
                    Some(Ordering::Greater) => {
                        let [a, b, c] = t.corners;
                        winding += if orient2d(a, b, c) == Ordering::Greater {
                            1
                        } else {
                            -1
                        };
                    }
                    Some(Ordering::Equal) => return None,
                    _ => {}
                }
            }
        }
        Some(winding)
    }

    /// Labels whose triangles the upward vertical ray from `p` stabs an odd number of times,
    /// sorted and distinct.
    ///
    /// Same symbolic perturbation as [`Bvh::vertical_parity`]. Triangles labelled `skip` are
    /// ignored, and so are subtrees whose `mask` entry is false (see [`Bvh::node_mask`]); the
    /// parity of every other label is exact. For disjoint closed components, a point is in the
    /// unbounded complement exactly when no label is odd. The list grows only with the number of
    /// stabbed triangles, never with the number of labels, and every reservation is fallible.
    ///
    /// # Errors
    /// [`ParityFailure::OnSurface`] when `p` lies exactly on a counted stabbed triangle, and
    /// [`ParityFailure::Allocation`] when a reservation fails.
    pub(super) fn vertical_odd_labels(
        &self,
        p: [f64; 3],
        skip: Option<u32>,
        mask: Option<&[bool]>,
    ) -> Result<Vec<u32>, ParityFailure> {
        let mut crossings: Vec<u32> = Vec::new();
        if self.nodes.is_empty() {
            return Ok(crossings);
        }
        let query = Point3::new(p[0], p[1], p[2]);
        let mut stack = vec![0usize];
        while let Some(index) = stack.pop() {
            let node = self.nodes[index];
            if mask.is_some_and(|m| !m.get(index).copied().unwrap_or(true))
                || p[0] < node.min[0]
                || p[0] > node.max[0]
                || p[1] < node.min[1]
                || p[1] > node.max[1]
                || p[2] > node.max[2]
            {
                continue;
            }
            if node.count == 0 {
                stack.push(node.start_or_right as usize);
                stack.push(index + 1);
                continue;
            }
            let start = node.start_or_right as usize;
            for slot in start..start + node.count as usize {
                // A point on its own component asks only about the other components.
                if Some(self.labels[slot]) == skip {
                    continue;
                }
                match stab_above(&self.triangles[slot], query) {
                    Some(Ordering::Greater) => {
                        if crossings.len() == crossings.capacity() {
                            crossings
                                .try_reserve(crossings.len().max(8))
                                .map_err(|_| ParityFailure::Allocation)?;
                        }
                        crossings.push(self.labels[slot]);
                    }
                    Some(Ordering::Equal) => return Err(ParityFailure::OnSurface),
                    _ => {}
                }
            }
        }
        // Keep each label that occurs an odd number of times, once.
        crossings.sort_unstable();
        let mut odd = 0;
        let mut run = 0;
        while run < crossings.len() {
            let label = crossings[run];
            let next = crossings[run..].partition_point(|&l| l == label) + run;
            if (next - run) % 2 == 1 {
                crossings[odd] = label;
                odd += 1;
            }
            run = next;
        }
        crossings.truncate(odd);
        Ok(crossings)
    }

    /// Per-node flags (indexed like the nodes): whether the subtree holds a triangle whose label
    /// satisfies `keep`. Children follow their parent in node order, so one reverse pass fills
    /// them. `None` when the reservation fails.
    pub(super) fn node_mask(&self, keep: impl Fn(u32) -> bool) -> Option<Vec<bool>> {
        let mut mask = Vec::new();
        mask.try_reserve_exact(self.nodes.len()).ok()?;
        mask.resize(self.nodes.len(), false);
        for index in (0..self.nodes.len()).rev() {
            let node = self.nodes[index];
            let start = node.start_or_right as usize;
            mask[index] = if node.count == 0 {
                mask[index + 1] || mask[start]
            } else {
                self.labels[start..start + node.count as usize]
                    .iter()
                    .any(|&label| keep(label))
            };
        }
        Some(mask)
    }

    /// Exact contact between `query` and the indexed triangles, and whether all of them are
    /// proven farther than `threshold`.
    ///
    /// Intersection is decided exactly. Branch and bound uses computed (upper-bound) distances;
    /// the decision uses the minimum of rigorous interval lower bounds over the examined
    /// triangles. Every pruned triangle's box is farther than an examined triangle's computed
    /// distance, which is at least that triangle's lower bound, so the minimum remains a valid
    /// lower bound for the whole surface. Boxes are compared inclusively with a relative shrink.
    ///
    /// # Errors
    /// Returns [`Cancelled`] when the caller's predicate requests it (polled at `checkpoint`).
    pub(super) fn clearance(
        &self,
        query: &Triangle,
        threshold: f64,
        cancel: &mut Cancellation<'_>,
        checkpoint: Checkpoint,
    ) -> Result<Clearance, Cancelled> {
        let (qmin, qmax) = bounds(query);
        let mut nearest = f64::INFINITY;
        let mut certified = f64::INFINITY;
        if self.nodes.is_empty() {
            return Ok(Clearance::Beyond);
        }
        let query_normal = triangle_normal(query);
        let mut stack = vec![0usize];
        let mut work = 0usize;
        while let Some(index) = stack.pop() {
            cancel.poll(checkpoint, work)?;
            work += 1;
            let node = self.nodes[index];
            // Branch and bound: once a nearer triangle is known, farther boxes cannot improve it.
            if box_box_distance(qmin, qmax, node.min, node.max) > threshold.min(nearest) {
                continue;
            }
            if node.count == 0 {
                stack.push(node.start_or_right as usize);
                stack.push(index + 1);
                continue;
            }
            let start = node.start_or_right as usize;
            for (slot, t) in (start..).zip(&self.triangles[start..start + node.count as usize]) {
                let (tmin, tmax) = bounds(t);
                if box_box_distance(qmin, qmax, tmin, tmax) > threshold.min(nearest) {
                    continue;
                }
                if self
                    .planes
                    .get(slot)
                    .is_some_and(|plane| plane_gap_exceeds(query, plane, threshold))
                {
                    continue;
                }
                // A rigorous projection gap beyond the threshold (along either normal) proves the
                // pair apart and beyond it, without the exact contact test or closest points.
                if projection_gap_exceeds(&query.corners, &t.corners, triangle_normal(t), threshold)
                    || projection_gap_exceeds(&query.corners, &t.corners, query_normal, threshold)
                {
                    continue;
                }
                if triangles_meet(query.corners, t.corners) {
                    return Ok(Clearance::Meets);
                }
                // Every examined triangle contributes its rigorous bound: a computed distance may
                // overestimate, so improving candidates alone would not bound the true minimum.
                let (distance_sq, bound) = separation(query, t);
                certified = certified.min(bound);
                nearest = nearest.min(distance_sq.sqrt());
            }
        }
        // Triangles pruned against `nearest` are at least that far, examined ones at least their
        // bound, and the rest beyond the threshold. Only the rigorous bound decides `Beyond`:
        // `nearest` may overestimate (nearly parallel edges), which must not hide a closer face.
        Ok(if certified.min(nearest) <= threshold {
            Clearance::Within
        } else {
            Clearance::Beyond
        })
    }
}

impl Bvh {
    /// Whether the closed segment `p q` meets an indexed triangle (exact).
    ///
    /// Node and triangle boxes are exact coordinate extrema, and the conservative slab filter
    /// ([`segment_may_touch_box`]) never skips a box the segment touches, so only the exact test
    /// decides.
    pub(super) fn segment_meets(&self, p: [f64; 3], q: [f64; 3]) -> bool {
        if self.nodes.is_empty() {
            return false;
        }
        let overlaps = |lo: [f64; 3], hi: [f64; 3]| segment_may_touch_box(p, q, lo, hi);
        let mut stack = vec![0usize];
        while let Some(index) = stack.pop() {
            let node = self.nodes[index];
            if !overlaps(node.min, node.max) {
                continue;
            }
            if node.count == 0 {
                stack.push(node.start_or_right as usize);
                stack.push(index + 1);
                continue;
            }
            let start = node.start_or_right as usize;
            for t in &self.triangles[start..start + node.count as usize] {
                let (tmin, tmax) = bounds(t);
                if overlaps(tmin, tmax) && segment_meets_triangle(p, q, t.corners) {
                    return true;
                }
            }
        }
        false
    }

    /// First pair of indexed triangles whose closed boxes overlap and for which `improper`
    /// returns true, as original input indices `(lower, higher)`; `None` if there is none.
    ///
    /// Dual traversal of the tree against itself visits every overlapping pair exactly once.
    /// Cancellation is polled at `checkpoint` throughout the traversal.
    ///
    /// # Errors
    /// Returns [`Cancelled`] when the caller's predicate requests it.
    pub(crate) fn find_pair(
        &self,
        cancel: &mut Cancellation<'_>,
        checkpoint: Checkpoint,
        mut improper: impl FnMut(u32, u32) -> bool,
    ) -> Result<Option<(u32, u32)>, Cancelled> {
        if self.nodes.is_empty() {
            return Ok(None);
        }
        let overlap =
            |a: &Node, b: &Node| (0..3).all(|k| a.min[k] <= b.max[k] && b.min[k] <= a.max[k]);
        let mut stack = vec![(0usize, 0usize)];
        let mut work = 0usize;
        while let Some((i, j)) = stack.pop() {
            cancel.poll(checkpoint, work)?;
            work += 1;
            let (a, b) = (self.nodes[i], self.nodes[j]);
            if !overlap(&a, &b) {
                continue;
            }
            match (a.count > 0, b.count > 0) {
                (true, true) => {
                    let (sa, sb) = (a.start_or_right as usize, b.start_or_right as usize);
                    for x in sa..sa + a.count as usize {
                        let first = if i == j { x + 1 } else { sb };
                        for y in first..sb + b.count as usize {
                            let (tx, ty) = (&self.triangles[x], &self.triangles[y]);
                            let (bx, by) = (bounds(tx), bounds(ty));
                            if !(0..3).all(|k| bx.0[k] <= by.1[k] && by.0[k] <= bx.1[k]) {
                                continue;
                            }
                            cancel.poll(checkpoint, work)?;
                            work += 1;
                            let (p, q) = (self.ids[x], self.ids[y]);
                            if improper(p.min(q), p.max(q)) {
                                return Ok(Some((p.min(q), p.max(q))));
                            }
                        }
                    }
                }
                (false, true) => {
                    stack.push((i + 1, j));
                    stack.push((a.start_or_right as usize, j));
                }
                (true, false) => {
                    stack.push((i, j + 1));
                    stack.push((i, b.start_or_right as usize));
                }
                (false, false) if i == j => {
                    let (left, right) = (i + 1, a.start_or_right as usize);
                    stack.push((left, left));
                    stack.push((right, right));
                    stack.push((left, right));
                }
                (false, false) => {
                    let (al, ar) = (i + 1, a.start_or_right as usize);
                    let (bl, br) = (j + 1, b.start_or_right as usize);
                    stack.extend([(al, bl), (al, br), (ar, bl), (ar, br)]);
                }
            }
        }
        Ok(None)
    }
}

/// Node pairs a parallel pair search ([`Bvh::pairs`]) is split into before the threads take
/// them: many more than cores, so unequal pairs balance.
const PAIR_TASKS: usize = 4_096;

impl Bvh {
    /// Every pair of indexed triangles whose closed boxes overlap and for which `keep` holds, as
    /// original input indices `(lower, higher)`, searched on every core. The tree's node pairs
    /// are split level by level into [`PAIR_TASKS`] tasks, each searched as [`Self::find_pair`]
    /// does: the pairs come in an order fixed by the tree alone, whatever the thread count. The
    /// caller's thread polls cancellation at `checkpoint`.
    ///
    /// # Errors
    /// Returns [`Cancelled`] when the caller's predicate requests it.
    pub(crate) fn pairs(
        &self,
        cancel: &mut Cancellation<'_>,
        checkpoint: Checkpoint,
        keep: impl Fn(u32, u32) -> bool + Sync,
    ) -> Result<Vec<(u32, u32)>, Cancelled> {
        if self.nodes.is_empty() {
            return Ok(Vec::new());
        }
        // Split level by level while some pair has children and there are too few tasks.
        let mut tasks = vec![(0usize, 0usize)];
        while tasks.len() < PAIR_TASKS {
            let mut next = Vec::with_capacity(4 * tasks.len());
            let mut split = false;
            for &(i, j) in &tasks {
                if !self.nodes_overlap(i, j) {
                    continue;
                }
                match self.child_pairs(i, j) {
                    Some(children) => {
                        split = true;
                        next.extend(children);
                    }
                    None => next.push((i, j)),
                }
            }
            tasks = next;
            if !split {
                break;
            }
        }
        let mut found: Vec<Vec<(u32, u32)>> = vec![Vec::new(); tasks.len()];
        crate::parallel::fill_balanced_cancellable(&mut found, cancel, checkpoint, |t, halted| {
            let mut out = Vec::new();
            let mut stack = vec![tasks[t]];
            let mut work = 0usize;
            while let Some((i, j)) = stack.pop() {
                work += 1;
                if work.is_multiple_of(256) && halted.load(AtomicOrdering::Relaxed) {
                    break;
                }
                if !self.nodes_overlap(i, j) {
                    continue;
                }
                if let Some(children) = self.child_pairs(i, j) {
                    stack.extend(children);
                    continue;
                }
                let (a, b) = (self.nodes[i], self.nodes[j]);
                let (sa, sb) = (a.start_or_right as usize, b.start_or_right as usize);
                for x in sa..sa + a.count as usize {
                    let first = if i == j { x + 1 } else { sb };
                    for y in first..sb + b.count as usize {
                        let (bx, by) = (bounds(&self.triangles[x]), bounds(&self.triangles[y]));
                        if !(0..3).all(|k| bx.0[k] <= by.1[k] && by.0[k] <= bx.1[k]) {
                            continue;
                        }
                        let (p, q) = (self.ids[x], self.ids[y]);
                        let pair = (p.min(q), p.max(q));
                        if keep(pair.0, pair.1) {
                            out.push(pair);
                        }
                    }
                }
            }
            out
        })?;
        Ok(found.concat())
    }

    /// Whether the boxes of nodes `i` and `j` overlap.
    fn nodes_overlap(&self, i: usize, j: usize) -> bool {
        let (a, b) = (&self.nodes[i], &self.nodes[j]);
        (0..3).all(|k| a.min[k] <= b.max[k] && b.min[k] <= a.max[k])
    }

    /// The node pairs that together hold every triangle pair of `(i, j)` (with `i == j`, each
    /// pair once); `None` for two leaves.
    fn child_pairs(&self, i: usize, j: usize) -> Option<Vec<(usize, usize)>> {
        let (a, b) = (self.nodes[i], self.nodes[j]);
        match (a.count > 0, b.count > 0) {
            (true, true) => None,
            (false, true) => Some(vec![(i + 1, j), (a.start_or_right as usize, j)]),
            (true, false) => Some(vec![(i, j + 1), (i, b.start_or_right as usize)]),
            (false, false) if i == j => {
                let (left, right) = (i + 1, a.start_or_right as usize);
                Some(vec![(left, left), (right, right), (left, right)])
            }
            (false, false) => {
                let (al, ar) = (i + 1, a.start_or_right as usize);
                let (bl, br) = (j + 1, b.start_or_right as usize);
                Some(vec![(al, bl), (al, br), (ar, bl), (ar, br)])
            }
        }
    }
}

/// Lower bound on the Euclidean distance between two closed axis-aligned boxes (zero when they
/// overlap).
///
/// The per-axis gaps are scaled by the largest one before squaring, so no square underflows
/// into the subnormal range where its relative error is unbounded: the scaled sum lies in
/// `[1, 3]` and every operation keeps a relative error of a few units in the last place, far
/// below the `1e-12` shrink. A result that would itself be subnormal is replaced by the largest
/// gap, which is exact there (a subtraction with a subnormal result is exact) and never exceeds
/// the distance.
fn box_box_distance(amin: [f64; 3], amax: [f64; 3], bmin: [f64; 3], bmax: [f64; 3]) -> f64 {
    let gaps: [f64; 3] = std::array::from_fn(|axis| {
        (bmin[axis] - amax[axis])
            .max(amin[axis] - bmax[axis])
            .max(0.0)
    });
    let largest = gaps[0].max(gaps[1]).max(gaps[2]);
    if largest.is_nan() || largest <= 0.0 {
        return 0.0;
    }
    let scaled: f64 = gaps
        .iter()
        .map(|gap| (gap / largest) * (gap / largest))
        .sum();
    let distance = largest * scaled.sqrt();
    if distance < f64::MIN_POSITIVE {
        return largest;
    }
    distance * (1.0 - 1e-12)
}

/// Conservative test whether the closed segment `p q` can touch the closed box `[lo, hi]`.
///
/// Exact coordinate comparisons reject boxes outside the segment's own box. Then, per axis on
/// which the segment moves, the parameters at which it crosses the two slab planes are enclosed
/// in outward-rounded intervals; the box is rejected only when the widened parameter ranges of
/// the three slabs and `[0, 1]` share no point. The exact ranges lie inside the widened ones, so
/// a touched box is never rejected. A division that cannot be enclosed skips that axis.
fn segment_may_touch_box(p: [f64; 3], q: [f64; 3], lo: [f64; 3], hi: [f64; 3]) -> bool {
    if (0..3).any(|k| p[k].max(q[k]) < lo[k] || p[k].min(q[k]) > hi[k]) {
        return false;
    }
    let (mut enter, mut exit) = (0.0f64, 1.0f64);
    for k in 0..3 {
        #[allow(clippy::float_cmp)] // Exact: no motion on this axis, already inside its slab.
        if p[k] == q[k] {
            continue;
        }
        let step = I::point(q[k]).sub(I::point(p[k]));
        let (Some(a), Some(b)) = (
            I::point(lo[k]).sub(I::point(p[k])).div(step),
            I::point(hi[k]).sub(I::point(p[k])).div(step),
        ) else {
            continue;
        };
        enter = enter.max(a.lo.min(b.lo));
        exit = exit.min(a.hi.max(b.hi));
    }
    // Incomparable (NaN) bounds keep the box.
    enter.partial_cmp(&exit) != Some(Ordering::Greater)
}

/// Heap key of a non-negative distance: binary64 bit patterns of non-negative values (negative
/// zero mapped to zero) sort like the values.
fn distance_key(distance: f64) -> u64 {
    if distance > 0.0 {
        distance.to_bits()
    } else {
        0
    }
}

impl Bvh {
    /// Certified lower bound on the distance from `query` to the indexed surface (zero when they
    /// meet), from a bounded best-first search (TD-066 oblique drain caps).
    ///
    /// Nodes are ordered by their box distance from the query's box, which bounds every triangle
    /// inside. Each examined triangle contributes its rigorous [`separation`] bound (zero if the
    /// triangles meet). At any point `min(best, next key)` bounds the whole surface from below;
    /// the search stops when the next box is no nearer than the best bound, or after a fixed
    /// number of visits, when the result is still certified, only less tight.
    ///
    /// # Errors
    /// Returns [`Cancelled`] when the caller's predicate requests it (polled at `checkpoint`).
    pub(super) fn lower_bound(
        &self,
        query: &Triangle,
        cancel: &mut Cancellation<'_>,
        checkpoint: Checkpoint,
    ) -> Result<f64, Cancelled> {
        if self.nodes.is_empty() {
            return Ok(f64::INFINITY);
        }
        let (qmin, qmax) = bounds(query);
        let key = |node: &Node| distance_key(box_box_distance(qmin, qmax, node.min, node.max));
        let mut heap = BinaryHeap::new();
        heap.push(Reverse((key(&self.nodes[0]), 0usize)));
        let mut best = f64::INFINITY;
        let mut visits = 0usize;
        while let Some(&Reverse((next, index))) = heap.peek() {
            let next = f64::from_bits(next);
            if next >= best || visits >= LOWER_BOUND_VISITS {
                return Ok(best.min(next));
            }
            heap.pop();
            cancel.poll(checkpoint, visits)?;
            visits += 1;
            let node = self.nodes[index];
            if node.count == 0 {
                let right = node.start_or_right as usize;
                heap.push(Reverse((key(&self.nodes[index + 1]), index + 1)));
                heap.push(Reverse((key(&self.nodes[right]), right)));
                continue;
            }
            let start = node.start_or_right as usize;
            for t in &self.triangles[start..start + node.count as usize] {
                if triangles_meet(query.corners, t.corners) {
                    return Ok(0.0);
                }
                let (tmin, tmax) = bounds(t);
                best = best.min(
                    separation(query, t)
                        .1
                        .max(box_box_distance(qmin, qmax, tmin, tmax)),
                );
            }
        }
        Ok(best)
    }
}

/// Whether the query's vertices are proven more than `threshold` from the plane's slab along its
/// normal, with the same squared comparison as [`projection_gap_exceeds`].
fn plane_gap_exceeds(query: &Triangle, plane: &Plane, threshold: f64) -> bool {
    let (mut low, mut high) = (f64::INFINITY, f64::NEG_INFINITY);
    for &v in &query.corners {
        let p = interval_dot(plane.normal, v);
        low = low.min(p.lo);
        high = high.max(p.hi);
    }
    let gap = I::point(low)
        .sub(I::point(plane.high))
        .lo
        .max(I::point(plane.low).sub(I::point(high)).lo);
    if gap.is_nan() || gap <= 0.0 || threshold.is_nan() || threshold < 0.0 {
        return false;
    }
    let squared = I::point(gap).mul(I::point(gap)).lo;
    let bound = I::point(threshold)
        .mul(I::point(threshold))
        .mul(I::point(plane.norm_sq))
        .hi;
    squared.is_finite() && bound.is_finite() && squared > bound
}

/// Node visits after which [`Bvh::disk_lower_bound`] stops refining.
const LOWER_BOUND_VISITS: usize = 1 << 14;

/// A planar disk normal to a world axis, enclosing a cap polygon, with that polygon's box.
#[derive(Clone, Copy, Debug)]
pub(super) struct CapDisk {
    /// Centre, in the cap plane.
    pub(super) center: [f64; 3],
    /// World axis normal to the disk.
    pub(super) axis: usize,
    /// Upper bound on the distance from the centre to every polygon vertex.
    pub(super) radius: f64,
    /// Exact coordinate extrema of the polygon.
    pub(super) min: [f64; 3],
    pub(super) max: [f64; 3],
}

impl Bvh {
    /// Certified lower bound on the distance from a cap polygon to the indexed surface, through
    /// its enclosing [`CapDisk`]; infinity for an empty index.
    ///
    /// The polygon lies in the disk, so any lower bound on the distance to the disk bounds the
    /// polygon's too. Best-first search over nodes is ordered by a lower bound for each node box:
    /// its distance from the polygon's box and its interval-proven projection gaps from the disk
    /// along the axis and towards the box centre. Each examined triangle contributes the largest of rigorous bounds: its box
    /// distance and the interval-proven gaps between its projection and the disk's projection onto
    /// the disk axis, the triangle's normal, and the directions from the centre to the triangle's
    /// centroid (in space and within the disk plane). Each costs constant work, whatever the
    /// number of polygon sides. At any point, `min(examined, next key)` bounds the whole surface
    /// from below, because every unexamined triangle lies in a box whose key bounds it. The
    /// search stops when the next box is no nearer than the best bound, or after a fixed number of
    /// visits; the result is certified either way, only less tight in the second case.
    ///
    /// # Errors
    /// Returns [`Cancelled`] when the caller's predicate requests it (polled at `checkpoint`).
    pub(super) fn disk_lower_bound(
        &self,
        disk: &CapDisk,
        cancel: &mut Cancellation<'_>,
        checkpoint: Checkpoint,
    ) -> Result<f64, Cancelled> {
        if self.nodes.is_empty() {
            return Ok(f64::INFINITY);
        }
        let mut normal = [0.0; 3];
        normal[disk.axis] = 1.0;
        // Node key: a lower bound for every triangle in the box, from the box distance and the
        // disk gaps along the axis and towards the box centre (in space and within the plane).
        let distance = |node: &Node| {
            let toward: [f64; 3] =
                std::array::from_fn(|k| f64::midpoint(node.min[k], node.max[k]) - disk.center[k]);
            let mut within = toward;
            within[disk.axis] = 0.0;
            let bound = [normal, toward, within]
                .into_iter()
                .map(|u| disk_box_gap(node.min, node.max, disk.center, disk.axis, disk.radius, u))
                .fold(
                    box_box_distance(disk.min, disk.max, node.min, node.max),
                    f64::max,
                );
            distance_key(bound)
        };
        let mut heap = BinaryHeap::new();
        heap.push(Reverse((distance(&self.nodes[0]), 0usize)));
        let mut best = f64::INFINITY;
        let mut visits = 0usize;
        while let Some(&Reverse((key, index))) = heap.peek() {
            let next = f64::from_bits(key);
            if next >= best || visits >= LOWER_BOUND_VISITS {
                return Ok(best.min(next));
            }
            heap.pop();
            cancel.poll(checkpoint, visits)?;
            visits += 1;
            let node = self.nodes[index];
            if node.count == 0 {
                let right = node.start_or_right as usize;
                heap.push(Reverse((distance(&self.nodes[index + 1]), index + 1)));
                heap.push(Reverse((distance(&self.nodes[right]), right)));
                continue;
            }
            let start = node.start_or_right as usize;
            for t in &self.triangles[start..start + node.count as usize] {
                let (tmin, tmax) = bounds(t);
                let centroid: [f64; 3] = std::array::from_fn(|k| {
                    (t.corners[0][k] + t.corners[1][k] + t.corners[2][k]) / 3.0
                });
                let toward: [f64; 3] = std::array::from_fn(|k| centroid[k] - disk.center[k]);
                let mut within = toward;
                within[disk.axis] = 0.0;
                let bound = [normal, triangle_normal(t), toward, within]
                    .into_iter()
                    .map(|u| disk_gap(t, disk.center, disk.axis, disk.radius, u))
                    .fold(box_box_distance(disk.min, disk.max, tmin, tmax), f64::max);
                best = best.min(bound);
            }
        }
        Ok(best)
    }
}

fn point(c: [f64; 3]) -> Point3 {
    Point3::new(c[0], c[1], c[2])
}

/// Exact sign of the projected orientation `(b-a) x (c-a)` in XY.
///
/// Uses the exact 3D predicate with a vertically lifted fourth point: `a + (0,0,1)` differs from
/// `a` only in Z by a positive amount for every admitted coordinate (|z| < 2^52), so the 3D sign
/// equals the 2D sign.
fn orient2d(a: [f64; 3], b: [f64; 3], c: [f64; 3]) -> Ordering {
    let lifted = Point3::new(a[0], a[1], a[2] + 1.0);
    orient3d(point(a), point(b), point(c), lifted)
}

/// Orientation of the symbolically perturbed query against directed projected edge `a -> b`.
fn orient2d_perturbed(a: [f64; 3], b: [f64; 3], p: [f64; 3]) -> Ordering {
    let exact = orient2d(a, b, p);
    if exact != Ordering::Equal {
        return exact;
    }
    // d/de of (b-a) x (p + (e, e^2) - a) is -(by-ay) at first order, then (bx-ax).
    match a[1].total_cmp(&b[1]) {
        Ordering::Equal => b[0].total_cmp(&a[0]),
        other => other,
    }
}

#[allow(clippy::many_single_char_names)] // Predicate notation: triangle a, b, c; query p/q; side s.
/// `Some(Greater)` when the triangle is stabbed strictly above `q`, `Some(Less)` when stabbed
/// below, `Some(Equal)` when `q` lies on it, `None` when the perturbed ray misses it.
fn stab_above(t: &Triangle, q: Point3) -> Option<Ordering> {
    let [a, b, c] = t.corners;
    let p = [q.x, q.y, q.z];
    let s = orient2d(a, b, c);
    if s == Ordering::Equal {
        return None;
    }
    for (u, v) in [(a, b), (b, c), (c, a)] {
        if orient2d_perturbed(u, v, p) != s {
            return None;
        }
    }
    // orient3d(a,b,c,q) is positive when q lies on the side of the (b-a)x(c-a) normal. With
    // counterclockwise projection (s > 0) that normal points up, so q is below the plane exactly
    // when the sign is negative; a clockwise projection flips the relation.
    let side = orient3d(point(a), point(b), point(c), q);
    Some(match (side, s) {
        (Ordering::Equal, _) => Ordering::Equal,
        (x, y) if x != y => Ordering::Greater,
        _ => Ordering::Less,
    })
}

#[cfg(test)]
mod tests {
    use super::{reorder, Bvh, CapDisk, Parity};
    use crate::{
        cancellation::{Cancellation, Checkpoint},
        preparation::sampled::distance::Triangle,
    };

    #[test]
    fn parallel_subtrees_build_the_sequential_hierarchy() {
        // TD-094: the same nodes and order from one thread or many, on a mesh large enough to
        // split across threads (deterministic pseudo-random triangles, some sharing centroids).
        use super::{build_subtree, node_count, Node, Shared};
        use std::sync::atomic::AtomicBool;
        let mut state = 0x2545_F491_4F6C_DD1Du64;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            f64::from(u32::try_from(state >> 40).unwrap()) / 1000.0
        };
        let triangles: Vec<Triangle> = (0..300_000)
            .map(|i| {
                let o = if i % 7 == 0 {
                    [1.0; 3]
                } else {
                    [next(), next(), next()]
                };
                Triangle::new([o, [o[0] + 1.0, o[1], o[2]], [o[0], o[1] + 1.0, o[2]]])
            })
            .collect();
        let build = |spawn: u32| {
            let mut centroids: Vec<([f64; 3], usize)> = triangles
                .iter()
                .enumerate()
                .map(|(i, t)| {
                    let c = std::array::from_fn(|a| {
                        (t.corners[0][a] + t.corners[1][a] + t.corners[2][a]) / 3.0
                    });
                    (c, i)
                })
                .collect();
            let empty = Node {
                min: [0.0; 3],
                max: [0.0; 3],
                start_or_right: 0,
                count: 0,
            };
            let mut nodes = vec![empty; node_count(triangles.len())];
            let halted = AtomicBool::new(false);
            let shared = Shared {
                triangle: &|index| triangles[index],
                halted: &halted,
            };
            let mut never = Cancellation::never();
            let mut caller = Some((&mut never, Checkpoint::HollowWork));
            assert!(build_subtree(
                &shared,
                &mut centroids,
                0,
                &mut nodes,
                0,
                spawn,
                &mut caller
            )
            .is_ok());
            let nodes: Vec<_> = nodes
                .iter()
                .map(|n| {
                    (
                        n.min.map(f64::to_bits),
                        n.max.map(f64::to_bits),
                        n.start_or_right,
                        n.count,
                    )
                })
                .collect();
            let order: Vec<usize> = centroids.iter().map(|c| c.1).collect();
            (nodes, order)
        };
        let sequential = build(0);
        assert_eq!(build(4), sequential);
        assert_eq!(build(1), sequential);
    }

    #[test]
    fn the_indexed_build_is_the_labeled_build() {
        // TD-094: triangles written once in slot order give the hierarchy, slots, labels and
        // ids of the build that reorders a copy, on a mesh large enough to use threads.
        use crate::{geometry::Point3, mesh::TriangleMesh};
        let side = 300u32;
        let vertices: Vec<Point3> = (0..side * side)
            .map(|i| {
                let (x, y) = (f64::from(i % side), f64::from(i / side));
                Point3::new(x, y, (x * 0.37 + y * 0.11).sin())
            })
            .collect();
        let mut faces = Vec::new();
        for y in 0..side - 1 {
            for x in 0..side - 1 {
                let v = y * side + x;
                faces.push([v, v + 1, v + side]);
                faces.push([v + 1, v + side + 1, v + side]);
            }
        }
        let mesh = TriangleMesh::try_new(vertices, faces.clone()).unwrap();
        let labels: Vec<u32> = (0..u32::try_from(faces.len()).unwrap())
            .map(|f| f % 5)
            .collect();
        let triangles: Vec<Triangle> = faces
            .iter()
            .map(|f| Triangle::new(f.map(|v| mesh.vertices()[v as usize].coordinates())))
            .collect();
        let mut never = Cancellation::never();
        let labeled =
            Bvh::build_labeled(triangles, &labels, &mut never, Checkpoint::HollowWork).unwrap();
        let indexed =
            Bvh::build_indexed(&mesh, &faces, &labels, &mut never, Checkpoint::HollowWork).unwrap();
        let bits = |bvh: &Bvh| {
            let nodes: Vec<_> = bvh
                .nodes
                .iter()
                .map(|n| {
                    (
                        n.min.map(f64::to_bits),
                        n.max.map(f64::to_bits),
                        n.start_or_right,
                        n.count,
                    )
                })
                .collect();
            let triangles: Vec<_> = bvh
                .triangles
                .iter()
                .map(|t| t.corners.map(|c| c.map(f64::to_bits)))
                .collect();
            (nodes, triangles, bvh.labels.clone(), bvh.ids.clone())
        };
        assert_eq!(bits(&indexed), bits(&labeled));
    }

    #[test]
    fn triangles_are_reordered_in_place_into_slot_order() {
        // TD-094: slot `p` takes the triangle at `order[p]`, through cycles of every length
        // (a fixed point, a swap, and a five-cycle) and labels follow.
        let order = [0usize, 2, 1, 4, 5, 6, 7, 3];
        let triangle = |i: usize| {
            let x = f64::from(u32::try_from(i).unwrap());
            Triangle::new([[x, 0.0, 0.0], [x, 1.0, 0.0], [x, 0.0, 1.0]])
        };
        let triangles: Vec<Triangle> = (0..order.len()).map(triangle).collect();
        let centroids: Vec<([f64; 3], usize)> = order.iter().map(|&i| ([0.0; 3], i)).collect();
        let labels: Vec<u32> = (10..18).collect();
        let (slots, slot_labels, ids) = reorder(
            triangles,
            &centroids,
            &labels,
            &mut Cancellation::never(),
            Checkpoint::HollowWork,
        )
        .unwrap();
        for (slot, &source) in order.iter().enumerate() {
            assert_eq!(slots[slot].corners, triangle(source).corners);
            assert_eq!(slot_labels[slot], labels[source]);
            assert_eq!(ids[slot] as usize, source);
        }
    }

    fn build(triangles: Vec<Triangle>) -> Bvh {
        Bvh::build(
            triangles,
            &mut Cancellation::never(),
            Checkpoint::HollowWork,
        )
        .unwrap()
    }

    fn clearance(bvh: &Bvh, query: &Triangle, threshold: f64) -> super::Clearance {
        bvh.clearance(
            query,
            threshold,
            &mut Cancellation::never(),
            Checkpoint::DrainageWork,
        )
        .unwrap()
    }

    fn cube(min: f64, max: f64) -> Vec<Triangle> {
        let corner = |i: usize| {
            [
                if i & 1 == 0 { min } else { max },
                if i & 2 == 0 { min } else { max },
                if i & 4 == 0 { min } else { max },
            ]
        };
        let quads = [
            [0, 2, 3, 1],
            [4, 5, 7, 6],
            [0, 1, 5, 4],
            [2, 6, 7, 3],
            [0, 4, 6, 2],
            [1, 3, 7, 5],
        ];
        let mut out = Vec::new();
        for q in quads {
            for t in [[q[0], q[1], q[2]], [q[0], q[2], q[3]]] {
                out.push(Triangle::new(t.map(corner)));
            }
        }
        out
    }

    #[test]
    fn clearance_decides_beyond_from_the_certified_bound_only() {
        use super::Clearance;
        // Review counterexample: the computed distance (3.5e-7, nearly parallel edges) far
        // exceeds the exact 1e-11 mm, and the interval bound proves nothing (zero).
        let s = Triangle::new([
            [
                -62.321_865_289_896_04,
                -40.600_826_339_883_72,
                10.167_937_777_769_552,
            ],
            [
                -8.055_629_407_157_287,
                -61.843_031_820_792_596,
                -71.096_718_435_284_13,
            ],
            [
                -36.370_227_588_182_83,
                -51.273_242_987_060_87,
                -31.239_936_153_501_95,
            ],
        ]);
        let t = Triangle::new([
            [
                -76.005_351_136_852_82,
                -35.244_505_348_102_59,
                30.659_200_992_464_342,
            ],
            [
                -8.055_629_081_021_202,
                -61.843_031_462_724_42,
                -71.096_718_311_097_05,
            ],
            [
                -34.007_267_108_865_21,
                -51.170_615_173_622_096,
                -29.688_844_504_007_356,
            ],
        ]);
        let bvh = build(vec![t]);
        for threshold in [2e-7, 1e-3, f64::INFINITY] {
            assert_eq!(
                clearance(&bvh, &s, threshold),
                Clearance::Within,
                "{threshold}"
            );
        }
        // A plainly distant triangle is beyond a small threshold and within a large one.
        let far = Triangle::new([[0.0, 0.0, 5.0], [1.0, 0.0, 5.0], [0.0, 1.0, 5.0]]);
        let bvh = build(cube(0.0, 2.0));
        assert_eq!(clearance(&bvh, &far, 1.0), Clearance::Beyond);
        assert_eq!(clearance(&bvh, &far, 10.0), Clearance::Within);
    }

    #[test]
    fn odd_labels_follow_nesting_and_masks_skip_whole_subtrees() {
        use super::ParityFailure;
        // Three nested cubes labelled 0 (outer), 1 and 2 (innermost).
        let mut triangles = cube(0.0, 6.0);
        triangles.extend(cube(1.0, 5.0));
        triangles.extend(cube(2.0, 4.0));
        let labels: Vec<u32> = (0..36).map(|i| i / 12).collect();
        let bvh = Bvh::build_labeled(
            triangles,
            &labels,
            &mut Cancellation::never(),
            Checkpoint::DrainageWork,
        )
        .unwrap();
        let odd = |p: [f64; 3], skip, mask| bvh.vertical_odd_labels(p, skip, mask).unwrap();
        assert_eq!(odd([3.0, 3.0, 3.0], None, None), [0, 1, 2]);
        assert_eq!(odd([3.0, 3.0, 1.5], None, None), [0, 1]);
        assert_eq!(odd([0.5, 0.5, 0.5], None, None), [0]);
        assert_eq!(odd([7.0, 3.0, 3.0], None, None), Vec::<u32>::new());
        assert_eq!(odd([3.0, 3.0, 3.0], Some(2), None), [0, 1]);
        // A mask keeping label 0 only: the inner cubes' subtrees are skipped, the outer parity
        // stays exact, and a point on a skipped surface is no longer "on the surface".
        let mask = bvh.node_mask(|label| label == 0).unwrap();
        let kept = odd([3.0, 3.0, 3.0], None, Some(&mask));
        assert!(kept.contains(&0), "{kept:?}");
        assert!(odd([3.0, 3.0, 4.0], None, Some(&mask)).contains(&0));
        assert_eq!(
            bvh.vertical_odd_labels([3.0, 3.0, 4.0], None, None),
            Err(ParityFailure::OnSurface)
        );
        // An all-false mask skips everything.
        let none = bvh.node_mask(|_| false).unwrap();
        assert!(none.iter().all(|&flag| !flag));
        assert_eq!(odd([3.0, 3.0, 3.0], None, Some(&none)), Vec::<u32>::new());
    }

    #[test]
    fn slab_filter_never_rejects_a_touched_box() {
        use super::segment_may_touch_box;
        use crate::preparation::sampled::contact::segment_meets_triangle;
        fn next(state: &mut u64, n: u64) -> u64 {
            *state ^= *state << 13;
            *state ^= *state >> 7;
            *state ^= *state << 17;
            *state % n
        }
        // Grid coordinates make touching corners, edges and faces frequent; every other round
        // uses irregular binary64 values.
        let state = &mut 0x1234_5678_9abc_def1_u64;
        let mut coordinate = |round: u32| {
            let grid = f64::from(u32::try_from(next(state, 9)).unwrap()) - 4.0;
            if round.is_multiple_of(2) {
                grid
            } else {
                grid / 3.0 + f64::from(u32::try_from(next(state, 1000)).unwrap()) * 1e-3
            }
        };
        let (mut touched, mut rejected) = (0, 0);
        for round in 0..20_000 {
            let p: [f64; 3] = std::array::from_fn(|_| coordinate(round));
            let q: [f64; 3] = std::array::from_fn(|_| coordinate(round));
            let a: [f64; 3] = std::array::from_fn(|_| coordinate(round));
            let b: [f64; 3] = std::array::from_fn(|_| coordinate(round));
            let lo: [f64; 3] = std::array::from_fn(|k| a[k].min(b[k]));
            let hi: [f64; 3] = std::array::from_fn(|k| a[k].max(b[k]));
            // Exact oracle: an endpoint inside the closed box, or contact with a face triangle.
            let corner = |i: usize| -> [f64; 3] {
                std::array::from_fn(|k| if i >> k & 1 == 0 { lo[k] } else { hi[k] })
            };
            let inside = |r: [f64; 3]| (0..3).all(|k| lo[k] <= r[k] && r[k] <= hi[k]);
            let faces = [
                [0, 2, 3, 1],
                [4, 5, 7, 6],
                [0, 1, 5, 4],
                [2, 6, 7, 3],
                [0, 4, 6, 2],
                [1, 3, 7, 5],
            ];
            let exact = inside(p)
                || inside(q)
                || faces.iter().any(|f| {
                    [[f[0], f[1], f[2]], [f[0], f[2], f[3]]].iter().any(|t| {
                        segment_meets_triangle(p, q, [corner(t[0]), corner(t[1]), corner(t[2])])
                    })
                });
            let kept = segment_may_touch_box(p, q, lo, hi);
            if exact {
                touched += 1;
                assert!(kept, "{p:?} {q:?} {lo:?} {hi:?}");
            } else if !kept {
                rejected += 1;
            }
        }
        assert!(touched > 2_000 && rejected > 2_000, "{touched} {rejected}");
    }

    #[test]
    fn cap_lower_bounds_are_certified_and_tight_for_parallel_faces() {
        use crate::preparation::sampled::distance::disjoint_triangles_distance_sq;
        let disk = |z: f64, radius: f64| CapDisk {
            center: [1.0, 1.0, z],
            axis: 2,
            radius,
            min: [1.0 - radius, 1.0 - radius, z],
            max: [1.0 + radius, 1.0 + radius, z],
        };
        let bound = |bvh: &Bvh, d: CapDisk| {
            bvh.disk_lower_bound(&d, &mut Cancellation::never(), Checkpoint::DrainageWork)
                .unwrap()
        };
        let bvh = build(cube(0.0, 2.0));
        // One millimetre below the bottom face, and 0.25 mm above the middle inside the cube,
        // where the side walls are 0.5 mm from the disk rim.
        let below = bound(&bvh, disk(-1.0, 0.5));
        assert!(below <= 1.0 && below > 1.0 - 1e-9, "{below}");
        let inner = bound(&bvh, disk(1.25, 0.5));
        assert!(inner <= 0.5 && inner > 0.5 - 1e-9, "{inner}");
        // Tilted surfaces: never above the computed distance to a polygon inside the disk.
        let tilted: Vec<Triangle> = cube(0.0, 2.0)
            .into_iter()
            .map(|t| {
                Triangle::new(
                    t.corners
                        .map(|c| [c[0] + 0.3 * c[2], c[1], c[2] + 0.2 * c[0]]),
                )
            })
            .collect();
        let bvh = build(tilted.clone());
        for step in 0..40 {
            let z = -3.0 + 0.1 * f64::from(step);
            // A square inscribed in the disk of radius 0.5 * sqrt(2) around (1, 1, z).
            let square = [[0.5, 0.5, z], [1.5, 0.5, z], [1.5, 1.5, z], [0.5, 1.5, z]];
            let value = bound(&bvh, disk(z, 0.5f64.hypot(0.5).next_up()));
            let fan = [
                Triangle::new([square[0], square[1], square[2]]),
                Triangle::new([square[0], square[2], square[3]]),
            ];
            let computed = tilted
                .iter()
                .flat_map(|t| {
                    fan.iter()
                        .map(move |f| disjoint_triangles_distance_sq(f, t))
                })
                .fold(f64::INFINITY, f64::min)
                .sqrt();
            assert!(
                value <= computed * (1.0 + 1e-12),
                "{z}: {value} > {computed}"
            );
            assert!(value >= 0.0);
        }
    }

    #[test]
    fn dense_walls_crossing_the_cap_plane_keep_a_positive_bound() {
        // 40,000 slivers on a cylinder of radius 1.6 crossing the plane of a radius-1.5 disk:
        // many more boxes than the visit budget lie inside the disk's square, yet the reported
        // bound stays close to the 0.1 mm gap.
        let n = 40_000u32;
        let triangles: Vec<Triangle> = (0..n)
            .map(|i| {
                let (a, b) = (
                    std::f64::consts::TAU * f64::from(i) / f64::from(n),
                    std::f64::consts::TAU * f64::from(i + 1) / f64::from(n),
                );
                Triangle::new([
                    [1.6 * a.cos(), 1.6 * a.sin(), -0.1],
                    [1.6 * b.cos(), 1.6 * b.sin(), -0.1],
                    [1.6 * a.cos(), 1.6 * a.sin(), 0.1],
                ])
            })
            .collect();
        let bvh = build(triangles);
        let disk = CapDisk {
            center: [0.0; 3],
            axis: 2,
            radius: 1.5,
            min: [-1.5, -1.5, 0.0],
            max: [1.5, 1.5, 0.0],
        };
        let value = bvh
            .disk_lower_bound(&disk, &mut Cancellation::never(), Checkpoint::DrainageWork)
            .unwrap();
        assert!(value <= 0.1 && value > 0.09, "{value}");
    }

    #[test]
    fn box_distances_stay_below_the_gap_in_the_subnormal_range() {
        use super::box_box_distance;
        // Review counterexample: a 1.6e-162 gap squared rounds up to the smallest subnormal.
        for gap in [
            1.6e-162,
            2.731_371_679_614_992e-162,
            3e-160,
            1e-200,
            5e-324,
            1e-3,
            1.0,
        ] {
            let d = box_box_distance([0.0; 3], [1.0, 1.0, 0.0], [0.0, 0.0, gap], [1.0; 3]);
            assert!(d <= gap, "{gap}: {d}");
            assert!(
                d >= gap * (1.0 - 1e-9) || gap < f64::MIN_POSITIVE,
                "{gap}: {d}"
            );
            // Two equal gaps: the distance is sqrt(2) times the gap, never more.
            let d = box_box_distance([-1.0; 3], [0.0; 3], [gap, -1.0, gap], [1.0; 3]);
            assert!(
                d <= gap * std::f64::consts::SQRT_2 * (1.0 + 1e-15),
                "{gap}: {d}"
            );
            assert!(d >= gap, "{gap}: {d}");
        }
    }

    #[test]
    fn node_reservations_match_the_built_tree_exactly() {
        use super::node_count;
        for n in [
            1usize, 2, 4, 5, 8, 9, 10, 20, 40, 100, 1000, 10_240, 32_768, 100_000,
        ] {
            let triangles: Vec<Triangle> = (0..n)
                .map(|i| {
                    let x = f64::from(u32::try_from(i).unwrap());
                    Triangle::new([[x, 0.0, 0.0], [x + 1.0, 0.0, 0.0], [x, 1.0, 0.5]])
                })
                .collect();
            let bvh = build(triangles);
            assert_eq!(bvh.nodes.len(), node_count(n), "{n}");
            assert_eq!(bvh.nodes.capacity(), bvh.nodes.len(), "{n}");
        }
        assert_eq!(node_count(0), 0);
    }

    #[test]
    fn construction_stops_on_cancellation() {
        let triangles: Vec<Triangle> = (0..20_000)
            .map(|i| {
                let x = f64::from(i);
                Triangle::new([[x, 0.0, 0.0], [x + 1.0, 0.0, 0.0], [x, 1.0, 0.5]])
            })
            .collect();
        let mut predicate = |c| c == Checkpoint::HollowWork;
        let result = Bvh::build(
            triangles,
            &mut Cancellation::new(&mut predicate),
            Checkpoint::HollowWork,
        );
        assert!(matches!(result, Err(super::BuildFailure::Cancelled(_))));
    }

    #[test]
    fn self_pair_search_finds_contacts_and_stops_on_cancellation() {
        use crate::cancellation::{Cancellation, Checkpoint};
        let mut triangles = cube(0.0, 2.0);
        triangles.extend(cube(1.0, 3.0));
        let bvh = build(triangles);
        // Faces of the two overlapping cubes cross; an always-false predicate visits every pair.
        let mut visited = 0;
        let found = bvh
            .find_pair(
                &mut Cancellation::never(),
                Checkpoint::DrainageWork,
                |_, _| {
                    visited += 1;
                    false
                },
            )
            .unwrap();
        assert_eq!(found, None);
        assert!(visited > 24, "{visited}");
        let found = bvh
            .find_pair(
                &mut Cancellation::never(),
                Checkpoint::DrainageWork,
                |x, y| (x < 12) != (y < 12),
            )
            .unwrap();
        assert!(found.is_some_and(|(x, y)| x < 12 && y >= 12), "{found:?}");
        let mut predicate = |c| c == Checkpoint::DrainageWork;
        let error = bvh
            .find_pair(
                &mut Cancellation::new(&mut predicate),
                Checkpoint::DrainageWork,
                |_, _| false,
            )
            .unwrap_err();
        assert_eq!(error.checkpoint, Checkpoint::DrainageWork);
    }

    #[test]
    fn the_parallel_pair_search_finds_what_the_serial_one_visits() {
        use crate::cancellation::{Cancellation, Checkpoint};
        // Many small overlapping cubes: enough nodes for the search to split into tasks.
        let mut triangles = Vec::new();
        for k in 0..400 {
            let at = f64::from(k % 20) * 0.7 + f64::from(k / 20) * 0.05;
            triangles.extend(cube(at, at + 1.0));
        }
        let bvh = build(triangles);
        let keep = |x: u32, y: u32| (x / 12) != (y / 12);
        let mut serial = Vec::new();
        bvh.find_pair(
            &mut Cancellation::never(),
            Checkpoint::BooleanContacts,
            |x, y| {
                if keep(x, y) {
                    serial.push((x, y));
                }
                false
            },
        )
        .unwrap();
        let parallel = bvh
            .pairs(
                &mut Cancellation::never(),
                Checkpoint::BooleanContacts,
                keep,
            )
            .unwrap();
        assert!(serial.len() > 1_000, "{}", serial.len());
        // The same pairs, in an order fixed by the tree: the same on every run.
        let mut sorted = parallel.clone();
        sorted.sort_unstable();
        serial.sort_unstable();
        assert_eq!(sorted, serial);
        let again = bvh
            .pairs(
                &mut Cancellation::never(),
                Checkpoint::BooleanContacts,
                keep,
            )
            .unwrap();
        assert_eq!(again, parallel);
        let mut predicate = |c| c == Checkpoint::BooleanContacts;
        let error = bvh
            .pairs(
                &mut Cancellation::new(&mut predicate),
                Checkpoint::BooleanContacts,
                keep,
            )
            .unwrap_err();
        assert_eq!(error.checkpoint, Checkpoint::BooleanContacts);
    }

    #[test]
    fn parity_is_exact_on_vertices_edges_and_faces_of_the_projection() {
        let mut triangles = cube(0.0, 2.0);
        // A nested inner shell makes its interior "outside" under even-odd parity.
        triangles.extend(cube(0.5, 1.5));
        let bvh = build(triangles);
        // (1,1) lies on projected diagonal edges; (0,0), (2,2) and (0,1) on projected corners/edges.
        for p in [
            [0.25, 0.25, 0.25],
            [1.0, 1.75, 1.0],
            [1.0, 1.0, 0.25],
            [1.0, 1.0, 1.75],
        ] {
            assert_eq!(bvh.vertical_parity(p), Parity::Inside, "{p:?}");
        }
        for p in [
            [1.0, 1.0, 1.0],
            [0.0, 0.0, -1.0],
            [2.0, 2.0, -1.0],
            [0.0, 1.0, -1.0],
            [1.0, 1.0, 3.0],
            [3.0, 1.0, 1.0],
            [2.0, 0.0, 5.0],
        ] {
            assert_eq!(bvh.vertical_parity(p), Parity::Outside, "{p:?}");
        }
        assert_eq!(bvh.vertical_parity([1.0, 1.0, 2.0]), Parity::OnSurface);
    }

    #[test]
    fn nearest_matches_brute_force() {
        let mut triangles = cube(0.0, 2.0);
        triangles.extend(cube(5.0, 6.0));
        let bvh = build(triangles.clone());
        for (i, p) in [
            [1.0, 1.0, 1.0],
            [3.5, 3.5, 3.5],
            [-1.0, 0.5, 7.0],
            [5.5, 5.5, 5.4],
            [2.0, 2.0, 2.0],
        ]
        .into_iter()
        .enumerate()
        {
            let brute = triangles
                .iter()
                .map(|t| super::point_triangle_distance_sq(p, t))
                .fold(f64::INFINITY, f64::min);
            let (fast, _) = bvh.nearest(p, i);
            assert_eq!(fast, brute, "{p:?}");
        }
    }
}
