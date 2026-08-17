//! `build_skinned_mesh`: the point where a document becomes something a GPU
//! can draw. All of this runs headless — a `Mesh` is a plain value, so none
//! of it needs a window, an adapter, or a running `App`.
//!
//! **Every fixture uses non-contiguous, out-of-order `BoneId`s, and the bone
//! whose index matters is deliberately not the one with the lowest id.** A
//! rig whose ids run 0,1,2 passes even when code uses a `BoneId` as a dense
//! index, and that bug does not crash: vertices simply follow the wrong limb.

use animus_core::doc::*;
use animus_core::ids::*;
use animus_core::solver::CompiledRig;
use animus_runtime::skinning::{
    BuildWarning, bone_rest_transforms, build_inverse_bindposes, build_skinned_mesh,
};
use bevy::mesh::{Mesh, VertexAttributeValues};
use glam::{Vec2, Vec3, Vec4};
use indexmap::IndexMap;

const TEX: AssetId = AssetId(3);
// Out of order on purpose: the shin is inserted first but has the higher id.
const J_HIP: JointId = JointId(70);
const J_KNEE: JointId = JointId(12);
const J_FOOT: JointId = JointId(44);
const B_SHIN: BoneId = BoneId(91);
const B_THIGH: BoneId = BoneId(28);

fn joint(id: JointId, rest: Vec2) -> Joint {
    Joint {
        id,
        name: format!("j{}", id.0),
        rest,
        rest_angle: 0.0,
        inv_mass: 1.0,
        pinned: false,
    }
}

fn bone(id: BoneId, a: JointId, b: JointId) -> Bone {
    Bone {
        id,
        name: format!("b{}", id.0),
        a,
        b,
        rest_length: None,
        stiffness: 0.8,
        damping: 0.1,
        length_mul: 1.0,
        attach_radius: 30.0,
    }
}

/// A two-bone leg. Joint rest positions are chosen so the thigh runs
/// straight *down* in image space (Y down), which is straight *down* in
/// world space too once Y flips — a direction where a sign error is
/// visible rather than symmetric.
fn puppet(vertices: usize) -> MeshPuppet {
    let mut joints = IndexMap::new();
    joints.insert(J_HIP, joint(J_HIP, Vec2::new(100.0, 100.0)));
    joints.insert(J_KNEE, joint(J_KNEE, Vec2::new(100.0, 140.0)));
    joints.insert(J_FOOT, joint(J_FOOT, Vec2::new(140.0, 140.0)));

    let mut bones = IndexMap::new();
    // insertion order, which is what CompiledRig's dense order follows —
    // note it is neither sorted by id nor the order a reader would guess
    bones.insert(B_SHIN, bone(B_SHIN, J_KNEE, J_FOOT));
    bones.insert(B_THIGH, bone(B_THIGH, J_HIP, J_KNEE));

    let mut mp = MeshPuppet::empty(TEX);
    mp.skeleton = SkeletonData { joints, bones };
    mp.mesh.positions = (0..vertices)
        .map(|i| Vec2::new(100.0 + i as f32, 100.0 + i as f32))
        .collect();
    mp.mesh.uvs = (0..vertices)
        .map(|i| Vec2::new(i as f32 / vertices as f32, 0.25))
        .collect();
    mp.mesh.triangles = if vertices >= 3 { vec![0, 1, 2] } else { vec![] };
    mp
}

fn attach(mp: &mut MeshPuppet, vertex: u32, bone: BoneId, weight: f32) {
    mp.attachments.entries.push(Attachment {
        vertex,
        bone,
        weight,
        local: Vec2::ZERO,
    });
    mp.attachments.entries.sort_by_key(|a| (a.vertex, a.bone.0));
}

fn cfg() -> SolverConfig {
    SolverConfig::default()
}

// ── the attribute type ─────────────────────────────────────────────────

#[test]
fn joint_index_is_uint16x4_not_unorm16x4() {
    // `[u16; 4]` is ambiguous between these two, and the wrong one is not a
    // crash: bone indices silently become normalized floats near zero, so
    // every vertex binds to bone 0 and the puppet moves as one rigid piece.
    let mp = puppet(3);
    let built = build_skinned_mesh(&mp, &cfg(), 32.0, Vec2::ZERO).expect("build");
    let attr = built
        .mesh
        .attribute(Mesh::ATTRIBUTE_JOINT_INDEX)
        .expect("the mesh must carry joint indices");
    assert!(
        matches!(attr, VertexAttributeValues::Uint16x4(_)),
        "joint indices must be Uint16x4, got {attr:?}"
    );
}

#[test]
fn a_bone_id_is_never_used_as_a_palette_index() {
    // Vertex 0 is attached to B_THIGH, which is *second* in insertion order,
    // so its dense index is 1 — while its BoneId value is 28. If the value
    // leaked through as an index, this reads 28.
    let mut mp = puppet(3);
    attach(&mut mp, 0, B_THIGH, 1.0);

    let rig = CompiledRig::build(&mp.skeleton, &cfg());
    let dense = rig.bone_index(B_THIGH).expect("thigh is in the rig");
    assert_eq!(dense, 1, "fixture assumption: thigh is the second bone");
    assert_ne!(
        dense as u64, B_THIGH.0,
        "fixture is useless if the id happens to equal the index"
    );

    let built = build_skinned_mesh(&mp, &cfg(), 32.0, Vec2::ZERO).expect("build");
    match built.mesh.attribute(Mesh::ATTRIBUTE_JOINT_INDEX).unwrap() {
        VertexAttributeValues::Uint16x4(v) => {
            assert_eq!(
                v[0][0], dense as u16,
                "vertex 0 must address the dense index"
            );
        }
        other => panic!("wrong attribute type: {other:?}"),
    }
}

// ── coordinates ────────────────────────────────────────────────────────

#[test]
fn positions_flip_in_y_and_uvs_do_not() {
    let mp = puppet(3);
    let ppu = 10.0;
    let pivot = Vec2::new(100.0, 100.0);
    let built = build_skinned_mesh(&mp, &cfg(), ppu, pivot).expect("build");

    let pos = match built.mesh.attribute(Mesh::ATTRIBUTE_POSITION).unwrap() {
        VertexAttributeValues::Float32x3(v) => v.clone(),
        other => panic!("positions: {other:?}"),
    };
    // vertex 1 sits at image (101, 101), one pixel right and one pixel DOWN
    // of the pivot, so world Y must be negative.
    assert!((pos[1][0] - 0.1).abs() < 1e-6, "x: {}", pos[1][0]);
    assert!(
        (pos[1][1] + 0.1).abs() < 1e-6,
        "y must be negative: {}",
        pos[1][1]
    );

    let uv = match built.mesh.attribute(Mesh::ATTRIBUTE_UV_0).unwrap() {
        VertexAttributeValues::Float32x2(v) => v.clone(),
        other => panic!("uvs: {other:?}"),
    };
    for (i, got) in uv.iter().enumerate() {
        let want = mp.mesh.uvs[i];
        assert!(
            (got[0] - want.x).abs() < 1e-6 && (got[1] - want.y).abs() < 1e-6,
            "uv {i} was flipped or scaled: {got:?} vs {want:?}"
        );
    }
}

// ── bind poses ─────────────────────────────────────────────────────────

#[test]
fn there_is_exactly_one_bind_pose_per_bone() {
    let mp = puppet(3);
    let built = build_skinned_mesh(&mp, &cfg(), 32.0, Vec2::ZERO).expect("build");
    assert_eq!(built.inverse_bindposes.len(), mp.skeleton.bones.len());
    assert_eq!(built.bone_rest_transforms.len(), mp.skeleton.bones.len());
}

#[test]
fn the_inverse_bind_pose_maps_a_bone_onto_its_own_axis() {
    // This is the whole frame convention in one assertion: joint A lands on
    // the origin, and joint B lands on +X at the bone's length. If the
    // rotation were built from the wrong direction, or Y were flipped twice,
    // B would land somewhere else.
    let mp = puppet(3);
    let ppu = 10.0;
    let pivot = Vec2::ZERO;
    let rig = CompiledRig::build(&mp.skeleton, &cfg());
    let inv = build_inverse_bindposes(&rig, ppu, pivot);

    for (b, inv_b) in inv.iter().enumerate().take(rig.bone_count()) {
        let (ja, jb) = rig.bone_joints(b).unwrap();
        let a_img = rig.joint_rest(ja as usize).unwrap();
        let b_img = rig.joint_rest(jb as usize).unwrap();
        let a_world = animus_runtime::img_to_world(a_img, pivot, ppu);
        let b_world = animus_runtime::img_to_world(b_img, pivot, ppu);
        let len = (b_world - a_world).length();

        let a_local = *inv_b * Vec4::new(a_world.x, a_world.y, a_world.z, 1.0);
        let b_local = *inv_b * Vec4::new(b_world.x, b_world.y, b_world.z, 1.0);

        assert!(
            a_local.truncate().length() < 1e-4,
            "bone {b}: joint A must land on the origin, got {a_local:?}"
        );
        assert!(
            (b_local.x - len).abs() < 1e-4 && b_local.y.abs() < 1e-4,
            "bone {b}: joint B must land on +X at {len}, got {b_local:?}"
        );
    }
}

#[test]
fn bone_rest_transforms_start_at_joint_a() {
    let mp = puppet(3);
    let ppu = 20.0;
    let pivot = Vec2::new(100.0, 100.0);
    let rig = CompiledRig::build(&mp.skeleton, &cfg());
    let transforms = bone_rest_transforms(&rig, ppu, pivot);

    for (b, t) in transforms.iter().enumerate().take(rig.bone_count()) {
        let (ja, _) = rig.bone_joints(b).unwrap();
        let a_img = rig.joint_rest(ja as usize).unwrap();
        let want = animus_runtime::img_to_world(a_img, pivot, ppu);
        let got = t.translation;
        assert!(
            (got - want).length() < 1e-5,
            "bone {b} starts at {got:?}, expected joint A at {want:?}"
        );
        assert_eq!(t.scale, Vec3::ONE);
    }
}

// ── limits are warnings, not refusals ──────────────────────────────────

#[test]
fn three_hundred_bones_builds_with_a_warning() {
    // M0-1 ran 257 and 512 bones clean on two discrete GPUs: Bevy's
    // MAX_JOINTS bounds only the fallback uniform-buffer path. Refusing here
    // would block rigs that work on the hardware this tool targets.
    let mut mp = puppet(3);
    let mut joints = IndexMap::new();
    let mut bones = IndexMap::new();
    for i in 0..301u64 {
        // ids deliberately sparse
        let jid = JointId(1000 + i * 7);
        joints.insert(jid, joint(jid, Vec2::new(i as f32, 0.0)));
    }
    let jids: Vec<JointId> = joints.keys().copied().collect();
    for i in 0..300usize {
        let bid = BoneId(50_000 + i as u64 * 3);
        bones.insert(bid, bone(bid, jids[i], jids[i + 1]));
    }
    mp.skeleton = SkeletonData { joints, bones };

    let built =
        build_skinned_mesh(&mp, &cfg(), 32.0, Vec2::ZERO).expect("300 bones must build, not fail");
    assert_eq!(built.inverse_bindposes.len(), 300);
    assert!(
        built
            .warnings
            .iter()
            .any(|w| matches!(w, BuildWarning::ManyBones { count: 300, .. })),
        "it must warn: {:?}",
        built.warnings
    );
}

#[test]
fn dropping_influence_past_four_bones_is_reported() {
    let mut mp = puppet(3);
    // five influences on one vertex; the GPU takes four
    let mut joints = mp.skeleton.joints.clone();
    let mut bones = mp.skeleton.bones.clone();
    for i in 0..4u64 {
        let jid = JointId(500 + i);
        joints.insert(jid, joint(jid, Vec2::new(i as f32, 5.0)));
    }
    let extra: Vec<JointId> = (0..4).map(|i| JointId(500 + i)).collect();
    for i in 0..3usize {
        let bid = BoneId(700 + i as u64);
        bones.insert(bid, bone(bid, extra[i], extra[i + 1]));
    }
    mp.skeleton = SkeletonData { joints, bones };

    attach(&mut mp, 0, B_THIGH, 0.5);
    attach(&mut mp, 0, B_SHIN, 0.2);
    attach(&mut mp, 0, BoneId(700), 0.15);
    attach(&mut mp, 0, BoneId(701), 0.1);
    attach(&mut mp, 0, BoneId(702), 0.05);

    let built = build_skinned_mesh(&mp, &cfg(), 32.0, Vec2::ZERO).expect("build");
    assert!(
        built
            .warnings
            .iter()
            .any(|w| matches!(w, BuildWarning::DroppedInfluence { .. })),
        "a vertex lost weight and nobody said so: {:?}",
        built.warnings
    );
}

// ── malformed input is an error, not a panic ───────────────────────────

#[test]
fn mismatched_uv_count_is_an_error() {
    let mut mp = puppet(3);
    mp.mesh.uvs.pop();
    assert!(build_skinned_mesh(&mp, &cfg(), 32.0, Vec2::ZERO).is_err());
}

#[test]
fn an_out_of_range_triangle_index_is_an_error() {
    let mut mp = puppet(3);
    mp.mesh.triangles = vec![0, 1, 99];
    assert!(build_skinned_mesh(&mp, &cfg(), 32.0, Vec2::ZERO).is_err());
}

#[test]
fn a_ragged_triangle_list_is_an_error() {
    let mut mp = puppet(3);
    mp.mesh.triangles = vec![0, 1];
    assert!(build_skinned_mesh(&mp, &cfg(), 32.0, Vec2::ZERO).is_err());
}
