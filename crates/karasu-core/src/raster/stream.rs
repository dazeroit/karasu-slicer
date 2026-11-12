//! Sequential mask generation with no retained cache; caller/source retention remains caller-owned.
use super::{
    rasterize_layer_with_cancellation, LcdProfile, RasterConfig, RasterError, RasterImage,
    RasterLimits,
};
use crate::{
    cancellation::{Cancellation, Checkpoint},
    slicing::engine::{RegionLayer, StreamState},
};
use std::{error::Error, fmt, iter::FusedIterator};

/// Fused one-layer-at-a-time adapter over a fallible M3 layer source.
///
/// A successful mask is owned by the caller. The adapter retains no previously returned geometry
/// or masks. Source iterator storage and masks retained by callers are outside raster budgets.
/// No output job, cache, threads or asynchronous cancellation is implied.
pub struct RasterStream<'c, I> {
    source: I,
    profile: LcdProfile,
    config: RasterConfig,
    limits: RasterLimits,
    cancellation: Cancellation<'c>,
    state: StreamState,
    completed: usize,
}
impl<'c, I> RasterStream<'c, I> {
    /// Construct lazily; validation and cancellation occur before the first source request.
    pub const fn new(
        source: I,
        profile: LcdProfile,
        config: RasterConfig,
        limits: RasterLimits,
        cancellation: Cancellation<'c>,
    ) -> Self {
        Self {
            source,
            profile,
            config,
            limits,
            cancellation,
            state: StreamState::NotStarted,
            completed: 0,
        }
    }
    /// Complete only after normal exhaustion and the final cancellation check.
    #[must_use]
    pub const fn state(&self) -> StreamState {
        self.state
    }
    /// Count of complete images returned before any terminal error.
    #[must_use]
    pub const fn completed_layers(&self) -> usize {
        self.completed
    }
}
/// Terminal source or whole-layer raster failure with its original cause preserved.
#[derive(Debug)]
pub enum RasterStreamError<E> {
    /// Underlying M3 source failed; no image is returned for that request.
    Source(E),
    /// Raster configuration, geometry, resource or cancellation error.
    Raster(RasterError),
}
impl<E: fmt::Display> fmt::Display for RasterStreamError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Source(e) => write!(f, "raster layer source failed: {e}"),
            Self::Raster(e) => e.fmt(f),
        }
    }
}
impl<E: Error + 'static> Error for RasterStreamError<E> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Source(e) => Some(e),
            Self::Raster(e) => Some(e),
        }
    }
}
impl<I, E> Iterator for RasterStream<'_, I>
where
    I: Iterator<Item = Result<RegionLayer, E>>,
{
    type Item = Result<RasterImage, RasterStreamError<E>>;
    fn next(&mut self) -> Option<Self::Item> {
        if matches!(self.state, StreamState::Complete | StreamState::Failed) {
            return None;
        }
        let validation = (|| {
            self.cancellation.check(Checkpoint::RasterStart)?;
            self.config.validate()?;
            super::render::preflight(self.profile, self.config, self.limits, 0)?;
            Ok::<_, RasterError>(())
        })();
        if let Err(e) = validation {
            self.state = StreamState::Failed;
            return Some(Err(RasterStreamError::Raster(e)));
        }
        self.state = StreamState::Running;
        let layer = match self.source.next() {
            Some(Ok(layer)) => layer,
            Some(Err(e)) => {
                self.state = StreamState::Failed;
                return Some(Err(RasterStreamError::Source(e)));
            }
            None => {
                if let Err(e) = self.cancellation.check(Checkpoint::RasterComplete) {
                    self.state = StreamState::Failed;
                    return Some(Err(RasterStreamError::Raster(e.into())));
                }
                self.state = StreamState::Complete;
                return None;
            }
        };
        match rasterize_layer_with_cancellation(
            &layer,
            self.profile,
            self.config,
            self.limits,
            &mut self.cancellation,
        ) {
            Ok(image) => {
                let Some(completed) = self.completed.checked_add(1) else {
                    self.state = StreamState::Failed;
                    return Some(Err(RasterStreamError::Raster(RasterError::SizeOverflow)));
                };
                self.completed = completed;
                Some(Ok(image))
            }
            Err(e) => {
                self.state = StreamState::Failed;
                Some(Err(RasterStreamError::Raster(e)))
            }
        }
    }
}
impl<I, E> FusedIterator for RasterStream<'_, I> where I: Iterator<Item = Result<RegionLayer, E>> {}

/// Fused adapter rasterizing each `body` layer minus the cut rings of its plane, one at a time.
///
/// See [`super::rasterize_layer_minus_rings_with_cancellation`]. The cut is queried once per
/// layer with that layer's plane, so body and cut can never fall out of step.
pub struct DifferenceRasterStream<'c, 'k, I> {
    body: I,
    cut: &'k dyn super::LayerCut,
    profile: LcdProfile,
    config: RasterConfig,
    limits: RasterLimits,
    cancellation: Cancellation<'c>,
    state: StreamState,
    completed: usize,
}
impl<'c, 'k, I> DifferenceRasterStream<'c, 'k, I> {
    /// Construct lazily; validation and cancellation occur before the first source request.
    pub const fn new(
        body: I,
        cut: &'k dyn super::LayerCut,
        profile: LcdProfile,
        config: RasterConfig,
        limits: RasterLimits,
        cancellation: Cancellation<'c>,
    ) -> Self {
        Self {
            body,
            cut,
            profile,
            config,
            limits,
            cancellation,
            state: StreamState::NotStarted,
            completed: 0,
        }
    }
    /// Complete only after normal exhaustion and the final cancellation check.
    #[must_use]
    pub const fn state(&self) -> StreamState {
        self.state
    }
    /// Count of complete images returned before any terminal error.
    #[must_use]
    pub const fn completed_layers(&self) -> usize {
        self.completed
    }
    fn fail<E>(
        &mut self,
        error: RasterStreamError<E>,
    ) -> Result<RasterImage, RasterStreamError<E>> {
        self.state = StreamState::Failed;
        Err(error)
    }
}
impl<I, E> Iterator for DifferenceRasterStream<'_, '_, I>
where
    I: Iterator<Item = Result<RegionLayer, E>>,
{
    type Item = Result<RasterImage, RasterStreamError<E>>;
    fn next(&mut self) -> Option<Self::Item> {
        if matches!(self.state, StreamState::Complete | StreamState::Failed) {
            return None;
        }
        let validation = (|| {
            self.cancellation.check(Checkpoint::RasterStart)?;
            self.config.validate()?;
            super::render::preflight(self.profile, self.config, self.limits, 0)?;
            Ok::<_, RasterError>(())
        })();
        if let Err(e) = validation {
            return Some(self.fail(RasterStreamError::Raster(e)));
        }
        self.state = StreamState::Running;
        let body = match self.body.next() {
            Some(Ok(body)) => body,
            Some(Err(e)) => return Some(self.fail(RasterStreamError::Source(e))),
            None => {
                if let Err(e) = self.cancellation.check(Checkpoint::RasterComplete) {
                    return Some(self.fail(RasterStreamError::Raster(e.into())));
                }
                self.state = StreamState::Complete;
                return None;
            }
        };
        // Cut sections at the Z the body was actually sectioned at.
        let rings = self.cut.rings(crate::slicing::schedule::LayerPlane {
            z_mm: body.sampled_z_mm(),
            ..body.plane()
        });
        match super::rasterize_layer_minus_rings_with_cancellation(
            &body,
            &rings,
            self.profile,
            self.config,
            self.limits,
            &mut self.cancellation,
        ) {
            Ok(image) => {
                let Some(completed) = self.completed.checked_add(1) else {
                    return Some(self.fail(RasterStreamError::Raster(RasterError::SizeOverflow)));
                };
                self.completed = completed;
                Some(Ok(image))
            }
            Err(e) => Some(self.fail(RasterStreamError::Raster(e))),
        }
    }
}
impl<I, E> FusedIterator for DifferenceRasterStream<'_, '_, I> where
    I: Iterator<Item = Result<RegionLayer, E>>
{
}
