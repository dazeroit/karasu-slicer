//! The keyboard focus ring (TD-142): a 2-point outline in the accent colour around the control
//! that has the keyboard's focus, drawn while the keyboard is in use.
//!
//! **Why its own ring:** egui shows a focused control as a pressed one, in a fill hardly apart
//! from the hover fill (1.37:1), and the application's own controls (icon buttons, the tool
//! strip, choices, sliders) paint no focus at all. The ring is one look for every control.
//!
//! **When:** a Tab or an arrow key turns the ring on, a pointer press turns it off, so a click
//! leaves no ring behind while keyboard navigation always shows where it is ("focus visible").
//!
//! **Where:** around the control's visible part, on the control's own layer, so a window or
//! menu above it covers the ring too. A control drawn in parts (a value field: its text and its
//! unit) names the rectangle the ring goes around with [`ring_around`].
//!
//! **In view:** egui does not scroll to a control the keyboard moves the focus to, so a Tab could
//! land on a control hidden in a panel's scrolled part. The areas that scroll are shown with
//! [`scrolled`], which brings a control the keyboard just focused into view.
use crate::theme;
use eframe::egui;

/// The ring's width, in points (owner, 2026-11-10).
pub const WIDTH: f32 = 2.0;
/// The ring's gap from the control, in points.
const GAP: f32 = 1.0;

/// Where the ring's state is kept in egui's memory.
fn keyboard_id() -> egui::Id {
    egui::Id::new("karasu-focus-keyboard")
}

/// Where the control the keyboard focused last frame, still to bring into view, is kept.
fn pending_id() -> egui::Id {
    egui::Id::new("karasu-focus-pending")
}

/// Where the control focused at the end of the last frame is kept.
fn last_id() -> egui::Id {
    egui::Id::new("karasu-focus-last")
}

/// Where the rectangles named this frame are kept.
fn around_id() -> egui::Id {
    egui::Id::new("karasu-focus-around")
}

/// The ring of the control `id` goes around `rect` this frame rather than around the control's
/// own rectangle.
pub fn ring_around(ctx: &egui::Context, id: egui::Id, rect: egui::Rect) {
    if ctx.memory(|m| m.has_focus(id)) {
        ctx.data_mut(|d| d.insert_temp(around_id(), (id, rect)));
    }
}

/// Whether the keyboard is in use: the last of this frame's Tab or arrow presses and pointer
/// presses decides, and the state lasts until the other kind comes.
fn follow_input(ctx: &egui::Context) -> bool {
    let mut keyboard = ctx.data(|d| d.get_temp::<bool>(keyboard_id()).unwrap_or(false));
    ctx.input(|i| {
        for event in &i.events {
            match event {
                egui::Event::Key {
                    key:
                        egui::Key::Tab
                        | egui::Key::ArrowUp
                        | egui::Key::ArrowDown
                        | egui::Key::ArrowLeft
                        | egui::Key::ArrowRight,
                    pressed: true,
                    ..
                } => keyboard = true,
                egui::Event::PointerButton { pressed: true, .. } => keyboard = false,
                _ => {}
            }
        }
    });
    ctx.data_mut(|d| d.insert_temp(keyboard_id(), keyboard));
    keyboard
}

/// The ring for this frame, if any: the focused control's layer and the rectangle the ring goes
/// around. Call once per frame, after every control is drawn.
pub fn ring(ctx: &egui::Context) -> Option<(egui::LayerId, egui::Rect)> {
    let keyboard = follow_input(ctx);
    let around = ctx.data_mut(|d| {
        let around = d.get_temp::<(egui::Id, egui::Rect)>(around_id());
        d.remove::<(egui::Id, egui::Rect)>(around_id());
        around
    });
    if !keyboard {
        return None;
    }
    let id = ctx.memory(egui::Memory::focused)?;
    let response = ctx.read_response(id)?;
    // The visible part only: a control scrolled partly out of its area is clipped there (its
    // interaction rectangle is then smaller than its rectangle).
    let clipped = response.interact_rect != response.rect;
    let rect = match around {
        Some((named, rect)) if named == id && clipped => rect.intersect(response.interact_rect),
        Some((named, rect)) if named == id => rect,
        _ => response.interact_rect,
    };
    (rect.is_positive()).then_some((response.layer_id, rect))
}

/// Show `area` with `add` inside, bringing into view a control the keyboard focused, if `add`
/// draws it.
pub fn scrolled<R>(
    area: egui::ScrollArea,
    ui: &mut egui::Ui,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::scroll_area::ScrollAreaOutput<R> {
    area.show(ui, |ui| {
        let inner = add(ui);
        keep_in_view(ui);
        inner
    })
}

/// Inside a scrolled area, after its content: scroll the control the keyboard just focused into
/// view if this content holds it.
fn keep_in_view(ui: &egui::Ui) {
    let ctx = ui.ctx();
    let Some(id) = ctx.data(|d| d.get_temp::<egui::Id>(pending_id())) else {
        return;
    };
    let Some(response) = ctx.read_response(id) else {
        return;
    };
    if response.layer_id == ui.layer_id() && ui.min_rect().contains_rect(response.rect) {
        // Kept for the areas around this one, which scroll too.
        ui.scroll_to_rect_animation(response.rect, None, egui::style::ScrollAnimation::none());
        // The area takes its new offset on the next frame: draw it at once.
        ctx.request_repaint();
    }
}

/// Draw the ring of this frame, if any, and note a control the keyboard focused anew, to bring
/// into view on the next frame. Call once per frame, after every control is drawn.
pub fn paint(ctx: &egui::Context) {
    let focused = ctx.memory(egui::Memory::focused);
    let last = ctx.data(|d| d.get_temp::<Option<egui::Id>>(last_id()).flatten());
    ctx.data_mut(|d| {
        d.insert_temp(last_id(), focused);
        d.remove::<egui::Id>(pending_id());
    });
    let ring = ring(ctx);
    if let (Some(id), true) = (focused, focused != last) {
        if ring.is_some()
            || ctx
                .read_response(id)
                .is_some_and(|r| r.interact_rect != r.rect)
        {
            ctx.data_mut(|d| d.insert_temp(pending_id(), id));
            // egui moves the focus as a frame ends: the next frame scrolls to it.
            ctx.request_repaint();
        }
    }
    if let Some((layer, rect)) = ring {
        let outer = rect.expand(GAP + WIDTH);
        ctx.layer_painter(layer).with_clip_rect(outer).rect_stroke(
            rect.expand(GAP),
            4.0,
            egui::Stroke::new(WIDTH, theme::ACCENT.color()),
            egui::StrokeKind::Outside,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(key: egui::Key) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }
    }

    /// Two buttons; the ring found after `events`.
    fn frame(
        ctx: &egui::Context,
        events: Vec<egui::Event>,
    ) -> (Option<(egui::LayerId, egui::Rect)>, [egui::Rect; 2]) {
        let mut rects = [egui::Rect::NOTHING; 2];
        let mut found = None;
        let input = egui::RawInput {
            events,
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| {
            rects[0] = ui.button("First").rect;
            let second = ui.button("Second");
            rects[1] = second.rect;
            ring_around(ui.ctx(), second.id, second.rect.expand(3.0));
            found = ring(ui.ctx());
        });
        output.textures_delta.clear();
        (found, rects)
    }

    #[test]
    fn the_ring_follows_the_keyboard_and_leaves_with_a_click() {
        let ctx = egui::Context::default();
        let (none, _) = frame(&ctx, Vec::new());
        assert!(none.is_none());
        let (first, rects) = frame(&ctx, vec![key(egui::Key::Tab)]);
        assert_eq!(first.map(|(_, r)| r), Some(rects[0]));
        // A rectangle named for the focused control is the one ringed.
        let (second, rects) = frame(&ctx, vec![key(egui::Key::Tab)]);
        assert_eq!(second.map(|(_, r)| r), Some(rects[1].expand(3.0)));
        // A pointer press anywhere ends keyboard navigation's ring, the focus aside.
        let press = egui::Event::PointerButton {
            pos: egui::pos2(500.0, 500.0),
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        };
        let (pressed, _) = frame(&ctx, vec![press]);
        assert!(pressed.is_none());
    }
}
