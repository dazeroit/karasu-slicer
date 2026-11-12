//! The Transform tool's panel (M6.5, owner's mockup): the axes of the handles, then Move, Rotate
//! and Scale.
//!
//! **Axes:** Global or Local, a sliding choice at the top; it steers the handles only. Scaling
//! always follows the model's own axes, so the choice is disabled, showing Local, in the Scale
//! mode.
//!
//! **Sections:** each has a header that shows its handles when clicked (the Move, Rotate or
//! Scale mode), icon buttons at its right, and the model's X, Y and Z values below, which follow
//! the handles as they are dragged:
//! - Move: the position of the model's centre in X and Y, and the height of its lowest point in
//!   Z, in millimetres on the plate; Drop to plate and Center.
//! - Rotate: the model's angles about the plate's X, Y and Z, in degrees; Reset.
//! - Scale: the model's scale along its own axes, in percent; Uniform (a typed value scales
//!   the three axes alike; the handles have their own uniform handle), Reset, and Fit to plate.
//!
//! **Locks:** each section's lock (a setting of the tool, kept until turned off) stops its
//! handles, values and buttons from changing any model; the other sections stay free.
//!
//! Every value typed is an undoable plate edit; repeated edits of one value merge into one step.
//! The panel's actions go back to the window, which performs them ([`Action`]).
use crate::{
    actions::Action,
    appearance::Appearance,
    i18n::tr,
    icons::{Icon, Icons},
    manipulate::{Mode, Space},
    theme,
    widgets::{choice, icon_button, Unit, UnitField},
    workspace::Workspace,
};
use eframe::egui;

/// The Transform tool's settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TransformUi {
    /// Whether moving, turning and scaling are locked, in that order.
    pub locked: [bool; 3],
    /// Whether a typed scale scales the three axes alike.
    pub uniform: bool,
}

impl Default for TransformUi {
    fn default() -> Self {
        Self {
            locked: [false; 3],
            uniform: true,
        }
    }
}

/// The section of `mode`, as an index into [`TransformUi::locked`].
fn slot(mode: Mode) -> usize {
    match mode {
        Mode::Move => 0,
        Mode::Rotate => 1,
        Mode::Scale => 2,
    }
}

/// Undo merge keys of typed values, apart from the drags' (small counters).
const MERGE_KEYS: u64 = 0x5452_414E_5346_0000; // "TRANSF"

impl TransformUi {
    /// Whether `mode`'s transform is locked.
    #[must_use]
    pub fn is_locked(self, mode: Mode) -> bool {
        self.locked[slot(mode)]
    }

    /// Lock `mode`'s transform, or unlock it.
    pub fn toggle_lock(&mut self, mode: Mode) {
        self.locked[slot(mode)] ^= true;
    }

    /// The panel for the selected model `index` if any, with the handles in `mode` along
    /// `space`. Values typed are applied here; the action clicked is returned for the window.
    #[allow(clippy::too_many_arguments)] // The panel's state, the plate and how it draws.
    pub fn panel(
        &mut self,
        ui: &mut egui::Ui,
        workspace: &mut Workspace,
        index: Option<usize>,
        mode: Mode,
        space: Space,
        icons: Option<&Icons>,
        appearance: &Appearance,
    ) -> Option<Action> {
        let mut action = None;
        // The axes of the handles.
        ui.add_enabled_ui(mode != Mode::Scale, |ui| {
            let shown = if mode == Mode::Scale {
                Space::Local
            } else {
                space
            };
            let mut chosen = usize::from(shown == Space::Local);
            let labels = [tr!("transform-global"), tr!("transform-local")];
            if choice(ui, &mut chosen, &labels)
                .on_hover_text(tr!("action-toggle-axes"))
                .changed()
            {
                action = Some(Action::ToggleAxes);
            }
        });
        ui.add_space(4.0);
        for section in [Mode::Move, Mode::Rotate, Mode::Scale] {
            if let Some(chosen) = self.section(
                ui,
                workspace,
                index,
                section,
                mode == section,
                icons,
                appearance,
            ) {
                action = Some(chosen);
            }
            ui.add_space(4.0);
        }
        action
    }

    /// One section: its header, its values and its buttons.
    #[allow(clippy::too_many_arguments)] // As the panel's.
    fn section(
        &mut self,
        ui: &mut egui::Ui,
        workspace: &mut Workspace,
        index: Option<usize>,
        section: Mode,
        active: bool,
        icons: Option<&Icons>,
        appearance: &Appearance,
    ) -> Option<Action> {
        let mut action = None;
        let locked = self.is_locked(section);
        let frame = egui::Frame::new()
            .corner_radius(8)
            .stroke(egui::Stroke::new(
                1.0,
                if active {
                    theme::INTERACTION.color()
                } else {
                    theme::BORDER.color().gamma_multiply(0.6)
                },
            ))
            .inner_margin(egui::Margin::same(0));
        frame.show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 0.0;
            if let Some(header) = self.header(ui, section, active, locked, index.is_some(), icons) {
                action = Some(header);
            }
            egui::Frame::new()
                .inner_margin(egui::Margin::symmetric(10, 8))
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 6.0;
                    ui.add_enabled_ui(index.is_some() && !locked, |ui| {
                        values(ui, workspace, index, section, self.uniform, appearance);
                        let buttons: &[Action] = match section {
                            Mode::Move => &[Action::DropToPlate, Action::CenterModel],
                            Mode::Rotate => &[],
                            Mode::Scale => &[Action::FitToPlate],
                        };
                        if !buttons.is_empty() {
                            ui.horizontal(|ui| {
                                let gap = ui.spacing().item_spacing.x;
                                #[allow(clippy::cast_precision_loss)] // One or two buttons.
                                let count = buttons.len() as f32;
                                let width = (ui.available_width() - gap * (count - 1.0)) / count;
                                for &button in buttons {
                                    let clicked = ui
                                        .add_sized(
                                            [width, ui.spacing().interact_size.y],
                                            egui::Button::new(button.label()),
                                        )
                                        .clicked();
                                    if clicked {
                                        action = Some(button);
                                    }
                                }
                            });
                        }
                    });
                });
        });
        action
    }

    /// A section's header: its icon and name, which show its handles when clicked, and its icon
    /// buttons at the right.
    fn header(
        &mut self,
        ui: &mut egui::Ui,
        section: Mode,
        active: bool,
        locked: bool,
        selected: bool,
        icons: Option<&Icons>,
    ) -> Option<Action> {
        let mut action = None;
        let (icon, name, show) = match section {
            Mode::Move => (Icon::Transform, tr!("mode-move"), Action::MoveMode),
            Mode::Rotate => (Icon::Rotate, tr!("mode-rotate"), Action::RotateMode),
            Mode::Scale => (Icon::Scale, tr!("mode-scale"), Action::ScaleMode),
        };
        let fill = if active {
            theme::PRIMARY.color()
        } else {
            theme::SURFACE.color().gamma_multiply(1.4)
        };
        let corner = egui::CornerRadius {
            nw: 8,
            ne: 8,
            sw: 0,
            se: 0,
        };
        egui::Frame::new()
            .fill(fill)
            .corner_radius(corner)
            .inner_margin(egui::Margin::symmetric(8, 4))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    // The right side first, so the name takes what is left.
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let lock_hint = if locked {
                            tr!("transform-unlock")
                        } else {
                            tr!("transform-lock")
                        };
                        let lock_icon = if locked {
                            Icon::LockClosed
                        } else {
                            Icon::LockOpen
                        };
                        if icon_button(ui, icons, lock_icon, locked, &lock_hint).clicked() {
                            action = Some(Action::ToggleLock(section));
                        }
                        let free = selected && !locked;
                        match section {
                            Mode::Move => {}
                            Mode::Rotate => {
                                let reset = ui.add_enabled_ui(free, |ui| {
                                    icon_button(
                                        ui,
                                        icons,
                                        Icon::Reset,
                                        false,
                                        &tr!("transform-reset-rotation"),
                                    )
                                });
                                if reset.inner.clicked() {
                                    action = Some(Action::ResetRotation);
                                }
                            }
                            Mode::Scale => {
                                let reset = ui.add_enabled_ui(free, |ui| {
                                    icon_button(
                                        ui,
                                        icons,
                                        Icon::Reset,
                                        false,
                                        &tr!("transform-reset-scale"),
                                    )
                                });
                                if reset.inner.clicked() {
                                    action = Some(Action::ResetScale);
                                }
                                let hint = if self.uniform {
                                    tr!("transform-uniform-on")
                                } else {
                                    tr!("transform-uniform-off")
                                };
                                if icon_button(ui, icons, Icon::Link, self.uniform, &hint).clicked()
                                {
                                    action = Some(Action::ToggleUniform);
                                }
                            }
                        }
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            if title(ui, icons, icon, &name).clicked() {
                                action = Some(show);
                            }
                        });
                    });
                });
            });
        action
    }
}

/// A section's icon and name over the header's free width, clickable, the name cut rather
/// than widening the panel.
fn title(ui: &mut egui::Ui, icons: Option<&Icons>, icon: Icon, name: &str) -> egui::Response {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(width, ui.spacing().interact_size.y),
        egui::Sense::click(),
    );
    let colour = theme::TEXT_PRIMARY.color();
    let icon_rect = egui::Rect::from_center_size(
        egui::pos2(rect.left() + 10.0, rect.center().y),
        egui::Vec2::splat(18.0),
    );
    if let Some(icons) = icons {
        icons.paint(ui.painter(), icon, icon_rect, colour);
    }
    let galley = ui.painter().layout_no_wrap(
        name.to_uppercase(),
        egui::FontId::proportional(13.0),
        colour,
    );
    let at = egui::pos2(
        icon_rect.right() + 8.0,
        rect.center().y - galley.size().y / 2.0,
    );
    ui.painter().with_clip_rect(rect).galley(at, galley, colour);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, name));
    response.on_hover_text(tr!("transform-show-handles"))
}

/// A section's X, Y and Z values: the selected model's, or with none the neutral ones (zero, or
/// 100 percent), disabled. Typed values are applied to the model.
fn values(
    ui: &mut egui::Ui,
    workspace: &mut Workspace,
    index: Option<usize>,
    section: Mode,
    uniform: bool,
    appearance: &Appearance,
) {
    let current = index.and_then(|i| current(workspace, i, section));
    let mut shown = current.unwrap_or(match section {
        Mode::Scale => [100.0; 3],
        Mode::Move | Mode::Rotate => [0.0; 3],
    });
    let (unit, range, step, decimals) = match section {
        Mode::Move => (Unit::Millimetre, -10_000.0..=10_000.0, 0.1, 2),
        Mode::Rotate => (Unit::Degree, -360.0..=360.0, 1.0, 1),
        Mode::Scale => (Unit::Percent, 1.0..=100_000.0, 1.0, 1),
    };
    for axis in 0..3 {
        let changed = ui
            .horizontal(|ui| {
                let letter = ["X", "Y", "Z"][axis];
                ui.add_sized(
                    [16.0, ui.spacing().interact_size.y],
                    egui::Label::new(
                        egui::RichText::new(letter)
                            .strong()
                            .color(appearance.axes[axis].color()),
                    ),
                );
                let field = ui.add(
                    UnitField::new(&mut shown[axis], unit)
                        .range(range.clone())
                        .step(step)
                        .decimals(decimals),
                );
                let mode = match section {
                    Mode::Move => tr!("mode-move"),
                    Mode::Rotate => tr!("mode-rotate"),
                    Mode::Scale => tr!("mode-scale"),
                };
                crate::widgets::name(&field, &tr!("a11y-axis", mode = mode, axis = letter));
                field.changed()
            })
            .inner;
        if let (true, Some(index), Some(before)) = (changed, index, current) {
            apply(
                workspace,
                index,
                section,
                axis,
                shown[axis],
                before,
                uniform,
            );
        }
    }
}

/// The values of model `index` for `section`: its centre's X and Y and its lowest point's
/// height, its angles, or its scale in percent.
fn current(workspace: &mut Workspace, index: usize, section: Mode) -> Option<[f64; 3]> {
    match section {
        Mode::Move => {
            let (anchor, placement) = workspace.anchor(index)?;
            Some([anchor[0], anchor[1], placement.min.z])
        }
        Mode::Rotate => Some(workspace.plate().objects.get(index)?.pose.rotate_deg),
        Mode::Scale => Some(
            workspace
                .plate()
                .objects
                .get(index)?
                .pose
                .scale
                .map(|s| s * 100.0),
        ),
    }
}

/// Apply value `value` typed for `axis` of `section` to model `index`, whose values were
/// `before`; a uniform scale scales the three axes alike.
fn apply(
    workspace: &mut Workspace,
    index: usize,
    section: Mode,
    axis: usize,
    value: f64,
    before: [f64; 3],
    uniform: bool,
) {
    let translation = workspace
        .anchor(index)
        .map(|(_, placement)| placement.translation_mm);
    #[allow(clippy::cast_possible_truncation)] // Small indices.
    let key = MERGE_KEYS + (slot(section) * 3 + axis) as u64;
    workspace.edit_merging(key, |plate| {
        let Some(object) = plate.objects.get_mut(index) else {
            return;
        };
        match section {
            // X and Y move the centre, from where the model is placed now; Z sets the lowest
            // point's height, resting on the plate at 0.
            Mode::Move if axis < 2 => {
                if let Some(translation) = translation {
                    object.translate_mm = translation;
                    object.translate_mm[axis] += value - before[axis];
                    object.pose.center = false;
                }
            }
            Mode::Move => {
                object.pose.drop = true;
                object.pose.lift_mm = (value > 1e-9).then_some(value);
            }
            Mode::Rotate => object.pose.rotate_deg[axis] = value,
            Mode::Scale => {
                let ratio = value / before[axis].max(1e-9);
                if uniform {
                    for scale in &mut object.pose.scale {
                        *scale = (*scale * ratio).clamp(0.01, 1000.0);
                    }
                } else {
                    object.pose.scale[axis] = (value / 100.0).clamp(0.01, 1000.0);
                }
            }
        }
    });
}

/// Whether `object` is off the plate: moved up freely (not resting), or resting at a height
/// typed in the Move section (a lift). Drop to plate rests it either way.
#[must_use]
pub fn is_lifted(object: &karasu_project::project::PlateObject) -> bool {
    !object.pose.drop || object.pose.lift_mm.is_some_and(|lift| lift > 0.0)
}

/// The uniform factor that makes a model of placed extents `size` (mm) as large as fits the
/// build volume `volume` (width, depth, height), a hair inside it.
#[must_use]
pub fn fit_factor(size: [f64; 3], volume: [f64; 3]) -> Option<f64> {
    let factor = (0..3)
        .filter(|&k| size[k] > 1e-9)
        .map(|k| volume[k] / size[k])
        .fold(f64::INFINITY, f64::min);
    (factor.is_finite() && factor > 0.0).then_some(factor * FIT_MARGIN)
}

/// How much of the build volume Fit to plate fills: a hair less than all, so the model lies
/// inside it after rounding.
const FIT_MARGIN: f64 = 0.999;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fitting_takes_the_tightest_axis_and_stays_inside() {
        // 10 x 20 x 5 mm in a 100 x 100 x 200 mm volume: Y binds, at five times.
        let factor = fit_factor([10.0, 20.0, 5.0], [100.0, 100.0, 200.0]).unwrap();
        assert!((factor - 5.0 * FIT_MARGIN).abs() < 1e-12);
        assert!(20.0 * factor <= 100.0);
        // A flat model (no height) fits by its other axes; nothing to fit is none.
        assert!(fit_factor([10.0, 10.0, 0.0], [100.0, 50.0, 10.0]).is_some());
        assert_eq!(fit_factor([0.0; 3], [100.0; 3]), None);
    }

    #[test]
    fn locks_belong_to_their_section() {
        let mut tool = TransformUi::default();
        tool.toggle_lock(Mode::Rotate);
        assert!(tool.is_locked(Mode::Rotate));
        assert!(!tool.is_locked(Mode::Move) && !tool.is_locked(Mode::Scale));
        tool.toggle_lock(Mode::Rotate);
        assert!(!tool.is_locked(Mode::Rotate));
        assert!(TransformUi::default().uniform);
    }

    #[test]
    fn the_panel_fits_its_width_in_english_and_in_a_longer_language() {
        let fixtures =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/reference/stl");
        let mut workspace = Workspace::new();
        workspace.open(fixtures.join("fx-01-cube.stl"));
        crate::workspace::tests::wait(&mut workspace);
        let width = 316.0;
        for locale in ["en-US", "pseudo"] {
            crate::i18n::install(crate::i18n::Catalog::open(locale, None).unwrap());
            for index in [None, Some(0)] {
                let mut tool = TransformUi::default();
                let ctx = egui::Context::default();
                let mut used = 0.0;
                for _ in 0..2 {
                    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                        ui.allocate_ui(egui::vec2(width, 4000.0), |ui| {
                            ui.set_width(width);
                            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                            let action = tool.panel(
                                ui,
                                &mut workspace,
                                index,
                                Mode::Scale,
                                Space::Local,
                                None,
                                &Appearance::default(),
                            );
                            assert_eq!(action, None);
                            used = ui.min_rect().width();
                        });
                    });
                    output.textures_delta.clear();
                }
                assert!(
                    used <= width + 0.5,
                    "{locale} {index:?}: {used} wide in {width}"
                );
            }
        }
        crate::i18n::install(crate::i18n::Catalog::reference());
    }

    #[test]
    fn typed_values_move_rotate_and_scale_the_model() {
        let fixtures =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/reference/stl");
        let mut workspace = Workspace::new();
        workspace.open(fixtures.join("fx-01-cube.stl"));
        crate::workspace::tests::wait(&mut workspace);
        // X moves the centre to the value typed.
        let before = current(&mut workspace, 0, Mode::Move).unwrap();
        apply(
            &mut workspace,
            0,
            Mode::Move,
            0,
            before[0] + 12.5,
            before,
            true,
        );
        let moved = current(&mut workspace, 0, Mode::Move).unwrap();
        assert!((moved[0] - (before[0] + 12.5)).abs() < 1e-9, "{moved:?}");
        assert!((moved[1] - before[1]).abs() < 1e-9);
        // Z lifts the lowest point to the height typed, and 0 rests it again; lifted so, the
        // model is off the plate (Drop to plate is offered), and dropping rests it.
        apply(&mut workspace, 0, Mode::Move, 2, 3.0, moved, true);
        assert!((current(&mut workspace, 0, Mode::Move).unwrap()[2] - 3.0).abs() < 1e-9);
        assert!(is_lifted(&workspace.plate().objects[0]));
        workspace.drop_to_plate(0);
        assert!(!is_lifted(&workspace.plate().objects[0]));
        assert!(current(&mut workspace, 0, Mode::Move).unwrap()[2].abs() < 1e-9);
        apply(&mut workspace, 0, Mode::Move, 2, 3.0, moved, true);
        let lifted = current(&mut workspace, 0, Mode::Move).unwrap();
        apply(&mut workspace, 0, Mode::Move, 2, 0.0, lifted, true);
        assert!(current(&mut workspace, 0, Mode::Move).unwrap()[2].abs() < 1e-9);
        // An angle typed is the model's angle.
        let angles = current(&mut workspace, 0, Mode::Rotate).unwrap();
        apply(&mut workspace, 0, Mode::Rotate, 2, 30.0, angles, true);
        assert_eq!(
            current(&mut workspace, 0, Mode::Rotate).unwrap(),
            [0.0, 0.0, 30.0]
        );
        // A uniform scale scales the three axes alike; otherwise one.
        let scale = current(&mut workspace, 0, Mode::Scale).unwrap();
        apply(&mut workspace, 0, Mode::Scale, 0, 200.0, scale, true);
        let doubled = current(&mut workspace, 0, Mode::Scale).unwrap();
        for value in doubled {
            assert!((value - 200.0).abs() < 1e-9, "{doubled:?}");
        }
        apply(&mut workspace, 0, Mode::Scale, 1, 50.0, doubled, false);
        let one = current(&mut workspace, 0, Mode::Scale).unwrap();
        assert!(
            (one[0] - 200.0).abs() < 1e-9 && (one[1] - 50.0).abs() < 1e-9,
            "{one:?}"
        );
        // Repeated typing of one value is one undo step: undoing it returns to 200 %.
        workspace.undo();
        assert!((current(&mut workspace, 0, Mode::Scale).unwrap()[1] - 200.0).abs() < 1e-9);
    }
}
