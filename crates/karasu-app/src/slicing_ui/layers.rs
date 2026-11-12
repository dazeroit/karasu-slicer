//! The Layers tool panel (M6.5, TD-103), drawn from the owner-approved mockup: the sliced
//! layers, one at a time.
//!
//! The body shows the exported layer, seen from above with the plate's front at the bottom, its
//! material white and the verification's marks over it; the wheel zooms at the pointer, a drag
//! moves a zoomed view, and a double click opens it in a larger window. With the image focused,
//! the arrows, Page Up and Page Down, Home and End browse the layers. "Follow in the viewport"
//! has the section cut the models at the layer shown. "This layer" gives its height, thickness,
//! exposure and kind as the print job resolved them, then its exposed area and its islands,
//! counted from its mask when it is shown. The fixed footer browses the layers and says which
//! one is shown, or that the slice is out of date.
use super::{legend, mark, plate_view, reduced, walls, SlicingUi, LAYER_IMAGE_MAX};
use crate::{
    i18n::{decimal, tr},
    icons::{Icon, Icons},
    theme,
    toasts::{notice, Severity},
    widgets::{outlined_icon_button, state_line, Unit, UnitField},
    workspace::Workspace,
};
use eframe::egui;
use karasu_core::{print_job::LayerPhase, raster::mask::SpanMask};
use karasu_project::project::Freshness;

/// The largest zoom of a layer image.
const MAX_ZOOM: f32 = 16.0;
/// Layers a Page Up or Page Down moves.
const PAGE_LAYERS: isize = 10;

/// How a layer image is shown: its zoom and the image point at its centre (0 to 1 each way).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Zoom {
    factor: f32,
    centre: egui::Vec2,
}

impl Default for Zoom {
    fn default() -> Self {
        Self {
            factor: 1.0,
            centre: egui::vec2(0.5, 0.5),
        }
    }
}

impl Zoom {
    /// The part of the image shown, in texture coordinates.
    fn uv(self) -> egui::Rect {
        let half = 0.5 / self.factor;
        egui::Rect::from_center_size(self.centre.to_pos2(), egui::Vec2::splat(2.0 * half))
    }

    /// Keep the view inside the image.
    fn clamp(&mut self) {
        self.factor = self.factor.clamp(1.0, MAX_ZOOM);
        let half = 0.5 / self.factor;
        self.centre.x = self.centre.x.clamp(half, 1.0 - half);
        self.centre.y = self.centre.y.clamp(half, 1.0 - half);
    }

    /// Zoom by `factor` keeping the image point under `at` (0 to 1 across the view) in place.
    fn zoom_at(&mut self, factor: f32, at: egui::Vec2) {
        let before = self.uv();
        let point = before.min.to_vec2() + at * before.size();
        self.factor *= factor;
        self.clamp();
        let size = self.uv().size();
        self.centre = point - at * size + size / 2.0;
        self.clamp();
    }

    /// Move the view by `delta` points of a view `size` points wide and tall.
    fn pan(&mut self, delta: egui::Vec2, size: egui::Vec2) {
        let scale = self.uv().size() / size.max(egui::Vec2::splat(1.0));
        self.centre -= delta * scale;
        self.clamp();
    }
}

/// How the Layers panel and its window show the layer.
#[derive(Clone, Copy, Debug)]
pub struct View {
    panel: Zoom,
    window: Zoom,
    /// The larger window is open.
    window_open: bool,
    /// The section cuts the models at the layer shown.
    pub follow: bool,
    /// The exposed area (mm²) and islands of the layer counted last, with its index.
    counted: Option<(usize, f64, usize)>,
}

impl Default for View {
    fn default() -> Self {
        Self {
            panel: Zoom::default(),
            window: Zoom::default(),
            window_open: false,
            follow: true,
            counted: None,
        }
    }
}

/// The cured area of `mask`, in mm², with `pixel_mm2` per pixel: each pixel weighs its
/// intensity, as the resin's volume does.
fn exposed_area(mask: &SpanMask, pixel_mm2: f64) -> f64 {
    let weighted: u64 = mask
        .rows()
        .flatten()
        .map(|s| u64::from(s.value()) * u64::from(s.end() - s.start()))
        .sum();
    #[allow(clippy::cast_precision_loss)] // Pixel counts.
    let pixels = weighted as f64 / 255.0;
    pixels * pixel_mm2
}

/// The separate cured regions of `mask`: runs joined across rows where they touch, corners
/// included.
fn islands(mask: &SpanMask) -> usize {
    fn find(parent: &mut [usize], mut k: usize) -> usize {
        while parent[k] != k {
            parent[k] = parent[parent[k]];
            k = parent[k];
        }
        k
    }
    let mut parent: Vec<usize> = (0..mask.span_count()).collect();
    let join = |parent: &mut Vec<usize>, a: usize, b: usize| {
        let (ra, rb) = (find(parent, a), find(parent, b));
        if ra != rb {
            parent[ra.max(rb)] = ra.min(rb);
        }
    };
    let mut previous: Option<(usize, &[karasu_core::raster::mask::Span])> = None;
    let mut first = 0;
    for row in mask.rows() {
        // Runs of one row that meet (different intensities side by side) are one region.
        for k in 1..row.len() {
            if row[k].start() == row[k - 1].end() {
                join(&mut parent, first + k - 1, first + k);
            }
        }
        if let Some((above_first, above)) = previous {
            let (mut i, mut j) = (0, 0);
            while i < above.len() && j < row.len() {
                // Touching or diagonal neighbours join.
                if above[i].start() <= row[j].end() && row[j].start() <= above[i].end() {
                    join(&mut parent, above_first + i, first + j);
                }
                if above[i].end() < row[j].end() {
                    i += 1;
                } else {
                    j += 1;
                }
            }
        }
        previous = Some((first, row));
        first += row.len();
    }
    (0..parent.len())
        .filter(|&k| find(&mut parent, k) == k)
        .count()
}

/// A layer's settings source, for "This layer".
fn kind(phase: LayerPhase) -> String {
    match phase {
        LayerPhase::Bottom => tr!("layers-kind-bottom"),
        LayerPhase::Transition => tr!("layers-kind-transition"),
        LayerPhase::Normal => tr!("layers-kind-normal"),
        LayerPhase::Override => tr!("layers-kind-plan"),
    }
}

impl SlicingUi {
    /// The number of layers of the last slice.
    fn layer_count(&self) -> usize {
        self.sliced.as_ref().map_or(0, |s| s.masks.len())
    }

    /// Show layer `index`, within the slice.
    fn go(&mut self, index: isize) {
        let count = self.layer_count();
        if count > 0 {
            self.layer = index.clamp(0, count.cast_signed() - 1).unsigned_abs();
        }
    }

    /// The top of the layer shown, in mm, while the panel follows it in the viewport.
    #[must_use]
    pub fn followed_top(&self) -> Option<f64> {
        let sliced = self.sliced.as_ref().filter(|_| self.view.follow)?;
        sliced
            .outcome
            .layers
            .get(self.layer)
            .map(|l| l.top_mm)
            .or_else(|| sliced.masks.get(self.layer).map(|m| m.z_mm))
    }

    /// The texture of the layer shown, with its marks, built again only when the layer or the
    /// focused finding changes.
    fn layer_texture(&mut self, ctx: &egui::Context) -> Option<egui::TextureHandle> {
        let sliced = self.sliced.as_ref()?;
        let mask = sliced.masks.get(self.layer)?;
        let focus = self.focus.filter(|f| f.layer == self.layer);
        if let Some((key, texture)) = &self.texture {
            if *key == (self.layer, focus) {
                return Some(texture.clone());
            }
        }
        let mut panel = reduced(&mask.mask, LAYER_IMAGE_MAX);
        if let Some(marks) = sliced
            .outcome
            .verification
            .as_ref()
            .and_then(|v| v.report.marks.get(self.layer))
        {
            mark(&mut panel, &mask.mask, marks, focus, LAYER_IMAGE_MAX);
        }
        let image = plate_view(&panel, sliced.orientation);
        let texture = ctx.load_texture("layer", image, egui::TextureOptions::NEAREST);
        self.texture = Some(((self.layer, focus), texture.clone()));
        Some(texture)
    }

    /// A layer image in `rect`: zoomed with the wheel at the pointer, moved with a drag, the
    /// keys browsing layers while focused. Returns whether it was double-clicked.
    fn image(
        &mut self,
        ui: &mut egui::Ui,
        rect: egui::Rect,
        texture: &egui::TextureHandle,
        in_window: bool,
    ) -> bool {
        let id = ui.id().with(("layer-image", in_window));
        let response = ui
            .interact(rect, id, egui::Sense::click_and_drag())
            .on_hover_text(tr!("layers-note"));
        let zoom = if in_window {
            &mut self.view.window
        } else {
            &mut self.view.panel
        };
        if response.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                let at = response
                    .hover_pos()
                    .map_or(egui::vec2(0.5, 0.5), |p| (p - rect.min) / rect.size());
                zoom.zoom_at((scroll / 200.0).exp(), at);
            }
        }
        if response.dragged() {
            zoom.pan(response.drag_delta(), rect.size());
        }
        let uv = zoom.uv();
        ui.painter().rect_filled(rect, 4.0, egui::Color32::BLACK);
        ui.painter()
            .image(texture.id(), rect, uv, egui::Color32::WHITE);
        if response.clicked() {
            response.request_focus();
        }
        if response.has_focus() {
            ui.painter().rect_stroke(
                rect,
                4.0,
                egui::Stroke::new(1.5, theme::ACCENT.color()),
                egui::StrokeKind::Outside,
            );
            self.keys(ui, id);
        }
        response.double_clicked()
    }

    /// The keys of a focused layer image: they browse the layers.
    fn keys(&mut self, ui: &egui::Ui, id: egui::Id) {
        ui.memory_mut(|m| {
            m.set_focus_lock_filter(
                id,
                egui::EventFilter {
                    vertical_arrows: true,
                    ..Default::default()
                },
            );
        });
        let steps = ui.input_mut(|i| {
            [
                (egui::Key::ArrowUp, 1),
                (egui::Key::ArrowDown, -1),
                (egui::Key::PageUp, PAGE_LAYERS),
                (egui::Key::PageDown, -PAGE_LAYERS),
                (egui::Key::Home, isize::MIN),
                (egui::Key::End, isize::MAX),
            ]
            .into_iter()
            .filter(|&(key, _)| i.consume_key(egui::Modifiers::NONE, key))
            .map(|(_, step)| step)
            .collect::<Vec<_>>()
        });
        for step in steps {
            match step {
                isize::MIN => self.go(0),
                isize::MAX => self.go(isize::MAX - 1),
                step => self.go(self.layer.cast_signed() + step),
            }
        }
    }

    /// The Layers panel's body: the layer, Follow in the viewport, this layer's facts and the
    /// marks' legend.
    pub fn layers_panel(&mut self, ui: &mut egui::Ui, icons: Option<&Icons>) {
        if self.layer_count() == 0 {
            notice(ui, Severity::Info, &tr!("layers-no-slice"), icons);
        }
        self.go(self.layer.cast_signed());
        let width = ui.available_width();
        let texture = self.layer_texture(ui.ctx());
        #[allow(clippy::cast_precision_loss)] // Display sizes.
        let height = texture.as_ref().map_or(width * 0.75, |t| {
            let [w, h] = t.size();
            (width * h as f32 / w.max(1) as f32).min(width * 1.2)
        });
        let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
        if let Some(texture) = &texture {
            if self.image(ui, rect, texture, false) {
                self.view.window_open = true;
            }
            self.zoom_buttons(ui, rect, icons);
        } else {
            ui.painter().rect_filled(rect, 4.0, egui::Color32::BLACK);
        }
        ui.add_space(4.0);
        super::panel::switch_row(
            ui,
            &mut self.view.follow,
            tr!("layers-follow"),
            tr!("layers-follow-hint"),
        );
        egui::CollapsingHeader::new(tr!("layers-this-layer"))
            .id_salt("layers-this-layer")
            .default_open(true)
            .show(ui, |ui| self.facts(ui));
        // The printed walls' findings, each showing its layer here, and the marks' legend.
        let mut chosen = None;
        if let Some(verification) = self
            .sliced
            .as_ref()
            .and_then(|s| s.outcome.verification.as_deref())
        {
            egui::CollapsingHeader::new(tr!("walls-title"))
                .id_salt("layers-walls")
                .default_open(true)
                .show(ui, |ui| {
                    chosen = walls(ui, verification);
                    ui.add_space(4.0);
                    legend(ui);
                });
        }
        if let Some(focus) = chosen {
            self.layer = focus.layer;
            self.focus = Some(focus);
        }
    }

    /// Zoom out, zoom in and the larger window, at the image's bottom right.
    fn zoom_buttons(&mut self, ui: &mut egui::Ui, rect: egui::Rect, icons: Option<&Icons>) {
        let side = ui.spacing().interact_size.y;
        let place = |k: f32| {
            egui::Rect::from_min_size(
                egui::pos2(
                    rect.right() - 6.0 - (3.0 - k) * (side + 4.0) + 4.0,
                    rect.bottom() - 6.0 - side,
                ),
                egui::Vec2::splat(side),
            )
        };
        for (k, icon, hint, factor) in [
            (0.0, Icon::Minus, tr!("layers-zoom-out"), 1.0 / 1.5),
            (1.0, Icon::Plus, tr!("layers-zoom-in"), 1.5),
        ] {
            let zoom = ui
                .scope_builder(egui::UiBuilder::new().max_rect(place(k)), |ui| {
                    crate::widgets::icon_button(ui, icons, icon, false, &hint)
                })
                .inner;
            if zoom.clicked() {
                self.view.panel.zoom_at(factor, egui::vec2(0.5, 0.5));
            }
        }
        let enlarge = ui
            .scope_builder(egui::UiBuilder::new().max_rect(place(2.0)), |ui| {
                crate::widgets::icon_button(
                    ui,
                    icons,
                    Icon::Enlarge,
                    false,
                    &tr!("layers-open-window"),
                )
            })
            .inner;
        if enlarge.clicked() {
            self.view.window_open = true;
        }
    }

    /// The layer's facts: as the print job resolved it, then its area and islands.
    fn facts(&mut self, ui: &mut egui::Ui) {
        let dash = || "–".to_owned();
        let Some(sliced) = &self.sliced else {
            for name in [
                tr!("layers-z-fact"),
                tr!("layers-thickness"),
                tr!("layers-exposure"),
                tr!("layers-kind"),
                tr!("layers-area"),
                tr!("layers-islands"),
            ] {
                crate::widgets::fact_row(ui, &name, &dash());
            }
            return;
        };
        let layer = sliced.outcome.layers.get(self.layer).copied();
        let mm = |v: f64| tr!("prepare-length-mm", length = decimal(v, 3));
        crate::widgets::fact_row(
            ui,
            &tr!("layers-z-fact"),
            &layer.map_or_else(dash, |l| mm(l.top_mm)),
        )
        .on_hover_text(tr!("layers-z-hint"));
        crate::widgets::fact_row(
            ui,
            &tr!("layers-thickness"),
            &layer.map_or_else(dash, |l| mm(l.thickness_mm)),
        );
        crate::widgets::fact_row(
            ui,
            &tr!("layers-exposure"),
            &layer.map_or_else(dash, |l| {
                tr!("layers-seconds", seconds = decimal(l.exposure_s, 2))
            }),
        );
        crate::widgets::fact_row(
            ui,
            &tr!("layers-kind"),
            &layer.map_or_else(dash, |l| kind(l.phase)),
        );
        // Counted once for the layer shown: a walk over its runs.
        let (area, count) = match self.view.counted {
            Some((index, area, count)) if index == self.layer => (area, count),
            _ => {
                let mask = &sliced.masks[self.layer].mask;
                let counted = (exposed_area(mask, sliced.pixel_mm2), islands(mask));
                self.view.counted = Some((self.layer, counted.0, counted.1));
                counted
            }
        };
        crate::widgets::fact_row(
            ui,
            &tr!("layers-area"),
            &tr!("layers-mm2", area = decimal(area, 1)),
        )
        .on_hover_text(tr!("layers-area-hint"));
        crate::widgets::fact_row(ui, &tr!("layers-islands"), &count.to_string())
            .on_hover_text(tr!("layers-islands-hint"));
    }

    /// The Layers panel's footer: the first, previous, typed, next and last layer, a slider,
    /// and which layer is shown or that the slice is out of date.
    pub fn layers_footer(
        &mut self,
        ui: &mut egui::Ui,
        workspace: &Workspace,
        icons: Option<&Icons>,
    ) {
        let count = self.layer_count();
        ui.add_enabled_ui(count > 0, |ui| {
            ui.horizontal(|ui| {
                let button = |ui: &mut egui::Ui, icon: Icon, hint: String| {
                    outlined_icon_button(ui, icons, icon, &hint).clicked()
                };
                let shown = self.layer.cast_signed();
                if button(ui, Icon::First, tr!("layers-first")) {
                    self.go(0);
                }
                if button(ui, Icon::ChevronLeft, tr!("layers-previous")) {
                    self.go(shown - 1);
                }
                #[allow(clippy::cast_precision_loss)] // Layer numbers.
                let mut number = (self.layer + 1) as f64;
                #[allow(clippy::cast_precision_loss)] // Layer numbers.
                let last = count.max(1) as f64;
                let field = ui.add(
                    UnitField::new(&mut number, Unit::Count)
                        .width(70.0)
                        .range(1.0..=last)
                        .step(1.0)
                        .decimals(0),
                );
                crate::widgets::name(&field, &tr!("a11y-layer"));
                #[allow(clippy::cast_possible_truncation)] // Within the layers.
                if field.changed() {
                    self.go(number.round() as isize - 1);
                }
                ui.label(
                    egui::RichText::new(tr!("layers-of", count = count))
                        .color(ui.visuals().weak_text_color()),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if button(ui, Icon::Last, tr!("layers-last")) {
                        self.go(isize::MAX - 1);
                    }
                    if button(ui, Icon::ChevronRight, tr!("layers-next")) {
                        self.go(shown + 1);
                    }
                });
            });
            let mut shown = self.layer + 1;
            let slider = crate::widgets::track_slider(ui, &mut shown, count)
                .on_hover_text(tr!("layers-slider-hint"));
            crate::widgets::name(&slider, &tr!("a11y-layer"));
            if slider.changed() {
                self.go(shown.cast_signed() - 1);
            }
        });
        ui.add_space(4.0);
        let stale = self
            .sliced
            .as_ref()
            .is_some_and(|s| workspace.freshness(s.revision) != Freshness::Current);
        let (text, colour, icon) = match &self.sliced {
            None => (tr!("slice-state-none"), theme::BORDER.color(), None),
            Some(_) if stale => (
                tr!("layers-stale"),
                theme::WARNING.color(),
                Some(crate::icons::Icon::Warning),
            ),
            Some(sliced) => {
                let z = sliced
                    .outcome
                    .layers
                    .get(self.layer)
                    .map_or(0.0, |l| l.top_mm);
                (
                    tr!(
                        "layers-shown",
                        layer = self.layer + 1,
                        count = count,
                        z = decimal(z, 3)
                    ),
                    theme::TEXT_PRIMARY.color(),
                    None,
                )
            }
        };
        state_line(ui, icons, icon, &text, colour);
    }

    /// The larger window of the layer shown, while it is open: zoomed with the wheel at the
    /// pointer and moved with a drag; its size is kept.
    pub fn layer_window(&mut self, ctx: &egui::Context, icons: Option<&Icons>) {
        if !self.view.window_open || self.layer_count() == 0 {
            return;
        }
        let Some(texture) = self.layer_texture(ctx) else {
            return;
        };
        let z = self
            .sliced
            .as_ref()
            .and_then(|s| s.outcome.layers.get(self.layer))
            .map_or(0.0, |l| l.top_mm);
        let title = tr!(
            "layers-shown",
            layer = self.layer + 1,
            count = self.layer_count(),
            z = decimal(z, 3)
        );
        let mut open = true;
        crate::dialog::tool_window(ctx, "layer-window", title, egui::vec2(640.0, 520.0))
            .open(&mut open)
            .show(ctx, |ui| {
                crate::dialog::remember_tool_window(ui, "layer-window");
                // The layers are browsed here too: a slider and the steps under the image.
                let bar = ui.spacing().interact_size.y + ui.spacing().item_spacing.y;
                let size =
                    (ui.available_size() - egui::vec2(0.0, bar)).max(egui::vec2(120.0, 90.0));
                let [w, h] = texture.size();
                #[allow(clippy::cast_precision_loss)] // Display sizes.
                let aspect = w as f32 / h.max(1) as f32;
                let fitted = if size.x / size.y > aspect {
                    egui::vec2(size.y * aspect, size.y)
                } else {
                    egui::vec2(size.x, size.x / aspect)
                };
                let (rect, _) = ui.allocate_exact_size(fitted, egui::Sense::hover());
                if self.image(ui, rect, &texture, true) {
                    self.view.window = Zoom::default();
                }
                self.window_bar(ui, icons);
            });
        self.view.window_open = open;
    }
}

impl SlicingUi {
    /// The larger window's bar: the previous layer, a slider and the next layer.
    fn window_bar(&mut self, ui: &mut egui::Ui, icons: Option<&Icons>) {
        let count = self.layer_count();
        ui.horizontal(|ui| {
            let shown = self.layer.cast_signed();
            if outlined_icon_button(ui, icons, Icon::ChevronLeft, &tr!("layers-previous")).clicked()
            {
                self.go(shown - 1);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if outlined_icon_button(ui, icons, Icon::ChevronRight, &tr!("layers-next"))
                    .clicked()
                {
                    self.go(shown + 1);
                }
                let mut layer = self.layer + 1;
                let slider = crate::widgets::track_slider(ui, &mut layer, count)
                    .on_hover_text(tr!("layers-slider-hint"));
                crate::widgets::name(&slider, &tr!("a11y-layer"));
                if slider.changed() {
                    self.go(layer.cast_signed() - 1);
                }
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::slicing_ui::tests::{finish, hollowed_cube, slicing_into};

    /// Draw the Layers panel and its footer 306 points wide, over two frames: the texts drawn
    /// and the width used.
    fn draw(slicing: &mut SlicingUi, workspace: &Workspace) -> (Vec<String>, f32) {
        let ctx = egui::Context::default();
        let mut texts = Vec::new();
        let mut used = 0.0;
        for _ in 0..2 {
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                ui.allocate_ui(egui::vec2(306.0, 4000.0), |ui| {
                    ui.set_width(306.0);
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                    slicing.layers_panel(ui, None);
                    slicing.layers_footer(ui, workspace, None);
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
    fn the_layers_panel_shows_a_sliced_layer_and_its_facts() {
        let folder = std::env::temp_dir().join(format!("karasu-layers-{}", std::process::id()));
        let workspace = hollowed_cube(&folder);
        let mut slicing = slicing_into(&folder);
        // Without a slice: the same structure, saying what it needs.
        let (texts, _) = draw(&mut slicing, &workspace);
        for expected in [
            tr!("layers-no-slice"),
            tr!("layers-follow"),
            tr!("layers-this-layer"),
            tr!("layers-exposure"),
            tr!("slice-state-none"),
        ] {
            assert!(texts.contains(&expected), "{expected} in {texts:?}");
        }
        assert_eq!(slicing.followed_top(), None);
        slicing.start(&workspace);
        finish(&mut slicing);
        let count = slicing.layer_count();
        assert!(count > 100, "{count}");
        // The first layer is a bottom layer, with the calibration's bottom exposure.
        let (texts, _) = draw(&mut slicing, &workspace);
        assert!(texts.contains(&tr!("layers-kind-bottom")), "{texts:?}");
        // Browsing stays within the slice, and Follow cuts at the layer's top.
        slicing.go(isize::MAX - 1);
        assert_eq!(slicing.layer, count - 1);
        slicing.go(-5);
        assert_eq!(slicing.layer, 0);
        slicing.go(40);
        let top = slicing.sliced.as_ref().unwrap().outcome.layers[40].top_mm;
        assert_eq!(slicing.followed_top(), Some(top));
        // A middle layer of the hollowed cube is one ring: one island, its area under the
        // cube's 8 x 8 mm.
        let (texts, _) = draw(&mut slicing, &workspace);
        assert!(texts.contains(&tr!("layers-kind-normal")), "{texts:?}");
        let (_, area, islands) = slicing.view.counted.unwrap();
        assert_eq!(islands, 1);
        assert!(area > 0.0 && area < 64.0, "{area}");
        // Every fact fits the panel, also in a longer language.
        for locale in ["en-US", "pseudo"] {
            crate::i18n::install(crate::i18n::Catalog::open(locale, None).unwrap());
            let (_, used) = draw(&mut slicing, &workspace);
            assert!(used <= 306.5, "{locale}: {used} wide");
        }
        crate::i18n::install(crate::i18n::Catalog::reference());
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_verified_slice_lists_its_findings_in_the_layers_panel() {
        let folder =
            std::env::temp_dir().join(format!("karasu-layers-walls-{}", std::process::id()));
        let workspace = hollowed_cube(&folder);
        let mut slicing = slicing_into(&folder);
        slicing.verify = true;
        slicing.start(&workspace);
        finish(&mut slicing);
        assert!(slicing
            .sliced
            .as_ref()
            .is_some_and(|s| s.outcome.verification.is_some()));
        let (texts, _) = draw(&mut slicing, &workspace);
        for expected in [tr!("walls-title"), tr!("walls-legend-pocket")] {
            assert!(texts.contains(&expected), "{expected} in {texts:?}");
        }
        // The cube is closed: its cavity is a pocket, listed with its layer.
        assert!(
            texts.iter().any(|t| t.starts_with(
                &tr!(
                    "walls-pocket",
                    number = 1_usize,
                    volume = "",
                    layer = 0_usize
                )[..6]
            )),
            "{texts:?}"
        );
        assert!(!slicing.take_show_layers());
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn the_exposed_area_weighs_each_pixel_by_its_intensity() {
        // Two full pixels and two half ones, 0.0025 mm² each.
        let mask = SpanMask::from_pixels(4, 1, &[255, 255, 128, 127]).unwrap();
        let area = exposed_area(&mask, 0.0025);
        assert!((area - 0.0075).abs() < 1e-12, "{area}");
    }

    #[test]
    fn islands_are_regions_joined_across_rows_and_corners() {
        // Two blocks apart, a diagonal pair joined at a corner, and a ring: three islands.
        #[rustfmt::skip]
        let pixels = [
            255, 255, 0,   0,   0,   0,   255, 0,
            255, 255, 0,   0,   0,   0,   0,   128,
            0,   0,   0,   255, 255, 255, 0,   0,
            0,   0,   0,   255, 0,   255, 0,   0,
            0,   0,   0,   255, 255, 255, 0,   0,
        ];
        let mask = SpanMask::from_pixels(8, 5, &pixels).unwrap();
        assert_eq!(islands(&mask), 3);
        assert_eq!(islands(&SpanMask::from_pixels(3, 3, &[0; 9]).unwrap()), 0);
        // Side by side with different intensities: one island.
        let edge = SpanMask::from_pixels(3, 1, &[255, 128, 255]).unwrap();
        assert_eq!(islands(&edge), 1);
    }

    #[test]
    fn a_zoom_keeps_the_point_under_the_pointer_and_stays_in_the_image() {
        let mut zoom = Zoom::default();
        zoom.zoom_at(4.0, egui::vec2(0.25, 0.25));
        let uv = zoom.uv();
        assert!((uv.width() - 0.25).abs() < 1e-6);
        // The image point that was under the pointer (a quarter in) is still a quarter in.
        let point = uv.min.to_vec2() + egui::vec2(0.25, 0.25) * uv.size();
        assert!(
            (point - egui::vec2(0.25, 0.25)).length() < 1e-6,
            "{point:?}"
        );
        zoom.pan(egui::vec2(10_000.0, 10_000.0), egui::vec2(100.0, 100.0));
        assert!(zoom.uv().min.x >= -1e-6 && zoom.uv().min.y >= -1e-6);
        zoom.zoom_at(0.01, egui::vec2(0.5, 0.5));
        assert_eq!(zoom, Zoom::default());
    }
}
