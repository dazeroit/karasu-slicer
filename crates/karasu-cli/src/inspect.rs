//! Read-only presentation of complete public-core model import and topology results.

use karasu_core::cancellation::Cancellation;
use karasu_core::mesh::formats::{
    FormatRegistry, ImportedModel, LengthUnit, ModelImportError, ModelImportOptions, UnitOrigin,
};
use karasu_core::mesh::repair::RepairChange;
use karasu_core::mesh::stl::{import_stl_path, ImportedStl, StlHeader, StlOptions, StlUnitPolicy};
use karasu_core::mesh::topology::{
    CheckStatus, DefectClass, TopologyIssue, TopologyLimits, TopologyReport, VertexDefect,
    MAX_SAMPLES, SMALL_COMPONENT_MM2,
};
use karasu_core::mesh::TriangleMesh;
use karasu_core::mesh::{Degeneracy, MeshDiagnostic};
use karasu_core::preparation::repair::{
    repair_source, self_intersections, IntersectionReport, RepairPreset, SourceRepairReport,
};
use karasu_project::diagnosis::{diagnose, Diagnosis, Printability};
use std::{
    fmt,
    io::{self, Write},
    path::Path,
};

const DETAIL_LIMIT: usize = 100;

struct QuotedPath<'a>(&'a Path);

impl fmt::Display for QuotedPath<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.0.as_os_str(), f)
    }
}

/// Validated `inspect` arguments.
#[derive(Debug, PartialEq)]
pub(super) struct InspectArgs {
    pub(super) path: std::path::PathBuf,
    /// Unit assumed for formats that declare none.
    pub(super) units: Option<LengthUnit>,
    /// Given with `--assume-mm`: unknown extensions are read as STL.
    pub(super) legacy_stl: bool,
    /// Given with `--diagnosis`: append the extended diagnosis (M6.4d r1, TD-092).
    pub(super) diagnosis: bool,
    /// Given with `--minima`: report the local minima instead, the model turned by these
    /// angles in degrees (TD-107).
    pub(super) minima: Option<[f64; 3]>,
}

/// Self-intersecting face pairs searched for at most; the report says when the bound stopped it.
const MAX_INTERSECTION_PAIRS: usize = 100_000;

/// Inspect a model: the detailed STL report for STL in millimetres, otherwise the model report.
pub(super) fn inspect_model(args: &InspectArgs, output: &mut impl Write) -> Result<(), String> {
    if let Some(rotate) = args.minima {
        return minima_report(args, rotate, output);
    }
    let registry = FormatRegistry::builtin();
    let format = match registry.for_path(&args.path) {
        Ok(format) => Some(format),
        Err(ModelImportError::UnknownFormat { .. }) if args.legacy_stl => None,
        Err(e) => return Err(format!("cannot inspect {}: {e}", QuotedPath(&args.path))),
    };
    let stl = format.is_none_or(|f| f.id() == "stl");
    if stl && args.units == Some(LengthUnit::Millimeter) {
        return inspect(&args.path, args.diagnosis, output);
    }
    let format = format.unwrap_or(&karasu_core::mesh::formats::StlFormat);
    let imported = registry
        .import_path(
            &args.path,
            &ModelImportOptions::new(args.units),
            &mut Cancellation::never(),
        )
        .or_else(|e| match e {
            // Legacy STL paths without the extension go straight to the selected format.
            ModelImportError::UnknownFormat { .. } => {
                let bytes = karasu_core::mesh::formats::read_bounded(
                    &args.path,
                    ModelImportOptions::new(args.units)
                        .limits
                        .input
                        .max_input_bytes,
                    &mut Cancellation::never(),
                )?;
                registry.import_bytes(
                    format,
                    &bytes,
                    &ModelImportOptions::new(args.units),
                    &mut Cancellation::never(),
                )
            }
            e => Err(e),
        })
        .map_err(|cause| {
            let hint = if matches!(cause, ModelImportError::UnitsRequired { .. }) {
                "; give it with --units (for example --units mm)"
            } else {
                ""
            };
            format!(
                "cannot import {} {}: {cause}{hint}",
                format.name(),
                QuotedPath(&args.path)
            )
        })?;
    let topology = imported.mesh.analyze_topology().map_err(|cause| {
        format!(
            "cannot analyze topology for {}: {cause}",
            QuotedPath(&args.path)
        )
    })?;
    let extended = args
        .diagnosis
        .then(|| extended(&args.path, &imported.mesh))
        .transpose()?;
    write_model_report(output, &args.path, format.name(), &imported, &topology)
        .and_then(|()| extended.map_or(Ok(()), |e| write_extended(output, &e)))
        .and_then(|()| output.flush())
        .map_err(|cause| format!("cannot write inspection output: {cause}"))
}

/// The extended diagnosis, the exact repair's preview and the states before and after it.
struct Extended {
    found: Diagnosis,
    intersections: IntersectionReport,
    /// The exact and standard presets' previews.
    previews: Vec<Preview>,
}

/// What a repair preset would change, and the diagnosis after it when it changed the model.
struct Preview {
    name: &'static str,
    report: SourceRepairReport,
    after: Option<Diagnosis>,
}

/// Compute the extended diagnosis in full before anything is written, so a failure leaves no
/// partial inspection on stdout.
fn extended(path: &Path, mesh: &TriangleMesh) -> Result<Extended, String> {
    let failed = |what: &str, cause: &dyn fmt::Display| {
        format!("cannot {what} for {}: {cause}", QuotedPath(path))
    };
    let found =
        diagnose(mesh, &mut Cancellation::never()).map_err(|e| failed("diagnose the model", &e))?;
    let intersections =
        self_intersections(mesh, MAX_INTERSECTION_PAIRS, &mut Cancellation::never())
            .map_err(|e| failed("search self-intersections", &e))?;
    let mut previews = Vec::new();
    for (name, preset) in [
        ("exact", RepairPreset::Exact),
        ("standard", RepairPreset::Standard),
        ("aggressive", RepairPreset::Aggressive),
    ] {
        let (repaired, report) = repair_source(
            mesh,
            preset.spec(),
            TopologyLimits::default(),
            &mut Cancellation::never(),
        )
        .map_err(|e| failed("repair the model", &e))?;
        let after = repaired
            .as_ref()
            .map(|m| diagnose(m, &mut Cancellation::never()))
            .transpose()
            .map_err(|e| failed("diagnose the repaired model", &e))?;
        previews.push(Preview {
            name,
            report,
            after,
        });
    }
    Ok(Extended {
        found,
        intersections,
        previews,
    })
}

fn printability(state: Printability) -> &'static str {
    match state {
        Printability::Ready => "ready",
        Printability::ReadyWithNotes => "ready with notes (slicing handles what remains)",
        Printability::NeedsRepairForHollowing => {
            "slices; hollowing needs repair (holes other than zero-area cracks)"
        }
        Printability::NeedsRepair => "needs repair (slicing would fail or could be wrong)",
    }
}

fn defect_class(class: DefectClass) -> &'static str {
    match class {
        DefectClass::Rim => "hole rim",
        DefectClass::Crack => "crack",
        DefectClass::NonManifoldEdge => "non-manifold edge",
        DefectClass::WindingConflict => "winding conflict",
        DefectClass::Needle => "needle face",
        DefectClass::Cap => "cap face",
        DefectClass::Duplicate => "duplicate face",
        DefectClass::OppositePair => "opposite face pair",
        DefectClass::NonManifoldVertex => "pinch vertex",
        DefectClass::SmallComponent => "small component",
        DefectClass::NonOrientable => "non-orientable component",
    }
}

fn repair_change(change: RepairChange) -> &'static str {
    match change {
        RepairChange::Duplicates => "duplicates collapsed",
        RepairChange::Reoriented => "faces flipped",
        RepairChange::TurnedOutward => "body turned outward",
        RepairChange::AmbiguousVolume => "flat body left unchanged",
        RepairChange::HoleFilled => "hole filled",
        RepairChange::CrackClosed => "crack closed",
        RepairChange::HoleRefused => "hole left open",
        RepairChange::Welded => "vertices welded",
        RepairChange::NeedleRemoved => "needle face removed",
        RepairChange::EdgeCollapsed => "short edge collapsed",
        RepairChange::CapFlipped => "flat face flipped",
        RepairChange::FinRemoved => "fin removed",
        RepairChange::FacesCut => "faces cut from a non-manifold edge",
        RepairChange::SmallShellRemoved => "small shell removed",
        RepairChange::InnerShellRemoved => "inner shell removed",
        RepairChange::VisibilityFlipped => "open part turned by visibility",
        RepairChange::RebuildDeviation => "largest deviation from the rebuilt solid",
    }
}

fn write_extended(out: &mut impl Write, extended: &Extended) -> io::Result<()> {
    let Extended {
        found,
        intersections,
        previews,
    } = extended;
    let defects = &found.defects;
    writeln!(out, "\nExtended diagnosis: complete")?;
    writeln!(
        out,
        "Needle faces (two corners at one position): {}",
        defects.needles
    )?;
    writeln!(out, "Cap faces (three corners on a line): {}", defects.caps)?;
    writeln!(out, "Cracks (hole rims on a line): {}", defects.cracks)?;
    writeln!(out, "Pinch vertices: {}", defects.non_manifold_vertices)?;
    writeln!(
        out,
        "Small components (under {SMALL_COMPONENT_MM2} mm^2): {}",
        defects.small_components
    )?;
    writeln!(
        out,
        "Smallest component area (mm^2): {}",
        defects.smallest_component_mm2
    )?;
    writeln!(
        out,
        "Self-intersecting face pairs: {}{}",
        intersections.pairs,
        if intersections.complete {
            String::new()
        } else {
            format!(" (search stopped at {MAX_INTERSECTION_PAIRS})")
        }
    )?;
    writeln!(
        out,
        "Defect locations shown: {} (at most {MAX_SAMPLES} per class)",
        defects.samples.len() + intersections.samples.len()
    )?;
    for sample in &defects.samples {
        let [x, y, z] = sample.point;
        writeln!(out, "  {} at {x} {y} {z}", defect_class(sample.class))?;
    }
    for &[x, y, z] in &intersections.samples {
        writeln!(out, "  self-intersection at {x} {y} {z}")?;
    }
    writeln!(
        out,
        "Printability as is (union, holes closed in layers): {}",
        printability(found.printability(true))
    )?;
    if intersections.pairs > 0 {
        writeln!(
            out,
            "Note: self-intersections can leave reversed slivers that layers refuse; \
             --reversed-tolerance leaves small ones empty"
        )?;
    }
    for preview in previews {
        write_preview(out, preview, found)?;
    }
    Ok(())
}

/// One preset's preview: what it would change, where, and the state after it.
fn write_preview(out: &mut impl Write, preview: &Preview, found: &Diagnosis) -> io::Result<()> {
    let Preview {
        name,
        report,
        after,
    } = preview;
    let mut parts = Vec::new();
    if let Some(w) = &report.weld {
        parts.push(format!(
            "weld {} vertices (largest move {} mm; {} faces collapse)",
            w.vertices_merged, w.max_displacement_mm, w.faces_collapsed
        ));
    }
    if let Some(d) = &report.degenerate {
        parts.push(format!(
            "remove {} needle faces, collapse {} short edges (largest move {} mm), flip {} flat \
             faces (largest apex distance {} mm), leave {} near-degenerate faces",
            d.needles_removed,
            d.edges_collapsed,
            d.max_displacement_mm,
            d.caps_flipped,
            d.max_cap_height_mm,
            d.left
        ));
    }
    let mesh = &report.mesh;
    let fins = report.non_manifold.as_ref().map(|n| {
        format!(
            "remove {} fins ({} faces, {} mm^2), cut {} faces ({} mm^2), leave {} non-manifold \
             edges",
            n.fins_removed, n.fin_faces, n.fin_area_mm2, n.faces_cut, n.cut_area_mm2, n.edges_left
        )
    });
    parts.push(format!(
        "collapse {} duplicate faces, flip {} faces, turn {} bodies outward",
        mesh.dropped_duplicate_faces, mesh.flipped_faces, mesh.outward_flipped_components
    ));
    parts.extend(fins);
    if let Some(v) = &report.visibility {
        parts.push(format!(
            "turn {} of {} open parts by visibility ({} undecided)",
            v.parts_flipped, v.open_parts, v.undecided
        ));
    }
    if let Some(fill) = &report.fill {
        parts.push(format!(
            "fill {} holes (largest cap {} mm^2; {} vertices added, {} rims split at a pinch, {} \
             refined caps replaced by unrefined ones where they met the mesh), \
             close {} cracks (largest offset {} mm), leave {} holes open",
            fill.holes_filled,
            fill.largest_cap_mm2,
            fill.vertices_added,
            fill.rims_split,
            fill.unrefined_caps,
            fill.cracks_closed,
            fill.largest_crack_offset_mm,
            fill.holes_refused
        ));
    }
    if let Some(s) = &report.shells {
        parts.push(format!(
            "remove {} small shells ({} mm^2) and {} inner shells ({} mm^3)",
            s.small_removed, s.small_area_mm2, s.inner_removed, s.inner_volume_mm3
        ));
    }
    let title = format!("{}{}", name[..1].to_uppercase(), &name[1..]);
    writeln!(out, "{title} repair would: {}", parts.join(", "))?;
    let changes: Vec<_> = report.all_samples().collect();
    writeln!(out, "{title} repair locations shown: {}", changes.len())?;
    for sample in changes {
        let [x, y, z] = sample.point;
        writeln!(out, "  {} at {x} {y} {z}", repair_change(sample.change))?;
    }
    writeln!(
        out,
        "Printability after {name} repair: {}",
        printability(after.as_ref().unwrap_or(found).printability(true))
    )
}

fn write_model_report(
    out: &mut impl Write,
    path: &Path,
    format: &str,
    imported: &ImportedModel,
    topology: &TopologyReport,
) -> io::Result<()> {
    writeln!(out, "{} model inspection", karasu_core::APPLICATION_NAME)?;
    writeln!(out, "Input path: {}", QuotedPath(path))?;
    writeln!(out, "Model format: {format}")?;
    writeln!(
        out,
        "Units: {} ({}; coordinates below are in millimeters)",
        imported.unit,
        match imported.unit_origin {
            UnitOrigin::Declared => "declared by the file",
            UnitOrigin::Assumed => "assumed",
        }
    )?;
    writeln!(out, "Objects placed: {}", imported.objects)?;
    if imported.notes.is_empty() {
        writeln!(out, "Import notes: none")?;
    } else {
        for note in &imported.notes {
            writeln!(out, "Import note: {note}")?;
        }
    }
    writeln!(
        out,
        "Coordinate space: source (no placement, scene transform, or repair)"
    )?;
    write_geometry(out, &imported.mesh, &imported.diagnostics, topology)
}

/// The detailed STL report, with the extended diagnosis when `diagnosis` is set.
pub(super) fn inspect(path: &Path, diagnosis: bool, output: &mut impl Write) -> Result<(), String> {
    let imported = import_stl_path(path, StlOptions::new(StlUnitPolicy::AssumeMillimeters))
        .map_err(|cause| format!("cannot import STL {}: {cause}", QuotedPath(path)))?;
    let topology = imported
        .mesh
        .analyze_topology()
        .map_err(|cause| format!("cannot analyze topology for {}: {cause}", QuotedPath(path)))?;
    let extended = diagnosis
        .then(|| extended(path, &imported.mesh))
        .transpose()?;
    // No analysis failure can leave a partial inspection on stdout.
    write_report(output, path, &imported, &topology)
        .and_then(|()| extended.map_or(Ok(()), |e| write_extended(output, &e)))
        .and_then(|()| output.flush())
        .map_err(|cause| format!("cannot write inspection output: {cause}"))
}

fn write_report(
    out: &mut impl Write,
    path: &Path,
    imported: &ImportedStl,
    topology: &TopologyReport,
) -> io::Result<()> {
    let format = match imported.metadata.header {
        StlHeader::Binary(_) => "binary",
        StlHeader::Ascii { .. } => "ASCII",
    };
    writeln!(out, "{} STL inspection", karasu_core::APPLICATION_NAME)?;
    // Debug-quoted OS text escapes control characters and preserves non-UTF-8 path diagnostics.
    writeln!(out, "Input path: {}", QuotedPath(path))?;
    writeln!(out, "STL format: {format}")?;
    writeln!(
        out,
        "Assumed physical units: millimeters (1 STL coordinate unit = 1 mm)"
    )?;
    writeln!(
        out,
        "Coordinate space: source (no placement, scene transform, or repair)"
    )?;
    write_geometry(out, &imported.mesh, &imported.diagnostics, topology)
}

/// Mesh statistics, bounds, diagnostics, topology and the printability disclaimer.
fn write_geometry(
    out: &mut impl Write,
    mesh: &TriangleMesh,
    diagnostics: &[MeshDiagnostic],
    topology: &TopologyReport,
) -> io::Result<()> {
    let stats = mesh.statistics();
    writeln!(out, "Triangles: {}", stats.triangle_count)?;
    writeln!(out, "Indexed vertices: {}", stats.vertex_count)?;
    if let Some(bounds) = stats.source_bounds {
        let min = bounds.min();
        let max = bounds.max();
        let size = bounds.dimensions();
        writeln!(
            out,
            "Source minimum XYZ (mm): {} {} {}",
            min.x, min.y, min.z
        )?;
        writeln!(
            out,
            "Source maximum XYZ (mm): {} {} {}",
            max.x, max.y, max.z
        )?;
        writeln!(
            out,
            "Source dimensions XYZ (mm): {} {} {}",
            size.x, size.y, size.z
        )?;
    } else {
        for field in ["minimum", "maximum", "dimensions"] {
            writeln!(out, "Source {field} XYZ (mm): absent (empty mesh)")?;
        }
    }
    write_diagnostics(out, diagnostics)?;
    write_topology(out, topology)?;
    writeln!(out, "Printability: not assessed; closed combinatorial topology does not establish a printable solid")
}

fn degeneracy(kind: Degeneracy) -> &'static str {
    match kind {
        Degeneracy::RepeatedVertex => "repeated vertex coordinates",
        Degeneracy::ZeroComputedArea => "zero computed area (not an exact collinearity proof)",
    }
}

fn write_diagnostics(out: &mut impl Write, diagnostics: &[MeshDiagnostic]) -> io::Result<()> {
    let mut counts = [0_usize; 6];
    for diagnostic in diagnostics {
        let index = match diagnostic {
            MeshDiagnostic::DegenerateTriangle { .. } => 0,
            MeshDiagnostic::DuplicateTriangle { .. } => 1,
            MeshDiagnostic::NonFiniteStoredNormal { .. } => 2,
            MeshDiagnostic::ZeroStoredNormal { .. } => 3,
            MeshDiagnostic::OpposingStoredNormal { .. } => 4,
            MeshDiagnostic::NonzeroAttribute { .. } => 5,
        };
        counts[index] += 1;
    }
    writeln!(out, "\nImport diagnostics total: {}", diagnostics.len())?;
    for (label, count) in [
        "Degenerate triangles",
        "Duplicate triangles",
        "Non-finite stored normals",
        "Zero stored normals",
        "Opposing stored normals",
        "Nonzero attributes",
    ]
    .into_iter()
    .zip(counts)
    {
        writeln!(out, "{label}: {count}")?;
    }
    writeln!(
        out,
        "Import diagnostic details shown: {} (limit {DETAIL_LIMIT})",
        diagnostics.len().min(DETAIL_LIMIT)
    )?;
    for diagnostic in diagnostics.iter().take(DETAIL_LIMIT) {
        match *diagnostic {
            MeshDiagnostic::DegenerateTriangle { triangle, kind } => writeln!(
                out,
                "  triangle {triangle}: degenerate; {}",
                degeneracy(kind)
            )?,
            MeshDiagnostic::DuplicateTriangle { triangle, first } => writeln!(
                out,
                "  triangle {triangle}: duplicate geometry of triangle {first}"
            )?,
            MeshDiagnostic::NonFiniteStoredNormal { triangle } => writeln!(
                out,
                "  triangle {triangle}: non-finite stored normal (retained)"
            )?,
            MeshDiagnostic::ZeroStoredNormal { triangle } => {
                writeln!(out, "  triangle {triangle}: zero stored normal (retained)")?;
            }
            MeshDiagnostic::OpposingStoredNormal { triangle } => writeln!(
                out,
                "  triangle {triangle}: stored normal opposes supplied winding (retained)"
            )?,
            MeshDiagnostic::NonzeroAttribute { triangle, value } => writeln!(
                out,
                "  triangle {triangle}: opaque nonzero attribute bits {value} (retained)"
            )?,
        }
    }
    writeln!(
        out,
        "Import diagnostic details omitted: {}",
        diagnostics.len().saturating_sub(DETAIL_LIMIT)
    )
}

fn status(value: CheckStatus) -> &'static str {
    match value {
        CheckStatus::Satisfied => "satisfied",
        CheckStatus::Failed => "failed",
        CheckStatus::Indeterminate => "indeterminate",
        CheckStatus::NotApplicable => "not applicable",
    }
}

fn vertex_defect(value: VertexDefect) -> &'static str {
    match value {
        VertexDefect::DisconnectedFans => "disconnected fans",
        VertexDefect::DuplicateLinkEdge => "duplicate link edge",
        VertexDefect::BranchedLink => "branched link",
    }
}

fn write_topology(out: &mut impl Write, topology: &TopologyReport) -> io::Result<()> {
    let s = topology.summary();
    writeln!(out, "\nTopology analysis: complete")?;
    for (label, value) in [
        ("Usable triangles", s.usable_triangles),
        ("Excluded triangles", s.excluded_triangles),
        ("Edges", s.edges),
        ("Boundary edges", s.boundary_edges),
        ("Two-face edges", s.two_face_edges),
        ("Non-manifold edges", s.non_manifold_edges),
        ("Winding conflicts", s.winding_conflicts),
        ("Edge-connected components", s.edge_components),
        ("Vertex-connected components", s.vertex_components),
        ("Interior-manifold vertices", s.interior_vertices),
        ("Boundary-manifold vertices", s.boundary_vertices),
        ("Non-manifold vertices", s.non_manifold_vertices),
        ("Indeterminate vertices", s.indeterminate_vertices),
        ("Isolated vertices", s.isolated_vertices),
    ] {
        writeln!(out, "{label}: {value}")?;
    }
    writeln!(out, "Edge-manifold check: {}", status(s.edge_manifold))?;
    writeln!(out, "Vertex-manifold check: {}", status(s.vertex_manifold))?;
    writeln!(out, "Local winding check: {}", status(s.local_winding))?;
    writeln!(
        out,
        "Closed combinatorial surface: {}",
        if s.closed_combinatorial_surface {
            "yes"
        } else {
            "no"
        }
    )?;
    writeln!(out, "Topology issues total: {}", topology.issues().len())?;
    writeln!(
        out,
        "Topology issue details shown: {} (limit {DETAIL_LIMIT})",
        topology.issues().len().min(DETAIL_LIMIT)
    )?;
    for issue in topology.issues().iter().take(DETAIL_LIMIT) {
        write_issue(out, issue, topology)?;
    }
    writeln!(
        out,
        "Topology issue details omitted: {}",
        topology.issues().len().saturating_sub(DETAIL_LIMIT)
    )
}

fn write_issue(
    out: &mut impl Write,
    issue: &TopologyIssue,
    topology: &TopologyReport,
) -> io::Result<()> {
    match *issue {
        TopologyIssue::ExcludedTriangle { triangle, reason } => writeln!(
            out,
            "  triangle {triangle}: excluded from topology; {}",
            degeneracy(reason)
        ),
        TopologyIssue::BoundaryEdge { edge } => writeln!(
            out,
            "  edge {edge} {:?}: boundary",
            topology.edges()[edge].key.vertices()
        ),
        TopologyIssue::NonManifoldEdge { edge } => writeln!(
            out,
            "  edge {edge} {:?}: non-manifold",
            topology.edges()[edge].key.vertices()
        ),
        TopologyIssue::WindingConflict { edge, triangles } => writeln!(
            out,
            "  edge {edge} {:?}: winding conflict between triangles {} and {}",
            topology.edges()[edge].key.vertices(),
            triangles[0],
            triangles[1]
        ),
        TopologyIssue::NonManifoldVertex { vertex, reason } => writeln!(
            out,
            "  vertex {vertex}: non-manifold; {}",
            vertex_defect(reason)
        ),
        TopologyIssue::IndeterminateVertex { vertex } => writeln!(
            out,
            "  vertex {vertex}: indeterminate (touches excluded faces)"
        ),
        TopologyIssue::IsolatedVertex { vertex } => writeln!(out, "  vertex {vertex}: isolated"),
    }
}

/// Minima listed at most, as other reports' details.
const MAX_MINIMA_LISTED: usize = 100;

/// The local minima report (TD-107): the model imported, turned by `rotate` (degrees, X then Y
/// then Z about the origin), its lowest point lowered onto the plate, then where its material
/// starts in mid-air.
fn minima_report(
    args: &InspectArgs,
    rotate: [f64; 3],
    output: &mut impl Write,
) -> Result<(), String> {
    use karasu_core::analysis::minima::{local_minima, LEVEL_TOLERANCE_MM, PLATE_CONTACT_MM};
    use karasu_core::geometry::{Affine3, Rotation3, Vector3};
    let registry = FormatRegistry::builtin();
    let imported = registry
        .import_path(
            &args.path,
            &ModelImportOptions::new(args.units),
            &mut Cancellation::never(),
        )
        .map_err(|cause| format!("cannot import {}: {cause}", QuotedPath(&args.path)))?;
    let mesh = &imported.mesh;
    let [x, y, z] = rotate;
    let turned = Rotation3::from_euler_xyz_degrees(Vector3::new(x, y, z))
        .map(Affine3::from_rotation)
        .map_err(|cause| format!("cannot turn the model: {cause}"))?;
    let lowest = mesh
        .transformed_bounds(&turned)
        .map_err(|cause| format!("cannot place the model: {cause}"))?
        .map_or(0.0, |bounds| bounds.min().z);
    let placement = Vector3::new(0.0, 0.0, -lowest);
    let placement = Affine3::from_translation(placement)
        .and_then(|lower| turned.then(lower))
        .map_err(|cause| format!("cannot place the model: {cause}"))?;
    let minima = local_minima(mesh, &placement).map_err(|cause| {
        format!(
            "cannot find the minima of {}: {cause}",
            QuotedPath(&args.path)
        )
    })?;
    let on_plate = minima.iter().filter(|m| m.on_plate).count();
    let write = |out: &mut dyn Write| -> io::Result<()> {
        writeln!(out, "Local minima (TD-107): {}", QuotedPath(&args.path))?;
        writeln!(
            out,
            "Placement: turned {x} {y} {z} degrees (X, Y, Z), lowest point on the plate"
        )?;
        writeln!(
            out,
            "Level within {LEVEL_TOLERANCE_MM} mm; on the plate at or under {PLATE_CONTACT_MM} mm"
        )?;
        writeln!(
            out,
            "Minima: {} ({on_plate} on the plate, {} in mid-air)",
            minima.len(),
            minima.len() - on_plate
        )?;
        for (k, minimum) in minima.iter().take(MAX_MINIMA_LISTED).enumerate() {
            let p = minimum.point;
            writeln!(
                out,
                "  {:>3}. x {:.4} y {:.4} z {:.4} mm, {} vertex(es){}",
                k + 1,
                p.x,
                p.y,
                p.z,
                minimum.vertices,
                if minimum.on_plate {
                    ", on the plate"
                } else {
                    ""
                }
            )?;
        }
        if minima.len() > MAX_MINIMA_LISTED {
            writeln!(
                out,
                "  … {} more not listed",
                minima.len() - MAX_MINIMA_LISTED
            )?;
        }
        out.flush()
    };
    write(output).map_err(|cause| format!("cannot write inspection output: {cause}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use karasu_core::mesh::stl::import_stl_bytes;

    #[test]
    fn report_write_errors_propagate_without_panicking() {
        struct Broken;
        impl Write for Broken {
            fn write(&mut self, _: &[u8]) -> io::Result<usize> {
                Err(io::ErrorKind::BrokenPipe.into())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let imported = import_stl_bytes(
            b"solid empty\nendsolid empty\n",
            StlOptions::new(StlUnitPolicy::AssumeMillimeters),
        )
        .unwrap();
        let report = imported.mesh.analyze_topology().unwrap();
        let error =
            write_report(&mut Broken, Path::new("empty.stl"), &imported, &report).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
    }
}
