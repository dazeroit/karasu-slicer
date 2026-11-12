//! Manual supports on any model (TD-062): document, geometry and validation.
use karasu_core::{
    cancellation::Cancellation,
    geometry::{Affine3, Point3, Vector3},
    mesh::{topology::TopologyLimits, TriangleMesh},
    supports::{
        build_supports, plan_braces, AxisSpec, BaseKind, BaseSpec, BoundPose, BracePattern,
        BracePlan, BuildVolume, DrainCylinder, FindingKind, PlanarMotion, Preset, RaftBars,
        RaftLayout, RaftShape, Severity, SupportDocument, SupportError, SupportKind,
        SupportOptions, SupportPart, SupportScene, SupportSurface, TipCone,
        DEFAULT_FILLET_DIAMETER_MM, DEFAULT_FILLET_HEIGHT_MM,
    },
};

/// Outward (or inward) box faces appended to `points`/`triangles`.
fn push_box(
    min: [f64; 3],
    max: [f64; 3],
    inward: bool,
    points: &mut Vec<Point3>,
    triangles: &mut Vec<[u32; 3]>,
) {
    let base = u32::try_from(points.len()).unwrap();
    for i in 0..8 {
        points.push(Point3::new(
            if i & 1 == 0 { min[0] } else { max[0] },
            if i & 2 == 0 { min[1] } else { max[1] },
            if i & 4 == 0 { min[2] } else { max[2] },
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
        for mut t in [[q[0], q[1], q[2]], [q[0], q[2], q[3]]] {
            if inward {
                t.swap(1, 2);
            }
            triangles.push(t.map(|v| v + base));
        }
    }
}

fn boxes(list: &[([f64; 3], [f64; 3], bool)]) -> TriangleMesh {
    let (mut points, mut triangles) = (Vec::new(), Vec::new());
    for &(min, max, inward) in list {
        push_box(min, max, inward, &mut points, &mut triangles);
    }
    TriangleMesh::try_new(points, triangles).unwrap()
}

/// A 20 x 20 x 5 mm slab floating at Z 5 to 10.
fn slab() -> TriangleMesh {
    boxes(&[([0.0, 0.0, 5.0], [20.0, 20.0, 10.0], false)])
}

fn volume() -> BuildVolume {
    BuildVolume {
        origin_xy_mm: [-50.0, -50.0],
        size_mm: [150.0, 150.0, 100.0],
    }
}

fn document(body: &str) -> SupportDocument {
    SupportDocument::from_json(
        format!(r#"{{"format":"karasu-supports","version":1,{body}}}"#).as_bytes(),
    )
    .unwrap()
}

fn build(
    doc: &SupportDocument,
    mesh: &TriangleMesh,
    interior_from: Option<usize>,
    drains: &[DrainCylinder],
) -> SupportScene {
    build_supports(
        doc,
        SupportSurface {
            mesh,
            interior_from,
        },
        drains,
        volume(),
        SupportOptions::default(),
        &mut Cancellation::never(),
    )
    .unwrap()
}

fn kinds(scene: &SupportScene, severity: Severity) -> Vec<(u64, FindingKind)> {
    scene
        .findings()
        .iter()
        .filter(|f| f.severity == severity)
        .map(|f| (f.entity, f.kind))
        .collect()
}

fn assert_closed(mesh: &TriangleMesh) {
    let report = mesh
        .defect_report_with_cancellation(TopologyLimits::default(), &mut Cancellation::never())
        .unwrap();
    assert_eq!(
        (report.boundary_edges, report.winding_conflicts),
        (0, 0),
        "{report:?}"
    );
}

#[test]
fn plate_supports_under_a_floating_slab_build_cleanly() {
    let mesh = slab();
    let doc = document(
        r#""supports":[
            {"id":1,"tip":{"point":[5,5,5]},"base":{"vertical":true}},
            {"id":2,"tip":{"point":[15,5,5]},"base":{"platform":[17,3]},"parameters":{"contact_shape":"sphere"}},
            {"id":3,"tip":{"point":[10,15,5],"axis":"straight"},"bends":[[10,17,2]],"base":{"platform":[10,18]}}
        ]"#,
    );
    let scene = build(&doc, &mesh, None, &[]);
    assert!(!scene.has_errors(), "{:?}", scene.findings());
    assert_eq!(scene.supports().len(), 3);
    assert!(scene
        .supports()
        .iter()
        .all(|s| s.base == BaseKind::Platform && s.parts > 0 && !s.interior_tip));
    assert_closed(scene.mesh());
    // Everything stands on the plate and stays below the slab's top.
    let zs: Vec<f64> = scene.mesh().vertices().iter().map(|p| p.z).collect();
    assert_eq!(zs.iter().copied().fold(f64::INFINITY, f64::min), 0.0);
    assert!(zs.iter().all(|&z| z < 10.0));
}

#[test]
fn tessellation_depends_only_on_each_support_and_is_deterministic() {
    let mesh = slab();
    let one = document(r#""supports":[{"id":1,"tip":{"point":[5,5,5]},"base":{"vertical":true}}]"#);
    let many = document(
        r#""supports":[{"id":1,"tip":{"point":[5,5,5]},"base":{"vertical":true}},
            {"id":2,"tip":{"point":[15,15,5]},"base":{"vertical":true},"parameters":{"sides":7}}]"#,
    );
    let a = build(&one, &mesh, None, &[]);
    let b = build(&many, &mesh, None, &[]);
    let c = build(&many, &mesh, None, &[]);
    let n = a.mesh().vertices().len();
    assert_eq!(a.mesh().vertices(), &b.mesh().vertices()[..n]);
    assert_eq!(b.mesh().vertices(), c.mesh().vertices());
    assert_eq!(b.mesh().triangles(), c.mesh().triangles());
}

#[test]
fn anchors_axes_and_piercing_are_validated() {
    let mesh = slab();
    let doc =
        document(r#""supports":[{"id":1,"tip":{"point":[5,5,4.5]},"base":{"vertical":true}}]"#);
    let scene = build(&doc, &mesh, None, &[]);
    assert_eq!(
        kinds(&scene, Severity::Error)[0],
        (1, FindingKind::AnchorOffSurface)
    );
    // A side-wall contact with a vertical axis deviates 90° from the normal.
    let doc = document(
        r#""supports":[{"id":5,"tip":{"point":[0,10,7],"axis":[0,0,-1]},"base":{"platform":[-5,10]}}]"#,
    );
    let scene = build(&doc, &mesh, None, &[]);
    assert!(kinds(&scene, Severity::Error).contains(&(5, FindingKind::AxisDeviation)));
    // Reported, not dropped: its shape is still built, to be seen.
    assert!(scene.supports().iter().any(|s| s.id == 5 && s.parts > 0));
    // A 0.3 mm thin plate pierced by 0.5 mm of penetration.
    let thin = boxes(&[([0.0, 0.0, 5.0], [20.0, 20.0, 5.3], false)]);
    let doc = document(
        r#""supports":[{"id":7,"tip":{"point":[5,5,5]},"base":{"vertical":true},"parameters":{"penetration_mm":0.5}}]"#,
    );
    let scene = build(&doc, &thin, None, &[]);
    assert!(kinds(&scene, Severity::Error).contains(&(7, FindingKind::Piercing)));
    assert!(scene.supports().iter().any(|s| s.id == 7 && s.parts > 0));
    let doc = document(
        r#""supports":[{"id":7,"tip":{"point":[5,5,5]},"base":{"vertical":true},"parameters":{"penetration_mm":0.2}}]"#,
    );
    assert!(!build(&doc, &thin, None, &[]).has_errors());
}

#[test]
fn collisions_are_proven_reported_and_accepted_explicitly() {
    // A slab with a block below it: a vertical support from (5, 5) passes through the block.
    let mesh = boxes(&[
        ([0.0, 0.0, 5.0], [20.0, 20.0, 10.0], false),
        ([3.0, 3.0, 0.0], [7.0, 7.0, 2.0], false),
    ]);
    let body = r#""supports":[{"id":1,"tip":{"point":[5,5,5]},"base":{"vertical":true}ACCEPT}]"#;
    let scene = build(&document(&body.replace("ACCEPT", "")), &mesh, None, &[]);
    assert!(kinds(&scene, Severity::Error).contains(&(1, FindingKind::Collision)));
    let scene = build(
        &document(&body.replace("ACCEPT", r#","accept_collision":true"#)),
        &mesh,
        None,
        &[],
    );
    assert!(!scene.has_errors());
    assert!(kinds(&scene, Severity::Accepted).contains(&(1, FindingKind::Collision)));
    // A support 2 mm beside the block's wall, with 0.3 mm clearance, is proven clear.
    let scene = build(
        &document(
            r#""supports":[{"id":2,"tip":{"point":[10.5,5,5]},"base":{"vertical":true},"parameters":{"pad_diameter_mm":2,"cone_diameter_mm":1.5}}]"#,
        ),
        &mesh,
        None,
        &[],
    );
    assert!(!scene.has_errors(), "{:?}", scene.findings());
}

#[test]
fn feet_on_the_model_on_supports_and_inside_cavities() {
    // An overhang above a lower step: the support stands on the step's top face.
    let mesh = boxes(&[
        ([0.0, 0.0, 0.0], [20.0, 10.0, 4.0], false),
        ([0.0, 10.0, 8.0], [20.0, 20.0, 12.0], false),
    ]);
    let doc = document(
        r#""supports":[
            {"id":1,"tip":{"point":[10,15,8]},"base":{"vertical":true}},
            {"id":2,"tip":{"point":[5,12,8]},"bends":[[5,8,6.5]],"base":{"model":[5,5,4]}},
            {"id":3,"tip":{"point":[15,18,8]},"base":{"support":1,"z_mm":3}}
        ],
        "braces":[{"id":9,"from":{"support":1,"z_mm":4},"to":{"support":3,"z_mm":4.5},"diameter_mm":0.6}]"#,
    );
    let scene = build(&doc, &mesh, None, &[]);
    assert!(!scene.has_errors(), "{:?}", scene.findings());
    let bases: Vec<BaseKind> = scene.supports().iter().map(|s| s.base).collect();
    assert_eq!(
        bases,
        [BaseKind::Platform, BaseKind::Model, BaseKind::Support]
    );
    assert_eq!(scene.braces(), 1);
    // A hollow box: the cavity's faces come after the outer faces and face into the cavity.
    let shell = boxes(&[
        ([0.0, 0.0, 0.0], [20.0, 20.0, 20.0], false),
        ([2.0, 2.0, 2.0], [18.0, 18.0, 18.0], true),
    ]);
    let doc =
        document(r#""supports":[{"id":4,"tip":{"point":[10,10,18]},"base":{"model":[10,10,2]}}]"#);
    let scene = build(&doc, &shell, Some(12), &[]);
    assert!(!scene.has_errors(), "{:?}", scene.findings());
    let s = &scene.supports()[0];
    assert!(s.interior_tip && s.interior_foot == Some(true) && s.base == BaseKind::Model);
}

#[test]
fn rafts_drain_holes_and_the_build_volume() {
    let mesh = boxes(&[([0.0, 0.0, 8.0], [20.0, 20.0, 13.0], false)]);
    let doc = document(
        r#""raft":{"thickness_mm":1,"margin_mm":1,"slope_deg":45},
        "supports":[{"id":1,"tip":{"point":[5,5,8]},"base":{"vertical":true}},{"id":2,"tip":{"point":[15,15,8]},"base":{"vertical":true}}]"#,
    );
    let scene = build(&doc, &mesh, None, &[]);
    assert!(!scene.has_errors(), "{:?}", scene.findings());
    assert!(scene.raft());
    assert_closed(scene.mesh());
    // The raft spans both feet's fillets plus margin and slope: 1 + 1 + 1 beyond each centre
    // (the raft is the feet: no pads, owner 2026-10-29).
    let xs: Vec<f64> = scene
        .mesh()
        .vertices()
        .iter()
        .filter(|p| p.z == 0.0)
        .map(|p| p.x)
        .collect();
    let (lo, hi) = xs
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &x| {
            (a.min(x), b.max(x))
        });
    assert!(
        (lo - 2.0).abs() < 1e-9 && (hi - 18.0).abs() < 1e-9,
        "{lo} {hi}"
    );
    // TD-123: drain holes cut only the model. One that runs beside the model, through the first
    // support and the raft, crosses no material, so nothing can plug it.
    for outside in [
        DrainCylinder {
            start_mm: [0.0, 5.0, 4.0],
            end_mm: [10.0, 5.0, 4.0],
            radius_mm: 1.0,
        },
        DrainCylinder {
            start_mm: [0.0, 10.0, 0.5],
            end_mm: [20.0, 10.0, 0.5],
            radius_mm: 0.2,
        },
    ] {
        assert!(!build(&doc, &mesh, None, &[outside]).has_errors());
    }
    // Outside a build volume starting at X 3: the raft reaches X 2.
    let scene = build_supports(
        &doc,
        SupportSurface {
            mesh: &mesh,
            interior_from: None,
        },
        &[],
        BuildVolume {
            origin_xy_mm: [3.0, 0.0],
            size_mm: [100.0, 100.0, 100.0],
        },
        SupportOptions::default(),
        &mut Cancellation::never(),
    )
    .unwrap();
    assert!(kinds(&scene, Severity::Error)
        .iter()
        .any(|&(_, k)| k == FindingKind::BuildVolume));
}

#[test]
fn a_tip_crossing_the_surface_beside_its_contact_is_a_collision() {
    // TD-124: a slab's underside at Z 8, and beside the tip a bulge hanging 2 mm below it from
    // X 6. The tip leans 50° towards the bulge, so its cone runs deep into it.
    let (sin, cos) = 50f64.to_radians().sin_cos();
    let support = format!(
        r#""supports":[{{"id":1,"tip":{{"point":[5,10,8],"axis":[{sin},0,{}]}},"bends":[[8,10,4]],"base":{{"vertical":true}}}}]"#,
        -cos
    );
    let on_contact = |mesh: &TriangleMesh| {
        build(&document(&support), mesh, None, &[])
            .findings()
            .iter()
            .filter(|f| f.part == SupportPart::Contact && f.kind == FindingKind::Collision)
            .map(|f| f.severity)
            .collect::<Vec<_>>()
    };
    let open = boxes(&[([0.0, 0.0, 8.0], [20.0, 20.0, 13.0], false)]);
    assert_eq!(on_contact(&open), []);
    let crease = boxes(&[
        ([0.0, 0.0, 8.0], [20.0, 20.0, 13.0], false),
        ([6.0, 0.0, 6.0], [10.0, 20.0, 8.0], false),
    ]);
    assert_eq!(on_contact(&crease), [Severity::Error]);
    // A bump that rises into the cone's side by less than the contact's penetration is the
    // contact's own: not counted.
    let bump = boxes(&[
        ([0.0, 0.0, 8.0], [20.0, 20.0, 13.0], false),
        ([5.4, 0.0, 7.9], [10.0, 20.0, 8.0], false),
    ]);
    assert_eq!(on_contact(&bump), []);
    // Leaning away from the bulge, the same tip clears it.
    let away = support
        .replace(&format!("[{sin},0,"), &format!("[{},0,", -sin))
        .replace("[8,10,4]", "[2,10,4]");
    let found: Vec<_> = build(&document(&away), &crease, None, &[])
        .findings()
        .iter()
        .filter(|f| f.part == SupportPart::Contact && f.kind == FindingKind::Collision)
        .cloned()
        .collect();
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn a_tip_follows_the_mean_normal_of_the_surface_under_its_contact() {
    // TD-124: a tip on the ridge of two facets each 30° off vertical measures its axis from
    // their mean, not from the one facet it is anchored on: straight down is within a 5° limit
    // (from either facet it would be 30° off).
    let drop = 1.0 / 3f64.sqrt();
    let mesh = TriangleMesh::try_new(
        vec![
            Point3::new(-5.0, -5.0, 10.0 - 5.0 * drop),
            Point3::new(0.0, -5.0, 10.0),
            Point3::new(0.0, 5.0, 10.0),
            Point3::new(-5.0, 5.0, 10.0 - 5.0 * drop),
            Point3::new(5.0, -5.0, 10.0 - 5.0 * drop),
            Point3::new(5.0, 5.0, 10.0 - 5.0 * drop),
        ],
        // Facing down on both sides of the ridge along Y.
        vec![[0, 2, 1], [0, 3, 2], [1, 2, 4], [2, 5, 4]],
    )
    .unwrap();
    let doc = document(
        r#""max_axis_deviation_deg":5,"supports":[{"id":1,"tip":{"point":[0,0,10],"axis":[0,0,-1]},"base":{"vertical":true}}]"#,
    );
    let scene = build(&doc, &mesh, None, &[]);
    let deviations: Vec<_> = scene
        .findings()
        .iter()
        .filter(|f| f.kind == FindingKind::AxisDeviation)
        .collect();
    assert!(deviations.is_empty(), "{deviations:?}");
}

#[test]
fn only_a_support_inside_a_drain_holes_channel_plugs_it() {
    // TD-123: a 1 mm hole up into the slab's underside (Z 8 to 13), from 1 mm below it to 3 mm
    // into the material, as the interface makes one.
    let mesh = boxes(&[([0.0, 0.0, 8.0], [20.0, 20.0, 13.0], false)]);
    let drain = DrainCylinder {
        start_mm: [10.0, 10.0, 7.0],
        end_mm: [10.0, 10.0, 11.0],
        radius_mm: 1.0,
    };
    // A support on the hole's rim, its shaft through the hole's margin below the surface: whole,
    // and nothing in the hole.
    let rim =
        document(r#""supports":[{"id":1,"tip":{"point":[11.4,10,8]},"base":{"vertical":true}}]"#);
    let found = kinds(&build(&rim, &mesh, None, &[drain]), Severity::Warning);
    assert!(!found.contains(&(1, FindingKind::DrainHole)), "{found:?}");
    // One whose tip reaches into the hole may plug it: a warning, not an error (TD-131).
    let inside =
        document(r#""supports":[{"id":1,"tip":{"point":[10,10,8]},"base":{"vertical":true}}]"#);
    let scene = build(&inside, &mesh, None, &[drain]);
    let found = kinds(&scene, Severity::Warning);
    assert!(found.contains(&(1, FindingKind::DrainHole)), "{found:?}");
    assert!(!scene.has_errors(), "{:?}", scene.findings());
    // A hole through both walls of a hollow slab: the cavity it crosses is no channel, so a
    // support whose shaft crosses the hole's axis in the cavity, its ends away from the hole,
    // plugs nothing.
    let shell = boxes(&[
        ([0.0, 0.0, 8.0], [20.0, 20.0, 20.0], false),
        ([2.0, 2.0, 10.0], [18.0, 18.0, 18.0], true),
    ]);
    let through = DrainCylinder {
        start_mm: [10.0, 10.0, 7.0],
        end_mm: [10.0, 10.0, 21.0],
        radius_mm: 1.0,
    };
    let cavity = document(
        r#""supports":[{"id":1,"tip":{"point":[12,10,18]},"bends":[[10,10,14]],"base":{"model":[8,10,10]}}]"#,
    );
    let scene = build(&cavity, &shell, Some(12), &[through]);
    let found = kinds(&scene, Severity::Warning);
    assert!(!found.contains(&(1, FindingKind::DrainHole)), "{found:?}");
    // Its foot on the cavity's floor 2 mm from the hole's axis stands on material.
    assert!(!scene.has_errors(), "{:?}", scene.findings());
    // Its tip on the cavity's ceiling right over the hole plugs the upper wall's channel, and
    // its foot on the floor right over the hole stands on nothing once the hole is cut: an
    // error (TD-131).
    let over =
        document(r#""supports":[{"id":1,"tip":{"point":[10,10,18]},"base":{"model":[10,10,10]}}]"#);
    let scene = build(&over, &shell, Some(12), &[through]);
    let found = kinds(&scene, Severity::Warning);
    assert!(found.contains(&(1, FindingKind::DrainHole)), "{found:?}");
    let errors = kinds(&scene, Severity::Error);
    assert_eq!(errors, vec![(1, FindingKind::FootInHole)], "{errors:?}");
}

#[test]
fn a_raft_is_the_feet_its_shafts_stand_on_it_through_fillets_alone_or_in_groups() {
    let mesh = boxes(&[([0.0, 0.0, 8.0], [60.0, 20.0, 13.0], false)]);
    let feet = r#""supports":[{"id":1,"tip":{"point":[5,5,8]},"base":{"vertical":true}},{"id":2,"tip":{"point":[12,5,8]},"base":{"vertical":true}},{"id":3,"tip":{"point":[50,15,8]},"base":{"vertical":true}}]"#;
    let parts = |scene: &SupportScene, part: SupportPart| {
        scene
            .part_ranges()
            .iter()
            .filter(|r| r.part == part)
            .count()
    };
    // One raft, no pads: each shaft stands on the raft through its fillet, which starts inside
    // the raft and rises above it.
    let single = document(&format!(
        r#""raft":{{"thickness_mm":1,"margin_mm":1,"slope_deg":60,"fillet_diameter_mm":3,"fillet_height_mm":0.5}},{feet}"#
    ));
    let scene = build(&single, &mesh, None, &[]);
    assert!(!scene.has_errors(), "{:?}", scene.findings());
    assert_closed(scene.mesh());
    assert_eq!(parts(&scene, SupportPart::Pad), 0);
    assert_eq!(parts(&scene, SupportPart::Raft), 1);
    assert_eq!(parts(&scene, SupportPart::Cone), 3);
    // Groups: feet 7 mm apart share one, the third, far away, has its own.
    let grouped = document(&format!(
        r#""raft":{{"thickness_mm":1,"margin_mm":1,"slope_deg":60,"groups_join_mm":10}},{feet}"#
    ));
    let scene = build(&grouped, &mesh, None, &[]);
    assert!(!scene.has_errors(), "{:?}", scene.findings());
    assert_eq!(parts(&scene, SupportPart::Raft), 2);
    let raft = grouped.raft.unwrap();
    assert_eq!(raft.layout, RaftLayout::Groups { join_mm: 10.0 });
    // A document without them takes the default fillet.
    assert!((raft.fillet_diameter_mm - DEFAULT_FILLET_DIAMETER_MM).abs() < 1e-12);
    assert!((raft.fillet_height_mm - DEFAULT_FILLET_HEIGHT_MM).abs() < 1e-12);
    // Joined from far enough, they share one again; a fillet of no height joins straight.
    let joined = document(&format!(
        r#""raft":{{"thickness_mm":1,"margin_mm":1,"slope_deg":60,"fillet_height_mm":0,"groups_join_mm":60}},{feet}"#
    ));
    let scene = build(&joined, &mesh, None, &[]);
    assert!(!scene.has_errors(), "{:?}", scene.findings());
    assert_eq!(parts(&scene, SupportPart::Raft), 1);
    // Out of range: refused.
    for wrong in [
        r#""fillet_diameter_mm":20"#,
        r#""fillet_height_mm":-1"#,
        r#""groups_join_mm":0.1"#,
    ] {
        let body =
            format!(r#""raft":{{"thickness_mm":1,"margin_mm":1,"slope_deg":60,{wrong}}},{feet}"#);
        let json = format!(r#"{{"format":"karasu-supports","version":1,{body}}}"#);
        assert!(
            SupportDocument::from_json(json.as_bytes()).is_err(),
            "{wrong}"
        );
    }
}

#[test]
fn a_skate_raft_narrows_towards_the_plate_and_fails_when_nothing_is_left_there() {
    let mesh = boxes(&[([0.0, 0.0, 8.0], [60.0, 20.0, 13.0], false)]);
    let feet = r#""supports":[{"id":1,"tip":{"point":[5,5,8]},"base":{"vertical":true}},{"id":2,"tip":{"point":[12,5,8]},"base":{"vertical":true}}]"#;
    // The raft's half widths along X at the plate and at its top.
    let widths = |scene: &SupportScene| -> [f64; 2] {
        let mesh = scene.mesh();
        let raft = scene
            .part_ranges()
            .iter()
            .find(|r| r.part == SupportPart::Raft)
            .unwrap();
        let mut spans = [[f64::INFINITY, f64::NEG_INFINITY]; 2];
        for triangle in &mesh.triangles()[raft.triangles.clone()] {
            for &v in triangle {
                let p = mesh.vertices()[v as usize];
                let level = usize::from(p.z > 0.5);
                spans[level] = [spans[level][0].min(p.x), spans[level][1].max(p.x)];
            }
        }
        spans.map(|[low, high]| high - low)
    };
    let raft = |shape: &str, thickness: f64| {
        document(&format!(
            r#""raft":{{"thickness_mm":{thickness},"margin_mm":1,"slope_deg":45,"fillet_diameter_mm":2{shape}}},{feet}"#
        ))
    };
    // A slab is wider at the plate: by the thickness on each side at 45°.
    let slab = build(&raft("", 1.0), &mesh, None, &[]);
    let [plate, top] = widths(&slab);
    assert!((plate - top - 2.0).abs() < 1e-6, "{plate} {top}");
    // A skate, the same raft the other way: narrower at the plate by as much, closed, and
    // without errors.
    let doc = raft(r#","shape":"skate""#, 1.0);
    assert_eq!(doc.raft.unwrap().shape, RaftShape::Skate);
    let skate = build(&doc, &mesh, None, &[]);
    assert!(!skate.has_errors(), "{:?}", skate.findings());
    assert_closed(skate.mesh());
    let [plate, skate_top] = widths(&skate);
    assert!((skate_top - top).abs() < 1e-6);
    assert!((top - plate - 2.0).abs() < 1e-6, "{plate} {top}");
    // The shape is written only for a skate: a slab's document reads as before.
    let written = String::from_utf8(doc.to_json()).unwrap();
    assert!(written.contains(r#""shape":"skate""#), "{written}");
    let slab_doc = raft("", 1.0);
    assert!(!String::from_utf8(slab_doc.to_json())
        .unwrap()
        .contains(r#""shape""#));
    assert_eq!(
        SupportDocument::from_json(&doc.to_json()).unwrap().raft,
        doc.raft
    );
    // 2.5 mm thick at 45°: under a 2 mm fillet with a 1 mm margin nothing is left at the plate,
    // and the build fails saying so rather than turning the raft inside out.
    let thick = raft(r#","shape":"skate""#, 2.5);
    assert!(!thick.raft.unwrap().stands_on_the_plate());
    assert!(raft("", 2.5).raft.unwrap().stands_on_the_plate());
    let failed = build_supports(
        &thick,
        SupportSurface {
            mesh: &mesh,
            interior_from: None,
        },
        &[],
        volume(),
        SupportOptions::default(),
        &mut Cancellation::never(),
    );
    match failed {
        Err(SupportError::Geometry(reason)) => assert!(reason.contains("skate"), "{reason}"),
        other => panic!("{:?}", other.map(|s| s.findings().len())),
    }
    // An unknown shape is refused.
    let json = format!(
        r#"{{"format":"karasu-supports","version":1,"raft":{{"thickness_mm":1,"margin_mm":1,"slope_deg":45,"shape":"wave"}},{feet}}}"#
    );
    assert!(SupportDocument::from_json(json.as_bytes()).is_err());
}

#[test]
fn a_web_raft_is_a_disc_under_each_foot_and_bars_between_neighbours() {
    let mesh = boxes(&[([0.0, 0.0, 8.0], [60.0, 20.0, 13.0], false)]);
    // Three feet in a triangle and one far away, beyond the longest bar.
    let feet = r#""supports":[{"id":1,"tip":{"point":[5,5,8]},"base":{"vertical":true}},{"id":2,"tip":{"point":[15,5,8]},"base":{"vertical":true}},{"id":3,"tip":{"point":[10,14,8]},"base":{"vertical":true}},{"id":4,"tip":{"point":[50,15,8]},"base":{"vertical":true}}]"#;
    let doc = document(&format!(
        r#""raft":{{"thickness_mm":1,"margin_mm":1,"slope_deg":60,"shape":"web","bar_width_mm":1,"bar_height_mm":0.8,"bar_longest_mm":20}},{feet}"#
    ));
    let raft = doc.raft.unwrap();
    assert_eq!(raft.shape, RaftShape::Web);
    assert_eq!(
        raft.bars,
        RaftBars {
            width_mm: 1.0,
            height_mm: 0.8,
            longest_mm: 20.0
        }
    );
    let scene = build(&doc, &mesh, None, &[]);
    assert!(!scene.has_errors(), "{:?}", scene.findings());
    // Four discs and the triangle's three bars: two pieces, the far foot alone.
    let rafts = scene
        .part_ranges()
        .iter()
        .filter(|r| r.part == SupportPart::Raft)
        .count();
    assert_eq!(rafts, 4 + 3);
    assert_eq!(scene.rafts(), 2);
    assert!(scene.raft());
    // Every raft part is a closed shell; the bars stand on the plate, 0.8 mm high.
    for range in scene
        .part_ranges()
        .iter()
        .filter(|r| r.part == SupportPart::Raft)
    {
        // Each edge once each way: closed and consistently wound.
        let mut edges = std::collections::BTreeMap::new();
        for t in &scene.mesh().triangles()[range.triangles.clone()] {
            for k in 0..3 {
                let (a, b) = (t[k], t[(k + 1) % 3]);
                *edges.entry((a.min(b), a.max(b))).or_insert(0) += if a < b { 1 } else { -1 };
            }
        }
        assert!(edges.values().all(|&n| n == 0));
    }
    let zs: Vec<f64> = scene
        .part_ranges()
        .iter()
        .filter(|r| r.part == SupportPart::Raft)
        .flat_map(|r| {
            scene.mesh().triangles()[r.triangles.clone()]
                .iter()
                .flatten()
        })
        .map(|&v| scene.mesh().vertices()[v as usize].z)
        .collect();
    assert!(zs.iter().all(|z| (-1e-9..=1.0 + 1e-9).contains(z)));
    assert!(zs.iter().any(|&z| (z - 0.8).abs() < 1e-9));
    // The bars' values are written for a web only and read back.
    let written = String::from_utf8(doc.to_json()).unwrap();
    assert!(
        written.contains(r#""shape":"web""#) && written.contains(r#""bar_longest_mm":20"#),
        "{written}"
    );
    assert_eq!(
        SupportDocument::from_json(&doc.to_json()).unwrap().raft,
        doc.raft
    );
    // Bars shorter than the feet's spacing: no bar, each foot a piece of its own.
    let short = document(&format!(
        r#""raft":{{"thickness_mm":1,"margin_mm":1,"slope_deg":60,"shape":"web","bar_longest_mm":5}},{feet}"#
    ));
    let scene = build(&short, &mesh, None, &[]);
    assert_eq!(scene.rafts(), 4);
    // Out of range: refused.
    for wrong in [
        r#""bar_width_mm":0.1"#,
        r#""bar_height_mm":6"#,
        r#""bar_longest_mm":0.5"#,
    ] {
        let json = format!(
            r#"{{"format":"karasu-supports","version":1,"raft":{{"thickness_mm":1,"margin_mm":1,"slope_deg":60,"shape":"web",{wrong}}},{feet}}}"#
        );
        assert!(
            SupportDocument::from_json(json.as_bytes()).is_err(),
            "{wrong}"
        );
    }
}

#[test]
fn contacts_fitted_to_the_surface_narrow_only_where_their_footprint_needs_it() {
    // Heavy contacts (0.6 mm) under the slab: one in the open, one 0.23 mm from its edge at
    // X 20, one 0.05 mm from it, and one near the edge that may not narrow at all.
    let doc = document(
        r#""preset":"heavy","defaults":{"contact_fit_min_mm":0.2},
        "supports":[{"id":1,"tip":{"point":[10,10,5]},"base":{"vertical":true}},
                    {"id":2,"tip":{"point":[19.77,10,5]},"base":{"vertical":true}},
                    {"id":3,"tip":{"point":[19.95,4,5]},"base":{"vertical":true}},
                    {"id":4,"tip":{"point":[19.77,16,5]},"base":{"vertical":true},"parameters":{"contact_fit_min_mm":0.6}}]"#,
    );
    let scene = build(&doc, &slab(), None, &[]);
    let diameters: Vec<f64> = scene
        .supports()
        .iter()
        .map(|s| s.contact_diameter_mm.unwrap())
        .collect();
    for (built, expected) in diameters.iter().zip([0.6, 0.45, 0.2, 0.6]) {
        assert!((built - expected).abs() < 1e-9, "{diameters:?}");
    }
    // Those with no room at their smallest are reported, as a warning on the contact: the one
    // whose smallest is its own diameter too.
    assert_eq!(
        kinds(&scene, Severity::Warning),
        [(3, FindingKind::ContactFit), (4, FindingKind::ContactFit)]
    );
    assert_eq!(kinds(&scene, Severity::Error), []);
    assert_closed(scene.mesh());
    // The smallest is at most the contact, and in range.
    let wrap = |parameters: &str| {
        format!(
            r#"{{"format":"karasu-supports","version":1,"supports":[{{"id":1,"tip":{{"point":[0,0,1]}},"base":{{"vertical":true}},"parameters":{parameters}}}]}}"#
        )
    };
    for (parameters, expected) in [
        (r#"{"contact_fit_min_mm":0.6}"#, "must not exceed"),
        (r#"{"contact_fit_min_mm":0.01}"#, "contact_fit_min_mm"),
    ] {
        let error = SupportDocument::from_json(wrap(parameters).as_bytes())
            .unwrap_err()
            .to_string();
        assert!(error.contains(expected), "{error}");
    }
    let doc = SupportDocument::from_json(wrap(r#"{"contact_fit_min_mm":0.1}"#).as_bytes()).unwrap();
    assert_eq!(doc.warnings.len(), 1, "below 0.2 mm is a soft range");
}

#[test]
fn documents_resolve_parameters_and_reject_invalid_input() {
    let doc = document(
        r#""preset":"heavy","defaults":{"contact_diameter_mm":0.5},
        "supports":[{"id":1,"tip":{"point":[0,0,1]},"base":{"vertical":true}},
                    {"id":2,"tip":{"point":[0,0,1]},"base":{"vertical":true},"parameters":{"contact_diameter_mm":0.25,"sides":8}}]"#,
    );
    let heavy = Preset::Heavy.parameters();
    assert_eq!(doc.supports[0].parameters.contact_diameter_mm, 0.5);
    assert_eq!(
        doc.supports[0].parameters.shaft_diameter_mm,
        heavy.shaft_diameter_mm
    );
    assert_eq!(doc.supports[1].parameters.contact_diameter_mm, 0.25);
    assert_eq!(
        (
            doc.supports[0].parameters.sides,
            doc.supports[1].parameters.sides
        ),
        (16, 8)
    );
    // Soft range: a 0.1 mm contact warns.
    let doc = document(
        r#""supports":[{"id":1,"tip":{"point":[0,0,1]},"base":{"vertical":true},"parameters":{"contact_diameter_mm":0.1}}]"#,
    );
    assert_eq!(doc.warnings.len(), 1);
    // Parents come before their children whatever the document order.
    let doc = document(
        r#""supports":[{"id":2,"tip":{"point":[0,0,1]},"base":{"support":1,"z_mm":0.5}},{"id":1,"tip":{"point":[1,0,1]},"base":{"vertical":true}}]"#,
    );
    assert_eq!(
        doc.supports.iter().map(|s| s.id).collect::<Vec<_>>(),
        [1, 2]
    );
    let wrap = |body: &str| format!(r#"{{"format":"karasu-supports","version":1,{body}}}"#);
    for (body, expected) in [
        (r#""supports":[],"extra":1"#, "unknown field"),
        (
            r#""supports":[{"id":1,"tip":{"point":[0,0,1]},"base":{"vertical":true},"parameters":{"tip_length_mm":40}}]"#,
            "tip_length_mm",
        ),
        (
            r#""supports":[{"id":1,"tip":{"point":[0,0,1]},"base":{"vertical":true},"parameters":{"contact_diameter_mm":2}}]"#,
            "must not exceed",
        ),
        (
            r#""supports":[{"id":1,"tip":{"point":[0,0,1]},"base":{"vertical":true}},{"id":1,"tip":{"point":[0,0,1]},"base":{"vertical":true}}]"#,
            "unique",
        ),
        (
            r#""supports":[{"id":1,"tip":{"point":[0,0,1]},"base":{"support":5,"z_mm":0}}]"#,
            "unknown support 5",
        ),
        (
            r#""supports":[{"id":1,"tip":{"point":[0,0,1]},"base":{"support":2,"z_mm":0}},{"id":2,"tip":{"point":[0,0,1]},"base":{"support":1,"z_mm":0}}]"#,
            "cycle",
        ),
        (
            r#""supports":[{"id":1,"tip":{"point":[0,0,1]},"base":{"vertical":true}}],"braces":[{"id":2,"from":{"support":1,"z_mm":0},"to":{"support":3,"z_mm":0},"diameter_mm":1}]"#,
            "unknown support 3",
        ),
        (
            r#""supports":[{"id":1,"tip":{"point":[0,0,1]},"base":{"vertical":false}}]"#,
            "must be true",
        ),
        (
            r#""supports":[{"id":1,"tip":{"point":[0,0,1],"axis":[0,0,0]},"base":{"vertical":true}}]"#,
            "nonzero",
        ),
    ] {
        let error = SupportDocument::from_json(wrap(body).as_bytes())
            .unwrap_err()
            .to_string();
        assert!(error.contains(expected), "{body}: {error}");
    }
    let wrong =
        SupportDocument::from_json(br#"{"format":"other","version":1,"supports":[]}"#).unwrap_err();
    assert!(matches!(wrong, SupportError::Invalid { entity: 0, .. }));
}

#[test]
fn bindings_record_the_pose_the_supports_were_placed_for() {
    let doc = document(
        r#""binding":{"translation_mm":[1,2,3],"rotate_deg":[0,0,90],"mirror":"x"},
           "supports":[{"id":1,"tip":{"point":[5,5,5]},"base":{"vertical":true}}]"#,
    );
    let binding = doc.binding.unwrap();
    assert_eq!(binding.translation_mm, Some([1.0, 2.0, 3.0]));
    assert_eq!(
        binding.pose,
        Some(BoundPose {
            rotate_deg: [0.0, 0.0, 90.0],
            scale: [1.0; 3],
            mirror: [true, false, false],
        })
    );
    // A translation alone keeps the exact-placement binding.
    let legacy = document(
        r#""binding":{"translation_mm":[1,2,3]},
           "supports":[{"id":1,"tip":{"point":[5,5,5]},"base":{"vertical":true}}]"#,
    );
    assert_eq!(legacy.binding.unwrap().pose, None);
    for (binding, reason) in [
        (r#"{"rotate_deg":[0,0,90]}"#, "also needs translation_mm"),
        (r#"{"translation_mm":[0,0,0],"scale":[1,0,1]}"#, "positive"),
        (
            r#"{"translation_mm":[0,0,0],"mirror":"xx"}"#,
            "distinct axes",
        ),
    ] {
        let error = SupportDocument::from_json(
            format!(
                r#"{{"format":"karasu-supports","version":1,"binding":{binding},
                    "supports":[{{"id":1,"tip":{{"point":[5,5,5]}},"base":{{"vertical":true}}}}]}}"#
            )
            .as_bytes(),
        )
        .unwrap_err();
        assert!(error.to_string().contains(reason), "{error}");
    }
}

#[test]
fn documents_move_rigidly_with_their_model_on_the_plate() {
    let doc = document(
        r#""binding":{"translation_mm":[0,0,0],"rotate_deg":[0,0,0]},
           "supports":[
            {"id":1,"tip":{"point":[5,5,5],"axis":[0,1,-1]},"base":{"vertical":true}},
            {"id":2,"tip":{"point":[15,5,5]},"bends":[[15,3,2]],"base":{"platform":[17,3]}},
            {"id":3,"tip":{"point":[10,15,5]},"base":{"support":1,"z_mm":2}}],
           "braces":[{"id":9,"from":{"support":1,"z_mm":1.5},"to":{"support":2,"z_mm":1.5},
                      "diameter_mm":0.8}]"#,
    );
    // A quarter turn about the slab's centre (10, 10), then 30 mm along X.
    let motion = PlanarMotion {
        turn_deg: 90.0,
        from_mm: [10.0, 10.0],
        to_mm: [40.0, 10.0],
    };
    assert!(!motion.is_identity());
    let moved = doc.moved(&motion);
    assert_eq!(moved.binding, None);
    let s = &moved.supports;
    // Whole quarter turns are exact.
    assert_eq!(s[0].tip_mm, [45.0, 5.0, 5.0]);
    assert_eq!(s[0].tip_axis, AxisSpec::Direction([-1.0, 0.0, -1.0]));
    assert_eq!(s[0].base, BaseSpec::Vertical);
    assert_eq!(s[1].tip_mm, [45.0, 15.0, 5.0]);
    assert_eq!(s[1].bends, vec![[47.0, 15.0, 2.0]]);
    assert_eq!(s[1].base, BaseSpec::Platform([47.0, 17.0]));
    assert_eq!(s[2].base, doc.supports[2].base);
    assert_eq!(moved.braces, doc.braces);
    assert_eq!(moved.supports[0].parameters, doc.supports[0].parameters);
    // The moved document builds on the moved slab exactly as the original on the original.
    let original = build(&doc, &slab(), None, &[]);
    let turned = build(
        &moved,
        &boxes(&[([30.0, 0.0, 5.0], [50.0, 20.0, 10.0], false)]),
        None,
        &[],
    );
    assert!(!original.has_errors(), "{:?}", original.findings());
    assert!(!turned.has_errors(), "{:?}", turned.findings());
    assert_eq!(turned.parts(), original.parts());
    assert_eq!(turned.braces(), original.braces());
    assert_eq!(
        turned.mesh().vertices().len(),
        original.mesh().vertices().len()
    );
    // The identity moves nothing.
    let still = PlanarMotion {
        turn_deg: 0.0,
        from_mm: [3.0, 4.0],
        to_mm: [3.0, 4.0],
    };
    assert!(still.is_identity());
    assert_eq!(doc.moved(&still).supports, doc.supports);
    // Other turns rotate exactly enough to keep heights and lengths.
    let skew = PlanarMotion {
        turn_deg: 30.0,
        ..motion
    };
    let p = skew.point([5.0, 5.0, 5.0]);
    assert_eq!(p[2], 5.0);
    assert!(((p[0] - 40.0).hypot(p[1] - 10.0) - 50f64.sqrt()).abs() < 1e-12);
}

#[test]
fn documents_in_a_model_frame_are_placed_with_their_model() {
    // The slab of `slab()` in its model frame (Z 0 to 5), mirrored in X and raised 5 mm by its
    // placement.
    let doc = document(
        r#""supports":[
            {"id":1,"tip":{"point":[5,5,0],"axis":[1,0,-1]},"base":{"vertical":true}},
            {"id":2,"tip":{"point":[15,5,0]},"bends":[[15,3,-3]],"base":{"platform":[17,3]}},
            {"id":3,"tip":{"point":[10,15,0]},"base":{"support":1,"z_mm":2}}],
           "braces":[{"id":9,"from":{"support":1,"z_mm":1.5},"to":{"support":2,"z_mm":1.5},
                      "diameter_mm":0.8}]"#,
    );
    let placement = Affine3::from_parts(
        [[-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        Vector3::new(20.0, 0.0, 5.0),
    )
    .unwrap();
    let placed = doc.placed(&placement).unwrap();
    assert_eq!(placed.binding, None);
    let s = &placed.supports;
    assert_eq!(s[0].tip_mm, [15.0, 5.0, 5.0]);
    assert_eq!(s[0].tip_axis, AxisSpec::Direction([-1.0, 0.0, -1.0]));
    assert_eq!(s[0].base, BaseSpec::Vertical);
    assert_eq!(s[1].tip_mm, [5.0, 5.0, 5.0]);
    assert_eq!(s[1].bends, vec![[5.0, 3.0, 2.0]]);
    // A foot on the plate is the model frame's point (x, y, 0), carried and set on the plate.
    assert_eq!(s[1].base, BaseSpec::Platform([3.0, 3.0]));
    // Heights on shafts, braces and parameters are world values.
    assert_eq!(s[2].base, doc.supports[2].base);
    assert_eq!(placed.braces, doc.braces);
    assert_eq!(placed.supports[0].parameters, doc.supports[0].parameters);
    let scene = build(&placed, &slab(), None, &[]);
    assert!(!scene.has_errors(), "{:?}", scene.findings());
    assert_eq!(scene.braces(), 1);
    // Every part's triangles, in mesh order, cover the mesh: one support can be drawn apart.
    let ranges = scene.part_ranges();
    assert_eq!(ranges.len(), scene.parts());
    assert_eq!(ranges[0].triangles.start, 0);
    assert!(ranges
        .windows(2)
        .all(|w| w[0].triangles.end == w[1].triangles.start));
    assert_eq!(
        ranges.last().unwrap().triangles.end,
        scene.mesh().triangles().len()
    );
    let owners: std::collections::BTreeSet<u64> = ranges.iter().map(|r| r.entity).collect();
    assert_eq!(owners, [1, 2, 3, 9].into());
    // The identity places nothing elsewhere.
    assert_eq!(
        doc.placed(&Affine3::identity()).unwrap().supports,
        doc.supports
    );
    // A placement that cannot carry a point names the support.
    let huge = Affine3::from_uniform_scale(1e308).unwrap();
    let error = doc.placed(&huge).unwrap_err().to_string();
    assert!(error.contains("support 1"), "{error}");
}

#[test]
fn documents_built_in_code_are_checked_as_documents_read() {
    let read = document(
        r#""supports":[
            {"id":2,"tip":{"point":[5,5,5]},"base":{"support":1,"z_mm":2}},
            {"id":1,"tip":{"point":[15,5,5]},"base":{"vertical":true},
             "parameters":{"tip_length_mm":0.5}}],
           "braces":[{"id":9,"from":{"support":1,"z_mm":1.5},"to":{"support":2,"z_mm":1.5},
                      "diameter_mm":0.8}]"#,
    );
    // The same document, built in code from an empty one, in the order an editor keeps.
    let mut built = SupportDocument::new(Preset::Medium);
    built.supports = read.supports.iter().rev().cloned().collect();
    built.braces = read.braces.clone();
    let checked = built.clone().validated().unwrap();
    assert_eq!(checked.supports, read.supports, "parents come first");
    assert_eq!(
        checked.warnings, read.warnings,
        "a short tip is a soft range"
    );
    assert_eq!(checked.clearance_mm, read.clearance_mm);
    // What reading refuses, checking refuses.
    let refused = |edit: &dyn Fn(&mut SupportDocument)| {
        let mut doc = built.clone();
        edit(&mut doc);
        doc.validated().unwrap_err().to_string()
    };
    assert!(refused(&|d| d.supports[1].id = 2).contains("unique"));
    assert!(refused(&|d| d.supports[0].parameters.contact_diameter_mm = 9.0).contains("contact"));
    assert!(refused(&|d| d.supports[0].tip_mm[0] = f64::NAN).contains("finite"));
    assert!(refused(&|d| d.supports[1].base = BaseSpec::Support {
        parent: 2,
        z_mm: 1.0
    })
    .contains("cycle"));
    assert!(refused(&|d| d.braces[0].to.0 = 7).contains("unknown support 7"));
    assert!(refused(&|d| d.clearance_mm = 9.0).contains("clearance"));
    assert!(
        refused(&|d| d.supports[0].tip_axis = AxisSpec::Direction([0.0; 3])).contains("nonzero")
    );
}

#[test]
fn routes_go_straight_aside_or_onto_the_model_below() {
    use karasu_core::supports::{find_route, Route, RouteQuery, SurfaceGrid, TILT_STEP_DEG};
    // A slab from Z 10 to 12 over a block from Z 4 to 6 under its left part.
    let model = boxes(&[
        ([0.0, 0.0, 10.0], [30.0, 20.0, 12.0], false),
        ([0.0, 0.0, 4.0], [8.0, 20.0, 6.0], false),
    ]);
    let grid = SurfaceGrid::build(&model, &mut Cancellation::never()).unwrap();
    assert!(grid.fits(&model));
    // Straight up from under the slab: its underside first, facing down; under the block, the
    // block's underside; past the slab, nothing.
    let (hit, normal, _) = grid.first_above(&model, [20.0, 10.0, 7.0], 30.0).unwrap();
    assert!((hit[2] - 10.0).abs() < 1e-9 && normal[2] < -0.99, "{hit:?}");
    let (hit, _, _) = grid.first_above(&model, [4.0, 10.0, 1.0], 30.0).unwrap();
    assert!((hit[2] - 4.0).abs() < 1e-9, "{hit:?}");
    assert!(grid.first_above(&model, [20.0, 10.0, 13.0], 30.0).is_none());
    // A capsule under the slab keeps clear of it; one reaching into it does not.
    assert!(grid.clear(&model, [20.0, 10.0, 7.0], [25.0, 10.0, 8.0], 0.5));
    assert!(!grid.clear(&model, [20.0, 10.0, 7.0], [25.0, 10.0, 9.8], 0.5));
    let cone = TipCone {
        from_mm: -0.2,
        radii_mm: [0.2, 0.5],
        length_mm: 2.5,
        footprint_mm: 0.2,
        margin_mm: 0.1,
        allowance_mm: 0.2,
        sides: 16,
    };
    let query = |x: f64| RouteQuery {
        tip_mm: [x, 10.0, 10.0],
        normal: [0.0, 0.0, -1.0],
        tip: cone,
        radius_mm: 0.8,
        floor_mm: 0.0,
        foot_length_mm: 2.0,
        foot_radius_mm: 0.2,
        max_axis_deviation_deg: 60.0,
        towards_shaft: false,
    };
    // Clear below: straight down.
    assert_eq!(find_route(&model, &grid, &query(20.0)), Route::Straight);
    // 1 mm inside the block's edge (owner, 2026-11-04): the tip turns, within the axis limit and
    // by whole steps, until its shaft straight down clears the block by the radius.
    let Route::Tilted(axis) = find_route(&model, &grid, &query(7.0)) else {
        panic!("no tilted tip");
    };
    let join = [
        7.0 + 2.5 * axis[0],
        10.0 + 2.5 * axis[1],
        10.0 + 2.5 * axis[2],
    ];
    assert!(join[0] >= 8.0 + 0.8, "{join:?}");
    let turned = (-axis[2]).acos().to_degrees();
    assert!(turned <= 60.0 + 1e-9, "{turned}");
    assert!(
        (turned / TILT_STEP_DEG - (turned / TILT_STEP_DEG).round()).abs() < 1e-9,
        "{turned}"
    );
    // Deterministic.
    assert_eq!(find_route(&model, &grid, &query(7.0)), Route::Tilted(axis));
    // 2 mm inside it, no tilt within the limit reaches past it: the nearest column beside it.
    let Route::Aside(knee) = find_route(&model, &grid, &query(6.0)) else {
        panic!("no column aside");
    };
    assert!(knee[0] >= 8.0 + 0.8, "{knee:?}");
    let slant = ((knee[0] - 6.0).hypot(knee[1] - 10.0)).atan2(10.0 - knee[2]);
    assert!(slant.to_degrees() <= 45.0 + 1e-9, "{knee:?}");
    // Over a wide block's middle, no column within 45° reaches the plate clear: the foot
    // stands on the block's top, facing up, with room for the tip and the foot between.
    let wide = boxes(&[
        ([0.0, 0.0, 10.0], [30.0, 20.0, 12.0], false),
        ([0.0, 0.0, 3.0], [30.0, 20.0, 4.0], false),
    ]);
    let grid = SurfaceGrid::build(&wide, &mut Cancellation::never()).unwrap();
    match find_route(&wide, &grid, &query(15.0)) {
        Route::OnModel {
            point_mm,
            normal,
            axis,
        } => {
            assert!(
                (point_mm[2] - 4.0).abs() < 1e-9 && normal[2] > 0.99,
                "{point_mm:?}"
            );
            assert_eq!(axis, [0.0, 0.0, 1.0]);
        }
        other => panic!("{other:?}"),
    }
    // TD-124: a tip 0.1 mm beside a bulge hanging 1 mm under the slab. Straight down, its
    // cone would run into the bulge's side deeper than its penetration, though the shaft below
    // is clear: the tip leans away from it instead.
    let crease = boxes(&[
        ([0.0, 0.0, 10.0], [30.0, 20.0, 12.0], false),
        ([6.0, 0.0, 9.0], [10.0, 20.0, 10.0], false),
    ]);
    let grid = SurfaceGrid::build(&crease, &mut Cancellation::never()).unwrap();
    let Route::Tilted(axis) = find_route(&crease, &grid, &query(5.9)) else {
        panic!("the tip went through the bulge");
    };
    assert!(axis[0] < 0.0, "{axis:?}");
    // Far from it, straight down as before.
    assert_eq!(find_route(&crease, &grid, &query(3.0)), Route::Straight);
}

#[test]
fn a_tip_towards_its_shaft_is_routed_as_it_is_built() {
    use karasu_core::supports::{find_route, Route, RouteQuery, SurfaceGrid};
    // TD-129: a tip pointing towards its shaft is built straight down when the shaft goes
    // straight down below it, so the route checks that tip, within the axis limit.
    let cone = TipCone {
        from_mm: -0.2,
        radii_mm: [0.2, 0.5],
        length_mm: 2.5,
        footprint_mm: 0.2,
        margin_mm: 0.1,
        allowance_mm: 0.2,
        sides: 16,
    };
    let query = |tip_mm: [f64; 3], normal: [f64; 3], towards_shaft: bool| RouteQuery {
        tip_mm,
        normal,
        tip: cone,
        radius_mm: 0.8,
        floor_mm: 0.0,
        foot_length_mm: 2.0,
        foot_radius_mm: 0.2,
        max_axis_deviation_deg: 60.0,
        towards_shaft,
    };
    let model = slab();
    let grid = SurfaceGrid::build(&model, &mut Cancellation::never()).unwrap();
    // Under the slab, straight down either way.
    for towards in [false, true] {
        let under = query([10.0, 10.0, 5.0], [0.0, 0.0, -1.0], towards);
        assert_eq!(find_route(&model, &grid, &under), Route::Straight);
    }
    // On its side, which faces +X: along the normal the join is clear of the slab, so straight
    // down from it. Pointing straight down, the tip would be 90° from the normal: it turns
    // instead, within the limit.
    let side = |towards| query([20.0, 10.0, 7.5], [1.0, 0.0, 0.0], towards);
    assert_eq!(find_route(&model, &grid, &side(false)), Route::Straight);
    let Route::Tilted(axis) = find_route(&model, &grid, &side(true)) else {
        panic!("a tip straight down off a side");
    };
    assert!(axis[0] >= 60f64.to_radians().cos() - 1e-9, "{axis:?}");
    // Over a wide block's middle, the foot stands on its top straight below the tip, where a tip
    // straight down points.
    let wide = boxes(&[
        ([0.0, 0.0, 10.0], [30.0, 20.0, 12.0], false),
        ([0.0, 0.0, 3.0], [30.0, 20.0, 4.0], false),
    ]);
    let grid = SurfaceGrid::build(&wide, &mut Cancellation::never()).unwrap();
    let over = query([15.0, 10.0, 10.0], [0.0, 0.0, -1.0], true);
    let Route::OnModel { point_mm, .. } = find_route(&wide, &grid, &over) else {
        panic!("no foot on the block");
    };
    assert_eq!(point_mm, [15.0, 10.0, 4.0]);
}

#[test]
fn a_document_written_reads_back_equal() {
    // TD-130: every kind of base, axis, origin and kind, overrides on top of a preset and of
    // the document's defaults, a raft by groups, braces with and without a diameter, and a
    // binding with a pose.
    let doc = document(
        r#""binding":{"model_sha256":"00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff",
            "translation_mm":[1.5,-2,3],"rotate_deg":[0,12.5,90],"scale":[1,2,0.5],"mirror":"xz"},
        "preset":"light","defaults":{"tip_length_mm":2.2,"sides":12},
        "clearance_mm":0.4,"anchor_tolerance_mm":0.07,"max_axis_deviation_deg":55,
        "raft":{"thickness_mm":1.2,"margin_mm":2,"slope_deg":60,"fillet_height_mm":0.5,"groups_join_mm":8},
        "supports":[
            {"id":1,"tip":{"point":[10,15,8],"axis":"straight"},"base":{"vertical":true},
             "parameters":{"contact_shape":"sphere","contact_fit_min_mm":0.2,"shaft_bottom_diameter_mm":1.6}},
            {"id":2,"tip":{"point":[5,12,8],"axis":[0.1,-0.2,-1]},"bends":[[5,8,6.5]],
             "base":{"model":[5,5,4],"axis":"normal"},"origin":"islands"},
            {"id":3,"tip":{"point":[15,18,8]},"base":{"support":1,"z_mm":3},"origin":"automatic"},
            {"id":4,"tip":{"point":[2,3,8]},"base":{"platform":[2.5,3.5]},"accept_collision":true},
            {"id":5,"tip":{"point":[3,3,8]},"base":{"model":[3,3,4]},"kind":"mini"}
        ],
        "braces":[{"id":9,"from":{"support":1,"z_mm":4},"to":{"support":3,"z_mm":4.5},"diameter_mm":0.6},
            {"id":10,"from":{"support":1,"z_mm":5},"to":{"support":3,"z_mm":5.5}}]"#,
    );
    let written = doc.to_json();
    let read = SupportDocument::from_json(&written).unwrap();
    assert_eq!(read, doc);
    // Deterministic, and a document built in code writes the same as the one read.
    assert_eq!(read.to_json(), written);
    assert_eq!(doc.clone().validated().unwrap().to_json(), written);
}

#[test]
fn a_support_says_what_placed_it() {
    use karasu_core::supports::SupportOrigin;
    // Unsaid, placed; the Islands tool's, "islands"; anything else is refused.
    let doc = document(
        r#""supports":[
            {"id":1,"tip":{"point":[0,0,10]},"base":{"vertical":true}},
            {"id":2,"tip":{"point":[5,0,10]},"base":{"vertical":true},"origin":"islands"}]"#,
    );
    assert_eq!(doc.supports[0].origin, SupportOrigin::Placed);
    assert_eq!(doc.supports[1].origin, SupportOrigin::Islands);
    assert!(SupportDocument::from_json(
        br#"{"format":"karasu-supports","version":1,"supports":[
            {"id":1,"tip":{"point":[0,0,10]},"base":{"vertical":true},"origin":"robot"}]}"#
    )
    .is_err());
}

/// The overhang above a lower step, as in [`feet_on_the_model_on_supports_and_inside_cavities`]:
/// the step's top at Z 4 for Y below 10, the overhang's underside at Z 8 for Y above 10.
fn step() -> TriangleMesh {
    boxes(&[
        ([0.0, 0.0, 0.0], [20.0, 10.0, 4.0], false),
        ([0.0, 10.0, 8.0], [20.0, 20.0, 12.0], false),
    ])
}

/// The lowest Z of entity `id`'s parts in `scene`.
fn lowest(scene: &SupportScene, id: u64) -> f64 {
    let vertices = scene.mesh().vertices();
    scene
        .part_ranges()
        .iter()
        .filter(|r| r.entity == id)
        .flat_map(|r| {
            scene.mesh().triangles()[r.triangles.clone()]
                .iter()
                .flat_map(|t| t.map(|i| vertices[i as usize].z))
        })
        .fold(f64::INFINITY, f64::min)
}

#[test]
fn mini_supports_stand_on_the_model_or_a_shaft_with_a_ball_at_each_end() {
    let mesh = step();
    let doc = document(
        r#""supports":[
            {"id":1,"kind":"mini","tip":{"point":[5,11,8]},"base":{"model":[5,9,4]}},
            {"id":2,"kind":"mini","tip":{"point":[15,11,8]},"base":{"model":[15,9,4]},
             "parameters":{"mini_penetration_mm":0}},
            {"id":3,"tip":{"point":[10,15,8]},"base":{"vertical":true}},
            {"id":4,"kind":"mini","tip":{"point":[11,15.5,8]},"base":{"support":3,"z_mm":4}}
        ]"#,
    );
    let medium = Preset::Medium.parameters();
    assert_eq!(
        (medium.mini_diameter_mm, medium.mini_penetration_mm),
        (0.4, 0.1)
    );
    assert_eq!(doc.supports[0].kind, SupportKind::Mini);
    assert_eq!(doc.supports[2].kind, SupportKind::Standard);
    let scene = build(&doc, &mesh, None, &[]);
    assert!(!scene.has_errors(), "{:?}", scene.findings());
    assert_closed(scene.mesh());
    let bases: Vec<(u64, BaseKind)> = scene.supports().iter().map(|s| (s.id, s.base)).collect();
    assert_eq!(
        bases,
        [
            (1, BaseKind::Model),
            (2, BaseKind::Model),
            (3, BaseKind::Platform),
            (4, BaseKind::Support)
        ]
    );
    // No cone and no foot: balls and rods of one diameter, the balls reaching the penetration
    // below the step's top (to the tessellation's rounding at the ball's pole).
    assert!(
        (lowest(&scene, 1) - (4.0 - 0.1)).abs() < 1e-9,
        "{}",
        lowest(&scene, 1)
    );
    assert!(
        (lowest(&scene, 2) - 4.0).abs() < 1e-9,
        "{}",
        lowest(&scene, 2)
    );
    let mini = |id: u64| {
        scene
            .part_ranges()
            .iter()
            .filter(|r| r.entity == id)
            .map(|r| r.part)
            .collect::<Vec<_>>()
    };
    assert!(!mini(1).contains(&SupportPart::Cone) && !mini(1).contains(&SupportPart::Pad));
    assert!(mini(4).contains(&SupportPart::Junction));
    // Its path runs between the balls' centres, 0.2 − 0.1 mm out of each surface.
    let path = scene.shaft_path(1).unwrap();
    assert!((path[0][2] - 7.9).abs() < 1e-12 && (path[1][2] - 4.1).abs() < 1e-12);
    assert_eq!(scene.shaft_diameter(1, 6.0), Some(0.4));
    // Braces planned between neighbours leave the mini supports out.
    let planned = plan_braces(
        &document(
            r#""supports":[
                {"id":1,"tip":{"point":[5,15,8]},"base":{"vertical":true}},
                {"id":2,"tip":{"point":[8,15,8]},"base":{"vertical":true}},
                {"id":3,"kind":"mini","tip":{"point":[6.5,11,8]},"base":{"model":[6.5,9,4]}}
            ]"#,
        ),
        SupportSurface {
            mesh: &mesh,
            interior_from: None,
        },
        &[],
        &BracePlan {
            max_distance_mm: 10.0,
            lowest_mm: 1.0,
            step_mm: 2.0,
            pattern: BracePattern::Horizontal,
            diameter_mm: None,
            max_diameter_mm: None,
        },
        &mut Cancellation::never(),
    )
    .unwrap();
    assert_ne!(planned.len(), 0);
    assert!(planned
        .iter()
        .all(|p| [p.brace.from.0, p.brace.to.0] == [1, 2]));
}

#[test]
fn mini_supports_are_checked_as_supports_and_refused_on_the_plate() {
    let mesh = step();
    // A mini support may touch the model (owner, 2026-10-30): a block 0.05 mm beside its rod is
    // no finding at all; a thin wall its rod goes through is a warning, never an error.
    let crossing =
        r#""supports":[{"id":1,"kind":"mini","tip":{"point":[5,11,8]},"base":{"model":[5,9,4]}}]"#;
    let beside = boxes(&[
        ([0.0, 0.0, 0.0], [20.0, 10.0, 4.0], false),
        ([0.0, 10.0, 8.0], [20.0, 20.0, 12.0], false),
        ([5.25, 9.0, 4.5], [6.0, 11.0, 7.5], false),
    ]);
    let scene = build(&document(crossing), &beside, None, &[]);
    assert_eq!(scene.findings(), &[], "{:?}", scene.findings());
    let walled = boxes(&[
        ([0.0, 0.0, 0.0], [20.0, 10.0, 4.0], false),
        ([0.0, 10.0, 8.0], [20.0, 20.0, 12.0], false),
        ([0.0, 10.6, 7.3], [20.0, 10.9, 7.6], false),
    ]);
    let scene = build(&document(crossing), &walled, None, &[]);
    assert!(!scene.has_errors(), "{:?}", scene.findings());
    assert_eq!(
        kinds(&scene, Severity::Warning),
        [(1, FindingKind::Collision)]
    );
    assert!(scene.findings()[0].detail.contains("axis crosses"));
    // A foot above the tip holds nothing: no shape.
    let scene = build(
        &document(
            r#""supports":[{"id":1,"kind":"mini","tip":{"point":[5,15,8]},"base":{"model":[5,15,12]}}]"#,
        ),
        &mesh,
        None,
        &[],
    );
    assert_eq!(
        kinds(&scene, Severity::Error),
        [(1, FindingKind::InvalidPath)]
    );
    assert_eq!(scene.supports()[0].parts, 0);
    // Anchors are checked as a standard support's.
    let scene = build(
        &document(
            r#""supports":[{"id":1,"kind":"mini","tip":{"point":[5,11,8]},"base":{"model":[5,9,3]}}]"#,
        ),
        &mesh,
        None,
        &[],
    );
    assert_eq!(
        kinds(&scene, Severity::Error),
        [(1, FindingKind::AnchorOffSurface)]
    );
    let wrap = |body: &str| format!(r#"{{"format":"karasu-supports","version":1,{body}}}"#);
    for (body, expected) in [
        (
            r#""supports":[{"id":1,"kind":"mini","tip":{"point":[0,0,1]},"base":{"vertical":true}}]"#,
            "not on the plate",
        ),
        (
            r#""supports":[{"id":1,"kind":"mini","tip":{"point":[0,0,1]},"base":{"platform":[0,0]}}]"#,
            "not on the plate",
        ),
        (
            r#""supports":[{"id":1,"kind":"mini","tip":{"point":[0,0,1]},"base":{"model":[0,0,0]},"parameters":{"mini_penetration_mm":0.2}}]"#,
            "less than half",
        ),
        (
            r#""supports":[{"id":1,"kind":"tiny","tip":{"point":[0,0,1]},"base":{"model":[0,0,0]}}]"#,
            "unknown variant",
        ),
    ] {
        let error = SupportDocument::from_json(wrap(body).as_bytes())
            .unwrap_err()
            .to_string();
        assert!(error.contains(expected), "{body}: {error}");
    }
    // Built in code, the same.
    let mut doc = document(
        r#""supports":[{"id":1,"kind":"mini","tip":{"point":[5,11,8]},"base":{"model":[5,9,4]}}]"#,
    );
    doc.supports[0].base = BaseSpec::Vertical;
    assert!(doc.validated().is_err());
}

#[test]
fn a_mini_support_finds_its_foot_straight_below_its_tip() {
    use karasu_core::supports::{find_mini_foot, MiniQuery, SurfaceGrid};
    let query = |x: f64| MiniQuery {
        tip_mm: [x, 10.0, 10.0],
        normal: [0.0, 0.0, -1.0],
        diameter_mm: 0.4,
        penetration_mm: 0.1,
        floor_mm: 0.0,
        clear: false,
    };
    // Under a slab's middle, the lower slab's top.
    let slabs = boxes(&[
        ([0.0, 0.0, 10.0], [30.0, 20.0, 12.0], false),
        ([0.0, 0.0, 3.0], [20.0, 20.0, 4.0], false),
        ([15.3, 0.0, 6.0], [16.0, 20.0, 8.0], false),
    ]);
    let grid = SurfaceGrid::build(&slabs, &mut Cancellation::never()).unwrap();
    let (point, normal) = find_mini_foot(&slabs, &grid, &query(10.0)).unwrap();
    assert!(
        (point[2] - 4.0).abs() < 1e-9 && normal[2] > 0.99,
        "{point:?}"
    );
    // Built there, it has no error.
    let doc = document(&format!(
        r#""supports":[{{"id":1,"kind":"mini","tip":{{"point":[10,10,10]}},"base":{{"model":[{},{},{}]}}}}]"#,
        point[0], point[1], point[2]
    ));
    let scene = build(&doc, &slabs, None, &[]);
    assert!(!scene.has_errors(), "{:?}", scene.findings());
    // Beside a block nearer than the clearance, there too: a mini support may touch the model.
    let (beside, _) = find_mini_foot(&slabs, &grid, &query(15.0)).unwrap();
    assert!((beside[2] - 4.0).abs() < 1e-9, "{beside:?}");
    // Over nothing but the plate, none.
    assert_eq!(find_mini_foot(&slabs, &grid, &query(25.0)), None);
    // TD-126: 0.1 mm beside the block, its rod straight down touches it. By hand that stays;
    // placed by a tool, the rod turns until it keeps off the model, and stands elsewhere.
    let (touching, _) = find_mini_foot(&slabs, &grid, &query(15.2)).unwrap();
    assert!((touching[0] - 15.2).abs() < 1e-9, "{touching:?}");
    let straight = |x: f64| [[x, 10.0, 10.0 - 0.1], [x, 10.0, 4.0 + 0.1]];
    assert!(!grid.rod_clear(&slabs, &straight(15.2), 0.2));
    let clear = |x: f64| MiniQuery {
        clear: true,
        ..query(x)
    };
    let (point, normal) = find_mini_foot(&slabs, &grid, &clear(15.2)).unwrap();
    assert!(
        normal[2] > 0.99 && (point[0] - 15.2).abs() > 1e-6,
        "{point:?}"
    );
    let rod = [[15.2, 10.0, 9.9], [point[0], point[1], point[2] + 0.1]];
    assert!(grid.rod_clear(&slabs, &rod, 0.2), "{point:?}");
    // Clear of anything, it goes straight down either way.
    assert_eq!(
        find_mini_foot(&slabs, &grid, &clear(10.0)),
        find_mini_foot(&slabs, &grid, &query(10.0))
    );
}

#[test]
fn a_drain_hole_removes_only_what_lies_strictly_inside_it() {
    // TD-131: a tip there has nothing to hold; one on the rim does.
    let hole = DrainCylinder {
        start_mm: [10.0, 10.0, 7.0],
        end_mm: [10.0, 10.0, 11.0],
        radius_mm: 1.0,
    };
    assert!(hole.removes([10.0, 10.0, 8.0]));
    assert!(hole.removes([10.9, 10.0, 8.0]));
    assert!(!hole.removes([11.0, 10.0, 8.0]), "the rim");
    assert!(!hole.removes([10.0, 10.0, 6.9]), "before its start");
    assert!(!hole.removes([10.0, 10.0, 11.1]), "past its end");
    let tilted = DrainCylinder {
        start_mm: [0.0; 3],
        end_mm: [4.0, 0.0, 4.0],
        radius_mm: 1.0,
    };
    assert!(tilted.removes([2.0, 0.0, 2.5]));
    assert!(!tilted.removes([2.0, 0.0, 3.5]));
    let degenerate = DrainCylinder {
        start_mm: [1.0; 3],
        end_mm: [1.0; 3],
        radius_mm: 1.0,
    };
    assert!(!degenerate.removes([1.0; 3]));
}

#[test]
fn a_model_foot_points_up_towards_its_shaft_within_the_axis_limit() {
    use karasu_core::supports::foot_axis;
    // TD-132: on a surface within the limit, straight up; beyond it, the normal turned up by the
    // limit less a half-degree margin; facing straight down, the normal.
    assert_eq!(foot_axis([0.6, 0.0, 0.8], 60.0), [0.0, 0.0, 1.0]);
    let steep = [68f64.to_radians().sin(), 0.0, 68f64.to_radians().cos()];
    let axis = foot_axis(steep, 60.0);
    let from_normal = (axis[0] * steep[0] + axis[2] * steep[2])
        .acos()
        .to_degrees();
    assert!((from_normal - 59.5).abs() < 1e-9, "{from_normal}");
    assert!((axis[2].acos().to_degrees() - 8.5).abs() < 1e-9, "{axis:?}");
    assert_eq!(foot_axis([0.0, 0.0, -1.0], 60.0), [0.0, 0.0, -1.0]);

    // A slab over a steep ramp facing +X and up, 68° from level. A support from under the slab
    // down to the ramp, its shaft twice as wide at its bottom as a merged trunk's may be: with
    // its foot along the normal, the ball at its shaft's bottom hangs outside the foot and
    // starts in mid-air; turned up towards the shaft, the foot holds it.
    let mut points = Vec::new();
    let mut triangles = Vec::new();
    push_box(
        [0.0, 0.0, 10.0],
        [30.0, 20.0, 12.0],
        false,
        &mut points,
        &mut triangles,
    );
    let base = u32::try_from(points.len()).unwrap();
    for y in [0.0, 20.0] {
        for [x, z] in [[14.0, 3.0], [16.0, 3.0], [14.0, 8.0]] {
            points.push(Point3::new(x, y, z));
        }
    }
    let [a0, b0, c0, a1, b1, c1] = [0, 1, 2, 3, 4, 5].map(|k| base + k);
    triangles.extend([
        [a0, b0, c0],
        [a1, c1, b1],
        [a0, b1, b0],
        [a0, a1, b1],
        [a0, c0, c1],
        [a0, c1, a1],
        [b0, b1, c1],
        [b0, c1, c0],
    ]);
    let mesh = TriangleMesh::try_new(points, triangles).unwrap();
    let point = [15.2, 10.0, 8.0 - 2.5 * 1.2];
    let normal = [5.0 / 29f64.sqrt(), 0.0, 2.0 / 29f64.sqrt()];
    let medium = Preset::Medium.parameters();
    let bottom = 2.0 * medium.shaft_diameter_mm;
    // Whether the lowest point of the ball at the foot's join lies inside the foot's frustum.
    let holds = |axis: [f64; 3]| {
        let join: Vec<f64> = (0..3)
            .map(|k| point[k] + axis[k] * medium.foot_length_mm)
            .collect();
        let low = [join[0], join[1], join[2] - bottom / 2.0];
        let rel: Vec<f64> = (0..3).map(|k| low[k] - point[k]).collect();
        let along: f64 = (0..3).map(|k| rel[k] * axis[k]).sum();
        let radial = (0..3)
            .map(|k| (rel[k] - along * axis[k]).powi(2))
            .sum::<f64>()
            .sqrt();
        let r0 = medium.foot_contact_diameter_mm / 2.0;
        let r1 = medium.foot_body_diameter_mm / 2.0;
        let radius = r0 + (r1 - r0) * (along / medium.foot_length_mm);
        (0.0..=medium.foot_length_mm).contains(&along) && radial <= radius
    };
    assert!(!holds(normal), "along the normal");
    let up = foot_axis(normal, 60.0);
    assert!(holds(up), "{up:?}");
    // Built that way with a shaft of one width, the support has no error: the foot is within
    // the axis limit. (So wide a bottom would come too close to the ramp: the build says so.)
    let doc = document(&format!(
        r#""preset":"medium","supports":[{{"id":1,"tip":{{"point":[15.2,10,10]}},
            "base":{{"model":[{},{},{}],"axis":[{},{},{}]}}}}]"#,
        point[0], point[1], point[2], up[0], up[1], up[2]
    ));
    let scene = build(&doc, &mesh, None, &[]);
    assert!(!scene.has_errors(), "{:?}", scene.findings());
}

#[test]
fn a_contact_s_width_at_the_surface_follows_its_cone_or_its_ball() {
    // TD-136: the presets' contacts, sunk 0.1 mm, at the surface.
    for (preset, width) in [
        (Preset::Light, 0.3 + 0.5 * 0.1 / 2.0),
        (Preset::Medium, 0.5 + 0.5 * 0.1 / 2.5),
        (Preset::Heavy, 0.6 + 0.4 * 0.1 / 2.5),
    ] {
        let p = preset.parameters();
        assert!((p.surface_width_mm() - width).abs() < 1e-12, "{preset:?}");
        assert!(p.surface_width_mm() > p.contact_diameter_mm);
    }
    // Sunk past the tip's length, the whole body; not sunk, the diameter itself.
    let medium = Preset::Medium.parameters();
    let deep = karasu_core::supports::SupportParameters {
        penetration_mm: 3.0,
        ..medium
    };
    assert!((deep.surface_width_mm() - medium.tip_body_diameter_mm).abs() < 1e-12);
    let flush = karasu_core::supports::SupportParameters {
        penetration_mm: 0.0,
        ..medium
    };
    assert!((flush.surface_width_mm() - medium.contact_diameter_mm).abs() < 1e-12);
    // A ball's is its footprint's.
    let ball = karasu_core::supports::SupportParameters {
        contact_shape: karasu_core::supports::ContactShape::Sphere,
        ..medium
    };
    assert!((ball.surface_width_mm() - 2.0 * ball.footprint_radius_mm()).abs() < 1e-12);
}

#[test]
#[ignore = "measures the raft shapes under 1600 feet: run with --release -- --ignored --nocapture"]
fn raft_shapes_under_many_feet_are_timed() {
    // A 40 × 40 grid of plate supports 2.3 mm apart, jittered, under a slab 100 mm wide.
    let mesh = boxes(&[([0.0, 0.0, 8.0], [104.0, 104.0, 13.0], false)]);
    let mut supports = Vec::new();
    for k in 0..1600_u32 {
        let jitter = f64::from((k * 7919) % 11) / 11.0 * 0.6;
        let x = 2.0 + f64::from(k % 40) * 2.3 + jitter;
        let y = 2.0 + f64::from(k / 40) * 2.3 - jitter / 2.0;
        supports.push(format!(
            r#"{{"id":{},"tip":{{"point":[{x},{y},8]}},"base":{{"vertical":true}}}}"#,
            k + 1
        ));
    }
    let feet = format!(r#""supports":[{}]"#, supports.join(","));
    for shape in ["slab", "skate", "web"] {
        let doc = document(&format!(
            r#""raft":{{"thickness_mm":1,"margin_mm":0.3,"slope_deg":60,"fillet_diameter_mm":1,"shape":"{shape}"}},{feet}"#
        ));
        let start = std::time::Instant::now();
        let scene = build(&doc, &mesh, None, &[]);
        let seconds = start.elapsed().as_secs_f64();
        let rafts = scene
            .part_ranges()
            .iter()
            .filter(|r| r.part == SupportPart::Raft)
            .count();
        eprintln!(
            "{shape}: {seconds:.3} s, {rafts} raft parts, {} pieces, {} triangles in all",
            scene.rafts(),
            scene.mesh().triangles().len()
        );
        assert!(
            !scene.has_errors(),
            "{:?}",
            scene.findings().iter().take(3).collect::<Vec<_>>()
        );
    }
}
