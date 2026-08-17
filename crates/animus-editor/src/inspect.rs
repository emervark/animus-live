//! The artist-facing inspector: hand-written rows that emit commands.
//!
//! Spec §10.4's decisive reason, restated because it shapes every line
//! here: **a reflection-driven inspector mutates in place and cannot be
//! undone.** Every widget in this module returns `Option<InspectorEdit>`
//! instead of touching the document, and the one writer system applies it.
//! `fn inspect_bone(ui, &Bone) -> Option<...>` costs ~30 lines and buys
//! Ctrl+Z on every slider.
//!
//! ## Gestures and merging
//!
//! A slider drag emits an edit per frame. The command layer merges
//! like-target commands until [`UndoStack::break_merge`] is called, so this
//! module also reports **when the gesture ended** (`released`), and the
//! writer seals the entry then. Without that, a drag would still collapse —
//! but so would two *separate* drags of the same slider, which is wrong.
//!
//! ## The ◎ affordance
//!
//! Every bindable row shows a Learn button. In M1 it is inert; it exists
//! now because spec §10.4 calls it what makes the signal bus usable, and
//! reserving the space means the layout does not shift under people's
//! hands when M2 arrives.

use animus_core::doc::{
    BoneParam, LayerScalar, Project, PuppetKind, RenameLayer, SetBoneParam, SetJointPinned,
    SetLayerScalar,
};
use bevy_egui::egui;

use crate::state::{EditorState, Selection};
use crate::theme;

/// A proposed edit plus whether the gesture that produced it has ended.
///
/// `command` is `None` on a pure release — a click on a slider without
/// moving it ends a gesture but changes nothing, and pushing a no-op
/// command for it would put an empty entry on the undo stack.
#[derive(Debug, Clone, PartialEq)]
pub struct InspectorEdit {
    pub command: Option<InspectorCommand>,
    /// True when the widget was released this frame: the undo entry seals.
    pub released: bool,
}

/// The concrete commands the inspector can emit. A closed set on purpose:
/// each variant is constructed with its `from` read from the document this
/// frame, which is what makes the inverse correct.
#[derive(Debug, Clone, PartialEq)]
pub enum InspectorCommand {
    BoneParam(SetBoneParam),
    JointPinned(SetJointPinned),
    LayerScalar(SetLayerScalar),
    LayerName(RenameLayer),
}

/// A labelled slider row: label left, mono value right, slider below,
/// inert ◎ on bindable rows. Returns the new value while dragging and
/// whether the drag released.
fn labelled_slider(
    ui: &mut egui::Ui,
    label: &str,
    value: f32,
    range: std::ops::RangeInclusive<f32>,
    bindable: bool,
) -> (Option<f32>, bool) {
    let mut current = value;
    let mut released = false;
    let mut changed = None;

    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(label).size(12.0).color(theme::SUB));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if bindable {
                // Inert until M2. A disabled small button, not a missing one.
                ui.add_enabled(
                    false,
                    egui::Button::new(egui::RichText::new("◎").size(10.0))
                        .fill(egui::Color32::TRANSPARENT)
                        .corner_radius(theme::R_BADGE),
                )
                .on_disabled_hover_text("Bind to a live channel (arrives in M2)");
            }
            ui.label(
                egui::RichText::new(format!("{current:.3}"))
                    .monospace()
                    .size(10.0)
                    .color(theme::MID),
            );
        });
    });

    let response = ui.add(
        egui::Slider::new(&mut current, range)
            .show_value(false)
            .trailing_fill(true),
    );
    if response.changed() {
        changed = Some(current);
    }
    if response.drag_stopped() {
        released = true;
    }

    (changed, released)
}

/// Draw the inspector for the current selection. Read-only against the
/// document; every change comes back as an edit.
pub fn inspector_ui(ui: &mut egui::Ui, doc: &Project, state: &EditorState) -> Vec<InspectorEdit> {
    let mut edits = Vec::new();

    match state.selection {
        Selection::None => {
            ui.label(
                egui::RichText::new("Nothing selected.")
                    .size(12.0)
                    .color(theme::DIM),
            );
            ui.label(
                egui::RichText::new("Click a joint or a layer.")
                    .size(11.0)
                    .color(theme::FAINT),
            );
        }

        Selection::Layer(id) => {
            let Some(layer) = doc.layer_data.get(&id) else {
                return edits;
            };
            section(ui, "layer");

            // Name: text edits commit on focus loss or Enter, not per
            // keystroke — a rename is one undo step by construction.
            let mut name = layer.name.clone();
            let response = ui.add(
                egui::TextEdit::singleline(&mut name)
                    .font(egui::TextStyle::Body)
                    .desired_width(f32::INFINITY),
            );
            if response.lost_focus() && name != layer.name && !name.trim().is_empty() {
                edits.push(InspectorEdit {
                    command: Some(InspectorCommand::LayerName(RenameLayer {
                        layer: id,
                        from: layer.name.clone(),
                        to: name,
                    })),
                    released: true,
                });
            }

            let (changed, released) =
                labelled_slider(ui, "opacity", layer.opacity, 0.0..=1.0, true);
            push_layer_scalar(&mut edits, doc, id, LayerScalar::Opacity, changed, released);

            let (changed, released) =
                labelled_slider(ui, "depth", layer.depth, -10.0..=10.0, false);
            push_layer_scalar(&mut edits, doc, id, LayerScalar::Depth, changed, released);
        }

        Selection::Joint(pid, jid) => {
            let Some(joint) = joint_of(doc, pid, jid) else {
                return edits;
            };
            section(ui, "joint");
            row(ui, "name", &joint.name);
            row(
                ui,
                "rest",
                format!("{:.1}, {:.1}", joint.rest.x, joint.rest.y),
            );

            let mut pinned = joint.pinned;
            if ui
                .checkbox(&mut pinned, egui::RichText::new("pinned").size(12.0))
                .changed()
            {
                edits.push(InspectorEdit {
                    command: Some(InspectorCommand::JointPinned(SetJointPinned {
                        puppet: pid,
                        joint: jid,
                        from: joint.pinned,
                        to: pinned,
                    })),
                    released: true,
                });
            }
            ui.label(
                egui::RichText::new("A pinned joint anchors the rig: the solver never moves it.")
                    .size(11.0)
                    .color(theme::FAINT),
            );
        }

        Selection::Bone(pid, bid) => {
            let Some(bone) = bone_of(doc, pid, bid) else {
                return edits;
            };
            section(ui, "bone");
            row(ui, "name", &bone.name);
            row(ui, "joints", format!("{} → {}", bone.a.0, bone.b.0));

            for (label, value, range, param, bindable) in [
                (
                    "stiffness",
                    bone.stiffness,
                    0.0..=1.0,
                    BoneParam::Stiffness,
                    true,
                ),
                ("damping", bone.damping, 0.0..=1.0, BoneParam::Damping, true),
                (
                    "length ×",
                    bone.length_mul,
                    0.25..=4.0,
                    BoneParam::LengthMul,
                    true,
                ),
                (
                    "attach radius",
                    bone.attach_radius,
                    0.0..=400.0,
                    BoneParam::AttachRadius,
                    false,
                ),
            ] {
                let (changed, released) = labelled_slider(ui, label, value, range, bindable);
                if changed.is_some() || released {
                    edits.push(InspectorEdit {
                        command: changed.map(|to| {
                            InspectorCommand::BoneParam(SetBoneParam {
                                puppet: pid,
                                bone: bid,
                                param,
                                from: value,
                                to,
                            })
                        }),
                        released,
                    });
                }
            }
        }

        Selection::Puppet(pid) => {
            let Some(puppet) = doc.puppets.get(&pid) else {
                return edits;
            };
            section(ui, "puppet");
            row(ui, "name", &puppet.name);
            if let PuppetKind::Mesh(m) = &puppet.kind {
                row(ui, "vertices", m.mesh.positions.len().to_string());
                row(ui, "triangles", (m.mesh.triangles.len() / 3).to_string());
                row(ui, "joints", m.skeleton.joints.len().to_string());
                row(ui, "bones", m.skeleton.bones.len().to_string());
            }
        }
    }

    edits
}

fn push_layer_scalar(
    edits: &mut Vec<InspectorEdit>,
    doc: &Project,
    id: animus_core::ids::LayerId,
    which: LayerScalar,
    changed: Option<f32>,
    released: bool,
) {
    if changed.is_none() && !released {
        return;
    }
    let Some(layer) = doc.layer_data.get(&id) else {
        return;
    };
    let from = match which {
        LayerScalar::Opacity => layer.opacity,
        LayerScalar::Depth => layer.depth,
    };
    edits.push(InspectorEdit {
        command: changed.map(|to| {
            InspectorCommand::LayerScalar(SetLayerScalar {
                layer: id,
                which,
                from,
                to,
            })
        }),
        released,
    });
}

fn joint_of(
    doc: &Project,
    pid: animus_core::ids::PuppetId,
    jid: animus_core::ids::JointId,
) -> Option<&animus_core::doc::Joint> {
    match &doc.puppets.get(&pid)?.kind {
        PuppetKind::Mesh(m) => m.skeleton.joints.get(&jid),
        _ => None,
    }
}

fn bone_of(
    doc: &Project,
    pid: animus_core::ids::PuppetId,
    bid: animus_core::ids::BoneId,
) -> Option<&animus_core::doc::Bone> {
    match &doc.puppets.get(&pid)?.kind {
        PuppetKind::Mesh(m) => m.skeleton.bones.get(&bid),
        _ => None,
    }
}

fn section(ui: &mut egui::Ui, title: &str) {
    ui.label(
        egui::RichText::new(title.to_uppercase())
            .size(10.5)
            .color(theme::FAINT)
            .strong(),
    );
    ui.add_space(theme::S_XS);
}

fn row(ui: &mut egui::Ui, name: &str, value: impl Into<String>) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(name).size(12.0).color(theme::SUB));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                egui::RichText::new(value)
                    .monospace()
                    .size(10.0)
                    .color(theme::MID),
            );
        });
    });
}
