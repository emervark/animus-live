//! Mutable, structure-of-arrays solver state for one rig instance.

use crate::solver::CompiledRig;
use glam::Vec2;

/// Per-instance solver state. Structure-of-arrays, no per-joint structs,
/// and no allocation once [`SolverState::rest`] has sized it.
pub struct SolverState {
    pub(crate) pos: Vec<Vec2>,
    /// Verlet's previous-position history, used to derive velocity each
    /// step.
    pub(crate) prev: Vec<Vec2>,
    /// Position at the previous FIXED tick, sampled for the renderer to
    /// interpolate from. Deliberately distinct from `prev`: `prev` is the
    /// integrator's bookkeeping and can be overwritten mid-step (e.g. by a
    /// driven target), while `prev_tick` must hold exactly what the joint
    /// looked like at the end of the last completed tick. Conflating the
    /// two makes the render-side interpolation subtly wrong.
    pub(crate) prev_tick: Vec<Vec2>,
    pub(crate) target: Vec<Option<Vec2>>,
}

impl SolverState {
    /// A state with every joint at its rest position, zero velocity, and
    /// no driven targets.
    pub fn rest(rig: &CompiledRig) -> Self {
        let n = rig.joint_count();
        Self {
            pos: rig.rest.clone(),
            prev: rig.rest.clone(),
            prev_tick: rig.rest.clone(),
            target: vec![None; n],
        }
    }

    /// Reset this state to the rig's rest pose, clearing targets. Used
    /// after the solver detects a non-finite value, to recover this
    /// puppet without propagating garbage.
    pub fn reset_to_rest(&mut self, rig: &CompiledRig) {
        self.pos.copy_from_slice(&rig.rest);
        self.prev.copy_from_slice(&rig.rest);
        self.prev_tick.copy_from_slice(&rig.rest);
        self.target.iter_mut().for_each(|t| *t = None);
    }

    /// Drive joint `joint` to `pos` for subsequent steps, until cleared.
    pub fn set_target(&mut self, joint: u32, pos: Vec2) {
        self.target[joint as usize] = Some(pos);
    }

    /// Stop driving joint `joint`; it resumes free simulation.
    pub fn clear_target(&mut self, joint: u32) {
        self.target[joint as usize] = None;
    }

    /// Current positions, in dense joint-index order.
    pub fn positions(&self) -> &[Vec2] {
        &self.pos
    }

    /// Positions as of the previous fixed tick, for render-side lerp.
    pub fn prev_tick_positions(&self) -> &[Vec2] {
        &self.prev_tick
    }

    /// Move joint `i` by `d`, adjusting both `pos` and `prev` so no
    /// velocity is injected. Used by tests to set up a stretched bone
    /// without also giving it initial momentum.
    pub fn displace(&mut self, i: u32, d: Vec2) {
        let i = i as usize;
        self.pos[i] += d;
        self.prev[i] += d;
    }

    /// Force a specific, possibly non-finite, position into joint `i`
    /// without touching `prev` — used only to exercise the NaN guard.
    #[cfg(test)]
    pub fn poison_for_test(&mut self, i: u32, pos: Vec2) {
        self.pos[i as usize] = pos;
    }
}
