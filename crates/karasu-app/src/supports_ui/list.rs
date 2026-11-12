//! The Supports tool's list window (owner, 2026-10-29): the selected model's supports, however
//! many, in a tool window opened from the tool strip (see `crate::list_window`). A row says
//! where the support stands and its state, its findings in its tooltip; the panel keeps the
//! selected support's values.
use super::{describe, errors, SupportsUi};
use crate::{
    appearance::Appearance,
    i18n::tr,
    list_window::{self, Event, Row, Spec},
    theme,
    workspace::Workspace,
};
use eframe::egui;
use karasu_core::supports::{
    BaseSpec, FindingKind, Severity, SupportDocument, SupportFinding, SupportKind, SupportOrigin,
    SupportScene,
};

/// What the list window lists (owner R8, 2026-11-10): every support, those with errors, or the
/// mini supports; the others are ghosts in the scene while it is open.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum ListFilter {
    #[default]
    All,
    Errors,
    Minis,
}

impl ListFilter {
    /// The filters in the window's choice order.
    const CHOICES: [Self; 3] = [Self::All, Self::Errors, Self::Minis];

    /// Whether a support, `wrong` when it has an error and `mini` when it is a mini support,
    /// is among those listed while the window is `open` and filters; `None` when it does not.
    pub(super) fn listed(self, open: bool, wrong: bool, mini: bool) -> Option<bool> {
        match self {
            _ if !open => None,
            Self::All => None,
            Self::Errors => Some(wrong),
            Self::Minis => Some(mini),
        }
    }
}

/// A finding's kind in a few words, for the state column; the full description is the row's
/// tooltip.
pub(super) fn short(kind: FindingKind) -> String {
    match kind {
        FindingKind::AnchorOffSurface => tr!("supports-short-off-surface"),
        FindingKind::AxisDeviation => tr!("supports-short-axis"),
        FindingKind::Piercing => tr!("supports-short-piercing"),
        FindingKind::Collision => tr!("supports-short-collision"),
        FindingKind::DrainHole => tr!("supports-short-drain-hole"),
        FindingKind::FootInHole => tr!("supports-short-foot-in-hole"),
        FindingKind::BuildVolume => tr!("supports-short-volume"),
        FindingKind::InvalidPath | FindingKind::InvalidParent => tr!("supports-short-path"),
        FindingKind::EmptyRaft => tr!("supports-short-raft"),
        FindingKind::SoftRange | FindingKind::SteepShaft | FindingKind::ContactFit => {
            tr!("list-ok")
        }
    }
}

/// Where support `id` of `scene` lies, its lowest and highest corners in world coordinates.
fn bounds(scene: &SupportScene, id: u64) -> Option<[[f64; 3]; 2]> {
    let mesh = scene.mesh();
    let (mut low, mut high) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
    for range in scene.part_ranges().iter().filter(|r| r.entity == id) {
        for triangle in &mesh.triangles()[range.triangles.clone()] {
            for &v in triangle {
                let p = mesh.vertices()[v as usize];
                for (k, c) in [p.x, p.y, p.z].into_iter().enumerate() {
                    low[k] = low[k].min(c);
                    high[k] = high[k].max(c);
                }
            }
        }
    }
    (low[0] <= high[0]).then_some([low, high])
}

/// Support `id`'s row: where it stands, its state from `scene`'s findings, its dot in the colour
/// the scene draws it (`selected` the selected support's identity).
fn row(
    scene: Option<&SupportScene>,
    document: Option<&SupportDocument>,
    appearance: &Appearance,
    selected: Option<u64>,
    id: u64,
) -> Row {
    let findings: Vec<&SupportFinding> = scene
        .map(|s| s.findings().iter().filter(|f| f.entity == id).collect())
        .unwrap_or_default();
    let error = findings.iter().find(|f| f.severity == Severity::Error);
    let accepted = findings.iter().any(|f| f.severity == Severity::Accepted);
    let interior = scene
        .and_then(|s| s.supports().iter().find(|b| b.id == id))
        .is_some_and(|b| b.interior_tip);
    let support = document.and_then(|d| d.supports.iter().find(|s| s.id == id));
    let fact = match support.map(|s| &s.base) {
        _ if interior => tr!("supports-stands-cavity"),
        Some(BaseSpec::Model { .. }) => tr!("supports-stands-model"),
        Some(BaseSpec::Support { .. }) => tr!("supports-stands-support"),
        _ => tr!("supports-stands-plate"),
    };
    // One the Islands tool placed says so, and a mini support.
    let fact = if support.is_some_and(|s| s.origin == SupportOrigin::Islands) {
        tr!("supports-from-islands", stands = fact)
    } else {
        fact
    };
    let fact = if support.is_some_and(|s| s.kind == SupportKind::Mini) {
        tr!("supports-mini-row", stands = fact)
    } else {
        fact
    };
    // A support in a drain hole is only a warning (TD-131), but the list still says it.
    let in_hole = findings.iter().any(|f| f.kind == FindingKind::DrainHole);
    let (state, state_colour) = match error {
        Some(f) => (short(f.kind), theme::ERROR.color()),
        None if accepted => (tr!("supports-state-accepted"), theme::WARNING.color()),
        None if in_hole => (short(FindingKind::DrainHole), theme::WARNING.color()),
        None => (tr!("list-ok"), theme::TEXT_PRIMARY.color()),
    };
    let chosen = Some(id) == selected;
    let dot = match (error.is_some(), chosen) {
        (true, true) => appearance.support_error_selected,
        (true, false) => appearance.support_error,
        (false, true) => appearance.support_selected,
        (false, false) => appearance.support,
    };
    let hint = if findings.is_empty() {
        tr!("list-row-hint")
    } else {
        findings
            .iter()
            .map(|f| describe(f))
            .collect::<Vec<_>>()
            .join("\n")
    };
    Row {
        dot: dot.color(),
        name: tr!("list-number", number = id),
        fact,
        state,
        state_colour,
        hint,
        selected: chosen,
    }
}

impl SupportsUi {
    /// The list window of model `index`'s supports, while open. Returns where the view should
    /// go, after a double click on a row.
    pub fn list_window(
        &mut self,
        ctx: &egui::Context,
        workspace: &mut Workspace,
        index: Option<usize>,
        appearance: &Appearance,
    ) -> Option<[[f64; 3]; 2]> {
        if !self.list_open {
            return None;
        }
        let index = index.filter(|&i| i < workspace.plate().objects.len());
        let document = index.and_then(|i| workspace.supports(i));
        let ids: Vec<u64> =
            document.map_or_else(Vec::new, |d| d.supports.iter().map(|s| s.id).collect());
        let scene = index
            .and_then(|i| self.builds.get(&i))
            .and_then(|b| b.scene.as_ref().ok());
        let wrong = scene.map_or_else(Vec::new, |s| errors(s.findings()));
        let minis: Vec<u64> = document.map_or_else(Vec::new, |d| {
            d.supports
                .iter()
                .filter(|s| s.kind == SupportKind::Mini)
                .map(|s| s.id)
                .collect()
        });
        let items: Vec<u64> = match self.list_filter {
            ListFilter::All => ids.clone(),
            ListFilter::Errors => ids
                .iter()
                .copied()
                .filter(|id| wrong.binary_search(id).is_ok())
                .collect(),
            ListFilter::Minis => minis.clone(),
        };
        let selected = self
            .selected
            .filter(|&(object, _)| Some(object) == index)
            .map(|(_, id)| id);
        let reveal = std::mem::take(&mut self.reveal)
            .then_some(selected)
            .flatten();
        let spec = Spec {
            id: "supports-list",
            title: tr!("supports-list-title", count = ids.len()),
            fact: tr!("supports-list-stands"),
            state: tr!("list-state"),
            filters: vec![
                tr!("list-all"),
                tr!("supports-list-errors", count = wrong.len()),
                tr!("supports-list-minis", count = minis.len()),
            ],
            summary: if index.is_some() {
                tr!(
                    "supports-list-summary",
                    count = ids.len(),
                    errors = wrong.len()
                )
            } else {
                tr!("panel-no-model")
            },
            tabs: Vec::new(),
            places: Vec::new(),
            remove: Some(tr!("supports-remove")),
        };
        let row = |id| row(scene, document, appearance, selected, id);
        let mut filter = ListFilter::CHOICES
            .iter()
            .position(|&f| f == self.list_filter)
            .unwrap_or(0);
        let event = list_window::show(
            ctx,
            &spec,
            &mut self.list_open,
            &mut 0,
            &mut filter,
            &mut 0,
            &items,
            reveal,
            row,
        );
        self.list_filter = ListFilter::CHOICES.get(filter).copied().unwrap_or_default();
        let object = index?;
        match event? {
            Event::Picked(id) => {
                self.selected = Some((object, id));
                self.active = None;
                None
            }
            Event::Focused(id) => {
                self.selected = Some((object, id));
                self.active = None;
                self.builds
                    .get(&object)
                    .and_then(|b| b.scene.as_ref().ok())
                    .and_then(|s| bounds(s, id))
            }
            Event::Removed(id) => {
                self.remove(workspace, object, id);
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::supports_ui::tests::{built, lifted_cube, ray};
    use karasu_core::supports::AxisSpec;

    #[test]
    fn the_filters_list_every_support_the_wrong_ones_or_the_minis() {
        use ListFilter::{All, Errors, Minis};
        // Closed, or with every support listed, nothing is left out.
        for filter in ListFilter::CHOICES {
            assert_eq!(filter.listed(false, true, true), None);
        }
        assert_eq!(All.listed(true, true, false), None);
        assert_eq!(Errors.listed(true, true, false), Some(true));
        assert_eq!(Errors.listed(true, false, true), Some(false));
        assert_eq!(Minis.listed(true, false, true), Some(true));
        assert_eq!(Minis.listed(true, true, false), Some(false));
        assert_eq!(ListFilter::CHOICES, [All, Errors, Minis]);
    }

    #[test]
    fn a_row_says_where_its_support_stands_and_its_first_error() {
        let (mut workspace, preparation, directory) = lifted_cube();
        let mut ui = SupportsUi {
            placing: true,
            ..SupportsUi::default()
        };
        let placed = workspace.placement_of(0).unwrap();
        let [cx, cy] =
            [0, 1].map(|k| f64::midpoint(placed.min.coordinates()[k], placed.max.coordinates()[k]));
        let under = ray(cx, cy, true);
        let hit = workspace.hit(&under);
        assert!(ui.click(&mut workspace, &under, hit, 0));
        ui.placing = false;
        // Placed in the scene: the list scrolls to it once.
        assert!(ui.reveal);
        built(&mut ui, &workspace, &preparation);
        let appearance = Appearance::default();
        let id = workspace.supports(0).unwrap().supports[0].id;
        let scene = ui.builds[&0].scene.as_ref().ok();
        let shown = row(scene, workspace.supports(0), &appearance, Some(id), id);
        assert_eq!(shown.fact, tr!("supports-stands-plate"));
        assert_eq!(shown.state, tr!("list-ok"));
        assert_eq!(shown.dot, appearance.support_selected.color());
        assert!(shown.selected);
        // Its tip turned past the limit: the row says so, red.
        let mut document = workspace.supports(0).unwrap().clone();
        document.supports[0].tip_axis = AxisSpec::Direction([1.0, 0.0, 0.0]);
        workspace.set_supports(0, Some(document), None);
        built(&mut ui, &workspace, &preparation);
        let scene = ui.builds[&0].scene.as_ref().ok();
        let shown = row(scene, workspace.supports(0), &appearance, None, id);
        assert_eq!(shown.state, tr!("supports-short-axis"));
        assert_eq!(shown.dot, appearance.support_error.color());
        assert!(shown.hint.contains(&tr!("supports-finding-axis")));
        // The support's parts are where the view goes on a double click.
        let [low, high] = bounds(ui.builds[&0].scene.as_ref().unwrap(), id).unwrap();
        assert!(low[2] < high[2] && low[2] >= 0.0);
        // Closed, the window shows nothing and asks for nothing.
        let ctx = egui::Context::default();
        assert!(ui
            .list_window(&ctx, &mut workspace, Some(0), &appearance)
            .is_none());
        let _ = std::fs::remove_dir_all(directory);
    }
}
