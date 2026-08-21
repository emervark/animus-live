//! Spring-solver tuning, shared by `Project::solver` and any
//! `MeshPuppet::solver_override`.

use glam::Vec2;
use serde::{Deserialize, Serialize};

/// A twelfth of the way home per tick: at 120Hz a released limb settles in
/// about a third of a second, which reads as a puppet with weight rather
/// than a rubber band or a corpse.
fn default_rest_pull() -> f32 {
    0.08
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SolverConfig {
    pub hz: u32,
    /// Constraint-relaxation passes per step. Deliberately incomplete
    /// convergence (4..8) is the intended feel, not a shortcut to fix.
    pub iterations: u32,
    pub gravity: Vec2,
    pub global_damping: f32,
    /// How hard every joint is pulled back to its rest position each tick,
    /// as a fraction of the distance. **This is what makes a puppet a
    /// puppet.**
    ///
    /// Rest is the zero point: edit mode authors it, live mode departs from
    /// it, and letting go returns to it. Without this the solver only holds
    /// bone *lengths*, so a limb pulled aside simply stays aside — the pose
    /// drifts wherever it was last pushed and nothing ever springs back.
    /// The motion an operator records is that return; a recording of pulls
    /// with no return plays back as a puppet that slowly ties itself in a
    /// knot.
    ///
    /// 0.0 is the old behaviour and stays reachable: a rag doll that keeps
    /// whatever shape it was left in.
    #[serde(default = "default_rest_pull")]
    pub rest_pull: f32,
    /// Accumulator clamp: caps how many substeps a single slow frame may
    /// run, so a stall doesn't turn into a burst of catch-up simulation.
    pub max_substeps_per_frame: u32,
    pub enabled: bool,
}

impl Default for SolverConfig {
    fn default() -> Self {
        Self {
            hz: 120,
            iterations: 8,
            gravity: Vec2::ZERO,
            global_damping: 0.98,
            rest_pull: default_rest_pull(),
            max_substeps_per_frame: 8,
            enabled: true,
        }
    }
}
