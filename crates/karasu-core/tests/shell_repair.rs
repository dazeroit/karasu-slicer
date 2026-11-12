//! Small and inner shells (stage R8) and visibility orientation (stage R6), M6.4d r6, TD-092.
use karasu_core::{
    cancellation::Cancellation,
    geometry::Point3,
    mesh::{repair::RepairChange, TriangleMesh},
    preparation::repair::{
        repair_source, RepairPreset, RepairSpec, ShellReport, Shells, SourceRepairReport,
    },
};

/// Boxes `(min, max, inward)` as one mesh; `skip` removes faces (by index) of the first box.
fn boxes(specs: &[([f64; 3], [f64; 3], bool)], skip: &[usize]) -> TriangleMesh {
    let mut points = Vec::new();
    let mut faces = Vec::new();
    for (index, &(min, max, inward)) in specs.iter().enumerate() {
        let base = u32::try_from(points.len()).unwrap();
        for i in 0..8 {
            let at = |bit: usize, axis: usize| if i & bit == 0 { min[axis] } else { max[axis] };
            points.push(Point3::new(at(1, 0), at(2, 1), at(4, 2)));
        }
        let quads = [
            [0, 2, 3, 1],
            [4, 5, 7, 6],
            [0, 1, 5, 4],
            [2, 6, 7, 3],
            [0, 4, 6, 2],
            [1, 3, 7, 5],
        ];
        for (f, t) in quads
            .iter()
            .flat_map(|q| [[q[0], q[1], q[2]], [q[0], q[2], q[3]]])
            .enumerate()
        {
            if index == 0 && skip.contains(&f) {
                continue;
            }
            let t = if inward { [t[0], t[2], t[1]] } else { t };
            faces.push(t.map(|v: u32| v + base));
        }
    }
    TriangleMesh::try_new(points, faces).unwrap()
}

fn repair(mesh: &TriangleMesh, spec: RepairSpec) -> (Option<TriangleMesh>, SourceRepairReport) {
    repair_source(
        mesh,
        spec,
        karasu_core::mesh::topology::TopologyLimits::default(),
        &mut Cancellation::never(),
    )
    .unwrap()
}

fn shells(mesh: &TriangleMesh, bounds: Shells) -> (Option<TriangleMesh>, ShellReport) {
    let (repaired, report) = repair(
        mesh,
        RepairSpec {
            shells: Some(bounds),
            ..RepairPreset::Off.spec()
        },
    );
    (repaired, report.shells.unwrap())
}

const NONE: Shells = Shells {
    min_area_mm2: 0.0,
    min_volume_mm3: 0.0,
    inner: false,
};

#[test]
fn small_shells_are_removed_by_area_or_volume() {
    // A 20 mm box, three 0.3 mm specks (0.54 mm² each) and a 1 mm cube (6 mm², 1 mm³).
    let mesh = boxes(
        &[
            ([0.0; 3], [20.0; 3], false),
            ([30.0, 0.0, 0.0], [30.3, 0.3, 0.3], false),
            ([31.0, 0.0, 0.0], [31.3, 0.3, 0.3], false),
            ([32.0, 0.0, 0.0], [32.3, 0.3, 0.3], false),
            ([40.0, 0.0, 0.0], [41.0, 1.0, 1.0], false),
        ],
        &[],
    );
    let (kept, report) = shells(
        &mesh,
        Shells {
            min_area_mm2: 1.0,
            ..NONE
        },
    );
    assert_eq!((report.small_removed, report.small_faces), (3, 36));
    assert!((report.small_area_mm2 - 3.0 * 0.54).abs() < 1e-9);
    assert_eq!(kept.unwrap().triangles().len(), 24);
    let (kept, report) = shells(
        &mesh,
        Shells {
            min_volume_mm3: 2.0,
            ..NONE
        },
    );
    assert_eq!(report.small_removed, 4);
    assert_eq!(kept.unwrap().triangles().len(), 12);
    assert_eq!(
        report
            .samples
            .iter()
            .filter(|s| s.change == RepairChange::SmallShellRemoved)
            .count(),
        4
    );
    // No bound, nothing removed.
    let (unchanged, report) = shells(&mesh, NONE);
    assert!(unchanged.is_none());
    assert_eq!(report, ShellReport::default());
}

#[test]
fn inner_shells_are_removed_but_cavities_and_bodies_in_cavities_are_kept() {
    let inner = Shells {
        inner: true,
        ..NONE
    };
    // A body inside a body: redundant material.
    let nested = boxes(
        &[([0.0; 3], [20.0; 3], false), ([5.0; 3], [10.0; 3], false)],
        &[],
    );
    let (kept, report) = shells(&nested, inner);
    assert_eq!(report.inner_removed, 1);
    assert!((report.inner_volume_mm3 - 125.0).abs() < 1e-9);
    assert_eq!(kept.unwrap().triangles().len(), 12);
    // A cavity (reversed inner box) is kept.
    let cavity = boxes(
        &[([0.0; 3], [20.0; 3], false), ([5.0; 3], [10.0; 3], true)],
        &[],
    );
    assert!(shells(&cavity, inner).0.is_none());
    // A body floating in a cavity is kept: the other shells' winding there is zero.
    let floating = boxes(
        &[
            ([0.0; 3], [30.0; 3], false),
            ([5.0; 3], [25.0; 3], true),
            ([10.0; 3], [20.0; 3], false),
        ],
        &[],
    );
    assert!(shells(&floating, inner).0.is_none());
    // Crossing bodies are left to slicing's union.
    let crossing = boxes(
        &[
            ([0.0; 3], [20.0; 3], false),
            ([10.0, 5.0, 5.0], [30.0, 15.0, 15.0], false),
        ],
        &[],
    );
    assert!(shells(&crossing, inner).0.is_none());
    // Of two nested bodies with nothing around them, only the inner one goes.
    let pair = boxes(
        &[
            ([0.0; 3], [20.0; 3], false),
            ([5.0; 3], [15.0; 3], false),
            ([8.0; 3], [12.0; 3], false),
        ],
        &[],
    );
    let (kept, report) = shells(&pair, inner);
    assert_eq!(report.inner_removed, 2);
    assert_eq!(kept.unwrap().triangles().len(), 12);
}

#[test]
fn open_parts_face_where_they_are_seen_from_outside() {
    let visibility = RepairSpec {
        visibility: true,
        ..RepairPreset::Off.spec()
    };
    // An open box (no top) facing inward: its walls see their own interior; turned.
    let inward = boxes(&[([0.0; 3], [10.0; 3], true)], &[2, 3]);
    let (turned, report) = repair(&inward, visibility);
    let v = report.visibility.unwrap();
    assert_eq!((v.open_parts, v.parts_flipped, v.faces_flipped), (1, 1, 10));
    let outward = boxes(&[([0.0; 3], [10.0; 3], false)], &[2, 3]);
    assert_eq!(turned.unwrap().triangles(), outward.triangles());
    // Facing outward already: unchanged.
    let (unchanged, report) = repair(&outward, visibility);
    assert!(unchanged.is_none());
    assert_eq!(report.visibility.unwrap().parts_flipped, 0);
    // A lone square sheet is seen equally from both sides: undecided, unchanged.
    let sheet = TriangleMesh::try_new(
        vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(1.0, 1.0, 0.0),
            Point3::new(0.0, 1.0, 0.0),
        ],
        vec![[0, 1, 2], [0, 2, 3]],
    )
    .unwrap();
    let (unchanged, report) = repair(&sheet, visibility);
    assert!(unchanged.is_none());
    assert_eq!(report.visibility.unwrap().undecided, 1);
}
