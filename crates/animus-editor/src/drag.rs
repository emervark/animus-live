//! Dragging a joint, in the two modes that mean two different things.
//!
//! This is the single most consequential mode switch in the application
//! (spec §8.6), so the meaning of each mode is stated here, once:
//!
//! - **Edit**: the drag moves the joint's *rest position*. It becomes a
//!   [`MoveJointRest`] command — undoable, merged into one step per
//!   gesture — and the document changes.
//! - **Live**: the drag writes a *target* into [`JointTargets`] and the
//!   solver pulls the puppet toward it. **The document is never touched.**
//!   What the operator sees is the springs answering, which is also what
//!   the audience would see — the preview is not a preview, it is the real
//!   behaviour.
//!
//! The state machine is a pure value: (gesture, mode, document) in,
//! [`DragEffect`] out. One system applies effects; nothing else writes.
//! That is what keeps the one-way rule checkable — the tests below assert
//! "live mode leaves the document byte-identical" directly, which would be
//! unprovable if tool code wrote wherever it pleased.

use animus_core::doc::{MoveJointRest, Project, PuppetKind};
use animus_core::ids::{JointId, PuppetId};
use glam::Vec2;

use crate::rig;
use crate::state::EditMode;

/// A drag in progress, or not.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum DragState {
    #[default]
    Idle,
    /// A joint is being dragged. `from` is its rest position at grab time,
    /// kept so the whole gesture inverts as one step in edit mode.
    Dragging {
        puppet: PuppetId,
        joint: JointId,
        from: Vec2,
        mode: EditMode,
    },
}

/// What the input layer observed this frame, in image pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DragEvent {
    /// Button went down at this position.
    Grab(Vec2),
    /// Pointer moved to this position with the button held.
    MoveTo(Vec2),
    /// Button released.
    Release,
}

/// What should happen. The caller applies exactly what is listed and
/// nothing more.
#[derive(Debug, Clone, PartialEq)]
pub enum DragEffect {
    None,
    /// Edit mode: apply this command (it merges with the gesture's earlier
    /// moves).
    Command(MoveJointRest),
    /// Edit mode, gesture over: seal the undo entry.
    EndGesture,
    /// Live mode: set the solver target for this joint.
    SetTarget {
        puppet: PuppetId,
        joint: JointId,
        pos: Vec2,
    },
    /// Live mode, gesture over: release the target and let the springs
    /// take the limb back.
    ClearTarget {
        puppet: PuppetId,
        joint: JointId,
    },
    /// Select the joint that was grabbed, whichever mode.
    Select {
        puppet: PuppetId,
        joint: JointId,
    },
}

fn joint_rest(project: &Project, puppet: PuppetId, joint: JointId) -> Option<Vec2> {
    match &project.puppets.get(&puppet)?.kind {
        PuppetKind::Mesh(m) => Some(m.skeleton.joints.get(&joint)?.rest),
        _ => None,
    }
}

/// Advance the state machine. Returns the effects in application order.
pub fn step(
    state: &mut DragState,
    event: DragEvent,
    mode: EditMode,
    project: &Project,
    puppet: PuppetId,
    // Where each joint is *drawn* right now: rest in edit mode, the solver's
    // live position in live mode. The grab is tested against this and not
    // against the document, because those two disagree the moment the puppet
    // moves — and after the first pull it never agrees again.
    joints: &[(JointId, Vec2)],
    grab_radius_img: f32,
) -> Vec<DragEffect> {
    match (*state, event) {
        (DragState::Idle, DragEvent::Grab(pos)) => {
            let Some(joint) = rig::joint_at(joints, pos, grab_radius_img) else {
                return vec![DragEffect::None];
            };
            let Some(from) = joint_rest(project, puppet, joint) else {
                return vec![DragEffect::None];
            };
            *state = DragState::Dragging {
                puppet,
                joint,
                from,
                mode,
            };
            let select = DragEffect::Select { puppet, joint };
            match mode {
                // Grabbing in live mode starts pulling immediately: the
                // point of live mode is that touching the puppet moves it.
                // Rig authors the rest pose, so a tap only selects. Edit and
                // Perform both pull the live puppet — the difference between
                // them is what happens to the pull afterwards, not what the
                // drag does.
                EditMode::Rig => vec![select],
                EditMode::Edit | EditMode::Live => {
                    vec![select, DragEffect::SetTarget { puppet, joint, pos }]
                }
            }
        }

        (
            DragState::Dragging {
                puppet,
                joint,
                from,
                mode,
            },
            DragEvent::MoveTo(pos),
        ) => match mode {
            EditMode::Rig => vec![DragEffect::Command(MoveJointRest {
                puppet,
                joint,
                from,
                to: pos,
            })],
            EditMode::Edit | EditMode::Live => {
                vec![DragEffect::SetTarget { puppet, joint, pos }]
            }
        },

        (
            DragState::Dragging {
                puppet,
                joint,
                mode,
                ..
            },
            DragEvent::Release,
        ) => {
            *state = DragState::Idle;
            match mode {
                EditMode::Rig => vec![DragEffect::EndGesture],
                // **EDIT keeps the pose.** Letting go there used to clear the
                // target, and the rest pull walked the limb straight back
                // home — so the step the operator had just posed was already
                // wrong by the time they looked at it. In EDIT the pose *is*
                // the standing instruction.
                EditMode::Edit => vec![DragEffect::EndGesture],
                EditMode::Live => vec![DragEffect::ClearTarget { puppet, joint }],
            }
        }

        // A grab while dragging, or a move/release while idle: input noise.
        // Losing a Release (alt-tab mid-drag) lands here as a later Grab
        // while Dragging — reset so the machine cannot wedge.
        (DragState::Dragging { .. }, DragEvent::Grab(_)) => {
            *state = DragState::Idle;
            vec![DragEffect::None]
        }
        (DragState::Idle, _) => vec![DragEffect::None],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use animus_core::doc::{
        Joint, Layer, MeshPuppet, Puppet, SkeletonData, UndoStack, apply_command,
    };
    use animus_core::ids::{AssetId, LayerId};
    use indexmap::IndexMap;

    /// The radius these tests were written against, back when it was a
    /// fixed 8 image pixels. Stated once so the gestures below keep meaning
    /// what they meant when the zoom-dependent radius arrived.
    const TEST_GRAB_RADIUS: f32 = 8.0;

    /// The fixture's single joint, where it is drawn.
    fn displayed() -> Vec<(JointId, Vec2)> {
        vec![(J, Vec2::new(40.0, 40.0))]
    }

    const PUPPET: PuppetId = PuppetId(50);
    const J: JointId = JointId(51);

    fn project() -> Project {
        let mut joints = IndexMap::new();
        joints.insert(
            J,
            Joint {
                id: J,
                name: "j".into(),
                rest: Vec2::new(40.0, 40.0),
                rest_angle: 0.0,
                inv_mass: 1.0,
                pinned: false,
            },
        );
        let mut mp = MeshPuppet::empty(AssetId(52));
        mp.skeleton = SkeletonData {
            joints,
            bones: IndexMap::new(),
        };

        let mut p = Project::new("Drag Test");
        p.next_id = 500;
        let layer = Layer::new(LayerId(53), "L");
        p.layer_data.insert(LayerId(53), layer);
        p.layers.push(LayerId(53));
        p.puppets.insert(
            PUPPET,
            Puppet {
                id: PUPPET,
                name: "p".into(),
                kind: PuppetKind::Mesh(mp),
            },
        );
        p
    }

    fn snapshot(p: &Project) -> serde_json::Value {
        serde_json::to_value(p).unwrap()
    }

    /// Drive a full gesture and apply its effects the way the system does.
    fn drag_gesture(
        p: &mut Project,
        stack: &mut UndoStack,
        targets: &mut Vec<(PuppetId, JointId, Option<Vec2>)>,
        mode: EditMode,
        path: &[Vec2],
    ) {
        let mut state = DragState::Idle;
        let mut events = vec![DragEvent::Grab(path[0])];
        events.extend(path[1..].iter().map(|p| DragEvent::MoveTo(*p)));
        events.push(DragEvent::Release);

        for event in events {
            for effect in step(
                &mut state,
                event,
                mode,
                p,
                PUPPET,
                &displayed(),
                TEST_GRAB_RADIUS,
            ) {
                match effect {
                    DragEffect::Command(cmd) => {
                        apply_command(p, stack, Box::new(cmd)).unwrap();
                    }
                    DragEffect::EndGesture => stack.break_merge(),
                    DragEffect::SetTarget { puppet, joint, pos } => {
                        targets.push((puppet, joint, Some(pos)));
                    }
                    DragEffect::ClearTarget { puppet, joint } => {
                        targets.push((puppet, joint, None));
                    }
                    DragEffect::Select { .. } | DragEffect::None => {}
                }
            }
        }
    }

    #[test]
    fn an_edit_drag_moves_the_rest_position_as_one_undo_step() {
        let mut p = project();
        let mut stack = UndoStack::new();
        let mut targets = Vec::new();
        let before = snapshot(&p);

        // Grab at the joint, drag through 30 positions.
        let path: Vec<Vec2> = (0..30)
            .map(|i| Vec2::new(40.0 + i as f32 * 2.0, 40.0))
            .collect();
        drag_gesture(&mut p, &mut stack, &mut targets, EditMode::Rig, &path);

        match &p.puppets[&PUPPET].kind {
            PuppetKind::Mesh(m) => {
                assert_eq!(m.skeleton.joints[&J].rest, Vec2::new(98.0, 40.0));
            }
            _ => unreachable!(),
        }
        assert_eq!(stack.len(), 1, "thirty moves, one undo entry");
        assert!(targets.is_empty(), "edit mode never touches the solver");

        stack.undo(&mut p).unwrap().unwrap();
        assert_eq!(snapshot(&p), before, "one undo returns to before the drag");
    }

    #[test]
    fn a_live_drag_leaves_the_document_byte_identical() {
        // The one-way rule, asserted directly rather than believed.
        let mut p = project();
        let mut stack = UndoStack::new();
        let mut targets = Vec::new();
        let before = snapshot(&p);

        let path: Vec<Vec2> = (0..30)
            .map(|i| Vec2::new(40.0 + i as f32 * 3.0, 40.0 + i as f32))
            .collect();
        drag_gesture(&mut p, &mut stack, &mut targets, EditMode::Live, &path);

        assert_eq!(
            snapshot(&p),
            before,
            "a live drag must not change the document at all"
        );
        assert!(stack.is_empty(), "and leaves nothing on the undo stack");
        assert!(
            targets.len() >= 30,
            "the solver received the gesture instead"
        );
        assert_eq!(
            targets.last().unwrap().2,
            None,
            "and release cleared the target, so the springs take over"
        );
    }

    #[test]
    fn grabbing_empty_space_does_nothing_in_either_mode() {
        let mut p = project();
        for mode in [EditMode::Rig, EditMode::Edit, EditMode::Live] {
            let mut state = DragState::Idle;
            let effects = step(
                &mut state,
                DragEvent::Grab(Vec2::new(500.0, 500.0)),
                mode,
                &p,
                PUPPET,
                &displayed(),
                TEST_GRAB_RADIUS,
            );
            assert_eq!(effects, vec![DragEffect::None]);
            assert_eq!(state, DragState::Idle, "no drag begins on empty space");
        }
        let _ = &mut p;
    }

    #[test]
    fn grabbing_a_joint_selects_it() {
        let p = project();
        let mut state = DragState::Idle;
        let effects = step(
            &mut state,
            DragEvent::Grab(Vec2::new(41.0, 40.0)),
            EditMode::Edit,
            &p,
            PUPPET,
            &displayed(),
            TEST_GRAB_RADIUS,
        );
        assert!(effects.contains(&DragEffect::Select {
            puppet: PUPPET,
            joint: J
        }));
    }

    #[test]
    fn the_mode_at_grab_time_governs_the_whole_gesture() {
        // Switching modes mid-drag must not turn a live pull into document
        // edits halfway through: the gesture keeps the meaning it started
        // with.
        let p = project();
        let mut state = DragState::Idle;
        step(
            &mut state,
            DragEvent::Grab(Vec2::new(40.0, 40.0)),
            EditMode::Live,
            &p,
            PUPPET,
            &displayed(),
            TEST_GRAB_RADIUS,
        );

        // The operator flips to Edit while still holding the button; the
        // move is *still* a solver target.
        let effects = step(
            &mut state,
            DragEvent::MoveTo(Vec2::new(60.0, 40.0)),
            EditMode::Edit,
            &p,
            PUPPET,
            &displayed(),
            TEST_GRAB_RADIUS,
        );
        assert!(
            matches!(effects[0], DragEffect::SetTarget { .. }),
            "the gesture keeps its grab-time meaning, got {effects:?}"
        );
    }

    #[test]
    fn a_lost_release_cannot_wedge_the_machine() {
        // Alt-tab mid-drag eats the Release; the next Grab must recover
        // rather than continue the old gesture.
        let p = project();
        let mut state = DragState::Idle;
        step(
            &mut state,
            DragEvent::Grab(Vec2::new(40.0, 40.0)),
            EditMode::Edit,
            &p,
            PUPPET,
            &displayed(),
            TEST_GRAB_RADIUS,
        );
        assert!(matches!(state, DragState::Dragging { .. }));

        let effects = step(
            &mut state,
            DragEvent::Grab(Vec2::new(40.0, 40.0)),
            EditMode::Edit,
            &p,
            PUPPET,
            &displayed(),
            TEST_GRAB_RADIUS,
        );
        assert_eq!(state, DragState::Idle, "the machine resets");
        assert_eq!(effects, vec![DragEffect::None]);
    }
}
