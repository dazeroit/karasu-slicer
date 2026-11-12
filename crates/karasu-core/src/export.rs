//! Versioned exporter registry and transactional streaming publication of complete print artifacts.
use crate::print_job::machine::{CodecError, EncodedPayload};
use crate::{
    cancellation::{Cancellation, Cancelled, Checkpoint},
    print_job::{JobError, JobLayer, PrintJob},
    profiles::LayerSettings,
    raster::{png, RasterImage},
};
use std::{
    collections::BTreeMap,
    error::Error,
    fmt,
    fs::{self, File, OpenOptions},
    io::{self, BufWriter, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc, Arc, Mutex, PoisonError,
    },
};
mod builtin;
pub mod models;
pub use builtin::{AnycubicWorkshop, BuiltinExporter, BuiltinFormat, PixelPolicy};

/// Seekable byte sink used by versioned machine container adapters.
pub trait WriteSeek: Write + Seek {}
impl<T: Write + Seek> WriteSeek for T {}
/// Independent limits; adapter scratch includes encoded data, not the caller's resident mask.
#[derive(Clone, Copy, Debug)]
pub struct ExportLimits {
    /// Maximum number of source layers.
    pub max_layers: usize,
    /// Maximum pixels in one source layer; default accommodates the 65-MiPixel P1 panel.
    pub max_pixels: usize,
    /// Maximum one-layer encoded bytes or bounded JSON record.
    pub max_layer_bytes: usize,
    /// Maximum published machine bytes or sum of reference files.
    pub max_output_bytes: u64,
    /// Maximum adapter metadata scratch or reference manifest/profile bytes, excluding JSONL.
    pub max_metadata_bytes: usize,
}
impl Default for ExportLimits {
    fn default() -> Self {
        Self {
            max_layers: 1_000_000,
            max_pixels: 128 * 1024 * 1024,
            max_layer_bytes: 256 * 1024 * 1024,
            max_output_bytes: u64::from(u32::MAX),
            // PP1 retains about 640 bytes per layer: this admits about 100,000 layers on the P1.
            max_metadata_bytes: 64 * 1024 * 1024,
        }
    }
}
/// Structured export failure; transactional helpers leave an existing destination untouched.
#[derive(Debug)]
pub enum ExportError {
    /// Validated job/plan failure.
    Job(JobError),
    /// I/O failure with operation context and its original source.
    Io {
        /// Operation that failed.
        operation: &'static str,
        /// Original filesystem or writer error.
        source: io::Error,
    },
    /// JSON output failure.
    Json(serde_json::Error),
    /// Source layer generation failed, preserving its original error.
    Source(Box<dyn Error>),
    /// Machine encoder failure, preserving its original error.
    Encoding(Box<dyn Error>),
    /// Unsupported target, input mismatch, count, budget or publication conflict.
    Invalid(String),
    /// Cooperative export cancellation.
    Cancelled(Cancelled),
}
impl fmt::Display for ExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Job(e) => e.fmt(f),
            Self::Io { operation, source } => write!(f, "{operation}: {source}"),
            Self::Json(e) => write!(f, "export JSON: {e}"),
            Self::Source(e) => write!(f, "layer source failed: {e}"),
            Self::Encoding(e) => write!(f, "machine encoding failed: {e}"),
            Self::Invalid(s) => f.write_str(s),
            Self::Cancelled(e) => e.fmt(f),
        }
    }
}
impl Error for ExportError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Job(e) => Some(e),
            Self::Io { source, .. } => Some(source),
            Self::Json(e) => Some(e),
            Self::Source(e) | Self::Encoding(e) => Some(e.as_ref()),
            Self::Cancelled(e) => Some(e),
            Self::Invalid(_) => None,
        }
    }
}
impl From<JobError> for ExportError {
    fn from(e: JobError) -> Self {
        Self::Job(e)
    }
}
impl From<serde_json::Error> for ExportError {
    fn from(e: serde_json::Error) -> Self {
        Self::Json(e)
    }
}
impl From<Cancelled> for ExportError {
    fn from(e: Cancelled) -> Self {
        Self::Cancelled(e)
    }
}
fn invalid(s: impl Into<String>) -> ExportError {
    ExportError::Invalid(s.into())
}
fn io_error(operation: &'static str, source: io::Error) -> ExportError {
    ExportError::Io { operation, source }
}
/// Stateful format session over an explicitly borrowed seekable sink.
///
/// Direct use can leave partial bytes on error. Use [`export_machine`] for publication semantics.
pub trait ExportSession {
    /// Encode exactly one already validated layer, in source order.
    /// # Errors
    /// Rejects unrepresentable pixels/settings, limits, I/O failures or cancellation.
    fn write_layer(
        &mut self,
        layer: &JobLayer,
        image: &RasterImage,
        cancel: &mut Cancellation<'_>,
    ) -> Result<(), ExportError>;
    /// Finalize container tables/checksums only after complete source exhaustion.
    /// # Errors
    /// Rejects incomplete state, output failures or cancellation.
    fn finish(self: Box<Self>, cancel: &mut Cancellation<'_>) -> Result<(), ExportError>;
    /// A thread-safe encoder of this session's per-layer payloads, when the format encodes layers
    /// independently of the session state. The default is none: layers are encoded while written.
    fn layer_encoder(&self) -> Option<Box<dyn LayerEncoder>> {
        None
    }
    /// Write a layer encoded by [`Self::layer_encoder`], in source order, with exactly the checks
    /// and bytes of [`Self::write_layer`]; an encoding failure surfaces here, after the checks.
    /// The default ignores the payload and writes the layer normally.
    /// # Errors
    /// As [`Self::write_layer`].
    fn write_encoded(
        &mut self,
        layer: &JobLayer,
        image: &RasterImage,
        encoded: EncodedLayer,
        cancel: &mut Cancellation<'_>,
    ) -> Result<(), ExportError> {
        drop(encoded);
        self.write_layer(layer, image, cancel)
    }
}

/// Encodes one layer image into a session's format payload, on any thread.
pub trait LayerEncoder: Send + Sync {
    /// Encode `image`; failures are carried in the result and reported when it is written.
    fn encode(&self, image: &RasterImage, cancel: &mut Cancellation<'_>) -> EncodedLayer;
}

/// An encoded layer payload (or its encoding failure), opaque outside its own session.
#[derive(Debug)]
pub struct EncodedLayer {
    pub(crate) payload: Result<EncodedPayload, CodecError>,
}

/// Per-layer parallel encoding for [`export_machine_parallel`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParallelEncoding {
    /// Encoding threads; zero uses every available core.
    pub threads: usize,
    /// Memory budget of layers pulled but not yet written, in retained mask bytes
    /// (`SpanMask::retained_bytes`). At most two layers per thread are held, and always one.
    pub window_bytes: usize,
}

impl Default for ParallelEncoding {
    fn default() -> Self {
        Self {
            threads: 0,
            window_bytes: 512 << 20,
        }
    }
}
/// Extension point for independently implemented, exact versioned machine dialects.
pub trait MachineExporter {
    /// Stable versioned identity; profiles opt into this exact string.
    fn id(&self) -> &str;
    /// Single filename suffix without a dot.
    fn extension(&self) -> &str;
    /// Validate every global/resolved setting before creating output.
    /// # Errors
    /// Rejects unsupported capabilities, scalar domains, counts or configured budgets.
    fn preflight(
        &self,
        job: &PrintJob,
        limits: ExportLimits,
        cancel: &mut Cancellation<'_>,
    ) -> Result<(), ExportError>;
    /// Begin a streaming session. The implementation must respect limits and consume no masks itself.
    /// # Errors
    /// Rejects metadata, writer failures, allocation/resource limits or cancellation.
    fn begin<'a>(
        &self,
        writer: &'a mut dyn WriteSeek,
        job: &PrintJob,
        limits: ExportLimits,
        cancel: &mut Cancellation<'_>,
    ) -> Result<Box<dyn ExportSession + 'a>, ExportError>;
}
/// Application-owned registry; adding a backend does not alter profile or job schemas.
#[derive(Default)]
pub struct ExportRegistry {
    formats: Vec<Box<dyn MachineExporter>>,
}
impl ExportRegistry {
    /// Construct an empty registry for explicit registration.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            formats: Vec::new(),
        }
    }
    /// Register an implementation with unique identity and a portable suffix.
    /// # Errors
    /// Rejects duplicate/empty IDs or invalid suffixes.
    pub fn register(&mut self, format: Box<dyn MachineExporter>) -> Result<(), ExportError> {
        if format.id().is_empty()
            || format.id().len() > 128
            || !format
                .id()
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            || format.extension().is_empty()
            || !format
                .extension()
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        {
            return Err(invalid(
                "exporter ID/suffix must be portable lowercase ASCII identifiers",
            ));
        }
        if self.get(format.id()).is_some() {
            return Err(invalid("duplicate exporter ID"));
        }
        self.formats
            .try_reserve(1)
            .map_err(|_| invalid("cannot allocate exporter registry"))?;
        self.formats.push(format);
        Ok(())
    }
    /// Find a backend by exact identity; never guesses a dialect from extension.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&dyn MachineExporter> {
        self.formats
            .iter()
            .find(|x| x.id() == id)
            .map(AsRef::as_ref)
    }
    /// Stable sorted identities for UI/CLI format selection.
    #[must_use]
    pub fn ids(&self) -> Vec<&str> {
        let mut ids: Vec<_> = self.formats.iter().map(|f| f.id()).collect();
        ids.sort_unstable();
        ids
    }
}
/// Completed-artifact metadata; volume is integrated sampled mask coverage, not measured usage.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize)]
pub struct ExportSummary {
    /// Successfully validated and written layers.
    pub layers: usize,
    /// Final file bytes, or sum of files for a reference directory.
    pub output_bytes: u64,
    /// Sampled nominal geometric volume in cubic millimeters.
    pub coverage_volume_mm3: f64,
    /// Duration estimate copied from the complete job plan.
    pub estimated_time_s: f64,
}
fn preflight(job: &PrintJob, limits: ExportLimits) -> Result<(), ExportError> {
    if job.layer_count() > limits.max_layers {
        return Err(invalid("job exceeds export layer budget"));
    }
    let [w, h] = job.printer().resolution_px;
    let pixels = u64::from(w) * u64::from(h);
    if pixels > u64::try_from(limits.max_pixels).map_err(|_| invalid("pixel budget overflow"))? {
        return Err(invalid("job exceeds per-layer pixel budget"));
    }
    Ok(())
}
fn validate_image(
    job: &PrintJob,
    index: usize,
    image: &RasterImage,
) -> Result<JobLayer, ExportError> {
    let layer = job.layer(index)?;
    let plane = image.plane();
    let lcd = job
        .printer()
        .lcd_profile()
        .map_err(|e| ExportError::Job(e.into()))?;
    if image.profile() != lcd
        || plane.index != layer.index
        || plane.z_mm.to_bits() != layer.sample_z_mm.to_bits()
        || plane.nominal_height_mm.to_bits() != layer.thickness_mm.to_bits()
    {
        return Err(invalid(format!(
            "mask {index} geometry/orientation/index/sample plane/thickness differs from the job"
        )));
    }
    Ok(layer)
}
#[allow(clippy::cast_precision_loss)] // LCD axes <=65536 bound intensity sum below 2^40, exactly representable in f64.
fn volume(image: &RasterImage, layer: &JobLayer) -> f64 {
    let pitch = image.profile().pitch_mm();
    image.stats().intensity_sum as f64 / 255. * pitch[0] * pitch[1] * layer.thickness_mm
}
/// Pull, check and account for layer `index` exactly as the sequential loop does before writing.
fn pull_layer<I, E>(
    job: &PrintJob,
    index: usize,
    source: &mut I,
    total: &mut f64,
    cancel: &mut Cancellation<'_>,
) -> Result<(JobLayer, RasterImage), ExportError>
where
    I: Iterator<Item = Result<RasterImage, E>>,
    E: Error + 'static,
{
    cancel.check(Checkpoint::ExportLayer)?;
    let image = source
        .next()
        .ok_or_else(|| invalid("layer source ended before job layer count"))?
        .map_err(|e| ExportError::Source(Box::new(e)))?;
    let layer = validate_image(job, index, &image)?;
    *total += volume(&image, &layer);
    if !total.is_finite() {
        return Err(invalid("mask-integrated volume overflow"));
    }
    Ok((layer, image))
}

/// Write every job layer in order, encoding on the calling thread; returns the covered volume.
fn write_layers<I, E>(
    job: &PrintJob,
    session: &mut dyn ExportSession,
    source: &mut I,
    cancel: &mut Cancellation<'_>,
) -> Result<f64, ExportError>
where
    I: Iterator<Item = Result<RasterImage, E>>,
    E: Error + 'static,
{
    let mut total = 0.;
    for index in 0..job.layer_count() {
        let (layer, image) = pull_layer(job, index, source, &mut total, cancel)?;
        session.write_layer(&layer, &image, cancel)?;
    }
    Ok(total)
}

/// [`write_layers`] with encoding on worker threads.
///
/// The calling thread pulls and checks up to a window of layers ahead, in order, and hands each
/// image to the workers; it writes encoded layers strictly in order. A pull failure is held until
/// every earlier layer is written, so the first failure in layer order is the one returned, as in
/// [`write_layers`]. Dropping the job channel and setting `closed` stops the workers on any exit.
#[allow(clippy::too_many_arguments)] // Private pipeline step; each input is distinct.
fn write_layers_parallel<I, E>(
    job: &PrintJob,
    session: &mut dyn ExportSession,
    encoder: &dyn LayerEncoder,
    source: &mut I,
    encoding: ParallelEncoding,
    stop: &(dyn Fn(Checkpoint) -> bool + Sync),
    cancel: &mut Cancellation<'_>,
) -> Result<f64, ExportError>
where
    I: Iterator<Item = Result<RasterImage, E>>,
    E: Error + 'static,
{
    let count = job.layer_count();
    let available = std::thread::available_parallelism().map_or(1, usize::from);
    let threads = if encoding.threads == 0 {
        available
    } else {
        encoding.threads
    }
    .clamp(1, count.max(1));
    let window = 2 * threads;
    let closed = AtomicBool::new(false);
    let (job_tx, job_rx) = mpsc::channel::<(usize, Arc<RasterImage>)>();
    let job_rx = Mutex::new(job_rx);
    let (done_tx, done_rx) = mpsc::channel::<(usize, EncodedLayer)>();
    std::thread::scope(|scope| {
        for _ in 0..threads {
            let (done_tx, job_rx, closed) = (done_tx.clone(), &job_rx, &closed);
            scope.spawn(move || loop {
                let received = job_rx.lock().unwrap_or_else(PoisonError::into_inner).recv();
                let Ok((index, image)) = received else {
                    return;
                };
                let mut call = |c| stop(c) || closed.load(Ordering::Acquire);
                let payload = encoder.encode(&image, &mut Cancellation::new(&mut call));
                if done_tx.send((index, payload)).is_err() {
                    return;
                }
            });
        }
        drop(done_tx);
        let mut pending: BTreeMap<usize, (JobLayer, Arc<RasterImage>)> = BTreeMap::new();
        let mut finished: BTreeMap<usize, EncodedLayer> = BTreeMap::new();
        let (mut pulled, mut written, mut total) = (0usize, 0usize, 0.);
        let mut held_bytes = 0usize;
        let mut held: Option<ExportError> = None;
        let result = (|| {
            while written < count {
                while held.is_none()
                    && pulled < count
                    && pulled - written < window
                    && (pulled == written || held_bytes < encoding.window_bytes)
                {
                    match pull_layer(job, pulled, source, &mut total, cancel) {
                        Ok((layer, image)) => {
                            held_bytes += image.mask().retained_bytes();
                            let image = Arc::new(image);
                            job_tx
                                .send((pulled, Arc::clone(&image)))
                                .map_err(|_| invalid("layer encoding workers stopped"))?;
                            pending.insert(pulled, (layer, image));
                            pulled += 1;
                        }
                        Err(error) => held = Some(error),
                    }
                }
                if written == pulled {
                    // Every earlier layer is written: the held pull failure is next in order.
                    return Err(held
                        .take()
                        .unwrap_or_else(|| invalid("layer export pipeline stalled")));
                }
                while !finished.contains_key(&written) {
                    let (index, layer) = done_rx
                        .recv()
                        .map_err(|_| invalid("layer encoding workers stopped"))?;
                    finished.insert(index, layer);
                }
                let payload = finished
                    .remove(&written)
                    .ok_or_else(|| invalid("missing encoded layer"))?;
                let (layer, image) = pending
                    .remove(&written)
                    .ok_or_else(|| invalid("missing pending layer"))?;
                held_bytes -= image.mask().retained_bytes();
                session.write_encoded(&layer, &image, payload, cancel)?;
                written += 1;
            }
            Ok(total)
        })();
        closed.store(true, Ordering::Release);
        drop(job_tx);
        result
    })
}

/// Publish a complete staged file at `destination` without replacing anything there.
///
/// A hard link publishes atomically. File systems without hard links (FAT32 and exFAT, the usual
/// USB and SD media of resin printers) fall back to creating the destination exclusively and copying
/// the staged bytes into it, then syncing it; a failed copy removes only that newly created file.
/// The fallback is not atomic: a reader or a crash during the copy can observe a partial file.
fn publish_file(stage: &Path, destination: &Path) -> Result<(), ExportError> {
    publish_file_with(stage, destination, |from, to| fs::hard_link(from, to))
}

/// [`publish_file`] with an explicit linking step, so the copy fallback is testable on file
/// systems that do support hard links.
fn publish_file_with(
    stage: &Path,
    destination: &Path,
    hard_link: impl FnOnce(&Path, &Path) -> io::Result<()>,
) -> Result<(), ExportError> {
    let link = match hard_link(stage, destination) {
        Ok(()) => return Ok(()),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
            return Err(io_error("publish machine file without replacement", e));
        }
        Err(e) => e,
    };
    let mut target = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(|e| {
            if e.kind() == io::ErrorKind::AlreadyExists {
                io_error("publish machine file without replacement", e)
            } else {
                io_error(
                    "publish machine file (hard link and exclusive copy both failed)",
                    io::Error::new(e.kind(), format!("hard link: {link}; copy: {e}")),
                )
            }
        })?;
    let copied = File::open(stage)
        .and_then(|mut source| io::copy(&mut source, &mut target))
        .and_then(|_| target.sync_all());
    if let Err(e) = copied {
        drop(target);
        // The destination was created exclusively above, so it is this operation's own file.
        let _ = fs::remove_file(destination);
        return Err(io_error("copy machine file to its destination", e));
    }
    Ok(())
}
static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
fn stage_path(destination: &Path) -> Result<PathBuf, ExportError> {
    let parent = destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let stem = destination
        .file_name()
        .ok_or_else(|| invalid("destination must have a filename"))?;
    // A leading dot hides the staging entry on Unix-like systems.
    let mut name = std::ffi::OsString::from(".");
    name.push(stem);
    name.push(format!(
        ".karasu-partial-{}-{}",
        std::process::id(),
        NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
    ));
    Ok(parent.join(name))
}
struct Stage {
    path: PathBuf,
    directory: bool,
    published: bool,
}
impl Drop for Stage {
    fn drop(&mut self) {
        if !self.published {
            if self.directory {
                let _ = fs::remove_dir_all(&self.path);
            } else {
                let _ = fs::remove_file(&self.path);
            }
        }
    }
}
struct LimitedFile {
    file: File,
    position: u64,
    length: u64,
    limit: u64,
}
impl Write for LimitedFile {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        let count = u64::try_from(data.len())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "output count overflow"))?;
        if self
            .position
            .checked_add(count)
            .is_none_or(|end| end > self.limit)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "export output byte limit exceeded",
            ));
        }
        let n = self.file.write(data)?;
        self.position +=
            u64::try_from(n).map_err(|_| io::Error::other("written count overflow"))?;
        self.length = self.length.max(self.position);
        Ok(n)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}
impl Seek for LimitedFile {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        let position = match from {
            SeekFrom::Start(n) => i128::from(n),
            SeekFrom::Current(n) => i128::from(self.position) + i128::from(n),
            SeekFrom::End(n) => i128::from(self.length) + i128::from(n),
        };
        if position < 0 || position > i128::from(self.limit) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "export seek exceeds output budget",
            ));
        }
        let position =
            u64::try_from(position).map_err(|_| io::Error::other("seek position overflow"))?;
        self.position = self.file.seek(SeekFrom::Start(position))?;
        Ok(self.position)
    }
}
/// Stream a complete machine job to an atomically published, create-only file.
///
/// Parent directory must exist. Destination extension must match the selected backend. Cooperative
/// cancellation does not interrupt external filesystem/I/O calls. Existing files are never replaced.
/// # Errors
/// Rejects unsupported settings, mismatched/failed/extra/missing layers, resource exhaustion,
/// cancellation, I/O failures and destination conflicts. Own temporary files are cleaned on failure.
#[allow(clippy::too_many_arguments)] // Explicit source, destination, adapter and limits own distinct policies.
pub fn export_machine<I, E>(
    destination: &Path,
    job: &PrintJob,
    format: &dyn MachineExporter,
    source: I,
    limits: ExportLimits,
    cancel: &mut Cancellation<'_>,
) -> Result<ExportSummary, ExportError>
where
    I: IntoIterator<Item = Result<RasterImage, E>>,
    E: Error + 'static,
{
    export_machine_with(destination, job, format, source, limits, None, cancel)
}

/// [`export_machine`] with per-layer encoding on worker threads, for formats whose sessions
/// provide a [`LayerEncoder`] (all built-in formats); others are written sequentially.
///
/// Layers are pulled from `source`, checked and accumulated in order on the calling thread as in
/// [`export_machine`], encoded by workers, and written strictly in order, so the published bytes,
/// the summary and the first failure are identical. `stop` is polled by the encoding workers (it
/// must be callable from every thread); `cancel` keeps every sequential checkpoint.
/// # Errors
/// As [`export_machine`].
#[allow(clippy::too_many_arguments)] // As export_machine, plus the parallel encoding policy.
pub fn export_machine_parallel<I, E>(
    destination: &Path,
    job: &PrintJob,
    format: &dyn MachineExporter,
    source: I,
    limits: ExportLimits,
    encoding: ParallelEncoding,
    stop: &(dyn Fn(Checkpoint) -> bool + Sync),
    cancel: &mut Cancellation<'_>,
) -> Result<ExportSummary, ExportError>
where
    I: IntoIterator<Item = Result<RasterImage, E>>,
    E: Error + 'static,
{
    export_machine_with(
        destination,
        job,
        format,
        source,
        limits,
        Some((encoding, stop)),
        cancel,
    )
}

type ParallelPolicy<'s> = Option<(ParallelEncoding, &'s (dyn Fn(Checkpoint) -> bool + Sync))>;

#[allow(clippy::too_many_arguments)] // Shared body of the sequential and parallel exports.
fn export_machine_with<I, E>(
    destination: &Path,
    job: &PrintJob,
    format: &dyn MachineExporter,
    source: I,
    limits: ExportLimits,
    parallel: ParallelPolicy<'_>,
    cancel: &mut Cancellation<'_>,
) -> Result<ExportSummary, ExportError>
where
    I: IntoIterator<Item = Result<RasterImage, E>>,
    E: Error + 'static,
{
    cancel.check(Checkpoint::ExportStart)?;
    preflight(job, limits)?;
    if !job.printer().format_ids.iter().any(|id| id == format.id()) {
        return Err(invalid(
            "printer profile does not opt into the selected format/version",
        ));
    }
    if destination.extension().and_then(|s| s.to_str()) != Some(format.extension()) {
        return Err(invalid(
            "destination extension differs from the selected format/version",
        ));
    }
    if destination
        .try_exists()
        .map_err(|e| io_error("check destination", e))?
    {
        return Err(invalid("destination already exists; export is create-only"));
    }
    format.preflight(job, limits, cancel)?;
    let path = stage_path(destination)?;
    let file = OpenOptions::new()
        .write(true)
        .read(true)
        .create_new(true)
        .open(&path)
        .map_err(|e| io_error("create machine staging file", e))?;
    let mut stage = Stage {
        path,
        directory: false,
        published: false,
    };
    let mut writer = LimitedFile {
        file,
        position: 0,
        length: 0,
        limit: limits.max_output_bytes,
    };
    let mut session = format.begin(&mut writer, job, limits, cancel)?;
    let mut source = source.into_iter();
    let encoder = parallel.and_then(|_| session.layer_encoder());
    let total = match (parallel, encoder) {
        (Some((encoding, stop)), Some(encoder)) => write_layers_parallel(
            job,
            session.as_mut(),
            encoder.as_ref(),
            &mut source,
            encoding,
            stop,
            cancel,
        )?,
        _ => write_layers(job, session.as_mut(), &mut source, cancel)?,
    };
    if let Some(extra) = source.next() {
        match extra {
            Err(e) => return Err(ExportError::Source(Box::new(e))),
            Ok(_) => return Err(invalid("layer source contains extra layers")),
        }
    }
    cancel.check(Checkpoint::ExportFinish)?;
    session.finish(cancel)?;
    writer
        .flush()
        .map_err(|e| io_error("flush machine staging file", e))?;
    writer
        .file
        .sync_all()
        .map_err(|e| io_error("sync machine staging file", e))?;
    let bytes = writer.length;
    drop(writer);
    cancel.check(Checkpoint::ExportPublish)?;
    publish_file(&stage.path, destination)?;
    // Publication is complete. Removal failure cannot invalidate the complete destination.
    let _ = fs::remove_file(&stage.path);
    stage.published = true;
    Ok(ExportSummary {
        layers: job.layer_count(),
        output_bytes: bytes,
        coverage_volume_mm3: total,
        estimated_time_s: job.estimated_time_s(),
    })
}
/// Stream numbered PNGs plus deterministic manifest.json and per-layer layers.jsonl to a new directory.
/// # Errors
/// Returns validation, source, PNG/JSON, budget, cancellation or filesystem failures. Failed staging
/// directories are removed; an existing destination is untouched. Rename publication requires a
/// cooperative parent directory (no hostile concurrent directory substitution).
pub fn export_reference<I, E>(
    destination: &Path,
    job: &PrintJob,
    source: I,
    limits: ExportLimits,
    cancel: &mut Cancellation<'_>,
) -> Result<ExportSummary, ExportError>
where
    I: IntoIterator<Item = Result<RasterImage, E>>,
    E: Error + 'static,
{
    cancel.check(Checkpoint::ExportStart)?;
    preflight(job, limits)?;
    if destination
        .try_exists()
        .map_err(|e| io_error("check reference destination", e))?
    {
        return Err(invalid("reference destination already exists"));
    }
    let path = stage_path(destination)?;
    fs::create_dir(&path).map_err(|e| io_error("create reference staging directory", e))?;
    let mut stage = Stage {
        path,
        directory: true,
        published: false,
    };
    let layer_file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(stage.path.join("layers.jsonl"))
        .map_err(|e| io_error("create layer index", e))?;
    let mut records = BufWriter::new(layer_file);
    let mut count_bytes = 0u64;
    let mut total = 0.;
    let mut source = source.into_iter();
    for index in 0..job.layer_count() {
        cancel.check(Checkpoint::ExportLayer)?;
        let image = source
            .next()
            .ok_or_else(|| invalid("layer source ended before job layer count"))?
            .map_err(|e| ExportError::Source(Box::new(e)))?;
        let layer = validate_image(job, index, &image)?;
        total += volume(&image, &layer);
        if !total.is_finite() {
            return Err(invalid("mask-integrated volume overflow"));
        }
        let remaining = limits
            .max_output_bytes
            .checked_sub(count_bytes)
            .ok_or_else(|| invalid("reference output byte budget exceeded"))?;
        let (png_bytes, encoded) =
            write_reference_layer(&stage.path, &layer, &image, remaining, limits, cancel)?;
        count_bytes += png_bytes;
        count_bytes = count_bytes
            .checked_add(
                u64::try_from(encoded.len() + 1)
                    .map_err(|_| invalid("record byte count overflow"))?,
            )
            .ok_or_else(|| invalid("reference output byte count overflow"))?;
        if count_bytes > limits.max_output_bytes {
            return Err(invalid("reference output byte budget exceeded"));
        }
        records
            .write_all(&encoded)
            .and_then(|()| records.write_all(b"\n"))
            .map_err(|e| io_error("write layer index", e))?;
    }
    if let Some(extra) = source.next() {
        match extra {
            Err(e) => return Err(ExportError::Source(Box::new(e))),
            Ok(_) => return Err(invalid("layer source contains extra layers")),
        }
    }
    records
        .flush()
        .map_err(|e| io_error("flush layer index", e))?;
    records
        .get_ref()
        .sync_all()
        .map_err(|e| io_error("sync layer index", e))?;
    drop(records);
    cancel.check(Checkpoint::ExportFinish)?;
    let summary = ExportSummary {
        layers: job.layer_count(),
        output_bytes: count_bytes,
        coverage_volume_mm3: total,
        estimated_time_s: job.estimated_time_s(),
    };
    let output_bytes = write_reference_manifest(&stage.path, job, summary, limits)?;
    cancel.check(Checkpoint::ExportPublish)?;
    if destination
        .try_exists()
        .map_err(|e| io_error("check final reference destination", e))?
    {
        return Err(invalid("reference destination appeared during export"));
    }
    fs::rename(&stage.path, destination)
        .map_err(|e| io_error("publish complete reference directory", e))?;
    stage.published = true;
    Ok(ExportSummary {
        output_bytes,
        ..summary
    })
}
#[derive(serde::Serialize)]
struct ApplicationMetadata {
    name: &'static str,
    version: &'static str,
}
#[derive(serde::Serialize)]
struct ReferenceSummary {
    layers: usize,
    payload_bytes_excluding_manifest: u64,
    coverage_volume_mm3: f64,
    estimated_time_s: f64,
}
#[derive(serde::Serialize)]
struct ReferenceManifest<'a> {
    schema_version: u32,
    application: ApplicationMetadata,
    job_id: &'a str,
    printer: &'a crate::profiles::PrinterProfile,
    resin: &'a crate::profiles::ResinProfile,
    calibration: &'a crate::profiles::PrintSettings,
    overrides: &'a [crate::print_job::LayerOverride],
    layer_count: usize,
    layers: &'static str,
    volume_kind: &'static str,
    duration_kind: &'static str,
    summary: ReferenceSummary,
}
#[derive(serde::Serialize)]
struct ReferenceLayerRecord<'a> {
    layer: &'a JobLayer,
    image: &'a str,
    raster: RasterMetadata,
    statistics: Statistics,
    mask_crc32: u32,
}
#[derive(serde::Serialize)]
struct RasterMetadata {
    samples_per_axis: u8,
    grayscale_levels: u16,
    /// Omitted when zero, so records made without a minimum gray are unchanged.
    #[serde(skip_serializing_if = "is_zero")]
    minimum_gray: u8,
    outside: &'static str,
    compensation: CompensationMetadata,
}
#[allow(clippy::trivially_copy_pass_by_ref)] // Serde passes the field by reference.
const fn is_zero(value: &u8) -> bool {
    *value == 0
}
#[derive(serde::Serialize)]
struct CompensationMetadata {
    kind: &'static str,
    radius_px: [u16; 2],
}
impl From<crate::raster::RasterConfig> for RasterMetadata {
    fn from(config: crate::raster::RasterConfig) -> Self {
        use crate::raster::{Compensation, OutsidePolicy};
        let outside = match config.outside {
            OutsidePolicy::Reject => "reject",
            OutsidePolicy::Clip => "clip",
        };
        let (kind, radius_px) = match config.compensation {
            Compensation::None => ("none", [0, 0]),
            Compensation::Dilate(radius) => ("dilate", radius),
            Compensation::Erode(radius) => ("erode", radius),
        };
        Self {
            samples_per_axis: config.samples_per_axis,
            grayscale_levels: config.grayscale_levels,
            minimum_gray: config.minimum_gray,
            outside,
            compensation: CompensationMetadata { kind, radius_px },
        }
    }
}
#[derive(serde::Serialize)]
struct Statistics {
    nonzero_pixels: usize,
    intensity_sum: u64,
    bounds: Option<[[u32; 2]; 2]>,
}
impl From<crate::raster::PreviewStats> for Statistics {
    fn from(stats: crate::raster::PreviewStats) -> Self {
        Self {
            nonzero_pixels: stats.nonzero_pixels,
            intensity_sum: stats.intensity_sum,
            bounds: stats.bounds,
        }
    }
}
const fn crc_table() -> [u32; 256] {
    let mut table = [0; 256];
    let mut index = 0;
    let mut byte = 0_u32;
    while index < 256 {
        let mut crc = byte;
        let mut bit = 0;
        while bit < 8 {
            crc = (crc >> 1) ^ if crc & 1 != 0 { 0xedb8_8320 } else { 0 };
            bit += 1;
        }
        table[index] = crc;
        index += 1;
        byte += 1;
    }
    table
}
const CRC32_TABLE: [u32; 256] = crc_table();
/// CRC-32 of the dense row-major mask, streamed one row at a time.
fn mask_crc32(image: &RasterImage, cancel: &mut Cancellation<'_>) -> Result<u32, ExportError> {
    let mask = image.mask();
    let width = mask.width() as usize;
    let mut row = Vec::new();
    row.try_reserve_exact(width)
        .map_err(|_| invalid("reference CRC row allocation"))?;
    row.resize(width, 0);
    let mut crc = u32::MAX;
    for y in 0..mask.height() as usize {
        cancel.poll(Checkpoint::ExportLayer, y)?;
        mask.fill_row(y, &mut row);
        for &byte in &row {
            let table_index = usize::from(crc.to_le_bytes()[0] ^ byte);
            crc = (crc >> 8) ^ CRC32_TABLE[table_index];
        }
    }
    Ok(!crc)
}
/// Copy validated one/two-stage settings into fixed writer metadata without changing units.
pub(crate) fn wire_settings(settings: &LayerSettings) -> crate::print_job::machine::WireSettings {
    let mut lift = [[0.; 2]; 2];
    let mut retract = [[0.; 2]; 2];
    for (to, from) in lift.iter_mut().zip(&settings.lift) {
        *to = [from.distance_mm, from.speed_mm_s];
    }
    for (to, from) in retract.iter_mut().zip(&settings.retract) {
        *to = [from.distance_mm, from.speed_mm_s];
    }
    crate::print_job::machine::WireSettings {
        exposure_s: settings.exposure_s,
        wait_before_exposure_s: settings.wait_before_exposure_s,
        wait_after_exposure_s: settings.wait_after_exposure_s,
        wait_after_lift_s: settings.wait_after_lift_s,
        pwm: settings.pwm,
        lift,
        retract,
    }
}

fn write_reference_manifest(
    directory: &Path,
    job: &PrintJob,
    summary: ExportSummary,
    limits: ExportLimits,
) -> Result<u64, ExportError> {
    let manifest = ReferenceManifest {
        schema_version: crate::print_job::JOB_SCHEMA_VERSION,
        application: ApplicationMetadata {
            name: crate::APPLICATION_NAME,
            version: env!("CARGO_PKG_VERSION"),
        },
        job_id: job.id(),
        printer: job.printer(),
        resin: job.resin(),
        calibration: job.settings(),
        overrides: job.overrides(),
        layer_count: job.layer_count(),
        layers: "layers.jsonl",
        volume_kind: "sampled_coverage_mm3",
        duration_kind: "nominal_without_firmware_acceleration_or_sensing",
        summary: ReferenceSummary {
            layers: summary.layers,
            payload_bytes_excluding_manifest: summary.output_bytes,
            coverage_volume_mm3: summary.coverage_volume_mm3,
            estimated_time_s: summary.estimated_time_s,
        },
    };
    let remaining = limits
        .max_output_bytes
        .checked_sub(summary.output_bytes)
        .ok_or_else(|| invalid("reference output byte budget exceeded"))?;
    let metadata_limit = u64::try_from(limits.max_metadata_bytes)
        .map_err(|_| invalid("manifest byte budget overflow"))?
        .min(remaining);
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(directory.join("manifest.json"))
        .map_err(|e| io_error("create reference manifest", e))?;
    let mut file = LimitedFile {
        file,
        position: 0,
        length: 0,
        limit: metadata_limit,
    };
    serde_json::to_writer_pretty(&mut file, &manifest)?;
    file.flush()
        .map_err(|e| io_error("flush reference manifest", e))?;
    file.file
        .sync_all()
        .map_err(|e| io_error("sync reference manifest", e))?;
    let output_bytes = summary
        .output_bytes
        .checked_add(file.length)
        .ok_or_else(|| invalid("reference output byte count overflow"))?;
    drop(file);
    Ok(output_bytes)
}

fn write_reference_layer(
    directory: &Path,
    layer: &JobLayer,
    image: &RasterImage,
    remaining: u64,
    limits: ExportLimits,
    cancel: &mut Cancellation<'_>,
) -> Result<(u64, Vec<u8>), ExportError> {
    let name = format!("layer-{:08}.png", layer.index);
    let file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(directory.join(&name))
        .map_err(|e| io_error("create reference layer", e))?;
    let mut file = LimitedFile {
        file,
        position: 0,
        length: 0,
        limit: remaining,
    };
    png::write_png_with_cancellation(&mut file, image, cancel)
        .map_err(|e| ExportError::Encoding(Box::new(e)))?;
    file.flush()
        .map_err(|e| io_error("flush reference PNG", e))?;
    file.file
        .sync_all()
        .map_err(|e| io_error("sync reference PNG", e))?;
    let png_bytes = file.length;
    let record = ReferenceLayerRecord {
        layer,
        image: &name,
        raster: RasterMetadata::from(image.config()),
        statistics: Statistics::from(image.stats()),
        mask_crc32: mask_crc32(image, cancel)?,
    };
    let encoded = serde_json::to_vec(&record)?;
    if encoded.len() > limits.max_layer_bytes {
        return Err(invalid(
            "reference layer metadata exceeds record byte budget",
        ));
    }
    Ok((png_bytes, encoded))
}

#[cfg(test)]
mod publication_tests {
    use super::{publish_file_with, ExportError};
    use std::{fs, io, path::PathBuf};

    fn directory(name: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("karasu-publish-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir(&path).unwrap();
        path
    }

    fn unsupported(_: &std::path::Path, _: &std::path::Path) -> io::Result<()> {
        Err(io::Error::from(io::ErrorKind::PermissionDenied))
    }

    #[test]
    fn media_without_hard_links_publish_by_exclusive_copy() {
        let dir = directory("copy");
        let stage = dir.join(".job.ctb.karasu-partial");
        let destination = dir.join("job.ctb");
        fs::write(&stage, b"complete bytes").unwrap();
        publish_file_with(&stage, &destination, unsupported).unwrap();
        assert_eq!(fs::read(&destination).unwrap(), b"complete bytes");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_copy_fallback_never_replaces_an_existing_destination() {
        let dir = directory("exists");
        let stage = dir.join(".job.ctb.karasu-partial");
        let destination = dir.join("job.ctb");
        fs::write(&stage, b"new").unwrap();
        fs::write(&destination, b"user file").unwrap();
        let error = publish_file_with(&stage, &destination, unsupported).unwrap_err();
        assert!(matches!(error, ExportError::Io { .. }), "{error}");
        assert_eq!(fs::read(&destination).unwrap(), b"user file");
        // A missing stage makes the copy fail: the exclusively created destination is removed.
        let other = dir.join("other.ctb");
        let missing = dir.join("missing");
        assert!(publish_file_with(&missing, &other, unsupported).is_err());
        assert!(!other.exists());
        let _ = fs::remove_dir_all(&dir);
    }
}
