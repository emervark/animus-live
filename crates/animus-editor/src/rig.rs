//! Rigging: joints from clicks, bones from drags, attachment by radius.
//!
//! Everything here is a pure function from (document, gesture) to a
//! [`SetSkeleton`] command — no egui, no ECS. The viewport calls these with
//! world positions it has already unprojected; the tests call them with
//! numbers. Keeping the tool logic out of the UI closure is what makes it
//! testable, and it is also what keeps the one-way rule intact: tools
//! *propose* commands, one system applies them.
//!
//! ## Auto-attach runs after every rig edit, on the whole puppet
//!
//! Adding a bone near a limb steals vertices from the bone that held them —
//! by design. Weights are a *derived* quantity in M1: the authored truth is
//! joints, bones and radii, and `auto_attach` recomputes the table from
//! scratch each time. Incremental attachment (only the new bone's
//! neighbourhood) would be faster and would also mean the table's contents
//! depend on the order edits happened in, which makes "re-run auto-mesh"
//! unreproducible. Weight painting in M6 changes this story; radius editing
//! today does not.

use animus_core::doc::{Bone, Joint, MeshPuppet, Project, PuppetKind, SetSkeleton};
use animus_core::ids::{BoneId, JointId, PuppetId};
use animus_core::skeleton::auto_attach;
use glam::Vec2;

/// How close a click must land to count as touching a joint, in image
/// pixels. One value shared by selection and bone-drawing so "I clicked the
/// joint" always means the same thing.
pub const JOINT_HIT_RADIUS_PX: f32 = 8.0;

/// Default reach of a new bone's attachment, in image pixels.
pub const DEFAULT_ATTACH_RADIUS_PX: f32 = 48.0;

fn mesh_puppet(project: &Project, id: PuppetId) -> Option<&MeshPuppet> {
    match &project.puppets.get(&id)?.kind {
        PuppetKind::Mesh(m) => Some(m),
        _ => None,
    }
}

/// The joint nearest to `pos`, if any is within [`JOINT_HIT_RADIUS_PX`].
pub fn joint_at(project: &Project, puppet: PuppetId, pos: Vec2) -> Option<JointId> {
    let mp = mesh_puppet(project, puppet)?;
    mp.skeleton
        .joints
        .values()
        .map(|j| (j.id, (j.rest - pos).length()))
        .filter(|(_, d)| *d <= JOINT_HIT_RADIUS_PX)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(id, _)| id)
}

/// Place a joint at `pos` (image pixels).
///
/// Pinned when it is the puppet's first joint: the first thing a rig needs
/// is an anchor, and a rig whose every joint is free slides off the canvas
/// the moment the solver starts. Unpinning is one click in the inspector.
pub fn place_joint(project: &mut Project, puppet: PuppetId, pos: Vec2) -> Option<SetSkeleton> {
    let mp = mesh_puppet(project, puppet)?;
    let first = mp.skeleton.joints.is_empty();

    let mut skeleton = mp.skeleton.clone();
    let attachments = mp.attachments.clone();

    let id = JointId(project.alloc_id());
    skeleton.joints.insert(
        id,
        Joint {
            id,
            name: format!("joint {}", skeleton.joints.len() + 1),
            rest: pos,
            rest_angle: 0.0,
            inv_mass: if first { 0.0 } else { 1.0 },
            pinned: first,
        },
    );

    // Placing a joint does not change which bones exist, so the attachment
    // table is untouched: weights belong to bones.
    Some(SetSkeleton::new(
        puppet,
        "Place joint",
        skeleton,
        attachments,
    ))
}

/// Connect two joints with a bone, re-attaching the whole puppet.
///
/// Returns `None` when the pair is degenerate: the same joint twice, a
/// missing joint, or a bone that already exists between the two (in either
/// direction — a spring has no direction, so `a→b` and `b→a` would be the
/// same constraint twice, which doubles its stiffness silently).
pub fn place_bone(
    project: &mut Project,
    puppet: PuppetId,
    a: JointId,
    b: JointId,
) -> Option<SetSkeleton> {
    if a == b {
        return None;
    }
    let mp = mesh_puppet(project, puppet)?;
    if !mp.skeleton.joints.contains_key(&a) || !mp.skeleton.joints.contains_key(&b) {
        return None;
    }
    let duplicate = mp
        .skeleton
        .bones
        .values()
        .any(|bone| (bone.a == a && bone.b == b) || (bone.a == b && bone.b == a));
    if duplicate {
        return None;
    }

    let mut skeleton = mp.skeleton.clone();
    let mesh = mp.mesh.clone();

    let id = BoneId(project.alloc_id());
    skeleton.bones.insert(
        id,
        Bone {
            id,
            name: format!("bone {}", skeleton.bones.len() + 1),
            a,
            b,
            rest_length: None,
            stiffness: 0.8,
            damping: 0.1,
            length_mul: 1.0,
            attach_radius: DEFAULT_ATTACH_RADIUS_PX,
        },
    );

    let attachments = auto_attach(&mesh, &skeleton);
    Some(SetSkeleton::new(puppet, "Add bone", skeleton, attachments))
}

/// Delete a joint, cascading into every bone that names it, then re-attach.
pub fn delete_joint(project: &Project, puppet: PuppetId, joint: JointId) -> Option<SetSkeleton> {
    let mp = mesh_puppet(project, puppet)?;
    if !mp.skeleton.joints.contains_key(&joint) {
        return None;
    }

    let mut skeleton = mp.skeleton.clone();
    skeleton.joints.shift_remove(&joint);
    skeleton
        .bones
        .retain(|_, bone| bone.a != joint && bone.b != joint);

    let attachments = auto_attach(&mp.mesh, &skeleton);
    Some(SetSkeleton::new(
        puppet,
        "Delete joint",
        skeleton,
        attachments,
    ))
}

/// Change a bone's attachment radius and re-attach.
///
/// The radius is authored truth (it survives in the document); the table it
/// produces is derived. Changing one without the other is the inconsistency
/// `SetSkeleton` exists to make unrepresentable.
pub fn set_attach_radius(
    project: &Project,
    puppet: PuppetId,
    bone: BoneId,
    radius: f32,
) -> Option<SetSkeleton> {
    let mp = mesh_puppet(project, puppet)?;
    if !mp.skeleton.bones.contains_key(&bone) {
        return None;
    }

    let mut skeleton = mp.skeleton.clone();
    skeleton.bones.get_mut(&bone)?.attach_radius = radius.max(0.0);

    let attachments = auto_attach(&mp.mesh, &skeleton);
    Some(SetSkeleton::new(
        puppet,
        "Set attachment radius",
        skeleton,
        attachments,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use animus_core::doc::{Layer, Puppet, SkeletonData, UndoStack, apply_command};
    use animus_core::ids::{AssetId, LayerId};
    use glam::Vec2;

    const PUPPET: PuppetId = PuppetId(40);
    const LAYER: LayerId = LayerId(41);

    /// A 100x100 grid mesh with no rig, as if freshly imported.
    fn project() -> Project {
        let mut mp = MeshPuppet::empty(AssetId(42));
        for y in 0..11 {
            for x in 0..11 {
                mp.mesh
                    .positions
                    .push(Vec2::new(x as f32 * 10.0, y as f32 * 10.0));
                mp.mesh
                    .uvs
                    .push(Vec2::new(x as f32 / 10.0, y as f32 / 10.0));
            }
        }
        for y in 0..10u32 {
            for x in 0..10u32 {
                let i = y * 11 + x;
                mp.mesh.triangles.extend([i, i + 1, i + 11]);
                mp.mesh.triangles.extend([i + 1, i + 12, i + 11]);
            }
        }

        let mut p = Project::new("Rig Test");
        p.next_id = 500;
        let mut layer = Layer::new(LAYER, "Puppet");
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

    fn run(p: &mut Project, stack: &mut UndoStack, cmd: SetSkeleton) {
        apply_command(p, stack, Box::new(cmd)).expect("apply");
        stack.break_merge();
    }

    fn skeleton(p: &Project) -> &SkeletonData {
        match &p.puppets[&PUPPET].kind {
            PuppetKind::Mesh(m) => &m.skeleton,
            _ => unreachable!(),
        }
    }

    fn attachments_len(p: &Project) -> usize {
        match &p.puppets[&PUPPET].kind {
            PuppetKind::Mesh(m) => m.attachments.entries.len(),
            _ => unreachable!(),
        }
    }

    #[test]
    fn the_first_joint_is_pinned_and_later_ones_are_free() {
        let mut p = project();
        let mut stack = UndoStack::new();

        let cmd = place_joint(&mut p, PUPPET, Vec2::new(50.0, 20.0)).unwrap();
        run(&mut p, &mut stack, cmd);
        let cmd = place_joint(&mut p, PUPPET, Vec2::new(50.0, 60.0)).unwrap();
        run(&mut p, &mut stack, cmd);

        let joints: Vec<&Joint> = skeleton(&p).joints.values().collect();
        assert!(joints[0].pinned, "the first joint anchors the rig");
        assert_eq!(joints[0].inv_mass, 0.0);
        assert!(!joints[1].pinned);
        assert_eq!(joints[1].inv_mass, 1.0);
    }

    #[test]
    fn a_bone_between_two_joints_attaches_vertices() {
        let mut p = project();
        let mut stack = UndoStack::new();

        let cmd = place_joint(&mut p, PUPPET, Vec2::new(50.0, 20.0)).unwrap();
        run(&mut p, &mut stack, cmd);
        let cmd = place_joint(&mut p, PUPPET, Vec2::new(50.0, 80.0)).unwrap();
        run(&mut p, &mut stack, cmd);

        let ids: Vec<JointId> = skeleton(&p).joints.keys().copied().collect();
        assert_eq!(attachments_len(&p), 0, "no bones yet, so no attachments");

        let cmd = place_bone(&mut p, PUPPET, ids[0], ids[1]).unwrap();
        run(&mut p, &mut stack, cmd);

        assert_eq!(skeleton(&p).bones.len(), 1);
        assert!(
            attachments_len(&p) > 0,
            "a bone with a 48px radius over a 100px mesh must catch vertices"
        );
    }

    #[test]
    fn a_duplicate_bone_is_refused_in_both_directions() {
        // a→b and b→a are the same spring; adding both doubles its stiffness
        // without anything on screen saying so.
        let mut p = project();
        let mut stack = UndoStack::new();
        let cmd = place_joint(&mut p, PUPPET, Vec2::new(10.0, 10.0)).unwrap();
        run(&mut p, &mut stack, cmd);
        let cmd = place_joint(&mut p, PUPPET, Vec2::new(90.0, 10.0)).unwrap();
        run(&mut p, &mut stack, cmd);
        let ids: Vec<JointId> = skeleton(&p).joints.keys().copied().collect();

        let cmd = place_bone(&mut p, PUPPET, ids[0], ids[1]).unwrap();
        run(&mut p, &mut stack, cmd);

        assert!(place_bone(&mut p, PUPPET, ids[0], ids[1]).is_none());
        assert!(place_bone(&mut p, PUPPET, ids[1], ids[0]).is_none());
        assert!(
            place_bone(&mut p, PUPPET, ids[0], ids[0]).is_none(),
            "and self-loops"
        );
    }

    #[test]
    fn deleting_a_joint_cascades_into_its_bones_and_reattaches() {
        let mut p = project();
        let mut stack = UndoStack::new();
        for pos in [
            Vec2::new(50.0, 10.0),
            Vec2::new(50.0, 50.0),
            Vec2::new(50.0, 90.0),
        ] {
            let cmd = place_joint(&mut p, PUPPET, pos).unwrap();
            run(&mut p, &mut stack, cmd);
        }
        let ids: Vec<JointId> = skeleton(&p).joints.keys().copied().collect();
        let cmd = place_bone(&mut p, PUPPET, ids[0], ids[1]).unwrap();
        run(&mut p, &mut stack, cmd);
        let cmd = place_bone(&mut p, PUPPET, ids[1], ids[2]).unwrap();
        run(&mut p, &mut stack, cmd);
        assert_eq!(skeleton(&p).bones.len(), 2);

        // Deleting the middle joint takes both bones with it.
        let cmd = delete_joint(&p, PUPPET, ids[1]).unwrap();
        run(&mut p, &mut stack, cmd);

        assert_eq!(skeleton(&p).joints.len(), 2);
        assert_eq!(
            skeleton(&p).bones.len(),
            0,
            "every bone naming the joint must go with it"
        );
        assert_eq!(
            attachments_len(&p),
            0,
            "and the attachments those bones held must not dangle"
        );

        // And one undo restores all of it — the reason this is a snapshot.
        stack.undo(&mut p).unwrap().expect("revert");
        assert_eq!(skeleton(&p).joints.len(), 3);
        assert_eq!(skeleton(&p).bones.len(), 2);
        assert!(attachments_len(&p) > 0);
    }

    #[test]
    fn widening_a_radius_catches_more_vertices() {
        let mut p = project();
        let mut stack = UndoStack::new();
        let cmd = place_joint(&mut p, PUPPET, Vec2::new(50.0, 30.0)).unwrap();
        run(&mut p, &mut stack, cmd);
        let cmd = place_joint(&mut p, PUPPET, Vec2::new(50.0, 70.0)).unwrap();
        run(&mut p, &mut stack, cmd);
        let ids: Vec<JointId> = skeleton(&p).joints.keys().copied().collect();
        let cmd = place_bone(&mut p, PUPPET, ids[0], ids[1]).unwrap();
        run(&mut p, &mut stack, cmd);

        let narrow = attachments_len(&p);
        let bone_id = *skeleton(&p).bones.keys().next().unwrap();

        let cmd = set_attach_radius(&p, PUPPET, bone_id, 200.0).unwrap();
        run(&mut p, &mut stack, cmd);

        assert!(
            attachments_len(&p) > narrow,
            "a 200px radius must reach more of a 100px mesh than 48px did"
        );
    }

    #[test]
    fn joint_hit_testing_picks_the_nearest_within_range() {
        let mut p = project();
        let mut stack = UndoStack::new();
        let cmd = place_joint(&mut p, PUPPET, Vec2::new(20.0, 20.0)).unwrap();
        run(&mut p, &mut stack, cmd);
        let cmd = place_joint(&mut p, PUPPET, Vec2::new(30.0, 20.0)).unwrap();
        run(&mut p, &mut stack, cmd);
        let ids: Vec<JointId> = skeleton(&p).joints.keys().copied().collect();

        assert_eq!(joint_at(&p, PUPPET, Vec2::new(21.0, 20.0)), Some(ids[0]));
        assert_eq!(joint_at(&p, PUPPET, Vec2::new(29.0, 20.0)), Some(ids[1]));
        assert_eq!(
            joint_at(&p, PUPPET, Vec2::new(60.0, 60.0)),
            None,
            "far from every joint is a miss, not the least-bad joint"
        );
    }

    #[test]
    fn rig_edits_never_touch_the_mesh() {
        // The reason SetSkeleton reports SkeletonChanged rather than
        // MeshRebuilt: rigging must never pay for a GPU mesh rebuild.
        let mut p = project();
        let mut stack = UndoStack::new();
        let before = match &p.puppets[&PUPPET].kind {
            PuppetKind::Mesh(m) => (m.mesh.positions.len(), m.mesh.triangles.len()),
            _ => unreachable!(),
        };

        let cmd = place_joint(&mut p, PUPPET, Vec2::new(50.0, 50.0)).unwrap();
        run(&mut p, &mut stack, cmd);

        let after = match &p.puppets[&PUPPET].kind {
            PuppetKind::Mesh(m) => (m.mesh.positions.len(), m.mesh.triangles.len()),
            _ => unreachable!(),
        };
        assert_eq!(before, after);
    }
}
