//! Versioned printer/resin profiles with explicit physical units and validated calibration.
//!
//! **Version one** documents are printers and *calibrated resins*: a resin with one calibration
//! per printer identity ([`ResinProfile`]). A print job takes a calibrated resin, and the command
//! line reads them.
//!
//! **The library** (schema version two, TD-106) keeps three kinds apart: printers, resins as
//! materials ([`ResinMaterial`]: no exposure) and print settings ([`PrintSettingProfile`]:
//! exposure, motion and image settings for one printer and one resin). A print setting and its
//! resin compose the calibrated resin a job takes ([`PrintSettingProfile::job_resin`]).
use crate::raster::{LcdProfile, Orientation};
use serde::{Deserialize, Serialize};
use std::{error::Error, fmt};
pub mod catalog;
pub mod store;

/// Schema of version-one documents: printers and calibrated resins.
pub const PROFILE_SCHEMA_VERSION: u32 = 1;
/// Schema of the library's documents (TD-106): printers, resins as materials and print settings.
/// A printer reads either version.
pub const LIBRARY_SCHEMA_VERSION: u32 = 2;
/// Maximum serialized profile bytes accepted by the library.
pub const MAX_PROFILE_BYTES: usize = 1_048_576;

/// Actionable profile validation or storage failure.
#[derive(Debug)]
pub enum ProfileError {
    /// A field violates the versioned profile contract.
    Invalid(String),
    /// A requested profile is absent or already exists.
    Conflict(String),
    /// Filesystem operation and its original cause.
    Io {
        /// Failed operation.
        operation: &'static str,
        /// Underlying filesystem error.
        source: std::io::Error,
    },
    /// Malformed JSON, including unknown and duplicate fields.
    Json(serde_json::Error),
}
impl fmt::Display for ProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(message) | Self::Conflict(message) => f.write_str(message),
            Self::Io { operation, source } => write!(f, "{operation}: {source}"),
            Self::Json(source) => write!(f, "invalid profile JSON: {source}"),
        }
    }
}
impl Error for ProfileError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Json(source) => Some(source),
            _ => None,
        }
    }
}
impl From<serde_json::Error> for ProfileError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}
fn invalid(message: impl Into<String>) -> ProfileError {
    ProfileError::Invalid(message.into())
}
fn text(value: &str, field: &str) -> Result<(), ProfileError> {
    if value.trim().is_empty() || value.len() > 4096 || value.chars().any(char::is_control) {
        return Err(invalid(format!(
            "{field} must contain 1..=4096 bytes of display text"
        )));
    }
    Ok(())
}
/// Validate a portable lowercase profile identifier, independent of its display name.
/// # Errors
/// Rejects traversal, separators, reserved Windows names and IDs longer than 64 bytes.
pub fn validate_profile_id(id: &str) -> Result<(), ProfileError> {
    let reserved = [
        "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
        "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
    ];
    if id.is_empty()
        || id.len() > 64
        || id.starts_with('-')
        || id.ends_with('-')
        || !id
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
        || reserved.contains(&id)
    {
        return Err(invalid(
            "profile ID must be a portable lowercase ASCII slug of 1..=64 bytes",
        ));
    }
    Ok(())
}
fn positive(value: f64, field: &str) -> Result<(), ProfileError> {
    if !value.is_finite() || value <= 0. {
        return Err(invalid(format!("{field} must be finite and positive")));
    }
    Ok(())
}
fn nonnegative(value: f64, field: &str) -> Result<(), ProfileError> {
    if !value.is_finite() || value < 0. {
        return Err(invalid(format!("{field} must be finite and nonnegative")));
    }
    Ok(())
}

/// Explicit engineering limits for a printer; these are not resin calibration values.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrinterLimits {
    /// Inclusive minimum layer thickness in millimeters.
    pub min_layer_height_mm: f64,
    /// Inclusive maximum layer thickness in millimeters.
    pub max_layer_height_mm: f64,
    /// Inclusive maximum exposure and individual wait duration in seconds.
    pub max_exposure_s: f64,
    /// Inclusive maximum linear motion speed in millimeters per second.
    pub max_speed_mm_s: f64,
    /// Inclusive maximum lift travel in millimeters.
    pub max_lift_mm: f64,
}
impl PrinterLimits {
    /// Validate finite positive bounds and their ordering.
    /// # Errors
    /// Rejects nonfinite, zero, negative or inverted bounds.
    pub fn validate(&self) -> Result<(), ProfileError> {
        for (v, field) in [
            (self.min_layer_height_mm, "minimum layer height"),
            (self.max_layer_height_mm, "maximum layer height"),
            (self.max_exposure_s, "maximum exposure"),
            (self.max_speed_mm_s, "maximum motion speed"),
            (self.max_lift_mm, "maximum lift"),
        ] {
            positive(v, field)?;
        }
        if self.min_layer_height_mm > self.max_layer_height_mm {
            return Err(invalid("minimum layer height exceeds maximum"));
        }
        Ok(())
    }
}
/// Version-one printer hardware and explicitly selected target-format identifiers.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrinterProfile {
    /// Must equal [`PROFILE_SCHEMA_VERSION`].
    pub schema_version: u32,
    /// Stable portable identity.
    pub id: String,
    /// User-facing name.
    pub name: String,
    /// Monotonically increasing user revision, starting at one.
    pub revision: u64,
    /// World X, Y and Z build dimensions in millimeters.
    pub build_size_mm: [f64; 3],
    /// Output width and height in pixels.
    pub resolution_px: [u32; 2],
    /// World XY minimum of the LCD; no implicit centering.
    pub origin_xy_mm: [f64; 2],
    /// Exchange world X and Y before output reflections.
    pub swap_xy: bool,
    /// Reflect output columns.
    pub mirror_x: bool,
    /// Reflect output rows.
    pub mirror_y: bool,
    /// Format/version IDs, checked by the selected export adapter.
    pub format_ids: Vec<String>,
    /// Explicit machine constraints.
    pub limits: PrinterLimits,
    /// Catalog vendor identity (TD-106), or `None` for a printer of no listed vendor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vendor: Option<String>,
    /// The manufacturer's model name, such as "Anycubic Photon P1", or `None` for a custom
    /// printer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// The catalog entry this printer was copied from, or `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub based_on: Option<CatalogRef>,
    /// Seconds the printer spends on each layer besides its exposure, rests and motion, such as
    /// preparing the next image (TD-122); added to the time estimate. `None` counts none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fixed_layer_time_s: Option<f64>,
}
/// The fixed times per layer a printer may state, in seconds (TD-122).
pub const FIXED_LAYER_TIME_S: std::ops::RangeInclusive<f64> = 0.0..=10.0;
impl PrinterProfile {
    /// Validate metadata, raster hardware and limits.
    /// # Errors
    /// Rejects invalid schema, metadata, raster domain or physical constraints.
    pub fn validate(&self) -> Result<(), ProfileError> {
        metadata(
            self.schema_version,
            &[PROFILE_SCHEMA_VERSION, LIBRARY_SCHEMA_VERSION],
            &self.id,
            &self.name,
            self.revision,
        )?;
        if let Some(vendor) = &self.vendor {
            validate_profile_id(vendor)?;
        }
        if let Some(model) = &self.model {
            text(model, "printer model")?;
        }
        if let Some(based_on) = &self.based_on {
            based_on.validate()?;
        }
        for v in self.build_size_mm {
            positive(v, "build dimension")?;
        }
        self.limits.validate()?;
        if self.limits.max_layer_height_mm > self.build_size_mm[2]
            || self.limits.max_lift_mm > self.build_size_mm[2]
        {
            return Err(invalid("printer limits exceed the Z build dimension"));
        }
        if self.format_ids.is_empty() || self.format_ids.len() > 64 {
            return Err(invalid("printer requires 1..=64 format IDs"));
        }
        if let Some(seconds) = self.fixed_layer_time_s {
            if !FIXED_LAYER_TIME_S.contains(&seconds) {
                return Err(invalid(format!(
                    "fixed time per layer must be within {} to {} s",
                    FIXED_LAYER_TIME_S.start(),
                    FIXED_LAYER_TIME_S.end()
                )));
            }
        }
        let mut ids = std::collections::BTreeSet::new();
        for id in &self.format_ids {
            text(id, "format ID")?;
            if !ids.insert(id) {
                return Err(invalid("duplicate format ID"));
            }
        }
        self.lcd_profile()?;
        Ok(())
    }
    /// Construct the M4 LCD contract with explicit origin and orientation.
    /// # Errors
    /// Rejects unsupported raster dimensions or numerical domain.
    pub fn lcd_profile(&self) -> Result<LcdProfile, ProfileError> {
        LcdProfile::try_new(
            self.resolution_px,
            self.origin_xy_mm,
            [self.build_size_mm[0], self.build_size_mm[1]],
            Orientation {
                swap_xy: self.swap_xy,
                flip_x: self.mirror_x,
                flip_y: self.mirror_y,
            },
        )
        .map_err(|source| invalid(format!("invalid LCD hardware: {source}")))
    }
}
fn metadata(
    version: u32,
    versions: &[u32],
    id: &str,
    name: &str,
    revision: u64,
) -> Result<(), ProfileError> {
    if !versions.contains(&version) {
        return Err(invalid(format!(
            "unsupported profile schema version {version}"
        )));
    }
    validate_profile_id(id)?;
    text(name, "profile name")?;
    if revision == 0 {
        return Err(invalid("profile revision must start at one"));
    }
    Ok(())
}
/// One positive-distance linear movement with speed in millimeters per second.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MotionStage {
    /// Positive distance in millimeters.
    pub distance_mm: f64,
    /// Positive speed in millimeters per second.
    pub speed_mm_s: f64,
}
/// Explicit exposure, waits and platform motion for a layer class.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayerSettings {
    /// Exposure duration in seconds.
    pub exposure_s: f64,
    /// Rest after retract and before exposure in seconds.
    pub wait_before_exposure_s: f64,
    /// Rest after exposure and before lifting in seconds.
    pub wait_after_exposure_s: f64,
    /// Rest at the top of the lift in seconds.
    pub wait_after_lift_s: f64,
    /// Light power, 1..=255.
    pub pwm: u8,
    /// One or two lift stages.
    pub lift: Vec<MotionStage>,
    /// One or two retract stages; total distance must equal lift travel.
    pub retract: Vec<MotionStage>,
}
impl LayerSettings {
    /// Validate physical settings, optionally against an explicit machine.
    /// # Errors
    /// Rejects invalid timings, zero power, inconsistent travel and exceeded limits.
    pub fn validate(&self, limits: Option<&PrinterLimits>) -> Result<(), ProfileError> {
        positive(self.exposure_s, "layer exposure")?;
        for v in [
            self.wait_before_exposure_s,
            self.wait_after_exposure_s,
            self.wait_after_lift_s,
        ] {
            nonnegative(v, "layer wait")?;
        }
        if self.pwm == 0 {
            return Err(invalid("light PWM must be in 1..=255"));
        }
        for stages in [&self.lift, &self.retract] {
            if !(1..=2).contains(&stages.len()) {
                return Err(invalid(
                    "lift/retract must have one or two positive-distance stages",
                ));
            }
            for stage in stages {
                positive(stage.distance_mm, "motion distance")?;
                positive(stage.speed_mm_s, "motion speed")?;
                positive(stage.distance_mm / stage.speed_mm_s, "motion duration")?;
            }
        }
        let lift = self.lift_distance_mm();
        let retract: f64 = self.retract.iter().map(|s| s.distance_mm).sum();
        // Explicit relative tolerance accommodates summation of independently entered stage distances.
        if !lift.is_finite()
            || !retract.is_finite()
            || (lift - retract).abs() > 1e-9 * lift.max(retract)
        {
            return Err(invalid(
                "lift and retract total distances must agree within relative tolerance 1e-9",
            ));
        }
        if let Some(limits) = limits {
            limits.validate()?;
            if [
                self.exposure_s,
                self.wait_before_exposure_s,
                self.wait_after_exposure_s,
                self.wait_after_lift_s,
            ]
            .iter()
            .any(|v| *v > limits.max_exposure_s)
                || lift > limits.max_lift_mm
                || self
                    .lift
                    .iter()
                    .chain(&self.retract)
                    .any(|s| s.speed_mm_s > limits.max_speed_mm_s)
            {
                return Err(invalid(
                    "layer exposure, wait or motion exceeds printer limits",
                ));
            }
        }
        Ok(())
    }
    /// Total positive lift distance in millimeters.
    #[must_use]
    pub fn lift_distance_mm(&self) -> f64 {
        self.lift.iter().map(|stage| stage.distance_mm).sum()
    }
}
/// Complete calibration settings; no implicit material or motion defaults.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrintSettings {
    /// Positive thickness in millimeters.
    pub layer_height_mm: f64,
    /// Number of initial bottom layers.
    pub bottom_layer_count: u32,
    /// Number of exposure-only interpolated layers after the bottom block.
    pub transition_layer_count: u32,
    /// Bottom-layer exposure and motion.
    pub bottom: LayerSettings,
    /// Normal-layer exposure and motion; also used by transition layers except exposure.
    pub normal: LayerSettings,
    /// The vat's heating target in °C, for printers that heat it (TD-118); `None` when not
    /// stated, and formats that record one then write [`DEFAULT_HEATING_C`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heating_temperature_c: Option<f64>,
}
/// The heating target a print setting starts from, and a format writes when none is stated, in
/// °C: the value Photon Workshop, Lychee and CHITUBOX all write for the P1 (TD-118).
pub const DEFAULT_HEATING_C: f64 = 25.0;
/// The heating targets a print setting may state, in °C.
pub const HEATING_C: std::ops::RangeInclusive<f64> = 10.0..=60.0;
impl PrintSettings {
    /// Validate complete calibration and optional printer limits.
    /// # Errors
    /// Rejects invalid physical values, counts or machine limits.
    pub fn validate(&self, limits: Option<&PrinterLimits>) -> Result<(), ProfileError> {
        positive(self.layer_height_mm, "layer height")?;
        self.bottom_layer_count
            .checked_add(self.transition_layer_count)
            .ok_or_else(|| invalid("bottom and transition layer counts overflow"))?;
        if self.bottom_layer_count == 0 && self.transition_layer_count != 0 {
            return Err(invalid(
                "transition layers require at least one bottom layer",
            ));
        }
        self.bottom.validate(limits)?;
        self.normal.validate(limits)?;
        if let Some(celsius) = self.heating_temperature_c {
            if !HEATING_C.contains(&celsius) {
                return Err(invalid(format!(
                    "heating temperature must be within {} to {} °C",
                    HEATING_C.start(),
                    HEATING_C.end()
                )));
            }
        }
        if let Some(limits) = limits {
            if self.layer_height_mm < limits.min_layer_height_mm
                || self.layer_height_mm > limits.max_layer_height_mm
            {
                return Err(invalid("layer height exceeds printer limits"));
            }
        }
        Ok(())
    }
}
/// A complete resin calibration tied to one printer identity.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrinterCalibration {
    /// Target printer profile ID.
    pub printer_id: String,
    /// Explicit calibrated settings.
    pub settings: PrintSettings,
}
/// Version-one user resin identity and per-printer calibrations.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResinProfile {
    /// Must equal [`PROFILE_SCHEMA_VERSION`].
    pub schema_version: u32,
    /// Stable portable identity.
    pub id: String,
    /// User-facing resin name.
    pub name: String,
    /// Monotonically increasing revision, starting at one.
    pub revision: u64,
    /// Optional positive density in grams per milliliter.
    pub density_g_per_ml: Option<f64>,
    /// Optional nonnegative cost per liter.
    pub cost_per_liter: Option<f64>,
    /// Optional three-letter uppercase currency code, required with cost.
    pub currency: Option<String>,
    /// Explicit printer-specific calibrations, with unique printer IDs.
    pub calibrations: Vec<PrinterCalibration>,
}
/// Resin needed for a volume of cured material, from a resin profile's optional properties.
///
/// The volume is the caller's estimate of cured material (for exports, the sampled mask volume,
/// where gray pixels count by their intensity). Waste, vat residue and failed layers are not
/// included.
#[derive(Clone, Debug, PartialEq)]
pub struct ResinUsage {
    /// Volume in milliliters.
    pub volume_ml: f64,
    /// Mass in grams, when the profile has a density.
    pub mass_g: Option<f64>,
    /// Cost in the profile's currency, with its code, when the profile has a cost.
    pub cost: Option<(f64, String)>,
}
impl ResinProfile {
    /// Resin needed for `volume_mm3` cubic millimeters of cured material.
    #[must_use]
    pub fn usage(&self, volume_mm3: f64) -> ResinUsage {
        let volume_ml = volume_mm3 / 1000.0;
        ResinUsage {
            volume_ml,
            mass_g: self.density_g_per_ml.map(|density| volume_ml * density),
            cost: self
                .cost_per_liter
                .zip(self.currency.clone())
                .map(|(per_liter, currency)| (volume_ml / 1000.0 * per_liter, currency)),
        }
    }
    /// Validate metadata, material properties and every calibration.
    /// # Errors
    /// Rejects unsupported schema, invalid values, missing or duplicate calibrations.
    pub fn validate(&self) -> Result<(), ProfileError> {
        metadata(
            self.schema_version,
            &[PROFILE_SCHEMA_VERSION],
            &self.id,
            &self.name,
            self.revision,
        )?;
        material(
            self.density_g_per_ml,
            self.cost_per_liter,
            self.currency.as_deref(),
        )?;
        if self.calibrations.is_empty() || self.calibrations.len() > 1024 {
            return Err(invalid("resin requires 1..=1024 printer calibrations"));
        }
        let mut ids = std::collections::BTreeSet::new();
        for c in &self.calibrations {
            validate_profile_id(&c.printer_id)?;
            if !ids.insert(&c.printer_id) {
                return Err(invalid("duplicate printer calibration"));
            }
            c.settings.validate(None)?;
        }
        Ok(())
    }
    /// Return a validated matching calibration without falling back to another machine.
    /// # Errors
    /// Rejects invalid profiles or absent printer-specific calibration.
    pub fn calibration_for(&self, printer_id: &str) -> Result<&PrintSettings, ProfileError> {
        self.validate()?;
        self.calibrations
            .iter()
            .find(|c| c.printer_id == printer_id)
            .map(|c| &c.settings)
            .ok_or_else(|| invalid(format!("resin has no calibration for printer {printer_id}")))
    }
}

/// Validate a resin's optional density, cost and currency.
fn material(
    density_g_per_ml: Option<f64>,
    cost_per_liter: Option<f64>,
    currency: Option<&str>,
) -> Result<(), ProfileError> {
    if let Some(d) = density_g_per_ml {
        positive(d, "resin density")?;
    }
    if let Some(cost) = cost_per_liter {
        nonnegative(cost, "resin cost")?;
    }
    if cost_per_liter.is_some() != currency.is_some()
        || currency.is_some_and(|c| c.len() != 3 || !c.bytes().all(|b| b.is_ascii_uppercase()))
    {
        return Err(invalid(
            "resin cost requires a three-letter uppercase currency code and vice versa",
        ));
    }
    Ok(())
}

/// The catalog entry a library profile was copied from (TD-106): its identity and the revision
/// copied, so a newer catalog revision can be offered.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogRef {
    /// Catalog identity.
    pub id: String,
    /// Catalog revision copied, starting at one.
    pub revision: u64,
}
impl CatalogRef {
    /// Validate the identity and revision.
    /// # Errors
    /// Rejects an invalid identity or a zero revision.
    pub fn validate(&self) -> Result<(), ProfileError> {
        validate_profile_id(&self.id)?;
        if self.revision == 0 {
            return Err(invalid("catalog revision must start at one"));
        }
        Ok(())
    }
}

/// What a resin is made for, as resin makers group their products.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResinKind {
    /// General-purpose resin.
    Standard,
    /// Tougher, less brittle resin sold as ABS-like.
    AbsLike,
    /// Resin washed in water instead of alcohol.
    WaterWashable,
    /// Impact-resistant engineering resin.
    Tough,
    /// Flexible or elastic resin.
    Flexible,
    /// Resin from plant-based oils.
    PlantBased,
    /// Clear resin.
    Clear,
    /// Resin for investment casting.
    Castable,
    /// Any other resin.
    Other,
}

/// A resin as a material (TD-106): who makes it and what it costs, with no exposure settings,
/// which belong to print settings.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResinMaterial {
    /// Must equal [`LIBRARY_SCHEMA_VERSION`].
    pub schema_version: u32,
    /// Stable portable identity.
    pub id: String,
    /// User-facing resin name.
    pub name: String,
    /// Monotonically increasing revision, starting at one.
    pub revision: u64,
    /// Catalog vendor identity, such as `anycubic`, or `None` for a resin of no listed vendor.
    pub vendor: Option<String>,
    /// What the resin is made for, or `None` when not stated.
    pub kind: Option<ResinKind>,
    /// Optional positive density in grams per milliliter.
    pub density_g_per_ml: Option<f64>,
    /// Optional nonnegative cost per liter.
    pub cost_per_liter: Option<f64>,
    /// Optional three-letter uppercase currency code, required with cost.
    pub currency: Option<String>,
    /// The catalog entry this resin was copied from, or `None`.
    pub based_on: Option<CatalogRef>,
    /// The resin's colour as its maker names it, such as "Grey" (TD-118): resins of one kind in
    /// different colours are different resins, each with its own print settings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub colour: Option<String>,
    /// A swatch of that colour, `#RRGGBB`, shown beside the resin's name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub swatch: Option<String>,
}
/// The red, green and blue of a `#RRGGBB` swatch, or `None` when it is not one.
#[must_use]
pub fn swatch_rgb(swatch: &str) -> Option<[u8; 3]> {
    let hex = swatch.strip_prefix('#')?;
    if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let channel = |k: usize| u8::from_str_radix(&hex[2 * k..2 * k + 2], 16).ok();
    Some([channel(0)?, channel(1)?, channel(2)?])
}
impl ResinMaterial {
    /// Validate metadata and material properties.
    /// # Errors
    /// Rejects an unsupported schema, invalid identity, text or material values.
    pub fn validate(&self) -> Result<(), ProfileError> {
        metadata(
            self.schema_version,
            &[LIBRARY_SCHEMA_VERSION],
            &self.id,
            &self.name,
            self.revision,
        )?;
        if let Some(vendor) = &self.vendor {
            validate_profile_id(vendor)?;
        }
        if let Some(based_on) = &self.based_on {
            based_on.validate()?;
        }
        if let Some(colour) = &self.colour {
            text(colour, "resin colour")?;
        }
        if self
            .swatch
            .as_deref()
            .is_some_and(|s| swatch_rgb(s).is_none())
        {
            return Err(invalid("a resin swatch must be #RRGGBB"));
        }
        material(
            self.density_g_per_ml,
            self.cost_per_liter,
            self.currency.as_deref(),
        )
    }
}

/// Largest XY compensation radius, in pixels, a print setting may ask for.
pub const MAX_COMPENSATION_PX: i16 = 64;

/// How a print setting turns layer sections into mask pixels (TD-106; plate settings before).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageSettings {
    /// Mask samples per pixel axis: 1 (binary masks), 2, 4, 8 or 16.
    pub antialias: u8,
    /// Smallest gray of a partially covered pixel; 0 keeps gray proportional to coverage.
    pub min_gray: u8,
    /// Signed XY compensation of every layer, in pixels (positive grows material).
    pub xy_compensation_px: i16,
    /// Signed XY compensation of the bottom layers instead, in pixels, when given.
    pub bottom_xy_compensation_px: Option<i16>,
}
impl Default for ImageSettings {
    /// Binary masks with no compensation, as the command line slices by default.
    fn default() -> Self {
        Self {
            antialias: 1,
            min_gray: 0,
            xy_compensation_px: 0,
            bottom_xy_compensation_px: None,
        }
    }
}
impl ImageSettings {
    /// Validate the sampling, gray floor and compensation radii.
    /// # Errors
    /// Rejects an unsupported sample count, a gray floor without antialiasing, or a radius
    /// beyond [`MAX_COMPENSATION_PX`].
    pub fn validate(&self) -> Result<(), ProfileError> {
        if !matches!(self.antialias, 1 | 2 | 4 | 8 | 16) {
            return Err(invalid(
                "antialiasing must be 1, 2, 4, 8 or 16 samples per axis",
            ));
        }
        if self.min_gray > 0 && self.antialias == 1 {
            return Err(invalid(
                "a minimum gray applies to antialiased edges: it requires antialiasing",
            ));
        }
        for pixels in std::iter::once(self.xy_compensation_px).chain(self.bottom_xy_compensation_px)
        {
            if pixels.abs() > MAX_COMPENSATION_PX {
                return Err(invalid(format!(
                    "XY compensation must be within {MAX_COMPENSATION_PX} pixels"
                )));
            }
        }
        Ok(())
    }
}

/// Where a print setting's values come from (TD-106).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SettingStatus {
    /// A manufacturer's values, as the catalog publishes them.
    Official,
    /// Copied from another resin's or printer's setting, to be calibrated.
    StartingPoint,
    /// Edited or entered by the user.
    User,
}

/// Exposure, motion and image settings for one printer and one resin (TD-106).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrintSettingProfile {
    /// Must equal [`LIBRARY_SCHEMA_VERSION`].
    pub schema_version: u32,
    /// Stable portable identity.
    pub id: String,
    /// User-facing name, such as "Normal 0.05 mm".
    pub name: String,
    /// Monotonically increasing revision, starting at one.
    pub revision: u64,
    /// The printer's identity.
    pub printer_id: String,
    /// The resin's identity.
    pub resin_id: String,
    /// The release film it was set for, such as "NFEP", or `None` when not stated.
    pub film: Option<String>,
    /// Where the values come from.
    pub status: SettingStatus,
    /// Where they were published or copied from, for display, or `None`.
    pub source: Option<String>,
    /// The catalog entry this setting was copied from, or `None`.
    pub based_on: Option<CatalogRef>,
    /// Layers, exposure, rests and motion.
    pub settings: PrintSettings,
    /// Masks and compensation.
    pub image: ImageSettings,
}
impl PrintSettingProfile {
    /// Validate metadata, references and every setting, without a printer's limits.
    /// # Errors
    /// Rejects an unsupported schema, invalid identities, text or settings.
    pub fn validate(&self) -> Result<(), ProfileError> {
        metadata(
            self.schema_version,
            &[LIBRARY_SCHEMA_VERSION],
            &self.id,
            &self.name,
            self.revision,
        )?;
        validate_profile_id(&self.printer_id)?;
        validate_profile_id(&self.resin_id)?;
        for (value, field) in [
            (&self.film, "release film"),
            (&self.source, "setting source"),
        ] {
            if let Some(value) = value {
                text(value, field)?;
            }
        }
        if let Some(based_on) = &self.based_on {
            based_on.validate()?;
        }
        self.settings.validate(None)?;
        self.image.validate()
    }
    /// The calibrated resin a print job takes: `resin`'s identity and material with this
    /// setting as its only calibration, for this setting's printer.
    /// # Errors
    /// Rejects a resin other than this setting's, or invalid profiles.
    pub fn job_resin(&self, resin: &ResinMaterial) -> Result<ResinProfile, ProfileError> {
        self.validate()?;
        resin.validate()?;
        if resin.id != self.resin_id {
            return Err(invalid(format!(
                "print setting {} is for resin {}, not {}",
                self.id, self.resin_id, resin.id
            )));
        }
        let job = ResinProfile {
            schema_version: PROFILE_SCHEMA_VERSION,
            id: resin.id.clone(),
            name: resin.name.clone(),
            revision: resin.revision,
            density_g_per_ml: resin.density_g_per_ml,
            cost_per_liter: resin.cost_per_liter,
            currency: resin.currency.clone(),
            calibrations: vec![PrinterCalibration {
                printer_id: self.printer_id.clone(),
                settings: self.settings.clone(),
            }],
        };
        job.validate()?;
        Ok(job)
    }
}
