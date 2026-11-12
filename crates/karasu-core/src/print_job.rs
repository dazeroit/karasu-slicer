//! Immutable versioned print plans, separate sample planes and printer positions, and resolved settings.
use crate::{
    cancellation::{Cancellation, Cancelled, Checkpoint},
    profiles::{
        validate_profile_id, LayerSettings, PrintSettings, PrinterProfile, ProfileError,
        ResinProfile,
    },
    slicing::schedule::{LayerPlane, LayerSchedule},
};
use serde::{Deserialize, Serialize};
use std::{error::Error, fmt, mem::size_of};
pub(crate) mod codecs;
pub(crate) mod ctb;
pub(crate) mod machine;
pub(crate) mod photon_workshop;
pub(crate) mod pp1;
pub(crate) mod pp1_scene;
pub(crate) mod sl1;
pub(crate) mod zip;

/// The longest application version the machine files can carry: the GOO header keeps it in a
/// 24-byte field with its terminating zero, Photon Workshop in 32 bytes (TD-151).
pub(crate) const MAX_VERSION_LEN: usize = 23;
// A longer version, such as a pre-release suffix with a commit hash, fails the build instead of
// every GOO export.
const _: () = assert!(
    env!("CARGO_PKG_VERSION").len() <= MAX_VERSION_LEN,
    "the version in Cargo.toml must be at most 23 characters to fit the GOO header"
);

/// Current reference job and manifest semantic version.
pub const JOB_SCHEMA_VERSION: u32 = 1;
/// Complete settings replacement at an explicitly named layer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayerOverride {
    /// Zero-based layer index.
    pub index: u64,
    /// Full layer settings, never an implicit patch.
    pub settings: LayerSettings,
}
/// Resolved nominal layer classification.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LayerPhase {
    /// Initial adhesion settings.
    Bottom,
    /// Exposure-only interpolation, normal motion/waits/PWM.
    Transition,
    /// Normal calibration.
    Normal,
    /// Explicit complete layer override.
    Override,
}
/// One complete resolved record, calculated on demand from an immutable plan.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct JobLayer {
    /// Contiguous zero-based index.
    pub index: u64,
    /// Explicit M3 world-space sample plane, millimeters.
    pub sample_z_mm: f64,
    /// Printer build position, distinct from sample Z, millimeters.
    pub position_z_mm: f64,
    /// Nominal material thickness, millimeters.
    pub thickness_mm: f64,
    /// Settings source class.
    pub phase: LayerPhase,
    /// Resolved physical settings.
    pub settings: LayerSettings,
}
/// Metadata budgets; caller-provided source meshes and masks are separate.
#[derive(Clone, Copy, Debug)]
pub struct JobLimits {
    /// Maximum number of planned layers.
    pub max_layers: usize,
    /// Planned schedule/override retained payload, excluding bounded profile snapshots/allocator overhead.
    pub max_metadata_bytes: usize,
}
impl Default for JobLimits {
    fn default() -> Self {
        Self {
            max_layers: 1_000_000,
            max_metadata_bytes: 128 * 1024 * 1024,
        }
    }
}
/// Invalid complete job or interrupted validation.
#[derive(Debug)]
pub enum JobError {
    /// Invalid printer or resin with original field context.
    Profile(ProfileError),
    /// Invalid plan, settings, sample or travel, with actionable context.
    Invalid(String),
    /// Fallible retained schedule reservation failed.
    Allocation,
    /// Cancelled during complete-plan validation.
    Cancelled(Cancelled),
}
impl fmt::Display for JobError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Profile(e) => e.fmt(f),
            Self::Invalid(e) => f.write_str(e),
            Self::Allocation => f.write_str("cannot allocate bounded print job metadata"),
            Self::Cancelled(e) => e.fmt(f),
        }
    }
}
impl Error for JobError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Profile(e) => Some(e),
            Self::Cancelled(e) => Some(e),
            _ => None,
        }
    }
}
impl From<ProfileError> for JobError {
    fn from(e: ProfileError) -> Self {
        Self::Profile(e)
    }
}
impl From<Cancelled> for JobError {
    fn from(e: Cancelled) -> Self {
        Self::Cancelled(e)
    }
}
fn invalid(message: impl Into<String>) -> JobError {
    JobError::Invalid(message.into())
}
/// Validated immutable snapshots and bounded sample schedule; settings are resolved on demand.
#[derive(Debug)]
pub struct PrintJob {
    id: String,
    printer: PrinterProfile,
    resin: ResinProfile,
    settings: PrintSettings,
    planes: Vec<LayerPlane>,
    /// Printed slabs of a variable-height schedule; `None` stacks calibration-height layers.
    slabs: Option<Vec<crate::slicing::schedule::Slab>>,
    overrides: Vec<LayerOverride>,
    estimated_time_s: f64,
}
impl PrintJob {
    /// Snapshot profiles and validate the complete layer plan with no inferred machine calibration.
    /// # Errors
    /// Rejects invalid profiles, absent calibration, bad sample/position/thickness/counts, settings,
    /// travel, duplicate overrides, configured metadata budgets or allocation failure.
    pub fn new(
        id: &str,
        printer: &PrinterProfile,
        resin: &ResinProfile,
        schedule: &LayerSchedule,
        overrides: &[LayerOverride],
        limits: JobLimits,
    ) -> Result<Self, JobError> {
        Self::new_with_cancellation(
            id,
            printer,
            resin,
            schedule,
            overrides,
            limits,
            &mut Cancellation::never(),
        )
    }
    /// Cancellable complete-plan validation before publishing a job.
    /// # Errors
    /// Returns [`Self::new`]'s failures and named cancellation with no partial job.
    #[allow(clippy::too_many_arguments)] // Explicit inputs preserve immutable profile/schedule ownership.
    pub fn new_with_cancellation(
        id: &str,
        printer: &PrinterProfile,
        resin: &ResinProfile,
        schedule: &LayerSchedule,
        overrides: &[LayerOverride],
        limits: JobLimits,
        cancel: &mut Cancellation<'_>,
    ) -> Result<Self, JobError> {
        cancel.check(Checkpoint::ExportStart)?;
        validate_profile_id(id)?;
        printer.validate()?;
        resin.validate()?;
        let settings = resin.calibration_for(&printer.id)?;
        settings.validate(Some(&printer.limits))?;
        let count = schedule.planes().len();
        if count == 0 || count > limits.max_layers || u32::try_from(count).is_err() {
            return Err(invalid(
                "print job layer count must be nonzero and within configured/u32 limits",
            ));
        }
        let preliminary =
            usize::try_from(settings.bottom_layer_count + settings.transition_layer_count)
                .map_err(|_| invalid("layer class count overflow"))?;
        if preliminary > count {
            return Err(invalid("bottom plus transition layers exceed job length"));
        }
        if overrides.len() > count {
            return Err(invalid("too many layer overrides"));
        }
        let bytes = count
            .checked_mul(size_of::<LayerPlane>())
            .and_then(|v| {
                v.checked_add(overrides.len().checked_mul(
                    size_of::<LayerOverride>() + 4 * size_of::<crate::profiles::MotionStage>(),
                )?)
            })
            .ok_or_else(|| invalid("job metadata byte count overflow"))?;
        if bytes > limits.max_metadata_bytes {
            return Err(invalid("print job exceeds metadata byte budget"));
        }
        let mut planes = Vec::new();
        planes
            .try_reserve_exact(count)
            .map_err(|_| JobError::Allocation)?;
        planes.extend_from_slice(schedule.planes());
        let mut replacements = Vec::new();
        replacements
            .try_reserve_exact(overrides.len())
            .map_err(|_| JobError::Allocation)?;
        for (i, value) in overrides.iter().enumerate() {
            cancel.poll(Checkpoint::ExportLayer, i)?;
            if value.index >= u64::try_from(count).map_err(|_| invalid("layer count overflow"))? {
                return Err(invalid(format!(
                    "override layer {} is outside the job",
                    value.index
                )));
            }
            value.settings.validate(Some(&printer.limits))?;
            replacements.push(value.clone());
        }
        replacements.sort_unstable_by_key(|x| x.index);
        if replacements.windows(2).any(|v| v[0].index == v[1].index) {
            return Err(invalid("duplicate layer override index"));
        }
        let slabs = schedule.slabs().map(<[_]>::to_vec);
        let mut job = Self {
            id: id.to_owned(),
            printer: printer.clone(),
            resin: resin.clone(),
            settings: settings.clone(),
            planes,
            slabs,
            overrides: replacements,
            estimated_time_s: 0.,
        };
        job.estimated_time_s = job.validate_layers(cancel)?;
        cancel.check(Checkpoint::ExportFinish)?;
        Ok(job)
    }
    fn validate_layers(&self, cancel: &mut Cancellation<'_>) -> Result<f64, JobError> {
        let mut estimated_time_s = 0.;
        // What the printer spends on every layer besides it (TD-122).
        let fixed = self.printer.fixed_layer_time_s.unwrap_or(0.);
        for index in 0..self.planes.len() {
            cancel.poll(Checkpoint::ExportLayer, index)?;
            let layer = self.layer(index)?;
            let plane = self.planes[index];
            let calibrated =
                plane.nominal_height_mm.to_bits() == self.settings.layer_height_mm.to_bits();
            if plane.index != layer.index || (self.slabs.is_none() && !calibrated) {
                return Err(invalid(format!(
                    "layer {index} schedule index/thickness differs from calibration"
                )));
            }
            if !calibrated {
                // Variable heights: bottom and transition layers keep the calibrated height, and
                // any other thickness needs explicit settings (no implied exposure).
                let classes = u64::from(self.settings.bottom_layer_count)
                    + u64::from(self.settings.transition_layer_count);
                if layer.index < classes {
                    return Err(invalid(format!(
                        "layer {index} is a bottom or transition layer but is {} mm thick, unlike \
                         the calibrated {} mm",
                        plane.nominal_height_mm, self.settings.layer_height_mm
                    )));
                }
                let limits = &self.printer.limits;
                if plane.nominal_height_mm < limits.min_layer_height_mm
                    || plane.nominal_height_mm > limits.max_layer_height_mm
                {
                    return Err(invalid(format!(
                        "layer {index} is {} mm thick, outside the printer's {} to {} mm",
                        plane.nominal_height_mm,
                        limits.min_layer_height_mm,
                        limits.max_layer_height_mm
                    )));
                }
                if layer.phase != LayerPhase::Override {
                    return Err(invalid(format!(
                        "layer {index} is {} mm thick, unlike the calibrated {} mm, and has no \
                         explicit settings",
                        plane.nominal_height_mm, self.settings.layer_height_mm
                    )));
                }
            }
            let lower = layer.position_z_mm - layer.thickness_mm;
            // A local eight-ulp budget only reconciles independently rounded slab endpoints; no
            // mesh coordinate is modified. Sampling remains an explicit caller choice.
            let slab_tolerance =
                8. * f64::EPSILON * layer.position_z_mm.abs().max(layer.thickness_mm);
            if !layer.position_z_mm.is_finite()
                || !plane.z_mm.is_finite()
                || plane.z_mm < lower - slab_tolerance
                || plane.z_mm > layer.position_z_mm + slab_tolerance
            {
                return Err(invalid(format!(
                    "sample plane for layer {index} is outside its printed slab"
                )));
            }
            if layer.position_z_mm + layer.settings.lift_distance_mm()
                > self.printer.build_size_mm[2]
            {
                return Err(invalid(format!(
                    "layer {index} position plus lift exceeds printer Z envelope"
                )));
            }
            let motion: f64 = layer
                .settings
                .lift
                .iter()
                .chain(&layer.settings.retract)
                .map(|s| s.distance_mm / s.speed_mm_s)
                .sum();
            estimated_time_s += layer.settings.exposure_s
                + layer.settings.wait_before_exposure_s
                + layer.settings.wait_after_exposure_s
                + layer.settings.wait_after_lift_s
                + motion
                + fixed;
            if !estimated_time_s.is_finite() {
                return Err(invalid("estimated job duration exceeds finite range"));
            }
        }
        Ok(estimated_time_s)
    }
    /// Stable job identity.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }
    /// Immutable printer hardware/revision snapshot.
    #[must_use]
    pub const fn printer(&self) -> &PrinterProfile {
        &self.printer
    }
    /// Immutable resin/calibration revision snapshot.
    #[must_use]
    pub const fn resin(&self) -> &ResinProfile {
        &self.resin
    }
    /// Complete calibration snapshot.
    #[must_use]
    pub const fn settings(&self) -> &PrintSettings {
        &self.settings
    }
    /// Explicit override snapshot in ascending unique index order.
    #[must_use]
    pub fn overrides(&self) -> &[LayerOverride] {
        &self.overrides
    }
    /// Explicit M3 sample planes.
    #[must_use]
    pub fn sample_planes(&self) -> &[LayerPlane] {
        &self.planes
    }
    /// Nonzero number of layers.
    #[must_use]
    pub fn layer_count(&self) -> usize {
        self.planes.len()
    }
    /// Nominal duration estimate, excluding firmware acceleration/heating/sensing.
    #[must_use]
    pub const fn estimated_time_s(&self) -> f64 {
        self.estimated_time_s
    }
    /// Resolve one complete layer without retaining an expanded per-layer settings array.
    /// # Errors
    /// Rejects out-of-range indices or a checked position/index overflow.
    pub fn layer(&self, index: usize) -> Result<JobLayer, JobError> {
        let plane = self
            .planes
            .get(index)
            .ok_or_else(|| invalid(format!("layer index {index} is outside the job")))?;
        let id = u64::try_from(index).map_err(|_| invalid("layer index overflow"))?;
        let bottom = u64::from(self.settings.bottom_layer_count);
        let transition = u64::from(self.settings.transition_layer_count);
        let (mut settings, mut phase) = if id < bottom {
            (self.settings.bottom.clone(), LayerPhase::Bottom)
        } else if id < bottom + transition {
            let mut value = self.settings.normal.clone();
            let j =
                u32::try_from(id - bottom + 1).map_err(|_| invalid("transition index overflow"))?;
            let fraction = f64::from(j) / (f64::from(self.settings.transition_layer_count) + 1.);
            value.exposure_s = self.settings.bottom.exposure_s
                + (self.settings.normal.exposure_s - self.settings.bottom.exposure_s) * fraction;
            (value, LayerPhase::Transition)
        } else {
            (self.settings.normal.clone(), LayerPhase::Normal)
        };
        if let Ok(replacement) = self.overrides.binary_search_by_key(&id, |v| v.index) {
            settings = self.overrides[replacement].settings.clone();
            phase = LayerPhase::Override;
        }
        let (position_z_mm, thickness_mm) = if let Some(slabs) = &self.slabs {
            (slabs[index].top_mm, slabs[index].height_mm)
        } else {
            let position_index =
                u32::try_from(index + 1).map_err(|_| invalid("printer position index overflow"))?;
            (
                f64::from(position_index) * self.settings.layer_height_mm,
                self.settings.layer_height_mm,
            )
        };
        Ok(JobLayer {
            index: id,
            sample_z_mm: plane.z_mm,
            position_z_mm,
            thickness_mm,
            phase,
            settings,
        })
    }
}
