//! Multiple models on one build plate (TD-065).
//!
//! A versioned `karasu-plate` JSON document lists objects. Each object has:
//! - its own model file and placement;
//! - optional hollowing, drain holes and supports;
//! - a number of copies.
//!
//! [`arrange`] packs footprints deterministically, and [`overlapping_pairs`] reports footprints
//! that overlap.
//!
//! The document only describes the plate. Reading models, preparing them and uniting them into
//! one sliced body belong to the caller (the CLI), which reuses the single-model pipeline per
//! object.
//!
//! **Footprints:**
//! - They are axis-aligned XY rectangles in world millimetres.
//! - They include everything an object prints: supports, raft and drain-hole cutters.
//! - Packing never rotates objects.
use crate::{
    mesh::formats::LengthUnit,
    preparation::{HoleTilt, MAX_HOLE_TILT_DEG},
};
use serde::Deserialize;
use std::fmt;

/// Document format name.
pub const FORMAT: &str = "karasu-plate";
/// Current document version. Version 2 (TD-089) adds drain holes at the object level, without
/// hollowing, and a surface hole's starting surface, fixed depth and tilt; version 1 documents
/// are still read.
pub const VERSION: u32 = 2;
/// Largest accepted document, in bytes.
pub const MAX_DOCUMENT_BYTES: usize = 1024 * 1024;
/// Most objects in one document.
pub const MAX_OBJECTS: usize = 256;
/// Most copies of one object.
pub const MAX_COPIES: u32 = 1000;
/// Most instances (objects times copies) on one plate.
pub const MAX_INSTANCES: usize = 4096;
/// Default gap between arranged footprints, in mm (owner decision, TD-065).
pub const DEFAULT_SPACING_MM: f64 = 2.0;
/// Largest accepted spacing, in mm.
const MAX_SPACING_MM: f64 = 100.0;

/// A plate document that cannot be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlateError {
    /// Malformed JSON or an unknown field.
    Json(String),
    /// A value that violates the schema, with the object index (1-based) when it has one.
    Invalid {
        /// 1-based object number, or `None` for the plate itself.
        object: Option<usize>,
        /// What is wrong.
        reason: String,
    },
}

impl fmt::Display for PlateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(e) => write!(f, "invalid plate: {e}"),
            Self::Invalid {
                object: Some(object),
                reason,
            } => write!(f, "invalid plate: object {object}: {reason}"),
            Self::Invalid {
                object: None,
                reason,
            } => write!(f, "invalid plate: {reason}"),
        }
    }
}

impl std::error::Error for PlateError {}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDrain {
    entry_mm: Option<[f64; 3]>,
    axis: Option<String>,
    direction: Option<[f64; 3]>,
    length_mm: Option<f64>,
    surface_mm: Option<[f64; 3]>,
    outside_mm: Option<f64>,
    inside_mm: Option<f64>,
    diameter_mm: f64,
    /// Version 2: `"outer"` (default) or `"cavity"`.
    surface: Option<String>,
    /// Version 2: a fixed depth from the surface, in mm.
    depth_mm: Option<f64>,
    /// Version 2: the tilt of the axis from the surface normal.
    tilt: Option<RawTilt>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTilt {
    angle_deg: f64,
    azimuth_deg: f64,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RawSimplify {
    Millimeters(f64),
    Keyword(String),
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawHollow {
    wall_mm: f64,
    cell_mm: Option<f64>,
    simplify_mm: Option<RawSimplify>,
    #[serde(default)]
    drain_holes: Vec<RawDrain>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(clippy::struct_excessive_bools)] // Independent JSON switches, as on the command line.
struct RawObject {
    model: String,
    name: Option<String>,
    units: Option<String>,
    #[serde(default)]
    rotate_deg: [f64; 3],
    #[serde(default = "unit_scale")]
    scale: [f64; 3],
    #[serde(default)]
    mirror: String,
    translate_mm: Option<[f64; 3]>,
    #[serde(default)]
    center: bool,
    #[serde(default)]
    drop: bool,
    lift_mm: Option<f64>,
    #[serde(default = "yes")]
    arrange: bool,
    #[serde(default = "one")]
    copies: u32,
    hollow: Option<RawHollow>,
    /// Version 2: drain holes independent of hollowing.
    #[serde(default)]
    drain_holes: Vec<RawDrain>,
    supports: Option<String>,
    #[serde(default)]
    accept_support_collisions: bool,
}

const fn unit_scale() -> [f64; 3] {
    [1.0; 3]
}
const fn yes() -> bool {
    true
}
const fn one() -> u32 {
    1
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPlate {
    format: String,
    version: u32,
    #[serde(default = "default_spacing")]
    spacing_mm: f64,
    objects: Vec<RawObject>,
}

const fn default_spacing() -> f64 {
    DEFAULT_SPACING_MM
}

/// A drain hole in the object's design frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DrainRequest {
    /// A cylinder from `entry_mm` along `direction` (an axis name or any vector, TD-066).
    Axis {
        /// Entry point, outside the model.
        entry_mm: [f64; 3],
        /// Nonzero direction.
        direction: [f64; 3],
        /// Cylinder length.
        length_mm: f64,
        /// Cylinder diameter.
        diameter_mm: f64,
    },
    /// Perpendicular to the surface nearest `surface_mm`, through the wall (TD-066, TD-089).
    Surface {
        /// A point on or near the surface.
        surface_mm: [f64; 3],
        /// Cylinder diameter.
        diameter_mm: f64,
        /// Entry distance outside the surface.
        outside_mm: f64,
        /// End distance past the cavity wall, with the automatic depth.
        inside_mm: f64,
        /// A fixed depth from the surface, or `None` to reach `inside_mm` past the cavity wall.
        depth_mm: Option<f64>,
        /// Whether the hole starts on the cavity's surface and goes out through the wall.
        from_cavity: bool,
        /// The tilt of the axis from the surface normal, measured in world coordinates (see
        /// [`HoleTilt`]), or `None` for a perpendicular hole.
        tilt: Option<HoleTilt>,
    },
}

/// Cavity simplification tolerance (TD-066).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SimplifyRequest {
    /// An explicit tolerance, in mm.
    Millimeters(f64),
    /// Half the smaller pixel pitch, at most half the layer height.
    Auto,
}

/// Hollowing of one object.
#[derive(Clone, Debug, PartialEq)]
pub struct HollowRequest {
    /// Wall thickness.
    pub wall_mm: f64,
    /// Explicit grid cell, or `None` for the default.
    pub cell_mm: Option<f64>,
    /// Certified cavity simplification, if requested.
    pub simplify: Option<SimplifyRequest>,
    /// Drain holes, in document order.
    pub drain_holes: Vec<DrainRequest>,
}

/// One object of a plate, with its copies.
///
/// Its **design placement** is `translate_mm`, `center`, `drop` and `lift_mm`, after rotation,
/// scale and mirroring, as for a single model:
/// - Supports documents and drain holes are written in the design placement's world coordinates.
/// - An arranged object, and each of its copies, is then moved rigidly in XY with everything it
///   prints.
#[derive(Clone, Debug, PartialEq)]
#[allow(clippy::struct_excessive_bools)] // Independent switches, as on the command line.
pub struct PlateObject {
    /// Model path as written; relative paths are relative to the plate document.
    pub model: String,
    /// Display name (default: the model path).
    pub name: String,
    /// Unit for model formats that declare none.
    pub units: Option<LengthUnit>,
    /// Fixed-axis X, then Y, then Z rotation, in degrees.
    pub rotate_deg: [f64; 3],
    /// Positive per-axis scale.
    pub scale: [f64; 3],
    /// Mirrored axes.
    pub mirror: [bool; 3],
    /// Explicit design translation (default zero).
    pub translate_mm: [f64; 3],
    /// Centre the design placement on the plate.
    pub center: bool,
    /// Rest the design placement on Z = 0.
    pub drop: bool,
    /// With `drop`, the lift above the plate (requires supports).
    pub lift_mm: Option<f64>,
    /// Whether the arranger places this object's copies; otherwise it is a fixed obstacle.
    pub arrange: bool,
    /// Number of instances (at least one).
    pub copies: u32,
    /// Hollowing and its drain holes (version 1 places them here).
    pub hollow: Option<HollowRequest>,
    /// Drain holes independent of hollowing (version 2), after the hollowing's own.
    pub drain_holes: Vec<DrainRequest>,
    /// Supports document path, relative to the plate document.
    pub supports: Option<String>,
    /// Accept every support collision (still reported).
    pub accept_support_collisions: bool,
}

/// A validated `karasu-plate` document.
#[derive(Clone, Debug, PartialEq)]
pub struct PlateDocument {
    /// Gap between arranged footprints, in mm.
    pub spacing_mm: f64,
    /// Objects in document order.
    pub objects: Vec<PlateObject>,
}

fn plate_error(reason: impl Into<String>) -> PlateError {
    PlateError::Invalid {
        object: None,
        reason: reason.into(),
    }
}

fn finite(values: &[f64]) -> bool {
    values.iter().all(|v| v.is_finite())
}

fn axis(text: &str) -> Option<[f64; 3]> {
    Some(match text {
        "+x" | "x" => [1., 0., 0.],
        "-x" => [-1., 0., 0.],
        "+y" | "y" => [0., 1., 0.],
        "-y" => [0., -1., 0.],
        "+z" | "z" => [0., 0., 1.],
        "-z" => [0., 0., -1.],
        _ => return None,
    })
}

/// The axes named in `text` (each of x, y and z at most once), or `None`.
pub(crate) fn mirror_axes(text: &str) -> Option<[bool; 3]> {
    let mut axes = [false; 3];
    for c in text.chars() {
        let slot = &mut axes[match c {
            'x' => 0,
            'y' => 1,
            'z' => 2,
            _ => return None,
        }];
        if *slot {
            return None;
        }
        *slot = true;
    }
    Some(axes)
}

fn positive(value: f64) -> bool {
    value.is_finite() && value > 0.
}

/// One drain hole: either `entry_mm` with `axis` or `direction` and `length_mm`, or `surface_mm`
/// with optional `outside_mm`, `inside_mm` and, in version 2, `surface`, `depth_mm` and `tilt`.
fn drain_request(d: &RawDrain, version: u32) -> Result<DrainRequest, String> {
    if !positive(d.diameter_mm) {
        return Err("drain hole diameter_mm must be positive".into());
    }
    if version < 2 && (d.surface.is_some() || d.depth_mm.is_some() || d.tilt.is_some()) {
        return Err("drain hole surface, depth_mm and tilt need document version 2".into());
    }
    match (d.entry_mm, d.surface_mm) {
        (Some(entry_mm), None)
            if d.outside_mm.is_none()
                && d.inside_mm.is_none()
                && d.surface.is_none()
                && d.depth_mm.is_none()
                && d.tilt.is_none() =>
        {
            let direction = match (&d.axis, d.direction) {
                (Some(name), None) => {
                    axis(name).ok_or("drain hole axis must be one of +x -x +y -y +z -z")?
                }
                (None, Some(v)) if finite(&v) && v.iter().any(|&c| c != 0.0) => v,
                _ => {
                    return Err(
                        "drain hole needs exactly one of axis and a nonzero finite direction"
                            .into(),
                    )
                }
            };
            match d.length_mm.filter(|&l| positive(l)) {
                Some(length_mm) if finite(&entry_mm) => Ok(DrainRequest::Axis {
                    entry_mm,
                    direction,
                    length_mm,
                    diameter_mm: d.diameter_mm,
                }),
                _ => Err("drain holes need a finite entry and a positive length_mm".into()),
            }
        }
        (None, Some(surface_mm))
            if d.axis.is_none() && d.direction.is_none() && d.length_mm.is_none() =>
        {
            let margin = |v: Option<f64>| v.unwrap_or(crate::preparation::SURFACE_HOLE_MARGIN_MM);
            let (outside_mm, inside_mm) = (margin(d.outside_mm), margin(d.inside_mm));
            if !finite(&surface_mm) || !positive(outside_mm) || !positive(inside_mm) {
                return Err("surface holes need a finite point and positive margins".into());
            }
            if d.depth_mm.is_some_and(|v| !positive(v)) {
                return Err("drain hole depth_mm must be positive".into());
            }
            let tilt = match &d.tilt {
                None => None,
                Some(t)
                    if t.angle_deg.is_finite()
                        && (0.0..=MAX_HOLE_TILT_DEG).contains(&t.angle_deg)
                        && t.azimuth_deg.is_finite()
                        && t.azimuth_deg.abs() <= 360.0 =>
                {
                    // No tilt is a perpendicular hole, recorded as such.
                    (t.angle_deg > 0.0).then_some(HoleTilt {
                        angle_deg: t.angle_deg,
                        azimuth_deg: t.azimuth_deg,
                    })
                }
                Some(_) => {
                    return Err(format!(
                        "drain hole tilt needs angle_deg from 0 to {MAX_HOLE_TILT_DEG} and \
                         azimuth_deg from -360 to 360"
                    ))
                }
            };
            let from_cavity = match d.surface.as_deref() {
                None | Some("outer") => false,
                Some("cavity") => true,
                Some(_) => return Err("drain hole surface must be \"outer\" or \"cavity\"".into()),
            };
            Ok(DrainRequest::Surface {
                surface_mm,
                diameter_mm: d.diameter_mm,
                outside_mm,
                inside_mm,
                depth_mm: d.depth_mm,
                from_cavity,
                tilt,
            })
        }
        _ => Err(
            "a drain hole is either entry_mm with axis or direction and length_mm, or \
             surface_mm with optional outside_mm and inside_mm"
                .into(),
        ),
    }
}

impl PlateObject {
    #[allow(clippy::too_many_lines)] // One linear pass of field checks.
    fn from_raw(raw: RawObject, version: u32) -> Result<Self, String> {
        if raw.model.is_empty() {
            return Err("model must name a file".into());
        }
        let units = raw
            .units
            .as_deref()
            .map(|u| LengthUnit::parse(u).ok_or_else(|| format!("unknown units {u:?}")))
            .transpose()?;
        if !finite(&raw.rotate_deg) {
            return Err("rotate_deg must be finite".into());
        }
        if !raw.scale.iter().all(|&s| positive(s)) {
            return Err("scale factors must be positive and finite".into());
        }
        let mirror = mirror_axes(&raw.mirror)
            .ok_or("mirror must list distinct axes among x, y and z, such as \"xy\"")?;
        if raw.translate_mm.is_some_and(|t| !finite(&t)) {
            return Err("translate_mm must be finite".into());
        }
        if raw.translate_mm.is_some() && (raw.center || raw.drop) {
            return Err("translate_mm cannot be combined with center or drop".into());
        }
        if raw.center && raw.arrange {
            return Err("center places a fixed object; give \"arrange\": false with it".into());
        }
        if let Some(lift) = raw.lift_mm {
            if !(lift.is_finite() && lift >= 0.) {
                return Err("lift_mm must be zero or positive".into());
            }
            if !raw.drop {
                return Err("lift_mm requires drop".into());
            }
            if lift > 0. && raw.supports.is_none() {
                return Err(
                    "lift_mm raises the model off the plate, which requires supports".into(),
                );
            }
        }
        if !(1..=MAX_COPIES).contains(&raw.copies) {
            return Err(format!("copies must be between 1 and {MAX_COPIES}"));
        }
        if raw.copies > 1 && !raw.arrange {
            return Err("copies are placed by the arranger; they need \"arrange\": true".into());
        }
        if raw.accept_support_collisions && raw.supports.is_none() {
            return Err("accept_support_collisions requires supports".into());
        }
        if raw.supports.as_deref() == Some("") {
            return Err("supports must name a file".into());
        }
        let hollow = raw
            .hollow
            .map(|h| -> Result<HollowRequest, String> {
                if !positive(h.wall_mm) || h.cell_mm.is_some_and(|c| !positive(c)) {
                    return Err("hollow wall_mm and cell_mm must be positive".into());
                }
                let drain_holes = h
                    .drain_holes
                    .iter()
                    .map(|d| drain_request(d, version))
                    .collect::<Result<_, String>>()?;
                let simplify = match h.simplify_mm {
                    None => None,
                    Some(RawSimplify::Millimeters(mm)) if positive(mm) => {
                        Some(SimplifyRequest::Millimeters(mm))
                    }
                    Some(RawSimplify::Keyword(k)) if k == "auto" => Some(SimplifyRequest::Auto),
                    Some(_) => {
                        return Err(
                            "hollow simplify_mm must be a positive tolerance or \"auto\"".into(),
                        )
                    }
                };
                Ok(HollowRequest {
                    wall_mm: h.wall_mm,
                    cell_mm: h.cell_mm,
                    simplify,
                    drain_holes,
                })
            })
            .transpose()?;
        if version < 2 && !raw.drain_holes.is_empty() {
            return Err("drain holes outside hollow need document version 2".into());
        }
        let drain_holes: Vec<DrainRequest> = raw
            .drain_holes
            .iter()
            .map(|d| drain_request(d, version))
            .collect::<Result<_, String>>()?;
        // TD-089: holes need no hollowing, except those that reach or start from its cavity.
        let needs_cavity = |d: &DrainRequest| {
            matches!(
                d,
                DrainRequest::Surface { depth_mm: None, .. }
                    | DrainRequest::Surface {
                        from_cavity: true,
                        ..
                    }
            )
        };
        if hollow.is_none() && drain_holes.iter().any(needs_cavity) {
            return Err("a surface hole from the cavity, or without depth_mm, needs hollow".into());
        }
        Ok(Self {
            name: raw.name.unwrap_or_else(|| raw.model.clone()),
            model: raw.model,
            units,
            rotate_deg: raw.rotate_deg,
            scale: raw.scale,
            mirror,
            translate_mm: raw.translate_mm.unwrap_or([0.; 3]),
            center: raw.center,
            drop: raw.drop,
            lift_mm: raw.lift_mm,
            arrange: raw.arrange,
            copies: raw.copies,
            hollow,
            drain_holes,
            supports: raw.supports,
            accept_support_collisions: raw.accept_support_collisions,
        })
    }
}

impl PlateDocument {
    /// Parse and validate a `karasu-plate` document.
    ///
    /// # Errors
    /// An oversized document, malformed JSON, unknown fields, a wrong format or version, no
    /// objects or too many, and invalid object values (with the object number).
    pub fn from_json(bytes: &[u8]) -> Result<Self, PlateError> {
        if bytes.len() > MAX_DOCUMENT_BYTES {
            return Err(plate_error(format!(
                "document exceeds {MAX_DOCUMENT_BYTES} bytes"
            )));
        }
        let raw: RawPlate =
            serde_json::from_slice(bytes).map_err(|e| PlateError::Json(e.to_string()))?;
        if raw.format != FORMAT || !(1..=VERSION).contains(&raw.version) {
            return Err(plate_error(format!(
                "document is {:?} version {}; expected {FORMAT:?} version 1 to {VERSION}",
                raw.format, raw.version
            )));
        }
        let version = raw.version;
        if !(raw.spacing_mm.is_finite() && (0. ..=MAX_SPACING_MM).contains(&raw.spacing_mm)) {
            return Err(plate_error(format!(
                "spacing_mm must be between 0 and {MAX_SPACING_MM}"
            )));
        }
        if raw.objects.is_empty() || raw.objects.len() > MAX_OBJECTS {
            return Err(plate_error(format!(
                "between 1 and {MAX_OBJECTS} objects are required"
            )));
        }
        let objects = raw
            .objects
            .into_iter()
            .enumerate()
            .map(|(i, o)| {
                PlateObject::from_raw(o, version).map_err(|reason| PlateError::Invalid {
                    object: Some(i + 1),
                    reason,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let instances: usize = objects.iter().map(|o| o.copies as usize).sum();
        if instances > MAX_INSTANCES {
            return Err(plate_error(format!(
                "{instances} instances exceed the limit of {MAX_INSTANCES}"
            )));
        }
        Ok(Self {
            spacing_mm: raw.spacing_mm,
            objects,
        })
    }
}

/// An axis-aligned XY rectangle in world millimetres.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    /// Lowest X and Y.
    pub min: [f64; 2],
    /// Highest X and Y.
    pub max: [f64; 2],
}

impl Rect {
    /// Width and depth.
    #[must_use]
    pub fn size(&self) -> [f64; 2] {
        [self.max[0] - self.min[0], self.max[1] - self.min[1]]
    }

    /// Whether the two rectangles are closer than `gap` on both axes (strictly overlapping
    /// interiors when `gap` is zero; touching edges do not overlap).
    #[must_use]
    pub fn within(&self, other: &Self, gap: f64) -> bool {
        (0..2).all(|k| self.min[k] < other.max[k] + gap && other.min[k] < self.max[k] + gap)
    }
}

/// Footprints that do not fit on the plate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unplaced {
    /// Indices of the footprints left out, increasing.
    pub items: Vec<usize>,
}

impl fmt::Display for Unplaced {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} footprint(s) do not fit on the plate",
            self.items.len()
        )
    }
}

impl std::error::Error for Unplaced {}

/// Pack footprints of the given `sizes` (width, depth) into `area`, at least `spacing_mm` from
/// each other and from every `obstacle`. Returns each footprint's lowest corner, in input order.
///
/// **Algorithm:** centre-out placement (owner, 2026-11-10): the plate's middle first, the group
/// growing outwards in a grid.
/// - **Order:** footprints are taken by decreasing area, then decreasing depth, then decreasing
///   width, then input order.
/// - **Candidates:** for each footprint, the corner that centres it on the plate, and those that
///   put it beside a footprint already placed or an obstacle: one spacing beyond one of its four
///   sides, flush with one of that side's ends or centred on it; inside the plate.
/// - **Choice:** the candidate whose centre is nearest the plate's middle (then the lowest Y,
///   then X) that meets nothing, one spacing clear; rectangles are looked up in a grid of cells
///   as large as the largest footprint, so each test sees only its neighbours.
/// - **Fallback:** when some footprints do not fit, next-fit shelf packing (`shelves`) is tried
///   too, and taken when it leaves fewer out: a plate never holds fewer than shelves fit.
///
/// The arithmetic is plain `f64` in a fixed order, so equal inputs give bitwise-equal results
/// on every platform.
///
/// # Errors
/// [`Unplaced`] lists every footprint that does not fit; nothing is placed outside `area`.
pub fn arrange(
    sizes: &[[f64; 2]],
    area: Rect,
    obstacles: &[Rect],
    spacing_mm: f64,
) -> Result<Vec<[f64; 2]>, Unplaced> {
    let mut order: Vec<usize> = (0..sizes.len()).collect();
    order.sort_by(|&a, &b| {
        let area_of = |k: usize| sizes[k][0] * sizes[k][1];
        area_of(b)
            .total_cmp(&area_of(a))
            .then(sizes[b][1].total_cmp(&sizes[a][1]))
            .then(sizes[b][0].total_cmp(&sizes[a][0]))
            .then(a.cmp(&b))
    });
    let middle: [f64; 2] = std::array::from_fn(|k| f64::midpoint(area.min[k], area.max[k]));
    let mut corners = vec![[0.; 2]; sizes.len()];
    let mut unplaced = Vec::new();
    // Cells as large as the largest footprint and its spacing: a candidate meets only what lies
    // in the few cells it covers.
    let largest = sizes.iter().flatten().copied().fold(1.0, f64::max);
    let mut taken = Taken::new(largest + spacing_mm, spacing_mm);
    for obstacle in obstacles {
        taken.add(*obstacle);
    }
    for &i in &order {
        match nearest_free(sizes[i], area, &taken, middle) {
            Some(corner) => {
                corners[i] = corner;
                taken.add(Rect {
                    min: corner,
                    max: [corner[0] + sizes[i][0], corner[1] + sizes[i][1]],
                });
            }
            None => unplaced.push(i),
        }
    }
    unplaced.sort_unstable();
    if !unplaced.is_empty() {
        // Close packing fits more on a tight plate: taken when it leaves fewer out.
        let (packed, left) = shelves(sizes, area, obstacles, spacing_mm);
        if left.len() < unplaced.len() {
            (corners, unplaced) = (packed, left);
        }
    }
    if unplaced.is_empty() {
        Ok(corners)
    } else {
        Err(Unplaced { items: unplaced })
    }
}

/// The shelf packing [`arrange`] falls back on when the middle-first placement leaves some
/// footprints out: next-fit shelves, by decreasing depth, then width, then input order, each
/// filling from the lowest X; a footprint that would meet an obstacle moves past its right
/// edge, one past the plate's right edge opens the next shelf one spacing above the deepest
/// footprint of the current one, and a shelf still empty when the obstacles leave it no room
/// starts again one spacing above the lowest top of those it met. Without obstacles the group
/// is centred on the area. Returns the corners and the footprints left out, increasing.
fn shelves(
    sizes: &[[f64; 2]],
    area: Rect,
    obstacles: &[Rect],
    spacing_mm: f64,
) -> (Vec<[f64; 2]>, Vec<usize>) {
    let mut order: Vec<usize> = (0..sizes.len()).collect();
    order.sort_by(|&a, &b| {
        sizes[b][1]
            .total_cmp(&sizes[a][1])
            .then(sizes[b][0].total_cmp(&sizes[a][0]))
            .then(a.cmp(&b))
    });
    let mut corners = vec![[0.; 2]; sizes.len()];
    let mut unplaced = Vec::new();
    let (mut x, mut shelf_y, mut shelf_depth) = (area.min[0], area.min[1], 0f64);
    // The lowest top, one spacing above, of the obstacles met on the current shelf.
    let mut shelf_block = f64::INFINITY;
    let mut placed: Vec<Rect> = Vec::new();
    for &i in &order {
        let [width, depth] = sizes[i];
        loop {
            if x + width > area.max[0] {
                if shelf_depth == 0. {
                    if shelf_block.is_finite() {
                        // An empty shelf the obstacles fill: above the lowest of them. Each
                        // obstacle met reaches above the shelf's bottom, so this rises.
                        shelf_y = shelf_block;
                        x = area.min[0];
                        shelf_block = f64::INFINITY;
                        continue;
                    }
                    // Too wide even for an empty shelf.
                    unplaced.push(i);
                    break;
                }
                shelf_y += shelf_depth + spacing_mm;
                x = area.min[0];
                shelf_depth = 0.;
                shelf_block = f64::INFINITY;
                continue;
            }
            if shelf_y + depth > area.max[1] {
                unplaced.push(i);
                break;
            }
            let candidate = Rect {
                min: [x, shelf_y],
                max: [x + width, shelf_y + depth],
            };
            let blocker = obstacles
                .iter()
                .filter(|o| candidate.within(o, spacing_mm))
                .fold(f64::NEG_INFINITY, |right, o| {
                    shelf_block = shelf_block.min(o.max[1] + spacing_mm);
                    right.max(o.max[0])
                });
            if blocker > f64::NEG_INFINITY {
                x = blocker + spacing_mm;
                continue;
            }
            corners[i] = candidate.min;
            placed.push(candidate);
            x += width + spacing_mm;
            shelf_depth = shelf_depth.max(depth);
            break;
        }
    }
    unplaced.sort_unstable();
    // Centred without obstacles, when everything fits (else the fallback is not taken).
    if unplaced.is_empty() && obstacles.is_empty() && !placed.is_empty() {
        let low = placed.iter().fold([f64::INFINITY; 2], |m, r| {
            [m[0].min(r.min[0]), m[1].min(r.min[1])]
        });
        let high = placed.iter().fold([f64::NEG_INFINITY; 2], |m, r| {
            [m[0].max(r.max[0]), m[1].max(r.max[1])]
        });
        let shift: [f64; 2] = std::array::from_fn(|k| {
            f64::midpoint(area.min[k], area.max[k]) - f64::midpoint(low[k], high[k])
        });
        for corner in &mut corners {
            *corner = [corner[0] + shift[0], corner[1] + shift[1]];
        }
    }
    (corners, unplaced)
}

/// The rectangles placed so far (obstacles included), indexed by the grid cells they cover.
struct Taken {
    rects: Vec<Rect>,
    cells: std::collections::HashMap<(i64, i64), Vec<usize>>,
    /// A cell's side, in mm.
    side: f64,
    /// The spacing kept from every rectangle.
    spacing: f64,
}

impl Taken {
    fn new(side: f64, spacing: f64) -> Self {
        Self {
            rects: Vec::new(),
            cells: std::collections::HashMap::new(),
            side,
            spacing,
        }
    }

    /// The cells a box from `min` to `max` covers.
    #[allow(clippy::cast_possible_truncation)] // Plate millimetres over cells of a millimetre or more.
    fn cells(&self, min: [f64; 2], max: [f64; 2]) -> impl Iterator<Item = (i64, i64)> {
        let at = |v: f64| (v / self.side).floor() as i64;
        let (x0, x1, y0, y1) = (at(min[0]), at(max[0]), at(min[1]), at(max[1]));
        (x0..=x1).flat_map(move |x| (y0..=y1).map(move |y| (x, y)))
    }

    fn add(&mut self, rect: Rect) {
        let k = self.rects.len();
        let grow = self.spacing;
        let cells: Vec<_> = self
            .cells(
                [rect.min[0] - grow, rect.min[1] - grow],
                [rect.max[0] + grow, rect.max[1] + grow],
            )
            .collect();
        for cell in cells {
            self.cells.entry(cell).or_default().push(k);
        }
        self.rects.push(rect);
    }

    /// Whether `candidate` comes within the spacing of a rectangle taken.
    fn meets(&self, candidate: &Rect) -> bool {
        self.cells(candidate.min, candidate.max).any(|cell| {
            self.cells.get(&cell).is_some_and(|ks| {
                ks.iter()
                    .any(|&k| candidate.within(&self.rects[k], self.spacing))
            })
        })
    }
}

/// The lowest corner, among the candidates of [`arrange`], of a footprint of `size` in `area`
/// nearest `middle` that keeps the spacing from every rectangle `taken`; `None` when none does.
fn nearest_free(size: [f64; 2], area: Rect, taken: &Taken, middle: [f64; 2]) -> Option<[f64; 2]> {
    let spacing = taken.spacing;
    let [w, d] = size;
    let mut candidates = vec![[middle[0] - w / 2.0, middle[1] - d / 2.0]];
    for r in &taken.rects {
        let mid = [
            f64::midpoint(r.min[0], r.max[0]),
            f64::midpoint(r.min[1], r.max[1]),
        ];
        // Beside it, flush with its edges or centred on it.
        for y in [r.min[1], r.max[1] - d, mid[1] - d / 2.0] {
            candidates.push([r.max[0] + spacing, y]);
            candidates.push([r.min[0] - spacing - w, y]);
        }
        for x in [r.min[0], r.max[0] - w, mid[0] - w / 2.0] {
            candidates.push([x, r.max[1] + spacing]);
            candidates.push([x, r.min[1] - spacing - d]);
        }
    }
    candidates.retain(|c| (0..2).all(|k| c[k] >= area.min[k] && c[k] + size[k] <= area.max[k]));
    let distance = |c: &[f64; 2]| {
        let [dx, dy] = [c[0] + w / 2.0 - middle[0], c[1] + d / 2.0 - middle[1]];
        dx * dx + dy * dy
    };
    candidates.sort_by(|a, b| {
        distance(a)
            .total_cmp(&distance(b))
            .then(a[1].total_cmp(&b[1]))
            .then(a[0].total_cmp(&b[0]))
    });
    candidates.into_iter().find(|&corner| {
        !taken.meets(&Rect {
            min: corner,
            max: [corner[0] + w, corner[1] + d],
        })
    })
}

/// Pairs `(i, j)`, `i < j`, of rectangles whose interiors overlap, in increasing order.
#[must_use]
pub fn overlapping_pairs(rects: &[Rect]) -> Vec<(usize, usize)> {
    let mut pairs = Vec::new();
    for (i, a) in rects.iter().enumerate() {
        for (j, b) in rects.iter().enumerate().skip(i + 1) {
            if a.within(b, 0.) {
                pairs.push((i, j));
            }
        }
    }
    pairs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn document(body: &str) -> Result<PlateDocument, PlateError> {
        PlateDocument::from_json(
            format!(r#"{{"format":"karasu-plate","version":1,{body}}}"#).as_bytes(),
        )
    }

    #[test]
    fn version_2_takes_holes_without_hollowing_and_from_the_cavity() {
        // TD-089: object-level holes, a starting surface and a fixed depth.
        let v2 = |body: &str| {
            PlateDocument::from_json(
                format!(r#"{{"format":"karasu-plate","version":2,{body}}}"#).as_bytes(),
            )
        };
        let plate = v2(r#""objects":[{"model":"a.stl","drain_holes":[
                {"entry_mm":[0,0,-1],"axis":"+z","length_mm":4,"diameter_mm":3},
                {"surface_mm":[5,5,10],"diameter_mm":2,"depth_mm":3}]},
              {"model":"b.stl","hollow":{"wall_mm":2},"drain_holes":[
                {"surface_mm":[5,5,3],"diameter_mm":2,"surface":"cavity","depth_mm":3,
                 "tilt":{"angle_deg":30,"azimuth_deg":90}}]}]"#)
        .unwrap();
        assert_eq!(plate.objects[0].drain_holes.len(), 2);
        assert!(matches!(
            plate.objects[0].drain_holes[1],
            DrainRequest::Surface {
                depth_mm: Some(3.),
                from_cavity: false,
                ..
            }
        ));
        assert!(matches!(
            plate.objects[1].drain_holes[0],
            DrainRequest::Surface {
                from_cavity: true,
                tilt: Some(HoleTilt {
                    angle_deg: 30.,
                    azimuth_deg: 90.
                }),
                ..
            }
        ));
        // A hole that reaches or starts from the cavity needs hollowing; version 1 knows none
        // of this; an unknown surface and later versions are refused.
        for (document, expected) in [
            (
                v2(
                    r#""objects":[{"model":"a","drain_holes":[{"surface_mm":[0,0,0],"diameter_mm":2}]}]"#,
                ),
                "needs hollow",
            ),
            (
                v2(
                    r#""objects":[{"model":"a","hollow":{"wall_mm":2},"drain_holes":[{"surface_mm":[0,0,0],"diameter_mm":2,"surface":"inside","depth_mm":1}]}]"#,
                ),
                "outer",
            ),
            (
                document(
                    r#""objects":[{"model":"a","drain_holes":[{"entry_mm":[0,0,0],"axis":"+z","length_mm":1,"diameter_mm":1}]}]"#,
                ),
                "version 2",
            ),
            (
                v2(
                    r#""objects":[{"model":"a","drain_holes":[{"surface_mm":[0,0,0],"diameter_mm":2,"depth_mm":1,"tilt":{"angle_deg":75,"azimuth_deg":0}}]}]"#,
                ),
                "angle_deg from 0 to 60",
            ),
            (
                v2(
                    r#""objects":[{"model":"a","drain_holes":[{"entry_mm":[0,0,0],"axis":"+z","length_mm":1,"diameter_mm":1,"tilt":{"angle_deg":5,"azimuth_deg":0}}]}]"#,
                ),
                "either entry_mm",
            ),
            (
                PlateDocument::from_json(
                    br#"{"format":"karasu-plate","version":3,"objects":[{"model":"a"}]}"#,
                ),
                "version 1 to 2",
            ),
        ] {
            let error = document.unwrap_err().to_string();
            assert!(error.contains(expected), "{error}");
        }
    }

    #[test]
    fn documents_resolve_defaults_and_refuse_contradictions() {
        let plate = document(
            r#""objects":[{"model":"a.stl","units":"mm","drop":true,"copies":3},
                {"model":"b.3mf","name":"B","translate_mm":[10,20,0],"arrange":false,"mirror":"xz",
                 "rotate_deg":[0,0,90],"supports":"b.json"},
                {"model":"c.stl","drop":true,"hollow":{"wall_mm":2,"drain_holes":
                  [{"entry_mm":[0,0,-1],"axis":"+z","length_mm":4,"diameter_mm":3}]}}]"#,
        )
        .unwrap();
        assert_eq!(plate.spacing_mm, 2.0);
        let [a, b, c] = &plate.objects[..] else {
            panic!("three objects")
        };
        assert_eq!((a.name.as_str(), a.copies, a.arrange), ("a.stl", 3, true));
        assert_eq!(a.units, Some(LengthUnit::Millimeter));
        assert_eq!(b.mirror, [true, false, true]);
        assert_eq!(b.translate_mm, [10., 20., 0.]);
        assert_eq!(b.supports.as_deref(), Some("b.json"));
        let hollow = c.hollow.as_ref().unwrap();
        assert!(matches!(
            hollow.drain_holes[0],
            DrainRequest::Axis {
                direction: [0., 0., 1.],
                ..
            }
        ));
        for (body, expected) in [
            (r#""objects":[]"#, "between 1 and"),
            (r#""spacing_mm":-1,"objects":[{"model":"a"}]"#, "spacing_mm"),
            (
                r#""objects":[{"model":"a","copies":0}]"#,
                "object 1: copies",
            ),
            (
                r#""objects":[{"model":"a","copies":2,"arrange":false}]"#,
                "need \"arrange\"",
            ),
            (
                r#""objects":[{"model":"a","translate_mm":[0,0,0],"drop":true}]"#,
                "cannot be combined",
            ),
            (
                r#""objects":[{"model":"a","center":true}]"#,
                "\"arrange\": false",
            ),
            (
                r#""objects":[{"model":"a","drop":true,"lift_mm":2}]"#,
                "requires supports",
            ),
            (
                r#""objects":[{"model":"a","mirror":"xx"}]"#,
                "distinct axes",
            ),
            (
                r#""objects":[{"model":"a","units":"parsec"}]"#,
                "unknown units",
            ),
            (
                r#""objects":[{"model":"a","hollow":{"wall_mm":2,"drain_holes":[{"entry_mm":[0,0,0],"axis":"up","length_mm":1,"diameter_mm":1}]}}]"#,
                "axis",
            ),
            (
                r#""objects":[{"model":"a","hollow":{"wall_mm":2,"drain_holes":[{"entry_mm":[0,0,0],"surface_mm":[0,0,0],"length_mm":1,"diameter_mm":1}]}}]"#,
                "either entry_mm",
            ),
            (
                r#""objects":[{"model":"a","hollow":{"wall_mm":2,"simplify_mm":"fine"}}]"#,
                "simplify_mm",
            ),
            (
                r#""objects":[{"model":"a","colour":"red"}]"#,
                "unknown field",
            ),
            (
                r#""objects":[{"model":"a","copies":1000},{"model":"b","copies":1000},{"model":"c","copies":1000},{"model":"d","copies":1000},{"model":"e","copies":1000}]"#,
                "instances exceed",
            ),
        ] {
            let error = document(body).unwrap_err().to_string();
            assert!(error.contains(expected), "{body}: {error}");
        }
    }

    const PLATE: Rect = Rect {
        min: [0., 0.],
        max: [100., 60.],
    };

    /// The rectangles of `sizes` at `corners`.
    fn placed(sizes: &[[f64; 2]], corners: &[[f64; 2]]) -> Vec<Rect> {
        corners
            .iter()
            .zip(sizes)
            .map(|(c, s)| Rect {
                min: *c,
                max: [c[0] + s[0], c[1] + s[1]],
            })
            .collect()
    }

    /// Whether every rectangle keeps `gap` from the others and the `obstacles`, inside `PLATE`.
    fn apart(rects: &[Rect], obstacles: &[Rect], gap: f64) -> bool {
        rects.iter().enumerate().all(|(i, a)| {
            (0..2).all(|k| a.min[k] >= PLATE.min[k] && a.max[k] <= PLATE.max[k])
                && rects.iter().skip(i + 1).all(|b| !a.within(b, gap))
                && obstacles.iter().all(|o| !a.within(o, gap))
        })
    }

    #[test]
    fn the_middle_comes_first_and_the_group_grows_outwards_in_a_grid() {
        // One footprint: in the middle of the plate.
        assert_eq!(
            arrange(&[[20., 10.]], PLATE, &[], 2.).unwrap(),
            [[40., 25.]]
        );
        // Three alike: a row through the middle.
        let sizes = [[20., 20.]; 3];
        let corners = arrange(&sizes, PLATE, &[], 2.).unwrap();
        assert_eq!(corners, [[40., 20.], [18., 20.], [62., 20.]]);
        // Six alike on a plate 60 mm deep: the middle first leaves room for three only, so the
        // shelves take them all, the group centred.
        let sizes = [[20., 20.]; 6];
        let corners = arrange(&sizes, PLATE, &[], 2.).unwrap();
        let rects = placed(&sizes, &corners);
        assert!(apart(&rects, &[], 2.));
        let low = [0, 1].map(|k| rects.iter().map(|r| r.min[k]).fold(f64::INFINITY, f64::min));
        let high = [0, 1].map(|k| {
            rects
                .iter()
                .map(|r| r.max[k])
                .fold(f64::NEG_INFINITY, f64::max)
        });
        for k in 0..2 {
            let mid = f64::midpoint(PLATE.min[k], PLATE.max[k]);
            assert!((f64::midpoint(low[k], high[k]) - mid).abs() < 1e-9);
        }
        // The largest first, in the middle; the rest around it.
        let sizes = [[10., 10.], [30., 20.], [10., 10.]];
        let corners = arrange(&sizes, PLATE, &[], 2.).unwrap();
        assert_eq!(corners[1], [35., 20.]);
        assert!(apart(&placed(&sizes, &corners), &[], 2.));
        // Identical inputs give identical bits.
        assert_eq!(arrange(&sizes, PLATE, &[], 2.).unwrap(), corners);
    }

    #[test]
    fn obstacles_are_kept_clear_and_overflow_is_reported() {
        // An obstacle in the middle: the footprints close around it, the spacing clear.
        let middle = Rect {
            min: [35., 20.],
            max: [65., 40.],
        };
        let sizes = [[20., 10.]; 4];
        let corners = arrange(&sizes, PLATE, &[middle], 2.).unwrap();
        let rects = placed(&sizes, &corners);
        assert!(apart(&rects, &[middle], 2.));
        // Each touches the obstacle's spacing on one side: none far away.
        for r in &rects {
            let gap = [0, 1]
                .map(|k| (r.min[k] - middle.max[k]).max(middle.min[k] - r.max[k]))
                .into_iter()
                .fold(f64::NEG_INFINITY, f64::max);
            assert!((gap - 2.).abs() < 1e-9, "{r:?}");
        }
        // An obstacle in a corner: the footprint still goes to the middle.
        let corner = Rect {
            min: [0., 0.],
            max: [20., 20.],
        };
        assert_eq!(
            arrange(&[[20., 10.]], PLATE, &[corner], 2.).unwrap(),
            [[40., 25.]]
        );
        // Fifteen 30 x 30 footprints cannot fit 100 x 60 mm: three, the rest reported.
        let error = arrange(&[[30., 30.]; 15], PLATE, &[], 2.).unwrap_err();
        assert_eq!(error.items.len(), 12);
        // Wider than the plate, or than the room an obstacle leaves.
        assert_eq!(
            arrange(&[[120., 5.]], PLATE, &[], 2.).unwrap_err().items,
            [0]
        );
        let across = Rect {
            min: [20., 0.],
            max: [60., 30.],
        };
        assert_eq!(
            arrange(&[[70., 30.]], PLATE, &[across], 2.)
                .unwrap_err()
                .items,
            [0]
        );
        // Above it there is room for a lower one.
        let corners = arrange(&[[70., 20.]], PLATE, &[across], 2.).unwrap();
        assert!(apart(&placed(&[[70., 20.]], &corners), &[across], 2.));
    }

    #[test]
    #[ignore = "measures arrange on many footprints: run with --release -- --ignored --nocapture"]
    fn arrange_is_timed_on_many_footprints() {
        // A 218 x 123 mm plate (a 12K panel's), footprints of mixed sizes, with and without a
        // model left in the middle.
        let area = Rect {
            min: [0., 0.],
            max: [218., 123.],
        };
        let middle = Rect {
            min: [94., 46.],
            max: [124., 76.],
        };
        for (count, side) in [(50, 12.), (100, 9.), (200, 6.), (400, 4.)] {
            let sizes: Vec<[f64; 2]> = (0..count)
                .map(|k: i32| [side + f64::from(k % 3), side + f64::from(k % 2)])
                .collect();
            for obstacles in [&[][..], &[middle][..]] {
                let start = std::time::Instant::now();
                let result = arrange(&sizes, area, obstacles, 2.);
                eprintln!(
                    "{count} footprints of {side} mm, {} obstacle(s): {:.1} ms, {} left out",
                    obstacles.len(),
                    start.elapsed().as_secs_f64() * 1000.,
                    result.err().map_or(0, |e| e.items.len())
                );
            }
        }
    }

    #[test]
    fn overlapping_pairs_ignore_touching_edges() {
        let r = |x: f64, y: f64| Rect {
            min: [x, y],
            max: [x + 10., y + 10.],
        };
        assert_eq!(
            overlapping_pairs(&[r(0., 0.), r(10., 0.), r(5., 5.), r(30., 30.)]),
            [(0, 2), (1, 2)]
        );
    }
}
