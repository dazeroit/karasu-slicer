//! Row-span layer masks (TD-055): each row stores only its nonzero runs of equal intensity.
//!
//! A mask holds, per row, the sorted, disjoint, maximal spans `[start, end)` of one nonzero
//! grayscale8 value; every other pixel is zero. The form is canonical: two masks with the same
//! pixels hold the same spans, so equality of masks is equality of images. Memory and work are
//! proportional to the spans, not to the panel area, which keeps 12K/16K layers cheap to build,
//! hold and encode.

use super::RasterError;
use std::mem::size_of;

/// One maximal run `[start, end)` of a nonzero intensity within a row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    start: u32,
    end: u32,
    value: u8,
}

impl Span {
    /// First pixel column of the run.
    #[must_use]
    pub const fn start(self) -> u32 {
        self.start
    }
    /// One past the last pixel column of the run.
    #[must_use]
    pub const fn end(self) -> u32 {
        self.end
    }
    /// The run's grayscale8 intensity (never zero).
    #[must_use]
    pub const fn value(self) -> u8 {
        self.value
    }
}

/// An immutable row-major grayscale8 mask stored as nonzero row spans.
///
/// Invariants: within each row, spans are sorted, nonempty, nonzero, inside `0..width`, disjoint,
/// and two adjacent spans (one ending where the next starts) have different values.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpanMask {
    width: u32,
    height: u32,
    spans: Vec<Span>,
    /// Row `y` holds `spans[rows[y]..rows[y + 1]]`; `height + 1` entries.
    rows: Vec<usize>,
}

impl SpanMask {
    /// Mask width in pixels.
    #[must_use]
    pub const fn width(&self) -> u32 {
        self.width
    }

    /// Mask height in pixels.
    #[must_use]
    pub const fn height(&self) -> u32 {
        self.height
    }

    /// Total pixels, `width * height`.
    #[must_use]
    pub fn pixel_count(&self) -> usize {
        self.width as usize * self.height as usize
    }

    /// The nonzero spans of row `y`, or `None` outside the mask.
    #[must_use]
    pub fn row(&self, y: u32) -> Option<&[Span]> {
        let y = y as usize;
        let (&start, &end) = (self.rows.get(y)?, self.rows.get(y + 1)?);
        Some(&self.spans[start..end])
    }

    /// Every row's spans, top to bottom.
    pub fn rows(&self) -> impl Iterator<Item = &[Span]> {
        self.rows
            .windows(2)
            .map(|bounds| &self.spans[bounds[0]..bounds[1]])
    }

    /// Total number of spans.
    #[must_use]
    pub fn span_count(&self) -> usize {
        self.spans.len()
    }

    /// The intensity at `(x, y)`, or `None` outside the mask.
    #[must_use]
    pub fn pixel(&self, x: u32, y: u32) -> Option<u8> {
        if x >= self.width {
            return None;
        }
        let row = self.row(y)?;
        let after = row.partition_point(|span| span.start <= x);
        Some(
            after
                .checked_sub(1)
                .map(|i| row[i])
                .filter(|span| x < span.end)
                .map_or(0, |span| span.value),
        )
    }

    /// Write row `y` into `out` (exactly `width` bytes); zero pixels are written too.
    ///
    /// # Panics
    /// When `y` is outside the mask or `out` is not `width` bytes long (caller invariants).
    pub(crate) fn fill_row(&self, y: usize, out: &mut [u8]) {
        assert_eq!(out.len(), self.width as usize, "row buffer width");
        out.fill(0);
        for span in &self.spans[self.rows[y]..self.rows[y + 1]] {
            out[span.start as usize..span.end as usize].fill(span.value);
        }
    }

    /// The dense row-major pixels.
    ///
    /// # Errors
    /// [`RasterError::SizeOverflow`] or [`RasterError::Allocation`] when the dense image cannot be
    /// represented or allocated.
    pub fn to_pixels(&self) -> Result<Vec<u8>, RasterError> {
        let width = self.width as usize;
        let total = width
            .checked_mul(self.height as usize)
            .ok_or(RasterError::SizeOverflow)?;
        let mut pixels = super::reserve::<u8>(total)?;
        pixels.resize(total, 0);
        if width > 0 {
            for (y, row) in pixels.chunks_exact_mut(width).enumerate() {
                self.fill_row(y, row);
            }
        }
        Ok(pixels)
    }

    /// The canonical mask of dense row-major `pixels`.
    ///
    /// # Errors
    /// [`RasterError::InvalidProfile`] when `pixels` is not `width * height` long, and allocation
    /// failures.
    pub fn from_pixels(width: u32, height: u32, pixels: &[u8]) -> Result<Self, RasterError> {
        let w = width as usize;
        if w.checked_mul(height as usize) != Some(pixels.len()) {
            return Err(RasterError::InvalidProfile);
        }
        let mut builder = SpanMaskBuilder::new(width, height, usize::MAX)?;
        if w > 0 {
            for row in pixels.chunks_exact(w) {
                let mut x = 0;
                while x < w {
                    let value = row[x];
                    let start = x;
                    while x < w && row[x] == value {
                        x += 1;
                    }
                    if value != 0 {
                        // Columns are below `width`, which fits u32.
                        builder.push(to_u32(start)?, to_u32(x)?, value)?;
                    }
                }
                builder.end_row()?;
            }
        } else {
            for _ in 0..height {
                builder.end_row()?;
            }
        }
        builder.finish()
    }

    /// Visit the image as maximal row-major runs `(value, length)` covering every pixel once,
    /// zeros included; runs continue across row ends. Stops at the first error of `visit`.
    pub(crate) fn for_each_run<E>(
        &self,
        mut visit: impl FnMut(u8, usize) -> Result<(), E>,
    ) -> Result<(), E> {
        let width = self.width as usize;
        let (mut value, mut length) = (0u8, 0usize);
        let mut segment = |v: u8, n: usize, visit: &mut dyn FnMut(u8, usize) -> Result<(), E>| {
            if n == 0 {
                return Ok(());
            }
            if v == value {
                length += n;
                return Ok(());
            }
            if length > 0 {
                visit(value, length)?;
            }
            (value, length) = (v, n);
            Ok(())
        };
        for row in self.rows() {
            let mut x = 0usize;
            for span in row {
                let (start, end) = (span.start as usize, span.end as usize);
                segment(0, start - x, &mut visit)?;
                segment(span.value, end - start, &mut visit)?;
                x = end;
            }
            segment(0, width - x, &mut visit)?;
        }
        if length > 0 {
            visit(value, length)?;
        }
        Ok(())
    }

    /// Bytes held by the span and row tables.
    #[must_use]
    pub fn retained_bytes(&self) -> usize {
        self.spans.capacity() * size_of::<Span>() + self.rows.capacity() * size_of::<usize>()
    }
}

fn to_u32(n: usize) -> Result<u32, RasterError> {
    u32::try_from(n).map_err(|_| RasterError::SizeOverflow)
}

/// Row-by-row construction of a canonical [`SpanMask`].
pub(crate) struct SpanMaskBuilder {
    mask: SpanMask,
    /// Span count cap from the caller's payload budget.
    max_spans: usize,
}

impl SpanMaskBuilder {
    /// Bytes reserved up front for `height` rows (the row table).
    pub(crate) fn row_table_bytes(height: usize) -> Option<usize> {
        height.checked_add(1)?.checked_mul(size_of::<usize>())
    }

    /// Bytes per retained span.
    pub(crate) const SPAN_BYTES: usize = size_of::<Span>();

    /// Start an empty mask; at most `max_spans` spans may be pushed.
    pub(crate) fn new(width: u32, height: u32, max_spans: usize) -> Result<Self, RasterError> {
        let mut rows = super::reserve::<usize>(
            (height as usize)
                .checked_add(1)
                .ok_or(RasterError::SizeOverflow)?,
        )?;
        rows.push(0);
        Ok(Self {
            mask: SpanMask {
                width,
                height,
                spans: Vec::new(),
                rows,
            },
            max_spans,
        })
    }

    /// Append `[start, end)` of `value` to the current row. Spans arrive left to right and never
    /// overlap; zero values and empty spans are ignored, and a span continuing the previous one
    /// with the same value extends it.
    pub(crate) fn push(&mut self, start: u32, end: u32, value: u8) -> Result<(), RasterError> {
        if value == 0 || start >= end {
            return Ok(());
        }
        if end > self.mask.width {
            return Err(RasterError::InvalidGeometry);
        }
        let row_start = *self.mask.rows.last().unwrap_or(&0);
        let in_row = self.mask.spans.len() > row_start;
        if let Some(last) = self.mask.spans.last_mut().filter(|_| in_row) {
            if start < last.end {
                return Err(RasterError::InvalidGeometry);
            }
            if start == last.end && value == last.value {
                last.end = end;
                return Ok(());
            }
        }
        if self.mask.spans.len() >= self.max_spans {
            return Err(RasterError::Limit {
                resource: "payload bytes",
                limit: self.max_spans.saturating_mul(Self::SPAN_BYTES),
            });
        }
        self.mask
            .spans
            .try_reserve(1)
            .map_err(|_| RasterError::Allocation)?;
        self.mask.spans.push(Span { start, end, value });
        Ok(())
    }

    /// Close the current row.
    pub(crate) fn end_row(&mut self) -> Result<(), RasterError> {
        if self.mask.rows.len() > self.mask.height as usize {
            return Err(RasterError::InvalidGeometry);
        }
        self.mask.rows.push(self.mask.spans.len());
        Ok(())
    }

    /// The finished mask; every row must have been closed.
    pub(crate) fn finish(mut self) -> Result<SpanMask, RasterError> {
        if self.mask.rows.len() != self.mask.height as usize + 1 {
            return Err(RasterError::InvalidGeometry);
        }
        self.mask.spans.shrink_to_fit();
        Ok(self.mask)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[allow(clippy::cast_possible_truncation)] // Small test dimensions, indices and levels.
    fn random_pixels(seed: u64, len: usize, levels: &[u8]) -> Vec<u8> {
        let mut state = seed | 1;
        (0..len)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                // Long runs as well as single pixels.
                levels[(state % levels.len() as u64) as usize]
            })
            .collect()
    }

    #[test]
    #[allow(clippy::cast_possible_truncation)] // Small test dimensions, indices and levels.
    fn dense_round_trip_is_canonical_and_exact() {
        for (case, (w, h)) in [(1, 1), (7, 3), (64, 9), (5, 1), (1, 6)]
            .into_iter()
            .enumerate()
        {
            for levels in [
                &[0u8, 255][..],
                &[0, 0, 0, 7, 255],
                &[0, 1, 2, 3, 255, 255, 255],
            ] {
                let pixels = random_pixels(case as u64 + 11, w * h, levels);
                let mask = SpanMask::from_pixels(w as u32, h as u32, &pixels).unwrap();
                assert_eq!(mask.to_pixels().unwrap(), pixels);
                for row in mask.rows() {
                    for pair in row.windows(2) {
                        assert!(pair[0].end <= pair[1].start);
                        assert!(pair[0].end < pair[1].start || pair[0].value != pair[1].value);
                    }
                    assert!(row.iter().all(|s| s.value != 0 && s.start < s.end));
                }
                for (i, &p) in pixels.iter().enumerate() {
                    assert_eq!(mask.pixel((i % w) as u32, (i / w) as u32), Some(p));
                }
                assert_eq!(mask.pixel(w as u32, 0), None);
                assert_eq!(mask.pixel(0, h as u32), None);
                // Runs cover every pixel, are maximal across rows and expand to the pixels.
                let mut expanded = Vec::new();
                let mut previous = None;
                mask.for_each_run(|value, length| {
                    assert!(length > 0 && previous != Some(value));
                    previous = Some(value);
                    expanded.extend(std::iter::repeat_n(value, length));
                    Ok::<_, ()>(())
                })
                .unwrap();
                assert_eq!(expanded, pixels);
            }
        }
    }

    #[test]
    fn builder_merges_and_rejects_invalid_spans() {
        let mut b = SpanMaskBuilder::new(10, 2, 3).unwrap();
        b.push(0, 2, 9).unwrap();
        b.push(2, 4, 9).unwrap(); // Continues the same value: merged.
        b.push(4, 4, 5).unwrap(); // Empty: ignored.
        b.push(5, 6, 0).unwrap(); // Zero: ignored.
        assert_eq!(b.push(3, 5, 1), Err(RasterError::InvalidGeometry));
        b.end_row().unwrap();
        b.push(0, 1, 9).unwrap(); // A new row never merges with the previous one.
        b.push(1, 2, 8).unwrap();
        assert!(matches!(b.push(3, 4, 7), Err(RasterError::Limit { .. })));
        assert_eq!(b.push(9, 11, 1), Err(RasterError::InvalidGeometry));
        b.end_row().unwrap();
        let mask = b.finish().unwrap();
        assert_eq!(mask.span_count(), 3);
        assert_eq!(
            mask.to_pixels().unwrap(),
            [9, 9, 9, 9, 0, 0, 0, 0, 0, 0, 9, 8, 0, 0, 0, 0, 0, 0, 0, 0]
        );
        assert!(SpanMaskBuilder::new(1, 1, 1).unwrap().finish().is_err());
        assert!(SpanMask::from_pixels(2, 2, &[0; 3]).is_err());
    }
}
