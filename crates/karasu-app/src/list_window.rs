//! The list windows (owner, 2026-10-29, from the canvas mockup "List windows"): a tool's items
//! of the selected model (its supports, its drain holes) in a tool window of their own, opened
//! from the tool strip, so the tool's panel keeps its values however many items there are.
//!
//! A list window is a tool window (`docs/UI-DESIGN.md`, "Windows"): not modal, moved by its
//! title bar, resized from its corner, and opened where and as large as it was left. Only the
//! rows in view are laid out (`ScrollArea::show_rows`, rows of one height), so its cost per
//! frame follows its height, not the number of items. A choice shows every item or only those
//! with a problem (or of another kind the list names, such as the mini supports). A click on a row selects its item, a double click also brings the view to
//! it, and its bin deletes it. When the item is selected elsewhere (in the scene), the list
//! scrolls to its row once.
use crate::{
    icons::{Icon, Icons},
    widgets::choice,
};
use eframe::egui;

/// The height of a row, in points.
const ROW_HEIGHT: f32 = 26.0;
/// The width of the first column (the dot and the name), in points.
const NAME_WIDTH: f32 = 72.0;
/// The width of the state column, in points.
const STATE_WIDTH: f32 = 120.0;
/// The room around a row's bin icon, in points: a 16-point icon in a row 26 points high.
const BIN_INSET: f32 = 5.0;

/// What a list window shows around its rows.
pub struct Spec<'a> {
    /// Its identity, under which its place is remembered.
    pub id: &'a str,
    /// Its title: what it lists and how many.
    pub title: String,
    /// The second column's heading (what the item is); the third's is the state.
    pub fact: String,
    /// The state column's heading.
    pub state: String,
    /// Tabs above the choice, when the window lists more than one kind of item (the drain
    /// holes and the suction cups); empty without.
    pub tabs: Vec<String>,
    /// The choice's labels: every item, then only those with a problem, and any other filter
    /// the list has (the mini supports, owner R8 2026-11-10).
    pub filters: Vec<String>,
    /// A second choice's labels, under the first, when the list has one (the islands' places:
    /// everywhere, outside, in the cavity); empty without.
    pub places: Vec<String>,
    /// The line under the rows.
    pub summary: String,
    /// The bin's tooltip; `None` for items that cannot be deleted from the list, which then
    /// shows no bin.
    pub remove: Option<String>,
}

/// One row, built only while it is in view.
pub struct Row {
    /// The dot, in the item's colour in the scene.
    pub dot: egui::Color32,
    /// Its name.
    pub name: String,
    /// What it is.
    pub fact: String,
    /// Its state, in `state_colour`.
    pub state: String,
    pub state_colour: egui::Color32,
    /// Its tooltip: its findings, or what a click does.
    pub hint: String,
    /// It is the selected item.
    pub selected: bool,
}

/// What the user did in a list window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event<T> {
    /// A row clicked: select its item.
    Picked(T),
    /// A row double-clicked: select its item and bring the view to it.
    Focused(T),
    /// A row's bin: delete its item.
    Removed(T),
}

/// Show list window `spec` when `open` (its close button clears it), on tab `tab` when `spec`
/// has tabs (the caller lists that tab's items): `items`, in order, of which `filter` (the place
/// of the filter chosen among `spec`'s) keeps some and `place` (the second choice's, when `spec`
/// has one) those of a place, the caller filtering both, each drawn by `row` while in view; the
/// list scrolls once to `reveal`'s row. Returns what the user did.
#[allow(clippy::too_many_arguments)] // The window's whole state.
pub fn show<T: Copy + PartialEq>(
    ctx: &egui::Context,
    spec: &Spec<'_>,
    open: &mut bool,
    tab: &mut usize,
    filter: &mut usize,
    place: &mut usize,
    items: &[T],
    reveal: Option<T>,
    mut row: impl FnMut(T) -> Row,
) -> Option<Event<T>> {
    if !*open {
        return None;
    }
    let mut event = None;
    crate::dialog::tool_window(ctx, spec.id, spec.title.as_str(), egui::vec2(340.0, 440.0))
        .open(open)
        .min_size(egui::vec2(280.0, 200.0))
        .show(ctx, |ui| {
            crate::dialog::remember_tool_window(ui, spec.id);
            // The window's whole area, laid out by hand and claimed exactly: what the content
            // claims would otherwise feed back into the window's size, which grows to fit it,
            // frame after frame (as the dialogs', see `dialog::show`).
            let whole = ui.available_rect_before_wrap();
            let mut inner = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(whole)
                    .layout(egui::Layout::top_down(egui::Align::Min)),
            );
            inner.set_clip_rect(whole.intersect(ui.clip_rect()));
            event = body(
                &mut inner,
                spec,
                (tab, filter, place),
                items,
                reveal,
                &mut row,
            );
            ui.advance_cursor_after_rect(whole);
        });
    event
}

/// The window's content in `ui`, which it fills: the choice, the headings, the rows in view,
/// and the summary at the bottom. Returns what the user did.
fn body<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    spec: &Spec<'_>,
    (tab, filter, place): (&mut usize, &mut usize, &mut usize),
    items: &[T],
    reveal: Option<T>,
    row: &mut impl FnMut(T) -> Row,
) -> Option<Event<T>> {
    let mut event = None;
    if !spec.tabs.is_empty() {
        choice(ui, tab, &spec.tabs);
        ui.add_space(2.0);
    }
    choice(ui, filter, &spec.filters);
    if !spec.places.is_empty() {
        ui.add_space(2.0);
        choice(ui, place, &spec.places);
    }
    ui.add_space(4.0);
    header(ui, spec);
    ui.separator();
    // The summary keeps the bottom line, its own height; the rows take what is left above it.
    let footer = footer_rect(ui.max_rect(), ui.text_style_height(&egui::TextStyle::Body));
    let gap = ui.spacing().item_spacing.y;
    let height = (footer.top() - 2.0 * gap - ui.cursor().top()).max(ROW_HEIGHT);
    let mut area = egui::ScrollArea::vertical()
        .id_salt((spec.id, "rows"))
        .auto_shrink([false, false])
        .max_height(height);
    if let Some(k) = reveal.and_then(|r| items.iter().position(|&i| i == r)) {
        #[allow(clippy::cast_precision_loss)] // A row's place, in points.
        let top = k as f32 * (ROW_HEIGHT + ui.spacing().item_spacing.y);
        area = area.vertical_scroll_offset((top - height / 2.0).max(0.0));
    }
    area.show_rows(ui, ROW_HEIGHT, items.len(), |ui, range| {
        let icons = Icons::shared(ui.ctx());
        for k in range {
            let item = items[k];
            if let Some(e) = line(ui, icons.as_deref(), &row(item), spec.remove.as_deref()) {
                event = Some(match e {
                    Line::Picked => Event::Picked(item),
                    Line::Focused => Event::Focused(item),
                    Line::Removed => Event::Removed(item),
                });
            }
        }
    });
    let rule = footer.top() - gap;
    ui.painter().hline(
        footer.x_range(),
        rule,
        ui.visuals().widgets.noninteractive.bg_stroke,
    );
    let mut summary = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(footer)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    summary.add(
        egui::Label::new(egui::RichText::new(&spec.summary).color(ui.visuals().weak_text_color()))
            .truncate(),
    );
    event
}

/// The summary's line at the bottom of `whole`, a line of text `text_height` high with a
/// little room under it, never above `whole`'s top.
fn footer_rect(whole: egui::Rect, text_height: f32) -> egui::Rect {
    let top = (whole.bottom() - text_height - 4.0).max(whole.top());
    egui::Rect::from_min_max(egui::pos2(whole.left(), top), whole.max)
}

/// Where a row's columns start in a row from `left`, `width` wide: the name (after the dot),
/// what the item is, and its state; the bin takes the end.
fn columns(left: f32, width: f32) -> [f32; 3] {
    let fact_room = (width - NAME_WIDTH - STATE_WIDTH - ROW_HEIGHT).max(20.0);
    [
        left + 20.0,
        left + NAME_WIDTH,
        left + NAME_WIDTH + fact_room,
    ]
}

/// The columns' headings, weak, over the rows.
/// They start where the rows' text does, each cut at the next column.
fn header(ui: &mut egui::Ui, spec: &Spec<'_>) {
    let font = egui::TextStyle::Small.resolve(ui.style());
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(width, ui.text_style_height(&egui::TextStyle::Small) + 2.0),
        egui::Sense::hover(),
    );
    let colour = ui.visuals().weak_text_color();
    let starts = columns(rect.left(), width);
    let ends = [starts[1], starts[2], rect.right() - ROW_HEIGHT];
    for ((text, start), end) in ["#", &spec.fact, &spec.state]
        .into_iter()
        .zip(starts)
        .zip(ends)
    {
        let clip = egui::Rect::from_min_max(
            egui::pos2(start, rect.top()),
            egui::pos2((end - 6.0).max(start), rect.bottom()),
        );
        ui.painter().with_clip_rect(clip).text(
            egui::pos2(start, rect.center().y),
            egui::Align2::LEFT_CENTER,
            text,
            font.clone(),
            colour,
        );
    }
}

/// A row's bin (owner, 2026-10-29): the row's whole height, the icon about 16 points, so it
/// reads at a glance; lit under the pointer.
fn bin_button(ui: &mut egui::Ui, icons: Option<&Icons>, hint: &str) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(egui::Vec2::splat(ROW_HEIGHT), egui::Sense::click());
    let response = response.on_hover_text(hint);
    if response.hovered() && ui.is_enabled() {
        ui.painter()
            .rect_filled(rect, 4.0, ui.visuals().widgets.hovered.weak_bg_fill);
    }
    if let Some(icons) = icons {
        icons.paint(
            ui.painter(),
            Icon::Delete,
            rect.shrink(BIN_INSET),
            ui.visuals().text_color(),
        );
    }
    response
}

/// What a click on a row did.
enum Line {
    Picked,
    Focused,
    Removed,
}

/// One row: its dot and name, what it is, its state and its bin, one height whatever the text.
fn line(ui: &mut egui::Ui, icons: Option<&Icons>, row: &Row, remove: Option<&str>) -> Option<Line> {
    let width = ui.available_width();
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(width, ROW_HEIGHT), egui::Sense::click());
    let bin = egui::Rect::from_min_size(
        egui::pos2(rect.right() - ROW_HEIGHT, rect.top()),
        egui::Vec2::splat(ROW_HEIGHT),
    );
    let visuals = ui.visuals();
    let fill = if row.selected {
        visuals.selection.bg_fill
    } else if response.hovered() {
        visuals.widgets.hovered.weak_bg_fill
    } else {
        egui::Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, 4.0, fill);
    let painter = ui.painter_at(rect);
    let font = egui::TextStyle::Body.resolve(ui.style());
    let middle = rect.center().y;
    painter.circle_filled(egui::pos2(rect.left() + 10.0, middle), 4.5, row.dot);
    let text = |at: f32, room: f32, text: &str, colour: egui::Color32| {
        let galley = ui
            .painter()
            .layout(text.to_owned(), font.clone(), colour, f32::INFINITY);
        let clip = egui::Rect::from_min_max(
            egui::pos2(at, rect.top()),
            egui::pos2((at + room).min(bin.left()), rect.bottom()),
        );
        ui.painter().with_clip_rect(clip).galley(
            egui::pos2(at, middle - galley.size().y / 2.0),
            galley,
            colour,
        );
    };
    let strong = visuals.strong_text_color();
    let plain = visuals.text_color();
    let [name, fact, state] = columns(rect.left(), width);
    text(name, fact - name - 2.0, &row.name, strong);
    text(fact, state - fact - 6.0, &row.fact, plain);
    text(state, STATE_WIDTH - 6.0, &row.state, row.state_colour);
    let removed = remove.is_some_and(|hint| {
        ui.put(bin, |ui: &mut egui::Ui| bin_button(ui, icons, hint))
            .clicked()
    });
    let response = response.on_hover_text(&row.hint);
    if removed {
        Some(Line::Removed)
    } else if response.double_clicked() {
        Some(Line::Focused)
    } else if response.clicked() {
        Some(Line::Picked)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Draw one row at the top left of an 800 × 600 screen over four frames, pressing the
    /// pointer at `at` `clicks` times in the last ones: what the row reported.
    fn press(at: egui::Pos2, clicks: usize) -> Option<Line> {
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        let row = Row {
            dot: egui::Color32::WHITE,
            name: "#1".to_owned(),
            fact: "On the plate".to_owned(),
            state: "OK".to_owned(),
            state_colour: egui::Color32::WHITE,
            hint: String::new(),
            selected: false,
        };
        let mut frames = vec![Vec::new(), vec![egui::Event::PointerMoved(at)]];
        for _ in 0..clicks {
            for pressed in [true, false] {
                frames.push(vec![egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                }]);
            }
        }
        let mut seen = None;
        for events in frames {
            let input = egui::RawInput {
                events,
                screen_rect: Some(screen),
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ui| {
                egui::Area::new(egui::Id::new("row"))
                    .fixed_pos(egui::Pos2::ZERO)
                    .show(ui.ctx(), |ui| {
                        ui.set_width(400.0);
                        if let Some(line) = line(ui, None, &row, Some("Delete")) {
                            seen = Some(line);
                        }
                    });
            });
            output.textures_delta.clear();
        }
        seen
    }

    #[test]
    fn the_summary_keeps_a_whole_line_at_the_bottom() {
        let whole = egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(300.0, 400.0));
        let footer = footer_rect(whole, 16.0);
        assert!(whole.contains_rect(footer));
        assert!(footer.height() >= 16.0);
        assert!((footer.bottom() - whole.bottom()).abs() < f32::EPSILON);
        // In a window too low for it, it stays inside.
        let low = egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(300.0, 8.0));
        assert!(low.contains_rect(footer_rect(low, 16.0)));
        // The headings start where the rows' text does.
        assert_eq!(columns(10.0, 300.0)[1], 10.0 + NAME_WIDTH);
    }

    #[test]
    fn the_window_keeps_its_size_while_the_pointer_moves() {
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 900.0));
        let spec = Spec {
            id: "sizes",
            title: "Supports (40)".to_owned(),
            fact: "Stands".to_owned(),
            state: "State".to_owned(),
            filters: vec!["All".to_owned(), "With errors (0)".to_owned()],
            tabs: Vec::new(),
            places: vec!["Everywhere".to_owned(), "Outside (40)".to_owned()],
            summary: "40 supports".to_owned(),
            remove: Some("Delete".to_owned()),
        };
        let items: Vec<u64> = (1..=40).collect();
        let (mut open, mut only, mut place) = (true, 0, 0);
        let mut sizes = Vec::new();
        for frame in 0..40 {
            #[allow(clippy::cast_precision_loss)] // A few frames.
            let at = egui::pos2(500.0 + frame as f32 * 3.0, 400.0);
            let input = egui::RawInput {
                events: vec![egui::Event::PointerMoved(at)],
                screen_rect: Some(screen),
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ui| {
                let row = |id: u64| Row {
                    dot: egui::Color32::WHITE,
                    name: format!("#{id}"),
                    fact: "On the plate".to_owned(),
                    state: "OK".to_owned(),
                    state_colour: egui::Color32::WHITE,
                    hint: String::new(),
                    selected: false,
                };
                let _ = show(
                    ui.ctx(),
                    &spec,
                    &mut open,
                    &mut 0,
                    &mut only,
                    &mut place,
                    &items,
                    None,
                    row,
                );
            });
            output.textures_delta.clear();
            if frame > 2 {
                sizes.push(
                    ctx.memory(|m| m.area_rect(egui::Id::new("sizes")))
                        .unwrap()
                        .size(),
                );
            }
        }
        assert!(sizes.windows(2).all(|w| w[0] == w[1]), "{sizes:?}");
    }

    #[test]
    fn a_row_is_picked_by_a_click_brought_into_view_by_two_and_deleted_by_its_bin() {
        let middle = egui::pos2(150.0, ROW_HEIGHT / 2.0);
        assert!(matches!(press(middle, 1), Some(Line::Picked)));
        assert!(matches!(press(middle, 2), Some(Line::Focused)));
        let bin = egui::pos2(400.0 - ROW_HEIGHT / 2.0, ROW_HEIGHT / 2.0);
        assert!(matches!(press(bin, 1), Some(Line::Removed)));
        assert!(press(egui::pos2(150.0, 200.0), 1).is_none());
    }
}
