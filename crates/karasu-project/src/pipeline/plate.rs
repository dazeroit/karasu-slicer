//! Plates of several objects, and arranged copies of one model (TD-065).
//!
//! **Preparation:** every object is prepared once at its design placement by
//! the pipeline's `prepare_object`: import, repair, placement, hollowing, drain holes and supports.
//!
//! **Instances:** each instance is that geometry moved rigidly in XY:
//! - arranged instances by the deterministic shelf packer;
//! - fixed objects not at all.
//!
//! **Slicing:** all instances are concatenated into one world mesh and sliced under the positive
//! winding rule, so overlapping instances print as their union. Drain cutters are moved with
//! their instance; footprints that carry drain holes may not overlap any other, which keeps the
//! cut rings disjoint.
use super::{
    drain_cylinders, model_label, placed_mesh,
    summary::{
        write_diagnosis, write_model, write_output, write_preparation, write_settings,
        write_source, write_subject, write_totals, write_transform,
    },
    BuiltinExporter, HoleCuts, PreparedObject, PrinterProfile, QuotedPath, Totals, TriangleMesh,
};
use crate::request::{
    Coordinates, DrainHole, Input, ModelSource, ObjectSpec, PlateSpec, Pose, Simplify,
    SliceRequest, Subject,
};
use karasu_core::{
    geometry::Point3,
    plate::{
        arrange, overlapping_pairs, DrainRequest, PlateDocument, Rect, SimplifyRequest,
        MAX_DOCUMENT_BYTES,
    },
    preparation::sampled::HoleSurface,
    raster::LayerCut,
    slicing::schedule::LayerPlane,
};
use std::{
    fmt::{self, Write},
    fs::File,
    io::Read,
    ops::{Deref, Range},
    path::Path,
};

/// A world mesh that is either an object's own or computed for the plate.
enum Surface<'a> {
    Borrowed(&'a TriangleMesh),
    Owned(TriangleMesh),
}

impl Deref for Surface<'_> {
    type Target = TriangleMesh;
    fn deref(&self) -> &TriangleMesh {
        match self {
            Self::Borrowed(mesh) => mesh,
            Self::Owned(mesh) => mesh,
        }
    }
}

/// Most overlapping pairs or unplaced instances listed in a message.
const MAX_LISTED: usize = 10;

/// The objects to prepare, their display names and the arrangement spacing.
///
/// A single-model request is its own only object. Plate document model and support paths are
/// relative to the document.
pub(super) fn objects(
    args: &SliceRequest,
) -> Result<(Vec<SliceRequest>, Vec<String>, f64), String> {
    let document;
    let (label, plate) = match &args.subject {
        Subject::Model(model) => {
            return Ok((
                vec![args.clone()],
                vec![model.display_name()],
                args.spacing_mm,
            ))
        }
        Subject::PlateFile(path) => {
            document = plate_spec(path)?;
            (QuotedPath(path).to_string(), &document)
        }
        Subject::Plate(plate) => (format!("{:?}", plate.name), plate),
    };
    let objects = (1..)
        .zip(&plate.objects)
        .map(|(index, object)| {
            args.for_object(object)
                .checked()
                .map_err(|e| format!("{label}: object {index}: {e}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let names = plate.objects.iter().map(|o| o.name.clone()).collect();
    Ok((objects, names, plate.spacing_mm))
}

/// Read a plate document as a plate of file models.
fn plate_spec(path: &Path) -> Result<PlateSpec, String> {
    let document = read_plate(path)?;
    let directory = path.parent().unwrap_or_else(|| Path::new(""));
    let objects = document
        .objects
        .into_iter()
        .map(|object| {
            let hollow = object.hollow.as_ref();
            ObjectSpec {
                model: ModelSource::File(directory.join(&object.model)),
                units: object.units,
                translate_mm: object.translate_mm,
                pose: Pose {
                    rotate_deg: object.rotate_deg,
                    scale: object.scale,
                    mirror: object.mirror,
                    center: object.center,
                    drop: object.drop,
                    lift_mm: object.lift_mm,
                },
                hollow_mm: hollow.map(|h| h.wall_mm),
                hollow_cell_mm: hollow.and_then(|h| h.cell_mm),
                drain_holes: hollow
                    .map_or(&[][..], |h| &h.drain_holes[..])
                    .iter()
                    .chain(&object.drain_holes)
                    .map(|&d| drain_hole(d))
                    .collect(),
                hole_coordinates: Coordinates::World,
                hollow_simplify: hollow.and_then(|h| h.simplify).map(|s| match s {
                    SimplifyRequest::Millimeters(mm) => Simplify::Millimeters(mm),
                    SimplifyRequest::Auto => Simplify::Auto,
                }),
                hollow_blockers: Vec::new(),
                supports: object
                    .supports
                    .as_ref()
                    .map(|s| Input::File(directory.join(s))),
                support_coordinates: Coordinates::World,
                accept_support_collisions: object.accept_support_collisions,
                copies: object.copies,
                arrange: object.arrange,
                name: object.name,
                // Plate documents (versions 1 and 2) carry only the job's repairs.
                repair: None,
            }
        })
        .collect();
    Ok(PlateSpec {
        name: path.display().to_string(),
        spacing_mm: document.spacing_mm,
        objects,
    })
}

fn drain_hole(request: DrainRequest) -> DrainHole {
    match request {
        DrainRequest::Axis {
            entry_mm,
            direction,
            length_mm,
            diameter_mm,
        } => DrainHole::Axis {
            entry_mm,
            direction,
            length_mm,
            diameter_mm,
        },
        DrainRequest::Surface {
            surface_mm,
            diameter_mm,
            outside_mm,
            inside_mm,
            depth_mm,
            from_cavity,
            tilt,
        } => DrainHole::Surface {
            surface_mm,
            diameter_mm,
            outside_mm,
            inside_mm,
            tilt,
            depth_mm,
            surface: if from_cavity {
                HoleSurface::Cavity
            } else {
                HoleSurface::Outer
            },
        },
    }
}

/// Read and validate a plate document.
fn read_plate(path: &Path) -> Result<PlateDocument, String> {
    let quoted = QuotedPath(path);
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|file| {
            file.take(u64::try_from(MAX_DOCUMENT_BYTES).unwrap_or(u64::MAX) + 1)
                .read_to_end(&mut bytes)
        })
        .map_err(|e| format!("cannot read plate {quoted}: {e}"))?;
    PlateDocument::from_json(&bytes).map_err(|e| format!("{quoted}: {e}"))
}

/// One placed copy of an object.
struct Instance {
    object: usize,
    /// 1-based copy number within the object.
    copy: u32,
    /// Rigid XY move from the design placement, in mm.
    offset: [f64; 2],
}

/// Where every instance goes, and what the plate holds.
pub(super) struct Layout {
    names: Vec<String>,
    /// Design footprint of each object.
    design: Vec<Rect>,
    instances: Vec<Instance>,
    /// Pairs of instances whose footprints overlap (united when sliced).
    overlaps: Vec<(usize, usize)>,
    spacing_mm: f64,
    /// Highest point of every object.
    pub(super) top_z: f64,
}

/// The XY rectangle of everything an object prints at its design placement: the sliced surface
/// (with supports and raft) and its drain-hole cutters.
fn footprint(object: &PreparedObject) -> Result<Rect, String> {
    let surface = world_surface(object)?;
    let mut rect = Rect {
        min: [f64::INFINITY; 2],
        max: [f64::NEG_INFINITY; 2],
    };
    let mut include = |x: f64, y: f64, radius: f64| {
        rect.min = [rect.min[0].min(x - radius), rect.min[1].min(y - radius)];
        rect.max = [rect.max[0].max(x + radius), rect.max[1].max(y + radius)];
    };
    for p in surface.vertices() {
        include(p.x, p.y, 0.);
    }
    // Drain cylinders as built: surface holes are oriented while draining.
    for cylinder in drain_cylinders(object.holes.as_ref()) {
        for [x, y, _] in [cylinder.start_mm, cylinder.end_mm] {
            include(x, y, cylinder.radius_mm);
        }
    }
    if rect.min[0] > rect.max[0] {
        return Err(format!(
            "{} contains no vertices",
            model_label(&object.args)
        ));
    }
    Ok(rect)
}

/// The sliced surface in world coordinates at the design placement.
fn world_surface(object: &PreparedObject) -> Result<Surface<'_>, String> {
    Ok(match (&object.supported, object.prepared.hollow()) {
        (Some(supported), _) => Surface::Borrowed(&supported.mesh),
        (None, Some(model)) => Surface::Borrowed(model.mesh()),
        (None, None) => Surface::Owned(placed_mesh(&object.mesh, &object.placement)?),
    })
}

impl Layout {
    /// Arrange the instances and check the plate.
    ///
    /// Arranged footprints keep one pixel pitch (the larger axis) from the plate edges, so that
    /// rounding in the rigid move can never push geometry off the panel.
    pub(super) fn new(
        objects: &[PreparedObject],
        names: Vec<String>,
        printer: &PrinterProfile,
        spacing_mm: f64,
    ) -> Result<Self, String> {
        let design = objects
            .iter()
            .map(footprint)
            .collect::<Result<Vec<_>, _>>()?;
        let mut instances = Vec::new();
        let (mut sizes, mut moved, mut fixed) = (Vec::new(), Vec::new(), Vec::new());
        for (index, (object, rect)) in objects.iter().zip(&design).enumerate() {
            for copy in 1..=object.args.copies {
                if object.args.arrange {
                    moved.push(instances.len());
                    sizes.push(rect.size());
                } else {
                    fixed.push(*rect);
                }
                instances.push(Instance {
                    object: index,
                    copy,
                    offset: [0.; 2],
                });
            }
        }
        let pitch = printer
            .lcd_profile()
            .map_err(|e| format!("invalid printer LCD: {e}"))?
            .pitch_mm();
        let margin = pitch[0].max(pitch[1]);
        let [ox, oy] = printer.origin_xy_mm;
        let [width, depth, _] = printer.build_size_mm;
        let area = Rect {
            min: [ox + margin, oy + margin],
            max: [ox + width - margin, oy + depth - margin],
        };
        let mut layout = Self {
            names,
            design,
            instances,
            overlaps: Vec::new(),
            spacing_mm,
            top_z: objects
                .iter()
                .map(|o| o.top_z)
                .fold(f64::NEG_INFINITY, f64::max),
        };
        if !sizes.is_empty() {
            let corners = arrange(&sizes, area, &fixed, spacing_mm).map_err(|unplaced| {
                let listed: Vec<String> = unplaced
                    .items
                    .iter()
                    .take(MAX_LISTED)
                    .map(|&k| layout.label(moved[k]))
                    .collect();
                format!(
                    "{} of {} arranged instances do not fit on the plate with {spacing_mm} mm \
                     spacing: {}{}",
                    unplaced.items.len(),
                    sizes.len(),
                    listed.join(", "),
                    if unplaced.items.len() > MAX_LISTED {
                        ", ..."
                    } else {
                        ""
                    }
                )
            })?;
            for (&slot, corner) in moved.iter().zip(corners) {
                let rect = layout.design[layout.instances[slot].object];
                layout.instances[slot].offset = [corner[0] - rect.min[0], corner[1] - rect.min[1]];
            }
        }
        layout.overlaps = overlapping_pairs(&layout.footprints());
        if let Some(&(a, b)) = layout.overlaps.iter().find(|&&(a, b)| {
            [a, b].iter().any(|&k| {
                !objects[layout.instances[k].object]
                    .args
                    .drain_holes
                    .is_empty()
            })
        }) {
            return Err(format!(
                "the footprints of {} and {} overlap, and drain holes would cut both; move them \
                 apart",
                layout.label(a),
                layout.label(b)
            ));
        }
        Ok(layout)
    }

    /// Each instance: its object, its 1-based copy number, and its rigid XY move from the
    /// object's design placement, in mm.
    pub(super) fn placed(&self) -> impl Iterator<Item = (usize, u32, [f64; 2])> + '_ {
        self.instances.iter().map(|i| (i.object, i.copy, i.offset))
    }

    /// Each instance's footprint where it prints.
    fn footprints(&self) -> Vec<Rect> {
        self.instances
            .iter()
            .map(|i| {
                let r = self.design[i.object];
                Rect {
                    min: [r.min[0] + i.offset[0], r.min[1] + i.offset[1]],
                    max: [r.max[0] + i.offset[0], r.max[1] + i.offset[1]],
                }
            })
            .collect()
    }

    fn label(&self, instance: usize) -> String {
        let i = &self.instances[instance];
        format!(
            "object {} ({:?}) copy {}",
            i.object + 1,
            self.names[i.object],
            i.copy
        )
    }

    /// Every instance's sliced surface, moved into place, as one world mesh.
    pub(super) fn united(
        &self,
        objects: &[PreparedObject],
    ) -> Result<(TriangleMesh, Vec<Range<usize>>), String> {
        let surfaces = objects
            .iter()
            .map(|o| world_surface(o).map(Some))
            .collect::<Result<Vec<_>, _>>()?;
        let (mesh, bases) = self.concatenate_at(&surfaces)?;
        // Each instance's supports, which the drain cutters leave (TD-123).
        let kept = bases
            .into_iter()
            .filter_map(|(object, base)| {
                let faces = objects[object].kept_faces()?;
                Some(base + faces.start..base + faces.end)
            })
            .collect();
        Ok((mesh, kept))
    }

    /// Every hollowed instance's cavity, moved into place (TD-095), or `None` when no object is
    /// hollowed.
    pub(super) fn cavities(
        &self,
        objects: &[PreparedObject],
    ) -> Result<Option<TriangleMesh>, String> {
        let cavities: Vec<_> = objects
            .iter()
            .map(|o| o.prepared.hollow().map(|m| Surface::Borrowed(m.cavity())))
            .collect();
        if cavities.iter().all(Option::is_none) {
            return Ok(None);
        }
        self.concatenate(&cavities).map(Some)
    }

    /// Every instance's placed model, without supports (adaptive layer heights read it).
    pub(super) fn models(&self, objects: &[PreparedObject]) -> Result<TriangleMesh, String> {
        let models = objects
            .iter()
            .map(|o| placed_mesh(&o.mesh, &o.placement).map(|m| Some(Surface::Owned(m))))
            .collect::<Result<Vec<_>, _>>()?;
        self.concatenate(&models)
    }

    /// The instances' meshes moved into place; objects without one (`None`) are left out.
    fn concatenate(&self, meshes: &[Option<Surface<'_>>]) -> Result<TriangleMesh, String> {
        self.concatenate_at(meshes).map(|(mesh, _)| mesh)
    }

    /// [`Self::concatenate`], with the object and first face of each instance placed.
    fn concatenate_at(
        &self,
        meshes: &[Option<Surface<'_>>],
    ) -> Result<(TriangleMesh, Vec<(usize, usize)>), String> {
        let (mut vertices, mut triangles) = (Vec::new(), Vec::new());
        let mut bases = Vec::new();
        for instance in &self.instances {
            let Some(mesh) = &meshes[instance.object] else {
                continue;
            };
            bases.push((instance.object, triangles.len()));
            let [dx, dy] = instance.offset;
            let base = u32::try_from(vertices.len())
                .ok()
                .filter(|b| {
                    u32::try_from(mesh.vertices().len()).is_ok_and(|n| b.checked_add(n).is_some())
                })
                .ok_or("the plate has too many vertices for one mesh")?;
            vertices.extend(
                mesh.vertices()
                    .iter()
                    .map(|p| Point3::new(p.x + dx, p.y + dy, p.z)),
            );
            triangles.extend(mesh.triangles().iter().map(|t| t.map(|v| v + base)));
        }
        let mesh = TriangleMesh::try_new(vertices, triangles)
            .map_err(|e| format!("cannot unite the plate: {e}"))?;
        Ok((mesh, bases))
    }

    /// The drain cutters of every instance, moved into place.
    pub(super) fn cut<'a>(&self, objects: &'a [PreparedObject]) -> Option<PlateCut<'a>> {
        let cuts: Vec<_> = self
            .instances
            .iter()
            .filter_map(|i| objects[i.object].cut().map(|cut| (cut, i.offset)))
            .collect();
        (!cuts.is_empty()).then_some(PlateCut { cuts })
    }
}

/// Drain cutters of several instances, removed as the union of their rings (TD-089).
pub(super) struct PlateCut<'a> {
    cuts: Vec<(&'a HoleCuts, [f64; 2])>,
}

impl LayerCut for PlateCut<'_> {
    fn rings(&self, plane: LayerPlane) -> Vec<Vec<[f64; 2]>> {
        self.cuts
            .iter()
            .flat_map(|&(cut, [dx, dy])| {
                cut.rings(plane)
                    .into_iter()
                    .map(move |ring| ring.into_iter().map(|[x, y]| [x + dx, y + dy]).collect())
            })
            .collect()
    }
}

/// The summary of a plate or of arranged copies: plate-wide lines, then each object with its
/// instances.
pub(super) fn write_summary(
    out: &mut impl Write,
    args: &SliceRequest,
    machine: Option<&BuiltinExporter>,
    totals: &Totals<'_>,
    objects: &[PreparedObject],
    layout: &Layout,
) -> fmt::Result {
    writeln!(out, "{} slice", karasu_core::APPLICATION_NAME)?;
    write_subject(out, &args.subject)?;
    write_output(out, args, machine)?;
    writeln!(
        out,
        "Objects: {}, instances {}, spacing {} mm",
        objects.len(),
        layout.instances.len(),
        layout.spacing_mm
    )?;
    write_settings(out, args, totals)?;
    write_totals(out, args, totals)?;
    for (index, object) in objects.iter().enumerate() {
        writeln!(out, "Object {}: {:?}", index + 1, layout.names[index])?;
        if let Some(model) = object.args.model() {
            write_model(out, "Model", model)?;
        }
        write_source(out, &object.diagnosis.source)?;
        let [w, d] = layout.design[index].size();
        writeln!(
            out,
            "Instances: {} ({}), footprint {w} x {d} mm",
            object.args.copies,
            if object.args.arrange {
                "arranged"
            } else {
                "fixed"
            }
        )?;
        let [tx, ty, tz] = object.placement.translation().components();
        for instance in layout.instances.iter().filter(|i| i.object == index) {
            let [dx, dy] = instance.offset;
            writeln!(
                out,
                "Instance {} translation (mm): {} {} {tz}",
                instance.copy,
                tx + dx,
                ty + dy
            )?;
        }
        write_transform(out, &object.args.pose)?;
        write_diagnosis(out, &object.diagnosis)?;
        if let Some(supported) = &object.supported {
            for line in &supported.lines {
                writeln!(out, "{line}")?;
            }
        }
        write_preparation(out, &object.prepared, object.holes.as_ref())?;
    }
    if layout.overlaps.is_empty() {
        writeln!(out, "Overlapping footprints: none")
    } else {
        let listed: Vec<String> = layout
            .overlaps
            .iter()
            .take(MAX_LISTED)
            .map(|&(a, b)| format!("{} and {}", layout.label(a), layout.label(b)))
            .collect();
        writeln!(
            out,
            "Overlapping footprints: {} pairs, united: {}{}",
            layout.overlaps.len(),
            listed.join("; "),
            if layout.overlaps.len() > MAX_LISTED {
                "; ..."
            } else {
                ""
            }
        )
    }
}
