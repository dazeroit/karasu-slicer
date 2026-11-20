//! Arrange's planning (TD-135, phase 5; TD-162): where every model of the plates arranged
//! goes, worked out from a snapshot of them ([`ArrangeInput`]) on a thread of its own
//! ([`ArrangeJob`]), and made by the workspace as one undoable step
//! ([`crate::workspace::Workspace::finish_arrange`]).
//!
//! **Plates arranged:** the plate shown ("This plate"), or every plate ("Every plate", TD-162
//! phase 3): those with the shown plate's printer, or all plates with a printer, whatever their
//! resin and print setting. Each keeps its name and its place in the project; the models it held
//! may go to any of them, on one whose printer they fit by footprint and by height. Plates added
//! go after the last plate arranged, with the shown plate's profiles and settings.
//!
//! **Planning** measures every model (its turn about Z when asked, its footprint and its top),
//! then packs them plate by plate in the project's order, as packed by footprint
//! ([`Plan::packed`]) and, with "Group by height", grouped by height ([`Plan::by_height`]),
//! keeping the grouping when it prints sooner by the estimate ([`Plan::sooner`]).
//!
//! **Footprints** (TD-165): each model takes its rectangle ([`Footprint::Box`]), or its convex
//! outline seen from above ([`Footprint::Outline`]), packed by
//! [`karasu_core::plate::outline::arrange_outlines`]; everything else is the same either way.
use crate::workspace::{convex_footprint, fresh_name, placed_footprint, Overflow, Unarranged};
use karasu_core::{
    cancellation::Cancellation,
    geometry::Point3,
    mesh::TriangleMesh,
    plate::{
        arrange_turning, footprint_turn,
        outline::{
            self, arrange_outlines, bounded, bounds, translated, PlacedOutline,
            MAX_OUTLINE_VERTICES,
        },
        Placed, Rect, Unplaced,
    },
    print_job::uniform_time_s,
    profiles::{PrintSettings, PrinterProfile},
};
use karasu_project::{
    placement::{self, Placement},
    project::{ModelId, PlateContent, PlateId, PlateObject},
};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex, PoisonError,
    },
    thread,
};

/// Which plates Arrange works on (TD-162, phase 3; owner, 2026-11-19).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ArrangeScope {
    /// The plate shown, its overflow to new plates.
    #[default]
    ThisPlate,
    /// Every plate [`PlateChoice`] takes, the models of all of them arranged together.
    EveryPlate,
}

/// Which plates "Every plate" takes (owner, 2026-11-19).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PlateChoice {
    /// The plates with the shown plate's printer.
    #[default]
    SamePrinter,
    /// Every plate with a printer, whatever its resin and print setting.
    All,
}

/// Why "Every plate" leaves a plate out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LeftOut {
    /// It has no printer, so no plate to place on.
    NoPrinter,
    /// Its printer is not the shown plate's ([`PlateChoice::SamePrinter`]).
    AnotherPrinter,
}

pub use karasu_core::plate::Footprint;

/// How Arrange arranges (TD-135, TD-162).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ArrangeOptions {
    /// The least distance between footprints, in mm, on every plate.
    pub spacing_mm: f64,
    /// Where the models that do not fit go.
    pub overflow: Overflow,
    /// Models of similar height share a plate, with the overflow to new plates.
    pub by_height: bool,
    /// Models may turn about Z, by any angle, to take less room.
    pub turn: bool,
    /// The plate shown, or every plate.
    pub scope: ArrangeScope,
    /// Which plates "Every plate" takes.
    pub plates: PlateChoice,
    /// "Every plate" removes the plates it leaves empty.
    pub remove_empty: bool,
    /// Rectangles or outlines (TD-165).
    pub footprint: Footprint,
}

/// What Arrange did (TD-135, phase 5; TD-162).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Arranged {
    /// Models arranged on the plate shown (those staying there not counted).
    pub on_plate: usize,
    /// Models moved to new plates, and those plates' names.
    pub moved: usize,
    pub new_plates: Vec<String>,
    /// Models that fit nowhere, left where they were.
    pub stayed: usize,
    /// The indices the models that left the plate shown had there, ascending.
    pub removed: Vec<usize>,
    /// The models were grouped by height (TD-162): the plates' estimated time is shorter so,
    /// by `saved_s` seconds, or as short on fewer plates.
    pub grouped: bool,
    pub saved_s: u64,
    /// The models turned about Z (TD-162).
    pub turned: usize,
    /// "Every plate": the models arranged, the plates they were on, and the names of those
    /// removed for being left empty.
    pub every: Option<(usize, usize, Vec<String>)>,
}

pub use karasu_core::plate::TURN_MARGIN;

/// `object` turned `turn_deg` about Z (counter-clockwise from above), its Z rotation kept
/// within [-180°, 180°); unchanged for no turn. The fixed-axis order X, Y, Z makes the Z
/// rotation a turn about the plate's vertical, which its supports and preparation follow.
pub(crate) fn turned_by(object: &PlateObject, turn_deg: f64) -> PlateObject {
    let mut object = object.clone();
    if turn_deg != 0.0 {
        let z = object.pose.rotate_deg[2] + turn_deg;
        object.pose.rotate_deg[2] = (z + 180.0).rem_euclid(360.0) - 180.0;
    }
    object
}

/// `object` placed on a plate centred at `centre`, `mesh` being its model's: its footprint (the
/// model, its supports and raft, from above), the translation the placement used, and its top
/// above the plate. `None` when it cannot be placed.
pub(crate) fn measured(
    mesh: &TriangleMesh,
    centre: [f64; 2],
    object: &PlateObject,
) -> Option<(Rect, [f64; 3], f64)> {
    measure(mesh, centre, object, false).map(|m| (m.rect, m.translation, m.top))
}

/// What Arrange measures of a model placed on a plate.
pub(crate) struct Measure {
    /// Its footprint's rectangle (the model, its supports and raft, from above).
    pub(crate) rect: Rect,
    /// The translation the placement used.
    pub(crate) translation: [f64; 3],
    /// Its top above the plate.
    pub(crate) top: f64,
    /// With outlines (TD-165): its convex outline, bounded to
    /// [`MAX_OUTLINE_VERTICES`], relative to its reference point ([`reference()`]), and that point.
    pub(crate) outline: Option<(Vec<[f64; 2]>, [f64; 2])>,
}

/// [`measured`], with the outline when `outline` asks for it (TD-165).
pub(crate) fn measure(
    mesh: &TriangleMesh,
    centre: [f64; 2],
    object: &PlateObject,
    outline: bool,
) -> Option<Measure> {
    let placement = placement::place(
        mesh,
        object.translate_mm,
        &object.pose,
        centre,
        &mut Cancellation::never(),
    )
    .ok()?;
    let outline = if outline {
        let hull = convex_footprint(mesh, &placement, object)?;
        let origin = reference(&placement)?;
        let relative = translated(
            &bounded(&hull, MAX_OUTLINE_VERTICES),
            [-origin[0], -origin[1]],
        );
        Some((relative, origin))
    } else {
        None
    };
    Some(Measure {
        rect: placed_footprint(&placement, object)?,
        translation: placement.translation_mm,
        top: placement.max.z,
        outline,
    })
}

/// Where `placement` puts the model's own origin, in world XY (TD-165). A turn about Z turns the
/// model's outline about this point exactly as it turns the outline in the plate's frame: the
/// turn is the last rotation the placement applies, so points keep their places relative to it.
pub(crate) fn reference(placement: &Placement) -> Option<[f64; 2]> {
    let origin = placement
        .transform
        .to_affine()
        .ok()?
        .transform_point(Point3::new(0.0, 0.0, 0.0))
        .ok()?;
    Some([origin.x, origin.y])
}

/// `turn_deg` within [-180°, 180°).
fn wrapped(turn_deg: f64) -> f64 {
    (turn_deg + 180.0).rem_euclid(360.0) - 180.0
}

/// The turn about Z, in degrees, that makes `object`'s convex footprint smallest on a plate of
/// `plate` (width, depth), or makes it fit there (TD-162; [`footprint_turn`]): zero when it
/// saves less than [`TURN_MARGIN`]. `None` when it cannot be placed.
pub(crate) fn turn_of(
    mesh: &TriangleMesh,
    centre: [f64; 2],
    object: &PlateObject,
    plate: [f64; 2],
) -> Option<f64> {
    let placement = placement::place(
        mesh,
        object.translate_mm,
        &object.pose,
        centre,
        &mut Cancellation::never(),
    )
    .ok()?;
    let hull = convex_footprint(mesh, &placement, object)?;
    Some(footprint_turn(&hull, plate, TURN_MARGIN))
}

/// What Arrange plans from ([`crate::workspace::Workspace::arrange_input`]): a snapshot of the
/// plates arranged, so the planning can run on another thread.
pub struct ArrangeInput {
    pub(crate) options: ArrangeOptions,
    /// The plates arranged, in the project's order: identity, content and revision.
    pub(crate) plates: Vec<(PlateId, PlateContent, u64)>,
    /// The plate shown, among them: its name and profiles go to plates added.
    pub(crate) shown: usize,
    /// Their models' meshes, shared.
    pub(crate) meshes: HashMap<ModelId, Arc<TriangleMesh>>,
    /// Every plate's name when it started.
    pub(crate) taken: Vec<String>,
    /// Every plate, in order, when it started: unchanged when the arrangement is made.
    pub(crate) order: Vec<PlateId>,
    /// Where plates added go.
    pub(crate) at: usize,
}

/// An arrangement planned, to make with [`crate::workspace::Workspace::finish_arrange`].
pub struct Planned {
    /// Every plate, in order, and the revisions of those arranged, when it started.
    pub(crate) order: Vec<PlateId>,
    pub(crate) revisions: Vec<(PlateId, u64)>,
    /// The plates arranged with their new contents, the plates added at their positions, and
    /// the plates removed for being left empty.
    pub(crate) contents: Vec<(PlateId, PlateContent)>,
    pub(crate) added: Vec<(usize, PlateContent)>,
    pub(crate) removed: Vec<PlateId>,
    pub(crate) out: Arranged,
}

/// What Arrange is doing (TD-162, owner 2026-11-19): measuring the models, one after another, or
/// packing them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ArrangeStage {
    #[default]
    Measuring,
    Packing,
}

/// How far a planning has gone, and whether it is asked to stop; shared between the thread that
/// plans and the window that shows it.
#[derive(Debug, Default)]
pub struct ArrangeProgress {
    /// The stage, and the models measured of how many.
    state: Mutex<(ArrangeStage, usize, usize)>,
    stop: AtomicBool,
}

impl ArrangeProgress {
    /// The stage, and the models measured of how many.
    #[must_use]
    pub fn state(&self) -> (ArrangeStage, usize, usize) {
        *self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Ask the planning to stop at its next step.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    fn stopped(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }

    fn set(&self, stage: ArrangeStage, done: usize, total: usize) {
        *self.state.lock().unwrap_or_else(PoisonError::into_inner) = (stage, done, total);
    }
}

/// An arrangement planned on a thread of its own (TD-162, owner 2026-11-19), so the window keeps
/// drawing its progress and its Cancel.
pub struct ArrangeJob {
    progress: Arc<ArrangeProgress>,
    result: mpsc::Receiver<Result<Planned, Unarranged>>,
}

impl ArrangeJob {
    /// Start planning `input` on a thread of its own.
    #[must_use]
    pub fn start(input: ArrangeInput) -> Self {
        let progress = Arc::new(ArrangeProgress::default());
        let shared = Arc::clone(&progress);
        let (sender, result) = mpsc::channel();
        thread::spawn(move || {
            let planned = plan_arrangement(&input, &shared);
            // The receiver is gone only when the job was dropped: nothing waits for it.
            let _ = sender.send(planned);
        });
        Self { progress, result }
    }

    /// How far it has gone.
    #[must_use]
    pub fn progress(&self) -> (ArrangeStage, usize, usize) {
        self.progress.state()
    }

    /// Ask it to stop: it plans no further, and its result is dropped with it.
    pub fn cancel(&self) {
        self.progress.stop();
    }

    /// Its result, once it is ready.
    #[must_use]
    pub fn poll(&self) -> Option<Result<Planned, Unarranged>> {
        match self.result.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(Err(Unarranged::Cancelled)),
        }
    }
}

/// A plate models are placed on: its room, how high its printer prints, its centre, and what
/// its time is estimated with.
#[derive(Clone)]
struct Target {
    area: Rect,
    height: f64,
    centre: [f64; 2],
    printer: PrinterProfile,
    settings: Option<PrintSettings>,
}

impl Target {
    /// The plate `content` gives, `None` without a printer.
    fn of(content: &PlateContent) -> Option<Self> {
        let printer = content.printer.clone()?;
        let [ox, oy] = printer.origin_xy_mm;
        let [width, depth, height] = printer.build_size_mm;
        Some(Self {
            area: Rect {
                min: [ox, oy],
                max: [ox + width, oy + depth],
            },
            height,
            centre: placement::plate_centre(&printer),
            settings: content.calibration().cloned(),
            printer,
        })
    }

    fn size(&self) -> [f64; 2] {
        self.area.size()
    }
}

/// The models' footprints, as the packer chosen takes them (TD-165).
#[derive(Clone, Copy)]
enum Shapes<'a> {
    /// Rectangles' sizes, and whether each may lie a quarter turn round (TD-162).
    Box {
        sizes: &'a [[f64; 2]],
        turnable: &'a [bool],
    },
    /// Convex outlines, relative to each model's reference point, in each orientation offered.
    Outline {
        orientations: &'a [Vec<Vec<[f64; 2]>>],
    },
}

/// Where the packer put a model: a rectangle's lowest corner, or an outline's orientation and
/// where its reference point goes.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Spot {
    Box(Placed),
    Outline(PlacedOutline),
}

/// The models to place: their footprints and their tops.
struct Models<'a> {
    shapes: Shapes<'a>,
    tops: &'a [f64],
}

impl Models<'_> {
    /// Whether model `i` fits `target` alone: by its top, and by its footprint any way it may
    /// lie.
    fn fit(&self, i: usize, target: &Target, spacing: f64) -> bool {
        self.tops[i] <= target.height && self.arrange(&[i], target.area, &[], spacing).is_ok()
    }

    /// The room model `i` takes, as the models are ordered by it.
    fn room(&self, i: usize) -> f64 {
        match self.shapes {
            Shapes::Box { sizes, .. } => sizes[i][0] * sizes[i][1],
            Shapes::Outline { orientations } => outline::area(&orientations[i][0]),
        }
    }

    /// `items` packed on `area` `spacing` apart and clear of `obstacles`, by the packer chosen.
    fn arrange(
        &self,
        items: &[usize],
        area: Rect,
        obstacles: &[Vec<[f64; 2]>],
        spacing: f64,
    ) -> Result<Vec<Spot>, Unplaced> {
        match self.shapes {
            Shapes::Box { sizes, turnable } => {
                let wanted: Vec<[f64; 2]> = items.iter().map(|&i| sizes[i]).collect();
                let free: Vec<bool> = items.iter().map(|&i| turnable[i]).collect();
                let rects: Vec<Rect> = obstacles.iter().map(|o| bounds(o)).collect();
                arrange_turning(&wanted, &free, area, &rects, spacing)
                    .map(|placed| placed.into_iter().map(Spot::Box).collect())
            }
            Shapes::Outline { orientations } => {
                let wanted: Vec<Vec<Vec<[f64; 2]>>> =
                    items.iter().map(|&i| orientations[i].clone()).collect();
                arrange_outlines(&wanted, area, obstacles, spacing)
                    .map(|placed| placed.into_iter().map(Spot::Outline).collect())
            }
        }
    }

    /// The polygon model `i` covers where it was put at `spot`: an obstacle to those after it.
    fn cover(&self, i: usize, spot: Spot) -> Vec<[f64; 2]> {
        match (self.shapes, spot) {
            (Shapes::Box { sizes, .. }, Spot::Box(placed)) => corners(lying(sizes[i], placed)),
            (Shapes::Outline { orientations }, Spot::Outline(placed)) => {
                translated(&orientations[i][placed.orientation], placed.offset)
            }
            // A spot always comes from the models' own packer.
            _ => Vec::new(),
        }
    }
}

/// The corners of `rect`, counter-clockwise from its lowest: the polygon it is.
fn corners(rect: Rect) -> Vec<[f64; 2]> {
    vec![
        rect.min,
        [rect.max[0], rect.min[1]],
        rect.max,
        [rect.min[0], rect.max[1]],
    ]
}

/// Where Arrange puts each model: the models of each plate arranged (in their order) with their
/// spots, then each new plate's, and those that fit nowhere and stay.
struct Plan {
    plates: Vec<Vec<(usize, Spot)>>,
    stayed: Vec<usize>,
}

impl Plan {
    /// As Arrange packs (TD-135): the models of `items` packed by footprint, the largest first,
    /// on each of `targets` in turn, clear of its `obstacles`, each taking those that fit it;
    /// with the overflow to new plates, those left packed onto plates like `new` in turn.
    #[allow(clippy::too_many_arguments)] // The models, the plates and the overflow's way.
    fn packed(
        items: &[usize],
        models: &Models<'_>,
        targets: &[Target],
        obstacles: &[Vec<Vec<[f64; 2]>>],
        new: &Target,
        spacing: f64,
        overflow: Overflow,
    ) -> Self {
        let mut left = items.to_vec();
        let mut plates = Vec::new();
        for (k, target) in targets.iter().enumerate() {
            let allowed: Vec<usize> = left
                .iter()
                .copied()
                .filter(|&i| models.tops[i] <= target.height)
                .collect();
            let (placed, _) = pack(&allowed, models, target.area, &obstacles[k], spacing);
            left.retain(|i| !placed.iter().any(|(p, _)| p == i));
            plates.push(placed);
        }
        if overflow == Overflow::NewPlates {
            loop {
                let allowed: Vec<usize> = left
                    .iter()
                    .copied()
                    .filter(|&i| models.tops[i] <= new.height)
                    .collect();
                let (on_new, _) = pack(&allowed, models, new.area, &[], spacing);
                if on_new.is_empty() {
                    break;
                }
                left.retain(|i| !on_new.iter().any(|(p, _)| p == i));
                plates.push(on_new);
            }
        }
        Self {
            plates,
            stayed: left,
        }
    }

    /// Grouped by height (TD-162), after the first-fit, longest-processing-time rule for a batch
    /// processing machine, where a batch lasts as long as its longest job (Uzsoy, 1994):
    /// - the models by decreasing height (`tops`), then decreasing footprint, then their order;
    /// - each plate of `targets` in turn, then new plates like `new`, takes the longest run of
    ///   the tallest left that fit it, found by bisection, then the shorter ones left that fit
    ///   around them: a shorter model on a taller plate costs no time;
    /// - a model that fits no plate stays.
    fn by_height(
        items: &[usize],
        models: &Models<'_>,
        targets: &[Target],
        new: &Target,
        spacing: f64,
    ) -> Self {
        let (mut order, stayed): (Vec<usize>, Vec<usize>) = items.iter().partition(|&&i| {
            targets
                .iter()
                .chain(std::iter::once(new))
                .any(|t| models.fit(i, t, spacing))
        });
        order.sort_by(|&a, &b| {
            models.tops[b]
                .total_cmp(&models.tops[a])
                .then(models.room(b).total_cmp(&models.room(a)))
                .then(a.cmp(&b))
        });
        let mut plates = Vec::new();
        for target in targets {
            let plate = fill(&mut order, models, target, spacing);
            plates.push(plate);
        }
        while !order.is_empty() {
            let plate = fill(&mut order, models, new, spacing);
            if plate.is_empty() {
                break;
            }
            plates.push(plate);
        }
        let mut stayed = stayed;
        stayed.extend(order);
        stayed.sort_unstable();
        Self { plates, stayed }
    }

    /// `packed`, or `grouped` when it prints sooner by the estimate (TD-162): shorter by a
    /// second or more, or as long within a second on fewer plates. With `grouped` chosen, the
    /// seconds it saves.
    fn sooner(packed: Self, grouped: Option<Self>, times: &Times<'_>) -> (Self, Option<u64>) {
        let Some(grouped) = grouped else {
            return (packed, None);
        };
        let before = packed.time_s(times);
        let after = grouped.time_s(times);
        // Within a second the two take the same time: fewer plates decide, then the packing.
        let better = if (before - after).abs() < 1.0 {
            grouped.plates(times) < packed.plates(times)
        } else {
            after < before
        };
        if better {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // Seconds.
            let saved = (before - after).max(0.0).round() as u64;
            (grouped, Some(saved))
        } else {
            (packed, None)
        }
    }

    /// The plate each model lands on, its index into the plan's plates; the models that stay
    /// count on the plate they come from.
    fn plate_of(&self, i: usize, times: &Times<'_>) -> Option<usize> {
        self.plates
            .iter()
            .position(|plate| plate.iter().any(|&(p, _)| p == i))
            .or_else(|| self.stayed.contains(&i).then(|| times.origin[i]))
    }

    /// How many plates hold a model.
    fn plates(&self, times: &Times<'_>) -> usize {
        let mut held: Vec<usize> = (0..times.origin.len())
            .filter_map(|i| self.plate_of(i, times))
            .collect();
        held.sort_unstable();
        held.dedup();
        held.len()
    }

    /// The plates' estimated time (TD-162): each prints as many layers as its tallest model
    /// needs, the models staying counted on the plate they come from, timed as a print job of
    /// that many calibrated layers with the plate's print setting on its printer. Without a
    /// print setting, the heights stand for the time, which they are proportional to.
    fn time_s(&self, times: &Times<'_>) -> f64 {
        let count = self.plates.len().max(times.targets.len());
        let mut tops = vec![0.0f64; count];
        for i in 0..times.origin.len() {
            if let Some(k) = self.plate_of(i, times) {
                tops[k] = tops[k].max(times.tops[i]);
            }
        }
        tops.iter()
            .enumerate()
            .map(|(k, &top)| {
                let target = times.targets.get(k).unwrap_or(times.new);
                match &target.settings {
                    Some(settings) => {
                        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                        let layers = (top / settings.layer_height_mm).ceil().max(0.0) as u64;
                        uniform_time_s(&target.printer, settings, layers)
                    }
                    None => top,
                }
            })
            .sum()
    }
}

/// What a plan's time is estimated from: the models' tops and the plates they come from, the
/// plates arranged and the plates added's kind.
struct Times<'a> {
    tops: &'a [f64],
    origin: &'a [usize],
    targets: &'a [Target],
    new: &'a Target,
}

/// The models of `order` (tallest first) that `target` takes, taken out of it: the longest run of
/// those that fit it, found by bisection, then those left around them.
fn fill(
    order: &mut Vec<usize>,
    models: &Models<'_>,
    target: &Target,
    spacing: f64,
) -> Vec<(usize, Spot)> {
    let candidates: Vec<usize> = order
        .iter()
        .copied()
        .filter(|&i| models.fit(i, target, spacing))
        .collect();
    if candidates.is_empty() {
        return Vec::new();
    }
    // The first `k` of the candidates packed together, if they all fit.
    let run = |k: usize| {
        models
            .arrange(&candidates[..k], target.area, &[], spacing)
            .ok()
            .map(|placed| {
                candidates[..k]
                    .iter()
                    .copied()
                    .zip(placed)
                    .collect::<Vec<_>>()
            })
    };
    // One fits alone; the longest run that fits, by bisection.
    let (mut low, mut high) = (1, candidates.len());
    let mut best = run(high);
    if best.is_some() {
        low = high;
    } else {
        best = run(1);
        while high - low > 1 {
            let middle = low + (high - low) / 2;
            match run(middle) {
                Some(placed) => {
                    low = middle;
                    best = Some(placed);
                }
                None => high = middle,
            }
        }
    }
    let mut plate = best.unwrap_or_default();
    let obstacles: Vec<Vec<[f64; 2]>> = plate
        .iter()
        .map(|&(i, spot)| models.cover(i, spot))
        .collect();
    let (around, _) = pack(&candidates[low..], models, target.area, &obstacles, spacing);
    plate.extend(around);
    order.retain(|i| !plate.iter().any(|(p, _)| p == i));
    plate
}

/// The rectangle a footprint of `size` covers where it was `placed`, turned or not.
fn lying(size: [f64; 2], placed: Placed) -> Rect {
    let [w, d] = if placed.turned {
        [size[1], size[0]]
    } else {
        size
    };
    Rect {
        min: placed.corner,
        max: [placed.corner[0] + w, placed.corner[1] + d],
    }
}

/// `items` packed on `area` `spacing` apart and clear of `obstacles` by the models' packer:
/// those placed, and those left, increasing. The packer lists what does not fit; those are left
/// out and the rest packed again, until all that remain fit.
fn pack(
    items: &[usize],
    models: &Models<'_>,
    area: Rect,
    obstacles: &[Vec<[f64; 2]>],
    spacing: f64,
) -> (Vec<(usize, Spot)>, Vec<usize>) {
    let mut keep = items.to_vec();
    let mut left = Vec::new();
    while !keep.is_empty() {
        match models.arrange(&keep, area, obstacles, spacing) {
            Ok(placed) => {
                left.sort_unstable();
                return (keep.into_iter().zip(placed).collect(), left);
            }
            Err(unplaced) => {
                for &k in unplaced.items.iter().rev() {
                    left.push(keep.remove(k));
                }
            }
        }
    }
    left.sort_unstable();
    (Vec::new(), left)
}

/// Plan the arrangement of `input` (TD-135, TD-162), reporting to `progress` and stopping when
/// it asks: measuring every model (its turn, its footprint and its top), then packing.
///
/// - **Packing:** see [`Plan::packed`] and, with the overflow to new plates and "Group by
///   height", [`Plan::by_height`], kept when [`Plan::sooner`] finds it prints sooner.
/// - **Turns about Z:** each model first takes the turn that makes its convex footprint
///   smallest, when that saves at least [`TURN_MARGIN`] of its room, or that makes it fit a
///   plate it fits no other way; the packer may then turn it a quarter turn more. The turn
///   changes only the model's Z rotation, which its supports and preparation follow.
/// - **"Every plate" with "Stay where they are":** a model that fits nowhere stays where it was,
///   on its plate, and that plate is packed around it; each round that leaves more such models
///   packs again around them all.
///
/// It touches no workspace, so it runs on a thread of its own ([`ArrangeJob`]).
///
/// # Errors
/// [`Unarranged::NoPrinter`] for a plate arranged without a printer; [`Unarranged::Loading`]
/// when a model cannot be placed; [`Unarranged::Cancelled`] when asked to stop.
#[allow(clippy::too_many_lines)] // Measuring, packing and the result, in order.
pub fn plan_arrangement(
    input: &ArrangeInput,
    progress: &ArrangeProgress,
) -> Result<Planned, Unarranged> {
    let options = input.options;
    let targets = input
        .plates
        .iter()
        .map(|(_, content, _)| Target::of(content))
        .collect::<Option<Vec<_>>>()
        .ok_or(Unarranged::NoPrinter)?;
    let new = targets[input.shown].clone();
    // Every model, plate by plate in order: the plate it comes from and the object.
    let items: Vec<(usize, &PlateObject)> = input
        .plates
        .iter()
        .enumerate()
        .flat_map(|(k, (_, content, _))| content.objects.iter().map(move |o| (k, o)))
        .collect();
    let count = items.len();
    let outlines = options.footprint == Footprint::Outline;
    let mut origin = Vec::with_capacity(count);
    let mut turns = Vec::with_capacity(count);
    let mut turned = Vec::with_capacity(count);
    let mut footprints = Vec::with_capacity(count);
    let mut tops = Vec::with_capacity(count);
    // With outlines (TD-165): each model's turns offered, and its outline turned by each.
    let mut offered: Vec<Vec<f64>> = Vec::new();
    let mut orientations: Vec<Vec<Vec<[f64; 2]>>> = Vec::new();
    for (done, &(k, object)) in items.iter().enumerate() {
        if progress.stopped() {
            return Err(Unarranged::Cancelled);
        }
        progress.set(ArrangeStage::Measuring, done, count);
        let mesh = input.meshes.get(&object.model).ok_or(Unarranged::Loading)?;
        let centre = targets[k].centre;
        let turn = if options.turn {
            turn_of(mesh, centre, object, new.size()).ok_or(Unarranged::Loading)?
        } else {
            0.0
        };
        let object = turned_by(object, turn);
        let measure = measure(mesh, centre, &object, outlines).ok_or(Unarranged::Loading)?;
        if let Some((relative, _)) = &measure.outline {
            let offers = karasu_core::plate::turns_offered(options.turn, turn);
            orientations.push(
                offers
                    .iter()
                    .map(|&t| outline::turned(relative, t))
                    .collect(),
            );
            offered.push(offers);
        }
        origin.push(k);
        turns.push(turn);
        turned.push(object);
        footprints.push((measure.rect, measure.translation));
        tops.push(measure.top);
    }
    if progress.stopped() {
        return Err(Unarranged::Cancelled);
    }
    progress.set(ArrangeStage::Packing, count, count);
    let sizes: Vec<[f64; 2]> = footprints.iter().map(|(rect, _)| rect.size()).collect();
    let turnable = vec![options.turn; count];
    let models = Models {
        shapes: if outlines {
            Shapes::Outline {
                orientations: &orientations,
            }
        } else {
            Shapes::Box {
                sizes: &sizes,
                turnable: &turnable,
            }
        },
        tops: &tops,
    };
    let times = Times {
        tops: &tops,
        origin: &origin,
        targets: &targets,
        new: &new,
    };
    let all: Vec<usize> = (0..count).collect();
    let spacing = options.spacing_mm;
    let every = options.scope == ArrangeScope::EveryPlate;
    let packed = if every && options.overflow == Overflow::Stay {
        // What stays keeps its place on its plate, which is packed around it.
        let mut stays: Vec<usize> = Vec::new();
        loop {
            let mut obstacles = vec![Vec::new(); targets.len()];
            for &i in &stays {
                let (_, object) = items[i];
                let mesh = &input.meshes[&object.model];
                let stays = measure(mesh, targets[origin[i]].centre, object, outlines)
                    .ok_or(Unarranged::Loading)?;
                // Where it stands: its rectangle, or its outline moved back to its place.
                obstacles[origin[i]].push(match stays.outline {
                    Some((relative, at)) => translated(&relative, at),
                    None => corners(stays.rect),
                });
            }
            let free: Vec<usize> = all.iter().copied().filter(|i| !stays.contains(i)).collect();
            let mut plan = Plan::packed(
                &free,
                &models,
                &targets,
                &obstacles,
                &new,
                spacing,
                Overflow::Stay,
            );
            if plan.stayed.is_empty() {
                plan.stayed = stays;
                break plan;
            }
            stays.extend(plan.stayed.iter().copied());
            stays.sort_unstable();
        }
    } else {
        let obstacles = vec![Vec::new(); targets.len()];
        Plan::packed(
            &all,
            &models,
            &targets,
            &obstacles,
            &new,
            spacing,
            options.overflow,
        )
    };
    let grouped = (options.by_height && options.overflow == Overflow::NewPlates)
        .then(|| Plan::by_height(&all, &models, &targets, &new, spacing));
    let (plan, saved) = Plan::sooner(packed, grouped, &times);
    if progress.stopped() {
        return Err(Unarranged::Cancelled);
    }
    // Each model as it lands: turned as planned, then moved to its spot.
    let mut landed: Vec<Option<PlateObject>> = vec![None; count];
    let mut out = Arranged {
        grouped: saved.is_some(),
        saved_s: saved.unwrap_or(0),
        ..Arranged::default()
    };
    for (k, plate) in plan.plates.iter().enumerate() {
        let target = targets.get(k).unwrap_or(&new);
        for &(i, spot) in plate {
            let mesh = &input.meshes[&turned[i].model];
            let (object, more) = match spot {
                // A quarter turn more where the packer turned it, its footprint's lowest corner
                // at the corner planned.
                Spot::Box(placed) => {
                    let (mut object, (rect, translation)) = if placed.turned {
                        let object = turned_by(&turned[i], 90.0);
                        let (rect, translation, _) =
                            measured(mesh, target.centre, &object).ok_or(Unarranged::Loading)?;
                        (object, (rect, translation))
                    } else {
                        (turned[i].clone(), footprints[i])
                    };
                    object.translate_mm[0] = translation[0] + placed.corner[0] - rect.min[0];
                    object.translate_mm[1] = translation[1] + placed.corner[1] - rect.min[1];
                    (object, if placed.turned { 90.0 } else { 0.0 })
                }
                // The orientation's turn, its reference point where the packer put it.
                Spot::Outline(placed) => {
                    let more = offered[i][placed.orientation];
                    let mut object = turned_by(&turned[i], more);
                    let placement = placement::place(
                        mesh,
                        object.translate_mm,
                        &object.pose,
                        target.centre,
                        &mut Cancellation::never(),
                    )
                    .map_err(|_| Unarranged::Loading)?;
                    let at = reference(&placement).ok_or(Unarranged::Loading)?;
                    let translation = placement.translation_mm;
                    object.translate_mm[0] = translation[0] + placed.offset[0] - at[0];
                    object.translate_mm[1] = translation[1] + placed.offset[1] - at[1];
                    (object, more)
                }
            };
            let mut object = object;
            object.pose.center = false;
            if wrapped(turns[i] + more) != 0.0 {
                out.turned += 1;
            }
            landed[i] = Some(object);
        }
    }
    // Each plate arranged: the models landed on it and those staying on it, in their order.
    let held = |k: usize| -> Vec<PlateObject> {
        (0..count)
            .filter_map(|i| match plan.plate_of(i, &times) {
                Some(p) if p == k => Some(landed[i].clone().unwrap_or_else(|| items[i].1.clone())),
                _ => None,
            })
            .collect()
    };
    let mut contents = Vec::with_capacity(input.plates.len());
    let mut removed = Vec::new();
    let mut emptied = Vec::new();
    for (k, (id, content, _)) in input.plates.iter().enumerate() {
        let objects = held(k);
        if every && options.remove_empty && objects.is_empty() && !content.objects.is_empty() {
            removed.push(*id);
            emptied.push(content.name.clone());
        }
        contents.push((
            *id,
            PlateContent {
                objects,
                ..content.clone()
            },
        ));
    }
    // The plates added, named after the plate shown ("Arms (2)").
    let shown = &input.plates[input.shown].1;
    let mut added = Vec::new();
    let mut number = 2;
    for k in targets.len()..plan.plates.len() {
        let objects = held(k);
        if objects.is_empty() {
            continue;
        }
        let name = fresh_name(&shown.name, &mut number, |name| {
            input.taken.iter().chain(&out.new_plates).any(|t| t == name)
        });
        out.moved += objects.len();
        added.push((
            input.at + added.len(),
            PlateContent {
                name: name.clone(),
                objects,
                ..shown.clone()
            },
        ));
        out.new_plates.push(name);
    }
    out.on_plate = plan.plates.get(input.shown).map_or(0, Vec::len);
    out.stayed = plan.stayed.len();
    out.removed = (0..count)
        .filter(|&i| origin[i] == input.shown && plan.plate_of(i, &times) != Some(input.shown))
        .map(|i| i - origin.iter().position(|&o| o == input.shown).unwrap_or(0))
        .collect();
    if every {
        out.every = Some((count, input.plates.len(), emptied));
    }
    Ok(Planned {
        order: input.order.clone(),
        revisions: input
            .plates
            .iter()
            .map(|(id, _, revision)| (*id, *revision))
            .collect(),
        contents,
        added,
        removed,
        out,
    })
}
