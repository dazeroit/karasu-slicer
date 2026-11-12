//! The viewport camera (M6.2): an orbit camera around a target point, in the world frame of the
//! core (millimetres, right-handed, Z up).
//!
//! **Projection:** perspective, with the wgpu depth range 0 (near) to 1 (far) and Y up in clip
//! space. Matrices are row-major `f64` here; [`Mat4::to_gpu`] gives the column-major `f32`
//! layout WGSL expects.
//!
//! **Controls:** orbiting changes the yaw (about Z) and the pitch (elevation, limited short of
//! the poles so the up direction stays defined); panning moves the target in the view plane.
//! Zooming scales the eye and the target about an anchor, the point under the pointer
//! (M6.4f): the anchor stays where it is on screen, and the target comes with it, so a detail
//! can be approached, orbited and panned at any size.
//!
//! **Depth of the target (owner, 2026-10-30):** orbiting turns about the target and panning
//! moves at the target's depth, so a target left far in front of or behind what is seen (after
//! panning across the model, or turning) makes both too fast or too slow. A drag first brings the
//! target along the line of sight to the depth of the surface it works on
//! ([`Camera::retarget`]): the view does not change, only what the next moves turn about.

/// A 4 x 4 matrix, row-major: `m[row][column]`, acting on column vectors.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mat4(pub [[f64; 4]; 4]);

impl Mat4 {
    /// `self * other`: `other` applies first.
    #[must_use]
    pub fn mul(&self, other: &Self) -> Self {
        let (a, b) = (&self.0, &other.0);
        Self(std::array::from_fn(|row| {
            std::array::from_fn(|column| (0..4).map(|k| a[row][k] * b[k][column]).sum())
        }))
    }

    /// The homogeneous image of a point, divided by w; `None` when w is not positive (behind
    /// the eye).
    #[must_use]
    pub fn project(&self, p: [f64; 3]) -> Option<[f64; 3]> {
        let m = &self.0;
        let row = |r: usize| m[r][0] * p[0] + m[r][1] * p[1] + m[r][2] * p[2] + m[r][3];
        let w = row(3);
        (w > 0.0).then(|| [row(0) / w, row(1) / w, row(2) / w])
    }

    /// Column-major `f32` values, as WGSL `mat4x4<f32>` reads them.
    #[must_use]
    pub fn to_gpu(self) -> [f32; 16] {
        #[allow(clippy::cast_possible_truncation)] // GPU precision.
        std::array::from_fn(|i| self.0[i % 4][i / 4] as f32)
    }
}

/// A ray from `origin` along the unit `direction`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ray {
    /// Start point.
    pub origin: [f64; 3],
    /// Unit direction.
    pub direction: [f64; 3],
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
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

fn normalized(a: [f64; 3]) -> [f64; 3] {
    let length = dot(a, a).sqrt();
    [a[0] / length, a[1] / length, a[2] / length]
}

/// Highest elevation, short of the pole where the view's up direction is undefined.
const MAX_PITCH: f64 = 89.0 * std::f64::consts::PI / 180.0;
/// Radians of orbit per pixel dragged.
const ORBIT_PER_PIXEL: f64 = 0.008;
/// Distance factor per zoom step (one wheel notch).
const ZOOM_STEP: f64 = 0.85;
/// The closest and farthest the eye comes to the target, in mm: close enough for a miniature's
/// details.
const MIN_DISTANCE: f64 = 0.05;
const MAX_DISTANCE: f64 = 100_000.0;

/// An orbit camera.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    /// The point the camera looks at and orbits around.
    pub target: [f64; 3],
    /// Distance from the eye to the target, in mm.
    pub distance: f64,
    /// Rotation about Z of the eye around the target, in radians (0: eye on +X).
    pub yaw: f64,
    /// Elevation of the eye above the target's horizontal plane, in radians.
    pub pitch: f64,
    /// Vertical field of view, in radians.
    pub fov_y: f64,
}

impl Camera {
    /// A camera looking at `target` from the front-right, above.
    #[must_use]
    pub fn new(target: [f64; 3], distance: f64) -> Self {
        Self {
            target,
            distance,
            yaw: -60f64.to_radians(),
            pitch: 30f64.to_radians(),
            fov_y: 40f64.to_radians(),
        }
    }

    /// The eye position.
    #[must_use]
    pub fn eye(&self) -> [f64; 3] {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        [
            self.target[0] + self.distance * cp * cy,
            self.target[1] + self.distance * cp * sy,
            self.target[2] + self.distance * sp,
        ]
    }

    /// The view's right, up and backward (towards the eye) unit vectors.
    pub(crate) fn basis(&self) -> ([f64; 3], [f64; 3], [f64; 3]) {
        let back = normalized(sub(self.eye(), self.target));
        let right = normalized(cross([0.0, 0.0, 1.0], back));
        let up = cross(back, right);
        (right, up, back)
    }

    /// Near and far clip distances, around the target at any zoom.
    fn clip(&self) -> (f64, f64) {
        let near = (self.distance * 1e-3).max(1e-3);
        (near, self.distance * 100.0 + 1000.0)
    }

    /// World to clip space for a viewport of `aspect` (width / height).
    #[must_use]
    pub fn view_projection(&self, aspect: f64) -> Mat4 {
        let (right, up, back) = self.basis();
        let eye = self.eye();
        let view = Mat4([
            [right[0], right[1], right[2], -dot(right, eye)],
            [up[0], up[1], up[2], -dot(up, eye)],
            [back[0], back[1], back[2], -dot(back, eye)],
            [0.0, 0.0, 0.0, 1.0],
        ]);
        let (near, far) = self.clip();
        let f = 1.0 / (self.fov_y / 2.0).tan();
        // Right-handed view space looks along -Z; depth maps near to 0 and far to 1.
        let projection = Mat4([
            [f / aspect, 0.0, 0.0, 0.0],
            [0.0, f, 0.0, 0.0],
            [0.0, 0.0, far / (near - far), near * far / (near - far)],
            [0.0, 0.0, -1.0, 0.0],
        ]);
        projection.mul(&view)
    }

    /// Where a world point appears in a `size` pixel viewport (origin top-left), or `None`
    /// behind the eye.
    #[must_use]
    pub fn screen(&self, point: [f64; 3], size: [f64; 2]) -> Option<[f64; 2]> {
        let [x, y, _] = self.view_projection(size[0] / size[1]).project(point)?;
        Some([
            f64::midpoint(x, 1.0) * size[0],
            f64::midpoint(1.0, -y) * size[1],
        ])
    }

    /// The ray from the eye through `pixel` of a `size` pixel viewport (origin top-left).
    #[must_use]
    pub fn ray(&self, pixel: [f64; 2], size: [f64; 2]) -> Ray {
        let (right, up, back) = self.basis();
        let half_height = (self.fov_y / 2.0).tan();
        let half_width = half_height * size[0] / size[1];
        let x = (pixel[0] / size[0] * 2.0 - 1.0) * half_width;
        let y = (1.0 - pixel[1] / size[1] * 2.0) * half_height;
        let direction = normalized([
            right[0] * x + up[0] * y - back[0],
            right[1] * x + up[1] * y - back[1],
            right[2] * x + up[2] * y - back[2],
        ]);
        Ray {
            origin: self.eye(),
            direction,
        }
    }

    /// Orbit by a pointer drag of `dx`, `dy` pixels.
    pub fn orbit(&mut self, dx: f64, dy: f64) {
        self.yaw -= dx * ORBIT_PER_PIXEL;
        self.pitch = (self.pitch + dy * ORBIT_PER_PIXEL).clamp(-MAX_PITCH, MAX_PITCH);
    }

    /// Move the target with a pointer drag of `dx`, `dy` pixels in a viewport `height` pixels
    /// tall, so the target plane follows the pointer.
    pub fn pan(&mut self, dx: f64, dy: f64, height: f64) {
        let (right, up, _) = self.basis();
        let per_pixel = 2.0 * self.distance * (self.fov_y / 2.0).tan() / height;
        for axis in 0..3 {
            self.target[axis] += (-dx * right[axis] + dy * up[axis]) * per_pixel;
        }
    }

    /// Bring the target along the line of sight to `depth` mm in front of the eye (clamped to
    /// the distances the camera keeps), the eye and the view unchanged.
    pub fn retarget(&mut self, depth: f64) {
        if !depth.is_finite() {
            return;
        }
        let eye = self.eye();
        let (_, _, back) = self.basis();
        let depth = depth.clamp(MIN_DISTANCE, MAX_DISTANCE);
        self.target = std::array::from_fn(|k| eye[k] - back[k] * depth);
        self.distance = depth;
    }

    /// How far in front of the eye `point` is, along the line of sight.
    #[must_use]
    pub fn depth_of(&self, point: [f64; 3]) -> f64 {
        let (_, _, back) = self.basis();
        -dot(sub(point, self.eye()), back)
    }

    /// Zoom in (positive `steps`) or out towards the target.
    pub fn zoom(&mut self, steps: f64) {
        self.zoom_at(steps, self.target);
    }

    /// Zoom in (positive `steps`) or out towards `anchor`: the eye and the target are scaled
    /// about it, so it stays where it is on screen and the target comes with it.
    pub fn zoom_at(&mut self, steps: f64, anchor: [f64; 3]) {
        let distance = (self.distance * ZOOM_STEP.powf(steps)).clamp(MIN_DISTANCE, MAX_DISTANCE);
        let scale = distance / self.distance;
        self.target = std::array::from_fn(|k| anchor[k] + (self.target[k] - anchor[k]) * scale);
        self.distance = distance;
    }

    /// Where `ray` meets the plane through the target facing the eye: what a zoom anchors on
    /// where the pointer is over no surface.
    #[must_use]
    pub fn focal_point(&self, ray: &Ray) -> [f64; 3] {
        let (_, _, back) = self.basis();
        let along = dot(ray.direction, back);
        let t = if along.abs() < 1e-12 {
            self.distance
        } else {
            dot(sub(self.target, ray.origin), back) / along
        };
        std::array::from_fn(|k| ray.origin[k] + ray.direction[k] * t.max(0.0))
    }

    /// Look at the target from `direction` (target to eye, any length), keeping the target and
    /// the distance. Straight up or down keeps the plate's front at the bottom of the screen.
    pub fn look_from(&mut self, direction: [f64; 3]) {
        let d = normalized(direction);
        let horizontal = d[0].hypot(d[1]);
        if horizontal > 1e-9 {
            self.yaw = d[1].atan2(d[0]);
        } else {
            self.yaw = -std::f64::consts::FRAC_PI_2;
        }
        self.pitch = d[2].atan2(horizontal).clamp(-MAX_PITCH, MAX_PITCH);
    }

    /// Look at the box `min`..`max` from the current direction, with all of it in view in a
    /// viewport of `aspect` (width / height).
    pub fn fit(&mut self, min: [f64; 3], max: [f64; 3], aspect: f64) {
        self.target = std::array::from_fn(|axis| f64::midpoint(min[axis], max[axis]));
        let radius = (dot(sub(max, min), sub(max, min)).sqrt() / 2.0).max(1.0);
        let half = (self.fov_y / 2.0).min((aspect * (self.fov_y / 2.0).tan()).atan());
        self.distance = radius / half.sin() * 1.05;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9 * (1.0 + a.abs().max(b.abs()))
    }

    #[test]
    fn the_target_is_at_the_centre_and_depth_is_in_range() {
        let camera = Camera::new([10.0, 20.0, 5.0], 200.0);
        let size = [800.0, 600.0];
        let centre = camera.screen(camera.target, size).unwrap();
        assert!(
            close(centre[0], 400.0) && close(centre[1], 300.0),
            "{centre:?}"
        );
        let [_, _, depth] = camera
            .view_projection(size[0] / size[1])
            .project(camera.target)
            .unwrap();
        assert!(depth > 0.0 && depth < 1.0, "{depth}");
        // A point behind the eye is not projected.
        let eye = camera.eye();
        let behind = std::array::from_fn(|k| eye[k] + (eye[k] - camera.target[k]));
        assert_eq!(camera.screen(behind, size), None);
        // Up in the world is up on the screen.
        let above = camera.screen([10.0, 20.0, 15.0], size).unwrap();
        assert!(above[1] < centre[1]);
    }

    #[test]
    fn rays_pass_through_their_pixels() {
        let camera = Camera::new([0.0; 3], 150.0);
        let size = [640.0, 480.0];
        for pixel in [[320.0, 240.0], [10.0, 20.0], [600.0, 470.0]] {
            let ray = camera.ray(pixel, size);
            assert!(close(dot(ray.direction, ray.direction), 1.0));
            // A point along the ray projects back onto the pixel.
            let point = std::array::from_fn(|k| ray.origin[k] + ray.direction[k] * 120.0);
            let back = camera.screen(point, size).unwrap();
            assert!(
                (back[0] - pixel[0]).abs() < 1e-6 && (back[1] - pixel[1]).abs() < 1e-6,
                "{pixel:?} {back:?}"
            );
        }
        let centre = camera.ray([320.0, 240.0], size);
        let to_target = normalized(sub(camera.target, camera.eye()));
        assert!(close(dot(centre.direction, to_target), 1.0));
    }

    #[test]
    fn zooming_at_a_point_keeps_it_under_the_pointer_and_brings_the_target() {
        // M6.4f: a point off the target stays where it is on screen while zooming in and out.
        let size = [800.0, 600.0];
        let mut camera = Camera::new([0.0; 3], 300.0);
        let anchor = [30.0, -20.0, 10.0];
        let before = camera.screen(anchor, size).unwrap();
        for steps in [3.0, 5.0, -2.0, 20.0] {
            camera.zoom_at(steps, anchor);
            let after = camera.screen(anchor, size).unwrap();
            assert!((after[0] - before[0]).abs() < 1e-6, "{after:?} {before:?}");
            assert!((after[1] - before[1]).abs() < 1e-6, "{after:?} {before:?}");
        }
        // Zoomed in, the target has come to the anchor, so orbiting and panning work there.
        assert!(dot(sub(camera.target, anchor), sub(camera.target, anchor)).sqrt() < 1.0);
        // Over no surface, the anchor is where the pointer's ray meets the target's plane.
        let camera = Camera::new([5.0, 5.0, 5.0], 200.0);
        let ray = camera.ray([123.0, 456.0], size);
        let focal = camera.focal_point(&ray);
        let (_, _, back) = camera.basis();
        assert!(dot(sub(focal, camera.target), back).abs() < 1e-9);
        let at = camera.screen(focal, size).unwrap();
        assert!((at[0] - 123.0).abs() < 1e-6 && (at[1] - 456.0).abs() < 1e-6);
    }

    #[test]
    fn retargeting_keeps_the_view_and_sets_the_depth_that_pans_and_orbits() {
        let mut camera = Camera::new([0.0; 3], 300.0);
        camera.yaw = 0.7;
        camera.pitch = 0.4;
        let size = [800.0, 600.0];
        let surface = [10.0, -5.0, 20.0];
        let before = camera.screen(surface, size).unwrap();
        let (eye, view) = (camera.eye(), camera.basis().2);
        let depth = camera.depth_of(surface);
        assert!(depth > 0.0 && depth < 300.0);
        camera.retarget(depth);
        // The eye and the view are as they were: nothing on screen moves.
        for (now, was) in [(camera.eye(), eye), (camera.basis().2, view)] {
            assert!(
                now.iter().zip(was).all(|(a, b)| close(*a, b)),
                "{now:?} {was:?}"
            );
        }
        let after = camera.screen(surface, size).unwrap();
        assert!(close(before[0], after[0]) && close(before[1], after[1]));
        assert!(close(camera.distance, depth) && close(camera.depth_of(camera.target), depth));
        // Panning at that depth moves a point there with the pointer, pixel for pixel.
        camera.pan(40.0, -25.0, size[1]);
        let panned = camera.screen(surface, size).unwrap();
        assert!(
            (panned[0] - after[0] - 40.0).abs() < 1.0,
            "{panned:?} {after:?}"
        );
        assert!(
            (panned[1] - after[1] + 25.0).abs() < 1.0,
            "{panned:?} {after:?}"
        );
        // A depth out of range is clamped; one that is not a number changes nothing.
        camera.retarget(0.0);
        assert!(close(camera.distance, MIN_DISTANCE));
        let kept = camera;
        camera.retarget(f64::NAN);
        assert_eq!(camera, kept);
    }

    #[test]
    fn looking_from_a_direction_sets_the_yaw_and_pitch() {
        let mut camera = Camera::new([0.0; 3], 100.0);
        // From the front (-Y), level.
        camera.look_from([0.0, -1.0, 0.0]);
        assert!(close(camera.yaw, -std::f64::consts::FRAC_PI_2) && close(camera.pitch, 0.0));
        // From the right, above at 45 degrees.
        camera.look_from([1.0, 0.0, 1.0]);
        assert!(close(camera.yaw, 0.0) && close(camera.pitch, std::f64::consts::FRAC_PI_4));
        // Straight down: as high as the pitch goes, with the front at the bottom.
        camera.look_from([0.0, 0.0, 2.0]);
        assert!(close(camera.pitch, MAX_PITCH));
        let (_, up, _) = camera.basis();
        assert!(up[1] > 0.99, "{up:?}");
        assert!(close(camera.distance, 100.0));
    }

    #[test]
    fn controls_orbit_pan_zoom_and_fit() {
        let mut camera = Camera::new([0.0; 3], 100.0);
        camera.orbit(0.0, 1e6);
        assert!(close(camera.pitch, MAX_PITCH));
        camera.orbit(0.0, -1e6);
        assert!(close(camera.pitch, -MAX_PITCH));
        // Panning keeps the distance and moves the target in the view plane.
        let mut panned = Camera::new([0.0; 3], 100.0);
        let before = panned.eye();
        panned.pan(50.0, 0.0, 500.0);
        assert!(close(panned.distance, 100.0));
        let moved = sub(panned.eye(), before);
        let (_, _, back) = panned.basis();
        assert!(dot(moved, back).abs() < 1e-9);
        assert!(dot(moved, moved) > 0.0);
        // Zooming in shortens the distance; zooming back restores it.
        let mut zoomed = Camera::new([0.0; 3], 100.0);
        zoomed.zoom(2.0);
        assert!(zoomed.distance < 100.0);
        zoomed.zoom(-2.0);
        assert!(close(zoomed.distance, 100.0));
        // Zooming in comes as close as a miniature's details need, no closer.
        zoomed.zoom(1e3);
        assert!(close(zoomed.distance, MIN_DISTANCE));
        // Fitting keeps every corner of the box on screen, in any aspect.
        for aspect in [0.5, 1.0, 2.5] {
            let mut fitted = Camera::new([0.0; 3], 1.0);
            let (min, max) = ([-10.0, 0.0, 0.0], [150.0, 80.0, 160.0]);
            fitted.fit(min, max, aspect);
            let size = [600.0 * aspect, 600.0];
            for corner in 0..8 {
                let p = std::array::from_fn(|k| if corner >> k & 1 == 1 { max[k] } else { min[k] });
                let [x, y] = fitted.screen(p, size).unwrap();
                assert!(
                    (0.0..=size[0]).contains(&x) && (0.0..=size[1]).contains(&y),
                    "{aspect} {p:?} {x} {y}"
                );
            }
        }
    }

    #[test]
    fn gpu_matrices_are_column_major() {
        let identity = Mat4(std::array::from_fn(|r| {
            std::array::from_fn(|c| if r == c { 1.0 } else { 0.0 })
        }));
        let mut m = identity;
        m.0[0][3] = 7.0;
        assert_eq!(m.to_gpu()[12], 7.0);
        assert_eq!(identity.mul(&m), m);
    }
}
