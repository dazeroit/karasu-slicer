use super::*;
use karasu_core::geometry::Point3;

/// A closed box from `min` to `max`, turned `tilt_deg` about X, its faces wound outward.
fn slab(min: [f64; 3], max: [f64; 3], tilt_deg: f64) -> TriangleMesh {
    let (sin, cos) = tilt_deg.to_radians().sin_cos();
    let corner = |i: usize| -> Point3 {
        let c: [f64; 3] = std::array::from_fn(|a| if i >> a & 1 == 1 { max[a] } else { min[a] });
        Point3::new(c[0], c[1] * cos - c[2] * sin, c[1] * sin + c[2] * cos)
    };
    let vertices = (0..8).map(corner).collect();
    let mut triangles = Vec::new();
    for q in [
        [0, 2, 3, 1],
        [4, 5, 7, 6],
        [0, 1, 5, 4],
        [2, 6, 7, 3],
        [0, 4, 6, 2],
        [1, 3, 7, 5],
    ] {
        triangles.push([q[0], q[1], q[2]]);
        triangles.push([q[0], q[2], q[3]]);
    }
    TriangleMesh::try_new(vertices, triangles).unwrap()
}

/// The bottom's first triangle: the first facing straight down (or down the most).
fn bottom(mesh: &TriangleMesh) -> u32 {
    (0..u32::try_from(mesh.triangles().len()).unwrap())
        .min_by(|&a, &b| {
            let z = |t| facet(mesh, t).unwrap().0[2];
            z(a).total_cmp(&z(b))
        })
        .unwrap()
}

const RADIUS: f64 = 0.26;

#[test]
fn a_flat_bottom_is_picked_whole_and_a_top_not_at_all() {
    let mesh = slab([0.0, 0.0, 5.0], [20.0, 10.0, 7.0], 0.0);
    let seed = bottom(&mesh);
    let face = pick(&mesh, seed, 5.0, &|_| true).unwrap();
    assert_eq!(face.triangles.len(), 2);
    assert!((face.area_mm2 - 200.0).abs() < 1e-9);
    assert!(face.tilt_deg.abs() < 1e-9 && (face.normal[2] + 1.0).abs() < 1e-12);
    // The top faces up: none; a side, kept out: only the seed's own.
    let top = (0..12u32)
        .find(|&t| facet(&mesh, t).unwrap().0[2] > 0.5)
        .unwrap();
    assert!(pick(&mesh, top, 5.0, &|_| true).is_none());
    let alone = pick(&mesh, seed, 5.0, &|t| t == seed).unwrap();
    assert_eq!(alone.triangles, vec![seed]);
}

#[test]
fn a_level_face_takes_corners_edges_and_a_grid_clear_of_them() {
    let mesh = slab([0.0, 0.0, 5.0], [20.0, 10.0, 7.0], 0.0);
    let face = pick(&mesh, bottom(&mesh), 5.0, &|_| true).unwrap();
    let values = DEFAULT_GRID;
    let made = tips(&mesh, &face, &values, RADIUS, (&|_| false, &|_| true));
    assert_eq!(made.corners, 4);
    assert_eq!(made.sides, 0, "a level face has no leading edge");
    assert!(made.edges >= 8 && made.grid > 0, "{made:?}");
    assert_eq!(
        made.tips.len(),
        made.corners + made.edges + made.sides + made.grid
    );
    let inset = values.inset_mm;
    let from_edges = |p: [f64; 3]| [p[0], 20.0 - p[0], p[1], 10.0 - p[1]];
    // Every corner the inset in from two edges; every edge tip the inset in from one.
    for (k, (p, normal, cavity)) in made.tips.iter().enumerate() {
        assert!((p[2] - 5.0).abs() < 1e-9 && normal[2] < 0.0 && !cavity);
        let d = from_edges(*p);
        let at_inset = d.iter().filter(|e| (**e - inset).abs() < 1e-6).count();
        if k < made.corners {
            assert_eq!(at_inset, 2, "{p:?}");
        } else if k < made.corners + made.edges {
            assert_eq!(at_inset, 1, "{p:?}");
        } else {
            // The grid: clear of the outline by the inset, and of its tips by half the spacing.
            assert!(d.iter().all(|e| *e >= inset - 1e-9), "{p:?}");
            for q in &made.tips[..made.corners + made.edges] {
                assert!((p[0] - q.0[0]).hypot(p[1] - q.0[1]) >= 1.5 - 1e-9, "{p:?}");
            }
        }
    }
    // The rows run along the longest edge (X here): grid points share their rows' Y.
    let grid: Vec<[f64; 3]> = made.tips[made.corners + made.edges..]
        .iter()
        .map(|t| t.0)
        .collect();
    for p in &grid {
        assert!(
            grid.iter().filter(|q| (q[1] - p[1]).abs() < 1e-9).count() > 1,
            "{grid:?}"
        );
    }
    // Without edges, no outline tips, and the grid the contact's radius clear of the outline.
    let plain = tips(
        &mesh,
        &face,
        &GridValues {
            edges: false,
            ..values
        },
        RADIUS,
        (&|_| false, &|_| true),
    );
    assert_eq!((plain.corners, plain.edges, plain.sides), (0, 0, 0));
    assert!(plain.grid > made.grid);
    assert!(plain
        .tips
        .iter()
        .all(|t| from_edges(t.0).iter().all(|e| *e >= RADIUS - 1e-9)));
}

#[test]
fn rows_at_an_angle_typed_turn_the_grid() {
    let mesh = slab([0.0, 0.0, 5.0], [30.0, 30.0, 7.0], 0.0);
    let face = pick(&mesh, bottom(&mesh), 5.0, &|_| true).unwrap();
    let turned = GridValues {
        along_longest: false,
        angle_deg: 45.0,
        edges: false,
        ..DEFAULT_GRID
    };
    let made = tips(&mesh, &face, &turned, RADIUS, (&|_| false, &|_| true));
    // Neighbours along the rows lie at 45°, one spacing apart.
    let points: Vec<[f64; 3]> = made.tips.iter().map(|t| t.0).collect();
    let diagonal = points.iter().any(|p| {
        points.iter().any(|q| {
            let (dx, dy) = (q[0] - p[0], q[1] - p[1]);
            (dx.hypot(dy) - 3.0).abs() < 1e-6 && (dx - dy).abs() < 1e-6 && dx > 0.0
        })
    });
    assert!(diagonal, "{points:?}");
}

#[test]
fn a_tilted_face_s_lowest_edge_is_held_close_and_from_its_side() {
    // TD-136 on the grid: 30° about X, the lowest edge at Y = 0 before turning.
    let mesh = slab([0.0, 0.0, 5.0], [15.0, 15.0, 7.0], 30.0);
    let face = pick(&mesh, bottom(&mesh), 5.0, &|_| true).unwrap();
    assert!((face.tilt_deg - 30.0).abs() < 1e-9);
    let made = tips(&mesh, &face, &DEFAULT_GRID, RADIUS, (&|_| false, &|_| true));
    assert!(made.sides >= 3, "{made:?}");
    let low = made
        .tips
        .iter()
        .map(|t| t.0[2])
        .fold(f64::INFINITY, f64::min);
    // The lowest edge's own tips: the radius up the slope from it.
    let lowest_z = (0..8)
        .map(|i| mesh.vertices()[i].z)
        .fold(f64::INFINITY, f64::min);
    let (sin, cos) = 30f64.to_radians().sin_cos();
    let under: Vec<f64> = made.tips[made.corners..made.corners + made.edges]
        .iter()
        .map(|t| (t.0[2] - lowest_z) / sin)
        .filter(|up| *up < 1.0)
        .collect();
    assert!(under.len() >= 3, "{under:?}");
    assert!(
        under.iter().all(|up| (up - RADIUS).abs() < 1e-6),
        "{under:?}"
    );
    // The side tips: on the face beside it, the radius up from the edge, facing down steeply.
    let first_side = made.corners + made.edges;
    for (p, normal, _) in &made.tips[first_side..first_side + made.sides] {
        assert!(((p[2] - lowest_z) / cos - RADIUS).abs() < 1e-6, "{p:?}");
        let tilt = (-normal[2]).acos().to_degrees();
        assert!((tilt - 60.0).abs() < 1e-6, "{tilt}");
    }
    assert!(low >= lowest_z);
}

#[test]
fn values_out_of_range_are_the_built_in_ones() {
    assert_eq!(DEFAULT_GRID.valid(), DEFAULT_GRID);
    let wrong = GridValues {
        spacing_mm: 0.0,
        ..DEFAULT_GRID
    };
    assert_eq!(wrong.valid(), DEFAULT_GRID);
    let grown = GridValues {
        branches: true,
        ..DEFAULT_GRID
    }
    .automatic();
    assert!(grown.branches && !grown.braces);
    assert!((grown.merge_mm).abs() < 1e-12);
}

#[test]
fn a_shape_on_the_screen_selects_the_triangles_facing_down_it_holds() {
    let mesh = slab([0.0, 0.0, 5.0], [20.0, 10.0, 7.0], 0.0);
    // Seen from below, the slab in the middle of an 800 x 600 viewport.
    let mut camera = crate::camera::Camera::new([10.0, 5.0, 6.0], 60.0);
    camera.look_from([0.0, 0.0, -1.0]);
    let size = [800.0, 600.0];
    let view = (&camera.view_projection(size[0] / size[1]), size);
    let whole = [[0.0, 0.0], [800.0, 0.0], [800.0, 600.0], [0.0, 600.0]];
    let all = select(&mesh, view, &whole, &|_| true);
    // The bottom's two triangles: the top faces up, the sides face sideways.
    assert_eq!(all.len(), 2);
    assert!(all.iter().all(|&t| facet(&mesh, t).unwrap().0[2] < -0.99));
    // A lasso round one triangle's centre alone selects it alone.
    let [a, b, c] = corners(&mesh, all[0]);
    let centre = scale(add(a, add(b, c)), 1.0 / 3.0);
    let [x, y] = camera.screen(centre, size).unwrap();
    let lasso = [[x - 5.0, y - 5.0], [x + 5.0, y - 4.0], [x, y + 6.0]];
    assert_eq!(select(&mesh, view, &lasso, &|_| true), vec![all[0]]);
    // Kept out, or a shape of two points: nothing.
    assert_eq!(select(&mesh, view, &whole, &|_| false).len(), 0);
    assert_eq!(select(&mesh, view, &whole[..2], &|_| true).len(), 0);
    // Their face: the bottom, as a click picks it; none of the top alone.
    let face = region(&mesh, all).unwrap();
    assert!((face.area_mm2 - 200.0).abs() < 1e-9 && face.tilt_deg.abs() < 1e-9);
    let top = (0..12u32)
        .filter(|&t| facet(&mesh, t).unwrap().0[2] > 0.5)
        .collect();
    assert!(region(&mesh, top).is_none());
}

#[test]
fn a_border_across_a_face_that_goes_on_is_no_edge() {
    // One of the bottom's two triangles alone: its two sides on the slab's edges are edges, the
    // diagonal across the bottom is not; only the corner between the two edges takes a tip.
    let mesh = slab([0.0, 0.0, 5.0], [20.0, 10.0, 7.0], 0.0);
    let bottom: Vec<u32> = (0..12u32)
        .filter(|&t| facet(&mesh, t).unwrap().0[2] < -0.99)
        .collect();
    let face = region(&mesh, vec![bottom[0]]).unwrap();
    let made = tips(&mesh, &face, &DEFAULT_GRID, RADIUS, (&|_| false, &|_| true));
    assert_eq!(made.corners, 1, "{made:?}");
    let [a, b, c] = corners(&mesh, bottom[0]);
    // The diagonal: the side whose ends lie on opposite corners of the bottom.
    let sides = [(a, b), (b, c), (c, a)];
    let (p, q) = sides
        .into_iter()
        .find(|(p, q)| (p[0] - q[0]).abs() > 1.0 && (p[1] - q[1]).abs() > 1.0)
        .unwrap();
    let off_diagonal = |t: [f64; 3]| {
        let (dx, dy) = (q[0] - p[0], q[1] - p[1]);
        ((t[0] - p[0]) * dy - (t[1] - p[1]) * dx).abs() / dx.hypot(dy)
    };
    for tip in &made.tips[..made.corners + made.edges] {
        assert!(
            off_diagonal(tip.0) > DEFAULT_GRID.inset_mm + 0.05,
            "{tip:?}"
        );
    }
    assert!(made.edges > 0);
}
