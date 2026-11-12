//! On-demand unsupported regions of the print (TD-107, level 3): the sliced masks read by
//! [`karasu_core::analysis::unsupported`], with what each region means in the world.
//!
//! The masks are the exported ones: supports in the job are material, so a print whose supports
//! hold everything shows no unsupported island. The analysis never fails the slice: it reports.

use karasu_core::{
    analysis::unsupported::{
        find_unsupported, IslandStart, RegionKind, SupportInput, SupportLayer, SupportReport,
        UnsupportedRegion,
    },
    cancellation::{Cancellation, Checkpoint},
    raster::{mask::SpanMask, parallel::ParallelRaster, LcdProfile},
};
use std::fmt::{self, Write};

/// Regions listed at most in the summary; the totals count every one.
const LISTED: usize = 20;

/// The print's unsupported regions, with how to place them in the world.
#[derive(Clone, Debug)]
pub struct Unsupported {
    /// What the analysis found.
    pub report: SupportReport,
    /// The critical angle, in degrees from the plate's plane.
    pub angle_deg: f64,
    /// The area of one pixel, in mm².
    pub pixel_mm2: f64,
    profile: LcdProfile,
    heights: Vec<f64>,
}

impl Unsupported {
    /// The world position of `region`'s lowest point, in mm: its lowest layer's centroid.
    #[must_use]
    pub fn world(&self, region: &UnsupportedRegion) -> [f64; 3] {
        let [x, y] = self
            .profile
            .pixel_to_world(region.lowest)
            .unwrap_or([f64::NAN; 2]);
        [
            x,
            y,
            self.heights
                .get(region.first_layer)
                .copied()
                .unwrap_or(f64::NAN),
        ]
    }

    /// Where `region` lies on the plate, over its layers: its pixel bounds' lowest and highest
    /// world corners, in mm.
    #[must_use]
    pub fn bounds_mm(&self, region: &UnsupportedRegion) -> [[f64; 2]; 2] {
        self.pixel_bounds_mm(region.bounds)
    }

    /// The world position where `start` begins, in mm: its centroid, at its layer's height.
    #[must_use]
    pub fn start_world(&self, start: &IslandStart) -> [f64; 3] {
        let [x, y] = self
            .profile
            .pixel_to_world(start.centroid)
            .unwrap_or([f64::NAN; 2]);
        [
            x,
            y,
            self.heights.get(start.layer).copied().unwrap_or(f64::NAN),
        ]
    }

    /// Where `start` lies on the plate: its pixel bounds' lowest and highest world corners, in
    /// mm.
    #[must_use]
    pub fn start_bounds_mm(&self, start: &IslandStart) -> [[f64; 2]; 2] {
        self.pixel_bounds_mm(start.bounds)
    }

    /// The area of `start`'s pixels, in mm².
    #[must_use]
    pub fn start_area_mm2(&self, start: &IslandStart) -> f64 {
        #[allow(clippy::cast_precision_loss)] // Pixel counts.
        let pixels = start.pixels as f64;
        pixels * self.pixel_mm2
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
        self.heights.get(layer).copied()
    }

    /// The area of `region`'s pixels, over its layers, in mm².
    #[must_use]
    pub fn area_mm2(&self, region: &UnsupportedRegion) -> f64 {
        #[allow(clippy::cast_precision_loss)] // Pixel counts.
        let pixels = region.pixels as f64;
        pixels * self.pixel_mm2
    }
}

/// Find the unsupported regions of the exported `masks` for the critical angle `angle_deg`.
///
/// # Errors
/// A message when the analysis cannot run (inconsistent layers, too many parts, cancellation).
pub(super) fn find(
    masks: &[SpanMask],
    spec: &ParallelRaster<'_>,
    angle_deg: f64,
    stop: &(dyn Fn(Checkpoint) -> bool + Sync),
) -> Result<Unsupported, String> {
    let started = std::time::Instant::now();
    let planes = spec.schedule.planes();
    let layers: Vec<SupportLayer<'_>> = planes
        .iter()
        .zip(masks)
        .map(|(plane, mask)| SupportLayer {
            z_mm: plane.z_mm,
            mask,
        })
        .collect();
    let world = spec.profile.pitch_mm();
    let pitch = if spec.profile.orientation().swap_xy {
        [world[1], world[0]]
    } else {
        world
    };
    let minimum = spec.raster.minimum_gray;
    let input = SupportInput {
        layers: &layers,
        pitch_mm: pitch,
        angle_deg,
        // A pixel touched at all is material, so no island smaller than a pixel is lost; a
        // pixel half covered supports, as the wall verifier's material (TD-107).
        material_gray: 1,
        support_gray: minimum + (255 - minimum).div_ceil(2),
    };
    let mut predicate = |checkpoint| stop(checkpoint);
    let report = find_unsupported(&input, &mut Cancellation::new(&mut predicate))
        .map_err(|e| format!("cannot find the unsupported regions: {e}"))?;
    let found = Unsupported {
        report,
        angle_deg,
        pixel_mm2: pitch[0] * pitch[1],
        profile: spec.profile,
        heights: planes.iter().map(|p| p.z_mm).collect(),
    };
    log::info!(
        target: "supports",
        "{} unsupported island(s) and {} overhang region(s) at {angle_deg} degrees, in {:.2} s",
        found.report.islands(),
        found.report.regions.len() - found.report.islands(),
        started.elapsed().as_secs_f64()
    );
    Ok(found)
}

/// The summary's lines: the counts, then the regions, islands first, lowest first.
pub(super) fn write(out: &mut impl Write, found: &Unsupported) -> fmt::Result {
    let regions = &found.report.regions;
    let islands = found.report.islands();
    let area = |kind| -> f64 {
        regions
            .iter()
            .filter(|r| r.kind == kind)
            .map(|r| found.area_mm2(r))
            // From +0: an empty sum of floats is -0, which would print as "-0.000".
            .fold(0.0, |sum, area| sum + area)
    };
    writeln!(
        out,
        "Unsupported regions (critical angle {} degrees): {islands} island(s), {:.3} mm^2; {} \
         overhang(s), {:.3} mm^2",
        found.angle_deg,
        area(RegionKind::Island),
        regions.len() - islands,
        area(RegionKind::Overhang)
    )?;
    // Each piece starting in mid-air, though several may grow into one island (owner,
    // 2026-10-30): each needs a support where it starts.
    writeln!(
        out,
        "Island starts: {}",
        regions.iter().map(|r| r.starts.len()).sum::<usize>()
    )?;
    let listed = regions
        .iter()
        .filter(|r| r.kind == RegionKind::Island)
        .chain(regions.iter().filter(|r| r.kind == RegionKind::Overhang))
        .take(LISTED);
    for region in listed {
        let [x, y, z] = found.world(region);
        writeln!(
            out,
            "Unsupported {}: layers {}-{} (x {x:.3}, y {y:.3}, z {z:.3} mm), {:.4} mm^2",
            match region.kind {
                RegionKind::Island => "island",
                RegionKind::Overhang => "overhang",
            },
            region.first_layer,
            region.last_layer,
            found.area_mm2(region)
        )?;
    }
    if regions.len() > LISTED {
        writeln!(
            out,
            "Unsupported regions not listed: {}",
            regions.len() - LISTED
        )?;
    }
    Ok(())
}
