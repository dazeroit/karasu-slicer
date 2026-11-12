//! The accessibility check of the interface (TD-142, ROADMAP M6 "accessibility checks"), for the
//! tests: what a draw shows is drawn headless with AccessKit on, and
//! - every control a user can reach has a name for screen readers;
//! - every stop of the Tab key's cycle is a visible control with a name;
//! - a modal dialog keeps the Tab cycle inside it.
//!
//! **Left out, as egui's own:** a window's title bar and resize corner (drag targets egui makes
//! focusable) and scroll bars. Labels carry their text as their value, as AccessKit asks.
use eframe::egui::{self, accesskit};

/// The window's size in the checks.
const SCREEN: egui::Vec2 = egui::vec2(1600.0, 1000.0);

/// One frame's nodes.
pub type Nodes = Vec<(accesskit::NodeId, accesskit::Node)>;

/// The control with the focus as a frame ends: its id, its layer and its visible rectangle.
/// Read inside the frame: once it ends, egui answers for the frame before.
pub type Focused = Option<(egui::Id, egui::LayerId, egui::Rect)>;

fn input(events: Vec<egui::Event>) -> egui::RawInput {
    egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, SCREEN)),
        events,
        ..egui::RawInput::default()
    }
}

/// One frame of `draw` with `events`; its nodes and the control with the focus.
fn frame(
    draw: &mut impl FnMut(&mut egui::Ui),
    ctx: &egui::Context,
    events: Vec<egui::Event>,
) -> (Nodes, Focused) {
    let mut focused = None;
    let mut output = ctx.run_ui(input(events), |ui| {
        draw(ui);
        let ctx = ui.ctx();
        focused = ctx.memory(egui::Memory::focused).and_then(|id| {
            ctx.read_response(id)
                .map(|r| (id, r.layer_id, r.interact_rect))
        });
    });
    output.textures_delta.clear();
    let nodes = output
        .platform_output
        .accesskit_update
        .map(|update| update.nodes)
        .unwrap_or_default();
    (nodes, focused)
}

fn tab() -> egui::Event {
    egui::Event::Key {
        key: egui::Key::Tab,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }
}

/// The ids of egui's own drag targets: every window's title bar and resize corner.
fn egui_own(ctx: &egui::Context) -> std::collections::HashSet<u64> {
    ctx.memory(|m| {
        m.layer_ids()
            .flat_map(|layer| {
                [
                    layer.id.with("__title_click"),
                    layer.id.with("resize").with("__resize_corner"),
                ]
            })
            .map(|id| id.accesskit_id().0)
            .collect()
    })
}

/// A node as a failure names it: its role, text and place.
fn describe(node: &accesskit::Node) -> String {
    let place = node
        .bounds()
        .map(|r| {
            format!(
                " at ({:.0}, {:.0}) {:.0}×{:.0}",
                r.x0,
                r.y0,
                r.x1 - r.x0,
                r.y1 - r.y0
            )
        })
        .unwrap_or_default();
    format!(
        "{:?} value {:?}{place}",
        node.role(),
        node.value().unwrap_or("")
    )
}

/// Whether `node` is a control a user can reach.
fn reachable(node: &accesskit::Node) -> bool {
    node.supports_action(accesskit::Action::Click) || node.supports_action(accesskit::Action::Focus)
}

/// Whether `node` needs a name of its own.
fn needs_name(node: &accesskit::Node) -> bool {
    !matches!(
        node.role(),
        accesskit::Role::Label | accesskit::Role::ScrollBar
    )
}

fn named(node: &accesskit::Node) -> bool {
    node.label().is_some_and(|l| !l.trim().is_empty())
}

/// What `draw` shows after a few frames: its AccessKit nodes and the control with the focus, as
/// the context `ctx` keeps them, so a caller can draw on.
pub fn settled(ctx: &egui::Context, mut draw: impl FnMut(&mut egui::Ui)) -> (Nodes, Focused) {
    ctx.enable_accesskit();
    let mut last = (Vec::new(), None);
    for _ in 0..4 {
        last = frame(&mut draw, ctx, Vec::new());
    }
    last
}

/// The failures of what `draw` shows: controls without a name, then Tab stops that are hidden,
/// nameless or (with a modal open) outside the modal. `draw` is called once per frame and
/// should paint the focus ring as the window does (`crate::focus::paint`).
pub fn failures(mut draw: impl FnMut(&mut egui::Ui)) -> Vec<String> {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    crate::theme::apply(&ctx);
    let mut nodes = Vec::new();
    // Windows settle their size over a few frames.
    for _ in 0..4 {
        nodes = frame(&mut draw, &ctx, Vec::new()).0;
    }
    let own = egui_own(&ctx);
    let mut failures: Vec<String> = nodes
        .iter()
        .filter(|(id, node)| {
            reachable(node) && needs_name(node) && !named(node) && !own.contains(&id.0)
        })
        .map(|(_, node)| format!("no name: {}", describe(node)))
        .collect();
    let modal = ctx.memory(egui::Memory::top_modal_layer);
    let mut first = None;
    let mut released = false;
    for _ in 0..400 {
        frame(&mut draw, &ctx, vec![tab()]);
        // egui moves the focus as the frame ends; the next frame scrolls to the control, and
        // the one after draws it there.
        frame(&mut draw, &ctx, Vec::new());
        frame(&mut draw, &ctx, Vec::new());
        let (nodes, focused) = frame(&mut draw, &ctx, Vec::new());
        let Some(id) = ctx.memory(egui::Memory::focused) else {
            // Past the last stop the focus is let go; the next Tab starts again.
            if first.is_some() || released {
                break;
            }
            released = true;
            continue;
        };
        if first == Some(id) {
            break;
        }
        first.get_or_insert(id);
        if own.contains(&id.accesskit_id().0) {
            continue;
        }
        let node = nodes
            .iter()
            .find(|(n, _)| n.0 == id.accesskit_id().0)
            .map(|(_, node)| node);
        let shown = node.map_or_else(|| format!("{id:?}"), describe);
        let Some((_, layer, visible)) = focused.filter(|(f, _, _)| *f == id) else {
            failures.push(format!("Tab stop drawn nowhere: {shown}"));
            continue;
        };
        if !visible.is_positive() {
            failures.push(format!("hidden Tab stop: {shown}"));
        }
        if node.is_some_and(|n| needs_name(n) && !named(n)) {
            failures.push(format!("Tab stop without a name: {shown}"));
        }
        if modal.is_some_and(|modal| modal != layer) {
            failures.push(format!("Tab stop outside the modal: {shown}"));
        }
    }
    failures.sort();
    failures.dedup();
    failures
}
