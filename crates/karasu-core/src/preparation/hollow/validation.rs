//! Exact support-halfspace and embedded-triangulation admission.
use super::{invalid, numeric::orient3d, resource, HollowError, Work};
use crate::{
    cancellation::Cancellation,
    geometry::Point3,
    mesh::{
        topology::{CheckStatus, TopologyLimits},
        TriangleMesh,
    },
};
use std::cmp::Ordering::{self, Equal, Greater, Less};

pub(super) fn validate(
    mesh: &TriangleMesh,
    work: &mut Work,
    cancel: &mut Cancellation<'_>,
) -> Result<Vec<usize>, HollowError> {
    let vertices = mesh.vertices();
    let faces = mesh.triangles();
    if vertices.len() > work.limits.max_input_vertices
        || faces.len() > work.limits.max_input_triangles
    {
        return Err(resource("validation input count"));
    }
    // M2 owns its fallible allocations. Reserve a conservative admission allowance
    // for its linear incidence/link/component storage, not an allocator/RSS bound.
    // Each triangle has three incidences; links are reused per vertex and each
    // partition retains at most all source faces/vertices. 4096 bytes per entity
    // exceeds these simultaneously live fixed-size records and geometric growth.
    let entities = vertices
        .len()
        .checked_add(faces.len())
        .ok_or_else(|| resource("topology accounting"))?;
    work.account(
        entities
            .checked_mul(4096)
            .ok_or_else(|| resource("topology accounting"))?,
    )?;
    work.charge(entities, cancel)?;
    let report = mesh.analyze_topology_with_cancellation(
        TopologyLimits {
            max_vertices: work.limits.max_input_vertices,
            max_triangles: work.limits.max_input_triangles,
        },
        cancel,
    )?;
    let summary = report.summary();
    if !summary.closed_combinatorial_surface
        || summary.local_winding != CheckStatus::Satisfied
        || summary.excluded_triangles != 0
        || summary.isolated_vertices != 0
        || summary.duplicate_triangles != 0
    {
        return Err(invalid(
            "source must be a closed consistently wound manifold without excluded faces",
            None,
        ));
    }
    if summary.edge_components != 1 {
        return Err(HollowError::Unsupported {
            reason: "multiple source components",
        });
    }
    drop(report);
    for (i, &a) in vertices.iter().enumerate() {
        for &b in &vertices[..i] {
            work.charge(1, cancel)?;
            if a == b {
                return Err(invalid(
                    "distinct source indices have identical coordinates",
                    None,
                ));
            }
        }
    }
    let mut planes = Vec::new();
    for (i, &face) in faces.iter().enumerate() {
        let [a, b, c] = face.map(|v| vertices[v as usize]);
        let mut behind = false;
        for &p in vertices {
            work.charge(1, cancel)?;
            match orient3d(a, b, c, p) {
                Greater => {
                    return Err(HollowError::Unsupported {
                        reason: "source is not an outward convex solid",
                    });
                }
                Less => behind = true,
                Equal => {}
            }
        }
        if !behind {
            return Err(invalid("source has no strictly interior side", Some(i)));
        }
        let mut duplicate = false;
        for &previous in &planes {
            let old: [u32; 3] = faces[previous];
            let mut coplanar = true;
            for v in old {
                work.charge(1, cancel)?;
                coplanar &= orient3d(a, b, c, vertices[v as usize]) == Equal;
            }
            if coplanar {
                duplicate = true;
                break;
            }
        }
        if !duplicate {
            if planes.len() >= work.limits.max_planes {
                return Err(resource("support plane count"));
            }
            work.push(&mut planes, i)?;
        }
    }
    for i in 0..faces.len() {
        for j in 0..i {
            work.charge(1, cancel)?;
            contacts(mesh, faces[i], faces[j], i, work, cancel)?;
            contacts(mesh, faces[j], faces[i], j, work, cancel)?;
        }
    }
    Ok(planes)
}

// Flatten using any projection with a nonzero exact triangle orientation. No
// computed normal or epsilon selects whether a point belongs to the triangle.
fn projection(
    points: [Point3; 3],
    work: &mut Work,
    cancel: &mut Cancellation<'_>,
) -> Result<usize, HollowError> {
    for axis in 0..3 {
        work.charge(1, cancel)?;
        let [a, b, c] = points.map(|p| project(p, axis));
        if orient3d(a, b, c, Point3::new(0., 0., 1.)) != Equal {
            return Ok(axis);
        }
    }
    Err(invalid("exactly degenerate triangle", None))
}
fn project(p: Point3, axis: usize) -> Point3 {
    let xyz = p.coordinates();
    Point3::new(xyz[(axis + 1) % 3], xyz[(axis + 2) % 3], 0.)
}
fn orient(
    a: Point3,
    b: Point3,
    c: Point3,
    work: &mut Work,
    cancel: &mut Cancellation<'_>,
) -> Result<Ordering, HollowError> {
    work.charge(1, cancel)?;
    Ok(orient3d(a, b, c, Point3::new(0., 0., 1.)))
}
fn inside(
    p: Point3,
    t: [Point3; 3],
    work: &mut Work,
    cancel: &mut Cancellation<'_>,
) -> Result<bool, HollowError> {
    let signs = [
        orient(t[0], t[1], p, work, cancel)?,
        orient(t[1], t[2], p, work, cancel)?,
        orient(t[2], t[0], p, work, cancel)?,
    ];
    Ok(!(signs.contains(&Less) && signs.contains(&Greater)))
}
fn contacts(
    mesh: &TriangleMesh,
    source_face: [u32; 3],
    target_face: [u32; 3],
    face: usize,
    work: &mut Work,
    cancel: &mut Cancellation<'_>,
) -> Result<(), HollowError> {
    let points = mesh.vertices();
    let target = target_face.map(|v| points[v as usize]);
    let axis = projection(target, work, cancel)?;
    let flat = target.map(|start_point| project(start_point, axis));
    let mut zero = [false; 3];
    for corner in 0..3 {
        work.charge(1, cancel)?;
        zero[corner] = orient3d(
            target[0],
            target[1],
            target[2],
            points[source_face[corner] as usize],
        ) == Equal;
        if zero[corner]
            && !target_face.contains(&source_face[corner])
            && inside(
                project(points[source_face[corner] as usize], axis),
                flat,
                work,
                cancel,
            )?
        {
            return Err(invalid(
                "geometric contact without shared indexed vertex",
                Some(face),
            ));
        }
    }
    // A supporting halfspace precludes an edge crossing source_face target plane unless
    // its endpoints both lie in that plane. Check those segments in exact 2D.
    for corner in 0..3 {
        let next = (corner + 1) % 3;
        if !zero[corner] || !zero[next] {
            continue;
        }
        let start_point = project(points[source_face[corner] as usize], axis);
        let end_point = project(points[source_face[next] as usize], axis);
        for edge in 0..3 {
            let edge_next = (edge + 1) % 3;
            let first = orient(start_point, end_point, flat[edge], work, cancel)?;
            let second = orient(start_point, end_point, flat[edge_next], work, cancel)?;
            let third = orient(flat[edge], flat[edge_next], start_point, work, cancel)?;
            let fourth = orient(flat[edge], flat[edge_next], end_point, work, cancel)?;
            if first != Equal
                && second != Equal
                && first != second
                && third != Equal
                && fourth != Equal
                && third != fourth
            {
                return Err(invalid("source triangles have crossing edges", Some(face)));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preparation::hollow::HollowLimits;

    fn tetrahedron(reverse: bool) -> TriangleMesh {
        let points = vec![
            Point3::new(0., 0., 0.),
            Point3::new(1., 0., 0.),
            Point3::new(0., 1., 0.),
            Point3::new(0., 0., 1.),
        ];
        let mut faces = vec![[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]];
        if reverse {
            for face in &mut faces {
                face.swap(1, 2);
            }
        }
        TriangleMesh::try_new(points, faces).expect("finite tetrahedron")
    }
    fn work() -> Work {
        Work {
            limits: HollowLimits::default(),
            used: 0,
            requested: 0,
        }
    }
    #[test]
    fn outward_tetrahedron_has_four_supporting_planes() {
        assert_eq!(
            validate(&tetrahedron(false), &mut work(), &mut Cancellation::never()).unwrap(),
            vec![0, 1, 2, 3]
        );
    }
    #[test]
    fn globally_reversed_surface_is_not_repaired() {
        assert!(matches!(
            validate(&tetrahedron(true), &mut work(), &mut Cancellation::never()),
            Err(HollowError::Unsupported { .. })
        ));
    }
    #[test]
    fn crossing_coplanar_edges_are_rejected() {
        let points = vec![
            Point3::new(0., 0., 0.),
            Point3::new(2., 0., 0.),
            Point3::new(0., 2., 0.),
            Point3::new(1., -1., 0.),
            Point3::new(1., 1., 0.),
            Point3::new(3., 1., 0.),
        ];
        let mesh = TriangleMesh::try_new(points, vec![[0, 1, 2], [3, 4, 5]]).unwrap();
        assert!(contacts(
            &mesh,
            [0, 1, 2],
            [3, 4, 5],
            0,
            &mut work(),
            &mut Cancellation::never()
        )
        .is_err());
    }
    #[test]
    fn ordinary_coplanar_quad_diagonal_is_allowed() {
        let points = vec![
            Point3::new(0., 0., 0.),
            Point3::new(1., 0., 0.),
            Point3::new(1., 1., 0.),
            Point3::new(0., 1., 0.),
        ];
        let mesh = TriangleMesh::try_new(points, vec![[0, 1, 2], [0, 2, 3]]).unwrap();
        contacts(
            &mesh,
            [0, 1, 2],
            [0, 2, 3],
            0,
            &mut work(),
            &mut Cancellation::never(),
        )
        .unwrap();
        contacts(
            &mesh,
            [0, 2, 3],
            [0, 1, 2],
            1,
            &mut work(),
            &mut Cancellation::never(),
        )
        .unwrap();
    }
}
