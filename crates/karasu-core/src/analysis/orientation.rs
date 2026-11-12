//! Tilts of a placed model for printing it, scored for its supports (TD-119, M6.4k).
//!
//! **A tilt.** What faces the plate decides where supports go: a direction of the model, two
//! degrees of freedom. A turn about the vertical axis changes neither, so a candidate is a
//! [`Tilt`]: the turn about a horizontal axis that takes one direction of the model as placed
//! now, [`Tilt::down`], to straight down. The model keeps its turn about the vertical axis.
//!
//! **Which tilts.** [`tilts`] samples the directions the constraints allow, evenly: about
//! [`ANY_STEP_DEG`] apart on the cap within the most tilt for any axis, [`AXIS_STEP_DEG`] apart
//! along the arc for the plate's X or Y alone. The pose now is always the first.
//!
//! **Rough measures** ([`rough`]), from the mesh alone, on every core:
//! - the area facing the plate past the critical angle, faces resting on the plate left out;
//! - the lowest points: vertices lower than every neighbour, above the plate, where the faces
//!   around them face down (a cavity's floor is no island). Each starts an island;
//! - the height;
//! - near protected points (within the keep-away distance), the area and lowest points again.
//!
//! A tilt is refused when a protected point's surface would face the plate past the critical
//! angle.
//!
//! **Sections** ([`largest_section_mm2`]): the largest area of the mesh's sections across the
//! height, every so often, from each crossing triangle's segment oriented by its outward normal:
//! the segments of a closed surface close into loops, so their shoelace sums give the material's
//! area, a cavity's loops subtracting.
//!
//! **Ranking** ([`rank`]): each measure divided by its largest among the candidates, weighed,
//! summed; the lowest first, ties by order.
//!
//! **Determinism.** Directions come from fixed sequences; measures are sums in a fixed order per
//! tilt, so equal inputs give equal results whatever the cores.
#![allow(clippy::many_single_char_names)] // Geometric notation: points, axes, heights.
use crate::{
    cancellation::{Cancellation, Cancelled, Checkpoint},
    mesh::TriangleMesh,
};

type V = [f64; 3];

/// Directions sampled for a tilt about any axis are about this far apart, in degrees.
pub const ANY_STEP_DEG: f64 = 5.0;
/// Directions sampled for a tilt about one plate axis are this far apart, in degrees.
pub const AXIS_STEP_DEG: f64 = 1.0;
/// At or under this height above the lowest point a surface rests on the plate, in mm.
pub const ON_PLATE_MM: f64 = 1.0e-2;
/// A lowest point near a protected point counts as this much protected area, in mm²: a support
/// stands there.
pub const LOWEST_POINT_MM2: f64 = 10.0;
/// The most directions [`tilts`] returns.
pub const MAX_TILTS: usize = 4096;

/// The plate axes a tilt may turn about.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TiltAxes {
    /// Any horizontal axis.
    #[default]
    Any,
    /// The plate's X axis only: forward and back.
    X,
    /// The plate's Y axis only: side to side.
    Y,
}

/// A turn about a horizontal axis taking [`Tilt::down`], a direction of the model as placed now,
/// to straight down.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tilt {
    /// The unit direction, in the pose now, that faces the plate after the tilt.
    pub down: V,
    /// The unit horizontal axis it turns about.
    pub axis: V,
}

impl Tilt {
    /// No tilt: the pose now.
    pub const NONE: Self = Self {
        down: [0.0, 0.0, -1.0],
        axis: [1.0, 0.0, 0.0],
    };

    /// The tilt taking `down` (normalised) to straight down; `flip` is the axis of the half
    /// turn when `down` points straight up. `None` for a zero or non-finite direction.
    #[must_use]
    pub fn towards(down: V, flip: V) -> Option<Self> {
        let down = unit(down)?;
        // down × (−Z): horizontal, zero when `down` is vertical.
        let axis = unit([-down[1], down[0], 0.0]).unwrap_or(if down[2] < 0.0 {
            Self::NONE.axis
        } else {
            flip
        });
        Some(Self { down, axis })
    }

    /// How far it tilts, in degrees.
    #[must_use]
    pub fn angle_deg(&self) -> f64 {
        (-self.down[2]).clamp(-1.0, 1.0).acos().to_degrees()
    }

    /// Its rotation (rows): `matrix · down` is straight down.
    #[must_use]
    pub fn matrix(&self) -> [[f64; 3]; 3] {
        let [x, y, z] = self.axis;
        let angle = self.angle_deg().to_radians();
        let (sine, cosine) = angle.sin_cos();
        let t = 1.0 - cosine;
        [
            [
                cosine + x * x * t,
                x * y * t - z * sine,
                x * z * t + y * sine,
            ],
            [
                y * x * t + z * sine,
                cosine + y * y * t,
                y * z * t - x * sine,
            ],
            [
                z * x * t - y * sine,
                z * y * t + x * sine,
                cosine + z * z * t,
            ],
        ]
    }

    /// The angle between this tilt's down and `other`'s, in degrees.
    #[must_use]
    pub fn apart_deg(&self, other: &Self) -> f64 {
        dot(self.down, other.down)
            .clamp(-1.0, 1.0)
            .acos()
            .to_degrees()
    }
}

/// The directions the constraints allow (see the module's documentation): tilts about `axes`,
/// at most `max_deg` from the pose now when given. The pose now comes first.
#[must_use]
pub fn tilts(axes: TiltAxes, max_deg: Option<f64>) -> Vec<Tilt> {
    let most = max_deg.map_or(180.0, |m| m.clamp(0.0, 180.0));
    let mut list = vec![Tilt::NONE];
    match axes {
        TiltAxes::Any => {
            let cap = most.to_radians();
            let step = ANY_STEP_DEG.to_radians();
            // Evenly over the cap: its solid angle by the area each direction takes.
            let area = std::f64::consts::TAU * (1.0 - cap.cos());
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // Clamped.
            let count = ((area / (step * step)).ceil() as usize).clamp(0, MAX_TILTS - 1);
            let golden = std::f64::consts::PI * (3.0 - 5f64.sqrt());
            for i in 0..count {
                #[allow(clippy::cast_precision_loss)] // Below MAX_TILTS.
                let (i, n) = (i as f64, count as f64);
                let polar = (1.0 - (i + 0.5) / n * (1.0 - cap.cos()))
                    .clamp(-1.0, 1.0)
                    .acos();
                let azimuth = i * golden;
                let down = [
                    polar.sin() * azimuth.cos(),
                    polar.sin() * azimuth.sin(),
                    -polar.cos(),
                ];
                if let Some(tilt) = Tilt::towards(down, [1.0, 0.0, 0.0]) {
                    list.push(tilt);
                }
            }
        }
        TiltAxes::X | TiltAxes::Y => {
            #[allow(clippy::cast_possible_truncation)] // At most 180 steps.
            let steps = (most / AXIS_STEP_DEG).floor() as i32;
            let flip = if axes == TiltAxes::X {
                [1.0, 0.0, 0.0]
            } else {
                [0.0, 1.0, 0.0]
            };
            for k in (-steps..=steps).filter(|&k| k != 0) {
                let angle = (f64::from(k) * AXIS_STEP_DEG).to_radians();
                // A full turn reaches straight up from both sides: once is enough.
                if k == -steps && most >= 180.0 {
                    continue;
                }
                let down = if axes == TiltAxes::X {
                    [0.0, angle.sin(), -angle.cos()]
                } else {
                    [angle.sin(), 0.0, -angle.cos()]
                };
                if let Some(tilt) = Tilt::towards(down, flip) {
                    list.push(tilt);
                }
            }
        }
    }
    list
}

/// A protected point, in the pose now: its place and its surface's outward unit normal.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Protected {
    /// Its place, in mm.
    pub point: V,
    /// Its surface's outward normal.
    pub normal: V,
}

/// What the rough measures are taken of.
#[derive(Clone, Copy, Debug)]
pub struct RoughRequest<'a> {
    /// The model as placed now: closed, oriented outward (the hollowed shell when hollowed).
    pub mesh: &'a TriangleMesh,
    /// The critical angle, in degrees from the plate's plane.
    pub angle_deg: f64,
    /// The plate axes tilts may turn about.
    pub axes: TiltAxes,
    /// The most tilt from the pose now, in degrees.
    pub max_tilt_deg: Option<f64>,
    /// The protected points.
    pub protected: &'a [Protected],
    /// How far round a protected point its surface is kept clean, in mm.
    pub keep_away_mm: f64,
}

/// A tilt's rough measures.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rough {
    /// The tilt.
    pub tilt: Tilt,
    /// Area facing the plate past the critical angle, in mm².
    pub facing_mm2: f64,
    /// Lowest points above the plate, each starting an island.
    pub lowest_points: usize,
    /// The model's height after the tilt, in mm.
    pub height_mm: f64,
    /// The lowest height along the tilt's up, in the pose now's coordinates (`-down · v` least
    /// over the vertices), in mm: where the tilted model rests.
    pub low_mm: f64,
    /// Area facing the plate near protected points, in mm².
    pub protected_mm2: f64,
    /// Lowest points near protected points.
    pub protected_points: usize,
}

/// The mesh's data every tilt reads, computed once.
struct Shape {
    vertices: Vec<V>,
    triangles: Vec<[u32; 3]>,
    /// Each triangle's normal by twice its area.
    normals: Vec<V>,
    /// Each vertex's neighbours along edges: `items[starts[v]..starts[v + 1]]`.
    starts: Vec<u32>,
    items: Vec<u32>,
    /// Each vertex's incident triangles' normals summed.
    vertex_normals: Vec<V>,
    near_triangle: Vec<bool>,
    near_vertex: Vec<bool>,
}

fn dot(a: V, b: V) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn sub(a: V, b: V) -> V {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: V, b: V) -> V {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn length(a: V) -> f64 {
    dot(a, a).sqrt()
}

fn unit(a: V) -> Option<V> {
    let l = length(a);
    (l > 0.0 && l.is_finite()).then(|| a.map(|c| c / l))
}

impl Shape {
    fn new(
        mesh: &TriangleMesh,
        protected: &[Protected],
        keep_away_mm: f64,
        cancel: &mut Cancellation<'_>,
    ) -> Result<Self, Cancelled> {
        let vertices: Vec<V> = mesh.vertices().iter().map(|p| [p.x, p.y, p.z]).collect();
        let triangles = mesh.triangles().to_vec();
        let mut normals = Vec::with_capacity(triangles.len());
        let mut vertex_normals = vec![[0.0; 3]; vertices.len()];
        let mut degree = vec![0u32; vertices.len() + 1];
        for (i, t) in triangles.iter().enumerate() {
            cancel.poll(Checkpoint::OrientationWork, i)?;
            let [a, b, c] = t.map(|v| vertices[v as usize]);
            let n = cross(sub(b, a), sub(c, a));
            normals.push(n);
            for &v in t {
                let sum = &mut vertex_normals[v as usize];
                *sum = [sum[0] + n[0], sum[1] + n[1], sum[2] + n[2]];
                degree[v as usize + 1] += 2;
            }
        }
        // Neighbours along edges, each edge seen from both of its triangles: duplicates are
        // harmless to "every neighbour is higher".
        for v in 0..vertices.len() {
            degree[v + 1] += degree[v];
        }
        let mut fill = degree.clone();
        let mut items = vec![0u32; degree[vertices.len()] as usize];
        for t in &triangles {
            for k in 0..3 {
                let (v, a, b) = (t[k], t[(k + 1) % 3], t[(k + 2) % 3]);
                let at = &mut fill[v as usize];
                items[*at as usize] = a;
                items[*at as usize + 1] = b;
                *at += 2;
            }
        }
        let near = |p: V| {
            protected
                .iter()
                .any(|q| length(sub(p, q.point)) <= keep_away_mm)
        };
        let near_vertex: Vec<bool> = vertices.iter().map(|&p| near(p)).collect();
        let near_triangle = triangles
            .iter()
            .map(|t| {
                let [a, b, c] = t.map(|v| vertices[v as usize]);
                near([0, 1, 2].map(|k| (a[k] + b[k] + c[k]) / 3.0))
            })
            .collect();
        Ok(Self {
            vertices,
            triangles,
            normals,
            starts: degree,
            items,
            vertex_normals,
            near_triangle,
            near_vertex,
        })
    }

    /// `tilt`'s measures, with `cosine` the critical angle's cosine.
    fn measure(&self, tilt: Tilt, cosine: f64) -> Rough {
        let up = tilt.down.map(|c| -c);
        let heights: Vec<f64> = self.vertices.iter().map(|&v| dot(v, up)).collect();
        let low = heights.iter().copied().fold(f64::INFINITY, f64::min);
        let high = heights.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let on_plate = low + ON_PLATE_MM;
        let (mut facing, mut protected_mm2) = (0.0, 0.0);
        for (i, (t, n)) in self.triangles.iter().zip(&self.normals).enumerate() {
            let twice = length(*n);
            if dot(*n, tilt.down) <= cosine * twice
                || t.iter().all(|&v| heights[v as usize] <= on_plate)
            {
                continue;
            }
            facing += twice / 2.0;
            if self.near_triangle[i] {
                protected_mm2 += twice / 2.0;
            }
        }
        let (mut lowest, mut protected_points) = (0, 0);
        for (v, &h) in heights.iter().enumerate() {
            if h <= on_plate || dot(self.vertex_normals[v], tilt.down) <= 0.0 {
                continue;
            }
            let around = &self.items[self.starts[v] as usize..self.starts[v + 1] as usize];
            if !around.is_empty() && around.iter().all(|&u| heights[u as usize] > h) {
                lowest += 1;
                if self.near_vertex[v] {
                    protected_points += 1;
                }
            }
        }
        Rough {
            tilt,
            facing_mm2: facing,
            lowest_points: lowest,
            height_mm: high - low,
            low_mm: low,
            protected_mm2,
            protected_points,
        }
    }
}

/// The rough measures of every tilt the request's constraints allow, in [`tilts`]' order; tilts
/// that would turn a protected point's surface to face the plate are left out.
///
/// # Errors
/// Cancellation, checked between groups of tilts.
pub fn rough(
    request: &RoughRequest<'_>,
    cancel: &mut Cancellation<'_>,
) -> Result<Vec<Rough>, Cancelled> {
    cancel.check(Checkpoint::OrientationWork)?;
    let cosine = request.angle_deg.to_radians().cos();
    let allowed: Vec<Tilt> = tilts(request.axes, request.max_tilt_deg)
        .into_iter()
        .filter(|t| {
            request
                .protected
                .iter()
                .all(|p| dot(p.normal, t.down) <= cosine)
        })
        .collect();
    let shape = Shape::new(
        request.mesh,
        request.protected,
        request.keep_away_mm,
        cancel,
    )?;
    let cores = std::thread::available_parallelism().map_or(1, usize::from);
    let mut measured = Vec::with_capacity(allowed.len());
    for group in allowed.chunks(cores * 4) {
        cancel.check(Checkpoint::OrientationWork)?;
        let mut out = vec![None; group.len()];
        crate::parallel::fill_costly_with(&mut out, |i| Some(shape.measure(group[i], cosine)));
        measured.extend(out.into_iter().flatten());
    }
    Ok(measured)
}

/// The largest area of `mesh`'s sections after `tilt`, taken every `step_mm` across its height
/// (see the module's documentation), in mm².
///
/// # Errors
/// Cancellation.
pub fn largest_section_mm2(
    mesh: &TriangleMesh,
    tilt: Tilt,
    step_mm: f64,
    cancel: &mut Cancellation<'_>,
) -> Result<f64, Cancelled> {
    Ok(section_areas_mm2(mesh, tilt, step_mm, cancel)?
        .into_iter()
        .fold(0.0, f64::max))
}

/// The areas of `mesh`'s sections after `tilt`, on planes `step_mm` apart from half a step
/// above its lowest point, lowest first, in mm².
///
/// # Errors
/// Cancellation.
pub fn section_areas_mm2(
    mesh: &TriangleMesh,
    tilt: Tilt,
    step_mm: f64,
    cancel: &mut Cancellation<'_>,
) -> Result<Vec<f64>, Cancelled> {
    cancel.check(Checkpoint::OrientationWork)?;
    let up = tilt.down.map(|c| -c);
    let vertices: Vec<V> = mesh.vertices().iter().map(|p| [p.x, p.y, p.z]).collect();
    let heights: Vec<f64> = vertices.iter().map(|&v| dot(v, up)).collect();
    let low = heights.iter().copied().fold(f64::INFINITY, f64::min);
    let high = heights.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    if high.is_nan() || high <= low || step_mm.is_nan() || step_mm <= 0.0 {
        return Ok(Vec::new());
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // A few thousand.
    let planes = (((high - low) / step_mm).ceil() as usize).max(1);
    // Coordinates across the planes: any right-handed pair with `up`.
    let u = unit(cross(
        up,
        if up[0].abs() < 0.9 {
            [1.0, 0.0, 0.0]
        } else {
            [0.0, 1.0, 0.0]
        },
    ))
    .unwrap_or([1.0, 0.0, 0.0]);
    let w = cross(up, u);
    let triangles = mesh.triangles();
    let cores = std::thread::available_parallelism().map_or(1, usize::from);
    let width = triangles.len().div_ceil(cores).max(1);
    let mut partial = vec![Vec::new(); triangles.len().div_ceil(width)];
    crate::parallel::fill_costly_with(&mut partial, |chunk| {
        let mut areas = vec![0.0; planes];
        for t in &triangles[chunk * width..((chunk + 1) * width).min(triangles.len())] {
            let corners = t.map(|v| vertices[v as usize]);
            let h = t.map(|v| heights[v as usize]);
            let normal = cross(sub(corners[1], corners[0]), sub(corners[2], corners[0]));
            let along = cross(up, normal);
            let (lo, hi) = (h[0].min(h[1]).min(h[2]), h[0].max(h[1]).max(h[2]));
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // Within planes.
            let first = (((lo - low) / step_mm - 0.5).ceil().max(0.0)) as usize;
            for (k, area) in areas.iter_mut().enumerate().skip(first) {
                #[allow(clippy::cast_precision_loss)] // A few thousand.
                let z = low + (k as f64 + 0.5) * step_mm;
                if z >= hi {
                    break;
                }
                if z <= lo {
                    continue;
                }
                // The two edges whose ends lie on either side of the plane.
                let mut ends = [[0.0; 3]; 2];
                let mut found = 0;
                for e in 0..3 {
                    let (a, b) = (e, (e + 1) % 3);
                    if (h[a] > z) != (h[b] > z) && found < 2 {
                        let s = (z - h[a]) / (h[b] - h[a]);
                        ends[found] =
                            [0, 1, 2].map(|c| corners[a][c] + s * (corners[b][c] - corners[a][c]));
                        found += 1;
                    }
                }
                if found < 2 {
                    continue;
                }
                let [mut p, mut q] = ends;
                // Material on the left, seen from above: the outward normal on the right.
                if dot(sub(q, p), along) < 0.0 {
                    std::mem::swap(&mut p, &mut q);
                }
                let (pu, pw, qu, qw) = (dot(p, u), dot(p, w), dot(q, u), dot(q, w));
                *area += (pu * qw - qu * pw) / 2.0;
            }
        }
        areas
    });
    cancel.check(Checkpoint::OrientationWork)?;
    let mut total = vec![0.0; planes];
    for areas in &partial {
        for (sum, a) in total.iter_mut().zip(areas) {
            *sum += a;
        }
    }
    Ok(total)
}

/// What a candidate measures, for [`rank`]; a measure not taken is zero for every candidate.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Measures {
    /// Area facing the plate past the critical angle, in mm².
    pub facing_mm2: f64,
    /// Islands (or lowest points, roughly).
    pub islands: f64,
    /// Suction cups.
    pub cups: f64,
    /// Height, in mm.
    pub height_mm: f64,
    /// The largest layer's area, in mm².
    pub largest_mm2: f64,
    /// Area facing the plate near protected points, with [`LOWEST_POINT_MM2`] for each lowest
    /// point or island there, in mm².
    pub protected_mm2: f64,
}

impl Measures {
    /// A tilt's rough measures.
    #[must_use]
    pub fn of(rough: &Rough) -> Self {
        #[allow(clippy::cast_precision_loss)] // Counts.
        Self {
            facing_mm2: rough.facing_mm2,
            islands: rough.lowest_points as f64,
            cups: 0.0,
            height_mm: rough.height_mm,
            largest_mm2: 0.0,
            protected_mm2: rough.protected_mm2 + LOWEST_POINT_MM2 * rough.protected_points as f64,
        }
    }

    fn values(&self) -> [f64; 6] {
        [
            self.facing_mm2,
            self.islands,
            self.cups,
            self.height_mm,
            self.largest_mm2,
            self.protected_mm2,
        ]
    }
}

/// How much each measure counts in the score.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Weights {
    /// Area facing the plate.
    pub facing: f64,
    /// Islands.
    pub islands: f64,
    /// Suction cups.
    pub cups: f64,
    /// Height.
    pub height: f64,
    /// The largest layer.
    pub largest: f64,
    /// Protected surfaces.
    pub protected: f64,
}

impl Weights {
    fn values(&self) -> [f64; 6] {
        [
            self.facing,
            self.islands,
            self.cups,
            self.height,
            self.largest,
            self.protected,
        ]
    }
}

/// Each candidate's score: its measures, each divided by the largest among `candidates`,
/// weighed and summed. Lower is better.
#[must_use]
pub fn scores(candidates: &[Measures], weights: &Weights) -> Vec<f64> {
    let mut most = [0.0f64; 6];
    for m in candidates {
        for (top, v) in most.iter_mut().zip(m.values()) {
            *top = top.max(v);
        }
    }
    candidates
        .iter()
        .map(|m| {
            m.values()
                .iter()
                .zip(most)
                .zip(weights.values())
                .map(|((v, top), w)| if top > 0.0 { w * v / top } else { 0.0 })
                .sum()
        })
        .collect()
}

/// The candidates' places, best (lowest score) first, ties by place.
#[must_use]
pub fn rank(candidates: &[Measures], weights: &Weights) -> Vec<usize> {
    let scores = scores(candidates, weights);
    let mut order: Vec<usize> = (0..candidates.len()).collect();
    order.sort_by(|&a, &b| scores[a].total_cmp(&scores[b]).then(a.cmp(&b)));
    order
}

/// From `order` (places in `tilts`, best first), at most `count` whose tilts lie at least
/// `apart_deg` from every one taken before.
#[must_use]
pub fn distinct(tilts: &[Tilt], order: &[usize], count: usize, apart_deg: f64) -> Vec<usize> {
    let mut taken: Vec<usize> = Vec::new();
    for &i in order {
        if taken.len() == count {
            break;
        }
        if taken
            .iter()
            .all(|&j| tilts[i].apart_deg(&tilts[j]) >= apart_deg)
        {
            taken.push(i);
        }
    }
    taken
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Point3;

    /// A closed box from `low` to `high`, outward.
    fn cube(low: V, high: V) -> TriangleMesh {
        let [x0, y0, z0] = low;
        let [x1, y1, z1] = high;
        let v = [
            [x0, y0, z0],
            [x1, y0, z0],
            [x1, y1, z0],
            [x0, y1, z0],
            [x0, y0, z1],
            [x1, y0, z1],
            [x1, y1, z1],
            [x0, y1, z1],
        ];
        let t = [
            [0, 2, 1],
            [0, 3, 2],
            [4, 5, 6],
            [4, 6, 7],
            [0, 1, 5],
            [0, 5, 4],
            [1, 2, 6],
            [1, 6, 5],
            [2, 3, 7],
            [2, 7, 6],
            [3, 0, 4],
            [3, 4, 7],
        ];
        TriangleMesh::try_new(
            v.iter().map(|p| Point3::new(p[0], p[1], p[2])).collect(),
            t.to_vec(),
        )
        .unwrap()
    }

    fn request<'a>(mesh: &'a TriangleMesh, protected: &'a [Protected]) -> RoughRequest<'a> {
        RoughRequest {
            mesh,
            angle_deg: 45.0,
            axes: TiltAxes::Any,
            max_tilt_deg: None,
            protected,
            keep_away_mm: 3.0,
        }
    }

    #[test]
    fn a_tilt_turns_its_direction_straight_down_about_a_horizontal_axis() {
        for down in [
            [0.0, 0.6, -0.8],
            [0.3, -0.2, 0.9],
            [-1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
        ] {
            let tilt = Tilt::towards(down, [0.0, 1.0, 0.0]).unwrap();
            let m = tilt.matrix();
            let turned: V = std::array::from_fn(|r| dot(m[r], tilt.down));
            assert!(length(sub(turned, [0.0, 0.0, -1.0])) < 1e-12, "{down:?}");
            assert!(tilt.axis[2].abs() < 1e-12);
        }
        assert!((Tilt::NONE.angle_deg()).abs() < 1e-12);
        assert_eq!(
            Tilt::towards([0.0, 0.0, 2.0], [0.0, 1.0, 0.0])
                .unwrap()
                .axis,
            [0.0, 1.0, 0.0]
        );
        assert!(Tilt::towards([0.0; 3], [1.0, 0.0, 0.0]).is_none());
    }

    #[test]
    fn tilts_keep_to_their_axis_and_limit_and_start_with_the_pose_now() {
        for (axes, most) in [
            (TiltAxes::Any, Some(30.0)),
            (TiltAxes::Any, None),
            (TiltAxes::X, Some(45.0)),
            (TiltAxes::Y, None),
        ] {
            let list = tilts(axes, most);
            assert_eq!(list[0], Tilt::NONE);
            for t in &list {
                assert!(
                    t.angle_deg() <= most.unwrap_or(180.0) + 1e-9,
                    "{axes:?} {t:?}"
                );
                match axes {
                    TiltAxes::X => assert!(t.down[0].abs() < 1e-12),
                    TiltAxes::Y => assert!(t.down[1].abs() < 1e-12),
                    TiltAxes::Any => {}
                }
            }
        }
        // One axis, a degree apart: 45 each way and the pose now.
        assert_eq!(tilts(TiltAxes::X, Some(45.0)).len(), 91);
        // A full turn about one axis: every degree once.
        assert_eq!(tilts(TiltAxes::Y, None).len(), 360);
        // Any axis within 30°: a few dozen, about 5° apart.
        let near = tilts(TiltAxes::Any, Some(30.0));
        assert!((80..200).contains(&near.len()), "{}", near.len());
    }

    #[test]
    fn a_box_standing_flat_needs_nothing_and_tilted_on_an_edge_faces_the_plate() {
        let cube = cube([0.0; 3], [10.0, 10.0, 10.0]);
        let none = Shape::new(&cube, &[], 0.0, &mut Cancellation::never())
            .unwrap()
            .measure(Tilt::NONE, 45f64.to_radians().cos());
        // Its bottom rests on the plate; its corners there are on the plate too.
        assert_eq!((none.facing_mm2, none.lowest_points), (0.0, 0));
        assert!((none.height_mm - 10.0).abs() < 1e-12);
        // On an edge, 45° about X: two faces at 45°, facing the plate past a 50° angle.
        let edge = Tilt::towards([0.0, 0.5f64.sqrt(), -0.5f64.sqrt()], [1.0, 0.0, 0.0]).unwrap();
        let shape = Shape::new(&cube, &[], 0.0, &mut Cancellation::never()).unwrap();
        let tilted = shape.measure(edge, 50f64.to_radians().cos());
        assert!((tilted.facing_mm2 - 200.0).abs() < 1e-9, "{tilted:?}");
        assert!((tilted.height_mm - 10.0 * 2f64.sqrt()).abs() < 1e-9);
        // At 45° exactly they stand alone.
        assert_eq!(
            shape.measure(edge, 45f64.to_radians().cos()).facing_mm2,
            0.0
        );
        // On a corner, its lowest point rests on the plate: no island.
        let corner = Tilt::towards([-1.0, -1.0, -1.0], [1.0, 0.0, 0.0]).unwrap();
        assert_eq!(
            shape
                .measure(corner, 45f64.to_radians().cos())
                .lowest_points,
            0
        );
    }

    #[test]
    fn a_box_lifted_over_another_starts_an_island_at_its_lowest_corner() {
        // Two boxes, one floating above the other's corner: on a corner, the floating one's
        // lowest vertex starts an island; flat, its whole bottom faces the plate.
        let mut vertices = Vec::new();
        let mut triangles = Vec::new();
        for (low, high) in [
            ([0.0; 3], [10.0, 10.0, 10.0]),
            ([20.0, 0.0, 20.0], [30.0, 10.0, 30.0]),
        ] {
            let part = cube(low, high);
            let base = u32::try_from(vertices.len()).unwrap();
            vertices.extend_from_slice(part.vertices());
            triangles.extend(part.triangles().iter().map(|t| t.map(|v| v + base)));
        }
        let mesh = TriangleMesh::try_new(vertices, triangles).unwrap();
        let shape = Shape::new(&mesh, &[], 0.0, &mut Cancellation::never()).unwrap();
        let flat = shape.measure(Tilt::NONE, 45f64.to_radians().cos());
        assert!((flat.facing_mm2 - 100.0).abs() < 1e-9);
        let corner = Tilt::towards([-1.0, -1.0, -1.0], [1.0, 0.0, 0.0]).unwrap();
        assert_eq!(
            shape
                .measure(corner, 45f64.to_radians().cos())
                .lowest_points,
            1
        );
    }

    #[test]
    fn protected_points_refuse_tilts_that_turn_them_to_the_plate_and_weigh_near_them() {
        let cube = cube([0.0; 3], [10.0, 10.0, 10.0]);
        // The top's middle: kept from facing the plate.
        let top = [Protected {
            point: [5.0, 5.0, 10.0],
            normal: [0.0, 0.0, 1.0],
        }];
        let found = rough(
            &RoughRequest {
                axes: TiltAxes::X,
                ..request(&cube, &top)
            },
            &mut Cancellation::never(),
        )
        .unwrap();
        assert!(found.iter().all(|r| r.tilt.angle_deg() < 135.0 + 1e-9));
        assert!(found.iter().any(|r| r.tilt.angle_deg() > 130.0));
        // Near the bottom's middle, its area counts when it faces the plate off the plate.
        let bottom = [Protected {
            point: [5.0, 5.0, 0.0],
            normal: [0.0, 0.0, -1.0],
        }];
        let shape = Shape::new(&cube, &bottom, 3.0, &mut Cancellation::never()).unwrap();
        let lifted = shape.measure(
            Tilt::towards([0.0, 0.2f64.sin(), -0.2f64.cos()], [1.0, 0.0, 0.0]).unwrap(),
            45f64.to_radians().cos(),
        );
        assert!(
            lifted.protected_mm2 > 0.0 && lifted.protected_mm2 <= 100.0,
            "{lifted:?}"
        );
    }

    #[test]
    fn sections_measure_the_material_with_a_cavity_subtracted() {
        let solid = cube([0.0; 3], [10.0, 20.0, 5.0]);
        let flat =
            largest_section_mm2(&solid, Tilt::NONE, 0.5, &mut Cancellation::never()).unwrap();
        assert!((flat - 200.0).abs() < 1e-9, "{flat}");
        // On its long side: 10 × 5.
        let side = Tilt::towards([0.0, 1.0, 0.0], [1.0, 0.0, 0.0]).unwrap();
        let standing = largest_section_mm2(&solid, side, 0.5, &mut Cancellation::never()).unwrap();
        assert!((standing - 50.0).abs() < 1e-9, "{standing}");
        // A shell: an inward box inside takes its section away.
        let mut vertices = solid.vertices().to_vec();
        let mut triangles = solid.triangles().to_vec();
        let inner = cube([2.0, 2.0, 1.0], [8.0, 18.0, 4.0]);
        let base = u32::try_from(vertices.len()).unwrap();
        vertices.extend_from_slice(inner.vertices());
        triangles.extend(
            inner
                .triangles()
                .iter()
                .map(|t| [t[0] + base, t[2] + base, t[1] + base]),
        );
        let shell = TriangleMesh::try_new(vertices, triangles).unwrap();
        let areas = section_areas_mm2(&shell, Tilt::NONE, 0.5, &mut Cancellation::never()).unwrap();
        // Planes at 0.25, 0.75, … 4.75 mm: the floor and the roof are solid; across the cavity
        // (from 1 to 4 mm) its 6 × 16 is gone.
        assert_eq!(areas.len(), 10);
        for (k, area) in areas.iter().enumerate() {
            let z = 0.25 + 0.5 * f64::from(u32::try_from(k).unwrap());
            let expected = if (1.0..4.0).contains(&z) {
                104.0
            } else {
                200.0
            };
            assert!((area - expected).abs() < 1e-9, "{z}: {area}");
        }
    }

    #[test]
    fn ranking_divides_each_measure_by_its_largest_and_weighs_it() {
        let a = Measures {
            facing_mm2: 100.0,
            height_mm: 50.0,
            ..Measures::default()
        };
        let b = Measures {
            facing_mm2: 50.0,
            height_mm: 100.0,
            ..Measures::default()
        };
        let fewer = Weights {
            facing: 2.0,
            islands: 1.0,
            cups: 1.0,
            height: 1.0,
            largest: 1.0,
            protected: 4.0,
        };
        assert_eq!(rank(&[a, b], &fewer), [1, 0]);
        let faster = Weights {
            facing: 1.0,
            height: 2.0,
            ..fewer
        };
        assert_eq!(rank(&[a, b], &faster), [0, 1]);
        // Equal: their order.
        assert_eq!(rank(&[a, a], &fewer), [0, 1]);
        // Distinct: a tilt within the separation of one taken is skipped.
        let list = [
            Tilt::NONE,
            Tilt::towards([0.0, 0.05, -1.0], [1.0, 0.0, 0.0]).unwrap(),
            Tilt::towards([0.0, 1.0, -1.0], [1.0, 0.0, 0.0]).unwrap(),
        ];
        assert_eq!(distinct(&list, &[0, 1, 2], 3, 8.0), [0, 2]);
    }
}
