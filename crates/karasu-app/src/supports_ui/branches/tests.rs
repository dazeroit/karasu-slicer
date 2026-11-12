use super::*;
use crate::supports_ui::height::TipAt;
use crate::supports_ui::tests::{built, lifted_cube, ray};
use std::time::Duration;

/// The cube lifted 15 mm, prepared, with one support under its middle, built: a trunk from
/// about 2 mm to above 12 mm. Returns the workspace, the interface and the middle.
fn trunk() -> (
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
    ui.toggle_placing();
    let under = ray(cx, cy, true);
    let hit = workspace.hit(&under);
    assert!(ui.click(&mut workspace, &under, hit, 0));
    built(&mut ui, &workspace, &preparation);
    (workspace, preparation, directory, ui, [cx, cy])
}

/// A ray across the trunk at height `z`, from the side.
fn across(x: f64, y: f64, z: f64) -> Ray {
    Ray {
        origin: [x, y - 40.0, z],
        direction: [0.0, 1.0, 0.0],
    }
}

#[test]
fn a_branch_joins_its_trunk_within_45_degrees_and_goes_with_it() {
    let (mut workspace, preparation, directory, mut ui, [cx, cy]) = trunk();
    let trunk = workspace.supports(0).unwrap().supports[0].id;
    ui.set_branching(true);
    assert!(!ui.tip_then_foot);
    // The tip, 5 mm aside under the cube: it waits for the shaft it joins.
    let under = ray(cx + 5.0, cy, true);
    let hit = workspace.hit(&under);
    assert!(ui.click(&mut workspace, &under, hit, 0));
    assert!(ui.branch_waiting());
    assert_eq!(workspace.supports(0).unwrap().supports.len(), 1);
    // Over the trunk, the branch it would add shows.
    ui.branch_hover(&workspace, Some(&across(cx, cy, 8.0)), 0);
    assert!(ui.preview.as_ref().is_some_and(|p| !p.error));
    // A click on the trunk adds it, joined within 45°, as one step.
    assert!(ui.click(&mut workspace, &across(cx, cy, 8.0), None, 0));
    let supports = workspace.supports(0).unwrap().supports.clone();
    assert_eq!(supports.len(), 2);
    let branch = &supports[1];
    let BaseSpec::Support { parent, z_mm } = branch.base else {
        panic!("{:?}", branch.base);
    };
    assert_eq!(parent, trunk);
    // The tip's join is about 2.5 mm below the cube, 5 mm aside: 45° puts it at most 12.5 mm.
    assert!(z_mm < 12.5 && z_mm > 6.0, "{z_mm}");
    assert!(!ui.branch_waiting());
    built(&mut ui, &workspace, &preparation);
    let scene = ui.scene(0).unwrap();
    assert!(!scene.has_errors(), "{:?}", scene.findings());
    // Its junction slides along the trunk.
    ui.selected = Some((0, branch.id));
    assert!(ui.press_junction_for_test(&mut workspace));
    let section = crate::viewport::Section::default();
    ui.drag_to(
        &mut workspace,
        &across(cx + 3.0, cy, z_mm - 2.0),
        (section, TipAt::Free),
        None,
    );
    ui.release(&mut workspace);
    let moved = workspace.supports(0).unwrap().supports[1].base;
    assert!(
        matches!(moved, BaseSpec::Support { z_mm: z, .. } if (z - (z_mm - 2.0)).abs() < 1e-6),
        "{moved:?}"
    );
    // Deleting the trunk deletes its branch, as one step.
    ui.selected = Some((0, trunk));
    assert!(ui.delete(&mut workspace));
    assert!(workspace.supports(0).is_none_or(|d| d.supports.is_empty()));
    workspace.undo();
    assert_eq!(workspace.supports(0).unwrap().supports.len(), 2);
    let _ = std::fs::remove_dir_all(directory);
}

#[test]
fn a_tip_too_far_for_45_degrees_is_refused() {
    let (mut workspace, _preparation, directory, mut ui, [cx, cy]) = trunk();
    ui.set_branching(true);
    // 9 mm aside both ways: 12.7 mm from the trunk, more than its height under the tip.
    let under = ray(cx + 9.0, cy + 9.0, true);
    let hit = workspace.hit(&under);
    assert!(ui.click(&mut workspace, &under, hit, 0));
    assert!(ui.click(&mut workspace, &across(cx, cy, 8.0), None, 0));
    assert!(ui.branch_refused);
    assert_eq!(workspace.supports(0).unwrap().supports.len(), 1);
    // Escape takes the tip back.
    assert!(ui.escape());
    assert!(!ui.branch_waiting());
    let _ = std::fs::remove_dir_all(directory);
}

#[test]
fn deleting_supports_takes_the_branches_on_them_and_theirs() {
    let mut document = SupportDocument::new(Preset::Medium);
    let support = |id: u64, base: BaseSpec| ManualSupport {
        id,
        tip_mm: [0.0, 0.0, 10.0],
        tip_axis: AxisSpec::Normal,
        bends: Vec::new(),
        base,
        parameters: Preset::Medium.parameters(),
        accept_collision: false,
        origin: SupportOrigin::Placed,
        kind: karasu_core::supports::SupportKind::Standard,
    };
    document.supports = vec![
        support(1, BaseSpec::Vertical),
        support(
            2,
            BaseSpec::Support {
                parent: 1,
                z_mm: 5.0,
            },
        ),
        support(
            3,
            BaseSpec::Support {
                parent: 2,
                z_mm: 6.0,
            },
        ),
        support(4, BaseSpec::Vertical),
    ];
    document.braces.push(karasu_core::supports::Brace {
        id: 5,
        from: (3, 6.0),
        to: (4, 6.0),
        diameter_mm: None,
        max_diameter_mm: None,
    });
    assert_eq!(with_branches(&document, &[1]), [1, 2, 3]);
    without(&mut document, &[1]);
    assert_eq!(document.supports.len(), 1);
    assert_eq!(document.supports[0].id, 4);
    assert_eq!(document.braces.len(), 0);
}

#[test]
fn a_branch_is_no_thicker_than_its_trunk() {
    let (mut workspace, preparation, directory, mut ui, [cx, cy]) = trunk();
    let trunk = workspace.supports(0).unwrap().supports[0].clone();
    // A Heavy branch on the Medium trunk takes the trunk's values, said in a notification: its
    // preset is the trunk's.
    ui.next = crate::supports_ui::NextSupport::of(crate::supports_ui::Weight::Heavy);
    ui.set_branching(true);
    let under = ray(cx + 5.0, cy, true);
    let hit = workspace.hit(&under);
    assert!(ui.click(&mut workspace, &under, hit, 0));
    assert!(ui.click(&mut workspace, &across(cx, cy, 8.0), None, 0));
    let branch = workspace.supports(0).unwrap().supports[1].clone();
    assert_eq!(branch.parameters, trunk.parameters);
    assert_eq!(ui.take_messages().len(), 1);
    // A Light branch keeps its own values, without a word.
    ui.next = crate::supports_ui::NextSupport::of(crate::supports_ui::Weight::Light);
    let under = ray(cx - 5.0, cy, true);
    let hit = workspace.hit(&under);
    assert!(ui.click(&mut workspace, &under, hit, 0));
    assert!(ui.click(&mut workspace, &across(cx, cy, 8.0), None, 0));
    let light = workspace.supports(0).unwrap().supports[2].clone();
    assert_eq!(light.parameters, Preset::Light.parameters());
    assert_eq!(ui.take_messages().len(), 0);
    // A value made thicker in the panel is brought back to the trunk's, the tip's body too.
    let BaseSpec::Support { z_mm, .. } = branch.base else {
        panic!("{:?}", branch.base);
    };
    let at = ui.scene(0).unwrap().shaft_diameter(trunk.id, z_mm).unwrap();
    let mut thick = branch.clone();
    thick.parameters.shaft_diameter_mm = 5.0;
    thick.parameters.shaft_bottom_diameter_mm = Some(6.0);
    thick.parameters.tip_body_diameter_mm = 4.0;
    let thinned = ui.no_thicker(0, thick);
    assert!((thinned.parameters.shaft_diameter_mm - at).abs() < 1e-9);
    assert_eq!(thinned.parameters.shaft_bottom_diameter_mm, Some(at));
    assert!((thinned.parameters.tip_body_diameter_mm - at).abs() < 1e-9);
    // A support on the plate keeps its values.
    let mut plate = trunk.clone();
    plate.parameters.shaft_diameter_mm = 5.0;
    assert!((ui.no_thicker(0, plate).parameters.shaft_diameter_mm - 5.0).abs() < 1e-12);
    built(&mut ui, &workspace, &preparation);
    assert!(!ui.scene(0).unwrap().has_errors());
    let _ = std::fs::remove_dir_all(directory);
}
