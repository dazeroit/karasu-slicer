//! Vertices only the arrangement made, taken out of the exact triangulation (TD-140 phase 4).
//!
//! Cell walls and planes extended across a leaf cut faces where no surface does. Joining the
//! pieces again ([`super::join_cut_faces`]) undoes most cuts, but not where the region is not
//! convex (a face around a hole): a vertex stays where a wall met the region's side, or inside
//! it, where rounding moves it. Such a vertex lies on no intersection of the inputs: every face
//! around it is in one plane, or in two planes meeting along a straight line through it. Taking
//! it out and triangulating the polygon its faces leave, in the same plane, covers exactly the
//! same surface with one vertex fewer to round.
//!
//! Every test is exact ([`super::turn`]); a vertex whose faces are not one disk, or whose polygon
//! cannot be triangulated without a triangle of no area, is left as it is.
use super::{reverses, turn, Plane, Point};
use std::cmp::Ordering;

/// Take out of `triangles` (each in plane `planes[t]`, its support `supports[planes[t]]`) every
/// vertex made by meeting planes (not an input's vertex) whose faces are one disk in one plane,
/// or in two planes on either side of a straight line through it. Vertices are visited in index
/// order: the result is the same on every run.
pub(super) fn decimate(
    points: &[Point],
    supports: &[Plane],
    triangles: &mut Vec<[u32; 3]>,
    planes: &mut Vec<u32>,
) {
    // Points made by meeting planes, not input vertices (which are made as grid points).
    let made: Vec<bool> = points.iter().map(|p| p.as_grid().is_none()).collect();
    let mut around: Vec<Vec<u32>> = vec![Vec::new(); points.len()];
    for (t, triangle) in triangles.iter().enumerate() {
        for &v in triangle {
            if made[v as usize] {
                around[v as usize].push(u32::try_from(t).expect("fewer than 2^32 triangles"));
            }
        }
    }
    let mut alive = vec![true; triangles.len()];
    for v in 0..points.len() {
        if !made[v] {
            continue;
        }
        let fan: Vec<u32> = around[v]
            .iter()
            .copied()
            .filter(|&t| alive[t as usize])
            .collect();
        let Some(replacement) = remove(points, supports, triangles, planes, &fan, v) else {
            continue;
        };
        for &t in &fan {
            alive[t as usize] = false;
        }
        for (triangle, plane) in replacement {
            let t = u32::try_from(triangles.len()).expect("fewer than 2^32 triangles");
            for &w in &triangle {
                if made[w as usize] {
                    around[w as usize].push(t);
                }
            }
            triangles.push(triangle);
            planes.push(plane);
            alive.push(true);
        }
    }
    let mut keep = alive.iter().copied();
    triangles.retain(|_| keep.next().unwrap_or(true));
    let mut keep = alive.iter().copied();
    planes.retain(|_| keep.next().unwrap_or(true));
}

/// The triangles that replace the `fan` around vertex `v`, when it can be taken out.
fn remove(
    points: &[Point],
    supports: &[Plane],
    triangles: &[[u32; 3]],
    planes: &[u32],
    fan: &[u32],
    v: usize,
) -> Option<Vec<([u32; 3], u32)>> {
    let v = u32::try_from(v).ok()?;
    // Each face as (v, x, y): the link edge x → y, in the faces' turning order around v.
    let mut link: Vec<(u32, u32, u32)> = Vec::with_capacity(fan.len());
    for &t in fan {
        let tri = triangles[t as usize];
        let k = tri.iter().position(|&w| w == v)?;
        link.push((tri[(k + 1) % 3], tri[(k + 2) % 3], planes[t as usize]));
    }
    if link.len() < 3 {
        return None;
    }
    // One cycle: each vertex leaves once and is reached once.
    let mut order = Vec::with_capacity(link.len());
    let mut at = 0;
    for _ in 0..link.len() {
        order.push(at);
        let next = link[at].1;
        let mut found = link.iter().enumerate().filter(|(_, e)| e.0 == next);
        let (j, _) = found.next()?;
        if found.next().is_some() {
            return None;
        }
        at = j;
    }
    if at != 0 || {
        let mut seen = order.clone();
        seen.sort_unstable();
        seen.dedup();
        seen.len() != link.len()
    } {
        return None;
    }
    let cycle: Vec<(u32, u32, u32)> = order.iter().map(|&i| link[i]).collect();
    // Where the plane changes along the cycle.
    let n = cycle.len();
    let changes: Vec<usize> = (0..n)
        .filter(|&i| cycle[i].2 != cycle[(i + n - 1) % n].2)
        .collect();
    match changes.len() {
        0 => {
            let polygon: Vec<u32> = cycle.iter().map(|e| e.0).collect();
            let plane = cycle[0].2;
            Some(
                fill(points, &supports[plane as usize], &polygon)?
                    .into_iter()
                    .map(|t| (t, plane))
                    .collect(),
            )
        }
        2 => {
            // Two runs, meeting at link vertices a and b on the planes' common line: v must lie
            // between them, so taking it out leaves the line straight.
            let (a, b) = (cycle[changes[0]].0, cycle[changes[1]].0);
            // Two planes not parallel meet in a line, which holds a, v and b; back to back (one
            // plane, both ways) they do not.
            let (p, q) = (cycle[changes[0]].2, cycle[changes[1]].2);
            if supports[p as usize].parallel(&supports[q as usize])
                || reverses(
                    &points[a as usize],
                    &points[v as usize],
                    &points[b as usize],
                )
            {
                return None;
            }
            let mut out = Vec::new();
            for (start, end) in [(changes[0], changes[1]), (changes[1], changes[0] + n)] {
                // The run's faces, from its first link vertex to the next run's first.
                let polygon: Vec<u32> = (start..=end).map(|i| cycle[i % n].0).collect();
                let plane = cycle[start].2;
                out.extend(
                    fill(points, &supports[plane as usize], &polygon)?
                        .into_iter()
                        .map(|t| (t, plane)),
                );
            }
            Some(out)
        }
        _ => None,
    }
}

/// Triangles covering the simple `polygon` (in the turning order of the faces it replaces) in
/// the plane `support`, none of them flat; `None` when no such triangulation is found.
fn fill(points: &[Point], support: &Plane, polygon: &[u32]) -> Option<Vec<[u32; 3]>> {
    let axis = (0..3)
        .max_by_key(|&k| support.n[k].unsigned_abs())
        .unwrap_or(0);
    // The faces turn counterclockwise about the support's normal; projected along `axis`, as
    // the normal's component on it.
    let orientation = support.n[axis].cmp(&0);
    let mut ids = polygon.to_vec();
    let mut out = Vec::with_capacity(ids.len().saturating_sub(2));
    while ids.len() > 3 {
        let n = ids.len();
        let at = |i: usize| ids[i % n];
        let bend = |i: usize| turn(points, at(i + n - 1), at(i), at(i + 1), axis);
        let ear = |i: usize| {
            bend(i) == orientation && {
                let (a, b, c) = (at(i + n - 1), at(i), at(i + 1));
                // No other vertex of the polygon inside the triangle or on its sides.
                (0..n).map(at).all(|p| {
                    p == a
                        || p == b
                        || p == c
                        || [(a, b), (b, c), (c, a)]
                            .iter()
                            .any(|&(s, e)| turn(points, s, e, p, axis) == orientation.reverse())
                })
            }
        };
        // An ear next to a vertex on a straight side first: clipping it never leaves three
        // points on one line.
        let chosen = (0..n)
            .find(|&i| {
                (bend(i + n - 1) == Ordering::Equal || bend(i + 1) == Ordering::Equal) && ear(i)
            })
            .or_else(|| (0..n).find(|&i| ear(i)))?;
        out.push([at(chosen + n - 1), at(chosen), at(chosen + 1)]);
        ids.remove(chosen);
    }
    (turn(points, ids[0], ids[1], ids[2], axis) == orientation).then(|| {
        out.push([ids[0], ids[1], ids[2]]);
        out
    })
}
