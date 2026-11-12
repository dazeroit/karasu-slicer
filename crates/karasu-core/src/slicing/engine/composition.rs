//! Explicit material labels bridge immutable layer provenance to exact polygon construction.
use super::RegionLayer;
use crate::{
    cancellation::{Cancellation, Cancelled, Checkpoint},
    slicing::{
        construct::{
            difference_material_rings_with_cancellation,
            difference_represented_material_rings_with_cancellation,
            prepared_material_rings_with_cancellation,
            prepared_represented_material_rings_with_cancellation,
            union_material_rings_with_cancellation,
            union_represented_material_rings_with_cancellation, ConstructError, ConstructLimits,
            ConstructedMaterial, MaterialRing, RepresentedMaterialRing,
        },
        grid::GridVertexOutcome,
        normalize::CoordinateAgreement,
        validity::EmbeddingStatus,
    },
};
use std::{error::Error, fmt, mem::size_of, ops::Range};

/// Failures of source-labelled composition; no partial constructed material is published.
#[derive(Debug, PartialEq)]
#[non_exhaustive]
pub enum LayerCompositionError {
    /// Cancellation observed during adapter work.
    Cancelled(Cancelled),
    /// Exact polygon construction failed.
    Construct(ConstructError),
    /// Face labels must cover the complete original face buffer.
    FaceMaterialCount {
        /// Required number of labels.
        expected: usize,
        /// Supplied number of labels.
        actual: usize,
    },
    /// Complete incidence or coordinate context is unresolved.
    UnresolvedSource,
    /// Open, graph-ambiguous, or nonsimple paths cannot define material.
    IneligiblePath {
        /// Original path index.
        path: usize,
    },
    /// Contributions to a path name different material sources.
    MixedPathMaterials {
        /// Original path index.
        path: usize,
    },
    /// A retained provenance reference is inconsistent.
    InvalidProvenance,
    /// Checked input counts or scratch bytes overflowed.
    SizeOverflow,
    /// A fallible scratch allocation failed.
    Allocation,
    /// Input boundary edge count exceeds its independent cap.
    InputEdgeLimit {
        /// Caller-selected edge cap.
        limit: usize,
    },
    /// Scratch storage exceeds explicit resource policy.
    PayloadLimit {
        /// Caller-selected byte cap.
        limit: usize,
    },
}
impl fmt::Display for LayerCompositionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled(e) => e.fmt(f),
            Self::Construct(e) => e.fmt(f),
            Self::FaceMaterialCount { expected, actual } => write!(
                f,
                "expected {expected} face material labels, received {actual}"
            ),
            Self::UnresolvedSource => f.write_str(
                "unresolved source incidence or coordinates prevent material composition",
            ),
            Self::IneligiblePath { path } => {
                write!(f, "path {path} is not a simple closed material boundary")
            }
            Self::MixedPathMaterials { path } => {
                write!(f, "path {path} contains different material source labels")
            }
            Self::InvalidProvenance => {
                f.write_str("inconsistent layer provenance during material composition")
            }
            Self::SizeOverflow => f.write_str("material composition scratch size overflow"),
            Self::Allocation => f.write_str("unable to allocate material composition scratch"),
            Self::InputEdgeLimit { limit } => write!(
                f,
                "material composition input edge limit exceeded ({limit})"
            ),
            Self::PayloadLimit { limit } => write!(
                f,
                "material composition scratch limit exceeded ({limit} bytes)"
            ),
        }
    }
}
impl Error for LayerCompositionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Cancelled(e) => Some(e),
            Self::Construct(e) => Some(e),
            _ => None,
        }
    }
}
impl From<Cancelled> for LayerCompositionError {
    fn from(e: Cancelled) -> Self {
        Self::Cancelled(e)
    }
}
/// Explicit material composition semantics, independent of strict layer diagnostics.
#[derive(Clone, Copy)]
pub(super) enum CompositionMode {
    Union,
    Difference,
    Prepared,
}
struct RingPlan {
    vertices: Range<usize>,
    material: u64,
    source_path: usize,
}
fn reserve<T>(count: usize) -> Result<Vec<T>, LayerCompositionError> {
    let mut result = Vec::new();
    result
        .try_reserve_exact(count)
        .map_err(|_| LayerCompositionError::Allocation)?;
    Ok(result)
}

/// Compose reviewed boundaries with parity within each explicit source and union between sources.
///
/// Represented-coordinate layers use lossless dyadic encoding and retain physical exponent metadata;
/// exact-grid layers retain caller grid units. Unsupported dyadic ranges fail without snapping.
///
/// Every face label is supplied by the caller; topology, winding and connectivity never infer source
/// ownership. Tangent-only context contributes no material. Cross-path contacts are permitted, but
/// unresolved incidence, conflicting coordinates, open paths and nonsimple paths are rejected.
/// Scratch is flat and bounded by construction input/payload limits. Original layer data and strict
/// completeness remain immutable and retain all provenance and diagnostics.
///
/// # Errors
/// Returns typed errors for incomplete or mixed source labels, unresolved source context,
/// ineligible paths, inconsistent provenance, resource limits, cancellation or exact construction
/// failure. No partial material result escapes.
pub fn compose_layer_material(
    layer: &RegionLayer,
    face_materials: &[u64],
    limits: ConstructLimits,
    cancel: &mut Cancellation<'_>,
) -> Result<ConstructedMaterial, LayerCompositionError> {
    compose_layer_with_mode(
        layer,
        face_materials,
        limits,
        cancel,
        CompositionMode::Union,
    )
}

/// Subtract the union of nonzero material sources from material zero.
///
/// Each label has even-odd interior semantics, including label zero. The adapter
/// retains the same provenance, coordinate, resource and eligibility requirements
/// as [`compose_layer_material`]. Original diagnostics remain immutable.
///
/// # Errors
/// Returns the same typed adapter and construction failures as
/// [`compose_layer_material`], without publishing partial material.
pub fn compose_layer_difference(
    layer: &RegionLayer,
    face_materials: &[u64],
    limits: ConstructLimits,
    cancel: &mut Cancellation<'_>,
) -> Result<ConstructedMaterial, LayerCompositionError> {
    compose_layer_with_mode(
        layer,
        face_materials,
        limits,
        cancel,
        CompositionMode::Difference,
    )
}

/// Compose prepared body, cutters and additive components from explicit face labels.
///
/// ID zero denotes body, nonzero IDs below `2^63` denote cutters, and IDs with the high bit set
/// denote additive components. Each ID uses parity. The result is `(body minus union cutters)
/// union additives`, so cutters cannot remove support material and supports survive absent body.
/// Ownership is never inferred from winding or topology. The requirements and preserved source
/// diagnostics are identical to [`compose_layer_material`].
/// # Errors
/// Returns the same typed adapter and construction failures without partial material.
pub fn compose_layer_prepared(
    layer: &RegionLayer,
    face_materials: &[u64],
    limits: ConstructLimits,
    cancel: &mut Cancellation<'_>,
) -> Result<ConstructedMaterial, LayerCompositionError> {
    compose_layer_with_mode(
        layer,
        face_materials,
        limits,
        cancel,
        CompositionMode::Prepared,
    )
}

pub(super) fn compose_layer_with_mode(
    layer: &RegionLayer,
    face_materials: &[u64],
    limits: ConstructLimits,
    cancel: &mut Cancellation<'_>,
    mode: CompositionMode,
) -> Result<ConstructedMaterial, LayerCompositionError> {
    cancel.check(Checkpoint::ConstructInput)?;
    let expected = layer.raw().summary().source_faces;
    if face_materials.len() != expected {
        return Err(LayerCompositionError::FaceMaterialCount {
            expected,
            actual: face_materials.len(),
        });
    }
    let summary = layer.summary().normalization;
    if summary.unresolved_groups != 0 || summary.coordinate_conflicts != 0 {
        return Err(LayerCompositionError::UnresolvedSource);
    }
    let vertex_count = input_count(layer, limits, cancel)?;
    if layer.uses_represented_coordinates() {
        return compose_represented(layer, face_materials, limits, vertex_count, cancel, mode);
    }
    let scratch_bytes = vertex_count
        .checked_mul(size_of::<[i64; 2]>())
        .and_then(|n| {
            layer
                .paths()
                .len()
                .checked_mul(size_of::<RingPlan>() + size_of::<MaterialRing<'_>>())
                .and_then(|m| n.checked_add(m))
        })
        .filter(|n| isize::try_from(*n).is_ok())
        .ok_or(LayerCompositionError::SizeOverflow)?;
    if scratch_bytes > limits.max_payload_bytes {
        return Err(LayerCompositionError::PayloadLimit {
            limit: limits.max_payload_bytes,
        });
    }
    let mut vertices = reserve(vertex_count)?;
    let mut plans = reserve(layer.paths().len())?;
    for path in 0..layer.paths().len() {
        cancel.check(Checkpoint::ConstructInput)?;
        let material = path_material(layer, path, face_materials, cancel)?;
        let start = vertices.len();
        for &vertex in layer
            .path_vertices(path)
            .ok_or(LayerCompositionError::InvalidProvenance)?
        {
            cancel.check(Checkpoint::ConstructInput)?;
            let GridVertexOutcome::Exact { xy } = layer
                .vertices()
                .get(vertex)
                .ok_or(LayerCompositionError::InvalidProvenance)?
                .grid
            else {
                return Err(LayerCompositionError::UnresolvedSource);
            };
            vertices.push(xy);
        }
        plans.push(RingPlan {
            vertices: start..vertices.len(),
            material,
            source_path: path,
        });
    }
    let mut rings = reserve(plans.len())?;
    for plan in &plans {
        cancel.check(Checkpoint::ConstructInput)?;
        rings.push(MaterialRing {
            vertices: &vertices[plan.vertices.clone()],
            material: plan.material,
            source_path: plan.source_path,
        });
    }
    let mut remaining_limits = limits;
    remaining_limits.max_payload_bytes -= scratch_bytes;
    match mode {
        CompositionMode::Union => {
            union_material_rings_with_cancellation(&rings, remaining_limits, cancel)
        }
        CompositionMode::Difference => {
            difference_material_rings_with_cancellation(&rings, remaining_limits, cancel)
        }
        CompositionMode::Prepared => {
            prepared_material_rings_with_cancellation(&rings, remaining_limits, cancel)
        }
    }
    .map_err(LayerCompositionError::Construct)
}

fn compose_represented(
    layer: &RegionLayer,
    labels: &[u64],
    limits: ConstructLimits,
    count: usize,
    cancel: &mut Cancellation<'_>,
    mode: CompositionMode,
) -> Result<ConstructedMaterial, LayerCompositionError> {
    let scratch = count
        .checked_mul(size_of::<[f64; 2]>())
        .and_then(|n| {
            layer
                .paths()
                .len()
                .checked_mul(size_of::<RingPlan>() + size_of::<RepresentedMaterialRing<'_>>())
                .and_then(|m| n.checked_add(m))
        })
        .ok_or(LayerCompositionError::SizeOverflow)?;
    isize::try_from(scratch).map_err(|_| LayerCompositionError::SizeOverflow)?;
    if scratch > limits.max_payload_bytes {
        return Err(LayerCompositionError::PayloadLimit {
            limit: limits.max_payload_bytes,
        });
    }
    let mut vertices = reserve(count)?;
    let mut plans = reserve(layer.paths().len())?;
    for path in 0..layer.paths().len() {
        cancel.check(Checkpoint::ConstructInput)?;
        let material = path_material(layer, path, labels, cancel)?;
        let start = vertices.len();
        for &vertex in layer
            .path_vertices(path)
            .ok_or(LayerCompositionError::InvalidProvenance)?
        {
            cancel.check(Checkpoint::ConstructInput)?;
            let record = layer
                .vertices()
                .get(vertex)
                .ok_or(LayerCompositionError::InvalidProvenance)?;
            let CoordinateAgreement::Agreed(point) = record.coordinates else {
                return Err(LayerCompositionError::UnresolvedSource);
            };
            if record.grid != GridVertexOutcome::Represented {
                return Err(LayerCompositionError::InvalidProvenance);
            }
            vertices.push([point.x, point.y]);
        }
        plans.push(RingPlan {
            vertices: start..vertices.len(),
            material,
            source_path: path,
        });
    }
    let mut rings = reserve(plans.len())?;
    for plan in &plans {
        cancel.check(Checkpoint::ConstructInput)?;
        rings.push(RepresentedMaterialRing {
            vertices: &vertices[plan.vertices.clone()],
            material: plan.material,
            source_path: plan.source_path,
        });
    }
    let mut remaining = limits;
    remaining.max_payload_bytes -= scratch;
    match mode {
        CompositionMode::Union => {
            union_represented_material_rings_with_cancellation(&rings, remaining, cancel)
        }
        CompositionMode::Difference => {
            difference_represented_material_rings_with_cancellation(&rings, remaining, cancel)
        }
        CompositionMode::Prepared => {
            prepared_represented_material_rings_with_cancellation(&rings, remaining, cancel)
        }
    }
    .map_err(LayerCompositionError::Construct)
}

fn input_count(
    layer: &RegionLayer,
    limits: ConstructLimits,
    cancel: &mut Cancellation<'_>,
) -> Result<usize, LayerCompositionError> {
    let mut vertex_count = 0usize;
    for (path, record) in layer.paths().iter().enumerate() {
        cancel.check(Checkpoint::ConstructInput)?;
        if record.embedding.status != EmbeddingStatus::SimpleCycle {
            return Err(LayerCompositionError::IneligiblePath { path });
        }
        vertex_count = vertex_count
            .checked_add(
                layer
                    .path_vertices(path)
                    .ok_or(LayerCompositionError::InvalidProvenance)?
                    .len(),
            )
            .ok_or(LayerCompositionError::SizeOverflow)?;
        if vertex_count > limits.max_input_edges {
            return Err(LayerCompositionError::InputEdgeLimit {
                limit: limits.max_input_edges,
            });
        }
    }
    Ok(vertex_count)
}

fn path_material(
    layer: &RegionLayer,
    path: usize,
    face_materials: &[u64],
    cancel: &mut Cancellation<'_>,
) -> Result<u64, LayerCompositionError> {
    let mut material = None;
    for &edge in layer
        .path_edges(path)
        .ok_or(LayerCompositionError::InvalidProvenance)?
    {
        for &occurrence in layer
            .edge_occurrences(edge)
            .ok_or(LayerCompositionError::InvalidProvenance)?
        {
            cancel.check(Checkpoint::ConstructInput)?;
            let segment = layer
                .segments()
                .get(occurrence)
                .ok_or(LayerCompositionError::InvalidProvenance)?;
            let event = layer
                .raw()
                .events()
                .get(segment.event)
                .ok_or(LayerCompositionError::InvalidProvenance)?;
            let label = *face_materials
                .get(event.source_face)
                .ok_or(LayerCompositionError::InvalidProvenance)?;
            if material.is_some_and(|old| old != label) {
                return Err(LayerCompositionError::MixedPathMaterials { path });
            }
            material = Some(label);
            for &endpoint in &segment.endpoints {
                let observation = layer
                    .observations()
                    .get(endpoint)
                    .ok_or(LayerCompositionError::InvalidProvenance)?;
                if observation.event != segment.event {
                    return Err(LayerCompositionError::InvalidProvenance);
                }
            }
        }
    }
    material.ok_or(LayerCompositionError::InvalidProvenance)
}
