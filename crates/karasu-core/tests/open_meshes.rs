//! Defective meshes (TD-054, phase 1): lean defect diagnosis and rim-paired chord closure of open
//! sections, against exact references.
//!
//! Closure oracle: removing side faces of a vertical prism leaves holes whose sections are exactly
//! the chords between the hole's rim crossings, so the closed masks must equal the complete
//! prism's, byte for byte.
// Fixture geometry uses small counts converted to coordinates and short geometric names.
#![allow(
    clippy::many_single_char_names,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation
)]
use karasu_core::{
    cancellation::{Cancellation, Checkpoint},
    geometry::Point3,
    mesh::{
        stl::{import_stl_path, StlOptions, StlUnitPolicy},
        topology::TopologyLimits,
        TriangleMesh,
    },
    raster::{
        parallel::{with_parallel_raster, LayerFill, ParallelItem, ParallelRaster},
        stream::RasterStreamError,
        Compensation, LcdProfile, Orientation, OutsidePolicy, RasterConfig, RasterImage,
        RasterLimits,
    },
    slicing::{
        engine::{LayerConfig, LayerEngineError, LayerFailure, LayerStageLimits},
        grid::BinaryGridSpec,
        regions::RegionFillRule,
        schedule::{
            build_layer_schedule, LayerSchedule, ScheduleEnd, ScheduleLimits, UniformScheduleSpec,
        },
        winding::{SectionClosure, WindingRule},
        SectionTransform,
    },
};
use std::{collections::BTreeMap, f64::consts::TAU, path::Path};

fn never(_: Checkpoint) -> bool {
    false
}

fn plan(first: f64, step: f64, count: u64) -> LayerSchedule {
    build_layer_schedule(
        UniformScheduleSpec::try_new(first, step, ScheduleEnd::Count(count)).unwrap(),
        ScheduleLimits {
            max_layers: 1000,
            max_payload_bytes: 1 << 20,
        },
    )
    .unwrap()
}

fn run(
    mesh: &TriangleMesh,
    schedule: &LayerSchedule,
    samples_per_axis: u8,
    closure: Option<f64>,
) -> Vec<ParallelItem> {
    let spec = ParallelRaster {
        mesh,
        schedule,
        transform: SectionTransform::Identity,
        layer_config: LayerConfig {
            grid: BinaryGridSpec::try_new([0., 0.], -20).unwrap(),
            fill_rule: RegionFillRule::EvenOddStrict,
            stages: LayerStageLimits::default(),
            max_output_payload_bytes: 1 << 26,
        },
        represented_coordinates: true,
        face_index_bytes: Some(1 << 24),
        vertex_avoiding_bytes: Some(1 << 24),
        profile: LcdProfile::try_new([60, 60], [-15., -15.], [30., 30.], Orientation::default())
            .unwrap(),
        raster: RasterConfig {
            samples_per_axis,
            grayscale_levels: 256,
            minimum_gray: 0,
            outside: OutsidePolicy::Reject,
            compensation: Compensation::None,
            kept: karasu_core::raster::KeptSampling::Antialiased,
        },
        bottom: None,
        limits: RasterLimits::default(),
        cut: None,
        kept_faces: &[],
        fill: LayerFill::Winding(WindingRule::Positive),
        closure: closure.map(|max_gap_mm| SectionClosure { max_gap_mm }),
        threads: 3,
        window_bytes: ParallelRaster::DEFAULT_WINDOW_BYTES,
    };
    with_parallel_raster(&spec, &never, |images| images.collect())
}

fn images(items: Vec<ParallelItem>) -> Vec<RasterImage> {
    items
        .into_iter()
        .map(|item| item.unwrap_or_else(|e| panic!("{e}")))
        .collect()
}

fn failure(items: &[ParallelItem]) -> &LayerFailure {
    match items.last() {
        Some(Err(RasterStreamError::Source(LayerEngineError { cause, .. }))) => cause,
        other => panic!("expected a layer failure, got {other:?}"),
    }
}

/// A vertical prism over a star polygon with `points` tips, without the side quads in `missing`.
fn star_prism(points: usize, radii: [f64; 2], height: f64, missing: &[usize]) -> TriangleMesh {
    let n = 2 * points;
    let mut vertices = Vec::new();
    for z in [0., height] {
        for k in 0..n {
            let angle = TAU * k as f64 / n as f64 + 0.1;
            let r = radii[k % 2];
            vertices.push(Point3::new(r * angle.cos(), r * angle.sin(), z));
        }
    }
    vertices.push(Point3::new(0., 0., 0.));
    vertices.push(Point3::new(0., 0., height));
    let (bottom, top) = (2 * n as u32, 2 * n as u32 + 1);
    let mut triangles = Vec::new();
    for i in 0..n {
        let j = (i + 1) % n;
        let (bi, bj, ti, tj) = (i as u32, j as u32, (i + n) as u32, (j + n) as u32);
        if !missing.contains(&i) {
            triangles.push([bi, bj, tj]);
            triangles.push([bi, tj, ti]);
        }
        triangles.push([bottom, bj, bi]);
        triangles.push([top, ti, tj]);
    }
    TriangleMesh::try_new(vertices, triangles).unwrap()
}

/// An axis-aligned box on an integer lattice of `cells` (unit spacing `step` mm, origin `min`),
/// every side subdivided into cell quads with shared lattice vertices, outward. `skip` removes
/// the quads of side `+x` at `(y cell, z cell)`.
fn lattice_box(
    cells: [usize; 3],
    min: [f64; 3],
    step: f64,
    skip: &[(usize, usize)],
) -> TriangleMesh {
    let mut index = BTreeMap::new();
    let mut vertices = Vec::new();
    let mut id = |p: [usize; 3]| {
        *index.entry(p).or_insert_with(|| {
            vertices.push(Point3::new(
                min[0] + step * p[0] as f64,
                min[1] + step * p[1] as f64,
                min[2] + step * p[2] as f64,
            ));
            u32::try_from(vertices.len() - 1).unwrap()
        })
    };
    let mut triangles = Vec::new();
    let mut quad = |q: [[usize; 3]; 4], triangles: &mut Vec<[u32; 3]>| {
        let v = q.map(&mut id);
        triangles.push([v[0], v[1], v[2]]);
        triangles.push([v[0], v[2], v[3]]);
    };
    let [nx, ny, nz] = cells;
    for a in 0..ny {
        for b in 0..nz {
            if !skip.contains(&(a, b)) {
                quad(
                    [
                        [nx, a, b],
                        [nx, a + 1, b],
                        [nx, a + 1, b + 1],
                        [nx, a, b + 1],
                    ],
                    &mut triangles,
                );
            }
            quad(
                [[0, a, b], [0, a, b + 1], [0, a + 1, b + 1], [0, a + 1, b]],
                &mut triangles,
            );
        }
    }
    for a in 0..nx {
        for b in 0..nz {
            quad(
                [
                    [a, ny, b],
                    [a, ny, b + 1],
                    [a + 1, ny, b + 1],
                    [a + 1, ny, b],
                ],
                &mut triangles,
            );
            quad(
                [[a, 0, b], [a + 1, 0, b], [a + 1, 0, b + 1], [a, 0, b + 1]],
                &mut triangles,
            );
        }
        for b in 0..ny {
            quad(
                [
                    [a, b, nz],
                    [a + 1, b, nz],
                    [a + 1, b + 1, nz],
                    [a, b + 1, nz],
                ],
                &mut triangles,
            );
            quad(
                [[a, b, 0], [a, b + 1, 0], [a + 1, b + 1, 0], [a + 1, b, 0]],
                &mut triangles,
            );
        }
    }
    TriangleMesh::try_new(vertices, triangles).unwrap()
}

#[test]
fn defect_report_counts_each_corpus_defect() {
    let report = |name: &str| {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/reference/stl")
            .join(name);
        import_stl_path(path, StlOptions::new(StlUnitPolicy::AssumeMillimeters))
            .unwrap()
            .mesh
            .defect_report_with_cancellation(TopologyLimits::default(), &mut Cancellation::never())
            .unwrap()
    };
    let cube = report("fx-01-cube.stl");
    assert_eq!(
        [
            cube.boundary_edges,
            cube.non_manifold_edges,
            cube.winding_conflicts
        ],
        [0, 0, 0]
    );
    assert_eq!([cube.components, cube.closed_components], [1, 1]);
    let hollow = report("fx-04-hollow-box.stl");
    assert_eq!([hollow.components, hollow.closed_components], [2, 2]);
    let degenerate = report("fx-08-degenerate-faces.stl");
    assert_eq!(
        [
            degenerate.excluded_triangles,
            degenerate.same_orientation_duplicates,
            degenerate.unbalanced_non_manifold_edges
        ],
        [2, 1, 3]
    );
    let reversed = report("fx-09-reversed-face.stl");
    assert_eq!(
        [
            reversed.winding_conflicts,
            reversed.non_orientable_components
        ],
        [3, 0]
    );
    let open = report("fx-10a-open.stl");
    assert_eq!(
        [
            open.boundary_edges,
            open.rim_loops,
            open.rim_chains,
            open.largest_rim_edges
        ],
        [4, 1, 0, 4]
    );
    assert_eq!(open.longest_rim_mm, 80.);
    assert!(open.rims[0].closed);
    let fin = report("fx-10b-non-manifold.stl");
    assert_eq!(
        [
            fin.boundary_edges,
            fin.rim_chains,
            fin.non_manifold_edges,
            fin.unbalanced_non_manifold_edges
        ],
        [2, 1, 1, 1]
    );
}

/// The defect report's edge counts equal the full topology report's on random meshes.
#[test]
fn defect_edge_counts_match_the_full_report() {
    let mut state = 0x2545_f491_4f6c_dd1d_u64;
    let mut next = |m: u32| {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        u32::try_from(state % u64::from(m)).unwrap()
    };
    for case in 0..300 {
        let points: Vec<Point3> = (0..=2 + next(12))
            .map(|_| Point3::new(f64::from(next(3)), f64::from(next(3)), f64::from(next(2))))
            .collect();
        let count = u32::try_from(points.len()).unwrap();
        let triangles = (0..=next(30))
            .map(|_| [next(count), next(count), next(count)])
            .collect();
        let mesh = TriangleMesh::try_new(points, triangles).unwrap();
        let full = *mesh.analyze_topology().unwrap().summary();
        let lean = mesh
            .defect_report_with_cancellation(TopologyLimits::default(), &mut Cancellation::never())
            .unwrap();
        assert_eq!(
            [
                lean.usable_triangles,
                lean.excluded_triangles,
                lean.edges,
                lean.boundary_edges,
                lean.two_face_edges,
                lean.non_manifold_edges,
                lean.winding_conflicts,
            ],
            [
                full.usable_triangles,
                full.excluded_triangles,
                full.edges,
                full.boundary_edges,
                full.two_face_edges,
                full.non_manifold_edges,
                full.winding_conflicts,
            ],
            "case {case}"
        );
        // Every boundary edge belongs to exactly one rim.
        assert!(lean.rims.iter().map(|r| r.edges).sum::<usize>() <= lean.boundary_edges);
        assert!(lean.boundary_edges == 0 || lean.rim_loops + lean.rim_chains > 0);
    }
}

#[test]
fn chord_closure_restores_prisms_with_missing_side_faces() {
    let schedule = plan(0.37, 0.71, 5);
    for (points, missing) in [
        (5, vec![0]),
        (6, vec![1, 4, 7]),
        (7, vec![0, 3, 6, 9, 12]),
        (8, vec![2, 5, 11, 14]),
    ] {
        let complete = star_prism(points, [9., 4.], 4., &[]);
        let open = star_prism(points, [9., 4.], 4., &missing);
        let defects = open
            .defect_report_with_cancellation(TopologyLimits::default(), &mut Cancellation::never())
            .unwrap();
        assert_eq!(defects.rim_loops, missing.len());
        for samples in [1, 4] {
            let expected = images(run(&complete, &schedule, samples, None));
            let closed = images(run(&open, &schedule, samples, Some(100.)));
            for (c, e) in closed.iter().zip(&expected) {
                assert_eq!(c.mask(), e.mask(), "{points} {missing:?} {samples}");
                let report = c.closure().unwrap();
                assert_eq!((report.chords, report.ambiguous_rims), (missing.len(), 0));
            }
            // Without closure the first layer fails as unbalanced.
            assert!(matches!(
                failure(&run(&open, &schedule, samples, None)),
                LayerFailure::Unbalanced { .. }
            ));
        }
    }
}

#[test]
fn wavy_rims_pair_crossings_by_the_shorter_matching() {
    // A U-shaped hole in the +x side of a 5 x 5 x 4 lattice box (2 mm cells): rows 1-3 of
    // columns 1 and 3 and row 1 of column 2. Planes in rows 2-3 cross the rim four times.
    let skip = [(1, 1), (2, 1), (3, 1), (1, 2), (3, 2), (1, 3), (3, 3)];
    let complete = lattice_box([5, 5, 4], [-5., -5., 0.], 2., &[]);
    let open = lattice_box([5, 5, 4], [-5., -5., 0.], 2., &skip);
    let schedule = plan(0.5, 1., 8);
    let expected = images(run(&complete, &schedule, 2, None));
    let closed = images(run(&open, &schedule, 2, Some(10.)));
    let mut ambiguous = 0;
    for (c, e) in closed.iter().zip(&expected) {
        assert_eq!(c.mask(), e.mask(), "{:?}", c.plane());
        ambiguous += c.closure().unwrap().ambiguous_rims;
    }
    // Rows 2 and 3 (z in (4, 8)) hold four of the eight planes.
    assert_eq!(ambiguous, 4);
}

#[test]
fn chords_beyond_the_gap_bound_fail_with_their_position() {
    // The whole +x side of a 10 mm box is missing: every layer needs a 10 mm chord.
    let skip: Vec<_> = (0..5).flat_map(|a| (0..2).map(move |b| (a, b))).collect();
    let open = lattice_box([5, 5, 2], [-5., -5., 0.], 2., &skip);
    let schedule = plan(0.5, 1., 4);
    let items = run(&open, &schedule, 1, Some(5.));
    let LayerFailure::OpenSectionGap {
        length_mm,
        from_mm,
        to_mm,
        max_gap_mm,
    } = failure(&items)
    else {
        panic!("{:?}", failure(&items));
    };
    assert_eq!((*length_mm, *max_gap_mm), (10., 5.));
    assert_eq!((from_mm[0], to_mm[0]), (5., 5.));
    assert!(failure(&items).to_string().contains("10 mm chord"));
    // The same layers close under a 10 mm bound.
    assert_eq!(images(run(&open, &schedule, 1, Some(10.))).len(), 4);
}

#[test]
fn a_mobius_strip_is_reported_non_orientable() {
    let n = 24;
    let mut vertices = Vec::new();
    for k in 0..n {
        let t = TAU * f64::from(k) / f64::from(n);
        for s in [-1., 1.] {
            // Half a twist over one turn.
            let (w, h) = (s * (t / 2.).cos(), s * (t / 2.).sin());
            let r = 10. + w;
            vertices.push(Point3::new(r * t.cos(), r * t.sin(), 5. + h));
        }
    }
    let mut triangles = Vec::new();
    for k in 0..n {
        let (a, b) = (2 * k, 2 * k + 1);
        // The last strip quad joins the rails swapped.
        let (c, d) = if k + 1 == n {
            (1, 0)
        } else {
            (2 * k + 2, 2 * k + 3)
        };
        triangles.push([a, c, d]);
        triangles.push([a, d, b]);
    }
    let mesh = TriangleMesh::try_new(vertices, triangles).unwrap();
    let report = mesh
        .defect_report_with_cancellation(TopologyLimits::default(), &mut Cancellation::never())
        .unwrap();
    assert_eq!(
        [report.components, report.non_orientable_components],
        [1, 1]
    );
    // The boundary is one geometric loop, but its direction reverses at the orientation seam,
    // so it cannot chain head to tail: two open rim chains.
    assert_eq!([report.rim_loops, report.rim_chains], [0, 2]);
}
