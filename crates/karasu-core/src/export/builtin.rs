//! Adapters between immutable jobs and independently specified machine encoders.
use super::{
    wire_settings, EncodedLayer, ExportError, ExportLimits, ExportSession, LayerEncoder,
    MachineExporter, WriteSeek,
};
use crate::{
    cancellation::{Cancellation, Checkpoint},
    preview::Previews,
    print_job::{
        machine::{
            preview_sizes, CodecError, EncoderLimits, MachineEncoder, MachineFormat, Quantization,
            WireJob, WireLayer,
        },
        pp1::{Pp1Encoder, PP1_PREVIEW_SIZES},
        sl1::{Sl1Encoder, Sl1Model, SL1_PREVIEW_SIZES},
        JobLayer, PrintJob,
    },
    raster::RasterImage,
};
/// Exact supported machine dialects; suffixes do not imply hardware interchangeability.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuiltinFormat {
    /// Unencrypted CTB version two. Reader-tested; encrypted-only printer firmware is unsupported.
    CtbV2Plain,
    /// Unencrypted CTB version three: per-layer motion and PWM records (TD-063).
    CtbV3Plain,
    /// Unencrypted CTB version four: two-stage motion and rest times (TD-063). Encrypted CTB,
    /// which some firmware requires, is not supported.
    CtbV4Plain,
    /// Unencrypted CTB version five: version four plus resin parameters (TD-063).
    CtbV5Plain,
    /// Elegoo GOO version three with per-layer advanced settings and grayscale8.
    GooV3,
    /// Anycubic Photon P1 version-three ZIP with native layer images; not P1 Max.
    AnycubicPhotonP1,
    /// Anycubic Photon Workshop binary file of one extension and revision (TD-063).
    AnycubicWorkshop(AnycubicWorkshop),
    /// Legacy Photon version one, explicitly delegating all motion to printer firmware.
    /// Job motion is used only for estimates; no motion instructions exist in this dialect.
    PhotonV1FirmwareMotion,
    /// Prusa SL1 archive (TD-063). The firmware manages motion and waits; job motion is used
    /// only for estimates.
    PrusaSl1FirmwareMotion,
    /// Prusa SL1S archive (TD-063), as [`Self::PrusaSl1FirmwareMotion`].
    PrusaSl1sFirmwareMotion,
}

/// Which encoder family serves a format.
#[derive(Clone, Copy)]
enum Kind {
    Machine(MachineFormat),
    Pp1,
    Sl1(Sl1Model),
}
/// Anycubic Photon Workshop binary targets (TD-063): one per file extension, at the revision
/// karasu writes for it. Revisions follow Lychee's minimal layout where references disagree;
/// printer acceptance is unverified until physical tests.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnycubicWorkshop {
    /// `.pws` revision 1: Photon / Photon S (bit-planes).
    Pws,
    /// `.pw0` revision 1: Photon Zero.
    Pw0,
    /// `.pwx` revision 1: Photon X.
    Pwx,
    /// `.pwmo` revision 1: Photon Mono.
    Pwmo,
    /// `.pwms` revision 1: Photon Mono SE.
    Pwms,
    /// `.pwmx` revision 1: Photon Mono X.
    Pwmx,
    /// `.pmsq` revision 515: Photon Mono SQ.
    Pmsq,
    /// `.dlp` revision 515: Photon Ultra.
    Dlp,
    /// `.pwma` revision 516: Photon Mono 4K.
    Pwma,
    /// `.pm3` revision 516: Photon M3.
    Pm3,
    /// `.pm3m` revision 516: Photon M3 Max.
    Pm3m,
    /// `.pwmb` revision 516: Photon Mono X 6K / M3 Plus.
    Pwmb,
    /// `.pm3r` revision 516: Photon M3 Premium.
    Pm3r,
    /// `.dl2p` revision 516: Photon D2.
    Dl2p,
    /// `.pmx2` revision 516: Photon Mono X2.
    Pmx2,
    /// `.pm3n` revision 517: Photon Mono 2.
    Pm3n,
    /// `.pm5` revision 517: Photon Mono M5.
    Pm5,
    /// `.px6s` revision 517: Photon Mono X 6Ks.
    Px6s,
    /// `.pm4n` revision 517: Photon Mono 4.
    Pm4n,
    /// `.pm5s` revision 518: Photon Mono M5s.
    Pm5s,
    /// `.m5sp` revision 518: Photon Mono M5s Pro.
    M5sp,
}

impl AnycubicWorkshop {
    /// Every target, in a stable order.
    pub const ALL: [Self; 21] = [
        Self::Pws,
        Self::Pw0,
        Self::Pwx,
        Self::Pwmo,
        Self::Pwms,
        Self::Pwmx,
        Self::Pmsq,
        Self::Dlp,
        Self::Pwma,
        Self::Pm3,
        Self::Pm3m,
        Self::Pwmb,
        Self::Pm3r,
        Self::Dl2p,
        Self::Pmx2,
        Self::Pm3n,
        Self::Pm5,
        Self::Px6s,
        Self::Pm4n,
        Self::Pm5s,
        Self::M5sp,
    ];
    /// File extension without a dot.
    #[must_use]
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Pws => "pws",
            Self::Pw0 => "pw0",
            Self::Pwx => "pwx",
            Self::Pwmo => "pwmo",
            Self::Pwms => "pwms",
            Self::Pwmx => "pwmx",
            Self::Pmsq => "pmsq",
            Self::Dlp => "dlp",
            Self::Pwma => "pwma",
            Self::Pm3 => "pm3",
            Self::Pm3m => "pm3m",
            Self::Pwmb => "pwmb",
            Self::Pm3r => "pm3r",
            Self::Dl2p => "dl2p",
            Self::Pmx2 => "pmx2",
            Self::Pm3n => "pm3n",
            Self::Pm5 => "pm5",
            Self::Px6s => "px6s",
            Self::Pm4n => "pm4n",
            Self::Pm5s => "pm5s",
            Self::M5sp => "m5sp",
        }
    }
    /// File revision (1, 515, 516, 517 or 518).
    #[must_use]
    pub const fn revision(self) -> u32 {
        match self {
            Self::Pws | Self::Pw0 | Self::Pwx | Self::Pwmo | Self::Pwms | Self::Pwmx => 1,
            Self::Pmsq | Self::Dlp => 515,
            Self::Pwma
            | Self::Pm3
            | Self::Pm3m
            | Self::Pwmb
            | Self::Pm3r
            | Self::Dl2p
            | Self::Pmx2 => 516,
            Self::Pm3n | Self::Pm5 | Self::Px6s | Self::Pm4n => 517,
            Self::Pm5s | Self::M5sp => 518,
        }
    }
    /// Stable format identifier, `anycubic-<extension>-v<revision>`.
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::Pws => "anycubic-pws-v1",
            Self::Pw0 => "anycubic-pw0-v1",
            Self::Pwx => "anycubic-pwx-v1",
            Self::Pwmo => "anycubic-pwmo-v1",
            Self::Pwms => "anycubic-pwms-v1",
            Self::Pwmx => "anycubic-pwmx-v1",
            Self::Pmsq => "anycubic-pmsq-v515",
            Self::Dlp => "anycubic-dlp-v515",
            Self::Pwma => "anycubic-pwma-v516",
            Self::Pm3 => "anycubic-pm3-v516",
            Self::Pm3m => "anycubic-pm3m-v516",
            Self::Pwmb => "anycubic-pwmb-v516",
            Self::Pm3r => "anycubic-pm3r-v516",
            Self::Dl2p => "anycubic-dl2p-v516",
            Self::Pmx2 => "anycubic-pmx2-v516",
            Self::Pm3n => "anycubic-pm3n-v517",
            Self::Pm5 => "anycubic-pm5-v517",
            Self::Px6s => "anycubic-px6s-v517",
            Self::Pm4n => "anycubic-pm4n-v517",
            Self::Pm5s => "anycubic-pm5s-v518",
            Self::M5sp => "anycubic-m5sp-v518",
        }
    }
    /// Printer models that use this target.
    #[must_use]
    pub const fn models(self) -> &'static str {
        match self {
            Self::Pws => "Photon / Photon S (bit-planes)",
            Self::Pw0 => "Photon Zero",
            Self::Pwx => "Photon X",
            Self::Pwmo => "Photon Mono",
            Self::Pwms => "Photon Mono SE",
            Self::Pwmx => "Photon Mono X",
            Self::Pmsq => "Photon Mono SQ",
            Self::Dlp => "Photon Ultra",
            Self::Pwma => "Photon Mono 4K",
            Self::Pm3 => "Photon M3",
            Self::Pm3m => "Photon M3 Max",
            Self::Pwmb => "Photon Mono X 6K / M3 Plus",
            Self::Pm3r => "Photon M3 Premium",
            Self::Dl2p => "Photon D2",
            Self::Pmx2 => "Photon Mono X2",
            Self::Pm3n => "Photon Mono 2",
            Self::Pm5 => "Photon Mono M5",
            Self::Px6s => "Photon Mono X 6Ks",
            Self::Pm4n => "Photon Mono 4",
            Self::Pm5s => "Photon Mono M5s",
            Self::M5sp => "Photon Mono M5s Pro",
        }
    }
    /// Whether layers are antialiasing bit-planes (`.pws`) rather than PW0.
    #[must_use]
    pub const fn bitplanes(self) -> bool {
        matches!(self, Self::Pws)
    }
}

/// Explicit policy for intensity values not representable by a machine dialect.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PixelPolicy {
    /// Reject any value whose native-format round trip changes the source intensity.
    Exact,
    /// Nearest native level; ties upward. Legacy binary Photon uses threshold 128.
    RoundToFormat,
}
/// One concrete versioned exporter and explicitly chosen pixel policy.
pub struct BuiltinExporter {
    format: BuiltinFormat,
    pixels: PixelPolicy,
    antialiasing: u8,
    previews: Option<Previews>,
}
impl BuiltinExporter {
    /// Select a dialect and its intensity-loss policy explicitly, for binary masks (one sample
    /// per pixel).
    #[must_use]
    pub const fn new(format: BuiltinFormat, pixels: PixelPolicy) -> Self {
        Self {
            format,
            pixels,
            antialiasing: 1,
            previews: None,
        }
    }
    /// The `[small, large]` preview sizes (`[width, height]`) this format stores.
    #[must_use]
    pub const fn preview_sizes(&self) -> [[u32; 2]; 2] {
        match self.kind() {
            Kind::Machine(format) => preview_sizes(format),
            Kind::Pp1 => PP1_PREVIEW_SIZES,
            Kind::Sl1(_) => SL1_PREVIEW_SIZES,
        }
    }
    /// Store `previews` (model thumbnails at [`Self::preview_sizes`]) in the file. Without them,
    /// GOO thumbnails are black, CTB and Photon previews are absent, and PP1 previews show the
    /// masks' projection.
    #[must_use]
    pub fn with_previews(mut self, previews: Previews) -> Self {
        self.previews = Some(previews);
        self
    }
    /// Declare the masks' supersampling per axis (`RasterConfig::samples_per_axis`), which the
    /// format metadata records. Every exported mask must use it, or the export fails.
    #[must_use]
    pub const fn with_antialiasing(mut self, samples_per_axis: u8) -> Self {
        self.antialiasing = samples_per_axis;
        self
    }
    /// The declared supersampling, checked against every mask.
    fn check_antialiasing(declared: u8, image: &RasterImage) -> Result<(), ExportError> {
        let used = image.config().samples_per_axis;
        if used == declared {
            Ok(())
        } else {
            Err(ExportError::Invalid(format!(
                "mask uses {used} samples per axis but the export declares {declared}"
            )))
        }
    }
    const fn machine_format(&self) -> Option<MachineFormat> {
        match self.format {
            BuiltinFormat::CtbV2Plain => Some(MachineFormat::CtbV2Plain),
            BuiltinFormat::CtbV3Plain => Some(MachineFormat::CtbV3Plain),
            BuiltinFormat::CtbV4Plain => Some(MachineFormat::CtbV4Plain),
            BuiltinFormat::CtbV5Plain => Some(MachineFormat::CtbV5Plain),
            BuiltinFormat::AnycubicWorkshop(target) => Some(MachineFormat::PhotonWorkshop(target)),
            BuiltinFormat::GooV3 => Some(MachineFormat::GooV3),
            BuiltinFormat::PhotonV1FirmwareMotion => Some(MachineFormat::PhotonV1),
            BuiltinFormat::PrusaSl1FirmwareMotion
            | BuiltinFormat::PrusaSl1sFirmwareMotion
            | BuiltinFormat::AnycubicPhotonP1 => None,
        }
    }
    const fn kind(&self) -> Kind {
        match self.format {
            BuiltinFormat::AnycubicPhotonP1 => Kind::Pp1,
            BuiltinFormat::PrusaSl1FirmwareMotion => Kind::Sl1(Sl1Model::Sl1),
            BuiltinFormat::PrusaSl1sFirmwareMotion => Kind::Sl1(Sl1Model::Sl1s),
            _ => match self.machine_format() {
                Some(format) => Kind::Machine(format),
                None => Kind::Pp1,
            },
        }
    }
    /// Formats whose firmware manages motion: job motion is not stored.
    const fn firmware_motion(&self) -> bool {
        matches!(
            self.format,
            BuiltinFormat::PhotonV1FirmwareMotion
                | BuiltinFormat::PrusaSl1FirmwareMotion
                | BuiltinFormat::PrusaSl1sFirmwareMotion
        )
    }
    fn quantization(&self) -> Quantization {
        match self.pixels {
            PixelPolicy::Exact => Quantization::Exact,
            PixelPolicy::RoundToFormat => Quantization::RoundToFormat,
        }
    }
    fn wire_job(&self, job: &PrintJob) -> Result<WireJob, ExportError> {
        let mut normal = wire_settings(&job.settings().normal);
        let mut bottom = wire_settings(&job.settings().bottom);
        if self.firmware_motion() {
            normal.lift = [[0.; 2]; 2];
            normal.retract = [[0.; 2]; 2];
            bottom.lift = [[0.; 2]; 2];
            bottom.retract = [[0.; 2]; 2];
        }
        // Writers pair each panel axis length with that axis's pixel count. With `swap_xy`,
        // pixel columns cover world Y, so the world build lengths swap into panel order.
        let printer = job.printer();
        let [x, y, z] = printer.build_size_mm;
        let build_mm = if printer.swap_xy {
            [y, x, z]
        } else {
            [x, y, z]
        };
        Ok(WireJob {
            resolution: printer.resolution_px,
            build_mm,
            layer_height_mm: job.settings().layer_height_mm,
            bottom_count: job.settings().bottom_layer_count,
            transition_count: job.settings().transition_layer_count,
            layer_count: u32::try_from(job.layer_count())
                .map_err(|_| ExportError::Invalid("machine layer count exceeds u32".into()))?,
            machine_name: if self.format == BuiltinFormat::AnycubicPhotonP1 {
                "Anycubic Photon P1".into()
            } else {
                job.printer().name.clone()
            },
            normal,
            bottom,
            estimated_time_s: job.estimated_time_s(),
            volume_ml: 0.,
            antialiasing: self.antialiasing,
            previews: self.previews.clone(),
            job_name: job.id().to_owned(),
            resin_name: job.resin().name.clone(),
            resin_density_g_per_ml: job.resin().density_g_per_ml.unwrap_or(0.),
            heating_temperature_c: job
                .settings()
                .heating_temperature_c
                .unwrap_or(crate::profiles::DEFAULT_HEATING_C),
            stored_mirror: stored_mirror(printer),
        })
    }
    fn wire_layer(&self, layer: &JobLayer) -> WireLayer {
        let mut settings = wire_settings(&layer.settings);
        if self.firmware_motion() {
            settings.lift = [[0.; 2]; 2];
            settings.retract = [[0.; 2]; 2];
        }
        WireLayer {
            z_mm: layer.position_z_mm,
            settings,
        }
    }
}

/// How the stored image is mirrored against a machine format's view from above with the plate's
/// back in the first row (TD-104), as GOO headers declare it. Karasu's raster puts the plate's
/// front in the first row, so an image the profile does not flip in Y is mirrored in Y against
/// that view, as both reference slicers write their GOO flags.
fn stored_mirror(printer: &crate::profiles::PrinterProfile) -> [bool; 2] {
    [printer.mirror_x, !printer.mirror_y]
}

fn error(e: CodecError) -> ExportError {
    match e {
        CodecError::Cancelled(e) => ExportError::Cancelled(e),
        e => ExportError::Encoding(Box::new(e)),
    }
}
fn limits(value: ExportLimits) -> EncoderLimits {
    EncoderLimits {
        max_output_bytes: value.max_output_bytes,
        max_layer_bytes: value.max_layer_bytes,
        max_metadata_bytes: value.max_metadata_bytes,
    }
}
impl MachineExporter for BuiltinExporter {
    fn id(&self) -> &str {
        match self.format {
            BuiltinFormat::CtbV2Plain => "ctb-v2-plain",
            BuiltinFormat::CtbV3Plain => "ctb-v3-plain",
            BuiltinFormat::CtbV4Plain => "ctb-v4-plain",
            BuiltinFormat::CtbV5Plain => "ctb-v5-plain",
            BuiltinFormat::AnycubicWorkshop(target) => target.id(),
            BuiltinFormat::GooV3 => "goo-v3",
            BuiltinFormat::AnycubicPhotonP1 => "anycubic-pp1-v3",
            BuiltinFormat::PhotonV1FirmwareMotion => "photon-v1-firmware-motion",
            BuiltinFormat::PrusaSl1FirmwareMotion => "prusa-sl1-firmware-motion",
            BuiltinFormat::PrusaSl1sFirmwareMotion => "prusa-sl1s-firmware-motion",
        }
    }
    fn extension(&self) -> &str {
        match self.format {
            BuiltinFormat::CtbV2Plain
            | BuiltinFormat::CtbV3Plain
            | BuiltinFormat::CtbV4Plain
            | BuiltinFormat::CtbV5Plain => "ctb",
            BuiltinFormat::AnycubicWorkshop(target) => target.extension(),
            BuiltinFormat::GooV3 => "goo",
            BuiltinFormat::AnycubicPhotonP1 => "pp1",
            BuiltinFormat::PhotonV1FirmwareMotion => "photon",
            BuiltinFormat::PrusaSl1FirmwareMotion => "sl1",
            BuiltinFormat::PrusaSl1sFirmwareMotion => "sl1s",
        }
    }
    fn preflight(
        &self,
        job: &PrintJob,
        value: ExportLimits,
        cancel: &mut Cancellation<'_>,
    ) -> Result<(), ExportError> {
        super::preflight(job, value)?;
        let wire = self.wire_job(job)?;
        match self.kind() {
            Kind::Machine(format) => MachineEncoder::validate_metadata(format, &wire),
            Kind::Pp1 => Pp1Encoder::validate_metadata(&wire),
            Kind::Sl1(model) => Sl1Encoder::validate_metadata(model, &wire),
        }
        .map_err(error)?;
        for index in 0..job.layer_count() {
            cancel.poll(Checkpoint::ExportLayer, index)?;
            let layer = job.layer(index)?;
            let encoded = self.wire_layer(&layer);
            let index = u32::try_from(index)
                .map_err(|_| ExportError::Invalid("machine layer index overflow".into()))?;
            match self.kind() {
                Kind::Machine(format) => {
                    MachineEncoder::validate_layer(format, &wire, &encoded, index)
                }
                Kind::Pp1 => Pp1Encoder::validate_layer(&wire, &encoded, index),
                Kind::Sl1(_) => Sl1Encoder::validate_layer(&wire, &encoded, index),
            }
            .map_err(error)?;
        }
        Ok(())
    }
    fn begin<'a>(
        &self,
        writer: &'a mut dyn WriteSeek,
        job: &PrintJob,
        value: ExportLimits,
        cancel: &mut Cancellation<'_>,
    ) -> Result<Box<dyn ExportSession + 'a>, ExportError> {
        let wire = self.wire_job(job)?;
        let encoder = match self.kind() {
            Kind::Machine(format) => Encoder::Machine(
                MachineEncoder::begin(
                    format,
                    wire,
                    writer,
                    limits(value),
                    self.quantization(),
                    cancel,
                )
                .map_err(error)?,
            ),
            Kind::Pp1 => Encoder::Pp1(
                Pp1Encoder::begin(wire, writer, limits(value), self.quantization(), cancel)
                    .map_err(error)?,
            ),
            Kind::Sl1(model) => Encoder::Sl1(
                Sl1Encoder::begin(model, wire, writer, limits(value), cancel).map_err(error)?,
            ),
        };
        Ok(Box::new(Session {
            writer,
            encoder,
            firmware_motion: self.firmware_motion(),
            antialiasing: self.antialiasing,
        }))
    }
}
enum Encoder {
    Machine(MachineEncoder),
    Pp1(Pp1Encoder),
    Sl1(Sl1Encoder),
}
struct Session<'a> {
    writer: &'a mut dyn WriteSeek,
    encoder: Encoder,
    firmware_motion: bool,
    antialiasing: u8,
}
impl Session<'_> {
    fn wire_layer(&self, layer: &JobLayer) -> WireLayer {
        let mut settings = wire_settings(&layer.settings);
        if self.firmware_motion {
            settings.lift = [[0.; 2]; 2];
            settings.retract = [[0.; 2]; 2];
        }
        WireLayer {
            z_mm: layer.position_z_mm,
            settings,
        }
    }
}
impl ExportSession for Session<'_> {
    fn write_layer(
        &mut self,
        layer: &JobLayer,
        image: &RasterImage,
        cancel: &mut Cancellation<'_>,
    ) -> Result<(), ExportError> {
        BuiltinExporter::check_antialiasing(self.antialiasing, image)?;
        let wire = self.wire_layer(layer);
        match &mut self.encoder {
            Encoder::Machine(encoder) => {
                encoder.write_layer(self.writer, &wire, image.mask(), cancel)
            }
            Encoder::Pp1(encoder) => encoder.write_layer(self.writer, &wire, image.mask(), cancel),
            Encoder::Sl1(encoder) => encoder.write_layer(self.writer, &wire, image.mask(), cancel),
        }
        .map_err(error)
    }
    fn finish(self: Box<Self>, cancel: &mut Cancellation<'_>) -> Result<(), ExportError> {
        match self.encoder {
            Encoder::Machine(encoder) => encoder.finish(self.writer, cancel),
            Encoder::Pp1(encoder) => encoder.finish(self.writer, cancel),
            Encoder::Sl1(encoder) => encoder.finish(self.writer, cancel),
        }
        .map_err(error)
    }
    fn layer_encoder(&self) -> Option<Box<dyn LayerEncoder>> {
        Some(Box::new(match &self.encoder {
            Encoder::Machine(encoder) => {
                let (format, quantization, limits, antialiasing) = encoder.encoding_parameters();
                BuiltinLayerEncoder::Machine(format, quantization, limits, antialiasing)
            }
            Encoder::Pp1(encoder) => {
                let (quantization, limits, resolution) = encoder.encoding_parameters();
                BuiltinLayerEncoder::Pp1(quantization, limits, resolution)
            }
            Encoder::Sl1(encoder) => BuiltinLayerEncoder::Sl1(encoder.encoding_parameters()),
        }))
    }
    fn write_encoded(
        &mut self,
        layer: &JobLayer,
        image: &RasterImage,
        payload: EncodedLayer,
        cancel: &mut Cancellation<'_>,
    ) -> Result<(), ExportError> {
        BuiltinExporter::check_antialiasing(self.antialiasing, image)?;
        let wire = self.wire_layer(layer);
        match &mut self.encoder {
            Encoder::Machine(encoder) => encoder.write_encoded(
                self.writer,
                &wire,
                [image.width(), image.height()],
                payload.payload,
                cancel,
            ),
            Encoder::Pp1(encoder) => {
                encoder.write_encoded(self.writer, &wire, image.mask(), payload.payload, cancel)
            }
            Encoder::Sl1(encoder) => encoder.write_encoded(
                self.writer,
                &wire,
                [image.width(), image.height()],
                payload.payload,
                cancel,
            ),
        }
        .map_err(error)
    }
}

/// The pure per-layer encoding of a built-in session, with copies of its parameters.
enum BuiltinLayerEncoder {
    Machine(MachineFormat, Quantization, EncoderLimits, u8),
    Pp1(Quantization, EncoderLimits, [u32; 2]),
    Sl1(EncoderLimits),
}

impl LayerEncoder for BuiltinLayerEncoder {
    fn encode(&self, image: &RasterImage, cancel: &mut Cancellation<'_>) -> EncodedLayer {
        let payload = match *self {
            Self::Machine(format, quantization, limits, antialiasing) => MachineEncoder::encode(
                format,
                quantization,
                limits,
                antialiasing,
                image.mask(),
                cancel,
            ),
            Self::Pp1(quantization, limits, resolution) => {
                Pp1Encoder::encode(quantization, limits, resolution, image.mask(), cancel)
            }
            Self::Sl1(limits) => Sl1Encoder::encode(limits, image.mask(), cancel),
        };
        EncodedLayer { payload }
    }
}
