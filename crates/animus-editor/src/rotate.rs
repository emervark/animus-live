//! Rotating a joint in a pose mode: forward kinematics that bends the drawing.
//!
//! The same word means two different acts either side of the RIG boundary,
//! and conflating them is the trap this module exists to avoid.
//!
//! - In **RIG**, rotating moves the skeleton's *rest* positions. The artwork
//!   does not bend: the bind pose moves with the bones, so what the operator
//!   is doing is re-placing the skeleton over a drawing that stays put. That
//!   is [`animus_core::doc::RotateJoint`], and it is a document edit with an
//!   undo entry.
//! - In **EDIT** and **PERFORM**, rotating *poses* the puppet. The rest stays
//!   where it is and the limb swings away from it, which is what makes the
//!   drawing deform — the same thing a hand does by dragging, only about a
//!   pivot instead of along a line.
//!
//! Blender draws the same distinction between edit mode and pose mode, for
//! the same reason, and an operator who knows one will guess the other.
//!
//! ## Absolute, not incremental
//!
//! The angle is measured **from rest**, never accumulated frame to frame:
//!
//! ```text
//! target(joint) = live(pivot) + rotate(rest(joint) - rest(pivot), angle)
//! ```
//!
//! An incremental version would drift — every frame's rounding error added
//! to the last, so a limb returned to 0° would not return to where it
//! started. This form is idempotent: writing the same angle a thousand times
//! puts the limb in exactly the same place, and 0° is exactly rest.
//!
//! The pivot is taken **live** rather than at rest so that a rotated limb
//! still follows a hand that has grabbed the joint it hangs from: the
//! shoulder goes where the hand pulls it and the arm keeps its angle.

use animus_core::ids::{JointId, PuppetId};
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use glam::Vec2;

use animus_runtime::{DocumentRes, HeldJoint, JointTargets};

use crate::rig;

/// How far each joint is turned from its rest orientation, in radians.
///
/// Session state, like the sequencer's pattern: a pose is not a document
/// edit, and writing one to the file would dirty a project nobody changed.
/// In EDIT the resulting positions are captured into the step, which is
/// where a pose that matters gets kept.
#[derive(Resource, Debug, Default)]
pub struct LiveRotations(pub HashMap<(PuppetId, JointId), f32>);

impl LiveRotations {
    pub fn get(&self, puppet: PuppetId, joint: JointId) -> f32 {
        self.0.get(&(puppet, joint)).copied().unwrap_or(0.0)
    }

    /// Set an angle, or forget the joint entirely once it is back at rest.
    ///
    /// Forgetting matters: an entry left at 0° would keep writing targets
    /// for every joint below it, and a written target is a standing
    /// instruction that pins those joints out of the springs' reach.
    pub fn set(&mut self, puppet: PuppetId, joint: JointId, angle: f32) {
        if angle.abs() < 1e-4 {
            self.0.remove(&(puppet, joint));
        } else {
            self.0.insert((puppet, joint), angle);
        }
    }
}

/// A rotation being dragged on the stage.
///
/// Held across frames like any gesture, and holding the joint rather than
/// only the angle: the operator can drag the handle right past another
/// joint, and picking up whichever one is nearest halfway through a turn
/// would be indistinguishable from the application losing its mind.
#[derive(Resource, Debug, Default)]
pub struct RotationDrag(pub Option<(PuppetId, JointId)>);

/// Which joints a rotation wrote last frame, so they can be released.
#[derive(Resource, Debug, Default)]
pub struct RotationDriven(Vec<(PuppetId, JointId)>);

/// Turn every rotated limb, and let go of any that stopped being rotated.
#[allow(clippy::too_many_arguments)]
pub fn apply_live_rotations(
    state: Res<crate::state::EditorState>,
    doc: Res<DocumentRes>,
    rotations: Res<LiveRotations>,
    held: Res<HeldJoint>,
    mut targets: ResMut<JointTargets>,
    mut driven: ResMut<RotationDriven>,
    solvers: Query<(
        &animus_runtime::PuppetRoot,
        &animus_runtime::CompiledRigRef,
        &animus_runtime::PuppetSolver,
    )>,
) {
    // RIG rotates the rest pose through the command stack instead; running
    // both would have the document edit and the live pose fight over the
    // same joints.
    let posing = state.mode != crate::state::EditMode::Rig;

    // Release first. A joint that was rotated and is not any more must have
    // its target taken back, or it stays pinned where the rotation left it.
    for (puppet, joint) in std::mem::take(&mut driven.0) {
        if held.0 != Some((puppet, joint)) {
            targets.clear_joint(puppet, joint);
        }
    }
    if !posing || rotations.0.is_empty() {
        return;
    }

    for (&(puppet, pivot_joint), &angle) in rotations.0.iter() {
        let Some(PuppetKindMesh { skeleton }) = mesh_of(&doc, puppet) else {
            continue;
        };
        let rest: HashMap<JointId, Vec2> = rig::joint_rest_positions(&doc.0, puppet)
            .into_iter()
            .collect();
        let Some(&pivot_rest) = rest.get(&pivot_joint) else {
            continue;
        };

        // Where the pivot actually is now, which is where the limb has to
        // hang from — not where the document says it rests.
        let pivot_live = solvers
            .iter()
            .find(|(root, _, _)| root.0 == puppet)
            .and_then(|(_, rig, solver)| {
                let dense = rig.0.joint_index(pivot_joint)?;
                solver.0.positions().get(dense as usize).copied()
            })
            .unwrap_or(pivot_rest);

        let (sin, cos) = angle.sin_cos();
        for joint in animus_core::skeleton::rig_tree(skeleton).descendants(pivot_joint) {
            if held.0 == Some((puppet, joint)) {
                // The hand outranks the dial, the same way it outranks the
                // sequencer.
                continue;
            }
            let Some(&at) = rest.get(&joint) else {
                continue;
            };
            let d = at - pivot_rest;
            let turned = pivot_live + Vec2::new(d.x * cos - d.y * sin, d.x * sin + d.y * cos);
            targets.set(puppet, joint, turned);
            driven.0.push((puppet, joint));
        }
    }
}

/// A borrow of the puppet's skeleton, or nothing if it is not a mesh puppet.
struct PuppetKindMesh<'a> {
    skeleton: &'a animus_core::doc::SkeletonData,
}

fn mesh_of(doc: &DocumentRes, puppet: PuppetId) -> Option<PuppetKindMesh<'_>> {
    match &doc.0.puppets.get(&puppet)?.kind {
        animus_core::doc::PuppetKind::Mesh(m) => Some(PuppetKindMesh {
            skeleton: &m.skeleton,
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const P: PuppetId = PuppetId(3);
    const J: JointId = JointId(7);

    /// **Zero is not a rotation.** An entry sitting at 0° would keep writing
    /// targets for every joint below it, and a written target pins that
    /// joint out of the springs' reach — a limb that has been rotated back
    /// to rest would go stiff instead of hanging.
    #[test]
    fn returning_to_rest_forgets_the_joint_rather_than_storing_zero() {
        let mut r = LiveRotations::default();
        r.set(P, J, 0.8);
        assert_eq!(r.0.len(), 1);
        r.set(P, J, 0.0);
        assert!(
            r.0.is_empty(),
            "a joint back at rest is still driving targets"
        );
        assert_eq!(r.get(P, J), 0.0);
    }

    #[test]
    fn an_unrotated_joint_reads_as_zero() {
        assert_eq!(LiveRotations::default().get(P, J), 0.0);
    }
}
