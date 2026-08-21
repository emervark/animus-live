//! What crossing a mode boundary actually does.
//!
//! Three modes, three jobs, and the boundary between them does something
//! rather than only changing what the next drag means:
//!
//! - **RIG** builds the skeleton. Entering it puts the puppet on the pose the
//!   document actually holds, so what is on screen is the rig as saved rather
//!   than as it was last played.
//! - **EDIT** poses one step at a time. Entering it — or selecting a
//!   different step — puts the puppet into that step's pose, because the
//!   thing being edited has to be the thing on screen.
//! - **PERFORM** is the show, and entering it disturbs nothing.
//!
//! A switch that changed only the meaning of the next drag would leave the
//! operator looking at a puppet frozen mid-gesture while the panel claims
//! they are editing its rest pose.

use animus_runtime::{HeldJoint, JointTargets, Sequencer};
use bevy::prelude::*;

use crate::drag::DragState;
use crate::interact::ActiveDrag;
use crate::state::{EditMode, EditorState, Selection};

/// Project the selected joint's live pull into [`EditorState::live_offset`].
///
/// The inspector's LIVE read-out needs the difference between where a joint
/// *is* and where its rest position says it should be, and the panel that
/// draws it runs inside egui's pass with no access to solver components. So
/// the number is computed here, once, and read there — the same one-way
/// shape the document projection uses.
pub fn track_selection_live(
    mut state: ResMut<EditorState>,
    doc: Res<animus_runtime::DocumentRes>,
    rotations: Res<crate::rotate::LiveRotations>,
    solvers: Query<(
        &animus_runtime::PuppetRoot,
        &animus_runtime::CompiledRigRef,
        &animus_runtime::PuppetSolver,
    )>,
) {
    let Selection::Joint(pid, jid) = state.selection else {
        if state.live_offset.is_some() {
            state.live_offset = None;
        }
        if state.live_rotation != 0.0 {
            state.live_rotation = 0.0;
        }
        return;
    };

    let angle = rotations.get(pid, jid);
    if state.live_rotation != angle {
        state.live_rotation = angle;
    }

    let rest = crate::rig::joint_rest_positions(&doc.0, pid)
        .into_iter()
        .find(|(id, _)| *id == jid)
        .map(|(_, at)| at);

    let live = solvers
        .iter()
        .find(|(root, _, _)| root.0 == pid)
        .and_then(|(_, rig, solver)| {
            let dense = rig.0.joint_index(jid)?;
            solver.0.positions().get(dense as usize).copied()
        });

    let offset = rest.zip(live).map(|(rest, live)| live - rest);
    if state.live_offset != offset {
        state.live_offset = offset;
    }
}

/// Hold the puppet on its rest pose for as long as RIG is the mode.
///
/// **The drawing has to agree with the skeleton.** RIG's own viewport badge
/// says REST POSE, and rest is what it edits — but the solver keeps whatever
/// state it was left in, so moving a joint's rest position used to swing the
/// bones while the artwork stayed where the last pull put it. Rotating a
/// shoulder made that unmissable: the bones fanned out and the arm did not
/// follow them.
///
/// Settling once on entering the mode is not enough, because the edits
/// happen *after* entering it. Rest is a fixed point of the solver, so
/// re-applying it every frame is idempotent — it costs one copy per puppet
/// and removes the whole class of "the rig and the picture disagree".
pub fn hold_rest_in_rig(
    state: Res<EditorState>,
    mut solvers: Query<(
        &animus_runtime::CompiledRigRef,
        &mut animus_runtime::PuppetSolver,
    )>,
) {
    if state.mode != EditMode::Rig {
        return;
    }
    for (rig, mut solver) in &mut solvers {
        solver.0.reset_to_rest(&rig.0);
    }
}

/// Settle the puppet when the mode changes, and when the edited step does.
#[allow(clippy::too_many_arguments)]
pub fn settle_on_mode_change(
    state: Res<EditorState>,
    mut seq: ResMut<Sequencer>,
    mut last: Local<Option<(EditMode, usize)>>,
    mut targets: ResMut<JointTargets>,
    mut held: ResMut<HeldJoint>,
    mut drag: ResMut<ActiveDrag>,
    mut rotations: ResMut<crate::rotate::LiveRotations>,
    mut solvers: Query<(
        &animus_runtime::CompiledRigRef,
        &mut animus_runtime::PuppetSolver,
    )>,
) {
    let now = (state.mode, seq.selected);
    let previous = last.replace(now);
    // First frame: record and do nothing. Settling on startup would stop a
    // pattern a saved session had every right to still be running.
    let Some(previous) = previous else { return };
    if previous == now {
        return;
    }

    // **The dial belongs to the pose, not to the session.** Crossing into a
    // different step means a different pose is on screen, and a rotation
    // left set would keep stamping the old angle onto the new step. What the
    // operator authored is already in the step they left.
    rotations.0.clear();

    match state.mode {
        EditMode::Rig => {
            // The rig is what is being edited, so the rig is what must be on
            // screen. Rest is a fixed point of the solver, so the puppet then
            // sits still without the solver having to be paused — which
            // matters, because pausing it would mean writing to the
            // document's `SolverConfig` and dirtying a file nobody edited.
            seq.running = false;
            seq.armed = false;
            targets.clear();
            held.0 = None;
            drag.0 = DragState::Idle;
            for (rig, mut solver) in &mut solvers {
                solver.0.reset_to_rest(&rig.0);
            }
        }
        EditMode::Edit => {
            // Show the step being edited. Without this, selecting step 4
            // leaves the puppet in step 3's pose and the operator edits one
            // step while looking at another.
            seq.armed = false;
            let selected = seq.selected;
            if let Some(pose) = seq.pose(selected).cloned() {
                targets.clear();
                for (puppet, joint, at) in pose {
                    targets.set(puppet, joint, at);
                }
            } else if previous.0 != EditMode::Edit {
                // An empty step has no pose to show, so entering Edit on one
                // starts from rest rather than from whatever the last mode
                // left behind.
                targets.clear();
                for (rig, mut solver) in &mut solvers {
                    solver.0.reset_to_rest(&rig.0);
                }
            }
        }
        // Going to the stage should not disturb the rig or the pattern.
        EditMode::Live => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use animus_core::ids::{JointId, PuppetId};

    const P: PuppetId = PuppetId(1);
    const J: JointId = JointId(2);

    fn app() -> App {
        let mut app = App::new();
        app.init_resource::<EditorState>()
            .init_resource::<Sequencer>()
            .init_resource::<JointTargets>()
            .init_resource::<HeldJoint>()
            .init_resource::<ActiveDrag>()
            .init_resource::<crate::rotate::LiveRotations>()
            .add_systems(Update, settle_on_mode_change);
        app
    }

    fn dirty(app: &mut App) {
        app.world_mut()
            .resource_mut::<JointTargets>()
            .set(P, J, glam::Vec2::new(5.0, 5.0));
        app.world_mut().resource_mut::<HeldJoint>().0 = Some((P, J));
    }

    fn set_mode(app: &mut App, mode: EditMode) {
        app.world_mut().resource_mut::<EditorState>().mode = mode;
        app.update();
    }

    /// The first frame must not settle anything.
    #[test]
    fn startup_is_not_a_mode_change() {
        let mut app = app();
        dirty(&mut app);
        app.update();
        assert!(
            !app.world().resource::<JointTargets>().0.is_empty(),
            "the first frame settled a show it had no reason to touch"
        );
    }

    #[test]
    fn entering_rig_lets_go_of_everything_the_performance_was_holding() {
        let mut app = app();
        set_mode(&mut app, EditMode::Live);
        app.world_mut().resource_mut::<Sequencer>().running = true;

        dirty(&mut app);
        set_mode(&mut app, EditMode::Rig);

        assert!(app.world().resource::<JointTargets>().0.is_empty());
        assert!(app.world().resource::<HeldJoint>().0.is_none());
        assert!(!app.world().resource::<Sequencer>().running, "and stopped");
    }

    /// Entering PERFORM must disturb nothing: the stage is where the rig is
    /// used, not where it is changed.
    #[test]
    fn entering_perform_leaves_everything_alone() {
        let mut app = app();
        set_mode(&mut app, EditMode::Rig);
        dirty(&mut app);
        set_mode(&mut app, EditMode::Live);
        assert!(!app.world().resource::<JointTargets>().0.is_empty());
    }

    /// **The step being edited has to be the step on screen.**
    ///
    /// Selecting a different step in EDIT without showing it means the
    /// operator poses step 4 while looking at step 3 — and every pose they
    /// author is wrong by exactly one step.
    #[test]
    fn selecting_a_step_in_edit_shows_that_step() {
        let mut app = app();
        {
            let mut seq = app.world_mut().resource_mut::<Sequencer>();
            seq.set_pose(2, vec![(P, J, glam::Vec2::new(9.0, 9.0))]);
        }
        set_mode(&mut app, EditMode::Edit);

        app.world_mut().resource_mut::<Sequencer>().select(2);
        app.update();

        let targets = app.world().resource::<JointTargets>();
        assert_eq!(
            targets.0.get(&(P, J)).copied(),
            Some(glam::Vec2::new(9.0, 9.0)),
            "the puppet must be showing the step that is being edited"
        );
    }
}
