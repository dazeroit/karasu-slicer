//! The Orient tool's search without the desktop application (TD-119, TD-158): tilts of one
//! placed model proposed under the user's constraints, each checked as a slice would check it.
//!
//! **Two passes** ([`search`]):
//! - the rough pass ([`karasu_core::analysis::orientation::rough`]) scores every tilt the
//!   constraints allow from the prepared mesh: the hollowed shell when the model is hollowed,
//!   otherwise the model placed;
//! - the best [`CHECKED`] distinct tilts, at least 8 degrees apart, are checked as a slice would
//!   check them: the model alone is sliced in that pose for its islands and, hollowed, its
//!   suction cups as the Drain holes tool shows them; its largest layer comes from the mesh's
//!   sections every 0.5 mm;
//! - the checked ones are ranked again with every measure, and the best [`SHOWN`] proposed.
//!
//! **The turn.** A candidate turns the model about its centre and moves it up so it rests where
//! it rested ([`turned`], [`turn_rows`]); its rotation is the tilt composed with the placement's
//! fixed-axis angles ([`crate::placement::euler_matrix`]).
//!
//! **Determinism.** The rough pass and the slices are deterministic and nothing depends on the
//! thread count, so equal inputs give equal candidates on one platform. The directions and
//! rotations use the platform's math library (`sin_cos`, `atan2`, `asin`), so, as for the support
//! planners (TD-157 item 6), equal candidates are not promised across platforms.
use crate::{
    pipeline::{slice_with, Observer, Preparation, SuctionCups},
    placement::{euler_matrix, matrix_euler},
    request::{SliceRequest, Subject},
};
use karasu_core::{
    analysis::orientation::{
        distinct, largest_section_mm2, rank, rough, Measures, Protected, RoughRequest, Tilt,
        TiltAxes, Weights, LOWEST_POINT_MM2,
    },
    analysis::unsupported::RegionKind,
    cancellation::{Cancellation, Checkpoint},
    geometry::{ObjectTransform, Point3},
    mesh::TriangleMesh,
};
use serde::{Deserialize, Serialize};
use std::{
    fmt,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

/// The most tilt from the pose now a user may set, in degrees.
pub const LIMIT_DEG: std::ops::RangeInclusive<f64> = 1.0..=180.0;
/// How far round a protected point its surface is kept clean, in mm.
pub const KEEP_AWAY_MM: std::ops::RangeInclusive<f64> = 0.0..=50.0;
/// A weight of the score.
pub const WEIGHT: std::ops::RangeInclusive<f64> = 0.0..=10.0;
/// The tilts checked as a slice would, after the rough pass.
pub const CHECKED: usize = 6;
/// The candidates proposed.
pub const SHOWN: usize = 5;
/// Candidates are at least this far apart, in degrees.
const APART_DEG: f64 = 8.0;
/// The sections measuring the largest layer are this far apart, in mm.
const SECTION_STEP_MM: f64 = 0.5;

/// The plate axes a tilt may turn about.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Axes {
    /// Any horizontal axis.
    #[default]
    Any,
    /// The plate's X axis only.
    X,
    /// The plate's Y axis only.
    Y,
}

impl Axes {
    /// The axes, as the panel lists them.
    pub const ALL: [Self; 3] = [Self::Any, Self::X, Self::Y];

    fn core(self) -> TiltAxes {
        match self {
            Self::Any => TiltAxes::Any,
            Self::X => TiltAxes::X,
            Self::Y => TiltAxes::Y,
        }
    }
}

/// What the score favours: each sets the weights.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    /// Less surface facing the plate and fewer islands and suction cups first.
    FewerSupports,
    /// Supports first, height and the largest layer counting too.
    #[default]
    Balanced,
    /// A lower print first: fewer layers.
    Faster,
}

impl Priority {
    /// The priorities, as the panel lists them.
    pub const ALL: [Self; 3] = [Self::FewerSupports, Self::Balanced, Self::Faster];

    /// The weights it sets.
    #[must_use]
    pub const fn weights(self) -> WeightValues {
        let [facing, islands, cups, height, largest] = match self {
            Self::FewerSupports => [2.0, 2.0, 2.0, 0.25, 0.25],
            Self::Balanced => [1.0, 1.0, 2.0, 0.5, 0.5],
            Self::Faster => [0.5, 0.5, 1.0, 2.0, 0.5],
        };
        WeightValues {
            facing,
            islands,
            cups,
            height,
            largest,
            protected: 4.0,
        }
    }
}

/// The weights of the score, as the user keeps them.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WeightValues {
    /// The area facing the plate past the critical angle.
    pub facing: f64,
    /// The islands.
    pub islands: f64,
    /// The suction cups, for a hollowed model.
    pub cups: f64,
    /// The height.
    pub height: f64,
    /// The largest layer's area.
    pub largest: f64,
    /// The area facing the plate and the islands near protected points.
    pub protected: f64,
}

impl Default for WeightValues {
    fn default() -> Self {
        Priority::Balanced.weights()
    }
}

impl WeightValues {
    fn core(self) -> Weights {
        Weights {
            facing: self.facing,
            islands: self.islands,
            cups: self.cups,
            height: self.height,
            largest: self.largest,
            protected: self.protected,
        }
    }
}

/// The tool's values (owner mockup): constraints and priority, kept while the user works, from
/// the defaults in Settings.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OrientValues {
    /// The plate axes a tilt may turn about.
    pub axes: Axes,
    /// The tilt is limited to `limit_deg` from the pose now (owner: on at 30° by default).
    pub limited: bool,
    /// The most tilt from the pose now when `limited`, in degrees.
    pub limit_deg: f64,
    /// How far round a protected point its surface is kept clean, in mm.
    pub keep_away_mm: f64,
    /// What the score favours; it sets `weights`.
    pub priority: Priority,
    /// How much each measure counts in the score.
    pub weights: WeightValues,
}

/// The values the tool starts from when the settings hold none.
pub const DEFAULT_ORIENT: OrientValues = OrientValues {
    axes: Axes::Any,
    limited: true,
    limit_deg: 30.0,
    keep_away_mm: 5.0,
    priority: Priority::Balanced,
    weights: Priority::Balanced.weights(),
};

impl Default for OrientValues {
    fn default() -> Self {
        DEFAULT_ORIENT
    }
}

impl OrientValues {
    /// These values with each one out of its range replaced by the built-in one.
    #[must_use]
    pub fn valid(self) -> Self {
        let d = DEFAULT_ORIENT;
        let weight = |v: f64, d: f64| if WEIGHT.contains(&v) { v } else { d };
        let w = self.weights;
        Self {
            limit_deg: if LIMIT_DEG.contains(&self.limit_deg) {
                self.limit_deg
            } else {
                d.limit_deg
            },
            keep_away_mm: if KEEP_AWAY_MM.contains(&self.keep_away_mm) {
                self.keep_away_mm
            } else {
                d.keep_away_mm
            },
            weights: WeightValues {
                facing: weight(w.facing, d.weights.facing),
                islands: weight(w.islands, d.weights.islands),
                cups: weight(w.cups, d.weights.cups),
                height: weight(w.height, d.weights.height),
                largest: weight(w.largest, d.weights.largest),
                protected: weight(w.protected, d.weights.protected),
            },
            ..self
        }
    }
}

/// A candidate orientation, with its measures.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Candidate {
    /// The tilt from the pose the search started from.
    pub tilt: Tilt,
    /// The model's rotation once turned, in degrees (fixed X, then Y, then Z).
    pub rotate_deg: [f64; 3],
    /// How far it moves up to rest where it rested, in mm (world Z).
    pub dz: f64,
    /// Area facing the plate past the critical angle, in mm².
    pub facing_mm2: f64,
    /// Islands, where the print starts in mid-air.
    pub islands: usize,
    /// Suction cups as the Drain holes tool shows them, for a hollowed model.
    pub cups: Option<usize>,
    /// Height, in mm.
    pub height_mm: f64,
    /// The largest layer's area, in mm².
    pub largest_mm2: f64,
    /// Area facing the plate near protected points, in mm².
    pub protected_mm2: f64,
    /// Islands near protected points.
    pub protected_islands: usize,
}

/// What a search is doing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// Scoring every tilt from the mesh.
    Rough,
    /// Checking the best ones as a slice would: this one of so many.
    Checking(usize, usize),
}

/// A search's result.
#[derive(Clone, Debug, PartialEq)]
pub struct Found {
    /// The candidates, best first: at most [`SHOWN`].
    pub candidates: Vec<Candidate>,
    /// Tilts scored in the rough pass.
    pub scored: usize,
    /// The model's centre then (the turn's pivot), in world mm.
    pub anchor: [f64; 3],
}

/// Why a search proposes nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SearchError {
    /// The caller's flag stopped it.
    Stopped,
    /// The model could not be placed, or a candidate could not be sliced: the cause.
    Failed(String),
}

impl fmt::Display for SearchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Stopped => f.write_str("the search was stopped"),
            Self::Failed(cause) => f.write_str(cause),
        }
    }
}

impl std::error::Error for SearchError {}

/// Where a search tells what it does.
pub trait Report {
    /// It begins `stage`.
    fn stage(&mut self, stage: Stage);
    /// The slice of the candidate being checked has done `share` (0 to 1) of its layers.
    fn progress(&mut self, share: f32);
}

/// Turns a slice's progress into a share of the work done.
struct Progress<'a> {
    report: &'a mut dyn Report,
    total: usize,
}

impl Observer for Progress<'_> {
    fn phase(&mut self, _: &str) {}

    fn begin_layers(&mut self, total: usize) {
        self.total = total;
    }

    fn layers(&mut self, completed: usize) {
        #[allow(clippy::cast_precision_loss)] // Layer counts.
        let share = completed as f32 / self.total.max(1) as f32;
        self.report.progress(share);
    }
}

fn product(a: [[f64; 3]; 3], b: [[f64; 3]; 3]) -> [[f64; 3]; 3] {
    std::array::from_fn(|r| std::array::from_fn(|c| (0..3).map(|k| a[r][k] * b[k][c]).sum()))
}

fn apply_rows(m: [[f64; 3]; 3], v: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|r| (0..3).map(|c| m[r][c] * v[c]).sum())
}

/// Where `point` (world mm, in the pose now) goes when the model turns by `tilt` about `anchor`
/// and moves `dz` up.
#[must_use]
pub fn turned(tilt: &Tilt, anchor: [f64; 3], dz: f64, point: [f64; 3]) -> [f64; 3] {
    let p = apply_rows(tilt.matrix(), std::array::from_fn(|k| point[k] - anchor[k]));
    [p[0] + anchor[0], p[1] + anchor[1], p[2] + anchor[2] + dz]
}

/// The world map of the turn, as rows of a 4 × 4 matrix: about `anchor` by `tilt`, then `dz`
/// up.
#[must_use]
pub fn turn_rows(tilt: &Tilt, anchor: [f64; 3], dz: f64) -> [[f64; 4]; 4] {
    let m = tilt.matrix();
    let moved = apply_rows(m, anchor);
    std::array::from_fn(|r| {
        if r == 3 {
            [0.0, 0.0, 0.0, 1.0]
        } else {
            let lift = if r == 2 { dz } else { 0.0 };
            [m[r][0], m[r][1], m[r][2], anchor[r] - moved[r] + lift]
        }
    })
}

/// `mesh` placed by `placement`, in world mm.
fn placed(mesh: &TriangleMesh, placement: &ObjectTransform) -> Option<TriangleMesh> {
    let vertices: Option<Vec<Point3>> = mesh
        .vertices()
        .iter()
        .map(|&v| placement.transform_point(v).ok())
        .collect();
    let mut triangles = mesh.triangles().to_vec();
    // A mirroring placement turns the winding: turn it back, so the faces stay outward.
    let s = placement.scale();
    if s.x * s.y * s.z < 0.0 {
        for t in &mut triangles {
            t.swap(1, 2);
        }
    }
    TriangleMesh::try_new(vertices?, triangles).ok()
}

/// What a search needs: the model in its pose now, the request that slices it alone, and the
/// values.
pub struct Inputs {
    /// The object's place in [`Self::shell`]'s preparation.
    pub index: usize,
    /// The model as imported, in its own frame.
    pub source: Arc<TriangleMesh>,
    /// Where the model is now: the source placed by it is searched when it is not hollowed.
    pub placement: ObjectTransform,
    /// The preparation holding the object: its hollowed shell, when it has one, is searched in
    /// place of the model; `None` for a model that is not hollowed.
    pub shell: Option<Arc<Preparation>>,
    /// The protected points, in the world, in the pose now.
    pub protected: Vec<Protected>,
    /// The request that slices the model alone: a plate of that one object, without its
    /// supports, finding its unsupported regions (see [`crate::support_plan::generate::alone`]).
    /// Each candidate is sliced by it, turned.
    pub request: SliceRequest,
    /// The file each candidate's slice is written to, and removed from.
    pub output: PathBuf,
    /// The constraints and the weights.
    pub values: OrientValues,
    /// The critical angle, in degrees from the plate.
    pub angle_deg: f64,
    /// The model's centre now, in world mm: the turn's pivot.
    pub anchor: [f64; 3],
    /// The model's lowest world Z now, where it rests.
    pub min_z: f64,
    /// The object's rotation now, in degrees (fixed X, then Y, then Z).
    pub rotate_deg: [f64; 3],
    /// Whether the object is dropped: one that is not moves up by the candidate's `dz` to rest
    /// where it rested.
    pub drop: bool,
}

/// The suction cups `cups` holds as the Drain holes tool shows them by default: at least
/// [`DEFAULT_SHOW_FROM_MM2`](crate::pipeline::DEFAULT_SHOW_FROM_MM2) wide and
/// [`DEFAULT_SHOW_TALLER_MM`](crate::pipeline::DEFAULT_SHOW_TALLER_MM) tall.
fn shown_cups(cups: &SuctionCups) -> usize {
    let shown_from = crate::pipeline::DEFAULT_SHOW_FROM_MM2;
    let taller = crate::pipeline::DEFAULT_SHOW_TALLER_MM;
    cups.report
        .cups
        .iter()
        .filter(|cup| {
            let bottom = cup
                .first_layer
                .checked_sub(1)
                .and_then(|l| cups.height(l))
                .unwrap_or(0.0);
            let top = cups.height(cup.last_layer).unwrap_or(bottom);
            cups.largest_area_mm2(cup) >= shown_from && top - bottom >= taller
        })
        .count()
}

/// Check the tilt of `r` as a slice would (see the module's documentation): the model alone
/// sliced in that pose for its islands and, `hollowed`, its suction cups; its largest layer from
/// `mesh`'s sections.
fn check(
    inputs: &Inputs,
    mesh: &TriangleMesh,
    r: &karasu_core::analysis::orientation::Rough,
    hollowed: bool,
    report: &mut dyn Report,
    stop: &AtomicBool,
) -> Result<Candidate, SearchError> {
    let halt = |_: Checkpoint| stop.load(Ordering::Relaxed);
    let values = inputs.values;
    let tilt = r.tilt;
    let up = tilt.down.map(|c| -c);
    let anchor = inputs.anchor;
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let dz = inputs.min_z - (r.low_mm - dot(anchor, up) + anchor[2]);
    let rotate_deg = matrix_euler(product(tilt.matrix(), euler_matrix(inputs.rotate_deg)));
    let mut request = inputs.request.clone();
    request.output.clone_from(&inputs.output);
    if let Subject::Plate(plate) = &mut request.subject {
        if let Some(object) = plate.objects.first_mut() {
            object.pose.rotate_deg = rotate_deg;
            if !inputs.drop {
                object.translate_mm[2] += dz;
            }
        }
    }
    if hollowed {
        request.find_suction_cups = Some(crate::pipeline::DEFAULT_MIN_VENT_MM2);
    }
    let mut observer = Progress { report, total: 0 };
    let outcome = slice_with(&request, None, &mut observer, &halt);
    let _ = std::fs::remove_file(&inputs.output);
    let outcome = outcome.map_err(|e| SearchError::Failed(e.to_string()))?;
    let protected: Vec<[f64; 3]> = inputs
        .protected
        .iter()
        .map(|p| turned(&tilt, anchor, dz, p.point))
        .collect();
    let near = |p: [f64; 3]| {
        protected.iter().any(|q| {
            (0..3).map(|c| (p[c] - q[c]).powi(2)).sum::<f64>().sqrt() <= values.keep_away_mm
        })
    };
    let (mut islands, mut protected_islands) = (0, 0);
    if let Some(found) = &outcome.unsupported {
        for region in found
            .report
            .regions
            .iter()
            .filter(|r| r.kind == RegionKind::Island)
        {
            for start in &region.starts {
                islands += 1;
                if near(found.start_world(start)) {
                    protected_islands += 1;
                }
            }
        }
    }
    let cups = outcome.suction_cups.as_deref().map(shown_cups);
    let mut predicate = |c: Checkpoint| halt(c);
    let largest_mm2 = largest_section_mm2(
        mesh,
        tilt,
        SECTION_STEP_MM,
        &mut Cancellation::new(&mut predicate),
    )
    .map_err(|_| SearchError::Stopped)?;
    Ok(Candidate {
        tilt,
        rotate_deg,
        dz,
        facing_mm2: r.facing_mm2,
        islands,
        cups: hollowed.then_some(cups.unwrap_or(0)),
        height_mm: r.height_mm,
        largest_mm2,
        protected_mm2: r.protected_mm2,
        protected_islands,
    })
}

/// Run a search (see the module's documentation), telling `report` its stages and stopping
/// when `stop` is set.
///
/// # Errors
/// [`SearchError::Stopped`] when stopped; [`SearchError::Failed`] when the model cannot be
/// placed or a candidate cannot be sliced.
pub fn search(
    inputs: &Inputs,
    report: &mut dyn Report,
    stop: &AtomicBool,
) -> Result<Found, SearchError> {
    report.stage(Stage::Rough);
    let halt = |_: Checkpoint| stop.load(Ordering::Relaxed);
    let mut predicate = |c: Checkpoint| halt(c);
    let mut cancel = Cancellation::new(&mut predicate);
    let shell = inputs
        .shell
        .as_ref()
        .and_then(|s| s.objects.get(inputs.index))
        .and_then(|o| o.as_ref().ok())
        .and_then(|o| o.hollowed());
    let own;
    let mesh = if let Some(model) = shell {
        model.mesh()
    } else {
        own = placed(&inputs.source, &inputs.placement)
            .ok_or_else(|| SearchError::Failed("the model cannot be placed".to_owned()))?;
        &own
    };
    let values = inputs.values;
    let found = rough(
        &RoughRequest {
            mesh,
            angle_deg: inputs.angle_deg,
            axes: values.axes.core(),
            max_tilt_deg: values.limited.then_some(values.limit_deg),
            protected: &inputs.protected,
            keep_away_mm: values.keep_away_mm,
        },
        &mut cancel,
    )
    .map_err(|_| SearchError::Stopped)?;
    if found.is_empty() {
        return Ok(Found {
            candidates: Vec::new(),
            scored: 0,
            anchor: inputs.anchor,
        });
    }
    let weights = values.weights.core();
    let rough_measures: Vec<Measures> = found.iter().map(Measures::of).collect();
    let tilts: Vec<Tilt> = found.iter().map(|r| r.tilt).collect();
    let best = distinct(&tilts, &rank(&rough_measures, &weights), CHECKED, APART_DEG);
    let hollowed = shell.is_some();
    let mut candidates = Vec::with_capacity(best.len());
    for (k, &i) in best.iter().enumerate() {
        report.stage(Stage::Checking(k + 1, best.len()));
        candidates.push(check(inputs, mesh, &found[i], hollowed, report, stop)?);
    }
    #[allow(clippy::cast_precision_loss)] // Counts.
    let measures: Vec<Measures> = candidates
        .iter()
        .map(|c| Measures {
            facing_mm2: c.facing_mm2,
            islands: c.islands as f64,
            cups: c.cups.unwrap_or(0) as f64,
            height_mm: c.height_mm,
            largest_mm2: c.largest_mm2,
            protected_mm2: c.protected_mm2 + LOWEST_POINT_MM2 * c.protected_islands as f64,
        })
        .collect();
    let candidates = rank(&measures, &weights)
        .into_iter()
        .take(SHOWN)
        .map(|i| candidates[i])
        .collect();
    Ok(Found {
        candidates,
        scored: found.len(),
        anchor: inputs.anchor,
    })
}
