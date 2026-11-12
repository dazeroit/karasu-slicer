//! The text summary of a completed job: one `Key: value` line per fact, in a fixed order, so
//! callers and tests can read single facts. Numbers use Rust's shortest round-trip formatting.
use super::{
    describe_defects, describe_repairs, Diagnosis, ImportFacts, Prepared, PreparedHoles,
    PreparedObject, QuotedPath, Totals, REFERENCE_FORMAT,
};
use crate::request::{Fill, ModelSource, Pose, SliceRequest, Subject};
use karasu_core::{
    export::{BuiltinExporter, MachineExporter},
    mesh::formats::UnitOrigin,
};
use std::fmt::{self, Write};

/// The model line, and the import notes when there are any.
pub(super) fn write_source(out: &mut impl Write, source: &ImportFacts) -> fmt::Result {
    writeln!(
        out,
        "Model: {} ({} {}, {} objects)",
        source.format.to_ascii_uppercase(),
        source.unit,
        match source.origin {
            UnitOrigin::Declared => "declared",
            UnitOrigin::Assumed => "assumed",
        },
        source.objects
    )?;
    if !source.notes.is_empty() {
        let notes: Vec<String> = source.notes.iter().map(ToString::to_string).collect();
        writeln!(out, "Import notes: {}", notes.join("; "))?;
    }
    Ok(())
}

/// The translation line, and the transform line unless the pose is the identity.
pub(super) fn write_placement(
    out: &mut impl Write,
    translation_mm: [f64; 3],
    pose: &Pose,
) -> fmt::Result {
    let [tx, ty, tz] = translation_mm;
    writeln!(out, "Translation (mm): {tx} {ty} {tz}")?;
    write_transform(out, pose)
}

/// The transform line, unless the pose is the identity.
pub(super) fn write_transform(out: &mut impl Write, pose: &Pose) -> fmt::Result {
    if !pose.is_rigid_identity() {
        let [rx, ry, rz] = pose.rotate_deg;
        let [sx, sy, sz] = pose.scale;
        let mirrored: String = ["x", "y", "z"]
            .iter()
            .zip(pose.mirror)
            .filter_map(|(axis, on)| on.then_some(*axis))
            .collect();
        writeln!(
            out,
            "Transform: rotate {rx} {ry} {rz} degrees, scale {sx} {sy} {sz}, mirror {}",
            if mirrored.is_empty() {
                "none"
            } else {
                &mirrored
            }
        )?;
    }
    Ok(())
}

/// The layer, fill, antialiasing and compensation lines.
pub(super) fn write_settings(
    out: &mut impl Write,
    args: &SliceRequest,
    totals: &Totals<'_>,
) -> fmt::Result {
    writeln!(out, "Layers: {}", totals.export.layers)?;
    if let Some(([thinnest, thickest], explicit)) = totals.layer_heights {
        writeln!(
            out,
            "Layer heights (mm): {thinnest} to {thickest}, {explicit} layers with explicit settings"
        )?;
    }
    writeln!(out, "Fill: {}", args.fill.id())?;
    writeln!(
        out,
        "Antialiasing (samples per pixel): {}",
        u16::from(args.antialias).pow(2)
    )?;
    if args.min_gray > 0 {
        writeln!(out, "Minimum edge gray (0-255): {}", args.min_gray)?;
    }
    let [px, py] = totals.pixel_mm;
    let describe = |pixels: i16| {
        let mm = |pitch: f64| f64::from(pixels) * pitch;
        format!("{pixels} px ({} mm in X, {} mm in Y)", mm(px), mm(py))
    };
    if args.xy_compensation_px != 0 || args.bottom_xy_compensation_px.is_some() {
        writeln!(
            out,
            "XY compensation: {}",
            describe(args.xy_compensation_px)
        )?;
    }
    if let Some(pixels) = args.bottom_xy_compensation_px {
        writeln!(
            out,
            "Bottom XY compensation: {} on the first {} layers",
            describe(pixels),
            totals.bottom_layers
        )?;
    }
    Ok(())
}

pub(super) fn write(
    out: &mut impl Write,
    args: &SliceRequest,
    machine: Option<&BuiltinExporter>,
    totals: &Totals<'_>,
    object: &PreparedObject,
) -> fmt::Result {
    writeln!(out, "{} slice", karasu_core::APPLICATION_NAME)?;
    write_subject(out, &args.subject)?;
    write_source(out, &object.diagnosis.source)?;
    write_output(out, args, machine)?;
    write_placement(out, object.placement.translation().components(), &args.pose)?;
    write_settings(out, args, totals)?;
    write_diagnosis(out, &object.diagnosis)?;
    write_totals(out, args, totals)?;
    if let Some(supported) = &object.supported {
        for line in &supported.lines {
            writeln!(out, "{line}")?;
        }
    }
    write_preparation(out, &object.prepared, object.holes.as_ref())
}

/// The format and output lines.
pub(super) fn write_output(
    out: &mut impl Write,
    args: &SliceRequest,
    machine: Option<&BuiltinExporter>,
) -> fmt::Result {
    writeln!(
        out,
        "Format: {}",
        machine.map_or(REFERENCE_FORMAT, |m| m.id())
    )?;
    writeln!(out, "Output path: {}", QuotedPath(&args.output))
}

/// The defects found, the repairs applied and the defects that remain.
pub(super) fn write_diagnosis(out: &mut impl Write, diagnosis: &Diagnosis) -> fmt::Result {
    writeln!(out, "Mesh defects: {}", describe_defects(&diagnosis.found))?;
    writeln!(out, "Repairs: {}", describe_repairs(&diagnosis.repairs))?;
    if let Some(remaining) = &diagnosis.remaining {
        writeln!(out, "Remaining defects: {}", describe_defects(remaining))?;
    }
    Ok(())
}

/// The union facts, output size, volume, time and resin lines.
pub(super) fn write_totals(
    out: &mut impl Write,
    args: &SliceRequest,
    totals: &Totals<'_>,
) -> fmt::Result {
    if args.fill == Fill::Union {
        writeln!(
            out,
            "Layers with merged overlapping material: {}",
            totals.facts.merged
        )?;
        match (args.close_holes_mm, totals.facts.longest) {
            (None, _) => writeln!(out, "Closed sections: disabled")?,
            (Some(_), None) => writeln!(out, "Closed sections: none")?,
            (Some(bound), Some((longest, layer))) => writeln!(
                out,
                "Closed sections: {} layers, {} chords, longest {longest} mm at layer {layer} \
                 (bound {bound} mm)",
                totals.facts.closed_layers, totals.facts.chords
            )?,
        }
        // Reported only when tolerated, so other summaries are unchanged (TD-079).
        if let Some(bound) = args.reversed_tolerance_mm2 {
            let facts = &totals.facts;
            #[allow(clippy::cast_precision_loss)] // Sample counts far below 2^52.
            let area = |samples: u64| samples as f64 * facts.sample_mm2;
            match facts.largest_reversed {
                None => writeln!(
                    out,
                    "Reversed slivers left empty: none (bound {bound} mm^2 per layer)"
                )?,
                Some((largest, layer)) => writeln!(
                    out,
                    "Reversed slivers left empty: {} layers, {} mm^2 in all, largest {} mm^2 at \
                     layer {layer} (bound {bound} mm^2 per layer)",
                    facts.reversed_layers,
                    area(facts.reversed_samples),
                    area(largest)
                )?,
            }
        }
    }
    writeln!(out, "Output bytes: {}", totals.export.output_bytes)?;
    writeln!(
        out,
        "Sampled mask volume (mm^3): {}",
        totals.export.coverage_volume_mm3
    )?;
    writeln!(
        out,
        "Estimated print time (s): {}",
        totals.export.estimated_time_s
    )?;
    let usage = &totals.resin;
    writeln!(out, "Resin volume (ml): {}", usage.volume_ml)?;
    if let Some(mass) = usage.mass_g {
        writeln!(out, "Resin mass (g): {mass}")?;
    }
    if let Some((cost, currency)) = &usage.cost {
        writeln!(out, "Resin cost ({currency}): {cost}")?;
    }
    Ok(())
}

/// The hollow wall lines (TD-093): requested, guaranteed, offset, passes, cell and enclosure.
fn write_hollow_wall(
    out: &mut impl Write,
    c: &karasu_core::preparation::sampled::SampledHollowCertificate,
) -> fmt::Result {
    let [lo, hi] = c.surface_distance_mm;
    let [_, vhi] = c.vertex_distance_mm;
    writeln!(out, "Hollow wall thickness (mm): {}", c.thickness_mm)?;
    // TD-093: the wall proven per cavity face, at or above the requested one.
    writeln!(out, "Hollow wall guaranteed (mm): {lo}")?;
    writeln!(out, "Hollow offset (mm): {}", c.offset_mm)?;
    writeln!(out, "Hollowing passes: {}", c.passes)?;
    writeln!(out, "Hollow grid cell (mm): {}", c.cell_mm)?;
    writeln!(out, "Certified wall range (mm): {lo} {hi}")?;
    // TD-089: a hollow placed with a scale has its wall scaled with it.
    #[allow(clippy::float_cmp)] // Exactly 1 when no scale was applied.
    if c.placement_scale != [1.0, 1.0] {
        let [low, high] = c.placement_scale;
        writeln!(out, "Hollow placement scale: {low} {high}")?;
    }
    writeln!(out, "Thickest wall at cavity vertices (mm): {vhi}")?;
    Ok(())
}

/// The hollowing certificate and drainage lines.
pub(super) fn write_preparation(
    out: &mut impl Write,
    prepared: &Prepared,
    holes: Option<&PreparedHoles>,
) -> fmt::Result {
    if let Some(model) = prepared.hollow() {
        let c = model.certificate();
        write_hollow_wall(out, &c)?;
        writeln!(out, "Cavity volume (mm^3): {}", c.cavity_volume_mm3)?;
        if let Some(s) = c.simplification {
            let [before, after] = s.triangles;
            writeln!(
                out,
                "Cavity simplification: tolerance (mm) {}, certified deviation (mm) {}, \
                 triangles {before} -> {after}, volume before (mm^3) {}, wall floor (mm) {}",
                s.tolerance_mm, s.max_deviation_mm, s.cavity_volume_before_mm3, s.wall_floor_mm
            )?;
        }
        writeln!(out, "Cavity surfaces: {}", c.cavity_surface_components)?;
        // TD-092 r3: printed only when they apply, so earlier summaries are unchanged.
        let left_out = model.measure_zero();
        if left_out.any() {
            writeln!(
                out,
                "Hollowing left out as zero measure: {} zero-area faces, {} zero-area cracks \
                 closed by splitting",
                left_out.zero_area_faces, left_out.cracks_closed
            )?;
        }
        let overlap = model.overlap();
        if overlap.surfaces > 0 {
            writeln!(
                out,
                "Cavities inside overlapping bodies: {} (in up to {} bodies; each emptied in every \
                 body)",
                overlap.surfaces, overlap.deepest
            )?;
        }
    }
    let hollow = prepared.hollow().is_some();
    let Some(holes) = holes else {
        if hollow {
            writeln!(
                out,
                "Drainage: none; the cavity is enclosed and will trap uncured resin"
            )?;
        }
        return Ok(());
    };
    for (index, (spec, found)) in (1..).zip(holes.specs().iter().zip(holes.findings())) {
        let mut facts = vec![if found.opens_outside {
            "opens to the outside".to_owned()
        } else {
            "does not open to the outside".to_owned()
        }];
        if found.through {
            facts.push("passes through the object".to_owned());
        }
        if hollow {
            facts.push(if found.regions.is_empty() {
                "reaches no cavity".to_owned()
            } else {
                let regions: Vec<String> =
                    found.regions.iter().map(|r| (r + 1).to_string()).collect();
                format!("reaches cavity region {}", regions.join(", "))
            });
        }
        if found.ends_in_material {
            facts.push("ends in material".to_owned());
        }
        writeln!(out, "Drain hole {index}: {}", facts.join(", "))?;
        let [x, y, z] = spec.entry_mm;
        let [dx, dy, dz] = spec.unit_direction().unwrap_or(spec.direction);
        writeln!(
            out,
            "Drain hole {index} axis: entry (mm) {x} {y} {z}, direction {dx} {dy} {dz}, \
             length (mm) {}",
            spec.length_mm
        )?;
    }
    if hollow {
        writeln!(out, "Cavity regions: {}", holes.regions().len())?;
        let enclosed = holes.enclosed();
        if enclosed == 0 {
            writeln!(
                out,
                "Drainage: every cavity region is opened to the outside by a drain hole (geometric; resin flow not verified)"
            )?;
        } else {
            writeln!(
                out,
                "Drainage: {enclosed} cavity region(s) have no drain hole opening them to the outside and may trap uncured resin"
            )?;
        }
    }
    Ok(())
}

/// What the job printed: the input model, or the plate.
pub(super) fn write_subject(out: &mut impl Write, subject: &Subject) -> fmt::Result {
    match subject {
        Subject::Model(model) => write_model(out, "Input", model),
        Subject::PlateFile(path) => writeln!(out, "Plate path: {}", QuotedPath(path)),
        Subject::Plate(plate) => writeln!(out, "Plate: {:?}", plate.name),
    }
}

/// `<role> path` for a model file, `<role> name` for an embedded model.
pub(super) fn write_model(out: &mut impl Write, role: &str, model: &ModelSource) -> fmt::Result {
    match model {
        ModelSource::File(path) => writeln!(out, "{role} path: {}", QuotedPath(path)),
        ModelSource::Embedded { name, .. } => writeln!(out, "{role} name: {name:?}"),
    }
}
