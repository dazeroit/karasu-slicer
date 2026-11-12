//! Background jobs (M6.1b, TD-071): a plate request sliced on its own thread, with progress
//! events, cooperative cancellation and the plate revision it was built from. A
//! [`PreparationJob`] prepares a request for display the same way, without slicing (M6.4a,
//! TD-082).
//!
//! The job owns its request: the embedded models and profiles are shared, never borrowed from the
//! project, so the project can be edited while the job runs. Whether the result still describes
//! the plate is decided when it arrives, with [`crate::project::Project::freshness`].
use crate::{
    pipeline::{self, Observer, PipelineCache, PipelineError, Preparation, SliceOutcome},
    project::PlateId,
    request::SliceRequest,
};
use karasu_core::raster::{mask::SpanMask, RasterImage};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender, TryRecvError},
        Arc,
    },
    thread::{self, JoinHandle},
};

/// Progress of a running job.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Progress {
    /// A named stage began.
    Phase(String),
    /// `completed` of `total` layers are done.
    Layers {
        /// Layers produced and exported, in order.
        completed: usize,
        /// Layers of the job.
        total: usize,
    },
}

/// Why a job produced no output.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JobError {
    /// [`Job::cancel`] was called before the output was published.
    Cancelled,
    /// The pipeline failed.
    Failed(PipelineError),
    /// The job's thread stopped without a result (a violated internal invariant).
    Lost,
}

/// One exported layer's mask, kept for display.
#[derive(Clone, Debug, PartialEq)]
pub struct LayerMask {
    /// Layer index from the plate up.
    pub index: u64,
    /// Height of the layer's sampling plane, in mm.
    pub z_mm: f64,
    /// The mask, as row spans of intensities (compact: memory follows the edges, not the area).
    pub mask: SpanMask,
}

/// A finished job: the plate and revision it was built from, and its result.
#[derive(Clone, Debug)]
pub struct Finished {
    /// The plate.
    pub plate: PlateId,
    /// The plate revision the request was built from.
    pub revision: u64,
    /// The published output's outcome, or why there is none.
    pub result: Result<SliceOutcome, JobError>,
    /// The exported layers' masks, in order, when the job was started to keep them; empty
    /// otherwise.
    pub masks: Vec<LayerMask>,
}

/// An event of a running job.
#[derive(Clone, Debug)]
pub enum Event {
    /// The job progressed.
    Progress(Progress),
    /// The job ended; no event follows.
    Finished(Finished),
}

/// A plate request running on its own thread.
#[derive(Debug)]
pub struct Job {
    plate: PlateId,
    revision: u64,
    cancel: Arc<AtomicBool>,
    events: Receiver<Event>,
    thread: Option<JoinHandle<()>>,
    finished: bool,
}

impl Job {
    /// Start slicing `request`, built from `revision` of `plate`. With `keep_masks`, the result
    /// also carries every exported layer's mask.
    #[must_use]
    pub fn start(plate: PlateId, revision: u64, request: SliceRequest, keep_masks: bool) -> Self {
        Self::start_with(plate, revision, request, keep_masks, None)
    }

    /// [`Self::start`], reusing and keeping cavities in `cache` (TD-089).
    #[must_use]
    pub fn start_with(
        plate: PlateId,
        revision: u64,
        request: SliceRequest,
        keep_masks: bool,
        cache: Option<Arc<PipelineCache>>,
    ) -> Self {
        let cancel = Arc::new(AtomicBool::new(false));
        let (sender, events) = mpsc::channel();
        let flag = Arc::clone(&cancel);
        let thread = thread::spawn(move || {
            let mut observer = Forward {
                sender: sender.clone(),
                total: 0,
                masks: keep_masks.then(Vec::new),
            };
            let stop = |_| flag.load(Ordering::Acquire);
            let result = pipeline::slice_with(&request, cache.as_deref(), &mut observer, &stop)
                .map_err(|e| {
                    // A failure after a cancellation request is the cancellation it caused.
                    if flag.load(Ordering::Acquire) {
                        JobError::Cancelled
                    } else {
                        JobError::Failed(e)
                    }
                });
            let masks = if result.is_ok() {
                observer.masks.take().unwrap_or_default()
            } else {
                Vec::new()
            };
            // The receiver may be gone; the job's effect (a published file or none) stands.
            let _ = sender.send(Event::Finished(Finished {
                plate,
                revision,
                result,
                masks,
            }));
        });
        Self {
            plate,
            revision,
            cancel,
            events,
            thread: Some(thread),
            finished: false,
        }
    }

    /// The plate the job slices.
    #[must_use]
    pub fn plate(&self) -> PlateId {
        self.plate
    }

    /// The plate revision the job was built from.
    #[must_use]
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Ask the job to stop at its next cancellation checkpoint. A job already publishing its
    /// output may still finish successfully.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Release);
    }

    /// The events that arrived since the last call, without blocking. After the
    /// [`Event::Finished`] event, the list is always empty.
    pub fn poll(&mut self) -> Vec<Event> {
        let mut events = Vec::new();
        while !self.finished {
            match self.events.try_recv() {
                Ok(event) => {
                    self.finished = matches!(event, Event::Finished(_));
                    events.push(event);
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.finished = true;
                    events.push(Event::Finished(self.lost()));
                }
            }
        }
        events
    }

    /// Block until the job ends, discarding progress, and return its result.
    #[must_use]
    pub fn wait(mut self) -> Finished {
        while !self.finished {
            match self.events.recv() {
                Ok(Event::Finished(finished)) => {
                    self.finished = true;
                    self.join();
                    return finished;
                }
                Ok(Event::Progress(_)) => {}
                Err(_) => break,
            }
        }
        self.join();
        self.lost()
    }

    /// The end of a job whose thread stopped without sending a result.
    fn lost(&self) -> Finished {
        Finished {
            plate: self.plate,
            revision: self.revision,
            result: Err(JobError::Lost),
            masks: Vec::new(),
        }
    }

    fn join(&mut self) {
        if let Some(thread) = self.thread.take() {
            // A panic is already reported as a lost result.
            let _ = thread.join();
        }
    }
}

impl Drop for Job {
    /// A dropped job is cancelled; its thread finishes on its own and publishes nothing new.
    fn drop(&mut self) {
        self.cancel();
    }
}

/// Sends the pipeline's progress to the job's owner.
struct Forward {
    sender: Sender<Event>,
    total: usize,
    /// Kept masks, when asked for.
    masks: Option<Vec<LayerMask>>,
}

impl Observer for Forward {
    fn mask(&mut self, image: &RasterImage) {
        if let Some(masks) = &mut self.masks {
            masks.push(LayerMask {
                index: image.plane().index,
                z_mm: image.plane().z_mm,
                mask: image.mask().clone(),
            });
        }
    }

    fn phase(&mut self, name: &str) {
        let _ = self
            .sender
            .send(Event::Progress(Progress::Phase(name.to_owned())));
    }

    fn begin_layers(&mut self, total: usize) {
        self.total = total;
        self.layers(0);
    }

    fn layers(&mut self, completed: usize) {
        let _ = self.sender.send(Event::Progress(Progress::Layers {
            completed,
            total: self.total,
        }));
    }
}

/// A finished preparation: the plate and revision it was built from, and its result.
#[derive(Debug)]
pub struct Prepared {
    /// The plate.
    pub plate: PlateId,
    /// The plate revision the request was built from.
    pub revision: u64,
    /// Every object's preparation, or why there is none.
    pub result: Result<Preparation, JobError>,
}

/// An event of a running preparation.
#[derive(Debug)]
pub enum PreparationEvent {
    /// A named stage began.
    Phase(String),
    /// The preparation ended; no event follows.
    Finished(Prepared),
}

/// A plate request prepared for display on its own thread ([`pipeline::prepare`]).
#[derive(Debug)]
pub struct PreparationJob {
    plate: PlateId,
    revision: u64,
    cancel: Arc<AtomicBool>,
    events: Receiver<PreparationEvent>,
    thread: Option<JoinHandle<()>>,
    finished: bool,
}

/// Sends a preparation's phases to the job's owner.
struct Phases(Sender<PreparationEvent>);

impl Observer for Phases {
    fn phase(&mut self, name: &str) {
        let _ = self.0.send(PreparationEvent::Phase(name.to_owned()));
    }
    fn begin_layers(&mut self, _: usize) {}
    fn layers(&mut self, _: usize) {}
}

impl PreparationJob {
    /// Start preparing `request`, built from `revision` of `plate`.
    #[must_use]
    pub fn start(plate: PlateId, revision: u64, request: SliceRequest) -> Self {
        Self::start_with(plate, revision, request, None, Vec::new())
    }

    /// [`Self::start`], reusing and keeping cavities in `cache` (TD-089), and keeping the hole
    /// probes of the objects listed in `probes` (see [`pipeline::prepare_with`]).
    #[must_use]
    pub fn start_with(
        plate: PlateId,
        revision: u64,
        request: SliceRequest,
        cache: Option<Arc<PipelineCache>>,
        probes: Vec<usize>,
    ) -> Self {
        let cancel = Arc::new(AtomicBool::new(false));
        let (sender, events) = mpsc::channel();
        let flag = Arc::clone(&cancel);
        let thread = thread::spawn(move || {
            let stop = |_| flag.load(Ordering::Acquire);
            let result = pipeline::prepare_with(
                &request,
                cache.as_deref(),
                &probes,
                &mut Phases(sender.clone()),
                &stop,
            );
            // Objects interrupted by a cancellation fail inside the result; report the
            // cancellation itself.
            let result = if flag.load(Ordering::Acquire) {
                Err(JobError::Cancelled)
            } else {
                result.map_err(JobError::Failed)
            };
            let _ = sender.send(PreparationEvent::Finished(Prepared {
                plate,
                revision,
                result,
            }));
        });
        Self {
            plate,
            revision,
            cancel,
            events,
            thread: Some(thread),
            finished: false,
        }
    }

    /// The plate revision the job was built from.
    #[must_use]
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Ask the job to stop at its next cancellation checkpoint.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Release);
    }

    /// The events that arrived since the last call, without blocking. After the
    /// [`PreparationEvent::Finished`] event, the list is always empty.
    pub fn poll(&mut self) -> Vec<PreparationEvent> {
        let mut events = Vec::new();
        while !self.finished {
            match self.events.try_recv() {
                Ok(event) => {
                    self.finished = matches!(event, PreparationEvent::Finished(_));
                    events.push(event);
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.finished = true;
                    events.push(PreparationEvent::Finished(self.lost()));
                }
            }
        }
        events
    }

    /// Block until the job ends, discarding progress, and return its result.
    #[must_use]
    pub fn wait(mut self) -> Prepared {
        while !self.finished {
            match self.events.recv() {
                Ok(PreparationEvent::Finished(prepared)) => {
                    self.finished = true;
                    if let Some(thread) = self.thread.take() {
                        let _ = thread.join();
                    }
                    return prepared;
                }
                Ok(PreparationEvent::Phase(_)) => {}
                Err(_) => break,
            }
        }
        self.lost()
    }

    fn lost(&self) -> Prepared {
        Prepared {
            plate: self.plate,
            revision: self.revision,
            result: Err(JobError::Lost),
        }
    }
}

impl Drop for PreparationJob {
    /// A dropped preparation is cancelled; its thread finishes on its own.
    fn drop(&mut self) {
        self.cancel();
    }
}

/// One plate of a model export job: its request, its name, and where its files go.
#[derive(Clone, Debug)]
pub struct PlateExport {
    /// The plate's request ([`crate::project::Project::request`]).
    pub request: SliceRequest,
    /// The plate's name, for messages.
    pub name: String,
    /// Its files: one for the plate, or one per object (by its 0-based position; an object
    /// without one is not exported).
    pub files: PlateFiles,
}

/// Where a plate's model files go.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlateFiles {
    /// One file for the plate.
    One(std::path::PathBuf),
    /// One file for each object, by its 0-based position.
    PerModel(Vec<(usize, std::path::PathBuf)>),
}

/// An event of a running model export.
#[derive(Debug)]
pub enum ModelExportEvent {
    /// A named stage of plate `plate` (its index in the job) began.
    Phase {
        /// The plate's index among the job's plates.
        plate: usize,
        /// The stage.
        name: String,
    },
    /// Plate `plate` is done: its files and drilled models, or why none was written.
    Plate {
        /// The plate's index among the job's plates.
        plate: usize,
        /// What it wrote.
        result: Result<pipeline::ModelExportOutcome, JobError>,
    },
    /// The export ended; no event follows. `cancelled` when it was stopped.
    Finished {
        /// Whether [`ModelExportJob::cancel`] stopped it.
        cancelled: bool,
    },
}

/// Plates' models exported one after another on their own thread ([`pipeline::export_models`],
/// TD-139). A cancellation stops the plate running; its files not begun and the plates after it
/// are not written, those written stay.
#[derive(Debug)]
pub struct ModelExportJob {
    cancel: Arc<AtomicBool>,
    events: Receiver<ModelExportEvent>,
    thread: Option<JoinHandle<()>>,
    finished: bool,
}

/// Sends a plate's phases to the job's owner.
struct ExportPhases {
    sender: Sender<ModelExportEvent>,
    plate: usize,
}

impl Observer for ExportPhases {
    fn phase(&mut self, name: &str) {
        let _ = self.sender.send(ModelExportEvent::Phase {
            plate: self.plate,
            name: name.to_owned(),
        });
    }
    fn begin_layers(&mut self, _: usize) {}
    fn layers(&mut self, _: usize) {}
}

impl ModelExportJob {
    /// Start exporting `plates` as `export` says (its `object` and `files` are taken from each
    /// plate's [`PlateFiles`]), reusing cavities in `cache`.
    #[must_use]
    pub fn start(
        plates: Vec<PlateExport>,
        export: pipeline::ModelExport,
        cache: Option<Arc<PipelineCache>>,
    ) -> Self {
        let cancel = Arc::new(AtomicBool::new(false));
        let (sender, events) = mpsc::channel();
        let flag = Arc::clone(&cancel);
        let thread = thread::spawn(move || {
            let stop = |_| flag.load(Ordering::Acquire);
            for (index, plate) in plates.into_iter().enumerate() {
                if flag.load(Ordering::Acquire) {
                    break;
                }
                let (files, only) = match &plate.files {
                    PlateFiles::One(_) => (pipeline::ModelFiles::PerPlate, export.object),
                    PlateFiles::PerModel(paths) => (
                        pipeline::ModelFiles::PerModel,
                        // One model alone keeps the caller's choice of it.
                        if paths.len() == 1 {
                            Some(paths[0].0)
                        } else {
                            export.object
                        },
                    ),
                };
                let options = pipeline::ModelExport {
                    files,
                    object: only,
                    ..export
                };
                let destination = |model: Option<(usize, &str)>| match (&plate.files, model) {
                    (PlateFiles::One(path), _) => path.clone(),
                    (PlateFiles::PerModel(paths), Some((number, _))) => paths
                        .iter()
                        .find(|(i, _)| *i + 1 == number)
                        .map_or_default(|(_, p)| p.clone()),
                    (PlateFiles::PerModel(_), None) => std::path::PathBuf::new(),
                };
                let mut observer = ExportPhases {
                    sender: sender.clone(),
                    plate: index,
                };
                let result = pipeline::export_models(
                    &plate.request,
                    &options,
                    &destination,
                    cache.as_deref(),
                    &mut observer,
                    &stop,
                );
                let result = if flag.load(Ordering::Acquire) && result.is_err() {
                    Err(JobError::Cancelled)
                } else {
                    result.map_err(JobError::Failed)
                };
                let _ = sender.send(ModelExportEvent::Plate {
                    plate: index,
                    result,
                });
            }
            let _ = sender.send(ModelExportEvent::Finished {
                cancelled: flag.load(Ordering::Acquire),
            });
        });
        Self {
            cancel,
            events,
            thread: Some(thread),
            finished: false,
        }
    }

    /// Ask the job to stop at its next cancellation checkpoint.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Release);
    }

    /// Whether the job has ended (its [`ModelExportEvent::Finished`] event was polled).
    #[must_use]
    pub fn finished(&self) -> bool {
        self.finished
    }

    /// The events that arrived since the last call, without blocking. After the
    /// [`ModelExportEvent::Finished`] event, the list is always empty.
    pub fn poll(&mut self) -> Vec<ModelExportEvent> {
        let mut events = Vec::new();
        while !self.finished {
            match self.events.try_recv() {
                Ok(event) => {
                    self.finished = matches!(event, ModelExportEvent::Finished { .. });
                    events.push(event);
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.finished = true;
                    events.push(ModelExportEvent::Finished { cancelled: false });
                }
            }
        }
        if self.finished {
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
        events
    }

    /// Block until the job ends and return every event, in order.
    #[must_use]
    pub fn wait(mut self) -> Vec<ModelExportEvent> {
        let mut events = Vec::new();
        while !self.finished {
            if let Ok(event) = self.events.recv() {
                self.finished = matches!(event, ModelExportEvent::Finished { .. });
                events.push(event);
            } else {
                self.finished = true;
                events.push(ModelExportEvent::Finished { cancelled: false });
            }
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        events
    }
}

impl Drop for ModelExportJob {
    fn drop(&mut self) {
        // A job dropped unfinished stops at its next checkpoint; its thread ends on its own.
        if !self.finished {
            self.cancel();
        }
    }
}
