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
    #[error("{0}")]
    Model(#[from] animus_core::gltf::GltfError),
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

/// What a dropped file is, decided by its extension.
///
/// By extension and not by sniffing the bytes: a `.gltf` is JSON and a
/// `.glb` is a container, so content-sniffing would need two probes to
/// answer a question the file name already answers. An unknown extension is
/// refused by name, which is a better error than "could not decode".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dropped {
    Image,
    Model,
}

pub fn kind_of(path: &Path) -> Option<Dropped> {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())?;
    match ext.as_str() {
        "png" | "jpg" | "jpeg" | "webp" | "tga" | "bmp" => Some(Dropped::Image),
        "gltf" | "glb" => Some(Dropped::Model),
        _ => None,
    }
}

/// Turn a dropped glTF into the same kind of command a PNG produces.
///
/// **Deliberately the same shape as [`build_import`]**: one asset, one
/// puppet, one new layer, one undoable command. A model and a cutout are
/// different things to the renderer and the same thing to the document, and
/// keeping the import paths parallel is what makes them stay that way.
pub fn build_model_import(
    path: &Path,
    project: &mut Project,
    store: &mut AssetStore,
) -> Result<(ImportImage, animus_core::gltf::ModelOutline), ImportFailure> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());

    let bytes =
        std::fs::read(path).map_err(|e| ImportFailure::Read(path.display().to_string(), e))?;

    // Read the structure first, so a model with nothing named is refused
    // before any bytes are written. Importing and then failing would leave
    // orphaned bytes in the bundle.
    let mut alloc = animus_core::ids::IdAlloc::from_next(project.next_id);
    let outline = animus_core::gltf::outline(&bytes, &mut alloc)?;
    project.next_id = alloc.peek();

    let asset_id = AssetId(project.alloc_id());
    let asset = store.import_model(path, asset_id)?;

    let mut model = animus_core::doc::ModelPuppet::new(asset_id);
    model.nodes = outline.nodes.clone();
    // The first clip, if there is one: a model that ships with an animation
    // almost always means it to be the idle, and leaving it stopped makes an
    // import look like it failed.
    model.animation = outline.animations.first().cloned();

    let puppet = Puppet {
        id: PuppetId(project.alloc_id()),
        name: display_name(&name),
        kind: PuppetKind::Model(model),
    };
    let mut layer = Layer::new(LayerId(project.alloc_id()), display_name(&name));
    layer.transform = model_placement(&outline, project.stage.canvas[1] as f32);

    Ok((
        ImportImage {
            asset,
            puppet,
            target: ImportTarget::NewLayer(layer),
        },
        outline,
    ))
}

/// How much of the stage's height a freshly imported model should fill.
///
/// Not the whole of it: a figure that touches both edges of the frame leaves
/// no room to see that it is standing on something, and an operator's first
/// act after an import is almost never "make this smaller".
///
/// A rigged model is measured by its skeleton rather than by its skin, and a
/// skeleton can read either slightly short (bones sit inside the body) or
/// slightly long (a rig often carries a ground marker and a nub above the
/// head). Neither error is large, but the margin here absorbs both.
const MODEL_FILL: f32 = 0.72;

/// Where a newly imported model sits, and how big.
///
/// **A model arrives in metres and the stage is measured in pixels.** Left
/// alone, a person-sized glTF lands eighteen percent of the height of a
/// 1080-pixel frame, off to one side of wherever its author put the origin —
/// which reads as a failed import rather than as a unit mismatch. So the
/// scale and the offset are computed once, here, from the file's own bounds,
/// and written into the layer transform where they remain an ordinary
/// editable placement rather than a hidden constant.
///
/// A file that declares no bounds is placed unscaled: a wrong guess is worse
/// than the honest original, and the layer transform is there to fix it.
fn model_placement(
    outline: &animus_core::gltf::ModelOutline,
    stage_height_px: f32,
) -> animus_core::doc::Transform2Or3 {
    use animus_core::doc::Transform2Or3;
    let ppu = animus_runtime::project::RenderScale::default().ppu;
    let stage = stage_height_px / ppu;

    let Some((min, max)) = outline.bounds else {
        return Transform2Or3::default();
    };

    let rotation = stand_up(max - min);
    // Measured *after* the correction, because the correction is what
    // decides which axis "height" means.
    let (min, max) = rotated_bounds(min, max, rotation);
    let height = max.y - min.y;
    if height <= 1e-4 {
        return Transform2Or3::default();
    }
    let scale = stage * MODEL_FILL / height;

    Transform2Or3::Spatial {
        // The model's own middle brought to the middle of the stage, after
        // scaling — the offset has to travel through the same scale, or a
        // model authored far from its origin lands off-screen.
        translation: -(min + max) * 0.5 * scale,
        rotation,
        scale: Vec3::splat(scale),
    }
}

/// The turn that stands a Z-up model upright, or nothing if it is already
/// the right way up.
///
/// **glTF says Y is up, and exporters disagree with it constantly.** Almost
/// every model converted from FBX — which is most of what a Sketchfab
/// download is — arrives Z-up, and Bevy renders it exactly as authored: face
/// down, seen from above, and fitted to the frame by its own thickness. That
/// is not a subtle wrongness an operator debugs; it is an import that looks
/// broken.
///
/// The test is deliberately narrow: **Y has to be the *smallest* of the three
/// extents.** A model that is genuinely Y-up is rarely flatter vertically
/// than it is in both horizontal directions — a standing figure certainly is
/// not, and neither is a cat, whose height beats its width even lying down.
/// A model that is flat in Y and long in Z is a model lying on its face.
fn stand_up(size: Vec3) -> Quat {
    let flat_in_y = size.y < size.x && size.y < size.z;
    if flat_in_y {
        // −90° about X: what was up the Z axis becomes up the Y axis.
        Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)
    } else {
        Quat::IDENTITY
    }
}

/// A box turned by `q`, re-measured. All eight corners, because a rotated box
/// is not described by two opposite corners of the original.
fn rotated_bounds(min: Vec3, max: Vec3, q: Quat) -> (Vec3, Vec3) {
    let mut lo = Vec3::splat(f32::INFINITY);
    let mut hi = Vec3::splat(f32::NEG_INFINITY);
    for i in 0..8 {
        let corner = Vec3::new(
            if i & 1 == 0 { min.x } else { max.x },
            if i & 2 == 0 { min.y } else { max.y },
            if i & 4 == 0 { min.z } else { max.z },
        );
        let p = q * corner;
        lo = lo.min(p);
        hi = hi.max(p);
    }
    (lo, hi)
}

/// "dancer.png" → "dancer".
fn display_name(file_name: &str) -> String {
    Path::new(file_name)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| file_name.to_string())
}

/// Decode image bytes into a GPU texture.
///
/// Shared by the drop path and the open-a-project path so a puppet cannot
/// arrive textured one way and white the other.
fn upload_texture(bytes: &[u8], name: &str, images: &mut Assets<Image>) -> Option<Handle<Image>> {
    let img = animus_core::image_in::decode(bytes, name).ok()?;
    let (w, h) = (img.width(), img.height());
    Some(images.add(Image::new(
        bevy::render::render_resource::Extent3d {
            width: w,
            height: h,
            ..default()
        },
        bevy::render::render_resource::TextureDimension::D2,
        img.into_raw(),
        bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
        bevy::asset::RenderAssetUsages::RENDER_WORLD,
    )))
}

/// Load the textures a project arrived with.
///
/// The runtime deliberately never touches the filesystem, so the handles
/// for an opened project have to be put there by someone who may — and the
/// only code that did was the drop handler. A project opened from disk
/// therefore spawned white puppets until this ran.
pub fn load_existing_textures(
    doc: Res<animus_runtime::DocumentRes>,
    mut textures: ResMut<animus_runtime::PuppetTextures>,
    mut images: ResMut<Assets<Image>>,
    mut status: ResMut<ImportStatus>,
    root: Res<ProjectRoot>,
) {
    let store = AssetStore::new(&root.0);
    let mut missing = Vec::new();
    for asset in doc.0.assets.values() {
        if !matches!(asset.kind, AssetKind::Image) {
            continue;
        }
        let path = store.path_for(asset);
        match std::fs::read(&path)
            .ok()
            .and_then(|bytes| upload_texture(&bytes, &asset.original_name, &mut images))
        {
            Some(handle) => {
                textures.0.insert(asset.id, handle);
            }
            // Named, not swallowed: a puppet with a missing texture renders
            // as a white silhouette, and "why is it white" is a question
            // that should be answered on screen rather than guessed at.
            None => missing.push(asset.original_name.clone()),
        }
    }
    if !missing.is_empty() {
        status.error(format!(
            "could not read {} image(s) from this project: {}",
            missing.len(),
            missing.join(", ")
        ));
    }
}

/// Handle files dropped onto the window.
#[allow(clippy::too_many_arguments)]
pub fn handle_dropped_files(
    mut events: MessageReader<FileDragAndDrop>,
    mut doc: ResMut<animus_runtime::DocumentRes>,
    mut pending: ResMut<animus_runtime::PendingChangesRes>,
    mut state: ResMut<EditorState>,
    mut status: ResMut<ImportStatus>,
    mut textures: ResMut<animus_runtime::PuppetTextures>,
    mut images: ResMut<Assets<Image>>,
    root: Res<ProjectRoot>,
) {
    for event in events.read() {
        let FileDragAndDrop::DroppedFile { path_buf, .. } = event else {
            continue;
        };

        let mut store = AssetStore::new(&root.0);

        if kind_of(path_buf) == Some(Dropped::Model) {
            match build_model_import(path_buf, &mut doc.0, &mut store) {
                Ok((command, outline)) => {
                    let label = format!(
                        "{} imported: {} node(s), {} clip(s)",
                        command.puppet.name,
                        outline.nodes.len(),
                        outline.animations.len()
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
                        Err(e) => status.error(format!("could not add the model: {e}")),
                    }
                }
                Err(e) => status.error(e.to_string()),
            }
            continue;
        }

        match build_import(path_buf, &mut doc.0, &mut store) {
            Ok((command, imported)) => {
                // The GPU texture, from the same bytes the mesh came from.
                // Without this the puppet spawns with an untextured white
                // material — found in the Task 15 dry run.
                if let Ok(bytes) = std::fs::read(path_buf)
                    && let Some(handle) =
                        upload_texture(&bytes, &command.asset.original_name, &mut images)
                {
                    textures.0.insert(command.asset.id, handle);
                }

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

    /// A small but real glTF, written here rather than committed, so the
    /// test exercises the format and not an exporter's habits.
    fn write_gltf(dir: &Path, name: &str) -> PathBuf {
        let doc = r#"{
          "asset": { "version": "2.0" },
          "scene": 0,
          "scenes": [ { "nodes": [0] } ],
          "nodes": [
            { "name": "root",  "children": [1] },
            { "name": "spine", "children": [2] },
            { "name": "head" }
          ],
          "animations": [
            { "name": "sway", "channels": [], "samplers": [] }
          ]
        }"#;
        let path = dir.join(name);
        std::fs::write(&path, doc).unwrap();
        path
    }

    /// **A model arrives exactly the way a cutout does**: one asset, one
    /// puppet, one new layer, one undoable command. Keeping the two import
    /// paths the same shape is what lets one rig tree and one binding path
    /// serve both.
    #[test]
    fn a_gltf_becomes_a_model_puppet_in_its_own_layer() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_gltf(dir.path(), "crow.gltf");
        let mut project = Project::new("Models");
        let mut store = AssetStore::new(dir.path());

        let (command, outline) =
            build_model_import(&path, &mut project, &mut store).expect("imports");

        assert_eq!(command.puppet.name, "crow");
        assert!(matches!(command.target, ImportTarget::NewLayer(_)));
        assert_eq!(command.asset.kind, AssetKind::Gltf);

        let PuppetKind::Model(model) = &command.puppet.kind else {
            panic!("a glTF must import as a model, not a mesh");
        };
        assert_eq!(model.nodes.len(), 3);
        assert_eq!(model.nodes[0].name, "root");
        assert_eq!(
            model.animation.as_deref(),
            Some("sway"),
            "a model that ships with a clip should not import stopped"
        );
        assert_eq!(outline.animations.len(), 1);
    }

    /// Ids come from the document, so a model imported into a project that
    /// already has puppets cannot collide with them.
    #[test]
    fn two_models_never_share_node_ids() {
        let dir = tempfile::tempdir().unwrap();
        let a = write_gltf(dir.path(), "one.gltf");
        let b = write_gltf(dir.path(), "two.gltf");
        let mut project = Project::new("Models");
        let mut store = AssetStore::new(dir.path());

        let (first, _) = build_model_import(&a, &mut project, &mut store).unwrap();
        let (second, _) = build_model_import(&b, &mut project, &mut store).unwrap();

        let (PuppetKind::Model(m1), PuppetKind::Model(m2)) =
            (&first.puppet.kind, &second.puppet.kind)
        else {
            panic!("both are models");
        };
        for node in &m1.nodes {
            assert!(
                !m2.nodes.iter().any(|n| n.id == node.id),
                "id {:?} was handed out twice",
                node.id
            );
        }
    }

    /// The store keeps the bytes, so the model still loads after a restart.
    #[test]
    fn the_model_bytes_land_in_the_bundle() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_gltf(dir.path(), "crow.gltf");
        let mut project = Project::new("Models");
        let mut store = AssetStore::new(dir.path());

        let (command, _) = build_model_import(&path, &mut project, &mut store).unwrap();
        assert!(
            store.path_for(&command.asset).exists(),
            "the bundle must hold the model, not a path to wherever it was dropped from"
        );
    }

    /// **Nothing named means nothing drivable**, and saying so beats
    /// importing something inert and leaving the operator to work out why
    /// the rig tree is empty.
    #[test]
    fn a_model_with_no_named_nodes_is_refused_before_anything_is_written() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("unnamed.gltf");
        std::fs::write(
            &path,
            r#"{ "asset": { "version": "2.0" }, "scene": 0,
                 "scenes": [ { "nodes": [0] } ], "nodes": [ { } ] }"#,
        )
        .unwrap();
        let mut project = Project::new("Models");
        let mut store = AssetStore::new(dir.path());

        let err = build_model_import(&path, &mut project, &mut store).expect_err("must refuse");
        assert!(err.to_string().contains("node names"), "{err}");
        assert!(
            !dir.path().join("assets").exists(),
            "nothing may be written when the import is refused"
        );
    }

    #[test]
    fn the_extension_decides_which_importer_runs() {
        assert_eq!(kind_of(Path::new("a/b/crow.glb")), Some(Dropped::Model));
        assert_eq!(kind_of(Path::new("a/b/crow.GLTF")), Some(Dropped::Model));
        assert_eq!(kind_of(Path::new("a/b/dancer.png")), Some(Dropped::Image));
        assert_eq!(kind_of(Path::new("a/b/notes.txt")), None);
        assert_eq!(kind_of(Path::new("a/b/noext")), None);
    }

    fn outline_of(min: Vec3, max: Vec3) -> animus_core::gltf::ModelOutline {
        animus_core::gltf::ModelOutline {
            nodes: Vec::new(),
            animations: Vec::new(),
            scenes: 1,
            bounds: Some((min, max)),
        }
    }

    fn placement(min: Vec3, max: Vec3) -> (Vec3, Quat, f32) {
        match model_placement(&outline_of(min, max), 1080.0) {
            animus_core::doc::Transform2Or3::Spatial {
                translation,
                rotation,
                scale,
            } => (translation, rotation, scale.x),
            other => panic!("a model is a spatial thing, got {other:?}"),
        }
    }

    /// **The point of the whole exercise**: a model arrives at a size worth
    /// looking at, whatever unit its author worked in. A model measured in
    /// centimetres and one measured in metres land the same height on stage.
    #[test]
    fn a_model_is_fitted_to_the_stage_whatever_unit_it_was_authored_in() {
        let stage = 1080.0 / 100.0;
        let metres = placement(Vec3::new(-0.5, 0.0, -0.5), Vec3::new(0.5, 1.8, 0.5));
        let centimetres = placement(Vec3::new(-50.0, 0.0, -50.0), Vec3::new(50.0, 180.0, 50.0));

        assert!((metres.2 * 1.8 - stage * MODEL_FILL).abs() < 1e-3, "metres");
        assert!(
            (centimetres.2 * 180.0 - stage * MODEL_FILL).abs() < 1e-3,
            "centimetres"
        );
    }

    /// A model authored with its feet at the origin is not left standing on
    /// the bottom edge of the frame: what is centred is the model, not
    /// whatever point its exporter happened to call zero.
    #[test]
    fn the_model_is_centred_rather_than_its_authors_origin() {
        let (translation, _, scale) =
            placement(Vec3::new(-0.5, 0.0, -0.5), Vec3::new(0.5, 1.8, 0.5));
        assert!(
            (translation.y + 0.9 * scale).abs() < 1e-3,
            "the middle of the figure comes to the middle of the stage: {translation:?}"
        );
        assert!(translation.x.abs() < 1e-3);
    }

    /// A Z-up export — which is most of what a converted FBX is — is stood
    /// upright, and measured after the turn rather than before it.
    #[test]
    fn a_model_lying_flat_in_y_is_stood_up() {
        let (_, rotation, scale) = placement(Vec3::new(-0.9, -0.1, 0.0), Vec3::new(0.9, 0.1, 1.8));
        assert!(
            rotation.angle_between(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)) < 1e-3,
            "turned a quarter about X: {rotation:?}"
        );
        // 1.8 was the Z extent; after standing up it is the height, and the
        // fit uses it. Measuring before the turn would have used the 0.2
        // thickness and scaled the model nine times too large.
        assert!((scale * 1.8 - 1080.0 / 100.0 * MODEL_FILL).abs() < 1e-3);
    }

    /// A model that is already the right way up is left alone. A standing
    /// figure is taller than it is deep, and turning it would be a bug an
    /// operator could only fix by hand.
    #[test]
    fn a_model_that_is_already_upright_is_not_turned() {
        let (_, rotation, _) = placement(Vec3::new(-0.9, 0.0, -0.2), Vec3::new(0.9, 1.8, 0.2));
        assert_eq!(rotation, Quat::IDENTITY);
    }

    /// So is a wide flat thing that is genuinely flat — a backdrop plane is
    /// not a figure lying down, and `stand_up` only fires when Y is the
    /// *smallest* of the three.
    #[test]
    fn a_model_wider_than_it_is_deep_is_not_turned() {
        let (_, rotation, _) = placement(Vec3::new(-5.0, -0.1, -0.05), Vec3::new(5.0, 0.1, 0.05));
        assert_eq!(rotation, Quat::IDENTITY, "Y is not the smallest here");
    }

    /// A file that says nothing about its size is placed as it is, rather
    /// than scaled by a guess. The layer transform is editable; a silent
    /// wrong number is not.
    #[test]
    fn a_model_with_no_bounds_is_left_where_it_is() {
        let mut outline = outline_of(Vec3::ZERO, Vec3::ONE);
        outline.bounds = None;
        assert!(matches!(
            model_placement(&outline, 1080.0),
            animus_core::doc::Transform2Or3::Flat {
                translation,
                rotation: 0.0,
                scale,
            } if translation == Vec2::ZERO && scale == Vec2::ONE
        ));
    }
}
