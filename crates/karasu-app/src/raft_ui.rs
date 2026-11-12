//! The Raft tool (TD-108, owner mockup 2026-10-29): the raft under the selected model's
//! supports, in the Supports step. The model's lift is the Automatic tool's (owner, 2026-11-04,
//! TD-128); the footer still says how high the model stands.
//!
//! The raft is the model's own value, shown while it is selected, each change one undoable step
//! (a value dragged in a field merges into one). The raft is one slab under every foot on the
//! plate, kept in the model's support document; a new one starts from the defaults set in
//! Settings › Supports. The tool has no modes, so no tool strip; what the raft lies under, and a
//! raft with nothing to lie under, are said in the footer.
use crate::{
    i18n::{decimal, tr},
    icons::{Icon, Icons},
    supports_ui::SupportsUi,
    theme,
    widgets::{choice, fact_row, state_line, switch, value_row, Unit, UnitField},
    workspace::Workspace,
};
use eframe::egui;
use karasu_core::supports::{
    BaseSpec, Preset, RaftBars, RaftLayout, RaftShape, RaftSpec, SupportDocument,
    DEFAULT_FILLET_DIAMETER_MM, DEFAULT_FILLET_HEIGHT_MM,
};

/// The raft a new one starts from when the settings hold none (owner's mockup).
pub const DEFAULT_RAFT: RaftSpec = RaftSpec {
    // 0.5 mm thick (owner, 2026-11-10; 1 mm before).
    thickness_mm: 0.5,
    margin_mm: 1.0,
    slope_deg: 60.0,
    fillet_diameter_mm: DEFAULT_FILLET_DIAMETER_MM,
    fillet_height_mm: DEFAULT_FILLET_HEIGHT_MM,
    layout: RaftLayout::Single,
    shape: RaftShape::Slab,
    bars: RaftBars::DEFAULT,
};
/// The side slope a raft takes when Skate is chosen, in degrees (owner, 2026-11-10, TD-144).
pub const SKATE_SLOPE_DEG: f64 = 45.0;
/// The raft shapes in the Type menu's order.
const SHAPES: [RaftShape; 3] = [RaftShape::Slab, RaftShape::Skate, RaftShape::Web];

/// Give `raft` `shape`: a skate starts with its sides at [`SKATE_SLOPE_DEG`] (owner,
/// 2026-11-10), which stay editable; a slab keeps the slope it has.
fn choose_shape(raft: &mut RaftSpec, shape: RaftShape) {
    if shape == RaftShape::Skate {
        raft.slope_deg = SKATE_SLOPE_DEG;
    }
    raft.shape = shape;
}

/// A raft shape's name in the Type menu.
fn shape_label(shape: RaftShape) -> String {
    match shape {
        RaftShape::Slab => tr!("raft-type-slab"),
        RaftShape::Skate => tr!("raft-type-skate"),
        RaftShape::Web => tr!("raft-type-web"),
    }
}
/// The distance under which feet share a raft when groups are first chosen, in mm.
pub const DEFAULT_JOIN_MM: f64 = 10.0;
/// The raft's thickness the document accepts, in mm.
pub const THICKNESS_MM: std::ops::RangeInclusive<f64> = 0.2..=5.0;
/// The margin beyond the outermost pads the document accepts, in mm.
pub const MARGIN_MM: std::ops::RangeInclusive<f64> = 0.0..=20.0;
/// The sides' slope from the plate the document accepts, in degrees.
pub const SLOPE_DEG: std::ops::RangeInclusive<f64> = 30.0..=90.0;
/// The fillet's diameter at the raft the document accepts, in mm.
pub const FILLET_DIAMETER_MM: std::ops::RangeInclusive<f64> = 0.2..=10.0;
/// The fillet's height above the raft the document accepts, in mm.
pub const FILLET_HEIGHT_MM: std::ops::RangeInclusive<f64> = 0.0..=5.0;
/// The distance under which feet share a raft the document accepts, in mm.
pub const JOIN_MM: std::ops::RangeInclusive<f64> = 0.5..=200.0;
/// A web's bar width the document accepts, in mm.
pub const BAR_WIDTH_MM: std::ops::RangeInclusive<f64> = 0.2..=10.0;
/// A web's bar height the document accepts, in mm.
pub const BAR_HEIGHT_MM: std::ops::RangeInclusive<f64> = 0.2..=5.0;
/// A web's longest bar the document accepts, in mm.
pub const BAR_LONGEST_MM: std::ops::RangeInclusive<f64> = 1.0..=200.0;
/// Merge keys of the tool's edits: the raft's, with the model's index.
const MERGE_KEYS: u64 = 0x5241_4654_0000_0000; // "RAFT"

/// `raft` if the document would accept it, else the built-in one.
#[must_use]
pub fn valid_raft(raft: RaftSpec) -> RaftSpec {
    let join = match raft.layout {
        RaftLayout::Single => true,
        RaftLayout::Groups { join_mm } => JOIN_MM.contains(&join_mm),
    };
    if THICKNESS_MM.contains(&raft.thickness_mm)
        && MARGIN_MM.contains(&raft.margin_mm)
        && SLOPE_DEG.contains(&raft.slope_deg)
        && FILLET_DIAMETER_MM.contains(&raft.fillet_diameter_mm)
        && FILLET_HEIGHT_MM.contains(&raft.fillet_height_mm)
        && BAR_WIDTH_MM.contains(&raft.bars.width_mm)
        && BAR_HEIGHT_MM.contains(&raft.bars.height_mm)
        && BAR_LONGEST_MM.contains(&raft.bars.longest_mm)
        && join
    {
        raft
    } else {
        DEFAULT_RAFT
    }
}

/// The Raft tool's state.
#[derive(Debug, Default)]
pub struct RaftUi {
    /// The panel's Advanced switch: the raft's margin and side slope show.
    pub advanced: bool,
}

/// A raft's values in `ui`: its type, its base (one raft, or one per group of feet, with the
/// distance that joins them), its thickness, and with `advanced` its margin, side slope and
/// fillet. Shared by the panel and the settings' defaults. Returns whether any changed.
pub fn raft_fields(ui: &mut egui::Ui, raft: &mut RaftSpec, advanced: bool) -> bool {
    let mut edited = false;
    let mut shape = raft.shape;
    value_row(ui, tr!("raft-type"), |ui| {
        egui::ComboBox::from_id_salt(ui.id().with("raft-type"))
            .width(ui.available_width())
            .selected_text(shape_label(shape))
            .show_ui(ui, |ui| {
                for choice in SHAPES {
                    ui.selectable_value(&mut shape, choice, shape_label(choice));
                }
            })
            .response
            .on_hover_text(tr!("raft-type-hint"))
    });
    if shape != raft.shape {
        choose_shape(raft, shape);
        edited = true;
    }
    if raft.shape == RaftShape::Web {
        edited |= bar_fields(ui, &mut raft.bars, advanced);
    } else {
        edited |= base_fields(ui, &mut raft.layout);
    }
    edited |= value_row(ui, tr!("raft-thickness"), |ui| {
        ui.add(
            UnitField::new(&mut raft.thickness_mm, Unit::Millimetre)
                .range(THICKNESS_MM)
                .step(0.05),
        )
        .on_hover_text(tr!("raft-thickness-hint"))
    })
    .changed();
    if !advanced {
        return edited;
    }
    edited |= advanced_fields(ui, raft);
    edited
}

/// A web's bars in `ui`: the longest, and with `advanced` their width and height (TD-144).
/// Returns whether any changed.
fn bar_fields(ui: &mut egui::Ui, bars: &mut RaftBars, advanced: bool) -> bool {
    let mut edited = value_row(ui, tr!("raft-bar-longest"), |ui| {
        ui.add(
            UnitField::new(&mut bars.longest_mm, Unit::Millimetre)
                .range(BAR_LONGEST_MM)
                .step(0.5),
        )
        .on_hover_text(tr!("raft-bar-longest-hint"))
    })
    .changed();
    if advanced {
        edited |= value_row(ui, tr!("raft-bar-width"), |ui| {
            ui.add(
                UnitField::new(&mut bars.width_mm, Unit::Millimetre)
                    .range(BAR_WIDTH_MM)
                    .step(0.05),
            )
            .on_hover_text(tr!("raft-bar-width-hint"))
        })
        .changed();
        edited |= value_row(ui, tr!("raft-bar-height"), |ui| {
            ui.add(
                UnitField::new(&mut bars.height_mm, Unit::Millimetre)
                    .range(BAR_HEIGHT_MM)
                    .step(0.05),
            )
            .on_hover_text(tr!("raft-bar-height-hint"))
        })
        .changed();
    }
    edited
}

/// A slab's or a skate's base in `ui`: one raft, or one per group of feet with the distance
/// that joins them. Returns whether it changed.
fn base_fields(ui: &mut egui::Ui, layout: &mut RaftLayout) -> bool {
    let mut edited = false;
    ui.label(tr!("raft-base"));
    let mut grouped = usize::from(matches!(layout, RaftLayout::Groups { .. }));
    let labels = [tr!("raft-base-single"), tr!("raft-base-groups")];
    if choice(ui, &mut grouped, &labels)
        .on_hover_text(tr!("raft-base-hint"))
        .changed()
    {
        *layout = if grouped == 1 {
            RaftLayout::Groups {
                join_mm: DEFAULT_JOIN_MM,
            }
        } else {
            RaftLayout::Single
        };
        edited = true;
    }
    if let RaftLayout::Groups { join_mm } = layout {
        edited |= value_row(ui, tr!("raft-join"), |ui| {
            ui.add(
                UnitField::new(join_mm, Unit::Millimetre)
                    .range(JOIN_MM)
                    .step(0.5),
            )
            .on_hover_text(tr!("raft-join-hint"))
        })
        .changed();
    }
    edited
}

/// The raft's Advanced values in `ui`: its margin, side slope and fillet. Returns whether any
/// changed.
fn advanced_fields(ui: &mut egui::Ui, raft: &mut RaftSpec) -> bool {
    let mut edited = value_row(ui, tr!("raft-margin"), |ui| {
        ui.add(
            UnitField::new(&mut raft.margin_mm, Unit::Millimetre)
                .range(MARGIN_MM)
                .step(0.1),
        )
        .on_hover_text(tr!("raft-margin-hint"))
    })
    .changed();
    edited |= value_row(ui, tr!("raft-slope"), |ui| {
        ui.add(
            UnitField::new(&mut raft.slope_deg, Unit::Degree)
                .range(SLOPE_DEG)
                .step(1.0)
                .decimals(0),
        )
        .on_hover_text(tr!("raft-slope-hint"))
    })
    .changed();
    edited |= value_row(ui, tr!("raft-fillet-diameter"), |ui| {
        ui.add(
            UnitField::new(&mut raft.fillet_diameter_mm, Unit::Millimetre)
                .range(FILLET_DIAMETER_MM)
                .step(0.05),
        )
        .on_hover_text(tr!("raft-fillet-diameter-hint"))
    })
    .changed();
    edited |= value_row(ui, tr!("raft-fillet-height"), |ui| {
        ui.add(
            UnitField::new(&mut raft.fillet_height_mm, Unit::Millimetre)
                .range(FILLET_HEIGHT_MM)
                .step(0.05),
        )
        .on_hover_text(tr!("raft-fillet-height-hint"))
    })
    .changed();
    edited
}

/// Give model `index` `raft`, or none, in its support document (made if it has none): one
/// undoable step, or merged with the next changes of its values when `merging`.
fn set_raft(workspace: &mut Workspace, index: usize, raft: Option<RaftSpec>, merging: bool) {
    let mut document = workspace
        .supports(index)
        .cloned()
        .unwrap_or_else(|| SupportDocument::new(Preset::Medium));
    document.raft = raft;
    let key = merging.then_some(MERGE_KEYS | 1 << 31 | index as u64);
    workspace.set_supports(index, Some(document), key);
}

/// How many of `document`'s supports stand on the plate (or would stand on the raft).
fn on_the_plate(document: Option<&SupportDocument>) -> usize {
    document.map_or(0, |d| {
        d.supports
            .iter()
            .filter(|s| matches!(s.base, BaseSpec::Platform(_) | BaseSpec::Vertical))
            .count()
    })
}

impl RaftUi {
    /// The panel's body: the raft of model `index`; `defaults` is what a new raft is made with.
    pub fn panel(
        &mut self,
        ui: &mut egui::Ui,
        workspace: &mut Workspace,
        supports: &SupportsUi,
        index: Option<usize>,
        defaults: RaftSpec,
    ) {
        let index = index.filter(|&i| i < workspace.plate().objects.len());
        ui.add_enabled_ui(index.is_some(), |ui| {
            crate::widgets::group(ui, tr!("tool-raft"), |ui| {
                self.raft(ui, workspace, supports, index, defaults);
            });
        });
    }

    /// The raft's switch, its values and what it lies under.
    fn raft(
        &mut self,
        ui: &mut egui::Ui,
        workspace: &mut Workspace,
        supports: &SupportsUi,
        index: Option<usize>,
        defaults: RaftSpec,
    ) {
        let document = index.and_then(|i| workspace.supports(i));
        let current = document.and_then(|d| d.raft);
        let mut on = current.is_some();
        let mut raft = current.unwrap_or(defaults);
        let switched = ui
            .horizontal(|ui| {
                let response = switch(ui, &mut on).on_hover_text(tr!("raft-on-hint"));
                crate::widgets::name(&response, &tr!("raft-on"));
                ui.add(egui::Label::new(tr!("raft-on")).wrap());
                response.changed()
            })
            .inner;
        let plate = on_the_plate(document);
        let built = index.and_then(|i| supports.raft_built(i));
        let mut edited = false;
        ui.add_enabled_ui(on, |ui| {
            edited |= raft_fields(ui, &mut raft, self.advanced);
            let under = if on {
                tr!("raft-under-count", count = plate)
            } else {
                "–".to_owned()
            };
            let count = match built.filter(|_| on) {
                Some((count, _)) => count.to_string(),
                None => "–".to_owned(),
            };
            fact_row(ui, &tr!("raft-count"), &count);
            fact_row(ui, &tr!("raft-under"), &under);
            let size = match built.filter(|_| on) {
                Some((_, [w, d])) => tr!(
                    "raft-footprint-size",
                    width = decimal(w, 1),
                    depth = decimal(d, 1)
                ),
                None => "–".to_owned(),
            };
            fact_row(ui, &tr!("raft-footprint"), &size);
        });
        let Some(index) = index else {
            return;
        };
        if switched || (edited && on) {
            // Turning it on or off is a step of its own; a value dragged merges into one.
            set_raft(workspace, index, on.then_some(raft), !switched);
        }
    }

    /// The panel's footer: the selected model's base in a few words.
    pub fn footer(
        ui: &mut egui::Ui,
        workspace: &Workspace,
        index: Option<usize>,
        icons: Option<&Icons>,
    ) {
        let (text, colour, icon) = state(workspace, index);
        state_line(ui, icons, icon, &text, colour);
    }
}

/// What the footer says: no model, a raft waiting for a support on the plate, a raft under the
/// supports, or none.
fn state(workspace: &Workspace, index: Option<usize>) -> (String, egui::Color32, Option<Icon>) {
    let Some(index) = index.filter(|&i| i < workspace.plate().objects.len()) else {
        return ("–".to_owned(), theme::BORDER.color(), None);
    };
    let document = workspace.supports(index);
    let plate = on_the_plate(document);
    let lift = decimal(crate::auto_ui::lift_of(workspace, index), 2);
    match document.and_then(|d| d.raft) {
        // A skate with nothing left at the plate under its narrowest foot: its build fails.
        Some(raft) if !raft.stands_on_the_plate() => (
            tr!("raft-state-no-base"),
            theme::ERROR.color(),
            Some(Icon::Error),
        ),
        Some(_) if plate == 0 => (
            tr!("raft-state-waiting"),
            theme::WARNING.color(),
            Some(Icon::Warning),
        ),
        Some(_) => (
            tr!("raft-state-on", count = plate, lift = lift),
            theme::SUCCESS.color(),
            Some(Icon::Success),
        ),
        None => (
            tr!("raft-state-off", lift = lift),
            theme::BORDER.color(),
            None,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_raft_out_of_the_documents_ranges_is_the_built_in_one() {
        assert_eq!(valid_raft(DEFAULT_RAFT), DEFAULT_RAFT);
        let thick = RaftSpec {
            thickness_mm: 2.0,
            ..DEFAULT_RAFT
        };
        assert_eq!(valid_raft(thick), thick);
        for wrong in [
            RaftSpec {
                thickness_mm: 0.1,
                ..DEFAULT_RAFT
            },
            RaftSpec {
                margin_mm: 25.0,
                ..DEFAULT_RAFT
            },
            RaftSpec {
                slope_deg: f64::NAN,
                ..DEFAULT_RAFT
            },
        ] {
            assert_eq!(valid_raft(wrong), DEFAULT_RAFT);
        }
        // Skate takes 45° sides; back to Slab, the slope stays.
        let mut raft = DEFAULT_RAFT;
        choose_shape(&mut raft, RaftShape::Skate);
        assert_eq!(
            (raft.shape, raft.slope_deg),
            (RaftShape::Skate, SKATE_SLOPE_DEG)
        );
        assert!(raft.stands_on_the_plate());
        choose_shape(&mut raft, RaftShape::Slab);
        assert_eq!(
            (raft.shape, raft.slope_deg),
            (RaftShape::Slab, SKATE_SLOPE_DEG)
        );
        assert_eq!(
            SHAPES.map(shape_label),
            [
                tr!("raft-type-slab"),
                tr!("raft-type-skate"),
                tr!("raft-type-web")
            ]
        );
        // A web keeps the slope it has.
        choose_shape(&mut raft, RaftShape::Web);
        assert_eq!(
            (raft.shape, raft.slope_deg),
            (RaftShape::Web, SKATE_SLOPE_DEG)
        );
        // Bars out of the document's ranges: the built-in raft.
        let wide = RaftSpec {
            bars: RaftBars {
                width_mm: 12.0,
                ..RaftBars::DEFAULT
            },
            ..DEFAULT_RAFT
        };
        assert_eq!(valid_raft(wide), DEFAULT_RAFT);
        // The built-in raft is one the document accepts.
        let mut document = SupportDocument::new(Preset::Medium);
        document.raft = Some(DEFAULT_RAFT);
        assert!(document.validated().is_ok());
    }

    #[test]
    fn a_model_is_lifted_and_its_raft_lies_under_the_supports_on_the_plate() {
        let (mut workspace, mut preparation, directory) = crate::supports_ui::tests::lifted_cube();
        crate::auto_ui::set_lift(&mut workspace, 0, 6.0);
        // Prepared again where it now stands, as the interface does.
        crate::hollowing_ui::tests::prepared_within(
            &workspace,
            &mut preparation,
            std::time::Duration::from_secs(60),
        );
        assert!((crate::auto_ui::lift_of(&workspace, 0) - 6.0).abs() < 1e-9);
        let low = workspace.placement_of(0).unwrap().min.coordinates()[2];
        assert!((low - 6.0).abs() < 1e-6, "{low}");
        // No raft yet: the footer says how high the model stands.
        let (text, _, _) = state(&workspace, Some(0));
        assert_eq!(text, tr!("raft-state-off", lift = decimal(6.0, 2)));
        // A raft with no support on the plate waits for one.
        set_raft(&mut workspace, 0, Some(DEFAULT_RAFT), false);
        let (text, _, icon) = state(&workspace, Some(0));
        assert_eq!(text, tr!("raft-state-waiting"));
        assert_eq!(icon, Some(Icon::Warning));
        // With a support on the plate it lies under it, built with the supports.
        let mut ui = SupportsUi::default();
        let placed = workspace.placement_of(0).unwrap();
        let [cx, cy] =
            [0, 1].map(|k| f64::midpoint(placed.min.coordinates()[k], placed.max.coordinates()[k]));
        let under = crate::supports_ui::tests::ray(cx, cy, true);
        let hit = workspace.hit(&under);
        ui.toggle_placing();
        assert!(ui.click(&mut workspace, &under, hit, 0));
        crate::supports_ui::tests::built(&mut ui, &workspace, &preparation);
        assert_eq!(workspace.supports(0).unwrap().raft, Some(DEFAULT_RAFT));
        let (count, [w, d]) = ui.raft_built(0).expect("a raft built");
        assert_eq!(count, 1);
        // The pad, its margin each side and the slope's run at the plate.
        assert!(
            w > 2.0 * DEFAULT_RAFT.margin_mm && d > 2.0 * DEFAULT_RAFT.margin_mm,
            "{w} {d}"
        );
        let (text, _, _) = state(&workspace, Some(0));
        assert_eq!(
            text,
            tr!("raft-state-on", count = 1_usize, lift = decimal(6.0, 2))
        );
        // A second support 10 mm away: one raft for both, or one each when grouped closer.
        let far = crate::supports_ui::tests::ray(cx + 7.0, cy + 7.0, true);
        let hit = workspace.hit(&far);
        assert!(ui.click(&mut workspace, &far, hit, 0));
        crate::supports_ui::tests::built(&mut ui, &workspace, &preparation);
        assert_eq!(ui.raft_built(0).map(|(n, _)| n), Some(1));
        let grouped = RaftSpec {
            layout: RaftLayout::Groups { join_mm: 5.0 },
            ..DEFAULT_RAFT
        };
        set_raft(&mut workspace, 0, Some(grouped), false);
        crate::supports_ui::tests::built(&mut ui, &workspace, &preparation);
        assert_eq!(ui.raft_built(0).map(|(n, _)| n), Some(2));
        // A skate (TD-144): built narrower at the plate than at its top; too thick for its
        // margin and slope, the footer says it has nothing left at the plate.
        let skate = RaftSpec {
            shape: RaftShape::Skate,
            slope_deg: SKATE_SLOPE_DEG,
            ..DEFAULT_RAFT
        };
        set_raft(&mut workspace, 0, Some(skate), false);
        crate::supports_ui::tests::built(&mut ui, &workspace, &preparation);
        assert_eq!(ui.raft_built(0).map(|(n, _)| n), Some(1));
        let (_, _, icon) = state(&workspace, Some(0));
        assert_eq!(icon, Some(Icon::Success));
        // A web (TD-144): the two feet, 10 mm apart, joined by a bar into one piece; with the
        // longest bar shorter than that, two.
        let web = RaftSpec {
            shape: RaftShape::Web,
            ..DEFAULT_RAFT
        };
        set_raft(&mut workspace, 0, Some(web), false);
        crate::supports_ui::tests::built(&mut ui, &workspace, &preparation);
        assert_eq!(ui.raft_built(0).map(|(n, _)| n), Some(1));
        let apart = RaftSpec {
            bars: RaftBars {
                longest_mm: 5.0,
                ..RaftBars::DEFAULT
            },
            ..web
        };
        set_raft(&mut workspace, 0, Some(apart), false);
        crate::supports_ui::tests::built(&mut ui, &workspace, &preparation);
        assert_eq!(ui.raft_built(0).map(|(n, _)| n), Some(2));
        let thick = RaftSpec {
            thickness_mm: 3.0,
            ..skate
        };
        set_raft(&mut workspace, 0, Some(thick), false);
        let (text, _, icon) = state(&workspace, Some(0));
        assert_eq!(text, tr!("raft-state-no-base"));
        assert_eq!(icon, Some(Icon::Error));
        set_raft(&mut workspace, 0, Some(DEFAULT_RAFT), false);
        // Off again: one step undoes it.
        set_raft(&mut workspace, 0, None, false);
        assert_eq!(workspace.supports(0).unwrap().raft, None);
        workspace.undo();
        assert_eq!(workspace.supports(0).unwrap().raft, Some(DEFAULT_RAFT));
        let _ = std::fs::remove_dir_all(directory);
    }
}
