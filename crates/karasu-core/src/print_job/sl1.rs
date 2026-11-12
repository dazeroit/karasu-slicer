//! Prusa SL1 and SL1S archives (TD-063).
//!
//! A ZIP holding:
//! - one 8-bit grayscale PNG per layer, `<job>00000.png` and following;
//! - RGBA thumbnails `thumbnail/thumbnail400x400.png` and `thumbnail/thumbnail800x480.png`, when
//!   previews are supplied;
//! - `config.ini`, written last because its totals (material, layer classes) are known only after
//!   every layer.
//!
//! Members are found by name, so their order is free.
//!
//! The firmware manages motion. Exposure is one normal time and one initial time that fades over
//! `numFade` layers; jobs that need anything else are refused.
use super::{
    machine::{check_previews, CodecError, EncodedPayload, EncoderLimits, WireJob, WireLayer},
    zip::ClassicZip,
};
use crate::{
    cancellation::{Cancellation, Checkpoint},
    export::WriteSeek,
    raster::{mask::SpanMask, png::encode_compressed},
};
use std::fmt::Write as _;

/// The Prusa printer an archive is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Sl1Model {
    Sl1,
    Sl1s,
}

impl Sl1Model {
    const fn name(self) -> &'static str {
        match self {
            Self::Sl1 => "SL1",
            Self::Sl1s => "SL1S",
        }
    }
}

/// `[small, large]` thumbnail sizes.
pub(crate) const SL1_PREVIEW_SIZES: [[u32; 2]; 2] = [[400, 400], [800, 480]];
/// Layers whose lit area exceeds this share of the display are counted as slow (tilt) layers.
const AREA_FILL_PERCENT: u64 = 50;
/// Fixed timestamp: the archive must not depend on the clock.
const TIMESTAMP: &str = "1970-01-01 at 00:00:00 UTC";

pub(crate) struct Sl1Encoder {
    model: Sl1Model,
    job: WireJob,
    limits: EncoderLimits,
    zip: ClassicZip,
    count: u32,
    previous_z: f64,
    encoded_volume_ml: f64,
    fast: u32,
    slow: u32,
}

/// Exposure of layer `index` under SL1's fade: the initial time on the first layer, then
/// `numFade` layers stepping linearly towards the normal time.
fn faded_exposure(job: &WireJob, index: u32) -> f64 {
    let (first, normal) = (job.bottom.exposure_s, job.normal.exposure_s);
    if index == 0 {
        first
    } else if index <= job.transition_count {
        let fraction = f64::from(index) / (f64::from(job.transition_count) + 1.);
        first + (normal - first) * fraction
    } else {
        normal
    }
}

impl Sl1Encoder {
    /// Job-level capabilities.
    #[allow(clippy::float_cmp)] // Settings must be represented exactly or refused.
    pub(crate) fn validate_metadata(model: Sl1Model, job: &WireJob) -> Result<(), CodecError> {
        super::machine::validate_antialiasing(job.antialiasing)?;
        if job.resolution.contains(&0) || job.layer_count == 0 || job.layer_height_mm <= 0. {
            return Err(CodecError::Invalid("dimensions or layer count"));
        }
        if job.bottom_count != 1 || !(3..=20).contains(&job.transition_count) {
            return Err(CodecError::Unsupported(
                "SL1 exposes one initial layer that fades over 3 to 20 layers: set 1 bottom \\
                 layer and 3 to 20 transition layers",
            ));
        }
        if job.transition_count >= job.layer_count {
            return Err(CodecError::Invalid("SL1 fade longer than the job"));
        }
        for s in [&job.bottom, &job.normal] {
            if s.wait_before_exposure_s != 0.
                || s.wait_after_exposure_s != 0.
                || s.wait_after_lift_s != 0.
                || s.pwm != 255
            {
                return Err(CodecError::Unsupported(
                    "SL1 firmware manages waits and light power; set them to zero and full power",
                ));
            }
            if !(s.exposure_s > 0. && s.exposure_s.is_finite()) {
                return Err(CodecError::Invalid("SL1 exposure"));
            }
        }
        let name = &job.job_name;
        if name.is_empty()
            || name.len() > 64
            || !name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        {
            return Err(CodecError::Invalid(
                "SL1 job names are 1 to 64 ASCII letters, digits, '_' or '-'",
            ));
        }
        let _ = model;
        Ok(())
    }

    /// Per-layer capabilities: the exposure must follow SL1's fade, with no other overrides.
    #[allow(clippy::float_cmp)] // Exposures must match the fade exactly.
    pub(crate) fn validate_layer(
        job: &WireJob,
        layer: &WireLayer,
        index: u32,
    ) -> Result<(), CodecError> {
        if index >= job.layer_count || layer.z_mm <= 0. || layer.z_mm > job.build_mm[2] {
            return Err(CodecError::Invalid("layer index or Z"));
        }
        if layer.settings.exposure_s != faded_exposure(job, index) {
            return Err(CodecError::Unsupported(
                "SL1 cannot store per-layer exposure beyond its initial fade",
            ));
        }
        Ok(())
    }

    pub(crate) fn begin(
        model: Sl1Model,
        job: WireJob,
        writer: &mut dyn WriteSeek,
        limits: EncoderLimits,
        cancel: &mut Cancellation<'_>,
    ) -> Result<Self, CodecError> {
        cancel.check(Checkpoint::ExportStart)?;
        Self::validate_metadata(model, &job)?;
        if let Some(previews) = &job.previews {
            check_previews(previews, SL1_PREVIEW_SIZES)?;
        }
        if writer.stream_position()? != 0 {
            return Err(CodecError::Invalid("SL1 output must start at byte zero"));
        }
        Ok(Self {
            model,
            job,
            limits,
            zip: ClassicZip::new(limits),
            count: 0,
            previous_z: 0.,
            encoded_volume_ml: 0.,
            fast: 0,
            slow: 0,
        })
    }

    pub(crate) const fn encoding_parameters(&self) -> EncoderLimits {
        self.limits
    }

    /// A compressed grayscale PNG of the mask, its intensity sum and its lit-pixel count; no
    /// session state, so any thread can encode.
    pub(crate) fn encode(
        limits: EncoderLimits,
        mask: &SpanMask,
        cancel: &mut Cancellation<'_>,
    ) -> Result<EncodedPayload, CodecError> {
        let (mut intensity, mut lit) = (0u64, 0u64);
        mask.for_each_run(|value, length| {
            intensity += u64::from(value) * length as u64;
            if value != 0 {
                lit += length as u64;
            }
            Ok::<_, CodecError>(())
        })?;
        let mut png = Vec::new();
        encode_compressed(
            &mut png,
            mask.width(),
            mask.height(),
            1,
            &mut |y, row| mask.fill_row(y, row),
            cancel,
        )
        .map_err(|e| match e {
            crate::raster::png::PngError::Cancelled(c) => CodecError::Cancelled(c),
            crate::raster::png::PngError::Io(e) => CodecError::Io(e),
            _ => CodecError::Invalid("SL1 layer PNG"),
        })?;
        if png.len() > limits.max_layer_bytes {
            return Err(CodecError::Limit("encoded layer byte budget"));
        }
        Ok(EncodedPayload {
            bytes: png,
            intensity,
            lit: Some((lit, None)),
            scene: None,
            member: None,
        })
    }

    pub(crate) fn write_layer(
        &mut self,
        writer: &mut dyn WriteSeek,
        layer: &WireLayer,
        mask: &SpanMask,
        cancel: &mut Cancellation<'_>,
    ) -> Result<(), CodecError> {
        cancel.check(Checkpoint::ExportLayer)?;
        self.precheck(layer, [mask.width(), mask.height()])?;
        let payload = Self::encode(self.limits, mask, cancel)?;
        self.emit(writer, layer, &payload, cancel)
    }

    pub(crate) fn write_encoded(
        &mut self,
        writer: &mut dyn WriteSeek,
        layer: &WireLayer,
        dimensions: [u32; 2],
        payload: Result<EncodedPayload, CodecError>,
        cancel: &mut Cancellation<'_>,
    ) -> Result<(), CodecError> {
        cancel.check(Checkpoint::ExportLayer)?;
        self.precheck(layer, dimensions)?;
        self.emit(writer, layer, &payload?, cancel)
    }

    fn precheck(&self, layer: &WireLayer, dimensions: [u32; 2]) -> Result<(), CodecError> {
        if self.count >= self.job.layer_count {
            return Err(CodecError::Invalid("too many layers"));
        }
        if dimensions != self.job.resolution {
            return Err(CodecError::Invalid("mask dimensions do not match job"));
        }
        if layer.z_mm <= self.previous_z {
            return Err(CodecError::Invalid("layer Z must increase"));
        }
        // `config.ini` has one layer height: variable layer heights cannot be stored (TD-064).
        let thickness = layer.z_mm - self.previous_z;
        if (thickness - self.job.layer_height_mm).abs() > 1e-9 * self.job.layer_height_mm {
            return Err(CodecError::Unsupported(
                "SL1 archives store one layer height; variable layer heights are not supported",
            ));
        }
        Self::validate_layer(&self.job, layer, self.count)
    }

    fn emit(
        &mut self,
        writer: &mut dyn WriteSeek,
        layer: &WireLayer,
        payload: &EncodedPayload,
        cancel: &mut Cancellation<'_>,
    ) -> Result<(), CodecError> {
        let name = format!("{}{:05}.png", self.job.job_name, self.count);
        self.zip.add(writer, &name, &payload.bytes, cancel)?;
        let pixel_area = self.job.build_mm[0] / f64::from(self.job.resolution[0])
            * (self.job.build_mm[1] / f64::from(self.job.resolution[1]));
        #[allow(clippy::cast_precision_loss)] // Bounded sums far below 2^52.
        let layer_ml =
            payload.intensity as f64 / 255. * pixel_area * (layer.z_mm - self.previous_z) / 1000.;
        self.encoded_volume_ml += layer_ml;
        let lit = payload.lit.map_or(0, |(lit, _)| lit);
        let pixels = u64::from(self.job.resolution[0]) * u64::from(self.job.resolution[1]);
        if lit * 100 > pixels * AREA_FILL_PERCENT {
            self.slow += 1;
        } else {
            self.fast += 1;
        }
        self.previous_z = layer.z_mm;
        self.count += 1;
        Ok(())
    }

    fn config(&self) -> Result<String, CodecError> {
        let j = &self.job;
        let number = |v: f64| {
            // Shortest round-trip text, locale independent.
            let mut text = format!("{v}");
            if text.ends_with(".0") {
                text.truncate(text.len() - 2);
            }
            text
        };
        let mut keys = vec![
            ("action", "print".to_owned()),
            ("expTime", number(j.normal.exposure_s)),
            ("expTimeFirst", number(j.bottom.exposure_s)),
            ("expUserProfile", "0".to_owned()),
            ("fileCreationTimestamp", TIMESTAMP.to_owned()),
            ("hollow", "0".to_owned()),
            ("jobDir", j.job_name.clone()),
            ("layerHeight", number(j.layer_height_mm)),
            ("numFade", j.transition_count.to_string()),
            ("numFast", self.fast.to_string()),
            ("numSlow", self.slow.to_string()),
            ("printTime", format!("{:.6}", j.estimated_time_s)),
            ("printerModel", self.model.name().to_owned()),
            ("printerVariant", "default".to_owned()),
            (
                "prusaSlicerVersion",
                concat!("karasu-slicer-", env!("CARGO_PKG_VERSION")).to_owned(),
            ),
            ("usedMaterial", format!("{:.6}", self.encoded_volume_ml)),
        ];
        keys.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
        let mut text = String::new();
        for (key, value) in keys {
            writeln!(text, "{key} = {value}").map_err(|_| CodecError::Invalid("SL1 config"))?;
        }
        Ok(text)
    }

    pub(crate) fn finish(
        mut self,
        writer: &mut dyn WriteSeek,
        cancel: &mut Cancellation<'_>,
    ) -> Result<(), CodecError> {
        cancel.check(Checkpoint::ExportFinish)?;
        if self.count != self.job.layer_count {
            return Err(CodecError::Invalid("missing layers"));
        }
        if let Some(previews) = self.job.previews.take() {
            for image in [&previews.small, &previews.large] {
                let (w, h) = (image.width(), image.height());
                let rgb = image.rgb();
                let mut png = Vec::new();
                encode_compressed(
                    &mut png,
                    w,
                    h,
                    4,
                    &mut |y, row| {
                        let start = y * w as usize * 3;
                        for (x, px) in row.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                            px[..3].copy_from_slice(&rgb[start + 3 * x..start + 3 * x + 3]);
                            px[3] = 255;
                        }
                    },
                    cancel,
                )
                .map_err(|_| CodecError::Invalid("SL1 thumbnail PNG"))?;
                self.zip.add(
                    writer,
                    &format!("thumbnail/thumbnail{w}x{h}.png"),
                    &png,
                    cancel,
                )?;
            }
        }
        let config = self.config()?;
        self.zip
            .add(writer, "config.ini", config.as_bytes(), cancel)?;
        self.zip.finish(writer, cancel)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::print_job::machine::WireSettings;
    use std::io::Cursor;

    fn settings(exposure_s: f64) -> WireSettings {
        WireSettings {
            exposure_s,
            wait_before_exposure_s: 0.,
            wait_after_exposure_s: 0.,
            wait_after_lift_s: 0.,
            pwm: 255,
            lift: [[0.; 2]; 2],
            retract: [[0.; 2]; 2],
        }
    }
    fn job() -> WireJob {
        WireJob {
            resolution: [3, 2],
            build_mm: [3., 2., 10.],
            layer_height_mm: 0.05,
            bottom_count: 1,
            transition_count: 3,
            layer_count: 5,
            machine_name: "SL1S".into(),
            normal: settings(2.),
            bottom: settings(10.),
            estimated_time_s: 30.,
            volume_ml: 0.,
            antialiasing: 1,
            previews: None,
            job_name: "part".into(),
            resin_name: String::new(),
            resin_density_g_per_ml: 0.,
            heating_temperature_c: crate::profiles::DEFAULT_HEATING_C,
            stored_mirror: [false; 2],
        }
    }
    fn limits() -> EncoderLimits {
        EncoderLimits {
            max_output_bytes: 1 << 24,
            max_metadata_bytes: 1 << 20,
            max_layer_bytes: 1 << 20,
        }
    }
    /// Members of a stored ZIP by name.
    fn members(data: &[u8]) -> Vec<(String, Vec<u8>)> {
        let mut out = Vec::new();
        let mut at = 0;
        while data[at..at + 4] == *b"PK\x03\x04" {
            let size = u32::from_le_bytes(data[at + 18..at + 22].try_into().unwrap()) as usize;
            let name_len = u16::from_le_bytes(data[at + 26..at + 28].try_into().unwrap()) as usize;
            let extra = u16::from_le_bytes(data[at + 28..at + 30].try_into().unwrap()) as usize;
            let name = String::from_utf8(data[at + 30..at + 30 + name_len].to_vec()).unwrap();
            let start = at + 30 + name_len + extra;
            out.push((name, data[start..start + size].to_vec()));
            at = start + size;
        }
        out
    }
    fn write(j: WireJob, masks: &[[u8; 6]]) -> Result<Vec<u8>, CodecError> {
        let exposures: Vec<f64> = (0..j.layer_count).map(|i| faded_exposure(&j, i)).collect();
        let mut out = Cursor::new(Vec::new());
        let mut cancel = Cancellation::never();
        let mut encoder = Sl1Encoder::begin(Sl1Model::Sl1s, j, &mut out, limits(), &mut cancel)?;
        for (i, (pixels, exposure)) in masks.iter().zip(exposures).enumerate() {
            let layer = WireLayer {
                z_mm: 0.05 * f64::from(u32::try_from(i + 1).unwrap()),
                settings: settings(exposure),
            };
            let mask = SpanMask::from_pixels(3, 2, pixels).unwrap();
            encoder.write_layer(&mut out, &layer, &mask, &mut cancel)?;
        }
        encoder.finish(&mut out, &mut cancel)?;
        Ok(out.into_inner())
    }

    #[test]
    fn archive_layers_config_and_fade() {
        let masks = [
            [255; 6],
            [0, 255, 0, 255, 255, 0],
            [0; 6],
            [9, 0, 0, 0, 0, 0],
            [0, 128, 0, 0, 0, 0],
        ];
        let data = write(job(), &masks).unwrap();
        let members = members(&data);
        let names: Vec<&str> = members.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(
            names,
            [
                "part00000.png",
                "part00001.png",
                "part00002.png",
                "part00003.png",
                "part00004.png",
                "config.ini"
            ]
        );
        // Layer PNGs: 8-bit grayscale (colour type 0) of the mask, compressed.
        for ((_, png), mask) in members.iter().zip(&masks) {
            assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
            assert_eq!((png[24], png[25]), (8, 0));
            let at = png.windows(4).position(|w| w == b"IDAT").unwrap();
            let size = u32::from_be_bytes(png[at - 4..at].try_into().unwrap()) as usize;
            let raw =
                miniz_oxide::inflate::decompress_to_vec_zlib(&png[at + 4..at + 4 + size]).unwrap();
            assert_eq!(raw, [&[0][..], &mask[..3], &[0], &mask[3..]].concat());
        }
        let config = String::from_utf8(members.last().unwrap().1.clone()).unwrap();
        let keys: Vec<&str> = config
            .lines()
            .map(|l| l.split(" = ").next().unwrap())
            .collect();
        let mut sorted = keys.clone();
        sorted.sort_unstable();
        assert_eq!(keys, sorted);
        assert_eq!(keys.len(), 16);
        for line in [
            "expTime = 2",
            "expTimeFirst = 10",
            "numFade = 3",
            "numFast = 4",
            "numSlow = 1",
            "jobDir = part",
            "printerModel = SL1S",
            "printTime = 30.000000",
            "layerHeight = 0.05",
        ] {
            assert!(config.lines().any(|l| l == line), "{line}\n{config}");
        }
    }

    #[test]
    fn unsupported_jobs_are_refused_before_output() {
        let check = |j: WireJob| Sl1Encoder::validate_metadata(Sl1Model::Sl1, &j);
        assert!(check(job()).is_ok());
        assert!(check(WireJob {
            bottom_count: 2,
            ..job()
        })
        .is_err());
        assert!(check(WireJob {
            transition_count: 2,
            ..job()
        })
        .is_err());
        assert!(check(WireJob {
            transition_count: 21,
            layer_count: 30,
            ..job()
        })
        .is_err());
        assert!(check(WireJob {
            job_name: "bad name".into(),
            ..job()
        })
        .is_err());
        let rested = WireJob {
            normal: WireSettings {
                wait_after_lift_s: 1.,
                ..settings(2.)
            },
            ..job()
        };
        assert!(check(rested).is_err());
        // A layer whose exposure leaves the fade is refused.
        let j = job();
        let layer = WireLayer {
            z_mm: 0.1,
            settings: settings(9.),
        };
        assert!(Sl1Encoder::validate_layer(&j, &layer, 1).is_err());
        let layer = WireLayer {
            z_mm: 0.1,
            settings: settings(faded_exposure(&j, 1)),
        };
        assert!(Sl1Encoder::validate_layer(&j, &layer, 1).is_ok());
        assert_eq!(faded_exposure(&j, 1), 8.0);
        assert_eq!(faded_exposure(&j, 4), 2.0);
    }

    #[test]
    fn variable_layer_heights_are_refused() {
        let j = job();
        let exposures = [faded_exposure(&j, 0), faded_exposure(&j, 1)];
        let mut out = Cursor::new(Vec::new());
        let mut cancel = Cancellation::never();
        let mut encoder =
            Sl1Encoder::begin(Sl1Model::Sl1s, j, &mut out, limits(), &mut cancel).unwrap();
        let mask = SpanMask::from_pixels(3, 2, &[0; 6]).unwrap();
        let layer = |z_mm, exposure| WireLayer {
            z_mm,
            settings: settings(exposure),
        };
        encoder
            .write_layer(&mut out, &layer(0.05, exposures[0]), &mask, &mut cancel)
            .unwrap();
        // A 0.025 mm second layer.
        let error = encoder
            .write_layer(&mut out, &layer(0.075, exposures[1]), &mask, &mut cancel)
            .unwrap_err();
        assert!(error.to_string().contains("one layer height"), "{error}");
    }
}
