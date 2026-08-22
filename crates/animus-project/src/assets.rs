//! Content-addressed asset storage.
//!
//! An asset's bytes live at `assets/<sha256[0..2]>/<sha256>.<ext>` under the
//! project root. Content addressing is what makes a project self-contained
//! and portable to a venue laptop: identical bytes imported twice cost one
//! file on disk, and `project.json` never churns because a path changed —
//! the path *is* the hash.
//!
//! **Models are the exception, and have to be.** A `.gltf` names its
//! geometry and its textures by relative path, so it is not one file but a
//! small family of them. A model therefore gets a directory of its own —
//! `assets/<sha256[0..2]>/<sha256>/<original name>` — with its references
//! copied beside it at the same relative paths they had outside. The hash is
//! still the address; what it addresses is a folder.

use crate::error::ProjectError;
use animus_core::doc::{AssetKind, AssetRef};
use animus_core::ids::AssetId;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use tracing::warn;

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

    /// Import a model and everything it refers to.
    ///
    /// The main file keeps its own name inside a directory named for its
    /// hash, and every relative `uri` it mentions — buffers and images — is
    /// copied beside it at the same relative path. That is what makes the
    /// bundle self-contained: the model resolves its own references exactly
    /// as it did in the folder it came from, because the shape of that
    /// folder came with it.
    ///
    /// **Paths that climb out are refused.** A `uri` of `../../secrets` in a
    /// downloaded model would otherwise write wherever it pleased, and a
    /// model is a file from the internet like any other.
    pub fn import_model(&mut self, src: &Path, id: AssetId) -> Result<AssetRef, ProjectError> {
        let bytes = fs::read(src)?;
        let sha256 = hex_sha256(&bytes);
        let original_name = src
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();

        let dir = model_dir(&self.root, &sha256);
        let dest = dir.join(&original_name);
        if !dest.exists() {
            fs::create_dir_all(&dir)?;
            fs::write(&dest, &bytes)?;

            let from_dir = src.parent().unwrap_or(Path::new("."));
            for rel in referenced_files(&bytes) {
                let source = from_dir.join(&rel);
                let target = dir.join(&rel);
                if !source.exists() {
                    // Named rather than swallowed: a model missing its own
                    // buffer will load as an empty scene, and "which file"
                    // is the only useful thing to know at that point.
                    warn!(
                        "{original_name} refers to {} but it is not there",
                        rel.display()
                    );
                    continue;
                }
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::copy(&source, &target)?;
            }
        }

        Ok(AssetRef {
            id,
            sha256,
            kind: AssetKind::Gltf,
            original_name,
            byte_len: bytes.len() as u64,
            width: None,
            height: None,
        })
    }

    /// The path an `AssetRef`'s bytes live (or would live) at, derived
    /// entirely from its content hash and kind.
    pub fn path_for(&self, r: &AssetRef) -> PathBuf {
        match r.kind {
            AssetKind::Gltf => model_dir(&self.root, &r.sha256).join(&r.original_name),
            _ => asset_path(&self.root, &r.sha256, r.kind),
        }
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

/// Where a model's own directory lives.
///
/// **A `.gltf` is not one file.** It names its geometry and its textures by
/// relative path, so importing only the JSON stores a model that loads to
/// nothing — the failure arrives later, as an empty scene, with the bytes
/// that would have explained it left behind on someone's desktop. A `.glb`
/// *is* one file, and gets a directory anyway so that both spellings of the
/// same format sit in the same shape.
fn model_dir(root: &Path, sha256: &str) -> PathBuf {
    let prefix = &sha256[0..2.min(sha256.len())];
    root.join("assets").join(prefix).join(sha256)
}

/// Every relative file a glTF refers to: its buffers and its images.
///
/// Read from the JSON directly rather than through a glTF library, because
/// this must work on a file the library might reject — a model that fails to
/// validate should still have its pieces gathered so the operator can be
/// told what is wrong rather than what is missing. A `.glb` carries its
/// buffers inside itself and yields nothing here, which is correct.
fn referenced_files(bytes: &[u8]) -> Vec<PathBuf> {
    let Ok(json) = serde_json::from_slice::<serde_json::Value>(bytes) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for section in ["buffers", "images"] {
        let Some(items) = json.get(section).and_then(|v| v.as_array()) else {
            continue;
        };
        for item in items {
            let Some(uri) = item.get("uri").and_then(|v| v.as_str()) else {
                continue;
            };
            // Embedded data needs no file, and a remote one is not ours to
            // fetch: a bundle that quietly reached the network to open would
            // be a bundle that stops working in a venue with no wifi.
            if uri.starts_with("data:") || uri.contains("://") {
                continue;
            }
            let decoded = percent_decode(uri);
            let rel = PathBuf::from(&decoded);
            if rel.is_absolute()
                || rel
                    .components()
                    .any(|c| matches!(c, std::path::Component::ParentDir))
            {
                warn!("ignoring a model reference that points outside its folder: {decoded}");
                continue;
            }
            if !out.contains(&rel) {
                out.push(rel);
            }
        }
    }
    out
}

/// glTF uris are percent-encoded, and a texture called `base colour.png`
/// arrives as `base%20colour.png`. Decoding by hand rather than pulling a
/// crate in for one rule.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
            if let Some(byte) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(byte);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut s = String::with_capacity(digest.len() * 2);
    for byte in digest {
        s.push_str(&format!("{byte:02x}"));
    }
    s
}
