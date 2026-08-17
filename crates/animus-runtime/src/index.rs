//! The document ↔ world bridge.
//!
//! Entities are a projection of the document, so something has to remember
//! which entity a `PuppetId` became. That is this. It is written only by the
//! sync system, and every entry is removed in the same frame the entities
//! are despawned — a stale `Entity` here would be handed to `commands` and
//! silently do nothing, which is the kind of bug that shows up as "the
//! puppet stopped responding" three minutes into a show.

use animus_core::ids::PuppetId;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;

/// The entities one puppet owns.
#[derive(Debug, Clone)]
pub struct PuppetEntities {
    /// Transform parent; carries `PuppetRoot`.
    pub root: Entity,
    /// Carries `Mesh3d`, `MeshMaterial3d`, `SkinnedMesh`.
    pub mesh: Entity,
    /// One per bone, in dense `CompiledRig` order. This vector **is** the
    /// order `SkinnedMesh::joints` was built with; reordering it silently
    /// re-binds every vertex to a different limb.
    pub bones: Vec<Entity>,
}

impl PuppetEntities {
    /// Root, mesh and every bone — everything to despawn.
    pub fn all(&self) -> impl Iterator<Item = Entity> + '_ {
        std::iter::once(self.root)
            .chain(std::iter::once(self.mesh))
            .chain(self.bones.iter().copied())
    }

    pub fn entity_count(&self) -> usize {
        2 + self.bones.len()
    }
}

#[derive(Resource, Debug, Default)]
pub struct EntityIndex {
    pub puppets: HashMap<PuppetId, PuppetEntities>,
}

impl EntityIndex {
    pub fn get(&self, id: PuppetId) -> Option<&PuppetEntities> {
        self.puppets.get(&id)
    }

    /// Total entities the index claims exist. The dev-build assertion in
    /// `sync` compares this against the document's own count.
    pub fn entity_count(&self) -> usize {
        self.puppets.values().map(|p| p.entity_count()).sum()
    }
}
