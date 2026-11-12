//! On-demand suction cups of the print (TD-114): the sliced masks read by
//! [`karasu_core::analysis::suction`], with where each cup lies in the world.
//!
//! The masks are the exported ones, supports and raft included: the raft or the plate can cap a
//! cup, and only openings left in the print vent it. The analysis never fails the slice: it
//! reports.

use karasu_core::{
    analysis::suction::{
        find_suction_cups, CupCap, CupInput, CupLayer, CupReport, Opening, SuctionCup,
    },
    cancellation::{Cancellation, Checkpoint},
    raster::{mask::SpanMask, parallel::ParallelRaster, LcdProfile},
};
use std::fmt::{self, Write};

/// Cups listed at most in the summary, the largest first; the totals count every one.
const LISTED: usize = 20;

/// The print's suction cups, with how to place them in the world.
#[derive(Clone, Debug)]
pub struct SuctionCups {
    /// What the analysis found.
    pub report: CupReport,
    /// The smallest opening that vents a cup, in mm².
    pub min_vent_mm2: f64,
    /// The area of one pixel, in mm².
    pub pixel_mm2: f64,
    profile: LcdProfile,
    planes: Vec<f64>,
    heights: Vec<f64>,
}

impl SuctionCups {
    /// Where `cup` is capped, in mm: its first layer's centroid, at the bottom of that layer,
    /// where a vent would relieve it.
    #[must_use]
    pub fn cap_world(&self, cup: &SuctionCup) -> [f64; 3] {
        let [x, y] = self
            .profile
            .pixel_to_world(cup.cap)
            .unwrap_or([f64::NAN; 2]);
        let layer = cup.first_layer;
        let z = match (self.planes.get(layer), self.heights.get(layer)) {
            (Some(plane), Some(height)) => plane - height / 2.0,
            _ => f64::NAN,
        };
        [x, y, z]
    }

    /// Where `cap` starts, in mm: its first layer's centroid, at the bottom of that layer.
    #[must_use]
    pub fn cap_point(&self, cap: &CupCap) -> [f64; 3] {
        let [x, y] = self
            .profile
            .pixel_to_world(cap.cap)
            .unwrap_or([f64::NAN; 2]);
        [x, y, self.slab(cap.layer).map_or(f64::NAN, |s| s[0])]
    }

    /// Where a vent opens into `cap`'s part wide enough, in mm: the centroid of where the
    /// vent's circle first fits in it, at the bottom of that layer.
    #[must_use]
    pub fn vent_point(&self, cap: &CupCap) -> [f64; 3] {
        let [x, y] = self
            .profile
            .pixel_to_world(cap.vent)
            .unwrap_or([f64::NAN; 2]);
        [x, y, self.slab(cap.vent_layer).map_or(f64::NAN, |s| s[0])]
    }

    /// Where `cap`'s first layer lies on the plate: its pixel bounds' lowest and highest world
    /// corners, in mm.
    #[must_use]
    pub fn cap_bounds_mm(&self, cap: &CupCap) -> [[f64; 2]; 2] {
        self.pixel_bounds_mm(cap.bounds)
    }

    /// Where `cup` lies on the plate, over its layers: its pixel bounds' lowest and highest
    /// world corners, in mm.
    #[must_use]
    pub fn bounds_mm(&self, cup: &SuctionCup) -> [[f64; 2]; 2] {
        self.pixel_bounds_mm(cup.bounds)
    }

    /// Pixel bounds' lowest and highest world corners, in mm.
    fn pixel_bounds_mm(&self, bounds: [u32; 4]) -> [[f64; 2]; 2] {
        let [c0, r0, c1, r1] = bounds.map(f64::from);
        let corners = [[c0, r0], [c1, r0], [c0, r1], [c1, r1]]
            .map(|p| self.profile.pixel_to_world(p).unwrap_or([f64::NAN; 2]));
        let low = [0, 1].map(|k| corners.iter().map(|c| c[k]).fold(f64::INFINITY, f64::min));
        let high = [0, 1].map(|k| {
            corners
                .iter()
                .map(|c| c[k])
                .fold(f64::NEG_INFINITY, f64::max)
        });
        [low, high]
    }

    /// The height of layer `layer`'s plane, in mm.
    #[must_use]
    pub fn height(&self, layer: usize) -> Option<f64> {
        self.planes.get(layer).copied()
    }

    /// The bottom and top of layer `layer`'s slab, in mm.
    #[must_use]
    pub fn slab(&self, layer: usize) -> Option<[f64; 2]> {
        let (plane, height) = (self.planes.get(layer)?, self.heights.get(layer)?);
        Some([plane - height / 2.0, plane + height / 2.0])
    }

    /// The area of `cup` at its widest layer, in mm²: what the film pulls on.
    #[must_use]
    pub fn largest_area_mm2(&self, cup: &SuctionCup) -> f64 {
        #[allow(clippy::cast_precision_loss)] // Pixel counts.
        let pixels = cup.largest_pixels as f64;
        pixels * self.pixel_mm2
    }
}

/// Find the suction cups of the exported `masks`, with `min_vent_mm2` the smallest vent.
///
/// # Errors
/// A message when the analysis cannot run (inconsistent layers, too many pieces, cancellation).
pub(super) fn find(
    masks: &[SpanMask],
    spec: &ParallelRaster<'_>,
    min_vent_mm2: f64,
    stop: &(dyn Fn(Checkpoint) -> bool + Sync),
) -> Result<SuctionCups, String> {
    let started = std::time::Instant::now();
    let planes = spec.schedule.planes();
    let heights: Vec<f64> = match spec.schedule.slabs() {
        Some(slabs) => slabs.iter().map(|s| s.height_mm).collect(),
        None => planes.iter().map(|p| p.nominal_height_mm).collect(),
    };
    let layers: Vec<CupLayer<'_>> = planes
        .iter()
        .zip(&heights)
        .zip(masks)
        .map(|((plane, &height_mm), mask)| CupLayer {
            z_mm: plane.z_mm,
            height_mm,
            mask,
        })
        .collect();
    let world = spec.profile.pitch_mm();
    let pitch = if spec.profile.orientation().swap_xy {
        [world[1], world[0]]
    } else {
        world
    };
    let input = CupInput {
        layers: &layers,
        pitch_mm: pitch,
        // A pixel cured at all seals: no opening is credited that the print would not leave.
        material_gray: 1,
        min_vent_mm2,
    };
    let mut predicate = |checkpoint| stop(checkpoint);
    let report = find_suction_cups(&input, &mut Cancellation::new(&mut predicate))
        .map_err(|e| format!("cannot find the suction cups: {e}"))?;
    let found = SuctionCups {
        report,
        min_vent_mm2,
        pixel_mm2: pitch[0] * pitch[1],
        profile: spec.profile,
        planes: planes.iter().map(|p| p.z_mm).collect(),
        heights,
    };
    log::info!(
        target: "suction",
        "{} suction cup(s) with vents of at least {min_vent_mm2} mm^2, in {:.2} s",
        found.report.cups.len(),
        started.elapsed().as_secs_f64()
    );
    Ok(found)
}

/// The summary's lines: the count, then the cups, the largest first.
pub(super) fn write(out: &mut impl Write, found: &SuctionCups) -> fmt::Result {
    let cups = &found.report.cups;
    let largest = cups
        .iter()
        .map(|c| found.largest_area_mm2(c))
        // From +0: an empty fold of floats would print as "-0.000".
        .fold(0.0, f64::max);
    writeln!(
        out,
        "Suction cups (vents of at least {} mm^2): {}, the largest {largest:.3} mm^2",
        found.min_vent_mm2,
        cups.len()
    )?;
    let mut listed: Vec<&SuctionCup> = cups.iter().collect();
    listed.sort_by_key(|c| std::cmp::Reverse(c.largest_pixels));
    for cup in listed.iter().take(LISTED) {
        let [x, y, z] = found.cap_world(cup);
        writeln!(
            out,
            "Suction cup: layers {}-{} (cap at x {x:.3}, y {y:.3}, z {z:.3} mm), {:.3} mm^2, \
             {:.3} mm^3, {}, {}, {} cap(s)",
            cup.first_layer,
            cup.last_layer,
            found.largest_area_mm2(cup),
            cup.volume_mm3,
            match cup.opening {
                Opening::None => "no opening",
                Opening::TooSmall => "openings too small",
            },
            if cup.closes {
                "closes over trapped resin"
            } else {
                "then open"
            },
            cup.caps.len()
        )?;
        // A cup's other separately capped parts each need their own vent.
        for cap in cup.caps.iter().skip(1) {
            let [x, y, z] = found.cap_point(cap);
            #[allow(clippy::cast_precision_loss)] // Pixel counts.
            let area = cap.largest_pixels as f64 * found.pixel_mm2;
            writeln!(
                out,
                "Suction cup part: layers {}-{} (cap at x {x:.3}, y {y:.3}, z {z:.3} mm), \
                 {area:.3} mm^2, {:.3} mm^3",
                cap.layer, cap.last_layer, cap.volume_mm3
            )?;
        }
    }
    if cups.len() > LISTED {
        writeln!(out, "Suction cups not listed: {}", cups.len() - LISTED)?;
    }
    Ok(())
}
