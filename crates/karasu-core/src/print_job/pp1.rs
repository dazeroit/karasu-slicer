//! Photon P1 ZIP/PW0 adapter from documented public interoperability structures.
use super::machine::EncodedPayload;
use super::{
    machine::{
        check_previews, CodecError, EncoderLimits, Quantization, WireJob, WireLayer, WireSettings,
    },
    pp1_scene::{layer_metrics, Scene},
    zip::{ClassicZip, Member},
};
use crate::{
    cancellation::{Cancellation, Checkpoint},
    export::WriteSeek,
    raster::mask::SpanMask,
};
use serde_json::{json, Value};
use std::io::Write;

/// Streaming masks, bounded retained controller/scene records, deterministic ZIP whose members
/// are deflated when that makes them smaller (TD-133).
pub(crate) struct Pp1Encoder {
    job: WireJob,
    limits: EncoderLimits,
    quantization: Quantization,
    zip: ClassicZip,
    scene: Scene,
    layers: Vec<WireLayer>,
    previous_z: f64,
    encoded_volume_ml: f64,
    projection: Vec<u8>,
}

#[allow(clippy::cast_possible_truncation)] // The protocol is binary32; reject overflow and underflow explicitly.
fn number(v: f64) -> Result<f32, CodecError> {
    if !v.is_finite() || !(v as f32).is_finite() || (v != 0. && v as f32 == 0.) {
        return Err(CodecError::Invalid("PP1 finite binary32 numeric domain"));
    }
    Ok(v as f32)
}

#[allow(clippy::float_cmp)] // A shared wire field must preserve both requested stage distances exactly.
fn settings(s: &WireSettings) -> Result<(), CodecError> {
    if s.exposure_s <= 0. || s.pwm != 255 {
        return Err(CodecError::Unsupported(
            "PP1 requires positive exposure and full PWM; a power field is unverified",
        ));
    }
    for v in [
        s.exposure_s,
        s.wait_before_exposure_s,
        s.wait_after_exposure_s,
        s.wait_after_lift_s,
    ] {
        number(v)?;
        if v < 0. {
            return Err(CodecError::Invalid("PP1 negative timing"));
        }
    }
    for [distance, speed] in s.lift.into_iter().chain(s.retract) {
        number(distance)?;
        number(speed)?;
        if distance < 0. || speed < 0. || (distance > 0. && speed == 0.) {
            return Err(CodecError::Invalid("PP1 invalid motion stage"));
        }
    }
    let [[l1, _], [l2, _]] = s.lift;
    let [[r1, _], [r2, _]] = s.retract;
    let mirrored = if l2 == 0. && r2 == 0. {
        r1 == l1
    } else {
        r1 == l2 && r2 == l1
    };
    if !mirrored {
        return Err(CodecError::Unsupported(
            "PP1 stage heights are shared by lift and retract: retract stages must mirror the lift stages",
        ));
    }
    Ok(())
}

/// PP1's motion bands, from the plate's lowest position up: `[height, up speed, down speed]`.
///
/// Lifting covers band 0, then band 1. Retracting runs in the opposite order, so the last retract
/// stage covers band 0, and a two-stage retract's speeds are read in reverse, as in the job's
/// motion order (OUTPUT-FORMATS-SPEC, two-stage motion). `settings` has checked that the stages
/// mirror.
fn bands(s: &WireSettings) -> [[f64; 3]; 2] {
    let [[l1, v1], [l2, v2]] = s.lift;
    let [[_, w1], [_, w2]] = s.retract;
    if l2 == 0. {
        [[l1, v1, w1], [0., 0., 0.]]
    } else {
        [[l1, v1, w2], [l2, v2, w1]]
    }
}

/// The quarter turn PP1's `rotate_z` declares for a stored image mirrored as `stored_mirror`
/// (TD-104), in degrees: none for karasu-slicer's unmirrored image (as Lychee writes), 180 for
/// the image mirrored in X and Y (as the manufacturer's slicer writes). A mirror along one axis
/// is not a turn, so PP1 cannot declare it: `None`.
/// The P1's firmware constants for its print-time and exposure estimates (TD-117), as Photon
/// Workshop 4.1.0 records them in the official `TEST.pp1` (SHA-256 `d48e98b7…a7c83a`) and as
/// Lychee 8.0.0 and CHITUBOX 1.2.3 write them alike. Photon Workshop refuses a file without
/// them, whether its `firmware_calc_print_time` flag is set or not. Owner's authorization,
/// 2026-11-03: they are facts about the machine, recorded for interoperability.
fn firmware_constants() -> (Value, Value) {
    (
        json!({"version":"2","FUNCTION":1,"HEIGHT_COMPENSATE":[0,0,0,0],"LAYER_COMPENSATE":[0,0,0,0],
            "MACHINE_AXIS_STEPS_PER_UNIT":[100.0,100.0,3200.0,94.0],"MACHINE_BLOCK_BUFFER_SIZE":32,
            "MACHINE_DEFAULT_ACCELERATION":1000.0,"MACHINE_DEFAULT_MINSEGMENTTIME":20000,
            "MACHINE_DEFAULT_XYJERK":20.0,"MACHINE_DEFAULT_ZJERK":0.2,"MACHINE_GENERATE_FRAME_TIME":450.0,
            "MACHINE_MAX_ACCELERATION":[1000,1000,160,1000],"MACHINE_MAX_FEEDRATE":[200.0,200.0,20.0,45.0],
            "MACHINE_MAX_STEP_FREQUENCY":256_000,"MACHINE_MINIMUM_PLANNER_SPEED":0.05,
            "MACHINE_MODE_ACCELERATION":[0,0,0,0],"MACHINE_NOR_LAYER_DOWN_HEIGHT_DIV":0.25,
            "MACHINE_NOR_LAYER_DOWN_SPEED_DIV":0.5,"MACHINE_NOR_LAYER_UP_HEIGHT_DIV":0.25,
            "MACHINE_NOR_LAYER_UP_SPEED_DIV":0.5,"MACHINE_STEP_MUL":1,"MACHINE_TIME_COMPENSATE":0.0,
            "MACHINE_TIM_PRES":30.0,"MACHINE_TIM_RCC_CLK":60.0,"TIMES_COMPENSATE":[0,0,0,0]}),
        json!({"energy_coeff":0.0,"machine_exposure_ton":0.4,"precision_coeff_value":[0.024,0.01,-0.2],
            "precision_per_volume":5.0,"precision_range_branch":[0.0,5.0,25.0]}),
    )
}

/// The P1's resin motion limit every reader expects in a resin's `slice_extpara` (TD-117): the
/// value the official sample, Lychee and CHITUBOX all write.
const MAX_ACCELERATION: f64 = 2.0;

/// The resin's identity as Photon Workshop names one, `brand@resin@film@setting`: `@` in the
/// user's resin name would split it, so it becomes `-`.
fn resin_identity(resin_name: &str) -> (String, String) {
    let resin = if resin_name.trim().is_empty() {
        "resin".to_owned()
    } else {
        resin_name.replace('@', "-")
    };
    let name = format!("karasu@{resin}@{FILM}@karasu");
    (resin, name)
}

/// The vat film every P1 file names: a resin profile's setting is for this film.
const FILM: &str = "NFEP";

fn rotate_z(stored_mirror: [bool; 2]) -> Option<f64> {
    match stored_mirror {
        [false, true] => Some(0.),
        [true, false] => Some(180.),
        _ => None,
    }
}

impl Pp1Encoder {
    pub(crate) fn validate_metadata(job: &WireJob) -> Result<(), CodecError> {
        if rotate_z(job.stored_mirror).is_none() {
            return Err(CodecError::Invalid(
                "PP1 declares only a turn: the printer must mirror in both X and Y, or in neither",
            ));
        }
        super::machine::validate_antialiasing(job.antialiasing)?;
        if job.resolution.contains(&0)
            || job.resolution.iter().any(|&n| n > 65536)
            || job.layer_count == 0
            || job.layer_count > 65525
            || job
                .bottom_count
                .checked_add(job.transition_count)
                .is_none_or(|n| n > job.layer_count)
            || job.bottom_count > u16::MAX.into()
            || job.transition_count > u16::MAX.into()
            || job.machine_name.is_empty()
            || job.machine_name.len() > 1024
            || job.machine_name.contains('\0')
            || job.layer_height_mm <= 0.
        {
            return Err(CodecError::Invalid(
                "PP1 dimensions, counts, machine identity or layer height",
            ));
        }
        for v in job.build_mm {
            number(v)?;
            if v <= 0. {
                return Err(CodecError::Invalid("PP1 build dimensions"));
            }
        }
        number(job.layer_height_mm)?;
        for v in [job.estimated_time_s, job.volume_ml] {
            number(v)?;
            if v < 0. {
                return Err(CodecError::Invalid("PP1 negative estimate"));
            }
        }
        settings(&job.normal)?;
        settings(&job.bottom)?;
        Ok(())
    }

    #[allow(clippy::float_cmp)] // Unsupported overrides must be detected exactly, without silently changing them.
    pub(crate) fn validate_layer(
        job: &WireJob,
        layer: &WireLayer,
        index: u32,
    ) -> Result<(), CodecError> {
        if index >= job.layer_count || layer.z_mm <= 0. || layer.z_mm > job.build_mm[2] {
            return Err(CodecError::Invalid(
                "PP1 layer identity or Z outside build volume",
            ));
        }
        number(layer.z_mm)?;
        settings(&layer.settings)?;
        let expected = if index < job.bottom_count {
            &job.bottom
        } else {
            &job.normal
        };
        let actual = &layer.settings;
        if actual.lift != expected.lift
            || actual.retract != expected.retract
            || actual.wait_before_exposure_s != expected.wait_before_exposure_s
            || actual.wait_after_exposure_s != expected.wait_after_exposure_s
            || actual.wait_after_lift_s != expected.wait_after_lift_s
            || actual.pwm != expected.pwm
        {
            return Err(CodecError::Unsupported(
                "PP1 per-layer overrides currently support exposure/Z only",
            ));
        }
        Ok(())
    }

    pub(crate) fn begin(
        job: WireJob,
        writer: &mut dyn WriteSeek,
        limits: EncoderLimits,
        quantization: Quantization,
        cancel: &mut Cancellation<'_>,
    ) -> Result<Self, CodecError> {
        cancel.check(Checkpoint::ExportStart)?;
        Self::validate_metadata(&job)?;
        if let Some(previews) = &job.previews {
            check_previews(previews, PP1_PREVIEW_SIZES)?;
        }
        if writer.stream_position()? != 0 {
            return Err(CodecError::Invalid("PP1 output must start at byte zero"));
        }
        // Controller JSON is generated incrementally at finish; scene and wire records stay bounded.
        let rows =
            usize::try_from(job.layer_count).map_err(|_| CodecError::Limit("PP1 layer count"))?;
        let metadata_bound = rows
            .checked_mul(640)
            .and_then(|n| n.checked_add(524_288))
            .and_then(|n| n.checked_add((job.resolution[0] as usize).checked_mul(80)?))
            .ok_or(CodecError::Limit("PP1 metadata arithmetic"))?;
        if metadata_bound > limits.max_metadata_bytes
            || metadata_bound as u64 > limits.max_output_bytes
        {
            return Err(CodecError::Limit(
                "PP1 retained controller/scene metadata budget",
            ));
        }
        let mut layers = Vec::new();
        layers
            .try_reserve_exact(rows)
            .map_err(|_| CodecError::Limit("PP1 controller allocation"))?;
        let mut projection = Vec::new();
        projection
            .try_reserve_exact(224 * 168)
            .map_err(|_| CodecError::Limit("PP1 preview allocation"))?;
        projection.resize(224 * 168, 0);
        let mut result = Self {
            job,
            limits,
            quantization,
            zip: ClassicZip::new(limits),
            scene: Scene::new(),
            layers,
            previous_z: 0.,
            encoded_volume_ml: 0.,
            projection,
        };
        let manifest = result.manifest();
        result.add_json(writer, "anycubic_photon_resins.pwsp", &manifest, cancel)?;
        result.add_json(writer, "software_info.conf", &json!({"mark":"karasu-slicer","version":env!("CARGO_PKG_VERSION"),"os":std::env::consts::OS,"opengl":"none"}), cancel)?;
        result.add_json(writer, "lcd_function.json", &json!({"models_processed_info":{"models":[],"scene_models_from":0,"slice_paras_process":1,"software_version":concat!("karasu-slicer ",env!("CARGO_PKG_VERSION"))},"rerf_function":{"enable":false,"model_name":"","model_type":1,"partition_exposure_array":[],"partition_num":0}}), cancel)?;
        Ok(result)
    }

    fn add_json(
        &mut self,
        writer: &mut dyn WriteSeek,
        name: &str,
        value: &Value,
        cancel: &mut Cancellation<'_>,
    ) -> Result<(), CodecError> {
        let data =
            serde_json::to_vec(value).map_err(|_| CodecError::Invalid("PP1 JSON metadata"))?;
        self.add_deflated(writer, name, data, cancel)
    }

    /// Add `data` as member `name`, deflated when that makes it smaller (TD-133).
    fn add_deflated(
        &mut self,
        writer: &mut dyn WriteSeek,
        name: &str,
        data: Vec<u8>,
        cancel: &mut Cancellation<'_>,
    ) -> Result<(), CodecError> {
        let member = Member::deflated(data, cancel)?;
        self.zip.add_member(writer, name, &member, cancel)
    }

    fn manifest(&self) -> Value {
        let j = &self.job;
        let n = &j.normal;
        let b = &j.bottom;
        let stages = |s: &WireSettings, band: usize| {
            let [height, up, down] = bands(s)[band];
            json!({"height":height,"up_speed":up,"down_speed":down})
        };
        let multi = n.lift[1][0] != 0. || b.lift[1][0] != 0.;
        let (print_time_paras, exp_time_paras) = firmware_constants();
        let (resin, resin_id) = resin_identity(&j.resin_name);
        // Geometry is job-owned. No commercial preset or material calibration is copied; the
        // firmware constants are the machine's (TD-117). Photon Workshop opens a file only with
        // the image format `pwszImg` (its only one for the P1, whatever the members' suffix),
        // the format's identity numbers, preview colours, a `picture` and a complete resin.
        json!({"version":"3","machine_type":{
            "version":"3","name":j.machine_name,"key_suffix":"pp1","key_image_format":"pwszImg",
            "max_file_version":518,"property":119,"raster_segments_capacity":100_000,
            "res_x":j.resolution[0],"res_y":j.resolution[1],
            "xy_pixel":j.build_mm[0]*1000./f64::from(j.resolution[0]),"xy_pixel_y":j.build_mm[1]*1000./f64::from(j.resolution[1]),
            // Validated: the stored image is either karasu-slicer's or turned half a turn.
            "rotate_z":rotate_z(j.stored_mirror).unwrap_or_default(),"max_samples":16,"print_xsize":j.build_mm[0],"print_ysize":j.build_mm[1],"print_zsize":j.build_mm[2],
            "prev_image_size":[224,168],"prev2_image_size":[336,252],"cloudprev_imag_size":[800,600],
            // Background, model and support colours for a reader's own renders: karasu's.
            "prev_back_color":[0.11,0.12,0.14],"prev_model_color":[0.80,0.80,0.80],
            "prev_supports_color":[0.36,0.55,0.95],"prev2_back_color":[0.11,0.12,0.14],
            "cloudprev_back_color":[0.93,0.94,0.96],
            "child_screen":[{"x":0,"y":0,"width":j.resolution[0],"height":j.resolution[1]}],
            "raster_antialiasing":1,"print_platform_zheights":[0.]},
            "machine_extern":{"version":"3","alias":j.machine_name,"picture":"","cloud_property":0,"device_cn_code":"",
                "factory_resins":[],"active_resins":[resin_id],"firmware_calc_print_time":0,
                "firmware_calc_print_time_paras":print_time_paras,"firmware_calc_exp_time_paras":exp_time_paras,
                // Unknown price and volume stay zero, as unknown density does; the P1 heats
                // the vat to the target (it showed 20 °C for a file without one, TD-118).
                "user_resins":[{"version":"2","property":{"version":"4","name":resin_id,"type":"User supplied",
                    "code":"","currency":"","price":0.0,"volume":0.0,"subfunc_code":0,
                    "density":j.resin_density_g_per_ml,"target_temperature":j.heating_temperature_c,
                    "brand_name":"karasu","resin_name":resin,"film_name":FILM,"setting_name":"karasu"},
                    "slice_extpara":{"version":"3","multi_state_used":u8::from(multi),"max_acceleration":MAX_ACCELERATION,"transition_layercount":j.transition_count,"transition_type":0,
                    "multi_state_paras":{"bott_0":stages(b,0),"bott_1":stages(b,1),"normal_0":stages(n,0),"normal_1":stages(n,1)},
                    "exposure_compensate":0.,"intelli_mode":0,"separate_support_exposure_delayed":0.,"material_scale_coeffs":[],"material_scale_coeffs_mode":0,"material_scale_xyz":{"x":1.,"y":1.,"z":1.}},
                    "slicepara":{"anti_count":j.antialiasing,"blur_level":0,"gray_level":0,"bott_layers":j.bottom_count,"bott_time":b.exposure_s,"bott_time_dual":b.exposure_s,"exposure_time":n.exposure_s,
                    "off_time":n.wait_before_exposure_s,"bott_off_time":b.wait_before_exposure_s,
                    "wait_before_lift":n.wait_after_exposure_s,"bott_wait_before_lift":b.wait_after_exposure_s,
                    "wait_after_lift":n.wait_after_lift_s,"bott_wait_after_lift":b.wait_after_lift_s,
                    "use_indivi_layerpara":1,"use_random_erode":0,"zthick":j.layer_height_mm,
                    "zup_height":n.lift[0][0]+n.lift[1][0],"zup_speed":n.lift[0][1],"zdown_speed":n.retract[0][1]}}]}})
    }

    pub(crate) fn write_layer(
        &mut self,
        writer: &mut dyn WriteSeek,
        layer: &WireLayer,
        mask: &SpanMask,
        cancel: &mut Cancellation<'_>,
    ) -> Result<(), CodecError> {
        cancel.check(Checkpoint::ExportLayer)?;
        let index = self.precheck(layer, mask)?;
        let payload = Self::encode(
            self.quantization,
            self.limits,
            self.job.resolution,
            mask,
            cancel,
        )?;
        self.emit(writer, layer, index, mask, payload, cancel)
    }

    /// [`Self::write_layer`] for a payload encoded beforehand by [`Self::encode`] (possibly on
    /// another thread): the same checks in the same order, then the same bytes.
    pub(crate) fn write_encoded(
        &mut self,
        writer: &mut dyn WriteSeek,
        layer: &WireLayer,
        mask: &SpanMask,
        payload: Result<EncodedPayload, CodecError>,
        cancel: &mut Cancellation<'_>,
    ) -> Result<(), CodecError> {
        cancel.check(Checkpoint::ExportLayer)?;
        let index = self.precheck(layer, mask)?;
        self.emit(writer, layer, index, mask, payload?, cancel)
    }

    /// Quantization and limits of [`Self::encode`] for this session.
    pub(crate) const fn encoding_parameters(&self) -> (Quantization, EncoderLimits, [u32; 2]) {
        (self.quantization, self.limits, self.job.resolution)
    }

    /// Pixels at or below this value are empty in the scene metrics: the format's zero bin.
    const fn zero_threshold(quantization: Quantization) -> u8 {
        if matches!(quantization, Quantization::RoundToFormat) {
            8
        } else {
            0
        }
    }

    /// Pure per-layer PW0 encoding, intensity sum and scene metrics at `resolution`; it depends on
    /// no session state, so it can run on any thread.
    pub(crate) fn encode(
        quantization: Quantization,
        limits: EncoderLimits,
        resolution: [u32; 2],
        mask: &SpanMask,
        cancel: &mut Cancellation<'_>,
    ) -> Result<EncodedPayload, CodecError> {
        let (bytes, intensity) =
            encode_pw0_with_sum(mask, quantization, limits.max_layer_bytes, cancel)?;
        let scene = layer_metrics(mask, resolution, Self::zero_threshold(quantization), cancel)?;
        // Deflated here, on the encoding's thread (TD-133).
        let member = Member::deflated(bytes, cancel)?;
        Ok(EncodedPayload {
            lit: None,
            bytes: Vec::new(),
            intensity,
            scene: Some(scene),
            member: Some(member),
        })
    }

    /// Session-state checks of one layer; returns its index.
    fn precheck(&self, layer: &WireLayer, mask: &SpanMask) -> Result<u32, CodecError> {
        let index = u32::try_from(self.layers.len()).map_err(|_| CodecError::Limit("PP1 index"))?;
        Self::validate_layer(&self.job, layer, index)?;
        if layer.z_mm <= self.previous_z || number(layer.z_mm)? <= number(self.previous_z)? {
            return Err(CodecError::Invalid(
                "PP1 layer Z must increase in f64 and binary32",
            ));
        }
        if [mask.width(), mask.height()] != self.job.resolution
            || mask.pixel_count() > self.limits.max_layer_bytes
        {
            return Err(CodecError::Limit(
                "PP1 input dimensions or layer byte budget",
            ));
        }
        Ok(index)
    }

    /// Record the scene and projection, store the payload, then advance the session.
    fn emit(
        &mut self,
        writer: &mut dyn WriteSeek,
        layer: &WireLayer,
        index: u32,
        mask: &SpanMask,
        payload: EncodedPayload,
        cancel: &mut Cancellation<'_>,
    ) -> Result<(), CodecError> {
        let EncodedPayload {
            bytes,
            intensity: intensity_sum,
            scene,
            lit: _,
            member,
        } = payload;
        let member = match member {
            Some(member) => member,
            None => Member::deflated(bytes, cancel)?,
        };
        let encoded_volume_ml = self.encoded_volume_ml
            + coverage_volume_ml(intensity_sum, &self.job, layer.z_mm - self.previous_z);
        number(encoded_volume_ml)?;
        let metrics = match scene {
            Some(metrics) => metrics,
            None => layer_metrics(
                mask,
                self.job.resolution,
                Self::zero_threshold(self.quantization),
                cancel,
            )?,
        };
        self.scene.push_metrics(
            layer.z_mm,
            &metrics,
            self.job.resolution,
            [self.job.build_mm[0], self.job.build_mm[1]],
            cancel,
        )?;
        self.update_projection(mask, cancel)?;
        self.zip.add_member(
            writer,
            &format!("layer_images/layer_{index}.pw0Img"),
            &member,
            cancel,
        )?;
        self.layers.push(layer.clone());
        self.previous_z = layer.z_mm;
        self.encoded_volume_ml = encoded_volume_ml;
        Ok(())
    }

    fn update_projection(
        &mut self,
        mask: &SpanMask,
        cancel: &mut Cancellation<'_>,
    ) -> Result<(), CodecError> {
        let [width, height] = self.job.resolution.map(|n| n as usize);
        // Pixel-center nearest-neighbor projection is deterministic and explicitly a mask preview.
        for y in 0..168 {
            cancel.poll(Checkpoint::ExportLayer, y)?;
            let source_y = ((2 * y + 1) * height / (2 * 168)).min(height - 1);
            for x in 0..224 {
                let source_x = ((2 * x + 1) * width / (2 * 224)).min(width - 1);
                let sample = to_u32(source_x)
                    .and_then(|x| Some((x, to_u32(source_y)?)))
                    .and_then(|(x, y)| mask.pixel(x, y))
                    .ok_or(CodecError::Invalid("PP1 projection sample"))?;
                let q = quantized(sample, self.quantization)?;
                let target = &mut self.projection[y * 224 + x];
                *target = (*target).max(q * 17);
            }
        }
        Ok(())
    }

    pub(crate) fn finish(
        mut self,
        writer: &mut dyn WriteSeek,
        cancel: &mut Cancellation<'_>,
    ) -> Result<(), CodecError> {
        cancel.check(Checkpoint::ExportFinish)?;
        if self.layers.len() != self.job.layer_count as usize {
            return Err(CodecError::Invalid("PP1 missing layers"));
        }
        let mut controller = Vec::new();
        controller
            .try_reserve_exact(self.layers.len() * 256 + 32)
            .map_err(|_| CodecError::Limit("PP1 layer JSON allocation"))?;
        write!(
            &mut controller,
            "{{\"count\":{},\"paras\":[",
            self.layers.len()
        )?;
        let mut previous = 0.;
        for (index, layer) in self.layers.iter().enumerate() {
            cancel.poll(Checkpoint::ExportFinish, index)?;
            if index != 0 {
                controller.push(b',');
            }
            let value = json!({"exposure_time":layer.settings.exposure_s,"layer_index":index,"layer_minheight":previous,"layer_thickness":layer.z_mm-previous,"zup_height":layer.settings.lift[0][0]+layer.settings.lift[1][0],"zup_speed":layer.settings.lift[0][1]});
            serde_json::to_writer(&mut controller, &value)
                .map_err(|_| CodecError::Invalid("PP1 layer JSON"))?;
            if controller.len() > self.limits.max_metadata_bytes {
                return Err(CodecError::Limit("PP1 layer JSON budget"));
            }
            previous = layer.z_mm;
        }
        controller.extend(b"]}");
        self.add_deflated(writer, "layers_controller.conf", controller, cancel)?;
        self.add_json(writer,"print_info.json",&json!({"model_layers_count":self.job.layer_count,"print_time":self.job.estimated_time_s,"volume":self.encoded_volume_ml,"sub_estimated_infos":[]}),cancel)?;
        let scene = self.scene.to_bytes(
            concat!("karasu-slicer ", env!("CARGO_PKG_VERSION")),
            self.limits.max_metadata_bytes,
        )?;
        self.add_deflated(writer, "scene.slice", scene, cancel)?;
        // Rendered model thumbnails; without them, the masks' maximum projection in gray. Both
        // opaque RGBA, as every P1 reader's own previews are (TD-117).
        let previews = self.job.previews.take();
        for (index, [width, height]) in [[224usize, 168usize], [336, 252]].into_iter().enumerate() {
            let mut png = Vec::new();
            let projection = &self.projection;
            // `begin` checked rendered previews against these sizes.
            let rendered = previews.as_ref().map(|p| [&p.small, &p.large][index].rgb());
            let mut fill = |y: usize, row: &mut [u8]| {
                let (pixels, _) = row.as_chunks_mut::<4>();
                for (x, pixel) in pixels.iter_mut().enumerate() {
                    let [r, g, b] = match rendered {
                        Some(rgb) => {
                            let at = (y * width + x) * 3;
                            [rgb[at], rgb[at + 1], rgb[at + 2]]
                        }
                        None => [projection[(y * 168 / height) * 224 + x * 224 / width]; 3],
                    };
                    *pixel = [r, g, b, 255];
                }
            };
            let encoded = crate::raster::png::encode_rgba(
                &mut png,
                u32::try_from(width).map_err(|_| CodecError::Limit("PP1 preview width"))?,
                u32::try_from(height).map_err(|_| CodecError::Limit("PP1 preview height"))?,
                &mut fill,
                cancel,
            );
            encoded.map_err(|e| match e {
                crate::raster::png::PngError::Cancelled(c) => CodecError::Cancelled(c),
                crate::raster::png::PngError::Io(e) => CodecError::Io(e),
                _ => CodecError::Invalid("PP1 preview PNG"),
            })?;
            self.add_deflated(
                writer,
                &format!("preview_images/preview_{index}.png"),
                png,
                cancel,
            )?;
        }
        self.zip.finish(writer, cancel)
    }
}

/// The `[small, large]` PP1 preview sizes; the gray projection fallback uses the same sizes.
pub(crate) const PP1_PREVIEW_SIZES: [[u32; 2]; 2] = [[224, 168], [336, 252]];

fn quantized(pixel: u8, policy: Quantization) -> Result<u8, CodecError> {
    if policy == Quantization::Exact && !pixel.is_multiple_of(17) {
        return Err(CodecError::Unsupported(
            "PP1 PW0 stores only grayscale multiples of 17; choose RoundToFormat",
        ));
    }
    u8::try_from((u16::from(pixel) + 8) / 17)
        .map_err(|_| CodecError::Invalid("PP1 quantization domain"))
}

#[allow(clippy::cast_precision_loss)] // Validated dimensions <=65536 bound intensity_sum below 2^40, exactly representable in f64.
fn coverage_volume_ml(intensity_sum: u64, job: &WireJob, thickness_mm: f64) -> f64 {
    let pixel_area_mm2 = job.build_mm[0] / f64::from(job.resolution[0])
        * (job.build_mm[1] / f64::from(job.resolution[1]));
    intensity_sum as f64 / 255. * pixel_area_mm2 * thickness_mm / 1000.
}

fn to_u32(n: usize) -> Option<u32> {
    u32::try_from(n).ok()
}

/// One-row mask of `pixels`, for tests.
#[cfg(test)]
fn row(pixels: &[u8]) -> SpanMask {
    SpanMask::from_pixels(u32::try_from(pixels.len()).unwrap(), 1, pixels).unwrap()
}

#[cfg(test)]
fn encode_pw0(
    pixels: &[u8],
    policy: Quantization,
    limit: usize,
    cancel: &mut Cancellation<'_>,
) -> Result<Vec<u8>, CodecError> {
    encode_pw0_with_sum(&row(pixels), policy, limit, cancel).map(|(bytes, _)| bytes)
}

/// PW0 runs of quantized levels: up to 4095 pixels of level 0 or 15 in two bytes, up to 15 of
/// any other level in one; work is proportional to the mask runs.
pub(crate) fn encode_pw0_with_sum(
    mask: &SpanMask,
    policy: Quantization,
    limit: usize,
    cancel: &mut Cancellation<'_>,
) -> Result<(Vec<u8>, u64), CodecError> {
    let mut out = Vec::new();
    out.try_reserve(mask.pixel_count().min(limit).min(65536))
        .map_err(|_| CodecError::Limit("PP1 PW0 allocation"))?;
    let mut intensity_sum = 0u64;
    let mut pending: Option<(u8, usize)> = None;
    let mut emit = |level: u8, mut remaining: usize, out: &mut Vec<u8>| {
        let max_run = if level == 0 || level == 15 { 4095 } else { 15 };
        while remaining > 0 {
            let length = remaining.min(max_run);
            remaining -= length;
            let required = if level == 0 || level == 15 { 2 } else { 1 };
            if out.len().checked_add(required).is_none_or(|n| n > limit) {
                return Err(CodecError::Limit("PP1 encoded layer byte budget"));
            }
            out.try_reserve(required)
                .map_err(|_| CodecError::Limit("PP1 PW0 allocation"))?;
            if required == 2 {
                out.extend(
                    (u16::from(level) << 12
                        | u16::try_from(length)
                            .map_err(|_| CodecError::Limit("PP1 run length"))?)
                    .to_be_bytes(),
                );
            } else {
                out.push(
                    level << 4
                        | u8::try_from(length)
                            .map_err(|_| CodecError::Limit("PP1 short run length"))?,
                );
            }
            intensity_sum = intensity_sum
                .checked_add(
                    u64::from(level)
                        * 17
                        * u64::try_from(length)
                            .map_err(|_| CodecError::Limit("PP1 intensity run"))?,
                )
                .ok_or(CodecError::Limit("PP1 intensity sum"))?;
        }
        Ok(())
    };
    let mut index = 0usize;
    mask.for_each_run(|value, length| {
        cancel.poll(Checkpoint::ExportLayer, index)?;
        index += 1;
        let level = quantized(value, policy)?;
        match &mut pending {
            Some((current, total)) if *current == level => *total += length,
            _ => {
                if let Some((current, total)) = pending.take() {
                    emit(current, total, &mut out)?;
                }
                pending = Some((level, length));
            }
        }
        Ok::<_, CodecError>(())
    })?;
    if let Some((current, total)) = pending {
        emit(current, total, &mut out)?;
    }
    Ok((out, intensity_sum))
}

/// The previous dense PW0 encoder (before TD-055), kept as a test oracle.
#[cfg(test)]
fn dense_encode_pw0_with_sum(
    pixels: &[u8],
    policy: Quantization,
    limit: usize,
    cancel: &mut Cancellation<'_>,
) -> Result<(Vec<u8>, u64), CodecError> {
    let mut out = Vec::new();
    out.try_reserve(pixels.len().min(limit).min(65536))
        .map_err(|_| CodecError::Limit("PP1 PW0 allocation"))?;
    let mut offset = 0;
    let mut intensity_sum = 0u64;
    while offset < pixels.len() {
        cancel.poll(Checkpoint::ExportLayer, offset)?;
        let level = quantized(pixels[offset], policy)?;
        let max_run = if level == 0 || level == 15 { 4095 } else { 15 };
        let mut length = 1;
        while length < max_run && offset + length < pixels.len() {
            cancel.poll(Checkpoint::ExportLayer, offset + length)?;
            if quantized(pixels[offset + length], policy)? != level {
                break;
            }
            length += 1;
        }
        let required = if level == 0 || level == 15 { 2 } else { 1 };
        if out.len().checked_add(required).is_none_or(|n| n > limit) {
            return Err(CodecError::Limit("PP1 encoded layer byte budget"));
        }
        out.try_reserve(required)
            .map_err(|_| CodecError::Limit("PP1 PW0 allocation"))?;
        if required == 2 {
            out.extend(
                (u16::from(level) << 12
                    | u16::try_from(length).map_err(|_| CodecError::Limit("PP1 run length"))?)
                .to_be_bytes(),
            );
        } else {
            out.push(
                level << 4
                    | u8::try_from(length)
                        .map_err(|_| CodecError::Limit("PP1 short run length"))?,
            );
        }
        intensity_sum = intensity_sum
            .checked_add(
                u64::from(level)
                    * 17
                    * u64::try_from(length).map_err(|_| CodecError::Limit("PP1 intensity run"))?,
            )
            .ok_or(CodecError::Limit("PP1 intensity sum"))?;
        offset += length;
    }
    Ok((out, intensity_sum))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preview::{PreviewImage, Previews};
    use std::io::Cursor;

    fn setting(exposure_s: f64) -> WireSettings {
        WireSettings {
            exposure_s,
            wait_before_exposure_s: 0.5,
            wait_after_exposure_s: 0.2,
            wait_after_lift_s: 0.3,
            pwm: 255,
            lift: [[2., 1.], [1., 2.]],
            retract: [[1., 3.], [2., 4.]],
        }
    }
    fn job() -> WireJob {
        WireJob {
            heating_temperature_c: 31.0,
            resolution: [3, 2],
            build_mm: [3., 2., 5.],
            layer_height_mm: 0.05,
            bottom_count: 1,
            transition_count: 1,
            layer_count: 3,
            machine_name: "Anycubic Photon P1".into(),
            normal: setting(2.),
            bottom: setting(20.),
            estimated_time_s: 42.,
            volume_ml: 0.001,
            antialiasing: 1,
            previews: None,
            job_name: "test".into(),
            resin_name: "Test resin".into(),
            resin_density_g_per_ml: 1.1,
            stored_mirror: [false, true],
        }
    }
    fn limits() -> EncoderLimits {
        EncoderLimits {
            max_output_bytes: 4 * 1024 * 1024,
            max_metadata_bytes: 1024 * 1024,
            max_layer_bytes: 1024 * 1024,
        }
    }
    fn decode(data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < data.len() {
            let b = data[i];
            i += 1;
            let level = b >> 4;
            let mut count = usize::from(b & 15);
            if level == 0 || level == 15 {
                count = count * 256 + usize::from(data[i]);
                i += 1;
            }
            assert_ne!(count, 0);
            out.extend(std::iter::repeat_n(level * 17, count));
        }
        out
    }
    /// The members of a written PP1, inflated when deflated, each checked against its CRC-32
    /// and size (TD-133).
    fn entries(data: &[u8]) -> std::collections::BTreeMap<String, Vec<u8>> {
        let mut entries = std::collections::BTreeMap::new();
        let mut at = 0;
        while data[at..at + 4] == *b"PK\x03\x04" {
            let method = u16::from_le_bytes(data[at + 8..at + 10].try_into().unwrap());
            let crc = u32::from_le_bytes(data[at + 14..at + 18].try_into().unwrap());
            let packed = u32::from_le_bytes(data[at + 18..at + 22].try_into().unwrap()) as usize;
            let size = u32::from_le_bytes(data[at + 22..at + 26].try_into().unwrap()) as usize;
            let name_len = u16::from_le_bytes(data[at + 26..at + 28].try_into().unwrap()) as usize;
            let extra_len = u16::from_le_bytes(data[at + 28..at + 30].try_into().unwrap()) as usize;
            let name = std::str::from_utf8(&data[at + 30..at + 30 + name_len]).unwrap();
            let start = at + 30 + name_len + extra_len;
            let raw = &data[start..start + packed];
            let member = match method {
                0 => raw.to_vec(),
                8 => miniz_oxide::inflate::decompress_to_vec(raw).unwrap(),
                other => panic!("method {other}"),
            };
            assert_eq!(member.len(), size, "{name}");
            assert_eq!(
                super::super::zip::crc32(&member, &mut Cancellation::never()).unwrap(),
                crc,
                "{name}"
            );
            entries.insert(name.into(), member);
            at = start + packed;
        }
        entries
    }
    #[test]
    fn pw0_all_gray_levels_and_explicit_nearest_quantization() {
        let all: Vec<u8> = (0..=255).collect();
        assert!(matches!(
            encode_pw0(&all, Quantization::Exact, 1024, &mut Cancellation::never()),
            Err(CodecError::Unsupported(_))
        ));
        let bytes = encode_pw0(
            &all,
            Quantization::RoundToFormat,
            1024,
            &mut Cancellation::never(),
        )
        .unwrap();
        let values: Vec<u8> = all
            .iter()
            .map(|&n| u8::try_from(((u16::from(n) + 8) / 17) * 17).unwrap())
            .collect();
        assert_eq!(decode(&bytes), values);
        assert_eq!(
            decode(
                &encode_pw0(
                    &values,
                    Quantization::Exact,
                    1024,
                    &mut Cancellation::never()
                )
                .unwrap()
            ),
            values
        );
    }
    #[test]
    #[allow(clippy::cast_possible_truncation)] // Small test dimensions, indices and levels.
    fn run_pw0_matches_the_dense_oracle_on_random_masks() {
        let mut state = 0x9e37_79b9_7f4a_7c15_u64;
        let mut next = |m: u64| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state % m
        };
        for case in 0..300 {
            let (w, h) = (1 + next(60) as u32, 1 + next(8) as u32);
            let levels: &[u8] = if case % 2 == 0 {
                &[0, 17, 136, 255]
            } else {
                &[0, 8, 9, 126, 255]
            };
            let mut pixels = Vec::new();
            while pixels.len() < (w * h) as usize {
                let value = levels[next(levels.len() as u64) as usize];
                let run = if next(4) == 0 {
                    1 + next(5000)
                } else {
                    1 + next(20)
                };
                pixels.extend(std::iter::repeat_n(value, run as usize));
            }
            pixels.truncate((w * h) as usize);
            let mask = SpanMask::from_pixels(w, h, &pixels).unwrap();
            for policy in [Quantization::Exact, Quantization::RoundToFormat] {
                let never = || Cancellation::never();
                assert_eq!(
                    encode_pw0_with_sum(&mask, policy, usize::MAX, &mut never()).ok(),
                    dense_encode_pw0_with_sum(&pixels, policy, usize::MAX, &mut never()).ok(),
                    "case {case}"
                );
            }
        }
    }
    #[test]
    fn pw0_runs_cross_rows_and_split_at_protocol_limits() {
        for level in [0u8, 17, 136, 238, 255] {
            for count in [1, 14, 15, 16, 4094, 4095, 4096, 8191] {
                let pixels = vec![level; count];
                let out = encode_pw0(
                    &pixels,
                    Quantization::Exact,
                    2000,
                    &mut Cancellation::never(),
                )
                .unwrap();
                assert_eq!(decode(&out), pixels);
                if level == 0 || level == 255 {
                    assert_eq!(out.len(), 2 * count.div_ceil(4095));
                } else {
                    assert_eq!(out.len(), count.div_ceil(15));
                }
            }
        }
    }
    /// What Photon Workshop needs to open the file (TD-117): its image format for the P1, the
    /// format's identity, preview colours, a picture, the firmware constants and a complete
    /// resin, the active one by name.
    fn readers_accept(settings: &Value) {
        let machine = &settings["machine_type"];
        assert_eq!(machine["key_image_format"], "pwszImg");
        assert_eq!(
            (&machine["max_file_version"], &machine["property"]),
            (&json!(518), &json!(119))
        );
        for colour in [
            "prev_back_color",
            "prev_model_color",
            "prev_supports_color",
            "prev2_back_color",
            "cloudprev_back_color",
        ] {
            assert_eq!(
                machine[colour].as_array().map(Vec::len),
                Some(3),
                "{colour}"
            );
        }
        let machine = &settings["machine_extern"];
        assert_eq!(machine["picture"], "");
        assert_eq!(
            machine["firmware_calc_print_time_paras"]["MACHINE_AXIS_STEPS_PER_UNIT"],
            json!([100.0, 100.0, 3200.0, 94.0])
        );
        assert_eq!(
            machine["firmware_calc_exp_time_paras"]["machine_exposure_ton"],
            0.4
        );
        let profile = &settings["machine_extern"]["user_resins"][0];
        assert_eq!(machine["active_resins"][0], profile["property"]["name"]);
        for key in [
            "code",
            "currency",
            "price",
            "volume",
            "density",
            "subfunc_code",
            "target_temperature",
            "brand_name",
            "resin_name",
            "film_name",
            "setting_name",
        ] {
            assert!(!profile["property"][key].is_null(), "{key}");
        }
        assert_eq!(profile["slice_extpara"]["max_acceleration"], 2.0);
        // The job's heating target (the test job's 31 °C) is the resin's (TD-118).
        assert_eq!(profile["property"]["target_temperature"], 31.0);
    }
    #[test]
    fn complete_zip_contains_truthful_metadata_and_asymmetric_exact_masks() {
        let j = job();
        let mut out = Cursor::new(Vec::new());
        let mut cancel = Cancellation::never();
        let mut encoder = Pp1Encoder::begin(
            j.clone(),
            &mut out,
            limits(),
            Quantization::Exact,
            &mut cancel,
        )
        .unwrap();
        let masks = [
            [0, 17, 255, 34, 0, 136],
            [0, 0, 0, 0, 0, 0],
            [255, 0, 238, 51, 68, 85],
        ];
        for (index, pixels) in masks.iter().enumerate() {
            let s = if index == 0 {
                j.bottom.clone()
            } else {
                j.normal.clone()
            };
            encoder
                .write_layer(
                    &mut out,
                    &WireLayer {
                        z_mm: 0.05 * f64::from(u32::try_from(index + 1).unwrap()),
                        settings: s,
                    },
                    &SpanMask::from_pixels(3, 2, pixels).unwrap(),
                    &mut cancel,
                )
                .unwrap();
        }
        encoder.finish(&mut out, &mut cancel).unwrap();
        let data = out.into_inner();
        let e = entries(&data);
        assert_eq!(e.len(), 11);
        for (index, mask) in masks.iter().enumerate() {
            assert_eq!(
                decode(&e[&format!("layer_images/layer_{index}.pw0Img")]),
                mask
            );
        }
        let settings: Value = serde_json::from_slice(&e["anycubic_photon_resins.pwsp"]).unwrap();
        assert_eq!(settings["machine_type"]["key_suffix"], "pp1");
        readers_accept(&settings);
        let profile = &settings["machine_extern"]["user_resins"][0];
        assert_eq!(profile["slicepara"]["use_indivi_layerpara"], 1);
        // Band 0 is lifted first and retracted last: its down speed is the last retract stage's.
        let normal = &profile["slice_extpara"]["multi_state_paras"];
        assert_eq!(
            normal["normal_0"],
            json!({"height":2.,"up_speed":1.,"down_speed":4.})
        );
        assert_eq!(
            normal["normal_1"],
            json!({"height":1.,"up_speed":2.,"down_speed":3.})
        );
        let controller: Value = serde_json::from_slice(&e["layers_controller.conf"]).unwrap();
        assert_eq!(controller["count"], 3);
        assert_eq!(controller["paras"][1]["exposure_time"], 2.);
        assert_eq!(e["scene.slice"].len(), 400 + 3 * 64);
        assert_eq!(
            &e["preview_images/preview_0.png"][..8],
            b"\x89PNG\r\n\x1a\n"
        );
        assert!(!String::from_utf8_lossy(&e["software_info.conf"]).contains("PhotonWorkshop"));
    }
    #[test]
    fn a_resin_is_named_as_photon_workshop_names_one() {
        assert_eq!(
            resin_identity("Grey 8K"),
            (
                "Grey 8K".to_owned(),
                "karasu@Grey 8K@NFEP@karasu".to_owned()
            )
        );
        // An `@` would split the name into more parts; no name is still a name.
        assert_eq!(resin_identity("a@b").1, "karasu@a-b@NFEP@karasu");
        assert_eq!(resin_identity(" ").1, "karasu@resin@NFEP@karasu");
    }
    #[test]
    fn rendered_previews_replace_the_gray_projection() {
        let image = |[width, height]: [u32; 2]| {
            let rgb = (0..width * height)
                .flat_map(|i| [u8::try_from(i % 251).unwrap(), 40, 200])
                .collect();
            PreviewImage::new(width, height, rgb).unwrap()
        };
        let previews = Previews {
            small: image(PP1_PREVIEW_SIZES[0]),
            large: image(PP1_PREVIEW_SIZES[1]),
        };
        for previews in [None, Some(previews)] {
            let rendered = previews.is_some();
            let j = WireJob { previews, ..job() };
            let mut out = Cursor::new(Vec::new());
            let mut cancel = Cancellation::never();
            let mut encoder = Pp1Encoder::begin(
                j.clone(),
                &mut out,
                limits(),
                Quantization::Exact,
                &mut cancel,
            )
            .unwrap();
            for index in 0..3 {
                encoder
                    .write_layer(
                        &mut out,
                        &WireLayer {
                            z_mm: 0.05 * f64::from(index + 1),
                            settings: if index == 0 {
                                j.bottom.clone()
                            } else {
                                j.normal.clone()
                            },
                        },
                        &SpanMask::from_pixels(3, 2, &[255; 6]).unwrap(),
                        &mut cancel,
                    )
                    .unwrap();
            }
            encoder.finish(&mut out, &mut cancel).unwrap();
            let data = out.into_inner();
            let e = entries(&data);
            for (index, [width, height]) in PP1_PREVIEW_SIZES.into_iter().enumerate() {
                let png = &e[&format!("preview_images/preview_{index}.png")];
                // IHDR: width, height, bit depth 8, then colour type 6 (RGBA), rendered or
                // gray: P1 readers' own previews are RGBA (TD-117).
                assert_eq!(
                    &png[16..24],
                    [width.to_be_bytes(), height.to_be_bytes()].concat()
                );
                assert_eq!(png[24..26], [8, 6], "rendered: {rendered}");
            }
        }
        let swapped = Previews {
            small: image(PP1_PREVIEW_SIZES[1]),
            large: image(PP1_PREVIEW_SIZES[0]),
        };
        let mut out = Cursor::new(Vec::new());
        let result = Pp1Encoder::begin(
            WireJob {
                previews: Some(swapped),
                ..job()
            },
            &mut out,
            limits(),
            Quantization::Exact,
            &mut Cancellation::never(),
        );
        assert!(matches!(result, Err(CodecError::Invalid(_))));
        assert_eq!(out.into_inner(), Vec::<u8>::new());
    }
    #[test]
    fn capability_rejections_happen_before_output() {
        let mut j = job();
        j.normal.pwm = 128;
        let mut out = Cursor::new(Vec::new());
        assert!(Pp1Encoder::begin(
            j,
            &mut out,
            limits(),
            Quantization::Exact,
            &mut Cancellation::never()
        )
        .is_err());
        assert_eq!(out.into_inner(), [] as [u8; 0]);
        let mut j = job();
        j.normal.retract[0][0] = 2.;
        assert!(Pp1Encoder::validate_metadata(&j).is_err());
        let j = job();
        let mut s = j.normal.clone();
        s.wait_after_lift_s += 1.;
        assert!(Pp1Encoder::validate_layer(
            &j,
            &WireLayer {
                z_mm: 0.1,
                settings: s
            },
            1
        )
        .is_err());
    }
    #[test]
    fn dimensions_resource_bounds_and_incomplete_finish_are_errors() {
        let mut tiny = limits();
        tiny.max_metadata_bytes = 1;
        assert!(Pp1Encoder::begin(
            job(),
            &mut Cursor::new(Vec::new()),
            tiny,
            Quantization::Exact,
            &mut Cancellation::never()
        )
        .is_err());
        let mut j = job();
        j.resolution[0] = 0;
        assert!(Pp1Encoder::validate_metadata(&j).is_err());
        assert!(encode_pw0(
            &[0, 255],
            Quantization::Exact,
            3,
            &mut Cancellation::never()
        )
        .is_err());
        let mut out = Cursor::new(Vec::new());
        let encoder = Pp1Encoder::begin(
            job(),
            &mut out,
            limits(),
            Quantization::Exact,
            &mut Cancellation::never(),
        )
        .unwrap();
        assert!(matches!(
            encoder.finish(&mut out, &mut Cancellation::never()),
            Err(CodecError::Invalid(_))
        ));
    }
    #[test]
    fn cancellation_prevents_first_output_and_interrupts_long_runs() {
        let mut predicate = |_| true;
        let mut cancel = Cancellation::new(&mut predicate);
        let mut out = Cursor::new(Vec::new());
        assert!(matches!(
            Pp1Encoder::begin(job(), &mut out, limits(), Quantization::Exact, &mut cancel),
            Err(CodecError::Cancelled(_))
        ));
        assert_eq!(out.into_inner(), [] as [u8; 0]);
        let mut polls = 0;
        let mut predicate = |_| {
            polls += 1;
            polls > 2
        };
        let mut cancel = Cancellation::new(&mut predicate);
        // Polled every 256 mask runs: alternating pixels give 10000 runs.
        let alternating: Vec<u8> = (0..10000)
            .map(|i| if i % 2 == 0 { 0 } else { 255 })
            .collect();
        assert!(matches!(
            encode_pw0(&alternating, Quantization::Exact, 100_000, &mut cancel),
            Err(CodecError::Cancelled(_))
        ));
    }

    #[test]
    fn print_volume_uses_quantized_grayscale_and_actual_layer_thickness() {
        let j = job();
        let mut out = Cursor::new(Vec::new());
        let mut cancel = Cancellation::never();
        let mut encoder = Pp1Encoder::begin(
            j.clone(),
            &mut out,
            limits(),
            Quantization::RoundToFormat,
            &mut cancel,
        )
        .unwrap();
        // Levels become 0, 0, 17, 119, 136, 255: encoded intensity sum 527 differs from input sum 525.
        let pixels = [0, 8, 9, 126, 128, 254];
        let (_, sum) =
            encode_pw0_with_sum(&row(&pixels), Quantization::RoundToFormat, 100, &mut cancel)
                .unwrap();
        assert_eq!(sum, 527);
        assert_ne!(sum, pixels.iter().map(|&v| u64::from(v)).sum::<u64>());
        for (index, z_mm) in [0.05, 0.12, 0.2].into_iter().enumerate() {
            let settings = if index == 0 {
                j.bottom.clone()
            } else {
                j.normal.clone()
            };
            encoder
                .write_layer(
                    &mut out,
                    &WireLayer { z_mm, settings },
                    &SpanMask::from_pixels(3, 2, &pixels).unwrap(),
                    &mut cancel,
                )
                .unwrap();
        }
        encoder.finish(&mut out, &mut cancel).unwrap();
        let data = out.into_inner();
        let members = entries(&data);
        let info: Value = serde_json::from_slice(&members["print_info.json"]).unwrap();
        let expected = 527. / 255. * 0.2 / 1000.;
        assert!((info["volume"].as_f64().unwrap() - expected).abs() < 1e-15);
        assert!(info.get("weight").is_none() && info.get("cost").is_none());
        assert_eq!(
            decode(&members["layer_images/layer_0.pw0Img"]),
            [0, 0, 17, 119, 136, 255]
        );
    }
}
