//! Independent analytic tests for right-handed, column-vector transform conventions.

use karasu_core::geometry::{Affine3, ObjectTransform, Point3, Rotation3, TransformError, Vector3};
use std::f64::consts::{FRAC_PI_2, PI};

const ZERO: Vector3 = Vector3::new(0.0, 0.0, 0.0);
const ONE: Vector3 = Vector3::new(1.0, 1.0, 1.0);
const ORIGIN: Point3 = Point3::new(0.0, 0.0, 0.0);

fn close(actual: [f64; 3], expected: [f64; 3]) {
    for (actual, expected) in actual.into_iter().zip(expected) {
        // Short small-angle chains use fewer than 64 rounding-sensitive operations per result.
        // A scale-aware 64-epsilon comparison is a test budget, never an engine tolerance.
        let tolerance = 64.0 * f64::EPSILON * expected.abs().max(1.0);
        assert!(
            (actual - expected).abs() <= tolerance,
            "{actual} versus {expected}; tolerance {tolerance}"
        );
    }
}

fn object(
    translation: Vector3,
    rotation: Rotation3,
    scale: Vector3,
    pivot: Point3,
) -> ObjectTransform {
    ObjectTransform::new(translation, rotation, scale, pivot).unwrap()
}

#[test]
fn identities_preserve_points_directions_and_public_parameters() {
    let position = Point3::new(-7.0, 2.0, 0.0);
    let direction = Vector3::new(9.0, -3.0, 2.0);
    let scene = ObjectTransform::identity();
    assert_eq!(scene.transform_point(position).unwrap(), position);
    assert_eq!(scene.transform_vector(direction).unwrap(), direction);
    assert_eq!(scene.translation(), ZERO);
    assert_eq!(scene.rotation(), Rotation3::identity());
    assert_eq!(scene.scale(), ONE);
    assert_eq!(scene.pivot(), ORIGIN);
    assert_eq!(scene.to_affine().unwrap(), Affine3::identity());
    assert_eq!(
        Affine3::identity().transform_point(position).unwrap(),
        position
    );
    assert_eq!(
        Affine3::identity().transform_vector(direction).unwrap(),
        direction
    );
}

#[test]
fn translation_changes_each_point_axis_but_never_a_direction() {
    for translation in [
        Vector3::new(15.0, 0.0, 0.0),
        Vector3::new(0.0, -10.0, 0.0),
        Vector3::new(0.0, 0.0, 5.0),
    ] {
        let transform = Affine3::from_translation(translation).unwrap();
        let position = transform
            .transform_point(Point3::new(-2.0, 3.0, -4.0))
            .unwrap();
        assert_eq!(
            position,
            Point3::new(
                -2.0 + translation.x,
                3.0 + translation.y,
                -4.0 + translation.z
            )
        );
        assert_eq!(transform.translation(), translation);
        assert_eq!(transform.transform_vector(ONE).unwrap(), ONE);
    }
}

#[test]
fn uniform_and_independent_scales_have_analytic_results() {
    let position = Point3::new(-2.0, 3.0, 4.0);
    assert_eq!(
        Affine3::from_uniform_scale(1.25)
            .unwrap()
            .transform_point(position)
            .unwrap(),
        Point3::new(-2.5, 3.75, 5.0)
    );
    let scale = Vector3::new(1.25, 0.8, 1.1);
    let transform = object(ZERO, Rotation3::identity(), scale, ORIGIN);
    close(
        transform.transform_point(position).unwrap().coordinates(),
        [-2.5, 2.4, 4.4],
    );
    close(
        transform
            .transform_vector(Vector3::new(-2.0, 3.0, 4.0))
            .unwrap()
            .components(),
        [-2.5, 2.4, 4.4],
    );
}

#[test]
fn positive_ninety_degree_axis_rotations_follow_right_hand_rule() {
    for (rotation, expected) in [
        (
            Rotation3::around_x_radians(FRAC_PI_2).unwrap(),
            [1.0, -3.0, 2.0],
        ),
        (
            Rotation3::around_y_radians(FRAC_PI_2).unwrap(),
            [3.0, 2.0, -1.0],
        ),
        (
            Rotation3::around_z_radians(FRAC_PI_2).unwrap(),
            [-2.0, 1.0, 3.0],
        ),
    ] {
        let affine = Affine3::from_rotation(rotation);
        close(
            affine
                .transform_point(Point3::new(1.0, 2.0, 3.0))
                .unwrap()
                .coordinates(),
            expected,
        );
        close(
            affine
                .transform_vector(Vector3::new(1.0, 2.0, 3.0))
                .unwrap()
                .components(),
            expected,
        );
    }
}

#[test]
fn negative_angles_rotate_in_the_opposite_direction() {
    let transform = Affine3::from_rotation(Rotation3::around_z_radians(-FRAC_PI_2).unwrap());
    close(
        transform
            .transform_point(Point3::new(2.0, 0.0, -1.0))
            .unwrap()
            .coordinates(),
        [0.0, -2.0, -1.0],
    );
}

#[test]
fn arbitrary_angle_matches_scalar_trigonometry() {
    let angle = 0.731_f64;
    let transform = Affine3::from_rotation(Rotation3::around_z_radians(angle).unwrap());
    let expected = [
        -3.0 * angle.cos() - 5.0 * angle.sin(),
        -3.0 * angle.sin() + 5.0 * angle.cos(),
        -9.0,
    ];
    close(
        transform
            .transform_point(Point3::new(-3.0, 5.0, -9.0))
            .unwrap()
            .coordinates(),
        expected,
    );
}

#[test]
fn arbitrary_axis_rotation_normalizes_axis_and_has_analytic_cyclic_result() {
    for length in [1.0, 1e200, 1e-200] {
        let rotation = Rotation3::from_axis_angle_radians(
            Vector3::new(length, length, length),
            2.0 * PI / 3.0,
        )
        .unwrap();
        close(
            Affine3::from_rotation(rotation)
                .transform_point(Point3::new(1.0, 2.0, 3.0))
                .unwrap()
                .coordinates(),
            [3.0, 1.0, 2.0],
        );
    }
}

#[test]
fn euler_xyz_is_extrinsic_x_then_y_then_z_not_the_reverse() {
    let rotation =
        Rotation3::from_euler_xyz_radians(Vector3::new(FRAC_PI_2, FRAC_PI_2, FRAC_PI_2)).unwrap();
    close(
        Affine3::from_rotation(rotation)
            .transform_point(Point3::new(1.0, 2.0, 3.0))
            .unwrap()
            .coordinates(),
        [3.0, 2.0, -1.0],
    );
}

#[test]
fn euler_arbitrary_angles_match_independent_sequential_scalar_rotations() {
    let angles = Vector3::new(0.31, -0.47, 0.83);
    let source = Point3::new(-3.0, 7.0, 11.0);
    let after_x = Point3::new(
        source.x,
        source.y * angles.x.cos() - source.z * angles.x.sin(),
        source.y * angles.x.sin() + source.z * angles.x.cos(),
    );
    let after_y = Point3::new(
        after_x.x * angles.y.cos() + after_x.z * angles.y.sin(),
        after_x.y,
        -after_x.x * angles.y.sin() + after_x.z * angles.y.cos(),
    );
    let expected = [
        after_y.x * angles.z.cos() - after_y.y * angles.z.sin(),
        after_y.x * angles.z.sin() + after_y.y * angles.z.cos(),
        after_y.z,
    ];
    let rotation = Rotation3::from_euler_xyz_radians(angles).unwrap();
    close(
        Affine3::from_rotation(rotation)
            .transform_point(source)
            .unwrap()
            .coordinates(),
        expected,
    );
}

#[test]
fn quaternion_normalization_handles_extreme_nonzero_magnitudes() {
    for magnitude in [f64::MAX, 1.0, f64::from_bits(1)] {
        let rotation = Rotation3::from_quaternion_xyzw([0.0, 0.0, magnitude, magnitude]).unwrap();
        let norm: f64 = rotation.quaternion_xyzw().iter().map(|v| v * v).sum();
        assert!((norm - 1.0).abs() < 8.0 * f64::EPSILON);
        close(
            Affine3::from_rotation(rotation)
                .transform_point(Point3::new(2.0, 0.0, 0.0))
                .unwrap()
                .coordinates(),
            [0.0, 2.0, 0.0],
        );
    }
}

#[test]
fn quaternion_sign_equivalence_is_not_assumed_by_representation_equality() {
    let positive = Rotation3::from_quaternion_xyzw([1.0, 2.0, 3.0, 4.0]).unwrap();
    let negative = Rotation3::from_quaternion_xyzw([-1.0, -2.0, -3.0, -4.0]).unwrap();
    assert_ne!(positive, negative);
    assert_eq!(positive.matrix(), negative.matrix());
}

#[test]
fn rotation_composition_matches_sequential_application_and_keeps_unit_norm() {
    let first = Rotation3::around_x_radians(0.3).unwrap();
    let next = Rotation3::around_z_radians(-0.7).unwrap();
    let position = Point3::new(-2.0, 4.0, 1.0);
    let expected = Affine3::from_rotation(next)
        .transform_point(
            Affine3::from_rotation(first)
                .transform_point(position)
                .unwrap(),
        )
        .unwrap();
    close(
        Affine3::from_rotation(first.then(next).unwrap())
            .transform_point(position)
            .unwrap()
            .coordinates(),
        expected.coordinates(),
    );
    let mut accumulated = Rotation3::identity();
    for _ in 0..1000 {
        accumulated = accumulated.then(first).unwrap();
    }
    let norm: f64 = accumulated.quaternion_xyzw().iter().map(|v| v * v).sum();
    assert!((norm - 1.0).abs() < 8.0 * f64::EPSILON);
}

#[test]
fn local_pivot_stays_fixed_under_rotation_and_scale() {
    let pivot = Point3::new(-3.0, 5.0, 7.0);
    let rotation = Rotation3::from_euler_xyz_radians(Vector3::new(0.3, -0.7, 1.2)).unwrap();
    for scale in [
        Vector3::new(2.0, 3.0, 4.0),
        ZERO,
        Vector3::new(-2.0, 3.0, 0.0),
    ] {
        assert_eq!(
            object(ZERO, rotation, scale, pivot)
                .transform_point(pivot)
                .unwrap(),
            pivot
        );
    }
}

#[test]
fn nonorigin_pivot_has_different_rotation_result_from_origin() {
    let rotation = Rotation3::around_z_radians(FRAC_PI_2).unwrap();
    let source = Point3::new(3.0, 1.0, 2.0);
    let pivoted = object(ZERO, rotation, ONE, Point3::new(2.0, 1.0, 0.0));
    close(
        pivoted.transform_point(source).unwrap().coordinates(),
        [2.0, 2.0, 2.0],
    );
    close(
        object(ZERO, rotation, ONE, ORIGIN)
            .transform_point(source)
            .unwrap()
            .coordinates(),
        [-1.0, 3.0, 2.0],
    );
}

#[test]
fn local_scale_precedes_rotation_and_world_translation_is_last() {
    let transform = object(
        Vector3::new(15.0, 10.0, 5.0),
        Rotation3::around_z_radians(FRAC_PI_2).unwrap(),
        Vector3::new(2.0, 3.0, 4.0),
        Point3::new(1.0, 2.0, 3.0),
    );
    close(
        transform
            .transform_point(Point3::new(2.0, 3.0, 4.0))
            .unwrap()
            .coordinates(),
        [13.0, 14.0, 12.0],
    );
    close(
        transform.transform_vector(ONE).unwrap().components(),
        [-3.0, 2.0, 4.0],
    );
}

#[test]
fn translation_after_pivoted_rotation_moves_pivot_by_requested_world_vector() {
    let pivot = Point3::new(4.0, -5.0, 6.0);
    let transform = object(
        Vector3::new(15.0, 10.0, 5.0),
        Rotation3::around_y_radians(0.7).unwrap(),
        Vector3::new(2.0, 3.0, 4.0),
        pivot,
    );
    assert_eq!(
        transform.transform_point(pivot).unwrap(),
        Point3::new(19.0, 5.0, 11.0)
    );
}

#[test]
fn affine_flattening_matches_direct_object_evaluation_at_ordinary_magnitudes() {
    let transform = object(
        Vector3::new(15.0, -10.0, 5.0),
        Rotation3::from_euler_xyz_radians(Vector3::new(0.1, -0.3, 0.7)).unwrap(),
        Vector3::new(1.25, 0.8, -1.1),
        Point3::new(-2.0, 3.0, 4.0),
    );
    for position in [ORIGIN, Point3::new(-7.0, 9.0, 3.0), transform.pivot()] {
        close(
            transform
                .to_affine()
                .unwrap()
                .transform_point(position)
                .unwrap()
                .coordinates(),
            transform.transform_point(position).unwrap().coordinates(),
        );
    }
}

#[test]
fn rotation_then_translation_differs_from_translation_then_rotation() {
    let rotation = Affine3::from_rotation(Rotation3::around_z_radians(FRAC_PI_2).unwrap());
    let translation = Affine3::from_translation(Vector3::new(5.0, 0.0, 0.0)).unwrap();
    let position = Point3::new(1.0, 0.0, 0.0);
    close(
        rotation
            .then(translation)
            .unwrap()
            .transform_point(position)
            .unwrap()
            .coordinates(),
        [5.0, 1.0, 0.0],
    );
    close(
        translation
            .then(rotation)
            .unwrap()
            .transform_point(position)
            .unwrap()
            .coordinates(),
        [0.0, 6.0, 0.0],
    );
}

#[test]
fn nonuniform_scale_and_rotation_do_not_commute() {
    let scale = Affine3::from_scale(Vector3::new(2.0, 3.0, 1.0)).unwrap();
    let rotation = Affine3::from_rotation(Rotation3::around_z_radians(FRAC_PI_2).unwrap());
    let position = Point3::new(1.0, 2.0, 3.0);
    close(
        scale
            .then(rotation)
            .unwrap()
            .transform_point(position)
            .unwrap()
            .coordinates(),
        [-6.0, 2.0, 3.0],
    );
    close(
        rotation
            .then(scale)
            .unwrap()
            .transform_point(position)
            .unwrap()
            .coordinates(),
        [-4.0, 3.0, 3.0],
    );
}

#[test]
fn affine_composition_matches_sequential_points_and_directions_and_identity() {
    let first = Affine3::from_parts(
        [[2.0, 1.0, 0.0], [0.0, 3.0, 1.0], [0.0, 0.0, -1.0]],
        Vector3::new(2.0, -5.0, 1.0),
    )
    .unwrap();
    let next = Affine3::from_parts(
        [[1.0, 0.0, 2.0], [1.0, 1.0, 0.0], [0.0, 2.0, 1.0]],
        Vector3::new(-7.0, 4.0, 3.0),
    )
    .unwrap();
    let position = Point3::new(3.0, -2.0, 5.0);
    let direction = Vector3::new(3.0, -2.0, 5.0);
    let combined = first.then(next).unwrap();
    assert_eq!(
        combined.transform_point(position).unwrap(),
        next.transform_point(first.transform_point(position).unwrap())
            .unwrap()
    );
    assert_eq!(
        combined.transform_vector(direction).unwrap(),
        next.transform_vector(first.transform_vector(direction).unwrap())
            .unwrap()
    );
    assert_eq!(Affine3::identity().then(first).unwrap(), first);
    assert_eq!(first.then(Affine3::identity()).unwrap(), first);
    assert_eq!(
        combined.linear(),
        [[2.0, 1.0, -2.0], [2.0, 4.0, 1.0], [0.0, 6.0, 1.0]]
    );
}

#[test]
fn explicit_affine_scale_rotation_translation_chain_matches_object_order() {
    let rotation = Rotation3::around_z_radians(0.4).unwrap();
    let scale = Vector3::new(1.25, 0.8, 1.1);
    let translation = Vector3::new(15.0, 10.0, 5.0);
    let affine = Affine3::from_scale(scale)
        .unwrap()
        .then(Affine3::from_rotation(rotation))
        .unwrap()
        .then(Affine3::from_translation(translation).unwrap())
        .unwrap();
    let expected = object(translation, rotation, scale, ORIGIN);
    close(
        affine
            .transform_point(Point3::new(-3.0, 7.0, 9.0))
            .unwrap()
            .coordinates(),
        expected
            .transform_point(Point3::new(-3.0, 7.0, 9.0))
            .unwrap()
            .coordinates(),
    );
}

#[test]
fn zero_scales_are_valid_singular_maps_without_implicit_repair() {
    let transform = object(
        Vector3::new(0.0, 0.0, 5.0),
        Rotation3::around_z_radians(0.7).unwrap(),
        ZERO,
        Point3::new(2.0, 3.0, 4.0),
    );
    assert_eq!(
        transform
            .transform_point(Point3::new(9.0, -7.0, 2.0))
            .unwrap(),
        Point3::new(2.0, 3.0, 9.0)
    );
    assert_eq!(transform.transform_vector(ONE).unwrap(), ZERO);
    assert_eq!(
        Affine3::from_uniform_scale(0.0)
            .unwrap()
            .transform_point(Point3::new(9.0, -7.0, 2.0))
            .unwrap(),
        ORIGIN
    );
}

#[test]
fn negative_scales_reflect_coordinates_and_triangle_orientation() {
    let transform = Affine3::from_scale(Vector3::new(-1.0, 1.0, 1.0)).unwrap();
    let first_edge = transform
        .transform_vector(Vector3::new(1.0, 0.0, 0.0))
        .unwrap();
    let second_edge = transform
        .transform_vector(Vector3::new(0.0, 1.0, 0.0))
        .unwrap();
    let cross_z = first_edge.x * second_edge.y - first_edge.y * second_edge.x;
    assert!(cross_z < 0.0); // Reflection reverses orientation; no index/winding adjustment occurs.
    assert_eq!(
        transform
            .transform_point(Point3::new(2.0, -3.0, 4.0))
            .unwrap(),
        Point3::new(-2.0, -3.0, 4.0)
    );
}

#[test]
fn nonfinite_translation_pivot_and_scale_are_rejected_per_component() {
    for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for component in 0..3 {
            let mut values = [1.0; 3];
            values[component] = invalid;
            let vector = Vector3::new(values[0], values[1], values[2]);
            let pivot = Point3::new(values[0], values[1], values[2]);
            for (parameter, result) in [
                (
                    "translation",
                    ObjectTransform::new(vector, Rotation3::identity(), ONE, ORIGIN),
                ),
                (
                    "scale",
                    ObjectTransform::new(ZERO, Rotation3::identity(), vector, ORIGIN),
                ),
                (
                    "pivot",
                    ObjectTransform::new(ZERO, Rotation3::identity(), ONE, pivot),
                ),
            ] {
                assert!(
                    matches!(result, Err(TransformError::NonFinite { parameter: found, component: found_component }) if found==parameter && found_component==component)
                );
            }
            assert!(Affine3::from_translation(vector).is_err());
            assert!(Affine3::from_scale(vector).is_err());
        }
    }
}

#[test]
fn nonfinite_rotation_inputs_and_exact_zero_rotation_representations_are_rejected() {
    assert_eq!(
        Rotation3::from_quaternion_xyzw([0.0; 4]),
        Err(TransformError::ZeroQuaternion)
    );
    assert_eq!(
        Rotation3::from_axis_angle_radians(ZERO, 0.0),
        Err(TransformError::ZeroRotationAxis)
    );
    for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for component in 0..4 {
            let mut quaternion = [1.0; 4];
            quaternion[component] = invalid;
            assert_eq!(
                Rotation3::from_quaternion_xyzw(quaternion),
                Err(TransformError::NonFinite {
                    parameter: "quaternion",
                    component
                })
            );
        }
        for component in 0..3 {
            let mut components = [0.1; 3];
            components[component] = invalid;
            let values = Vector3::new(components[0], components[1], components[2]);
            assert!(Rotation3::from_euler_xyz_radians(values).is_err());
            assert!(Rotation3::from_axis_angle_radians(values, 0.3).is_err());
        }
        assert!(Rotation3::around_x_radians(invalid).is_err());
        assert!(Rotation3::around_y_radians(invalid).is_err());
        assert!(Rotation3::around_z_radians(invalid).is_err());
    }
}

#[test]
fn nonfinite_affine_coefficients_points_and_directions_are_rejected() {
    for component in 0..9 {
        let mut linear = Affine3::identity().linear();
        linear[component / 3][component % 3] = f64::INFINITY;
        assert_eq!(
            Affine3::from_parts(linear, ZERO),
            Err(TransformError::NonFinite {
                parameter: "linear matrix",
                component
            })
        );
    }
    for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for component in 0..3 {
            let mut values = [0.0; 3];
            values[component] = invalid;
            let position = Point3::new(values[0], values[1], values[2]);
            let direction = Vector3::new(values[0], values[1], values[2]);
            assert!(Affine3::identity().transform_point(position).is_err());
            assert!(Affine3::identity().transform_vector(direction).is_err());
            assert!(ObjectTransform::identity()
                .transform_point(position)
                .is_err());
            assert!(ObjectTransform::identity()
                .transform_vector(direction)
                .is_err());
        }
    }
}

#[test]
fn point_direction_and_composition_overflow_are_controlled_errors() {
    let scale = Affine3::from_uniform_scale(2.0).unwrap();
    assert!(matches!(
        scale.transform_point(Point3::new(f64::MAX, 0.0, 0.0)),
        Err(TransformError::ArithmeticOverflow { .. })
    ));
    assert!(matches!(
        scale.transform_vector(Vector3::new(f64::MAX, 0.0, 0.0)),
        Err(TransformError::ArithmeticOverflow { .. })
    ));
    let huge_scale = Affine3::from_uniform_scale(f64::MAX).unwrap();
    assert!(matches!(
        huge_scale.then(scale),
        Err(TransformError::ArithmeticOverflow { .. })
    ));
    let shift = Affine3::from_translation(Vector3::new(f64::MAX, 0.0, 0.0)).unwrap();
    assert!(shift
        .transform_point(Point3::new(f64::MAX, 0.0, 0.0))
        .is_err());
    assert_eq!(
        shift
            .transform_vector(Vector3::new(f64::MAX, 0.0, 0.0))
            .unwrap(),
        Vector3::new(f64::MAX, 0.0, 0.0)
    );
    assert!(shift.then(shift).is_err());
    assert!(object(
        ZERO,
        Rotation3::identity(),
        Vector3::new(2.0, 1.0, 1.0),
        ORIGIN
    )
    .transform_point(Point3::new(f64::MAX, 0.0, 0.0))
    .is_err());
}

#[test]
fn large_finite_pivot_can_be_evaluated_even_when_affine_flattening_overflows() {
    let pivot = Point3::new(f64::MAX, 0.0, 0.0);
    let transform = object(
        ZERO,
        Rotation3::identity(),
        Vector3::new(2.0, 1.0, 1.0),
        pivot,
    );
    assert_eq!(transform.transform_point(pivot).unwrap(), pivot);
    assert!(matches!(
        transform.to_affine(),
        Err(TransformError::ArithmeticOverflow { .. })
    ));
    assert!(transform
        .transform_point(Point3::new(-f64::MAX, 0.0, 0.0))
        .is_err());
}

#[test]
fn identity_linear_mapping_avoids_spurious_large_pivot_cancellation() {
    for rotation in [
        Rotation3::identity(),
        Rotation3::from_quaternion_xyzw([0.0, 0.0, 0.0, -1.0]).unwrap(),
    ] {
        let transform = object(
            Vector3::new(2.0, 0.0, 0.0),
            rotation,
            ONE,
            Point3::new(f64::MAX, 0.0, 0.0),
        );
        assert_eq!(
            transform
                .transform_point(Point3::new(1.0, 0.0, 0.0))
                .unwrap(),
            Point3::new(3.0, 0.0, 0.0)
        );
        assert_eq!(
            transform.to_affine().unwrap().translation(),
            Vector3::new(2.0, 0.0, 0.0)
        );
    }
}

#[test]
fn very_large_finite_angles_remain_finite_without_claiming_phase_accuracy() {
    let rotation = Rotation3::around_z_radians(f64::MAX).unwrap();
    assert!(rotation
        .quaternion_xyzw()
        .iter()
        .all(|value| value.is_finite()));
    assert!(Affine3::from_rotation(rotation)
        .transform_point(Point3::new(1.0, 1.0, 1.0))
        .unwrap()
        .coordinates()
        .iter()
        .all(|value| value.is_finite()));
}

#[test]
fn repeated_composition_and_application_are_deterministic() {
    let transform = object(
        Vector3::new(15.0, 10.0, 5.0),
        Rotation3::around_z_radians(0.3).unwrap(),
        Vector3::new(1.25, -0.8, 0.0),
        Point3::new(-3.0, 5.0, 7.0),
    );
    let expected = transform
        .to_affine()
        .unwrap()
        .then(Affine3::from_uniform_scale(2.0).unwrap())
        .unwrap();
    for _ in 0..20 {
        let repeated = transform
            .to_affine()
            .unwrap()
            .then(Affine3::from_uniform_scale(2.0).unwrap())
            .unwrap();
        assert_eq!(repeated, expected);
        assert_eq!(
            repeated
                .transform_point(Point3::new(-7.0, 9.0, 11.0))
                .unwrap(),
            expected
                .transform_point(Point3::new(-7.0, 9.0, 11.0))
                .unwrap()
        );
    }
}

#[test]
fn identity_rotation_composition_preserves_orientation_within_rounding_budget() {
    let rotation = Rotation3::from_euler_xyz_radians(Vector3::new(0.31, -0.47, 0.83)).unwrap();
    let source = Point3::new(-3.0, 7.0, 11.0);
    let expected = Affine3::from_rotation(rotation)
        .transform_point(source)
        .unwrap();
    for result in [
        rotation.then(Rotation3::identity()).unwrap(),
        Rotation3::identity().then(rotation).unwrap(),
    ] {
        close(
            Affine3::from_rotation(result)
                .transform_point(source)
                .unwrap()
                .coordinates(),
            expected.coordinates(),
        );
    }
}

#[test]
fn object_directions_match_affine_and_detect_overflow_independently_of_translation() {
    let transform = object(
        Vector3::new(15.0, 10.0, 5.0),
        Rotation3::from_euler_xyz_radians(Vector3::new(0.31, -0.47, 0.83)).unwrap(),
        Vector3::new(1.25, -0.8, 1.1),
        Point3::new(-3.0, 5.0, 7.0),
    );
    let direction = Vector3::new(-3.0, 7.0, 11.0);
    close(
        transform.transform_vector(direction).unwrap().components(),
        transform
            .to_affine()
            .unwrap()
            .transform_vector(direction)
            .unwrap()
            .components(),
    );
    assert!(object(
        ZERO,
        Rotation3::identity(),
        Vector3::new(2.0, 1.0, 1.0),
        ORIGIN
    )
    .transform_vector(Vector3::new(f64::MAX, 0.0, 0.0))
    .is_err());
    let cancellation = Affine3::from_parts([[1.0, 1.0, -1.0], [0.0; 3], [0.0; 3]], ZERO).unwrap();
    // The exact sum is finite, but the documented fixed-order first partial sum overflows.
    assert!(matches!(
        cancellation.transform_point(Point3::new(f64::MAX, f64::MAX, f64::MAX)),
        Err(TransformError::ArithmeticOverflow { .. })
    ));
}

mod round_trips {
    use super::{
        object, Affine3, ObjectTransform, Point3, Rotation3, Vector3, FRAC_PI_2, ONE, ORIGIN, ZERO,
    };
    use karasu_core::mesh::stl::{import_stl_path, StlOptions, StlUnitPolicy};
    use std::{fs, path::PathBuf};

    fn magnitude(values: [f64; 3]) -> f64 {
        values
            .into_iter()
            .fold(0.0, |largest, value| largest.max(value.abs()))
    }

    fn near(actual: [f64; 3], expected: [f64; 3], working_magnitude: f64) {
        // Absolute + relative binary64 budget for these short forward/reverse chains only.
        // Include independently calculated working magnitude to cover cancellation near zero.
        // Coordinates/offsets stay below 100 mm, |scale| is 0.5..=4 (condition ratio <= 8),
        // and reciprocals are <= 2. The 128-epsilon allowance covers both evaluation paths;
        // it is not a universal inverse guarantee, STL quantization allowance, or engine epsilon.
        assert!(working_magnitude.is_finite() && working_magnitude <= 100.0);
        for (actual, expected) in actual.into_iter().zip(expected) {
            let tolerance = 128.0 * f64::EPSILON * (1.0 + working_magnitude.max(expected.abs()));
            assert!(
                (actual - expected).abs() <= tolerance,
                "{actual} versus {expected}; tolerance {tolerance}"
            );
        }
    }

    // Independent scalar trig, not production quaternion/matrix/composition code.
    fn rotate_xyz([x, y, z]: [f64; 3], angles: Vector3) -> [f64; 3] {
        let after_x_y = y * angles.x.cos() - z * angles.x.sin();
        let after_x_z = y * angles.x.sin() + z * angles.x.cos();
        let after_y_x = x * angles.y.cos() + after_x_z * angles.y.sin();
        let final_height = -x * angles.y.sin() + after_x_z * angles.y.cos();
        [
            after_y_x * angles.z.cos() - after_x_y * angles.z.sin(),
            after_y_x * angles.z.sin() + after_x_y * angles.z.cos(),
            final_height,
        ]
    }

    fn unrotate_xyz([x, y, z]: [f64; 3], angles: Vector3) -> [f64; 3] {
        // R^-1 = Rx(-x) * Ry(-y) * Rz(-z): undo Z, then Y, then X.
        let after_z_x = x * angles.z.cos() + y * angles.z.sin();
        let after_z_y = -x * angles.z.sin() + y * angles.z.cos();
        let recovered_x = after_z_x * angles.y.cos() - z * angles.y.sin();
        let after_y_z = after_z_x * angles.y.sin() + z * angles.y.cos();
        [
            recovered_x,
            after_z_y * angles.x.cos() + after_y_z * angles.x.sin(),
            -after_z_y * angles.x.sin() + after_y_z * angles.x.cos(),
        ]
    }

    fn reverse_rotation(angles: Vector3) -> Rotation3 {
        Rotation3::around_z_radians(-angles.z)
            .unwrap()
            .then(Rotation3::around_y_radians(-angles.y).unwrap())
            .unwrap()
            .then(Rotation3::around_x_radians(-angles.x).unwrap())
            .unwrap()
    }

    fn explicit_forward(transform: ObjectTransform) -> Affine3 {
        let pivot = transform.pivot();
        Affine3::from_translation(Vector3::new(-pivot.x, -pivot.y, -pivot.z))
            .unwrap()
            .then(Affine3::from_scale(transform.scale()).unwrap())
            .unwrap()
            .then(Affine3::from_rotation(transform.rotation()))
            .unwrap()
            .then(Affine3::from_translation(Vector3::new(pivot.x, pivot.y, pivot.z)).unwrap())
            .unwrap()
            .then(Affine3::from_translation(transform.translation()).unwrap())
            .unwrap()
    }

    fn explicit_reverse(transform: ObjectTransform, rotation_reverse: Rotation3) -> Affine3 {
        let t = transform.translation();
        let c = transform.pivot();
        let s = transform.scale();
        // Test-local construction for well-conditioned, nonzero scales only; no inverse API.
        assert!(s
            .components()
            .into_iter()
            .all(|v| (0.5..=4.0).contains(&v.abs())));
        Affine3::from_translation(Vector3::new(-t.x, -t.y, -t.z))
            .unwrap()
            .then(Affine3::from_translation(Vector3::new(-c.x, -c.y, -c.z)).unwrap())
            .unwrap()
            .then(Affine3::from_rotation(rotation_reverse))
            .unwrap()
            .then(Affine3::from_scale(Vector3::new(1.0 / s.x, 1.0 / s.y, 1.0 / s.z)).unwrap())
            .unwrap()
            .then(Affine3::from_translation(Vector3::new(c.x, c.y, c.z)).unwrap())
            .unwrap()
    }

    fn check_object_round_trip(transform: ObjectTransform, angles: Vector3, source: Point3) {
        let c = transform.pivot();
        let s = transform.scale();
        let t = transform.translation();
        let rotated = rotate_xyz(
            [
                (source.x - c.x) * s.x,
                (source.y - c.y) * s.y,
                (source.z - c.z) * s.z,
            ],
            angles,
        );
        let expected = [
            rotated[0] + c.x + t.x,
            rotated[1] + c.y + t.y,
            rotated[2] + c.z + t.z,
        ];
        let working = magnitude(expected)
            .max(magnitude(source.coordinates()))
            .max(magnitude(c.coordinates()))
            .max(magnitude(t.components()))
            .max(magnitude(rotated));
        let reverse = explicit_reverse(transform, reverse_rotation(angles));
        for forward in [transform.to_affine().unwrap(), explicit_forward(transform)] {
            let world = forward.transform_point(source).unwrap();
            near(world.coordinates(), expected, working);
            near(
                reverse.transform_point(world).unwrap().coordinates(),
                source.coordinates(),
                working,
            );
            let unrotated = unrotate_xyz(
                [
                    world.x - t.x - c.x,
                    world.y - t.y - c.y,
                    world.z - t.z - c.z,
                ],
                angles,
            );
            near(
                [
                    c.x + unrotated[0] / s.x,
                    c.y + unrotated[1] / s.y,
                    c.z + unrotated[2] / s.z,
                ],
                source.coordinates(),
                working,
            );
        }
        let world = transform.transform_point(source).unwrap();
        near(world.coordinates(), expected, working);
        near(
            reverse.transform_point(world).unwrap().coordinates(),
            source.coordinates(),
            working,
        );
    }

    #[test]
    fn translations_reverse_exactly_for_dyadic_coordinates_and_offsets() {
        for t in [Vector3::new(7.0, 5.0, 3.0), Vector3::new(-7.0, -5.0, -3.0)] {
            let forward = Affine3::from_translation(t).unwrap();
            let reverse = Affine3::from_translation(Vector3::new(-t.x, -t.y, -t.z)).unwrap();
            for source in [Point3::new(3.5, -2.25, 5.125), Point3::new(-8.0, 4.0, -1.0)] {
                let world = forward.transform_point(source).unwrap();
                assert_eq!(
                    world,
                    Point3::new(source.x + t.x, source.y + t.y, source.z + t.z)
                );
                // Small dyadic additions/subtractions have no rounding loss in these cases.
                assert_eq!(reverse.transform_point(world).unwrap(), source);
                assert_eq!(
                    forward
                        .then(reverse)
                        .unwrap()
                        .transform_point(source)
                        .unwrap(),
                    source
                );
                assert_eq!(
                    object(t, Rotation3::identity(), ONE, ORIGIN)
                        .transform_point(source)
                        .unwrap(),
                    world
                );
            }
        }
    }

    #[test]
    fn axis_rotations_reverse_off_axis_points_with_scalar_angle_oracles() {
        let source = Point3::new(3.0, -2.0, 5.0);
        for angle in [FRAC_PI_2, -0.731] {
            for angles in [
                Vector3::new(angle, 0.0, 0.0),
                Vector3::new(0.0, angle, 0.0),
                Vector3::new(0.0, 0.0, angle),
            ] {
                check_object_round_trip(
                    object(
                        ZERO,
                        Rotation3::from_euler_xyz_radians(angles).unwrap(),
                        ONE,
                        ORIGIN,
                    ),
                    angles,
                    source,
                );
            }
        }
    }

    #[test]
    fn combined_euler_round_trip_requires_reversing_axis_order() {
        let angles = Vector3::new(0.31, -0.47, 0.83);
        let source = Point3::new(-3.0, 7.0, 11.0);
        let transform = object(
            ZERO,
            Rotation3::from_euler_xyz_radians(angles).unwrap(),
            ONE,
            ORIGIN,
        );
        check_object_round_trip(transform, angles, source);
        let wrong = Affine3::from_rotation(
            Rotation3::from_euler_xyz_radians(Vector3::new(-angles.x, -angles.y, -angles.z))
                .unwrap(),
        );
        let recovered = wrong
            .transform_point(transform.transform_point(source).unwrap())
            .unwrap();
        assert!((recovered.x - source.x).abs() > 0.1);
    }

    #[test]
    fn normalized_xyzw_quaternion_and_conjugate_match_rational_oracles() {
        let rotation = Rotation3::from_quaternion_xyzw([1.0, 2.0, 3.0, 4.0]).unwrap();
        let [x, y, z, w] = rotation.quaternion_xyzw();
        let root = 30.0_f64.sqrt();
        for (actual, raw) in [x, y, z, w].into_iter().zip([1.0, 2.0, 3.0, 4.0]) {
            assert!((actual - raw / root).abs() <= 8.0 * f64::EPSILON);
        }
        let reverse =
            Affine3::from_rotation(Rotation3::from_quaternion_xyzw([-x, -y, -z, w]).unwrap());
        let source = Point3::new(3.0, -2.0, 5.0);
        // The XYZW quaternion /sqrt(30) gives R =
        // [[2,-10,11], [14,5,2], [-5,10,10]] / 15, independently derived.
        let expected = Point3::new(27.0 / 5.0, 14.0 / 5.0, 1.0);
        let world = Affine3::from_rotation(rotation)
            .transform_point(source)
            .unwrap();
        near(world.coordinates(), expected.coordinates(), 6.0);
        near(
            reverse.transform_point(expected).unwrap().coordinates(),
            source.coordinates(),
            6.0,
        );
        near(
            reverse.transform_point(world).unwrap().coordinates(),
            source.coordinates(),
            6.0,
        );
    }

    #[test]
    fn nonzero_uniform_nonuniform_and_reflected_scales_reverse() {
        let source = Point3::new(3.5, -2.25, 5.125);
        for s in [
            Vector3::new(2.0, 2.0, 2.0),
            Vector3::new(2.0, 0.5, 4.0),
            Vector3::new(-2.0, 0.5, 4.0),
            Vector3::new(-2.0, 0.5, -4.0),
        ] {
            let forward = Affine3::from_scale(s).unwrap();
            let reverse =
                Affine3::from_scale(Vector3::new(1.0 / s.x, 1.0 / s.y, 1.0 / s.z)).unwrap();
            let world = forward.transform_point(source).unwrap();
            assert_eq!(
                world,
                Point3::new(source.x * s.x, source.y * s.y, source.z * s.z)
            );
            // Powers of two and bounded dyadic inputs permit exact equality here.
            assert_eq!(reverse.transform_point(world).unwrap(), source);
            check_object_round_trip(object(ZERO, Rotation3::identity(), s, ORIGIN), ZERO, source);
        }
        let uniform = Affine3::from_uniform_scale(1.25).unwrap();
        let reverse = Affine3::from_uniform_scale(1.0 / 1.25).unwrap();
        near(
            reverse
                .transform_point(uniform.transform_point(source).unwrap())
                .unwrap()
                .coordinates(),
            source.coordinates(),
            7.0,
        );
    }

    #[test]
    fn pivoted_rotation_and_nonuniform_scale_reverse_at_and_away_from_pivot() {
        let angles = Vector3::new(0.31, -0.47, 0.83);
        for pivot in [ORIGIN, Point3::new(2.0, -3.0, 4.0)] {
            let transform = object(
                ZERO,
                Rotation3::from_euler_xyz_radians(angles).unwrap(),
                Vector3::new(2.0, 0.5, -1.5),
                pivot,
            );
            assert_eq!(transform.transform_point(pivot).unwrap(), pivot);
            for source in [
                pivot,
                Point3::new(5.0, 1.0, -2.0),
                Point3::new(-7.0, 4.0, 3.0),
            ] {
                check_object_round_trip(transform, angles, source);
            }
        }
    }

    #[test]
    fn noncommuting_affine_round_trip_has_analytic_result_and_wrong_order_counterexample() {
        let angles = Vector3::new(0.0, 0.0, FRAC_PI_2);
        let transform = object(
            Vector3::new(7.0, -5.0, 3.0),
            Rotation3::around_z_radians(FRAC_PI_2).unwrap(),
            Vector3::new(2.0, 0.5, -1.0),
            Point3::new(2.0, -3.0, 4.0),
        );
        let source = Point3::new(5.0, 1.0, -2.0);
        check_object_round_trip(transform, angles, source);
        // p-c=(3,4,-6), S*(p-c)=(6,2,6), Rz90=(-2,6,6), +c+t=(7,-2,13).
        let world = Point3::new(7.0, -2.0, 13.0);
        near(
            explicit_forward(transform)
                .transform_point(source)
                .unwrap()
                .coordinates(),
            world.coordinates(),
            13.0,
        );
        near(
            explicit_reverse(transform, reverse_rotation(angles))
                .transform_point(world)
                .unwrap()
                .coordinates(),
            source.coordinates(),
            13.0,
        );
        // Deliberately undo scale before rotation: (world-t-c)=(-2,6,6),
        // S^-1=(-1,12,-6), Rz-90=(12,1,-6), +c=(14,-2,-2), not (5,1,-2).
        let wrong = Affine3::from_translation(Vector3::new(-9.0, 8.0, -7.0))
            .unwrap()
            .then(Affine3::from_scale(Vector3::new(0.5, 2.0, -1.0)).unwrap())
            .unwrap()
            .then(Affine3::from_rotation(reverse_rotation(angles)))
            .unwrap()
            .then(Affine3::from_translation(Vector3::new(2.0, -3.0, 4.0)).unwrap())
            .unwrap();
        let wrong_result = wrong.transform_point(world).unwrap();
        near(wrong_result.coordinates(), [14.0, -2.0, -2.0], 14.0);
        assert!((wrong_result.x - source.x).abs() > 8.0);
    }

    #[test]
    fn affine_direction_round_trips_ignore_translation_and_pivot_offsets() {
        let angles = Vector3::new(0.31, -0.47, 0.83);
        let rotation = Rotation3::from_euler_xyz_radians(angles).unwrap();
        let s = Vector3::new(-2.0, 0.5, 1.5);
        let source = Vector3::new(3.0, -2.0, 5.0);
        let expected = rotate_xyz([source.x * s.x, source.y * s.y, source.z * s.z], angles);
        let linear_only = object(ZERO, rotation, s, ORIGIN);
        let offset = object(
            Vector3::new(7.0, -5.0, 3.0),
            rotation,
            s,
            Point3::new(2.0, -3.0, 4.0),
        );
        for transform in [linear_only, offset] {
            for forward in [explicit_forward(transform), transform.to_affine().unwrap()] {
                let world = forward.transform_vector(source).unwrap();
                near(world.components(), expected, 12.0);
                assert_eq!(
                    world,
                    linear_only
                        .to_affine()
                        .unwrap()
                        .transform_vector(source)
                        .unwrap()
                );
                near(
                    explicit_reverse(transform, reverse_rotation(angles))
                        .transform_vector(world)
                        .unwrap()
                        .components(),
                    source.components(),
                    12.0,
                );
            }
            assert_eq!(
                transform.transform_vector(source).unwrap(),
                linear_only.transform_vector(source).unwrap()
            );
        }
    }

    fn fixture_path(id: &str) -> PathBuf {
        // These IDs each have one entry; the authoritative manifest supplies the filename.
        let entry = include_str!("../../../fixtures/reference/manifest.json")
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
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/reference")
            .join(filename)
    }

    #[test]
    fn canonical_cube_and_inclined_prism_round_trips_preserve_all_source_data() {
        let angles = Vector3::new(0.31, -0.47, 0.83);
        let transform = object(
            Vector3::new(7.0, -5.0, 3.0),
            Rotation3::from_euler_xyz_radians(angles).unwrap(),
            Vector3::new(-2.0, 0.5, 1.5),
            Point3::new(2.0, -3.0, 4.0),
        );
        for id in ["FX-01", "FX-05"] {
            let path = fixture_path(id);
            let bytes = fs::read(&path).unwrap();
            let options = StlOptions::new(StlUnitPolicy::AssumeMillimeters);
            let original = import_stl_path(&path, options).unwrap();
            let imported = import_stl_path(&path, options).unwrap();
            assert_eq!(imported.mesh.vertices().len(), 8);
            for source in imported.mesh.vertices() {
                // Recover promoted serialized f32 coordinates, not ideal pre-serialization values.
                check_object_round_trip(transform, angles, *source);
            }
            // Full value equality includes coordinates, indices/winding, source bounds,
            // diagnostics, original headers, facet normals/attributes and unit interpretation.
            assert_eq!(imported, original);
            assert_eq!(fs::read(&path).unwrap(), bytes); // Also implies unchanged file hashes.
        }
    }

    #[test]
    fn zero_scale_collapses_distinct_points_and_cannot_have_a_unique_reverse() {
        let transform = object(
            Vector3::new(7.0, -5.0, 3.0),
            Rotation3::around_z_radians(0.731).unwrap(),
            Vector3::new(0.0, 2.0, -1.0),
            Point3::new(2.0, -3.0, 4.0),
        );
        let first = Point3::new(5.0, 1.0, -2.0);
        let second = Point3::new(-7.0, 1.0, -2.0);
        assert_ne!(first, second);
        assert_eq!(
            transform.transform_point(first).unwrap(),
            transform.transform_point(second).unwrap()
        );
        assert_eq!(
            explicit_forward(transform).transform_point(first).unwrap(),
            explicit_forward(transform).transform_point(second).unwrap()
        );
        // Two different preimages of one result rule out a unique inverse; never divide by zero.
    }
}
