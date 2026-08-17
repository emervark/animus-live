//! The panels.
//!
//! Everything here reads the document and writes nothing. Edits arrive later
//! as `DocCommand`s through a single system (spec §8.6), so a panel that
//! wants to change something returns the intent rather than reaching into
//! `Project`.

use animus_core::doc::Project;
use animus_runtime::DocumentRes;
use bevy_egui::egui;

use crate::import::ImportStatus;
use crate::inspect::{InspectorEdit, inspector_ui};
use crate::state::{EditMode, EditorState, Selection, TabKind, Tool};
use crate::theme;
use crate::viewport::{ViewportInput, ViewportTarget, viewport_widget};

/// Everything the dock produced this frame, for the writer systems.
#[derive(Debug, Default)]
pub struct DockOutput {
    pub viewport_input: Option<ViewportInput>,
    pub inspector_edits: Vec<InspectorEdit>,
    pub layer_move: Option<(animus_core::ids::LayerId, i32)>,
    pub wants_undo: bool,
    pub wants_redo: bool,
}

/// A small tracked label, the system's "label" role.
fn label(ui: &mut egui::Ui, text: &str) {
    ui.label(
        egui::RichText::new(text.to_uppercase())
            .size(10.5)
            .color(theme::FAINT)
            .strong(),
    );
}

/// Engine-produced values are mono and dimmer than the human-written name
/// beside them. The Mono-Means-Machine rule.
fn data(text: impl Into<String>) -> egui::RichText {
    egui::RichText::new(text)
        .monospace()
        .size(10.0)
        .color(theme::DIM)
}

fn row(ui: &mut egui::Ui, name: &str, value: impl Into<String>) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(name).size(12.0).color(theme::SUB));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(data(value));
        });
    });
}

/// A panel that has nothing to show yet says so, and says when it will.
fn stub(ui: &mut egui::Ui, what: &str, when: &str) {
    ui.add_space(theme::S_SM);
    ui.label(egui::RichText::new(what).size(12.0).color(theme::DIM));
    ui.add_space(theme::S_XS);
    ui.label(egui::RichText::new(when).size(11.0).color(theme::FAINT));
}

pub struct TabViewer<'a> {
    pub state: &'a mut EditorState,
    pub doc: &'a Project,
    pub viewport_texture: Option<egui::TextureId>,
    pub target: Option<&'a ViewportTarget>,
    pub import_status: &'a ImportStatus,
    /// Filled by the Inspector tab; the writer system applies them.
    pub inspector_edits: Vec<InspectorEdit>,
    pub wants_undo: bool,
    pub wants_redo: bool,
    /// Layer the operator asked to move, and the direction (+1 toward the
    /// front of the paint order).
    pub layer_move: Option<(animus_core::ids::LayerId, i32)>,
    /// Filled in by the viewport tab, read by the caller afterwards. The
    /// panel cannot touch the camera itself: it runs inside egui's closure,
    /// where the ECS is not available.
    pub viewport_input: Option<ViewportInput>,
}

impl egui_dock::TabViewer for TabViewer<'_> {
    type Tab = TabKind;

    fn title(&mut self, tab: &mut Self::Tab) -> egui::WidgetText {
        tab.title().into()
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Self::Tab) {
        match tab {
            TabKind::Viewport => self.viewport(ui),
            TabKind::Layers => self.layers(ui),
            TabKind::Assets => self.assets(ui),
            TabKind::Tools => self.tools(ui),
            TabKind::Inspector => self.inspector(ui),
            TabKind::Solver => self.solver(ui),
            TabKind::Channels => stub(
                ui,
                "Live channels appear here once a source is connected.",
                "OSC, MIDI and audio arrive in M2.",
            ),
            TabKind::Bindings => stub(
                ui,
                "Bindings map a channel onto a parameter.",
                "Use the ◎ beside any value to start one, from M2.",
            ),
        }
    }

    /// The viewport is the one surface that must not be closed by accident:
    /// without it there is nothing to edit.
    fn closeable(&mut self, tab: &mut Self::Tab) -> bool {
        !matches!(tab, TabKind::Viewport)
    }
}

impl TabViewer<'_> {
    fn viewport(&mut self, ui: &mut egui::Ui) {
        let available = ui.available_size();
        let size = egui::vec2(available.x.max(16.0), available.y.max(16.0));
        let input = viewport_widget(ui, self.viewport_texture, size);
        self.status_strip(ui, input.rect);
        self.viewport_input = Some(input);
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
            egui::FontId::monospace(9.5),
            theme::DIM,
        );
    }

    fn layers(&mut self, ui: &mut egui::Ui) {
        if self.doc.layers.is_empty() {
            stub(ui, "No layers yet.", "Import an image to make one.");
            return;
        }

        // Painted back to front, so the topmost layer reads at the top of the
        // list — the order an artist expects, not the order the Vec is in.
        // The arrows move a layer within the paint order; the writer turns
        // that into a ReorderLayers command plus the depth rewrite that
        // keeps `layer.depth` the single source of truth for world Z.
        let layer_ids: Vec<_> = self.doc.layers.iter().rev().copied().collect();
        for layer_id in layer_ids {
            let Some(layer) = self.doc.layer_data.get(&layer_id) else {
                continue;
            };
            let selected = self.state.selection == Selection::Layer(layer_id);
            ui.horizontal(|ui| {
                if ui
                    .add(
                        egui::Button::new(
                            egui::RichText::new(&layer.name)
                                .size(12.5)
                                .color(if selected { theme::BRIGHT } else { theme::INK }),
                        )
                        .fill(if selected {
                            theme::WELL_HOVER
                        } else {
                            egui::Color32::TRANSPARENT
                        })
                        .corner_radius(theme::R_INPUT),
                    )
                    .clicked()
                {
                    self.state.selection = Selection::Layer(layer_id);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("▼").clicked() {
                        self.layer_move = Some((layer_id, -1));
                    }
                    if ui.small_button("▲").clicked() {
                        self.layer_move = Some((layer_id, 1));
                    }
                    ui.label(data(format!("z {:.2}", layer.depth)));
                });
            });
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
                    ui.label(egui::RichText::new(message).size(11.5).color(colour));
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
                        .size(12.0)
                        .color(theme::INK),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(data(format!("#{}", id.0)));
                });
            });
        }
    }

    fn tools(&mut self, ui: &mut egui::Ui) {
        label(ui, "tool");
        for tool in Tool::ALL {
            let selected = self.state.tool == tool;
            let response = ui.add(
                egui::Button::new(
                    egui::RichText::new(tool.label())
                        .size(12.5)
                        .color(if selected { theme::BRIGHT } else { theme::SOFT }),
                )
                .fill(if selected {
                    theme::WELL_HOVER
                } else {
                    egui::Color32::TRANSPARENT
                })
                .corner_radius(theme::R_INPUT)
                .min_size(egui::vec2(ui.available_width(), 0.0)),
            );
            if response.clicked() {
                self.state.tool = tool;
            }
            ui.horizontal(|ui| {
                ui.add_space(theme::S_SM);
                ui.label(data(tool.key_hint()));
            });
        }

        ui.add_space(theme::S_MD);
        label(ui, "mode");
        ui.horizontal(|ui| {
            for (mode, name) in [(EditMode::Edit, "Edit"), (EditMode::Live, "Live")] {
                let selected = self.state.mode == mode;
                // Live is coral because live means the audience can see it —
                // the same meaning the colour carries everywhere else.
                let colour = match (selected, mode) {
                    (true, EditMode::Live) => theme::LIVE_CORAL,
                    (true, EditMode::Edit) => theme::BRIGHT,
                    _ => theme::DIM,
                };
                if ui
                    .add(
                        egui::Button::new(egui::RichText::new(name).size(12.0).color(colour))
                            .fill(if selected {
                                theme::WELL_HOVER
                            } else {
                                egui::Color32::TRANSPARENT
                            })
                            .corner_radius(theme::R_BUTTON),
                    )
                    .clicked()
                {
                    self.state.mode = mode;
                }
            }
        });
        ui.label(
            egui::RichText::new(match self.state.mode {
                EditMode::Edit => "Dragging moves a joint's rest position.",
                EditMode::Live => "Dragging pulls the puppet. The document is untouched.",
            })
            .size(11.0)
            .color(theme::FAINT),
        );
    }

    fn inspector(&mut self, ui: &mut egui::Ui) {
        let edits = inspector_ui(ui, self.doc, self.state);
        self.inspector_edits.extend(edits);

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
                .size(10.5)
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
                .size(9.5)
                .color(theme::FAINT),
            );
        });
        for label in self.state.undo.labels().take(12) {
            ui.label(egui::RichText::new(label).size(11.5).color(theme::DIM));
        }
    }

    fn solver(&mut self, ui: &mut egui::Ui) {
        let s = &self.doc.solver;
        label(ui, "solver");
        row(ui, "rate", format!("{} Hz", s.hz));
        row(ui, "iterations", s.iterations.to_string());
        row(ui, "damping", format!("{:.3}", s.global_damping));
        row(
            ui,
            "gravity",
            format!("{:.2}, {:.2}", s.gravity.x, s.gravity.y),
        );
        row(ui, "state", if s.enabled { "running" } else { "paused" });
    }
}

/// Draw the whole dock. Called from the egui pass.
pub fn draw(
    ctx: &egui::Context,
    state: &mut EditorState,
    doc: &DocumentRes,
    viewport_texture: Option<egui::TextureId>,
    target: Option<&ViewportTarget>,
    import_status: &ImportStatus,
) -> DockOutput {
    // `CentralPanel::show` is deprecated in egui 0.34 in favour of
    // `show_inside`, which needs a `Ui` — and there is no non-deprecated way
    // to get a root `Ui` from a `Context`. egui's own `show_dyn` carries the
    // same `expect(deprecated)` for the same reason. Revisit when egui offers
    // a top-level replacement.
    #![expect(deprecated)]
    let mut out = DockOutput::default();
    egui::CentralPanel::default()
        .frame(egui::Frame::NONE.fill(theme::APP_BG))
        .show(ctx, |ui| {
            let mut dock = std::mem::replace(&mut state.dock, egui_dock::DockState::new(vec![]));
            let mut viewer = TabViewer {
                state,
                doc: &doc.0,
                viewport_texture,
                target,
                import_status,
                inspector_edits: Vec::new(),
                layer_move: None,
                wants_undo: false,
                wants_redo: false,
                viewport_input: None,
            };
            egui_dock::DockArea::new(&mut dock)
                .style(dock_style(&ui.ctx().global_style()))
                .show_inside(ui, &mut viewer);
            out.viewport_input = viewer.viewport_input;
            out.inspector_edits = std::mem::take(&mut viewer.inspector_edits);
            out.layer_move = viewer.layer_move;
            out.wants_undo = viewer.wants_undo;
            out.wants_redo = viewer.wants_redo;
            state.dock = dock;
        });
    out
}

fn dock_style(base: &egui::Style) -> egui_dock::Style {
    let mut style = egui_dock::Style::from_egui(base);
    style.dock_area_padding = None;
    style.tab_bar.bg_fill = theme::APP_BG;
    style.tab_bar.hline_color = theme::SEAM;
    style.tab.active.bg_fill = theme::WORKSPACE;
    style.tab.active.text_color = theme::BRIGHT;
    style.tab.inactive.bg_fill = theme::APP_BG;
    style.tab.inactive.text_color = theme::DIM;
    style.tab.focused.bg_fill = theme::WORKSPACE;
    style.tab.focused.text_color = theme::BRIGHT;
    style.tab.hovered.text_color = theme::INK;
    style.separator.color_idle = theme::SEAM;
    style.separator.color_hovered = theme::WELL_HOVER;
    style.separator.color_dragged = theme::GO_GREEN;
    style
}
