//! The panels.
//!
//! Everything here reads the document and writes nothing. Edits arrive later
//! as `DocCommand`s through a single system (spec §8.6), so a panel that
//! wants to change something returns the intent rather than reaching into
//! `Project`.

use animus_core::doc::{Project, PuppetKind};
use animus_runtime::DocumentRes;
use bevy_egui::egui;

use crate::state::{EditMode, EditorState, Selection, TabKind, Tool};
use crate::theme;

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
        let rect = ui.available_rect_before_wrap();
        ui.painter().rect_filled(rect, 0.0, theme::SCREEN_BLACK);

        match self.viewport_texture {
            Some(id) => {
                // Task 7 replaces this with the interactive image. Drawing it
                // plainly here keeps the layout honest in the meantime.
                let size = rect.size();
                ui.painter().image(
                    id,
                    rect,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
                let _ = size;
            }
            None => {
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "no viewport target yet",
                    egui::FontId::monospace(11.0),
                    theme::FAINT,
                );
            }
        }
    }

    fn layers(&mut self, ui: &mut egui::Ui) {
        if self.doc.layers.is_empty() {
            stub(ui, "No layers yet.", "Import an image to make one.");
            return;
        }

        // Painted back to front, so the topmost layer reads at the top of the
        // list — the order an artist expects, not the order the Vec is in.
        for layer_id in self.doc.layers.iter().rev() {
            let Some(layer) = self.doc.layer_data.get(layer_id) else {
                continue;
            };
            let selected = self.state.selection == Selection::Layer(*layer_id);
            let response = ui.add(
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
                .corner_radius(theme::R_INPUT)
                .min_size(egui::vec2(ui.available_width(), 0.0)),
            );
            if response.clicked() {
                self.state.selection = Selection::Layer(*layer_id);
            }
            ui.horizontal(|ui| {
                ui.add_space(theme::S_SM);
                ui.label(data(format!("z {:.2}", layer.depth)));
                ui.label(data(format!("{} puppets", layer.contents.len())));
            });
        }
    }

    fn assets(&mut self, ui: &mut egui::Ui) {
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
        match self.state.selection {
            Selection::None => stub(ui, "Nothing selected.", "Pick a layer, joint or bone."),
            Selection::Layer(id) => {
                let Some(layer) = self.doc.layer_data.get(&id) else {
                    return;
                };
                label(ui, "layer");
                row(ui, "name", layer.name.clone());
                row(ui, "depth", format!("{:.3}", layer.depth));
                row(ui, "opacity", format!("{:.2}", layer.opacity));
                row(ui, "puppets", layer.contents.len().to_string());
            }
            Selection::Puppet(id) => {
                let Some(puppet) = self.doc.puppets.get(&id) else {
                    return;
                };
                label(ui, "puppet");
                row(ui, "name", puppet.name.clone());
                if let PuppetKind::Mesh(m) = &puppet.kind {
                    row(ui, "vertices", m.mesh.positions.len().to_string());
                    row(ui, "triangles", (m.mesh.triangles.len() / 3).to_string());
                    row(ui, "joints", m.skeleton.joints.len().to_string());
                    row(ui, "bones", m.skeleton.bones.len().to_string());
                }
            }
            Selection::Joint(pid, jid) => {
                label(ui, "joint");
                row(ui, "id", format!("{}", jid.0));
                row(ui, "puppet", format!("{}", pid.0));
            }
            Selection::Bone(pid, bid) => {
                label(ui, "bone");
                row(ui, "id", format!("{}", bid.0));
                row(ui, "puppet", format!("{}", pid.0));
            }
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
pub fn draw(ctx: &egui::Context, state: &mut EditorState, doc: &DocumentRes) {
    // `CentralPanel::show` is deprecated in egui 0.34 in favour of
    // `show_inside`, which needs a `Ui` — and there is no non-deprecated way
    // to get a root `Ui` from a `Context`. egui's own `show_dyn` carries the
    // same `expect(deprecated)` for the same reason. Revisit when egui offers
    // a top-level replacement.
    #![expect(deprecated)]
    let viewport_texture = None; // Task 7 supplies this
    egui::CentralPanel::default()
        .frame(egui::Frame::NONE.fill(theme::APP_BG))
        .show(ctx, |ui| {
            let mut dock = std::mem::replace(&mut state.dock, egui_dock::DockState::new(vec![]));
            let mut viewer = TabViewer {
                state,
                doc: &doc.0,
                viewport_texture,
            };
            egui_dock::DockArea::new(&mut dock)
                .style(dock_style(&ui.ctx().global_style()))
                .show_inside(ui, &mut viewer);
            state.dock = dock;
        });
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
