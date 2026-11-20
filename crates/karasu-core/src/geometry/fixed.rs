//! Trigonometry and lengths from fixed IEEE operations only, so that results are bit-identical on
//! every platform (TD-061, issue #21).
//!
//! The platform math library (`sin`, `cos`, `tan`, `acos`, `hypot`, …) may differ in the last bit
//! between operating systems. Code whose decisions must agree everywhere — the transforms of
//! TD-061 and the support planners — uses these functions instead: they are built from
//! addition, subtraction, multiplication, division and square root, which IEEE 754 rounds
//! correctly, and the compiler does not fuse them.

/// Sine and cosine of a finite angle in degrees, from fixed IEEE operations only (no platform
/// math library), so results are identical on every platform.
///
/// The angle is reduced exactly: the remainder modulo 360 is exact, and each reflection into
/// `[0, 45]` degrees is an exact subtraction (Sterbenz). Only the conversion to radians and the
/// Taylor polynomials round; on `[0, π/4]` the omitted terms are below 2^-60 relative. Multiples
/// of 90 degrees give exact results.
#[must_use]
pub fn sin_cos_degrees(degrees: f64) -> (f64, f64) {
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

/// The length `√(dx² + dy²)` of the plan vector `(dx, dy)`, from fixed IEEE operations: unlike
/// the platform's `hypot`, identical on every platform. Model coordinates in millimetres neither
/// overflow nor underflow the squares.
#[must_use]
pub fn plan_length(dx: f64, dy: f64) -> f64 {
    (dx * dx + dy * dy).sqrt()
}

/// The tangent of a finite angle in degrees, as the quotient of [`sin_cos_degrees`]: identical on
/// every platform. Infinite or very large near odd multiples of 90 degrees.
#[must_use]
pub fn tan_degrees(degrees: f64) -> f64 {
    let (sine, cosine) = sin_cos_degrees(degrees);
    sine / cosine
}

/// The angle in degrees, in `[0, 180]`, whose cosine is `cosine` (clamped to `[-1, 1]`), from
/// fixed IEEE operations only, so results are identical on every platform. NaN gives NaN.
///
/// `acos c = 2 atan t` with `t = √((1 − c)/(1 + c))`, where `1 − c` and `1 + c` are exact near
/// either end (Sterbenz). The arc tangent folds `t > 1` to `90° − atan(1/t)`, halves its angle
/// three times with `atan u = 2 atan(u / (1 + √(1 + u²)))` to `|u| ≤ tan(π/32)`, and sums the
/// Taylor series through `u^19`, whose omitted terms are below 2^-60 relative. A cosine of 1, 0
/// or −1 gives exactly 0, 90 or 180 degrees.
#[must_use]
pub fn acos_degrees(cosine: f64) -> f64 {
    let c = cosine.clamp(-1.0, 1.0);
    if c.is_nan() {
        return f64::NAN;
    }
    if c <= -1.0 {
        return 180.0;
    }
    if c == 0.0 {
        return 90.0;
    }
    let t = ((1.0 - c) / (1.0 + c)).sqrt();
    2.0 * atan_degrees(t)
}

/// The arc tangent in degrees of a finite `t ≥ 0`, as [`acos_degrees`] describes.
fn atan_degrees(t: f64) -> f64 {
    let (base, sign, mut u) = if t > 1.0 {
        (90.0, -1.0, 1.0 / t)
    } else {
        (0.0, 1.0, t)
    };
    for _ in 0..3 {
        u /= 1.0 + (1.0 + u * u).sqrt();
    }
    // atan u = u (1 − u²/3 + u⁴/5 − …), in Horner form.
    let square = u * u;
    let mut series = 0.0;
    for k in (0..=9_u8).rev() {
        series = 1.0 / (2.0 * f64::from(k) + 1.0) - square * series;
    }
    // Three halvings: the angle is eight times the reduced one.
    base + sign * (8.0 * u * series) * (180.0 / std::f64::consts::PI)
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
    fn plan_length_is_the_root_of_the_squares() {
        assert_eq!(plan_length(3.0, 4.0), 5.0);
        assert_eq!(plan_length(-3.0, 0.0), 3.0);
        assert!((plan_length(0.1, 0.2) - 0.1_f64.hypot(0.2)).abs() <= f64::EPSILON);
    }

    #[test]
    fn degree_tangent_is_the_quotient() {
        assert_eq!(tan_degrees(0.0), 0.0);
        assert!((tan_degrees(45.0) - 1.0).abs() <= f64::EPSILON);
        assert!((tan_degrees(44.5) - 44.5_f64.to_radians().tan()).abs() < 1e-15);
        assert!((tan_degrees(-30.0) + 30.0_f64.to_radians().tan()).abs() < 1e-15);
    }

    #[test]
    fn degree_arc_cosine_is_exact_at_the_ends_and_accurate_elsewhere() {
        assert_eq!(acos_degrees(1.0).to_bits(), 0.0_f64.to_bits());
        assert_eq!(acos_degrees(0.0), 90.0);
        assert_eq!(acos_degrees(-0.0), 90.0);
        assert_eq!(acos_degrees(-1.0), 180.0);
        // Clamped outside [-1, 1], as a normal's rounding can leave it.
        assert_eq!(acos_degrees(1.0 + 1e-15), 0.0);
        assert_eq!(acos_degrees(-1.0 - 1e-15), 180.0);
        assert!(acos_degrees(f64::NAN).is_nan());
        assert!((acos_degrees(0.5) - 60.0).abs() < 1e-13);
        let mut c = -1.0_f64;
        while c <= 1.0 {
            let expected = c.acos().to_degrees();
            assert!((acos_degrees(c) - expected).abs() < 1e-12, "{c}");
            c += 0.001_3;
        }
        // Near either end, where the angle changes fastest.
        for c in [1.0_f64 - 1e-12, 1.0 - 1e-6, -1.0 + 1e-12, -1.0 + 1e-6] {
            let expected = c.acos().to_degrees();
            assert!(
                (acos_degrees(c) - expected).abs() <= 1e-12 * expected.max(1.0),
                "{c}"
            );
        }
        // It inverts the cosine.
        let mut degrees = 0.0;
        while degrees <= 180.0 {
            let round_trip = acos_degrees(sin_cos_degrees(degrees).1);
            assert!((round_trip - degrees).abs() < 1e-11, "{degrees}");
            degrees += 0.25;
        }
    }
}
