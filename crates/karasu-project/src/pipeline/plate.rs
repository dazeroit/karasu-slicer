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
//! their instance, and each cuts only its own instance's material (TD-166): an instance whose
//! cutters reach another one's footprint is sliced in a cut group of its own, apart from every
//! instance it reaches. Strict layers cannot keep groups apart and refuse such a plate.
use super::{
    model_label, placed_mesh,
    summary::{
        write_diagnosis, write_model, write_output, write_preparation, write_settings,
        write_source, write_subject, write_totals, write_transform,
    },
    BuiltinExporter, HoleCuts, PreparedObject, PrinterProfile, QuotedPath, Totals, TriangleMesh,
    DRAIN_DISTANCE_ERROR_MM,
};
use crate::request::{
    Coordinates, DrainHole, Input, ModelSource, ObjectSpec, PlateSpec, Pose, Simplify,
    SliceRequest, Subject,
};
use karasu_core::{
    geometry::Point3,
    plate::{
        arrange_turning, convex_hull, footprint_turn,
        outline::{
            self, arrange_outlines, bounded, bounds, outlines_overlap, MAX_OUTLINE_VERTICES,
        },
        overlapping_pairs, turned_size, turns_offered, DrainRequest, Footprint, PlateDocument,
        Rect, SimplifyRequest, Unplaced, MAX_DOCUMENT_BYTES, TURN_MARGIN,
    },
    preparation::sampled::HoleSurface,
    raster::LayerCut,
    slicing::{schedule::LayerPlane, winding::FaceGroup},
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
    /// Rigid XY move from the design placement, in mm, after the turn.
    offset: [f64; 2],
    /// Its turn about Z, in degrees within [-180, 180), about its object's pivot (issue #48):
    /// zero unless arranged with turns.
    turn_deg: f64,
    /// Its cut group (TD-166): 0, unless its drain cutters reach another instance.
    group: usize,
}

/// How arranged instances are packed (issue #48, TD-165): whether they may turn about Z, and
/// whether they take their rectangles or their outlines. The default turns nothing and packs
/// rectangles, as plates always were.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Arrangement {
    pub(super) turn: bool,
    pub(super) footprint: Footprint,
}

/// A drain cylinder of an object at its design placement: its axis's ends and its radius, in
/// world mm, with its 1-based hole number.
#[derive(Clone, Copy, Debug)]
struct Cylinder {
    hole: usize,
    start: [f64; 3],
    end: [f64; 3],
    /// The axis's unit direction.
    direction: [f64; 3],
    radius: f64,
}

/// The cosine and sine of `turn_deg`, exact for quarter turns.
fn cos_sin(turn_deg: f64) -> (f64, f64) {
    let quarter = turn_deg.rem_euclid(360.0);
    #[allow(clippy::float_cmp)] // Exact quarter turns only.
    if quarter == 0.0 {
        (1.0, 0.0)
    } else if quarter == 90.0 {
        (0.0, 1.0)
    } else if quarter == 180.0 {
        (-1.0, 0.0)
    } else if quarter == 270.0 {
        (0.0, -1.0)
    } else {
        let (sin, cos) = turn_deg.to_radians().sin_cos();
        (cos, sin)
    }
}

/// `turn_deg` within [-180°, 180°).
fn wrapped(turn_deg: f64) -> f64 {
    (turn_deg + 180.0).rem_euclid(360.0) - 180.0
}

/// `p` turned about `pivot` by the angle of `cos` and `sin`.
fn turn_about(p: [f64; 2], pivot: [f64; 2], cos: f64, sin: f64) -> [f64; 2] {
    let [x, y] = [p[0] - pivot[0], p[1] - pivot[1]];
    [x * cos - y * sin + pivot[0], x * sin + y * cos + pivot[1]]
}

/// A drain cutter that reaches another instance's footprint (TD-166).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Reach {
    /// The instance whose cutter it is.
    from: usize,
    /// Its drain hole, 1-based.
    hole: usize,
    /// The instance it reaches.
    to: usize,
}

/// The plate's world mesh, its supports' faces and its instances in cut groups 1 and above
/// (see [`Layout::united`]).
type United = (TriangleMesh, Vec<Range<usize>>, Vec<FaceGroup>);

/// Where every instance goes, and what the plate holds.
pub(super) struct Layout {
    names: Vec<String>,
    /// Design footprint of each object.
    design: Vec<Rect>,
    /// Each object's convex outline at its design placement (TD-165), for the overlap test.
    outlines: Vec<Vec<[f64; 2]>>,
    instances: Vec<Instance>,
    /// Pairs of instances whose footprints overlap (united when sliced): their rectangles, and
    /// then their outlines (TD-165).
    overlaps: Vec<(usize, usize)>,
    /// Drain cutters that reach another instance, in instance and hole order (TD-166).
    reaches: Vec<Reach>,
    /// Drain cylinders of each object at its design placement.
    cutters: Vec<Vec<Cylinder>>,
    spacing_mm: f64,
    /// Highest point of every object.
    pub(super) top_z: f64,
}

/// The XY rectangle of everything an object prints at its design placement: the sliced surface,
/// with supports and raft. Drain cutters are not part of it: each cuts only its own object
/// (TD-166), and [`cutter_box`] bounds them.
fn footprint(object: &PreparedObject) -> Result<Rect, String> {
    let surface = world_surface(object)?;
    let mut rect = Rect {
        min: [f64::INFINITY; 2],
        max: [f64::NEG_INFINITY; 2],
    };
    for p in surface.vertices() {
        rect.min = [rect.min[0].min(p.x), rect.min[1].min(p.y)];
        rect.max = [rect.max[0].max(p.x), rect.max[1].max(p.y)];
    }
    if rect.min[0] > rect.max[0] {
        return Err(format!(
            "{} contains no vertices",
            model_label(&object.args)
        ));
    }
    Ok(rect)
}

/// The convex outline of the sliced surface of `object` at its design placement, from above
/// (TD-165): what the overlap test compares where rectangles overlap.
fn outline(object: &PreparedObject) -> Result<Vec<[f64; 2]>, String> {
    let surface = world_surface(object)?;
    let points: Vec<[f64; 2]> = surface.vertices().iter().map(|p| [p.x, p.y]).collect();
    Ok(convex_hull(&points))
}

/// The drain cylinders of `object` at its design placement, as cut.
fn cylinders(object: &PreparedObject) -> Vec<Cylinder> {
    let Some(holes) = &object.holes else {
        return Vec::new();
    };
    holes
        .specs
        .iter()
        .enumerate()
        .filter_map(|(index, spec)| {
            let a = spec.unit_direction().ok()?;
            let p = spec.entry_mm;
            Some(Cylinder {
                hole: index + 1,
                start: p,
                end: std::array::from_fn(|k| p[k] + a[k] * spec.length_mm),
                direction: a,
                radius: spec.diameter_mm / 2.0,
            })
        })
        .collect()
}

/// The XY box of drain `cylinder` (TD-166).
///
/// A cylinder with axis from `p` to `q`, unit direction `a` and radius `r` spans
/// `min(p_k, q_k) - r·√(1 - a_k²)` to `max(p_k, q_k) + r·√(1 - a_k²)` along axis `k`: its end
/// discs' extent. The polygonal cylinder as built lies within [`DRAIN_DISTANCE_ERROR_MM`] of it,
/// so the box grows by that much, and its rings never leave it.
fn cutter_box(cylinder: &Cylinder) -> Rect {
    let (p, q, a) = (cylinder.start, cylinder.end, cylinder.direction);
    let reach =
        |k: usize| cylinder.radius * (1.0 - a[k] * a[k]).max(0.0).sqrt() + DRAIN_DISTANCE_ERROR_MM;
    Rect {
        min: [p[0].min(q[0]) - reach(0), p[1].min(q[1]) - reach(1)],
        max: [p[0].max(q[0]) + reach(0), p[1].max(q[1]) + reach(1)],
    }
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
    /// Arrange the instances and check the plate, packing the arranged ones as `arrangement`
    /// says.
    ///
    /// Arranged footprints keep one pixel pitch (the larger axis) from the plate edges, so that
    /// rounding in the rigid move can never push geometry off the panel.
    pub(super) fn new(
        objects: &[PreparedObject],
        names: Vec<String>,
        printer: &PrinterProfile,
        spacing_mm: f64,
        arrangement: Arrangement,
    ) -> Result<Self, String> {
        let design = objects
            .iter()
            .map(footprint)
            .collect::<Result<Vec<_>, _>>()?;
        let outlines = objects.iter().map(outline).collect::<Result<Vec<_>, _>>()?;
        let mut instances = Vec::new();
        let (mut moved, mut fixed) = (Vec::new(), Vec::new());
        for (index, (object, rect)) in objects.iter().zip(&design).enumerate() {
            for copy in 1..=object.args.copies {
                if object.args.arrange {
                    moved.push(instances.len());
                } else {
                    fixed.push((index, *rect));
                }
                instances.push(Instance {
                    object: index,
                    copy,
                    offset: [0.; 2],
                    turn_deg: 0.0,
                    group: 0,
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
            outlines,
            instances,
            overlaps: Vec::new(),
            reaches: Vec::new(),
            cutters: objects.iter().map(cylinders).collect(),
            spacing_mm,
            top_z: objects
                .iter()
                .map(|o| o.top_z)
                .fold(f64::NEG_INFINITY, f64::max),
        };
        if !moved.is_empty() {
            let unplaced = |unplaced: Unplaced| {
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
                    moved.len(),
                    listed.join(", "),
                    if unplaced.items.len() > MAX_LISTED {
                        ", ..."
                    } else {
                        ""
                    }
                )
            };
            // Each object's own turn first (TD-162): the one that makes its outline's rectangle
            // smallest, or makes it fit.
            let turns: Vec<f64> = (0..objects.len())
                .map(|o| {
                    if arrangement.turn {
                        footprint_turn(&layout.outlines[o], area.size(), TURN_MARGIN)
                    } else {
                        0.0
                    }
                })
                .collect();
            let placements = match arrangement.footprint {
                Footprint::Box => layout.boxes(&moved, &turns, &fixed, (area, arrangement)),
                Footprint::Outline => {
                    layout.by_outline(&moved, &turns, &fixed, (area, arrangement))
                }
            }
            .map_err(unplaced)?;
            for (&slot, (turn_deg, offset)) in moved.iter().zip(placements) {
                layout.instances[slot].turn_deg = turn_deg;
                layout.instances[slot].offset = offset;
            }
        }
        layout.overlaps = layout.overlapping();
        layout.group_cutters();
        Ok(layout)
    }

    /// The `moved` instances packed as rectangles, each object first turned by `turns`, and
    /// with turns free to lie a quarter turn more; clear of the `fixed` instances. Each
    /// instance's turn and move.
    fn boxes(
        &self,
        moved: &[usize],
        turns: &[f64],
        fixed: &[(usize, Rect)],
        (area, arrangement): (Rect, Arrangement),
    ) -> Result<Vec<(f64, [f64; 2])>, Unplaced> {
        let object = |slot: usize| self.instances[slot].object;
        let sizes: Vec<[f64; 2]> = moved
            .iter()
            .map(|&slot| {
                let o = object(slot);
                if turns[o] == 0.0 {
                    self.design[o].size()
                } else {
                    turned_size(&self.outlines[o], turns[o])
                }
            })
            .collect();
        let rects: Vec<Rect> = fixed.iter().map(|&(_, rect)| rect).collect();
        let free = vec![arrangement.turn; moved.len()];
        let placed = arrange_turning(&sizes, &free, area, &rects, self.spacing_mm)?;
        Ok(moved
            .iter()
            .zip(placed)
            .map(|(&slot, placed)| {
                let o = object(slot);
                let turn_deg = wrapped(turns[o] + if placed.turned { 90.0 } else { 0.0 });
                // The rectangle as it lies, its lowest corner at the corner planned.
                let rect = self.turned_rect(o, turn_deg);
                (
                    turn_deg,
                    [
                        placed.corner[0] - rect.min[0],
                        placed.corner[1] - rect.min[1],
                    ],
                )
            })
            .collect())
    }

    /// The `moved` instances packed by their outlines (TD-165), each object first turned by
    /// `turns` and, with turns, offered the turns of [`turns_offered`]; clear of the `fixed`
    /// instances' outlines. Each instance's turn and move.
    fn by_outline(
        &self,
        moved: &[usize],
        turns: &[f64],
        fixed: &[(usize, Rect)],
        (area, arrangement): (Rect, Arrangement),
    ) -> Result<Vec<(f64, [f64; 2])>, Unplaced> {
        let object = |slot: usize| self.instances[slot].object;
        // Each moved instance's turns offered, and its outline about its pivot turned by each.
        let offers: Vec<Vec<f64>> = moved
            .iter()
            .map(|&slot| {
                let o = object(slot);
                turns_offered(arrangement.turn, turns[o])
                    .into_iter()
                    .map(|t| turns[o] + t)
                    .collect()
            })
            .collect();
        let shapes: Vec<Vec<Vec<[f64; 2]>>> = moved
            .iter()
            .zip(&offers)
            .map(|(&slot, offers)| {
                let o = object(slot);
                let pivot = self.pivot(o);
                let about = outline::translated(
                    &bounded(&self.outlines[o], MAX_OUTLINE_VERTICES),
                    [-pivot[0], -pivot[1]],
                );
                offers.iter().map(|&t| outline::turned(&about, t)).collect()
            })
            .collect();
        let obstacles: Vec<Vec<[f64; 2]>> = fixed
            .iter()
            .map(|&(o, _)| self.outlines[o].clone())
            .collect();
        let placed = arrange_outlines(&shapes, area, &obstacles, self.spacing_mm)?;
        Ok(moved
            .iter()
            .zip(placed)
            .zip(&offers)
            .map(|((&slot, placed), offers)| {
                let pivot = self.pivot(object(slot));
                // The pivot goes where the packer put the outline's origin.
                (
                    wrapped(offers[placed.orientation]),
                    [placed.offset[0] - pivot[0], placed.offset[1] - pivot[1]],
                )
            })
            .collect())
    }

    /// The point object `object` turns about: the middle of its design footprint.
    fn pivot(&self, object: usize) -> [f64; 2] {
        let r = self.design[object];
        [
            f64::midpoint(r.min[0], r.max[0]),
            f64::midpoint(r.min[1], r.max[1]),
        ]
    }

    /// Object `object`'s footprint turned `turn_deg` about its pivot, before any move: its design
    /// footprint for no turn, otherwise its outline's rectangle so turned.
    fn turned_rect(&self, object: usize, turn_deg: f64) -> Rect {
        if turn_deg == 0.0 {
            return self.design[object];
        }
        let (cos, sin) = cos_sin(turn_deg);
        let pivot = self.pivot(object);
        let turned: Vec<[f64; 2]> = self.outlines[object]
            .iter()
            .map(|&p| turn_about(p, pivot, cos, sin))
            .collect();
        bounds(&turned)
    }

    /// Where `instance` puts point `p` of its object's design placement, in world XY: turned
    /// about the object's pivot, then moved. With no turn, only moved, as plates always were.
    fn place(&self, instance: &Instance, p: [f64; 2], (cos, sin): (f64, f64)) -> [f64; 2] {
        let [dx, dy] = instance.offset;
        if instance.turn_deg == 0.0 {
            return [p[0] + dx, p[1] + dy];
        }
        let [x, y] = turn_about(p, self.pivot(instance.object), cos, sin);
        [x + dx, y + dy]
    }

    /// The pairs of instances whose footprints overlap, in increasing order: their rectangles,
    /// then, where those overlap, their convex outlines (TD-165), so models clear of each other
    /// whose rectangles overlap, as Arrange by outline places them, are not united.
    fn overlapping(&self) -> Vec<(usize, usize)> {
        let at = |k: usize| self.placed_outline(k);
        overlapping_pairs(&self.footprints())
            .into_iter()
            .filter(|&(a, b)| outlines_overlap(&at(a), &at(b)))
            .collect()
    }

    /// Find the drain cutters that reach another instance's footprint, and give each instance
    /// its cut group (TD-166).
    ///
    /// Group 0 holds every instance but those whose cutters reach another; their cutters cut
    /// only their own material. Each of the others takes, in instance order, the lowest group
    /// from 1 that holds no instance it reaches or that reaches it, so no group holds a cutter
    /// and an instance it reaches. Touching counts as reaching.
    fn group_cutters(&mut self) {
        let footprints = self.footprints();
        let meets = |a: &Rect, b: &Rect| {
            a.min[0] <= b.max[0]
                && b.min[0] <= a.max[0]
                && a.min[1] <= b.max[1]
                && b.min[1] <= a.max[1]
        };
        let mut reaches = Vec::new();
        for (from, instance) in self.instances.iter().enumerate() {
            let boxes: Vec<(usize, Rect)> = self.cutters[instance.object]
                .iter()
                .map(|cylinder| (cylinder.hole, self.placed_cutter(instance, cylinder)))
                .collect();
            let Some(all) = boxes.iter().map(|&(_, r)| r).reduce(|a, b| Rect {
                min: [a.min[0].min(b.min[0]), a.min[1].min(b.min[1])],
                max: [a.max[0].max(b.max[0]), a.max[1].max(b.max[1])],
            }) else {
                continue;
            };
            for &(hole, rect) in &boxes {
                for (to, footprint) in footprints.iter().enumerate() {
                    if to != from && meets(&all, footprint) && meets(&rect, footprint) {
                        reaches.push(Reach { from, hole, to });
                    }
                }
            }
        }
        let mut neighbours = vec![Vec::new(); self.instances.len()];
        for reach in &reaches {
            neighbours[reach.from].push(reach.to);
            neighbours[reach.to].push(reach.from);
        }
        let mut reaching = vec![false; self.instances.len()];
        for reach in &reaches {
            reaching[reach.from] = true;
        }
        for index in 0..self.instances.len() {
            if !reaching[index] {
                continue;
            }
            let taken: Vec<usize> = neighbours[index]
                .iter()
                .filter(|&&other| reaching[other])
                .map(|&other| self.instances[other].group)
                .collect();
            // Among one more group than it has neighbours, one is always free.
            let free = taken.len() + 1;
            self.instances[index].group = (1..=free).find(|g| !taken.contains(g)).unwrap_or(free);
        }
        self.reaches = reaches;
    }

    /// Refuse drain cutters that reach another instance, for strict layers: they cannot keep a
    /// cut to its own instance (TD-166).
    ///
    /// # Errors
    /// The first such cutter, named with the instance it reaches.
    pub(super) fn refuse_reaches(&self) -> Result<(), String> {
        match self.reaches.first() {
            None => Ok(()),
            Some(reach) => Err(format!(
                "drain hole {} of {} reaches {}, and strict layers cannot keep a drain hole to \
                 its own model; slice with union layers or move them apart",
                reach.hole,
                self.label(reach.from),
                self.label(reach.to)
            )),
        }
    }

    /// Each instance: its object, its 1-based copy number, and its rigid XY move from the
    /// object's design placement, in mm. The model export lays plates out with no turns
    /// ([`Arrangement::default`]), so the move is the whole of it there.
    pub(super) fn placed(&self) -> impl Iterator<Item = (usize, u32, [f64; 2])> + '_ {
        self.instances.iter().map(|i| (i.object, i.copy, i.offset))
    }

    /// Each instance's footprint where it prints.
    fn footprints(&self) -> Vec<Rect> {
        self.instances
            .iter()
            .map(|i| {
                let r = self.turned_rect(i.object, i.turn_deg);
                Rect {
                    min: [r.min[0] + i.offset[0], r.min[1] + i.offset[1]],
                    max: [r.max[0] + i.offset[0], r.max[1] + i.offset[1]],
                }
            })
            .collect()
    }

    /// Instance `k`'s outline where it prints.
    fn placed_outline(&self, k: usize) -> Vec<[f64; 2]> {
        let instance = &self.instances[k];
        let turn = cos_sin(instance.turn_deg);
        self.outlines[instance.object]
            .iter()
            .map(|&p| self.place(instance, p, turn))
            .collect()
    }

    /// The XY box of `instance`'s drain `cylinder` where it prints: the cylinder turned and
    /// moved with it.
    fn placed_cutter(&self, instance: &Instance, cylinder: &Cylinder) -> Rect {
        if instance.turn_deg == 0.0 {
            let r = cutter_box(cylinder);
            let [dx, dy] = instance.offset;
            return Rect {
                min: [r.min[0] + dx, r.min[1] + dy],
                max: [r.max[0] + dx, r.max[1] + dy],
            };
        }
        let turn = cos_sin(instance.turn_deg);
        let end = |p: [f64; 3]| {
            let [x, y] = self.place(instance, [p[0], p[1]], turn);
            [x, y, p[2]]
        };
        let (cos, sin) = turn;
        let a = cylinder.direction;
        cutter_box(&Cylinder {
            start: end(cylinder.start),
            end: end(cylinder.end),
            direction: [a[0] * cos - a[1] * sin, a[0] * sin + a[1] * cos, a[2]],
            ..*cylinder
        })
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

    /// Every instance's sliced surface, moved into place, as one world mesh; the faces of each
    /// instance's supports, which the drain cutters leave (TD-123); and the instances in cut
    /// groups 1 and above, whose cutters cut only their own material (TD-166).
    pub(super) fn united(&self, objects: &[PreparedObject]) -> Result<United, String> {
        let surfaces = objects
            .iter()
            .map(|o| world_surface(o).map(Some))
            .collect::<Result<Vec<_>, _>>()?;
        let (mesh, ranges) = self.concatenate_at(&surfaces)?;
        let kept = ranges
            .iter()
            .filter_map(|(instance, faces, _)| {
                let kept = objects[self.instances[*instance].object].kept_faces()?;
                Some(faces.start + kept.start..faces.start + kept.end)
            })
            .collect();
        let groups = ranges
            .into_iter()
            .filter(|(instance, _, _)| self.instances[*instance].group > 0)
            .map(|(instance, faces, vertices)| FaceGroup {
                group: self.instances[instance].group,
                faces,
                vertices,
            })
            .collect();
        Ok((mesh, kept, groups))
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

    /// [`Self::concatenate`], with each instance placed and the ranges of its faces and
    /// vertices in the mesh.
    #[allow(clippy::type_complexity)] // A mesh and, per instance, two index ranges.
    fn concatenate_at(
        &self,
        meshes: &[Option<Surface<'_>>],
    ) -> Result<(TriangleMesh, Vec<(usize, Range<usize>, Range<usize>)>), String> {
        let (mut vertices, mut triangles) = (Vec::new(), Vec::new());
        let mut bases = Vec::new();
        for (index, instance) in self.instances.iter().enumerate() {
            let Some(mesh) = &meshes[instance.object] else {
                continue;
            };
            bases.push((
                index,
                triangles.len()..triangles.len() + mesh.triangles().len(),
                vertices.len()..vertices.len() + mesh.vertices().len(),
            ));
            let turn = cos_sin(instance.turn_deg);
            let base = u32::try_from(vertices.len())
                .ok()
                .filter(|b| {
                    u32::try_from(mesh.vertices().len()).is_ok_and(|n| b.checked_add(n).is_some())
                })
                .ok_or("the plate has too many vertices for one mesh")?;
            vertices.extend(mesh.vertices().iter().map(|p| {
                let [x, y] = self.place(instance, [p.x, p.y], turn);
                Point3::new(x, y, p.z)
            }));
            triangles.extend(mesh.triangles().iter().map(|t| t.map(|v| v + base)));
        }
        let mesh = TriangleMesh::try_new(vertices, triangles)
            .map_err(|e| format!("cannot unite the plate: {e}"))?;
        Ok((mesh, bases))
    }

    /// The drain cutters of every instance, moved into place, each in its instance's cut group:
    /// `groups` are [`Self::united`]'s (TD-166).
    pub(super) fn cut<'a>(
        &self,
        objects: &'a [PreparedObject],
        groups: Vec<FaceGroup>,
    ) -> Option<PlateCut<'a>> {
        let cuts: Vec<_> = self
            .instances
            .iter()
            .filter_map(|i| {
                let placing = Placing {
                    offset: i.offset,
                    turn_deg: i.turn_deg,
                    pivot: self.pivot(i.object),
                };
                objects[i.object].cut().map(|cut| (cut, placing, i.group))
            })
            .collect();
        let count = groups.iter().map(|g| g.group + 1).max().unwrap_or(1);
        (!cuts.is_empty()).then_some(PlateCut {
            cuts,
            groups,
            count,
        })
    }
}

/// How an instance's drain rings are placed: turned about the pivot, then moved (issue #48).
#[derive(Clone, Copy)]
struct Placing {
    offset: [f64; 2],
    turn_deg: f64,
    pivot: [f64; 2],
}

/// Drain cutters of several instances, removed as the union of their rings (TD-089), each
/// from its own cut group's material (TD-166).
pub(super) struct PlateCut<'a> {
    /// Each instance's cutters, how they are placed, and its cut group.
    cuts: Vec<(&'a HoleCuts, Placing, usize)>,
    /// The instances in cut groups 1 and above, in the united mesh.
    groups: Vec<FaceGroup>,
    /// Number of cut groups, group 0 included.
    count: usize,
}

impl PlateCut<'_> {
    /// The rings of the cutters of group `group`, or of every group, turned and moved into place.
    fn moved_rings(&self, plane: LayerPlane, group: Option<usize>) -> Vec<Vec<[f64; 2]>> {
        self.cuts
            .iter()
            .filter(|&&(_, _, g)| group.is_none_or(|group| g == group))
            .flat_map(|&(cut, placing, _)| {
                let [dx, dy] = placing.offset;
                let (cos, sin) = cos_sin(placing.turn_deg);
                cut.rings(plane).into_iter().map(move |ring| {
                    ring.into_iter()
                        .map(|p| {
                            if placing.turn_deg == 0.0 {
                                [p[0] + dx, p[1] + dy]
                            } else {
                                let [x, y] = turn_about(p, placing.pivot, cos, sin);
                                [x + dx, y + dy]
                            }
                        })
                        .collect()
                })
            })
            .collect()
    }
}

impl LayerCut for PlateCut<'_> {
    fn rings(&self, plane: LayerPlane) -> Vec<Vec<[f64; 2]>> {
        self.moved_rings(plane, None)
    }

    fn groups(&self) -> &[FaceGroup] {
        &self.groups
    }

    fn group_rings(&self, plane: LayerPlane) -> Vec<Vec<Vec<[f64; 2]>>> {
        (0..self.count)
            .map(|group| self.moved_rings(plane, Some(group)))
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
            // The design translation turned and moved with the instance (issue #48).
            let [x, y] = layout.place(instance, [tx, ty], cos_sin(instance.turn_deg));
            writeln!(
                out,
                "Instance {} translation (mm): {x} {y} {tz}",
                instance.copy
            )?;
            if instance.turn_deg != 0.0 {
                writeln!(
                    out,
                    "Instance {} turn about Z (deg): {}",
                    instance.copy, instance.turn_deg
                )?;
            }
        }
        write_transform(out, &object.args.pose)?;
        write_diagnosis(out, &object.diagnosis)?;
        if let Some(supported) = &object.supported {
            for line in &supported.lines {
                writeln!(out, "{line}")?;
            }
        }
        write_preparation(
            out,
            &object.prepared,
            object.holes.as_ref(),
            object.args.hollow_blockers.len(),
        )?;
    }
    if layout.overlaps.is_empty() {
        writeln!(out, "Overlapping footprints: none")?;
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
        )?;
    }
    write_reaches(out, layout)
}

/// The drain cutters that reach other instances, said only when there are any, so other
/// summaries are unchanged (TD-166).
fn write_reaches(out: &mut impl Write, layout: &Layout) -> fmt::Result {
    if !layout.reaches.is_empty() {
        let listed: Vec<String> = layout
            .reaches
            .iter()
            .take(MAX_LISTED)
            .map(|r| {
                format!(
                    "hole {} of {} reaches {}",
                    r.hole,
                    layout.label(r.from),
                    layout.label(r.to)
                )
            })
            .collect();
        writeln!(
            out,
            "Drain holes reaching other models: {}, each cutting only its own model: {}{}",
            layout.reaches.len(),
            listed.join("; "),
            if layout.reaches.len() > MAX_LISTED {
                "; ..."
            } else {
                ""
            }
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fixed 10 x 10 mm objects at `corners`, with cutters along X whose boxes are
    /// `[min x, min y, max x, max y]` relative to their corners (within the 1 µm tolerance), and
    /// their cutters grouped.
    fn layout(objects: &[([f64; 2], &[[f64; 4]])]) -> Layout {
        let mut layout = Layout {
            names: (1..=objects.len()).map(|k| format!("o{k}")).collect(),
            design: objects
                .iter()
                .map(|&(c, _)| Rect {
                    min: c,
                    max: [c[0] + 10.0, c[1] + 10.0],
                })
                .collect(),
            outlines: objects
                .iter()
                .map(|&(c, _)| {
                    vec![
                        c,
                        [c[0] + 10.0, c[1]],
                        [c[0] + 10.0, c[1] + 10.0],
                        [c[0], c[1] + 10.0],
                    ]
                })
                .collect(),
            instances: (0..objects.len())
                .map(|object| Instance {
                    object,
                    copy: 1,
                    offset: [0.0; 2],
                    turn_deg: 0.0,
                    group: 0,
                })
                .collect(),
            overlaps: Vec::new(),
            reaches: Vec::new(),
            cutters: objects
                .iter()
                .map(|&(c, boxes)| {
                    (1..)
                        .zip(boxes)
                        .map(|(hole, b)| {
                            // A cylinder along X whose box is `b`, within the tolerance.
                            let y = c[1] + f64::midpoint(b[1], b[3]);
                            Cylinder {
                                hole,
                                start: [c[0] + b[0], y, 1.0],
                                end: [c[0] + b[2], y, 1.0],
                                direction: [1.0, 0.0, 0.0],
                                radius: (b[3] - b[1]) / 2.0,
                            }
                        })
                        .collect()
                })
                .collect(),
            spacing_mm: 2.0,
            top_z: 1.0,
        };
        layout.group_cutters();
        layout
    }

    fn groups(layout: &Layout) -> Vec<usize> {
        layout.instances.iter().map(|i| i.group).collect()
    }

    fn reaches(layout: &Layout) -> Vec<(usize, usize, usize)> {
        layout
            .reaches
            .iter()
            .map(|r| (r.from, r.hole, r.to))
            .collect()
    }

    /// A cutter through the right side of its box, reaching 3 mm beyond it.
    const RIGHT: [f64; 4] = [8.0, 4.0, 13.0, 6.0];
    /// A cutter through the left side of its box, reaching 3 mm beyond it.
    const LEFT: [f64; 4] = [-3.0, 4.0, 2.0, 6.0];
    /// A cutter inside its box.
    const INSIDE: [f64; 4] = [4.0, 4.0, 6.0, 6.0];

    #[test]
    fn cutters_that_reach_each_other_are_grouped_apart() {
        // TD-166: four boxes in a row with 2 mm gaps. The first two reach each other, the third
        // is drilled within itself, the fourth is plain.
        let layout = layout(&[
            ([0.0, 0.0], &[INSIDE, RIGHT]),
            ([12.0, 0.0], &[LEFT]),
            ([24.0, 0.0], &[INSIDE]),
            ([36.0, 0.0], &[]),
        ]);
        assert_eq!(reaches(&layout), [(0, 2, 1), (1, 1, 0)]);
        assert_eq!(groups(&layout), [1, 2, 0, 0]);
    }

    #[test]
    fn cutters_that_reach_plain_models_share_a_group() {
        // The first reaches the plain second, the third the plain fourth: both cutters can be
        // in group 1, as neither reaches the other's box.
        let layout = layout(&[
            ([0.0, 0.0], &[RIGHT]),
            ([12.0, 0.0], &[]),
            ([24.0, 0.0], &[RIGHT]),
            ([36.0, 0.0], &[]),
        ]);
        assert_eq!(reaches(&layout), [(0, 1, 1), (2, 1, 3)]);
        assert_eq!(groups(&layout), [1, 0, 1, 0]);
        // A chain: the first reaches the second, which reaches the plain third.
        let chain = layout_chain();
        assert_eq!(groups(&chain), [1, 2, 0]);
    }

    fn layout_chain() -> Layout {
        layout(&[
            ([0.0, 0.0], &[RIGHT]),
            ([12.0, 0.0], &[RIGHT]),
            ([24.0, 0.0], &[]),
        ])
    }

    #[test]
    fn a_cutter_that_touches_another_box_reaches_it() {
        // Its box ends exactly where the next box starts.
        let touching = layout(&[([0.0, 0.0], &[[8.0, 4.0, 12.0, 6.0]]), ([12.0, 0.0], &[])]);
        assert_eq!(groups(&touching), [1, 0]);
        let short = layout(&[([0.0, 0.0], &[[8.0, 4.0, 11.9, 6.0]]), ([12.0, 0.0], &[])]);
        assert_eq!(reaches(&short), []);
        assert_eq!(groups(&short), [0, 0]);
    }

    #[test]
    fn a_turned_instance_turns_its_footprint_and_cutters_about_its_pivot() {
        // Issue #48: a 10 x 10 mm object with a cutter along X through its right side, turned a
        // quarter about its middle (5, 5) and moved 20 mm along X.
        let mut layout = layout(&[([0.0, 0.0], &[RIGHT])]);
        layout.instances[0].turn_deg = 90.0;
        layout.instances[0].offset = [20.0, 0.0];
        assert_eq!(
            layout.footprints()[0],
            Rect {
                min: [20.0, 0.0],
                max: [30.0, 10.0]
            }
        );
        // The cutter, from (8, 5) to (13, 5) with radius 1, now runs along Y at X 25.
        let cutter = layout.placed_cutter(&layout.instances[0], &layout.cutters[0][0]);
        let near = |a: [f64; 2], b: [f64; 2]| (0..2).all(|k| (a[k] - b[k]).abs() < 1e-9);
        assert!(near(cutter.min, [23.999, 7.999]), "{cutter:?}");
        assert!(near(cutter.max, [26.001, 13.001]), "{cutter:?}");
        // With no turn the instance only moves, as before.
        layout.instances[0].turn_deg = 0.0;
        let cutter = layout.placed_cutter(&layout.instances[0], &layout.cutters[0][0]);
        assert!(near(cutter.min, [27.999, 3.999]), "{cutter:?}");
    }

    #[test]
    fn strict_layers_refuse_the_first_reach() {
        assert_eq!(
            layout_chain().refuse_reaches().unwrap_err(),
            r#"drain hole 1 of object 1 ("o1") copy 1 reaches object 2 ("o2") copy 1, and strict layers cannot keep a drain hole to its own model; slice with union layers or move them apart"#
        );
        assert!(layout(&[([0.0, 0.0], &[INSIDE])]).refuse_reaches().is_ok());
    }
}
