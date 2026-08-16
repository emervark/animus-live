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
    /// bound to `bone_ids[i]` for `weights[i]` — i.e. the caller supplies
    /// the actual `BoneId`s (e.g. from `rig_with_bones`'s return), rather
    /// than this helper inventing ones that happen to equal a dense
    /// index. `bake_influences` resolves each `Attachment::bone` through
    /// `CompiledRig::bone_index`, so the raw `BoneId` value must never be
    /// assumed to be a dense index.
    fn attachments_for_one_vertex(weights: &[f32], bone_ids: &[BoneId]) -> AttachmentTable {
        assert_eq!(weights.len(), bone_ids.len());
        let entries = weights
            .iter()
            .zip(bone_ids)
            .map(|(&weight, &bone)| Attachment {
                vertex: 0,
                bone,
                weight,
                local: Vec2::ZERO,
            })
            .collect();
        AttachmentTable { entries }
    }

    /// A `CompiledRig` with `n` bones spanning `n + 1` joints, laid end to
    /// end along the X axis, plus the `BoneId`s assigned to those bones in
    /// dense-index order (`bone_ids[i]` is the bone at dense index `i`).
    ///
    /// The `BoneId`s are deliberately **not** `0..n` and deliberately
    /// **not** in ascending order with dense index (`10_000 - i`, i.e.
    /// descending): real `BoneId`s come from the project-wide `IdAlloc`
    /// sequence shared by every entity type, so they are never dense or
    /// 0-based per puppet. A `bake_influences` that (re-)treated
    /// `Attachment::bone.0` as a literal dense index would misbehave
    /// against this rig, catching the regression instead of passing by
    /// coincidence the way an earlier version of this test suite did.
    fn rig_with_bones(n: usize) -> (CompiledRig, Vec<BoneId>) {
        let mut skel = SkeletonData::default();
        for i in 0..=n {
            // Joint ids offset well clear of 0 (the reserved "unset"
            // sentinel, see `ids::IdAlloc`) and of the bone-id range below.
            let jid = 20_000 + i as u64;
            skel.joints
                .insert(JointId(jid), joint(jid, Vec2::new(i as f32 * 10.0, 0.0)));
        }
        let mut bone_ids = Vec::with_capacity(n);
        for i in 0..n {
            let bid = 10_000 - i as u64;
            let a = 20_000 + i as u64;
            let b = 20_000 + i as u64 + 1;
            skel.bones.insert(BoneId(bid), bone(bid, a, b, 30.0));
            bone_ids.push(BoneId(bid));
        }
        let rig = CompiledRig::build(&skel, &SolverConfig::default());
        (rig, bone_ids)
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
        let (rig, bone_ids) = rig_with_bones(5);
        let att = attachments_for_one_vertex(&[0.5, 0.3, 0.1, 0.05, 0.05], &bone_ids);
        let baked = bake_influences(&att, &rig, 1).unwrap();
        let sum: f32 = baked.joint_weight[0].iter().sum();
        assert_relative_eq!(sum, 1.0, epsilon = 1e-5);
        assert!(baked.max_dropped_mass > 0.04 && baked.max_dropped_mass < 0.06);
    }

    #[test]
    fn bake_pads_with_zeros_when_there_are_fewer_than_four_influences() {
        let (rig, bone_ids) = rig_with_bones(1);
        let att = attachments_for_one_vertex(&[1.0], &bone_ids);
        let baked = bake_influences(&att, &rig, 1).unwrap();
        assert_relative_eq!(baked.joint_weight[0][0], 1.0, epsilon = 1e-5);
        assert_eq!(&baked.joint_weight[0][1..], &[0.0, 0.0, 0.0]);
    }

    #[test]
    fn an_unattached_vertex_bakes_to_zero_weights_not_a_panic() {
        let att = AttachmentTable::default();
        let (rig, _bone_ids) = rig_with_bones(1);
        let baked = bake_influences(&att, &rig, 3).unwrap();
        assert_eq!(baked.joint_weight.len(), 3);
        assert_eq!(baked.joint_weight[0], [0.0; 4]);
    }

    #[test]
    fn more_than_256_bones_is_a_clear_error_not_a_render_panic() {
        // Bevy's MAX_JOINTS counts entries in SkinnedMesh.joints, and per
        // spec section 7.3 those are our BONES, not our joints.
        let (rig, _bone_ids) = rig_with_bones(300);
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
        let (rig, bone_ids) = rig_with_bones(5);
        let att = attachments_for_one_vertex(&[0.2, 0.2, 0.2, 0.2, 0.2], &bone_ids);
        let a = bake_influences(&att, &rig, 1).unwrap();
        let b = bake_influences(&att, &rig, 1).unwrap();
        assert_eq!(a.joint_index, b.joint_index);
    }

    #[test]
    fn bake_resolves_bone_ids_through_the_rigs_dense_index_not_the_raw_id() {
        // BoneIds are allocated from the project-wide IdAlloc sequence
        // (shared with layers, joints, assets, ...), so they are never
        // dense or 0-based for one puppet's bones. Build a rig whose
        // three bones have deliberately non-contiguous, out-of-order ids
        // — BoneId(17) inserted first (dense index 0), BoneId(3) second
        // (dense index 1), BoneId(29) third (dense index 2) — so that a
        // `bake_influences` that (mis)treated `Attachment::bone.0` as a
        // literal dense index would either index out of bounds or select
        // the wrong bone.
        let mut skel = SkeletonData::default();
        skel.joints
            .insert(JointId(1), joint(1, Vec2::new(0.0, 0.0)));
        skel.joints
            .insert(JointId(2), joint(2, Vec2::new(10.0, 0.0)));
        skel.joints
            .insert(JointId(3), joint(3, Vec2::new(20.0, 0.0)));
        skel.joints
            .insert(JointId(4), joint(4, Vec2::new(30.0, 0.0)));
        skel.bones.insert(BoneId(17), bone(17, 1, 2, 30.0));
        skel.bones.insert(BoneId(3), bone(3, 2, 3, 30.0));
        skel.bones.insert(BoneId(29), bone(29, 3, 4, 30.0));
        let rig = CompiledRig::build(&skel, &SolverConfig::default());

        // Sanity-check the mapping itself before relying on it below.
        assert_eq!(rig.bone_index(BoneId(17)), Some(0));
        assert_eq!(rig.bone_index(BoneId(3)), Some(1));
        assert_eq!(rig.bone_index(BoneId(29)), Some(2));

        let att = AttachmentTable {
            entries: vec![Attachment {
                vertex: 0,
                bone: BoneId(29),
                weight: 1.0,
                local: Vec2::ZERO,
            }],
        };
        let baked = bake_influences(&att, &rig, 1).unwrap();
        assert_eq!(
            baked.joint_index[0][0], 2,
            "must store BoneId(29)'s DENSE index (2), not its raw id (29) \
             or the AttachmentTable's insertion order"
        );
        assert_relative_eq!(baked.joint_weight[0][0], 1.0, epsilon = 1e-5);
    }

    #[test]
    fn bake_reports_an_attachment_naming_a_bone_the_rig_does_not_contain() {
        // An AttachmentTable and a CompiledRig built from out-of-sync
        // skeletons is a caller bug, not input to silently tolerate.
        let (rig, _bone_ids) = rig_with_bones(1);
        let att = AttachmentTable {
            entries: vec![Attachment {
                vertex: 0,
                bone: BoneId(999_999),
                weight: 1.0,
                local: Vec2::ZERO,
            }],
        };
        let err = bake_influences(&att, &rig, 1).unwrap_err();
        assert!(matches!(
            err,
            BakeError::UnknownBone {
                vertex: 0,
                bone: BoneId(999_999)
            }
        ));
    }
}
