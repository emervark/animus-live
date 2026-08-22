//! Letting Bevy read assets out of the open project.
//!
//! Images never needed this: they are read as bytes and handed straight to
//! `Assets<Image>`, because a PNG is one file with no dependencies. A glTF
//! is not — it pulls in buffers, textures and materials by relative path —
//! so it has to go through the asset server, and the asset server reads
//! through *sources*.
//!
//! ## Why the root is behind a lock
//!
//! Bevy builds its asset sources once, when the plugin is added, and a
//! source's root is fixed from then on. But the project root is not fixed:
//! **Open Project changes it mid-session.** A source pinned to whatever
//! project happened to be on the command line would quietly keep reading
//! from the old bundle, and the failure would look like a model that
//! renders as the wrong model rather than like a path bug.
//!
//! So the reader holds a shared root and resolves against it per read.

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use bevy::asset::io::{
    AssetReader, AssetReaderError, AssetSourceBuilder, AssetSourceId, PathStream, Reader, VecReader,
};
use bevy::prelude::*;

/// The scheme Animus assets are addressed under: `animus://ab/<sha>.glb`.
pub const SOURCE: &str = "animus";

/// The open project's directory, shared with the asset reader.
///
/// A resource as well as a handle inside the reader, so the editor can set
/// it on Open without reaching into asset internals.
#[derive(Resource, Debug, Clone, Default)]
pub struct ProjectAssetRoot(pub Arc<RwLock<PathBuf>>);

impl ProjectAssetRoot {
    pub fn set(&self, root: impl Into<PathBuf>) {
        if let Ok(mut guard) = self.0.write() {
            *guard = root.into();
        }
    }

    pub fn get(&self) -> PathBuf {
        self.0.read().map(|g| g.clone()).unwrap_or_default()
    }
}

struct ProjectReader {
    root: Arc<RwLock<PathBuf>>,
}

impl ProjectReader {
    fn resolve(&self, path: &Path) -> PathBuf {
        self.root
            .read()
            .map(|g| g.clone())
            .unwrap_or_default()
            .join(path)
    }
}

impl AssetReader for ProjectReader {
    /// **The whole file, in memory, every time.**
    ///
    /// Delegating to Bevy's own `FileAssetReader` would have been the
    /// obvious move and does not work: the reader it returns borrows the
    /// reader that made it, and this one is constructed per call because
    /// its root can change. Reading the bytes outright sidesteps that
    /// entirely, and the sizes involved — a model, a texture — are ones the
    /// importer already reads whole anyway.
    async fn read<'a>(&'a self, path: &'a Path) -> Result<impl Reader + 'a, AssetReaderError> {
        let full = self.resolve(path);
        match std::fs::read(&full) {
            Ok(bytes) => Ok(VecReader::new(bytes)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                Err(AssetReaderError::NotFound(full))
            }
            Err(e) => Err(AssetReaderError::Io(e.into())),
        }
    }

    /// Assets in a bundle carry no `.meta` sidecar, and `NotFound` is how
    /// that is spelled — Bevy treats it as "use the defaults" rather than
    /// as a failure.
    async fn read_meta<'a>(&'a self, path: &'a Path) -> Result<impl Reader + 'a, AssetReaderError> {
        let full = self.resolve(path);
        match std::fs::read(full.with_extension("meta")) {
            Ok(bytes) => Ok(VecReader::new(bytes)),
            Err(_) => Err(AssetReaderError::NotFound(full)),
        }
    }

    /// Not supported, and it does not need to be: nothing loads a folder
    /// out of a bundle. Assets are addressed by content hash, so there is
    /// never a directory to enumerate — saying so beats implementing a
    /// stream that would only ever be wrong.
    async fn read_directory<'a>(
        &'a self,
        path: &'a Path,
    ) -> Result<Box<PathStream>, AssetReaderError> {
        Err(AssetReaderError::NotFound(self.resolve(path)))
    }

    async fn is_directory<'a>(&'a self, path: &'a Path) -> Result<bool, AssetReaderError> {
        Ok(self.resolve(path).is_dir())
    }
}

/// Register the source. **Must run before `AssetPlugin`**, which is to say
/// before `DefaultPlugins`, because Bevy builds its sources once.
pub fn register(app: &mut App) -> ProjectAssetRoot {
    let root = ProjectAssetRoot::default();
    let shared = root.0.clone();
    app.register_asset_source(
        AssetSourceId::from(SOURCE),
        AssetSourceBuilder::new(move || {
            Box::new(ProjectReader {
                root: shared.clone(),
            })
        }),
    );
    root
}

/// Where an asset's bytes live, as an asset path Bevy can load.
///
/// The same layout the bundle uses, spelled once here so the loader and the
/// store cannot drift: a two-character prefix directory, then the hash.
pub fn asset_uri(asset: &animus_core::doc::AssetRef, sub_asset: &str) -> String {
    let prefix = &asset.sha256[0..2.min(asset.sha256.len())];
    match asset.kind {
        // **A model keeps its own name inside a directory of its hash**,
        // because a `.gltf` names its buffers and textures by relative path
        // and only resolves if that shape came with it. The store writes it
        // that way; this reads it back the same way, and the two must not
        // be allowed to drift.
        animus_core::doc::AssetKind::Gltf => format!(
            "{SOURCE}://assets/{prefix}/{}/{}{sub_asset}",
            asset.sha256, asset.original_name
        ),
        animus_core::doc::AssetKind::Image => {
            format!("{SOURCE}://assets/{prefix}/{}.png{sub_asset}", asset.sha256)
        }
        animus_core::doc::AssetKind::Font => {
            format!("{SOURCE}://assets/{prefix}/{}.ttf{sub_asset}", asset.sha256)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use animus_core::doc::{AssetKind, AssetRef};
    use animus_core::ids::AssetId;

    fn asset(kind: AssetKind) -> AssetRef {
        AssetRef {
            id: AssetId(1),
            sha256: "abcdef0123456789".into(),
            kind,
            original_name: "thing".into(),
            byte_len: 0,
            width: None,
            height: None,
        }
    }

    /// **The path must match the bundle's own layout**, or a model imports
    /// successfully and then fails to load with nothing on screen to say
    /// the two disagreed about where it went.
    #[test]
    fn the_uri_matches_the_stores_prefix_layout() {
        let uri = asset_uri(&asset(AssetKind::Gltf), "#Scene0");
        assert_eq!(
            uri, "animus://assets/ab/abcdef0123456789/thing#Scene0",
            "a model keeps its own name inside a directory of its hash, so              its relative buffers and textures still resolve"
        );
    }

    #[test]
    fn the_extension_comes_from_the_kind_not_the_original_name() {
        let uri = asset_uri(&asset(AssetKind::Image), "");
        assert!(uri.ends_with(".png"), "{uri}");
    }

    /// Open Project changes the root mid-session; a reader pinned to the
    /// startup project would keep serving the old bundle.
    #[test]
    fn the_root_can_be_changed_after_it_is_shared() {
        let root = ProjectAssetRoot::default();
        let shared = root.0.clone();
        root.set("/shows/tonight.animus");
        assert_eq!(
            shared.read().unwrap().to_string_lossy(),
            "/shows/tonight.animus"
        );
    }
}
