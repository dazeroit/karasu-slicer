//! The final check (M6.4i, TD-112) is headless
//! ([`karasu_project::support_plan::islands::check`]); this gives it the model as the
//! application holds it.
use crate::{preparation_ui::PreparationUi, supports_ui::SupportsUi, workspace::Workspace};
pub use karasu_project::support_plan::islands::check::{holds, ModelInput};
use std::sync::Arc;

/// Model `index` as a slice builds its supports, for the final check and braces: `None` until
/// it is placed and prepared. The braces take `braces`.
pub(crate) fn model_input(
    workspace: &Workspace,
    preparation: &PreparationUi,
    supports: &SupportsUi,
    index: usize,
) -> Option<ModelInput> {
    let object = workspace.plate().objects.get(index)?;
    let (shared, _) = preparation.shared(workspace, index)?;
    let loaded = workspace.meshes().get(&object.model)?;
    Some(ModelInput {
        mesh: Arc::clone(&loaded.mesh),
        placement: workspace.placement_of(index)?.transform,
        preparation: shared,
        index,
        printer: workspace.printer()?.clone(),
        braces: supports.bracing.new.plan(),
    })
}
