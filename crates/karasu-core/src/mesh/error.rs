//! Structured failures for mesh construction and STL import.

use crate::cancellation::Cancelled;
use std::{error::Error, fmt, io};

/// An invalid mesh or a failed fallible allocation. Geometry is never partially published.
#[derive(Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum MeshError {
    /// Caller-requested cancellation, distinct from invalid geometry or allocation failure.
    Cancelled(Cancelled),
    /// A coordinate is NaN or infinite.
    NonFiniteVertex {
        /// Vertex buffer index.
        vertex: usize,
        /// Coordinate index: 0=X, 1=Y, 2=Z.
        axis: usize,
    },
    /// A face refers outside the vertex buffer.
    InvalidIndex {
        /// Triangle index.
        triangle: usize,
        /// Corner index, 0 through 2.
        corner: usize,
        /// Invalid vertex index.
        index: u32,
        /// Number of available vertices.
        vertex_count: usize,
    },
    /// Finite endpoints have an extent that cannot be represented as finite `f64`.
    UnrepresentableBounds,
    /// A buffer or hash table could not reserve memory.
    Allocation {
        /// Purpose of the allocation, without private input data.
        context: &'static str,
    },
}

impl fmt::Display for MeshError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled(cancelled) => cancelled.fmt(f),
            Self::NonFiniteVertex { vertex, axis } => {
                write!(f, "vertex {vertex}, axis {axis}: coordinate must be finite")
            }
            Self::InvalidIndex { triangle, corner, index, vertex_count } => write!(
                f, "triangle {triangle}, corner {corner}: index {index} exceeds vertex count {vertex_count}"
            ),
            Self::UnrepresentableBounds => write!(f, "source bounds exceed finite f64 extent range"),
            Self::Allocation { context } => write!(f, "unable to reserve memory for {context}"),
        }
    }
}

impl Error for MeshError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Cancelled(cause) => Some(cause),
            _ => None,
        }
    }
}

impl From<Cancelled> for MeshError {
    fn from(value: Cancelled) -> Self {
        Self::Cancelled(value)
    }
}

/// A caller-controlled import resource.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImportResource {
    /// Total encoded input bytes.
    InputBytes,
    /// Facet records.
    Triangles,
    /// Exactly distinct vertex coordinates.
    Vertices,
    /// Bytes in one ASCII line, excluding its newline.
    AsciiLineBytes,
    /// Bytes in one ASCII numeric token.
    NumericTokenBytes,
}

/// A contextual external-input failure. I/O and mesh causes remain available through `source()`.
#[derive(Debug)]
#[non_exhaustive]
pub enum StlError {
    /// Caller-requested cancellation, including subordinate mesh/diagnostic phases.
    Cancelled(Cancelled),
    /// Underlying reader or file-open failure.
    Io(io::Error),
    /// No encoded STL was provided.
    EmptyInput,
    /// Auto detection cannot identify the representation.
    UnrecognizedFormat,
    /// Binary structure ends before the required absolute byte length.
    Truncated {
        /// Required byte length.
        expected: usize,
        /// Actual byte length.
        actual: usize,
    },
    /// Binary data contains bytes beyond its declared facet records.
    TrailingBytes {
        /// Length implied by the count.
        expected: usize,
        /// Actual byte length.
        actual: usize,
    },
    /// A count, size, or index cannot be represented on this platform.
    SizeOverflow,
    /// Input exceeds a configured cap.
    LimitExceeded {
        /// Resource being bounded.
        resource: ImportResource,
        /// Configured maximum.
        limit: usize,
    },
    /// ASCII grammar error, including unexpected end of input.
    AsciiSyntax {
        /// One-based line number.
        line: usize,
        /// Expected structure; does not echo arbitrary input text.
        expected: &'static str,
    },
    /// A numeric token cannot be parsed as `f64`.
    InvalidNumber {
        /// One-based line number.
        line: usize,
    },
    /// A vertex coordinate is NaN or infinite.
    NonFiniteCoordinate {
        /// Zero-based facet number.
        facet: usize,
        /// Zero-based corner number.
        corner: usize,
        /// Coordinate index: 0=X, 1=Y, 2=Z.
        axis: usize,
    },
    /// Mesh validation or allocation failed.
    Mesh(MeshError),
}

impl fmt::Display for StlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled(cancelled) => cancelled.fmt(f),
            Self::Io(e) => write!(f, "STL I/O failure: {e}"),
            Self::EmptyInput => write!(f, "STL input is empty"),
            Self::UnrecognizedFormat => write!(
                f,
                "unrecognized STL representation; select an explicit format if known"
            ),
            Self::Truncated { expected, actual } => write!(
                f,
                "truncated binary STL: need {expected} bytes, found {actual}"
            ),
            Self::TrailingBytes { expected, actual } => write!(
                f,
                "binary STL has trailing bytes: expected {expected}, found {actual}"
            ),
            Self::SizeOverflow => write!(f, "STL count, size, or index arithmetic overflow"),
            Self::LimitExceeded { resource, limit } => {
                write!(f, "STL {resource:?} limit exceeded ({limit})")
            }
            Self::AsciiSyntax { line, expected } => {
                write!(f, "ASCII STL line {line}: expected {expected}")
            }
            Self::InvalidNumber { line } => {
                write!(f, "ASCII STL line {line}: invalid numeric token")
            }
            Self::NonFiniteCoordinate {
                facet,
                corner,
                axis,
            } => write!(
                f,
                "STL facet {facet}, corner {corner}, axis {axis}: coordinate must be finite"
            ),
            Self::Mesh(e) => write!(f, "STL mesh failure: {e}"),
        }
    }
}

impl Error for StlError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Cancelled(cause) => Some(cause),
            Self::Io(e) => Some(e),
            Self::Mesh(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for StlError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<MeshError> for StlError {
    fn from(value: MeshError) -> Self {
        match value {
            MeshError::Cancelled(cause) => Self::Cancelled(cause),
            other => Self::Mesh(other),
        }
    }
}

impl From<Cancelled> for StlError {
    fn from(value: Cancelled) -> Self {
        Self::Cancelled(value)
    }
}
