//! Immutable, dense compilation of a [`SkeletonData`] for the solver's
//! hot loop. Built once per rig and shared by `Arc` into the ECS.

use crate::doc::{SkeletonData, SolverConfig};
use crate::ids::{BoneId, JointId};
use glam::Vec2;
use std::collections::HashMap;

/// Immutable, dense form of a skeleton, built once and shared by `Arc`.
///
/// Joints and bones are flattened from the `IndexMap`-based [`SkeletonData`]
/// into parallel dense arrays, in **deterministic order** (`IndexMap`
/// insertion order): the golden test depends on this, and it is what makes
/// a future `par_iter_mut` safe to introduce without changing results.
#[derive(Debug, Clone)]
pub struct CompiledRig {
    pub(crate) rest: Vec<Vec2>,
    pub(crate) inv_mass: Vec<f32>,
    pub(crate) pinned: Vec<bool>,
    pub(crate) bone_a: Vec<u32>,
    pub(crate) bone_b: Vec<u32>,
    pub(crate) rest_length: Vec<f32>,
    pub(crate) stiffness: Vec<f32>,
    pub(crate) length_mul: Vec<f32>,
    pub(crate) gravity: Vec2,
    pub(crate) damping: f32,
    pub(crate) iterations: u32,
    joint_index: HashMap<JointId, u32>,
    /// `BoneId` -> dense index into `bone_a`/`bone_b`/`rest_length`/etc.
    ///
    /// `BoneId`s are allocated from the project-wide `IdAlloc` sequence
    /// shared by every entity type, so they are neither 0-based nor dense
    /// nor contiguous for a given puppet's bones — this map, not the raw
    /// `BoneId` value, is the only correct way to find a bone's position
    /// in the dense arrays (see `bone_index`).
    bone_index: HashMap<BoneId, u32>,
}

impl CompiledRig {
    /// Flattens `skel` into dense arrays, applying `cfg`'s global tuning.
    /// A bone whose endpoints are not present in `skel.joints` is skipped
    /// (with a warning) rather than causing a panic — a document with a
    /// dangling bone reference should still load and simulate the rest of
    /// the rig.
    pub fn build(skel: &SkeletonData, cfg: &SolverConfig) -> Self {
        let mut rest = Vec::with_capacity(skel.joints.len());
        let mut inv_mass = Vec::with_capacity(skel.joints.len());
        let mut pinned = Vec::with_capacity(skel.joints.len());
        let mut joint_index = HashMap::with_capacity(skel.joints.len());

        for (idx, (id, joint)) in skel.joints.iter().enumerate() {
            rest.push(joint.rest);
            inv_mass.push(if joint.pinned { 0.0 } else { joint.inv_mass });
            pinned.push(joint.pinned);
            joint_index.insert(*id, idx as u32);
        }

        let mut bone_a = Vec::with_capacity(skel.bones.len());
        let mut bone_b = Vec::with_capacity(skel.bones.len());
        let mut rest_length = Vec::with_capacity(skel.bones.len());
        let mut stiffness = Vec::with_capacity(skel.bones.len());
        let mut length_mul = Vec::with_capacity(skel.bones.len());
        let mut bone_index = HashMap::with_capacity(skel.bones.len());

        for (id, bone) in skel.bones.iter() {
            let (Some(&ia), Some(&ib)) = (joint_index.get(&bone.a), joint_index.get(&bone.b))
            else {
                tracing::warn!(
                    bone = %bone.id,
                    a = %bone.a,
                    b = %bone.b,
                    "bone references a joint missing from the skeleton; skipping"
                );
                continue;
            };
            let rl = bone
                .rest_length
                .unwrap_or_else(|| (rest[ib as usize] - rest[ia as usize]).length());
            let dense = bone_a.len() as u32;
            bone_a.push(ia);
            bone_b.push(ib);
            rest_length.push(rl);
            stiffness.push(bone.stiffness);
            length_mul.push(bone.length_mul);
            bone_index.insert(*id, dense);
        }

        Self {
            rest,
            inv_mass,
            pinned,
            bone_a,
            bone_b,
            rest_length,
            stiffness,
            length_mul,
            gravity: cfg.gravity,
            damping: cfg.global_damping,
            iterations: cfg.iterations,
            joint_index,
            bone_index,
        }
    }

    /// Dense index of `id`, if it exists in this compiled rig.
    pub fn joint_index(&self, id: JointId) -> Option<u32> {
        self.joint_index.get(&id).copied()
    }

    /// How many bones the dense arrays hold. This is the length the GPU
    /// skinning palette must have, and the order every consumer must agree
    /// on: `BakedInfluences::joint_index` stores indices into it, and the
    /// render side's inverse bind poses and bone entities are built in the
    /// same order. Two different orders here is the failure that makes
    /// vertices follow the wrong limb without crashing.
    pub fn bone_count(&self) -> usize {
        self.bone_a.len()
    }

    /// The two joint indices bone `bone` spans, in dense joint order.
    ///
    /// Public because the render side needs it twice: once to build bind
    /// poses at rest, and once per frame to derive each bone entity's
    /// transform from the solver's joint positions.
    pub fn bone_joints(&self, bone: usize) -> Option<(u32, u32)> {
        Some((*self.bone_a.get(bone)?, *self.bone_b.get(bone)?))
    }

    /// A joint's rest position, image space, in dense joint order.
    pub fn joint_rest(&self, joint: usize) -> Option<Vec2> {
        self.rest.get(joint).copied()
    }

    /// Dense index of bone `id` into `bone_a`/`bone_b`/etc — the index
    /// `BakedInfluences::joint_index` (in `crate::skeleton`) must store,
    /// NOT the raw `BoneId` value. `BoneId`s are allocated from a
    /// project-wide sequence shared with every other entity type, so they
    /// are not dense or 0-based; only this lookup is safe to use to place
    /// a bone in the dense arrays.
    pub fn bone_index(&self, id: BoneId) -> Option<u32> {
        self.bone_index.get(&id).copied()
    }

    /// The target length (before `length_mul`) of bone `bone`.
    pub fn rest_length(&self, bone: usize) -> f32 {
        self.rest_length[bone]
    }

    /// Number of joints in this rig.
    pub(crate) fn joint_count(&self) -> usize {
        self.rest.len()
    }
}
