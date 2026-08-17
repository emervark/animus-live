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
        transform.scale = if rest_len > 1e-6 {
            Vec3::new(dir.length() / rest_len, 1.0, 1.0)
        } else {
            Vec3::ONE
        };
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
