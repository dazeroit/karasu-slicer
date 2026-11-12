//! Exact coplanar patch perimeter with coherent attached noncoplanar context.
use karasu_core::{
    geometry::Point3,
    mesh::TriangleMesh,
    slicing::{
        normalize::{
            normalize_section, IncidenceIssue, NormalizationLimits, Ownership, SourceFeature,
        },
        section_mesh_z, SectionLimits, SectionTransform,
    },
};

fn check(faces: Vec<[u32; 3]>, third: Point3, expected: Ownership, issue: Option<IncidenceIssue>) {
    let mesh = TriangleMesh::try_new(
        vec![
            Point3::new(0., 0., 0.),
            Point3::new(4., 0., 0.),
            third,
            Point3::new(0., 0., 1.),
        ],
        faces,
    )
    .unwrap();
    let raw = section_mesh_z(
        &mesh,
        0.,
        SectionTransform::Identity,
        SectionLimits::default(),
    )
    .unwrap();
    let view = normalize_section(&raw, NormalizationLimits::default()).unwrap();
    let group = view
        .groups()
        .iter()
        .find(|g| g.endpoints == [SourceFeature::Vertex(0), SourceFeature::Vertex(1)])
        .unwrap();
    assert_eq!(group.ownership, expected);
    assert_eq!(group.occurrences.len(), mesh.triangles().len());
    assert!(std::ptr::eq(view.raw(), std::ptr::from_ref(&raw)));
    if let Some(issue) = issue {
        assert!(group.issues.contains(issue));
    } else {
        assert!(group.issues.is_empty());
    }
}

#[test]
fn coherent_cap_side_pair_preserves_both_occurrences_as_patch_perimeter() {
    check(
        vec![[0, 1, 2], [1, 0, 3]],
        Point3::new(0., 4., 0.),
        Ownership::CoplanarPatchBoundary,
        None,
    );
}
#[test]
fn agreeing_shared_edge_winding_is_unresolved() {
    check(
        vec![[0, 1, 2], [0, 1, 3]],
        Point3::new(0., 4., 0.),
        Ownership::Unresolved,
        Some(IncidenceIssue::WindingConflict),
    );
}
#[test]
fn duplicate_cap_faces_are_not_hidden_by_side_context() {
    check(
        vec![[0, 1, 2], [0, 1, 2], [1, 0, 3]],
        Point3::new(0., 4., 0.),
        Ownership::Unresolved,
        Some(IncidenceIssue::NonManifoldEdge),
    );
}
#[test]
fn degenerate_cap_cannot_certify_perimeter() {
    check(
        vec![[0, 1, 2], [1, 0, 3]],
        Point3::new(2., 0., 0.),
        Ownership::Unresolved,
        Some(IncidenceIssue::DegenerateOrUnassessed),
    );
}
