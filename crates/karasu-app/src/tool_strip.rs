//! The tool strip (owner, 2026-10-29, from the canvas mockup "Tool strip"): a narrow column of
//! icon buttons at the viewport's left for the tool in use's modes and switches, one click
//! away; its values stay in the tool's panel.
//!
//! A button performs its action ([`Action`]), lit while on, its name and key in its tooltip;
//! a button with options shows a small triangle in its corner, which opens them (so does a
//! right click). The strip is opaque but light: the panels' colour, a little translucent, with no
//! blur (owner). Each tool says whether it shows one and what is in it.
use crate::{
    actions::{Action, Keymap},
    icons::{Icon, Icons},
    theme,
};
use eframe::egui;

/// A button's side, in points.
const SIDE: f32 = 36.0;
/// How far the strip stands from the viewport's left edge, in points.
const MARGIN: f32 = 12.0;
/// The strip's background: the panels' colour, a little translucent (owner, 2026-10-29).
const OPACITY: f32 = 0.86;

/// One item of the strip.
pub enum Item {
    /// A button that performs `action`, lit when `on`; with `options` (their labels and which
    /// is chosen), a triangle opens them.
    Button {
        action: Action,
        icon: Icon,
        on: bool,
        enabled: bool,
        options: Vec<(String, bool)>,
    },
    /// A line between groups.
    Separator,
}

impl Item {
    /// A button for `action` with `icon`, lit when `on`.
    #[must_use]
    pub fn button(action: Action, icon: Icon, on: bool, enabled: bool) -> Self {
        Self::Button {
            action,
            icon,
            on,
            enabled,
            options: Vec::new(),
        }
    }

    /// The button with `options` to choose among: their labels and which is chosen.
    #[must_use]
    pub fn with_options(self, options: Vec<(String, bool)>) -> Self {
        match self {
            Self::Button {
                action,
                icon,
                on,
                enabled,
                ..
            } => Self::Button {
                action,
                icon,
                on,
                enabled,
                options,
            },
            Self::Separator => Self::Separator,
        }
    }
}

/// What the strip was asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Picked {
    /// A button's action.
    Action(Action),
    /// Option `index` of `action`'s button.
    Option(Action, usize),
}

/// Show the strip of `items` at the left of `viewport`, a quarter of the way down. Returns what
/// was clicked, if anything.
pub fn show(
    ctx: &egui::Context,
    viewport: egui::Rect,
    items: &[Item],
    keymap: &Keymap,
    icons: Option<&Icons>,
) -> Option<Picked> {
    if items.is_empty() {
        return None;
    }
    let mut picked = None;
    let top = viewport.top() + viewport.height() * 0.25;
    egui::Area::new(egui::Id::new("tool-strip"))
        .fixed_pos(egui::pos2(viewport.left() + MARGIN, top))
        .show(ctx, |ui| {
            let fill = theme::SURFACE.color().gamma_multiply(OPACITY);
            egui::Frame::new()
                .fill(fill)
                .stroke(egui::Stroke::new(1.0, theme::BORDER.color()))
                .corner_radius(10.0)
                .inner_margin(4.0)
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(0.0, 4.0);
                    ui.vertical(|ui| {
                        for (k, item) in items.iter().enumerate() {
                            match item {
                                Item::Separator => {
                                    let (rect, _) = ui.allocate_exact_size(
                                        egui::vec2(SIDE, 5.0),
                                        egui::Sense::hover(),
                                    );
                                    ui.painter().hline(
                                        rect.x_range().shrink(6.0),
                                        rect.center().y,
                                        egui::Stroke::new(1.0, theme::BORDER.color()),
                                    );
                                }
                                Item::Button {
                                    action,
                                    icon,
                                    on,
                                    enabled,
                                    options,
                                } => {
                                    let clicked = ui.add_enabled_ui(*enabled, |ui| {
                                        button(ui, k, *action, *icon, *on, options, keymap, icons)
                                    });
                                    if let Some(p) = clicked.inner {
                                        picked = Some(p);
                                    }
                                }
                            }
                        }
                    });
                });
        });
    picked
}

/// One button: its icon lit when `on`, its tooltip, and its options' triangle and menu.
#[allow(clippy::too_many_arguments)] // The button's whole look.
fn button(
    ui: &mut egui::Ui,
    k: usize,
    action: Action,
    icon: Icon,
    on: bool,
    options: &[(String, bool)],
    keymap: &Keymap,
    icons: Option<&Icons>,
) -> Option<Picked> {
    let (rect, response) = ui.allocate_exact_size(egui::Vec2::splat(SIDE), egui::Sense::click());
    let tip = keymap.tooltip(ui.ctx(), action);
    let response = response.on_hover_text(&tip);
    let enabled = ui.is_enabled();
    let (fill, stroke) = if on {
        (
            theme::PRIMARY.color(),
            egui::Stroke::new(1.0, theme::INTERACTION.color()),
        )
    } else if response.hovered() && enabled {
        (
            ui.visuals().widgets.hovered.weak_bg_fill,
            egui::Stroke::NONE,
        )
    } else {
        (egui::Color32::TRANSPARENT, egui::Stroke::NONE)
    };
    ui.painter()
        .rect(rect, 8.0, fill, stroke, egui::StrokeKind::Inside);
    let colour = if enabled {
        theme::TEXT_PRIMARY.color()
    } else {
        ui.visuals().weak_text_color()
    };
    if let Some(icons) = icons {
        icons.paint(ui.painter(), icon, rect.shrink(8.0), colour);
    }
    response
        .widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, enabled, on, &tip));
    let mut picked = response.clicked().then_some(Picked::Action(action));
    if options.is_empty() {
        return picked;
    }
    // The options' triangle in the bottom right corner; it and a right click open them.
    let corner = egui::Rect::from_min_max(rect.max - egui::vec2(12.0, 12.0), rect.max);
    let triangle = ui
        .interact(
            corner,
            ui.id().with(("tool-strip-options", k)),
            egui::Sense::click(),
        )
        .on_hover_text(tr_options(&tip));
    crate::widgets::name(&triangle, &tr_options(&tip));
    let tip_colour = if enabled {
        theme::ACCENT.color()
    } else {
        ui.visuals().weak_text_color()
    };
    let inset = corner.shrink(2.0);
    ui.painter().add(egui::Shape::convex_polygon(
        vec![inset.right_top(), inset.right_bottom(), inset.left_bottom()],
        tip_colour,
        egui::Stroke::NONE,
    ));
    // The triangle, drawn over the button, takes its own clicks.
    egui::Popup::menu(&triangle).show(|ui| options_menu(ui, action, options, &mut picked));
    egui::Popup::context_menu(&response).show(|ui| options_menu(ui, action, options, &mut picked));
    picked
}

/// The options of `action`'s button, the chosen one marked; a click picks one.
fn options_menu(
    ui: &mut egui::Ui,
    action: Action,
    options: &[(String, bool)],
    picked: &mut Option<Picked>,
) {
    for (index, (label, chosen)) in options.iter().enumerate() {
        if ui.selectable_label(*chosen, label).clicked() {
            *picked = Some(Picked::Option(action, index));
        }
    }
}

/// The triangle's tooltip: the button's, with its options.
fn tr_options(tip: &str) -> String {
    crate::i18n::tr!("strip-options", button = tip)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Click the strip of `items` at `at`, over two frames (the area settles on the first):
    /// what was picked.
    fn click(items: &[Item], at: egui::Pos2) -> Option<Picked> {
        let ctx = egui::Context::default();
        let viewport = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        let keymap = Keymap::default();
        let mut picked = None;
        for events in [
            Vec::new(),
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
                screen_rect: Some(viewport),
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ui| {
                if let Some(p) = show(ui.ctx(), viewport, items, &keymap, None) {
                    picked = Some(p);
                }
            });
            output.textures_delta.clear();
        }
        picked
    }

    #[test]
    fn a_button_performs_its_action_and_a_disabled_one_does_not() {
        // The first button: a quarter of the way down, inside the margin and the frame.
        let first = egui::pos2(MARGIN + 4.0 + SIDE / 2.0 - 4.0, 150.0 + 4.0 + SIDE / 2.0);
        let items = [
            Item::button(Action::Place, Icon::Supports, false, true),
            Item::Separator,
        ];
        assert_eq!(click(&items, first), Some(Picked::Action(Action::Place)));
        let disabled = [Item::button(Action::Place, Icon::Supports, false, false)];
        assert_eq!(click(&disabled, first), None);
        assert_eq!(click(&[], first), None);
    }
}
