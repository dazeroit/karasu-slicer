//! Public mesh invariants, source-space bounds, and non-destructive diagnostics.

use karasu_core::geometry::{Point3, Vector3};
use karasu_core::mesh::{Degeneracy, FaceNormal, MeshDiagnostic, MeshError, TriangleMesh};

fn triangle() -> Vec<Point3> {
    vec![
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(2.0, 0.0, 0.0),
        Point3::new(0.0, 3.0, 0.0),
    ]
}

#[test]
fn empty_mesh_has_no_bounds_and_no_sentinel_values() {
    let mesh = TriangleMesh::try_new(vec![], vec![]).unwrap();
    assert_eq!(mesh.source_bounds(), None);
    assert_eq!(mesh.statistics().vertex_count, 0);
    assert_eq!(mesh.statistics().triangle_count, 0);
    assert_eq!(mesh.diagnostics().unwrap(), []);
    assert_eq!(mesh.triangle_normal(0), None);
}

#[test]
fn bounds_preserve_negative_and_zero_source_coordinates() {
    let vertices = vec![Point3::new(-9.0, -4.0, 0.0), Point3::new(5.0, 8.0, 20.0)];
    let mesh = TriangleMesh::try_new(vertices.clone(), vec![]).unwrap();
    let bounds = mesh.source_bounds().unwrap();
    assert_eq!(mesh.vertices(), vertices);
    assert_eq!(bounds.min(), Point3::new(-9.0, -4.0, 0.0));
    assert_eq!(bounds.max(), Point3::new(5.0, 8.0, 20.0));
    assert_eq!(bounds.dimensions(), Vector3::new(14.0, 12.0, 20.0));
    assert_eq!(bounds.center(), Point3::new(-2.0, 2.0, 10.0));
    assert_eq!(mesh.statistics().source_bounds, Some(bounds));
}

#[test]
fn point_bounds_have_zero_extents() {
    let mesh = TriangleMesh::try_new(vec![Point3::new(-3.0, 1.0, 7.0)], vec![]).unwrap();
    let bounds = mesh.source_bounds().unwrap();
    assert_eq!(bounds.dimensions(), Vector3::new(0.0, 0.0, 0.0));
    assert_eq!(bounds.center(), Point3::new(-3.0, 1.0, 7.0));
}

#[test]
fn center_avoids_overflow_for_large_finite_endpoints() {
    let mesh = TriangleMesh::try_new(
        vec![
            Point3::new(1.0e308, 0.0, 0.0),
            Point3::new(1.2e308, 0.0, 0.0),
        ],
        vec![],
    )
    .unwrap();
    let bounds = mesh.source_bounds().unwrap();
    assert!(bounds.center().x.is_finite());
    assert!(bounds.dimensions().x.is_finite());
    assert!(bounds.center().x >= bounds.min().x && bounds.center().x <= bounds.max().x);
}

#[test]
fn unrepresentable_extent_is_a_structured_range_error() {
    assert_eq!(
        TriangleMesh::try_new(
            vec![
                Point3::new(-f64::MAX, 0.0, 0.0),
                Point3::new(f64::MAX, 0.0, 0.0)
            ],
            vec![]
        ),
        Err(MeshError::UnrepresentableBounds)
    );
}

#[test]
fn manual_construction_rejects_nonfinite_values_on_every_axis() {
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for axis in 0..3 {
            let mut coordinates = [0.0; 3];
            coordinates[axis] = value;
            assert_eq!(
                TriangleMesh::try_new(
                    vec![Point3::new(coordinates[0], coordinates[1], coordinates[2])],
                    vec![]
                ),
                Err(MeshError::NonFiniteVertex { vertex: 0, axis })
            );
        }
    }
}

#[test]
fn invalid_indices_have_triangle_and_corner_context() {
    for corner in 0..3 {
        let mut indices = [0, 1, 2];
        indices[corner] = u32::MAX;
        assert_eq!(
            TriangleMesh::try_new(triangle(), vec![[0, 1, 2], indices]),
            Err(MeshError::InvalidIndex {
                triangle: 1,
                corner,
                index: u32::MAX,
                vertex_count: 3
            })
        );
    }
    assert!(matches!(
        TriangleMesh::try_new(vec![], vec![[0, 0, 0]]),
        Err(MeshError::InvalidIndex { .. })
    ));
}

#[test]
fn normal_follows_preserved_winding() {
    let mesh = TriangleMesh::try_new(triangle(), vec![[0, 1, 2], [2, 1, 0]]).unwrap();
    assert_eq!(mesh.triangles(), [[0, 1, 2], [2, 1, 0]]);
    assert_eq!(
        mesh.triangle_normal(0),
        Some(FaceNormal::Unit(Vector3::new(0.0, 0.0, 1.0)))
    );
    assert_eq!(
        mesh.triangle_normal(1),
        Some(FaceNormal::Unit(Vector3::new(0.0, 0.0, -1.0)))
    );
    assert_eq!(mesh.triangle_normal(2), None);
}

#[test]
fn repeated_and_collinear_faces_are_retained_and_classified_separately() {
    let mesh = TriangleMesh::try_new(
        vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(2.0, 0.0, 0.0),
        ],
        vec![[0, 0, 1], [0, 1, 2]],
    )
    .unwrap();
    assert_eq!(
        mesh.diagnostics().unwrap(),
        vec![
            MeshDiagnostic::DegenerateTriangle {
                triangle: 0,
                kind: Degeneracy::RepeatedVertex
            },
            MeshDiagnostic::DegenerateTriangle {
                triangle: 1,
                kind: Degeneracy::ZeroComputedArea
            }
        ]
    );
    assert_eq!(mesh.statistics().triangle_count, 2);
}

#[test]
fn duplicate_geometry_detection_ignores_winding_and_distinct_manual_indices() {
    let mut vertices = triangle();
    vertices.extend(triangle());
    let mesh = TriangleMesh::try_new(vertices, vec![[0, 1, 2], [2, 0, 1], [5, 4, 3]]).unwrap();
    let expected = vec![
        MeshDiagnostic::DuplicateTriangle {
            triangle: 1,
            first: 0,
        },
        MeshDiagnostic::DuplicateTriangle {
            triangle: 2,
            first: 0,
        },
    ];
    assert_eq!(mesh.diagnostics().unwrap(), expected);
    assert_eq!(mesh.diagnostics().unwrap(), expected);
    assert_eq!(mesh.statistics().vertex_count, 6);
}

#[test]
fn signed_zero_is_the_only_coordinate_normalization() {
    let mesh = TriangleMesh::try_new(
        vec![Point3::new(-0.0, 0.0, -0.0), Point3::new(0.0, 0.0, 0.0)],
        vec![[0, 1, 1]],
    )
    .unwrap();
    assert_eq!(
        mesh.vertices()[0].coordinates().map(f64::to_bits),
        [0, 0, 0]
    );
    assert_eq!(mesh.statistics().vertex_count, 2); // Manual construction does not weld vertices.
    assert_eq!(
        mesh.triangle_normal(0),
        Some(FaceNormal::Undefined(Degeneracy::RepeatedVertex))
    );
}

#[test]
fn scaled_normal_calculation_handles_small_and_large_triangles() {
    for scale in [1e-200, 1e200] {
        let mesh = TriangleMesh::try_new(
            vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(scale, 0.0, 0.0),
                Point3::new(0.0, scale, 0.0),
            ],
            vec![[0, 1, 2]],
        )
        .unwrap();
        assert_eq!(
            mesh.triangle_normal(0),
            Some(FaceNormal::Unit(Vector3::new(0.0, 0.0, 1.0)))
        );
        assert_eq!(mesh.diagnostics().unwrap(), []);
    }
}

#[test]
fn thin_triangle_is_not_welded_or_rejected_by_a_global_epsilon() {
    let mesh = TriangleMesh::try_new(
        vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(1.0, 1e-100, 0.0),
        ],
        vec![[0, 1, 2]],
    )
    .unwrap();
    assert_eq!(mesh.diagnostics().unwrap(), []);
    assert_eq!(mesh.statistics().vertex_count, 3);
}
