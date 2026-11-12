//! The sidebar's list of the plate's objects (M6.5, TD-098): a click selects an object, as a
//! click on it in the viewport does, the eye hides it from the scene (it is still printed), a
//! right click offers isolating it or moving or copying it to another plate, and a button on
//! the row (or the Delete key) deletes it. A row dragged onto a plate in the plates bar or the
//! overview moves its object there, with Ctrl copies it (TD-135).
//!
//! **Checked models** (owner, 2026-11-10): each row has a check box, apart from the selection
//! the tools work on. A bar over the list checks or clears them all, says how many are checked,
//! and hides or shows them, or deletes them, together.
use crate::{i18n::tr, icons::Icon, theme, workspace::Workspace};
use eframe::egui;
use karasu_project::project::PlateId;
use std::collections::BTreeSet;

/// What a model row carries while it is dragged (TD-135): the object's index on the plate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DraggedModel(pub usize);

/// What the user asked of the sidebar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Request {
    /// Close the sidebar.
    Close,
    /// Select object `index`.
    Select(usize),
    /// Delete object `index`.
    Delete(usize),
    /// Hide object `index`, or show it again.
    ToggleHidden(usize),
    /// Show object `index` alone, or end its isolation.
    Isolate(usize),
    /// Show every object.
    ShowAll,
    /// Move object `index` to plate `to` (a new plate for `None`), or copy it there.
    Transfer {
        index: usize,
        to: Option<PlateId>,
        copy: bool,
    },
    /// Export the model at this index (TD-139).
    ExportModel(usize),
    /// Add copies of the model at this index (owner, 2026-11-10).
    Clone(usize),
    /// Check the model at this index, or clear it.
    Check(usize, bool),
    /// Check every model, or clear them all.
    CheckAll(bool),
    /// Hide the checked models, or show them.
    HideChecked(bool),
    /// Delete the checked models, once confirmed.
    DeleteChecked,
}

/// A row's height in the list.
const ROW_HEIGHT: f32 = 26.0;
/// The room a row's check box takes at its left, in points.
const CHECK_WIDTH: f32 = 22.0;
/// The space left of a row's name.
const ROW_INSET: f32 = 8.0;
/// The rows the list keeps in view however much the plate's section takes.
pub const MIN_ROWS: f32 = 5.0;

/// The height the list of `count` objects takes, with its rows' spacing.
pub fn list_height(ui: &egui::Ui, count: usize) -> f32 {
    #[allow(clippy::cast_precision_loss)] // A few rows.
    let rows = count as f32;
    rows * (ROW_HEIGHT + ui.spacing().item_spacing.y)
}

/// The header of the list: the objects' count, and how many the scene shows while some are
/// hidden.
pub fn objects_title(workspace: &Workspace) -> String {
    let count = workspace.plate().objects.len();
    let hidden = workspace.hidden_count();
    if hidden == 0 {
        tr!("sidebar-objects", count = count)
    } else {
        tr!(
            "sidebar-objects-shown",
            count = count,
            shown = count - hidden
        )
    }
}

/// The plate's objects, `selected` lit, one row each: the file name from the left, shortened in
/// the middle when it does not fit and keeping its extension (the full name on hover), faint
/// when the object is hidden; then the eye, which hides or shows it, and the delete button. The
/// two buttons show on the row under the pointer or the selected one, and the struck eye always
/// on a hidden object's row. A right click opens the row's menu. Returns what was asked.
#[allow(clippy::too_many_lines)] // One row's drawing, its buttons, its menu and its drag.
pub fn objects(
    ui: &mut egui::Ui,
    workspace: &Workspace,
    (selected, checked): (Option<usize>, &BTreeSet<usize>),
    icons: Option<&crate::icons::Icons>,
) -> Option<Request> {
    let objects = &workspace.plate().objects;
    if objects.is_empty() {
        ui.label(egui::RichText::new(tr!("sidebar-no-objects")).color(theme::ACCENT.color()));
        return None;
    }
    let mut request = None;
    let side = ui.spacing().interact_size.y;
    let font = egui::TextStyle::Body.resolve(ui.style());
    for (index, object) in objects.iter().enumerate() {
        let lit = selected == Some(index);
        let hidden = workspace.is_hidden(index);
        let (rect, row) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), ROW_HEIGHT),
            egui::Sense::click_and_drag(),
        );
        row.dnd_set_drag_payload(DraggedModel(index));
        let hovered = ui.rect_contains_pointer(rect);
        let visuals = ui.visuals();
        let (fill, colour) = if lit {
            (visuals.selection.bg_fill, theme::TEXT_PRIMARY.color())
        } else if hovered {
            (visuals.widgets.hovered.weak_bg_fill, visuals.text_color())
        } else {
            (egui::Color32::TRANSPARENT, visuals.text_color())
        };
        let colour = if hidden {
            visuals.weak_text_color()
        } else {
            colour
        };
        ui.painter().rect_filled(rect, 4.0, fill);
        // The name keeps clear of the check box and the buttons' places, shown or not, so it
        // never moves.
        let room = rect.width() - 2.0 * ROW_INSET - CHECK_WIDTH - 2.0 * side - 4.0;
        let painter = ui.painter();
        let width = |text: &str| {
            painter
                .layout_no_wrap(text.to_owned(), font.clone(), colour)
                .size()
                .x
        };
        let name = shorten_keeping_extension(&object.name, |text| width(text) <= room);
        painter.text(
            egui::pos2(rect.left() + ROW_INSET + CHECK_WIDTH, rect.center().y),
            egui::Align2::LEFT_CENTER,
            &name,
            font.clone(),
            colour,
        );
        let row = match (name == object.name, hidden) {
            (true, false) => row,
            (false, false) => row.on_hover_text(&object.name),
            (_, true) => row.on_hover_text(tr!("sidebar-hidden", name = &object.name)),
        };
        row.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, lit, &object.name)
        });
        // A hidden object cannot be selected: it is shown first, with its eye.
        if row.clicked() && !hidden {
            request = Some(Request::Select(index));
        }
        row.context_menu(|ui| {
            if let Some(chosen) = row_menu(ui, workspace, index, hidden) {
                request = Some(chosen);
                ui.close();
            }
        });
        // The check box, over the row: a click on it checks, it does not select.
        let mut on = checked.contains(&index);
        let check_rect = egui::Rect::from_min_size(
            egui::pos2(rect.left() + ROW_INSET - 4.0, rect.top()),
            egui::vec2(CHECK_WIDTH, ROW_HEIGHT),
        );
        let check = ui
            .scope_builder(
                egui::UiBuilder::new()
                    .max_rect(check_rect)
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
                |ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    ui.checkbox(&mut on, "")
                },
            )
            .inner
            .on_hover_text(tr!("sidebar-check-object", name = &object.name));
        crate::widgets::name(&check, &tr!("sidebar-check-object", name = &object.name));
        if check.changed() {
            request = Some(Request::Check(index, on));
        }
        let place = |k: f32| {
            egui::Rect::from_center_size(
                egui::pos2(
                    rect.right() - side / 2.0 - 2.0 - k * (side + 2.0),
                    rect.center().y,
                ),
                egui::Vec2::splat(side),
            )
        };
        let button = |ui: &mut egui::Ui, k: f32, icon: Icon, hint: String| {
            ui.scope_builder(egui::UiBuilder::new().max_rect(place(k)), |ui| {
                crate::widgets::icon_button(ui, icons, icon, false, &hint)
            })
            .inner
            .clicked()
        };
        if hovered || lit {
            let hint = tr!("sidebar-delete-object", name = &object.name);
            if button(ui, 0.0, Icon::Delete, hint) {
                request = Some(Request::Delete(index));
            }
        }
        if hovered || lit || hidden {
            let (icon, hint) = if hidden {
                (
                    Icon::EyeOff,
                    tr!("sidebar-show-object", name = &object.name),
                )
            } else {
                (Icon::Eye, tr!("sidebar-hide-object", name = &object.name))
            };
            if button(ui, 1.0, icon, hint) {
                request = Some(Request::ToggleHidden(index));
            }
        }
    }
    request
}

/// The bar over the list (owner, 2026-11-10): a check box that checks every model, or clears
/// them (mixed while only some are), how many are checked, and the checked models' eye and
/// bin. Returns what was asked.
pub fn checked_bar(
    ui: &mut egui::Ui,
    workspace: &Workspace,
    checked: &BTreeSet<usize>,
    icons: Option<&crate::icons::Icons>,
) -> Option<Request> {
    let count = workspace.plate().objects.len();
    if count == 0 {
        return None;
    }
    let mut request = None;
    let some = checked.len();
    ui.horizontal(|ui| {
        ui.add_space(ROW_INSET - 4.0);
        let mut all = some == count;
        let response = ui
            .add(egui::Checkbox::new(&mut all, "").indeterminate(some > 0 && some < count))
            .on_hover_text(tr!("sidebar-check-all-hint"));
        crate::widgets::name(&response, &tr!("sidebar-check-all"));
        if response.changed() {
            // Mixed or none: check them all; all: clear them.
            request = Some(Request::CheckAll(some < count));
        }
        let text = if some == 0 {
            tr!("sidebar-checked-none")
        } else {
            tr!("sidebar-checked", count = some)
        };
        ui.label(egui::RichText::new(text).color(ui.visuals().weak_text_color()));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(2.0);
            ui.add_enabled_ui(some > 0, |ui| {
                if crate::widgets::icon_button(
                    ui,
                    icons,
                    Icon::Delete,
                    false,
                    &tr!("sidebar-delete-checked", count = some),
                )
                .clicked()
                {
                    request = Some(Request::DeleteChecked);
                }
                // Hide while any checked one shows; show them when all are hidden.
                let showing = checked.iter().any(|&k| !workspace.is_hidden(k));
                let (icon, hint) = if showing || some == 0 {
                    (Icon::Eye, tr!("sidebar-hide-checked", count = some))
                } else {
                    (Icon::EyeOff, tr!("sidebar-show-checked", count = some))
                };
                if crate::widgets::icon_button(ui, icons, icon, false, &hint).clicked() {
                    request = Some(Request::HideChecked(showing));
                }
            });
        });
    });
    request
}

/// A row's menu: hide or show, isolate or end the isolation, show all, clone, move or copy to a
/// plate, export, delete. The viewport opens it too, on a model right-clicked (owner, 2026-11-10).
pub fn row_menu(
    ui: &mut egui::Ui,
    workspace: &Workspace,
    index: usize,
    hidden: bool,
) -> Option<Request> {
    let mut chosen = None;
    let show_hide = if hidden {
        tr!("action-show")
    } else {
        tr!("action-hide")
    };
    if ui.button(show_hide).clicked() {
        chosen = Some(Request::ToggleHidden(index));
    }
    let isolate = if workspace.isolated() == Some(index) {
        tr!("action-end-isolation")
    } else {
        tr!("action-isolate")
    };
    if ui.button(isolate).clicked() {
        chosen = Some(Request::Isolate(index));
    }
    if ui
        .add_enabled(
            workspace.hidden_count() > 0,
            egui::Button::new(tr!("action-show-all")),
        )
        .clicked()
    {
        chosen = Some(Request::ShowAll);
    }
    ui.separator();
    if ui.button(tr!("sidebar-clone-model")).clicked() {
        chosen = Some(Request::Clone(index));
    }
    for (label, copy) in [
        (tr!("sidebar-move-to-plate"), false),
        (tr!("sidebar-copy-to-plate"), true),
    ] {
        ui.menu_button(label, |ui| {
            if let Some(to) = plate_menu(ui, workspace) {
                chosen = Some(Request::Transfer { index, to, copy });
            }
        });
    }
    if ui.button(tr!("sidebar-export-model")).clicked() {
        chosen = Some(Request::ExportModel(index));
    }
    ui.separator();
    if ui.button(tr!("action-delete")).clicked() {
        chosen = Some(Request::Delete(index));
    }
    chosen
}

/// The plates an object can go to: every plate but the one shown, numbered as the overview
/// numbers them, then a new plate. Returns the chosen one, `Some(None)` for a new plate.
#[allow(clippy::option_option)] // No choice yet, or a plate that may be a new one.
fn plate_menu(ui: &mut egui::Ui, workspace: &Workspace) -> Option<Option<PlateId>> {
    let mut chosen = None;
    let shown = workspace.plate_id();
    for (index, plate) in workspace.plates().iter().enumerate() {
        if plate.id() == shown {
            continue;
        }
        let label = tr!(
            "plates-overview-name",
            number = index + 1,
            name = &plate.content().name
        );
        if ui.button(label).clicked() {
            chosen = Some(Some(plate.id()));
        }
    }
    if workspace.plates().len() > 1 {
        ui.separator();
    }
    if ui.button(tr!("action-new-plate")).clicked() {
        chosen = Some(None);
    }
    chosen
}

/// `name` as is when it `fits`, or else shortened in the middle, "…" standing for what is left
/// out, keeping the extension (a short one, after the last dot) whole: the longest such name
/// that fits, or the ellipsis and the extension alone.
pub fn shorten_keeping_extension(name: &str, fits: impl Fn(&str) -> bool) -> String {
    if fits(name) {
        return name.to_owned();
    }
    let (stem, extension) = match name.rfind('.') {
        Some(dot) if dot > 0 && name.len() - dot <= 6 => name.split_at(dot),
        _ => (name, ""),
    };
    let chars: Vec<char> = stem.chars().collect();
    let shortened = |kept: usize| {
        let mut text: String = chars[..kept].iter().collect();
        text.push('…');
        text.push_str(extension);
        text
    };
    // The longest kept start that fits: fitting only gets harder as more is kept.
    let (mut low, mut high) = (0, chars.len());
    while low < high {
        let middle = (low + high).div_ceil(2);
        if fits(&shortened(middle)) {
            low = middle;
        } else {
            high = middle - 1;
        }
    }
    shortened(low)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_name_is_shortened_in_the_middle_and_keeps_its_extension() {
        let fits = |n: usize| move |text: &str| text.chars().count() <= n;
        let name = "VillageHouse(3x3)_32mm_Roof_B_Front.stl";
        assert_eq!(shorten_keeping_extension(name, fits(100)), name);
        assert_eq!(
            shorten_keeping_extension(name, fits(20)),
            "VillageHouse(3x….stl"
        );
        // Too narrow even for a letter: the ellipsis and the extension.
        assert_eq!(shorten_keeping_extension(name, fits(3)), "….stl");
        // Without an extension, or a dot that does not start one, the end is cut.
        assert_eq!(shorten_keeping_extension("abcdefghij", fits(6)), "abcde…");
        assert_eq!(
            shorten_keeping_extension("part.version-twelve", fits(8)),
            "part.ve…"
        );
        assert_eq!(shorten_keeping_extension(".hidden-file", fits(5)), ".hid…");
    }

    #[test]
    fn the_list_shows_names_from_the_left_and_shortens_long_ones_to_the_row() {
        let folder =
            std::env::temp_dir().join(format!("karasu-sidebar-list-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let mut workspace = Workspace::new();
        let names = [
            "Base_01.stl",
            "VillageHouse(3x3)_32mm_Roof_B_Front_with_a_long_tail.stl",
        ];
        for name in names {
            let model = folder.join(name);
            std::fs::write(&model, crate::workspace::tests::box_stl([0.0; 3], [8.0; 3])).unwrap();
            workspace.open(model);
            crate::workspace::tests::wait(&mut workspace);
        }
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(260.0, 400.0),
            )),
            ..egui::RawInput::default()
        };
        let mut row = egui::Rect::NOTHING;
        let mut output = ctx.run_ui(input, |ui| {
            ui.set_width(260.0);
            row = ui.max_rect();
            objects(ui, &workspace, (Some(0), &BTreeSet::new()), None);
        });
        output.textures_delta.clear();
        let texts: Vec<(String, egui::Rect)> = output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Text(shape) => Some((
                    shape.galley.text().to_owned(),
                    shape.galley.rect.translate(shape.pos.to_vec2()),
                )),
                _ => None,
            })
            .collect();
        let (short, short_rect) = texts.iter().find(|(t, _)| t == names[0]).unwrap();
        assert_eq!(short, names[0]);
        let (long, long_rect) = texts.iter().find(|(t, _)| t.contains('…')).unwrap();
        assert!(
            long.ends_with("….stl") && long.starts_with("VillageHouse"),
            "{long}"
        );
        // From the left after the check box, together, and clear of the delete button's place.
        assert!((short_rect.left() - long_rect.left()).abs() < 0.5);
        assert!((short_rect.left() - row.left() - ROW_INSET - CHECK_WIDTH).abs() < 0.5);
        assert!(
            long_rect.right() <= row.right() - ROW_INSET - 18.0,
            "{long_rect:?}"
        );
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_click_on_the_check_box_checks_and_one_on_the_name_selects() {
        let folder =
            std::env::temp_dir().join(format!("karasu-sidebar-check-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let mut workspace = Workspace::new();
        let model = folder.join("cube.stl");
        std::fs::write(&model, crate::workspace::tests::box_stl([0.0; 3], [8.0; 3])).unwrap();
        workspace.open(model);
        crate::workspace::tests::wait(&mut workspace);
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(260.0, 400.0));
        let click = |at: egui::Pos2| -> Vec<Option<Request>> {
            let presses = [
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
            presses
                .into_iter()
                .map(|events| {
                    let input = egui::RawInput {
                        events,
                        screen_rect: Some(screen),
                        ..egui::RawInput::default()
                    };
                    let mut request = None;
                    let mut output = ctx.run_ui(input, |ui| {
                        ui.set_width(260.0);
                        request = objects(ui, &workspace, (None, &BTreeSet::new()), None);
                    });
                    output.textures_delta.clear();
                    request
                })
                .collect()
        };
        let top = 8.0 + ROW_HEIGHT / 2.0;
        let on_box = click(egui::pos2(8.0 + ROW_INSET + 6.0, top));
        assert_eq!(on_box.last(), Some(&Some(Request::Check(0, true))));
        let on_name = click(egui::pos2(8.0 + ROW_INSET + CHECK_WIDTH + 30.0, top));
        assert_eq!(on_name.last(), Some(&Some(Request::Select(0))));
        let _ = std::fs::remove_dir_all(&folder);
    }
}
