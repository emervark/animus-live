//! Dropping an image in, and what to say when it does not work.
//!
//! The geometry is `animus_core::image_in`; the storage is
//! `animus_project::AssetStore`. What lives here is the orchestration and,
//! more importantly, **the error surface**: every way an import can fail
//! ends as one sentence in the Assets panel rather than a log line nobody
//! reads or a panic that takes the session with it.

use std::path::{Path, PathBuf};

use animus_core::doc::{
    AssetKind, ImportImage, ImportTarget, Layer, MatteParams, MeshPuppet, Project, Puppet,
    PuppetKind,
};
use animus_core::ids::{AssetId, LayerId, PuppetId};
use animus_core::image_in::{self, ImportedMesh};
use animus_project::AssetStore;
use bevy::prelude::*;

use crate::state::EditorState;

/// Where the project's assets live.
///
/// Task 13 (the binary) sets this from the file being edited. Until then it
/// points at an untitled project beside the working directory, which is
/// enough to import and rig but is not where a real show would keep its
/// files.
#[derive(Resource, Debug, Clone)]
pub struct ProjectRoot(pub PathBuf);

impl Default for ProjectRoot {
    fn default() -> Self {
        Self(
            std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join("untitled.animus"),
        )
    }
}

/// The last thing an import said, shown in the Assets panel.
///
/// One slot, not a log: the operator cares about the file they just dropped,
/// and a growing list of past failures is noise during a show.
#[derive(Resource, Debug, Clone, Default)]
pub struct ImportStatus {
    pub message: Option<String>,
    pub is_error: bool,
}

impl ImportStatus {
    pub fn ok(&mut self, msg: impl Into<String>) {
        self.message = Some(msg.into());
        self.is_error = false;
    }

    pub fn error(&mut self, msg: impl Into<String>) {
        self.message = Some(msg.into());
        self.is_error = true;
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ImportFailure {
    #[error("{0}")]
    Image(#[from] image_in::ImportError),
    #[error("could not read {0}: {1}")]
    Read(String, std::io::Error),
    #[error("could not store the image: {0}")]
    Store(#[from] animus_project::ProjectError),
}

/// Turn a file on disk into a command that adds a puppet.
///
/// Nothing is mutated here: the command is built and handed back, so the
/// caller owns when it is applied and the undo stack sees exactly one entry.
pub fn build_import(
    path: &Path,
    project: &mut Project,
    store: &mut AssetStore,
) -> Result<(ImportImage, ImportedMesh), ImportFailure> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());

    let bytes =
        std::fs::read(path).map_err(|e| ImportFailure::Read(path.display().to_string(), e))?;

    // Decode first, so an unsupported format or a fully opaque image is
    // refused *before* anything is written to the asset store. Importing
    // then failing would leave orphaned bytes on disk.
    let img = image_in::decode(&bytes, &name)?;
    let (width, height) = (img.width(), img.height());

    let matte = MatteParams::default();
    let params = image_in::starting_params();
    let imported = image_in::mesh_from_image(img, &matte, &params)?;

    let asset_id = AssetId(project.alloc_id());
    let mut asset = store.import(path, AssetKind::Image, asset_id)?;
    asset.width = Some(width);
    asset.height = Some(height);

    let mut mesh_puppet = MeshPuppet::empty(asset_id);
    mesh_puppet.matte = matte;
    mesh_puppet.mesh = imported.mesh.clone();

    let puppet = Puppet {
        id: PuppetId(project.alloc_id()),
        name: display_name(&name),
        kind: PuppetKind::Mesh(mesh_puppet),
    };

    // A new layer per import: an imported puppet that lands inside someone
    // else's layer inherits its depth and opacity, which is a surprise. The
    // layer list is where merging them is an obvious, reversible action.
    let layer = Layer::new(LayerId(project.alloc_id()), display_name(&name));

    Ok((
        ImportImage {
            asset,
            puppet,
            target: ImportTarget::NewLayer(layer),
        },
        imported,
    ))
}

/// "dancer.png" → "dancer".
fn display_name(file_name: &str) -> String {
    Path::new(file_name)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| file_name.to_string())
}

/// Handle files dropped onto the window.
pub fn handle_dropped_files(
    mut events: MessageReader<FileDragAndDrop>,
    mut doc: ResMut<animus_runtime::DocumentRes>,
    mut pending: ResMut<animus_runtime::PendingChangesRes>,
    mut state: ResMut<EditorState>,
    mut status: ResMut<ImportStatus>,
    root: Res<ProjectRoot>,
) {
    for event in events.read() {
        let FileDragAndDrop::DroppedFile { path_buf, .. } = event else {
            continue;
        };

        let mut store = AssetStore::new(&root.0);
        match build_import(path_buf, &mut doc.0, &mut store) {
            Ok((command, imported)) => {
                let label = format!(
                    "{} imported: {} vertices, {} triangles",
                    command.puppet.name,
                    imported.vertex_count(),
                    imported.triangle_count()
                );
                match animus_core::doc::apply_command(
                    &mut doc.0,
                    &mut state.undo,
                    Box::new(command),
                ) {
                    Ok(changes) => {
                        pending.extend(changes.0);
                        state.undo.break_merge();
                        status.ok(label);
                    }
                    Err(e) => status.error(format!("could not add the puppet: {e}")),
                }
            }
            // Every arm of this is a sentence naming the fix, because this
            // is the first thing most people will hit.
            Err(e) => status.error(e.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    fn write_disc_png(dir: &Path, name: &str, alpha: u8) -> PathBuf {
        let mut img = RgbaImage::from_pixel(128, 128, Rgba([0, 0, 0, 0]));
        for (x, y, px) in img.enumerate_pixels_mut() {
            let d = ((x as f32 - 64.0).powi(2) + (y as f32 - 64.0).powi(2)).sqrt();
            if d <= 40.0 {
                *px = Rgba([200, 60, 40, alpha]);
            }
        }
        let path = dir.join(name);
        img.save(&path).unwrap();
        path
    }

    #[test]
    fn a_cutout_png_becomes_a_puppet_in_its_own_layer() {
        let dir = tempfile::tempdir().unwrap();
        let png = write_disc_png(dir.path(), "dancer.png", 255);
        let root = dir.path().join("show.animus");

        let mut project = Project::new("Test");
        let mut store = AssetStore::new(&root);
        let (command, imported) = build_import(&png, &mut project, &mut store).expect("imports");

        assert_eq!(
            command.puppet.name, "dancer",
            "named after the file, not the path"
        );
        assert!(matches!(command.target, ImportTarget::NewLayer(_)));
        assert_eq!(command.asset.width, Some(128));
        assert!(imported.triangle_count() > 0);
    }

    #[test]
    fn nothing_is_written_to_the_asset_store_when_the_image_is_refused() {
        // Decoding and silhouette extraction happen before the store is
        // touched, so a refused import leaves no orphaned bytes on disk.
        let dir = tempfile::tempdir().unwrap();
        let png = write_disc_png(dir.path(), "opaque.png", 255);
        // Make it fully opaque, which the PNG-only path refuses.
        let mut img = image::open(&png).unwrap().to_rgba8();
        for px in img.pixels_mut() {
            px.0[3] = 255;
        }
        img.save(&png).unwrap();

        let root = dir.path().join("show.animus");
        let mut project = Project::new("Test");
        let mut store = AssetStore::new(&root);

        let err = build_import(&png, &mut project, &mut store).unwrap_err();
        assert!(
            err.to_string().contains("no transparency"),
            "the message must name the fix, got: {err}"
        );
        assert!(
            !root.join("assets").exists(),
            "a refused import must not leave files behind"
        );
    }

    #[test]
    fn an_unsupported_format_names_itself_and_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("dancer.jpg");
        std::fs::write(&path, b"not really a jpeg").unwrap();

        let root = dir.path().join("show.animus");
        let mut project = Project::new("Test");
        let mut store = AssetStore::new(&root);

        let err = build_import(&path, &mut project, &mut store).unwrap_err();
        assert!(err.to_string().contains("jpg"), "got: {err}");
        assert!(!root.join("assets").exists());
    }

    #[test]
    fn a_missing_file_is_an_error_not_a_panic() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("show.animus");
        let mut project = Project::new("Test");
        let mut store = AssetStore::new(&root);

        let err = build_import(&dir.path().join("nope.png"), &mut project, &mut store).unwrap_err();
        assert!(matches!(err, ImportFailure::Read(..)));
    }

    #[test]
    fn ids_come_from_the_projects_allocator_and_never_collide() {
        let dir = tempfile::tempdir().unwrap();
        let a = write_disc_png(dir.path(), "one.png", 255);
        let b = write_disc_png(dir.path(), "two.png", 200);
        let root = dir.path().join("show.animus");

        let mut project = Project::new("Test");
        let mut store = AssetStore::new(&root);
        let (first, _) = build_import(&a, &mut project, &mut store).unwrap();
        let (second, _) = build_import(&b, &mut project, &mut store).unwrap();

        assert_ne!(first.asset.id, second.asset.id);
        assert_ne!(first.puppet.id, second.puppet.id);
        match (&first.target, &second.target) {
            (ImportTarget::NewLayer(x), ImportTarget::NewLayer(y)) => assert_ne!(x.id, y.id),
            _ => panic!("both imports should create their own layer"),
        }
    }

    #[test]
    fn the_display_name_drops_the_extension_but_never_the_whole_name() {
        assert_eq!(display_name("dancer.png"), "dancer");
        assert_eq!(display_name("dancer"), "dancer");
        assert_eq!(display_name(".png"), ".png");
    }
}
