//! Component generation, geometric certificates and exact prepared-scene slicing.
use super::{
    geometry, profile, validation, SupportCertificate, SupportComponent, SupportError,
    SupportIntersectionPolicy, SupportLimits, SupportPlan, SupportProgress, SupportStage, Work,
};
use crate::{
    cancellation::{Cancellation, Checkpoint},
    mesh::TriangleMesh,
    preparation::{
        drainage::{drain_model, DrainedModel},
        hollow::convex_source,
    },
    slicing::{
        construct::{ConstructLimits, PREPARED_ADDITIVE_ID_START},
        engine::{LayerConfig, LayerEngine, LayerProgress},
        schedule::LayerSchedule,
        SectionTransform,
    },
};
use std::mem::size_of;
// One inline source variant avoids a separate infallible heap allocation; only one exists per scene.
#[allow(clippy::large_enum_variant)]
#[derive(Debug)]
pub(super) enum PreparedBase {
    Solid {
        mesh: TriangleMesh,
        labels: Vec<u64>,
    },
    Drained(DrainedModel),
}
impl PreparedBase {
    pub(super) fn exterior(&self) -> (&TriangleMesh, usize) {
        match self {
            Self::Solid { mesh, .. } => (mesh, mesh.triangles().len()),
            Self::Drained(model) => {
                let mesh = model.hollowed_model().mesh();
                (
                    mesh,
                    mesh.triangles().len() - model.hollowed_model().cavity().triangles().len(),
                )
            }
        }
    }
    pub(super) fn cavity(&self) -> Option<&TriangleMesh> {
        match self {
            Self::Solid { .. } => None,
            Self::Drained(model) => Some(model.hollowed_model().cavity()),
        }
    }
    pub(super) fn boundary_scene(&self) -> (&TriangleMesh, &[u64]) {
        match self {
            Self::Solid { mesh, labels } => (mesh, labels),
            Self::Drained(model) => model.boundary_scene(),
        }
    }
}
/// Immutable certified support scene. Only the prepared-material layer engine evaluates its union.
#[derive(Debug)]
pub struct SupportedModel {
    base: PreparedBase,
    components: Vec<SupportComponent>,
    certificates: Vec<SupportCertificate>,
    boundaries: TriangleMesh,
    labels: Vec<u64>,
    plan: SupportPlan,
}
impl SupportedModel {
    /// Complete immutable recipe snapshot for controls, revision inspection and preview transforms.
    #[must_use]
    pub fn plan(&self) -> &SupportPlan {
        &self.plan
    }
    /// Optional unmodified hollow/drain scene; a solid model has no invented cavity.
    #[must_use]
    pub fn drained_model(&self) -> Option<&DrainedModel> {
        if let PreparedBase::Drained(model) = &self.base {
            Some(model)
        } else {
            None
        }
    }
    /// Owned individually closed support components, not a final Boolean boundary mesh.
    #[must_use]
    pub fn components(&self) -> &[SupportComponent] {
        &self.components
    }
    /// Geometric contact certificates in recipe order, without a strength guarantee.
    #[must_use]
    pub fn certificates(&self) -> &[SupportCertificate] {
        &self.certificates
    }
    /// Full immutable recipe identity including model, profiles, orientations and semantic tolerances.
    #[must_use]
    pub fn plan_record(&self) -> &[u8] {
        self.plan.canonical_record()
    }
    /// Backend identity separate from plan schema.
    #[must_use]
    pub const fn backend_id(&self) -> &'static str {
        "convex-parametric-lofts-v1"
    }
    /// Slice `(model minus cutters) union support components` in one exact planar arrangement.
    #[must_use]
    pub fn layer_engine<'a, 'c, 'o>(
        &'a self,
        schedule: &'a LayerSchedule,
        config: LayerConfig,
        construct: ConstructLimits,
        cancel: Cancellation<'c>,
        observer: Option<&'o mut dyn FnMut(LayerProgress)>,
    ) -> LayerEngine<'a, 'c, 'o> {
        LayerEngine::new(
            &self.boundaries,
            schedule,
            SectionTransform::Identity,
            config,
            cancel,
            observer,
        )
        .with_represented_coordinates()
        .with_prepared_materials(&self.labels, construct)
    }
}
fn resource(resource: &'static str) -> SupportError {
    SupportError::Resource { resource }
}
fn reserve<T>(count: usize) -> Result<Vec<T>, SupportError> {
    let mut result = Vec::new();
    result
        .try_reserve_exact(count)
        .map_err(|_| resource("support allocation"))?;
    Ok(result)
}
fn add(first: usize, second: usize) -> Result<usize, SupportError> {
    first
        .checked_add(second)
        .ok_or_else(|| resource("support admission arithmetic"))
}
fn multiply(first: usize, second: usize) -> Result<usize, SupportError> {
    first
        .checked_mul(second)
        .ok_or_else(|| resource("support admission arithmetic"))
}
fn mesh_capacity(vertices: usize, faces: usize) -> Result<usize, SupportError> {
    // One represented vertex, one triangle and one material label per respective entity.
    add(
        multiply(vertices, size_of::<crate::geometry::Point3>())?,
        multiply(faces, 20)?,
    )
}
fn base_allowances(
    plan: &SupportPlan,
    limits: SupportLimits,
) -> Result<(usize, usize), SupportError> {
    let hollow = limits.drainage.hollow;
    let solid = plan.model().operations().is_empty();
    let holes = plan.model().operations().len().saturating_sub(1);
    let cutter_sections = multiply(holes, limits.drainage.max_segments)?;
    let vertices = hollow.max_input_vertices.max(add(
        hollow.max_output_vertices,
        multiply(cutter_sections, 2)?,
    )?);
    let faces = hollow.max_input_triangles.max(add(
        hollow.max_output_triangles,
        multiply(cutter_sections, 4)?,
    )?);
    let subordinate = if solid {
        hollow.max_payload_bytes
    } else {
        limits.drainage.max_payload_bytes
    };
    // Base processing owns its original boundary. Reserve an additional assembled copy and
    // a conservative second copy for source labels and fixed scene/mesh bookkeeping.
    let bytes = add(subordinate, multiply(mesh_capacity(vertices, faces)?, 2)?)?;
    let work = if solid {
        hollow.max_work
    } else {
        add(hollow.max_work, limits.drainage.max_work)?
    };
    Ok((bytes, work))
}
fn payload_allowance(
    plan: &SupportPlan,
    limits: SupportLimits,
    components: usize,
    stations: usize,
    base_bytes: usize,
) -> Result<usize, SupportError> {
    let component = limits.component;
    let mesh_bytes = mesh_capacity(
        component.max_output_vertices,
        component.max_output_triangles,
    )?;
    let component_bytes = add(
        add(component.max_payload_bytes, multiply(mesh_bytes, 2)?)?,
        add(
            multiply(
                component.max_input_vertices,
                size_of::<crate::geometry::Point3>(),
            )?,
            size_of::<SupportComponent>(),
        )?,
    )?;
    // Each profile allocates separate local XY and transformed XYZ buffers, plus its outer
    // Vec descriptor. Counting all stations covers cumulative requests across recipes.
    let profile_bytes = add(
        multiply(
            limits.max_profile_vertices,
            size_of::<[f64; 2]>() + size_of::<crate::geometry::Point3>(),
        )?,
        size_of::<Vec<crate::geometry::Point3>>(),
    )?;
    let records = add(
        multiply(plan.recipes().len(), size_of::<SupportCertificate>())?,
        size_of::<SupportedModel>(),
    )?;
    add(
        add(base_bytes, multiply(components, component_bytes)?)?,
        add(
            add(multiply(stations, profile_bytes)?, records)?,
            plan.canonical_record().len(),
        )?,
    )
}
fn admission(plan: &SupportPlan, limits: SupportLimits) -> Result<(usize, usize), SupportError> {
    if limits.clearance_mm.to_bits() != plan.clearance_mm().to_bits()
        || limits.contact_tolerance_mm.to_bits() != plan.contact_tolerance_mm().to_bits()
    {
        return Err(SupportError::Invalid {
            reason: "evaluation tolerances differ from immutable plan",
        });
    }
    if limits.max_profile_vertices > 256
        || limits.max_profile_vertices < 3
        || plan.recipes().len() > limits.max_supports
    {
        return Err(resource("support or profile count"));
    }
    let mut components = 0usize;
    let mut stations = 0usize;
    for recipe in plan.recipes() {
        if recipe.stations.len() > limits.max_stations {
            return Err(resource("station count"));
        }
        components = add(components, recipe.stations.len() - 1)?;
        stations = add(stations, recipe.stations.len())?;
    }
    let (base_bytes, base_work) = base_allowances(plan, limits)?;
    let payload = payload_allowance(plan, limits, components, stations, base_bytes)?;
    if payload > limits.max_payload_bytes {
        return Err(resource("aggregate support payload"));
    }
    // The quadratic edge/vertex proof dominates profile admission. Linear work includes
    // origin-side checks, transforms and exact rank probes after placement rounding.
    let profile_units = add(
        multiply(limits.max_profile_vertices, limits.max_profile_vertices)?,
        multiply(limits.max_profile_vertices, 8)?,
    )?;
    let work = add(
        add(
            multiply(components, limits.component.max_work)?,
            multiply(stations, profile_units)?,
        )?,
        base_work,
    )?;
    if work > limits.max_work {
        return Err(resource("aggregate support work"));
    }
    Ok((components, work))
}

fn base_model(
    source: &TriangleMesh,
    plan: &SupportPlan,
    limits: SupportLimits,
    cancel: &mut Cancellation<'_>,
) -> Result<PreparedBase, SupportError> {
    if plan.model().operations().is_empty() {
        let mesh = convex_source(source, plan.model(), limits.drainage.hollow, cancel)
            .map_err(SupportError::Geometry)?;
        let mut labels = reserve(mesh.triangles().len())?;
        labels.resize(mesh.triangles().len(), 0);
        Ok(PreparedBase::Solid { mesh, labels })
    } else {
        Ok(PreparedBase::Drained(
            drain_model(source, plan.model(), limits.drainage, cancel)
                .map_err(SupportError::Drainage)?,
        ))
    }
}
fn assemble(
    base: &PreparedBase,
    components: &[SupportComponent],
    work: &mut Work,
    cancel: &mut Cancellation<'_>,
) -> Result<(TriangleMesh, Vec<u64>), SupportError> {
    let (model, model_labels) = base.boundary_scene();
    let mut count = model.vertices().len();
    let mut faces = model.triangles().len();
    for part in components {
        count = count
            .checked_add(part.mesh.vertices().len())
            .ok_or_else(|| resource("scene vertices"))?;
        faces = faces
            .checked_add(part.mesh.triangles().len())
            .ok_or_else(|| resource("scene faces"))?;
    }
    let mut vertices = reserve(count)?;
    let mut triangles = reserve(faces)?;
    let mut labels = reserve(faces)?;
    for (index, mesh) in std::iter::once(model)
        .chain(components.iter().map(|part| &part.mesh))
        .enumerate()
    {
        let offset = u32::try_from(vertices.len()).map_err(|_| resource("scene index"))?;
        for &point in mesh.vertices() {
            work.tick(cancel)?;
            vertices.push(point);
        }
        for (face_id, &face) in mesh.triangles().iter().enumerate() {
            work.tick(cancel)?;
            let mut shifted = [0; 3];
            for i in 0..3 {
                shifted[i] = face[i]
                    .checked_add(offset)
                    .ok_or_else(|| resource("scene index"))?;
            }
            triangles.push(shifted);
            labels.push(if index == 0 {
                model_labels[face_id]
            } else {
                PREPARED_ADDITIVE_ID_START
                    .checked_add(u64::try_from(index - 1).map_err(|_| resource("component label"))?)
                    .ok_or_else(|| resource("component label"))?
            });
        }
    }
    Ok((
        TriangleMesh::try_new_with_cancellation(vertices, triangles, cancel)?,
        labels,
    ))
}
/// Construct explicit parametric support components and certify contact, platform and collision rules.
///
/// Sources are convex solids or M5.5-B/C models. Supports rise through independently oriented polygonal
/// stations; only their final segment may contact one exterior facet. Every other model, cavity or
/// cutter interaction must be proven separated. Uncertain separation is a typed failure, not repair.
/// Adjacent components intentionally share a full station section. Cross-support overlap is explicit.
/// # Errors
/// Returns structured parameter, stale-revision, contact, uncertain collision, resource, source,
/// component, placement and cancellation failures, without publishing a partial valid scene.
pub fn generate_supports(
    source: &TriangleMesh,
    plan: &SupportPlan,
    limits: SupportLimits,
    cancel: &mut Cancellation<'_>,
) -> Result<SupportedModel, SupportError> {
    generate_supports_with_progress(source, plan, limits, cancel, &mut |_| {})
}
fn notify(
    observer: &mut dyn FnMut(SupportProgress),
    stage: SupportStage,
    plan: &SupportPlan,
    completed_supports: usize,
    support_id: Option<u64>,
    component_index: Option<usize>,
) {
    observer(SupportProgress {
        stage,
        completed_supports,
        total_supports: plan.recipes().len(),
        support_id,
        component_index,
    });
}
/// Generate a complete immutable snapshot with advisory deterministic phase/count observations.
///
/// The observer runs synchronously and must return promptly without panicking. A Complete event
/// describes this snapshot computation, not the current revision of an external session. Original
/// input remains unchanged and no partial valid scene is exposed on failure/cancellation.
/// # Errors
/// Preserves all structured failures and cooperative cancellation of [`generate_supports`].
pub fn generate_supports_with_progress(
    source: &TriangleMesh,
    plan: &SupportPlan,
    limits: SupportLimits,
    cancel: &mut Cancellation<'_>,
    observer: &mut dyn FnMut(SupportProgress),
) -> Result<SupportedModel, SupportError> {
    cancel.check(Checkpoint::SupportStart)?;
    notify(observer, SupportStage::Admission, plan, 0, None, None);
    let (count, charged) = admission(plan, limits)?;
    notify(observer, SupportStage::Source, plan, 0, None, None);
    let base = base_model(source, plan, limits, cancel)?;
    let mut components = reserve(count)?;
    let mut certificates = reserve(plan.recipes().len())?;
    let mut work = Work {
        used: charged,
        limit: limits.max_work,
    };
    for (recipe_index, recipe) in plan.recipes().iter().enumerate() {
        notify(
            observer,
            SupportStage::Profiles,
            plan,
            recipe_index,
            Some(recipe.id),
            None,
        );
        let start = components.len();
        let mut rings = reserve(recipe.stations.len())?;
        for station in &recipe.stations {
            work.tick(cancel)?;
            rings.push(profile::ring(station, limits.max_profile_vertices, cancel)?);
        }
        for index in 0..rings.len() - 1 {
            notify(
                observer,
                SupportStage::Components,
                plan,
                recipe_index,
                Some(recipe.id),
                Some(index),
            );
            work.tick(cancel)?;
            let mesh = geometry::loft(&rings[index], &rings[index + 1], limits.component, cancel)?;
            components.push(SupportComponent {
                support_id: recipe.id,
                segment_index: index,
                mesh,
            });
        }
        notify(
            observer,
            SupportStage::Validation,
            plan,
            recipe_index,
            Some(recipe.id),
            None,
        );
        let certificate = validation::certify(
            &base,
            recipe,
            &components[start..],
            &rings,
            limits,
            &mut work,
            cancel,
        )?;
        certificates.push(certificate);
        if plan.intersection_policy() == SupportIntersectionPolicy::RejectUncertified {
            separate_existing(
                &components[..start],
                &components[start..],
                recipe.id,
                limits.clearance_mm,
                &mut work,
                cancel,
            )?;
        }
    }
    notify(
        observer,
        SupportStage::Assembly,
        plan,
        plan.recipes().len(),
        None,
        None,
    );
    let (boundaries, labels) = assemble(&base, &components, &mut work, cancel)?;
    cancel.check(Checkpoint::SupportPublish)?;
    notify(
        observer,
        SupportStage::Complete,
        plan,
        plan.recipes().len(),
        None,
        None,
    );
    Ok(SupportedModel {
        base,
        components,
        certificates,
        boundaries,
        labels,
        plan: plan.clone(),
    })
}

fn separate_existing(
    previous: &[SupportComponent],
    current: &[SupportComponent],
    support_id: u64,
    clearance_mm: f64,
    work: &mut Work,
    cancel: &mut Cancellation<'_>,
) -> Result<(), SupportError> {
    for older in previous {
        for newer in current {
            if !validation::meshes_separated(&older.mesh, &newer.mesh, clearance_mm, work, cancel)?
            {
                return Err(SupportError::Collision {
                    support: support_id,
                    component: newer.segment_index,
                    obstacle: "another support or uncertain separation",
                });
            }
        }
    }
    Ok(())
}
