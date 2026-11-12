//! The final check (M6.4i, TD-112), shared by the Automatic tool and the Islands tool's Support
//! the islands and Support it here: planned supports are built as a slice builds them, against
//! the model, with the kept ones. A planned support built with an error (its tip piercing a
//! thin part, a part too close to the model or outside the build volume,
//! a path the build refuses) goes, with why at its tip; the supports joining it (branches and
//! merged trunks, through others too) are grown again on supports of their own and checked in
//! the next round, a few rounds at most. Where a standard support goes, a mini support standing
//! on the model below its tip takes its place when the values allow one (owner, 2026-10-30), and
//! is checked in the next round too. A plan so checked never offers a support that a slice would
//! build with an error. A support entering a drain hole is only warned about (TD-131): it
//! stays, as holding the island comes first; but a tip where a hole cuts the material away
//! holds nothing there, so it goes as one with an error does.
use super::{add_planned, grow, Growing, Island, IslandSupports, Outcome, Planned, Skip, Tip};
use crate::{
    gizmo::Affine, islands_ui::PROBE_MM, preparation_ui::PreparationUi, supports_ui::SupportsUi,
    workspace::Workspace,
};
use karasu_core::{
    geometry::ObjectTransform,
    mesh::TriangleMesh,
    supports::{BracePlan, FindingKind, Preset, Severity, SupportDocument, SupportKind},
};
use karasu_project::pipeline::{ObjectPreparation, Preparation};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

/// The final check's rounds at most: a round regrows what hung from the supports it removed.
const CHECK_ROUNDS: usize = 3;
/// How far in plan from where an island starts a refusal the final check left there tells why
/// the island was left, in mm.
const START_REACH_MM: f64 = 2.0;

/// The model as a slice builds its supports: its mesh, placement and preparation, the printer,
/// and the braces' plan.
pub struct ModelInput {
    pub mesh: Arc<TriangleMesh>,
    pub placement: ObjectTransform,
    pub preparation: Arc<Preparation>,
    pub index: usize,
    pub printer: karasu_core::profiles::PrinterProfile,
    pub braces: BracePlan,
}

/// The supports a plan keeps and adds to: the document without what the plan replaces (model
/// frame), and the way from world to that frame.
pub struct Kept<'a> {
    pub document: Option<&'a SupportDocument>,
    pub to_model: &'a Affine,
}

/// Model `index` as a slice builds its supports, for the final check and braces: `None` until
/// it is placed and prepared. The braces take `braces`.
pub(crate) fn model_input(
    workspace: &Workspace,
    preparation: &PreparationUi,
    supports: &SupportsUi,
    index: usize,
) -> Option<ModelInput> {
    let object = workspace.plate().objects.get(index)?;
    let (shared, _) = preparation.shared(workspace, index)?;
    let loaded = workspace.meshes().get(&object.model)?;
    Some(ModelInput {
        mesh: Arc::clone(&loaded.mesh),
        placement: workspace.placement_of(index)?.transform,
        preparation: shared,
        index,
        printer: workspace.printer()?.clone(),
        braces: supports.bracing.new.plan(),
    })
}

/// Whether a tip at `tip` holds `island` where it starts (owner, 2026-10-30: there the resin
/// cures in mid-air): on its first layer's footprint, widened by
/// [`crate::islands_ui::HOLD_REACH_MM`] and by `join_mm` (islands that close share a support),
/// within [`super::START_MM`] above where it starts.
pub(crate) fn holds(island: &Island, [x, y, z]: [f64; 3], join_mm: f64) -> bool {
    let under = island.heights[0];
    let reach = crate::islands_ui::HOLD_REACH_MM + join_mm.max(0.0);
    let [[x0, y0], [x1, y1]] = island.footprint;
    (under - PROBE_MM..=under + super::START_MM + reach).contains(&z)
        && (x0 - reach..=x1 + reach).contains(&x)
        && (y0 - reach..=y1 + reach).contains(&y)
}

/// Whether one of `tips` holds `island` where it starts, islands closer than `join_mm` sharing
/// a support: see [`holds`].
pub(crate) fn held(island: &Island, tips: &[[f64; 3]], join_mm: f64) -> bool {
    tips.iter().any(|&tip| holds(island, tip, join_mm))
}

/// The distance between `a` and `b`.
pub(crate) fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1]).hypot(a[2] - b[2])
}

/// Why a finding's support cannot stay.
fn skip_of(kind: FindingKind) -> Skip {
    match kind {
        FindingKind::FootInHole => Skip::FootInHole,
        FindingKind::Collision => Skip::Collides,
        FindingKind::Piercing => Skip::Pierces,
        _ => Skip::Unbuildable,
    }
}

/// `planned` added to `kept`, built as a slice builds them against the model: the planned ones
/// built with an error, by their places, with why; `None` when they cannot be built.
fn built_errors(
    kept: &Kept<'_>,
    model: &ModelInput,
    prepared: &ObjectPreparation,
    planned: &Outcome,
    stop: &AtomicBool,
) -> Option<BTreeMap<usize, Skip>> {
    let mut document = kept
        .document
        .cloned()
        .unwrap_or_else(|| SupportDocument::new(Preset::Medium));
    let ids = add_planned(&mut document, kept.to_model, planned);
    let scene = karasu_project::pipeline::build_model_supports(
        &document,
        &model.mesh,
        &model.placement,
        prepared,
        &model.printer,
        &|_| stop.load(Ordering::Relaxed),
    )
    .ok()?;
    let place: HashMap<u64, usize> = ids.iter().enumerate().map(|(k, &id)| (id, k)).collect();
    let mut failed = BTreeMap::new();
    // A tip where a drain hole cuts the material away holds nothing (TD-131).
    let cuts = prepared.drain_cylinders();
    for (k, planned) in planned.supports.iter().enumerate() {
        if let Some(&(tip, _)) = planned.routed.anchors.first() {
            if cuts.iter().any(|c| c.removes(tip)) {
                failed.insert(k, Skip::DrainHole);
            }
        }
    }
    for f in scene
        .findings()
        .iter()
        .filter(|f| f.severity == Severity::Error)
    {
        if let Some(&k) = place.get(&f.entity) {
            failed.entry(k).or_insert(skip_of(f.kind));
        }
    }
    Some(failed)
}

/// `gone` (places among `supports`) with every support hanging from them, through others too.
fn with_hanging(supports: &[Planned], mut gone: BTreeSet<usize>) -> BTreeSet<usize> {
    loop {
        let more: Vec<usize> = (0..supports.len())
            .filter(|j| {
                !gone.contains(j) && supports[*j].joins.is_some_and(|(t, _)| gone.contains(&t))
            })
            .collect();
        if more.is_empty() {
            return gone;
        }
        gone.extend(more);
    }
}

/// The final check of `plan` (see the module) with `kept`'s supports against `model`: what it
/// removes leaves why at its tip in `why_at`; what hung from it is regrown with `growing`'s
/// values, without branches.
pub(crate) fn check_built(
    kept: &Kept<'_>,
    model: &ModelInput,
    plan: &mut Outcome,
    growing: &Growing<'_>,
    why_at: &mut Vec<([f64; 3], Skip)>,
    stop: &AtomicBool,
) {
    let Some(Ok(prepared)) = model.preparation.objects.get(model.index) else {
        return;
    };
    let alone = IslandSupports {
        branch_mm: 0.0,
        merge_mm: 0.0,
        ..*growing.values
    };
    let regrowing = Growing {
        values: &alone,
        ..*growing
    };
    for _ in 0..CHECK_ROUNDS {
        if stop.load(Ordering::Relaxed) || plan.supports.is_empty() {
            return;
        }
        let Some(failed) = built_errors(kept, model, prepared, plan, stop) else {
            return;
        };
        if failed.is_empty() {
            return;
        }
        // What hangs from them goes too, to be grown again.
        let supports = std::mem::take(&mut plan.supports);
        let gone = with_hanging(&supports, failed.keys().copied().collect());
        for (&k, &why) in &failed {
            why_at.push((supports[k].routed.support.tip_mm, why));
        }
        let Some(mesh) = regrowing.surface.mesh() else {
            return;
        };
        let again: Vec<(usize, Tip)> = gone
            .iter()
            .filter(|j| !failed.contains_key(j))
            .enumerate()
            .filter_map(|(g, &j)| {
                let (tip, normal) = *supports[j].routed.anchors.first()?;
                let cavity = regrowing
                    .surface
                    .grid()
                    .first_above(mesh, [tip[0], tip[1], tip[2] - 0.01], tip[2] + 0.01)
                    .is_some_and(|(_, _, t)| regrowing.surface.is_cavity(t));
                Some((g, (tip, normal, cavity)))
            })
            .collect();
        // A standard support refused (its tip piercing a thin part, a part too close to the
        // model) leaves a mini support at its tip, when the values allow one
        // (owner, 2026-10-30); a mini support refused is not tried again, nor a tip a drain
        // hole cuts away (TD-131).
        let minis: Vec<Planned> = failed
            .iter()
            .filter(|&(_, &why)| why != Skip::DrainHole)
            .map(|(k, _)| k)
            .filter(|_| growing.values.minis)
            .filter(|&&k| supports[k].routed.support.kind == SupportKind::Standard)
            .filter_map(|&k| {
                let (tip, normal) = *supports[k].routed.anchors.first()?;
                let parameters = growing.next.parameters();
                let (mut routed, _) = super::mini_built(
                    growing.surface,
                    &parameters,
                    growing.document,
                    growing.volume,
                    tip,
                    normal,
                )?;
                routed.support.origin = growing.origin;
                Some(Planned {
                    routed,
                    joins: None,
                })
            })
            .collect();
        let mut new_place = vec![None; supports.len()];
        for (j, planned) in supports.into_iter().enumerate() {
            if !gone.contains(&j) {
                new_place[j] = Some(plan.supports.len());
                plan.supports.push(planned);
            }
        }
        plan.supports.extend(minis);
        for p in &mut plan.supports {
            p.joins = p.joins.and_then(|(t, z)| new_place[t].map(|t| (t, z)));
        }
        let grove = grow(&again, again.len(), &regrowing, stop);
        for (g, (count, why)) in grove.kept.iter().zip(&grove.why).enumerate() {
            if *count == 0 {
                why_at.push((again[g].1 .0, *why));
            }
        }
        let base = plan.supports.len();
        plan.supports
            .extend(grove.supports.into_iter().map(|mut p| {
                p.joins = p.joins.map(|(trunk, z)| (trunk + base, z));
                p
            }));
        plan.branches = plan.supports.iter().filter(|p| p.joins.is_some()).count();
    }
}

/// Why the final check removed a support from where an island starts at `point`, if it did.
pub(crate) fn why_near(why_at: &[([f64; 3], Skip)], point: [f64; 3]) -> Option<Skip> {
    why_at
        .iter()
        .filter(|w| (w.0[0] - point[0]).hypot(w.0[1] - point[1]) <= START_REACH_MM)
        .min_by(|a, b| distance(a.0, point).total_cmp(&distance(b.0, point)))
        .map(|w| w.1)
}

/// Why the failed tip nearest `point` found no support; "no way" without one.
pub(crate) fn nearest_why(why_at: &[([f64; 3], Skip)], point: [f64; 3]) -> Skip {
    why_at
        .iter()
        .min_by(|a, b| distance(a.0, point).total_cmp(&distance(b.0, point)))
        .map_or(Skip::NoWay, |w| w.1)
}
