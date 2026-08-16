//! Which bones move which vertices, and by how much: auto-attachment
//! weighting and the fixed-size GPU skinning bake. See spec §7.2-7.3.
//!
//! **Vocabulary note.** The renderer this eventually feeds is Bevy, whose
//! skinning API calls the palette entries "joints" (`SkinnedMesh::joints`,
//! `Mesh::ATTRIBUTE_JOINT_INDEX`). In Animus Live those entries are our
//! [`Bone`](crate::doc::Bone)s, not our [`Joint`](crate::doc::Joint)s: a
//! `Joint` is a point mass in the spring graph with no meaningful
//! orientation of its own, while a `Bone` spans two joints and has a
//! frame — origin at joint A, X axis along A→B — that rotates when a limb
//! swings. [`Attachment::local`](crate::doc::Attachment::local) is
//! recorded in that frame. `BakedInfluences::joint_index` therefore holds
//! bone indices, and `MAX_SKIN_BONES` bounds bones per puppet, not
//! joints.

mod attach;
mod bake;

pub use attach::auto_attach;
pub use bake::{BakeError, BakedInfluences, MAX_INFLUENCES, MAX_SKIN_BONES, bake_influences};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::{
        Attachment, AttachmentTable, Bone, Joint, MeshData, SkeletonData, SolverConfig,
    };
    use crate::ids::{BoneId, JointId};
    use crate::solver::CompiledRig;
    use approx::assert_relative_eq;
    use glam::Vec2;

    fn joint(id: u64, pos: Vec2) -> Joint {
        Joint {
            id: JointId(id),
            name: format!("j{id}"),
            rest: pos,
            rest_angle: 0.0,
            inv_mass: 1.0,
            pinned: false,
        }
    }

    fn bone(id: u64, a: u64, b: u64, attach_radius: f32) -> Bone {
        Bone {
            id: BoneId(id),
            name: format!("b{id}"),
            a: JointId(a),
            b: JointId(b),
            rest_length: None,
            stiffness: 0.8,
            damping: 0.1,
            length_mul: 1.0,
            attach_radius,
        }
    }

    /// A single bone from `(0,0)` to `(100,0)` with `attach_radius: 30.0`,
    /// and a mesh with exactly the given vertex positions.
    fn one_bone_with_vertices(positions: &[Vec2]) -> (MeshData, SkeletonData) {
        let mesh = MeshData {
            positions: positions.to_vec(),
            ..Default::default()
        };

        let mut skel = SkeletonData::default();
        skel.joints
            .insert(JointId(1), joint(1, Vec2::new(0.0, 0.0)));
        skel.joints
            .insert(JointId(2), joint(2, Vec2::new(100.0, 0.0)));
        skel.bones.insert(BoneId(1), bone(1, 1, 2, 30.0));

        (mesh, skel)
    }

    /// Two parallel, 10px-apart bones (`(0,0)-(100,0)` and
    /// `(0,10)-(100,10)`, both `attach_radius: 30.0`), each within reach of
    /// `vertex` — so `auto_attach` must produce more than one candidate
    /// bone for it and normalize across both.
    fn two_overlapping_bones_with_vertex(vertex: Vec2) -> (MeshData, SkeletonData) {
        let mesh = MeshData {
            positions: vec![vertex],
            ..Default::default()
        };

        let mut skel = SkeletonData::default();
        skel.joints
            .insert(JointId(1), joint(1, Vec2::new(0.0, 0.0)));
        skel.joints
            .insert(JointId(2), joint(2, Vec2::new(100.0, 0.0)));
        skel.joints
            .insert(JointId(3), joint(3, Vec2::new(0.0, 10.0)));
        skel.joints
            .insert(JointId(4), joint(4, Vec2::new(100.0, 10.0)));
        skel.bones.insert(BoneId(1), bone(1, 1, 2, 30.0));
        skel.bones.insert(BoneId(2), bone(2, 3, 4, 30.0));

        (mesh, skel)
    }

    /// A single vertex 0 with one `Attachment` per weight in `weights`,
    /// bound to bones `BoneId(0)..BoneId(weights.len())` in order — the
    /// same dense indices `rig_with_bones` gives its `CompiledRig` bones,
    /// since `bake_influences` resolves `Attachment::bone` as a bone index
    /// into `CompiledRig` directly.
    fn attachments_for_one_vertex(weights: &[f32]) -> AttachmentTable {
        let entries = weights
            .iter()
            .enumerate()
            .map(|(i, &weight)| Attachment {
                vertex: 0,
                bone: BoneId(i as u64),
                weight,
                local: Vec2::ZERO,
            })
            .collect();
        AttachmentTable { entries }
    }

    /// A `CompiledRig` with `n` bones spanning `n + 1` joints, laid end to
    /// end along the X axis. Bones get `BoneId(0)..BoneId(n)`, matching
    /// their dense index in the compiled rig (`SkeletonData.bones`
    /// insertion order).
    fn rig_with_bones(n: usize) -> CompiledRig {
        let mut skel = SkeletonData::default();
        for i in 0..=n {
            // Joint ids start at 1: 0 is the reserved "unset" sentinel
            // (see `ids::IdAlloc`).
            let jid = i as u64 + 1;
            skel.joints
                .insert(JointId(jid), joint(jid, Vec2::new(i as f32 * 10.0, 0.0)));
        }
        for i in 0..n {
            // Bone ids match their dense index in `CompiledRig` (0-based),
            // which is what `bake_influences` expects `Attachment::bone`
            // to resolve to.
            let bid = i as u64;
            skel.bones
                .insert(BoneId(bid), bone(bid, bid + 1, bid + 2, 30.0));
        }
        CompiledRig::build(&skel, &SolverConfig::default())
    }

    #[test]
    fn a_vertex_on_a_bone_gets_full_weight_from_it() {
        // bone from (0,0) to (100,0), radius 30; vertex at (50,0)
        let (mesh, skel) = one_bone_with_vertices(&[Vec2::new(50.0, 0.0)]);
        let t = auto_attach(&mesh, &skel);
        assert_eq!(t.entries.len(), 1);
        assert_relative_eq!(t.entries[0].weight, 1.0, epsilon = 1e-5);
    }

    #[test]
    fn a_vertex_beyond_the_radius_gets_no_attachment() {
        let (mesh, skel) = one_bone_with_vertices(&[Vec2::new(50.0, 500.0)]);
        let t = auto_attach(&mesh, &skel);
        assert!(t.entries.is_empty());
    }

    #[test]
    fn weights_are_normalized_per_vertex_across_bones() {
        let (mesh, skel) = two_overlapping_bones_with_vertex(Vec2::new(50.0, 5.0));
        let t = auto_attach(&mesh, &skel);
        let sum: f32 = t
            .entries
            .iter()
            .filter(|a| a.vertex == 0)
            .map(|a| a.weight)
            .sum();
        assert_relative_eq!(sum, 1.0, epsilon = 1e-5);
    }

    #[test]
    fn local_coords_are_recorded_in_each_bones_frame() {
        let (mesh, skel) = one_bone_with_vertices(&[Vec2::new(50.0, 10.0)]);
        let t = auto_attach(&mesh, &skel);
        // Bone frame origin is joint A at (0,0), X axis along the bone.
        assert_relative_eq!(t.entries[0].local.x, 50.0, epsilon = 1e-4);
        assert_relative_eq!(t.entries[0].local.y, 10.0, epsilon = 1e-4);
    }

    #[test]
    fn bake_keeps_the_top_four_influences_and_renormalizes() {
        let att = attachments_for_one_vertex(&[0.5, 0.3, 0.1, 0.05, 0.05]);
        let rig = rig_with_bones(5);
        let baked = bake_influences(&att, &rig, 1).unwrap();
        let sum: f32 = baked.joint_weight[0].iter().sum();
        assert_relative_eq!(sum, 1.0, epsilon = 1e-5);
        assert!(baked.max_dropped_mass > 0.04 && baked.max_dropped_mass < 0.06);
    }

    #[test]
    fn bake_pads_with_zeros_when_there_are_fewer_than_four_influences() {
        let att = attachments_for_one_vertex(&[1.0]);
        let rig = rig_with_bones(1);
        let baked = bake_influences(&att, &rig, 1).unwrap();
        assert_relative_eq!(baked.joint_weight[0][0], 1.0, epsilon = 1e-5);
        assert_eq!(&baked.joint_weight[0][1..], &[0.0, 0.0, 0.0]);
    }

    #[test]
    fn an_unattached_vertex_bakes_to_zero_weights_not_a_panic() {
        let att = AttachmentTable::default();
        let rig = rig_with_bones(1);
        let baked = bake_influences(&att, &rig, 3).unwrap();
        assert_eq!(baked.joint_weight.len(), 3);
        assert_eq!(baked.joint_weight[0], [0.0; 4]);
    }

    #[test]
    fn more_than_256_bones_is_a_clear_error_not_a_render_panic() {
        // Bevy's MAX_JOINTS counts entries in SkinnedMesh.joints, and per
        // spec section 7.3 those are our BONES, not our joints.
        let rig = rig_with_bones(300);
        let err = bake_influences(&AttachmentTable::default(), &rig, 1).unwrap_err();
        assert!(matches!(
            err,
            BakeError::TooManyBones {
                count: 300,
                max: 256
            }
        ));
    }

    #[test]
    fn top_four_selection_is_stable_under_equal_weights() {
        // Ties must break deterministically by bone index, or the golden
        // mesh changes between runs.
        let att = attachments_for_one_vertex(&[0.2, 0.2, 0.2, 0.2, 0.2]);
        let rig = rig_with_bones(5);
        let a = bake_influences(&att, &rig, 1).unwrap();
        let b = bake_influences(&att, &rig, 1).unwrap();
        assert_eq!(a.joint_index, b.joint_index);
    }
}
