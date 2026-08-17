//! The Bevy half: projects the document into entities, meshes and motion.
//!
//! Everything above this crate is Bevy-free. Everything here exists to turn
//! `animus_core::doc` values into something a GPU can draw, in one
//! direction only — the document is never written from the scene.
#![forbid(unsafe_code)]

pub mod coords;
pub mod skinning;

pub use coords::{img_to_world, img_to_world_angle, world_to_img};
pub use skinning::{BuildError, SkinnedMeshBuild, build_inverse_bindposes, build_skinned_mesh};
