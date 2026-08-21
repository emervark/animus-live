//! The one system that turns viewport gestures into writes.
//!
//! Everything upstream is pure: the viewport reports what happened
//! ([`ViewportInput`]), the drag machine decides what it means
//! ([`DragEffect`]), and this system is the single place effects touch the
//! world — commands into the document through the undo stack, targets into
//! [`JointTargets`]. Spec §8.6 calls for exactly one writer; this is it.

use animus_core::doc::{PuppetKind, apply_command};
use animus_core::ids::PuppetId;
use animus_runtime::{DocumentRes, JointTargets, PendingChangesRes, RenderScale};
use bevy::prelude::*;
use glam::Vec2;

use crate::dock::{DockOutput, LayerEdit, StepAction};
use crate::drag::{DragEffect, DragEvent, DragState, step};
use crate::hit;
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
        // **A selected layer names its puppet.** Treating every non-puppet
        // selection as "the first puppet in the document" meant that with two
        // images imported, every joint and bone landed on the first one
        // however carefully the operator had selected the second.
        Selection::Layer(layer) => doc
            .0
            .layer_data
            .get(&layer)
            .and_then(|l| l.contents.first().copied())
            .or_else(|| doc.0.puppets.keys().next().copied()),
        Selection::None => doc.0.puppets.keys().next().copied(),
    }
}

/// The puppet's joint ids, in skeleton order.
fn rig_joint_ids(doc: &DocumentRes, puppet: PuppetId) -> Vec<animus_core::ids::JointId> {
    match doc.0.puppets.get(&puppet).map(|p| &p.kind) {
        Some(PuppetKind::Mesh(mp)) => mp.skeleton.joints.keys().copied().collect(),
        _ => Vec::new(),
    }
}

/// World position → image pixels for a specific puppet.
///
/// **The layer's offset comes out first.** Image space is the puppet's own,
/// and the layer transform is what stands between it and the world. Skipping
/// that step is how placing a joint stopped landing under the cursor the
/// moment a puppet had been moved on the stage: every click was converted as
/// though the artwork were still at the origin, so the joint appeared exactly
/// as far from the pointer as the puppet had been dragged.
fn world_to_puppet_img(
    doc: &DocumentRes,
    scale: &RenderScale,
    puppet: PuppetId,
    world: Vec2,
) -> Option<Vec2> {
    crate::hit::stage_to_img(&doc.0, puppet, scale.ppu, world)
}

/// A layer being dragged across the stage.
///
/// Separate from [`ActiveDrag`], which is about joints. The two never run at
/// once: a grab tries the rig first and only falls through to the artwork when
/// it misses every joint.
#[derive(Resource, Debug, Default)]
pub struct LayerDrag(pub Option<LayerDragState>);

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LayerDragState {
    pub layer: animus_core::ids::LayerId,
    /// Where the pointer was in world units when the grab happened.
    pub grab_world: Vec2,
    /// Where the layer was then, so the whole gesture is one offset from one
    /// origin rather than an accumulation of per-frame deltas that drift.
    pub start: animus_core::doc::LayerPlacement,
    /// The corner being dragged, or `None` for a plain move.
    pub corner: Option<crate::hit::Corner>,
    /// The puppet's own bounds, before the layer transform. Captured at the
    /// grab so a resize solves against a fixed box rather than one that moves
    /// under it every frame.
    pub local: (Vec2, Vec2),
}

/// Where a corner drag puts the layer.
///
/// The **opposite corner stays still**, which is what makes a corner handle
/// feel like a corner handle rather than a slider: the operator anchors the
/// shape at the point they are not touching. That anchor is what forces
/// translation and scale to move together — resizing about a fixed point is a
/// scale *and* a shift, and doing only the scale slides the puppet out from
/// under the hand.
fn resize(
    active: &LayerDragState,
    corner: crate::hit::Corner,
    world: Vec2,
    uniform: bool,
) -> animus_core::doc::LayerPlacement {
    use animus_core::doc::{LayerPlacement, MIN_LAYER_SCALE};

    let (lo, hi) = active.local;
    let here = corner.of(lo, hi);
    let anchor_local = corner.opposite().of(lo, hi);
    let anchor_world = active.start.translation + anchor_local * active.start.scale;

    let span = here - anchor_local;
    let want = world - anchor_world;
    let scale = Vec2::new(
        if span.x.abs() > 1e-6 {
            want.x / span.x
        } else {
            active.start.scale.x
        },
        if span.y.abs() > 1e-6 {
            want.y / span.y
        } else {
            active.start.scale.y
        },
    );
    // Shift keeps the drawing's proportions. Measured against the scale the
    // gesture *started* from, not against 1.0, so a layer deliberately
    // squashed before the drag stays as squashed as it was — Shift locks the
    // aspect ratio, it does not reset one.
    let scale = if uniform {
        let start = active.start.scale;
        let fx = if start.x.abs() > 1e-6 {
            scale.x / start.x
        } else {
            1.0
        };
        let fy = if start.y.abs() > 1e-6 {
            scale.y / start.y
        } else {
            1.0
        };
        // The axis the hand moved further along leads, so the corner keeps
        // following the pointer on at least one axis instead of lagging on
        // both.
        let f = if fx.abs() > fy.abs() { fx } else { fy };
        Vec2::new(start.x * f, start.y * f)
    } else {
        scale
    };
    // Never through zero. A flipped puppet is a legitimate thing to want, but
    // arriving at it by passing through nothing loses the handles on the way.
    let scale = Vec2::new(
        scale.x.abs().max(MIN_LAYER_SCALE) * if scale.x < 0.0 { -1.0 } else { 1.0 },
        scale.y.abs().max(MIN_LAYER_SCALE) * if scale.y < 0.0 { -1.0 } else { 1.0 },
    );

    LayerPlacement {
        translation: anchor_world - anchor_local * scale,
        scale,
    }
}

/// Apply this frame's viewport input to the world.
#[allow(clippy::too_many_arguments)]
pub fn apply_interactions(
    input: Res<FrameViewportInput>,
    mut doc: ResMut<DocumentRes>,
    mut state: ResMut<EditorState>,
    mut drag: ResMut<ActiveDrag>,
    mut layer_drag: ResMut<LayerDrag>,
    mut targets: ResMut<JointTargets>,
    mut pending: ResMut<PendingChangesRes>,
    mut pending_bone: ResMut<PendingBone>,
    mut seq: ResMut<animus_runtime::Sequencer>,
    scale: Res<RenderScale>,
    cameras: Query<(&Camera, &GlobalTransform), With<ViewportCamera>>,
    target: Option<Res<ViewportTarget>>,
    mut held: ResMut<animus_runtime::HeldJoint>,
    solvers: Query<(
        &animus_runtime::PuppetRoot,
        &animus_runtime::CompiledRigRef,
        &animus_runtime::PuppetSolver,
    )>,
) {
    let Some(input) = input.0 else { return };
    let Ok((camera, camera_xf)) = cameras.single() else {
        return;
    };
    let Some(puppet) = active_puppet(&doc, state.selection) else {
        return;
    };

    // How close counts as "on the joint", in image pixels at this zoom. The
    // gizmo is drawn at the same radius, so the operator is aiming at the
    // thing they can see rather than at an invisible 8-pixel disc.
    let grab_radius = rig::grab_radius_img(
        target.as_ref().map(|t| t.world_per_pixel).unwrap_or(0.01),
        scale.ppu,
    );

    // Where the joints are *drawn*, which is what the hand is aiming at.
    //
    // Live mode shows the solver's positions, so a pulled or looping puppet
    // has its joints away from rest; hit-testing the document there misses
    // every one of them. Edit mode shows rest, and rest is what it edits.
    let displayed: Vec<(animus_core::ids::JointId, Vec2)> = match state.mode {
        crate::state::EditMode::Edit | crate::state::EditMode::Live => solvers
            .iter()
            .find(|(root, _, _)| root.0 == puppet)
            .map(|(_, rig, solver)| {
                let live = solver.0.positions();
                rig_joint_ids(&doc, puppet)
                    .into_iter()
                    .filter_map(|id| {
                        let dense = rig.0.joint_index(id)? as usize;
                        Some((id, *live.get(dense)?))
                    })
                    .collect()
            })
            .unwrap_or_else(|| rig::joint_rest_positions(&doc.0, puppet)),
        crate::state::EditMode::Rig => rig::joint_rest_positions(&doc.0, puppet),
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
            } else if input.dragging_left {
                // Held, not moved-this-frame: a hand that pauses mid-gesture
                // is still holding the joint.
                //
                // The grab is tested where the button went **down**, the move
                // follows where the hand is **now**. Testing both at "now" is
                // what made selection a coin flip: egui only calls a press a
                // drag once it passes a threshold, by which time the hand has
                // drifted off the joint it was aiming at.
                if matches!(drag.0, DragState::Idle)
                    && let Some(pixel) = input.press_origin.or(input.interact_at)
                    && let Some(img) = to_img(pixel)
                {
                    events.push(DragEvent::Grab(img));
                }
                if let Some(pixel) = input.interact_at.or(input.hover_at)
                    && let Some(img) = to_img(pixel)
                    && !matches!(drag.0, DragState::Idle)
                {
                    events.push(DragEvent::MoveTo(img));
                }
            } else if !matches!(drag.0, DragState::Idle) {
                events.push(DragEvent::Release);
            }

            // Nothing on the rig was grabbed, so the click means the picture.
            //
            // Moving artwork is a BUILD act: in PERFORM a drag pulls the
            // puppet, and having the same gesture sometimes slide the whole
            // layer instead would be the mode confusion this editor has spent
            // its life removing.
            let building = state.mode == crate::state::EditMode::Rig;
            let grabbed_a_joint = events.iter().any(|e| match e {
                DragEvent::Grab(img) => rig::joint_at(&displayed, *img, grab_radius).is_some(),
                _ => false,
            }) || !matches!(drag.0, DragState::Idle);

            if building && !grabbed_a_joint {
                let to_world =
                    |pixel: Vec2| crate::viewport::camera::pixel_to_world(camera, camera_xf, pixel);
                // Handles are grabbed at a size in screen pixels, like joints:
                // a target that shrinks as you zoom out is a target you cannot
                // hit exactly when you most need it.
                let handle_radius = crate::rig::JOINT_SCREEN_RADIUS_PX
                    * target.as_ref().map(|t| t.world_per_pixel).unwrap_or(0.01);

                if layer_drag.0.is_none()
                    && (input.dragging_left || input.clicked_at.is_some())
                    && let Some(pixel) = input.press_origin.or(input.interact_at)
                    && let Some(world) = to_world(pixel)
                {
                    // A handle on the *selected* puppet outranks the artwork
                    // under the cursor: the corner of a box sticks out past
                    // the picture, and reaching for it must not pick up
                    // whatever is behind it.
                    let selected = match state.selection {
                        Selection::Puppet(p) => Some(p),
                        Selection::Joint(p, _) | Selection::Bone(p, _) => Some(p),
                        _ => None,
                    };
                    let handle = selected.and_then(|p| {
                        let (lo, hi) = crate::hit::selection_box(&doc.0, p, scale.ppu)?;
                        let corner = crate::hit::corner_at(lo, hi, world, handle_radius)?;
                        let layer = crate::hit::layer_of(&doc.0, p)?;
                        let local = crate::hit::puppet_bounds_local(&doc.0, p, scale.ppu)?;
                        Some((layer, corner, local))
                    });

                    if let Some((layer, corner, local)) = handle {
                        layer_drag.0 = Some(LayerDragState {
                            layer,
                            grab_world: world,
                            start: crate::hit::layer_placement(&doc.0, layer),
                            corner: Some(corner),
                            local,
                        });
                    } else if let Some((hit_puppet, layer)) =
                        hit::puppet_at(&doc.0, scale.ppu, world)
                    {
                        state.selection = Selection::Puppet(hit_puppet);
                        if let Some(local) =
                            crate::hit::puppet_bounds_local(&doc.0, hit_puppet, scale.ppu)
                        {
                            layer_drag.0 = Some(LayerDragState {
                                layer,
                                grab_world: world,
                                start: crate::hit::layer_placement(&doc.0, layer),
                                corner: None,
                                local,
                            });
                        }
                    }
                }

                if let Some(active) = layer_drag.0 {
                    if input.dragging_left {
                        if let Some(pixel) = input.interact_at.or(input.hover_at)
                            && let Some(world) = to_world(pixel)
                        {
                            let to = match active.corner {
                                None => animus_core::doc::LayerPlacement {
                                    translation: active.start.translation
                                        + (world - active.grab_world),
                                    scale: active.start.scale,
                                },
                                Some(corner) => resize(&active, corner, world, input.shift),
                            };
                            let from = crate::hit::layer_placement(&doc.0, active.layer);
                            if from != to {
                                let cmd = animus_core::doc::TransformLayer {
                                    layer: active.layer,
                                    from,
                                    to,
                                };
                                match apply_command(&mut doc.0, &mut state.undo, Box::new(cmd)) {
                                    Ok(changes) => pending.extend(changes.0),
                                    Err(e) => error!("place layer failed: {e}"),
                                }
                            }
                        }
                    } else {
                        // The gesture is over: seal it so the next one is its
                        // own undo step rather than merging into this one.
                        state.undo.break_merge();
                        layer_drag.0 = None;
                    }
                }
            }

            for event in events {
                let effects = step(
                    &mut drag.0,
                    event,
                    state.mode,
                    &doc.0,
                    puppet,
                    &displayed,
                    grab_radius,
                );
                for effect in effects {
                    apply_effect(
                        effect,
                        &mut doc,
                        &mut state,
                        &mut targets,
                        &mut pending,
                        &mut held,
                    );
                }
            }
        }

        // Creating is an Edit-mode act, always. Live mode is the show: a
        // hand on the puppet mid-performance must be able to pull it and
        // nothing else, or a stray click leaves a joint in the rig with the
        // audience watching. Manipulating is allowed in both; authoring is
        // not. (The tools are also disabled in the panel, so this is the
        // second of two doors rather than the only one.)
        Tool::Joint if state.mode == crate::state::EditMode::Rig => {
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

        Tool::Bone if state.mode == crate::state::EditMode::Rig => {
            if let Some(pixel) = input.clicked_at
                && let Some(img) = to_img(pixel)
            {
                match (pending_bone.0, rig::joint_at(&displayed, img, grab_radius)) {
                    // First click on a joint arms the bone.
                    (None, Some(joint)) => {
                        pending_bone.0 = Some((puppet, joint));
                        state.selection = Selection::Joint(puppet, joint);
                    }
                    // Second click on another joint completes it, and leaves
                    // that joint armed for the next one.
                    //
                    // A skeleton is a chain: hip to knee to ankle. Disarming
                    // after every bone made the operator click the joint they
                    // had just connected a second time, once per bone, for
                    // the whole rig. Now the far end of the bone you just
                    // made is the near end of the next one, and a limb is one
                    // click per joint.
                    (Some((armed_puppet, a)), Some(b)) if armed_puppet == puppet => {
                        if let Some(cmd) = rig::place_bone(&mut doc.0, puppet, a, b) {
                            match apply_command(&mut doc.0, &mut state.undo, Box::new(cmd)) {
                                Ok(changes) => {
                                    pending.extend(changes.0);
                                    state.undo.break_merge();
                                }
                                Err(e) => error!("place bone failed: {e}"),
                            }
                        }
                        pending_bone.0 = Some((puppet, b));
                        state.selection = Selection::Joint(puppet, b);
                    }
                    // A click on empty space, or the wrong puppet: disarm.
                    _ => pending_bone.0 = None,
                }
            }
        }

        // Vertex editing is M6; the authoring tools in live mode are the
        // rule above and do nothing here on purpose.
        Tool::Vertex | Tool::Joint | Tool::Bone => {}
    }

    // ── EDIT writes the pose into the step being edited ──
    //
    // Continuously while dragging, not on release: the operator chose "the
    // step updates as you move it", and a pose that only lands on mouse-up
    // means the grid disagrees with the screen for the whole gesture.
    if state.mode == crate::state::EditMode::Edit && input.dragging_left {
        let pose = animus_runtime::capture_from(
            solvers
                .iter()
                .map(|(root, rig, solver)| (root.0, rig, solver)),
        );
        if !pose.is_empty() {
            let step = seq.selected;
            seq.set_pose(step, pose);
        }
    }

    let _ = target;
}

fn apply_effect(
    effect: DragEffect,
    doc: &mut DocumentRes,
    state: &mut EditorState,
    targets: &mut JointTargets,
    pending: &mut PendingChangesRes,
    held: &mut animus_runtime::HeldJoint,
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
        // Publishing the held joint is what lets a clip stand aside for a
        // hand on the same limb.
        DragEffect::SetTarget { puppet, joint, pos } => {
            targets.set(puppet, joint, pos);
            held.0 = Some((puppet, joint));
        }
        DragEffect::ClearTarget { puppet, joint } => {
            targets.clear_joint(puppet, joint);
            if held.0 == Some((puppet, joint)) {
                held.0 = None;
            }
        }
    }
}

/// Apply the dock's edits: inspector commands, layer moves, undo and redo.
///
/// The second writer system, and the last one — everything the UI can do
/// funnels through here or through `apply_interactions`.
#[allow(clippy::too_many_arguments)]
pub fn apply_dock_output(
    mut out: ResMut<FrameDockOutput>,
    mut doc: ResMut<DocumentRes>,
    mut state: ResMut<EditorState>,
    mut pending: ResMut<PendingChangesRes>,
    mut output_config: Option<ResMut<animus_output::OutputConfig>>,
    mut targets: ResMut<JointTargets>,
    mut drag: ResMut<ActiveDrag>,
    mut solvers: Query<(
        &animus_runtime::CompiledRigRef,
        &mut animus_runtime::PuppetSolver,
    )>,
    mut seq: ResMut<animus_runtime::Sequencer>,
    mut wants_fit: ResMut<crate::fit::WantsFit>,
    mut rotations: ResMut<crate::rotate::LiveRotations>,
) {
    let Some(out) = out.0.take() else { return };

    for action in out.step_actions {
        match action {
            StepAction::Select(i) => seq.select(i),
            StepAction::Clear(i) => seq.clear_step(i),
            StepAction::ClearAll => seq.clear_all(),
            StepAction::SetRunning(on) => {
                seq.running = on;
                if !on {
                    // Leaving the playhead where it stopped would make the
                    // next run start mid-bar, which is the one thing a grid
                    // must never do.
                    seq.position = 0.0;
                    seq.armed = false;
                }
            }
            StepAction::SetArmed(on) => seq.armed = on,
            StepAction::SetLength(n) => seq.set_len(n),
            StepAction::SetBpm(v) => seq.bpm = v.clamp(20.0, 300.0),
            StepAction::ToggleCollapsed => {
                state.clips_collapsed = !state.clips_collapsed;
            }
        }
    }

    // Back to rest. Solver state only — the document never recorded the pull,
    // so there is nothing here for undo to have caught, and nothing for this
    // to undo. Any gesture in flight is dropped with it, or the hand would
    // keep pulling a puppet that had just been reset.
    // A pose-mode rotation. Session state, so it never reaches the document
    // or the undo stack — in EDIT the positions it produces are what the
    // step captures, which is where a pose that matters gets kept.
    if let Some(angle) = out.set_live_rotation
        && let crate::state::Selection::Joint(pid, jid) = state.selection
    {
        rotations.set(pid, jid, angle);
    }

    if out.wants_reset_pose {
        for (rig, mut solver) in &mut solvers {
            solver.0.reset_to_rest(&rig.0);
        }
        targets.clear();
        drag.0 = DragState::Idle;
        // Without this the dial would write its angle straight back on the
        // next frame, and the button would look broken.
        rotations.0.clear();
    }

    // Framing is a view change, so it does not touch the document either.
    if out.wants_fit {
        wants_fit.0 = true;
    }

    // The Output panel: two app-state switches and one document edit, which
    // is exactly why they are separated here. Resolution re-crops the whole
    // show, so it goes through the command stack; fullscreen and vsync are
    // preferences about this machine and do not belong in the file.
    if let Some(cfg) = output_config.as_deref_mut() {
        if let Some(on) = out.output_edits.set_vsync {
            cfg.vsync = on;
        }
        if let Some(on) = out.output_edits.set_fullscreen {
            cfg.fullscreen = Some(on);
        }
    }
    if let Some(to) = out.output_edits.set_canvas
        && doc.0.stage.canvas != to
    {
        let cmd = animus_core::doc::SetStageCanvas {
            from: doc.0.stage.canvas,
            to,
        };
        match apply_command(&mut doc.0, &mut state.undo, Box::new(cmd)) {
            Ok(changes) => pending.extend(changes.0),
            Err(e) => error!("set resolution failed: {e}"),
        }
    }

    // The vsync switch is app state, not document state: it is not part of
    // the show, so it does not go through the command layer or undo.
    if let Some(vsync) = out.set_output_vsync
        && let Some(cfg) = output_config.as_deref_mut()
    {
        cfg.vsync = vsync;
    }

    for edit in out.inspector_edits {
        if let Some(command) = edit.command {
            // Deletions arrive as an intent and are built here, where the
            // document is in hand: the panel names what should go, this
            // decides what that means for the skeleton.
            let boxed: Box<dyn animus_core::doc::DocCommand> = match command {
                InspectorCommand::BoneParam(c) => Box::new(c),
                InspectorCommand::JointPinned(c) => Box::new(c),
                InspectorCommand::LayerScalar(c) => Box::new(c),
                InspectorCommand::LayerName(c) => Box::new(c),
                InspectorCommand::SolverParam(c) => Box::new(c),
                InspectorCommand::JointMass(c) => Box::new(c),
                InspectorCommand::JointRotation(c) => Box::new(c),
                InspectorCommand::DeleteJoint(pid, jid) => {
                    let Some(cmd) = rig::delete_joint(&doc.0, pid, jid) else {
                        continue;
                    };
                    state.selection = Selection::None;
                    Box::new(cmd)
                }
                InspectorCommand::DeleteBone(pid, bid) => {
                    let Some(cmd) = rig::delete_bone(&doc.0, pid, bid) else {
                        continue;
                    };
                    state.selection = Selection::None;
                    Box::new(cmd)
                }
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

    for edit in out.layer_edits {
        apply_layer_edit(&mut doc, &mut state, &mut pending, edit);
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

/// Hide, duplicate or delete a layer, through the command stack.
///
/// All three go through `apply_command` rather than touching `Project`
/// directly, so all three are one Ctrl+Z away — which matters most for the
/// one that destroys work.
fn apply_layer_edit(
    doc: &mut DocumentRes,
    state: &mut EditorState,
    pending: &mut PendingChangesRes,
    edit: LayerEdit,
) {
    let command: Box<dyn animus_core::doc::DocCommand> = match edit {
        LayerEdit::SetVisible(layer, to) => {
            let Some(current) = doc.0.layer_data.get(&layer) else {
                return;
            };
            Box::new(animus_core::doc::SetLayerVisible {
                layer,
                from: current.visible,
                to,
            })
        }
        LayerEdit::ResetPlacement(layer) => {
            let from = crate::hit::layer_placement(&doc.0, layer);
            let to = animus_core::doc::LayerPlacement {
                translation: Vec2::ZERO,
                scale: Vec2::ONE,
            };
            if from == to {
                // Already home. Pushing a no-op would still cost an undo step
                // and leave the operator wondering what the last Ctrl+Z did.
                return;
            }
            Box::new(animus_core::doc::TransformLayer { layer, from, to })
        }
        LayerEdit::Duplicate(layer) => Box::new(animus_core::doc::DuplicateLayer::new(layer)),
        LayerEdit::Delete(layer) => {
            // A selection pointing at a layer that is about to stop existing
            // would leave the inspector describing a ghost.
            if state.selection == Selection::Layer(layer) {
                state.selection = Selection::None;
            }
            Box::new(animus_core::doc::RemoveLayer::new(layer))
        }
    };

    match apply_command(&mut doc.0, &mut state.undo, command) {
        Ok(changes) => {
            pending.extend(changes.0);
            // Each of these is a whole act, never half of a gesture, so none
            // of them may merge with whatever the operator does next.
            state.undo.break_merge();
        }
        Err(e) => error!("layer edit failed: {e}"),
    }
}

/// Keyboard shortcuts: tools, mode, undo and redo.
///
/// The tooltips advertise these, and a shortcut that is advertised but
/// absent is worse than none. Gated on egui not holding keyboard focus.
#[allow(clippy::too_many_arguments)]
pub fn keyboard_shortcuts(
    keys: Res<ButtonInput<KeyCode>>,
    egui_focus: Res<EguiWantsKeyboard>,
    mut state: ResMut<EditorState>,
    mut doc: ResMut<DocumentRes>,
    mut pending: ResMut<PendingChangesRes>,
    mut targets: ResMut<JointTargets>,
    mut drag: ResMut<ActiveDrag>,
    mut seq: ResMut<animus_runtime::Sequencer>,
    mut rotations: ResMut<crate::rotate::LiveRotations>,
    mut solvers: Query<(
        &animus_runtime::CompiledRigRef,
        &mut animus_runtime::PuppetSolver,
    )>,
) {
    if egui_focus.0 {
        return;
    }

    // R: back to rest. The same effect as the Tools panel's button, because
    // a puppet left in a shape by a pull is the state an operator most often
    // wants out of, and reaching for a panel to do it is a reach too many.
    if keys.just_pressed(KeyCode::KeyR) {
        for (rig, mut solver) in &mut solvers {
            solver.0.reset_to_rest(&rig.0);
        }
        targets.clear();
        drag.0 = DragState::Idle;
        rotations.0.clear();
    }

    if keys.just_pressed(KeyCode::KeyV) {
        state.tool = Tool::Select;
    }
    // J and B are authoring tools, and authoring is Edit-mode only — so the
    // shortcut refuses in live mode rather than arming a tool that would
    // silently do nothing.
    let editing = state.mode == crate::state::EditMode::Rig;
    if editing && keys.just_pressed(KeyCode::KeyJ) {
        state.tool = Tool::Joint;
    }
    if editing && keys.just_pressed(KeyCode::KeyB) {
        state.tool = Tool::Bone;
    }

    // Delete acts on whatever is selected, in whatever mode. A key that only
    // works on two of the four things an operator can select is a key they
    // stop reaching for.
    let del = keys.just_pressed(KeyCode::Delete) || keys.just_pressed(KeyCode::Backspace);

    // In EDIT the grid is the work, so Delete empties the step being posed
    // rather than reaching into the rig underneath it.
    if del && state.mode == crate::state::EditMode::Edit {
        let step = seq.selected;
        seq.clear_step(step);
    }
    // A layer is a whole image and everything rigged onto it, so deleting one
    // goes through the same command the panel's trash icon uses — one act,
    // one Ctrl+Z.
    if del && let Selection::Layer(layer) = state.selection {
        apply_layer_edit(&mut doc, &mut state, &mut pending, LayerEdit::Delete(layer));
    }

    if editing && del {
        let command = match state.selection {
            Selection::Joint(pid, jid) => rig::delete_joint(&doc.0, pid, jid),
            Selection::Bone(pid, bid) => rig::delete_bone(&doc.0, pid, bid),
            _ => None,
        };
        if let Some(cmd) = command {
            match apply_command(&mut doc.0, &mut state.undo, Box::new(cmd)) {
                Ok(changes) => {
                    pending.extend(changes.0);
                    state.undo.break_merge();
                    state.selection = Selection::None;
                }
                Err(e) => error!("delete failed: {e}"),
            }
        }
    }
    // Tab cycles the three stages in the order the work happens.
    if keys.just_pressed(KeyCode::Tab) {
        use crate::state::EditMode as M;
        state.mode = match state.mode {
            M::Rig => M::Edit,
            M::Edit => M::Live,
            M::Live => M::Rig,
        };
        state.tool = crate::chrome::Stage::of(state.mode).tool();
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
