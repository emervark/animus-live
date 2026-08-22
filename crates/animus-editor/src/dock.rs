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
    /// Choose the track TAP writes into.
    SelectTrack(usize),
    /// Empty → full → ghost → empty.
    Cycle(usize, usize),
    /// Right-click: straight back to empty.
    ClearCell(usize, usize),
    ClearTrack(usize),
    ClearAll,
    /// Give the selected joint a row of its own.
    AddTrack,
    RemoveTrack(usize),
    ToggleMute(usize),
    ToggleSolo(usize),
    /// Held this frame: hit the selected track, and write it if armed.
    Tap,
    /// Stop and return to the top of the bar.
    Stop,
    SetRunning(bool),
    SetArmed(bool),
    SetLength(usize),
    SetQuantize(animus_runtime::Quantize),
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
    /// Lock the layer against the pointer, or unlock it.
    SetLocked(animus_core::ids::LayerId, bool),
}

/// Which of a joint's three parameters a Learn is arming against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LearnAxis {
    X,
    Y,
    Rotation,
}

/// What the CHANNELS and BIND panels asked for this frame.
///
/// Intents, like every other panel output: the panel names the act and one
/// writer performs it. The bus is live data arriving off a thread, and a
/// panel reaching into it mid-frame would be reading and writing the same
/// thing at once.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SignalEdit {
    ToggleChannel(animus_signal::ChannelId),
    ToggleBinding(usize),
    Remove(usize),
    /// Arm Learn against the selected joint, on this axis.
    Learn(LearnAxis),
    /// Finish an armed Learn by naming the source rather than moving it.
    BindArmedTo(animus_signal::ChannelId),
    CancelLearn,
    /// Widen or narrow how far a binding moves its joint.
    SetRange(usize, f32),
    /// Add a generator channel. `true` locks it to the transport.
    AddGenerator(bool),
    SetShape(animus_signal::ChannelId, animus_signal::Shape),
    SetRate(animus_signal::ChannelId, f32),
    /// Show or hide a binding's envelope editor.
    ToggleEnvelope(usize),
    /// Whatever the envelope editor asked for, on this binding.
    Envelope(usize, crate::widgets::EnvelopeEdit),
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
    /// What the CHANNELS and BIND panels asked for.
    pub signal_edits: Vec<SignalEdit>,
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
    // Wrapped, like every other sentence in a sidebar: a label that refuses
    // to wrap reports its full width as a requirement and holds the panel
    // open against the operator's drag.
    ui.add(
        egui::Label::new(
            egui::RichText::new(what)
                .size(theme::FS_CONTROL)
                .color(theme::DIM),
        )
        .wrap(),
    );
    ui.add_space(theme::S_XS);
    ui.add(
        egui::Label::new(
            egui::RichText::new(when)
                .size(theme::FS_SM)
                .color(theme::FAINT),
        )
        .wrap(),
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
    /// The live bus, if it is running. Read-only here.
    pub signal: Option<&'a animus_runtime::SignalBusRes>,
    /// What its sources managed to open.
    pub signal_status: Option<&'a animus_runtime::SignalStatus>,
    /// Filled by the Inspector tab; the writer system applies them.
    pub inspector_edits: Vec<InspectorEdit>,
    /// Filled by CHANNELS and BIND.
    pub signal_edits: Vec<SignalEdit>,
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
        self.viewport_toolbar(ui);
        // The strip is a row of its own rather than an overlay: it is text
        // the operator reads *about* the stage, and text painted on top of a
        // black canvas competes with the puppet for the same pixels.
        let strip = theme::FS_TINY + theme::S_MD;
        let available = ui.available_size();
        let size = egui::vec2(available.x.max(16.0), (available.y - strip).max(16.0));
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
        self.stage_readout(ui, input.rect);
        self.viewport_input = Some(input);
        self.status_strip(ui);
    }

    /// The bar over the stage: how it is framed, and what is drawn on it.
    ///
    /// Framing on the left, overlays in the middle, what-surface-is-this on
    /// the right — the comp's order, and the order the questions arrive in.
    fn viewport_toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = theme::S_XS;
            ui.add_space(theme::S_SM);

            if ui
                .add(
                    egui::Button::new(
                        egui::RichText::new("Fit")
                            .size(theme::FS_SM)
                            .color(theme::MID),
                    )
                    .fill(theme::WELL)
                    .corner_radius(theme::R_BADGE),
                )
                .on_hover_text("Frame everything — F")
                .clicked()
            {
                self.wants_fit = true;
            }

            if let Some(target) = self.target {
                // Zoom as a percentage of one image pixel to one screen
                // pixel, which is the only ratio that means anything while
                // placing a joint on a knuckle.
                let percent = if target.world_per_pixel > 0.0 {
                    100.0 / (target.world_per_pixel * 100.0)
                } else {
                    100.0
                };
                ui.label(
                    egui::RichText::new(format!("{percent:.0}%"))
                        .monospace()
                        .size(theme::FS_TINY)
                        .color(theme::DIM),
                );
            }

            ui.add_space(theme::S_SM);
            separator(ui);
            ui.add_space(theme::S_SM);

            for (i, (name, tip, ready)) in crate::state::Overlays::ALL.iter().enumerate() {
                let on = *ready && self.state.overlays.get(i);
                let ink = match (*ready, on) {
                    (false, _) => theme::DISABLED,
                    (true, true) => theme::MID,
                    (true, false) => theme::FAINT,
                };
                if crate::widgets::chip(ui, name, on, ink)
                    .on_hover_text(*tip)
                    .clicked()
                    && *ready
                {
                    self.state.overlays.toggle(i);
                }
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(theme::S_SM);
                let on = self.state.show_dev_inspector;
                if crate::widgets::chip(
                    ui,
                    "DIAGNOSTICS",
                    on,
                    if on { theme::DATA_CYAN } else { theme::FAINT },
                )
                .on_hover_text("Show solver and cursor diagnostics under the stage")
                .clicked()
                {
                    self.state.show_dev_inspector = !on;
                }
            });
        });
        ui.add_space(theme::S_XS);
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
    /// The stage's own caption, inside the canvas at the bottom left.
    ///
    /// The output size first, because "what will the audience see" is the
    /// question a stage answers. World-per-pixel follows because every
    /// offset in this editor is quoted in pixels, and without the conversion
    /// on screen a coordinate bug is a guessing game — M0-2 spent three
    /// rounds on one for want of this line.
    fn stage_readout(&self, ui: &mut egui::Ui, rect: egui::Rect) {
        let Some(target) = self.target else { return };
        let canvas = self.doc.stage.canvas;
        let p = ui.painter();

        let head = format!(
            "STAGE {} \u{00d7} {}    1 px = {:.4} world",
            canvas[0], canvas[1], target.world_per_pixel
        );
        p.text(
            egui::pos2(rect.min.x + theme::S_MD, rect.max.y - theme::S_MD),
            egui::Align2::LEFT_BOTTOM,
            head,
            egui::FontId::monospace(theme::FS_TINY),
            theme::DIM,
        );

        // Cursor and click coordinates are diagnostics, and stay behind the
        // toggle that says so.
        if self.state.show_dev_inspector {
            let mut diag = format!("target {} \u{00d7} {}", target.size.x, target.size.y);
            if let Some(c) = target.cursor_world {
                diag.push_str(&format!("    cursor {:.2}, {:.2}", c.x, c.y));
            }
            if let Some(c) = target.last_click_world {
                diag.push_str(&format!("    click {:.2}, {:.2}", c.x, c.y));
            }
            p.text(
                egui::pos2(
                    rect.min.x + theme::S_MD,
                    rect.max.y - theme::S_MD - theme::FS_TINY - theme::S_XS,
                ),
                egui::Align2::LEFT_BOTTOM,
                diag,
                egui::FontId::monospace(theme::FS_TINY),
                theme::HINT,
            );
        }
    }

    /// One line under the stage: everything true about the show right now.
    ///
    /// Six facts an operator would otherwise have to hunt for in six places.
    /// Each is a label and a value, and the value takes the Signal Rule's
    /// colour when it is a state worth noticing.
    fn status_strip(&self, ui: &mut egui::Ui) {
        let height = theme::FS_TINY + theme::S_MD;
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), height),
            egui::Sense::hover(),
        );
        let p = ui.painter();
        p.rect_filled(rect, 0.0, theme::STATUS_BG);

        let selection = match self.state.selection {
            Selection::None => "none".to_string(),
            Selection::Layer(_) => "layer".to_string(),
            Selection::Puppet(_) => "puppet".to_string(),
            Selection::Joint(_, j) => format!("joint {}", j.0),
            Selection::Bone(_, b) => format!("bone {}", b.0),
        };
        let (solver, solver_ink) = if self.doc.solver.enabled {
            (format!("{} Hz", self.doc.solver.hz), theme::GO_GREEN)
        } else {
            ("paused".to_string(), theme::CAUTION_AMBER)
        };
        let (output, output_ink) = match &self.output {
            Some(o) => (o.short.clone(), theme::LIVE_CORAL),
            None => ("off".to_string(), theme::DIM),
        };

        let mut x = rect.min.x + theme::S_MD;
        for (name, value, ink) in [
            ("selection", selection.as_str(), theme::MID),
            ("tool", self.state.tool.label(), theme::MID),
            ("solver", solver.as_str(), solver_ink),
            ("output", output.as_str(), output_ink),
        ] {
            let ng = p.layout_no_wrap(
                name.to_string(),
                egui::FontId::monospace(theme::FS_TINY),
                theme::FAINT,
            );
            let vg = p.layout_no_wrap(
                value.to_string(),
                egui::FontId::monospace(theme::FS_TINY),
                ink,
            );
            let y = rect.center().y - ng.size().y * 0.5;
            let nw = ng.size().x;
            let vw = vg.size().x;
            p.galley(egui::pos2(x, y), ng, theme::FAINT);
            p.galley(egui::pos2(x + nw + theme::S_2XS, y), vg, ink);
            x += nw + vw + theme::S_2XS + theme::S_LG;
        }
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
        crate::widgets::panel_header(ui, "Layers", Some(&format!("{}", self.doc.layers.len())));
        ui.add_space(theme::S_XS);

        if self.doc.layers.is_empty() {
            stub(ui, "No layers yet.", "Import an image to make one.");
            return;
        }

        // Painted back to front, so the topmost layer reads at the top of the
        // list — the order an artist expects, not the order the Vec is in.
        let layer_ids: Vec<_> = self.doc.layers.iter().rev().copied().collect();

        for layer_id in layer_ids {
            let Some(layer) = self.doc.layer_data.get(&layer_id) else {
                continue;
            };
            let selected = self.state.selection == Selection::Layer(layer_id);
            let visible = layer.visible;
            let locked = layer.locked;

            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = theme::S_XS;

                // The grip marks where a row can be taken hold of. Inert for
                // now, and drawn anyway: the comp reorders by dragging, and a
                // handle that appears only once dragging works would move
                // every row sideways on the day it lands.
                let (grip, _) =
                    ui.allocate_exact_size(egui::vec2(10.0, 18.0), egui::Sense::hover());
                icons::draw(
                    ui.painter(),
                    icons::Icon::Grip,
                    grip.shrink2(egui::vec2(1.0, 4.0)),
                    theme::GHOST,
                );

                const ICON: f32 = 20.0;
                let gap = ui.spacing().item_spacing.x;
                let name_width = (ui.available_width() - (ICON + gap) * 2.0).max(40.0);

                let ink = match (selected, visible) {
                    (true, true) => theme::BRIGHT,
                    (true, false) => theme::SOFT,
                    // A hidden layer is dimmed in the list as well, so the
                    // list and the stage agree without anyone checking.
                    (false, true) => theme::INK,
                    (false, false) => theme::FAINT,
                };
                // `add_sized`, not `min_size`: a minimum lets a long name grow
                // past the budget and push the icons off the panel edge, which
                // is how the delete control disappeared on the first layer
                // whose name did not happen to be short.
                let name = ui
                    .add_sized(
                        egui::vec2(name_width, ui.spacing().interact_size.y),
                        egui::Button::new(
                            egui::RichText::new(&layer.name)
                                .size(theme::FS_CONTROL)
                                .color(ink),
                        )
                        .fill(if selected {
                            theme::SELECT_WASH
                        } else {
                            egui::Color32::TRANSPARENT
                        })
                        .corner_radius(theme::R_INPUT)
                        .truncate(),
                    )
                    .on_hover_text(format!("{} · z {:.2}", layer.name, layer.depth));
                if name.clicked() {
                    self.state.selection = Selection::Layer(layer_id);
                }

                // **The rest live in the row's own menu.** The comp's row
                // carries three controls; ours has six things it can do, and
                // six icons in a 260px column is a row nobody can read. What
                // stays out is what an operator touches mid-show.
                name.context_menu(|ui| {
                    if ui.button("Bring forward").clicked() {
                        self.layer_move = Some((layer_id, 1));
                        ui.close();
                    }
                    if ui.button("Send backward").clicked() {
                        self.layer_move = Some((layer_id, -1));
                        ui.close();
                    }
                    if ui.button("Duplicate").clicked() {
                        self.layer_edits.push(LayerEdit::Duplicate(layer_id));
                        ui.close();
                    }
                    if ui.button("Reset position").clicked() {
                        self.layer_edits.push(LayerEdit::ResetPlacement(layer_id));
                        ui.close();
                    }
                    ui.separator();
                    if ui
                        .button(egui::RichText::new("Delete layer").color(theme::LIVE_CORAL))
                        .on_hover_text("Ctrl+Z brings it back.")
                        .clicked()
                    {
                        self.layer_edits.push(LayerEdit::Delete(layer_id));
                        ui.close();
                    }
                });

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

                // **Locked is not hidden.** A backdrop has to stay on screen
                // while the operator works over the top of it, and hiding it
                // to stop grabbing it by accident means working blind.
                if icons::button(
                    ui,
                    if locked {
                        icons::Icon::Lock
                    } else {
                        icons::Icon::Unlock
                    },
                    locked,
                    locked.then_some(theme::CAUTION_AMBER),
                )
                .on_hover_text(if locked {
                    "Unlock. The layer can be selected and moved again."
                } else {
                    "Lock. The layer stays on screen but ignores the pointer."
                })
                .clicked()
                {
                    self.layer_edits
                        .push(LayerEdit::SetLocked(layer_id, !locked));
                }
            });
        }
    }

    /// CHANNELS: what is arriving from outside, right now.
    ///
    /// **Nothing here is a list of what a controller might send.** A row
    /// appears the first time a value lands on it, named after where it came
    /// from — which is how an operator finds the knob they want: by turning
    /// it, not by reading a manual.
    fn channels(&mut self, ui: &mut egui::Ui) {
        let Some(signal) = self.signal else {
            stub(
                ui,
                "The signal bus is not running.",
                "MIDI and OSC open when the editor starts.",
            );
            return;
        };
        let status = self.signal_status;

        crate::widgets::panel_header(
            ui,
            "Live inputs",
            Some(&format!("{}", signal.bus.channels.len())),
        );
        ui.add_space(theme::S_XS);

        // What opened, said plainly. A port that failed is the single most
        // useful thing this panel can tell someone whose controller is not
        // working, and it is exactly what a decorative panel would hide.
        if let Some(status) = status {
            for (name, live, detail) in [
                ("MIDI", status.midi_live(), status.midi_summary()),
                ("OSC", status.osc_live(), status.osc_summary()),
            ] {
                ui.horizontal(|ui| {
                    crate::widgets::chip(
                        ui,
                        name,
                        live,
                        if live { theme::DATA_CYAN } else { theme::DIM },
                    );
                    ui.label(
                        egui::RichText::new(detail)
                            .size(theme::FS_SM)
                            .color(if live { theme::DIM } else { theme::HINT }),
                    );
                });
            }
        }

        // **A generator is a channel like any other**, so it is added here
        // rather than in a system of its own: an LFO and a fader are the
        // same kind of thing to everything downstream, and the panel should
        // not pretend otherwise.
        ui.add_space(theme::S_SM);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = theme::S_XS;
            if crate::widgets::chip(ui, "+ LFO", false, theme::MID)
                .on_hover_text("A free-running oscillator with its own rate")
                .clicked()
            {
                self.signal_edits.push(SignalEdit::AddGenerator(false));
            }
            if crate::widgets::chip(ui, "+ SYNC", false, theme::MID)
                .on_hover_text("An oscillator locked to the transport, in beats")
                .clicked()
            {
                self.signal_edits.push(SignalEdit::AddGenerator(true));
            }
        });

        ui.add_space(theme::S_SM);
        if signal.bus.channels.is_empty() {
            crate::widgets::note(
                ui,
                "Nothing yet. Move a knob, a fader or a phone and it will \
                 appear here — or add an LFO above.",
            );
            return;
        }

        crate::widgets::note(
            ui,
            "A connected source does not move the puppet until it is bound.",
        );
        ui.add_space(theme::S_SM);

        for channel in &signal.bus.channels {
            let bound = signal.bus.bindings.iter().any(|b| b.src == channel.id);
            ui.horizontal(|ui| {
                let on = channel.on;
                if crate::widgets::chip(
                    ui,
                    if on { "ON" } else { "OFF" },
                    on,
                    if on { theme::GO_GREEN } else { theme::FAINT },
                )
                .on_hover_text(if on {
                    "Silence this channel. It keeps arriving; it stops driving."
                } else {
                    "Let this channel drive again."
                })
                .clicked()
                {
                    self.signal_edits
                        .push(SignalEdit::ToggleChannel(channel.id));
                }
                ui.label(
                    egui::RichText::new(channel.source.label())
                        .monospace()
                        .size(theme::FS_SM)
                        .color(if on { theme::INK } else { theme::FAINT }),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new(format!("{:.2}", channel.value))
                            .monospace()
                            .size(theme::FS_TINY)
                            .color(theme::MID),
                    );
                    if bound {
                        ui.label(
                            egui::RichText::new("bound")
                                .monospace()
                                .size(theme::FS_MICRO)
                                .color(theme::GO_GREEN),
                        );
                    }
                });
            });
            crate::widgets::meter(
                ui,
                channel.value,
                if channel.on {
                    theme::DATA_CYAN
                } else {
                    theme::GHOST
                },
            );

            // A generator has settings; a received channel has none, because
            // what arrives is whatever the far end decided to send.
            if let Some(wave) = channel.generator {
                let locked = matches!(channel.source, animus_signal::Source::BpmSync { .. });
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = theme::S_XS;
                    for shape in animus_signal::Shape::ALL {
                        let on = shape == wave.shape;
                        if crate::widgets::chip(
                            ui,
                            shape.label(),
                            on,
                            if on { theme::BRIGHT } else { theme::DIM },
                        )
                        .clicked()
                        {
                            self.signal_edits
                                .push(SignalEdit::SetShape(channel.id, shape));
                        }
                    }
                });
                let mut rate = wave.rate;
                // Beats per cycle when locked, cycles per second when free:
                // two different questions, so two different units and two
                // different ranges rather than one slider that means
                // whichever the operator last remembered.
                let response = if locked {
                    crate::widgets::SliderRow::new("Cycle", &mut rate, 0.25..=32.0)
                        .suffix(" beats")
                        .decimals(2)
                        .default_value(4.0)
                        .show(ui)
                } else {
                    crate::widgets::SliderRow::new("Rate", &mut rate, 0.01..=8.0)
                        .suffix(" Hz")
                        .decimals(2)
                        .default_value(0.25)
                        .show(ui)
                };
                if response.changed() {
                    self.signal_edits
                        .push(SignalEdit::SetRate(channel.id, rate));
                }
            }
            ui.add_space(theme::S_2XS);
        }
    }

    /// BIND: which channel drives what, and how far.
    fn bindings(&mut self, ui: &mut egui::Ui) {
        let Some(signal) = self.signal else {
            stub(
                ui,
                "The signal bus is not running.",
                "MIDI and OSC open when the editor starts.",
            );
            return;
        };

        crate::widgets::panel_header(
            ui,
            "Bindings",
            Some(&format!("{}", signal.bus.bindings.len())),
        );
        ui.add_space(theme::S_XS);

        // Learn is armed against the selected joint, so the panel says which
        // one rather than leaving the operator to guess what they are about
        // to wire a knob to.
        match self.state.selection {
            Selection::Joint(_, joint) => {
                let learning = signal.bus.learn.is_some();
                ui.horizontal(|ui| {
                    for (label, axis) in [
                        ("Learn X", LearnAxis::X),
                        ("Learn Y", LearnAxis::Y),
                        ("Learn Rot", LearnAxis::Rotation),
                    ] {
                        if ui
                            .add(
                                egui::Button::new(
                                    egui::RichText::new(label).size(theme::FS_SM).color(
                                        if learning {
                                            theme::CAUTION_AMBER
                                        } else {
                                            theme::INK
                                        },
                                    ),
                                )
                                .fill(if learning {
                                    theme::WARN_WASH
                                } else {
                                    theme::WELL
                                })
                                .corner_radius(theme::R_BADGE),
                            )
                            .on_hover_text(
                                "Arm, then move the control you want. The next thing \
                                 that moves takes this target.",
                            )
                            .clicked()
                        {
                            self.signal_edits.push(SignalEdit::Learn(axis));
                        }
                    }
                    if learning
                        && ui
                            .add(
                                egui::Button::new(
                                    egui::RichText::new("Cancel")
                                        .size(theme::FS_SM)
                                        .color(theme::SUB),
                                )
                                .fill(theme::WELL)
                                .corner_radius(theme::R_BADGE),
                            )
                            .clicked()
                    {
                        self.signal_edits.push(SignalEdit::CancelLearn);
                    }
                });
                crate::widgets::note(
                    ui,
                    &if signal.bus.learn.is_some() {
                        format!(
                            "Armed for joint {}. Move a control, or pick a source below.",
                            joint.0
                        )
                    } else {
                        format!("Arming will wire a control to joint {}.", joint.0)
                    },
                );

                // **Two ways to finish an arming.** A generator never
                // arrives, so Learn alone could never catch one; and an LFO
                // that claimed an arming by itself would steal every binding
                // from the fader the operator was reaching for, because it
                // is always moving.
                if learning && !signal.bus.channels.is_empty() {
                    ui.add_space(theme::S_XS);
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(theme::S_XS, theme::S_XS);
                        for channel in &signal.bus.channels {
                            if crate::widgets::chip(
                                ui,
                                &channel.source.label(),
                                false,
                                theme::DATA_CYAN,
                            )
                            .on_hover_text("Wire this source to the armed target")
                            .clicked()
                            {
                                self.signal_edits.push(SignalEdit::BindArmedTo(channel.id));
                            }
                        }
                    });
                }
            }
            _ => crate::widgets::note(ui, "Select a joint to bind a control to it."),
        }

        ui.add_space(theme::S_SM);
        if signal.bus.bindings.is_empty() {
            crate::widgets::note(ui, "Nothing is bound yet.");
            return;
        }

        for (index, binding) in signal.bus.bindings.iter().enumerate() {
            let source = signal
                .bus
                .channel(binding.src)
                .map(|c| c.source.label())
                .unwrap_or_else(|| "gone".into());
            let (_, joint) = binding.dst.joint();

            ui.horizontal(|ui| {
                let on = binding.on;
                if crate::widgets::chip(
                    ui,
                    if on { "ON" } else { "OFF" },
                    on,
                    if on { theme::GO_GREEN } else { theme::FAINT },
                )
                .clicked()
                {
                    self.signal_edits.push(SignalEdit::ToggleBinding(index));
                }
                ui.label(
                    egui::RichText::new(source)
                        .monospace()
                        .size(theme::FS_TINY)
                        .color(theme::MID),
                );
                // Drawn, not typed: U+2192 is missing from egui built-in
                // faces and renders as a hollow box, which reads as a broken
                // row rather than as "this drives that".
                let (arrow, _) =
                    ui.allocate_exact_size(egui::vec2(16.0, 12.0), egui::Sense::hover());
                icons::draw(ui.painter(), icons::Icon::ArrowRight, arrow, theme::FAINT);
                ui.label(
                    egui::RichText::new(format!("joint {} {}", joint.0, binding.dst.axis()))
                        .monospace()
                        .size(theme::FS_TINY)
                        .color(theme::INK),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if icons::button(ui, icons::Icon::Trash, false, Some(theme::LIVE_CORAL))
                        .on_hover_text("Remove this binding.")
                        .clicked()
                    {
                        self.signal_edits.push(SignalEdit::Remove(index));
                    }
                });
            });

            let mut span = binding.high;
            let rotation = matches!(binding.dst, animus_signal::Target::JointRotation(..));
            let response = crate::widgets::SliderRow::new(
                "Range",
                &mut span,
                if rotation { 1.0..=180.0 } else { 4.0..=400.0 },
            )
            .suffix(binding.dst.unit())
            .decimals(0)
            .default_value(binding.dst.default_range())
            .show(ui);
            if response.changed() {
                self.signal_edits.push(SignalEdit::SetRange(index, span));
            }

            // **The envelope opens where the binding is.** Resolume puts it
            // on the parameter for the same reason: the curve and the thing
            // it shapes have to be readable together, or the operator is
            // tuning a shape they cannot see the effect of.
            let open = self.state.open_envelope == Some(index);
            ui.horizontal(|ui| {
                if crate::widgets::chip(
                    ui,
                    "ENVELOPE",
                    open || !binding.envelope.is_identity(),
                    if open {
                        theme::BRIGHT
                    } else if binding.envelope.is_identity() {
                        theme::DIM
                    } else {
                        theme::GO_GREEN
                    },
                )
                .on_hover_text(
                    "Shape whatever arrives on this channel. Double-click the \
                     curve to add a keyframe, double-click one to remove it, \
                     right-click for its easing.",
                )
                .clicked()
                {
                    self.signal_edits.push(SignalEdit::ToggleEnvelope(index));
                }
                if !binding.envelope.is_identity() {
                    ui.label(
                        egui::RichText::new(format!("{} keys", binding.envelope.len()))
                            .monospace()
                            .size(theme::FS_MICRO)
                            .color(theme::HINT),
                    );
                }
            });
            if open {
                let live = signal.bus.channel(binding.src).map(|c| c.value);
                if let Some(edit) = crate::widgets::envelope_editor(ui, &binding.envelope, live) {
                    self.signal_edits.push(SignalEdit::Envelope(index, edit));
                }
            }
            ui.add_space(theme::S_2XS);
        }

        ui.add_space(theme::S_SM);
        crate::widgets::note(
            ui,
            "Your hand always wins: dragging a bound joint overrides its \
             binding until you let go.",
        );
    }

    /// Three presets, and what each one costs.
    ///
    /// **The cost is on screen, not in a manual.** Iterations and rate are
    /// the two numbers that decide whether a show runs at all on the machine
    /// it is running on, and an operator choosing between them at a venue
    /// half an hour before doors has no way to measure. Naming the trade —
    /// looser and cheap, or tight and expensive — is what makes the choice
    /// possible without the measurement.
    fn solver_quality(&mut self, ui: &mut egui::Ui) {
        let s = self.doc.solver;
        // Draft, Show, Max. `hz` is not a document command yet, so the
        // presets move iterations only and the row says so rather than
        // pretending the rate changed too.
        const PRESETS: [(&str, u32, &str); 3] = [
            ("Draft", 4, "loose and cheap — rigging, not performing"),
            ("Show", 8, "the default: enough for a stage"),
            ("Max", 16, "tight and expensive — stiff rigs and close-ups"),
        ];

        let current = PRESETS
            .iter()
            .min_by_key(|(_, it, _)| s.iterations.abs_diff(*it))
            .map(|(name, ..)| *name);

        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = theme::S_XS;
            for (name, iterations, _) in PRESETS {
                let on = current == Some(name) && s.iterations == iterations;
                if crate::widgets::chip(ui, name, on, if on { theme::BRIGHT } else { theme::DIM })
                    .clicked()
                {
                    self.inspector_edits.push(crate::inspect::InspectorEdit {
                        command: Some(crate::inspect::InspectorCommand::SolverParam(
                            animus_core::doc::SetSolverParam {
                                param: animus_core::doc::SolverParam::Iterations,
                                from: s.iterations as f32,
                                to: iterations as f32,
                            },
                        )),
                        released: true,
                    });
                }
            }
        });
        ui.add_space(theme::S_XS);
        let detail = PRESETS
            .iter()
            .find(|(name, it, _)| current == Some(name) && s.iterations == *it)
            .map(|(_, it, why)| format!("{it} iterations · {} Hz — {why}", s.hz))
            .unwrap_or_else(|| {
                format!("{} iterations · {} Hz — tuned by hand", s.iterations, s.hz)
            });
        crate::widgets::note(ui, &detail);
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
    /// The rig, whatever kind of puppet it belongs to.
    ///
    /// **One tree for both.** A mesh puppet's joints are point masses and a
    /// model's nodes are transforms, and above this line that difference is
    /// not visible: both carry a `JointId`, both indent by their parent,
    /// both select, rotate and bind the same way. Two trees would have meant
    /// two of everything downstream.
    fn rig_tree(&mut self, ui: &mut egui::Ui) {
        // Every puppet, not the first: a show with a cutout and a model has
        // two rigs, and showing whichever happened to be inserted first is
        // how the other one becomes unreachable.
        let puppets: Vec<_> = self.doc.puppets.keys().copied().collect();
        for pid in puppets {
            self.one_rig(ui, pid);
            ui.add_space(theme::S_MD);
        }
    }

    fn one_rig(&mut self, ui: &mut egui::Ui, pid: animus_core::ids::PuppetId) {
        let Some(puppet) = self.doc.puppets.get(&pid) else {
            return;
        };
        let mesh = match &puppet.kind {
            animus_core::doc::PuppetKind::Mesh(m) => m,
            animus_core::doc::PuppetKind::Model(model) => {
                self.model_tree(ui, pid, &puppet.name, model);
                return;
            }
        };
        let pid = &pid;
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

    /// A model's skeleton, drawn exactly like a mesh puppet's.
    fn model_tree(
        &mut self,
        ui: &mut egui::Ui,
        pid: animus_core::ids::PuppetId,
        name: &str,
        model: &animus_core::doc::ModelPuppet,
    ) {
        crate::widgets::panel_header(
            ui,
            &format!("Model \u{00b7} {name}"),
            Some(&format!("{} nodes", model.nodes.len())),
        );
        ui.add_space(theme::S_XS);

        if model.nodes.is_empty() {
            crate::widgets::note(ui, "This model has no named nodes to drive.");
            return;
        }

        // Depth by walking up the parents, rather than by recursing: the
        // list is already in the file's own depth-first order, so the only
        // thing missing is how far in each row sits.
        let depth_of = |id: animus_core::ids::JointId| {
            let mut depth = 0;
            let mut at = model.node(id).and_then(|n| n.parent);
            while let Some(parent) = at {
                depth += 1;
                at = model.node(parent).and_then(|n| n.parent);
                if depth > 32 {
                    break;
                }
            }
            depth
        };

        let mut clicked = None;
        for node in &model.nodes {
            let selected = self.state.selection == Selection::Joint(pid, node.id);
            let (rect, response) = ui
                .allocate_exact_size(egui::vec2(ui.available_width(), 22.0), egui::Sense::click());
            if response.clicked() {
                clicked = Some(Selection::Joint(pid, node.id));
            }
            let p = ui.painter();
            if selected {
                p.rect_filled(rect, theme::R_BADGE as f32, theme::SELECT_WASH);
            } else if response.hovered() {
                p.rect_filled(rect, theme::R_BADGE as f32, theme::WELL);
            }

            // Indent stops climbing after ten levels. An exporter's own
            // scaffolding — `Sketchfab_model / <file>.fbx / RootNode /
            // rig_CharRoot / Object_4 / _rootJoint` — is six levels deep
            // before a bone the operator would recognise appears, and a
            // faithful indent pushes every real name off the panel. Depth
            // past that point still *reads* as deep; it just stops paying
            // for it in width.
            let depth = depth_of(node.id).min(10) as f32;
            let x = rect.min.x + theme::S_SM + depth * theme::S_SM;
            let ink = if selected { theme::BRIGHT } else { theme::MID };
            p.circle_filled(egui::pos2(x, rect.center().y), 2.0, theme::GHOST);
            let galley = p.layout_no_wrap(
                node.name.clone(),
                egui::FontId::proportional(theme::FS_SM + 0.5),
                ink,
            );
            // Clipped, not wrapped: a long bone name should run out of the
            // row rather than double its height and break the rhythm of a
            // list forty rows long.
            p.with_clip_rect(rect.intersect(p.clip_rect())).galley(
                egui::pos2(x + theme::S_SM, rect.center().y - galley.size().y * 0.5),
                galley,
                ink,
            );
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
        crate::widgets::note(
            ui,
            "Gravity pulls every unpinned joint. Damping is how fast motion dies; \
             fewer iterations is a looser, more rubbery puppet.",
        );

        crate::widgets::divider(ui);
        crate::widgets::section_label(ui, "solver quality");
        ui.add_space(theme::S_SM);
        self.solver_quality(ui);

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
/// The sequencer: transport across the top, one row per limb below.
///
/// The comp's shape, and the shape a drum machine has had since the 808 —
/// because it is the shape that lets an operator read a rhythm rather than
/// decode one.
pub fn steps_strip(
    ui: &mut egui::Ui,
    seq: &animus_runtime::Sequencer,
    mode: EditMode,
    puppet_name: &str,
    collapsed: bool,
    actions: &mut Vec<StepAction>,
) {
    transport_row(ui, seq, mode, puppet_name, collapsed, actions);
    if collapsed {
        return;
    }
    ui.add_space(theme::S_SM);

    if seq.tracks.is_empty() {
        ui.label(
            egui::RichText::new("No tracks yet.")
                .size(theme::FS_CONTROL)
                .color(theme::DIM),
        );
        ui.add_space(theme::S_XS);
        ui.label(
            egui::RichText::new(
                "Select a joint, then press + to give that limb a row. Each cell is one hit.",
            )
            .size(theme::FS_SM)
            .color(theme::HINT),
        );
        return;
    }

    egui::ScrollArea::vertical()
        .id_salt("track_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ruler(ui, seq);
            for i in 0..seq.tracks.len() {
                track_row(ui, seq, i, actions);
            }
        });
}

/// The width the track's name column takes, from the comp.
const NAME_W: f32 = 190.0;
/// The two mute/solo buttons at the end of a row.
const TAIL_W: f32 = 52.0;

/// Where the cells start and how wide each one is, so the ruler and every
/// row agree to the pixel.
fn cell_metrics(ui: &egui::Ui, len: usize) -> (f32, f32) {
    let gap = 3.0;
    let total = (ui.available_width() - NAME_W - TAIL_W).max(80.0);
    let each = ((total - gap * (len.saturating_sub(1)) as f32) / len.max(1) as f32).max(6.0);
    (each, gap)
}

/// Step numbers over the grid, with the downbeats lit.
fn ruler(ui: &mut egui::Ui, seq: &animus_runtime::Sequencer) {
    let (each, gap) = cell_metrics(ui, seq.len);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        ui.add_space(NAME_W);
        for i in 0..seq.len {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(each, 12.0), egui::Sense::hover());
            let here = seq.running && i == seq.current();
            let downbeat = i % 4 == 0;
            let ink = if here {
                theme::GO_GREEN
            } else if downbeat {
                theme::SUB
            } else {
                theme::GHOST
            };
            // Only the downbeats are numbered. Sixteen numbers in a row is a
            // ruler nobody reads; four is a bar you can count.
            if downbeat || here {
                let galley = ui.painter().layout_no_wrap(
                    format!("{}", i + 1),
                    egui::FontId::monospace(theme::FS_MICRO),
                    ink,
                );
                ui.painter().galley(
                    egui::pos2(rect.center().x - galley.size().x * 0.5, rect.min.y),
                    galley,
                    ink,
                );
            }
        }
    });
    ui.add_space(theme::S_XS);
}

/// One limb's row: what it is, when it gets hit, and whether it sounds.
fn track_row(
    ui: &mut egui::Ui,
    seq: &animus_runtime::Sequencer,
    index: usize,
    actions: &mut Vec<StepAction>,
) {
    let Some(track) = seq.tracks.get(index) else {
        return;
    };
    let selected = seq.selected == index;
    let audible = seq.audible(track);
    let ink = egui::Color32::from_rgb(track.ink[0], track.ink[1], track.ink[2]);
    let fired = seq.fired.get(index).copied().unwrap_or(0.0);
    let (each, gap) = cell_metrics(ui, seq.len);

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = gap;

        // ── the name column ──
        let (name_rect, name) =
            ui.allocate_exact_size(egui::vec2(NAME_W, 26.0), egui::Sense::click());
        if name.clicked() {
            actions.push(StepAction::SelectTrack(index));
        }
        let p = ui.painter();
        if selected {
            p.rect_filled(name_rect, theme::R_CHIP as f32, theme::WELL_HOVER);
        } else if name.hovered() {
            p.rect_filled(name_rect, theme::R_CHIP as f32, theme::WELL);
        }
        // The colour bar is the row's identity; the disc beside it flashes
        // when the track fires, which is how an operator finds the limb a
        // sound belongs to without reading anything.
        let bar = egui::Rect::from_min_size(
            egui::pos2(name_rect.min.x + theme::S_2XS, name_rect.min.y),
            egui::vec2(3.0, 26.0),
        );
        p.rect_filled(bar, 1.5, if audible { ink } else { theme::GHOST });
        p.circle_filled(
            egui::pos2(bar.max.x + theme::S_SM + 4.5, name_rect.center().y),
            4.5,
            ink.gamma_multiply(0.25 + 0.75 * fired),
        );

        let text_x = bar.max.x + theme::S_SM + 14.0;
        let title_ink = if audible { theme::INK } else { theme::FAINT };
        let title = p.layout_no_wrap(
            track.name.clone(),
            egui::FontId::proportional(theme::FS_CONTROL),
            title_ink,
        );
        p.galley(
            egui::pos2(text_x, name_rect.center().y - title.size().y - 1.0),
            title,
            title_ink,
        );
        let meta = p.layout_no_wrap(
            format!("{} hits", track.hits(seq.len)),
            egui::FontId::monospace(theme::FS_MICRO),
            theme::HINT,
        );
        p.galley(
            egui::pos2(text_x, name_rect.center().y + 1.0),
            meta,
            theme::HINT,
        );
        name.context_menu(|ui| {
            if ui.button("Clear this track").clicked() {
                actions.push(StepAction::ClearTrack(index));
                ui.close();
            }
            ui.separator();
            if ui
                .button(egui::RichText::new("Remove track").color(theme::LIVE_CORAL))
                .clicked()
            {
                actions.push(StepAction::RemoveTrack(index));
                ui.close();
            }
        });

        // ── the cells ──
        for i in 0..seq.len {
            let velocity = track.steps.get(i).copied().unwrap_or(0.0);
            let cell = crate::widgets::step_cell(
                ui,
                each,
                velocity,
                ink,
                audible,
                seq.running && i == seq.current(),
                i % 4 == 0,
            );
            if cell.clicked() {
                actions.push(StepAction::Cycle(index, i));
            }
            if cell.secondary_clicked() {
                actions.push(StepAction::ClearCell(index, i));
            }
            cell.on_hover_text(if velocity >= animus_runtime::FULL {
                format!("Full hit · step {}", i + 1)
            } else if velocity > 0.0 {
                format!("Ghost hit · step {}", i + 1)
            } else {
                format!("Empty · step {}", i + 1)
            });
        }

        // ── mute and solo ──
        for (label, on, mark, action) in [
            (
                "M",
                track.mute,
                theme::LIVE_CORAL,
                StepAction::ToggleMute(index),
            ),
            (
                "S",
                track.solo,
                theme::CAUTION_AMBER,
                StepAction::ToggleSolo(index),
            ),
        ] {
            let (rect, response) =
                ui.allocate_exact_size(egui::vec2(22.0, 22.0), egui::Sense::click());
            if response.clicked() {
                actions.push(action);
            }
            let p = ui.painter();
            p.rect_filled(
                rect,
                theme::R_BADGE as f32,
                if on { theme::WELL_HOVER } else { theme::WELL },
            );
            let ink = if on { mark } else { theme::FAINT };
            let galley = p.layout_no_wrap(
                label.to_string(),
                egui::FontId::monospace(theme::FS_TINY),
                ink,
            );
            p.galley(rect.center() - galley.size() * 0.5, galley, ink);
            response.on_hover_text(if label == "M" {
                "Mute this track"
            } else {
                "Solo this track — every other row goes quiet"
            });
        }
    });
    ui.add_space(2.0);
}

/// Transport, record, tempo and grid, in one row.
fn transport_row(
    ui: &mut egui::Ui,
    seq: &animus_runtime::Sequencer,
    mode: EditMode,
    puppet_name: &str,
    collapsed: bool,
    actions: &mut Vec<StepAction>,
) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = theme::S_XS;

        ui.label(
            egui::RichText::new("SEQUENCER")
                .size(theme::FS_LABEL)
                .color(theme::FAINT)
                .strong(),
        );
        ui.label(
            egui::RichText::new(puppet_name)
                .monospace()
                .size(theme::FS_TINY)
                .color(theme::DIM),
        );
        ui.add_space(theme::S_SM);

        if icons::button(ui, icons::Icon::Stop, false, Some(theme::SUB))
            .on_hover_text("Stop and return to step 1")
            .clicked()
        {
            actions.push(StepAction::Stop);
        }
        let running = seq.running;
        if icons::button(
            ui,
            if running {
                icons::Icon::Stop
            } else {
                icons::Icon::Play
            },
            running,
            Some(if running { theme::GO_GREEN } else { theme::SUB }),
        )
        .on_hover_text("Run or pause the pattern")
        .clicked()
        {
            actions.push(StepAction::SetRunning(!running));
        }

        record_button(ui, seq, mode, actions);

        // **TAP plays whether or not it records.** Finding the sound comes
        // before committing it, so the button is useful with record off and
        // writes under the playhead when it is on.
        let empty = seq.tracks.is_empty();
        let tap = ui.add(
            egui::Button::new(
                egui::RichText::new("TAP")
                    .monospace()
                    .size(theme::FS_TINY)
                    .color(if empty { theme::DISABLED } else { theme::INK }),
            )
            .fill(theme::WELL)
            .corner_radius(theme::R_BADGE),
        );
        if tap.is_pointer_button_down_on() && !empty {
            actions.push(StepAction::Tap);
        }
        tap.on_hover_text("Hold to hit the selected track. With record armed it writes the step.");

        ui.add_space(theme::S_SM);
        separator(ui);
        ui.add_space(theme::S_SM);

        ui.label(
            egui::RichText::new("GRID")
                .size(theme::FS_MICRO)
                .color(theme::FAINT),
        );
        for q in animus_runtime::Quantize::ALL {
            let on = q == seq.quantize;
            if crate::widgets::chip(
                ui,
                q.label(),
                on,
                if on { theme::BRIGHT } else { theme::DIM },
            )
            .clicked()
            {
                actions.push(StepAction::SetQuantize(q));
            }
        }

        ui.add_space(theme::S_SM);
        ui.label(
            egui::RichText::new("STEPS")
                .size(theme::FS_MICRO)
                .color(theme::FAINT),
        );
        for n in animus_runtime::STEP_COUNTS {
            let on = n == seq.len;
            if crate::widgets::chip(
                ui,
                &n.to_string(),
                on,
                if on { theme::BRIGHT } else { theme::DIM },
            )
            .on_hover_text("Steps beyond this length are kept and come back when it grows.")
            .clicked()
            {
                actions.push(StepAction::SetLength(n));
            }
        }

        ui.add_space(theme::S_SM);
        let mut bpm = seq.bpm;
        if ui
            .add(
                egui::DragValue::new(&mut bpm)
                    .speed(0.5)
                    .range(40.0..=200.0)
                    .suffix(" BPM"),
            )
            .changed()
        {
            actions.push(StepAction::SetBpm(bpm));
        }
        beat_dots(ui, seq);

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(theme::S_SM);
            if ui
                .add(
                    egui::Button::new(
                        egui::RichText::new(if collapsed { "Expand" } else { "Collapse" })
                            .size(theme::FS_SM)
                            .color(theme::SUB),
                    )
                    .fill(theme::WELL)
                    .corner_radius(theme::R_BADGE),
                )
                .on_hover_text("Fold the grid away for more rigging room")
                .clicked()
            {
                actions.push(StepAction::ToggleCollapsed);
            }
            if !collapsed {
                if ui
                    .add(
                        egui::Button::new(
                            egui::RichText::new("Clear pattern")
                                .size(theme::FS_SM)
                                .color(theme::SUB),
                        )
                        .fill(theme::WELL)
                        .corner_radius(theme::R_BADGE),
                    )
                    .clicked()
                {
                    actions.push(StepAction::ClearAll);
                }
                if icons::button(ui, icons::Icon::Plus, false, Some(theme::SUB))
                    .on_hover_text("Give the selected joint a track of its own")
                    .clicked()
                {
                    actions.push(StepAction::AddTrack);
                }
            }
        });
    });
}

/// Arm live recording. Coral, because armed is a state the audience can end
/// up in the middle of.
fn record_button(
    ui: &mut egui::Ui,
    seq: &animus_runtime::Sequencer,
    mode: EditMode,
    actions: &mut Vec<StepAction>,
) {
    let armed = seq.armed;
    let allowed = mode != EditMode::Rig;
    let response = ui.add_enabled(
        allowed,
        egui::Button::new(
            egui::RichText::new(if armed { "REC" } else { "Arm record" })
                .size(theme::FS_SM)
                .color(if armed { theme::BRIGHT } else { theme::SUB }),
        )
        .fill(if armed {
            theme::STOP_SURFACE
        } else {
            theme::WELL
        })
        .stroke(if armed {
            egui::Stroke::new(1.0_f32, theme::STOP_BORDER)
        } else {
            egui::Stroke::NONE
        })
        .corner_radius(theme::R_BADGE),
    );
    let response = if allowed {
        response.on_hover_text(
            "Arm live input. Hold TAP to write hits into the selected track under the playhead.",
        )
    } else {
        response.on_disabled_hover_text("Recording belongs to EDIT and PERFORM, not to RIG.")
    };
    if response.clicked() {
        actions.push(StepAction::SetArmed(!armed));
    }
}

/// Four dots that count the bar, so the tempo is visible without a number.
fn beat_dots(ui: &mut egui::Ui, seq: &animus_runtime::Sequencer) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(44.0, 10.0), egui::Sense::hover());
    let p = ui.painter();
    let per_beat = seq.quantize.division().max(1.0) as usize;
    let beat = (seq.current() / per_beat) % 4;
    for i in 0..4 {
        let at = egui::pos2(rect.min.x + 5.0 + i as f32 * 11.0, rect.center().y);
        let lit = seq.running && i == beat;
        p.circle_filled(
            at,
            if lit { 3.5 } else { 2.5 },
            if lit { theme::GO_GREEN } else { theme::GHOST },
        );
    }
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
    signal: Option<&animus_runtime::SignalBusRes>,
    signal_status: Option<&animus_runtime::SignalStatus>,
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
                seq.tracks.iter().filter(|t| t.hits(seq.len) > 0).count(),
                seq.armed,
                signal_status,
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
        signal,
        signal_status,
        inspector_edits: Vec::new(),
        signal_edits: Vec::new(),
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
            // **The panel decides the width, not its contents.** A scroll
            // area lays out against an unbounded width unless it is told
            // otherwise, so one long sentence would report itself as a
            // requirement and hold the sidebar open against the operator's
            // drag. Clamping here makes every child wrap to the panel
            // instead of the panel stretching to the child.
            let inner = ui.available_width();
            egui::ScrollArea::vertical()
                .id_salt("left_scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.set_max_width(inner);
                    match viewer.state.left_tab {
                        LeftTab::Scene => viewer.scene(ui),
                        LeftTab::Assets => viewer.assets(ui),
                        LeftTab::Tools => viewer.tools_body(ui),
                    }
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
            let inner = ui.available_width();
            egui::ScrollArea::vertical()
                .id_salt("right_scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.set_max_width(inner);
                    match viewer.state.right_tab {
                        RightTab::Inspect => viewer.inspector(ui),
                        RightTab::Physics => viewer.solver(ui),
                        RightTab::Channels => viewer.channels(ui),
                        RightTab::Bind => viewer.bindings(ui),
                    }
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
    out.signal_edits = std::mem::take(&mut viewer.signal_edits);
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
