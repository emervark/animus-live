//! The components that make a document entity findable, and the state that
//! makes the solver parallel.

use std::sync::Arc;

use animus_core::ids::{BoneId, LayerId, PuppetId};
use animus_core::solver::{CompiledRig, SolverState};
use bevy::prelude::*;

/// The root of one puppet. Layer transforms move this, and everything the
/// puppet owns hangs off it.
#[derive(Component, Debug, Clone, Copy)]
pub struct PuppetRoot(pub PuppetId);

/// The entity carrying `Mesh3d` + `SkinnedMesh`. Separate from the root so
/// a mesh rebuild does not have to disturb the bone entities.
#[derive(Component, Debug, Clone, Copy)]
pub struct PuppetMesh(pub PuppetId);

/// One skinning entity per bone.
///
/// `index` is the **dense** index into `CompiledRig`, which is the order
/// `SkinnedMesh::joints` and the inverse bind poses are both in. It is not
/// derived from `BoneId`, which comes from a project-wide allocator and is
/// neither 0-based nor contiguous.
#[derive(Component, Debug, Clone, Copy)]
pub struct BoneOf {
    pub puppet: PuppetId,
    pub bone: BoneId,
    pub index: u32,
}

/// The immutable solver input, shared by `Arc` so the sync system can swap
/// in a freshly compiled rig without any reader taking a lock.
#[derive(Component, Clone)]
pub struct CompiledRigRef(pub Arc<CompiledRig>);

/// The mutable solver state.
///
/// **This being a component rather than a field of a resource is what makes
/// the solver parallel**: `par_iter_mut` can hand each puppet's state to a
/// different thread, which a `HashMap` inside one resource could not.
#[derive(Component)]
pub struct PuppetSolver(pub SolverState);

/// Which layer a puppet root belongs to, so layer edits can find their
/// puppets without walking the document.
#[derive(Component, Debug, Clone, Copy)]
pub struct LayerOf(pub LayerId);

/// Editor-only visuals: gizmos, handles, helpers.
///
/// Carried together with `RenderLayers::layer(1)` so the output window's
/// camera never sees them. M0-4 confirmed the isolation works; this
/// component is what makes it greppable.
#[derive(Component, Debug, Clone, Copy)]
pub struct EditorOnly;

/// Per-vertex bone influences, kept on the CPU beside the GPU mesh.
///
/// The mesh asset itself is `RENDER_WORLD` only, so nothing can read the
/// weights back once uploaded — but the editor needs them to skin its
/// wireframe the same way the GPU skins the artwork. Without them the
/// wireframe can only be drawn at the bind pose, which puts a second,
/// stationary puppet on screen the moment anything moves.
#[derive(Component, Debug, Clone)]
pub struct MeshInfluences {
    /// Up to four bone indices per vertex, in `CompiledRig` bone order.
    pub joint_index: Vec<[u16; 4]>,
    /// The weight for each corresponding entry. Sums to 1.0 per vertex.
    pub joint_weight: Vec<[f32; 4]>,
}
