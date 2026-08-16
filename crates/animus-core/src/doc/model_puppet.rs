//! The glTF-model puppet: an embedded animated model, optionally with a
//! few named joints driven live from the signal bus.

use crate::ids::AssetId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelPuppet {
    pub asset: AssetId,
    #[serde(default)]
    pub scene_index: usize,
    #[serde(default)]
    pub animation: Option<String>,
    /// glTF joints driven live from the bus.
    #[serde(default)]
    pub driven_joints: Vec<DrivenJoint>,
}

/// One glTF skeleton node whose transform is overridden live, by name.
/// Channel routing is untyped for now, matching `Project::bindings` — see
/// that field's doc comment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DrivenJoint {
    pub node_name: String,
    pub channel: String,
}
