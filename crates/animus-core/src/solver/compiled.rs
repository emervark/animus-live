//! Immutable, dense compilation of a [`SkeletonData`] for the solver's
//! hot loop. Built once per rig and shared by `Arc` into the ECS.

use crate::doc::{SkeletonData, SolverConfig};
use crate::ids::JointId;
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

        for bone in skel.bones.values() {
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
            bone_a.push(ia);
            bone_b.push(ib);
            rest_length.push(rl);
            stiffness.push(bone.stiffness);
            length_mul.push(bone.length_mul);
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
        }
    }

    /// Dense index of `id`, if it exists in this compiled rig.
    pub fn joint_index(&self, id: JointId) -> Option<u32> {
        self.joint_index.get(&id).copied()
    }

    /// The target length (before `length_mul`) of bone `bone`.
    pub fn rest_length(&self, bone: usize) -> f32 {
        self.rest_length[bone]
    }

    /// Number of joints in this rig.
    pub fn joint_count(&self) -> usize {
        self.rest.len()
    }
}
