//! Content-addressed asset storage.
//!
//! An asset's bytes live at `assets/<sha256[0..2]>/<sha256>.<ext>` under the
//! project root. Content addressing is what makes a project self-contained
//! and portable to a venue laptop: identical bytes imported twice cost one
//! file on disk, and `project.json` never churns because a path changed —
//! the path *is* the hash.

use crate::error::ProjectError;
use animus_core::doc::{AssetKind, AssetRef};
use animus_core::ids::AssetId;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// A content-addressed asset store rooted at a project directory.
///
/// `import` is idempotent by content: importing the same bytes twice never
/// writes a second copy, and returns an `AssetRef` describing the one file
/// that exists.
///
/// The `id` on a returned `AssetRef` is allocated from a counter local to
/// this `AssetStore` instance, distinct from `Project::next_id`. Content
/// addressing guarantees the same bytes always resolve to the same file
/// regardless of `id`; wiring an imported asset's `id` into a specific
/// `Project`'s ID space (so it doesn't collide with that project's other
/// entities) is the caller's job, e.g. by replacing it with
/// `project.alloc_id()` before inserting into `Project::assets`.
pub struct AssetStore {
    root: PathBuf,
    by_hash: HashMap<String, AssetRef>,
    next_id: u64,
}

impl AssetStore {
    /// Open (or prepare) the asset store rooted at `root` — a project
    /// directory such as `MyShow.animus/`. Does not require `root` or its
    /// `assets/` subdirectory to exist yet; `import` creates what it needs.
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
            by_hash: HashMap::new(),
            next_id: 1,
        }
    }

    /// Import `src`'s bytes into the store, returning an `AssetRef`. If
    /// bytes with this exact SHA-256 are already stored, no new file is
    /// written and the existing `AssetRef` is returned.
    pub fn import(&mut self, src: &Path, kind: AssetKind) -> Result<AssetRef, ProjectError> {
        let bytes = fs::read(src)?;
        let sha256 = hex_sha256(&bytes);

        if let Some(existing) = self.by_hash.get(&sha256) {
            return Ok(existing.clone());
        }

        let original_name = src
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();

        let dest = asset_path(&self.root, &sha256, kind);
        if !dest.exists() {
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&dest, &bytes)?;
        }

        let id = AssetId(self.next_id);
        self.next_id += 1;

        let asset = AssetRef {
            id,
            sha256: sha256.clone(),
            kind,
            original_name,
            byte_len: bytes.len() as u64,
            width: None,
            height: None,
        };
        self.by_hash.insert(sha256, asset.clone());
        Ok(asset)
    }

    /// The path an `AssetRef`'s bytes live (or would live) at, derived
    /// entirely from its content hash and kind.
    pub fn path_for(&self, r: &AssetRef) -> PathBuf {
        asset_path(&self.root, &r.sha256, r.kind)
    }
}

/// The extension used for a stored asset's file name. Derived from `kind`
/// alone (not from the source file's own extension) so that `import` and
/// `path_for` always agree on where a given `AssetRef` lives, even though
/// `AssetRef` itself carries no extension field.
fn ext_for_kind(kind: AssetKind) -> &'static str {
    match kind {
        AssetKind::Image => "png",
        AssetKind::Gltf => "glb",
        AssetKind::Font => "ttf",
    }
}

fn asset_path(root: &Path, sha256: &str, kind: AssetKind) -> PathBuf {
    let prefix = &sha256[0..2.min(sha256.len())];
    let file_name = format!("{sha256}.{}", ext_for_kind(kind));
    root.join("assets").join(prefix).join(file_name)
}

fn hex_sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut s = String::with_capacity(digest.len() * 2);
    for byte in digest {
        s.push_str(&format!("{byte:02x}"));
    }
    s
}
