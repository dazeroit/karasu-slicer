use super::{
    components, util, vertex, CheckStatus, Degeneracy, DirectedEdge, EdgeIncidence, EdgeKind,
    EdgeSummary, MeshDiagnostic, TopologyError, TopologyIssue, TopologyLimits, TopologyReport,
    TopologyResource, TopologySummary, VertexIncidence, VertexKind,
};
use crate::cancellation::{Cancellation, Checkpoint};
use crate::mesh::TriangleMesh;

pub(in crate::mesh) fn analyze(
    mesh: &TriangleMesh,
    limits: TopologyLimits,
    cancellation: &mut Cancellation<'_>,
) -> Result<TopologyReport, TopologyError> {
    cancellation.check(Checkpoint::TopologyStart)?;
    check_limits(mesh, limits)?;
    // These bounds also bound every later degree, incidence, and component counter.
    mesh.triangles()
        .len()
        .checked_mul(3)
        .ok_or(TopologyError::SizeOverflow)?;
    mesh.vertices()
        .len()
        .checked_add(1)
        .ok_or(TopologyError::SizeOverflow)?;
    let geometry_diagnostics = mesh.diagnostics_with_cancellation(cancellation)?;
    cancellation.check(Checkpoint::TopologyPreparation)?;
    let mut excluded = util::filled(mesh.triangles().len(), None)?;
    let mut issues = Vec::new();
    for (index, diagnostic) in geometry_diagnostics.iter().enumerate() {
        cancellation.poll(Checkpoint::TopologyPreparation, index)?;
        if let MeshDiagnostic::DegenerateTriangle { triangle, kind } = *diagnostic {
            excluded[triangle] = Some(kind);
            util::push(
                &mut issues,
                TopologyIssue::ExcludedTriangle {
                    triangle,
                    reason: kind,
                },
            )?;
        }
    }
    let (edges, uses) = edge_index(mesh, &excluded, &mut issues, cancellation)?;
    let (vertices, incident_faces) = vertex::analyze(mesh, &excluded, &mut issues, cancellation)?;
    let edge_components = components::analyze(
        mesh,
        &excluded,
        &edges,
        &uses,
        (&vertices, &incident_faces),
        false,
        cancellation,
    )?;
    let vertex_components = components::analyze(
        mesh,
        &excluded,
        &edges,
        &uses,
        (&vertices, &incident_faces),
        true,
        cancellation,
    )?;
    let summary = summarize(
        mesh,
        &excluded,
        &geometry_diagnostics,
        &edges,
        &vertices,
        [
            edge_components.components.len(),
            vertex_components.components.len(),
        ],
        cancellation,
    )?;
    cancellation.check(Checkpoint::TopologyComplete)?;
    Ok(TopologyReport {
        summary,
        edges,
        uses,
        vertices,
        incident_faces,
        issues,
        geometry_diagnostics,
        edge_components,
        vertex_components,
    })
}

/// Edge closure and orientation counts with the full analysis's definitions: degenerate triangles
/// (the `DegenerateTriangle` diagnostic) are excluded, edges are keyed by vertex indices, and a
/// two-face edge is a winding conflict when both uses run in the same direction.
pub(in crate::mesh) fn edge_summary(
    mesh: &TriangleMesh,
    limits: TopologyLimits,
    cancellation: &mut Cancellation<'_>,
) -> Result<EdgeSummary, TopologyError> {
    cancellation.check(Checkpoint::TopologyStart)?;
    check_limits(mesh, limits)?;
    let triangles = mesh.triangles();
    let mut uses: Vec<u64> = Vec::new();
    util::reserve(
        &mut uses,
        triangles
            .len()
            .checked_mul(3)
            .ok_or(TopologyError::SizeOverflow)?,
    )?;
    let mut excluded_triangles = 0;
    for (triangle, &[a, b, c]) in triangles.iter().enumerate() {
        cancellation.poll(Checkpoint::EdgeIncidence, triangle)?;
        let points = [a, b, c].map(|index| mesh.vertices()[index as usize]);
        if matches!(
            crate::mesh::diagnostics::normal(points),
            crate::mesh::FaceNormal::Undefined(_)
        ) {
            excluded_triangles += 1;
            continue;
        }
        for (from, to) in [(a, b), (b, c), (c, a)] {
            uses.push(u64::from(from) << 32 | u64::from(to));
        }
    }
    // Undirected key: the smaller index in the high word.
    let key = |u: u64| {
        let (from, to) = (u >> 32, u & 0xffff_ffff);
        from.min(to) << 32 | from.max(to)
    };
    cancellation.check(Checkpoint::EdgeSorting)?;
    // The key covers the whole element.
    crate::parallel::sort_by_key(&mut uses, |&u| (key(u), u));
    cancellation.check(Checkpoint::EdgeSorting)?;
    let mut summary = EdgeSummary {
        usable_triangles: triangles.len() - excluded_triangles,
        excluded_triangles,
        edges: 0,
        boundary_edges: 0,
        two_face_edges: 0,
        non_manifold_edges: 0,
        winding_conflicts: 0,
    };
    let mut start = 0;
    while start < uses.len() {
        cancellation.poll(Checkpoint::EdgeIncidence, start)?;
        let mut end = start + 1;
        while end < uses.len() && key(uses[end]) == key(uses[start]) {
            end += 1;
        }
        summary.edges += 1;
        match end - start {
            1 => summary.boundary_edges += 1,
            2 => {
                summary.two_face_edges += 1;
                // Equal directed values: both uses leave the same vertex.
                if uses[start] == uses[start + 1] {
                    summary.winding_conflicts += 1;
                }
            }
            _ => summary.non_manifold_edges += 1,
        }
        start = end;
    }
    cancellation.check(Checkpoint::TopologyComplete)?;
    Ok(summary)
}

fn check_limits(mesh: &TriangleMesh, limits: TopologyLimits) -> Result<(), TopologyError> {
    for (count, limit, resource) in [
        (
            mesh.vertices().len(),
            limits.max_vertices,
            TopologyResource::Vertices,
        ),
        (
            mesh.triangles().len(),
            limits.max_triangles,
            TopologyResource::Triangles,
        ),
    ] {
        if count > limit {
            return Err(TopologyError::LimitExceeded { resource, limit });
        }
    }
    Ok(())
}

fn edge_index(
    mesh: &TriangleMesh,
    excluded: &[Option<Degeneracy>],
    issues: &mut Vec<TopologyIssue>,
    cancellation: &mut Cancellation<'_>,
) -> Result<(Vec<EdgeIncidence>, Vec<DirectedEdge>), TopologyError> {
    cancellation.check(Checkpoint::EdgeIncidence)?;
    let usable = util::count(
        excluded,
        Option::is_none,
        cancellation,
        Checkpoint::EdgeIncidence,
    )?;
    let mut uses = Vec::new();
    util::reserve(
        &mut uses,
        usable.checked_mul(3).ok_or(TopologyError::SizeOverflow)?,
    )?;
    for (triangle, &[a, b, c]) in mesh.triangles().iter().enumerate() {
        cancellation.poll(Checkpoint::EdgeIncidence, triangle)?;
        if excluded[triangle].is_some() {
            continue;
        }
        for (local_edge, from, to) in [(0, a, b), (1, b, c), (2, c, a)] {
            uses.push(DirectedEdge {
                triangle,
                local_edge,
                from,
                to,
            });
        }
    }
    // The complete sort key is unique, so unstable sorting still has a unique observable result.
    cancellation.check(Checkpoint::EdgeSorting)?;
    crate::parallel::sort_by_key(&mut uses, |u| (u.key(), u.triangle, u.local_edge));
    cancellation.check(Checkpoint::EdgeSorting)?;
    let mut edges = Vec::new();
    let mut start = 0;
    while start < uses.len() {
        cancellation.poll(Checkpoint::EdgeIncidence, start)?;
        let key = uses[start].key();
        let mut end = start + 1;
        while end < uses.len() && uses[end].key() == key {
            cancellation.poll(Checkpoint::EdgeIncidence, end)?;
            end += 1;
        }
        let kind = match end - start {
            1 => EdgeKind::Boundary,
            2 => EdgeKind::TwoFace,
            _ => EdgeKind::NonManifold,
        };
        let winding_conflict =
            kind == EdgeKind::TwoFace && uses[start].from == uses[start + 1].from;
        let edge = edges.len();
        match kind {
            EdgeKind::Boundary => util::push(issues, TopologyIssue::BoundaryEdge { edge })?,
            EdgeKind::NonManifold => util::push(issues, TopologyIssue::NonManifoldEdge { edge })?,
            EdgeKind::TwoFace => {}
        }
        if winding_conflict {
            util::push(
                issues,
                TopologyIssue::WindingConflict {
                    edge,
                    triangles: [uses[start].triangle, uses[start + 1].triangle],
                },
            )?;
        }
        util::push(
            &mut edges,
            EdgeIncidence {
                key,
                kind,
                winding_conflict,
                uses: start..end,
            },
        )?;
        start = end;
    }
    Ok((edges, uses))
}

fn status(failed: bool, incomplete: bool, empty: bool) -> CheckStatus {
    if failed {
        CheckStatus::Failed
    } else if incomplete {
        CheckStatus::Indeterminate
    } else if empty {
        CheckStatus::NotApplicable
    } else {
        CheckStatus::Satisfied
    }
}

fn summarize(
    mesh: &TriangleMesh,
    excluded: &[Option<Degeneracy>],
    diagnostics: &[MeshDiagnostic],
    edges: &[EdgeIncidence],
    vertices: &[VertexIncidence],
    [edge_components, vertex_components]: [usize; 2],
    cancellation: &mut Cancellation<'_>,
) -> Result<TopologySummary, TopologyError> {
    cancellation.check(Checkpoint::TopologySummary)?;
    let excluded_triangles = util::count(
        excluded,
        Option::is_some,
        cancellation,
        Checkpoint::TopologySummary,
    )?;
    let usable_triangles = mesh.triangles().len() - excluded_triangles;
    let boundary_edges = util::count(
        edges,
        |e| e.kind == EdgeKind::Boundary,
        cancellation,
        Checkpoint::TopologySummary,
    )?;
    let non_manifold_edges = util::count(
        edges,
        |e| e.kind == EdgeKind::NonManifold,
        cancellation,
        Checkpoint::TopologySummary,
    )?;
    let winding_conflicts = util::count(
        edges,
        |e| e.winding_conflict,
        cancellation,
        Checkpoint::TopologySummary,
    )?;
    let count_vertices = |kind, cancellation: &mut Cancellation<'_>| {
        util::count(
            vertices,
            |v| v.kind == kind,
            cancellation,
            Checkpoint::TopologySummary,
        )
    };
    let isolated_vertices = count_vertices(VertexKind::Isolated, cancellation)?;
    let non_manifold_vertices = util::count(
        vertices,
        |v| matches!(v.kind, VertexKind::NonManifold(_)),
        cancellation,
        Checkpoint::TopologySummary,
    )?;
    let edge_manifold = status(
        non_manifold_edges > 0,
        excluded_triangles > 0,
        usable_triangles == 0,
    );
    let vertex_manifold = status(
        non_manifold_vertices > 0 || isolated_vertices > 0,
        excluded_triangles > 0,
        usable_triangles == 0,
    );
    let local_winding = status(
        winding_conflicts > 0,
        excluded_triangles > 0 || non_manifold_edges > 0,
        usable_triangles == 0,
    );
    Ok(TopologySummary {
        source_vertices: mesh.vertices().len(),
        source_triangles: mesh.triangles().len(),
        usable_triangles,
        excluded_triangles,
        duplicate_triangles: util::count(
            diagnostics,
            |d| matches!(d, MeshDiagnostic::DuplicateTriangle { .. }),
            cancellation,
            Checkpoint::TopologySummary,
        )?,
        edges: edges.len(),
        boundary_edges,
        two_face_edges: edges.len() - boundary_edges - non_manifold_edges,
        non_manifold_edges,
        winding_conflicts,
        interior_vertices: count_vertices(VertexKind::InteriorManifold, cancellation)?,
        boundary_vertices: count_vertices(VertexKind::BoundaryManifold, cancellation)?,
        non_manifold_vertices,
        indeterminate_vertices: count_vertices(VertexKind::Indeterminate, cancellation)?,
        isolated_vertices,
        edge_components,
        vertex_components,
        no_boundary_edges: boundary_edges == 0,
        edge_manifold,
        vertex_manifold,
        local_winding,
        closed_combinatorial_surface: usable_triangles > 0
            && edge_manifold == CheckStatus::Satisfied
            && vertex_manifold == CheckStatus::Satisfied
            && boundary_edges == 0,
    })
}
