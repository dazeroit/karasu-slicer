//! The Hollow and Drain holes tool panels (M6.5, TD-098), drawn from the owner-approved mockups.
//!
//! Each panel follows the tool panel rules (`docs/UI-DESIGN.md`, "Tool panels"): the body
//! scrolls and holds the tool's settings and the facts they need; a fixed footer holds the main
//! actions and the model's state, the most important thing first. Without a selected model the
//! structure is the same, its controls disabled and showing the tool's values. Messages are not
//! drawn here: what an applied change did comes as a toast once the model is prepared
//! ([`HollowingUi::take_messages`]).
use super::{
    default_depth, Awaiting, HollowingUi, Part, MIN_WALL_MM, SOLID_HOLE_DEPTH_MM, THIN_WALL_MM,
    TINY_REGION_MM3,
};
use crate::{
    i18n::{decimal, tr},
    icons::{Icon, Icons},
    messages::{Level, Message},
    preparation_ui::PreparationUi,
    theme,
    widgets::{
        action_pair, auto_default, auto_value_row, default_auto_row, default_row, fact_row,
        length_default, state_line, value_row, Unit, UnitField,
    },
    workspace::Workspace,
};
use eframe::egui;
use karasu_core::preparation::{
    sampled::{HoleFindings, HoleSurface, SampledHollowCertificate, SampledHollowedModel},
    HoleTilt, MAX_HOLE_TILT_DEG,
};
use karasu_project::{pipeline::ObjectPreparation, project::Hollowing, request::DrainHole};

mod blockers;
mod cups;
mod regions;

/// What the preparation says of an object now.
enum Prepared<'a> {
    /// No printer or resin: nothing is prepared.
    NeedsProfiles,
    /// Being prepared.
    Pending,
    /// Refused, with the reason.
    Failed(String),
    /// Prepared.
    Done(&'a ObjectPreparation),
}

/// The preparation of object `index` as the plate is now.
fn prepared<'a>(
    workspace: &Workspace,
    preparation: &'a PreparationUi,
    index: usize,
) -> Prepared<'a> {
    let plate = workspace.plate();
    if plate.setting().is_none() {
        return Prepared::NeedsProfiles;
    }
    match preparation
        .current(workspace)
        .and_then(|c| c.objects.get(index))
    {
        None => Prepared::Pending,
        Some(Err(error)) => Prepared::Failed(error.reason().to_owned()),
        Some(Ok(object)) => Prepared::Done(object),
    }
}

/// How the cavity of a prepared object drains.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Drainage {
    /// The object is not hollowed.
    NoCavity,
    /// Hollowed, without a hole.
    NoHole,
    /// This many cavity regions no hole opens, each of 1 mm³ or more.
    Undrained(usize),
    /// Every region drains apart from this many pockets under 1 mm³.
    Pockets(usize),
    /// Every cavity region drains.
    Drains,
}

fn drainage(object: &ObjectPreparation) -> Drainage {
    if object.hollowed().is_none() {
        return Drainage::NoCavity;
    }
    let Some(holes) = object.holes() else {
        return Drainage::NoHole;
    };
    let enclosed = holes.regions().iter().filter(|r| r.drained_by.is_empty());
    let large = enclosed
        .clone()
        .filter(|r| r.volume_mm3 >= TINY_REGION_MM3)
        .count();
    let pockets = enclosed.count() - large;
    match (large, pockets) {
        (0, 0) => Drainage::Drains,
        (0, pockets) => Drainage::Pockets(pockets),
        (large, _) => Drainage::Undrained(large),
    }
}

/// What a panel's footer says of the model: a line, its colour and icon, a detail for its
/// tooltip, and whether it leads to the Drain holes tool.
struct State {
    text: String,
    colour: egui::Color32,
    icon: Option<Icon>,
    detail: Option<String>,
    to_holes: bool,
}

impl State {
    /// A neutral state, without an icon.
    fn plain(text: String) -> Self {
        Self {
            text,
            colour: theme::BORDER.color(),
            icon: None,
            detail: None,
            to_holes: false,
        }
    }

    fn info(text: String) -> Self {
        Self {
            colour: theme::ACCENT.color(),
            icon: Some(Icon::Info),
            ..Self::plain(text)
        }
    }

    fn success(text: String) -> Self {
        Self {
            colour: theme::SUCCESS.color(),
            icon: Some(Icon::Success),
            ..Self::plain(text)
        }
    }

    fn warning(text: String) -> Self {
        Self {
            colour: theme::WARNING.color(),
            icon: Some(Icon::Warning),
            ..Self::plain(text)
        }
    }

    fn error(text: String) -> Self {
        Self {
            colour: theme::ERROR.color(),
            icon: Some(Icon::Error),
            ..Self::plain(text)
        }
    }

    fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    fn leads_to_holes(mut self) -> Self {
        self.to_holes = true;
        self
    }

    /// The state of a hollowed object's cavity that does not drain, or `None` when it does.
    fn undrained(drainage: Drainage) -> Option<Self> {
        match drainage {
            Drainage::NoCavity | Drainage::Drains => None,
            Drainage::NoHole => Some(Self::warning(tr!("prepare-no-drain-hole"))),
            Drainage::Undrained(count) => {
                Some(Self::warning(tr!("prepare-undrained", count = count)))
            }
            Drainage::Pockets(count) => {
                Some(Self::warning(tr!("prepare-tiny-regions", count = count)))
            }
        }
    }

    /// Draw it: the line, with its detail as a tooltip.
    fn show(&self, ui: &mut egui::Ui, icons: Option<&Icons>) {
        let response = state_line(ui, icons, self.icon, &self.text, self.colour);
        if let Some(detail) = &self.detail {
            response.on_hover_text(detail);
        }
    }
}

/// The state of a preparation that has not certified anything yet, or `None` once it has.
fn unprepared(prepared: &Prepared<'_>) -> Option<State> {
    match prepared {
        Prepared::NeedsProfiles => Some(State::warning(tr!("prepare-needs-profiles"))),
        Prepared::Pending => Some(State::plain(tr!("prepare-pending"))),
        Prepared::Failed(reason) => {
            Some(State::error(tr!("prepare-failed")).detail(reason.clone()))
        }
        Prepared::Done(_) => None,
    }
}

impl HollowingUi {
    /// The Advanced switch of `part`'s panel.
    pub fn advanced(&mut self, part: Part) -> &mut bool {
        match part {
            Part::Hollow => &mut self.hollow_advanced,
            Part::Holes => &mut self.holes_advanced,
        }
    }

    /// The body of `part`'s panel, for the selected object `index` if any (TD-098).
    #[allow(clippy::too_many_arguments)] // The plate, its preparation and the look.
    pub fn panel(
        &mut self,
        ui: &mut egui::Ui,
        workspace: &mut Workspace,
        preparation: &mut PreparationUi,
        index: Option<usize>,
        part: Part,
        icons: Option<&Icons>,
    ) {
        let index = index.filter(|&i| i < workspace.plate().objects.len());
        if let Some(index) = index {
            self.follow(workspace, preparation, index, part);
        }
        match part {
            Part::Hollow => self.hollow_body(ui, workspace, preparation, index),
            Part::Holes => self.holes_body(ui, workspace, preparation, index, icons),
        }
    }

    /// The footer of `part`'s panel: its actions and the model's state. Returns whether the
    /// Drain holes tool was asked for.
    pub fn footer(
        &mut self,
        ui: &mut egui::Ui,
        workspace: &mut Workspace,
        preparation: &mut PreparationUi,
        index: Option<usize>,
        part: Part,
        icons: Option<&Icons>,
    ) -> bool {
        let index = index.filter(|&i| i < workspace.plate().objects.len());
        match part {
            Part::Hollow => self.hollow_footer(ui, workspace, preparation, index, icons),
            Part::Holes => {
                self.holes_footer(ui, workspace, preparation, index, icons);
                false
            }
        }
    }

    /// Keep the holes' state with object `index` while `part`'s panel shows it: new holes of
    /// objects that moved places are dropped, a selected hole of another object released, and
    /// holes are placed, picked and dragged only with the Drain holes tool.
    fn follow(
        &mut self,
        workspace: &Workspace,
        preparation: &mut PreparationUi,
        index: usize,
        part: Part,
    ) {
        self.check_pending(workspace);
        if self
            .selected
            .is_some_and(|(o, which)| o != index || self.hole_of(workspace, o, which).is_none())
        {
            self.selected = None;
        }
        let hollowed = workspace
            .plate()
            .objects
            .get(index)
            .is_some_and(|o| o.hollowing().is_some());
        if !hollowed {
            self.start = HoleSurface::Outer;
        }
        if part != Part::Holes {
            self.close();
        }
        if part != Part::Hollow {
            self.blockers.close();
        }
        preparation.set_probes(self.probes(self.placing.then_some(index)));
    }

    /// The hollowing draft of object `index`, or the tool's hollowing without one.
    fn shown_hollowing(&mut self, workspace: &Workspace, index: Option<usize>) -> Hollowing {
        match index.and_then(|i| {
            workspace
                .plate()
                .objects
                .get(i)
                .map(|o| (i, o.hollowing().copied()))
        }) {
            Some((i, applied)) => self.draft(i, applied).edit,
            None => self.new_hollowing,
        }
    }

    /// The Hollow panel's body: the wall, the grid cell with Advanced, and what the
    /// preparation certified.
    fn hollow_body(
        &mut self,
        ui: &mut egui::Ui,
        workspace: &mut Workspace,
        preparation: &PreparationUi,
        index: Option<usize>,
    ) {
        let applied = index
            .and_then(|i| workspace.plate().objects.get(i))
            .map(|o| o.hollowing().copied());
        let before = self.shown_hollowing(workspace, index);
        let mut edit = before;
        let advanced = self.hollow_advanced;
        // The tool's values show their defaults' reset buttons (owner, 2026-11-02); a hollowed
        // model's own values are its.
        let tool = applied.flatten().is_none();
        let defaults = self.tool_defaults;
        ui.add_enabled_ui(index.is_some(), |ui| {
            crate::widgets::group_with_hint(
                ui,
                tr!("prepare-hollow"),
                tr!("prepare-hollow-hint"),
                |ui| {
                    let wall = tool.then(|| length_default(defaults.wall_mm, 2, Unit::Millimetre));
                    default_row(
                        ui,
                        tr!("prepare-wall"),
                        &mut edit.wall_mm,
                        wall.as_ref(),
                        |ui, wall| {
                            ui.add(
                                UnitField::new(wall, Unit::Millimetre)
                                    .range(MIN_WALL_MM..=20.0)
                                    .step(0.05),
                            )
                        },
                    );
                    if advanced {
                        let cell = tool.then(|| auto_default(defaults.cell_mm, Unit::Millimetre));
                        default_auto_row(
                            ui,
                            tr!("prepare-cell"),
                            &mut edit.cell_mm,
                            0.25,
                            cell.as_ref(),
                            |ui, cell| {
                                ui.add(
                                    UnitField::new(cell, Unit::Millimetre)
                                        .range(0.02..=1.0)
                                        .step(0.01),
                                )
                                .on_hover_text(tr!("prepare-cell-hint"))
                            },
                        );
                    }
                    // Only a project file asks for it now; the draft keeps it.
                    if edit.simplify.is_some() {
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(tr!("prepare-simplified"))
                                    .small()
                                    .color(ui.visuals().weak_text_color()),
                            )
                            .wrap(),
                        );
                    }
                },
            );
        });
        if let (Some(i), Some(applied)) = (index, applied) {
            if edit != before {
                self.draft(i, applied).edit = edit;
                // Editing a model never hollowed edits the tool's hollowing (design rule 6).
                if applied.is_none() {
                    self.new_hollowing = edit;
                }
            }
        }
        egui::CollapsingHeader::new(tr!("blockers-section"))
            .id_salt("hollow-blockers")
            .default_open(true)
            .show(ui, |ui| self.blockers_section(ui, workspace, index));
        ui.add_space(4.0);
        let certificate =
            index.filter(|_| applied.flatten().is_some()).and_then(|i| {
                match prepared(workspace, preparation, i) {
                    Prepared::Done(object) => {
                        object.hollowed().map(SampledHollowedModel::certificate)
                    }
                    _ => None,
                }
            });
        egui::CollapsingHeader::new(tr!("prepare-result"))
            .id_salt("hollow-result")
            .default_open(true)
            .show(ui, |ui| facts(ui, certificate.as_ref(), advanced));
    }

    /// The Hollow panel's footer: the draft's state with Apply hollowing and Remove hollowing,
    /// then the model's state. Returns whether Add drain holes was clicked.
    fn hollow_footer(
        &mut self,
        ui: &mut egui::Ui,
        workspace: &mut Workspace,
        preparation: &PreparationUi,
        index: Option<usize>,
        icons: Option<&Icons>,
    ) -> bool {
        let object = index.and_then(|i| {
            let o = workspace.plate().objects.get(i)?;
            Some((i, o.hollowing().copied()))
        });
        let edit = self.shown_hollowing(workspace, index);
        // New, changed or removed blockers are part of the draft (owner, 2026-11-02).
        let (blocked, unblocked) =
            object.map_or((0, false), |(i, _)| self.blockers.changes(workspace, i));
        let blockers_changed = blocked > 0 || unblocked;
        let (text, colour) = match object {
            None => ("–".to_owned(), theme::BORDER),
            Some((_, None)) => (tr!("prepare-hollow-none"), theme::BORDER),
            Some((_, Some(applied))) if applied == edit && !blockers_changed => {
                (tr!("prepare-hollow-applied"), theme::ACCENT)
            }
            Some((_, Some(applied))) if applied == edit => {
                (tr!("blockers-changed", count = blocked), theme::WARNING)
            }
            Some((_, Some(_))) => (tr!("prepare-hollow-changed"), theme::WARNING),
        };
        ui.add_enabled_ui(object.is_some(), |ui| {
            ui.add(
                egui::Label::new(egui::RichText::new(text).color(colour.color()).small()).wrap(),
            );
            let (apply, remove) = action_pair(
                ui,
                [tr!("prepare-apply-hollow"), tr!("prepare-remove-hollow")],
                (
                    object.is_some_and(|(_, applied)| applied != Some(edit) || blockers_changed),
                    object.is_some_and(|(_, applied)| applied.is_some()),
                ),
            );
            let remove = remove.on_hover_text(tr!("prepare-remove-hollow-hint"));
            if let Some((i, _)) = object {
                if apply.clicked() {
                    let blockers = self.blockers.edited(workspace, i);
                    workspace.hollow_with(i, Some(edit), blockers);
                    self.blockers.applied();
                    self.new_hollowing = edit;
                    self.awaiting = Some(Awaiting::Hollowed {
                        object: i,
                        revision: workspace.revision(),
                    });
                } else if remove.clicked() {
                    workspace.hollow_with(i, None, Vec::new());
                    self.blockers.applied();
                    self.outbox
                        .push(Message::new(Level::Info, tr!("prepare-done-removed")));
                }
            }
        });
        ui.add_space(4.0);
        let state = self.hollow_state(workspace, preparation, index);
        state.show(ui, icons);
        state.to_holes
            && ui
                .add_sized(
                    [ui.available_width(), ui.spacing().interact_size.y],
                    egui::Button::new(tr!("prepare-add-holes")).truncate(),
                )
                .on_hover_text(tr!("prepare-to-holes-hint"))
                .clicked()
    }

    /// The most important thing about object `index`'s hollowing: the draft's risks first,
    /// then whether it is hollowed, scaled, prepared and drained.
    fn hollow_state(
        &mut self,
        workspace: &Workspace,
        preparation: &PreparationUi,
        index: Option<usize>,
    ) -> State {
        let Some((i, object)) = index.and_then(|i| Some((i, workspace.plate().objects.get(i)?)))
        else {
            return State::plain("–".to_owned());
        };
        let applied = object.hollowing().copied();
        let scale = object.pose.scale;
        let edit = self.draft(i, applied).edit;
        if let Some(warning) = predicted_limit(workspace, i, &edit) {
            return State::error(warning);
        }
        if edit.wall_mm < THIN_WALL_MM {
            return State::warning(tr!("prepare-thin-wall"));
        }
        let Some(applied) = applied else {
            return State::plain(tr!("prepare-solid"));
        };
        // The cavity scales with the object (TD-089): say what the wall has become.
        let low = scale.iter().copied().fold(f64::INFINITY, f64::min);
        let high = scale.iter().copied().fold(0.0, f64::max);
        if (low - 1.0).abs() > 1e-9 || (high - 1.0).abs() > 1e-9 {
            return State::warning(tr!(
                "prepare-scaled-wall",
                min = decimal(applied.wall_mm * low, 2),
                max = decimal(applied.wall_mm * high, 2)
            ))
            .detail(tr!("prepare-scaled-wall-hint"));
        }
        let prepared = prepared(workspace, preparation, i);
        if let Some(state) = unprepared(&prepared) {
            return state;
        }
        let Prepared::Done(object) = prepared else {
            return State::plain(tr!("prepare-pending"));
        };
        let Some(hollow) = object.hollowed() else {
            return State::plain(tr!("prepare-solid"));
        };
        match drainage(object) {
            // Here the state leads to the tool that adds them.
            Drainage::NoHole => return State::warning(tr!("prepare-enclosed")).leads_to_holes(),
            drainage => {
                if let Some(state) = State::undrained(drainage) {
                    return state.leads_to_holes();
                }
            }
        }
        State::success(tr!(
            "prepare-hollowed",
            wall = decimal(hollow.certificate().surface_distance_mm[0], 2)
        ))
    }

    /// The Drain holes panel's body: the new holes' settings, the holes, and how the cavity
    /// drains.
    #[allow(clippy::too_many_lines)] // One panel, top to bottom.
    fn holes_body(
        &mut self,
        ui: &mut egui::Ui,
        workspace: &mut Workspace,
        preparation: &mut PreparationUi,
        index: Option<usize>,
        icons: Option<&Icons>,
    ) {
        let object = index.and_then(|i| workspace.plate().objects.get(i));
        let hollowed = object.is_some_and(|o| o.hollowing().is_some());
        let auto_depth = default_depth(object);
        // Known once the object is prepared: no hole can be cut, so none is placed (TD-094).
        let unavailable = index.and_then(|i| preparation.drainage_unavailable(workspace, i));
        if unavailable.is_some() {
            self.placing = false;
            self.candidate = None;
        }
        // A model without a cavity has no cavity to start from.
        if !hollowed {
            self.start = HoleSurface::Outer;
        }
        ui.add_enabled_ui(index.is_some(), |ui| {
            crate::widgets::group(ui, tr!("prepare-new-holes"), |ui| {
                // Placing and the surface are on the tool strip (owner, 2026-10-29).
                let defaults = self.tool_defaults;
                let diameter = length_default(defaults.hole_diameter_mm, 1, Unit::Millimetre);
                default_row(
                    ui,
                    tr!("hole-diameter"),
                    &mut self.hole_mm,
                    Some(&diameter),
                    |ui, diameter| {
                        ui.add(
                            UnitField::new(diameter, Unit::Millimetre)
                                .range(0.5..=20.0)
                                .step(0.05)
                                .decimals(1),
                        )
                    },
                );
                let depth = auto_default(defaults.hole_depth_mm, Unit::Millimetre);
                default_auto_row(
                    ui,
                    tr!("hole-depth"),
                    &mut self.depth_mm,
                    auto_depth,
                    Some(&depth),
                    |ui, depth| {
                        ui.add(
                            UnitField::new(depth, Unit::Millimetre)
                                .range(0.1..=100.0)
                                .step(0.05),
                        )
                        .on_hover_text(tr!("prepare-new-depth-hint"))
                    },
                );
            });
        });
        self.selected_hole(ui, workspace, preparation, index, auto_depth);
        egui::CollapsingHeader::new(tr!("prepare-drainage"))
            .id_salt("holes-drainage")
            .default_open(true)
            .show(ui, |ui| {
                self.drainage(ui, workspace, preparation, index, icons);
            });
        egui::CollapsingHeader::new(tr!("cups-section"))
            .id_salt("holes-cups")
            .default_open(true)
            .show(ui, |ui| {
                self.cups_section(ui, workspace, preparation, index, icons);
            });
    }

    /// The selected hole of model `index`: its state and its values, which change it alone (the
    /// list of holes is a window of its own, owner 2026-10-29). Without one, the same fields,
    /// disabled, with the next hole's values.
    fn selected_hole(
        &mut self,
        ui: &mut egui::Ui,
        workspace: &mut Workspace,
        preparation: &mut PreparationUi,
        index: Option<usize>,
        auto_depth: f64,
    ) {
        let chosen = index.and_then(|object| {
            let (o, which) = self.selected?;
            (o == object).then_some(which)
        });
        let listed = self.listed(workspace, preparation, index);
        let selected = chosen.and_then(|which| listed.into_iter().find(|l| l.which == which));
        let title = selected
            .as_ref()
            .map_or_else(|| tr!("holes-selected"), super::list::Listed::name);
        let shown = selected.as_ref().map_or(
            DrainHole::Surface {
                surface_mm: [0.0; 3],
                diameter_mm: self.hole_mm,
                outside_mm: 0.0,
                inside_mm: 0.0,
                tilt: None,
                depth_mm: self.depth_mm,
                surface: self.start,
            },
            |l| l.hole,
        );
        let mut edited = shown;
        let advanced = self.holes_advanced;
        crate::widgets::group(ui, title, |ui| {
            ui.add_enabled_ui(selected.is_some(), |ui| {
                let state = match &selected {
                    Some(l) if l.refusal.is_some() => tr!("holes-state-refused"),
                    Some(l) if l.unchecked => tr!("holes-state-checking"),
                    Some(_) => tr!("list-ok"),
                    None => "–".to_owned(),
                };
                let line = fact_row(ui, &tr!("list-state"), &state);
                if let Some(l) = &selected {
                    let hint: Vec<&str> = l
                        .refusal
                        .iter()
                        .map(String::as_str)
                        .chain(l.notes.iter().map(String::as_str))
                        .collect();
                    if !hint.is_empty() {
                        line.on_hover_text(hint.join("\n"));
                    }
                }
                hole_fields(ui, &mut edited, advanced, auto_depth);
            })
        });
        if let (Some(l), Some(object)) = (selected, index) {
            if edited != l.hole {
                // A new diameter keeps the axis where it was (TD-121).
                if edited.diameter_mm().to_bits() != l.hole.diameter_mm().to_bits() {
                    let axis = self
                        .shape(workspace, preparation, object, l.which)
                        .and_then(|(spec, _)| super::unit(spec.direction));
                    if let Some(axis) = axis {
                        edited = super::keeping_axis(workspace, preparation, object, edited, axis);
                    }
                }
                self.change_hole(workspace, preparation, object, l.which, edited);
            }
        }
    }

    /// The Drain holes panel's footer: Apply holes and Discard for the new holes, then the
    /// model's state.
    fn holes_footer(
        &mut self,
        ui: &mut egui::Ui,
        workspace: &mut Workspace,
        preparation: &mut PreparationUi,
        index: Option<usize>,
        icons: Option<&Icons>,
    ) {
        let count = self
            .pending
            .iter()
            .filter(|p| Some(p.object) == index)
            .count();
        ui.add_enabled_ui(index.is_some(), |ui| {
            let (apply, discard) = action_pair(
                ui,
                [
                    tr!("prepare-apply-holes", count = count),
                    tr!("prepare-discard-holes"),
                ],
                (count > 0, count > 0),
            );
            let apply = apply.on_hover_text(tr!("prepare-apply-holes-hint"));
            if let Some(i) = index {
                if apply.clicked() {
                    self.apply_pending(workspace, i);
                    self.awaiting = Some(Awaiting::Holes {
                        object: i,
                        revision: workspace.revision(),
                        count,
                    });
                } else if discard.clicked() {
                    self.discard(Some(i));
                }
            }
        });
        ui.add_space(4.0);
        self.cups_footer(ui, workspace, index);
        ui.add_space(4.0);
        self.holes_state(workspace, preparation, index)
            .show(ui, icons);
    }

    /// The most important thing about object `index`'s holes: why none can be made, the hole
    /// under the pointer while placing, a refused hole, then how the cavity drains.
    fn holes_state(
        &mut self,
        workspace: &Workspace,
        preparation: &mut PreparationUi,
        index: Option<usize>,
    ) -> State {
        let Some(i) = index else {
            return State::plain("–".to_owned());
        };
        if let Some(reason) = preparation.drainage_unavailable(workspace, i) {
            return State::warning(tr!("prepare-holes-unavailable")).detail(reason);
        }
        if self.placing {
            // Where the pointer is, a refused hole says why, and a valid one what it would do.
            if let Some(reason) = self.candidate_refusal() {
                return State::error(reason.to_owned());
            }
            let findings = self
                .candidate
                .as_ref()
                .and_then(|c| c.verdict.as_ref()?.findings.as_ref())
                .map_or_else(Vec::new, notes);
            if let Some(first) = findings.first() {
                return State::warning(first.clone()).detail(findings.join("\n"));
            }
            return State::info(tr!("prepare-placing")).detail(tr!("prepare-placing-hint"));
        }
        let applied = preparation
            .verdicts(workspace, i)
            .into_iter()
            .flatten()
            .enumerate()
            .find_map(|(k, v)| {
                let reason = v.refusal.as_ref()?;
                Some((tr!("prepare-hole", number = k + 1), reason.clone()))
            });
        let new = self
            .pending
            .iter()
            .filter(|p| p.object == i)
            .enumerate()
            .find_map(|(n, p)| {
                let reason = p.verdict.as_ref()?.1.refusal.as_ref()?;
                Some((tr!("prepare-new-hole", number = n + 1), reason.clone()))
            });
        if let Some((hole, reason)) = applied.or(new) {
            return State::error(tr!("prepare-hole-refused", hole = hole, reason = reason));
        }
        let object = workspace.plate().objects.get(i);
        if object.is_none_or(|o| o.hollowing().is_none()) {
            let depth = self.depth_mm.unwrap_or(SOLID_HOLE_DEPTH_MM);
            return State::plain(tr!("prepare-holes-solid", depth = decimal(depth, 1)));
        }
        let prepared = prepared(workspace, preparation, i);
        if let Some(state) = unprepared(&prepared) {
            return state;
        }
        let Prepared::Done(object) = prepared else {
            return State::plain(tr!("prepare-pending"));
        };
        let drainage = drainage(object);
        State::undrained(drainage).unwrap_or_else(|| match drainage {
            Drainage::NoCavity => State::plain(tr!("prepare-solid")),
            _ => State::success(tr!("prepare-drains")),
        })
    }

    /// What applied hollowing and holes did, once their preparation ended, as messages. A
    /// change overtaken by another edit before it was prepared reports nothing.
    pub(super) fn report_applied(&mut self, workspace: &Workspace, preparation: &PreparationUi) {
        let Some(awaiting) = self.awaiting else {
            return;
        };
        let (object, revision) = match awaiting {
            Awaiting::Hollowed { object, revision }
            | Awaiting::Holes {
                object, revision, ..
            } => (object, revision),
        };
        if workspace.revision() != revision {
            self.awaiting = None;
            return;
        }
        let Some(prepared) = preparation.current(workspace) else {
            return;
        };
        self.awaiting = None;
        let message = match (prepared.objects.get(object), awaiting) {
            (Some(Err(error)), _) => Some(
                Message::new(Level::Error, tr!("prepare-done-failed"))
                    .detail(error.reason().to_owned()),
            ),
            (Some(Ok(prepared)), Awaiting::Hollowed { .. }) => prepared.hollowed().map(|h| {
                Message::new(
                    Level::Success,
                    tr!(
                        "prepare-done-hollowed",
                        wall = decimal(h.certificate().surface_distance_mm[0], 2)
                    ),
                )
            }),
            (Some(Ok(prepared)), Awaiting::Holes { count, .. }) => Some(match drainage(prepared) {
                Drainage::Undrained(regions) => Message::new(
                    Level::Warning,
                    tr!("prepare-done-undrained", count = regions),
                ),
                _ => Message::new(Level::Success, tr!("prepare-done-holes", count = count)),
            }),
            (None, _) => None,
        };
        self.outbox.extend(message);
    }
}

/// The selected hole's diameter, depth and, with Advanced, tilt. The tilt is the one kept in
/// the model frame: it turns with the object. An automatic depth shows `auto_depth`.
fn hole_fields(ui: &mut egui::Ui, hole: &mut DrainHole, advanced: bool, auto_depth: f64) {
    let DrainHole::Surface {
        diameter_mm,
        tilt,
        depth_mm,
        ..
    } = hole
    else {
        return;
    };
    value_row(ui, tr!("hole-diameter"), |ui| {
        ui.add(
            UnitField::new(diameter_mm, Unit::Millimetre)
                .range(0.5..=20.0)
                .step(0.05)
                .decimals(1),
        )
    });
    let mut auto = depth_mm.is_none();
    let mut depth = depth_mm.unwrap_or(auto_depth);
    let changed = auto_value_row(ui, tr!("hole-depth"), &mut auto, |ui| {
        ui.add(
            UnitField::new(&mut depth, Unit::Millimetre)
                .range(0.1..=100.0)
                .step(0.05),
        )
        .on_hover_text(tr!("hole-depth-hint"))
    });
    if changed {
        *depth_mm = (!auto).then_some(depth);
    }
    if !advanced {
        return;
    }
    let mut current = tilt.unwrap_or(HoleTilt {
        angle_deg: 0.0,
        azimuth_deg: 0.0,
    });
    let angle = value_row(ui, tr!("hole-tilt"), |ui| {
        ui.add(
            UnitField::new(&mut current.angle_deg, Unit::Degree)
                .range(0.0..=MAX_HOLE_TILT_DEG)
                .step(0.5)
                .decimals(1),
        )
        .on_hover_text(tr!("hole-tilt-hint"))
    });
    let azimuth = value_row(ui, tr!("hole-tilt-direction"), |ui| {
        ui.add(
            UnitField::new(&mut current.azimuth_deg, Unit::Degree)
                .range(0.0..=360.0)
                .step(1.0)
                .decimals(0),
        )
    });
    if angle.changed() || azimuth.changed() {
        // No tilt is a perpendicular hole, recorded as such.
        *tilt = (current.angle_deg > 0.0).then_some(current);
    }
}

/// What a hole does that deserves attention (TD-089): nothing is refused, so these are notes.
pub(super) fn notes(findings: &HoleFindings) -> Vec<String> {
    let mut notes = Vec::new();
    if !findings.opens_outside {
        notes.push(tr!("hole-finding-closed"));
    }
    if findings.through {
        notes.push(tr!("hole-finding-through"));
    }
    if !findings.reaches_cavity {
        notes.push(tr!("hole-finding-no-cavity"));
    }
    if findings.ends_in_material {
        notes.push(tr!("hole-finding-in-material"));
    }
    notes
}

/// Why the hollowing `edit` of object `index` is predicted to exceed the memory budget, before
/// it is applied (TD-094): a wall under the predicted minimum, or an explicit cell too fine.
/// Only predicted, so it warns and refuses nothing.
pub(super) fn predicted_limit(
    workspace: &Workspace,
    index: usize,
    edit: &Hollowing,
) -> Option<String> {
    use karasu_project::pipeline::{cavity_predicted_to_fit, predicted_minimum_wall_mm};
    let model = workspace.plate().objects.get(index)?.model;
    let area = workspace.meshes().get(&model)?.area_mm2;
    let budget = workspace.memory_budget();
    match edit.cell_mm {
        None => {
            let minimum = predicted_minimum_wall_mm(area, budget);
            (edit.wall_mm < minimum).then(|| {
                tr!(
                    "prepare-wall-below-minimum",
                    minimum = decimal(minimum.max(0.01), 2)
                )
            })
        }
        Some(cell) => {
            (!cavity_predicted_to_fit(area, cell, budget)).then(|| tr!("prepare-cell-too-fine"))
        }
    }
}

/// What the hollowing certified (TD-093), or dashes before it has: the wall proven over every
/// cavity face, where it is thicker, the passes it took (with Advanced), the volumes, and the
/// resin the cavity saves.
fn facts(ui: &mut egui::Ui, c: Option<&SampledHollowCertificate>, advanced: bool) {
    let dash = || "–".to_owned();
    let length = |mm: f64| tr!("prepare-length-mm", length = decimal(mm, 2));
    let volume = |mm3: f64| tr!("prepare-volume-mm3", volume = decimal(mm3, 0));
    fact_row(
        ui,
        &tr!("prepare-guaranteed-wall"),
        &c.map_or_else(dash, |c| length(c.surface_distance_mm[0])),
    )
    .on_hover_text(tr!("prepare-guaranteed-wall-hint"));
    fact_row(
        ui,
        &tr!("prepare-thickest-wall"),
        &c.map_or_else(dash, |c| length(c.vertex_distance_mm[1])),
    )
    .on_hover_text(tr!("prepare-thickest-wall-hint"));
    if advanced {
        fact_row(
            ui,
            &tr!("prepare-passes"),
            &c.map_or_else(dash, |c| c.passes.to_string()),
        )
        .on_hover_text(tr!("prepare-passes-hint"));
    }
    fact_row(
        ui,
        &tr!("prepare-cavity-volume"),
        &c.map_or_else(dash, |c| volume(c.cavity_volume_mm3)),
    );
    fact_row(
        ui,
        &tr!("prepare-material-volume"),
        &c.map_or_else(dash, |c| volume(c.material_volume_mm3())),
    );
    fact_row(
        ui,
        &tr!("prepare-resin-saved"),
        &c.and_then(resin_saved_percent)
            .map_or_else(dash, |p| tr!("prepare-percent", percent = decimal(p, 0))),
    )
    .on_hover_text(tr!("prepare-resin-saved-hint"));
}

/// The share of the solid model's volume the cavity takes, in percent: the resin the hollowed
/// print does not use. `None` for a model without volume.
fn resin_saved_percent(c: &SampledHollowCertificate) -> Option<f64> {
    (c.source_volume_mm3 > 0.0).then(|| 100.0 * c.cavity_volume_mm3 / c.source_volume_mm3)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hollowing_ui::tests::cube;

    /// The width a panel's body has: the panel, less its frame.
    const WIDTH: f32 = 316.0;

    /// Draw `part`'s panel and footer for `index`, `WIDTH` wide, over two frames: the texts
    /// drawn, and the width used.
    fn draw(
        state: &mut HollowingUi,
        workspace: &mut Workspace,
        preparation: &mut PreparationUi,
        index: Option<usize>,
        part: Part,
    ) -> (Vec<String>, f32) {
        let ctx = egui::Context::default();
        let mut texts = Vec::new();
        let mut used = 0.0;
        for _ in 0..2 {
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                ui.allocate_ui(egui::vec2(WIDTH, 4000.0), |ui| {
                    ui.set_width(WIDTH);
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                    state.panel(ui, workspace, preparation, index, part, None);
                    state.footer(ui, workspace, preparation, index, part, None);
                    used = ui.min_rect().width();
                });
            });
            output.textures_delta.clear();
            texts = output
                .shapes
                .iter()
                .filter_map(|s| match &s.shape {
                    egui::epaint::Shape::Text(t) => Some(t.galley.text().to_owned()),
                    _ => None,
                })
                .collect();
        }
        (texts, used)
    }

    #[test]
    fn without_a_selection_both_panels_keep_their_structure_and_change_nothing() {
        let mut state = HollowingUi::default();
        let mut workspace = Workspace::new();
        let mut preparation = PreparationUi::default();
        let before = workspace.plate().clone();
        let (texts, _) = draw(
            &mut state,
            &mut workspace,
            &mut preparation,
            None,
            Part::Hollow,
        );
        for expected in [
            // A group's title is written in capitals on its frame.
            tr!("prepare-hollow").to_uppercase(),
            tr!("prepare-wall"),
            tr!("prepare-result"),
            tr!("prepare-guaranteed-wall"),
            tr!("prepare-resin-saved"),
            tr!("prepare-apply-hollow"),
            tr!("prepare-remove-hollow"),
        ] {
            assert!(texts.contains(&expected), "{expected} in {texts:?}");
        }
        let (texts, _) = draw(
            &mut state,
            &mut workspace,
            &mut preparation,
            None,
            Part::Holes,
        );
        for expected in [
            tr!("prepare-new-holes").to_uppercase(),
            tr!("hole-diameter"),
            tr!("holes-selected").to_uppercase(),
            tr!("prepare-drainage"),
            tr!("prepare-apply-holes", count = 0_usize),
            tr!("prepare-discard-holes"),
        ] {
            assert!(texts.contains(&expected), "{expected} in {texts:?}");
        }
        assert_eq!(*workspace.plate(), before);
        assert!(!state.placing);
    }

    #[test]
    fn the_footers_say_the_most_important_thing_first() {
        let (mut workspace, mut preparation, directory) = cube(false);
        let mut state = HollowingUi::default();
        // Solid: nothing to warn about.
        let hollow = state.hollow_state(&workspace, &preparation, Some(0));
        assert_eq!(hollow.text, tr!("prepare-solid"));
        assert!(!hollow.to_holes);
        let holes = state.holes_state(&workspace, &mut preparation, Some(0));
        assert_eq!(
            holes.text,
            tr!(
                "prepare-holes-solid",
                depth = decimal(SOLID_HOLE_DEPTH_MM, 1)
            )
        );
        // A thin wall in the draft comes first.
        state.draft(0, None).edit.wall_mm = 0.5;
        let hollow = state.hollow_state(&workspace, &preparation, Some(0));
        assert_eq!(hollow.text, tr!("prepare-thin-wall"));
        assert_eq!(hollow.icon, Some(Icon::Warning));
        // Placing: the footer says how.
        state.placing = true;
        let holes = state.holes_state(&workspace, &mut preparation, Some(0));
        assert_eq!(holes.text, tr!("prepare-placing"));
        // Without a selection: a dash.
        assert_eq!(state.hollow_state(&workspace, &preparation, None).text, "–");
        // Scaled after hollowing: the wall scaled with it.
        workspace.edit(|plate| {
            plate.objects[0].set_hollowing(Some(Hollowing {
                wall_mm: 2.0,
                cell_mm: None,
                simplify: None,
            }));
            plate.objects[0].pose.scale = [1.0, 2.0, 1.0];
        });
        let hollow = state.hollow_state(&workspace, &preparation, Some(0));
        assert_eq!(
            hollow.text,
            tr!(
                "prepare-scaled-wall",
                min = decimal(2.0, 2),
                max = decimal(4.0, 2)
            )
        );
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn a_hollowed_cube_shows_what_was_certified_and_leads_to_drain_holes() {
        let (mut workspace, mut preparation, directory) = cube(true);
        let mut state = HollowingUi {
            hollow_advanced: true,
            ..HollowingUi::default()
        };
        let prepared = preparation.current(&workspace).unwrap().objects[0]
            .as_ref()
            .unwrap();
        assert_eq!(drainage(prepared), Drainage::NoHole);
        let certificate = prepared.hollowed().unwrap().certificate();
        // A 20 mm cube with a 2 mm wall: the cavity is about 16³ of 20³.
        let saved = resin_saved_percent(&certificate).unwrap();
        assert!((45.0..60.0).contains(&saved), "{saved}");
        // The enclosed cavity is the footer's state, with the way to the Drain holes tool.
        let hollow = state.hollow_state(&workspace, &preparation, Some(0));
        assert_eq!(hollow.text, tr!("prepare-enclosed"));
        assert!(hollow.to_holes);
        let holes = state.holes_state(&workspace, &mut preparation, Some(0));
        assert_eq!(holes.text, tr!("prepare-no-drain-hole"));
        // Both panels keep their width with every setting shown, also in the pseudo-locale.
        state.holes_advanced = true;
        for locale in ["en-US", "pseudo"] {
            crate::i18n::install(crate::i18n::Catalog::open(locale, None).unwrap());
            for part in [Part::Hollow, Part::Holes] {
                let (texts, used) =
                    draw(&mut state, &mut workspace, &mut preparation, Some(0), part);
                assert!(used <= WIDTH + 0.5, "{locale} {part:?}: {used} wide");
                if locale == "en-US" && part == Part::Hollow {
                    let percent = tr!("prepare-percent", percent = decimal(saved, 0));
                    assert!(texts.contains(&percent), "{percent} in {texts:?}");
                }
            }
        }
        crate::i18n::install(crate::i18n::Catalog::reference());
        // Applied hollowing is reported once prepared; one overtaken by an edit is not.
        state.awaiting = Some(Awaiting::Hollowed {
            object: 0,
            revision: workspace.revision(),
        });
        state.report_applied(&workspace, &preparation);
        let wall = decimal(certificate.surface_distance_mm[0], 2);
        assert_eq!(
            state.outbox,
            [Message::new(
                Level::Success,
                tr!("prepare-done-hollowed", wall = wall)
            )]
        );
        state.outbox.clear();
        state.awaiting = Some(Awaiting::Hollowed {
            object: 0,
            revision: workspace.revision() + 1,
        });
        state.report_applied(&workspace, &preparation);
        assert_eq!(state.outbox, Vec::new());
        assert_eq!(state.awaiting, None);
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn a_model_never_hollowed_starts_from_the_tools_hollowing() {
        let mut state = HollowingUi::default();
        state.new_hollowing.wall_mm = 1.5;
        assert_eq!(state.draft(3, None).edit.wall_mm, 1.5);
        // A hollowed one starts from its own.
        let own = Hollowing {
            wall_mm: 3.0,
            cell_mm: None,
            simplify: None,
        };
        assert_eq!(state.draft(3, Some(own)).edit, own);
    }
}
