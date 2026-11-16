//! The values supports are made with: the core's presets as weights, the user's own presets,
//! and the next support's values (M6.4g, TD-108), shared by supports placed by hand and the
//! planners.
use karasu_core::supports::{AxisSpec, ContactShape, Preset, SupportParameters};
use serde::{Deserialize, Serialize};

/// The weight of new supports: the core's presets (TD-062).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Weight {
    /// The core's Light preset.
    Light,
    /// The core's Medium preset.
    #[default]
    Medium,
    /// The core's Heavy preset.
    Heavy,
}

impl Weight {
    /// The weights, lightest first.
    pub const ALL: [Self; 3] = [Self::Light, Self::Medium, Self::Heavy];

    /// The core's preset of this weight.
    #[must_use]
    pub fn preset(self) -> Preset {
        match self {
            Self::Light => Preset::Light,
            Self::Medium => Preset::Medium,
            Self::Heavy => Preset::Heavy,
        }
    }
}

/// Every value of a support and how its tip leans: what a preset keeps and what the next
/// support is made with.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SupportValues {
    /// The tip's diameter where it meets the model, in mm.
    pub contact_diameter_mm: f64,
    /// The tip's length from the contact to the tip's join, in mm.
    pub tip_length_mm: f64,
    /// The shaft's diameter at the tip's join, in mm.
    pub shaft_diameter_mm: f64,
    /// How deep the contact reaches below the model's surface, in mm.
    pub penetration_mm: f64,
    /// A ball at the contact, not a flat cone.
    pub sphere_contact: bool,
    /// The contact ball's diameter, in mm.
    pub contact_sphere_diameter_mm: f64,
    /// The tip's diameter where it joins the shaft, in mm.
    pub tip_body_diameter_mm: f64,
    /// A shaft wider at its foot; `None` keeps it straight.
    pub shaft_bottom_diameter_mm: Option<f64>,
    /// The sides of every round part.
    pub sides: u16,
    /// The plate pad's diameter, in mm.
    pub pad_diameter_mm: f64,
    /// The plate pad's thickness, in mm.
    pub pad_thickness_mm: f64,
    /// The diameter of the cone on the pad, in mm.
    pub cone_diameter_mm: f64,
    /// The height of the cone on the pad, in mm.
    pub cone_height_mm: f64,
    /// A foot on the model: its diameter where it meets the model, in mm.
    pub foot_contact_diameter_mm: f64,
    /// A foot on the model: its diameter at its join, in mm.
    pub foot_body_diameter_mm: f64,
    /// A foot on the model: its length, in mm.
    pub foot_length_mm: f64,
    /// A foot on the model: how deep it reaches below the model's surface, in mm.
    pub foot_penetration_mm: f64,
    /// A mini support's diameter and the depth of its ends (owner, 2026-10-30).
    pub mini_diameter_mm: f64,
    /// How deep each end of a mini support reaches below the model's surface, in mm.
    pub mini_penetration_mm: f64,
    /// The tip points at the shaft, not along the surface's normal.
    pub tip_towards_shaft: bool,
}

impl Default for SupportValues {
    fn default() -> Self {
        Self::of(Weight::Medium)
    }
}

impl SupportValues {
    /// The weight's own values, the tip along the surface's normal.
    #[must_use]
    pub fn of(weight: Weight) -> Self {
        Self::with(&weight.preset().parameters(), false)
    }

    /// The values `p`, the tip pointing at the shaft when `tip_towards_shaft`.
    #[must_use]
    pub fn with(p: &SupportParameters, tip_towards_shaft: bool) -> Self {
        Self {
            contact_diameter_mm: p.contact_diameter_mm,
            tip_length_mm: p.tip_length_mm,
            shaft_diameter_mm: p.shaft_diameter_mm,
            penetration_mm: p.penetration_mm,
            sphere_contact: p.contact_shape == ContactShape::Sphere,
            contact_sphere_diameter_mm: p.contact_sphere_diameter_mm,
            tip_body_diameter_mm: p.tip_body_diameter_mm,
            shaft_bottom_diameter_mm: p.shaft_bottom_diameter_mm,
            sides: p.sides,
            pad_diameter_mm: p.pad_diameter_mm,
            pad_thickness_mm: p.pad_thickness_mm,
            cone_diameter_mm: p.cone_diameter_mm,
            cone_height_mm: p.cone_height_mm,
            foot_contact_diameter_mm: p.foot_contact_diameter_mm,
            foot_body_diameter_mm: p.foot_body_diameter_mm,
            foot_length_mm: p.foot_length_mm,
            foot_penetration_mm: p.foot_penetration_mm,
            mini_diameter_mm: p.mini_diameter_mm,
            mini_penetration_mm: p.mini_penetration_mm,
            tip_towards_shaft,
        }
    }

    /// The complete parameters of a support made with these values.
    #[must_use]
    pub fn parameters(&self) -> SupportParameters {
        SupportParameters {
            contact_shape: if self.sphere_contact {
                ContactShape::Sphere
            } else {
                ContactShape::Cone
            },
            contact_diameter_mm: self.contact_diameter_mm,
            contact_fit_min_mm: None,
            contact_sphere_diameter_mm: self.contact_sphere_diameter_mm,
            tip_body_diameter_mm: self.tip_body_diameter_mm,
            tip_length_mm: self.tip_length_mm,
            penetration_mm: self.penetration_mm,
            shaft_diameter_mm: self.shaft_diameter_mm,
            shaft_bottom_diameter_mm: self.shaft_bottom_diameter_mm,
            sides: self.sides,
            pad_diameter_mm: self.pad_diameter_mm,
            pad_thickness_mm: self.pad_thickness_mm,
            cone_diameter_mm: self.cone_diameter_mm,
            cone_height_mm: self.cone_height_mm,
            foot_contact_diameter_mm: self.foot_contact_diameter_mm,
            foot_body_diameter_mm: self.foot_body_diameter_mm,
            foot_length_mm: self.foot_length_mm,
            foot_penetration_mm: self.foot_penetration_mm,
            mini_diameter_mm: self.mini_diameter_mm,
            mini_penetration_mm: self.mini_penetration_mm,
        }
    }

    /// Whether a support can be made with these values (a settings file may keep any): their
    /// ranges and couplings, as the core checks them.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.parameters().validate(0, &mut Vec::new()).is_ok()
    }
}

/// A support preset of the user's own (owner, 2026-10-27): a name and every value, kept with the
/// settings for every project, beside Light, Medium and Heavy.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SupportPreset {
    /// The name the user gave it.
    pub name: String,
    /// Its values.
    pub values: SupportValues,
}

/// What the next support is made with: the preset chosen (a weight, or one of the user's own)
/// and its values as changed in the panel (the four in view, the rest under Advanced). Choosing
/// a preset sets every value to its own. Changed values are for the model at hand (owner,
/// 2026-10-27): the presets keep values.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NextSupport {
    /// The weight chosen, or the one the user's preset was made from.
    pub weight: Weight,
    /// The user's preset chosen, by its place in the list.
    pub mine: Option<usize>,
    /// The values the next support is made with.
    pub values: SupportValues,
    /// The contact fits the surface, narrowing down to this diameter at least (TD-116; owner,
    /// 2026-11-02): the tool's choice, kept whatever preset is chosen, not a preset's value.
    pub contact_fit_min_mm: Option<f64>,
}

impl Default for NextSupport {
    fn default() -> Self {
        Self::of(Weight::Medium)
    }
}

impl NextSupport {
    /// The weight's own values, the tip along the surface's normal.
    #[must_use]
    pub fn of(weight: Weight) -> Self {
        Self {
            weight,
            mine: None,
            values: SupportValues::of(weight),
            contact_fit_min_mm: None,
        }
    }

    /// The user's preset `index` of `presets`, or Medium when there is none.
    #[must_use]
    pub fn of_preset(presets: &[SupportPreset], index: usize) -> Self {
        presets
            .get(index)
            .filter(|p| p.values.is_valid())
            .map_or_else(Self::default, |preset| Self {
                weight: Weight::Medium,
                mine: Some(index),
                values: preset.values,
                contact_fit_min_mm: None,
            })
    }

    /// `preset`'s values, with this one's contact fitting (a preset chosen keeps it).
    #[must_use]
    pub fn chosen(&self, preset: Self) -> Self {
        Self {
            contact_fit_min_mm: self.contact_fit_min_mm,
            ..preset
        }
    }

    /// These values, the contact fitting the surface down to `least` when given.
    #[must_use]
    pub fn fitting(self, least: Option<f64>) -> Self {
        Self {
            contact_fit_min_mm: least,
            ..self
        }
    }

    /// The complete parameters of a support made now: a smallest fitted contact wider than the
    /// contact is the contact.
    #[must_use]
    pub fn parameters(&self) -> SupportParameters {
        fitted(self.values.parameters(), self.contact_fit_min_mm)
    }

    /// The next support's tip axis.
    #[must_use]
    pub fn axis(&self) -> AxisSpec {
        if self.values.tip_towards_shaft {
            AxisSpec::Straight
        } else {
            AxisSpec::Normal
        }
    }
}

/// `parameters` whose contact fits the surface down to `least` when given, at most their
/// contact (the core's coupling).
#[must_use]
pub fn fitted(mut parameters: SupportParameters, least: Option<f64>) -> SupportParameters {
    parameters.contact_fit_min_mm = least.map(|m| m.min(parameters.contact_diameter_mm));
    parameters
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_next_support_keeps_every_value_of_its_weight() {
        for weight in Weight::ALL {
            let next = NextSupport::of(weight);
            assert_eq!(next.parameters(), weight.preset().parameters());
            assert_eq!(next.axis(), AxisSpec::Normal);
            assert!(next.values.is_valid());
        }
    }
}
