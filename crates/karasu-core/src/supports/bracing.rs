//! Braces planned between neighbouring supports (the Braces tool's automatic braces, owner
//! mockup 2026-10-29).
//!
//! **Neighbours** (owner, 2026-10-29: a brace joins two supports next to each other, never past a
//! third). Each built support stands at the plan position of its shaft's lowest point. Two
//! supports no farther apart than the plan's distance are a pair only when no third support is
//! nearer to both of them than they are to each other: with A, B and C in a row, A–B and B–C,
//! never A–C; on a grid, the cells' sides, no diagonal; in a triangle, its two shorter sides.
//! These pairs (the relative neighbourhood graph) never cross in plan and keep every other shaft
//! out of the region about the segment. As a guard for exact ties, pairs are also taken nearest
//! first, ties by their IDs, and kept only when their segment crosses no pair kept before. A
//! grid of cells the size of the distance finds the near supports and the kept pairs, so the
//! work follows the supports.
//!
//! **Heights.** Brace levels are the plan's lowest height above the floor (the plate, or the
//! raft's top) and every step above it, the same for every pair. A pair takes the levels both its
//! shafts reach, from a brace's diameter above the higher shaft bottom to a diameter below the
//! lower tip join. A pattern joins them: one level brace per level, one diagonal per step
//! alternating sides, or both diagonals.
//!
//! **Nothing is dropped silently.** A brace a build would find too close to the model or crossing
//! a drain hole, and a brace on a pair the document already braces near that height, is returned
//! with why it was skipped. Kept braces take the IDs after the document's highest. The plan
//! depends only on the document, the surface and the plan, in a fixed order: it is deterministic.
#![allow(clippy::many_single_char_names)] // Geometric notation: points and segments.
use super::{
    build::{channel, shafts, DrainCylinder, SupportSurface},
    document::{Brace, SupportDocument, SupportKind},
    index::{clear_of_tapered, proven_clear},
    shapes::{frustum, sphere, Part, V},
    SupportError,
};
use crate::{
    cancellation::{Cancellation, Checkpoint},
    geometry::Point3,
    mesh::TriangleMesh,
};
use std::collections::{BTreeMap, BTreeSet};

/// The sides of a planned brace, as a build makes them.
const SIDES: u16 = 16;

/// How a pair of neighbouring supports is braced between two levels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BracePattern {
    /// A level brace at each level.
    Horizontal,
    /// One diagonal per step, alternating sides.
    #[default]
    Zigzag,
    /// Both diagonals per step: the stiffest, with twice the braces.
    Cross,
}

/// What braces to plan, in millimetres.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BracePlan {
    /// Supports farther apart than this in plan are not joined.
    pub max_distance_mm: f64,
    /// The lowest level, above the plate or the raft's top.
    pub lowest_mm: f64,
    /// The height between levels.
    pub step_mm: f64,
    /// How a pair is joined between levels.
    pub pattern: BracePattern,
    /// The braces' diameter (0.1–10 mm, as the document accepts); `None` makes automatic
    /// braces, each the thinner of its two shafts where it meets them...
    pub diameter_mm: Option<f64>,
    /// ... up to this diameter (0.1–10 mm; TD-136), `None` for no limit.
    pub max_diameter_mm: Option<f64>,
}

/// Why a planned brace is not kept.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Skipped {
    /// It is not proven the document's clearance away from the model.
    Model,
    /// It enters a drain hole's channel through the model, which it would plug (TD-123). A
    /// support doing so is only warned about (TD-131), as it may hold an island; a brace holds
    /// none, so the plan still leaves it out.
    DrainHole,
    /// The document already braces the pair near that height.
    Joined,
}

/// One brace of a plan.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlannedBrace {
    /// The brace; a skipped one has ID zero.
    pub brace: Brace,
    /// Its ends on the shafts, in world millimetres.
    pub ends_mm: [[f64; 3]; 2],
    /// Its diameter as built: the plan's, or the thinner shaft's at its ends up to the plan's
    /// largest.
    pub diameter_mm: f64,
    /// Why it is not kept, if it is not.
    pub skipped: Option<Skipped>,
}

/// A support that can be braced: its ID, plan position, shaft heights and widest shaft
/// diameter.
struct Standing {
    id: u64,
    xy: [f64; 2],
    span: [f64; 2],
    widest: f64,
}

/// Plan braces between `doc`'s neighbouring supports against `surface` and its `drains`, in
/// document order: see the module documentation. Kept braces carry the IDs to add them with;
/// skipped ones say why.
///
/// # Errors
/// A plan out of range, resource limits and cancellation.
pub fn plan_braces(
    doc: &SupportDocument,
    surface: SupportSurface<'_>,
    drains: &[DrainCylinder],
    plan: &BracePlan,
    cancel: &mut Cancellation<'_>,
) -> Result<Vec<PlannedBrace>, SupportError> {
    check(plan)?;
    let shafts = shafts(doc, surface, cancel)?;
    // Only a hole's channel through the model can be plugged (TD-123).
    let channels: Vec<DrainCylinder> = drains
        .iter()
        .flat_map(|d| channel(&shafts.index, d))
        .collect();
    // Mini supports are short rods between the model's own parts: nothing to brace.
    let minis: BTreeSet<u64> = doc
        .supports
        .iter()
        .filter(|s| s.kind == SupportKind::Mini)
        .map(|s| s.id)
        .collect();
    let standing: Vec<Standing> = shafts
        .paths
        .iter()
        .filter(|(id, _)| !minis.contains(id))
        .filter_map(|(&id, path)| {
            let lowest = path.points.iter().min_by(|a, b| a[2].total_cmp(&b[2]))?;
            Some(Standing {
                id,
                xy: [lowest[0], lowest[1]],
                span: path.span(),
                widest: 2.0 * path.widest(),
            })
        })
        .collect();
    let pairs = neighbours(&standing, plan.max_distance_mm);
    let clearance = doc.clearance_mm;
    let mut next = doc
        .supports
        .iter()
        .map(|s| s.id)
        .chain(doc.braces.iter().map(|b| b.id))
        .max()
        .unwrap_or(0)
        + 1;
    let mut planned = Vec::new();
    for (n, &(i, j)) in pairs.iter().enumerate() {
        cancel.poll(Checkpoint::SupportWork, n)?;
        let (a, b) = (&standing[i], &standing[j]);
        for (za, zb, swap) in segments(a, b, shafts.floor, plan) {
            let (from, to) = if swap { (b, a) } else { (a, b) };
            let path = |s: &Standing, z: f64| shafts.paths.get(&s.id)?.at(z);
            let (Some((p, rp)), Some((q, rq))) = (path(from, za), path(to, zb)) else {
                continue;
            };
            let brace = Brace {
                id: 0,
                from: (from.id, za),
                to: (to.id, zb),
                diameter_mm: plan.diameter_mm,
                max_diameter_mm: plan.diameter_mm.map_or(plan.max_diameter_mm, |_| None),
            };
            let r = brace.radius_mm(rp, rq);
            let skipped = if joined(doc, a.id, b.id, za.min(zb), za.max(zb), plan.step_mm) {
                Some(Skipped::Joined)
            } else {
                let parts = [
                    frustum(p, r, q, r, SIDES),
                    sphere(p, r, SIDES),
                    sphere(q, r, SIDES),
                ];
                if parts
                    .iter()
                    .any(|part| !clear_of_model(&shafts.index, part, clearance))
                {
                    Some(Skipped::Model)
                } else if parts.iter().any(|part| {
                    channels
                        .iter()
                        .any(|c| !clear_of_tapered(part, c.start_mm, c.end_mm, c.radius_mm))
                }) {
                    Some(Skipped::DrainHole)
                } else {
                    None
                }
            };
            let id = if skipped.is_none() {
                next += 1;
                next - 1
            } else {
                0
            };
            planned.push(PlannedBrace {
                brace: Brace { id, ..brace },
                ends_mm: [p, q],
                diameter_mm: 2.0 * r,
                skipped,
            });
        }
    }
    Ok(planned)
}

/// The plan's values in their ranges, or why not.
fn check(plan: &BracePlan) -> Result<(), SupportError> {
    let invalid = |reason: &str| SupportError::Invalid {
        entity: 0,
        reason: reason.to_owned(),
    };
    if !(plan.max_distance_mm.is_finite() && plan.max_distance_mm > 0.0) {
        return Err(invalid("the braces' distance must be positive"));
    }
    if !(plan.lowest_mm.is_finite() && plan.lowest_mm >= 0.0) {
        return Err(invalid("the lowest brace must not be below the floor"));
    }
    if !(plan.step_mm.is_finite() && plan.step_mm > 0.0) {
        return Err(invalid("the braces' step must be positive"));
    }
    if plan.diameter_mm.is_some_and(|d| !(0.1..=10.0).contains(&d)) {
        return Err(invalid("the braces' diameter must be 0.1–10 mm"));
    }
    if plan
        .max_diameter_mm
        .is_some_and(|d| !(0.1..=10.0).contains(&d))
    {
        return Err(invalid("the braces' largest diameter must be 0.1–10 mm"));
    }
    Ok(())
}

/// Whether `part` is proven `clearance` away from every triangle of the surface near it.
fn clear_of_model(index: &super::index::TriangleIndex<'_>, part: &Part, clearance: f64) -> bool {
    let lo = [0, 1, 2].map(|k| {
        part.vertices
            .iter()
            .map(|v| v[k])
            .fold(f64::INFINITY, f64::min)
            - clearance
    });
    let hi = [0, 1, 2].map(|k| {
        part.vertices
            .iter()
            .map(|v| v[k])
            .fold(f64::NEG_INFINITY, f64::max)
            + clearance
    });
    index
        .candidates(lo, hi)
        .into_iter()
        .all(|t| proven_clear(part, index.corners(t), clearance))
}

/// Whether `doc` already braces supports `a` and `b` less than half a step from the heights
/// `[low, high]`.
fn joined(doc: &SupportDocument, a: u64, b: u64, low: f64, high: f64, step: f64) -> bool {
    doc.braces.iter().any(|e| {
        let pair = (e.from.0 == a && e.to.0 == b) || (e.from.0 == b && e.to.0 == a);
        let (e_low, e_high) = (e.from.1.min(e.to.1), e.from.1.max(e.to.1));
        let apart = (e_low - high).max(low - e_high).max(0.0);
        pair && apart < step / 2.0
    })
}

/// The braces of the pair `a`, `b` by the plan's pattern: each as its height on its first and
/// on its second support, and whether its first is `b`.
fn segments(a: &Standing, b: &Standing, floor: f64, plan: &BracePlan) -> Vec<(f64, f64, bool)> {
    // Clear of the shafts' end joints by a brace's diameter, an automatic one at most the
    // wider shaft's and its largest.
    let widest = a.widest.max(b.widest);
    let margin = plan
        .diameter_mm
        .unwrap_or_else(|| plan.max_diameter_mm.map_or(widest, |d| widest.min(d)));
    let low = a.span[0].max(b.span[0]) + margin;
    let high = a.span[1].min(b.span[1]) - margin;
    let first = floor + plan.lowest_mm;
    if high < low || high < first {
        return Vec::new();
    }
    // The first level at or above `low`; levels are counted from the lowest, so their parity
    // is the same on every pair.
    let skip = ((low - first) / plan.step_mm).ceil().max(0.0);
    let level = |k: f64| first + k * plan.step_mm;
    let mut out = Vec::new();
    let mut k = skip;
    while level(k) <= high {
        let (z, above) = (level(k), level(k + 1.0));
        let even = (k % 2.0) == 0.0;
        match plan.pattern {
            BracePattern::Horizontal => out.push((z, z, false)),
            // Even steps rise from the first support, odd ones from the second.
            BracePattern::Zigzag if above <= high => out.push((z, above, !even)),
            BracePattern::Cross if above <= high => {
                out.push((z, above, false));
                out.push((z, above, true));
            }
            BracePattern::Zigzag | BracePattern::Cross => {}
        }
        k += 1.0;
    }
    out
}

/// The pairs of `standing` (by their places) no farther apart than `distance` in plan with no
/// third support between them, nearest first, each kept only when it crosses no pair kept
/// before: see the module documentation.
fn neighbours(standing: &[Standing], distance: f64) -> Vec<(usize, usize)> {
    // Plate coordinates stay far inside `i64`.
    #[allow(clippy::cast_possible_truncation)]
    let cell = |p: [f64; 2]| p.map(|c| (c / distance).floor() as i64);
    let mut cells: BTreeMap<[i64; 2], Vec<usize>> = BTreeMap::new();
    for (k, s) in standing.iter().enumerate() {
        cells.entry(cell(s.xy)).or_default().push(k);
    }
    let apart = |i: usize, j: usize| {
        let (p, q) = (standing[i].xy, standing[j].xy);
        (p[0] - q[0]).hypot(p[1] - q[1])
    };
    // A third support nearer to both `i` and `j` than they are apart (`d`) stands between them;
    // nearer than `d` to `i`, it lies in `i`'s cell or a neighbouring one.
    let between = |i: usize, j: usize, d: f64| {
        let [cx, cy] = cell(standing[i].xy);
        (-1..=1).any(|dx| {
            (-1..=1).any(|dy| {
                cells
                    .get(&[cx + dx, cy + dy])
                    .into_iter()
                    .flatten()
                    .any(|&k| k != i && k != j && apart(i, k).max(apart(j, k)) < d)
            })
        })
    };
    let mut candidates: Vec<(f64, usize, usize)> = Vec::new();
    for (i, s) in standing.iter().enumerate() {
        let [cx, cy] = cell(s.xy);
        for dx in -1..=1 {
            for dy in -1..=1 {
                for &j in cells.get(&[cx + dx, cy + dy]).into_iter().flatten() {
                    let d = apart(i, j);
                    if j > i && d <= distance && d > 0.0 && !between(i, j, d) {
                        candidates.push((d, i, j));
                    }
                }
            }
        }
    }
    candidates.sort_by(|a, b| {
        a.0.total_cmp(&b.0)
            .then(standing[a.1].id.cmp(&standing[b.1].id))
            .then(standing[a.2].id.cmp(&standing[b.2].id))
    });
    // Kept pairs by the cells their bounding boxes cover: a pair is at most a cell long.
    let mut kept: Vec<(usize, usize)> = Vec::new();
    let mut by_cell: BTreeMap<[i64; 2], Vec<usize>> = BTreeMap::new();
    for (_, i, j) in candidates {
        let (p, q) = (standing[i].xy, standing[j].xy);
        let [lo, hi] = [
            cell([p[0].min(q[0]), p[1].min(q[1])]),
            cell([p[0].max(q[0]), p[1].max(q[1])]),
        ];
        let mut near: Vec<usize> = Vec::new();
        for x in lo[0]..=hi[0] {
            for y in lo[1]..=hi[1] {
                near.extend(by_cell.get(&[x, y]).into_iter().flatten());
            }
        }
        let crossed = near.iter().any(|&k| {
            let (a, b) = kept[k];
            crosses([i, j], [a, b], standing)
        });
        if crossed {
            continue;
        }
        for x in lo[0]..=hi[0] {
            for y in lo[1]..=hi[1] {
                by_cell.entry([x, y]).or_default().push(kept.len());
            }
        }
        kept.push((i, j));
    }
    kept
}

/// Whether the plan segments of the pairs `[i, j]` and `[a, b]` cross: meet inside both, or
/// overlap along one line. Pairs sharing a support meet only there unless they overlap.
fn crosses([i, j]: [usize; 2], [a, b]: [usize; 2], standing: &[Standing]) -> bool {
    let at = |k: usize| standing[k].xy;
    let turn = |o: [f64; 2], p: [f64; 2], q: [f64; 2]| {
        (p[0] - o[0]) * (q[1] - o[1]) - (p[1] - o[1]) * (q[0] - o[0])
    };
    let (p, q, c, d) = (at(i), at(j), at(a), at(b));
    let shared = [i, j].iter().any(|k| *k == a || *k == b);
    if shared {
        // From the shared end, the other ends lie on one ray: the pairs overlap.
        let (o, u, v) = if i == a {
            (p, q, d)
        } else if i == b {
            (p, q, c)
        } else if j == a {
            (q, p, d)
        } else {
            (q, p, c)
        };
        let along = (u[0] - o[0]) * (v[0] - o[0]) + (u[1] - o[1]) * (v[1] - o[1]);
        return turn(o, u, v) == 0.0 && along > 0.0;
    }
    let (d1, d2) = (turn(p, q, c), turn(p, q, d));
    let (d3, d4) = (turn(c, d, p), turn(c, d, q));
    if d1 * d2 < 0.0 && d3 * d4 < 0.0 {
        return true;
    }
    // A support exactly on the other segment counts as crossing it.
    let within = |o: [f64; 2], u: [f64; 2], v: [f64; 2]| {
        (v[0] - o[0].min(u[0])) * (v[0] - o[0].max(u[0])) <= 0.0
            && (v[1] - o[1].min(u[1])) * (v[1] - o[1].max(u[1])) <= 0.0
    };
    (d1 == 0.0 && within(p, q, c))
        || (d2 == 0.0 && within(p, q, d))
        || (d3 == 0.0 && within(c, d, p))
        || (d4 == 0.0 && within(c, d, q))
}

/// Closed bars between each pair of ends, each of its diameter, in world millimetres: braces as a
/// preview draws them, without the joints a build adds at their ends.
///
/// # Errors
/// A diameter that is not positive and finite, or a mesh beyond the limits.
pub fn brace_mesh(bars: &[([[f64; 3]; 2], f64)]) -> Result<TriangleMesh, SupportError> {
    let mut vertices: Vec<Point3> = Vec::new();
    let mut triangles: Vec<[u32; 3]> = Vec::new();
    for &([p, q], diameter_mm) in bars {
        if !(diameter_mm.is_finite() && diameter_mm > 0.0) {
            return Err(SupportError::Invalid {
                entity: 0,
                reason: "a brace's diameter must be positive".to_owned(),
            });
        }
        let r = diameter_mm / 2.0;
        // Coincident ends make no bar.
        if p.iter().zip(&q).all(|(a, b)| (a - b).abs() <= 0.0) {
            continue;
        }
        let part = frustum(p, r, q, r, SIDES);
        let offset =
            u32::try_from(vertices.len()).map_err(|_| SupportError::Resource("brace vertices"))?;
        vertices.extend(
            part.vertices
                .iter()
                .map(|v: &V| Point3::new(v[0], v[1], v[2])),
        );
        triangles.extend(part.triangles.iter().map(|t| t.map(|i| i + offset)));
    }
    TriangleMesh::try_new(vertices, triangles)
        .map_err(|e| SupportError::Geometry(format!("brace mesh: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::supports::{
        build_supports, AxisSpec, BaseSpec, BuildVolume, ManualSupport, Preset, SupportOptions,
    };

    /// A 40 x 40 slab from Z 20 to 22, facing out.
    fn slab() -> TriangleMesh {
        let v = |x: f64, y: f64, z: f64| Point3::new(x, y, z);
        let vertices = vec![
            v(0.0, 0.0, 20.0),
            v(40.0, 0.0, 20.0),
            v(40.0, 40.0, 20.0),
            v(0.0, 40.0, 20.0),
            v(0.0, 0.0, 22.0),
            v(40.0, 0.0, 22.0),
            v(40.0, 40.0, 22.0),
            v(0.0, 40.0, 22.0),
        ];
        let triangles = vec![
            [0, 2, 1],
            [0, 3, 2],
            [4, 5, 6],
            [4, 6, 7],
            [0, 1, 5],
            [0, 5, 4],
            [1, 2, 6],
            [1, 6, 5],
            [2, 3, 7],
            [2, 7, 6],
            [3, 0, 4],
            [3, 4, 7],
        ];
        TriangleMesh::try_new(vertices, triangles).unwrap()
    }

    /// Supports under the slab's bottom at `xys`, IDs from 1.
    fn document(xys: &[[f64; 2]]) -> SupportDocument {
        let mut doc = SupportDocument::new(Preset::Medium);
        for (k, &[x, y]) in xys.iter().enumerate() {
            doc.supports.push(ManualSupport {
                id: k as u64 + 1,
                tip_mm: [x, y, 20.0],
                tip_axis: AxisSpec::Normal,
                bends: Vec::new(),
                base: BaseSpec::Vertical,
                parameters: Preset::Medium.parameters(),
                accept_collision: false,
                origin: crate::supports::SupportOrigin::Placed,
                kind: crate::supports::SupportKind::Standard,
            });
        }
        doc
    }

    fn plan(pattern: BracePattern) -> BracePlan {
        BracePlan {
            max_distance_mm: 12.0,
            lowest_mm: 4.0,
            step_mm: 4.0,
            pattern,
            diameter_mm: Some(0.8),
            max_diameter_mm: None,
        }
    }

    fn planned(doc: &SupportDocument, mesh: &TriangleMesh, plan: &BracePlan) -> Vec<PlannedBrace> {
        plan_braces(
            doc,
            SupportSurface {
                mesh,
                interior_from: None,
            },
            &[],
            plan,
            &mut Cancellation::never(),
        )
        .unwrap()
    }

    #[test]
    fn a_square_of_supports_is_braced_round_its_sides_only() {
        let mesh = slab();
        // A 10 mm square: its 14.1 mm diagonals are within 15 mm, but each has the other two
        // corners nearer to both its ends: only the four sides.
        let doc = document(&[[10.0, 10.0], [20.0, 10.0], [20.0, 20.0], [10.0, 20.0]]);
        let wide = BracePlan {
            max_distance_mm: 15.0,
            ..plan(BracePattern::Horizontal)
        };
        let braces = planned(&doc, &mesh, &wide);
        let mut pairs: Vec<(u64, u64)> = braces
            .iter()
            .map(|p| {
                (
                    p.brace.from.0.min(p.brace.to.0),
                    p.brace.from.0.max(p.brace.to.0),
                )
            })
            .collect();
        pairs.dedup();
        assert_eq!(pairs, [(1, 2), (1, 4), (2, 3), (3, 4)]);
        // Levels from 4 mm every 4 mm, below the tip joins.
        let heights: Vec<f64> = braces
            .iter()
            .filter(|p| (p.brace.from.0, p.brace.to.0) == (1, 2))
            .map(|p| p.brace.from.1)
            .collect();
        let top = 20.0 - Preset::Medium.parameters().tip_length_mm - 0.8;
        assert_ne!(heights.len(), 0);
        for (z, level) in heights.iter().zip([4.0, 8.0, 12.0, 16.0]) {
            assert!((z - level).abs() < 1e-9, "{heights:?}");
            assert!(*z <= top);
        }
        // Every kept brace has its own new ID, after the supports'.
        let ids: Vec<u64> = braces.iter().map(|p| p.brace.id).collect();
        assert!(braces.iter().all(|p| p.skipped.is_none()));
        assert_eq!(ids, (5..5 + ids.len() as u64).collect::<Vec<_>>());
        // Built as planned, without a finding.
        let mut built = doc.clone();
        built.braces = braces.iter().map(|p| p.brace).collect();
        let scene = build_supports(
            &built.validated().unwrap(),
            SupportSurface {
                mesh: &mesh,
                interior_from: None,
            },
            &[],
            BuildVolume {
                origin_xy_mm: [0.0, 0.0],
                size_mm: [100.0, 100.0, 100.0],
            },
            SupportOptions::default(),
            &mut Cancellation::never(),
        )
        .unwrap();
        assert_eq!(scene.braces(), braces.len());
        assert!(!scene.has_errors(), "{:?}", scene.findings());
        let end = scene.shaft_point(1, 8.0).unwrap();
        assert!((end[0] - 10.0).abs() < 1e-9 && (end[2] - 8.0).abs() < 1e-12);
        let [low, high] = scene.shaft_span(1).unwrap();
        assert!(low < 4.0 && high < 20.0 && high > 15.0, "{low} {high}");
        // Its path: the tip's join down to the base join, straight.
        let path = scene.shaft_path(1).unwrap();
        assert_eq!(path.len(), 2);
        assert!((path[0][2] - high).abs() < 1e-12 && (path[1][2] - low).abs() < 1e-12);
    }

    #[test]
    fn patterns_join_each_step_once_or_twice_and_far_supports_not_at_all() {
        let mesh = slab();
        let doc = document(&[[10.0, 10.0], [20.0, 10.0], [35.0, 35.0]]);
        let count = |pattern| planned(&doc, &mesh, &plan(pattern)).len();
        let level = count(BracePattern::Horizontal);
        let zigzag = count(BracePattern::Zigzag);
        // One pair: n levels give n level braces, n - 1 diagonals, twice that crossed.
        assert!(level >= 3);
        assert_eq!(zigzag, level - 1);
        assert_eq!(count(BracePattern::Cross), 2 * zigzag);
        // Zigzag diagonals alternate their rising side.
        let braces = planned(&doc, &mesh, &plan(BracePattern::Zigzag));
        assert_eq!(braces[0].brace.from.0, 1);
        assert_eq!(braces[1].brace.from.0, 2);
        assert!(braces.iter().all(|p| p.brace.from.1 < p.brace.to.1));
    }

    #[test]
    fn braces_near_the_model_or_already_there_are_skipped_and_said_why() {
        let mesh = slab();
        let mut doc = document(&[[10.0, 10.0], [20.0, 10.0]]);
        doc.braces.push(Brace {
            id: 9,
            from: (1, 8.0),
            to: (2, 8.0),
            diameter_mm: Some(0.8),
            max_diameter_mm: None,
        });
        // High levels reach under the slab: a step of 2 mm from 2 mm reaches 16 mm.
        let close = BracePlan {
            lowest_mm: 2.0,
            step_mm: 2.0,
            ..plan(BracePattern::Horizontal)
        };
        let braces = planned(&doc, &mesh, &close);
        let at = |z: f64| braces.iter().find(|p| (p.brace.from.1 - z).abs() < 1e-9);
        assert_eq!(at(8.0).unwrap().skipped, Some(Skipped::Joined));
        assert_eq!(at(8.0).unwrap().brace.id, 0);
        assert_eq!(at(6.0).unwrap().skipped, None);
        // New IDs follow the brace already there.
        assert!(at(6.0).unwrap().brace.id > 9);
        // Out of range: refused, not clamped.
        let wrong = BracePlan {
            step_mm: 0.0,
            ..close
        };
        assert!(plan_braces(
            &doc,
            SupportSurface {
                mesh: &mesh,
                interior_from: None,
            },
            &[],
            &wrong,
            &mut Cancellation::never(),
        )
        .is_err());
        // A drain hole up through the slab, between the pair: it cuts only the model (TD-123),
        // so the braces below it are kept as without it.
        let drain = DrainCylinder {
            start_mm: [15.0, 5.0, 0.0],
            end_mm: [15.0, 5.0, 30.0],
            radius_mm: 6.0,
        };
        let drained = plan_braces(
            &doc,
            SupportSurface {
                mesh: &mesh,
                interior_from: None,
            },
            &[drain],
            &close,
            &mut Cancellation::never(),
        )
        .unwrap();
        assert_eq!(drained, braces);
        // A clearance larger than the slab's distance from the top level skips it as too close.
        doc.clearance_mm = 4.0;
        let braces = planned(&doc, &mesh, &close);
        let top = braces.last().unwrap();
        assert_eq!(top.skipped, Some(Skipped::Model), "{braces:?}");
    }

    #[test]
    fn a_preview_is_one_closed_bar_per_brace() {
        let mesh = brace_mesh(&[
            ([[0.0, 0.0, 1.0], [5.0, 0.0, 1.0]], 0.8),
            ([[0.0; 3], [0.0; 3]], 0.8),
        ])
        .unwrap();
        // One 16-sided bar with flat caps: the coincident pair is left out.
        assert_eq!(mesh.triangles().len(), 2 * 16 + 2 * 14);
        assert!(brace_mesh(&[([[0.0; 3], [1.0; 3]], 0.0)]).is_err());
    }

    #[test]
    fn automatic_braces_take_the_thinner_shaft_where_they_meet_it() {
        let mesh = slab();
        let mut doc = document(&[[10.0, 10.0], [20.0, 10.0]]);
        // The second support's shaft narrows to 0.6 mm at its top, from 1.2 mm at its foot.
        doc.supports[1].parameters.shaft_diameter_mm = 0.6;
        doc.supports[1].parameters.shaft_bottom_diameter_mm = Some(1.2);
        let automatic = BracePlan {
            diameter_mm: None,
            ..plan(BracePattern::Horizontal)
        };
        let braces = planned(&doc, &mesh, &automatic);
        assert!(braces.len() >= 2);
        let full = Preset::Medium.parameters().shaft_diameter_mm;
        for p in &braces {
            assert_eq!(p.brace.diameter_mm, None);
            // Thinner higher up, never wider than the first shaft.
            assert!(p.diameter_mm > 0.6 && p.diameter_mm < 1.2, "{p:?}");
            assert!(p.diameter_mm <= full + 1e-12);
        }
        assert!(braces[0].diameter_mm > braces[braces.len() - 1].diameter_mm);
        // Built, an automatic brace has the diameter the plan said.
        let mut built = doc.clone();
        built.braces = braces.iter().map(|p| p.brace).collect();
        let scene = build_supports(
            &built.validated().unwrap(),
            SupportSurface {
                mesh: &mesh,
                interior_from: None,
            },
            &[],
            BuildVolume {
                origin_xy_mm: [0.0, 0.0],
                size_mm: [100.0, 100.0, 100.0],
            },
            SupportOptions::default(),
            &mut Cancellation::never(),
        )
        .unwrap();
        assert!(!scene.has_errors(), "{:?}", scene.findings());
        let z = braces[0].brace.to.1;
        let thin = scene.shaft_diameter(2, z).unwrap();
        assert!((thin.min(full) - braces[0].diameter_mm).abs() < 1e-9);
        // A document without a brace diameter reads as automatic.
        let json = br#"{"format":"karasu-supports","version":1,
            "supports":[{"id":1,"tip":{"point":[0,0,10]},"base":{"vertical":true}},
                        {"id":2,"tip":{"point":[5,0,10]},"base":{"vertical":true}}],
            "braces":[{"id":3,"from":{"support":1,"z_mm":4},"to":{"support":2,"z_mm":4}}]}"#;
        let read = SupportDocument::from_json(json).unwrap();
        assert_eq!(read.braces[0].diameter_mm, None);
    }

    #[test]
    fn automatic_braces_take_the_thinner_shaft_up_to_their_largest_diameter() {
        let mesh = slab();
        // Heavy's shafts (1.2 mm) braced automatically up to 1.0 mm (TD-136).
        let mut doc = document(&[[10.0, 10.0], [20.0, 10.0]]);
        for s in &mut doc.supports {
            s.parameters.shaft_diameter_mm = 1.2;
        }
        let capped = BracePlan {
            diameter_mm: None,
            max_diameter_mm: Some(1.0),
            ..plan(BracePattern::Horizontal)
        };
        let braces = planned(&doc, &mesh, &capped);
        assert_ne!(braces.len(), 0);
        for p in &braces {
            assert_eq!(p.brace.diameter_mm, None);
            assert_eq!(p.brace.max_diameter_mm, Some(1.0));
            assert!((p.diameter_mm - 1.0).abs() < 1e-12, "{p:?}");
        }
        // Thinner shafts than the largest keep their own.
        let thin = BracePlan {
            max_diameter_mm: Some(1.5),
            ..capped
        };
        assert!(planned(&doc, &mesh, &thin)
            .iter()
            .all(|p| (p.diameter_mm - 1.2).abs() < 1e-12));
        // A brace of its own diameter takes no largest one, even when the plan has one.
        let fixed = BracePlan {
            diameter_mm: Some(0.8),
            ..capped
        };
        assert!(planned(&doc, &mesh, &fixed)
            .iter()
            .all(|p| p.brace.max_diameter_mm.is_none() && (p.diameter_mm - 0.8).abs() < 1e-12));
        // The largest diameter is written and read back; with a diameter of its own, refused.
        let mut built = doc.clone();
        built.braces = braces.iter().map(|p| p.brace).collect();
        let read = SupportDocument::from_json(&built.to_json()).unwrap();
        assert_eq!(read.braces, built.braces);
        let both = br#"{"format":"karasu-supports","version":1,
            "supports":[{"id":1,"tip":{"point":[0,0,10]},"base":{"vertical":true}},
                        {"id":2,"tip":{"point":[5,0,10]},"base":{"vertical":true}}],
            "braces":[{"id":3,"from":{"support":1,"z_mm":4},"to":{"support":2,"z_mm":4},
                       "diameter_mm":0.8,"max_diameter_mm":1.0}]}"#;
        assert!(SupportDocument::from_json(both).is_err());
        assert!(check(&BracePlan {
            max_diameter_mm: Some(0.0),
            ..capped
        })
        .is_err());
    }

    /// The pairs `standing` at `xys` (IDs from 1) are braced in, within `distance`.
    fn pairs_of(xys: &[[f64; 2]], distance: f64) -> Vec<(u64, u64)> {
        let standing: Vec<Standing> = xys
            .iter()
            .enumerate()
            .map(|(k, &xy)| Standing {
                id: k as u64 + 1,
                xy,
                span: [0.0, 10.0],
                widest: 1.0,
            })
            .collect();
        let mut pairs: Vec<(u64, u64)> = neighbours(&standing, distance)
            .into_iter()
            .map(|(i, j)| (standing[i].id, standing[j].id))
            .collect();
        pairs.sort_unstable();
        pairs
    }

    #[test]
    fn a_brace_joins_next_supports_never_past_a_third() {
        // A row, not quite straight: A–B, B–C, C–D, never past one.
        assert_eq!(
            pairs_of(&[[0.0, 0.0], [6.0, 0.8], [12.0, -0.5], [18.0, 0.3]], 25.0),
            [(1, 2), (2, 3), (3, 4)]
        );
        // A triangle: its two shorter sides; the longest has the third corner between.
        assert_eq!(
            pairs_of(&[[0.0, 0.0], [10.0, 0.0], [4.0, 5.0]], 25.0),
            [(1, 3), (2, 3)]
        );
        // A 3 x 3 grid: the cells' twelve sides, no diagonal.
        let grid: Vec<[f64; 2]> = (0..9)
            .map(|k| [f64::from(k % 3) * 6.0, f64::from(k / 3) * 6.0])
            .collect();
        let pairs = pairs_of(&grid, 25.0);
        assert_eq!(pairs.len(), 12, "{pairs:?}");
        assert!(pairs.iter().all(|&(a, b)| {
            let at = |id: u64| grid[usize::try_from(id).unwrap() - 1];
            let (p, q) = (at(a), at(b));
            ((p[0] - q[0]).hypot(p[1] - q[1]) - 6.0).abs() < 1e-9
        }));
        // Beyond the distance, nothing.
        assert_eq!(pairs_of(&[[0.0, 0.0], [30.0, 0.0]], 25.0), []);
    }
}
