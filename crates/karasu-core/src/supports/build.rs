//! Support scene construction and validation (TD-062).
#![allow(clippy::many_single_char_names)] // Geometric notation: points, axes, radii.
use super::{
    document::{
        AxisSpec, BaseSpec, ContactShape, ManualSupport, RaftLayout, RaftShape, RaftSpec,
        SupportDocument, SupportKind, SKATE_MIN_PLATE_RADIUS_MM,
    },
    fit,
    index::{
        clear_of_tapered, normal, point_triangle_distance, proven_clear, segment_meets_triangle,
        TriangleIndex, PROOF_MARGIN_MM,
    },
    shapes::{add, dot, frustum, length, scale, sphere, sub, unit, Part, V},
    tip::{reach, surface_normal, Reach, TipCone, NEAR_FOOTPRINTS},
    FindingKind, Severity, SupportError, SupportFinding, SupportPart,
};
use crate::{
    cancellation::{Cancellation, Checkpoint},
    geometry::{sin_cos_degrees, Point3},
    mesh::TriangleMesh,
    preparation::hollow::{convex_component, HollowLimits},
};
use std::{cmp::Ordering, collections::BTreeMap};

mod web;

/// The model surface supports stand on: the sliced mesh in world millimetres.
#[derive(Clone, Copy, Debug)]
pub struct SupportSurface<'a> {
    /// Closed, outward surface: the placed model, or the hollowed shell.
    pub mesh: &'a TriangleMesh,
    /// Index of the first cavity triangle of a hollowed shell (its source faces come first),
    /// or `None` for a solid model.
    pub interior_from: Option<usize>,
}

/// A drain hole: a solid cylinder that the slicer subtracts from the model's layers, not the
/// supports' (TD-123).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DrainCylinder {
    /// Axis start, in world millimetres.
    pub start_mm: [f64; 3],
    /// Axis end, in world millimetres.
    pub end_mm: [f64; 3],
    /// Radius in millimetres.
    pub radius_mm: f64,
}

impl DrainCylinder {
    /// Whether `point` (world millimetres) lies strictly inside the cylinder, where the hole
    /// removes the model's material (TD-131): a surface point there has nothing left to hold.
    /// A point on the rim, at the radius, is not inside. A degenerate axis contains nothing.
    #[must_use]
    pub fn removes(&self, point: [f64; 3]) -> bool {
        let axis = sub(self.end_mm, self.start_mm);
        let length_sq = dot(axis, axis);
        if length_sq <= 0.0 || !length_sq.is_finite() {
            return false;
        }
        let offset = sub(point, self.start_mm);
        let along = dot(offset, axis) / length_sq;
        if !(0.0..=1.0).contains(&along) {
            return false;
        }
        let radial = sub(offset, axis.map(|c| c * along));
        dot(radial, radial) < self.radius_mm * self.radius_mm
    }
}

/// The printer's build volume: `[origin, origin + size]` in XY and `[0, height]` in Z.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BuildVolume {
    /// World XY of the LCD origin, in millimetres.
    pub origin_xy_mm: [f64; 2],
    /// Width, depth and height, in millimetres.
    pub size_mm: [f64; 3],
}

/// Caller choices for a build.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SupportOptions {
    /// Accept every collision with the model (each one is still reported).
    pub accept_all_collisions: bool,
}

/// What a support's base stands on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BaseKind {
    /// The plate or the raft.
    Platform,
    /// The model surface.
    Model,
    /// Another support.
    Support,
}

/// One built (or rejected) support.
#[derive(Clone, Debug, PartialEq)]
pub struct BuiltSupport {
    /// Support ID.
    pub id: u64,
    /// What it is made of.
    pub kind: SupportKind,
    /// What its base stands on.
    pub base: BaseKind,
    /// Whether its tip touches a cavity surface.
    pub interior_tip: bool,
    /// Whether its model foot touches a cavity surface, for model bases.
    pub interior_foot: Option<bool>,
    /// Number of parts; zero when it was rejected.
    pub parts: usize,
    /// The contact's diameter as built: narrowed when the support's contact fits the surface
    /// (TD-116). `None` for a mini support and a rejected one.
    pub contact_diameter_mm: Option<f64>,
}

/// The triangles of one part in a [`SupportScene`]'s mesh.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScenePart {
    /// Support or brace ID; zero for the raft.
    pub entity: u64,
    /// What the part is.
    pub part: SupportPart,
    /// Its triangles in the combined mesh.
    pub triangles: std::ops::Range<usize>,
}

/// Built supports: their combined mesh, per-support outcomes and every finding.
#[derive(Debug)]
pub struct SupportScene {
    mesh: TriangleMesh,
    ranges: Vec<ScenePart>,
    supports: Vec<BuiltSupport>,
    braces: usize,
    rafts: usize,
    parts: usize,
    findings: Vec<SupportFinding>,
    paths: BTreeMap<u64, Path>,
}

impl SupportScene {
    /// Every support part and the raft, concatenated: closed outward shells to be unioned with
    /// the model by positive winding.
    #[must_use]
    pub const fn mesh(&self) -> &TriangleMesh {
        &self.mesh
    }
    /// Every part's triangles in [`Self::mesh`], in mesh order, so one support can be drawn or
    /// picked apart from the others.
    #[must_use]
    pub fn part_ranges(&self) -> &[ScenePart] {
        &self.ranges
    }
    /// Per-support outcomes, in build order (parents first).
    #[must_use]
    pub fn supports(&self) -> &[BuiltSupport] {
        &self.supports
    }
    /// Built braces.
    #[must_use]
    pub const fn braces(&self) -> usize {
        self.braces
    }
    /// Whether a raft was built.
    #[must_use]
    pub const fn raft(&self) -> bool {
        self.rafts > 0
    }
    /// How many separate rafts were built: one, one per group of feet, or a web's pieces
    /// (TD-144).
    #[must_use]
    pub const fn rafts(&self) -> usize {
        self.rafts
    }
    /// Total parts, raft included.
    #[must_use]
    pub const fn parts(&self) -> usize {
        self.parts
    }
    /// Every finding, in document order.
    #[must_use]
    pub fn findings(&self) -> &[SupportFinding] {
        &self.findings
    }
    /// Whether any finding prevents slicing.
    #[must_use]
    pub fn has_errors(&self) -> bool {
        self.findings.iter().any(|f| f.severity == Severity::Error)
    }
    /// The point at world height `z_mm` on support `id`'s shaft as built, where a brace or a
    /// support standing on it would join it: its first non-horizontal segment from the top that
    /// reaches the height. `None` for a support that was not built or a height its shaft does
    /// not reach.
    #[must_use]
    pub fn shaft_point(&self, id: u64, z_mm: f64) -> Option<[f64; 3]> {
        self.paths.get(&id)?.at(z_mm).map(|(p, _)| p)
    }
    /// The diameter of support `id`'s shaft as built at world height `z_mm` (a shaft may narrow
    /// towards its tip), where [`Self::shaft_point`] finds it.
    #[must_use]
    pub fn shaft_diameter(&self, id: u64, z_mm: f64) -> Option<f64> {
        self.paths.get(&id)?.at(z_mm).map(|(_, r)| 2.0 * r)
    }
    /// The points of support `id`'s shaft as built, in world millimetres: from its tip's join,
    /// through its bends, to its base join; `None` for a support that was not built.
    #[must_use]
    pub fn shaft_path(&self, id: u64) -> Option<&[[f64; 3]]> {
        self.paths.get(&id).map(|p| p.points.as_slice())
    }
    /// The lowest and highest heights of support `id`'s shaft as built, from its base join to
    /// its tip join; `None` for a support that was not built.
    #[must_use]
    pub fn shaft_span(&self, id: u64) -> Option<[f64; 2]> {
        self.paths.get(&id).map(Path::span)
    }
}

/// A part to check, with its owner.
struct Owned {
    entity: u64,
    part: SupportPart,
    accepted: bool,
    /// Contact and model-foot parts may touch the model.
    exempt: bool,
    shape: Part,
    /// Of an exempt contact or foot, the stretch of its cone away from where it touches, which
    /// must not meet the model (TD-124).
    reach: Option<Reach>,
}

/// The built path of a valid support: points from the tip join down to the base join, with the
/// shaft radius at each.
#[derive(Debug)]
pub(super) struct Path {
    pub(super) points: Vec<V>,
    radii: Vec<f64>,
}

impl Path {
    /// The largest shaft radius along the path.
    pub(super) fn widest(&self) -> f64 {
        self.radii.iter().copied().fold(0.0, f64::max)
    }

    /// The lowest and highest heights of the path's points.
    pub(super) fn span(&self) -> [f64; 2] {
        let zs = self.points.iter().map(|p| p[2]);
        [
            zs.clone().fold(f64::INFINITY, f64::min),
            zs.fold(f64::NEG_INFINITY, f64::max),
        ]
    }

    /// The point on the path at height `z` (first segment from the top containing it) and the
    /// radius there; `None` when no non-horizontal segment reaches `z`.
    pub(super) fn at(&self, z: f64) -> Option<(V, f64)> {
        for i in 0..self.points.len() - 1 {
            let (a, b) = (self.points[i], self.points[i + 1]);
            let (lo, hi) = (a[2].min(b[2]), a[2].max(b[2]));
            if lo < hi && (lo..=hi).contains(&z) {
                let t = (z - a[2]) / (b[2] - a[2]);
                let mut p = add(a, scale(sub(b, a), t));
                p[2] = z;
                let r = self.radii[i] + (self.radii[i + 1] - self.radii[i]) * t;
                return Some((p, r));
            }
        }
        None
    }
}

struct Builder<'a, 'c, 'k> {
    doc: &'a SupportDocument,
    surface: SupportSurface<'a>,
    index: TriangleIndex<'a>,
    parts: Vec<Owned>,
    findings: Vec<SupportFinding>,
    paths: BTreeMap<u64, Path>,
    cancel: &'k mut Cancellation<'c>,
    /// Plate floor: the raft top, or zero.
    floor: f64,
    /// `cos` of the maximum axis deviation.
    cos_deviation: f64,
}

fn finding(entity: u64, part: SupportPart, kind: FindingKind, detail: String) -> SupportFinding {
    SupportFinding {
        entity,
        part,
        kind,
        severity: Severity::Error,
        detail,
    }
}

fn fmt_point(p: V) -> String {
    format!("({}, {}, {})", p[0], p[1], p[2])
}

/// A surface anchor: triangle, outward unit normal and whether it is a cavity surface.
struct Anchor {
    normal: V,
    interior: bool,
}

impl Builder<'_, '_, '_> {
    fn error(&mut self, entity: u64, part: SupportPart, kind: FindingKind, detail: String) {
        self.findings.push(finding(entity, part, kind, detail));
    }

    /// The anchor at `p`: the surface's normal there, the mean within `radius` (TD-124), and
    /// whether it is the cavity's.
    fn anchor(&mut self, id: u64, part: SupportPart, p: V, radius: f64) -> Option<Anchor> {
        let tolerance = self.doc.anchor_tolerance_mm;
        let Some((triangle, _)) = self.index.nearest(p, tolerance) else {
            self.error(
                id,
                part,
                FindingKind::AnchorOffSurface,
                format!(
                    "{} is not on the model surface (no surface within {tolerance} mm)",
                    fmt_point(p)
                ),
            );
            return None;
        };
        let Some(n) = normal(self.index.corners(triangle)) else {
            self.error(
                id,
                part,
                FindingKind::AnchorOffSurface,
                format!("{} lies on degenerate triangle {triangle}", fmt_point(p)),
            );
            return None;
        };
        let reach = [radius; 3];
        let near = self
            .index
            .candidates(sub(p, reach), add(p, reach))
            .into_iter()
            .map(|t| self.index.corners(t));
        Some(Anchor {
            normal: surface_normal(near, p, n, radius),
            interior: self
                .surface
                .interior_from
                .is_some_and(|first| triangle as usize >= first),
        })
    }

    fn axis(
        &mut self,
        id: u64,
        part: SupportPart,
        spec: AxisSpec,
        anchor: &Anchor,
        from: V,
        target: V,
    ) -> Option<V> {
        let axis = match spec {
            AxisSpec::Normal => Some(anchor.normal),
            AxisSpec::Straight => unit(sub(target, from)),
            AxisSpec::Direction(d) => unit(d),
        };
        let Some(axis) = axis else {
            self.error(
                id,
                part,
                FindingKind::InvalidPath,
                "the axis has no direction".to_owned(),
            );
            return None;
        };
        let cosine = dot(axis, anchor.normal);
        if cosine < self.cos_deviation {
            let degrees = cosine.clamp(-1.0, 1.0).acos().to_degrees();
            self.error(
                id,
                part,
                FindingKind::AxisDeviation,
                format!(
                    "axis deviates {degrees:.1}° from the surface normal (at most {}°)",
                    self.doc.max_axis_deviation_deg
                ),
            );
        }
        // Reported, never dropped: the axis still defines the support's shape.
        Some(axis)
    }

    /// Penetration from depth 2τ to `depth + τ` along `-axis` must not meet any surface.
    fn check_piercing(&mut self, id: u64, part: SupportPart, p: V, axis: V, depth: f64) {
        let tau = self.doc.anchor_tolerance_mm;
        // The segment from `2τ` to `depth + τ` is empty only when `depth <= τ` (TD-136: a contact
        // sunk exactly `2τ`, every preset's 0.1 mm, is checked too).
        if depth <= tau {
            return;
        }
        let a = sub(p, scale(axis, 2.0 * tau));
        let b = sub(p, scale(axis, depth + tau));
        let lo = [0, 1, 2].map(|k| a[k].min(b[k]));
        let hi = [0, 1, 2].map(|k| a[k].max(b[k]));
        if let Some(t) = self
            .index
            .candidates(lo, hi)
            .into_iter()
            .find(|&t| segment_meets_triangle(a, b, self.index.corners(t)))
        {
            self.error(
                id,
                part,
                FindingKind::Piercing,
                format!("penetration of {depth} mm reaches another surface (triangle {t})"),
            );
        }
    }

    fn push(&mut self, s: &ManualSupport, part: SupportPart, exempt: bool, shape: Part) {
        self.parts.push(Owned {
            entity: s.id,
            part,
            accepted: s.accept_collision,
            exempt,
            shape,
            reach: None,
        });
    }

    /// Give the last contact or foot part pushed for support `id` its `reach` (TD-124).
    fn set_reach(&mut self, id: u64, part: SupportPart, reach: Option<Reach>) {
        if let Some(owned) = self
            .parts
            .iter_mut()
            .rev()
            .find(|o| o.entity == id && o.part == part)
        {
            owned.reach = reach;
        }
    }

    /// The point the straight tip axis aims at: the first bend, else the base join's location.
    fn straight_target(&self, s: &ManualSupport) -> Option<V> {
        let p = &s.parameters;
        if let Some(&bend) = s.bends.first() {
            return Some(bend);
        }
        // On a raft the shaft ends at its fillet's top; on the plate, at its pad's cone.
        let join = match self.doc.raft {
            Some(raft) => self.floor + raft.fillet_height_mm,
            None => self.floor + p.pad_thickness_mm + p.cone_height_mm,
        };
        match s.base {
            BaseSpec::Platform([x, y]) => Some([x, y, join]),
            BaseSpec::Vertical => Some([s.tip_mm[0], s.tip_mm[1], join]),
            BaseSpec::Model { point_mm, .. } => Some(point_mm),
            BaseSpec::Support { parent, z_mm } => self.paths.get(&parent)?.at(z_mm).map(|(q, _)| q),
        }
    }

    #[allow(clippy::too_many_lines)] // One support, step by step as specified.
    fn support(&mut self, s: &ManualSupport) -> Option<BuiltSupport> {
        if s.kind == SupportKind::Mini {
            return self.mini(s);
        }
        let id = s.id;
        let p = &s.parameters;
        let kind = match s.base {
            BaseSpec::Platform(_) | BaseSpec::Vertical => BaseKind::Platform,
            BaseSpec::Model { .. } => BaseKind::Model,
            BaseSpec::Support { .. } => BaseKind::Support,
        };
        let tip = s.tip_mm;
        let anchor = self.anchor(id, SupportPart::Contact, tip, p.footprint_radius_mm())?;
        let target = self.straight_target(s).unwrap_or(sub(tip, [0.0, 0.0, 1.0]));
        let a = self.axis(id, SupportPart::Contact, s.tip_axis, &anchor, tip, target)?;
        let join = add(tip, scale(a, p.tip_length_mm));
        // A contact fitted to the surface narrows first (TD-116); nothing else changes.
        let tau = self.doc.anchor_tolerance_mm;
        let fitted = fit::fitted(&self.index, tip, anchor.normal, p, tau);
        if let Some((narrowed, false)) = &fitted {
            self.findings.push(SupportFinding {
                entity: id,
                part: SupportPart::Contact,
                kind: FindingKind::ContactFit,
                severity: Severity::Warning,
                detail: format!(
                    "the contact's footprint does not lie on the model even at {} mm",
                    narrowed.contact_diameter_mm
                ),
            });
        }
        let p = fitted.as_ref().map_or(p, |(narrowed, _)| narrowed);
        self.check_piercing(id, SupportPart::Contact, tip, a, p.penetration_mm);
        let mut parts = Vec::new();
        let r_body = p.tip_body_diameter_mm / 2.0;
        let r_contact = p.contact_diameter_mm / 2.0;
        // Away from its contact, the tip's cone must not meet the model (TD-124).
        let tip_reach = TipCone::of(p, self.doc.anchor_tolerance_mm).reach(tip, a, anchor.normal);
        let mut foot_reach = None;
        match p.contact_shape {
            ContactShape::Cone => {
                parts.push((
                    SupportPart::Contact,
                    true,
                    frustum(
                        sub(tip, scale(a, p.penetration_mm)),
                        r_contact,
                        join,
                        r_body,
                        p.sides,
                    ),
                ));
            }
            ContactShape::Sphere => {
                let rho = p.contact_sphere_diameter_mm / 2.0;
                if rho - p.penetration_mm >= p.tip_length_mm {
                    self.error(
                        id,
                        SupportPart::Contact,
                        FindingKind::InvalidPath,
                        "the contact sphere reaches past the tip join; lengthen the tip".to_owned(),
                    );
                    return None;
                }
                let centre = add(tip, scale(a, rho - p.penetration_mm));
                parts.push((SupportPart::Contact, true, sphere(centre, rho, p.sides)));
                parts.push((
                    SupportPart::Contact,
                    true,
                    frustum(centre, r_contact, join, r_body, p.sides),
                ));
            }
        }
        // The base join and the base parts.
        let mut path_points = vec![join];
        path_points.extend_from_slice(&s.bends);
        let last = *path_points.last().expect("the path starts at the tip join");
        let r_top = p.shaft_diameter_mm / 2.0;
        let r_bottom = p.shaft_bottom_diameter_mm.unwrap_or(p.shaft_diameter_mm) / 2.0;
        let mut interior_foot = None;
        let mut base_parts = Vec::new();
        let base_join = match s.base {
            BaseSpec::Platform(_) | BaseSpec::Vertical => {
                let [x, y] = match s.base {
                    BaseSpec::Platform(xy) => xy,
                    _ => [last[0], last[1]],
                };
                if let Some(raft) = self.doc.raft {
                    // The raft is the foot: the shaft stands on it through its fillet, which
                    // starts inside the raft so the two print as one.
                    let b = [x, y, self.floor + raft.fillet_height_mm];
                    if last[2] <= b[2] {
                        self.error(
                            id,
                            SupportPart::Cone,
                            FindingKind::InvalidPath,
                            format!(
                                "the raft's fillet at Z {} is not below the shaft above it (Z {})",
                                b[2], last[2]
                            ),
                        );
                        return None;
                    }
                    let sunk = [x, y, self.floor - raft.thickness_mm / 2.0];
                    let wide = (raft.fillet_diameter_mm / 2.0).max(r_bottom);
                    base_parts.push((
                        SupportPart::Cone,
                        false,
                        frustum(sunk, wide, b, r_bottom, p.sides),
                    ));
                    b
                } else {
                    let pad_top = self.floor + p.pad_thickness_mm;
                    let b = [x, y, pad_top + p.cone_height_mm];
                    if last[2] <= b[2] {
                        self.error(
                            id,
                            SupportPart::Cone,
                            FindingKind::InvalidPath,
                            format!(
                                "the base join at Z {} is not below the shaft above it (Z {})",
                                b[2], last[2]
                            ),
                        );
                        return None;
                    }
                    base_parts.push((
                        SupportPart::Pad,
                        false,
                        frustum(
                            [x, y, self.floor],
                            p.pad_diameter_mm / 2.0,
                            [x, y, pad_top],
                            p.pad_diameter_mm / 2.0,
                            p.sides,
                        ),
                    ));
                    if p.cone_height_mm > 0.0 {
                        base_parts.push((
                            SupportPart::Cone,
                            false,
                            frustum(
                                [x, y, pad_top],
                                p.cone_diameter_mm / 2.0,
                                b,
                                r_bottom,
                                p.sides,
                            ),
                        ));
                    }
                    b
                }
            }
            BaseSpec::Model { point_mm, axis } => {
                let foot = self.anchor(
                    id,
                    SupportPart::Foot,
                    point_mm,
                    p.foot_contact_diameter_mm / 2.0,
                )?;
                interior_foot = Some(foot.interior);
                let b_axis = self.axis(id, SupportPart::Foot, axis, &foot, point_mm, last)?;
                self.check_piercing(
                    id,
                    SupportPart::Foot,
                    point_mm,
                    b_axis,
                    p.foot_penetration_mm,
                );
                let f = add(point_mm, scale(b_axis, p.foot_length_mm));
                foot_reach = reach(
                    point_mm,
                    b_axis,
                    foot.normal,
                    (-p.foot_penetration_mm, p.foot_contact_diameter_mm / 2.0),
                    (p.foot_length_mm, p.foot_body_diameter_mm / 2.0),
                    2.0 * self.doc.anchor_tolerance_mm,
                    (
                        p.foot_penetration_mm,
                        NEAR_FOOTPRINTS * p.foot_contact_diameter_mm / 2.0,
                    ),
                    p.sides,
                );
                base_parts.push((
                    SupportPart::Foot,
                    true,
                    frustum(
                        sub(point_mm, scale(b_axis, p.foot_penetration_mm)),
                        p.foot_contact_diameter_mm / 2.0,
                        f,
                        p.foot_body_diameter_mm / 2.0,
                        p.sides,
                    ),
                ));
                f
            }
            BaseSpec::Support { parent, z_mm } => {
                let Some((x, _)) = self.paths.get(&parent).and_then(|path| path.at(z_mm)) else {
                    self.error(
                        id,
                        SupportPart::Junction,
                        FindingKind::InvalidParent,
                        format!("support {parent} is invalid or its shaft does not reach Z {z_mm}"),
                    );
                    return None;
                };
                base_parts.push((SupportPart::Junction, false, sphere(x, r_bottom, p.sides)));
                x
            }
        };
        path_points.push(base_join);
        // Shaft: radius linear in arc length from the top to the bottom radius.
        let lengths: Vec<f64> = path_points
            .windows(2)
            .map(|w| length(sub(w[1], w[0])))
            .collect();
        if let Some(i) = lengths.iter().position(|&l| l <= 1e-9) {
            self.error(
                id,
                SupportPart::Shaft,
                FindingKind::InvalidPath,
                format!("path points {i} and {} coincide", i + 1),
            );
            return None;
        }
        let total: f64 = lengths.iter().sum();
        let mut radii = vec![r_top];
        let mut run = 0.0;
        for l in &lengths {
            run += l;
            radii.push(r_top + (r_bottom - r_top) * (run / total));
        }
        for (i, w) in path_points.windows(2).enumerate() {
            let rise = (w[0][2] - w[1][2]).abs();
            // More than 45° from vertical: the rise is below cos 45° of the length.
            if rise < lengths[i] * std::f64::consts::FRAC_1_SQRT_2 {
                self.findings.push(SupportFinding {
                    entity: id,
                    part: SupportPart::Shaft,
                    kind: FindingKind::SteepShaft,
                    severity: Severity::Warning,
                    detail: format!("shaft segment {i} is more than 45° from vertical"),
                });
            }
            parts.push((
                SupportPart::Shaft,
                false,
                frustum(w[0], radii[i], w[1], radii[i + 1], p.sides),
            ));
        }
        for (i, &point) in path_points.iter().enumerate() {
            let mut r = radii[i];
            if i == 0 {
                r = r.max(r_body);
            }
            parts.push((SupportPart::Joint, false, sphere(point, r, p.sides)));
        }
        parts.extend(base_parts);
        // A support with an error whose shape is still defined (its axis, its penetration) is
        // built and reported, never dropped (spec section 5): the caller sees it, and decides
        // whether it is sliced.
        let count = parts.len();
        for (part, exempt, shape) in parts {
            self.push(s, part, exempt, shape);
        }
        self.set_reach(id, SupportPart::Contact, tip_reach);
        if foot_reach.is_some() {
            self.set_reach(id, SupportPart::Foot, foot_reach);
        }
        self.paths.insert(
            id,
            Path {
                points: path_points,
                radii,
            },
        );
        Some(BuiltSupport {
            id,
            kind: s.kind,
            base: kind,
            interior_tip: anchor.interior,
            interior_foot,
            parts: count,
            contact_diameter_mm: Some(p.contact_diameter_mm),
        })
    }

    /// A mini support (specification, section 4): a rod of the mini radius `r` along its path,
    /// from a ball at its tip to a ball at its foot on the model, or to its junction on a parent's
    /// shaft. Each ball's centre lies `r − p` out of the surface along its axis, so it reaches the
    /// mini penetration `p` below it.
    ///
    /// A mini support may touch the model anywhere (owner, 2026-10-30: its rod rests among the
    /// model's parts, and where they overlap the resin is one): no part of it keeps the
    /// clearance. Where its axis crosses the model, a warning says so. Within `2(r + clearance)`
    /// of an end on the model, along the path, the rod is that end's part (Contact or Foot), as
    /// a tip is, for what shows and picks a tip.
    #[allow(clippy::too_many_lines)] // One support, step by step as specified.
    fn mini(&mut self, s: &ManualSupport) -> Option<BuiltSupport> {
        let id = s.id;
        let p = &s.parameters;
        let r = p.mini_diameter_mm / 2.0;
        let depth = p.mini_penetration_mm;
        let tip = s.tip_mm;
        let anchor = self.anchor(id, SupportPart::Contact, tip, r)?;
        let target = self.straight_target(s).unwrap_or(sub(tip, [0.0, 0.0, 1.0]));
        let a = self.axis(id, SupportPart::Contact, s.tip_axis, &anchor, tip, target)?;
        self.check_piercing(id, SupportPart::Contact, tip, a, depth);
        let top = add(tip, scale(a, r - depth));
        let mut points = vec![top];
        points.extend_from_slice(&s.bends);
        let last = *points.last().expect("the path starts at the tip's ball");
        let (kind, interior_foot, bottom) = match s.base {
            BaseSpec::Model { point_mm, axis } => {
                let foot = self.anchor(id, SupportPart::Foot, point_mm, r)?;
                let b = self.axis(id, SupportPart::Foot, axis, &foot, point_mm, last)?;
                self.check_piercing(id, SupportPart::Foot, point_mm, b, depth);
                (
                    BaseKind::Model,
                    Some(foot.interior),
                    add(point_mm, scale(b, r - depth)),
                )
            }
            BaseSpec::Support { parent, z_mm } => {
                let Some((x, _)) = self.paths.get(&parent).and_then(|path| path.at(z_mm)) else {
                    self.error(
                        id,
                        SupportPart::Junction,
                        FindingKind::InvalidParent,
                        format!("support {parent} is invalid or its shaft does not reach Z {z_mm}"),
                    );
                    return None;
                };
                (BaseKind::Support, None, x)
            }
            BaseSpec::Platform(_) | BaseSpec::Vertical => {
                self.error(
                    id,
                    SupportPart::Foot,
                    FindingKind::InvalidPath,
                    "a mini support stands on the model or on another support".to_owned(),
                );
                return None;
            }
        };
        points.push(bottom);
        let lengths: Vec<f64> = points.windows(2).map(|w| length(sub(w[1], w[0]))).collect();
        if let Some(i) = lengths.iter().position(|&l| l <= 1e-9) {
            self.error(
                id,
                SupportPart::Shaft,
                FindingKind::InvalidPath,
                format!("path points {i} and {} coincide", i + 1),
            );
            return None;
        }
        // The foot prints first and holds the tip: it must lie below it.
        if bottom[2] >= top[2] {
            self.error(
                id,
                SupportPart::Foot,
                FindingKind::InvalidPath,
                format!(
                    "the foot's ball at Z {} is not below the tip's (Z {})",
                    bottom[2], top[2]
                ),
            );
            return None;
        }
        let total: f64 = lengths.iter().sum();
        let reach = 2.0 * (r + self.doc.clearance_mm);
        let on_model = kind == BaseKind::Model;
        // The stretches by arc length: the tip's, the shaft's, and the foot's on the model.
        let top_end = reach.min(total);
        let foot_start = if on_model {
            (total - reach).max(top_end)
        } else {
            total
        };
        let mut parts = vec![(SupportPart::Contact, true, sphere(top, r, p.sides))];
        let mut through = Vec::new();
        let tau = self.doc.anchor_tolerance_mm;
        let mut run = 0.0;
        for (i, w) in points.windows(2).enumerate() {
            let rise = (w[0][2] - w[1][2]).abs();
            if rise < lengths[i] * std::f64::consts::FRAC_1_SQRT_2 {
                self.findings.push(SupportFinding {
                    entity: id,
                    part: SupportPart::Shaft,
                    kind: FindingKind::SteepShaft,
                    severity: Severity::Warning,
                    detail: format!("shaft segment {i} is more than 45° from vertical"),
                });
            }
            let (from, to) = (run, run + lengths[i]);
            let at = |t: f64| add(w[0], scale(sub(w[1], w[0]), (t - from) / lengths[i]));
            let mut cuts = vec![from];
            cuts.extend(
                [top_end, foot_start]
                    .into_iter()
                    .filter(|&c| from < c && c < to),
            );
            cuts.push(to);
            for c in cuts.windows(2) {
                if c[1] - c[0] <= 1e-9 {
                    continue;
                }
                let middle = f64::midpoint(c[0], c[1]);
                let part = if middle < top_end {
                    SupportPart::Contact
                } else if middle > foot_start {
                    SupportPart::Foot
                } else {
                    SupportPart::Shaft
                };
                // Its axis, short of the balls' centres near the surface.
                let lo = c[0].max(2.0 * tau);
                let hi = if on_model {
                    c[1].min(total - 2.0 * tau)
                } else {
                    c[1]
                };
                if lo < hi {
                    through.push((part, at(lo), at(hi)));
                }
                parts.push((part, true, frustum(at(c[0]), r, at(c[1]), r, p.sides)));
            }
            run = to;
        }
        for &bend in &s.bends {
            parts.push((SupportPart::Joint, true, sphere(bend, r, p.sides)));
        }
        parts.push(if on_model {
            (SupportPart::Foot, true, sphere(bottom, r, p.sides))
        } else {
            (SupportPart::Junction, true, sphere(bottom, r, p.sides))
        });
        // The first crossing, said once.
        let crossing = through.into_iter().find_map(|(part, from, to)| {
            let lo = [0, 1, 2].map(|k| from[k].min(to[k]));
            let hi = [0, 1, 2].map(|k| from[k].max(to[k]));
            self.index
                .candidates(lo, hi)
                .into_iter()
                .find(|&t| segment_meets_triangle(from, to, self.index.corners(t)))
                .map(|t| (part, t))
        });
        if let Some((part, t)) = crossing {
            self.findings.push(SupportFinding {
                entity: id,
                part,
                kind: FindingKind::Collision,
                severity: Severity::Warning,
                detail: format!("its axis crosses the model (triangle {t})"),
            });
        }
        let count = parts.len();
        for (part, exempt, shape) in parts {
            self.push(s, part, exempt, shape);
        }
        let radii = vec![r; points.len()];
        self.paths.insert(id, Path { points, radii });
        Some(BuiltSupport {
            id,
            kind: s.kind,
            base: kind,
            interior_tip: anchor.interior,
            interior_foot,
            parts: count,
            contact_diameter_mm: None,
        })
    }

    fn brace(&mut self, brace: &super::Brace) -> bool {
        let end = |(support, z): (u64, f64)| self.paths.get(&support).and_then(|p| p.at(z));
        let (Some((a, ra)), Some((b, rb))) = (end(brace.from), end(brace.to)) else {
            self.error(
                brace.id,
                SupportPart::Brace,
                FindingKind::InvalidParent,
                format!(
                    "brace ends must lie on valid support shafts (support {} at Z {}, support {} at Z {})",
                    brace.from.0, brace.from.1, brace.to.0, brace.to.1
                ),
            );
            return false;
        };
        if length(sub(b, a)) <= 1e-9 {
            self.error(
                brace.id,
                SupportPart::Brace,
                FindingKind::InvalidPath,
                "brace ends coincide".to_owned(),
            );
            return false;
        }
        // Automatic: the thinner shaft where the brace meets it, up to its largest.
        let r = brace.radius_mm(ra, rb);
        for shape in [frustum(a, r, b, r, 16), sphere(a, r, 16), sphere(b, r, 16)] {
            self.parts.push(Owned {
                entity: brace.id,
                part: SupportPart::Brace,
                accepted: false,
                exempt: false,
                reach: None,
                shape,
            });
        }
        true
    }
}

/// The sign of the 2D cross product of `a - o` and `b - o`, exact: orient3d with a lifted
/// fourth point.
fn turn_2d(o: [f64; 2], a: [f64; 2], b: [f64; 2]) -> Ordering {
    crate::preparation::hollow::numeric::orient3d(
        Point3::new(o[0], o[1], 0.0),
        Point3::new(a[0], a[1], 0.0),
        Point3::new(b[0], b[1], 0.0),
        Point3::new(o[0], o[1], 1.0),
    )
}

/// The convex hull of 2D points by the monotone chain with exact orientation signs, counter-
/// clockwise without collinear points.
fn hull_2d(mut points: Vec<[f64; 2]>) -> Vec<[f64; 2]> {
    points.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
    points.dedup();
    if points.len() < 3 {
        return points;
    }
    let turn = turn_2d;
    let mut hull: Vec<[f64; 2]> = Vec::new();
    for pass in 0..2 {
        let start = hull.len();
        let iter: Box<dyn Iterator<Item = &[f64; 2]>> = if pass == 0 {
            Box::new(points.iter())
        } else {
            Box::new(points.iter().rev())
        };
        for &p in iter {
            while hull.len() >= start + 2
                && turn(hull[hull.len() - 2], hull[hull.len() - 1], p) != Ordering::Greater
            {
                hull.pop();
            }
            hull.push(p);
        }
        hull.pop();
    }
    hull
}

/// The representative of `k`'s group in the union-find `parent`, halving the path on the way.
fn root(parent: &mut [usize], mut k: usize) -> usize {
    while parent[k] != k {
        parent[k] = parent[parent[k]];
        k = parent[k];
    }
    k
}

/// The groups of the plate feet at `centres` that share a raft, each by the feet's places in
/// order, the groups by their first foot: one group for [`RaftLayout::Single`]; for
/// [`RaftLayout::Groups`], feet closer than its distance, directly or through others. A grid of
/// that size finds the near feet, so the work follows the feet, not their pairs.
fn raft_groups(centres: &[[f64; 2]], layout: RaftLayout) -> Vec<Vec<usize>> {
    let RaftLayout::Groups { join_mm } = layout else {
        return vec![(0..centres.len()).collect()];
    };
    // Cells of the distance's size: feet nearer than it lie in the same or neighbouring cells.
    // Coordinates stay within a build plate, far inside `i64`.
    #[allow(clippy::cast_possible_truncation)]
    let cell = |p: [f64; 2]| p.map(|c| (c / join_mm).floor() as i64);
    let mut cells: BTreeMap<[i64; 2], Vec<usize>> = BTreeMap::new();
    for (k, &p) in centres.iter().enumerate() {
        cells.entry(cell(p)).or_default().push(k);
    }
    let mut parent: Vec<usize> = (0..centres.len()).collect();
    for (k, &p) in centres.iter().enumerate() {
        let [cx, cy] = cell(p);
        for dx in -1..=1 {
            for dy in -1..=1 {
                for &j in cells.get(&[cx + dx, cy + dy]).into_iter().flatten() {
                    let q = centres[j];
                    if j > k && (p[0] - q[0]).hypot(p[1] - q[1]) < join_mm {
                        let (a, c) = (root(&mut parent, k), root(&mut parent, j));
                        parent[a.max(c)] = a.min(c);
                    }
                }
            }
        }
    }
    let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for k in 0..centres.len() {
        let r = root(&mut parent, k);
        groups.entry(r).or_default().push(k);
    }
    groups.into_values().collect()
}

/// One raft under `feet` (each a base centre, its fillet's radius and its sides): the hull of
/// their fillets' rings widened by `spec`'s margin at its top, and at the plate widened further
/// by its slope's spread (a slab) or narrowed by it (a skate, TD-144). A skate narrower than
/// [`SKATE_MIN_PLATE_RADIUS_MM`] at the plate under a foot fails: it would stand on a point or
/// turn inside out.
fn raft_part(
    feet: &[(V, f64, u16)],
    spec: RaftSpec,
    cancel: &mut Cancellation<'_>,
) -> Result<Owned, SupportError> {
    if let Some(&([x, y, _], r, _)) = feet
        .iter()
        .find(|&&(_, r, _)| spec.plate_radius_mm(r) < SKATE_MIN_PLATE_RADIUS_MM)
    {
        return Err(SupportError::Geometry(format!(
            "the skate raft under the foot at ({:.2}, {:.2}) mm is {:.2} mm wide at the plate, \
             less than {:.1} mm: make it thinner, its sides steeper or its margin wider",
            x,
            y,
            2.0 * spec.plate_radius_mm(r),
            2.0 * SKATE_MIN_PLATE_RADIUS_MM,
        )));
    }
    // Signed: out for a slab, in for a skate.
    let spread = spec.plate_radius_mm(0.0) - spec.margin_mm;
    let ring = |widen: f64| -> Vec<[f64; 2]> {
        let mut out = Vec::new();
        for &(c, r, sides) in feet {
            for k in 0..sides {
                let (s, co) = sin_cos_degrees(360.0 * f64::from(k) / f64::from(sides));
                out.push([c[0] + (r + widen) * co, c[1] + (r + widen) * s]);
            }
        }
        out
    };
    let top = hull_2d(ring(spec.margin_mm));
    let bottom = hull_2d(ring(spec.margin_mm + spread));
    let points: Vec<Point3> = top
        .iter()
        .map(|p| Point3::new(p[0], p[1], spec.thickness_mm))
        .chain(bottom.iter().map(|p| Point3::new(p[0], p[1], 0.0)))
        .collect();
    let limits = HollowLimits {
        max_input_vertices: 1 << 14,
        max_planes: 1 << 15,
        ..HollowLimits::default()
    };
    let mesh = convex_component(points, limits, cancel)
        .map_err(|e| SupportError::Geometry(format!("raft hull: {e}")))?;
    let vertices: Vec<V> = mesh.vertices().iter().map(|p| [p.x, p.y, p.z]).collect();
    let centre =
        [0, 1].map(|k| vertices.iter().map(|v| v[k]).sum::<f64>() / as_f64(vertices.len()));
    let radius = vertices
        .iter()
        .map(|v| ((v[0] - centre[0]).powi(2) + (v[1] - centre[1]).powi(2)).sqrt())
        .fold(0.0, f64::max);
    Ok(Owned {
        entity: 0,
        part: SupportPart::Raft,
        accepted: false,
        exempt: false,
        reach: None,
        shape: Part {
            vertices,
            triangles: mesh.triangles().to_vec(),
            axis: [
                [centre[0], centre[1], 0.0],
                [centre[0], centre[1], spec.thickness_mm],
            ],
            radius,
            taper: [radius; 2],
        },
    })
}

/// Whether a part (its `shape` within `bounds`) is clear of `channel` (see [`channel`]): their
/// boxes lie apart, or else the part's tapered capsule is clear of the channel's cylinder. The
/// boxes come first because a capsule is tight for a shaft but not for a raft: a flat slab's
/// capsule is a ball as wide as the raft, which would reach drain holes far above it.
pub(super) fn clear_of_drain(shape: &Part, [lo, hi]: [V; 2], channel: &DrainCylinder) -> bool {
    let reach = channel.radius_mm + PROOF_MARGIN_MM;
    let apart = (0..3).any(|k| {
        let low = channel.start_mm[k].min(channel.end_mm[k]) - reach;
        let high = channel.start_mm[k].max(channel.end_mm[k]) + reach;
        hi[k] < low || lo[k] > high
    });
    apart || clear_of_tapered(shape, channel.start_mm, channel.end_mm, channel.radius_mm)
}

/// The channel of `drain` through the model indexed by `index` (TD-123): its cylinder over each
/// stretch of its axis inside the material, from where the axis enters the surface to where it
/// leaves it, or to its end when it ends in the material. Material there would plug the hole;
/// outside it (the entry's margin before the surface, a cavity it crosses, the margin past a
/// cavity wall) it does not, since the slicer cuts only the model. Empty when the axis meets no
/// surface.
///
/// The surface is closed and outward, and a hole starts outside the material, as drainage
/// requires: a crossing against a face's normal enters it, along it leaves. Crossings at one
/// point in one direction (an edge shared by two faces) count once; an axis that only touches
/// the surface enters and leaves at once.
pub(super) fn channel(index: &TriangleIndex<'_>, drain: &DrainCylinder) -> Vec<DrainCylinder> {
    let (p, q) = (drain.start_mm, drain.end_mm);
    let along = sub(q, p);
    let lo = [0, 1, 2].map(|k| p[k].min(q[k]));
    let hi = [0, 1, 2].map(|k| p[k].max(q[k]));
    let mut crossings: Vec<(f64, i32)> = index
        .candidates(lo, hi)
        .into_iter()
        .filter_map(|t| {
            let corners = index.corners(t);
            if !segment_meets_triangle(p, q, corners) {
                return None;
            }
            // Where the axis meets the triangle's plane; a coplanar axis gives no single point.
            let n = normal(corners)?;
            let facing = dot(n, along);
            let at = (dot(n, sub(corners[0], p)) / facing).clamp(0.0, 1.0);
            (facing.abs() > 1e-12).then_some((at, if facing < 0.0 { 1 } else { -1 }))
        })
        .collect();
    crossings.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    crossings.dedup_by(|b, a| a.1 == b.1 && (b.0 - a.0).abs() <= 1e-9);
    let point = |t: f64| add(p, scale(along, t));
    let mut channels = Vec::new();
    let (mut depth, mut from) = (0i32, 0.0);
    for (at, step) in crossings {
        let was = depth > 0;
        depth += step;
        match (was, depth > 0) {
            (false, true) => from = at,
            (true, false) if at > from => channels.push(DrainCylinder {
                start_mm: point(from),
                end_mm: point(at),
                radius_mm: drain.radius_mm,
            }),
            _ => {}
        }
    }
    if depth > 0 && from < 1.0 {
        channels.push(DrainCylinder {
            start_mm: point(from),
            end_mm: q,
            radius_mm: drain.radius_mm,
        });
    }
    channels
}

/// The supports' shafts as [`build_supports`] builds them: the surface's index, each valid
/// support's path by its ID, and the plate floor (the raft's top, or zero). Braces planned
/// along them join them where a build will.
pub(super) struct Shafts<'a> {
    pub(super) index: TriangleIndex<'a>,
    pub(super) paths: BTreeMap<u64, Path>,
    pub(super) floor: f64,
}

/// The shafts of `doc`'s supports against `surface`: see [`Shafts`].
///
/// # Errors
/// Resource limits and cancellation.
pub(super) fn shafts<'a>(
    doc: &'a SupportDocument,
    surface: SupportSurface<'a>,
    cancel: &mut Cancellation<'_>,
) -> Result<Shafts<'a>, SupportError> {
    cancel.check(Checkpoint::SupportStart)?;
    let index = TriangleIndex::build(surface.mesh, cancel)?;
    let mut b = Builder {
        doc,
        surface,
        index,
        parts: Vec::new(),
        findings: Vec::new(),
        paths: BTreeMap::new(),
        cancel,
        floor: doc.raft.map_or(0.0, |r| r.thickness_mm),
        cos_deviation: sin_cos_degrees(doc.max_axis_deviation_deg).1,
    };
    for (i, s) in doc.supports.iter().enumerate() {
        b.cancel.poll(Checkpoint::SupportWork, i)?;
        // Only the path is wanted: the parts it makes are dropped.
        let _ = b.support(s);
        b.parts.clear();
    }
    Ok(Shafts {
        index: b.index,
        paths: b.paths,
        floor: b.floor,
    })
}

/// Build and validate the supports of `doc` against `surface`.
///
/// Parts that fail validation are omitted from the mesh and reported; check
/// [`SupportScene::has_errors`] before slicing.
///
/// # Errors
/// A raft hull that cannot be built, resource limits and cancellation.
#[allow(clippy::too_many_lines)] // The specified build sequence.
pub fn build_supports(
    doc: &SupportDocument,
    surface: SupportSurface<'_>,
    drains: &[DrainCylinder],
    volume: BuildVolume,
    options: SupportOptions,
    cancel: &mut Cancellation<'_>,
) -> Result<SupportScene, SupportError> {
    cancel.check(Checkpoint::SupportStart)?;
    let index = TriangleIndex::build(surface.mesh, cancel)?;
    let mut b = Builder {
        doc,
        surface,
        index,
        parts: Vec::new(),
        findings: doc.warnings.clone(),
        paths: BTreeMap::new(),
        cancel,
        floor: doc.raft.map_or(0.0, |r| r.thickness_mm),
        cos_deviation: sin_cos_degrees(doc.max_axis_deviation_deg).1,
    };
    let mut supports = Vec::with_capacity(doc.supports.len());
    for (i, s) in doc.supports.iter().enumerate() {
        b.cancel.poll(Checkpoint::SupportWork, i)?;
        // A foot on the model where a hole cuts it away stands on nothing (TD-131).
        if let BaseSpec::Model { point_mm, .. } = s.base {
            if let Some(k) = drains.iter().position(|d| d.removes(point_mm)) {
                b.findings.push(finding(
                    s.id,
                    SupportPart::Foot,
                    FindingKind::FootInHole,
                    format!(
                        "stands at {} where drain hole {} cuts the model away: nothing is under it",
                        fmt_point(point_mm),
                        k + 1
                    ),
                ));
            }
        }
        let built = b.support(s).unwrap_or(BuiltSupport {
            id: s.id,
            kind: s.kind,
            base: match s.base {
                BaseSpec::Platform(_) | BaseSpec::Vertical => BaseKind::Platform,
                BaseSpec::Model { .. } => BaseKind::Model,
                BaseSpec::Support { .. } => BaseKind::Support,
            },
            interior_tip: false,
            interior_foot: None,
            parts: 0,
            contact_diameter_mm: None,
        });
        supports.push(built);
    }
    let braces = doc.braces.iter().filter(|brace| b.brace(brace)).count();
    // Rafts: the hull of the plate feet's fillets, widened by the margin at the top and by the
    // slope below; one for every foot, or one for each group of feet.
    let mut rafts = 0;
    if let Some(spec) = doc.raft {
        let feet: Vec<(V, f64, u16)> = doc
            .supports
            .iter()
            .zip(&supports)
            .filter(|(_, built)| built.parts > 0 && built.base == BaseKind::Platform)
            .filter_map(|(s, _)| {
                let path = b.paths.get(&s.id)?;
                let base = *path.points.last()?;
                let p = &s.parameters;
                let shaft = p.shaft_bottom_diameter_mm.unwrap_or(p.shaft_diameter_mm);
                Some((base, spec.fillet_diameter_mm.max(shaft) / 2.0, p.sides))
            })
            .collect();
        if feet.is_empty() {
            b.error(
                0,
                SupportPart::Raft,
                FindingKind::EmptyRaft,
                "a raft needs at least one valid plate support".to_owned(),
            );
        } else if spec.shape == RaftShape::Web {
            // A disc under each foot and bars between neighbours (TD-144).
            let built = web::web(&feet, spec, b.cancel)?;
            b.parts.extend(built.parts);
            rafts = built.pieces;
        } else {
            let centres: Vec<[f64; 2]> = feet.iter().map(|(c, _, _)| [c[0], c[1]]).collect();
            for group in raft_groups(&centres, spec.layout) {
                let members: Vec<(V, f64, u16)> = group.iter().map(|&k| feet[k]).collect();
                let part = raft_part(&members, spec, b.cancel)?;
                b.parts.push(part);
                rafts += 1;
            }
        }
    }
    // Clearance, drain holes and the build volume, part by part. Only a hole's channel through
    // the model can be plugged (TD-123), which is only a warning (TD-131).
    let channels: Vec<Vec<DrainCylinder>> = drains.iter().map(|d| channel(&b.index, d)).collect();
    let clearance = doc.clearance_mm;
    let mut checks = Vec::new();
    for (i, owned) in b.parts.iter().enumerate() {
        b.cancel.poll(Checkpoint::SupportWork, i)?;
        let shape = &owned.shape;
        let lo = [0, 1, 2].map(|k| {
            shape
                .vertices
                .iter()
                .map(|v| v[k])
                .fold(f64::INFINITY, f64::min)
        });
        let hi = [0, 1, 2].map(|k| {
            shape
                .vertices
                .iter()
                .map(|v| v[k])
                .fold(f64::NEG_INFINITY, f64::max)
        });
        if let Some(reach) = owned.reach.as_ref().filter(|_| owned.exempt) {
            // Away from where it touches, a contact or foot must not enter the model (TD-124).
            let [lo, hi] = reach.bounds();
            if let Some(t) = b
                .index
                .candidates(lo, hi)
                .into_iter()
                .find(|&t| reach.meets(b.index.corners(t)))
            {
                let accepted = owned.accepted || options.accept_all_collisions;
                checks.push(SupportFinding {
                    entity: owned.entity,
                    part: owned.part,
                    kind: FindingKind::Collision,
                    severity: if accepted { Severity::Accepted } else { Severity::Error },
                    detail: format!(
                        "meets the model away from where it touches it (triangle {t}): the {} crosses the surface beside it",
                        if owned.part == SupportPart::Foot { "foot" } else { "tip" }
                    ),
                });
            }
        }
        if !owned.exempt {
            let grow = |v: V, d: f64| v.map(|c| c + d);
            if let Some(t) = b
                .index
                .candidates(grow(lo, -clearance), grow(hi, clearance))
                .into_iter()
                .find(|&t| !proven_clear(shape, b.index.corners(t), clearance))
            {
                let accepted = owned.accepted || options.accept_all_collisions;
                let distance = shape
                    .vertices
                    .iter()
                    .map(|&v| point_triangle_distance(v, b.index.corners(t)))
                    .fold(f64::INFINITY, f64::min);
                checks.push(SupportFinding {
                    entity: owned.entity,
                    part: owned.part,
                    kind: FindingKind::Collision,
                    severity: if accepted { Severity::Accepted } else { Severity::Error },
                    detail: format!(
                        "not proven {clearance} mm clear of the model (triangle {t}; nearest vertex {distance:.4} mm away)"
                    ),
                });
            }
        }
        // A warning, not an error (TD-131): a hole often makes an island at its own rim, and
        // holding the island comes first; the hole stays partly open around the support.
        for (k, channel) in channels.iter().enumerate() {
            if channel.iter().any(|c| !clear_of_drain(shape, [lo, hi], c)) {
                checks.push(SupportFinding {
                    entity: owned.entity,
                    part: owned.part,
                    kind: FindingKind::DrainHole,
                    severity: Severity::Warning,
                    detail: format!(
                        "enters drain hole {} through the model and may partly plug it",
                        k + 1
                    ),
                });
            }
        }
        let [ox, oy] = volume.origin_xy_mm;
        let [w, d, h] = volume.size_mm;
        if lo[0] < ox || lo[1] < oy || hi[0] > ox + w || hi[1] > oy + d || lo[2] < 0.0 || hi[2] > h
        {
            checks.push(finding(
                owned.entity,
                owned.part,
                FindingKind::BuildVolume,
                format!(
                    "extends to [{}, {}] x [{}, {}] x [{}, {}] mm, outside the build volume",
                    lo[0], hi[0], lo[1], hi[1], lo[2], hi[2]
                ),
            ));
        }
    }
    b.findings.extend(checks);
    // The combined mesh.
    let mut vertices = Vec::new();
    let mut triangles = Vec::new();
    let mut ranges = Vec::with_capacity(b.parts.len());
    for owned in &b.parts {
        let start = triangles.len();
        let offset = u32::try_from(vertices.len())
            .map_err(|_| SupportError::Resource("support vertices"))?;
        vertices.extend(
            owned
                .shape
                .vertices
                .iter()
                .map(|v| Point3::new(v[0], v[1], v[2])),
        );
        for t in &owned.shape.triangles {
            triangles.push(t.map(|i| i + offset));
        }
        ranges.push(ScenePart {
            entity: owned.entity,
            part: owned.part,
            triangles: start..triangles.len(),
        });
    }
    let parts = b.parts.len();
    let mesh = TriangleMesh::try_new_with_cancellation(vertices, triangles, b.cancel)
        .map_err(|e| SupportError::Geometry(format!("support mesh: {e}")))?;
    Ok(SupportScene {
        mesh,
        ranges,
        supports,
        braces,
        rafts,
        parts,
        findings: b.findings,
        paths: b.paths,
    })
}

fn as_f64(n: usize) -> f64 {
    #[allow(clippy::cast_precision_loss)] // Part vertex counts are tiny.
    let value = n as f64;
    value
}
