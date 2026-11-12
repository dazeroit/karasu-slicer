//! The project's plates in the window (TD-135, from the owner-approved mockup "karasu multiple
//! plates mockup"): the plates bar at the bottom of the viewport, the Plate menu's actions, and
//! what showing another plate changes.
//!
//! **The plates bar** shows one card per plate in project order, its thumbnail from above, its
//! name, its number of models and where its slice stands; a click shows the plate, a card
//! dragged onto another moves it there, its menu renames, duplicates or deletes it. Folded, it
//! is a row of tabs, with Previous, Next and New; folded or not is a persistent setting.
//!
//! **Showing another plate** rebuilds the interfaces that hold a plate's state (the selection,
//! the preparation, the supports, the tools' plans, the section) as opening a project does, the
//! tools' values kept. The viewport stays, with every model uploaded: nothing is uploaded again.
//! Each plate's slicing is kept while another is shown, its result, its masks and a slice still
//! running included, so a plate shown again can be exported without slicing it again. Revisions
//! are unique across the project (TD-135), so no result computed for one plate is taken for
//! another's.
//!
//! **Thumbnails** are drawn by the viewport's renderer, from above, with the meshes it already
//! holds, into a texture of their own (see
//! [`crate::viewport::Viewport::render_thumbnail`]): a plate's thumbnail
//! is drawn again only when its revision changes, one plate a frame at most, and none while
//! the bar is folded. The overview's larger thumbnails are drawn the same way, only while it is
//! open.
//!
//! **The overview** ([`overview`]) is a tool window of larger cards, one per plate, with its
//! profiles and its last slice's facts.
//!
//! **Arrange models…** ([`arrange`]) packs the plate shown's models, the overflow to new plates
//! or left in place; **Slice every plate** and **Export every plate…** ([`batch`]) take the
//! plates one after another.
mod arrange;
mod batch;
mod clone;
mod overview;

use super::MainWindow;
use crate::{
    actions::Action,
    camera::Mat4,
    dialog::{self, Part, Size},
    i18n::tr,
    icons::Icon,
    slicing_ui::{SliceState, SlicingUi},
    theme,
    widgets::icon_button,
};
use eframe::egui;
use karasu_project::project::{Freshness, PlateId};
use std::collections::HashMap;

/// The least time between two thumbnails of the plate shown, which a drag edits every frame.
const THUMBNAIL_PAUSE: std::time::Duration = std::time::Duration::from_millis(500);
/// The card's image, in points.
const THUMBNAIL_POINTS: egui::Vec2 = egui::vec2(96.0, 54.0);
/// A card's size, in points.
const CARD: egui::Vec2 = egui::vec2(230.0, 76.0);
/// The bar's height, open and folded, in points.
const OPEN_HEIGHT: f32 = 92.0;
const FOLDED_HEIGHT: f32 = 40.0;
/// The room the bar keeps right of the plates for New, All plates… and the fold button.
const BAR_ENDS: f32 = 190.0;
/// The plate shown's card or tab (owner, 2026-11-05: quieter than the selection's cyan, its
/// white text easy to read): a dark violet of the palette's primary, outlined in its accent.
const LIT_FILL: egui::Color32 = egui::Color32::from_rgb(0x2C, 0x25, 0x47);

/// The plates' state in the window.
#[derive(Default)]
pub(super) struct Plates {
    /// The plate the per-plate interfaces were made for.
    shown: Option<PlateId>,
    /// The other plates' slicing, kept while another plate is shown.
    pub(super) slicings: HashMap<PlateId, SlicingUi>,
    /// Each plate's thumbnails, of each size.
    thumbnails: HashMap<(PlateId, Kind), Thumbnail>,
    /// The name typed in the Rename plate dialog, while it is open.
    renaming: Option<String>,
    /// The overview is open.
    pub(super) overview: bool,
    /// The overview was opened this frame: it opens in the middle.
    overview_opened: bool,
    /// The plate whose deletion waits for the user's answer.
    pub(super) deleting: Option<PlateId>,
    /// Arrange models… is open, with this spacing in mm.
    arranging: Option<f64>,
    /// Clone is open (owner, 2026-11-10).
    cloning: Option<clone::CloneForm>,
    /// Export every plate… is open.
    exporting: Option<batch::ExportForm>,
    /// Plates being sliced, or exported, one after another.
    batch: Option<batch::Batch>,
    /// Arrange's and Export every plate's choices, kept across restarts.
    pub(super) choices: crate::settings::PlateSettings,
}

/// A plate's thumbnail.
#[derive(Clone, Copy)]
struct Thumbnail {
    texture: egui::TextureId,
    /// The plate's revision it shows.
    revision: u64,
    /// When it was drawn.
    drawn: std::time::Instant,
    /// The box the plate's models cover, low and high corners in mm, when it has any.
    footprint: Option<[[f64; 2]; 2]>,
}

/// A thumbnail's size: the plates bar's, or the overview's larger one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Kind {
    Bar,
    Overview,
}

impl Kind {
    /// The size in pixels: twice the image's points, for high-density screens.
    fn pixels(self) -> [u32; 2] {
        match self {
            Self::Bar => [192, 108],
            Self::Overview => [512, 288],
        }
    }

    /// The key the viewport keeps plate `id`'s thumbnail of this size under.
    fn key(self, id: PlateId) -> u64 {
        // Plate identities are small sequence numbers: the low bit is free for the size.
        id.0 << 1 | u64::from(self == Self::Overview)
    }
}

/// How a plate's slice stands, for its card.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shown {
    NotSliced,
    Slicing,
    Failed,
    Sliced,
    OutOfDate,
}

impl Shown {
    fn label(self) -> String {
        match self {
            Self::NotSliced => tr!("plates-state-none"),
            Self::Slicing => tr!("plates-state-running"),
            Self::Failed => tr!("plates-state-failed"),
            Self::Sliced => tr!("plates-state-sliced"),
            Self::OutOfDate => tr!("plates-state-stale"),
        }
    }

    fn color(self) -> egui::Color32 {
        match self {
            Self::NotSliced => theme::BORDER.color(),
            Self::Slicing => theme::ACCENT.color(),
            Self::Failed => theme::ERROR.color(),
            Self::Sliced => theme::SUCCESS.color(),
            Self::OutOfDate => theme::WARNING.color(),
        }
    }
}

/// An orthographic view from straight above of `volume` (low and high corners, mm), fitted to
/// an image of `aspect` (width over height), depth 0 at the top of the volume.
fn from_above(volume: [[f64; 3]; 2], aspect: f64) -> Mat4 {
    let [[x0, y0, z0], [x1, y1, z1]] = volume;
    let (cx, cy) = (f64::midpoint(x0, x1), f64::midpoint(y0, y1));
    let (mut w, mut h) = ((x1 - x0).max(1.0), (y1 - y0).max(1.0));
    // The plate whole in the image, the spare room split on both sides.
    if w / h > aspect {
        h = w / aspect;
    } else {
        w = h * aspect;
    }
    let depth = (z1 - z0).max(1.0);
    Mat4([
        [2.0 / w, 0.0, 0.0, -2.0 * cx / w],
        [0.0, 2.0 / h, 0.0, -2.0 * cy / h],
        [0.0, 0.0, -1.0 / depth, z1 / depth],
        [0.0, 0.0, 0.0, 1.0],
    ])
}

impl MainWindow {
    /// Make the per-plate interfaces follow the plate shown, whatever showed it (a card, the
    /// Plate menu, an undo); keep the others' slicing going. Called first in every frame.
    pub(super) fn follow_plate(&mut self, ctx: &egui::Context) {
        let current = self.workspace.plate_id();
        for slicing in self.plates.slicings.values_mut() {
            if slicing.poll() {
                ctx.request_repaint();
            }
        }
        // A plate no longer in the project keeps nothing.
        let exists = |id: &PlateId| self.workspace.project().plate(*id).is_some();
        if self.plates.slicings.keys().any(|id| !exists(id))
            || self.plates.thumbnails.keys().any(|(id, _)| !exists(id))
        {
            let project = self.workspace.project();
            self.plates
                .slicings
                .retain(|id, _| project.plate(*id).is_some());
            let gone: Vec<(PlateId, Kind)> = self
                .plates
                .thumbnails
                .keys()
                .copied()
                .filter(|(id, _)| project.plate(*id).is_none())
                .collect();
            for (id, kind) in gone {
                self.plates.thumbnails.remove(&(id, kind));
                if let (Some(viewport), Some(state)) =
                    (self.viewport.as_mut(), self.render_state.as_ref())
                {
                    viewport.drop_thumbnail(state, kind.key(id));
                }
            }
        }
        let Some(previous) = self.plates.shown.replace(current) else {
            return;
        };
        if previous == current {
            return;
        }
        let mut settings = self.collect_settings(ctx);
        // The plate shown keeps its own printer, resin and print setting.
        settings.profiles = crate::settings::ProfileChoice::default();
        // What the tools' panels set for the next operation is the user's while they work
        // (owner, 2026-11-02): it stays across plates.
        let values = self.tool_values();
        let slicing = self
            .plates
            .slicings
            .remove(&current)
            .unwrap_or_else(|| self.slicing.sibling());
        let left = std::mem::replace(&mut self.slicing, slicing);
        if self.workspace.project().plate(previous).is_some() {
            self.plates.slicings.insert(previous, left);
        }
        self.selected = None;
        self.deleting = None;
        self.checked.clear();
        self.deleting_checked = false;
        self.clearing = None;
        self.drag = None;
        self.preparation = crate::preparation_ui::PreparationUi::default();
        self.hollowing = crate::hollowing_ui::HollowingUi::default();
        self.supports = crate::supports_ui::SupportsUi::new();
        self.raft = crate::raft_ui::RaftUi::default();
        self.islands = crate::islands_ui::IslandsUi::new();
        self.auto = crate::auto_ui::AutoUi::new();
        self.grid = crate::grid_ui::GridUi::new();
        self.orient = crate::orient_ui::OrientUi::new();
        self.section = crate::section_ui::SectionUi::default();
        self.region_view = None;
        self.followed = None;
        self.lines_built = false;
        self.apply_settings(ctx, &settings);
        self.set_tool_values(&values);
        log::info!(target: "app", "plate {} shown", current.0);
    }

    /// Forget every plate's state as another project replaces this one, whose plate identities
    /// and revisions start over: the plates' slicings (their files with them, a running batch
    /// cancelled), the thumbnails (freed from the renderer), the dialogs. Arrange's and Export
    /// every plate's choices stay, and the overview stays open or closed.
    pub(super) fn forget_plates(&mut self) {
        if let (Some(viewport), Some(state)) = (self.viewport.as_mut(), self.render_state.as_ref())
        {
            for &(id, kind) in self.plates.thumbnails.keys() {
                viewport.drop_thumbnail(state, kind.key(id));
            }
        }
        if self.plates.batch.is_some() {
            log::info!(target: "app", "plates' batch dropped with its project");
        }
        self.plates = Plates {
            choices: std::mem::take(&mut self.plates.choices),
            overview: self.plates.overview,
            ..Plates::default()
        };
    }

    /// Whether a Plate menu action can be taken now.
    pub(super) fn plate_action_enabled(&self, action: Action) -> bool {
        let several = self.workspace.plates().len() > 1;
        match action {
            Action::DeletePlate | Action::PreviousPlate | Action::NextPlate => several,
            Action::ArrangeModels => {
                !self.workspace.plate().objects.is_empty()
                    && self.workspace.plate().printer.is_some()
            }
            Action::SliceEveryPlate | Action::ExportEveryPlate => self.plates.batch.is_none(),
            _ => true,
        }
    }

    /// Take a Plate menu action.
    pub(super) fn perform_plate(&mut self, action: Action) {
        match action {
            Action::NewPlate => {
                let name = self.new_plate_name();
                self.workspace.new_plate(name);
            }
            Action::DuplicatePlate => self.duplicate_plate(self.workspace.plate_id()),
            Action::RenamePlate => self.plates.renaming = Some(self.workspace.plate().name.clone()),
            Action::DeletePlate => self.ask_delete_plate(self.workspace.plate_id()),
            Action::PreviousPlate => {
                self.workspace.step_plate(-1);
            }
            Action::NextPlate => {
                self.workspace.step_plate(1);
            }
            Action::TogglePlatesBar => {
                self.chrome.plates_bar_open = !self.chrome.plates_bar_open;
            }
            Action::PlatesOverview => {
                self.plates.overview = !self.plates.overview;
                self.plates.overview_opened = self.plates.overview;
            }
            Action::ArrangeModels => self.open_arrange(),
            Action::SliceEveryPlate => self.slice_every_plate(),
            Action::ExportEveryPlate => self.open_export_every_plate(),
            _ => {}
        }
    }

    /// A new plate's name: "Plate n" with the first number from the plates' count up that no
    /// plate is named with (owner, 2026-11-05: deleting a plate and adding one gave two
    /// "Plate 2").
    fn new_plate_name(&self) -> String {
        let plates = self.workspace.plates();
        // `n` plates take at most `n` of the `n + 1` numbers from `n + 1` to `2n + 1`.
        (plates.len() + 1..=2 * plates.len() + 1)
            .map(|number| tr!("plate-default-name", number = number))
            .find(|name| plates.iter().all(|p| p.content().name != *name))
            .unwrap_or_default()
    }

    /// Delete plate `id`: at once when it has no model, otherwise once the user confirms
    /// (owner, 2026-11-05). Undo brings it back either way.
    fn ask_delete_plate(&mut self, id: PlateId) {
        match self.workspace.project().plate(id) {
            Some(plate) if plate.content().objects.is_empty() => {
                self.workspace.delete_plate(id);
            }
            Some(_) if self.workspace.plates().len() > 1 => self.plates.deleting = Some(id),
            _ => {}
        }
    }

    /// The confirmation of a plate's deletion, while it waits.
    pub(super) fn confirm_plate_deletion(&mut self, ctx: &egui::Context) {
        let Some(id) = self.plates.deleting else {
            return;
        };
        let Some(plate) = self.workspace.project().plate(id) else {
            self.plates.deleting = None;
            return;
        };
        let content = plate.content();
        match dialog::confirm(
            ctx,
            "delete-plate",
            &tr!("delete-plate-title", name = &content.name),
            &tr!("delete-plate-text", count = content.objects.len()),
            &tr!("action-delete"),
            true,
        ) {
            Some(true) => {
                self.plates.deleting = None;
                self.workspace.delete_plate(id);
            }
            Some(false) => self.plates.deleting = None,
            None => {}
        }
    }

    /// Move object `index` of the plate shown to plate `to` (a new plate for `None`), or copy it
    /// there, and say where it went or why it stayed (TD-135, phase 3).
    pub(super) fn transfer_object(&mut self, index: usize, to: Option<PlateId>, copy: bool) {
        use crate::{
            messages::{Level, Message},
            workspace::{Landing, Refused},
        };
        let Some(model) = self
            .workspace
            .plate()
            .objects
            .get(index)
            .map(|o| o.name.clone())
        else {
            return;
        };
        let name = self.new_plate_name();
        let plate = to
            .and_then(|id| self.workspace.project().plate(id))
            .map_or_else(|| name.clone(), |p| p.content().name.clone());
        let message = match self.workspace.transfer(index, to, copy, name) {
            Ok(landing) => {
                if !copy {
                    self.forget_object(index);
                }
                let (model, plate) = (model.as_str(), plate.as_str());
                let text = match (copy, landing) {
                    (false, Landing::Kept) => tr!("plates-moved", model = model, plate = plate),
                    (false, Landing::Beside) => {
                        tr!("plates-moved-beside", model = model, plate = plate)
                    }
                    (true, Landing::Kept) => tr!("plates-copied", model = model, plate = plate),
                    (true, Landing::Beside) => {
                        tr!("plates-copied-beside", model = model, plate = plate)
                    }
                };
                Message::new(Level::Success, text)
            }
            Err(Refused::NoRoom) => Message::new(
                Level::Warning,
                tr!("plates-transfer-no-room", model = &model, plate = &plate),
            ),
            Err(Refused::Loading) => Message::new(Level::Info, tr!("plates-transfer-loading")),
            Err(Refused::Unknown) => return,
        };
        self.messages.push(message);
    }

    /// Add a copy of plate `id` after it and show it.
    fn duplicate_plate(&mut self, id: PlateId) {
        let Some(plate) = self.workspace.project().plate(id) else {
            return;
        };
        let name = tr!("plate-copy-name", name = &plate.content().name);
        self.workspace.duplicate_plate(id, name);
    }

    /// Plate `id`'s slicing: the plate shown's, or the one kept for it.
    fn slicing_of(&self, id: PlateId) -> Option<&SlicingUi> {
        if id == self.workspace.plate_id() {
            Some(&self.slicing)
        } else {
            self.plates.slicings.get(&id)
        }
    }

    /// Plate `id`'s last slice's facts, when it is of the plate as it is now.
    pub(super) fn current_facts(&self, id: PlateId) -> Option<crate::slicing_ui::SliceFacts> {
        self.slicing_of(id)?
            .slice_facts()
            .filter(|f| self.workspace.project().freshness(id, f.revision) == Freshness::Current)
    }

    /// Ask where to export plate `id`'s last slice, and export it: the plate's own slicing
    /// says how it went.
    fn export_plate(&mut self, id: PlateId) {
        let Some(plate) = self.workspace.project().plate(id) else {
            return;
        };
        if self.current_facts(id).is_none() {
            return;
        }
        let name = plate.content().name.clone();
        let slicing = if id == self.workspace.plate_id() {
            Some(&mut self.slicing)
        } else {
            self.plates.slicings.get_mut(&id)
        };
        if let Some(slicing) = slicing {
            slicing.export_as(&name);
        }
    }

    /// Take the messages of the other plates' slicing: a slice ending, an export written.
    pub(super) fn take_plates_messages(&mut self) -> Vec<crate::messages::Message> {
        self.plates
            .slicings
            .values_mut()
            .flat_map(SlicingUi::take_messages)
            .collect()
    }

    /// Where plate `id`'s slice stands.
    fn plate_slice(&self, id: PlateId) -> Shown {
        let state = self
            .slicing_of(id)
            .map_or(SliceState::None, SlicingUi::slice_state);
        match state {
            SliceState::None => Shown::NotSliced,
            SliceState::Running => Shown::Slicing,
            SliceState::Failed => Shown::Failed,
            SliceState::Sliced(revision) => {
                match self.workspace.project().freshness(id, revision) {
                    Freshness::Current => Shown::Sliced,
                    _ => Shown::OutOfDate,
                }
            }
        }
    }

    /// Draw again at most one thumbnail of size `kind` whose plate changed since it was drawn.
    fn refresh_thumbnail(&mut self, ctx: &egui::Context, kind: Kind) {
        let (Some(render), Some(viewport)) = (self.render_state.clone(), self.viewport.as_mut())
        else {
            return;
        };
        let color = self.appearance.object.linear();
        let now = std::time::Instant::now();
        // Plates whose thumbnail is older than their content; the shown plate, edited frame
        // after frame by a drag, at most twice a second.
        let stale: Vec<(PlateId, u64)> = self
            .workspace
            .plates()
            .iter()
            .filter(|p| {
                self.plates.thumbnails.get(&(p.id(), kind)).is_none_or(|t| {
                    t.revision != p.revision()
                        && (p.id() != self.workspace.plate_id()
                            || now.duration_since(t.drawn) >= THUMBNAIL_PAUSE)
                })
            })
            .map(|p| (p.id(), p.revision()))
            .collect();
        // The shown plate's thumbnail, held back by the pause, is drawn once it ends.
        let shown = self.workspace.plate_id();
        if self
            .plates
            .thumbnails
            .get(&(shown, kind))
            .is_some_and(|t| t.revision != self.workspace.revision())
        {
            ctx.request_repaint_after(THUMBNAIL_PAUSE);
        }
        // The first that can be drawn: a plate without a printer, or with a model still
        // loading, waits.
        let Some((id, revision, drawn)) = stale.into_iter().find_map(|(id, revision)| {
            self.workspace
                .plate_draws(id, color)
                .map(|drawn| (id, revision, drawn))
        }) else {
            return;
        };
        let volume = drawn.volume;
        let pixels = kind.pixels();
        let aspect = f64::from(pixels[0]) / f64::from(pixels[1]);
        let projection = from_above(volume, aspect);
        let top = volume[1][2] + 1000.0;
        #[allow(clippy::cast_possible_truncation)] // GPU precision.
        let eye = [
            f64::midpoint(volume[0][0], volume[1][0]) as f32,
            f64::midpoint(volume[0][1], volume[1][1]) as f32,
            top as f32,
        ];
        let clear = self.appearance.plate.encoded().map(|c| c.max(0.0));
        let clear = [clear[0], clear[1], clear[2], 1.0];
        if let Some(texture) = viewport.render_thumbnail(
            &render,
            kind.key(id),
            pixels,
            &projection,
            eye,
            [0.3, -0.4, 1.0],
            clear,
            &drawn.draws,
        ) {
            self.plates.thumbnails.insert(
                (id, kind),
                Thumbnail {
                    texture,
                    revision,
                    drawn: now,
                    footprint: drawn.footprint,
                },
            );
            ctx.request_repaint();
        }
    }

    /// The plates bar at the bottom of the viewport: cards with thumbnails, or a row of tabs.
    pub(super) fn plates_bar(&mut self, ui: &mut egui::Ui) {
        let open = self.chrome.plates_bar_open;
        if open {
            self.refresh_thumbnail(ui.ctx(), Kind::Bar);
        }
        let mut chosen: Option<PlateId> = None;
        let mut moved: Option<(PlateId, usize)> = None;
        let mut dropped: Option<ModelDrop> = None;
        let mut act: Option<Action> = None;
        egui::Panel::bottom("plates")
            .resizable(false)
            .exact_size(if open { OPEN_HEIGHT } else { FOLDED_HEIGHT })
            .frame(
                egui::Frame::new()
                    .fill(theme::SURFACE.color())
                    .inner_margin(egui::Margin::symmetric(8, if open { 6 } else { 8 })),
            )
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    let icons = self.icons.as_ref();
                    if !open
                        && icon_button(
                            ui,
                            icons,
                            Icon::ChevronLeft,
                            false,
                            &Action::PreviousPlate.label(),
                        )
                        .clicked()
                    {
                        act = Some(Action::PreviousPlate);
                    }
                    crate::focus::scrolled(
                        egui::ScrollArea::horizontal()
                            .max_width((ui.available_width() - BAR_ENDS).max(60.0)),
                        ui,
                        |ui| {
                            ui.horizontal_centered(|ui| {
                                self.plate_items(
                                    ui,
                                    open,
                                    &mut chosen,
                                    (&mut moved, &mut dropped),
                                    &mut act,
                                );
                            });
                        },
                    );
                    if !open
                        && icon_button(
                            ui,
                            icons,
                            Icon::ChevronRight,
                            false,
                            &Action::NextPlate.label(),
                        )
                        .clicked()
                    {
                        act = Some(Action::NextPlate);
                    }
                    if icon_button(ui, icons, Icon::Plus, false, &Action::NewPlate.label())
                        .clicked()
                    {
                        act = Some(Action::NewPlate);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // The fold button at the right end, folded or open (owner, 2026-11-10).
                        let (icon, hint) = if open {
                            (Icon::ChevronDown, tr!("plates-fold"))
                        } else {
                            (Icon::ChevronUp, tr!("plates-unfold"))
                        };
                        if icon_button(ui, icons, icon, false, &hint).clicked() {
                            act = Some(Action::TogglePlatesBar);
                        }
                        let all = ui
                            .selectable_label(self.plates.overview, Action::PlatesOverview.label())
                            .on_hover_text(tr!("action-plates-overview-hint"));
                        if all.clicked() {
                            act = Some(Action::PlatesOverview);
                        }
                    });
                });
            });
        if let Some(id) = chosen {
            self.workspace.show_plate(id);
        }
        if let Some((id, to)) = moved {
            self.workspace.move_plate(id, to);
        }
        if let Some(drop) = dropped {
            self.transfer_object(drop.index, Some(drop.to), drop.copy);
        }
        if let Some(action) = act {
            self.perform(action);
        }
        // What is drawn after the bar in this frame (the tools' panels, the overview) is of
        // the plate shown now.
        self.follow_plate(ui.ctx());
    }

    /// The plates' cards (open) or tabs (folded), in project order.
    fn plate_items(
        &self,
        ui: &mut egui::Ui,
        open: bool,
        chosen: &mut Option<PlateId>,
        (moved, dropped): (&mut Option<(PlateId, usize)>, &mut Option<ModelDrop>),
        act: &mut Option<Action>,
    ) {
        let current = self.workspace.plate_id();
        for (index, plate) in self.workspace.plates().iter().enumerate() {
            let id = plate.id();
            let content = plate.content();
            let state = self.plate_slice(id);
            let lit = id == current;
            let response = if open {
                self.card(ui, id, &content.name, content.objects.len(), state, lit)
            } else {
                tab(ui, &content.name, state, lit)
            };
            let response = response.on_hover_text(tr!(
                "plates-card-hint",
                name = &content.name,
                state = state.label()
            ));
            if response.clicked() {
                *chosen = Some(id);
            }
            response.dnd_set_drag_payload(id);
            if let Some(dragged) = released::<PlateId>(&response) {
                if *dragged != id {
                    *moved = Some((*dragged, index));
                }
            }
            *dropped = dropped.or(model_drop(
                ui,
                &response,
                id,
                !lit,
                if open { 8.0 } else { 12.0 },
            ));
            response.context_menu(|ui| {
                for action in [
                    Action::RenamePlate,
                    Action::DuplicatePlate,
                    Action::DeletePlate,
                ] {
                    let enabled = self.plate_action_enabled(action);
                    if ui
                        .add_enabled(enabled, egui::Button::new(action.label()))
                        .clicked()
                    {
                        // The menu acts on the plate it was opened on.
                        *chosen = Some(id);
                        *act = Some(action);
                        ui.close();
                    }
                }
            });
        }
    }

    /// One plate's card: its thumbnail, name, models and slice.
    fn card(
        &self,
        ui: &mut egui::Ui,
        id: PlateId,
        name: &str,
        models: usize,
        state: Shown,
        lit: bool,
    ) -> egui::Response {
        let (rect, response) = ui.allocate_exact_size(CARD, egui::Sense::click_and_drag());
        plate_info(&response, name, state, lit);
        let painter = ui.painter_at(rect);
        let (fill, stroke) = if lit {
            (LIT_FILL, theme::ACCENT.color())
        } else if response.hovered() {
            (theme::BACKGROUND.color(), theme::BORDER.color())
        } else {
            (theme::BACKGROUND.color(), theme::SURFACE.color())
        };
        painter.rect(
            rect,
            8.0,
            fill,
            egui::Stroke::new(1.0, stroke),
            egui::StrokeKind::Inside,
        );
        let image = egui::Rect::from_min_size(
            rect.min + egui::vec2(8.0, (CARD.y - THUMBNAIL_POINTS.y) / 2.0),
            THUMBNAIL_POINTS,
        );
        match self.plates.thumbnails.get(&(id, Kind::Bar)) {
            Some(thumbnail) => painter.image(
                thumbnail.texture,
                image,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            ),
            None => painter.rect_filled(image, 4.0, self.appearance.plate.color()),
        };
        let text = image.right() + 10.0;
        let line = |row: f32| egui::pos2(text, rect.top() + 12.0 + row * 19.0);
        let width = rect.right() - text - 6.0;
        let font = |size| egui::FontId::proportional(size);
        let fit = |s: &str, size: f32| {
            let galley =
                painter.layout_no_wrap(s.to_owned(), font(size), theme::TEXT_PRIMARY.color());
            if galley.size().x <= width {
                s.to_owned()
            } else {
                let mut cut: String = s
                    .chars()
                    .take(s.chars().count().saturating_sub(1))
                    .collect();
                while !cut.is_empty()
                    && painter
                        .layout_no_wrap(format!("{cut}…"), font(size), egui::Color32::WHITE)
                        .size()
                        .x
                        > width
                {
                    cut.pop();
                }
                format!("{cut}…")
            }
        };
        painter.text(
            line(0.0),
            egui::Align2::LEFT_TOP,
            fit(name, 14.0),
            font(14.0),
            theme::TEXT_PRIMARY.color(),
        );
        painter.text(
            line(1.0),
            egui::Align2::LEFT_TOP,
            tr!("plates-models", count = models),
            font(12.0),
            theme::TEXT_PRIMARY.color().gamma_multiply(0.7),
        );
        let label = state.label();
        let chip = painter.layout_no_wrap(label, font(11.0), theme::BACKGROUND.color());
        let chip_rect = egui::Rect::from_min_size(line(2.0), chip.size() + egui::vec2(12.0, 4.0));
        painter.rect_filled(chip_rect, 8.0, state.color());
        painter.galley(
            chip_rect.min + egui::vec2(6.0, 2.0),
            chip,
            theme::BACKGROUND.color(),
        );
        response
    }

    /// The Rename plate dialog, while it is open: modal, Enter renames, Escape cancels.
    pub(super) fn rename_plate_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut name) = self.plates.renaming.take() else {
            return;
        };
        let mut done = None;
        let response = dialog::show(
            ctx,
            "rename-plate",
            &tr!("plate-rename-title"),
            Size::Fixed(360.0),
            |ui, part| match part {
                Part::Body => {
                    ui.label(tr!("plate-rename-label"));
                    let field =
                        ui.add(egui::TextEdit::singleline(&mut name).desired_width(f32::INFINITY));
                    crate::widgets::name(&field, &tr!("plate-rename-label"));
                    if !field.has_focus() && !ui.memory(|m| m.has_focus(field.id)) {
                        field.request_focus();
                    }
                    if field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        done = Some(true);
                    }
                }
                Part::Footer { enter } => {
                    let valid = !name.trim().is_empty();
                    if dialog::main_action(ui, &tr!("plate-rename-action"), valid, enter) {
                        done = Some(true);
                    }
                    if dialog::cancel_action(ui) {
                        done = Some(false);
                    }
                }
            },
        );
        match done {
            Some(true) if !name.trim().is_empty() => {
                let name = name.trim().to_owned();
                if name != self.workspace.plate().name {
                    self.workspace.rename_plate(name);
                }
            }
            Some(_) => {}
            None if response.cancel => {}
            None => self.plates.renaming = Some(name),
        }
    }
}

/// A model row dropped on a plate (TD-135, phase 3): the object, the plate and whether Ctrl
/// was held, which copies it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ModelDrop {
    index: usize,
    to: PlateId,
    copy: bool,
}

/// What a model row dragged onto `response` (plate `id`'s card, tab or image) does: outlined
/// while it hovers there when the plate can take it (`takes`: not the plate shown), and
/// dropped there, the model to move or copy. `rounding` is the outline's corners.
fn model_drop(
    ui: &egui::Ui,
    response: &egui::Response,
    id: PlateId,
    takes: bool,
    rounding: f32,
) -> Option<ModelDrop> {
    if !takes {
        return None;
    }
    if response
        .dnd_hover_payload::<crate::sidebar::DraggedModel>()
        .is_some()
    {
        ui.painter().rect_stroke(
            response.rect,
            rounding,
            egui::Stroke::new(2.0, theme::INTERACTION.color()),
            egui::StrokeKind::Inside,
        );
    }
    let dragged = released::<crate::sidebar::DraggedModel>(response)?;
    Some(ModelDrop {
        index: dragged.0,
        to: id,
        copy: ui.input(|i| i.modifiers.command),
    })
}

/// What of type `T` is dropped on `response` this frame. Unlike egui's
/// `Response::dnd_release_payload`, which takes whatever is carried and drops it when it is of
/// another type, this leaves a payload of another type for the widget that takes it: a card
/// takes a plate dragged onto it, or a model row (owner, 2026-11-06: models dropped on the bar
/// were lost to the plates' check).
fn released<T: std::any::Any + Send + Sync>(
    response: &egui::Response,
) -> Option<std::sync::Arc<T>> {
    if egui::DragAndDrop::has_payload_of_type::<T>(&response.ctx) {
        response.dnd_release_payload::<T>()
    } else {
        None
    }
}

/// While a model row is dragged, a line beside the pointer saying where to drop it.
pub(super) fn model_drag_hint(ctx: &egui::Context, workspace: &crate::workspace::Workspace) {
    let Some(dragged) = egui::DragAndDrop::payload::<crate::sidebar::DraggedModel>(ctx) else {
        return;
    };
    let (Some(pointer), Some(object)) = (
        ctx.pointer_latest_pos(),
        workspace.plate().objects.get(dragged.0),
    ) else {
        return;
    };
    let copy = ctx.input(|i| i.modifiers.command);
    let text = if copy {
        tr!("plates-drag-copy", model = &object.name)
    } else {
        tr!("plates-drag-move", model = &object.name)
    };
    egui::Area::new(egui::Id::new("model-drag-hint"))
        .order(egui::Order::Tooltip)
        .fixed_pos(pointer + egui::vec2(16.0, 12.0))
        .interactable(false)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.label(text);
            });
        });
}

/// One plate's tab in the folded bar: its state's dot and its name.
fn tab(ui: &mut egui::Ui, name: &str, state: Shown, lit: bool) -> egui::Response {
    let font = egui::FontId::proportional(12.0);
    let galley = ui
        .painter()
        .layout_no_wrap(name.to_owned(), font, theme::TEXT_PRIMARY.color());
    let size = egui::vec2(galley.size().x + 30.0, 24.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
    plate_info(&response, name, state, lit);
    let painter = ui.painter_at(rect);
    let (fill, stroke) = if lit {
        (LIT_FILL, theme::ACCENT.color())
    } else {
        (theme::BACKGROUND.color(), theme::BORDER.color())
    };
    painter.rect(
        rect,
        12.0,
        fill,
        egui::Stroke::new(1.0, stroke),
        egui::StrokeKind::Inside,
    );
    painter.circle_filled(
        rect.left_center() + egui::vec2(12.0, 0.0),
        3.5,
        state.color(),
    );
    painter.galley(
        egui::pos2(rect.left() + 22.0, rect.center().y - galley.size().y / 2.0),
        galley,
        theme::TEXT_PRIMARY.color(),
    );
    response
}

/// A plate's card or tab to screen readers (TD-142): a button named by the plate and its slice
/// state, selected while the plate is the current one.
fn plate_info(response: &egui::Response, name: &str, state: Shown, lit: bool) {
    let label = tr!("a11y-plate", name = name, state = state.label());
    response
        .widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, lit, &label));
}

/// Keep the viewport's renderer reachable for thumbnails outside the viewport's own drawing.
pub(super) fn keep_render_state(window: &mut MainWindow, frame: &eframe::Frame) {
    if window.render_state.is_none() {
        window.render_state = frame.wgpu_render_state().cloned();
    }
}

#[cfg(test)]
mod tests {
    use super::released;
    use crate::sidebar::DraggedModel;
    use eframe::egui;
    use karasu_project::project::PlateId;

    #[test]
    fn a_model_dropped_on_a_card_is_not_lost_to_the_plates_check() {
        // Owner, 2026-11-06: a card checks for a plate dropped on it first; a model row
        // dropped there must reach the check that takes it.
        let ctx = egui::Context::default();
        let card = egui::Rect::from_min_size(egui::pos2(10.0, 10.0), egui::vec2(100.0, 50.0));
        let over = egui::pos2(50.0, 30.0);
        let input = |events: Vec<egui::Event>| egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(400.0, 300.0),
            )),
            events,
            ..Default::default()
        };
        let mut taken = (None, None);
        let frames = [
            vec![egui::Event::PointerMoved(over)],
            vec![egui::Event::PointerButton {
                pos: over,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        ];
        for (k, events) in frames.into_iter().enumerate() {
            let mut output = ctx.run_ui(input(events), |ui| {
                if k == 0 {
                    egui::DragAndDrop::set_payload(ui.ctx(), DraggedModel(3));
                }
                let response =
                    ui.interact(card, egui::Id::new("card"), egui::Sense::click_and_drag());
                if k == 1 {
                    taken = (
                        released::<PlateId>(&response).map(|p| *p),
                        released::<DraggedModel>(&response).map(|m| *m),
                    );
                }
            });
            output.textures_delta.clear();
        }
        assert_eq!(taken, (None, Some(DraggedModel(3))));
    }
}
