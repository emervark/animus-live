//! Projecting the document into entities, one way only.
//!
//! `DocumentRes` is the truth. Entities are its shadow. Nothing here ever
//! writes back into the document — spec §8.6 — and the only input to this
//! module is the list of [`DocChange`]s the command layer produced.
//!
//! ## Why the changes are bucketed before anything is rebuilt
//!
//! A drag emits a `JointMoved` per mouse-move, and a single frame can carry
//! a hundred of them. Handling each one as it comes would recompile the same
//! rig a hundred times. So changes are folded into at most one unit of work
//! per puppet first, and the work is graded: moving a joint recompiles the
//! rig, changing the skeleton also respawns bone entities, and only a
//! topology change rebuilds the GPU mesh.

use std::sync::Arc;

use animus_core::doc::{DocChange, MeshPuppet, Project, PuppetKind, SolverConfig, Transform2Or3};
use animus_core::ids::{AssetId, PuppetId};
use animus_core::solver::{CompiledRig, SolverState};
use bevy::camera::visibility::DynamicSkinnedMeshBounds;
use bevy::mesh::skinning::{SkinnedMesh, SkinnedMeshInverseBindposes};
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use glam::Vec2;

use crate::components::{BoneOf, CompiledRigRef, LayerOf, PuppetMesh, PuppetRoot, PuppetSolver};
use crate::index::{EntityIndex, PuppetEntities};
use crate::skinning::{BuildWarning, build_skinned_mesh};

/// The document. Systems take `Res`, never `ResMut`, except the one system
/// that applies commands.
#[derive(Resource, Debug)]
pub struct DocumentRes(pub Project);

/// Bumped whenever the document changes, so anything that caches per-puppet
/// work can tell whether its cache is still good.
#[derive(Resource, Debug, Default)]
pub struct DocRevision {
    pub global: u64,
    pub per_puppet: HashMap<PuppetId, u64>,
}

impl DocRevision {
    pub fn bump(&mut self, puppet: Option<PuppetId>) {
        self.global += 1;
        if let Some(p) = puppet {
            *self.per_puppet.entry(p).or_default() += 1;
        }
    }
}

/// Changes waiting to be projected. Drained every frame by [`sync_document`].
#[derive(Resource, Debug, Default)]
pub struct PendingChangesRes(pub Vec<DocChange>);

impl PendingChangesRes {
    pub fn push(&mut self, c: DocChange) {
        self.0.push(c);
    }

    pub fn extend(&mut self, changes: impl IntoIterator<Item = DocChange>) {
        self.0.extend(changes);
    }
}

/// Texture handles by document asset id.
///
/// The runtime does not touch the filesystem — the editor or the app loads
/// images and puts the handles here. That keeps this crate testable with no
/// disk, and it keeps asset loading policy out of the projection.
#[derive(Resource, Debug, Default)]
pub struct PuppetTextures(pub HashMap<AssetId, Handle<Image>>);

/// Pixels per world unit. One number for the whole show: puppets authored
/// at different image resolutions should still sit at a predictable scale
/// relative to each other.
#[derive(Resource, Debug, Clone, Copy)]
pub struct RenderScale {
    pub ppu: f32,
}

impl Default for RenderScale {
    fn default() -> Self {
        Self { ppu: 100.0 }
    }
}

/// Warnings raised while building puppets, for the editor to show. Replaced
/// wholesale each time a puppet is rebuilt, never appended to forever.
#[derive(Resource, Debug, Default)]
pub struct BuildWarnings(pub HashMap<PuppetId, Vec<BuildWarning>>);

/// How much work one puppet needs. Ordered: a later variant subsumes the
/// earlier ones, so folding a frame's changes together is a `max`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Lifecycle {
    Spawn,
    Despawn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Work {
    /// Recompile the rig and the bind poses. Joint rest positions moved, so
    /// the bone frames moved with them — but the mesh did not.
    Rig,
    /// The above, plus respawn bone entities: bones may have appeared or
    /// gone, and `SkinnedMesh::joints` must match the new dense order.
    Skeleton,
    /// Everything, including the GPU mesh. Topology changed.
    Full,
}

/// The pivot: the centre of the mesh's image-space bounding box.
///
/// Predictable beats clever here — a puppet's origin should not move because
/// someone deleted a vertex on one edge, and centre-of-bbox is the choice an
/// artist can predict without being told.
pub fn puppet_pivot(mp: &MeshPuppet) -> Vec2 {
    if mp.mesh.positions.is_empty() {
        return Vec2::ZERO;
    }
    let mut min = mp.mesh.positions[0];
    let mut max = mp.mesh.positions[0];
    for p in &mp.mesh.positions {
        min = min.min(*p);
        max = max.max(*p);
    }
    (min + max) * 0.5
}

fn mesh_puppet(doc: &Project, id: PuppetId) -> Option<&MeshPuppet> {
    match &doc.puppets.get(&id)?.kind {
        PuppetKind::Mesh(m) => Some(m),
        PuppetKind::Model(_) => None,
    }
}

fn solver_config<'a>(doc: &'a Project, mp: &'a MeshPuppet) -> &'a SolverConfig {
    mp.solver_override.as_ref().unwrap_or(&doc.solver)
}

fn layer_of(doc: &Project, id: PuppetId) -> Option<animus_core::ids::LayerId> {
    doc.layer_data
        .iter()
        .find(|(_, l)| l.contents.contains(&id))
        .map(|(lid, _)| *lid)
}

fn root_transform(doc: &Project, id: PuppetId) -> Transform {
    // `layer.depth` is authoritative world Z (spec §7.4), which is what lets
    // 2D layers interleave with 3D models later without a second concept.
    //
    // The layer's own transform rides on top of it. It was in the document
    // and read by nothing until now, which meant a puppet could only ever sit
    // at the origin: there was no way to place one at the edge of the stage so
    // the audience sees half of it.
    let Some(layer) = layer_of(doc, id).and_then(|lid| doc.layer_data.get(&lid)) else {
        return Transform::IDENTITY;
    };
    match layer.transform {
        Transform2Or3::Flat {
            translation,
            rotation,
            scale,
        } => Transform {
            translation: Vec3::new(translation.x, translation.y, layer.depth),
            rotation: Quat::from_rotation_z(rotation),
            scale: Vec3::new(scale.x, scale.y, 1.0),
        },
        // A spatial layer carries its own Z, but depth stays authoritative
        // for paint order, so it overrides.
        Transform2Or3::Spatial {
            translation,
            rotation,
            scale,
        } => Transform {
            translation: Vec3::new(translation.x, translation.y, layer.depth),
            rotation,
            scale,
        },
    }
}

/// Drains [`PendingChangesRes`] and makes the world match the document.
#[allow(clippy::too_many_arguments)]
pub fn sync_document(
    mut commands: Commands,
    doc: Res<DocumentRes>,
    mut pending: ResMut<PendingChangesRes>,
    mut index: ResMut<EntityIndex>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut bindposes: ResMut<Assets<SkinnedMeshInverseBindposes>>,
    mut warnings: ResMut<BuildWarnings>,
    textures: Option<Res<PuppetTextures>>,
    scale: Res<RenderScale>,
) {
    if pending.0.is_empty() {
        return;
    }

    // Changes are a *sequence*, not a set: "added then removed" means gone,
    // and "removed then added" means present. Bucketing without keeping that
    // order is how a puppet survives its own deletion.
    let mut lifecycle: HashMap<PuppetId, Lifecycle> = HashMap::default();
    let mut work: HashMap<PuppetId, Work> = HashMap::default();

    for change in pending.0.drain(..) {
        match change {
            DocChange::PuppetAdded(id) => {
                lifecycle.insert(id, Lifecycle::Spawn);
                work.remove(&id); // a fresh spawn subsumes any earlier edit
            }
            DocChange::PuppetRemoved(id) => {
                lifecycle.insert(id, Lifecycle::Despawn);
                work.remove(&id);
            }
            DocChange::MeshRebuilt(id) => raise(&mut work, id, Work::Full),
            DocChange::SkeletonChanged(id) => raise(&mut work, id, Work::Skeleton),
            DocChange::JointMoved(id, _) => raise(&mut work, id, Work::Rig),
            DocChange::MaterialChanged(id) => raise(&mut work, id, Work::Full),
            // Gravity, damping and iteration count are compiled *into*
            // `CompiledRig`, so a solver change is a rig rebuild for every
            // puppet. Left in the arm below, the document said one thing and
            // the springs kept doing another: gravity was editable, saved,
            // and read by nothing after startup.
            DocChange::SolverConfigChanged => {
                for id in doc.0.puppets.keys() {
                    raise(&mut work, *id, Work::Rig);
                }
            }
            DocChange::LayerPropsChanged(_)
            | DocChange::LayerOrderChanged
            | DocChange::LayerAdded(_)
            | DocChange::LayerRemoved(_) => {
                // Not per-puppet. Root transforms are re-derived at the end,
                // which is the whole cost of a layer edit.
            }
        }
    }

    for (id, state) in lifecycle {
        // Despawn either way: `Spawn` for a puppet that already has entities
        // is a respawn, and leaving the old ones would double it.
        despawn_puppet(&mut commands, &mut index, id);
        match state {
            Lifecycle::Despawn => {
                warnings.0.remove(&id);
                work.remove(&id);
            }
            Lifecycle::Spawn => {
                work.remove(&id);
                spawn_puppet(
                    &mut commands,
                    &doc.0,
                    id,
                    &mut index,
                    &mut meshes,
                    &mut materials,
                    &mut bindposes,
                    &mut warnings,
                    textures.as_deref(),
                    &scale,
                );
            }
        }
    }

    for (id, level) in work {
        if index.get(id).is_none() {
            continue; // edited then removed in the same frame
        }
        match level {
            Work::Full => {
                despawn_puppet(&mut commands, &mut index, id);
                spawn_puppet(
                    &mut commands,
                    &doc.0,
                    id,
                    &mut index,
                    &mut meshes,
                    &mut materials,
                    &mut bindposes,
                    &mut warnings,
                    textures.as_deref(),
                    &scale,
                );
            }
            Work::Skeleton => {
                despawn_puppet(&mut commands, &mut index, id);
                spawn_puppet(
                    &mut commands,
                    &doc.0,
                    id,
                    &mut index,
                    &mut meshes,
                    &mut materials,
                    &mut bindposes,
                    &mut warnings,
                    textures.as_deref(),
                    &scale,
                );
            }
            Work::Rig => rebuild_rig(&mut commands, &doc.0, id, &index, &mut bindposes, &scale),
        }
    }

    // Root transforms are cheap and depend on layer depth, which any layer
    // change can move. Visibility rides along for the same reason: it lives on
    // the layer, and any layer change can flip it.
    for (id, ents) in index.puppets.iter() {
        commands
            .entity(ents.root)
            .insert(root_transform(&doc.0, *id))
            .insert(layer_visibility(&doc.0, *id));
    }
}

/// A puppet is visible when the layer it stands on is.
///
/// **Hidden means hidden everywhere**, projector included. The alternative —
/// dimming the row in the panel and leaving the artwork on stage — is the
/// worst of both: the operator believes the audience cannot see it.
///
/// `Hidden` rather than removing the entity: the solver keeps running, the rig
/// keeps its state, and showing it again is one frame rather than a respawn.
fn layer_visibility(doc: &Project, id: PuppetId) -> Visibility {
    let visible = layer_of(doc, id)
        .and_then(|lid| doc.layer_data.get(&lid))
        .map(|l| l.visible)
        .unwrap_or(true);
    if visible {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    }
}

fn raise(work: &mut HashMap<PuppetId, Work>, id: PuppetId, level: Work) {
    let slot = work.entry(id).or_insert(level);
    if level > *slot {
        *slot = level;
    }
}

/// Despawn a puppet: the root, and with it everything parented to it.
///
/// The root only. The mesh and every bone are spawned `ChildOf(root)` and
/// `despawn` takes descendants with it, so despawning them again by id is
/// not belt-and-braces — it is a second despawn of an entity that is already
/// gone, and Bevy logs one error per bone for it. A rig edit on a
/// seven-bone puppet printed seven, on every edit.
fn despawn_puppet(commands: &mut Commands, index: &mut EntityIndex, id: PuppetId) {
    if let Some(ents) = index.puppets.remove(&id) {
        commands.entity(ents.root).despawn();
    }
}

/// Recompile the rig and bind poses without touching entities or the mesh.
/// The path a joint drag takes.
fn rebuild_rig(
    commands: &mut Commands,
    doc: &Project,
    id: PuppetId,
    index: &EntityIndex,
    bindposes: &mut Assets<SkinnedMeshInverseBindposes>,
    scale: &RenderScale,
) {
    let Some(mp) = mesh_puppet(doc, id) else {
        return;
    };
    let Some(ents) = index.get(id) else { return };

    let cfg = *solver_config(doc, mp);
    let rig = Arc::new(CompiledRig::build(&mp.skeleton, &cfg));
    let pivot = puppet_pivot(mp);
    let inv = crate::skinning::build_inverse_bindposes(&rig, scale.ppu, pivot);
    let rests = crate::skinning::bone_rest_transforms(&rig, scale.ppu, pivot);

    let handle = bindposes.add(SkinnedMeshInverseBindposes::from(inv));
    commands.entity(ents.mesh).insert(SkinnedMesh {
        inverse_bindposes: handle,
        joints: ents.bones.clone(),
    });

    for (i, e) in ents.bones.iter().enumerate() {
        if let Some(t) = rests.get(i) {
            commands.entity(*e).insert(*t);
        }
    }

    commands.entity(ents.root).insert((
        CompiledRigRef(rig.clone()),
        PuppetSolver(SolverState::rest(&rig)),
    ));
}

#[allow(clippy::too_many_arguments)]
fn spawn_puppet(
    commands: &mut Commands,
    doc: &Project,
    id: PuppetId,
    index: &mut EntityIndex,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    bindposes: &mut Assets<SkinnedMeshInverseBindposes>,
    warnings: &mut BuildWarnings,
    textures: Option<&PuppetTextures>,
    scale: &RenderScale,
) {
    let Some(mp) = mesh_puppet(doc, id) else {
        return;
    };
    let cfg = *solver_config(doc, mp);
    let pivot = puppet_pivot(mp);

    let built = match build_skinned_mesh(mp, &cfg, scale.ppu, pivot) {
        Ok(b) => b,
        Err(e) => {
            // A malformed puppet must not take down the show: log it, skip
            // it, leave every other puppet running.
            error!("puppet {id:?} could not be built: {e}");
            return;
        }
    };
    if built.warnings.is_empty() {
        warnings.0.remove(&id);
    } else {
        warnings.0.insert(id, built.warnings.clone());
    }

    let rig = Arc::new(CompiledRig::build(&mp.skeleton, &cfg));
    let solver = SolverState::rest(&rig);

    let root = commands
        .spawn((
            PuppetRoot(id),
            CompiledRigRef(rig.clone()),
            PuppetSolver(solver),
            crate::solve::LastStepOutcome(animus_core::solver::StepOutcome::Ok),
            root_transform(doc, id),
            layer_visibility(doc, id),
        ))
        .id();
    if let Some(lid) = layer_of(doc, id) {
        commands.entity(root).insert(LayerOf(lid));
    }

    // Bones first: `SkinnedMesh::joints` needs their entities, and they must
    // be in dense rig order.
    //
    // The `BoneId` for each dense slot is resolved *through the rig*, never
    // by taking the nth key of `SkeletonData.bones`. Those two orders agree
    // right up until `CompiledRig::build` skips a bone whose endpoints are
    // missing — at which point every later slot would be labelled with its
    // neighbour's id, and the editor would select the wrong bone.
    let mut dense_ids = vec![None; built.bone_rest_transforms.len()];
    for bid in mp.skeleton.bones.keys() {
        if let Some(i) = rig.bone_index(*bid)
            && let Some(slot) = dense_ids.get_mut(i as usize)
        {
            *slot = Some(*bid);
        }
    }

    let bones: Vec<Entity> = built
        .bone_rest_transforms
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let bone_id = dense_ids[i].unwrap_or(animus_core::ids::BoneId(0));
            commands
                .spawn((
                    BoneOf {
                        puppet: id,
                        bone: bone_id,
                        index: i as u32,
                    },
                    *t,
                    ChildOf(root),
                ))
                .id()
        })
        .collect();

    let material = materials.add(StandardMaterial {
        base_color_texture: textures.and_then(|tx| tx.0.get(&mp.texture).cloned()),
        // M1 material, spec §7.4: flat, double-sided, alpha-blended.
        unlit: true,
        cull_mode: None,
        double_sided: true,
        alpha_mode: AlphaMode::Blend,
        ..default()
    });

    let inv_handle = bindposes.add(SkinnedMeshInverseBindposes::from(
        built.inverse_bindposes.clone(),
    ));

    // One batch: a mesh carrying ATTRIBUTE_JOINT_INDEX without a SkinnedMesh
    // panics at render time (bevy#22469), so these can never be split across
    // frames.
    let influences = built.influences.clone();
    let mesh_entity = commands
        .spawn((
            PuppetMesh(id),
            influences,
            Mesh3d(meshes.add(built.mesh)),
            MeshMaterial3d(material),
            Transform::default(),
            ChildOf(root),
        ))
        .id();
    // Skinning components only when the mesh was built for them. An
    // unrigged puppet renders as the flat picture it is, which is what makes
    // an imported image visible before its first bone exists.
    if built.skinned {
        commands.entity(mesh_entity).insert((
            SkinnedMesh {
                inverse_bindposes: inv_handle,
                joints: bones.clone(),
            },
            DynamicSkinnedMeshBounds,
        ));
    }

    index.puppets.insert(
        id,
        PuppetEntities {
            root,
            mesh: mesh_entity,
            bones,
        },
    );
}
