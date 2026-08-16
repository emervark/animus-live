//! Spring-solver tuning, shared by `Project::solver` and any
//! `MeshPuppet::solver_override`.

use glam::Vec2;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SolverConfig {
    pub hz: u32,
    /// Constraint-relaxation passes per step. Deliberately incomplete
    /// convergence (4..8) is the intended feel, not a shortcut to fix.
    pub iterations: u32,
    pub gravity: Vec2,
    pub global_damping: f32,
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
            max_substeps_per_frame: 8,
            enabled: true,
        }
    }
}
