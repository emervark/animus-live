//! `Puppet`: the thing a `Layer` displays — either a rigged 2D mesh or an
//! embedded glTF model.

use crate::doc::{MeshPuppet, ModelPuppet};
use crate::ids::PuppetId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Puppet {
    pub id: PuppetId,
    pub name: String,
    pub kind: PuppetKind,
}

// `MeshPuppet` is meaningfully larger than `ModelPuppet` (it inlines the
// whole mesh/skeleton/attachment tables), but boxing it would change the
// shape later tasks match on for no real benefit: `Puppet` values live in
// `Project::puppets`, an `IndexMap` with tens of entries, not a hot
// contiguous buffer, so the size difference is never a per-frame cost.
// Revisit this allow if `PuppetKind` is ever stored in a large flat array.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PuppetKind {
    Mesh(MeshPuppet),
    Model(ModelPuppet),
}
