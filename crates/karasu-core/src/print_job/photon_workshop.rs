//! Anycubic Photon Workshop binary files (TD-063).
//!
//! An `ANYCUBIC` file mark with a revision (1, 515, 516, 517 or 518) and an address table of
//! sections, in this order:
//! 1. HEADER;
//! 2. PREVIEW;
//! 3. colour table;
//! 4. LAYERDEF;
//! 5. EXTRA;
//! 6. MACHINE;
//! 7. SOFTWARE;
//! 8. MODEL;
//! 9. SUBIMGS;
//! 10. layer images, concatenated.
//!
//! Each revision has a subset of the sections. All values are little-endian, and there is no
//! checksum, footer or encryption. Layer images are PW0 (shared with PP1); the legacy `.pws` uses
//! antialiasing bit-planes of 1-bit runs.
use super::machine::{
    check_previews, checked_u32, set_f32, set_u32, CodecError, Quantization, WireJob, WireLayer,
    WireSettings,
};
use crate::{
    cancellation::{Cancellation, Checkpoint},
    export::AnycubicWorkshop,
    raster::mask::SpanMask,
};

/// Preview size of this family.
pub(crate) const PREVIEW_SIZE: [u32; 2] = [224, 168];
/// LAYERDEF record size.
const RECORD: usize = 32;
/// SUBIMGS record size.
const SUB_RECORD: usize = 44;

/// File-mark size and area count by revision.
const fn mark(version: u32) -> (usize, u32) {
    match version {
        1 => (48, 4),
        515 => (48, 5),
        516 => (52, 8),
        517 => (56, 9),
        _ => (64, 11),
    }
}

/// HEADER section size by revision (its length field is the size minus 16).
const fn header_bytes(version: u32) -> usize {
    match version {
        1 | 515 => 96,
        516 => 100,
        517 => 108,
        _ => 112,
    }
}

fn name(out: &mut Vec<u8>, tag: &str) {
    let mut field = [0u8; 12];
    field[..tag.len()].copy_from_slice(tag.as_bytes());
    out.extend(field);
}

fn fixed_text(out: &mut Vec<u8>, text: &str, n: usize) -> Result<(), CodecError> {
    if !text.is_ascii() || text.len() >= n || text.as_bytes().contains(&0) {
        return Err(CodecError::Invalid(
            "Photon Workshop text must fit its ASCII field",
        ));
    }
    out.extend(text.as_bytes());
    out.resize(out.len() + n - text.len(), 0);
    Ok(())
}

fn push_f32(out: &mut Vec<u8>, value: f64) -> Result<(), CodecError> {
    let at = out.len();
    out.extend([0; 4]);
    set_f32(out, at, value)
}

fn push_u32(out: &mut Vec<u8>, value: u32) {
    out.extend(value.to_le_bytes());
}

/// Offsets patched when the job finishes.
pub(super) struct WorkshopLayout {
    pub bytes: Vec<u8>,
    /// Absolute offset of the HEADER section.
    pub header: u64,
    /// Absolute offset of the first LAYERDEF record.
    pub records: u64,
    /// Absolute offset of the first SUBIMGS record (revision 518).
    pub sub_records: Option<u64>,
    /// Absolute offset of the MODEL section (revisions 517 and 518).
    pub model: Option<u64>,
}

/// Lift and retract in the family's two-stage form: `(lift1 distance, lift1 speed, retract1
/// speed, lift2 distance, lift2 speed, retract2 speed)`, speeds in mm/s.
///
/// Lifting moves `lift1` then `lift2`; retracting covers `lift2`'s distance first (retract1),
/// then `lift1`'s (retract2). A single stage uses its retract speed for both.
fn stages(s: &WireSettings) -> Result<[f64; 6], CodecError> {
    let [[l1, v1], [l2, v2]] = s.lift;
    let [[r1, w1], [r2, w2]] = s.retract;
    if l2 == 0. && r2 == 0. {
        return Ok([l1, v1, w1, 0., 0., w1]);
    }
    #[allow(clippy::float_cmp)] // The family stores no retract distances: they must mirror lift.
    let mirrored = r1 == l2 && r2 == l1;
    if !mirrored {
        return Err(CodecError::Unsupported(
            "Photon Workshop two-stage retract must mirror the lift stages",
        ));
    }
    Ok([l1, v1, w1, l2, v2, w2])
}

/// Capabilities of `target` for one set of settings.
pub(super) fn validate_settings(
    target: AnycubicWorkshop,
    s: &WireSettings,
) -> Result<(), CodecError> {
    if s.wait_after_exposure_s != 0. || s.wait_after_lift_s != 0. {
        return Err(CodecError::Unsupported(
            "Photon Workshop files store only a light-off delay, not rest times",
        ));
    }
    if s.pwm != 255 {
        return Err(CodecError::Unsupported(
            "Photon Workshop files store no light PWM",
        ));
    }
    if (s.lift[1][0] != 0. || s.retract[1][0] != 0.) && target.revision() < 516 {
        return Err(CodecError::Unsupported(
            "two-stage motion needs Photon Workshop revision 516 or later",
        ));
    }
    stages(s).map(|_| ())
}

/// Job-level capabilities of `target`.
#[allow(clippy::float_cmp)] // One stored value must equal both requests exactly.
pub(super) fn validate_job(target: AnycubicWorkshop, job: &WireJob) -> Result<(), CodecError> {
    validate_settings(target, &job.bottom)?;
    validate_settings(target, &job.normal)?;
    if job.bottom.wait_before_exposure_s != job.normal.wait_before_exposure_s {
        return Err(CodecError::Unsupported(
            "Photon Workshop files store one light-off delay for all layers",
        ));
    }
    if target.revision() < 516
        && (job.bottom.lift != job.normal.lift || job.bottom.retract != job.normal.retract)
    {
        return Err(CodecError::Unsupported(
            "distinct bottom motion needs Photon Workshop revision 516 or later",
        ));
    }
    if target.bitplanes() && job.antialiasing > 16 {
        return Err(CodecError::Invalid("at most 16 antialiasing bit-planes"));
    }
    if target.revision() == 518 && job.resolution.iter().any(|&n| n > 65535) {
        return Err(CodecError::Limit("revision 518 stores 16-bit resolution"));
    }
    if !job.machine_name.is_ascii() || job.machine_name.len() >= 96 {
        return Err(CodecError::Invalid(
            "Photon Workshop machine name must fit 95 ASCII bytes",
        ));
    }
    Ok(())
}

/// Every section before the layer images.
#[allow(clippy::too_many_lines)] // One fixed layout, section by section.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // Duration range-checked by preflight.
pub(super) fn header(
    target: AnycubicWorkshop,
    job: &WireJob,
) -> Result<WorkshopLayout, CodecError> {
    let version = target.revision();
    let (mark_bytes, areas) = mark(version);
    let mut out = vec![0; mark_bytes];
    out[..8].copy_from_slice(b"ANYCUBIC");
    set_u32(&mut out, 12, version);
    set_u32(&mut out, 16, areas);
    let slot = |index: usize| 20 + 4 * index;
    // HEADER.
    let header_at = out.len();
    set_u32(&mut out, slot(0), checked_u32(header_at as u64)?);
    let size = header_bytes(version);
    name(&mut out, "HEADER");
    push_u32(&mut out, u32::try_from(size - 16).expect("small"));
    let (normal, bottom) = (&job.normal, &job.bottom);
    let pixel_um = (0..2)
        .map(|k| (job.build_mm[k] / f64::from(job.resolution[k]) * 1000.).round())
        .fold(0.0, f64::max);
    let normal_stages = stages(normal)?;
    for value in [
        pixel_um,
        job.layer_height_mm,
        normal.exposure_s,
        normal.wait_before_exposure_s,
        bottom.exposure_s,
        f64::from(job.bottom_count),
        normal_stages[0] + normal_stages[3],
        normal_stages[1],
        normal_stages[5],
        0.0, // volume, patched at the end
    ] {
        push_f32(&mut out, value)?;
    }
    push_u32(&mut out, u32::from(job.antialiasing.max(1)));
    push_u32(&mut out, job.resolution[0]);
    push_u32(&mut out, job.resolution[1]);
    push_f32(&mut out, 0.0)?; // weight, patched at the end
    push_f32(&mut out, 0.0)?; // price: not modelled
    push_u32(&mut out, 0); // currency
    push_u32(&mut out, 0); // per-layer override flag
    push_u32(
        &mut out,
        if version == 1 {
            0
        } else {
            job.estimated_time_s.ceil() as u32
        },
    );
    if version >= 516 {
        push_u32(&mut out, job.transition_count);
        push_u32(&mut out, 0); // transition type
        push_u32(&mut out, 0); // advanced mode
    }
    if version >= 517 {
        out.extend(u16::from(job.antialiasing.max(1)).to_le_bytes());
        out.extend(0u16.to_le_bytes()); // blur
        push_u32(&mut out, 0); // resin code: not modelled
    }
    out.resize(header_at + size, 0);
    // PREVIEW: 224 x 168 words with blue in the high bits, row-major from the top left.
    let preview_at = out.len();
    set_u32(&mut out, slot(2), checked_u32(preview_at as u64)?);
    let [w, h] = PREVIEW_SIZE;
    name(&mut out, "PREVIEW");
    push_u32(&mut out, w * h * 2 + 12);
    push_u32(&mut out, w);
    push_u32(&mut out, 42);
    push_u32(&mut out, h);
    match &job.previews {
        Some(previews) => {
            check_previews(previews, [PREVIEW_SIZE, PREVIEW_SIZE])?;
            for &[r, g, b] in previews.large.rgb().as_chunks::<3>().0 {
                let word = (u16::from(b >> 3) << 11) | (u16::from(g >> 2) << 5) | u16::from(r >> 3);
                out.extend(word.to_le_bytes());
            }
        }
        None => out.resize(out.len() + (w * h * 2) as usize, 0),
    }
    // Colour table (revision 515 and later): sixteen gray levels.
    if version >= 515 {
        let here = checked_u32(out.len() as u64)?;
        set_u32(&mut out, slot(3), here);
        push_u32(&mut out, 0);
        push_u32(&mut out, 16);
        out.extend((0..16u8).map(|i| i * 16 + 15));
        push_u32(&mut out, 0);
    }
    // LAYERDEF, records filled as layers are written.
    let layerdef_at = out.len();
    set_u32(&mut out, slot(4), checked_u32(layerdef_at as u64)?);
    name(&mut out, "LAYERDEF");
    let records_bytes = (job.layer_count as usize)
        .checked_mul(RECORD)
        .ok_or(CodecError::Limit("Photon Workshop layer table"))?;
    push_u32(&mut out, checked_u32(records_bytes as u64 + 4)?);
    push_u32(&mut out, job.layer_count);
    let records = out.len();
    out.resize(records + records_bytes, 0);
    // EXTRA (516 and later): two-stage motion for bottom and normal layers.
    if version >= 516 {
        let here = checked_u32(out.len() as u64)?;
        set_u32(&mut out, slot(5), here);
        name(&mut out, "EXTRA");
        push_u32(&mut out, 24);
        for s in [bottom, normal] {
            push_u32(&mut out, 2);
            for value in stages(s)? {
                push_f32(&mut out, value)?;
            }
        }
    }
    // MACHINE (516 and later).
    if version >= 516 {
        let machine_at = out.len();
        set_u32(&mut out, slot(6), checked_u32(machine_at as u64)?);
        let machine_bytes = if version == 518 { 224 } else { 156 };
        name(&mut out, "MACHINE");
        push_u32(&mut out, machine_bytes);
        fixed_text(&mut out, &job.machine_name, 96)?;
        fixed_text(
            &mut out,
            if target.bitplanes() {
                "pwsimg"
            } else {
                "pw0Img"
            },
            16,
        )?;
        // Two words observed as 16 and 15 in reference files; their meaning is not established.
        push_u32(&mut out, 16);
        push_u32(&mut out, 15);
        for &size in &job.build_mm {
            push_f32(&mut out, size)?;
        }
        push_u32(&mut out, version);
        push_u32(&mut out, 0);
        if version == 518 {
            for k in 0..2 {
                push_f32(
                    &mut out,
                    job.build_mm[k] / f64::from(job.resolution[k]) * 1000.,
                )?;
            }
            for k in 0..10u32 {
                push_u32(&mut out, u32::from(k == 8));
            }
            for &pixels in &job.resolution {
                let n = u16::try_from(pixels)
                    .map_err(|_| CodecError::Limit("revision 518 resolution"))?;
                out.extend(n.to_le_bytes());
            }
        }
        out.resize(machine_at + machine_bytes as usize, 0);
    }
    // SOFTWARE (517 and later; revision 516 points the slot past MACHINE).
    if version >= 516 {
        let here = checked_u32(out.len() as u64)?;
        set_u32(&mut out, slot(1), here);
    }
    if version >= 517 {
        fixed_text(&mut out, "karasu-slicer", 32)?;
        push_u32(&mut out, 164);
        fixed_text(&mut out, env!("CARGO_PKG_VERSION"), 32)?;
        // No host OS: the bytes must not depend on the platform.
        fixed_text(&mut out, "", 64)?;
        fixed_text(&mut out, "karasu-slicer", 32)?;
    }
    // MODEL (517 and later): the bounding box, patched at the end.
    let mut model = None;
    if version >= 517 {
        model = Some(out.len() as u64);
        let here = checked_u32(out.len() as u64)?;
        set_u32(&mut out, slot(8), here);
        name(&mut out, "MODEL");
        out.resize(out.len() + 36, 0);
    }
    // SUBIMGS (518): one record per layer image.
    let mut sub_records = None;
    if version == 518 {
        let here = checked_u32(out.len() as u64)?;
        set_u32(&mut out, slot(9), here);
        name(&mut out, "SUBIMGS");
        let bytes = (job.layer_count as usize)
            .checked_mul(SUB_RECORD)
            .ok_or(CodecError::Limit("Photon Workshop sub-image table"))?;
        push_u32(&mut out, checked_u32(bytes as u64 + 24)?);
        push_u32(&mut out, job.layer_count);
        push_u32(&mut out, 1);
        sub_records = Some(out.len() as u64);
        out.resize(out.len() + bytes, 0);
    }
    // Layer images start here. Revisions 1 and 515 have no image slot: the MACHINE slot (and in
    // 515 the EXTRA slot) hold the image address, as in reference files.
    let images = checked_u32(out.len() as u64)?;
    match version {
        1 => set_u32(&mut out, slot(6), images),
        515 => {
            set_u32(&mut out, slot(5), images);
            set_u32(&mut out, slot(6), images);
        }
        _ => set_u32(&mut out, slot(7), images),
    }
    Ok(WorkshopLayout {
        bytes: out,
        header: header_at as u64,
        records: records as u64,
        sub_records,
        model,
    })
}

/// The LAYERDEF record of one layer.
pub(super) fn layer_record(
    layer: &WireLayer,
    image_at: u64,
    image_len: usize,
    thickness_mm: f64,
    lit_pixels: u64,
) -> Result<[u8; RECORD], CodecError> {
    let stages = stages(&layer.settings)?;
    let mut record = [0u8; RECORD];
    set_u32(&mut record, 0, checked_u32(image_at)?);
    set_u32(&mut record, 4, checked_u32(image_len as u64)?);
    set_f32(&mut record, 8, stages[0] + stages[3])?;
    set_f32(&mut record, 12, stages[1])?;
    set_f32(&mut record, 16, layer.settings.exposure_s)?;
    set_f32(&mut record, 20, thickness_mm)?;
    set_u32(
        &mut record,
        24,
        u32::try_from(lit_pixels).map_err(|_| CodecError::Limit("lit pixel count"))?,
    );
    Ok(record)
}

/// The SUBIMGS record of one layer image.
pub(super) fn sub_record(
    image_at: u64,
    image_len: usize,
    lit_pixels: u64,
) -> Result<[u8; SUB_RECORD], CodecError> {
    let mut record = [0u8; SUB_RECORD];
    set_u32(&mut record, 0, checked_u32(image_at)?);
    set_u32(&mut record, 4, checked_u32(image_len as u64)?);
    set_u32(
        &mut record,
        8,
        u32::try_from(lit_pixels).map_err(|_| CodecError::Limit("lit pixel count"))?,
    );
    Ok(record)
}

/// The MODEL section body after its name: the bounding box (min then max, mm) and padding.
pub(super) fn model_body(bounds: Option<[[f64; 3]; 2]>) -> Result<Vec<u8>, CodecError> {
    let mut out = Vec::with_capacity(36);
    let [lo, hi] = bounds.unwrap_or([[0.; 3]; 2]);
    for value in lo.into_iter().chain(hi) {
        push_f32(&mut out, value)?;
    }
    out.resize(36, 0);
    Ok(out)
}

/// An encoded layer image: bytes, the intensity sum of the stored levels, the lit-pixel count
/// (summed over bit-planes for `.pws`) and the pixel bounds of lit pixels.
pub(super) struct WorkshopImage {
    pub bytes: Vec<u8>,
    pub intensity: u64,
    pub lit: u64,
    pub bounds: Option<[[u32; 2]; 2]>,
}

/// Pixel bounds `[[min x, min y], [max x, max y]]` of the nonzero samples of `mask`.
fn lit_bounds(mask: &SpanMask) -> Option<[[u32; 2]; 2]> {
    let width = u64::from(mask.width());
    let mut bounds: Option<[[u32; 2]; 2]> = None;
    let mut at = 0u64;
    let _ = mask.for_each_run(|value, length| {
        if value != 0 {
            let first = at;
            let last = at + length as u64 - 1;
            let (y0, y1) = (first / width, last / width);
            let (x0, x1) = if y0 == y1 {
                (first % width, last % width)
            } else {
                (0, width - 1)
            };
            #[allow(clippy::cast_possible_truncation)] // Below the u32 dimensions.
            let span = [[x0 as u32, y0 as u32], [x1 as u32, y1 as u32]];
            bounds = Some(match bounds {
                None => span,
                Some([lo, hi]) => [
                    [lo[0].min(span[0][0]), lo[1].min(span[0][1])],
                    [hi[0].max(span[1][0]), hi[1].max(span[1][1])],
                ],
            });
        }
        at += length as u64;
        Ok::<_, ()>(())
    });
    bounds
}

/// Encode one layer: PW0 levels, or `planes` antialiasing bit-planes for `.pws`.
pub(super) fn encode_image(
    target: AnycubicWorkshop,
    planes: u8,
    quantization: Quantization,
    mask: &SpanMask,
    limit: usize,
    cancel: &mut Cancellation<'_>,
) -> Result<WorkshopImage, CodecError> {
    let bounds = lit_bounds(mask);
    if !target.bitplanes() {
        let (bytes, intensity) =
            super::pp1::encode_pw0_with_sum(mask, quantization, limit, cancel)?;
        let mut lit = 0u64;
        mask.for_each_run(|value, length| {
            // A pixel is lit when its stored level is nonzero (values 0..=8 round to level 0).
            if value > 8 {
                lit += length as u64;
            }
            Ok::<_, CodecError>(())
        })?;
        return Ok(WorkshopImage {
            bytes,
            intensity,
            lit,
            bounds,
        });
    }
    // Bit-planes: c of N planes lit stores gray c·256/N − 1 (0 when none). The stored count c
    // rounds gray·N/255 to nearest; `Exact` accepts only grays that decode exactly.
    let n = u32::from(planes.max(1));
    let decode = |c: u32| if c == 0 { 0 } else { c * 256 / n - 1 };
    let count = |value: u8| -> Result<u32, CodecError> {
        let c = (u32::from(value) * n + 127) / 255;
        if quantization == Quantization::Exact && decode(c) != u32::from(value) {
            return Err(CodecError::Unsupported(
                ".pws stores only bit-plane grays; choose RoundToFormat",
            ));
        }
        Ok(c)
    };
    let mut bytes = Vec::new();
    let (mut intensity, mut lit) = (0u64, 0u64);
    let emit = |on: bool, mut length: usize, bytes: &mut Vec<u8>| -> Result<(), CodecError> {
        while length > 0 {
            let run = length.min(127);
            if bytes.len() >= limit {
                return Err(CodecError::Limit("encoded layer byte budget"));
            }
            #[allow(clippy::cast_possible_truncation)] // At most 127.
            bytes.push(u8::from(on) << 7 | run as u8);
            length -= run;
        }
        Ok(())
    };
    for plane in 1..=n {
        let mut index = 0usize;
        // Consecutive runs in the same state merge into one.
        let mut pending: Option<(bool, usize)> = None;
        mask.for_each_run(|value, length| {
            cancel.poll(Checkpoint::ExportLayer, index)?;
            index += 1;
            let c = count(value)?;
            let on = c >= plane;
            if plane == 1 {
                intensity += u64::from(decode(c)) * length as u64;
            }
            if on {
                lit += length as u64;
            }
            match &mut pending {
                Some((state, total)) if *state == on => *total += length,
                _ => {
                    if let Some((state, total)) = pending.take() {
                        emit(state, total, &mut bytes)?;
                    }
                    pending = Some((on, length));
                }
            }
            Ok::<_, CodecError>(())
        })?;
        if let Some((state, total)) = pending {
            emit(state, total, &mut bytes)?;
        }
    }
    Ok(WorkshopImage {
        bytes,
        intensity,
        lit,
        bounds,
    })
}
