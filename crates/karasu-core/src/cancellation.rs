//! Synchronous cooperative cancellation for mesh and slicing work.
//!
//! A predicate is borrowed exclusively for the call and executes on the caller's thread. Returning
//! `true` requests cancellation; returning `false` continues. No threads, signals, or progress events
//! are created. A predicate may read a caller-owned atomic flag, but synchronization belongs to the
//! caller. Predicates should return promptly and must not panic.
//!
//! Operations check before input work and before publishing success, even for empty input. Observed
//! cancellation wins at that checkpoint; an ordinary error encountered before the next checkpoint
//! remains that error. Requests after the final checkpoint need not prevent success. No partial
//! successful value escapes; consumed reader bytes cannot be rolled back.
//!
//! Record loops poll every 256 iterations starting at zero; byte scans/read calls use 8 KiB blocks.
//! Phase boundaries also poll. These are work checkpoints, not elapsed-time guarantees: external
//! reads, allocations/initialization, individual hash operations, sorts, deduplication, string scans,
//! bulk copies, and destruction cannot be interrupted internally. Sorting has checks on both sides.
//! Checkpoint sequences are deterministic for fixed input/options on the same build, but exact poll
//! counts are not a compatibility promise across releases. See the architecture for phase limits.
//!
//! ```
//! use karasu_core::cancellation::{Cancellation, Checkpoint};
//! use karasu_core::mesh::stl::{import_stl_bytes_with_cancellation, StlOptions, StlUnitPolicy};
//! let mut predicate = |point| point == Checkpoint::ImportComplete;
//! let mut cancellation = Cancellation::new(&mut predicate);
//! let result = import_stl_bytes_with_cancellation(
//!     b"solid empty\nendsolid empty\n",
//!     StlOptions::new(StlUnitPolicy::AssumeMillimeters), &mut cancellation);
//! assert!(matches!(result, Err(karasu_core::mesh::StlError::Cancelled(_))));
//! ```

use std::{error::Error, fmt};

/// A cancellation observation point, not a progress event or percentage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Checkpoint {
    /// While a mesh Boolean finds the triangles that meet (TD-140).
    BooleanContacts,
    /// While a mesh Boolean divides its cells and classifies their polygons (TD-140).
    BooleanCells,
    /// While a mesh Boolean makes its faces a closed triangle mesh (TD-140).
    BooleanMesh,
    /// Before a headless preparation-session operation.
    SessionStart,
    /// Immediately before publishing a session revision or accepted scene.
    SessionPublish,
    /// Before parametric support plan or scene admission.
    SupportStart,
    /// Bounded profile, contact, collision and assembly work.
    SupportWork,
    /// Immediately before publishing a complete support plan or scene.
    SupportPublish,
    /// Before drainage plan and geometry admission.
    DrainageStart,
    /// Bounded drainage geometry and containment work.
    DrainageWork,
    /// Immediately before publishing a complete drained scene.
    DrainagePublish,
    /// Before hollowing recipe/source admission.
    HollowStart,
    /// Bounded hollowing geometry and predicate traversal.
    HollowWork,
    /// Before independent convex cavity error certification.
    HollowCertificate,
    /// Immediately before publishing the complete hollowed geometry.
    HollowPublish,
    /// Before preparation plan/history admission.
    PreparationStart,
    /// Bounded preparation parameter and history traversal.
    PreparationRecords,
    /// Immediately before publishing a complete plan/history change.
    PreparationPublish,
    /// Before print job validation and export planning.
    ExportStart,
    /// Bounded mask encoding or export layer metadata work.
    ExportLayer,
    /// Encoder tables, checksums and finalization.
    ExportFinish,
    /// Immediately before transactional final publication.
    ExportPublish,
    /// Before wall-verification validation and resource planning (TD-095).
    VerifyStart,
    /// Wall verification: empty-run components across layers.
    VerifyComponents,
    /// Wall verification: in-plane distances and their combination across layers.
    VerifyDistances,
    /// Before each layer of the unsupported regions' analysis (TD-107).
    UnsupportedLayer,
    /// Before each layer of the suction cups' analysis (TD-114).
    SuctionLayer,
    /// Orientation's tilts and sections (TD-119).
    OrientationWork,
    /// Before LCD raster validation and resource planning.
    RasterStart,
    /// Raster input conversion and edge traversal.
    RasterInput,
    /// Sample rows, span accumulation and pixel morphology.
    RasterRows,
    /// Before publishing a whole raster image.
    RasterComplete,
    /// Before PNG output and during bounded PNG blocks.
    RasterPng,
    /// Before opening, reading, or validating STL input.
    ImportStart,
    /// Before and after an external read call.
    Read,
    /// Automatic format detection, including textual classification.
    Detection,
    /// Binary facet decoding.
    BinaryParsing,
    /// ASCII validation, line consumption, and facet decoding.
    AsciiParsing,
    /// Exact-coordinate vertex identity and facet construction.
    Deduplication,
    /// Before constructing an owned mesh.
    MeshStart,
    /// Finite-coordinate validation and signed-zero normalization.
    MeshVertices,
    /// Triangle-index validation.
    MeshIndices,
    /// Source-space extrema accumulation.
    SourceBounds,
    /// Before publishing an owned mesh.
    MeshComplete,
    /// Before geometric diagnostic allocation/work.
    DiagnosticsStart,
    /// Degeneracy and duplicate-face analysis.
    GeometryDiagnostics,
    /// Before publishing geometric diagnostics.
    DiagnosticsComplete,
    /// Stored-normal and attribute diagnostics.
    ImportDiagnostics,
    /// Before publishing an imported mesh and metadata.
    ImportComplete,
    /// Before topology limits or work.
    TopologyStart,
    /// Topology preparation, allocation, and exclusions.
    TopologyPreparation,
    /// Directed uses and edge incidence grouping.
    EdgeIncidence,
    /// Immediately before/after an uninterruptible edge sort.
    EdgeSorting,
    /// Vertex-to-face indexing.
    VertexIndex,
    /// Vertex links, including inner high-incidence loops.
    VertexNeighborhoods,
    /// Before/after vertex-link sorts and deduplication.
    VertexSorting,
    /// Connectivity, component membership, and output buffers.
    Components,
    /// Before/after component membership sorts and deduplication.
    ComponentSorting,
    /// Final whole-report counts and classifications.
    TopologySummary,
    /// Before publishing a complete topology report.
    TopologyComplete,
    /// Before and during transformed-vertex bounds work.
    WorldBounds,
    /// Before publishing transformed bounds, including an empty result.
    BoundsComplete,
    /// Before mesh-section plane validation, limits, or source traversal.
    SectionStart,
    /// Source-face traversal, including checked transforms and local intersections.
    SectionFaces,
    /// Immediately before/after an uninterruptible section event-buffer reservation.
    SectionStorage,
    /// Before publishing a complete raw single-plane mesh section, including empty output.
    SectionComplete,
    /// Before raw-section normalization limits or validation.
    NormalizationStart,
    /// Normalization preflight and source-feature observation extraction.
    NormalizationEvents,
    /// Before/after each fallible normalization buffer reservation.
    NormalizationStorage,
    /// Before/after each normalization index sort.
    NormalizationSorting,
    /// Feature/coordinate/segment grouping, including high-incidence inner loops.
    NormalizationGroups,
    /// Before publishing a complete normalized incidence view, including empty input.
    NormalizationComplete,
    /// Before graph limits or input traversal.
    GraphStart,
    /// Graph input counting and canonical record assembly.
    GraphRecords,
    /// Immediately before/after each fallible graph buffer reservation.
    GraphStorage,
    /// Immediately before/after each indivisible adjacency sort.
    GraphSorting,
    /// Adjacency emission, range construction and degree accounting.
    GraphAdjacency,
    /// Structured conflict and potential endpoint/branch diagnostics.
    GraphDiagnostics,
    /// Before publishing a complete incidence graph, including empty input.
    GraphComplete,
    /// Before contour path limits or graph inspection.
    PathsStart,
    /// Graph consistency checks and bounded path-workspace initialization.
    PathsInput,
    /// Immediately before/after each fallible path buffer reservation.
    PathsStorage,
    /// Candidate component discovery, including high-degree adjacency scans.
    PathsComponents,
    /// Boundary scans, trail traversal and incidence accounting.
    PathsTraversal,
    /// Canonical start/direction scans and before/after bulk rotations/reversals.
    PathsCanonicalization,
    /// Immediately before/after the indivisible path-record sort.
    PathsSorting,
    /// Path ambiguity and complete-only incidence diagnostics.
    PathsDiagnostics,
    /// Before publishing the complete path result, including empty input.
    PathsComplete,
    /// Before planar path inspection limits or input work.
    ValidityStart,
    /// Bounded inspection of graph context, paths and edge coordinates.
    ValidityInput,
    /// Immediately before/after a fallible validity buffer reservation.
    ValidityStorage,
    /// Collapsed edges and bounded inner unordered-pair loops.
    ValidityPairs,
    /// Before emitting each new geometric finding.
    ValidityDiagnostics,
    /// Before publishing a complete planar inspection, including empty input.
    ValidityComplete,
    /// Before cycle area limits or input work.
    AreaStart,
    /// Bounded cycle area metadata preflight.
    AreaInput,
    /// Immediately before/after a fallible area output reservation.
    AreaStorage,
    /// Bounded inner area terms, counted globally across cycles.
    AreaTerms,
    /// Immediately before/after indivisible fixed-size magnitude conversion.
    AreaRounding,
    /// Bounded per-path area outcome assembly.
    AreaRecords,
    /// Before publishing a complete area report, including empty/all-skipped input.
    AreaComplete,
    /// Before exact-grid vertex mapping limits or input work.
    GridStart,
    /// Bounded exact-grid feature metadata preflight.
    GridInput,
    /// Immediately before/after the nonempty exact-grid outcome reservation.
    GridStorage,
    /// Bounded outcome assembly, including unresolved vertices.
    GridRecords,
    /// Bounded exact coordinate conversions, counted globally across both axes.
    GridCoordinates,
    /// Before publishing a complete coordinate view, including empty/all-conflict input.
    GridComplete,
    /// Before strict region-hierarchy limits or input work.
    RegionsStart,
    /// Bounded inspection of region prerequisites and retained ambiguity.
    RegionsInput,
    /// Immediately before/after each fallible region buffer reservation.
    RegionsStorage,
    /// Exact integer bounds assembly for eligible cycles.
    RegionsBounds,
    /// Bounded unordered exact containment-pair classification.
    RegionsPairs,
    /// Bounded containment-depth and immediate-parent assembly.
    RegionsForest,
    /// Bounded boundary, region, hole and path-outcome assembly.
    RegionsOutput,
    /// Before publishing a complete strict region hierarchy, including empty input.
    RegionsComplete,
    /// Before uniform layer-schedule count derivation or resource checks.
    ScheduleStart,
    /// Before each bounded exact terminal-count search probe.
    ScheduleCount,
    /// Immediately before/after nonempty layer-plane reservation.
    ScheduleStorage,
    /// Plane generation, checked every 256 output indices starting at zero.
    SchedulePlanes,
    /// Before publishing a complete schedule, including an empty schedule.
    ScheduleComplete,
    /// Before any work by a newly advanced layer stream.
    LayersStart,
    /// Before processing the next scheduled layer.
    LayerStart,
    /// Before/after each nonempty owned layer-output reservation.
    LayerStorage,
    /// Bounded projection records when converting temporary reports to owned output.
    LayerOutput,
    /// Immediately before publishing one owned layer.
    LayerComplete,
    /// Before normal stream exhaustion, including an empty stream.
    LayersComplete,
    /// Before atomic collection preflight.
    CollectionStart,
    /// Before/after a nonempty collection-header reservation.
    CollectionStorage,
    /// Before publishing the full atomic layer collection.
    CollectionComplete,
    /// Before exact constructive material work.
    ConstructStart,
    /// Material assignment and integer ring validation.
    ConstructInput,
    /// Bounded original-segment intersection/contact pairs.
    ConstructPairs,
    /// Exact source-segment splitting and atomic grouping.
    ConstructSplit,
    /// Symbolic left/right material classification.
    ConstructClassify,
    /// Directed boundary walks, decomposition and containment hierarchy.
    ConstructTopology,
    /// Before/after constructive buffer reservations.
    ConstructStorage,
    /// Before publishing complete exact constructed material.
    ConstructComplete,
}

/// Cancellation observed at a named checkpoint; no successful partial value is returned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cancelled {
    /// Point at which the caller's predicate requested cancellation.
    pub checkpoint: Checkpoint,
}

impl fmt::Display for Cancelled {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "operation cancelled at {:?}", self.checkpoint)
    }
}

impl Error for Cancelled {}

/// Exclusively borrowed predicate, used synchronously without internal synchronization or threads.
/// The controller does not latch requests: callers retain ownership of predicate state. Once a
/// request is observed, that invocation returns immediately through its error path.
pub struct Cancellation<'a> {
    predicate: Option<&'a mut dyn FnMut(Checkpoint) -> bool>,
}

impl<'a> Cancellation<'a> {
    /// Borrows a predicate for the controller's lifetime. `true` requests cancellation.
    pub fn new(predicate: &'a mut dyn FnMut(Checkpoint) -> bool) -> Self {
        Self {
            predicate: Some(predicate),
        }
    }

    /// Creates the no-predicate controller used by compatibility wrappers.
    #[must_use]
    pub const fn never() -> Self {
        Self { predicate: None }
    }

    pub(crate) fn check(&mut self, checkpoint: Checkpoint) -> Result<(), Cancelled> {
        if self
            .predicate
            .as_mut()
            .is_some_and(|predicate| predicate(checkpoint))
        {
            Err(Cancelled { checkpoint })
        } else {
            Ok(())
        }
    }

    pub(crate) fn poll(
        &mut self,
        checkpoint: Checkpoint,
        iteration: usize,
    ) -> Result<(), Cancelled> {
        if iteration.is_multiple_of(256) {
            self.check(checkpoint)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_polling_starts_at_zero_and_repeats_every_256_records() {
        let mut calls = 0;
        let mut predicate = |_| {
            calls += 1;
            false
        };
        let mut cancellation = Cancellation::new(&mut predicate);
        for index in 0..1024 {
            cancellation
                .poll(Checkpoint::GeometryDiagnostics, index)
                .unwrap();
        }
        assert_eq!(calls, 4);
    }

    #[test]
    fn no_predicate_controller_never_cancels() {
        let mut cancellation = Cancellation::never();
        assert_eq!(cancellation.check(Checkpoint::ImportStart), Ok(()));
        assert_eq!(cancellation.check(Checkpoint::ImportComplete), Ok(()));
    }
}
