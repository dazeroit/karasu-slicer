//! On-demand verification of the printed walls (TD-095): the sliced masks, with the drain holes'
//! cuts and the expected cavities rasterized alone, read by [`karasu_core::raster::verify`].

use karasu_core::{
    cancellation::{Cancellation, Checkpoint},
    mesh::TriangleMesh,
    raster::{
        mask::SpanMask,
        parallel::{with_parallel_raster, LayerFill, ParallelRaster},
        rasterize_rings_with_cancellation,
        verify::{verify_walls, VerifyInput, VerifyLayer, VerifyLimits, VoxelAt, WallReport},
        Compensation, LcdProfile, RasterConfig,
    },
    slicing::{winding::WindingRule, SectionTransform},
};
use std::fmt::{self, Write};

/// The wall a verification compares against when no object is hollowed, in mm: the interface's
/// thin-wall warning (TD-093).
pub const UNHOLLOWED_WALL_MM: f64 = 1.0;

/// The walls a job asked for and certified.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Walls {
    /// The thinnest requested wall of the hollowed objects as placed (scaled with them), or
    /// [`UNHOLLOWED_WALL_MM`] when none is hollowed.
    pub requested_mm: f64,
    /// The thinnest certified wall of the hollowed objects as placed, or `None`.
    pub certified_mm: Option<f64>,
}

/// What the verification of a sliced job found (TD-095).
#[derive(Clone, Debug, PartialEq)]
pub struct Verification {
    /// The verifier's findings.
    pub report: WallReport,
    /// The walls compared against.
    pub walls: Walls,
    /// How far a printed wall may fall under the certified one on the print lattice: one voxel
    /// diagonal, in mm.
    pub tolerance_mm: f64,
    /// Why the printed layers contradict the certificate, when they do.
    pub failure: Option<String>,
    profile: LcdProfile,
    heights: Vec<f64>,
}

impl Verification {
    /// The world position of a voxel's centre, in mm.
    #[must_use]
    pub fn world(&self, at: VoxelAt) -> [f64; 3] {
        let [x, y] = self
            .profile
            .pixel_to_world(at.pixel.map(|v| f64::from(v) + 0.5))
            .unwrap_or([f64::NAN; 2]);
        [
            x,
            y,
            self.heights.get(at.layer).copied().unwrap_or(f64::NAN),
        ]
    }
}

/// What the verification reads from the job.
pub(super) struct Job<'a> {
    /// The exported masks, in layer order.
    pub masks: &'a [SpanMask],
    /// The print's raster specification: schedule, profile, sampling and limits.
    pub spec: &'a ParallelRaster<'a>,
    /// The expected cavities, in world coordinates, when any object is hollowed.
    pub cavity: Option<&'a TriangleMesh>,
    pub walls: Walls,
}

/// Verify `job`'s printed walls.
///
/// # Errors
/// A message when the cavities or the holes cannot be rasterized, or the verifier fails.
pub(super) fn verify(
    job: &Job<'_>,
    stop: &(dyn Fn(Checkpoint) -> bool + Sync),
) -> Result<Verification, String> {
    let spec = job.spec;
    let started = std::time::Instant::now();
    // The holes and cavities are compared with the material, without the print's compensation.
    let plain = RasterConfig {
        compensation: Compensation::None,
        ..spec.raster
    };
    let planes = spec.schedule.planes();
    let heights: Vec<f64> = match spec.schedule.slabs() {
        Some(slabs) => slabs.iter().map(|s| s.height_mm).collect(),
        None => planes.iter().map(|p| p.nominal_height_mm).collect(),
    };
    let cavities = job
        .cavity
        .map(|mesh| cavity_masks(spec, mesh, plain, stop))
        .transpose()?;
    let mut predicate = |checkpoint| stop(checkpoint);
    let mut cancel = Cancellation::new(&mut predicate);
    let holes = match spec.cut {
        Some(cut) => planes
            .iter()
            .map(|&plane| {
                let rings = cut.rings(plane);
                (!rings.is_empty())
                    .then(|| {
                        rasterize_rings_with_cancellation(
                            &rings,
                            plane,
                            spec.profile,
                            plain,
                            spec.limits,
                            &mut cancel,
                        )
                        .map(|image| image.mask().clone())
                    })
                    .transpose()
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("cannot rasterize the drain holes for wall verification: {e}"))?,
        None => vec![None; planes.len()],
    };
    let layers: Vec<VerifyLayer<'_>> = (0..planes.len())
        .map(|l| VerifyLayer {
            z_mm: planes[l].z_mm,
            height_mm: heights[l],
            print: &job.masks[l],
            holes: holes[l].as_ref(),
            cavity: cavities.as_ref().map(|c| &c[l]),
        })
        .collect();
    let world = spec.profile.pitch_mm();
    let pitch = if spec.profile.orientation().swap_xy {
        [world[1], world[0]]
    } else {
        world
    };
    let minimum = spec.raster.minimum_gray;
    let input = VerifyInput {
        layers: &layers,
        pitch_mm: pitch,
        // Half of the samples covered (TD-095).
        material_gray: minimum + (255 - minimum).div_ceil(2),
        wall_mm: job.walls.requested_mm,
    };
    let report = verify_walls(&input, VerifyLimits::default(), &mut cancel)
        .map_err(|e| format!("cannot verify the printed walls: {e}"))?;
    let tallest = heights.iter().copied().fold(0.0, f64::max);
    let tolerance_mm = (pitch[0] * pitch[0] + pitch[1] * pitch[1] + tallest * tallest).sqrt();
    let mut verification = Verification {
        report,
        walls: job.walls,
        tolerance_mm,
        failure: None,
        profile: spec.profile,
        heights: planes.iter().map(|p| p.z_mm).collect(),
    };
    verification.failure = failure(&verification);
    record(&verification, started.elapsed().as_secs_f64());
    Ok(verification)
}

/// The expected cavities' masks: `mesh` rasterized alone on the print's planes and profile, with
/// the nonzero rule and `plain` sampling.
fn cavity_masks(
    spec: &ParallelRaster<'_>,
    mesh: &TriangleMesh,
    plain: RasterConfig,
    stop: &(dyn Fn(Checkpoint) -> bool + Sync),
) -> Result<Vec<SpanMask>, String> {
    let cavity = ParallelRaster {
        mesh,
        schedule: spec.schedule,
        transform: SectionTransform::Identity,
        layer_config: spec.layer_config,
        represented_coordinates: spec.represented_coordinates,
        face_index_bytes: spec.face_index_bytes,
        vertex_avoiding_bytes: spec.vertex_avoiding_bytes,
        profile: spec.profile,
        raster: plain,
        bottom: None,
        limits: spec.limits,
        cut: None,
        kept_faces: &[],
        fill: LayerFill::Winding(WindingRule::NonZero),
        closure: None,
        threads: spec.threads,
        window_bytes: spec.window_bytes,
    };
    with_parallel_raster(&cavity, stop, |masks| {
        masks
            .map(|item| item.map(|image| image.mask().clone()))
            .collect::<Result<Vec<_>, _>>()
    })
    .map_err(|e| format!("cannot rasterize the cavities for wall verification: {e}"))
}

/// Record a verification's findings in the session log (TD-096).
fn record(v: &Verification, seconds: f64) {
    let report = &v.report;
    log::info!(
        target: "verify",
        "printed wall {}, {} breached voxels, {} undrained pocket(s), in {seconds:.2} s",
        report.thinnest.map_or_else(
            || "not within reach".to_owned(),
            |t| format!("{:.4} mm (at least {:.4} mm)", t.measured_mm, t.guaranteed_mm)
        ),
        report.breaches.voxels,
        report.pockets.len()
    );
    if let Some(reason) = &v.failure {
        log::warn!(target: "verify", "verification failed: {reason}");
    }
}

/// Why the printed layers contradict the certificate: a breach, or a wall under the certified
/// one by more than the lattice tolerance.
fn failure(v: &Verification) -> Option<String> {
    let report = &v.report;
    if let Some(first) = report.breaches.first {
        let [x, y, z] = v.world(first);
        return Some(format!(
            "the cavity reaches the outside other than through a drain hole: {} voxels \
             ({:.4} mm^3), the first at layer {} (x {x:.3}, y {y:.3}, z {z:.3} mm)",
            report.breaches.voxels, report.breaches.volume_mm3, first.layer
        ));
    }
    let (certified, thinnest) = (v.walls.certified_mm?, report.thinnest?);
    (thinnest.measured_mm < certified - v.tolerance_mm).then(|| {
        let [x, y, z] = v.world(thinnest.at);
        format!(
            "the printed wall is {:.4} mm at layer {} (x {x:.3}, y {y:.3}, z {z:.3} mm), under the \
             certified {certified:.4} mm by more than one voxel ({:.4} mm)",
            thinnest.measured_mm, thinnest.at.layer, v.tolerance_mm
        )
    })
}

/// The summary lines of a verification.
pub(super) fn write(out: &mut impl Write, found: &Verification) -> fmt::Result {
    let report = &found.report;
    match report.thinnest {
        Some(thinnest) => {
            let [x, y, z] = found.world(thinnest.at);
            writeln!(
                out,
                "Printed wall (mm): {} measured, at least {} guaranteed",
                thinnest.measured_mm, thinnest.guaranteed_mm
            )?;
            writeln!(
                out,
                "Thinnest printed wall at: layer {} (x {x:.3}, y {y:.3}, z {z:.3} mm)",
                thinnest.at.layer
            )?;
        }
        None if report.cavity_voxels == 0 => writeln!(out, "Printed wall (mm): no cavity")?,
        None => writeln!(
            out,
            "Printed wall (mm): more than {} everywhere",
            found.walls.requested_mm
        )?,
    }
    writeln!(
        out,
        "Printed wall thinner than {} mm (cavity voxels): {}",
        found.walls.requested_mm, report.thin_voxels
    )?;
    match report.breaches.first {
        Some(first) => {
            let [x, y, z] = found.world(first);
            writeln!(
                out,
                "Wall breaches: {} voxels ({} mm^3), the first at layer {} (x {x:.3}, y {y:.3}, \
                 z {z:.3} mm)",
                report.breaches.voxels, report.breaches.volume_mm3, first.layer
            )?;
        }
        None => writeln!(out, "Wall breaches: none")?,
    }
    match report.pockets.first() {
        Some(lowest) => {
            let [x, y, z] = found.world(lowest.first);
            let volume: f64 = report.pockets.iter().map(|p| p.volume_mm3).sum();
            writeln!(
                out,
                "Undrained pockets: {} ({volume} mm^3), the lowest at layer {} (x {x:.3}, \
                 y {y:.3}, z {z:.3} mm)",
                report.pockets.len(),
                lowest.first.layer
            )?;
        }
        None => writeln!(out, "Undrained pockets: none")?,
    }
    match &found.failure {
        Some(reason) => writeln!(out, "Wall verification: failed, {reason}"),
        None => writeln!(out, "Wall verification: passed"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use karasu_core::raster::{
        verify::{Breaches, ThinnestWall},
        Orientation,
    };

    fn verification(breaches: Breaches, thinnest: Option<ThinnestWall>) -> Verification {
        Verification {
            report: WallReport {
                requested_mm: 2.0,
                thinnest,
                thin_voxels: 0,
                cavity_voxels: 10,
                breaches,
                pockets: Vec::new(),
                marks: Vec::new(),
                marks_dropped: 0,
            },
            walls: Walls {
                requested_mm: 2.0,
                certified_mm: Some(2.05),
            },
            tolerance_mm: 0.06,
            failure: None,
            profile: LcdProfile::try_new(
                [100, 50],
                [0.0, 0.0],
                [100.0, 50.0],
                Orientation {
                    swap_xy: false,
                    flip_x: false,
                    flip_y: false,
                },
            )
            .unwrap(),
            heights: vec![0.025],
        }
    }

    #[test]
    fn a_breach_or_a_wall_under_the_certificate_fails_the_verification() {
        let at = VoxelAt {
            layer: 0,
            pixel: [3, 4],
        };
        let wall = |measured_mm| {
            Some(ThinnestWall {
                distance_mm: measured_mm + 0.03,
                measured_mm,
                guaranteed_mm: measured_mm - 0.03,
                at,
            })
        };
        // Within one voxel of the certified wall: passed.
        assert_eq!(failure(&verification(Breaches::default(), wall(2.0))), None);
        // More than one voxel under it: failed, where.
        let thin = failure(&verification(Breaches::default(), wall(1.9))).unwrap();
        assert!(
            thin.contains("1.9000 mm at layer 0 (x 3.500, y 4.500"),
            "{thin}"
        );
        // Any breach fails, first.
        let breaches = Breaches {
            voxels: 7,
            volume_mm3: 0.5,
            first: Some(at),
        };
        let message = failure(&verification(breaches, wall(2.0))).unwrap();
        assert!(message.contains("7 voxels"), "{message}");
        // No certified wall: only breaches can fail.
        let mut unhollowed = verification(Breaches::default(), wall(0.1));
        unhollowed.walls.certified_mm = None;
        assert_eq!(failure(&unhollowed), None);
    }
}
