//! Small widgets egui does not provide (M6.5): a switch, a value field with its unit, and the
//! rows tool panels lay their settings out with.
//!
//! **Names for assistive technology** (TD-142): every control a user can reach reports a name
//! to screen readers through AccessKit. A row names its control with its label ([`name`]);
//! a control drawn alone is named where it is drawn.
//!
//! **Rows keep panels to their width** (design rule, TD-098): text of free length is never on
//! one line with a control, as egui never wraps a horizontal row. A setting's checkbox takes the
//! whole width and wraps; its value goes on the line below, indented; a labelled value has its
//! label wrap in the column left of a fixed-width field.
use crate::{
    i18n::{decimal, tr},
    icons::{Icon, Icons},
    theme,
};
use eframe::egui;
use std::ops::RangeInclusive;

/// A value field's width, and its unit segment's, in points: the same everywhere, so units line
/// up in a column of fields.
pub const FIELD_WIDTH: f32 = 100.0;
const UNIT_WIDTH: f32 = 38.0;
/// The gap between a value field's frame and its text, in points.
const FIELD_INSET: f32 = 8.0;

/// A unit a value field shows. The value itself is always in the unit's base (millimetres for
/// lengths); a field only shows and reads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    Millimetre,
    SquareMillimetre,
    Degree,
    Percent,
    Second,
    /// A ratio, shown with a multiplication sign.
    Ratio,
    /// A speed in millimetres per second.
    MillimetrePerSecond,
    /// A speed in millimetres per minute.
    MillimetrePerMinute,
    /// A temperature in degrees Celsius.
    Celsius,
    /// A plain count, such as a layer number: no unit segment.
    Count,
}

impl Unit {
    /// The unit's symbol.
    #[must_use]
    pub fn symbol(self) -> String {
        match self {
            Self::Millimetre => tr!("unit-mm"),
            Self::SquareMillimetre => tr!("unit-mm2"),
            Self::Degree => tr!("unit-degree"),
            Self::Percent => tr!("unit-percent"),
            Self::Second => tr!("unit-second"),
            Self::Ratio => tr!("unit-ratio"),
            Self::MillimetrePerSecond => tr!("unit-mm-per-s"),
            Self::MillimetrePerMinute => tr!("unit-mm-per-min"),
            Self::Celsius => tr!("unit-celsius"),
            Self::Count => String::new(),
        }
    }
}

/// Give `response`'s control `label` as the name screen readers announce (TD-142), for a
/// control whose visible label is drawn apart from it, such as a row's. Nothing is done while
/// no assistive technology is attached.
pub fn name(response: &egui::Response, label: &str) {
    response
        .ctx
        .accesskit_node_builder(response.id, |node| node.set_label(label));
}

/// Give the control `id` the name `label` for screen readers, as [`name`] does, for a control
/// egui draws without handing back its response: a collapsing section's arrow, whose id is the
/// section's (`egui::collapsing_header::CollapsingState::id`).
pub fn name_id(ctx: &egui::Context, id: egui::Id, label: &str) {
    ctx.accesskit_node_builder(id, |node| node.set_label(label));
}

/// A value field: the value at the left, typed after a click (Enter or leaving the field keeps
/// it, Escape restores it, a value out of range is brought into it), stepped with the Up and
/// Down keys while focused; its unit at the right in a segment of its own, where a horizontal
/// drag changes the value. One frame and one height with the other controls, a fixed width.
pub struct UnitField<'a> {
    value: &'a mut f64,
    unit: Unit,
    range: RangeInclusive<f64>,
    step: f64,
    decimals: usize,
    width: f32,
}

impl<'a> UnitField<'a> {
    /// A field for `value` in `unit`, from 0 to 1000, stepped by 0.1 with 2 decimals.
    pub fn new(value: &'a mut f64, unit: Unit) -> Self {
        Self {
            value,
            unit,
            range: 0.0..=1000.0,
            step: 0.1,
            decimals: 2,
            width: FIELD_WIDTH,
        }
    }

    /// A width other than [`FIELD_WIDTH`], for a narrow column such as the section panel.
    #[must_use]
    pub fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    /// The values allowed.
    #[must_use]
    pub fn range(mut self, range: RangeInclusive<f64>) -> Self {
        self.range = range;
        self
    }

    /// The step of the keys, and of a drag per two points.
    #[must_use]
    pub fn step(mut self, step: f64) -> Self {
        self.step = step;
        self
    }

    /// The decimals shown.
    #[must_use]
    pub fn decimals(mut self, decimals: usize) -> Self {
        self.decimals = decimals;
        self
    }
}

/// Read a typed value, with a point or a comma as the decimal separator.
fn parse(text: &str) -> Option<f64> {
    text.trim()
        .replace(',', ".")
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())
}

impl egui::Widget for UnitField<'_> {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let size = egui::vec2(self.width, ui.spacing().interact_size.y);
        let (rect, response) = ui.allocate_exact_size(size, egui::Sense::hover());
        let id = response.id;
        // A count has no unit, and so no segment to drag.
        let unit_width = match self.unit {
            Unit::Count => 0.0,
            // Speeds have longer symbols.
            Unit::MillimetrePerSecond | Unit::MillimetrePerMinute => UNIT_WIDTH + 18.0,
            _ => UNIT_WIDTH,
        };
        let unit_rect =
            egui::Rect::from_min_max(egui::pos2(rect.right() - unit_width, rect.top()), rect.max);
        let value_rect =
            egui::Rect::from_min_max(rect.min, egui::pos2(unit_rect.left(), rect.bottom()));
        let clamp = |v: f64| v.clamp(*self.range.start(), *self.range.end());
        let before = *self.value;
        // The frame's fills go under the typed text: their places are kept now, filled below.
        let backdrop = [(); 3].map(|()| ui.painter().add(egui::Shape::Noop));
        // The unit's segment: a drag there changes the value. It takes no keyboard focus: the
        // typed value has it, with the Up and Down keys (TD-142).
        let drag = ui
            .interact(unit_rect, id.with("drag"), egui::Sense::DRAG)
            .on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
        if drag.dragged() {
            let delta = f64::from(drag.drag_delta().x) / 2.0;
            *self.value = clamp(*self.value + delta * self.step);
        }
        // The value: typed, kept in a buffer while the field has the keyboard.
        let text_id = id.with("text");
        let focused = ui.memory(|m| m.has_focus(text_id));
        let mut text = if focused {
            ui.data(|d| d.get_temp::<String>(text_id))
                .unwrap_or_else(|| decimal(*self.value, self.decimals))
        } else {
            decimal(*self.value, self.decimals)
        };
        let edit = egui::TextEdit::singleline(&mut text)
            .id(text_id)
            .frame(egui::Frame::NONE)
            .margin(egui::Margin::ZERO)
            .vertical_align(egui::Align::Center);
        // Inset from the frame: a frameless text edit takes no margin of its own.
        let text_rect = egui::Rect::from_min_max(
            egui::pos2(value_rect.left() + FIELD_INSET, value_rect.top()),
            egui::pos2(value_rect.right() - FIELD_INSET / 2.0, value_rect.bottom()),
        );
        let typed = ui.put(text_rect, edit);
        if typed.has_focus() {
            let (up, down) = ui.input_mut(|i| {
                (
                    i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp),
                    i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown),
                )
            });
            if up || down {
                let base = parse(&text).unwrap_or(*self.value);
                *self.value = clamp(base + if up { self.step } else { -self.step });
                text = decimal(*self.value, self.decimals);
            }
            ui.data_mut(|d| d.insert_temp(text_id, text.clone()));
        } else if typed.lost_focus() {
            if !ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                if let Some(v) = parse(&text) {
                    *self.value = clamp(v);
                }
            }
            ui.data_mut(|d| d.remove::<String>(text_id));
        }
        paint_field(ui, rect, unit_rect, backdrop, &typed, &self.unit.symbol());
        // The focus ring goes around the whole field, not only its text (TD-142).
        crate::focus::ring_around(ui.ctx(), typed.id, rect);
        // The typed text's response first: the field's identity is the control that takes the
        // focus, so naming it names what a screen reader reads.
        let mut response = typed.union(response).union(drag);
        #[allow(clippy::float_cmp)] // Any change, however small.
        if *self.value != before {
            response.mark_changed();
        }
        response
    }
}

/// A value field's frame: one fill and stroke around both parts (the stroke in the selection's
/// colour while typing), the unit's segment shaded apart behind a divider, and the unit. The
/// fills go in the `backdrop` places kept before the typed text, so they are under it.
fn paint_field(
    ui: &egui::Ui,
    rect: egui::Rect,
    unit_rect: egui::Rect,
    backdrop: [egui::layers::ShapeIdx; 3],
    typed: &egui::Response,
    symbol: &str,
) {
    let visuals = ui.visuals();
    let stroke = if typed.has_focus() {
        visuals.selection.stroke
    } else {
        visuals.widgets.inactive.bg_stroke
    };
    let radius = visuals.widgets.inactive.corner_radius;
    let right = egui::CornerRadius {
        nw: 0,
        sw: 0,
        ..radius
    };
    let painter = ui.painter();
    let [fill, unit_fill, divider] = backdrop;
    painter.set(
        fill,
        egui::Shape::rect_filled(rect, radius, visuals.extreme_bg_color),
    );
    if symbol.is_empty() {
        painter.rect_stroke(rect, radius, stroke, egui::StrokeKind::Inside);
        return;
    }
    painter.set(
        unit_fill,
        egui::Shape::rect_filled(unit_rect, right, theme::SURFACE.color()),
    );
    painter.set(
        divider,
        egui::Shape::line_segment(
            [unit_rect.left_top(), unit_rect.left_bottom()],
            visuals.widgets.inactive.bg_stroke,
        ),
    );
    painter.rect_stroke(rect, radius, stroke, egui::StrokeKind::Inside);
    let colour = if ui.is_enabled() {
        ui.style().interact(typed).text_color()
    } else {
        visuals.weak_text_color()
    };
    painter.text(
        unit_rect.center(),
        egui::Align2::CENTER_CENTER,
        symbol,
        egui::TextStyle::Body.resolve(ui.style()),
        colour,
    );
}

/// A choice among `labels`, as a rounded bar whose highlight slides to the chosen one (M6.5):
/// `chosen` is its index. Returns the bar's response, changed when a click chose another.
pub fn choice(ui: &mut egui::Ui, chosen: &mut usize, labels: &[String]) -> egui::Response {
    choice_among(ui, chosen, labels, &[])
}

/// A [`choice`] whose options at the indices in `unavailable` show disabled and take no click.
pub fn choice_among(
    ui: &mut egui::Ui,
    chosen: &mut usize,
    labels: &[String],
    unavailable: &[usize],
) -> egui::Response {
    let height = ui.spacing().interact_size.y + 6.0;
    let size = egui::vec2(ui.available_width(), height);
    let (rect, mut response) = ui.allocate_exact_size(size, egui::Sense::hover());
    #[allow(clippy::cast_precision_loss)] // A handful of choices.
    let count = labels.len().max(1) as f32;
    let cell = rect.width() / count;
    let enabled = ui.is_enabled();
    for (k, label) in labels.iter().enumerate() {
        #[allow(clippy::cast_precision_loss)] // A handful of choices.
        let left = rect.left() + cell * k as f32;
        let place =
            egui::Rect::from_min_size(egui::pos2(left, rect.top()), egui::vec2(cell, height));
        let segment = ui.interact(place, response.id.with(k), egui::Sense::click());
        // Each option is a radio button to screen readers, named by its text (TD-142).
        let open = enabled && !unavailable.contains(&k);
        segment.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::RadioButton, open, *chosen == k, label)
        });
        crate::focus::ring_around(ui.ctx(), segment.id, place.shrink(1.0));
        if segment.clicked() && *chosen != k && !unavailable.contains(&k) {
            *chosen = k;
            response.mark_changed();
        }
    }
    #[allow(clippy::cast_precision_loss)] // A handful of choices.
    let target = *chosen as f32;
    let at = ui
        .ctx()
        .animate_value_with_time(response.id.with("slide"), target, 0.18);
    let radius = height / 2.0;
    let painter = ui.painter();
    painter.rect(
        rect,
        radius,
        ui.visuals().extreme_bg_color,
        ui.visuals().widgets.inactive.bg_stroke,
        egui::StrokeKind::Inside,
    );
    let knob = egui::Rect::from_min_size(
        egui::pos2(rect.left() + cell * at, rect.top()),
        egui::vec2(cell, height),
    )
    .shrink(3.0);
    let fill = if enabled {
        theme::PRIMARY.color()
    } else {
        theme::BORDER.color()
    };
    painter.rect_filled(knob, radius - 3.0, fill);
    for (k, label) in labels.iter().enumerate() {
        #[allow(clippy::cast_precision_loss)] // A handful of choices.
        let centre = egui::pos2(rect.left() + cell * (k as f32 + 0.5), rect.center().y);
        let colour = if enabled && !unavailable.contains(&k) {
            theme::TEXT_PRIMARY.color()
        } else {
            ui.visuals().weak_text_color()
        };
        painter.text(
            centre,
            egui::Align2::CENTER_CENTER,
            label,
            egui::FontId::proportional(13.0),
            colour,
        );
    }
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::RadioGroup,
            enabled,
            labels.get(*chosen).map_or("", String::as_str),
        )
    });
    response
}

/// A small square button showing only `icon`, lit while `on` (a switch such as a lock, or
/// false for a plain button), with `hint` as its tooltip.
pub fn icon_button(
    ui: &mut egui::Ui,
    icons: Option<&Icons>,
    icon: Icon,
    on: bool,
    hint: &str,
) -> egui::Response {
    icon_button_framed(ui, icons, icon, on, hint, (false, None))
}

/// An [`icon_button`] on a surface of colour `surface` (a notification's): under the pointer it
/// takes a lighter shade of that colour instead of the interface's violet, so it belongs to the
/// surface it stands on (owner R7, 2026-11-10).
pub fn icon_button_on(
    ui: &mut egui::Ui,
    icons: Option<&Icons>,
    icon: Icon,
    hint: &str,
    surface: egui::Color32,
) -> egui::Response {
    icon_button_framed(
        ui,
        icons,
        icon,
        false,
        hint,
        (false, Some(hovered_on(surface))),
    )
}

/// The fill of a button under the pointer on a surface of colour `surface`: the surface's
/// colour a fifth of the way to white, which keeps primary text and icons readable on it.
pub fn hovered_on(surface: egui::Color32) -> egui::Color32 {
    surface.lerp_to_gamma(egui::Color32::WHITE, 0.2)
}

/// An [`icon_button`] outlined at rest, so it reads as a button among text and fields, as in
/// the library's rows and the layer steps.
pub fn outlined_icon_button(
    ui: &mut egui::Ui,
    icons: Option<&Icons>,
    icon: Icon,
    hint: &str,
) -> egui::Response {
    icon_button_framed(ui, icons, icon, false, hint, (true, None))
}

fn icon_button_framed(
    ui: &mut egui::Ui,
    icons: Option<&Icons>,
    icon: Icon,
    on: bool,
    hint: &str,
    (framed, hover): (bool, Option<egui::Color32>),
) -> egui::Response {
    let side = ui.spacing().interact_size.y;
    let (rect, response) = ui.allocate_exact_size(egui::Vec2::splat(side), egui::Sense::click());
    let response = response.on_hover_text(hint);
    let visuals = ui.style().interact_selectable(&response, on);
    let (fill, stroke) = if on {
        (
            theme::INTERACTION.color(),
            egui::Stroke::new(1.0, theme::ACCENT.color()),
        )
    } else if response.hovered() && ui.is_enabled() {
        match hover {
            Some(fill) => (fill, egui::Stroke::NONE),
            None => (visuals.weak_bg_fill, visuals.bg_stroke),
        }
    } else if framed {
        (
            egui::Color32::TRANSPARENT,
            egui::Stroke::new(1.0, theme::BORDER.color()),
        )
    } else {
        (egui::Color32::TRANSPARENT, egui::Stroke::NONE)
    };
    ui.painter()
        .rect(rect, 6.0, fill, stroke, egui::StrokeKind::Inside);
    if let Some(icons) = icons {
        let colour = if ui.is_enabled() {
            theme::TEXT_PRIMARY.color()
        } else {
            ui.visuals().weak_text_color()
        };
        icons.paint(ui.painter(), icon, rect.shrink(6.0), colour);
    }
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, ui.is_enabled(), on, hint)
    });
    response
}

/// A one-line text field with inner padding, one height with the other controls (owner,
/// 2026-10-26), `width` wide.
pub fn text_field(text: &mut String, width: f32) -> egui::TextEdit<'_> {
    egui::TextEdit::singleline(text)
        .margin(egui::Margin::symmetric(8, 4))
        .desired_width(width)
}

/// A setting with a value: its checkbox, whose text wraps across the row, then its value field
/// on the line below, indented and enabled while the setting is on. Returns whether either
/// changed.
pub fn check_value_row(
    ui: &mut egui::Ui,
    on: &mut bool,
    label: impl Into<egui::WidgetText>,
    field: impl FnOnce(&mut egui::Ui) -> egui::Response,
) -> bool {
    let label = label.into();
    let text = label.text().to_owned();
    let mut changed = ui.checkbox(on, label).changed();
    let enabled = *on;
    ui.indent(ui.next_auto_id(), |ui| {
        let response = ui.add_enabled_ui(enabled, field).inner;
        name(&response, &text);
        changed |= response.changed();
    });
    changed
}

/// A labelled value: the label wraps in the column left of the fixed-width field. Returns the
/// field's response.
pub fn value_row(
    ui: &mut egui::Ui,
    label: impl Into<egui::WidgetText>,
    field: impl FnOnce(&mut egui::Ui) -> egui::Response,
) -> egui::Response {
    let label = label.into();
    let text = label.text().to_owned();
    ui.horizontal(|ui| {
        let spacing = ui.spacing().item_spacing.x;
        let left = (ui.available_width() - FIELD_WIDTH - spacing).max(40.0);
        ui.allocate_ui_with_layout(
            egui::vec2(left, 0.0),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.set_width(left);
                ui.add(egui::Label::new(label).wrap());
            },
        );
        let response = field(ui);
        name(&response, &text);
        response
    })
    .inner
}

/// A tool's value's default (owner, 2026-11-02): what the tool starts from, as the settings
/// set it, and how it reads. A field given one shows a reset button while it differs.
#[derive(Clone, Debug, PartialEq)]
pub struct DefaultValue<T> {
    /// The default.
    pub value: T,
    /// The default as the reset button's tooltip names it.
    pub shown: String,
}

impl<T> DefaultValue<T> {
    /// `value`, read as `shown`.
    pub fn new(value: T, shown: impl Into<String>) -> Self {
        Self {
            value,
            shown: shown.into(),
        }
    }
}

/// `value` with `digits` decimals and `unit`'s symbol, as a default's tooltip names it.
#[must_use]
pub fn with_unit(value: f64, digits: usize, unit: Unit) -> String {
    let symbol = unit.symbol();
    if symbol.is_empty() {
        decimal(value, digits)
    } else {
        format!("{} {symbol}", decimal(value, digits))
    }
}

/// A length's default, read with `digits` decimals and `unit`.
#[must_use]
pub fn length_default(value: f64, digits: usize, unit: Unit) -> DefaultValue<f64> {
    DefaultValue::new(value, with_unit(value, digits, unit))
}

/// The default of a value that may be automatic (`None`), read with two decimals and `unit`.
#[must_use]
pub fn auto_default(value: Option<f64>, unit: Unit) -> DefaultValue<Option<f64>> {
    let shown = value.map_or_else(|| tr!("prepare-auto"), |v| with_unit(v, 2, unit));
    DefaultValue::new(value, shown)
}

/// A switch's default.
#[must_use]
pub fn switch_default(on: bool) -> DefaultValue<bool> {
    DefaultValue::new(on, on_off(on))
}

/// On or off, as a switch's default reads.
#[must_use]
pub fn on_off(on: bool) -> String {
    if on {
        tr!("value-on")
    } else {
        tr!("value-off")
    }
}

/// The reset button of a value that `differs` from its default, `shown` (owner, 2026-11-02):
/// the reset icon, its tooltip naming the default; while the value is the default, the same
/// room, empty, so the fields of a panel stay aligned. Returns whether it was clicked.
pub fn reset_button(ui: &mut egui::Ui, differs: bool, shown: &str) -> bool {
    if !differs {
        let side = ui.spacing().interact_size.y;
        ui.allocate_exact_size(egui::Vec2::splat(side), egui::Sense::hover());
        return false;
    }
    let icons = Icons::shared(ui.ctx());
    icon_button(
        ui,
        icons.as_deref(),
        Icon::Reset,
        false,
        &tr!("reset-to-default", value = shown),
    )
    .clicked()
}

/// A [`value_row`] for `value`, with a reset button before its field while it differs from
/// `default` (none without one): `field` draws the field for the value. Returns whether the
/// value changed, by the field or the reset.
pub fn default_row<T: PartialEq + Clone>(
    ui: &mut egui::Ui,
    label: impl Into<egui::WidgetText>,
    value: &mut T,
    default: Option<&DefaultValue<T>>,
    field: impl FnOnce(&mut egui::Ui, &mut T) -> egui::Response,
) -> bool {
    let Some(default) = default else {
        return value_row(ui, label, |ui| field(ui, value)).changed();
    };
    let label = label.into();
    let text = label.text().to_owned();
    let mut reset = false;
    let changed = ui
        .horizontal(|ui| {
            let spacing = ui.spacing().item_spacing.x;
            let side = ui.spacing().interact_size.y;
            let left = (ui.available_width() - FIELD_WIDTH - side - 2.0 * spacing).max(40.0);
            ui.allocate_ui_with_layout(
                egui::vec2(left, 0.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_width(left);
                    ui.add(egui::Label::new(label).wrap());
                },
            );
            reset = reset_button(ui, *value != default.value, &default.shown);
            let response = field(ui, value);
            name(&response, &text);
            response.changed()
        })
        .inner;
    if reset {
        *value = default.value.clone();
    }
    changed || reset
}

/// A switch with its label and hint, and a reset button at the row's end while it differs from
/// `default` (none without one). Returns whether it changed, by a click or the reset.
pub fn default_switch(
    ui: &mut egui::Ui,
    on: &mut bool,
    (label, hint): (String, String),
    default: Option<&DefaultValue<bool>>,
) -> bool {
    let mut reset = false;
    // From the top: the switch, the label's first line centred on it, and the reset button.
    let changed = ui
        .horizontal_top(|ui| {
            let response = switch(ui, on).on_hover_text(hint);
            name(&response, &label);
            let spacing = ui.spacing().item_spacing.x;
            let side = ui.spacing().interact_size.y;
            let room = if default.is_some() {
                (ui.available_width() - side - spacing).max(20.0)
            } else {
                ui.available_width()
            };
            ui.allocate_ui_with_layout(
                egui::vec2(room, 0.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_width(room);
                    switch_label(ui, label);
                },
            );
            if let Some(default) = default {
                reset = reset_button(ui, *on != default.value, &default.shown);
            }
            response.changed()
        })
        .inner;
    if let (true, Some(default)) = (reset, default) {
        *on = default.value;
    }
    changed || reset
}

/// A [`choice`] among `labels` with a reset button at its end while it differs from `default`
/// (none without one). Returns the choice's response, marked changed by the reset too.
pub fn default_choice(
    ui: &mut egui::Ui,
    chosen: &mut usize,
    labels: &[String],
    default: Option<&DefaultValue<usize>>,
) -> egui::Response {
    let Some(default) = default else {
        return choice(ui, chosen, labels);
    };
    let (mut response, reset) = ui
        .horizontal(|ui| {
            let spacing = ui.spacing().item_spacing.x;
            let side = ui.spacing().interact_size.y;
            let width = (ui.available_width() - side - spacing).max(40.0);
            let response = ui
                .allocate_ui_with_layout(
                    egui::vec2(width, 0.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_width(width);
                        choice(ui, chosen, labels)
                    },
                )
                .inner;
            let reset = reset_button(ui, *chosen != default.value, &default.shown);
            (response, reset)
        })
        .inner;
    if reset {
        *chosen = default.value;
        response.mark_changed();
    }
    response
}

/// A labelled value that can be left automatic: the label, and under it in the same column the
/// `auto` switch with its text, wrap left of the fixed-width field, which is disabled while
/// automatic. Returns whether the switch or the field changed.
pub fn auto_value_row(
    ui: &mut egui::Ui,
    label: impl Into<egui::WidgetText>,
    auto: &mut bool,
    field: impl FnOnce(&mut egui::Ui) -> egui::Response,
) -> bool {
    auto_row(ui, label, auto, field, None).0
}

/// An [`auto_value_row`] for `value`, `None` while automatic (the field then shows
/// `automatic`), with a reset button before its field while it differs from `default` (none
/// without one). Returns whether the value changed, by the switch, the field or the reset.
pub fn default_auto_row(
    ui: &mut egui::Ui,
    label: impl Into<egui::WidgetText>,
    value: &mut Option<f64>,
    automatic: f64,
    default: Option<&DefaultValue<Option<f64>>>,
    field: impl FnOnce(&mut egui::Ui, &mut f64) -> egui::Response,
) -> bool {
    let mut auto = value.is_none();
    let mut number = value.unwrap_or(automatic);
    let reset = default.map(|d| (*value != d.value, d.shown.as_str()));
    let (changed, reset) = auto_row(ui, label, &mut auto, |ui| field(ui, &mut number), reset);
    if changed {
        *value = (!auto).then_some(number);
    }
    if let (true, Some(default)) = (reset, default) {
        *value = default.value;
    }
    changed || reset
}

/// An [`auto_value_row`], with the reset button of a value that differs from its default
/// before the field when `reset` is given. Returns whether the switch or the field changed, and
/// whether the reset was clicked.
fn auto_row(
    ui: &mut egui::Ui,
    label: impl Into<egui::WidgetText>,
    auto: &mut bool,
    field: impl FnOnce(&mut egui::Ui) -> egui::Response,
    reset: Option<(bool, &str)>,
) -> (bool, bool) {
    let label = label.into();
    let text = label.text().to_owned();
    let mut changed = false;
    let mut clicked = false;
    let enabled = !*auto;
    ui.horizontal(|ui| {
        let spacing = ui.spacing().item_spacing.x;
        let side = ui.spacing().interact_size.y;
        let button = if reset.is_some() { side + spacing } else { 0.0 };
        let left = (ui.available_width() - FIELD_WIDTH - spacing - button).max(40.0);
        ui.allocate_ui_with_layout(
            egui::vec2(left, 0.0),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.set_width(left);
                ui.add(egui::Label::new(label).wrap());
                ui.horizontal_wrapped(|ui| {
                    let response = switch(ui, auto);
                    name(&response, &tr!("a11y-automatic", setting = text.as_str()));
                    changed |= response.changed();
                    ui.add(
                        egui::Label::new(egui::RichText::new(tr!("prepare-auto")).small()).wrap(),
                    );
                });
            },
        );
        if let Some((differs, shown)) = reset {
            clicked = reset_button(ui, differs, shown);
        }
        let response = ui.add_enabled_ui(enabled, field).inner;
        name(&response, &text);
        changed |= response.changed();
    });
    (changed, clicked)
}

/// A fact: its name, weak and wrapping, and its value at the right, both from the row's top so
/// their first lines align whichever wraps. Returns the name's response, for a hint.
pub fn fact_row(ui: &mut egui::Ui, name: &str, value: &str) -> egui::Response {
    ui.horizontal_top(|ui| {
        let spacing = ui.spacing().item_spacing.x;
        let left = (ui.available_width() * 0.55 - spacing).max(40.0);
        let name = ui
            .allocate_ui_with_layout(
                egui::vec2(left, 0.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_width(left);
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(name).color(ui.visuals().weak_text_color()),
                        )
                        .wrap(),
                    )
                },
            )
            .inner;
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
            ui.add(egui::Label::new(value).wrap());
        });
        name
    })
    .inner
}

/// The state a panel's footer says (design rule, TD-098): `icon` in `colour` if any, and the
/// text, strong and wrapping, in the same colour. Returns the text's response, for a detail.
pub fn state_line(
    ui: &mut egui::Ui,
    icons: Option<&Icons>,
    icon: Option<Icon>,
    text: &str,
    colour: egui::Color32,
) -> egui::Response {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::Vec2::splat(20.0), egui::Sense::hover());
        if let (Some(icons), Some(icon)) = (icons, icon) {
            icons.paint(ui.painter(), icon, rect, colour);
        }
        ui.add(egui::Label::new(egui::RichText::new(text).color(colour).strong()).wrap())
    })
    .inner
}

/// Two buttons sharing a footer's width (design rule, TD-098): the first is the panel's main
/// action, `enabled.0`, the second its undoing, `enabled.1`. Returns their responses.
pub fn action_pair(
    ui: &mut egui::Ui,
    labels: [String; 2],
    enabled: (bool, bool),
) -> (egui::Response, egui::Response) {
    ui.horizontal(|ui| {
        let half = (ui.available_width() - ui.spacing().item_spacing.x) / 2.0;
        let size = egui::vec2(half, ui.spacing().interact_size.y);
        let [first, second] = labels;
        // Sized: a long translation is cut, never widening the footer.
        let button = |ui: &mut egui::Ui, label: String, on: bool| {
            ui.add_enabled_ui(on, |ui| {
                ui.add_sized(size, egui::Button::new(label).truncate())
            })
            .inner
        };
        let first = button(ui, first, enabled.0);
        let second = button(ui, second, enabled.1);
        (first, second)
    })
    .inner
}

/// A horizontal slider for `value` among `1..=count` (a layer, M6.5), as wide as the space
/// available: see [`range_slider`].
pub fn track_slider(ui: &mut egui::Ui, value: &mut usize, count: usize) -> egui::Response {
    let width = ui.available_width();
    range_slider(ui, value, 1..=count.max(1), width)
}

/// A horizontal slider `width` points wide for `value` among `range`: a visible track, the part
/// up to the value in the accent colour, and a handle like the section bar's. A click or a drag
/// sets the value; while it has the keyboard's focus, the arrows step it by one, Page Up and
/// Page Down by a tenth of the range, Home and End take it to the ends (TD-142). It reports
/// itself to screen readers as a slider with its value; its name is given with [`name`].
/// Returns the response, changed when the value changed.
pub fn range_slider(
    ui: &mut egui::Ui,
    value: &mut usize,
    range: RangeInclusive<usize>,
    width: f32,
) -> egui::Response {
    let size = egui::vec2(width, ui.spacing().interact_size.y);
    let (rect, mut response) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
    let (low, high) = (*range.start(), (*range.end()).max(*range.start()));
    let steps = high - low;
    let (left, right) = (rect.left() + 8.0, rect.right() - 8.0);
    #[allow(clippy::cast_precision_loss)] // Layer counts and percentages.
    let fraction = |v: usize| (v.saturating_sub(low)) as f32 / steps.max(1) as f32;
    if let Some(pointer) = response.interact_pointer_pos() {
        if response.clicked() || response.dragged() {
            let t = ((pointer.x - left) / (right - left).max(1.0)).clamp(0.0, 1.0);
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                clippy::cast_precision_loss
            )] // Within the range.
            let chosen = low + (t * steps as f32).round() as usize;
            if chosen != *value {
                *value = chosen;
                response.mark_changed();
            }
        }
    }
    if response.has_focus() {
        let stepped = slider_keys(ui, response.id, *value, low, high);
        if stepped != *value {
            *value = stepped;
            response.mark_changed();
        }
    }
    *value = (*value).clamp(low, high);
    #[allow(clippy::cast_precision_loss)] // Layer counts and percentages.
    let shown = *value as f64;
    response.widget_info(|| egui::WidgetInfo::slider(ui.is_enabled(), shown, ""));
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let y = rect.center().y;
        let at = egui::pos2(egui::lerp(left..=right, fraction(*value)), y);
        let enabled = ui.is_enabled();
        painter.line_segment(
            [egui::pos2(left, y), egui::pos2(right, y)],
            egui::Stroke::new(4.0, theme::BORDER.color()),
        );
        if enabled {
            painter.line_segment(
                [egui::pos2(left, y), at],
                egui::Stroke::new(4.0, theme::ACCENT.color()),
            );
        }
        let hovered = response.hovered() || response.dragged();
        let colour = if !enabled {
            theme::BORDER.color()
        } else if hovered {
            theme::INTERACTION.color()
        } else {
            theme::PRIMARY.color()
        };
        painter.rect_filled(
            egui::Rect::from_center_size(at, egui::vec2(10.0, 18.0)),
            3.0,
            colour,
        );
    }
    response
}

/// The value a focused slider's keys give `value` among `low..=high`: the arrows step it by
/// one, Page Up and Page Down by a tenth of the range (at least one), Home and End take it to
/// the ends. The arrows are kept from moving the focus while the slider has it.
fn slider_keys(ui: &egui::Ui, id: egui::Id, value: usize, low: usize, high: usize) -> usize {
    ui.memory_mut(|m| {
        m.set_focus_lock_filter(
            id,
            egui::EventFilter {
                horizontal_arrows: true,
                vertical_arrows: true,
                ..Default::default()
            },
        );
    });
    let page = ((high - low) / 10).max(1);
    ui.input_mut(|i| {
        let mut value = value;
        let none = egui::Modifiers::NONE;
        if i.consume_key(none, egui::Key::ArrowRight) || i.consume_key(none, egui::Key::ArrowUp) {
            value = value.saturating_add(1);
        }
        if i.consume_key(none, egui::Key::ArrowLeft) || i.consume_key(none, egui::Key::ArrowDown) {
            value = value.saturating_sub(1);
        }
        if i.consume_key(none, egui::Key::PageUp) {
            value = value.saturating_add(page);
        }
        if i.consume_key(none, egui::Key::PageDown) {
            value = value.saturating_sub(page);
        }
        if i.consume_key(none, egui::Key::Home) {
            value = low;
        }
        if i.consume_key(none, egui::Key::End) {
            value = high;
        }
        value.clamp(low, high)
    })
}

/// A group of a panel's or a window's settings (owner, 2026-11-10): a rounded frame in the border
/// colour, its title set into the top edge in the accent colour and capitals, and the group's
/// rows inside. Groups stand apart by their own margins. Returns what `add` returns.
pub fn group<R>(
    ui: &mut egui::Ui,
    title: impl Into<String>,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    group_with_hint(ui, title, "", add)
}

/// A [`group`] whose title shows `hint` under the pointer (none when empty).
pub fn group_with_hint<R>(
    ui: &mut egui::Ui,
    title: impl Into<String>,
    hint: impl AsRef<str>,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let title = title.into().to_uppercase();
    let font = egui::FontId::proportional(11.5);
    let colour = theme::ACCENT.color();
    let galley = ui.painter().layout_no_wrap(title.clone(), font, colour);
    let half = galley.size().y / 2.0;
    // Room above the frame for the title's upper half, and space from the group before.
    ui.add_space(GROUP_GAP + half);
    let inner = egui::Frame::new()
        .stroke(egui::Stroke::new(1.0, theme::BORDER.color()))
        .corner_radius(8.0)
        .inner_margin(egui::Margin {
            left: 10,
            right: 10,
            top: 14,
            bottom: 8,
        })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        });
    let frame = inner.response.rect;
    // The title over the frame's top edge, on the panel's own fill, so the edge breaks for it.
    let at = egui::pos2(frame.left() + 12.0, frame.top() - half);
    let behind = egui::Rect::from_min_size(at, galley.size()).expand2(egui::vec2(5.0, 0.0));
    let painter = ui.painter();
    painter.rect_filled(behind, 0.0, ui.visuals().window_fill);
    painter.galley(at, galley, colour);
    let hint = hint.as_ref();
    if !hint.is_empty() {
        let id = inner.response.id.with("title");
        ui.interact(behind, id, egui::Sense::hover())
            .on_hover_text(hint);
    }
    // To screen readers, the group is named by its title.
    inner
        .response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, &title));
    inner.inner
}

/// The space between two groups, in points.
const GROUP_GAP: f32 = 8.0;

/// The settings a switch turns on (owner, 2026-11-10): with `on`, drawn by `add` under the switch,
/// indented and joined to it by an accent rule on their left; with it off, not drawn at all.
/// Returns what `add` returns, or `None` when off.
pub fn depends<R>(ui: &mut egui::Ui, on: bool, add: impl FnOnce(&mut egui::Ui) -> R) -> Option<R> {
    if !on {
        return None;
    }
    let inner = egui::Frame::new()
        .outer_margin(egui::Margin {
            left: 8,
            ..egui::Margin::ZERO
        })
        .inner_margin(egui::Margin {
            left: 12,
            ..egui::Margin::ZERO
        })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        });
    let rect = inner.response.rect;
    let rule = theme::ACCENT.color().gamma_multiply(0.45);
    ui.painter().line_segment(
        [
            rect.left_top() + egui::vec2(1.0, 2.0),
            rect.left_bottom() + egui::vec2(1.0, -2.0),
        ],
        egui::Stroke::new(2.0, rule),
    );
    Some(inner.inner)
}

/// A label beside a [`switch`], wrapping: its first line is centred on the switch, as a single
/// line is in a row of controls.
pub fn switch_label(ui: &mut egui::Ui, text: impl Into<egui::WidgetText>) -> egui::Response {
    let line = ui.text_style_height(&egui::TextStyle::Body);
    ui.add_space(((ui.spacing().interact_size.y - line) / 2.0).max(0.0));
    ui.add(egui::Label::new(text).wrap())
}

/// A switch for `on`, like a checkbox: a pill whose knob slides to the right when on, in the
/// primary colour, and to the left when off. It animates with egui's own easing, takes the
/// keyboard's focus and reports itself to accessibility tools as a checkbox. It stands as tall
/// as the other controls, the pill centred in it, so a row's label lines up with it (owner,
/// 2026-11-10); a label that may wrap goes in [`switch_label`].
pub fn switch(ui: &mut egui::Ui, on: &mut bool) -> egui::Response {
    let row = ui.spacing().interact_size.y;
    let height = row * 0.7;
    let pill = egui::vec2(height * 1.9, height);
    let (cell, mut response) =
        ui.allocate_exact_size(egui::vec2(pill.x, row), egui::Sense::click());
    let rect = egui::Rect::from_center_size(cell.center(), pill);
    crate::focus::ring_around(ui.ctx(), response.id, rect);
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Checkbox, ui.is_enabled(), *on, "")
    });
    if ui.is_rect_visible(rect) {
        let how_on = ui.ctx().animate_bool_responsive(response.id, *on);
        let visuals = ui.style().interact_selectable(&response, *on);
        let fill = if *on {
            theme::PRIMARY.color()
        } else {
            theme::BORDER.color()
        };
        let radius = rect.height() / 2.0;
        ui.painter().rect(
            rect,
            radius,
            fill,
            visuals.bg_stroke,
            egui::StrokeKind::Inside,
        );
        let x = egui::lerp((rect.left() + radius)..=(rect.right() - radius), how_on);
        let knob = egui::pos2(x, rect.center().y);
        ui.painter()
            .circle_filled(knob, radius * 0.75, theme::TEXT_PRIMARY.color());
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every Rust source of the application, with its path.
    fn sources() -> Vec<(std::path::PathBuf, String)> {
        let mut folders = vec![std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src")];
        let mut files = Vec::new();
        while let Some(folder) = folders.pop() {
            for entry in std::fs::read_dir(&folder).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    folders.push(path);
                } else if path.extension().is_some_and(|e| e == "rs") {
                    let text = std::fs::read_to_string(&path).unwrap();
                    files.push((path, text));
                }
            }
        }
        files
    }

    /// A button's label made of symbols only, such as "×" or "‹", depends on the font and
    /// says nothing to a screen reader: buttons show an icon with a hint, or words.
    #[test]
    fn no_button_is_labelled_with_symbols_alone() {
        const OPENINGS: [&str; 4] = [
            "button(\"",
            "Button::new(\"",
            "Button::new(egui::RichText::new(\"",
            "Button::selectable(false, \"",
        ];
        let mut found = Vec::new();
        for (path, text) in sources() {
            for opening in OPENINGS {
                for (at, _) in text.match_indices(opening) {
                    let label = &text[at + opening.len()..];
                    let label = &label[..label.find('"').unwrap_or(label.len())];
                    if !label.chars().any(char::is_alphanumeric) {
                        let line = text[..at].lines().count();
                        found.push(format!("{}:{line}: {label:?}", path.display()));
                    }
                }
            }
        }
        assert!(found.is_empty(), "symbol buttons:\n{}", found.join("\n"));
    }

    #[test]
    fn no_slider_is_eguis_own() {
        // Built, so that this test is not found.
        let opening = ["Slider", "::new("].concat();
        let found: Vec<String> = sources()
            .iter()
            .flat_map(|(path, text)| {
                text.match_indices(&opening).map(move |(at, _)| {
                    format!("{}:{}", path.display(), text[..at].lines().count())
                })
            })
            .collect();
        assert!(found.is_empty(), "egui sliders:\n{}", found.join("\n"));
    }

    #[test]
    fn a_facts_name_and_value_start_on_one_line() {
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(260.0, 400.0),
            )),
            ..egui::RawInput::default()
        };
        let facts = [
            ("Panel (pixels)", "13312 × 5120"),
            ("Build volume", "223.6 × 126 × 230 mm, long enough to wrap"),
        ];
        let mut output = ctx.run_ui(input, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                ui.set_width(240.0);
                for (name, value) in facts {
                    fact_row(ui, name, value);
                }
            });
        });
        output.textures_delta.clear();
        let top = |text: &str| {
            output
                .shapes
                .iter()
                .find_map(|clipped| match &clipped.shape {
                    egui::Shape::Text(shape) if shape.galley.text() == text => Some(shape.pos.y),
                    _ => None,
                })
                .unwrap_or_else(|| panic!("{text} is not drawn"))
        };
        for (name, value) in facts {
            assert!(
                (top(name) - top(value)).abs() < 0.5,
                "{name}: {} against {}",
                top(name),
                top(value)
            );
        }
    }

    /// A click at the switch's place over `frames` frames, the switch `enabled` or not.
    fn click(on: bool, enabled: bool) -> bool {
        let ctx = egui::Context::default();
        let mut on = on;
        let at = egui::pos2(12.0, 12.0);
        let frames = [
            vec![egui::Event::PointerMoved(at)],
            vec![egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            }],
            vec![egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        ];
        for events in frames {
            let input = egui::RawInput {
                events,
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ui| {
                ui.add_enabled_ui(enabled, |ui| switch(ui, &mut on));
            });
            output.textures_delta.clear();
        }
        on
    }

    #[test]
    fn typed_values_read_a_point_or_a_comma_and_nothing_else() {
        assert_eq!(parse(" 0.25 "), Some(0.25));
        assert_eq!(parse("0,25"), Some(0.25));
        assert_eq!(parse("1e-3"), Some(0.001));
        assert_eq!(parse("abc"), None);
        assert_eq!(parse("inf"), None);
    }

    #[test]
    fn a_value_field_has_its_fixed_width_and_shows_its_value_and_unit() {
        let ctx = egui::Context::default();
        let mut mm = 0.05;
        let mut rect = egui::Rect::NOTHING;
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            rect = ui
                .add(UnitField::new(&mut mm, Unit::Millimetre).decimals(3))
                .rect;
        });
        output.textures_delta.clear();
        assert!((rect.width() - FIELD_WIDTH).abs() < 0.5, "{rect:?}");
        // The value's text starts inside the frame, not on its border.
        let value_left = output
            .shapes
            .iter()
            .find_map(|s| match &s.shape {
                egui::epaint::Shape::Text(t) if t.galley.text() == decimal(0.05, 3) => {
                    Some(t.pos.x + t.galley.rect.left())
                }
                _ => None,
            })
            .unwrap();
        assert!(
            value_left >= rect.left() + FIELD_INSET - 0.5,
            "{value_left} in {rect:?}"
        );
        let texts: Vec<String> = output
            .shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::epaint::Shape::Text(t) => Some(t.galley.text().to_owned()),
                _ => None,
            })
            .collect();
        assert!(texts.contains(&decimal(0.05, 3)), "{texts:?}");
        assert!(texts.contains(&tr!("unit-mm")), "{texts:?}");
        // Drawing changes nothing.
        assert!((mm - 0.05).abs() < f64::EPSILON);
    }

    /// A click on the right half of a two-way choice whose second option is `unavailable` or
    /// not: the index chosen after it.
    fn choose_second(unavailable: &[usize]) -> usize {
        let ctx = egui::Context::default();
        let mut chosen = 0;
        let at = egui::pos2(150.0, 14.0);
        let labels = [tr!("prepare-start-outer"), tr!("prepare-start-cavity")];
        for events in [
            vec![egui::Event::PointerMoved(at)],
            vec![egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            }],
            vec![egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        ] {
            let input = egui::RawInput {
                events,
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ui| {
                ui.set_width(200.0);
                choice_among(ui, &mut chosen, &labels, unavailable);
            });
            output.textures_delta.clear();
        }
        chosen
    }

    /// A click at `x` on a 200-point track slider over `count` values: the value after it.
    fn click_track(x: f32, count: usize) -> usize {
        click_range(x, 1..=count)
    }

    /// Click a 200-point slider over `range` at `x` from a value at its start: the value taken.
    fn click_range(x: f32, range: RangeInclusive<usize>) -> usize {
        let ctx = egui::Context::default();
        let mut value = *range.start();
        let at = egui::pos2(x, 10.0);
        for events in [
            vec![egui::Event::PointerMoved(at)],
            vec![egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            }],
            vec![egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        ] {
            let input = egui::RawInput {
                events,
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ui| {
                range_slider(ui, &mut value, range.clone(), 200.0);
            });
            output.textures_delta.clear();
        }
        value
    }

    #[test]
    fn a_track_slider_takes_the_value_under_a_click() {
        // The track runs 8 points in from each end of the slider.
        assert_eq!(click_track(3.0, 101), 1);
        assert_eq!(click_track(197.0, 101), 101);
        let middle = click_track(100.0, 101);
        assert!((49..=53).contains(&middle), "{middle}");
    }

    #[test]
    fn a_range_slider_starts_at_its_range() {
        assert_eq!(click_range(3.0, 50..=150), 50);
        assert_eq!(click_range(197.0, 50..=150), 150);
        let middle = click_range(100.0, 50..=150);
        assert!((98..=102).contains(&middle), "{middle}");
    }

    #[test]
    fn an_unavailable_option_takes_no_click() {
        assert_eq!(choose_second(&[]), 1);
        assert_eq!(choose_second(&[1]), 0);
    }

    #[test]
    fn a_group_frames_its_rows_and_dependent_rows_show_only_when_on() {
        let ctx = egui::Context::default();
        crate::theme::apply(&ctx);
        let (mut texts, mut shown, mut hidden) = (Vec::new(), None, Some(()));
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            group(ui, "Trees and braces", |ui| {
                ui.label("Branches");
                shown = depends(ui, true, |ui| {
                    ui.label("Join tips within");
                });
                hidden = depends(ui, false, |ui| {
                    ui.label("Never drawn");
                });
            });
        });
        output.textures_delta.clear();
        for clipped in &output.shapes {
            if let egui::Shape::Text(text) = &clipped.shape {
                texts.push(text.galley.text().to_owned());
            }
        }
        assert_eq!((shown, hidden), (Some(()), None));
        assert!(texts.contains(&"TREES AND BRACES".to_owned()), "{texts:?}");
        assert!(texts.contains(&"Join tips within".to_owned()), "{texts:?}");
        assert!(!texts.contains(&"Never drawn".to_owned()), "{texts:?}");
    }

    #[test]
    fn a_switch_rows_label_is_centred_on_the_switch() {
        // The owner's screenshot, 2026-11-10: the label sat below the switch.
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        crate::theme::apply(&ctx);
        let mut nodes = Vec::new();
        for _ in 0..2 {
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                let mut on = false;
                default_switch(ui, &mut on, ("Fit contact".into(), "hint".into()), None);
                let mut on = true;
                ui.horizontal(|ui| {
                    switch(ui, &mut on);
                    ui.label("Plain row");
                });
            });
            output.textures_delta.clear();
            nodes = output.platform_output.accesskit_update.unwrap().nodes;
        }
        let centre = |pick: &dyn Fn(&egui::accesskit::Node) -> bool| -> Vec<f64> {
            nodes
                .iter()
                .filter(|(_, n)| pick(n))
                .filter_map(|(_, n)| n.bounds())
                .map(|r| f64::midpoint(r.y0, r.y1))
                .collect()
        };
        let mut switches = centre(&|n| n.role() == egui::accesskit::Role::CheckBox);
        let mut labels = centre(&|n| {
            n.role() == egui::accesskit::Role::Label
                && matches!(n.value(), Some("Fit contact" | "Plain row"))
        });
        switches.sort_by(f64::total_cmp);
        labels.sort_by(f64::total_cmp);
        assert_eq!((switches.len(), labels.len()), (2, 2));
        for (s, l) in switches.iter().zip(&labels) {
            assert!((s - l).abs() <= 1.5, "switch centre {s}, label centre {l}");
        }
    }

    #[test]
    fn a_click_turns_the_switch_unless_it_is_disabled() {
        assert!(click(false, true));
        assert!(!click(true, true));
        assert!(!click(false, false));
    }
}
