//! Deterministic, bounded LCD masks from reviewed M3 material. See the M4 implementation contract.
use crate::cancellation::{Cancellation, Cancelled, Checkpoint};
use crate::slicing::{
    engine::RegionLayer,
    schedule::LayerPlane,
    winding::{ClosureReport, WindingLayer, WindingRule},
};
use mask::SpanMask;
use std::{error::Error, fmt};
mod input;
pub mod mask;
pub mod parallel;
pub mod png;
mod render;
pub mod stream;
pub mod verify;

/// Explicit output axis permutation and reflection, applied in that order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Orientation {
    /// Exchange world X and Y before output flips.
    pub swap_xy: bool,
    /// Reflect output columns around the panel center.
    pub flip_x: bool,
    /// Reflect output rows around the panel center.
    pub flip_y: bool,
}
/// Validated physical panel subset; no printer or exposure defaults.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LcdProfile {
    dimensions: [u32; 2],
    origin_mm: [f64; 2],
    extent_mm: [f64; 2],
    orientation: Orientation,
}
impl LcdProfile {
    /// Validate nonzero dimensions <=65536 and finite positive physical extents.
    /// # Errors
    /// Rejects invalid dimensions, nonfinite origins/extents, or unrepresentable bounds/pitches.
    pub fn try_new(
        dimensions: [u32; 2],
        origin_mm: [f64; 2],
        extent_mm: [f64; 2],
        orientation: Orientation,
    ) -> Result<Self, RasterError> {
        let result = Self {
            dimensions,
            origin_mm,
            extent_mm,
            orientation,
        };
        if dimensions.iter().any(|&n| n == 0 || n > 65536)
            || origin_mm.iter().any(|n| !n.is_finite())
            || extent_mm.iter().any(|n| !n.is_finite() || *n <= 0.)
        {
            return Err(RasterError::InvalidProfile);
        }
        for axis in 0..2 {
            let upper = origin_mm[axis] + extent_mm[axis];
            let pitch = result.pitch_mm()[axis];
            if !upper.is_finite()
                || upper <= origin_mm[axis]
                || !pitch.is_finite()
                || pitch <= 0.
                || (origin_mm[axis].abs().max(upper.abs()) + origin_mm[axis].abs()) / pitch
                    > NUMERIC_DOMAIN
            {
                return Err(RasterError::InvalidProfile);
            }
        }
        Ok(result)
    }
    /// Output width and height in pixels.
    #[must_use]
    pub const fn dimensions(self) -> [u32; 2] {
        self.dimensions
    }
    /// Physical world XY minimum in millimeters.
    #[must_use]
    pub const fn origin_mm(self) -> [f64; 2] {
        self.origin_mm
    }
    /// Physical world XY dimensions in millimeters.
    #[must_use]
    pub const fn extent_mm(self) -> [f64; 2] {
        self.extent_mm
    }
    /// Explicit axis mapping.
    #[must_use]
    pub const fn orientation(self) -> Orientation {
        self.orientation
    }
    /// World X/Y pixel pitches, accounting for axis permutation.
    #[must_use]
    pub fn pitch_mm(self) -> [f64; 2] {
        let [w, h] = self.dimensions.map(f64::from);
        let sizes = if self.orientation.swap_xy {
            [h, w]
        } else {
            [w, h]
        };
        [self.extent_mm[0] / sizes[0], self.extent_mm[1] / sizes[1]]
    }
    /// Map finite world XY to continuous pixel boundary coordinates.
    /// # Errors
    /// Rejects nonfinite values or values outside the documented arithmetic domain.
    pub fn world_to_pixel(self, xy: [f64; 2]) -> Result<[f64; 2], RasterError> {
        let pitch = self.pitch_mm();
        let mut p = [0.; 2];
        for a in 0..2 {
            if !xy[a].is_finite()
                || xy[a].abs() / pitch[a] + self.origin_mm[a].abs() / pitch[a] > NUMERIC_DOMAIN
            {
                return Err(RasterError::NumericRange);
            }
            p[a] = (xy[a] - self.origin_mm[a]) / pitch[a];
        }
        if self.orientation.swap_xy {
            p.swap(0, 1);
        }
        if self.orientation.flip_x {
            p[0] = f64::from(self.dimensions[0]) - p[0];
        }
        if self.orientation.flip_y {
            p[1] = f64::from(self.dimensions[1]) - p[1];
        }
        Ok(p)
    }
    /// Inverse continuous transform; performs no rounding to pixel centers.
    /// # Errors
    /// Rejects nonfinite coordinates or a result outside the arithmetic domain.
    pub fn pixel_to_world(self, mut p: [f64; 2]) -> Result<[f64; 2], RasterError> {
        if p.iter().any(|v| !v.is_finite()) {
            return Err(RasterError::NumericRange);
        }
        if self.orientation.flip_x {
            p[0] = f64::from(self.dimensions[0]) - p[0];
        }
        if self.orientation.flip_y {
            p[1] = f64::from(self.dimensions[1]) - p[1];
        }
        if self.orientation.swap_xy {
            p.swap(0, 1);
        }
        let pitch = self.pitch_mm();
        let xy = [
            p[0] * pitch[0] + self.origin_mm[0],
            p[1] * pitch[1] + self.origin_mm[1],
        ];
        self.world_to_pixel(xy)?;
        Ok(xy)
    }
}
const NUMERIC_DOMAIN: f64 = 16_777_216.;
/// Policy for material outside the physical panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutsidePolicy {
    /// Reject a layer with any boundary vertex outside the closed panel.
    Reject,
    /// Sample only the panel; retain original crossings while clipping spans.
    Clip,
}
/// Discrete grayscale morphology, after coverage quantization; exterior pixels are zero.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Compensation {
    /// Preserve sampled intensities.
    None,
    /// Rectangle maximum: expand material and shrink holes by these X/Y pixel radii.
    Dilate([u16; 2]),
    /// Rectangle minimum: contract material and expand holes by these X/Y pixel radii.
    Erode([u16; 2]),
}
/// Explicit raster settings. All fields are validated before work or allocation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RasterConfig {
    /// Midpoint samples per axis: 1,2,4,8,16. One selects binary center sampling.
    pub samples_per_axis: u8,
    /// Number of equally spaced intensity levels, 2..256, nearest/ties-up rounding.
    pub grayscale_levels: u16,
    /// Smallest intensity of a partially covered pixel, for resins that do not cure below a
    /// gray level. A coverage fraction `c` strictly between 0 and 1 becomes the intensity
    /// `minimum_gray + (255 - minimum_gray) * c` before rounding to `grayscale_levels`; empty
    /// pixels stay 0 and fully covered pixels 255. Zero keeps intensity proportional to
    /// coverage. Binary sampling (one sample per axis) has no partial pixels.
    pub minimum_gray: u8,
    /// Explicit clipping/rejection policy.
    pub outside: OutsidePolicy,
    /// Explicit pixel-space compensation.
    pub compensation: Compensation,
    /// How the kept bodies of a winding layer (the supports, TD-123) are sampled (TD-134).
    pub kept: KeptSampling,
}
/// How a winding layer's kept bodies, the supports (TD-123), are sampled (TD-134).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum KeptSampling {
    /// As the rest of the layer: anti-aliased with it.
    #[default]
    Antialiased,
    /// Binary: a pixel whose centre-most sample lies in a kept body is full, and the rest of the
    /// layer is anti-aliased alone, so the supports' edges carry no gray.
    Binary,
}
impl RasterConfig {
    pub(crate) fn validate(self) -> Result<(), RasterError> {
        if ![1, 2, 4, 8, 16].contains(&self.samples_per_axis)
            || !(2..=256).contains(&self.grayscale_levels)
        {
            return Err(RasterError::InvalidConfig);
        }
        Ok(())
    }
}
/// Independent per-call caps; bytes exclude caller-owned input, allocator overhead and process RSS.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RasterLimits {
    /// Maximum output pixel count.
    pub max_pixels: usize,
    /// Maximum input boundary edges, including horizontal edges.
    pub max_edges: usize,
    /// Maximum planned simultaneous requested vector payload.
    pub max_payload_bytes: usize,
    /// Maximum charged conversion, scanning, sort, span and morphology work.
    pub max_work: usize,
}
impl Default for RasterLimits {
    fn default() -> Self {
        Self {
            // Accommodates the 13312 x 5120 (65-MiPixel) Anycubic Photon P1 panel, matching
            // `ExportLimits::default().max_pixels`.
            max_pixels: 128 * 1024 * 1024,
            max_edges: 1_000_000,
            max_payload_bytes: 256 * 1024 * 1024,
            // Rows charge per span endpoint, not per pixel; compensation charges a few units per
            // pixel.
            max_work: 2_000_000_000,
        }
    }
}
/// Preview statistics, measured after compensation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PreviewStats {
    /// Count of nonzero pixels.
    pub nonzero_pixels: usize,
    /// Sum of grayscale8 values, not physical volume or emitted light.
    pub intensity_sum: u64,
    /// Inclusive minimum/exclusive maximum nonzero pixel rectangle, or empty.
    pub bounds: Option<[[u32; 2]; 2]>,
}
/// Extreme winding numbers of a winding layer over the intervals between row crossings that hold a
/// sample, including the zero outside all material.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindingRange {
    /// Smallest sampled winding number (never above zero).
    pub min: i64,
    /// Largest sampled winding number (never below zero). Above one, the fill rule merged
    /// overlapping material.
    pub max: i64,
    /// Samples whose winding number is negative (counted on the sampling grid, so a pixel holds
    /// `samples_per_axis` squared of them).
    pub negative_samples: u64,
}
/// Atomic owned grayscale8 LCD layer for headless preview/export.
///
/// The mask is held as nonzero row spans ([`SpanMask`], TD-055); [`Self::to_pixels`] gives the
/// dense row-major image when one is needed.
#[derive(Debug, PartialEq)]
pub struct RasterImage {
    pub(crate) profile: LcdProfile,
    pub(crate) config: RasterConfig,
    pub(crate) plane: LayerPlane,
    pub(crate) stats: PreviewStats,
    pub(crate) winding: Option<WindingRange>,
    pub(crate) closure: Option<ClosureReport>,
    pub(crate) mask: SpanMask,
    pub(crate) requested_payload_bytes: usize,
}
impl RasterImage {
    /// Validated output width.
    #[must_use]
    pub const fn width(&self) -> u32 {
        self.profile.dimensions[0]
    }
    /// Validated output height.
    #[must_use]
    pub const fn height(&self) -> u32 {
        self.profile.dimensions[1]
    }
    /// Sampled coverage intensities as nonzero row spans.
    #[must_use]
    pub const fn mask(&self) -> &SpanMask {
        &self.mask
    }
    /// Row-major linear sampled coverage intensities, materialized from the spans.
    /// # Errors
    /// Allocation failure of the dense image.
    pub fn to_pixels(&self) -> Result<Vec<u8>, RasterError> {
        self.mask.to_pixels()
    }
    /// Physical extent, origin and output orientation for preview.
    #[must_use]
    pub const fn profile(&self) -> LcdProfile {
        self.profile
    }
    /// Settings that produced this image.
    #[must_use]
    pub const fn config(&self) -> RasterConfig {
        self.config
    }
    /// Original layer index and Z in millimeters.
    #[must_use]
    pub const fn plane(&self) -> LayerPlane {
        self.plane
    }
    /// Post-compensation intensity statistics.
    #[must_use]
    pub const fn stats(&self) -> PreviewStats {
        self.stats
    }
    /// Sampled winding extremes of a winding layer, or `None` for a strict layer.
    #[must_use]
    pub const fn winding(&self) -> Option<WindingRange> {
        self.winding
    }
    /// Chords that section closure added to a winding layer, or `None` for a strict layer.
    #[must_use]
    pub const fn closure(&self) -> Option<ClosureReport> {
        self.closure
    }
    /// Preflighted scratch plus retained span bytes, excluding caller input and PNG scratch.
    #[must_use]
    pub const fn requested_payload_bytes(&self) -> usize {
        self.requested_payload_bytes
    }
}
/// Whole-layer raster failure; no partial successful image is returned.
#[derive(Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum RasterError {
    /// Invalid dimensions, extent, origin or pitch.
    InvalidProfile,
    /// Unsupported sampling or intensity configuration.
    InvalidConfig,
    /// Unresolved M3 geometry cannot define an image.
    UnresolvedGeometry,
    /// An upstream private-index invariant was not satisfied.
    InvalidGeometry,
    /// Raster arithmetic/conversion exceeds the documented supported range.
    NumericRange,
    /// A boundary vertex is outside a profile using the reject policy.
    OutsidePanel,
    /// Count or storage-size arithmetic overflow.
    SizeOverflow,
    /// Fallible vector reservation failed.
    Allocation,
    /// Configured resource budget exhausted.
    Limit {
        /// Resource name: pixels, edges, payload bytes, or work.
        resource: &'static str,
        /// Requested cap.
        limit: usize,
    },
    /// Cooperative cancellation preserved with its named checkpoint.
    Cancelled(Cancelled),
    /// Under the positive winding rule a sample has a negative winding number: the source is
    /// inside out, or a reversed shell is not enclosed by material.
    NegativeWinding {
        /// Zero-based layer index.
        layer: u64,
        /// The first pixel found with a negative sample, as image column and row.
        pixel: [u32; 2],
    },
    /// Under [`WindingRule::PositiveBounded`] a layer has more negative samples than allowed.
    NegativeWindingAboveBound {
        /// Zero-based layer index.
        layer: u64,
        /// The first pixel found with a negative sample, as image column and row.
        pixel: [u32; 2],
        /// Negative samples in the layer.
        samples: u64,
        /// The bound.
        max_samples: u64,
    },
}
impl fmt::Display for RasterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Limit { resource, limit } => {
                write!(f, "raster {resource} limit exceeded ({limit})")
            }
            Self::Cancelled(e) => e.fmt(f),
            Self::InvalidProfile => f.write_str("LCD profile requires dimensions 1..65536 and finite positive extents, representable pitches and supported origins"),
            Self::InvalidConfig => f.write_str("raster samples per axis must be 1, 2, 4, 8 or 16 and grayscale levels must be 2..256"),
            Self::UnresolvedGeometry => f.write_str("unresolved M3 geometry cannot define a complete LCD mask; select an explicit supported material composition first"),
            Self::InvalidGeometry => f.write_str("inconsistent M3 boundary references or odd scanline crossing count"),
            Self::NumericRange => f.write_str("raster projection exceeds the supported 2^24-pixel arithmetic domain or collapses a nonzero boundary edge"),
            Self::OutsidePanel => f.write_str("a boundary vertex lies outside the LCD panel; move the geometry or explicitly select clipping"),
            Self::SizeOverflow => f.write_str("raster count or storage size overflow"),
            Self::Allocation => f.write_str("unable to allocate bounded raster storage"),
            Self::NegativeWinding { layer, pixel: [x, y] } => write!(f, "negative winding number under the positive rule at layer {layer}, pixel ({x}, {y}): the model is inside out or has an unenclosed reversed shell; fix its orientation or select the nonzero rule"),
            Self::NegativeWindingAboveBound { layer, pixel: [x, y], samples, max_samples } => write!(f, "negative winding number at layer {layer}, from pixel ({x}, {y}), over {samples} samples, more than the {max_samples} tolerated: the model is inside out or has an unenclosed reversed shell; fix its orientation"),
        }
    }
}
impl Error for RasterError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        if let Self::Cancelled(e) = self {
            Some(e)
        } else {
            None
        }
    }
}
impl From<Cancelled> for RasterError {
    fn from(e: Cancelled) -> Self {
        Self::Cancelled(e)
    }
}
#[derive(Clone, Copy, Debug)]
pub(crate) struct Edge {
    pub a: [f64; 2],
    pub b: [f64; 2],
}
pub(crate) fn cap(n: usize, limit: usize, resource: &'static str) -> Result<(), RasterError> {
    if n > limit {
        Err(RasterError::Limit { resource, limit })
    } else {
        Ok(())
    }
}
pub(crate) fn reserve<T>(n: usize) -> Result<Vec<T>, RasterError> {
    let mut v = Vec::new();
    v.try_reserve_exact(n)
        .map_err(|_| RasterError::Allocation)?;
    Ok(v)
}
/// Render one complete strict layer, or its explicitly attached constructed material union.
/// # Errors
/// Rejects unresolved geometry, unsupported numeric/configuration domains, resource exhaustion,
/// allocation failure and cancellation. Source geometry is never changed.
pub fn rasterize_layer(
    layer: &RegionLayer,
    profile: LcdProfile,
    config: RasterConfig,
    limits: RasterLimits,
) -> Result<RasterImage, RasterError> {
    rasterize_layer_with_cancellation(layer, profile, config, limits, &mut Cancellation::never())
}
/// Cancellable variant with atomic image publication.
/// # Errors
/// Returns the same structured failures as [`rasterize_layer`], including cancellation.
pub fn rasterize_layer_with_cancellation(
    layer: &RegionLayer,
    profile: LcdProfile,
    config: RasterConfig,
    limits: RasterLimits,
    cancel: &mut Cancellation<'_>,
) -> Result<RasterImage, RasterError> {
    cancel.check(Checkpoint::RasterStart)?;
    config.validate()?;
    let count = input::edge_count(layer, limits, cancel)?;
    let bytes = render::preflight(profile, config, limits, count)?;
    let edges = input::edges(layer, profile, config, limits, count, cancel)?;
    render::render(
        &edges,
        layer.plane(),
        profile,
        config,
        limits,
        bytes,
        cancel,
    )
}
/// Closed world-space regions (millimetres) removed from the material of one layer.
///
/// Implementors return the cut rings for a plane; the cut is their union, so rings may overlap
/// (TD-089: drain holes may). Used by drained models whose cutters are evaluated analytically
/// instead of being sliced.
pub trait LayerCut {
    /// Closed rings, each listed once without repeating its first vertex.
    fn rings(&self, plane: LayerPlane) -> Vec<Vec<[f64; 2]>>;
}

/// Rasterize the material of `body` minus the union of the `cut` rings (world rings, mm), without
/// constructing a difference polygon.
///
/// Each sample point is covered exactly when it is inside `body` and outside `cut`, so the mask
/// equals the sampled polygonal difference. `body` follows the configured outside policy; cut
/// vertices may lie outside the panel, since removing material there changes nothing.
/// # Errors
/// Returns the failures of [`rasterize_layer`] for the body, with both edge sets counted against
/// the limits.
pub fn rasterize_layer_minus_rings_with_cancellation(
    body: &RegionLayer,
    cut: &[Vec<[f64; 2]>],
    profile: LcdProfile,
    config: RasterConfig,
    limits: RasterLimits,
    cancel: &mut Cancellation<'_>,
) -> Result<RasterImage, RasterError> {
    cancel.check(Checkpoint::RasterStart)?;
    config.validate()?;
    let body_count = input::edge_count(body, limits, cancel)?;
    // Both edge reservations are counted at full capacity before either is made.
    let cut_count = input::ring_edge_count(cut, limits)?;
    let count = body_count
        .checked_add(cut_count)
        .ok_or(RasterError::SizeOverflow)?;
    let bytes = render::preflight(profile, config, limits, count)?;
    let body_edges = input::edges(body, profile, config, limits, body_count, cancel)?;
    let cut_edges = input::ring_edges(cut, profile, cut_count, cancel)?;
    render::render_difference(
        &body_edges,
        &cut_edges,
        &[],
        render::Fill::Parity,
        body.plane(),
        profile,
        config,
        limits,
        bytes,
        cancel,
    )
}

/// Rasterize the union of `rings` (world rings, mm) alone, as material, at `plane`: for example the
/// drain holes' cuts of one layer, so a verifier can tell a hole from a breach (TD-095). Ring
/// vertices may lie outside the panel.
/// # Errors
/// The failures of [`rasterize_layer`].
pub fn rasterize_rings_with_cancellation(
    rings: &[Vec<[f64; 2]>],
    plane: LayerPlane,
    profile: LcdProfile,
    config: RasterConfig,
    limits: RasterLimits,
    cancel: &mut Cancellation<'_>,
) -> Result<RasterImage, RasterError> {
    cancel.check(Checkpoint::RasterStart)?;
    config.validate()?;
    let count = input::ring_edge_count(rings, limits)?;
    let bytes = render::preflight(profile, config, limits, count)?;
    let edges = input::ring_edges(rings, profile, count, cancel)?;
    render::render_difference(
        &edges,
        &[],
        &[],
        render::Fill::Winding(WindingRule::NonZero),
        plane,
        profile,
        config,
        limits,
        bytes,
        cancel,
    )
}

/// Render one winding layer under `rule`, minus the union of the `cut` rings (world rings, mm; empty
/// for none), without constructing any polygon.
///
/// Each sample is covered exactly when its winding number with respect to the layer's directed
/// segments satisfies `rule` and it lies outside `cut`, or inside the material the cut leaves:
/// where the winding number of the layer's kept segments (the supports, TD-123) is positive. On a
/// closed embedded surface with outward faces every rule gives the strict layer's mask. The image reports the sampled winding range.
///
/// With [`KeptSampling::Binary`] and anti-aliasing (TD-134), the rule applies to the model alone
/// (the layer's segments without its kept ones), and a pixel whose centre-most sample lies in a
/// kept body is full: the supports are binary, whole, and never cut.
/// # Errors
/// The failures of [`rasterize_layer`]; [`RasterError::NegativeWinding`] under
/// [`WindingRule::Positive`] when a sampled winding number is negative, and
/// [`RasterError::NegativeWindingAboveBound`] under [`WindingRule::PositiveBounded`] when more
/// samples than its bound are.
pub fn rasterize_winding_layer_with_cancellation(
    layer: &WindingLayer,
    rule: WindingRule,
    cut: &[Vec<[f64; 2]>],
    profile: LcdProfile,
    config: RasterConfig,
    limits: RasterLimits,
    cancel: &mut Cancellation<'_>,
) -> Result<RasterImage, RasterError> {
    cancel.check(Checkpoint::RasterStart)?;
    config.validate()?;
    let cut_count = input::ring_edge_count(cut, limits)?;
    // Binary supports (TD-134): the model is sampled alone, its supports at pixel centres.
    let binary = config.kept == KeptSampling::Binary
        && config.samples_per_axis > 1
        && !layer.kept_segments().is_empty();
    // Every segment is within the limits; the model's are some of them.
    let all = input::winding_edge_count(layer, limits)?;
    let body_count = if binary {
        layer.model_segments().len()
    } else {
        all
    };
    // The material the cut leaves matters only where there is a cut, or binary supports.
    let kept = if cut.is_empty() && !binary {
        &[][..]
    } else {
        layer.kept_segments()
    };
    let count = body_count
        .checked_add(cut_count)
        .and_then(|n| n.checked_add(kept.len()))
        .ok_or(RasterError::SizeOverflow)?;
    let bytes = render::preflight(profile, config, limits, count)?;
    let body_edges = if binary {
        // The model alone, without the supports' segments.
        input::segment_edges(layer.model_segments(), profile, config, body_count, cancel)?
    } else {
        input::winding_edges(layer, profile, config, body_count, cancel)?
    };
    let cut_edges = input::ring_edges(cut, profile, cut_count, cancel)?;
    let kept_edges = input::segment_edges(kept, profile, config, kept.len(), cancel)?;
    let mut image = render::render_difference(
        &body_edges,
        &cut_edges,
        &kept_edges,
        render::Fill::Winding(rule),
        layer.plane(),
        profile,
        config,
        limits,
        bytes,
        cancel,
    )?;
    image.closure = Some(layer.closure());
    Ok(image)
}
