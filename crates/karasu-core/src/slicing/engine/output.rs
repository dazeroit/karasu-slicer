//! Owned projection of a single reviewed pipeline result.
//!
//! Adjacency and exact-coordinate sorting are derived lookups, deliberately omitted; original
//! observations, occurrences, identities, paths, hierarchy and diagnostics remain recoverable.
use super::{LayerConfig, LayerFailure};
use crate::slicing::{
    area::{measure_cycle_areas_with_cancellation, AreaSummary, PathAreaOutcome},
    graph::{
        build_incidence_graph_with_cancellation, GraphDiagnostic, GraphEdge, GraphSummary,
        VertexDegrees,
    },
    grid::{
        map_exact_grid_vertices_with_cancellation, BinaryGridSpec, GridSummary, GridVertexOutcome,
    },
    normalize::{
        normalize_section_with_cancellation, CoordinateAgreement, EndpointObservation,
        IncidenceIssues, NormalizationSummary, Ownership, SegmentOccurrence, SourceFeature,
    },
    paths::{
        extract_paths_with_cancellation, EdgeOutcome, PathComponent, PathDiagnostic, PathIssues,
        PathKind, PathSummary,
    },
    regions::{
        assemble_strict_regions_with_cancellation, PathRegionOutcome, RegionBoundary,
        RegionCompleteness, RegionDiagnostic, RegionFillRule, RegionSummary, SectionRegions,
    },
    schedule::LayerPlane,
    section_mesh_z_indexed_with_cancellation, section_mesh_z_with_cancellation,
    validity::{
        inspect_planar_paths_with_cancellation, PathEmbedding, PlanarDiagnostic, ValiditySummary,
    },
    FaceSection, FaceZCursor, FaceZIndex, MeshSection, SectionTransform,
};
use crate::{
    cancellation::{Cancellation, Checkpoint},
    mesh::TriangleMesh,
};
use std::{mem::size_of, ops::Range};

/// Complete stage summaries; working-stage payload counts are independent of owned output bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LayerSummary {
    /// Retained normalization stage summary.
    pub normalization: NormalizationSummary,
    /// Retained graph stage summary.
    pub graph: GraphSummary,
    /// Retained paths stage summary.
    pub paths: PathSummary,
    /// Retained validity stage summary.
    pub validity: ValiditySummary,
    /// Retained area stage summary.
    pub area: AreaSummary,
    /// Retained grid stage summary.
    pub grid: GridSummary,
    /// Retained regions stage summary.
    pub regions: RegionSummary,
}
/// Canonical source-feature vertex with every observation preserved.
#[derive(Clone, Debug, PartialEq)]
pub struct LayerVertex {
    /// Original source identity.
    pub feature: SourceFeature,
    /// Exact represented-coordinate agreement or explicit conflict.
    pub coordinates: CoordinateAgreement,
    /// Complete and candidate degree counts.
    pub degrees: VertexDegrees,
    /// Exact-grid mapping outcome.
    pub grid: GridVertexOutcome,
    observations: Range<usize>,
}
/// Complete incidence edge, including unresolved and internal groups.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayerEdge {
    /// Original canonical graph edge.
    pub graph: GraphEdge,
    /// Conservative incidence ownership.
    pub ownership: Ownership,
    /// Independent ambiguity flags.
    pub issues: IncidenceIssues,
    /// Distinct contributing source faces.
    pub source_faces: usize,
    /// Path use or explicit nonpath outcome.
    pub outcome: EdgeOutcome,
    occurrences: Range<usize>,
}
/// Canonical path with geometry and region outcomes; no path is silently omitted.
#[derive(Clone, Debug, PartialEq)]
pub struct LayerPath {
    /// Candidate component ID.
    pub component: usize,
    /// Graph-level path classification.
    pub kind: PathKind,
    /// Whether the graph walk returns to its start.
    pub closed: bool,
    /// Graph uncertainty flags.
    pub issues: PathIssues,
    /// Open-trail endpoint IDs.
    pub endpoints: Option<[usize; 2]>,
    /// Planar geometric assessment.
    pub embedding: PathEmbedding,
    /// Exact area/orientation assessment.
    pub area: PathAreaOutcome,
    /// Region eligibility or assigned boundary.
    pub region_outcome: PathRegionOutcome,
    vertices: Range<usize>,
    edges: Range<usize>,
}
/// Material region with direct hole references into the owned boundary array.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayerMaterialRegion {
    outer: usize,
    holes: Range<usize>,
}
impl LayerMaterialRegion {
    /// Index of the outer boundary in the owned boundary array.
    #[must_use]
    pub const fn outer_boundary(&self) -> usize {
        self.outer
    }
}
/// Atomic owned layer; IDs and ordering match the unchanged individual stages.
///
/// Payload bytes count requested vector lengths plus retained raw-event capacity, not allocator
/// overhead, temporary overlapping stage storage, object headers or process resident memory.
#[derive(Debug, PartialEq)]
pub struct RegionLayer {
    plane: LayerPlane,
    raw: MeshSection,
    represented_coordinates: bool,
    grid_spec: BinaryGridSpec,
    fill_rule: RegionFillRule,
    completeness: RegionCompleteness,
    summary: LayerSummary,
    payload_bytes: usize,
    material_union: Option<crate::slicing::construct::ConstructedMaterial>,
    data: LayerData,
}
impl RegionLayer {
    /// Additional exact material composition, present after explicit union or difference selection.
    /// The accessor name is retained for compatibility; the selected engine policy defines the solid.
    /// Original strict completeness and diagnostics are preserved independently.
    #[must_use]
    pub const fn material_union(&self) -> Option<&crate::slicing::construct::ConstructedMaterial> {
        self.material_union.as_ref()
    }
    pub(super) fn attach_material_union(
        &mut self,
        material: crate::slicing::construct::ConstructedMaterial,
        limit: usize,
    ) -> Result<(), LayerFailure> {
        let bytes = self
            .payload_bytes
            .checked_add(material.payload_bytes())
            .ok_or(LayerFailure::SizeOverflow)?;
        if bytes > limit {
            return Err(LayerFailure::OutputLimit { limit });
        }
        self.payload_bytes = bytes;
        self.material_union = Some(material);
        Ok(())
    }

    /// Immutable plane.
    #[must_use]
    pub const fn plane(&self) -> LayerPlane {
        self.plane
    }
    /// World Z actually sectioned, in mm: the scheduled plane's Z unless the engine moved it off
    /// a vertex height (see [`super::LayerEngine::with_vertex_avoiding_planes`]).
    #[must_use]
    pub const fn sampled_z_mm(&self) -> f64 {
        self.raw.plane_z()
    }
    /// Whether world-XY coordinates were retained without requesting integer grid mapping.
    #[must_use]
    pub const fn uses_represented_coordinates(&self) -> bool {
        self.represented_coordinates
    }

    /// Caller grid spec; inactive when `uses_represented_coordinates()` is true.
    #[must_use]
    pub const fn grid_spec(&self) -> BinaryGridSpec {
        self.grid_spec
    }
    /// Immutable fill rule.
    #[must_use]
    pub const fn fill_rule(&self) -> RegionFillRule {
        self.fill_rule
    }
    /// Immutable completeness.
    #[must_use]
    pub const fn completeness(&self) -> RegionCompleteness {
        self.completeness
    }
    /// Immutable summary.
    #[must_use]
    pub const fn summary(&self) -> LayerSummary {
        self.summary
    }
    /// Immutable payload bytes.
    #[must_use]
    pub const fn payload_bytes(&self) -> usize {
        self.payload_bytes
    }
    /// Unmodified raw face events and source provenance.
    #[must_use]
    pub const fn raw(&self) -> &MeshSection {
        &self.raw
    }
    /// Retained vertices in original ID order.
    #[must_use]
    pub fn vertices(&self) -> &[LayerVertex] {
        &self.data.vertices
    }
    /// Retained edges in original ID order.
    #[must_use]
    pub fn edges(&self) -> &[LayerEdge] {
        &self.data.edges
    }
    /// Retained paths in original ID order.
    #[must_use]
    pub fn paths(&self) -> &[LayerPath] {
        &self.data.paths
    }
    /// Retained observations in original ID order.
    #[must_use]
    pub fn observations(&self) -> &[EndpointObservation] {
        &self.data.observations
    }
    /// Retained segments in original ID order.
    #[must_use]
    pub fn segments(&self) -> &[SegmentOccurrence] {
        &self.data.segments
    }
    /// Retained boundaries in original ID order.
    #[must_use]
    pub fn boundaries(&self) -> &[RegionBoundary] {
        &self.data.boundaries
    }
    /// Retained regions in original ID order.
    #[must_use]
    pub fn regions(&self) -> &[LayerMaterialRegion] {
        &self.data.regions
    }
    /// Retained graph diagnostics in original ID order.
    #[must_use]
    pub fn graph_diagnostics(&self) -> &[GraphDiagnostic] {
        &self.data.graph_diagnostics
    }
    /// Retained path diagnostics in original ID order.
    #[must_use]
    pub fn path_diagnostics(&self) -> &[PathDiagnostic] {
        &self.data.path_diagnostics
    }
    /// Retained planar diagnostics in original ID order.
    #[must_use]
    pub fn planar_diagnostics(&self) -> &[PlanarDiagnostic] {
        &self.data.planar_diagnostics
    }
    /// Retained region diagnostics in original ID order.
    #[must_use]
    pub fn region_diagnostics(&self) -> &[RegionDiagnostic] {
        &self.data.region_diagnostics
    }
    /// Retained components in original ID order.
    #[must_use]
    pub fn components(&self) -> &[PathComponent] {
        &self.data.components
    }
    /// Flat referenced IDs for a valid record, or `None` for an invalid ID.
    #[must_use]
    pub fn vertex_observations(&self, id: usize) -> Option<&[usize]> {
        self.data
            .feature_order
            .get(self.data.vertices.get(id)?.observations.clone())
    }
    /// Flat referenced IDs for a valid record, or `None` for an invalid ID.
    #[must_use]
    pub fn edge_occurrences(&self, id: usize) -> Option<&[usize]> {
        self.data
            .segment_order
            .get(self.data.edges.get(id)?.occurrences.clone())
    }
    /// Flat referenced IDs for a valid record, or `None` for an invalid ID.
    #[must_use]
    pub fn path_vertices(&self, id: usize) -> Option<&[usize]> {
        self.data
            .path_vertex_ids
            .get(self.data.paths.get(id)?.vertices.clone())
    }
    /// Flat referenced IDs for a valid record, or `None` for an invalid ID.
    #[must_use]
    pub fn path_edges(&self, id: usize) -> Option<&[usize]> {
        self.data
            .path_edge_ids
            .get(self.data.paths.get(id)?.edges.clone())
    }
    /// Flat referenced IDs for a valid record, or `None` for an invalid ID.
    #[must_use]
    pub fn region_holes(&self, id: usize) -> Option<&[usize]> {
        self.data
            .hole_ids
            .get(self.data.regions.get(id)?.holes.clone())
    }
}
fn add(a: usize, b: usize) -> Result<usize, LayerFailure> {
    a.checked_add(b).ok_or(LayerFailure::SizeOverflow)
}
fn bytes<T>(n: usize) -> Result<usize, LayerFailure> {
    n.checked_mul(size_of::<T>())
        .filter(|&n| isize::try_from(n).is_ok())
        .ok_or(LayerFailure::SizeOverflow)
}
fn reserve<T>(n: usize, cancel: &mut Cancellation<'_>) -> Result<Vec<T>, LayerFailure> {
    let mut v = Vec::new();
    if n != 0 {
        cancel
            .check(Checkpoint::LayerStorage)
            .map_err(LayerFailure::Cancelled)?;
        v.try_reserve_exact(n)
            .map_err(|_| LayerFailure::Allocation)?;
        cancel
            .check(Checkpoint::LayerStorage)
            .map_err(LayerFailure::Cancelled)?;
    }
    Ok(v)
}
fn tick(cancel: &mut Cancellation<'_>, count: &mut usize) -> Result<(), LayerFailure> {
    cancel
        .poll(Checkpoint::LayerOutput, *count)
        .map_err(LayerFailure::Cancelled)?;
    *count = add(*count, 1)?;
    Ok(())
}
fn copy<T: Copy>(
    source: &[T],
    output: &mut Vec<T>,
    cancel: &mut Cancellation<'_>,
    count: &mut usize,
) -> Result<(), LayerFailure> {
    for &item in source {
        tick(cancel, count)?;
        output.push(item);
    }
    Ok(())
}
#[derive(Debug, PartialEq)]
struct LayerData {
    vertices: Vec<LayerVertex>,
    edges: Vec<LayerEdge>,
    paths: Vec<LayerPath>,
    observations: Vec<EndpointObservation>,
    segments: Vec<SegmentOccurrence>,
    boundaries: Vec<RegionBoundary>,
    regions: Vec<LayerMaterialRegion>,
    graph_diagnostics: Vec<GraphDiagnostic>,
    path_diagnostics: Vec<PathDiagnostic>,
    planar_diagnostics: Vec<PlanarDiagnostic>,
    region_diagnostics: Vec<RegionDiagnostic>,
    components: Vec<PathComponent>,
    feature_order: Vec<usize>,
    segment_order: Vec<usize>,
    path_vertex_ids: Vec<usize>,
    path_edge_ids: Vec<usize>,
    hole_ids: Vec<usize>,
}
#[derive(Clone, Copy)]
struct ProjectionPlan {
    lengths: [usize; 17],
    payload_bytes: usize,
}
#[cfg(test)]
pub(super) fn generate(
    mesh: &TriangleMesh,
    plane: LayerPlane,
    transform: SectionTransform<'_>,
    config: &LayerConfig,
    cancel: &mut Cancellation<'_>,
) -> Result<RegionLayer, LayerFailure> {
    generate_with_coordinates(
        mesh, plane, plane.z_mm, transform, config, false, None, cancel,
    )
}

/// `plane` is the layer's scheduled identity; `section_z` is the Z actually sectioned (equal to
/// `plane.z_mm` unless the engine moved the plane off a vertex height).
#[allow(clippy::too_many_arguments)] // Private stage entry point; each input is distinct.
pub(super) fn generate_with_coordinates(
    mesh: &TriangleMesh,
    plane: LayerPlane,
    section_z: f64,
    transform: SectionTransform<'_>,
    config: &LayerConfig,
    represented: bool,
    index: Option<(&FaceZIndex, &mut FaceZCursor)>,
    cancel: &mut Cancellation<'_>,
) -> Result<RegionLayer, LayerFailure> {
    let raw = match index {
        Some(index) => section_mesh_z_indexed_with_cancellation(
            mesh,
            section_z,
            transform,
            index,
            config.stages.section,
            cancel,
        ),
        None => section_mesh_z_with_cancellation(
            mesh,
            section_z,
            transform,
            config.stages.section,
            cancel,
        ),
    }
    .map_err(LayerFailure::Section)?;
    let normalized = normalize_section_with_cancellation(&raw, config.stages.normalization, cancel)
        .map_err(LayerFailure::Normalization)?;
    let graph = build_incidence_graph_with_cancellation(&normalized, config.stages.graph, cancel)
        .map_err(LayerFailure::Graph)?;
    let paths = extract_paths_with_cancellation(&graph, config.stages.paths, cancel)
        .map_err(LayerFailure::Paths)?;
    let inspection = inspect_planar_paths_with_cancellation(&paths, config.stages.validity, cancel)
        .map_err(LayerFailure::Validity)?;
    let area = measure_cycle_areas_with_cancellation(&inspection, config.stages.area, cancel)
        .map_err(LayerFailure::Area)?;
    let coordinate_view = if represented {
        super::super::grid::retain_represented_vertices_with_cancellation
    } else {
        map_exact_grid_vertices_with_cancellation
    };
    let grid = coordinate_view(&inspection, config.grid, config.stages.grid, cancel)
        .map_err(LayerFailure::Grid)?;
    let regions = match config.fill_rule {
        RegionFillRule::EvenOddStrict => assemble_strict_regions_with_cancellation(
            &inspection,
            &area,
            &grid,
            config.stages.regions,
            cancel,
        )
        .map_err(LayerFailure::Regions)?,
    };
    let mut work = 0;
    let plan = preflight(&regions, config.max_output_payload_bytes, &mut work, cancel)?;
    let mut data = allocate(plan, cancel)?;
    copy_vertices_edges(&regions, &mut data, &mut work, cancel)?;
    copy_paths_regions(&regions, &mut data, &mut work, cancel)?;
    copy_flat(&regions, &mut data, &mut work, cancel)?;
    let completeness = regions.completeness();
    let summary = LayerSummary {
        normalization: normalized.summary(),
        graph: graph.summary(),
        paths: paths.summary(),
        validity: inspection.summary(),
        area: area.summary(),
        grid: grid.summary(),
        regions: regions.summary(),
    };
    Ok(RegionLayer {
        plane,
        raw,
        represented_coordinates: represented,
        grid_spec: config.grid,
        fill_rule: config.fill_rule,
        completeness,
        summary,
        payload_bytes: plan.payload_bytes,
        material_union: None,
        data,
    })
}
fn preflight(
    regions: &SectionRegions<'_, '_, '_, '_, '_, '_>,
    limit: usize,
    work: &mut usize,
    cancel: &mut Cancellation<'_>,
) -> Result<ProjectionPlan, LayerFailure> {
    let inspection = regions.inspection();
    let paths = inspection.paths();
    let graph = paths.graph();
    let normalized = graph.normalized();
    let mut path_vertex_count = 0;
    let mut path_edge_count = 0;
    let mut hole_count = 0;
    for id in 0..paths.paths().len() {
        tick(cancel, work)?;
        path_vertex_count = add(
            path_vertex_count,
            paths
                .path_vertices(id)
                .ok_or(LayerFailure::InvalidProjection)?
                .len(),
        )?;
        path_edge_count = add(
            path_edge_count,
            paths
                .path_edges(id)
                .ok_or(LayerFailure::InvalidProjection)?
                .len(),
        )?;
    }
    for id in 0..regions.regions().len() {
        tick(cancel, work)?;
        hole_count = add(
            hole_count,
            regions
                .region_holes(id)
                .ok_or(LayerFailure::InvalidProjection)?
                .len(),
        )?;
    }
    let lengths = [
        graph.vertices().len(),
        graph.edges().len(),
        paths.paths().len(),
        normalized.observations().len(),
        normalized.segments().len(),
        regions.boundaries().len(),
        regions.regions().len(),
        graph.diagnostics().len(),
        paths.diagnostics().len(),
        inspection.diagnostics().len(),
        regions.diagnostics().len(),
        paths.components().len(),
        normalized.feature_order().len(),
        normalized.segment_order().len(),
        path_vertex_count,
        path_edge_count,
        hole_count,
    ];
    let sizes = [
        size_of::<LayerVertex>(),
        size_of::<LayerEdge>(),
        size_of::<LayerPath>(),
        size_of::<EndpointObservation>(),
        size_of::<SegmentOccurrence>(),
        size_of::<RegionBoundary>(),
        size_of::<LayerMaterialRegion>(),
        size_of::<GraphDiagnostic>(),
        size_of::<PathDiagnostic>(),
        size_of::<PlanarDiagnostic>(),
        size_of::<RegionDiagnostic>(),
        size_of::<PathComponent>(),
        size_of::<usize>(),
        size_of::<usize>(),
        size_of::<usize>(),
        size_of::<usize>(),
        size_of::<usize>(),
    ];
    let mut payload_bytes = bytes::<FaceSection>(normalized.raw().event_capacity())?;
    for (length, size) in lengths.into_iter().zip(sizes) {
        let count = length
            .checked_mul(size)
            .filter(|&n| isize::try_from(n).is_ok())
            .ok_or(LayerFailure::SizeOverflow)?;
        payload_bytes = add(payload_bytes, count)?;
    }
    if payload_bytes > limit {
        return Err(LayerFailure::OutputLimit { limit });
    }
    Ok(ProjectionPlan {
        lengths,
        payload_bytes,
    })
}
fn allocate(
    plan: ProjectionPlan,
    cancel: &mut Cancellation<'_>,
) -> Result<LayerData, LayerFailure> {
    Ok(LayerData {
        vertices: reserve::<LayerVertex>(plan.lengths[0], cancel)?,
        edges: reserve::<LayerEdge>(plan.lengths[1], cancel)?,
        paths: reserve::<LayerPath>(plan.lengths[2], cancel)?,
        observations: reserve::<EndpointObservation>(plan.lengths[3], cancel)?,
        segments: reserve::<SegmentOccurrence>(plan.lengths[4], cancel)?,
        boundaries: reserve::<RegionBoundary>(plan.lengths[5], cancel)?,
        regions: reserve::<LayerMaterialRegion>(plan.lengths[6], cancel)?,
        graph_diagnostics: reserve::<GraphDiagnostic>(plan.lengths[7], cancel)?,
        path_diagnostics: reserve::<PathDiagnostic>(plan.lengths[8], cancel)?,
        planar_diagnostics: reserve::<PlanarDiagnostic>(plan.lengths[9], cancel)?,
        region_diagnostics: reserve::<RegionDiagnostic>(plan.lengths[10], cancel)?,
        components: reserve::<PathComponent>(plan.lengths[11], cancel)?,
        feature_order: reserve::<usize>(plan.lengths[12], cancel)?,
        segment_order: reserve::<usize>(plan.lengths[13], cancel)?,
        path_vertex_ids: reserve::<usize>(plan.lengths[14], cancel)?,
        path_edge_ids: reserve::<usize>(plan.lengths[15], cancel)?,
        hole_ids: reserve::<usize>(plan.lengths[16], cancel)?,
    })
}
fn copy_vertices_edges(
    regions: &SectionRegions<'_, '_, '_, '_, '_, '_>,
    data: &mut LayerData,
    work: &mut usize,
    cancel: &mut Cancellation<'_>,
) -> Result<(), LayerFailure> {
    let inspection = regions.inspection();
    let paths = inspection.paths();
    let graph = paths.graph();
    let normalized = graph.normalized();
    let grid = regions.grid();
    for (id, vertex) in graph.vertices().iter().enumerate() {
        tick(cancel, work)?;
        if vertex.feature_group != id {
            return Err(LayerFailure::InvalidProjection);
        }
        let feature = normalized
            .features()
            .get(id)
            .ok_or(LayerFailure::InvalidProjection)?;
        if normalized
            .feature_order()
            .get(feature.observations.clone())
            .is_none()
        {
            return Err(LayerFailure::InvalidProjection);
        }
        data.vertices.push(LayerVertex {
            feature: feature.feature,
            coordinates: feature.coordinates,
            degrees: vertex.degrees,
            grid: grid.vertex(id).ok_or(LayerFailure::InvalidProjection)?,
            observations: feature.observations.clone(),
        });
    }
    for (id, edge) in graph.edges().iter().enumerate() {
        tick(cancel, work)?;
        if edge.group != id {
            return Err(LayerFailure::InvalidProjection);
        }
        let group = normalized
            .groups()
            .get(id)
            .ok_or(LayerFailure::InvalidProjection)?;
        if normalized
            .segment_order()
            .get(group.occurrences.clone())
            .is_none()
        {
            return Err(LayerFailure::InvalidProjection);
        }
        data.edges.push(LayerEdge {
            graph: *edge,
            ownership: group.ownership,
            issues: group.issues,
            source_faces: group.source_faces,
            outcome: paths
                .edge_outcome(id)
                .ok_or(LayerFailure::InvalidProjection)?,
            occurrences: group.occurrences.clone(),
        });
    }
    Ok(())
}
fn copy_paths_regions(
    regions: &SectionRegions<'_, '_, '_, '_, '_, '_>,
    data: &mut LayerData,
    work: &mut usize,
    cancel: &mut Cancellation<'_>,
) -> Result<(), LayerFailure> {
    let inspection = regions.inspection();
    let paths = inspection.paths();
    let area = regions.areas();
    for (id, path) in paths.paths().iter().enumerate() {
        tick(cancel, work)?;
        let vs = paths
            .path_vertices(id)
            .ok_or(LayerFailure::InvalidProjection)?;
        let es = paths
            .path_edges(id)
            .ok_or(LayerFailure::InvalidProjection)?;
        let vr = data.path_vertex_ids.len()..add(data.path_vertex_ids.len(), vs.len())?;
        let er = data.path_edge_ids.len()..add(data.path_edge_ids.len(), es.len())?;
        copy(vs, &mut data.path_vertex_ids, cancel, work)?;
        copy(es, &mut data.path_edge_ids, cancel, work)?;
        data.paths.push(LayerPath {
            component: path.component,
            kind: path.kind,
            closed: path.closed,
            issues: path.issues,
            endpoints: path.endpoints,
            embedding: *inspection
                .embeddings()
                .get(id)
                .ok_or(LayerFailure::InvalidProjection)?,
            area: *area
                .outcomes()
                .get(id)
                .ok_or(LayerFailure::InvalidProjection)?,
            region_outcome: *regions
                .path_outcomes()
                .get(id)
                .ok_or(LayerFailure::InvalidProjection)?,
            vertices: vr,
            edges: er,
        });
    }
    for (id, region) in regions.regions().iter().enumerate() {
        tick(cancel, work)?;
        let ids = regions
            .region_holes(id)
            .ok_or(LayerFailure::InvalidProjection)?;
        let holes = data.hole_ids.len()..add(data.hole_ids.len(), ids.len())?;
        copy(ids, &mut data.hole_ids, cancel, work)?;
        data.regions.push(LayerMaterialRegion {
            outer: region.outer_boundary(),
            holes,
        });
    }
    Ok(())
}
fn copy_flat(
    regions: &SectionRegions<'_, '_, '_, '_, '_, '_>,
    data: &mut LayerData,
    work: &mut usize,
    cancel: &mut Cancellation<'_>,
) -> Result<(), LayerFailure> {
    let inspection = regions.inspection();
    let paths = inspection.paths();
    let graph = paths.graph();
    let normalized = graph.normalized();
    copy(
        normalized.observations(),
        &mut data.observations,
        cancel,
        work,
    )?;
    copy(normalized.segments(), &mut data.segments, cancel, work)?;
    copy(regions.boundaries(), &mut data.boundaries, cancel, work)?;
    copy(
        graph.diagnostics(),
        &mut data.graph_diagnostics,
        cancel,
        work,
    )?;
    copy(
        paths.diagnostics(),
        &mut data.path_diagnostics,
        cancel,
        work,
    )?;
    copy(
        inspection.diagnostics(),
        &mut data.planar_diagnostics,
        cancel,
        work,
    )?;
    copy(
        regions.diagnostics(),
        &mut data.region_diagnostics,
        cancel,
        work,
    )?;
    copy(paths.components(), &mut data.components, cancel, work)?;
    copy(
        normalized.feature_order(),
        &mut data.feature_order,
        cancel,
        work,
    )?;
    copy(
        normalized.segment_order(),
        &mut data.segment_order,
        cancel,
        work,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        geometry::Point3,
        slicing::{
            area::AreaLimits, engine::LayerStageLimits, graph::GraphLimits, grid::GridLimits,
            normalize::NormalizationLimits, paths::PathLimits, regions::RegionLimits,
            validity::ValidityLimits, SectionLimits,
        },
    };

    fn fixture() -> (TriangleMesh, LayerPlane, LayerConfig) {
        let mesh = TriangleMesh::try_new(
            vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(4.0, 0.0, 0.0),
                Point3::new(0.0, 4.0, 0.0),
            ],
            vec![[0, 1, 2]],
        )
        .unwrap();
        let plane = LayerPlane {
            index: 0,
            z_mm: 0.0,
            nominal_height_mm: 1.0,
        };
        let config = LayerConfig {
            grid: BinaryGridSpec::try_new([0.0, 0.0], 0).unwrap(),
            fill_rule: RegionFillRule::EvenOddStrict,
            stages: LayerStageLimits {
                section: SectionLimits::default(),
                normalization: NormalizationLimits::default(),
                graph: GraphLimits::default(),
                paths: PathLimits::default(),
                validity: ValidityLimits::default(),
                area: AreaLimits::default(),
                grid: GridLimits::default(),
                regions: RegionLimits::default(),
            },
            max_output_payload_bytes: usize::MAX,
        };
        (mesh, plane, config)
    }

    #[test]
    fn owned_coplanar_cycle_preserves_all_provenance_and_flat_references() {
        let (mesh, plane, config) = fixture();
        let layer = generate(
            &mesh,
            plane,
            SectionTransform::Identity,
            &config,
            &mut Cancellation::never(),
        )
        .unwrap();
        assert_eq!(layer.raw().events().len(), 1);
        assert_eq!(layer.vertices().len(), 3);
        assert_eq!(layer.edges().len(), 3);
        assert_eq!(layer.paths().len(), 1);
        assert_eq!(layer.boundaries().len(), 1);
        assert_eq!(layer.regions().len(), 1);
        assert_eq!(layer.completeness(), RegionCompleteness::Complete);
        for (id, vertex) in layer.vertices().iter().enumerate() {
            for &observation in layer.vertex_observations(id).unwrap() {
                assert_eq!(layer.observations()[observation].feature, vertex.feature);
                assert_eq!(layer.observations()[observation].event, 0);
            }
        }
        for (id, edge) in layer.edges().iter().enumerate() {
            assert_eq!(
                layer.edge_occurrences(id).unwrap().len(),
                edge.graph.multiplicity
            );
            for &occurrence in layer.edge_occurrences(id).unwrap() {
                assert_eq!(layer.segments()[occurrence].event, 0);
            }
        }
        assert_eq!(layer.path_vertices(0).unwrap().len(), 3);
        assert_eq!(layer.path_edges(0).unwrap().len(), 3);
        assert_eq!(layer.region_holes(0).unwrap(), [] as [usize; 0]);
        assert_eq!(layer.regions()[0].outer_boundary(), 0);
        assert!(layer.path_vertices(1).is_none());
        assert!(layer.vertex_observations(3).is_none());
        assert!(layer.edge_occurrences(3).is_none());
        assert!(layer.region_holes(1).is_none());
    }

    #[test]
    fn exact_output_cap_accepts_and_one_byte_less_rejects_before_storage() {
        let (mesh, plane, mut config) = fixture();
        let layer = generate(
            &mesh,
            plane,
            SectionTransform::Identity,
            &config,
            &mut Cancellation::never(),
        )
        .unwrap();
        config.max_output_payload_bytes = layer.payload_bytes();
        assert_eq!(
            generate(
                &mesh,
                plane,
                SectionTransform::Identity,
                &config,
                &mut Cancellation::never()
            )
            .unwrap(),
            layer
        );
        config.max_output_payload_bytes -= 1;
        let mut predicate = |point| {
            assert_ne!(point, Checkpoint::LayerStorage);
            false
        };
        assert!(matches!(
            generate(
                &mesh,
                plane,
                SectionTransform::Identity,
                &config,
                &mut Cancellation::new(&mut predicate)
            ),
            Err(LayerFailure::OutputLimit { .. })
        ));
    }

    #[test]
    fn projection_cancellation_returns_no_owned_layer() {
        let (mesh, plane, config) = fixture();
        for checkpoint in [Checkpoint::LayerStorage, Checkpoint::LayerOutput] {
            let mut predicate = |point| point == checkpoint;
            let result = generate(
                &mesh,
                plane,
                SectionTransform::Identity,
                &config,
                &mut Cancellation::new(&mut predicate),
            );
            assert!(
                matches!(result, Err(LayerFailure::Cancelled(cancelled)) if cancelled.checkpoint == checkpoint)
            );
        }
    }
}
