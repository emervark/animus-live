//! The `DocCommand` spine: every command inverts, gestures merge into one
//! undo step, and the stack stays inside its caps.
//!
//! These drive the public API only — if a test here needs something the
//! editor cannot reach, the API is wrong rather than the test.
//!
//! **The fixture uses deliberately non-contiguous, out-of-order IDs.** A
//! project whose `BoneId`s happen to run 0, 1, 2 passes even if code treats
//! an ID as an index, and that mistake is silent: vertices follow the wrong
//! limb rather than crashing. Spec §7.2.

use animus_core::doc::*;
use animus_core::ids::*;
use glam::Vec2;
use indexmap::IndexMap;
use proptest::prelude::*;

// ── fixture ────────────────────────────────────────────────────────────

const LAYER: LayerId = LayerId(11);
const PUPPET: PuppetId = PuppetId(23);
const TEX: AssetId = AssetId(5);
/// Non-contiguous on purpose, and not in sorted order either.
const J_HIP: JointId = JointId(41);
const J_KNEE: JointId = JointId(17);
const J_FOOT: JointId = JointId(88);
const B_THIGH: BoneId = BoneId(64);
const B_SHIN: BoneId = BoneId(29);

fn joint(id: JointId, name: &str, rest: Vec2) -> Joint {
    Joint {
        id,
        name: name.into(),
        rest,
        rest_angle: 0.0,
        inv_mass: 1.0,
        pinned: false,
    }
}

fn bone(id: BoneId, name: &str, a: JointId, b: JointId) -> Bone {
    Bone {
        id,
        name: name.into(),
        a,
        b,
        rest_length: None,
        stiffness: 0.8,
        damping: 0.1,
        length_mul: 1.0,
        attach_radius: 24.0,
    }
}

fn mesh_puppet(vertices: usize) -> Puppet {
    let mut joints = IndexMap::new();
    joints.insert(J_HIP, joint(J_HIP, "hip", Vec2::new(50.0, 70.0)));
    joints.insert(J_KNEE, joint(J_KNEE, "knee", Vec2::new(52.0, 84.0)));
    joints.insert(J_FOOT, joint(J_FOOT, "foot", Vec2::new(54.0, 96.0)));

    let mut bones = IndexMap::new();
    bones.insert(B_THIGH, bone(B_THIGH, "thigh", J_HIP, J_KNEE));
    bones.insert(B_SHIN, bone(B_SHIN, "shin", J_KNEE, J_FOOT));

    let mut mp = MeshPuppet::empty(TEX);
    mp.skeleton = SkeletonData { joints, bones };
    mp.mesh.positions = (0..vertices)
        .map(|i| Vec2::new(i as f32 * 0.5, i as f32 * 0.25))
        .collect();
    mp.mesh.uvs = mp.mesh.positions.iter().map(|_| Vec2::ZERO).collect();
    mp.mesh.triangles = (0..vertices as u32).collect();

    Puppet {
        id: PUPPET,
        name: "dancer".into(),
        kind: PuppetKind::Mesh(mp),
    }
}

fn fixture() -> Project {
    let mut p = Project::new("Test Show");
    p.next_id = 200; // above every hand-picked id above
    let mut layer = Layer::new(LAYER, "Puppets");
    layer.contents.push(PUPPET);
    p.layer_data.insert(LAYER, layer);
    p.layers.push(LAYER);
    p.puppets.insert(PUPPET, mesh_puppet(64));
    p
}

/// `Project` has no `PartialEq`, and adding one would invite comparing
/// documents by accident in hot code. Comparing the serialized form is what
/// "the same project" actually means for save/reload anyway.
fn snapshot(p: &Project) -> serde_json::Value {
    serde_json::to_value(p).expect("a project must serialize")
}

fn joint_rest(p: &Project, j: JointId) -> Vec2 {
    match &p.puppets[&PUPPET].kind {
        PuppetKind::Mesh(m) => m.skeleton.joints[&j].rest,
        _ => panic!("fixture puppet is a mesh puppet"),
    }
}

// ── every command inverts ──────────────────────────────────────────────

fn assert_inverts(cmd: Box<dyn DocCommand>) {
    let mut p = fixture();
    let before = snapshot(&p);
    let mut stack = UndoStack::new();
    let label = cmd.label().to_string();

    apply_command(&mut p, &mut stack, cmd).expect("apply");
    assert_ne!(
        snapshot(&p),
        before,
        "{label}: applying it changed nothing, so the test proves nothing"
    );

    stack
        .undo(&mut p)
        .expect("something to undo")
        .expect("revert");
    assert_eq!(snapshot(&p), before, "{label}: revert did not restore");
}

#[test]
fn move_joint_rest_inverts() {
    assert_inverts(Box::new(MoveJointRest {
        puppet: PUPPET,
        joint: J_KNEE,
        from: Vec2::new(52.0, 84.0),
        to: Vec2::new(60.0, 90.0),
    }));
}

#[test]
fn set_bone_param_inverts() {
    assert_inverts(Box::new(SetBoneParam {
        puppet: PUPPET,
        bone: B_SHIN,
        param: BoneParam::Stiffness,
        from: 0.8,
        to: 0.35,
    }));
}

#[test]
fn set_joint_pinned_inverts() {
    assert_inverts(Box::new(SetJointPinned {
        puppet: PUPPET,
        joint: J_FOOT,
        from: false,
        to: true,
    }));
}

#[test]
fn set_layer_scalar_inverts() {
    assert_inverts(Box::new(SetLayerScalar {
        layer: LAYER,
        which: LayerScalar::Opacity,
        from: 1.0,
        to: 0.4,
    }));
}

#[test]
fn rename_layer_inverts() {
    assert_inverts(Box::new(RenameLayer {
        layer: LAYER,
        from: "Puppets".into(),
        to: "Downstage".into(),
    }));
}

#[test]
fn add_layer_inverts() {
    assert_inverts(Box::new(AddLayer {
        layer: Layer::new(LayerId(120), "Backdrop"),
    }));
}

#[test]
fn add_puppet_inverts() {
    let mut extra = mesh_puppet(8);
    extra.id = PuppetId(140);
    assert_inverts(Box::new(AddPuppet {
        puppet: extra,
        layer: LAYER,
    }));
}

#[test]
fn remove_puppet_inverts_including_its_place_in_the_layer() {
    // Two puppets, so "put it back" has a wrong answer available: appending
    // instead of restoring the slot would pass with only one.
    let mut p = fixture();
    let mut second = mesh_puppet(8);
    second.id = PuppetId(150);
    p.puppets.insert(second.id, second);
    p.layer_data
        .get_mut(&LAYER)
        .unwrap()
        .contents
        .push(PuppetId(150));

    let before = snapshot(&p);
    let mut stack = UndoStack::new();
    apply_command(&mut p, &mut stack, Box::new(RemovePuppet::new(PUPPET))).expect("apply");
    assert!(!p.puppets.contains_key(&PUPPET));

    stack.undo(&mut p).unwrap().expect("revert");
    assert_eq!(
        snapshot(&p),
        before,
        "the puppet must return to its original paint slot, not to the end"
    );
}

#[test]
fn replace_puppet_round_trips_a_5k_vertex_mesh() {
    let mut p = fixture();
    let before = snapshot(&p);
    let mut stack = UndoStack::new();

    let big = mesh_puppet(5_000);
    apply_command(
        &mut p,
        &mut stack,
        Box::new(ReplacePuppet::new(
            PUPPET,
            "Retriangulate",
            DocChange::MeshRebuilt(PUPPET),
            big,
        )),
    )
    .expect("apply");

    match &p.puppets[&PUPPET].kind {
        PuppetKind::Mesh(m) => assert_eq!(m.mesh.positions.len(), 5_000),
        _ => panic!("still a mesh puppet"),
    }

    stack.undo(&mut p).unwrap().expect("revert");
    assert_eq!(snapshot(&p), before);
}

// ── granularity ────────────────────────────────────────────────────────

#[test]
fn moving_a_joint_does_not_claim_the_mesh_changed() {
    // If this regresses, every drag rebuilds a GPU mesh.
    let mut p = fixture();
    let mut cmd = MoveJointRest {
        puppet: PUPPET,
        joint: J_KNEE,
        from: Vec2::new(52.0, 84.0),
        to: Vec2::new(53.0, 85.0),
    };
    let changes = cmd.apply(&mut p).expect("apply");
    assert!(changes.contains(DocChange::JointMoved(PUPPET, J_KNEE)));
    assert!(!changes.contains(DocChange::MeshRebuilt(PUPPET)));
    assert!(!changes.contains(DocChange::SkeletonChanged(PUPPET)));
}

#[test]
fn changing_a_bone_parameter_rebuilds_the_rig_not_the_mesh() {
    let mut p = fixture();
    let mut cmd = SetBoneParam {
        puppet: PUPPET,
        bone: B_THIGH,
        param: BoneParam::Stiffness,
        from: 0.8,
        to: 0.5,
    };
    let changes = cmd.apply(&mut p).expect("apply");
    assert!(changes.contains(DocChange::SkeletonChanged(PUPPET)));
    assert!(!changes.contains(DocChange::MeshRebuilt(PUPPET)));
}

// ── merging ────────────────────────────────────────────────────────────

#[test]
fn a_two_hundred_event_drag_is_one_undo_step() {
    let mut p = fixture();
    let start = joint_rest(&p, J_KNEE);
    let before = snapshot(&p);
    let mut stack = UndoStack::new();

    let mut prev = start;
    for i in 1..=200 {
        let to = start + Vec2::new(i as f32 * 0.1, 0.0);
        apply_command(
            &mut p,
            &mut stack,
            Box::new(MoveJointRest {
                puppet: PUPPET,
                joint: J_KNEE,
                from: prev,
                to,
            }),
        )
        .expect("apply");
        prev = to;
    }

    assert_eq!(stack.len(), 1, "a drag must collapse into one entry");
    stack.undo(&mut p).unwrap().expect("revert");
    assert_eq!(
        snapshot(&p),
        before,
        "one undo must return to before the whole drag"
    );
}

#[test]
fn break_merge_starts_a_new_entry() {
    let mut p = fixture();
    let mut stack = UndoStack::new();
    let a = joint_rest(&p, J_KNEE);
    let b = a + Vec2::new(5.0, 0.0);
    let c = b + Vec2::new(5.0, 0.0);

    apply_command(
        &mut p,
        &mut stack,
        Box::new(MoveJointRest {
            puppet: PUPPET,
            joint: J_KNEE,
            from: a,
            to: b,
        }),
    )
    .unwrap();
    stack.break_merge(); // mouse-up
    apply_command(
        &mut p,
        &mut stack,
        Box::new(MoveJointRest {
            puppet: PUPPET,
            joint: J_KNEE,
            from: b,
            to: c,
        }),
    )
    .unwrap();

    assert_eq!(stack.len(), 2, "two gestures are two undo steps");
    stack.undo(&mut p).unwrap().unwrap();
    assert_eq!(
        joint_rest(&p, J_KNEE),
        b,
        "undo returns to the first gesture's end"
    );
}

#[test]
fn different_joints_do_not_merge_into_each_other() {
    let mut p = fixture();
    let mut stack = UndoStack::new();
    let knee = joint_rest(&p, J_KNEE);
    let foot = joint_rest(&p, J_FOOT);

    apply_command(
        &mut p,
        &mut stack,
        Box::new(MoveJointRest {
            puppet: PUPPET,
            joint: J_KNEE,
            from: knee,
            to: knee + Vec2::X,
        }),
    )
    .unwrap();
    apply_command(
        &mut p,
        &mut stack,
        Box::new(MoveJointRest {
            puppet: PUPPET,
            joint: J_FOOT,
            from: foot,
            to: foot + Vec2::X,
        }),
    )
    .unwrap();

    assert_eq!(stack.len(), 2);
}

// ── redo, and the caps ─────────────────────────────────────────────────

#[test]
fn redo_replays_and_a_new_edit_clears_the_redo_branch() {
    let mut p = fixture();
    let mut stack = UndoStack::new();
    let knee = joint_rest(&p, J_KNEE);
    let moved = knee + Vec2::new(9.0, 0.0);

    apply_command(
        &mut p,
        &mut stack,
        Box::new(MoveJointRest {
            puppet: PUPPET,
            joint: J_KNEE,
            from: knee,
            to: moved,
        }),
    )
    .unwrap();
    stack.undo(&mut p).unwrap().unwrap();
    assert_eq!(joint_rest(&p, J_KNEE), knee);

    stack.redo(&mut p).unwrap().unwrap();
    assert_eq!(joint_rest(&p, J_KNEE), moved);

    stack.undo(&mut p).unwrap().unwrap();
    assert!(stack.can_redo());
    apply_command(
        &mut p,
        &mut stack,
        Box::new(RenameLayer {
            layer: LAYER,
            from: "Puppets".into(),
            to: "Other".into(),
        }),
    )
    .unwrap();
    assert!(!stack.can_redo(), "a new edit abandons the redo branch");
}

#[test]
fn the_entry_cap_drops_the_oldest() {
    let mut p = fixture();
    let mut stack = UndoStack::with_caps(10, usize::MAX);
    for i in 0..25 {
        apply_command(
            &mut p,
            &mut stack,
            Box::new(RenameLayer {
                layer: LAYER,
                from: format!("n{}", i),
                to: format!("n{}", i + 1),
            }),
        )
        .unwrap();
        stack.break_merge();
    }
    assert_eq!(stack.len(), 10);
}

#[test]
fn the_memory_cap_drops_the_oldest_but_keeps_one() {
    let mut p = fixture();
    // Cap far below one snapshot, so every push overflows it.
    let mut stack = UndoStack::with_caps(100, 1_024);
    for _ in 0..5 {
        apply_command(
            &mut p,
            &mut stack,
            Box::new(ReplacePuppet::new(
                PUPPET,
                "Retriangulate",
                DocChange::MeshRebuilt(PUPPET),
                mesh_puppet(2_000),
            )),
        )
        .unwrap();
        stack.break_merge();
    }
    assert_eq!(
        stack.len(),
        1,
        "an oversized command stays undoable once rather than not at all"
    );
    assert!(stack.can_undo());
}

// ── the property that catches asymmetric commands ──────────────────────

#[derive(Debug, Clone)]
enum Op {
    MoveJoint(u8, f32, f32),
    BoneStiffness(u8, f32),
    PinJoint(u8, bool),
    LayerOpacity(f32),
    Rename(u8),
    Replace(u16),
    Reorder,
}

fn op_strategy() -> impl Strategy<Value = Op> {
    prop_oneof![
        (0u8..3, -50.0f32..50.0, -50.0f32..50.0).prop_map(|(j, x, y)| Op::MoveJoint(j, x, y)),
        (0u8..2, 0.0f32..1.0).prop_map(|(b, s)| Op::BoneStiffness(b, s)),
        (0u8..3, any::<bool>()).prop_map(|(j, v)| Op::PinJoint(j, v)),
        (0.0f32..1.0).prop_map(Op::LayerOpacity),
        (0u8..8).prop_map(Op::Rename),
        (1u16..40).prop_map(Op::Replace),
        Just(Op::Reorder),
    ]
}

fn build(op: &Op, p: &Project) -> Box<dyn DocCommand> {
    let joints = [J_HIP, J_KNEE, J_FOOT];
    let bones = [B_THIGH, B_SHIN];
    match *op {
        Op::MoveJoint(j, x, y) => {
            let joint = joints[j as usize % 3];
            Box::new(MoveJointRest {
                puppet: PUPPET,
                joint,
                from: joint_rest(p, joint),
                to: Vec2::new(x, y),
            })
        }
        Op::BoneStiffness(b, s) => {
            let bone = bones[b as usize % 2];
            let from = match &p.puppets[&PUPPET].kind {
                PuppetKind::Mesh(m) => m.skeleton.bones[&bone].stiffness,
                _ => unreachable!(),
            };
            Box::new(SetBoneParam {
                puppet: PUPPET,
                bone,
                param: BoneParam::Stiffness,
                from,
                to: s,
            })
        }
        Op::PinJoint(j, v) => {
            let joint = joints[j as usize % 3];
            let from = match &p.puppets[&PUPPET].kind {
                PuppetKind::Mesh(m) => m.skeleton.joints[&joint].pinned,
                _ => unreachable!(),
            };
            Box::new(SetJointPinned {
                puppet: PUPPET,
                joint,
                from,
                to: v,
            })
        }
        Op::LayerOpacity(o) => Box::new(SetLayerScalar {
            layer: LAYER,
            which: LayerScalar::Opacity,
            from: p.layer_data[&LAYER].opacity,
            to: o,
        }),
        Op::Rename(n) => Box::new(RenameLayer {
            layer: LAYER,
            from: p.layer_data[&LAYER].name.clone(),
            to: format!("layer-{n}"),
        }),
        Op::Replace(v) => Box::new(ReplacePuppet::new(
            PUPPET,
            "Retriangulate",
            DocChange::MeshRebuilt(PUPPET),
            mesh_puppet(v as usize),
        )),
        Op::Reorder => {
            let mut to = p.layers.clone();
            to.reverse();
            Box::new(ReorderLayers {
                from: p.layers.clone(),
                to,
            })
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// Any sequence of commands, fully undone, returns the exact document.
    /// This is the test that catches a command whose `revert` is not the
    /// inverse of its `apply` — the failure mode that is invisible until a
    /// user undoes past it and their show is subtly wrong.
    #[test]
    fn undoing_everything_restores_the_document(ops in prop::collection::vec(op_strategy(), 1..40)) {
        let mut p = fixture();
        let before = snapshot(&p);
        let mut stack = UndoStack::new();

        for op in &ops {
            let cmd = build(op, &p);
            apply_command(&mut p, &mut stack, cmd).expect("apply");
            stack.break_merge(); // each op is its own gesture here
        }

        while stack.can_undo() {
            stack.undo(&mut p).unwrap().expect("revert");
        }

        prop_assert_eq!(snapshot(&p), before);
    }

    /// The same, but as one continuous gesture: no `break_merge`, so like
    /// commands fold together and the merge path is what is being tested.
    ///
    /// This variant exists because the version above missed a real bug. A
    /// deliberate fault — `merge` overwriting `from` as well as `to`, which
    /// breaks the invariant that a merged pair still inverts — was caught by
    /// the hand-written drag test and sailed straight through the property,
    /// because breaking the gesture after every operation meant `merge` was
    /// never called. A property that never reaches the code it is meant to
    /// cover is worth exactly as much as no property.
    #[test]
    fn undoing_an_unbroken_gesture_restores_the_document(ops in prop::collection::vec(op_strategy(), 1..40)) {
        let mut p = fixture();
        let before = snapshot(&p);
        let mut stack = UndoStack::new();

        for op in &ops {
            let cmd = build(op, &p);
            apply_command(&mut p, &mut stack, cmd).expect("apply");
        }

        while stack.can_undo() {
            stack.undo(&mut p).unwrap().expect("revert");
        }

        prop_assert_eq!(snapshot(&p), before);
    }

    /// And redoing all of it lands back where the sequence ended.
    #[test]
    fn redoing_everything_returns_to_the_edited_state(ops in prop::collection::vec(op_strategy(), 1..20)) {
        let mut p = fixture();
        let mut stack = UndoStack::new();
        for op in &ops {
            let cmd = build(op, &p);
            apply_command(&mut p, &mut stack, cmd).expect("apply");
            stack.break_merge();
        }
        let edited = snapshot(&p);

        while stack.can_undo() {
            stack.undo(&mut p).unwrap().expect("revert");
        }
        while stack.can_redo() {
            stack.redo(&mut p).unwrap().expect("apply");
        }

        prop_assert_eq!(snapshot(&p), edited);
    }
}
