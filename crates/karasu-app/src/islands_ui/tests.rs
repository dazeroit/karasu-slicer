use super::*;
use crate::supports_ui::tests::lifted_cube;
use crate::supports_ui::{NextSupport, Weight};
use karasu_core::supports::{BaseSpec, SupportOrigin};
use std::sync::atomic::AtomicBool;
use std::time::Duration;

/// An 8 mm cube lifted 6 mm above the plate on a preset printer, prepared: small, so a debug
/// build slices it in reasonable time.
fn lifted_small_cube(name: &str) -> (Workspace, PreparationUi, std::path::PathBuf) {
    let folder = std::env::temp_dir().join(format!("karasu-islands-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    let model = folder.join("cube.stl");
    std::fs::write(&model, crate::workspace::tests::box_stl([0.0; 3], [8.0; 3])).unwrap();
    let (mut workspace, mut preparation) =
        crate::hollowing_ui::tests::placed(model, None, Duration::from_secs(60));
    // The plate's format left as the preset printer does not write: the search writes the one
    // Slice would choose. Centred, so its supports' pads stay within the build volume.
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

/// Poll until the search ends, within five minutes (a debug build slices slowly).
fn searched(ui: &mut IslandsUi, workspace: &Workspace, preparation: &PreparationUi) {
    let started = Instant::now();
    let supports = SupportsUi::new();
    while ui.searching() {
        ui.poll(workspace, preparation, &supports, Some(0));
        assert!(started.elapsed() < Duration::from_secs(300), "no islands");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Ask for a plan with `ask`, then poll the supports' surface for routes and the plan until it
/// is made and, for Support it here, applied; within a minute.
fn planned(
    ui: &mut IslandsUi,
    supports: &mut SupportsUi,
    workspace: &mut Workspace,
    preparation: &PreparationUi,
    ask: impl FnOnce(&mut IslandsUi),
) {
    ask(ui);
    let here = ui.here.is_some();
    let started = Instant::now();
    loop {
        supports.poll(workspace, preparation, Some(0));
        ui.poll(workspace, preparation, supports, Some(0));
        if here {
            ui.finish_here(workspace, Some(0));
            if ui.here.is_none() {
                return;
            }
        } else if ui.plan_outcome(Some(0)).is_some() {
            return;
        }
        assert!(started.elapsed() < Duration::from_secs(60), "no plan");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn an_island_is_in_the_cavity_when_the_surface_above_it_is_the_cavity() {
    // A 20 mm cube hollowed with a 2 mm wall: its cavity from 2 to 18 mm.
    let (workspace, preparation, directory) = crate::hollowing_ui::tests::cube(true);
    let (shared, _) = preparation.shared(&workspace, 0).unwrap();
    let shell = shared.objects[0].as_ref().unwrap().hollowed().unwrap();
    let grid = SurfaceGrid::build(shell.mesh(), &mut Cancellation::never()).unwrap();
    let from = shell.exterior_faces();
    let [x0, y0, _] = workspace.placement_of(0).unwrap().min.coordinates();
    let (x, y) = (x0 + 10.0, y0 + 10.0);
    // Under the cavity's ceiling: in the cavity; under the cube's bottom, or past its top:
    // outside.
    assert!(in_cavity(shell.mesh(), &grid, from, [x, y, 17.9], 18.1));
    assert!(!in_cavity(shell.mesh(), &grid, from, [x, y, -0.1], 0.1));
    assert!(!in_cavity(shell.mesh(), &grid, from, [x, y, 21.0], 22.0));
    let _ = std::fs::remove_dir_all(directory);
}

#[test]
fn islands_are_found_supported_here_and_held_by_their_support() {
    let (mut workspace, preparation, directory) = lifted_small_cube("found");
    let mut ui = IslandsUi::new();
    let mut supports = SupportsUi::new();
    // The cube lifted 6 mm starts in mid-air: its bottom is one island.
    ui.find(&workspace, &preparation, 0);
    assert!(ui.searching());
    searched(&mut ui, &workspace, &preparation);
    assert!(
        ui.found(&workspace, Some(0)).is_some(),
        "no islands: {:?}",
        ui.take_messages()
    );
    let (islands, current) = ui.found(&workspace, Some(0)).expect("islands found");
    assert!(current);
    assert_eq!(islands.len(), 1, "{islands:?}");
    let island = islands[0];
    assert!((island.point[2] - 6.0).abs() < 0.2, "{island:?}");
    assert!(!island.inside);
    assert!(island.area_mm2 > 50.0, "{island:?}");
    assert_eq!(holder(&workspace, 0, &island, 0.0), None);
    let messages = ui.take_messages();
    assert_eq!(messages.len(), 1);
    // Supported here: one support where it starts, under its first layer's centroid, added as
    // one step once planned on the surface for routes; the island is held and the search is out
    // of date.
    let revision = workspace.revision();
    supports.route_wanted = true;
    // Without branches: a support of its own under every tip.
    supports.islands.branch_mm = 0.0;
    supports.islands.merge_mm = 0.0;
    planned(&mut ui, &mut supports, &mut workspace, &preparation, |ui| {
        ui.support_here(0);
    });
    assert_ne!(workspace.revision(), revision);
    let added = workspace.supports(0).unwrap().supports.clone();
    assert_eq!(added.len(), 1, "{added:?}");
    let to_world = workspace.model_to_world(0).unwrap();
    let tip = to_world.point(added[0].tip_mm);
    assert!(
        (tip[0] - island.point[0]).hypot(tip[1] - island.point[1]) < 1e-6,
        "{tip:?} {island:?}"
    );
    // Straight down to the plate: nothing is under the cube.
    assert!(added
        .iter()
        .all(|s| s.base == BaseSpec::Vertical && s.bends.is_empty()));
    assert!(holder(&workspace, 0, &island, 0.0).is_some());
    let (_, current) = ui.found(&workspace, Some(0)).unwrap();
    assert!(!current);
    assert_eq!(ui.take_messages().len(), 1);
    // They say the Islands tool placed them: deleting the islands' supports deletes them, and
    // only them, as one step.
    assert!(added.iter().all(|s| s.origin == SupportOrigin::Islands));
    let mut document = workspace.supports(0).cloned().unwrap();
    document
        .supports
        .push(karasu_core::supports::ManualSupport {
            id: 99,
            origin: SupportOrigin::Placed,
            ..added[0].clone()
        });
    workspace.set_supports(0, Some(document), None);
    supports.delete_island_supports(&mut workspace, 0);
    let left = workspace.supports(0).unwrap().supports.clone();
    assert_eq!(left.iter().map(|s| s.id).collect::<Vec<_>>(), [99]);
    // Back to before the islands were supported: the deletion, the support added by hand, then
    // the supports.
    for _ in 0..3 {
        workspace.undo();
    }
    assert_eq!(holder(&workspace, 0, &island, 0.0), None);
    // Support the islands: one support, previewed, added on Apply.
    planned(
        &mut ui,
        &mut supports,
        &mut workspace,
        &preparation,
        IslandsUi::toggle_supporting,
    );
    let outcome = ui.plan_outcome(Some(0)).expect("a plan");
    assert_eq!((outcome.supports.len(), outcome.held), (1, 1));
    assert!(ui.planned.as_ref().unwrap().preview.is_some());
    assert_eq!(ui.apply_plan(&mut workspace, 0), 1);
    assert!(!ui.supporting());
    assert_eq!(workspace.supports(0).unwrap().supports.len(), 1);
    workspace.undo();
    // The local minima show when asked: the cube's bottom face, 6 mm up.
    ui.show_minima = true;
    let started = Instant::now();
    while ui.poll(&workspace, &preparation, &supports, Some(0)) {
        assert!(started.elapsed() < Duration::from_secs(60), "no minima");
        std::thread::sleep(Duration::from_millis(10));
    }
    let points = ui.minima.as_ref().and_then(|m| m.points.clone()).unwrap();
    assert_eq!(points.len(), 1, "{points:?}");
    assert!((points[0][2] - 6.0).abs() < 1e-6);
    let _ = std::fs::remove_dir_all(directory);
}

#[test]
fn a_support_holds_an_island_when_its_tip_is_within_its_footprint() {
    let (mut workspace, _, directory) = lifted_cube();
    let island = Island {
        point: [0.0, 0.0, 5.0],
        layers: [100, 101],
        heights: [4.95, 5.05],
        area_mm2: 1.0,
        footprint: [[-1.0, -1.0], [1.0, 1.0]],
        inside: false,
    };
    let placed = workspace.placement_of(0).unwrap();
    let centre =
        [0, 1].map(|k| f64::midpoint(placed.min.coordinates()[k], placed.max.coordinates()[k]));
    let moved = Island {
        footprint: [
            [centre[0] - 1.0, centre[1] - 1.0],
            [centre[0] + 1.0, centre[1] + 1.0],
        ],
        ..island
    };
    assert_eq!(holder(&workspace, 0, &moved, 0.0), None);
    let mut supports = SupportsUi::new();
    supports.toggle_placing();
    let under = crate::supports_ui::tests::ray(centre[0], centre[1], true);
    let hit = workspace.hit(&under);
    assert!(supports.click(&mut workspace, &under, hit, 0));
    assert!(holder(&workspace, 0, &moved, 0.0).is_some());
    // Elsewhere on the plate, it holds nothing.
    let far = Island {
        footprint: [
            [centre[0] + 5.0, centre[1] + 5.0],
            [centre[0] + 6.0, centre[1] + 6.0],
        ],
        ..island
    };
    assert_eq!(holder(&workspace, 0, &far, 0.0), None);
    let _ = std::fs::remove_dir_all(directory);
}

#[test]
fn an_island_on_the_cavity_ceiling_is_held_from_the_cavity_floor() {
    // A 20 mm cube hollowed with a 2 mm wall: its cavity from 2 to 18 mm. Its ceiling joins the
    // walls (an overhang, not an island), so an island is set there by hand.
    let (workspace, preparation, directory) = crate::hollowing_ui::tests::cube(true);
    let mut supports = SupportsUi::new();
    supports.route_wanted = true;
    let started = Instant::now();
    let surface = loop {
        supports.poll(&workspace, &preparation, Some(0));
        if let Some(surface) = supports.route_surface(&workspace, &preparation, 0) {
            break surface;
        }
        assert!(started.elapsed() < Duration::from_secs(60), "no surface");
        std::thread::sleep(Duration::from_millis(10));
    };
    let placed = workspace.placement_of(0).unwrap();
    let [x0, y0, _] = placed.min.coordinates();
    let (x, y) = (x0 + 10.0, y0 + 10.0);
    let island = Island {
        point: [x, y, 18.05],
        layers: [360, 361],
        heights: [17.95, 18.1],
        area_mm2: 0.2,
        footprint: [[x - 0.3, y - 0.3], [x + 0.3, y + 0.3]],
        inside: true,
    };
    let next = NextSupport::of(Weight::Medium);
    let volume = karasu_core::supports::BuildVolume {
        origin_xy_mm: workspace.printer().unwrap().origin_xy_mm,
        size_mm: workspace.printer().unwrap().build_size_mm,
    };
    // Even without feet on the model, a support in the cavity stands on its floor.
    let values = plan::IslandSupports {
        foot_on_model: false,
        ..plan::DEFAULT_ISLAND_SUPPORTS
    };
    let outcome = plan::plan(
        &[(0, island)],
        &surface,
        &next,
        None,
        &values,
        volume,
        None,
        &AtomicBool::new(false),
    );
    assert_eq!((outcome.supports.len(), outcome.skipped.len()), (1, 0));
    let support = &outcome.supports[0].routed.support;
    assert!(support.tip_mm[2] > 17.9, "{support:?}");
    let BaseSpec::Model { point_mm, .. } = support.base else {
        panic!("{:?}", support.base);
    };
    assert!(point_mm[2] > 1.0 && point_mm[2] < 3.0, "{point_mm:?}");
    // Outside the cavity, under the cube's floor, the same island would go to the plate.
    let under = Island {
        point: [x, y, 0.05],
        heights: [-0.05, 0.1],
        inside: false,
        ..island
    };
    let outcome = plan::plan(
        &[(0, under)],
        &surface,
        &next,
        None,
        &values,
        volume,
        None,
        &AtomicBool::new(false),
    );
    assert_eq!(outcome.supports.len() + outcome.skipped.len(), 1);
    let _ = std::fs::remove_dir_all(directory);
}

#[test]
fn islands_closer_than_the_join_distance_share_a_support_under_the_lowest() {
    let island = |x: f64, z: f64| {
        (
            0,
            Island {
                point: [x, 0.0, z],
                layers: [1, 1],
                heights: [z - 0.05, z],
                area_mm2: 0.1,
                footprint: [[x - 0.1, -0.1], [x + 0.1, 0.1]],
                inside: false,
            },
        )
    };
    let islands = [
        island(0.0, 5.0),
        island(0.6, 4.9),
        island(1.5, 5.0),
        island(9.0, 5.0),
    ];
    // 0.6 mm apart: one group, led by the lower; 1.5 is within 1 mm of 0.6, so it joins too.
    let found = plan::groups(&islands, 1.0);
    assert_eq!(found, [vec![1, 0, 2], vec![3]]);
    // No joining: every island its own.
    assert_eq!(plan::groups(&islands, 0.0).len(), 4);
}

#[test]
#[ignore = "finds and supports the islands of a local large model; see the documentation"]
fn a_local_model_s_islands_are_found_and_supported() {
    let Ok(path) = std::env::var("KARASU_INTERFACE_MODEL") else {
        eprintln!("KARASU_INTERFACE_MODEL is not set: no islands to find");
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
    // Centred on the test printer's plate, lifted for its supports.
    workspace.edit(|plate| {
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
    let mut ui = IslandsUi::new();
    let started = Instant::now();
    ui.find(&workspace, &preparation, 0);
    searched(&mut ui, &workspace, &preparation);
    let found = started.elapsed();
    assert!(
        ui.found(&workspace, Some(0)).is_some(),
        "no islands: {:?}",
        ui.take_messages()
    );
    let (islands, _) = ui.found(&workspace, Some(0)).expect("islands found");
    let inside = islands.iter().filter(|i| i.inside).count();
    println!(
        "{triangles} triangles: {} islands ({inside} in the cavity) found in {:.2} s",
        islands.len(),
        found.as_secs_f64()
    );
    let mut supports = SupportsUi::new();
    supports.route_wanted = true;
    let started = Instant::now();
    while supports
        .route_surface(&workspace, &preparation, 0)
        .is_none()
    {
        supports.poll(&workspace, &preparation, Some(0));
        assert!(started.elapsed() < limit, "no surface");
        std::thread::sleep(Duration::from_millis(5));
    }
    println!(
        "surface for routes gridded in {:.2} s",
        started.elapsed().as_secs_f64()
    );
    for weight in [Weight::Light, Weight::Medium] {
        supports.islands.weight = weight;
        let started = Instant::now();
        planned(&mut ui, &mut supports, &mut workspace, &preparation, |ui| {
            if !ui.supporting() {
                ui.toggle_supporting();
            }
        });
        let took = started.elapsed();
        let outcome = ui.plan_outcome(Some(0)).unwrap();
        let on_plate = outcome
            .supports
            .iter()
            .filter(|p| p.joins.is_none())
            .count();
        println!("{weight:?}: {on_plate} supports stand on the plate or the model");
        let why = |skip: plan::Skip| outcome.skipped.iter().filter(|(_, w)| *w == skip).count();
        println!(
            "{weight:?}: {} supports ({} branches) for {} islands ({} joined) planned in \
             {:.2} s; skipped: {} out of reach, {} no way, {} no surface, {} unbuildable",
            outcome.supports.len(),
            outcome.branches,
            outcome.held,
            outcome.joined,
            took.as_secs_f64(),
            why(plan::Skip::OutOfReach),
            why(plan::Skip::NoWay),
            why(plan::Skip::NoSurface),
            why(plan::Skip::Unbuildable)
        );
    }
}

#[test]
fn stepping_out_of_the_cavity_turns_off_the_view_stepping_turned_on() {
    let (workspace, _, directory) = crate::hollowing_ui::tests::cube(true);
    let mut ui = IslandsUi::new();
    let mut supports = SupportsUi::new();
    let island = |inside: bool| Island {
        point: [10.0, 10.0, if inside { 18.0 } else { 0.5 }],
        layers: [1, 2],
        heights: [0.4, 0.6],
        area_mm2: 1.0,
        footprint: [[9.0, 9.0], [11.0, 11.0]],
        inside,
    };
    ui.found = Some(Found {
        object: 0,
        revision: workspace.revision(),
        islands: vec![island(false), island(true), island(false)],
    });
    // In the cavity: the view turns on; outside again: it turns off.
    ui.go_to(&workspace, 0, 1, &mut supports);
    assert!(supports.cavity_view);
    ui.go_to(&workspace, 0, 2, &mut supports);
    assert!(!supports.cavity_view);
    // Framed around its footprint, flat at the top of its first layer, where the section cuts
    // the model (owner, 2026-11-04): its planes at 0.4 under it and 0.5 in it, so 0.55.
    let [low, high] = ui.take_view().unwrap();
    assert!((low[2] - 0.55).abs() < 1e-12 && (high[2] - 0.55).abs() < 1e-12);
    assert!(low[0] < 9.0 && high[0] > 11.0);
    assert_eq!(ui.take_view(), None);
    // Turned on by the user, it stays on outside.
    supports.toggle_cavity_view();
    ui.go_to(&workspace, 0, 1, &mut supports);
    ui.go_to(&workspace, 0, 0, &mut supports);
    assert!(supports.cavity_view);
    let _ = std::fs::remove_dir_all(directory);
}

#[test]
fn islands_hidden_as_too_small_leave_the_list_and_its_counts_and_are_said() {
    let mut ui = IslandsUi::new();
    let island = |area_mm2: f64| Island {
        point: [10.0, 10.0, 0.5],
        layers: [1, 2],
        heights: [0.4, 0.6],
        area_mm2,
        footprint: [[9.0, 9.0], [11.0, 11.0]],
        inside: false,
    };
    let islands = [island(0.004), island(1.0), island(0.0), island(0.01)];
    let holders = [None; 4];
    // Nothing hidden by default.
    assert_eq!(ui.shown(&islands, &holders), vec![0, 1, 2, 3]);
    assert_eq!(ui.hidden_note(&islands), None);
    // Under 0.01 mm²: the two smaller are left out and said; one of 0.01 stays.
    ui.hide_under_mm2 = 0.01;
    assert_eq!(ui.shown(&islands, &holders), vec![1, 3]);
    assert_eq!(
        ui.hidden_note(&islands),
        Some(crate::i18n::tr!(
            "islands-hidden",
            count = 2_usize,
            area = "0.01"
        ))
    );
    ui.hide_under_mm2 = 5.0;
    assert_eq!(ui.shown(&islands, &holders), Vec::<usize>::new());
}

#[test]
fn tips_close_together_share_a_trunk_as_branches() {
    // The 20 mm cube lifted 15 mm: room under it for branches at 45°.
    let (mut workspace, mut preparation, directory) = lifted_cube();
    workspace.edit(|plate| plate.objects[0].pose.lift_mm = Some(15.0));
    crate::hollowing_ui::tests::prepared_within(
        &workspace,
        &mut preparation,
        Duration::from_secs(60),
    );
    let mut supports = SupportsUi::new();
    supports.route_wanted = true;
    let started = Instant::now();
    let surface = loop {
        supports.poll(&workspace, &preparation, Some(0));
        if let Some(surface) = supports.route_surface(&workspace, &preparation, 0) {
            break surface;
        }
        assert!(started.elapsed() < Duration::from_secs(60), "no surface");
        std::thread::sleep(Duration::from_millis(10));
    };
    let placed = workspace.placement_of(0).unwrap();
    let [x0, y0, z0] = placed.min.coordinates();
    let (x, y) = (x0 + 10.0, y0 + 10.0);
    // Nine islands 3 mm apart under the cube's middle: nine tips.
    let islands = grid_islands([x, y, z0], 3);
    let next = NextSupport::of(Weight::Light);
    let volume = karasu_core::supports::BuildVolume {
        origin_xy_mm: workspace.printer().unwrap().origin_xy_mm,
        size_mm: workspace.printer().unwrap().build_size_mm,
    };
    let plan = |values: &plan::IslandSupports| {
        plan::plan(
            &islands,
            &surface,
            &next,
            None,
            values,
            volume,
            None,
            &AtomicBool::new(false),
        )
    };
    // Without branches, nine supports; with them (5 mm, four a trunk), trunks and branches,
    // every branch after its trunk.
    let alone = plan(&plan::IslandSupports {
        branch_mm: 0.0,
        merge_mm: 0.0,
        ..plan::DEFAULT_ISLAND_SUPPORTS
    });
    assert_eq!((alone.supports.len(), alone.branches), (9, 0));
    let branching = plan::IslandSupports {
        merge_mm: 0.0,
        ..plan::DEFAULT_ISLAND_SUPPORTS
    };
    let outcome = plan(&branching);
    assert_eq!(outcome.supports.len(), 9);
    assert!(outcome.branches >= 4, "{} branches", outcome.branches);
    assert_eq!(outcome.held, 9);
    for (k, planned) in outcome.supports.iter().enumerate() {
        if let Some((trunk, z)) = planned.joins {
            assert!(trunk < k && outcome.supports[trunk].joins.is_none());
            assert!(z < planned.routed.support.tip_mm[2]);
        }
    }
    assert!(plan::preview(&outcome, volume).is_some());
    // Applied: branches on their trunks, built without an error.
    let ids = plan::apply(&mut workspace, 0, &outcome);
    assert_eq!(ids.len(), 9);
    let document = workspace.supports(0).unwrap();
    let branches = document
        .supports
        .iter()
        .filter(|s| matches!(s.base, BaseSpec::Support { .. }))
        .count();
    assert_eq!(branches, outcome.branches);
    crate::supports_ui::tests::built(&mut supports, &workspace, &preparation);
    let scene = supports.scene(0).unwrap();
    assert!(!scene.has_errors(), "{:?}", scene.findings());
    let _ = std::fs::remove_dir_all(directory);
}

#[test]
fn trunks_close_together_merge_into_a_thicker_one() {
    // The 20 mm cube lifted 15 mm, 25 islands 3 mm apart under its middle: 25 tips, each
    // on its own trunk (no branches), which then merge, none leaning more than the default
    // branch distance aside.
    let (mut workspace, mut preparation, directory) = lifted_cube();
    workspace.edit(|plate| plate.objects[0].pose.lift_mm = Some(15.0));
    crate::hollowing_ui::tests::prepared_within(
        &workspace,
        &mut preparation,
        Duration::from_secs(60),
    );
    let mut supports = SupportsUi::new();
    supports.route_wanted = true;
    let started = Instant::now();
    let surface = loop {
        supports.poll(&workspace, &preparation, Some(0));
        if let Some(surface) = supports.route_surface(&workspace, &preparation, 0) {
            break surface;
        }
        assert!(started.elapsed() < Duration::from_secs(60), "no surface");
        std::thread::sleep(Duration::from_millis(10));
    };
    let [x0, y0, z0] = workspace.placement_of(0).unwrap().min.coordinates();
    let (x, y) = (x0 + 10.0, y0 + 10.0);
    let islands = grid_islands([x, y, z0], 5);
    let next = NextSupport::of(Weight::Light);
    let volume = karasu_core::supports::BuildVolume {
        origin_xy_mm: workspace.printer().unwrap().origin_xy_mm,
        size_mm: workspace.printer().unwrap().build_size_mm,
    };
    let plan = |values: &plan::IslandSupports| {
        plan::plan(
            &islands,
            &surface,
            &next,
            None,
            values,
            volume,
            None,
            &AtomicBool::new(false),
        )
    };
    let on_plate = |outcome: &plan::Outcome| {
        outcome
            .supports
            .iter()
            .filter(|p| p.joins.is_none())
            .count()
    };
    let unmerged = plan(&plan::IslandSupports {
        branch_mm: 0.0,
        merge_mm: 0.0,
        ..plan::DEFAULT_ISLAND_SUPPORTS
    });
    // Groups large enough that a main trunk carries more than a full tree's tips.
    let merged = plan(&plan::IslandSupports {
        branch_mm: 0.0,
        per_group: 8,
        ..plan::DEFAULT_ISLAND_SUPPORTS
    });
    assert_eq!(merged.supports.len(), unmerged.supports.len());
    assert!(
        on_plate(&merged) < on_plate(&unmerged),
        "{} on the plate, {} before",
        on_plate(&merged),
        on_plate(&unmerged)
    );
    // A merged trunk bends, then joins below the bend; a main trunk is wider at its foot.
    let bent = merged
        .supports
        .iter()
        .filter(|p| p.joins.is_some() && !p.routed.support.bends.is_empty())
        .count();
    assert!(bent > 0);
    for planned in &merged.supports {
        if let (Some((trunk, z)), Some(bend)) = (planned.joins, planned.routed.support.bends.last())
        {
            assert!(z < bend[2]);
            // Straight trunks: the junction lies under the main trunk's tip.
            let main = merged.supports[trunk].routed.support.tip_mm;
            let aside = (bend[0] - main[0]).hypot(bend[1] - main[1]);
            let most = plan::DEFAULT_ISLAND_SUPPORTS.branch_mm;
            assert!(aside <= most + 1e-9, "{aside} mm aside");
        }
    }
    assert!(merged.supports.iter().any(|p| {
        let q = &p.routed.support.parameters;
        q.shaft_bottom_diameter_mm
            .is_some_and(|b| b > q.shaft_diameter_mm)
    }));
    // Applied and built without an error.
    plan::apply(&mut workspace, 0, &merged);
    crate::supports_ui::tests::built(&mut supports, &workspace, &preparation);
    let scene = supports.scene(0).unwrap();
    assert!(!scene.has_errors(), "{:?}", scene.findings());
    let _ = std::fs::remove_dir_all(directory);
}

#[test]
#[ignore = "supports a local model's first island here and reports the build; see the documentation"]
fn a_local_model_s_first_island_is_supported_here() {
    let Ok(path) = std::env::var("KARASU_INTERFACE_MODEL") else {
        eprintln!("KARASU_INTERFACE_MODEL is not set: no island to support");
        return;
    };
    let turn = std::env::var("KARASU_INTERFACE_ROTATE_X").map_or(0.0, |t| t.parse().unwrap());
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
    workspace.edit(|plate| {
        let pose = &mut plate.objects[0].pose;
        pose.rotate_deg = [turn, 0.0, 0.0];
        pose.center = true;
        pose.drop = true;
        pose.lift_mm = Some(5.0);
    });
    crate::hollowing_ui::tests::prepared_within(&workspace, &mut preparation, limit);
    let mut ui = IslandsUi::new();
    ui.find(&workspace, &preparation, 0);
    searched(&mut ui, &workspace, &preparation);
    let (islands, _) = ui.found(&workspace, Some(0)).expect("islands");
    let first = islands[0];
    println!("island 1: {first:?}");
    let mut supports = SupportsUi::new();
    supports.route_wanted = true;
    planned(&mut ui, &mut supports, &mut workspace, &preparation, |ui| {
        ui.support_here(0);
    });
    let document = workspace.supports(0).expect("supports").clone();
    let object = &workspace.plate().objects[0];
    let mesh = &workspace.meshes()[&object.model].mesh;
    let placement = workspace.placement_of(0).unwrap().transform;
    let (shared, _) = preparation.shared(&workspace, 0).unwrap();
    let scene = karasu_project::pipeline::build_model_supports(
        &document,
        mesh,
        &placement,
        shared.objects[0].as_ref().unwrap(),
        workspace.printer().unwrap(),
        &|_| false,
    )
    .unwrap();
    let to_world = workspace.model_to_world(0).unwrap();
    for s in &document.supports {
        println!(
            "support {} tip {:?} base {:?} bends {}",
            s.id,
            to_world
                .point(s.tip_mm)
                .map(|c| (c * 100.0).round() / 100.0),
            s.base,
            s.bends.len()
        );
    }
    for f in scene.findings() {
        println!("  {f}");
    }
    let nearest = document
        .supports
        .iter()
        .map(|s| {
            let [x, y, z] = to_world.point(s.tip_mm);
            ((x - first.point[0]).hypot(y - first.point[1]), z)
        })
        .min_by(|a, b| a.0.total_cmp(&b.0));
    println!(
        "nearest tip to where it starts: {nearest:?}; held: {:?}",
        holder(&workspace, 0, &first, 0.0)
    );
}

/// `side` × `side` small islands 3 mm apart, centred on `centre`'s plan position, starting at its
/// height: one tip each.
fn grid_islands(centre: [f64; 3], side: u32) -> Vec<(usize, Island)> {
    let [cx, cy, z] = centre;
    let half = 1.5 * f64::from(side - 1);
    (0..side * side)
        .map(|k| {
            let (column, row) = (f64::from(k % side), f64::from(k / side));
            let (px, py) = (cx - half + 3.0 * column, cy - half + 3.0 * row);
            (
                k as usize,
                Island {
                    point: [px, py, z + 0.05],
                    layers: [300, 301],
                    heights: [z - 0.05, z + 0.1],
                    area_mm2: 0.1,
                    footprint: [[px - 0.2, py - 0.2], [px + 0.2, py + 0.2]],
                    inside: false,
                },
            )
        })
        .collect()
}

/// The surface for routes of model 0, gridded within a minute.
fn route_surface(
    workspace: &Workspace,
    preparation: &PreparationUi,
    supports: &mut SupportsUi,
) -> Arc<crate::supports_ui::route::RouteSurface> {
    supports.route_wanted = true;
    let started = Instant::now();
    loop {
        supports.poll(workspace, preparation, Some(0));
        if let Some(surface) = supports.route_surface(workspace, preparation, 0) {
            return surface;
        }
        assert!(started.elapsed() < Duration::from_secs(60), "no surface");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn an_island_is_held_where_it_starts_but_not_where_a_drain_hole_cuts_it_away() {
    // The 20 mm cube lifted 15 mm and hollowed, a drain hole up through the middle of its
    // bottom, where its island starts.
    let (mut workspace, mut preparation, directory) = lifted_cube();
    workspace.edit(|plate| {
        plate.objects[0].pose.center = true;
        plate.objects[0].pose.lift_mm = Some(15.0);
        plate.objects[0].set_hollowing(Some(karasu_project::project::Hollowing {
            wall_mm: 2.0,
            cell_mm: Some(0.25),
            simplify: None,
        }));
    });
    workspace.add_holes(
        0,
        vec![karasu_project::request::DrainHole::Surface {
            surface_mm: [10.0, 10.0, 0.0],
            diameter_mm: 2.0,
            outside_mm: 1.0,
            inside_mm: 1.0,
            tilt: None,
            depth_mm: None,
            surface: karasu_core::preparation::sampled::HoleSurface::Outer,
        }],
    );
    crate::hollowing_ui::tests::prepared_within(
        &workspace,
        &mut preparation,
        Duration::from_secs(120),
    );
    let mut supports = SupportsUi::new();
    let surface = route_surface(&workspace, &preparation, &mut supports);
    let [x0, y0, z0] = workspace.placement_of(0).unwrap().min.coordinates();
    let (x, y) = (x0 + 10.0, y0 + 10.0);
    // An island starting under the hole: its first layer a point there, or the whole bottom.
    let island = |half: f64| Island {
        point: [x, y, z0 + 0.05],
        layers: [300, 380],
        heights: [z0 - 0.05, z0 + 4.0],
        area_mm2: 4.0 * half * half,
        footprint: [[x - half, y - half], [x + half, y + half]],
        inside: false,
    };
    let values = plan::IslandSupports {
        branch_mm: 0.0,
        merge_mm: 0.0,
        ..plan::DEFAULT_ISLAND_SUPPORTS
    };
    let volume = karasu_core::supports::BuildVolume {
        origin_xy_mm: workspace.printer().unwrap().origin_xy_mm,
        size_mm: workspace.printer().unwrap().build_size_mm,
    };
    let model = plan::check::model_input(&workspace, &preparation, &supports, 0).unwrap();
    let to_model = workspace.model_to_world(0).unwrap().inverse().unwrap();
    let planned = |island: Island| {
        plan::plan(
            &[(0, island)],
            &surface,
            &NextSupport::of(Weight::Light),
            None,
            &values,
            volume,
            Some((&model, &to_model)),
            &AtomicBool::new(false),
        )
    };
    // Starting at a point where the hole is: the hole cuts that material away, so there is
    // nothing to hold, and the hole is said (TD-131); no support beside it stands for it.
    let point = planned(island(0.2));
    assert_eq!(point.held, 0);
    assert_eq!(point.skipped, vec![(0, plan::Skip::DrainHole)]);
    assert!(point.supports.is_empty());
    // Starting over the whole bottom: its centre, in the hole, refused; one support a little
    // aside holds it.
    let outcome = planned(island(7.0));
    assert_eq!((outcome.held, outcome.skipped.len()), (1, 0));
    assert_eq!(outcome.supports.len(), 1);
    let [tx, ty, tz] = outcome.supports[0].routed.support.tip_mm;
    assert!(tz <= z0 + plan::START_MM, "{tz}");
    assert!((tx - x).hypot(ty - y) >= 2.0 - 1e-9, "{tx} {ty}");
    // Applied, it builds without an error.
    plan::apply(&mut workspace, 0, &outcome);
    crate::supports_ui::tests::built(&mut supports, &workspace, &preparation);
    let scene = supports.scene(0).unwrap();
    assert!(!scene.has_errors(), "{:?}", scene.findings());
    let _ = std::fs::remove_dir_all(directory);
}

#[test]
fn an_island_s_tips_stay_where_it_starts() {
    // A slab whose bottom steps up: 6 mm under its left half, 9 mm under its right one. The
    // region from the low step up to the high one gets tips only on the low step.
    let directory =
        std::env::temp_dir().join(format!("karasu-islands-step-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("step.stl");
    let low = crate::workspace::tests::box_stl([0.0, 0.0, 6.0], [10.0, 10.0, 12.0]);
    let high = crate::workspace::tests::box_stl([10.0, 0.0, 9.0], [20.0, 10.0, 12.0]);
    let inner = high
        .trim_start_matches("solid box\n")
        .trim_end_matches("endsolid box\n");
    std::fs::write(
        &path,
        low.replace("endsolid box\n", &format!("{inner}endsolid box\n")),
    )
    .unwrap();
    let (mut workspace, mut preparation) =
        crate::hollowing_ui::tests::placed(path, None, Duration::from_secs(60));
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
    let mut supports = SupportsUi::new();
    let surface = route_surface(&workspace, &preparation, &mut supports);
    let [x0, y0, z0] = workspace.placement_of(0).unwrap().min.coordinates();
    let island = Island {
        point: [x0 + 5.0, y0 + 5.0, z0 + 0.05],
        layers: [120, 180],
        heights: [z0 - 0.05, z0 + 3.0],
        area_mm2: 200.0,
        footprint: [[x0, y0], [x0 + 20.0, y0 + 10.0]],
        inside: false,
    };
    let volume = karasu_core::supports::BuildVolume {
        origin_xy_mm: workspace.printer().unwrap().origin_xy_mm,
        size_mm: workspace.printer().unwrap().build_size_mm,
    };
    let outcome = plan::plan(
        &[(0, island)],
        &surface,
        &NextSupport::of(Weight::Light),
        None,
        &plan::IslandSupports {
            branch_mm: 0.0,
            merge_mm: 0.0,
            ..plan::DEFAULT_ISLAND_SUPPORTS
        },
        volume,
        None,
        &AtomicBool::new(false),
    );
    assert_eq!(outcome.held, 1);
    // On the low step (its right edge, x = 10, included), never on the high one.
    for p in &outcome.supports {
        let [tx, _, tz] = p.routed.support.tip_mm;
        assert!(
            tx <= x0 + 10.0 + 1e-9 && tz < z0 + 1.0,
            "{:?}",
            [tx - x0, tz - z0]
        );
    }
    let _ = std::fs::remove_dir_all(directory);
}

#[test]
#[ignore = "compares a local model's islands with its local minima; see the documentation"]
fn a_local_model_s_islands_and_minima_are_compared() {
    let Ok(path) = std::env::var("KARASU_INTERFACE_MODEL") else {
        eprintln!("KARASU_INTERFACE_MODEL is not set");
        return;
    };
    let turn = std::env::var("KARASU_INTERFACE_ROTATE_X").map_or(0.0, |t| t.parse().unwrap());
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
    workspace.edit(|plate| {
        let pose = &mut plate.objects[0].pose;
        pose.rotate_deg = [turn, 0.0, 0.0];
        pose.center = true;
        pose.drop = true;
        pose.lift_mm = Some(5.0);
    });
    crate::hollowing_ui::tests::prepared_within(&workspace, &mut preparation, limit);
    let mut ui = IslandsUi::new();
    ui.find(&workspace, &preparation, 0);
    searched(&mut ui, &workspace, &preparation);
    let (islands, _) = ui.found(&workspace, Some(0)).expect("islands");
    let islands = islands.to_vec();
    let (shared, _) = preparation.shared(&workspace, 0).unwrap();
    let object = &workspace.plate().objects[0];
    let mesh = &workspace.meshes()[&object.model].mesh;
    let placement = workspace.placement_of(0).unwrap().transform;
    let minima = minima_of(&shared, 0, Some(mesh), &placement);
    let inside = islands.iter().filter(|i| i.inside).count();
    let within = |m: &[f64; 3]| {
        islands.iter().any(|i| {
            let [[x0, y0], [x1, y1]] = i.footprint;
            (x0 - 0.6..=x1 + 0.6).contains(&m[0])
                && (y0 - 0.6..=y1 + 0.6).contains(&m[1])
                && (i.heights[0] - 1.0..=i.heights[1] + 0.6).contains(&m[2])
        })
    };
    let starts = |m: &[f64; 3]| {
        islands.iter().any(|i| {
            (m[0] - i.point[0]).hypot(m[1] - i.point[1]) <= 2.0 && (m[2] - i.point[2]).abs() <= 1.0
        })
    };
    let outside_minima: Vec<&[f64; 3]> = minima.iter().filter(|m| !within(m)).collect();
    println!(
        "{} islands ({inside} in the cavity); {} minima off the plate: {} within an island's region, {} at an island's start, {} in no island",
        islands.len(),
        minima.len(),
        minima.iter().filter(|m| within(m)).count(),
        minima.iter().filter(|m| starts(m)).count(),
        outside_minima.len()
    );
    for m in outside_minima.iter().take(8) {
        println!("  minimum in no island: {m:?}");
    }
}

#[test]
#[ignore = "lists a local model's islands whose start finds no place for a support"]
fn a_local_model_s_islands_without_a_place_are_listed() {
    let Ok(path) = std::env::var("KARASU_INTERFACE_MODEL") else {
        return;
    };
    let limit = Duration::from_secs(600);
    let (mut workspace, mut preparation) =
        crate::hollowing_ui::tests::placed(path.into(), None, limit);
    workspace.edit(|plate| {
        let pose = &mut plate.objects[0].pose;
        pose.center = true;
        pose.drop = true;
        pose.lift_mm = Some(5.0);
    });
    crate::hollowing_ui::tests::prepared_within(&workspace, &mut preparation, limit);
    let mut ui = IslandsUi::new();
    ui.find(&workspace, &preparation, 0);
    searched(&mut ui, &workspace, &preparation);
    let islands = ui.found(&workspace, Some(0)).expect("islands").0.to_vec();
    let mut supports = SupportsUi::new();
    let surface = route_surface(&workspace, &preparation, &mut supports);
    let mesh = surface.mesh().unwrap();
    let mut none = 0;
    for island in &islands {
        if !plan::start_tips(&surface, mesh, island).is_empty() {
            continue;
        }
        none += 1;
        if none <= 10 {
            let [x, y, _] = island.point;
            let hit = surface.grid().first_above(
                mesh,
                [x, y, island.heights[0] - 1e-3],
                island.heights[1] + 1.0,
            );
            println!(
                "no place: {island:?}\n   hit {:?}",
                hit.map(|(p, n, _)| (p.map(|c| (c * 1000.0).round() / 1000.0), n))
            );
        }
    }
    println!("{none} of {} islands without a place", islands.len());
}

#[test]
fn an_island_too_low_for_the_preset_says_how_much_to_lift_the_model() {
    // The cube lifted 3 mm: its bottom starts 3 mm above the plate, a Light support needs
    // 3.2 mm (a 2 mm tip, a 0.8 mm cone, a 0.4 mm base).
    let (mut workspace, mut preparation, directory) = lifted_small_cube("too-low");
    workspace.edit(|plate| plate.objects[0].pose.lift_mm = Some(3.0));
    crate::hollowing_ui::tests::prepared_within(
        &workspace,
        &mut preparation,
        Duration::from_secs(60),
    );
    let mut ui = IslandsUi::new();
    ui.find(&workspace, &preparation, 0);
    searched(&mut ui, &workspace, &preparation);
    ui.take_messages();
    let mut supports = SupportsUi::new();
    supports.route_wanted = true;
    planned(&mut ui, &mut supports, &mut workspace, &preparation, |ui| {
        ui.support_here(0);
    });
    assert!(workspace.supports(0).is_none());
    let messages = ui.take_messages();
    let said = messages
        .iter()
        .find(|m| m.level == Level::Warning)
        .unwrap_or_else(|| panic!("{messages:?}"));
    let detail = said.detail.as_deref().unwrap();
    assert!(detail.contains("3.0 mm above"), "{detail}");
    assert!(detail.contains("needs 3.2 mm"), "{detail}");
    assert!(detail.contains("by 0.2 mm more"), "{detail}");
    let _ = std::fs::remove_dir_all(directory);
}

#[test]
fn an_island_with_no_room_for_a_support_gets_a_mini_one_on_the_model_below() {
    // A slab 2 mm above a wide block: no support fits under it (a Light support needs a 2 mm
    // tip and a 1.5 mm foot), none finds a way aside, so before mini supports it was left.
    let folder = std::env::temp_dir().join(format!("karasu-islands-mini-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    let path = folder.join("gap.stl");
    let block = crate::workspace::tests::box_stl([0.0, 0.0, 0.0], [20.0, 20.0, 4.0]);
    let inner = block
        .trim_start_matches("solid box\n")
        .trim_end_matches("endsolid box\n");
    let slab = crate::workspace::tests::box_stl([5.0, 5.0, 6.0], [15.0, 15.0, 8.0]);
    let stl = slab.replace("endsolid box\n", &format!("{inner}endsolid box\n"));
    std::fs::write(&path, stl).unwrap();
    let (mut workspace, preparation) =
        crate::hollowing_ui::tests::placed(path, None, Duration::from_secs(60));
    // Without mini supports nothing is added, so the same plate serves with them.
    for minis in [false, true] {
        let mut ui = IslandsUi::new();
        ui.find(&workspace, &preparation, 0);
        searched(&mut ui, &workspace, &preparation);
        let (islands, _) = ui.found(&workspace, Some(0)).expect("islands found");
        assert_eq!(islands.len(), 1, "{islands:?}");
        let mut supports = SupportsUi::new();
        supports.route_wanted = true;
        supports.islands.minis = minis;
        planned(&mut ui, &mut supports, &mut workspace, &preparation, |ui| {
            ui.support_here(0);
        });
        let added = workspace.supports(0).map(|d| d.supports.clone());
        if !minis {
            assert!(added.as_ref().is_none_or(Vec::is_empty), "{added:?}");
            continue;
        }
        let added = added.unwrap_or_else(|| panic!("no mini support: {:?}", ui.take_messages()));
        assert_eq!(added.len(), 1, "{added:?}");
        assert_eq!(added[0].kind, karasu_core::supports::SupportKind::Mini);
        let BaseSpec::Model { point_mm, .. } = added[0].base else {
            panic!("{:?}", added[0].base);
        };
        let to_world = workspace.model_to_world(0).unwrap();
        let [foot, tip] = [point_mm, added[0].tip_mm].map(|p| to_world.point(p));
        let z0 = workspace.placement_of(0).unwrap().min.coordinates()[2];
        assert!((foot[2] - (z0 + 4.0)).abs() < 1e-6, "{foot:?}");
        assert!(tip[2] > z0 + 5.9, "{tip:?}");
    }
    let _ = std::fs::remove_dir_all(folder);
}

#[test]
fn going_to_an_island_shows_its_first_layer_in_section() {
    // Owner, 2026-11-04: an island listed at layer 134 (index 133) showed layer 133. The
    // section snaps to the nearest layer boundary: the cut must reach the top of the island's
    // first layer.
    let (mut workspace, _, directory) = crate::hollowing_ui::tests::cube(false);
    let h = crate::layer_plan_ui::calibration(&workspace).map_or(0.05, |c| c.layer_height_mm);
    let island = Island {
        point: [10.0, 10.0, 133.5 * h],
        layers: [133, 140],
        heights: [132.5 * h, 140.5 * h],
        area_mm2: 1.0,
        footprint: [[9.0, 9.0], [11.0, 11.0]],
        inside: false,
    };
    let [low, _] = super::around(&island);
    let mut section = crate::section_ui::SectionUi::default();
    section.cut_above(&mut workspace, low[2]);
    let above = section.section(&mut workspace).above.unwrap();
    // Cut at the island's first layer's top: that layer is the highest shown.
    assert!(
        (above - 134.0 * h).abs() < 1e-9,
        "{above} with {h} mm layers"
    );
    let _ = std::fs::remove_dir_all(directory);
}
