//! The localized arrangement (TD-140, phase 2): the faces of a Boolean result.
//!
//! **Semantics.** Each input is a closed oriented triangle mesh on the grid (every edge used as
//! often in each direction); a point is in input `i` when its winding number there is positive, so
//! overlapping bodies count once. The operation is a function `keep` of the memberships, one per
//! input: union, intersection, difference and any combination of several inputs are the same
//! algorithm. The result's faces are the parts of the inputs' triangles with `keep` true on one
//! side and false on the other, facing out of the result.
//!
//! **Algorithm.**
//! 1. **Contacts.** A deterministic BVH pairs the triangles whose boxes overlap, on every core;
//!    an exact test marks those that meet a triangle of another input, or meet one of their own
//!    input other than along their shared vertices and edges. The others are **untouched**: no
//!    surface meets them, so the winding numbers on each side are the same all over them, and
//!    each is classified once and kept whole. Every input triangle is kept compactly, its planes
//!    and grid corners ([`InputTriangle`]); only the touched ones become polygons to cut.
//! 2. **Cells.** The inputs' box, widened by half a step, is the first cell; its corner lies
//!    outside every input, where every winding number is 0. A cell is halved across its longest
//!    axis at an odd number of half-steps, so a wall never contains a grid point and no input
//!    plane lies in a wall. Touched triangles are clipped to the halves; untouched ones go whole to
//!    each half they reach. Halving goes in two phases. While a cell holds more than [`LEAF`]
//!    touched parts or more than [`PART_CELL`] polygons, it is halved; then its touched parts'
//!    faces are made (steps 5 and 6). Then, while it holds more than [`WHOLE_LEAF`] untouched
//!    triangles, it is halved again for them alone, its touched parts carried whole to each half
//!    they reach, as obstacles for the walks and never cut again: a dense surface around a touched
//!    triangle no longer cuts it into as many parts as there are cells. A cell also stops when
//!    halving it three times in a row did not thin it (polygons fanning out from one point stay
//!    together however small the cell), or when it is one step wide.
//! 3. **Reference points.** Each cell's lower corner, perturbed, has a known winding-number
//!    vector, found by walking from its parent's corner along the axes ([`super::trace`]).
//! 4. **Early outs.** A cell whose `keep` cannot change, given the memberships of the inputs with
//!    no polygon in it, contributes nothing and is not divided further.
//! 5. **Parts.** Each touched polygon is cut by the support of every other touched polygon that
//!    may cross it, and by the edge planes of every coplanar one, so that no surface crosses a
//!    part's interior.
//! 6. **Classification.** For a part, or an untouched triangle whose centroid the cell holds, the
//!    winding numbers on both sides come from a walk from the cell's corner to the centroid of
//!    three of its vertices, strictly inside it, then across the plane through every coplanar part
//!    covering that point. Coplanar parts covering the same region give one face.
//!
//! Every decision is an exact integer sign; the order of cells and of polygons is fixed, so the
//! faces are the same, in the same order, on every platform and for any number of threads.
//! Untouched triangles that are faces are named by index ([`Face::Whole`]), not copied.
use super::{
    grid::GridPoint,
    plane::Plane,
    polygon::{InputTriangle, Polygon, Sheet},
    trace::{walk, Wide},
};
use crate::{
    cancellation::{Cancellation, Cancelled, Checkpoint},
    preparation::sampled::{
        bvh::{BuildFailure, Bvh},
        contact::{improper_contact, triangles_meet},
        distance::Triangle,
    },
};
use std::{
    cmp::Ordering,
    collections::HashMap,
    sync::atomic::{AtomicBool, Ordering as AtomicOrdering},
};

/// The most polygons a leaf holds before it is halved again.
pub(crate) const LEAF: usize = 16;

/// The most untouched triangles a cell holds, once its touched parts' faces are made, before it
/// is halved again: each costs a walk across the cell's polygons, which is cheap beside halving
/// while there are few.
const WHOLE_LEAF: usize = 64;

/// The most polygons a cell holds when its touched parts' faces are made: each part's walk
/// crosses them all, so a cell holding many untouched triangles is halved first, clipping its
/// touched parts, though they are few.
const PART_CELL: usize = 1024;

/// Halvings in a row that may leave a cell all its parent's polygons before it is a leaf: three,
/// one across each axis. Polygons that all meet in one point or along one line (a fan of
/// triangles around a hole's end cap) stay together however small the cell; halving further
/// would only clip the other polygons around them into more parts.
const STALLS: u8 = 3;

/// An input: a closed oriented triangle mesh on the grid.
#[derive(Clone, Debug, Default)]
pub(crate) struct Mesh {
    pub(crate) vertices: Vec<GridPoint>,
    pub(crate) triangles: Vec<[u32; 3]>,
}

/// A face of the result.
#[derive(Clone, Debug)]
pub(crate) enum Face {
    /// A part of a touched triangle, its support facing out of the result (boxed: faces are
    /// mostly whole triangles, a few bytes each).
    Part(Box<Polygon>),
    /// A whole untouched input triangle, by its index in [`Arranged::triangles`], facing out of
    /// the result as its support does or, `flipped`, the other way. Its vertices are the input's
    /// grid points, and no other face's vertex lies inside one of its edges but at its ends.
    Whole { index: u32, flipped: bool },
}

/// The arrangement's result: the input triangles, and the faces of the result, which name the
/// untouched triangles among them rather than copy them.
#[derive(Debug, Default)]
pub(crate) struct Arranged {
    pub(crate) triangles: Vec<InputTriangle>,
    pub(crate) faces: Vec<Face>,
}

impl Arranged {
    /// The polygon of `face`, its support facing out of the result.
    #[cfg(test)]
    pub(crate) fn polygon(&self, face: &Face) -> Polygon {
        match face {
            Face::Part(polygon) => (**polygon).clone(),
            Face::Whole { index, flipped } => {
                let mut polygon = self.triangles[*index as usize].polygon(*index);
                if *flipped {
                    polygon.support = polygon.support.flipped();
                }
                polygon
            }
        }
    }
}

/// Why an arrangement stopped.
#[derive(Debug)]
pub(crate) enum ArrangeError {
    Cancelled(Cancelled),
    /// Memory for the contact search could not be reserved.
    Resource,
}

impl std::fmt::Display for ArrangeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cancelled(c) => write!(f, "the Boolean was cancelled at {:?}", c.checkpoint),
            Self::Resource => write!(f, "memory for the Boolean's contact search ran out"),
        }
    }
}

impl std::error::Error for ArrangeError {}

impl From<Cancelled> for ArrangeError {
    fn from(c: Cancelled) -> Self {
        Self::Cancelled(c)
    }
}

/// A cell: its walls in half-steps (odd numbers, `lo < hi` on each axis) and what it holds.
#[derive(Clone)]
struct Cell {
    lo: [i64; 3],
    hi: [i64; 3],
    /// Parts of touched triangles, clipped to the cell.
    clipped: Vec<Polygon>,
    /// Untouched triangles reaching the cell, whole, by index.
    whole: Vec<u32>,
    /// The winding numbers at the perturbed lower corner.
    winding: Vec<i64>,
    /// How many halvings in a row left it as many polygons as its parent held (touched parts
    /// only, until `done`).
    stalled: u8,
    /// Whether the touched parts' faces are made: halving now only places the untouched
    /// triangles in smaller cells, the touched parts carried whole to each half they reach as
    /// obstacles for the walks.
    done: bool,
}

impl Cell {
    /// The number of polygons it holds.
    fn held(&self) -> usize {
        self.clipped.len() + self.whole.len()
    }

    /// Whether halving left this half of `parent` about as many polygons as it: no fewer of any
    /// kind until the touched parts are done, then more than seven eighths of its untouched
    /// triangles (the parts, carried whole, are what they are). Triangles fanning out from one
    /// point leave a cell one or two at a time however small it gets; a leaf holding them all
    /// costs less than the halvings.
    fn stalls(&self, parent: &Self) -> bool {
        if self.done {
            8 * self.whole.len() > 7 * parent.whole.len()
        } else {
            self.held() >= parent.held() && self.clipped.len() >= parent.clipped.len()
        }
    }

    /// What a walk needs of every polygon the cell holds.
    fn polygons<'a>(&'a self, all: &'a [InputTriangle]) -> impl Iterator<Item = Sheet<'a>> + Clone {
        self.clipped
            .iter()
            .map(Polygon::sheet)
            .chain(self.whole.iter().map(move |&i| all[i as usize].sheet()))
    }
}

/// A wall plane: `2·x_axis = half`, its positive side above it (`upper`) or below.
fn wall(axis: usize, half: i64, upper: bool) -> Plane {
    let mut n = [0; 3];
    n[axis] = if upper { 2 } else { -2 };
    let d = if upper {
        -i128::from(half)
    } else {
        i128::from(half)
    };
    Plane { n, d }
}

/// The input's vertices welded by grid position, in first-occurrence order, and its triangles on
/// them.
fn weld(mesh: &Mesh) -> (Vec<GridPoint>, Vec<[u32; 3]>) {
    let mut index: HashMap<GridPoint, u32> = HashMap::new();
    let mut vertices = Vec::new();
    let mut map = Vec::with_capacity(mesh.vertices.len());
    for &v in &mesh.vertices {
        let next = u32::try_from(vertices.len()).unwrap_or(u32::MAX);
        let id = *index.entry(v).or_insert_with(|| {
            vertices.push(v);
            next
        });
        map.push(id);
    }
    let triangles = mesh
        .triangles
        .iter()
        .map(|t| t.map(|v| map[v as usize]))
        .collect();
    (vertices, triangles)
}

/// The faces of the Boolean result `keep` of `inputs`, facing out of the result.
///
/// # Errors
/// Cancellation, and a failed reservation in the contact search.
pub(crate) fn arrange(
    inputs: &[Mesh],
    keep: &(dyn Fn(&[bool]) -> bool + Sync),
    cancel: &mut Cancellation<'_>,
) -> Result<Arranged, ArrangeError> {
    let cores = std::thread::available_parallelism().map_or(1, usize::from);
    arrange_with(inputs, keep, cancel, 64 * cores)
}

/// [`arrange`], stepping level by level until there are `target` cells before finishing them
/// in parallel. The faces do not depend on `target`.
fn arrange_with(
    inputs: &[Mesh],
    keep: &(dyn Fn(&[bool]) -> bool + Sync),
    cancel: &mut Cancellation<'_>,
    target: usize,
) -> Result<Arranged, ArrangeError> {
    let count = inputs.len();
    // Every non-degenerate triangle as a polygon, with its welded corners for the contacts.
    let mut candidates = Vec::new();
    for (input, mesh) in inputs.iter().enumerate() {
        let (vertices, triangles) = weld(mesh);
        let input = u32::try_from(input).map_err(|_| ArrangeError::Resource)?;
        candidates.extend(
            triangles
                .into_iter()
                .map(|t| (input, t, t.map(|v| vertices[v as usize]))),
        );
    }
    let mut made: Vec<Option<InputTriangle>> = vec![None; candidates.len()];
    crate::parallel::fill_with(&mut made, |i| {
        let (input, _, c) = candidates[i];
        InputTriangle::new(c[0], c[1], c[2], input)
    });
    let faces_of: Vec<_> = candidates
        .into_iter()
        .zip(&made)
        .filter_map(|(face, triangle)| triangle.as_ref().map(|_| face))
        .collect();
    // Collected in place: the triangles are never held twice.
    let all: Vec<InputTriangle> = made.into_iter().flatten().collect();
    if u32::try_from(all.len()).is_err() {
        return Err(ArrangeError::Resource);
    }
    let touched = contacts(&faces_of, cancel)?;
    let Some((lo, hi)) = bounds(&faces_of) else {
        return Ok(Arranged::default());
    };
    drop(faces_of);
    let mut clipped = Vec::new();
    let mut whole = Vec::new();
    for (i, triangle) in (0_u32..).zip(&all) {
        if touched[i as usize] {
            clipped.push(triangle.polygon(i));
        } else {
            whole.push(i);
        }
    }
    // The first cell's walls half a step outside every vertex.
    let root = Cell {
        lo: lo.map(|v| 2 * v - 1),
        hi: hi.map(|v| 2 * v + 1),
        clipped,
        whole,
        winding: vec![0; count],
        stalled: 0,
        done: false,
    };
    let context = Context { all: &all, keep };
    let faces = finish_all(&context, root, target, cancel)?;
    Ok(Arranged {
        triangles: all,
        faces,
    })
}

/// The faces of `root`, finished as [`Context::finish`] would, the work shared among threads:
/// stepped level by level until there are `target` cells, then each finished on its own.
///
/// # Errors
/// Cancellation, polled while every level and every cell is worked on: a cell being finished
/// stops at its next step.
fn finish_all(
    context: &Context<'_>,
    root: Cell,
    target: usize,
    cancel: &mut Cancellation<'_>,
) -> Result<Vec<Face>, ArrangeError> {
    // Step level by level, each cell replaced in place by what it becomes (its halves, or itself
    // with its touched parts' faces made), the faces it made kept before its first successor,
    // until there is work for every core. The frontier is then in depth-first order: each of its
    // cells is finished on its own, in parallel, and the faces are joined in that order, the
    // same order as finishing the first cell alone, for any thread count.
    let mut frontier: Vec<(Vec<Face>, Option<Cell>)> = vec![(Vec::new(), Some(root))];
    loop {
        let live = frontier.iter().filter(|(_, cell)| cell.is_some()).count();
        if live == 0 || live >= target {
            break;
        }
        let mut stepped: Vec<Option<(Vec<Face>, Vec<Cell>)>> =
            (0..frontier.len()).map(|_| None).collect();
        crate::parallel::fill_balanced_cancellable(
            &mut stepped,
            cancel,
            Checkpoint::BooleanCells,
            |i, _| {
                frontier[i].1.as_ref().map(|cell| {
                    let mut faces = Vec::new();
                    let next = context.step(cell.clone(), &mut faces);
                    (faces, next)
                })
            },
        )?;
        let mut next = Vec::with_capacity(2 * frontier.len());
        for ((mut prefix, cell), stepped) in frontier.into_iter().zip(stepped) {
            match stepped {
                None => next.push((prefix, cell)),
                Some((faces, successors)) => {
                    prefix.extend(faces);
                    if successors.is_empty() {
                        next.push((prefix, None));
                    } else {
                        for successor in successors {
                            next.push((std::mem::take(&mut prefix), Some(successor)));
                        }
                    }
                }
            }
        }
        frontier = next;
    }
    let mut parts: Vec<Vec<Face>> = (0..frontier.len()).map(|_| Vec::new()).collect();
    crate::parallel::fill_balanced_cancellable(
        &mut parts,
        cancel,
        Checkpoint::BooleanCells,
        |i, halted| {
            let mut faces = frontier[i].0.clone();
            if let Some(cell) = &frontier[i].1 {
                faces.extend(context.finish(cell.clone(), halted));
            }
            faces
        },
    )?;
    Ok(parts.concat())
}

/// What every cell of one arrangement shares.
struct Context<'a> {
    all: &'a [InputTriangle],
    keep: &'a (dyn Fn(&[bool]) -> bool + Sync),
}

impl Context<'_> {
    /// Where to halve `cell` as it is finished: while it holds more than [`LEAF`] touched parts
    /// or more than [`PART_CELL`] polygons, then, once the parts' faces are made, more than
    /// [`WHOLE_LEAF`] untouched triangles; `None` when it is settled, stalled, or one step wide.
    fn split_of(&self, cell: &Cell) -> Option<(usize, i64)> {
        let over = if cell.done {
            cell.whole.len() > WHOLE_LEAF
        } else {
            cell.clipped.len() > LEAF || cell.held() > PART_CELL
        };
        if !over || cell.stalled >= STALLS || settled(cell, self.all, self.keep) {
            return None;
        }
        middle(cell)
    }

    /// The faces of `cell`, depth first, the lower half first; cut short, to be discarded, once
    /// `halted` is raised.
    fn finish(&self, cell: Cell, halted: &AtomicBool) -> Vec<Face> {
        let mut faces = Vec::new();
        let mut stack = vec![cell];
        while let Some(next) = stack.pop() {
            if halted.load(AtomicOrdering::Relaxed) {
                break;
            }
            let successors = self.step(next, &mut faces);
            stack.extend(successors.into_iter().rev());
        }
        faces
    }

    /// One step of finishing `cell`: its halves, in the order they are finished; or its
    /// touched parts' faces added to `faces` and the cell, done, to halve further for its
    /// untouched triangles; or its untouched triangles' faces, and nothing more.
    fn step(&self, mut cell: Cell, faces: &mut Vec<Face>) -> Vec<Cell> {
        if let Some((axis, middle)) = self.split_of(&cell) {
            let (below, above) = halve(&cell, self.all, axis, middle);
            return vec![below, above];
        }
        if cell.held() == 0 || settled(&cell, self.all, self.keep) {
            return Vec::new();
        }
        if !cell.done {
            part_faces(&cell, self.all, self.keep, faces);
            cell.done = true;
            cell.stalled = 0;
            if self.split_of(&cell).is_some() {
                return vec![cell];
            }
        }
        whole_faces(&cell, self.all, self.keep, faces);
        Vec::new()
    }
}

/// Where to halve `cell`: its longest axis and an odd number of half-steps near its middle;
/// `None` when it is one step wide.
fn middle(cell: &Cell) -> Option<(usize, i64)> {
    let axis = (0..3)
        .max_by_key(|&k| (cell.hi[k] - cell.lo[k], std::cmp::Reverse(k)))
        .unwrap_or(0);
    let width = cell.hi[axis] - cell.lo[axis];
    if width <= 2 {
        return None;
    }
    let mut middle = cell.lo[axis] + width / 2;
    if middle % 2 == 0 {
        middle += 1;
    }
    Some((axis, middle))
}

/// Which triangles meet another surface: a triangle of another input anywhere, or one of their
/// own input other than along the vertices they share.
fn contacts(
    faces: &[(u32, [u32; 3], [GridPoint; 3])],
    cancel: &mut Cancellation<'_>,
) -> Result<Vec<bool>, ArrangeError> {
    let corners = |i: usize| faces[i].2.map(|v| v.point().coordinates());
    let triangles: Vec<Triangle> = (0..faces.len())
        .map(|i| Triangle::new(corners(i)))
        .collect();
    let bvh = Bvh::build(triangles, cancel, Checkpoint::BooleanContacts).map_err(|e| match e {
        BuildFailure::Cancelled(c) => ArrangeError::Cancelled(c),
        BuildFailure::Allocation => ArrangeError::Resource,
    })?;
    let meeting = bvh.pairs(cancel, Checkpoint::BooleanContacts, |a, b| {
        let (a, b) = (a as usize, b as usize);
        if separated(&faces[a], &faces[b]) {
            false
        } else if let Some(meet) = coplanar_contact(&faces[a], &faces[b]) {
            meet
        } else if faces[a].0 == faces[b].0 {
            improper_contact(faces[a].1, corners(a), faces[b].1, corners(b))
        } else {
            triangles_meet(corners(a), corners(b))
        }
    })?;
    let mut touched = vec![false; faces.len()];
    for (a, b) in meeting {
        touched[a as usize] = true;
        touched[b as usize] = true;
    }
    Ok(touched)
}

/// The exact orientation of `d` against the triangle `(a, b, c)` of grid points, in `i128`:
/// differences below 2²⁷, the determinant below `6·2⁸¹`.
fn orient(a: GridPoint, b: GridPoint, c: GridPoint, d: GridPoint) -> Ordering {
    let diff = |p: GridPoint| [p.x - a.x, p.y - a.y, p.z - a.z].map(i128::from);
    let (u, v, w) = (diff(b), diff(c), diff(d));
    let det = u[0] * (v[1] * w[2] - v[2] * w[1]) - u[1] * (v[0] * w[2] - v[2] * w[0])
        + u[2] * (v[0] * w[1] - v[1] * w[0]);
    det.cmp(&0)
}

/// A quick exact proof that two triangles meet at most where they may: one has every vertex it
/// does not share with the other strictly on one side of the other's plane. Then they meet only
/// in their shared vertices (or edge), which is proper within one input and, across inputs,
/// shared vertices are impossible (different inputs share no index but may share positions, which
/// this test does not count as shared). `false` decides nothing.
pub(crate) fn separated(a: &Candidate, b: &Candidate) -> bool {
    let one_side = |s: &Candidate, t: &Candidate| {
        let own = s.0 == t.0;
        let [p, q, r] = t.2;
        let mut side = Ordering::Equal;
        for (k, &v) in s.2.iter().enumerate() {
            if own && t.1.contains(&s.1[k]) {
                continue;
            }
            match orient(p, q, r, v) {
                Ordering::Equal => return false,
                o if side == Ordering::Equal => side = o,
                o if o != side => return false,
                _ => {}
            }
        }
        side != Ordering::Equal
    };
    one_side(a, b) || one_side(b, a)
}

/// A triangle for the contact search: its input, its welded vertex indices and its corners.
pub(crate) type Candidate = (u32, [u32; 3], [GridPoint; 3]);

/// The exact orientation of `c` against the directed line `a → b`, in the projection that drops
/// coordinate `axis`.
fn orient2(a: GridPoint, b: GridPoint, c: GridPoint, axis: usize) -> Ordering {
    let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
    let (a, b, c) = (a.coordinates(), b.coordinates(), c.coordinates());
    let d = |p: [i64; 3], k: usize| i128::from(p[k] - a[k]);
    (d(b, u) * d(c, v) - d(b, v) * d(c, u)).cmp(&0)
}

/// Whether two coplanar triangles meet improperly (within one input: beyond the vertices and
/// edge they share) or at all (across inputs), decided exactly in the plane; `None` when they are
/// not coplanar. The same rules as the general test: a shared edge is proper when the triangles
/// lie on its two sides; a shared vertex when neither corner there holds a boundary ray of the
/// other; no shared vertex when an edge line separates them strictly.
pub(crate) fn coplanar_contact(a: &Candidate, b: &Candidate) -> Option<bool> {
    let [p, q, r] = a.2;
    if b.2.iter().any(|&v| orient(p, q, r, v) != Ordering::Equal) {
        return None;
    }
    // Project along the axis the common normal leans on most; orientations keep their sign
    // for every triangle of the plane.
    let normal = super::plane::Plane::through(p, q, r)?;
    let axis = (0..3)
        .max_by_key(|&k| normal.n[k].unsigned_abs())
        .unwrap_or(0);
    let own = a.0 == b.0;
    let shared: Vec<(usize, usize)> = if own {
        (0..3)
            .flat_map(|i| (0..3).map(move |j| (i, j)))
            .filter(|&(i, j)| a.1[i] == b.1[j])
            .collect()
    } else {
        Vec::new()
    };
    let o = |x: GridPoint, y: GridPoint, z: GridPoint| orient2(x, y, z, axis);
    Some(match shared.len() {
        // Three shared vertices: a duplicated face.
        3 => true,
        2 => {
            let (i0, j0) = shared[0];
            let (i1, _) = shared[1];
            let (s, t) = (a.2[i0], a.2[i1]);
            let lone_a = a.2[3 - i0 - i1];
            let lone_b = b.2[(0..3)
                .find(|j| shared.iter().all(|&(_, sj)| sj != *j))
                .unwrap_or(j0)];
            o(s, t, lone_a) == o(s, t, lone_b)
        }
        1 => {
            let (i, j) = shared[0];
            let w = a.2[i];
            let rays = |f: &Candidate, at: usize| {
                let (x, y) = (f.2[(at + 1) % 3], f.2[(at + 2) % 3]);
                // Counterclockwise in the projection.
                if o(w, x, y) == Ordering::Less {
                    (y, x)
                } else {
                    (x, y)
                }
            };
            let ((a1, a2), (b1, b2)) = (rays(a, i), rays(b, j));
            let holds = |(x, y): (GridPoint, GridPoint), ray: GridPoint| {
                o(w, x, ray) != Ordering::Less && o(w, ray, y) != Ordering::Less
            };
            holds((a1, a2), b1) || holds((a1, a2), b2) || holds((b1, b2), a1) || holds((b1, b2), a2)
        }
        _ => {
            // Closed triangles meet unless an edge line has the other strictly outside.
            let outside = |f: &Candidate, g: &Candidate| {
                let turn = o(f.2[0], f.2[1], f.2[2]);
                (0..3).any(|k| {
                    let (x, y) = (f.2[k], f.2[(k + 1) % 3]);
                    g.2.iter().all(|&v| o(x, y, v) == turn.reverse())
                })
            };
            !(outside(a, b) || outside(b, a))
        }
    })
}

/// The grid box of every triangle.
fn bounds(faces: &[(u32, [u32; 3], [GridPoint; 3])]) -> Option<([i64; 3], [i64; 3])> {
    let mut lo = [i64::MAX; 3];
    let mut hi = [i64::MIN; 3];
    for (_, _, corners) in faces {
        for v in corners {
            for (k, c) in v.coordinates().into_iter().enumerate() {
                lo[k] = lo[k].min(c);
                hi[k] = hi[k].max(c);
            }
        }
    }
    (lo[0] <= hi[0]).then_some((lo, hi))
}

/// The two halves of `cell` across the wall at `middle` half-steps on `axis`: touched parts
/// clipped, untouched triangles whole to each half they reach, and the winding numbers at each
/// half's lower corner.
fn halve(cell: &Cell, all: &[InputTriangle], axis: usize, middle: i64) -> (Cell, Cell) {
    let upper_wall = wall(axis, middle, true);
    let (mut below, mut above) = (Vec::new(), Vec::new());
    for polygon in &cell.clipped {
        if cell.done {
            // Whole, to each half it reaches: some vertex strictly inside it.
            let sides: Vec<Ordering> = polygon
                .vertices
                .iter()
                .map(|v| v.compare_half(axis, middle))
                .collect();
            if sides.contains(&Ordering::Greater) {
                above.push(polygon.clone());
            }
            if sides.contains(&Ordering::Less) {
                below.push(polygon.clone());
            }
        } else {
            let split = polygon.split(&upper_wall);
            above.extend(split.positive);
            below.extend(split.negative);
        }
    }
    let (mut whole_below, mut whole_above) = (Vec::new(), Vec::new());
    for &i in &cell.whole {
        // An untouched triangle's corners are grid points, none on a wall (an odd number of
        // half-steps): each is strictly on one side, by its doubled coordinate.
        let corners = &all[i as usize].corners;
        if corners.iter().any(|v| 2 * v.coordinates()[axis] > middle) {
            whole_above.push(i);
        }
        if corners.iter().any(|v| 2 * v.coordinates()[axis] < middle) {
            whole_below.push(i);
        }
    }
    let mut corner = cell.lo;
    corner[axis] = middle;
    let mut winding = cell.winding.clone();
    walk(
        &cell.polygons(all),
        cell.lo,
        &Wide::half(corner),
        &mut winding,
    );
    let mut hi = cell.hi;
    hi[axis] = middle;
    let mut lo = cell.lo;
    lo[axis] = middle;
    let mut below = Cell {
        lo: cell.lo,
        hi,
        clipped: below,
        whole: whole_below,
        winding: cell.winding.clone(),
        stalled: 0,
        done: cell.done,
    };
    let mut above = Cell {
        lo,
        hi: cell.hi,
        clipped: above,
        whole: whole_above,
        winding,
        stalled: 0,
        done: cell.done,
    };
    for half in [&mut below, &mut above] {
        if half.stalls(cell) {
            half.stalled = cell.stalled + 1;
        }
    }
    (below, above)
}

/// Whether `keep` is the same whatever the memberships of the inputs with polygons in `cell`,
/// the others' fixed by the cell's winding numbers: then nothing in the cell is a face. Only
/// decided for a few such inputs (2⁸ cases); otherwise `false`.
fn settled(cell: &Cell, all: &[InputTriangle], keep: &(dyn Fn(&[bool]) -> bool + Sync)) -> bool {
    let mut present: Vec<usize> = cell.polygons(all).map(|p| p.input as usize).collect();
    present.sort_unstable();
    present.dedup();
    if present.len() > 8 {
        return false;
    }
    let mut membership: Vec<bool> = cell.winding.iter().map(|&w| w > 0).collect();
    let mut first = None;
    for case in 0..1_u32 << present.len() {
        for (bit, &input) in present.iter().enumerate() {
            membership[input] = case >> bit & 1 == 1;
        }
        let value = keep(&membership);
        match first {
            None => first = Some(value),
            Some(v) if v != value => return false,
            Some(_) => {}
        }
    }
    true
}

/// Whether the two supports are the same plane, whichever their sides.
fn coplanar(a: &Polygon, b: &Polygon) -> bool {
    a.support.parallel(&b.support) && b.support.side(&a.vertices[0]) == Ordering::Equal
}

/// The parts of touched polygons that no other touched polygon crosses.
fn parts(polygons: &[Polygon]) -> Vec<Polygon> {
    let mut out = Vec::new();
    for (i, polygon) in polygons.iter().enumerate() {
        let mut pieces = vec![polygon.clone()];
        for (j, other) in polygons.iter().enumerate() {
            if i == j {
                continue;
            }
            let planes: Vec<Plane> = if coplanar(polygon, other) {
                other.edges.clone()
            } else if polygon.support.parallel(&other.support) {
                continue;
            } else {
                // `other` may cross `polygon` only if it is not strictly on one side of its
                // support; the cut is wasted when it misses, never wrong.
                let sides = other.sides(&polygon.support);
                if sides.iter().all(|&s| s == Ordering::Greater)
                    || sides.iter().all(|&s| s == Ordering::Less)
                {
                    continue;
                }
                vec![other.support]
            };
            for plane in &planes {
                let mut next = Vec::with_capacity(pieces.len() + 1);
                for piece in pieces {
                    if piece.straddles(plane) {
                        let split = piece.split(plane);
                        next.extend(split.positive);
                        next.extend(split.negative);
                    } else {
                        next.push(piece);
                    }
                }
                pieces = next;
            }
        }
        out.extend(pieces);
    }
    out
}

/// A point strictly inside `polygon`: the centroid of its first vertex and two others not
/// collinear with it.
fn inside(polygon: &Polygon) -> Wide {
    let v = &polygon.vertices;
    for k in 2..v.len() {
        if !collinear(polygon, 0, k - 1, k) {
            return Wide::centroid(&v[0], &v[k - 1], &v[k]);
        }
    }
    unreachable!("a polygon of the arrangement has area")
}

/// Whether vertices `a`, `b`, `c` of `polygon` are collinear, decided in the projection along
/// the axis its normal leans on most (where the projection keeps the plane's lines).
fn collinear(polygon: &Polygon, a: usize, b: usize, c: usize) -> bool {
    use super::int::Int;
    let axis = (0..3)
        .max_by_key(|&k| polygon.support.n[k].unsigned_abs())
        .unwrap_or(0);
    let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
    let points = [a, b, c].map(|i| polygon.vertices[i].homogeneous());
    // (x1 w0 − x0 w1)(y2 w0 − y0 w2) − (y1 w0 − y0 w1)(x2 w0 − x0 w2), the orientation times
    // the positive w0² w1 w2.
    let difference = |i: usize, k: usize| -> Int<7> {
        let (ci, wi) = (&points[i].0, &points[i].1);
        let (c0, w0) = (&points[0].0, &points[0].1);
        Int::<7>::product(&ci[k], w0).sub(&Int::product(&c0[k], wi))
    };
    let left: Int<14> = Int::product(&difference(1, u), &difference(2, v));
    let right: Int<14> = Int::product(&difference(1, v), &difference(2, u));
    left == right
}

/// Whether the perturbed point lies inside the cell.
fn owns(cell: &Cell, point: &Wide) -> bool {
    (0..3).all(|k| {
        point.perturbed_side(&wall(k, cell.lo[k], true)) == Ordering::Greater
            && point.perturbed_side(&wall(k, cell.hi[k], false)) == Ordering::Greater
    })
}

/// Whether the polygon on `support` is a face, and if so whether it faces out of the result
/// against its support (`Some(true)`): its winding numbers on both sides from a walk from the
/// cell's corner to `centre`, then across every part in `cover` (the polygon included).
fn classify(
    cell: &Cell,
    all: &[InputTriangle],
    support: &Plane,
    centre: &Wide,
    cover: &[Sheet<'_>],
    keep: &(dyn Fn(&[bool]) -> bool + Sync),
) -> Option<bool> {
    let mut winding = cell.winding.clone();
    walk(&cell.polygons(all), cell.lo, centre, &mut winding);
    // The perturbed centre lies on the side of its plane given by the first nonzero normal
    // component. Across the plane, each covering part is crossed once.
    let front = centre.perturbed_side(support) == Ordering::Greater;
    let mut other_side = winding.clone();
    for covering in cover {
        // Same orientation as `polygon`: crossing from front to back raises its input's number.
        let same = covering
            .support
            .n
            .iter()
            .zip(support.n)
            .any(|(a, b)| *a != 0 && (*a > 0) == (b > 0));
        other_side[covering.input as usize] += if same == front { 1 } else { -1 };
    }
    let (in_front, behind) = if front {
        (winding, other_side)
    } else {
        (other_side, winding)
    };
    let member = |w: &[i64]| keep(&w.iter().map(|&v| v > 0).collect::<Vec<bool>>());
    let (inside_front, inside_behind) = (member(&in_front), member(&behind));
    if inside_front == inside_behind {
        return None;
    }
    Some(inside_front)
}

/// Cut the touched polygons of `cell` into parts and keep those between `keep` and not `keep`.
fn part_faces(
    cell: &Cell,
    all: &[InputTriangle],
    keep: &(dyn Fn(&[bool]) -> bool + Sync),
    faces: &mut Vec<Face>,
) {
    let parts = parts(&cell.clipped);
    for (index, part) in parts.iter().enumerate() {
        let centre = inside(part);
        // The parts covering the centre on the same plane, this one included.
        let mut cover = vec![part.sheet()];
        let mut first = true;
        for (j, other) in parts.iter().enumerate() {
            if j == index || !coplanar(part, other) {
                continue;
            }
            if other
                .edges
                .iter()
                .all(|edge| centre.side(edge) == Ordering::Greater)
            {
                cover.push(other.sheet());
                first &= j > index;
            }
        }
        // One face for parts covering the same region: the first of them makes it.
        if !first {
            continue;
        }
        faces.extend(
            classify(cell, all, &part.support, &centre, &cover, keep).map(|flipped| {
                let mut face = part.clone();
                if flipped {
                    face.support = face.support.flipped();
                }
                Face::Part(Box::new(face))
            }),
        );
    }
}

/// Keep or drop whole the untouched triangles whose centroid `cell` holds.
fn whole_faces(
    cell: &Cell,
    all: &[InputTriangle],
    keep: &(dyn Fn(&[bool]) -> bool + Sync),
    faces: &mut Vec<Face>,
) {
    for &i in &cell.whole {
        let triangle = &all[i as usize];
        let v = triangle.vertices();
        let centre = Wide::centroid(&v[0], &v[1], &v[2]);
        if owns(cell, &centre) {
            faces.extend(
                classify(
                    cell,
                    all,
                    &triangle.support,
                    &centre,
                    &[triangle.sheet()],
                    keep,
                )
                .map(|flipped| Face::Whole { index: i, flipped }),
            );
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::{arrange, Mesh};
    use crate::boolean::{
        grid::GridPoint,
        int::tests::Rng,
        polygon::Polygon,
        trace::{tests::cube, walk, Wide},
    };
    use crate::cancellation::Cancellation;

    /// A triangle of grid points.
    pub(crate) type Triangle = [GridPoint; 3];

    /// The twelve outward triangles of the box `[lo, hi]`.
    pub(crate) fn box_triangles(lo: [i64; 3], hi: [i64; 3]) -> Vec<Triangle> {
        let p = |i: usize| GridPoint {
            x: if i & 1 == 0 { lo[0] } else { hi[0] },
            y: if i & 2 == 0 { lo[1] } else { hi[1] },
            z: if i & 4 == 0 { lo[2] } else { hi[2] },
        };
        let faces = [
            [0, 2, 3, 1],
            [4, 5, 7, 6],
            [0, 1, 5, 4],
            [2, 6, 7, 3],
            [0, 4, 6, 2],
            [1, 3, 7, 5],
        ];
        faces
            .iter()
            .flat_map(|f| [[p(f[0]), p(f[1]), p(f[2])], [p(f[0]), p(f[2]), p(f[3])]])
            .collect()
    }

    /// The outward triangles of a tetrahedron, whatever the order of its corners.
    pub(crate) fn tetrahedron(c: [GridPoint; 4]) -> Vec<Triangle> {
        let volume = {
            let d = |a: GridPoint, b: GridPoint| [b.x - a.x, b.y - a.y, b.z - a.z].map(i128::from);
            let (u, v, w) = (d(c[0], c[1]), d(c[0], c[2]), d(c[0], c[3]));
            u[0] * (v[1] * w[2] - v[2] * w[1]) - u[1] * (v[0] * w[2] - v[2] * w[0])
                + u[2] * (v[0] * w[1] - v[1] * w[0])
        };
        let [a, b, cc, d] = if volume > 0 {
            c
        } else {
            [c[1], c[0], c[2], c[3]]
        };
        vec![[a, cc, b], [a, b, d], [b, cc, d], [cc, a, d]]
    }

    /// An independent oracle: the winding number of `triangles` at `q / 8` by an upward vertical
    /// ray, in plain integers; `None` when the ray meets an edge or the point lies on a plane.
    pub(crate) fn oracle(triangles: &[Triangle], q: [i64; 3]) -> Option<i64> {
        let mut winding = 0;
        for t in triangles {
            let p = t.map(|v| [v.x, v.y, v.z].map(|c| i128::from(c) * 8));
            let q = q.map(i128::from);
            let orient = |a: [i128; 3], b: [i128; 3]| {
                (a[0] - q[0]) * (b[1] - q[1]) - (a[1] - q[1]) * (b[0] - q[0])
            };
            // A triangle with no area seen from above (vertical, or flat) is never crossed by
            // the vertical ray, but on its own plane.
            let area = (p[1][0] - p[0][0]) * (p[2][1] - p[0][1])
                - (p[1][1] - p[0][1]) * (p[2][0] - p[0][0]);
            if area == 0 {
                continue;
            }
            let o = [orient(p[0], p[1]), orient(p[1], p[2]), orient(p[2], p[0])];
            if o.contains(&0) {
                // On an edge's vertical plane: only a problem when the projection is inside.
                if o.iter().all(|&v| v >= 0) || o.iter().all(|&v| v <= 0) {
                    return None;
                }
                continue;
            }
            if !(o.iter().all(|&v| v > 0) || o.iter().all(|&v| v < 0)) {
                continue;
            }
            let u = [0, 1, 2].map(|k| p[1][k] - p[0][k]);
            let v = [0, 1, 2].map(|k| p[2][k] - p[0][k]);
            let n = [
                u[1] * v[2] - u[2] * v[1],
                u[2] * v[0] - u[0] * v[2],
                u[0] * v[1] - u[1] * v[0],
            ];
            let height = (0..3).map(|k| n[k] * (q[k] - p[0][k])).sum::<i128>();
            if height == 0 {
                return None;
            }
            // Below the triangle: the ray crosses it.
            if (height > 0) != (n[2] > 0) {
                winding += if n[2] > 0 { 1 } else { -1 };
            }
        }
        Some(winding)
    }

    /// Triangles as an input mesh, each with its own three vertices (welded by the arrangement).
    pub(crate) fn mesh(triangles: &[Triangle]) -> Mesh {
        Mesh {
            vertices: triangles.iter().flatten().copied().collect(),
            triangles: (0..triangles.len())
                .map(|i| {
                    let i = u32::try_from(3 * i).unwrap();
                    [i, i + 1, i + 2]
                })
                .collect(),
        }
    }

    /// The faces of `keep` of `inputs` bound exactly the region `keep` says, at `samples` random
    /// points of the box `[-span, span]` (eighths of a step): the faces' winding number there is
    /// 1 inside the result and 0 outside, the oracle's memberships deciding which.
    fn check(
        inputs: &[Vec<Triangle>],
        keep: &(dyn Fn(&[bool]) -> bool + Sync),
        span: i64,
        samples: usize,
        seed: u64,
    ) -> usize {
        let meshes: Vec<Mesh> = inputs.iter().map(|t| mesh(t)).collect();
        let faces = arrange(&meshes, keep, &mut Cancellation::never()).unwrap();
        let mut rng = Rng::new(seed);
        let outside = [-(span * 2 + 1001); 3];
        let mut checked = 0;
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
            let mut winding = [0_i64; 1];
            let tagged: Vec<Polygon> = faces
                .faces
                .iter()
                .map(|f| Polygon {
                    input: 0,
                    ..faces.polygon(f)
                })
                .collect();
            walk(
                &tagged.iter().map(Polygon::sheet),
                [outside[0] | 1, outside[1] | 1, outside[2] | 1],
                &Wide::rational(q, 8),
                &mut winding,
            );
            let expected = i64::from(keep(&memberships));
            assert_eq!(
                winding[0], expected,
                "at {q:?} (eighths), memberships {memberships:?}"
            );
            checked += 1;
        }
        faces.faces.len()
    }

    pub(crate) fn union(m: &[bool]) -> bool {
        m.iter().any(|&b| b)
    }
    pub(crate) fn intersection(m: &[bool]) -> bool {
        m.iter().all(|&b| b)
    }
    pub(crate) fn difference(m: &[bool]) -> bool {
        m[0] && !m[1..].iter().any(|&b| b)
    }

    #[test]
    fn the_integer_contact_tests_agree_with_the_general_ones() {
        // Random coplanar pairs on a tilted plane and random pairs in space, sharing 0 to 3
        // vertices: the quick tests decide as `improper_contact` (one input) and
        // `triangles_meet` (two inputs) do.
        use super::{coplanar_contact, separated};
        use crate::preparation::sampled::contact::{improper_contact, triangles_meet};
        let mut rng = Rng::new(31);
        let mut decided = [0_usize; 2];
        for round in 0..40_000 {
            let coplanar = round % 2 == 0;
            let mut point = || {
                let mut c = || i64::try_from(rng.next() % 9).unwrap() - 4;
                let (u, v) = (c(), c());
                // On the plane z = 2x + 3y, or anywhere.
                let z = if coplanar { 2 * u + 3 * v } else { c() };
                GridPoint { x: u, y: v, z }
            };
            let pool: Vec<GridPoint> = (0..6).map(|_| point()).collect();
            let pick = |rng: &mut Rng| u32::try_from(rng.next() % 6).unwrap();
            let fa = [pick(&mut rng), pick(&mut rng), pick(&mut rng)];
            let fb = [pick(&mut rng), pick(&mut rng), pick(&mut rng)];
            let corners = |f: [u32; 3]| f.map(|i| pool[i as usize]);
            let (ca, cb) = (corners(fa), corners(fb));
            if Polygon::triangle(ca[0], ca[1], ca[2], 0).is_none()
                || Polygon::triangle(cb[0], cb[1], cb[2], 0).is_none()
                || fa.iter().collect::<std::collections::BTreeSet<_>>().len() < 3
                || fb.iter().collect::<std::collections::BTreeSet<_>>().len() < 3
            {
                continue;
            }
            let mm = |c: [GridPoint; 3]| c.map(|v| v.point().coordinates());
            for own in [true, false] {
                let a = (0, fa, ca);
                let b = (u32::from(!own), fb, cb);
                let expected = if own {
                    improper_contact(fa, mm(ca), fb, mm(cb))
                } else {
                    triangles_meet(mm(ca), mm(cb))
                };
                if separated(&a, &b) {
                    assert!(!expected, "separated but meeting: {a:?} {b:?}");
                    decided[0] += 1;
                } else if let Some(meet) = coplanar_contact(&a, &b) {
                    assert_eq!(meet, expected, "{a:?} {b:?} own {own}");
                    decided[1] += 1;
                }
            }
        }
        assert!(decided[0] > 5_000 && decided[1] > 5_000, "{decided:?}");
    }

    #[test]
    fn the_faces_are_the_same_however_the_work_is_shared() {
        // Finishing from the first cell alone, or from thousands, gives the same faces in the
        // same order: what the threads do never changes the result.
        let mut shell = box_triangles([0, 0, 0], [30, 30, 30]);
        shell.extend(
            box_triangles([3, 3, 3], [27, 27, 27])
                .into_iter()
                .map(|[a, b, c]| [a, c, b]),
        );
        let inputs = [
            mesh(&shell),
            mesh(&box_triangles([12, 13, -5], [17, 18, 10])),
        ];
        let faces = |target| {
            super::arrange_with(&inputs, &difference, &mut Cancellation::never(), target).unwrap()
        };
        let (one, many) = (faces(1), faces(4_096));
        assert!(!one.faces.is_empty());
        assert_eq!(one.faces.len(), many.faces.len());
        for (a, b) in one
            .faces
            .iter()
            .map(|f| one.polygon(f))
            .zip(many.faces.iter().map(|f| many.polygon(f)))
        {
            assert_eq!(a.support, b.support);
            assert_eq!(a.edges, b.edges);
            assert!(a.vertices.iter().zip(&b.vertices).all(|(p, q)| p.same(q)));
        }
    }

    #[test]
    fn two_overlapping_boxes_union_intersect_and_subtract() {
        let inputs = [
            box_triangles([0, 0, 0], [10, 10, 10]),
            box_triangles([4, 3, 5], [17, 12, 14]),
        ];
        for keep in [union as fn(&[bool]) -> bool, intersection, difference] {
            check(&inputs, &keep, 20, 400, 1);
        }
    }

    #[test]
    fn boxes_sharing_faces_and_identical_boxes_give_one_surface() {
        // Touching along a face: the union has no wall between them.
        let touching = [
            box_triangles([0, 0, 0], [10, 10, 10]),
            box_triangles([10, 0, 0], [20, 10, 10]),
        ];
        let faces = check(&touching, &union, 22, 400, 2);
        assert!(faces >= 10, "{faces}");
        // The same box twice: one surface, its winding number 1, not 2.
        let twice = [
            box_triangles([0, 0, 0], [10, 10, 10]),
            box_triangles([0, 0, 0], [10, 10, 10]),
        ];
        check(&twice, &union, 14, 300, 3);
        check(&twice, &intersection, 14, 300, 4);
        check(&twice, &difference, 14, 300, 5);
        // Coplanar faces partly overlapping, and a box inside another touching its floor.
        let stepped = [
            box_triangles([0, 0, 0], [10, 10, 10]),
            box_triangles([5, 5, 0], [15, 15, 6]),
        ];
        for keep in [union as fn(&[bool]) -> bool, intersection, difference] {
            check(&stepped, &keep, 18, 300, 6);
        }
    }

    #[test]
    fn an_input_overlapping_itself_counts_once() {
        // Two overlapping boxes as one input: its own union.
        let mut overlapping = box_triangles([0, 0, 0], [10, 10, 10]);
        overlapping.extend(box_triangles([5, 5, 5], [15, 15, 15]));
        check(&[overlapping], &|m: &[bool]| m[0], 18, 400, 7);
    }

    #[test]
    fn random_tetrahedra_combine_exactly() {
        let mut rng = Rng::new(8);
        let mut g = || {
            let mut c = || i64::try_from(rng.next() % 41).unwrap() - 20;
            GridPoint {
                x: c(),
                y: c(),
                z: c(),
            }
        };
        for round in 0..40 {
            let inputs: Vec<Vec<Triangle>> =
                (0..3).map(|_| tetrahedron([g(), g(), g(), g()])).collect();
            let keep: fn(&[bool]) -> bool = [union, intersection, difference][round % 3];
            check(&inputs, &keep, 22, 60, 100 + round as u64);
        }
    }

    /// The box `[lo, hi]` with each face cut into `n × n` squares of two triangles, outward.
    pub(crate) fn fine_box(lo: [i64; 3], hi: [i64; 3], n: i64) -> Vec<Triangle> {
        let mut out = Vec::new();
        // Each face as (fixed axis, its value, outward sign, the two other axes).
        for (axis, value, outward) in [
            (0, lo[0], -1),
            (0, hi[0], 1),
            (1, lo[1], -1),
            (1, hi[1], 1),
            (2, lo[2], -1),
            (2, hi[2], 1),
        ] {
            let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
            let at = |i: i64, j: i64| {
                let mut c = [0; 3];
                c[axis] = value;
                c[u] = lo[u] + (hi[u] - lo[u]) * i / n;
                c[v] = lo[v] + (hi[v] - lo[v]) * j / n;
                GridPoint {
                    x: c[0],
                    y: c[1],
                    z: c[2],
                }
            };
            for i in 0..n {
                for j in 0..n {
                    let (a, b, c, d) = (at(i, j), at(i + 1, j), at(i + 1, j + 1), at(i, j + 1));
                    // (u, v, axis) is right-handed: counterclockwise in (u, v) faces +axis.
                    if outward > 0 {
                        out.push([a, b, c]);
                        out.push([a, c, d]);
                    } else {
                        out.push([a, c, b]);
                        out.push([a, d, c]);
                    }
                }
            }
        }
        out
    }

    #[test]
    #[ignore = "a measurement: cargo test --release -p karasu-core boolean::arrange -- --ignored --nocapture"]
    fn a_fine_shell_drilled_by_four_holes() {
        let n = 150;
        let mut shell = fine_box([0, 0, 0], [300_000, 300_000, 300_000], n);
        let inner: Vec<Triangle> =
            fine_box([12_000, 12_000, 12_000], [288_000, 288_000, 288_000], n)
                .into_iter()
                .map(|[a, b, c]| [a, c, b])
                .collect();
        shell.extend(inner);
        let mut holes = Vec::new();
        for k in 0..4 {
            let x = 40_000 + 60_000 * k;
            holes.extend(box_triangles(
                [x, 140_000, -5_000],
                [x + 8_192, 148_192, 20_000],
            ));
        }
        let triangles = shell.len() + holes.len();
        let inputs = vec![mesh(&shell), mesh(&holes)];
        let started = std::time::Instant::now();
        let faces = arrange(&inputs, &difference, &mut Cancellation::never()).unwrap();
        println!(
            "{triangles} triangles: {} faces in {:.2} s",
            faces.faces.len(),
            started.elapsed().as_secs_f64()
        );
    }

    #[test]
    fn a_hollow_box_drilled_through_its_wall() {
        // A shell (outer box and reversed inner box) minus a square prism from outside into the
        // cavity: the drain hole's shape of TD-139.
        let mut shell = box_triangles([0, 0, 0], [30, 30, 30]);
        let inner: Vec<Triangle> = box_triangles([3, 3, 3], [27, 27, 27])
            .into_iter()
            .map(|[a, b, c]| [a, c, b])
            .collect();
        shell.extend(inner);
        let prism = box_triangles([12, 13, -5], [17, 18, 10]);
        let faces = check(&[shell, prism], &difference, 40, 500, 9);
        assert!(faces > 24, "{faces}");
        let _ = cube;
    }
}
