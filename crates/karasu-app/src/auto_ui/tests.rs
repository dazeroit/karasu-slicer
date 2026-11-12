use super::*;
use crate::islands_ui::plan::Skip;
use crate::supports_ui::tests::{built, ray};
use karasu_core::supports::BaseSpec;
use std::time::Duration;

/// An 8 mm cube lifted 6 mm above the plate on a preset printer, prepared: its bottom is one
/// island and one overhang, small enough for a debug build.
fn lifted_small_cube(name: &str) -> (Workspace, PreparationUi, std::path::PathBuf) {
    let folder = std::env::temp_dir().join(format!("karasu-auto-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    let model = folder.join("cube.stl");
    std::fs::write(&model, crate::workspace::tests::box_stl([0.0; 3], [8.0; 3])).unwrap();
    let (mut workspace, mut preparation) =
        crate::hollowing_ui::tests::placed(model, None, Duration::from_secs(60));
    // Centred on the plate, so its supports' pads stay within the build volume.
    workspace.edit(|plate| {
        plate.objects[0].pose.center = true;
        plate.objects[0].pose.drop = true;
        plate.objects[0].pose.lift_mm = Some(6.0);
    });
    crate::hollowing_ui::tests::prepared_within(
        &workspace,
        &mut preparation,
        Duration::from_secs(60),
    );
    (workspace, preparation, folder)
}

/// Generate, then poll the surface for routes and the plan until it is made, within five
/// minutes (a debug build slices slowly).
fn generated(
    ui: &mut AutoUi,
    supports: &mut SupportsUi,
    workspace: &Workspace,
    preparation: &PreparationUi,
) {
    ui.generate(0);
    supports.route_wanted = true;
    let started = Instant::now();
    while ui.planning() {
        supports.poll(workspace, preparation, Some(0));
        ui.poll(workspace, preparation, supports);
        assert!(started.elapsed() < Duration::from_secs(300), "no plan");
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(ui.planned.is_some(), "{:?}", ui.take_messages());
}

/// Model 0's supports' identities by origin: the automatic ones, and the others.
fn by_origin(workspace: &Workspace) -> (Vec<u64>, Vec<u64>) {
    let document = workspace.supports(0).expect("supports");
    let ids = |automatic: bool| -> Vec<u64> {
        document
            .supports
            .iter()
            .filter(|s| (s.origin == SupportOrigin::Automatic) == automatic)
            .map(|s| s.id)
            .collect()
    };
    (ids(true), ids(false))
}

#[test]
fn generated_supports_are_previewed_then_applied_as_one_step() {
    let (mut workspace, preparation, directory) = lifted_small_cube("applied");
    let mut ui = AutoUi::new();
    let mut supports = SupportsUi::new();
    generated(&mut ui, &mut supports, &workspace, &preparation);
    let (planned, current) = ui.plan_of(&workspace, Some(0)).unwrap();
    assert!(current);
    let outcome = &planned.outcome;
    // The bottom is held, all of it within the spacing of a tip, and nothing is left.
    assert_eq!(outcome.islands, [1, 1]);
    assert_eq!(
        outcome.overhangs[0], outcome.overhangs[1],
        "{:?}",
        outcome.left
    );
    assert!(outcome.left.is_empty(), "{:?}", outcome.left);
    let count = outcome.plan.supports.len();
    assert!(count >= 4, "{count} supports");
    assert!(planned.preview.is_some());
    // Applied: one undoable step, every support automatic.
    let revision = workspace.revision();
    assert_eq!(ui.apply(&mut workspace, 0), count);
    assert_ne!(workspace.revision(), revision);
    let (automatic, others) = by_origin(&workspace);
    assert_eq!((automatic.len(), others.len()), (count, 0));
    assert!(ui.applied_of(&workspace, Some(0)).is_some());
    let messages = ui.take_messages();
    assert_eq!(messages[0].level, Level::Success, "{messages:?}");
    assert!(workspace.undo());
    assert!(workspace.supports(0).is_none());
    let _ = std::fs::remove_dir_all(directory);
}

#[test]
fn generating_again_replaces_only_the_automatic_supports() {
    let (mut workspace, preparation, directory) = lifted_small_cube("again");
    let mut supports = SupportsUi::new();
    // One support placed by hand under the bottom's middle.
    let placed = workspace.placement_of(0).unwrap();
    let [cx, cy] =
        [0, 1].map(|k| f64::midpoint(placed.min.coordinates()[k], placed.max.coordinates()[k]));
    supports.straight_only = true;
    supports.toggle_placing();
    let up = ray(cx, cy, true);
    let hit = workspace.hit(&up);
    assert!(supports.click(&mut workspace, &up, hit, 0));
    supports.toggle_placing();
    built(&mut supports, &workspace, &preparation);
    let (_, by_hand) = by_origin(&workspace);
    assert_eq!(by_hand.len(), 1);
    let mut ui = AutoUi::new();
    generated(&mut ui, &mut supports, &workspace, &preparation);
    // No tip next to it: the island it holds is left to it.
    let (planned, _) = ui.plan_of(&workspace, Some(0)).unwrap();
    assert_eq!(planned.outcome.islands, [0, 0]);
    // At least the spacing away, as close as a wide underside's share allows (TD-127): the
    // cube's 64 mm² bottom is wider than the 20 mm² set.
    let middle = [cx, cy];
    let values = supports.automatic;
    let room = values.spacing_mm * values.wide_factor(64.0);
    for p in &planned.outcome.plan.supports {
        let [x, y, _] = p.routed.support.tip_mm;
        assert!((x - middle[0]).hypot(y - middle[1]) >= room - 1e-9);
    }
    let first = ui.apply(&mut workspace, 0);
    assert!(first > 0);
    let (automatic, others) = by_origin(&workspace);
    assert_eq!((automatic.len(), others.clone()), (first, by_hand.clone()));
    // Again, with branches off: the automatic supports are replaced, the one by hand stays.
    supports.automatic.branches = false;
    supports.automatic.braces = false;
    generated(&mut ui, &mut supports, &workspace, &preparation);
    let second = ui.apply(&mut workspace, 0);
    let (automatic, others) = by_origin(&workspace);
    assert_eq!((automatic.len(), others), (second, by_hand.clone()));
    let document = workspace.supports(0).unwrap();
    assert!(document
        .supports
        .iter()
        .filter(|s| s.origin == SupportOrigin::Automatic)
        .all(|s| !matches!(s.base, BaseSpec::Support { .. })));
    // A plan of the cavity alone would replace none of them: their tips are all outside.
    let surface = supports.route_surface(&workspace, &preparation, 0).unwrap();
    let to_world = workspace.model_to_world(0).unwrap();
    let mut sorted = automatic.clone();
    sorted.sort_unstable();
    for (place, expected) in [
        (Place::Everywhere, sorted.clone()),
        (Place::Outside, sorted),
        (Place::Cavity, Vec::new()),
    ] {
        let mut ids = replaced(document, &surface, &to_world, place);
        ids.sort_unstable();
        assert_eq!(ids, expected, "{place:?}");
    }
    // Deleting the automatic supports leaves the one placed by hand.
    supports.delete_automatic_supports(&mut workspace, 0);
    let (automatic, others) = by_origin(&workspace);
    assert_eq!((automatic.len(), others), (0, by_hand));
    let _ = std::fs::remove_dir_all(directory);
}

#[test]
fn supports_the_final_check_refuses_are_left_out_and_said() {
    // A sheet 0.12 mm thick lifted 6 mm: the tips, reaching 0.1 mm in (Medium, TD-136) with the
    // anchor tolerance's 0.05 mm, would pierce it.
    let folder = std::env::temp_dir().join(format!("karasu-auto-sheet-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    let model = folder.join("sheet.stl");
    std::fs::write(
        &model,
        crate::workspace::tests::box_stl([0.0; 3], [8.0, 8.0, 0.12]),
    )
    .unwrap();
    let (mut workspace, mut preparation) =
        crate::hollowing_ui::tests::placed(model, None, Duration::from_secs(60));
    workspace.edit(|plate| {
        plate.objects[0].pose.center = true;
        plate.objects[0].pose.drop = true;
        plate.objects[0].pose.lift_mm = Some(6.0);
    });
    crate::hollowing_ui::tests::prepared_within(
        &workspace,
        &mut preparation,
        Duration::from_secs(60),
    );
    let mut ui = AutoUi::new();
    let mut supports = SupportsUi::new();
    generated(&mut ui, &mut supports, &workspace, &preparation);
    let (planned, _) = ui.plan_of(&workspace, Some(0)).unwrap();
    let outcome = &planned.outcome;
    // Nothing is planned that a slice would build with an error; the sheet is said to be left,
    // because its tips would pierce it.
    assert!(
        outcome.plan.supports.is_empty(),
        "{:?}",
        outcome.plan.supports.len()
    );
    assert!(outcome.left.iter().any(|l| l.why == Skip::Pierces));
    assert!(
        outcome.left.iter().all(|l| l.why == Skip::Pierces),
        "{:?}",
        outcome.left
    );
    let (text, ..) = ui.state(&workspace, Some(0));
    assert!(text.contains("left unsupported"), "{text}");
    let _ = std::fs::remove_dir_all(folder);
}

#[test]
fn generate_lifts_the_model_to_the_tool_s_lift_first() {
    // TD-128: the cube stands 6 mm above the plate; the tool's lift is 5 mm.
    let (mut workspace, _, directory) = lifted_small_cube("lift");
    let mut ui = AutoUi::new();
    ui.generate_lifted(&mut workspace, 0, 5.0);
    assert!((lift_of(&workspace, 0) - 5.0).abs() < 1e-9);
    assert!(ui.planning());
    // One undoable step, back to where it stood.
    assert!(workspace.undo());
    assert!((lift_of(&workspace, 0) - 6.0).abs() < 1e-9);
    // Already there: the plan starts and nothing else changes.
    let mut ui = AutoUi::new();
    let at = workspace.revision();
    ui.generate_lifted(&mut workspace, 0, 6.0);
    assert!(ui.planning());
    assert_eq!(workspace.revision(), at);
    let _ = std::fs::remove_dir_all(directory);
}

#[test]
fn a_plan_made_before_an_edit_is_out_of_date_and_cannot_be_applied() {
    let (mut workspace, preparation, directory) = lifted_small_cube("stale");
    let mut ui = AutoUi::new();
    let mut supports = SupportsUi::new();
    generated(&mut ui, &mut supports, &workspace, &preparation);
    workspace.edit(|plate| plate.objects[0].pose.lift_mm = Some(7.0));
    let (_, current) = ui.plan_of(&workspace, Some(0)).unwrap();
    assert!(!current);
    assert_eq!(ui.apply(&mut workspace, 0), 0);
    assert!(workspace.supports(0).is_none());
    let _ = std::fs::remove_dir_all(directory);
}

#[test]
#[ignore = "plans every support of a local large model; see the documentation"]
fn a_local_model_s_supports_are_planned() {
    let Ok(path) = std::env::var("KARASU_INTERFACE_MODEL") else {
        eprintln!("KARASU_INTERFACE_MODEL is not set: no supports to plan");
        return;
    };
    let lift = std::env::var("KARASU_INTERFACE_LIFT").map_or(5.0, |l| l.parse().unwrap());
    let hollowing =
        std::env::var("KARASU_INTERFACE_WALL")
            .ok()
            .map(|w| karasu_project::project::Hollowing {
                wall_mm: w.parse().unwrap(),
                ..crate::hollowing_ui::DEFAULT_HOLLOWING
            });
    let limit = Duration::from_secs(600);
    let (mut workspace, mut preparation) =
        crate::hollowing_ui::tests::placed(path.into(), hollowing, limit);
    // KARASU_INTERFACE_ROTATE="x,y,z" turns the model (degrees), KARASU_INTERFACE_SCALE="200"
    // scales it (percent), as the Transform tool would.
    let triple = |name: &str| {
        std::env::var(name).ok().map(|v| {
            let values: Vec<f64> = v.split(',').map(|c| c.trim().parse().unwrap()).collect();
            [values[0], values[1], values[2]]
        })
    };
    let rotate = triple("KARASU_INTERFACE_ROTATE");
    let scale =
        std::env::var("KARASU_INTERFACE_SCALE").map_or(1.0, |s| s.parse::<f64>().unwrap() / 100.0);
    // Centred on the test printer's plate, lifted for its supports.
    workspace.edit(|plate| {
        if let Some(rotate) = rotate {
            plate.objects[0].pose.rotate_deg = rotate;
        }
        plate.objects[0].pose.scale = [scale; 3];
        plate.objects[0].pose.center = true;
        plate.objects[0].pose.drop = true;
        plate.objects[0].pose.lift_mm = Some(lift);
    });
    crate::hollowing_ui::tests::prepared_within(&workspace, &mut preparation, limit);
    let triangles = workspace
        .meshes()
        .values()
        .next()
        .map_or(0, |m| m.mesh.triangles().len());
    let mut supports = SupportsUi::new();
    // KARASU_INTERFACE_MINIS=0 plans without mini supports, to compare.
    supports.automatic.minis = std::env::var("KARASU_INTERFACE_MINIS").map_or(true, |v| v != "0");
    // KARASU_INTERFACE_SPHERE=1 ends the contacts in a ball.
    supports.automatic.sphere_contact =
        std::env::var("KARASU_INTERFACE_SPHERE").is_ok_and(|v| v == "1");
    // KARASU_INTERFACE_FIT=0.2 fits the contacts to the surface down to 0.2 mm (TD-116).
    supports.automatic.contact_fit_min_mm = std::env::var("KARASU_INTERFACE_FIT")
        .ok()
        .map(|m| m.parse().unwrap());
    for cover in [plan::Cover::IslandsOnly, plan::Cover::All] {
        supports.automatic.cover = cover;
        let mut ui = AutoUi::new();
        let started = Instant::now();
        generated(&mut ui, &mut supports, &workspace, &preparation);
        let took = started.elapsed().as_secs_f64();
        let (planned, _) = ui.plan_of(&workspace, Some(0)).unwrap();
        report_plan(triangles, cover, &planned.outcome, took);
        assert!(ui.apply(&mut workspace, 0) > 0);
        report_build(&workspace, &preparation);
        // A raft turned on under them, as the Raft tool turns it on.
        let mut rafted = workspace.supports(0).unwrap().clone();
        rafted.raft = Some(crate::raft_ui::DEFAULT_RAFT);
        workspace.set_supports(0, Some(rafted), None);
        report_build(&workspace, &preparation);
        assert!(workspace.undo());
        assert!(workspace.undo());
    }
    // Hollowed: the outside first, then the cavity alone, which leaves the outside's supports.
    if workspace.plate().objects[0].hollowing().is_some() {
        supports.automatic.cover = plan::Cover::All;
        let mut ui = AutoUi::new();
        let mut applied = Vec::new();
        for place in [Place::Outside, Place::Cavity] {
            supports.automatic.place = place;
            let started = Instant::now();
            generated(&mut ui, &mut supports, &workspace, &preparation);
            let took = started.elapsed().as_secs_f64();
            let count = ui.apply(&mut workspace, 0);
            let (automatic, _) = by_origin(&workspace);
            println!(
                "{place:?}: {count} supports added, {} automatic in all; {took:.2} s",
                automatic.len()
            );
            applied.push((count, automatic));
        }
        let [(outside, first), (cavity, all)] = [applied[0].clone(), applied[1].clone()];
        assert!(first.iter().all(|id| all.contains(id)));
        assert_eq!(all.len(), outside + cavity);
        report_build(&workspace, &preparation);
    }
}

/// The plan's summary for the measurement: its supports, coverage, places left and why, and
/// the time it took with the search.
fn report_plan(triangles: usize, cover: plan::Cover, o: &plan::AutoOutcome, took: f64) {
    let standing = o.plan.supports.iter().filter(|p| p.joins.is_none()).count();
    let minis = o
        .plan
        .supports
        .iter()
        .filter(|p| p.routed.support.kind == karasu_core::supports::SupportKind::Mini)
        .count();
    println!(
        "{triangles} triangles, {cover:?}: {} supports ({standing} standing, {minis} mini, {} branches), \
         {} braces; islands {}/{}; overhang points {}/{}; closing check {} passes, {} added; \
         {} places left; {took:.2} s with the search",
        o.plan.supports.len(),
        o.plan.branches,
        o.braces.len(),
        o.islands[0],
        o.islands[1],
        o.overhangs[0],
        o.overhangs[1],
        o.passes,
        o.added,
        o.left.len()
    );
    let why = |skip: crate::islands_ui::plan::Skip| o.left.iter().filter(|l| l.why == skip).count();
    println!(
        "left: {} out of reach, {} no way, {} no surface, {} unbuildable, {} too low, \
         {} collide, {} pierce; {} islands",
        why(crate::islands_ui::plan::Skip::OutOfReach),
        why(crate::islands_ui::plan::Skip::NoWay),
        why(crate::islands_ui::plan::Skip::NoSurface),
        why(crate::islands_ui::plan::Skip::Unbuildable),
        why(crate::islands_ui::plan::Skip::TooLow),
        why(crate::islands_ui::plan::Skip::Collides),
        why(crate::islands_ui::plan::Skip::Pierces),
        o.left.iter().filter(|l| l.island).count()
    );
}

/// What kind of support `s` is, for the report.
fn kind(s: &karasu_core::supports::ManualSupport) -> &'static str {
    if s.kind == karasu_core::supports::SupportKind::Mini {
        return "mini";
    }
    match (s.base, s.bends.is_empty()) {
        (BaseSpec::Support { .. }, true) => "branch",
        (BaseSpec::Support { .. }, false) => "merged trunk",
        (BaseSpec::Model { .. }, _) => "foot on model",
        (_, false) => "knee to plate",
        _ => "straight",
    }
}

/// How the mini supports of `document`, built as `scene`, meet the placed model: of each rod
/// between its balls (away from them as [`SurfaceGrid::rod_clear`] says), whether its surface
/// touches the model, enters it by more than half its radius, or its axis crosses it.
fn report_minis(
    document: &karasu_core::supports::SupportDocument,
    scene: &karasu_core::supports::SupportScene,
    mesh: &karasu_core::mesh::TriangleMesh,
    placement: &karasu_core::geometry::ObjectTransform,
) {
    let world = karasu_project::pipeline::placed_mesh(mesh, placement).unwrap();
    let grid = karasu_core::supports::SurfaceGrid::build(
        &world,
        &mut karasu_core::cancellation::Cancellation::never(),
    )
    .unwrap();
    let (mut minis, mut touch, mut half, mut axis) = (0, 0, 0, 0);
    for s in &document.supports {
        if s.kind != karasu_core::supports::SupportKind::Mini {
            continue;
        }
        let Some(path) = scene.shaft_path(s.id) else {
            continue;
        };
        minis += 1;
        let r = s.parameters.mini_diameter_mm / 2.0;
        // As the Islands and Automatic tools check them (TD-126), and at half and no radius.
        let meets = [r, r / 2.0, 1e-6].map(|radius| !grid.rod_clear(&world, path, radius));
        touch += usize::from(meets[0]);
        half += usize::from(meets[1]);
        axis += usize::from(meets[2]);
    }
    println!(
        "minis: {minis}; their rods away from the balls touch the model: {touch}, enter it by \
         over half their radius: {half}, cross it on their axis: {axis}"
    );
}

/// How many of `document`'s standard supports standing on the model (built as `scene`) leave
/// the lowest point of the ball at their shaft's bottom outside their foot (TD-132): there the
/// support starts in mid-air.
fn report_feet(
    workspace: &Workspace,
    document: &karasu_core::supports::SupportDocument,
    scene: &karasu_core::supports::SupportScene,
) {
    let to_world = workspace.model_to_world(0).unwrap();
    let (mut feet, mut hanging, mut wider) = (0, 0, 0);
    for s in &document.supports {
        let BaseSpec::Model { point_mm, .. } = s.base else {
            continue;
        };
        if s.kind != karasu_core::supports::SupportKind::Standard {
            continue;
        }
        let Some(&join) = scene.shaft_path(s.id).and_then(|path| path.last()) else {
            continue;
        };
        feet += 1;
        let p = &s.parameters;
        let bottom = p.shaft_bottom_diameter_mm.unwrap_or(p.shaft_diameter_mm);
        wider += usize::from(bottom > p.foot_body_diameter_mm + 1e-9);
        let anchor = to_world.point(point_mm);
        let rel = |q: [f64; 3]| [q[0] - anchor[0], q[1] - anchor[1], q[2] - anchor[2]];
        let length = rel(join).iter().map(|c| c * c).sum::<f64>().sqrt();
        let axis = rel(join).map(|c| c / length);
        let low = rel([join[0], join[1], join[2] - bottom / 2.0]);
        let along: f64 = (0..3).map(|k| low[k] * axis[k]).sum();
        let radial = (0..3)
            .map(|k| (low[k] - along * axis[k]).powi(2))
            .sum::<f64>()
            .sqrt();
        let (r0, r1) = (
            p.foot_contact_diameter_mm / 2.0,
            p.foot_body_diameter_mm / 2.0,
        );
        let radius = r0 + (r1 - r0) * (along / length).clamp(0.0, 1.0);
        hanging += usize::from(!(0.0..=length).contains(&along) || radial > radius);
    }
    println!(
        "feet on the model: {feet}; their shaft's bottom ball hanging outside the foot: \
         {hanging}; shaft bottom wider than the foot: {wider}"
    );
}

/// Model 0's supports built as a slice builds them: their findings by kind and severity, the
/// supports left out, the steep shafts and the long runs aside by the kind of support.
fn report_build(workspace: &Workspace, preparation: &PreparationUi) {
    let document = workspace.supports(0).unwrap();
    let object = &workspace.plate().objects[0];
    let mesh = &workspace.meshes()[&object.model].mesh;
    let placement = workspace.placement_of(0).unwrap().transform;
    let (shared, _) = preparation.shared(workspace, 0).unwrap();
    let prepared = shared.objects[0].as_ref().unwrap();
    let started = Instant::now();
    let scene = karasu_project::pipeline::build_model_supports(
        document,
        mesh,
        &placement,
        prepared,
        workspace.printer().unwrap(),
        &|_| false,
    )
    .unwrap();
    let took = started.elapsed().as_secs_f64();
    let mut findings = std::collections::BTreeMap::new();
    for f in scene.findings() {
        *findings
            .entry(format!("{:?} {:?}", f.severity, f.kind))
            .or_insert(0) += 1;
    }
    let left_out = scene.supports().iter().filter(|s| s.parts == 0).count();
    let support = |id: u64| document.supports.iter().find(|s| s.id == id);
    let mut steep = std::collections::BTreeMap::new();
    for f in scene
        .findings()
        .iter()
        .filter(|f| f.kind == karasu_core::supports::FindingKind::SteepShaft)
    {
        *steep
            .entry(support(f.entity).map_or("brace", kind))
            .or_insert(0) += 1;
    }
    // Long slanted runs: any segment of a built shaft reaching more than 8 mm aside.
    let mut wide = std::collections::BTreeMap::new();
    for s in &document.supports {
        let aside = scene.shaft_path(s.id).map_or(0.0, |path| {
            path.windows(2)
                .map(|w| (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]))
                .fold(0.0, f64::max)
        });
        if aside > 8.0 {
            *wide.entry(kind(s)).or_insert(0) += 1;
        }
    }
    // Contacts fitted to the surface, and how many narrowed.
    let narrowed: Vec<f64> = scene
        .supports()
        .iter()
        .filter_map(|built| {
            let asked = support(built.id)?.parameters.contact_diameter_mm;
            built.contact_diameter_mm.filter(|&d| d < asked - 1e-9)
        })
        .collect();
    let fitted = document
        .supports
        .iter()
        .filter(|s| s.parameters.contact_fit_min_mm.is_some())
        .count();
    let balls = document
        .supports
        .iter()
        .filter(|s| s.parameters.contact_shape == karasu_core::supports::ContactShape::Sphere)
        .count();
    report_minis(document, &scene, mesh, &placement);
    report_feet(workspace, document, &scene);
    println!(
        "built in {took:.3} s: {left_out} left out; findings {findings:?}; steep shafts \
         {steep:?}; over 8 mm aside {wide:?}; {balls} ball contacts; {fitted} contacts fitted, \
         {} narrowed (smallest {:.2} mm)",
        narrowed.len(),
        narrowed.iter().copied().fold(f64::INFINITY, f64::min)
    );
    for f in scene
        .findings()
        .iter()
        .filter(|f| f.severity == karasu_core::supports::Severity::Error)
        .take(8)
    {
        println!("  {f}");
    }
}
