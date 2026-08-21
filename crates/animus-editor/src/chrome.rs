//! The frame around the work: title bar and workflow navigator.
//!
//! The dock gives the operator *panels*. This module gives them the three
//! things a panel cannot: what show they are in, **which of two modes they are
//! working in**, and where in building the puppet they have got to.
//!
//! Follows the `Animus Live — Perform` design comp and the M1 UI audit. The
//! audit's first finding is the one that shapes this file: building a puppet,
//! animating it and running a show were all visible at once with no strong
//! boundary between them, and the most consequential distinction in the whole
//! editor — does dragging a joint change the saved rig, or only pull the live
//! puppet — lived in a small toggle inside a side panel. It is now the single
//! largest control in the chrome.
//!
//! The Signal Rule still governs: coral means the audience can see it or it is
//! being recorded, green means running or ready, cyan means input and output
//! data, amber means caution.

use animus_core::doc::{Project, PuppetKind};
use bevy_egui::egui;

use crate::dock::OutputInfo;
use crate::state::{EditMode, EditorState, Tool};
use crate::theme;

pub const TITLE_HEIGHT: f32 = 44.0;
pub const STAGE_HEIGHT: f32 = 40.0;

// ── the three modes ────────────────────────────────────────────────────

/// What the operator calls the mode they are in.
///
/// The engine names these `Rig`, `Edit` and `Live` because that is what they
/// mean to the solver. The operator gets the words for what they are *doing*.
pub fn mode_label(mode: EditMode) -> &'static str {
    Stage::of(mode).label()
}

/// The sentence beside the switch, shown always and in every mode.
///
/// Persistent rather than a tooltip: the point is that the operator never has
/// to remember which mode they are in, and a hint you must hover for is a hint
/// you have to go looking for.
pub fn mode_sentence(mode: EditMode) -> &'static str {
    Stage::of(mode).hint()
}

/// What the viewport calls itself in each mode.
pub fn stage_badge(mode: EditMode) -> &'static str {
    match mode {
        EditMode::Rig => "WORKBENCH · REST POSE",
        EditMode::Edit => "STEP EDITOR",
        EditMode::Live => "STAGE · AUDIENCE VIEW",
    }
}

/// The instruction over the viewport, telling the operator what a drag will
/// do right now.
///
/// Three modes, three different answers to the same gesture — which is
/// exactly why the sentence is on the stage and not only in the chrome.
pub fn viewport_instruction(mode: EditMode) -> (&'static str, &'static str) {
    match mode {
        EditMode::Rig => (
            "RIG",
            "Drag a joint to move its rest position. This changes the saved rig.",
        ),
        EditMode::Edit => (
            "POSE",
            "Drag to pose the puppet. The selected step keeps whatever you leave it in.",
        ),
        EditMode::Live => (
            "PULL",
            "Drag a joint to move the live puppet. Nothing is written.",
        ),
    }
}

// ── the three stages ───────────────────────────────────────────────────

/// Where in making a show the operator is.
///
/// **The stage and the mode are the same thing.** They were two controls —
/// a five-step workflow strip and a BUILD/PERFORM switch — describing
/// overlapping ideas, which meant the operator had to keep both in their head
/// and reconcile them. One control, three stages, and the mode *is* the
/// stage.
///
/// Import and Mesh are gone from the strip on purpose. Importing an image is
/// importing; there is nothing to navigate to. The mesh is built at import and
/// adjusted while rigging. A step that is never a destination is a step that
/// only takes up room.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Rig,
    Edit,
    Perform,
}

pub const STAGES: [Stage; 3] = [Stage::Rig, Stage::Edit, Stage::Perform];

impl Stage {
    pub fn mode(self) -> EditMode {
        match self {
            Stage::Rig => EditMode::Rig,
            Stage::Edit => EditMode::Edit,
            Stage::Perform => EditMode::Live,
        }
    }

    pub fn of(mode: EditMode) -> Stage {
        match mode {
            EditMode::Rig => Stage::Rig,
            EditMode::Edit => Stage::Edit,
            EditMode::Live => Stage::Perform,
        }
    }

    /// The tool this stage arms. Rigging starts with the joint tool because
    /// that is the first thing an unrigged puppet needs; the other two have
    /// nothing to author, so they hold the hand.
    pub fn tool(self) -> Tool {
        match self {
            Stage::Rig => Tool::Joint,
            Stage::Edit | Stage::Perform => Tool::Select,
        }
    }

    pub fn number(self) -> u8 {
        match self {
            Stage::Rig => 1,
            Stage::Edit => 2,
            Stage::Perform => 3,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Stage::Rig => "RIG",
            Stage::Edit => "EDIT",
            Stage::Perform => "PERFORM",
        }
    }

    /// One line of "what am I doing here", shown on hover and beside the
    /// switch.
    pub fn hint(self) -> &'static str {
        match self {
            Stage::Rig => "Place joints and bones. Dragging moves a joint's saved rest position.",
            Stage::Edit => "Pose the puppet into the selected step. The rig is not touched.",
            Stage::Perform => {
                "Run the pattern. Grab a limb to take it over; let go to hand it back."
            }
        }
    }

    /// What the document must contain before this stage is worth entering.
    pub fn requirement(self) -> &'static str {
        match self {
            Stage::Rig => "Drop a PNG on the viewport to begin.",
            Stage::Edit => "Rig the puppet first: at least two joints and one bone.",
            Stage::Perform => "Pose at least one step, or arm record and perform into the grid.",
        }
    }

    /// Whether the document satisfies this stage's exit condition.
    pub fn complete(self, project: &Project) -> bool {
        let meshes = || {
            project.puppets.values().filter_map(|p| match &p.kind {
                PuppetKind::Mesh(m) => Some(m),
                _ => None,
            })
        };
        match self {
            Stage::Rig => {
                meshes().any(|m| m.skeleton.joints.len() >= 2 && !m.skeleton.bones.is_empty())
            }
            // Neither of these is ever "done": a pattern is never finished
            // and a show is never over. A tick on them would be a claim about
            // the performance rather than about the document.
            Stage::Edit | Stage::Perform => false,
        }
    }

    /// Whether the operator may go here at all.
    pub fn available(self, project: &Project) -> bool {
        match self {
            Stage::Rig => true,
            // Posing and performing an unrigged puppet would be posing
            // nothing: there are no joints to move.
            Stage::Edit | Stage::Perform => Stage::Rig.complete(project),
        }
    }
}

/// What the document contains, for the navigator's right-hand readout.
pub fn puppet_summary(project: &Project) -> String {
    let Some((name, mesh)) = project.puppets.values().find_map(|p| match &p.kind {
        PuppetKind::Mesh(m) => Some((p.name.as_str(), m)),
        _ => None,
    }) else {
        return "No puppet yet".to_string();
    };
    format!(
        "{name} · {} joints · {} bones · {} tris",
        mesh.skeleton.joints.len(),
        mesh.skeleton.bones.len(),
        mesh.mesh.triangles.len() / 3
    )
}

// ── drawing ────────────────────────────────────────────────────────────

/// A chip: the system's small pill of state.
///
/// A signal-coloured chip gets a wash of its own colour; a resting one gets
/// the plain white veil. Same rule as everywhere else — saturation is state.
fn chip(ui: &mut egui::Ui, text: &str, ink: egui::Color32, dot: bool) -> egui::Response {
    let font = egui::FontId::monospace(theme::FS_TINY);
    let galley = ui.painter().layout_no_wrap(text.to_string(), font, ink);
    let dot_w = if dot { 11.0 } else { 0.0 };
    let size = egui::vec2(galley.size().x + dot_w + theme::S_SM * 2.0, 19.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::hover());

    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        let wash = if dot {
            egui::Color32::from_rgba_unmultiplied(ink.r(), ink.g(), ink.b(), 36)
        } else {
            theme::WELL
        };
        p.rect_filled(rect, theme::R_BADGE, wash);
        let mut x = rect.min.x + theme::S_SM;
        if dot {
            p.circle_filled(egui::pos2(x + 2.5, rect.center().y), 2.5, ink);
            x += dot_w;
        }
        p.galley(
            egui::pos2(x, rect.center().y - galley.size().y * 0.5),
            galley,
            ink,
        );
    }
    response
}

/// Project, mode, and what the machine is doing.
#[allow(clippy::too_many_arguments)]
pub fn title_bar(
    ui: &mut egui::Ui,
    state: &mut EditorState,
    project: &Project,
    output: Option<&OutputInfo>,
    clips_playing: usize,
    recording: bool,
    file_status: Option<&str>,
    file_request: &mut Option<crate::files::FileAction>,
) {
    ui.horizontal_centered(|ui| {
        ui.add_space(theme::S_MD);
        ui.label(
            egui::RichText::new("ANIMUS LIVE")
                .size(theme::FS_LABEL)
                .color(theme::SUB)
                .strong(),
        );
        ui.label(
            egui::RichText::new(&project.meta.name)
                .size(theme::FS_BASE)
                .color(theme::BRIGHT)
                .strong(),
        );

        file_menu(ui, file_request);
        view_menu(ui, state);

        // No mode switch here. With three stages that *are* the three modes,
        // a switch in the title bar and a navigator below it would be two
        // controls showing the same state — the duplication this pass exists
        // to remove. The navigator wins: it carries the completion ticks and
        // the puppet summary, which a switch cannot.
        ui.add_space(theme::S_MD);
        ui.label(
            egui::RichText::new(Stage::of(state.mode).hint())
                .size(theme::FS_SM)
                .color(theme::DIM),
        );

        // What just happened to the file, beside the controls that did it.
        if let Some(msg) = file_status {
            ui.add_space(theme::S_MD);
            ui.label(
                egui::RichText::new(msg)
                    .size(theme::FS_SM)
                    .color(theme::DIM),
            );
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(theme::S_MD);

            if project.solver.enabled {
                chip(
                    ui,
                    &format!("SOLVER {} Hz", project.solver.hz),
                    theme::GO_GREEN,
                    true,
                );
            } else {
                // Paused is a real state and gets said out loud: a puppet that
                // will not move is otherwise indistinguishable from one that
                // is broken.
                chip(ui, "SOLVER PAUSED", theme::CAUTION_AMBER, true);
            }

            match output {
                Some(o) => {
                    chip(ui, &o.short, theme::LIVE_CORAL, true).on_hover_text(&o.description)
                }
                None => chip(ui, "OUTPUT OFF", theme::DIM, false),
            };

            if recording {
                chip(ui, "REC", theme::LIVE_CORAL, true);
            } else if clips_playing > 0 {
                chip(
                    ui,
                    &format!("PLAYING {clips_playing}"),
                    theme::GO_GREEN,
                    true,
                );
            }
        });
    });
}

/// Open, Save, Save As.
///
/// Text rather than icons: these three are the only controls in the chrome
/// whose meaning cannot be drawn, and a wrong guess here loses work.
fn file_menu(ui: &mut egui::Ui, request: &mut Option<crate::files::FileAction>) {
    use crate::files::FileAction;
    ui.add_space(theme::S_MD);
    ui.spacing_mut().item_spacing.x = theme::S_HAIR;
    for (action, label, tip) in [
        (
            FileAction::Open,
            "Open",
            "Ctrl+O — open an .animus project folder.",
        ),
        (
            FileAction::Save,
            "Save",
            "Ctrl+S — write the project where it lives. A project that has              never been saved is asked about first.",
        ),
        (
            FileAction::SaveAs,
            "Save As",
            "Ctrl+Shift+S — write the project somewhere new and work there              from now on.",
        ),
    ] {
        if ui
            .add(
                egui::Button::new(
                    egui::RichText::new(label)
                        .size(theme::FS_SM)
                        .color(theme::SOFT),
                )
                .fill(egui::Color32::TRANSPARENT)
                .corner_radius(theme::R_BADGE),
            )
            .on_hover_text(tip)
            .clicked()
        {
            *request = Some(action);
        }
    }
}

/// Every panel, with a tick beside the ones that are open.
///
/// A menu rather than a row of toggles: eight panels is more than the chrome
/// has room for, and this is a control an operator touches when something has
/// gone missing, not one they use while working.
fn view_menu(ui: &mut egui::Ui, state: &mut EditorState) {
    use crate::state::{TabKind, tab_is_open, toggle_tab};

    ui.menu_button(
        egui::RichText::new("View")
            .size(theme::FS_SM)
            .color(theme::SOFT),
        |ui| {
            ui.set_min_width(150.0);
            for tab in TabKind::ALL {
                let open = tab_is_open(&state.dock, tab);
                let locked = tab == TabKind::Viewport;
                let mut checked = open;
                let response = ui.add_enabled(
                    !locked,
                    egui::Checkbox::new(
                        &mut checked,
                        egui::RichText::new(tab.title()).size(theme::FS_CONTROL),
                    ),
                );
                let response = if locked {
                    response.on_disabled_hover_text(
                        "The viewport stays. Without it there is nothing to edit.",
                    )
                } else if tab.is_stub() {
                    response.on_hover_text("Not wired up yet — the panel says so itself.")
                } else {
                    response
                };
                if response.changed() {
                    toggle_tab(&mut state.dock, tab);
                }
            }

            ui.separator();
            if ui
                .button(egui::RichText::new("Reset layout").size(theme::FS_CONTROL))
                .on_hover_text("Put every panel back where it started.")
                .clicked()
            {
                state.dock = crate::state::default_layout();
                ui.close();
            }
        },
    )
    .response
    .on_hover_text("Show or hide panels. A panel closed by its X comes back here.");
}

/// The workflow navigator: five stages, ticks derived from the document.
pub fn stage_bar(ui: &mut egui::Ui, state: &mut EditorState, project: &Project) {
    let current = Stage::of(state.mode);
    ui.horizontal_centered(|ui| {
        ui.add_space(theme::S_MD);
        ui.label(
            egui::RichText::new("PUPPET WORKFLOW")
                .size(theme::FS_LABEL)
                .color(theme::FAINT)
                .strong(),
        );
        ui.add_space(theme::S_MD);
        ui.spacing_mut().item_spacing.x = 0.0;

        for (i, stage) in STAGES.iter().copied().enumerate() {
            if i > 0 {
                connector(ui);
            }
            if stage_tab(ui, stage, stage == current, project).clicked() {
                state.mode = stage.mode();
                state.tool = stage.tool();
            }
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(theme::S_MD);
            ui.label(
                egui::RichText::new(puppet_summary(project))
                    .monospace()
                    .size(theme::FS_TINY)
                    .color(theme::DIM),
            );
        });
    });
}

/// The rail between two stages. A line, not a chevron: this is a route the
/// operator can walk in both directions.
fn connector(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(18.0, 1.0), egui::Sense::hover());
    ui.painter().hline(
        rect.x_range(),
        rect.center().y,
        egui::Stroke::new(1.0_f32, theme::SEAM),
    );
}

fn stage_tab(ui: &mut egui::Ui, stage: Stage, active: bool, project: &Project) -> egui::Response {
    let available = stage.available(project);
    let complete = stage.complete(project);

    let ink = if active {
        theme::BRIGHT
    } else if !available {
        theme::GHOST
    } else {
        theme::SOFT
    };

    let font = egui::FontId::proportional(theme::FS_CONTROL);
    let text = format!("{} {}", stage.number(), stage.label());
    let galley = ui.painter().layout_no_wrap(text, font, ink);
    let badge = 15.0;
    let size = egui::vec2(galley.size().x + badge + theme::S_MD * 2.0 + 8.0, 28.0);
    let sense = if available {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(size, sense);

    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        if active {
            p.rect_filled(rect, theme::R_BUTTON, theme::WORKSPACE);
            p.rect_stroke(
                rect,
                theme::R_BUTTON,
                egui::Stroke::new(1.0_f32, theme::SEAM),
                egui::StrokeKind::Inside,
            );
        } else if available && response.hovered() {
            p.rect_filled(rect, theme::R_BUTTON, theme::WELL);
        }

        let cx = rect.min.x + theme::S_MD + badge * 0.5;
        let cy = rect.center().y;

        if active {
            // "You are here" is a coral dot with a halo — the same coral the
            // mode switch and the stage badge use.
            p.circle_filled(egui::pos2(cx, cy), 3.5, theme::LIVE_CORAL);
            p.circle_stroke(
                egui::pos2(cx, cy),
                6.0,
                egui::Stroke::new(
                    2.0_f32,
                    egui::Color32::from_rgba_unmultiplied(242, 96, 106, 46),
                ),
            );
        } else if complete {
            p.circle_filled(
                egui::pos2(cx, cy),
                badge * 0.5,
                egui::Color32::from_rgba_unmultiplied(87, 200, 120, 36),
            );
            // Drawn, not typed. U+2713 is missing from egui's built-in
            // faces and renders as tofu — and a completion tick that shows a
            // hollow box is worse than no tick at all.
            let s = badge * 0.28;
            p.add(egui::Shape::line(
                vec![
                    egui::pos2(cx - s, cy),
                    egui::pos2(cx - s * 0.2, cy + s * 0.8),
                    egui::pos2(cx + s, cy - s * 0.8),
                ],
                egui::Stroke::new(1.6_f32, theme::GO_GREEN),
            ));
        } else {
            p.circle_stroke(
                egui::pos2(cx, cy),
                badge * 0.5,
                egui::Stroke::new(1.0_f32, theme::SEAM),
            );
            p.text(
                egui::pos2(cx, cy),
                egui::Align2::CENTER_CENTER,
                stage.number().to_string(),
                egui::FontId::proportional(theme::FS_TINY),
                ink,
            );
        }

        p.galley(
            egui::pos2(
                rect.min.x + theme::S_MD + badge + 8.0,
                cy - galley.size().y * 0.5,
            ),
            galley,
            ink,
        );
    }

    // An unticked or unavailable stage explains itself. "Looks unfinished" is
    // not a message; "Add at least two joints and one bone" is.
    let tip = if !available {
        stage.requirement().to_string()
    } else if complete || stage == Stage::Perform {
        stage.hint().to_string()
    } else {
        format!("{}\n{}", stage.hint(), stage.requirement())
    };
    response.on_hover_text(tip)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Stage and mode are the same thing, so the round trip must close in
    /// both directions. If it ever doesn't, clicking a stage would leave that
    /// stage unlit — a control that visibly refuses its own click.
    #[test]
    fn every_stage_is_its_own_mode_and_back() {
        for stage in STAGES {
            assert_eq!(Stage::of(stage.mode()), stage, "{stage:?}");
        }
        for mode in [EditMode::Rig, EditMode::Edit, EditMode::Live] {
            assert_eq!(Stage::of(mode).mode(), mode, "{mode:?}");
        }
    }

    /// Only RIG arms an authoring tool. Posing and performing have nothing to
    /// author, and a rig tool left in hand there is a stray click away from
    /// changing the skeleton mid-show.
    #[test]
    fn only_rig_arms_an_authoring_tool() {
        assert_eq!(Stage::Rig.tool(), Tool::Joint);
        assert_eq!(Stage::Edit.tool(), Tool::Select);
        assert_eq!(Stage::Perform.tool(), Tool::Select);
    }

    /// Completion comes from the document, so an empty project claims
    /// nothing — and the two stages that need a rig are refused until there
    /// is one, because posing a puppet with no joints is posing nothing.
    #[test]
    fn an_unrigged_project_can_only_be_rigged() {
        let project = Project::new("empty");
        assert!(!Stage::Rig.complete(&project));
        assert!(Stage::Rig.available(&project));
        assert!(!Stage::Edit.available(&project));
        assert!(!Stage::Perform.available(&project));
    }

    #[test]
    fn the_summary_says_so_when_there_is_no_puppet() {
        assert_eq!(puppet_summary(&Project::new("empty")), "No puppet yet");
    }

    /// Three modes must never read the same, in words or in colour.
    #[test]
    fn each_mode_says_something_different_about_the_same_gesture() {
        let labels: Vec<_> = STAGES.iter().map(|s| s.label()).collect();
        let hints: Vec<_> = STAGES.iter().map(|s| s.hint()).collect();
        for (i, a) in labels.iter().enumerate() {
            for b in labels.iter().skip(i + 1) {
                assert_ne!(a, b);
            }
        }
        for (i, a) in hints.iter().enumerate() {
            for b in hints.iter().skip(i + 1) {
                assert_ne!(a, b);
            }
        }
        // And the viewport says three different things about dragging, which
        // is the whole reason there are three modes.
        let verbs: Vec<_> = [EditMode::Rig, EditMode::Edit, EditMode::Live]
            .iter()
            .map(|m| viewport_instruction(*m).0)
            .collect();
        assert_eq!(verbs, vec!["RIG", "POSE", "PULL"]);
        assert!(viewport_instruction(EditMode::Rig).1.contains("saved rig"));
        assert!(
            viewport_instruction(EditMode::Live)
                .1
                .contains("Nothing is written")
        );
    }
}
