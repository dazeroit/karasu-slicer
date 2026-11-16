//! Supports planned under the islands (owner mockup "Islands tool", revised 2026-10-29): Support
//! the islands previews a support under every island without one, and Support it here adds the
//! chosen island's at once, by the same rules. The plan itself is headless
//! ([`karasu_project::support_plan::islands`]); this is the application's side of it.
//!
//! **Off the interface thread.** A plan is made on a thread of its own against the selected
//! model's surface for routes (kept by the Supports tool's state while the Islands tool is in
//! use), with a preview of the kept supports built against small triangles of the surface at
//! their anchors, as the placing preview is.
use super::{holder, Island};
use crate::{
    preparation_ui::centre,
    supports_ui::{
        route::{RouteSurface, Routed},
        NextSupport,
    },
    workspace::Workspace,
};
use karasu_core::{
    mesh::TriangleMesh,
    supports::{BuildVolume, Preset, SupportDocument},
};
pub use karasu_project::support_plan::islands::{
    add_planned, alone, plan, IslandSupports, Outcome, Skip, Tip, BRANCH_MM, CONTACT_MIN_MM,
    DEFAULT_ISLAND_SUPPORTS, JOIN_MM, LEVELS, MERGE_MM, PER_GROUP, PER_TRUNK, SKIP_UNDER_MM2,
};
#[cfg(test)]
pub use karasu_project::support_plan::islands::{start_tips, START_MM};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc,
};

pub(crate) mod check;

/// The kept supports of `outcome` built against small triangles of the surface at their
/// anchors, in world coordinates, for the preview: the mesh and its centre.
pub fn preview(outcome: &Outcome, volume: BuildVolume) -> Option<(TriangleMesh, [f64; 3])> {
    let planned: Vec<(&Routed, Option<(usize, f64)>)> = outcome
        .supports
        .iter()
        .map(|p| (&p.routed, p.joins))
        .collect();
    let scene = alone(&planned, volume)?;
    let built = scene.mesh();
    let mesh = TriangleMesh::try_new(built.vertices().to_vec(), built.triangles().to_vec()).ok()?;
    if mesh.triangles().is_empty() {
        return None;
    }
    let origin = centre(&mesh);
    Some((mesh, origin))
}

/// Add `outcome`'s supports to model `index`, in its model frame, as one undoable step.
/// Returns the identities they took.
pub fn apply(workspace: &mut Workspace, index: usize, outcome: &Outcome) -> Vec<u64> {
    let Some(to_model) = workspace.model_to_world(index).and_then(|m| m.inverse()) else {
        return Vec::new();
    };
    let mut document = workspace
        .supports(index)
        .cloned()
        .unwrap_or_else(|| SupportDocument::new(Preset::Medium));
    let ids = add_planned(&mut document, &to_model, outcome);
    if !ids.is_empty() {
        workspace.set_supports(index, Some(document), None);
    }
    ids
}

/// The islands of `islands` (all, or only `only`) that Support the islands plans for on model
/// `index`: without a support, and (all of them) not smaller than the skip area.
pub fn wanted(
    workspace: &Workspace,
    index: usize,
    islands: &[Island],
    only: Option<usize>,
    skip_under_mm2: f64,
    join_mm: f64,
) -> Vec<(usize, Island)> {
    islands
        .iter()
        .copied()
        .enumerate()
        .filter(|&(k, island)| match only {
            Some(one) => k == one,
            None => island.area_mm2 >= skip_under_mm2,
        })
        .filter(|(_, island)| holder(workspace, index, island, join_mm).is_none())
        .collect()
}

/// A plan's preview: the kept supports' mesh and its centre.
pub type Preview = Option<(TriangleMesh, [f64; 3])>;

/// A plan running on its own thread, and what it is of.
pub struct Job<K> {
    pub key: K,
    pub stop: Arc<AtomicBool>,
    pub receiver: mpsc::Receiver<(Outcome, Preview)>,
}

impl<K> Drop for Job<K> {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Start planning supports under `islands` on its own thread.
#[allow(clippy::too_many_arguments)] // The plan's whole context.
pub fn start<K>(
    key: K,
    islands: Vec<(usize, Island)>,
    surface: Arc<RouteSurface>,
    next: NextSupport,
    document: Option<SupportDocument>,
    values: IslandSupports,
    volume: BuildVolume,
    checked: Option<(check::ModelInput, crate::gizmo::Affine)>,
) -> Option<Job<K>> {
    let stop = Arc::new(AtomicBool::new(false));
    let cancelled = Arc::clone(&stop);
    let (sender, receiver) = mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("island-supports".to_owned())
        .spawn(move || {
            let started = std::time::Instant::now();
            let outcome = plan(
                &islands,
                &surface,
                &next,
                document.as_ref(),
                &values,
                volume,
                checked.as_ref().map(|(model, to_model)| (model, to_model)),
                &cancelled,
            );
            let mesh = preview(&outcome, volume);
            log::debug!(
                target: "supports",
                "planned {} supports for {} islands ({} skipped) in {:.0} ms",
                outcome.supports.len(),
                outcome.held,
                outcome.skipped.len(),
                started.elapsed().as_secs_f64() * 1000.0
            );
            let _ = sender.send((outcome, mesh));
        });
    spawned.ok().map(|_| Job {
        key,
        stop,
        receiver,
    })
}
