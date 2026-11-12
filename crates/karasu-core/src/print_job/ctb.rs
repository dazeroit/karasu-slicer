//! Plain CTB versions 3, 4 and 5 (TD-063): sections and per-layer records around the version-2
//! skeleton. Images use the same 7-bit RLE as version 2 with XOR seed 0 (plain), and there are no
//! checksums. Encrypted CTB is out of scope.
//!
//! Section order for versions 4 and 5:
//! 1. header (112 bytes);
//! 2. previews (two 32-byte records with RLE15 data);
//! 3. print parameters (60);
//! 4. slicer information (76);
//! 5. machine name;
//! 6. notice (320);
//! 7. version-4 block (464);
//! 8. version 5 only: resin parameters;
//! 9. layer table (36 per layer);
//! 10. each layer's `LayerDefEx` (84) followed by its image.
//!
//! Version 3 keeps the version-2 order and adds the `LayerDefEx` records.
use super::machine::{
    check_previews, checked_u32, legacy_previews, preview_sizes, set_f32, set_u32, CodecError,
    MachineFormat, WireJob, WireLayer, WireSettings,
};
use crate::preview::{PreviewImage, Previews};

/// Magic of plain CTB versions 4 and 5 (versions 2 and 3 use `0x12FD_0086`).
const MAGIC_V4: u32 = 0x12fd_0106;
/// `LayerDefEx` record size.
pub(super) const LAYER_DEF_EX: usize = 84;
/// Size of the notice section of versions 4 and 5.
const NOTICE_BYTES: usize = 320;
/// Karasu's own text for the notice section (owner decision, TD-063); NUL padded.
const NOTICE: &str =
    "Written by karasu-slicer, an independent slicer. This is a plain, unencrypted \
CTB print file; its layout was implemented from behavioural research without vendor code.";
/// Size of the version-4 block.
const V4_BLOCK_BYTES: usize = 464;
/// Observed constant at offsets 0x0C and 0x14 of the version-4 block in reference files; its
/// meaning is not established (recorded as an observation, written for compatibility).
const V4_BLOCK_OBSERVED: f64 = 4.0;

/// Offsets inside a built CTB header that are patched when the job finishes.
pub(super) struct CtbLayout {
    /// The bytes of every section before the layer table, and the zeroed table.
    pub bytes: Vec<u8>,
    /// Absolute offset of the layer table.
    pub table: u64,
    /// Absolute offset of the print parameters (volume at +20).
    pub print_parameters: u64,
    /// Absolute offset of the version-4 block (total stationary time at +0x28), if any.
    pub v4_block: Option<u64>,
}

/// The CTB version number of `format`.
pub(super) const fn version(format: MachineFormat) -> u32 {
    match format {
        MachineFormat::CtbV3Plain => 3,
        MachineFormat::CtbV4Plain => 4,
        MachineFormat::CtbV5Plain => 5,
        _ => 2,
    }
}

fn mm_per_min(speed_mm_s: f64) -> f64 {
    speed_mm_s * 60.
}

/// Karasu's version as `0xMMmmpp00`.
fn writer_version() -> u32 {
    packed_version(env!("CARGO_PKG_VERSION"))
}

/// A Semantic Versioning string as `0xMMmmpp00`: the pre-release and build suffixes (`-rc.1`,
/// `+abc`) are left out, a part that is not a number counts as 0 and each part is limited to 255.
fn packed_version(version: &str) -> u32 {
    let core = version.split(['-', '+']).next().unwrap_or("");
    let mut parts = core
        .split('.')
        .map(|p| p.parse::<u32>().unwrap_or(0).min(255));
    let (major, minor, patch) = (
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
    );
    (major << 24) | (minor << 16) | (patch << 8)
}

/// Black previews at the CTB sizes, for versions that cannot express absent previews.
fn black_previews() -> Previews {
    let [[sw, sh], [lw, lh]] = preview_sizes(MachineFormat::CtbV4Plain);
    let black = |w: u32, h: u32| {
        PreviewImage::new(w, h, vec![0; w as usize * h as usize * 3])
            .expect("CTB preview sizes are valid")
    };
    Previews {
        small: black(sw, sh),
        large: black(lw, lh),
    }
}

/// The header and every section up to the (zeroed) layer table of plain CTB v3, v4 or v5.
#[allow(clippy::too_many_lines)] // One fixed layout, field by field.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // Duration range-checked by metadata preflight.
pub(super) fn header(format: MachineFormat, job: &WireJob) -> Result<CtbLayout, CodecError> {
    let version = version(format);
    let v45 = version >= 4;
    let (normal, bottom) = (&job.normal, &job.bottom);
    let mut out = vec![0; 112];
    set_u32(&mut out, 0, if v45 { MAGIC_V4 } else { 0x12fd_0086 });
    set_u32(&mut out, 4, version);
    for (i, &size) in job.build_mm.iter().enumerate() {
        set_f32(&mut out, 8 + i * 4, size)?;
    }
    set_f32(&mut out, 32, job.layer_height_mm)?;
    set_f32(&mut out, 36, normal.exposure_s)?;
    set_f32(&mut out, 40, bottom.exposure_s)?;
    // Light-off delay: version 3 uses it; versions 4 and 5 use rest times instead (never both).
    set_f32(
        &mut out,
        44,
        if v45 {
            0.
        } else {
            normal.wait_before_exposure_s
        },
    )?;
    set_u32(&mut out, 48, job.bottom_count);
    set_u32(&mut out, 52, job.resolution[0]);
    set_u32(&mut out, 56, job.resolution[1]);
    set_u32(&mut out, 68, job.layer_count);
    set_u32(&mut out, 76, job.estimated_time_s.ceil() as u32);
    // Masks already have their physical orientation: no firmware mirror.
    set_u32(&mut out, 80, 0);
    // Antialiasing level sets: CTB stores gray per pixel, so one set.
    set_u32(&mut out, 92, 1);
    out[96..98].copy_from_slice(&u16::from(normal.pwm).to_le_bytes());
    out[98..100].copy_from_slice(&u16::from(bottom.pwm).to_le_bytes());
    // Offset 100: XOR seed 0, plain images.
    if v45 {
        // Versions 4 and 5 cannot express absent previews: black ones stand in.
        let previews = job.previews.clone().unwrap_or_else(black_previews);
        check_previews(&previews, preview_sizes(format))?;
        legacy_previews(&mut out, &previews)?;
    }
    let print_parameters = out.len();
    let mut p = vec![0; 60];
    // Versions 4 and 5 store total lift travel (both stages); version 3 has one stage.
    for (at, n) in [
        (0, bottom.lift[0][0] + bottom.lift[1][0]),
        (4, mm_per_min(bottom.lift[0][1])),
        (8, normal.lift[0][0] + normal.lift[1][0]),
        (12, mm_per_min(normal.lift[0][1])),
        (16, mm_per_min(normal.retract[0][1])),
        (20, job.volume_ml),
        (
            32,
            if v45 {
                0.
            } else {
                bottom.wait_before_exposure_s
            },
        ),
        (
            36,
            if v45 {
                0.
            } else {
                normal.wait_before_exposure_s
            },
        ),
    ] {
        set_f32(&mut p, at, n)?;
    }
    set_u32(&mut p, 40, job.bottom_count);
    out.extend(p);
    let slicer_at = out.len();
    let mut s = vec![0; 76];
    if v45 {
        for (at, n) in [
            (0x00, bottom.lift[1][0]),
            (0x04, mm_per_min(bottom.lift[1][1])),
            (0x08, normal.lift[1][0]),
            (0x0c, mm_per_min(normal.lift[1][1])),
            (0x10, normal.retract[1][0]),
            (0x14, mm_per_min(normal.retract[1][1])),
            (0x18, normal.wait_after_lift_s),
            (0x34, normal.wait_before_exposure_s),
            (0x38, normal.wait_after_lift_s),
        ] {
            set_f32(&mut s, at, n)?;
        }
    }
    if !job.machine_name.is_ascii() || job.machine_name.len() > 4096 {
        return Err(CodecError::Invalid(
            "CTB machine name must be ASCII and bounded",
        ));
    }
    let name_at = slicer_at + 76;
    set_u32(&mut s, 0x1c, checked_u32(name_at as u64)?);
    set_u32(
        &mut s,
        0x20,
        u32::try_from(job.machine_name.len()).map_err(|_| CodecError::Limit("machine name"))?,
    );
    // Antialiasing flag: 0x0F for supersampled gray masks, 0x07 for binary ones.
    s[0x24] = if job.antialiasing > 1 { 0x0f } else { 0x07 };
    // Per-layer-settings byte observed in reference files: 0x20 (v3), 0x50 (v5); 0x40 for v4 is
    // inferred from the same pattern.
    s[0x27] = match version {
        3 => 0x20,
        4 => 0x40,
        _ => 0x50,
    };
    set_u32(&mut s, 0x2c, u32::from(job.antialiasing));
    set_u32(&mut s, 0x30, writer_version());
    set_u32(&mut s, 0x3c, job.transition_count);
    out.extend(s);
    out.extend(job.machine_name.as_bytes());
    let mut v4_block = None;
    if v45 {
        let notice_at = out.len();
        let mut notice = NOTICE.as_bytes().to_vec();
        notice.resize(NOTICE_BYTES, 0);
        out.extend(notice);
        let block_at = out.len();
        set_u32(&mut out, slicer_at + 0x40, checked_u32(block_at as u64)?);
        let mut b = vec![0; V4_BLOCK_BYTES];
        for (at, n) in [
            (0x00, mm_per_min(bottom.retract[0][1])),
            (0x04, mm_per_min(bottom.retract[1][1])),
            (0x0c, V4_BLOCK_OBSERVED),
            (0x14, V4_BLOCK_OBSERVED),
            (0x18, normal.wait_before_exposure_s),
            (0x1c, normal.wait_after_lift_s),
            (0x20, normal.wait_after_exposure_s),
            (0x24, bottom.retract[1][0]),
        ] {
            set_f32(&mut b, at, n)?;
        }
        set_u32(&mut b, 0x34, job.layer_count);
        set_u32(&mut b, 0x48, checked_u32(notice_at as u64)?);
        set_u32(&mut b, 0x4c, NOTICE_BYTES as u32);
        out.extend(b);
        v4_block = Some(block_at as u64);
        if version == 5 {
            let resin_at = out.len();
            set_u32(&mut out, block_at + 0x50, checked_u32(resin_at as u64)?);
            out.extend(resin_parameters(job, resin_at)?);
        }
    } else if let Some(previews) = &job.previews {
        check_previews(previews, preview_sizes(format))?;
        legacy_previews(&mut out, previews)?;
    }
    set_u32(&mut out, 84, checked_u32(print_parameters as u64)?);
    set_u32(&mut out, 88, 60);
    set_u32(&mut out, 104, checked_u32(slicer_at as u64)?);
    set_u32(&mut out, 108, 76);
    let table = out.len();
    set_u32(&mut out, 64, checked_u32(table as u64)?);
    Ok(CtbLayout {
        bytes: out,
        table: table as u64,
        print_parameters: print_parameters as u64,
        v4_block,
    })
}

/// Version-5 resin parameters: 40 bytes, then three strings end to end (resin, resin, machine,
/// as observed in reference files; their exact meanings are not established).
fn resin_parameters(job: &WireJob, at: usize) -> Result<Vec<u8>, CodecError> {
    if job.resin_name.as_bytes().contains(&0) || job.resin_name.len() > 4096 {
        return Err(CodecError::Invalid(
            "CTB resin name must be bounded and NUL-free",
        ));
    }
    let strings = [
        job.resin_name.as_bytes(),
        job.resin_name.as_bytes(),
        job.machine_name.as_bytes(),
    ];
    let mut out = vec![0; 40];
    let mut next = at + 40;
    for (i, text) in strings.iter().enumerate() {
        set_u32(&mut out, 8 + 8 * i, checked_u32(next as u64)?);
        set_u32(
            &mut out,
            12 + 8 * i,
            u32::try_from(text.len()).map_err(|_| CodecError::Limit("resin text"))?,
        );
        next += text.len();
    }
    set_f32(&mut out, 0x20, job.resin_density_g_per_ml)?;
    for text in strings {
        out.extend_from_slice(text);
    }
    Ok(out)
}

/// The 36-byte table row and the 84-byte `LayerDefEx` of one layer whose record starts at
/// `record` (the image follows the record).
pub(super) fn layer_records(
    format: MachineFormat,
    layer: &WireLayer,
    record: u64,
    image_len: usize,
) -> Result<([u8; 36], [u8; LAYER_DEF_EX]), CodecError> {
    let v45 = version(format) >= 4;
    let s: &WireSettings = &layer.settings;
    let image_at = record + LAYER_DEF_EX as u64;
    let mut row = [0u8; 36];
    set_f32(&mut row, 0, layer.z_mm)?;
    set_f32(&mut row, 4, s.exposure_s)?;
    set_f32(&mut row, 8, if v45 { 0. } else { s.wait_before_exposure_s })?;
    // A 64-bit address split into a low word and a page; the writer stays below 4 GiB.
    set_u32(&mut row, 12, checked_u32(image_at)?);
    set_u32(&mut row, 16, checked_u32(image_len as u64)?);
    set_u32(&mut row, 20, 0);
    set_u32(&mut row, 24, checked_u32(LAYER_DEF_EX as u64)?);
    let mut ex = [0u8; LAYER_DEF_EX];
    ex[..36].copy_from_slice(&row);
    set_u32(
        &mut ex,
        0x24,
        checked_u32(LAYER_DEF_EX as u64 + image_len as u64)?,
    );
    for (at, n) in [
        (0x28, s.lift[0][0] + s.lift[1][0]),
        (0x2c, mm_per_min(s.lift[0][1])),
        (0x30, s.lift[1][0]),
        (0x34, mm_per_min(s.lift[1][1])),
        (0x38, mm_per_min(s.retract[0][1])),
        (0x3c, s.retract[1][0]),
        (0x40, mm_per_min(s.retract[1][1])),
        (0x44, if v45 { s.wait_after_exposure_s } else { 0. }),
        (0x48, if v45 { s.wait_after_lift_s } else { 0. }),
        (0x4c, if v45 { s.wait_before_exposure_s } else { 0. }),
        (0x50, f64::from(s.pwm)),
    ] {
        set_f32(&mut ex, at, n)?;
    }
    Ok((row, ex))
}

/// The stationary time one layer contributes: exposure and waits, in seconds.
pub(super) fn stationary_s(s: &WireSettings) -> f64 {
    s.exposure_s + s.wait_before_exposure_s + s.wait_after_exposure_s + s.wait_after_lift_s
}

#[cfg(test)]
mod tests {
    use super::packed_version;

    #[test]
    fn the_writer_version_packs_only_the_numeric_version() {
        assert_eq!(packed_version("1.2.3"), 0x0102_0300);
        assert_eq!(packed_version("1.2.3-rc.1"), 0x0102_0300);
        assert_eq!(packed_version("2.0.0-dev"), 0x0200_0000);
        assert_eq!(packed_version("0.1.0+build.7"), 0x0001_0000);
        assert_eq!(packed_version("300.0.1"), 0xff00_0100);
        assert_eq!(packed_version("1.x"), 0x0100_0000);
    }
}
