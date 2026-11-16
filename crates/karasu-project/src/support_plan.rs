//! The planners of the automatic supports, headless: the Automatic tool's plan of every support
//! a model needs ([`auto`]) and Support the islands' plan of a support where each island starts
//! ([`islands`]), with what they share with supports placed by hand: the supports' values
//! ([`values`]), routing clear of the model ([`route`]), branches joining a trunk ([`branches`])
//! and trunks merging into trees ([`trees`]).
//!
//! The desktop application plans with them on threads of its own and shows the result before it
//! is applied (TD-156); the command line plans with them through [`generate`], the Automatic
//! tool's Generate and Apply for one model of a slicing request (TD-157), which reads what a
//! slice finds through [`found`], the tool's value set through [`settings`] and the Braces
//! tool's values through [`braces`]. Everything here works in world millimetres on a model's
//! surface for routes ([`route::RouteSurface`]) and is deterministic for equal inputs; the plans
//! take the caller's cancellation flag. The decisions are TD-112 (the Automatic tool and the
//! final check), TD-127 (density and wide undersides), TD-136 (edges, corners and leading
//! edges), TD-137 (the cavity's own preset and density) and TD-138 (the Grid mode's supports,
//! grown by [`auto::plan_tips`]).
use karasu_core::{geometry::Point3, mesh::TriangleMesh};

pub mod auto;
pub mod braces;
pub mod branches;
pub mod found;
pub mod generate;
pub mod islands;
pub mod route;
pub mod settings;
pub mod trees;
pub mod values;

/// An equilateral triangle of circumradius `radius` centred on `point`, in the plane normal to
/// `normal` and facing along it: the surface where a support's tip touches, for building the
/// support alone (its preview, or a planned support's check).
#[must_use]
pub fn tangent_triangle(point: [f64; 3], normal: [f64; 3], radius: f64) -> Option<TriangleMesh> {
    let length = normal.iter().map(|v| v * v).sum::<f64>().sqrt();
    if !(length.is_finite() && length > 0.0) {
        return None;
    }
    let n = normal.map(|v| v / length);
    // A direction across the normal: the axis it is least along.
    let other = if n[0].abs() < 0.9 {
        [1.0, 0.0, 0.0]
    } else {
        [0.0, 1.0, 0.0]
    };
    let cross = |a: [f64; 3], b: [f64; 3]| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    let u = cross(n, other);
    let u_length = u.iter().map(|v| v * v).sum::<f64>().sqrt();
    let u = u.map(|v| v / u_length);
    let v = cross(n, u);
    let corner = |angle_deg: f64| {
        let (sin, cos) = angle_deg.to_radians().sin_cos();
        Point3::new(
            point[0] + radius * (cos * u[0] + sin * v[0]),
            point[1] + radius * (cos * u[1] + sin * v[1]),
            point[2] + radius * (cos * u[2] + sin * v[2]),
        )
    };
    // Counter-clockwise about the normal (u, then v = n × u): the face looks along `normal`.
    let corners = vec![corner(0.0), corner(120.0), corner(240.0)];
    TriangleMesh::try_new(corners, vec![[0, 1, 2]]).ok()
}
