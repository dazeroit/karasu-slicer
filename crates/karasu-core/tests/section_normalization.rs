//! Independent incidence/ownership oracles, no contour or filled-region inference.
#![allow(clippy::float_cmp)] // Exact source coordinates, zero and dyadic analytic expectations.

use karasu_core::cancellation::{Cancellation, Cancelled, Checkpoint};
use karasu_core::geometry::section::{EndpointOrigin, TrianglePlaneEvent};
use karasu_core::geometry::{Affine3, ObjectTransform, Point3, Rotation3, Vector3};
use karasu_core::mesh::{
    stl::{import_stl_path, ImportedStl, StlOptions, StlUnitPolicy},
    TriangleMesh,
};
use karasu_core::slicing::normalize::*;
use karasu_core::slicing::{section_mesh_z, MeshSection, SectionLimits, SectionTransform};
use std::{error::Error, path::PathBuf};

fn raw(mesh: &TriangleMesh) -> MeshSection {
    section_mesh_z(
        mesh,
        0.0,
        SectionTransform::Identity,
        SectionLimits::default(),
    )
    .unwrap()
}
fn norm(raw: &MeshSection) -> NormalizedSection<'_> {
    normalize_section(raw, NormalizationLimits::default()).unwrap()
}
fn mesh(vertices: &[[f64; 3]], faces: Vec<[u32; 3]>) -> TriangleMesh {
    TriangleMesh::try_new(
        vertices
            .iter()
            .map(|&[x, y, z]| Point3::new(x, y, z))
            .collect(),
        faces,
    )
    .unwrap()
}
fn square(faces: Vec<[u32; 3]>) -> TriangleMesh {
    mesh(
        &[
            [0.0, 0.0, 0.0],
            [4.0, 0.0, 0.0],
            [4.0, 4.0, 0.0],
            [0.0, 4.0, 0.0],
        ],
        faces,
    )
}
fn diagonal<'a>(view: &'a NormalizedSection<'_>) -> &'a SegmentGroup {
    view.groups()
        .iter()
        .find(|g| g.endpoints == [SourceFeature::Vertex(0), SourceFeature::Vertex(2)])
        .unwrap()
}
fn text<'a>(entry: &'a str, name: &str) -> &'a str {
    entry
        .split_once(&format!("\"{name}\":"))
        .unwrap()
        .1
        .trim_start()
        .strip_prefix('"')
        .unwrap()
        .split('"')
        .next()
        .unwrap()
}
fn fixture(id: &str, variant: Option<&str>) -> ImportedStl {
    let manifest = include_str!("../../../fixtures/reference/manifest.json");
    let entry = manifest
        .split("\"fixture_id\":")
        .skip(1)
        .find(|entry| {
            text(&format!("\"fixture_id\":{entry}"), "fixture_id") == id
                && variant.map_or_else(
                    || {
                        entry
                            .split_once("\"variant\":")
                            .unwrap()
                            .1
                            .trim_start()
                            .starts_with("null")
                    },
                    |v| text(entry, "variant") == v,
                )
        })
        .unwrap();
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/reference")
        .join(text(entry, "filename"));
    import_stl_path(path, StlOptions::new(StlUnitPolicy::AssumeMillimeters)).unwrap()
}

fn check_partition(view: &NormalizedSection<'_>) {
    let mut feature_uses = view.feature_order().to_vec();
    feature_uses.sort_unstable();
    assert_eq!(
        feature_uses,
        (0..view.observations().len()).collect::<Vec<_>>()
    );
    let mut segment_uses = view.segment_order().to_vec();
    segment_uses.sort_unstable();
    assert_eq!(segment_uses, (0..view.segments().len()).collect::<Vec<_>>());
    assert_eq!(
        view.features()
            .iter()
            .map(|g| g.observations.len())
            .sum::<usize>(),
        view.observations().len()
    );
    assert_eq!(
        view.coordinates()
            .iter()
            .map(|g| g.observations.len())
            .sum::<usize>(),
        view.observations().len()
    );
    assert_eq!(
        view.groups()
            .iter()
            .map(|g| g.occurrences.len())
            .sum::<usize>(),
        view.segments().len()
    );
    for g in view.groups() {
        for &id in &view.segment_order()[g.occurrences.clone()] {
            assert_eq!(
                view.segments()[id]
                    .endpoints
                    .map(|p| view.observations()[p].feature),
                g.endpoints
            );
        }
    }
    assert_eq!(view.summary().coordinate_conflicts, 0);
}

#[test]
fn empty_input_borrows_raw_and_has_empty_partitions() {
    let mesh = mesh(&[], vec![]);
    let raw = raw(&mesh);
    let view = norm(&raw);
    assert!(std::ptr::eq(view.raw(), std::ptr::from_ref(&raw)));
    assert_eq!(view.plane_z(), 0.0);
    assert_eq!(view.summary(), NormalizationSummary::default());
    check_partition(&view);
    assert!(normalize_section(
        &raw,
        NormalizationLimits {
            max_events: 0,
            max_incidences: 0,
            max_payload_bytes: 0
        }
    )
    .is_ok());
    let mut predicate = |p| p == Checkpoint::NormalizationComplete;
    assert_eq!(
        normalize_section_with_cancellation(
            &raw,
            NormalizationLimits::default(),
            &mut Cancellation::new(&mut predicate)
        ),
        Err(NormalizationError::Cancelled(Cancelled {
            checkpoint: Checkpoint::NormalizationComplete
        }))
    );
}

#[test]
fn planar_square_retains_six_occurrences_and_classifies_only_the_internal_diagonal() {
    let mesh = square(vec![[0, 1, 2], [0, 2, 3]]);
    let raw = raw(&mesh);
    let view = norm(&raw);
    assert_eq!(view.segments().len(), 6);
    assert_eq!(view.groups().len(), 5);
    assert_eq!(view.features().len(), 4);
    assert_eq!(view.summary().internal_patch_edges, 1);
    assert_eq!(view.summary().unresolved_groups, 0);
    assert_eq!(diagonal(&view).ownership, Ownership::CoplanarPatchInternal);
    assert_eq!(diagonal(&view).occurrences.len(), 2);
    let exterior = view
        .groups()
        .iter()
        .filter(|g| g.ownership == Ownership::CoplanarPatchBoundary)
        .map(|g| g.endpoints)
        .collect::<Vec<_>>();
    assert_eq!(
        exterior,
        vec![
            [SourceFeature::Vertex(0), SourceFeature::Vertex(1)],
            [SourceFeature::Vertex(0), SourceFeature::Vertex(3)],
            [SourceFeature::Vertex(1), SourceFeature::Vertex(2)],
            [SourceFeature::Vertex(2), SourceFeature::Vertex(3)]
        ]
    );
    let directions = view.segment_order()[diagonal(&view).occurrences.clone()]
        .iter()
        .map(|&i| view.segments()[i].reversed)
        .collect::<Vec<_>>();
    assert_eq!(directions, vec![true, false]);
    check_partition(&view);
}

#[test]
fn one_coplanar_triangle_is_three_local_patch_boundary_candidates() {
    let mesh = square(vec![[0, 1, 2]]);
    let raw = raw(&mesh);
    let view = norm(&raw);
    assert_eq!(view.groups().len(), 3);
    assert!(view
        .groups()
        .iter()
        .all(|g| g.ownership == Ownership::CoplanarPatchBoundary));
    assert_eq!(view.summary().coplanar_faces, 1);
}

#[test]
fn duplicate_and_oppositely_wound_duplicate_faces_never_cancel_by_parity() {
    for faces in [vec![[0, 1, 2], [0, 1, 2]], vec![[0, 1, 2], [2, 1, 0]]] {
        let mesh = square(faces);
        let raw = raw(&mesh);
        let view = norm(&raw);
        assert_eq!(view.segments().len(), 6);
        assert_eq!(view.groups().len(), 3);
        for g in view.groups() {
            assert_eq!(g.ownership, Ownership::Unresolved);
            assert_eq!(g.occurrences.len(), 2);
            assert!(g.issues.contains(IncidenceIssue::DuplicateFace));
            assert!(g.issues.contains(IncidenceIssue::OverlappingCoplanar));
        }
    }
}

#[test]
fn inconsistent_patch_winding_is_explicitly_unresolved() {
    let mesh = square(vec![[0, 1, 2], [0, 3, 2]]);
    let raw = raw(&mesh);
    let view = norm(&raw);
    assert_eq!(diagonal(&view).ownership, Ownership::Unresolved);
    assert!(diagonal(&view)
        .issues
        .contains(IncidenceIssue::WindingConflict));
}

#[test]
fn same_side_third_vertices_fail_internal_patch_invariants_even_with_opposed_edge_uses() {
    let mesh = mesh(
        &[
            [0.0, 0.0, 0.0],
            [4.0, 0.0, 0.0],
            [1.0, 3.0, 0.0],
            [2.0, 2.0, 0.0],
        ],
        vec![[0, 1, 2], [1, 0, 3]],
    );
    let raw = raw(&mesh);
    let view = norm(&raw);
    let group = view
        .groups()
        .iter()
        .find(|g| g.endpoints == [SourceFeature::Vertex(0), SourceFeature::Vertex(1)])
        .unwrap();
    assert_eq!(group.ownership, Ownership::Unresolved);
    assert!(group.issues.contains(IncidenceIssue::OverlappingCoplanar));
    assert!(!group.issues.contains(IncidenceIssue::WindingConflict));
}

#[test]
fn coherent_coplanar_face_and_noncoplanar_face_retain_a_patch_boundary() {
    let mesh = mesh(
        &[
            [0.0, 0.0, 0.0],
            [4.0, 0.0, 0.0],
            [0.0, 4.0, 0.0],
            [0.0, 0.0, 2.0],
        ],
        vec![[0, 1, 2], [1, 0, 3]],
    );
    let raw = raw(&mesh);
    let view = norm(&raw);
    assert_eq!(view.segments().len(), 4);
    let group = view
        .groups()
        .iter()
        .find(|g| g.endpoints == [SourceFeature::Vertex(0), SourceFeature::Vertex(1)])
        .unwrap();
    assert_eq!(group.ownership, Ownership::CoplanarPatchBoundary);
    assert!(group.issues.is_empty());
    assert_eq!(group.occurrences.len(), 2);
    assert_eq!(group.source_faces, 2);
}

#[test]
fn more_than_two_faces_is_not_an_internal_patch_or_an_even_odd_boundary() {
    let mesh = mesh(
        &[
            [0.0, 0.0, 0.0],
            [4.0, 0.0, 0.0],
            [1.0, 3.0, 0.0],
            [1.0, -3.0, 0.0],
            [2.0, 4.0, 0.0],
        ],
        vec![[0, 1, 2], [1, 0, 3], [0, 1, 4]],
    );
    let raw = raw(&mesh);
    let view = norm(&raw);
    let g = view
        .groups()
        .iter()
        .find(|g| g.endpoints == [SourceFeature::Vertex(0), SourceFeature::Vertex(1)])
        .unwrap();
    assert_eq!(g.source_faces, 3);
    assert_eq!(g.occurrences.len(), 3);
    assert_eq!(g.ownership, Ownership::Unresolved);
    assert!(g.issues.contains(IncidenceIssue::NonManifoldEdge));
}

#[test]
fn tangent_point_is_an_observation_not_a_segment() {
    let mesh = mesh(
        &[[0.0, 0.0, 0.0], [4.0, 0.0, 1.0], [0.0, 4.0, 1.0]],
        vec![[0, 1, 2]],
    );
    let raw = raw(&mesh);
    let view = norm(&raw);
    assert_eq!(view.observations().len(), 1);
    assert_eq!(view.observations()[0].feature, SourceFeature::Vertex(0));
    assert_eq!(view.observations()[0].segment_ends, 0);
    assert_eq!(view.summary().tangent_events, 1);
    assert_eq!(view.segments(), []);
}

#[test]
fn coherent_opposite_side_on_edge_faces_are_candidates_and_same_side_is_unresolved() {
    for height in [-1.0, 1.0] {
        let mesh = mesh(
            &[
                [0.0, 0.0, 0.0],
                [4.0, 0.0, 0.0],
                [0.0, 4.0, 1.0],
                [0.0, -4.0, height],
            ],
            vec![[0, 1, 2], [1, 0, 3]],
        );
        let raw = raw(&mesh);
        let view = norm(&raw);
        assert_eq!(view.groups().len(), 1);
        assert_eq!(view.segments().len(), 2);
        if height < 0.0 {
            assert_eq!(
                view.groups()[0].ownership,
                Ownership::OnPlaneCrossingCandidate
            );
            assert!(view.groups()[0].issues.is_empty());
        } else {
            assert_eq!(view.groups()[0].ownership, Ownership::Unresolved);
            assert!(view.groups()[0]
                .issues
                .contains(IncidenceIssue::OnEdgeOwnership));
        }
    }
}

#[test]
fn cube_interior_has_square_incidence_without_extracting_a_ring() {
    let cube = fixture("FX-01", None);
    let raw = section_mesh_z(
        &cube.mesh,
        10.0,
        SectionTransform::Identity,
        SectionLimits::default(),
    )
    .unwrap();
    let view = norm(&raw);
    assert_eq!(view.groups().len(), 8);
    assert_eq!(view.features().len(), 8);
    assert_eq!(view.segments().len(), 8);
    for g in view.groups() {
        assert_eq!(g.ownership, Ownership::CrossingCandidate);
        assert_eq!(g.occurrences.len(), 1);
    }
    for feature in view.features() {
        assert!(matches!(feature.feature, SourceFeature::Edge(_)));
        assert_eq!(feature.segment_ends, 2);
        let CoordinateAgreement::Agreed(p) = feature.coordinates else {
            panic!("agreement")
        };
        assert!(p.x.abs() == 10.0 || p.y.abs() == 10.0);
        assert_eq!(p.z, 10.0);
    }
    check_partition(&view);
}

#[test]
fn cylinder_and_oblique_sections_keep_shared_crossing_edge_identity() {
    for (id, height, expected) in [("FX-02", 10.0, 256), ("FX-05", 17.0, 8)] {
        let fixture = fixture(id, None);
        let raw = section_mesh_z(
            &fixture.mesh,
            height,
            SectionTransform::Identity,
            SectionLimits::default(),
        )
        .unwrap();
        let view = norm(&raw);
        assert_eq!(view.segments().len(), expected);
        assert_eq!(view.groups().len(), expected);
        assert!(view.features().iter().all(|g| g.segment_ends == 2));
        assert_eq!(view.summary().unresolved_groups, 0);
        check_partition(&view);
    }
}

#[test]
fn disconnected_identical_coordinates_remain_different_features_and_segment_groups() {
    let vertices = [
        [0.0, 0.0, -1.0],
        [4.0, 0.0, 1.0],
        [0.0, 4.0, 1.0],
        [0.0, 0.0, -1.0],
        [4.0, 0.0, 1.0],
        [0.0, 4.0, 1.0],
    ];
    let mesh = mesh(&vertices, vec![[0, 1, 2], [3, 4, 5]]);
    let raw = raw(&mesh);
    let view = norm(&raw);
    assert_eq!(view.groups().len(), 2);
    assert_eq!(view.features().len(), 4);
    assert_eq!(view.coordinates().len(), 2);
    assert!(view.coordinates().iter().all(|g| g.distinct_features == 2));
    assert_eq!(view.summary().coincident_feature_positions, 2);
}

#[test]
fn one_ulp_and_sub_vendor_bin_separations_are_never_welded() {
    for shift in [f64::EPSILON, 1.0 / 4096.0] {
        let mesh = mesh(
            &[
                [0.0, 0.0, -1.0],
                [0.0, 0.0, 1.0],
                [0.0, 4.0, 1.0],
                [shift, 0.0, -1.0],
                [shift, 0.0, 1.0],
                [shift, 4.0, 1.0],
            ],
            vec![[0, 1, 2], [3, 4, 5]],
        );
        let raw = raw(&mesh);
        let view = norm(&raw);
        assert_eq!(view.groups().len(), 2);
        assert_eq!(view.features().len(), 4);
        assert_eq!(view.coordinates().len(), 4);
        assert_eq!(view.summary().coincident_feature_positions, 0);
    }
}

#[test]
fn crossing_duplicates_retain_multiplicity_and_reversed_local_provenance() {
    let mesh = mesh(
        &[[0.0, 0.0, -1.0], [4.0, 0.0, 1.0], [0.0, 4.0, 1.0]],
        vec![[0, 1, 2], [2, 1, 0], [0, 1, 2]],
    );
    let raw = raw(&mesh);
    let view = norm(&raw);
    assert_eq!(view.groups().len(), 1);
    assert_eq!(view.groups()[0].occurrences.len(), 3);
    assert_eq!(view.groups()[0].source_faces, 3);
    assert_eq!(view.groups()[0].ownership, Ownership::Unresolved);
    assert!(view.groups()[0]
        .issues
        .contains(IncidenceIssue::MultipleContributions));
    assert_eq!(
        view.segments().iter().map(|s| s.event).collect::<Vec<_>>(),
        vec![0, 1, 2]
    );
}

#[test]
fn repeated_indices_and_zero_length_coplanar_edges_are_retained() {
    let mesh = mesh(
        &[[0.0, 0.0, 0.0], [4.0, 0.0, 0.0]],
        vec![[0, 0, 1], [0, 0, 0]],
    );
    let raw = raw(&mesh);
    let view = norm(&raw);
    assert_eq!(view.segments().len(), 6);
    assert_eq!(view.summary().coplanar_faces, 2);
    assert!(view
        .groups()
        .iter()
        .all(|g| g.ownership == Ownership::Unresolved));
    for group in view.groups().iter().filter(|g| g.source_faces == 1) {
        assert!(!group.issues.contains(IncidenceIssue::DuplicateFace));
    }
    assert!(view
        .groups()
        .iter()
        .any(|g| g.endpoints == [SourceFeature::Vertex(0); 2]
            && g.issues.contains(IncidenceIssue::Collapsed)));
    check_partition(&view);
}

#[test]
fn collapsed_crossing_keeps_two_source_edges_and_coincident_geometry() {
    let mesh = mesh(
        &[[0.0, 0.0, -1.0], [0.0, 0.0, 1.0], [0.0, 0.0, 3.0]],
        vec![[0, 1, 2]],
    );
    let raw = raw(&mesh);
    let view = norm(&raw);
    assert_eq!(view.features().len(), 2);
    assert_eq!(view.coordinates()[0].distinct_features, 2);
    assert!(view.groups()[0].issues.contains(IncidenceIssue::Collapsed));
    assert_eq!(view.groups()[0].ownership, Ownership::Unresolved);
}

#[test]
fn nonmanifold_vertex_neighborhood_retains_high_degree_identity_without_routing_a_contour() {
    let mesh = mesh(
        &[
            [0.0, 0.0, 0.0],
            [2.0, 0.0, -1.0],
            [2.0, 2.0, 1.0],
            [-2.0, 0.0, -1.0],
            [-2.0, -2.0, 1.0],
            [0.0, 3.0, -1.0],
            [2.0, 3.0, 1.0],
        ],
        vec![[0, 1, 2], [0, 3, 4], [0, 5, 6]],
    );
    let raw = raw(&mesh);
    let view = norm(&raw);
    let vertex = view
        .features()
        .iter()
        .find(|g| g.feature == SourceFeature::Vertex(0))
        .unwrap();
    assert_eq!(vertex.segment_ends, 3);
    assert_eq!(vertex.observations.len(), 3);
    assert_eq!(view.groups().len(), 3);
    check_partition(&view);
}

#[test]
fn singular_and_reflected_transforms_preserve_raw_identity_and_local_classification() {
    let mesh = square(vec![[0, 1, 2], [0, 2, 3]]);
    for scale in [
        Vector3::new(-2.0, 3.0, 1.0),
        Vector3::new(1.0, 1.0, 0.0),
        Vector3::new(0.0, 1.0, 1.0),
    ] {
        let transform = Affine3::from_scale(scale).unwrap();
        let raw = section_mesh_z(
            &mesh,
            0.0,
            SectionTransform::Affine(&transform),
            SectionLimits::default(),
        )
        .unwrap();
        let view = norm(&raw);
        assert_eq!(view.segments().len(), 6);
        if scale.x == 0.0 {
            assert_eq!(view.summary().internal_patch_edges, 0);
            assert!(view
                .groups()
                .iter()
                .all(|g| g.ownership == Ownership::Unresolved));
        } else {
            assert_eq!(view.summary().internal_patch_edges, 1);
        }
        check_partition(&view);
    }
}

#[test]
fn exact_coplanar_predicate_can_certify_subnormal_triangles() {
    let eta = f64::from_bits(1);
    let mesh = mesh(
        &[
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, eta, 0.0],
            [0.0, eta, 0.0],
        ],
        vec![[0, 1, 2], [0, 2, 3]],
    );
    let raw = raw(&mesh);
    let view = norm(&raw);
    assert_eq!(diagonal(&view).ownership, Ownership::CoplanarPatchInternal);
    assert_eq!(view.summary().unresolved_groups, 0);
}

#[test]
fn exact_ownership_can_certify_rank_when_the_raw_computed_cross_is_zero() {
    let step = f64::EPSILON;
    let mesh = mesh(
        &[
            [0.0, 0.0, 0.0],
            [1.0, 1.0 + step, 0.0],
            [1.0 - step, 1.0, 0.0],
            [1.0, 0.0, 0.0],
        ],
        vec![[0, 1, 2], [1, 0, 3]],
    );
    let raw = raw(&mesh);
    assert_eq!(
        raw.events()[0].intersection.context.area_observation,
        karasu_core::geometry::section::TriangleAreaObservation::ZeroComputedCross
    );
    let view = norm(&raw);
    let group = view
        .groups()
        .iter()
        .find(|g| g.endpoints == [SourceFeature::Vertex(0), SourceFeature::Vertex(1)])
        .unwrap();
    assert_eq!(group.ownership, Ownership::CoplanarPatchInternal);
}

#[test]
fn signed_zero_normalizes_without_changing_raw_payload_or_identity() {
    let mesh = square(vec![[0, 1, 2]]);
    let transform = ObjectTransform::new(
        Vector3::new(-0.0, -0.0, -0.0),
        Rotation3::identity(),
        Vector3::new(1.0, 1.0, 1.0),
        Point3::new(0.0, 0.0, 0.0),
    )
    .unwrap();
    let raw = section_mesh_z(
        &mesh,
        -0.0,
        SectionTransform::Object(&transform),
        SectionLimits::default(),
    )
    .unwrap();
    let view = norm(&raw);
    assert_eq!(view.plane_z().to_bits(), 0);
    for obs in view.observations() {
        for x in obs.position.coordinates() {
            if x == 0.0 {
                assert_eq!(x.to_bits(), 0);
            }
        }
    }
    assert!(std::ptr::eq(view.raw(), std::ptr::from_ref(&raw)));
}

#[test]
fn face_permutation_preserves_group_keys_and_ownership_with_provenance_remapping() {
    let a = square(vec![[0, 1, 2], [0, 2, 3]]);
    let b = square(vec![[0, 2, 3], [0, 1, 2]]);
    let ar = raw(&a);
    let br = raw(&b);
    let av = norm(&ar);
    let bv = norm(&br);
    let keys = |v: &NormalizedSection<'_>| {
        v.groups()
            .iter()
            .map(|g| {
                (
                    g.endpoints,
                    g.ownership,
                    g.occurrences.len(),
                    g.source_faces,
                    g.issues,
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(keys(&av), keys(&bv));
    for segment in av.segments() {
        let old_face = av.raw().events()[segment.event].source_face;
        let mapped_face = 1 - old_face;
        assert!(bv
            .segments()
            .iter()
            .any(
                |s| bv.raw().events()[s.event].source_face == mapped_face && s.kind == segment.kind
            ));
    }
}

#[test]
fn all_canonical_small_fixtures_preserve_partition_multiplicity_and_complete_source() {
    for (id, variant) in [
        ("FX-01", None),
        ("FX-02", None),
        ("FX-03", None),
        ("FX-04", None),
        ("FX-05", None),
        ("FX-06", None),
        ("FX-07", None),
        ("FX-08", None),
        ("FX-09", None),
        ("FX-10", Some("A")),
        ("FX-10", Some("B")),
    ] {
        let imported = fixture(id, variant);
        let before = fixture(id, variant);
        for height in [0.0, 10.0, 20.0] {
            let raw = section_mesh_z(
                &imported.mesh,
                height,
                SectionTransform::Identity,
                SectionLimits::default(),
            )
            .unwrap();
            let snapshot = raw.events().to_vec();
            let view = norm(&raw);
            check_partition(&view);
            let expected = raw
                .events()
                .iter()
                .map(|r| match r.intersection.event {
                    TrianglePlaneEvent::None | TrianglePlaneEvent::TangentPoint { .. } => 0,
                    TrianglePlaneEvent::Segment { .. }
                    | TrianglePlaneEvent::CoplanarEdge { .. } => 1,
                    TrianglePlaneEvent::CoplanarTriangle { .. } => 3,
                })
                .sum::<usize>();
            assert_eq!(view.segments().len(), expected);
            assert_eq!(raw.events(), snapshot);
            assert_eq!(view, norm(&raw));
        }
        assert_eq!(imported, before);
    }
}

#[test]
fn exact_limits_zero_limits_and_large_configured_budgets_have_typed_outcomes() {
    let mesh = square(vec![[0, 1, 2]]);
    let raw = raw(&mesh);
    let exact = NormalizationLimits {
        max_events: 1,
        max_incidences: 3,
        max_payload_bytes: usize::MAX,
    };
    assert!(normalize_section(&raw, exact).is_ok());
    for (limits, resource, limit) in [
        (
            NormalizationLimits {
                max_events: 0,
                ..exact
            },
            NormalizationResource::Events,
            0,
        ),
        (
            NormalizationLimits {
                max_incidences: 2,
                ..exact
            },
            NormalizationResource::Incidences,
            2,
        ),
        (
            NormalizationLimits {
                max_payload_bytes: 0,
                ..exact
            },
            NormalizationResource::PayloadBytes,
            0,
        ),
    ] {
        assert_eq!(
            normalize_section(&raw, limits),
            Err(NormalizationError::LimitExceeded { resource, limit })
        );
    }
    assert!(normalize_section(
        &raw,
        NormalizationLimits {
            max_events: usize::MAX,
            max_incidences: usize::MAX,
            max_payload_bytes: usize::MAX
        }
    )
    .is_ok());
}

#[test]
fn payload_budget_accepts_its_exact_minimum_and_rejects_one_byte_less() {
    let mesh = square(vec![[0, 1, 2]]);
    let raw = raw(&mesh);
    let mut low = 0;
    let mut high = 100_000;
    while low < high {
        let middle = low + (high - low) / 2;
        let limits = NormalizationLimits {
            max_payload_bytes: middle,
            ..NormalizationLimits::default()
        };
        if normalize_section(&raw, limits).is_ok() {
            high = middle;
        } else {
            low = middle + 1;
        }
    }
    assert!(low > 0);
    assert!(normalize_section(
        &raw,
        NormalizationLimits {
            max_payload_bytes: low,
            ..NormalizationLimits::default()
        }
    )
    .is_ok());
    assert!(matches!(
        normalize_section(
            &raw,
            NormalizationLimits {
                max_payload_bytes: low - 1,
                ..NormalizationLimits::default()
            }
        ),
        Err(NormalizationError::LimitExceeded {
            resource: NormalizationResource::PayloadBytes,
            ..
        })
    ));
}

#[test]
fn pre_cancelled_and_final_cancellation_are_atomic_and_retryable() {
    let mesh = square(vec![[0, 1, 2], [0, 2, 3]]);
    let raw = raw(&mesh);
    let expected = norm(&raw);
    for phase in [
        Checkpoint::NormalizationStart,
        Checkpoint::NormalizationComplete,
    ] {
        let mut predicate = |p| p == phase;
        let error = normalize_section_with_cancellation(
            &raw,
            NormalizationLimits::default(),
            &mut Cancellation::new(&mut predicate),
        )
        .unwrap_err();
        assert_eq!(
            error,
            NormalizationError::Cancelled(Cancelled { checkpoint: phase })
        );
        assert!(error
            .source()
            .unwrap()
            .downcast_ref::<Cancelled>()
            .is_some());
        assert_eq!(expected, norm(&raw));
    }
    let mut predicate = |_| true;
    assert!(matches!(
        normalize_section_with_cancellation(
            &raw,
            NormalizationLimits {
                max_events: 0,
                ..NormalizationLimits::default()
            },
            &mut Cancellation::new(&mut predicate)
        ),
        Err(NormalizationError::Cancelled(_))
    ));
}

#[test]
fn all_sort_and_allocation_boundaries_allow_cancellation_without_partial_success() {
    let mesh = square(vec![[0, 1, 2], [0, 2, 3]]);
    let raw = raw(&mesh);
    for phase in [
        Checkpoint::NormalizationStorage,
        Checkpoint::NormalizationSorting,
    ] {
        for wanted in 1..=if phase == Checkpoint::NormalizationStorage {
            16
        } else {
            6
        } {
            let mut polls = 0;
            let mut predicate = |p| {
                if p == phase {
                    polls += 1;
                }
                p == phase && polls == wanted
            };
            assert_eq!(
                normalize_section_with_cancellation(
                    &raw,
                    NormalizationLimits::default(),
                    &mut Cancellation::new(&mut predicate)
                ),
                Err(NormalizationError::Cancelled(Cancelled {
                    checkpoint: phase
                }))
            );
            assert_eq!(polls, wanted);
        }
    }
}

#[test]
fn event_and_high_incidence_group_loops_have_deterministic_intermediate_cancellation() {
    let mesh = mesh(
        &[[0.0, 0.0, -1.0], [4.0, 0.0, 1.0], [0.0, 4.0, 1.0]],
        vec![[0, 1, 2]; 1024],
    );
    let raw = raw(&mesh);
    for phase in [
        Checkpoint::NormalizationEvents,
        Checkpoint::NormalizationGroups,
    ] {
        for wanted in [3, 7] {
            let mut seen = 0;
            let mut predicate = |p| {
                if p == phase {
                    seen += 1;
                }
                p == phase && seen == wanted
            };
            assert_eq!(
                normalize_section_with_cancellation(
                    &raw,
                    NormalizationLimits::default(),
                    &mut Cancellation::new(&mut predicate)
                ),
                Err(NormalizationError::Cancelled(Cancelled {
                    checkpoint: phase
                }))
            );
            assert_eq!(seen, wanted);
        }
    }
    let mut trace = Vec::new();
    let mut predicate = |p| {
        trace.push(p);
        false
    };
    let first = normalize_section_with_cancellation(
        &raw,
        NormalizationLimits::default(),
        &mut Cancellation::new(&mut predicate),
    )
    .unwrap();
    let mut again = Vec::new();
    let mut predicate = |p| {
        again.push(p);
        false
    };
    let second = normalize_section_with_cancellation(
        &raw,
        NormalizationLimits::default(),
        &mut Cancellation::new(&mut predicate),
    )
    .unwrap();
    assert_eq!(first, second);
    assert_eq!(trace, again);
}

#[test]
fn never_cancelled_wrapper_is_equal_and_errors_before_a_later_poll_are_not_replaced() {
    let mesh = square(vec![[0, 1, 2]]);
    let raw = raw(&mesh);
    let mut predicate = |_| false;
    assert_eq!(
        norm(&raw),
        normalize_section_with_cancellation(
            &raw,
            NormalizationLimits::default(),
            &mut Cancellation::new(&mut predicate)
        )
        .unwrap()
    );
    let mut trace = Vec::new();
    let mut predicate = |p| {
        trace.push(p);
        p == Checkpoint::NormalizationComplete
    };
    assert!(matches!(
        normalize_section_with_cancellation(
            &raw,
            NormalizationLimits {
                max_events: 0,
                ..NormalizationLimits::default()
            },
            &mut Cancellation::new(&mut predicate)
        ),
        Err(NormalizationError::LimitExceeded { .. })
    ));
    assert_eq!(trace, vec![Checkpoint::NormalizationStart]);
}

#[test]
fn repeated_execution_preserves_coordinate_bits_and_raw_local_origins() {
    let imported = fixture("FX-05", None);
    let raw = section_mesh_z(
        &imported.mesh,
        17.0,
        SectionTransform::Identity,
        SectionLimits::default(),
    )
    .unwrap();
    let expected = norm(&raw);
    for _ in 0..20 {
        let actual = norm(&raw);
        assert_eq!(actual, expected);
        for (a, b) in actual.observations().iter().zip(expected.observations()) {
            assert_eq!(
                a.position.coordinates().map(f64::to_bits),
                b.position.coordinates().map(f64::to_bits)
            );
            assert_eq!(a.origin, b.origin);
            assert!(matches!(a.origin, EndpointOrigin::Edge(_)));
        }
    }
}

#[test]
fn on_plane_crossing_requires_coherent_distinct_faces_and_preserves_both_supports() {
    for faces in [
        vec![[0, 1, 2], [0, 1, 3]],
        vec![[0, 1, 2], [0, 1, 2]],
        vec![[0, 1, 2], [1, 0, 3], [0, 1, 4]],
    ] {
        let source = mesh(
            &[
                [0., 0., 0.],
                [4., 0., 0.],
                [0., 4., 1.],
                [0., -4., -1.],
                [0., 8., 2.],
            ],
            faces,
        );
        let section = raw(&source);
        let normalized = norm(&section);
        assert_eq!(normalized.groups()[0].ownership, Ownership::Unresolved);
        assert_eq!(normalized.segments().len(), source.triangles().len());
        assert_eq!(
            normalized.groups()[0].occurrences.len(),
            source.triangles().len()
        );
    }
}
