//! Deterministic incremental hull of derived finite candidate points, never source mesh repair.
//!
//! Exact positive visibility retains coplanar facets. Each insertion replaces one visible disk by
//! a cone over its single oriented horizon cycle. A horizon edge cannot be collinear with the new
//! point: its removed incident triangle would then have zero, rather than positive, visibility.

use super::{numeric::orient3d, resource, uncertain, HollowError, Work};
use crate::{cancellation::Cancellation, geometry::Point3, mesh::TriangleMesh};
use std::cmp::Ordering;

type Face = [usize; 3];
type Edge = [usize; 2];

fn compare_points(left: &Point3, right: &Point3) -> Ordering {
    left.x
        .total_cmp(&right.x)
        .then(left.y.total_cmp(&right.y))
        .then(left.z.total_cmp(&right.z))
}

fn edge_key(edge: Edge) -> [usize; 2] {
    [edge[0].min(edge[1]), edge[0].max(edge[1])]
}

fn noncollinear(
    first: Point3,
    second: Point3,
    third: Point3,
    work: &mut Work,
    cancel: &mut Cancellation<'_>,
) -> Result<bool, HollowError> {
    // These four affine-independent probes cannot all lie on a genuine triangle's plane.
    for probe in [
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(1.0, 0.0, 0.0),
        Point3::new(0.0, 1.0, 0.0),
        Point3::new(0.0, 0.0, 1.0),
    ] {
        work.charge(1, cancel)?;
        if orient3d(first, second, third, probe) != Ordering::Equal {
            return Ok(true);
        }
    }
    Ok(false)
}

fn seed(
    points: &[Point3],
    work: &mut Work,
    cancel: &mut Cancellation<'_>,
) -> Result<[usize; 4], HollowError> {
    if points.len() < 4 {
        return Err(uncertain("fewer than four distinct cavity candidates"));
    }
    let first = 0;
    let second = points.len() - 1;
    let mut third = None;
    for index in 1..second {
        if noncollinear(points[first], points[second], points[index], work, cancel)? {
            third = Some(index);
            break;
        }
    }
    let third = third.ok_or_else(|| uncertain("collinear cavity candidates"))?;
    for fourth in 1..second {
        work.charge(1, cancel)?;
        if orient3d(points[first], points[second], points[third], points[fourth]) != Ordering::Equal
        {
            return Ok([first, second, third, fourth]);
        }
    }
    Err(uncertain("coplanar cavity candidates"))
}

fn push_face(faces: &mut Vec<Face>, face: Face, work: &mut Work) -> Result<(), HollowError> {
    if faces.len() >= work.limits.max_output_triangles {
        return Err(resource("hull triangles"));
    }
    work.push(faces, face)
}

fn horizon(
    edges: &mut Vec<Edge>,
    degrees: &mut [[u8; 2]],
    work: &mut Work,
    cancel: &mut Cancellation<'_>,
) -> Result<(), HollowError> {
    work.charge(edges.len(), cancel)?;
    edges.sort_unstable_by_key(|&edge| (edge_key(edge), edge));
    work.charge(edges.len(), cancel)?;
    let mut write = 0;
    let mut read = 0;
    while read < edges.len() {
        work.charge(1, cancel)?;
        let mut end = read + 1;
        while end < edges.len() && edge_key(edges[end]) == edge_key(edges[read]) {
            end += 1;
        }
        match end - read {
            1 => {
                edges[write] = edges[read];
                write += 1;
            }
            2 if edges[read] == [edges[read + 1][1], edges[read + 1][0]] => {}
            _ => return Err(uncertain("nonmanifold visible hull edge")),
        }
        read = end;
    }
    edges.truncate(write);
    if edges.len() < 3 {
        return Err(uncertain("degenerate hull horizon"));
    }
    edges.sort_unstable();
    work.charge(edges.len(), cancel)?;
    for edge in edges.iter() {
        degrees[edge[0]] = [0; 2];
        degrees[edge[1]] = [0; 2];
    }
    for edge in edges.iter() {
        work.charge(1, cancel)?;
        degrees[edge[0]][0] = degrees[edge[0]][0]
            .checked_add(1)
            .ok_or_else(|| uncertain("hull horizon degree"))?;
        degrees[edge[1]][1] = degrees[edge[1]][1]
            .checked_add(1)
            .ok_or_else(|| uncertain("hull horizon degree"))?;
    }
    for edge in edges.iter() {
        if degrees[edge[0]] != [1, 1] || degrees[edge[1]] != [1, 1] {
            return Err(uncertain("branched hull horizon"));
        }
    }
    let start = edges[0][0];
    let mut vertex = start;
    for step in 0..edges.len() {
        work.charge(1, cancel)?;
        let index = edges
            .binary_search_by_key(&vertex, |edge| edge[0])
            .map_err(|_| uncertain("open hull horizon"))?;
        vertex = edges[index][1];
        if vertex == start && step + 1 != edges.len() {
            return Err(uncertain("multiple hull horizon cycles"));
        }
    }
    if vertex != start {
        return Err(uncertain("open hull horizon"));
    }
    Ok(())
}

struct Scratch {
    retained: Vec<Face>,
    edges: Vec<Edge>,
    degrees: Vec<[u8; 2]>,
}

fn insert(
    index: usize,
    points: &[Point3],
    faces: &mut Vec<Face>,
    scratch: &mut Scratch,
    work: &mut Work,
    cancel: &mut Cancellation<'_>,
) -> Result<(), HollowError> {
    scratch.retained.clear();
    scratch.edges.clear();
    for &face in faces.iter() {
        work.charge(1, cancel)?;
        if orient3d(
            points[face[0]],
            points[face[1]],
            points[face[2]],
            points[index],
        ) == Ordering::Greater
        {
            for edge in [[face[0], face[1]], [face[1], face[2]], [face[2], face[0]]] {
                work.push(&mut scratch.edges, edge)?;
            }
        } else {
            push_face(&mut scratch.retained, face, work)?;
        }
    }
    if scratch.edges.is_empty() {
        return Ok(());
    }
    horizon(&mut scratch.edges, &mut scratch.degrees, work, cancel)?;
    for &edge in &scratch.edges {
        work.charge(1, cancel)?;
        push_face(&mut scratch.retained, [edge[0], edge[1], index], work)?;
    }
    std::mem::swap(faces, &mut scratch.retained);
    Ok(())
}

fn compact(
    points: &[Point3],
    faces: &[Face],
    work: &mut Work,
    cancel: &mut Cancellation<'_>,
) -> Result<TriangleMesh, HollowError> {
    let mut mapping = Vec::new();
    work.reserve(&mut mapping, points.len())?;
    mapping.resize(points.len(), u32::MAX);
    for face in faces {
        for &vertex in face {
            mapping[vertex] = 0;
        }
    }
    let mut vertices = Vec::new();
    for (index, &point) in points.iter().enumerate() {
        work.charge(1, cancel)?;
        if mapping[index] != u32::MAX {
            mapping[index] =
                u32::try_from(vertices.len()).map_err(|_| resource("hull vertex indices"))?;
            work.push(&mut vertices, point)?;
        }
    }
    let mut triangles = Vec::new();
    work.reserve(&mut triangles, faces.len())?;
    for face in faces {
        work.charge(1, cancel)?;
        triangles.push(face.map(|index| mapping[index]));
    }
    TriangleMesh::try_new_with_cancellation(vertices, triangles, cancel).map_err(HollowError::from)
}

pub(super) fn build(
    mut points: Vec<Point3>,
    work: &mut Work,
    cancel: &mut Cancellation<'_>,
) -> Result<TriangleMesh, HollowError> {
    for point in &mut points {
        work.charge(1, cancel)?;
        for coordinate in [&mut point.x, &mut point.y, &mut point.z] {
            if !coordinate.is_finite() {
                return Err(uncertain("nonfinite hull candidate"));
            }
            if *coordinate == 0.0 {
                *coordinate = 0.0;
            }
        }
    }
    points.sort_unstable_by(compare_points);
    work.charge(points.len(), cancel)?;
    points.dedup();
    if points.len() > work.limits.max_output_vertices {
        return Err(resource("hull vertices"));
    }
    let [first, second, third, fourth] = seed(&points, work, cancel)?;
    let mut faces = Vec::new();
    for (mut face, opposite) in [
        ([first, second, third], fourth),
        ([first, fourth, second], third),
        ([second, fourth, third], first),
        ([third, fourth, first], second),
    ] {
        work.charge(1, cancel)?;
        if orient3d(
            points[face[0]],
            points[face[1]],
            points[face[2]],
            points[opposite],
        ) == Ordering::Greater
        {
            face.swap(1, 2);
        }
        push_face(&mut faces, face, work)?;
    }
    let mut scratch = Scratch {
        retained: Vec::new(),
        edges: Vec::new(),
        degrees: Vec::new(),
    };
    work.reserve(&mut scratch.degrees, points.len())?;
    scratch.degrees.resize(points.len(), [0; 2]);
    for index in 0..points.len() {
        work.charge(1, cancel)?;
        if ![first, second, third, fourth].contains(&index) {
            insert(index, &points, &mut faces, &mut scratch, work, cancel)?;
        }
    }
    // Certify inclusion of every candidate, including those discarded as interior/coplanar.
    for face in &faces {
        for &point in &points {
            work.charge(1, cancel)?;
            if orient3d(points[face[0]], points[face[1]], points[face[2]], point)
                == Ordering::Greater
            {
                return Err(uncertain("hull does not contain every candidate"));
            }
        }
    }
    compact(&points, &faces, work, cancel)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preparation::hollow::HollowLimits;

    fn checked_hull(points: &[Point3]) -> TriangleMesh {
        let mut work = Work {
            limits: HollowLimits::default(),
            used: 0,
            requested: 0,
        };
        let mesh = build(points.to_owned(), &mut work, &mut Cancellation::never()).unwrap();
        for face in mesh.triangles() {
            for &point in points {
                assert_ne!(
                    orient3d(
                        mesh.vertices()[face[0] as usize],
                        mesh.vertices()[face[1] as usize],
                        mesh.vertices()[face[2] as usize],
                        point
                    ),
                    Ordering::Greater
                );
            }
        }
        let mut edges = Vec::new();
        for face in mesh.triangles() {
            for edge in [[face[0], face[1]], [face[1], face[2]], [face[2], face[0]]] {
                edges.push(edge);
            }
        }
        for edge in &edges {
            assert_eq!(
                edges
                    .iter()
                    .filter(|other| **other == [edge[1], edge[0]])
                    .count(),
                1
            );
        }
        mesh
    }

    #[test]
    fn cube_with_coplanar_and_interior_candidates_is_closed_and_deterministic() {
        let mut points = Vec::new();
        for x in [0.0, 1.0] {
            for y in [0.0, 1.0] {
                for z in [0.0, 1.0] {
                    points.push(Point3::new(x, y, z));
                }
            }
        }
        points.extend([
            Point3::new(0.5, 0.5, 0.5),
            Point3::new(0.5, 0.5, 0.0),
            Point3::new(-0.0, 0.0, 0.0),
        ]);
        let forward = checked_hull(&points);
        points.reverse();
        let reversed = checked_hull(&points);
        assert_eq!(forward, reversed);
        assert!(forward.vertices().len() >= 8);
    }

    #[test]
    fn tetrahedron_has_four_faces_and_discards_interior_points() {
        let mesh = checked_hull(&[
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(4.0, 0.0, 0.0),
            Point3::new(0.0, 4.0, 0.0),
            Point3::new(0.0, 0.0, 4.0),
            Point3::new(0.5, 0.5, 0.5),
        ]);
        assert_eq!(mesh.vertices().len(), 4);
        assert_eq!(mesh.triangles().len(), 4);
    }

    #[test]
    fn lower_dimensional_candidates_and_resource_exhaustion_are_explicit() {
        let points = vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(0.0, 1.0, 0.0),
            Point3::new(1.0, 1.0, 0.0),
        ];
        let mut work = Work {
            limits: HollowLimits::default(),
            used: 0,
            requested: 0,
        };
        assert!(matches!(
            build(points, &mut work, &mut Cancellation::never()),
            Err(HollowError::Uncertain { .. })
        ));
        work.limits.max_work = 0;
        assert!(matches!(
            build(
                vec![Point3::new(0.0, 0.0, 0.0)],
                &mut work,
                &mut Cancellation::never()
            ),
            Err(HollowError::Resource { .. })
        ));
    }

    #[test]
    fn deterministic_integer_clouds_retain_all_supporting_halfspaces() {
        let mut state = 503_u64;
        for _ in 0..8 {
            let mut points = Vec::new();
            for _ in 0..32 {
                let mut coordinate = || {
                    state = state
                        .wrapping_mul(6_364_136_223_846_793_005)
                        .wrapping_add(1);
                    f64::from(i32::try_from((state >> 32) % 31).unwrap() - 15)
                };
                points.push(Point3::new(coordinate(), coordinate(), coordinate()));
            }
            checked_hull(&points);
        }
    }
}
