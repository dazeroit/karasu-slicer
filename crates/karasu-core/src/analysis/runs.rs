//! Pixel runs shared by the layer analyses: each row's runs, grown by a lattice disc, and a
//! union-find whose groups do not depend on visiting order.
use crate::raster::mask::SpanMask;

/// Runs of pixels per row: row `y` holds `runs[rows[y]..rows[y + 1]]`, sorted and disjoint,
/// each `[start, end)`.
#[derive(Clone, Debug, Default)]
pub(super) struct Runs {
    pub(super) rows: Vec<usize>,
    pub(super) runs: Vec<[u32; 2]>,
}

impl Runs {
    pub(super) fn row(&self, y: usize) -> &[[u32; 2]] {
        match (self.rows.get(y), self.rows.get(y + 1)) {
            (Some(&a), Some(&b)) => &self.runs[a..b],
            _ => &[],
        }
    }

    /// The pixels of `mask` whose gray reaches `gray`, adjacent spans merged.
    pub(super) fn of(mask: &SpanMask, gray: u8) -> Self {
        let mut runs = Self {
            rows: Vec::with_capacity(mask.height() as usize + 1),
            runs: Vec::new(),
        };
        runs.rows.push(0);
        for row in mask.rows() {
            let first = runs.runs.len();
            for span in row.iter().filter(|s| s.value() >= gray) {
                // Adjacent spans of this row (different grays) make one run.
                let extends = runs.runs.len() > first
                    && runs.runs.last().is_some_and(|last| last[1] == span.start());
                match runs.runs.last_mut() {
                    Some(last) if extends => last[1] = span.end(),
                    _ => runs.runs.push([span.start(), span.end()]),
                }
            }
            runs.rows.push(runs.runs.len());
        }
        runs
    }

    pub(super) fn height(&self) -> usize {
        self.rows.len().saturating_sub(1)
    }
}

/// How much of the reach's own size a centre may lie beyond it and still be within reach: the
/// rounding of the angle's tangent and of the planes' heights (a slope exactly at the critical
/// angle computes a reach of 0.999… pixel), far below any pixel.
const REACH_ROUNDING: f64 = 1.0e-9;

/// The half-widths, in columns, of the lattice disc of radius `d` mm: for each row offset `dy`
/// from 0 up, the largest column offset whose centre lies within `d`.
pub(super) fn disc(d: f64, pitch: [f64; 2]) -> Vec<u32> {
    let d = d * (1.0 + REACH_ROUNDING);
    let rows = (d / pitch[1]).floor();
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // Small, non-negative.
    let rows = rows as u32;
    (0..=rows)
        .map(|dy| {
            let along = f64::from(dy) * pitch[1];
            let left = (d * d - along * along).max(0.0).sqrt() / pitch[0];
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // Non-negative.
            let half = left.floor() as u32;
            half
        })
        .collect()
}

/// Row `y` of `source` grown by the disc `half` (half-widths by row offset), within `width`:
/// sorted, merged runs written to `out`.
pub(super) fn reach_row(
    source: &Runs,
    y: usize,
    half: &[u32],
    width: u32,
    out: &mut Vec<[u32; 2]>,
) {
    out.clear();
    let reach = half.len().saturating_sub(1);
    let from = y.saturating_sub(reach);
    let to = (y + reach).min(source.height().saturating_sub(1));
    for row in from..=to {
        let w = half[row.abs_diff(y)];
        for &[a, b] in source.row(row) {
            out.push([a.saturating_sub(w), (b + w).min(width)]);
        }
    }
    out.sort_unstable();
    let mut merged = 0;
    for k in 0..out.len() {
        if merged > 0 && out[k][0] <= out[merged - 1][1] {
            out[merged - 1][1] = out[merged - 1][1].max(out[k][1]);
        } else {
            out[merged] = out[k];
            merged += 1;
        }
    }
    out.truncate(merged);
}

/// `runs` less `cut` (both sorted, disjoint), written to `out`.
pub(super) fn subtract(runs: &[[u32; 2]], cut: &[[u32; 2]], out: &mut Vec<[u32; 2]>) {
    out.clear();
    let mut c = 0;
    for &[start, end] in runs {
        let mut at = start;
        while c < cut.len() && cut[c][1] <= at {
            c += 1;
        }
        let mut k = c;
        while at < end {
            match cut.get(k) {
                Some(&[cs, ce]) if cs < end => {
                    if cs > at {
                        out.push([at, cs]);
                    }
                    at = at.max(ce);
                    k += 1;
                }
                _ => {
                    out.push([at, end]);
                    at = end;
                }
            }
        }
    }
}

/// A union-find whose roots are the lowest index, so groups do not depend on visiting order.
#[derive(Debug, Default)]
pub(super) struct Union(Vec<usize>);

impl Union {
    pub(super) fn add(&mut self) -> usize {
        self.0.push(self.0.len());
        self.0.len() - 1
    }

    pub(super) fn find(&mut self, mut a: usize) -> usize {
        while self.0[a] != a {
            self.0[a] = self.0[self.0[a]];
            a = self.0[a];
        }
        a
    }

    pub(super) fn join(&mut self, a: usize, b: usize) {
        let (a, b) = (self.find(a), self.find(b));
        if a < b {
            self.0[b] = a;
        } else if b < a {
            self.0[a] = b;
        }
    }
}

/// Whether runs `[a0, a1)` and `[b0, b1)` on adjacent rows touch by an edge or a corner, with
/// `slack` more columns of reach on each side (0: 8-neighbours).
pub(super) fn touch(a: [u32; 2], b: [u32; 2], slack: u32) -> bool {
    a[0] <= b[1] + slack && b[0] <= a[1] + slack
}
