//! The Drain holes tool's list window (owner, 2026-10-29): the selected model's holes, applied
//! then new, in a tool window opened from the tool strip (see `crate::list_window`). A row says
//! the hole's size and state, why it is refused or what it does in its tooltip; the panel keeps
//! the selected hole's values.
use super::{default_depth, merge_key, panel::notes, HoleRef, HollowingUi};
use crate::{
    appearance::Appearance,
    i18n::{decimal, tr},
    list_window::{self, Event, Row, Spec},
    preparation_ui::PreparationUi,
    theme,
    workspace::Workspace,
};
use eframe::egui;
use karasu_project::request::DrainHole;

/// How much room a hole is given when the view is brought to it, around its diameter, in mm.
const AROUND_MM: f64 = 4.0;

/// A hole of the selected model, as the list window and the panel show it.
pub(super) struct Listed {
    pub which: HoleRef,
    pub hole: DrainHole,
    /// Why it cannot be cut, if it cannot.
    pub refusal: Option<String>,
    /// What it does that deserves attention.
    pub notes: Vec<String>,
    /// A new hole whose check has not come back yet.
    pub unchecked: bool,
    /// Its number among the applied holes, or among the new ones.
    pub number: usize,
}

impl Listed {
    /// A hole no longer there (the list changed since its rows were chosen): an empty row.
    fn gone(which: HoleRef) -> Self {
        Self {
            which,
            hole: DrainHole::Axis {
                entry_mm: [0.0; 3],
                direction: [0.0, 0.0, 1.0],
                length_mm: 0.0,
                diameter_mm: 0.0,
            },
            refusal: None,
            notes: Vec::new(),
            unchecked: true,
            number: 0,
        }
    }

    /// Its name: "Hole 3", or "New hole 1".
    pub(super) fn name(&self) -> String {
        match self.which {
            HoleRef::Applied(_) => tr!("prepare-hole", number = self.number),
            HoleRef::New(_) => tr!("prepare-new-hole", number = self.number),
        }
    }

    /// Its tooltip: why it is refused and what it does, or what a click on its row does.
    fn hint(&self) -> String {
        let lines: Vec<&str> = self
            .refusal
            .iter()
            .map(String::as_str)
            .chain(self.notes.iter().map(String::as_str))
            .collect();
        if lines.is_empty() {
            tr!("list-row-hint")
        } else {
            lines.join("\n")
        }
    }
}

/// Hole `l`'s row: its size, its state, its dot in the colour the scene draws it (`chosen`: it is
/// the selected one); an automatic depth shows `auto_depth`.
fn hole_row(l: &Listed, chosen: bool, appearance: &Appearance, auto_depth: f64) -> Row {
    let new = matches!(l.which, HoleRef::New(_));
    let (state, state_colour) = if l.refusal.is_some() {
        (tr!("holes-state-refused"), theme::ERROR.color())
    } else if l.unchecked {
        (tr!("holes-state-checking"), theme::TEXT_PRIMARY.color())
    } else if new {
        (tr!("holes-state-new"), theme::ACCENT.color())
    } else if !l.notes.is_empty() {
        // Its notes are said, not shown by the amber alone (TD-142).
        (tr!("holes-state-ok-notes"), theme::WARNING.color())
    } else {
        (tr!("list-ok"), theme::TEXT_PRIMARY.color())
    };
    let dot = if chosen {
        appearance.hole_selected.color()
    } else if l.refusal.is_some() {
        theme::ERROR.color()
    } else if l.unchecked {
        appearance.hole_unchecked.color()
    } else if new {
        appearance.hole_valid.color()
    } else {
        appearance.drain_hole.color()
    };
    let depth = match l.hole {
        DrainHole::Surface {
            depth_mm: Some(d), ..
        } => tr!("holes-list-depth", depth = decimal(d, 1)),
        DrainHole::Surface { depth_mm: None, .. } => {
            tr!("holes-list-auto-depth", depth = decimal(auto_depth, 1))
        }
        DrainHole::Axis { length_mm, .. } => tr!("holes-list-depth", depth = decimal(length_mm, 1)),
    };
    Row {
        dot,
        name: if new {
            tr!("holes-list-new", number = l.number)
        } else {
            tr!("list-number", number = l.number)
        },
        fact: tr!(
            "holes-list-fact",
            diameter = decimal(l.hole.diameter_mm(), 1),
            depth = depth
        ),
        state,
        state_colour,
        hint: l.hint(),
        selected: chosen,
    }
}

impl HollowingUi {
    /// Model `index`'s holes, applied then new, with their verdicts.
    pub(super) fn listed(
        &self,
        workspace: &Workspace,
        preparation: &mut PreparationUi,
        index: Option<usize>,
    ) -> Vec<Listed> {
        let Some(object) = index else {
            return Vec::new();
        };
        let verdicts: Vec<(Option<String>, Vec<String>)> =
            preparation.verdicts(workspace, object).map_or_default(|v| {
                v.iter()
                    .map(|v| {
                        (
                            v.refusal.clone(),
                            v.findings.as_ref().map_or_else(Vec::new, notes),
                        )
                    })
                    .collect()
            });
        let applied = workspace
            .holes(object)
            .into_iter()
            .enumerate()
            .map(|(k, hole)| {
                let (refusal, notes) = verdicts.get(k).cloned().unwrap_or_default();
                Listed {
                    which: HoleRef::Applied(k),
                    hole,
                    refusal,
                    notes,
                    unchecked: false,
                    number: k + 1,
                }
            });
        let new = self
            .pending
            .iter()
            .enumerate()
            .filter(|(_, p)| p.object == object)
            .enumerate()
            .map(|(n, (position, p))| {
                let verdict = p.verdict.as_ref().map(|(_, v)| v);
                Listed {
                    which: HoleRef::New(position),
                    hole: p.hole,
                    refusal: verdict.and_then(|v| v.refusal.clone()),
                    notes: verdict
                        .and_then(|v| v.findings.as_ref())
                        .map_or_else(Vec::new, notes),
                    unchecked: verdict.is_none(),
                    number: n + 1,
                }
            });
        applied.chain(new).collect()
    }

    /// Delete hole `which` of model `object`: an applied one as one undoable step, a new one
    /// from the new holes.
    pub(super) fn delete_hole(&mut self, workspace: &mut Workspace, object: usize, which: HoleRef) {
        match which {
            HoleRef::Applied(k) => {
                workspace.remove_hole(object, k);
                self.selected = None;
            }
            HoleRef::New(position) => self.drop_new(position),
        }
    }

    /// Change hole `which` of model `object` to `hole`: an applied one as a step merged with
    /// the next changes of the same field, a new one checked again.
    pub(super) fn change_hole(
        &mut self,
        workspace: &mut Workspace,
        preparation: &PreparationUi,
        object: usize,
        which: HoleRef,
        hole: DrainHole,
    ) {
        match which {
            HoleRef::Applied(k) => {
                let key = merge_key(object, 16 + k as u64);
                workspace.set_hole(object, k, hole, Some(key));
            }
            HoleRef::New(position) => {
                self.pending[position].hole = hole;
                self.recheck(workspace, preparation, position);
            }
        }
    }

    /// The list window of model `index`'s holes, while open. Returns where the view should go,
    /// after a double click on a row.
    pub fn holes_window(
        &mut self,
        ctx: &egui::Context,
        workspace: &mut Workspace,
        preparation: &mut PreparationUi,
        index: Option<usize>,
        appearance: &Appearance,
    ) -> Option<[[f64; 3]; 2]> {
        if !self.holes_list_open {
            return None;
        }
        let index = index.filter(|&i| i < workspace.plate().objects.len());
        // The suction cups are the window's second tab (owner mockup, 2026-10-31).
        if self.cups_listed {
            self.cups_window(ctx, workspace, index, appearance);
            return None;
        }
        let listed = self.listed(workspace, preparation, index);
        let refused = listed.iter().filter(|l| l.refusal.is_some()).count();
        let items: Vec<HoleRef> = listed
            .iter()
            .filter(|l| !self.only_refused || l.refusal.is_some())
            .map(|l| l.which)
            .collect();
        let selected = self
            .selected
            .filter(|&(object, _)| Some(object) == index)
            .map(|(_, which)| which);
        let reveal = std::mem::take(&mut self.reveal_hole)
            .then_some(selected)
            .flatten();
        let auto_depth = default_depth(index.and_then(|i| workspace.plate().objects.get(i)));
        let spec = Spec {
            id: "holes-list",
            title: tr!("holes-list-title", count = listed.len()),
            fact: tr!("holes-list-size"),
            state: tr!("list-state"),
            filters: vec![tr!("list-all"), tr!("holes-list-refused", count = refused)],
            summary: if index.is_some() {
                tr!(
                    "holes-list-summary",
                    count = listed.len(),
                    refused = refused
                )
            } else {
                tr!("panel-no-model")
            },
            tabs: vec![tr!("cups-tab-holes"), tr!("cups-tab-cups")],
            places: Vec::new(),
            remove: Some(tr!("prepare-hole-remove")),
        };
        let row = |which: HoleRef| match listed.iter().find(|l| l.which == which) {
            Some(l) => hole_row(l, Some(which) == selected, appearance, auto_depth),
            None => hole_row(&Listed::gone(which), false, appearance, auto_depth),
        };
        let mut tab = 0;
        let mut filter = usize::from(self.only_refused);
        let event = list_window::show(
            ctx,
            &spec,
            &mut self.holes_list_open,
            &mut tab,
            &mut filter,
            &mut 0,
            &items,
            reveal,
            row,
        );
        self.only_refused = filter == 1;
        self.cups_listed = tab == 1;
        let object = index?;
        match event? {
            Event::Picked(which) => {
                self.selected = Some((object, which));
                None
            }
            Event::Focused(which) => {
                self.selected = Some((object, which));
                let hole = listed.iter().find(|l| l.which == which)?.hole;
                let point = match hole {
                    DrainHole::Surface { surface_mm, .. } => surface_mm,
                    DrainHole::Axis { entry_mm, .. } => entry_mm,
                };
                let at = workspace.model_to_world(object)?.point(point);
                let half = hole.diameter_mm() / 2.0 + AROUND_MM;
                Some([at.map(|c| c - half), at.map(|c| c + half)])
            }
            Event::Removed(which) => {
                self.delete_hole(workspace, object, which);
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use karasu_core::preparation::sampled::HoleSurface;

    fn listed(which: HoleRef, refusal: Option<&str>, unchecked: bool) -> Listed {
        Listed {
            which,
            hole: DrainHole::Surface {
                surface_mm: [0.0; 3],
                diameter_mm: 3.0,
                outside_mm: 1.0,
                inside_mm: 1.0,
                tilt: None,
                depth_mm: None,
                surface: HoleSurface::Outer,
            },
            refusal: refusal.map(str::to_owned),
            notes: Vec::new(),
            unchecked,
            number: 2,
        }
    }

    #[test]
    fn a_holes_row_says_its_size_and_whether_it_is_refused_new_or_checked() {
        let appearance = Appearance::default();
        let applied = hole_row(
            &listed(HoleRef::Applied(1), None, false),
            false,
            &appearance,
            4.5,
        );
        assert_eq!(applied.name, tr!("list-number", number = 2_usize));
        assert_eq!(applied.state, tr!("list-ok"));
        assert_eq!(applied.dot, appearance.drain_hole.color());
        assert!(applied.fact.contains(&decimal(4.5, 1)), "{}", applied.fact);
        let refused = hole_row(
            &listed(HoleRef::Applied(1), Some("It misses the cavity"), false),
            false,
            &appearance,
            4.5,
        );
        assert_eq!(refused.state, tr!("holes-state-refused"));
        assert_eq!(refused.dot, theme::ERROR.color());
        assert_eq!(refused.hint, "It misses the cavity");
        let checking = hole_row(&listed(HoleRef::New(0), None, true), true, &appearance, 4.5);
        assert_eq!(checking.name, tr!("holes-list-new", number = 2_usize));
        assert_eq!(checking.state, tr!("holes-state-checking"));
        assert_eq!(checking.dot, appearance.hole_selected.color());
        let new = hole_row(
            &listed(HoleRef::New(0), None, false),
            false,
            &appearance,
            4.5,
        );
        assert_eq!(new.state, tr!("holes-state-new"));
        assert_eq!(new.dot, appearance.hole_valid.color());
    }
}
