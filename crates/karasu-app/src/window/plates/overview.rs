//! The plates' overview (TD-135, phase 2, from the owner-approved mockup "karasu multiple plates
//! mockup"): a tool window of cards, one per plate in project order, opened from the plates bar's
//! All plates… or the Plate menu.
//!
//! Each card shows the plate's thumbnail from above, larger than the bar's, with where its slice
//! stands and the box its models cover; its number and name; its printer, resin and print
//! setting; its models; and its last slice's layers and height, print time and resin, faded
//! when the plate has changed since. Open shows the plate, Duplicate copies it after itself,
//! Export writes its last slice (only while it is of the plate as it is), Delete deletes it
//! (undoable; the last plate stays). A card's image dragged onto another's moves the plate there,
//! a double click shows it. The last card adds a plate. The footer totals the plates: their
//! models, how many are sliced as they are, and those slices' print time and resin.
//!
//! **Cost.** The window lays out one card per plate while open; its thumbnails are drawn only
//! while it is open, one a frame at most, and again only when their plate's revision changes.
use super::{Kind, Shown, LIT_FILL};
use crate::{
    actions::Action,
    i18n::{decimal, tr},
    icons::{Icon, Icons},
    slicing_ui::duration,
    theme,
    widgets::outlined_icon_button,
    window::MainWindow,
};
use eframe::egui;
use karasu_project::project::PlateId;

/// The window's identity, under which its place is remembered.
const ID: &str = "plates-overview";
/// A card's size, in points.
const CARD: egui::Vec2 = egui::vec2(280.0, 344.0);
/// The room between cards and around them, in points.
const GAP: f32 = 14.0;
/// The room inside a card, in points.
const PADDING: f32 = 12.0;
/// A card's image, in points: the overview's thumbnails at half their pixels.
const IMAGE: egui::Vec2 = egui::vec2(256.0, 144.0);
/// The height of a fact's row, in points: a line of small text.
const FACT_ROW: f32 = 15.0;
/// The opacity of a slice's facts older than the plate (as the Slice panel shows them).
const STALE: f32 = 0.55;

/// What the user did in the overview.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Event {
    Open(PlateId),
    Duplicate(PlateId),
    Export(PlateId),
    Delete(PlateId),
    Move(PlateId, usize),
    Transfer(super::ModelDrop),
    New,
    SliceEvery,
    ExportEvery,
    StopBatch,
}

impl MainWindow {
    /// The overview, while it is open.
    pub(in crate::window) fn plates_overview(&mut self, ctx: &egui::Context) {
        if !self.plates.overview {
            return;
        }
        self.refresh_thumbnail(ctx, Kind::Overview);
        let mut open = true;
        let mut event = None;
        let title = tr!(
            "plates-overview-title",
            count = self.workspace.plates().len()
        );
        let recentre = std::mem::take(&mut self.plates.overview_opened);
        crate::dialog::centred_tool_window(ctx, ID, title, egui::vec2(940.0, 640.0), recentre)
            .open(&mut open)
            .min_size(egui::vec2(CARD.x + 2.0 * GAP, 420.0))
            .show(ctx, |ui| {
                crate::dialog::remember_tool_window(ui, ID);
                // The window's whole area, laid out by hand and claimed exactly, as the list
                // windows do: what the content claims would otherwise grow the window.
                let whole = ui.available_rect_before_wrap();
                let mut inner = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(whole)
                        .layout(egui::Layout::top_down(egui::Align::Min)),
                );
                inner.set_clip_rect(whole.intersect(ui.clip_rect()));
                event = self.overview_body(&mut inner);
                ui.advance_cursor_after_rect(whole);
            });
        self.plates.overview = open;
        match event {
            Some(Event::Open(id)) => {
                self.workspace.show_plate(id);
            }
            Some(Event::Duplicate(id)) => self.duplicate_plate(id),
            Some(Event::Export(id)) => self.export_plate(id),
            Some(Event::Delete(id)) => self.ask_delete_plate(id),
            Some(Event::Move(id, to)) => self.workspace.move_plate(id, to),
            Some(Event::Transfer(drop)) => {
                self.transfer_object(drop.index, Some(drop.to), drop.copy);
            }
            Some(Event::New) => self.perform(Action::NewPlate),
            Some(Event::SliceEvery) => self.perform(Action::SliceEveryPlate),
            Some(Event::ExportEvery) => self.perform(Action::ExportEveryPlate),
            Some(Event::StopBatch) => self.cancel_batch(),
            None => return,
        }
        // What is drawn after the overview in this frame is of the plate shown now.
        self.follow_plate(ctx);
    }

    /// The window's content in `ui`, which it fills: the hint, the cards, and the totals at the
    /// bottom.
    fn overview_body(&self, ui: &mut egui::Ui) -> Option<Event> {
        let mut event = None;
        ui.label(
            egui::RichText::new(tr!("plates-overview-hint"))
                .small()
                .color(ui.visuals().weak_text_color()),
        );
        let line = ui.spacing().interact_size.y;
        let whole = ui.max_rect();
        let footer = egui::Rect::from_min_max(
            egui::pos2(whole.left(), (whole.bottom() - line - 6.0).max(whole.top())),
            whole.max,
        );
        let gap = ui.spacing().item_spacing.y;
        let height = (footer.top() - 2.0 * gap - ui.cursor().top()).max(CARD.y / 2.0);
        crate::focus::scrolled(
            egui::ScrollArea::vertical()
                .id_salt((ID, "cards"))
                .auto_shrink([false, false])
                .max_height(height),
            ui,
            |ui| {
                let columns = columns(ui.available_width());
                let icons = Icons::shared(ui.ctx());
                let plates = self.workspace.plates();
                // Every plate's card, then the New plate card.
                let mut cells = (0..=plates.len()).peekable();
                ui.add_space(GAP / 2.0);
                while cells.peek().is_some() {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = GAP;
                        for index in cells.by_ref().take(columns) {
                            let made = if index < plates.len() {
                                self.overview_card(ui, icons.as_deref(), index)
                            } else {
                                new_card(ui, icons.as_deref())
                            };
                            event = event.or(made);
                        }
                    });
                    ui.add_space(GAP - ui.spacing().item_spacing.y);
                }
            },
        );
        ui.painter().hline(
            footer.x_range(),
            footer.top() - gap,
            ui.visuals().widgets.noninteractive.bg_stroke,
        );
        // The footer's actions at the right, then the totals, or the batch's progress, in the
        // room left.
        let mut totals = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(footer)
                .layout(egui::Layout::right_to_left(egui::Align::Center)),
        );
        let idle = self.plates.batch.is_none();
        if idle {
            if totals
                .button(Action::ExportEveryPlate.label())
                .on_hover_text(tr!("action-export-every-plate-hint"))
                .clicked()
            {
                event = Some(Event::ExportEvery);
            }
            if totals
                .button(Action::SliceEveryPlate.label())
                .on_hover_text(tr!("action-slice-every-plate-hint"))
                .clicked()
            {
                event = Some(Event::SliceEvery);
            }
        } else if totals.button(tr!("batch-stop")).clicked() {
            event = Some(Event::StopBatch);
        }
        let line = self
            .batch_status()
            .unwrap_or_else(|| self.overview_totals());
        totals.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
            ui.add(
                egui::Label::new(egui::RichText::new(line).color(ui.visuals().weak_text_color()))
                    .truncate(),
            );
        });
        event
    }

    /// The footer's totals: every plate's models, how many plates are sliced as they are, and
    /// those slices' print time and resin.
    fn overview_totals(&self) -> String {
        let plates = self.workspace.plates();
        let models: usize = plates.iter().map(|p| p.content().objects.len()).sum();
        let sliced: Vec<_> = plates
            .iter()
            .filter_map(|p| self.current_facts(p.id()))
            .collect();
        tr!(
            "plates-overview-totals",
            models = models,
            sliced = sliced.len(),
            plates = plates.len(),
            time = duration(sliced.iter().map(|f| f.time_s).sum()),
            volume = decimal(sliced.iter().map(|f| f.resin_ml).sum(), 1),
        )
    }

    /// Plate `index`'s card. Returns what the user did on it.
    fn overview_card(
        &self,
        ui: &mut egui::Ui,
        icons: Option<&Icons>,
        index: usize,
    ) -> Option<Event> {
        let plate = &self.workspace.plates()[index];
        let (id, content) = (plate.id(), plate.content());
        let lit = id == self.workspace.plate_id();
        let state = self.plate_slice(id);
        let mut event = None;
        let (rect, _) = ui.allocate_exact_size(CARD, egui::Sense::hover());
        let (fill, stroke) = if lit {
            (LIT_FILL, theme::ACCENT.color())
        } else {
            (theme::BACKGROUND.color(), theme::BORDER.color())
        };
        ui.painter().rect(
            rect,
            10.0,
            fill,
            egui::Stroke::new(1.0, stroke),
            egui::StrokeKind::Inside,
        );
        let mut card = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(rect.shrink(PADDING))
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        card.spacing_mut().item_spacing.y = 6.0;
        // The image: dragged onto another card's, it moves the plate there.
        let (image, response) = card.allocate_exact_size(IMAGE, egui::Sense::click_and_drag());
        super::plate_info(&response, &content.name, state, lit);
        self.overview_image(&card, image, id, state);
        let response =
            response.on_hover_text(tr!("plates-overview-image-hint", name = &content.name));
        if response.double_clicked() {
            event = Some(Event::Open(id));
        }
        response.dnd_set_drag_payload(id);
        if let Some(dragged) = super::released::<PlateId>(&response) {
            if *dragged != id {
                event = Some(Event::Move(*dragged, index));
            }
        }
        if let Some(drop) = super::model_drop(&card, &response, id, !lit, 8.0) {
            event = Some(Event::Transfer(drop));
        }
        card.add(
            egui::Label::new(
                egui::RichText::new(tr!(
                    "plates-overview-name",
                    number = index + 1,
                    name = &content.name
                ))
                .strong()
                .size(15.0),
            )
            .truncate(),
        );
        let dash = || "–".to_owned();
        let profiles = tr!(
            "plates-overview-profiles",
            printer = content
                .printer
                .as_ref()
                .map_or_else(dash, |p| p.name.clone()),
            resin = content.resin.as_ref().map_or_else(dash, |r| r.name.clone()),
            setting = content.setting().map_or_else(dash, |s| s.name.clone()),
        );
        card.add(
            egui::Label::new(
                egui::RichText::new(profiles)
                    .small()
                    .color(card.visuals().weak_text_color()),
            )
            .truncate(),
        );
        self.overview_facts(&mut card, id, content.objects.len(), state);
        event = event.or(self.overview_actions(&mut card, icons, rect, id, lit));
        event
    }

    /// A card's facts in `ui`: plate `id`'s `models` and its last slice's, faded when the
    /// plate has changed since (its slice `state`).
    fn overview_facts(&self, ui: &mut egui::Ui, id: PlateId, models: usize, state: Shown) {
        let dash = || "–".to_owned();
        let facts = self
            .slicing_of(id)
            .and_then(crate::slicing_ui::SlicingUi::slice_facts);
        let faded = facts.is_some() && state != Shown::Sliced;
        egui::Grid::new((ID, "facts", id.0))
            .num_columns(2)
            .spacing(egui::vec2(12.0, 2.0))
            // Rows one line of small text high, not a control's height.
            .min_row_height(FACT_ROW)
            .min_col_width((IMAGE.x - 12.0) / 2.0)
            .show(ui, |ui| {
                let weak = ui.visuals().weak_text_color();
                let row = |ui: &mut egui::Ui, label: String, value: String, faded: bool| {
                    ui.label(egui::RichText::new(label).small().color(weak));
                    let colour = if faded {
                        theme::TEXT_PRIMARY.color().gamma_multiply(STALE)
                    } else {
                        theme::TEXT_PRIMARY.color()
                    };
                    ui.label(egui::RichText::new(value).small().color(colour));
                    ui.end_row();
                };
                row(ui, tr!("plates-overview-models"), models.to_string(), false);
                row(
                    ui,
                    tr!("plates-overview-layers"),
                    facts.map_or_else(dash, |f| {
                        tr!(
                            "plates-overview-layers-value",
                            layers = f.layers,
                            height = decimal(f.height_mm, 1)
                        )
                    }),
                    faded,
                );
                row(
                    ui,
                    tr!("slice-time"),
                    facts.map_or_else(dash, |f| duration(f.time_s)),
                    faded,
                );
                row(
                    ui,
                    tr!("slice-resin"),
                    facts.map_or_else(dash, |f| {
                        tr!("slice-volume-ml", volume = decimal(f.resin_ml, 1))
                    }),
                    faded,
                );
            });
    }

    /// A card's actions along the bottom of its `rect`, in its `ui`: Open (but for the plate
    /// shown, `lit`), Duplicate, Export and Delete. Returns the one clicked.
    fn overview_actions(
        &self,
        ui: &mut egui::Ui,
        icons: Option<&Icons>,
        rect: egui::Rect,
        id: PlateId,
        lit: bool,
    ) -> Option<Event> {
        let mut event = None;
        // The actions at the card's foot.
        let side = ui.spacing().interact_size.y;
        let bottom = rect.bottom() - PADDING;
        let actions = egui::Rect::from_min_max(
            egui::pos2(rect.left() + PADDING, bottom - side),
            egui::pos2(rect.right() - PADDING, bottom),
        );
        let mut row = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(actions)
                .layout(egui::Layout::right_to_left(egui::Align::Center)),
        );
        row.spacing_mut().item_spacing.x = 6.0;
        let several = self.workspace.plates().len() > 1;
        let delete = row
            .add_enabled_ui(several, |ui| {
                outlined_icon_button(ui, icons, Icon::Delete, &Action::DeletePlate.label())
            })
            .inner
            .on_disabled_hover_text(tr!("plates-overview-last"));
        if delete.clicked() {
            event = Some(Event::Delete(id));
        }
        let exportable = self.current_facts(id).is_some();
        let export = row
            .add_enabled_ui(exportable, |ui| {
                outlined_icon_button(ui, icons, Icon::Export, &tr!("plates-overview-export"))
            })
            .inner
            .on_disabled_hover_text(tr!("action-export-disabled"));
        if export.clicked() {
            event = Some(Event::Export(id));
        }
        if outlined_icon_button(
            &mut row,
            icons,
            Icon::Duplicate,
            &Action::DuplicatePlate.label(),
        )
        .clicked()
        {
            event = Some(Event::Duplicate(id));
        }
        let open = row
            .add_enabled(
                !lit,
                egui::Button::new(egui::RichText::new(tr!("plates-overview-open")).strong())
                    .fill(theme::PRIMARY.color())
                    .min_size(egui::vec2(row.available_width(), side)),
            )
            .on_disabled_hover_text(tr!("plates-overview-shown"));
        if open.clicked() {
            event = Some(Event::Open(id));
        }
        event
    }

    /// A card's image in `rect`: plate `id`'s thumbnail, where its slice stands at the top
    /// right, and the box its models cover at the bottom left.
    fn overview_image(&self, ui: &egui::Ui, rect: egui::Rect, id: PlateId, state: Shown) {
        let painter = ui.painter_at(rect);
        let thumbnail = self.plates.thumbnails.get(&(id, Kind::Overview));
        painter.rect_filled(rect, 8.0, self.appearance.plate.color());
        if let Some(thumbnail) = thumbnail {
            painter.image(
                thumbnail.texture,
                rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
        painter.rect_stroke(
            rect,
            8.0,
            egui::Stroke::new(1.0, theme::SURFACE.color()),
            egui::StrokeKind::Inside,
        );
        let font = egui::FontId::proportional(11.0);
        let chip = painter.layout_no_wrap(state.label(), font.clone(), theme::BACKGROUND.color());
        let chip_rect = egui::Rect::from_min_size(
            egui::pos2(rect.right() - chip.size().x - 22.0, rect.top() + 10.0),
            chip.size() + egui::vec2(12.0, 4.0),
        );
        painter.rect_filled(chip_rect, 8.0, state.color());
        painter.galley(
            chip_rect.min + egui::vec2(6.0, 2.0),
            chip,
            theme::BACKGROUND.color(),
        );
        if let Some([low, high]) = thumbnail.and_then(|t| t.footprint) {
            let text = tr!(
                "plates-overview-footprint",
                width = decimal(high[0] - low[0], 0),
                depth = decimal(high[1] - low[1], 0)
            );
            let galley = painter.layout_no_wrap(text, font, theme::TEXT_PRIMARY.color());
            let at = egui::pos2(rect.left() + 10.0, rect.bottom() - 8.0 - galley.size().y);
            painter.rect_filled(
                egui::Rect::from_min_size(at, galley.size()).expand2(egui::vec2(5.0, 2.0)),
                4.0,
                theme::BACKGROUND.color().gamma_multiply(0.75),
            );
            painter.galley(at, galley, theme::TEXT_PRIMARY.color());
        }
    }
}

/// How many cards fit side by side in `width` points, one at least.
fn columns(width: f32) -> usize {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // A few columns.
    let fit = ((width + GAP) / (CARD.x + GAP)).floor().max(1.0) as usize;
    fit
}

/// The last card, which adds a plate. Returns `Event::New` when clicked.
fn new_card(ui: &mut egui::Ui, icons: Option<&Icons>) -> Option<Event> {
    let (rect, response) = ui.allocate_exact_size(CARD, egui::Sense::click());
    let response = response.on_hover_text(tr!("action-new-plate-hint"));
    let painter = ui.painter();
    let colour = if response.hovered() {
        theme::INTERACTION.color()
    } else {
        theme::BORDER.color()
    };
    let r = rect.shrink(0.5);
    let corners = [
        r.left_top(),
        r.right_top(),
        r.right_bottom(),
        r.left_bottom(),
        r.left_top(),
    ];
    painter.extend(egui::Shape::dashed_line(
        &corners,
        egui::Stroke::new(1.0, colour),
        6.0,
        4.0,
    ));
    let centre = rect.center();
    if let Some(icons) = icons {
        icons.paint(
            painter,
            Icon::Plus,
            egui::Rect::from_center_size(centre - egui::vec2(0.0, 24.0), egui::Vec2::splat(28.0)),
            theme::ACCENT.color(),
        );
    }
    painter.text(
        centre + egui::vec2(0.0, 6.0),
        egui::Align2::CENTER_TOP,
        Action::NewPlate.label(),
        egui::FontId::proportional(14.0),
        theme::ACCENT.color(),
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, Action::NewPlate.label())
    });
    response.clicked().then_some(Event::New)
}

#[cfg(test)]
mod tests {
    use super::{columns, CARD, GAP};

    #[test]
    fn as_many_cards_as_fit_side_by_side_and_one_at_least() {
        assert_eq!(columns(0.0), 1);
        assert_eq!(columns(CARD.x), 1);
        assert_eq!(columns(2.0 * CARD.x + GAP - 1.0), 1);
        assert_eq!(columns(2.0 * CARD.x + GAP), 2);
        assert_eq!(columns(3.0 * CARD.x + 2.0 * GAP + 50.0), 3);
    }
}
