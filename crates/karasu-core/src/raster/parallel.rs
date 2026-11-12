//! Parallel layer generation (TD-050): independent layers are sectioned, assembled and rasterized
//! on worker threads and delivered strictly in layer order.
//!
//! Each layer is a pure function of the immutable mesh, transform, configuration, its scheduled
//! plane (and predecessor), and the optional cut. Workers compute whole layers with the sequential
//! engine's own per-layer function, so every image is identical to the sequential stream's for any
//! thread count. A bounded reorder window keeps memory proportional to the layers in flight; the
//! first error in layer order is the one reported, as sequentially.

use super::{
    rasterize_layer_minus_rings_with_cancellation, rasterize_layer_with_cancellation,
    rasterize_winding_layer_with_cancellation, render, stream::RasterStreamError, LayerCut,
    LcdProfile, RasterConfig, RasterError, RasterImage, RasterLimits,
};
use crate::{
    cancellation::{Cancellation, Checkpoint},
    mesh::TriangleMesh,
    slicing::{
        engine::{LayerConfig, LayerEngineError, LayerFailure, LayerKernel},
        schedule::{LayerPlane, LayerSchedule},
        winding::{SectionClosure, WindingRule},
        FaceZCursor, SectionError, SectionTransform,
    },
};
use std::{
    collections::BTreeMap,
    ops::Range,
    panic::{catch_unwind, AssertUnwindSafe},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Condvar, Mutex, MutexGuard, PoisonError,
    },
};

/// One delivered layer image, or the terminal failure of that layer.
pub type ParallelItem = Result<RasterImage, RasterStreamError<LayerEngineError>>;

/// How each layer's section becomes material.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayerFill {
    /// The strict pipeline: even-odd regions of an embedded section. Layers whose contours touch,
    /// overlap or self-intersect are unresolved and fail to rasterize.
    Strict,
    /// Winding-number layers under the rule (TD-050): overlapping bodies, touching shells and
    /// self-intersections are filled by the rule; open or inconsistently oriented sections fail.
    /// Requires `vertex_avoiding_bytes`.
    Winding(WindingRule),
}

/// Inputs of a parallel layer stream; the same choices the sequential engine and raster streams
/// take, plus the fill, the thread count and the memory budget of the reorder window.
pub struct ParallelRaster<'a> {
    /// Source mesh, borrowed for the whole stream.
    pub mesh: &'a TriangleMesh,
    /// Layer planes, in order.
    pub schedule: &'a LayerSchedule,
    /// Source-to-world mapping.
    pub transform: SectionTransform<'a>,
    /// Per-layer stage configuration.
    pub layer_config: LayerConfig,
    /// Retain represented coordinates (as `LayerEngine::with_represented_coordinates`).
    pub represented_coordinates: bool,
    /// Face Z index budget in bytes (as `LayerEngine::with_face_z_index`), or `None`.
    pub face_index_bytes: Option<usize>,
    /// Vertex-avoiding planes budget (as `LayerEngine::with_vertex_avoiding_planes`), or `None`.
    pub vertex_avoiding_bytes: Option<usize>,
    /// LCD geometry.
    pub profile: LcdProfile,
    /// Raster sampling and quantization.
    pub raster: RasterConfig,
    /// A different raster configuration for the first layers (such as stronger compensation of
    /// the bottom layers), or `None` to use `raster` everywhere.
    pub bottom: Option<BottomRaster>,
    /// Raster limits, per layer.
    pub limits: RasterLimits,
    /// Analytic regions removed from every layer (drainage cutters), or `None`.
    pub cut: Option<&'a (dyn LayerCut + Sync)>,
    /// Faces of `mesh` whose material `cut` leaves (the supports, TD-123), as face index ranges
    /// in any order; empty to cut everything. Winding layers only.
    pub kept_faces: &'a [Range<usize>],
    /// Material interpretation of each layer.
    pub fill: LayerFill,
    /// Close open sections at holes in winding layers (TD-054), or `None` to fail them.
    pub closure: Option<SectionClosure>,
    /// Worker threads; zero uses every available core.
    pub threads: usize,
    /// Memory budget of finished and in-flight layers: the in-flight count is this budget over
    /// the raster preflight bytes per layer, and finished layers wait only while their retained
    /// mask bytes stay within it (the next layer in order is always admitted).
    pub window_bytes: usize,
}

impl ParallelRaster<'_> {
    /// Default reorder-window budget: 1 GiB.
    pub const DEFAULT_WINDOW_BYTES: usize = 1 << 30;
}

/// The raster configuration of the first layers of a stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BottomRaster {
    /// Layers, counted from the first schedule index, that use `raster`.
    pub layers: u64,
    /// Their raster configuration.
    pub raster: RasterConfig,
}

impl ParallelRaster<'_> {
    /// The raster configuration of the layer at schedule index `index`.
    fn raster_for(&self, index: u64) -> RasterConfig {
        match self.bottom {
            Some(bottom) if index < bottom.layers => bottom.raster,
            _ => self.raster,
        }
    }
}

/// Shared coordination state. Results wait in `results` until the consumer reaches them.
struct State {
    next_claim: usize,
    next_emit: usize,
    results: BTreeMap<usize, ParallelItem>,
    /// Retained mask bytes of the finished images in `results`.
    held_bytes: usize,
}

struct Shared {
    state: Mutex<State>,
    /// Signalled when a result arrives.
    ready: Condvar,
    /// Signalled when the window advances or the stream closes.
    space: Condvar,
    /// Layers with a larger index are abandoned: an earlier layer failed.
    abandon_after: AtomicUsize,
    /// The consumer finished or stopped early: abandon everything.
    closed: AtomicBool,
}

fn lock(state: &Mutex<State>) -> MutexGuard<'_, State> {
    // A panic never happens while the lock is held (worker panics are caught outside it).
    state.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Run `consume` over the layer images of `spec`, generated on worker threads and delivered in
/// layer order, then join the workers.
///
/// With [`LayerFill::Strict`], the returned iterator yields exactly what the sequential
/// `LayerEngine` plus `RasterStream` (or `DifferenceRasterStream` with `spec.cut`) would yield: the
/// same images, and the same first failure in layer order, after which it ends. Winding layers are
/// likewise identical for every thread count. `stop` is the caller's cancellation predicate; it
/// must be safe to call from every worker thread. Dropping the iterator early stops the workers.
pub fn with_parallel_raster<R>(
    spec: &ParallelRaster<'_>,
    stop: &(dyn Fn(Checkpoint) -> bool + Sync),
    consume: impl FnOnce(&mut dyn Iterator<Item = ParallelItem>) -> R,
) -> R {
    let planes = spec.schedule.planes();
    let first = planes.first().copied();
    // The raster stream's opening validation, then the shared kernel, on the calling thread.
    let mut call = |c| stop(c);
    let mut cancel = Cancellation::new(&mut call);
    let opening = (|| {
        cancel.check(Checkpoint::RasterStart)?;
        spec.raster.validate()?;
        let mut bytes = render::preflight(spec.profile, spec.raster, spec.limits, 0)?;
        if let Some(bottom) = spec.bottom {
            bottom.raster.validate()?;
            bytes = bytes.max(render::preflight(
                spec.profile,
                bottom.raster,
                spec.limits,
                0,
            )?);
        }
        Ok(bytes)
    })();
    let per_layer = match opening {
        Ok(bytes) => bytes.max(1),
        Err(e) => return consume(&mut std::iter::once(Err(RasterStreamError::Raster(e)))),
    };
    let kernel = LayerKernel::build(
        spec.mesh,
        spec.transform,
        &spec.layer_config,
        spec.represented_coordinates,
        spec.face_index_bytes,
        spec.vertex_avoiding_bytes,
        &mut cancel,
    )
    .and_then(|kernel| match (spec.fill, spec.closure) {
        (LayerFill::Winding(_), Some(closure)) => kernel.with_closure(closure, &mut cancel),
        _ => Ok(kernel),
    })
    .and_then(|kernel| match spec.fill {
        LayerFill::Strict if !spec.kept_faces.is_empty() => Err(LayerFailure::KeptFacesNeedWinding),
        _ => Ok(kernel.with_kept_faces(spec.kept_faces)),
    });
    let kernel = match kernel {
        Ok(kernel) => kernel,
        Err(cause) => {
            let error = LayerEngineError {
                plane: first,
                cause,
            };
            return consume(&mut std::iter::once(Err(RasterStreamError::Source(error))));
        }
    };
    let available = std::thread::available_parallelism().map_or(1, usize::from);
    let threads = if spec.threads == 0 {
        available
    } else {
        spec.threads
    }
    .clamp(1, planes.len().max(1));
    let window = (spec.window_bytes / per_layer).clamp(1, 2 * threads);
    let shared = Shared {
        state: Mutex::new(State {
            next_claim: 0,
            next_emit: 0,
            results: BTreeMap::new(),
            held_bytes: 0,
        }),
        ready: Condvar::new(),
        space: Condvar::new(),
        abandon_after: AtomicUsize::new(usize::MAX),
        closed: AtomicBool::new(false),
    };
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| worker(spec, &kernel, &shared, window, stop));
        }
        let mut stream = Ordered {
            shared: &shared,
            total: planes.len(),
            stop,
            done: false,
        };
        let result = consume(&mut stream);
        // Release workers waiting for window space and abandon unfinished layers.
        shared.closed.store(true, Ordering::Release);
        let _guard = lock(&shared.state);
        shared.space.notify_all();
        result
    })
}

/// In-order consumer side of the window.
struct Ordered<'s> {
    shared: &'s Shared,
    total: usize,
    stop: &'s (dyn Fn(Checkpoint) -> bool + Sync),
    done: bool,
}

impl Iterator for Ordered<'_> {
    type Item = ParallelItem;

    fn next(&mut self) -> Option<ParallelItem> {
        if self.done {
            return None;
        }
        let mut state = lock(&self.shared.state);
        loop {
            let key = state.next_emit;
            if let Some(item) = state.results.remove(&key) {
                state.next_emit += 1;
                state.held_bytes -= retained(&item);
                if item.is_err() {
                    self.done = true;
                    self.shared.closed.store(true, Ordering::Release);
                }
                self.shared.space.notify_all();
                return Some(item);
            }
            if state.next_emit >= self.total {
                self.done = true;
                drop(state);
                // The raster stream's closing check.
                let mut call = |c| (self.stop)(c);
                let result = Cancellation::new(&mut call).check(Checkpoint::RasterComplete);
                return result
                    .err()
                    .map(|e| Err(RasterStreamError::Raster(RasterError::from(e))));
            }
            state = self
                .shared
                .ready
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }
}

/// Retained mask bytes of a finished layer.
fn retained(item: &ParallelItem) -> usize {
    item.as_ref()
        .map_or(0, |image| image.mask().retained_bytes())
}

/// A worker's face Z index position.
enum Cursor {
    /// No index: every layer scans all faces.
    FullScan,
    /// This worker's own sweep over the shared index.
    Ready(FaceZCursor),
    /// The cursor's reservation failed; every claimed layer reports the allocation failure.
    Unavailable,
}

/// Claim layers in increasing order, compute them, and publish their results.
fn worker(
    spec: &ParallelRaster<'_>,
    kernel: &LayerKernel<'_>,
    shared: &Shared,
    window: usize,
    stop: &(dyn Fn(Checkpoint) -> bool + Sync),
) {
    let planes = spec.schedule.planes();
    // Each worker claims increasing indices, so its own cursor only ever sweeps forward.
    let mut cursor = match &kernel.index {
        None => Cursor::FullScan,
        Some(index) => FaceZCursor::new(index).map_or(Cursor::Unavailable, Cursor::Ready),
    };
    loop {
        let index = {
            let mut state = lock(&shared.state);
            loop {
                let claim = state.next_claim;
                if claim >= planes.len()
                    || shared.closed.load(Ordering::Acquire)
                    || claim > shared.abandon_after.load(Ordering::Acquire)
                {
                    return;
                }
                if claim < state.next_emit + window
                    && (claim == state.next_emit || state.held_bytes < spec.window_bytes)
                {
                    state.next_claim += 1;
                    break claim;
                }
                state = shared
                    .space
                    .wait(state)
                    .unwrap_or_else(PoisonError::into_inner);
            }
        };
        let plane = planes[index];
        let previous = index.checked_sub(1).map(|k| planes[k]);
        let item = catch_unwind(AssertUnwindSafe(|| {
            compute(
                spec,
                kernel,
                &mut cursor,
                plane,
                previous,
                index,
                shared,
                stop,
            )
        }))
        .unwrap_or(Err(RasterStreamError::Source(LayerEngineError {
            plane: Some(plane),
            cause: LayerFailure::WorkerPanicked,
        })));
        if item.is_err() {
            shared.abandon_after.fetch_min(index, Ordering::AcqRel);
        }
        let mut state = lock(&shared.state);
        state.held_bytes += retained(&item);
        state.results.insert(index, item);
        shared.ready.notify_all();
    }
}

#[allow(clippy::too_many_arguments)] // Private worker step; each input is distinct.
fn compute(
    spec: &ParallelRaster<'_>,
    kernel: &LayerKernel<'_>,
    cursor: &mut Cursor,
    plane: LayerPlane,
    previous: Option<LayerPlane>,
    index: usize,
    shared: &Shared,
    stop: &(dyn Fn(Checkpoint) -> bool + Sync),
) -> ParallelItem {
    // Abandoned layers stop at their next checkpoint; their results are never delivered.
    let mut call = |c| {
        stop(c)
            || shared.closed.load(Ordering::Acquire)
            || index > shared.abandon_after.load(Ordering::Acquire)
    };
    let mut cancel = Cancellation::new(&mut call);
    let raster = spec.raster_for(plane.index);
    let source = |cause| {
        RasterStreamError::Source(LayerEngineError {
            plane: Some(plane),
            cause,
        })
    };
    let cursor = match cursor {
        Cursor::Ready(cursor) => Some(cursor),
        Cursor::Unavailable => return Err(source(LayerFailure::Section(SectionError::Allocation))),
        Cursor::FullScan => None,
    };
    if let LayerFill::Winding(rule) = spec.fill {
        let layer = kernel
            .winding_layer(plane, previous, cursor, &mut cancel)
            .map_err(source)?;
        // Cut sections at the Z the body was actually sectioned at.
        let rings = spec.cut.map_or_else(Vec::new, |cut| {
            cut.rings(LayerPlane {
                z_mm: layer.sampled_z_mm(),
                ..layer.plane()
            })
        });
        return rasterize_winding_layer_with_cancellation(
            &layer,
            rule,
            &rings,
            spec.profile,
            raster,
            spec.limits,
            &mut cancel,
        )
        .map_err(RasterStreamError::Raster);
    }
    let layer = kernel
        .layer(plane, previous, cursor, &mut cancel)
        .map_err(source)?;
    let image = match spec.cut {
        Some(cut) => {
            // Cut sections at the Z the body was actually sectioned at.
            let rings = cut.rings(LayerPlane {
                z_mm: layer.sampled_z_mm(),
                ..layer.plane()
            });
            rasterize_layer_minus_rings_with_cancellation(
                &layer,
                &rings,
                spec.profile,
                raster,
                spec.limits,
                &mut cancel,
            )
        }
        None => rasterize_layer_with_cancellation(
            &layer,
            spec.profile,
            raster,
            spec.limits,
            &mut cancel,
        ),
    };
    image.map_err(RasterStreamError::Raster)
}
