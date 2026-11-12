use super::{
    util::{self, UnionFind},
    ComponentPartition, ConnectedComponent, Degeneracy, DirectedEdge, EdgeIncidence, EdgeKind,
    TopologyError, VertexIncidence,
};
use crate::cancellation::{Cancellation, Checkpoint};
use crate::mesh::TriangleMesh;

pub(super) fn analyze(
    mesh: &TriangleMesh,
    excluded: &[Option<Degeneracy>],
    edges: &[EdgeIncidence],
    uses: &[DirectedEdge],
    (vertices, incident_faces): (&[VertexIncidence], &[usize]),
    through_vertices: bool,
    cancellation: &mut Cancellation<'_>,
) -> Result<ComponentPartition, TopologyError> {
    cancellation.check(Checkpoint::Components)?;
    let mut sets = UnionFind::new(mesh.triangles().len())?;
    if through_vertices {
        for (index, vertex) in vertices.iter().enumerate() {
            cancellation.poll(Checkpoint::Components, index)?;
            let mut first = None;
            for (index, &face) in incident_faces[vertex.faces.clone()].iter().enumerate() {
                cancellation.poll(Checkpoint::Components, index)?;
                if excluded[face].is_some() {
                    continue;
                }
                if let Some(first) = first {
                    sets.join(first, face);
                } else {
                    first = Some(face);
                }
            }
        }
    } else {
        for (index, edge) in edges.iter().enumerate() {
            cancellation.poll(Checkpoint::Components, index)?;
            let incidences = &uses[edge.uses.clone()];
            for (index, incidence) in incidences[1..].iter().enumerate() {
                cancellation.poll(Checkpoint::Components, index)?;
                sets.join(incidences[0].triangle, incidence.triangle);
            }
        }
    }
    finalize(mesh, excluded, edges, uses, &mut sets, cancellation)
}

fn finalize(
    mesh: &TriangleMesh,
    excluded: &[Option<Degeneracy>],
    edges: &[EdgeIncidence],
    uses: &[DirectedEdge],
    sets: &mut UnionFind,
    cancellation: &mut Cancellation<'_>,
) -> Result<ComponentPartition, TopologyError> {
    cancellation.check(Checkpoint::Components)?;
    let mut root_ids = util::filled(excluded.len(), usize::MAX)?;
    let mut face_ids = util::filled(excluded.len(), usize::MAX)?;
    let mut components = Vec::new();
    let mut members = Vec::new();
    let mut vertex_members = Vec::new();
    for (face, exclusion) in excluded.iter().enumerate() {
        cancellation.poll(Checkpoint::Components, face)?;
        if exclusion.is_some() {
            continue;
        }
        let root = sets.root(face);
        let id = if root_ids[root] == usize::MAX {
            root_ids[root] = components.len();
            util::push(
                &mut components,
                ConnectedComponent {
                    triangle_count: 0,
                    vertex_count: 0,
                    edge_count: 0,
                    boundary_edges: 0,
                    two_face_edges: 0,
                    non_manifold_edges: 0,
                    winding_conflicts: 0,
                    triangles: 0..0,
                    vertices: 0..0,
                },
            )?;
            root_ids[root]
        } else {
            root_ids[root]
        };
        face_ids[face] = id;
        components[id].triangle_count += 1;
        util::push(&mut members, (id, face))?;
        for vertex in mesh.triangles()[face] {
            util::push(&mut vertex_members, (id, vertex))?;
        }
    }
    cancellation.check(Checkpoint::ComponentSorting)?;
    members.sort_unstable();
    cancellation.check(Checkpoint::ComponentSorting)?;
    vertex_members.sort_unstable();
    cancellation.check(Checkpoint::ComponentSorting)?;
    vertex_members.dedup();
    cancellation.check(Checkpoint::ComponentSorting)?;
    for (index, &(id, _)) in vertex_members.iter().enumerate() {
        cancellation.poll(Checkpoint::Components, index)?;
        components[id].vertex_count += 1;
    }
    for (index, edge) in edges.iter().enumerate() {
        cancellation.poll(Checkpoint::Components, index)?;
        let component = &mut components[face_ids[uses[edge.uses.start].triangle]];
        component.edge_count += 1;
        match edge.kind {
            EdgeKind::Boundary => component.boundary_edges += 1,
            EdgeKind::TwoFace => component.two_face_edges += 1,
            EdgeKind::NonManifold => component.non_manifold_edges += 1,
        }
        if edge.winding_conflict {
            component.winding_conflicts += 1;
        }
    }
    let mut face_offset = 0;
    let mut vertex_offset = 0;
    for (index, component) in components.iter_mut().enumerate() {
        cancellation.poll(Checkpoint::Components, index)?;
        component.triangles = face_offset..face_offset + component.triangle_count;
        component.vertices = vertex_offset..vertex_offset + component.vertex_count;
        face_offset += component.triangle_count;
        vertex_offset += component.vertex_count;
    }
    let mut triangles = Vec::new();
    util::reserve(&mut triangles, members.len())?;
    for (index, (_, face)) in members.into_iter().enumerate() {
        cancellation.poll(Checkpoint::Components, index)?;
        triangles.push(face);
    }
    let mut vertices = Vec::new();
    util::reserve(&mut vertices, vertex_members.len())?;
    for (index, (_, vertex)) in vertex_members.into_iter().enumerate() {
        cancellation.poll(Checkpoint::Components, index)?;
        vertices.push(vertex);
    }
    Ok(ComponentPartition {
        components,
        triangles,
        vertices,
    })
}
