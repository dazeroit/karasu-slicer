//! Accessibility checks of the main window in every tool and dialog (TD-142); see
//! [`crate::accessibility_check`].
use super::*;
use crate::accessibility_check::failures;

/// The failures of `window` as it is.
fn window_failures(window: &mut MainWindow) -> Vec<String> {
    let frame = eframe::Frame::_new_kittest();
    failures(|ui| window.show(ui, &frame, None))
}

/// A window with one model placed and selected; its folder, removed by the caller.
fn with_model(name: &str) -> (MainWindow, std::path::PathBuf) {
    let (workspace, preparation, folder) = crate::grid_ui::tests::lifted_slab(name);
    let mut window = MainWindow::new(Vec::new());
    window.workspace = workspace;
    window.preparation = preparation;
    window.selected = Some(0);
    (window, folder)
}

#[test]
fn every_tool_names_its_controls_and_tab_reaches_only_visible_named_ones() {
    let mut report = Vec::new();
    let mut empty = MainWindow::new(Vec::new());
    report.extend(
        window_failures(&mut empty)
            .into_iter()
            .map(|f| format!("empty: {f}")),
    );
    let (mut window, folder) = with_model("a11y-tools");
    report.extend(
        window_failures(&mut window)
            .into_iter()
            .map(|f| format!("Select: {f}")),
    );
    for tool in [
        Tool::Transform,
        Tool::Orient,
        Tool::Repair,
        Tool::Hollow,
        Tool::DrainHoles,
        Tool::Automatic,
        Tool::Supports,
        Tool::Islands,
        Tool::Braces,
        Tool::Raft,
        Tool::Slice,
        Tool::Layers,
    ] {
        window.perform(Action::Tool(tool));
        report.extend(
            window_failures(&mut window)
                .into_iter()
                .map(|f| format!("{tool:?}: {f}")),
        );
    }
    // The tools' list windows.
    for tool in [
        Tool::Automatic,
        Tool::Supports,
        Tool::Islands,
        Tool::DrainHoles,
    ] {
        window.perform(Action::Tool(tool));
        window.perform(Action::ToggleList);
        report.extend(
            window_failures(&mut window)
                .into_iter()
                .map(|f| format!("{tool:?} list: {f}")),
        );
        window.perform(Action::ToggleList);
    }
    let _ = std::fs::remove_dir_all(folder);
    assert!(report.is_empty(), "{}", report.join("\n"));
}

#[test]
fn every_dialog_names_its_controls_and_a_modal_keeps_the_focus() {
    let mut report = Vec::new();
    for action in [
        Action::OpenSettings,
        Action::EditProfiles,
        Action::PlatesOverview,
        Action::ExportModels,
        Action::ToggleLog,
        Action::ToggleMessages,
        Action::ArrangeModels,
        Action::ExportEveryPlate,
        Action::RenamePlate,
        Action::Delete,
        Action::About,
    ] {
        let (mut window, folder) = with_model(&format!("a11y-{action:?}"));
        window.perform(action);
        report.extend(
            window_failures(&mut window)
                .into_iter()
                .map(|f| format!("{action:?}: {f}")),
        );
        let _ = std::fs::remove_dir_all(folder);
    }
    // Clone, from a model's menu.
    let (mut window, folder) = with_model("a11y-clone");
    window.open_clone(0);
    report.extend(
        window_failures(&mut window)
            .into_iter()
            .map(|f| format!("Clone: {f}")),
    );
    let _ = std::fs::remove_dir_all(folder);
    // A plate's deletion asks first: with two plates.
    let (mut window, folder) = with_model("a11y-delete-plate");
    window.perform(Action::NewPlate);
    window.perform(Action::DeletePlate);
    report.extend(
        window_failures(&mut window)
            .into_iter()
            .map(|f| format!("DeletePlate: {f}")),
    );
    let _ = std::fs::remove_dir_all(folder);
    assert!(report.is_empty(), "{}", report.join("\n"));
}

#[test]
fn a_tool_window_that_opens_takes_the_focus_and_the_3d_view_takes_none() {
    let (mut window, folder) = with_model("a11y-open");
    let ctx = egui::Context::default();
    let frame = eframe::Frame::_new_kittest();
    let (nodes, focused) =
        crate::accessibility_check::settled(&ctx, |ui| window.show(ui, &frame, None));
    assert!(focused.is_none(), "{focused:?}");
    // The 3D view is named but takes no focus: the camera has no keys (owner, 2026-11-10).
    let view = nodes
        .iter()
        .find(|(_, n)| n.label() == Some(tr!("a11y-viewport").as_str()))
        .map(|(_, n)| n)
        .expect("the 3D view's node");
    assert!(!view.supports_action(egui::accesskit::Action::Focus));
    for action in [
        Action::PlatesOverview,
        Action::ToggleMessages,
        Action::ToggleLog,
    ] {
        window.perform(action);
        let (_, focused) =
            crate::accessibility_check::settled(&ctx, |ui| window.show(ui, &frame, None));
        let (_, layer, _) = focused.unwrap_or_else(|| panic!("{action:?}: no focus"));
        assert_eq!(layer.order, egui::Order::Middle, "{action:?}");
        window.perform(action);
        crate::accessibility_check::settled(&ctx, |ui| window.show(ui, &frame, None));
    }
    let _ = std::fs::remove_dir_all(folder);
}
