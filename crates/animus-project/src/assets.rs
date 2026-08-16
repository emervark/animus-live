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
use std::fs;
use std::path::{Path, PathBuf};

/// A content-addressed asset store rooted at a project directory.
///
/// `import` never writes a second copy of bytes it has already stored: if
/// `assets/<sha[0..2]>/<sha>.<ext>` already exists on disk, the file is
/// left untouched and only a fresh `AssetRef` is built and returned.
///
/// **ID authority.** `AssetStore` does not allocate `AssetId`s. A
/// `Project`'s IDs are one single monotonic sequence shared across every
/// kind of entity — layers, puppets, joints, bones, assets — via
/// `Project::alloc_id`; `Project::next_id` is the watermark that keeps an
/// ID from ever being handed out twice, even across sessions. An
/// `AssetStore` counting on its own has no way to honor that watermark, so
/// it would either collide with IDs the project already believes are
/// spent, or leave `next_id` a lie. `import` therefore takes the `AssetId`
/// as a parameter: the caller allocates it via `project.alloc_id()` (or an
/// equivalent authority) and passes it in. `AssetStore` is the *only*
/// answer to "where do these bytes live"; the `Project` remains the only
/// answer to "what ID does this get".
///
/// **Dedup is the caller's job, too.** Content addressing means importing
/// the same bytes twice is safe — it costs one file on disk regardless —
/// but if the caller allocates a fresh `AssetId` on every call, it will
/// get two distinct `AssetRef`s (different `id`s) pointing at the same
/// file. That's not corruption, but it is wasteful and confusing to
/// present in a UI. Before calling `import`, a caller that wants a single
/// `AssetRef` per distinct asset should hash the source bytes (or just
/// call `import` and inspect `sha256`) and look for an existing entry with
/// that `sha256` in `Project::assets`, reusing it instead of allocating a
/// new ID. `AssetStore` deliberately does not do this lookup itself — it
/// has no access to a `Project` and should not be given one just to
/// perform it; this is a contract for whatever code owns the import UI.
pub struct AssetStore {
    root: PathBuf,
}

impl AssetStore {
    /// Open (or prepare) the asset store rooted at `root` — a project
    /// directory such as `MyShow.animus/`. Does not require `root` or its
    /// `assets/` subdirectory to exist yet; `import` creates what it needs.
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
        }
    }

    /// Import `src`'s bytes into the store under `id`, returning an
    /// `AssetRef`. If bytes with this exact SHA-256 are already stored, no
    /// new file is written; a fresh `AssetRef` carrying `id` is still
    /// returned (dedup of *references*, as opposed to dedup of *files*, is
    /// the caller's responsibility — see the struct-level doc comment).
    pub fn import(
        &mut self,
        src: &Path,
        kind: AssetKind,
        id: AssetId,
    ) -> Result<AssetRef, ProjectError> {
        let bytes = fs::read(src)?;
        let sha256 = hex_sha256(&bytes);

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

        Ok(AssetRef {
            id,
            sha256,
            kind,
            original_name,
            byte_len: bytes.len() as u64,
            width: None,
            height: None,
        })
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
