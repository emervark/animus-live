//! The per-puppet non-finite guard.
//!
//! A single NaN or Inf in one puppet's solver state must never propagate
//! into the rest of the show: it's reset to its rest pose and simulation
//! continues for everyone else. This is a per-instance guard, not a
//! global one — one performer's puppet glitching out doesn't take down
//! the stage.

use glam::Vec2;

/// True if every position is finite.
pub(crate) fn all_finite(pos: &[Vec2]) -> bool {
    pos.iter().all(|p| p.is_finite())
}
