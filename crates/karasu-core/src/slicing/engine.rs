//! Owned deterministic layer generation with atomic items, fused failure and synchronous progress.
//!
//! The reviewed strict pipeline is composed under TD-029. Geometrically unresolved layers
//! remain explicit successful data; an operational error never publishes a partial layer. Source,
//! schedule and transforms remain immutable. Exhaustion, not the last successful item, completes a
//! stream. Construction is available only through explicit material-source labels. No rasterization, repair,
//! threads or printer defaults are introduced.
//!
//! ```
//! use karasu_core::{geometry::Point3, mesh::TriangleMesh, cancellation::Cancellation};
//! use karasu_core::slicing::{SectionTransform, grid::BinaryGridSpec,
//!     regions::RegionFillRule, schedule::*, engine::*};
//! let mesh = TriangleMesh::try_new(vec![Point3::new(0.,0.,0.), Point3::new(2.,0.,0.),
//!     Point3::new(0.,2.,0.)], vec![[0,1,2]])?;
//! let schedule = build_layer_schedule(
//!     UniformScheduleSpec::try_new(0.,1.,ScheduleEnd::Count(1))?,
//!     ScheduleLimits { max_layers: 1, max_payload_bytes: 1024 })?;
//! let config = LayerConfig { grid: BinaryGridSpec::try_new([0.,0.],0)?,
//!     fill_rule: RegionFillRule::EvenOddStrict, stages: LayerStageLimits::default(),
//!     max_output_payload_bytes: 1024*1024 };
//! let mut engine = LayerEngine::new(&mesh, &schedule, SectionTransform::Identity,
//!     config, Cancellation::never(), None);
//! let layer = engine.next().unwrap()?;
//! assert_eq!(layer.regions().len(),1);
//! assert!(engine.next().is_none());
//! assert_eq!(engine.state(),StreamState::Complete);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use super::{
    area::{AreaError, AreaLimits},
    below_vertex_heights,
    graph::{GraphError, GraphLimits},
    grid::{BinaryGridSpec, GridError, GridLimits},
    normalize::{NormalizationError, NormalizationLimits},
    paths::{PathError, PathLimits},
    regions::{RegionError, RegionFillRule, RegionLimits},
    schedule::{LayerPlane, LayerSchedule},
    section_mesh_z_indexed_with_cancellation, section_mesh_z_with_cancellation,
    validity::{ValidityError, ValidityLimits},
    vertex_heights,
    winding::{self, SectionClosure, UnbalancedCrossing, WindingLayer},
    FaceZCursor, FaceZIndex, FaceZSweep, SectionError, SectionLimits, SectionTransform,
};
use crate::{
    cancellation::{Cancellation, Cancelled, Checkpoint},
    mesh::{
        topology::{RimTable, TopologyError, TopologyLimits},
        TriangleMesh,
    },
};
use std::{error::Error, fmt, iter::FusedIterator, ops::Range};

mod composition;
mod output;
use super::construct::ConstructLimits;
pub use composition::{
    compose_layer_difference, compose_layer_material, compose_layer_prepared, LayerCompositionError,
};
use composition::{compose_layer_with_mode, CompositionMode};
pub use output::{
    LayerEdge, LayerMaterialRegion, LayerPath, LayerSummary, LayerVertex, RegionLayer,
};

/// Independent reviewed per-stage work/storage policies; no physical defaults are selected.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LayerStageLimits {
    /// Full source-face scan and raw events.
    pub section: SectionLimits,
    /// Lossless source-feature incidence normalization.
    pub normalization: NormalizationLimits,
    /// Complete and candidate graph construction.
    pub graph: GraphLimits,
    /// Conservative path extraction.
    pub paths: PathLimits,
    /// Exact represented-XY embedding/contact inspection.
    pub validity: ValidityLimits,
    /// Exact orientation and rounded cycle magnitude.
    pub area: AreaLimits,
    /// Exact-only coordinate conversion.
    pub grid: GridLimits,
    /// Strict nonconstructive hierarchy assembly.
    pub regions: RegionLimits,
}

/// Explicit geometry interpretation, work caps and owned payload cap for each layer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayerConfig {
    /// Explicit exact binary world-XY grid; no origin/step default.
    pub grid: BinaryGridSpec,
    /// Explicit reviewed material interpretation; currently only strict even-odd.
    pub fill_rule: RegionFillRule,
    /// Existing independent stage limits, enforced unchanged.
    pub stages: LayerStageLimits,
    /// Retained raw capacity plus requested owned flat buffers, excluding header and working reports.
    pub max_output_payload_bytes: usize,
}

/// Terminal lifecycle is distinct from the completeness of individual geometric layers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StreamState {
    /// Constructed without any processing or progress callback.
    NotStarted,
    /// Processing started; normal exhaustion has not yet been observed.
    Running,
    /// Final stream cancellation check passed and normal exhaustion was observed.
    Complete,
    /// An operational error or cancellation was returned; no further items can escape.
    Failed,
}

/// Named work domain, not a percentage weighting or estimate of runtime.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayerProgressStage {
    /// Serial composition of the reviewed M3 geometric stages and owned projection.
    M3LayerGeneration,
}

/// Advisory progress observation; only successful layer publication increments the count.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayerProgressKind {
    /// The initial cancellation check passed, before any layer work.
    Started,
    /// About to process the named layer, with the previous completed count.
    LayerStarted,
    /// One whole layer passed its final publication check.
    LayerCompleted,
    /// Normal exhaustion and, for collection, final atomic publication checks passed.
    Completed,
    /// A typed failure terminated generation or collection; never followed by Completed.
    Failed,
}

/// Monotonic completed-layer progress, independent of cancellation and geometric validity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LayerProgress {
    /// Named operation domain.
    pub stage: LayerProgressStage,
    /// Lifecycle event within that domain.
    pub kind: LayerProgressKind,
    /// Number of individually completed output layers, never a floating percentage.
    pub completed_layers: u64,
    /// Exact number of scheduled layers, including explicitly empty plans.
    pub total_layers: u64,
    /// Active/completed/failed layer identity, or None for stream/collection boundaries.
    pub current_layer: Option<u64>,
}

/// Underlying typed failure, separate from its enclosing scheduled-plane context.
#[derive(Debug, PartialEq)]
#[non_exhaustive]
pub enum LayerFailure {
    /// Raw full-face section failure.
    Section(SectionError),
    /// Source-feature normalization failure.
    Normalization(NormalizationError),
    /// Incidence graph failure.
    Graph(GraphError),
    /// Conservative path extraction failure.
    Paths(PathError),
    /// Planar embedding/contact failure.
    Validity(ValidityError),
    /// Cycle area/orientation failure.
    Area(AreaError),
    /// Exact-only grid failure; no point is silently rounded.
    Grid(GridError),
    /// Strict hierarchy assembly failure.
    Regions(RegionError),
    /// Explicit material-source composition failed before layer publication.
    Composition(LayerCompositionError),
    /// Cancellation in the stream, owned projection or collector itself.
    Cancelled(Cancelled),
    /// Requested owned per-layer payload exceeds the explicit cap.
    OutputLimit {
        /// Caller-selected maximum requested bytes.
        limit: usize,
    },
    /// Checked count, capacity, byte or host representation overflow.
    SizeOverflow,
    /// A fallible owned-output or collection reservation failed.
    Allocation,
    /// Defensive inconsistency between immutable private-constructor reports during projection.
    InvalidProjection,
    /// Collection can represent a whole schedule only from a fresh engine.
    CollectionAlreadyStarted,
    /// Full schedule count exceeds the explicit collection cap.
    CollectionLayerLimit {
        /// Caller-selected maximum layer count.
        limit: u64,
    },
    /// Headers plus generated owned payload exceed the explicit aggregate collection cap.
    CollectionPayloadLimit {
        /// Caller-selected maximum requested bytes.
        limit: usize,
    },
    /// A parallel layer worker panicked while computing this layer (an internal defect); no
    /// partial result is used.
    WorkerPanicked,
    /// Faces whose material a cut leaves (TD-123) were given for strict layers, which cannot
    /// tell them apart: only winding layers can.
    KeptFacesNeedWinding,
    /// With vertex-avoiding planes, no plane below the scheduled one both avoids every vertex
    /// height and stays above the previous layer's plane.
    PlaneSeparation {
        /// The scheduled plane Z in mm.
        scheduled_z_mm: f64,
    },
    /// A winding layer's oriented section does not close: an open boundary edge or
    /// inconsistently oriented faces cross the plane. No guessed closure is filled.
    Unbalanced {
        /// Crossing points where leaving and arriving segments differ.
        crossings: usize,
        /// The first of them in XY order, at most
        /// [`MAX_UNBALANCED_DETAILS`](super::winding::MAX_UNBALANCED_DETAILS).
        details: Vec<UnbalancedCrossing>,
    },
    /// Winding layers need a non-singular source-to-world transform.
    SingularTransform,
    /// Winding layers are sectioned only on vertex-avoiding planes, which were not enabled.
    VertexAvoidanceRequired,
    /// Section closure needs a finite, positive gap bound.
    InvalidGapBound,
    /// Boundary rim extraction for section closure failed.
    Topology(TopologyError),
    /// Section closure would need a chord longer than its gap bound: the hole is too large for
    /// the bound, or the source is a surface rather than a solid.
    OpenSectionGap {
        /// Chord length (mm).
        length_mm: f64,
        /// World XY of the open chain end the chord starts at (mm).
        from_mm: [f64; 2],
        /// World XY of the open chain start the chord ends at (mm).
        to_mm: [f64; 2],
        /// The gap bound (mm).
        max_gap_mm: f64,
    },
}
impl fmt::Display for LayerFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Section(e) => write!(f, "section stage: {e}"),
            Self::Normalization(e) => write!(f, "normalization stage: {e}"),
            Self::Graph(e) => write!(f, "graph stage: {e}"),
            Self::Paths(e) => write!(f, "path stage: {e}"),
            Self::Validity(e) => write!(f, "planar inspection stage: {e}"),
            Self::Area(e) => write!(f, "area stage: {e}"),
            Self::Grid(e) => write!(f, "exact grid stage: {e}"),
            Self::Regions(e) => write!(f, "strict regions stage: {e}"),
            Self::Composition(e) => write!(f, "material composition stage: {e}"),
            Self::Cancelled(e) => e.fmt(f),
            Self::OutputLimit { limit } => {
                write!(f, "owned layer payload limit exceeded ({limit} bytes)")
            }
            Self::SizeOverflow => f.write_str("layer output count or storage capacity overflow"),
            Self::Allocation => f.write_str("unable to reserve owned layer output"),
            Self::InvalidProjection => {
                f.write_str("inconsistent immutable reports during owned layer projection")
            }
            Self::CollectionAlreadyStarted => {
                f.write_str("atomic collection requires an unstarted layer engine")
            }
            Self::CollectionLayerLimit { limit } => {
                write!(f, "collection layer limit exceeded ({limit})")
            }
            Self::CollectionPayloadLimit { limit } => {
                write!(f, "collection payload limit exceeded ({limit} bytes)")
            }
            Self::WorkerPanicked => f.write_str("a parallel layer worker panicked"),
            Self::KeptFacesNeedWinding => {
                f.write_str("material kept from the cut needs winding layers, not strict ones")
            }
            Self::PlaneSeparation { scheduled_z_mm } => write!(
                f,
                "no plane below z = {scheduled_z_mm} mm avoids every vertex height above the \
                 previous layer"
            ),
            Self::Unbalanced { crossings, details } => {
                write!(
                    f,
                    "the oriented section does not close at {crossings} crossing point(s)"
                )?;
                if let Some(first) = details.first() {
                    let [px, py] = first.position_mm;
                    let [v0, v1] = first.edge;
                    write!(
                        f,
                        "; first at ({px}, {py}) mm on mesh edge {v0}-{v1}: {}",
                        if first.open_boundary() {
                            "an open boundary (an odd number of faces use the edge)"
                        } else {
                            "faces sharing the edge are inconsistently oriented"
                        }
                    )?;
                }
                Ok(())
            }
            Self::SingularTransform => {
                f.write_str("winding layers need a non-singular source-to-world transform")
            }
            Self::VertexAvoidanceRequired => {
                f.write_str("winding layers need vertex-avoiding planes")
            }
            Self::InvalidGapBound => {
                f.write_str("section closure needs a finite, positive gap bound")
            }
            Self::Topology(e) => write!(f, "boundary rims: {e}"),
            Self::OpenSectionGap {
                length_mm,
                from_mm,
                to_mm,
                max_gap_mm,
            } => write!(
                f,
                "closing an open section at a hole needs a {length_mm} mm chord from ({}, {}) to \
                 ({}, {}) mm, above the {max_gap_mm} mm gap bound: the hole is larger than the \
                 bound or the model is an open surface rather than a solid",
                from_mm[0], from_mm[1], to_mm[0], to_mm[1]
            ),
        }
    }
}
impl Error for LayerFailure {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Section(e) => Some(e),
            Self::Normalization(e) => Some(e),
            Self::Graph(e) => Some(e),
            Self::Paths(e) => Some(e),
            Self::Validity(e) => Some(e),
            Self::Area(e) => Some(e),
            Self::Grid(e) => Some(e),
            Self::Regions(e) => Some(e),
            Self::Composition(e) => Some(e),
            Self::Cancelled(e) => Some(e),
            Self::Topology(e) => Some(e),
            _ => None,
        }
    }
}
impl From<Cancelled> for LayerFailure {
    fn from(cause: Cancelled) -> Self {
        Self::Cancelled(cause)
    }
}

/// One terminal failure with its scheduled plane, if work was associated with an active layer.
#[derive(Debug, PartialEq)]
pub struct LayerEngineError {
    /// Plane whose atomic publication failed; None for stream/collection preflight/exhaustion.
    pub plane: Option<LayerPlane>,
    /// Underlying stage, resource, projection or cancellation cause.
    pub cause: LayerFailure,
}
impl fmt::Display for LayerEngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(plane) = self.plane {
            write!(
                f,
                "layer {} at Z {} mm: {}",
                plane.index, plane.z_mm, self.cause
            )
        } else {
            write!(f, "layer stream: {}", self.cause)
        }
    }
}
impl Error for LayerEngineError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.cause)
    }
}

struct MaterialComposition<'a> {
    mode: CompositionMode,
    face_materials: &'a [u64],
    limits: ConstructLimits,
}

/// Serial pull engine borrowing immutable inputs and caller-owned synchronous controllers.
///
/// A callback must return promptly and not panic. Dropping early makes no completion claim. Each
/// result owns its data; retained successful layers contribute caller-owned memory independently of
/// this engine. After one error, `next` is permanently None. The schedule remains borrowed unchanged.
pub struct LayerEngine<'a, 'c, 'o> {
    mesh: &'a TriangleMesh,
    schedule: &'a LayerSchedule,
    transform: SectionTransform<'a>,
    config: LayerConfig,
    cancel: Cancellation<'c>,
    observer: Option<&'o mut dyn FnMut(LayerProgress)>,
    state: StreamState,
    completed: usize,
    defer_completion: bool,
    represented_coordinates: bool,
    material_composition: Option<MaterialComposition<'a>>,
    face_index: FaceIndexState,
    vertex_heights: VertexHeightState,
    /// Sampled Z of the last completed layer, for the strictly increasing plane check.
    last_z_mm: f64,
}
/// Opt-in vertex-avoiding planes: requested budget, then the sorted distinct vertex heights.
#[derive(Debug)]
enum VertexHeightState {
    Disabled,
    Requested(usize),
    Built(Vec<f64>),
}
/// Opt-in Z index lifecycle: requested budget, then the built sweep.
#[derive(Debug)]
enum FaceIndexState {
    Disabled,
    Requested(usize),
    Built(FaceZSweep),
}
impl<'a, 'c, 'o> LayerEngine<'a, 'c, 'o> {
    /// Bind immutable reviewed inputs without processing, allocating output or invoking observers.
    #[must_use]
    pub fn new(
        mesh: &'a TriangleMesh,
        schedule: &'a LayerSchedule,
        transform: SectionTransform<'a>,
        config: LayerConfig,
        cancel: Cancellation<'c>,
        observer: Option<&'o mut dyn FnMut(LayerProgress)>,
    ) -> Self {
        Self {
            mesh,
            schedule,
            transform,
            config,
            cancel,
            observer,
            state: StreamState::NotStarted,
            completed: 0,
            defer_completion: false,
            represented_coordinates: false,
            material_composition: None,
            face_index: FaceIndexState::Disabled,
            vertex_heights: VertexHeightState::Disabled,
            last_z_mm: f64::NEG_INFINITY,
        }
    }
    /// Sample each layer on a plane that contains no mesh vertex.
    ///
    /// A scheduled plane is kept unless it equals the world Z of some face corner. It then moves
    /// to the middle of the vertex-free interval just below it, which reaches down to the next
    /// lower vertex height or to the bottom of the layer's slab (half a nominal height below),
    /// whichever is higher. No vertex lies between the two planes, so the layer is the section
    /// just below the scheduled one, and with no vertex on the plane every face is either missed
    /// or crossed strictly: horizontal faces, on-plane edges and vertices cannot occur. Without
    /// this, a plane through a horizontal face yields the face itself, which for a step (a face
    /// with material both below and above, such as the top of a plate carrying a boss) is neither
    /// side's material. Keeping the plane mid-interval keeps crossing points well separated from
    /// the vertices. The layer keeps its scheduled [`RegionLayer::plane`] (identity, Z and nominal
    /// height, as print jobs expect); [`RegionLayer::sampled_z_mm`] reports the sectioned Z.
    ///
    /// The heights are computed once on the first layer, with at most `max_bytes` requested (8 per
    /// face corner). A lowered plane that would not stay above the previous layer's plane fails
    /// with [`LayerFailure::PlaneSeparation`].
    #[must_use]
    pub fn with_vertex_avoiding_planes(mut self, max_bytes: usize) -> Self {
        self.vertex_heights = VertexHeightState::Requested(max_bytes);
        self
    }
    /// Evaluate only faces whose world Z range contains each plane, using an index built once on
    /// the first layer with at most `max_index_bytes` requested bytes (24 per face).
    ///
    /// Events, paths, regions and errors are identical to the full scan, except that a transform
    /// failure of any face is reported when the index is built (first layer) and section summaries
    /// count skipped faces in `prefiltered_faces`. `SectionLimits::max_faces` then bounds the
    /// evaluated candidates per plane instead of the whole mesh. Cost per layer becomes
    /// proportional to the faces crossing it instead of the mesh size.
    #[must_use]
    pub fn with_face_z_index(mut self, max_index_bytes: usize) -> Self {
        self.face_index = FaceIndexState::Requested(max_index_bytes);
        self
    }
    /// Retain binary64 millimeter coordinates instead of requesting `ExactOnly` grid mapping.
    ///
    /// Strict containment uses exact predicates on the represented coordinates. No snapping,
    /// welding or tolerance is introduced; configured grid metadata becomes inactive.
    #[must_use]
    pub fn with_represented_coordinates(mut self) -> Self {
        self.represented_coordinates = true;
        self
    }

    /// Opt into exact union between explicitly labelled material sources, with parity within each.
    ///
    /// Labels index every source face. All contributions to one path must agree on its label.
    /// Strict reports and diagnostics remain unchanged; the additional result is separately exposed.
    #[must_use]
    pub fn with_material_union(
        mut self,
        face_materials: &'a [u64],
        limits: ConstructLimits,
    ) -> Self {
        self.material_composition = Some(MaterialComposition {
            mode: CompositionMode::Union,
            face_materials,
            limits,
        });
        self
    }
    /// Subtract all nonzero material sources from material zero, with parity within each label.
    ///
    /// Labels cover every source face and must agree within each path. The result is
    /// exposed through the existing `material_union` compatibility accessor, which can
    /// hold either composition policy. Strict diagnostics remain unchanged.
    #[must_use]
    pub fn with_material_difference(
        mut self,
        face_materials: &'a [u64],
        limits: ConstructLimits,
    ) -> Self {
        self.material_composition = Some(MaterialComposition {
            mode: CompositionMode::Difference,
            face_materials,
            limits,
        });
        self
    }
    /// Compose prepared body-minus-cutters and additive components using explicit face labels.
    ///
    /// ID zero is body, nonzero IDs below `2^63` are cutters, and high-bit IDs are additive
    /// components. Parity is local to each ID. Additives survive cutters and absent body;
    /// winding, connectivity and nesting never infer ownership. The existing `material_union`
    /// compatibility accessor exposes the result without changing strict diagnostics.
    #[must_use]
    pub fn with_prepared_materials(
        mut self,
        face_materials: &'a [u64],
        limits: ConstructLimits,
    ) -> Self {
        self.material_composition = Some(MaterialComposition {
            mode: CompositionMode::Prepared,
            face_materials,
            limits,
        });
        self
    }
    /// Lifecycle including the distinction between the last yielded layer and normal exhaustion.
    #[must_use]
    pub const fn state(&self) -> StreamState {
        self.state
    }
    /// Individually published layers, independent of whether the stream ultimately completes.
    #[must_use]
    pub fn completed_layers(&self) -> u64 {
        // The cursor only advances after successful publication of its indexed schedule item.
        self.schedule.planes()[..self.completed]
            .last()
            .map_or(0, |plane| plane.index + 1)
    }
    /// Exact count from the validated schedule; no mesh-dependent recomputation.
    #[must_use]
    pub fn total_layers(&self) -> u64 {
        // A validated u64 count has last index <=u64::MAX-1, independently of host usize.
        self.schedule
            .planes()
            .last()
            .map_or(0, |plane| plane.index + 1)
    }

    fn notify(&mut self, kind: LayerProgressKind, current_layer: Option<u64>) {
        let event = LayerProgress {
            stage: LayerProgressStage::M3LayerGeneration,
            kind,
            completed_layers: self.completed_layers(),
            total_layers: self.total_layers(),
            current_layer,
        };
        if let Some(observer) = &mut self.observer {
            observer(event);
        }
    }
    fn fail(&mut self, plane: Option<LayerPlane>, cause: LayerFailure) -> LayerEngineError {
        self.state = StreamState::Failed;
        self.notify(LayerProgressKind::Failed, plane.map(|p| p.index));
        LayerEngineError { plane, cause }
    }
}
impl Iterator for LayerEngine<'_, '_, '_> {
    type Item = Result<RegionLayer, LayerEngineError>;

    fn next(&mut self) -> Option<Self::Item> {
        if matches!(self.state, StreamState::Complete | StreamState::Failed) {
            return None;
        }
        if self.state == StreamState::NotStarted {
            if let Err(e) = self.cancel.check(Checkpoint::LayersStart) {
                return Some(Err(self.fail(None, e.into())));
            }
            self.state = StreamState::Running;
            self.notify(LayerProgressKind::Started, None);
        }
        let Some(plane) = self.schedule.planes().get(self.completed).copied() else {
            if let Err(e) = self.cancel.check(Checkpoint::LayersComplete) {
                return Some(Err(self.fail(None, e.into())));
            }
            self.state = StreamState::Complete;
            if !self.defer_completion {
                self.notify(LayerProgressKind::Completed, None);
            }
            return None;
        };
        if let Err(e) = self.cancel.check(Checkpoint::LayerStart) {
            return Some(Err(self.fail(Some(plane), e.into())));
        }
        self.notify(LayerProgressKind::LayerStarted, Some(plane.index));
        if let FaceIndexState::Requested(max_bytes) = self.face_index {
            match FaceZSweep::build(self.mesh, self.transform, max_bytes, &mut self.cancel) {
                Ok(index) => self.face_index = FaceIndexState::Built(index),
                Err(e) => return Some(Err(self.fail(Some(plane), LayerFailure::Section(e)))),
            }
        }
        if let VertexHeightState::Requested(max_bytes) = self.vertex_heights {
            match vertex_heights(self.mesh, self.transform, max_bytes, &mut self.cancel) {
                Ok(heights) => self.vertex_heights = VertexHeightState::Built(heights),
                Err(e) => return Some(Err(self.fail(Some(plane), LayerFailure::Section(e)))),
            }
        }
        let section_z = match &self.vertex_heights {
            VertexHeightState::Built(heights) => {
                let z_mm = below_vertex_heights(plane.z_mm, 0.5 * plane.nominal_height_mm, heights);
                if z_mm <= self.last_z_mm {
                    let cause = LayerFailure::PlaneSeparation {
                        scheduled_z_mm: plane.z_mm,
                    };
                    return Some(Err(self.fail(Some(plane), cause)));
                }
                z_mm
            }
            _ => plane.z_mm,
        };
        let index = match &mut self.face_index {
            FaceIndexState::Built(sweep) => Some(sweep.parts()),
            _ => None,
        };
        let generated = output::generate_with_coordinates(
            self.mesh,
            plane,
            section_z,
            self.transform,
            &self.config,
            self.represented_coordinates,
            index,
            &mut self.cancel,
        );
        let mut layer = match generated {
            Ok(layer) => layer,
            Err(cause) => return Some(Err(self.fail(Some(plane), cause))),
        };
        if let Some(policy) = &self.material_composition {
            let result = compose_layer_with_mode(
                &layer,
                policy.face_materials,
                policy.limits,
                &mut self.cancel,
                policy.mode,
            );
            let material = match result {
                Ok(material) => material,
                Err(error) => {
                    return Some(Err(self.fail(Some(plane), LayerFailure::Composition(error))))
                }
            };
            if let Err(cause) =
                layer.attach_material_union(material, self.config.max_output_payload_bytes)
            {
                return Some(Err(self.fail(Some(plane), cause)));
            }
        }
        if let Err(e) = self.cancel.check(Checkpoint::LayerComplete) {
            return Some(Err(self.fail(Some(plane), e.into())));
        }
        self.completed += 1;
        self.last_z_mm = section_z;
        self.notify(LayerProgressKind::LayerCompleted, Some(plane.index));
        Some(Ok(layer))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        if matches!(self.state, StreamState::Complete | StreamState::Failed) {
            return (0, Some(0));
        }
        (
            0,
            self.schedule
                .planes()
                .len()
                .saturating_sub(self.completed)
                .checked_add(1),
        )
    }
}
impl FusedIterator for LayerEngine<'_, '_, '_> {}

/// Immutable per-mesh state from which any scheduled layer can be computed independently, as the
/// sequential engine computes it: used by parallel layer generation (TD-050).
pub(crate) struct LayerKernel<'a> {
    pub(crate) mesh: &'a TriangleMesh,
    pub(crate) transform: SectionTransform<'a>,
    pub(crate) config: LayerConfig,
    pub(crate) represented_coordinates: bool,
    /// Shared face Z index, or `None` for full scans.
    pub(crate) index: Option<FaceZIndex>,
    /// Sorted distinct vertex heights for vertex-avoiding planes, or `None`.
    pub(crate) heights: Option<Vec<f64>>,
    /// Section closure for winding layers: the gap bound and the mesh's boundary rims.
    pub(crate) closure: Option<(SectionClosure, RimTable)>,
    /// Faces whose material a cut leaves (TD-123): sorted, disjoint ranges.
    pub(crate) kept: Vec<Range<usize>>,
}

impl<'a> LayerKernel<'a> {
    /// Build the shared index and vertex heights once, with the sequential engine's limits.
    ///
    /// # Errors
    /// The section failure the sequential engine reports on its first layer.
    pub(crate) fn build(
        mesh: &'a TriangleMesh,
        transform: SectionTransform<'a>,
        config: &LayerConfig,
        represented_coordinates: bool,
        face_index_bytes: Option<usize>,
        vertex_avoiding_bytes: Option<usize>,
        cancel: &mut Cancellation<'_>,
    ) -> Result<Self, LayerFailure> {
        let index = face_index_bytes
            .map(|bytes| FaceZIndex::build(mesh, transform, bytes, cancel))
            .transpose()
            .map_err(LayerFailure::Section)?;
        let heights = vertex_avoiding_bytes
            .map(|bytes| vertex_heights(mesh, transform, bytes, cancel))
            .transpose()
            .map_err(LayerFailure::Section)?;
        Ok(Self {
            mesh,
            transform,
            config: *config,
            represented_coordinates,
            index,
            heights,
            closure: None,
            kept: Vec::new(),
        })
    }

    /// Keep the material of the faces in `ranges` from a cut (TD-123): their segments are listed
    /// apart in winding layers. The ranges may come in any order and overlap; they are sorted and
    /// merged, which changes no face's membership.
    #[must_use]
    pub(crate) fn with_kept_faces(mut self, ranges: &[Range<usize>]) -> Self {
        let mut sorted: Vec<Range<usize>> =
            ranges.iter().filter(|r| r.start < r.end).cloned().collect();
        sorted.sort_unstable_by_key(|r| (r.start, r.end));
        let mut merged: Vec<Range<usize>> = Vec::with_capacity(sorted.len());
        for range in sorted {
            match merged.last_mut() {
                Some(last) if range.start <= last.end => last.end = last.end.max(range.end),
                _ => merged.push(range),
            }
        }
        self.kept = merged;
        self
    }

    /// Close open sections at holes in winding layers (TD-054), extracting the mesh's boundary
    /// rims once.
    ///
    /// # Errors
    /// [`LayerFailure::InvalidGapBound`] for a non-finite or non-positive bound, and rim
    /// extraction failures.
    pub(crate) fn with_closure(
        mut self,
        closure: SectionClosure,
        cancel: &mut Cancellation<'_>,
    ) -> Result<Self, LayerFailure> {
        if !(closure.max_gap_mm.is_finite() && closure.max_gap_mm > 0.) {
            return Err(LayerFailure::InvalidGapBound);
        }
        let (_, rims) = self
            .mesh
            .rims_with_cancellation(TopologyLimits::default(), cancel)
            .map_err(LayerFailure::Topology)?;
        self.closure = Some((closure, rims));
        Ok(self)
    }

    /// The Z sectioned for `plane`: the scheduled Z, or the vertex-avoiding one (see
    /// [`LayerEngine::with_vertex_avoiding_planes`]).
    pub(crate) fn sampled_z(&self, plane: LayerPlane) -> f64 {
        self.heights.as_deref().map_or(plane.z_mm, |heights| {
            below_vertex_heights(plane.z_mm, 0.5 * plane.nominal_height_mm, heights)
        })
    }

    /// Compute layer `plane`, whose predecessor in the schedule is `previous`, exactly as the
    /// sequential engine does. `cursor` must come from this kernel's index (one per thread).
    ///
    /// # Errors
    /// The sequential engine's stage failures, cancellation and plane separation.
    pub(crate) fn layer(
        &self,
        plane: LayerPlane,
        previous: Option<LayerPlane>,
        cursor: Option<&mut FaceZCursor>,
        cancel: &mut Cancellation<'_>,
    ) -> Result<RegionLayer, LayerFailure> {
        cancel.check(Checkpoint::LayerStart)?;
        let section_z = self.separated_z(plane, previous)?;
        let index = self.index.as_ref().zip(cursor);
        let layer = output::generate_with_coordinates(
            self.mesh,
            plane,
            section_z,
            self.transform,
            &self.config,
            self.represented_coordinates,
            index,
            cancel,
        )?;
        cancel.check(Checkpoint::LayerComplete)?;
        Ok(layer)
    }

    /// The sampled Z of `plane`, strictly above that of its predecessor `previous`.
    fn separated_z(
        &self,
        plane: LayerPlane,
        previous: Option<LayerPlane>,
    ) -> Result<f64, LayerFailure> {
        let section_z = self.sampled_z(plane);
        if self.heights.is_some()
            && previous.is_some_and(|previous| section_z <= self.sampled_z(previous))
        {
            return Err(LayerFailure::PlaneSeparation {
                scheduled_z_mm: plane.z_mm,
            });
        }
        Ok(section_z)
    }

    /// Compute the winding layer of `plane` (TD-050): the balanced oriented section on its
    /// vertex-avoiding plane. The section stage limits and the output payload cap apply as for
    /// strict layers; none of the strict stages after the section run.
    ///
    /// # Errors
    /// [`LayerFailure::VertexAvoidanceRequired`] without vertex heights, section failures,
    /// [`LayerFailure::Unbalanced`], [`LayerFailure::SingularTransform`], plane separation,
    /// resource limits and cancellation.
    pub(crate) fn winding_layer(
        &self,
        plane: LayerPlane,
        previous: Option<LayerPlane>,
        cursor: Option<&mut FaceZCursor>,
        cancel: &mut Cancellation<'_>,
    ) -> Result<WindingLayer, LayerFailure> {
        cancel.check(Checkpoint::LayerStart)?;
        if self.heights.is_none() {
            return Err(LayerFailure::VertexAvoidanceRequired);
        }
        let section_z = self.separated_z(plane, previous)?;
        let limits = self.config.stages.section;
        let section = match self.index.as_ref().zip(cursor) {
            Some(index) => section_mesh_z_indexed_with_cancellation(
                self.mesh,
                section_z,
                self.transform,
                index,
                limits,
                cancel,
            ),
            None => section_mesh_z_with_cancellation(
                self.mesh,
                section_z,
                self.transform,
                limits,
                cancel,
            ),
        }
        .map_err(LayerFailure::Section)?;
        let closure = self
            .closure
            .as_ref()
            .map(|(closure, rims)| winding::Closure {
                rims,
                max_gap_mm: closure.max_gap_mm,
            });
        let layer = winding::layer(
            plane,
            &section,
            self.transform,
            self.config.max_output_payload_bytes,
            closure,
            &self.kept,
            cancel,
        )?;
        cancel.check(Checkpoint::LayerComplete)?;
        Ok(layer)
    }
}

/// Independent aggregate collection limits; no total-process-RSS guarantee.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CollectionLimits {
    /// Full known schedule count, never a silently truncated prefix.
    pub max_layers: u64,
    /// Requested `RegionLayer` headers plus each layer's owned payload.
    pub max_payload_bytes: usize,
}

/// A normally exhausted full schedule; individual layers can still be geometrically unresolved.
#[derive(Debug, PartialEq)]
pub struct LayerCollection {
    layers: Vec<RegionLayer>,
    payload_bytes: usize,
}
impl LayerCollection {
    /// Every layer in original schedule order, including explicit empty/tangent/unresolved layers.
    #[must_use]
    pub fn layers(&self) -> &[RegionLayer] {
        &self.layers
    }
    /// Requested header payload plus retained raw capacity and requested per-layer flat buffers.
    #[must_use]
    pub const fn payload_bytes(&self) -> usize {
        self.payload_bytes
    }
}

/// Consume a fresh engine into an atomic full collection under independent aggregate caps.
///
/// Generation progress may report individually completed layers before a later collection failure.
/// Completed is emitted only after successful stream exhaustion and the final collection check.
/// Headers are reserved once; each generated payload is checked before retention. Partial buffers
/// are dropped on failure. Collection requires a fresh engine and cannot certify a consumed suffix.
/// # Errors
/// Returns contextual stage/cancellation/resource failure with no successful partial collection.
pub fn collect_layers(
    mut engine: LayerEngine<'_, '_, '_>,
    limits: CollectionLimits,
) -> Result<LayerCollection, LayerEngineError> {
    if engine.state != StreamState::NotStarted {
        return Err(engine.fail(None, LayerFailure::CollectionAlreadyStarted));
    }
    engine.defer_completion = true;
    let result = (|| -> Result<LayerCollection, LayerEngineError> {
        let preflight = (|| -> Result<usize, LayerFailure> {
            engine.cancel.check(Checkpoint::CollectionStart)?;
            if engine.total_layers() > limits.max_layers {
                return Err(LayerFailure::CollectionLayerLimit {
                    limit: limits.max_layers,
                });
            }
            let bytes = engine
                .schedule
                .planes()
                .len()
                .checked_mul(size_of::<RegionLayer>())
                .ok_or(LayerFailure::SizeOverflow)?;
            isize::try_from(bytes).map_err(|_| LayerFailure::SizeOverflow)?;
            if bytes > limits.max_payload_bytes {
                return Err(LayerFailure::CollectionPayloadLimit {
                    limit: limits.max_payload_bytes,
                });
            }
            Ok(bytes)
        })();
        let mut payload_bytes = preflight.map_err(|e| engine.fail(None, e))?;
        let mut layers = Vec::new();
        if engine.total_layers() > 0 {
            let reserve = (|| -> Result<(), LayerFailure> {
                engine.cancel.check(Checkpoint::CollectionStorage)?;
                layers
                    .try_reserve_exact(engine.schedule.planes().len())
                    .map_err(|_| LayerFailure::Allocation)?;
                engine.cancel.check(Checkpoint::CollectionStorage)?;
                Ok(())
            })();
            reserve.map_err(|e| engine.fail(None, e))?;
        }
        while let Some(item) = engine.next() {
            let layer = item?;
            let plane = Some(layer.plane());
            payload_bytes = payload_bytes
                .checked_add(layer.payload_bytes())
                .ok_or_else(|| engine.fail(plane, LayerFailure::SizeOverflow))?;
            if payload_bytes > limits.max_payload_bytes {
                return Err(engine.fail(
                    plane,
                    LayerFailure::CollectionPayloadLimit {
                        limit: limits.max_payload_bytes,
                    },
                ));
            }
            layers.push(layer);
        }
        engine
            .cancel
            .check(Checkpoint::CollectionComplete)
            .map_err(|e| engine.fail(None, e.into()))?;
        Ok(LayerCollection {
            layers,
            payload_bytes,
        })
    })();
    if result.is_ok() {
        engine.notify(LayerProgressKind::Completed, None);
    }
    result
}
