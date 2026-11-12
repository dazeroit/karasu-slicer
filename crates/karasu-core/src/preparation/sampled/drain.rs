//! Certified finite drainage cuts into sampled (possibly non-convex) cavities (TD-047).
//!
//! The cut geometry is the existing certified cardinal polygonal cylinder of M5.5-C. Instead of
//! convex supporting planes, an opening is certified by exact tests against the represented
//! surfaces:
//!
//! - every entry-cap triangle is disjoint from the placed source surface and proven farther than
//!   the required clearance, and the cap's centre is joined to the outside of the source's
//!   bounding box by a straight segment, in one of 26 directions, that meets no source triangle
//!   (exact). The cap therefore lies in the unbounded exterior rather than a sealed void. Only
//!   when every such segment is blocked does the certificate fall back to parity: the centre
//!   must lie outside every edge-connected source component individually, which is sound after
//!   an exact check that the source is embedded with pairwise-disjoint bodies;
//! - every end-cap triangle is disjoint from the cavity surface and proven farther than the
//!   clearance, and the cap lies inside the cavity (odd parity over all cavity surfaces).
//!
//! The cut is a connected prism removed from the material whose two caps lie in the exterior and
//! in one cavity region, so that region becomes geometrically connected to the exterior. No
//! convexity is needed. Regions reached only incidentally by a prism wall are not claimed drained.
//!
//! The drained solid is `shell \ (C1 ∪ … ∪ Cn)`. Exact per-layer polygon construction does not
//! scale to marching-tetrahedra shells, so the shell is sliced as one strict layer stream and the
//! cutters' horizontal sections are computed analytically ([`LayerCut`]): the certified polygon
//! for a vertical prism, a rectangle for a horizontal one. They are subtracted per raster sample
//! by [`crate::raster::stream::DifferenceRasterStream`]. A plane exactly on a cap or tangent to a
//! prism edge removes nothing (a measure-zero boundary), so no plane position can fail.

use super::{
    bvh::{BuildFailure, Bvh, CapDisk, Clearance, Parity, ParityFailure},
    contact::improper_contact,
    distance::{closest_point_on_triangle, triangle_normal, Triangle},
    edge_components, hollow_mesh_sampled_with_progress, SampledHollowLimits, SampledHollowProgress,
    SampledHollowStage, SampledHollowedModel,
};
use crate::{
    cancellation::{Cancellation, Checkpoint},
    geometry::{sin_cos_degrees, Point3},
    mesh::TriangleMesh,
    preparation::{
        drainage::{
            cylinder::{cylinder, is_cardinal, oblique_cylinder, Cutter},
            DrainageError, OpeningIssue,
        },
        hollow::numeric::I,
        HoleSpec, HoleTilt, Operation, OperationKind, PlanLimits, PreparationPlan, SurfaceHoleSpec,
    },
    raster::LayerCut,
    slicing::{
        engine::{LayerConfig, LayerEngine, LayerProgress},
        schedule::{LayerPlane, LayerSchedule},
        SectionTransform,
    },
};

/// Drainage policy for sampled cavities; hollowing keeps its own limits.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SampledDrainageLimits {
    /// Limits of the preceding sampled hollowing.
    pub hollow: SampledHollowLimits,
    /// Maximum finite cuts.
    pub max_holes: usize,
    /// Maximum sides per polygonal cylinder (hard maximum 4096).
    pub max_segments: usize,
    /// Required strict separation, in mm, of entry caps from the source surface, end caps from
    /// the cavity surface, and between cutter bounding boxes.
    pub minimum_clearance_mm: f64,
}

impl Default for SampledDrainageLimits {
    fn default() -> Self {
        Self {
            hollow: SampledHollowLimits::default(),
            max_holes: 32,
            max_segments: 4096,
            minimum_clearance_mm: 1e-6,
        }
    }
}

/// One certified opening.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SampledDrainageOpening {
    /// Stable operation ID, in plan order.
    pub operation_id: u64,
    /// Sides of the represented polygonal cut.
    pub segments: usize,
    /// Bounded radial deviation from the requested cylinder, in mm.
    pub radial_error_mm: f64,
    /// Bounded axial endpoint deviation, in mm.
    pub axial_error_mm: f64,
    /// Certified lower bound on the distance from the entry cap to the source surface, in mm.
    /// It is at least the required clearance (which the certificate proved strictly exceeded)
    /// and may be loose: it comes from bounded work (see TD-047).
    pub entry_clearance_mm: f64,
    /// Certified lower bound on the distance from the end cap to the cavity surface, in mm,
    /// with the same meaning as `entry_clearance_mm`.
    pub terminal_clearance_mm: f64,
    /// Index into [`SampledDrainedModel::cavity_regions`] of the region this cut drains.
    pub cavity_region: usize,
    /// Entry centre of the cut as built, in world mm (resolved for surface holes, TD-066).
    pub entry_mm: [f64; 3],
    /// Unit cut direction as built.
    pub direction: [f64; 3],
    /// Cut length as built, in mm.
    pub length_mm: f64,
}

/// One connected cavity region.
#[derive(Clone, Debug, PartialEq)]
pub struct CavityRegion {
    /// Enclosed volume estimate (outer surface minus directly nested islands), in mm³. Tiny
    /// isolated pockets of the reconstruction are regions too; none is filled or discarded.
    pub volume_mm3: f64,
    /// Closed surfaces bounding the region: its outer surface plus directly nested islands.
    pub surfaces: usize,
    /// Operations whose end cap lies in this region, in plan order. Empty means no certified
    /// opening: a cut that merely passes through the region is not credited to it.
    pub drained_by: Vec<u64>,
    /// A vertex of the region's outer surface, in the cavity mesh's coordinates (mm): a point
    /// that is certainly on the region, to locate it (TD-101).
    pub point_mm: [f64; 3],
    /// The exact coordinate extrema of the region's outer surface (`[min, max]`, mm), which
    /// bound its nested islands too.
    pub bounds_mm: [[f64; 3]; 2],
}

/// One certified cardinal prism: its axis, axial extent and polygon in the two radial axes
/// (`(axis + 1) % 3`, `(axis + 2) % 3`), exactly as represented in the cutter mesh.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Prism {
    axis: usize,
    low: f64,
    high: f64,
    ring: Vec<[f64; 2]>,
}

/// One certified oblique prism (TD-066): its entry and end rings exactly as represented.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct ObliquePrism {
    entry: Vec<[f64; 3]>,
    end: Vec<[f64; 3]>,
}

impl ObliquePrism {
    pub(super) fn from_cutter(cut: &Cutter) -> Self {
        Self {
            entry: cut.entry.iter().map(|p| p.coordinates()).collect(),
            end: cut.end.iter().map(|p| p.coordinates()).collect(),
        }
    }

    /// Horizontal section at `z`: the convex hull of the plane's crossings with every ring and
    /// lateral edge, which is the section of the convex prism. `None` without area; a plane
    /// through a single vertex or edge removes nothing (measure zero).
    fn section(&self, z: f64) -> Option<Vec<[f64; 2]>> {
        let n = self.entry.len();
        let mut points = Vec::new();
        let mut crossing = |a: [f64; 3], b: [f64; 3]| {
            #[allow(clippy::float_cmp)] // Exact vertex-on-plane detection.
            if a[2] == z {
                points.push([a[0], a[1]]);
            } else if (a[2] - z) * (b[2] - z) < 0.0 {
                let t = (z - a[2]) / (b[2] - a[2]);
                points.push([a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1])]);
            }
        };
        for i in 0..n {
            let j = (i + 1) % n;
            crossing(self.entry[i], self.entry[j]);
            crossing(self.end[i], self.end[j]);
            crossing(self.entry[i], self.end[i]);
            crossing(self.end[i], self.entry[i]);
        }
        convex_hull(points)
    }
}

/// Counter-clockwise convex hull (Andrew's monotone chain, collinear points dropped), or `None`
/// with fewer than three corners.
fn convex_hull(mut points: Vec<[f64; 2]>) -> Option<Vec<[f64; 2]>> {
    points.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
    points.dedup();
    if points.len() < 3 {
        return None;
    }
    let turn = |o: [f64; 2], a: [f64; 2], b: [f64; 2]| {
        (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
    };
    let mut hull: Vec<[f64; 2]> = Vec::with_capacity(points.len() * 2);
    for pass in 0..2 {
        let start = hull.len();
        let ordered: Box<dyn Iterator<Item = &[f64; 2]>> = if pass == 0 {
            Box::new(points.iter())
        } else {
            Box::new(points.iter().rev())
        };
        for &p in ordered {
            while hull.len() >= start + 2
                && turn(hull[hull.len() - 2], hull[hull.len() - 1], p) <= 0.0
            {
                hull.pop();
            }
            hull.push(p);
        }
        hull.pop();
    }
    (hull.len() >= 3).then_some(hull)
}

/// A certified cutter prism of either construction.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Cut {
    Cardinal(Prism),
    Oblique(ObliquePrism),
}

impl Cut {
    pub(super) fn section(&self, z: f64) -> Option<Vec<[f64; 2]>> {
        match self {
            Self::Cardinal(prism) => prism.section(z),
            Self::Oblique(prism) => prism.section(z),
        }
    }
}

impl Prism {
    pub(super) fn from_cutter(cut: &Cutter, axis: usize) -> Self {
        let radial = [(axis + 1) % 3, (axis + 2) % 3];
        let start = cut.entry[0].coordinates()[axis];
        let finish = cut.end[0].coordinates()[axis];
        Self {
            axis,
            low: start.min(finish),
            high: start.max(finish),
            ring: cut
                .entry
                .iter()
                .map(|p| {
                    let c = p.coordinates();
                    [c[radial[0]], c[radial[1]]]
                })
                .collect(),
        }
    }

    /// Horizontal section at `z` as one world XY ring, or `None` when it has no area.
    fn section(&self, z: f64) -> Option<Vec<[f64; 2]>> {
        match self.axis {
            2 => (self.low < z && z < self.high).then(|| self.ring.clone()),
            // Horizontal prism: Z is the second radial coordinate for X (y, z) and the first for
            // Y (z, x); the section is the axial extent times the ring's chord at that height.
            0 => {
                let (low, high) = self.chord(z, 1, 0)?;
                Some(vec![
                    [self.low, low],
                    [self.high, low],
                    [self.high, high],
                    [self.low, high],
                ])
            }
            _ => {
                let (low, high) = self.chord(z, 0, 1)?;
                Some(vec![
                    [low, self.low],
                    [high, self.low],
                    [high, self.high],
                    [low, self.high],
                ])
            }
        }
    }

    /// Chord of the convex ring along radial coordinate `along` at `height` on radial coordinate
    /// `at`; `None` if the line misses the interior.
    #[allow(clippy::float_cmp)] // Exact vertex-on-line detection.
    fn chord(&self, height: f64, at: usize, along: usize) -> Option<(f64, f64)> {
        let (mut low, mut high) = (f64::INFINITY, f64::NEG_INFINITY);
        for (k, a) in self.ring.iter().enumerate() {
            let b = self.ring[(k + 1) % self.ring.len()];
            if a[at] == height {
                low = low.min(a[along]);
                high = high.max(a[along]);
            }
            if (a[at] - height) * (b[at] - height) < 0.0 {
                let t = (height - a[at]) / (b[at] - a[at]);
                let x = a[along] + t * (b[along] - a[along]);
                low = low.min(x);
                high = high.max(x);
            }
        }
        (high > low).then_some((low, high))
    }
}

/// Immutable drained scene: the hollow shell and the union of disjoint cutter prisms.
#[derive(Debug)]
pub struct SampledDrainedModel {
    hollow: SampledHollowedModel,
    cutters: TriangleMesh,
    prisms: Vec<Cut>,
    openings: Vec<SampledDrainageOpening>,
    regions: Vec<CavityRegion>,
    plan: Vec<u8>,
}

impl SampledDrainedModel {
    /// Hollowing result; its shell is the body from which the cutters are subtracted.
    #[must_use]
    pub fn hollowed_model(&self) -> &SampledHollowedModel {
        &self.hollow
    }

    /// Certified openings in plan order.
    #[must_use]
    pub fn openings(&self) -> &[SampledDrainageOpening] {
        &self.openings
    }

    /// Cavity regions in a deterministic order (first appearance of their outer surface).
    #[must_use]
    pub fn cavity_regions(&self) -> &[CavityRegion] {
        &self.regions
    }

    /// Regions with no certified opening. Geometric statement only: no physical drainage claim.
    #[must_use]
    pub fn enclosed_cavities(&self) -> usize {
        self.regions
            .iter()
            .filter(|r| r.drained_by.is_empty())
            .count()
    }

    /// Plan identity including every cut.
    #[must_use]
    pub fn plan_record(&self) -> &[u8] {
        &self.plan
    }

    /// Backend identity.
    #[must_use]
    pub const fn backend_id(&self) -> &'static str {
        "sampled-cardinal-layer-difference-v1"
    }

    /// Union of the cutter prisms, in plan order (pairwise separated, closed, outward).
    #[must_use]
    pub fn cutters(&self) -> &TriangleMesh {
        &self.cutters
    }

    /// Strict layer engine over the shell (exterior plus reversed cavity) in world coordinates.
    #[must_use]
    pub fn body_layers<'a, 'c, 'o>(
        &'a self,
        schedule: &'a LayerSchedule,
        config: LayerConfig,
        cancel: Cancellation<'c>,
        observer: Option<&'o mut dyn FnMut(LayerProgress)>,
    ) -> LayerEngine<'a, 'c, 'o> {
        LayerEngine::new(
            self.hollow.mesh(),
            schedule,
            SectionTransform::Identity,
            config,
            cancel,
            observer,
        )
        .with_represented_coordinates()
    }
}

impl LayerCut for SampledDrainedModel {
    /// Disjoint analytic sections of the separated cutter prisms at the plane.
    fn rings(&self, plane: LayerPlane) -> Vec<Vec<[f64; 2]>> {
        self.prisms
            .iter()
            .filter_map(|p| p.section(plane.z_mm))
            .collect()
    }
}

pub(super) fn unsupported(reason: &'static str) -> DrainageError {
    DrainageError::Unsupported { reason }
}

pub(super) fn resource(resource: &'static str) -> DrainageError {
    DrainageError::Resource { resource }
}

fn opening(operation_id: u64, issue: OpeningIssue) -> DrainageError {
    DrainageError::Opening {
        operation_id,
        issue,
    }
}

/// Labeled surface index with per-component signed volumes.
pub(super) struct Surface {
    pub(super) bvh: Bvh,
    components: usize,
    volumes: Vec<f64>,
    /// One vertex of each component, for nesting queries.
    witnesses: Vec<[f64; 3]>,
    /// The coordinate extrema of each component's vertices, `[min, max]`.
    bounds: Vec<[[f64; 3]; 2]>,
    /// Exact coordinate extrema of every vertex.
    pub(super) min: [f64; 3],
    pub(super) max: [f64; 3],
}

impl Surface {
    /// Bytes the index keeps once built, excluding allocator overhead.
    pub(super) fn retained_bytes(&self) -> usize {
        self.bvh.retained_bytes()
            + self.volumes.capacity() * std::mem::size_of::<f64>()
            + self.witnesses.capacity() * std::mem::size_of::<[f64; 3]>()
            + self.bounds.capacity() * std::mem::size_of::<[[f64; 3]; 2]>()
    }

    pub(super) fn build(
        mesh: &TriangleMesh,
        faces: &[[u32; 3]],
        cancel: &mut Cancellation<'_>,
    ) -> Result<Self, DrainageError> {
        let (labels, components) = edge_components(faces, cancel).map_err(DrainageError::Hollow)?;
        let mut volumes = filled(components, 0.0)?;
        let mut witnesses = filled(components, [0.0; 3])?;
        let mut bounds = filled(components, [[f64::INFINITY; 3], [f64::NEG_INFINITY; 3]])?;
        let mut seen = filled(components, false)?;
        let (mut min, mut max) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
        for (index, face) in faces.iter().enumerate() {
            cancel.poll(Checkpoint::DrainageWork, index)?;
            let corners = face.map(|v| mesh.vertices()[v as usize].coordinates());
            for corner in corners {
                for k in 0..3 {
                    min[k] = min[k].min(corner[k]);
                    max[k] = max[k].max(corner[k]);
                }
            }
            let label = labels[index] as usize;
            let [low, high] = &mut bounds[label];
            for corner in corners {
                for k in 0..3 {
                    low[k] = low[k].min(corner[k]);
                    high[k] = high[k].max(corner[k]);
                }
            }
            if !seen[label] {
                seen[label] = true;
                witnesses[label] = corners[0];
            }
            // Signed volume relative to the component's own first vertex: the rounding error then
            // scales with the component size, not with its distance from the world origin, so
            // the outer/island sign stays reliable for tiny pockets far from the origin.
            let o = witnesses[label];
            let [a, b, c] = corners.map(|p| [p[0] - o[0], p[1] - o[1], p[2] - o[2]]);
            volumes[label] += (a[0] * (b[1] * c[2] - b[2] * c[1])
                + a[1] * (b[2] * c[0] - b[0] * c[2])
                + a[2] * (b[0] * c[1] - b[1] * c[0]))
                / 6.0;
        }
        let bvh = Bvh::build_indexed(mesh, faces, &labels, cancel, Checkpoint::DrainageWork)
            .map_err(|failure| match failure {
                BuildFailure::Cancelled(cancelled) => DrainageError::Cancelled(cancelled),
                BuildFailure::Allocation => resource("surface index"),
            })?;
        Ok(Self {
            bvh,
            components,
            volumes,
            witnesses,
            bounds,
            min,
            max,
        })
    }

    /// Whether `p` is joined to a point outside the surface's bounding box by a straight segment
    /// that meets no triangle (exact). Then `p` lies in the unbounded component of the surface's
    /// complement, since the outside of a box is connected, unbounded and free of the surface.
    ///
    /// The 26 directions towards the neighbours of a unit cube are tried in a fixed order:
    /// `first` (away from the model), the other axis directions, the 12 face diagonals and the 8
    /// body diagonals. Each segment ends beyond the box, checked exactly on the represented end.
    fn escapes(&self, p: [f64; 3], first: [i8; 3]) -> bool {
        let span = (0..3)
            .map(|k| self.max[k].max(p[k]) - self.min[k].min(p[k]))
            .fold(0.0, f64::max);
        let length = span + 1.0;
        let mut directions: Vec<[i8; 3]> = (0..27)
            .map(|code: i8| [code / 9 - 1, code / 3 % 3 - 1, code % 3 - 1])
            .filter(|d| *d != [0; 3] && *d != first)
            .collect();
        directions.sort_by_key(|d| d.iter().filter(|&&c| c != 0).count());
        std::iter::once(first).chain(directions).any(|d| {
            let q: [f64; 3] = std::array::from_fn(|k| p[k] + length * f64::from(d[k]));
            let outside = (0..3).any(|k| q[k] > self.max[k] || q[k] < self.min[k]);
            outside && !self.bvh.segment_meets(p, q)
        })
    }

    /// Certified clearance of a cardinal cap from the surface: `None` when the cap meets the
    /// surface or is not proven farther than `required`; otherwise a certified lower bound on the
    /// distance, at least `required`.
    ///
    /// `polygon` is the convex cap ring normal to world `axis`, and `center` its centre. When the
    /// centre lies exactly in the cap plane, the cap is covered by the wedges from the centre to
    /// each edge (a ray from the centre through any cap point leaves the cap through an edge);
    /// otherwise by the fan from the first vertex. Each piece is admitted exactly with a threshold
    /// of `required`, so farther boxes are pruned at once, and small wedges keep each query local.
    /// The reported value comes from one bounded best-first search over the disk that encloses the
    /// cap ([`Bvh::disk_lower_bound`]).
    fn cap_clearance(
        &self,
        polygon: &[Point3],
        center: [f64; 3],
        axis: Option<usize>,
        required: f64,
        cancel: &mut Cancellation<'_>,
    ) -> Result<Option<f64>, DrainageError> {
        let ring: Vec<[f64; 3]> = {
            let mut ring = Vec::new();
            ring.try_reserve_exact(polygon.len())
                .map_err(|_| resource("cap ring"))?;
            ring.extend(polygon.iter().map(|p| p.coordinates()));
            ring
        };
        #[allow(clippy::float_cmp)] // Exact coplanarity with the cap plane.
        let planar = axis.is_some_and(|axis| ring.iter().all(|v| v[axis] == center[axis]));
        let count = ring.len();
        let pieces = (0..count).filter_map(|k| {
            let (a, b) = (ring[k], ring[(k + 1) % count]);
            if planar {
                Some(Triangle::new([center, a, b]))
            } else {
                // Fan from the first vertex: its two incident edges add no triangle.
                (k != 0 && k + 1 != count).then(|| Triangle::new([ring[0], a, b]))
            }
        });
        for piece in pieces {
            match self
                .bvh
                .clearance(&piece, required, cancel, Checkpoint::DrainageWork)?
            {
                Clearance::Beyond => {}
                Clearance::Meets | Clearance::Within => return Ok(None),
            }
        }
        if !planar {
            // An oblique cap (TD-066): the smallest certified bound over its fan pieces. A
            // non-planar cardinal cap has no enclosing disk; the admission still proved the
            // distance > required.
            if axis.is_some() {
                return Ok(Some(required));
            }
            let mut bound = f64::INFINITY;
            for k in 1..count - 1 {
                let piece = Triangle::new([ring[0], ring[k], ring[k + 1]]);
                bound = bound.min(self.bvh.lower_bound(
                    &piece,
                    cancel,
                    Checkpoint::DrainageWork,
                )?);
            }
            return Ok(Some(bound.max(required)));
        }
        let Some(axis) = axis else {
            return Ok(Some(required));
        };
        // Upper bound on every vertex's distance from the centre, and the ring's exact box.
        let mut radius = 0.0f64;
        let (mut min, mut max) = (ring[0], ring[0]);
        for v in &ring {
            let squared = (0..3).fold(I::point(0.0), |sum, k| {
                let d = I::point(v[k]).sub(I::point(center[k]));
                sum.add(d.mul(d))
            });
            radius = radius.max(squared.sqrt().map_or(f64::INFINITY, |r| r.hi));
            for k in 0..3 {
                min[k] = min[k].min(v[k]);
                max[k] = max[k].max(v[k]);
            }
        }
        let disk = CapDisk {
            center,
            axis,
            radius,
            min,
            max,
        };
        let bound = self
            .bvh
            .disk_lower_bound(&disk, cancel, Checkpoint::DrainageWork)?;
        // Both values are certified lower bounds; the admission proved the distance > required.
        Ok(Some(bound.max(required)))
    }

    /// Components with odd parity at a point not on the surface (excluding `skip` and the
    /// subtrees `mask` rules out), sorted and distinct.
    pub(super) fn parity(
        &self,
        p: [f64; 3],
        skip: Option<u32>,
        mask: Option<&[bool]>,
    ) -> Result<Vec<u32>, DrainageError> {
        self.bvh
            .vertical_odd_labels(p, skip, mask)
            .map_err(|failure| match failure {
                ParityFailure::OnSurface => DrainageError::Uncertain {
                    reason: "parity query point lies on the surface",
                },
                ParityFailure::Allocation => resource("parity crossings"),
            })
    }
}

/// Whether the source is embedded with pairwise-disjoint bodies: the per-component parity
/// fallback is sound only then, because two bodies that touch or overlap (or a self-intersecting
/// body) can seal a pocket that is outside every component individually. Any improper contact is
/// found by an exact test.
///
/// Every pair of triangles with overlapping boxes is tested. The work is proportional to the
/// number of such pairs: near-linear for well-shaped meshes, but up to quadratic in the triangle
/// count when boxes overlap widely (a high-valence vertex such as the apex of a fan-triangulated
/// cap, or long sliver strips). Cancellation is polled throughout.
fn check_embedded(
    mesh: &TriangleMesh,
    faces: &[[u32; 3]],
    surface: &Surface,
    cancel: &mut Cancellation<'_>,
) -> Result<bool, DrainageError> {
    let corners = |face: [u32; 3]| face.map(|v| mesh.vertices()[v as usize].coordinates());
    let found = surface
        .bvh
        .find_pair(cancel, Checkpoint::DrainageWork, |x, y| {
            let (fx, fy) = (faces[x as usize], faces[y as usize]);
            improper_contact(fx, corners(fx), fy, corners(fy))
        })?;
    Ok(found.is_none())
}

/// A vector of `count` copies of `value`, reserved fallibly.
fn filled<T: Clone>(count: usize, value: T) -> Result<Vec<T>, DrainageError> {
    let mut vector = Vec::new();
    vector
        .try_reserve_exact(count)
        .map_err(|_| resource("drainage working memory"))?;
    vector.resize(count, value);
    Ok(vector)
}

/// Peak bytes one indexed surface face needs while building a [`Surface`]: its component label,
/// then the larger of the two phases, which never overlap (TD-094): the edge-component
/// labelling (three edge keys and their merge buffer, then a parent and a root label) or the
/// BVH construction with the indexed triangle, written once in slot order.
pub(super) const SURFACE_BYTES_PER_FACE: usize = std::mem::size_of::<u32>()
    + max(
        2 * 3 * std::mem::size_of::<(u64, u32)>(),
        std::mem::size_of::<Triangle>() + Bvh::BUILD_BYTES_PER_TRIANGLE,
    );

const fn max(a: usize, b: usize) -> usize {
    if a > b {
        a
    } else {
        b
    }
}

/// Cavity regions from outward cavity surfaces: one per positive-volume component, plus the
/// island surfaces directly nested inside it. Returns regions and, per component, its region.
///
/// Ownership needs only the outer surfaces, so island rays skip every subtree that holds island
/// triangles alone: a ray no longer crosses the islands stacked above it.
pub(super) fn cavity_regions(
    cavity: &Surface,
    cancel: &mut Cancellation<'_>,
) -> Result<(Vec<CavityRegion>, Vec<usize>), DrainageError> {
    let mut region_of = filled(cavity.components, usize::MAX)?;
    let mut regions = Vec::new();
    regions
        .try_reserve_exact(cavity.volumes.iter().filter(|&&v| v > 0.0).count())
        .map_err(|_| resource("cavity regions"))?;
    for (k, &volume) in cavity.volumes.iter().enumerate() {
        if volume > 0.0 {
            region_of[k] = regions.len();
            regions.push(CavityRegion {
                volume_mm3: volume,
                surfaces: 1,
                drained_by: Vec::new(),
                point_mm: cavity.witnesses[k],
                bounds_mm: cavity.bounds[k],
            });
        }
    }
    // Built on the first island only: most cavities have none.
    let mut outer_mask: Option<Vec<bool>> = None;
    for (k, &volume) in cavity.volumes.iter().enumerate() {
        if volume > 0.0 {
            continue;
        }
        cancel.check(Checkpoint::DrainageWork)?;
        // An island belongs to the innermost outer surface containing it.
        let label = u32::try_from(k).map_err(|_| resource("cavity component index"))?;
        let mask = match &outer_mask {
            Some(mask) => mask,
            None => outer_mask.insert(
                cavity
                    .bvh
                    .node_mask(|l| cavity.volumes.get(l as usize).is_some_and(|&v| v > 0.0))
                    .ok_or_else(|| resource("cavity outer-surface mask"))?,
            ),
        };
        let odd = cavity.parity(cavity.witnesses[k], Some(label), Some(mask))?;
        let owner = innermost(cavity, &odd).ok_or(DrainageError::Uncertain {
            reason: "cavity island without an enclosing outer surface",
        })?;
        let region = &mut regions[region_of[owner]];
        region.volume_mm3 += volume;
        region.surfaces += 1;
        region_of[k] = region_of[owner];
    }
    Ok((regions, region_of))
}

/// Smallest-volume outer (positive) component among the odd-parity components `odd`, i.e. the
/// innermost outer surface around the point.
pub(super) fn innermost(cavity: &Surface, odd: &[u32]) -> Option<usize> {
    odd.iter()
        .map(|&k| k as usize)
        .filter(|&k| cavity.volumes.get(k).is_some_and(|&v| v > 0.0))
        .min_by(|&a, &b| {
            cavity.volumes[a]
                .total_cmp(&cavity.volumes[b])
                .then(a.cmp(&b))
        })
}

/// Hollows with the sampled backend, then certifies and assembles finite drainage cuts.
///
/// The plan must be one hollow operation followed by zero or more holes. Cuts must be parallel to
/// a world axis. Each needs an entry cap entirely in the unbounded exterior and an end cap entirely
/// inside the cavity, both clear of the respective surface by `minimum_clearance_mm`; cutter
/// bounding boxes must be separated. Regions without a certified cut are reported as enclosed.
///
/// # Errors
/// Structured recipe, hollowing, opening, overlap, numerical, resource and cancellation errors;
/// nothing partial is published.
pub fn drain_model_sampled(
    source: &TriangleMesh,
    plan: &PreparationPlan,
    limits: SampledDrainageLimits,
    cancel: &mut Cancellation<'_>,
    progress: &mut dyn FnMut(SampledHollowProgress),
) -> Result<SampledDrainedModel, DrainageError> {
    cancel.check(Checkpoint::DrainageStart)?;
    let recipe = Recipe::of(plan, limits)?;
    let prefix_operations = plan.operations()[..recipe.prefix].to_vec();
    let prefix = PreparationPlan::new(
        plan.source(),
        plan.sampling(),
        prefix_operations,
        PlanLimits::default(),
        cancel,
    )
    .map_err(DrainageError::Plan)?;
    let hollow =
        hollow_mesh_sampled_with_progress(source, &prefix, limits.hollow, cancel, progress)
            .map_err(DrainageError::Hollow)?;
    drain_shell(hollow, plan, &recipe, limits, cancel, progress)
}

/// Certifies and assembles the drainage cuts of `plan` on a model already hollowed (TD-089).
///
/// `hollow` must be the plan's hollowing prefix, typically computed in the model frame and
/// mapped by the object's placement ([`SampledHollowedModel::placed`]). The plan's holes are in
/// the hollow's coordinates (world millimetres for a placed hollow), and the plan's own
/// placement is not applied again. The cuts are certified against the shell and cavity exactly
/// as [`drain_model_sampled`] certifies them, without hollowing again. The hollow's thickness
/// and cell are checked against the plan.
///
/// # Errors
/// A hollow that does not match the plan, and the opening, overlap, numerical, resource and
/// cancellation errors of [`drain_model_sampled`].
pub fn drain_hollowed_sampled(
    hollow: SampledHollowedModel,
    plan: &PreparationPlan,
    limits: SampledDrainageLimits,
    cancel: &mut Cancellation<'_>,
    progress: &mut dyn FnMut(SampledHollowProgress),
) -> Result<SampledDrainedModel, DrainageError> {
    cancel.check(Checkpoint::DrainageStart)?;
    let recipe = Recipe::of(plan, limits)?;
    let certificate = hollow.certificate();
    if certificate.thickness_mm.to_bits() != recipe.thickness_mm.to_bits()
        || certificate.cell_mm.to_bits() != plan.sampling().cell_mm.to_bits()
    {
        return Err(unsupported(
            "the hollowed model does not match the plan's thickness or cell",
        ));
    }
    drain_shell(hollow, plan, &recipe, limits, cancel, progress)
}

/// A validated hollow-then-holes recipe: where its holes start and the hollow's thickness.
struct Recipe {
    /// Operations before the holes: an optional repair, the hollow and an optional
    /// simplification.
    prefix: usize,
    thickness_mm: f64,
}

impl Recipe {
    fn of(plan: &PreparationPlan, limits: SampledDrainageLimits) -> Result<Self, DrainageError> {
        let clearance = limits.minimum_clearance_mm;
        if !clearance.is_finite() || clearance <= 0.0 {
            return Err(unsupported("clearance must be positive finite millimeters"));
        }
        // An optional leading source repair (TD-054) belongs to the hollowing prefix.
        let (repair, operations) = super::split_repair(plan.operations());
        let Some(first) = operations.first() else {
            return Err(unsupported("hollow-first recipe required"));
        };
        // The hollowing prefix: the hollow and an optional cavity simplification (TD-066).
        let shell_operations = 1 + usize::from(
            operations
                .get(1)
                .is_some_and(|o| matches!(o.kind, OperationKind::Simplify(_))),
        );
        let holes = &operations[shell_operations..];
        let OperationKind::Hollow(spec) = &first.kind else {
            return Err(unsupported(
                "only hollow followed by drainage cuts is supported",
            ));
        };
        if holes.iter().any(|o| {
            !matches!(
                o.kind,
                OperationKind::Hole(_) | OperationKind::SurfaceHole(_)
            )
        }) {
            return Err(unsupported(
                "only hollow followed by drainage cuts is supported",
            ));
        }
        if holes.len() > limits.max_holes || limits.max_segments > 4096 || limits.max_segments < 8 {
            return Err(resource("hole or segment limit"));
        }
        Ok(Self {
            prefix: usize::from(repair.is_some()) + shell_operations,
            thickness_mm: spec.thickness_mm,
        })
    }
}

/// Certify and assemble the holes of `plan` on `hollow`.
fn drain_shell(
    hollow: SampledHollowedModel,
    plan: &PreparationPlan,
    recipe: &Recipe,
    limits: SampledDrainageLimits,
    cancel: &mut Cancellation<'_>,
    progress: &mut dyn FnMut(SampledHollowProgress),
) -> Result<SampledDrainedModel, DrainageError> {
    let clearance = limits.minimum_clearance_mm;
    let holes = &plan.operations()[recipe.prefix..];
    let count = holes.len();
    let shell = hollow.mesh();
    let exterior_faces = hollow.exterior_faces();
    let source_faces = &shell.triangles()[..exterior_faces];
    let total = u64::try_from(count).map_err(|_| resource("hole count"))?;
    progress(SampledHollowProgress {
        stage: SampledHollowStage::Drainage,
        completed_items: 0,
        total_items: total,
    });
    // Both surface indexes are admitted against the payload cap before either is built, each on
    // its own: they are built one after the other.
    for faces in [source_faces.len(), hollow.cavity().triangles().len()] {
        let surface_bytes = faces
            .checked_mul(SURFACE_BYTES_PER_FACE)
            .and_then(|bytes| u64::try_from(bytes).ok())
            .ok_or_else(|| resource("drainage surface index payload"))?;
        if surface_bytes > limits.hollow.max_payload_bytes {
            return Err(resource("drainage surface index payload"));
        }
    }
    let source = Surface::build(shell, source_faces, cancel)?;
    let cavity_mesh = hollow.cavity();
    let cavity = Surface::build(cavity_mesh, cavity_mesh.triangles(), cancel)?;
    let (mut regions, region_of) = cavity_regions(&cavity, cancel)?;
    let mut certifier = Certifier {
        source,
        cavity,
        region_of,
        clearance,
        embedding: Embedding::Unchecked {
            shell,
            faces: source_faces,
        },
    };
    let (cutters, prisms, openings) = evaluate_cuts(
        holes,
        plan,
        limits,
        &mut certifier,
        &mut regions,
        cancel,
        progress,
    )?;
    drop(certifier);
    let cutters = assemble(&cutters, cancel)?;
    let record = plan.canonical_record().to_vec();
    cancel.check(Checkpoint::DrainagePublish)?;
    Ok(SampledDrainedModel {
        hollow,
        cutters,
        prisms,
        openings,
        regions,
        plan: record,
    })
}

/// Operation identity a probed hole reports in its errors.
pub const PROBE_OPERATION_ID: u64 = 1;

/// Where a surface hole opens: the point of the surface it starts from, and the surface's
/// outward unit normal there, from which its tilt is measured.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HoleFrame {
    /// The surface point nearest the requested one, in world millimetres.
    pub foot_mm: [f64; 3],
    /// The outward unit normal at the foot (averaged over the hole's radius).
    pub outward: [f64; 3],
}

/// One drain hole certified alone by a [`DrainageProbe`].
#[derive(Debug)]
pub struct ProbedHole {
    /// The certified opening: entry, direction, length, cavity region and clearances.
    pub opening: SampledDrainageOpening,
    /// The cut as resolved.
    pub spec: HoleSpec,
    /// Where a surface hole opens (`None` for an explicit cut).
    pub frame: Option<HoleFrame>,
    /// The cutter as built, in world millimetres, for display.
    pub cutter: TriangleMesh,
    /// The cutter's bounding box: per axis, its minimum and maximum.
    pub bounds: [[f64; 2]; 3],
}

/// A drain hole a [`DrainageProbe`] refused, with as much of its shape as was resolved, so an
/// interface can still show where it would be.
#[derive(Debug)]
pub struct RefusedHole {
    /// Why the hole is refused.
    pub error: DrainageError,
    /// The cut as resolved, when its axis was. A surface hole whose automatic depth found no
    /// cavity is given a nominal length of [`NOMINAL_REFUSED_DEPTH_MM`] past the surface, for
    /// display only.
    pub spec: Option<HoleSpec>,
    /// Where a surface hole opens, when its axis was resolved.
    pub frame: Option<HoleFrame>,
    /// The cutter of `spec`, when it could be built.
    pub cutter: Option<TriangleMesh>,
}

/// Depth past the surface given, for display only, to a refused hole whose automatic depth
/// found no cavity.
pub const NOMINAL_REFUSED_DEPTH_MM: f64 = 5.0;

/// Certifies drain holes one at a time against a hollowed model (TD-084), so an interface can
/// tell whether a hole is valid while it is being placed.
///
/// A hole is resolved, built and certified exactly as [`drain_model_sampled`] does with the
/// same `distance_error_mm` and limits, and must be separated from the `others` cutter boxes as
/// drainage requires between cuts. Building the probe indexes the shell and the cavity once,
/// and decides once whether the source is embedded (the check drainage runs only when an entry
/// has no straight escape), so each [`Self::check`] is a few index queries.
pub struct DrainageProbe {
    certifier: Certifier<'static>,
    clearance: f64,
    distance_error_mm: f64,
    max_segments: usize,
}

impl DrainageProbe {
    /// Index `model` for probing holes.
    ///
    /// # Errors
    /// Invalid limits, cancellation, or an index that cannot be built.
    pub fn new(
        model: &SampledHollowedModel,
        limits: SampledDrainageLimits,
        distance_error_mm: f64,
        cancel: &mut Cancellation<'_>,
    ) -> Result<Self, DrainageError> {
        let clearance = limits.minimum_clearance_mm;
        if !clearance.is_finite() || clearance <= 0.0 {
            return Err(unsupported("clearance must be positive finite millimeters"));
        }
        let shell = model.mesh();
        let exterior_faces = model.exterior_faces();
        let source_faces = &shell.triangles()[..exterior_faces];
        let source = Surface::build(shell, source_faces, cancel)?;
        let cavity_mesh = model.cavity();
        let cavity = Surface::build(cavity_mesh, cavity_mesh.triangles(), cancel)?;
        let (_, region_of) = cavity_regions(&cavity, cancel)?;
        let embedded = check_embedded(shell, source_faces, &source, cancel)?;
        Ok(Self {
            certifier: Certifier {
                source,
                cavity,
                region_of,
                clearance,
                embedding: Embedding::Known(embedded),
            },
            clearance,
            distance_error_mm,
            max_segments: limits.max_segments,
        })
    }

    /// Certify one hole (an [`OperationKind::Hole`] or [`OperationKind::SurfaceHole`]) apart
    /// from the cutters bounded by `others`. Errors name [`PROBE_OPERATION_ID`].
    ///
    /// # Errors
    /// The refused hole, with its [`DrainageError`] (an opening issue, a cut that cannot be
    /// built, a potential overlap with another cutter, or cancellation) and the part of its
    /// shape that was resolved.
    pub fn check(
        &mut self,
        hole: &OperationKind,
        others: &[[[f64; 2]; 3]],
        cancel: &mut Cancellation<'_>,
    ) -> Result<ProbedHole, Box<RefusedHole>> {
        let id = PROBE_OPERATION_ID;
        let refused = |error, spec, frame, cutter| {
            Box::new(RefusedHole {
                error,
                spec,
                frame,
                cutter,
            })
        };
        // The same steps as drainage's, keeping what each resolved.
        let (spec, frame, pending) = match *hole {
            OperationKind::Hole(spec) => (spec, None, None),
            OperationKind::SurfaceHole(request) => {
                let axis = self
                    .certifier
                    .surface_axis(request, id, cancel)
                    .map_err(|e| refused(e, None, None, None))?;
                let frame = HoleFrame {
                    foot_mm: axis.foot,
                    outward: axis.outward,
                };
                match self.certifier.axis_length(request, &axis, id, cancel) {
                    Ok(length) => (axis.spec(request, length), Some(frame), None),
                    Err(error) => (
                        axis.spec(request, request.outside_mm + NOMINAL_REFUSED_DEPTH_MM),
                        Some(frame),
                        Some(error),
                    ),
                }
            }
            _ => {
                return Err(refused(
                    unsupported("only drainage cuts can be probed"),
                    None,
                    None,
                    None,
                ))
            }
        };
        let cut = build_cut(spec, self.distance_error_mm, self.max_segments, id, cancel)
            .map_err(|e| refused(e, Some(spec), frame, None))?;
        if let Some(error) = pending {
            return Err(refused(error, Some(spec), frame, Some(cut.mesh)));
        }
        let evaluated =
            certify_cut(&mut self.certifier, spec, cut, id, cancel).map_err(|back| {
                let (error, cut) = *back;
                refused(error, Some(spec), frame, Some(cut.mesh))
            })?;
        if others
            .iter()
            .any(|&other| !separated(evaluated.bounds, other, self.clearance))
        {
            return Err(refused(
                DrainageError::PotentialOverlap {
                    first: 0,
                    second: id,
                },
                Some(spec),
                frame,
                Some(evaluated.cut.mesh),
            ));
        }
        Ok(ProbedHole {
            opening: evaluated.opening,
            spec,
            frame,
            cutter: evaluated.cut.mesh,
            bounds: evaluated.bounds,
        })
    }
}

/// Whether the source is embedded with pairwise-disjoint bodies ([`check_embedded`]).
enum Embedding<'a> {
    /// Not checked yet: the check runs once, only for an entry that no straight escape segment
    /// joins to the outside of the source's bounding box. The hollow shell's first `faces` are
    /// the placed source.
    Unchecked {
        shell: &'a TriangleMesh,
        faces: &'a [[u32; 3]],
    },
    /// Checked: embedded or not.
    Known(bool),
}

/// Surfaces and admission state shared by the certificates of every cut.
struct Certifier<'a> {
    source: Surface,
    cavity: Surface,
    /// Region of each cavity surface component.
    region_of: Vec<usize>,
    clearance: f64,
    embedding: Embedding<'a>,
}

/// One certified cut, before its separation from the others is checked.
struct Evaluated {
    cut: Cutter,
    /// The world axis of a cardinal cut.
    axis_index: Option<usize>,
    opening: SampledDrainageOpening,
    bounds: [[f64; 2]; 3],
}

/// A surface hole's resolved opening: its foot, the outward normal, entry and inward axis.
struct SurfaceAxis {
    foot: [f64; 3],
    outward: [f64; 3],
    entry: [f64; 3],
    inward: [f64; 3],
}

impl SurfaceAxis {
    fn spec(&self, request: SurfaceHoleSpec, length_mm: f64) -> HoleSpec {
        HoleSpec {
            entry_mm: self.entry,
            direction: self.inward,
            length_mm,
            diameter_mm: request.diameter_mm,
        }
    }
}

/// Resolve, build and certify one cut (a hole or a surface hole), as drainage does.
fn evaluate_one(
    certifier: &mut Certifier<'_>,
    operation: &Operation,
    distance_error_mm: f64,
    max_segments: usize,
    cancel: &mut Cancellation<'_>,
) -> Result<Evaluated, DrainageError> {
    let spec = match operation.kind {
        OperationKind::Hole(spec) => spec,
        OperationKind::SurfaceHole(request) => certifier.resolve(request, operation.id, cancel)?,
        _ => {
            return Err(unsupported(
                "only hollow followed by drainage cuts is supported",
            ))
        }
    };
    let cut = build_cut(spec, distance_error_mm, max_segments, operation.id, cancel)?;
    certify_cut(certifier, spec, cut, operation.id, cancel).map_err(|refused| refused.0)
}

/// Build the polygonal cylinder of `spec`.
pub(super) fn build_cut(
    spec: HoleSpec,
    distance_error_mm: f64,
    max_segments: usize,
    id: u64,
    cancel: &mut Cancellation<'_>,
) -> Result<Cutter, DrainageError> {
    let build = if is_cardinal(spec.direction) {
        cylinder
    } else {
        oblique_cylinder
    };
    build(spec, distance_error_mm, max_segments, cancel).map_err(|error| match error {
        DrainageError::Unsupported { reason }
        | DrainageError::Uncertain { reason }
        | DrainageError::Resource { resource: reason } => DrainageError::Cut {
            operation_id: id,
            reason,
        },
        other => other,
    })
}

/// Certify a built cut and describe its opening; a refused cut is handed back with the error.
fn certify_cut(
    certifier: &mut Certifier<'_>,
    spec: HoleSpec,
    cut: Cutter,
    id: u64,
    cancel: &mut Cancellation<'_>,
) -> Result<Evaluated, Box<(DrainageError, Cutter)>> {
    let axis = match spec.unit_direction() {
        Ok(axis) => axis,
        Err(error) => return Err(Box::new((DrainageError::Plan(error), cut))),
    };
    let axis_index =
        is_cardinal(spec.direction).then(|| (0..3).find(|&k| axis[k] != 0.0).unwrap_or(2));
    let entry_center = spec.entry_mm;
    let end_center: [f64; 3] = std::array::from_fn(|k| entry_center[k] + axis[k] * spec.length_mm);
    let outcome = certifier.certify(
        &cut,
        [entry_center, end_center],
        axis_index,
        axis,
        id,
        cancel,
    );
    let (entry_clearance, terminal_clearance, region) = match outcome {
        Ok(found) => found,
        Err(error) => return Err(Box::new((error, cut))),
    };
    let bounds = bounds(cut.mesh.vertices());
    Ok(Evaluated {
        opening: SampledDrainageOpening {
            operation_id: id,
            segments: cut.entry.len(),
            radial_error_mm: cut.radial_error_mm,
            axial_error_mm: cut.axial_error_mm,
            entry_clearance_mm: entry_clearance,
            terminal_clearance_mm: terminal_clearance,
            cavity_region: region,
            entry_mm: entry_center,
            direction: axis,
            length_mm: spec.length_mm,
        },
        cut,
        axis_index,
        bounds,
    })
}

/// Whether two cutter boxes are separated by more than `clearance` along some axis (the
/// conservative separation drainage requires between cuts).
fn separated(a: [[f64; 2]; 3], b: [[f64; 2]; 3], clearance: f64) -> bool {
    (0..3).any(|i| {
        I::point(a[i][1]).add(I::point(clearance)).hi < b[i][0]
            || I::point(b[i][1]).add(I::point(clearance)).hi < a[i][0]
    })
}

/// Build, certify and separate every cut in plan order; record which region each drains.
type CutResults = (Vec<TriangleMesh>, Vec<Cut>, Vec<SampledDrainageOpening>);
fn evaluate_cuts(
    holes: &[Operation],
    plan: &PreparationPlan,
    limits: SampledDrainageLimits,
    certifier: &mut Certifier<'_>,
    regions: &mut [CavityRegion],
    cancel: &mut Cancellation<'_>,
    progress: &mut dyn FnMut(SampledHollowProgress),
) -> Result<CutResults, DrainageError> {
    let clearance = limits.minimum_clearance_mm;
    let total = u64::try_from(holes.len()).map_err(|_| resource("hole count"))?;
    let mut cutters = Vec::new();
    let mut prisms = Vec::new();
    let mut openings = Vec::new();
    let mut boxes: Vec<(u64, [[f64; 2]; 3])> = Vec::new();
    for (done, operation) in (1..).zip(holes) {
        cancel.check(Checkpoint::DrainageWork)?;
        let evaluated = evaluate_one(
            certifier,
            operation,
            plan.sampling().distance_error_mm,
            limits.max_segments,
            cancel,
        )?;
        let id = operation.id;
        // Conservative cutter separation, as for convex drainage.
        for &(other_id, other) in &boxes {
            if !separated(evaluated.bounds, other, clearance) {
                return Err(DrainageError::PotentialOverlap {
                    first: other_id,
                    second: id,
                });
            }
        }
        boxes.push((id, evaluated.bounds));
        regions[evaluated.opening.cavity_region].drained_by.push(id);
        prisms.push(match evaluated.axis_index {
            Some(axis) => Cut::Cardinal(Prism::from_cutter(&evaluated.cut, axis)),
            None => Cut::Oblique(ObliquePrism::from_cutter(&evaluated.cut)),
        });
        openings.push(evaluated.opening);
        cutters.push(evaluated.cut.mesh);
        progress(SampledHollowProgress {
            stage: SampledHollowStage::Drainage,
            completed_items: done,
            total_items: total,
        });
    }
    Ok((cutters, prisms, openings))
}

/// The tangents a hole's tilt direction is measured in (see [`HoleTilt`]), for the unit
/// `outward` normal: the reference tangent, then the one a quarter turn counterclockwise seen
/// from outside.
#[must_use]
pub fn hole_tangents(outward: [f64; 3]) -> ([f64; 3], [f64; 3]) {
    let reference = if outward[2].abs() > 0.9 {
        [1.0, 0.0, 0.0]
    } else {
        [0.0, 0.0, 1.0]
    };
    let along = (0..3).map(|k| reference[k] * outward[k]).sum::<f64>();
    let u: [f64; 3] = std::array::from_fn(|k| reference[k] - along * outward[k]);
    let length = u.iter().map(|c| c * c).sum::<f64>().sqrt();
    let u = u.map(|c| c / length);
    // Counterclockwise seen from outside: the second tangent is outward x u.
    let v = [
        outward[1] * u[2] - outward[2] * u[1],
        outward[2] * u[0] - outward[0] * u[2],
        outward[0] * u[1] - outward[1] * u[0],
    ];
    (u, v)
}

/// The inward axis of a hole tilted from the unit `outward` normal (see [`HoleTilt`]), a unit
/// vector.
#[must_use]
pub fn tilted_axis(outward: [f64; 3], tilt: HoleTilt) -> [f64; 3] {
    let (u, v) = hole_tangents(outward);
    // Fixed IEEE operations, not the platform's `sin_cos`: the axis is the same bit for bit on
    // every platform (TD-061).
    let (sin_a, cos_a) = sin_cos_degrees(tilt.azimuth_deg);
    let (sin_t, cos_t) = sin_cos_degrees(tilt.angle_deg);
    std::array::from_fn(|k| -cos_t * outward[k] + sin_t * (cos_a * u[k] + sin_a * v[k]))
}

/// The outward normal a surface hole's axis follows at `foot`, a point of `mesh`'s surface
/// (TD-066): the area-weighted mean normal of every face within `radius` of `foot`, as drainage
/// resolves a surface hole, in `mesh`'s frame. With [`axis_tilt`] it turns a hole along a chosen
/// axis exactly (the suction cups' vents, TD-114): a single face's normal can lean far from it
/// on a sculpted surface. `None` when it is undefined: no face that near, or normals that
/// cancel. The faces are scanned, not indexed: for a single hole asked for by the user.
#[must_use]
pub fn surface_hole_outward(mesh: &TriangleMesh, foot: [f64; 3], radius: f64) -> Option<[f64; 3]> {
    let r2 = radius * radius;
    let vertices = mesh.vertices();
    let mut sum = [0.0; 3];
    for corners in mesh.triangles() {
        let triangle = Triangle::new(corners.map(|v| {
            let p = vertices[v as usize];
            [p.x, p.y, p.z]
        }));
        let near = closest_point_on_triangle(foot, &triangle);
        if (0..3).map(|k| (near[k] - foot[k]).powi(2)).sum::<f64>() <= r2 {
            let n = triangle_normal(&triangle);
            for k in 0..3 {
                sum[k] += n[k];
            }
        }
    }
    let norm = sum.iter().map(|c| c * c).sum::<f64>().sqrt();
    (norm.is_finite() && norm > 0.0).then(|| sum.map(|c| c / norm))
}

/// The tilt of the inward `axis` from the unit `outward` normal: the inverse of
/// [`tilted_axis`]. `None` for a zero or non-finite axis.
#[must_use]
pub fn axis_tilt(outward: [f64; 3], axis: [f64; 3]) -> Option<HoleTilt> {
    let dot = |a: [f64; 3], b: [f64; 3]| (0..3).map(|k| a[k] * b[k]).sum::<f64>();
    let length = dot(axis, axis).sqrt();
    if !(length.is_finite() && length > 0.0) {
        return None;
    }
    let axis = axis.map(|c| c / length);
    let (u, v) = hole_tangents(outward);
    let (x, y) = (dot(axis, u), dot(axis, v));
    Some(HoleTilt {
        angle_deg: x.hypot(y).atan2(-dot(axis, outward)).to_degrees(),
        azimuth_deg: y.atan2(x).to_degrees().rem_euclid(360.0),
    })
}

/// `tilt`, measured about the unit `from` normal in one frame, in another frame where the same
/// surface has the unit normal `to` and directions are carried by the linear map `linear`
/// (rows): the hole leans the same way on the moved surface (TD-089). The tangents a tilt is
/// measured in are fixed per frame, so its numbers change while the axis turns with the model.
/// `None` for a singular map.
#[must_use]
pub fn map_tilt(
    tilt: HoleTilt,
    from: [f64; 3],
    linear: [[f64; 3]; 3],
    to: [f64; 3],
) -> Option<HoleTilt> {
    let axis = tilted_axis(from, tilt);
    let carried = linear.map(|row| (0..3).map(|k| row[k] * axis[k]).sum::<f64>());
    axis_tilt(to, carried)
}

impl Certifier<'_> {
    /// Whether the source is embedded, checked once when first needed.
    fn embedded(&mut self, cancel: &mut Cancellation<'_>) -> Result<bool, DrainageError> {
        if let Embedding::Unchecked { shell, faces } = self.embedding {
            let embedded = check_embedded(shell, faces, &self.source, cancel)?;
            self.embedding = Embedding::Known(embedded);
        }
        Ok(matches!(self.embedding, Embedding::Known(true)))
    }

    /// Resolve a surface hole (TD-066) into an explicit cut.
    ///
    /// - **Foot:** the source surface point nearest the request.
    /// - **Axis:** the inward, area-weighted mean normal of every source face within the hole's
    ///   radius of the foot (the outward source orientation is admitted by hollowing).
    /// - **Entry:** `outside_mm` outside the foot along the outward normal.
    /// - **Length:** to the first cavity surface along the axis, found by bisection on the
    ///   exact segment test, plus `inside_mm`. The cut is then certified like any other.
    fn resolve(
        &self,
        request: SurfaceHoleSpec,
        id: u64,
        cancel: &mut Cancellation<'_>,
    ) -> Result<HoleSpec, DrainageError> {
        let axis = self.surface_axis(request, id, cancel)?;
        let length = self.axis_length(request, &axis, id, cancel)?;
        Ok(axis.spec(request, length))
    }

    /// Where a surface hole opens: its foot, the outward normal, its entry and inward axis.
    fn surface_axis(
        &self,
        request: SurfaceHoleSpec,
        id: u64,
        cancel: &mut Cancellation<'_>,
    ) -> Result<SurfaceAxis, DrainageError> {
        let cut = |reason| DrainageError::Cut {
            operation_id: id,
            reason,
        };
        let (_, slot) = self.source.bvh.nearest(request.surface_mm, 0);
        let nearest = self
            .source
            .bvh
            .triangle(slot)
            .ok_or_else(|| cut("the model has no surface"))?;
        let foot = closest_point_on_triangle(request.surface_mm, nearest);
        let radius = request.diameter_mm / 2.0;
        // The faces within the radius, summed in source order as a scan would add them.
        let mut sum = [0.0; 3];
        for (index, (_, slot)) in self
            .source
            .bvh
            .within(foot, radius * radius)
            .into_iter()
            .enumerate()
        {
            cancel.poll(Checkpoint::DrainageWork, index)?;
            if let Some(triangle) = self.source.bvh.triangle(slot) {
                let n = triangle_normal(triangle);
                for k in 0..3 {
                    sum[k] += n[k];
                }
            }
        }
        let norm = sum.iter().map(|c| c * c).sum::<f64>().sqrt();
        if !(norm.is_finite() && norm > 0.0) {
            return Err(cut("the surface normal at the hole is undefined"));
        }
        let outward = sum.map(|c| c / norm);
        let (entry, inward) = match request.tilt {
            None => (
                std::array::from_fn(|k| foot[k] + request.outside_mm * outward[k]),
                // `0 - c` keeps zero components positive (no `-0` in reports or records).
                outward.map(|c| 0.0 - c),
            ),
            Some(tilt) => {
                let axis = tilted_axis(outward, tilt);
                let entry: [f64; 3] =
                    std::array::from_fn(|k| foot[k] - request.outside_mm * axis[k]);
                (entry, axis)
            }
        };
        Ok(SurfaceAxis {
            foot,
            outward,
            entry,
            inward,
        })
    }

    /// The cut length of a surface hole along its axis: to its fixed depth, or `inside_mm` past
    /// the first cavity surface.
    fn axis_length(
        &self,
        request: SurfaceHoleSpec,
        axis: &SurfaceAxis,
        id: u64,
        cancel: &mut Cancellation<'_>,
    ) -> Result<f64, DrainageError> {
        let cut = |reason| DrainageError::Cut {
            operation_id: id,
            reason,
        };
        let (foot, inward) = (axis.foot, axis.inward);
        // First cavity crossing along the axis from the foot.
        let span = (0..3)
            .map(|k| self.source.max[k] - self.source.min[k])
            .fold(0.0, f64::max)
            * 2.0
            + 1.0;
        let along = |t: f64| -> [f64; 3] { std::array::from_fn(|k| foot[k] + t * inward[k]) };
        // A fixed depth ends the cut there; certification then decides whether it drains.
        if let Some(depth) = request.depth_mm {
            return Ok(request.outside_mm + depth);
        }
        if !self.cavity.bvh.segment_meets(foot, along(span)) {
            return Err(cut("the hole axis does not reach the cavity"));
        }
        let (mut low, mut high) = (0.0, span);
        for _ in 0..80 {
            cancel.check(Checkpoint::DrainageWork)?;
            let middle = f64::midpoint(low, high);
            if middle <= low || middle >= high {
                break;
            }
            if self.cavity.bvh.segment_meets(foot, along(middle)) {
                high = middle;
            } else {
                low = middle;
            }
        }
        Ok(request.outside_mm + high + request.inside_mm)
    }

    /// Certify one cut along world `axis`: entry cap in the unbounded exterior and end cap inside
    /// one cavity region, both clear of their surfaces. Returns entry clearance, end clearance
    /// and region.
    fn certify(
        &mut self,
        cut: &Cutter,
        [entry_center, end_center]: [[f64; 3]; 2],
        axis: Option<usize>,
        direction: [f64; 3],
        id: u64,
        cancel: &mut Cancellation<'_>,
    ) -> Result<(f64, f64, usize), DrainageError> {
        let clearance = self.clearance;
        // Entry: disjoint from the source surface and clear of it.
        let entry_clearance = self
            .source
            .cap_clearance(&cut.entry, entry_center, axis, clearance, cancel)?
            .ok_or_else(|| opening(id, OpeningIssue::EntryNotExterior))?;
        // The cap is connected, disjoint from the surface and contains its centre (its certified
        // inner radius is positive), so the centre's complement component is the cap's. The
        // escape search starts away from the model, opposite to the cut direction.
        // The world direction nearest to the reverse of the cut.
        let dominant = axis.unwrap_or_else(|| {
            (0..3)
                .max_by(|&a, &b| {
                    direction[a]
                        .abs()
                        .total_cmp(&direction[b].abs())
                        .then(b.cmp(&a))
                })
                .unwrap_or(2)
        });
        let mut backward = [0i8; 3];
        backward[dominant] = if end_center[dominant] < entry_center[dominant] {
            1
        } else {
            -1
        };
        if !self.source.escapes(entry_center, backward) {
            // No direct escape: per-component parity, sound for an embedded source only.
            if !self.embedded(cancel)? {
                return Err(opening(id, OpeningIssue::SourceNotEmbedded));
            }
            if !self.source.parity(entry_center, None, None)?.is_empty() {
                return Err(opening(id, OpeningIssue::EntryNotExterior));
            }
        }
        // End: disjoint from the cavity surface, clear of it, inside one cavity region.
        let terminal = self
            .cavity
            .cap_clearance(&cut.end, end_center, axis, clearance, cancel)?;
        let cavity_parity = self.cavity.parity(end_center, None, None);
        let region = match (terminal, &cavity_parity) {
            (Some(_), Ok(odd)) if odd.len() % 2 == 1 => {
                innermost(&self.cavity, odd).map(|k| self.region_of[k])
            }
            _ => None,
        };
        let (Some(terminal_clearance), Some(region)) = (terminal, region) else {
            return Err(opening(id, self.classify_end(entry_center, end_center)));
        };
        Ok((entry_clearance, terminal_clearance, region))
    }

    /// Most specific failure category for an end-cap centre that did not certify a cavity entry.
    ///
    /// Contact with a surface is decided exactly (a point query, whatever the face orientation)
    /// before parity. A centre exactly on the source surface ends the cut at the model boundary:
    /// too long if the axis crossed the cavity, too short otherwise. A centre on the cavity
    /// surface is in contact with it, like a cap that touches it.
    fn classify_end(&self, entry: [f64; 3], end: [f64; 3]) -> OpeningIssue {
        let inside = |surface: &Surface| {
            if surface.bvh.segment_meets(end, end) {
                return None;
            }
            Some(surface.bvh.vertical_parity(end) == Parity::Inside)
        };
        // Ending in the material or on its boundary: too long if the axis already crossed the
        // cavity surface.
        let crossed = || {
            if self.cavity.bvh.segment_meets(entry, end) {
                OpeningIssue::Overshoot
            } else {
                OpeningIssue::Shallow
            }
        };
        match (inside(&self.source), inside(&self.cavity)) {
            (Some(false), _) => OpeningIssue::MissedModel,
            (None, _) | (Some(true), Some(false)) => crossed(),
            (Some(true), _) => OpeningIssue::InsufficientClearance,
        }
    }
}

fn bounds(points: &[Point3]) -> [[f64; 2]; 3] {
    std::array::from_fn(|i| {
        points
            .iter()
            .fold([f64::INFINITY, f64::NEG_INFINITY], |b, p| {
                [b[0].min(p.coordinates()[i]), b[1].max(p.coordinates()[i])]
            })
    })
}

/// Concatenate the separated cutter prisms, in plan order.
pub(super) fn assemble(
    cutters: &[TriangleMesh],
    cancel: &mut Cancellation<'_>,
) -> Result<TriangleMesh, DrainageError> {
    let nv = cutters
        .iter()
        .try_fold(0usize, |n, m| n.checked_add(m.vertices().len()))
        .ok_or_else(|| resource("cutter vertex count"))?;
    let nf = cutters
        .iter()
        .try_fold(0usize, |n, m| n.checked_add(m.triangles().len()))
        .ok_or_else(|| resource("cutter triangle count"))?;
    let mut vertices = Vec::new();
    let mut triangles = Vec::new();
    vertices
        .try_reserve_exact(nv)
        .map_err(|_| resource("cutter vertices"))?;
    triangles
        .try_reserve_exact(nf)
        .map_err(|_| resource("cutter triangles"))?;
    for mesh in cutters {
        let offset = u32::try_from(vertices.len()).map_err(|_| resource("cutter vertex index"))?;
        vertices.extend_from_slice(mesh.vertices());
        for (index, &face) in mesh.triangles().iter().enumerate() {
            cancel.poll(Checkpoint::DrainageWork, index)?;
            let mut shifted = [0; 3];
            for k in 0..3 {
                shifted[k] = face[k]
                    .checked_add(offset)
                    .ok_or_else(|| resource("cutter vertex index"))?;
            }
            triangles.push(shifted);
        }
    }
    Ok(TriangleMesh::try_new_with_cancellation(
        vertices, triangles, cancel,
    )?)
}

#[cfg(test)]
mod tests {
    use super::{convex_hull, ObliquePrism, Prism};

    #[test]
    fn oblique_sections_are_the_convex_section_of_the_prism() {
        // A sqrt(2) x 1 rectangle normal to (1, 0, 1), extruded along it.
        let entry = vec![
            [0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [-1.0, 1.0, 1.0],
            [-1.0, 0.0, 1.0],
        ];
        let end: Vec<[f64; 3]> = entry
            .iter()
            .map(|p| [p[0] + 4.0, p[1], p[2] + 4.0])
            .collect();
        let prism = ObliquePrism { entry, end };
        // At Z 2 the plane crosses four lateral edges: sqrt(2) / cos 45 = 2 mm by 1 mm.
        let ring = prism.section(2.0).unwrap();
        let area: f64 = (0..ring.len())
            .map(|k| {
                let (a, b) = (ring[k], ring[(k + 1) % ring.len()]);
                a[0] * b[1] - a[1] * b[0]
            })
            .sum::<f64>()
            / 2.0;
        assert_eq!(ring, [[0.0, 0.0], [2.0, 0.0], [2.0, 1.0], [0.0, 1.0]]);
        assert!((area - 2.0).abs() < 1e-12, "{area}");
        assert!(prism.section(-1.0).is_none());
        assert!(prism.section(6.0).is_none());
        assert_eq!(convex_hull(vec![[0.0, 0.0], [1.0, 0.0], [2.0, 0.0]]), None);
        assert_eq!(
            convex_hull(vec![
                [1.0, 1.0],
                [0.0, 0.0],
                [1.0, 0.0],
                [0.0, 1.0],
                [0.5, 0.5]
            ]),
            Some(vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]])
        );
    }

    fn square() -> Vec<[f64; 2]> {
        vec![[0.0, 0.0], [2.0, 0.0], [2.0, 2.0], [0.0, 2.0]]
    }

    #[test]
    fn vertical_prisms_cut_only_strictly_between_their_caps() {
        let prism = Prism {
            axis: 2,
            low: 1.0,
            high: 3.0,
            ring: square(),
        };
        assert_eq!(prism.section(2.0), Some(square()));
        assert_eq!(prism.section(1.0), None);
        assert_eq!(prism.section(3.0), None);
        assert_eq!(prism.section(0.5), None);
    }

    #[test]
    fn horizontal_prisms_cut_axial_rectangles_and_ignore_tangent_planes() {
        // X prism over x in [5, 9]; ring in (y, z): a diamond with vertices at z 0 and z 2.
        let diamond = vec![[1.0, 0.0], [2.0, 1.0], [1.0, 2.0], [0.0, 1.0]];
        let prism = Prism {
            axis: 0,
            low: 5.0,
            high: 9.0,
            ring: diamond.clone(),
        };
        assert_eq!(
            prism.section(1.0),
            Some(vec![[5.0, 0.0], [9.0, 0.0], [9.0, 2.0], [5.0, 2.0]])
        );
        assert_eq!(
            prism.section(0.5),
            Some(vec![[5.0, 0.5], [9.0, 0.5], [9.0, 1.5], [5.0, 1.5]])
        );
        // Planes through the top or bottom vertex touch only an edge: nothing is removed.
        assert_eq!(prism.section(2.0), None);
        assert_eq!(prism.section(0.0), None);
        assert_eq!(prism.section(3.0), None);
        // Y prism over y in [5, 9]; ring in (z, x).
        let prism = Prism {
            axis: 1,
            low: 5.0,
            high: 9.0,
            ring: diamond,
        };
        assert_eq!(
            prism.section(1.0),
            Some(vec![[0.0, 5.0], [2.0, 5.0], [2.0, 9.0], [0.0, 9.0]])
        );
    }
}
