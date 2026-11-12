//! Large, irregular and non-simple holes (M6.4d r5, TD-092, stage R7).
use karasu_core::{
    cancellation::Cancellation,
    geometry::Point3,
    mesh::{
        topology::{DefectReport, TopologyLimits},
        TriangleMesh,
    },
    preparation::repair::{
        repair_source, self_intersections, FillHoles, FillReport, HoleRefusal, RepairPreset,
        RepairSpec,
    },
};
use std::f64::consts::{PI, TAU};

fn defects(mesh: &TriangleMesh) -> DefectReport {
    mesh.defect_report_with_cancellation(TopologyLimits::default(), &mut Cancellation::never())
        .unwrap()
}

fn fill(mesh: &TriangleMesh, holes: FillHoles) -> (Option<TriangleMesh>, FillReport) {
    let spec = RepairSpec {
        fill_holes: Some(holes),
        allow_unfilled: true,
        ..RepairPreset::Off.spec()
    };
    let (filled, report) = repair_source(
        mesh,
        spec,
        TopologyLimits::default(),
        &mut Cancellation::never(),
    )
    .unwrap();
    (filled, report.fill.unwrap())
}

fn closed(mesh: &TriangleMesh) -> bool {
    let d = defects(mesh);
    d.boundary_edges == 0 && d.winding_conflicts == 0 && d.unbalanced_non_manifold_edges == 0
}

fn intersecting_pairs(mesh: &TriangleMesh) -> usize {
    self_intersections(mesh, usize::MAX, &mut Cancellation::never())
        .unwrap()
        .pairs
}

/// A unit-radius UV sphere with `around` longitudes and `bands` latitude bands, outward, without
/// the faces for which `drop(centroid)` holds.
fn sphere(around: u32, bands: u32, drop: impl Fn([f64; 3]) -> bool) -> TriangleMesh {
    let mut points = vec![Point3::new(0.0, 0.0, 1.0)];
    for b in 1..bands {
        let theta = PI * f64::from(b) / f64::from(bands);
        for a in 0..around {
            let phi = TAU * f64::from(a) / f64::from(around);
            points.push(Point3::new(
                theta.sin() * phi.cos(),
                theta.sin() * phi.sin(),
                theta.cos(),
            ));
        }
    }
    points.push(Point3::new(0.0, 0.0, -1.0));
    let south = u32::try_from(points.len() - 1).unwrap();
    let ring = |b: u32, a: u32| 1 + (b - 1) * around + a % around;
    let mut faces = Vec::new();
    for a in 0..around {
        faces.push([0, ring(1, a), ring(1, a + 1)]);
        for b in 1..bands - 1 {
            let (p, q, r, s) = (
                ring(b, a),
                ring(b + 1, a),
                ring(b + 1, a + 1),
                ring(b, a + 1),
            );
            faces.push([p, q, r]);
            faces.push([p, r, s]);
        }
        faces.push([ring(bands - 1, a), south, ring(bands - 1, a + 1)]);
    }
    let kept = faces
        .into_iter()
        .filter(|f| {
            let c: [f64; 3] = std::array::from_fn(|k| {
                f.iter()
                    .map(|&v| points[v as usize].coordinates()[k])
                    .sum::<f64>()
                    / 3.0
            });
            !drop(c)
        })
        .collect();
    TriangleMesh::try_new(points, kept).unwrap()
}

#[test]
fn a_large_irregular_hole_is_refined_faired_and_closed_without_intersections() {
    // A wavy cut through the upper sphere: a non-planar rim of a few hundred edges.
    let mesh = sphere(96, 48, |c| {
        let phi = c[1].atan2(c[0]);
        c[2] > 0.35 + 0.15 * (5.0 * phi).sin()
    });
    let rim = defects(&mesh);
    assert_eq!(rim.rim_loops, 1);
    assert!(rim.largest_rim_edges > 96, "{rim:?}");
    // Without new vertices, the minimum-weight cap is long and thin; refined, it matches the
    // surrounding faces.
    let exact = FillHoles {
        max_hole_edges: usize::MAX,
        ..FillHoles::default()
    };
    let (flat, report) = fill(&mesh, exact);
    assert_eq!((report.holes_filled, report.vertices_added), (1, 0));
    let flat = flat.unwrap();
    let refined_spec = FillHoles {
        refine: true,
        ..exact
    };
    let (refined, report) = fill(&mesh, refined_spec);
    let refined = refined.unwrap();
    assert_eq!(report.holes_filled, 1);
    assert!(report.vertices_added > 100, "{report:?}");
    assert!(closed(&refined) && closed(&flat));
    assert_eq!(intersecting_pairs(&refined), 0);
    // No cap edge is much longer than the sphere's edges near the rim (about 0.065).
    let original = mesh.triangles().len();
    let longest = refined.triangles()[original..]
        .iter()
        .flat_map(|t| (0..3).map(move |k| (t[k], t[(k + 1) % 3])))
        .map(|(a, b)| {
            let (p, q) = (
                refined.vertices()[a as usize],
                refined.vertices()[b as usize],
            );
            ((p.x - q.x).powi(2) + (p.y - q.y).powi(2) + (p.z - q.z).powi(2)).sqrt()
        })
        .fold(0.0f64, f64::max);
    assert!(longest < 0.3, "{longest}");
    // The faired membrane stays within the hole's span: below the sphere's top, above the rim's
    // lowest point.
    for p in &refined.vertices()[mesh.vertices().len()..] {
        assert!(p.z > 0.19 && p.z < 1.0, "{p:?}");
    }
    // The same input gives the same cap.
    assert_eq!(fill(&mesh, refined_spec).0.unwrap(), refined);
}

#[test]
fn a_hole_beyond_the_triangulation_bound_needs_refinement() {
    // 520 longitudes: the rim of the polar cap has 520 edges.
    let mesh = sphere(520, 6, |c| c[2] > 0.6);
    let unbounded = FillHoles {
        max_hole_edges: usize::MAX,
        ..FillHoles::default()
    };
    let (unchanged, report) = fill(&mesh, unbounded);
    assert!(unchanged.is_none());
    assert_eq!(report.refused[0].reason, HoleRefusal::TooLarge);
    let (filled, report) = fill(
        &mesh,
        FillHoles {
            refine: true,
            ..unbounded
        },
    );
    let filled = filled.unwrap();
    assert_eq!((report.holes_filled, report.largest_filled_edges), (1, 520));
    assert!(report.vertices_added > 0);
    assert!(closed(&filled));
    assert_eq!(intersecting_pairs(&filled), 0);
}

#[test]
fn a_planar_rim_is_faired_into_a_dome_unless_the_surface_meets_it_at_a_crease() {
    let refine = FillHoles {
        refine: true,
        ..FillHoles::default()
    };
    // The polar cap of a sphere: a planar rim that the surface meets at a shallow angle. The
    // cap continues the sphere's slope and rises above the rim, short of the sphere's top.
    let mesh = sphere(48, 24, |c| c[2] > 0.9);
    let (domed, report) = fill(&mesh, refine);
    let domed = domed.unwrap();
    assert!(report.vertices_added > 0 && closed(&domed));
    let top = domed.vertices()[mesh.vertices().len()..]
        .iter()
        .map(|p| p.z)
        .fold(f64::MIN, f64::max);
    assert!(top > 0.95 && top < 1.0, "{top}");
    // The same cap of a sphere cut by a plane through its equator: the surface meets the rim
    // vertically, at a crease, and the cap stays in the rim's plane.
    let mesh = sphere(48, 24, |c| c[2] > 0.0);
    let (flat, report) = fill(&mesh, refine);
    let flat = flat.unwrap();
    assert!(report.vertices_added > 0 && closed(&flat));
    for p in &flat.vertices()[mesh.vertices().len()..] {
        assert!(p.z.abs() < 1e-9, "{p:?}");
    }
}

/// A 2 x 2 grid on the top of a box, with the two triangles of the cells at (0, 0) and (1, 1)
/// removed: two holes touching at the centre vertex, one rim through it twice.
fn bow_tie() -> TriangleMesh {
    let mut points = Vec::new();
    for j in 0..3 {
        for i in 0..3 {
            points.push(Point3::new(f64::from(i) * 5.0, f64::from(j) * 5.0, 10.0));
        }
    }
    for [x, y] in [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]] {
        points.push(Point3::new(x, y, 0.0));
    }
    let top = |i: u32, j: u32| j * 3 + i;
    let mut faces = Vec::new();
    for j in 0..2 {
        for i in 0..2 {
            if (i, j) == (0, 0) || (i, j) == (1, 1) {
                continue;
            }
            let (a, b, c, d) = (top(i, j), top(i + 1, j), top(i + 1, j + 1), top(i, j + 1));
            faces.push([a, b, c]);
            faces.push([a, c, d]);
        }
    }
    // Bottom and sides, through the top grid's border vertices.
    faces.extend([[9, 11, 10], [9, 12, 11]]);
    let border = [
        (0, 9, 1, 2, 10),
        (2, 10, 5, 8, 11),
        (8, 11, 7, 6, 12),
        (6, 12, 3, 0, 9),
    ];
    for (a, low, mid, b, next_low) in border {
        faces.push([a, low, mid]);
        faces.push([mid, low, next_low]);
        faces.push([mid, next_low, b]);
    }
    TriangleMesh::try_new(points, faces).unwrap()
}

#[test]
fn a_rim_through_a_pinch_vertex_is_capped_as_its_loops() {
    let mesh = bow_tie();
    let d = defects(&mesh);
    assert!(d.boundary_edges == 8 && d.winding_conflicts == 0, "{d:?}");
    let (unchanged, report) = fill(&mesh, FillHoles::default());
    assert!(unchanged.is_none());
    assert!(report
        .refused
        .iter()
        .all(|r| r.reason == HoleRefusal::NotSimple));
    let (filled, report) = fill(
        &mesh,
        FillHoles {
            split_rims: true,
            ..FillHoles::default()
        },
    );
    let filled = filled.unwrap();
    assert_eq!(report.holes_refused, 0);
    assert_eq!(report.vertices_added, 0);
    assert!(closed(&filled));
    assert_eq!(report.rims_split, 1);
    // Both loops capped, the centre's fan is whole again: no pinch is left.
    assert_eq!(defects(&filled).non_manifold_vertices, 0);
}

#[test]
fn a_refined_cap_that_meets_the_mesh_falls_back_to_the_exact_cap() {
    // The polar cap of a sphere removed (z > 0.8), and a small closed box across where the
    // sphere's surface was: the faired cap, which follows the curvature up to z = 1, passes
    // through it; the flat minimum-weight cap at z = 0.8 does not.
    let open = sphere(48, 24, |c| c[2] > 0.8);
    let mut points = open.vertices().to_vec();
    let mut faces = open.triangles().to_vec();
    let base = u32::try_from(points.len()).unwrap();
    for i in 0..8 {
        let at = |bit, low, high| if i & bit == 0 { low } else { high };
        points.push(Point3::new(
            at(1, -0.05, 0.05),
            at(2, -0.05, 0.05),
            at(4, 0.96, 1.06),
        ));
    }
    for q in [
        [0, 2, 3, 1],
        [4, 5, 7, 6],
        [0, 1, 5, 4],
        [2, 6, 7, 3],
        [0, 4, 6, 2],
        [1, 3, 7, 5],
    ] {
        faces.push([q[0] + base, q[1] + base, q[2] + base]);
        faces.push([q[0] + base, q[2] + base, q[3] + base]);
    }
    let mesh = TriangleMesh::try_new(points, faces).unwrap();
    let (filled, report) = fill(
        &mesh,
        FillHoles {
            refine: true,
            ..FillHoles::default()
        },
    );
    assert_eq!(
        (
            report.holes_filled,
            report.unrefined_caps,
            report.vertices_added
        ),
        (1, 1, 0),
        "{report:?}"
    );
    let filled = filled.unwrap();
    assert!(closed(&filled));
    assert_eq!(intersecting_pairs(&filled), 0);
}
