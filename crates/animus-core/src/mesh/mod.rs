//! Mesh editing and integrity checks. See spec §3.2.

pub mod edit;
pub mod invariants;

pub use invariants::{MeshDefect, validate};
