//! Where a branch joins its trunk (owner, 2026-10-30): a branch is a support whose shaft joins
//! another support's instead of reaching the plate, so tips close together share one trunk, and
//! a tip over the model need not go down through it. Shared by branches placed by hand and the
//! planners' trees.
//!
//! A branch joins the shaft at the highest height where its slanted part (from the tip's join,
//! the tip pointing at the junction) is at most 45° from vertical, the tip within the axis limit
//! from the surface, and, when the model's surface for routes is gridded, clear of the model by
//! the support's widest radius and the clearance.
//!
//! The planners keep a branch's tip along the surface's normal (TD-164,
//! [`Junction::keep_normal`]): the slanted part starts at the tip's join along the normal, and
//! the junction is lowered until that part is at most 45° from vertical.
use super::route::{contact_clearance, widest, RouteSurface};
use karasu_core::{
    geometry::{plan_length, sin_cos_degrees},
    supports::{
        BaseSpec, SupportDocument, SupportKind, SupportParameters, SupportScene, TipCone,
        DEFAULT_ANCHOR_TOLERANCE_MM,
    },
};

/// The supports of `document` that `ids` are, and every support standing on any of them,
/// directly or through others: what deleting them deletes (owner: deleting a support deletes
/// the branches on it, and theirs).
#[must_use]
pub fn with_branches(document: &SupportDocument, ids: &[u64]) -> Vec<u64> {
    let mut gone: Vec<u64> = ids.to_vec();
    loop {
        let more: Vec<u64> = document
            .supports
            .iter()
            .filter(|s| !gone.contains(&s.id))
            .filter(
                |s| matches!(s.base, BaseSpec::Support { parent, .. } if gone.contains(&parent)),
            )
            .map(|s| s.id)
            .collect();
        if more.is_empty() {
            return gone;
        }
        gone.extend(more);
    }
}

/// `document` without the supports `ids`, the branches on them, and the braces on any of them.
pub fn without(document: &mut SupportDocument, ids: &[u64]) {
    let gone = with_branches(document, ids);
    document.supports.retain(|s| !gone.contains(&s.id));
    document
        .braces
        .retain(|b| !gone.contains(&b.from.0) && !gone.contains(&b.to.0));
}

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
    /// A standard branch's tip stays along the surface's normal (the planners, TD-164), rather
    /// than pointing at the junction.
    pub keep_normal: bool,
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
    let limit = sin_cos_degrees(j.document.map_or(60.0, |d| d.max_axis_deviation_deg)).1;
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
    let bottom = low + FROM_ENDS * j.trunk_mm;
    // A standard tip keeps the clearance from the walls beside its contact, when the document
    // asks (TD-164).
    let walls = contact_clearance(j.document).filter(|_| j.kind == SupportKind::Standard);
    if j.keep_normal && j.kind == SupportKind::Standard {
        // The tip along the normal, or no junction: it is never tilted (TD-164).
        let axis = unit(normal)?;
        if let Some((r, mesh)) = surface {
            if !r.grid().reach_clear(mesh, &cone, j.tip, axis, axis)
                || walls.is_some_and(|c| !r.grid().contact_clear(mesh, &cone, j.tip, axis, axis, c))
            {
                return None;
            }
        }
        let join = add(j.tip, scale(axis, length));
        let mut z = (high - FROM_ENDS * j.trunk_mm).min(join[2]);
        while z >= bottom {
            if let Some(at) = j.scene.shaft_point(j.parent, z) {
                let across = plan_length(join[0] - at[0], join[1] - at[1]);
                let rise = join[2] - at[2];
                if rise > 0.0
                    && across <= rise * SLANT
                    && radius.is_none_or(|radius| {
                        surface.is_none_or(|(r, mesh)| r.grid().clear(mesh, join, at, radius))
                    })
                {
                    return Some(at);
                }
            }
            z -= STEP_MM;
        }
        return None;
    }
    let top = (high - FROM_ENDS * j.trunk_mm).min(j.tip[2] - length);
    let mut z = top;
    while z >= bottom {
        if let Some(at) = j.scene.shaft_point(j.parent, z) {
            let towards = unit(sub(at, j.tip));
            if let Some(axis) = towards.filter(|a| dot(*a, normal) >= limit) {
                let join = add(j.tip, scale(axis, length));
                let across = plan_length(join[0] - at[0], join[1] - at[1]);
                let rise = join[2] - at[2];
                // A standard tip also keeps off the model beside its contact (TD-124): a lower
                // junction leans it another way.
                let clear = radius.is_none_or(|radius| {
                    surface.is_none_or(|(r, mesh)| {
                        r.grid().clear(mesh, join, at, radius)
                            && r.grid().reach_clear(mesh, &cone, j.tip, axis, normal)
                            && walls.is_none_or(|c| {
                                r.grid().contact_clear(mesh, &cone, j.tip, axis, normal, c)
                            })
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
