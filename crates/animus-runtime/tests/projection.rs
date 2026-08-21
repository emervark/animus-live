//! Document → ECS projection, driven through a real `App` with no window,
//! no adapter and no GPU.
//!
//! The assertions people usually skip are the ones here: that a drag does
//! **not** rebuild the GPU mesh, and that despawning leaves nothing behind.
//! Both are invisible until a show stutters or a puppet stops responding.

use animus_core::doc::*;
use animus_core::ids::{AssetId as DocAssetId, BoneId, JointId, LayerId, PuppetId};
use animus_runtime::*;
use bevy::asset::AssetApp;
use bevy::mesh::skinning::{SkinnedMesh, SkinnedMeshInverseBindposes};
use bevy::prelude::*;
use glam::Vec2;
use indexmap::IndexMap;

const LAYER: LayerId = LayerId(9);
const PUPPET: PuppetId = PuppetId(31);
const TEX: DocAssetId = DocAssetId(4);
const J_A: JointId = JointId(77);
const J_B: JointId = JointId(21);
const J_C: JointId = JointId(50);
const B_ONE: BoneId = BoneId(93);
const B_TWO: BoneId = BoneId(35);

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
        attach_radius: 40.0,
    }
}

fn document() -> Project {
    let mut joints = IndexMap::new();
    joints.insert(J_A, joint(J_A, Vec2::new(50.0, 20.0)));
    joints.insert(J_B, joint(J_B, Vec2::new(50.0, 60.0)));
    joints.insert(J_C, joint(J_C, Vec2::new(50.0, 100.0)));

    let mut bones = IndexMap::new();
    bones.insert(B_ONE, bone(B_ONE, J_A, J_B));
    bones.insert(B_TWO, bone(B_TWO, J_B, J_C));

    let mut mp = MeshPuppet::empty(TEX);
    mp.skeleton = SkeletonData { joints, bones };
    mp.mesh.positions = vec![
        Vec2::new(40.0, 10.0),
        Vec2::new(60.0, 10.0),
        Vec2::new(50.0, 110.0),
    ];
    mp.mesh.uvs = vec![Vec2::ZERO; 3];
    mp.mesh.triangles = vec![0, 1, 2];
    mp.attachments.entries.push(Attachment {
        vertex: 2,
        bone: B_TWO,
        weight: 1.0,
        local: Vec2::ZERO,
    });

    let mut p = Project::new("Projection Test");
    p.next_id = 500;
    let mut layer = Layer::new(LAYER, "Puppets");
    layer.depth = 0.25;
    layer.contents.push(PUPPET);
    p.layer_data.insert(LAYER, layer);
    p.layers.push(LAYER);
    p.puppets.insert(
        PUPPET,
        Puppet {
            id: PUPPET,
            name: "test".into(),
            kind: PuppetKind::Mesh(mp),
        },
    );
    p
}

fn app_with(doc: Project) -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(AssetPlugin::default())
        .init_asset::<Mesh>()
        .init_asset::<StandardMaterial>()
        .init_asset::<SkinnedMeshInverseBindposes>()
        .init_asset::<Image>()
        .add_plugins(RuntimePlugin::new(doc));
    app
}

fn push(app: &mut App, change: DocChange) {
    app.world_mut()
        .resource_mut::<PendingChangesRes>()
        .push(change);
}

fn counts(app: &mut App) -> (usize, usize, usize) {
    let roots = app
        .world_mut()
        .query::<&PuppetRoot>()
        .iter(app.world())
        .count();
    let meshes = app
        .world_mut()
        .query::<&PuppetMesh>()
        .iter(app.world())
        .count();
    let bones = app.world_mut().query::<&BoneOf>().iter(app.world()).count();
    (roots, meshes, bones)
}

fn mesh_asset_id(app: &mut App) -> bevy::asset::AssetId<Mesh> {
    let handle = app
        .world_mut()
        .query::<&Mesh3d>()
        .iter(app.world())
        .next()
        .expect("a puppet mesh entity")
        .0
        .clone();
    handle.id()
}

// ── spawn and despawn ──────────────────────────────────────────────────

#[test]
fn adding_a_puppet_spawns_a_root_a_mesh_and_one_entity_per_bone() {
    let mut app = app_with(document());
    push(&mut app, DocChange::PuppetAdded(PUPPET));
    app.update();

    assert_eq!(counts(&mut app), (1, 1, 2), "root, mesh, two bones");

    let index = app.world().resource::<EntityIndex>();
    let ents = index.get(PUPPET).expect("indexed");
    assert_eq!(ents.bones.len(), 2);
    assert_eq!(ents.entity_count(), 4);
}

#[test]
fn removing_a_puppet_leaves_nothing_behind() {
    let mut app = app_with(document());
    push(&mut app, DocChange::PuppetAdded(PUPPET));
    app.update();
    push(&mut app, DocChange::PuppetRemoved(PUPPET));
    app.update();

    assert_eq!(counts(&mut app), (0, 0, 0), "every entity is gone");
    assert!(
        app.world().resource::<EntityIndex>().get(PUPPET).is_none(),
        "and the index does not still claim they exist"
    );
    assert_eq!(app.world().resource::<EntityIndex>().entity_count(), 0);
}

#[test]
fn the_skinned_mesh_addresses_the_bone_entities_in_index_order() {
    // SkinnedMesh::joints, the inverse bind poses and EntityIndex::bones are
    // three views of one order. If they disagree, vertices follow the wrong
    // limb and nothing errors.
    let mut app = app_with(document());
    push(&mut app, DocChange::PuppetAdded(PUPPET));
    app.update();

    let expected = app
        .world()
        .resource::<EntityIndex>()
        .get(PUPPET)
        .unwrap()
        .bones
        .clone();
    let skinned = app
        .world_mut()
        .query::<&SkinnedMesh>()
        .iter(app.world())
        .next()
        .expect("a SkinnedMesh")
        .joints
        .clone();
    assert_eq!(skinned, expected);
}

#[test]
fn bone_entities_carry_their_dense_index_and_their_document_id() {
    let mut app = app_with(document());
    push(&mut app, DocChange::PuppetAdded(PUPPET));
    app.update();

    let mut seen: Vec<(u32, BoneId)> = app
        .world_mut()
        .query::<&BoneOf>()
        .iter(app.world())
        .map(|b| (b.index, b.bone))
        .collect();
    seen.sort_by_key(|(i, _)| *i);

    assert_eq!(seen, vec![(0, B_ONE), (1, B_TWO)]);
    // and the ids are not the indices, or this fixture proves nothing
    assert_ne!(B_ONE.0, 0);
    assert_ne!(B_TWO.0, 1);
}

#[test]
fn the_root_sits_at_its_layers_depth() {
    let mut app = app_with(document());
    push(&mut app, DocChange::PuppetAdded(PUPPET));
    app.update();

    let z = app
        .world_mut()
        .query_filtered::<&Transform, With<PuppetRoot>>()
        .iter(app.world())
        .next()
        .expect("a root")
        .translation
        .z;
    assert!((z - 0.25).abs() < 1e-6, "layer depth is world Z, got {z}");
}

/// Hiding a layer has to take its puppets off the stage.
///
/// The gap this pins: `Layer::visible` was written by the panel, saved to the
/// file, and read by nothing. The row dimmed, the eye struck through, and the
/// puppet stayed on the projector — the worst possible failure, because the
/// operator believes the audience cannot see it.
#[test]
fn hiding_a_layer_takes_its_puppets_off_the_stage() {
    let mut app = app_with(document());
    push(&mut app, DocChange::PuppetAdded(PUPPET));
    app.update();

    let visibility = |app: &mut App| {
        *app.world_mut()
            .query_filtered::<&Visibility, With<PuppetRoot>>()
            .iter(app.world())
            .next()
            .expect("a root")
    };
    assert_eq!(visibility(&mut app), Visibility::Inherited);

    let layer = *app
        .world()
        .resource::<DocumentRes>()
        .0
        .layers
        .first()
        .unwrap();
    app.world_mut()
        .resource_mut::<DocumentRes>()
        .0
        .layer_data
        .get_mut(&layer)
        .unwrap()
        .visible = false;
    push(&mut app, DocChange::LayerPropsChanged(layer));
    app.update();

    assert_eq!(
        visibility(&mut app),
        Visibility::Hidden,
        "the puppet is still being drawn after its layer was hidden"
    );

    app.world_mut()
        .resource_mut::<DocumentRes>()
        .0
        .layer_data
        .get_mut(&layer)
        .unwrap()
        .visible = true;
    push(&mut app, DocChange::LayerPropsChanged(layer));
    app.update();
    assert_eq!(
        visibility(&mut app),
        Visibility::Inherited,
        "and showing it again brings it back"
    );
}

// ── the distinction that costs frames ──────────────────────────────────

#[test]
fn moving_a_joint_does_not_rebuild_the_gpu_mesh() {
    // A drag emits one of these per mouse-move. Rebuilding a mesh asset per
    // event is the difference between a smooth drag and a slideshow.
    let mut app = app_with(document());
    push(&mut app, DocChange::PuppetAdded(PUPPET));
    app.update();
    let before = mesh_asset_id(&mut app);

    for _ in 0..50 {
        push(&mut app, DocChange::JointMoved(PUPPET, J_B));
    }
    app.update();

    assert_eq!(
        mesh_asset_id(&mut app),
        before,
        "the mesh handle must be the same asset after fifty joint moves"
    );
    assert_eq!(counts(&mut app), (1, 1, 2), "and no entity churn");
}

#[test]
fn a_topology_change_does_rebuild_the_gpu_mesh() {
    let mut app = app_with(document());
    push(&mut app, DocChange::PuppetAdded(PUPPET));
    app.update();
    let before = mesh_asset_id(&mut app);

    push(&mut app, DocChange::MeshRebuilt(PUPPET));
    app.update();

    assert_ne!(
        mesh_asset_id(&mut app),
        before,
        "a topology change must produce a new mesh asset"
    );
    assert_eq!(counts(&mut app), (1, 1, 2), "still exactly one puppet");
}

#[test]
fn a_skeleton_change_respawns_bones_without_leaking_the_old_ones() {
    let mut app = app_with(document());
    push(&mut app, DocChange::PuppetAdded(PUPPET));
    app.update();

    push(&mut app, DocChange::SkeletonChanged(PUPPET));
    app.update();

    assert_eq!(
        counts(&mut app),
        (1, 1, 2),
        "two bones before and two after — not four"
    );
    let index = app.world().resource::<EntityIndex>();
    assert_eq!(index.entity_count(), 4);
}

#[test]
fn many_changes_in_one_frame_produce_one_puppet() {
    // Folding is the whole point of bucketing: a frame carrying an add and
    // fifty edits must not spawn fifty-one puppets.
    let mut app = app_with(document());
    push(&mut app, DocChange::PuppetAdded(PUPPET));
    for _ in 0..50 {
        push(&mut app, DocChange::JointMoved(PUPPET, J_A));
        push(&mut app, DocChange::MeshRebuilt(PUPPET));
    }
    app.update();

    assert_eq!(counts(&mut app), (1, 1, 2));
}

#[test]
fn a_puppet_added_and_removed_in_the_same_frame_leaves_nothing() {
    let mut app = app_with(document());
    push(&mut app, DocChange::PuppetAdded(PUPPET));
    push(&mut app, DocChange::MeshRebuilt(PUPPET));
    push(&mut app, DocChange::PuppetRemoved(PUPPET));
    app.update();

    assert_eq!(counts(&mut app), (0, 0, 0));
    assert_eq!(app.world().resource::<EntityIndex>().entity_count(), 0);
}

/// Changing gravity must reach the springs, not just the file.
///
/// The bug this pins: solver settings are compiled into `CompiledRig`, and
/// `SolverConfigChanged` used to fall into the "not per-puppet" arm and do
/// nothing. Gravity was editable, saved, shown in the panel — and read by
/// nothing after startup.
#[test]
fn a_solver_change_recompiles_every_rig() {
    let mut app = app_with(document());
    app.update();

    let before = app
        .world_mut()
        .query::<&CompiledRigRef>()
        .iter(app.world())
        .next()
        .map(|r| std::sync::Arc::as_ptr(&r.0))
        .expect("a rig");

    app.world_mut()
        .resource_mut::<DocumentRes>()
        .0
        .solver
        .gravity = Vec2::new(0.0, -9.8);
    push(&mut app, DocChange::SolverConfigChanged);
    app.update();

    let after = app
        .world_mut()
        .query::<&CompiledRigRef>()
        .iter(app.world())
        .next()
        .map(|r| std::sync::Arc::as_ptr(&r.0))
        .expect("a rig");

    assert_ne!(
        before, after,
        "the rig must be recompiled, or the new gravity never reaches the solver"
    );
}

#[test]
fn an_empty_change_list_does_nothing_at_all() {
    // An *empty document*, because a document holding a puppet is not an
    // empty change list any more: the plugin announces what it was handed.
    let mut app = app_with(Project::new("Nothing In It"));
    app.update();
    assert_eq!(counts(&mut app), (0, 0, 0));
}

/// Opening a project must put it on stage without anyone asking.
///
/// The bug this pins: every puppet in a document loaded from disk was
/// described to the layer list and to nothing else, so a saved show reopened
/// as an empty stage — no mesh, no gizmos, no projector image, no error.
#[test]
fn a_document_that_arrives_whole_is_projected_without_being_asked() {
    let mut app = app_with(document());
    app.update();

    assert_eq!(counts(&mut app), (1, 1, 2));
    assert_eq!(
        app.world().resource::<EntityIndex>().puppets.len(),
        1,
        "the index describes the puppet the document arrived holding"
    );
}

// ── the sequencer actually drives the puppet ───────────────────────────

/// **Poses authored in EDIT must play in PERFORM.**
///
/// The whole instrument rests on this: a step holds a pose, the transport
/// walks the steps, and the targets follow. If this does not move, nothing
/// the operator authored is ever heard.
#[test]
fn a_running_sequencer_drives_the_targets_toward_each_step() {
    use animus_runtime::{JointTargets, Sequencer};

    let mut app = app_with(document());
    app.init_resource::<JointTargets>()
        .init_resource::<animus_runtime::HeldJoint>()
        .add_plugins(animus_runtime::SequencerPlugin);
    push(&mut app, DocChange::PuppetAdded(PUPPET));
    app.update();

    // Two very different poses, two steps apart.
    let joint = {
        let world = app.world_mut();
        let rig = world
            .query::<&animus_runtime::CompiledRigRef>()
            .iter(world)
            .next()
            .expect("a rig")
            .0
            .clone();
        rig.joint_id(0).expect("a joint")
    };

    {
        let mut seq = app.world_mut().resource_mut::<Sequencer>();
        seq.set_len(4);
        seq.set_pose(0, vec![(PUPPET, joint, Vec2::new(0.0, 0.0))]);
        seq.set_pose(2, vec![(PUPPET, joint, Vec2::new(500.0, 0.0))]);
        // Fast steps and a real sleep below: a headless `app.update()` loop
        // has microsecond frames, so wall-clock time is what the transport
        // actually reads.
        seq.bpm = 6000.0; // 10ms per step
        seq.running = true;
    }

    let mut seen: Vec<Vec2> = Vec::new();
    for _ in 0..120 {
        std::thread::sleep(std::time::Duration::from_millis(2));
        app.update();
        if let Some(v) = app
            .world()
            .resource::<JointTargets>()
            .0
            .get(&(PUPPET, joint))
        {
            seen.push(*v);
        }
    }

    assert!(
        !seen.is_empty(),
        "the sequencer never wrote a target: nothing the operator posed can play"
    );
    let spread = seen.iter().map(|v| v.x).fold(f32::NEG_INFINITY, f32::max)
        - seen.iter().map(|v| v.x).fold(f32::INFINITY, f32::min);
    assert!(
        spread > 100.0,
        "the target barely moved ({spread:.1}px): the steps are not being played"
    );
}

/// **Live recording overdubs; it does not overwrite.**
///
/// Arming used to write the whole body into every step it crossed, which
/// pinned every joint the hand was not touching to wherever it happened to
/// be. The next step then recorded that same frozen body, and the one after
/// that — so across a bar the only thing that ever differed was the limb
/// still in the hand. That is the "only the last change gets saved" an
/// operator sees, and it is what this test exists to keep out.
#[test]
fn arming_records_the_played_joint_and_leaves_the_rest_of_the_step_alone() {
    use animus_runtime::{HeldJoint, JointTargets, Sequencer};

    let mut app = app_with(document());
    app.init_resource::<JointTargets>()
        .init_resource::<HeldJoint>()
        .add_plugins(animus_runtime::SequencerPlugin);
    push(&mut app, DocChange::PuppetAdded(PUPPET));
    app.update();

    let (played, untouched) = {
        let world = app.world_mut();
        let rig = world
            .query::<&animus_runtime::CompiledRigRef>()
            .iter(world)
            .next()
            .expect("a rig")
            .0
            .clone();
        (rig.joint_id(0).expect("a joint"), rig.joint_id(1).unwrap())
    };

    // A step the operator authored earlier, holding a joint they are about
    // to leave alone.
    let authored = Vec2::new(-300.0, 44.0);
    {
        let mut seq = app.world_mut().resource_mut::<Sequencer>();
        seq.set_len(4);
        for step in 0..4 {
            seq.set_pose(step, vec![(PUPPET, untouched, authored)]);
        }
        seq.bpm = 6000.0;
        seq.running = true;
        seq.armed = true;
    }
    // The hand is on one joint, and only that joint may be recorded.
    app.world_mut().resource_mut::<HeldJoint>().0 = Some((PUPPET, played));

    for _ in 0..120 {
        std::thread::sleep(std::time::Duration::from_millis(2));
        app.update();
    }

    let seq = app.world().resource::<Sequencer>();
    for step in 0..4 {
        let pose = seq
            .pose(step)
            .unwrap_or_else(|| panic!("step {step} empty"));
        let kept = pose
            .iter()
            .find(|(_, j, _)| *j == untouched)
            .map(|(_, _, v)| *v);
        assert_eq!(
            kept,
            Some(authored),
            "step {step}: recording overwrote a joint the hand never touched"
        );
    }
    assert!(
        (0..4).any(|s| seq
            .pose(s)
            .is_some_and(|p| p.iter().any(|(_, j, _)| *j == played))),
        "the joint the operator was actually holding was never recorded"
    );
}
