//! The Supports tool's trees (owner, 2026-10-30): "Merge trunks" merges the selected model's
//! supports standing on their own, placed by hand or by the Islands tool, as the Islands tool
//! merges its own ([`super::trees`]); "Join a support" takes the selected support and joins it
//! to the shaft clicked next, the same way.
//!
//! **Merge trunks** is a mode: the merge is worked out on a thread of its own against the
//! model's supports as built and its surface for routes, the supports it would change are drawn
//! in the selection's colour, and the footer says how many would merge, with Apply (one undoable
//! step) and Discard. It uses the merge values of the Islands tool's supports (distance, trunks a
//! group, levels).
//!
//! **Join a support** needs a support standing on its own: its shaft keeps going down to a bend
//! below what hangs from it, then leans at most 45° to the shaft clicked, clear of the model; a
//! shaft it cannot reach that way, or one standing on it, is said and nothing changes.
use super::{
    trees::{self, join_onto, MergeValues, Merged, Room, Stem},
    SupportsUi,
};
use crate::{
    camera::Ray,
    i18n::tr,
    messages::{Level, Message},
    workspace::Workspace,
};
use karasu_core::supports::{BaseSpec, SupportDocument, SupportScene};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc,
};

/// A merge worked out, or being worked out, for a model's supports at a plate revision.
pub(super) struct Merging {
    object: usize,
    revision: u64,
    stop: Arc<AtomicBool>,
    receiver: Option<mpsc::Receiver<Merged>>,
    /// The merge, and its supports' identities in the document's order.
    done: Option<(Merged, Vec<u64>)>,
}

impl Drop for Merging {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// The stems of `document`'s supports as built in `scene`, in the document's order: their
/// shafts, what they stand on and the braces on them.
fn stems(document: &SupportDocument, scene: &SupportScene) -> Vec<Stem> {
    let place = |id: u64| document.supports.iter().position(|s| s.id == id);
    document
        .supports
        .iter()
        .map(|s| {
            let joins = match s.base {
                BaseSpec::Support { parent, z_mm } => place(parent).map(|p| (p, z_mm)),
                _ => None,
            };
            Stem {
                shaft: scene
                    .shaft_path(s.id)
                    .map(<[[f64; 3]]>::to_vec)
                    .unwrap_or_default(),
                shaft_mm: s.parameters.shaft_diameter_mm,
                // A mini support is too thin to merge with trunks.
                free: !matches!(s.base, BaseSpec::Support { .. })
                    && s.kind == karasu_core::supports::SupportKind::Standard,
                joins,
                braced: document
                    .braces
                    .iter()
                    .flat_map(|b| [b.from, b.to])
                    .filter(|(id, _)| *id == s.id)
                    .map(|(_, z)| z)
                    .collect(),
            }
        })
        .collect()
}

/// `document` with `merged` done: every merged support bent and joined to its main trunk (by
/// identity, `ids` in the document's order), every main trunk's foot widened; `to_model`
/// carries the bends into the model's frame.
fn merged_into(
    document: &mut SupportDocument,
    merged: &Merged,
    ids: &[u64],
    to_model: &crate::gizmo::Affine,
) {
    let Some(to_world) = to_model.inverse() else {
        return;
    };
    for m in &merged.merges {
        let (Some(&id), Some(&parent)) = (ids.get(m.stem), ids.get(m.joins.0)) else {
            continue;
        };
        if let Some(s) = document.supports.iter_mut().find(|s| s.id == id) {
            trees::end_at(&mut s.bends, to_model.point(m.bend), |b| {
                to_world.point(b)[2]
            });
            s.base = BaseSpec::Support {
                parent,
                z_mm: m.joins.1,
            };
        }
    }
    for &(k, foot) in &merged.wider {
        let Some(&id) = ids.get(k) else {
            continue;
        };
        if let Some(s) = document.supports.iter_mut().find(|s| s.id == id) {
            let p = &mut s.parameters;
            p.shaft_bottom_diameter_mm = Some(p.shaft_bottom_diameter_mm.unwrap_or(0.0).max(foot));
        }
    }
}

impl SupportsUi {
    /// The room merging keeps on model `index`: its surface for routes, the floor and the
    /// clearance; `None` while the surface is not gridded.
    fn room<'a>(&'a self, workspace: &'a Workspace, index: usize) -> Option<Room<'a>> {
        let surface = self.current_route(workspace, index)?;
        let document = workspace.supports(index);
        Some(Room {
            mesh: surface.mesh()?,
            surface,
            floor: document
                .and_then(|d| d.raft)
                .map_or(0.0, |r| r.thickness_mm),
            clearance: document.map_or(0.3, |d| d.clearance_mm),
        })
    }

    /// Whether Merge trunks shows a merge.
    #[must_use]
    pub fn merging(&self) -> bool {
        self.merging.is_some()
    }

    /// Show the merge of the trunks, or leave it without changing anything.
    pub fn toggle_merging(&mut self, index: Option<usize>) {
        if self.merging.take().is_some() {
            return;
        }
        let Some(object) = index else {
            return;
        };
        self.placing = false;
        self.joining = None;
        self.forget_placing();
        self.merging = Some(Merging {
            object,
            revision: u64::MAX,
            stop: Arc::new(AtomicBool::new(false)),
            receiver: None,
            done: None,
        });
    }

    /// Keep the merge current while it shows: start it once the model's supports are built and
    /// its surface gridded, again when the plate changes, and take it when done. Returns
    /// whether a repaint is needed soon.
    pub(super) fn poll_merging(
        &mut self,
        workspace: &Workspace,
        preparation: &crate::preparation_ui::PreparationUi,
    ) -> bool {
        let Some(merging) = &self.merging else {
            return false;
        };
        let object = merging.object;
        if let Some(receiver) = &merging.receiver {
            let taken = receiver.try_recv();
            let merging = self.merging.as_mut().expect("merging");
            return match taken {
                Ok(merged) => {
                    let ids = workspace
                        .supports(object)
                        .map(|d| d.supports.iter().map(|s| s.id).collect())
                        .unwrap_or_default();
                    merging.done = Some((merged, ids));
                    merging.receiver = None;
                    true
                }
                Err(mpsc::TryRecvError::Empty) => true,
                Err(mpsc::TryRecvError::Disconnected) => {
                    merging.receiver = None;
                    false
                }
            };
        }
        if merging.revision == workspace.revision() {
            return false;
        }
        // The supports built as they are now, and the surface gridded.
        if !self.current(workspace, preparation, object) {
            return true;
        }
        let (Some(document), Some(scene), Some(surface)) = (
            workspace.supports(object),
            self.scene(object),
            self.route_surface(workspace, preparation, object),
        ) else {
            return true;
        };
        let stems = stems(document, scene);
        let floor = document.raft.map_or(0.0, |r| r.thickness_mm);
        let clearance = document.clearance_mm;
        let values = MergeValues {
            merge_mm: self.islands.merge_mm,
            per_group: self.islands.per_group,
            levels: self.islands.levels,
            aside_mm: trees::aside_for(self.islands.branch_mm),
        };
        let stop = Arc::new(AtomicBool::new(false));
        let cancelled = Arc::clone(&stop);
        let (sender, receiver) = mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("merge-trunks".to_owned())
            .spawn(move || {
                let started = std::time::Instant::now();
                let merged = surface.mesh().map_or_else(Merged::default, |mesh| {
                    let room = Room {
                        surface: &surface,
                        mesh,
                        floor,
                        clearance,
                    };
                    trees::merge(&stems, values, &room, &cancelled)
                });
                log::debug!(
                    target: "supports",
                    "merged {} of {} trunks in {:.0} ms",
                    merged.merges.len(),
                    stems.len(),
                    started.elapsed().as_secs_f64() * 1000.0
                );
                let _ = sender.send(merged);
            });
        let merging = self.merging.as_mut().expect("merging");
        merging.revision = workspace.revision();
        merging.stop = stop;
        merging.receiver = spawned.ok().map(|_| receiver);
        merging.done = None;
        true
    }

    /// The merge for model `index`, when done for its supports as they are: how many trunks
    /// would merge, and into how many main trunks.
    pub(super) fn merge_counts(&self, index: Option<usize>) -> Option<(usize, usize)> {
        let merging = self.merging.as_ref().filter(|m| Some(m.object) == index)?;
        let (merged, _) = merging.done.as_ref()?;
        let mut mains: Vec<usize> = merged.merges.iter().map(|m| m.joins.0).collect();
        mains.sort_unstable();
        mains.dedup();
        Some((merged.merges.len(), mains.len()))
    }

    /// Whether support `id` of model `index` is one the merge shown would change.
    pub(super) fn merge_marks(&self, index: usize, id: u64) -> bool {
        self.merging
            .as_ref()
            .filter(|m| m.object == index)
            .and_then(|m| m.done.as_ref())
            .is_some_and(|(merged, ids)| {
                let place = ids.iter().position(|&i| i == id);
                place.is_some_and(|k| merged.merges.iter().any(|m| m.stem == k || m.joins.0 == k))
            })
    }

    /// Apply: the merge shown, as one undoable step; the mode ends. Returns how many trunks
    /// merged.
    pub(super) fn apply_merging(&mut self, workspace: &mut Workspace, index: usize) -> usize {
        let Some(mut merging) = self.merging.take().filter(|m| m.object == index) else {
            return 0;
        };
        let current = merging.revision == workspace.revision();
        let (Some((merged, ids)), true) = (merging.done.take(), current) else {
            return 0;
        };
        let (Some(mut document), Some(to_model)) = (
            workspace.supports(index).cloned(),
            workspace.model_to_world(index).and_then(|m| m.inverse()),
        ) else {
            return 0;
        };
        if merged.merges.is_empty() {
            return 0;
        }
        merged_into(&mut document, &merged, &ids, &to_model);
        workspace.set_supports(index, Some(document), None);
        let count = merged.merges.len();
        self.outbox.push(Message::new(
            Level::Success,
            tr!("supports-merged", count = count),
        ));
        count
    }

    /// Whether Join a support waits for the shaft the selected support joins.
    #[must_use]
    pub fn joining(&self) -> bool {
        self.joining.is_some()
    }

    /// Join a support: the selected support of model `index` joins the shaft clicked next; again,
    /// it stops. A support already on another is said and waits for nothing.
    pub fn toggle_joining(&mut self, workspace: &Workspace, index: Option<usize>) {
        if self.joining.take().is_some() {
            return;
        }
        let Some((object, id)) = self.selected.filter(|&(o, _)| Some(o) == index) else {
            return;
        };
        let on_another = workspace
            .supports(object)
            .and_then(|d| d.supports.iter().find(|s| s.id == id))
            .is_some_and(|s| matches!(s.base, BaseSpec::Support { .. }));
        if on_another {
            self.outbox
                .push(Message::new(Level::Warning, tr!("supports-join-already")));
            return;
        }
        self.placing = false;
        self.merging = None;
        self.forget_placing();
        self.join_refused = false;
        self.joining = Some((object, id));
    }

    /// A click along `ray` on model `index` while a support waits to join a shaft: on a shaft it
    /// can reach, it bends and joins it, as one undoable step. Returns whether the click was
    /// taken.
    pub(super) fn join_click(
        &mut self,
        workspace: &mut Workspace,
        ray: &Ray,
        index: usize,
    ) -> bool {
        let Some((object, id)) = self.joining.filter(|&(o, _)| o == index) else {
            return false;
        };
        let Some((target, _)) = self.shaft_under(ray, index) else {
            return true;
        };
        let (Some(document), Some(scene)) = (workspace.supports(object), self.scene(object)) else {
            return true;
        };
        // Not onto itself, nor onto what stands on it.
        if super::branches::with_branches(document, &[id]).contains(&target) {
            self.join_refused = true;
            return true;
        }
        let place = |id: u64| document.supports.iter().position(|s| s.id == id);
        let (Some(other), Some(main)) = (place(id), place(target)) else {
            return true;
        };
        let stems = stems(document, scene);
        let joins: Vec<_> = stems.iter().map(|s| s.joins).collect();
        let found = self
            .room(workspace, object)
            .and_then(|room| join_onto(&stems, &joins, other, main, &room, None));
        let (Some((bend, z)), Some(to_model)) = (
            found,
            workspace.model_to_world(object).and_then(|m| m.inverse()),
        ) else {
            self.join_refused = true;
            return true;
        };
        let mut document = document.clone();
        let Some(to_world) = to_model.inverse() else {
            return true;
        };
        if let Some(s) = document.supports.iter_mut().find(|s| s.id == id) {
            trees::end_at(&mut s.bends, to_model.point(bend), |b| to_world.point(b)[2]);
            s.base = BaseSpec::Support {
                parent: target,
                z_mm: z,
            };
        }
        workspace.set_supports(object, Some(document), None);
        self.joining = None;
        self.join_refused = false;
        true
    }
}

#[cfg(test)]
mod tests;
