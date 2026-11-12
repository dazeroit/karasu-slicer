//! The view cube and the home button (M6.4f), in the viewport's top-right corner.
//!
//! **The cube** is drawn turned as the camera sees the world, in an orthographic projection of
//! the camera's basis. Each face is split into 3 x 3 cells: the centre cell looks at the face,
//! the edge cells look along the edge between two faces, and the corner cells look from the
//! corner. That gives 26 views in all. The cell under the pointer is highlighted, and a click
//! turns the camera to that view. The camera keeps its target and distance, and only its
//! direction changes.
//!
//! **Faces.** World Z is up, and the plate's front is at minimum Y, so FRONT looks from -Y, RIGHT
//! from +X and TOP from +Z (with the front at the bottom of the screen). Each face's name is
//! printed on it, turned and foreshortened with it: upright on the side faces, read from the
//! front on the top and the bottom. Names fade out as their face turns edge-on.
//!
//! **Hover.** The cells that give the same view light together: one on a face, two across an
//! edge, three around a corner, with no line between them.
//!
//! **Home** returns to the view the window opens with: the default direction, fitted to the
//! plate.
//!
//! **Drawing.** The cube is a navigation control, like the gizmo handles, so it is drawn with
//! the interface's painter: at most 27 small polygons and three labels, rebuilt from the camera
//! each frame. The geometry is plain `f64`, tested headless.
use crate::{camera::Camera, i18n::tr, theme};
use eframe::egui;

/// Side of the cube's square on screen, in points.
pub const CUBE_POINTS: f32 = 96.0;
/// Size of the home button, in points.
pub const HOME_POINTS: f32 = 26.0;
/// Gap from the viewport's edges, in points.
const MARGIN: f32 = 12.0;
/// How far below the viewport's top the cube ends, in points: overlays in that corner start
/// lower.
pub const BOTTOM: f32 = MARGIN + CUBE_POINTS;

/// A face of the cube: its outward normal and two axes spanning it, so that `u × v = normal`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Face {
    /// The face's outward normal, a world axis.
    pub normal: [f64; 3],
    u: [f64; 3],
    v: [f64; 3],
}

/// The six faces.
pub const FACES: [Face; 6] = [
    Face {
        normal: [0.0, 0.0, 1.0],
        u: [1.0, 0.0, 0.0],
        v: [0.0, 1.0, 0.0],
    },
    Face {
        normal: [0.0, 0.0, -1.0],
        u: [0.0, 1.0, 0.0],
        v: [1.0, 0.0, 0.0],
    },
    Face {
        normal: [0.0, -1.0, 0.0],
        u: [1.0, 0.0, 0.0],
        v: [0.0, 0.0, 1.0],
    },
    Face {
        normal: [0.0, 1.0, 0.0],
        u: [0.0, 0.0, 1.0],
        v: [1.0, 0.0, 0.0],
    },
    Face {
        normal: [1.0, 0.0, 0.0],
        u: [0.0, 1.0, 0.0],
        v: [0.0, 0.0, 1.0],
    },
    Face {
        normal: [-1.0, 0.0, 0.0],
        u: [0.0, 0.0, 1.0],
        v: [0.0, 1.0, 0.0],
    },
];

/// One of a face's 3 x 3 cells: `i`, `j` in -1..=1 along the face's `u` and `v`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    /// The face, by index into [`FACES`].
    pub face: usize,
    /// The cell along the face's `u` axis: -1, 0 or 1.
    pub i: i8,
    /// The cell along the face's `v` axis: -1, 0 or 1.
    pub j: i8,
}

impl Cell {
    /// The direction the camera looks from for this cell: the face's normal, plus the
    /// neighbouring faces' normals for an edge or a corner cell.
    #[must_use]
    pub fn direction(self) -> [f64; 3] {
        let face = FACES[self.face];
        let (i, j) = (f64::from(self.i), f64::from(self.j));
        std::array::from_fn(|k| face.normal[k] + i * face.u[k] + j * face.v[k])
    }

    /// The view this cell gives, as whole steps along the world axes: the cells of one edge
    /// (two faces) or one corner (three faces) share it.
    #[must_use]
    pub fn view(self) -> [i8; 3] {
        #[allow(clippy::cast_possible_truncation)] // Whole numbers from -1 to 1.
        self.direction().map(|c| c.round() as i8)
    }

    /// The cell's corners on the unit cube (half side 1), in order around it.
    fn corners(self) -> [[f64; 3]; 4] {
        // Cell bounds along each axis: the outer cells are a third of the face, from the edge.
        let span = |n: i8| match n {
            -1 => (-1.0, -1.0 / 3.0),
            0 => (-1.0 / 3.0, 1.0 / 3.0),
            _ => (1.0 / 3.0, 1.0),
        };
        quad(FACES[self.face], span(self.i), span(self.j))
    }
}

/// The corners of the part of `face` between `u` and `v` (each from -1 to 1), in order around it.
fn quad(face: Face, u: (f64, f64), v: (f64, f64)) -> [[f64; 3]; 4] {
    let point =
        |a: f64, b: f64| std::array::from_fn(|k| face.normal[k] + a * face.u[k] + b * face.v[k]);
    [
        point(u.0, v.0),
        point(u.1, v.0),
        point(u.1, v.1),
        point(u.0, v.1),
    ]
}

/// The name shown on face `face` of [`FACES`].
fn label(face: usize) -> String {
    match face {
        0 => tr!("viewcube-top"),
        1 => tr!("viewcube-bottom"),
        2 => tr!("viewcube-front"),
        3 => tr!("viewcube-back"),
        4 => tr!("viewcube-right"),
        _ => tr!("viewcube-left"),
    }
}

/// The text's rightward and upward directions on face `face`. Up is world Z on the side faces,
/// towards the back on the top and towards the front on the bottom, so both read from the front.
/// Right is `up x normal`, so the text is not mirrored when seen from outside.
fn text_axes(face: usize) -> ([f64; 3], [f64; 3]) {
    let up = match face {
        0 => [0.0, 1.0, 0.0],
        1 => [0.0, -1.0, 0.0],
        _ => [0.0, 0.0, 1.0],
    };
    (cross(up, FACES[face].normal), up)
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// The cube as `camera` sees it, laid out in `rect`.
#[derive(Clone, Copy, Debug)]
pub struct Projection {
    right: [f64; 3],
    up: [f64; 3],
    back: [f64; 3],
    centre: egui::Pos2,
    /// Points per unit of the cube's half side.
    scale: f32,
}

impl Projection {
    /// The projection of `camera`'s view into `rect`.
    #[must_use]
    pub fn new(camera: &Camera, rect: egui::Rect) -> Self {
        let (right, up, back) = camera.basis();
        Self {
            right,
            up,
            back,
            centre: rect.center(),
            // The cube's diagonal (2 sqrt 3) fits the square.
            scale: rect.width().min(rect.height()) / (2.0 * 3f32.sqrt()),
        }
    }

    /// Where a point of the cube is on screen.
    #[must_use]
    pub fn screen(&self, p: [f64; 3]) -> egui::Pos2 {
        #[allow(clippy::cast_possible_truncation)] // Screen points.
        let (x, y) = (dot(p, self.right) as f32, dot(p, self.up) as f32);
        self.centre + egui::vec2(x, -y) * self.scale
    }

    /// The text `galley` printed on face `face`, as a mesh turned and foreshortened with it:
    /// upright on the side faces, and read from the front on the top and the bottom. It is laid
    /// out at its size on screen for a face seen square on, and shrunk to fit a narrow face.
    /// `atlas` is the font texture's size, in texels.
    pub fn label_on(&self, face: usize, galley: &egui::Galley, atlas: [usize; 2]) -> egui::Shape {
        let normal = FACES[face].normal;
        let (right, up) = text_axes(face);
        let size = galley.size();
        // Cube units per point of text: as on screen, but at most 0.8 of the face's width.
        let unit = (1.0 / self.scale).min(1.6 / size.x.max(1.0));
        let centre = size / 2.0;
        #[allow(clippy::cast_precision_loss)] // Texture sizes.
        let texel = egui::vec2(1.0 / atlas[0] as f32, 1.0 / atlas[1] as f32);
        let mut mesh = egui::Mesh::default();
        for row in &galley.rows {
            let glyphs = &row.row.visuals.mesh;
            #[allow(clippy::cast_possible_truncation)] // Small meshes.
            let first = mesh.vertices.len() as u32;
            mesh.indices
                .extend(glyphs.indices.iter().map(|index| index + first));
            mesh.vertices.extend(glyphs.vertices.iter().map(|vertex| {
                let at = row.pos + vertex.pos.to_vec2() - centre;
                let (x, y) = (f64::from(at.x * unit), f64::from(at.y * unit));
                egui::epaint::Vertex {
                    pos: self.screen(std::array::from_fn(|k| {
                        normal[k] + x * right[k] - y * up[k]
                    })),
                    uv: (vertex.uv.to_vec2() * texel).to_pos2(),
                    color: vertex.color,
                }
            }));
        }
        egui::Shape::mesh(mesh)
    }

    /// The faces turned towards the eye, the farthest first.
    #[must_use]
    pub fn visible(&self) -> Vec<usize> {
        let mut faces: Vec<usize> = (0..FACES.len())
            .filter(|&f| dot(FACES[f].normal, self.back) > 1e-6)
            .collect();
        faces.sort_by(|&a, &b| {
            dot(FACES[a].normal, self.back).total_cmp(&dot(FACES[b].normal, self.back))
        });
        faces
    }

    /// The cell under `pointer`, on a face turned towards the eye.
    #[must_use]
    pub fn cell_at(&self, pointer: egui::Pos2) -> Option<Cell> {
        // Visible faces of a convex body do not overlap: the first that holds it is the one.
        self.visible().into_iter().rev().find_map(|face| {
            cells(face).find(|cell| inside(&cell.corners().map(|c| self.screen(c)), pointer))
        })
    }
}

/// Fills the convex quad `points` with `colour`, as two triangles without edge smoothing.
fn fill(painter: &egui::Painter, points: [egui::Pos2; 4], colour: egui::Color32) {
    let mut mesh = egui::Mesh::default();
    for point in points {
        mesh.colored_vertex(point, colour);
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(egui::Shape::mesh(mesh));
}

/// Draws the four sides of the quad `points` as separate segments, which have no corner joins.
fn edges(painter: &egui::Painter, points: [egui::Pos2; 4], stroke: egui::Stroke) {
    for k in 0..4 {
        painter.line_segment([points[k], points[(k + 1) % 4]], stroke);
    }
}

/// The nine cells of face `face`.
fn cells(face: usize) -> impl Iterator<Item = Cell> {
    (-1i8..=1).flat_map(move |i| (-1i8..=1).map(move |j| Cell { face, i, j }))
}

/// Whether `p` lies in the convex polygon `polygon` (either winding).
fn inside(polygon: &[egui::Pos2; 4], p: egui::Pos2) -> bool {
    let mut side: Option<bool> = None;
    for k in 0..4 {
        let (a, b) = (polygon[k], polygon[(k + 1) % 4]);
        let cross = (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x);
        if cross.abs() < 1e-6 {
            continue;
        }
        if *side.get_or_insert(cross > 0.0) != (cross > 0.0) {
            return false;
        }
    }
    true
}

/// Where the cube and the home button go in a viewport `rect`.
#[must_use]
pub fn layout(rect: egui::Rect) -> (egui::Rect, egui::Rect) {
    let cube = egui::Rect::from_min_size(
        egui::pos2(rect.max.x - MARGIN - CUBE_POINTS, rect.min.y + MARGIN),
        egui::vec2(CUBE_POINTS, CUBE_POINTS),
    );
    let home = egui::Rect::from_min_size(
        egui::pos2(cube.min.x - HOME_POINTS - 4.0, cube.min.y),
        egui::vec2(HOME_POINTS, HOME_POINTS),
    );
    (cube, home)
}

/// Whether the screen point `p` falls on the cube or the home button over the viewport `rect`,
/// so the viewport leaves that click to them.
pub fn covers(rect: egui::Rect, p: egui::Pos2) -> bool {
    let (cube, home) = layout(rect);
    cube.contains(p) || home.contains(p)
}

/// What the user asked for this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Request {
    /// Look from this direction.
    LookFrom([f64; 3]),
    /// Return to the initial view.
    Home,
}

/// Draw the cube and the home button over the viewport `rect`, and take their clicks.
pub fn show(ui: &mut egui::Ui, rect: egui::Rect, camera: &Camera) -> Option<Request> {
    let (cube_rect, home_rect) = layout(rect);
    let projection = Projection::new(camera, cube_rect);
    let cube = ui
        .interact(cube_rect, ui.id().with("view-cube"), egui::Sense::click())
        .on_hover_text(tr!("viewcube-hint"));
    let hovered = cube.hover_pos().and_then(|p| projection.cell_at(p));
    let painter = ui.painter_at(rect);
    let faces = projection.visible();
    // Each face is one quad: cells drawn apart would show seams, and a face seen edge-on would
    // squeeze its grid into flickering lines. Quads are filled as plain triangles and edged with
    // separate segments: egui's polygon outline and edge smoothing extend each corner along
    // its mitre without a limit, so a face seen edge-on, whose corners are very sharp, would
    // throw long spikes across the viewport.
    let border = egui::Stroke::new(1.0, theme::BORDER.color());
    for &face in &faces {
        let facing = dot(FACES[face].normal, projection.back);
        #[allow(clippy::cast_possible_truncation)] // A shade.
        let shade = (0.55 + 0.45 * facing) as f32;
        let outline = quad(FACES[face], (-1.0, 1.0), (-1.0, 1.0)).map(|c| projection.screen(c));
        fill(
            &painter,
            outline,
            theme::SURFACE.color().gamma_multiply(shade).to_opaque(),
        );
        edges(&painter, outline, border);
    }
    // The hovered view lights every cell that gives it, across the cube's edges, as one
    // region: drawn after all the faces, and edged in its own colour over the cube's edges, so
    // no line divides it.
    if let Some(view) = hovered.map(Cell::view) {
        let light = theme::INTERACTION.color();
        for &face in &faces {
            for cell in cells(face).filter(|cell| cell.view() == view) {
                let points = cell.corners().map(|c| projection.screen(c));
                fill(&painter, points, light);
                edges(&painter, points, egui::Stroke::new(1.0, light));
            }
        }
    }
    #[allow(clippy::redundant_closure_for_method_calls)] // The method alone is not general enough.
    let atlas = ui.ctx().fonts(|fonts| fonts.font_image_size());
    for &face in &faces {
        let facing = dot(FACES[face].normal, projection.back);
        // Labels fade out as their face turns away, before it is too thin to read.
        #[allow(clippy::cast_possible_truncation)] // An opacity.
        let opacity = ((facing - 0.15) / 0.3).clamp(0.0, 1.0) as f32;
        if opacity > 0.0 {
            #[allow(clippy::cast_possible_truncation)] // A shade.
            let shade = (0.55 + 0.45 * facing) as f32;
            let colour = theme::TEXT_PRIMARY.color().gamma_multiply(shade * opacity);
            let galley =
                painter.layout_no_wrap(label(face), egui::FontId::proportional(11.0), colour);
            painter.add(projection.label_on(face, &galley, atlas));
        }
    }
    let home = ui
        .interact(home_rect, ui.id().with("view-home"), egui::Sense::click())
        .on_hover_text(tr!("viewcube-home"));
    let fill = if home.hovered() {
        theme::INTERACTION.color()
    } else {
        theme::SURFACE.color()
    };
    painter.rect_filled(home_rect, 4.0, fill);
    paint_house(&painter, home_rect.shrink(6.0), theme::TEXT_PRIMARY.color());
    if home.clicked() {
        return Some(Request::Home);
    }
    if cube.clicked() {
        return hovered.map(|cell| Request::LookFrom(cell.direction()));
    }
    None
}

/// A house: a roof and a body with a door.
fn paint_house(painter: &egui::Painter, rect: egui::Rect, colour: egui::Color32) {
    let stroke = egui::Stroke::new(1.5, colour);
    let (l, r, t, b) = (rect.left(), rect.right(), rect.top(), rect.bottom());
    let eave = t + rect.height() * 0.45;
    let mid = rect.center().x;
    painter.line_segment([egui::pos2(l, eave), egui::pos2(mid, t)], stroke);
    painter.line_segment([egui::pos2(mid, t), egui::pos2(r, eave)], stroke);
    let (wl, wr) = (l + rect.width() * 0.15, r - rect.width() * 0.15);
    painter.line_segment([egui::pos2(wl, eave), egui::pos2(wl, b)], stroke);
    painter.line_segment([egui::pos2(wr, eave), egui::pos2(wr, b)], stroke);
    painter.line_segment([egui::pos2(wl, b), egui::pos2(wr, b)], stroke);
    let door = rect.width() * 0.12;
    let door_top = b - rect.height() * 0.3;
    painter.line_segment(
        [egui::pos2(mid - door, b), egui::pos2(mid - door, door_top)],
        stroke,
    );
    painter.line_segment(
        [
            egui::pos2(mid - door, door_top),
            egui::pos2(mid + door, door_top),
        ],
        stroke,
    );
    painter.line_segment(
        [egui::pos2(mid + door, door_top), egui::pos2(mid + door, b)],
        stroke,
    );
}

/// A camera turning from one view to another over a short time, eased at both ends.
#[derive(Clone, Copy, Debug)]
pub struct Turn {
    from: Camera,
    to: Camera,
    started: std::time::Instant,
}

/// How long a turn takes.
pub const TURN_SECONDS: f64 = 0.3;

impl Turn {
    /// A turn from `from` to `to`, the yaw taking the shorter way round.
    #[must_use]
    pub fn new(from: Camera, mut to: Camera) -> Self {
        let tau = std::f64::consts::TAU;
        let delta = (to.yaw - from.yaw).rem_euclid(tau);
        to.yaw = from.yaw
            + if delta > tau / 2.0 {
                delta - tau
            } else {
                delta
            };
        Self {
            from,
            to,
            started: std::time::Instant::now(),
        }
    }

    /// The camera `seconds` into the turn, and whether the turn is over.
    #[must_use]
    pub fn at(&self, seconds: f64) -> (Camera, bool) {
        let t = (seconds / TURN_SECONDS).clamp(0.0, 1.0);
        let eased = t * t * (3.0 - 2.0 * t);
        let mix = |a: f64, b: f64| a + (b - a) * eased;
        let (a, b) = (self.from, self.to);
        let camera = Camera {
            target: std::array::from_fn(|k| mix(a.target[k], b.target[k])),
            // Distances mix geometrically, so a large change feels even.
            distance: a.distance * (b.distance / a.distance).powf(eased),
            yaw: mix(a.yaw, b.yaw),
            pitch: mix(a.pitch, b.pitch),
            fov_y: b.fov_y,
        };
        (camera, t >= 1.0)
    }

    /// The camera now, and whether the turn is over.
    #[must_use]
    pub fn now(&self) -> (Camera, bool) {
        self.at(self.started.elapsed().as_secs_f64())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square() -> egui::Rect {
        egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(100.0, 100.0))
    }

    #[test]
    fn faces_are_right_handed_and_cells_cover_26_views() {
        for face in FACES {
            let n = face.normal;
            let cross = [
                face.u[1] * face.v[2] - face.u[2] * face.v[1],
                face.u[2] * face.v[0] - face.u[0] * face.v[2],
                face.u[0] * face.v[1] - face.u[1] * face.v[0],
            ];
            assert_eq!(cross, n, "{face:?}");
        }
        let mut views: Vec<[i8; 3]> = (0..6).flat_map(cells).map(Cell::view).collect();
        views.sort_unstable();
        // A face's view is one cell, an edge's two (one per face), a corner's three: the cells
        // lit together on hover.
        for view in [[0, -1, 0], [1, -1, 0], [1, -1, 1], [-1, 1, -1]] {
            let touched = view.iter().filter(|&&c| c != 0).count();
            assert_eq!(views.iter().filter(|&&v| v == view).count(), touched);
        }
        views.dedup();
        assert_eq!(views.len(), 26);
    }

    #[test]
    fn labels_read_left_to_right_and_upright_on_every_face() {
        // Seen square on, each face's text runs to the screen's right and up the screen. The top
        // is seen from above with the front at the bottom, the bottom from below with the front
        // at the top.
        for (face, from) in FACES.iter().enumerate() {
            let mut camera = Camera::new([0.0; 3], 100.0);
            camera.look_from(from.normal);
            let projection = Projection::new(&camera, square());
            let (right, up) = text_axes(face);
            let centre = projection.screen(from.normal);
            let along =
                |d: [f64; 3]| projection.screen(std::array::from_fn(|k| from.normal[k] + d[k]));
            assert!(along(right).x - centre.x > 20.0, "face {face}");
            assert!(centre.y - along(up).y > 20.0, "face {face}");
        }
    }

    #[test]
    fn faces_seen_edge_on_draw_nothing_outside_the_cube() {
        // egui joins polygon corners without a mitre limit, so a face seen edge-on once threw
        // spikes across the viewport. Every view near the edge-on cases is tessellated, with the
        // pointer on the cube so a view is lit too, and every vertex must stay by the cube.
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        let (cube, home) = layout(screen);
        let near = cube.union(home).expand(2.0);
        let mut camera = Camera::new([0.0; 3], 100.0);
        for (yaw, pitch) in [
            (0.0, 0.0),
            (-90.0, 0.001),
            (-89.999, 30.0),
            (-45.0, 0.0),
            (10.0, 89.0),
        ] {
            camera.yaw = f64::to_radians(yaw);
            camera.pitch = f64::to_radians(pitch);
            for pointer in [cube.center(), cube.center() + egui::vec2(20.0, -20.0)] {
                let input = egui::RawInput {
                    screen_rect: Some(screen),
                    events: vec![egui::Event::PointerMoved(pointer)],
                    ..Default::default()
                };
                let mut output = ctx.run_ui(input, |ui| {
                    show(ui, screen, &camera);
                });
                output.textures_delta.clear();
                let meshes = ctx.tessellate(output.shapes, output.pixels_per_point);
                for clipped in meshes {
                    if let egui::epaint::Primitive::Mesh(mesh) = clipped.primitive {
                        for vertex in mesh.vertices {
                            assert!(
                                near.contains(vertex.pos),
                                "yaw {yaw} pitch {pitch}: {vertex:?}"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn the_cell_under_the_pointer_is_the_one_shown_there() {
        // Looking at the front face, its centre cell is at the cube's centre on screen, and its
        // top-right cell looks from the front, the top and the right.
        let mut camera = Camera::new([0.0; 3], 100.0);
        camera.look_from([0.0, -1.0, 0.0]);
        let projection = Projection::new(&camera, square());
        assert_eq!(projection.visible(), [2]);
        let centre = projection.cell_at(egui::pos2(50.0, 50.0)).unwrap();
        assert_eq!(centre.direction(), [0.0, -1.0, 0.0]);
        let corner = projection.cell_at(egui::pos2(70.0, 30.0)).unwrap();
        assert_eq!(corner.direction(), [1.0, -1.0, 1.0]);
        assert_eq!(projection.cell_at(egui::pos2(2.0, 2.0)), None);
        // From the front-right above, three faces show, the front among them.
        camera.look_from([1.0, -1.0, 1.0]);
        let projection = Projection::new(&camera, square());
        let mut shown = projection.visible();
        shown.sort_unstable();
        assert_eq!(shown, [0, 2, 4]);
        let at_corner = projection.cell_at(egui::pos2(50.0, 50.0)).unwrap();
        assert_eq!(at_corner.direction(), [1.0, -1.0, 1.0]);
    }

    #[test]
    fn a_turn_eases_to_its_view_the_short_way_round() {
        let mut from = Camera::new([0.0; 3], 100.0);
        from.yaw = 3.0;
        let mut to = Camera::new([10.0, 0.0, 0.0], 400.0);
        to.yaw = -3.0;
        let turn = Turn::new(from, to);
        let (start, done) = turn.at(0.0);
        assert!(!done && (start.yaw - 3.0).abs() < 1e-12);
        // Halfway, the yaw has gone past pi rather than back through zero.
        let (half, _) = turn.at(TURN_SECONDS / 2.0);
        assert!(half.yaw > 3.0, "{}", half.yaw);
        assert!((half.distance - 200.0).abs() < 1e-9);
        let (end, done) = turn.at(TURN_SECONDS * 2.0);
        assert!(done);
        assert!(((end.yaw - (std::f64::consts::TAU - 3.0)).abs()) < 1e-12);
        assert_eq!(end.target, [10.0, 0.0, 0.0]);
    }
}
