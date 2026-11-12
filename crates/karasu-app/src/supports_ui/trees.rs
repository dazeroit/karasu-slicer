//! Trunks joining trunks (owner, 2026-10-30: branching, phase 3), shared by the Islands tool's
//! plan and the Supports tool's "Merge trunks" and "Join a support": supports standing on their
//! own (on the plate, or on the model) close together in plan merge into fewer, as a tree's
//! limbs into its trunk.
//!
//! **A level.** Stems standing on their own closer than the merge distance in plan (between
//! their shafts' lowest points), up to the stems a group takes, form a group: each one not yet
//! taken with the next ones near it, in order. The one nearest the group's middle stays where it
//! stands: the main trunk. Each other one keeps its shaft down to a bend, then leans at most 45°
//! from vertical to the main trunk and joins it there, clear of the model, never further aside
//! than the merge's reach (owner, 2026-10-30: the branch distance; longer leans were weak
//! diagonals hanging across the model): the bend is as high as it can be below what hangs from
//! its shaft (its branches' junctions and braces' ends, which
//! never hang from a part taken away), at least a pad's room above the floor. A stem with no
//! such bend stays where it stands. Levels repeat on the main trunks left, as many as asked.
//!
//! **Thicker below.** A main trunk carries the tips of what joins it: its shaft widens towards
//! its foot by the square root of the tips it carries over a full tree's, at most twice its
//! diameter, as a tree's trunk does.
//!
//! **Plain data.** Stems carry their shafts as built (a polyline from the tip's join down), so
//! merging runs on any thread; the surface for routes proves the slanted parts clear of the
//! model. Deterministic: stems by their places, groups in order.
use super::route::RouteSurface;
use karasu_core::mesh::TriangleMesh;
use std::sync::atomic::{AtomicBool, Ordering};

/// The steepest a merged trunk's slanted part goes, as the tangent of its angle from vertical:
/// 45°, less a margin for rounding.
const SLANT: f64 = 0.98;
/// How far a bend stays below what hangs from the shaft, in mm.
const BELOW_HANGING_MM: f64 = 1.0;
/// How far a bend stays above the floor, in mm: room for the pad and its cone.
const ABOVE_FLOOR_MM: f64 = 3.0;
/// The step junctions are looked for down a main trunk, in mm.
const STEP_MM: f64 = 0.25;
/// The step bends are looked for down a stem, in mm.
const BEND_STEP_MM: f64 = 1.0;
/// How far a junction stays from a main trunk's ends, in its shaft's diameters.
const FROM_ENDS: f64 = 1.5;
/// The tips a trunk carries at its own diameter: a full tree of the default branches.
const TIPS_AT_DIAMETER: f64 = 5.0;
/// How much thicker a main trunk's foot gets at most.
const MOST_THICKER: f64 = 2.0;

/// A support as merging sees it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Stem {
    /// Its shaft as built, in world millimetres, from its tip's join down to its base join.
    pub shaft: Vec<[f64; 3]>,
    /// Its shaft's diameter at its top.
    pub shaft_mm: f64,
    /// It stands on its own (the plate, the raft or the model) and may merge.
    pub free: bool,
    /// The stem it stands on, and the height where, for a branch.
    pub joins: Option<(usize, f64)>,
    /// The heights of braces' ends on its shaft.
    pub braced: Vec<f64>,
}

/// A stem merged: its place, the bend its shaft turns at, and the main trunk it joins (its
/// place) and the height where.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Merge {
    pub stem: usize,
    pub bend: [f64; 3],
    pub joins: (usize, f64),
}

/// End `bends`, a shaft's bends from its tip down, at `bend` on that shaft, where it leaves
/// for another: the bends below it come after it along the shaft and are dropped. `height`
/// tells a bend's height on the plate (bends may be kept in a model's frame).
pub(crate) fn end_at(bends: &mut Vec<[f64; 3]>, bend: [f64; 3], height: impl Fn([f64; 3]) -> f64) {
    let z = height(bend);
    bends.retain(|&b| height(b) > z);
    bends.push(bend);
}

/// What merging stems does: the stems merged, and the main trunks' new feet (place and shaft
/// bottom diameter).
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Merged {
    pub merges: Vec<Merge>,
    pub wider: Vec<(usize, f64)>,
}

/// How stems merge: within `merge_mm` in plan, `per_group` at most a group, `levels` times,
/// each leaning at most `aside_mm` aside.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct MergeValues {
    pub merge_mm: f64,
    pub per_group: u32,
    pub levels: u32,
    pub aside_mm: f64,
}

/// How far aside a merged trunk leans at most for a branch distance of `branch_mm`: that
/// distance, or the default one's without branches.
#[must_use]
pub(crate) fn aside_for(branch_mm: f64) -> f64 {
    if branch_mm > 0.0 {
        branch_mm
    } else {
        crate::islands_ui::plan::DEFAULT_ISLAND_SUPPORTS.branch_mm
    }
}

/// What the slanted parts are kept clear of, and the room they keep.
pub(crate) struct Room<'a> {
    pub surface: &'a RouteSurface,
    pub mesh: &'a TriangleMesh,
    /// The plate's height, or the raft's top.
    pub floor: f64,
    pub clearance: f64,
}

/// The point at height `z` on `shaft` (the first segment from the top that reaches it), if any.
fn at(shaft: &[[f64; 3]], z: f64) -> Option<[f64; 3]> {
    shaft.windows(2).find_map(|w| {
        let (a, b) = (w[0], w[1]);
        let (lo, hi) = (a[2].min(b[2]), a[2].max(b[2]));
        (lo < hi && (lo..=hi).contains(&z)).then(|| {
            let t = (z - a[2]) / (b[2] - a[2]);
            [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, z]
        })
    })
}

/// The lowest and highest heights of `shaft`.
fn span(shaft: &[[f64; 3]]) -> Option<[f64; 2]> {
    let zs = shaft.iter().map(|p| p[2]);
    let low = zs.clone().fold(f64::INFINITY, f64::min);
    let high = zs.fold(f64::NEG_INFINITY, f64::max);
    (low < high).then_some([low, high])
}

/// Merge `stems`, level after level: see the module.
pub(crate) fn merge(
    stems: &[Stem],
    values: MergeValues,
    room: &Room<'_>,
    stop: &AtomicBool,
) -> Merged {
    let mut merged = Merged::default();
    if values.merge_mm <= 0.0 {
        return merged;
    }
    let mut joins: Vec<Option<(usize, f64)>> = stems.iter().map(|s| s.joins).collect();
    for _ in 0..values.levels {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        let roots: Vec<usize> = (0..stems.len())
            .filter(|&k| stems[k].free && joins[k].is_none() && span(&stems[k].shaft).is_some())
            .collect();
        let foot = |k: usize| {
            let last = stems[k].shaft.last().copied().unwrap_or([0.0; 3]);
            [last[0], last[1]]
        };
        let mut any = false;
        for group in groups(&roots, &foot, values.merge_mm, values.per_group) {
            let main = nearest_middle(&group, &foot);
            for &other in group.iter().filter(|&&k| k != main) {
                let aside = Some(values.aside_mm);
                if let Some((bend, z)) = join_onto(stems, &joins, other, main, room, aside) {
                    joins[other] = Some((main, z));
                    merged.merges.push(Merge {
                        stem: other,
                        bend,
                        joins: (main, z),
                    });
                    any = true;
                }
            }
        }
        if !any {
            break;
        }
    }
    merged.wider = wider(stems, &joins);
    merged
}

/// The groups of `roots` closer than `merge_mm` in plan by their `foot`: each root not yet
/// taken, in order, with the next ones not taken near it, up to `per_group` in all.
fn groups(
    roots: &[usize],
    foot: &dyn Fn(usize) -> [f64; 2],
    merge_mm: f64,
    per_group: u32,
) -> Vec<Vec<usize>> {
    let mut taken = vec![false; roots.len()];
    let mut out = Vec::new();
    for seed in 0..roots.len() {
        if taken[seed] {
            continue;
        }
        taken[seed] = true;
        let mut group = vec![roots[seed]];
        let p = foot(roots[seed]);
        for other in seed + 1..roots.len() {
            if group.len() >= per_group as usize {
                break;
            }
            let q = foot(roots[other]);
            if !taken[other] && (p[0] - q[0]).hypot(p[1] - q[1]) <= merge_mm {
                taken[other] = true;
                group.push(roots[other]);
            }
        }
        if group.len() > 1 {
            out.push(group);
        }
    }
    out
}

/// The stem of `group` nearest its middle in plan, ties by place.
fn nearest_middle(group: &[usize], foot: &dyn Fn(usize) -> [f64; 2]) -> usize {
    #[allow(clippy::cast_precision_loss)] // A few stems.
    let count = group.len() as f64;
    let middle = [0, 1].map(|k| group.iter().map(|&s| foot(s)[k]).sum::<f64>() / count);
    *group
        .iter()
        .min_by(|&&a, &&b| {
            let d = |s: usize| {
                let p = foot(s);
                (p[0] - middle[0]).hypot(p[1] - middle[1])
            };
            d(a).total_cmp(&d(b)).then(a.cmp(&b))
        })
        .expect("a group has members")
}

/// Where stem `other` would bend and join stem `main` (as merging joins them; `joins` tells
/// what hangs from `other`), leaning at most `aside_mm` aside when given: the bend and the
/// junction's height, if any. Also the manual "Join a support", where the user chose the shaft.
pub(crate) fn join_onto(
    stems: &[Stem],
    joins: &[Option<(usize, f64)>],
    other: usize,
    main: usize,
    room: &Room<'_>,
    aside_mm: Option<f64>,
) -> Option<([f64; 3], f64)> {
    let (shaft, trunk) = (&stems[other].shaft, &stems[main].shaft);
    let ([low, high], [main_low, main_high]) = (span(shaft)?, span(trunk)?);
    let hanging = joins
        .iter()
        .filter_map(|j| *j)
        .filter(|(t, _)| *t == other)
        .map(|(_, z)| z)
        .chain(stems[other].braced.iter().copied())
        .fold(high, f64::min);
    let radius = stems[other].shaft_mm / 2.0 + room.clearance;
    let reach = FROM_ENDS * stems[main].shaft_mm;
    let (top, bottom) = (main_high - reach, main_low + reach);
    let column = at(trunk, bottom)?;
    let mut h = hanging - BELOW_HANGING_MM;
    while h >= (room.floor + ABOVE_FLOOR_MM).max(low) {
        if let Some(bend) = at(shaft, h) {
            // The highest junction 45° allows from the bend, then lower while not clear; at
            // least a step below the bend, which may lie on the main trunk itself (two shafts
            // routed aside to one column), so the two never coincide.
            let across = (bend[0] - column[0]).hypot(bend[1] - column[1]);
            let mut z = (h - across / SLANT).min(top).min(h - STEP_MM);
            while z >= bottom {
                let Some(point) = at(trunk, z) else {
                    break;
                };
                let across = (bend[0] - point[0]).hypot(bend[1] - point[1]);
                if across <= (bend[2] - point[2]) * SLANT
                    && aside_mm.is_none_or(|most| across <= most)
                    && room.surface.grid().clear(room.mesh, bend, point, radius)
                {
                    return Some((bend, z));
                }
                z -= STEP_MM;
            }
        }
        h -= BEND_STEP_MM;
    }
    None
}

/// The main trunks' new feet: each stem carrying more tips than a full tree, standing on its
/// own, widened by the square root of its tips over a full tree's, at most twice.
fn wider(stems: &[Stem], joins: &[Option<(usize, f64)>]) -> Vec<(usize, f64)> {
    let mut carried = vec![1.0_f64; stems.len()];
    for k in 0..stems.len() {
        let mut at = joins[k].map(|(t, _)| t);
        let mut hops = 0;
        while let Some(trunk) = at.filter(|_| hops < stems.len()) {
            carried[trunk] += 1.0;
            at = joins[trunk].map(|(t, _)| t);
            hops += 1;
        }
    }
    (0..stems.len())
        .filter(|&k| joins[k].is_none() && carried[k] > TIPS_AT_DIAMETER)
        .map(|k| {
            let factor = (carried[k] / TIPS_AT_DIAMETER).sqrt().min(MOST_THICKER);
            (k, stems[k].shaft_mm * factor)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_point_on_a_bent_shaft_is_on_its_first_segment_reaching_the_height() {
        let shaft = [[0.0, 0.0, 10.0], [2.0, 0.0, 8.0], [2.0, 0.0, 1.0]];
        assert_eq!(at(&shaft, 9.0), Some([1.0, 0.0, 9.0]));
        assert_eq!(at(&shaft, 5.0), Some([2.0, 0.0, 5.0]));
        assert_eq!(at(&shaft, 11.0), None);
        assert_eq!(span(&shaft), Some([1.0, 10.0]));
    }

    #[test]
    fn a_shaft_ended_above_its_knee_drops_the_knee() {
        // A shaft from its tip down to a knee at 10 mm, then to the plate.
        let knee = [5.0, 0.0, 10.0];
        // Ended at 6 mm, below the knee: the knee stays before the bend.
        let mut bends = vec![knee];
        end_at(&mut bends, [5.0, 0.0, 6.0], |b| b[2]);
        assert_eq!(bends, vec![knee, [5.0, 0.0, 6.0]]);
        // Ended at 14 mm, above the knee: the knee came after it, and goes.
        let mut bends = vec![knee];
        end_at(&mut bends, [1.0, 0.0, 14.0], |b| b[2]);
        assert_eq!(bends, vec![[1.0, 0.0, 14.0]]);
    }

    #[test]
    fn a_main_trunk_widens_with_the_tips_it_carries() {
        let stem = |joins: Option<(usize, f64)>| Stem {
            shaft: vec![[0.0, 0.0, 10.0], [0.0, 0.0, 1.0]],
            shaft_mm: 1.0,
            free: true,
            joins,
            braced: Vec::new(),
        };
        // A trunk carrying nineteen others carries twenty tips: twice its diameter at most.
        let mut stems = vec![stem(None)];
        stems.extend((0..19).map(|_| stem(Some((0, 5.0)))));
        let joins: Vec<_> = stems.iter().map(|s| s.joins).collect();
        assert_eq!(wider(&stems, &joins), [(0, 2.0)]);
        // Five tips are a full tree: no wider.
        stems.truncate(5);
        let joins: Vec<_> = stems.iter().map(|s| s.joins).collect();
        assert_eq!(wider(&stems, &joins), []);
    }
}
