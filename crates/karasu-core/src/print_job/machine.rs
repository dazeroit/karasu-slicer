//! Bounded, independent serializers for explicitly identified machine revisions.
use super::codecs::{ctb_rle, goo_rle, photon_rle};
use crate::{
    cancellation::{Cancellation, Cancelled, Checkpoint},
    export::AnycubicWorkshop,
    export::WriteSeek,
    preview::{rgb15, rgb565, PreviewImage, Previews},
    raster::mask::SpanMask,
};
use std::{
    error::Error,
    fmt,
    io::{self, SeekFrom},
};

/// One layer's encoded format payload and the intensity sum of its (quantized) samples.
#[derive(Debug)]
pub(crate) struct EncodedPayload {
    pub bytes: Vec<u8>,
    pub intensity: u64,
    /// Lit pixels and their pixel bounds (Photon Workshop records and MODEL).
    pub lit: Option<(u64, Option<[[u32; 2]; 2]>)>,
    /// Per-layer P1 scene metrics, measured with the encoding (P1 only).
    pub scene: Option<super::pp1_scene::LayerMetrics>,
    /// The layer as its ZIP member, deflated with the encoding (P1 only, TD-133); `bytes` is
    /// then empty.
    pub member: Option<super::zip::Member>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MachineFormat {
    CtbV2Plain,
    /// Plain CTB version 3: per-layer `LayerDefEx` records (TD-063).
    CtbV3Plain,
    /// Plain CTB version 4: two-stage motion and rest times (TD-063).
    CtbV4Plain,
    /// Plain CTB version 5: version 4 plus resin parameters (TD-063).
    CtbV5Plain,
    GooV3,
    PhotonV1,
    /// Anycubic Photon Workshop binary file (TD-063).
    PhotonWorkshop(AnycubicWorkshop),
}

impl MachineFormat {
    /// Any CTB version.
    pub(crate) const fn is_ctb(self) -> bool {
        matches!(
            self,
            Self::CtbV2Plain | Self::CtbV3Plain | Self::CtbV4Plain | Self::CtbV5Plain
        )
    }
    /// Formats with two-stage motion, rest times and full per-layer settings.
    const fn full_motion(self) -> bool {
        matches!(self, Self::GooV3 | Self::CtbV4Plain | Self::CtbV5Plain)
    }
    /// Formats that store per-layer lift, retract and PWM.
    const fn per_layer_motion(self) -> bool {
        matches!(
            self,
            Self::GooV3 | Self::CtbV3Plain | Self::CtbV4Plain | Self::CtbV5Plain
        )
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Quantization {
    Exact,
    RoundToFormat,
}
#[derive(Clone, Copy, Debug)]
#[allow(clippy::struct_field_names)] // Match public ExportLimits names to prevent adapter mapping mistakes.
pub(crate) struct EncoderLimits {
    pub max_output_bytes: u64,
    pub max_metadata_bytes: usize,
    pub max_layer_bytes: usize,
}
#[derive(Clone, Debug)]
pub(crate) struct WireSettings {
    pub exposure_s: f64,
    pub wait_before_exposure_s: f64,
    pub wait_after_exposure_s: f64,
    pub wait_after_lift_s: f64,
    pub pwm: u8,
    /// [distance mm, speed mm/s], first then optional second stage.
    pub lift: [[f64; 2]; 2],
    pub retract: [[f64; 2]; 2],
}
#[derive(Clone, Debug)]
pub(crate) struct WireJob {
    /// Pixel columns and rows.
    pub resolution: [u32; 2],
    /// Panel lengths along pixel columns and rows, then build height (mm); with `swap_xy` the
    /// first two are the printer's world Y and X lengths.
    pub build_mm: [f64; 3],
    pub layer_height_mm: f64,
    pub bottom_count: u32,
    pub transition_count: u32,
    pub layer_count: u32,
    pub machine_name: String,
    pub normal: WireSettings,
    pub bottom: WireSettings,
    pub estimated_time_s: f64,
    pub volume_ml: f64,
    /// Mask supersampling per axis (1: binary masks), recorded in the format's metadata.
    pub antialiasing: u8,
    /// Model thumbnails at this format's sizes ([`preview_sizes`]), or `None` for blank or absent
    /// previews.
    pub previews: Option<Previews>,
    /// Job identity (SL1 layer names and `jobDir`).
    pub job_name: String,
    /// Resin name (CTB version 5 resin parameters).
    pub resin_name: String,
    /// Resin density in g/mL, or zero when unknown (CTB version 5).
    pub resin_density_g_per_ml: f64,
    /// The vat's heating target in °C (PP1, TD-118).
    pub heating_temperature_c: f64,
    /// Whether the stored image is mirrored along its pixel columns and rows against the
    /// format's view from above with the plate's back in the first row, as the GOO header
    /// declares it (TD-104).
    pub stored_mirror: [bool; 2],
}

/// The `[small, large]` preview sizes `format` stores, as `[width, height]`.
pub(crate) const fn preview_sizes(format: MachineFormat) -> [[u32; 2]; 2] {
    match format {
        MachineFormat::GooV3 => [[116, 116], [290, 290]],
        MachineFormat::CtbV2Plain
        | MachineFormat::CtbV3Plain
        | MachineFormat::CtbV4Plain
        | MachineFormat::CtbV5Plain
        | MachineFormat::PhotonV1 => [[200, 125], [400, 300]],
        MachineFormat::PhotonWorkshop(_) => [
            super::photon_workshop::PREVIEW_SIZE,
            super::photon_workshop::PREVIEW_SIZE,
        ],
    }
}

/// Check that `previews` have exactly `sizes` (`[small, large]`).
pub(crate) fn check_previews(previews: &Previews, sizes: [[u32; 2]; 2]) -> Result<(), CodecError> {
    for (image, [width, height]) in [&previews.small, &previews.large].into_iter().zip(sizes) {
        if [image.width(), image.height()] != [width, height] {
            return Err(CodecError::Invalid(
                "preview size does not match the format",
            ));
        }
    }
    Ok(())
}

/// CTB/Photon preview RLE: 15-bit colour words (LE), with a run flag (bit 5) followed by a word
/// holding `0x3000 | (run - 1)` for runs of 2 to 4096 pixels.
fn rle15(image: &PreviewImage) -> Vec<u8> {
    let words: Vec<u16> = image
        .rgb()
        .as_chunks::<3>()
        .0
        .iter()
        .map(|&p| rgb15(p))
        .collect();
    let mut out = Vec::with_capacity(words.len());
    let mut at = 0;
    while at < words.len() {
        let colour = words[at];
        let mut end = at + 1;
        while end < words.len() && words[end] == colour && end - at < 4096 {
            end += 1;
        }
        let run = end - at;
        if run == 1 {
            out.extend(colour.to_le_bytes());
        } else {
            out.extend((colour | 0x0020).to_le_bytes());
            // Runs are at most 4096 pixels, so `run - 1` fits 12 bits.
            #[allow(clippy::cast_possible_truncation)]
            out.extend((0x3000 | (run - 1) as u16).to_le_bytes());
        }
        at = end;
    }
    out
}

/// Append the large then the small preview, each a 32-byte descriptor (size, data address and
/// length) followed by its RLE15 data, and point the header's offsets 60 and 72 at them.
pub(super) fn legacy_previews(out: &mut Vec<u8>, previews: &Previews) -> Result<(), CodecError> {
    for (image, at) in [(&previews.large, 60usize), (&previews.small, 72)] {
        let descriptor = out.len();
        let data = rle15(image);
        set_u32(out, at, checked_u32(descriptor as u64)?);
        out.resize(descriptor + 32, 0);
        set_u32(out, descriptor, image.width());
        set_u32(out, descriptor + 4, image.height());
        set_u32(out, descriptor + 8, checked_u32((descriptor + 32) as u64)?);
        set_u32(
            out,
            descriptor + 12,
            u32::try_from(data.len()).map_err(|_| CodecError::Limit("preview size"))?,
        );
        out.extend(data);
    }
    Ok(())
}

/// Mask supersampling per axis must be one the raster offers.
pub(crate) fn validate_antialiasing(samples_per_axis: u8) -> Result<(), CodecError> {
    if [1, 2, 4, 8, 16].contains(&samples_per_axis) {
        Ok(())
    } else {
        Err(CodecError::Invalid(
            "antialiasing must be 1, 2, 4, 8 or 16 samples per axis",
        ))
    }
}
#[derive(Clone, Debug)]
pub(crate) struct WireLayer {
    pub z_mm: f64,
    pub settings: WireSettings,
}
#[derive(Debug)]
pub(crate) enum CodecError {
    Io(io::Error),
    Cancelled(Cancelled),
    Invalid(&'static str),
    Unsupported(&'static str),
    Limit(&'static str),
}
impl fmt::Display for CodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled(e) => write!(f, "{e}"),
            Self::Io(e) => write!(f, "machine output I/O failed: {e}"),
            Self::Invalid(s) => write!(f, "invalid machine job: {s}"),
            Self::Unsupported(s) => write!(f, "unsupported machine capability: {s}"),
            Self::Limit(s) => write!(f, "machine output limit: {s}"),
        }
    }
}
impl Error for CodecError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            _ => None,
        }
    }
}
impl From<Cancelled> for CodecError {
    fn from(e: Cancelled) -> Self {
        Self::Cancelled(e)
    }
}
impl From<io::Error> for CodecError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

pub(crate) struct MachineEncoder {
    format: MachineFormat,
    job: WireJob,
    limits: EncoderLimits,
    quantization: Quantization,
    count: u32,
    table: u64,
    position: u64,
    previous_z: f64,
    encoded_volume_ml: f64,
    /// Absolute offset of the CTB print parameters (volume at +20).
    print_parameters: u64,
    /// Absolute offset of the CTB version-4 block, if any.
    v4_block: Option<u64>,
    /// Sum of exposures and waits, for the CTB version-4 block.
    stationary_s: f64,
    /// Photon Workshop offsets patched as layers arrive and at the end.
    workshop: Option<WorkshopOffsets>,
    /// Lit-pixel bounds over all layers (pixels) and the lowest and highest lit layer Z.
    lit_bounds: Option<([[u32; 2]; 2], [f64; 2])>,
}

/// Where a Photon Workshop file's records and patched fields live.
#[derive(Clone, Copy, Debug)]
struct WorkshopOffsets {
    header: u64,
    records: u64,
    sub_records: Option<u64>,
    model: Option<u64>,
}
#[allow(clippy::cast_possible_truncation)] // The checked wire conversion is this function's contract.
fn f32_checked(v: f64) -> Result<f32, CodecError> {
    if !v.is_finite() || !((v as f32).is_finite()) || (v > 0. && v as f32 == 0.) {
        return Err(CodecError::Invalid(
            "number cannot be represented as finite f32",
        ));
    }
    Ok(v as f32)
}
fn push_f32(out: &mut Vec<u8>, v: f64, big: bool) -> Result<(), CodecError> {
    let v = f32_checked(v)?;
    out.extend(if big {
        v.to_be_bytes()
    } else {
        v.to_le_bytes()
    });
    Ok(())
}
fn text(out: &mut Vec<u8>, v: &str, n: usize) -> Result<(), CodecError> {
    if !v.is_ascii() || v.len() >= n || v.as_bytes().contains(&0) {
        return Err(CodecError::Invalid(
            "fixed machine text must fit ASCII field",
        ));
    }
    out.extend(v.as_bytes());
    out.resize(out.len() + n - v.len(), 0);
    Ok(())
}
pub(super) fn set_u32(v: &mut [u8], at: usize, n: u32) {
    v[at..at + 4].copy_from_slice(&n.to_le_bytes());
}
pub(super) fn set_f32(v: &mut [u8], at: usize, n: f64) -> Result<(), CodecError> {
    v[at..at + 4].copy_from_slice(&f32_checked(n)?.to_le_bytes());
    Ok(())
}
pub(super) fn checked_u32(v: u64) -> Result<u32, CodecError> {
    u32::try_from(v).map_err(|_| CodecError::Limit("32-bit machine address"))
}
// One nanometer permits addition roundoff in independently split millimeter travel,
// while remaining far below the machine model's meaningful travel precision.
const MOTION_TRAVEL_TOLERANCE_MM: f64 = 1e-9;
fn validate_settings(s: &WireSettings) -> Result<(), CodecError> {
    if s.exposure_s <= 0. || s.pwm == 0 {
        return Err(CodecError::Invalid("exposure and PWM must be positive"));
    }
    for v in [
        s.exposure_s,
        s.wait_before_exposure_s,
        s.wait_after_exposure_s,
        s.wait_after_lift_s,
    ] {
        if v < 0. {
            return Err(CodecError::Invalid("negative timing"));
        }
        f32_checked(v)?;
    }
    for [distance, speed] in s.lift.into_iter().chain(s.retract) {
        if distance < 0. || speed < 0. || (distance > 0. && speed == 0.) {
            return Err(CodecError::Invalid("invalid motion segment"));
        }
        f32_checked(distance)?;
        f32_checked(speed * 60.)?;
    }
    if ((s.lift[0][0] + s.lift[1][0]) - (s.retract[0][0] + s.retract[1][0])).abs()
        > MOTION_TRAVEL_TOLERANCE_MM
    {
        return Err(CodecError::Invalid("lift and retract travel differ"));
    }
    Ok(())
}
fn validate_capability(format: MachineFormat, s: &WireSettings) -> Result<(), CodecError> {
    validate_settings(s)?;
    if let MachineFormat::PhotonWorkshop(target) = format {
        return super::photon_workshop::validate_settings(target, s);
    }
    if !format.full_motion() {
        if s.lift[1][0] != 0. || s.retract[1][0] != 0. {
            return Err(CodecError::Unsupported(
                "two-stage motion requires GOO or CTB version 4 or 5",
            ));
        }
        if s.wait_before_exposure_s != 0.
            || s.wait_after_exposure_s != 0.
            || s.wait_after_lift_s != 0.
        {
            return Err(CodecError::Unsupported(
                "distinct rest periods require GOO or CTB version 4 or 5; legacy light-off \
                 includes firmware motion",
            ));
        }
        if format == MachineFormat::PhotonV1
            && (s.lift[0][0] != 0. || s.retract[0][0] != 0. || s.pwm != 255)
        {
            return Err(CodecError::Unsupported(
                "Photon V1 has no motion/PWM fields; choose explicit firmware-managed motion",
            ));
        }
    }
    Ok(())
}
impl MachineEncoder {
    #[allow(clippy::float_cmp)] // Exact settings identity is required; approximate equality would discard a requested value.
    pub(crate) fn validate_metadata(
        format: MachineFormat,
        job: &WireJob,
    ) -> Result<(), CodecError> {
        // Photon v1 records no antialiasing; its binary masks come from the explicit pixel policy.
        validate_antialiasing(job.antialiasing)?;
        if job.resolution.contains(&0)
            || job.layer_count == 0
            || job.layer_height_mm <= 0.
            || job
                .bottom_count
                .checked_add(job.transition_count)
                .is_none_or(|n| n > job.layer_count)
        {
            return Err(CodecError::Invalid(
                "dimensions, layer count or bottom/transition count",
            ));
        }
        for v in job.build_mm {
            if v <= 0. {
                return Err(CodecError::Invalid("build dimensions"));
            }
            f32_checked(v)?;
        }
        f32_checked(job.layer_height_mm)?;
        if job.estimated_time_s < 0.
            || !job.estimated_time_s.is_finite()
            || job.estimated_time_s.ceil() > f64::from(u32::MAX)
            || job.volume_ml < 0.
        {
            return Err(CodecError::Invalid("duration or volume"));
        }
        f32_checked(job.volume_ml)?;
        validate_capability(format, &job.bottom)?;
        validate_capability(format, &job.normal)?;
        if format == MachineFormat::CtbV2Plain
            && job.bottom.retract[0][1] != job.normal.retract[0][1]
        {
            return Err(CodecError::Unsupported(
                "CTB V2 has one common retract speed",
            ));
        }
        if format == MachineFormat::GooV3 {
            if job.resolution.iter().any(|&n| n > 65535) || job.transition_count > 65535 {
                return Err(CodecError::Limit("GOO 16-bit dimensions/count"));
            }
            if !job.machine_name.is_ascii()
                || job.machine_name.len() >= 32
                || job.machine_name.as_bytes().contains(&0)
            {
                return Err(CodecError::Invalid(
                    "GOO machine identity must fit 31 ASCII bytes",
                ));
            }
            f32_checked(job.volume_ml * 1000.)?;
        } else if !job.machine_name.is_ascii()
            || job.machine_name.len() > 4096
            || job.machine_name.as_bytes().contains(&0)
        {
            return Err(CodecError::Invalid("legacy machine identity"));
        }
        if let MachineFormat::PhotonWorkshop(target) = format {
            super::photon_workshop::validate_job(target, job)?;
        }
        if matches!(
            format,
            MachineFormat::CtbV4Plain | MachineFormat::CtbV5Plain
        ) && job.machine_name.is_empty()
        {
            return Err(CodecError::Invalid(
                "CTB version 4 and 5 need a machine name (firmware may reject an empty one)",
            ));
        }
        if format == MachineFormat::CtbV5Plain {
            f32_checked(job.resin_density_g_per_ml)?;
            if job.resin_density_g_per_ml < 0. {
                return Err(CodecError::Invalid("resin density"));
            }
        }
        Ok(())
    }
    pub(crate) fn validate_layer(
        format: MachineFormat,
        job: &WireJob,
        layer: &WireLayer,
        index: u32,
    ) -> Result<(), CodecError> {
        if index >= job.layer_count || layer.z_mm <= 0. || layer.z_mm > job.build_mm[2] {
            return Err(CodecError::Invalid("layer index or Z"));
        }
        f32_checked(layer.z_mm)?;
        validate_capability(format, &layer.settings)?;
        if !format.per_layer_motion() {
            let expected = if index < job.bottom_count {
                &job.bottom
            } else {
                &job.normal
            };
            #[allow(clippy::float_cmp)] // One stored light-off value must match exactly.
            let light_off_differs = matches!(format, MachineFormat::PhotonWorkshop(_))
                && layer.settings.wait_before_exposure_s != expected.wait_before_exposure_s;
            if layer.settings.lift != expected.lift
                || layer.settings.retract != expected.retract
                || layer.settings.pwm != expected.pwm
                || light_off_differs
            {
                return Err(CodecError::Unsupported(
                    "per-layer motion or PWM requires GOO or CTB version 3 or later",
                ));
            }
        }
        Ok(())
    }
    pub(crate) fn begin(
        format: MachineFormat,
        job: WireJob,
        writer: &mut dyn WriteSeek,
        limits: EncoderLimits,
        quantization: Quantization,
        cancellation: &mut Cancellation<'_>,
    ) -> Result<Self, CodecError> {
        cancellation.check(Checkpoint::ExportStart)?;
        if writer.stream_position()? != 0 {
            return Err(CodecError::Invalid(
                "machine output must start at byte zero",
            ));
        }
        Self::validate_metadata(format, &job)?;
        let mut encoder = Self {
            format,
            job,
            limits,
            quantization,
            count: 0,
            table: 0,
            position: 0,
            previous_z: 0.,
            encoded_volume_ml: 0.,
            print_parameters: 112,
            v4_block: None,
            stationary_s: 0.,
            workshop: None,
            lit_bounds: None,
        };
        let header = match format {
            MachineFormat::GooV3 => encoder.goo_header()?,
            MachineFormat::CtbV3Plain | MachineFormat::CtbV4Plain | MachineFormat::CtbV5Plain => {
                encoder.ctb_header()?
            }
            MachineFormat::PhotonWorkshop(target) => {
                let layout = super::photon_workshop::header(target, &encoder.job)?;
                if layout.bytes.len() > encoder.limits.max_metadata_bytes {
                    return Err(CodecError::Limit("Photon Workshop metadata budget"));
                }
                encoder.workshop = Some(WorkshopOffsets {
                    header: layout.header,
                    records: layout.records,
                    sub_records: layout.sub_records,
                    model: layout.model,
                });
                layout.bytes
            }
            _ => encoder.legacy_header()?,
        };
        encoder.append(writer, &header)?;
        Ok(encoder)
    }
    fn append(&mut self, writer: &mut dyn WriteSeek, data: &[u8]) -> Result<(), CodecError> {
        let end = self
            .position
            .checked_add(data.len() as u64)
            .ok_or(CodecError::Limit("output length overflow"))?;
        if end > self.limits.max_output_bytes || end > u64::from(u32::MAX) {
            return Err(CodecError::Limit(
                "output byte budget or 32-bit address space",
            ));
        }
        writer.write_all(data)?;
        self.position = end;
        Ok(())
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // Duration is range-checked as finite nonnegative u32 by metadata preflight.
    fn legacy_header(&mut self) -> Result<Vec<u8>, CodecError> {
        let ctb = self.format == MachineFormat::CtbV2Plain;
        let mut out = vec![0; 112];
        set_u32(&mut out, 0, if ctb { 0x12fd_0086 } else { 0x12fd_0019 });
        set_u32(&mut out, 4, if ctb { 2 } else { 1 });
        for i in 0..3 {
            set_f32(&mut out, 8 + i * 4, self.job.build_mm[i])?;
        }
        set_f32(&mut out, 32, self.job.layer_height_mm)?;
        set_f32(&mut out, 36, self.job.normal.exposure_s)?;
        set_f32(&mut out, 40, self.job.bottom.exposure_s)?;
        set_f32(&mut out, 44, self.job.normal.wait_before_exposure_s)?;
        set_u32(&mut out, 48, self.job.bottom_count);
        set_u32(&mut out, 52, self.job.resolution[0]);
        set_u32(&mut out, 56, self.job.resolution[1]);
        set_u32(&mut out, 68, self.job.layer_count);
        set_u32(&mut out, 76, self.job.estimated_time_s.ceil() as u32);
        // Masks already have their requested physical orientation; do not request a second firmware mirror.
        set_u32(&mut out, 80, 0);
        let previews = self.job.previews.as_ref();
        if ctb {
            set_u32(&mut out, 84, 112);
            set_u32(&mut out, 88, 60);
            set_u32(&mut out, 92, 1);
            out[96..98].copy_from_slice(&u16::from(self.job.normal.pwm).to_le_bytes());
            out[98..100].copy_from_slice(&u16::from(self.job.bottom.pwm).to_le_bytes());
            set_u32(&mut out, 104, 172);
            set_u32(&mut out, 108, 76);
            let mut p = vec![0; 60];
            for (at, n) in [
                (0, self.job.bottom.lift[0][0]),
                (4, self.job.bottom.lift[0][1] * 60.),
                (8, self.job.normal.lift[0][0]),
                (12, self.job.normal.lift[0][1] * 60.),
                (16, self.job.normal.retract[0][1] * 60.),
                (20, self.job.volume_ml),
                (32, self.job.bottom.wait_before_exposure_s),
                (36, self.job.normal.wait_before_exposure_s),
            ] {
                set_f32(&mut p, at, n)?;
            }
            set_u32(&mut p, 40, self.job.bottom_count);
            out.extend(p);
            let mut slicer = vec![0; 76];
            set_u32(&mut slicer, 28, 248);
            set_u32(
                &mut slicer,
                32,
                u32::try_from(self.job.machine_name.len())
                    .map_err(|_| CodecError::Limit("machine name"))?,
            );
            // Antialiasing flag: 0x0F for supersampled grey masks, 0x07 for binary ones.
            set_u32(
                &mut slicer,
                36,
                if self.job.antialiasing > 1 { 0x0f } else { 7 },
            );
            set_u32(&mut slicer, 44, 1);
            out.extend(slicer);
            if !self.job.machine_name.is_ascii() || self.job.machine_name.len() > 4096 {
                return Err(CodecError::Invalid(
                    "CTB machine name must be ASCII and bounded",
                ));
            }
            out.extend(self.job.machine_name.as_bytes());
        }
        if let Some(previews) = previews {
            check_previews(previews, preview_sizes(self.format))?;
            legacy_previews(&mut out, previews)?;
        }
        self.table = out.len() as u64;
        let table_size = (self.job.layer_count as usize)
            .checked_mul(36)
            .ok_or(CodecError::Limit("layer table"))?;
        if out
            .len()
            .checked_add(table_size)
            .is_none_or(|n| n > self.limits.max_metadata_bytes)
        {
            return Err(CodecError::Limit("legacy header/table metadata budget"));
        }
        if table_size as u64 > self.limits.max_output_bytes {
            return Err(CodecError::Limit("layer table budget"));
        }
        set_u32(&mut out, 64, checked_u32(self.table)?);
        out.try_reserve_exact(table_size)
            .map_err(|_| CodecError::Limit("layer table allocation"))?;
        out.resize(out.len() + table_size, 0);
        Ok(out)
    }
    /// Plain CTB v3, v4 or v5 sections and the zeroed layer table.
    fn ctb_header(&mut self) -> Result<Vec<u8>, CodecError> {
        let layout = super::ctb::header(self.format, &self.job)?;
        let mut out = layout.bytes;
        let table_size = (self.job.layer_count as usize)
            .checked_mul(36)
            .ok_or(CodecError::Limit("layer table"))?;
        if out
            .len()
            .checked_add(table_size)
            .is_none_or(|n| n > self.limits.max_metadata_bytes)
        {
            return Err(CodecError::Limit("CTB header/table metadata budget"));
        }
        out.try_reserve_exact(table_size)
            .map_err(|_| CodecError::Limit("layer table allocation"))?;
        out.resize(out.len() + table_size, 0);
        self.table = layout.table;
        self.print_parameters = layout.print_parameters;
        self.v4_block = layout.v4_block;
        Ok(out)
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // Duration is range-checked as finite nonnegative u32 by metadata preflight.
    fn goo_header(&self) -> Result<Vec<u8>, CodecError> {
        if self.limits.max_metadata_bytes < 195_477 {
            return Err(CodecError::Limit("GOO fixed header metadata budget"));
        }
        let mut out = Vec::with_capacity(195_477);
        out.extend(b"V3.0\x07\x00\x00\x00DLP\x00");
        text(&mut out, "karasu-slicer", 32)?;
        text(&mut out, env!("CARGO_PKG_VERSION"), 24)?;
        text(&mut out, "", 24)?;
        text(&mut out, &self.job.machine_name, 32)?;
        text(&mut out, &self.job.machine_name, 32)?;
        text(&mut out, "", 32)?;
        // Supersampling factor per axis, then grey and blur levels (no post-processing).
        out.extend(u16::from(self.job.antialiasing).to_be_bytes());
        out.extend(0u16.to_be_bytes());
        out.extend(0u16.to_be_bytes());
        // Big-endian RGB565 thumbnails of the model, or black when none were rendered.
        if let Some(previews) = &self.job.previews {
            check_previews(previews, preview_sizes(MachineFormat::GooV3))?;
            for image in [&previews.small, &previews.large] {
                for &p in image.rgb().as_chunks::<3>().0 {
                    out.extend(rgb565(p).to_be_bytes());
                }
                out.extend(b"\r\n");
            }
        } else {
            out.resize(out.len() + 116 * 116 * 2, 0);
            out.extend(b"\r\n");
            out.resize(out.len() + 290 * 290 * 2, 0);
            out.extend(b"\r\n");
        }
        out.extend(self.job.layer_count.to_be_bytes());
        for n in self.job.resolution {
            out.extend(
                u16::try_from(n)
                    .map_err(|_| CodecError::Limit("GOO dimensions exceed 65535"))?
                    .to_be_bytes(),
            );
        }
        // The stored image's mirroring along columns and rows (the format's x and y mirror).
        out.extend(self.job.stored_mirror.map(u8::from));
        for n in self
            .job
            .build_mm
            .into_iter()
            .chain([self.job.layer_height_mm, self.job.normal.exposure_s])
        {
            push_f32(&mut out, n, true)?;
        }
        out.push(1);
        push_f32(&mut out, 0., true)?;
        for s in [&self.job.bottom, &self.job.normal] {
            for n in [
                s.wait_after_exposure_s,
                s.wait_after_lift_s,
                s.wait_before_exposure_s,
            ] {
                push_f32(&mut out, n, true)?;
            }
        }
        push_f32(&mut out, self.job.bottom.exposure_s, true)?;
        out.extend(self.job.bottom_count.to_be_bytes());
        for stage in 0..2 {
            for direction in 0..2 {
                for s in [&self.job.bottom, &self.job.normal] {
                    let m = if direction == 0 {
                        s.lift[stage]
                    } else {
                        s.retract[stage]
                    };
                    push_f32(&mut out, m[0], true)?;
                    push_f32(&mut out, m[1] * 60., true)?;
                }
            }
        }
        out.extend(u16::from(self.job.bottom.pwm).to_be_bytes());
        out.extend(u16::from(self.job.normal.pwm).to_be_bytes());
        out.push(1);
        out.extend((self.job.estimated_time_s.ceil() as u32).to_be_bytes());
        for n in [self.job.volume_ml * 1000., 0., 0.] {
            push_f32(&mut out, n, true)?;
        }
        text(&mut out, "", 8)?;
        out.extend(195_477_u32.to_be_bytes());
        out.push(1);
        out.extend(
            u16::try_from(self.job.transition_count)
                .map_err(|_| CodecError::Limit("GOO transition count"))?
                .to_be_bytes(),
        );
        if out.len() != 195_477 {
            return Err(CodecError::Invalid("GOO header layout invariant"));
        }
        Ok(out)
    }
    #[allow(clippy::too_many_lines)] // Keep one-layer validation, encoding and wire publication adjacent.
    /// Format, quantization and limits of [`Self::encode`] for this session.
    pub(crate) const fn encoding_parameters(
        &self,
    ) -> (MachineFormat, Quantization, EncoderLimits, u8) {
        (
            self.format,
            self.quantization,
            self.limits,
            self.job.antialiasing,
        )
    }

    pub(crate) fn write_layer(
        &mut self,
        writer: &mut dyn WriteSeek,
        layer: &WireLayer,
        mask: &SpanMask,
        cancellation: &mut Cancellation<'_>,
    ) -> Result<(), CodecError> {
        cancellation.check(Checkpoint::ExportLayer)?;
        self.precheck(layer, [mask.width(), mask.height()])?;
        let payload = Self::encode(
            self.format,
            self.quantization,
            self.limits,
            self.job.antialiasing,
            mask,
            cancellation,
        )?;
        self.emit(writer, layer, &payload)
    }

    /// [`Self::write_layer`] for a payload encoded beforehand by [`Self::encode`] (possibly on
    /// another thread): the same checks in the same order, then the same bytes.
    pub(crate) fn write_encoded(
        &mut self,
        writer: &mut dyn WriteSeek,
        layer: &WireLayer,
        dimensions: [u32; 2],
        payload: Result<EncodedPayload, CodecError>,
        cancellation: &mut Cancellation<'_>,
    ) -> Result<(), CodecError> {
        cancellation.check(Checkpoint::ExportLayer)?;
        self.precheck(layer, dimensions)?;
        self.emit(writer, layer, &payload?)
    }

    /// Session-state checks of one layer, before any encoding.
    fn precheck(&self, layer: &WireLayer, dimensions: [u32; 2]) -> Result<(), CodecError> {
        if self.count >= self.job.layer_count {
            return Err(CodecError::Invalid("too many layers"));
        }
        if dimensions != self.job.resolution {
            return Err(CodecError::Invalid("mask dimensions do not match job"));
        }
        let pixel_count = u64::from(dimensions[0]) * u64::from(dimensions[1]);
        if pixel_count > self.limits.max_layer_bytes as u64 {
            return Err(CodecError::Limit("input layer byte budget"));
        }
        if layer.z_mm <= self.previous_z
            || layer.z_mm > self.job.build_mm[2]
            || f32_checked(layer.z_mm)? <= f32_checked(self.previous_z)?
        {
            return Err(CodecError::Invalid(
                "layer Z must increase within build height in f64 and f32",
            ));
        }
        Self::validate_layer(self.format, &self.job, layer, self.count)
    }

    /// Pure per-layer encoding: format quantization, the sample intensity sum and the run-length
    /// payload. It depends on no session state, so layers can be encoded on any thread.
    pub(crate) fn encode(
        format: MachineFormat,
        quantization: Quantization,
        limits: EncoderLimits,
        antialiasing: u8,
        mask: &SpanMask,
        cancellation: &mut Cancellation<'_>,
    ) -> Result<EncodedPayload, CodecError> {
        if let MachineFormat::PhotonWorkshop(target) = format {
            let image = super::photon_workshop::encode_image(
                target,
                antialiasing,
                quantization,
                mask,
                limits.max_layer_bytes,
                cancellation,
            )?;
            return Ok(EncodedPayload {
                bytes: image.bytes,
                intensity: image.intensity,
                lit: Some((image.lit, image.bounds)),
                scene: None,
                member: None,
            });
        }
        let round = quantization == Quantization::RoundToFormat && format != MachineFormat::GooV3;
        let map = |v: u8| -> u8 {
            if !round {
                v
            } else if format == MachineFormat::PhotonV1 {
                if v >= 128 {
                    255
                } else {
                    0
                }
            } else {
                match v {
                    0 | 1 => 0,
                    255 => 255,
                    _ => {
                        if v & 1 == 0 {
                            v + 1
                        } else {
                            v
                        }
                    }
                }
            }
        };
        let mut intensity = 0u64;
        let mut index = 0usize;
        mask.for_each_run(|value, length| {
            cancellation.poll(Checkpoint::ExportLayer, index)?;
            index += 1;
            intensity = (length as u64)
                .checked_mul(u64::from(map(value)))
                .and_then(|n| intensity.checked_add(n))
                .ok_or(CodecError::Limit("encoded intensity sum"))?;
            Ok::<_, CodecError>(())
        })?;
        let image = match format {
            MachineFormat::GooV3 => goo_rle(mask, cancellation, limits.max_layer_bytes)?,
            MachineFormat::CtbV2Plain
            | MachineFormat::CtbV3Plain
            | MachineFormat::CtbV4Plain
            | MachineFormat::CtbV5Plain => {
                ctb_rle(mask, map, false, cancellation, limits.max_layer_bytes)?
            }
            MachineFormat::PhotonV1 => photon_rle(mask, map, cancellation, limits.max_layer_bytes)?,
            MachineFormat::PhotonWorkshop(_) => unreachable!("encoded above"),
        };
        if image.len() > limits.max_layer_bytes {
            return Err(CodecError::Limit("encoded layer byte budget"));
        }
        Ok(EncodedPayload {
            bytes: image,
            intensity,
            lit: None,
            scene: None,
            member: None,
        })
    }

    /// Append an encoded layer and its table/prefix records, then advance the session.
    fn emit(
        &mut self,
        writer: &mut dyn WriteSeek,
        layer: &WireLayer,
        payload: &EncodedPayload,
    ) -> Result<(), CodecError> {
        let intensity = payload.intensity;
        let image = &payload.bytes;
        // A bounded sample sum is a numerical estimate, not an exact volume measurement.
        #[allow(clippy::cast_precision_loss)]
        let layer_volume_ml = (intensity as f64 / 255.)
            * (self.job.build_mm[0] / f64::from(self.job.resolution[0]))
            * (self.job.build_mm[1] / f64::from(self.job.resolution[1]))
            * (layer.z_mm - self.previous_z)
            / 1000.;
        let encoded_volume_ml = self.encoded_volume_ml + layer_volume_ml;
        if !encoded_volume_ml.is_finite() {
            return Err(CodecError::Invalid("encoded coverage volume overflow"));
        }
        if self.format == MachineFormat::GooV3 {
            let s = &layer.settings;
            let mut prefix = Vec::with_capacity(70);
            prefix.extend(0u16.to_be_bytes());
            for n in [
                self.job.build_mm[2],
                layer.z_mm,
                s.exposure_s,
                0.,
                s.wait_after_exposure_s,
                s.wait_after_lift_s,
                s.wait_before_exposure_s,
            ] {
                push_f32(&mut prefix, n, true)?;
            }
            for m in s.lift.into_iter().chain(s.retract) {
                push_f32(&mut prefix, m[0], true)?;
                push_f32(&mut prefix, m[1] * 60., true)?;
            }
            prefix.extend(u16::from(s.pwm).to_be_bytes());
            prefix.extend(b"\r\n");
            prefix.extend(checked_u32(image.len() as u64)?.to_be_bytes());
            self.append(writer, &prefix)?;
            self.append(writer, image)?;
            self.append(writer, b"\r\n")?;
        } else if let (Some(offsets), Some((lit, bounds))) = (self.workshop, payload.lit) {
            let thickness = layer.z_mm - self.previous_z;
            let record = super::photon_workshop::layer_record(
                layer,
                self.position,
                image.len(),
                thickness,
                lit,
            )?;
            writer.seek(SeekFrom::Start(
                offsets.records + 32 * u64::from(self.count),
            ))?;
            writer.write_all(&record)?;
            if let Some(sub) = offsets.sub_records {
                let record = super::photon_workshop::sub_record(self.position, image.len(), lit)?;
                writer.seek(SeekFrom::Start(sub + 44 * u64::from(self.count)))?;
                writer.write_all(&record)?;
            }
            writer.seek(SeekFrom::Start(self.position))?;
            self.append(writer, image)?;
            if let Some([lo, hi]) = bounds {
                self.lit_bounds = Some(match self.lit_bounds {
                    None => ([lo, hi], [layer.z_mm - thickness, layer.z_mm]),
                    Some(([a, b], [z0, _])) => (
                        [
                            [a[0].min(lo[0]), a[1].min(lo[1])],
                            [b[0].max(hi[0]), b[1].max(hi[1])],
                        ],
                        [z0, layer.z_mm],
                    ),
                });
            }
        } else if matches!(
            self.format,
            MachineFormat::CtbV3Plain | MachineFormat::CtbV4Plain | MachineFormat::CtbV5Plain
        ) {
            let (row, ex) =
                super::ctb::layer_records(self.format, layer, self.position, image.len())?;
            writer.seek(SeekFrom::Start(self.table + 36 * u64::from(self.count)))?;
            writer.write_all(&row)?;
            writer.seek(SeekFrom::Start(self.position))?;
            self.append(writer, &ex)?;
            self.append(writer, image)?;
            self.stationary_s += super::ctb::stationary_s(&layer.settings);
        } else {
            let mut row = vec![0; 36];
            set_f32(&mut row, 0, layer.z_mm)?;
            set_f32(&mut row, 4, layer.settings.exposure_s)?;
            set_f32(&mut row, 8, layer.settings.wait_before_exposure_s)?;
            set_u32(&mut row, 12, checked_u32(self.position)?);
            set_u32(&mut row, 16, checked_u32(image.len() as u64)?);
            writer.seek(SeekFrom::Start(self.table + 36 * u64::from(self.count)))?;
            writer.write_all(&row)?;
            writer.seek(SeekFrom::Start(self.position))?;
            self.append(writer, image)?;
        }
        self.encoded_volume_ml = encoded_volume_ml;
        self.previous_z = layer.z_mm;
        self.count += 1;
        Ok(())
    }
    /// Patch a Photon Workshop file's volume, weight and model bounds.
    fn finish_workshop(&self, writer: &mut dyn WriteSeek) -> Result<(), CodecError> {
        let Some(offsets) = self.workshop else {
            return Ok(());
        };
        // HEADER fields: volume (mL) at +52 and weight (g) at +68, from the section start.
        writer.seek(SeekFrom::Start(offsets.header + 52))?;
        writer.write_all(&f32_checked(self.encoded_volume_ml)?.to_le_bytes())?;
        writer.seek(SeekFrom::Start(offsets.header + 68))?;
        let weight = self.encoded_volume_ml * self.job.resin_density_g_per_ml;
        writer.write_all(&f32_checked(weight)?.to_le_bytes())?;
        if let Some(model) = offsets.model {
            // Lit-pixel bounds in panel millimetres (pixel edges), and lit layer heights.
            let pitch = [0, 1].map(|k| self.job.build_mm[k] / f64::from(self.job.resolution[k]));
            let bounds = self.lit_bounds.map(|([lo, hi], [z0, z1])| {
                [
                    [f64::from(lo[0]) * pitch[0], f64::from(lo[1]) * pitch[1], z0],
                    [
                        f64::from(hi[0] + 1) * pitch[0],
                        f64::from(hi[1] + 1) * pitch[1],
                        z1,
                    ],
                ]
            });
            writer.seek(SeekFrom::Start(model + 12))?;
            writer.write_all(&super::photon_workshop::model_body(bounds)?)?;
        }
        Ok(())
    }
    pub(crate) fn finish(
        mut self,
        writer: &mut dyn WriteSeek,
        cancellation: &mut Cancellation<'_>,
    ) -> Result<(), CodecError> {
        cancellation.check(Checkpoint::ExportFinish)?;
        if self.count != self.job.layer_count {
            return Err(CodecError::Invalid("missing layers"));
        }
        if self.format == MachineFormat::GooV3 {
            self.append(writer, b"\0\0\0\x07\0\0\0DLP\0")?;
        } else if self.format.is_ctb() {
            writer.seek(SeekFrom::Start(28))?;
            writer.write_all(&f32_checked(self.previous_z)?.to_le_bytes())?;
            writer.seek(SeekFrom::Start(self.position))?;
        }
        if let Some(block) = self.v4_block {
            writer.seek(SeekFrom::Start(block + 0x28))?;
            writer.write_all(&f32_checked(self.stationary_s)?.to_le_bytes())?;
        }
        match self.format {
            MachineFormat::CtbV2Plain
            | MachineFormat::CtbV3Plain
            | MachineFormat::CtbV4Plain
            | MachineFormat::CtbV5Plain => {
                writer.seek(SeekFrom::Start(self.print_parameters + 20))?;
                writer.write_all(&f32_checked(self.encoded_volume_ml)?.to_le_bytes())?;
            }
            MachineFormat::GooV3 => {
                writer.seek(SeekFrom::Start(195_450))?;
                writer.write_all(&f32_checked(self.encoded_volume_ml * 1000.)?.to_be_bytes())?;
            }
            MachineFormat::PhotonV1 => {}
            MachineFormat::PhotonWorkshop(_) => self.finish_workshop(writer)?,
        }
        writer.seek(SeekFrom::Start(self.position))?;
        writer.flush()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    fn settings() -> WireSettings {
        WireSettings {
            exposure_s: 2.,
            wait_before_exposure_s: 0.,
            wait_after_exposure_s: 0.,
            wait_after_lift_s: 0.,
            pwm: 255,
            lift: [[0., 0.]; 2],
            retract: [[0., 0.]; 2],
        }
    }
    fn job() -> WireJob {
        WireJob {
            resolution: [3, 2],
            build_mm: [3., 2., 10.],
            layer_height_mm: 0.05,
            bottom_count: 1,
            transition_count: 1,
            layer_count: 3,
            machine_name: "Test machine".into(),
            normal: settings(),
            bottom: WireSettings {
                exposure_s: 10.,
                ..settings()
            },
            estimated_time_s: 18.,
            volume_ml: 0.001,
            antialiasing: 1,
            previews: None,
            job_name: "test".into(),
            resin_name: "Test resin".into(),
            resin_density_g_per_ml: 1.1,
            heating_temperature_c: 25.0,
            stored_mirror: [false; 2],
        }
    }
    fn limits() -> EncoderLimits {
        EncoderLimits {
            max_output_bytes: 1_000_000,
            max_metadata_bytes: 1_000_000,
            max_layer_bytes: 1024,
        }
    }
    fn encode(format: MachineFormat) -> Vec<u8> {
        encode_job(format, job())
    }
    fn encode_job(format: MachineFormat, j: WireJob) -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        let mut cancel = Cancellation::never();
        let mut encoder = MachineEncoder::begin(
            format,
            j,
            &mut out,
            limits(),
            Quantization::Exact,
            &mut cancel,
        )
        .unwrap();
        for (i, exposure) in [10., 6., 2.].into_iter().enumerate() {
            let layer = WireLayer {
                z_mm: f64::from(u32::try_from(i + 1).unwrap()) * 0.05,
                settings: WireSettings {
                    exposure_s: exposure,
                    ..settings()
                },
            };
            encoder
                .write_layer(
                    &mut out,
                    &layer,
                    &SpanMask::from_pixels(3, 2, &[0, 255, 0, 255, 255, 0]).unwrap(),
                    &mut cancel,
                )
                .unwrap();
        }
        encoder.finish(&mut out, &mut cancel).unwrap();
        out.into_inner()
    }
    /// Two-stage motion with rest times, as CTB v4/v5 and GOO store them.
    fn staged(exposure_s: f64) -> WireSettings {
        WireSettings {
            exposure_s,
            wait_before_exposure_s: 0.5,
            wait_after_exposure_s: 0.25,
            wait_after_lift_s: 0.75,
            pwm: 200,
            lift: [[2., 1.], [3., 0.5]],
            retract: [[3., 2.], [2., 0.25]],
        }
    }
    fn encode_layers(format: MachineFormat, j: WireJob, layers: &[WireSettings]) -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        let mut cancel = Cancellation::never();
        let mut encoder = MachineEncoder::begin(
            format,
            j,
            &mut out,
            limits(),
            Quantization::Exact,
            &mut cancel,
        )
        .unwrap();
        for (i, settings) in layers.iter().enumerate() {
            let layer = WireLayer {
                z_mm: f64::from(u32::try_from(i + 1).unwrap()) * 0.05,
                settings: settings.clone(),
            };
            let mask = SpanMask::from_pixels(3, 2, &[0, 255, 0, 255, 255, 0]).unwrap();
            encoder
                .write_layer(&mut out, &layer, &mask, &mut cancel)
                .unwrap();
        }
        encoder.finish(&mut out, &mut cancel).unwrap();
        out.into_inner()
    }
    fn f32_at(b: &[u8], at: usize) -> f32 {
        f32::from_le_bytes(b[at..at + 4].try_into().unwrap())
    }
    #[test]
    fn photon_workshop_revisions_sections_records_and_images() {
        let tag = |b: &[u8], at: usize, name: &str| {
            let mut field = [0u8; 12];
            field[..name.len()].copy_from_slice(name.as_bytes());
            assert_eq!(&b[at..at + 12], &field, "{name}");
        };
        for target in [
            AnycubicWorkshop::Pws,
            AnycubicWorkshop::Pwmx,
            AnycubicWorkshop::Pmsq,
            AnycubicWorkshop::Pwma,
            AnycubicWorkshop::Pm5,
            AnycubicWorkshop::Pm5s,
        ] {
            let version = target.revision();
            let b = encode(MachineFormat::PhotonWorkshop(target));
            assert_eq!(&b[..12], b"ANYCUBIC\0\0\0\0");
            assert_eq!(u32_at(&b, 12), version as usize);
            let slot = |i: usize| u32_at(&b, 20 + 4 * i);
            tag(&b, slot(0), "HEADER");
            tag(&b, slot(2), "PREVIEW");
            let layerdef = slot(4);
            tag(&b, layerdef, "LAYERDEF");
            assert_eq!(u32_at(&b, layerdef + 16), 3);
            if version >= 516 {
                tag(&b, slot(5), "EXTRA");
                tag(&b, slot(6), "MACHINE");
            }
            if version >= 517 {
                tag(&b, slot(8), "MODEL");
                // The model's lit bounds: pixels 0..2 x 0..2 at 1 mm, Z 0 to 0.15 mm.
                let model = slot(8) + 12;
                let bounds: Vec<f32> = (0..6).map(|k| f32_at(&b, model + 4 * k)).collect();
                assert_eq!(bounds, [0.0, 0.0, 0.0, 2.0, 2.0, 0.15]);
            }
            if version == 518 {
                tag(&b, slot(9), "SUBIMGS");
            }
            // Images follow each other to the end of the file, as the records say.
            let images = if version >= 516 { slot(7) } else { slot(6) };
            let mut expected = images;
            for i in 0..3 {
                let record = layerdef + 20 + 32 * i;
                assert_eq!(u32_at(&b, record), expected);
                expected += u32_at(&b, record + 4);
                assert!((f32_at(&b, record + 20) - 0.05).abs() < 1e-6, "thickness");
                assert_eq!(u32_at(&b, record + 24), 3, "lit pixels");
            }
            assert_eq!(expected, b.len());
            // Header: exposure, bottom count (as f32), volume patched at the end.
            let header = slot(0);
            assert_eq!(f32_at(&b, header + 24), 2.0);
            assert_eq!(f32_at(&b, header + 36), 1.0);
            assert!((f32_at(&b, header + 52) - 0.00045).abs() < 1e-10);
            if target.bitplanes() {
                // One plane for binary masks: lit runs set bit 7.
                let first = u32_at(&b, layerdef + 20);
                assert_eq!(&b[first..first + 4], &[0x01, 0x81, 0x01, 0x82]);
            }
        }
        // Capabilities: no PWM, no rest times, two stages from revision 516, mirrored retract.
        let two_stage = WireSettings {
            lift: [[2., 1.], [3., 4.]],
            retract: [[3., 5.], [2., 0.5]],
            ..settings()
        };
        let job_with = |s: WireSettings| WireJob {
            normal: s.clone(),
            bottom: WireSettings {
                exposure_s: 10.,
                ..s
            },
            ..job()
        };
        let check = |target, s: WireSettings| {
            MachineEncoder::validate_metadata(MachineFormat::PhotonWorkshop(target), &job_with(s))
        };
        assert!(check(AnycubicWorkshop::Pwma, two_stage.clone()).is_ok());
        assert!(check(AnycubicWorkshop::Pwmx, two_stage.clone()).is_err());
        let unmirrored = WireSettings {
            retract: [[2., 5.], [3., 0.5]],
            ..two_stage
        };
        assert!(check(AnycubicWorkshop::Pwma, unmirrored).is_err());
        assert!(check(
            AnycubicWorkshop::Pm5,
            WireSettings {
                pwm: 200,
                ..settings()
            }
        )
        .is_err());
        let rested = WireSettings {
            wait_after_lift_s: 1.,
            ..settings()
        };
        assert!(check(AnycubicWorkshop::Pm5, rested).is_err());
    }

    #[test]
    fn photon_workshop_bitplanes_round_gray_to_planes() {
        let mask = SpanMask::from_pixels(5, 1, &[0, 63, 128, 191, 255]).unwrap();
        let encode_planes = |quantization| {
            MachineEncoder::encode(
                MachineFormat::PhotonWorkshop(AnycubicWorkshop::Pws),
                quantization,
                limits(),
                4,
                &mask,
                &mut Cancellation::never(),
            )
        };
        // Four planes store grays 0, 63, 127, 191 and 255: 128 is not exact.
        assert!(matches!(
            encode_planes(Quantization::Exact),
            Err(CodecError::Unsupported(_))
        ));
        let payload = encode_planes(Quantization::RoundToFormat).unwrap();
        // Plane i lights pixels with at least i of 4: [0, 1, 2, 3, 4] lit planes.
        assert_eq!(
            payload.bytes,
            [0x01, 0x84, 0x02, 0x83, 0x03, 0x82, 0x04, 0x81]
        );
        assert_eq!(payload.lit, Some((10, Some([[1, 0], [4, 0]]))));
        assert_eq!(payload.intensity, 63 + 127 + 191 + 255);
    }

    #[test]
    #[allow(clippy::too_many_lines)] // One layout walked field by field across three versions.
    fn plain_ctb_versions_3_to_5_records_and_sections() {
        let plain = encode(MachineFormat::CtbV2Plain);
        let v2_table = u32_at(&plain, 64);
        for format in [
            MachineFormat::CtbV3Plain,
            MachineFormat::CtbV4Plain,
            MachineFormat::CtbV5Plain,
        ] {
            let version = super::super::ctb::version(format);
            let staged_job = version >= 4;
            let j = if staged_job {
                WireJob {
                    normal: staged(2.),
                    bottom: staged(10.),
                    ..job()
                }
            } else {
                job()
            };
            let layers: Vec<WireSettings> = if staged_job {
                vec![
                    staged(10.),
                    WireSettings {
                        pwm: 150,
                        ..staged(6.)
                    },
                    staged(2.),
                ]
            } else {
                vec![
                    j.bottom.clone(),
                    WireSettings {
                        pwm: 150,
                        ..j.normal.clone()
                    },
                    j.normal.clone(),
                ]
            };
            let b = encode_layers(format, j, &layers);
            assert_eq!(
                u32_at(&b, 0),
                if version >= 4 {
                    0x12fd_0106
                } else {
                    0x12fd_0086
                }
            );
            assert_eq!(u32_at(&b, 4), version as usize);
            assert_eq!(u32_at(&b, 100), 0, "XOR seed 0: plain images");
            let table = u32_at(&b, 64);
            for (i, settings) in layers.iter().enumerate() {
                let row = table + 36 * i;
                assert_eq!((u32_at(&b, row + 20), u32_at(&b, row + 24)), (0, 84));
                let image = u32_at(&b, row + 12);
                let len = u32_at(&b, row + 16);
                // The LayerDefEx before the image repeats the row and holds the record size.
                assert_eq!(&b[image - 84..image - 48], &b[row..row + 36]);
                assert_eq!(u32_at(&b, image - 84 + 0x24), 84 + len);
                let ex = image - 84;
                assert_eq!(f32_at(&b, ex + 0x50), f32::from(settings.pwm));
                let total_lift = settings.lift[0][0] + settings.lift[1][0];
                assert!((f64::from(f32_at(&b, ex + 0x28)) - total_lift).abs() < 1e-6);
                if version >= 4 {
                    assert_eq!(
                        f32_at(&b, row + 8),
                        0.0,
                        "light-off is zero; rest times are used"
                    );
                    assert_eq!(f32_at(&b, ex + 0x30), 3.0);
                    assert_eq!(f32_at(&b, ex + 0x34), 30.0);
                    assert_eq!(f32_at(&b, ex + 0x3c), 2.0);
                    assert_eq!(f32_at(&b, ex + 0x44), 0.25);
                    assert_eq!(f32_at(&b, ex + 0x48), 0.75);
                    assert_eq!(f32_at(&b, ex + 0x4c), 0.5);
                }
                // The image bytes equal the version-2 encoding of the same mask.
                let v2_image = u32_at(&plain, v2_table + 36 * i + 12);
                assert_eq!(&b[image..image + len], &plain[v2_image..v2_image + len]);
            }
            let slicer = u32_at(&b, 104);
            assert_eq!(u32_at(&b, slicer + 0x3c), 1, "transition count");
            assert_eq!(b[slicer + 0x24], 0x07);
            if version >= 4 {
                // Black previews stand in; the notice is karasu's own text.
                assert!(u32_at(&b, 60) != 0 && u32_at(&b, 72) != 0);
                let block = u32_at(&b, slicer + 0x40);
                let notice = u32_at(&b, block + 0x48);
                assert_eq!(u32_at(&b, block + 0x4c), 320);
                assert!(b[notice..notice + 320].starts_with(b"Written by karasu-slicer"));
                assert_eq!(u32_at(&b, block + 0x34), 3);
                // Stationary time: exposures 18 s plus 1.5 s of waits per layer.
                assert!((f32_at(&b, block + 0x28) - 22.5).abs() < 1e-5);
                let resin = u32_at(&b, block + 0x50);
                assert_eq!(resin != 0, version == 5);
                if resin != 0 {
                    assert!((f32_at(&b, resin + 0x20) - 1.1).abs() < 1e-6);
                    let (at, len) = (u32_at(&b, resin + 8), u32_at(&b, resin + 12));
                    assert_eq!(&b[at..at + len], b"Test resin");
                }
            }
            // The encoded volume is patched into the print parameters.
            let parameters = u32_at(&b, 84);
            assert!((f32_at(&b, parameters + 20) - 0.00045).abs() < 1e-10);
        }
        // Version 3 has one motion stage; two stages and rest times need version 4 or GOO.
        let staged_v3 = WireJob {
            normal: staged(2.),
            bottom: staged(10.),
            ..job()
        };
        let result = MachineEncoder::begin(
            MachineFormat::CtbV3Plain,
            staged_v3,
            &mut Cursor::new(Vec::new()),
            limits(),
            Quantization::Exact,
            &mut Cancellation::never(),
        );
        assert!(matches!(result, Err(CodecError::Unsupported(_))));
        // Version 4 needs a machine name.
        let unnamed = WireJob {
            machine_name: String::new(),
            ..job()
        };
        assert!(MachineEncoder::validate_metadata(MachineFormat::CtbV4Plain, &unnamed).is_err());
    }
    #[test]
    fn headers_tables_footer_and_layer_metadata() {
        for format in [MachineFormat::CtbV2Plain, MachineFormat::PhotonV1] {
            let b = encode(format);
            assert_eq!(
                &b[4..8],
                &(if format == MachineFormat::PhotonV1 {
                    1u32
                } else {
                    2
                })
                .to_le_bytes()
            );
            if format == MachineFormat::CtbV2Plain {
                let encoded = f32::from_le_bytes(b[132..136].try_into().unwrap());
                assert!((encoded - 0.00045).abs() < 1e-10);
            }
            let table = u32::from_le_bytes(b[64..68].try_into().unwrap()) as usize;
            for (i, exposure) in [10f32, 6., 2.].into_iter().enumerate() {
                let row = &b[table + 36 * i..table + 36 * (i + 1)];
                assert_eq!(f32::from_le_bytes(row[4..8].try_into().unwrap()), exposure);
                let at = u32::from_le_bytes(row[12..16].try_into().unwrap()) as usize;
                let n = u32::from_le_bytes(row[16..20].try_into().unwrap()) as usize;
                assert!(at + n <= b.len());
            }
        }
        let b = encode(MachineFormat::GooV3);
        assert_eq!(&b[..12], b"V3.0\x07\0\0\0DLP\0");
        assert_eq!(&b[b.len() - 11..], b"\0\0\0\x07\0\0\0DLP\0");
        assert_eq!(
            f32::from_be_bytes(b[195_487..195_491].try_into().unwrap()),
            10.
        );
        assert_eq!(b[195_445], 1);
        assert!((f32::from_be_bytes(b[195_450..195_454].try_into().unwrap()) - 0.45).abs() < 1e-6);
    }
    /// Upper half white (long runs), lower half a horizontal gradient (single pixels).
    fn preview([width, height]: [u32; 2]) -> PreviewImage {
        let mut rgb = Vec::new();
        for y in 0..height {
            for x in 0..width {
                let level = u8::try_from(x * 255 / width).unwrap();
                rgb.extend(if y < height / 2 {
                    [255, 255, 255]
                } else {
                    [level, 255 - level, 128]
                });
            }
        }
        PreviewImage::new(width, height, rgb).unwrap()
    }
    fn previews(format: MachineFormat) -> Previews {
        let [small, large] = preview_sizes(format);
        Previews {
            small: preview(small),
            large: preview(large),
        }
    }
    fn u32_at(b: &[u8], at: usize) -> usize {
        u32::from_le_bytes(b[at..at + 4].try_into().unwrap()) as usize
    }
    /// Independent RLE15 decoding to colour words.
    fn decode_rle15(data: &[u8]) -> Vec<u16> {
        let words: Vec<u16> = data
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&w| u16::from_le_bytes(w))
            .collect();
        let mut out = Vec::new();
        let mut at = 0;
        while at < words.len() {
            let word = words[at];
            at += 1;
            if word & 0x20 == 0 {
                out.push(word);
            } else {
                assert_eq!(words[at] & 0xf000, 0x3000);
                let run = usize::from(words[at] & 0x0fff) + 1;
                assert!(run >= 2);
                out.extend(std::iter::repeat_n(word & !0x20, run));
                at += 1;
            }
        }
        out
    }
    #[test]
    fn previews_are_stored_in_every_legacy_and_goo_layout() {
        for format in [MachineFormat::CtbV2Plain, MachineFormat::PhotonV1] {
            let plain = encode(format);
            assert_eq!((u32_at(&plain, 60), u32_at(&plain, 72)), (0, 0));
            let wanted = previews(format);
            let b = encode_job(
                format,
                WireJob {
                    previews: Some(wanted.clone()),
                    ..job()
                },
            );
            let table = u32_at(&b, 64);
            for (at, image) in [(60, &wanted.large), (72, &wanted.small)] {
                let descriptor = u32_at(&b, at);
                assert_eq!(
                    [u32_at(&b, descriptor), u32_at(&b, descriptor + 4)],
                    [image.width() as usize, image.height() as usize]
                );
                let (data, n) = (u32_at(&b, descriptor + 8), u32_at(&b, descriptor + 12));
                assert_eq!(data, descriptor + 32);
                assert!(data + n <= table);
                let expected: Vec<u16> = image
                    .rgb()
                    .as_chunks::<3>()
                    .0
                    .iter()
                    .map(|&p| rgb15(p))
                    .collect();
                assert_eq!(decode_rle15(&b[data..data + n]), expected);
            }
            // Layers are unchanged by the previews, only moved.
            let shift = table - u32_at(&plain, 64);
            assert_eq!(b.len() - plain.len(), shift);
            assert_eq!(&b[table + 36 * 3..], &plain[table - shift + 36 * 3..]);
        }
        let wanted = previews(MachineFormat::GooV3);
        let b = encode_job(
            MachineFormat::GooV3,
            WireJob {
                previews: Some(wanted.clone()),
                ..job()
            },
        );
        let plain = encode(MachineFormat::GooV3);
        let mut at = 194;
        for image in [&wanted.small, &wanted.large] {
            for &p in image.rgb().as_chunks::<3>().0 {
                assert_eq!(b[at..at + 2], rgb565(p).to_be_bytes());
                at += 2;
            }
            assert_eq!(&b[at..at + 2], b"\r\n");
            at += 2;
        }
        assert!(plain[194..at]
            .iter()
            .all(|&byte| byte == 0 || byte == b'\r' || byte == b'\n'));
        assert_eq!((&b[..194], &b[at..]), (&plain[..194], &plain[at..]));
    }
    #[test]
    fn preview_sizes_must_match_the_format() {
        for format in [
            MachineFormat::CtbV2Plain,
            MachineFormat::PhotonV1,
            MachineFormat::GooV3,
        ] {
            let mut swapped = previews(format);
            std::mem::swap(&mut swapped.small, &mut swapped.large);
            let result = MachineEncoder::begin(
                format,
                WireJob {
                    previews: Some(swapped),
                    ..job()
                },
                &mut Cursor::new(Vec::new()),
                limits(),
                Quantization::Exact,
                &mut Cancellation::never(),
            );
            assert!(matches!(result, Err(CodecError::Invalid(_))));
        }
    }
    #[test]
    fn cancellation_and_partial_jobs_do_not_succeed() {
        let mut out = Cursor::new(Vec::new());
        let mut predicate = |point| point == Checkpoint::ExportStart;
        let mut cancel = Cancellation::new(&mut predicate);
        assert!(matches!(
            MachineEncoder::begin(
                MachineFormat::GooV3,
                job(),
                &mut out,
                limits(),
                Quantization::Exact,
                &mut cancel
            ),
            Err(CodecError::Cancelled(_))
        ));
        assert_eq!(out.get_ref().len(), 0);
        let mut cancel = Cancellation::never();
        let encoder = MachineEncoder::begin(
            MachineFormat::GooV3,
            job(),
            &mut out,
            limits(),
            Quantization::Exact,
            &mut cancel,
        )
        .unwrap();
        assert!(matches!(
            encoder.finish(&mut out, &mut cancel),
            Err(CodecError::Invalid(_))
        ));
    }
    #[test]
    fn format_limits_and_unsupported_settings_fail_before_output() {
        let mut j = job();
        j.normal.wait_after_lift_s = 1.;
        let mut out = Cursor::new(Vec::new());
        assert!(matches!(
            MachineEncoder::begin(
                MachineFormat::CtbV2Plain,
                j,
                &mut out,
                limits(),
                Quantization::Exact,
                &mut Cancellation::never()
            ),
            Err(CodecError::Unsupported(_))
        ));
        assert_eq!(out.get_ref().len(), 0);
        let mut j = job();
        j.resolution = [65536, 1];
        assert!(matches!(
            MachineEncoder::begin(
                MachineFormat::GooV3,
                j,
                &mut out,
                limits(),
                Quantization::Exact,
                &mut Cancellation::never()
            ),
            Err(CodecError::Limit(_))
        ));
        assert_eq!(out.get_ref().len(), 0);
    }
    #[test]
    fn explicit_quantization_and_exact_rejection() {
        for format in [MachineFormat::CtbV2Plain, MachineFormat::PhotonV1] {
            for policy in [Quantization::Exact, Quantization::RoundToFormat] {
                let mut out = Cursor::new(Vec::new());
                let j = job();
                let mut cancel = Cancellation::never();
                let layer = WireLayer {
                    z_mm: 0.05,
                    settings: j.bottom.clone(),
                };
                let mut encoder =
                    MachineEncoder::begin(format, j, &mut out, limits(), policy, &mut cancel)
                        .unwrap();
                let result = encoder.write_layer(
                    &mut out,
                    &layer,
                    &SpanMask::from_pixels(3, 2, &[0, 2, 127, 128, 254, 255]).unwrap(),
                    &mut cancel,
                );
                if policy == Quantization::Exact {
                    assert!(matches!(result, Err(CodecError::Unsupported(_))));
                } else {
                    result.unwrap();
                    let b = out.get_ref();
                    let table = u32::from_le_bytes(b[64..68].try_into().unwrap()) as usize;
                    let at =
                        u32::from_le_bytes(b[table + 12..table + 16].try_into().unwrap()) as usize;
                    if format == MachineFormat::CtbV2Plain {
                        assert_eq!(&b[at..], &[0, 1, 63, 64, 255, 2]);
                    } else {
                        assert_eq!(&b[at..], &[3, 131]);
                    }
                }
            }
        }
    }

    #[test]
    fn short_write_preserves_underlying_failure() {
        struct Failing(Cursor<Vec<u8>>);
        impl std::io::Write for Failing {
            fn write(&mut self, _: &[u8]) -> io::Result<usize> {
                Err(io::Error::new(
                    io::ErrorKind::StorageFull,
                    "injected full device",
                ))
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        impl std::io::Seek for Failing {
            fn seek(&mut self, p: SeekFrom) -> io::Result<u64> {
                self.0.seek(p)
            }
        }
        let mut out = Failing(Cursor::new(Vec::new()));
        assert!(
            matches!(MachineEncoder::begin(MachineFormat::GooV3,job(),&mut out,limits(),Quantization::Exact,&mut Cancellation::never()),Err(CodecError::Io(ref e)) if e.kind()==io::ErrorKind::StorageFull)
        );
    }
}
