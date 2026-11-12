//! The crafted repair corpus (M6.4d r9, TD-092, plan section 8): every generated defect class
//! through the Exact, Standard and Aggressive presets, with the properties every repair keeps.
//!
//! - **Outcome:** each case states, per preset, whether the repaired model is closed (no open
//!   edge, no unbalanced non-manifold edge, no winding conflict, no excluded face).
//! - **Nothing silent:** a derived mesh is returned exactly when the report records a change, and
//!   every change is located by at least one sample.
//! - **Winding:** on a closed source the winding number is unchanged at every probe unless the
//!   report records a change of orientation or a removed shell; each case also states the winding
//!   expected at its probes once closed.
//! - **Welding:** no vertex moves farther than the weld tolerance.
//! - **Determinism:** the stages before rebuilding run on one thread, and two runs give the same
//!   mesh and report (thread counts of rebuilding are covered by `rebuild_solid`).
//!
//! The table of outcomes is printed with `--nocapture` for the corpus record. With
//! `KARASU_CORPUS_EXPORT` set to a directory, each case is also written there as a binary STL, for
//! black-box comparison with other slicers (an STL keeps positions, not shared indices, so the
//! split-vertex case reads back as the clean box).
use karasu_core::{
    cancellation::Cancellation,
    geometry::Point3,
    mesh::{
        topology::{DefectReport, TopologyLimits},
        TriangleMesh,
    },
    preparation::repair::{repair_source, RepairPreset, RepairSpec, SourceRepairReport},
};

/// Points and faces assembled from boxes and loose faces.
#[derive(Default)]
struct Soup {
    points: Vec<Point3>,
    faces: Vec<[u32; 3]>,
    /// Whether points with the same position share an index, so touching boxes share vertices.
    share: bool,
}

/// The outward quads of a box whose corner `i` has bit 1, 2, 4 set for maximum x, y, z. As
/// triangles, faces 0-1 are the bottom, 2-3 the top, 4-5 the front (minimum y), 6-7 the back,
/// 8-9 the left (minimum x) and 10-11 the right.
const QUADS: [[u32; 4]; 6] = [
    [0, 2, 3, 1],
    [4, 5, 7, 6],
    [0, 1, 5, 4],
    [2, 6, 7, 3],
    [0, 4, 6, 2],
    [1, 3, 7, 5],
];

impl Soup {
    #[allow(clippy::float_cmp)] // Sharing is by exact position.
    fn point(&mut self, p: [f64; 3]) -> u32 {
        if let Some(i) = self
            .points
            .iter()
            .position(|q| q.coordinates() == p)
            .filter(|_| self.share)
        {
            return u32::try_from(i).unwrap();
        }
        self.points.push(Point3::new(p[0], p[1], p[2]));
        u32::try_from(self.points.len() - 1).unwrap()
    }

    /// A box, inward when `inward`, without the faces in `skip`; returns its corner indices.
    fn cuboid(&mut self, min: [f64; 3], max: [f64; 3], inward: bool, skip: &[usize]) -> [u32; 8] {
        let corners: [u32; 8] = std::array::from_fn(|i| {
            let at = |bit: usize, axis: usize| if i & bit == 0 { min[axis] } else { max[axis] };
            self.point([at(1, 0), at(2, 1), at(4, 2)])
        });
        for (f, t) in QUADS
            .iter()
            .flat_map(|q| [[q[0], q[1], q[2]], [q[0], q[2], q[3]]])
            .enumerate()
        {
            if !skip.contains(&f) {
                let t = t.map(|v| corners[v as usize]);
                self.faces.push(if inward { [t[0], t[2], t[1]] } else { t });
            }
        }
        corners
    }

    fn mesh(self) -> TriangleMesh {
        TriangleMesh::try_new(self.points, self.faces).unwrap()
    }
}

fn soup() -> Soup {
    Soup::default()
}

fn shared() -> Soup {
    Soup {
        share: true,
        ..Soup::default()
    }
}

const TEN: [f64; 3] = [10.0; 3];
const ORIGIN: [f64; 3] = [0.0; 3];
const INSIDE: [f64; 3] = [3.0, 4.0, 6.0];
const OUTSIDE: [f64; 3] = [-5.0, 4.0, 6.0];

/// A generated case: the source, whether each of Exact, Standard and Aggressive closes it, and
/// the winding expected at probe points once it is closed.
struct Case {
    name: &'static str,
    mesh: TriangleMesh,
    closes: [bool; 3],
    probes: Vec<([f64; 3], i64)>,
}

fn case(
    name: &'static str,
    mesh: TriangleMesh,
    closes: [bool; 3],
    probes: &[([f64; 3], i64)],
) -> Case {
    Case {
        name,
        mesh,
        closes,
        probes: probes.to_vec(),
    }
}

/// A 10 mm box whose front bottom edge is split at a vertex at `offset` below it (toward -y and
/// -z equally), on the front face only: a crack, a zero-width one at offset 0.
fn cracked(offset: f64) -> TriangleMesh {
    let mut s = soup();
    let c = s.cuboid(ORIGIN, TEN, false, &[4]);
    let m = s.point([5.0, -offset, -offset]);
    s.faces.extend([[c[0], m, c[5]], [m, c[1], c[5]]]);
    s.mesh()
}

/// An `n`-sided prism of radius 10 and height 10 with its top missing.
fn open_prism(n: u32) -> TriangleMesh {
    let mut s = soup();
    let angle = |i: u32| f64::from(i) * std::f64::consts::TAU / f64::from(n);
    let ring = |z: f64, s: &mut Soup| -> Vec<u32> {
        (0..n)
            .map(|i| s.point([10.0 * angle(i).cos(), 10.0 * angle(i).sin(), z]))
            .collect()
    };
    let bottom = ring(0.0, &mut s);
    let top = ring(10.0, &mut s);
    let centre = s.point([0.0, 0.0, 0.0]);
    for i in 0..n as usize {
        let j = (i + 1) % n as usize;
        s.faces.push([centre, bottom[j], bottom[i]]);
        s.faces.push([bottom[i], bottom[j], top[j]]);
        s.faces.push([bottom[i], top[j], top[i]]);
    }
    s.mesh()
}

#[allow(clippy::too_many_lines)] // The corpus, one case per defect class.
fn corpus() -> Vec<Case> {
    let cube = || {
        let mut s = soup();
        s.cuboid(ORIGIN, TEN, false, &[]);
        s
    };
    let solid = &[(INSIDE, 1), (OUTSIDE, 0)];
    let mut cases = vec![case("clean box", cube().mesh(), [true; 3], solid)];

    let mut s = soup();
    s.cuboid(ORIGIN, TEN, true, &[]);
    cases.push(case("inside-out box", s.mesh(), [true; 3], solid));

    let mut s = cube();
    s.faces[0] = [s.faces[0][0], s.faces[0][2], s.faces[0][1]];
    cases.push(case("one face reversed", s.mesh(), [true; 3], solid));

    let mut s = cube();
    s.faces.push(s.faces[2]);
    cases.push(case("duplicated face", s.mesh(), [true; 3], solid));

    // The right side uses a copy of corner 7: equal positions, separate indices.
    let mut s = cube();
    let copy = u32::try_from(s.points.len()).unwrap();
    s.points.push(s.points[7]);
    for face in &mut s.faces[10..12] {
        for v in face.iter_mut().filter(|v| **v == 7) {
            *v = copy;
        }
    }
    cases.push(case("split vertex", s.mesh(), [true; 3], solid));

    // A needle: a face whose two corners are copies of one position.
    let mut s = cube();
    let copy = u32::try_from(s.points.len()).unwrap();
    s.points.push(s.points[1]);
    s.faces.push([0, 1, copy]);
    cases.push(case("needle", s.mesh(), [true; 3], solid));

    // A cap: a zero-area face with distinct corners closing a T-junction 0.1 µm high.
    let mut s = cube();
    let m = s.point([5.0, 0.0, 0.0001]);
    s.faces.retain(|f| *f != [0, 1, 5]);
    s.faces.extend([[0, m, 5], [m, 1, 5], [0, 1, m]]);
    cases.push(case("flat cap", s.mesh(), [true; 3], solid));

    // A fold: two faces folded flat onto each other along the top's diagonal.
    let mut s = cube();
    let apex = s.point([2.0, 8.0, 10.0]);
    s.faces.retain(|f| *f != [4, 7, 6]);
    s.faces.extend([[4, 7, apex], [4, apex, 6], [apex, 7, 6]]);
    cases.push(case("fold", s.mesh(), [true; 3], solid));

    cases.push(case("T-junction crack", cracked(0.0), [true; 3], solid));
    cases.push(case("crack 0.5 um", cracked(0.0005), [true; 3], solid));
    cases.push(case("crack 10 um", cracked(0.01), [true; 3], solid));

    // Two halves 0.8 µm apart across x = 5, a 1 µm cell border, without their facing sides.
    let mut s = soup();
    s.cuboid(ORIGIN, [4.9996, 10.0, 10.0], false, &[10, 11]);
    s.cuboid([5.0004, 0.0, 0.0], TEN, false, &[8, 9]);
    let halves = &[([2.5, 5.0, 5.0], 1), ([7.5, 5.0, 5.0], 1), (OUTSIDE, 0)];
    cases.push(case(
        "seam across a cell border",
        s.mesh(),
        [true; 3],
        halves,
    ));

    // A fin: one face hanging from the top back edge (three faces on that edge).
    let mut s = cube();
    let apex = s.point([5.0, 15.0, 15.0]);
    s.faces.push([6, 7, apex]);
    cases.push(case("fin, 3 faces", s.mesh(), [false, true, true], solid));

    // Two boxes sharing an edge: four faces on it, balanced.
    let mut s = shared();
    s.cuboid(ORIGIN, TEN, false, &[]);
    s.cuboid([10.0, 10.0, 0.0], [20.0, 20.0, 10.0], false, &[]);
    let pair = &[(INSIDE, 1), ([15.0, 15.0, 5.0], 1), ([15.0, 5.0, 5.0], 0)];
    cases.push(case("shared edge, 4 faces", s.mesh(), [true; 3], pair));

    // Two boxes sharing a corner: a pinch vertex.
    let mut s = shared();
    s.cuboid(ORIGIN, TEN, false, &[]);
    s.cuboid(TEN, [20.0; 3], false, &[]);
    let pinch = &[(INSIDE, 1), ([15.0; 3], 1), ([15.0, 5.0, 5.0], 0)];
    cases.push(case("pinch vertex", s.mesh(), [true; 3], pinch));

    // Two holes meeting at corner 0: a rim through it twice.
    let mut s = soup();
    s.cuboid(ORIGIN, TEN, false, &[0, 4]);
    cases.push(case("non-simple hole", s.mesh(), [true; 3], solid));

    // Planar rims at a crease: the cap stays flat (0.5 mm above it is outside).
    let tube = &[([0.0, 0.0, 5.0], 1), ([0.0, 0.0, 10.5], 0)];
    cases.push(case("hole of 48 edges", open_prism(48), [true; 3], tube));
    cases.push(case(
        "hole of 300 edges",
        open_prism(300),
        [false, false, true],
        tube,
    ));

    // A model with a cavity, both turned inside out.
    let mut s = soup();
    s.cuboid(ORIGIN, TEN, true, &[]);
    s.cuboid([3.0; 3], [7.0; 3], false, &[]);
    let hollow = &[([1.0; 3], 1), ([5.0; 3], 0), (OUTSIDE, 0)];
    cases.push(case(
        "inside-out with its cavity",
        s.mesh(),
        [true; 3],
        hollow,
    ));

    let mut s = cube();
    s.cuboid([3.0; 3], [7.0; 3], true, &[]);
    cases.push(case("box with a cavity", s.mesh(), [true; 3], hollow));

    let mut s = cube();
    s.cuboid([5.0; 3], [15.0; 3], false, &[]);
    let overlap = &[([2.0; 3], 1), ([7.0; 3], 2), ([12.0; 3], 1), (OUTSIDE, 0)];
    cases.push(case("overlapping shells", s.mesh(), [true; 3], overlap));

    let mut s = cube();
    s.cuboid([3.0; 3], [7.0; 3], false, &[]);
    let nested = &[([1.0; 3], 1), ([5.0; 3], 2), (OUTSIDE, 0)];
    cases.push(case("nested shells", s.mesh(), [true; 3], nested));

    // A body inside another sharing its bottom: the bottom's faces twice on the same side.
    let mut s = shared();
    s.cuboid(ORIGIN, TEN, false, &[]);
    s.cuboid(ORIGIN, [10.0, 10.0, 5.0], false, &[]);
    let stacked = &[([5.0, 5.0, 2.0], 2), ([5.0, 5.0, 8.0], 1), (OUTSIDE, 0)];
    cases.push(case("shells sharing a face", s.mesh(), [true; 3], stacked));

    let mut s = cube();
    s.cuboid([20.0, 0.0, 0.0], [20.3, 0.3, 0.3], false, &[]);
    cases.push(case("small shell", s.mesh(), [true; 3], solid));

    // A lone square sheet: nothing to enclose.
    let mut s = soup();
    let q = [
        [0.0, 0.0, 0.0],
        [10.0, 0.0, 0.0],
        [10.0, 10.0, 0.0],
        [0.0, 10.0, 0.0],
    ]
    .map(|p| s.point(p));
    s.faces.extend([[q[0], q[1], q[2]], [q[0], q[2], q[3]]]);
    cases.push(case("open sheet", s.mesh(), [false; 3], &[(OUTSIDE, 0)]));

    // A box open at the top and turned inside out.
    let mut s = soup();
    s.cuboid(ORIGIN, TEN, true, &[2, 3]);
    cases.push(case("open box, inside out", s.mesh(), [true; 3], solid));
    cases
}

fn defects(mesh: &TriangleMesh) -> DefectReport {
    mesh.defect_report_with_cancellation(TopologyLimits::default(), &mut Cancellation::never())
        .unwrap()
}

fn closed(d: &DefectReport) -> bool {
    d.boundary_edges == 0
        && d.winding_conflicts == 0
        && d.unbalanced_non_manifold_edges == 0
        && d.excluded_triangles == 0
}

/// The generalized winding number of `mesh` at `p`: the faces' solid angles over 4π.
fn winding(mesh: &TriangleMesh, p: [f64; 3]) -> f64 {
    let sub = |v: u32| {
        let q = mesh.vertices()[v as usize].coordinates();
        [q[0] - p[0], q[1] - p[1], q[2] - p[2]]
    };
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let norm = |a: [f64; 3]| dot(a, a).sqrt();
    let total: f64 = mesh
        .triangles()
        .iter()
        .map(|t| {
            let [a, b, c] = t.map(sub);
            let cross = [
                b[1] * c[2] - b[2] * c[1],
                b[2] * c[0] - b[0] * c[2],
                b[0] * c[1] - b[1] * c[0],
            ];
            let (la, lb, lc) = (norm(a), norm(b), norm(c));
            2.0 * dot(a, cross)
                .atan2(la * lb * lc + dot(a, b) * lc + dot(b, c) * la + dot(c, a) * lb)
        })
        .sum();
    total / (4.0 * std::f64::consts::PI)
}

fn repair(mesh: &TriangleMesh, spec: RepairSpec) -> (Option<TriangleMesh>, SourceRepairReport) {
    repair_source(
        mesh,
        spec,
        TopologyLimits::default(),
        &mut Cancellation::never(),
    )
    .unwrap()
}

const PRESETS: [RepairPreset; 3] = [
    RepairPreset::Exact,
    RepairPreset::Standard,
    RepairPreset::Aggressive,
];

/// Whether the report records a change that may change the winding number: turned orientation,
/// removed shells, cut faces or filled holes.
fn changes_winding(report: &SourceRepairReport) -> bool {
    report.mesh.outward_flipped_components > 0
        || report.mesh.reoriented_components > 0
        || report
            .visibility
            .as_ref()
            .is_some_and(|v| v.parts_flipped > 0)
        || report
            .shells
            .as_ref()
            .is_some_and(|s| s.small_removed + s.inner_removed > 0)
        || report
            .non_manifold
            .as_ref()
            .is_some_and(|n| n.fins_removed + n.faces_cut > 0)
}

#[test]
fn every_crafted_defect_meets_its_outcome_and_the_repair_properties() {
    let mut failures = Vec::new();
    println!("| Case | Defects | Exact | Standard | Aggressive |\n| --- | --- | --- | --- | --- |");
    for case in corpus() {
        let source = defects(&case.mesh);
        let mut row = Vec::new();
        for (preset, &expected) in PRESETS.iter().zip(&case.closes) {
            let (repaired, report) = repair(&case.mesh, preset.spec());
            let fail = |what: String| format!("{} / {preset:?}: {what}", case.name);
            // Nothing silent.
            if repaired.is_some() != report.changed() {
                failures.push(fail(
                    "derived mesh without a recorded change, or the reverse".into(),
                ));
            }
            if report.changed() && report.all_samples().next().is_none() {
                failures.push(fail("a change without a location".into()));
            }
            let result = repaired.as_ref().unwrap_or(&case.mesh);
            let d = defects(result);
            let is_closed = closed(&d);
            if is_closed != expected {
                failures.push(fail(format!(
                    "closed {is_closed}, expected {expected}: {d:?}"
                )));
            }
            // Winding: kept on a closed source, and as expected once closed.
            for &(p, w) in &case.probes {
                let after = winding(result, p);
                if closed(&source) && !changes_winding(&report) {
                    let before = winding(&case.mesh, p);
                    if (after - before).abs() > 1e-6 {
                        failures.push(fail(format!("winding at {p:?} {before:.3} -> {after:.3}")));
                    }
                }
                #[allow(clippy::cast_precision_loss)] // Small winding numbers.
                if is_closed && (after - w as f64).abs() > 1e-6 {
                    failures.push(fail(format!(
                        "winding at {p:?} is {after:.3}, expected {w}"
                    )));
                }
            }
            // Determinism.
            let (again, again_report) = repair(&case.mesh, preset.spec());
            if again != repaired || again_report != report {
                failures.push(fail("a second run differs".into()));
            }
            let changes: Vec<String> = {
                let mut kinds: Vec<_> = report
                    .all_samples()
                    .map(|s| format!("{:?}", s.change))
                    .collect();
                kinds.dedup();
                kinds
            };
            row.push(format!(
                "{}{}",
                if is_closed { "closed" } else { "open" },
                if changes.is_empty() {
                    String::new()
                } else {
                    format!(" ({})", changes.join(", "))
                }
            ));
        }
        println!(
            "| {} | {} | {} |",
            case.name,
            describe(&source),
            row.join(" | ")
        );
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// The source's defects, briefly.
fn describe(d: &DefectReport) -> String {
    let mut parts = Vec::new();
    for (count, what) in [
        (d.boundary_edges, "open edges"),
        (d.non_manifold_edges, "non-manifold edges"),
        (d.winding_conflicts, "winding conflicts"),
        (
            d.same_orientation_duplicates + d.opposite_duplicate_pairs,
            "duplicates",
        ),
        (d.needles, "needles"),
        (d.caps, "caps"),
        (d.cracks, "cracks"),
        (d.non_manifold_vertices, "pinch vertices"),
        (d.small_components, "small shells"),
    ] {
        if count > 0 {
            parts.push(format!("{count} {what}"));
        }
    }
    if parts.is_empty() {
        "none".into()
    } else {
        parts.join(", ")
    }
}

#[test]
fn welding_moves_no_vertex_farther_than_its_tolerance() {
    for case in corpus() {
        for tolerance in [0.0, 0.001, 0.01] {
            let spec = RepairSpec {
                weld_mm: Some(tolerance),
                ..RepairPreset::Off.spec()
            };
            let (welded, _) = repair(&case.mesh, spec);
            let Some(welded) = welded else { continue };
            let gap = |p: &Point3, among: &[Point3]| {
                among
                    .iter()
                    .map(|q| {
                        let (a, b) = (p.coordinates(), q.coordinates());
                        ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2))
                            .sqrt()
                    })
                    .fold(f64::INFINITY, f64::min)
            };
            for p in case.mesh.vertices() {
                let moved = gap(p, welded.vertices());
                assert!(
                    moved <= tolerance,
                    "{}: a vertex moved {moved} mm at {tolerance}",
                    case.name
                );
            }
        }
    }
}

#[test]
fn the_corpus_is_exported_on_request() {
    let Ok(directory) = std::env::var("KARASU_CORPUS_EXPORT") else {
        return;
    };
    std::fs::create_dir_all(&directory).unwrap();
    for case in corpus() {
        let mut bytes = vec![0u8; 80];
        bytes[..20].copy_from_slice(b"karasu repair corpus");
        let count = u32::try_from(case.mesh.triangles().len()).unwrap();
        bytes.extend_from_slice(&count.to_le_bytes());
        for t in case.mesh.triangles() {
            bytes.extend_from_slice(&[0u8; 12]);
            for &v in t {
                for c in case.mesh.vertices()[v as usize].coordinates() {
                    #[allow(clippy::cast_possible_truncation)] // STL stores single precision.
                    bytes.extend_from_slice(&(c as f32).to_le_bytes());
                }
            }
            bytes.extend_from_slice(&[0u8; 2]);
        }
        let name = case.name.replace([' ', ','], "-").replace("--", "-");
        std::fs::write(
            std::path::Path::new(&directory).join(format!("{name}.stl")),
            bytes,
        )
        .unwrap();
    }
}
