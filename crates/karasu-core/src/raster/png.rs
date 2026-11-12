//! Bounded-memory grayscale PNG export following <https://www.w3.org/TR/png-3/>.
//!
//! Samples are encoded verbatim with filter zero and stored DEFLATE blocks (RFC 1950/1951).
//! No gamma metadata is asserted. Failure or cancellation can leave a partial destination;
//! callers requiring atomic publication must write a temporary file and rename it themselves.

use crate::cancellation::{Cancellation, Cancelled, Checkpoint};
use std::{
    collections::TryReserveError,
    error::Error,
    fmt,
    io::{self, Write},
};

/// Failure while validating or streaming a grayscale PNG.
#[derive(Debug)]
pub enum PngError {
    /// Dimensions or sample count violate the PNG representation.
    InvalidImage,
    /// The bounded block buffer could not be allocated.
    Allocation(TryReserveError),
    /// The destination writer rejected output; bytes may already have been written.
    Io(io::Error),
    /// The caller cancelled output; bytes may already have been written.
    Cancelled(Cancelled),
}
impl fmt::Display for PngError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidImage => f.write_str("invalid PNG dimensions or grayscale sample count"),
            Self::Allocation(error) => write!(f, "PNG block allocation failed: {error}"),
            Self::Io(error) => write!(f, "PNG output failed: {error}"),
            Self::Cancelled(error) => error.fmt(f),
        }
    }
}
impl Error for PngError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidImage => None,
            Self::Allocation(error) => Some(error),
            Self::Io(error) => Some(error),
            Self::Cancelled(error) => Some(error),
        }
    }
}
impl From<io::Error> for PngError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}
impl From<Cancelled> for PngError {
    fn from(error: Cancelled) -> Self {
        Self::Cancelled(error)
    }
}

/// Streams a PNG with at most 65,535 bytes of block storage plus one row.
///
/// The writer is neither flushed nor rolled back. Stored DEFLATE blocks provide deterministic
/// encoding without compression dependencies. Samples retain their original coverage values.
/// # Errors
/// Returns invalid dimensions, allocation, or destination I/O errors.
pub fn write_png<W: Write>(writer: &mut W, image: &super::RasterImage) -> Result<(), PngError> {
    write_png_with_cancellation(writer, image, &mut Cancellation::never())
}

/// Streams a PNG, checking cancellation before output and between bounded blocks.
///
/// Cancellation or any writer failure can leave partial output in the destination.
/// # Errors
/// Returns validation, allocation, I/O, or cancellation errors.
pub fn write_png_with_cancellation<W: Write>(
    writer: &mut W,
    image: &super::RasterImage,
    cancellation: &mut Cancellation<'_>,
) -> Result<(), PngError> {
    let mask = image.mask();
    encode_rows(
        writer,
        image.width(),
        image.height(),
        &mut |y, row| mask.fill_row(y, row),
        cancellation,
    )
}

/// A grayscale PNG of `pixels`, row by row (the tests' reference for the streaming writers).
#[cfg(test)]
pub(crate) fn encode<W: Write>(
    writer: &mut W,
    width: u32,
    height: u32,
    pixels: &[u8],
    cancellation: &mut Cancellation<'_>,
) -> Result<(), PngError> {
    let w = usize::try_from(width).map_err(|_| PngError::InvalidImage)?;
    let h = usize::try_from(height).map_err(|_| PngError::InvalidImage)?;
    if w.checked_mul(h) != Some(pixels.len()) {
        return Err(PngError::InvalidImage);
    }
    encode_rows(
        writer,
        width,
        height,
        &mut |y, row| row.copy_from_slice(&pixels[y * w..(y + 1) * w]),
        cancellation,
    )
}

/// Stream a PNG whose row `y` is written by `fill_row(y, row)` into a `width`-byte buffer.
fn encode_rows<W: Write>(
    writer: &mut W,
    width: u32,
    height: u32,
    fill_row: &mut dyn FnMut(usize, &mut [u8]),
    cancellation: &mut Cancellation<'_>,
) -> Result<(), PngError> {
    encode_typed(writer, width, height, 1, fill_row, cancellation)
}

/// An 8-bit RGB PNG of `rgb` (`width * height * 3` bytes, rows top to bottom), with the same
/// stored DEFLATE encoding as the grayscale writer.
pub(crate) fn encode_rgb<W: Write>(
    writer: &mut W,
    width: u32,
    height: u32,
    rgb: &[u8],
    cancellation: &mut Cancellation<'_>,
) -> Result<(), PngError> {
    let w = usize::try_from(width).map_err(|_| PngError::InvalidImage)? * 3;
    let h = usize::try_from(height).map_err(|_| PngError::InvalidImage)?;
    if w.checked_mul(h) != Some(rgb.len()) {
        return Err(PngError::InvalidImage);
    }
    encode_typed(
        writer,
        width,
        height,
        3,
        &mut |y, row| row.copy_from_slice(&rgb[y * w..(y + 1) * w]),
        cancellation,
    )
}

/// An 8-bit RGBA PNG whose row `y` (`width * 4` bytes) `fill_row(y, row)` writes, with the same
/// stored DEFLATE encoding as the grayscale writer.
pub(crate) fn encode_rgba<W: Write>(
    writer: &mut W,
    width: u32,
    height: u32,
    fill_row: &mut dyn FnMut(usize, &mut [u8]),
    cancellation: &mut Cancellation<'_>,
) -> Result<(), PngError> {
    encode_typed(writer, width, height, 4, fill_row, cancellation)
}

/// Grayscale (`channels` 1), RGB (3) or RGBA (4) rows of `width * channels` bytes.
fn encode_typed<W: Write>(
    writer: &mut W,
    width: u32,
    height: u32,
    channels: usize,
    fill_row: &mut dyn FnMut(usize, &mut [u8]),
    cancellation: &mut Cancellation<'_>,
) -> Result<(), PngError> {
    cancellation.check(Checkpoint::RasterPng)?;
    if width == 0 || height == 0 || width > i32::MAX as u32 || height > i32::MAX as u32 {
        return Err(PngError::InvalidImage);
    }
    let width_usize = usize::try_from(width)
        .ok()
        .and_then(|w| w.checked_mul(channels))
        .ok_or(PngError::InvalidImage)?;
    let height_usize = usize::try_from(height).map_err(|_| PngError::InvalidImage)?;
    width_usize
        .checked_mul(height_usize)
        .ok_or(PngError::InvalidImage)?;
    let mut row = Vec::new();
    row.try_reserve_exact(width_usize)
        .map_err(PngError::Allocation)?;
    row.resize(width_usize, 0);
    let mut cached = None;
    let stride = width_usize.checked_add(1).ok_or(PngError::InvalidImage)?;
    let total = stride
        .checked_mul(height_usize)
        .ok_or(PngError::InvalidImage)?;
    let mut block = Vec::new();
    block
        .try_reserve_exact(total.min(65535))
        .map_err(PngError::Allocation)?;
    writer.write_all(b"\x89PNG\r\n\x1a\n")?;
    let mut ihdr = [0_u8; 13];
    ihdr[..4].copy_from_slice(&width.to_be_bytes());
    ihdr[4..8].copy_from_slice(&height.to_be_bytes());
    ihdr[8] = 8;
    // Colour type 0 is grayscale, 2 is RGB, 6 is RGBA.
    ihdr[9] = match channels {
        3 => 2,
        4 => 6,
        _ => 0,
    };
    chunk(writer, *b"IHDR", &[&ihdr])?;
    chunk(writer, *b"IDAT", &[&[0x78, 0x01]])?;
    let mut offset = 0;
    let mut a = 1_u32;
    let mut b = 0_u32;
    while offset < total {
        cancellation.check(Checkpoint::RasterPng)?;
        block.clear();
        let count = (total - offset).min(65535);
        for index in offset..offset + count {
            let column = index % stride;
            let byte = if column == 0 {
                0
            } else {
                let y = index / stride;
                if cached != Some(y) {
                    fill_row(y, &mut row);
                    cached = Some(y);
                }
                row[column - 1]
            };
            block.push(byte);
            a = (a + u32::from(byte)) % 65521;
            b = (b + a) % 65521;
        }
        offset += count;
        let length = u16::try_from(count).map_err(|_| PngError::InvalidImage)?;
        let [lo, hi] = length.to_le_bytes();
        let [nlo, nhi] = (!length).to_le_bytes();
        let header = [u8::from(offset == total), lo, hi, nlo, nhi];
        chunk(writer, *b"IDAT", &[&header, &block])?;
    }
    chunk(writer, *b"IDAT", &[&((b << 16) | a).to_be_bytes()])?;
    cancellation.check(Checkpoint::RasterPng)?;
    chunk(writer, *b"IEND", &[])?;
    Ok(())
}

/// A compressed 8-bit PNG (gray, RGB or RGBA by `channels`): unfiltered scanlines compressed
/// with zlib at level 1. The output is deterministic for the pinned compressor version
/// (`miniz_oxide`, TD-057). Used where archive size matters (SL1, TD-063); the stored encoders
/// above keep their bytes.
pub(crate) fn encode_compressed<W: Write>(
    writer: &mut W,
    width: u32,
    height: u32,
    channels: usize,
    fill_row: &mut dyn FnMut(usize, &mut [u8]),
    cancellation: &mut Cancellation<'_>,
) -> Result<(), PngError> {
    cancellation.check(Checkpoint::RasterPng)?;
    if width == 0 || height == 0 || width > i32::MAX as u32 || height > i32::MAX as u32 {
        return Err(PngError::InvalidImage);
    }
    let colour = match channels {
        1 => 0,
        3 => 2,
        4 => 6,
        _ => return Err(PngError::InvalidImage),
    };
    let stride = usize::try_from(width)
        .ok()
        .and_then(|w| w.checked_mul(channels))
        .ok_or(PngError::InvalidImage)?;
    let rows = usize::try_from(height).map_err(|_| PngError::InvalidImage)?;
    let total = stride
        .checked_add(1)
        .and_then(|s| s.checked_mul(rows))
        .ok_or(PngError::InvalidImage)?;
    let mut raw = Vec::new();
    raw.try_reserve_exact(total).map_err(PngError::Allocation)?;
    let mut row = vec![0u8; stride];
    for y in 0..rows {
        cancellation.poll(Checkpoint::RasterPng, y)?;
        fill_row(y, &mut row);
        raw.push(0);
        raw.extend_from_slice(&row);
    }
    let compressed = miniz_oxide::deflate::compress_to_vec_zlib(&raw, 1);
    drop(raw);
    writer.write_all(b"\x89PNG\r\n\x1a\n")?;
    let mut ihdr = [0_u8; 13];
    ihdr[..4].copy_from_slice(&width.to_be_bytes());
    ihdr[4..8].copy_from_slice(&height.to_be_bytes());
    ihdr[8] = 8;
    ihdr[9] = colour;
    chunk(writer, *b"IHDR", &[&ihdr])?;
    for part in compressed.chunks(1 << 20) {
        cancellation.check(Checkpoint::RasterPng)?;
        chunk(writer, *b"IDAT", &[part])?;
    }
    chunk(writer, *b"IEND", &[])?;
    Ok(())
}

fn chunk<W: Write>(writer: &mut W, kind: [u8; 4], parts: &[&[u8]]) -> Result<(), PngError> {
    let length: usize = parts.iter().map(|part| part.len()).sum();
    writer.write_all(
        &u32::try_from(length)
            .map_err(|_| PngError::InvalidImage)?
            .to_be_bytes(),
    )?;
    writer.write_all(&kind)?;
    let mut crc = update_crc(u32::MAX, &kind);
    for part in parts {
        writer.write_all(part)?;
        crc = update_crc(crc, part);
    }
    writer.write_all(&(!crc).to_be_bytes())?;
    Ok(())
}

#[allow(clippy::cast_possible_truncation)] // Table index is strictly less than 256.
const fn crc_table() -> [u32; 256] {
    let mut table = [0; 256];
    let mut index = 0;
    while index < 256 {
        let mut value = index as u32;
        let mut bit = 0;
        while bit < 8 {
            value = if value & 1 != 0 {
                (value >> 1) ^ 0xedb8_8320
            } else {
                value >> 1
            };
            bit += 1;
        }
        table[index] = value;
        index += 1;
    }
    table
}
fn update_crc(mut crc: u32, bytes: &[u8]) -> u32 {
    const TABLE: [u32; 256] = crc_table();
    for byte in bytes {
        crc = (crc >> 8) ^ TABLE[((crc ^ u32::from(*byte)) & 255) as usize];
    }
    crc
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn crc_matches_standard_check_vector() {
        assert_eq!(!update_crc(u32::MAX, b"123456789"), 0xcbf4_3926);
    }
    #[test]
    fn dimensions_fail_before_writing() {
        let mut output = Vec::new();
        assert!(matches!(
            encode(&mut output, 0, 1, &[], &mut Cancellation::never()),
            Err(PngError::InvalidImage)
        ));
        assert!(matches!(
            encode(&mut output, 2, 2, &[0], &mut Cancellation::never()),
            Err(PngError::InvalidImage)
        ));
        assert_eq!(output, [] as [u8; 0]);
    }
    #[test]
    fn stored_blocks_split_and_filter_rows_exactly() {
        let pixels: Vec<u8> = (0..70000)
            .map(|index| u8::try_from(index % 251).unwrap())
            .collect();
        let mut output = Vec::new();
        encode(&mut output, 35000, 2, &pixels, &mut Cancellation::never()).unwrap();
        assert_eq!(&output[..8], b"\x89PNG\r\n\x1a\n");
        let mut offset = 8;
        let mut compressed = Vec::new();
        while offset < output.len() {
            let length =
                u32::from_be_bytes(output[offset..offset + 4].try_into().unwrap()) as usize;
            let kind = &output[offset + 4..offset + 8];
            let data = &output[offset + 8..offset + 8 + length];
            let crc = u32::from_be_bytes(
                output[offset + 8 + length..offset + 12 + length]
                    .try_into()
                    .unwrap(),
            );
            assert_eq!(!update_crc(update_crc(u32::MAX, kind), data), crc);
            if kind == b"IDAT" {
                compressed.extend_from_slice(data);
            }
            offset += 12 + length;
        }
        assert_eq!(&compressed[..2], &[0x78, 1]);
        let mut position = 2;
        let mut decoded = Vec::new();
        let mut blocks = 0;
        loop {
            let final_block = compressed[position];
            let length =
                u16::from_le_bytes(compressed[position + 1..position + 3].try_into().unwrap());
            let inverse =
                u16::from_le_bytes(compressed[position + 3..position + 5].try_into().unwrap());
            assert_eq!(length, !inverse);
            position += 5;
            decoded.extend_from_slice(&compressed[position..position + usize::from(length)]);
            position += usize::from(length);
            blocks += 1;
            if final_block == 1 {
                break;
            }
            assert_eq!(final_block, 0);
            assert_eq!(length, 65535);
        }
        assert_eq!(blocks, 2);
        assert_eq!(position + 4, compressed.len());
        assert_eq!(decoded[0], 0);
        assert_eq!(decoded[35001], 0);
        assert_eq!(&decoded[1..35001], &pixels[..35000]);
        assert_eq!(&decoded[35002..], &pixels[35000..]);
        let (mut a, mut b) = (1_u32, 0_u32);
        for byte in decoded {
            a = (a + u32::from(byte)) % 65521;
            b = (b + a) % 65521;
        }
        assert_eq!(&compressed[position..], &((b << 16) | a).to_be_bytes());
    }
    #[test]
    fn cancellation_before_output_is_atomic() {
        let mut predicate = |_| true;
        let mut output = Vec::new();
        assert!(matches!(
            encode(
                &mut output,
                1,
                1,
                &[0],
                &mut Cancellation::new(&mut predicate)
            ),
            Err(PngError::Cancelled(_))
        ));
        assert_eq!(output, [] as [u8; 0]);
    }
    #[test]
    fn cancellation_between_blocks_reports_partial_destination() {
        let pixels = vec![127; 70_000];
        let mut checks = 0;
        let mut predicate = |_| {
            checks += 1;
            checks == 3
        };
        let mut output = Vec::new();
        let error = encode(
            &mut output,
            35_000,
            2,
            &pixels,
            &mut Cancellation::new(&mut predicate),
        )
        .unwrap_err();
        assert!(matches!(error, PngError::Cancelled(_)));
        assert!(output.len() > 65_535);
        assert!(!output.ends_with(b"IEND\xaeB`\x82"));
    }
    #[test]
    fn writer_failure_retains_cause() {
        struct Broken;
        impl Write for Broken {
            fn write(&mut self, _: &[u8]) -> io::Result<usize> {
                Err(io::Error::new(io::ErrorKind::BrokenPipe, "test sink"))
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let error = encode(&mut Broken, 1, 1, &[255], &mut Cancellation::never()).unwrap_err();
        assert!(matches!(&error, PngError::Io(error) if error.kind() == io::ErrorKind::BrokenPipe));
        assert!(error.source().is_some());
    }
}
