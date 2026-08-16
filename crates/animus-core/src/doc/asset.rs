//! Assets: the content-addressed files a project references.
//!
//! Asset *bytes* live on disk under `assets/<sha[0..2]>/<sha>.<ext>` (spec
//! §5); this table is just the metadata a `Project` needs to resolve an
//! `AssetId` to a file without touching disk.

use crate::ids::AssetId;
use serde::{Deserialize, Serialize};

/// A reference to an asset's bytes on disk, keyed by `AssetId` in
/// `Project::assets`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetRef {
    pub id: AssetId,
    /// Content hash; the file lives at `assets/<sha256[0..2]>/<sha256>.<ext>`.
    pub sha256: String,
    pub kind: AssetKind,
    /// The name the user imported it under. UI display only — never used
    /// to resolve the file.
    pub original_name: String,
    pub byte_len: u64,
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetKind {
    Image,
    Gltf,
    Font,
}
