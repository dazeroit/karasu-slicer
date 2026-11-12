//! Interface colours beyond the brand palette (owner decision, 2026-10-20; TD-074).
//!
//! The approved palette ([`crate::theme`]) is the brand identity. The interface also uses other
//! colours where convention matters, such as the X, Y and Z axes in red, green and blue, and a
//! warm default for models. Every colour here is meant to be set by the user in the settings;
//! the settings file keeps them (TD-097), and the defaults below apply until the user changes
//! them.
use crate::viewport::{Filter, Pattern};
use eframe::egui::Color32;

/// The scene's default brightness, in percent.
pub const DEFAULT_BRIGHTNESS_PERCENT: u16 = 100;
/// The light a face seen edge-on keeps by default, in percent (the shading's former fixed 35).
pub const DEFAULT_AMBIENT_PERCENT: u16 = 35;
/// The brightness the settings offer, in percent.
pub const BRIGHTNESS_RANGE: std::ops::RangeInclusive<u16> = 50..=150;
/// The edge-on light the settings offer, in percent.
pub const AMBIENT_RANGE: std::ops::RangeInclusive<u16> = 0..=80;

impl Appearance {
    /// The scene's lighting from the brightness and ambient settings, kept in their ranges.
    #[must_use]
    pub fn shading(&self) -> crate::viewport::Shading {
        let clamp = |value: u16, range: &std::ops::RangeInclusive<u16>| {
            f32::from(value.clamp(*range.start(), *range.end())) / 100.0
        };
        crate::viewport::Shading {
            brightness: clamp(self.brightness_percent, &BRIGHTNESS_RANGE),
            ambient: clamp(self.ambient_percent, &AMBIENT_RANGE),
        }
    }
}

/// An sRGB colour with alpha (TD-076): red, green, blue, then alpha (255 is opaque), not
/// premultiplied.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Rgba(pub [u8; 4]);

impl Rgba {
    /// An opaque colour.
    #[must_use]
    pub const fn opaque([r, g, b]: [u8; 3]) -> Self {
        Self([r, g, b, 0xFF])
    }

    /// The colour for egui.
    #[must_use]
    pub fn color(self) -> Color32 {
        let [r, g, b, a] = self.0;
        Color32::from_rgba_unmultiplied(r, g, b, a)
    }

    /// Linear RGB and alpha, as the viewport shaders light and blend them.
    #[must_use]
    pub fn linear(self) -> [f32; 4] {
        let [r, g, b, a] = self.0.map(|c| f32::from(c) / 255.0);
        let [r, g, b] = [r, g, b].map(|c| {
            if c <= 0.040_45 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        });
        [r, g, b, a]
    }

    /// The sRGB-encoded channels from 0 to 1, as the viewport's target stores them.
    #[must_use]
    pub fn encoded(self) -> [f64; 4] {
        self.0.map(|c| f64::from(c) / 255.0)
    }
}

/// The style of a surface filter (M6.4f): its colour, whose alpha is its opacity, and its
/// pattern.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FilterStyle {
    pub color: Rgba,
    pub pattern: Pattern,
}

impl FilterStyle {
    /// The filter as the viewport draws it.
    #[must_use]
    pub fn filter(self) -> Filter {
        Filter {
            color: self.color.linear(),
            pattern: self.pattern,
        }
    }
}

/// The hollowing blockers' default colours (TD-115).
pub const BLOCKER: [u8; 4] = [0x18, 0xB5, 0xA5, 0x99];
pub const BLOCKER_NEW: [u8; 4] = [0x8E, 0x52, 0xFF, 0xA6];
pub const BLOCKER_SELECTED: [u8; 4] = [0xFF, 0x4F, 0xD8, 0xB3];

/// The colours of the 3D viewport and its handles.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
// Colours added later read from an earlier settings file at their defaults.
#[serde(default)]
pub struct Appearance {
    /// Models.
    pub object: Rgba,
    /// The selected model.
    pub selected: Rgba,
    /// The outline around the selected model's silhouette (M6.4f).
    pub selection_outline: Rgba,
    /// Whether the selected model is outlined. Not a colour, but set with them in the settings.
    pub show_selection_outline: bool,
    /// The filter over the parts of a model outside the build volume (M6.4f).
    pub outside_filter: FilterStyle,
    /// The same filter on the selected model.
    pub outside_filter_selected: FilterStyle,
    /// The viewport background.
    pub background: Rgba,
    /// The plate's surface.
    pub plate: Rgba,
    /// The plate grid, every 10 mm.
    pub grid: Rgba,
    /// The plate grid's major lines, every 50 mm.
    pub grid_major: Rgba,
    /// The plate's outline and the build volume's edges.
    pub outline: Rgba,
    /// The border around the plate, which stays when the view goes below it.
    pub plate_border: Rgba,
    /// The tab outside the plate's front edge.
    pub front_tab: Rgba,
    /// The word on the front tab.
    pub front_label: Rgba,
    /// The X, Y and Z axes of the handles.
    pub axes: [Rgba; 3],
    /// The handles at rest that are not an axis: the supports' and drain holes' handles and the
    /// uniform scale handle (owner, 2026-11-10: one colour, shown in the Handles group).
    pub uniform: Rgba,
    /// The free-move square.
    pub free_move: Rgba,
    /// The free-move square under the pointer.
    pub free_move_hovered: Rgba,
    /// Any other handle under the pointer.
    pub hovered: Rgba,
    /// Manual supports.
    pub support: Rgba,
    /// Supports with an error finding.
    pub support_error: Rgba,
    /// The selected support (owner, 2026-10-29).
    pub support_selected: Rgba,
    /// The selected support, when it has an error finding.
    pub support_error_selected: Rgba,
    /// The cavity of a hollowed model, seen in the section view.
    pub cavity: Rgba,
    /// The cavity of the selected model.
    pub cavity_selected: Rgba,
    /// The selected model's outside in the cavity view, a ghost: its opacity shows how much of
    /// the inside shows through (owner, 2026-10-29).
    pub model_ghost: Rgba,
    /// The section's caps: the material the section cuts through.
    pub section_cap: Rgba,
    /// The supports' section caps (owner, 2026-10-30).
    pub support_section_cap: Rgba,
    /// The contact's height line on the models while a support is placed or its tip dragged
    /// (owner, 2026-11-06, TD-137).
    pub contact_line: Rgba,
    /// Whether the contact's height line shows. Not a colour, but set with them.
    pub show_contact_line: bool,
    /// Supports, braces and drain holes the list window's With errors leaves out, as ghosts
    /// (owner, 2026-10-31): its opacity shows how much of them stays.
    pub filtered_ghost: Rgba,
    /// Drain holes.
    pub drain_hole: Rgba,
    /// The markers of suction cups with no opening, and of those whose openings are narrower
    /// than the minimum vent (TD-114).
    pub suction_cup: Rgba,
    pub suction_cup_narrow: Rgba,
    /// Hollowing blockers (owner, 2026-11-02): applied, new or changed, and the selected one;
    /// their opacity shows the model through them.
    pub blocker: Rgba,
    pub blocker_new: Rgba,
    pub blocker_selected: Rgba,
    /// The selected drain hole.
    pub hole_selected: Rgba,
    /// A drain hole about to be placed or moved, where it is certified.
    pub hole_valid: Rgba,
    /// A drain hole about to be placed or moved, before its check ends.
    pub hole_unchecked: Rgba,
    /// Repair markers (TD-092): a defect that makes slicing fail.
    pub marker_fails: Rgba,
    /// A defect that only blocks hollowing.
    pub marker_hollowing: Rgba,
    /// A defect slicing handles.
    pub marker_harmless: Rgba,
    /// A place a repair changed.
    pub marker_repaired: Rgba,
    /// Overhangs on the models (TD-107): the colour at the critical angle's limit, whose alpha
    /// is how much it covers the model's.
    pub overhang_limit: Rgba,
    /// The colour of a level surface facing the plate, the gradient's other end.
    pub overhang_level: Rgba,
    /// The Orient tool's protected points (TD-119).
    pub protected: Rgba,
    /// The interface's status colours (TD-142): warnings, errors and successes in text and
    /// icons, and the notifications' fills, in the order of [`crate::theme::STATUS_TOKENS`].
    pub status: [Rgba; crate::theme::STATUS_COUNT],
    /// The scene's brightness, in percent: 100 shows a face turned to the viewer in its exact
    /// colour.
    pub brightness_percent: u16,
    /// The light a face seen edge-on keeps, in percent of a face turned to the viewer: more
    /// lights the shadows, less shows the relief.
    pub ambient_percent: u16,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            // A warm grey for models, and a flesh tone for the selected one (owner's choice,
            // 2026-10-23), as sculpting applications show the model being worked on.
            object: Rgba::opaque([0xB8, 0xB0, 0xA8]),
            selected: Rgba::opaque([0xD9, 0xA0, 0x88]),
            // Light grey (owner's choice): it stands out on the dark background and the plate.
            selection_outline: Rgba::opaque([0xC8, 0xC8, 0xC8]),
            show_selection_outline: true,
            // Strong red on models, dark stripes on the selected one, which keep its own colour
            // readable (owner's example, Lychee Slicer).
            outside_filter: FilterStyle {
                color: Rgba([0xD4, 0x1E, 0x1E, 0xE0]),
                pattern: Pattern::Solid,
            },
            outside_filter_selected: FilterStyle {
                color: Rgba([0x15, 0x16, 0x1C, 0xC0]),
                pattern: Pattern::Stripes,
            },
            background: Rgba::opaque([0x0B, 0x0B, 0x12]),
            // 80 percent transparent (owner choice): models show through from below.
            plate: Rgba([0x1C, 0x1C, 0x22, 0x33]),
            grid: Rgba::opaque([0x4A, 0x46, 0x5C]),
            grid_major: Rgba::opaque([0x6B, 0x68, 0x76]),
            outline: Rgba::opaque([0x3A, 0x3A, 0x42]),
            plate_border: Rgba::opaque([0x6B, 0x68, 0x76]),
            // Lighter than the plate, so the front reads at a glance.
            front_tab: Rgba([0x4A, 0x46, 0x5C, 0xB3]),
            front_label: Rgba::opaque([0xD8, 0xD6, 0xE2]),
            axes: [
                Rgba::opaque([0xE5, 0x48, 0x4D]),
                Rgba::opaque([0x46, 0xC2, 0x5A]),
                Rgba::opaque([0x3E, 0x7B, 0xFA]),
            ],
            // Amber (owner, 2026-11-10): seen on light models, apart from the hover yellow.
            uniform: Rgba::opaque([0xE6, 0xA8, 0x17]),
            // Translucent yellow (owner's choice): the model shows through the square.
            free_move: Rgba([0xFF, 0xFF, 0x00, 0x32]),
            free_move_hovered: Rgba::opaque([0xFF, 0xE0, 0x66]),
            hovered: Rgba::opaque([0xFF, 0xD1, 0x4A]),
            support: Rgba::opaque([0xA9, 0xC8, 0xE8]),
            support_error: Rgba::opaque([0xE5, 0x5B, 0x5B]),
            // Brighter than the supports' own, so the selected one stands out (owner).
            support_selected: Rgba::opaque([0x2F, 0x8C, 0xFF]),
            support_error_selected: Rgba::opaque([0xFF, 0x2E, 0x2E]),
            cavity: Rgba::opaque([0x8D, 0x8A, 0x7E]),
            // A darker shade of the selected model's flesh tone.
            cavity_selected: Rgba::opaque([0x9C, 0x6B, 0x58]),
            // Faint and light: the outside's shape without hiding the cavity.
            model_ghost: Rgba([0xC8, 0xC4, 0xD4, 0x2E]),
            // Near white (owner's choice): cut material reads clearly against both model colours.
            section_cap: Rgba::opaque([0xF5, 0xF5, 0xF5]),
            // Blue (owner, 2026-10-30), deeper than the supports' own: cut supports read apart
            // from the near-white model caps and from the supports' surface.
            support_section_cap: Rgba::opaque([0x2E, 0x5B, 0xD9]),
            // A faint grey: where the others stand reads, and the wrong ones stand out.
            filtered_ghost: Rgba([0xB4, 0xBA, 0xC8, 0x30]),
            // Blue (owner choice): placed holes stand apart from the model and the preview.
            drain_hole: Rgba::opaque([0x3D, 0x8B, 0xF2]),
            // Red and amber, as the mockup: no opening at all, or openings too narrow.
            suction_cup: Rgba::opaque([0xFF, 0x6B, 0x6B]),
            suction_cup_narrow: Rgba::opaque([0xF2, 0xB3, 0x3D]),
            // Teal once applied, violet while a draft, magenta when selected (owner, 2026-11-02:
            // more visible than the mockup's grey and amber): cool, saturated hues stand apart
            // from both model colours, and their opacity still shows the model through them.
            blocker: Rgba(BLOCKER),
            blocker_new: Rgba(BLOCKER_NEW),
            blocker_selected: Rgba(BLOCKER_SELECTED),
            hole_selected: Rgba::opaque([0x9C, 0xCB, 0xFF]),
            hole_valid: Rgba::opaque([0x5B, 0xD1, 0x7A]),
            hole_unchecked: Rgba::opaque([0x6E, 0x6A, 0x7A]),
            marker_fails: Rgba::opaque([0xE5, 0x48, 0x4D]),
            marker_hollowing: Rgba::opaque([0xF2, 0x9B, 0x2E]),
            marker_harmless: Rgba::opaque([0x9A, 0x98, 0xA6]),
            marker_repaired: Rgba::opaque([0x5B, 0xD1, 0x7A]),
            // Yellow to red (owner, 2026-10-26), as other slicers mark overhangs in warm tones.
            overhang_limit: Rgba([0xFF, 0xD2, 0x3F, 0xE0]),
            overhang_level: Rgba([0xE5, 0x30, 0x2B, 0xE6]),
            // Green, as the mockup's: kept clean of supports.
            protected: Rgba::opaque([0x2E, 0xB8, 0x6A]),
            // Dark (owner, 2026-11-06): it stands out on both light model colours.
            contact_line: Rgba::opaque([0x24, 0x22, 0x2A]),
            show_contact_line: true,
            status: crate::theme::STATUS_TOKENS.map(|token| Rgba(token.rgba)),
            brightness_percent: DEFAULT_BRIGHTNESS_PERCENT,
            ambient_percent: DEFAULT_AMBIENT_PERCENT,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_convert_to_linear_light() {
        // The owner's model colour, #b8b0a8, opaque by default.
        assert_eq!(Appearance::default().object, Rgba([0xB8, 0xB0, 0xA8, 0xFF]));
        // sRGB extremes are exact; mid grey is darker in linear light; alpha stays linear.
        assert_eq!(Rgba([0, 0, 0, 255]).linear(), [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(Rgba([255, 255, 255, 51]).linear(), [1.0, 1.0, 1.0, 0.2]);
        let grey = Rgba::opaque([128, 128, 128]).linear()[0];
        assert!((grey - 0.2158).abs() < 1e-3, "{grey}");
        assert_eq!(Rgba([255, 0, 51, 0]).encoded(), [1.0, 0.0, 0.2, 0.0]);
        // egui receives the colour unpremultiplied: a translucent white keeps its alpha.
        assert_eq!(Rgba([255, 255, 255, 128]).color().a(), 128);
    }
}
