//! Shaded model previews for print-file thumbnails (TD-059).
//!
//! **Rendering:**
//! - A deterministic software renderer draws the model from a fixed isometric viewpoint
//!   (front-right, above), scaled to fit the image with a margin.
//! - It uses flat two-sided Lambert shading, a depth buffer, and 3 × 3 supersampling averaged with
//!   integer rounding.
//!
//! **Determinism:** only IEEE-754 additions, multiplications, divisions and square roots are used
//! (no trigonometry, no fused operations), so the same mesh gives the same pixels on every platform
//! and for every thread count.
//!
//! **Purpose:** the image is a thumbnail for the printer's file browser. It is not a mask and never
//! affects printed layers.

use crate::cancellation::{Cancellation, Cancelled, Checkpoint};
use crate::geometry::Affine3;
use crate::mesh::TriangleMesh;
use std::{error::Error, fmt};

/// An 8-bit RGB image, rows top to bottom.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreviewImage {
    width: u32,
    height: u32,
    rgb: Vec<u8>,
}

impl PreviewImage {
    /// An image from `width * height * 3` bytes in R, G, B order.
    ///
    /// # Errors
    /// [`PreviewError::Size`] when the dimensions are zero, too large, or do not match the bytes.
    pub fn new(width: u32, height: u32, rgb: Vec<u8>) -> Result<Self, PreviewError> {
        let expected = (width as usize)
            .checked_mul(height as usize)
            .and_then(|n| n.checked_mul(3));
        if width == 0 || height == 0 || width > MAX_SIDE || height > MAX_SIDE {
            return Err(PreviewError::Size);
        }
        if expected != Some(rgb.len()) {
            return Err(PreviewError::Size);
        }
        Ok(Self { width, height, rgb })
    }
    /// Width in pixels.
    #[must_use]
    pub const fn width(&self) -> u32 {
        self.width
    }
    /// Height in pixels.
    #[must_use]
    pub const fn height(&self) -> u32 {
        self.height
    }
    /// Pixels as R, G, B bytes, rows top to bottom.
    #[must_use]
    pub fn rgb(&self) -> &[u8] {
        &self.rgb
    }
    /// The pixel at column `x`, row `y`.
    #[must_use]
    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 3]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let at = (y as usize * self.width as usize + x as usize) * 3;
        Some([self.rgb[at], self.rgb[at + 1], self.rgb[at + 2]])
    }
}

/// The small and large previews a print file carries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Previews {
    /// The file browser's list image.
    pub small: PreviewImage,
    /// The detail image.
    pub large: PreviewImage,
}

/// Largest preview side, in pixels.
pub const MAX_SIDE: u32 = 4096;

/// Why a preview cannot be rendered.
#[derive(Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum PreviewError {
    /// Invalid dimensions.
    Size,
    /// The placement cannot map a vertex to finite coordinates.
    Transform,
    /// A buffer could not be allocated.
    Allocation,
    /// Cooperative cancellation.
    Cancelled(Cancelled),
}

impl fmt::Display for PreviewError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Size => write!(f, "preview dimensions must be 1..={MAX_SIDE} pixels"),
            Self::Transform => {
                f.write_str("the placement maps the model outside finite coordinates")
            }
            Self::Allocation => f.write_str("preview buffer allocation failed"),
            Self::Cancelled(c) => c.fmt(f),
        }
    }
}

impl Error for PreviewError {}

impl From<Cancelled> for PreviewError {
    fn from(c: Cancelled) -> Self {
        Self::Cancelled(c)
    }
}

/// Background colour.
pub const BACKGROUND: [u8; 3] = [30, 32, 38];
/// Base model colour before shading.
pub const MODEL: [u8; 3] = [178, 196, 214];
/// Supersamples per pixel axis.
const SAMPLES: usize = 3;

fn normalize(v: [f64; 3]) -> [f64; 3] {
    let length = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    [v[0] / length, v[1] / length, v[2] / length]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// Render `mesh`, placed by `placement`, into a `width` × `height` preview.
///
/// # Errors
/// Invalid dimensions, a placement producing non-finite coordinates, allocation failure or
/// cancellation.
#[allow(clippy::too_many_lines, clippy::many_single_char_names)] // One renderer; geometric names.
pub fn render_preview(
    mesh: &TriangleMesh,
    placement: Affine3,
    width: u32,
    height: u32,
    cancel: &mut Cancellation<'_>,
) -> Result<PreviewImage, PreviewError> {
    cancel.check(Checkpoint::RasterStart)?;
    if width == 0 || height == 0 || width > MAX_SIDE || height > MAX_SIDE {
        return Err(PreviewError::Size);
    }
    // Camera: looking from front-right, above, towards the model.
    let eye = normalize([1.0, -1.3, 0.9]);
    let forward = [-eye[0], -eye[1], -eye[2]];
    let right = normalize(cross(forward, [0.0, 0.0, 1.0]));
    let up = cross(right, forward);
    let light = normalize([0.6, -1.0, 1.4]);
    let light = [dot(light, right), dot(light, up), dot(light, forward)];
    // Project every vertex once: screen x, screen y (up) and depth (larger is farther).
    let mut projected = Vec::new();
    projected
        .try_reserve_exact(mesh.vertices().len())
        .map_err(|_| PreviewError::Allocation)?;
    for (index, &point) in mesh.vertices().iter().enumerate() {
        cancel.poll(Checkpoint::RasterInput, index)?;
        let p = placement
            .transform_point(point)
            .map_err(|_| PreviewError::Transform)?;
        let p = [p.x, p.y, p.z];
        projected.push([dot(p, right), dot(p, up), dot(p, forward)]);
    }
    let (sw, sh) = (width as usize * SAMPLES, height as usize * SAMPLES);
    let samples = sw.checked_mul(sh).ok_or(PreviewError::Allocation)?;
    let mut depth = Vec::new();
    depth
        .try_reserve_exact(samples)
        .map_err(|_| PreviewError::Allocation)?;
    depth.resize(samples, f64::INFINITY);
    let mut shade = Vec::new();
    shade
        .try_reserve_exact(samples)
        .map_err(|_| PreviewError::Allocation)?;
    // Zero means background; otherwise 1 + shade level in 0..=255.
    shade.resize(samples, 0u16);
    // Fit the projected bounds with a margin, preserving aspect ratio.
    let used = mesh
        .triangles()
        .iter()
        .flatten()
        .map(|&v| projected[v as usize]);
    let mut bounds = [
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    for [x, y, _] in used {
        bounds = [
            bounds[0].min(x),
            bounds[1].min(y),
            bounds[2].max(x),
            bounds[3].max(y),
        ];
    }
    let drawable = bounds[0].is_finite() && bounds[2] > bounds[0] && bounds[3] > bounds[1];
    if drawable {
        let margin = 0.08;
        #[allow(clippy::cast_precision_loss)] // Sides are at most 4096 * 3.
        let (fw, fh) = (sw as f64, sh as f64);
        let scale = ((fw * (1.0 - 2.0 * margin)) / (bounds[2] - bounds[0]))
            .min((fh * (1.0 - 2.0 * margin)) / (bounds[3] - bounds[1]));
        let center = [
            f64::midpoint(bounds[0], bounds[2]),
            f64::midpoint(bounds[1], bounds[3]),
        ];
        // Screen sample coordinates: x to the right, y downward.
        let to_screen = |p: [f64; 3]| {
            [
                fw / 2.0 + (p[0] - center[0]) * scale,
                fh / 2.0 - (p[1] - center[1]) * scale,
                p[2],
            ]
        };
        for (index, triangle) in mesh.triangles().iter().enumerate() {
            cancel.poll(Checkpoint::RasterRows, index)?;
            let [a, b, c] = triangle.map(|v| to_screen(projected[v as usize]));
            // Two-sided Lambert shading, in camera coordinates (an orthonormal basis keeps angles).
            let q = triangle.map(|v| projected[v as usize]);
            let e1 = [q[1][0] - q[0][0], q[1][1] - q[0][1], q[1][2] - q[0][2]];
            let e2 = [q[2][0] - q[0][0], q[2][1] - q[0][1], q[2][2] - q[0][2]];
            let normal = cross(e1, e2);
            let length = dot(normal, normal).sqrt();
            if length == 0.0 || !length.is_finite() {
                continue;
            }
            let lambert = (dot(normal, light) / length).abs();
            // The level is in 0..=255 by construction.
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let level = (255.0 * (0.28 + 0.72 * lambert)).round().min(255.0) as u16;
            // Rasterize over the bounding box of sample centers.
            let area = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
            if area == 0.0 {
                continue;
            }
            let min_x = a[0].min(b[0]).min(c[0]).floor().max(0.0);
            let max_x = a[0].max(b[0]).max(c[0]).ceil().min(fw);
            let min_y = a[1].min(b[1]).min(c[1]).floor().max(0.0);
            let max_y = a[1].max(b[1]).max(c[1]).ceil().min(fh);
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // Clamped above.
            let (x0, x1, y0, y1) = (
                min_x as usize,
                max_x as usize,
                min_y as usize,
                max_y as usize,
            );
            for sy in y0..y1 {
                #[allow(clippy::cast_precision_loss)]
                let py = sy as f64 + 0.5;
                for sx in x0..x1 {
                    #[allow(clippy::cast_precision_loss)]
                    let px = sx as f64 + 0.5;
                    let w0 = (b[0] - px) * (c[1] - py) - (b[1] - py) * (c[0] - px);
                    let w1 = (c[0] - px) * (a[1] - py) - (c[1] - py) * (a[0] - px);
                    let w2 = (a[0] - px) * (b[1] - py) - (a[1] - py) * (b[0] - px);
                    let inside = if area > 0.0 {
                        w0 >= 0.0 && w1 >= 0.0 && w2 >= 0.0
                    } else {
                        w0 <= 0.0 && w1 <= 0.0 && w2 <= 0.0
                    };
                    if !inside {
                        continue;
                    }
                    let z = (w0 * a[2] + w1 * b[2] + w2 * c[2]) / area;
                    let at = sy * sw + sx;
                    // Nearer wins; equal depth keeps the earlier triangle (deterministic order).
                    if z < depth[at] {
                        depth[at] = z;
                        shade[at] = 1 + level;
                    }
                }
            }
        }
    }
    drop(depth);
    // Average each pixel's samples with integer rounding.
    let mut rgb = Vec::new();
    rgb.try_reserve_exact(width as usize * height as usize * 3)
        .map_err(|_| PreviewError::Allocation)?;
    #[allow(clippy::cast_possible_truncation)] // Nine samples.
    let count = (SAMPLES * SAMPLES) as u32;
    for y in 0..height as usize {
        cancel.poll(Checkpoint::RasterRows, y)?;
        for x in 0..width as usize {
            let mut sums = [0u32; 3];
            for dy in 0..SAMPLES {
                for dx in 0..SAMPLES {
                    let value = shade[(y * SAMPLES + dy) * sw + x * SAMPLES + dx];
                    let colour = if value == 0 {
                        BACKGROUND
                    } else {
                        // Shade levels and colours are at most 255, so the product fits.
                        #[allow(clippy::cast_possible_truncation)]
                        MODEL.map(|m| ((u32::from(m) * u32::from(value - 1) + 127) / 255) as u8)
                    };
                    for (sum, channel) in sums.iter_mut().zip(colour) {
                        *sum += u32::from(channel);
                    }
                }
            }
            // An average of 8-bit channels fits a byte.
            #[allow(clippy::cast_possible_truncation)]
            rgb.extend(sums.map(|s| ((s + count / 2) / count) as u8));
        }
    }
    cancel.check(Checkpoint::RasterComplete)?;
    PreviewImage::new(width, height, rgb)
}

/// Write `image` as an RGB PNG.
///
/// # Errors
/// Destination I/O failures or cancellation.
pub fn write_png<W: std::io::Write>(
    writer: &mut W,
    image: &PreviewImage,
    cancel: &mut Cancellation<'_>,
) -> Result<(), crate::raster::png::PngError> {
    crate::raster::png::encode_rgb(writer, image.width, image.height, &image.rgb, cancel)
}

/// RGB565 word of an 8-bit colour (rounded per channel).
#[must_use]
pub fn rgb565([r, g, b]: [u8; 3]) -> u16 {
    let scale = |v: u8, max: u32| (u32::from(v) * max + 127) / 255;
    // Channels are scaled into 5, 6 and 5 bits.
    #[allow(clippy::cast_possible_truncation)]
    let word = (scale(r, 31) << 11 | scale(g, 63) << 5 | scale(b, 31)) as u16;
    word
}

/// The 15-bit CTB/Photon preview colour (R bits 15..11, G 10..6, B 4..0; bit 5 is the run flag).
#[must_use]
pub fn rgb15([r, g, b]: [u8; 3]) -> u16 {
    let scale = |v: u8| (u32::from(v) * 31 + 127) / 255;
    #[allow(clippy::cast_possible_truncation)] // Three 5-bit channels.
    let word = (scale(r) << 11 | scale(g) << 6 | scale(b)) as u16;
    word
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{ObjectTransform, Point3, Rotation3, Vector3};

    fn cube() -> TriangleMesh {
        let points = (0..8)
            .map(|i| {
                Point3::new(
                    f64::from(i & 1) * 10.,
                    f64::from((i >> 1) & 1) * 10.,
                    f64::from((i >> 2) & 1) * 10.,
                )
            })
            .collect();
        let quads = [
            [0, 2, 3, 1],
            [4, 5, 7, 6],
            [0, 1, 5, 4],
            [2, 6, 7, 3],
            [0, 4, 6, 2],
            [1, 3, 7, 5],
        ];
        let triangles = quads
            .iter()
            .flat_map(|q| [[q[0], q[1], q[2]], [q[0], q[2], q[3]]])
            .collect();
        TriangleMesh::try_new(points, triangles).unwrap()
    }

    fn identity() -> Affine3 {
        ObjectTransform::new(
            Vector3::new(0., 0., 0.),
            Rotation3::identity(),
            Vector3::new(1., 1., 1.),
            Point3::new(0., 0., 0.),
        )
        .unwrap()
        .to_affine()
        .unwrap()
    }

    #[test]
    fn a_cube_fills_the_center_and_leaves_the_corners_as_background() {
        let image =
            render_preview(&cube(), identity(), 116, 116, &mut Cancellation::never()).unwrap();
        assert_eq!(
            (image.width(), image.height(), image.rgb().len()),
            (116, 116, 116 * 116 * 3)
        );
        assert_eq!(image.pixel(0, 0), Some(BACKGROUND));
        assert_eq!(image.pixel(115, 115), Some(BACKGROUND));
        let center = image.pixel(58, 58).unwrap();
        assert_ne!(center, BACKGROUND);
        // Three visible faces get three distinct shades.
        let mut shades: Vec<[u8; 3]> = (0..116)
            .flat_map(|y| (0..116).map(move |x| (x, y)))
            .filter_map(|(x, y)| image.pixel(x, y))
            .filter(|&p| p != BACKGROUND)
            .collect();
        shades.sort_unstable();
        shades.dedup();
        assert!(shades.len() >= 3);
        // Rendering is deterministic, and translation does not move the fitted image.
        let again =
            render_preview(&cube(), identity(), 116, 116, &mut Cancellation::never()).unwrap();
        assert_eq!(image, again);
        let moved = ObjectTransform::new(
            Vector3::new(50., 20., 0.),
            Rotation3::identity(),
            Vector3::new(1., 1., 1.),
            Point3::new(0., 0., 0.),
        )
        .unwrap()
        .to_affine()
        .unwrap();
        let shifted = render_preview(&cube(), moved, 116, 116, &mut Cancellation::never()).unwrap();
        assert_eq!(image, shifted);
    }

    #[test]
    fn sizes_colours_and_empty_meshes() {
        let empty = TriangleMesh::try_new(Vec::new(), Vec::new()).unwrap();
        let image = render_preview(&empty, identity(), 4, 3, &mut Cancellation::never()).unwrap();
        assert!(image.rgb().chunks(3).all(|p| p == BACKGROUND));
        assert_eq!(
            render_preview(&cube(), identity(), 0, 3, &mut Cancellation::never()),
            Err(PreviewError::Size)
        );
        assert!(PreviewImage::new(2, 2, vec![0; 11]).is_err());
        assert_eq!(rgb565([255, 255, 255]), 0xffff);
        assert_eq!(rgb565([255, 0, 0]), 0xf800);
        assert_eq!(rgb15([255, 255, 255]), 0xffdf);
        assert_eq!(rgb15([0, 0, 255]), 0x001f);
        assert_eq!(rgb15([0, 255, 0]) & 0x0020, 0);
    }
}
