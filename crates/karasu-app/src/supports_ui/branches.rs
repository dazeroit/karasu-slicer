//! Branches (owner, 2026-10-30): supports whose shaft joins another support's instead of
//! reaching the plate, so tips close together share one trunk, and a tip over the model need not
//! go down through it. The support document already holds them (a base on a support at a
//! height); this places and deletes them.
//!
//! **Placed** with the Place option "Branch onto a support": a click on the model sets the
//! branch's tip, then a click on a support's shaft adds the branch there, as one undoable step,
//! with the next support's values. It joins the shaft at the highest height where its slanted
//! part (from the tip's join, the tip pointing at the junction) is at most 45° from vertical,
//! the tip within the axis limit from the surface, and, when the model's surface for routes is
//! gridded, clear of the model by the support's widest radius and the clearance. A shaft with
//! no such height is said and nothing is added. The junction is a handle that slides along the
//! trunk.
//!
//! **No thicker than their trunk** (owner, 2026-10-30): a branch is at most as thick as its
//! trunk where it joins it. Placed, or given a preset, thicker than that (its shaft, top or
//! bottom, or its tip's body), it takes its trunk's values, said in a notification, so its
//! preset is its trunk's; a value changed in the panel past that is brought back to it.
//!
//! **Deleted with their trunk** (owner): deleting a support deletes the branches on it, and
//! theirs, with the braces on any of them.
use super::{route::widest, Preview, SupportsUi, FACING_UP};
use crate::i18n::tr;
use crate::{
    camera::Ray,
    workspace::{SurfaceHit, Workspace},
};
use karasu_core::supports::{
    AxisSpec, BaseSpec, ManualSupport, Preset, SupportDocument, SupportKind, SupportOrigin,
    SupportParameters, TipCone, DEFAULT_ANCHOR_TOLERANCE_MM,
};

/// The steepest a branch's slanted part goes, as the tangent of its angle from vertical: 45°,
/// the steepest shaft the build accepts without a warning, less a margin for rounding.
const SLANT: f64 = 0.98;
/// How far a junction stays from its trunk's ends, in its shaft's diameters.
const FROM_ENDS: f64 = 1.5;
/// The step a junction is looked for down a trunk, in mm.
const STEP_MM: f64 = 0.1;

/// A branch's tip, clicked, waiting for the shaft it joins.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct BranchTip {
    pub(super) object: usize,
    /// On the surface, in world coordinates, and its outward normal there.
    pub(super) point: [f64; 3],
    pub(super) normal: [f64; 3],
}

/// The supports of `document` that `ids` are, and every support standing on any of them,
/// directly or through others: what deleting them deletes.
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

/// How thick a support of `parameters` is along its path: its shaft, top or bottom, or its tip's
/// body; a mini support, its diameter.
fn thickness(parameters: &SupportParameters, kind: SupportKind) -> f64 {
    if kind == SupportKind::Mini {
        return parameters.mini_diameter_mm;
    }
    parameters
        .shaft_diameter_mm
        .max(parameters.shaft_bottom_diameter_mm.unwrap_or(0.0))
        .max(parameters.tip_body_diameter_mm)
}

/// `parameters` at most `trunk_mm` thick (the document's least diameter, 0.1 mm, at least): the
/// shaft, top and bottom, and the tip's body, its contact no wider than its body; a mini
/// support's diameter, its ends reaching less deep than its radius (the core's coupling).
fn thinned(
    mut parameters: SupportParameters,
    trunk_mm: f64,
    kind: SupportKind,
) -> SupportParameters {
    let most = trunk_mm.max(0.1);
    if kind == SupportKind::Mini {
        parameters.mini_diameter_mm = parameters.mini_diameter_mm.min(most);
        // Thinner, its ends may reach past its radius: half the radius then.
        let radius = parameters.mini_diameter_mm / 2.0;
        if parameters.mini_penetration_mm >= radius {
            parameters.mini_penetration_mm = radius / 2.0;
        }
        return parameters;
    }
    parameters.shaft_diameter_mm = parameters.shaft_diameter_mm.min(most);
    parameters.shaft_bottom_diameter_mm = parameters.shaft_bottom_diameter_mm.map(|d| d.min(most));
    parameters.tip_body_diameter_mm = parameters.tip_body_diameter_mm.min(most);
    parameters.contact_diameter_mm = parameters
        .contact_diameter_mm
        .min(parameters.tip_body_diameter_mm);
    parameters.contact_fit_min_mm = parameters
        .contact_fit_min_mm
        .map(|m| m.min(parameters.contact_diameter_mm));
    parameters
}

/// What a branch's junction is looked for with: the trunk's build, its identity there and its
/// shaft's diameter, the branch's tip (point and outward normal) and values, the document (its
/// axis limit and clearance; the defaults without one) and the model's surface for routes, if
/// gridded.
pub(crate) struct Junction<'a> {
    pub(crate) scene: &'a karasu_core::supports::SupportScene,
    pub(crate) parent: u64,
    pub(crate) trunk_mm: f64,
    pub(crate) tip: [f64; 3],
    pub(crate) normal: [f64; 3],
    pub(crate) parameters: &'a SupportParameters,
    /// What the branch is made of: a mini support's rod starts at its tip's ball.
    pub(crate) kind: SupportKind,
    pub(crate) document: Option<&'a SupportDocument>,
    pub(crate) surface: Option<&'a super::route::RouteSurface>,
}

/// Where a branch joins its trunk: the highest point of the trunk's shaft, clear of its ends,
/// where the branch's slanted part from its tip's join (the tip pointing at the point, within
/// the axis limit) is at most 45° from vertical and, with the surface, clear of the model by
/// its widest radius and the clearance; `None` without one. Shared by branches placed by hand
/// and the Islands tool's.
pub(crate) fn junction_on(j: &Junction<'_>) -> Option<[f64; 3]> {
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

impl SupportsUi {
    /// How thick the trunk of `support` of model `index` is where it joins it, if it is a
    /// branch and its trunk is built.
    fn trunk_thickness(&self, index: usize, support: &ManualSupport) -> Option<f64> {
        let BaseSpec::Support { parent, z_mm } = support.base else {
            return None;
        };
        self.scene(index)?.shaft_diameter(parent, z_mm)
    }

    /// `support` of model `index` with a value changed in the panel: a branch brought back to
    /// no thicker than its trunk where it joins it; otherwise as it is.
    pub(super) fn no_thicker(&self, index: usize, mut support: ManualSupport) -> ManualSupport {
        if let Some(trunk) = self.trunk_thickness(index, &support) {
            support.parameters = thinned(support.parameters, trunk, support.kind);
        }
        support
    }

    /// `support` of model `index` placed or given a preset: a branch thicker than its trunk
    /// where it joins it takes its trunk's values, said in a notification; otherwise as it is.
    pub(super) fn as_branch(
        &mut self,
        workspace: &Workspace,
        index: usize,
        mut support: ManualSupport,
        tell: bool,
    ) -> ManualSupport {
        let Some(trunk) = self.trunk_thickness(index, &support) else {
            return support;
        };
        if thickness(&support.parameters, support.kind) <= trunk + 1e-9 {
            return support;
        }
        let BaseSpec::Support { parent, .. } = support.base else {
            return support;
        };
        let values = workspace
            .supports(index)
            .and_then(|d| d.supports.iter().find(|s| s.id == parent))
            .map(|s| s.parameters.clone());
        // A trunk itself tapered below the junction: its own values, still no thicker there.
        // A mini branch keeps its own values, thinned.
        let values = values.filter(|_| support.kind == SupportKind::Standard);
        support.parameters = thinned(values.unwrap_or(support.parameters), trunk, support.kind);
        if tell {
            self.outbox.push(crate::messages::Message::new(
                crate::messages::Level::Info,
                tr!("supports-branch-trunk-values"),
            ));
        }
        support
    }

    /// A new branch's tip axis: pointing at its junction; a mini support's ball sits along the
    /// surface's normal.
    fn branch_axis(&self) -> AxisSpec {
        if self.mini {
            AxisSpec::Normal
        } else {
            AxisSpec::Straight
        }
    }

    /// Place branches onto supports (the Place option), or whole supports.
    pub fn set_branching(&mut self, on: bool) {
        self.branching = on;
        if on {
            self.tip_then_foot = false;
        }
        self.forget_placing();
    }

    /// Whether a branch's tip waits for the shaft it joins.
    #[must_use]
    pub fn branch_waiting(&self) -> bool {
        self.branch_tip.is_some()
    }

    /// Where a branch from the tip at `tip` (outward `normal`) on model `index` joins support
    /// `parent`'s shaft as built: the junction point, in world coordinates; see the module.
    pub(super) fn junction(
        &self,
        workspace: &Workspace,
        index: usize,
        tip: [f64; 3],
        normal: [f64; 3],
        parent: u64,
    ) -> Option<[f64; 3]> {
        let document = workspace.supports(index);
        let p = self.next.parameters();
        let trunk = document
            .and_then(|d| d.supports.iter().find(|s| s.id == parent))
            .map_or(p.shaft_diameter_mm, |s| s.parameters.shaft_diameter_mm);
        junction_on(&Junction {
            scene: self.scene(index)?,
            parent,
            trunk_mm: trunk,
            tip,
            normal,
            parameters: &p,
            kind: self.next_kind(),
            document,
            surface: self.current_route(workspace, index),
        })
    }

    /// Follow the pointer's `ray` while a branch's tip waits on model `index`: the branch it
    /// would add to the shaft under the pointer shows, joined where it would join.
    pub fn branch_hover(&mut self, workspace: &Workspace, ray: Option<&Ray>, index: usize) {
        let Some(tip) = self.branch_tip.filter(|t| t.object == index) else {
            return;
        };
        let target = ray
            .and_then(|r| self.shaft_under(r, index))
            .map(|(parent, _)| parent);
        let junction = target.and_then(|parent| {
            Some((
                parent,
                self.junction(workspace, index, tip.point, tip.normal, parent)?,
            ))
        });
        self.branch_refused = target.is_some() && junction.is_none();
        let Some((parent, at)) = junction else {
            self.preview = None;
            return;
        };
        #[allow(clippy::float_cmp)] // The same junction exactly: nothing to build again.
        if self.preview.as_ref().is_some_and(|p| p.at == at) {
            return;
        }
        let support = self.as_branch(
            workspace,
            index,
            ManualSupport {
                id: 1,
                tip_mm: tip.point,
                tip_axis: self.branch_axis(),
                bends: Vec::new(),
                base: BaseSpec::Support {
                    parent,
                    z_mm: at[2],
                },
                parameters: self.next.parameters(),
                accept_collision: false,
                origin: SupportOrigin::Placed,
                kind: self.next_kind(),
            },
            false,
        );
        self.stamp += 1;
        let stamp = self.stamp;
        self.preview =
            super::alone(workspace, support, &[(tip.point, tip.normal)], Some(at)).map(|scene| {
                Preview {
                    stamp,
                    ..Preview::of(scene, at, self.next)
                }
            });
    }

    /// A click while placing branches on model `index`: on the model, the branch's tip; then on
    /// a support's shaft, the branch, added as one undoable step and selected. Returns whether
    /// the click was taken.
    pub(super) fn branch_click(
        &mut self,
        workspace: &mut Workspace,
        ray: &Ray,
        hit: Option<SurfaceHit>,
        index: usize,
    ) -> bool {
        let Some(tip) = self.branch_tip.filter(|t| t.object == index) else {
            let Some(hit) = hit.filter(|h| h.object == index) else {
                return true;
            };
            if hit.normal[2] > FACING_UP {
                self.candidate = Some(super::Candidate::FacingUp);
                return true;
            }
            self.branch_tip = Some(BranchTip {
                object: index,
                point: hit.point,
                normal: hit.normal,
            });
            self.preview = None;
            return true;
        };
        let Some((parent, _)) = self.shaft_under(ray, index) else {
            return true;
        };
        let Some(at) = self.junction(workspace, index, tip.point, tip.normal, parent) else {
            self.branch_refused = true;
            return true;
        };
        let Some(to_model) = workspace.model_to_world(index).and_then(|m| m.inverse()) else {
            return true;
        };
        let id = Self::next_id(workspace, index);
        let mut document = workspace
            .supports(index)
            .cloned()
            .unwrap_or_else(|| SupportDocument::new(Preset::Medium));
        let branch = self.as_branch(
            workspace,
            index,
            ManualSupport {
                id,
                tip_mm: to_model.point(tip.point),
                tip_axis: self.branch_axis(),
                bends: Vec::new(),
                base: BaseSpec::Support {
                    parent,
                    z_mm: at[2],
                },
                parameters: self.next.parameters(),
                accept_collision: false,
                origin: SupportOrigin::Placed,
                kind: self.next_kind(),
            },
            true,
        );
        document.supports.push(branch);
        workspace.set_supports(index, Some(document), None);
        self.branch_tip = None;
        self.branch_refused = false;
        self.preview = None;
        self.selected = Some((index, id));
        self.reveal = true;
        true
    }
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

#[cfg(test)]
mod tests;
