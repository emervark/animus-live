//! The glTF-model puppet: an embedded animated model whose own skeleton
//! nodes are joints like any other.
//!
//! **A model's node and a mesh puppet's joint are the same thing to
//! everything above the projection layer.** Both carry a [`JointId`], both
//! appear in the rig tree, both can be selected, rotated, bound to a
//! channel and shaped by an envelope. The ids are minted at import — from
//! the glTF's own node names, read before anything is spawned — precisely so
//! that no panel, binding or sequencer track ever has to ask which kind of
//! puppet it is looking at.
//!
//! What differs is only what happens at the bottom: a mesh puppet's joints
//! are point masses the solver integrates, and a model's nodes are
//! transforms written straight onto the scene. The physics does not follow
//! the model in, because the solver is two-dimensional and a glTF skeleton
//! is not — a spring simulation run on a 3D rig projected into a plane is
//! plausible for exactly one camera angle and wrong for every other.

use crate::ids::{AssetId, JointId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelPuppet {
    pub asset: AssetId,
    #[serde(default)]
    pub scene_index: usize,
    /// The clip to play, by name. `None` leaves the model in its bind pose.
    #[serde(default)]
    pub animation: Option<String>,
    /// The model's own skeleton, with the ids this document gave it.
    ///
    /// Recorded rather than rediscovered on load: a binding refers to a
    /// `JointId`, and an id derived fresh each session from whatever order
    /// the scene happened to spawn in would point at a different limb every
    /// time the file was opened.
    #[serde(default)]
    pub nodes: Vec<ModelNode>,
    /// Kept for files written before [`ModelPuppet::nodes`] existed. Newer
    /// code reads `nodes`; this is here so an old project still loads.
    #[serde(default)]
    pub driven_joints: Vec<DrivenJoint>,
}

impl ModelPuppet {
    pub fn new(asset: AssetId) -> Self {
        Self {
            asset,
            scene_index: 0,
            animation: None,
            nodes: Vec::new(),
            driven_joints: Vec::new(),
        }
    }

    pub fn node(&self, id: JointId) -> Option<&ModelNode> {
        self.nodes.iter().find(|n| n.id == id)
    }

    pub fn node_named(&self, name: &str) -> Option<&ModelNode> {
        self.nodes.iter().find(|n| n.name == name)
    }

    /// Every node hanging below `id`, in breadth-first order.
    ///
    /// The model's counterpart to [`crate::skeleton::rig_tree`]'s
    /// `descendants`, and it lives here for the same reason that one lives in
    /// `skeleton`: **a rotation carries everything below it**, and the panel
    /// that says so, the gizmo that draws it and the projection that applies
    /// it must all be asking the same question of the same data. A node whose
    /// `parent` points back into its own chain — which a hand-edited file can
    /// express — stops the walk rather than looping forever.
    pub fn descendants(&self, id: JointId) -> Vec<JointId> {
        self.descendants_with_depth(id)
            .into_iter()
            .map(|(id, _)| id)
            .collect()
    }

    /// The same walk, keeping how far below `id` each node sits.
    ///
    /// The depth is what a hit fades by: a shove at a shoulder should reach
    /// the hand later and softer, and "how many joints away" is the only
    /// measure of that which does not depend on how long anyone drew the
    /// bones.
    pub fn descendants_with_depth(&self, id: JointId) -> Vec<(JointId, usize)> {
        let mut out: Vec<(JointId, usize)> = Vec::new();
        let mut frontier = vec![(id, 0usize)];
        while let Some((at, depth)) = frontier.pop() {
            for node in &self.nodes {
                if node.parent == Some(at)
                    && node.id != id
                    && !out.iter().any(|(seen, _)| *seen == node.id)
                {
                    out.push((node.id, depth + 1));
                    frontier.push((node.id, depth + 1));
                }
            }
        }
        out
    }
}

/// One node of the model's skeleton, and the id this document knows it by.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelNode {
    pub id: JointId,
    /// The glTF node's own name. The projection finds the spawned entity by
    /// it, so a model whose nodes are unnamed cannot be driven — which the
    /// importer says out loud rather than importing something inert.
    pub name: String,
    /// The node this one hangs from, so the rig tree can indent it exactly
    /// as it indents a mesh puppet's bones.
    #[serde(default)]
    pub parent: Option<JointId>,
}

/// One glTF skeleton node whose transform is overridden live, by name.
///
/// Superseded by [`ModelNode`] and the ordinary binding path. Kept so a
/// project written before that change still loads.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DrivenJoint {
    pub node_name: String,
    pub channel: String,
}
