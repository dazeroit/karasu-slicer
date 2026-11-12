//! Variable and adaptive layer heights (TD-064).
//!
//! A versioned `karasu-layers` JSON document divides the print into Z ranges from the plate up.
//! Each range has either a fixed layer height or an adaptive rule.
//!
//! [`plan_layers`] turns the document into per-layer heights in integer nanometres, which feed
//! [`crate::slicing::schedule::build_slab_schedule`], and into explicit per-layer settings for
//! every layer whose height differs from the resin calibration. Rules:
//! - Exposure is never inferred: a range states it, or the document's height-to-exposure table
//!   is interpolated between its points; heights outside the table are refused.
//! - Bottom and transition layers keep the calibrated height.
//! - **Range boundaries (carry policy):** a layer takes the height of the range containing its
//!   bottom, so the first layer of a new range starts at the first slab bottom at or above the
//!   boundary.
//! - **Adaptive heights:** the classic stair-step bound `h ≤ c / |n_z|` over the non-horizontal
//!   faces near each layer, clamped to `[min, max]` and floored to the Z step. Growth between
//!   consecutive layers is limited by `max_ratio`, while decreases are immediate.
use crate::{
    cancellation::{Cancellation, Cancelled, Checkpoint},
    mesh::TriangleMesh,
    print_job::LayerOverride,
    profiles::PrintSettings,
};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Document format name.
pub const FORMAT: &str = "karasu-layers";
/// Supported document version.
pub const VERSION: u32 = 1;
/// Largest accepted document, in bytes.
pub const MAX_DOCUMENT_BYTES: usize = 1024 * 1024;
/// Most ranges in one document.
const MAX_RANGES: usize = 1024;
/// Most points in the exposure table.
const MAX_EXPOSURES: usize = 256;
/// Nanometres per millimetre.
const NM_PER_MM: f64 = 1e6;

/// A layer-plan document or plan that cannot be used.
#[derive(Debug)]
pub enum LayerPlanError {
    /// Malformed JSON or an unknown field.
    Json(String),
    /// A value that violates the schema or cannot be planned.
    Invalid(String),
    /// Cooperative cancellation.
    Cancelled(Cancelled),
}

impl fmt::Display for LayerPlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(e) | Self::Invalid(e) => write!(f, "invalid layer plan: {e}"),
            Self::Cancelled(c) => write!(f, "{c}"),
        }
    }
}

impl std::error::Error for LayerPlanError {}

impl From<Cancelled> for LayerPlanError {
    fn from(c: Cancelled) -> Self {
        Self::Cancelled(c)
    }
}

fn invalid(reason: impl Into<String>) -> LayerPlanError {
    LayerPlanError::Invalid(reason.into())
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RawAdaptive {
    min_height_mm: f64,
    max_height_mm: f64,
    max_deviation_mm: f64,
    #[serde(default = "default_ratio")]
    max_growth_ratio: f64,
}

const fn default_ratio() -> f64 {
    1.5
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RawRange {
    from_mm: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    height_mm: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    exposure_s: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    adaptive: Option<RawAdaptive>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RawExposure {
    height_mm: f64,
    exposure_s: f64,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RawDocument {
    format: String,
    version: u32,
    #[serde(default = "default_step")]
    z_step_mm: f64,
    ranges: Vec<RawRange>,
    #[serde(default)]
    exposures: Vec<RawExposure>,
}

const fn default_step() -> f64 {
    0.001
}

/// An adaptive height rule.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AdaptiveRule {
    /// Smallest layer height, in nanometres (a multiple of the Z step).
    pub min_nm: u64,
    /// Largest layer height, in nanometres (a multiple of the Z step).
    pub max_nm: u64,
    /// Largest stair-step deviation `c`, in millimetres.
    pub max_deviation_mm: f64,
    /// Largest ratio between a layer's height and the previous layer's.
    pub max_growth_ratio: f64,
}

impl AdaptiveRule {
    /// The height of a layer starting at `bottom_nm` after a layer `previous_nm` thick: the
    /// slope bound, limited by growth and the maximum, floored to the step and clamped.
    fn height(
        &self,
        slopes: Option<&SlopeIndex>,
        bottom_nm: u64,
        previous_nm: Option<u64>,
        step_nm: u64,
    ) -> u64 {
        let bound = slopes.map_or(f64::INFINITY, |s| {
            s.bound(bottom_nm, self.max_nm, self.max_deviation_mm)
        });
        #[allow(clippy::cast_precision_loss)] // Heights below 2^31 nm.
        let grown = previous_nm.map_or(f64::INFINITY, |p| p as f64 * self.max_growth_ratio);
        #[allow(clippy::cast_precision_loss)] // Heights below 2^31 nm.
        let wanted = bound.min(grown).min(self.max_nm as f64);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // At most `max_nm`.
        let floored = (wanted.max(0.) as u64) / step_nm * step_nm;
        floored.clamp(self.min_nm, self.max_nm)
    }
}

/// How one range chooses its heights.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RangeHeight {
    /// One fixed height in nanometres.
    Fixed(u64),
    /// Heights from the model's slopes.
    Adaptive(AdaptiveRule),
}

/// One Z range, from `from_nm` to the next range's start.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HeightRange {
    /// Range start in nanometres; the first range starts at zero.
    pub from_nm: u64,
    /// Height rule.
    pub height: RangeHeight,
    /// Explicit exposure of the range's layers, overriding the table.
    pub exposure_s: Option<f64>,
}

/// A validated `karasu-layers` document.
#[derive(Clone, Debug, PartialEq)]
pub struct LayerPlanDocument {
    /// Z step in nanometres: every height is a multiple of it.
    pub z_step_nm: u64,
    /// Ranges in increasing start order, the first at zero.
    pub ranges: Vec<HeightRange>,
    /// Height-to-exposure points, increasing in height: `(height nm, exposure s)`.
    pub exposures: Vec<(u64, f64)>,
}

/// Millimetres to whole nanometres, refusing values that are not whole nanometres.
fn to_nm(mm: f64, what: &str) -> Result<u64, LayerPlanError> {
    let nm = mm * NM_PER_MM;
    let rounded = nm.round();
    if !mm.is_finite() || mm < 0. || (nm - rounded).abs() > 1e-6 * rounded.max(1.) || rounded > 1e9
    {
        return Err(invalid(format!(
            "{what} {mm} mm is not a whole number of nanometres within 1 m"
        )));
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // Checked range.
    Ok(rounded as u64)
}

impl LayerPlanDocument {
    /// Parse and validate a `karasu-layers` document.
    ///
    /// # Errors
    /// An oversized document, malformed JSON, unknown fields, a wrong format or version, ranges that are unordered, do
    /// not start at zero or mix fixed and adaptive rules, heights off the Z step, and exposure
    /// tables that are unordered or non-positive.
    pub fn from_json(bytes: &[u8]) -> Result<Self, LayerPlanError> {
        if bytes.len() > MAX_DOCUMENT_BYTES {
            return Err(invalid(format!(
                "document exceeds {MAX_DOCUMENT_BYTES} bytes"
            )));
        }
        let raw: RawDocument =
            serde_json::from_slice(bytes).map_err(|e| LayerPlanError::Json(e.to_string()))?;
        if raw.format != FORMAT || raw.version != VERSION {
            return Err(invalid(format!(
                "document is {:?} version {}; expected {FORMAT:?} version {VERSION}",
                raw.format, raw.version
            )));
        }
        let step = to_nm(raw.z_step_mm, "z_step_mm")?;
        if step == 0 {
            return Err(invalid("z_step_mm must be positive"));
        }
        let height = |mm: f64, what: &str| -> Result<u64, LayerPlanError> {
            let nm = to_nm(mm, what)?;
            if nm == 0 || nm % step != 0 || nm > 1_000_000 {
                return Err(invalid(format!(
                    "{what} {mm} mm must be positive, at most 1 mm and a multiple of the {} mm Z step",
                    raw.z_step_mm
                )));
            }
            Ok(nm)
        };
        if raw.ranges.is_empty() || raw.ranges.len() > MAX_RANGES {
            return Err(invalid(format!(
                "between 1 and {MAX_RANGES} ranges are required"
            )));
        }
        let mut ranges = Vec::with_capacity(raw.ranges.len());
        for (i, r) in raw.ranges.iter().enumerate() {
            let from_nm = to_nm(r.from_mm, "range from_mm")?;
            if i == 0 && from_nm != 0 {
                return Err(invalid("the first range must start at 0 mm"));
            }
            if ranges
                .last()
                .is_some_and(|p: &HeightRange| p.from_nm >= from_nm)
            {
                return Err(invalid(format!(
                    "range {i} must start above the previous range"
                )));
            }
            let rule = parse_rule(i, r, &height)?;
            ranges.push(HeightRange {
                from_nm,
                height: rule,
                exposure_s: r.exposure_s,
            });
        }
        if raw.exposures.len() > MAX_EXPOSURES {
            return Err(invalid(format!("at most {MAX_EXPOSURES} exposure points")));
        }
        let mut exposures: Vec<(u64, f64)> = Vec::with_capacity(raw.exposures.len());
        for e in &raw.exposures {
            let h = height(e.height_mm, "exposure height_mm")?;
            if !(e.exposure_s.is_finite() && e.exposure_s > 0.) {
                return Err(invalid("exposure table times must be positive"));
            }
            if exposures.last().is_some_and(|&(p, _)| p >= h) {
                return Err(invalid("exposure table heights must increase"));
            }
            exposures.push((h, e.exposure_s));
        }
        Ok(Self {
            z_step_nm: step,
            ranges,
            exposures,
        })
    }

    /// The document as `karasu-layers` JSON, which [`Self::from_json`] reads back exactly
    /// (TD-130): heights in millimetres from their whole nanometres.
    ///
    /// # Panics
    /// Never: the document holds numbers and strings only, which always serialise.
    #[must_use]
    pub fn to_json(&self) -> Vec<u8> {
        #[allow(clippy::cast_precision_loss)] // Heights below 1 m in nanometres: exact in f64.
        let mm = |nm: u64| nm as f64 / NM_PER_MM;
        let raw = RawDocument {
            format: FORMAT.to_owned(),
            version: VERSION,
            z_step_mm: mm(self.z_step_nm),
            ranges: self
                .ranges
                .iter()
                .map(|r| {
                    let (height_mm, adaptive) = match r.height {
                        RangeHeight::Fixed(nm) => (Some(mm(nm)), None),
                        RangeHeight::Adaptive(a) => (
                            None,
                            Some(RawAdaptive {
                                min_height_mm: mm(a.min_nm),
                                max_height_mm: mm(a.max_nm),
                                max_deviation_mm: a.max_deviation_mm,
                                max_growth_ratio: a.max_growth_ratio,
                            }),
                        ),
                    };
                    RawRange {
                        from_mm: mm(r.from_nm),
                        height_mm,
                        exposure_s: r.exposure_s,
                        adaptive,
                    }
                })
                .collect(),
            exposures: self
                .exposures
                .iter()
                .map(|&(nm, exposure_s)| RawExposure {
                    height_mm: mm(nm),
                    exposure_s,
                })
                .collect(),
        };
        // Plain structs of numbers and strings: serialising them cannot fail.
        serde_json::to_vec(&raw).expect("a layer plan always serialises")
    }

    /// Whether any range is adaptive, so planning reads the model's slopes.
    #[must_use]
    pub fn is_adaptive(&self) -> bool {
        self.ranges
            .iter()
            .any(|r| matches!(r.height, RangeHeight::Adaptive(_)))
    }

    /// Exposure for a layer `height_nm` thick, interpolated linearly between table points.
    fn table_exposure(&self, height_nm: u64) -> Option<f64> {
        let points = &self.exposures;
        let at = points.iter().position(|&(h, _)| h >= height_nm)?;
        let (h1, e1) = points[at];
        if h1 == height_nm {
            return Some(e1);
        }
        let (h0, e0) = *points.get(at.checked_sub(1)?)?;
        #[allow(clippy::cast_precision_loss)] // Heights below 2^31 nm.
        let t = (height_nm - h0) as f64 / (h1 - h0) as f64;
        Some(e0 + (e1 - e0) * t)
    }
}

/// The height rule of range `i`, with `height` converting and checking a height.
fn parse_rule(
    i: usize,
    r: &RawRange,
    height: &dyn Fn(f64, &str) -> Result<u64, LayerPlanError>,
) -> Result<RangeHeight, LayerPlanError> {
    let rule = match (r.height_mm, &r.adaptive) {
        (Some(h), None) => RangeHeight::Fixed(height(h, "range height_mm")?),
        (None, Some(a)) => {
            let min_nm = height(a.min_height_mm, "adaptive min_height_mm")?;
            let max_nm = height(a.max_height_mm, "adaptive max_height_mm")?;
            if min_nm > max_nm {
                return Err(invalid("adaptive min_height_mm exceeds max_height_mm"));
            }
            if !(a.max_deviation_mm.is_finite() && a.max_deviation_mm > 0.) {
                return Err(invalid("adaptive max_deviation_mm must be positive"));
            }
            if !(a.max_growth_ratio.is_finite() && a.max_growth_ratio >= 1.) {
                return Err(invalid("adaptive max_growth_ratio must be at least 1"));
            }
            RangeHeight::Adaptive(AdaptiveRule {
                min_nm,
                max_nm,
                max_deviation_mm: a.max_deviation_mm,
                max_growth_ratio: a.max_growth_ratio,
            })
        }
        _ => {
            return Err(invalid(format!(
                "range {i} needs exactly one of height_mm and adaptive"
            )))
        }
    };
    if let Some(e) = r.exposure_s {
        if !(e.is_finite() && e > 0.) {
            return Err(invalid(format!("range {i} exposure_s must be positive")));
        }
        if matches!(rule, RangeHeight::Adaptive(_)) {
            return Err(invalid(format!(
                "range {i} is adaptive: its exposures come from the exposure table"
            )));
        }
    }
    Ok(rule)
}

/// The steepest stair-step factor of the faces in each Z bucket: the largest `|n_z|` of a unit
/// normal, so that a deviation `c` allows layers up to `c / |n_z|` thick. It does not depend on
/// any rule, so every adaptive range shares it.
struct SlopeIndex {
    bucket_nm: u64,
    cosines: Vec<f64>,
}

impl SlopeIndex {
    fn build(
        mesh: &TriangleMesh,
        bucket_nm: u64,
        top_nm: u64,
        cancel: &mut Cancellation<'_>,
    ) -> Result<Self, LayerPlanError> {
        let buckets = usize::try_from(top_nm / bucket_nm + 2).map_err(|_| invalid("Z range"))?;
        let mut cosines = vec![0f64; buckets];
        let vertices = mesh.vertices();
        #[allow(clippy::cast_precision_loss)] // Bucket sizes below 2^31 nm.
        let bucket_size_mm = bucket_nm as f64 / NM_PER_MM;
        for (face, corners) in mesh.triangles().iter().enumerate() {
            cancel.poll(Checkpoint::SchedulePlanes, face)?;
            let [p, q, r] = corners.map(|v| vertices[v as usize]);
            // Exactly horizontal faces (equal vertex heights) are boundary features, not slopes.
            if p.z.to_bits() == q.z.to_bits() && q.z.to_bits() == r.z.to_bits() {
                continue;
            }
            let (first_edge, second_edge) = (
                [q.x - p.x, q.y - p.y, q.z - p.z],
                [r.x - p.x, r.y - p.y, r.z - p.z],
            );
            let normal = [
                first_edge[1] * second_edge[2] - first_edge[2] * second_edge[1],
                first_edge[2] * second_edge[0] - first_edge[0] * second_edge[2],
                first_edge[0] * second_edge[1] - first_edge[1] * second_edge[0],
            ];
            let length = normal.iter().map(|c| c * c).sum::<f64>().sqrt();
            // Degenerate faces have no slope; vertical faces (zero cosine) leave no stair step.
            if !(length.is_finite() && length > 0.) {
                continue;
            }
            let cosine = normal[2].abs() / length;
            let (low, high) = (p.z.min(q.z).min(r.z), p.z.max(q.z).max(r.z));
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // Clamped below.
            let (first, last) = (
                ((low.max(0.) / bucket_size_mm).floor() as usize).min(buckets - 1),
                ((high.max(0.) / bucket_size_mm).floor() as usize).min(buckets - 1),
            );
            for slot in &mut cosines[first..=last] {
                *slot = slot.max(cosine);
            }
        }
        Ok(Self { bucket_nm, cosines })
    }

    /// The largest layer height (nanometres) that deviation `deviation_mm` allows over the
    /// buckets `[from, from + span]` touches; infinite where no sloped face lies.
    fn bound(&self, from_nm: u64, span_nm: u64, deviation_mm: f64) -> f64 {
        let first = usize::try_from(from_nm / self.bucket_nm).unwrap_or(usize::MAX);
        let last = usize::try_from((from_nm + span_nm) / self.bucket_nm).unwrap_or(usize::MAX);
        let steepest = self
            .cosines
            .iter()
            .take(last.saturating_add(1))
            .skip(first)
            .copied()
            .fold(0., f64::max);
        if steepest > 0. {
            deviation_mm * NM_PER_MM / steepest
        } else {
            f64::INFINITY
        }
    }
}

/// Per-layer heights and the settings of layers that differ from the calibration.
#[derive(Clone, Debug, PartialEq)]
pub struct PlannedLayers {
    /// Layer heights in nanometres, from the plate up.
    pub heights_nm: Vec<u64>,
    /// Explicit settings for every layer whose height or exposure differs from the calibration.
    pub overrides: Vec<LayerOverride>,
    /// Layers per range, in document order.
    pub layers_per_range: Vec<usize>,
}

/// Plan the layers of a model `top_mm` tall.
///
/// Layers are added while their centre lies at or below the top, as the uniform plan does.
/// `surface` is the placed model surface in world millimetres; adaptive ranges read its slopes
/// and require it.
///
/// # Errors
/// An adaptive document without a surface, bottom or transition layers whose range is not
/// calibrated, a range the print reaches that
/// starts no layer, layers that would need an exposure
/// the document does not give, a top beyond one metre, more than `max_layers` layers, and
/// cancellation.
pub fn plan_layers(
    doc: &LayerPlanDocument,
    calibration: &PrintSettings,
    surface: Option<&TriangleMesh>,
    top_mm: f64,
    max_layers: usize,
    cancel: &mut Cancellation<'_>,
) -> Result<PlannedLayers, LayerPlanError> {
    let top_ceiling = (top_mm * NM_PER_MM).ceil();
    if !(top_ceiling.is_finite() && (0. ..=1e9).contains(&top_ceiling)) {
        return Err(invalid(format!(
            "model top {top_mm} mm is not within 0 to 1000 mm"
        )));
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // Checked range.
    let ceiling_nm = top_ceiling as u64;
    let calibrated_nm = to_nm(calibration.layer_height_mm, "calibration layer height")?;
    // Buckets as fine as the smallest adaptive maximum keep every look-ahead within two buckets.
    let bucket_nm = doc
        .ranges
        .iter()
        .filter_map(|range| match range.height {
            RangeHeight::Adaptive(rule) => Some(rule.max_nm),
            RangeHeight::Fixed(_) => None,
        })
        .min();
    let slopes = match (bucket_nm, surface) {
        (Some(bucket_nm), Some(surface)) => {
            Some(SlopeIndex::build(surface, bucket_nm, ceiling_nm, cancel)?)
        }
        (Some(_), None) => return Err(invalid("adaptive ranges need the model surface")),
        (None, _) => None,
    };
    let mut heights: Vec<u64> = Vec::new();
    let mut overrides = Vec::new();
    let mut layers_per_range = vec![0usize; doc.ranges.len()];
    let fixed_layers =
        calibration.bottom_layer_count as usize + calibration.transition_layer_count as usize;
    let mut bottom = 0u64;
    let mut previous: Option<u64> = None;
    loop {
        cancel.poll(Checkpoint::SchedulePlanes, heights.len())?;
        let r = doc
            .ranges
            .iter()
            .rposition(|range| range.from_nm <= bottom)
            .unwrap_or(0);
        let range = doc.ranges[r];
        let height = match range.height {
            RangeHeight::Fixed(h) => h,
            RangeHeight::Adaptive(rule) => {
                rule.height(slopes.as_ref(), bottom, previous, doc.z_step_nm)
            }
        };
        // A layer belongs to the print while its centre lies at or below the top.
        if 2 * bottom + height > 2 * ceiling_nm {
            break;
        }
        if heights.len() >= max_layers {
            return Err(invalid(format!("more than {max_layers} layers")));
        }
        let index = heights.len();
        if index < fixed_layers && (height != calibrated_nm || range.exposure_s.is_some()) {
            return Err(invalid(format!(
                "layer {index} is a bottom or transition layer: its range must use the calibrated \
                 {} mm height and no explicit exposure",
                calibration.layer_height_mm
            )));
        }
        let differs = height != calibrated_nm || range.exposure_s.is_some();
        if differs {
            let exposure = match range.exposure_s {
                Some(e) => e,
                None => doc.table_exposure(height).ok_or_else(|| {
                    #[allow(clippy::cast_precision_loss)] // Heights below 2^31 nm.
                    let mm = height as f64 / NM_PER_MM;
                    invalid(format!(
                        "layer {index} is {mm} mm thick, but neither its range nor the exposure \
                         table gives an exposure for that height"
                    ))
                })?,
            };
            let mut settings = calibration.normal.clone();
            settings.exposure_s = exposure;
            overrides.push(LayerOverride {
                index: index as u64,
                settings,
            });
        }
        heights.push(height);
        layers_per_range[r] += 1;
        previous = Some(height);
        bottom += height;
    }
    if heights.is_empty() {
        return Err(invalid("the model is lower than half of its first layer"));
    }
    check_skipped_ranges(doc, &layers_per_range, bottom)?;
    Ok(PlannedLayers {
        heights_nm: heights,
        overrides,
        layers_per_range,
    })
}

/// A range the stack reaches but that starts no layer would be skipped silently (carry policy:
/// the layer below crossed it entirely); `top_nm` is the stack's top.
fn check_skipped_ranges(
    doc: &LayerPlanDocument,
    layers_per_range: &[usize],
    top_nm: u64,
) -> Result<(), LayerPlanError> {
    for (range, &count) in doc.ranges.iter().zip(layers_per_range) {
        if count == 0 && range.from_nm < top_nm {
            #[allow(clippy::cast_precision_loss)] // Positions below 2^30 nm.
            let from_mm = range.from_nm as f64 / NM_PER_MM;
            return Err(invalid(format!(
                "the range from {from_mm} mm starts no layer: the layer below it crosses its whole \
                 extent"
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        geometry::Point3,
        profiles::{LayerSettings, MotionStage},
    };

    fn settings() -> PrintSettings {
        let layer = |exposure_s| LayerSettings {
            exposure_s,
            wait_before_exposure_s: 0.,
            wait_after_exposure_s: 0.,
            wait_after_lift_s: 0.,
            pwm: 255,
            lift: vec![MotionStage {
                distance_mm: 5.,
                speed_mm_s: 2.,
            }],
            retract: vec![MotionStage {
                distance_mm: 5.,
                speed_mm_s: 2.,
            }],
        };
        PrintSettings {
            heating_temperature_c: None,
            layer_height_mm: 0.05,
            bottom_layer_count: 2,
            transition_layer_count: 0,
            bottom: layer(20.),
            normal: layer(2.),
        }
    }

    fn doc(body: &str) -> Result<LayerPlanDocument, LayerPlanError> {
        LayerPlanDocument::from_json(
            format!(r#"{{"format":"karasu-layers","version":1,{body}}}"#).as_bytes(),
        )
    }

    #[test]
    fn a_plan_written_reads_back_equal() {
        // TD-130: fixed and adaptive ranges, explicit exposures, the table and a custom step.
        let plan = doc(r#""z_step_mm":0.005,"ranges":[
                {"from_mm":0,"height_mm":0.05,"exposure_s":2.5},
                {"from_mm":1.2,"adaptive":{"min_height_mm":0.025,"max_height_mm":0.1,
                    "max_deviation_mm":0.03,"max_growth_ratio":1.25}},
                {"from_mm":7.315,"height_mm":0.035}],
            "exposures":[{"height_mm":0.025,"exposure_s":1.8},{"height_mm":0.1,"exposure_s":3.1}]"#)
        .unwrap();
        let written = plan.to_json();
        assert_eq!(LayerPlanDocument::from_json(&written).unwrap(), plan);
        assert_eq!(plan.to_json(), written);
    }

    /// A cone of half-angle 45° (apex up): every face has `|n_z| = cos 45°`.
    fn cone() -> TriangleMesh {
        let mut points = vec![Point3::new(0., 0., 10.), Point3::new(0., 0., 0.)];
        let n = 64u32;
        for k in 0..n {
            let a = std::f64::consts::TAU * f64::from(k) / f64::from(n);
            points.push(Point3::new(10. * a.cos(), 10. * a.sin(), 0.));
        }
        let mut triangles = Vec::new();
        for k in 0..n {
            let (i, j) = (2 + k, 2 + (k + 1) % n);
            triangles.push([0, i, j]);
            triangles.push([1, j, i]);
        }
        TriangleMesh::try_new(points, triangles).unwrap()
    }

    #[test]
    fn fixed_ranges_carry_heights_and_exposures() {
        let d = doc(r#""ranges":[{"from_mm":0,"height_mm":0.05},{"from_mm":0.3,"height_mm":0.025,"exposure_s":1.5},{"from_mm":0.5,"height_mm":0.1}],"exposures":[{"height_mm":0.05,"exposure_s":2},{"height_mm":0.1,"exposure_s":3.5}]"#).unwrap();
        let plan = plan_layers(
            &d,
            &settings(),
            Some(&cone()),
            1.0,
            1000,
            &mut Cancellation::never(),
        )
        .unwrap();
        // 6 layers to 0.3, 8 thin layers to 0.5, then 0.1 mm layers while centres stay <= 1.0.
        assert_eq!(plan.layers_per_range, [6, 8, 5]);
        assert_eq!(
            &plan.heights_nm[..7],
            &[50_000; 6]
                .iter()
                .copied()
                .chain([25_000])
                .collect::<Vec<_>>()[..]
        );
        assert_eq!(plan.heights_nm.iter().sum::<u64>(), 1_000_000);
        // Thin layers use the range exposure; thick layers the table (3.5 s at 0.1 mm).
        assert_eq!(plan.overrides.len(), 13);
        assert_eq!(plan.overrides[0].index, 6);
        assert_eq!(plan.overrides[0].settings.exposure_s, 1.5);
        assert_eq!(plan.overrides[12].settings.exposure_s, 3.5);
        // An interpolated table value.
        assert!((d.table_exposure(75_000).unwrap() - 2.75).abs() < 1e-12);
        assert_eq!(d.table_exposure(25_000), None);
    }

    #[test]
    fn adaptive_heights_follow_slopes_and_growth_limits() {
        let d = doc(r#""ranges":[{"from_mm":0,"height_mm":0.05},{"from_mm":0.1,"adaptive":{"min_height_mm":0.02,"max_height_mm":0.1,"max_deviation_mm":0.02,"max_growth_ratio":1.5}}],"exposures":[{"height_mm":0.02,"exposure_s":1},{"height_mm":0.1,"exposure_s":4}]"#).unwrap();
        let plan = plan_layers(
            &d,
            &settings(),
            Some(&cone()),
            10.0,
            10_000,
            &mut Cancellation::never(),
        )
        .unwrap();
        // 45° faces allow c / cos 45° = 0.0283 mm, floored to the 1 µm step: 0.028 mm.
        let adaptive = &plan.heights_nm[2..];
        assert!(adaptive.iter().all(|&h| h == 28_000), "{adaptive:?}");
        // Exposures interpolate: 1 + 3 * (8 / 80) = 1.3 s.
        assert!((plan.overrides[0].settings.exposure_s - 1.3).abs() < 1e-12);
        // Above a vertical-walled column the bound is the maximum, reached by 1.5x growth.
        let column = {
            let (points, triangles) = box_mesh([0., 0., 0.], [5., 5., 10.]);
            TriangleMesh::try_new(points, triangles).unwrap()
        };
        let plan = plan_layers(
            &d,
            &settings(),
            Some(&column),
            2.0,
            10_000,
            &mut Cancellation::never(),
        )
        .unwrap();
        assert_eq!(&plan.heights_nm[2..5], &[75_000, 100_000, 100_000]);
        // Exactly horizontal faces are not slopes.
        assert_eq!(
            plan.heights_nm.iter().filter(|&&h| h == 100_000).count(),
            plan.heights_nm.len() - 3
        );
    }

    fn box_mesh(min: [f64; 3], max: [f64; 3]) -> (Vec<Point3>, Vec<[u32; 3]>) {
        let points = (0..8)
            .map(|i| {
                Point3::new(
                    if i & 1 == 0 { min[0] } else { max[0] },
                    if i & 2 == 0 { min[1] } else { max[1] },
                    if i & 4 == 0 { min[2] } else { max[2] },
                )
            })
            .collect();
        let mut triangles = Vec::new();
        for q in [
            [0, 2, 3, 1],
            [4, 5, 7, 6],
            [0, 1, 5, 4],
            [2, 6, 7, 3],
            [0, 4, 6, 2],
            [1, 3, 7, 5],
        ] {
            triangles.push([q[0], q[1], q[2]]);
            triangles.push([q[0], q[2], q[3]]);
        }
        (points, triangles)
    }

    #[test]
    fn invalid_documents_and_missing_exposures_are_refused() {
        for (body, expected) in [
            (r#""ranges":[]"#, "between 1"),
            (r#""ranges":[{"from_mm":1,"height_mm":0.05}]"#, "start at 0"),
            (
                r#""ranges":[{"from_mm":0,"height_mm":0.0505}],"z_step_mm":0.001"#,
                "multiple",
            ),
            (
                r#""ranges":[{"from_mm":0,"height_mm":0.05},{"from_mm":0,"height_mm":0.05}]"#,
                "above the previous",
            ),
            (r#""ranges":[{"from_mm":0}]"#, "exactly one"),
            (
                r#""ranges":[{"from_mm":0,"height_mm":0.05}],"exposures":[{"height_mm":0.1,"exposure_s":2},{"height_mm":0.05,"exposure_s":1}]"#,
                "increase",
            ),
            (
                r#""ranges":[{"from_mm":0,"height_mm":0.05}],"extra":1"#,
                "unknown field",
            ),
        ] {
            let error = doc(body).unwrap_err().to_string();
            assert!(error.contains(expected), "{body}: {error}");
        }
        // A 0.01 mm range inside one 0.05 mm layer would be skipped.
        let d = doc(r#""ranges":[{"from_mm":0,"height_mm":0.05},{"from_mm":0.32,"height_mm":0.025,"exposure_s":1},{"from_mm":0.33,"height_mm":0.05}]"#).unwrap();
        let error = plan_layers(
            &d,
            &settings(),
            Some(&cone()),
            1.0,
            1000,
            &mut Cancellation::never(),
        )
        .unwrap_err();
        assert!(error.to_string().contains("starts no layer"), "{error}");
        // A 0.03 mm range with no exposure anywhere.
        let d =
            doc(r#""ranges":[{"from_mm":0,"height_mm":0.05},{"from_mm":0.2,"height_mm":0.03}]"#)
                .unwrap();
        let error = plan_layers(
            &d,
            &settings(),
            Some(&cone()),
            1.0,
            1000,
            &mut Cancellation::never(),
        )
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("neither its range nor the exposure table"),
            "{error}"
        );
    }
}
