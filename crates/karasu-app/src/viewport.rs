//! The 3D viewport renderer (M6.2, TD-073), on the wgpu device eframe already uses.
//!
//! **Pass:** the scene renders into an offscreen colour target holding sRGB-encoded bytes, with a
//! depth buffer and 4x multisampling where the adapter supports it, and egui shows the resolved
//! texture as an image. Colours arrive linear and the shaders encode them, so what is shown is
//! exactly the configured colour on a face turned towards the viewer.
//! The scene stays independent of egui's own render pass.
//!
//! **Meshes:** each mesh is uploaded once as indexed `f32` positions relative to an origin near
//! the mesh. Draws place it with a model matrix, so moving, turning or scaling an object never
//! uploads vertices again. Shading is flat, from screen-space derivatives (see `viewport.wgsl`).
//!
//! **Lines:** the plate grid and the build volume, as coloured line segments.
//!
//! **Plate (TD-088):** the plate's surface and grid fade out while the view is below the plate,
//! so they never hide what is under the models (holes, supports); the border around the plate,
//! the front tab and the build volume's edges stay, so the plate's limits remain visible.
//!
//! **Label:** text lying on the plate (the front tab's word), drawn as glyph quads with depth,
//! from a snapshot of egui's font atlas taken when the text was laid out, so later changes to
//! the atlas never affect it.
//!
//! **Section (M6.4b3, TD-086):** meshes may be cut below a lower plane and above an upper one.
//! Where a plane opens a closed model it is capped: a stencil pass counts, for each pixel, the
//! parity of the model surfaces left on the line of sight once that plane alone cuts them (odd
//! where the plane lies in material), and a flat cap is drawn on the plane only there. A hollowed
//! model's cavity surface counts too, so its cap is the wall's cross-section around the
//! cavity. Holes and other meshes that are not closed solids are not capped.
//!
//! **Support sections (owner, 2026-10-30):** supports are capped too, in a colour of their own.
//! A support is a union of overlapping closed convex parts (frustums, spheres, the raft), so
//! parity would open the cap wherever two parts overlap. Their count is the winding instead:
//! front faces add one and back faces take one away in the stencil's low bits, so a pixel is in
//! a support where the count is not zero, however many parts hold it. The two planes share the
//! counter: each plane's support cap clears it where it is drawn and where it is hidden, so the
//! next plane counts from zero. Support caps come before the models' caps, which win where a tip
//! enters the model. They are never opened by holes.
//!
//! **Hole openings (TD-089):** the drain holes cut into an object are shown as openings, by
//! constructive solid geometry on the GPU, without changing a mesh. The holes are cylinders in
//! a data texture, two texels each, read with `textureLoad` (the OpenGL fallback has no storage
//! buffers in fragment shaders) and written only when they change. Each draw names the range of
//! holes that cut it ([`Draw::holes`]): its fragments inside them are discarded, which opens the
//! outer surface, the cavity's and the section's caps. A hole's bore is the far side of its
//! cylinder, drawn only where it lies in material: a stencil bit counts, for each pixel, the
//! parity of the object's surfaces beyond the bore on the line of sight, as the section's caps
//! do. A hole that runs on into the air or a cavity shows no bore there. Where holes overlap on
//! the screen, each pixel shows the bore of the hole the line of sight enters first.
//!
//! **Selection outline (M6.4f):** the selected object's whole silhouette is outlined. Its draws
//! are rendered first into a one-channel mask, uncut by the section and unopened by holes, so
//! the outline keeps the object's full shape in a section view. A full-screen pass at the end
//! then colours the pixels outside the mask within the outline's width of it, smoothing the
//! outer edge by distance, over everything else. A scissor keeps that pass to the object's
//! bounds on screen, so its cost follows the object's size, not the window's.
//!
//! **Surface filters (M6.4f):** a draw may carry a filter for the parts of its surface outside
//! the build volume: a colour, its opacity and a pattern (solid, or diagonal stripes on the
//! screen), mixed into the draw's colour before lighting, so the surface keeps its relief. The
//! test is per fragment against the volume in the frame uniforms, so the filter's edge follows
//! the volume's faces exactly, and nothing is uploaded when an object moves. Other surface
//! conditions (overhangs) are meant to take filters the same way.
//!
//! **Cavity view (owner, 2026-10-29):** a draw has a [`Look`]. A ghost is drawn after the opaque
//! scene in its colour's opacity without writing depth, so what lies inside shows through it
//! and what lies in front of it still hides it. A surface seen from inside leaves out the faces
//! turned towards the eye by their winding (the back ones under a mirroring placement), so a
//! cavity's far side shows with what stands in it. Neither is capped by the section. Both are
//! pipeline states only: nothing is uploaded again when the view changes.
use crate::camera::Mat4;
use eframe::{
    egui, egui_wgpu,
    wgpu::{self, util::DeviceExt as _},
};
use karasu_core::mesh::TriangleMesh;
use std::collections::HashMap;

/// Offscreen colour format. egui shows native textures as sRGB-encoded values, so the target
/// stores encoded bytes: the shaders light in linear values and encode their output.
const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
/// Depth with a stencil, which caps the section.
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth24PlusStencil8;
/// The selection outline's mask: coverage of the outlined draws, one channel.
const MASK_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R8Unorm;
/// The outline pass's uniforms: its colour and its width.
const OUTLINE_BYTES: u64 = 32;
/// Bytes of one uniform block: a 4 x 4 matrix and five vectors.
const UNIFORM_BYTES: u64 = 144;
/// Bytes of the frame's uniform block: a 4 x 4 matrix and eleven vectors.
const FRAME_BYTES: u64 = 240;
/// Under this height a surface rests on the plate and is no overhang, in mm: the shader's
/// interpolated `f32` positions on a model's base lying on the plate.
const PLATE_CONTACT_MM: f32 = 1.0e-2;
/// How far outside the build volume a surface must be to be filtered, in mm: the shader's
/// interpolated `f32` positions on a face lying exactly on a volume face (a model resting on the
/// plate) must not flicker in and out.
const VOLUME_TOLERANCE_MM: f32 = 1.0e-3;
/// Holes per row of the hole texture, as `hole_at` in `viewport.wgsl` reads it; each hole takes
/// two texels.
const HOLES_PER_ROW: u32 = 256;
/// The vertex attributes of coloured vertices: position and colour.
const COLOURED_ATTRIBUTES: [wgpu::VertexAttribute; 2] =
    wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x4];
/// Half the side of the section caps, in mm: larger than any build volume.
const CAP_HALF_SIDE_MM: f32 = 2000.0;

/// A mesh on the GPU.
struct GpuMesh {
    /// One part, or several when the mesh exceeds the device's largest buffer.
    parts: Vec<GpuPart>,
}

/// Triangles of a mesh with the vertices they use, each buffer within the device's limit.
struct GpuPart {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    index_count: u32,
}

impl GpuMesh {
    /// Draw every part with the pass's current pipeline and bind groups.
    fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        self.draw_triangles(pass, None);
    }

    /// Draw the `[first, count]` triangles of a mesh uploaded as one part (a support, TD-108),
    /// or every part with `None`. A range over a split mesh draws nothing: ranges are kept for
    /// meshes within one buffer.
    fn draw_triangles(&self, pass: &mut wgpu::RenderPass<'_>, triangles: Option<[u32; 2]>) {
        let one = self.parts.len() == 1;
        for part in &self.parts {
            let indices = match triangles {
                None => 0..part.index_count,
                Some([first, count]) if one => {
                    let end = first.saturating_add(count).saturating_mul(3);
                    first.saturating_mul(3).min(part.index_count)..end.min(part.index_count)
                }
                Some(_) => return,
            };
            pass.set_vertex_buffer(0, part.vertices.slice(..));
            pass.set_index_buffer(part.indices.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(indices, 0, 0..1);
        }
    }
}

/// Bytes a part needs at most per triangle: three indices and, at worst, three vertices of its
/// own, each three `f32` or one `u32`.
const PART_BYTES_PER_TRIANGLE: u64 = 3 * 4 + 3 * 3 * 4;

/// A mesh's triangles split into parts of at most `max_triangles`, each with the global indices
/// of the vertices it uses and its triangles indexed into them (a mesh larger than the device's
/// largest buffer, TD-094). A mesh within the limit is one part over all its vertices.
fn split_parts(
    vertex_count: usize,
    triangles: &[[u32; 3]],
    max_triangles: usize,
) -> Vec<(Vec<u32>, Vec<u32>)> {
    let mut parts = Vec::new();
    // Local index of each global vertex in the part being built; reset after each part.
    let mut local = vec![u32::MAX; vertex_count];
    for chunk in triangles.chunks(max_triangles.max(1)) {
        let mut used: Vec<u32> = Vec::new();
        let mut indices = Vec::with_capacity(chunk.len() * 3);
        for &v in chunk.iter().flatten() {
            let slot = &mut local[v as usize];
            if *slot == u32::MAX {
                *slot = u32::try_from(used.len()).unwrap_or(u32::MAX);
                used.push(v);
            }
            indices.push(*slot);
        }
        for &v in &used {
            local[v as usize] = u32::MAX;
        }
        parts.push((used, indices));
    }
    parts
}

/// One mesh drawn with a placement and a colour.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Draw {
    /// The mesh, as uploaded with [`Viewport::upload`].
    pub mesh: u64,
    /// Mesh vertices to world.
    pub model: Mat4,
    /// Linear RGB and alpha.
    pub color: [f32; 4],
    /// Drawn last, only where something nearer hides it, without writing depth (M6.4b2,
    /// TD-089): a drain hole's ghost shows it through the model, and never over its own solid
    /// or opening; translucent colours let the model show through it.
    pub overlay: bool,
    /// How the section caps this draw.
    pub cap: Cap,
    /// The first and the number of the holes ([`Viewport::set_holes`]) that cut this draw.
    pub holes: [u32; 2],
    /// This draw is the bore of the hole of this index: drawn only where it lies in the
    /// material of the draws its `holes` cut.
    pub bore: Option<u32>,
    /// The filter over the parts of this draw outside the build volume (M6.4f).
    pub outside: Option<Filter>,
    /// Only these `[first, count]` triangles of the mesh: one support of a built scene, in its
    /// own colour (TD-108); `None` draws them all.
    pub triangles: Option<[u32; 2]>,
    /// How the surface is shown: solid, as a ghost, or from inside (the cavity view).
    pub look: Look,
}

/// How the section caps a draw.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Cap {
    /// Not capped: not a closed solid, or not meant to be.
    #[default]
    Off,
    /// A closed solid, capped where the section's parity is odd (models and cavities).
    Model,
    /// A union of closed outward parts that may overlap, capped where their winding is not zero
    /// in the support colour (supports, owner 2026-10-30).
    Support,
}

/// How a draw's surface is shown (the cavity view, owner 2026-10-29).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Look {
    /// Opaque, as every surface is by default.
    #[default]
    Solid,
    /// Translucent in its colour's opacity, drawn after the opaque scene without writing
    /// depth: what is inside shows through it.
    Ghost,
    /// A closed surface seen from inside: its faces turned towards the eye (by their winding,
    /// outward) are left out, so its far side shows with what stands inside it.
    Inside,
}

/// A surface filter (M6.4f): a colour mixed into a draw's own where a condition holds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Filter {
    /// Linear RGB, and alpha as the filter's opacity.
    pub color: [f32; 4],
    pub pattern: Pattern,
}

/// How a filter covers the surface.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Pattern {
    /// Everywhere.
    Solid,
    /// Diagonal stripes on the screen, half of each period.
    Stripes,
}

impl Filter {
    /// The filter as the shader reads it: its colour, then its pattern (0 none, 1 solid,
    /// 2 stripes).
    fn shader(filter: Option<Self>) -> [[f32; 4]; 2] {
        match filter {
            None => [[0.0; 4], [0.0; 4]],
            Some(f) => {
                let pattern = match f.pattern {
                    Pattern::Solid => 1.0,
                    Pattern::Stripes => 2.0,
                };
                // Only models' surfaces have the filter: they also show their overhangs.
                [f.color, [pattern, 1.0, 0.0, 0.0]]
            }
        }
    }
}

/// The scene's lighting (a user setting): the light follows the view, so a face turned to the
/// viewer gets `brightness` and one seen edge-on `brightness × ambient`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shading {
    /// How bright the scene is: 1 shows a face turned to the viewer in its exact colour.
    pub brightness: f32,
    /// The share of the light a face gets edge-on, from 0 to 1.
    pub ambient: f32,
}

impl Default for Shading {
    fn default() -> Self {
        Self {
            brightness: 1.0,
            ambient: 0.35,
        }
    }
}

impl Shading {
    /// The lighting as the shader reads it: the edge-on light, then what facing the light
    /// adds.
    fn shader(self) -> [f32; 4] {
        let brightness = self.brightness.max(0.0);
        let ambient = self.ambient.clamp(0.0, 1.0);
        [brightness * ambient, brightness * (1.0 - ambient), 0.0, 0.0]
    }
}

/// How the models' overhangs are shown (TD-107): the critical angle, measured from the plate's
/// plane, and the colours at its limit and on a level surface facing the plate (linear RGB and
/// opacity).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OverhangStyle {
    /// The critical angle, in degrees, between 0 and 90.
    pub angle_deg: f32,
    /// The colour at the angle's limit.
    pub limit: [f32; 4],
    /// The colour of a level surface facing the plate.
    pub level: [f32; 4],
}

impl OverhangStyle {
    /// The style as the shader reads it: the cosine of the angle, the switch and the plate's
    /// contact height; then the two colours.
    fn shader(style: Option<Self>) -> [[f32; 4]; 3] {
        match style {
            None => [[0.0; 4]; 3],
            Some(s) => [
                [
                    s.angle_deg.clamp(0.0, 90.0).to_radians().cos(),
                    1.0,
                    PLATE_CONTACT_MM,
                    0.0,
                ],
                s.limit,
                s.level,
            ],
        }
    }
}

/// The contact's height line on the models (TD-137): a horizontal line around them at the
/// height where a support's contact is, or would be, placed, drawn by the models' shader.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Guide {
    /// The height, in world mm.
    pub height_mm: f64,
    /// Linear RGB and opacity.
    pub colour: [f32; 4],
}

impl Guide {
    /// Half the line's width, in pixels.
    const HALF_WIDTH_PX: f32 = 0.75;

    /// The line as the shader reads it: the height, the switch and the half width; then the
    /// colour.
    #[allow(clippy::cast_possible_truncation)] // Heights within a build volume.
    fn shader(guide: Option<Self>) -> [[f32; 4]; 2] {
        guide.map_or([[0.0; 4]; 2], |g| {
            [
                [g.height_mm as f32, 1.0, Self::HALF_WIDTH_PX, 0.0],
                g.colour,
            ]
        })
    }
}

/// The selection outline (M6.4f) of some draws.
#[derive(Clone, Debug, PartialEq)]
pub struct Outline {
    /// The outlined draws, by index into the frame's draws.
    pub draws: Vec<usize>,
    /// Linear RGB and alpha.
    pub color: [f32; 4],
    /// The width beyond the silhouette, in pixels.
    pub width: f32,
    /// The pixels the outline may cover (`x`, `y`, width, height, from the top left), or the
    /// whole image when `None`: the outlined draws' bounds on screen grown by the width.
    pub scissor: Option<[u32; 4]>,
}

impl Outline {
    /// The scissor of an outline `grow` pixels wide around the world box `min`..`max`, seen with
    /// `view_projection` in a `size` pixel image: the box's corners on screen, grown and clipped
    /// to the image. `None` (the whole image) when a corner is behind the eye, where the box's
    /// image is unbounded.
    #[must_use]
    pub fn scissor_of(
        view_projection: &Mat4,
        min: [f64; 3],
        max: [f64; 3],
        size: [u32; 2],
        grow: f64,
    ) -> Option<[u32; 4]> {
        let (width, height) = (f64::from(size[0]), f64::from(size[1]));
        let mut low = [f64::INFINITY; 2];
        let mut high = [f64::NEG_INFINITY; 2];
        for corner in 0..8 {
            let point = std::array::from_fn(|k| if corner >> k & 1 == 0 { min[k] } else { max[k] });
            let [ndc_x, ndc_y, _] = view_projection.project(point)?;
            // Normalised device coordinates to pixels from the top left.
            let pixel = [
                f64::midpoint(ndc_x, 1.0) * width,
                f64::midpoint(1.0, -ndc_y) * height,
            ];
            for k in 0..2 {
                low[k] = low[k].min(pixel[k]);
                high[k] = high[k].max(pixel[k]);
            }
        }
        let clip = |v: f64, limit: f64| v.clamp(0.0, limit);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // Clipped to the image.
        let [x0, y0, x1, y1] = [
            clip((low[0] - grow).floor(), width),
            clip((low[1] - grow).floor(), height),
            clip((high[0] + grow).ceil(), width),
            clip((high[1] + grow).ceil(), height),
        ]
        .map(|v| v as u32);
        Some([x0, y0, x1 - x0, y1 - y0])
    }
}

/// A drain hole's cylinder, in world millimetres, for the openings (TD-089).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HoleCylinder {
    /// The centre of the cylinder's start.
    pub entry_mm: [f64; 3],
    /// The unit axis.
    pub axis: [f64; 3],
    pub length_mm: f64,
    pub radius_mm: f64,
}

/// One end of a coloured line segment.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LineVertex {
    /// World position, in mm.
    pub position: [f32; 3],
    /// Linear RGB and alpha.
    pub color: [f32; 4],
}

/// One corner of a glyph quad of the plate's label.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LabelVertex {
    /// World position, in mm.
    pub position: [f32; 3],
    /// Texture coordinates in the font atlas snapshot, from 0 to 1.
    pub uv: [f32; 2],
    /// Linear RGB and alpha.
    pub color: [f32; 4],
}

/// The plate's label on the GPU: its glyph triangles and the atlas snapshot they sample.
struct Label {
    vertices: wgpu::Buffer,
    count: u32,
    group: wgpu::BindGroup,
}

/// The size-dependent render targets.
struct Targets {
    size: [u32; 2],
    /// The multisampled colour target, when multisampling.
    multisampled: Option<wgpu::TextureView>,
    /// The single-sample texture egui shows.
    resolved: wgpu::TextureView,
    depth: wgpu::TextureView,
    /// The selection outline's mask, and the outline pass's bind group over it.
    mask: wgpu::TextureView,
    outline_group: wgpu::BindGroup,
}

/// A per-draw uniform buffer and its bind group.
struct Slot {
    buffer: wgpu::Buffer,
    group: wgpu::BindGroup,
}

/// The renderer and its GPU resources.
pub struct Viewport {
    samples: u32,
    mesh_pipeline: wgpu::RenderPipeline,
    /// Meshes drawn where something nearer hides them ([`Draw::overlay`]).
    overlay_pipeline: wgpu::RenderPipeline,
    /// Translucent surfaces over the opaque scene ([`Look::Ghost`]).
    ghost_pipeline: wgpu::RenderPipeline,
    /// Closed surfaces seen from inside ([`Look::Inside`]): their front faces left out, or their
    /// back faces under a mirroring placement, which turns the winding over.
    inside_pipelines: [wgpu::RenderPipeline; 2],
    /// The section's opening parity, lower and upper plane.
    count_pipelines: [wgpu::RenderPipeline; 2],
    /// The section's caps, lower and upper plane.
    cap_pipelines: [wgpu::RenderPipeline; 2],
    /// The supports' winding at a section plane, and their caps where it is not zero (either
    /// plane: the quad carries its plane).
    wind_pipeline: wgpu::RenderPipeline,
    support_cap_pipeline: wgpu::RenderPipeline,
    /// The parity of the surfaces beyond each hole's bore, and the bores where it is odd.
    hole_count_pipeline: wgpu::RenderPipeline,
    bore_pipeline: wgpu::RenderPipeline,
    /// The holes' cylinders as last written, and the texture holding them, with `hole_rows`
    /// rows of [`HOLES_PER_ROW`] holes.
    holes: Vec<f32>,
    hole_texture: wgpu::Texture,
    hole_rows: u32,
    frame_layout: wgpu::BindGroupLayout,
    /// The cap colours of models and supports (linear RGB and alpha).
    cap_colors: [[f32; 4]; 2],
    /// The caps' quads, the models' lower and upper plane, then the supports': rebuilt only
    /// when a colour changes; their heights come from the frame uniforms.
    caps: Option<(wgpu::Buffer, u32)>,
    line_pipeline: wgpu::RenderPipeline,
    surface_pipeline: wgpu::RenderPipeline,
    /// The grid and the plate's surface, faded by [`Viewport::set_plate_fade`].
    faded_line_pipeline: wgpu::RenderPipeline,
    faded_surface_pipeline: wgpu::RenderPipeline,
    /// The opacity of the plate's surface and grid, from 0 to 1.
    plate_fade: f32,
    /// How the models' overhangs are shown, if they are (TD-107).
    overhangs: Option<OverhangStyle>,
    /// The contact's height line, if it shows (TD-137).
    guide: Option<Guide>,
    /// The lighting.
    shading: Shading,
    /// The build volume's world bounds, low and high corners in mm, for surface filters; none
    /// without a printer.
    volume: Option<[[f32; 3]; 2]>,
    /// The period of a filter's stripes, in pixels.
    stripe_period: f32,
    label_pipeline: wgpu::RenderPipeline,
    label_layout: wgpu::BindGroupLayout,
    label: Option<Label>,
    /// The selection outline: its mask's pipeline, the outline pass, the pass's bind group
    /// layout and uniforms.
    mask_pipeline: wgpu::RenderPipeline,
    outline_pipeline: wgpu::RenderPipeline,
    outline_layout: wgpu::BindGroupLayout,
    outline_buffer: wgpu::Buffer,
    frame_buffer: wgpu::Buffer,
    frame_group: wgpu::BindGroup,
    object_layout: wgpu::BindGroupLayout,
    slots: Vec<Slot>,
    meshes: HashMap<u64, GpuMesh>,
    lines: Option<(wgpu::Buffer, u32)>,
    surface: Option<(wgpu::Buffer, u32)>,
    grid: Option<(wgpu::Buffer, u32)>,
    plate: Option<(wgpu::Buffer, u32)>,
    targets: Option<Targets>,
    texture: Option<egui::TextureId>,
    /// The plates' thumbnails (TD-135): each its own targets and egui texture, by key, drawn
    /// with the same meshes, pipelines and slots as the view.
    thumbnails: std::collections::HashMap<u64, (Option<Targets>, Option<egui::TextureId>)>,
    /// While a thumbnail is drawn: the plate, its lines and the front word are left out.
    scene_hidden: bool,
}

/// `f32` values as native-endian bytes for a GPU buffer.
fn f32_bytes(values: impl IntoIterator<Item = f32>) -> Vec<u8> {
    values.into_iter().flat_map(f32::to_ne_bytes).collect()
}

/// No section: no mesh reaches this height.
const NO_SECTION: f32 = 1.0e30;

/// The section planes for the shader: the lowest and highest heights kept, each `None` when
/// that side is not cut.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Section {
    /// Meshes are cut away below this height.
    pub below: Option<f64>,
    /// Meshes are cut away above this height.
    pub above: Option<f64>,
}

impl Section {
    #[allow(clippy::cast_possible_truncation)] // GPU precision for a display cut.
    fn shader(self) -> [f32; 4] {
        [
            self.below.map_or(-NO_SECTION, |z| z as f32),
            self.above.map_or(NO_SECTION, |z| z as f32),
            0.0,
            0.0,
        ]
    }
}

/// A uniform block: a matrix and five vectors.
fn uniform<const N: usize>(matrix: &Mat4, vectors: [[f32; 4]; N]) -> Vec<u8> {
    f32_bytes(
        matrix
            .to_gpu()
            .into_iter()
            .chain(vectors.into_iter().flatten()),
    )
}

/// The holes that cut a draw, as the shader reads them: first, count and the hole whose bore
/// it is (-1 for none).
#[allow(clippy::cast_precision_loss)] // Hole indices stay far below 2^24.
fn hole_vector(draw: &Draw) -> [f32; 4] {
    [
        draw.holes[0] as f32,
        draw.holes[1] as f32,
        draw.bore.map_or(-1.0, |b| b as f32),
        0.0,
    ]
}

/// What a pipeline does with the stencil.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stencil {
    /// Nothing.
    Off,
    /// Flips `bit` for every surface drawn, without depth or colour: the parity of a section
    /// plane's opening.
    Count(u32),
    /// Draws only where `bit` is set: the section's cap.
    Cap(u32),
    /// Adds one for every front face and takes one for every back face in the low bits of
    /// `mask`, without depth or colour: the supports' winding at a section plane.
    Wind(u32),
    /// Draws only where the bits of `mask` are not zero, and clears them where it draws or is
    /// hidden: the supports' cap.
    Union(u32),
}

impl Stencil {
    /// Whether the pipeline only counts, writing neither depth nor colour.
    const fn counts(self) -> bool {
        matches!(self, Self::Count(_) | Self::Wind(_))
    }
}

/// The supports' winding counter: the stencil's low bits, where increments carry. Sixteen parts
/// overlapping at one point would wrap it to zero; supports never stack that many.
const SUPPORT_MASK: u32 = 0x0F;
/// The cap stencil bits of the lower and upper section planes.
const LOWER_BIT: u32 = 0x10;
const UPPER_BIT: u32 = 0x20;
/// The stencil bit of the holes' bores.
const HOLE_BIT: u32 = 0x40;

/// The frame's bind group layout: its uniforms and the holes.
fn frame_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("viewport-frame"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(FRAME_BYTES),
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
        ],
    })
}

/// A hole texture of `rows` rows of [`HOLES_PER_ROW`] holes, two `f32` RGBA texels each.
fn hole_texture(device: &wgpu::Device, rows: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("viewport-holes"),
        size: wgpu::Extent3d {
            width: 2 * HOLES_PER_ROW,
            height: rows.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba32Float,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

/// The frame's bind group over `uniforms` and the `holes` texture.
fn frame_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    uniforms: &wgpu::Buffer,
    holes: &wgpu::Texture,
) -> wgpu::BindGroup {
    let view = holes.create_view(&wgpu::TextureViewDescriptor::default());
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("viewport-frame"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: uniforms.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&view),
            },
        ],
    })
}

fn uniform_layout(device: &wgpu::Device, label: &str) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some(label),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: wgpu::BufferSize::new(UNIFORM_BYTES),
            },
            count: None,
        }],
    })
}

fn uniform_slot(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    label: &str,
) -> (wgpu::Buffer, wgpu::BindGroup) {
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: UNIFORM_BYTES,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(label),
        layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: buffer.as_entire_binding(),
        }],
    });
    (buffer, group)
}

/// Whether `draw` is capped as `cap`: drawn solid. A ghost or a surface seen from inside is
/// not capped: the cavity view shows the inside, not a cut.
fn is_capped(draw: &Draw, cap: Cap) -> bool {
    cap != Cap::Off && draw.cap == cap && !draw.overlay && draw.look == Look::Solid
}

/// Whether `model` mirrors (its linear part's determinant is negative), turning the winding of
/// the faces it places over.
fn mirrors(model: &Mat4) -> bool {
    let m = &model.0;
    let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
    det < 0.0
}

/// A pipeline over the shared shader module.
struct PipelineSpec<'a> {
    label: &'a str,
    vertex: &'a str,
    fragment: &'a str,
    buffers: &'a [Option<wgpu::VertexBufferLayout<'a>>],
    topology: wgpu::PrimitiveTopology,
    /// Depth bias pushing the primitives back (lines stay visible on coplanar faces).
    bias: i32,
    /// Drawn last where something nearer hides it, without writing depth.
    overlay: bool,
    stencil: Stencil,
    /// The faces left out, by their winding.
    cull: Option<wgpu::Face>,
    /// Drawn without writing depth, over what is nearer only where it is nearer itself.
    ghost: bool,
}

fn pipeline(
    device: &wgpu::Device,
    module: &wgpu::ShaderModule,
    layout: &wgpu::PipelineLayout,
    samples: u32,
    spec: &PipelineSpec<'_>,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(spec.label),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module,
            entry_point: Some(spec.vertex),
            buffers: spec.buffers,
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        primitive: wgpu::PrimitiveState {
            topology: spec.topology,
            cull_mode: spec.cull,
            ..wgpu::PrimitiveState::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: Some(!spec.overlay && !spec.ghost && !spec.stencil.counts()),
            depth_compare: Some(if spec.stencil.counts() {
                wgpu::CompareFunction::Always
            } else if spec.overlay {
                // Only where something nearer hides it.
                wgpu::CompareFunction::Greater
            } else {
                wgpu::CompareFunction::LessEqual
            }),
            stencil: stencil_state(spec.stencil),
            bias: wgpu::DepthBiasState {
                constant: spec.bias,
                slope_scale: if spec.bias == 0 { 0.0 } else { 1.0 },
                clamp: 0.0,
            },
        }),
        multisample: wgpu::MultisampleState {
            count: samples,
            mask: !0,
            alpha_to_coverage_enabled: false,
        },
        fragment: Some(wgpu::FragmentState {
            module,
            entry_point: Some(spec.fragment),
            targets: &[Some(wgpu::ColorTargetState {
                format: COLOR_FORMAT,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: if spec.stencil.counts() {
                    wgpu::ColorWrites::empty()
                } else {
                    wgpu::ColorWrites::ALL
                },
            })],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        }),
        multiview_mask: None,
        cache: None,
    })
}

/// The stencil state of a pipeline.
fn stencil_state(stencil: Stencil) -> wgpu::StencilState {
    let face = |compare, pass_op| wgpu::StencilFaceState {
        compare,
        fail_op: wgpu::StencilOperation::Keep,
        depth_fail_op: wgpu::StencilOperation::Keep,
        pass_op,
    };
    match stencil {
        Stencil::Off => wgpu::StencilState::default(),
        Stencil::Count(bit) => {
            let flip = face(
                wgpu::CompareFunction::Always,
                wgpu::StencilOperation::Invert,
            );
            wgpu::StencilState {
                front: flip,
                back: flip,
                read_mask: 0,
                write_mask: bit,
            }
        }
        Stencil::Cap(bit) => {
            let only = face(wgpu::CompareFunction::Equal, wgpu::StencilOperation::Keep);
            wgpu::StencilState {
                front: only,
                back: only,
                read_mask: bit,
                write_mask: 0,
            }
        }
        Stencil::Wind(mask) => wgpu::StencilState {
            front: face(
                wgpu::CompareFunction::Always,
                wgpu::StencilOperation::IncrementWrap,
            ),
            back: face(
                wgpu::CompareFunction::Always,
                wgpu::StencilOperation::DecrementWrap,
            ),
            read_mask: 0,
            write_mask: mask,
        },
        Stencil::Union(mask) => {
            // Drawn where the count is not zero (the reference is zero); cleared where drawn or
            // hidden, so the count is zero everywhere once the cap is down.
            let clear = wgpu::StencilFaceState {
                compare: wgpu::CompareFunction::NotEqual,
                fail_op: wgpu::StencilOperation::Keep,
                depth_fail_op: wgpu::StencilOperation::Zero,
                pass_op: wgpu::StencilOperation::Zero,
            };
            wgpu::StencilState {
                front: clear,
                back: clear,
                read_mask: mask,
                write_mask: mask,
            }
        }
    }
}

/// The selection outline's pipelines: the mask of the outlined draws (single-sampled, without
/// depth, uncut), and the full-screen outline pass over it in the scene's pass, drawn over
/// everything. Also the outline pass's bind group layout: the mask and the uniforms.
fn outline_pipelines(
    device: &wgpu::Device,
    module: &wgpu::ShaderModule,
    frame_layout: &wgpu::BindGroupLayout,
    object_layout: &wgpu::BindGroupLayout,
    samples: u32,
) -> (
    wgpu::RenderPipeline,
    wgpu::RenderPipeline,
    wgpu::BindGroupLayout,
) {
    let mask_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("viewport-outline-mask"),
        bind_group_layouts: &[Some(frame_layout), Some(object_layout)],
        immediate_size: 0,
    });
    let mask = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("viewport-outline-mask"),
        layout: Some(&mask_layout),
        vertex: wgpu::VertexState {
            module,
            entry_point: Some("mesh_vs"),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: 12,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &wgpu::vertex_attr_array![0 => Float32x3],
            })],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module,
            entry_point: Some("mask_fs"),
            targets: &[Some(wgpu::ColorTargetState {
                format: MASK_FORMAT,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        }),
        multiview_mask: None,
        cache: None,
    });
    let outline_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("viewport-outline"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(OUTLINE_BYTES),
                },
                count: None,
            },
        ],
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("viewport-outline"),
        bind_group_layouts: &[Some(frame_layout), None, Some(&outline_layout)],
        immediate_size: 0,
    });
    // At the near plane, so it passes the depth test everywhere: over everything.
    let outline = pipeline(
        device,
        module,
        &layout,
        samples,
        &PipelineSpec {
            label: "viewport-outline",
            vertex: "outline_vs",
            fragment: "outline_fs",
            buffers: &[],
            topology: wgpu::PrimitiveTopology::TriangleList,
            bias: 0,
            overlay: false,
            stencil: Stencil::Off,
            cull: None,
            ghost: false,
        },
    );
    (mask, outline, outline_layout)
}

/// 4x multisampling when the adapter supports it for both targets, otherwise none.
fn sample_count(adapter: &wgpu::Adapter) -> u32 {
    let supports = |format| {
        adapter
            .get_texture_format_features(format)
            .flags
            .sample_count_supported(4)
    };
    if supports(COLOR_FORMAT) && supports(DEPTH_FORMAT) {
        4
    } else {
        1
    }
}

impl Viewport {
    /// Create the pipelines on eframe's device.
    #[must_use]
    #[allow(clippy::too_many_lines)] // Every pipeline of the viewport, declared together.
    pub fn new(state: &egui_wgpu::RenderState) -> Self {
        let device = &state.device;
        let samples = sample_count(&state.adapter);
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("viewport"),
            source: wgpu::ShaderSource::Wgsl(include_str!("viewport.wgsl").into()),
        });
        let frame_layout = frame_layout(device);
        let object_layout = uniform_layout(device, "viewport-object");
        let frame_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("viewport-frame"),
            size: FRAME_BYTES,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let hole_texture = hole_texture(device, 1);
        let frame_group = frame_group(device, &frame_layout, &frame_buffer, &hole_texture);
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("viewport-mesh"),
            bind_group_layouts: &[Some(&frame_layout), Some(&object_layout)],
            immediate_size: 0,
        });
        // Lines are in world coordinates: the frame uniforms only.
        let line_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("viewport-lines"),
            bind_group_layouts: &[Some(&frame_layout)],
            immediate_size: 0,
        });
        let mesh_spec = |label, overlay, stencil| PipelineSpec {
            label,
            vertex: "mesh_vs",
            fragment: match stencil {
                Stencil::Count(HOLE_BIT) => "hole_count_fs",
                Stencil::Cap(HOLE_BIT) => "bore_fs",
                _ => "mesh_fs",
            },
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: 12,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &wgpu::vertex_attr_array![0 => Float32x3],
            })],
            topology: wgpu::PrimitiveTopology::TriangleList,
            bias: 2,
            overlay,
            stencil,
            cull: None,
            ghost: false,
        };
        let mesh_pipeline = pipeline(
            device,
            &module,
            &layout,
            samples,
            &mesh_spec("viewport-mesh", false, Stencil::Off),
        );
        let overlay_pipeline = pipeline(
            device,
            &module,
            &layout,
            samples,
            &mesh_spec("viewport-overlay", true, Stencil::Off),
        );
        let ghost_pipeline = pipeline(
            device,
            &module,
            &layout,
            samples,
            &PipelineSpec {
                ghost: true,
                ..mesh_spec("viewport-ghost", false, Stencil::Off)
            },
        );
        let inside_pipelines = [wgpu::Face::Front, wgpu::Face::Back].map(|face| {
            pipeline(
                device,
                &module,
                &layout,
                samples,
                &PipelineSpec {
                    cull: Some(face),
                    ..mesh_spec("viewport-inside", false, Stencil::Off)
                },
            )
        });
        let count_pipelines = [LOWER_BIT, UPPER_BIT].map(|bit| {
            pipeline(
                device,
                &module,
                &layout,
                samples,
                &mesh_spec("viewport-section-count", false, Stencil::Count(bit)),
            )
        });
        let wind_pipeline = pipeline(
            device,
            &module,
            &layout,
            samples,
            &mesh_spec("viewport-support-wind", false, Stencil::Wind(SUPPORT_MASK)),
        );
        let hole_count_pipeline = pipeline(
            device,
            &module,
            &layout,
            samples,
            &mesh_spec("viewport-hole-count", false, Stencil::Count(HOLE_BIT)),
        );
        let bore_pipeline = pipeline(
            device,
            &module,
            &layout,
            samples,
            &mesh_spec("viewport-bore", false, Stencil::Cap(HOLE_BIT)),
        );
        let coloured_layout = || {
            Some(wgpu::VertexBufferLayout {
                array_stride: 28,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &COLOURED_ATTRIBUTES,
            })
        };
        let cap_pipeline = |label, fragment, stencil| {
            pipeline(
                device,
                &module,
                &line_layout,
                samples,
                &PipelineSpec {
                    label,
                    vertex: "cap_vs",
                    fragment,
                    buffers: &[coloured_layout()],
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    bias: 0,
                    overlay: false,
                    stencil,
                    cull: None,
                    ghost: false,
                },
            )
        };
        let cap_pipelines = [LOWER_BIT, UPPER_BIT]
            .map(|bit| cap_pipeline("viewport-section-cap", "cap_fs", Stencil::Cap(bit)));
        // One for both planes: they share the supports' counter, which each cap clears.
        let support_cap_pipeline = cap_pipeline(
            "viewport-support-cap",
            "support_cap_fs",
            Stencil::Union(SUPPORT_MASK),
        );
        let line_pipeline_with = |label, fragment| {
            pipeline(
                device,
                &module,
                &line_layout,
                samples,
                &PipelineSpec {
                    label,
                    vertex: "line_vs",
                    fragment,
                    buffers: &[coloured_layout()],
                    topology: wgpu::PrimitiveTopology::LineList,
                    bias: 0,
                    overlay: false,
                    stencil: Stencil::Off,
                    cull: None,
                    ghost: false,
                },
            )
        };
        let line_pipeline = line_pipeline_with("viewport-lines", "line_fs");
        let faded_line_pipeline = line_pipeline_with("viewport-grid", "faded_fs");
        // Unlit coloured triangles (the plate), pushed back like meshes so lines stay on top.
        let surface_pipeline_with = |label, fragment| {
            pipeline(
                device,
                &module,
                &line_layout,
                samples,
                &PipelineSpec {
                    label,
                    vertex: "line_vs",
                    fragment,
                    buffers: &[coloured_layout()],
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    bias: 2,
                    overlay: false,
                    stencil: Stencil::Off,
                    cull: None,
                    ghost: false,
                },
            )
        };
        let surface_pipeline = surface_pipeline_with("viewport-surface", "line_fs");
        let faded_surface_pipeline = surface_pipeline_with("viewport-plate", "faded_fs");
        let label_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("viewport-label"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let label_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("viewport-label"),
                bind_group_layouts: &[Some(&frame_layout), None, Some(&label_layout)],
                immediate_size: 0,
            });
        // Glyphs lie on the plate: depth-tested like lines, so models in front hide them.
        let label_pipeline = pipeline(
            device,
            &module,
            &label_pipeline_layout,
            samples,
            &PipelineSpec {
                label: "viewport-label",
                vertex: "label_vs",
                fragment: "label_fs",
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: 36,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![
                        0 => Float32x3, 1 => Float32x2, 2 => Float32x4
                    ],
                })],
                topology: wgpu::PrimitiveTopology::TriangleList,
                bias: 0,
                overlay: false,
                stencil: Stencil::Off,
                cull: None,
                ghost: false,
            },
        );
        let (mask_pipeline, outline_pipeline, outline_layout) =
            outline_pipelines(device, &module, &frame_layout, &object_layout, samples);
        let outline_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("viewport-outline"),
            size: OUTLINE_BYTES,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            samples,
            mesh_pipeline,
            overlay_pipeline,
            ghost_pipeline,
            inside_pipelines,
            count_pipelines,
            cap_pipelines,
            wind_pipeline,
            support_cap_pipeline,
            hole_count_pipeline,
            bore_pipeline,
            holes: Vec::new(),
            hole_texture,
            hole_rows: 1,
            frame_layout,
            cap_colors: [[0.5, 0.5, 0.5, 1.0]; 2],
            caps: coloured(
                device,
                "viewport-section-caps",
                &cap_quads([[0.5, 0.5, 0.5, 1.0]; 2]),
            ),
            line_pipeline,
            surface_pipeline,
            faded_line_pipeline,
            faded_surface_pipeline,
            plate_fade: 1.0,
            overhangs: None,
            guide: None,
            shading: Shading::default(),
            volume: None,
            stripe_period: 12.0,
            label_pipeline,
            label_layout,
            label: None,
            mask_pipeline,
            outline_pipeline,
            outline_layout,
            outline_buffer,
            frame_buffer,
            frame_group,
            object_layout,
            slots: Vec::new(),
            meshes: HashMap::new(),
            lines: None,
            surface: None,
            grid: None,
            plate: None,
            targets: None,
            texture: None,
            thumbnails: std::collections::HashMap::new(),
            scene_hidden: false,
        }
    }

    /// Set the colours of the section's caps, the models' then the supports' (linear RGB and
    /// alpha). Their quads are uploaded again only when a colour changes.
    pub fn set_cap_colors(&mut self, device: &wgpu::Device, colors: [[f32; 4]; 2]) {
        #[allow(clippy::float_cmp)] // The same configured colours, not computed ones.
        if colors != self.cap_colors {
            self.cap_colors = colors;
            self.caps = coloured(device, "viewport-section-caps", &cap_quads(colors));
        }
    }

    /// Set the drain holes the draws' [`Draw::holes`] ranges index. The texture is written only
    /// when they change, and grows by rows as needed.
    #[allow(clippy::cast_possible_truncation)] // GPU precision for display.
    pub fn set_holes(&mut self, state: &egui_wgpu::RenderState, holes: &[HoleCylinder]) {
        let values: Vec<f32> = holes
            .iter()
            .flat_map(|h| {
                let entry = h.entry_mm.into_iter().chain([h.radius_mm]);
                entry.chain(h.axis).chain([h.length_mm])
            })
            .map(|v| v as f32)
            .collect();
        #[allow(clippy::float_cmp)] // Exactly the values written last.
        if values == self.holes {
            return;
        }
        let per_row = HOLES_PER_ROW as usize;
        let rows = u32::try_from(holes.len().div_ceil(per_row)).unwrap_or(u32::MAX);
        if rows > self.hole_rows {
            self.hole_rows = rows.next_power_of_two();
            self.hole_texture = hole_texture(&state.device, self.hole_rows);
            self.frame_group = frame_group(
                &state.device,
                &self.frame_layout,
                &self.frame_buffer,
                &self.hole_texture,
            );
        }
        if rows > 0 {
            // Whole rows: the last one padded with zeros, which no draw's range reaches.
            let mut padded = values.clone();
            padded.resize(rows as usize * per_row * 8, 0.0);
            state.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.hole_texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &f32_bytes(padded),
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(2 * HOLES_PER_ROW * 16),
                    rows_per_image: Some(rows),
                },
                wgpu::Extent3d {
                    width: 2 * HOLES_PER_ROW,
                    height: rows,
                    depth_or_array_layers: 1,
                },
            );
        }
        self.holes = values;
    }

    /// Samples per pixel of the scene (4 with multisampling, otherwise 1).
    #[must_use]
    pub fn samples(&self) -> u32 {
        self.samples
    }

    /// Whether the mesh `key` is uploaded.
    #[must_use]
    pub fn has_mesh(&self, key: u64) -> bool {
        self.meshes.contains_key(&key)
    }

    /// Free the mesh `key`, if uploaded.
    pub fn remove(&mut self, key: u64) {
        self.meshes.remove(&key);
    }

    /// Upload `mesh` as `key`, with vertex positions relative to `origin`.
    pub fn upload(
        &mut self,
        device: &wgpu::Device,
        key: u64,
        mesh: &TriangleMesh,
        origin: [f64; 3],
    ) {
        let points = mesh.vertices();
        #[allow(clippy::cast_possible_truncation)] // GPU precision, relative to a nearby origin.
        let position = |v: u32| {
            let p = points[v as usize];
            [p.x - origin[0], p.y - origin[1], p.z - origin[2]].map(|c| c as f32)
        };
        let bytes = |indices: &mut dyn Iterator<Item = u32>| -> Vec<u8> {
            indices.flat_map(u32::to_ne_bytes).collect()
        };
        let part = |vertices: Vec<u8>, indices: Vec<u8>| GpuPart {
            vertices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("viewport-mesh-vertices"),
                contents: &vertices,
                usage: wgpu::BufferUsages::VERTEX,
            }),
            index_count: u32::try_from(indices.len() / 4).unwrap_or(u32::MAX),
            indices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("viewport-mesh-indices"),
                contents: &indices,
                usage: wgpu::BufferUsages::INDEX,
            }),
        };
        // TD-094: a buffer over the device's limit is a validation error, which ends the
        // application; a large cavity is drawn in parts instead.
        let limit = device.limits().max_buffer_size;
        let vertex_bytes = points.len() as u64 * 12;
        let index_bytes = mesh.triangles().len() as u64 * 12;
        let parts = if vertex_bytes <= limit && index_bytes <= limit {
            let vertices = f32_bytes(
                (0..points.len()).flat_map(|v| position(u32::try_from(v).unwrap_or(u32::MAX))),
            );
            vec![part(
                vertices,
                bytes(&mut mesh.triangles().iter().flatten().copied()),
            )]
        } else {
            let max_triangles = usize::try_from(limit / PART_BYTES_PER_TRIANGLE).unwrap_or(1);
            split_parts(points.len(), mesh.triangles(), max_triangles)
                .into_iter()
                .map(|(used, indices)| {
                    part(
                        f32_bytes(used.into_iter().flat_map(position)),
                        bytes(&mut indices.into_iter()),
                    )
                })
                .collect()
        };
        self.meshes.insert(key, GpuMesh { parts });
    }

    /// Replace the line segments (pairs of vertices): the `grid`, which fades with the plate,
    /// and the `edges`, which stay.
    pub fn set_lines(&mut self, device: &wgpu::Device, grid: &[LineVertex], edges: &[LineVertex]) {
        self.grid = coloured(device, "viewport-grid", grid);
        self.lines = coloured(device, "viewport-lines", edges);
    }

    /// Replace the unlit surface triangles (triples of vertices): the `plate`, which fades, and
    /// the `marks` around it, which stay.
    pub fn set_surface(
        &mut self,
        device: &wgpu::Device,
        plate: &[LineVertex],
        marks: &[LineVertex],
    ) {
        self.plate = coloured(device, "viewport-plate", plate);
        self.surface = coloured(device, "viewport-surface", marks);
    }

    /// Set the build volume the surface filters test against (world mm, low and high corners),
    /// or none, and the period of their stripes in pixels.
    pub fn set_filters(&mut self, volume: Option<[[f32; 3]; 2]>, stripe_period: f32) {
        self.volume = volume;
        self.stripe_period = stripe_period.max(2.0);
    }

    /// Light the scene with `shading`.
    pub fn set_shading(&mut self, shading: Shading) {
        self.shading = shading;
    }

    /// Show the models' overhangs (TD-107) with `style`, or not with `None`.
    pub fn set_overhangs(&mut self, style: Option<OverhangStyle>) {
        self.overhangs = style;
    }

    /// Show the contact's height line (TD-137), or not with `None`: a uniform, so moving it
    /// rebuilds nothing.
    pub fn set_guide(&mut self, guide: Option<Guide>) {
        self.guide = guide;
    }

    /// Set the opacity of the plate's surface and grid, from 0 (hidden) to 1.
    pub fn set_plate_fade(&mut self, fade: f32) {
        self.plate_fade = fade.clamp(0.0, 1.0);
    }

    /// Replace the plate's label: glyph triangles (triples of vertices) sampling `atlas`, a
    /// snapshot of the font atlas whose alpha is the glyph coverage. No vertices clear it.
    pub fn set_label(
        &mut self,
        state: &egui_wgpu::RenderState,
        atlas: &egui::ColorImage,
        vertices: &[LabelVertex],
    ) {
        let [width, height] = atlas.size.map(|n| u32::try_from(n).unwrap_or(u32::MAX));
        if vertices.is_empty() || width == 0 || height == 0 {
            self.label = None;
            return;
        }
        let device = &state.device;
        let texture = device.create_texture_with_data(
            &state.queue,
            &wgpu::TextureDescriptor {
                label: Some("viewport-label-atlas"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            &atlas
                .pixels
                .iter()
                .flat_map(egui::Color32::to_array)
                .collect::<Vec<u8>>(),
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("viewport-label"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..wgpu::SamplerDescriptor::default()
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("viewport-label"),
            layout: &self.label_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });
        let bytes = f32_bytes(
            vertices
                .iter()
                .flat_map(|v| v.position.into_iter().chain(v.uv).chain(v.color)),
        );
        self.label = Some(Label {
            vertices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("viewport-label"),
                contents: &bytes,
                usage: wgpu::BufferUsages::VERTEX,
            }),
            count: u32::try_from(vertices.len()).unwrap_or(u32::MAX),
            group,
        });
    }
}

/// The section caps' quads, the models' in `colors[0]` then the supports' in `colors[1]`: each
/// the lower plane's, then the upper plane's, with its plane's index in z (`cap_vs` puts it at
/// the plane's height). Quad `2 * kind + plane` starts at vertex `6 *` that.
fn cap_quads(colors: [[f32; 4]; 2]) -> Vec<LineVertex> {
    let h = CAP_HALF_SIDE_MM;
    colors
        .into_iter()
        .flat_map(|color| {
            [0.0, 1.0].into_iter().flat_map(move |plane| {
                [[-h, -h], [h, -h], [h, h], [-h, -h], [h, h], [-h, h]].map(|[x, y]| LineVertex {
                    position: [x, y, plane],
                    color,
                })
            })
        })
        .collect()
}

/// A vertex buffer of coloured vertices, or `None` when there are none.
fn coloured(
    device: &wgpu::Device,
    label: &str,
    vertices: &[LineVertex],
) -> Option<(wgpu::Buffer, u32)> {
    {
        (!vertices.is_empty()).then(|| {
            let bytes = f32_bytes(
                vertices
                    .iter()
                    .flat_map(|v| v.position.into_iter().chain(v.color)),
            );
            let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents: &bytes,
                usage: wgpu::BufferUsages::VERTEX,
            });
            (buffer, u32::try_from(vertices.len()).unwrap_or(u32::MAX))
        })
    }
}

impl Viewport {
    /// Render a plate's thumbnail (TD-135): `draws` with `view_projection` into the `size`
    /// pixel image kept under `key`, over `clear`, without the view's plate, lines, front word,
    /// section, holes or outline, and return its texture. The view's own image is untouched:
    /// only the targets are the thumbnail's, the meshes and pipelines are shared.
    #[allow(clippy::too_many_arguments)] // One frame's inputs, as `render` takes them.
    pub fn render_thumbnail(
        &mut self,
        state: &egui_wgpu::RenderState,
        key: u64,
        size: [u32; 2],
        view_projection: &Mat4,
        eye: [f32; 3],
        light: [f32; 3],
        clear: [f64; 4],
        draws: &[Draw],
    ) -> Option<egui::TextureId> {
        let (targets, texture) = self.thumbnails.remove(&key).unwrap_or((None, None));
        let main = (
            std::mem::replace(&mut self.targets, targets),
            std::mem::replace(&mut self.texture, texture),
        );
        self.scene_hidden = true;
        let fade = std::mem::replace(&mut self.plate_fade, 0.0);
        let shown = self.render(
            state,
            size,
            view_projection,
            eye,
            light,
            Section::default(),
            clear,
            draws,
            None,
        );
        self.plate_fade = fade;
        self.scene_hidden = false;
        let thumbnail = (
            std::mem::replace(&mut self.targets, main.0),
            std::mem::replace(&mut self.texture, main.1),
        );
        self.thumbnails.insert(key, thumbnail);
        shown
    }

    /// Forget the thumbnail kept under `key`, freeing its texture.
    pub fn drop_thumbnail(&mut self, state: &egui_wgpu::RenderState, key: u64) {
        if let Some((_, Some(texture))) = self.thumbnails.remove(&key) {
            state.renderer.write().free_texture(&texture);
        }
    }
}

impl Viewport {
    /// (Re)create the targets for `size` pixels and register the texture with egui.
    /// Render the coverage of the `outline`'s draws into the mask, uncut: their slots are the
    /// frame's, and the mask pipeline's shader ignores the section and the holes.
    fn outline_mask(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        targets: &Targets,
        draws: &[Draw],
        outline: &Outline,
    ) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("viewport-outline-mask"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &targets.mask,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: None,
            occlusion_query_set: None,
            timestamp_writes: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.mask_pipeline);
        pass.set_bind_group(0, &self.frame_group, &[]);
        for &index in &outline.draws {
            let (Some(draw), Some(slot)) = (draws.get(index), self.slots.get(index)) else {
                continue;
            };
            let Some(mesh) = self.meshes.get(&draw.mesh) else {
                continue;
            };
            pass.set_bind_group(1, &slot.group, &[]);
            mesh.draw_triangles(&mut pass, draw.triangles);
        }
    }

    fn ensure_targets(&mut self, state: &egui_wgpu::RenderState, size: [u32; 2]) {
        if self.targets.as_ref().is_some_and(|t| t.size == size) {
            return;
        }
        let device = &state.device;
        let texture = |label, format, samples, usage| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d {
                        width: size[0],
                        height: size[1],
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: samples,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage,
                    view_formats: &[],
                })
                .create_view(&wgpu::TextureViewDescriptor::default())
        };
        let attachment = wgpu::TextureUsages::RENDER_ATTACHMENT;
        let mask = texture(
            "viewport-outline-mask",
            MASK_FORMAT,
            1,
            attachment | wgpu::TextureUsages::TEXTURE_BINDING,
        );
        let resolved = texture(
            "viewport-color",
            COLOR_FORMAT,
            1,
            attachment | wgpu::TextureUsages::TEXTURE_BINDING,
        );
        let targets = Targets {
            size,
            multisampled: (self.samples > 1).then(|| {
                texture(
                    "viewport-color-msaa",
                    COLOR_FORMAT,
                    self.samples,
                    attachment,
                )
            }),
            depth: texture("viewport-depth", DEPTH_FORMAT, self.samples, attachment),
            outline_group: device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("viewport-outline"),
                layout: &self.outline_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&mask),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: self.outline_buffer.as_entire_binding(),
                    },
                ],
            }),
            mask,
            resolved,
        };
        let mut renderer = state.renderer.write();
        match self.texture {
            Some(id) => renderer.update_egui_texture_from_wgpu_texture(
                device,
                &targets.resolved,
                wgpu::FilterMode::Linear,
                id,
            ),
            None => {
                self.texture = Some(renderer.register_native_texture(
                    device,
                    &targets.resolved,
                    wgpu::FilterMode::Linear,
                ));
            }
        }
        self.targets = Some(targets);
    }

    /// Render `draws` and the lines with `view_projection` into a `size` pixel image, lit from
    /// `light`, over `clear` (sRGB-encoded channels and alpha, 0 to 1), and return the texture
    /// egui shows. The `section` cuts meshes below and above its planes and caps the closed
    /// solids it opens; the plate and lines are never cut.
    ///
    /// Colours may be translucent: lines, the plate and handles blend over what is behind them.
    /// Translucent models need a sorted pass without depth writes, which is not here yet: they
    /// blend in plate order (TD-076).
    ///
    /// The `outline`'s draws are outlined over everything, uncut by the section.
    #[allow(clippy::too_many_arguments, clippy::too_many_lines)] // One frame, pass by pass.
    pub fn render(
        &mut self,
        state: &egui_wgpu::RenderState,
        size: [u32; 2],
        view_projection: &Mat4,
        eye: [f32; 3],
        light: [f32; 3],
        section: Section,
        clear: [f64; 4],
        draws: &[Draw],
        outline: Option<&Outline>,
    ) -> Option<egui::TextureId> {
        let size = [size[0].max(1), size[1].max(1)];
        self.ensure_targets(state, size);
        let (device, queue) = (&state.device, &state.queue);
        let mut frame = section.shader();
        frame[2] = self.plate_fade;
        #[allow(clippy::cast_precision_loss)] // Hole counts stay far below 2^24.
        let holes = (self.holes.len() / 8) as f32;
        let overhang = OverhangStyle::shader(self.overhangs);
        let guide = Guide::shader(self.guide);
        let ([low, high], on) = match self.volume {
            Some(bounds) => (bounds, 1.0),
            None => ([[0.0; 3]; 2], 0.0),
        };
        queue.write_buffer(
            &self.frame_buffer,
            0,
            &uniform(
                view_projection,
                [
                    [light[0], light[1], light[2], 0.0],
                    frame,
                    [eye[0], eye[1], eye[2], holes],
                    [
                        low[0] - VOLUME_TOLERANCE_MM,
                        low[1] - VOLUME_TOLERANCE_MM,
                        low[2] - VOLUME_TOLERANCE_MM,
                        self.stripe_period,
                    ],
                    [
                        high[0] + VOLUME_TOLERANCE_MM,
                        high[1] + VOLUME_TOLERANCE_MM,
                        high[2] + VOLUME_TOLERANCE_MM,
                        on,
                    ],
                    overhang[0],
                    overhang[1],
                    overhang[2],
                    self.shading.shader(),
                    guide[0],
                    guide[1],
                ],
            ),
        );
        // Each draw cut by both planes; then, for each cut plane, each capped draw cut by that
        // plane alone, to count its opening.
        let planes: Vec<(usize, [f32; 4])> = [
            (0, section.below, [1.0, 0.0, 0.0, 0.0]),
            (1, section.above, [0.0, 1.0, 0.0, 0.0]),
        ]
        .into_iter()
        .filter_map(|(plane, height, clip)| height.map(|_| (plane, clip)))
        .collect();
        let capped: Vec<&Draw> = draws.iter().filter(|d| is_capped(d, Cap::Model)).collect();
        let supports: Vec<&Draw> = draws
            .iter()
            .filter(|d| is_capped(d, Cap::Support))
            .collect();
        let needed = draws.len() + planes.len() * (capped.len() + supports.len());
        let bores = draws.iter().any(|d| d.bore.is_some());
        while self.slots.len() < needed {
            let (buffer, group) = uniform_slot(device, &self.object_layout, "viewport-object");
            self.slots.push(Slot { buffer, group });
        }
        for (draw, slot) in draws.iter().zip(&self.slots) {
            let both = [1.0, 1.0, 0.0, 0.0];
            let holes = hole_vector(draw);
            let [filter, pattern] = Filter::shader(draw.outside);
            queue.write_buffer(
                &slot.buffer,
                0,
                &uniform(&draw.model, [draw.color, both, holes, filter, pattern]),
            );
        }
        let mut next = draws.len();
        let mut counts = Vec::new();
        for &(plane, clip) in &planes {
            for draw in &capped {
                // The section's parity counts the solids as they are, holes or not; its caps
                // open where the holes are.
                let slot = &self.slots[next];
                let uncut = [0.0, 0.0, -1.0, 0.0];
                let [filter, pattern] = Filter::shader(None);
                queue.write_buffer(
                    &slot.buffer,
                    0,
                    &uniform(&draw.model, [draw.color, clip, uncut, filter, pattern]),
                );
                counts.push((plane, draw.mesh, next));
                next += 1;
            }
        }
        // The supports' winding, each draw's own triangles (whole parts) cut by one plane.
        let mut windings = Vec::new();
        for &(plane, clip) in &planes {
            for draw in &supports {
                let slot = &self.slots[next];
                let [filter, pattern] = Filter::shader(None);
                queue.write_buffer(
                    &slot.buffer,
                    0,
                    &uniform(
                        &draw.model,
                        [draw.color, clip, [0.0, 0.0, -1.0, 0.0], filter, pattern],
                    ),
                );
                windings.push((plane, draw.mesh, draw.triangles, next));
                next += 1;
            }
        }
        let targets = self.targets.as_ref()?;
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("viewport"),
        });
        let outline = outline.filter(|o| !o.draws.is_empty() && o.width > 0.0);
        if let Some(outline) = outline {
            let [r, g, b, a] = outline.color;
            queue.write_buffer(
                &self.outline_buffer,
                0,
                &f32_bytes([r, g, b, a, outline.width, 0.0, 0.0, 0.0]),
            );
            self.outline_mask(&mut encoder, targets, draws, outline);
        }
        {
            let (view, resolve_target) = match &targets.multisampled {
                Some(multisampled) => (multisampled, Some(&targets.resolved)),
                None => (&targets.resolved, None),
            };
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("viewport"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            // egui composites the image premultiplied; blending over this
                            // premultiplied clear keeps every pixel premultiplied.
                            r: clear[0] * clear[3],
                            g: clear[1] * clear[3],
                            b: clear[2] * clear[3],
                            a: clear[3],
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &targets.depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(0),
                        store: wgpu::StoreOp::Discard,
                    }),
                }),
                occlusion_query_set: None,
                timestamp_writes: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &self.frame_group, &[]);
            for (draw, slot) in draws
                .iter()
                .zip(&self.slots)
                .filter(|(d, _)| !d.overlay && d.bore.is_none() && d.look != Look::Ghost)
            {
                let Some(mesh) = self.meshes.get(&draw.mesh) else {
                    continue;
                };
                pass.set_pipeline(match draw.look {
                    Look::Inside => &self.inside_pipelines[usize::from(mirrors(&draw.model))],
                    _ => &self.mesh_pipeline,
                });
                pass.set_bind_group(1, &slot.group, &[]);
                mesh.draw_triangles(&mut pass, draw.triangles);
            }
            // The plate is drawn after the models, so a translucent plate shows them through
            // when seen from below. A thumbnail has none of the view's scene.
            let faded = self.plate_fade > 0.0;
            let scene = !self.scene_hidden;
            for (buffer, pipeline) in [
                (
                    self.plate.as_ref().filter(|_| faded),
                    &self.faded_surface_pipeline,
                ),
                (self.surface.as_ref(), &self.surface_pipeline),
                (
                    self.grid.as_ref().filter(|_| faded),
                    &self.faded_line_pipeline,
                ),
                (self.lines.as_ref(), &self.line_pipeline),
            ] {
                if let Some((buffer, count)) = buffer.filter(|_| scene) {
                    pass.set_pipeline(pipeline);
                    pass.set_vertex_buffer(0, buffer.slice(..));
                    pass.draw(0..*count, 0..1);
                }
            }
            if let Some(label) = self.label.as_ref().filter(|_| scene) {
                pass.set_pipeline(&self.label_pipeline);
                pass.set_bind_group(2, &label.group, &[]);
                pass.set_vertex_buffer(0, label.vertices.slice(..));
                pass.draw(0..label.count, 0..1);
            }
            // The supports' section first, so the models' caps win where a tip enters the model:
            // for each plane, the winding, then the cap where it is not zero, which clears it.
            if let Some((buffer, _)) = self.caps.as_ref().filter(|_| !windings.is_empty()) {
                for &(plane, _) in &planes {
                    pass.set_pipeline(&self.wind_pipeline);
                    for &(_, mesh, triangles, slot) in windings.iter().filter(|w| w.0 == plane) {
                        let Some(mesh) = self.meshes.get(&mesh) else {
                            continue;
                        };
                        pass.set_bind_group(1, &self.slots[slot].group, &[]);
                        mesh.draw_triangles(&mut pass, triangles);
                    }
                    let first = u32::try_from((2 + plane) * 6).unwrap_or(0);
                    pass.set_vertex_buffer(0, buffer.slice(..));
                    pass.set_pipeline(&self.support_cap_pipeline);
                    pass.set_stencil_reference(0);
                    pass.draw(first..first + 6, 0..1);
                }
            }
            // The section: each plane's opening parity, then its cap where the parity is odd.
            for &(plane, mesh, slot) in &counts {
                let Some(mesh) = self.meshes.get(&mesh) else {
                    continue;
                };
                pass.set_pipeline(&self.count_pipelines[plane]);
                pass.set_bind_group(1, &self.slots[slot].group, &[]);
                mesh.draw(&mut pass);
            }
            // One flat quad on each cut plane, drawn only where its opening's parity is odd.
            if let Some((buffer, _)) = &self.caps {
                pass.set_vertex_buffer(0, buffer.slice(..));
                for &(plane, _) in &planes {
                    let first = u32::try_from(plane * 6).unwrap_or(0);
                    pass.set_pipeline(&self.cap_pipelines[plane]);
                    pass.set_stencil_reference([LOWER_BIT, UPPER_BIT][plane]);
                    pass.draw(first..first + 6, 0..1);
                }
            }
            // The holes' bores: the parity of each cut solid's surfaces beyond the bore, then the
            // bores where it is odd, in material.
            if bores {
                pass.set_pipeline(&self.hole_count_pipeline);
                for (draw, slot) in draws
                    .iter()
                    .zip(&self.slots)
                    .filter(|(d, _)| is_capped(d, Cap::Model) && d.holes[1] > 0)
                {
                    let Some(mesh) = self.meshes.get(&draw.mesh) else {
                        continue;
                    };
                    pass.set_bind_group(1, &slot.group, &[]);
                    mesh.draw_triangles(&mut pass, draw.triangles);
                }
                pass.set_pipeline(&self.bore_pipeline);
                pass.set_stencil_reference(HOLE_BIT);
                for (draw, slot) in draws
                    .iter()
                    .zip(&self.slots)
                    .filter(|(d, _)| d.bore.is_some() && !d.overlay)
                {
                    let Some(mesh) = self.meshes.get(&draw.mesh) else {
                        continue;
                    };
                    pass.set_bind_group(1, &slot.group, &[]);
                    mesh.draw_triangles(&mut pass, draw.triangles);
                }
            }
            // Ghosts over the opaque scene, without hiding what lies behind them.
            pass.set_pipeline(&self.ghost_pipeline);
            for (draw, slot) in draws
                .iter()
                .zip(&self.slots)
                .filter(|(d, _)| d.look == Look::Ghost && !d.overlay)
            {
                let Some(mesh) = self.meshes.get(&draw.mesh) else {
                    continue;
                };
                pass.set_bind_group(1, &slot.group, &[]);
                mesh.draw_triangles(&mut pass, draw.triangles);
            }
            // Overlays last, where models, the plate, caps or lines hide them.
            pass.set_pipeline(&self.overlay_pipeline);
            for (draw, slot) in draws.iter().zip(&self.slots).filter(|(d, _)| d.overlay) {
                let Some(mesh) = self.meshes.get(&draw.mesh) else {
                    continue;
                };
                pass.set_bind_group(1, &slot.group, &[]);
                mesh.draw_triangles(&mut pass, draw.triangles);
            }
            // The selection outline over everything, within its scissor.
            if let Some(outline) = outline {
                let [x, y, w, h] = outline.scissor.map_or([0, 0, size[0], size[1]], |s| {
                    let x = s[0].min(size[0]);
                    let y = s[1].min(size[1]);
                    [x, y, s[2].min(size[0] - x), s[3].min(size[1] - y)]
                });
                if w > 0 && h > 0 {
                    pass.set_scissor_rect(x, y, w, h);
                    pass.set_pipeline(&self.outline_pipeline);
                    pass.set_bind_group(2, &targets.outline_group, &[]);
                    pass.draw(0..3, 0..1);
                }
            }
        }
        queue.submit([encoder.finish()]);
        self.texture
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meshes_over_the_buffer_limit_are_split_into_parts_that_keep_every_triangle() {
        // TD-094: a strip of 7 triangles over 9 vertices, in parts of at most 3 triangles.
        let triangles: Vec<[u32; 3]> = (0..7u32).map(|i| [i, i + 1, i + 2]).collect();
        let parts = split_parts(9, &triangles, 3);
        assert_eq!(parts.len(), 3);
        let mut rebuilt = Vec::new();
        for (used, indices) in &parts {
            assert!(indices.len() <= 9 && used.len() <= 9);
            // Each part's vertices are its own, each once.
            let mut distinct = used.clone();
            distinct.sort_unstable();
            distinct.dedup();
            assert_eq!(distinct.len(), used.len());
            rebuilt.extend(
                indices
                    .chunks(3)
                    .map(|t| [0, 1, 2].map(|k| used[t[k] as usize])),
            );
        }
        assert_eq!(rebuilt, triangles);
        // Within the limit, one part over every vertex in order.
        let whole = split_parts(9, &triangles, 7);
        assert_eq!(whole.len(), 1);
        assert_eq!(whole[0].0, (0..9).collect::<Vec<u32>>());
    }

    #[test]
    fn a_ghost_or_a_surface_seen_from_inside_is_not_capped_and_mirrors_are_found() {
        let identity = Mat4([
            [1.0, 0.0, 0.0, 5.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ]);
        let solid = Draw {
            mesh: 1,
            model: identity,
            color: [1.0; 4],
            overlay: false,
            cap: Cap::Model,
            holes: [0, 0],
            bore: None,
            outside: None,
            triangles: None,
            look: Look::Solid,
        };
        assert!(is_capped(&solid, Cap::Model));
        assert!(!is_capped(&solid, Cap::Support));
        for look in [Look::Ghost, Look::Inside] {
            assert!(!is_capped(&Draw { look, ..solid }, Cap::Model));
        }
        assert!(!is_capped(
            &Draw {
                overlay: true,
                ..solid
            },
            Cap::Model
        ));
        // Supports are capped on their own; an uncapped draw never is.
        let support = Draw {
            cap: Cap::Support,
            ..solid
        };
        assert!(is_capped(&support, Cap::Support) && !is_capped(&support, Cap::Model));
        let off = Draw {
            cap: Cap::Off,
            ..solid
        };
        assert!(!is_capped(&off, Cap::Off));
        // Turning keeps the winding; mirroring one axis turns it over.
        assert!(!mirrors(&identity));
        let mut mirrored = identity;
        mirrored.0[1][1] = -1.0;
        assert!(mirrors(&mirrored));
        let mut turned = identity;
        turned.0[0] = [0.0, -1.0, 0.0, 0.0];
        turned.0[1] = [1.0, 0.0, 0.0, 0.0];
        assert!(!mirrors(&turned));
    }

    #[test]
    fn the_outline_scissor_covers_the_box_on_screen_and_its_width() {
        // A view straight down the Z axis onto a 10 mm square, orthographic for clarity: x and
        // y map from -20..20 mm to the 200 x 100 pixel image.
        let scale = 1.0 / 20.0;
        let view = Mat4([
            [scale, 0.0, 0.0, 0.0],
            [0.0, scale, 0.0, 0.0],
            [0.0, 0.0, 0.01, 0.5],
            [0.0, 0.0, 0.0, 1.0],
        ]);
        let scissor =
            Outline::scissor_of(&view, [0.0, 0.0, 0.0], [10.0, 10.0, 5.0], [200, 100], 3.0);
        // x 0..10 mm is pixels 100..150; y 0..10 mm is rows 25..50 (rows grow downwards).
        assert_eq!(scissor, Some([97, 22, 56, 31]));
        // Clipped to the image: x -50..5 mm reaches pixel 125 (+3), y 50..-5 mm row 62.5 (+3).
        let wide =
            Outline::scissor_of(&view, [-50.0, -5.0, 0.0], [5.0, 50.0, 1.0], [200, 100], 3.0);
        assert_eq!(wide, Some([0, 0, 128, 66]));
        // A corner behind the eye: the whole image.
        let mut behind = view;
        behind.0[3] = [0.0, 0.0, 1.0, 0.0];
        assert_eq!(
            Outline::scissor_of(&behind, [0.0, 0.0, -1.0], [1.0, 1.0, 1.0], [200, 100], 3.0),
            None
        );
    }

    #[test]
    #[allow(clippy::float_cmp)] // Exact values, passed through.
    fn filters_reach_the_shader_as_colour_and_pattern() {
        assert_eq!(Filter::shader(None), [[0.0; 4], [0.0; 4]]);
        let red = [1.0, 0.0, 0.0, 0.5];
        let solid = Filter {
            color: red,
            pattern: Pattern::Solid,
        };
        assert_eq!(Filter::shader(Some(solid)), [red, [1.0, 1.0, 0.0, 0.0]]);
        let stripes = Filter {
            pattern: Pattern::Stripes,
            ..solid
        };
        assert_eq!(Filter::shader(Some(stripes))[1][0], 2.0);
        // Five vectors after the matrix fill a uniform block exactly.
        let block = uniform(&Mat4([[0.0; 4]; 4]), [[0.0; 4]; 5]);
        assert_eq!(block.len() as u64, UNIFORM_BYTES);
        // Eleven fill the frame's.
        let frame = uniform(&Mat4([[0.0; 4]; 4]), [[0.0; 4]; 11]);
        assert_eq!(frame.len() as u64, FRAME_BYTES);
        // The contact's height line: its height, the switch and its half width, then its
        // colour; none switches it off.
        let guide = Guide {
            height_mm: 12.5,
            colour: [0.1, 0.1, 0.1, 1.0],
        };
        assert_eq!(
            Guide::shader(Some(guide)),
            [[12.5, 1.0, Guide::HALF_WIDTH_PX, 0.0], [0.1, 0.1, 0.1, 1.0]]
        );
        assert_eq!(Guide::shader(None), [[0.0; 4]; 2]);
        // The default lighting is the shader's former fixed one: 35 percent edge-on, full
        // facing the light.
        let [edge, facing, ..] = Shading::default().shader();
        assert!((edge - 0.35).abs() < 1e-6 && (edge + facing - 1.0).abs() < 1e-6);
        let brighter = Shading {
            brightness: 1.5,
            ambient: 0.5,
        };
        assert_eq!(brighter.shader(), [0.75, 0.75, 0.0, 0.0]);
        // Overhangs: the critical angle's cosine and the switch, then the two colours; none
        // switches them off.
        let style = OverhangStyle {
            angle_deg: 45.0,
            limit: [1.0, 0.8, 0.2, 0.9],
            level: [0.9, 0.1, 0.1, 0.9],
        };
        let [params, limit, level] = OverhangStyle::shader(Some(style));
        assert!((params[0] - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6);
        assert_eq!(params[1..], [1.0, PLATE_CONTACT_MM, 0.0]);
        assert_eq!((limit, level), (style.limit, style.level));
        assert_eq!(OverhangStyle::shader(None)[0][1], 0.0);
    }

    #[test]
    fn the_shaders_are_valid() {
        // Shaders compile only when the viewport is created; check them here, without a GPU.
        use wgpu::naga;
        let module = naga::front::wgsl::parse_str(include_str!("viewport.wgsl"))
            .unwrap_or_else(|e| panic!("{}", e.emit_to_string(include_str!("viewport.wgsl"))));
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::default(),
        )
        .validate(&module)
        .unwrap();
        for entry in [
            "mesh_fs",
            "hole_count_fs",
            "bore_fs",
            "cap_fs",
            "support_cap_fs",
            "cap_vs",
            "mask_fs",
            "outline_vs",
            "outline_fs",
        ] {
            assert!(
                module.entry_points.iter().any(|e| e.name == entry),
                "{entry}"
            );
        }
    }

    #[test]
    fn the_supports_count_their_winding_apart_and_their_cap_clears_it() {
        // The counter is the low bits, where increments carry; the other bits stay apart.
        assert_eq!(SUPPORT_MASK & (LOWER_BIT | UPPER_BIT | HOLE_BIT), 0);
        assert_eq!(SUPPORT_MASK & (SUPPORT_MASK + 1), 0);
        let wind = stencil_state(Stencil::Wind(SUPPORT_MASK));
        assert_eq!(wind.front.pass_op, wgpu::StencilOperation::IncrementWrap);
        assert_eq!(wind.back.pass_op, wgpu::StencilOperation::DecrementWrap);
        assert_eq!(wind.write_mask, SUPPORT_MASK);
        assert!(Stencil::Wind(SUPPORT_MASK).counts());
        // The cap draws where the count is not zero, and leaves it zero where it draws or is
        // hidden, so the other plane counts from zero.
        let union = stencil_state(Stencil::Union(SUPPORT_MASK));
        assert_eq!(union.front.compare, wgpu::CompareFunction::NotEqual);
        assert_eq!(union.front.pass_op, wgpu::StencilOperation::Zero);
        assert_eq!(union.front.depth_fail_op, wgpu::StencilOperation::Zero);
        assert_eq!(union.read_mask, SUPPORT_MASK);
        assert_eq!(union.write_mask, SUPPORT_MASK);
        assert!(!Stencil::Union(SUPPORT_MASK).counts());
    }

    #[test]
    #[allow(clippy::float_cmp)] // Exact values, passed through.
    fn cap_quads_carry_their_plane_and_colour() {
        // The models' quads, lower plane (0) then upper (1), then the supports', covering the
        // build volume, each in its colour: their heights come from the frame uniforms, so
        // moving a plane uploads nothing.
        let [model, support] = [[0.1, 0.2, 0.3, 1.0], [0.0, 0.3, 0.9, 1.0]];
        let quads = cap_quads([model, support]);
        assert_eq!(quads.len(), 24);
        for (quad, vertices) in quads.chunks(6).enumerate() {
            let plane = if quad % 2 == 0 { 0.0 } else { 1.0 };
            let color = if quad < 2 { model } else { support };
            assert!(vertices
                .iter()
                .all(|v| v.position[2] == plane && v.color == color));
        }
        assert!(quads
            .iter()
            .all(|v| v.position[0].abs() == CAP_HALF_SIDE_MM
                && v.position[1].abs() == CAP_HALF_SIDE_MM));
    }
}
