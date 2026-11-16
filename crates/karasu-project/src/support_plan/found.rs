//! What a model sliced alone needs held, as the planners take it (TD-107, TD-112): the
//! unsupported regions a slice finds on its layers ([`crate::pipeline::Unsupported`]), told
//! as [`Island`]s, and the islands among them, one where each piece starts in mid-air.
//!
//! On a hollowed model a region or an island whose underside is a cavity triangle starts on the
//! cavity's ceiling ([`in_cavity`]), the test that also places a support's tip there, so the
//! label and the support agree. The Islands tool, the Automatic tool and the command line's
//! automatic supports (TD-157) read a slice's regions through it alike.
use super::islands::{Island, PROBE_MM};
use crate::pipeline::{Preparation, Unsupported};
use karasu_core::{
    analysis::unsupported::RegionKind, cancellation::Cancellation, mesh::TriangleMesh,
    supports::SurfaceGrid,
};
use std::sync::atomic::{AtomicBool, Ordering};

/// Whether an island at `point`, its layers' top at `top`, starts on the cavity's ceiling of the
/// hollowed `shell` (gridded by `grid`, its cavity triangles from `cavity_from`): the surface
/// straight above the air just under it is a cavity triangle. The same test places a support's
/// tip there, so the label and the support agree.
#[must_use]
pub fn in_cavity(
    shell: &TriangleMesh,
    grid: &SurfaceGrid,
    cavity_from: usize,
    point: [f64; 3],
    top: f64,
) -> bool {
    grid.first_above(shell, point, top + PROBE_MM)
        .is_some_and(|(_, _, t)| t as usize >= cavity_from)
}

/// The islands among `found`'s unsupported regions, lowest first: see [`found_of`].
#[must_use]
pub fn islands_of(
    found: &Unsupported,
    shell: Option<&Preparation>,
    index: usize,
    stop: &AtomicBool,
) -> Vec<Island> {
    found_of(found, shell, index, stop).1
}

/// `found`'s unsupported regions with their kinds, each told as an [`Island`] is over all its
/// layers, and its islands, one where each piece starts in mid-air (owner, 2026-10-30: several
/// may grow into one region, each needing its support where it starts), lowest first. With
/// object `index`'s hollowed preparation in `shell`, a region or an island whose underside is a
/// cavity triangle starts on the cavity's ceiling ([`in_cavity`]); the shell is gridded once for
/// them all.
#[must_use]
pub fn found_of(
    found: &Unsupported,
    shell: Option<&Preparation>,
    index: usize,
    stop: &AtomicBool,
) -> (Vec<(RegionKind, Island)>, Vec<Island>) {
    let hollow = shell
        .and_then(|s| s.objects.get(index))
        .and_then(|o| o.as_ref().ok())
        .and_then(|o| o.hollowed());
    let gridded = hollow.and_then(|model| {
        let mut stop = |_| stop.load(Ordering::Relaxed);
        let grid = SurfaceGrid::build(model.mesh(), &mut Cancellation::new(&mut stop)).ok()?;
        Some((model.mesh(), grid, model.exterior_faces()))
    });
    let inside = |point: [f64; 3], top: f64| {
        gridded
            .as_ref()
            .is_some_and(|(mesh, grid, from)| in_cavity(mesh, grid, *from, point, top))
    };
    let under_layer = |layer: usize| {
        layer
            .checked_sub(1)
            .and_then(|k| found.height(k))
            .unwrap_or(0.0)
    };
    let mut regions = Vec::new();
    let mut islands = Vec::new();
    for r in &found.report.regions {
        let point = found.world(r);
        let under = under_layer(r.first_layer);
        let top = found.height(r.last_layer).unwrap_or(point[2]);
        regions.push((
            r.kind,
            Island {
                point,
                layers: [r.first_layer, r.last_layer],
                heights: [under, top],
                area_mm2: found.area_mm2(r),
                footprint: found.bounds_mm(r),
                inside: inside([point[0], point[1], under - 1e-3], top),
            },
        ));
        for start in &r.starts {
            let point = found.start_world(start);
            let under = under_layer(start.layer);
            islands.push(Island {
                point,
                layers: [start.layer, r.last_layer],
                heights: [under, top],
                area_mm2: found.start_area_mm2(start),
                footprint: found.start_bounds_mm(start),
                inside: inside([point[0], point[1], under - 1e-3], top),
            });
        }
    }
    islands.sort_by(|a, b| {
        a.layers[0]
            .cmp(&b.layers[0])
            .then(a.point[0].total_cmp(&b.point[0]))
            .then(a.point[1].total_cmp(&b.point[1]))
    });
    (regions, islands)
}
