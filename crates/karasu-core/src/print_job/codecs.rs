//! Independently implemented public machine raster encodings.
// Counts are bounded to 28 bits (Photon 7) before casts; byte casts deliberately select wire octets.
#![allow(clippy::cast_possible_truncation)]
use super::machine::CodecError;
use crate::{
    cancellation::{Cancellation, Checkpoint},
    raster::mask::SpanMask,
};

fn reserve(out: &mut Vec<u8>, additional: usize, maximum: usize) -> Result<(), CodecError> {
    if out
        .len()
        .checked_add(additional)
        .is_none_or(|n| n > maximum)
    {
        return Err(CodecError::Limit("encoded layer byte budget"));
    }
    out.try_reserve_exact(if out.capacity() - out.len() < additional {
        additional.max(out.capacity().min(maximum - out.len() - additional))
    } else {
        0
    })
    .map_err(|_| CodecError::Limit("encoded layer allocation"))
}

/// Longest GOO or CTB run: 28-bit counts.
const MAX_RUN: usize = 0x0fff_ffff;

/// Feed the mask's maximal row-major runs of `key(value)` to `emit`, in order.
///
/// Runs of the mask with equal keys are coalesced, so `emit` sees exactly the groups a pixel scan
/// would form. Cancellation is polled per mask run: the work is proportional to the runs.
fn keyed_runs<K: Copy + Eq>(
    mask: &SpanMask,
    cancellation: &mut Cancellation<'_>,
    mut key: impl FnMut(u8) -> Result<K, CodecError>,
    mut emit: impl FnMut(K, usize) -> Result<(), CodecError>,
) -> Result<(), CodecError> {
    let mut pending: Option<(K, usize)> = None;
    let mut index = 0usize;
    mask.for_each_run(|value, length| {
        cancellation.poll(Checkpoint::ExportLayer, index)?;
        index += 1;
        let k = key(value)?;
        match &mut pending {
            Some((current, total)) if *current == k => *total += length,
            _ => {
                if let Some((current, total)) = pending.take() {
                    emit(current, total)?;
                }
                pending = Some((k, length));
            }
        }
        Ok::<(), CodecError>(())
    })?;
    if let Some((current, total)) = pending {
        emit(current, total)?;
    }
    Ok(())
}

/// Encode GOO V3 absolute chunks without changing any 8-bit sample.
/// The mask's row-major order is the wire row order.
pub(crate) fn goo_rle(
    mask: &SpanMask,
    cancellation: &mut Cancellation<'_>,
    max_bytes: usize,
) -> Result<Vec<u8>, CodecError> {
    let mut output = Vec::new();
    reserve(&mut output, 2, max_bytes)?;
    output.push(0x55);
    keyed_runs(mask, cancellation, Ok, |value, mut remaining| {
        while remaining > 0 {
            let count = remaining.min(MAX_RUN);
            remaining -= count;
            goo_chunk(&mut output, value, count as u32, max_bytes)?;
        }
        Ok(())
    })?;
    let mut sum = 0u8;
    for (i, &byte) in output[1..].iter().enumerate() {
        cancellation.poll(Checkpoint::ExportLayer, i)?;
        sum = sum.wrapping_add(byte);
    }
    let checksum = !sum;
    output.push(checksum);
    Ok(output)
}

/// One GOO chunk of `count` (at most [`MAX_RUN`]) pixels of `value`.
fn goo_chunk(
    output: &mut Vec<u8>,
    value: u8,
    count: u32,
    max_bytes: usize,
) -> Result<(), CodecError> {
    let extra = match count {
        0..=15 => 0,
        16..=4095 => 1,
        4096..=0x000f_ffff => 2,
        _ => 3,
    };
    let kind = match value {
        0 => 0,
        255 => 3,
        _ => 1,
    };
    reserve(
        output,
        1 + usize::from(extra) + usize::from(kind == 1) + 1,
        max_bytes,
    )?;
    output.push((kind << 6) | (extra << 4) | (count & 15) as u8);
    if kind == 1 {
        output.push(value);
    }
    for n in (0..extra).rev() {
        output.push((count >> (4 + 8 * u32::from(n))) as u8);
    }
    Ok(())
}

/// Encode only exactly representable CTB seven-bit values of `map(value)`.
pub(crate) fn ctb_rle(
    mask: &SpanMask,
    map: impl Fn(u8) -> u8,
    quantize: bool,
    cancellation: &mut Cancellation<'_>,
    max_bytes: usize,
) -> Result<Vec<u8>, CodecError> {
    let mut output = Vec::new();
    let key = |value: u8| {
        let value = map(value);
        if !quantize && value != 0 && (value < 3 || value & 1 == 0) {
            return Err(CodecError::Unsupported(
                "CTB seven-bit grayscale cannot preserve this sample; explicitly select quantization",
            ));
        }
        Ok(value >> 1)
    };
    keyed_runs(mask, cancellation, key, |code, mut remaining| {
        while remaining > 0 {
            let count = remaining.min(MAX_RUN);
            remaining -= count;
            ctb_chunk(&mut output, code, count as u32, max_bytes)?;
        }
        Ok(())
    })?;
    Ok(output)
}

/// One CTB chunk of `count` (at most [`MAX_RUN`]) pixels of seven-bit `code`.
fn ctb_chunk(
    output: &mut Vec<u8>,
    code: u8,
    count: u32,
    max_bytes: usize,
) -> Result<(), CodecError> {
    let extra = match count {
        0..=1 => 0,
        2..=0x7f => 1,
        0x80..=0x3fff => 2,
        0x4000..=0x1f_ffff => 3,
        _ => 4,
    };
    reserve(output, 1 + extra, max_bytes)?;
    output.push(code | if count > 1 { 0x80 } else { 0 });
    if count > 1 {
        match count {
            0..=0x7f => output.push(count as u8),
            0x80..=0x3fff => {
                output.push(0x80 | (count >> 8) as u8);
                output.push(count as u8);
            }
            0x4000..=0x1f_ffff => {
                output.push(0xc0 | (count >> 16) as u8);
                output.push((count >> 8) as u8);
                output.push(count as u8);
            }
            _ => {
                output.push(0xe0 | (count >> 24) as u8);
                output.push((count >> 16) as u8);
                output.push((count >> 8) as u8);
                output.push(count as u8);
            }
        }
    }
    Ok(())
}

/// Encode legacy Photon V1 binary masks of `map(value)`; fractional coverage is rejected.
pub(crate) fn photon_rle(
    mask: &SpanMask,
    map: impl Fn(u8) -> u8,
    cancellation: &mut Cancellation<'_>,
    max_bytes: usize,
) -> Result<Vec<u8>, CodecError> {
    let mut output = Vec::new();
    let key = |value: u8| {
        let value = map(value);
        if value != 0 && value != 255 {
            return Err(CodecError::Unsupported("Photon V1 requires binary pixels"));
        }
        Ok(value)
    };
    keyed_runs(mask, cancellation, key, |value, mut remaining| {
        while remaining > 0 {
            let count = remaining.min(125);
            remaining -= count;
            reserve(&mut output, 1, max_bytes)?;
            output.push(count as u8 | if value == 255 { 0x80 } else { 0 });
        }
        Ok(())
    })?;
    Ok(output)
}

/// The previous dense pixel-scan encoders (before TD-055), kept as test oracles.
#[cfg(test)]
pub(crate) mod dense {
    use super::{reserve, CodecError};
    use crate::cancellation::{Cancellation, Checkpoint};

    /// Encode GOO V3 absolute chunks without changing any 8-bit sample.
    /// The caller supplies pixels in the explicitly selected wire row order.
    pub(super) fn goo_rle(
        pixels: &[u8],
        cancellation: &mut Cancellation<'_>,
        max_bytes: usize,
    ) -> Result<Vec<u8>, CodecError> {
        let mut output = Vec::new();
        reserve(&mut output, 2, max_bytes)?;
        output.push(0x55);
        let mut at = 0;
        while at < pixels.len() {
            cancellation.poll(Checkpoint::ExportLayer, at)?;
            let value = pixels[at];
            let mut end = at + 1;
            while end < pixels.len() && pixels[end] == value && end - at < 0x0fff_ffff {
                cancellation.poll(Checkpoint::ExportLayer, end)?;
                end += 1;
            }
            let count = (end - at) as u32;
            let extra = match count {
                0..=15 => 0,
                16..=4095 => 1,
                4096..=0x000f_ffff => 2,
                _ => 3,
            };
            let kind = match value {
                0 => 0,
                255 => 3,
                _ => 1,
            };
            reserve(
                &mut output,
                1 + usize::from(extra) + usize::from(kind == 1) + 1,
                max_bytes,
            )?;
            output.push((kind << 6) | (extra << 4) | (count & 15) as u8);
            if kind == 1 {
                output.push(value);
            }
            for n in (0..extra).rev() {
                output.push((count >> (4 + 8 * u32::from(n))) as u8);
            }
            at = end;
        }
        let mut sum = 0u8;
        for (i, &byte) in output[1..].iter().enumerate() {
            cancellation.poll(Checkpoint::ExportLayer, i)?;
            sum = sum.wrapping_add(byte);
        }
        let checksum = !sum;
        output.push(checksum);
        Ok(output)
    }

    /// Encode only exactly representable CTB seven-bit values.
    pub(super) fn ctb_rle(
        pixels: &[u8],
        quantize: bool,
        cancellation: &mut Cancellation<'_>,
        max_bytes: usize,
    ) -> Result<Vec<u8>, CodecError> {
        let mut output = Vec::new();
        let mut at = 0;
        while at < pixels.len() {
            cancellation.poll(Checkpoint::ExportLayer, at)?;
            let value = pixels[at];
            if !quantize && value != 0 && (value < 3 || value & 1 == 0) {
                return Err(CodecError::Unsupported(
                    "CTB seven-bit grayscale cannot preserve this sample; explicitly select quantization",
                ));
            }
            let code = value >> 1;
            let mut end = at + 1;
            while end < pixels.len() && pixels[end] >> 1 == code && end - at < 0x0fff_ffff {
                if !quantize && pixels[end] != 0 && (pixels[end] < 3 || pixels[end] & 1 == 0) {
                    return Err(CodecError::Unsupported(
                        "CTB seven-bit grayscale cannot preserve this sample; explicitly select quantization",
                    ));
                }
                cancellation.poll(Checkpoint::ExportLayer, end)?;
                end += 1;
            }
            let count = (end - at) as u32;
            let extra = match count {
                0..=1 => 0,
                2..=0x7f => 1,
                0x80..=0x3fff => 2,
                0x4000..=0x1f_ffff => 3,
                _ => 4,
            };
            reserve(&mut output, 1 + extra, max_bytes)?;
            output.push(code | if count > 1 { 0x80 } else { 0 });
            if count > 1 {
                match count {
                    0..=0x7f => output.push(count as u8),
                    0x80..=0x3fff => {
                        output.push(0x80 | (count >> 8) as u8);
                        output.push(count as u8);
                    }
                    0x4000..=0x1f_ffff => {
                        output.push(0xc0 | (count >> 16) as u8);
                        output.push((count >> 8) as u8);
                        output.push(count as u8);
                    }
                    _ => {
                        output.push(0xe0 | (count >> 24) as u8);
                        output.push((count >> 16) as u8);
                        output.push((count >> 8) as u8);
                        output.push(count as u8);
                    }
                }
            }
            at = end;
        }
        Ok(output)
    }

    /// Encode legacy Photon V1 binary masks; fractional coverage is rejected.
    pub(super) fn photon_rle(
        pixels: &[u8],
        cancellation: &mut Cancellation<'_>,
        max_bytes: usize,
    ) -> Result<Vec<u8>, CodecError> {
        let mut output = Vec::new();
        let mut at = 0;
        while at < pixels.len() {
            cancellation.poll(Checkpoint::ExportLayer, at)?;
            let value = pixels[at];
            if value != 0 && value != 255 {
                return Err(CodecError::Unsupported("Photon V1 requires binary pixels"));
            }
            let mut end = at + 1;
            while end < pixels.len() && pixels[end] == value && end - at < 125 {
                cancellation.poll(Checkpoint::ExportLayer, end)?;
                end += 1;
            }
            reserve(&mut output, 1, max_bytes)?;
            output.push((end - at) as u8 | if value == 255 { 0x80 } else { 0 });
            at = end;
        }
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn row(p: &[u8]) -> SpanMask {
        SpanMask::from_pixels(u32::try_from(p.len()).unwrap(), 1, p).unwrap()
    }
    fn goo_rle(p: &[u8]) -> Vec<u8> {
        super::goo_rle(&row(p), &mut Cancellation::never(), usize::MAX).unwrap()
    }
    fn ctb_rle(p: &[u8], q: bool) -> Result<Vec<u8>, CodecError> {
        super::ctb_rle(&row(p), |v| v, q, &mut Cancellation::never(), usize::MAX)
    }
    fn photon_rle(p: &[u8]) -> Result<Vec<u8>, CodecError> {
        super::photon_rle(&row(p), |v| v, &mut Cancellation::never(), usize::MAX)
    }

    #[test]
    fn goo_absolute_wire_vectors_and_checksum() {
        assert_eq!(goo_rle(&[0, 0, 255, 7]), [0x55, 2, 0xc1, 0x41, 7, 0xf4]);
        // A 16-pixel count stores the low nibble before the high count byte.
        assert_eq!(goo_rle(&[128; 16]), [0x55, 0x50, 128, 1, 0x2e]);
        let mut expected = vec![0x55, 0x20, 1, 0];
        expected.push(!0x21);
        assert_eq!(goo_rle(&vec![0; 4096]), expected);
    }

    #[test]
    fn ctb_exactness_and_count_boundaries() {
        for bad in [1, 2, 128, 254] {
            assert!(ctb_rle(&[bad], false).is_err());
        }
        assert_eq!(ctb_rle(&[0, 3, 255], false).unwrap(), [0, 1, 127]);
        assert_eq!(ctb_rle(&[2], true).unwrap(), [1]);
        assert_eq!(ctb_rle(&[255; 128], false).unwrap(), [255, 0x80, 0x80]);
        assert_eq!(
            ctb_rle(&vec![255; 16384], false).unwrap(),
            [255, 0xc0, 0x40, 0]
        );
        assert!(matches!(
            ctb_rle(&[0, 1], false),
            Err(CodecError::Unsupported(_))
        ));
    }

    #[test]
    fn cancellation_between_runs_and_encoded_budget_are_enforced() {
        // Cancellation is polled every 256 mask runs: alternating pixels give 1100 runs.
        let pixels: Vec<u8> = (0..1100)
            .map(|i| if i % 2 == 0 { 0 } else { 255 })
            .collect();
        let mut polls = 0;
        let mut predicate = |_| {
            polls += 1;
            polls == 3
        };
        assert!(matches!(
            super::goo_rle(
                &row(&pixels),
                &mut Cancellation::new(&mut predicate),
                usize::MAX
            ),
            Err(CodecError::Cancelled(_))
        ));
        assert!(matches!(
            super::goo_rle(&row(&[1, 2, 3]), &mut Cancellation::never(), 4),
            Err(CodecError::Limit(_))
        ));
        assert!(matches!(
            super::ctb_rle(
                &row(&[0, 255, 0]),
                |v| v,
                false,
                &mut Cancellation::never(),
                2
            ),
            Err(CodecError::Limit(_))
        ));
        assert!(matches!(
            super::photon_rle(&row(&[0, 255, 0]), |v| v, &mut Cancellation::never(), 2),
            Err(CodecError::Limit(_))
        ));
    }

    #[test]
    fn photon_binary_and_compatibility_run_limit() {
        assert_eq!(photon_rle(&[255; 126]).unwrap(), [253, 129]);
        assert_eq!(photon_rle(&[0, 255, 0]).unwrap(), [1, 129, 1]);
        assert!(photon_rle(&[127]).is_err());
    }

    /// Masks of several rows with long and single-pixel runs over a few levels.
    fn random_masks() -> impl Iterator<Item = (u32, u32, Vec<u8>)> {
        let mut state = 0x2545_f491_4f6c_dd1d_u64;
        let mut next = move |m: u64| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state % m
        };
        (0..300).map(move |case| {
            let (w, h) = (1 + next(70) as u32, 1 + next(9) as u32);
            let levels: &[u8] = match case % 3 {
                0 => &[0, 255],
                1 => &[0, 3, 127, 255],
                _ => &[0, 1, 2, 128, 254, 255],
            };
            let mut pixels = Vec::new();
            while pixels.len() < (w * h) as usize {
                let value = levels[next(levels.len() as u64) as usize];
                let run = if next(3) == 0 {
                    1 + next(200)
                } else {
                    1 + next(4)
                };
                pixels.extend(std::iter::repeat_n(value, run as usize));
            }
            pixels.truncate((w * h) as usize);
            (w, h, pixels)
        })
    }

    #[test]
    fn run_encoders_match_the_dense_oracles_byte_for_byte() {
        let never = || Cancellation::never();
        for (w, h, pixels) in random_masks() {
            let mask = SpanMask::from_pixels(w, h, &pixels).unwrap();
            assert_eq!(
                super::goo_rle(&mask, &mut never(), usize::MAX).unwrap(),
                dense::goo_rle(&pixels, &mut never(), usize::MAX).unwrap()
            );
            for quantize in [false, true] {
                assert_eq!(
                    super::ctb_rle(&mask, |v| v, quantize, &mut never(), usize::MAX).ok(),
                    dense::ctb_rle(&pixels, quantize, &mut never(), usize::MAX).ok()
                );
            }
            assert_eq!(
                super::photon_rle(&mask, |v| v, &mut never(), usize::MAX).ok(),
                dense::photon_rle(&pixels, &mut never(), usize::MAX).ok()
            );
            // Mapping (as format quantization does) equals encoding the mapped pixels.
            let threshold = |v: u8| if v >= 128 { 255 } else { 0 };
            let mapped: Vec<u8> = pixels.iter().map(|&v| threshold(v)).collect();
            assert_eq!(
                super::photon_rle(&mask, threshold, &mut never(), usize::MAX).unwrap(),
                dense::photon_rle(&mapped, &mut never(), usize::MAX).unwrap()
            );
        }
    }
}
