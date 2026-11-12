//! Lossless common dyadic encoding; no physical grid or floating multiplication is used.
use super::{
    cap, compose_bounded, invalid, Cancellation, Checkpoint, Composition, ConstructError,
    ConstructInputIssue, ConstructLimits, ConstructResource, ConstructedMaterial, MaterialRing,
    Result,
};

/// One simple closed ring in represented binary64 world-XY millimeters.
#[derive(Clone, Copy, Debug)]
pub struct RepresentedMaterialRing<'a> {
    /// Finite coordinates; closure is implicit.
    pub vertices: &'a [[f64; 2]],
    /// Explicit source ID; parity within a source and API-selected composition between sources.
    pub material: u64,
    /// Unique source-path identity retained in every supporting edge.
    pub source_path: usize,
}

// Decompose the represented value into signed odd significand times a power of two.
// Zero has no scale requirement. Subnormal values need no floating arithmetic.
fn decode(value: f64) -> Option<(bool, u64, i16)> {
    if !value.is_finite() {
        return None;
    }
    let bits = value.to_bits();
    let encoded = u16::try_from((bits >> 52) & 0x7ff).ok()?;
    let mut significand = bits & ((1_u64 << 52) - 1);
    let mut exponent = if encoded == 0 {
        -1074
    } else {
        significand |= 1_u64 << 52;
        i16::try_from(encoded).ok()? - 1075
    };
    if significand != 0 {
        let zeros = significand.trailing_zeros();
        significand >>= zeros;
        exponent += i16::try_from(zeros).ok()?;
    }
    Some((bits >> 63 != 0, significand, exponent))
}

fn encode(value: f64, exponent: i16) -> Option<i64> {
    let (negative, significand, own_exponent) = decode(value)?;
    if significand == 0 {
        return Some(0);
    }
    let shift = u32::try_from(i32::from(own_exponent) - i32::from(exponent)).ok()?;
    let magnitude = significand.checked_shl(shift)?;
    // checked_shl checks shift width, not discarded high bits.
    if (magnitude >> shift) != significand || magnitude > 1_u64 << 60 {
        return None;
    }
    let integer = i64::try_from(magnitude).ok()?;
    Some(if negative { -integer } else { integer })
}

fn tick(work: &mut usize, limit: usize, cancel: &mut Cancellation<'_>) -> Result<()> {
    cancel.check(Checkpoint::ConstructInput)?;
    *work = work.checked_add(1).ok_or(ConstructError::SizeOverflow)?;
    cap(*work, limit, ConstructResource::Work)
}

/// Independently construct material from finite binary64 millimeter rings without rounding.
///
/// The exact common scale is derived from the least significant nonzero binary digit of every
/// coordinate. All encoded magnitudes must fit +/-2^60; wider dynamic ranges fail explicitly.
/// Result coordinates retain this scale through `ConstructedMaterial::dyadic_exponent_mm`.
/// # Errors
/// Returns the cancellable variant's structured validation, range, resource and topology errors.
pub fn union_represented_material_rings(
    rings: &[RepresentedMaterialRing<'_>],
    limits: ConstructLimits,
) -> Result<ConstructedMaterial> {
    union_represented_material_rings_with_cancellation(rings, limits, &mut Cancellation::never())
}

/// Cancellable, resource-bounded lossless encoding followed by the exact arrangement constructor.
///
/// Encoding scratch is charged before reservation and subtracted from the constructor's remaining
/// budget. Cancellation is checked for each ring and vertex; no partial result is published.
/// # Errors
/// Rejects nonfinite coordinates, unsupported exact dyadic ranges, invalid rings, resource limits,
/// allocation/capacity failures, cancellation and constructor failures. Never snaps or repairs input.
pub fn union_represented_material_rings_with_cancellation(
    rings: &[RepresentedMaterialRing<'_>],
    limits: ConstructLimits,
    cancel: &mut Cancellation<'_>,
) -> Result<ConstructedMaterial> {
    compose_represented(rings, limits, cancel, Composition::Union)
}

/// Losslessly subtract nonzero cutter sources from body source zero in represented millimeters.
///
/// Each source uses even-odd parity; cutters are unioned across distinct nonzero IDs.
/// An absent body yields empty material. No snapping, tolerance or coordinate rounding is used.
/// The exact encoding range and retained physical scale match [`union_represented_material_rings`].
/// # Errors
/// Returns the cancellable variant's structured input, range, budget and topology failures.
pub fn difference_represented_material_rings(
    rings: &[RepresentedMaterialRing<'_>],
    limits: ConstructLimits,
) -> Result<ConstructedMaterial> {
    difference_represented_material_rings_with_cancellation(
        rings,
        limits,
        &mut Cancellation::never(),
    )
}

/// Cancellable represented-coordinate body-minus-unioned-cutters composition.
///
/// Encoding scratch and work share the constructor's checked aggregate budgets.
/// # Errors
/// Rejects invalid input, unsupported dyadic ranges, resource exhaustion, arithmetic or topology
/// failures, and cancellation. No successful partial material is returned.
pub fn difference_represented_material_rings_with_cancellation(
    rings: &[RepresentedMaterialRing<'_>],
    limits: ConstructLimits,
    cancel: &mut Cancellation<'_>,
) -> Result<ConstructedMaterial> {
    compose_represented(rings, limits, cancel, Composition::Difference)
}

/// Lossless represented-coordinate prepared composition: body minus cutters, union additives.
///
/// IDs and parity follow [`super::prepared_material_rings`]. In particular, additive components
/// survive cutters and absent body. The exact dyadic range and retained scale match
/// [`union_represented_material_rings`]; no snapping or implicit source ownership is introduced.
/// # Errors
/// Returns the cancellable variant's structured input, range, budget and topology failures.
pub fn prepared_represented_material_rings(
    rings: &[RepresentedMaterialRing<'_>],
    limits: ConstructLimits,
) -> Result<ConstructedMaterial> {
    prepared_represented_material_rings_with_cancellation(rings, limits, &mut Cancellation::never())
}

/// Cancellable lossless prepared-material composition with aggregate encoding/construction budgets.
/// # Errors
/// Rejects invalid input, unsupported dyadic ranges, resource exhaustion, arithmetic or topology
/// failures and cancellation. No successful partial material is returned.
pub fn prepared_represented_material_rings_with_cancellation(
    rings: &[RepresentedMaterialRing<'_>],
    limits: ConstructLimits,
    cancel: &mut Cancellation<'_>,
) -> Result<ConstructedMaterial> {
    compose_represented(rings, limits, cancel, Composition::Prepared)
}

fn compose_represented(
    rings: &[RepresentedMaterialRing<'_>],
    limits: ConstructLimits,
    cancel: &mut Cancellation<'_>,
    composition: Composition,
) -> Result<ConstructedMaterial> {
    cancel.check(Checkpoint::ConstructStart)?;
    let mut work = 0;
    let mut count = 0usize;
    let mut exponent = None;
    for ring in rings {
        tick(&mut work, limits.max_work, cancel)?;
        if ring.vertices.len() < 3 {
            return Err(invalid(
                ring.source_path,
                ConstructInputIssue::TooFewVertices,
            ));
        }
        count = count
            .checked_add(ring.vertices.len())
            .ok_or(ConstructError::SizeOverflow)?;
        cap(count, limits.max_input_edges, ConstructResource::InputEdges)?;
        for xy in ring.vertices {
            tick(&mut work, limits.max_work, cancel)?;
            for &value in xy {
                let (_, significand, own) = decode(value).ok_or_else(|| {
                    invalid(ring.source_path, ConstructInputIssue::NonFiniteCoordinate)
                })?;
                if significand != 0 {
                    exponent = Some(exponent.map_or(own, |old: i16| old.min(own)));
                }
            }
        }
    }
    let exponent = exponent.unwrap_or(0);
    let scratch = count
        .checked_mul(size_of::<[i64; 2]>())
        .and_then(|n| {
            rings
                .len()
                .checked_mul(size_of::<MaterialRing<'_>>())
                .and_then(|m| n.checked_add(m))
        })
        .ok_or(ConstructError::SizeOverflow)?;
    isize::try_from(scratch).map_err(|_| ConstructError::SizeOverflow)?;
    cap(
        scratch,
        limits.max_payload_bytes,
        ConstructResource::PayloadBytes,
    )?;
    cancel.check(Checkpoint::ConstructStorage)?;
    let mut vertices = Vec::new();
    vertices
        .try_reserve_exact(count)
        .map_err(|_| ConstructError::Allocation)?;
    let mut encoded = Vec::new();
    encoded
        .try_reserve_exact(rings.len())
        .map_err(|_| ConstructError::Allocation)?;
    cancel.check(Checkpoint::ConstructStorage)?;
    for ring in rings {
        for xy in ring.vertices {
            tick(&mut work, limits.max_work, cancel)?;
            let x = encode(xy[0], exponent)
                .ok_or_else(|| invalid(ring.source_path, ConstructInputIssue::DyadicRange))?;
            let y = encode(xy[1], exponent)
                .ok_or_else(|| invalid(ring.source_path, ConstructInputIssue::DyadicRange))?;
            vertices.push([x, y]);
        }
    }
    let mut start = 0;
    for ring in rings {
        tick(&mut work, limits.max_work, cancel)?;
        let end = start + ring.vertices.len();
        encoded.push(MaterialRing {
            vertices: &vertices[start..end],
            material: ring.material,
            source_path: ring.source_path,
        });
        start = end;
    }
    let mut remaining = limits;
    remaining.max_payload_bytes -= scratch;
    remaining.max_work -= work;
    let mut result = compose_bounded(&encoded, remaining, cancel, 1_i64 << 60, composition)?;
    result.dyadic_exponent_mm = Some(exponent);
    result.summary.work = result
        .summary
        .work
        .checked_add(work)
        .ok_or(ConstructError::SizeOverflow)?;
    result.summary.requested_payload_bytes = result
        .summary
        .requested_payload_bytes
        .checked_add(scratch)
        .ok_or(ConstructError::SizeOverflow)?;
    cancel.check(Checkpoint::ConstructComplete)?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bit_encoding_handles_sign_zero_subnormals_and_range_without_rounding() {
        assert_eq!(encode(-0.0, -1074), Some(0));
        assert_eq!(encode(-f64::from_bits(1), -1074), Some(-1));
        assert_eq!(encode(f64::from_bits(3), -1074), Some(3));
        assert_eq!(encode(0.1, -55), Some(3_602_879_701_896_397));
        assert_eq!(encode(1.0, -60), Some(1_i64 << 60));
        assert_eq!(encode(1.0, -61), None);
        assert_eq!(encode(1.0, -100), None);
        assert_eq!(decode(f64::INFINITY), None);
        assert_eq!(decode(f64::NAN), None);
    }
}
