//! The Boolean kernel's grid (TD-140): every input vertex is snapped once to the nearest point of
//! a fixed global grid of 2⁻¹⁵ mm (about 0.03 µm), ties to even.
//!
//! **Policy.** The snap moves a vertex by at most half a step on each axis, 2⁻¹⁶ mm (0.015 µm),
//! far below any resin printer's pixel; it is a stated property of every Boolean result, never a
//! silent change. Grid points are exact `f64` values both ways, so a vertex already on the grid
//! (any result of the kernel) is not moved again; within ±512 mm they are exact `f32` values too
//! (a multiple of 2⁻¹⁵ below 2⁹ has at most 24 significant bits), so a model file of any format
//! stores a result on the grid exactly (phase 5). Coordinates must be finite and within the
//! admitted ±2,000 mm, so a grid coordinate is an integer of magnitude at most 65,536,000 < 2²⁶:
//! the bound every later width in [`super::plane`] is derived from. A coordinate outside it is
//! refused with the axis and the value, never clamped.
//!
//! The grid was 2⁻¹² mm (and ±10,000 mm) until phase 5 (2026-11-09): the bounds are in steps, so
//! a finer grid over a smaller range keeps every proof, and the finer grid is what lets a result
//! rounded to it keep its slivers apart (TD-140 phase 5).
use crate::geometry::Point3;
use std::{error::Error, fmt};

/// Grid steps per millimetre: 2¹⁵.
pub(crate) const STEPS_PER_MM: f64 = 32_768.0;
/// The largest admitted coordinate magnitude, in millimetres.
pub(crate) const MAX_ABS_MM: f64 = 2_000.0;
/// The largest grid coordinate magnitude: 2,000 mm in steps, below 2²⁶.
pub(crate) const MAX_ABS_STEPS: i64 = 65_536_000;

/// A point of the grid, in steps of 2⁻¹⁵ mm.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct GridPoint {
    pub(crate) x: i64,
    pub(crate) y: i64,
    pub(crate) z: i64,
}

/// Why a point cannot be snapped.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum GridError {
    /// A coordinate is not finite.
    NotFinite { axis: usize, value: f64 },
    /// A coordinate lies beyond ±2,000 mm.
    OutOfRange { axis: usize, value: f64 },
}

impl fmt::Display for GridError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = |axis: usize| ["x", "y", "z"].get(axis).copied().unwrap_or("?");
        match self {
            Self::NotFinite { axis, value } => {
                write!(f, "coordinate {} is not finite ({value})", name(*axis))
            }
            Self::OutOfRange { axis, value } => write!(
                f,
                "coordinate {} is {value} mm, beyond the admitted ±{MAX_ABS_MM} mm",
                name(*axis)
            ),
        }
    }
}

impl Error for GridError {}

/// One coordinate in steps, rounded to the nearest step, ties to even.
fn snap_coordinate(axis: usize, value: f64) -> Result<i64, GridError> {
    if !value.is_finite() {
        return Err(GridError::NotFinite { axis, value });
    }
    if value.abs() > MAX_ABS_MM {
        return Err(GridError::OutOfRange { axis, value });
    }
    // Multiplying by a power of two is exact; the rounded value is an integer of magnitude at most
    // 65,536,000, which converts to `i64` exactly.
    #[allow(clippy::cast_possible_truncation)] // An integer below 2^26, exact.
    let steps = (value * STEPS_PER_MM).round_ties_even() as i64;
    debug_assert!(steps.abs() <= MAX_ABS_STEPS);
    Ok(steps)
}

impl GridPoint {
    /// `point` snapped to the grid.
    ///
    /// # Errors
    /// A coordinate that is not finite or lies beyond ±2,000 mm.
    pub(crate) fn snap(point: Point3) -> Result<Self, GridError> {
        Ok(Self {
            x: snap_coordinate(0, point.x)?,
            y: snap_coordinate(1, point.y)?,
            z: snap_coordinate(2, point.z)?,
        })
    }

    /// The point in millimetres, exactly.
    pub(crate) fn point(self) -> Point3 {
        #[allow(clippy::cast_precision_loss)] // Below 2^26: exact.
        let mm = |steps: i64| steps as f64 / STEPS_PER_MM;
        Point3::new(mm(self.x), mm(self.y), mm(self.z))
    }

    /// Its coordinates.
    pub(crate) const fn coordinates(self) -> [i64; 3] {
        [self.x, self.y, self.z]
    }
}

#[cfg(test)]
mod tests {
    use super::{GridError, GridPoint, MAX_ABS_STEPS, STEPS_PER_MM};
    use crate::geometry::Point3;

    #[test]
    fn points_snap_to_the_nearest_step_ties_to_even_and_come_back_exactly() {
        let half = 0.5 / STEPS_PER_MM;
        let snapped = GridPoint::snap(Point3::new(1.0, -2.5, half)).unwrap();
        assert_eq!(snapped.coordinates(), [32_768, -81_920, 0]);
        // 1.5 steps rounds to 2, 2.5 steps to 2: ties to even.
        let ties = GridPoint::snap(Point3::new(
            1.5 / STEPS_PER_MM,
            2.5 / STEPS_PER_MM,
            -half * 3.0,
        ))
        .unwrap();
        assert_eq!(ties.coordinates(), [2, 2, -2]);
        // A grid point is not moved again, and its millimetres are exact.
        let again = GridPoint::snap(snapped.point()).unwrap();
        assert_eq!(again, snapped);
        // The displacement is at most half a step.
        let p = Point3::new(12.345_678_9, -0.000_1, 1_999.999_999);
        let q = GridPoint::snap(p).unwrap().point();
        for (a, b) in [(p.x, q.x), (p.y, q.y), (p.z, q.z)] {
            assert!((a - b).abs() <= half);
        }
    }

    #[test]
    fn the_range_is_admitted_to_its_ends_and_nothing_beyond() {
        let end = GridPoint::snap(Point3::new(2_000.0, -2_000.0, 0.0)).unwrap();
        assert_eq!(end.coordinates(), [MAX_ABS_STEPS, -MAX_ABS_STEPS, 0]);
        const { assert!(MAX_ABS_STEPS < 1 << 26) };
        assert_eq!(
            GridPoint::snap(Point3::new(0.0, 2_000.000_1, 0.0)),
            Err(GridError::OutOfRange {
                axis: 1,
                value: 2_000.000_1
            })
        );
        assert!(matches!(
            GridPoint::snap(Point3::new(0.0, 0.0, f64::NAN)),
            Err(GridError::NotFinite { axis: 2, .. })
        ));
        let message = GridPoint::snap(Point3::new(-20_000.0, 0.0, 0.0))
            .unwrap_err()
            .to_string();
        assert!(message.contains("coordinate x is -20000 mm"), "{message}");
        // Grid points within ±512 mm are f32 values: a model file stores them exactly.
        for mm in [511.999_969_482_421_9, -300.123_456, 0.000_03] {
            let g = GridPoint::snap(Point3::new(mm, 0.0, 0.0)).unwrap().point();
            #[allow(clippy::cast_possible_truncation)]
            let stored = f64::from(g.x as f32);
            assert_eq!(stored.to_bits(), g.x.to_bits(), "{mm}");
        }
    }
}
