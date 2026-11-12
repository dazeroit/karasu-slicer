//! Dialogs (design rule, owner 2026-10-26; `docs/UI-DESIGN.md`, "Windows").
//!
//! A dialog carries a task to complete or cancel. It is modal (the rest of the interface is
//! dimmed and cannot be used), opens in the middle of the screen, has a title bar with its name
//! and a close button that cancels, a footer always in view with Cancel and the main action at
//! the right, and is never larger than the screen less a margin. Escape cancels; Enter performs
//! the main action when nothing has the keyboard. A resizable dialog fills the size the user
//! gives it, so its content never leaves empty space when it changes; egui remembers the size
//! for the session.
use crate::{i18n::tr, settings::WindowPlace, theme};
use eframe::egui;
use std::collections::BTreeMap;

/// The space a dialog keeps from the screen's edges.
const SCREEN_MARGIN: f32 = 48.0;
/// The height of a dialog's footer.
const FOOTER_HEIGHT: f32 = 40.0;

/// How a dialog is sized.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Size {
    /// A fixed width; the height of the content, scrolling past the screen.
    Fixed(f32),
    /// Resizable by the user from `default`, never under `min`.
    Resizable {
        /// The size it opens at the first time.
        default: egui::Vec2,
        /// The smallest size.
        min: egui::Vec2,
    },
}

/// Which part of a dialog its content is asked to draw.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    /// The body, which fills the dialog (resizable) or scrolls (fixed).
    Body,
    /// The footer, at the bottom, drawn right to left: the main action first.
    Footer {
        /// Enter was pressed in this, the topmost dialog, with nothing else taking the
        /// keyboard: the main action should run.
        enter: bool,
    },
}

/// What happened to a dialog this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Response {
    /// The close button or Escape: cancel.
    pub cancel: bool,
}

/// The sizes and places the user left the windows at (TD-097), kept in the context for the
/// dialogs and tool windows to open at and for the settings to save: resizable dialogs' sizes,
/// and tool windows' places, by their identities.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Remembered {
    /// Resizable dialogs' sizes.
    pub dialogs: BTreeMap<String, [f32; 2]>,
    /// Tool windows' places.
    pub windows: BTreeMap<String, WindowPlace>,
    /// The print setting editor shows speeds in mm/min.
    pub speeds_per_minute: bool,
}

impl Remembered {
    fn id() -> egui::Id {
        egui::Id::new("karasu-remembered")
    }

    /// What `ctx` remembers.
    #[must_use]
    pub fn of(ctx: &egui::Context) -> Self {
        ctx.data(|d| d.get_temp(Self::id())).unwrap_or_default()
    }

    /// Remember `self` in `ctx`.
    pub fn keep(self, ctx: &egui::Context) {
        ctx.data_mut(|d| d.insert_temp(Self::id(), self));
    }

    /// Change what `ctx` remembers, when `change` changes it.
    pub fn change(ctx: &egui::Context, change: impl FnOnce(&mut Self)) {
        let mut remembered = Self::of(ctx);
        let before = remembered.clone();
        change(&mut remembered);
        if remembered != before {
            remembered.keep(ctx);
        }
    }
}

/// Show the dialog `id` titled `title`, its body and footer drawn by `content`.
pub fn show(
    ctx: &egui::Context,
    id: &str,
    title: &str,
    size: Size,
    mut content: impl FnMut(&mut egui::Ui, Part),
) -> Response {
    // The topmost dialog, as egui's modal tracking says (the last frame's, for a new one).
    let layer = egui::LayerId::new(egui::Order::Foreground, egui::Id::new(id));
    let top = ctx.memory_mut(|m| {
        m.set_modal_layer(layer);
        m.top_modal_layer() == Some(layer)
    });
    let enter = top
        && ctx.input(|i| i.key_pressed(egui::Key::Enter))
        && ctx.memory(|m| m.focused().is_none());
    let screen = ctx.content_rect().size() - egui::Vec2::splat(SCREEN_MARGIN * 2.0);
    let mut closed = false;
    let modal = egui::Modal::new(egui::Id::new(id))
        .frame(egui::Frame::window(&ctx.global_style()))
        .show(ctx, |ui| {
            // The chrome above the body: the title bar and its rule.
            let chrome = 40.0;
            match size {
                Size::Resizable { default, min } => {
                    // The size the user left it at, once they resized it.
                    let default = Remembered::of(ctx)
                        .dialogs
                        .get(id)
                        .map_or(default, |&[w, h]| egui::vec2(w, h))
                        .max(min)
                        .min(screen);
                    // egui's resize corner takes the focus: named for screen readers (TD-142).
                    let corner = ui
                        .make_persistent_id(egui::IdSalt::new(id))
                        .with("__resize_corner");
                    egui::Resize::default()
                        .id_salt(id)
                        .default_size(default)
                        .min_size(min)
                        .max_size(screen)
                        .with_stroke(false)
                        .show(ui, |ui| {
                            // The dialog's whole area, laid out by hand and claimed exactly: what
                            // the content claims would otherwise feed back into `Resize`, which
                            // grows to fit it, frame after frame.
                            let area = ui.available_rect_before_wrap();
                            closed = title_bar(ui, title);
                            let spacing = ui.spacing().item_spacing.y;
                            let footer_top = area.bottom() - FOOTER_HEIGHT;
                            let body = egui::Rect::from_min_max(
                                egui::pos2(area.left(), ui.cursor().top()),
                                egui::pos2(area.right(), footer_top - spacing),
                            );
                            let mut body_ui = ui.new_child(
                                egui::UiBuilder::new()
                                    .max_rect(body)
                                    .layout(egui::Layout::top_down(egui::Align::Min)),
                            );
                            body_ui.set_clip_rect(body.intersect(ui.clip_rect()));
                            content(&mut body_ui, Part::Body);
                            let mut footer_ui = ui.new_child(
                                egui::UiBuilder::new()
                                    .max_rect(egui::Rect::from_min_max(
                                        egui::pos2(area.left(), footer_top),
                                        area.max,
                                    ))
                                    .layout(egui::Layout::top_down(egui::Align::Min)),
                            );
                            footer(&mut footer_ui, &mut content, enter);
                            ui.advance_cursor_after_rect(area);
                            let size = [area.width().round(), area.height().round()];
                            let resized = (size[0] - default.x).abs() >= 0.5
                                || (size[1] - default.y).abs() >= 0.5;
                            if resized {
                                Remembered::change(ui.ctx(), |r| {
                                    r.dialogs.insert(id.to_owned(), size);
                                });
                            }
                        });
                    crate::widgets::name_id(ctx, corner, &tr!("a11y-resize"));
                }
                Size::Fixed(width) => {
                    ui.set_width(width);
                    closed = title_bar(ui, title);
                    crate::focus::scrolled(
                        egui::ScrollArea::vertical()
                            .id_salt((id, "body"))
                            .max_height(screen.y - chrome - FOOTER_HEIGHT),
                        ui,
                        |ui| content(ui, Part::Body),
                    );
                    footer(ui, &mut content, enter);
                }
            }
        });
    let escape = modal.is_top_modal
        && !modal.any_popup_open
        && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
    Response {
        cancel: closed || escape,
    }
}

/// A tool window (design rule, `docs/UI-DESIGN.md`, "Windows"): not modal, movable and
/// resizable, opening `default` large in the middle of the screen the first time, then where
/// and as large as the user left it (also across restarts, with [`remember_tool_window`] in its
/// content); kept on the screen.
pub fn tool_window(
    ctx: &egui::Context,
    id: &str,
    title: impl Into<egui::WidgetText>,
    default: egui::Vec2,
) -> egui::Window<'static> {
    focus_on_open(ctx, id);
    let screen = ctx.content_rect();
    let largest = screen.size() - egui::Vec2::splat(SCREEN_MARGIN * 2.0);
    let window = egui::Window::new(title).id(egui::Id::new(id));
    let window = match Remembered::of(ctx).windows.get(id) {
        Some(place) => window
            .pivot(egui::Align2::LEFT_TOP)
            .default_pos(egui::pos2(place.position[0], place.position[1]))
            .default_size(egui::vec2(place.size[0], place.size[1]).min(largest)),
        None => window
            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(screen.center())
            .default_size(default.min(largest)),
    };
    window
        .max_size(screen.size() - egui::Vec2::splat(SCREEN_MARGIN))
        .resizable(true)
        .collapsible(false)
        .constrain(true)
}

/// Note whether tool window `id` opens on this frame (its builder was not called on the frame
/// before), for [`remember_tool_window`] to move the keyboard's focus into it (TD-142). Call
/// before the window is shown, every frame it is.
fn focus_on_open(ctx: &egui::Context, id: &str) {
    let window = egui::Id::new(id);
    let key = window.with("karasu-shown-on-pass");
    let pass = ctx.cumulative_pass_nr();
    let opening = ctx.data_mut(|d| {
        let before = d.get_temp::<u64>(key);
        d.insert_temp(key, pass);
        // Not shown on the pass before (nor on this one, drawn again): opening.
        before.is_none_or(|before| before + 1 < pass)
    });
    if opening {
        ctx.data_mut(|d| d.insert_temp(window.with("karasu-opening"), true));
    }
}

/// On the frame tool window `id` opens, from the start of its content: give the keyboard's focus
/// to its first control (TD-142). egui has no call for it, so a focus entry, a point with the
/// focus, passes it on to the next control drawn, as the Tab key does; it exists on that frame
/// only, and so is never a stop of the Tab key.
fn enter_on_open(ui: &egui::Ui, id: &str) {
    // A window's first frame only measures it, its controls invisible: wait for one shown.
    if ui.is_sizing_pass() || !ui.is_visible() {
        return;
    }
    let opening = egui::Id::new(id).with("karasu-opening");
    if !ui
        .ctx()
        .data_mut(|d| d.remove_temp::<bool>(opening).unwrap_or(false))
    {
        return;
    }
    let entry = egui::Id::new(id).with("karasu-focus-entry");
    ui.memory_mut(|m| {
        m.request_focus(entry);
        m.move_focus(egui::FocusDirection::Next);
    });
    let point = egui::Rect::from_min_size(ui.cursor().min, egui::Vec2::ZERO);
    let _ = ui.interact(point, entry, egui::Sense::FOCUSABLE);
}

/// A tool window that opens in the middle of the application every time (owner, 2026-11-05,
/// for the plates' overview) and as large as it was left: `recentre` on the frame it opens.
/// Moved while open, it stays where it is put until closed.
pub fn centred_tool_window(
    ctx: &egui::Context,
    id: &str,
    title: impl Into<egui::WidgetText>,
    default: egui::Vec2,
    recentre: bool,
) -> egui::Window<'static> {
    focus_on_open(ctx, id);
    let screen = ctx.content_rect();
    let largest = screen.size() - egui::Vec2::splat(SCREEN_MARGIN * 2.0);
    let size = Remembered::of(ctx)
        .windows
        .get(id)
        .map_or(default, |place| egui::vec2(place.size[0], place.size[1]));
    // The pivot stays the centre, so the window keeps its place from frame to frame.
    let mut window = egui::Window::new(title)
        .id(egui::Id::new(id))
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(screen.center())
        .default_size(size.min(largest));
    if recentre {
        window = window.current_pos(screen.center());
    }
    window
        .max_size(screen.size() - egui::Vec2::splat(SCREEN_MARGIN))
        .resizable(true)
        .collapsible(false)
        .constrain(true)
}

/// Remember where tool window `id` is and its content's size, from within its content `ui`, so
/// it opens there again.
pub fn remember_tool_window(ui: &egui::Ui, id: &str) {
    enter_on_open(ui, id);
    let ctx = ui.ctx();
    let Some(outer) = ctx.memory(|m| m.area_rect(egui::Id::new(id))) else {
        return;
    };
    let size = ui.max_rect().size();
    let place = WindowPlace {
        position: [outer.min.x.round(), outer.min.y.round()],
        size: [size.x.round(), size.y.round()],
    };
    Remembered::change(ctx, |r| {
        r.windows.insert(id.to_owned(), place);
    });
}

/// The dialog's name and its close button, then a rule. Returns whether it was closed.
fn title_bar(ui: &mut egui::Ui, title: &str) -> bool {
    let mut closed = false;
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(title).strong().size(15.0));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let icons = crate::icons::Icons::shared(ui.ctx());
            closed = crate::widgets::icon_button(
                ui,
                icons.as_deref(),
                crate::icons::Icon::Close,
                false,
                &tr!("action-cancel"),
            )
            .clicked();
        });
    });
    ui.separator();
    closed
}

fn footer(ui: &mut egui::Ui, content: &mut impl FnMut(&mut egui::Ui, Part), enter: bool) {
    ui.separator();
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        content(ui, Part::Footer { enter });
    });
}

/// The footer's main action, at the right: drawn first in the footer's right-to-left layout.
/// Returns whether it was clicked, or Enter chose it.
pub fn main_action(ui: &mut egui::Ui, label: &str, enabled: bool, enter: bool) -> bool {
    let clicked = ui
        .add_enabled(
            enabled,
            egui::Button::new(egui::RichText::new(label).strong()).fill(theme::PRIMARY.color()),
        )
        .clicked();
    clicked || (enabled && enter)
}

/// The footer's Cancel, left of the main action. Returns whether it was clicked.
pub fn cancel_action(ui: &mut egui::Ui) -> bool {
    ui.button(tr!("action-cancel")).clicked()
}

/// A small confirmation over the current dialog: `text`, Cancel and the `action` (drawn in the
/// error colour when it destroys something). Returns `Some(true)` to go ahead, `Some(false)` to
/// cancel, `None` while undecided.
pub fn confirm(
    ctx: &egui::Context,
    id: &str,
    title: &str,
    text: &str,
    action: &str,
    destructive: bool,
) -> Option<bool> {
    let mut decided = None;
    let response = show(ctx, id, title, Size::Fixed(380.0), |ui, part| match part {
        Part::Body => {
            ui.add(egui::Label::new(text).wrap());
        }
        Part::Footer { enter } => {
            let label = if destructive {
                egui::RichText::new(action)
                    .strong()
                    .color(theme::ERROR.color())
            } else {
                egui::RichText::new(action).strong()
            };
            if ui.button(label).clicked() || enter {
                decided = Some(true);
            }
            if cancel_action(ui) {
                decided = Some(false);
            }
        }
    });
    if response.cancel {
        decided = Some(false);
    }
    decided
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The dialog `id`'s area after a few frames of a resizable dialog.
    fn opened(ctx: &egui::Context, id: &str) -> egui::Rect {
        for _ in 0..3 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1600.0, 1000.0),
                )),
                ..egui::RawInput::default()
            };
            let mut output = ctx.run_ui(input, |ui| {
                show(
                    ui.ctx(),
                    id,
                    "Dialog",
                    Size::Resizable {
                        default: egui::vec2(700.0, 500.0),
                        min: egui::vec2(400.0, 300.0),
                    },
                    |_, _| {},
                );
            });
            output.textures_delta.clear();
        }
        ctx.memory(|m| m.area_rect(egui::Id::new(id))).unwrap()
    }

    #[test]
    fn a_resizable_dialog_opens_at_the_size_it_was_left_at() {
        // The first time, its default; nothing to remember while it keeps it.
        let ctx = egui::Context::default();
        let first = opened(&ctx, "probe");
        assert!(Remembered::of(&ctx).dialogs.is_empty());
        // Remembered (as the settings restore it), it opens at that size, never under its
        // minimum.
        let ctx = egui::Context::default();
        Remembered::change(&ctx, |r| {
            r.dialogs.insert("probe".into(), [900.0, 640.0]);
        });
        let kept = opened(&ctx, "probe");
        assert!(
            (kept.width() - first.width() - 200.0).abs() < 1.0,
            "{kept:?} {first:?}"
        );
        assert!((kept.height() - first.height() - 140.0).abs() < 1.0);
        let ctx = egui::Context::default();
        Remembered::change(&ctx, |r| {
            r.dialogs.insert("probe".into(), [100.0, 100.0]);
        });
        let small = opened(&ctx, "probe");
        assert!(
            (small.width() - first.width() + 300.0).abs() < 1.0,
            "{small:?}"
        );
    }
}
