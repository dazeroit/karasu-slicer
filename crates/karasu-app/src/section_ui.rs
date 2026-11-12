//! The section panel (M6.4b3, TD-086; reworked in M6.5, TD-102): a narrow column at the
//! viewport's right with a vertical slider of two handles, always available.
//!
//! The lower handle cuts the models below its height and the upper one above; dragging them
//! shows any slab of the plate, from below and from above. Heights snap to the plate's planned
//! layers ([`crate::layer_plan_ui::Planner`]: the resin calibration's height, or the plate's
//! layer plan) and each handle's layer number and height are fields, typed or stepped. Handles
//! keep their place on the slider, a fraction of the models' height, so turning or scaling a
//! model never moves them: the cut follows the model instead. The viewport caps every closed
//! solid the cut opens, so a hollowed model shows its wall's cross-section around the cavity
//! ([`crate::viewport::Section`]). Double-clicking the slider shows everything again.
//!
//! **The active handle** is the one last dragged, clicked near or typed into, outlined. Keyboard
//! shortcuts move it ([`SectionStep`], owner R10 2026-11-10): by default the Up and Down arrows
//! a layer, Page Up and Page Down ten, Home and End to the plate and the top. They work without
//! the slider having the focus, so the cut can be stepped while another tool is used, and they
//! are set in Settings › Keyboard shortcuts › Section. As every shortcut, they wait while a field
//! is typed in or a dialog is open, and while another control holds the keyboard (one reached
//! with Tab, or the layer image, which steps its own layers with the same keys).
//!
//! **Layer heights (TD-102):** the plate's ranges of adaptive or fixed layers are bands on the
//! slider. A right click on it offers to add a range from the layer clicked, and on a band to
//! edit or remove it, in a window ([`crate::layer_plan_ui::RangeWindow`]). The cut itself changes
//! nothing about the plate; the ranges do, as undoable plate edits.
use crate::{
    i18n::{decimal, tr},
    layer_plan_ui::{self, nearest, Planner, Range, RangeWindow},
    messages::{Level, Message},
    theme,
    viewport::Section,
    widgets::{Unit, UnitField},
    workspace::Workspace,
};
use eframe::egui;
use std::sync::Arc;

/// Width of the panel, in points.
pub const WIDTH: f32 = 104.0;
/// Layer height used when the plate has no calibrated resin, in mm.
const FALLBACK_LAYER_MM: f64 = 0.05;
/// Layers a Page Up or Page Down moves.
const PAGE_LAYERS: isize = 10;

/// A step of the active handle, which a keyboard shortcut takes (owner R10, 2026-11-10).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SectionStep {
    /// A layer up.
    LayerUp,
    /// A layer down.
    LayerDown,
    /// Ten layers up.
    PageUp,
    /// Ten layers down.
    PageDown,
    /// Down to the plate.
    Bottom,
    /// Up to the top.
    Top,
}

impl SectionStep {
    /// Every step, in the order the settings list them.
    pub const ALL: [Self; 6] = [
        Self::LayerUp,
        Self::LayerDown,
        Self::PageUp,
        Self::PageDown,
        Self::Bottom,
        Self::Top,
    ];

    /// Its name in the settings.
    #[must_use]
    pub fn label(self) -> String {
        match self {
            Self::LayerUp => tr!("section-step-layer-up"),
            Self::LayerDown => tr!("section-step-layer-down"),
            Self::PageUp => tr!("section-step-page-up"),
            Self::PageDown => tr!("section-step-page-down"),
            Self::Bottom => tr!("section-step-bottom"),
            Self::Top => tr!("section-step-top"),
        }
    }

    /// The layers it moves: up positive; the plate and the top as the extremes.
    const fn layers(self) -> isize {
        match self {
            Self::LayerUp => 1,
            Self::LayerDown => -1,
            Self::PageUp => PAGE_LAYERS,
            Self::PageDown => -PAGE_LAYERS,
            Self::Bottom => isize::MIN,
            Self::Top => isize::MAX,
        }
    }
}

/// Which handle a drag moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Handle {
    Lower,
    Upper,
}

/// The section panel's state.
pub struct SectionUi {
    /// The lowest height kept, as a fraction of the models' top: 0 rests on the plate.
    lower: f64,
    /// The highest height kept, as a fraction of the models' top: 1 rests at the top.
    upper: f64,
    dragging: Option<Handle>,
    /// The handle the keys and the typed fields move.
    active: Handle,
    /// The slider's identity, once shown: its shortcuts work while it or nothing holds the
    /// keyboard.
    slider_id: Option<egui::Id>,
    /// The plate's planned layers.
    planner: Planner,
    /// The height a right click opened the menu at.
    menu_at: Option<f64>,
    /// The window adding or editing a layer range.
    window: Option<RangeWindow>,
    /// Messages for the window: ranges applied or removed.
    outbox: Vec<Message>,
}

impl Default for SectionUi {
    fn default() -> Self {
        Self {
            lower: 0.0,
            upper: 1.0,
            dragging: None,
            active: Handle::Upper,
            slider_id: None,
            planner: Planner::default(),
            menu_at: None,
            window: None,
            outbox: Vec::new(),
        }
    }
}

/// The plate's layer height: the resin calibration's for the printer.
fn layer_height(workspace: &Workspace) -> f64 {
    layer_plan_ui::calibration(workspace).map_or(FALLBACK_LAYER_MM, |c| c.layer_height_mm)
}

/// Uniform layers of `layer` mm up to `top`, rounded up to a layer: without a plan.
fn uniform(layer: f64, top: f64) -> Vec<f64> {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // Layer counts.
    let count = ((top / layer).ceil() as usize).max(1);
    #[allow(clippy::cast_precision_loss)] // Layer counts.
    (0..=count).map(|k| k as f64 * layer).collect()
}

/// The screen height of a slider `fraction` between `y_top` (1) and `y_bottom` (0).
#[allow(clippy::cast_possible_truncation)] // Screen positions.
fn slider_y(fraction: f64, y_top: f32, y_bottom: f32) -> f32 {
    y_bottom - fraction as f32 * (y_bottom - y_top)
}

impl SectionUi {
    /// The layer boundaries the handles snap to: the planned ones, else uniform layers of the
    /// calibrated (or a default) height up to the models' top. Empty without models.
    fn boundaries(&self, workspace: &mut Workspace) -> Arc<Vec<f64>> {
        let planned = self.planner.boundaries();
        if planned.len() > 1 {
            return planned;
        }
        let top = workspace
            .placements()
            .iter()
            .flatten()
            .map(|placement| placement.max.z)
            .reduce(f64::max);
        Arc::new(top.map_or_else(Vec::new, |top| uniform(layer_height(workspace), top)))
    }

    /// The section to draw: the planes that cut something.
    #[must_use]
    pub fn section(&self, workspace: &mut Workspace) -> Section {
        let boundaries = self.boundaries(workspace);
        self.section_at(&boundaries)
    }

    /// The section over the layer `boundaries`.
    fn section_at(&self, boundaries: &[f64]) -> Section {
        let Some(&top) = boundaries.last() else {
            return Section::default();
        };
        let [low, high] = self.heights(boundaries);
        Section {
            below: (low > 0.0).then_some(low),
            above: (high < top).then_some(high),
        }
    }

    /// The positions in `boundaries` of the lowest and highest heights kept, at least a layer
    /// apart.
    fn indices(&self, boundaries: &[f64]) -> [usize; 2] {
        let top = boundaries.last().copied().unwrap_or(0.0);
        let at = |fraction: f64| nearest(boundaries, fraction * top).unwrap_or(0);
        let low = at(self.lower).min(boundaries.len().saturating_sub(2));
        let high = at(self.upper)
            .max(low + 1)
            .min(boundaries.len().saturating_sub(1));
        [low, high]
    }

    /// The lowest and highest heights kept, on layer boundaries, at least a layer apart.
    fn heights(&self, boundaries: &[f64]) -> [f64; 2] {
        if boundaries.is_empty() {
            return [0.0; 2];
        }
        self.indices(boundaries).map(|k| boundaries[k])
    }

    /// Put `handle` at layer boundary `index`, keeping the other at least a layer away.
    fn set_layer(&mut self, handle: Handle, index: usize, boundaries: &[f64]) {
        let Some(&top) = boundaries.last().filter(|&&t| t > 0.0) else {
            return;
        };
        let [low, high] = self.indices(boundaries);
        let index = match handle {
            Handle::Lower => index.min(high.saturating_sub(1)),
            Handle::Upper => index.max(low + 1).min(boundaries.len() - 1),
        };
        let fraction = boundaries[index] / top;
        match handle {
            Handle::Lower => self.lower = fraction,
            Handle::Upper => self.upper = fraction,
        }
        self.active = handle;
    }

    /// Keep everything below `z` (mm), cutting the models there, so that what lies at `z` shows
    /// from above; the lower handle goes back to the plate.
    pub fn cut_above(&mut self, workspace: &mut Workspace, z: f64) {
        let boundaries = self.boundaries(workspace);
        let Some(&top) = boundaries.last().filter(|&&t| t > 0.0) else {
            return;
        };
        (self.lower, self.upper) = (0.0, (z / top).clamp(0.0, 1.0));
    }

    /// The handles' places, to go back to.
    #[must_use]
    pub fn view(&self) -> [f64; 2] {
        [self.lower, self.upper]
    }

    /// Put the handles back at `view`.
    pub fn set_view(&mut self, [lower, upper]: [f64; 2]) {
        (self.lower, self.upper) = (lower, upper);
    }

    /// The plate's planned layers, for the Slice panel.
    #[must_use]
    pub fn planner(&self) -> &Planner {
        &self.planner
    }

    /// Messages produced since the last call.
    pub fn take_messages(&mut self) -> Vec<Message> {
        std::mem::take(&mut self.outbox)
    }

    /// The panel's content: each handle's layer and height over and under the two-handle
    /// slider, with the plate's layer ranges on it.
    pub fn panel(&mut self, ui: &mut egui::Ui, workspace: &mut Workspace) {
        if self.planner.update(workspace) {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(100));
        }
        ui.vertical_centered(|ui| {
            ui.label(egui::RichText::new(tr!("section-title")).strong())
                .on_hover_text(tr!("section-hint"));
        });
        let boundaries = self.boundaries(workspace);
        if boundaries.len() < 2 {
            ui.label(egui::RichText::new("–").color(theme::BORDER.color()));
            return;
        }
        self.fields(ui, Handle::Upper, &boundaries);
        let fields_height = 2.0 * (ui.spacing().interact_size.y + 18.0) + 16.0;
        let height = (ui.available_height() - fields_height).max(80.0);
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), height),
            egui::Sense::click_and_drag(),
        );
        self.slider(ui, &response, rect, workspace, &boundaries);
        // To screen readers, a slider at the active handle's layer (TD-142).
        let [low, high] = self.indices(&boundaries);
        let (layer, place) = if self.active == Handle::Upper {
            (high, tr!("a11y-section-top", what = tr!("section-title")))
        } else {
            (low, tr!("a11y-section-bottom", what = tr!("section-title")))
        };
        #[allow(clippy::cast_precision_loss)] // Layer numbers.
        response.widget_info(|| egui::WidgetInfo::slider(true, layer as f64, &place));
        self.fields(ui, Handle::Lower, &boundaries);
    }

    /// A handle's layer and height, typed: framed while it is the active handle.
    fn fields(&mut self, ui: &mut egui::Ui, handle: Handle, boundaries: &[f64]) {
        let [low, high] = self.indices(boundaries);
        let index = if handle == Handle::Upper { high } else { low };
        #[allow(clippy::cast_precision_loss)] // Layer numbers.
        let mut layer = index as f64;
        let mut z = boundaries[index];
        #[allow(clippy::cast_precision_loss)] // Layer numbers.
        let last = (boundaries.len() - 1) as f64;
        let stroke = if self.active == handle {
            egui::Stroke::new(1.0, theme::ACCENT.color())
        } else {
            egui::Stroke::NONE
        };
        egui::Frame::new()
            .stroke(stroke)
            .corner_radius(6.0)
            .inner_margin(2.0)
            .show(ui, |ui| {
                let width = ui.available_width();
                let small = |text: String| {
                    egui::RichText::new(text)
                        .small()
                        .color(theme::BORDER.color())
                };
                ui.label(small(tr!("section-layer-label")));
                let typed_layer = ui.add(
                    UnitField::new(&mut layer, Unit::Count)
                        .width(width)
                        .range(0.0..=last)
                        .step(1.0)
                        .decimals(0),
                );
                ui.label(small(tr!("section-z-label")));
                let typed_z = ui.add(
                    UnitField::new(&mut z, Unit::Millimetre)
                        .width(width)
                        .range(0.0..=boundaries[boundaries.len() - 1])
                        .step(0.05),
                );
                let named = |label: String| {
                    if handle == Handle::Upper {
                        tr!("a11y-section-top", what = label)
                    } else {
                        tr!("a11y-section-bottom", what = label)
                    }
                };
                crate::widgets::name(&typed_layer, &named(tr!("section-layer-label")));
                crate::widgets::name(&typed_z, &named(tr!("section-z-label")));
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // In range.
                if typed_layer.changed() {
                    self.set_layer(handle, layer.round() as usize, boundaries);
                } else if typed_z.changed() {
                    if let Some(k) = nearest(boundaries, z) {
                        self.set_layer(handle, k, boundaries);
                    }
                } else if typed_layer.gained_focus() || typed_z.gained_focus() {
                    self.active = handle;
                }
            });
    }

    /// The slider: dragged, clicked, stepped with the keys while focused, and its menu.
    fn slider(
        &mut self,
        ui: &mut egui::Ui,
        response: &egui::Response,
        rect: egui::Rect,
        workspace: &mut Workspace,
        boundaries: &[f64],
    ) {
        let top = boundaries[boundaries.len() - 1];
        let (y_top, y_bottom) = (rect.top() + 8.0, rect.bottom() - 8.0);
        let z_of = |y: f32| f64::from((y_bottom - y) / (y_bottom - y_top)) * top;
        let at = |fraction: f64| slider_y(fraction, y_top, y_bottom);
        let nearer = |pointer: egui::Pos2, this: &Self| {
            if (pointer.y - at(this.lower)).abs() < (pointer.y - at(this.upper)).abs() {
                Handle::Lower
            } else {
                Handle::Upper
            }
        };
        if response.drag_started() || response.clicked() {
            response.request_focus();
            if let Some(pointer) = response.interact_pointer_pos() {
                let handle = nearer(pointer, self);
                self.active = handle;
                if response.drag_started() {
                    self.dragging = Some(handle);
                } else if let Some(k) = nearest(boundaries, z_of(pointer.y)) {
                    self.set_layer(handle, k, boundaries);
                }
            }
        }
        // Only while the button is held: the release frame may already report a pointer elsewhere.
        if let (Some(handle), Some(pointer), true) = (
            self.dragging,
            response.interact_pointer_pos(),
            response.dragged(),
        ) {
            if let Some(k) = nearest(boundaries, z_of(pointer.y)) {
                self.set_layer(handle, k, boundaries);
            }
        }
        if response.drag_stopped() {
            self.dragging = None;
        }
        if response.double_clicked() {
            (self.lower, self.upper) = (0.0, 1.0);
        }
        self.slider_id = Some(response.id);
        if response.has_focus() {
            // The arrows step the handle (a shortcut) rather than moving the focus.
            ui.memory_mut(|m| {
                m.set_focus_lock_filter(
                    response.id,
                    egui::EventFilter {
                        vertical_arrows: true,
                        ..Default::default()
                    },
                );
            });
        }
        if response.secondary_clicked() {
            self.menu_at = response
                .interact_pointer_pos()
                .and_then(|p| nearest(boundaries, z_of(p.y)))
                .map(|k| boundaries[k]);
        }
        let ranges = layer_plan_ui::ranges(workspace);
        response.context_menu(|ui| self.menu(ui, workspace, &ranges, boundaries));
        self.paint(
            ui,
            response,
            rect.center().x,
            [y_top, y_bottom],
            &ranges,
            top,
        );
    }

    /// The slider's identity, once shown.
    #[must_use]
    pub const fn slider_id(&self) -> Option<egui::Id> {
        self.slider_id
    }

    /// Move the active handle by `step` among `workspace`'s layers (a shortcut, owner R10).
    pub fn step(&mut self, workspace: &mut Workspace, step: SectionStep) {
        let boundaries = self.boundaries(workspace);
        self.step_in(step, &boundaries);
    }

    /// Move the active handle by `step` among the layer `boundaries`.
    fn step_in(&mut self, step: SectionStep, boundaries: &[f64]) {
        if boundaries.len() < 2 {
            return;
        }
        let [low, high] = self.indices(boundaries);
        let current = if self.active == Handle::Upper {
            high
        } else {
            low
        };
        let last = boundaries.len() - 1;
        let index = match step.layers() {
            isize::MIN => 0,
            isize::MAX => last,
            layers => current.saturating_add_signed(layers).min(last),
        };
        if index != current {
            self.set_layer(self.active, index, boundaries);
        }
    }

    /// The slider's menu at the height clicked: add a range from there; on a band, edit or
    /// remove it; show everything.
    fn menu(
        &mut self,
        ui: &mut egui::Ui,
        workspace: &mut Workspace,
        ranges: &[Range],
        boundaries: &[f64],
    ) {
        let Some(z) = self.menu_at else {
            ui.close();
            return;
        };
        let layer = nearest(boundaries, z).unwrap_or(0);
        ui.label(
            egui::RichText::new(tr!("section-menu-from", z = decimal(z, 2), layer = layer))
                .small()
                .color(theme::BORDER.color()),
        );
        let band = ranges
            .iter()
            .enumerate()
            .filter(|(_, r)| r.from_mm <= z)
            .max_by(|a, b| a.1.from_mm.total_cmp(&b.1.from_mm))
            .map(|(k, r)| (k, *r));
        if let Some((k, range)) = band {
            if ui
                .button(tr!("section-edit-range", what = range.name()))
                .clicked()
            {
                self.window = Some(RangeWindow::edit(workspace, range));
                ui.close();
            }
            if ui
                .button(tr!("section-remove-range", what = range.name()))
                .clicked()
            {
                let mut rest = ranges.to_vec();
                rest.remove(k);
                layer_plan_ui::set(workspace, &rest, &layer_plan_ui::exposures(workspace));
                self.outbox.push(Message::new(
                    Level::Info,
                    tr!("layers-removed", what = range.describe()),
                ));
                ui.close();
            }
            ui.separator();
        }
        // Ranges need the calibration they start above.
        let calibrated = layer_plan_ui::calibration(workspace).is_some();
        ui.add_enabled_ui(calibrated, |ui| {
            for (adaptive, label) in [
                (true, tr!("section-add-adaptive")),
                (false, tr!("section-add-fixed")),
            ] {
                if ui
                    .button(label)
                    .on_disabled_hover_text(tr!("section-add-needs-profiles"))
                    .clicked()
                {
                    self.window = Some(RangeWindow::add(workspace, z, adaptive));
                    ui.close();
                }
            }
        });
        ui.separator();
        if ui.button(tr!("section-show-all")).clicked() {
            (self.lower, self.upper) = (0.0, 1.0);
            ui.close();
        }
    }

    /// The window adding or editing a range, while it is open.
    pub fn window(&mut self, ctx: &egui::Context, workspace: &mut Workspace) {
        let Some(window) = &mut self.window else {
            return;
        };
        let boundaries = self.planner.boundaries();
        let (open, message) = window.show(ctx, workspace, &boundaries);
        self.outbox.extend(message);
        if !open {
            self.window = None;
        }
    }

    /// The slider's track, the plate's layer ranges as bands, the kept range and the two
    /// handles, the active one outlined, between `ys` (top, bottom).
    ///
    /// Handles are drawn at their fractions, not at the heights snapped to layers, so they hold
    /// still while a turning model changes the snapped heights.
    fn paint(
        &self,
        ui: &egui::Ui,
        response: &egui::Response,
        x: f32,
        ys: [f32; 2],
        ranges: &[Range],
        top: f64,
    ) {
        let painter = ui.painter();
        let [y_top, y_bottom] = ys;
        let at = |fraction: f64| egui::pos2(x, slider_y(fraction, y_top, y_bottom));
        painter.line_segment(
            [egui::pos2(x, y_top), egui::pos2(x, y_bottom)],
            egui::Stroke::new(3.0, theme::BORDER.color()),
        );
        let mut starts: Vec<f64> = ranges.iter().map(|r| r.from_mm).collect();
        starts.sort_by(f64::total_cmp);
        for range in ranges.iter().filter(|r| r.from_mm < top) {
            let until = starts
                .iter()
                .copied()
                .find(|&s| s > range.from_mm)
                .unwrap_or(top)
                .min(top);
            let band = egui::Rect::from_two_pos(
                at(range.from_mm / top) - egui::vec2(8.0, 0.0),
                at(until / top) + egui::vec2(8.0, 0.0),
            );
            painter.rect_filled(band, 3.0, range.colour().gamma_multiply(0.85));
        }
        painter.line_segment(
            [at(self.lower), at(self.upper)],
            egui::Stroke::new(5.0, theme::ACCENT.color()),
        );
        for (handle, fraction) in [(Handle::Lower, self.lower), (Handle::Upper, self.upper)] {
            let at = at(fraction);
            let hovered = response
                .hover_pos()
                .is_some_and(|p| (p.y - at.y).abs() < 8.0);
            let color = if self.dragging == Some(handle) || hovered {
                theme::INTERACTION.color()
            } else {
                theme::PRIMARY.color()
            };
            let knob = egui::Rect::from_center_size(at, egui::vec2(30.0, 10.0));
            painter.rect_filled(knob, 3.0, color);
            if self.active == handle {
                painter.rect_stroke(
                    knob.expand(2.0),
                    4.0,
                    egui::Stroke::new(1.5, theme::TEXT_PRIMARY.color()),
                    egui::StrokeKind::Outside,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cut_above_keeps_what_lies_below_a_height() {
        // A 20 mm cube (TD-101): cut at 7 mm, the section keeps the plate to there.
        let fixtures =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/reference/stl");
        let mut workspace = Workspace::new();
        workspace.open(fixtures.join("fx-01-cube.stl"));
        crate::workspace::tests::wait(&mut workspace);
        let mut section = SectionUi::default();
        assert_eq!(section.section(&mut workspace), Section::default());
        section.cut_above(&mut workspace, 7.0);
        let cut = section.section(&mut workspace);
        assert_eq!(cut.below, None);
        let above = cut.above.unwrap();
        assert!((above - 7.0).abs() <= layer_height(&workspace), "{above}");
    }

    #[test]
    fn heights_snap_to_layers_and_only_real_cuts_count() {
        let ten = uniform(0.05, 10.0);
        assert_eq!(ten.len(), 201);
        assert_eq!(nearest(&ten, 1.234), Some(25));
        // Handles at the ends cut nothing, whatever the models' top; a raised lower handle cuts
        // below, a lowered upper one above.
        let mut ui = SectionUi::default();
        assert_eq!(ui.section_at(&ten), Section::default());
        assert_eq!(ui.section_at(&uniform(0.05, 25.0)), Section::default());
        (ui.lower, ui.upper) = (0.2, 0.65);
        let cut = ui.section_at(&ten);
        assert!((cut.below.unwrap() - 2.0).abs() < 1e-9);
        assert!((cut.above.unwrap() - 6.5).abs() < 1e-9);
        // A model turned taller keeps the handles where they were: the cut scales with it, on
        // layers.
        let cut = ui.section_at(&uniform(0.05, 20.0));
        assert!((cut.below.unwrap() - 4.0).abs() < 1e-9);
        assert!((cut.above.unwrap() - 13.0).abs() < 1e-9);
        // Handles never cross: at least a layer is kept.
        (ui.lower, ui.upper) = (0.5, 0.5);
        let [low, high] = ui.heights(&ten);
        assert!((high - low - 0.05).abs() < 1e-9);
    }

    #[test]
    fn handles_snap_to_variable_layers_and_step_with_the_keys() {
        // Five layers of 0.05 mm, then layers of 0.025 mm up to 0.5 mm.
        let mut boundaries: Vec<f64> = (0..=5).map(|k| f64::from(k) * 0.05).collect();
        boundaries.extend((1..=10).map(|k| 0.25 + f64::from(k) * 0.025));
        let mut ui = SectionUi::default();
        ui.set_layer(Handle::Upper, 7, &boundaries);
        assert_eq!(ui.active, Handle::Upper);
        assert!((ui.heights(&boundaries)[1] - 0.3).abs() < 1e-9);
        // The lower handle stays at least a layer under the upper one.
        ui.set_layer(Handle::Lower, 12, &boundaries);
        assert_eq!(ui.indices(&boundaries), [6, 7]);
        // The steps move the active handle by layers: ten up (to the top, 15), one down; the
        // lower one to the plate, and back to the top stops a layer under the upper one.
        ui.active = Handle::Upper;
        for (step, expected) in [(SectionStep::PageUp, 15), (SectionStep::LayerDown, 14)] {
            ui.step_in(step, &boundaries);
            assert_eq!(ui.indices(&boundaries)[1], expected);
        }
        ui.active = Handle::Lower;
        ui.step_in(SectionStep::Bottom, &boundaries);
        assert_eq!(ui.indices(&boundaries)[0], 0);
        ui.step_in(SectionStep::LayerUp, &boundaries);
        assert_eq!(ui.indices(&boundaries)[0], 1);
        ui.step_in(SectionStep::PageDown, &boundaries);
        assert_eq!(ui.indices(&boundaries)[0], 0);
        ui.step_in(SectionStep::Top, &boundaries);
        assert_eq!(ui.indices(&boundaries), [13, 14]);
    }
}
