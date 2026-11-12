//! Checked affine composition and the independent project object-transform convention.
//!
//! ```
//! use karasu_core::geometry::{ObjectTransform, Point3, Rotation3, Vector3};
//! let transform = ObjectTransform::new(
//!     Vector3::new(15.0, 10.0, 5.0),
//!     Rotation3::around_z_radians(std::f64::consts::FRAC_PI_2)?,
//!     Vector3::new(1.25, 0.8, 1.1),
//!     Point3::new(0.0, 0.0, 0.0),
//! )?;
//! let world = transform.transform_point(Point3::new(0.0, 0.0, 0.0))?;
//! assert_eq!(world, Point3::new(15.0, 10.0, 5.0));
//! // Explicit chronological composition: this object mapping, then another translation.
//! let composed = transform.to_affine()?.then(
//!     karasu_core::geometry::Affine3::from_translation(Vector3::new(0.0, 0.0, 2.0))?
//! )?;
//! assert_eq!(composed.transform_point(Point3::new(0.0, 0.0, 0.0))?.z, 7.0);
//! # Ok::<(), karasu_core::geometry::TransformError>(())
//! ```

use super::{
    transform_error::{finite, validate},
    Point3, Rotation3, TransformError, Vector3,
};

/// Finite affine coefficients: column-vector mapping `point_out = linear * point_in + translation`.
/// A general affine may include shear, reflection, or singularity. No inverse is offered in M2.2.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Affine3 {
    linear: [[f64; 3]; 3],
    translation: Vector3,
}

const IDENTITY_LINEAR: [[f64; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

fn dot(coefficients: [f64; 3], vector: [f64; 3]) -> Result<f64, TransformError> {
    let first = finite(coefficients[0] * vector[0], "linear product")?;
    let second = finite(coefficients[1] * vector[1], "linear product")?;
    let third = finite(coefficients[2] * vector[2], "linear product")?;
    finite(finite(first + second, "linear sum")? + third, "linear sum")
}

fn product(matrix: [[f64; 3]; 3], vector: [f64; 3]) -> Result<[f64; 3], TransformError> {
    let mut result = [0.0; 3];
    for (row, coefficients) in matrix.into_iter().enumerate() {
        result[row] = dot(coefficients, vector)?;
    }
    Ok(result)
}

fn add(
    left: [f64; 3],
    right: [f64; 3],
    operation: &'static str,
) -> Result<[f64; 3], TransformError> {
    let mut result = [0.0; 3];
    for axis in 0..3 {
        result[axis] = finite(left[axis] + right[axis], operation)?;
    }
    Ok(result)
}

fn point([x, y, z]: [f64; 3]) -> Point3 {
    Point3::new(x, y, z)
}
fn vector([x, y, z]: [f64; 3]) -> Vector3 {
    Vector3::new(x, y, z)
}

impl Affine3 {
    /// Identity mapping; the implicit fourth homogeneous row is always `[0,0,0,1]`.
    #[must_use]
    pub const fn identity() -> Self {
        Self {
            linear: IDENTITY_LINEAR,
            translation: Vector3::new(0.0, 0.0, 0.0),
        }
    }

    /// Accepts finite row-major coefficients without restricting determinant, shear, or singularity.
    ///
    /// # Errors
    /// Rejects non-finite coefficients/translation. Later application/composition is also checked.
    pub fn from_parts(linear: [[f64; 3]; 3], translation: Vector3) -> Result<Self, TransformError> {
        for (row, coefficients) in linear.iter().enumerate() {
            for (column, coefficient) in coefficients.iter().enumerate() {
                if !coefficient.is_finite() {
                    return Err(TransformError::NonFinite {
                        parameter: "linear matrix",
                        component: 3 * row + column,
                    });
                }
            }
        }
        validate(&translation.components(), "translation")?;
        Ok(Self {
            linear,
            translation,
        })
    }

    /// World/parent-space translation in millimeters, with identity linear part.
    ///
    /// # Errors
    /// Rejects non-finite components.
    pub fn from_translation(translation: Vector3) -> Result<Self, TransformError> {
        Self::from_parts(IDENTITY_LINEAR, translation)
    }

    /// Pure rotation from an already validated unit quaternion.
    #[must_use]
    pub fn from_rotation(rotation: Rotation3) -> Self {
        Self {
            linear: rotation.matrix(),
            translation: Vector3::new(0.0, 0.0, 0.0),
        }
    }

    /// Axis-aligned scale. Zero and negative finite factors are allowed without winding changes.
    ///
    /// # Errors
    /// Rejects non-finite factors. Zero is singular; no inverse is provided.
    pub fn from_scale(scale: Vector3) -> Result<Self, TransformError> {
        validate(&scale.components(), "scale")?;
        Self::from_parts(
            [
                [scale.x, 0.0, 0.0],
                [0.0, scale.y, 0.0],
                [0.0, 0.0, scale.z],
            ],
            Vector3::new(0.0, 0.0, 0.0),
        )
    }

    /// Equal scale factors on all axes, including zero/negative factors under the same policy.
    ///
    /// # Errors
    /// Rejects a non-finite factor.
    pub fn from_uniform_scale(factor: f64) -> Result<Self, TransformError> {
        Self::from_scale(Vector3::new(factor, factor, factor))
    }

    /// Applies this mapping first, then `next`: the result represents `next * self`.
    /// Fixed row/column/component evaluation order; no implicit commutation or decomposition.
    ///
    /// # Errors
    /// Rejects non-finite intermediate/composed coefficients, even if exact arithmetic could cancel.
    pub fn then(self, next: Self) -> Result<Self, TransformError> {
        let mut linear = [[0.0; 3]; 3];
        for (row, output_row) in linear.iter_mut().enumerate() {
            for (column, output) in output_row.iter_mut().enumerate() {
                let input = [
                    self.linear[0][column],
                    self.linear[1][column],
                    self.linear[2][column],
                ];
                *output = dot(next.linear[row], input)?;
            }
        }
        let translation = add(
            product(next.linear, self.translation.components())?,
            next.translation.components(),
            "affine composition translation",
        )?;
        Self::from_parts(linear, vector(translation))
    }

    /// Transforms a position, applying the linear part and then translation.
    ///
    /// # Errors
    /// Rejects non-finite input or arithmetic overflow. No coordinate clamping is performed.
    pub fn transform_point(self, input: Point3) -> Result<Point3, TransformError> {
        validate(&input.coordinates(), "point")?;
        Ok(point(add(
            product(self.linear, input.coordinates())?,
            self.translation.components(),
            "point translation",
        )?))
    }

    /// Transforms a direction/displacement by the linear part only, never by translation.
    /// This is not a normal-vector transform under non-uniform scaling.
    ///
    /// # Errors
    /// Rejects non-finite input or arithmetic overflow.
    pub fn transform_vector(self, input: Vector3) -> Result<Vector3, TransformError> {
        validate(&input.components(), "direction")?;
        Ok(vector(product(self.linear, input.components())?))
    }

    /// Row-major storage of the linear coefficients, acting on column vectors.
    #[must_use]
    pub const fn linear(self) -> [[f64; 3]; 3] {
        self.linear
    }

    /// Output-space translation in millimeters.
    #[must_use]
    pub const fn translation(self) -> Vector3 {
        self.translation
    }
}

/// Immutable scene parameters, separate from source mesh/metadata:
/// `p_world = t + c + R * (S * (p_local - c))`. Pivot `c` is local, scale acts on local axes,
/// rotation is active/right-handed, and translation `t` is applied last in world space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ObjectTransform {
    translation: Vector3,
    rotation: Rotation3,
    scale: Vector3,
    pivot: Point3,
}

impl ObjectTransform {
    /// Identity with zero translation/pivot, identity rotation, and unit scale.
    #[must_use]
    pub const fn identity() -> Self {
        Self {
            translation: Vector3::new(0.0, 0.0, 0.0),
            rotation: Rotation3::identity(),
            scale: Vector3::new(1.0, 1.0, 1.0),
            pivot: Point3::new(0.0, 0.0, 0.0),
        }
    }

    /// Validates finite parameters. Zero scales collapse dimensions; negative scales reflect.
    /// Neither changes source winding, repairs geometry, or implies an invertible transform.
    ///
    /// # Errors
    /// Rejects non-finite translation, scale, or pivot. Rotation is already validated by its type.
    pub fn new(
        translation: Vector3,
        rotation: Rotation3,
        scale: Vector3,
        pivot: Point3,
    ) -> Result<Self, TransformError> {
        validate(&translation.components(), "translation")?;
        validate(&scale.components(), "scale")?;
        validate(&pivot.coordinates(), "pivot")?;
        Ok(Self {
            translation,
            rotation,
            scale,
            pivot,
        })
    }

    /// Evaluates subtract-pivot → local scale → rotation → add-pivot → world translation.
    /// Exact identity linear parameters bypass pivot arithmetic to avoid gratuitous cancellation.
    ///
    /// # Errors
    /// Rejects non-finite input or any non-finite intermediate/result; no implicit repositioning.
    pub fn transform_point(self, input: Point3) -> Result<Point3, TransformError> {
        validate(&input.coordinates(), "point")?;
        let rotation = self.rotation.matrix();
        if rotation == IDENTITY_LINEAR && self.scale == Vector3::new(1.0, 1.0, 1.0) {
            return Ok(point(add(
                input.coordinates(),
                self.translation.components(),
                "point translation",
            )?));
        }
        let pivot = self.pivot.coordinates();
        let local = add(
            input.coordinates(),
            pivot.map(|value| -value),
            "subtract local pivot",
        )?;
        let mut scaled = [0.0; 3];
        for ((output, coordinate), factor) in
            scaled.iter_mut().zip(local).zip(self.scale.components())
        {
            *output = finite(coordinate * factor, "local scale")?;
        }
        let rotated = product(rotation, scaled)?;
        Ok(point(add(
            add(rotated, pivot, "add local pivot")?,
            self.translation.components(),
            "world translation",
        )?))
    }

    /// Applies local scale and rotation without pivot/translation. Does not transform normals.
    ///
    /// # Errors
    /// Rejects non-finite inputs or overflow.
    pub fn transform_vector(self, input: Vector3) -> Result<Vector3, TransformError> {
        validate(&input.components(), "direction")?;
        let mut scaled = [0.0; 3];
        for ((output, component), factor) in scaled
            .iter_mut()
            .zip(input.components())
            .zip(self.scale.components())
        {
            *output = finite(component * factor, "direction scale")?;
        }
        Ok(vector(product(self.rotation.matrix(), scaled)?))
    }

    /// Flattens to `L = R*S`, `b = (c - L*c) + t`, equivalent to
    /// `T(t) * T(c) * R * S * T(-c)` for column vectors. Explicit affine composition may retain
    /// shear and cannot generally be decomposed back into one object transform.
    ///
    /// # Errors
    /// Rejects non-finite coefficient arithmetic. Pivot cancellation/rounding can differ from
    /// direct evaluation; even an object usable at a given point may lack this finite flattening.
    pub fn to_affine(self) -> Result<Affine3, TransformError> {
        let mut linear = self.rotation.matrix();
        for row in &mut linear {
            for (coefficient, factor) in row.iter_mut().zip(self.scale.components()) {
                *coefficient = finite(*coefficient * factor, "object linear coefficients")?;
            }
        }
        let pivot = self.pivot.coordinates();
        let mapped = product(linear, pivot)?;
        let offset = add(
            add(pivot, mapped.map(|value| -value), "affine pivot offset")?,
            self.translation.components(),
            "affine world translation",
        )?;
        Affine3::from_parts(linear, vector(offset))
    }

    /// World-space translation, in millimeters.
    #[must_use]
    pub const fn translation(self) -> Vector3 {
        self.translation
    }
    /// Unit-quaternion active rotation.
    #[must_use]
    pub const fn rotation(self) -> Rotation3 {
        self.rotation
    }
    /// Local-axis scale factors (dimensionless).
    #[must_use]
    pub const fn scale(self) -> Vector3 {
        self.scale
    }
    /// Source/local-space pivot in millimeters.
    #[must_use]
    pub const fn pivot(self) -> Point3 {
        self.pivot
    }
}
