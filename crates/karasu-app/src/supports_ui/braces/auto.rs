//! The automatic braces (owner mockup, 2026-10-29): planned for the selected model on a thread
//! of their own whenever its supports, its placement, its preparation or the panel's values
//! change, the previous plan stopped; shown, those kept and those skipped, until Apply adds the
//! kept ones as one undoable step, or Discard.
use super::super::{centre, support_draw, translation, Input, SupportsUi};
use crate::{
    appearance::Appearance,
    i18n::tr,
    messages::{Level, Message},
    preparation_ui::PreparationUi,
    viewport::{Draw, Viewport},
    workspace::Workspace,
};
use eframe::egui_wgpu;
use karasu_core::{
    mesh::TriangleMesh,
    supports::{brace_mesh, BracePlan, PlannedBrace, Skipped},
};
use karasu_project::pipeline::plan_model_braces;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    time::Instant,
};

/// The viewport mesh keys of the kept braces and of those skipped near the model or a hole.
const KEPT_KEY: u64 = 0x4252_4143_0000_0002; // "BRAC"
const SKIPPED_KEY: u64 = 0x4252_4143_0000_0003;

/// What a plan is of: the model, its supports as they are built, and the plan.
#[derive(Clone, Debug, PartialEq)]
struct Key {
    object: usize,
    input: Input,
    plan: BracePlan,
}

/// A plan running on its own thread.
struct Job {
    key: Key,
    stop: Arc<AtomicBool>,
    receiver: mpsc::Receiver<Result<Vec<PlannedBrace>, String>>,
}

impl Drop for Job {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// A finished plan, and its kept and skipped braces' meshes.
struct Planned {
    key: Key,
    braces: Result<Vec<PlannedBrace>, String>,
    kept: Option<(TriangleMesh, [f64; 3])>,
    skipped: Option<(TriangleMesh, [f64; 3])>,
    uploaded: bool,
}

/// The automatic braces' state, while that mode is on.
#[derive(Default)]
pub(in crate::supports_ui) struct Auto {
    job: Option<Job>,
    planned: Option<Planned>,
}

/// What the footer says of a plan.
pub(in crate::supports_ui) enum Outcome {
    /// Nothing to plan: no model, or supports not built yet.
    Waiting,
    /// A plan running.
    Planning,
    /// A plan that could not be made, and why.
    Failed(String),
    /// How many braces would be added, and how many were skipped for each reason: the model,
    /// a drain hole, a pair already braced.
    Ready { kept: usize, skipped: [usize; 3] },
}

/// A mesh of `braces`' bars, each of its diameter, about its centre; `None` when there is none.
fn bars(braces: &[&PlannedBrace]) -> Option<(TriangleMesh, [f64; 3])> {
    if braces.is_empty() {
        return None;
    }
    let bars: Vec<([[f64; 3]; 2], f64)> =
        braces.iter().map(|p| (p.ends_mm, p.diameter_mm)).collect();
    let mesh = brace_mesh(&bars).ok()?;
    let origin = centre(&mesh);
    Some((mesh, origin))
}

impl Auto {
    /// The plan's draws: the kept braces in the valid colour, the skipped ones near the model or
    /// a hole in the error colour.
    pub(super) fn draws(&self, appearance: &Appearance) -> Vec<Draw> {
        let Some(planned) = &self.planned else {
            return Vec::new();
        };
        let mut draws = Vec::new();
        if let Some((_, origin)) = &planned.kept {
            draws.push(support_draw(
                KEPT_KEY,
                translation(*origin),
                appearance.hole_valid.linear(),
                None,
            ));
        }
        if let Some((_, origin)) = &planned.skipped {
            draws.push(support_draw(
                SKIPPED_KEY,
                translation(*origin),
                appearance.support_error.linear(),
                None,
            ));
        }
        draws
    }

    /// Upload a new plan's meshes once.
    pub(super) fn sync(&mut self, viewport: &mut Viewport, state: &egui_wgpu::RenderState) {
        let Some(planned) = self.planned.as_mut().filter(|p| !p.uploaded) else {
            return;
        };
        planned.uploaded = true;
        Self::retire(viewport);
        for (key, mesh) in [(KEPT_KEY, &planned.kept), (SKIPPED_KEY, &planned.skipped)] {
            if let Some((mesh, origin)) = mesh {
                viewport.upload(&state.device, key, mesh, *origin);
            }
        }
    }

    /// Free the plan's meshes.
    pub(super) fn retire(viewport: &mut Viewport) {
        viewport.remove(KEPT_KEY);
        viewport.remove(SKIPPED_KEY);
    }

    /// The plan for model `index` with `plan` as it stands: see [`Outcome`].
    pub(super) fn outcome(&self, index: Option<usize>) -> Outcome {
        let current = self
            .planned
            .as_ref()
            .filter(|p| Some(p.key.object) == index);
        if self.job.is_some() {
            return Outcome::Planning;
        }
        match current.map(|p| &p.braces) {
            None => Outcome::Waiting,
            Some(Err(reason)) => Outcome::Failed(reason.clone()),
            Some(Ok(braces)) => {
                let mut skipped = [0; 3];
                for p in braces {
                    match p.skipped {
                        Some(Skipped::Model) => skipped[0] += 1,
                        Some(Skipped::DrainHole) => skipped[1] += 1,
                        Some(Skipped::Joined) => skipped[2] += 1,
                        None => {}
                    }
                }
                Outcome::Ready {
                    kept: braces.iter().filter(|p| p.skipped.is_none()).count(),
                    skipped,
                }
            }
        }
    }
}

impl SupportsUi {
    /// Keep the automatic braces of model `selected` current: take a finished plan, stop an
    /// outdated one, start the next. Returns whether a repaint is needed soon.
    pub(in crate::supports_ui) fn poll_auto(
        &mut self,
        workspace: &Workspace,
        preparation: &PreparationUi,
        selected: Option<usize>,
    ) -> bool {
        let plan = self.bracing.new.plan();
        let Some(auto) = self.bracing.auto.as_mut() else {
            return false;
        };
        let Some(index) = selected else {
            auto.job = None;
            auto.planned = None;
            return false;
        };
        if let Some(job) = &auto.job {
            match job.receiver.try_recv() {
                Ok(braces) => {
                    let key = job.key.clone();
                    auto.job = None;
                    let (kept, skipped) = match &braces {
                        Ok(all) => {
                            let kept: Vec<&PlannedBrace> =
                                all.iter().filter(|p| p.skipped.is_none()).collect();
                            let near: Vec<&PlannedBrace> = all
                                .iter()
                                .filter(|p| {
                                    matches!(p.skipped, Some(Skipped::Model | Skipped::DrainHole))
                                })
                                .collect();
                            (bars(&kept), bars(&near))
                        }
                        Err(_) => (None, None),
                    };
                    auto.planned = Some(Planned {
                        key,
                        braces,
                        kept,
                        skipped,
                        uploaded: false,
                    });
                }
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => auto.job = None,
            }
        }
        let Some((input, shared)) = Self::input(workspace, preparation, index) else {
            auto.job = None;
            auto.planned = None;
            return false;
        };
        let key = Key {
            object: index,
            input,
            plan,
        };
        if auto.planned.as_ref().is_some_and(|p| p.key == key) {
            return auto.job.is_some();
        }
        if auto.job.as_ref().is_some_and(|j| j.key == key) {
            return true;
        }
        auto.job = start(workspace, key, shared);
        auto.job.is_some()
    }

    /// The automatic braces' state for model `index`.
    pub(in crate::supports_ui) fn auto_outcome(&self, index: Option<usize>) -> Option<Outcome> {
        self.bracing.auto.as_ref().map(|a| a.outcome(index))
    }

    /// Apply: add the kept braces of the current plan to model `index`, as one undoable step,
    /// and leave the mode. Returns how many were added.
    pub(in crate::supports_ui) fn apply_auto(
        &mut self,
        workspace: &mut Workspace,
        index: usize,
    ) -> usize {
        let Some(auto) = self.bracing.auto.take() else {
            return 0;
        };
        let Some(planned) = auto.planned.filter(|p| p.key.object == index) else {
            return 0;
        };
        // Planned for the document as it is: the new IDs are free.
        let Some(mut document) = workspace
            .supports(index)
            .filter(|d| **d == planned.key.input.document)
            .cloned()
        else {
            return 0;
        };
        let Ok(braces) = planned.braces else {
            return 0;
        };
        let kept: Vec<_> = braces
            .iter()
            .filter(|p| p.skipped.is_none())
            .map(|p| p.brace)
            .collect();
        if kept.is_empty() {
            return 0;
        }
        let count = kept.len();
        document.braces.extend(kept);
        workspace.set_supports(index, Some(document), None);
        self.outbox.push(Message::new(
            Level::Success,
            tr!("braces-auto-added", count = count),
        ));
        count
    }
}

/// Start planning `key` against its preparation in `shared`, on a thread of its own.
fn start(
    workspace: &Workspace,
    key: Key,
    shared: Arc<karasu_project::pipeline::Preparation>,
) -> Option<Job> {
    let index = key.object;
    let object = workspace.plate().objects.get(index)?;
    let mesh = Arc::clone(&workspace.meshes().get(&object.model)?.mesh);
    let stop = Arc::new(AtomicBool::new(false));
    let (sender, receiver) = mpsc::channel();
    let cancelled = Arc::clone(&stop);
    let document = key.input.document.clone();
    let placement = key.input.placement;
    let plan = key.plan;
    let spawned = std::thread::Builder::new()
        .name("braces".to_owned())
        .spawn(move || {
            let Some(Ok(prepared)) = shared.objects.get(index) else {
                let _ = sender.send(Err(tr!("supports-unprepared")));
                return;
            };
            let started = Instant::now();
            let result = plan_model_braces(&document, &mesh, &placement, prepared, &plan, &|_| {
                cancelled.load(Ordering::Relaxed)
            });
            if let Ok(braces) = &result {
                log::debug!(
                    target: "supports",
                    "planned {} braces for {} supports in {:.0} ms",
                    braces.len(),
                    document.supports.len(),
                    started.elapsed().as_secs_f64() * 1000.0
                );
            }
            let _ = sender.send(result);
        });
    match spawned {
        Ok(_) => Some(Job {
            key,
            stop,
            receiver,
        }),
        Err(error) => {
            log::warn!(target: "supports", "cannot start planning the braces: {error}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::tests::built;
    use super::*;
    use std::time::Duration;

    #[test]
    fn automatic_braces_are_planned_shown_and_applied_as_one_step() {
        let (mut workspace, preparation, directory, mut ui, _) =
            super::super::tests::two_supports();
        ui.toggle_auto_braces();
        assert!(ui.auto_bracing());
        ui.bracing.new.lowest_mm = 3.0;
        ui.bracing.new.step_mm = 2.0;
        ui.bracing.new.pattern = super::super::Pattern::Horizontal;
        let started = Instant::now();
        while ui.poll_auto(&workspace, &preparation, Some(0)) {
            assert!(started.elapsed() < Duration::from_secs(60), "no plan");
            std::thread::sleep(Duration::from_millis(10));
        }
        let Some(Outcome::Ready { kept, skipped }) = ui.auto_outcome(Some(0)) else {
            panic!("no plan");
        };
        // The shafts reach from about 2 mm to above 12 mm: levels 3, 5, 7, 9 and 11 mm.
        assert!(kept >= 4, "{kept} {skipped:?}");
        assert_eq!(skipped, [0, 0, 0]);
        assert_eq!(
            ui.brace_draws(&crate::appearance::Appearance::default())
                .len(),
            1
        );
        // A value changed plans again.
        ui.bracing.new.step_mm = 4.0;
        assert!(ui.poll_auto(&workspace, &preparation, Some(0)));
        while ui.poll_auto(&workspace, &preparation, Some(0)) {
            std::thread::sleep(Duration::from_millis(10));
        }
        let Some(Outcome::Ready { kept, .. }) = ui.auto_outcome(Some(0)) else {
            panic!("no plan");
        };
        // Applied: one step, the mode left, a notification.
        let revision = workspace.revision();
        assert_eq!(ui.apply_auto(&mut workspace, 0), kept);
        assert!(!ui.auto_bracing());
        assert_ne!(workspace.revision(), revision);
        assert_eq!(workspace.supports(0).unwrap().braces.len(), kept);
        assert_eq!(ui.take_messages().len(), 1);
        built(&mut ui, &workspace, &preparation);
        assert_eq!(ui.scene(0).unwrap().braces(), kept);
        assert!(!ui.scene(0).unwrap().has_errors());
        // Planned again, the pairs already braced there are skipped.
        ui.toggle_auto_braces();
        while ui.poll_auto(&workspace, &preparation, Some(0)) {
            std::thread::sleep(Duration::from_millis(10));
        }
        let Some(Outcome::Ready {
            kept: again,
            skipped,
        }) = ui.auto_outcome(Some(0))
        else {
            panic!("no plan");
        };
        assert_eq!((again, skipped[2]), (0, kept));
        workspace.undo();
        assert_eq!(workspace.supports(0).unwrap().braces.len(), 0);
        let _ = std::fs::remove_dir_all(directory);
    }
}
