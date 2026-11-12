use super::{
    util::{self, UnionFind},
    Degeneracy, TopologyError, TopologyIssue, VertexDefect, VertexIncidence, VertexKind,
};
use crate::cancellation::{Cancellation, Checkpoint};
use crate::mesh::TriangleMesh;

pub(super) fn analyze(
    mesh: &TriangleMesh,
    excluded: &[Option<Degeneracy>],
    issues: &mut Vec<TopologyIssue>,
    cancellation: &mut Cancellation<'_>,
) -> Result<(Vec<VertexIncidence>, Vec<usize>), TopologyError> {
    let (offsets, faces) = index(mesh, cancellation)?;
    let mut vertices = Vec::new();
    util::reserve(&mut vertices, mesh.vertices().len())?;
    let mut links = Vec::new();
    for vertex in 0..mesh.vertices().len() {
        cancellation.poll(Checkpoint::VertexNeighborhoods, vertex)?;
        links.clear();
        let range = offsets[vertex]..offsets[vertex + 1];
        let mut excluded_faces = 0;
        for (index, &triangle) in faces[range.clone()].iter().enumerate() {
            cancellation.poll(Checkpoint::VertexNeighborhoods, index)?;
            if excluded[triangle].is_some() {
                excluded_faces += 1;
                continue;
            }
            let face = mesh.triangles()[triangle];
            // The face is nondegenerate: exactly two corners differ from this source vertex.
            let opposite = if face[0] as usize == vertex {
                [face[1], face[2]]
            } else if face[1] as usize == vertex {
                [face[0], face[2]]
            } else {
                [face[0], face[1]]
            };
            util::push(
                &mut links,
                (opposite[0].min(opposite[1]), opposite[0].max(opposite[1])),
            )?;
        }
        let (partial_kind, usable_link_components) = classify(&mut links, cancellation)?;
        let kind = if range.is_empty() {
            VertexKind::Isolated
        } else if excluded_faces > 0 {
            VertexKind::Indeterminate
        } else {
            partial_kind
        };
        match kind {
            VertexKind::Isolated => util::push(issues, TopologyIssue::IsolatedVertex { vertex })?,
            VertexKind::Indeterminate => {
                util::push(issues, TopologyIssue::IndeterminateVertex { vertex })?;
            }
            VertexKind::NonManifold(reason) => {
                util::push(issues, TopologyIssue::NonManifoldVertex { vertex, reason })?;
            }
            VertexKind::InteriorManifold | VertexKind::BoundaryManifold => {}
        }
        vertices.push(VertexIncidence {
            kind,
            usable_faces: links.len(),
            excluded_faces,
            usable_link_components,
            faces: range,
        });
    }
    Ok((vertices, faces))
}

fn index(
    mesh: &TriangleMesh,
    cancellation: &mut Cancellation<'_>,
) -> Result<(Vec<usize>, Vec<usize>), TopologyError> {
    cancellation.check(Checkpoint::VertexIndex)?;
    let count = mesh
        .vertices()
        .len()
        .checked_add(1)
        .ok_or(TopologyError::SizeOverflow)?;
    let mut offsets = util::filled(count, 0_usize)?;
    for (index, face) in mesh.triangles().iter().enumerate() {
        cancellation.poll(Checkpoint::VertexIndex, index)?;
        for (corner, &v) in face.iter().enumerate() {
            if !face[..corner].contains(&v) {
                offsets[v as usize + 1] += 1;
            }
        }
    }
    for vertex in 1..offsets.len() {
        cancellation.poll(Checkpoint::VertexIndex, vertex)?;
        offsets[vertex] = offsets[vertex]
            .checked_add(offsets[vertex - 1])
            .ok_or(TopologyError::SizeOverflow)?;
    }
    let mut faces = util::filled(offsets[mesh.vertices().len()], 0)?;
    let mut cursors = Vec::new();
    util::reserve(&mut cursors, mesh.vertices().len())?;
    cursors.extend_from_slice(&offsets[..mesh.vertices().len()]);
    for (triangle, face) in mesh.triangles().iter().enumerate() {
        cancellation.poll(Checkpoint::VertexIndex, triangle)?;
        for (corner, &v) in face.iter().enumerate() {
            if !face[..corner].contains(&v) {
                let cursor = &mut cursors[v as usize];
                faces[*cursor] = triangle;
                *cursor += 1;
            }
        }
    }
    Ok((offsets, faces))
}

fn classify(
    links: &mut [(u32, u32)],
    cancellation: &mut Cancellation<'_>,
) -> Result<(VertexKind, usize), TopologyError> {
    cancellation.check(Checkpoint::VertexNeighborhoods)?;
    if links.is_empty() {
        return Ok((VertexKind::Indeterminate, 0));
    }
    cancellation.check(Checkpoint::VertexSorting)?;
    links.sort_unstable();
    cancellation.check(Checkpoint::VertexSorting)?;
    let mut duplicate = false;
    for (index, pair) in links.windows(2).enumerate() {
        cancellation.poll(Checkpoint::VertexNeighborhoods, index)?;
        if pair[0] == pair[1] {
            duplicate = true;
            break;
        }
    }
    let mut nodes = Vec::new();
    util::reserve(
        &mut nodes,
        links
            .len()
            .checked_mul(2)
            .ok_or(TopologyError::SizeOverflow)?,
    )?;
    for (index, &(a, b)) in links.iter().enumerate() {
        cancellation.poll(Checkpoint::VertexNeighborhoods, index)?;
        nodes.extend([a, b]);
    }
    cancellation.check(Checkpoint::VertexSorting)?;
    nodes.sort_unstable();
    cancellation.check(Checkpoint::VertexSorting)?;
    nodes.dedup();
    cancellation.check(Checkpoint::VertexSorting)?;
    let mut degree = util::filled(nodes.len(), 0_usize)?;
    let mut sets = UnionFind::new(nodes.len())?;
    for (index, &(a, b)) in links.iter().enumerate() {
        cancellation.poll(Checkpoint::VertexNeighborhoods, index)?;
        // Endpoints were inserted above. partition_point avoids an unnecessary panic/unwrapping.
        let a = nodes.partition_point(|&node| node < a);
        let b = nodes.partition_point(|&node| node < b);
        degree[a] += 1;
        degree[b] += 1;
        sets.join(a, b);
    }
    let mut components = 0;
    for node in 0..nodes.len() {
        cancellation.poll(Checkpoint::VertexNeighborhoods, node)?;
        components += usize::from(sets.root(node) == node);
    }
    let count_degree = |value, cancellation: &mut Cancellation<'_>| {
        util::count(
            &degree,
            |&d| d == value,
            cancellation,
            Checkpoint::VertexNeighborhoods,
        )
    };
    let kind = if components > 1 {
        VertexKind::NonManifold(VertexDefect::DisconnectedFans)
    } else if duplicate {
        VertexKind::NonManifold(VertexDefect::DuplicateLinkEdge)
    } else if nodes.len() >= 3
        && util::all(
            &degree,
            |&d| d == 2,
            cancellation,
            Checkpoint::VertexNeighborhoods,
        )?
    {
        VertexKind::InteriorManifold
    } else if count_degree(1, cancellation)? == 2
        && util::all(
            &degree,
            |&d| d == 1 || d == 2,
            cancellation,
            Checkpoint::VertexNeighborhoods,
        )?
    {
        VertexKind::BoundaryManifold
    } else {
        VertexKind::NonManifold(VertexDefect::BranchedLink)
    };
    Ok((kind, components))
}
