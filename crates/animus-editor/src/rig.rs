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

/// How big a joint is to the hand and to the eye, in **screen** pixels.
///
/// Screen pixels, not image pixels. A fixed 8-image-pixel radius is a
/// comfortable target on a 500px sketch and about two screen pixels on a
/// 2160px one, so grabbing a joint on a real drawing was a game of chance.
/// The gizmo is drawn at this same radius, which is the actual rule: **what
/// you can see is what you can grab.**
pub const JOINT_SCREEN_RADIUS_PX: f32 = 9.0;

/// The grab radius in image pixels, for the viewport's current zoom.
///
/// `world_per_pixel` is measured from the live camera rather than derived
/// from the projection — the viewport probes it, and M0-2 found the derived
/// version wrong by 28×.
pub fn grab_radius_img(world_per_pixel: f32, ppu: f32) -> f32 {
    (JOINT_SCREEN_RADIUS_PX * world_per_pixel * ppu).max(1.0)
}

/// Floor for a new bone's reach, in image pixels. Only binds for bones so
/// short that a proportional radius would attach nothing at all.
pub const MIN_ATTACH_RADIUS_PX: f32 = 48.0;

/// How far a new bone reaches, as a share of its own length.
pub const ATTACH_RADIUS_OF_LENGTH: f32 = 0.6;

/// A new bone's reach, derived from the bone itself.
///
/// **A fixed pixel radius cannot serve two drawings.** 48px is a comfortable
/// band on a 500px sketch and a sliver on a 2160px character — so a bone
/// drawn on a large drawing captured almost no vertices and the puppet
/// simply did not follow its new rig, which read as "joints don't bind to
/// the mesh". A reach that scales with the bone holds at every size: a thigh
/// grabs a wide band, a finger a narrow one. The radius stays authored
/// truth and the inspector's slider still overrides it.
pub fn default_attach_radius(a: Vec2, b: Vec2) -> f32 {
    ((b - a).length() * ATTACH_RADIUS_OF_LENGTH).max(MIN_ATTACH_RADIUS_PX)
}

fn mesh_puppet(project: &Project, id: PuppetId) -> Option<&MeshPuppet> {
    match &project.puppets.get(&id)?.kind {
        PuppetKind::Mesh(m) => Some(m),
        _ => None,
    }
}

/// The joint nearest to `pos`, if any is within `radius_img` image pixels.
///
/// **Hit-tested against where each joint is drawn, which is not always where
/// it rests.** In live mode the gizmo follows the solver, so a puppet that
/// has been pulled — or one a clip is looping — has its joints somewhere
/// else entirely. Testing the rest positions there means clicking a red dot
/// and hitting nothing, for every joint, from the first pull onward. That is
/// why the caller passes positions instead of the document: the one list is
/// what the eye sees and the hand aims at.
///
/// The radius is passed in for the same reason — it depends on the zoom, see
/// [`grab_radius_img`].
pub fn joint_at(joints: &[(JointId, Vec2)], pos: Vec2, radius_img: f32) -> Option<JointId> {
    joints
        .iter()
        .map(|(id, at)| (*id, (*at - pos).length()))
        .filter(|(_, d)| *d <= radius_img)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(id, _)| id)
}

/// Every joint's rest position, in the order the skeleton stores them.
///
/// The edit-mode list, and the fallback whenever a puppet has no solver
/// state yet.
pub fn joint_rest_positions(project: &Project, puppet: PuppetId) -> Vec<(JointId, Vec2)> {
    match mesh_puppet(project, puppet) {
        Some(mp) => mp
            .skeleton
            .joints
            .values()
            .map(|j| (j.id, j.rest))
            .collect(),
        None => Vec::new(),
    }
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
    let (rest_a, rest_b) = (skeleton.joints[&a].rest, skeleton.joints[&b].rest);

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
            attach_radius: default_attach_radius(rest_a, rest_b),
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

/// Delete a bone, leaving its joints, then re-attach.
///
/// The joints stay because they are positions an operator placed by hand,
/// and a spring is the cheap half to redraw. Deleting the joint is the other
/// command, and it cascades the other way.
pub fn delete_bone(project: &Project, puppet: PuppetId, bone: BoneId) -> Option<SetSkeleton> {
    let mp = mesh_puppet(project, puppet)?;
    if !mp.skeleton.bones.contains_key(&bone) {
        return None;
    }

    let mut skeleton = mp.skeleton.clone();
    skeleton.bones.shift_remove(&bone);

    let attachments = auto_attach(&mp.mesh, &skeleton);
    Some(SetSkeleton::new(
        puppet,
        "Delete bone",
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

    /// What a radius controls, now that no vertex is ever left behind.
    ///
    /// Every vertex has at least one bone in any case — the nearest one, as
    /// a fallback — so widening a radius no longer changes *whether* a
    /// vertex is attached. It changes how many bones share it, which is what
    /// the radius was always for: the blend across a limb's seam.
    #[test]
    fn widening_a_radius_blends_more_vertices_across_two_bones() {
        let mut p = project();
        let mut stack = UndoStack::new();
        for y in [20.0, 50.0, 80.0] {
            let cmd = place_joint(&mut p, PUPPET, Vec2::new(50.0, y)).unwrap();
            run(&mut p, &mut stack, cmd);
        }
        let ids: Vec<JointId> = skeleton(&p).joints.keys().copied().collect();
        let cmd = place_bone(&mut p, PUPPET, ids[0], ids[1]).unwrap();
        run(&mut p, &mut stack, cmd);
        let cmd = place_bone(&mut p, PUPPET, ids[1], ids[2]).unwrap();
        run(&mut p, &mut stack, cmd);

        let narrow = attachments_len(&p);
        let bones: Vec<BoneId> = skeleton(&p).bones.keys().copied().collect();
        for bone in bones {
            let cmd = set_attach_radius(&p, PUPPET, bone, 200.0).unwrap();
            run(&mut p, &mut stack, cmd);
        }

        assert!(
            attachments_len(&p) > narrow,
            "a wide radius must put more vertices under both bones at once;              {narrow} entries before, {} after",
            attachments_len(&p)
        );
    }

    /// No vertex is ever left with nothing to follow.
    ///
    /// An unattached vertex is multiplied by a zero weight sum on the GPU
    /// and collapses onto the puppet's origin, so a rig that reaches most of
    /// a drawing would put a spike through the rest of it.
    #[test]
    fn every_vertex_finds_a_bone_even_far_outside_the_radius() {
        let mut p = project();
        let mut stack = UndoStack::new();
        // Two joints in one corner: most of the 100x100 mesh is far outside
        // any reasonable radius of the bone between them.
        let cmd = place_joint(&mut p, PUPPET, Vec2::new(0.0, 0.0)).unwrap();
        run(&mut p, &mut stack, cmd);
        let cmd = place_joint(&mut p, PUPPET, Vec2::new(5.0, 0.0)).unwrap();
        run(&mut p, &mut stack, cmd);
        let ids: Vec<JointId> = skeleton(&p).joints.keys().copied().collect();
        let cmd = place_bone(&mut p, PUPPET, ids[0], ids[1]).unwrap();
        run(&mut p, &mut stack, cmd);

        let mesh_vertices = match &p.puppets[&PUPPET].kind {
            PuppetKind::Mesh(m) => m.mesh.positions.len(),
            _ => unreachable!(),
        };
        let attached: std::collections::HashSet<u32> = match &p.puppets[&PUPPET].kind {
            PuppetKind::Mesh(m) => m.attachments.entries.iter().map(|a| a.vertex).collect(),
            _ => unreachable!(),
        };
        assert_eq!(
            attached.len(),
            mesh_vertices,
            "every vertex must follow something, or it collapses to the origin"
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

        let r = 8.0;
        let at = joint_rest_positions(&p, PUPPET);
        assert_eq!(joint_at(&at, Vec2::new(21.0, 20.0), r), Some(ids[0]));
        assert_eq!(joint_at(&at, Vec2::new(29.0, 20.0), r), Some(ids[1]));
        assert_eq!(
            joint_at(&at, Vec2::new(60.0, 60.0), r),
            None,
            "far from every joint is a miss, not the least-bad joint"
        );
    }

    /// The hit test follows the joint, not the document.
    ///
    /// The bug this pins: joints were hit-tested against `joint.rest` while
    /// the gizmo was drawn at the solver's live position. One pull moved the
    /// puppet, and from then on every click on a red dot found nothing —
    /// which read as "recording works once and then selection dies".
    #[test]
    fn a_joint_is_grabbed_where_it_is_drawn_not_where_it_rests() {
        let rest = Vec2::new(100.0, 100.0);
        let live = Vec2::new(400.0, 260.0);
        let id = JointId(7);

        // Aiming at the drawn position hits it.
        assert_eq!(
            joint_at(&[(id, live)], live + Vec2::new(3.0, 0.0), 20.0),
            Some(id)
        );
        // Aiming at the rest position, while it is drawn elsewhere, does not.
        assert_eq!(joint_at(&[(id, live)], rest, 20.0), None);
        // And the nearest of several wins.
        let other = JointId(8);
        assert_eq!(
            joint_at(
                &[(id, live), (other, live + Vec2::new(15.0, 0.0))],
                live,
                20.0
            ),
            Some(id)
        );
    }

    /// The grab radius follows the zoom, because the *drawn* joint does.
    ///
    /// Zoomed out on a 2160px drawing, a joint covers a few image pixels per
    /// screen pixel and must still be grabbable; zoomed in, the same gesture
    /// must not sweep up its neighbour.
    #[test]
    fn the_grab_radius_tracks_the_zoom() {
        let ppu = 100.0;
        // Fitting a 2160px puppet to screen: ~3 image px per screen px.
        let zoomed_out = grab_radius_img(0.031, ppu);
        // One image pixel per screen pixel.
        let zoomed_in = grab_radius_img(0.01, ppu);

        assert!(
            zoomed_out > zoomed_in,
            "zooming out must widen the grab radius, got {zoomed_out} vs {zoomed_in}"
        );
        assert!(
            zoomed_out > 20.0,
            "a fit-to-screen puppet needs a target bigger than a few pixels, got {zoomed_out}"
        );
        assert!(
            grab_radius_img(0.0, ppu) >= 1.0,
            "a degenerate zoom must not make every joint unreachable"
        );
    }

    /// A new bone must actually take hold of the artwork it spans.
    ///
    /// The bug this pins: the reach was a fixed 48 image pixels, which on a
    /// 2160px character grabs a sliver — the rig existed, the mesh ignored
    /// it, and it read as "joints don't bind to the mesh".
    #[test]
    fn a_new_bones_reach_scales_with_the_bone() {
        let short = default_attach_radius(Vec2::ZERO, Vec2::new(30.0, 0.0));
        let long = default_attach_radius(Vec2::ZERO, Vec2::new(1200.0, 0.0));

        assert_eq!(
            short, MIN_ATTACH_RADIUS_PX,
            "a bone shorter than the floor still reaches the floor"
        );
        assert!(
            long > 600.0,
            "a bone spanning a large drawing must reach across it, got {long}"
        );
    }

    /// Deleting a bone leaves its joints; deleting a joint takes its bones.
    #[test]
    fn deleting_a_bone_keeps_its_joints_and_deleting_a_joint_takes_its_bones() {
        let mut p = project();
        let mut stack = UndoStack::new();
        let cmd = place_joint(&mut p, PUPPET, Vec2::new(20.0, 20.0)).unwrap();
        run(&mut p, &mut stack, cmd);
        let cmd = place_joint(&mut p, PUPPET, Vec2::new(80.0, 20.0)).unwrap();
        run(&mut p, &mut stack, cmd);
        let ids: Vec<JointId> = skeleton(&p).joints.keys().copied().collect();
        let cmd = place_bone(&mut p, PUPPET, ids[0], ids[1]).unwrap();
        run(&mut p, &mut stack, cmd);
        let bone = *skeleton(&p).bones.keys().next().unwrap();

        let cmd = delete_bone(&p, PUPPET, bone).unwrap();
        run(&mut p, &mut stack, cmd);
        assert!(skeleton(&p).bones.is_empty(), "the bone is gone");
        assert_eq!(skeleton(&p).joints.len(), 2, "its joints stayed");

        // And the other direction: a joint takes every bone that names it.
        let cmd = place_bone(&mut p, PUPPET, ids[0], ids[1]).unwrap();
        run(&mut p, &mut stack, cmd);
        let cmd = delete_joint(&p, PUPPET, ids[0]).unwrap();
        run(&mut p, &mut stack, cmd);
        assert_eq!(skeleton(&p).joints.len(), 1);
        assert!(
            skeleton(&p).bones.is_empty(),
            "a bone cannot outlive a joint it names"
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
