//! Explicit uniform world-Z layer plans with exact dyadic membership and one binary64 rounding.
//!
//! No first-plane, printer, mesh-bound, exposure or compatibility default is selected (TD-027).
//! Counts and indices use u64 independently of host storage. Output is atomic and strictly increasing;
//! exact off-terminal membership may publish a rounded scalar equal to that terminal.
//!
//! ```
//! use karasu_core::slicing::schedule::{build_layer_schedule, ScheduleEnd, ScheduleLimits,
//!     UniformScheduleSpec};
//! let spec = UniformScheduleSpec::try_new(0.0, 0.25, ScheduleEnd::Through(1.0))?;
//! let schedule = build_layer_schedule(spec, ScheduleLimits {
//!     max_layers: 100, max_payload_bytes: 4096,
//! })?;
//! assert_eq!(schedule.planes().len(), 5);
//! assert_eq!(schedule.planes()[4].z_mm, 1.0);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use crate::cancellation::{Cancellation, Cancelled, Checkpoint};
use std::{cmp::Ordering, error::Error, fmt};

/// Exactly one explicit termination rule, evaluated before rounding plane coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ScheduleEnd {
    /// Exactly this many planes, including an explicitly empty count.
    Count(u64),
    /// Exact mathematical planes strictly below this finite world-Z in mm.
    Before(f64),
    /// Exact mathematical planes at or below this finite world-Z in mm.
    Through(f64),
}

/// Validated explicit world-Z configuration; inputs name their represented dyadic values in mm.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UniformScheduleSpec {
    first_plane_z_mm: f64,
    layer_height_mm: f64,
    end: ScheduleEnd,
}
impl UniformScheduleSpec {
    /// Validate first Z, positive height, then terminal Z. Signed-zero positions become +0.
    /// # Errors
    /// Returns a structured invalid-input reason, even when the requested count is zero.
    pub fn try_new(
        first_plane_z_mm: f64,
        layer_height_mm: f64,
        end: ScheduleEnd,
    ) -> std::result::Result<Self, ScheduleSpecError> {
        if !first_plane_z_mm.is_finite() {
            return Err(ScheduleSpecError::NonFiniteFirstPlane);
        }
        if !layer_height_mm.is_finite() || layer_height_mm <= 0.0 {
            return Err(ScheduleSpecError::InvalidHeight);
        }
        let end = match end {
            ScheduleEnd::Count(n) => ScheduleEnd::Count(n),
            ScheduleEnd::Before(z) | ScheduleEnd::Through(z) => {
                if !z.is_finite() {
                    return Err(ScheduleSpecError::NonFiniteTerminal);
                }
                let canonical = if z == 0.0 { 0.0 } else { z };
                if matches!(end, ScheduleEnd::Before(_)) {
                    ScheduleEnd::Before(canonical)
                } else {
                    ScheduleEnd::Through(canonical)
                }
            }
        };
        Ok(Self {
            first_plane_z_mm: if first_plane_z_mm == 0.0 {
                0.0
            } else {
                first_plane_z_mm
            },
            layer_height_mm,
            end,
        })
    }
    /// Explicit first world-Z in millimeters, canonicalizing zero only.
    #[must_use]
    pub const fn first_plane_z_mm(&self) -> f64 {
        self.first_plane_z_mm
    }
    /// Unchanged strictly positive nominal height in millimeters.
    #[must_use]
    pub const fn layer_height_mm(&self) -> f64 {
        self.layer_height_mm
    }
    /// Explicit count or exact terminal membership rule.
    #[must_use]
    pub const fn end(&self) -> ScheduleEnd {
        self.end
    }
}

/// Invalid explicit configuration, checked before a builder is called.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScheduleSpecError {
    /// First Z is NaN or infinite.
    NonFiniteFirstPlane,
    /// Height is nonfinite, zero (of either sign), or negative.
    InvalidHeight,
    /// Before/Through terminal Z is NaN or infinite.
    NonFiniteTerminal,
}
impl fmt::Display for ScheduleSpecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NonFiniteFirstPlane => "schedule first plane Z must be finite",
            Self::InvalidHeight => "schedule nominal layer height must be finite and positive",
            Self::NonFiniteTerminal => "schedule terminal Z must be finite",
        })
    }
}
impl Error for ScheduleSpecError {}

/// Caller-selected output caps, not a physical sampling policy or total-process memory budget.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScheduleLimits {
    /// Maximum plane count, with u64 semantics on every host.
    pub max_layers: u64,
    /// Requested plane payload only; excludes allocator overhead, input and fixed stack scratch.
    pub max_payload_bytes: usize,
}

/// One published finite world-Z; machine and exposure metadata belong to M5.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayerPlane {
    /// Contiguous zero-based layer identity, independent of host usize.
    pub index: u64,
    /// Once-rounded exact first+index*height in mm, canonical +0 at exact zero.
    pub z_mm: f64,
    /// Unchanged nominal height, not a difference between rounded plane coordinates.
    pub nominal_height_mm: f64,
}

/// One printed slab of a slab schedule, in millimetres: the layer cures `[bottom, top]`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Slab {
    /// Slab bottom (the previous slab's top; zero for the first).
    pub bottom_mm: f64,
    /// Slab top: the layer's printed position.
    pub top_mm: f64,
    /// Slab height.
    pub height_mm: f64,
}

/// Complete immutable schedule, published only after every plane passes range/resolution checks.
#[derive(Debug, PartialEq)]
pub struct LayerSchedule {
    spec: Option<UniformScheduleSpec>,
    planes: Vec<LayerPlane>,
    slabs: Option<Vec<Slab>>,
}
impl LayerSchedule {
    /// Validated explicit uniform configuration, or `None` for a slab schedule.
    #[must_use]
    pub const fn spec(&self) -> Option<UniformScheduleSpec> {
        self.spec
    }
    /// The printed slabs of a slab schedule ([`build_slab_schedule`]), one per plane, or `None`
    /// for a uniform plane schedule.
    #[must_use]
    pub fn slabs(&self) -> Option<&[Slab]> {
        self.slabs.as_deref()
    }
    /// All planes in increasing index and strictly increasing represented Z order.
    #[must_use]
    pub fn planes(&self) -> &[LayerPlane] {
        &self.planes
    }
}

/// Atomic schedule failure; no successfully truncated or partial schedule is returned.
#[derive(Debug, PartialEq)]
#[non_exhaustive]
pub enum ScheduleError {
    /// Cooperative cancellation retaining its observation checkpoint.
    Cancelled(Cancelled),
    /// A terminal rule requires more than `u64::MAX` planes.
    IndexRange,
    /// Mathematical count exceeds the caller's count cap.
    LayerLimit {
        /// Configured maximum number of planes.
        limit: u64,
    },
    /// Count, byte capacity or host usize/isize range is not representable.
    SizeOverflow,
    /// Requested plane payload exceeds the caller's byte cap.
    PayloadLimit {
        /// Configured requested-payload byte cap.
        limit: usize,
    },
    /// Fallible plane-buffer reservation failed.
    Allocation,
    /// Exact plane rounds outside finite binary64 range.
    PlaneOutOfRange {
        /// First unpublishable index.
        index: u64,
    },
    /// Increasing exact planes cannot be published as strictly increasing binary64 values.
    ResolutionLoss {
        /// First plane whose rounded Z does not exceed the previous one.
        index: u64,
        /// Previously accepted rounded world-Z in mm.
        previous_z_mm: f64,
        /// Rejected rounded world-Z in mm.
        z_mm: f64,
    },
    /// Defensive fixed-storage overflow, unreachable in the proved valid input domain.
    ArithmeticOverflow,
}
impl fmt::Display for ScheduleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled(cause) => cause.fmt(f),
            Self::IndexRange => f.write_str("schedule requires more than u64::MAX planes"),
            Self::LayerLimit { limit } => write!(f, "schedule layer limit exceeded ({limit})"),
            Self::SizeOverflow => f.write_str("schedule count or host storage capacity overflow"),
            Self::PayloadLimit { limit } => {
                write!(
                    f,
                    "schedule requested payload limit exceeded ({limit} bytes)"
                )
            }
            Self::Allocation => f.write_str("unable to reserve layer planes"),
            Self::PlaneOutOfRange { index } => {
                write!(f, "schedule plane {index} rounds outside finite binary64 Z")
            }
            Self::ResolutionLoss {
                index,
                previous_z_mm,
                z_mm,
            } => write!(
                f,
                "schedule plane {index} Z {z_mm} mm does not exceed previous Z {previous_z_mm} mm"
            ),
            Self::ArithmeticOverflow => f.write_str("schedule fixed exact arithmetic overflow"),
        }
    }
}
impl Error for ScheduleError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Cancelled(cause) => Some(cause),
            _ => None,
        }
    }
}
impl From<Cancelled> for ScheduleError {
    fn from(cause: Cancelled) -> Self {
        Self::Cancelled(cause)
    }
}
type Result<T> = std::result::Result<T, ScheduleError>;

/// Build a complete uniform layer plan without geometry processing or implicit defaults.
/// # Errors
/// Returns typed count, capacity, allocation, range or rounding-resolution failure.
pub fn build_layer_schedule(
    spec: UniformScheduleSpec,
    limits: ScheduleLimits,
) -> Result<LayerSchedule> {
    build_layer_schedule_with_cancellation(spec, limits, &mut Cancellation::never())
}

/// Build atomically with TD-014 cooperative checkpoints, including empty-plan publication.
///
/// Exact terminal-count search (at most 65 probes including the domain check) precedes the layer
/// cap, checked host storage capacity and payload cap. One fallible reservation precedes rounding.
/// Plane polling occurs at 0,256,...; fixed arithmetic, allocation and destruction are indivisible.
/// Cost O(34*(64+N)), O(N) output plus fixed scratch; no progress or latency guarantee is implied.
/// # Errors
/// No partial successful plan escapes; observed cancellation retains its checkpoint and cause.
pub fn build_layer_schedule_with_cancellation(
    spec: UniformScheduleSpec,
    limits: ScheduleLimits,
    cancel: &mut Cancellation<'_>,
) -> Result<LayerSchedule> {
    cancel.check(Checkpoint::ScheduleStart)?;
    let first = Exact::decode(spec.first_plane_z_mm);
    let height = Exact::decode(spec.layer_height_mm);
    let count = plane_count(spec.end, &first, &height, cancel)?;
    if count > limits.max_layers {
        return Err(ScheduleError::LayerLimit {
            limit: limits.max_layers,
        });
    }
    let capacity = usize::try_from(count).map_err(|_| ScheduleError::SizeOverflow)?;
    let bytes = capacity
        .checked_mul(size_of::<LayerPlane>())
        .ok_or(ScheduleError::SizeOverflow)?;
    isize::try_from(bytes).map_err(|_| ScheduleError::SizeOverflow)?;
    if bytes > limits.max_payload_bytes {
        return Err(ScheduleError::PayloadLimit {
            limit: limits.max_payload_bytes,
        });
    }
    let mut planes: Vec<LayerPlane> = Vec::new();
    if capacity > 0 {
        cancel.check(Checkpoint::ScheduleStorage)?;
        planes
            .try_reserve_exact(capacity)
            .map_err(|_| ScheduleError::Allocation)?;
        cancel.check(Checkpoint::ScheduleStorage)?;
    }
    let mut exact = first;
    for offset in 0..capacity {
        cancel.poll(Checkpoint::SchedulePlanes, offset)?;
        // The preflight count is u64 and capacity is its checked usize representation.
        let index = u64::try_from(offset).map_err(|_| ScheduleError::SizeOverflow)?;
        let z_mm = exact
            .round()
            .ok_or(ScheduleError::PlaneOutOfRange { index })?;
        if let Some(previous) = planes.last() {
            if z_mm <= previous.z_mm {
                return Err(ScheduleError::ResolutionLoss {
                    index,
                    previous_z_mm: previous.z_mm,
                    z_mm,
                });
            }
        }
        planes.push(LayerPlane {
            index,
            z_mm,
            nominal_height_mm: spec.layer_height_mm,
        });
        if offset + 1 < capacity {
            exact.add(&height)?;
        }
    }
    cancel.check(Checkpoint::ScheduleComplete)?;
    Ok(LayerSchedule {
        spec: Some(spec),
        planes,
        slabs: None,
    })
}

/// Nanometres per millimetre.
const NM_PER_MM: f64 = 1e6;
/// Slab positions must stay within this many nanometres (one metre), far inside exact f64 integers.
const MAX_SLAB_TOP_NM: u64 = 1_000_000_000;

/// Build a schedule of stacked slabs from Z = 0, one layer per height in `heights_nm`
/// (nanometres, each positive), sampling each slab at its centre.
///
/// Positions are exact integer sums in nanometres, converted to millimetres with one correctly
/// rounded division each, so they never accumulate floating-point error and do not depend on the
/// platform.
/// # Errors
/// A zero height, a stack above one metre, the caller's layer and payload caps,
/// allocation failure and cancellation.
pub fn build_slab_schedule(
    heights_nm: &[u64],
    limits: ScheduleLimits,
    cancel: &mut Cancellation<'_>,
) -> Result<LayerSchedule> {
    cancel.check(Checkpoint::ScheduleStart)?;
    let count = u64::try_from(heights_nm.len()).map_err(|_| ScheduleError::SizeOverflow)?;
    if count > limits.max_layers {
        return Err(ScheduleError::LayerLimit {
            limit: limits.max_layers,
        });
    }
    let bytes = heights_nm
        .len()
        .checked_mul(size_of::<LayerPlane>() + size_of::<Slab>())
        .ok_or(ScheduleError::SizeOverflow)?;
    if bytes > limits.max_payload_bytes {
        return Err(ScheduleError::PayloadLimit {
            limit: limits.max_payload_bytes,
        });
    }
    let mut planes = Vec::new();
    let mut slabs = Vec::new();
    planes
        .try_reserve_exact(heights_nm.len())
        .map_err(|_| ScheduleError::Allocation)?;
    slabs
        .try_reserve_exact(heights_nm.len())
        .map_err(|_| ScheduleError::Allocation)?;
    let mut bottom = 0u64;
    for (offset, &height) in heights_nm.iter().enumerate() {
        cancel.poll(Checkpoint::SchedulePlanes, offset)?;
        let index = u64::try_from(offset).map_err(|_| ScheduleError::SizeOverflow)?;
        let top = bottom
            .checked_add(height)
            .filter(|&top| height > 0 && top <= MAX_SLAB_TOP_NM)
            .ok_or(ScheduleError::PlaneOutOfRange { index })?;
        #[allow(clippy::cast_precision_loss)] // Exact: integers below 2^31.
        let (bottom_mm, top_mm, height_mm, centre_mm) = (
            bottom as f64 / NM_PER_MM,
            top as f64 / NM_PER_MM,
            height as f64 / NM_PER_MM,
            (bottom + top) as f64 / (2. * NM_PER_MM),
        );
        planes.push(LayerPlane {
            index,
            z_mm: centre_mm,
            nominal_height_mm: height_mm,
        });
        slabs.push(Slab {
            bottom_mm,
            top_mm,
            height_mm,
        });
        bottom = top;
    }
    cancel.check(Checkpoint::ScheduleComplete)?;
    Ok(LayerSchedule {
        spec: None,
        planes,
        slabs: Some(slabs),
    })
}

fn plane_count(
    end: ScheduleEnd,
    first: &Exact,
    height: &Exact,
    cancel: &mut Cancellation<'_>,
) -> Result<u64> {
    let (terminal, inclusive) = match end {
        ScheduleEnd::Count(n) => return Ok(n),
        ScheduleEnd::Before(z) => (Exact::decode(z), false),
        ScheduleEnd::Through(z) => (Exact::decode(z), true),
    };
    let mut includes = |index| -> Result<bool> {
        cancel.check(Checkpoint::ScheduleCount)?;
        let mut plane = height.multiply(index)?;
        plane.add(first)?;
        let order = plane.compare(&terminal);
        Ok(order.is_lt() || (inclusive && order.is_eq()))
    };
    if includes(u64::MAX)? {
        return Err(ScheduleError::IndexRange);
    }
    let (mut low, mut high) = (0, u64::MAX);
    while low < high {
        let mid = low + (high - low) / 2;
        if includes(mid)? {
            low = mid + 1;
        } else {
            high = mid;
        }
    }
    Ok(low)
}

// In eta=2^-1074 units, |finite f64|<2^2098, |a+u64*h|<2^2163.
// 34*64=2176 magnitude bits cover all probes and recurrence intermediates.
const WORDS: usize = 34;
#[derive(Clone)]
struct Exact {
    words: [u64; WORDS],
    negative: bool,
}
impl Exact {
    fn zero() -> Self {
        Self {
            words: [0; WORDS],
            negative: false,
        }
    }

    fn decode(value: f64) -> Self {
        // All callers receive finite validated inputs; binary64 exponent fits usize on supported hosts.
        let bits = value.to_bits();
        let exponent = usize::try_from((bits >> 52) & 2047).unwrap();
        debug_assert_ne!(exponent, 2047);
        let fraction = bits & ((1_u64 << 52) - 1);
        let significand = if exponent == 0 {
            fraction
        } else {
            fraction | (1_u64 << 52)
        };
        let shift = exponent.saturating_sub(1);
        let mut exact = Self::zero();
        let aligned = u128::from(significand) << (shift % 64);
        exact.words[shift / 64] = low_word(aligned);
        exact.words[shift / 64 + 1] = u64::try_from(aligned >> 64).unwrap();
        exact.negative = bits >> 63 != 0 && significand != 0;
        exact
    }

    fn multiply(&self, factor: u64) -> Result<Self> {
        let mut result = Self::zero();
        let mut carry = 0_u128;
        for (out, &word) in result.words.iter_mut().zip(&self.words) {
            // (2^64-1)^2 + (2^64-1) < 2^128.
            let product = u128::from(word) * u128::from(factor) + carry;
            *out = low_word(product);
            carry = product >> 64;
        }
        if carry != 0 {
            return Err(ScheduleError::ArithmeticOverflow);
        }
        result.negative = self.negative && factor != 0;
        Ok(result)
    }

    fn magnitude_order(&self, other: &Self) -> Ordering {
        self.words.iter().rev().cmp(other.words.iter().rev())
    }

    fn compare(&self, other: &Self) -> Ordering {
        match (self.negative, other.negative) {
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            (false, false) => self.magnitude_order(other),
            (true, true) => self.magnitude_order(other).reverse(),
        }
    }

    fn add(&mut self, other: &Self) -> Result<()> {
        if self.negative == other.negative {
            let mut carry = 0_u128;
            for (left, &right) in self.words.iter_mut().zip(&other.words) {
                let sum = u128::from(*left) + u128::from(right) + carry;
                *left = low_word(sum);
                carry = sum >> 64;
            }
            if carry != 0 {
                return Err(ScheduleError::ArithmeticOverflow);
            }
        } else {
            let reverse = self.magnitude_order(other).is_lt();
            let mut borrow = false;
            for (left, &right) in self.words.iter_mut().zip(&other.words) {
                let (a, b) = if reverse {
                    (right, *left)
                } else {
                    (*left, right)
                };
                let (difference, first) = a.overflowing_sub(b);
                let (difference, second) = difference.overflowing_sub(u64::from(borrow));
                *left = difference;
                borrow = first || second;
            }
            if borrow {
                return Err(ScheduleError::ArithmeticOverflow);
            }
            if reverse {
                self.negative = other.negative;
            }
        }
        if self.words.iter().all(|&word| word == 0) {
            self.negative = false;
        }
        Ok(())
    }

    fn bit(&self, index: usize) -> bool {
        self.words
            .get(index / 64)
            .is_some_and(|word| word & (1_u64 << (index % 64)) != 0)
    }

    fn round(&self) -> Option<f64> {
        let length = self
            .words
            .iter()
            .rposition(|&word| word != 0)
            .map_or(0, |i| {
                i * 64 + 64 - usize::try_from(self.words[i].leading_zeros()).unwrap()
            });
        let mut shift = length.saturating_sub(53);
        let mut significand = 0_u64;
        for bit in shift..length {
            significand |= u64::from(self.bit(bit)) << (bit - shift);
        }
        if shift > 0 && self.bit(shift - 1) {
            let sticky = self.words[..(shift - 1) / 64].iter().any(|&word| word != 0)
                || ((shift - 1) % 64 != 0
                    && self.words[(shift - 1) / 64] & ((1_u64 << ((shift - 1) % 64)) - 1) != 0);
            if sticky || significand & 1 != 0 {
                significand += 1;
            }
        }
        if significand == 1_u64 << 53 {
            significand >>= 1;
            shift += 1;
        }
        let magnitude = if significand < 1_u64 << 52 {
            significand
        } else {
            if shift >= 2046 {
                return None;
            }
            (u64::try_from(shift + 1).unwrap() << 52) | (significand - (1_u64 << 52))
        };
        Some(f64::from_bits(magnitude | (u64::from(self.negative) << 63)))
    }
}

fn low_word(value: u128) -> u64 {
    u64::try_from(value & u128::from(u64::MAX)).unwrap()
}

#[cfg(test)]
mod tests;
