use super::*;

#[test]
fn values_out_of_range_take_the_defaults_and_priorities_set_the_weights() {
    let wrong = OrientValues {
        limit_deg: 0.0,
        keep_away_mm: -1.0,
        weights: WeightValues {
            facing: 50.0,
            ..WeightValues::default()
        },
        ..DEFAULT_ORIENT
    }
    .valid();
    assert_eq!(wrong.limit_deg, DEFAULT_ORIENT.limit_deg);
    assert_eq!(wrong.keep_away_mm, DEFAULT_ORIENT.keep_away_mm);
    assert_eq!(wrong.weights.facing, DEFAULT_ORIENT.weights.facing);
    // Owner, 2026-11-03: the tilt limited to 30° by default.
    let defaults = OrientValues::default();
    assert!(defaults.limited && (defaults.limit_deg - 30.0).abs() < 1e-12);
    let fewer = Priority::FewerSupports.weights();
    let faster = Priority::Faster.weights();
    assert!(fewer.facing > faster.facing && faster.height > fewer.height);
    // Saved and read back.
    let json = serde_json::to_string(&DEFAULT_ORIENT).unwrap();
    assert_eq!(
        serde_json::from_str::<OrientValues>(&json).unwrap(),
        DEFAULT_ORIENT
    );
    assert_eq!(
        serde_json::from_str::<OrientValues>("{}").unwrap(),
        DEFAULT_ORIENT
    );
}

#[test]
fn a_turn_keeps_its_pivot_and_moves_up_by_its_lift() {
    let tilt = Tilt::towards([0.0, 0.5, -0.5f64.sqrt() * 1.2], [1.0, 0.0, 0.0]).unwrap();
    let anchor = [10.0, 20.0, 5.0];
    let m = turn_matrix(&tilt, anchor, 2.0);
    let at = |p: [f64; 3]| -> [f64; 3] {
        std::array::from_fn(|r| m.0[r][0] * p[0] + m.0[r][1] * p[1] + m.0[r][2] * p[2] + m.0[r][3])
    };
    let pivot = at(anchor);
    assert!((pivot[0] - 10.0).abs() < 1e-12 && (pivot[1] - 20.0).abs() < 1e-12);
    assert!((pivot[2] - 7.0).abs() < 1e-12);
    // The same map as a point turned on its own.
    let p = [13.0, 18.0, 9.0];
    let (a, b) = (at(p), turned(&tilt, anchor, 2.0, p));
    assert!((0..3).all(|k| (a[k] - b[k]).abs() < 1e-12));
}

#[test]
fn tilts_are_described_by_their_plate_axes() {
    let about_x = Tilt::towards([0.0, 0.5, -0.75f64.sqrt()], [1.0, 0.0, 0.0]).unwrap();
    assert!(describe(&about_x).contains('X'), "{}", describe(&about_x));
    assert!(!describe(&about_x).contains('Y'));
    let about_y = Tilt::towards([0.5, 0.0, -0.75f64.sqrt()], [0.0, 1.0, 0.0]).unwrap();
    assert!(describe(&about_y).contains('Y') && !describe(&about_y).contains('X'));
    assert_eq!(describe(&Tilt::NONE), tr!("orient-tilt-none"));
}

/// A 6 × 10 × 3 mm box lying flat on the plate on a preset printer, prepared.
fn flat_box(name: &str) -> (Workspace, PreparationUi, std::path::PathBuf) {
    let folder = std::env::temp_dir().join(format!("karasu-orient-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    let model = folder.join("box.stl");
    std::fs::write(
        &model,
        crate::workspace::tests::box_stl([0.0; 3], [6.0, 10.0, 3.0]),
    )
    .unwrap();
    let timeout = std::time::Duration::from_secs(60);
    let (mut workspace, mut preparation) = crate::hollowing_ui::tests::placed(model, None, timeout);
    workspace.edit(|plate| {
        plate.objects[0].pose.center = true;
        plate.objects[0].pose.drop = true;
    });
    crate::hollowing_ui::tests::prepared_within(&workspace, &mut preparation, timeout);
    (workspace, preparation, folder)
}

/// Find, then poll until the search ends, within five minutes (a debug build slices slowly).
fn found(ui: &mut OrientUi, workspace: &Workspace, preparation: &PreparationUi) {
    ui.find(workspace, preparation, 0);
    let started = Instant::now();
    while ui.searching() {
        ui.poll();
        assert!(started.elapsed().as_secs() < 300, "no orientations");
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(ui.searched.is_some(), "{:?}", ui.take_messages());
}

#[test]
fn a_flat_box_is_proposed_as_it_lies_and_turned_as_one_step_supports_asked_first() {
    let (mut workspace, preparation, folder) = flat_box("turned");
    let mut ui = OrientUi::new();
    // Within 30° of lying flat, about X only: lying flat needs no support at all.
    ui.values.axes = Axes::X;
    found(&mut ui, &workspace, &preparation);
    let (candidates, current) = ui.found_of(&workspace, Some(0)).unwrap();
    let candidates = candidates.to_vec();
    assert!(current && !candidates.is_empty());
    assert!(candidates.len() <= SHOWN);
    let best = candidates[0];
    assert!(best.tilt.angle_deg() < 0.5, "{best:?}");
    assert_eq!((best.facing_mm2, best.islands), (0.0, 0));
    for c in &candidates {
        assert!(c.tilt.angle_deg() <= 30.0 + 1e-9);
        assert!(c.tilt.down[0].abs() < 1e-12, "about X only: {c:?}");
    }
    // The preview turns the model about its centre; the scene stays as it is.
    ui.hovered = Some(candidates.len() - 1);
    assert!(ui.preview(&workspace, Some(0)).is_some());
    let before = workspace.plate().objects[0].pose.rotate_deg;
    // Apply the second, as one step.
    ui.hovered = None;
    ui.chosen = 1;
    let tilt = candidates[1].tilt;
    ui.apply(&mut workspace, 0);
    let after = workspace.plate().objects[0].pose.rotate_deg;
    assert_ne!(after, before);
    let turned = euler_matrix(after);
    let expected = product(tilt.matrix(), euler_matrix(before));
    for r in 0..3 {
        for c in 0..3 {
            assert!((turned[r][c] - expected[r][c]).abs() < 1e-9);
        }
    }
    // Turned, the candidates describe the model before: none is shown any more.
    assert!(ui.found_of(&workspace, Some(0)).is_none());
    assert!(workspace.undo());
    assert_eq!(workspace.plate().objects[0].pose.rotate_deg, before);
    // With supports, Apply asks first; nothing changes until it is confirmed.
    let document = karasu_core::supports::SupportDocument::from_json(
        br#"{"format":"karasu-supports","version":1,"supports":[{"id":1,"tip":{"point":[3,5,0]},"base":{"vertical":true}}]}"#,
    )
    .unwrap();
    workspace.set_supports(0, Some(document), None);
    found(&mut ui, &workspace, &preparation);
    ui.chosen = 1;
    ui.apply(&mut workspace, 0);
    assert_eq!(ui.confirming, Some(0));
    assert_eq!(workspace.plate().objects[0].pose.rotate_deg, before);
    ui.confirming = None;
    ui.turn(&mut workspace, 0, true);
    assert!(workspace.supports(0).is_none());
    assert!(workspace.undo());
    assert!(workspace.supports(0).is_some());
    let _ = std::fs::remove_dir_all(folder);
}

#[test]
fn a_protected_point_follows_the_model_and_refuses_turning_its_surface_to_the_plate() {
    let (mut workspace, preparation, folder) = flat_box("protected");
    let mut ui = OrientUi::new();
    // The top's middle, protected: no tilt beyond 90° may turn it down.
    let map = workspace.model_to_world(0).unwrap();
    let top = map.point([3.0, 5.0, 3.0]);
    assert!(OrientUi::add_point(&mut workspace, 0, top, [0.0, 0.0, 1.0]));
    let point = OrientUi::points_of(&workspace, 0)[0];
    assert!((0..3).all(|k| (point.point_mm[k] - [3.0, 5.0, 3.0][k]).abs() < 1e-9));
    // Kept with the object: undone with it, and back again.
    assert!(workspace.undo());
    assert_eq!(OrientUi::points_of(&workspace, 0), []);
    assert!(workspace.redo());
    assert_eq!(OrientUi::points_of(&workspace, 0), [point]);
    assert!((point.normal[2] - 1.0).abs() < 1e-12);
    ui.values.axes = Axes::Y;
    ui.values.limited = false;
    found(&mut ui, &workspace, &preparation);
    let (candidates, _) = ui.found_of(&workspace, Some(0)).unwrap();
    assert!(candidates
        .iter()
        .all(|c| c.tilt.angle_deg() <= 135.0 + 1e-9));
    // While protecting, the model shows as it is: a click lands on it.
    assert!(ui.preview(&workspace, Some(0)).is_some());
    ui.protecting = true;
    assert!(ui.preview(&workspace, Some(0)).is_none());
    // Escape stops protecting, then discards the candidates.
    assert!(ui.escape() && !ui.protecting);
    assert!(ui.found_of(&workspace, Some(0)).is_some());
    assert!(ui.escape());
    assert!(
        ui.found_of(&workspace, Some(0)).is_none() && ui.preview(&workspace, Some(0)).is_none()
    );
    assert!(!ui.escape());
    // Points changed: the candidates are out of date.
    found(&mut ui, &workspace, &preparation);
    assert!(OrientUi::remove_last(&mut workspace, 0));
    assert!(matches!(ui.found_of(&workspace, Some(0)), Some((_, false))));
    let _ = std::fs::remove_dir_all(folder);
}

#[test]
#[ignore = "orients a local large model; set KARASU_INTERFACE_MODEL (and KARASU_INTERFACE_WALL)"]
fn a_local_model_is_oriented() {
    let Ok(path) = std::env::var("KARASU_INTERFACE_MODEL") else {
        eprintln!("KARASU_INTERFACE_MODEL is not set: nothing to orient");
        return;
    };
    let hollowing =
        std::env::var("KARASU_INTERFACE_WALL")
            .ok()
            .map(|w| karasu_project::project::Hollowing {
                wall_mm: w.parse().unwrap(),
                ..crate::hollowing_ui::DEFAULT_HOLLOWING
            });
    let limit = std::time::Duration::from_secs(600);
    let (mut workspace, mut preparation) =
        crate::hollowing_ui::tests::placed(path.into(), hollowing, limit);
    workspace.edit(|plate| {
        plate.objects[0].pose.center = true;
        plate.objects[0].pose.drop = true;
        plate.objects[0].pose.lift_mm = Some(5.0);
    });
    crate::hollowing_ui::tests::prepared_within(&workspace, &mut preparation, limit);
    for (axes, limited) in [(Axes::Any, true), (Axes::Any, false)] {
        let mut ui = OrientUi::new();
        ui.values.axes = axes;
        ui.values.limited = limited;
        let started = Instant::now();
        found(&mut ui, &workspace, &preparation);
        let took = started.elapsed().as_secs_f64();
        let (candidates, _) = ui.found_of(&workspace, Some(0)).unwrap();
        println!(
            "{axes:?}, limited {limited}: {} candidates in {took:.2} s",
            candidates.len()
        );
        for c in candidates {
            println!(
                "  {:5.1}° facing {:7.1} mm² islands {:4} cups {:?} height {:6.2} mm largest {:7.1} mm²",
                c.tilt.angle_deg(),
                c.facing_mm2,
                c.islands,
                c.cups,
                c.height_mm,
                c.largest_mm2
            );
        }
    }
}
