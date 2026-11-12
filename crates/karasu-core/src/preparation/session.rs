//! Headless preparation ownership, immutable worker snapshots and current-only scene publication.
//!
//! A session binds one immutable decoded asset to its caller-asserted original-byte SHA-256.
//! It does not verify the correspondence between those bytes and the mesh. Workers own snapshots;
//! no threads, GUI framework, project persistence or source rebinding are introduced here.
use super::supports::{
    generate_supports_with_progress, SupportCertificate, SupportComponent, SupportError,
    SupportHistory, SupportLimits, SupportPlan, SupportProgress, SupportedModel,
};
use crate::{
    cancellation::{Cancellation, Cancelled, Checkpoint},
    mesh::TriangleMesh,
};
use std::{error::Error, fmt, sync::Arc};

/// Lifecycle of the current requested revision; a retained preview may belong to an older revision.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreparationStatus {
    /// Edited or never evaluated.
    Dirty,
    /// A snapshot was issued and has not been accepted.
    Pending,
    /// The newest requested job produced the current validated scene.
    Ready,
    /// The current job failed without replacing the last completed scene.
    Failed,
    /// Cancellation was observed without replacing the last completed scene.
    Cancelled,
}

/// Structured lifecycle errors retain the underlying preparation or cancellation cause.
#[derive(Debug)]
pub enum SessionError {
    /// The proposed plan names another asset; create a separate session for it.
    SourceMismatch,
    /// The outcome belongs to a different session, even if its content matches.
    ForeignOutcome,
    /// A newer edit or request superseded this outcome.
    StaleOutcome,
    /// Current-only geometry is unavailable in this lifecycle state.
    NotCurrent {
        /// Lifecycle state that prevents current-only use.
        status: PreparationStatus,
    },
    /// The monotonic request counter cannot advance.
    RequestExhausted,
    /// A preparation/history operation failed.
    Preparation(SupportError),
    /// Cancellation was observed in the session boundary itself.
    Cancelled(Cancelled),
}
impl fmt::Display for SessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SourceMismatch => {
                f.write_str("preparation plan names a different original asset")
            }
            Self::ForeignOutcome => f.write_str("preparation outcome belongs to another session"),
            Self::StaleOutcome => {
                f.write_str("preparation outcome was superseded by an edit or request")
            }
            Self::NotCurrent { status } => {
                write!(f, "current prepared scene is unavailable ({status:?})")
            }
            Self::RequestExhausted => f.write_str("preparation request counter exhausted"),
            Self::Preparation(error) => write!(f, "preparation session: {error}"),
            Self::Cancelled(error) => error.fmt(f),
        }
    }
}
impl Error for SessionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Preparation(error) => Some(error),
            Self::Cancelled(error) => Some(error),
            _ => None,
        }
    }
}
impl From<SupportError> for SessionError {
    fn from(value: SupportError) -> Self {
        Self::Preparation(value)
    }
}
impl From<Cancelled> for SessionError {
    fn from(value: Cancelled) -> Self {
        Self::Cancelled(value)
    }
}

#[derive(Debug)]
struct Ticket {
    owner: Arc<()>,
    generation: u64,
    request: u64,
    plan: SupportPlan,
}
/// Owned worker input. Only evaluation can produce its corresponding opaque outcome.
#[derive(Debug)]
pub struct PreparationTask {
    ticket: Ticket,
    source: Arc<TriangleMesh>,
    limits: SupportLimits,
}
impl PreparationTask {
    /// Immutable recipe snapshot for progress/UI correlation; it cannot edit the session.
    #[must_use]
    pub fn plan(&self) -> &SupportPlan {
        &self.ticket.plan
    }
    /// Session-local generation captured before work starts.
    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.ticket.generation
    }
    /// Monotonic request captured before work starts, distinct from edit generation.
    #[must_use]
    pub const fn request(&self) -> u64 {
        self.ticket.request
    }
    /// Evaluate on the caller's chosen thread, preserving typed failures in an opaque outcome.
    #[must_use]
    pub fn evaluate(self, cancel: &mut Cancellation<'_>) -> PreparationOutcome {
        self.evaluate_with_progress(cancel, &mut |_| {})
    }
    /// Evaluate with advisory snapshot progress. Complete does not mean session publication;
    /// acceptance must still check the current revision.
    #[must_use]
    pub fn evaluate_with_progress(
        self,
        cancel: &mut Cancellation<'_>,
        observer: &mut dyn FnMut(SupportProgress),
    ) -> PreparationOutcome {
        let result = generate_supports_with_progress(
            &self.source,
            &self.ticket.plan,
            self.limits,
            cancel,
            observer,
        );
        PreparationOutcome {
            ticket: self.ticket,
            result,
        }
    }
}
/// Unforgeable evaluation result associated with its exact source, owner, generation and request.
#[derive(Debug)]
pub struct PreparationOutcome {
    ticket: Ticket,
    result: Result<SupportedModel, SupportError>,
}
impl PreparationOutcome {
    /// Inspect a worker failure without discarding the outcome needed for lifecycle publication.
    #[must_use]
    pub fn error(&self) -> Option<&SupportError> {
        self.result.as_ref().err()
    }
    /// Request identity for bounded caller-owned progress/diagnostic records.
    #[must_use]
    pub const fn request(&self) -> u64 {
        self.ticket.request
    }
}

#[derive(Debug)]
struct Completed {
    scene: SupportedModel,
    generation: u64,
    request: u64,
}
/// Preview-only retained geometry. It deliberately exposes no scene or layer/export engine.
#[derive(Debug)]
pub struct RetainedPreparation<'a> {
    completed: &'a Completed,
    current: bool,
}
impl RetainedPreparation<'_> {
    /// Whether this preview is also the newest ready revision.
    #[must_use]
    pub const fn is_current(&self) -> bool {
        self.current
    }
    /// Generation that produced this preview; compare only within its owning session.
    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.completed.generation
    }
    /// Request that produced this preview.
    #[must_use]
    pub const fn request(&self) -> u64 {
        self.completed.request
    }
    /// Immutable recipe snapshot for retained preview controls and original placement.
    #[must_use]
    pub fn plan(&self) -> &SupportPlan {
        self.completed.scene.plan()
    }
    /// Exact canonical recipe record for the retained geometry.
    #[must_use]
    pub fn plan_record(&self) -> &[u8] {
        self.completed.scene.plan_record()
    }
    /// Explicit convex support components, not a fused material boundary.
    #[must_use]
    pub fn components(&self) -> &[SupportComponent] {
        self.completed.scene.components()
    }
    /// Retained geometric contact certificates, without a physical-strength guarantee.
    #[must_use]
    pub fn certificates(&self) -> &[SupportCertificate] {
        self.completed.scene.certificates()
    }
    /// Optional retained cavity boundary for a hollow model.
    #[must_use]
    pub fn cavity(&self) -> Option<&TriangleMesh> {
        self.completed
            .scene
            .drained_model()
            .map(|model| model.hollowed_model().cavity())
    }
}

/// One immutable asset, bounded edit history and at most one retained completed scene.
///
/// Worker snapshots may coexist externally and are bounded by the caller's worker policy. Starting
/// newer work invalidates current-only access immediately, even for an unchanged recipe: resource
/// policy retries must not let an older job overwrite the newest requested result. Existing borrowed
/// scenes cannot overlap mutable edits through safe Rust's borrow rules.
#[derive(Debug)]
pub struct PreparationSession {
    source: Arc<TriangleMesh>,
    source_sha256: [u8; 32],
    owner: Arc<()>,
    history: SupportHistory,
    latest_request: u64,
    status: PreparationStatus,
    completed: Option<Completed>,
}
impl PreparationSession {
    /// Bind one decoded source and admitted initial plan to a bounded immutable history.
    ///
    /// The plan's source digest is an asset-loader assertion, not a hash of this decoded mesh.
    /// # Errors
    /// Rejects history count, requested-capacity or allocation admission failures.
    pub fn new(
        source: Arc<TriangleMesh>,
        initial: SupportPlan,
        max_snapshots: usize,
        max_history_bytes: usize,
    ) -> Result<Self, SessionError> {
        let source_sha256 = initial.model().source().original_sha256;
        let history = SupportHistory::new(initial, max_snapshots, max_history_bytes)?;
        Ok(Self {
            source,
            source_sha256,
            owner: Arc::new(()),
            history,
            latest_request: 0,
            status: PreparationStatus::Dirty,
            completed: None,
        })
    }
    /// Current immutable plan; no mutable history escape is provided.
    #[must_use]
    pub fn plan(&self) -> &SupportPlan {
        self.history.current()
    }
    /// Owner-local edit generation, including undo and redo.
    #[must_use]
    pub fn generation(&self) -> u64 {
        self.history.generation()
    }
    /// Current lifecycle state.
    #[must_use]
    pub const fn status(&self) -> PreparationStatus {
        self.status
    }
    /// Apply a complete replacement plan atomically; canonical-identical edits are no-ops.
    /// # Errors
    /// Rejects another original asset, cancellation or the underlying history admission error.
    pub fn apply(
        &mut self,
        plan: SupportPlan,
        cancel: &mut Cancellation<'_>,
    ) -> Result<bool, SessionError> {
        cancel.check(Checkpoint::SessionStart)?;
        if plan.model().source().original_sha256 != self.source_sha256 {
            return Err(SessionError::SourceMismatch);
        }
        let changed = self.history.apply_with_cancellation(plan, cancel)?;
        if changed {
            self.status = PreparationStatus::Dirty;
        }
        Ok(changed)
    }
    /// Restore the previous plan without treating retained geometry as current.
    /// # Errors
    /// Preserves history and lifecycle on cancellation or history failure.
    pub fn undo(&mut self, cancel: &mut Cancellation<'_>) -> Result<bool, SessionError> {
        cancel.check(Checkpoint::SessionStart)?;
        let changed = self.history.undo_with_cancellation(cancel)?;
        if changed {
            self.status = PreparationStatus::Dirty;
        }
        Ok(changed)
    }
    /// Restore the following retained plan, requiring a fresh evaluation for current-only access.
    /// # Errors
    /// Preserves history and lifecycle on cancellation or history failure.
    pub fn redo(&mut self, cancel: &mut Cancellation<'_>) -> Result<bool, SessionError> {
        cancel.check(Checkpoint::SessionStart)?;
        let changed = self.history.redo_with_cancellation(cancel)?;
        if changed {
            self.status = PreparationStatus::Dirty;
        }
        Ok(changed)
    }
    /// Issue an owned worker snapshot; a newer request supersedes all previous requests.
    /// # Errors
    /// Cancellation or request-counter exhaustion leaves lifecycle and completed geometry intact.
    pub fn begin(
        &mut self,
        limits: SupportLimits,
        cancel: &mut Cancellation<'_>,
    ) -> Result<PreparationTask, SessionError> {
        cancel.check(Checkpoint::SessionStart)?;
        let request = self
            .latest_request
            .checked_add(1)
            .ok_or(SessionError::RequestExhausted)?;
        let task = PreparationTask {
            ticket: Ticket {
                owner: self.owner.clone(),
                generation: self.generation(),
                request,
                plan: self.plan().clone(),
            },
            source: self.source.clone(),
            limits,
        };
        cancel.check(Checkpoint::SessionPublish)?;
        self.latest_request = request;
        self.status = PreparationStatus::Pending;
        Ok(task)
    }
    /// Publish only a result from this session's newest exact revision/request.
    /// # Errors
    /// Foreign/stale outcomes leave all state intact. Current failures and observed cancellation
    /// mark the revision unavailable and retain the previous completed preview. Original evaluation
    /// errors remain available through the error source chain.
    pub fn accept(
        &mut self,
        outcome: PreparationOutcome,
        cancel: &mut Cancellation<'_>,
    ) -> Result<(), SessionError> {
        if !Arc::ptr_eq(&outcome.ticket.owner, &self.owner) {
            return Err(SessionError::ForeignOutcome);
        }
        if outcome.ticket.generation != self.generation()
            || outcome.ticket.request != self.latest_request
            || outcome.ticket.plan.canonical_record() != self.plan().canonical_record()
        {
            return Err(SessionError::StaleOutcome);
        }
        if let Err(error) = cancel.check(Checkpoint::SessionStart) {
            self.status = PreparationStatus::Cancelled;
            return Err(error.into());
        }
        let scene = match outcome.result {
            Ok(scene) => scene,
            Err(error) => {
                let mut cause: &(dyn Error + 'static) = &error;
                let mut cancelled = cause.is::<Cancelled>();
                while let Some(next) = cause.source() {
                    cancelled |= next.is::<Cancelled>();
                    cause = next;
                }
                self.status = if cancelled {
                    PreparationStatus::Cancelled
                } else {
                    PreparationStatus::Failed
                };
                return Err(error.into());
            }
        };
        if let Err(error) = cancel.check(Checkpoint::SessionPublish) {
            self.status = PreparationStatus::Cancelled;
            return Err(error.into());
        }
        self.completed = Some(Completed {
            scene,
            generation: outcome.ticket.generation,
            request: outcome.ticket.request,
        });
        self.status = PreparationStatus::Ready;
        Ok(())
    }
    /// Evaluate synchronously using exactly the same worker-snapshot and publication gates.
    /// # Errors
    /// Propagates begin, evaluation or publication errors while retaining the last completed preview.
    pub fn recompute(
        &mut self,
        limits: SupportLimits,
        cancel: &mut Cancellation<'_>,
    ) -> Result<(), SessionError> {
        let task = self.begin(limits, cancel)?;
        let outcome = task.evaluate(cancel);
        self.accept(outcome, cancel)
    }
    /// Return only the newest successfully evaluated current scene for slicing/export.
    /// # Errors
    /// Returns `NotCurrent` after edits, pending requests, failures or cancellation.
    pub fn current_scene(&self) -> Result<&SupportedModel, SessionError> {
        if let Some(completed) = &self.completed {
            if self.status == PreparationStatus::Ready
                && completed.generation == self.generation()
                && completed.request == self.latest_request
                && completed.scene.plan_record() == self.plan().canonical_record()
            {
                return Ok(&completed.scene);
            }
        }
        Err(SessionError::NotCurrent {
            status: self.status,
        })
    }
    /// Last successful preview, explicitly labelled current/stale; never a slice/export capability.
    #[must_use]
    pub fn last_completed(&self) -> Option<RetainedPreparation<'_>> {
        self.completed
            .as_ref()
            .map(|completed| RetainedPreparation {
                completed,
                current: self.current_scene().is_ok(),
            })
    }
}
