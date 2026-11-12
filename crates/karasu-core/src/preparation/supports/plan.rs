//! Versioned immutable support recipes; historical preparation bytes remain untouched.
use super::{
    SupportError, SupportIntersectionPolicy, SupportLimits, SupportPlan, SupportProfile,
    SupportRecipe,
};
use crate::{
    cancellation::{Cancellation, Checkpoint},
    preparation::{OperationKind, PreparationPlan},
};
use std::{mem::size_of, sync::Arc};

#[derive(Debug)]
pub(super) struct PlanData {
    model: PreparationPlan,
    recipes: Vec<SupportRecipe>,
    policy: SupportIntersectionPolicy,
    record: Vec<u8>,
    payload: usize,
    clearance_mm: f64,
    contact_tolerance_mm: f64,
}
fn invalid(reason: &'static str) -> SupportError {
    SupportError::Invalid { reason }
}
fn resource(resource: &'static str) -> SupportError {
    SupportError::Resource { resource }
}
fn coordinate(v: f64) -> bool {
    v.is_finite() && v.abs() <= 10_000.
}
fn dimension(v: f64) -> bool {
    v.is_finite() && v > 0. && v <= 1000.
}
struct Encoder<'a, 'b> {
    bytes: Vec<u8>,
    cap: usize,
    work: usize,
    max_work: usize,
    cancel: &'a mut Cancellation<'b>,
}
impl Encoder<'_, '_> {
    fn raw(&mut self, b: &[u8]) -> Result<(), SupportError> {
        self.cancel.check(Checkpoint::SupportWork)?;
        self.work = self
            .work
            .checked_add(b.len())
            .ok_or_else(|| resource("plan work"))?;
        if self.work > self.max_work {
            return Err(resource("plan work"));
        }
        let end = self
            .bytes
            .len()
            .checked_add(b.len())
            .ok_or_else(|| resource("plan bytes"))?;
        if end > self.cap {
            return Err(resource("plan bytes"));
        }
        if end > self.bytes.capacity() {
            let grown = self
                .bytes
                .capacity()
                .checked_mul(2)
                .unwrap_or(self.cap)
                .max(64)
                .min(self.cap)
                .max(end);
            self.bytes
                .try_reserve_exact(grown - self.bytes.len())
                .map_err(|_| resource("plan allocation"))?;
        }
        self.bytes.extend_from_slice(b);
        Ok(())
    }
    fn integer(&mut self, n: u64) -> Result<(), SupportError> {
        self.raw(&n.to_le_bytes())
    }
    fn count(&mut self, n: usize) -> Result<(), SupportError> {
        self.integer(u64::try_from(n).map_err(|_| resource("plan count"))?)
    }
    fn number(&mut self, n: f64) -> Result<(), SupportError> {
        self.integer(if n == 0. { 0 } else { n.to_bits() })
    }
    fn numbers(&mut self, ns: &[f64]) -> Result<(), SupportError> {
        for &n in ns {
            self.number(n)?;
        }
        Ok(())
    }
}
fn encode_recipes(e: &mut Encoder<'_, '_>, recipes: &[SupportRecipe]) -> Result<(), SupportError> {
    for recipe in recipes {
        e.integer(recipe.id)?;
        e.count(recipe.contact.source_triangle)?;
        e.numbers(&recipe.contact.position_mm)?;
        e.number(recipe.contact.maximum_penetration_mm)?;
        e.count(recipe.stations.len())?;
        for station in &recipe.stations {
            e.numbers(&station.center_mm)?;
            e.numbers(&station.rotation.quaternion_xyzw())?;
            for row in station.rotation.matrix() {
                e.numbers(&row)?;
            }
            match &station.profile {
                SupportProfile::EllipsePolygon { radii_mm, sides } => {
                    e.integer(0)?;
                    e.numbers(radii_mm)?;
                    e.integer(u64::from(*sides))?;
                }
                SupportProfile::Rectangle { size_mm } => {
                    e.integer(1)?;
                    e.numbers(size_mm)?;
                }
                SupportProfile::RegularPolygon { radius_mm, sides } => {
                    e.integer(2)?;
                    e.number(*radius_mm)?;
                    e.integer(u64::from(*sides))?;
                }
                SupportProfile::ConvexPolygon { vertices_mm } => {
                    e.integer(3)?;
                    e.count(vertices_mm.len())?;
                    for v in vertices_mm {
                        e.numbers(v)?;
                    }
                }
                SupportProfile::Point => e.integer(4)?,
            }
        }
    }
    Ok(())
}
fn validate_stations(
    recipe: &SupportRecipe,
    limits: SupportLimits,
    payload: &mut usize,
    admission_work: &mut usize,
    cancel: &mut Cancellation<'_>,
) -> Result<(), SupportError> {
    for (station_index, station) in recipe.stations.iter().enumerate() {
        cancel.check(Checkpoint::SupportWork)?;
        if !station.center_mm.into_iter().all(coordinate)
            || (station_index != 0 && station.center_mm[2] <= 0.)
            || (station_index == 0 && station.center_mm[2] != 0.)
        {
            return Err(invalid("finite station centers above platform zero"));
        }
        let count = match &station.profile {
            SupportProfile::EllipsePolygon { radii_mm, sides } => {
                if !radii_mm.iter().copied().all(dimension) {
                    return Err(invalid("positive ellipse radii"));
                }
                usize::from(*sides)
            }
            SupportProfile::RegularPolygon { radius_mm, sides } => {
                if !dimension(*radius_mm) {
                    return Err(invalid("positive polygon radius"));
                }
                usize::from(*sides)
            }
            SupportProfile::Rectangle { size_mm } => {
                if !size_mm.iter().copied().all(dimension) {
                    return Err(invalid("positive rectangle dimensions"));
                }
                4
            }
            SupportProfile::ConvexPolygon { vertices_mm } => {
                if vertices_mm.len() > limits.max_profile_vertices.min(256) {
                    return Err(resource("profile vertices"));
                }
                *admission_work = admission_work
                    .checked_add(vertices_mm.len())
                    .ok_or_else(|| resource("plan work"))?;
                if *admission_work > limits.max_work {
                    return Err(resource("plan work"));
                }
                if vertices_mm
                    .iter()
                    .flatten()
                    .any(|v| !v.is_finite() || v.abs() > 1000.)
                {
                    return Err(invalid("finite bounded polygon coordinates"));
                }
                *payload = payload
                    .checked_add(
                        vertices_mm
                            .capacity()
                            .checked_mul(size_of::<[f64; 2]>())
                            .ok_or_else(|| resource("profile bytes"))?,
                    )
                    .ok_or_else(|| resource("plan payload"))?;
                vertices_mm.len()
            }
            SupportProfile::Point => {
                if station_index + 1 != recipe.stations.len() {
                    return Err(invalid("point profile only at terminal station"));
                }
                continue;
            }
        };
        if count < 3 {
            return Err(invalid("at least three profile vertices"));
        }
        if count > limits.max_profile_vertices.min(256) {
            return Err(resource("profile vertices"));
        }
    }
    Ok(())
}
fn validate_model(
    model: &PreparationPlan,
    recipe_count: usize,
    limits: SupportLimits,
) -> Result<(), SupportError> {
    if !dimension(limits.clearance_mm) || !dimension(limits.contact_tolerance_mm) {
        return Err(invalid("positive finite support tolerances"));
    }
    if model
        .operations()
        .iter()
        .any(|op| matches!(op.kind, OperationKind::Support(_)))
    {
        return Err(invalid(
            "legacy support operations require explicit conversion",
        ));
    }
    if recipe_count > limits.max_supports {
        return Err(resource("support count"));
    }
    Ok(())
}
impl SupportPlan {
    /// Validate and freeze ordered parametric recipes against an exact upstream model revision.
    ///
    /// Coordinates are world millimeters, bounded to +/-10000; positive dimensions are at most
    /// 1000 mm. The first section center lies at platform Z zero; remaining centers lie above it.
    /// Horizontal and descending segments are permitted. Geometry validation later
    /// proves convex profiles, contact, collision and penetration; this constructor does not.
    /// Quaternion representation and emitted matrix are recorded, preserving both authoring
    /// orientation and authoritative rounded geometry. Zero signs are canonicalized.
    /// # Errors
    /// Rejects obsolete contacts, invalid parameters, legacy support operations, limits or
    /// cancellation. No partly admitted plan is returned.
    pub fn new(
        model: PreparationPlan,
        recipes: Vec<SupportRecipe>,
        policy: SupportIntersectionPolicy,
        limits: SupportLimits,
        cancel: &mut Cancellation<'_>,
    ) -> Result<Self, SupportError> {
        cancel.check(Checkpoint::SupportStart)?;
        validate_model(&model, recipes.len(), limits)?;
        let key = model.model_key();
        let mut payload = size_of::<PlanData>()
            .checked_add(model.payload_bytes())
            .and_then(|n| {
                recipes
                    .capacity()
                    .checked_mul(size_of::<SupportRecipe>())
                    .and_then(|v| n.checked_add(v))
            })
            .ok_or_else(|| resource("plan payload"))?;
        let mut admission_work = 0usize;
        for (index, recipe) in recipes.iter().enumerate() {
            cancel.check(Checkpoint::SupportWork)?;
            admission_work = admission_work
                .checked_add(index)
                .and_then(|n| n.checked_add(recipe.stations.len()))
                .and_then(|n| n.checked_add(1))
                .ok_or_else(|| resource("plan work"))?;
            if admission_work > limits.max_work {
                return Err(resource("plan work"));
            }
            if recipe.id == 0 || recipes[..index].iter().any(|r| r.id == recipe.id) {
                return Err(invalid("unique nonzero support IDs"));
            }
            if recipe.surface != key {
                return Err(SupportError::StaleAnchor { support: recipe.id });
            }
            if recipe.stations.len() < 4 {
                return Err(invalid("at least four support stations"));
            }
            if recipe.stations.len() > limits.max_stations {
                return Err(resource("station count"));
            }
            if !recipe.contact.position_mm.into_iter().all(coordinate)
                || !dimension(recipe.contact.maximum_penetration_mm)
            {
                return Err(invalid("finite contact and positive penetration"));
            }
            payload = payload
                .checked_add(
                    recipe
                        .stations
                        .capacity()
                        .checked_mul(size_of::<super::SupportStation>())
                        .ok_or_else(|| resource("station bytes"))?,
                )
                .and_then(|n| n.checked_add(model.canonical_record().len()))
                .ok_or_else(|| resource("plan payload"))?;
            validate_stations(recipe, limits, &mut payload, &mut admission_work, cancel)?;
        }
        if payload > limits.max_payload_bytes {
            return Err(resource("plan payload"));
        }
        let mut e = Encoder {
            bytes: Vec::new(),
            cap: limits.max_payload_bytes - payload,
            work: admission_work,
            max_work: limits.max_work,
            cancel,
        };
        e.raw(b"KSUP1")?;
        e.count(model.canonical_record().len())?;
        e.raw(model.canonical_record())?;
        e.integer(match policy {
            SupportIntersectionPolicy::RejectUncertified => 0,
            SupportIntersectionPolicy::Union => 1,
        })?;
        e.number(limits.clearance_mm)?;
        e.number(limits.contact_tolerance_mm)?;
        e.count(recipes.len())?;
        encode_recipes(&mut e, &recipes)?;
        payload = payload
            .checked_add(e.bytes.capacity())
            .ok_or_else(|| resource("plan payload"))?;
        if payload > limits.max_payload_bytes {
            return Err(resource("plan payload"));
        }
        e.cancel.check(Checkpoint::SupportPublish)?;
        Ok(Self {
            inner: Arc::new(PlanData {
                model,
                recipes,
                policy,
                record: e.bytes,
                payload,
                clearance_mm: limits.clearance_mm,
                contact_tolerance_mm: limits.contact_tolerance_mm,
            }),
        })
    }
    /// Immutable upstream preparation model.
    #[must_use]
    pub fn model(&self) -> &PreparationPlan {
        &self.inner.model
    }
    /// Ordered immutable manual support recipes.
    #[must_use]
    pub fn recipes(&self) -> &[SupportRecipe] {
        &self.inner.recipes
    }
    /// Explicit policy for intersections between distinct recipes.
    #[must_use]
    pub fn intersection_policy(&self) -> SupportIntersectionPolicy {
        self.inner.policy
    }
    /// Versioned deterministic content identity, not a project-file decoder contract.
    #[must_use]
    pub fn canonical_record(&self) -> &[u8] {
        &self.inner.record
    }
    /// Semantic clearance retained in the plan identity, in millimeters.
    #[must_use]
    pub fn clearance_mm(&self) -> f64 {
        self.inner.clearance_mm
    }
    /// Semantic contact tolerance retained in the plan identity, in millimeters.
    #[must_use]
    pub fn contact_tolerance_mm(&self) -> f64 {
        self.inner.contact_tolerance_mm
    }
    /// Conservative retained payload allowance, including shared model/anchor bytes repeatedly.
    #[must_use]
    pub fn payload_bytes(&self) -> usize {
        self.inner.payload
    }
}

/// Bounded immutable support-plan snapshots with transactional undo/redo.
#[derive(Debug)]
pub struct SupportHistory {
    snapshots: Vec<SupportPlan>,
    cursor: usize,
    generation: u64,
    max_snapshots: usize,
    max_payload_bytes: usize,
}
impl SupportHistory {
    /// Retain an initial snapshot within explicit count and conservative retained-byte caps.
    /// # Errors
    /// Rejects zero snapshot capacity, insufficient payload or allocation failure.
    pub fn new(
        initial: SupportPlan,
        max_snapshots: usize,
        max_payload_bytes: usize,
    ) -> Result<Self, SupportError> {
        if max_snapshots == 0 {
            return Err(resource("history snapshots"));
        }
        if initial
            .payload_bytes()
            .checked_add(size_of::<SupportPlan>())
            .is_none_or(|n| n > max_payload_bytes)
        {
            return Err(resource("history payload"));
        }
        let mut snapshots = Vec::new();
        snapshots
            .try_reserve_exact(1)
            .map_err(|_| resource("history allocation"))?;
        snapshots.push(initial);
        Ok(Self {
            snapshots,
            cursor: 0,
            generation: 0,
            max_snapshots,
            max_payload_bytes,
        })
    }
    /// Current immutable snapshot.
    #[must_use]
    pub fn current(&self) -> &SupportPlan {
        &self.snapshots[self.cursor]
    }
    /// Monotonic revision counter for each successful edit, undo or redo.
    #[must_use]
    pub fn generation(&self) -> u64 {
        self.generation
    }
    /// Publish a changed recipe snapshot, truncating redo only after all checks succeed.
    /// # Errors
    /// Rejects count, byte, revision-counter or allocation exhaustion without changing history.
    pub fn apply(&mut self, plan: SupportPlan) -> Result<bool, SupportError> {
        self.apply_with_cancellation(plan, &mut Cancellation::never())
    }
    /// Cancellable edit publication; cancellation leaves current, redo and generation unchanged.
    /// # Errors
    /// Returns the edit's resource errors or the observed cancellation checkpoint.
    pub fn apply_with_cancellation(
        &mut self,
        plan: SupportPlan,
        cancel: &mut Cancellation<'_>,
    ) -> Result<bool, SupportError> {
        cancel.check(Checkpoint::SupportStart)?;
        if plan.canonical_record() == self.current().canonical_record() {
            return Ok(false);
        }
        let count = self
            .cursor
            .checked_add(2)
            .ok_or_else(|| resource("history snapshots"))?;
        if count > self.max_snapshots {
            return Err(resource("history snapshots"));
        }
        let mut bytes = plan.payload_bytes();
        for snapshot in &self.snapshots[..=self.cursor] {
            cancel.check(Checkpoint::SupportWork)?;
            bytes = bytes
                .checked_add(snapshot.payload_bytes())
                .ok_or_else(|| resource("history payload"))?;
        }
        let bytes = count
            .max(self.snapshots.capacity())
            .checked_mul(size_of::<SupportPlan>())
            .and_then(|n| bytes.checked_add(n))
            .ok_or_else(|| resource("history payload"))?;
        if bytes > self.max_payload_bytes {
            return Err(resource("history payload"));
        }
        let generation = self
            .generation
            .checked_add(1)
            .ok_or_else(|| resource("history generation"))?;
        cancel.check(Checkpoint::SupportPublish)?;
        if count > self.snapshots.capacity() {
            self.snapshots
                .try_reserve_exact(count - self.snapshots.len())
                .map_err(|_| resource("history allocation"))?;
        }
        self.snapshots.truncate(self.cursor + 1);
        self.snapshots.push(plan);
        self.cursor += 1;
        self.generation = generation;
        Ok(true)
    }
    /// Move to the preceding snapshot, preserving redo.
    /// # Errors
    /// Reports generation exhaustion before mutation.
    pub fn undo(&mut self) -> Result<bool, SupportError> {
        self.undo_with_cancellation(&mut Cancellation::never())
    }
    /// Cancellable undo, with no state mutation on cancellation.
    /// # Errors
    /// Reports generation exhaustion or the observed cancellation checkpoint.
    pub fn undo_with_cancellation(
        &mut self,
        cancel: &mut Cancellation<'_>,
    ) -> Result<bool, SupportError> {
        cancel.check(Checkpoint::SupportStart)?;
        if self.cursor == 0 {
            return Ok(false);
        }
        let generation = self
            .generation
            .checked_add(1)
            .ok_or_else(|| resource("history generation"))?;
        cancel.check(Checkpoint::SupportPublish)?;
        self.cursor -= 1;
        self.generation = generation;
        Ok(true)
    }
    /// Move to the following retained snapshot.
    /// # Errors
    /// Reports generation exhaustion before mutation.
    pub fn redo(&mut self) -> Result<bool, SupportError> {
        self.redo_with_cancellation(&mut Cancellation::never())
    }
    /// Cancellable redo, with no state mutation on cancellation.
    /// # Errors
    /// Reports generation exhaustion or the observed cancellation checkpoint.
    pub fn redo_with_cancellation(
        &mut self,
        cancel: &mut Cancellation<'_>,
    ) -> Result<bool, SupportError> {
        cancel.check(Checkpoint::SupportStart)?;
        if self.cursor + 1 == self.snapshots.len() {
            return Ok(false);
        }
        let generation = self
            .generation
            .checked_add(1)
            .ok_or_else(|| resource("history generation"))?;
        cancel.check(Checkpoint::SupportPublish)?;
        self.cursor += 1;
        self.generation = generation;
        Ok(true)
    }
}
