//! Where a branch joins its trunk (owner, 2026-10-30): a branch is a support whose shaft joins
//! another support's instead of reaching the plate, so tips close together share one trunk, and
//! a tip over the model need not go down through it. Shared by branches placed by hand and the
//! planners' trees.
//!
//! A branch joins the shaft at the highest height where its slanted part (from the tip's join,
//! the tip pointing at the junction) is at most 45° from vertical, the tip within the axis limit
//! from the surface, and, when the model's surface for routes is gridded, clear of the model by
//! the support's widest radius and the clearance.
use super::route::{widest, RouteSurface};
use karasu_core::supports::{
    SupportDocument, SupportKind, SupportParameters, SupportScene, TipCone,
    DEFAULT_ANCHOR_TOLERANCE_MM,
};

/// The steepest a branch's slanted part goes, as the tangent of its angle from vertical: 45°,
/// the steepest shaft the build accepts without a warning, less a margin for rounding.
const SLANT: f64 = 0.98;
/// How far a junction stays from its trunk's ends, in its shaft's diameters.
const FROM_ENDS: f64 = 1.5;
/// The step a junction is looked for down a trunk, in mm.
const STEP_MM: f64 = 0.1;

/// What a branch's junction is looked for with: the trunk's build, its identity there and its
/// shaft's diameter, the branch's tip (point and outward normal) and values, the document (its
/// axis limit and clearance; the defaults without one) and the model's surface for routes, if
/// gridded.
pub struct Junction<'a> {
    /// The trunk's build.
    pub scene: &'a SupportScene,
    /// The trunk's identity in that build.
    pub parent: u64,
    /// The trunk's shaft diameter, in mm.
    pub trunk_mm: f64,
    /// The branch's tip, in world coordinates.
    pub tip: [f64; 3],
    /// The surface's outward normal at the tip.
    pub normal: [f64; 3],
    /// The branch's values.
    pub parameters: &'a SupportParameters,
    /// What the branch is made of: a mini support's rod starts at its tip's ball.
    pub kind: SupportKind,
    /// The document whose axis limit and clearance apply; the defaults without one.
    pub document: Option<&'a SupportDocument>,
    /// The model's surface for routes, if gridded.
    pub surface: Option<&'a RouteSurface>,
}

/// Where a branch joins its trunk: the highest point of the trunk's shaft, clear of its ends,
/// where the branch's slanted part from its tip's join (the tip pointing at the point, within
/// the axis limit) is at most 45° from vertical and, with the surface, clear of the model by
/// its widest radius and the clearance; `None` without one. Shared by branches placed by hand
/// and the Islands tool's.
#[must_use]
pub fn junction_on(j: &Junction<'_>) -> Option<[f64; 3]> {
    let [low, high] = j.scene.shaft_span(j.parent)?;
    let clearance = j.document.map_or(0.3, |d| d.clearance_mm);
    // A mini support's rod starts at its tip's ball and may touch the model (owner,
    // 2026-10-30): it keeps no clearance.
    let (length, radius) = match j.kind {
        SupportKind::Standard => (
            j.parameters.tip_length_mm,
            Some(widest(j.parameters) + clearance),
        ),
        SupportKind::Mini => {
            let r = j.parameters.mini_diameter_mm / 2.0;
            (r - j.parameters.mini_penetration_mm, None)
        }
    };
    let limit = j
        .document
        .map_or(60.0, |d| d.max_axis_deviation_deg)
        .to_radians()
        .cos();
    let surface = j.surface.and_then(|r| r.mesh().map(|m| (r, m)));
    let cone = TipCone::of(
        j.parameters,
        j.document
            .map_or(DEFAULT_ANCHOR_TOLERANCE_MM, |d| d.anchor_tolerance_mm),
    );
    // The axis limit is measured from the surface's mean normal, as the build measures it
    // (TD-124).
    let normal = j.surface.map_or(j.normal, |r| {
        let radius = match j.kind {
            SupportKind::Standard => j.parameters.footprint_radius_mm(),
            SupportKind::Mini => j.parameters.mini_diameter_mm / 2.0,
        };
        r.normal_at(j.tip, j.normal, radius)
    });
    let top = (high - FROM_ENDS * j.trunk_mm).min(j.tip[2] - length);
    let bottom = low + FROM_ENDS * j.trunk_mm;
    let mut z = top;
    while z >= bottom {
        if let Some(at) = j.scene.shaft_point(j.parent, z) {
            let towards = unit(sub(at, j.tip));
            if let Some(axis) = towards.filter(|a| dot(*a, normal) >= limit) {
                let join = add(j.tip, scale(axis, length));
                let across = (join[0] - at[0]).hypot(join[1] - at[1]);
                let rise = join[2] - at[2];
                // A standard tip also keeps off the model beside its contact (TD-124): a lower
                // junction leans it another way.
                let clear = radius.is_none_or(|radius| {
                    surface.is_none_or(|(r, mesh)| {
                        r.grid().clear(mesh, join, at, radius)
                            && r.grid().reach_clear(mesh, &cone, j.tip, axis, normal)
                    })
                });
                if rise > 0.0 && across <= rise * SLANT && clear {
                    return Some(at);
                }
            }
        }
        z -= STEP_MM;
    }
    None
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn scale(a: [f64; 3], k: f64) -> [f64; 3] {
    a.map(|c| c * k)
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn unit(a: [f64; 3]) -> Option<[f64; 3]> {
    let length = dot(a, a).sqrt();
    (length.is_finite() && length > 0.0).then(|| scale(a, 1.0 / length))
}
