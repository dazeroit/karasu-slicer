//! The extended defect diagnosis (M6.4d r1, TD-092): needles and caps, cracks, pinch vertices,
//! small components, and where each defect is.
use karasu_core::{
    cancellation::Cancellation,
    geometry::Point3,
    mesh::{
        topology::{DefectClass, DefectReport, TopologyLimits, MAX_SAMPLES},
        TriangleMesh,
    },
    preparation::repair::{self_intersections, IntersectionReport},
};

fn diagnose(mesh: &TriangleMesh) -> DefectReport {
    mesh.defect_report_with_cancellation(TopologyLimits::default(), &mut Cancellation::never())
        .unwrap()
}

/// The 8 corners and 12 outward faces of a box, with vertex indices offset by `base`.
fn cube(min: [f64; 3], max: [f64; 3], base: u32) -> (Vec<Point3>, Vec<[u32; 3]>) {
    let points = (0..8)
        .map(|i| {
            Point3::new(
                if i & 1 == 0 { min[0] } else { max[0] },
                if i & 2 == 0 { min[1] } else { max[1] },
                if i & 4 == 0 { min[2] } else { max[2] },
            )
        })
        .collect();
    let triangles = [
        [0, 2, 3, 1],
        [4, 5, 7, 6],
        [0, 1, 5, 4],
        [2, 6, 7, 3],
        [0, 4, 6, 2],
        [1, 3, 7, 5],
    ]
    .iter()
    .flat_map(|q| [[q[0], q[1], q[2]], [q[0], q[2], q[3]]])
    .map(|t: [u32; 3]| t.map(|v| v + base))
    .collect();
    (points, triangles)
}

fn mesh(parts: &[(Vec<Point3>, Vec<[u32; 3]>)]) -> TriangleMesh {
    let points = parts.iter().flat_map(|p| p.0.clone()).collect();
    let triangles = parts.iter().flat_map(|p| p.1.clone()).collect();
    TriangleMesh::try_new(points, triangles).unwrap()
}

fn samples(report: &DefectReport, class: DefectClass) -> Vec<[f64; 3]> {
    report
        .samples
        .iter()
        .filter(|s| s.class == class)
        .map(|s| s.point)
        .collect()
}

#[test]
fn a_clean_box_has_no_defects_and_no_samples() {
    let report = diagnose(&mesh(&[cube([0.0; 3], [10.0; 3], 0)]));
    assert_eq!(
        (
            report.needles,
            report.caps,
            report.cracks,
            report.non_manifold_vertices,
            report.small_components,
        ),
        (0, 0, 0, 0, 0)
    );
    assert!(report.samples.is_empty(), "{:?}", report.samples);
    assert!((report.smallest_component_mm2 - 600.0).abs() < 1e-9);
}

#[test]
fn caps_at_t_junctions_are_cracks_and_needles_are_told_apart() {
    // The front side split at the bottom edge's midpoint (vertex 8), the bottom not: a crack
    // along the edge, with the zero-area cap an exporter leaves in it.
    let (mut points, triangles) = cube([0.0; 3], [10.0; 3], 0);
    points.push(Point3::new(5.0, 0.0, 0.0));
    let mut triangles: Vec<[u32; 3]> = triangles.into_iter().filter(|t| *t != [0, 1, 5]).collect();
    triangles.extend([[0, 8, 5], [8, 1, 5], [0, 1, 8]]);
    // A needle elsewhere: two of its corners at one position, at different indices.
    points.push(Point3::new(10.0, 10.0, 10.0));
    triangles.push([7, 9, 6]);
    let report = diagnose(&TriangleMesh::try_new(points, triangles).unwrap());
    assert_eq!((report.caps, report.needles), (1, 1), "{report:?}");
    assert_eq!((report.rim_loops, report.cracks), (1, 1), "{report:?}");
    let cap = samples(&report, DefectClass::Cap);
    assert!((cap[0][0] - 5.0).abs() < 1e-9 && cap[0][1] == 0.0 && cap[0][2] == 0.0);
    assert_eq!(samples(&report, DefectClass::Crack).len(), 1);
    assert_eq!(samples(&report, DefectClass::Rim).len(), 1);
    assert_eq!(samples(&report, DefectClass::Needle).len(), 1);
}

#[test]
fn boxes_touching_at_a_corner_pinch_it() {
    let report = diagnose(&mesh(&[
        cube([0.0; 3], [10.0; 3], 0),
        // The second box's lowest corner is the first box's highest one, vertex 7.
        {
            let (points, triangles) = cube([10.0; 3], [20.0; 3], 8);
            let triangles = triangles
                .into_iter()
                .map(|t| t.map(|v| if v == 8 { 7 } else { v }))
                .collect();
            (points, triangles)
        },
    ]));
    assert_eq!(report.non_manifold_vertices, 1, "{report:?}");
    assert_eq!(
        samples(&report, DefectClass::NonManifoldVertex),
        [[10.0; 3]]
    );
    assert_eq!(report.non_manifold_edges, 0);
}

#[test]
fn small_components_are_counted_and_samples_are_bounded() {
    // A 10 mm box and twenty specks of 0.3 mm (0.54 mm² each).
    let mut parts = vec![cube([0.0; 3], [10.0; 3], 0)];
    for k in 0..20u32 {
        let x = 20.0 + f64::from(k);
        parts.push(cube([x, 0.0, 0.0], [x + 0.3, 0.3, 0.3], 8 * (k + 1)));
    }
    let report = diagnose(&mesh(&parts));
    assert_eq!(report.components, 21);
    assert_eq!(report.small_components, 20);
    assert!((report.smallest_component_mm2 - 0.54).abs() < 1e-9);
    assert_eq!(
        samples(&report, DefectClass::SmallComponent).len(),
        MAX_SAMPLES
    );
}

fn intersections(mesh: &TriangleMesh, max_pairs: usize) -> IntersectionReport {
    self_intersections(mesh, max_pairs, &mut Cancellation::never()).unwrap()
}

#[test]
fn a_clean_box_and_boxes_touching_at_a_corner_do_not_intersect() {
    let report = intersections(&mesh(&[cube([0.0; 3], [10.0; 3], 0)]), usize::MAX);
    assert_eq!(
        report,
        IntersectionReport {
            pairs: 0,
            complete: true,
            samples: Vec::new()
        }
    );
    // Sharing the corner vertex is proper contact; two coincident corners at different indices
    // would not be, but import welds exact positions.
    let pinched = mesh(&[cube([0.0; 3], [10.0; 3], 0), {
        let (points, triangles) = cube([10.0; 3], [20.0; 3], 8);
        let triangles = triangles
            .into_iter()
            .map(|t| t.map(|v| if v == 8 { 7 } else { v }))
            .collect();
        (points, triangles)
    }]);
    assert_eq!(intersections(&pinched, usize::MAX).pairs, 0);
}

#[test]
fn overlapping_boxes_intersect_where_they_cross() {
    // Two boxes crossing: the second's faces pierce the first's top and sides.
    let crossing = mesh(&[
        cube([0.0; 3], [10.0; 3], 0),
        cube([5.0, 2.0, 5.0], [15.0, 8.0, 15.0], 8),
    ]);
    let report = intersections(&crossing, usize::MAX);
    assert!(report.complete);
    assert!(report.pairs > 0, "{report:?}");
    assert_eq!(report.samples.len(), report.pairs.min(16));
    // Every pair involves a face of each box, so every first face lies on one of the two
    // boundaries, near the crossing.
    for p in &report.samples {
        assert!(
            (0.0..=15.0).contains(&p[0]) && (0.0..=15.0).contains(&p[2]),
            "{p:?}"
        );
    }
    let bounded = intersections(&crossing, 1);
    assert_eq!((bounded.pairs, bounded.complete), (1, false));
}

#[test]
fn faces_without_a_normal_are_left_to_the_defect_report() {
    // A needle through the box's interior would cross nothing with area; it is a defect, not an
    // intersection.
    let (mut points, mut triangles) = cube([0.0; 3], [10.0; 3], 0);
    points.push(Point3::new(-5.0, 5.0, 5.0));
    points.push(Point3::new(15.0, 5.0, 5.0));
    triangles.push([8, 9, 9]);
    let report = intersections(
        &TriangleMesh::try_new(points, triangles).unwrap(),
        usize::MAX,
    );
    assert_eq!(report.pairs, 0);
}
