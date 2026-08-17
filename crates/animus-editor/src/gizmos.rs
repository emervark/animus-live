//! Drawing the rig over the puppet: bones, joints, wireframe, radii.
//!
//! Everything here goes to `RenderLayers::layer(1)`, which the output
//! window's camera does not carry — M0-4 confirmed by eye that layer-1
//! content never reaches the projector. The `EditorOnly` component makes
//! the same fact greppable.
//!
//! Colours come from the ink ramp, not the signal set: a wireframe is not a
//! state (see `theme::gizmo` and the Signal Rule). The two exceptions mean
//! exactly what they mean everywhere else — cyan is a joint bound to a live
//! channel, coral is a joint being driven right now.
//!
//! Everything is drawn from the **document plus solver state**, never read
//! back from the GPU mesh. That is the one-way rule again: the scene is a
//! projection, and so is this.

use animus_core::doc::PuppetKind;
use animus_runtime::{
    CompiledRigRef, DocumentRes, PuppetRoot, PuppetSolver, RenderScale, img_to_world, puppet_pivot,
};
use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;

use crate::state::{EditMode, EditorState, Selection};
use crate::theme;

/// The render layer editor helpers live on. Layer 0 is the show.
pub const EDITOR_LAYER: usize = 1;

fn colour(c: egui::Color32) -> Color {
    Color::srgba_u8(c.r(), c.g(), c.b(), c.a())
}

/// Configure gizmos to render on the editor layer only.
pub fn setup(mut store: ResMut<GizmoConfigStore>) {
    let (config, _) = store.config_mut::<DefaultGizmoConfigGroup>();
    config.render_layers = RenderLayers::layer(EDITOR_LAYER);
    // Screen-space width: bones stay legible zoomed out, per spec §10.3.
    config.line.width = 2.0;
}

/// Draw every puppet's rig from the live solver state.
pub fn draw_rigs(
    mut gizmos: Gizmos,
    doc: Res<DocumentRes>,
    scale: Res<RenderScale>,
    state: Res<EditorState>,
    roots: Query<(
        &PuppetRoot,
        &CompiledRigRef,
        &PuppetSolver,
        &GlobalTransform,
    )>,
) {
    for (root, rig, solver, xf) in &roots {
        let Some(puppet) = doc.0.puppets.get(&root.0) else {
            continue;
        };
        let PuppetKind::Mesh(mp) = &puppet.kind else {
            continue;
        };
        let pivot = puppet_pivot(mp);
        let offset = xf.translation();

        // In live mode the rig follows the solver; in edit mode it shows the
        // authored rest pose, because that is what the operator is editing.
        let joint_world = |dense: usize| -> Option<Vec3> {
            let img = match state.mode {
                EditMode::Live => *solver.0.positions().get(dense)?,
                EditMode::Edit => rig.0.joint_rest(dense)?,
            };
            Some(img_to_world(img, pivot, scale.ppu) + offset)
        };

        // Wireframe, at the bottom of the ink ramp. Skipped beyond a budget:
        // spec §10.3 warns 10k vertices is 30k segments, and a rig editor
        // does not need the wireframe to stay correct at that density —
        // culling it beats stuttering.
        const WIREFRAME_TRIANGLE_BUDGET: usize = 8_000;
        if mp.mesh.triangles.len() / 3 <= WIREFRAME_TRIANGLE_BUDGET {
            let wire = colour(theme::gizmo::WIREFRAME);
            for tri in mp.mesh.triangles.chunks_exact(3) {
                let p =
                    |i: u32| img_to_world(mp.mesh.positions[i as usize], pivot, scale.ppu) + offset;
                let (a, b, c) = (p(tri[0]), p(tri[1]), p(tri[2]));
                gizmos.line(a, b, wire);
                gizmos.line(b, c, wire);
                gizmos.line(c, a, wire);
            }
        }

        // Bones, from the same dense order the skinning uses.
        for b in 0..rig.0.bone_count() {
            let Some((ja, jb)) = rig.0.bone_joints(b) else {
                continue;
            };
            let (Some(a), Some(bp)) = (joint_world(ja as usize), joint_world(jb as usize)) else {
                continue;
            };
            gizmos.line(a, bp, colour(theme::gizmo::BONE));
        }

        // Joints. A pinned joint is a square — form carries the state.
        //
        // The dense index comes from the rig, never from enumeration order.
        // The two agree until `CompiledRig::build` skips a joint, at which
        // point every later joint would be drawn at its neighbour's
        // position — the same class of bug as using a BoneId as an index.
        for joint in mp.skeleton.joints.values() {
            let Some(dense) = rig.0.joint_index(joint.id) else {
                continue;
            };
            let Some(pos) = joint_world(dense as usize) else {
                continue;
            };
            let selected = state.selection == Selection::Joint(root.0, joint.id);
            let radius = scale.ppu * 0.04;

            if joint.pinned {
                let half = radius * 0.9;
                gizmos.rect(
                    Isometry3d::from_translation(pos),
                    Vec2::splat(half * 2.0),
                    colour(theme::gizmo::PINNED),
                );
            } else {
                gizmos.circle(
                    Isometry3d::from_translation(pos),
                    radius,
                    colour(theme::gizmo::JOINT),
                );
            }
            if selected {
                gizmos.circle(
                    Isometry3d::from_translation(pos),
                    radius * 1.8,
                    colour(theme::gizmo::SELECTION),
                );
            }
        }

        // The selected bone's attachment radius, as a circle at each end —
        // the visual for the one slider the rigging story has.
        if let Selection::Bone(pid, bid) = state.selection
            && pid == root.0
            && let Some(bone) = mp.skeleton.bones.get(&bid)
        {
            // The radius is authored in image pixels; world units are
            // pixels divided by pixels-per-unit. One conversion, spelled
            // once.
            let world_radius = bone.attach_radius / scale.ppu;
            for end in [bone.a, bone.b] {
                if let Some(dense) = rig.0.joint_index(end)
                    && let Some(pos) = joint_world(dense as usize)
                {
                    gizmos.circle(
                        Isometry3d::from_translation(pos),
                        world_radius,
                        colour(theme::gizmo::RADIUS_FILL),
                    );
                }
            }
        }
    }
}
