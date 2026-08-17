//! The solver driver: targets in, bones out, and the guard that keeps one
//! bad puppet from taking down a show.
//!
//! Headless, through a real `App`. `Time<Fixed>` is advanced by hand so the
//! tests are deterministic rather than dependent on how long the machine
//! took to run them.

use animus_core::doc::*;
use animus_core::ids::{AssetId as DocAssetId, BoneId, JointId, LayerId, PuppetId};
use animus_runtime::solve::SolverPlugin;
use animus_runtime::*;
use bevy::asset::AssetApp;
use bevy::mesh::skinning::SkinnedMeshInverseBindposes;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use glam::Vec2;
use indexmap::IndexMap;
use std::time::Duration;

const LAYER: LayerId = LayerId(9);
const PUPPET: PuppetId = PuppetId(31);
const OTHER: PuppetId = PuppetId(32);
const TEX: DocAssetId = DocAssetId(4);
const J_ROOT: JointId = JointId(77);
const J_TIP: JointId = JointId(21);
const B_SPINE: BoneId = BoneId(93);

fn puppet(id: PuppetId) -> Puppet {
    let mut joints = IndexMap::new();
    let mut root = Joint {
        id: J_ROOT,
        name: "root".into(),
        rest: Vec2::new(50.0, 100.0),
        rest_angle: 0.0,
        inv_mass: 0.0,
        pinned: true,
    };
    root.pinned = true;
    joints.insert(J_ROOT, root);
    joints.insert(
        J_TIP,
        Joint {
            id: J_TIP,
            name: "tip".into(),
            rest: Vec2::new(50.0, 40.0),
            rest_angle: 0.0,
            inv_mass: 1.0,
            pinned: false,
        },
    );

    let mut bones = IndexMap::new();
    bones.insert(
        B_SPINE,
        Bone {
            id: B_SPINE,
            name: "spine".into(),
            a: J_ROOT,
            b: J_TIP,
            rest_length: None,
            stiffness: 0.8,
            damping: 0.1,
            length_mul: 1.0,
            attach_radius: 60.0,
        },
    );

    let mut mp = MeshPuppet::empty(TEX);
    mp.skeleton = SkeletonData { joints, bones };
    mp.mesh.positions = vec![
        Vec2::new(40.0, 40.0),
        Vec2::new(60.0, 40.0),
        Vec2::new(50.0, 100.0),
    ];
    mp.mesh.uvs = vec![Vec2::ZERO; 3];
    mp.mesh.triangles = vec![0, 1, 2];
    mp.attachments.entries.push(Attachment {
        vertex: 0,
        bone: B_SPINE,
        weight: 1.0,
        local: Vec2::ZERO,
    });

    Puppet {
        id,
        name: format!("p{}", id.0),
        kind: PuppetKind::Mesh(mp),
    }
}

fn document(ids: &[PuppetId]) -> Project {
    let mut p = Project::new("Solver Test");
    p.next_id = 500;
    let mut layer = Layer::new(LAYER, "Puppets");
    for id in ids {
        layer.contents.push(*id);
        p.puppets.insert(*id, puppet(*id));
    }
    p.layer_data.insert(LAYER, layer);
    p.layers.push(LAYER);
    p
}

#[derive(Resource, Default, Debug)]
struct PanicLog(Vec<PuppetId>);

fn collect_panics(mut reader: MessageReader<SolverPanic>, mut log: ResMut<PanicLog>) {
    for p in reader.read() {
        log.0.push(p.0);
    }
}

fn app_with(ids: &[PuppetId]) -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(AssetPlugin::default())
        .init_asset::<Mesh>()
        .init_asset::<StandardMaterial>()
        .init_asset::<SkinnedMeshInverseBindposes>()
        .init_asset::<Image>()
        .add_plugins(RuntimePlugin::new(document(ids)))
        .add_plugins(SolverPlugin)
        // Deterministic clock: each `update()` advances exactly one 60Hz
        // frame, so these tests do not depend on how fast the machine ran
        // them. Advancing `Time<Virtual>` by hand does not work here --
        // `TimePlugin` overwrites it from the real clock every frame.
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
            1.0 / 60.0,
        )))
        .init_resource::<PanicLog>()
        .add_systems(Update, collect_panics);

    for id in ids {
        app.world_mut()
            .resource_mut::<PendingChangesRes>()
            .push(DocChange::PuppetAdded(*id));
    }
    app.update();
    app
}

/// One 60Hz frame, which is several solver ticks at the default 120Hz.
fn advance(app: &mut App, _secs: f32) {
    app.update();
}

fn joint_pos(app: &mut App, puppet: PuppetId, joint: JointId) -> Vec2 {
    let mut q = app
        .world_mut()
        .query::<(&PuppetRoot, &CompiledRigRef, &PuppetSolver)>();
    for (root, rig, solver) in q.iter(app.world()) {
        if root.0 == puppet {
            let i = rig.0.joint_index(joint).expect("joint is in the rig");
            return solver.0.positions()[i as usize];
        }
    }
    panic!("no such puppet");
}

// ── the basics ─────────────────────────────────────────────────────────

#[test]
fn a_pinned_joint_never_moves() {
    let mut app = app_with(&[PUPPET]);
    let start = joint_pos(&mut app, PUPPET, J_ROOT);

    // Pull hard on the free end for a long time.
    app.world_mut()
        .resource_mut::<JointTargets>()
        .set(PUPPET, J_TIP, Vec2::new(400.0, -300.0));
    for _ in 0..100 {
        advance(&mut app, 1.0 / 60.0);
    }

    let end = joint_pos(&mut app, PUPPET, J_ROOT);
    assert!(
        (end - start).length() < 1e-4,
        "a pinned joint moved from {start:?} to {end:?}"
    );
}

#[test]
fn a_dragged_joint_follows_and_the_bone_keeps_its_length_after_release() {
    // Worth stating because it surprises people, including the author of the
    // first version of this test: **a released joint does not return to its
    // rest position.** The solver is a graph of distance constraints, not a
    // set of angular springs — nothing in it defines a preferred angle, so a
    // limb keeps whatever direction it was left in and simply stops. That is
    // Animata's model and it is the reason the motion reads as cloth rather
    // than as machinery.
    //
    // What must hold after release is that the bone returns to its rest
    // *length* and the motion damps out.
    let mut app = app_with(&[PUPPET]);
    let rest = joint_pos(&mut app, PUPPET, J_TIP);
    let anchor = joint_pos(&mut app, PUPPET, J_ROOT);
    let rest_len = (rest - anchor).length();

    app.world_mut()
        .resource_mut::<JointTargets>()
        .set(PUPPET, J_TIP, Vec2::new(120.0, 40.0));
    for _ in 0..60 {
        advance(&mut app, 1.0 / 60.0);
    }
    let pulled = joint_pos(&mut app, PUPPET, J_TIP);
    assert!(
        (pulled - rest).length() > 5.0,
        "the joint should have followed the target, moved {:?}",
        pulled - rest
    );

    // Release: the absence of a target is what lets go. There is no "release"
    // command, deliberately.
    app.world_mut().resource_mut::<JointTargets>().clear();
    for _ in 0..240 {
        advance(&mut app, 1.0 / 60.0);
    }

    let released = joint_pos(&mut app, PUPPET, J_TIP);
    let len = (released - anchor).length();
    assert!(
        (len - rest_len).abs() < rest_len * 0.05,
        "the bone must relax to its rest length {rest_len}, got {len}"
    );

    // And it must have stopped: two more frames barely move it.
    let a = joint_pos(&mut app, PUPPET, J_TIP);
    advance(&mut app, 1.0 / 60.0);
    advance(&mut app, 1.0 / 60.0);
    let b = joint_pos(&mut app, PUPPET, J_TIP);
    assert!(
        (b - a).length() < 0.5,
        "the motion should have damped out, still moving {:?}",
        b - a
    );
}

#[test]
fn a_disabled_solver_does_not_move_anything() {
    let mut app = app_with(&[PUPPET]);
    app.world_mut()
        .resource_mut::<DocumentRes>()
        .0
        .solver
        .enabled = false;
    let before = joint_pos(&mut app, PUPPET, J_TIP);

    app.world_mut()
        .resource_mut::<JointTargets>()
        .set(PUPPET, J_TIP, Vec2::new(400.0, 400.0));
    for _ in 0..30 {
        advance(&mut app, 1.0 / 60.0);
    }

    assert_eq!(joint_pos(&mut app, PUPPET, J_TIP), before);
}

// ── the guard ──────────────────────────────────────────────────────────

#[test]
fn a_non_finite_puppet_is_reset_and_reported_without_touching_the_others() {
    let mut app = app_with(&[PUPPET, OTHER]);
    let other_before = joint_pos(&mut app, OTHER, J_TIP);

    // Inject NaN into one puppet only.
    {
        let world = app.world_mut();
        let mut q = world.query::<(&PuppetRoot, &CompiledRigRef, &mut PuppetSolver)>();
        for (root, rig, mut solver) in q.iter_mut(world) {
            if root.0 == PUPPET {
                let i = rig.0.joint_index(J_TIP).unwrap();
                solver.0.displace(i, Vec2::splat(f32::NAN));
            }
        }
    }

    advance(&mut app, 1.0 / 60.0);

    // Assert on the message, not on `LastStepOutcome`: one 60Hz frame runs
    // two 120Hz ticks, and the second one succeeds, so the component is back
    // to `Ok` by the time a test could read it. The message is the contract.
    let seen = app.world().resource::<PanicLog>().0.clone();
    assert!(
        seen.contains(&PUPPET),
        "the guard must report the puppet it reset, saw {seen:?}"
    );
    assert!(
        !seen.contains(&OTHER),
        "and must not report the healthy one"
    );

    let recovered = joint_pos(&mut app, PUPPET, J_TIP);
    assert!(
        recovered.is_finite(),
        "the puppet must be back at a finite pose, got {recovered:?}"
    );
    assert!(
        (joint_pos(&mut app, OTHER, J_TIP) - other_before).length() < 1e-3,
        "the other puppet must be untouched"
    );
}

// ── writeback ──────────────────────────────────────────────────────────

#[test]
fn bone_transforms_follow_the_solver() {
    let mut app = app_with(&[PUPPET]);
    let before = bone_transform(&mut app);

    app.world_mut()
        .resource_mut::<JointTargets>()
        .set(PUPPET, J_TIP, Vec2::new(150.0, 100.0));
    for _ in 0..60 {
        advance(&mut app, 1.0 / 60.0);
    }
    let after = bone_transform(&mut app);

    assert!(
        (after.rotation.to_axis_angle().1 - before.rotation.to_axis_angle().1).abs() > 0.05,
        "pulling the tip sideways must rotate the bone"
    );
}

#[test]
fn bone_transforms_are_finite_every_frame() {
    // Interpolation reads two buffers and divides by nothing, but a NaN here
    // would propagate into the transform hierarchy and take the whole scene
    // with it.
    let mut app = app_with(&[PUPPET]);
    app.world_mut()
        .resource_mut::<JointTargets>()
        .set(PUPPET, J_TIP, Vec2::new(90.0, 10.0));
    for _ in 0..120 {
        advance(&mut app, 1.0 / 120.0);
        let t = bone_transform(&mut app);
        assert!(t.translation.is_finite() && t.scale.is_finite());
    }
}

fn bone_transform(app: &mut App) -> Transform {
    let mut q = app.world_mut().query::<(&BoneOf, &Transform)>();
    q.iter(app.world())
        .find(|(b, _)| b.puppet == PUPPET)
        .map(|(_, t)| *t)
        .expect("a bone entity")
}
