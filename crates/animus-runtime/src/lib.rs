//! The Bevy half: projects the document into entities, meshes and motion.
//!
//! Everything above this crate is Bevy-free. Everything here exists to turn
//! `animus_core::doc` values into something a GPU can draw, in one
//! direction only — the document is never written from the scene.
#![forbid(unsafe_code)]

pub mod components;
pub mod coords;
pub mod index;
pub mod plugin;
pub mod project;
pub mod sequencer;
pub mod skinning;
pub mod solve;

pub use components::{
    BoneOf, CompiledRigRef, EditorOnly, LayerOf, MeshInfluences, PuppetMesh, PuppetRoot,
    PuppetSolver,
};
pub use coords::{img_to_world, img_to_world_angle, world_to_img};
pub use index::{EntityIndex, PuppetEntities};
pub use plugin::{RuntimePlugin, SyncSet};
pub use project::{
    BuildWarnings, DocRevision, DocumentRes, PendingChangesRes, PuppetTextures, RenderScale,
    puppet_pivot, sync_document,
};
pub use sequencer::{
    FALLOFF, FULL, GHOST, Quantize, STEP_COUNTS, Sequencer, SequencerPlugin, SequencerSet,
    TRACK_INKS, Track, run_sequencer,
};
pub use skinning::{BuildError, SkinnedMeshBuild, build_inverse_bindposes, build_skinned_mesh};
pub use solve::{HeldJoint, JointTargets, SolveSet, SolverPanic, SolverPlugin, WritebackSet};
