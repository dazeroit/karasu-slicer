use super::{transform_error::validate, TransformError, Vector3};

/// Right-handed active rotation represented by a finite unit quaternion in X/Y/Z/W order.
/// Inputs/compositions are explicitly normalized without an epsilon. Quaternion equality is
/// representation equality, not an approximate test of equivalent orientations (q and -q).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rotation3 {
    quaternion: [f64; 4],
}

impl Rotation3 {
    /// No rotation.
    #[must_use]
    pub const fn identity() -> Self {
        Self {
            quaternion: [0.0, 0.0, 0.0, 1.0],
        }
    }

    /// Normalizes a finite nonzero quaternion using maximum-component scaling to avoid norm
    /// overflow/underflow. Very small relative components may round/underflow as ordinary `f64`.
    ///
    /// # Errors
    /// Rejects non-finite components and the exactly zero quaternion; no near-zero cutoff is used.
    pub fn from_quaternion_xyzw(quaternion: [f64; 4]) -> Result<Self, TransformError> {
        validate(&quaternion, "quaternion")?;
        let scale = quaternion
            .iter()
            .fold(0.0_f64, |largest, value| largest.max(value.abs()));
        if scale == 0.0 {
            return Err(TransformError::ZeroQuaternion);
        }
        let scaled = quaternion.map(|value| value / scale);
        let length = scaled
            .iter()
            .fold(0.0, |sum, value| sum + value * value)
            .sqrt();
        Ok(Self {
            quaternion: scaled.map(|value| value / length),
        })
    }

    /// Builds a right-hand-rule rotation around a source-axis direction. Angle is in radians;
    /// the finite nonzero axis is normalized without a length tolerance.
    ///
    /// # Errors
    /// Rejects non-finite angle/axis components or an exactly zero axis.
    pub fn from_axis_angle_radians(axis: Vector3, angle: f64) -> Result<Self, TransformError> {
        validate(&axis.components(), "rotation axis")?;
        validate(&[angle], "angle radians")?;
        let axis = axis.unit().ok_or(TransformError::ZeroRotationAxis)?;
        let (sine, cosine) = (angle * 0.5).sin_cos();
        Self::from_quaternion_xyzw([axis.x * sine, axis.y * sine, axis.z * sine, cosine])
    }

    /// Active fixed-axis (extrinsic) X, then Y, then Z rotations, in radians. With column vectors,
    /// `R = Rz * Ry * Rx`. No Euler-angle extraction or uniqueness is implied.
    ///
    /// # Errors
    /// Rejects any non-finite angle component.
    pub fn from_euler_xyz_radians(angles: Vector3) -> Result<Self, TransformError> {
        validate(&angles.components(), "Euler radians")?;
        let rotate_x = Self::around_x_radians(angles.x)?;
        let rotate_y = Self::around_y_radians(angles.y)?;
        let rotate_z = Self::around_z_radians(angles.z)?;
        rotate_x.then(rotate_y)?.then(rotate_z)
    }

    /// Active fixed-axis (extrinsic) X, then Y, then Z rotations, in degrees, as
    /// [`Self::from_euler_xyz_radians`].
    ///
    /// Unlike the radian constructors, which use the platform's `sin_cos`, the half-angle sines and
    /// cosines are evaluated with fixed IEEE operations only, so the rotation is bit-identical on
    /// every platform. Angles that are multiples of 180 degrees give exact quaternion components.
    ///
    /// # Errors
    /// Rejects any non-finite angle component.
    pub fn from_euler_xyz_degrees(angles: Vector3) -> Result<Self, TransformError> {
        validate(&angles.components(), "Euler degrees")?;
        let axis = |index: usize, degrees: f64| {
            let (sine, cosine) = sin_cos_degrees(degrees * 0.5);
            let mut quaternion = [0.0, 0.0, 0.0, cosine];
            quaternion[index] = sine;
            Self::from_quaternion_xyzw(quaternion)
        };
        axis(0, angles.x)?
            .then(axis(1, angles.y)?)?
            .then(axis(2, angles.z)?)
    }

    /// Right-hand-rule active X rotation in radians.
    ///
    /// # Errors
    /// Rejects a non-finite angle.
    pub fn around_x_radians(angle: f64) -> Result<Self, TransformError> {
        Self::from_axis_angle_radians(Vector3::new(1.0, 0.0, 0.0), angle)
    }

    /// Right-hand-rule active Y rotation in radians.
    ///
    /// # Errors
    /// Rejects a non-finite angle.
    pub fn around_y_radians(angle: f64) -> Result<Self, TransformError> {
        Self::from_axis_angle_radians(Vector3::new(0.0, 1.0, 0.0), angle)
    }

    /// Right-hand-rule active Z rotation in radians.
    ///
    /// # Errors
    /// Rejects a non-finite angle.
    pub fn around_z_radians(angle: f64) -> Result<Self, TransformError> {
        Self::from_axis_angle_radians(Vector3::new(0.0, 0.0, 1.0), angle)
    }

    /// Applies this rotation first, then `next`: Hamilton product `q_next * q_self`.
    /// The bounded product is renormalized to avoid accumulated quaternion norm drift.
    ///
    /// # Errors
    /// Propagates quaternion validation failures; finite unit inputs have a nonzero product.
    pub fn then(self, next: Self) -> Result<Self, TransformError> {
        let [ax, ay, az, aw] = self.quaternion;
        let [bx, by, bz, bw] = next.quaternion;
        Self::from_quaternion_xyzw([
            bw * ax + bx * aw + by * az - bz * ay,
            bw * ay - bx * az + by * aw + bz * ax,
            bw * az + bx * ay - by * ax + bz * aw,
            bw * aw - bx * ax - by * ay - bz * az,
        ])
    }

    /// Normalized X/Y/Z/W components. No canonical sign is imposed.
    #[must_use]
    pub const fn quaternion_xyzw(self) -> [f64; 4] {
        self.quaternion
    }

    /// Row-major coefficient storage for a 3×3 matrix acting on column vectors. No translation.
    /// Entries are rounded `f64`; orthogonality is approximate, never repaired with a global epsilon.
    #[must_use]
    pub fn matrix(self) -> [[f64; 3]; 3] {
        let [x, y, z, w] = self.quaternion;
        [
            [
                1.0 - 2.0 * (y * y + z * z),
                2.0 * (x * y - z * w),
                2.0 * (x * z + y * w),
            ],
            [
                2.0 * (x * y + z * w),
                1.0 - 2.0 * (x * x + z * z),
                2.0 * (y * z - x * w),
            ],
            [
                2.0 * (x * z - y * w),
                2.0 * (y * z + x * w),
                1.0 - 2.0 * (x * x + y * y),
            ],
        ]
    }
}

/// Sine and cosine of a finite angle in degrees, from fixed IEEE operations only (no platform
/// math library), so results are identical on every platform.
///
/// The angle is reduced exactly: the remainder modulo 360 is exact, and each reflection into
/// `[0, 45]` degrees is an exact subtraction (Sterbenz). Only the conversion to radians and the
/// Taylor polynomials round; on `[0, π/4]` the omitted terms are below 2^-60 relative. Multiples
/// of 90 degrees give exact results.
pub(crate) fn sin_cos_degrees(degrees: f64) -> (f64, f64) {
    let mut angle = degrees % 360.0;
    if angle < 0.0 {
        angle += 360.0;
    }
    // Quadrant symmetries: sin(a + 180) = -sin a, sin(180 - a) = sin a, cos(180 - a) = -cos a.
    let (mut sine_sign, mut cosine_sign) = (1.0, 1.0);
    if angle >= 180.0 {
        angle -= 180.0;
        sine_sign = -sine_sign;
        cosine_sign = -cosine_sign;
    }
    if angle > 90.0 {
        angle = 180.0 - angle;
        cosine_sign = -cosine_sign;
    }
    // sin(90 - a) = cos a.
    let swap = angle > 45.0;
    if swap {
        angle = 90.0 - angle;
    }
    let radians = angle * (std::f64::consts::PI / 180.0);
    let square = radians * radians;
    // Horner forms of the Taylor series through x^19 and x^18:
    // sin x = x (1 - x²/(2·3) (1 - x²/(4·5) (…))), cos x = 1 - x²/(1·2) (1 - x²/(3·4) (…)).
    let (mut sine, mut cosine) = (1.0, 1.0);
    for k in (1..=9_u8).rev() {
        let even = 2.0 * f64::from(k);
        sine = 1.0 - square / (even * (even + 1.0)) * sine;
        cosine = 1.0 - square / ((even - 1.0) * even) * cosine;
    }
    let sine = radians * sine;
    let (sine, cosine) = if swap { (cosine, sine) } else { (sine, cosine) };
    (sine_sign * sine, cosine_sign * cosine)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn degree_sine_and_cosine_are_exact_at_right_angles_and_accurate_elsewhere() {
        for (degrees, expected) in [
            (0.0, (0.0, 1.0)),
            (90.0, (1.0, 0.0)),
            (180.0, (0.0, -1.0)),
            (270.0, (-1.0, 0.0)),
            (360.0, (0.0, 1.0)),
            (-90.0, (-1.0, 0.0)),
            (450.0, (1.0, 0.0)),
        ] {
            let (sine, cosine) = sin_cos_degrees(degrees);
            // Signed zeros compare equal, as the rotation algebra treats them.
            assert_eq!((sine, cosine), expected, "{degrees}");
        }
        assert!((sin_cos_degrees(30.0).0 - 0.5).abs() <= f64::EPSILON / 2.0);
        let mut degrees = -720.0;
        while degrees <= 720.0 {
            let (sine, cosine) = sin_cos_degrees(degrees);
            // The platform functions on the reduced angle, whose conversion to radians rounds.
            let (s, c) = degrees.rem_euclid(360.0).to_radians().sin_cos();
            assert!(
                (sine - s).abs() < 1e-15 && (cosine - c).abs() < 1e-15,
                "{degrees}"
            );
            degrees += 0.37;
        }
    }

    #[test]
    fn degree_euler_rotations_match_the_radian_convention() {
        let angles = [10.0, -35.5, 123.25];
        let degrees =
            Rotation3::from_euler_xyz_degrees(Vector3::new(angles[0], angles[1], angles[2]))
                .unwrap();
        let radians = Rotation3::from_euler_xyz_radians(Vector3::new(
            angles[0].to_radians(),
            angles[1].to_radians(),
            angles[2].to_radians(),
        ))
        .unwrap();
        for (row_d, row_r) in degrees.matrix().iter().zip(radians.matrix()) {
            for (d, r) in row_d.iter().zip(row_r) {
                assert!((d - r).abs() < 1e-15);
            }
        }
        // A half turn about Z is exact.
        let half = Rotation3::from_euler_xyz_degrees(Vector3::new(0.0, 0.0, 180.0)).unwrap();
        assert_eq!(
            half.matrix(),
            [[-1.0, 0.0, 0.0], [0.0, -1.0, 0.0], [0.0, 0.0, 1.0]]
        );
        assert!(Rotation3::from_euler_xyz_degrees(Vector3::new(f64::NAN, 0.0, 0.0)).is_err());
    }
}
