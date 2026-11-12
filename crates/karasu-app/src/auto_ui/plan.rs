//! Every support of a model planned at once (M6.4i, owner mockup "Automatic supports tool",
//! 2026-10-30).
//!
//! **What needs support** is the per-layer analysis's (TD-107): its islands, and with "Islands
//! and overhangs" its overhangs too, found on the model sliced alone.
//!
//! **Islands** are supported as Support the islands supports them ([`crate::islands_ui::plan`]):
//! close ones share a support, a large one takes several. An island a support placed by hand
//! already holds is left to it.
//!
//! **Overhangs.** Each region's footprint is probed on a grid half its tip spacing apart, a
//! vertical line up from under the region to its first surface. A point is a candidate where
//! that surface faces down less steeply than the critical angle. A point a drain hole cuts away
//! is none: the surface probed there is removed, with nothing to hold (TD-131). Each candidate
//! wants a tip within its reach: the tip spacing under a level surface, growing in proportion to
//! the slope up to the spacing at the critical angle.
//!
//! **Edges and corners** (owner, 2026-11-06, TD-136): where the underside ends, its outline
//! takes candidates of its own [`EDGE_INSET_MM`] in from the edge, and from both edges at a
//! corner; beside a leading edge, which the underside rises away from, the face beside it takes
//! side tips ([`edges`]); a leading edge's own tips stand the contact's radius from it, as close
//! as supports placed by hand. Candidates are taken corners first, then edges, then the inside,
//! each lowest first, and a candidate becomes a tip when no tip (the islands', those placed by
//! hand, and those taken before) lies within its reach. Side tips go between the tips taken
//! (half their reach from them) and their reach apart, and one that finds no support is
//! reported as a place left unsupported.
//!
//! **Density** (owner, 2026-11-04, TD-127): the reach is scaled by `sqrt(100 / density)`, so
//! the density is the tips per area as a percentage of the spacing's. With **Closer under wide
//! undersides** on, a region whose underside spans more than the area set (an island's, or an
//! overhang's, over its layers: what the supports hold) takes its tips closer, by
//! `sqrt(from / area)` and no closer than the share set: a wide surface pulls on the film and
//! sags more, a small one keeps the spacing.
//!
//! **Where** (owner, 2026-10-30): on a hollowed model the plan covers everywhere, only the
//! outside or only the cavity; islands and candidates elsewhere are left out, and Generate
//! replaces only the automatic supports of the place it covers (see [`Place::holds`]).
//!
//! **Growing** is shared with the Islands tool ([`crate::islands_ui::plan::grow`]): tips close
//! together share trunks, each routed down as placing routes it, then trunks merge into trees.
//!
//! **The closing check.** A tip whose support finds no way down leaves its candidates without
//! one. Each pass tries the next candidates that are still uncovered, lowest first, never a point
//! tried before, up to the passes asked for or until a pass adds nothing. Whatever is still
//! uncovered is reported as places left unsupported, each with why. The check is geometric:
//! slicing again with the supports would not tell it, since the per-layer analysis judges a
//! layer's self-support (a 0.05 mm reach at 45°), which the gap between any two tips exceeds.
//!
//! **The final check.** The supports are built as a slice builds them, against the model, with
//! the kept ones. A planned support built with an error (its tip piercing a thin part, a part
//! too close to the model, a path the build refuses) goes; what hung from it is grown again on
//! supports of its own and checked in the next round. Its place is reported as left, with why.
//!
//! **Braces**, when asked for, are the Braces tool's automatic ones over the kept supports and
//! the planned ones together.
//!
//! The plan is deterministic for equal inputs: candidates and passes follow a fixed order, and
//! growing is ([`crate::islands_ui::plan::grow`]).
use crate::{
    gizmo::Affine,
    islands_ui::{
        plan::{
            self, add_planned,
            check::{self, check_built, distance, held, Kept},
            groups, grow, hold_every_start, Growing, IslandSupports, Outcome, Skip, Starts, Tip,
        },
        Island, PROBE_MM,
    },
    supports_ui::{route::RouteSurface, NextSupport, SupportPreset, Weight},
};
use edges::Rank;
use karasu_core::{
    analysis::unsupported::RegionKind,
    mesh::TriangleMesh,
    supports::{Brace, BuildVolume, DrainCylinder, SupportDocument, SupportOrigin},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeSet, HashMap},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

/// The tip spacing under a level surface the panel accepts, in mm.
pub const SPACING_MM: std::ops::RangeInclusive<f64> = 0.5..=20.0;
/// The lifts the panel accepts, in mm (TD-128).
pub const LIFT_MM: std::ops::RangeInclusive<f64> = 0.0..=50.0;
/// The densities the panel accepts, in percent of the tips the spacing gives (TD-127).
pub const DENSITY_PCT: std::ops::RangeInclusive<f64> = 50.0..=200.0;
/// The underside areas from which tips stand closer that the panel accepts, in mm².
pub const WIDE_FROM_MM2: std::ops::RangeInclusive<f64> = 1.0..=1000.0;
/// The closest share of the spacing under a wide underside the panel accepts, in percent.
pub const WIDE_CLOSEST_PCT: std::ops::RangeInclusive<f64> = 20.0..=100.0;
/// The closing check's passes the panel accepts.
pub const PASSES: std::ops::RangeInclusive<f64> = 0.0..=10.0;
/// The finest grid an overhang is probed on, in mm.
const FINEST_PROBE_MM: f64 = 0.25;
/// How far in from an underside's edge its edge and corner tips stand, in mm (TD-136): the test
/// tiles held flat on tips 0.4 mm in, and curled or fell with their outer tips 1.25 mm in.
pub const EDGE_INSET_MM: f64 = 0.4;
/// A side tip is held when a planned tip stands this close to it, in mm.
const SIDE_HELD_MM: f64 = 0.01;

/// What the Automatic tool covers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Cover {
    /// Islands and overhangs: everything the analysis finds.
    #[default]
    All,
    /// Only the islands, as Support the islands supports them.
    IslandsOnly,
}

/// Where on a hollowed model the Automatic tool plans (owner, 2026-10-30).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Place {
    /// Outside and inside the cavity.
    #[default]
    Everywhere,
    /// Only outside: the cavity is left as it is.
    Outside,
    /// Only inside the cavity, from its ceiling to its floor: the outside is left as it is.
    Cavity,
}

impl Place {
    /// Whether a place in the cavity (`cavity`) or outside it is covered.
    #[must_use]
    pub const fn holds(self, cavity: bool) -> bool {
        match self {
            Self::Everywhere => true,
            Self::Outside => !cavity,
            Self::Cavity => cavity,
        }
    }
}

/// `place`, or as the settings before it held it: "inside" on or off.
fn place_or_inside<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Place, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Saved {
        Place(Place),
        Inside(bool),
    }
    Ok(match Saved::deserialize(d)? {
        Saved::Place(place) => place,
        Saved::Inside(true) => Place::Everywhere,
        Saved::Inside(false) => Place::Outside,
    })
}

/// How the Automatic tool plans: tool values, from Settings › Supports each time the tool
/// opens (owner's mockup values; feet on the model on, owner 2026-10-30).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
#[allow(clippy::struct_excessive_bools)] // The panel's switches, each its own value.
pub struct AutoSupports {
    /// What is covered.
    pub cover: Cover,
    /// Where on a hollowed model; everywhere on one that is not. Settings that knew only
    /// "inside" read as everywhere (on) or outside (off).
    #[serde(alias = "inside", deserialize_with = "place_or_inside")]
    pub place: Place,
    /// Islands smaller than this get no support, in mm².
    pub skip_under_mm2: f64,
    /// How high the model's lowest point stands above the plate when the plan is made, in mm
    /// (owner, 2026-11-04, TD-128): Generate lifts the model there first.
    pub lift_mm: f64,
    /// The preset: a weight, or the user's preset at this place (the window's, never kept).
    pub weight: Weight,
    #[serde(skip)]
    pub mine: Option<usize>,
    /// Tips stand this far apart under a level surface, in mm.
    pub spacing_mm: f64,
    /// ... and this far apart where the surface is as steep as the critical angle, in mm.
    pub steep_spacing_mm: f64,
    /// The tips per area, in percent of what the spacings give (TD-127).
    pub density_pct: f64,
    /// Tips stand closer under undersides wider than `wide_from_mm2` (TD-127).
    pub wide: bool,
    /// The underside area from which tips stand closer, in mm².
    pub wide_from_mm2: f64,
    /// The closest they stand there, in percent of the spacing.
    pub wide_closest_pct: f64,
    /// Islands closer than this share a support, in mm.
    pub join_mm: f64,
    /// A support whose way down reaches no plate stands on the model below.
    pub feet_on_model: bool,
    /// Where a support finds no room or no way down, a mini support stands on the model below
    /// its tip (owner, 2026-10-30).
    pub minis: bool,
    /// Tips close together share trunks, and trunks merge.
    pub branches: bool,
    /// Tips closer than this in plan share a trunk, in mm.
    pub branch_mm: f64,
    /// The branches a trunk takes at most.
    pub per_trunk: u32,
    /// Trunks closer than this merge, in mm.
    pub merge_mm: f64,
    /// The trunks a group merges at most.
    pub per_group: u32,
    /// How many times trunks merge.
    pub levels: u32,
    /// The Braces tool's automatic braces are added.
    pub braces: bool,
    /// The closing check's passes at most.
    pub passes: u32,
    /// The contacts fit the surface, narrowing down to this diameter at least (TD-116).
    pub contact_fit_min_mm: Option<f64>,
    /// The contacts end in a ball, not a cone, whatever the preset (owner, 2026-11-02).
    pub sphere_contact: bool,
    /// A hollowed model's cavity takes its own preset and density (owner, 2026-11-06,
    /// TD-137): the cavity is not seen and is lighter to hold. Off, it takes the outside's.
    pub cavity_own: bool,
    /// The cavity's preset: a weight, or the user's preset at this place (never kept).
    pub cavity_weight: Weight,
    #[serde(skip)]
    pub cavity_mine: Option<usize>,
    /// The cavity's tips per area, in percent of what the spacings give.
    pub cavity_density_pct: f64,
}

/// The values the Automatic tool starts from when the settings hold none.
pub const DEFAULT_AUTO_SUPPORTS: AutoSupports = AutoSupports {
    cover: Cover::All,
    place: Place::Everywhere,
    skip_under_mm2: 0.0,
    lift_mm: 5.0,
    // Medium, and no braces (owner, 2026-11-06, TD-136): Light held no flat area on the test
    // plates, and braces made the supports hard to remove where none were needed.
    weight: Weight::Medium,
    mine: None,
    spacing_mm: 2.5,
    steep_spacing_mm: 5.0,
    density_pct: 100.0,
    wide: true,
    wide_from_mm2: 20.0,
    wide_closest_pct: 60.0,
    join_mm: 0.0,
    feet_on_model: true,
    minis: true,
    branches: true,
    branch_mm: 5.0,
    per_trunk: 4,
    merge_mm: 10.0,
    per_group: 3,
    levels: 2,
    braces: false,
    passes: 3,
    contact_fit_min_mm: None,
    sphere_contact: false,
    cavity_own: false,
    cavity_weight: Weight::Medium,
    cavity_mine: None,
    cavity_density_pct: 100.0,
};

impl Default for AutoSupports {
    fn default() -> Self {
        DEFAULT_AUTO_SUPPORTS
    }
}

impl AutoSupports {
    /// These values if every one is in its range, else the built-in ones.
    #[must_use]
    pub fn valid(self) -> Self {
        let islands = self.growing().valid() == self.growing();
        if islands
            && SPACING_MM.contains(&self.spacing_mm)
            && SPACING_MM.contains(&self.steep_spacing_mm)
            && self.steep_spacing_mm >= self.spacing_mm
            && LIFT_MM.contains(&self.lift_mm)
            && DENSITY_PCT.contains(&self.density_pct)
            && DENSITY_PCT.contains(&self.cavity_density_pct)
            && WIDE_FROM_MM2.contains(&self.wide_from_mm2)
            && WIDE_CLOSEST_PCT.contains(&self.wide_closest_pct)
            && PASSES.contains(&f64::from(self.passes))
        {
            self
        } else {
            DEFAULT_AUTO_SUPPORTS
        }
    }

    /// How much the density scales the tips' spacing: the tips per area go with its inverse
    /// square.
    #[must_use]
    pub fn density_scale(&self) -> f64 {
        (100.0 / self.density_pct).sqrt()
    }

    /// How much closer the tips stand under an underside of `area_mm2` (TD-127): 1 when wide
    /// undersides are off or it is no wider than the area set, else `sqrt(from / area)`, no
    /// closer than the share set.
    #[must_use]
    pub fn wide_factor(&self, area_mm2: f64) -> f64 {
        if !self.wide || area_mm2 <= self.wide_from_mm2 {
            return 1.0;
        }
        (self.wide_from_mm2 / area_mm2)
            .sqrt()
            .max(self.wide_closest_pct / 100.0)
    }

    /// The farthest any candidate reaches for a tip, in mm: the spacing at the critical angle
    /// as the density scales it.
    #[must_use]
    pub fn widest_reach(&self) -> f64 {
        self.steep_spacing_mm * self.density_scale()
    }

    /// What the supports are made with: the preset's values.
    #[must_use]
    pub fn next(&self, presets: &[SupportPreset]) -> NextSupport {
        self.growing().next(presets)
    }

    /// The values the cavity is planned with, when it takes its own (TD-137): its preset and
    /// density, the rest the outside's, covering the cavity only.
    #[must_use]
    pub fn cavity(&self) -> Option<Self> {
        self.cavity_own.then_some(Self {
            weight: self.cavity_weight,
            mine: self.cavity_mine,
            density_pct: self.cavity_density_pct,
            place: Place::Cavity,
            ..*self
        })
    }

    /// The values the shared growing takes: no branches nor merging with Branches off.
    #[must_use]
    pub fn growing(&self) -> IslandSupports {
        IslandSupports {
            weight: self.weight,
            mine: self.mine,
            join_mm: self.join_mm,
            skip_under_mm2: self.skip_under_mm2,
            foot_on_model: self.feet_on_model,
            minis: self.minis,
            branch_mm: if self.branches { self.branch_mm } else { 0.0 },
            per_trunk: self.per_trunk,
            merge_mm: if self.branches { self.merge_mm } else { 0.0 },
            per_group: self.per_group,
            levels: self.levels,
            contact_fit_min_mm: self.contact_fit_min_mm,
            sphere_contact: self.sphere_contact,
        }
    }
}

/// A point of an overhang that wants a tip within `reach` of it, and how its place was found.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Candidate {
    tip: Tip,
    reach: f64,
    rank: Rank,
}

/// A place left unsupported: where, and why its tips found no support.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Left {
    /// Its lowest point, in world millimetres.
    pub point: [f64; 3],
    /// Its extent: lowest and highest corners, in world millimetres.
    pub bounds: [[f64; 3]; 2],
    /// An island (otherwise part of an overhang).
    pub island: bool,
    /// Why.
    pub why: Skip,
}

/// A finished plan: the supports, what they cover, what is left, and the braces to add.
#[derive(Default)]
pub struct AutoOutcome {
    /// The supports planned, as Support the islands plans them.
    pub plan: Outcome,
    /// Islands held, and islands wanted (held by supports placed by hand excluded).
    pub islands: [usize; 2],
    /// Overhang points covered, and every overhang point: their share approximates the
    /// overhangs' area covered.
    pub overhangs: [usize; 2],
    /// Places left unsupported, lowest first.
    pub left: Vec<Left>,
    /// Tips the closing check added, and the passes it ran.
    pub added: usize,
    pub passes: u32,
    /// Braces between the supports, numbered as [`add_planned`] numbers the supports after the
    /// document's own.
    pub braces: Vec<Brace>,
}

/// Everything a plan is made from.
pub struct Input {
    /// The regions found, in world millimetres, for their overhangs.
    pub regions: Vec<(RegionKind, Island)>,
    /// The islands found, one where each piece starts in mid-air.
    pub islands: Vec<Island>,
    /// The supports the plan keeps: the document without the automatic ones, model frame.
    pub document: Option<SupportDocument>,
    /// The kept supports' tips, in world millimetres.
    pub kept_tips: Vec<[f64; 3]>,
    pub surface: Arc<RouteSurface>,
    pub next: NextSupport,
    pub values: AutoSupports,
    /// What the cavity's supports are made with, when a hollowed model's cavity takes its own
    /// preset and density (TD-137); `None` otherwise.
    pub cavity_next: Option<NextSupport>,
    /// The critical angle, in degrees from the plate.
    pub angle_deg: f64,
    pub volume: BuildVolume,
    /// From world to the model frame.
    pub to_model: Affine,
    /// For the final check and the braces; without it, neither is made.
    pub model: Option<check::ModelInput>,
}

/// A region's underside as the plan probes it: the first surface above within its heights.
struct Underside<'a> {
    surface: &'a RouteSurface,
    mesh: &'a TriangleMesh,
    cuts: &'a [DrainCylinder],
    place: Place,
    heights: [f64; 2],
    angle: f64,
}

impl edges::Probe for Underside<'_> {
    fn between(&self, x: f64, y: f64, from_z: f64, to_z: f64) -> Option<edges::Seen> {
        let (point, normal, t) =
            self.surface
                .grid()
                .first_above(self.mesh, [x, y, from_z], to_z)?;
        Some(edges::Seen {
            point,
            normal,
            cavity: self.surface.is_cavity(t),
            cut: self.cuts.iter().any(|c| c.removes(point)),
        })
    }

    fn look(&self, x: f64, y: f64) -> Option<edges::Seen> {
        let [under, top] = self.heights;
        self.between(x, y, under - 1e-3, top + PROBE_MM)
    }

    fn wanted(&self, seen: &edges::Seen) -> bool {
        // Facing down, less steep than the critical angle, within the region's layers.
        seen.normal[2] < 0.0
            && tilt(seen.normal) < self.angle
            && seen.point[2] >= self.heights[0] - PROBE_MM
            && self.holds(seen)
            && !seen.cut
    }

    fn holds(&self, seen: &edges::Seen) -> bool {
        self.place.holds(seen.cavity)
    }
}

/// How far a surface of outward `normal` faces away from straight down, in radians.
fn tilt(normal: [f64; 3]) -> f64 {
    (-normal[2]).clamp(-1.0, 1.0).acos()
}

/// What `regions` want held: their candidates and the side tips beside their leading edges.
#[derive(Default)]
struct Probed {
    candidates: Vec<Candidate>,
    sides: Vec<Candidate>,
}

/// The overhang candidates of `regions`: see the module documentation and [`edges`]. Corners,
/// then edges, then the inside, each lowest first, ties by plan position; no point twice, and
/// none where one of `cuts` (the drain holes) removes the material: the surface probed there
/// is cut away, with nothing to hold (TD-131). The side tips, lowest first.
fn candidates(
    surface: &RouteSurface,
    mesh: &TriangleMesh,
    regions: &[(RegionKind, Island)],
    cuts: &[DrainCylinder],
    (values, radius): (&AutoSupports, f64),
    angle_deg: f64,
) -> Probed {
    let angle = angle_deg.to_radians();
    let scale = values.density_scale();
    let mut seen = BTreeSet::new();
    let mut out = Probed::default();
    for (_, region) in regions {
        // Its own spacing: the density's, closer under a wide underside (TD-127).
        let closer = scale * values.wide_factor(region.area_mm2);
        let step = (values.spacing_mm * closer / 2.0).max(FINEST_PROBE_MM);
        let underside = Underside {
            surface,
            mesh,
            cuts,
            place: values.place,
            heights: region.heights,
            angle,
        };
        let [[x0, y0], [x1, y1]] = region.footprint;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // A few per mm.
        let count = |extent: f64| (extent / step).floor() as usize + 1;
        let (nx, ny) = (count(x1 - x0), count(y1 - y0));
        #[allow(clippy::cast_precision_loss)] // A few hundred points per side.
        let start = |low: f64, extent: f64, n: usize| low + (extent - step * (n - 1) as f64) / 2.0;
        let mut grid = edges::Grid {
            start: [start(x0, x1 - x0, nx), start(y0, y1 - y0, ny)],
            step,
            nx,
            ny,
            seen: Vec::with_capacity(nx * ny),
            wanted: Vec::with_capacity(nx * ny),
        };
        for j in 0..ny {
            for i in 0..nx {
                #[allow(clippy::cast_precision_loss)] // A few hundred points per side.
                let (x, y) = (
                    grid.start[0] + step * i as f64,
                    grid.start[1] + step * j as f64,
                );
                let look = edges::Probe::look(&underside, x, y);
                grid.wanted.push(
                    look.as_ref()
                        .is_some_and(|s| edges::Probe::wanted(&underside, s)),
                );
                grid.seen.push(look);
            }
        }
        let reach = |s: &edges::Seen| {
            closer
                * (values.spacing_mm
                    + (values.steep_spacing_mm - values.spacing_mm) * (tilt(s.normal) / angle))
        };
        let mut push = |s: &edges::Seen, rank: Rank| {
            #[allow(clippy::cast_possible_truncation)] // A key, rounded to a micrometre.
            let key = s.point.map(|c| (c * 1e3).round() as i64);
            if seen.insert(key) {
                out.candidates.push(Candidate {
                    tip: (s.point, s.normal, s.cavity),
                    reach: reach(s),
                    rank,
                });
            }
        };
        for (s, &wanted) in grid.seen.iter().zip(&grid.wanted) {
            if let Some(s) = s.as_ref().filter(|_| wanted) {
                push(s, Rank::Inside);
            }
        }
        let outline = edges::outline(&underside, &grid, radius);
        for (s, rank) in &outline.tips {
            push(s, *rank);
        }
        // A side tip's reach along its edge is the level spacing's.
        out.sides.extend(outline.sides.iter().map(|s| Candidate {
            tip: (s.point, s.normal, s.cavity),
            reach: closer * values.spacing_mm,
            rank: Rank::Edge,
        }));
    }
    let order = |a: &Candidate, b: &Candidate| {
        let (p, q) = (a.tip.0, b.tip.0);
        a.rank
            .cmp(&b.rank)
            .then(p[2].total_cmp(&q[2]))
            .then(p[0].total_cmp(&q[0]))
            .then(p[1].total_cmp(&q[1]))
    };
    out.candidates.sort_by(order);
    out.sides.sort_by(order);
    out
}

/// Points kept in cells `size` wide, to find those near a point.
struct Near {
    size: f64,
    cells: HashMap<[i64; 3], Vec<[f64; 3]>>,
}

impl Near {
    fn new(size: f64) -> Self {
        Self {
            size: size.max(FINEST_PROBE_MM),
            cells: HashMap::new(),
        }
    }

    #[allow(clippy::cast_possible_truncation)] // Cells of millimetres.
    fn cell(&self, p: [f64; 3]) -> [i64; 3] {
        p.map(|c| (c / self.size).floor() as i64)
    }

    fn add(&mut self, p: [f64; 3]) {
        let cell = self.cell(p);
        self.cells.entry(cell).or_default().push(p);
    }

    /// Whether a point lies within `reach` (at most the cells' size) of `p`.
    fn within(&self, p: [f64; 3], reach: f64) -> bool {
        let [cx, cy, cz] = self.cell(p);
        (-1..=1).any(|dx| {
            (-1..=1).any(|dy| {
                (-1..=1).any(|dz| {
                    self.cells
                        .get(&[cx + dx, cy + dy, cz + dz])
                        .is_some_and(|points| {
                            points.iter().any(|q| {
                                let d = [p[0] - q[0], p[1] - q[1], p[2] - q[2]];
                                (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt() <= reach
                            })
                        })
                })
            })
        })
    }
}

/// Plan every support of `input`: see the module documentation. `stage` is told each stage's
/// start: 0 placing the tips, 1 growing, 2 the closing check's pass, 3 the final check, 4
/// braces.
///
/// **The cavity's own values** (TD-137): when a hollowed model's cavity takes its own preset
/// and density, a plan of the cavity alone is made with them, and a plan of everywhere is two:
/// the outside with the outside's values, then the cavity with its own, joined, with braces
/// over both. Each surface is probed by both, each keeping its own place.
pub fn plan(input: &Input, stage: &dyn Fn(u32), stop: &AtomicBool) -> AutoOutcome {
    let values = input.values;
    let (Some(next), Some(cavity)) = (input.cavity_next.as_ref(), values.cavity()) else {
        return plan_with(input, (&values, &input.next), stage, stop);
    };
    match values.place {
        Place::Outside => plan_with(input, (&values, &input.next), stage, stop),
        Place::Cavity => plan_with(input, (&cavity, next), stage, stop),
        Place::Everywhere => {
            let outside = AutoSupports {
                place: Place::Outside,
                braces: false,
                ..values
            };
            let inside = AutoSupports {
                braces: false,
                ..cavity
            };
            let mut out = plan_with(input, (&outside, &input.next), stage, stop);
            if stop.load(Ordering::Relaxed) {
                return out;
            }
            out.join(plan_with(input, (&inside, next), stage, stop));
            if let Some(model) = input.model.as_ref().filter(|_| values.braces) {
                stage(4);
                out.braces = plan_braces(input, model, &out.plan, stop);
            }
            out
        }
    }
}

impl AutoOutcome {
    /// Join `other`, a plan of another place on the same model, to this one: its supports
    /// after these (the trunks they join numbered after them), the counts summed and the places
    /// left together, lowest first. Braces are planned again over both by the caller.
    fn join(&mut self, other: Self) {
        let base = self.plan.supports.len();
        self.plan
            .supports
            .extend(other.plan.supports.into_iter().map(|mut p| {
                p.joins = p.joins.map(|(trunk, z)| (trunk + base, z));
                p
            }));
        self.plan.held += other.plan.held;
        self.plan.joined += other.plan.joined;
        self.plan.branches += other.plan.branches;
        self.plan.skipped.extend(other.plan.skipped);
        self.plan.room_needed_mm = self.plan.room_needed_mm.max(other.plan.room_needed_mm);
        self.islands = [0, 1].map(|k| self.islands[k] + other.islands[k]);
        self.overhangs = [0, 1].map(|k| self.overhangs[k] + other.overhangs[k]);
        self.left.extend(other.left);
        self.left.sort_by(|a, b| a.point[2].total_cmp(&b.point[2]));
        self.added += other.added;
        self.passes = self.passes.max(other.passes);
        self.braces.clear();
    }
}

/// Supports for `tips` chosen beforehand (the Grid mode's, TD-138): each grown as the plan grows
/// its tips, with branches when `values` ask, made with `next` and of `origin`, then built as a
/// slice builds them against the model, those with an error going. A tip left without a support
/// is a place left unsupported, with why. `input`'s regions and islands are not used.
pub(crate) fn plan_tips(
    input: &Input,
    (values, next): (&AutoSupports, &NextSupport),
    (tips, origin): (&[Tip], SupportOrigin),
    stop: &AtomicBool,
) -> AutoOutcome {
    let mut out = AutoOutcome::default();
    let surface = input.surface.as_ref();
    let growing_values = IslandSupports {
        merge_mm: 0.0,
        ..values.growing()
    };
    let growing = Growing {
        surface,
        next,
        document: input.document.as_ref(),
        values: &growing_values,
        volume: input.volume,
        origin,
    };
    let tasks: Vec<(usize, Tip)> = tips.iter().copied().enumerate().collect();
    let mut why_at: Vec<([f64; 3], Skip)> = Vec::new();
    let grove = grow(&tasks, tasks.len(), &growing, stop);
    for (g, (kept, why)) in grove.kept.iter().zip(&grove.why).enumerate() {
        if *kept == 0 {
            why_at.push((tasks[g].1 .0, *why));
        }
    }
    out.plan.branches = grove.branches;
    out.plan.supports = grove.supports;
    if let Some(model) = &input.model {
        let kept = Kept {
            document: input.document.as_ref(),
            to_model: &input.to_model,
        };
        check_built(&kept, model, &mut out.plan, &growing, &mut why_at, stop);
    }
    let made: Vec<[f64; 3]> = out
        .plan
        .supports
        .iter()
        .map(|p| p.routed.support.tip_mm)
        .collect();
    for &(point, ..) in tips {
        if !made.iter().any(|&tip| distance(tip, point) <= SIDE_HELD_MM) {
            out.left.push(Left {
                point,
                bounds: [point, point],
                island: false,
                why: check::nearest_why(&why_at, point),
            });
        }
    }
    out.left.sort_by(|a, b| a.point[2].total_cmp(&b.point[2]));
    out.overhangs = [tips.len() - out.left.len(), tips.len()];
    out
}

/// Plan the supports of `input` with `values`, made with `next`: see [`plan()`].
#[allow(clippy::too_many_lines)] // The plan's stages, in order.
fn plan_with(
    input: &Input,
    (values, next): (&AutoSupports, &NextSupport),
    stage: &dyn Fn(u32),
    stop: &AtomicBool,
) -> AutoOutcome {
    let mut out = AutoOutcome::default();
    let surface = input.surface.as_ref();
    let Some(mesh) = surface.mesh() else {
        return out;
    };
    let growing_values = values.growing();
    let growing = Growing {
        surface,
        next,
        document: input.document.as_ref(),
        values: &growing_values,
        volume: input.volume,
        origin: SupportOrigin::Automatic,
    };
    stage(0);
    // The islands wanted, grouped as Support the islands groups them, each held where it starts
    // as Support the islands holds it.
    let islands: Vec<(usize, Island)> = input
        .islands
        .iter()
        .filter(|island| {
            island.area_mm2 >= values.skip_under_mm2
                && values.place.holds(island.inside)
                && !held(island, &input.kept_tips, values.join_mm)
        })
        .copied()
        .enumerate()
        .collect();
    let island_groups = groups(&islands, values.join_mm);
    let kept = Kept {
        document: input.document.as_ref(),
        to_model: &input.to_model,
    };
    let checking = input.model.as_ref().map(|model| (&kept, model));
    let mut why_at: Vec<([f64; 3], Skip)> = Vec::new();
    let group_why = hold_every_start(
        &Starts {
            islands: &islands,
            groups: &island_groups,
            surface,
            mesh,
            growing: &growing,
            checking,
        },
        &mut out.plan,
        &mut why_at,
        stop,
    );
    // The overhangs' candidates, thinned against every tip so far.
    let probed = if values.cover == Cover::All {
        let cuts = input
            .model
            .as_ref()
            .and_then(|m| m.preparation.objects.get(m.index))
            .and_then(|p| p.as_ref().ok())
            .map(karasu_project::pipeline::ObjectPreparation::drain_cylinders)
            .unwrap_or_default();
        candidates(
            surface,
            mesh,
            &input.regions,
            &cuts,
            (values, next.parameters().surface_width_mm() / 2.0),
            input.angle_deg,
        )
    } else {
        Probed::default()
    };
    let all = probed.candidates;
    let mut near = Near::new(values.widest_reach());
    for &tip in &input.kept_tips {
        near.add(tip);
    }
    for p in &out.plan.supports {
        near.add(p.routed.support.tip_mm);
    }
    let mut tried = BTreeSet::new();
    let take = |near: &mut Near, tried: &mut BTreeSet<usize>, tasks: &mut Vec<(usize, Tip)>| {
        let mut chosen = Near::new(values.widest_reach());
        for (k, c) in all.iter().enumerate() {
            if tried.contains(&k)
                || near.within(c.tip.0, c.reach)
                || chosen.within(c.tip.0, c.reach)
            {
                continue;
            }
            tried.insert(k);
            chosen.add(c.tip.0);
            // Each a group of its own.
            tasks.push((tasks.len(), c.tip));
        }
    };
    let mut tasks: Vec<(usize, Tip)> = Vec::new();
    take(&mut near, &mut tried, &mut tasks);
    // The side tips beside leading edges (TD-136): the tips under such an edge hold it from
    // below, these from its side, between them, as supports placed by hand are: each half its
    // reach from the tips taken, and its reach from the other side tips.
    let mut taken_now = Near::new(values.widest_reach());
    for (_, tip) in &tasks {
        taken_now.add(tip.0);
    }
    let mut sides: Vec<Tip> = Vec::new();
    let mut taken = Near::new(values.widest_reach());
    for c in &probed.sides {
        if !taken_now.within(c.tip.0, c.reach / 2.0) && !taken.within(c.tip.0, c.reach) {
            taken.add(c.tip.0);
            sides.push(c.tip);
        }
    }
    for &tip in &sides {
        tasks.push((tasks.len(), tip));
    }
    if stop.load(Ordering::Relaxed) {
        return out;
    }
    stage(1);
    let grove = grow(&tasks, tasks.len(), &growing, stop);
    for (g, (kept, why)) in grove.kept.iter().zip(&grove.why).enumerate() {
        if *kept == 0 {
            why_at.push((tasks[g].1 .0, *why));
        }
    }
    let base = out.plan.supports.len();
    out.plan.branches += grove.branches;
    out.plan
        .supports
        .extend(grove.supports.into_iter().map(|mut p| {
            p.joins = p.joins.map(|(trunk, z)| (trunk + base, z));
            p
        }));
    // The closing check: candidates still uncovered get the next tips, pass after pass.
    let tips_now = |supports: &[plan::Planned]| {
        let mut kept = Near::new(values.widest_reach());
        for s in supports {
            kept.add(s.routed.support.tip_mm);
        }
        for &tip in &input.kept_tips {
            kept.add(tip);
        }
        kept
    };
    for pass in 1..=values.passes {
        if stop.load(Ordering::Relaxed) || all.is_empty() {
            break;
        }
        stage(2);
        let mut kept = tips_now(&out.plan.supports);
        let mut more: Vec<(usize, Tip)> = Vec::new();
        take(&mut kept, &mut tried, &mut more);
        if more.is_empty() {
            break;
        }
        out.passes = pass;
        for (k, task) in more.iter_mut().enumerate() {
            task.0 = k;
        }
        let grove = grow(&more, more.len(), &growing, stop);
        for (g, (count, why)) in grove.kept.iter().zip(&grove.why).enumerate() {
            if *count == 0 {
                why_at.push((more[g].1 .0, *why));
            }
        }
        // Their trunks' places follow the supports before them.
        let base = out.plan.supports.len();
        out.added += grove.supports.len();
        out.plan.branches += grove.branches;
        out.plan
            .supports
            .extend(grove.supports.into_iter().map(|mut p| {
                p.joins = p.joins.map(|(trunk, z)| (trunk + base, z));
                p
            }));
    }
    plan::merge(
        &mut out.plan,
        surface,
        mesh,
        &growing_values,
        input.document.as_ref(),
        input.volume,
        stop,
    );
    // The final check: built as a slice builds them, supports with errors go.
    if let Some(model) = &input.model {
        stage(3);
        let kept = Kept {
            document: input.document.as_ref(),
            to_model: &input.to_model,
        };
        check_built(&kept, model, &mut out.plan, &growing, &mut why_at, stop);
    }
    // What is left: islands no tip holds, and uncovered candidates joined into places, each
    // with why the failed tip nearest it found no support.
    let nearest_why = |point: [f64; 3]| check::nearest_why(&why_at, point);
    let final_tips: Vec<[f64; 3]> = out
        .plan
        .supports
        .iter()
        .map(|p| p.routed.support.tip_mm)
        .chain(input.kept_tips.iter().copied())
        .collect();
    let mut held_now = 0;
    for (m, (_, island)) in islands.iter().enumerate() {
        if held(island, &final_tips, values.join_mm) {
            held_now += 1;
        } else {
            let [[x0, y0], [x1, y1]] = island.footprint;
            out.left.push(Left {
                point: island.point,
                bounds: [[x0, y0, island.heights[0]], [x1, y1, island.heights[1]]],
                island: true,
                why: check::why_near(&why_at, island.point).unwrap_or(group_why[m]),
            });
        }
    }
    out.islands = [held_now, islands.len()];
    for &(point, ..) in &sides {
        if !final_tips
            .iter()
            .any(|&tip| distance(tip, point) <= SIDE_HELD_MM)
        {
            out.left.push(Left {
                point,
                bounds: [point, point],
                island: false,
                why: nearest_why(point),
            });
        }
    }
    let kept = tips_now(&out.plan.supports);
    let uncovered: Vec<&Candidate> = all
        .iter()
        .filter(|c| !kept.within(c.tip.0, c.reach))
        .collect();
    out.overhangs = [all.len() - uncovered.len(), all.len()];
    let mut places: Vec<Vec<&Candidate>> = Vec::new();
    for c in uncovered {
        let p = c.tip.0;
        let joined = places.iter_mut().find(|place| {
            place
                .iter()
                .any(|q| distance(p, q.tip.0) <= values.widest_reach())
        });
        match joined {
            Some(place) => place.push(c),
            None => places.push(vec![c]),
        }
    }
    for place in places {
        let lowest = place[0].tip.0;
        let mut bounds = [lowest, lowest];
        for c in &place {
            let p = c.tip.0;
            bounds[0] = std::array::from_fn(|k| bounds[0][k].min(p[k]));
            bounds[1] = std::array::from_fn(|k| bounds[1][k].max(p[k]));
        }
        out.left.push(Left {
            point: lowest,
            bounds,
            island: false,
            why: nearest_why(lowest),
        });
    }
    out.left.sort_by(|a, b| a.point[2].total_cmp(&b.point[2]));
    if let Some(model) = input.model.as_ref().filter(|_| values.braces) {
        stage(4);
        out.braces = plan_braces(input, model, &out.plan, stop);
    }
    out
}

/// The braces over the kept supports and `planned` together, numbered as [`add_planned`] will
/// number them; none when they cannot be planned.
fn plan_braces(
    input: &Input,
    model: &check::ModelInput,
    planned: &Outcome,
    stop: &AtomicBool,
) -> Vec<Brace> {
    if planned.supports.is_empty() {
        return Vec::new();
    }
    let mut document = input
        .document
        .clone()
        .unwrap_or_else(|| SupportDocument::new(karasu_core::supports::Preset::Medium));
    add_planned(&mut document, &input.to_model, planned);
    let Some(Ok(prepared)) = model.preparation.objects.get(model.index) else {
        return Vec::new();
    };
    karasu_project::pipeline::plan_model_braces(
        &document,
        &model.mesh,
        &model.placement,
        prepared,
        &model.braces,
        &|_| stop.load(Ordering::Relaxed),
    )
    .map(|planned| {
        planned
            .into_iter()
            .filter(|p| p.skipped.is_none())
            .map(|p| p.brace)
            .collect()
    })
    .unwrap_or_default()
}

/// The document `document` (the kept supports) with `outcome`'s supports and braces added, as
/// Apply adds them.
#[must_use]
pub fn with_planned(
    document: Option<&SupportDocument>,
    to_model: &Affine,
    outcome: &AutoOutcome,
) -> SupportDocument {
    let mut document = document
        .cloned()
        .unwrap_or_else(|| SupportDocument::new(karasu_core::supports::Preset::Medium));
    add_planned(&mut document, to_model, &outcome.plan);
    document.braces.extend(outcome.braces.iter().copied());
    document
}

mod edges;
#[cfg(test)]
mod tests;
