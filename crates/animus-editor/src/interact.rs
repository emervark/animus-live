//! The one system that turns viewport gestures into writes.
//!
//! Everything upstream is pure: the viewport reports what happened
//! ([`ViewportInput`]), the drag machine decides what it means
//! ([`DragEffect`]), and this system is the single place effects touch the
//! world — commands into the document through the undo stack, targets into
//! [`JointTargets`]. Spec §8.6 calls for exactly one writer; this is it.

use animus_core::doc::{PuppetKind, apply_command};
use animus_core::ids::PuppetId;
use animus_runtime::{DocumentRes, JointTargets, PendingChangesRes, RenderScale, world_to_img};
use bevy::prelude::*;
use glam::Vec2;

use crate::dock::DockOutput;
use crate::drag::{DragEffect, DragEvent, DragState, step};
use crate::inspect::InspectorCommand;
use crate::rig;
use crate::state::{EditorState, Selection, Tool};
use crate::viewport::{ViewportCamera, ViewportInput, ViewportTarget};

/// The gesture in progress. A resource so it survives frames; a drag is
/// nothing but state across frames.
#[derive(Resource, Debug, Default)]
pub struct ActiveDrag(pub DragState);

/// The viewport input for this frame, published by the UI system for
/// whoever runs after it.
///
/// The UI runs inside `EguiPrimaryContextPass` where the rest of the ECS is
/// not reachable, so the input is carried across the schedule boundary in a
/// resource rather than applied inline.
#[derive(Resource, Debug, Default)]
pub struct FrameViewportInput(pub Option<ViewportInput>);

/// The dock's non-viewport output for this frame: inspector edits, layer
/// moves, undo/redo intents. Same schedule-boundary reason as
/// [`FrameViewportInput`].
#[derive(Resource, Debug, Default)]
pub struct FrameDockOutput(pub Option<DockOutput>);

/// Whether egui has keyboard focus (a text field is being typed into),
/// published from the egui pass. Tool shortcuts must not fire while the
/// operator is renaming a layer that happens to contain a J.
#[derive(Resource, Debug, Default)]
pub struct EguiWantsKeyboard(pub bool);

/// Bone-tool state: the first joint of a pending bone, if one was clicked.
#[derive(Resource, Debug, Default)]
pub struct PendingBone(pub Option<(PuppetId, animus_core::ids::JointId)>);

/// Which puppet gestures act on.
///
/// M1 keeps this simple: the selected puppet if there is one, otherwise the
/// only puppet, otherwise nothing. Multi-puppet hit-testing (top-most wins,
/// click-through) is an M6 concern and this function is where it will live.
fn active_puppet(doc: &DocumentRes, selection: Selection) -> Option<PuppetId> {
    match selection {
        Selection::Puppet(id) | Selection::Joint(id, _) | Selection::Bone(id, _) => Some(id),
        _ => doc.0.puppets.keys().next().copied(),
    }
}

/// World position → image pixels for a specific puppet.
fn world_to_puppet_img(
    doc: &DocumentRes,
    scale: &RenderScale,
    puppet: PuppetId,
    world: Vec2,
) -> Option<Vec2> {
    let p = doc.0.puppets.get(&puppet)?;
    let PuppetKind::Mesh(mp) = &p.kind else {
        return None;
    };
    let pivot = animus_runtime::puppet_pivot(mp);
    Some(world_to_img(world.extend(0.0), pivot, scale.ppu))
}

/// Apply this frame's viewport input to the world.
#[allow(clippy::too_many_arguments)]
pub fn apply_interactions(
    input: Res<FrameViewportInput>,
    mut doc: ResMut<DocumentRes>,
    mut state: ResMut<EditorState>,
    mut drag: ResMut<ActiveDrag>,
    mut targets: ResMut<JointTargets>,
    mut pending: ResMut<PendingChangesRes>,
    mut pending_bone: ResMut<PendingBone>,
    scale: Res<RenderScale>,
    cameras: Query<(&Camera, &GlobalTransform), With<ViewportCamera>>,
    target: Option<Res<ViewportTarget>>,
) {
    let Some(input) = input.0 else { return };
    let Ok((camera, camera_xf)) = cameras.single() else {
        return;
    };
    let Some(puppet) = active_puppet(&doc, state.selection) else {
        return;
    };

    // Image-pixel position of the pointer, via the camera. Every gesture
    // below works in image pixels because that is the space joints live in.
    let to_img = |pixel: Vec2| -> Option<Vec2> {
        let world = crate::viewport::camera::pixel_to_world(camera, camera_xf, pixel)?;
        world_to_puppet_img(&doc, &scale, puppet, world)
    };

    match state.tool {
        // Select is also the drag tool: grabbing a joint and moving it is
        // the whole point of the editor.
        Tool::Select => {
            // Order matters and is deliberate:
            //   click        = a tap: grab + release, which selects and (in
            //                  live mode) gives the springs one tug
            //   drag active  = grab on the first frame, then moves
            //   drag ended   = release
            // egui reports a click only when the press did not turn into a
            // drag, so the arms are mutually exclusive by construction.
            let mut events: Vec<DragEvent> = Vec::new();

            if let Some(pixel) = input.clicked_at
                && let Some(img) = to_img(pixel)
            {
                events.push(DragEvent::Grab(img));
                events.push(DragEvent::Release);
            } else if input.drag_left.is_some() {
                if let Some(pixel) = input.hover_at
                    && let Some(img) = to_img(pixel)
                {
                    if matches!(drag.0, DragState::Idle) {
                        events.push(DragEvent::Grab(img));
                    } else {
                        events.push(DragEvent::MoveTo(img));
                    }
                }
            } else if !matches!(drag.0, DragState::Idle) {
                events.push(DragEvent::Release);
            }

            for event in events {
                let effects = step(&mut drag.0, event, state.mode, &doc.0, puppet);
                for effect in effects {
                    apply_effect(effect, &mut doc, &mut state, &mut targets, &mut pending);
                }
            }
        }

        Tool::Joint => {
            if let Some(pixel) = input.clicked_at
                && let Some(img) = to_img(pixel)
                && let Some(cmd) = rig::place_joint(&mut doc.0, puppet, img)
            {
                match apply_command(&mut doc.0, &mut state.undo, Box::new(cmd)) {
                    Ok(changes) => {
                        pending.extend(changes.0);
                        state.undo.break_merge();
                    }
                    Err(e) => error!("place joint failed: {e}"),
                }
            }
        }

        Tool::Bone => {
            if let Some(pixel) = input.clicked_at
                && let Some(img) = to_img(pixel)
            {
                match (pending_bone.0, rig::joint_at(&doc.0, puppet, img)) {
                    // First click on a joint arms the bone.
                    (None, Some(joint)) => {
                        pending_bone.0 = Some((puppet, joint));
                        state.selection = Selection::Joint(puppet, joint);
                    }
                    // Second click on another joint completes it.
                    (Some((armed_puppet, a)), Some(b)) if armed_puppet == puppet => {
                        pending_bone.0 = None;
                        if let Some(cmd) = rig::place_bone(&mut doc.0, puppet, a, b) {
                            match apply_command(&mut doc.0, &mut state.undo, Box::new(cmd)) {
                                Ok(changes) => {
                                    pending.extend(changes.0);
                                    state.undo.break_merge();
                                }
                                Err(e) => error!("place bone failed: {e}"),
                            }
                        }
                    }
                    // A click on empty space, or the wrong puppet: disarm.
                    _ => pending_bone.0 = None,
                }
            }
        }

        // Vertex editing is M6.
        Tool::Vertex => {}
    }

    let _ = target;
}

fn apply_effect(
    effect: DragEffect,
    doc: &mut DocumentRes,
    state: &mut EditorState,
    targets: &mut JointTargets,
    pending: &mut PendingChangesRes,
) {
    match effect {
        DragEffect::None => {}
        DragEffect::Select { puppet, joint } => {
            state.selection = Selection::Joint(puppet, joint);
        }
        DragEffect::Command(cmd) => {
            match apply_command(&mut doc.0, &mut state.undo, Box::new(cmd)) {
                Ok(changes) => pending.extend(changes.0),
                Err(e) => error!("drag command failed: {e}"),
            }
        }
        DragEffect::EndGesture => state.undo.break_merge(),
        DragEffect::SetTarget { puppet, joint, pos } => targets.set(puppet, joint, pos),
        DragEffect::ClearTarget { puppet, joint } => targets.clear_joint(puppet, joint),
    }
}

/// Apply the dock's edits: inspector commands, layer moves, undo and redo.
///
/// The second writer system, and the last one — everything the UI can do
/// funnels through here or through `apply_interactions`.
pub fn apply_dock_output(
    mut out: ResMut<FrameDockOutput>,
    mut doc: ResMut<DocumentRes>,
    mut state: ResMut<EditorState>,
    mut pending: ResMut<PendingChangesRes>,
    mut output_config: Option<ResMut<animus_output::OutputConfig>>,
) {
    let Some(out) = out.0.take() else { return };

    // The vsync switch is app state, not document state: it is not part of
    // the show, so it does not go through the command layer or undo.
    if let Some(vsync) = out.set_output_vsync
        && let Some(cfg) = output_config.as_deref_mut()
    {
        cfg.vsync = vsync;
    }

    for edit in out.inspector_edits {
        if let Some(command) = edit.command {
            let boxed: Box<dyn animus_core::doc::DocCommand> = match command {
                InspectorCommand::BoneParam(c) => Box::new(c),
                InspectorCommand::JointPinned(c) => Box::new(c),
                InspectorCommand::LayerScalar(c) => Box::new(c),
                InspectorCommand::LayerName(c) => Box::new(c),
            };
            match apply_command(&mut doc.0, &mut state.undo, boxed) {
                Ok(changes) => pending.extend(changes.0),
                Err(e) => error!("inspector edit failed: {e}"),
            }
        }
        if edit.released {
            state.undo.break_merge();
        }
    }

    if let Some((layer, direction)) = out.layer_move {
        apply_layer_move(&mut doc, &mut state, &mut pending, layer, direction);
    }

    // Undo/redo drain the same PendingChanges pipe as everything else, so
    // the scene rebuilds from whatever the commands report.
    if out.wants_undo
        && let Some(result) = state.undo.undo(&mut doc.0)
    {
        match result {
            Ok(changes) => pending.extend(changes.0),
            Err(e) => error!("undo failed: {e}"),
        }
    }
    if out.wants_redo
        && let Some(result) = state.undo.redo(&mut doc.0)
    {
        match result {
            Ok(changes) => pending.extend(changes.0),
            Err(e) => error!("redo failed: {e}"),
        }
    }
}

/// Move a layer one step through the paint order and rewrite depths.
///
/// Spec §7.4's rule: `layer.depth` is authoritative world Z, and the list
/// reorders by rewriting depths with even spacing. Doing both in one undo
/// entry (ReorderLayers merges nothing, so the depth writes ride the same
/// gesture via an unsealed stack) keeps "move layer up" a single Ctrl+Z.
fn apply_layer_move(
    doc: &mut DocumentRes,
    state: &mut EditorState,
    pending: &mut PendingChangesRes,
    layer: animus_core::ids::LayerId,
    direction: i32,
) {
    let order = doc.0.layers.clone();
    let Some(index) = order.iter().position(|l| *l == layer) else {
        return;
    };
    let target = index as i32 + direction;
    if target < 0 || target as usize >= order.len() {
        return;
    }
    let mut to = order.clone();
    to.swap(index, target as usize);

    let reorder = animus_core::doc::ReorderLayers {
        from: order,
        to: to.clone(),
    };
    match apply_command(&mut doc.0, &mut state.undo, Box::new(reorder)) {
        Ok(changes) => pending.extend(changes.0),
        Err(e) => {
            error!("layer reorder failed: {e}");
            return;
        }
    }

    // Rewrite depths to match the new order: index * 0.01, back to front.
    for (i, lid) in to.iter().enumerate() {
        let Some(l) = doc.0.layer_data.get(lid) else {
            continue;
        };
        let want = i as f32 * 0.01;
        if (l.depth - want).abs() > f32::EPSILON {
            let cmd = animus_core::doc::SetLayerScalar {
                layer: *lid,
                which: animus_core::doc::LayerScalar::Depth,
                from: l.depth,
                to: want,
            };
            match apply_command(&mut doc.0, &mut state.undo, Box::new(cmd)) {
                Ok(changes) => pending.extend(changes.0),
                Err(e) => error!("depth rewrite failed: {e}"),
            }
        }
    }
    state.undo.break_merge();
}

/// Keyboard shortcuts: tools, mode, undo and redo.
///
/// The tooltips advertise these, and a shortcut that is advertised but
/// absent is worse than none. Gated on egui not holding keyboard focus.
pub fn keyboard_shortcuts(
    keys: Res<ButtonInput<KeyCode>>,
    egui_focus: Res<EguiWantsKeyboard>,
    mut state: ResMut<EditorState>,
    mut doc: ResMut<DocumentRes>,
    mut pending: ResMut<PendingChangesRes>,
) {
    if egui_focus.0 {
        return;
    }

    if keys.just_pressed(KeyCode::KeyV) {
        state.tool = Tool::Select;
    }
    if keys.just_pressed(KeyCode::KeyJ) {
        state.tool = Tool::Joint;
    }
    if keys.just_pressed(KeyCode::KeyB) {
        state.tool = Tool::Bone;
    }
    if keys.just_pressed(KeyCode::Tab) {
        state.mode = match state.mode {
            crate::state::EditMode::Edit => crate::state::EditMode::Live,
            crate::state::EditMode::Live => crate::state::EditMode::Edit,
        };
    }

    let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    if ctrl && keys.just_pressed(KeyCode::KeyZ) {
        let result = if shift {
            state.undo.redo(&mut doc.0)
        } else {
            state.undo.undo(&mut doc.0)
        };
        if let Some(result) = result {
            match result {
                Ok(changes) => pending.extend(changes.0),
                Err(e) => error!("undo/redo failed: {e}"),
            }
        }
    }
}
