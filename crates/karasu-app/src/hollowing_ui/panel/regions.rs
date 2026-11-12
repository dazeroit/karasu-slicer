//! Where the cavity traps resin (M6.5, TD-101): the Drain holes panel's Drainage section, with
//! a Show button on every cavity region no hole drains, and the markers that locate those regions
//! in the viewport.
//!
//! The core gives each cavity region a point on its outer surface and its bounds, in world
//! millimetres as prepared. With the Drain holes tool on the selected model, every undrained
//! region has an orange marker drawn through the model like the holes' ghosts, a constant size
//! on screen, smaller for pockets under 1 mm³; the pointer on one says what it marks. Show, or a
//! click on a marker, asks the window to frame the region and to section the model through it
//! ([`HollowingUi::take_region`]), so the cavity shows cut open. Nothing is drawn or computed
//! for a model without undrained regions; the marker mesh is uploaded once.
use super::{prepared, unprepared, HollowingUi, Prepared, TINY_REGION_MM3};
use crate::{
    appearance::Appearance,
    camera::{Camera, Mat4},
    i18n::{decimal, tr},
    icons::{Icon, Icons},
    preparation_ui::PreparationUi,
    repair_panel::{marker, MARKER_SIZE},
    theme,
    viewport::{Draw, Viewport},
    workspace::Workspace,
};
use eframe::{egui, egui_wgpu};
use karasu_core::preparation::sampled::CavityRegion;

/// Viewport mesh key of the cavity marker, apart from the other keys.
const MARKER_KEY: u64 = 0x4341_5649_5459_4D4B; // "CAVITYMK"
/// Pockets listed one by one at most; the rest are counted.
const POCKETS_LISTED: usize = 20;

/// A cavity region no hole drains: its position among the object's regions, whether it is a
/// pocket under 1 mm³ (and then its number among them), its volume and where it is.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Trap {
    region: usize,
    pocket: Option<usize>,
    volume_mm3: f64,
    point: [f64; 3],
    bounds: [[f64; 3]; 2],
}

impl Trap {
    /// What it is, for its row and its marker's tooltip.
    fn text(&self) -> String {
        let volume = decimal(self.volume_mm3, usize::from(self.pocket.is_some()));
        match self.pocket {
            Some(number) => tr!("prepare-pocket-tip", number = number, volume = volume),
            None => tr!(
                "prepare-region-enclosed",
                number = self.region + 1,
                volume = volume
            ),
        }
    }
}

/// The cavity regions of a prepared object no hole drains, in region order; pockets under 1 mm³
/// are numbered among themselves.
fn traps(regions: &[CavityRegion]) -> Vec<Trap> {
    let mut pockets = 0;
    regions
        .iter()
        .enumerate()
        .filter(|(_, r)| r.drained_by.is_empty())
        .map(|(region, r)| {
            let pocket = (r.volume_mm3 < TINY_REGION_MM3).then(|| {
                pockets += 1;
                pockets
            });
            Trap {
                region,
                pocket,
                volume_mm3: r.volume_mm3,
                point: r.point_mm,
                bounds: r.bounds_mm,
            }
        })
        .collect()
}

/// The undrained regions of object `index` in the current preparation, if it has any holes.
fn current_traps(
    workspace: &Workspace,
    preparation: &PreparationUi,
    index: usize,
) -> Option<Vec<Trap>> {
    let Prepared::Done(object) = prepared(workspace, preparation, index) else {
        return None;
    };
    Some(traps(object.holes()?.regions()))
}

/// A line of the section: `icon` in `colour`, the text wrapping, and a Show button at the right
/// when `show` (its response, or `None`).
fn line(
    ui: &mut egui::Ui,
    icons: Option<&Icons>,
    (icon, colour, size): (Icon, egui::Color32, f32),
    text: String,
    show: bool,
    highlighted: bool,
) -> Option<egui::Response> {
    let frame = egui::Frame::new()
        .corner_radius(6.0)
        .inner_margin(2.0)
        .fill(if highlighted {
            theme::INTERACTION.color().gamma_multiply(0.35)
        } else {
            egui::Color32::TRANSPARENT
        })
        .stroke(if highlighted {
            egui::Stroke::new(1.0, theme::ACCENT.color())
        } else {
            egui::Stroke::NONE
        });
    frame
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    let button = show.then(|| {
                        ui.small_button(tr!("prepare-region-show"))
                            .on_hover_text(tr!("prepare-region-show-hint"))
                    });
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Min), |ui| {
                        let (rect, _) =
                            ui.allocate_exact_size(egui::Vec2::splat(16.0), egui::Sense::hover());
                        if let Some(icons) = icons {
                            icons.paint(
                                ui.painter(),
                                icon,
                                egui::Rect::from_center_size(
                                    rect.center(),
                                    egui::Vec2::splat(size),
                                ),
                                colour,
                            );
                        }
                        ui.add(egui::Label::new(text).wrap());
                    });
                    button
                })
                .inner
            })
            .inner
        })
        .inner
}

impl HollowingUi {
    /// The Drainage section for object `index`: each cavity region with the holes that drain it,
    /// a Show button on each that none drains, the pockets under 1 mm³ counted and listed on
    /// demand, then the markers' switch and legend.
    pub(super) fn drainage(
        &mut self,
        ui: &mut egui::Ui,
        workspace: &Workspace,
        preparation: &PreparationUi,
        index: Option<usize>,
        icons: Option<&Icons>,
    ) {
        self.drainage_lines(ui, workspace, preparation, index, icons);
        ui.add_space(4.0);
        ui.separator();
        // A view preference of the tool: always available.
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let markers = crate::widgets::switch(ui, &mut self.cavity.markers);
                crate::widgets::name(&markers, &tr!("prepare-cavity-markers"));
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.add(egui::Label::new(tr!("prepare-cavity-markers")).wrap());
                });
            });
        });
        let swatch = |ui: &mut egui::Ui, radius: f32, text: String| {
            ui.horizontal(|ui| {
                let (rect, _) =
                    ui.allocate_exact_size(egui::Vec2::splat(14.0), egui::Sense::hover());
                ui.painter()
                    .circle_filled(rect.center(), radius, theme::WARNING.color());
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(text)
                            .small()
                            .color(ui.visuals().weak_text_color()),
                    )
                    .wrap(),
                );
            });
        };
        swatch(ui, 6.0, tr!("prepare-marker-region"));
        swatch(ui, 3.5, tr!("prepare-marker-pocket"));
    }

    /// The Drainage section's lines.
    fn drainage_lines(
        &mut self,
        ui: &mut egui::Ui,
        workspace: &Workspace,
        preparation: &PreparationUi,
        index: Option<usize>,
        icons: Option<&Icons>,
    ) {
        let weak = |ui: &mut egui::Ui, text: String| {
            ui.add(
                egui::Label::new(egui::RichText::new(text).color(ui.visuals().weak_text_color()))
                    .wrap(),
            );
        };
        let Some(i) = index else {
            weak(ui, "–".to_owned());
            return;
        };
        if workspace
            .plate()
            .objects
            .get(i)
            .is_none_or(|o| o.hollowing().is_none())
        {
            weak(ui, tr!("prepare-no-cavity"));
            return;
        }
        let prepared = prepared(workspace, preparation, i);
        if let Some(state) = unprepared(&prepared) {
            weak(ui, state.text);
            return;
        }
        let Prepared::Done(object) = prepared else {
            return;
        };
        let Some(holes) = object.holes() else {
            let text = if object.hollowed().is_some() {
                tr!("prepare-no-drain-hole")
            } else {
                tr!("prepare-no-cavity")
            };
            weak(ui, text);
            return;
        };
        let shown = self.cavity.shown.filter(|(o, _)| *o == i).map(|(_, r)| r);
        let mut show = None;
        let warning = (Icon::Warning, theme::WARNING.color(), 16.0);
        let traps = traps(holes.regions());
        for (number, region) in (1usize..).zip(holes.regions()) {
            if region.volume_mm3 < TINY_REGION_MM3 {
                continue;
            }
            if region.drained_by.is_empty() {
                let trap = traps.iter().find(|t| t.region == number - 1);
                let text = trap.map_or_else(String::new, Trap::text);
                let button = line(ui, icons, warning, text, true, shown == Some(number - 1));
                if button.is_some_and(|b| b.clicked()) {
                    show = trap.copied();
                }
            } else {
                let holes: Vec<String> =
                    region.drained_by.iter().map(ToString::to_string).collect();
                line(
                    ui,
                    icons,
                    (Icon::Success, theme::SUCCESS.color(), 16.0),
                    tr!(
                        "prepare-region-drained",
                        number = number,
                        volume = decimal(region.volume_mm3, 0),
                        holes = holes.join(", ")
                    ),
                    false,
                    false,
                );
            }
        }
        if let Some(trap) = self.pocket_lines(ui, &traps, shown, icons) {
            show = Some(trap);
        }
        if let Some(trap) = show {
            self.show_trap(i, &trap);
        }
    }

    /// The pockets under 1 mm³ among `traps`: counted, then listed on demand, each with Show.
    /// Returns the one whose Show was clicked.
    fn pocket_lines(
        &mut self,
        ui: &mut egui::Ui,
        traps: &[Trap],
        shown: Option<usize>,
        icons: Option<&Icons>,
    ) -> Option<Trap> {
        let weak = |ui: &mut egui::Ui, text: String| {
            ui.add(
                egui::Label::new(egui::RichText::new(text).color(ui.visuals().weak_text_color()))
                    .wrap(),
            );
        };
        let warning = (Icon::Warning, theme::WARNING.color(), 16.0);
        let mut show = None;
        let pockets: Vec<&Trap> = traps.iter().filter(|t| t.pocket.is_some()).collect();
        if pockets.is_empty() {
            return None;
        }
        line(
            ui,
            icons,
            warning,
            tr!("prepare-tiny-regions", count = pockets.len()),
            false,
            false,
        );
        let label = if self.cavity.pockets_open {
            tr!("prepare-pockets-hide")
        } else {
            tr!("prepare-pockets-list")
        };
        ui.indent("pockets", |ui| {
            if ui.link(label).clicked() {
                self.cavity.pockets_open = !self.cavity.pockets_open;
            }
            if self.cavity.pockets_open {
                for trap in pockets.iter().take(POCKETS_LISTED) {
                    let text = tr!(
                        "prepare-pocket",
                        number = trap.pocket.unwrap_or(0),
                        volume = decimal(trap.volume_mm3, 1)
                    );
                    let small = (Icon::Warning, theme::WARNING.color(), 11.0);
                    let highlighted = shown == Some(trap.region);
                    if line(ui, icons, small, text, true, highlighted).is_some_and(|b| b.clicked())
                    {
                        show = Some(**trap);
                    }
                }
                if pockets.len() > POCKETS_LISTED {
                    weak(
                        ui,
                        tr!(
                            "prepare-pockets-more",
                            count = pockets.len() - POCKETS_LISTED
                        ),
                    );
                }
            }
        });
        show
    }

    /// Ask the window to frame `trap` of object `index` and section the model through it.
    fn show_trap(&mut self, index: usize, trap: &Trap) {
        self.cavity.shown = Some((index, trap.region));
        self.cavity.request = Some(trap.bounds);
    }

    /// The bounds of a cavity region asked to be shown, or of a suction cup chosen (its
    /// footprint at its widest layer), once: the window frames it and sections the model
    /// through its middle.
    pub fn take_region(&mut self) -> Option<[[f64; 3]; 2]> {
        self.cavity
            .request
            .take()
            .or_else(|| self.cups.take_region())
    }

    /// The region shown is no longer: the view went back to the whole model.
    pub fn forget_region(&mut self) {
        self.cavity.shown = None;
    }

    /// The markers of object `focus`'s undrained cavity regions (the Drain holes tool on the
    /// selected model), drawn through the model, a constant size on screen as seen from `eye`.
    #[allow(clippy::too_many_arguments)] // The viewport, the plate and the eye.
    pub fn cavity_draws(
        &mut self,
        viewport: &mut Viewport,
        state: &egui_wgpu::RenderState,
        workspace: &Workspace,
        preparation: &PreparationUi,
        focus: Option<usize>,
        eye: [f64; 3],
        appearance: &Appearance,
    ) -> Vec<Draw> {
        self.cavity.marks.clear();
        let Some(index) = focus.filter(|_| self.cavity.markers) else {
            return Vec::new();
        };
        let Some(traps) = current_traps(workspace, preparation, index).filter(|t| !t.is_empty())
        else {
            return Vec::new();
        };
        if !self.cavity.uploaded {
            viewport.upload(&state.device, MARKER_KEY, &marker(), [0.0; 3]);
            self.cavity.uploaded = true;
        }
        let shown = self
            .cavity
            .shown
            .filter(|(o, _)| *o == index)
            .map(|(_, r)| r);
        let color = appearance.marker_hollowing.linear();
        let mut ghost = color;
        ghost[3] *= 0.5;
        let mut draws = Vec::new();
        for trap in &traps {
            let p = trap.point;
            let scale = match (shown == Some(trap.region), trap.pocket) {
                (true, _) => 1.8,
                (false, None) => 1.3,
                (false, Some(_)) => 0.75,
            };
            let distance =
                ((p[0] - eye[0]).powi(2) + (p[1] - eye[1]).powi(2) + (p[2] - eye[2]).powi(2))
                    .sqrt();
            let size = MARKER_SIZE * scale * distance;
            let model = Mat4([
                [size, 0.0, 0.0, p[0]],
                [0.0, size, 0.0, p[1]],
                [0.0, 0.0, size, p[2]],
                [0.0, 0.0, 0.0, 1.0],
            ]);
            for (color, overlay) in [(color, false), (ghost, true)] {
                draws.push(Draw {
                    mesh: MARKER_KEY,
                    model,
                    color,
                    overlay,
                    cap: crate::viewport::Cap::Off,
                    holes: [0, 0],
                    bore: None,
                    outside: None,
                    triangles: None,
                    look: crate::viewport::Look::Solid,
                });
            }
            self.cavity
                .marks
                .push((index, (trap.region, trap.bounds, trap.point), trap.text()));
        }
        draws
    }

    /// The cavity marker under `pointer` in the viewport `rect`, if any: its tooltip.
    #[must_use]
    pub fn cavity_marker_at(
        &self,
        camera: &Camera,
        rect: egui::Rect,
        pointer: egui::Pos2,
    ) -> Option<&str> {
        self.cavity_mark_at(camera, rect, pointer)
            .map(|k| self.cavity.marks[k].2.as_str())
    }

    /// A click at `pointer`: on a cavity marker, its region is shown. Returns whether it was on
    /// one.
    pub fn click_cavity_marker(
        &mut self,
        camera: &Camera,
        rect: egui::Rect,
        pointer: egui::Pos2,
    ) -> bool {
        let Some(k) = self.cavity_mark_at(camera, rect, pointer) else {
            return false;
        };
        let (index, (region, bounds, _), _) = &self.cavity.marks[k];
        let (index, region, bounds) = (*index, *region, *bounds);
        self.cavity.shown = Some((index, region));
        self.cavity.request = Some(bounds);
        true
    }

    /// The position in the drawn marks of the one nearest `pointer`, within reach.
    fn cavity_mark_at(
        &self,
        camera: &Camera,
        rect: egui::Rect,
        pointer: egui::Pos2,
    ) -> Option<usize> {
        const REACH: f32 = 10.0;
        let size = [f64::from(rect.width()), f64::from(rect.height())];
        self.cavity
            .marks
            .iter()
            .enumerate()
            .filter_map(|(k, (_, (_, _, point), _))| {
                let [x, y] = camera.screen(*point, size)?;
                #[allow(clippy::cast_possible_truncation)] // Screen positions.
                let at = rect.min + egui::vec2(x as f32, y as f32);
                let distance = at.distance(pointer);
                (distance <= REACH).then_some((distance, k))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, k)| k)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn region(volume_mm3: f64, drained_by: &[u64], at: f64) -> CavityRegion {
        CavityRegion {
            volume_mm3,
            surfaces: 1,
            drained_by: drained_by.to_vec(),
            point_mm: [at; 3],
            bounds_mm: [[at - 1.0; 3], [at + 1.0; 3]],
        }
    }

    #[test]
    fn undrained_regions_are_located_and_pockets_numbered_among_themselves() {
        let regions = [
            region(8412.0, &[1], 0.0),
            region(0.6, &[], 10.0),
            region(2.0, &[], 20.0),
            region(0.3, &[], 30.0),
            region(0.4, &[2], 40.0),
        ];
        let traps = traps(&regions);
        let found: Vec<(usize, Option<usize>, [f64; 3])> = traps
            .iter()
            .map(|t| (t.region, t.pocket, t.point))
            .collect();
        assert_eq!(
            found,
            [
                (1, Some(1), [10.0; 3]),
                (2, None, [20.0; 3]),
                (3, Some(2), [30.0; 3])
            ]
        );
        // The region keeps its number among all regions, as the Drainage section counts them.
        assert_eq!(
            traps[1].text(),
            tr!(
                "prepare-region-enclosed",
                number = 3_usize,
                volume = decimal(2.0, 0)
            )
        );
        assert_eq!(
            traps[2].text(),
            tr!(
                "prepare-pocket-tip",
                number = 2_usize,
                volume = decimal(0.3, 1)
            )
        );
    }

    #[test]
    fn a_region_is_shown_once_from_the_panel_or_its_marker() {
        let mut ui = HollowingUi::default();
        let trap = traps(&[region(2.0, &[], 5.0)])[0];
        ui.show_trap(0, &trap);
        assert_eq!(ui.cavity.shown, Some((0, 0)));
        assert_eq!(ui.take_region(), Some([[4.0; 3], [6.0; 3]]));
        assert_eq!(ui.take_region(), None);
        // A marker drawn at the camera's target is under the viewport's centre.
        let camera = Camera::new([5.0; 3], 100.0);
        let rect = egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(800.0, 600.0));
        ui.cavity
            .marks
            .push((0, (0, trap.bounds, trap.point), trap.text()));
        assert_eq!(
            ui.cavity_marker_at(&camera, rect, rect.center() + egui::vec2(3.0, -4.0)),
            Some(trap.text().as_str())
        );
        assert_eq!(ui.cavity_marker_at(&camera, rect, rect.left_top()), None);
        assert!(!ui.click_cavity_marker(&camera, rect, rect.left_top()));
        assert!(ui.click_cavity_marker(&camera, rect, rect.center()));
        assert_eq!(ui.take_region(), Some(trap.bounds));
        ui.forget_region();
        assert_eq!(ui.cavity.shown, None);
    }

    #[test]
    fn a_line_with_show_keeps_the_panels_width_in_a_longer_language() {
        let width = 316.0;
        for locale in ["en-US", "pseudo"] {
            crate::i18n::install(crate::i18n::Catalog::open(locale, None).unwrap());
            let ctx = egui::Context::default();
            let mut used = 0.0;
            for _ in 0..2 {
                let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                    ui.allocate_ui(egui::vec2(width, 4000.0), |ui| {
                        ui.set_width(width);
                        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                        let text = traps(&[region(2.0, &[], 5.0)])[0].text();
                        let warning = (Icon::Warning, theme::WARNING.color(), 16.0);
                        line(ui, None, warning, text, true, true);
                        used = ui.min_rect().width();
                    });
                });
                output.textures_delta.clear();
            }
            assert!(used <= width + 0.5, "{locale}: {used} wide in {width}");
        }
        crate::i18n::install(crate::i18n::Catalog::reference());
    }
}
