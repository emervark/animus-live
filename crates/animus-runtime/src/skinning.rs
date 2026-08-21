//! Where the document meets Bevy's GPU skinning. Spec §7.2, §7.3.
//!
//! ## The skinning palette is per BONE, not per joint
//!
//! This is the single most misleading thing in the whole pipeline, because
//! Bevy calls the palette entries "joints" and so does the vertex attribute
//! (`ATTRIBUTE_JOINT_INDEX`). In Animus Live they are **bones**:
//!
//! - A `Joint` is a point mass. In a spring graph nothing defines which way
//!   it faces, so it has no frame to skin with.
//! - A `Bone` spans two joints and therefore *does* have a frame: origin at
//!   joint A, +X along A→B. That frame is what rotates when a limb swings,
//!   and `Attachment::local` is already recorded in it.
//!
//! So there is one skinning entity, and one inverse bindpose, per bone.
//!
//! ## `BoneId` is not a bone index
//!
//! `BoneId`s come from the project-wide `IdAlloc`, so a three-bone puppet
//! can hold `BoneId(17)`, `BoneId(23)`, `BoneId(24)`. The palette is dense
//! from 0. `animus_core::skeleton::bake_influences` does that mapping
//! through `CompiledRig::bone_index`; this module must feed the bind poses
//! **in the same dense order** or every vertex follows the wrong limb —
//! silently, without a crash.

use animus_core::doc::{MeshPuppet, SolverConfig};
use animus_core::skeleton::{BakeError, BakedInfluences, bake_influences};
use animus_core::solver::CompiledRig;
use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, Mesh, PrimitiveTopology, VertexAttributeValues};
use bevy::prelude::*;
use glam::{Mat4, Quat, Vec2, Vec3};

use crate::components::MeshInfluences;
use crate::coords::img_to_world;

#[derive(Debug, thiserror::Error)]
pub enum BuildError {
    #[error("baking skin influences failed: {0}")]
    Bake(#[from] BakeError),
    #[error("mesh has {positions} positions but {uvs} uvs; they must match")]
    UvCountMismatch { positions: usize, uvs: usize },
    #[error("triangle index {index} is out of range for {vertices} vertices")]
    TriangleOutOfRange { index: u32, vertices: usize },
    #[error("triangle index list has length {0}, which is not a multiple of 3")]
    RaggedTriangles(usize),
    #[error("generating skinned mesh bounds failed: {0}")]
    Bounds(String),
}

/// Everything the sync system needs to spawn one puppet, produced together
/// so the pieces cannot drift apart.
///
/// They must also be **spawned together**: a mesh carrying
/// `ATTRIBUTE_JOINT_INDEX` without a `SkinnedMesh` component panics at
/// render time (bevy#22469), so the caller inserts all of this in one
/// `commands` batch, never across frames.
pub struct SkinnedMeshBuild {
    pub mesh: Mesh,
    /// Whether the mesh carries joint attributes.
    ///
    /// The caller must attach `SkinnedMesh` if and only if this is true:
    /// either half without the other is a render-time panic.
    pub skinned: bool,
    /// One per bone, in `CompiledRig` bone order.
    pub inverse_bindposes: Vec<Mat4>,
    /// Bone rest transforms in puppet-local space, same order. The caller
    /// spawns one entity per entry as a child of the puppet root and a
    /// sibling of the others — the solver writes these every frame.
    pub bone_rest_transforms: Vec<Transform>,
    /// Warnings worth showing an artist. Never a reason to refuse to build.
    pub warnings: Vec<BuildWarning>,
    /// The same weights that went into the mesh, kept for the editor.
    pub influences: MeshInfluences,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BuildWarning {
    /// Bones beyond Bevy's uniform-buffer skinning path. Fine on any GPU
    /// with storage buffers — measured at 257 and 512 bones on two discrete
    /// GPUs in M0-1 — and a real limit only on hardware without them.
    ManyBones { count: usize, limit: usize },
    /// A vertex lost influence to the four-per-vertex GPU limit.
    DroppedInfluence { max_fraction: f32 },
}

/// Rest frame of one bone in puppet-local world space.
///
/// Origin at joint A, +X along A→B — the frame `Attachment::local` was
/// recorded in. `length_mul` squash/stretch appears later as a scale along
/// this frame's local X.
fn bone_rest_transform(a_img: Vec2, b_img: Vec2, pivot: Vec2, ppu: f32) -> Transform {
    let a = img_to_world(a_img, pivot, ppu);
    let b = img_to_world(b_img, pivot, ppu);
    let dir = b - a;
    Transform {
        translation: a,
        rotation: Quat::from_rotation_z(dir.y.atan2(dir.x)),
        scale: Vec3::ONE,
    }
}

/// Inverse bind poses, one per bone, in `CompiledRig` bone order.
pub fn build_inverse_bindposes(rig: &CompiledRig, ppu: f32, pivot: Vec2) -> Vec<Mat4> {
    bone_rest_transforms(rig, ppu, pivot)
        .into_iter()
        .map(|t| Mat4::from_rotation_translation(t.rotation, t.translation).inverse())
        .collect()
}

/// Bone rest transforms, one per bone, in `CompiledRig` bone order.
pub fn bone_rest_transforms(rig: &CompiledRig, ppu: f32, pivot: Vec2) -> Vec<Transform> {
    // Dense order comes from the rig, never from `SkeletonData`'s map order
    // and never from the numeric value of a `BoneId`.
    (0..rig.bone_count())
        .map(|i| {
            let (ja, jb) = rig.bone_joints(i).expect("i < bone_count");
            let a = rig
                .joint_rest(ja as usize)
                .expect("bone endpoints are joints");
            let b = rig
                .joint_rest(jb as usize)
                .expect("bone endpoints are joints");
            bone_rest_transform(a, b, pivot, ppu)
        })
        .collect()
}

/// Build the GPU mesh, the bind poses and the bone rest transforms.
pub fn build_skinned_mesh(
    mp: &MeshPuppet,
    solver: &SolverConfig,
    ppu: f32,
    pivot: Vec2,
) -> Result<SkinnedMeshBuild, BuildError> {
    let n = mp.mesh.positions.len();
    if mp.mesh.uvs.len() != n {
        return Err(BuildError::UvCountMismatch {
            positions: n,
            uvs: mp.mesh.uvs.len(),
        });
    }
    if !mp.mesh.triangles.len().is_multiple_of(3) {
        return Err(BuildError::RaggedTriangles(mp.mesh.triangles.len()));
    }
    if let Some(bad) = mp.mesh.triangles.iter().find(|i| **i as usize >= n) {
        return Err(BuildError::TriangleOutOfRange {
            index: *bad,
            vertices: n,
        });
    }

    let rig = CompiledRig::build(&mp.skeleton, solver);
    let baked: BakedInfluences = bake_influences(&mp.attachments, &rig, n)?;

    let mut warnings = Vec::new();
    if baked.exceeds_uniform_bone_limit() {
        warnings.push(BuildWarning::ManyBones {
            count: baked.bone_count,
            limit: animus_core::skeleton::UNIFORM_PATH_BONE_LIMIT,
        });
    }
    if baked.max_dropped_mass > 0.0 {
        warnings.push(BuildWarning::DroppedInfluence {
            max_fraction: baked.max_dropped_mass,
        });
    }

    let positions: Vec<[f32; 3]> = mp
        .mesh
        .positions
        .iter()
        .map(|p| img_to_world(*p, pivot, ppu).to_array())
        .collect();
    // UVs are NOT flipped: image-space v increasing downward already matches
    // wgpu. Verified by eye in M0-1 with a "TOP"-labelled texture.
    let uvs: Vec<[f32; 2]> = mp.mesh.uvs.iter().map(|uv| [uv.x, uv.y]).collect();

    // RENDER_WORLD only, per spec §7.2: once uploaded, the CPU copy is
    // dead weight. The editor's wireframe is drawn from the document, not
    // read back from the mesh asset, so nothing needs MAIN_WORLD.
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 0.0, 1.0]; n]);
    // MUST be spelled as Uint16x4. A bare `[u16; 4]` is ambiguous with
    // Unorm16x4, and picking the wrong one turns bone indices into
    // normalized floats — the mesh renders, wrongly.
    // A puppet with no bones is a picture, and a picture is not skinned.
    //
    // Attaching the joint attributes anyway gives every vertex index 0 at
    // weight 0, which the shader resolves to the origin: the artwork
    // collapses to a point and the operator sees nothing until they place
    // their first bone. Worse, a mesh carrying `ATTRIBUTE_JOINT_INDEX`
    // without a matching `SkinnedMesh` panics at render time (bevy#22469),
    // so the two decisions have to be made together — here and at the spawn.
    let skinned = rig.bone_count() > 0;
    if skinned {
        mesh.insert_attribute(
            Mesh::ATTRIBUTE_JOINT_INDEX,
            VertexAttributeValues::Uint16x4(baked.joint_index.clone()),
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT, baked.joint_weight.clone());
    }
    mesh.insert_indices(Indices::U32(mp.mesh.triangles.clone()));

    // Without this a skinned mesh is culled against its rest-pose bounds:
    // measured in M0-1 at 28.3% of frames wrongly discarded once the mesh
    // nears a view edge, against 0.1% with it.
    let mesh = if skinned {
        mesh.with_generated_skinned_mesh_bounds()
            .map_err(|e| BuildError::Bounds(format!("{e:?}")))?
    } else {
        // An unskinned mesh never leaves its bind pose, so Bevy's own
        // bind-pose bounds are exactly right.
        mesh
    };

    Ok(SkinnedMeshBuild {
        mesh,
        skinned,
        influences: MeshInfluences {
            joint_index: baked.joint_index.clone(),
            joint_weight: baked.joint_weight.clone(),
        },
        inverse_bindposes: build_inverse_bindposes(&rig, ppu, pivot),
        bone_rest_transforms: bone_rest_transforms(&rig, ppu, pivot),
        warnings,
    })
}
