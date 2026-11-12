//! The versioned `karasu-supports` JSON document: schema, presets, parameter resolution and
//! validation of ranges, couplings, identifiers and references (TD-062).
use super::{FindingKind, Severity, SupportError, SupportFinding, SupportPart};
use crate::geometry::{Affine3, Point3, Vector3};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Document format name.
pub const FORMAT: &str = "karasu-supports";
/// Supported document version.
pub const VERSION: u32 = 1;
/// Largest accepted document, in bytes.
pub const MAX_DOCUMENT_BYTES: usize = 64 * 1024 * 1024;
/// Most supports plus braces in one document.
pub const MAX_ENTITIES: usize = 100_000;
/// Most bend points of one support.
pub const MAX_BENDS: usize = 64;
/// The steps a contact fitted to the surface narrows by, in millimetres (TD-116).
pub const CONTACT_FIT_STEP_MM: f64 = 0.05;
/// How far from the surface a support's anchor may be when a document does not say, in mm.
pub const DEFAULT_ANCHOR_TOLERANCE_MM: f64 = 0.05;
/// Coordinates must lie within this many millimetres of the origin.
const COORDINATE_DOMAIN_MM: f64 = 10_000.0;

/// Built-in parameter presets, chosen by karasu within the ranges observed in reference slicers
/// and set by two test prints on the Photon P1 (TD-136, `docs/research/SUPPORT-PRESETS-RESEARCH.md`
/// E-SUP-02 and E-SUP-03): every contact sunk 0.1 mm, which held as well as deeper ones and came
/// off as easily; Heavy on a slender tip, since a stiff one made its supports hard to remove.
/// They are geometric parameters, proven on flat test tiles of one resin only.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Preset {
    /// Small contacts for fine details.
    Light,
    /// The default.
    #[default]
    Medium,
    /// Large contacts and wide feet for heavy parts.
    Heavy,
}

/// How the contact ends on the model.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ContactShape {
    /// A flat cone tip of `contact_diameter_mm`.
    Cone,
    /// A ball of `contact_sphere_diameter_mm` at the contact.
    Sphere,
}

/// Complete, resolved parameters of one support, in millimetres.
#[derive(Clone, Debug, PartialEq)]
pub struct SupportParameters {
    /// Contact end shape.
    pub contact_shape: ContactShape,
    /// Diameter of the tip where it meets the model.
    pub contact_diameter_mm: f64,
    /// With a value, the contact narrows to fit the surface (TD-116): the build takes the widest
    /// diameter from `contact_diameter_mm` down to this one, in steps of
    /// [`CONTACT_FIT_STEP_MM`], whose footprint lies on the model, the ball and the penetration
    /// narrowing with it. `None` keeps the contact as it is.
    pub contact_fit_min_mm: Option<f64>,
    /// Diameter of the contact ball (sphere contacts).
    pub contact_sphere_diameter_mm: f64,
    /// Diameter of the tip where it joins the shaft.
    pub tip_body_diameter_mm: f64,
    /// Tip length from the contact to the tip join.
    pub tip_length_mm: f64,
    /// Depth the contact reaches below the model surface.
    pub penetration_mm: f64,
    /// Shaft diameter at the tip join.
    pub shaft_diameter_mm: f64,
    /// Shaft diameter at the base join; `None` keeps the shaft uniform.
    pub shaft_bottom_diameter_mm: Option<f64>,
    /// Polygon sides of every round part (rings and sphere meridians).
    pub sides: u16,
    /// Plate pad diameter.
    pub pad_diameter_mm: f64,
    /// Plate pad thickness.
    pub pad_thickness_mm: f64,
    /// Diameter of the transition cone at the pad top.
    pub cone_diameter_mm: f64,
    /// Height of the transition cone from the pad top to the base join.
    pub cone_height_mm: f64,
    /// Diameter of a model foot where it meets the model.
    pub foot_contact_diameter_mm: f64,
    /// Diameter of a model foot at its join.
    pub foot_body_diameter_mm: f64,
    /// Model foot length.
    pub foot_length_mm: f64,
    /// Depth a model foot reaches below the model surface.
    pub foot_penetration_mm: f64,
    /// Diameter of a mini support ([`SupportKind::Mini`]): its whole length and both ends.
    pub mini_diameter_mm: f64,
    /// Depth each end of a mini support reaches below the model surface; zero touches it.
    pub mini_penetration_mm: f64,
}

impl Preset {
    /// The preset's complete parameters.
    #[must_use]
    pub fn parameters(self) -> SupportParameters {
        // [light, medium, heavy] for each value.
        let pick = |values: [f64; 3]| match self {
            Self::Light => values[0],
            Self::Medium => values[1],
            Self::Heavy => values[2],
        };
        SupportParameters {
            contact_shape: ContactShape::Cone,
            contact_diameter_mm: pick([0.3, 0.5, 0.6]),
            contact_fit_min_mm: None,
            contact_sphere_diameter_mm: pick([0.5, 0.6, 0.8]),
            tip_body_diameter_mm: pick([0.8, 1.0, 1.0]),
            tip_length_mm: pick([2.0, 2.5, 2.5]),
            penetration_mm: pick([0.1, 0.1, 0.1]),
            shaft_diameter_mm: pick([0.8, 1.0, 1.2]),
            shaft_bottom_diameter_mm: None,
            sides: 16,
            pad_diameter_mm: pick([4.0, 5.0, 7.0]),
            pad_thickness_mm: pick([0.4, 0.5, 0.6]),
            cone_diameter_mm: pick([2.0, 2.5, 3.5]),
            cone_height_mm: pick([0.8, 1.0, 1.2]),
            foot_contact_diameter_mm: pick([0.3, 0.4, 0.6]),
            foot_body_diameter_mm: pick([0.8, 1.0, 1.4]),
            foot_length_mm: pick([1.5, 2.0, 2.5]),
            foot_penetration_mm: pick([0.15, 0.2, 0.3]),
            mini_diameter_mm: pick([0.3, 0.4, 0.5]),
            mini_penetration_mm: pick([0.05, 0.1, 0.15]),
        }
    }
}

/// Partial parameters: `defaults` of a document or `parameters` of one support.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Overrides {
    #[serde(skip_serializing_if = "Option::is_none")]
    contact_shape: Option<ContactShape>,
    #[serde(skip_serializing_if = "Option::is_none")]
    contact_diameter_mm: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    contact_fit_min_mm: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    contact_sphere_diameter_mm: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tip_body_diameter_mm: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tip_length_mm: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    penetration_mm: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    shaft_diameter_mm: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    shaft_bottom_diameter_mm: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sides: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pad_diameter_mm: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pad_thickness_mm: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cone_diameter_mm: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cone_height_mm: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    foot_contact_diameter_mm: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    foot_body_diameter_mm: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    foot_length_mm: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    foot_penetration_mm: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mini_diameter_mm: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mini_penetration_mm: Option<f64>,
}

impl Overrides {
    fn apply(&self, p: &mut SupportParameters) {
        macro_rules! take {
            ($($field:ident),*) => {$(
                if let Some(value) = self.$field {
                    p.$field = value;
                }
            )*};
        }
        take!(
            contact_shape,
            contact_diameter_mm,
            contact_sphere_diameter_mm,
            tip_body_diameter_mm,
            tip_length_mm,
            penetration_mm,
            shaft_diameter_mm,
            sides,
            pad_diameter_mm,
            pad_thickness_mm,
            cone_diameter_mm,
            cone_height_mm,
            foot_contact_diameter_mm,
            foot_body_diameter_mm,
            foot_length_mm,
            foot_penetration_mm,
            mini_diameter_mm,
            mini_penetration_mm
        );
        if self.shaft_bottom_diameter_mm.is_some() {
            p.shaft_bottom_diameter_mm = self.shaft_bottom_diameter_mm;
        }
        if self.contact_fit_min_mm.is_some() {
            p.contact_fit_min_mm = self.contact_fit_min_mm;
        }
    }
}

/// Direction of a tip or model-foot axis.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AxisSpec {
    /// The outward normal of the anchor triangle.
    Normal,
    /// Towards the adjacent path point.
    Straight,
    /// An explicit direction (normalised when built).
    Direction([f64; 3]),
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum AxisMode {
    #[default]
    Normal,
    Straight,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(untagged)]
enum RawAxis {
    Mode(AxisMode),
    Direction([f64; 3]),
}

impl Default for RawAxis {
    fn default() -> Self {
        Self::Mode(AxisMode::Normal)
    }
}

impl From<RawAxis> for AxisSpec {
    fn from(raw: RawAxis) -> Self {
        match raw {
            RawAxis::Mode(AxisMode::Normal) => Self::Normal,
            RawAxis::Mode(AxisMode::Straight) => Self::Straight,
            RawAxis::Direction(d) => Self::Direction(d),
        }
    }
}

/// Where a support's base stands.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BaseSpec {
    /// On the plate (or raft) at this world XY.
    Platform([f64; 2]),
    /// On the plate below the last path point.
    Vertical,
    /// On the model surface at this point, with a foot along `axis`.
    Model {
        /// Anchor point on the model surface.
        point_mm: [f64; 3],
        /// Foot axis, pointing out of the surface.
        axis: AxisSpec,
    },
    /// On another support's shaft at height `z_mm`.
    Support {
        /// The parent support's ID.
        parent: u64,
        /// World height of the junction.
        z_mm: f64,
    },
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
enum RawBase {
    Platform(RawPlatform),
    Vertical(RawVertical),
    Model(RawModelBase),
    Support(RawSupportBase),
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RawPlatform {
    platform: [f64; 2],
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RawVertical {
    vertical: bool,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RawModelBase {
    model: [f64; 3],
    #[serde(default)]
    axis: RawAxis,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RawSupportBase {
    support: u64,
    z_mm: f64,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RawTip {
    point: [f64; 3],
    #[serde(default)]
    axis: RawAxis,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RawSupport {
    id: u64,
    tip: RawTip,
    #[serde(default)]
    bends: Vec<[f64; 3]>,
    base: RawBase,
    #[serde(default)]
    parameters: Overrides,
    #[serde(default)]
    accept_collision: bool,
    #[serde(default)]
    origin: SupportOrigin,
    #[serde(default)]
    kind: SupportKind,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RawBraceEnd {
    support: u64,
    z_mm: f64,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RawBrace {
    id: u64,
    from: RawBraceEnd,
    to: RawBraceEnd,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    diameter_mm: Option<f64>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    max_diameter_mm: Option<f64>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RawRaft {
    thickness_mm: f64,
    margin_mm: f64,
    slope_deg: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    fillet_diameter_mm: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    fillet_height_mm: Option<f64>,
    /// Present: one raft per group of feet closer than this; absent: one raft.
    #[serde(skip_serializing_if = "Option::is_none")]
    groups_join_mm: Option<f64>,
    /// The raft's shape (TD-144); absent: a slab, as every document before it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    shape: Option<RaftShape>,
    /// A web's bars (TD-144), written for a web only; absent: the defaults.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    bar_width_mm: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    bar_height_mm: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    bar_longest_mm: Option<f64>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RawBinding {
    #[serde(skip_serializing_if = "Option::is_none")]
    model_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    translation_mm: Option<[f64; 3]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rotate_deg: Option<[f64; 3]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    scale: Option<[f64; 3]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mirror: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RawDocument {
    format: String,
    version: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    binding: Option<RawBinding>,
    #[serde(default)]
    preset: Preset,
    #[serde(default)]
    defaults: Overrides,
    #[serde(skip_serializing_if = "Option::is_none")]
    clearance_mm: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    anchor_tolerance_mm: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_axis_deviation_deg: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    raft: Option<RawRaft>,
    supports: Vec<RawSupport>,
    #[serde(default)]
    braces: Vec<RawBrace>,
}

/// The model and placement a document was written for.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Binding {
    /// Lowercase hexadecimal SHA-256 of the model file, when recorded.
    pub model_sha256: Option<String>,
    /// World translation of the placement, when recorded.
    pub translation_mm: Option<[f64; 3]>,
    /// The model's orientation and size when the supports were placed, when recorded (TD-072).
    /// With it, the supports follow the model through moves on the plate and turns about the
    /// vertical axis; without it, the placement must match `translation_mm` exactly.
    pub pose: Option<BoundPose>,
}

/// The orientation and size a document's supports were placed for (TD-072): the model's
/// rotation, scale and mirroring, with the meanings of the placement options of the same names.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoundPose {
    /// Fixed-axis X, then Y, then Z rotation, in degrees.
    pub rotate_deg: [f64; 3],
    /// Positive per-axis scale.
    pub scale: [f64; 3],
    /// Mirrored axes.
    pub mirror: [bool; 3],
}

/// A rigid motion that keeps the plate horizontal: a turn about the vertical axis through
/// `from_mm`, then a move that takes `from_mm` to `to_mm` (TD-072). Heights are unchanged.
///
/// Turns by whole multiples of 90 degrees use exact sines and cosines, so such moves keep
/// coordinates exact up to the translation's rounding.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlanarMotion {
    /// Counter-clockwise turn seen from above, in degrees.
    pub turn_deg: f64,
    /// The XY point the turn is about, before the motion.
    pub from_mm: [f64; 2],
    /// Where that point goes.
    pub to_mm: [f64; 2],
}

impl PlanarMotion {
    /// Whether the motion moves nothing.
    #[must_use]
    #[allow(clippy::float_cmp)] // Only the exact identity skips the motion.
    pub fn is_identity(&self) -> bool {
        self.turn_deg == 0.0 && self.from_mm == self.to_mm
    }

    fn sin_cos(&self) -> (f64, f64) {
        let quarter = self.turn_deg / 90.0;
        if quarter.fract() == 0.0 {
            // Exact values for whole quarter turns.
            // Reduced modulo 4 first: an exact whole number from 0 to 3.
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            match quarter.rem_euclid(4.0) as u8 {
                0 => (0.0, 1.0),
                1 => (1.0, 0.0),
                2 => (0.0, -1.0),
                _ => (-1.0, 0.0),
            }
        } else {
            self.turn_deg.to_radians().sin_cos()
        }
    }

    /// The moved point.
    #[must_use]
    pub fn point(&self, point: [f64; 3]) -> [f64; 3] {
        let [x, y] = self.xy([point[0], point[1]]);
        [x, y, point[2]]
    }

    /// The moved XY point.
    #[must_use]
    pub fn xy(&self, [x, y]: [f64; 2]) -> [f64; 2] {
        let (sin, cos) = self.sin_cos();
        let [dx, dy] = [x - self.from_mm[0], y - self.from_mm[1]];
        [
            self.to_mm[0] + (dx * cos - dy * sin),
            self.to_mm[1] + (dx * sin + dy * cos),
        ]
    }

    /// The turned direction.
    #[must_use]
    pub fn direction(&self, [x, y, z]: [f64; 3]) -> [f64; 3] {
        let (sin, cos) = self.sin_cos();
        [x * cos - y * sin, x * sin + y * cos, z]
    }
}

/// A raft: the plate supports' foot (owner, 2026-10-29). With one, a support standing on the
/// plate has no pad of its own: its shaft stands on the raft's top through a fillet.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RaftSpec {
    /// Raft thickness; the shafts stand on its top.
    pub thickness_mm: f64,
    /// Margin beyond the outermost fillets at the raft top.
    pub margin_mm: f64,
    /// Side-wall angle from the horizontal, in degrees (90 is vertical).
    pub slope_deg: f64,
    /// Where a shaft meets the raft: the fillet's diameter at the raft top (at least the
    /// shaft's own).
    pub fillet_diameter_mm: f64,
    /// The fillet's height above the raft top; zero joins the shaft straight.
    pub fillet_height_mm: f64,
    /// One raft under every plate foot, or one under each group of feet.
    pub layout: RaftLayout,
    /// Its shape: which way its sides slope, or a web of discs and bars (TD-144).
    pub shape: RaftShape,
    /// A web's bars; kept, unused, for the other shapes.
    pub bars: RaftBars,
}

/// The bars of a web raft between its feet's discs (TD-144).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RaftBars {
    /// The bar's width at its top, in mm; it widens down to the plate by the raft's slope.
    pub width_mm: f64,
    /// The bar's height above the plate, in mm.
    pub height_mm: f64,
    /// Feet whose centres lie farther apart than this, in mm, are not joined by a bar.
    pub longest_mm: f64,
}

impl RaftBars {
    /// 1 mm wide, 0.5 mm high, joining feet up to 20 mm apart (owner, 2026-11-10).
    pub const DEFAULT: Self = Self {
        width_mm: 1.0,
        height_mm: 0.5,
        longest_mm: 20.0,
    };
}

impl Default for RaftBars {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// A raft's shape (owner, 2026-11-10, TD-144).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RaftShape {
    /// Wider at the plate: its sides slope out from its top down to the plate.
    #[default]
    Slab,
    /// Narrower at the plate: its sides slope in from its top down to the plate, over its whole
    /// thickness, so a scraper slips under its edge.
    Skate,
    /// A disc under each foot, its sides sloping out as a slab's, and low bars joining
    /// neighbouring feet in triangles that never cross.
    Web,
}

/// The narrowest a skate raft may be at the plate under a foot, as a radius from the foot's
/// centre, in mm: a tenth of a millimetre, two pixels of a 50 µm panel. Narrower, the raft
/// would stand on a point or turn inside out, and its build fails instead (TD-144).
pub const SKATE_MIN_PLATE_RADIUS_MM: f64 = 0.1;

/// How rafts lie under the plate feet.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RaftLayout {
    /// One raft under every plate foot.
    Single,
    /// One raft under each group of plate feet: feet whose centres lie less than `join_mm`
    /// apart, directly or through others, share one.
    Groups {
        /// The distance under which feet share a raft, in mm.
        join_mm: f64,
    },
}

/// A raft's fillet diameter when a document gives none, in mm.
pub const DEFAULT_FILLET_DIAMETER_MM: f64 = 2.0;
/// A raft's fillet height when a document gives none, in mm.
pub const DEFAULT_FILLET_HEIGHT_MM: f64 = 0.8;

impl RaftSpec {
    /// How far its sides reach out (a slab) or in (a skate) from its top's edge down to the
    /// plate, in mm: its thickness over the tangent of its slope; zero for vertical sides.
    #[must_use]
    pub fn spread_mm(&self) -> f64 {
        let (sine, cosine) = crate::geometry::sin_cos_degrees(self.slope_deg);
        if cosine > 0.0 {
            self.thickness_mm * cosine / sine
        } else {
            0.0
        }
    }

    /// The radius the raft has at the plate under a foot whose fillet has `radius_mm`: its
    /// top's (the fillet's widened by the margin), widened by the spread for a slab and
    /// narrowed by it for a skate.
    #[must_use]
    pub fn plate_radius_mm(&self, radius_mm: f64) -> f64 {
        let top = radius_mm + self.margin_mm;
        match self.shape {
            RaftShape::Slab | RaftShape::Web => top + self.spread_mm(),
            RaftShape::Skate => top - self.spread_mm(),
        }
    }

    /// Whether a skate raft keeps [`SKATE_MIN_PLATE_RADIUS_MM`] at the plate under its
    /// narrowest foot (a fillet of its own diameter): true for every slab. A foot with a wider
    /// shaft is wider still.
    #[must_use]
    pub fn stands_on_the_plate(&self) -> bool {
        self.plate_radius_mm(self.fillet_diameter_mm / 2.0) >= SKATE_MIN_PLATE_RADIUS_MM
    }

    /// Check the raft's values against the document's ranges.
    fn checked(self) -> Result<Self, SupportError> {
        ranged(0, "raft thickness_mm", self.thickness_mm, [0.2, 5.0])?;
        ranged(0, "raft margin_mm", self.margin_mm, [0.0, 20.0])?;
        ranged(0, "raft slope_deg", self.slope_deg, [30.0, 90.0])?;
        ranged(
            0,
            "raft fillet_diameter_mm",
            self.fillet_diameter_mm,
            [0.2, 10.0],
        )?;
        ranged(
            0,
            "raft fillet_height_mm",
            self.fillet_height_mm,
            [0.0, 5.0],
        )?;
        ranged(0, "raft bar_width_mm", self.bars.width_mm, [0.2, 10.0])?;
        ranged(0, "raft bar_height_mm", self.bars.height_mm, [0.2, 5.0])?;
        ranged(0, "raft bar_longest_mm", self.bars.longest_mm, [1.0, 200.0])?;
        if let RaftLayout::Groups { join_mm } = self.layout {
            ranged(0, "raft groups_join_mm", join_mm, [0.5, 200.0])?;
        }
        Ok(self)
    }
}

/// What made a support: placed by hand (or by a support tool one at a time), or under the
/// islands by the Islands tool, which can then find and delete its own (owner, 2026-10-29).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SupportOrigin {
    /// Placed: the default, and every support of a document that does not say.
    #[default]
    Placed,
    /// Placed under an island by the Islands tool.
    Islands,
    /// Planned by the Automatic tool (M6.4i): generating again replaces these, and only these.
    Automatic,
}

/// What a support is made of (owner, 2026-10-30).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SupportKind {
    /// A tip, a shaft and a base on the plate, the model or another support: the default.
    #[default]
    Standard,
    /// A mini support: a thin rod of one diameter with a rounded end at each anchor, no tip cone
    /// and no foot, standing on the model or on another support. It holds a spot with too little
    /// room below for a standard support, or no way down to the plate.
    Mini,
}

/// One manual support, with its complete resolved parameters.
#[derive(Clone, Debug, PartialEq)]
pub struct ManualSupport {
    /// Stable nonzero ID.
    pub id: u64,
    /// Contact point on the model surface.
    pub tip_mm: [f64; 3],
    /// Tip axis.
    pub tip_axis: AxisSpec,
    /// Bend points of the shaft, from the tip down.
    pub bends: Vec<[f64; 3]>,
    /// Where the base stands.
    pub base: BaseSpec,
    /// Complete parameters: preset, then document defaults, then this support's overrides.
    pub parameters: SupportParameters,
    /// Whether a collision of this support with the model is accepted (and still reported).
    pub accept_collision: bool,
    /// What made it.
    pub origin: SupportOrigin,
    /// What it is made of.
    pub kind: SupportKind,
}

/// A brace between two support shafts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Brace {
    /// Stable nonzero ID, unique across supports and braces.
    pub id: u64,
    /// First end: support ID and world height.
    pub from: (u64, f64),
    /// Second end: support ID and world height.
    pub to: (u64, f64),
    /// Brace diameter; `None` is automatic: the thinner of the two shafts where the brace
    /// meets them, worked out at each build, so the brace follows its supports.
    pub diameter_mm: Option<f64>,
    /// An automatic brace's largest diameter (TD-136): it is the thinner shaft's up to this.
    /// `None` leaves it the thinner shaft's, as documents written before TD-136 read. Only an
    /// automatic brace has one.
    pub max_diameter_mm: Option<f64>,
}

impl Brace {
    /// The brace's radius where it meets shafts of radii `a` and `b`, in mm: its own diameter's,
    /// or the thinner shaft's up to its largest.
    #[must_use]
    pub fn radius_mm(&self, a: f64, b: f64) -> f64 {
        self.diameter_mm.map_or_else(
            || {
                let thinner = a.min(b);
                self.max_diameter_mm
                    .map_or(thinner, |d| thinner.min(d / 2.0))
            },
            |d| d / 2.0,
        )
    }
}

/// A validated `karasu-supports` document.
#[derive(Clone, Debug, PartialEq)]
pub struct SupportDocument {
    /// Recorded model and placement, if any.
    pub binding: Option<Binding>,
    /// Preset the parameters started from.
    pub preset: Preset,
    /// Minimum distance between non-contact support parts and the model.
    pub clearance_mm: f64,
    /// Largest distance between an anchor and the model surface.
    pub anchor_tolerance_mm: f64,
    /// Largest angle between a tip or foot axis and the anchor normal, in degrees.
    pub max_axis_deviation_deg: f64,
    /// Raft, if any.
    pub raft: Option<RaftSpec>,
    /// Supports, parents before the supports standing on them.
    pub supports: Vec<ManualSupport>,
    /// Braces.
    pub braces: Vec<Brace>,
    /// Soft-range warnings found while reading.
    pub warnings: Vec<SupportFinding>,
}

fn invalid(entity: u64, reason: String) -> SupportError {
    SupportError::Invalid { entity, reason }
}

fn coordinate(entity: u64, what: &str, values: &[f64]) -> Result<(), SupportError> {
    if values
        .iter()
        .all(|v| v.is_finite() && v.abs() <= COORDINATE_DOMAIN_MM)
    {
        Ok(())
    } else {
        Err(invalid(
            entity,
            format!("{what} must be finite and within {COORDINATE_DOMAIN_MM} mm of the origin"),
        ))
    }
}

/// A value in its hard range, or an error naming the field.
fn ranged(entity: u64, name: &str, value: f64, [lo, hi]: [f64; 2]) -> Result<(), SupportError> {
    if value.is_finite() && (lo..=hi).contains(&value) {
        Ok(())
    } else {
        Err(invalid(
            entity,
            format!("{name} is {value}, outside the accepted range {lo} to {hi}"),
        ))
    }
}

impl SupportParameters {
    /// The radius of the contact's footprint at the surface, in mm: a cone's contact, or the
    /// circle where a ball sunk by the penetration meets the tangent plane. The surface's normal
    /// at a tip is the mean over it (TD-116, TD-124).
    #[must_use]
    pub fn footprint_radius_mm(&self) -> f64 {
        match self.contact_shape {
            ContactShape::Cone => self.contact_diameter_mm / 2.0,
            ContactShape::Sphere => {
                let rho = self.contact_sphere_diameter_mm / 2.0;
                let d = self.penetration_mm;
                if d >= rho {
                    rho
                } else {
                    (d * (2.0 * rho - d)).sqrt()
                }
            }
        }
    }

    /// The contact's width where it meets a level surface, in mm (TD-136): the mark it leaves.
    /// A cone's diameter is its end's, `penetration_mm` inside the model; the cone widens
    /// towards the tip's body over the tip's length, so at the surface it is
    /// `d + (body - d) * penetration / length` wide (the whole body's past the tip's length). A
    /// ball's is its footprint's ([`Self::footprint_radius_mm`]).
    #[must_use]
    pub fn surface_width_mm(&self) -> f64 {
        match self.contact_shape {
            ContactShape::Cone => {
                let d = self.contact_diameter_mm;
                let share = if self.tip_length_mm > 0.0 {
                    (self.penetration_mm / self.tip_length_mm).min(1.0)
                } else {
                    1.0
                };
                d + (self.tip_body_diameter_mm - d) * share
            }
            ContactShape::Sphere => 2.0 * self.footprint_radius_mm(),
        }
    }

    /// Check hard ranges and couplings (errors) and soft ranges (warnings).
    ///
    /// # Errors
    /// The first value outside its hard range or violating a coupling, naming the field.
    #[allow(clippy::too_many_lines)] // Flat tables of ranges, couplings and soft ranges.
    pub fn validate(
        &self,
        entity: u64,
        warnings: &mut Vec<SupportFinding>,
    ) -> Result<(), SupportError> {
        let p = self;
        for (name, value, range) in [
            ("contact_diameter_mm", p.contact_diameter_mm, [0.05, 5.0]),
            (
                "contact_sphere_diameter_mm",
                p.contact_sphere_diameter_mm,
                [0.05, 5.0],
            ),
            ("tip_body_diameter_mm", p.tip_body_diameter_mm, [0.1, 10.0]),
            ("tip_length_mm", p.tip_length_mm, [0.2, 30.0]),
            ("penetration_mm", p.penetration_mm, [0.0, 5.0]),
            ("shaft_diameter_mm", p.shaft_diameter_mm, [0.1, 20.0]),
            ("pad_diameter_mm", p.pad_diameter_mm, [0.5, 50.0]),
            ("pad_thickness_mm", p.pad_thickness_mm, [0.1, 5.0]),
            ("cone_diameter_mm", p.cone_diameter_mm, [0.1, 50.0]),
            ("cone_height_mm", p.cone_height_mm, [0.0, 20.0]),
            (
                "foot_contact_diameter_mm",
                p.foot_contact_diameter_mm,
                [0.05, 5.0],
            ),
            (
                "foot_body_diameter_mm",
                p.foot_body_diameter_mm,
                [0.1, 10.0],
            ),
            ("foot_length_mm", p.foot_length_mm, [0.2, 30.0]),
            ("foot_penetration_mm", p.foot_penetration_mm, [0.0, 5.0]),
            ("mini_diameter_mm", p.mini_diameter_mm, [0.1, 2.0]),
            ("mini_penetration_mm", p.mini_penetration_mm, [0.0, 1.0]),
        ] {
            ranged(entity, name, value, range)?;
        }
        if let Some(bottom) = p.shaft_bottom_diameter_mm {
            ranged(entity, "shaft_bottom_diameter_mm", bottom, [0.1, 20.0])?;
        }
        if let Some(least) = p.contact_fit_min_mm {
            ranged(entity, "contact_fit_min_mm", least, [0.05, 5.0])?;
            if least > p.contact_diameter_mm {
                return Err(invalid(
                    entity,
                    format!(
                        "contact_fit_min_mm ({least}) must not exceed contact_diameter_mm ({})",
                        p.contact_diameter_mm
                    ),
                ));
            }
        }
        if !(3..=64).contains(&p.sides) {
            return Err(invalid(
                entity,
                format!("sides is {}, outside the accepted range 3 to 64", p.sides),
            ));
        }
        for (smaller, larger, a, b) in [
            (
                "contact_diameter_mm",
                "tip_body_diameter_mm",
                p.contact_diameter_mm,
                p.tip_body_diameter_mm,
            ),
            (
                "foot_contact_diameter_mm",
                "foot_body_diameter_mm",
                p.foot_contact_diameter_mm,
                p.foot_body_diameter_mm,
            ),
            (
                "cone_diameter_mm",
                "pad_diameter_mm",
                p.cone_diameter_mm,
                p.pad_diameter_mm,
            ),
            (
                "contact_diameter_mm",
                "contact_sphere_diameter_mm",
                p.contact_diameter_mm,
                p.contact_sphere_diameter_mm,
            ),
        ] {
            if a > b {
                return Err(invalid(
                    entity,
                    format!("{smaller} ({a}) must not exceed {larger} ({b})"),
                ));
            }
        }
        // A mini support's end is a ball centred off the surface (section 4 of the specification):
        // reaching deeper than its radius would sink its centre, and the shaft with it.
        if p.mini_penetration_mm >= p.mini_diameter_mm / 2.0 {
            return Err(invalid(
                entity,
                format!(
                    "mini_penetration_mm ({}) must be less than half of mini_diameter_mm ({})",
                    p.mini_penetration_mm, p.mini_diameter_mm
                ),
            ));
        }
        let mut soft = |name: &str, value: f64, ok: bool, range: &str| {
            if !ok {
                warnings.push(SupportFinding {
                    entity,
                    part: SupportPart::Parameters,
                    kind: FindingKind::SoftRange,
                    severity: Severity::Warning,
                    detail: format!("{name} is {value}; {range} is recommended"),
                });
            }
        };
        soft(
            "contact_diameter_mm",
            p.contact_diameter_mm,
            p.contact_diameter_mm >= 0.2,
            "0.2 mm or more",
        );
        if let Some(least) = p.contact_fit_min_mm {
            soft("contact_fit_min_mm", least, least >= 0.2, "0.2 mm or more");
        }
        soft(
            "tip_length_mm",
            p.tip_length_mm,
            p.tip_length_mm >= 1.0,
            "1 mm or more",
        );
        soft(
            "penetration_mm",
            p.penetration_mm,
            p.penetration_mm <= 1.0,
            "1 mm or less",
        );
        soft("sides", f64::from(p.sides), p.sides >= 6, "6 or more");
        soft(
            "pad_thickness_mm",
            p.pad_thickness_mm,
            p.pad_thickness_mm <= 2.0,
            "2 mm or less",
        );
        soft(
            "foot_contact_diameter_mm",
            p.foot_contact_diameter_mm,
            p.foot_contact_diameter_mm >= 0.2,
            "0.2 mm or more",
        );
        soft(
            "foot_penetration_mm",
            p.foot_penetration_mm,
            p.foot_penetration_mm <= 1.0,
            "1 mm or less",
        );
        soft(
            "mini_diameter_mm",
            p.mini_diameter_mm,
            p.mini_diameter_mm >= 0.2,
            "0.2 mm or more",
        );
        Ok(())
    }
}

fn axis(entity: u64, raw: RawAxis) -> Result<AxisSpec, SupportError> {
    let spec = AxisSpec::from(raw);
    if let AxisSpec::Direction(d) = spec {
        // A zero or non-finite vector has no positive finite squared length.
        let squared = d[0] * d[0] + d[1] * d[1] + d[2] * d[2];
        if !(squared.is_finite() && squared > 0.0) {
            return Err(invalid(
                entity,
                "an explicit axis must be a finite nonzero vector".to_owned(),
            ));
        }
    }
    Ok(spec)
}

impl SupportDocument {
    /// The document moved rigidly with its model by `motion` (TD-072): tips, bends, bases and
    /// axes. Heights of support-to-support junctions and braces, the raft and every parameter are
    /// unchanged. The result is unbound: it describes a placement the binding does not record.
    #[must_use]
    pub fn moved(&self, motion: &PlanarMotion) -> Self {
        let axis = |axis: AxisSpec| match axis {
            AxisSpec::Direction(d) => AxisSpec::Direction(motion.direction(d)),
            other => other,
        };
        let supports = self
            .supports
            .iter()
            .map(|s| ManualSupport {
                tip_mm: motion.point(s.tip_mm),
                tip_axis: axis(s.tip_axis),
                bends: s.bends.iter().map(|&b| motion.point(b)).collect(),
                base: match s.base {
                    BaseSpec::Platform(xy) => BaseSpec::Platform(motion.xy(xy)),
                    BaseSpec::Model { point_mm, axis: a } => BaseSpec::Model {
                        point_mm: motion.point(point_mm),
                        axis: axis(a),
                    },
                    other @ (BaseSpec::Vertical | BaseSpec::Support { .. }) => other,
                },
                ..s.clone()
            })
            .collect();
        Self {
            binding: None,
            supports,
            ..self.clone()
        }
    }

    /// The document written in a model's own frame, carried to the world by its `placement`
    /// (TD-108): tips, bends and feet on the model follow every move, turn, scale and mirror; an
    /// explicit axis is carried by the placement's linear part (normalised when built). A foot on
    /// the plate at `[x, y]` is the point `(x, y, 0)` of the model frame, carried and set on the
    /// plate. Heights on shafts (supports standing on supports, braces), the raft and every
    /// parameter are world values and stay as they are. The result is unbound.
    ///
    /// # Errors
    /// A point or axis the placement cannot carry (not finite, or overflowing), naming the support.
    pub fn placed(&self, placement: &Affine3) -> Result<Self, SupportError> {
        let carried = |id: u64, what: &str, p: [f64; 3]| {
            placement
                .transform_point(Point3::new(p[0], p[1], p[2]))
                .map(Point3::coordinates)
                .map_err(|e| invalid(id, format!("its {what} cannot be placed: {e}")))
        };
        let axis = |id: u64, axis: AxisSpec| match axis {
            AxisSpec::Direction([x, y, z]) => placement
                .transform_vector(Vector3::new(x, y, z))
                .map(|v| AxisSpec::Direction(v.components()))
                .map_err(|e| invalid(id, format!("its axis cannot be placed: {e}"))),
            other => Ok(other),
        };
        let supports = self
            .supports
            .iter()
            .map(|s| {
                let base = match s.base {
                    BaseSpec::Platform([x, y]) => {
                        let [x, y, _] = carried(s.id, "foot", [x, y, 0.0])?;
                        BaseSpec::Platform([x, y])
                    }
                    BaseSpec::Model { point_mm, axis: a } => BaseSpec::Model {
                        point_mm: carried(s.id, "foot", point_mm)?,
                        axis: axis(s.id, a)?,
                    },
                    other @ (BaseSpec::Vertical | BaseSpec::Support { .. }) => other,
                };
                Ok(ManualSupport {
                    tip_mm: carried(s.id, "tip", s.tip_mm)?,
                    tip_axis: axis(s.id, s.tip_axis)?,
                    bends: s
                        .bends
                        .iter()
                        .map(|&b| carried(s.id, "bend", b))
                        .collect::<Result<_, _>>()?,
                    base,
                    ..s.clone()
                })
            })
            .collect::<Result<_, SupportError>>()?;
        Ok(Self {
            binding: None,
            supports,
            ..self.clone()
        })
    }

    /// Parse and validate a `karasu-supports` document.
    ///
    /// # Errors
    /// Malformed JSON, unknown fields, a wrong format or version, values outside their hard
    /// ranges, broken couplings, duplicate or zero IDs, unknown references and cycles of
    /// supports standing on supports. Nothing is clamped or dropped.
    pub fn from_json(bytes: &[u8]) -> Result<Self, SupportError> {
        if bytes.len() > MAX_DOCUMENT_BYTES {
            return Err(invalid(
                0,
                format!("document exceeds {MAX_DOCUMENT_BYTES} bytes"),
            ));
        }
        let raw: RawDocument =
            serde_json::from_slice(bytes).map_err(|e| SupportError::Json(e.to_string()))?;
        if raw.format != FORMAT || raw.version != VERSION {
            return Err(invalid(
                0,
                format!(
                    "document is {:?} version {}; expected {FORMAT:?} version {VERSION}",
                    raw.format, raw.version
                ),
            ));
        }
        if raw.supports.len() + raw.braces.len() > MAX_ENTITIES {
            return Err(invalid(
                0,
                format!("more than {MAX_ENTITIES} supports and braces"),
            ));
        }
        let clearance_mm = raw.clearance_mm.unwrap_or(0.3);
        ranged(0, "clearance_mm", clearance_mm, [0.0, 5.0])?;
        let anchor_tolerance_mm = raw
            .anchor_tolerance_mm
            .unwrap_or(DEFAULT_ANCHOR_TOLERANCE_MM);
        ranged(0, "anchor_tolerance_mm", anchor_tolerance_mm, [0.001, 1.0])?;
        let max_axis_deviation_deg = raw.max_axis_deviation_deg.unwrap_or(60.0);
        ranged(
            0,
            "max_axis_deviation_deg",
            max_axis_deviation_deg,
            [0.0, 89.0],
        )?;
        let raft = raw
            .raft
            .map(|r| {
                RaftSpec {
                    thickness_mm: r.thickness_mm,
                    margin_mm: r.margin_mm,
                    slope_deg: r.slope_deg,
                    fillet_diameter_mm: r.fillet_diameter_mm.unwrap_or(DEFAULT_FILLET_DIAMETER_MM),
                    fillet_height_mm: r.fillet_height_mm.unwrap_or(DEFAULT_FILLET_HEIGHT_MM),
                    layout: r
                        .groups_join_mm
                        .map_or(RaftLayout::Single, |join_mm| RaftLayout::Groups { join_mm }),
                    shape: r.shape.unwrap_or_default(),
                    bars: {
                        let d = RaftBars::default();
                        RaftBars {
                            width_mm: r.bar_width_mm.unwrap_or(d.width_mm),
                            height_mm: r.bar_height_mm.unwrap_or(d.height_mm),
                            longest_mm: r.bar_longest_mm.unwrap_or(d.longest_mm),
                        }
                    },
                }
                .checked()
            })
            .transpose()?;
        let binding = raw.binding.map(read_binding).transpose()?;
        let mut warnings = Vec::new();
        let mut ids = BTreeSet::new();
        let mut supports = Vec::with_capacity(raw.supports.len());
        for s in raw.supports {
            if s.id == 0 || !ids.insert(s.id) {
                return Err(invalid(
                    s.id,
                    "support IDs must be nonzero and unique".to_owned(),
                ));
            }
            supports.push(read_support(s, raw.preset, &raw.defaults, &mut warnings)?);
        }
        let support_ids: BTreeSet<u64> = supports.iter().map(|s| s.id).collect();
        let mut braces = Vec::with_capacity(raw.braces.len());
        for b in raw.braces {
            if b.id == 0 || !ids.insert(b.id) {
                return Err(invalid(
                    b.id,
                    "brace IDs must be nonzero and unique across supports and braces".to_owned(),
                ));
            }
            braces.push(read_brace(&b, &support_ids)?);
        }
        let supports = parents_first(supports)?;
        Ok(Self {
            binding,
            preset: raw.preset,
            clearance_mm,
            anchor_tolerance_mm,
            max_axis_deviation_deg,
            raft,
            supports,
            braces,
            warnings,
        })
    }
}

impl SupportDocument {
    /// The document as `karasu-supports` JSON, which [`Self::from_json`] reads back to an equal
    /// document (TD-130): every support with its complete parameters, so neither the preset nor
    /// any default changes what it reads as, and the checks the document holds. Its warnings
    /// are not written; reading finds them again.
    ///
    /// # Panics
    /// Never: the document holds numbers and strings only, which always serialise.
    #[must_use]
    pub fn to_json(&self) -> Vec<u8> {
        let raw = RawDocument {
            format: FORMAT.to_owned(),
            version: VERSION,
            binding: self.binding.as_ref().map(raw_binding),
            preset: self.preset,
            defaults: Overrides::default(),
            clearance_mm: Some(self.clearance_mm),
            anchor_tolerance_mm: Some(self.anchor_tolerance_mm),
            max_axis_deviation_deg: Some(self.max_axis_deviation_deg),
            raft: self.raft.map(|r| RawRaft {
                thickness_mm: r.thickness_mm,
                margin_mm: r.margin_mm,
                slope_deg: r.slope_deg,
                fillet_diameter_mm: Some(r.fillet_diameter_mm),
                fillet_height_mm: Some(r.fillet_height_mm),
                groups_join_mm: match r.layout {
                    RaftLayout::Single => None,
                    RaftLayout::Groups { join_mm } => Some(join_mm),
                },
                // A slab is written as before, so earlier builds still read it.
                shape: (r.shape != RaftShape::Slab).then_some(r.shape),
                bar_width_mm: (r.shape == RaftShape::Web).then_some(r.bars.width_mm),
                bar_height_mm: (r.shape == RaftShape::Web).then_some(r.bars.height_mm),
                bar_longest_mm: (r.shape == RaftShape::Web).then_some(r.bars.longest_mm),
            }),
            supports: self.supports.iter().map(raw_support).collect(),
            braces: self
                .braces
                .iter()
                .map(|b| RawBrace {
                    id: b.id,
                    from: RawBraceEnd {
                        support: b.from.0,
                        z_mm: b.from.1,
                    },
                    to: RawBraceEnd {
                        support: b.to.0,
                        z_mm: b.to.1,
                    },
                    diameter_mm: b.diameter_mm,
                    max_diameter_mm: b.max_diameter_mm,
                })
                .collect(),
        };
        // Plain structs of numbers and strings: serialising them cannot fail.
        serde_json::to_vec(&raw).expect("a support document always serialises")
    }
}

fn raw_axis(axis: AxisSpec) -> RawAxis {
    match axis {
        AxisSpec::Normal => RawAxis::Mode(AxisMode::Normal),
        AxisSpec::Straight => RawAxis::Mode(AxisMode::Straight),
        AxisSpec::Direction(d) => RawAxis::Direction(d),
    }
}

fn raw_binding(b: &Binding) -> RawBinding {
    RawBinding {
        model_sha256: b.model_sha256.clone(),
        translation_mm: b.translation_mm,
        rotate_deg: b.pose.map(|p| p.rotate_deg),
        scale: b.pose.map(|p| p.scale),
        mirror: b.pose.map(|p| {
            ['x', 'y', 'z']
                .into_iter()
                .zip(p.mirror)
                .filter_map(|(axis, mirrored)| mirrored.then_some(axis))
                .collect()
        }),
    }
}

fn raw_support(s: &ManualSupport) -> RawSupport {
    let p = &s.parameters;
    RawSupport {
        id: s.id,
        tip: RawTip {
            point: s.tip_mm,
            axis: raw_axis(s.tip_axis),
        },
        bends: s.bends.clone(),
        base: match s.base {
            BaseSpec::Platform(platform) => RawBase::Platform(RawPlatform { platform }),
            BaseSpec::Vertical => RawBase::Vertical(RawVertical { vertical: true }),
            BaseSpec::Model { point_mm, axis } => RawBase::Model(RawModelBase {
                model: point_mm,
                axis: raw_axis(axis),
            }),
            BaseSpec::Support { parent, z_mm } => RawBase::Support(RawSupportBase {
                support: parent,
                z_mm,
            }),
        },
        parameters: Overrides {
            contact_shape: Some(p.contact_shape),
            contact_diameter_mm: Some(p.contact_diameter_mm),
            contact_fit_min_mm: p.contact_fit_min_mm,
            contact_sphere_diameter_mm: Some(p.contact_sphere_diameter_mm),
            tip_body_diameter_mm: Some(p.tip_body_diameter_mm),
            tip_length_mm: Some(p.tip_length_mm),
            penetration_mm: Some(p.penetration_mm),
            shaft_diameter_mm: Some(p.shaft_diameter_mm),
            shaft_bottom_diameter_mm: p.shaft_bottom_diameter_mm,
            sides: Some(p.sides),
            pad_diameter_mm: Some(p.pad_diameter_mm),
            pad_thickness_mm: Some(p.pad_thickness_mm),
            cone_diameter_mm: Some(p.cone_diameter_mm),
            cone_height_mm: Some(p.cone_height_mm),
            foot_contact_diameter_mm: Some(p.foot_contact_diameter_mm),
            foot_body_diameter_mm: Some(p.foot_body_diameter_mm),
            foot_length_mm: Some(p.foot_length_mm),
            foot_penetration_mm: Some(p.foot_penetration_mm),
            mini_diameter_mm: Some(p.mini_diameter_mm),
            mini_penetration_mm: Some(p.mini_penetration_mm),
        },
        accept_collision: s.accept_collision,
        origin: s.origin,
        kind: s.kind,
    }
}

impl SupportDocument {
    /// An empty document with `preset` and the default checks of a document read without
    /// them: 0.3 mm of clearance, 0.05 mm of anchor tolerance and 60° of axis deviation; no
    /// raft, no binding.
    #[must_use]
    pub fn new(preset: Preset) -> Self {
        Self {
            binding: None,
            preset,
            clearance_mm: 0.3,
            anchor_tolerance_mm: DEFAULT_ANCHOR_TOLERANCE_MM,
            max_axis_deviation_deg: 60.0,
            raft: None,
            supports: Vec::new(),
            braces: Vec::new(),
            warnings: Vec::new(),
        }
    }

    /// A document built in code (the interface edits supports without JSON), checked as
    /// [`Self::from_json`] checks one read: the ranges of the document, the raft, every
    /// parameter and coupling and every brace, nonzero unique IDs, finite coordinates and axes,
    /// known references, and no cycle. Its supports come back parents first and its warnings are
    /// the soft ranges found now. Nothing is clamped or dropped.
    ///
    /// # Errors
    /// The first problem found, naming the support or brace.
    pub fn validated(self) -> Result<Self, SupportError> {
        if self.supports.len() + self.braces.len() > MAX_ENTITIES {
            return Err(invalid(
                0,
                format!("more than {MAX_ENTITIES} supports and braces"),
            ));
        }
        ranged(0, "clearance_mm", self.clearance_mm, [0.0, 5.0])?;
        ranged(
            0,
            "anchor_tolerance_mm",
            self.anchor_tolerance_mm,
            [0.001, 1.0],
        )?;
        ranged(
            0,
            "max_axis_deviation_deg",
            self.max_axis_deviation_deg,
            [0.0, 89.0],
        )?;
        if let Some(r) = self.raft {
            r.checked()?;
        }
        let direction = |id: u64, a: AxisSpec| match a {
            AxisSpec::Direction(d) => axis(id, RawAxis::Direction(d)).map(drop),
            AxisSpec::Normal | AxisSpec::Straight => Ok(()),
        };
        let mut warnings = Vec::new();
        let mut ids = BTreeSet::new();
        for s in &self.supports {
            if s.id == 0 || !ids.insert(s.id) {
                return Err(invalid(
                    s.id,
                    "support IDs must be nonzero and unique".to_owned(),
                ));
            }
            coordinate(s.id, "tip point", &s.tip_mm)?;
            direction(s.id, s.tip_axis)?;
            if s.bends.len() > MAX_BENDS {
                return Err(invalid(s.id, format!("more than {MAX_BENDS} bends")));
            }
            for bend in &s.bends {
                coordinate(s.id, "bend point", bend)?;
            }
            match s.base {
                BaseSpec::Platform(xy) => coordinate(s.id, "platform base", &xy)?,
                BaseSpec::Vertical => {}
                BaseSpec::Model { point_mm, axis: a } => {
                    coordinate(s.id, "model base", &point_mm)?;
                    direction(s.id, a)?;
                }
                BaseSpec::Support { z_mm, .. } => {
                    coordinate(s.id, "support base height", &[z_mm])?;
                }
            }
            mini_base(s.id, s.kind, s.base)?;
            s.parameters.validate(s.id, &mut warnings)?;
        }
        let support_ids: BTreeSet<u64> = self.supports.iter().map(|s| s.id).collect();
        for b in &self.braces {
            if b.id == 0 || !ids.insert(b.id) {
                return Err(invalid(
                    b.id,
                    "brace IDs must be nonzero and unique across supports and braces".to_owned(),
                ));
            }
            let end = |(support, z_mm): (u64, f64)| RawBraceEnd { support, z_mm };
            read_brace(
                &RawBrace {
                    id: b.id,
                    from: end(b.from),
                    to: end(b.to),
                    diameter_mm: b.diameter_mm,
                    max_diameter_mm: b.max_diameter_mm,
                },
                &support_ids,
            )?;
        }
        let supports = parents_first(self.supports)?;
        Ok(Self {
            supports,
            warnings,
            ..self
        })
    }
}

/// One support's validated record with its complete parameters.
fn read_support(
    s: RawSupport,
    preset: Preset,
    defaults: &Overrides,
    warnings: &mut Vec<SupportFinding>,
) -> Result<ManualSupport, SupportError> {
    coordinate(s.id, "tip point", &s.tip.point)?;
    if s.bends.len() > MAX_BENDS {
        return Err(invalid(s.id, format!("more than {MAX_BENDS} bends")));
    }
    for bend in &s.bends {
        coordinate(s.id, "bend point", bend)?;
    }
    let base = match s.base {
        RawBase::Platform(p) => {
            coordinate(s.id, "platform base", &p.platform)?;
            BaseSpec::Platform(p.platform)
        }
        RawBase::Vertical(v) if v.vertical => BaseSpec::Vertical,
        RawBase::Vertical(_) => {
            return Err(invalid(s.id, "\"vertical\" base must be true".to_owned()))
        }
        RawBase::Model(m) => {
            coordinate(s.id, "model base", &m.model)?;
            BaseSpec::Model {
                point_mm: m.model,
                axis: axis(s.id, m.axis)?,
            }
        }
        RawBase::Support(p) => {
            coordinate(s.id, "support base height", &[p.z_mm])?;
            BaseSpec::Support {
                parent: p.support,
                z_mm: p.z_mm,
            }
        }
    };
    mini_base(s.id, s.kind, base)?;
    let mut parameters = preset.parameters();
    defaults.apply(&mut parameters);
    s.parameters.apply(&mut parameters);
    parameters.validate(s.id, warnings)?;
    Ok(ManualSupport {
        id: s.id,
        tip_mm: s.tip.point,
        tip_axis: axis(s.id, s.tip.axis)?,
        bends: s.bends,
        base,
        parameters,
        accept_collision: s.accept_collision,
        origin: s.origin,
        kind: s.kind,
    })
}

/// A mini support stands on the model or on another support: on the plate it would have no pad
/// to hold it.
fn mini_base(entity: u64, kind: SupportKind, base: BaseSpec) -> Result<(), SupportError> {
    match (kind, base) {
        (SupportKind::Mini, BaseSpec::Platform(_) | BaseSpec::Vertical) => Err(invalid(
            entity,
            "a mini support stands on the model or on another support, not on the plate".to_owned(),
        )),
        _ => Ok(()),
    }
}

/// A validated binding.
fn read_binding(b: RawBinding) -> Result<Binding, SupportError> {
    if let Some(hash) = &b.model_sha256 {
        if hash.len() != 64 || !hash.bytes().all(|c| c.is_ascii_hexdigit()) {
            return Err(invalid(
                0,
                "binding model_sha256 must be 64 hexadecimal digits".to_owned(),
            ));
        }
    }
    if let Some(t) = b.translation_mm {
        coordinate(0, "binding translation_mm", &t)?;
    }
    let pose = if b.rotate_deg.is_some() || b.scale.is_some() || b.mirror.is_some() {
        if b.translation_mm.is_none() {
            return Err(invalid(
                0,
                "a binding with rotate_deg, scale or mirror also needs translation_mm".to_owned(),
            ));
        }
        let rotate_deg = b.rotate_deg.unwrap_or([0.0; 3]);
        if !rotate_deg.iter().all(|v| v.is_finite()) {
            return Err(invalid(0, "binding rotate_deg must be finite".to_owned()));
        }
        let scale = b.scale.unwrap_or([1.0; 3]);
        if !scale.iter().all(|s| s.is_finite() && *s > 0.0) {
            return Err(invalid(
                0,
                "binding scale factors must be positive and finite".to_owned(),
            ));
        }
        let mirror =
            crate::plate::mirror_axes(b.mirror.as_deref().unwrap_or("")).ok_or_else(|| {
                invalid(
                    0,
                    "binding mirror must list distinct axes among x, y and z, such as \"xy\""
                        .to_owned(),
                )
            })?;
        Some(BoundPose {
            rotate_deg,
            scale,
            mirror,
        })
    } else {
        None
    };
    Ok(Binding {
        model_sha256: b.model_sha256.map(|h| h.to_ascii_lowercase()),
        translation_mm: b.translation_mm,
        pose,
    })
}

/// One validated brace between known supports.
fn read_brace(b: &RawBrace, support_ids: &BTreeSet<u64>) -> Result<Brace, SupportError> {
    if let Some(diameter) = b.diameter_mm {
        ranged(b.id, "brace diameter_mm", diameter, [0.1, 10.0])?;
    }
    if let Some(largest) = b.max_diameter_mm {
        ranged(b.id, "brace max_diameter_mm", largest, [0.1, 10.0])?;
        if b.diameter_mm.is_some() {
            return Err(invalid(
                b.id,
                "a brace with its own diameter_mm takes no max_diameter_mm".to_owned(),
            ));
        }
    }
    for end in [&b.from, &b.to] {
        if !support_ids.contains(&end.support) {
            return Err(invalid(
                b.id,
                format!("brace refers to unknown support {}", end.support),
            ));
        }
        coordinate(b.id, "brace height", &[end.z_mm])?;
    }
    if b.from.support == b.to.support {
        return Err(invalid(
            b.id,
            "a brace must join two different supports".to_owned(),
        ));
    }
    Ok(Brace {
        id: b.id,
        from: (b.from.support, b.from.z_mm),
        to: (b.to.support, b.to.z_mm),
        diameter_mm: b.diameter_mm,
        max_diameter_mm: b.max_diameter_mm,
    })
}

/// Order supports so that every parent precedes the supports standing on it, keeping document
/// order otherwise (deterministic). Unknown parents and cycles are errors.
fn parents_first(supports: Vec<ManualSupport>) -> Result<Vec<ManualSupport>, SupportError> {
    let index: BTreeMap<u64, usize> = supports
        .iter()
        .enumerate()
        .map(|(i, s)| (s.id, i))
        .collect();
    let parent = |s: &ManualSupport| match s.base {
        BaseSpec::Support { parent, .. } => Some(parent),
        _ => None,
    };
    for s in &supports {
        if let Some(p) = parent(s) {
            if !index.contains_key(&p) {
                return Err(invalid(s.id, format!("base refers to unknown support {p}")));
            }
            if p == s.id {
                return Err(invalid(s.id, "a support cannot stand on itself".to_owned()));
            }
        }
    }
    // Depth of each support along its parent chain; a chain longer than the count is a cycle.
    let mut depth = vec![0usize; supports.len()];
    for (i, s) in supports.iter().enumerate() {
        let mut current = s;
        let mut steps = 0usize;
        while let Some(p) = parent(current) {
            steps += 1;
            if steps > supports.len() {
                return Err(invalid(
                    s.id,
                    "supports standing on supports form a cycle".to_owned(),
                ));
            }
            current = &supports[index[&p]];
        }
        depth[i] = steps;
    }
    let mut order: Vec<usize> = (0..supports.len()).collect();
    order.sort_by_key(|&i| (depth[i], i));
    let mut slots: Vec<Option<ManualSupport>> = supports.into_iter().map(Some).collect();
    Ok(order.into_iter().filter_map(|i| slots[i].take()).collect())
}
