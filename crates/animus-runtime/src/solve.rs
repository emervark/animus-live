//! Driving the solver, and writing its result onto the bones. Spec §8.2–8.5.
//!
//! ## The shape of a frame
//!
//! Targets in, fixed-rate stepping, guard, then one interpolated writeback
//! per render frame. The solver runs at `SolverConfig.hz` (120 by default)
//! while the display runs at 60, 144 or 240 — without interpolation those
//! two rates beat against each other visibly, so writeback lerps between the
//! previous tick and the current one by `overstep_fraction()`.
//!
//! ## Why the state is a component
//!
//! `PuppetSolver` being a component rather than a field of one resource is
//! what lets `par_iter_mut` hand each puppet to a different thread. With
//! 10–60 joints per puppet that only matters past roughly fifty puppets, but
//! it costs nothing to get right now and cannot be retrofitted cheaply.
//!
//! ## Dropping simulation time is the correct failure
//!
//! `Time::<Virtual>::max_delta` clamps the frame delta *before* the
//! accumulator sees it, so a hitch drops simulation time instead of paying
//! it back as a burst of catch-up substeps. For a live show that is always
//! the right call: a puppet a few milliseconds behind is invisible, a frame
//! spike is not.

use animus_core::ids::{JointId, PuppetId};
use animus_core::solver::{StepOutcome, step};
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::transform::TransformSystems;
use glam::Vec2;

use crate::components::{BoneOf, CompiledRigRef, PuppetRoot, PuppetSolver};
use crate::coords::img_to_world;
use crate::project::{DocumentRes, RenderScale};

/// Where solving happens, inside `FixedUpdate`.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SolveSet {
    /// Targets → solver state.
    Apply,
    /// One fixed tick per puppet, in parallel.
    Step,
    /// Non-finite state → reset, and say so.
    Guard,
}

/// Where the solver's result reaches the scene, in `PostUpdate`.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WritebackSet {
    Bones,
}

/// Joints being pulled somewhere: by a hand in the viewport now, by a
/// binding or a clip later.
///
/// M2 introduces `TargetPath` and a general `TargetValues` map; until then
/// this is the concrete thing live dragging needs, and the bus will feed it
/// rather than replace it.
#[derive(Resource, Debug, Default)]
pub struct JointTargets(pub HashMap<(PuppetId, JointId), Vec2>);

impl JointTargets {
    pub fn set(&mut self, puppet: PuppetId, joint: JointId, image_pos: Vec2) {
        self.0.insert((puppet, joint), image_pos);
    }

    pub fn clear_joint(&mut self, puppet: PuppetId, joint: JointId) {
        self.0.remove(&(puppet, joint));
    }

    pub fn clear(&mut self) {
        self.0.clear();
    }
}

/// The joint a hand is holding right now.
///
/// Clips skip it. That single rule is what makes grabbing a looping limb
/// feel like taking it over rather than fighting it: the clip keeps playing,
/// keeps driving every other joint it owns, and simply stops writing to the
/// one under the hand until the hand lets go.
#[derive(Resource, Debug, Default)]
pub struct HeldJoint(pub Option<(PuppetId, JointId)>);

/// How far each joint is turned from its rest orientation, in radians.
///
/// **Beside [`JointTargets`] rather than in the editor**, because a rotation
/// is a pose and posing is what the runtime does. It was in the editor while
/// the only thing that could turn a joint was a hand on a dial; once a
/// binding could turn one too, keeping it there would have meant applying
/// the same idea in two crates — the sort of split that ends with one of
/// them quietly disagreeing.
///
/// Session state, like the sequencer's pattern: a pose is not a document
/// edit, and writing one to the file would dirty a project nobody changed.
#[derive(Resource, Debug, Default)]
pub struct LiveRotations(pub bevy::platform::collections::HashMap<(PuppetId, JointId), f32>);

impl LiveRotations {
    pub fn get(&self, puppet: PuppetId, joint: JointId) -> f32 {
        self.0.get(&(puppet, joint)).copied().unwrap_or(0.0)
    }

    /// Set an angle, or forget the joint entirely once it is back at rest.
    ///
    /// Forgetting matters: an entry left at 0 degrees would keep writing
    /// targets for every joint below it, and a written target is a standing
    /// instruction that pins those joints out of the springs' reach.
    pub fn set(&mut self, puppet: PuppetId, joint: JointId, angle: f32) {
        if angle.abs() < 1e-4 {
            self.0.remove(&(puppet, joint));
        } else {
            self.0.insert((puppet, joint), angle);
        }
    }
}

/// A puppet's state went non-finite and was reset. One puppet, not the show.
#[derive(Message, Debug, Clone, Copy)]
pub struct SolverPanic(pub PuppetId);

/// What the last tick did, recorded per puppet so the parallel step can
/// report without touching shared state.
#[derive(Component, Debug, Clone, Copy)]
pub struct LastStepOutcome(pub StepOutcome);

/// Targets → solver state, once per tick.
pub fn apply_targets(
    targets: Res<JointTargets>,
    mut q: Query<(&PuppetRoot, &CompiledRigRef, &mut PuppetSolver)>,
) {
    for (root, rig, mut solver) in &mut q {
        // Clearing first is what lets go of a released joint: a stale target
        // would hold the limb where the mouse left it for the rest of the
        // show.
        solver.0.clear_all_targets();

        for ((pid, jid), pos) in targets.0.iter() {
            if *pid != root.0 {
                continue;
            }
            if let Some(index) = rig.0.joint_index(*jid) {
                solver.0.set_target(index, *pos);
            }
        }
    }
}

/// One fixed tick per puppet, in parallel.
pub fn step_solvers(
    time: Res<Time<Fixed>>,
    doc: Res<DocumentRes>,
    mut q: Query<(&CompiledRigRef, &mut PuppetSolver, &mut LastStepOutcome)>,
) {
    if !doc.0.solver.enabled {
        return;
    }
    let dt = time.delta_secs();
    q.par_iter_mut().for_each(|(rig, mut solver, mut outcome)| {
        outcome.0 = step(&rig.0, &mut solver.0, dt);
    });
}

/// Report the puppets the solver had to reset.
pub fn guard_solvers(
    mut panics: MessageWriter<SolverPanic>,
    q: Query<(&PuppetRoot, &LastStepOutcome), Changed<LastStepOutcome>>,
) {
    for (root, outcome) in &q {
        if outcome.0 == StepOutcome::ResetDueToNonFinite {
            // `step` already reset this puppet to rest; the event exists so
            // the editor can say which puppet, rather than leaving the
            // operator to notice a limb went missing.
            error!("puppet {:?} solver went non-finite and was reset", root.0);
            panics.write(SolverPanic(root.0));
        }
    }
}

/// Solver state → bone transforms, interpolated.
///
/// Each bone's transform is derived from its two joints exactly as the bind
/// pose was: translation at joint A, Z rotation from the A→B direction. That
/// symmetry is the whole reason skinning lands where it should.
pub fn writeback_bones(
    fixed: Res<Time<Fixed>>,
    scale: Res<RenderScale>,
    doc: Res<DocumentRes>,
    roots: Query<(&PuppetRoot, &CompiledRigRef, &PuppetSolver)>,
    mut bones: Query<(&BoneOf, &mut Transform)>,
) {
    let alpha = fixed.overstep_fraction();

    // Gather per-puppet interpolated joint positions once, rather than
    // re-deriving them for every bone that shares a joint.
    let mut per_puppet: HashMap<PuppetId, (Vec<Vec2>, &CompiledRigRef)> = HashMap::default();
    for (root, rig, solver) in &roots {
        let now = solver.0.positions();
        let prev = solver.0.prev_tick_positions();
        let lerped: Vec<Vec2> = now
            .iter()
            .zip(prev.iter())
            .map(|(a, b)| *b + (*a - *b) * alpha)
            .collect();
        per_puppet.insert(root.0, (lerped, rig));
    }

    for (bone, mut transform) in &mut bones {
        let Some((joints, rig)) = per_puppet.get(&bone.puppet) else {
            continue;
        };
        let Some((ja, jb)) = rig.0.bone_joints(bone.index as usize) else {
            continue;
        };
        let (Some(a_img), Some(b_img)) = (
            joints.get(ja as usize).copied(),
            joints.get(jb as usize).copied(),
        ) else {
            continue;
        };

        let a = img_to_world(a_img, pivot_for(&doc, bone.puppet), scale.ppu);
        let b = img_to_world(b_img, pivot_for(&doc, bone.puppet), scale.ppu);
        let dir = b - a;
        transform.translation = a;
        transform.rotation = Quat::from_rotation_z(dir.y.atan2(dir.x));

        // `length_mul` is squash/stretch along the bone's own X. Applying it
        // here rather than to the joints keeps it out of the constraint
        // solve, where it would fight the distance constraint.
        let rest_len = rig.0.rest_length(bone.index as usize);
        transform.scale = Vec3::new(bone_stretch(dir.length(), rest_len, scale.ppu), 1.0, 1.0);
    }
}

/// Squash/stretch along a bone's own X, as a ratio of two lengths in the
/// *same* unit.
///
/// The rest length is image pixels — the solver's space — and the live
/// length is world units. Dividing them directly gives `1/ppu`, which at the
/// default scale squashed every bone to a hundredth of its length: the
/// puppet collapsed onto its own skeleton, arms first, and the head and feet
/// beyond the end joints were pulled in as if cropped. It looked like a
/// skinning bug and it is a unit bug, so the conversion lives in one named
/// function rather than inline in the system.
fn bone_stretch(current_world: f32, rest_image_px: f32, ppu: f32) -> f32 {
    let rest_world = rest_image_px / ppu;
    if rest_world > 1e-6 {
        current_world / rest_world
    } else {
        1.0
    }
}

fn pivot_for(doc: &DocumentRes, id: PuppetId) -> Vec2 {
    match doc.0.puppets.get(&id).map(|p| &p.kind) {
        Some(animus_core::doc::PuppetKind::Mesh(m)) => crate::project::puppet_pivot(m),
        _ => Vec2::ZERO,
    }
}

/// Installs the solver schedule. Separate from `RuntimePlugin` so a headless
/// test can project a document without running physics.
pub struct SolverPlugin;

impl Plugin for SolverPlugin {
    fn build(&self, app: &mut App) {
        let hz = app
            .world()
            .get_resource::<DocumentRes>()
            .map(|d| d.0.solver.hz)
            .unwrap_or(120)
            .max(1);
        let substeps = app
            .world()
            .get_resource::<DocumentRes>()
            .map(|d| d.0.solver.max_substeps_per_frame)
            .unwrap_or(8)
            .max(1);

        app.init_resource::<JointTargets>()
            .init_resource::<HeldJoint>()
            .add_message::<SolverPanic>()
            .insert_resource(Time::<Fixed>::from_hz(hz as f64));

        // The spiral-of-death guard: clamp the frame delta before the
        // accumulator, so a stall drops simulation time instead of paying it
        // back all at once.
        let mut virt = Time::<Virtual>::default();
        virt.set_max_delta(std::time::Duration::from_secs_f64(
            substeps as f64 / hz as f64,
        ));
        app.insert_resource(virt);

        app.configure_sets(
            FixedUpdate,
            (SolveSet::Apply, SolveSet::Step, SolveSet::Guard).chain(),
        )
        .add_systems(
            FixedUpdate,
            (
                apply_targets.in_set(SolveSet::Apply),
                step_solvers.in_set(SolveSet::Step),
                guard_solvers.in_set(SolveSet::Guard),
            ),
        )
        .add_systems(
            PostUpdate,
            writeback_bones
                .in_set(WritebackSet::Bones)
                .before(TransformSystems::Propagate),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::bone_stretch;

    /// A bone at its rest length is not stretched — whatever the scale.
    ///
    /// This is the assertion that was missing. Nothing compared the
    /// writeback's transform against the bind pose it has to agree with, so a
    /// rest pose that rendered as a puppet squashed to 1% of itself passed
    /// every test in the suite.
    #[test]
    fn a_bone_at_rest_is_not_stretched_at_any_scale() {
        for ppu in [1.0, 50.0, 100.0, 512.0] {
            let rest_px = 413.0;
            let rest_world = rest_px / ppu;
            let stretch = bone_stretch(rest_world, rest_px, ppu);
            assert!(
                (stretch - 1.0).abs() < 1e-5,
                "ppu {ppu}: rest length must scale to 1.0, got {stretch}"
            );
        }
    }

    #[test]
    fn stretching_a_bone_scales_along_its_own_x() {
        let ppu = 100.0;
        let rest_px = 200.0;
        // Pulled to twice its rest length, measured in world units.
        assert!((bone_stretch(4.0, rest_px, ppu) - 2.0).abs() < 1e-5);
        // Squashed to half.
        assert!((bone_stretch(1.0, rest_px, ppu) - 0.5).abs() < 1e-5);
    }

    #[test]
    fn a_degenerate_bone_does_not_divide_by_zero() {
        assert_eq!(bone_stretch(1.0, 0.0, 100.0), 1.0);
    }
}
