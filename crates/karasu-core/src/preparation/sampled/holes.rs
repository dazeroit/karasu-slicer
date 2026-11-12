//! Drain holes as cuts the user places freely (TD-089, M6.4c c2).
//!
//! Unlike certified drainage ([`super::drain_model_sampled`]), nothing about a hole's path is
//! refused: a hole may start on the outer surface or on the cavity's surface, pass through the
//! whole object, overlap other holes, or be cut into a model that is not hollowed. What a hole
//! does is reported as [`HoleFindings`] instead:
//!
//! - whether it opens to the outside, and on how many sides;
//! - whether it reaches a cavity, and which regions;
//! - whether it ends in material.
//!
//! Findings come from the hole's axis and eight lines on its side. Along each line the crossings
//! with the outer and cavity surfaces are located by bisection on the exact segment test, and
//! every stretch between them is classified (outside, material, void) by exact parity at its
//! middle. A feature thinner than the bisection tolerance on all nine lines can be missed; the
//! findings are advice, never a certificate.
//!
//! The cuts ([`HoleCuts`]) are subtracted from every layer as the union of their exact analytic
//! sections, so overlapping holes remove their union. They are indexed by height, so a layer
//! costs only the holes that cross it.
use super::{
    distance::{closest_point_on_triangle, triangle_normal},
    drain::{
        assemble, build_cut, cavity_regions, innermost, resource, unsupported, CavityRegion, Cut,
        ObliquePrism, Prism, Surface, SURFACE_BYTES_PER_FACE,
    },
    hole_tangents, tilted_axis, HoleFrame, SampledHollowedModel,
};
use crate::{
    cancellation::{Cancellation, Checkpoint},
    mesh::TriangleMesh,
    preparation::{
        drainage::{cylinder::is_cardinal, DrainageError},
        HoleSpec, SurfaceHoleSpec,
    },
    raster::LayerCut,
    slicing::schedule::LayerPlane,
};

/// Default bound on the holes of one object. It exists because every work must be bounded
/// (`AGENTS.md`), not as a product limit: a layer only pays for the holes that cross it.
pub const MAX_HOLES: usize = 1024;

/// Lines on the side of a hole sampled for its findings, besides its axis.
const SIDE_LINES: usize = 8;

/// The surface a hole is anchored on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HoleSurface {
    /// The model's outer surface: the hole is cut inward, from outside.
    Outer,
    /// The cavity's surface of a hollowed model: the hole is cut through the wall from inside,
    /// towards the outside.
    Cavity,
}

/// A hole anchored on a surface and resolved into a cut ([`HoleProbe::resolve`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ResolvedHole {
    /// The cut: entry on the free side of the anchor, direction into the material, length.
    pub spec: HoleSpec,
    /// Where it opens: the anchor's foot and the unit normal towards the free side (the
    /// exterior for [`HoleSurface::Outer`], the void for [`HoleSurface::Cavity`]), from which
    /// its tilt is measured.
    pub frame: HoleFrame,
}

/// What a hole does, from its axis and side lines (see the module documentation).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)] // Independent findings, each a yes or no.
pub struct HoleFindings {
    /// Some sampled line has a stretch outside the model next to model or void: the hole opens
    /// to the outside and can drain.
    pub opens_outside: bool,
    /// The axis leaves the model to the outside more than once: the hole passes through the
    /// object and opens on both sides.
    pub through: bool,
    /// Some sampled line crosses a cavity.
    pub reaches_cavity: bool,
    /// The axis ends inside material, for example in the opposite wall.
    pub ends_in_material: bool,
    /// The cavity regions the sampled lines cross, sorted and distinct (indexes into
    /// [`HoleProbe::regions`]).
    pub regions: Vec<usize>,
}

/// Where a point of a sampled line lies.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Place {
    Outside,
    Material,
    /// Inside the cavity region with this index.
    Void(usize),
    /// On a surface at every probe of the stretch: not classified.
    Unknown,
}

struct CavityIndex {
    surface: Surface,
    region_of: Vec<usize>,
    regions: Vec<CavityRegion>,
}

/// The memory a [`HoleProbe`] may take while it is built (TD-094).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProbeLimits {
    /// Requested bytes for building one index, excluding allocator overhead.
    pub max_payload_bytes: u64,
}

impl Default for ProbeLimits {
    fn default() -> Self {
        Self {
            max_payload_bytes: super::SampledHollowLimits::default().max_payload_bytes,
        }
    }
}

impl ProbeLimits {
    /// Peak bytes one indexed face needs while an index is built.
    pub const BYTES_PER_FACE: u64 = SURFACE_BYTES_PER_FACE as u64;

    /// The most faces one index may have within these limits.
    #[must_use]
    pub fn max_faces(self) -> u64 {
        self.max_payload_bytes / Self::BYTES_PER_FACE
    }

    /// Admit an index over `faces` triangles.
    fn admit(self, faces: usize) -> Result<(), DrainageError> {
        if u64::try_from(faces).map_or(true, |faces| faces > self.max_faces()) {
            return Err(resource("hole probe index payload"));
        }
        Ok(())
    }
}

/// Resolves holes on a model and reports their findings (TD-089). Building it indexes the outer
/// surface and, for a hollowed model, the cavity once; each resolution or findings query is then
/// a few index queries, fast enough while the pointer moves.
pub struct HoleProbe {
    source: Surface,
    cavity: Option<CavityIndex>,
}

impl std::fmt::Debug for HoleProbe {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HoleProbe")
            .field("cavity", &self.cavity.is_some())
            .finish_non_exhaustive()
    }
}

impl HoleProbe {
    /// Bytes the probe keeps, excluding allocator overhead: what keeping it in a cache costs.
    #[must_use]
    pub fn retained_bytes(&self) -> u64 {
        let cavity = self.cavity.as_ref().map_or(0, |c| {
            c.surface.retained_bytes()
                + c.region_of.capacity() * std::mem::size_of::<usize>()
                + c.regions.capacity() * std::mem::size_of::<CavityRegion>()
        });
        u64::try_from(self.source.retained_bytes() + cavity).unwrap_or(u64::MAX)
    }

    /// Index a model that is not hollowed: `mesh` is its surface, in the coordinates holes are
    /// given in (world millimetres for a placed model).
    ///
    /// # Errors
    /// Resource exhaustion and cancellation.
    pub fn solid(
        mesh: &TriangleMesh,
        limits: ProbeLimits,
        cancel: &mut Cancellation<'_>,
    ) -> Result<Self, DrainageError> {
        Self::admit_solid(mesh, limits)?;
        Ok(Self {
            source: Surface::build(mesh, mesh.triangles(), cancel)?,
            cavity: None,
        })
    }

    /// Whether [`Self::solid`] admits `mesh`, decided from its size alone, without building
    /// anything: a preparation can say before any hole is placed that none can be.
    ///
    /// # Errors
    /// [`DrainageError::Resource`] when the index would exceed the payload cap.
    pub fn admit_solid(mesh: &TriangleMesh, limits: ProbeLimits) -> Result<(), DrainageError> {
        limits.admit(mesh.triangles().len())
    }

    /// Whether [`Self::hollow`] admits `model`, decided from its size alone, as
    /// [`Self::admit_solid`].
    ///
    /// # Errors
    /// [`DrainageError::Resource`] when an index would exceed the payload cap.
    pub fn admit_hollow(
        model: &SampledHollowedModel,
        limits: ProbeLimits,
    ) -> Result<(), DrainageError> {
        // The two indexes are built one after the other, so each is admitted on its own: the
        // peak is one index's construction plus the other's retained part.
        limits.admit(model.exterior_faces())?;
        limits.admit(model.cavity().triangles().len())
    }

    /// Index a hollowed model: its shell's exterior and its cavity, in the shell's coordinates.
    ///
    /// # Errors
    /// Resource exhaustion and cancellation.
    pub fn hollow(
        model: &SampledHollowedModel,
        limits: ProbeLimits,
        cancel: &mut Cancellation<'_>,
    ) -> Result<Self, DrainageError> {
        Self::admit_hollow(model, limits)?;
        let shell = model.mesh();
        let source = Surface::build(shell, &shell.triangles()[..model.exterior_faces()], cancel)?;
        let cavity_mesh = model.cavity();
        let surface = Surface::build(cavity_mesh, cavity_mesh.triangles(), cancel)?;
        let (regions, region_of) = cavity_regions(&surface, cancel)?;
        Ok(Self {
            source,
            cavity: Some(CavityIndex {
                surface,
                region_of,
                regions,
            }),
        })
    }

    /// The cavity regions of a hollowed model, with no hole credited (see
    /// [`Self::drained_regions`]); empty for a solid model.
    #[must_use]
    pub fn regions(&self) -> &[CavityRegion] {
        self.cavity.as_ref().map_or(&[], |c| &c.regions)
    }

    /// Resolve a hole anchored at the point of `surface` nearest `request.surface_mm`.
    ///
    /// - **Foot:** the nearest point of the chosen surface.
    /// - **Free side:** the area-weighted mean normal of the surface's faces within the hole's
    ///   radius of the foot, turned towards the exterior ([`HoleSurface::Outer`]) or the void
    ///   ([`HoleSurface::Cavity`]).
    /// - **Axis:** opposite to it, into the material, or tilted from it ([`tilted_axis`]).
    /// - **Entry:** `outside_mm` before the foot on the free side.
    /// - **Length:** `outside_mm` plus the fixed `depth_mm`. Without a depth, an outer hole of a
    ///   hollowed model reaches `inside_mm` past the first cavity crossing (the command line's
    ///   surface holes); any other hole needs a depth.
    ///
    /// # Errors
    /// A cavity anchor on a model that is not hollowed, a missing depth that cannot be found, an
    /// undefined normal, non-finite or non-positive parameters, and cancellation.
    pub fn resolve(
        &self,
        request: SurfaceHoleSpec,
        surface: HoleSurface,
        cancel: &mut Cancellation<'_>,
    ) -> Result<ResolvedHole, DrainageError> {
        let positive = |v: f64| v.is_finite() && v > 0.0;
        if !positive(request.diameter_mm)
            || !request.outside_mm.is_finite()
            || request.outside_mm < 0.0
            || request.depth_mm.is_some_and(|d| !positive(d))
            || request.surface_mm.iter().any(|v| !v.is_finite())
        {
            return Err(unsupported(
                "a hole needs a finite point, a positive diameter and depth, and a finite margin",
            ));
        }
        let (anchor, toward_free) = match (surface, &self.cavity) {
            (HoleSurface::Outer, _) => (&self.source, 1.0),
            // The cavity surface faces the material: the void is behind it.
            (HoleSurface::Cavity, Some(cavity)) => (&cavity.surface, -1.0),
            (HoleSurface::Cavity, None) => {
                return Err(unsupported("a hole from the cavity needs a hollowed model"))
            }
        };
        let (_, slot) = anchor.bvh.nearest(request.surface_mm, 0);
        let nearest = anchor
            .bvh
            .triangle(slot)
            .ok_or_else(|| unsupported("the model has no surface"))?;
        let foot = closest_point_on_triangle(request.surface_mm, nearest);
        let radius = request.diameter_mm / 2.0;
        // The faces within the radius, summed in index order.
        let mut sum = [0.0; 3];
        for (index, (_, slot)) in anchor
            .bvh
            .within(foot, radius * radius)
            .into_iter()
            .enumerate()
        {
            cancel.poll(Checkpoint::DrainageWork, index)?;
            if let Some(triangle) = anchor.bvh.triangle(slot) {
                let n = triangle_normal(triangle);
                for k in 0..3 {
                    sum[k] += n[k];
                }
            }
        }
        let norm = sum.iter().map(|c| c * c).sum::<f64>().sqrt();
        if !(norm.is_finite() && norm > 0.0) {
            return Err(unsupported("the surface normal at the hole is undefined"));
        }
        let outward = sum.map(|c| toward_free * c / norm);
        // `0 - c` keeps zero components positive (no `-0` in reports or records).
        let axis = request.tilt.map_or_else(
            || outward.map(|c| 0.0 - c),
            |tilt| tilted_axis(outward, tilt),
        );
        let entry: [f64; 3] = std::array::from_fn(|k| foot[k] - request.outside_mm * axis[k]);
        let depth = match (request.depth_mm, surface, &self.cavity) {
            (Some(depth), _, _) => depth,
            (None, HoleSurface::Outer, Some(cavity)) => {
                self.first_crossing(&cavity.surface, foot, axis, cancel)? + request.inside_mm
            }
            _ => return Err(unsupported("this hole needs a depth")),
        };
        Ok(ResolvedHole {
            spec: HoleSpec {
                entry_mm: entry,
                direction: axis,
                length_mm: request.outside_mm + depth,
                diameter_mm: request.diameter_mm,
            },
            frame: HoleFrame {
                foot_mm: foot,
                outward,
            },
        })
    }

    /// Distance from `foot` along `axis` to the first crossing of `surface`.
    fn first_crossing(
        &self,
        surface: &Surface,
        foot: [f64; 3],
        axis: [f64; 3],
        cancel: &mut Cancellation<'_>,
    ) -> Result<f64, DrainageError> {
        let span = (0..3)
            .map(|k| self.source.max[k] - self.source.min[k])
            .fold(0.0, f64::max)
            * 2.0
            + 1.0;
        let along = |t: f64| -> [f64; 3] { std::array::from_fn(|k| foot[k] + t * axis[k]) };
        if !surface.bvh.segment_meets(foot, along(span)) {
            return Err(unsupported("the hole axis does not reach the cavity"));
        }
        let (mut low, mut high) = (0.0, span);
        for _ in 0..80 {
            cancel.check(Checkpoint::DrainageWork)?;
            let middle = f64::midpoint(low, high);
            if middle <= low || middle >= high {
                break;
            }
            if surface.bvh.segment_meets(foot, along(middle)) {
                high = middle;
            } else {
                low = middle;
            }
        }
        Ok(high)
    }

    /// What the cut `spec` does (see [`HoleFindings`]).
    ///
    /// # Errors
    /// An invalid direction, resource exhaustion and cancellation.
    pub fn findings(
        &self,
        spec: &HoleSpec,
        cancel: &mut Cancellation<'_>,
    ) -> Result<HoleFindings, DrainageError> {
        let axis = spec.unit_direction().map_err(DrainageError::Plan)?;
        let (u, v) = hole_tangents(axis);
        let radius = spec.diameter_mm / 2.0;
        let mut findings = HoleFindings::default();
        for line in 0..=SIDE_LINES {
            cancel.check(Checkpoint::DrainageWork)?;
            let offset: [f64; 3] = if line == 0 {
                [0.0; 3]
            } else {
                #[allow(clippy::cast_precision_loss)] // Eight lines.
                let angle = std::f64::consts::TAU * (line - 1) as f64 / SIDE_LINES as f64;
                let (sin, cos) = angle.sin_cos();
                std::array::from_fn(|k| radius * (cos * u[k] + sin * v[k]))
            };
            let start: [f64; 3] = std::array::from_fn(|k| spec.entry_mm[k] + offset[k]);
            let end: [f64; 3] = std::array::from_fn(|k| start[k] + spec.length_mm * axis[k]);
            let places = self.classify(start, end, cancel)?;
            let mut outside_runs = 0;
            for (k, &place) in places.iter().enumerate() {
                match place {
                    Place::Outside => {
                        if k == 0 || places[k - 1] != Place::Outside {
                            outside_runs += 1;
                        }
                        let next_to_body = (k > 0 && places[k - 1] != Place::Outside)
                            || places.get(k + 1).is_some_and(|&p| p != Place::Outside);
                        findings.opens_outside |= next_to_body;
                    }
                    Place::Void(region) => {
                        findings.reaches_cavity = true;
                        findings.regions.push(region);
                    }
                    Place::Material | Place::Unknown => {}
                }
            }
            if line == 0 {
                findings.through = outside_runs >= 2;
                findings.ends_in_material = places.last() == Some(&Place::Material);
            }
        }
        findings.regions.sort_unstable();
        findings.regions.dedup();
        Ok(findings)
    }

    /// The cavity regions, each credited with the holes (1-based, in `findings` order) that
    /// open to the outside and cross it. A region with none is enclosed.
    #[must_use]
    pub fn drained_regions(&self, findings: &[HoleFindings]) -> Vec<CavityRegion> {
        let mut regions = self.regions().to_vec();
        for (id, hole) in (1u64..).zip(findings) {
            if !hole.opens_outside {
                continue;
            }
            for &region in &hole.regions {
                if let Some(r) = regions.get_mut(region) {
                    r.drained_by.push(id);
                }
            }
        }
        regions
    }

    /// The places along the segment `start`..`end`, one per stretch between surface crossings,
    /// in order.
    fn classify(
        &self,
        start: [f64; 3],
        end: [f64; 3],
        cancel: &mut Cancellation<'_>,
    ) -> Result<Vec<Place>, DrainageError> {
        let length = (0..3)
            .map(|k| (end[k] - start[k]).powi(2))
            .sum::<f64>()
            .sqrt();
        let tolerance = (length * 1e-6).max(1e-4);
        let at =
            |t: f64| -> [f64; 3] { std::array::from_fn(|k| start[k] + t * (end[k] - start[k])) };
        let mut breaks = vec![0.0, 1.0];
        let mut surfaces = vec![&self.source];
        if let Some(cavity) = &self.cavity {
            surfaces.push(&cavity.surface);
        }
        for surface in surfaces {
            crossings(
                surface,
                &at,
                0.0,
                1.0,
                tolerance / length.max(f64::MIN_POSITIVE),
                &mut breaks,
                cancel,
            )?;
        }
        breaks.sort_by(f64::total_cmp);
        breaks.dedup();
        let mut places: Vec<Place> = Vec::new();
        for pair in breaks.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            if b <= a {
                continue;
            }
            let place = self.place_in(&at, a, b);
            if places.last() != Some(&place) {
                places.push(place);
            }
        }
        Ok(places)
    }

    /// The place of the stretch `a`..`b`: probed at its middle, then at its thirds when the
    /// middle lies on a surface.
    ///
    /// Inside means a positive winding number, as slicing's union fill decides material: bodies
    /// that touch or overlap, common in sculpted models, count as material, not as outside, as
    /// an even-odd test would make their overlap.
    fn place_in(&self, at: &dyn Fn(f64) -> [f64; 3], a: f64, b: f64) -> Place {
        for fraction in [0.5, 1.0 / 3.0, 2.0 / 3.0] {
            let p = at(a + fraction * (b - a));
            let Some(winding) = self.source.bvh.vertical_winding(p) else {
                continue;
            };
            if winding <= 0 {
                return Place::Outside;
            }
            let Some(cavity) = &self.cavity else {
                return Place::Material;
            };
            // The cavity surface faces the material: it winds once around the void.
            let Some(void) = cavity.surface.bvh.vertical_winding(p) else {
                continue;
            };
            if void <= 0 {
                return Place::Material;
            }
            let Ok(odd) = cavity.surface.parity(p, None, None) else {
                continue;
            };
            return innermost(&cavity.surface, &odd)
                .map_or(Place::Unknown, |k| Place::Void(cavity.region_of[k]));
        }
        Place::Unknown
    }
}

/// Parameters in `a`..`b` (of the segment `at`) where it crosses `surface`, located to `step`
/// by bisection on the exact segment test, appended to `out`.
fn crossings(
    surface: &Surface,
    at: &dyn Fn(f64) -> [f64; 3],
    a: f64,
    b: f64,
    step: f64,
    out: &mut Vec<f64>,
    cancel: &mut Cancellation<'_>,
) -> Result<(), DrainageError> {
    let mut pending = vec![(a, b)];
    while let Some((a, b)) = pending.pop() {
        cancel.poll(Checkpoint::DrainageWork, pending.len())?;
        if !surface.bvh.segment_meets(at(a), at(b)) {
            continue;
        }
        if b - a <= step {
            out.push(f64::midpoint(a, b));
            continue;
        }
        let middle = f64::midpoint(a, b);
        if middle <= a || middle >= b {
            out.push(middle);
            continue;
        }
        pending.push((middle, b));
        pending.push((a, middle));
    }
    Ok(())
}

/// The cuts of an object's holes, subtracted from every layer as the union of their exact
/// sections (TD-089).
#[derive(Debug)]
pub struct HoleCuts {
    prisms: Vec<Cut>,
    /// Per prism (sorted by its lowest height): lowest and highest height and prism index.
    heights: Vec<(f64, f64, usize)>,
    cutters: TriangleMesh,
}

impl HoleCuts {
    /// Build the polygonal cylinders of `holes` within `distance_error_mm` and at most
    /// `max_segments` sides each.
    ///
    /// # Errors
    /// More than `max_holes` holes, an invalid hole (direction, length or diameter), resource
    /// exhaustion and cancellation. Nothing about a hole's path is refused.
    pub fn build(
        holes: &[HoleSpec],
        distance_error_mm: f64,
        max_holes: usize,
        max_segments: usize,
        cancel: &mut Cancellation<'_>,
    ) -> Result<Self, DrainageError> {
        if holes.len() > max_holes || !(8..=4096).contains(&max_segments) {
            return Err(resource("hole or segment limit"));
        }
        let mut prisms = Vec::new();
        let mut heights = Vec::new();
        let mut meshes = Vec::new();
        for (index, &spec) in holes.iter().enumerate() {
            cancel.check(Checkpoint::DrainageWork)?;
            let id = u64::try_from(index + 1).map_err(|_| resource("hole count"))?;
            let cut = build_cut(spec, distance_error_mm, max_segments, id, cancel)?;
            let axis = spec.unit_direction().map_err(DrainageError::Plan)?;
            let prism = if is_cardinal(spec.direction) {
                Cut::Cardinal(Prism::from_cutter(
                    &cut,
                    (0..3).find(|&k| axis[k] != 0.0).unwrap_or(2),
                ))
            } else {
                Cut::Oblique(ObliquePrism::from_cutter(&cut))
            };
            let (low, high) = cut
                .mesh
                .vertices()
                .iter()
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| {
                    (lo.min(p.z), hi.max(p.z))
                });
            heights.push((low, high, prisms.len()));
            prisms.push(prism);
            meshes.push(cut.mesh);
        }
        heights.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.2.cmp(&b.2)));
        Ok(Self {
            prisms,
            heights,
            cutters: assemble(&meshes, cancel)?,
        })
    }

    /// The cylinders as built, in hole order, for display.
    #[must_use]
    pub fn cutters(&self) -> &TriangleMesh {
        &self.cutters
    }

    /// Number of holes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.prisms.len()
    }

    /// Whether there are no holes.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.prisms.is_empty()
    }
}

impl LayerCut for HoleCuts {
    /// The sections of the holes that cross the plane, in hole order; overlapping sections are
    /// removed as their union.
    fn rings(&self, plane: LayerPlane) -> Vec<Vec<[f64; 2]>> {
        let z = plane.z_mm;
        // Holes starting above the plane are skipped without looking at their sections.
        let reach = self.heights.partition_point(|&(low, _, _)| low <= z);
        let mut crossing: Vec<usize> = self.heights[..reach]
            .iter()
            .filter(|&&(_, high, _)| high >= z)
            .map(|&(_, _, index)| index)
            .collect();
        crossing.sort_unstable();
        crossing
            .into_iter()
            .filter_map(|index| self.prisms[index].section(z))
            .collect()
    }
}
