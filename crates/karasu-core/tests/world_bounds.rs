//! Tight vertex bounds and source preservation, with independently computed fixture expectations.

use karasu_core::geometry::{Affine3, ObjectTransform, Point3, Rotation3, TransformError, Vector3};
use karasu_core::mesh::stl::{import_stl_path, ImportedStl, StlOptions, StlUnitPolicy};
use karasu_core::mesh::{Aabb, TransformedBoundsError, TriangleMesh};
use std::{error::Error, f64::consts::FRAC_PI_2, path::PathBuf};

const ZERO: Vector3 = Vector3::new(0.0, 0.0, 0.0);
const ONE: Vector3 = Vector3::new(1.0, 1.0, 1.0);
const ORIGIN: Point3 = Point3::new(0.0, 0.0, 0.0);
const MANIFEST: &str = include_str!("../../../fixtures/reference/manifest.json");

fn fixture(id: &str) -> ImportedStl {
    // Extract only the versioned manifest's filename field, never guess a fixture filename.
    let entry = MANIFEST
        .split("\"fixture_id\":")
        .skip(1)
        .find(|entry| entry.trim_start().starts_with(&format!("\"{id}\"")))
        .unwrap();
    let filename = entry
        .split_once("\"filename\":")
        .unwrap()
        .1
        .trim_start()
        .strip_prefix('"')
        .unwrap()
        .split('"')
        .next()
        .unwrap();
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/reference")
        .join(filename);
    import_stl_path(path, StlOptions::new(StlUnitPolicy::AssumeMillimeters)).unwrap()
}

fn object(
    translation: Vector3,
    rotation: Rotation3,
    scale: Vector3,
    pivot: Point3,
) -> ObjectTransform {
    ObjectTransform::new(translation, rotation, scale, pivot).unwrap()
}

fn close(actual: [f64; 3], expected: [f64; 3]) {
    // Fixtures/offsets here have magnitude at most 100 mm; allow 256 binary64 epsilons at
    // that scale for trig/quaternion/independent-scalar evaluation. Not an STL/UI tolerance.
    let tolerance = 256.0 * f64::EPSILON * 100.0;
    for (actual, expected) in actual.into_iter().zip(expected) {
        assert!(
            (actual - expected).abs() <= tolerance,
            "{actual} versus {expected}"
        );
    }
}

fn assert_bounds(bounds: Aabb, min: [f64; 3], max: [f64; 3]) {
    close(bounds.min().coordinates(), min);
    close(bounds.max().coordinates(), max);
}

// Independent scalar Rx -> Ry -> Rz evaluation: no production rotation/matrix/bounds helper
// is used to construct the oracle. Source vertices retain actual STL f32 quantization.
fn independent_point(
    point: Point3,
    angles: Vector3,
    scale: Vector3,
    pivot: Point3,
    translation: Vector3,
) -> [f64; 3] {
    let local_x = (point.x - pivot.x) * scale.x;
    let local_y = (point.y - pivot.y) * scale.y;
    let local_z = (point.z - pivot.z) * scale.z;
    let after_x_y = local_y * angles.x.cos() - local_z * angles.x.sin();
    let after_x_z = local_y * angles.x.sin() + local_z * angles.x.cos();
    let after_y_x = local_x * angles.y.cos() + after_x_z * angles.y.sin();
    let final_height = -local_x * angles.y.sin() + after_x_z * angles.y.cos();
    [
        after_y_x * angles.z.cos() - after_x_y * angles.z.sin() + pivot.x + translation.x,
        after_y_x * angles.z.sin() + after_x_y * angles.z.cos() + pivot.y + translation.y,
        final_height + pivot.z + translation.z,
    ]
}

fn independent_bounds(
    vertices: &[Point3],
    angles: Vector3,
    scale: Vector3,
    pivot: Point3,
    translation: Vector3,
) -> ([f64; 3], [f64; 3]) {
    let mut min = [f64::INFINITY; 3];
    let mut max = [f64::NEG_INFINITY; 3];
    for point in vertices {
        let point = independent_point(*point, angles, scale, pivot, translation);
        for axis in 0..3 {
            min[axis] = min[axis].min(point[axis]);
            max[axis] = max[axis].max(point[axis]);
        }
    }
    (min, max)
}

#[test]
fn identity_bounds_equal_source_bounds_in_both_paths() {
    for id in ["FX-01", "FX-05", "FX-08"] {
        let imported = fixture(id);
        assert_eq!(
            imported
                .mesh
                .world_bounds(&ObjectTransform::identity())
                .unwrap(),
            imported.mesh.source_bounds()
        );
        assert_eq!(
            imported
                .mesh
                .transformed_bounds(&Affine3::identity())
                .unwrap(),
            imported.mesh.source_bounds()
        );
    }
}

#[test]
fn empty_mesh_has_no_world_bounds_even_for_extreme_valid_object_parameters() {
    let mesh = TriangleMesh::try_new(vec![], vec![]).unwrap();
    let transform = object(
        ZERO,
        Rotation3::identity(),
        Vector3::new(f64::MAX, 1.0, 1.0),
        Point3::new(f64::MAX, 0.0, 0.0),
    );
    assert_eq!(mesh.world_bounds(&transform).unwrap(), None);
    assert_eq!(mesh.transformed_bounds(&Affine3::identity()).unwrap(), None);
}

#[test]
fn cube_translation_preserves_size_and_explicit_five_mm_lift() {
    let imported = fixture("FX-01");
    let transform = object(
        Vector3::new(15.0, 10.0, 5.0),
        Rotation3::identity(),
        ONE,
        ORIGIN,
    );
    let bounds = imported.mesh.world_bounds(&transform).unwrap().unwrap();
    assert_eq!(bounds.min(), Point3::new(5.0, 0.0, 5.0));
    assert_eq!(bounds.max(), Point3::new(25.0, 20.0, 25.0));
    assert_eq!(bounds.dimensions(), Vector3::new(20.0, 20.0, 20.0));
}

#[test]
fn cube_uniform_and_independent_scaling_match_analytic_extents() {
    let imported = fixture("FX-01");
    for (scale, expected_min, expected_max) in [
        (
            Vector3::new(1.25, 1.25, 1.25),
            [-12.5, -12.5, 0.0],
            [12.5, 12.5, 25.0],
        ),
        (
            Vector3::new(1.25, 0.8, 1.1),
            [-12.5, -8.0, 0.0],
            [12.5, 8.0, 22.0],
        ),
    ] {
        assert_bounds(
            imported
                .mesh
                .world_bounds(&object(ZERO, Rotation3::identity(), scale, ORIGIN))
                .unwrap()
                .unwrap(),
            expected_min,
            expected_max,
        );
    }
}

#[test]
fn cube_quarter_turn_plus_nonuniform_scale_has_analytic_bounds() {
    let imported = fixture("FX-01");
    let transform = object(
        Vector3::new(15.0, 10.0, 5.0),
        Rotation3::around_z_radians(FRAC_PI_2).unwrap(),
        Vector3::new(2.0, 3.0, 4.0),
        ORIGIN,
    );
    assert_bounds(
        imported.mesh.world_bounds(&transform).unwrap().unwrap(),
        [-15.0, -10.0, 5.0],
        [45.0, 30.0, 85.0],
    );
}

#[test]
fn pivoted_scale_may_put_vertices_below_plate_without_automatic_adjustment() {
    let imported = fixture("FX-01");
    let transform = object(
        ZERO,
        Rotation3::identity(),
        Vector3::new(2.0, 2.0, 2.0),
        Point3::new(0.0, 0.0, 10.0),
    );
    let bounds = imported.mesh.world_bounds(&transform).unwrap().unwrap();
    assert_eq!(bounds.min(), Point3::new(-20.0, -20.0, -10.0));
    assert_eq!(bounds.max(), Point3::new(20.0, 20.0, 30.0));
}

#[test]
fn fx05_z_thirty_degree_vertex_bounds_match_independent_math_and_secondary_gui_precision() {
    let imported = fixture("FX-05");
    let angles = Vector3::new(0.0, 0.0, 30.0_f64.to_radians());
    let transform = object(
        ZERO,
        Rotation3::around_z_radians(angles.z).unwrap(),
        ONE,
        ORIGIN,
    );
    let bounds = imported.mesh.world_bounds(&transform).unwrap().unwrap();
    let (min, max) = independent_bounds(imported.mesh.vertices(), angles, ONE, ORIGIN, ZERO);
    assert_bounds(bounds, min, max);
    close(
        bounds.dimensions().components(),
        [
            28.857_155_951_470_816,
            27.863_533_428_766_498,
            35.429_767_608_642_58,
        ],
    );
    for (extent, observed) in bounds
        .dimensions()
        .components()
        .into_iter()
        .zip([28.86, 27.86, 35.43])
    {
        assert!((extent - observed).abs() <= 0.005); // Secondary CHITUBOX display consistency only.
    }
    assert!((bounds.dimensions().x - 18.37).abs() > 10.0); // Lychee's unchanged field is not a world-bound oracle.
}

#[test]
fn fx05_combined_reference_matches_explicit_zero_pivot_assumptions() {
    let imported = fixture("FX-05");
    let angles = Vector3::new(0.0, 0.0, 30.0_f64.to_radians());
    let translation = Vector3::new(15.0, 10.0, 5.0);
    let scale = Vector3::new(1.25, 1.25, 1.25);
    let transform = object(
        translation,
        Rotation3::around_z_radians(angles.z).unwrap(),
        scale,
        ORIGIN,
    );
    let bounds = imported.mesh.world_bounds(&transform).unwrap().unwrap();
    let (min, max) =
        independent_bounds(imported.mesh.vertices(), angles, scale, ORIGIN, translation);
    assert_bounds(bounds, min, max);
    close(
        bounds.dimensions().components(),
        [
            36.071_444_939_338_52,
            34.829_416_785_958_124,
            44.287_209_510_803_22,
        ],
    );
    assert_bounds(
        bounds,
        [-3.035_722_469_669_26, -7.414_708_392_979_062, 5.0],
        [
            33.035_722_469_669_26,
            27.414_708_392_979_062,
            49.287_209_510_803_22,
        ],
    );
}

#[test]
fn fx05_mixed_axes_nonuniform_scale_and_nonorigin_pivot_match_independent_scalar_oracle() {
    let imported = fixture("FX-05");
    let angles = Vector3::new(0.31, -0.47, 0.83);
    let scale = Vector3::new(1.25, 0.8, 1.1);
    let pivot = Point3::new(-3.0, 5.0, 7.0);
    let translation = Vector3::new(15.0, -10.0, 5.0);
    let transform = object(
        translation,
        Rotation3::from_euler_xyz_radians(angles).unwrap(),
        scale,
        pivot,
    );
    let (min, max) =
        independent_bounds(imported.mesh.vertices(), angles, scale, pivot, translation);
    assert_bounds(
        imported.mesh.world_bounds(&transform).unwrap().unwrap(),
        min,
        max,
    );
    assert_bounds(
        imported
            .mesh
            .transformed_bounds(&transform.to_affine().unwrap())
            .unwrap()
            .unwrap(),
        min,
        max,
    );
}

#[test]
fn fx05_rotated_source_aabb_corners_are_not_tight_mesh_bounds() {
    let imported = fixture("FX-05");
    let source = imported.mesh.source_bounds().unwrap();
    let rotation =
        Affine3::from_rotation(Rotation3::around_z_radians(30.0_f64.to_radians()).unwrap());
    let mut corners = Vec::new();
    for x in [source.min().x, source.max().x] {
        for y in [source.min().y, source.max().y] {
            for z in [source.min().z, source.max().z] {
                corners.push(Point3::new(x, y, z));
            }
        }
    }
    let enclosing_mesh = TriangleMesh::try_new(corners, vec![]).unwrap();
    let enclosure = enclosing_mesh
        .transformed_bounds(&rotation)
        .unwrap()
        .unwrap();
    let tight = imported
        .mesh
        .transformed_bounds(&rotation)
        .unwrap()
        .unwrap();
    close(
        enclosure.dimensions().components(),
        [
            31.780_873_450_250_112,
            36.673_286_697_723_63,
            35.429_767_608_642_58,
        ],
    );
    assert!(enclosure.dimensions().x > tight.dimensions().x + 2.0);
    assert!(enclosure.dimensions().y > tight.dimensions().y + 8.0);
}

#[test]
fn original_vertices_indices_bounds_winding_metadata_and_diagnostics_are_unchanged() {
    for id in ["FX-01", "FX-05", "FX-08"] {
        let imported = fixture(id);
        let vertices = imported.mesh.vertices().to_vec();
        let indices = imported.mesh.triangles().to_vec();
        let statistics = imported.mesh.statistics();
        let metadata = format!("{:?}", imported.metadata);
        let diagnostics = imported.diagnostics.clone();
        let normals: Vec<_> = (0..statistics.triangle_count)
            .map(|i| imported.mesh.triangle_normal(i))
            .collect();
        let transform = object(
            Vector3::new(15.0, 10.0, 5.0),
            Rotation3::around_z_radians(0.7).unwrap(),
            Vector3::new(-2.0, 0.0, 1.25),
            Point3::new(-3.0, 5.0, 7.0),
        );
        let expected = imported.mesh.world_bounds(&transform).unwrap();
        for _ in 0..10 {
            assert_eq!(imported.mesh.world_bounds(&transform).unwrap(), expected);
        }
        let _ = imported
            .mesh
            .transformed_bounds(&transform.to_affine().unwrap())
            .unwrap();
        assert_eq!(imported.mesh.vertices(), vertices);
        assert_eq!(imported.mesh.triangles(), indices);
        assert_eq!(imported.mesh.statistics(), statistics);
        assert_eq!(format!("{:?}", imported.metadata), metadata);
        assert_eq!(imported.diagnostics, diagnostics);
        assert_eq!(
            (0..statistics.triangle_count)
                .map(|i| imported.mesh.triangle_normal(i))
                .collect::<Vec<_>>(),
            normals
        );
    }
}

#[test]
fn zero_and_negative_scales_produce_finite_flat_or_reflected_world_bounds() {
    let imported = fixture("FX-01");
    let flat = object(
        Vector3::new(0.0, 0.0, 5.0),
        Rotation3::identity(),
        ZERO,
        ORIGIN,
    );
    let bounds = imported.mesh.world_bounds(&flat).unwrap().unwrap();
    assert_eq!(bounds.min(), Point3::new(0.0, 0.0, 5.0));
    assert_eq!(bounds.dimensions(), ZERO);
    let reflected = object(
        ZERO,
        Rotation3::identity(),
        Vector3::new(-2.0, 3.0, -1.0),
        ORIGIN,
    );
    assert_bounds(
        imported.mesh.world_bounds(&reflected).unwrap().unwrap(),
        [-20.0, -30.0, -20.0],
        [20.0, 30.0, 0.0],
    );
}

#[test]
fn unused_manual_vertices_are_included_and_large_finite_translation_works() {
    let mesh =
        TriangleMesh::try_new(vec![ORIGIN, Point3::new(1e300, 0.0, 0.0)], vec![[0, 0, 0]]).unwrap();
    let shift = Affine3::from_translation(Vector3::new(1e300, -1e300, 5.0)).unwrap();
    let bounds = mesh.transformed_bounds(&shift).unwrap().unwrap();
    assert_eq!(bounds.min(), Point3::new(1e300, -1e300, 5.0));
    assert_eq!(bounds.max(), Point3::new(2e300, -1e300, 5.0));
    assert!(bounds.dimensions().x.is_finite());
}

#[test]
fn point_overflow_reports_original_vertex_and_preserves_error_cause() {
    let mesh =
        TriangleMesh::try_new(vec![ORIGIN, Point3::new(f64::MAX / 2.0, 0.0, 0.0)], vec![]).unwrap();
    let transform = object(
        ZERO,
        Rotation3::identity(),
        Vector3::new(4.0, 1.0, 1.0),
        ORIGIN,
    );
    let error = mesh.world_bounds(&transform).unwrap_err();
    assert!(matches!(
        error,
        TransformedBoundsError::Vertex {
            vertex: 1,
            source: TransformError::ArithmeticOverflow { .. }
        }
    ));
    assert!(error.source().is_some());
    assert!(mesh
        .transformed_bounds(&transform.to_affine().unwrap())
        .is_err());
}

#[test]
fn finite_transformed_vertices_with_overflowing_extent_are_rejected() {
    let half_max = f64::MAX / 2.0;
    let mesh = TriangleMesh::try_new(
        vec![
            Point3::new(-half_max, 0.0, 0.0),
            Point3::new(half_max, 0.0, 0.0),
        ],
        vec![],
    )
    .unwrap();
    let transform = object(
        ZERO,
        Rotation3::identity(),
        Vector3::new(1.5, 1.0, 1.0),
        ORIGIN,
    );
    assert_eq!(
        mesh.world_bounds(&transform),
        Err(TransformedBoundsError::UnrepresentableExtent)
    );
    assert_eq!(
        mesh.transformed_bounds(&transform.to_affine().unwrap()),
        Err(TransformedBoundsError::UnrepresentableExtent)
    );
}
