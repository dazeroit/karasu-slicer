//! Supports planned under the islands (owner mockup "Islands tool", revised 2026-10-29): Support
//! the islands previews a support under every island without one, and Support it here adds the
//! chosen island's at once, by the same rules.
//!
//! **Where** (owner, 2026-10-30: one support, where the island starts; there the resin cures in
//! mid-air). An island is where one piece starts in mid-air ([`crate::islands_ui::Island`]).
//! Islands closer than the join distance (in plan and in height) share one support, under the
//! lowest of them. Its tip goes under the centroid of the island's first layer, where a vertical
//! line up from just under it first meets the surface facing down; when that is refused (no way
//! down, or the final check), the next points of that first layer are tried, nearest first, a
//! few at most. A hollowed model's cavity triangle there means the island starts on the cavity's
//! ceiling. What grows from an island higher up is an overhang, the Automatic tool's.
//!
//! **Trunks and branches** (owner, 2026-10-30: branching, phase 2). Tips closer than the branch
//! distance in plan (in the cavity, or outside, never mixed), up to the branches a trunk takes,
//! share one trunk: the tip nearest their middle that finds its way down; the others join its
//! shaft as branches, where a branch placed by hand would ([`junction_on`]). A tip that cannot
//! join it gets a support of its own. A branch distance of zero makes no branches.
//!
//! **Trees of several levels** (phase 3): trunks standing on their own close together then merge,
//! a main trunk thickened below for what it carries (see [`crate::supports_ui::trees`]).
//!
//! **How down.** Each support is routed as placing routes it ([`route_support`]): straight to the
//! plate, aside with a knee, or standing on the model below. Standing on the model is kept only
//! with the panel's switch on, or in the cavity, where a support always goes from the ceiling to
//! the floor; a support with no way down is skipped and its island counted with the reason.
//! Nothing goes through the model by this tool.
//!
//! **Mini supports** (owner, 2026-10-30; TD-113): with the values' switch on, a tip whose
//! standard support is refused (no way down, too low, out of reach, unbuildable) gets a mini
//! support standing on the model straight below it, where one fits. A mini support is never a
//! trunk and never merged: it is too thin to carry others.
//!
//! **The final check** ([`check`]): with the model given, the supports are built as a slice
//! builds them and those a slice would build with an error go (owner, 2026-10-30), what hung
//! from them grown again; an island left without a support is skipped with why. A support
//! entering a drain hole stays, only warned about (TD-131): a hole often makes the island.
//!
//! **Off the interface thread.** A plan is made on a thread of its own against the selected
//! model's surface for routes (kept by the Supports tool's state while the Islands tool is in
//! use), with a preview of the kept supports built against small triangles of the surface at
//! their anchors, as the placing preview is.
use super::{holder, Island, PROBE_MM};
use crate::{
    preparation_ui::centre,
    supports_ui::{
        branches::{junction_on, Junction},
        route::{mini_routed, route_support, RouteSurface, Routed},
        tangent_triangle,
        trees::{self, MergeValues, Room, Stem},
        NextSupport, SupportPreset, Weight,
    },
    workspace::Workspace,
};
use karasu_core::{
    cancellation::Cancellation,
    geometry::Point3,
    mesh::TriangleMesh,
    supports::{
        build_supports, AxisSpec, BaseSpec, BuildVolume, ManualSupport, Preset, Route,
        SupportDocument, SupportKind, SupportOptions, SupportOrigin, SupportParameters,
        SupportScene, SupportSurface,
    },
};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc,
};

pub(crate) mod check;

/// How far above where an island starts its tips go, in mm: its first layers' underside.
pub const START_MM: f64 = 1.0;
/// The rings around an island's centroid looked over for more places its support may stand:
/// every this many mm...
const RING_STEP_MM: f64 = 0.25;
/// ... this many rings (3 mm), and this many directions on each.
const RING_STEPS: u32 = 12;
const RING_DIRECTIONS: u32 = 12;
/// Places kept for an island's support are at least this far apart, in mm.
const APART_MM: f64 = 0.5;
/// A place tried after a refused one is at least this far from every one refused, in mm: past
/// a drain hole, the thin part a tip pierced, or what it came too close to.
const RETRY_APART_MM: f64 = 2.0;
/// The places kept for an island's support, and how many are tried at most.
const CANDIDATES: usize = 96;
const ATTEMPTS: usize = 3;
/// The distance under which islands share a support the panel accepts, in mm.
pub const JOIN_MM: std::ops::RangeInclusive<f64> = 0.0..=20.0;
/// The smallest islands' areas Support the islands can leave out, in mm².
pub const SKIP_UNDER_MM2: std::ops::RangeInclusive<f64> = 0.0..=100.0;
/// The distance under which tips share a trunk the panel accepts, in mm; zero, no branches.
pub const BRANCH_MM: std::ops::RangeInclusive<f64> = 0.0..=20.0;
/// The branches a trunk takes at most, as the panel accepts them.
pub const PER_TRUNK: std::ops::RangeInclusive<f64> = 1.0..=12.0;
/// The distance under which trunks merge the panel accepts, in mm; zero, no merging.
pub const MERGE_MM: std::ops::RangeInclusive<f64> = 0.0..=30.0;
/// The trunks a group merges at most, as the panel accepts them.
pub const PER_GROUP: std::ops::RangeInclusive<f64> = 2.0..=8.0;
/// The levels of merging the panel accepts.
pub const LEVELS: std::ops::RangeInclusive<f64> = 1.0..=4.0;
/// The range of the smallest diameter a fitted contact narrows to, in mm (the core's).
pub const CONTACT_MIN_MM: std::ops::RangeInclusive<f64> = 0.05..=5.0;

/// How the islands' supports are made: tool values, from Settings › Supports each time the tool
/// opens (owner's mockup values).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IslandSupports {
    /// The preset: a weight, or the user's preset at this place.
    pub weight: Weight,
    pub mine: Option<usize>,
    /// Islands closer than this share a support, in mm.
    pub join_mm: f64,
    /// Support the islands leaves out islands smaller than this, in mm².
    pub skip_under_mm2: f64,
    /// A support whose way down reaches no plate stands on the model below.
    pub foot_on_model: bool,
    /// Where a support finds no room or no way down, a mini support stands on the model below
    /// its tip (owner, 2026-10-30).
    pub minis: bool,
    /// Tips closer than this in plan share a trunk, in mm; zero, no branches.
    pub branch_mm: f64,
    /// The branches a trunk takes at most.
    pub per_trunk: u32,
    /// Trunks on the plate closer than this merge into fewer, in mm; zero, no merging.
    pub merge_mm: f64,
    /// The trunks a group merges at most, its main trunk included.
    pub per_group: u32,
    /// How many times trunks merge, each level on the main trunks of the one before.
    pub levels: u32,
    /// The contacts fit the surface, narrowing down to this diameter at least (TD-116).
    pub contact_fit_min_mm: Option<f64>,
    /// The contacts end in a ball, not a cone, whatever the preset (owner, 2026-11-02).
    pub sphere_contact: bool,
}

/// The values island supports start from when the settings hold none.
pub const DEFAULT_ISLAND_SUPPORTS: IslandSupports = IslandSupports {
    weight: Weight::Light,
    mine: None,
    join_mm: 0.0,
    skip_under_mm2: 0.0,
    foot_on_model: true,
    minis: true,
    branch_mm: 5.0,
    per_trunk: 4,
    merge_mm: 10.0,
    per_group: 3,
    levels: 2,
    contact_fit_min_mm: None,
    sphere_contact: false,
};

impl Default for IslandSupports {
    fn default() -> Self {
        DEFAULT_ISLAND_SUPPORTS
    }
}

impl IslandSupports {
    /// These values if every one is in its range, else the built-in ones.
    #[must_use]
    pub fn valid(self) -> Self {
        if JOIN_MM.contains(&self.join_mm)
            && SKIP_UNDER_MM2.contains(&self.skip_under_mm2)
            && BRANCH_MM.contains(&self.branch_mm)
            && PER_TRUNK.contains(&f64::from(self.per_trunk))
            && MERGE_MM.contains(&self.merge_mm)
            && PER_GROUP.contains(&f64::from(self.per_group))
            && LEVELS.contains(&f64::from(self.levels))
            && self
                .contact_fit_min_mm
                .is_none_or(|m| CONTACT_MIN_MM.contains(&m))
        {
            self
        } else {
            DEFAULT_ISLAND_SUPPORTS
        }
    }

    /// What the supports are made with: the preset's values, with the contact's shape chosen
    /// here, fitting the surface when asked.
    #[must_use]
    pub fn next(&self, presets: &[SupportPreset]) -> NextSupport {
        let mut next = match self.mine {
            Some(i) if i < presets.len() => NextSupport::of_preset(presets, i),
            _ => NextSupport::of(self.weight),
        }
        .fitting(self.contact_fit_min_mm);
        next.values.sphere_contact = self.sphere_contact;
        next
    }
}

/// Why an island got no support.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Skip {
    /// No underside was found under it.
    NoSurface,
    /// Its way down reaches no plate, and standing on the model is off.
    OutOfReach,
    /// No way down at all clear of the model.
    NoWay,
    /// Its support cannot be built there with the preset: too close to the plate, or its path
    /// is degenerate.
    Unbuildable,
    /// A drain hole cuts the material away where its tip would stand: nothing there to hold
    /// (TD-131; the final check).
    DrainHole,
    /// Its foot would stand on the model where a drain hole cuts it away (TD-131; the final
    /// check).
    FootInHole,
    /// Too close to the plate (or the raft) for the preset: its tip, base cone and base need
    /// more room than there is ([`room_needed`]).
    TooLow,
    /// Built against the model, a part of its support comes closer than the clearance (the
    /// Automatic tool's final check).
    Collides,
    /// Built against the model, its tip reaches through a thin part (the Automatic tool's final
    /// check).
    Pierces,
}

/// A support planned under an island: the support in world coordinates with its anchors, and
/// for a branch (or a merged trunk) the trunk it joins (its place among the plan's supports)
/// and the height where it joins it.
pub struct Planned {
    pub routed: Routed,
    pub joins: Option<(usize, f64)>,
}

/// A plan: the supports kept, the islands they hold (joined ones included), how many supports
/// join another (branches and merged trunks), and the islands skipped with why.
#[derive(Default)]
pub struct Outcome {
    pub supports: Vec<Planned>,
    pub held: usize,
    pub joined: usize,
    pub branches: usize,
    pub skipped: Vec<(usize, Skip)>,
    /// The room a support of the plan's preset needs under its tip, and where supports stand,
    /// in mm: what an island skipped as too low lacks.
    pub room_needed_mm: f64,
    pub floor_mm: f64,
}

/// A tip under an island: its point, its outward normal, and whether it lies on a cavity
/// triangle.
pub(crate) type Tip = ([f64; 3], [f64; 3], bool);

/// Where `island`'s support may stand, nearest where it starts first: under the centroid of its
/// first layer, then points of its first layer around it (rings every [`RING_STEP_MM`], each a
/// support there would hold it, [`check::holds`]), none within [`APART_MM`] of one before; each
/// a point, its outward normal and whether it lies on a cavity triangle.
pub(crate) fn start_tips(surface: &RouteSurface, mesh: &TriangleMesh, island: &Island) -> Vec<Tip> {
    let [under, top] = island.heights;
    let probe = |x: f64, y: f64| {
        let (point, normal, t) =
            surface
                .grid()
                .first_above(mesh, [x, y, under - 1e-3], top + PROBE_MM)?;
        // Its underside where it starts: facing down, holding it.
        (normal[2] < 0.0 && check::holds(island, point, 0.0)).then_some((
            point,
            normal,
            surface.is_cavity(t),
        ))
    };
    let [cx, cy, _] = island.point;
    let mut found: Vec<Tip> = Vec::new();
    let mut add = |tip: Tip| {
        if found.len() < CANDIDATES
            && found
                .iter()
                .all(|f| check::distance(f.0, tip.0) >= APART_MM)
        {
            found.push(tip);
        }
    };
    if let Some(tip) = probe(cx, cy) {
        add(tip);
    }
    for ring in 1..=RING_STEPS {
        let r = f64::from(ring) * RING_STEP_MM;
        for k in 0..RING_DIRECTIONS {
            let a = std::f64::consts::TAU * f64::from(k) / f64::from(RING_DIRECTIONS);
            if let Some(tip) = probe(cx + r * a.cos(), cy + r * a.sin()) {
                add(tip);
            }
        }
    }
    found
}

/// The islands among `islands` (their places with them) that share a support: groups of those
/// closer than `join` in plan and in height, directly or through others, each led by its
/// lowest.
pub(crate) fn groups(islands: &[(usize, Island)], join: f64) -> Vec<Vec<usize>> {
    let mut order: Vec<usize> = (0..islands.len()).collect();
    order.sort_by(|&a, &b| {
        let (p, q) = (islands[a].1.point, islands[b].1.point);
        p[2].total_cmp(&q[2])
            .then(p[0].total_cmp(&q[0]))
            .then(p[1].total_cmp(&q[1]))
    });
    let mut out: Vec<Vec<usize>> = Vec::new();
    for &k in &order {
        let p = islands[k].1.point;
        let near = out.iter().position(|group| {
            group.iter().any(|&m| {
                let q = islands[m].1.point;
                (p[0] - q[0]).hypot(p[1] - q[1]) <= join && (p[2] - q[2]).abs() <= join
            })
        });
        match near.filter(|_| join > 0.0) {
            Some(g) => out[g].push(k),
            None => out.push(vec![k]),
        }
    }
    out
}

/// The tips of `tasks` (by their places) that share a trunk: each tip not yet taken, in order,
/// with the next ones not taken within `branch_mm` of it in plan, on the same side of the
/// cavity's surface, up to `per_trunk` more; alone without branches.
fn trees(tasks: &[(usize, Tip)], branch_mm: f64, per_trunk: u32) -> Vec<Vec<usize>> {
    let mut taken = vec![false; tasks.len()];
    let mut out = Vec::new();
    for seed in 0..tasks.len() {
        if taken[seed] {
            continue;
        }
        taken[seed] = true;
        let mut members = vec![seed];
        let (point, _, cavity) = tasks[seed].1;
        for other in seed + 1..tasks.len() {
            if branch_mm <= 0.0 || members.len() > per_trunk as usize {
                break;
            }
            let (q, _, inside) = tasks[other].1;
            if !taken[other]
                && inside == cavity
                && (point[0] - q[0]).hypot(point[1] - q[1]) <= branch_mm
            {
                taken[other] = true;
                members.push(other);
            }
        }
        out.push(members);
    }
    out
}

/// What a trunk and its branches are planned with.
struct Tree<'a> {
    /// The tips sharing it, by their places in `tasks`.
    members: &'a [usize],
    tasks: &'a [(usize, Tip)],
    surface: &'a RouteSurface,
    parameters: &'a SupportParameters,
    axis: AxisSpec,
    document: Option<&'a SupportDocument>,
    values: &'a IslandSupports,
    volume: BuildVolume,
    origin: SupportOrigin,
}

/// A tip planned: its place in the tasks, and its support (with the trunk's place in the tasks
/// and the junction's height, for a branch), or why it has none.
struct Grown {
    task: usize,
    result: Result<(Routed, Option<(usize, f64)>), Skip>,
}

/// The support of the tip at `task`, routed on its own, built alone: see the module. Where a
/// standard support is refused and `minis` and the values allow it, a mini support standing on
/// the model straight below the tip (owner, 2026-10-30); the standard support's refusal without
/// one.
fn routed_alone(t: &Tree<'_>, task: usize, minis: bool) -> Result<(Routed, SupportScene), Skip> {
    match standard_alone(t, task) {
        Err(skip) if minis && t.values.minis => mini_alone(t, task).ok_or(skip),
        standard => standard,
    }
}

/// The mini support of the tip at `task`, standing on the model with its rod clear of it, built
/// alone; `None` where it finds nowhere to stand or cannot be built.
fn mini_alone(t: &Tree<'_>, task: usize) -> Option<(Routed, SupportScene)> {
    let (tip, normal, _) = t.tasks[task].1;
    mini_built(t.surface, t.parameters, t.document, t.volume, tip, normal)
}

/// The mini support of `parameters` with its tip at `tip` (outward `normal`), standing on the
/// model on `surface` with its rod clear of it (TD-126), built alone within `volume`; `None`
/// where it finds nowhere to stand or cannot be built.
pub(crate) fn mini_built(
    surface: &RouteSurface,
    parameters: &SupportParameters,
    document: Option<&SupportDocument>,
    volume: BuildVolume,
    tip: [f64; 3],
    normal: [f64; 3],
) -> Option<(Routed, SupportScene)> {
    // Placed by the tool: its rod keeps off the model, or it is not placed (TD-126).
    let routed = mini_routed(surface, parameters, document, tip, normal, true)?;
    let scene = alone(&[(&routed, None)], volume)?;
    scene
        .supports()
        .iter()
        .all(|s| s.parts > 0)
        .then_some((routed, scene))
}

/// The standard support of the tip at `task`, routed on its own, built alone.
fn standard_alone(t: &Tree<'_>, task: usize) -> Result<(Routed, SupportScene), Skip> {
    let (tip, normal, cavity) = t.tasks[task].1;
    let (routed, route) = route_support(t.surface, t.parameters, t.axis, t.document, tip, normal);
    match route {
        Route::Straight | Route::Tilted(_) | Route::Aside(_) => {}
        Route::OnModel { .. } if t.values.foot_on_model || cavity => {}
        Route::OnModel { .. } => return Err(Skip::OutOfReach),
        Route::None => return Err(Skip::NoWay),
    }
    match alone(&[(&routed, None)], t.volume) {
        Some(scene) if scene.supports().iter().all(|s| s.parts > 0) => Ok((routed, scene)),
        // Said with how much room it lacks, when that is why (owner, 2026-10-30).
        _ if tip[2] - floor(t.document) < room_needed(t.parameters) => Err(Skip::TooLow),
        _ => Err(Skip::Unbuildable),
    }
}

/// The height a support of `parameters` needs under its tip: the tip, the base's cone and the
/// base, in mm.
#[must_use]
pub(crate) fn room_needed(parameters: &SupportParameters) -> f64 {
    parameters.tip_length_mm + parameters.cone_height_mm + parameters.pad_thickness_mm
}

/// Where supports stand: the raft's top, or the plate.
pub(crate) fn floor(document: Option<&SupportDocument>) -> f64 {
    document
        .and_then(|d| d.raft)
        .map_or(0.0, |r| r.thickness_mm)
}

/// A trunk and its branches: the tip nearest the members' middle that finds its way down is
/// the trunk (trunk first in the result), the others join it where they can, or get supports
/// of their own.
fn tree(t: &Tree<'_>) -> Vec<Grown> {
    let alone_all = || -> Vec<Grown> {
        t.members
            .iter()
            .map(|&task| Grown {
                task,
                result: routed_alone(t, task, true).map(|(routed, _)| (routed, None)),
            })
            .collect()
    };
    if t.members.len() < 2 {
        return alone_all();
    }
    #[allow(clippy::cast_precision_loss)] // A few tips.
    let count = t.members.len() as f64;
    let middle = [0, 1].map(|k| t.members.iter().map(|&m| t.tasks[m].1 .0[k]).sum::<f64>() / count);
    let mut order = t.members.to_vec();
    order.sort_by(|&a, &b| {
        let d = |m: usize| {
            let p = t.tasks[m].1 .0;
            (p[0] - middle[0]).hypot(p[1] - middle[1])
        };
        d(a).total_cmp(&d(b)).then(a.cmp(&b))
    });
    // The first few nearest the middle are tried as the trunk: a standard support, as a mini
    // support is too thin to carry branches.
    let found = order
        .iter()
        .take(3)
        .find_map(|&m| routed_alone(t, m, false).ok().map(|built| (m, built)));
    let Some((trunk, (trunk_routed, scene))) = found else {
        return alone_all();
    };
    let mut grown = vec![Grown {
        task: trunk,
        result: Ok((trunk_routed, None)),
    }];
    for &task in t.members.iter().filter(|&&m| m != trunk) {
        let (tip, normal, _) = t.tasks[task].1;
        let at = junction_on(&Junction {
            scene: &scene,
            parent: 1,
            trunk_mm: t.parameters.shaft_diameter_mm,
            tip,
            normal,
            parameters: t.parameters,
            kind: karasu_core::supports::SupportKind::Standard,
            document: t.document,
            surface: Some(t.surface),
        });
        let result = match at {
            Some(at) => Ok((
                Routed {
                    support: ManualSupport {
                        id: 1,
                        tip_mm: tip,
                        tip_axis: AxisSpec::Straight,
                        bends: Vec::new(),
                        base: BaseSpec::Vertical,
                        parameters: t.parameters.clone(),
                        accept_collision: false,
                        origin: t.origin,
                        kind: karasu_core::supports::SupportKind::Standard,
                    },
                    anchors: vec![(
                        tip,
                        t.surface
                            .normal_at(tip, normal, t.parameters.footprint_radius_mm()),
                    )],
                },
                Some((trunk, at[2])),
            )),
            None => routed_alone(t, task, true).map(|(routed, _)| (routed, None)),
        };
        grown.push(Grown { task, result });
    }
    grown
}

/// Plan supports under `islands` (their places in the search with them) on `surface`, made with
/// `next`'s values, the document's clearance, floor and axis limit applying, each checked to
/// build within `volume`, and with `checked` (the model, and the way to its frame) the final
/// check: see the module documentation. Deterministic for equal inputs.
#[allow(clippy::too_many_arguments)] // The plan's whole context.
pub fn plan(
    islands: &[(usize, Island)],
    surface: &RouteSurface,
    next: &NextSupport,
    document: Option<&SupportDocument>,
    values: &IslandSupports,
    volume: BuildVolume,
    checked: Option<(&check::ModelInput, &crate::gizmo::Affine)>,
    stop: &AtomicBool,
) -> Outcome {
    let mut outcome = Outcome {
        room_needed_mm: room_needed(&next.parameters()),
        floor_mm: floor(document),
        ..Outcome::default()
    };
    let Some(mesh) = surface.mesh() else {
        outcome.skipped = islands.iter().map(|&(k, _)| (k, Skip::NoSurface)).collect();
        return outcome;
    };
    let growing = Growing {
        surface,
        next,
        document,
        values,
        volume,
        origin: SupportOrigin::Islands,
    };
    let kept = checked.map(|(model, to_model)| (check::Kept { document, to_model }, model));
    let checking = kept.as_ref().map(|(kept, model)| (kept, *model));
    let mut why_at = Vec::new();
    let groups = groups(islands, values.join_mm);
    let why = hold_every_start(
        &Starts {
            islands,
            groups: &groups,
            surface,
            mesh,
            growing: &growing,
            checking,
        },
        &mut outcome,
        &mut why_at,
        stop,
    );
    merge(&mut outcome, surface, mesh, values, document, volume, stop);
    // Merging bends trunks: they are checked again.
    if let Some((kept, model)) = checking {
        check::check_built(kept, model, &mut outcome, &growing, &mut why_at, stop);
    }
    tally(
        &mut outcome,
        values.join_mm,
        islands,
        &groups,
        &why,
        &why_at,
    );
    outcome
}

/// What [`hold_starts`] holds: the islands (their places in the search with them), grouped as
/// they share supports, on `surface` (its `mesh`), grown with `growing`, and checked with
/// `checking` (the kept supports and the model) when given.
pub(crate) struct Starts<'a> {
    pub islands: &'a [(usize, Island)],
    pub groups: &'a [Vec<usize>],
    pub surface: &'a RouteSurface,
    pub mesh: &'a TriangleMesh,
    pub growing: &'a Growing<'a>,
    pub checking: Option<(&'a check::Kept<'a>, &'a check::ModelInput)>,
}

/// One support for each group of `starts`, where its lowest island starts: the first place
/// [`start_tips`] gives, then, while it is refused (no way down, or the final check), the next
/// at least [`RETRY_APART_MM`] from every one refused, [`ATTEMPTS`] at most. The supports are added to `outcome`; what the final check refused
/// leaves why in `why_at`. Returns each group's last refusal (unused for a group held).
pub(crate) fn hold_starts(
    starts: &Starts<'_>,
    outcome: &mut Outcome,
    why_at: &mut Vec<([f64; 3], Skip)>,
    stop: &AtomicBool,
) -> Vec<Skip> {
    let lead = |g: usize| &starts.islands[starts.groups[g][0]].1;
    let places: Vec<Vec<Tip>> = (0..starts.groups.len())
        .map(|g| start_tips(starts.surface, starts.mesh, lead(g)))
        .collect();
    let mut why = vec![Skip::NoSurface; starts.groups.len()];
    let mut tried: Vec<Vec<[f64; 3]>> = vec![Vec::new(); starts.groups.len()];
    for _ in 0..ATTEMPTS {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        let tips: Vec<[f64; 3]> = outcome
            .supports
            .iter()
            .map(|p| p.routed.support.tip_mm)
            .collect();
        let tasks: Vec<(usize, Tip)> = (0..starts.groups.len())
            .filter(|&g| !check::held(lead(g), &tips, starts.growing.values.join_mm))
            .filter_map(|g| {
                let place = places[g].iter().find(|p| {
                    tried[g]
                        .iter()
                        .all(|&t| check::distance(t, p.0) >= RETRY_APART_MM)
                })?;
                Some((g, *place))
            })
            .collect();
        if tasks.is_empty() {
            break;
        }
        for &(g, (tip, _, _)) in &tasks {
            tried[g].push(tip);
        }
        let grove = grow(&tasks, starts.groups.len(), starts.growing, stop);
        for &(g, _) in &tasks {
            if grove.kept[g] == 0 {
                why[g] = grove.why[g];
            }
        }
        let base = outcome.supports.len();
        outcome.branches += grove.branches;
        outcome
            .supports
            .extend(grove.supports.into_iter().map(|mut p| {
                p.joins = p.joins.map(|(trunk, z)| (trunk + base, z));
                p
            }));
        if let Some((kept, model)) = starts.checking {
            check::check_built(kept, model, outcome, starts.growing, why_at, stop);
        }
        for &(g, (tip, _, _)) in &tasks {
            if let Some(refused) = check::why_near(why_at, tip) {
                why[g] = refused;
            }
        }
    }
    why
}

/// One support where each of `starts`' islands starts: their groups first, as [`hold_starts`]
/// holds them, then each island its group's support leaves (a chain of islands joined across
/// the join distance) on a support of its own. Returns each island's last refusal, by its
/// place (unused for one held).
pub(crate) fn hold_every_start(
    starts: &Starts<'_>,
    outcome: &mut Outcome,
    why_at: &mut Vec<([f64; 3], Skip)>,
    stop: &AtomicBool,
) -> Vec<Skip> {
    let join = starts.growing.values.join_mm;
    let mut why = vec![Skip::NoSurface; starts.islands.len()];
    let refused = hold_starts(starts, outcome, why_at, stop);
    for (members, refused) in starts.groups.iter().zip(&refused) {
        for &m in members {
            why[m] = *refused;
        }
    }
    let tips: Vec<[f64; 3]> = outcome
        .supports
        .iter()
        .map(|p| p.routed.support.tip_mm)
        .collect();
    let left: Vec<Vec<usize>> = (0..starts.islands.len())
        .filter(|&m| !check::held(&starts.islands[m].1, &tips, join))
        .map(|m| vec![m])
        .collect();
    if !left.is_empty() {
        let alone = hold_starts(
            &Starts {
                groups: &left,
                ..*starts
            },
            outcome,
            why_at,
            stop,
        );
        for (members, refused) in left.iter().zip(alone) {
            why[members[0]] = refused;
        }
    }
    why
}

/// Tell `outcome`'s islands held, by the supports it holds, where they start: an island not
/// held is skipped with why (the final check's near where it starts, else its last refusal in
/// `why`, by its place).
fn tally(
    outcome: &mut Outcome,
    join_mm: f64,
    islands: &[(usize, Island)],
    groups: &[Vec<usize>],
    why: &[Skip],
    why_at: &[([f64; 3], Skip)],
) {
    let tips: Vec<[f64; 3]> = outcome
        .supports
        .iter()
        .map(|p| p.routed.support.tip_mm)
        .collect();
    let (mut held, mut held_groups) = (0, 0);
    outcome.skipped.clear();
    for members in groups {
        let mut any = false;
        for &m in members {
            let island = &islands[m].1;
            if check::held(island, &tips, join_mm) {
                held += 1;
                any = true;
            } else {
                let because = check::why_near(why_at, island.point).unwrap_or(why[m]);
                outcome.skipped.push((islands[m].0, because));
            }
        }
        held_groups += usize::from(any);
    }
    outcome.held = held;
    outcome.joined = held.saturating_sub(held_groups);
}

/// What supports are grown with: the surface for routes, the supports' values and the
/// document's limits, the tree values, the build volume and the origin they are added with.
pub(crate) struct Growing<'a> {
    pub surface: &'a RouteSurface,
    pub next: &'a NextSupport,
    pub document: Option<&'a SupportDocument>,
    pub values: &'a IslandSupports,
    pub volume: BuildVolume,
    pub origin: SupportOrigin,
}

/// Supports grown from tips: trunks before their branches, which name them by their place; and
/// for each group of tips, how many got a support, or why its last tip was refused (no tip: no
/// underside).
pub(crate) struct Grove {
    pub supports: Vec<Planned>,
    pub branches: usize,
    pub kept: Vec<usize>,
    pub why: Vec<Skip>,
}

/// Grow supports from `tasks` (each tip with its group, groups numbered below `groups`): tips
/// close together share trunks, each routed down as placing routes it. Every core takes the
/// next trunk as it is free (a few, in a cavity, cost far more than the rest), and the results
/// are put back in order, so the plan is the same whatever the cores (owner's `HillGiant`
/// hollowed: 5.3 s on one).
pub(crate) fn grow(
    tasks: &[(usize, Tip)],
    groups: usize,
    growing: &Growing<'_>,
    stop: &AtomicBool,
) -> Grove {
    let parameters = growing.next.parameters();
    let trees = trees(tasks, growing.values.branch_mm, growing.values.per_trunk);
    let tree = |members: &Vec<usize>| {
        tree(&Tree {
            members,
            tasks,
            surface: growing.surface,
            parameters: &parameters,
            axis: growing.next.axis(),
            document: growing.document,
            values: growing.values,
            volume: growing.volume,
            origin: growing.origin,
        })
    };
    let cores = std::thread::available_parallelism().map_or(1, std::num::NonZero::get);
    let taken = std::sync::atomic::AtomicUsize::new(0);
    let mut results: Vec<(usize, Vec<Grown>)> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..cores.min(trees.len()))
            .map(|_| {
                scope.spawn(|| {
                    let mut done = Vec::new();
                    while !stop.load(Ordering::Relaxed) {
                        let k = taken.fetch_add(1, Ordering::Relaxed);
                        let Some(members) = trees.get(k) else {
                            break;
                        };
                        done.push((k, tree(members)));
                    }
                    done
                })
            })
            .collect();
        // A core that failed plans nothing: its tips are left out, the others stay.
        handles
            .into_iter()
            .flat_map(|h| h.join().unwrap_or_default())
            .collect()
    });
    results.sort_by_key(|(k, _)| *k);
    let mut grove = Grove {
        supports: Vec::new(),
        branches: 0,
        kept: vec![0; groups],
        why: vec![Skip::NoSurface; groups],
    };
    for (_, grown) in results {
        let mut placed: Vec<(usize, usize)> = Vec::new();
        for one in grown {
            let g = tasks[one.task].0;
            match one.result {
                Ok((mut routed, joins)) => {
                    routed.support.origin = growing.origin;
                    let joins = joins.and_then(|(trunk, z)| {
                        let at = placed.iter().find(|(t, _)| *t == trunk)?.1;
                        Some((at, z))
                    });
                    grove.branches += usize::from(joins.is_some());
                    placed.push((one.task, grove.supports.len()));
                    grove.supports.push(Planned { routed, joins });
                    grove.kept[g] += 1;
                }
                Err(skip) => grove.why[g] = skip,
            }
        }
    }
    grove
}

/// Merge the trunks of `outcome` standing on their own (on the plate or the model), as
/// [`trees::merge`] merges them, each built alone for its shaft; a main trunk's foot widens.
#[allow(clippy::too_many_arguments)] // The plan's whole context.
pub(crate) fn merge(
    outcome: &mut Outcome,
    surface: &RouteSurface,
    mesh: &TriangleMesh,
    values: &IslandSupports,
    document: Option<&SupportDocument>,
    volume: BuildVolume,
    stop: &AtomicBool,
) {
    if values.merge_mm <= 0.0 {
        return;
    }
    let stems: Vec<Stem> = outcome
        .supports
        .iter()
        .map(|p| {
            // A mini support is too thin to merge with trunks.
            let free = p.joins.is_none()
                && p.routed.support.kind == SupportKind::Standard
                && matches!(
                    p.routed.support.base,
                    BaseSpec::Vertical | BaseSpec::Model { .. }
                );
            let shaft = if free {
                alone(&[(&p.routed, None)], volume)
                    .and_then(|scene| scene.shaft_path(1).map(<[[f64; 3]]>::to_vec))
                    .unwrap_or_default()
            } else {
                Vec::new()
            };
            Stem {
                shaft,
                shaft_mm: p.routed.support.parameters.shaft_diameter_mm,
                free,
                joins: p.joins,
                braced: Vec::new(),
            }
        })
        .collect();
    let room = Room {
        surface,
        mesh,
        floor: document
            .and_then(|d| d.raft)
            .map_or(0.0, |r| r.thickness_mm),
        clearance: document.map_or(0.3, |d| d.clearance_mm),
    };
    let merged = trees::merge(
        &stems,
        MergeValues {
            merge_mm: values.merge_mm,
            per_group: values.per_group,
            levels: values.levels,
            aside_mm: trees::aside_for(values.branch_mm),
        },
        &room,
        stop,
    );
    for m in &merged.merges {
        let planned = &mut outcome.supports[m.stem];
        trees::end_at(&mut planned.routed.support.bends, m.bend, |b| b[2]);
        planned.joins = Some(m.joins);
    }
    outcome.branches += merged.merges.len();
    for &(k, foot) in &merged.wider {
        let p = &mut outcome.supports[k].routed.support.parameters;
        p.shaft_bottom_diameter_mm = Some(p.shaft_bottom_diameter_mm.unwrap_or(0.0).max(foot));
    }
}

/// The kept supports of `outcome` built against small triangles of the surface at their
/// anchors, in world coordinates, for the preview: the mesh and its centre.
pub fn preview(outcome: &Outcome, volume: BuildVolume) -> Option<(TriangleMesh, [f64; 3])> {
    let planned: Vec<(&Routed, Option<(usize, f64)>)> = outcome
        .supports
        .iter()
        .map(|p| (&p.routed, p.joins))
        .collect();
    let scene = alone(&planned, volume)?;
    let built = scene.mesh();
    let mesh = TriangleMesh::try_new(built.vertices().to_vec(), built.triangles().to_vec()).ok()?;
    if mesh.triangles().is_empty() {
        return None;
    }
    let origin = centre(&mesh);
    Some((mesh, origin))
}

/// `supports`, numbered from 1 in order, built against small triangles of the surface at their
/// anchors, in world coordinates, as the placing preview builds one; a branch joins the support
/// at its place among them (which comes before it) at its height.
fn alone(
    supports: &[(&Routed, Option<(usize, f64)>)],
    volume: BuildVolume,
) -> Option<SupportScene> {
    let mut document = SupportDocument::new(Preset::Medium);
    let mut points: Vec<Point3> = Vec::new();
    let mut triangles = Vec::new();
    for (k, (planned, joins)) in supports.iter().enumerate() {
        let p = &planned.support.parameters;
        let radius = p
            .contact_sphere_diameter_mm
            .max(p.tip_body_diameter_mm)
            .max(p.foot_body_diameter_mm)
            + 1.0;
        for &(point, normal) in &planned.anchors {
            let one = tangent_triangle(point, normal, radius)?;
            let base = u32::try_from(points.len()).ok()?;
            points.extend_from_slice(one.vertices());
            triangles.extend(one.triangles().iter().map(|t| t.map(|v| v + base)));
        }
        let mut support = ManualSupport {
            id: k as u64 + 1,
            ..planned.support.clone()
        };
        if let Some((trunk, z_mm)) = joins {
            support.base = BaseSpec::Support {
                parent: *trunk as u64 + 1,
                z_mm: *z_mm,
            };
        }
        document.supports.push(support);
    }
    if document.supports.is_empty() {
        return None;
    }
    // A merged trunk may come before the one it joins: parents first, as a build needs them.
    let document = document.validated().ok()?;
    let surface = TriangleMesh::try_new(points, triangles).ok()?;
    build_supports(
        &document,
        SupportSurface {
            mesh: &surface,
            interior_from: None,
        },
        &[],
        volume,
        SupportOptions::default(),
        &mut Cancellation::never(),
    )
    .ok()
}

/// Add `outcome`'s supports to model `index`, in its model frame, as one undoable step.
/// Returns the identities they took.
pub fn apply(workspace: &mut Workspace, index: usize, outcome: &Outcome) -> Vec<u64> {
    let Some(to_model) = workspace.model_to_world(index).and_then(|m| m.inverse()) else {
        return Vec::new();
    };
    let mut document = workspace
        .supports(index)
        .cloned()
        .unwrap_or_else(|| SupportDocument::new(Preset::Medium));
    let ids = add_planned(&mut document, &to_model, outcome);
    if !ids.is_empty() {
        workspace.set_supports(index, Some(document), None);
    }
    ids
}

/// Add `outcome`'s supports to `document`, taken to the model frame by `to_model`, with the
/// identities after its highest and the origin they were grown with. Returns those identities.
pub(crate) fn add_planned(
    document: &mut SupportDocument,
    to_model: &crate::gizmo::Affine,
    outcome: &Outcome,
) -> Vec<u64> {
    let first = document
        .supports
        .iter()
        .map(|s| s.id)
        .chain(document.braces.iter().map(|b| b.id))
        .max()
        .unwrap_or(0)
        + 1;
    // Every support's identity first: a merged trunk may join one after it.
    let ids: Vec<u64> = (first..).take(outcome.supports.len()).collect();
    for (&next, planned) in ids.iter().zip(&outcome.supports) {
        let world = &planned.routed.support;
        // A branch joins its trunk, added before it, at its height (a world value).
        let base = match planned.joins {
            Some((trunk, z_mm)) => BaseSpec::Support {
                parent: ids[trunk],
                z_mm,
            },
            None => world.base,
        };
        document.supports.push(ManualSupport {
            id: next,
            tip_mm: to_model.point(world.tip_mm),
            tip_axis: crate::supports_ui::route::axis_to_model(world.tip_axis, to_model),
            bends: world.bends.iter().map(|&b| to_model.point(b)).collect(),
            base: match base {
                BaseSpec::Model { point_mm, axis } => BaseSpec::Model {
                    point_mm: to_model.point(point_mm),
                    axis: crate::supports_ui::route::axis_to_model(axis, to_model),
                },
                other => other,
            },
            parameters: world.parameters.clone(),
            accept_collision: false,
            origin: world.origin,
            kind: world.kind,
        });
    }
    ids
}

/// The islands of `islands` (all, or only `only`) that Support the islands plans for on model
/// `index`: without a support, and (all of them) not smaller than the skip area.
pub fn wanted(
    workspace: &Workspace,
    index: usize,
    islands: &[Island],
    only: Option<usize>,
    skip_under_mm2: f64,
    join_mm: f64,
) -> Vec<(usize, Island)> {
    islands
        .iter()
        .copied()
        .enumerate()
        .filter(|&(k, island)| match only {
            Some(one) => k == one,
            None => island.area_mm2 >= skip_under_mm2,
        })
        .filter(|(_, island)| holder(workspace, index, island, join_mm).is_none())
        .collect()
}

/// A plan's preview: the kept supports' mesh and its centre.
pub type Preview = Option<(TriangleMesh, [f64; 3])>;

/// A plan running on its own thread, and what it is of.
pub struct Job<K> {
    pub key: K,
    pub stop: Arc<AtomicBool>,
    pub receiver: mpsc::Receiver<(Outcome, Preview)>,
}

impl<K> Drop for Job<K> {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Start planning supports under `islands` on its own thread.
#[allow(clippy::too_many_arguments)] // The plan's whole context.
pub fn start<K>(
    key: K,
    islands: Vec<(usize, Island)>,
    surface: Arc<RouteSurface>,
    next: NextSupport,
    document: Option<SupportDocument>,
    values: IslandSupports,
    volume: BuildVolume,
    checked: Option<(check::ModelInput, crate::gizmo::Affine)>,
) -> Option<Job<K>> {
    let stop = Arc::new(AtomicBool::new(false));
    let cancelled = Arc::clone(&stop);
    let (sender, receiver) = mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("island-supports".to_owned())
        .spawn(move || {
            let started = std::time::Instant::now();
            let outcome = plan(
                &islands,
                &surface,
                &next,
                document.as_ref(),
                &values,
                volume,
                checked.as_ref().map(|(model, to_model)| (model, to_model)),
                &cancelled,
            );
            let mesh = preview(&outcome, volume);
            log::debug!(
                target: "supports",
                "planned {} supports for {} islands ({} skipped) in {:.0} ms",
                outcome.supports.len(),
                outcome.held,
                outcome.skipped.len(),
                started.elapsed().as_secs_f64() * 1000.0
            );
            let _ = sender.send((outcome, mesh));
        });
    spawned.ok().map(|_| Job {
        key,
        stop,
        receiver,
    })
}
