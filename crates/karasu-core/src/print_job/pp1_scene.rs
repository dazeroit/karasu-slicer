//! Independently derived Photon P1 scene metadata and bounded raster component statistics.
//!
//! Areas describe nonzero raster pixels, not proprietary contour reconstruction. Connectivity is
//! eight-neighbor. Bounds are outer pixel edges in centered panel millimeters. These definitions
//! are stable and intentionally do not claim equality with a commercial contour-area estimator.
use super::machine::CodecError;
use crate::{
    cancellation::{Cancellation, Checkpoint},
    raster::mask::{Span, SpanMask},
};

#[derive(Clone, Debug)]
struct Row {
    z: f32,
    area: f32,
    bounds: [f32; 4],
    objects: u32,
    largest_area: f32,
}
/// Small retained layer records; component scratch is proportional to the raster width.
#[derive(Default)]
pub(crate) struct Scene {
    rows: Vec<Row>,
    bounds: Option<[f32; 4]>,
}
fn allocation<T>(v: &mut Vec<T>, additional: usize) -> Result<(), CodecError> {
    v.try_reserve_exact(additional)
        .map_err(|_| CodecError::Limit("P1 scene allocation"))
}
#[allow(clippy::cast_possible_truncation)] // Explicitly checked protocol f32 conversion.
fn scalar(value: f64) -> Result<f32, CodecError> {
    let converted = value as f32;
    if !value.is_finite() || !converted.is_finite() || (value != 0. && converted == 0.) {
        return Err(CodecError::Invalid("P1 scene scalar exceeds f32 domain"));
    }
    Ok(converted)
}
fn root(parent: &mut [usize], mut n: usize) -> usize {
    while parent[n] != n {
        parent[n] = parent[parent[n]];
        n = parent[n];
    }
    n
}
fn union(parent: &mut [usize], areas: &mut [u64], a: usize, b: usize) -> usize {
    let a = root(parent, a);
    let b = root(parent, b);
    if a == b {
        return a;
    }
    // Deterministic smaller-label representative; at most two active scanlines exist.
    let (small, large) = if a < b { (a, b) } else { (b, a) };
    parent[large] = small;
    areas[small] += areas[large];
    areas[large] = 0;
    small
}

/// Half-open runs `[start, end)` of pixels above `threshold` in one row of spans; touching spans
/// above the threshold form one run.
fn runs(row: &[Span], threshold: u8, out: &mut Vec<(usize, usize)>) {
    out.clear();
    for span in row.iter().filter(|span| span.value() > threshold) {
        let (start, end) = (span.start() as usize, span.end() as usize);
        match out.last_mut() {
            Some(last) if last.1 == start => last.1 = end,
            _ => out.push((start, end)),
        }
    }
}

/// Count one finished component of `area` pixels.
fn finish(objects: &mut u32, largest: &mut u64, area: u64) -> Result<(), CodecError> {
    *objects = objects
        .checked_add(1)
        .ok_or(CodecError::Limit("P1 component count"))?;
    *largest = (*largest).max(area);
    Ok(())
}

/// Raster metrics of one P1 layer: a pure function of its pixels, so it can be computed on encoder
/// workers and appended to the scene in layer order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LayerMetrics {
    resolution: [u32; 2],
    /// Pixels above the zero threshold.
    nonzero: u64,
    /// Outer pixel bounds `[x0, y0, x1, y1]` of those pixels (meaningful when `nonzero > 0`).
    bounds: [usize; 4],
    /// Eight-connected components.
    objects: u32,
    /// Pixel count of the largest component.
    largest: u64,
}

/// Measure one layer: nonzero area, outer bounds, eight-connected components and the largest one.
///
/// Components are labelled per run of pixels above `zero_threshold`, row by row: each run joins
/// the runs of the previous row within one pixel of it, and a component is finished when no run
/// of the current row continues it. Scratch stays proportional to the raster width, and the
/// work is proportional to the mask's spans, not to the panel area.
pub(crate) fn layer_metrics(
    mask: &SpanMask,
    resolution: [u32; 2],
    zero_threshold: u8,
    cancellation: &mut Cancellation<'_>,
) -> Result<LayerMetrics, CodecError> {
    cancellation.check(Checkpoint::ExportLayer)?;
    let [width, height] = resolution.map(|n| n as usize);
    if width == 0 || width > 65536 || height == 0 || [mask.width(), mask.height()] != resolution {
        return Err(CodecError::Invalid("P1 scene dimensions"));
    }
    // A row holds at most `width / 2 + 1` runs; union-find nodes are the previous row's open
    // components plus the current row's runs.
    let row_runs = width / 2 + 1;
    let nodes = 2 * row_runs;
    let mut current = Vec::new();
    let mut previous: Vec<(usize, usize, usize)> = Vec::new();
    let mut parent = Vec::new();
    let mut areas = Vec::new();
    let mut present = Vec::new();
    let mut compact = Vec::new();
    let mut open_areas = Vec::new();
    let mut next_areas = Vec::new();
    allocation(&mut current, row_runs)?;
    allocation(&mut previous, row_runs)?;
    allocation(&mut parent, nodes)?;
    allocation(&mut areas, nodes)?;
    allocation(&mut present, nodes)?;
    allocation(&mut compact, nodes)?;
    allocation(&mut open_areas, row_runs)?;
    allocation(&mut next_areas, row_runs)?;
    let (mut objects, mut largest, mut nonzero) = (0u32, 0u64, 0u64);
    let mut bounds = [width, height, 0usize, 0usize];
    for (y, row) in mask.rows().enumerate() {
        cancellation.poll(Checkpoint::ExportLayer, y)?;
        runs(row, zero_threshold, &mut current);
        if current.is_empty() && previous.is_empty() {
            continue;
        }
        // Nodes 0..open are the previous row's components; open + i is the current row's run i.
        let open = open_areas.len();
        parent.clear();
        parent.extend(0..open + current.len());
        areas.clear();
        areas.extend_from_slice(&open_areas);
        let mut j = 0;
        for (i, &(start, end)) in current.iter().enumerate() {
            let length = (end - start) as u64;
            areas.push(length);
            nonzero += length;
            bounds = [
                bounds[0].min(start),
                bounds[1].min(y),
                bounds[2].max(end),
                bounds[3].max(y + 1),
            ];
            // A previous run [ps, pe) touches [start, end) diagonally or directly exactly when
            // ps <= end and pe >= start. Runs are ordered, so a forward sweep finds them all.
            while j < previous.len() && previous[j].1 < start {
                j += 1;
            }
            let mut k = j;
            while k < previous.len() && previous[k].0 <= end {
                union(&mut parent, &mut areas, open + i, previous[k].2);
                k += 1;
            }
        }
        present.clear();
        present.resize(parent.len(), false);
        for i in 0..current.len() {
            let label = root(&mut parent, open + i);
            present[label] = true;
        }
        compact.clear();
        compact.resize(parent.len(), 0usize);
        next_areas.clear();
        for n in 0..parent.len() {
            if parent[n] != n {
                continue;
            }
            if present[n] {
                compact[n] = next_areas.len();
                next_areas.push(areas[n]);
            } else {
                finish(&mut objects, &mut largest, areas[n])?;
            }
        }
        previous.clear();
        for (i, &(start, end)) in current.iter().enumerate() {
            let label = root(&mut parent, open + i);
            previous.push((start, end, compact[label]));
        }
        std::mem::swap(&mut open_areas, &mut next_areas);
    }
    for &area in &open_areas {
        finish(&mut objects, &mut largest, area)?;
    }
    Ok(LayerMetrics {
        resolution,
        nonzero,
        bounds,
        objects,
        largest,
    })
}

impl Scene {
    pub(crate) fn new() -> Self {
        Self::default()
    }
    /// Append one layer using exact nonzero-pixel area and eight-neighbor component counts.
    /// The caller admits the maximum retained 64-byte records before processing the job.
    #[cfg(test)] // Encoders measure on workers and append with `push_metrics`.
    pub(crate) fn push_layer(
        &mut self,
        z_mm: f64,
        pixels: &[u8],
        resolution: [u32; 2],
        build_xy: [f64; 2],
        cancellation: &mut Cancellation<'_>,
    ) -> Result<(), CodecError> {
        self.push_layer_quantized(z_mm, pixels, resolution, build_xy, cancellation, 0)
    }
    /// Derive metrics after an explicit quantizer's zero threshold without allocating a mask copy.
    #[cfg(test)] // Encoders measure on workers and append with `push_metrics`.
    pub(crate) fn push_layer_quantized(
        &mut self,
        z_mm: f64,
        pixels: &[u8],
        resolution: [u32; 2],
        build_xy: [f64; 2],
        cancellation: &mut Cancellation<'_>,
        zero_threshold: u8,
    ) -> Result<(), CodecError> {
        self.admit(z_mm, resolution, build_xy)?;
        let mask = SpanMask::from_pixels(resolution[0], resolution[1], pixels)
            .map_err(|_| CodecError::Invalid("P1 scene dimensions"))?;
        let metrics = layer_metrics(&mask, resolution, zero_threshold, cancellation)?;
        self.push_metrics(z_mm, &metrics, resolution, build_xy, cancellation)
    }
    /// The checks of a new layer record, before any measurement.
    fn admit(
        &self,
        z_mm: f64,
        resolution: [u32; 2],
        build_xy: [f64; 2],
    ) -> Result<f32, CodecError> {
        if resolution.contains(&0)
            || resolution[0] > 65536
            || build_xy.iter().any(|&v| !v.is_finite() || v <= 0.)
            || z_mm <= 0.
        {
            return Err(CodecError::Invalid(
                "P1 scene dimensions, envelope or layer Z",
            ));
        }
        let z = scalar(z_mm)?;
        if self.rows.last().is_some_and(|row| z <= row.z) {
            return Err(CodecError::Invalid("P1 scene layer Z must increase in f32"));
        }
        Ok(z)
    }
    /// Append one layer from metrics measured beforehand (possibly on another thread) at the
    /// same resolution.
    pub(crate) fn push_metrics(
        &mut self,
        z_mm: f64,
        metrics: &LayerMetrics,
        resolution: [u32; 2],
        build_xy: [f64; 2],
        cancellation: &mut Cancellation<'_>,
    ) -> Result<(), CodecError> {
        let z = self.admit(z_mm, resolution, build_xy)?;
        if metrics.resolution != resolution {
            return Err(CodecError::Invalid("P1 scene metrics resolution"));
        }
        let LayerMetrics {
            nonzero,
            bounds,
            objects,
            largest,
            ..
        } = *metrics;
        let pitch = [
            build_xy[0] / f64::from(resolution[0]),
            build_xy[1] / f64::from(resolution[1]),
        ];
        let pixel_area = pitch[0] * pitch[1];
        let physical = if nonzero == 0 {
            [0f32; 4]
        } else {
            let mut result = [0f32; 4];
            for axis in 0..2 {
                result[axis] = scalar(
                    f64::from(
                        u32::try_from(bounds[axis])
                            .map_err(|_| CodecError::Limit("P1 raster bound"))?,
                    ) * pitch[axis]
                        - build_xy[axis] / 2.,
                )?;
                result[axis + 2] = scalar(
                    f64::from(
                        u32::try_from(bounds[axis + 2])
                            .map_err(|_| CodecError::Limit("P1 raster bound"))?,
                    ) * pitch[axis]
                        - build_xy[axis] / 2.,
                )?;
            }
            result
        };
        // Counts are bounded by a u32-by-u32 raster and the caller's admitted pixel budget.
        #[allow(clippy::cast_precision_loss)]
        let area = scalar(nonzero as f64 * pixel_area)?;
        #[allow(clippy::cast_precision_loss)]
        let largest_area = scalar(largest as f64 * pixel_area)?;
        self.rows
            .try_reserve(1)
            .map_err(|_| CodecError::Limit("P1 retained scene records"))?;
        cancellation.check(Checkpoint::ExportLayer)?;
        self.rows.push(Row {
            z,
            area,
            bounds: physical,
            objects,
            largest_area,
        });
        if nonzero != 0 {
            self.bounds = Some(match self.bounds {
                Some(b) => [
                    b[0].min(physical[0]),
                    b[1].min(physical[1]),
                    b[2].max(physical[2]),
                    b[3].max(physical[3]),
                ],
                None => physical,
            });
        }
        Ok(())
    }
    /// Serialize the observed P1 scene revision 3 with explicit Karasu provenance.
    pub(crate) fn to_bytes(
        &self,
        _title: &str,
        max_metadata_bytes: usize,
    ) -> Result<Vec<u8>, CodecError> {
        let count = u32::try_from(self.rows.len())
            .map_err(|_| CodecError::Limit("P1 scene layer count"))?;
        let size = self
            .rows
            .len()
            .checked_mul(64)
            .and_then(|n| n.checked_add(400))
            .ok_or(CodecError::Limit("P1 scene byte count"))?;
        if size > max_metadata_bytes {
            return Err(CodecError::Limit("P1 scene metadata budget"));
        }
        let mut out = Vec::new();
        allocation(&mut out, size)?;
        out.resize(396, 0);
        out[..13].copy_from_slice(b"ANYCUBIC-PWSZ");
        let software = concat!("karasu-slicer ", env!("CARGO_PKG_VERSION"));
        if software.len() >= 64 {
            return Err(CodecError::Invalid("P1 software provenance exceeds field"));
        }
        out[16..16 + software.len()].copy_from_slice(software.as_bytes());
        for (at, value) in [(80, 3u32), (84, 3), (100, count), (392, count)] {
            out[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        out[96..100].copy_from_slice(&1f32.to_le_bytes());
        let bounds = self.bounds.unwrap_or([0.; 4]);
        let maximum_z = self.rows.last().map_or(0., |row| row.z);
        for (at, value) in [
            (104, bounds[0]),
            (108, bounds[1]),
            (112, 0.),
            (116, bounds[2]),
            (120, bounds[3]),
            (124, maximum_z),
        ] {
            out[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        out[388..392].copy_from_slice(b"<---");
        for row in &self.rows {
            for value in [
                row.z,
                row.area,
                row.bounds[0],
                row.bounds[1],
                row.bounds[2],
                row.bounds[3],
            ] {
                out.extend(value.to_le_bytes());
            }
            out.extend(row.objects.to_le_bytes());
            out.extend(row.largest_area.to_le_bytes());
            out.resize(out.len() + 32, 0);
        }
        out.extend(b"--->");
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn inspect(pixels: &[u8], resolution: [u32; 2]) -> Scene {
        let mut scene = Scene::new();
        scene
            .push_layer(
                0.05,
                pixels,
                resolution,
                resolution.map(f64::from),
                &mut Cancellation::never(),
            )
            .unwrap();
        scene
    }
    #[test]
    fn diagonal_connectivity_holes_and_finished_components() {
        let scene = inspect(&[255, 0, 255, 0, 255, 0, 255, 0, 255], [3, 3]);
        assert_eq!(scene.rows[0].objects, 1);
        assert_eq!(scene.rows[0].area, 5.);
        assert_eq!(scene.rows[0].largest_area, 5.);
        let scene = inspect(&[255, 0, 0, 0, 0, 0, 0, 0, 255], [3, 3]);
        assert_eq!(scene.rows[0].objects, 2);
        assert_eq!(scene.rows[0].largest_area, 1.);
        let scene = inspect(&[255, 255, 255, 255, 0, 255, 255, 255, 255], [3, 3]);
        assert_eq!(scene.rows[0].objects, 1);
        assert_eq!(scene.rows[0].area, 8.);
    }
    #[test]
    fn revision_three_wire_offsets_and_centered_bounds() {
        let scene = inspect(&[255, 0, 0, 0], [2, 2]);
        let out = scene.to_bytes("ignored", 1024).unwrap();
        assert_eq!(out.len(), 464);
        assert_eq!(&out[84..88], &3u32.to_le_bytes());
        assert_eq!(&out[388..392], b"<---");
        assert_eq!(&out[460..], b"--->");
        assert_eq!(scene.rows[0].bounds, [-1., -1., 0., 0.]);
        assert!(matches!(scene.to_bytes("", 463), Err(CodecError::Limit(_))));
    }
    #[test]
    fn compact_scan_matches_independent_flood_fill_for_every_three_square_mask() {
        for bits in 0u16..512 {
            let pixels: Vec<_> = (0..9)
                .map(|i| if bits & (1 << i) != 0 { 255 } else { 0 })
                .collect();
            let scene = inspect(&pixels, [3, 3]);
            let mut seen = [false; 9];
            let mut objects = 0;
            let mut maximum = 0u32;
            for start in 0..9 {
                if pixels[start] == 0 || seen[start] {
                    continue;
                }
                objects += 1;
                let mut stack = vec![start];
                seen[start] = true;
                let mut area = 0;
                while let Some(index) = stack.pop() {
                    area += 1;
                    let x = index % 3;
                    let y = index / 3;
                    for yy in y.saturating_sub(1)..=(y + 1).min(2) {
                        for xx in x.saturating_sub(1)..=(x + 1).min(2) {
                            let next = yy * 3 + xx;
                            if pixels[next] != 0 && !seen[next] {
                                seen[next] = true;
                                stack.push(next);
                            }
                        }
                    }
                }
                maximum = maximum.max(area);
            }
            assert_eq!(scene.rows[0].objects, objects, "mask {bits}");
            assert_eq!(
                scene.rows[0].largest_area,
                f32::from(u16::try_from(maximum).unwrap()),
                "mask {bits}"
            );
        }
    }
    #[test]
    fn explicit_quantization_removes_only_values_in_the_zero_bin() {
        let mut scene = Scene::new();
        scene
            .push_layer_quantized(
                0.05,
                &[8, 9, 17],
                [3, 1],
                [3., 1.],
                &mut Cancellation::never(),
                8,
            )
            .unwrap();
        assert_eq!(scene.rows[0].objects, 1);
        assert_eq!(scene.rows[0].area, 2.);
        assert_eq!(scene.rows[0].bounds, [-0.5, -0.5, 1.5, 0.5]);
    }
    /// Independent flood fill: nonzero pixels, bounds, eight-connected components, largest one.
    fn flood(pixels: &[u8], width: usize, threshold: u8) -> (u64, [usize; 4], u32, u64) {
        let height = pixels.len() / width;
        let lit = |i: usize| pixels[i] > threshold;
        let mut seen = vec![false; pixels.len()];
        let (mut nonzero, mut objects, mut largest) = (0, 0, 0);
        let mut bounds = [width, height, 0, 0];
        for start in 0..pixels.len() {
            if !lit(start) || seen[start] {
                continue;
            }
            objects += 1;
            seen[start] = true;
            let mut stack = vec![start];
            let mut area = 0;
            while let Some(index) = stack.pop() {
                area += 1;
                let (x, y) = (index % width, index / width);
                bounds = [
                    bounds[0].min(x),
                    bounds[1].min(y),
                    bounds[2].max(x + 1),
                    bounds[3].max(y + 1),
                ];
                for yy in y.saturating_sub(1)..=(y + 1).min(height - 1) {
                    for xx in x.saturating_sub(1)..=(x + 1).min(width - 1) {
                        let next = yy * width + xx;
                        if lit(next) && !seen[next] {
                            seen[next] = true;
                            stack.push(next);
                        }
                    }
                }
            }
            nonzero += area;
            largest = largest.max(area);
        }
        (nonzero, bounds, objects, largest)
    }
    #[test]
    fn run_labelling_matches_flood_fill_on_random_masks() {
        let mut state = 0x2545_f491_4f6c_dd1d_u64;
        let mut next = |m: u64| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            usize::try_from(state % m).unwrap()
        };
        for case in 0..600u64 {
            let width = [1, 3, 7, 8, 9, 16, 31, 64, 67][usize::try_from(case % 9).unwrap()];
            let height = 1 + next(24);
            let density = 1 + next(100);
            // Long uniform stretches and isolated values, around the thresholds.
            let mut pixels = vec![0u8; width * height];
            let mut value = 0u8;
            for pixel in &mut pixels {
                if next(100) < 30 {
                    value = if next(100) < density {
                        [255, 9, 8, 200, 201, 1][next(6)]
                    } else {
                        0
                    };
                }
                *pixel = value;
            }
            for threshold in [0, 8, 200] {
                let resolution = [
                    u32::try_from(width).unwrap(),
                    u32::try_from(height).unwrap(),
                ];
                let metrics = layer_metrics(
                    &SpanMask::from_pixels(resolution[0], resolution[1], &pixels).unwrap(),
                    resolution,
                    threshold,
                    &mut Cancellation::never(),
                )
                .unwrap();
                let (nonzero, bounds, objects, largest) = flood(&pixels, width, threshold);
                assert_eq!(
                    (metrics.nonzero, metrics.objects, metrics.largest),
                    (nonzero, objects, largest),
                    "case {case} threshold {threshold}"
                );
                if nonzero > 0 {
                    assert_eq!(metrics.bounds, bounds, "case {case} threshold {threshold}");
                }
            }
        }
    }
    #[test]
    fn empty_layer_does_not_invent_objects() {
        let scene = inspect(&[0; 6], [3, 2]);
        assert_eq!(scene.rows[0].objects, 0);
        assert_eq!(scene.rows[0].area, 0.);
        assert!(scene.bounds.is_none());
    }
}
