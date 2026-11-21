//! The Automatic tool's whole value set, as the desktop application keeps it in its settings
//! file and the command line reads it (TD-157): the plan's values ([`AutoSupports`]), the user's
//! support presets and the one the plan starts from, the Braces tool's values (the braces the
//! plan adds when asked) and the critical angle.
//!
//! **Two documents** are read ([`AutoSettings::from_json`]):
//! - the application's own `settings.json`, recognised by its `schema_version` or its `tools`
//!   object: the values are taken from `tools` exactly as the application takes them when it
//!   starts, its schema's migrations ([`migrate`]) included, and a value the application would
//!   refuse is its built-in one, as there, so the command line plans what the application
//!   plans with the same file;
//! - an object of the command line's own: the [`AutoSupports`] fields as the settings keep them,
//!   with optionally `support_presets`, `auto_preset` and `auto_cavity_preset` (the presets the
//!   plan and the cavity start from, by name), the Braces tool's values under their settings
//!   names (`brace_auto_diameter`, `brace_diameter_mm`, `brace_distance_mm`, `brace_lowest_mm`,
//!   `brace_step_mm`, `brace_pattern`) and `overhang_angle_deg`. Written by hand, it is checked
//!   strictly: an unknown key, a value out of its range, an invalid preset or a preset name that
//!   names none is refused, never replaced.
//!
//! A field left out takes the application's default either way.
use super::{
    auto::{
        AutoSupports, CLEARANCE_MM, DEFAULT_AUTO_SUPPORTS, DENSITY_PCT, LIFT_MM, MINI_DENSITY_PCT,
        PASSES, SPACING_MM, WIDE_CLOSEST_PCT, WIDE_FROM_MM2,
    },
    braces::{NewBraces, Pattern, DEFAULT_BRACES, DIAMETER_MM, DISTANCE_MM, LOWEST_MM, STEP_MM},
    islands::{
        BRANCH_MM, CONTACT_MIN_MM, JOIN_MM, LEVELS, MERGE_MM, MINI_BRANCHES, MINI_MAX_LENGTH_MM,
        MINI_MIN_HEIGHT_MM, PER_GROUP, PER_TRUNK, SKIP_UNDER_MM2,
    },
    values::{SupportPreset, Weight},
};
use serde::Deserialize;
use std::ops::RangeInclusive;

/// The critical angle's default, in degrees from the plate's plane (owner, TD-107): the
/// overhangs' angle, which the Automatic tool starts from.
pub const DEFAULT_OVERHANG_ANGLE_DEG: f64 = 45.0;
/// The critical angles the user may set, in degrees: a surface level with the plate always
/// needs support, and a vertical one never.
pub const OVERHANG_ANGLE_RANGE: RangeInclusive<f64> = 1.0..=89.0;
/// The islands' and the Automatic tool's join distance's default before schema 2, in mm.
pub const OLD_JOIN_MM: f64 = 1.0;
/// The new braces' diameter before schema 4, in mm.
pub const OLD_BRACE_DIAMETER_MM: f64 = 0.8;

/// Bring the Automatic tool's values and the new braces' diameter, read from a settings file of
/// schema `schema`, up to date, as the application reads its file: schema 2 (2026-10-30) turns
/// the join distance from the old default, 1 mm, to none; schema 4 (2026-11-06, TD-136) turns a
/// Light preset to Medium and the braces off, the first defaults, and a diameter still at
/// 0.8 mm to 1.0 mm. A value the user chose otherwise stays.
pub fn migrate(schema: u32, auto: &mut AutoSupports, brace_diameter_mm: &mut f64) {
    #[allow(clippy::float_cmp)] // The old defaults, exactly as they were saved.
    if schema < 2 && auto.join_mm == OLD_JOIN_MM {
        auto.join_mm = 0.0;
    }
    if schema < 4 {
        if auto.weight == Weight::Light {
            auto.weight = DEFAULT_AUTO_SUPPORTS.weight;
        }
        auto.braces = DEFAULT_AUTO_SUPPORTS.braces;
        #[allow(clippy::float_cmp)]
        if *brace_diameter_mm == OLD_BRACE_DIAMETER_MM {
            *brace_diameter_mm = DEFAULT_BRACES.diameter_mm;
        }
    }
}

/// Everything the Automatic tool plans with.
#[derive(Clone, Debug, PartialEq)]
pub struct AutoSettings {
    /// The plan's values; `mine` and `cavity_mine` choose among `presets`.
    pub values: AutoSupports,
    /// The user's support presets.
    pub presets: Vec<SupportPreset>,
    /// The Braces tool's values: the braces the plan adds when `values.braces` asks.
    pub braces: NewBraces,
    /// The critical angle, in degrees from the plate.
    pub angle_deg: f64,
}

impl Default for AutoSettings {
    /// The application's built-in values: Medium, islands and overhangs, minis and branches on,
    /// no braces, lifted 5 mm, at 45 degrees (TD-112, TD-136).
    fn default() -> Self {
        Self {
            values: DEFAULT_AUTO_SUPPORTS,
            presets: Vec::new(),
            braces: DEFAULT_BRACES,
            angle_deg: DEFAULT_OVERHANG_ANGLE_DEG,
        }
    }
}

/// The tools' kept values the Automatic tool plans with, under their settings names; the
/// application's defaults for those left out.
#[derive(Deserialize)]
#[serde(default)]
struct Tools {
    overhang_angle_deg: f64,
    auto_supports: AutoSupports,
    auto_preset: Option<String>,
    auto_cavity_preset: Option<String>,
    support_presets: Vec<SupportPreset>,
    brace_auto_diameter: bool,
    brace_diameter_mm: f64,
    brace_distance_mm: f64,
    brace_lowest_mm: f64,
    brace_step_mm: f64,
    brace_pattern: Pattern,
}

impl Default for Tools {
    fn default() -> Self {
        Self {
            overhang_angle_deg: DEFAULT_OVERHANG_ANGLE_DEG,
            auto_supports: DEFAULT_AUTO_SUPPORTS,
            auto_preset: None,
            auto_cavity_preset: None,
            support_presets: Vec::new(),
            brace_auto_diameter: DEFAULT_BRACES.auto_diameter,
            brace_diameter_mm: DEFAULT_BRACES.diameter_mm,
            brace_distance_mm: DEFAULT_BRACES.max_distance_mm,
            brace_lowest_mm: DEFAULT_BRACES.lowest_mm,
            brace_step_mm: DEFAULT_BRACES.step_mm,
            brace_pattern: DEFAULT_BRACES.pattern,
        }
    }
}

impl Tools {
    fn braces(&self) -> NewBraces {
        NewBraces {
            auto_diameter: self.brace_auto_diameter,
            diameter_mm: self.brace_diameter_mm,
            max_distance_mm: self.brace_distance_mm,
            lowest_mm: self.brace_lowest_mm,
            step_mm: self.brace_step_mm,
            pattern: self.brace_pattern,
        }
    }
}

/// The keys of the command line's own document besides the [`AutoSupports`] fields.
const OWN_KEYS: [&str; 10] = [
    "overhang_angle_deg",
    "auto_preset",
    "auto_cavity_preset",
    "support_presets",
    "brace_auto_diameter",
    "brace_diameter_mm",
    "brace_distance_mm",
    "brace_lowest_mm",
    "brace_step_mm",
    "brace_pattern",
];

impl AutoSettings {
    /// The values `bytes` hold: the application's `settings.json`, read as the application
    /// reads it, or the command line's own object, checked strictly (see the module).
    ///
    /// # Errors
    /// Malformed JSON, a document that is neither, a value of the wrong type; in the command
    /// line's own object, also an unknown key, a value out of its range, an invalid preset or a
    /// preset name that names none.
    pub fn from_json(bytes: &[u8]) -> Result<Self, String> {
        let document: serde_json::Value =
            serde_json::from_slice(bytes).map_err(|e| format!("invalid JSON: {e}"))?;
        let serde_json::Value::Object(object) = document else {
            return Err("expected a JSON object".to_owned());
        };
        if object.contains_key("tools") || object.contains_key("schema_version") {
            Self::from_application(&object)
        } else {
            Self::from_own(object)
        }
    }

    /// The application's settings: see [`Self::from_json`].
    fn from_application(
        object: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<Self, String> {
        let schema = object
            .get("schema_version")
            .and_then(serde_json::Value::as_u64)
            .and_then(|s| u32::try_from(s).ok())
            .ok_or("an application settings file needs its schema_version, a whole number")?;
        let mut tools = match object.get("tools") {
            Some(tools) => Tools::deserialize(tools)
                .map_err(|e| format!("invalid tools in the settings file: {e}"))?,
            None => Tools::default(),
        };
        migrate(
            schema,
            &mut tools.auto_supports,
            &mut tools.brace_diameter_mm,
        );
        // The application leaves out presets a support would refuse, and takes a value a tool
        // would refuse as its built-in one.
        let presets: Vec<SupportPreset> = tools
            .support_presets
            .iter()
            .filter(|p| p.values.is_valid())
            .cloned()
            .collect();
        let position = |name: &Option<String>| {
            name.as_ref()
                .and_then(|name| presets.iter().position(|p| p.name == *name))
        };
        let values = AutoSupports {
            mine: position(&tools.auto_preset),
            cavity_mine: position(&tools.auto_cavity_preset),
            ..AutoSupports {
                mine: None,
                cavity_mine: None,
                ..tools.auto_supports
            }
            .valid()
        };
        Ok(Self {
            values,
            presets,
            braces: tools.braces().valid(),
            angle_deg: if OVERHANG_ANGLE_RANGE.contains(&tools.overhang_angle_deg) {
                tools.overhang_angle_deg
            } else {
                DEFAULT_OVERHANG_ANGLE_DEG
            },
        })
    }

    /// The command line's own object: see [`Self::from_json`].
    fn from_own(object: serde_json::Map<String, serde_json::Value>) -> Result<Self, String> {
        let Ok(serde_json::Value::Object(fields)) = serde_json::to_value(DEFAULT_AUTO_SUPPORTS)
        else {
            return Err("the Automatic tool's values cannot be listed".to_owned());
        };
        let mut auto = serde_json::Map::new();
        let mut rest = serde_json::Map::new();
        for (key, value) in object {
            if fields.contains_key(&key) || key == "inside" {
                auto.insert(key, value);
            } else if OWN_KEYS.contains(&key.as_str()) {
                rest.insert(key, value);
            } else {
                return Err(format!(
                    "unknown key {key:?}: expected the Automatic tool's values ({}), {}, or an \
                     application settings.json",
                    fields.keys().cloned().collect::<Vec<_>>().join(", "),
                    OWN_KEYS.join(", ")
                ));
            }
        }
        let values = AutoSupports::deserialize(serde_json::Value::Object(auto))
            .map_err(|e| format!("invalid value: {e}"))?;
        let tools = Tools::deserialize(serde_json::Value::Object(rest))
            .map_err(|e| format!("invalid value: {e}"))?;
        if let Some(problem) = out_of_range(&values) {
            return Err(problem);
        }
        if let Some(bad) = tools.support_presets.iter().find(|p| !p.values.is_valid()) {
            return Err(format!(
                "support preset {:?} holds a value a support refuses",
                bad.name
            ));
        }
        let braces = tools.braces();
        if braces.valid() != braces {
            return Err(format!(
                "a brace value is out of its range: brace_diameter_mm {}, brace_distance_mm {}, \
                 brace_lowest_mm {}, brace_step_mm {}",
                range(&DIAMETER_MM),
                range(&DISTANCE_MM),
                range(&LOWEST_MM),
                range(&STEP_MM)
            ));
        }
        if !OVERHANG_ANGLE_RANGE.contains(&tools.overhang_angle_deg) {
            return Err(format!(
                "overhang_angle_deg must be {} degrees",
                range(&OVERHANG_ANGLE_RANGE)
            ));
        }
        let position = |key: &str, name: &Option<String>| match name {
            None => Ok(None),
            Some(name) => tools
                .support_presets
                .iter()
                .position(|p| p.name == *name)
                .map(Some)
                .ok_or_else(|| format!("{key} names {name:?}, which support_presets lacks")),
        };
        Ok(Self {
            values: AutoSupports {
                mine: position("auto_preset", &tools.auto_preset)?,
                cavity_mine: position("auto_cavity_preset", &tools.auto_cavity_preset)?,
                ..values
            },
            presets: tools.support_presets,
            braces,
            angle_deg: tools.overhang_angle_deg,
        })
    }
}

/// `low to high`, as a range is said.
fn range(range: &RangeInclusive<f64>) -> String {
    format!("from {} to {}", range.start(), range.end())
}

/// The first of `values` out of its range, said with its range; `None` when every value is in
/// its range (then [`AutoSupports::valid`] keeps them).
fn out_of_range(values: &AutoSupports) -> Option<String> {
    let checks: [(&str, f64, &RangeInclusive<f64>); 21] = [
        ("spacing_mm", values.spacing_mm, &SPACING_MM),
        ("steep_spacing_mm", values.steep_spacing_mm, &SPACING_MM),
        ("lift_mm", values.lift_mm, &LIFT_MM),
        ("density_pct", values.density_pct, &DENSITY_PCT),
        (
            "cavity_density_pct",
            values.cavity_density_pct,
            &DENSITY_PCT,
        ),
        ("wide_from_mm2", values.wide_from_mm2, &WIDE_FROM_MM2),
        (
            "wide_closest_pct",
            values.wide_closest_pct,
            &WIDE_CLOSEST_PCT,
        ),
        ("passes", f64::from(values.passes), &PASSES),
        ("join_mm", values.join_mm, &JOIN_MM),
        ("skip_under_mm2", values.skip_under_mm2, &SKIP_UNDER_MM2),
        ("branch_mm", values.branch_mm, &BRANCH_MM),
        ("per_trunk", f64::from(values.per_trunk), &PER_TRUNK),
        ("merge_mm", values.merge_mm, &MERGE_MM),
        ("per_group", f64::from(values.per_group), &PER_GROUP),
        ("levels", f64::from(values.levels), &LEVELS),
        ("clearance_mm", values.clearance_mm, &CLEARANCE_MM),
        (
            "mini_density_pct",
            values.mini_density_pct,
            &MINI_DENSITY_PCT,
        ),
        (
            "mini_branches",
            f64::from(values.mini_branches),
            &MINI_BRANCHES,
        ),
        (
            "mini_max_length_mm",
            values.mini_max_length_mm,
            &MINI_MAX_LENGTH_MM,
        ),
        (
            "mini_min_height_mm",
            values.mini_min_height_mm,
            &MINI_MIN_HEIGHT_MM,
        ),
        (
            "contact_fit_min_mm",
            values.contact_fit_min_mm.unwrap_or(*CONTACT_MIN_MM.start()),
            &CONTACT_MIN_MM,
        ),
    ];
    for (key, value, allowed) in checks {
        if !allowed.contains(&value) {
            return Some(format!("{key} must be {}, not {value}", range(allowed)));
        }
    }
    if values.mini_min_height_mm > values.mini_max_length_mm {
        return Some(format!(
            "mini_min_height_mm ({}) must be at most mini_max_length_mm ({})",
            values.mini_min_height_mm, values.mini_max_length_mm
        ));
    }
    if values.steep_spacing_mm < values.spacing_mm {
        return Some(format!(
            "steep_spacing_mm ({}) must be at least spacing_mm ({})",
            values.steep_spacing_mm, values.spacing_mm
        ));
    }
    // Every check of the application's own is above: these values are kept as they are.
    debug_assert_eq!(values.valid(), *values);
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::support_plan::auto::{Cover, Place};

    #[test]
    fn the_application_s_settings_are_read_as_the_application_reads_them() {
        // A schema 3 file: Light and braces are the first defaults, turned to Medium and off;
        // the 0.8 mm braces to 1.0 mm. The preset named is found among the valid ones.
        let preset = serde_json::to_value(SupportPreset {
            name: "Fine".to_owned(),
            values: crate::support_plan::values::NextSupport::of(Weight::Light).values,
        })
        .unwrap();
        let file = serde_json::json!({
            "schema_version": 3,
            "appearance": {"anything": true},
            "tools": {
                "overhang_angle_deg": 30.0,
                "auto_supports": {"weight": "light", "braces": true, "density_pct": 150.0,
                                  "cover": "islands-only"},
                "auto_preset": "Fine",
                "support_presets": [preset],
                "brace_diameter_mm": 0.8,
                "unknown_tool_value": 1
            }
        });
        let read = AutoSettings::from_json(file.to_string().as_bytes()).unwrap();
        assert_eq!(read.values.weight, Weight::Medium);
        assert!(!read.values.braces);
        assert_eq!(read.values.cover, Cover::IslandsOnly);
        assert!((read.values.density_pct - 150.0).abs() < 1e-12);
        assert_eq!(read.values.mine, Some(0));
        assert!((read.braces.diameter_mm - 1.0).abs() < 1e-12);
        assert!((read.angle_deg - 30.0).abs() < 1e-12);
        // Values the application would refuse are its built-in ones.
        let refused = serde_json::json!({
            "schema_version": 4,
            "tools": {"auto_supports": {"density_pct": 500.0}, "overhang_angle_deg": 90.0,
                      "brace_step_mm": 0.0}
        });
        let read = AutoSettings::from_json(refused.to_string().as_bytes()).unwrap();
        assert_eq!(read.values, DEFAULT_AUTO_SUPPORTS);
        assert_eq!(read.braces, DEFAULT_BRACES);
        assert!((read.angle_deg - DEFAULT_OVERHANG_ANGLE_DEG).abs() < 1e-12);
        // Without tools: the defaults.
        let read = AutoSettings::from_json(br#"{"schema_version": 4}"#).unwrap();
        assert_eq!(read, AutoSettings::default());
        assert!(AutoSettings::from_json(br#"{"tools": {}}"#).is_err());
    }

    #[test]
    fn the_command_line_s_own_values_are_checked_strictly() {
        let read = AutoSettings::from_json(
            br#"{"place": "outside", "minis": false, "brace_pattern": "cross",
                 "overhang_angle_deg": 40}"#,
        )
        .unwrap();
        assert_eq!(read.values.place, Place::Outside);
        assert!(!read.values.minis);
        assert_eq!(read.braces.pattern, Pattern::Cross);
        assert!((read.angle_deg - 40.0).abs() < 1e-12);
        assert_eq!(read.values.lift_mm, DEFAULT_AUTO_SUPPORTS.lift_mm);
        for (refused, says) in [
            (&br#"{"densty_pct": 120}"#[..], "unknown key \"densty_pct\""),
            (
                br#"{"density_pct": 20}"#,
                "density_pct must be from 50 to 200",
            ),
            (
                br#"{"spacing_mm": 6, "steep_spacing_mm": 5}"#,
                "at least spacing_mm",
            ),
            (
                br#"{"mini_density_pct": 10}"#,
                "mini_density_pct must be from 25 to 100",
            ),
            (
                br#"{"mini_branches": 5}"#,
                "mini_branches must be from 0 to 4",
            ),
            (
                br#"{"mini_max_length_mm": 60}"#,
                "mini_max_length_mm must be from 1 to 50",
            ),
            (
                br#"{"clearance_mm": 6}"#,
                "clearance_mm must be from 0 to 5",
            ),
            (
                br#"{"mini_min_height_mm": 11}"#,
                "mini_min_height_mm must be from 0 to 10",
            ),
            (
                br#"{"mini_min_height_mm": 4, "mini_max_length_mm": 3}"#,
                "at most mini_max_length_mm",
            ),
            (br#"{"auto_preset": "Fine"}"#, "support_presets lacks"),
            (
                br#"{"brace_step_mm": 0}"#,
                "brace value is out of its range",
            ),
            (
                br#"{"overhang_angle_deg": 0}"#,
                "overhang_angle_deg must be",
            ),
            (br#"{"weight": "feather"}"#, "invalid value"),
            (br"[1]", "expected a JSON object"),
        ] {
            let error = AutoSettings::from_json(refused).unwrap_err();
            assert!(error.contains(says), "{error}");
        }
    }
}
