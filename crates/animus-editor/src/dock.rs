//! The panels.
//!
//! Everything here reads the document and writes nothing. Edits arrive later
//! as `DocCommand`s through a single system (spec §8.6), so a panel that
//! wants to change something returns the intent rather than reaching into
//! `Project`.

use animus_core::doc::Project;
use animus_runtime::DocumentRes;
use bevy_egui::egui;

use crate::chrome;
use crate::icons;
use crate::import::ImportStatus;
use crate::inspect::{InspectorEdit, inspector_ui};
use crate::state::{EditMode, EditorState, LeftTab, PanelSizes, RightTab, Selection, Tool};
use crate::theme;
use crate::viewport::{ViewportInput, ViewportTarget, viewport_widget};

/// The launcher folded down to its transport row.
const STRIP_COLLAPSED: f32 = 46.0;

/// What the step grid asked for. Intent, like every other panel output: the
/// panel names the act, the writer system performs it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StepAction {
    /// Choose the step being edited, and the one a recording writes first.
    Select(usize),
    /// Empty a step. It becomes a rest.
    Clear(usize),
    ClearAll,
    SetRunning(bool),
    SetArmed(bool),
    SetLength(usize),
    SetBpm(f32),
    /// Fold the grid down to its transport row.
    ToggleCollapsed,
}

/// What the operator asked to do to a layer.
///
/// Intents, like every other panel output: the panel names the act and the
/// writer system performs it through the command stack, so all three are one
/// Ctrl+Z away.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LayerEdit {
    SetVisible(animus_core::ids::LayerId, bool),
    /// Put the artwork back at the middle of the stage at its original size.
    ResetPlacement(animus_core::ids::LayerId),
    Duplicate(animus_core::ids::LayerId),
    Delete(animus_core::ids::LayerId),
}

/// What the Output panel asked for this frame.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct OutputEdits {
    pub set_vsync: Option<bool>,
    pub set_fullscreen: Option<bool>,
    /// The stage size, which is document state and goes through the command
    /// stack rather than being poked into the project.
    pub set_canvas: Option<[u32; 2]>,
}

/// What the editor knows about the output window.
///
/// A struct rather than a tuple because the two strings are not
/// interchangeable and a `(bool, String, String)` invites swapping them: one
/// is a chip, the other is a sentence.
#[derive(Debug, Clone)]
pub struct OutputInfo {
    pub vsync: bool,
    /// Whether the window is fullscreen right now, not what was asked for.
    pub fullscreen: bool,
    /// Full explanation, for the panel and the chip's tooltip.
    pub description: String,
    /// Two or three words, for the title bar chip.
    pub short: String,
}

/// Everything the dock produced this frame, for the writer systems.
#[derive(Debug, Default)]
pub struct DockOutput {
    pub viewport_input: Option<ViewportInput>,
    pub inspector_edits: Vec<InspectorEdit>,
    pub layer_move: Option<(animus_core::ids::LayerId, i32)>,
    /// Hide, duplicate and delete, in the order the operator asked for them.
    pub layer_edits: Vec<LayerEdit>,
    /// The operator flipped the output vsync switch.
    pub set_output_vsync: Option<bool>,
    /// Everything the Output panel changed.
    pub output_edits: OutputEdits,
    /// Put every puppet back on its rest pose.
    pub wants_reset_pose: bool,
    /// A pose-mode rotation for the selected joint, in radians.
    pub set_live_rotation: Option<f32>,
    /// Frame everything in the viewport.
    pub wants_fit: bool,
    /// What the clip panel asked for this frame.
    pub step_actions: Vec<StepAction>,
    pub wants_undo: bool,
    pub wants_redo: bool,
    /// Open, Save or Save As, if the operator asked for one.
    pub file_request: Option<crate::files::FileAction>,
}

/// A small tracked label, the system's "label" role.
fn label(ui: &mut egui::Ui, text: &str) {
    ui.label(
        egui::RichText::new(text.to_uppercase())
            .size(theme::FS_LABEL)
            .color(theme::FAINT)
            .strong(),
    );
}

/// Engine-produced values are mono and dimmer than the human-written name
/// beside them. The Mono-Means-Machine rule.
fn data(text: impl Into<String>) -> egui::RichText {
    egui::RichText::new(text)
        .monospace()
        .size(theme::FS_LABEL)
        .color(theme::DIM)
}

fn row(ui: &mut egui::Ui, name: &str, value: impl Into<String>) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(name)
                .size(theme::FS_CONTROL)
                .color(theme::SUB),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(data(value));
        });
    });
}

/// A panel that has nothing to show yet says so, and says when it will.
fn stub(ui: &mut egui::Ui, what: &str, when: &str) {
    ui.add_space(theme::S_SM);
    ui.label(
        egui::RichText::new(what)
            .size(theme::FS_CONTROL)
            .color(theme::DIM),
    );
    ui.add_space(theme::S_XS);
    ui.label(
        egui::RichText::new(when)
            .size(theme::FS_SM)
            .color(theme::FAINT),
    );
}

pub struct TabViewer<'a> {
    pub state: &'a mut EditorState,
    pub doc: &'a Project,
    pub viewport_texture: Option<egui::TextureId>,
    pub target: Option<&'a ViewportTarget>,
    pub import_status: &'a ImportStatus,
    /// (vsync on, human description) of the output window, if the plugin is
    /// installed.
    pub output: Option<OutputInfo>,
    /// Filled by the Inspector tab; the writer system applies them.
    pub inspector_edits: Vec<InspectorEdit>,
    pub wants_undo: bool,
    pub wants_redo: bool,
    /// Layer the operator asked to move, and the direction (+1 toward the
    /// front of the paint order).
    pub layer_move: Option<(animus_core::ids::LayerId, i32)>,
    /// Hide, duplicate and delete asked for this frame.
    pub layer_edits: Vec<LayerEdit>,
    /// The operator flipped the output vsync switch this frame.
    pub set_output_vsync: Option<bool>,
    /// Everything the Output panel changed this frame.
    pub output_edits: OutputEdits,
    /// The operator asked for the rest pose back.
    pub wants_reset_pose: bool,
    /// A pose-mode rotation for the selected joint, in radians.
    pub set_live_rotation: Option<f32>,
    /// The operator asked to frame everything.
    pub wants_fit: bool,
    /// Filled in by the viewport tab, read by the caller afterwards. The
    /// panel cannot touch the camera itself: it runs inside egui's closure,
    /// where the ECS is not available.
    pub viewport_input: Option<ViewportInput>,
}

impl TabViewer<'_> {
    fn viewport(&mut self, ui: &mut egui::Ui) {
        let available = ui.available_size();
        let size = egui::vec2(available.x.max(16.0), available.y.max(16.0));
        let input = viewport_widget(ui, self.viewport_texture, size);

        // The empty state carries the first instruction, in the place the
        // operator is already looking. The 10-minute test allows no manual;
        // this line is the manual.
        if self.doc.puppets.is_empty() {
            ui.painter().text(
                input.rect.center(),
                egui::Align2::CENTER_CENTER,
                "Drag a PNG with a transparent background here",
                egui::FontId::proportional(theme::FS_LG),
                theme::DIM,
            );
            ui.painter().text(
                input.rect.center() + egui::vec2(0.0, 24.0),
                egui::Align2::CENTER_CENTER,
                "then press J to place joints, B to connect bones",
                egui::FontId::proportional(theme::FS_CONTROL),
                theme::FAINT,
            );
        }

        self.mode_overlay(ui, input.rect);
        self.status_strip(ui, input.rect);
        self.viewport_input = Some(input);
    }

    /// What mode this is, said twice on the stage itself.
    ///
    /// The badge names the surface (`STAGE` or `WORKBENCH`); the pill says
    /// what a drag will do. Both are here rather than only in the chrome
    /// because the operator's eyes are on the puppet, not on the title bar —
    /// and the one question this whole boundary exists to answer, "will this
    /// drag change my saved rig", has to be answerable without looking away.
    fn mode_overlay(&self, ui: &mut egui::Ui, rect: egui::Rect) {
        let live = self.state.mode == EditMode::Live;
        let accent = if live { theme::LIVE_CORAL } else { theme::DIM };
        let p = ui.painter();

        // Badge, top right.
        let badge = chrome::stage_badge(self.state.mode);
        let font = egui::FontId::monospace(theme::FS_TINY);
        let galley = p.layout_no_wrap(badge.to_string(), font, accent);
        let size = galley.size() + egui::vec2(theme::S_MD, theme::S_XS * 2.0);
        let at = egui::Rect::from_min_size(
            egui::pos2(rect.max.x - size.x - theme::S_MD, rect.min.y + theme::S_MD),
            size,
        );
        p.rect_filled(
            at,
            theme::R_BADGE,
            if live {
                egui::Color32::from_rgba_unmultiplied(242, 96, 106, 30)
            } else {
                theme::WELL
            },
        );
        if live {
            p.rect_stroke(
                at,
                theme::R_BADGE,
                egui::Stroke::new(
                    1.0_f32,
                    egui::Color32::from_rgba_unmultiplied(242, 96, 106, 102),
                ),
                egui::StrokeKind::Inside,
            );
        }
        p.galley(
            at.min + egui::vec2(theme::S_SM, theme::S_XS),
            galley,
            accent,
        );

        // Instruction pill, bottom centre, clear of the status strip.
        let (verb, sentence) = chrome::viewport_instruction(self.state.mode);
        let vg = p.layout_no_wrap(
            verb.to_string(),
            egui::FontId::monospace(theme::FS_LABEL),
            accent,
        );
        let sg = p.layout_no_wrap(
            sentence.to_string(),
            egui::FontId::proportional(theme::FS_SM),
            theme::MID,
        );
        let inner = vg.size().x + theme::S_MD + sg.size().x;
        let pill = egui::Rect::from_center_size(
            egui::pos2(rect.center().x, rect.max.y - 44.0),
            egui::vec2(inner + theme::S_LG * 2.0, 30.0),
        );
        // Only when the pill has somewhere to sit. On a tiny viewport the
        // instruction would cover the puppet it is describing.
        if pill.width() < rect.width() - theme::S_XL && rect.height() > 140.0 {
            p.rect_filled(pill, theme::R_BUTTON, theme::STATUS_BG);
            p.rect_stroke(
                pill,
                theme::R_BUTTON,
                egui::Stroke::new(
                    1.0_f32,
                    if live {
                        egui::Color32::from_rgba_unmultiplied(242, 96, 106, 102)
                    } else {
                        theme::SEAM
                    },
                ),
                egui::StrokeKind::Inside,
            );
            let x = pill.min.x + theme::S_LG;
            let cy = pill.center().y;
            p.galley(egui::pos2(x, cy - vg.size().y * 0.5), vg.clone(), accent);
            p.galley(
                egui::pos2(x + vg.size().x + theme::S_MD, cy - sg.size().y * 0.5),
                sg,
                theme::MID,
            );
        }
    }

    /// What is true right now, in one line at the bottom of the view.
    ///
    /// World-per-pixel is here because every offset in this editor is quoted
    /// in pixels, and without the conversion on screen a coordinate bug is a
    /// guessing game. M0-2 spent three rounds on one for want of this line.
    fn status_strip(&self, ui: &mut egui::Ui, rect: egui::Rect) {
        let Some(target) = self.target else { return };
        let strip = egui::Rect::from_min_max(egui::pos2(rect.min.x, rect.max.y - 20.0), rect.max);
        ui.painter().rect_filled(strip, 0.0, theme::STATUS_BG);

        let mut text = format!(
            "1 px = {:.4} world    {} x {}",
            target.world_per_pixel, target.size.x, target.size.y
        );
        if let Some(c) = target.cursor_world {
            text.push_str(&format!("    cursor {:.2}, {:.2}", c.x, c.y));
        }
        if let Some(c) = target.last_click_world {
            text.push_str(&format!("    click {:.2}, {:.2}", c.x, c.y));
        }
        ui.painter().text(
            strip.left_center() + egui::vec2(theme::S_SM, 0.0),
            egui::Align2::LEFT_CENTER,
            text,
            egui::FontId::monospace(theme::FS_TINY),
            theme::DIM,
        );
    }

    /// SCENE: what is in the show, and what its skeleton looks like.
    ///
    /// Two lists in one tab because they answer the same question at two
    /// depths — which artwork is on stage, and how it is jointed. The comp
    /// stacks them for that reason rather than to save a tab.
    fn scene(&mut self, ui: &mut egui::Ui) {
        self.layers(ui);
        ui.add_space(theme::S_LG);
        self.rig_tree(ui);
    }

    fn layers(&mut self, ui: &mut egui::Ui) {
        if self.doc.layers.is_empty() {
            stub(ui, "No layers yet.", "Import an image to make one.");
            return;
        }

        // Painted back to front, so the topmost layer reads at the top of the
        // list — the order an artist expects, not the order the Vec is in.
        // The arrows move a layer within the paint order; the writer turns
        // that into a ReorderLayers command plus the depth rewrite that keeps
        // `layer.depth` the single source of truth for world Z.
        let layer_ids: Vec<_> = self.doc.layers.iter().rev().copied().collect();

        for layer_id in layer_ids {
            let Some(layer) = self.doc.layer_data.get(&layer_id) else {
                continue;
            };
            let selected = self.state.selection == Selection::Layer(layer_id);

            ui.horizontal(|ui| {
                // Visibility first, at the left edge, where the eye scans a
                // list of rows. It is the one control an operator reaches for
                // mid-show and it must not move when the row is renamed.
                let visible = layer.visible;
                if icons::button(
                    ui,
                    if visible {
                        icons::Icon::Eye
                    } else {
                        icons::Icon::EyeOff
                    },
                    visible,
                    (!visible).then_some(theme::CAUTION_AMBER),
                )
                .on_hover_text(if visible {
                    "Hide this layer. It leaves the stage and the projector."
                } else {
                    "Show this layer."
                })
                .clicked()
                {
                    self.layer_edits
                        .push(LayerEdit::SetVisible(layer_id, !visible));
                }

                // Everything in one left-to-right flow, with the name sized
                // to leave exactly enough room for the icons.
                //
                // A nested `right_to_left` group was tried first and its
                // buttons took hover but never a click, while the eye outside
                // it worked from the same helper. Rather than keep guessing at
                // egui's nested-layout interaction, this computes the width.
                const ICON: f32 = 20.0;
                const ICONS: usize = 4;
                let gap = ui.spacing().item_spacing.x;
                let reserved = (ICON + gap) * ICONS as f32;
                let name_width = (ui.available_width() - reserved).max(40.0);

                let ink = match (selected, visible) {
                    (true, true) => theme::BRIGHT,
                    (true, false) => theme::SOFT,
                    // A hidden layer is dimmed in the list as well, so the
                    // list and the stage agree without anyone checking.
                    (false, true) => theme::INK,
                    (false, false) => theme::FAINT,
                };
                // `add_sized`, not `min_size`: a minimum lets a long name grow
                // past the budget and push the trash icon off the panel edge,
                // which is how the delete control disappeared on the first
                // layer whose name did not happen to be short.
                if ui
                    .add_sized(
                        egui::vec2(name_width, ui.spacing().interact_size.y),
                        egui::Button::new(
                            egui::RichText::new(&layer.name)
                                .size(theme::FS_CONTROL)
                                .color(ink),
                        )
                        .fill(if selected {
                            theme::WELL_HOVER
                        } else {
                            egui::Color32::TRANSPARENT
                        })
                        .corner_radius(theme::R_INPUT)
                        .truncate(),
                    )
                    .on_hover_text(format!("{} · z {:.2}", layer.name, layer.depth))
                    .clicked()
                {
                    self.state.selection = Selection::Layer(layer_id);
                }

                if icons::button(ui, icons::Icon::Up, false, None)
                    .on_hover_text(format!("Bring forward. Now at z {:.2}.", layer.depth))
                    .clicked()
                {
                    self.layer_move = Some((layer_id, 1));
                }
                if icons::button(ui, icons::Icon::Down, false, None)
                    .on_hover_text(format!("Send backward. Now at z {:.2}.", layer.depth))
                    .clicked()
                {
                    self.layer_move = Some((layer_id, -1));
                }
                if icons::button(ui, icons::Icon::Copy, false, None)
                    .on_hover_text("Duplicate this layer and everything on it.")
                    .clicked()
                {
                    self.layer_edits.push(LayerEdit::Duplicate(layer_id));
                }

                // Delete is last and the only coral control in the row: it is
                // the only one that destroys work.
                //
                // The last layer is deletable too. Refusing it left the
                // operator with no way to start over from one bad import
                // except by duplicating it first, and "you may not empty this
                // project" is not a rule the document needs — an empty
                // project is a legal document and the viewport already knows
                // how to ask for an image.
                if icons::button(ui, icons::Icon::Trash, false, Some(theme::LIVE_CORAL))
                    .on_hover_text("Delete this layer and everything on it. Ctrl+Z brings it back.")
                    .clicked()
                {
                    self.layer_edits.push(LayerEdit::Delete(layer_id));
                }
            });
        }
    }

    /// Where the show goes, and how big it is.
    pub fn output_body(&mut self, ui: &mut egui::Ui) {
        label(ui, "window");
        match &self.output {
            Some(o) => {
                ui.label(
                    egui::RichText::new(&o.description)
                        .size(theme::FS_SM)
                        .color(theme::SUB),
                );

                ui.add_space(theme::S_SM);
                let mut fullscreen = o.fullscreen;
                if ui
                    .checkbox(
                        &mut fullscreen,
                        egui::RichText::new("Fullscreen on this display").size(theme::FS_CONTROL),
                    )
                    .on_hover_text(
                        "Fills the display the output window is currently on. Drag the \
                         window to the projector first, then tick this.",
                    )
                    .changed()
                {
                    self.output_edits.set_fullscreen = Some(fullscreen);
                }

                let mut vsync = o.vsync;
                if ui
                    .checkbox(
                        &mut vsync,
                        egui::RichText::new("Vsync").size(theme::FS_CONTROL),
                    )
                    .on_hover_text(
                        "On is the safer default: a clean projector image. The cost is \
                         editor frame rate, not the show.",
                    )
                    .changed()
                {
                    self.output_edits.set_vsync = Some(vsync);
                }
            }
            None => stub(
                ui,
                "No output window.",
                "It opens at startup unless output is disabled.",
            ),
        }

        ui.add_space(theme::S_MD);
        label(ui, "resolution");
        ui.label(
            egui::RichText::new("What the audience sees. Puppets are placed against it.")
                .size(theme::FS_SM)
                .color(theme::FAINT),
        );
        ui.add_space(theme::S_XS);

        let canvas = self.doc.stage.canvas;
        ui.label(data(format!("{} x {}", canvas[0], canvas[1])));
        ui.add_space(theme::S_XS);

        // Presets first, because the answer is almost always one of these and
        // typing four digits twice is four chances to get it wrong.
        for (name, size) in [
            ("1920 x 1080", [1920u32, 1080]),
            ("1280 x 720", [1280, 720]),
            ("3840 x 2160", [3840, 2160]),
            ("1080 x 1920", [1080, 1920]),
        ] {
            let selected = canvas == size;
            if ui
                .add(
                    egui::Button::new(
                        egui::RichText::new(name)
                            .size(theme::FS_SM)
                            .color(if selected { theme::BRIGHT } else { theme::SOFT }),
                    )
                    .fill(if selected {
                        theme::WELL_HOVER
                    } else {
                        egui::Color32::TRANSPARENT
                    })
                    .corner_radius(theme::R_BADGE)
                    .min_size(egui::vec2(ui.available_width(), 0.0)),
                )
                .clicked()
                && !selected
            {
                self.output_edits.set_canvas = Some(size);
            }
        }

        ui.add_space(theme::S_XS);
        ui.horizontal(|ui| {
            let mut w = canvas[0] as i64;
            let mut h = canvas[1] as i64;
            let cw = ui.add(egui::DragValue::new(&mut w).speed(8).range(16..=16384));
            ui.label(
                egui::RichText::new("x")
                    .size(theme::FS_SM)
                    .color(theme::FAINT),
            );
            let ch = ui.add(egui::DragValue::new(&mut h).speed(8).range(16..=16384));
            if cw.changed() || ch.changed() {
                self.output_edits.set_canvas = Some([w.max(16) as u32, h.max(16) as u32]);
            }
        });

        ui.add_space(theme::S_MD);
        label(ui, "streaming");
        // Named rather than hidden. An operator who came for Spout needs to
        // know it is absent before the show, not during it — and Syphon is
        // worth naming too, because asking for it on Windows is a category
        // error rather than a missing feature.
        ui.label(
            egui::RichText::new(
                "No Spout or NDI output yet. The projector window is the only route to a \
                 screen. Syphon is macOS-only; Spout is its Windows equivalent and is the \
                 one this would grow.",
            )
            .size(theme::FS_SM)
            .color(theme::FAINT),
        );
    }

    /// The skeleton as a list, indented by depth.
    ///
    /// **A rig stores no hierarchy** — bones connect joint pairs and nothing
    /// names a parent — so the tree here is derived by
    /// [`animus_core::skeleton::rig_tree`], the same function forward
    /// kinematics uses. That matters: what the operator reads in this list is
    /// exactly what a rotation will carry, rather than a second opinion about
    /// the same bones.
    fn rig_tree(&mut self, ui: &mut egui::Ui) {
        let Some((pid, puppet)) = self.doc.puppets.iter().next() else {
            return;
        };
        let animus_core::doc::PuppetKind::Mesh(mesh) = &puppet.kind else {
            return;
        };
        crate::widgets::panel_header(
            ui,
            &format!("Rig · {}", puppet.name),
            Some(&format!("{} joints", mesh.skeleton.joints.len())),
        );
        ui.add_space(theme::S_XS);

        if mesh.skeleton.joints.is_empty() {
            ui.label(
                egui::RichText::new("No joints yet. Place them in RIG.")
                    .size(theme::FS_SM)
                    .color(theme::HINT),
            );
            return;
        }

        let tree = animus_core::skeleton::rig_tree(&mesh.skeleton);
        // Depth-first from each root, so a limb reads as a limb rather than
        // as every joint at that distance from the hip.
        let mut stack: Vec<(animus_core::ids::JointId, usize)> =
            tree.roots().iter().rev().map(|j| (*j, 0)).collect();
        let mut clicked = None;
        while let Some((id, depth)) = stack.pop() {
            for child in tree.children(id).iter().rev() {
                stack.push((*child, depth + 1));
            }
            let Some(joint) = mesh.skeleton.joints.get(&id) else {
                continue;
            };
            let selected = self.state.selection == Selection::Joint(*pid, id);

            let (rect, response) = ui
                .allocate_exact_size(egui::vec2(ui.available_width(), 22.0), egui::Sense::click());
            if response.clicked() {
                clicked = Some(Selection::Joint(*pid, id));
            }
            let p = ui.painter();
            if selected {
                p.rect_filled(rect, theme::R_BADGE as f32, theme::SELECT_WASH);
            } else if response.hovered() {
                p.rect_filled(rect, theme::R_BADGE as f32, theme::WELL);
            }

            let x = rect.min.x + theme::S_SM + depth as f32 * theme::S_MD;
            // Pinned joints carry a mark rather than a colour: pinning is a
            // fact about the rig, not a live signal.
            let ink = if selected { theme::BRIGHT } else { theme::MID };
            p.circle_filled(
                egui::pos2(x, rect.center().y),
                2.0,
                if joint.pinned {
                    theme::DIM
                } else {
                    theme::GHOST
                },
            );
            let galley = p.layout_no_wrap(
                joint.name.clone(),
                egui::FontId::proportional(theme::FS_SM + 0.5),
                ink,
            );
            p.galley(
                egui::pos2(x + theme::S_SM, rect.center().y - galley.size().y * 0.5),
                galley,
                ink,
            );
            if joint.pinned {
                let tag = p.layout_no_wrap(
                    "pinned".into(),
                    egui::FontId::monospace(theme::FS_TINY),
                    theme::HINT,
                );
                p.galley(
                    egui::pos2(
                        rect.max.x - theme::S_SM - tag.size().x,
                        rect.center().y - tag.size().y * 0.5,
                    ),
                    tag,
                    theme::HINT,
                );
            }
        }
        if let Some(sel) = clicked {
            self.state.selection = sel;
        }
    }

    fn assets(&mut self, ui: &mut egui::Ui) {
        // The import message goes at the top, where the eye already is after
        // dropping a file — and it stays until the next import rather than
        // vanishing on a timer, because the sentence usually contains the
        // instruction for what to do next.
        if let Some(message) = &self.import_status.message {
            let colour = if self.import_status.is_error {
                theme::LIVE_CORAL
            } else {
                theme::GO_GREEN
            };
            egui::Frame::NONE
                .fill(theme::WELL)
                .corner_radius(theme::R_INPUT)
                .inner_margin(egui::Margin::same(theme::S_SM as i8))
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(message)
                            .size(theme::FS_SM)
                            .color(colour),
                    );
                });
            ui.add_space(theme::S_SM);
        }

        if self.doc.assets.is_empty() {
            stub(
                ui,
                "No images imported.",
                "Drag a PNG with transparency into the viewport.",
            );
            return;
        }
        for (id, asset) in self.doc.assets.iter() {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(&asset.original_name)
                        .size(theme::FS_CONTROL)
                        .color(theme::INK),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(data(format!("#{}", id.0)));
                });
            });
        }
    }

    /// Mode first, then tools, in a scroll area.
    ///
    /// The mode changes what every tool *means*, so it leads. It is also the
    /// control that must never be unreachable: when the performance strip
    /// took its share of the window, MODE was the first thing to fall below
    /// this panel's fold, and a Live switch you cannot see is a Live switch
    /// you cannot leave.
    fn tools_body(&mut self, ui: &mut egui::Ui) {
        // The mode used to live here. It is in the title bar now: it is the
        // most consequential control in the editor and it belongs where the
        // operator cannot miss it, not three panels down a sidebar.
        self.pose_actions(ui);
        ui.add_space(theme::S_MD);
        label(
            ui,
            match self.state.mode {
                EditMode::Rig => "rig tools",
                EditMode::Edit => "posing",
                EditMode::Live => "performance",
            },
        );
        // Authoring tools are Edit-mode only: live mode is the show, and a
        // stray click must not add a joint to a rig with an audience
        // watching. Disabled rather than hidden, so the tool does not appear
        // to vanish — and the tooltip says which mode it lives in.
        let editing = self.state.mode == EditMode::Rig;
        for tool in Tool::ALL {
            let selected = self.state.tool == tool;
            let enabled = editing || matches!(tool, Tool::Select);
            // Name and shortcut on one row. They were two rows until the
            // stage bar took its share of the window and the tool list ran
            // off the bottom of its panel: eight rows for four tools is a
            // list that only fits on a big screen.
            let response = ui.add_enabled(
                enabled,
                egui::Button::new(
                    egui::RichText::new(tool.label())
                        .size(theme::FS_CONTROL)
                        .color(if selected { theme::BRIGHT } else { theme::SOFT }),
                )
                .shortcut_text(
                    egui::RichText::new(tool.key_hint())
                        .monospace()
                        .size(theme::FS_LABEL)
                        .color(theme::FAINT),
                )
                .fill(if selected {
                    theme::WELL_HOVER
                } else {
                    egui::Color32::TRANSPARENT
                })
                .corner_radius(theme::R_INPUT)
                .min_size(egui::vec2(ui.available_width(), 0.0)),
            );
            let response = response.on_hover_text(match (tool, enabled) {
                (Tool::Select, _) => "Click a joint to select it; drag to move it. In Live mode, dragging pulls the puppet.",
                (Tool::Joint, true) => "Click on the puppet to place a joint. The first one is pinned and anchors the rig.",
                (Tool::Bone, true) => "Click one joint, then another, to connect them with a spring.",
                (Tool::Vertex, _) => "Mesh editing arrives in a later milestone.",
                (_, false) => "Rigging happens in Edit mode. Live mode only moves what is already there.",
            });
            if response.clicked() {
                self.state.tool = tool;
            }
        }

        ui.add_space(theme::S_SM);
        ui.label(
            egui::RichText::new(match self.state.mode {
                EditMode::Rig => "Edits here change the saved rig.",
                EditMode::Edit => {
                    "Dragging poses the selected step. Switch to RIG to move joints \
                     permanently."
                }
                EditMode::Live => {
                    "Rig editing is unavailable in PERFORM. Switch to RIG to move joints \
                     permanently."
                }
            })
            .size(theme::FS_SM)
            .color(theme::FAINT),
        );
    }

    /// The two things that undo a performance without touching the document.
    ///
    /// They sit at the top of the panel because they are the way back from a
    /// live pull, and the hand that did the pulling is already here.
    /// The layer holding whatever is selected, if anything is.
    fn selection_layer(&self) -> Option<animus_core::ids::LayerId> {
        match self.state.selection {
            Selection::Layer(l) => Some(l),
            Selection::Puppet(p) | Selection::Joint(p, _) | Selection::Bone(p, _) => {
                crate::hit::layer_of(self.doc, p)
            }
            Selection::None => None,
        }
    }

    /// Which layers a position reset would move: the selected one, or every
    /// one when nothing is selected.
    fn reset_placement_targets(&self) -> Vec<animus_core::ids::LayerId> {
        match self.selection_layer() {
            Some(l) => vec![l],
            None => self.doc.layers.clone(),
        }
    }

    fn pose_actions(&mut self, ui: &mut egui::Ui) {
        if ui
            .add(
                egui::Button::new(egui::RichText::new("Reset pose").size(theme::FS_CONTROL))
                    .shortcut_text(
                        egui::RichText::new("R")
                            .monospace()
                            .size(theme::FS_LABEL)
                            .color(theme::FAINT),
                    )
                    .corner_radius(theme::R_BUTTON)
                    .min_size(egui::vec2(ui.available_width(), 0.0)),
            )
            .on_hover_text(
                "R — put every puppet back on its rest pose and let go of any joint being \
                 pulled. The document is not touched.",
            )
            .clicked()
        {
            self.wants_reset_pose = true;
        }

        // Position is document state, so this one is undoable and this one
        // has a scope. Pose is the solver's and resetting all of it costs
        // nothing; placement is composition work, and quietly flattening a
        // stage the operator arranged would be the opposite of a reset.
        let targets = self.reset_placement_targets();
        let (label, tip) = match targets.len() {
            0 => (
                "Reset position",
                "Nothing to move: there is no artwork on the stage yet.".to_string(),
            ),
            1 if self.selection_layer().is_some() => (
                "Reset position",
                "Put the selected artwork back at the middle of the stage at its \
                 original size. Ctrl+Z undoes it."
                    .to_string(),
            ),
            n => (
                "Reset all positions",
                format!(
                    "Nothing is selected, so this puts all {n} layers back at the middle \
                     of the stage at their original size. Select one first to move only it."
                ),
            ),
        };
        if ui
            .add_enabled(
                !targets.is_empty(),
                egui::Button::new(egui::RichText::new(label).size(theme::FS_CONTROL))
                    .corner_radius(theme::R_BUTTON)
                    .min_size(egui::vec2(ui.available_width(), 0.0)),
            )
            .on_hover_text(tip)
            .on_disabled_hover_text("Import an image first.")
            .clicked()
        {
            for layer in targets {
                self.layer_edits.push(LayerEdit::ResetPlacement(layer));
            }
        }

        if ui
            .add(
                egui::Button::new(egui::RichText::new("Fit view").size(theme::FS_CONTROL))
                    .shortcut_text(
                        egui::RichText::new("F")
                            .monospace()
                            .size(theme::FS_LABEL)
                            .color(theme::FAINT),
                    )
                    .corner_radius(theme::R_BUTTON)
                    .min_size(egui::vec2(ui.available_width(), 0.0)),
            )
            .on_hover_text("F — put everything back in the frame. Changes the view, not the show.")
            .clicked()
        {
            self.wants_fit = true;
        }
    }

    fn inspector(&mut self, ui: &mut egui::Ui) {
        let out = inspector_ui(ui, self.doc, self.state);
        self.inspector_edits.extend(out.edits);
        if out.wants_reset_pose {
            self.wants_reset_pose = true;
        }
        if out.set_live_rotation.is_some() {
            self.set_live_rotation = out.set_live_rotation;
        }

        ui.add_space(theme::S_MD);
        self.undo_history(ui);
    }

    /// The undo history: labels, newest first, and the two buttons.
    ///
    /// The list is read straight from the stack, so what it shows is what
    /// Ctrl+Z will actually do — not a parallel record that can drift.
    fn undo_history(&mut self, ui: &mut egui::Ui) {
        ui.label(
            egui::RichText::new("HISTORY")
                .size(theme::FS_LABEL)
                .color(theme::FAINT)
                .strong(),
        );
        ui.horizontal(|ui| {
            if ui
                .add_enabled(self.state.undo.can_undo(), egui::Button::new("Undo"))
                .clicked()
            {
                self.wants_undo = true;
            }
            if ui
                .add_enabled(self.state.undo.can_redo(), egui::Button::new("Redo"))
                .clicked()
            {
                self.wants_redo = true;
            }
            ui.label(
                egui::RichText::new(format!(
                    "{} steps · {:.1} MB",
                    self.state.undo.len(),
                    self.state.undo.memory_bytes() as f64 / (1024.0 * 1024.0)
                ))
                .monospace()
                .size(theme::FS_TINY)
                .color(theme::FAINT),
            );
        });
        for label in self.state.undo.labels().take(12) {
            ui.label(
                egui::RichText::new(label)
                    .size(theme::FS_SM)
                    .color(theme::DIM),
            );
        }
    }

    /// The performance view: record a gesture, loop it, set its speed.
    ///
    /// A clip writes into the same targets a hand does, so everything here
    /// is about *when* rather than *what* — and the one rule worth stating
    /// on screen is that grabbing a looping limb takes it over.
    fn solver(&mut self, ui: &mut egui::Ui) {
        let s = self.doc.solver;
        label(ui, "solver");
        row(ui, "rate", format!("{} Hz", s.hz));
        row(ui, "state", if s.enabled { "running" } else { "paused" });

        // Live, not read-only: these three are what a puppet's *feel* is made
        // of, and the only way to find the right value is to move it while
        // watching the puppet. Each goes through the command stack, so a
        // wrong turn is one Ctrl+Z.
        // The default beside each one, so a puppet that behaves differently
        // from the last one shows *where* it was changed rather than leaving
        // the operator to compare five numbers against memory.
        let d = animus_core::doc::SolverConfig::default();
        for (name, value, range, param, default) in [
            (
                "gravity",
                s.gravity.y,
                -40.0..=40.0,
                animus_core::doc::SolverParam::GravityY,
                d.gravity.y,
            ),
            (
                "sideways",
                s.gravity.x,
                -40.0..=40.0,
                animus_core::doc::SolverParam::GravityX,
                d.gravity.x,
            ),
            (
                "return to rest",
                s.rest_pull,
                0.0..=0.5,
                animus_core::doc::SolverParam::RestPull,
                d.rest_pull,
            ),
            (
                "damping",
                s.global_damping,
                0.80..=1.0,
                animus_core::doc::SolverParam::Damping,
                d.global_damping,
            ),
            (
                "iterations",
                s.iterations as f32,
                1.0..=16.0,
                animus_core::doc::SolverParam::Iterations,
                d.iterations as f32,
            ),
        ] {
            let (changed, released) =
                crate::inspect::labelled_slider(ui, name, value, range, false, Some(default));
            if changed.is_some() || released {
                self.inspector_edits.push(crate::inspect::InspectorEdit {
                    command: changed.map(|to| {
                        crate::inspect::InspectorCommand::SolverParam(
                            animus_core::doc::SetSolverParam {
                                param,
                                from: value,
                                to,
                            },
                        )
                    }),
                    released,
                });
            }
        }
        ui.label(
            egui::RichText::new(
                "Gravity pulls every unpinned joint. Damping is how fast motion dies;                  fewer iterations is a looser, more rubbery puppet.",
            )
            .size(theme::FS_SM)
            .color(theme::FAINT),
        );

        ui.add_space(theme::S_MD);
        label(ui, "output");
        if let Some(OutputInfo {
            vsync, description, ..
        }) = &self.output
        {
            // The trade is stated next to the switch, because the operator
            // deciding at a venue should not need the manual: M0-4 measured
            // an output synced to a 30Hz display clamping the whole app.
            let mut on = *vsync;
            if ui
                .checkbox(
                    &mut on,
                    egui::RichText::new("vsync").size(theme::FS_CONTROL),
                )
                .changed()
            {
                self.set_output_vsync = Some(on);
            }
            ui.label(
                egui::RichText::new(if on {
                    "Clean projector image; the editor runs at the projector's rate."
                } else {
                    "Fast editor; the projector may tear slightly."
                })
                .size(theme::FS_SM)
                .color(theme::FAINT),
            );
            ui.label(
                egui::RichText::new(description.as_str())
                    .monospace()
                    .size(theme::FS_TINY)
                    .color(theme::DIM),
            );
        } else {
            ui.label(
                egui::RichText::new("no output window")
                    .size(theme::FS_SM)
                    .color(theme::FAINT),
            );
        }
    }
}

/// Draw the whole dock. Called from the egui pass.
/// A hairline between two groups of controls.
///
/// Painted rather than typed: U+2502 is not in egui's built-in faces and
/// renders as a hollow box, which reads as a broken button rather than as a
/// divider.
fn separator(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(1.0, 20.0), egui::Sense::hover());
    ui.painter().vline(
        rect.center().x,
        rect.y_range(),
        egui::Stroke::new(1.0_f32, theme::SEAM),
    );
}

/// The step grid: a drum machine for poses.
///
/// A panel of the window rather than a tab in the dock, because a bar of
/// steps wants the full width and the dock's splits would not give it one.
pub fn steps_strip(
    ui: &mut egui::Ui,
    seq: &animus_runtime::Sequencer,
    mode: EditMode,
    puppet: &str,
    collapsed: bool,
    actions: &mut Vec<StepAction>,
) {
    transport_row(ui, seq, mode, puppet, actions);
    if collapsed {
        return;
    }
    ui.add_space(theme::S_SM);
    egui::ScrollArea::vertical()
        .id_salt("step_grid")
        .auto_shrink([false, false])
        .show(ui, |ui| step_grid(ui, seq, mode, actions));
}

/// Transport on the left, grid settings on the right.
fn transport_row(
    ui: &mut egui::Ui,
    seq: &animus_runtime::Sequencer,
    mode: EditMode,
    puppet: &str,
    actions: &mut Vec<StepAction>,
) {
    ui.horizontal(|ui| {
        label(ui, "steps");
        ui.label(
            egui::RichText::new(puppet)
                .monospace()
                .size(theme::FS_TINY)
                .color(theme::FAINT),
        );
        ui.add_space(theme::S_SM);

        if icons::button(ui, icons::Icon::Stop, false, Some(theme::MID))
            .on_hover_text("Stop the transport and park the playhead at step 1.")
            .clicked()
        {
            actions.push(StepAction::SetRunning(false));
        }
        if icons::button(
            ui,
            if seq.running {
                icons::Icon::Stop
            } else {
                icons::Icon::Play
            },
            false,
            Some(theme::GO_GREEN),
        )
        .on_hover_text("Run the pattern. Each step fires as the playhead enters it.")
        .clicked()
        {
            actions.push(StepAction::SetRunning(!seq.running));
        }

        record_button(ui, seq, mode, actions);

        ui.add_space(theme::S_MD);
        separator(ui);
        ui.add_space(theme::S_MD);

        ui.label(
            egui::RichText::new("Steps")
                .size(theme::FS_SM)
                .color(theme::SUB),
        );
        for n in animus_runtime::STEP_COUNTS {
            let selected = seq.len() == n;
            let blocked = seq.len_blocked_by(n);
            let response = ui.add_enabled(
                blocked.is_none(),
                egui::Button::new(
                    egui::RichText::new(n.to_string())
                        .monospace()
                        .size(theme::FS_LABEL)
                        .color(if selected { theme::BRIGHT } else { theme::DIM }),
                )
                .fill(if selected {
                    theme::WELL_HOVER
                } else {
                    egui::Color32::TRANSPARENT
                })
                .corner_radius(theme::R_BADGE),
            );
            let response = match blocked {
                Some(floor) => response.on_disabled_hover_text(format!(
                    "Step {floor} holds a pose. Clear it and this length opens up."
                )),
                None => response,
            };
            if response.clicked() {
                actions.push(StepAction::SetLength(n));
            }
        }

        ui.add_space(theme::S_MD);
        let mut bpm = seq.bpm;
        if ui
            .add(
                egui::DragValue::new(&mut bpm)
                    .speed(0.5)
                    .range(20.0..=300.0)
                    .suffix(" BPM"),
            )
            .on_hover_text("One step is one beat.")
            .changed()
        {
            actions.push(StepAction::SetBpm(bpm));
        }
        beat_dots(ui, seq);

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .button(egui::RichText::new("Collapse").size(theme::FS_SM))
                .on_hover_text("Fold the grid away and give the height back to the stage.")
                .clicked()
            {
                actions.push(StepAction::ToggleCollapsed);
            }
            if seq.filled() > 0
                && ui
                    .button(egui::RichText::new("Clear all").size(theme::FS_SM))
                    .on_hover_text("Empty every step.")
                    .clicked()
            {
                actions.push(StepAction::ClearAll);
            }
        });
    });
}

/// Arm record, and the reason it is refused when it is.
///
/// Armed by hand, always. Entering PERFORM never arms it — an editor that
/// starts recording because you changed screens is an editor you cannot trust
/// with a show.
fn record_button(
    ui: &mut egui::Ui,
    seq: &animus_runtime::Sequencer,
    mode: EditMode,
    actions: &mut Vec<StepAction>,
) {
    // Live recording writes the pose the operator is *holding* as the
    // playhead crosses each step, so it needs the transport and a hand — both
    // of which live in PERFORM. In EDIT you author steps one at a time by
    // posing them, which needs no arming.
    let blocked = match mode {
        EditMode::Rig => Some("Recording captures live pulls. Switch to PERFORM first."),
        EditMode::Edit => {
            Some("In EDIT a step is posed directly. Arm record in PERFORM to play it in.")
        }
        EditMode::Live => None,
    };

    if seq.armed {
        ui.add_enabled(
            false,
            egui::Button::new(
                egui::RichText::new("\u{23FA} Armed")
                    .size(theme::FS_SM)
                    .color(theme::LIVE_CORAL),
            )
            .fill(egui::Color32::from_rgba_unmultiplied(242, 96, 106, 26))
            .corner_radius(theme::R_CONTROL),
        );
        if ui
            .button(egui::RichText::new("Disarm").size(theme::FS_SM))
            .on_hover_text("Stop writing poses into the grid.")
            .clicked()
        {
            actions.push(StepAction::SetArmed(false));
        }
        return;
    }

    let response = ui.add_enabled(
        blocked.is_none(),
        egui::Button::new(
            egui::RichText::new("\u{23FA} Arm record")
                .size(theme::FS_SM)
                .color(theme::LIVE_CORAL),
        )
        .corner_radius(theme::R_CONTROL),
    );
    let response = match blocked {
        Some(why) => response.on_disabled_hover_text(why),
        None => response.on_hover_text(
            "Write the pose you are holding into each step as the playhead crosses it.",
        ),
    };
    if response.clicked() {
        actions.push(StepAction::SetArmed(true));
    }
}

/// Four dots: where the grid is in the bar.
fn beat_dots(ui: &mut egui::Ui, seq: &animus_runtime::Sequencer) {
    const N: usize = 4;
    let here = seq.current() % N;
    ui.add_space(theme::S_SM);
    for i in 0..N {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(7.0, 7.0), egui::Sense::hover());
        let lit = seq.running && i == here;
        ui.painter().circle_filled(
            rect.center(),
            3.0,
            if lit { theme::GO_GREEN } else { theme::GHOST },
        );
    }
}

/// Tall enough for a number, a state and a readout.
const PAD_HEIGHT: f32 = 56.0;

/// The grid. Four to a row whatever the length, so a bar reads as a bar and
/// 16 reads as four of them.
fn step_grid(
    ui: &mut egui::Ui,
    seq: &animus_runtime::Sequencer,
    mode: EditMode,
    actions: &mut Vec<StepAction>,
) {
    // Eight across: a bar of eight is the shape an operator reads at a
    // glance, and wrapping it into two rows of four turns one bar into what
    // looks like two.
    const PER_ROW: usize = 8;
    const PAD_CHROME: f32 = theme::S_SM * 2.0 + 2.0;

    let playhead = seq.running.then(|| seq.current());
    let spacing = ui.spacing().item_spacing.x;
    let budget =
        ui.available_width() - spacing * (PER_ROW as f32 - 1.0) - PAD_CHROME * PER_ROW as f32;
    let width = (budget / PER_ROW as f32).max(70.0);

    let count = seq.len();
    for row in 0..count.div_ceil(PER_ROW) {
        // `horizontal_top`: `horizontal` centres items of unequal height
        // against each other, which stepped each pad down the row.
        ui.horizontal_top(|ui| {
            for col in 0..PER_ROW {
                let i = row * PER_ROW + col;
                if i < count {
                    step_pad(ui, seq, mode, i, playhead == Some(i), width, actions);
                }
            }
        });
    }
}

/// One step: a pose, or the room for one.
#[allow(clippy::too_many_arguments)]
fn step_pad(
    ui: &mut egui::Ui,
    seq: &animus_runtime::Sequencer,
    mode: EditMode,
    index: usize,
    under_playhead: bool,
    width: f32,
    actions: &mut Vec<StepAction>,
) {
    let posed = seq.pose(index);
    let selected = seq.selected == index;
    let editing = mode == EditMode::Edit;

    let accent = if under_playhead {
        theme::GO_GREEN
    } else if selected && editing {
        theme::BRIGHT
    } else if posed.is_some() {
        theme::SOFT
    } else {
        theme::GHOST
    };

    let fill = if under_playhead {
        theme::PLAYING_CARD
    } else if posed.is_some() {
        theme::WELL
    } else {
        egui::Color32::TRANSPARENT
    };
    let edge = if under_playhead || selected {
        accent
    } else {
        theme::SEAM
    };

    let actions_before = actions.len();
    let response = egui::Frame::NONE
        .fill(fill)
        .stroke(egui::Stroke::new(1.0_f32, edge))
        .corner_radius(theme::R_CARD)
        .inner_margin(egui::Margin::symmetric(
            theme::S_SM as i8,
            theme::S_XS as i8,
        ))
        .show(ui, |ui| {
            ui.set_width(width);
            ui.set_height(PAD_HEIGHT);
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(format!("{}", index + 1))
                            .monospace()
                            .size(theme::FS_LABEL)
                            .color(accent),
                    );
                    if selected && editing {
                        ui.label(
                            egui::RichText::new("EDITING")
                                .monospace()
                                .size(theme::FS_MICRO)
                                .color(theme::BRIGHT),
                        );
                    }
                    if posed.is_some() {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .small_button("\u{00D7}")
                                .on_hover_text("Clear this step. It becomes a rest.")
                                .clicked()
                            {
                                actions.push(StepAction::Clear(index));
                            }
                        });
                    }
                });

                match posed {
                    Some(pose) => {
                        ui.label(
                            egui::RichText::new("posed")
                                .size(theme::FS_SM)
                                .color(theme::SOFT),
                        );
                        ui.label(data(format!("{} joints", pose.len())));
                    }
                    None => {
                        // A rest is a choice, so it says so rather than
                        // looking like a step that failed to load.
                        ui.label(
                            egui::RichText::new("rest")
                                .size(theme::FS_SM)
                                .color(theme::GHOST),
                        );
                    }
                }
            });
        })
        .response;

    let child_took_it = actions.len() > actions_before;
    let hit = ui.interact(
        response.rect,
        ui.id().with(("step", index)),
        egui::Sense::click(),
    );
    if hit.clicked() && !child_took_it {
        actions.push(StepAction::Select(index));
    }
    hit.on_hover_text(match (posed.is_some(), editing) {
        (_, true) => "Click to edit this step. The puppet jumps to its pose.",
        (true, false) => "Holds a pose. Switch to EDIT to change it.",
        (false, false) => "A rest: nothing is written, so the springs carry on.",
    });
}

#[allow(clippy::too_many_arguments)]
pub fn draw(
    ctx: &egui::Context,
    state: &mut EditorState,
    doc: &DocumentRes,
    viewport_texture: Option<egui::TextureId>,
    target: Option<&ViewportTarget>,
    import_status: &ImportStatus,
    output: Option<OutputInfo>,
    seq: &animus_runtime::Sequencer,
    file_status: Option<&str>,
) -> DockOutput {
    // `CentralPanel::show` is deprecated in egui 0.34 in favour of
    // `show_inside`, which needs a `Ui` — and there is no non-deprecated way
    // to get a root `Ui` from a `Context`. egui's own `show_dyn` carries the
    // same `expect(deprecated)` for the same reason. Revisit when egui offers
    // a top-level replacement.
    #![expect(deprecated)]
    let mut out = DockOutput::default();

    // Order matters and is the whole trick: egui gives each panel its slice
    // in declaration order and the central panel takes what is left, so the
    // chrome must be declared before the dock.
    //
    // An earlier note here claimed panels were painted over by the central
    // panel and that only a foreground `Area` could survive. That was wrong,
    // and wrong for an embarrassing reason: the screenshots it was based on
    // were cropped by a DPI-unaware capture process, so the bottom fifth of
    // the window — exactly where a bottom panel lands — was never in frame.
    // The panels had been working the whole time.
    egui::TopBottomPanel::top("animus_titlebar")
        .exact_height(chrome::TITLE_HEIGHT)
        .frame(
            egui::Frame::NONE
                .fill(theme::STATUS_BG)
                .stroke(egui::Stroke::NONE),
        )
        .show(ctx, |ui| {
            chrome::title_bar(
                ui,
                state,
                &doc.0,
                output.as_ref(),
                seq.filled(),
                seq.armed,
                file_status,
                &mut out.file_request,
                &mut out.wants_undo,
                &mut out.wants_redo,
            );
        });

    egui::TopBottomPanel::top("animus_stages")
        .exact_height(chrome::STAGE_HEIGHT)
        .frame(egui::Frame::NONE.fill(theme::APP_BG))
        .show(ctx, |ui| {
            chrome::stage_bar(ui, state, &doc.0);
            let y = ui.max_rect().max.y;
            ui.painter().hline(
                ui.max_rect().x_range(),
                y,
                egui::Stroke::new(1.0_f32, theme::SEAM),
            );
        });

    // The launcher drives one puppet; name it rather than leaving the
    // operator to infer it from what moves.
    let puppet_name = doc
        .0
        .puppets
        .values()
        .next()
        .map(|p| p.name.clone())
        .unwrap_or_else(|| "no puppet".into());

    // **Draggable, not fixed.** A pattern with more tracks than fit is the
    // normal case once a puppet has a few limbs, and the alternative to
    // dragging this edge up is scrolling a grid whose rows are the point.
    // Collapsed is still exact: folded away means folded away.
    let sequencer = egui::TopBottomPanel::bottom("animus_clips");
    let sequencer = if state.clips_collapsed {
        sequencer.exact_height(STRIP_COLLAPSED)
    } else {
        sequencer
            .resizable(true)
            .default_height(state.panels.sequencer)
            .height_range(PanelSizes::SEQUENCER)
    };
    sequencer
        .frame(
            egui::Frame::NONE
                .fill(theme::SIDE_PANEL)
                .inner_margin(egui::Margin::symmetric(
                    theme::S_MD as i8,
                    theme::S_SM as i8,
                )),
        )
        .show(ctx, |ui| {
            ui.painter().hline(
                ui.max_rect().expand(theme::S_MD).x_range(),
                ui.max_rect().min.y - theme::S_SM,
                egui::Stroke::new(1.0_f32, theme::SEAM),
            );
            steps_strip(
                ui,
                seq,
                state.mode,
                &puppet_name,
                state.clips_collapsed,
                &mut out.step_actions,
            );
        });
    if !state.clips_collapsed {
        // Read back what the drag settled on, so it outlives the session.
        if let Some(rect) = ctx.memory(|m| m.area_rect("animus_clips")) {
            state.panels.sequencer = rect.height();
        }
    }

    let mut viewer = TabViewer {
        state,
        doc: &doc.0,
        viewport_texture,
        target,
        import_status,
        output: output.clone(),
        inspector_edits: Vec::new(),
        layer_move: None,
        layer_edits: Vec::new(),
        set_output_vsync: None,
        output_edits: OutputEdits::default(),
        wants_reset_pose: false,
        set_live_rotation: None,
        wants_fit: false,
        wants_undo: false,
        wants_redo: false,
        viewport_input: None,
    };

    // Declaration order is the layout: egui hands each panel its slice in
    // turn and the central panel takes what is left. The sidebars come after
    // the bottom strip declared above, so the sequencer spans the full width
    // the way the comp draws it.
    let mut sizes = viewer.state.panels;

    egui::SidePanel::left("animus_left")
        .resizable(true)
        .default_width(sizes.left)
        .width_range(PanelSizes::LEFT)
        .frame(sidebar_frame())
        .show(ctx, |ui| {
            sizes.left = ui.max_rect().width();
            let active = LeftTab::ALL
                .iter()
                .position(|t| *t == viewer.state.left_tab)
                .unwrap_or(0);
            let labels: Vec<&str> = LeftTab::ALL.iter().map(|t| t.label()).collect();
            if let Some(i) = crate::widgets::tab_bar(ui, &labels, active) {
                viewer.state.left_tab = LeftTab::ALL[i];
            }
            ui.add_space(theme::S_MD);
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| match viewer.state.left_tab {
                    LeftTab::Scene => viewer.scene(ui),
                    LeftTab::Assets => viewer.assets(ui),
                    LeftTab::Tools => viewer.tools_body(ui),
                });
        });

    egui::SidePanel::right("animus_right")
        .resizable(true)
        .default_width(sizes.right)
        .width_range(PanelSizes::RIGHT)
        .frame(sidebar_frame())
        .show(ctx, |ui| {
            sizes.right = ui.max_rect().width();
            let active = RightTab::ALL
                .iter()
                .position(|t| *t == viewer.state.right_tab)
                .unwrap_or(0);
            let labels: Vec<&str> = RightTab::ALL.iter().map(|t| t.label()).collect();
            if let Some(i) = crate::widgets::tab_bar(ui, &labels, active) {
                viewer.state.right_tab = RightTab::ALL[i];
            }
            ui.add_space(theme::S_MD);
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| match viewer.state.right_tab {
                    RightTab::Inspect => viewer.inspector(ui),
                    RightTab::Physics => viewer.solver(ui),
                    RightTab::Channels => stub(
                        ui,
                        "Live channels appear here once a source is connected.",
                        "OSC and MIDI are being wired up.",
                    ),
                    RightTab::Bind => stub(
                        ui,
                        "Bindings map a channel onto a parameter.",
                        "Use the mark beside any value to start one.",
                    ),
                });
        });

    egui::CentralPanel::default()
        .frame(egui::Frame::NONE.fill(theme::APP_BG))
        .show(ctx, |ui| viewer.viewport(ui));

    // **Output settings live behind the output chip**, not in a panel of
    // their own. The comp puts the *state* there — OFF, PREVIEW, LIVE — and
    // the settings belong with the state that reports them: an operator who
    // wants to know where the show is going and one who wants to change it
    // are reaching for the same thing.
    if viewer.state.output_menu_open {
        let mut open = true;
        egui::Window::new("Output")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(300.0)
            .anchor(
                egui::Align2::RIGHT_TOP,
                [-theme::S_MD, chrome::TITLE_HEIGHT],
            )
            .frame(
                egui::Frame::NONE
                    .fill(theme::MENU_BG)
                    .stroke(egui::Stroke::new(1.0_f32, theme::SEAM))
                    .corner_radius(theme::R_CARD)
                    .inner_margin(egui::Margin::same(theme::S_MD as i8)),
            )
            .show(ctx, |ui| viewer.output_body(ui));
        viewer.state.output_menu_open = open;
    }

    viewer.state.panels = sizes;
    out.viewport_input = viewer.viewport_input;
    out.inspector_edits = std::mem::take(&mut viewer.inspector_edits);
    out.layer_move = viewer.layer_move;
    out.layer_edits = std::mem::take(&mut viewer.layer_edits);
    out.set_output_vsync = viewer.set_output_vsync;
    out.output_edits = viewer.output_edits;
    out.wants_reset_pose = viewer.wants_reset_pose;
    out.set_live_rotation = viewer.set_live_rotation;
    out.wants_fit = viewer.wants_fit;
    out.wants_undo = viewer.wants_undo;
    out.wants_redo = viewer.wants_redo;

    out
}

/// Both sidebars, so they cannot drift apart.
fn sidebar_frame() -> egui::Frame {
    egui::Frame::NONE
        .fill(theme::SIDE_PANEL)
        .inner_margin(egui::Margin::symmetric(
            theme::S_SM as i8,
            theme::S_SM as i8,
        ))
}
