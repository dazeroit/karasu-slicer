//! The workflow bar (M6.5, TD-098): the workflow's steps and the chosen step's tools.
//!
//! **Steps** (Prepare, Hollowing, Supports, Export) stay lit when chosen. Prepare starts on
//! Select, a neutral tool: objects are selected as with any tool, but no handles or panel show. The chosen step shows
//! its tools below a fading line, and choosing a tool shows its panel at the viewport's right.
//! Each step remembers its tool. Both rows are centred.
//!
//! **Collapsed**, the bar keeps the steps' row and names the step and tool in use at its left,
//! so the user always knows where they are.
//!
//! **Only tools that exist are offered.** A step without tools yet is shown disabled, with a
//! note saying they come later.
//!
//! Steps, tools and the collapse are actions ([`Action`]): the bar only reports what was
//! clicked, and the window performs it.
use crate::{
    actions::Action,
    i18n::tr,
    icons::{Icon, Icons},
    theme,
};
use eframe::egui;

/// A step of the workflow.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Step {
    Prepare,
    Hollowing,
    Supports,
    Export,
}

/// A tool of a step, which shows its panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Tool {
    /// Selecting objects, with no handles and no panel: the neutral tool (owner, M6.5).
    Select,
    /// The selected object's handles: move, turn and scale.
    Transform,
    /// Tilts of the selected object proposed under constraints (M6.4k, TD-119).
    Orient,
    /// The selected object's repairs, and the plate's.
    Repair,
    /// Hollowing the selected object.
    Hollow,
    /// Drain holes in the selected object.
    DrainHoles,
    /// Every support of the selected object planned at once (M6.4i, TD-112).
    Automatic,
    /// Supports under the selected object (TD-108).
    Supports,
    /// What the selected object's print starts in mid-air, and the supports that hold it
    /// (owner, 2026-10-29).
    Islands,
    /// Braces between the selected object's supports (owner, 2026-10-29).
    Braces,
    /// The selected object's base: its lift and the raft under its supports (TD-108).
    Raft,
    /// The output format, slicing and export.
    Slice,
    /// The sliced layers, one at a time (M6.5, TD-103).
    Layers,
}

impl Step {
    /// The steps, in the workflow's order.
    pub const ALL: [Self; 4] = [Self::Prepare, Self::Hollowing, Self::Supports, Self::Export];

    /// The step's name.
    #[must_use]
    pub fn label(self) -> String {
        match self {
            Self::Prepare => tr!("workflow-prepare"),
            Self::Hollowing => tr!("workflow-hollowing"),
            Self::Supports => tr!("workflow-supports"),
            Self::Export => tr!("workflow-export"),
        }
    }

    /// The step's tools, in the bar's order; none while they are not built.
    #[must_use]
    pub fn tools(self) -> &'static [Tool] {
        match self {
            Self::Prepare => &[Tool::Select, Tool::Transform, Tool::Orient, Tool::Repair],
            Self::Hollowing => &[Tool::Hollow, Tool::DrainHoles],
            Self::Supports => &[
                Tool::Automatic,
                Tool::Supports,
                Tool::Islands,
                Tool::Braces,
                Tool::Raft,
            ],
            Self::Export => &[Tool::Slice, Tool::Layers],
        }
    }

    fn index(self) -> usize {
        match self {
            Self::Prepare => 0,
            Self::Hollowing => 1,
            Self::Supports => 2,
            Self::Export => 3,
        }
    }
}

impl Tool {
    /// Every tool, in the workflow bar's order.
    pub const ALL: [Self; 13] = [
        Self::Select,
        Self::Transform,
        Self::Orient,
        Self::Repair,
        Self::Hollow,
        Self::DrainHoles,
        Self::Automatic,
        Self::Supports,
        Self::Islands,
        Self::Braces,
        Self::Raft,
        Self::Slice,
        Self::Layers,
    ];

    /// The tool's name.
    #[must_use]
    pub fn label(self) -> String {
        match self {
            Self::Select => tr!("tool-select"),
            Self::Transform => tr!("tool-transform"),
            Self::Orient => tr!("tool-orient"),
            Self::Repair => tr!("tool-repair"),
            Self::Hollow => tr!("tool-hollow"),
            Self::DrainHoles => tr!("tool-drain-holes"),
            Self::Automatic => tr!("tool-automatic"),
            Self::Supports => tr!("tool-supports"),
            Self::Islands => tr!("tool-islands"),
            Self::Braces => tr!("tool-braces"),
            Self::Raft => tr!("tool-raft"),
            Self::Slice => tr!("tool-slice"),
            Self::Layers => tr!("tool-layers"),
        }
    }

    /// The step the tool belongs to.
    #[must_use]
    pub fn step(self) -> Step {
        match self {
            Self::Select | Self::Transform | Self::Orient | Self::Repair => Step::Prepare,
            Self::Hollow | Self::DrainHoles => Step::Hollowing,
            Self::Automatic | Self::Supports | Self::Islands | Self::Braces | Self::Raft => {
                Step::Supports
            }
            Self::Slice | Self::Layers => Step::Export,
        }
    }

    /// The tool's icon.
    #[must_use]
    pub fn icon(self) -> Icon {
        match self {
            Self::Select => Icon::Select,
            Self::Transform => Icon::Transform,
            Self::Orient => Icon::Orient,
            Self::Repair => Icon::Repair,
            Self::Hollow => Icon::Hollow,
            Self::DrainHoles => Icon::DrainHoles,
            // Swapped (owner, 2026-10-30): the plain support for the tool that plans them all,
            // the support with a plus for the one that adds them by hand.
            Self::Automatic => Icon::Supports,
            Self::Supports => Icon::Automatic,
            Self::Islands => Icon::Islands,
            Self::Braces => Icon::Braces,
            Self::Raft => Icon::Raft,
            Self::Slice => Icon::Slice,
            Self::Layers => Icon::Layers,
        }
    }

    /// Whether the tool shows a panel: every tool but Select.
    #[must_use]
    pub fn has_panel(self) -> bool {
        self != Self::Select
    }
}

/// The bar's state: the chosen step, each step's chosen tool, and whether the tools show.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Workflow {
    step: Step,
    tools: [Option<Tool>; 4],
    tools_shown: bool,
}

impl Default for Workflow {
    /// Prepare, each step on its first tool, tools shown.
    fn default() -> Self {
        Self {
            step: Step::Prepare,
            tools: Step::ALL.map(|s| s.tools().first().copied()),
            tools_shown: true,
        }
    }
}

/// The steps' row's height, in points.
const STEP_ROW: f32 = 38.0;
/// A step's button: its least size, and its corners rounded into a pill (owner, M6.5).
const STEP_BUTTON: egui::Vec2 = egui::vec2(104.0, 32.0);
/// A tool's button: a square, the same for every tool whatever its label (owner, M6.5).
const TOOL_BUTTON: f32 = 76.0;
/// A tool's icon's side, in points.
const TOOL_ICON: f32 = 32.0;
/// The tools' row's height, in points.
const TOOL_ROW: f32 = TOOL_BUTTON + 4.0;

impl Workflow {
    /// The chosen step's tool, whose panel shows; none for a step without tools.
    #[must_use]
    pub fn tool(&self) -> Option<Tool> {
        self.tools[self.step.index()]
    }

    /// Choose `step`, with the tool it had.
    pub fn choose_step(&mut self, step: Step) {
        self.step = step;
    }

    /// Choose `tool`, and its step.
    pub fn choose_tool(&mut self, tool: Tool) {
        self.step = tool.step();
        self.tools[self.step.index()] = Some(tool);
    }

    /// Whether the bar shows the step's tools (a user setting).
    pub fn tools_shown(&mut self) -> &mut bool {
        &mut self.tools_shown
    }

    /// Show or hide the tools' row.
    pub fn toggle_tools(&mut self) {
        self.tools_shown = !self.tools_shown;
    }

    /// Where the user is, as the collapsed bar names it: the step, and its tool.
    #[must_use]
    pub fn crumb(&self) -> String {
        match self.tool() {
            Some(tool) => format!("{}  ›  {}", self.step.label(), tool.label()),
            None => self.step.label(),
        }
    }

    /// Draw the bar, with the tools' `icons` once loaded; returns the action clicked, if any.
    pub fn bar(
        &self,
        ui: &mut egui::Ui,
        toggle_hint: &str,
        icons: Option<&Icons>,
    ) -> Option<Action> {
        let mut action = None;
        let (line, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), STEP_ROW),
            egui::Sense::hover(),
        );
        centred(ui, line, egui::Id::new("workflow-steps"), |ui| {
            for step in Step::ALL {
                let button = egui::Button::selectable(
                    self.step == step,
                    egui::RichText::new(step.label()).size(14.0).strong(),
                )
                .min_size(STEP_BUTTON)
                .corner_radius(STEP_BUTTON.y / 2.0);
                let response = ui
                    .add_enabled(!step.tools().is_empty(), button)
                    .on_disabled_hover_text(tr!("workflow-step-later"));
                if response.clicked() {
                    action = Some(Action::Step(step));
                }
            }
        });
        let mut right = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(line)
                .layout(egui::Layout::right_to_left(egui::Align::Center)),
        );
        let label = if self.tools_shown {
            tr!("workflow-hide-tools")
        } else {
            tr!("workflow-show-tools")
        };
        if right.button(label).on_hover_text(toggle_hint).clicked() {
            action = Some(Action::ToggleTools);
        }
        if !self.tools_shown {
            let mut left = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(line)
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
            );
            left.label(
                egui::RichText::new(self.crumb())
                    .strong()
                    .color(theme::ACCENT.color()),
            );
            return action;
        }
        ui.add_space(6.0);
        let (rule, _) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
        fading_rule(ui.painter(), rule, theme::BORDER.color());
        ui.add_space(6.0);
        let (line, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), TOOL_ROW),
            egui::Sense::hover(),
        );
        let id = egui::Id::new(("workflow-tools", self.step));
        centred(ui, line, id, |ui| {
            for &tool in self.step.tools() {
                let chosen = self.tool() == Some(tool);
                if tool_button(ui, chosen, &tool.label(), icons.map(|i| (i, tool.icon()))).clicked()
                {
                    action = Some(Action::Tool(tool));
                }
            }
        });
        action
    }
}

/// A tool's square button: its icon in the middle, tinted as the label, and the label in capitals
/// at the bottom; lit when `selected`.
fn tool_button(
    ui: &mut egui::Ui,
    selected: bool,
    label: &str,
    icon: Option<(&Icons, Icon)>,
) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(egui::Vec2::splat(TOOL_BUTTON), egui::Sense::click());
    let visuals = ui.style().interact_selectable(&response, selected);
    let painter = ui.painter();
    painter.rect(
        rect,
        8.0,
        visuals.weak_bg_fill,
        visuals.bg_stroke,
        egui::StrokeKind::Inside,
    );
    // Every line of the label centred, a wrapped one too.
    let mut job = egui::text::LayoutJob::simple(
        label.to_uppercase(),
        egui::FontId::proportional(10.0),
        visuals.text_color(),
        rect.width() - 8.0,
    );
    job.halign = egui::Align::Center;
    let galley = painter.layout_job(job);
    // A centred galley's lines are laid out about x = 0: it is placed by its centre.
    let at = egui::pos2(rect.center().x, rect.bottom() - 8.0 - galley.size().y);
    if let Some((icons, icon)) = icon {
        let above = egui::Rect::from_min_max(rect.min, egui::pos2(rect.right(), at.y - 2.0));
        let square = egui::Rect::from_center_size(above.center(), egui::Vec2::splat(TOOL_ICON));
        icons.paint(painter, icon, square, visuals.text_color());
    }
    painter.galley(at, galley, visuals.text_color());
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, label)
    });
    response
}

/// Lay out a row centred in `line`. egui lays a row out from its left, so the row's width is
/// kept (under `id`) from the frame before and the row starts half the spare width in; a new
/// width takes one more frame to settle.
fn centred(ui: &mut egui::Ui, line: egui::Rect, id: egui::Id, add: impl FnOnce(&mut egui::Ui)) {
    let width: f32 = ui.ctx().data(|d| d.get_temp(id)).unwrap_or(0.0);
    let left = line.left() + ((line.width() - width) / 2.0).max(0.0);
    let mut row = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(egui::Rect::from_min_max(
                egui::pos2(left, line.top()),
                line.max,
            ))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    add(&mut row);
    let used = row.min_rect().width();
    if (used - width).abs() > 0.5 {
        ui.ctx().data_mut(|d| d.insert_temp(id, used));
        ui.ctx().request_repaint();
    }
}

/// A one-point line across `rect` in `colour`, fading out over a fifth of its length at each
/// end. Drawn as a mesh, so the fade is per vertex, with no texture.
fn fading_rule(painter: &egui::Painter, rect: egui::Rect, colour: egui::Color32) {
    let stops = [
        (0.0, egui::Color32::TRANSPARENT),
        (0.2, colour),
        (0.8, colour),
        (1.0, egui::Color32::TRANSPARENT),
    ];
    let mut mesh = egui::Mesh::default();
    for (at, colour) in stops {
        let x = rect.left() + rect.width() * at;
        mesh.colored_vertex(egui::pos2(x, rect.top()), colour);
        mesh.colored_vertex(egui::pos2(x, rect.bottom()), colour);
    }
    for k in 0..3u32 {
        let i = 2 * k;
        mesh.add_triangle(i, i + 1, i + 2);
        mesh.add_triangle(i + 1, i + 3, i + 2);
    }
    painter.add(egui::Shape::mesh(mesh));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_keep_their_tool_and_tools_choose_their_step() {
        let mut workflow = Workflow::default();
        // Prepare starts on Select, the neutral tool without handles or panel.
        assert_eq!(
            (workflow.step, workflow.tool()),
            (Step::Prepare, Some(Tool::Select))
        );
        assert!(!Tool::Select.has_panel() && Tool::Transform.has_panel());
        workflow.choose_tool(Tool::Transform);
        workflow.choose_tool(Tool::DrainHoles);
        assert_eq!(workflow.step, Step::Hollowing);
        workflow.choose_step(Step::Prepare);
        assert_eq!(workflow.tool(), Some(Tool::Transform));
        // Back to Hollowing, the step still has its drain holes.
        workflow.choose_step(Step::Hollowing);
        assert_eq!(workflow.tool(), Some(Tool::DrainHoles));
        // The Supports step starts on its first tool, Automatic (M6.4i).
        workflow.choose_step(Step::Supports);
        assert_eq!(workflow.tool(), Some(Tool::Automatic));
    }

    #[test]
    fn every_tool_belongs_to_the_step_that_lists_it() {
        for step in Step::ALL {
            for &tool in step.tools() {
                assert_eq!(tool.step(), step, "{tool:?}");
                assert_ne!(tool.label(), "");
            }
        }
    }

    /// Shapes and the clicked action of the bar drawn headless, over two frames (the rows
    /// centre on the second).
    fn draw(workflow: &Workflow) -> (usize, Option<Action>) {
        let ctx = egui::Context::default();
        let mut result = (0, None);
        for _ in 0..2 {
            let mut action = None;
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                action = workflow.bar(ui, "", None);
            });
            output.textures_delta.clear();
            result = (output.shapes.len(), action);
        }
        result
    }

    #[test]
    fn the_bar_draws_open_and_collapsed_without_acting_by_itself() {
        let mut workflow = Workflow::default();
        let (open, action) = draw(&workflow);
        assert_eq!(action, None);
        workflow.toggle_tools();
        let (collapsed, action) = draw(&workflow);
        assert_eq!(action, None);
        // Collapsed, the tools' row is gone: fewer shapes than open.
        assert!(collapsed < open, "{collapsed} < {open}");
    }

    #[test]
    fn a_wrapped_label_is_centred_line_by_line() {
        // "SLICE AND EXPORT" takes two lines in a tool's square; both centre on the button.
        let ctx = egui::Context::default();
        let mut rows = Vec::new();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let mut job = egui::text::LayoutJob::simple(
                "SLICE AND EXPORT".to_owned(),
                egui::FontId::proportional(10.0),
                egui::Color32::WHITE,
                TOOL_BUTTON - 8.0,
            );
            job.halign = egui::Align::Center;
            let galley = ui.painter().layout_job(job);
            rows = galley.rows.iter().map(|r| r.rect().center().x).collect();
        });
        output.textures_delta.clear();
        assert_eq!(rows.len(), 2);
        for centre in rows {
            assert!(centre.abs() < 1.0, "{centre}");
        }
    }

    #[test]
    fn collapsing_keeps_the_crumb_of_where_the_user_is() {
        let mut workflow = Workflow::default();
        workflow.toggle_tools();
        assert!(!workflow.tools_shown);
        workflow.choose_tool(Tool::Hollow);
        let crumb = workflow.crumb();
        assert!(
            crumb.starts_with(&tr!("workflow-hollowing")) && crumb.ends_with(&tr!("tool-hollow")),
            "{crumb}"
        );
    }
}
