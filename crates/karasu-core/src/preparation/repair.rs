//! Source repairs as an explicit preparation step (TD-054): mesh repairs, hole filling and
//! outward orientation, applied to a derived mesh in a fixed order and reported.
//!
//! Order: welding (R1) and degenerate faces (R2), when asked; duplicate collapse, which also
//! cancels the folds welding leaves, and consistent orientation (`mesh::repair`); then crack
//! closing and hole filling (closed components appear only once holes are capped); then `outward`. Every step is
//! deterministic and idempotent: repairing a repaired mesh changes nothing, so a preparation plan
//! that records the repair reproduces its result from the bound source.

pub use super::sampled::fill::{
    FillError, FillHoles, FillReport, HoleRefusal, RefusedHole, DEFAULT_MAX_HOLE_EDGES,
    MAX_EXACT_CAP_EDGES, MAX_REFUSED_DETAILS,
};
mod degenerate;
mod fins;
mod weld;

pub use degenerate::{DegenerateReport, MAX_PASSES};
pub use fins::{NonManifold, NonManifoldReport};
pub use weld::WeldReport;

pub use super::sampled::intersections::{
    self_intersections, IntersectionError, IntersectionReport, MAX_INTERSECTION_SAMPLES,
};
pub use super::sampled::shells::{ShellReport, Shells, VisibilityReport};
pub use super::sampled::{RebuildReport, DEFAULT_REBUILD_MM, MAX_REBUILD_MM, MIN_REBUILD_MM};
use crate::{
    cancellation::{Cancellation, Cancelled},
    mesh::{
        repair::{record, RepairChange, RepairPolicy, RepairReport, RepairSample},
        topology::{TopologyError, TopologyLimits},
        TriangleMesh,
    },
};
use std::{error::Error, fmt};

/// Which source repairs to apply.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[allow(clippy::struct_excessive_bools)] // Independent, recorded switches; not a state machine.
pub struct RepairSpec {
    /// Collapse duplicate faces to their net orientation.
    pub duplicates: bool,
    /// Make orientation consistent within each component.
    pub orientation: bool,
    /// Turn outermost inside-out closed components outward (after hole filling).
    pub outward: bool,
    /// Cap simple holes, or `None` to leave them open.
    pub fill_holes: Option<FillHoles>,
    /// Accept holes that filling refuses; otherwise any refused hole fails the repair.
    pub allow_unfilled: bool,
    /// Weld vertices closer than this distance, in mm, before every other repair (stage R1);
    /// `Some(0.0)` merges vertices at equal positions only. See [`WeldReport`].
    pub weld_mm: Option<f64>,
    /// Remove degenerate faces after welding (stage R2): faces with a repeated vertex always;
    /// with a positive tolerance, in mm, also collapse edges no longer than it and flip faces whose
    /// apex lies within it of their longest edge. See [`DegenerateReport`].
    pub degenerate_mm: Option<f64>,
    /// With hole filling, also close cracks whose rim lies within this distance, in mm, of a line
    /// (stage R4); without it only exactly collinear cracks close.
    pub crack_mm: Option<f64>,
    /// Repair unbalanced non-manifold edges after duplicate collapse and orientation (stage R5).
    /// See [`NonManifoldReport`].
    pub non_manifold: Option<NonManifold>,
    /// Turn open parts to face where they are visible from outside, before hole filling
    /// (stage R6). See [`VisibilityReport`].
    pub visibility: bool,
    /// Remove small and inner shells after hole filling (stage R8). See [`ShellReport`].
    pub shells: Option<Shells>,
    /// Rebuild the model as a solid at this resolution, in mm, after every other repair
    /// (stage R9, the last resort). See [`RebuildReport`].
    pub rebuild_mm: Option<f64>,
    /// Accept a model whose import left out facets with a non-finite coordinate (TD-092 r9).
    /// Those facets have no position, so nothing can be made of them; the import drops them
    /// ([`crate::mesh::stl::NonFinitePolicy::Drop`]) and records them, and without this switch a
    /// job refuses such a model as the import would. [`repair_source`] does not use it, and
    /// [`Self::is_empty`] ignores it.
    pub non_finite: bool,
}

/// The largest tolerance a repair accepts, in mm: tolerances close defects far below a printer's
/// pixel, not features.
pub const MAX_TOLERANCE_MM: f64 = 1.0;

/// The tolerance of the `Standard` preset, in mm, as CHITUBOX snaps vertices to
/// a 1 µm grid and zips cracks within 0.001 (research CHI-MR-04, CHI-MR-11).
pub const STANDARD_TOLERANCE_MM: f64 = 0.001;

impl RepairSpec {
    /// Whether the spec changes anything at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        !(self.duplicates
            || self.orientation
            || self.outward
            || self.fill_holes.is_some()
            || self.weld_mm.is_some()
            || self.degenerate_mm.is_some()
            || self.non_manifold.is_some()
            || self.visibility
            || self.shells.is_some()
            || self.rebuild_mm.is_some())
    }

    /// Check the tolerances: finite, from 0 to [`MAX_TOLERANCE_MM`], and cracks only with hole
    /// filling. Returns what is wrong.
    ///
    /// # Errors
    /// The first tolerance out of range, or a crack tolerance without hole filling.
    pub fn check(&self) -> Result<(), &'static str> {
        for (value, what) in [
            (self.weld_mm, "weld tolerance"),
            (self.degenerate_mm, "degenerate face tolerance"),
            (self.crack_mm, "crack tolerance"),
        ] {
            if value.is_some_and(|t| !(0.0..=MAX_TOLERANCE_MM).contains(&t)) {
                return Err(what);
            }
        }
        if self.crack_mm.is_some() && self.fill_holes.is_none() {
            return Err("crack tolerance without hole filling");
        }
        if self.shells.is_some_and(|s| {
            !(s.min_area_mm2.is_finite()
                && s.min_area_mm2 >= 0.0
                && s.min_volume_mm3.is_finite()
                && s.min_volume_mm3 >= 0.0)
        }) {
            return Err("shell bounds");
        }
        if self
            .rebuild_mm
            .is_some_and(|r| !(MIN_REBUILD_MM..=MAX_REBUILD_MM).contains(&r))
        {
            return Err("rebuild resolution");
        }
        Ok(())
    }
}

/// A named set of source repairs (M6.4d, TD-092). Each preset is a fixed [`RepairSpec`]; stages
/// with tolerances join the presets that use them as they are implemented.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RepairPreset {
    /// No repair: the model is diagnosed and used as it is.
    Off,
    /// Every repair that moves no vertex and needs no tolerance: facets without a position left
    /// out at import, welding of equal positions, removal of faces with a repeated vertex,
    /// duplicate collapse, consistent orientation, crack closing and hole filling with validated
    /// caps, then outward orientation. Holes that filling refuses stay open and are reported,
    /// rather than failing the repair.
    Exact,
    /// `Exact`, with welding, degenerate faces and cracks at [`STANDARD_TOLERANCE_MM`] (vertices
    /// move by at most that distance), and fins removed ([`NonManifold::Fins`]).
    Standard,
    /// `Standard`, with every face on an edge left non-manifold removed for hole filling to close
    /// ([`NonManifold::Aggressive`]), holes of any size, open parts turned by visibility and
    /// shells under 1 mm² removed.
    Aggressive,
    /// `Aggressive`, then the model rebuilt as a solid at [`DEFAULT_REBUILD_MM`] (stage R9): always
    /// closed and manifold when the earlier stages give it a winding number, within the reported
    /// deviation of the source. An interface derives the resolution from the printer.
    Rebuild,
}

impl RepairPreset {
    /// The repairs of the preset.
    #[must_use]
    pub const fn spec(self) -> RepairSpec {
        match self {
            Self::Off => RepairSpec {
                non_finite: false,
                duplicates: false,
                orientation: false,
                outward: false,
                fill_holes: None,
                allow_unfilled: false,
                weld_mm: None,
                degenerate_mm: None,
                crack_mm: None,
                non_manifold: None,
                visibility: false,
                shells: None,
                rebuild_mm: None,
            },
            Self::Exact => RepairSpec {
                non_finite: true,
                duplicates: true,
                orientation: true,
                outward: true,
                fill_holes: Some(FillHoles {
                    max_hole_edges: DEFAULT_MAX_HOLE_EDGES,
                    split_rims: true,
                    refine: false,
                }),
                allow_unfilled: true,
                weld_mm: Some(0.0),
                degenerate_mm: Some(0.0),
                crack_mm: None,
                non_manifold: None,
                visibility: false,
                shells: None,
                rebuild_mm: None,
            },
            Self::Standard => RepairSpec {
                weld_mm: Some(STANDARD_TOLERANCE_MM),
                degenerate_mm: Some(STANDARD_TOLERANCE_MM),
                crack_mm: Some(STANDARD_TOLERANCE_MM),
                non_manifold: Some(NonManifold::Fins),
                fill_holes: Some(FillHoles {
                    max_hole_edges: DEFAULT_MAX_HOLE_EDGES,
                    split_rims: true,
                    refine: true,
                }),
                ..Self::Exact.spec()
            },
            Self::Aggressive => RepairSpec {
                non_manifold: Some(NonManifold::Aggressive),
                visibility: true,
                shells: Some(Shells {
                    min_area_mm2: crate::mesh::topology::SMALL_COMPONENT_MM2,
                    min_volume_mm3: 0.0,
                    inner: false,
                }),
                fill_holes: Some(FillHoles {
                    max_hole_edges: usize::MAX,
                    split_rims: true,
                    refine: true,
                }),
                ..Self::Standard.spec()
            },
            Self::Rebuild => RepairSpec {
                rebuild_mm: Some(DEFAULT_REBUILD_MM),
                ..Self::Aggressive.spec()
            },
        }
    }

    /// The preset whose repairs are exactly `spec`, if any.
    #[must_use]
    pub fn of(spec: RepairSpec) -> Option<Self> {
        [
            Self::Off,
            Self::Exact,
            Self::Standard,
            Self::Aggressive,
            Self::Rebuild,
        ]
        .into_iter()
        .find(|preset| preset.spec() == spec)
    }
}

/// What the source repairs did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SourceRepairReport {
    /// Welding, when requested (stage R1).
    pub weld: Option<WeldReport>,
    /// Degenerate faces, when requested (stage R2).
    pub degenerate: Option<DegenerateReport>,
    /// Non-manifold edges, when requested (stage R5).
    pub non_manifold: Option<NonManifoldReport>,
    /// Open parts turned by visibility, when requested (stage R6).
    pub visibility: Option<VisibilityReport>,
    /// Small and inner shells, when requested (stage R8).
    pub shells: Option<ShellReport>,
    /// The rebuilt solid, when requested (stage R9).
    pub rebuild: Option<RebuildReport>,
    /// Where the rebuilt surface deviates most from the source, when rebuilt.
    pub rebuild_samples: Vec<RepairSample>,
    /// Duplicate and orientation repairs, including outward flips.
    pub mesh: RepairReport,
    /// Hole filling, when requested; it locates its own changes.
    pub fill: Option<FillReport>,
    /// Where duplicate collapse and the orientation repairs changed the model (or declined to):
    /// at most [`crate::mesh::repair::MAX_REPAIR_SAMPLES`] per kind, in source coordinates.
    pub samples: Vec<RepairSample>,
    /// Facets the import left out for a non-finite coordinate, accepted by
    /// [`RepairSpec::non_finite`]. Set by the caller that imported the model, never by
    /// [`repair_source`]; they have no position, so no sample locates them.
    pub non_finite: Option<crate::mesh::stl::DroppedFacets>,
}

impl SourceRepairReport {
    /// Every located change, mesh repairs first, then hole filling.
    pub fn all_samples(&self) -> impl Iterator<Item = &RepairSample> {
        self.weld
            .iter()
            .flat_map(|w| w.samples.iter())
            .chain(self.degenerate.iter().flat_map(|d| d.samples.iter()))
            .chain(&self.samples)
            .chain(self.non_manifold.iter().flat_map(|n| n.samples.iter()))
            .chain(self.visibility.iter().flat_map(|v| v.samples.iter()))
            .chain(self.fill.iter().flat_map(|f| f.samples.iter()))
            .chain(self.shells.iter().flat_map(|s| s.samples.iter()))
            .chain(&self.rebuild_samples)
    }

    /// Whether any face was removed, flipped or added.
    #[must_use]
    pub fn changed(&self) -> bool {
        self.weld.as_ref().is_some_and(|w| w.clusters > 0)
            || self
                .degenerate
                .as_ref()
                .is_some_and(|d| d.needles_removed + d.edges_collapsed + d.caps_flipped > 0)
            || self
                .non_manifold
                .as_ref()
                .is_some_and(|n| n.fins_removed + n.faces_cut > 0)
            || self
                .visibility
                .as_ref()
                .is_some_and(|v| v.parts_flipped > 0)
            || self
                .shells
                .as_ref()
                .is_some_and(|s| s.small_removed + s.inner_removed > 0)
            || self.rebuild.is_some()
            || self.mesh.changed()
            || self
                .fill
                .as_ref()
                .is_some_and(|f| f.holes_filled > 0 || f.cracks_closed > 0)
    }
}

/// Why source repair failed.
#[derive(Debug, PartialEq)]
#[non_exhaustive]
pub enum RepairError {
    /// Cooperative cancellation.
    Cancelled(Cancelled),
    /// Mesh repair or analysis failure.
    Topology(TopologyError),
    /// Hole filling failure.
    Fill(FillError),
    /// The spec is not valid: what is wrong.
    Invalid(&'static str),
    /// Rebuilding the solid failed: the hollowing core's reason.
    Rebuild(String),
    /// Hole filling left holes open and the spec does not accept that.
    UnfilledHoles {
        /// Holes left open.
        holes: usize,
        /// The first of them.
        first: RefusedHole,
    },
}

impl fmt::Display for RepairError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled(e) => e.fmt(f),
            Self::Topology(e) => write!(f, "mesh repair: {e}"),
            Self::Fill(e) => e.fmt(f),
            Self::Invalid(what) => write!(f, "invalid repair: {what}"),
            Self::Rebuild(e) => write!(f, "rebuilding the solid: {e}"),
            Self::UnfilledHoles { holes, first } => write!(
                f,
                "{holes} hole(s) could not be filled; the first, at vertex {} with {} boundary \\
                 edges: {:?}",
                first.first_vertex, first.edges, first.reason
            ),
        }
    }
}

impl From<Cancelled> for RepairError {
    fn from(cancelled: Cancelled) -> Self {
        Self::Cancelled(cancelled)
    }
}

impl Error for RepairError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Cancelled(e) => Some(e),
            Self::Topology(e) => Some(e),
            Self::Fill(e) => Some(e),
            Self::Invalid(_) | Self::Rebuild(_) | Self::UnfilledHoles { .. } => None,
        }
    }
}

fn merge(into: &mut RepairReport, more: &RepairReport) {
    into.dropped_duplicate_faces += more.dropped_duplicate_faces;
    into.cancelled_duplicate_groups += more.cancelled_duplicate_groups;
    into.flipped_faces += more.flipped_faces;
    into.flipped_area_mm2 += more.flipped_area_mm2;
    into.reoriented_components += more.reoriented_components;
    into.non_orientable_components = more.non_orientable_components;
    into.outward_flipped_components += more.outward_flipped_components;
    into.nested_reversed_components = more.nested_reversed_components;
    into.ambiguous_volume_components = more.ambiguous_volume_components;
}

/// Apply `spec` to `mesh`. Returns the derived mesh (`None` when nothing changed) and the report.
///
/// # Errors
/// Cancellation, analysis and allocation failures, and [`RepairError::UnfilledHoles`] when holes
/// stay open without `allow_unfilled`.
#[allow(clippy::too_many_lines)] // The stages in their order, each one a call and its report.
pub fn repair_source(
    mesh: &TriangleMesh,
    spec: RepairSpec,
    limits: TopologyLimits,
    cancel: &mut Cancellation<'_>,
) -> Result<(Option<TriangleMesh>, SourceRepairReport), RepairError> {
    spec.check().map_err(RepairError::Invalid)?;
    let mut report = SourceRepairReport::default();
    let mut current: Option<TriangleMesh> = None;
    if let Some(tolerance) = spec.weld_mm {
        let (welded, weld_report) = weld::weld(mesh, tolerance, cancel)?;
        if welded.is_some() {
            current = welded;
        }
        report.weld = Some(weld_report);
    }
    if let Some(tolerance) = spec.degenerate_mm {
        let source = current.as_ref().unwrap_or(mesh);
        let (cleaned, degenerate_report) =
            degenerate::repair_degenerate(source, tolerance, cancel)?;
        if cleaned.is_some() {
            current = cleaned;
        }
        report.degenerate = Some(degenerate_report);
    }
    let first = RepairPolicy {
        duplicates: spec.duplicates,
        orientation: spec.orientation,
        outward: false,
    };
    let (repaired, mesh_report, samples) = current
        .as_ref()
        .unwrap_or(mesh)
        .repair_with_samples(first, limits, cancel)
        .map_err(RepairError::Topology)?;
    if repaired.is_some() {
        current = repaired;
    }
    report.mesh = mesh_report;
    report.samples = samples;
    if let Some(mode) = spec.non_manifold {
        let source = current.as_ref().unwrap_or(mesh);
        let (repaired, non_manifold) = fins::repair_non_manifold(source, mode, cancel)?;
        if repaired.is_some() {
            current = repaired;
        }
        report.non_manifold = Some(non_manifold);
    }
    if spec.visibility {
        let source = current.as_ref().unwrap_or(mesh);
        let (turned, visibility) = super::sampled::shells::orient_by_visibility(source, cancel)?;
        if turned.is_some() {
            current = turned;
        }
        report.visibility = Some(visibility);
    }
    if let Some(fill) = spec.fill_holes {
        let source = current.as_ref().unwrap_or(mesh);
        let crack = spec.crack_mm.unwrap_or(0.0);
        let (filled, fill_report) =
            super::sampled::fill::fill_holes(source, Some(fill), crack, limits, cancel).map_err(
                |e| match e {
                    FillError::Cancelled(c) => RepairError::Cancelled(c),
                    e => RepairError::Fill(e),
                },
            )?;
        if !spec.allow_unfilled {
            if let Some(&first) = fill_report.refused.first() {
                return Err(RepairError::UnfilledHoles {
                    holes: fill_report.holes_refused,
                    first,
                });
            }
        }
        if filled.is_some() {
            current = filled;
        }
        report.fill = Some(fill_report);
    }
    if let Some(bounds) = spec.shells {
        let source = current.as_ref().unwrap_or(mesh);
        let (kept, shells) = super::sampled::shells::remove_shells(source, bounds, cancel)?;
        if kept.is_some() {
            current = kept;
        }
        report.shells = Some(shells);
    }
    if spec.outward {
        let source = current.as_ref().unwrap_or(mesh);
        let (turned, outward_report, samples) = source
            .repair_with_samples(
                RepairPolicy {
                    duplicates: false,
                    orientation: true,
                    outward: true,
                },
                limits,
                cancel,
            )
            .map_err(RepairError::Topology)?;
        merge(&mut report.mesh, &outward_report);
        for sample in samples {
            record(&mut report.samples, sample.change, sample.point).map_err(|_| {
                RepairError::Topology(TopologyError::Mesh(crate::mesh::MeshError::Allocation {
                    context: "mesh repair samples",
                }))
            })?;
        }
        if turned.is_some() {
            current = turned;
        }
    }
    if let Some(resolution) = spec.rebuild_mm {
        let source = current.as_ref().unwrap_or(mesh);
        let (solid, rebuild) = super::sampled::rebuild_solid(
            source,
            resolution,
            super::sampled::SampledHollowLimits::default(),
            cancel,
        )
        .map_err(|e| match e {
            super::hollow::HollowError::Cancelled(c) => RepairError::Cancelled(c),
            super::hollow::HollowError::Resource { resource } => {
                RepairError::Rebuild(format!("resource limit reached: {resource}"))
            }
            e => RepairError::Rebuild(e.to_string()),
        })?;
        if let Some(point) = rebuild.largest_deviation_at {
            record(
                &mut report.rebuild_samples,
                RepairChange::RebuildDeviation,
                point,
            )
            .map_err(|_| {
                RepairError::Topology(TopologyError::Mesh(crate::mesh::MeshError::Allocation {
                    context: "rebuild samples",
                }))
            })?;
        }
        current = Some(solid);
        report.rebuild = Some(rebuild);
    }
    Ok((current, report))
}
