use std::{error::Error, fmt};

/// Invalid transform input or non-finite intermediate/output arithmetic. No partial value is returned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum TransformError {
    /// A supplied component is NaN or infinite.
    NonFinite {
        /// Input name (e.g. `pivot`, `point`, or `linear matrix`).
        parameter: &'static str,
        /// Component index: X/Y/Z, X/Y/Z/W, or flattened row-major matrix index.
        component: usize,
    },
    /// An exactly zero quaternion cannot define a rotation.
    ZeroQuaternion,
    /// An exactly zero axis cannot define an axis-angle rotation, even for a zero angle.
    ZeroRotationAxis,
    /// Finite inputs produced a non-finite arithmetic result or intermediate.
    ArithmeticOverflow {
        /// Operation being evaluated; no input coordinates are copied into errors.
        operation: &'static str,
    },
}

impl fmt::Display for TransformError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonFinite {
                parameter,
                component,
            } => write!(
                formatter,
                "transform {parameter}, component {component}: value must be finite"
            ),
            Self::ZeroQuaternion => {
                write!(formatter, "a zero quaternion does not define a rotation")
            }
            Self::ZeroRotationAxis => write!(
                formatter,
                "a zero axis does not define an axis-angle rotation"
            ),
            Self::ArithmeticOverflow { operation } => write!(
                formatter,
                "non-finite arithmetic while evaluating {operation}"
            ),
        }
    }
}

impl Error for TransformError {}

pub(super) fn validate(values: &[f64], parameter: &'static str) -> Result<(), TransformError> {
    for (component, value) in values.iter().enumerate() {
        if !value.is_finite() {
            return Err(TransformError::NonFinite {
                parameter,
                component,
            });
        }
    }
    Ok(())
}

pub(super) fn finite(value: f64, operation: &'static str) -> Result<f64, TransformError> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(TransformError::ArithmeticOverflow { operation })
    }
}
