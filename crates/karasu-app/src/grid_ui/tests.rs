use super::*;
use crate::supports_ui::tests::ray;
use plan::PickMode;
use std::time::Duration;

/// A slab 20 × 10 × 2 mm lifted 15 mm above the plate (room for branches), centred, on a preset
/// printer, prepared.
pub(crate) fn lifted_slab(name: &str) -> (Workspace, PreparationUi, std::path::PathBuf) {
    slab_at(name, 15.0)
}

/// The slab `lift` mm above the plate (on it at zero).
fn slab_at(name: &str, lift: f64) -> (Workspace, PreparationUi, std::path::PathBuf) {
    let folder = std::env::temp_dir().join(format!("karasu-grid-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    let model = folder.join("slab.stl");
    std::fs::write(
        &model,
        crate::workspace::tests::box_stl([0.0; 3], [20.0, 10.0, 2.0]),
    )
    .unwrap();
    let (mut workspace, mut preparation) =
        crate::hollowing_ui::tests::placed(model, None, Duration::from_secs(60));
    workspace.edit(|plate| {
        plate.objects[0].pose.center = true;
        plate.objects[0].pose.drop = true;
        plate.objects[0].pose.lift_mm = Some(lift).filter(|l| *l > 0.0);
    });
    crate::hollowing_ui::tests::prepared_within(
        &workspace,
        &mut preparation,
        Duration::from_secs(60),
    );
    (workspace, preparation, folder)
}

/// Poll until no plan is wanted or running, within two minutes.
fn planned(
    grid: &mut GridUi,
    supports: &mut SupportsUi,
    workspace: &Workspace,
    prep: &PreparationUi,
) {
    supports.route_wanted = true;
    let started = std::time::Instant::now();
    loop {
        supports.poll(workspace, prep, Some(0));
        let busy = grid.poll(workspace, prep, supports);
        if !busy && !grid.planning() {
            break;
        }
        assert!(started.elapsed() < Duration::from_secs(120), "no grid");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn a_flat_underside_clicked_is_gridded_previewed_and_applied_as_one_step() {
    let (mut workspace, preparation, folder) = lifted_slab("applied");
    let mut supports = SupportsUi::new();
    let mut grid = GridUi::new();
    let placed = workspace.placement_of(0).unwrap();
    let (min, max) = (placed.min.coordinates(), placed.max.coordinates());
    let [cx, cy] = [0, 1].map(|k| f64::midpoint(min[k], max[k]));
    // Off, a click is not the mode's.
    let under = ray(cx, cy, true);
    let hit = workspace.hit(&under);

    assert!(!grid.click(&workspace, hit, (0, false)));
    grid.toggle();
    // The top faces up: said, nothing picked.
    let top = ray(cx, cy, false);
    let hit = workspace.hit(&top);

    assert!(grid.click(&workspace, hit, (0, false)));
    assert_eq!(grid.take_messages()[0].level, Level::Warning);
    assert!(grid.pick.is_none());
    // The bottom: picked, planned.
    let hit = workspace.hit(&under);

    assert!(grid.click(&workspace, hit, (0, false)));
    planned(&mut grid, &mut supports, &workspace, &preparation);
    let (plan, current) = grid.plan_of(&workspace, Some(0)).expect("a plan");
    assert!(current);
    assert!((plan.face.area_mm2 - 200.0).abs() < 1e-6 && plan.face.tilt_deg.abs() < 1e-6);
    let tips = &plan.tips;
    assert_eq!((tips.corners, tips.sides), (4, 0));
    assert!(tips.edges > 0 && tips.grid > 0);
    let count = plan.outcome.plan.supports.len();
    assert_eq!(count, tips.tips.len(), "{:?}", plan.outcome.left);
    assert_eq!(plan.outcome.left.len(), 0);
    // Straight down by default: no branches.
    assert_eq!(plan.outcome.plan.branches, 0);
    // A value changed: the same face planned again, with fewer tips farther apart.
    supports.grid.spacing_mm = 5.0;
    supports.grid.across_mm = 5.0;
    planned(&mut grid, &mut supports, &workspace, &preparation);
    let (wider, _) = grid.plan_of(&workspace, Some(0)).unwrap();
    assert!(wider.outcome.plan.supports.len() < count);
    // Branches on: some tips join a trunk.
    supports.grid.branches = true;
    planned(&mut grid, &mut supports, &workspace, &preparation);
    let (branched, _) = grid.plan_of(&workspace, Some(0)).unwrap();
    assert!(branched.outcome.plan.branches > 0);
    let count = branched.outcome.plan.supports.len();
    // Applied: one undoable step, the supports placed (not the Automatic tool's to replace).
    let revision = workspace.revision();
    assert_eq!(grid.apply(&mut workspace, 0), count);
    assert_ne!(workspace.revision(), revision);
    let document = workspace.supports(0).unwrap();
    assert_eq!(document.supports.len(), count);
    assert!(document
        .supports
        .iter()
        .all(|s| s.origin == SupportOrigin::Placed));
    assert_eq!(grid.take_messages()[0].level, Level::Success);
    // Nothing picked after Apply: no plan again.
    planned(&mut grid, &mut supports, &workspace, &preparation);
    assert!(grid.plan_of(&workspace, Some(0)).is_none());
    assert!(workspace.undo());
    assert!(workspace.supports(0).is_none());
    // Discard, and the mode off, drop the face.
    let hit = workspace.hit(&under);

    assert!(grid.click(&workspace, hit, (0, false)));
    grid.discard();
    assert!(grid.pick.is_none());
    grid.toggle();
    assert!(!grid.active);
    let _ = std::fs::remove_dir_all(folder);
}

#[test]
fn a_rectangle_or_a_lasso_drawn_from_below_grids_what_it_holds() {
    let (workspace, preparation, folder) = lifted_slab("shapes");
    let mut supports = SupportsUi::new();
    let mut grid = GridUi::new();
    grid.toggle();
    grid.set_mode(PickMode::Rectangle);
    assert!(grid.shaping());
    // A click alone picks nothing with a shape to draw.
    assert!(grid.click(&workspace, None, (0, false)));
    assert!(grid.pick.is_none());
    // Seen from below, the slab's bottom in an 800 x 600 viewport.
    let placed = workspace.placement_of(0).unwrap();
    let (min, max) = (placed.min.coordinates(), placed.max.coordinates());
    let centre: [f64; 3] = std::array::from_fn(|k| f64::midpoint(min[k], max[k]));
    let mut camera = Camera::new(centre, 80.0);
    camera.look_from([0.0, 0.0, -1.0]);
    let size = [800.0, 600.0];
    let corner = |x: f64, y: f64| camera.screen([x, y, min[2]], size).unwrap();
    let (a, b) = (
        corner(min[0] - 1.0, min[1] - 1.0),
        corner(max[0] + 1.0, max[1] + 1.0),
    );
    // A shape too small is dropped.
    grid.begin_shape(a);
    grid.extend_shape([a[0] + 1.0, a[1] + 1.0]);
    grid.finish_shape(&workspace, (&camera, size), (0, false));
    assert!(grid.pick.is_none() && !grid.drawing());
    // A rectangle over the whole bottom: drawn, then picked; planned as a click on it.
    grid.begin_shape(a);
    grid.extend_shape([f64::midpoint(a[0], b[0]), a[1]]);
    grid.extend_shape(b);
    assert_eq!(grid.shape().unwrap().len(), 4);
    grid.finish_shape(&workspace, (&camera, size), (0, false));
    planned(&mut grid, &mut supports, &workspace, &preparation);
    let (plan, _) = grid.plan_of(&workspace, Some(0)).expect("a plan");
    assert!((plan.face.area_mm2 - 200.0).abs() < 1e-6);
    assert_eq!(plan.tips.corners, 4);
    let whole = plan.outcome.plan.supports.len();
    assert!(whole > 0 && plan.outcome.left.is_empty());
    // Kept as its triangles: a value changed plans them again.
    assert!(matches!(
        grid.pick.as_ref().map(|p| &p.at),
        Some(At::Triangles { .. })
    ));
    supports.grid.spacing_mm = 5.0;
    planned(&mut grid, &mut supports, &workspace, &preparation);
    let (wider, _) = grid.plan_of(&workspace, Some(0)).unwrap();
    assert!(wider.outcome.plan.supports.len() < whole);
    // A lasso round the bottom's left half: only tips it holds, fewer; its border across the
    // bottom, which goes on, takes no edge tips.
    supports.grid.spacing_mm = 3.0;
    grid.set_mode(PickMode::Lasso);
    let lasso = [
        corner(min[0] + 0.5, min[1] + 0.5),
        corner(centre[0], min[1] + 0.5),
        corner(centre[0], max[1] - 0.5),
        corner(min[0] + 0.5, max[1] - 0.5),
    ];
    grid.begin_shape(lasso[0]);
    for at in &lasso[1..] {
        grid.extend_shape(*at);
    }
    grid.finish_shape(&workspace, (&camera, size), (0, false));
    planned(&mut grid, &mut supports, &workspace, &preparation);
    let (half, _) = grid
        .plan_of(&workspace, Some(0))
        .expect("a plan of the half");
    let count = half.outcome.plan.supports.len();
    assert!(count > 0 && count < whole, "{count} of {whole}");
    for tip in &half.tips.tips {
        let at = camera.screen(tip.0, size).unwrap();
        assert!(plan::inside_polygon(&lasso, at), "{tip:?}");
        // None the inset in from the lasso's border across the bottom (at X = centre).
        assert!((tip.0[0] - (centre[0] - 0.4)).abs() > 0.05, "{tip:?}");
    }
    let _ = std::fs::remove_dir_all(folder);
}

#[test]
fn a_face_on_the_plate_grows_nothing_and_says_why() {
    // The slab resting on the plate: its bottom is picked, but no support has room under it.
    let (mut workspace, preparation, folder) = slab_at("plate", 0.0);
    let mut supports = SupportsUi::new();
    let mut grid = GridUi::new();
    grid.toggle();
    let placed = workspace.placement_of(0).unwrap();
    let (min, max) = (placed.min.coordinates(), placed.max.coordinates());
    let under = ray(
        f64::midpoint(min[0], max[0]),
        f64::midpoint(min[1], max[1]),
        true,
    );
    let hit = workspace.hit(&under);
    assert!(grid.click(&workspace, hit, (0, false)));
    planned(&mut grid, &mut supports, &workspace, &preparation);
    assert_eq!(grid.planned_count(&workspace, Some(0)), Some(0));
    let messages = grid.take_messages();
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert_eq!(messages[0].level, Level::Warning);
    assert_eq!(messages[0].text, tr!("grid-none"));
    let _ = std::fs::remove_dir_all(folder);
}

#[test]
#[ignore = "grids the base of a local model; see the documentation"]
#[allow(clippy::many_single_char_names, clippy::too_many_lines)] // Notation; one run, timed.
fn a_local_model_s_base_is_gridded() {
    // KARASU_INTERFACE_MODEL: a model standing on a flat base, lifted 5 mm; its widest level
    // triangle facing down is clicked, and each stage timed.
    let Ok(path) = std::env::var("KARASU_INTERFACE_MODEL") else {
        eprintln!("KARASU_INTERFACE_MODEL is not set: nothing to grid");
        return;
    };
    let limit = Duration::from_secs(600);
    let (mut workspace, mut preparation) =
        crate::hollowing_ui::tests::placed(path.into(), None, limit);
    // KARASU_INTERFACE_ROTATE="x,y,z" turns the model (degrees), as the Transform tool would.
    // KARASU_INTERFACE_LIFT: the lift, in mm (5 by default).
    let lift = std::env::var("KARASU_INTERFACE_LIFT").map_or(5.0, |l| l.parse().unwrap());
    let rotate = std::env::var("KARASU_INTERFACE_ROTATE").ok().map(|v| {
        let values: Vec<f64> = v.split(',').map(|c| c.trim().parse().unwrap()).collect();
        [values[0], values[1], values[2]]
    });
    workspace.edit(|plate| {
        if let Some(rotate) = rotate {
            plate.objects[0].pose.rotate_deg = rotate;
        }
        plate.objects[0].pose.center = true;
        plate.objects[0].pose.drop = true;
        plate.objects[0].pose.lift_mm = Some(lift).filter(|l| *l > 0.0);
    });
    crate::hollowing_ui::tests::prepared_within(&workspace, &mut preparation, limit);
    let mut supports = SupportsUi::new();
    supports.route_wanted = true;
    let started = std::time::Instant::now();
    let surface = loop {
        supports.poll(&workspace, &preparation, Some(0));
        if let Some(surface) = supports.route_surface(&workspace, &preparation, 0) {
            break surface;
        }
        assert!(started.elapsed() < limit, "no surface");
        std::thread::sleep(Duration::from_millis(10));
    };
    let mesh = surface.mesh().unwrap();
    // The widest triangle facing down within a few degrees of level: most often the base's.
    let seed = (0..u32::try_from(mesh.triangles().len()).unwrap())
        .filter_map(|t| {
            let [a, b, c] = mesh.triangles()[t as usize].map(|v| mesh.vertices()[v as usize]);
            let (u, v) = (
                [b.x - a.x, b.y - a.y, b.z - a.z],
                [c.x - a.x, c.y - a.y, c.z - a.z],
            );
            let n = [
                u[1] * v[2] - u[2] * v[1],
                u[2] * v[0] - u[0] * v[2],
                u[0] * v[1] - u[1] * v[0],
            ];
            let length = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            (length > 0.0 && n[2] / length < -0.995).then_some((t, length))
        })
        .max_by(|p, q| p.1.total_cmp(&q.1))
        .map(|(t, _)| t)
        .expect("a level underside");
    let values = DEFAULT_GRID_FOR_TEST;
    let started = std::time::Instant::now();
    let face = plan::pick(mesh, seed, values.flat_deg, &|_| true).expect("a face");
    let picked = started.elapsed();
    let radius = values.next(&[]).parameters().surface_width_mm() / 2.0;
    let started = std::time::Instant::now();
    let tips = plan::tips(mesh, &face, &values, radius, (&|_| false, &|_| true));
    let tipped = started.elapsed();
    println!(
        "{} triangles: face of {} triangles, {:.0} mm², tilted {:.1}°, picked in {picked:?}; {} tips ({} corners, {} edges, {} sides, {} grid) in {tipped:?}",
        mesh.triangles().len(),
        face.triangles.len(),
        face.area_mm2,
        face.tilt_deg,
        tips.tips.len(),
        tips.corners,
        tips.edges,
        tips.sides,
        tips.grid
    );
    let printer = workspace.printer().unwrap();
    let to_model = workspace.model_to_world(0).unwrap().inverse().unwrap();
    let input = Input {
        regions: Vec::new(),
        islands: Vec::new(),
        document: None,
        kept_tips: Vec::new(),
        surface: Arc::clone(&surface),
        next: values.next(&[]),
        cavity_next: None,
        values: values.automatic(),
        angle_deg: 45.0,
        volume: BuildVolume {
            origin_xy_mm: printer.origin_xy_mm,
            size_mm: printer.build_size_mm,
        },
        to_model,
        model: check::model_input(&workspace, &preparation, &supports, 0),
    };
    let started = std::time::Instant::now();
    let outcome = plan_tips(
        &input,
        (&input.values, &input.next),
        (&tips.tips, SupportOrigin::Placed),
        &AtomicBool::new(false),
    );
    let mut why: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for left in &outcome.left {
        *why.entry(format!("{:?}", left.why)).or_default() += 1;
    }
    println!(
        "{} supports grown and checked, {} left {why:?}, in {:?}",
        outcome.plan.supports.len(),
        outcome.left.len(),
        started.elapsed()
    );
}

/// The built-in values, for the local model's grid.
const DEFAULT_GRID_FOR_TEST: GridValues = plan::DEFAULT_GRID;
