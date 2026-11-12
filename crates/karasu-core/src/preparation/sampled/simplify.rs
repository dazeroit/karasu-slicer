//! Certified simplification of a sampled cavity surface (TD-066).
//!
//! **Operation:** deterministic half-edge collapses: a vertex `u` merges into a neighbour `v`, so
//! every surviving vertex is an original iso-surface vertex. A collapse replaces the star of `u`
//! (its incident faces) by a fan around `v` over the same boundary loop.
//!
//! **What every accepted collapse proves:**
//! - **Topology:** the link condition holds (the two vertices opposite edge `uv` are their only
//!   common neighbours), so the surface stays a closed edge-manifold of the same topology.
//! - **Deviation:** both the old star and the new fan project injectively onto one plane (every
//!   face has an interval-proven positive orientation along the star's mean normal `n`). They are
//!   then two piecewise-linear height functions over the same polygon. Their difference at any
//!   point is at most the spread of the star's vertex heights along `n`. Each face carries the
//!   bound `eps` on its distance from the original surface, and a new face gets
//!   `eps = spread + max eps of the star`. Both directions are covered: every point of the fan
//!   is within `eps` of the star and vice versa, and so within `eps` of the original surface.
//!   Only collapses with `eps <= tolerance` are accepted.
//! - **Wall floor:** every new face is proven exactly ([`Bvh::clearance`]) to be farther than
//!   `floor` from the source surface. The cavity therefore never comes closer to the outside
//!   than the floor, and stays inside the solid, because its vertices are interior and no face
//!   meets the source.
//! - **Embedding:** no new face meets any other face of the cavity except where their shared
//!   vertices allow (exact [`improper_contact`]).
//!
//! **Determinism:** rounds evaluate candidates in parallel into per-vertex slots, then select an
//! independent set (stars two rings apart) in `(eps, u)` order, prove each collapse's wall floor
//! in parallel, and apply them sequentially in the same order with the embedding check. Results
//! are identical for every thread count.
use super::{
    bvh::{Bvh, Clearance},
    contact::improper_contact,
    distance::{interval_dot, Triangle},
    resource, uncertain,
};
use crate::{
    cancellation::{Cancellation, Checkpoint},
    geometry::Point3,
    mesh::TriangleMesh,
    preparation::hollow::{numeric::I, HollowError},
};
use std::collections::HashMap;

/// Rounds after which simplification stops even if collapses remain.
const MAX_ROUNDS: u32 = 64;

/// Longest new edge, in grid cells (four mean original edges each): faces stay local enough for
/// the embedding check, while flat walls still drop most of their triangles.
const MAX_EDGE_CELLS: f64 = 8.0;

/// A simplified cavity and its per-face deviation certificate.
pub(super) struct Simplified {
    /// The simplified cavity, outward from the void, with original vertex coordinates.
    pub(super) cavity: TriangleMesh,
    /// Certified bound on each face's distance from the original cavity surface; zero for
    /// original faces, which also keep their original vertex indices' meaning below.
    pub(super) deviation: Vec<f64>,
    /// Original vertex index of each simplified vertex.
    pub(super) original_vertex: Vec<u32>,
}

/// Simplification request.
#[derive(Clone, Copy)]
pub(super) struct Request {
    /// Largest accepted deviation from the original cavity surface, in mm.
    pub(super) tolerance: f64,
    /// Every new face must be proven farther than this from the source surface, in mm.
    pub(super) floor: f64,
    /// Worker threads.
    pub(super) threads: usize,
}

/// One proposed collapse of `u` into `v`.
struct Candidate {
    u: u32,
    eps: f64,
    fan: Vec<[u32; 3]>,
    /// Neighbours of `u`, sorted.
    ring: Vec<u32>,
}

struct State {
    points: Vec<[f64; 3]>,
    faces: Vec<[u32; 3]>,
    alive: Vec<bool>,
    eps: Vec<f64>,
    /// Alive faces incident to each vertex, unordered.
    incident: Vec<Vec<u32>>,
    /// Round in which each vertex's star last changed.
    changed: Vec<u32>,
    /// Round in which each vertex last failed to collapse (`u32::MAX`: never).
    failed: Vec<u32>,
    /// Vertices whose star changed in the current round.
    touched: Vec<u32>,
    grid: Grid,
    /// Longest edge a new face may have (keeps embedding checks local).
    max_edge_sq: f64,
}

/// Uniform hash grid of face boxes for the embedding check.
struct Grid {
    cell: f64,
    cells: HashMap<[i64; 3], Vec<u32>>,
}

impl Grid {
    fn range(&self, corners: &[[f64; 3]; 3]) -> Option<[[i64; 2]; 3]> {
        let mut range = [[0i64; 2]; 3];
        for k in 0..3 {
            let lo = corners.iter().map(|p| p[k]).fold(f64::INFINITY, f64::min);
            let hi = corners
                .iter()
                .map(|p| p[k])
                .fold(f64::NEG_INFINITY, f64::max);
            let (lo, hi) = ((lo / self.cell).floor(), (hi / self.cell).floor());
            if !(lo.is_finite() && hi.is_finite()) || hi - lo > 1e6 {
                return None;
            }
            #[allow(clippy::cast_possible_truncation)] // Bounded by the coordinate admission.
            {
                range[k] = [lo as i64, hi as i64];
            }
        }
        Some(range)
    }

    fn insert(&mut self, face: u32, corners: &[[f64; 3]; 3]) -> Result<(), HollowError> {
        let range = self
            .range(corners)
            .ok_or_else(|| resource("cavity simplification grid"))?;
        for x in range[0][0]..=range[0][1] {
            for y in range[1][0]..=range[1][1] {
                for z in range[2][0]..=range[2][1] {
                    self.cells.entry([x, y, z]).or_default().push(face);
                }
            }
        }
        Ok(())
    }

    /// Whether `test` holds for any face sharing a cell with the corners' box (faces in several
    /// cells may be tested more than once). `None` when the box is too large for the grid.
    fn any(&self, corners: &[[f64; 3]; 3], mut test: impl FnMut(u32) -> bool) -> Option<bool> {
        let range = self.range(corners)?;
        for x in range[0][0]..=range[0][1] {
            for y in range[1][0]..=range[1][1] {
                for z in range[2][0]..=range[2][1] {
                    if let Some(faces) = self.cells.get(&[x, y, z]) {
                        if faces.iter().any(|&f| test(f)) {
                            return Some(true);
                        }
                    }
                }
            }
        }
        Some(false)
    }
}

fn boxes_overlap(a: &[[f64; 3]; 3], b: &[[f64; 3]; 3]) -> bool {
    (0..3).all(|k| {
        let (alo, ahi) = (
            a[0][k].min(a[1][k]).min(a[2][k]),
            a[0][k].max(a[1][k]).max(a[2][k]),
        );
        let (blo, bhi) = (
            b[0][k].min(b[1][k]).min(b[2][k]),
            b[0][k].max(b[1][k]).max(b[2][k]),
        );
        alo <= bhi && blo <= ahi
    })
}

fn interval(p: [f64; 3]) -> [I; 3] {
    p.map(I::point)
}

/// Interval of `((b - a) x (c - a)) . n`.
fn orientation(a: [f64; 3], b: [f64; 3], c: [f64; 3], n: [f64; 3]) -> I {
    let (a, b, c) = (interval(a), interval(b), interval(c));
    let ab = [0, 1, 2].map(|k| b[k].sub(a[k]));
    let ac = [0, 1, 2].map(|k| c[k].sub(a[k]));
    let cross = [
        ab[1].mul(ac[2]).sub(ab[2].mul(ac[1])),
        ab[2].mul(ac[0]).sub(ab[0].mul(ac[2])),
        ab[0].mul(ac[1]).sub(ab[1].mul(ac[0])),
    ];
    cross[0]
        .mul(I::point(n[0]))
        .add(cross[1].mul(I::point(n[1])))
        .add(cross[2].mul(I::point(n[2])))
}

impl State {
    fn corners(&self, face: [u32; 3]) -> [[f64; 3]; 3] {
        face.map(|v| self.points[v as usize])
    }

    /// Sorted distinct neighbours of `u`.
    fn ring(&self, u: u32) -> Vec<u32> {
        let mut ring: Vec<u32> = self.incident[u as usize]
            .iter()
            .flat_map(|&f| self.faces[f as usize])
            .filter(|&w| w != u)
            .collect();
        ring.sort_unstable();
        ring.dedup();
        ring
    }

    /// The best collapse of `u`, or `None` if no neighbour gives a valid one within `tolerance`.
    fn evaluate(&self, u: u32, tolerance: f64) -> Option<Candidate> {
        let (normal, eps) = self.star_frame(u, tolerance)?;
        let ring = self.ring(u);
        let fan = self.best_fan(u, &ring, normal)?;
        Some(Candidate { u, eps, fan, ring })
    }

    /// The star's mean unit normal and the deviation bound of collapsing it, if the star
    /// projects injectively along that normal and the bound is within `tolerance`.
    #[allow(clippy::many_single_char_names)] // Corners a, b, c; normal n; vertex w.
    fn star_frame(&self, u: u32, tolerance: f64) -> Option<([f64; 3], f64)> {
        let star = &self.incident[u as usize];
        if star.len() < 3 {
            return None;
        }
        // Mean normal of the star (area weighted, computed); any direction is valid for the
        // proof, which only needs the interval-proven orientations below.
        let mut normal = [0.0; 3];
        for &f in star {
            let [a, b, c] = self.corners(self.faces[f as usize]);
            let (ab, ac) = (
                [b[0] - a[0], b[1] - a[1], b[2] - a[2]],
                [c[0] - a[0], c[1] - a[1], c[2] - a[2]],
            );
            normal[0] += ab[1] * ac[2] - ab[2] * ac[1];
            normal[1] += ab[2] * ac[0] - ab[0] * ac[2];
            normal[2] += ab[0] * ac[1] - ab[1] * ac[0];
        }
        let norm = normal.iter().map(|c| c * c).sum::<f64>().sqrt();
        if !(norm.is_finite() && norm > 0.0) {
            return None;
        }
        let n = normal.map(|c| c / norm);
        let (mut low, mut high) = (f64::INFINITY, f64::NEG_INFINITY);
        for &f in star {
            let face = self.faces[f as usize];
            let [a, b, c] = self.corners(face);
            if orientation(a, b, c, n).lo <= 0.0 {
                return None;
            }
            for w in face {
                let h = interval_dot(self.points[w as usize], n);
                low = low.min(h.lo);
                high = high.max(h.hi);
            }
        }
        let base = star
            .iter()
            .map(|&f| self.eps[f as usize])
            .fold(0.0, f64::max);
        let eps = I::point(high).sub(I::point(low)).add(I::point(base)).hi;
        (eps.is_finite() && eps <= tolerance).then_some((n, eps))
    }

    /// The fan of the valid collapse of `u` into its nearest possible neighbour (ties by
    /// index): link condition, edge length and interval-proven orientation along `n`.
    #[allow(clippy::many_single_char_names)] // Vertices u, v, w; corners a, b, c; normal n.
    fn best_fan(&self, u: u32, ring: &[u32], n: [f64; 3]) -> Option<Vec<[u32; 3]>> {
        let star = &self.incident[u as usize];
        let length_sq = |a: [f64; 3], b: [f64; 3]| (0..3).map(|k| (a[k] - b[k]).powi(2)).sum();
        let origin = self.points[u as usize];
        let mut order: Vec<(f64, u32)> = ring
            .iter()
            .map(|&v| (length_sq(origin, self.points[v as usize]), v))
            .collect();
        order.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        let faces = |f: &u32| self.faces[*f as usize];
        for (_, v) in order {
            // Link condition: exactly the two vertices opposite edge uv are common neighbours.
            let mut opposite = [u32::MAX; 3];
            let mut sharing = 0;
            for face in star.iter().map(faces).filter(|face| face.contains(&v)) {
                opposite[sharing.min(2)] = face
                    .into_iter()
                    .find(|&w| w != u && w != v)
                    .unwrap_or(u32::MAX);
                sharing += 1;
            }
            if sharing != 2 || opposite[0] == opposite[1] {
                continue;
            }
            // On a closed manifold every neighbour of v appears in exactly two of its faces.
            let common = self.incident[v as usize]
                .iter()
                .map(faces)
                .flat_map(IntoIterator::into_iter)
                .filter(|&w| w != v && w != u && ring.binary_search(&w).is_ok())
                .count();
            if common != 4 {
                continue;
            }
            let moved = |face: [u32; 3]| face.map(|w| if w == u { v } else { w });
            let fan = star
                .iter()
                .map(faces)
                .filter(|face| !face.contains(&v))
                .map(moved);
            let valid = fan.clone().all(|face| {
                let [a, b, c] = self.corners(face);
                let short = [(a, b), (b, c), (c, a)]
                    .iter()
                    .all(|&(p, q)| length_sq(p, q) <= self.max_edge_sq);
                short && orientation(a, b, c, n).lo > 0.0
            });
            if valid {
                return Some(fan.collect());
            }
        }
        None
    }

    /// Whether every face of `fan` is properly embedded among the alive faces with index at
    /// least `since` outside the star of `u`, and, with `internal`, among themselves.
    fn embedded(&self, candidate: &Candidate, since: usize, internal: bool) -> Option<bool> {
        let star = &self.incident[candidate.u as usize];
        for (i, &face) in candidate.fan.iter().enumerate() {
            let corners = self.corners(face);
            let contact = self.grid.any(&corners, |other| {
                if (other as usize) < since || !self.alive[other as usize] || star.contains(&other)
                {
                    return false;
                }
                let g = self.faces[other as usize];
                let g_corners = self.corners(g);
                boxes_overlap(&corners, &g_corners) && improper_contact(face, corners, g, g_corners)
            })?;
            if contact {
                return Some(false);
            }
            if internal {
                for &g in &candidate.fan[i + 1..] {
                    if improper_contact(face, corners, g, self.corners(g)) {
                        return Some(false);
                    }
                }
            }
        }
        Some(true)
    }

    fn apply(&mut self, candidate: Candidate, round: u32) -> Result<(), HollowError> {
        let star = std::mem::take(&mut self.incident[candidate.u as usize]);
        let mut touched: Vec<u32> = Vec::new();
        for &f in &star {
            self.alive[f as usize] = false;
            for w in self.faces[f as usize] {
                if w != candidate.u {
                    self.incident[w as usize].retain(|&g| g != f);
                    touched.push(w);
                }
            }
        }
        for face in candidate.fan {
            let index =
                u32::try_from(self.faces.len()).map_err(|_| resource("cavity face index"))?;
            self.faces.push(face);
            self.alive.push(true);
            self.eps.push(candidate.eps);
            for w in face {
                self.incident[w as usize].push(index);
                touched.push(w);
            }
            let corners = self.corners(face);
            self.grid.insert(index, &corners)?;
        }
        for &w in &touched {
            self.changed[w as usize] = round;
        }
        self.touched.extend(touched);
        Ok(())
    }
}

impl State {
    fn new(cavity: &TriangleMesh, cancel: &mut Cancellation<'_>) -> Result<Self, HollowError> {
        let points: Vec<[f64; 3]> = cavity.vertices().iter().map(|p| p.coordinates()).collect();
        let faces = cavity.triangles().to_vec();
        let mut incident = vec![Vec::new(); points.len()];
        // The grid cell is a few typical edges, so most faces fall in one or two cells.
        let mut edges = 0.0;
        for (index, face) in faces.iter().enumerate() {
            cancel.poll(Checkpoint::HollowWork, index)?;
            let index = u32::try_from(index).map_err(|_| resource("cavity face index"))?;
            for &v in face {
                incident[v as usize].push(index);
            }
            let [a, b, _] = face.map(|v| points[v as usize]);
            edges += (0..3).map(|k| (a[k] - b[k]).powi(2)).sum::<f64>().sqrt();
        }
        #[allow(clippy::cast_precision_loss)] // Face counts far below 2^52.
        let mean_edge = edges / faces.len().max(1) as f64;
        let mut state = Self {
            alive: vec![true; faces.len()],
            eps: vec![0.0; faces.len()],
            changed: vec![0; points.len()],
            failed: vec![u32::MAX; points.len()],
            touched: Vec::new(),
            grid: Grid {
                cell: (4.0 * mean_edge).max(1e-3),
                cells: HashMap::new(),
            },
            max_edge_sq: (MAX_EDGE_CELLS * 4.0 * mean_edge).powi(2),
            points,
            faces,
            incident,
        };
        for (index, face) in (0u32..).zip(state.faces.iter()) {
            cancel.poll(Checkpoint::HollowWork, index as usize)?;
            let corners = face.map(|v| state.points[v as usize]);
            state.grid.insert(index, &corners)?;
        }
        Ok(state)
    }

    /// Vertices to evaluate in `round`: all at first, then those whose star or a neighbour's
    /// star changed in the previous round.
    fn dirty(&mut self, round: u32) -> Vec<u32> {
        if round == 1 {
            #[allow(clippy::cast_possible_truncation)] // Vertex indices are u32.
            return (0..self.points.len() as u32).collect();
        }
        let mut touched = std::mem::take(&mut self.touched);
        touched.sort_unstable();
        touched.dedup();
        let mut dirty: Vec<u32> = touched
            .iter()
            .flat_map(|&w| &self.incident[w as usize])
            .flat_map(|&f| self.faces[f as usize])
            .collect();
        dirty.sort_unstable();
        dirty.dedup();
        dirty
    }

    /// Whether a cached candidate is still valid: neither `u` nor a neighbour changed since
    /// round `at` (its star, deviation and link condition depend on nothing else).
    fn current(&self, at: u32, candidate: &Candidate) -> bool {
        self.changed[candidate.u as usize] < at
            && candidate
                .ring
                .iter()
                .all(|&w| self.changed[w as usize] < at)
    }

    /// Refresh the cached candidates of the dirty vertices, in parallel.
    fn evaluate_round(
        &mut self,
        round: u32,
        cache: &mut [Option<(u32, Candidate)>],
        request: Request,
    ) -> Result<(), HollowError> {
        let dirty = self.dirty(round);
        let state = &*self;
        let slots = parallel(dirty.len(), request.threads, |k| {
            let u = dirty[k] as usize;
            if cache[u]
                .as_ref()
                .is_some_and(|(at, c)| state.current(*at, c))
            {
                return None;
            }
            let unchanged_failure =
                state.failed[u] != u32::MAX && state.failed[u] >= state.changed[u];
            #[allow(clippy::cast_possible_truncation)] // Vertex indices are u32.
            Some(
                (!unchanged_failure)
                    .then(|| state.evaluate(u as u32, request.tolerance))
                    .flatten(),
            )
        })
        .ok_or_else(|| uncertain("cavity simplification worker failed"))?;
        for (&u, slot) in dirty.iter().zip(slots) {
            let u = u as usize;
            match slot {
                None => {}
                Some(Some(candidate)) => cache[u] = Some((round, candidate)),
                Some(None) => {
                    cache[u] = None;
                    if !self.incident[u].is_empty() {
                        self.failed[u] = round;
                    }
                }
            }
        }
        Ok(())
    }
}

/// An independent set of cached candidates (stars two rings apart), in `(eps, u)` order; the
/// selected candidates leave the cache.
fn select(cache: &mut [Option<(u32, Candidate)>]) -> Vec<Candidate> {
    let mut order: Vec<(f64, u32)> = cache
        .iter()
        .zip(0u32..)
        .filter_map(|(entry, u)| entry.as_ref().map(|(_, c)| (c.eps, u)))
        .collect();
    order.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    let mut marked = vec![false; cache.len()];
    let mut selected = Vec::new();
    for (_, u) in order {
        let u = u as usize;
        let free = cache[u].as_ref().is_some_and(|(_, candidate)| {
            !marked[u] && candidate.ring.iter().all(|&w| !marked[w as usize])
        });
        if !free {
            continue;
        }
        if let Some((_, candidate)) = cache[u].take() {
            marked[u] = true;
            for &w in &candidate.ring {
                marked[w as usize] = true;
            }
            selected.push(candidate);
        }
    }
    selected
}

/// Simplify `cavity` within `request.tolerance` of itself, keeping every new face farther than
/// `request.floor` from the source indexed by `source`.
///
/// # Errors
/// Resource failures and cancellation; an invalid result is [`HollowError::Uncertain`].
pub(super) fn simplify(
    cavity: &TriangleMesh,
    source: &Bvh,
    request: Request,
    cancel: &mut Cancellation<'_>,
) -> Result<Simplified, HollowError> {
    let mut state = State::new(cavity, cancel)?;
    let mut cache: Vec<Option<(u32, Candidate)>> = (0..state.points.len()).map(|_| None).collect();
    for round in 1..=MAX_ROUNDS {
        cancel.check(Checkpoint::HollowWork)?;
        state.evaluate_round(round, &mut cache, request)?;
        let selected = select(&mut cache);
        if selected.is_empty() {
            break;
        }
        // Embedding against the faces alive at the start of the round, and the wall floor, in
        // parallel; faces created in this round are checked sequentially, in the fixed order.
        let state_ref = &state;
        let proven = parallel(selected.len(), request.threads, |k| {
            let candidate = &selected[k];
            state_ref.embedded(candidate, 0, true) == Some(true)
                && candidate.fan.iter().all(|&face| {
                    source.clearance(
                        &Triangle::new(state_ref.corners(face)),
                        request.floor,
                        &mut Cancellation::never(),
                        Checkpoint::HollowWork,
                    ) == Ok(Clearance::Beyond)
                })
        })
        .ok_or_else(|| uncertain("cavity simplification worker failed"))?;
        let mut applied = 0usize;
        let round_start = state.faces.len();
        for (index, (candidate, proven)) in selected.into_iter().zip(proven).enumerate() {
            cancel.poll(Checkpoint::HollowWork, index)?;
            let u = candidate.u as usize;
            let embedded = proven
                && state
                    .embedded(&candidate, round_start, false)
                    .ok_or_else(|| resource("cavity simplification grid"))?;
            if embedded {
                state.apply(candidate, round)?;
                applied += 1;
            } else {
                state.failed[u] = round;
            }
        }
        if applied == 0 {
            break;
        }
    }
    compact(&state, cancel)
}

/// `work(0..count)` on `threads` workers taking blocks from a shared counter; results in index
/// order, so they do not depend on the thread count. `None` if a worker panicked.
fn parallel<R: Send>(
    count: usize,
    threads: usize,
    work: impl Fn(usize) -> R + Sync,
) -> Option<Vec<R>> {
    const BLOCK: usize = 64;
    let next = std::sync::atomic::AtomicUsize::new(0);
    let mut pairs: Vec<(usize, R)> = Vec::with_capacity(count);
    let joined = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..threads.max(1))
            .map(|_| {
                scope.spawn(|| {
                    let mut local = Vec::new();
                    loop {
                        let start = next.fetch_add(BLOCK, std::sync::atomic::Ordering::Relaxed);
                        if start >= count {
                            break local;
                        }
                        for index in start..(start + BLOCK).min(count) {
                            local.push((index, work(index)));
                        }
                    }
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().ok())
            .collect::<Option<Vec<_>>>()
    })?;
    for part in joined {
        pairs.extend(part);
    }
    pairs.sort_unstable_by_key(|(index, _)| *index);
    Some(pairs.into_iter().map(|(_, r)| r).collect())
}

/// Alive faces in index order over the used vertices in index order.
fn compact(state: &State, cancel: &mut Cancellation<'_>) -> Result<Simplified, HollowError> {
    let mut map = vec![u32::MAX; state.points.len()];
    for (index, face) in state.faces.iter().enumerate() {
        if state.alive[index] {
            for &v in face {
                map[v as usize] = 0;
            }
        }
    }
    let mut vertices = Vec::new();
    let mut original_vertex = Vec::new();
    for (v, slot) in map.iter_mut().enumerate() {
        if *slot == 0 {
            *slot = u32::try_from(vertices.len()).map_err(|_| resource("cavity vertex index"))?;
            let p = state.points[v];
            vertices.push(Point3::new(p[0], p[1], p[2]));
            original_vertex.push(u32::try_from(v).map_err(|_| resource("cavity vertex index"))?);
        }
    }
    let mut triangles = Vec::new();
    let mut deviation = Vec::new();
    for (index, face) in state.faces.iter().enumerate() {
        if state.alive[index] {
            triangles.push(face.map(|v| map[v as usize]));
            deviation.push(state.eps[index]);
        }
    }
    let cavity = TriangleMesh::try_new_with_cancellation(vertices, triangles, cancel)?;
    Ok(Simplified {
        cavity,
        deviation,
        original_vertex,
    })
}
