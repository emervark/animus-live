//! Mesh editing and integrity checks. See spec §3.2.

mod edit;
mod invariants;

pub use invariants::{MeshDefect, validate};
