//! The Web raft (owner, 2026-11-10, TD-144 phase 2): a disc under each plate foot and low bars
//! joining neighbouring feet, in triangles that never cross.
//!
//! **Discs:** under each foot, the slab of that foot alone: a frustum from the plate to the
//! raft's thickness, its top the fillet widened by the margin, its sides at the raft's slope.
//!
//! **Bars:** every pair of feet whose centres lie no farther apart than the longest bar is a
//! candidate. Candidates are taken shortest first (ties by the feet's places), and one becomes a
//! bar unless it crosses a bar already kept or passes through another foot's disc. Crossings are
//! decided with exact orientation signs, so the bars are a deterministic set of segments that
//! never cross: the greedy triangulation of the feet, cut at the longest bar. Feet whose discs
//! overlap at their tops are joined already and get no bar.
//!
//! **Shapes:** a bar is a straight prism from one foot's centre to the other's, its section a
//! trapezoid: the bar's width at its top, widened down to the plate by the raft's slope over
//! the bar's height. Its ends lie inside the discs. Discs and bars stay separate closed convex
//! parts, overlapping as every support's parts do; the slicer unions them by winding.
//!
//! **Pieces:** discs that overlap at the plate, and feet a bar joins, form one piece; the scene
//! counts pieces as its rafts.
use super::{turn_2d, Owned, SupportError, SupportPart};
use crate::{
    cancellation::{Cancellation, Checkpoint},
    supports::{
        document::RaftSpec,
        shapes::{frustum, Part, V},
    },
};
use std::{cmp::Ordering, collections::HashMap};

/// The most candidate bars a web considers before it refuses as beyond the limits: feet packed
/// so densely that every foot has hundreds of others within the longest bar.
const MAX_CANDIDATES: usize = 1 << 22;

/// A web's parts and how many separate pieces they make.
pub(super) struct Web {
    pub(super) parts: Vec<Owned>,
    pub(super) pieces: usize,
}

/// The web raft under `feet` (each a base centre, its fillet's radius and its sides) with
/// `spec`'s values.
pub(super) fn web(
    feet: &[(V, f64, u16)],
    spec: RaftSpec,
    cancel: &mut Cancellation<'_>,
) -> Result<Web, SupportError> {
    let centres: Vec<[f64; 2]> = feet.iter().map(|(c, _, _)| [c[0], c[1]]).collect();
    let tops: Vec<f64> = feet.iter().map(|&(_, r, _)| r + spec.margin_mm).collect();
    let plates: Vec<f64> = feet
        .iter()
        .map(|&(_, r, _)| spec.plate_radius_mm(r))
        .collect();
    let mut parts = Vec::with_capacity(feet.len() * 4);
    for (k, &(c, _, sides)) in feet.iter().enumerate() {
        cancel.poll(Checkpoint::SupportWork, k)?;
        let shape = frustum(
            [c[0], c[1], 0.0],
            plates[k],
            [c[0], c[1], spec.thickness_mm],
            tops[k],
            sides,
        );
        parts.push(owned(shape));
    }
    let bars = choose_bars(&centres, &tops, spec.bars.longest_mm, cancel)?;
    let bar_spec = RaftSpec {
        thickness_mm: spec.bars.height_mm,
        ..spec
    };
    let half_top = spec.bars.width_mm / 2.0;
    let half_plate = half_top + bar_spec.spread_mm();
    for &(i, j) in &bars {
        parts.push(owned(bar(
            centres[i],
            centres[j],
            [half_plate, half_top],
            spec.bars.height_mm,
        )));
    }
    let pieces = pieces(&centres, &plates, &bars);
    Ok(Web { parts, pieces })
}

/// A raft part of `shape`.
fn owned(shape: Part) -> Owned {
    Owned {
        entity: 0,
        part: SupportPart::Raft,
        accepted: false,
        exempt: false,
        reach: None,
        shape,
    }
}

/// The grid cell of `p` for cells `size` wide.
#[allow(clippy::cast_possible_truncation)] // Plate coordinates over cells of a millimetre or more.
fn cell(p: [f64; 2], size: f64) -> (i64, i64) {
    ((p[0] / size).floor() as i64, (p[1] / size).floor() as i64)
}

/// The cells, `size` wide, around the segment from `a` to `b`: those next to the cells of points
/// taken along it every half cell. Any point within half a cell of the segment lies in one of
/// them (it is within three quarters of a cell of a point taken, so its cell is at most one
/// away in each direction), so two segments that meet share one, and a disc no wider than a
/// cell that comes near the segment has its centre in one. Sorted, without repeats.
fn corridor(a: [f64; 2], b: [f64; 2], size: f64) -> Vec<(i64, i64)> {
    let mut out: Vec<(i64, i64)> = walk(a, b, size).collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// The cells of [`corridor`] from `a` towards `b`, some more than once.
fn walk(a: [f64; 2], b: [f64; 2], size: f64) -> impl Iterator<Item = (i64, i64)> {
    let length = (b[0] - a[0]).hypot(b[1] - a[1]);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // A few dozen steps.
    let steps = (length / (size / 2.0)).ceil() as usize;
    (0..=steps).flat_map(move |k| {
        let t = if steps == 0 {
            0.0
        } else {
            as_fraction(k, steps)
        };
        let (x, y) = cell([a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1])], size);
        (-1..=1).flat_map(move |dx| (-1..=1).map(move |dy| (x + dx, y + dy)))
    })
}

/// `k` over `n`, as a fraction.
#[allow(clippy::cast_precision_loss)] // Step counts far below 2^52.
fn as_fraction(k: usize, n: usize) -> f64 {
    k as f64 / n as f64
}

/// The bars between feet at `centres` whose discs have `tops` radii at their tops: candidates no
/// longer than `longest`, shortest first, kept unless they cross a kept bar or pass through
/// another foot's disc. Each bar is a pair of feet's places, the lower first, in the order kept.
pub(super) fn choose_bars(
    centres: &[[f64; 2]],
    tops: &[f64],
    longest: f64,
    cancel: &mut Cancellation<'_>,
) -> Result<Vec<(usize, usize)>, SupportError> {
    let size = longest.max(1.0);
    let mut grid: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    for (k, &c) in centres.iter().enumerate() {
        grid.entry(cell(c, size)).or_default().push(k);
    }
    let apart = |i: usize, j: usize| {
        let d = [centres[j][0] - centres[i][0], centres[j][1] - centres[i][1]];
        d[0] * d[0] + d[1] * d[1]
    };
    // Candidates: pairs within the longest bar whose discs do not already overlap.
    let mut candidates: Vec<(f64, usize, usize)> = Vec::new();
    for (i, &c) in centres.iter().enumerate() {
        cancel.poll(Checkpoint::SupportWork, i)?;
        let (x, y) = cell(c, size);
        for dx in -1..=1 {
            for dy in -1..=1 {
                for &j in grid.get(&(x + dx, y + dy)).into_iter().flatten() {
                    let squared = apart(i, j);
                    let joined = (tops[i] + tops[j]).powi(2);
                    if j > i && squared <= longest * longest && squared > joined {
                        candidates.push((squared, i, j));
                    }
                }
            }
        }
        if candidates.len() > MAX_CANDIDATES {
            return Err(SupportError::Resource("web raft candidate bars"));
        }
    }
    candidates.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
    // Kept bars and feet by the cells of a finer grid, at least as wide as the widest disc, so
    // a disc that comes near a segment lies in the cells along it (see `corridor`).
    let widest = tops.iter().copied().fold(0.0, f64::max);
    let fine = (2.0 * widest).max(longest / 32.0).max(0.05);
    let mut feet_cells: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    for (k, &c) in centres.iter().enumerate() {
        feet_cells.entry(cell(c, fine)).or_default().push(k);
    }
    let mut kept: Vec<(usize, usize)> = Vec::new();
    let mut bar_cells: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    // The candidate that last looked at each kept bar, so a bar in several cells is tested once.
    let mut seen: Vec<usize> = Vec::new();
    for (n, &(_, i, j)) in candidates.iter().enumerate() {
        cancel.poll(Checkpoint::SupportWork, n)?;
        let (a, b) = (centres[i], centres[j]);
        // Through another foot's disc: refused. Most long candidates are, near their start, so
        // the cells are walked from it and the walk stops at the first disc met.
        let blocked = walk(a, b, fine).any(|c| {
            feet_cells
                .get(&c)
                .into_iter()
                .flatten()
                .any(|&k| k != i && k != j && segment_distance(centres[k], a, b) < tops[k])
        });
        if blocked {
            continue;
        }
        let along = corridor(a, b, fine);
        // Across a kept bar: refused.
        seen.resize(kept.len(), usize::MAX);
        let crosses = along.iter().any(|c| {
            bar_cells.get(c).into_iter().flatten().any(|&m| {
                if seen[m] == n {
                    return false;
                }
                seen[m] = n;
                let (p, q) = kept[m];
                p != i && p != j && q != i && q != j && cross(a, b, centres[p], centres[q])
            })
        });
        if crosses {
            continue;
        }
        let m = kept.len();
        kept.push((i, j));
        for c in along {
            bar_cells.entry(c).or_default().push(m);
        }
    }
    Ok(kept)
}

/// Whether segments `a`–`b` and `c`–`d`, with no end in common, meet: by exact orientation
/// signs, touching and overlapping included.
fn cross(a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2]) -> bool {
    let (o1, o2) = (turn_2d(a, b, c), turn_2d(a, b, d));
    let (o3, o4) = (turn_2d(c, d, a), turn_2d(c, d, b));
    if o1 != o2 && o3 != o4 {
        return true;
    }
    // All on one line: they meet where their extents overlap.
    let within = |p: [f64; 2], q: [f64; 2], r: [f64; 2]| {
        r[0] >= p[0].min(q[0])
            && r[0] <= p[0].max(q[0])
            && r[1] >= p[1].min(q[1])
            && r[1] <= p[1].max(q[1])
    };
    o1 == Ordering::Equal
        && o2 == Ordering::Equal
        && (within(a, b, c) || within(a, b, d) || within(c, d, a) || within(c, d, b))
}

/// The distance from `p` to the segment from `a` to `b`.
fn segment_distance(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let ab = [b[0] - a[0], b[1] - a[1]];
    let ap = [p[0] - a[0], p[1] - a[1]];
    let length = ab[0] * ab[0] + ab[1] * ab[1];
    let t = if length > 0.0 {
        ((ap[0] * ab[0] + ap[1] * ab[1]) / length).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let q = [a[0] + t * ab[0], a[1] + t * ab[1]];
    ((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2)).sqrt()
}

/// A bar from `a` to `b` on the plate, `height` high, its half widths at the plate and at its
/// top: a closed convex prism, wound outward.
fn bar(a: [f64; 2], b: [f64; 2], [plate, top]: [f64; 2], height: f64) -> Part {
    let along = [b[0] - a[0], b[1] - a[1]];
    let length = along[0].hypot(along[1]);
    let u = [along[0] / length, along[1] / length];
    // The left of the bar's direction.
    let left = [-u[1], u[0]];
    let corner = |p: [f64; 2], side: f64, half: f64, z: f64| -> V {
        [
            p[0] + left[0] * side * half,
            p[1] + left[1] * side * half,
            z,
        ]
    };
    // Counterclockwise seen from above: right at `a`, right at `b`, left at `b`, left at `a`.
    let ring = |half: f64, z: f64| -> [V; 4] {
        [
            corner(a, -1.0, half, z),
            corner(b, -1.0, half, z),
            corner(b, 1.0, half, z),
            corner(a, 1.0, half, z),
        ]
    };
    let vertices: Vec<V> = ring(plate, 0.0)
        .into_iter()
        .chain(ring(top, height))
        .collect();
    let mut triangles = vec![[0, 2, 1], [0, 3, 2], [4, 5, 6], [4, 6, 7]];
    for k in 0..4u32 {
        let next = (k + 1) % 4;
        triangles.push([k, next, 4 + next]);
        triangles.push([k, 4 + next, 4 + k]);
    }
    let radius = plate.max(top.hypot(height));
    Part {
        vertices,
        triangles,
        axis: [[a[0], a[1], 0.0], [b[0], b[1], 0.0]],
        radius,
        taper: [radius; 2],
    }
}

/// The root of `k`'s set in the union-find `parent`, halving the path on the way.
fn root(parent: &mut [usize], mut k: usize) -> usize {
    while parent[k] != k {
        parent[k] = parent[parent[k]];
        k = parent[k];
    }
    k
}

/// How many pieces the discs at `centres`, `plates` wide at the plate, make with `bars`: discs
/// that overlap and feet a bar joins are one.
fn pieces(centres: &[[f64; 2]], plates: &[f64], bars: &[(usize, usize)]) -> usize {
    let mut parent: Vec<usize> = (0..centres.len()).collect();
    let join = |parent: &mut Vec<usize>, i: usize, j: usize| {
        let (a, b) = (root(parent, i), root(parent, j));
        if a != b {
            parent[a.max(b)] = a.min(b);
        }
    };
    for &(i, j) in bars {
        join(&mut parent, i, j);
    }
    let widest = plates.iter().copied().fold(0.0, f64::max);
    let size = (2.0 * widest).max(1.0);
    let mut grid: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    for (k, &c) in centres.iter().enumerate() {
        grid.entry(cell(c, size)).or_default().push(k);
    }
    for (i, &c) in centres.iter().enumerate() {
        let (x, y) = cell(c, size);
        for dx in -1..=1 {
            for dy in -1..=1 {
                for &j in grid.get(&(x + dx, y + dy)).into_iter().flatten() {
                    let d = (centres[j][0] - c[0]).hypot(centres[j][1] - c[1]);
                    if j > i && d < plates[i] + plates[j] {
                        join(&mut parent, i, j);
                    }
                }
            }
        }
    }
    (0..centres.len())
        .filter(|&k| root(&mut parent, k) == k)
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chosen(centres: &[[f64; 2]], top: f64, longest: f64) -> Vec<(usize, usize)> {
        let tops = vec![top; centres.len()];
        choose_bars(centres, &tops, longest, &mut Cancellation::never()).unwrap()
    }

    #[test]
    fn a_square_of_feet_takes_its_sides_and_one_diagonal() {
        let square = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
        let bars = chosen(&square, 2.0, 20.0);
        // Four sides, then the first diagonal by the feet's places; the other would cross it.
        assert_eq!(bars, vec![(0, 1), (0, 3), (1, 2), (2, 3), (0, 2)]);
        // Shorter than the diagonal, only the sides.
        assert_eq!(chosen(&square, 2.0, 12.0).len(), 4);
        // Shorter than a side, none.
        assert_eq!(chosen(&square, 2.0, 9.0), Vec::new());
        // Discs touching along the sides join those feet already: only a diagonal.
        assert_eq!(chosen(&square, 5.0, 20.0), vec![(0, 2)]);
    }

    #[test]
    fn bars_never_cross_nor_pass_through_a_disc_and_repeat_exactly() {
        // A jittered grid, deterministic.
        let mut centres = Vec::new();
        for k in 0..400_u32 {
            let (x, y) = (f64::from(k % 20), f64::from(k / 20));
            let jitter = f64::from((k * 7919) % 13) / 13.0;
            centres.push([x * 4.0 + jitter, y * 4.0 - jitter / 2.0]);
        }
        let tops = vec![1.0; centres.len()];
        let bars = choose_bars(&centres, &tops, 12.0, &mut Cancellation::never()).unwrap();
        assert!(bars.len() > 2 * centres.len(), "{}", bars.len());
        for (m, &(i, j)) in bars.iter().enumerate() {
            let (a, b) = (centres[i], centres[j]);
            for (k, &c) in centres.iter().enumerate() {
                if k != i && k != j {
                    assert!(segment_distance(c, a, b) >= tops[k], "{i}-{j} through {k}");
                }
            }
            for &(p, q) in &bars[m + 1..] {
                if p != i && p != j && q != i && q != j {
                    assert!(!cross(a, b, centres[p], centres[q]), "{i}-{j} x {p}-{q}");
                }
            }
        }
        // The same feet give the same bars.
        assert_eq!(
            choose_bars(&centres, &tops, 12.0, &mut Cancellation::never()).unwrap(),
            bars
        );
        // One piece: every foot is joined.
        let plates = vec![1.5; centres.len()];
        assert_eq!(pieces(&centres, &plates, &bars), 1);
        assert_eq!(pieces(&centres, &plates, &[]), centres.len());
    }

    #[test]
    fn a_bar_is_a_closed_trapezoid_prism_wound_outward() {
        let part = bar([0.0, 0.0], [10.0, 0.0], [1.0, 0.5], 1.0);
        assert_eq!((part.vertices.len(), part.triangles.len()), (8, 12));
        // Every face's normal points away from the bar's middle.
        let centre = [5.0, 0.0, 0.5];
        for t in &part.triangles {
            let [p, q, r] = t.map(|k| part.vertices[k as usize]);
            let (u, v) = (
                [q[0] - p[0], q[1] - p[1], q[2] - p[2]],
                [r[0] - p[0], r[1] - p[1], r[2] - p[2]],
            );
            let normal = [
                u[1] * v[2] - u[2] * v[1],
                u[2] * v[0] - u[0] * v[2],
                u[0] * v[1] - u[1] * v[0],
            ];
            let out = [p[0] - centre[0], p[1] - centre[1], p[2] - centre[2]];
            assert!(
                normal[0] * out[0] + normal[1] * out[1] + normal[2] * out[2] > 0.0,
                "{t:?}"
            );
        }
        // Each edge is shared by two faces.
        let mut edges: HashMap<(u32, u32), i32> = HashMap::new();
        for t in &part.triangles {
            for k in 0..3 {
                let (a, b) = (t[k], t[(k + 1) % 3]);
                *edges.entry((a.min(b), a.max(b))).or_default() += if a < b { 1 } else { -1 };
            }
        }
        assert!(edges.values().all(|&n| n == 0));
    }
}
