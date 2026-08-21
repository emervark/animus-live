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
use animus_core::ids::{BoneId, JointId, PuppetId};
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
    /// A solver setting for the whole show: gravity, damping, iterations.
    SolverParam(animus_core::doc::SetSolverParam),
    /// Delete the selected joint, cascading into its bones.
    DeleteJoint(PuppetId, JointId),
    /// Delete the selected bone, leaving its joints.
    DeleteBone(PuppetId, BoneId),
    /// How readily the joint is thrown about.
    JointMass(animus_core::doc::SetJointMass),
    /// Turn the joint, carrying everything that hangs off it.
    JointRotation(animus_core::doc::RotateJoint),
}

/// A destructive action, spelled as one.
///
/// Coral is the Signal Rule's live colour and this is not live, so the
/// button carries its weight in the word rather than the fill: an operator
/// should read what will go before they can click it, and the hover text
/// says what it takes with it.
fn danger(ui: &mut egui::Ui, label: &str, hint: &str) -> bool {
    ui.add(
        egui::Button::new(
            egui::RichText::new(label)
                .size(theme::FS_CONTROL)
                .color(theme::INK),
        )
        .fill(theme::WELL)
        .corner_radius(theme::R_BUTTON)
        .min_size(egui::vec2(ui.available_width(), 0.0)),
    )
    .on_hover_text(hint)
    .clicked()
}

/// A labelled slider row: label left, mono value right, slider below,
/// inert ◎ on bindable rows. Returns the new value while dragging and
/// whether the drag released.
pub(crate) fn labelled_slider(
    ui: &mut egui::Ui,
    label: &str,
    value: f32,
    range: std::ops::RangeInclusive<f32>,
    bindable: bool,
    default: Option<f32>,
) -> (Option<f32>, bool) {
    let mut current = value;

    // The comp's control, not egui's restyled. See `widgets::SliderRow`: a
    // 3px rail, a filled 12px handle, and the value in its own field.
    let mut row = crate::widgets::SliderRow::new(label, &mut current, range);
    if let Some(d) = default {
        row = row.default_value(d);
    }
    let response = row.show(ui);

    if bindable {
        // Inert until M2. A disabled control, not a missing one, so the
        // operator learns where bindings will live before they exist.
        ui.add_enabled(
            false,
            egui::Button::new(
                egui::RichText::new("bind")
                    .size(theme::FS_TINY)
                    .color(theme::FAINT),
            )
            .fill(egui::Color32::TRANSPARENT)
            .corner_radius(theme::R_BADGE),
        )
        .on_disabled_hover_text("Bind to a live channel (arrives in M2)");
    }

    let changed = response.changed().then_some(current);
    // A custom control has no drag-stop of its own to report, and the writer
    // only uses `released` to break the undo merge. Losing the pointer is the
    // same moment for that purpose.
    let released = response.changed() && !ui.input(|i| i.pointer.any_down());

    (changed, released)
}

/// Mass is shown in kilograms; the solver stores its inverse.
const MIN_MASS: f32 = 0.2;
const MAX_MASS: f32 = 4.0;

/// What one pass over the inspector produced.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct InspectorOut {
    pub edits: Vec<InspectorEdit>,
    /// The LIVE section's reset button. Not a command: the pull it clears
    /// was never written to the document, so there is nothing to undo.
    pub wants_reset_pose: bool,
    /// A new angle for the selected joint, in radians, set in a pose mode.
    /// Session state rather than a document edit — see [`crate::rotate`].
    pub set_live_rotation: Option<f32>,
}

/// Draw the inspector for the current selection. Read-only against the
/// document; every change comes back as an edit.
pub fn inspector_ui(ui: &mut egui::Ui, doc: &Project, state: &EditorState) -> InspectorOut {
    let mut out = InspectorOut::default();
    let mut wants_reset = false;

    match state.selection {
        Selection::None => {
            crate::widgets::note(ui, "Nothing selected.");
            crate::widgets::note(ui, "Click a joint or a layer.");
        }

        Selection::Layer(id) => {
            let Some(layer) = doc.layer_data.get(&id) else {
                out.wants_reset_pose = wants_reset;
                return out;
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
                out.edits.push(InspectorEdit {
                    command: Some(InspectorCommand::LayerName(RenameLayer {
                        layer: id,
                        from: layer.name.clone(),
                        to: name,
                    })),
                    released: true,
                });
            }

            let (changed, released) =
                labelled_slider(ui, "opacity", layer.opacity, 0.0..=1.0, true, Some(1.0));
            push_layer_scalar(
                &mut out.edits,
                doc,
                id,
                LayerScalar::Opacity,
                changed,
                released,
            );

            let (changed, released) =
                labelled_slider(ui, "depth", layer.depth, -10.0..=10.0, false, None);
            push_layer_scalar(
                &mut out.edits,
                doc,
                id,
                LayerScalar::Depth,
                changed,
                released,
            );
        }

        Selection::Joint(pid, jid) => {
            let Some(joint) = joint_of(doc, pid, jid) else {
                out.wants_reset_pose = wants_reset;
                return out;
            };
            let puppet_name = doc
                .puppets
                .get(&pid)
                .map(|p| p.name.as_str())
                .unwrap_or("Puppet");
            crate::widgets::breadcrumb(ui, &[puppet_name, "Rig", &joint.name]);
            ui.add_space(theme::S_MD);

            crate::widgets::section_label(ui, "joint");
            ui.add_space(theme::S_SM);
            crate::widgets::field_row(ui, "Name", &joint.name);
            crate::widgets::vec_row(ui, "Position", joint.rest.x, joint.rest.y);

            let mut pinned = joint.pinned;
            let caption = if pinned {
                "Pinned to the stage"
            } else {
                "Free — follows physics"
            };
            if crate::widgets::toggle_row(ui, "Pin", &mut pinned, caption).changed() {
                out.edits.push(InspectorEdit {
                    command: Some(InspectorCommand::JointPinned(SetJointPinned {
                        puppet: pid,
                        joint: jid,
                        from: joint.pinned,
                        to: pinned,
                    })),
                    released: true,
                });
            }

            crate::widgets::divider(ui);
            rotation_section(ui, &mut out, doc, state, pid, jid, joint);

            crate::widgets::divider(ui);
            crate::widgets::section_label(ui, "behaviour");
            ui.add_space(theme::S_SM);
            // Mass, shown as mass rather than as the inverse the solver
            // stores. A pinned joint is immovable whatever it weighs, so the
            // control says so instead of pretending to do something.
            let mass = if joint.inv_mass > 0.0 {
                1.0 / joint.inv_mass
            } else {
                MAX_MASS
            };
            let mut shown = mass;
            let response = crate::widgets::SliderRow::new("Mass", &mut shown, MIN_MASS..=MAX_MASS)
                .suffix(" kg")
                .decimals(2)
                .default_value(1.0)
                .show(ui);
            if response.changed() {
                out.edits.push(InspectorEdit {
                    command: Some(InspectorCommand::JointMass(
                        animus_core::doc::SetJointMass {
                            puppet: pid,
                            joint: jid,
                            from: joint.inv_mass,
                            to: 1.0 / shown.max(MIN_MASS),
                        },
                    )),
                    released: !ui.input(|i| i.pointer.any_down()),
                });
            }
            if joint.pinned {
                crate::widgets::note(ui, "Pinned: the solver never moves this joint.");
            }

            crate::widgets::divider(ui);
            crate::widgets::section_label(ui, "live");
            ui.add_space(theme::S_SM);
            let (offset, ink) = match state.live_offset {
                Some(d) => (
                    format!("{:+.1}, {:+.1}", d.x, d.y),
                    if d.length() > 0.5 {
                        theme::LIVE_CORAL
                    } else {
                        theme::DIM
                    },
                ),
                None => ("—".to_string(), theme::DIM),
            };
            crate::widgets::readout_row(ui, "Pull offset from rest", &offset, ink);
            ui.add_space(theme::S_XS);
            if crate::widgets::wide_button(ui, "Reset to rest position").clicked() {
                // A solver act, not a document edit — there is nothing here
                // for undo to have caught, so it travels on its own flag
                // rather than as a command.
                wants_reset = true;
            }
            crate::widgets::note(
                ui,
                "Resetting clears the live pull only. The rest position saved in RIG is untouched.",
            );

            ui.add_space(theme::S_MD);
            if danger(
                ui,
                "Delete joint",
                "Delete — also removes every bone that uses it.",
            ) {
                out.edits.push(InspectorEdit {
                    command: Some(InspectorCommand::DeleteJoint(pid, jid)),
                    released: true,
                });
            }
        }

        Selection::Bone(pid, bid) => {
            let Some(bone) = bone_of(doc, pid, bid) else {
                out.wants_reset_pose = wants_reset;
                return out;
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
                // Bone parameters are per-bone by design, so there is no
                // single default to measure them against — a "modified" tag
                // here would mark every deliberately-tuned limb as suspect.
                let (changed, released) = labelled_slider(ui, label, value, range, bindable, None);
                if changed.is_some() || released {
                    out.edits.push(InspectorEdit {
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

            ui.add_space(theme::S_SM);
            if danger(ui, "Delete bone", "Delete — the joints at both ends stay.") {
                out.edits.push(InspectorEdit {
                    command: Some(InspectorCommand::DeleteBone(pid, bid)),
                    released: true,
                });
            }
        }

        Selection::Puppet(pid) => {
            let Some(puppet) = doc.puppets.get(&pid) else {
                out.wants_reset_pose = wants_reset;
                return out;
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

    out.wants_reset_pose = wants_reset;
    out
}

/// The ROTATION section: one dial, and a sentence naming what it will move.
///
/// **The sentence is the point.** A rig stores no hierarchy — the parent
/// relation is derived from the bone graph — so which joints a rotation
/// carries is a consequence of how the rigger connected things, and it is
/// not visible from the drawing. Naming them before the operator drags turns
/// "why did the torso move?" into something they can see and fix.
/// **The same dial means two different things either side of RIG**, and the
/// panel says which. In RIG it moves the rest positions — re-placing the
/// skeleton over a drawing that does not bend. In EDIT and PERFORM it poses
/// the puppet, and the drawing follows. Blender draws the same line between
/// edit mode and pose mode; an operator who knows one will guess the other.
fn rotation_section(
    ui: &mut egui::Ui,
    out: &mut InspectorOut,
    doc: &Project,
    state: &EditorState,
    pid: PuppetId,
    jid: JointId,
    joint: &animus_core::doc::Joint,
) {
    let rigging = state.mode == crate::state::EditMode::Rig;
    crate::widgets::section_label(ui, "rotation");
    ui.add_space(theme::S_SM);

    let current = if rigging {
        joint.rest_angle
    } else {
        state.live_rotation
    };
    let mut degrees = current.to_degrees();
    let response = crate::widgets::SliderRow::new("Angle", &mut degrees, -180.0..=180.0)
        .suffix("°")
        .decimals(0)
        .default_value(0.0)
        .show(ui);

    if response.changed() {
        if rigging {
            out.edits.push(InspectorEdit {
                command: Some(InspectorCommand::JointRotation(
                    animus_core::doc::RotateJoint {
                        puppet: pid,
                        joint: jid,
                        from: joint.rest_angle,
                        to: degrees.to_radians(),
                    },
                )),
                released: !ui.input(|i| i.pointer.any_down()),
            });
        } else {
            // A pose, not an edit: nothing reaches the document, so nothing
            // reaches the undo stack either.
            out.set_live_rotation = Some(degrees.to_radians());
        }
    }

    crate::widgets::note(ui, &carries_text(doc, pid, jid));
    crate::widgets::note(
        ui,
        if rigging {
            "RIG moves the skeleton over the artwork; the drawing does not bend."
        } else {
            "Poses the limb away from rest, so the drawing bends with it."
        },
    );
}

/// "rotates head, shoulder.R, shoulder.L +4 below", or that it moves nothing.
///
/// Three names then a count: enough to recognise the limb, short enough to
/// stay one line in a narrow panel.
fn carries_text(doc: &Project, pid: PuppetId, jid: JointId) -> String {
    let Some(PuppetKind::Mesh(m)) = doc.puppets.get(&pid).map(|p| &p.kind) else {
        return String::new();
    };
    let below = animus_core::skeleton::rig_tree(&m.skeleton).descendants(jid);
    if below.is_empty() {
        return "Nothing hangs off this joint, so rotating it moves nothing.".into();
    }

    const SHOWN: usize = 3;
    let names: Vec<&str> = below
        .iter()
        .take(SHOWN)
        .filter_map(|id| m.skeleton.joints.get(id).map(|j| j.name.as_str()))
        .collect();
    let rest = below.len().saturating_sub(names.len());
    if rest == 0 {
        format!("Rotates {}.", names.join(", "))
    } else {
        format!("Rotates {} +{rest} below.", names.join(", "))
    }
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
            .size(theme::FS_LABEL)
            .color(theme::FAINT)
            .strong(),
    );
    ui.add_space(theme::S_XS);
}

fn row(ui: &mut egui::Ui, name: &str, value: impl Into<String>) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(name)
                .size(theme::FS_CONTROL)
                .color(theme::SUB),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                egui::RichText::new(value)
                    .monospace()
                    .size(theme::FS_LABEL)
                    .color(theme::MID),
            );
        });
    });
}
