// The viewport's shaders (M6.2): flat-shaded meshes and coloured lines.
//
// Positions are millimetres. Meshes keep their source vertices, relative to an origin near the
// mesh, so f32 keeps sub-micrometre precision; each draw's `model` matrix places them.

struct Frame {
    view_projection: mat4x4<f32>,
    // Direction towards the light, in world space (w unused).
    light: vec4<f32>,
    // The section (M6.4b3): x is the lowest height kept, y the highest (huge values when there
    // is no section). z is the plate's opacity (TD-088): 1 seen from above, fading to 0 below.
    section: vec4<f32>,
    // The eye's position (xyz) and the number of holes (w).
    eye: vec4<f32>,
    // The build volume for surface filters (M6.4f), widened by a tolerance: its low corner and
    // the stripes' period in pixels (w), its high corner and whether there is a volume (w).
    volume_low: vec4<f32>,
    volume_high: vec4<f32>,
    // Overhangs on the models' surfaces (TD-107): x the cosine of the critical angle, y whether
    // they are shown, z the height under which a surface rests on the plate (mm); then the
    // colours (linear RGB and opacity) at the angle's limit and on a level surface.
    overhang: vec4<f32>,
    overhang_limit: vec4<f32>,
    overhang_level: vec4<f32>,
    // The lighting (a user setting): x the light a face gets edge-on (ambient), y what a face
    // turned to the light adds, both scaled by the brightness.
    shading: vec4<f32>,
    // The contact's height line (TD-137): x the height (mm), y whether it shows, z its half
    // width in pixels; then its colour (linear RGB and opacity), drawn unlit over the models.
    guide: vec4<f32>,
    guide_colour: vec4<f32>,
};

struct Object {
    model: mat4x4<f32>,
    // Linear RGB and alpha.
    color: vec4<f32>,
    // Which section planes cut this draw: x the lower one, y the upper one (1 cuts, 0 not).
    // Capping counts each plane's opening alone.
    clip: vec4<f32>,
    // The holes that cut this draw: x the first, y how many; z the hole whose bore this draw is
    // (-1 for none).
    holes: vec4<f32>,
    // The filter outside the build volume (M6.4f): linear RGB and opacity, then its pattern in
    // x (0 none, 1 solid, 2 stripes) and in y whether this draw is a model's surface, which
    // shows its overhangs.
    outside_color: vec4<f32>,
    outside_pattern: vec4<f32>,
};

// A drain hole's cylinder (TD-089): the centre of its start and its radius, its unit axis and
// its length, in world millimetres.
struct Hole {
    entry: vec4<f32>,
    axis: vec4<f32>,
};

@group(0) @binding(0) var<uniform> frame: Frame;
// The holes, two texels each, 256 to a row (a texture: the OpenGL fallback has no storage
// buffers in fragment shaders).
@group(0) @binding(1) var holes: texture_2d<f32>;

// Hole `i`.
fn hole_at(i: u32) -> Hole {
    let at = vec2<i32>(i32((i % 256u) * 2u), i32(i / 256u));
    return Hole(textureLoad(holes, at, 0), textureLoad(holes, at + vec2<i32>(1, 0), 0));
}

// Whether `p` is inside hole `i`.
fn in_hole(p: vec3<f32>, i: u32) -> bool {
    let hole = hole_at(i);
    let v = p - hole.entry.xyz;
    let t = dot(v, hole.axis.xyz);
    return t >= 0.0 && t <= hole.axis.w && dot(v, v) - t * t <= hole.entry.w * hole.entry.w;
}

// Whether `p` is inside any of `count` holes from `first`.
fn in_holes(p: vec3<f32>, first: u32, count: u32) -> bool {
    for (var i = first; i < first + count; i++) {
        if (in_hole(p, i)) {
            return true;
        }
    }
    return false;
}

// Where the ray from `origin` along the unit `u` is inside hole `i`, as distances along it
// (entering, leaving); entering after leaving when it misses.
fn span(origin: vec3<f32>, u: vec3<f32>, i: u32) -> vec2<f32> {
    let hole = hole_at(i);
    let d = hole.axis.xyz;
    let w = origin - hole.entry.xyz;
    let ud = dot(u, d);
    let wd = dot(w, d);
    let up = u - ud * d;
    let wp = w - wd * d;
    let a = dot(up, up);
    let b = dot(up, wp);
    let c = dot(wp, wp) - hole.entry.w * hole.entry.w;
    let miss = vec2<f32>(1.0, 0.0);
    var near = -1.0e30;
    var far = 1.0e30;
    // Across the side wall.
    if (a < 1.0e-12) {
        if (c > 0.0) {
            return miss;
        }
    } else {
        let disc = b * b - a * c;
        if (disc < 0.0) {
            return miss;
        }
        let q = sqrt(disc);
        near = (-b - q) / a;
        far = (-b + q) / a;
    }
    // Between the ends.
    if (abs(ud) < 1.0e-12) {
        if (wd < 0.0 || wd > hole.axis.w) {
            return miss;
        }
    } else {
        let s0 = -wd / ud;
        let s1 = (hole.axis.w - wd) / ud;
        near = max(near, min(s0, s1));
        far = min(far, max(s0, s1));
    }
    return vec2<f32>(near, far);
}

// The hole of the draw's range the line of sight through `p` enters first, or -1.
fn first_hole(p: vec3<f32>) -> i32 {
    let u = normalize(p - frame.eye.xyz);
    let first = u32(object.holes.x);
    var best = -1;
    var nearest = 1.0e30;
    for (var i = first; i < first + u32(object.holes.y); i++) {
        let s = span(frame.eye.xyz, u, i);
        if (s.x <= s.y && s.y > 0.0 && s.x < nearest) {
            nearest = s.x;
            best = i32(i);
        }
    }
    return best;
}

// The render target holds sRGB-encoded bytes, as egui expects of the images it shows; lighting
// is computed in linear light and encoded here.
fn srgb(linear: vec3<f32>) -> vec3<f32> {
    let low = linear * 12.92;
    let high = 1.055 * pow(linear, vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(high, low, linear <= vec3<f32>(0.0031308));
}
@group(1) @binding(0) var<uniform> object: Object;

// Whether the section cuts `p` away from this draw.
fn sectioned(p: vec3<f32>) -> bool {
    return (object.clip.x > 0.5 && p.z < frame.section.x)
        || (object.clip.y > 0.5 && p.z > frame.section.y);
}

// Flat shading: the face normal from screen-space derivatives, without per-face vertices.
// Lighting is two-sided, so the winding and mirrored placements do not matter. The light
// follows the view, so at the default brightness a face turned towards the viewer shows
// exactly the object's colour and faces seen edge-on darken to the ambient light (35 percent
// by default).
fn shade(world: vec3<f32>) -> vec4<f32> {
    return shade_as(world, object.color.rgb);
}

// Flat shading of the linear colour `base`.
fn shade_as(world: vec3<f32>, base: vec3<f32>) -> vec4<f32> {
    return shade_with(face_normal(world), base);
}

// The face's normal at `world`, from screen-space derivatives, turned towards the eye: the side
// seen, which is the outside of a closed model's surface.
fn face_normal(world: vec3<f32>) -> vec3<f32> {
    let normal = normalize(cross(dpdx(world), dpdy(world)));
    return select(-normal, normal, dot(normal, frame.eye.xyz - world) >= 0.0);
}

// Flat shading of the linear colour `base` on a face of `normal`.
fn shade_with(normal: vec3<f32>, base: vec3<f32>) -> vec4<f32> {
    let lit = frame.shading.x + frame.shading.y * abs(dot(normal, normalize(frame.light.xyz)));
    return vec4<f32>(srgb(base * lit), object.color.a);
}

// A model's colour with its overhangs (TD-107): a surface facing the plate whose slope above
// level is under the critical angle, unless it rests on the plate, takes a colour from the
// limit's (at the angle) to the level one's (a ceiling), mixed by that colour's opacity.
fn with_overhang(base: vec3<f32>, world: vec3<f32>, normal: vec3<f32>) -> vec3<f32> {
    let limit = frame.overhang.x;
    let down = -normal.z;
    if (object.outside_pattern.y < 0.5 || frame.overhang.y < 0.5 || down <= limit
        || world.z <= frame.overhang.z) {
        return base;
    }
    let t = clamp((down - limit) / max(1.0 - limit, 1.0e-6), 0.0, 1.0);
    let colour = mix(frame.overhang_limit, frame.overhang_level, t);
    return mix(base, colour.rgb, colour.a);
}

// How much a filter with `pattern` (1 solid, 2 stripes) covers the pixel at `at`: stripes run
// diagonally on the screen, half of each period, with edges smoothed over a pixel.
fn coverage(pattern: f32, at: vec2<f32>) -> f32 {
    if (pattern < 1.5) {
        return 1.0;
    }
    let period = frame.volume_low.w;
    let f = fract((at.x + at.y) / period);
    // Distance to the nearest stripe edge in periods, positive inside a stripe; a period
    // spans period / sqrt(2) pixels across the stripes.
    let edge = select(-min(f - 0.5, 1.0 - f), min(f, 0.5 - f), f < 0.5);
    return clamp(edge * period * 0.70710678 + 0.5, 0.0, 1.0);
}

// The draw's colour at `world` on a face of `normal`: its overhangs, then its filter mixed in
// where the surface lies outside the build volume.
fn filtered(world: vec3<f32>, at: vec2<f32>, normal: vec3<f32>) -> vec3<f32> {
    let base = with_overhang(object.color.rgb, world, normal);
    let pattern = object.outside_pattern.x;
    let outside = any(world < frame.volume_low.xyz) || any(world > frame.volume_high.xyz);
    if (pattern < 0.5 || frame.volume_high.w < 0.5 || !outside) {
        return base;
    }
    let amount = object.outside_color.a * coverage(pattern, at);
    return mix(base, object.outside_color.rgb, amount);
}

struct MeshOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
};

@vertex
fn mesh_vs(@location(0) position: vec3<f32>) -> MeshOut {
    let world = object.model * vec4<f32>(position, 1.0);
    var out: MeshOut;
    out.clip = frame.view_projection * world;
    out.world = world.xyz;
    return out;
}

// How much the contact's height line (TD-137) covers a model's fragment at `world`, whose
// height changes by `dz` a pixel: a band `frame.guide.z` pixels each side of the height,
// smoothed over a pixel, so it keeps its width on screen however the surface is seen.
fn guide_coverage(world: vec3<f32>, dz: f32) -> f32 {
    if (object.outside_pattern.y < 0.5 || frame.guide.y < 0.5) {
        return 0.0;
    }
    let pixels = abs(world.z - frame.guide.x) / max(dz, 1.0e-6);
    return clamp(frame.guide.z + 0.5 - pixels, 0.0, 1.0) * frame.guide_colour.a;
}

@fragment
fn mesh_fs(input: MeshOut) -> @location(0) vec4<f32> {
    // The derivatives first, in uniform control flow.
    let normal = face_normal(input.world);
    let dz = fwidth(input.world.z);
    // Cut by the section, or opened by a hole.
    if (sectioned(input.world)
        || in_holes(input.world, u32(object.holes.x), u32(object.holes.y))) {
        discard;
    }
    let shaded = shade_with(normal, filtered(input.world, input.clip.xy, normal));
    let line = guide_coverage(input.world, dz);
    return vec4<f32>(mix(shaded.rgb, srgb(frame.guide_colour.rgb), line), shaded.a);
}

// The parity of a cut solid's surfaces beyond the bore of the hole its line of sight enters
// first: odd where the bore lies in material. Only the stencil is written.
@fragment
fn hole_count_fs(input: MeshOut) -> @location(0) vec4<f32> {
    let hole = first_hole(input.world);
    if (hole < 0) {
        discard;
    }
    let u = normalize(input.world - frame.eye.xyz);
    let bore = span(frame.eye.xyz, u, u32(hole)).y;
    if (distance(input.world, frame.eye.xyz) <= bore) {
        discard;
    }
    return vec4<f32>(0.0);
}

// A hole's bore: the far side of its cylinder, where the line of sight enters this hole first
// (the stencil keeps it to the material).
@fragment
fn bore_fs(input: MeshOut) -> @location(0) vec4<f32> {
    let own = i32(object.holes.z);
    if (sectioned(input.world) || first_hole(input.world) != own) {
        discard;
    }
    let u = normalize(input.world - frame.eye.xyz);
    let s = span(frame.eye.xyz, u, u32(own));
    if (distance(input.world, frame.eye.xyz) < 0.5 * (s.x + s.y)) {
        discard;
    }
    return shade(input.world);
}

struct LineOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn line_vs(@location(0) position: vec3<f32>, @location(1) color: vec4<f32>) -> LineOut {
    var out: LineOut;
    out.clip = frame.view_projection * vec4<f32>(position, 1.0);
    out.color = color;
    return out;
}

struct CapOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) world: vec3<f32>,
};

// The section's caps: one quad per plane, uploaded once, with the plane's index (0 lower,
// 1 upper) in z. Each frame's heights come from the frame uniforms, so moving a plane uploads
// nothing.
@vertex
fn cap_vs(@location(0) position: vec3<f32>, @location(1) color: vec4<f32>) -> CapOut {
    let z = select(frame.section.x, frame.section.y, position.z > 0.5);
    var out: CapOut;
    out.world = vec3<f32>(position.xy, z);
    out.clip = frame.view_projection * vec4<f32>(out.world, 1.0);
    out.color = color;
    return out;
}

// A cap is open where a hole crosses the plane.
@fragment
fn cap_fs(input: CapOut) -> @location(0) vec4<f32> {
    if (in_holes(input.world, 0u, u32(frame.eye.w))) {
        discard;
    }
    return vec4<f32>(srgb(input.color.rgb), input.color.a);
}

// The supports' cap: never opened by holes, which do not cut supports.
@fragment
fn support_cap_fs(input: CapOut) -> @location(0) vec4<f32> {
    return vec4<f32>(srgb(input.color.rgb), input.color.a);
}

@fragment
fn line_fs(input: LineOut) -> @location(0) vec4<f32> {
    return vec4<f32>(srgb(input.color.rgb), input.color.a);
}

// The plate's surface and grid, which fade out when the view goes below the plate.
@fragment
fn faded_fs(input: LineOut) -> @location(0) vec4<f32> {
    return vec4<f32>(srgb(input.color.rgb), input.color.a * frame.section.z);
}

// Text lying on the plate (the front tab's word): glyph quads in world coordinates, sampling
// the coverage of a snapshot of egui's font atlas. Group 2, so the object group stays free.
@group(2) @binding(0) var label_texture: texture_2d<f32>;
@group(2) @binding(1) var label_sampler: sampler;

struct LabelOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
};

@vertex
fn label_vs(
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
) -> LabelOut {
    var out: LabelOut;
    out.clip = frame.view_projection * vec4<f32>(position, 1.0);
    out.uv = uv;
    out.color = color;
    return out;
}

@fragment
fn label_fs(input: LabelOut) -> @location(0) vec4<f32> {
    let coverage = textureSample(label_texture, label_sampler, input.uv).a;
    return vec4<f32>(srgb(input.color.rgb), input.color.a * coverage);
}

// The selection outline (M6.4f). The mask covers the outlined draws whole: neither the section
// nor the holes cut it, so the outline keeps the object's full silhouette.
@fragment
fn mask_fs(input: MeshOut) -> @location(0) vec4<f32> {
    return vec4<f32>(1.0);
}

struct OutlineStyle {
    // Linear RGB and alpha.
    color: vec4<f32>,
    // The width beyond the silhouette, in pixels (x).
    width: vec4<f32>,
};

@group(2) @binding(0) var outline_mask: texture_2d<f32>;
@group(2) @binding(1) var<uniform> outline: OutlineStyle;

// One triangle covering the image, at the near plane.
@vertex
fn outline_vs(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let corner = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4<f32>(corner * 2.0 - 1.0, 0.0, 1.0);
}

// Pixels outside the mask within the width of it, faded over the last pixel by their distance
// to the nearest covered pixel, so the outer edge is smooth.
@fragment
fn outline_fs(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let p = vec2<i32>(at.xy);
    if (textureLoad(outline_mask, p, 0).r > 0.5) {
        discard;
    }
    let width = outline.width.x;
    let reach = i32(ceil(width));
    let last = vec2<i32>(textureDimensions(outline_mask)) - 1;
    var nearest = 1.0e9;
    for (var dy = -reach; dy <= reach; dy++) {
        for (var dx = -reach; dx <= reach; dx++) {
            let q = p + vec2<i32>(dx, dy);
            if (any(q < vec2<i32>(0)) || any(q > last)) {
                continue;
            }
            if (textureLoad(outline_mask, q, 0).r > 0.5) {
                nearest = min(nearest, length(vec2<f32>(f32(dx), f32(dy))));
            }
        }
    }
    let alpha = clamp(width + 0.5 - nearest, 0.0, 1.0);
    if (alpha <= 0.0) {
        discard;
    }
    return vec4<f32>(srgb(outline.color.rgb), outline.color.a * alpha);
}
