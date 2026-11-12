use super::*;
use crate::supports_ui::tests::{built, lifted_cube, ray};
use std::time::{Duration, Instant};

/// The cube lifted 15 mm, prepared, with supports placed by hand under its bottom at `dxs` from
/// its middle along X, built. Returns the workspace, the interface and the middle.
fn supported(
    dxs: &[f64],
) -> (
    Workspace,
    crate::preparation_ui::PreparationUi,
    std::path::PathBuf,
    SupportsUi,
    [f64; 2],
) {
    let (mut workspace, mut preparation, directory) = lifted_cube();
    workspace.edit(|plate| plate.objects[0].pose.lift_mm = Some(15.0));
    crate::hollowing_ui::tests::prepared_within(
        &workspace,
        &mut preparation,
        Duration::from_secs(60),
    );
    let mut ui = SupportsUi::new();
    let placed = workspace.placement_of(0).unwrap();
    let [cx, cy] =
        [0, 1].map(|k| f64::midpoint(placed.min.coordinates()[k], placed.max.coordinates()[k]));
    ui.straight_only = true;
    ui.toggle_placing();
    for &dx in dxs {
        let under = ray(cx + dx, cy, true);
        let hit = workspace.hit(&under);
        assert!(ui.click(&mut workspace, &under, hit, 0));
    }
    ui.toggle_placing();
    ui.selected = None;
    built(&mut ui, &workspace, &preparation);
    (workspace, preparation, directory, ui, [cx, cy])
}

/// Poll until `done`, within a minute.
fn until(
    ui: &mut SupportsUi,
    workspace: &Workspace,
    preparation: &crate::preparation_ui::PreparationUi,
    done: impl Fn(&SupportsUi) -> bool,
) {
    let started = Instant::now();
    while !done(ui) {
        ui.poll(workspace, preparation, Some(0));
        assert!(started.elapsed() < Duration::from_secs(60), "not done");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn trunks_placed_by_hand_merge_into_a_tree_as_one_step() {
    let (mut workspace, preparation, directory, mut ui, _) = supported(&[-4.0, 0.0, 4.0]);
    ui.toggle_merging(Some(0));
    assert!(ui.merging());
    until(&mut ui, &workspace, &preparation, |ui| {
        ui.merge_counts(Some(0)).is_some()
    });
    // Three in a row within 10 mm: two lean into the middle one.
    assert_eq!(ui.merge_counts(Some(0)), Some((2, 1)));
    let ids: Vec<u64> = workspace
        .supports(0)
        .unwrap()
        .supports
        .iter()
        .map(|s| s.id)
        .collect();
    assert!(ids.iter().all(|&id| ui.merge_marks(0, id)));
    let revision = workspace.revision();
    assert_eq!(ui.apply_merging(&mut workspace, 0), 2);
    assert!(!ui.merging());
    assert_ne!(workspace.revision(), revision);
    let supports = workspace.supports(0).unwrap().supports.clone();
    let joined: Vec<_> = supports
        .iter()
        .filter(|s| matches!(s.base, BaseSpec::Support { parent, .. } if parent == ids[1]))
        .collect();
    assert_eq!(joined.len(), 2);
    assert!(joined.iter().all(|s| s.bends.len() == 1));
    assert_eq!(ui.take_messages().len(), 1);
    built(&mut ui, &workspace, &preparation);
    let scene = ui.scene(0).unwrap();
    assert!(!scene.has_errors(), "{:?}", scene.findings());
    workspace.undo();
    assert!(workspace
        .supports(0)
        .unwrap()
        .supports
        .iter()
        .all(|s| s.base == BaseSpec::Vertical));
    let _ = std::fs::remove_dir_all(directory);
}

#[test]
fn a_support_joins_the_shaft_clicked_below_its_braces_and_never_its_own_branch() {
    let (mut workspace, preparation, directory, mut ui, [cx, cy]) = supported(&[-2.0, 2.0, 8.0]);
    let ids: Vec<u64> = workspace
        .supports(0)
        .unwrap()
        .supports
        .iter()
        .map(|s| s.id)
        .collect();
    // A brace from the first support to the third at 9 mm: the first bends below it.
    let mut document = workspace.supports(0).cloned().unwrap();
    document.braces.push(karasu_core::supports::Brace {
        id: 50,
        from: (ids[0], 9.0),
        to: (ids[2], 9.0),
        diameter_mm: None,
        max_diameter_mm: None,
    });
    workspace.set_supports(0, Some(document), None);
    built(&mut ui, &workspace, &preparation);
    // The first joins the second.
    ui.selected = Some((0, ids[0]));
    ui.toggle_joining(&workspace, Some(0));
    assert!(ui.joining());
    until(&mut ui, &workspace, &preparation, |ui| ui.route.is_some());
    let across = |x: f64, z: f64| Ray {
        origin: [x, cy - 40.0, z],
        direction: [0.0, 1.0, 0.0],
    };
    assert!(ui.click(&mut workspace, &across(cx + 2.0, 6.0), None, 0));
    assert!(!ui.joining(), "refused: {}", ui.join_refused);
    let first = workspace.supports(0).unwrap().supports[0].clone();
    let BaseSpec::Support { parent, z_mm } = first.base else {
        panic!("{:?}", first.base);
    };
    assert_eq!(parent, ids[1]);
    let bend = workspace.model_to_world(0).unwrap().point(first.bends[0]);
    assert!(bend[2] < 9.0 && z_mm < bend[2], "{bend:?} {z_mm}");
    built(&mut ui, &workspace, &preparation);
    assert!(!ui.scene(0).unwrap().has_errors());
    // The second cannot join the first, which stands on it.
    ui.selected = Some((0, ids[1]));
    ui.toggle_joining(&workspace, Some(0));
    assert!(ui.click(&mut workspace, &across(cx - 2.0, 12.0), None, 0));
    assert!(ui.joining() && ui.join_refused);
    assert!(ui.escape());
    assert!(!ui.joining());
    // A support already on another says so and waits for nothing.
    ui.selected = Some((0, ids[0]));
    ui.toggle_joining(&workspace, Some(0));
    assert!(!ui.joining());
    assert_eq!(ui.take_messages().len(), 1);
    let _ = std::fs::remove_dir_all(directory);
}
