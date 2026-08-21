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
use animus_runtime::{CompiledRigRef, DocumentRes, PuppetRoot, PuppetSolver, RenderScale};
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
/// The mesh wireframe, on its own so it can be drawn thinner.
///
/// A separate config group rather than a thinner colour: line width is a
/// property of the group, and the wireframe is the one thing here the eye
/// should be able to look *past*. Bones and joints are the subject; the mesh
/// is the paper they are drawn on.
#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct WireGizmos;

/// Bone width, in screen pixels. Legible zoomed out, per spec §10.3.
const RIG_LINE_WIDTH: f32 = 2.0;
/// The wireframe, a quarter thinner.
const WIRE_LINE_WIDTH: f32 = RIG_LINE_WIDTH * 0.75;

pub fn setup(mut store: ResMut<GizmoConfigStore>) {
    // Always in front of the artwork. The rig is the thing being edited and
    // the drawing is what it is being edited *against*, so a joint hidden
    // behind an opaque shoulder is a joint the operator cannot rig.
    let (config, _) = store.config_mut::<DefaultGizmoConfigGroup>();
    config.render_layers = RenderLayers::layer(EDITOR_LAYER);
    config.line.width = RIG_LINE_WIDTH;
    config.depth_bias = -1.0;

    let (wire, _) = store.config_mut::<WireGizmos>();
    wire.render_layers = RenderLayers::layer(EDITOR_LAYER);
    wire.line.width = WIRE_LINE_WIDTH;
    // A shade less forward than the rig, so a bone crossing the mesh reads as
    // being on top of it rather than fighting it for the same pixels.
    wire.depth_bias = -0.9;
}

/// A filled dot.
///
/// Bevy's gizmos draw outlines, and an outline nine pixels across reads as a
/// ring rather than a handle; concentric rings fill it. Drawn at a size in
/// *screen* pixels so a joint is the same target at every zoom — the same
/// size the hit test uses (`rig::JOINT_SCREEN_RADIUS_PX`).
fn dot(gizmos: &mut Gizmos, pos: Vec3, radius: f32, fill: Color) {
    const RINGS: usize = 6;
    for i in 1..=RINGS {
        let r = radius * i as f32 / RINGS as f32;
        gizmos.circle(Isometry3d::from_translation(pos), r, fill);
    }
}

/// The stage: what the audience will actually see.
///
/// A dashed rectangle at the output's own size, drawn faintly so it frames the
/// work without competing with it. It exists because until now nothing in the
/// editor said where the picture ends — an operator could place a puppet
/// perfectly on a viewport that shows more than the projector does, and only
/// find out in the venue.
///
/// **Nothing is clipped to it.** A puppet half outside the frame is a shot: a
/// character entering from the wing, or one shoulder filling the screen. The
/// line marks the edge; it does not enforce it.
pub fn draw_stage(
    mut gizmos: Gizmos,
    doc: Res<DocumentRes>,
    scale: Res<RenderScale>,
    state: Res<EditorState>,
) {
    let ppu = if scale.ppu > 0.0 { scale.ppu } else { 100.0 };
    let half = Vec2::new(
        doc.0.stage.canvas[0] as f32 / ppu * 0.5,
        doc.0.stage.canvas[1] as f32 / ppu * 0.5,
    );
    if !half.x.is_finite() || half.x <= 0.0 || half.y <= 0.0 {
        return;
    }

    // Behind the rig but in front of the artwork, and always at the same
    // faint weight: this is furniture, not state, so it takes the bottom of
    // the ink ramp and never a signal colour.
    let ink = colour(theme::gizmo::STAGE_FRAME);
    let z = 0.0;
    let corners = [
        Vec3::new(-half.x, -half.y, z),
        Vec3::new(half.x, -half.y, z),
        Vec3::new(half.x, half.y, z),
        Vec3::new(-half.x, half.y, z),
    ];
    // Dashes sized in world units from the stage itself, so the frame reads
    // the same at any zoom rather than turning into a solid line when the
    // operator zooms out.
    let dash = (half.x * 2.0) / 96.0;
    for i in 0..4 {
        dashed(&mut gizmos, corners[i], corners[(i + 1) % 4], dash, ink);
    }

    // The title-safe area: a 5% inset, off by default.
    //
    // Separate from the output frame above rather than replacing it, because
    // they answer different questions. The frame is where the picture ends;
    // this is where a projector's overscan, a screen's bezel or a badly
    // masked surface can start eating it. A face on the line is a face that
    // survives the studio and loses an ear in the venue.
    if state.overlays.safe {
        let inset = half * 0.9;
        let safe = [
            Vec3::new(-inset.x, -inset.y, z),
            Vec3::new(inset.x, -inset.y, z),
            Vec3::new(inset.x, inset.y, z),
            Vec3::new(-inset.x, inset.y, z),
        ];
        let safe_ink = colour(theme::gizmo::SAFE_FRAME);
        for i in 0..4 {
            dashed(
                &mut gizmos,
                safe[i],
                safe[(i + 1) % 4],
                dash * 0.6,
                safe_ink,
            );
        }
    }
}

/// A dashed segment, because `Gizmos` has no dash pattern.
fn dashed(gizmos: &mut Gizmos, a: Vec3, b: Vec3, dash: f32, ink: Color) {
    let span = b - a;
    let len = span.length();
    if len < 1e-6 || dash <= 0.0 {
        return;
    }
    let steps = (len / (dash * 2.0)).ceil() as usize;
    let steps = steps.clamp(1, 512);
    for i in 0..steps {
        let t0 = (i as f32 * dash * 2.0) / len;
        let t1 = ((i as f32 * dash * 2.0) + dash) / len;
        if t0 >= 1.0 {
            break;
        }
        gizmos.line(a + span * t0, a + span * t1.min(1.0), ink);
    }
}

/// The selection box around a chosen puppet, with corner handles.
///
/// Drawn only in BUILD. On the stage the puppet is the show, and a white box
/// round it would be the one thing in the frame that is not the performance.
/// The rotation gizmo: a ring round the selected joint with a handle on it.
///
/// **A dial belongs where the limb is.** Rotating from a panel means looking
/// away from the thing being rotated, and at the moment an operator is
/// judging an angle by eye that is the one place they cannot afford to look.
/// The panel keeps its dial for typing an exact number; this is for aiming.
///
/// Drawn only around a selected joint that has something below it. A ring on
/// a fingertip would be a control that does nothing, and the sentence in the
/// inspector saying so is easy to miss when your eyes are on the stage.
pub fn draw_rotation_gizmo(
    mut gizmos: Gizmos,
    doc: Res<DocumentRes>,
    scale: Res<RenderScale>,
    state: Res<EditorState>,
    rotations: Res<animus_runtime::LiveRotations>,
    target: Option<Res<crate::viewport::ViewportTarget>>,
    roots: Query<(&PuppetRoot, &CompiledRigRef, &PuppetSolver)>,
) {
    let Selection::Joint(puppet, joint) = state.selection else {
        return;
    };
    if !crate::hit::puppet_visible(&doc.0, puppet) {
        return;
    }
    let Some(PuppetKind::Mesh(mesh)) = doc.0.puppets.get(&puppet).map(|p| &p.kind) else {
        return;
    };
    if animus_core::skeleton::rig_tree(&mesh.skeleton)
        .descendants(joint)
        .is_empty()
    {
        return;
    }

    // Where the joint is *now*, so the ring follows a limb the sequencer or
    // a hand is moving rather than hovering over where it used to rest.
    let Some(centre) = roots
        .iter()
        .find(|(r, _, _)| r.0 == puppet)
        .and_then(|(_, rig, solver)| {
            let dense = rig.0.joint_index(joint)?;
            let img = match state.mode {
                EditMode::Rig => rig.0.joint_rest(dense as usize)?,
                _ => *solver.0.positions().get(dense as usize)?,
            };
            crate::hit::img_to_stage(&doc.0, puppet, scale.ppu, img)
        })
    else {
        return;
    };

    // A fixed size on screen, not in the world: a gizmo that shrank with the
    // zoom would be unusable at exactly the magnification an operator uses
    // to place a joint precisely.
    let world_per_pixel = target.as_ref().map(|t| t.world_per_pixel).unwrap_or(0.01);
    let radius = ROTATION_GIZMO_RADIUS_PX * world_per_pixel;
    let z = 0.0;
    let angle = rotations.get(puppet, joint);
    let live = angle.abs() > 1e-3;
    let ink = colour(if live {
        theme::DATA_CYAN
    } else {
        theme::gizmo::SELECTED_RING
    });

    // The track, dashed, so it reads as somewhere to grab rather than as a
    // circle the puppet is inside.
    //
    // Brighter than the wireframe it sits on top of. Drawn at the same
    // weight, the ring disappeared into the mesh at exactly the zoom where
    // an operator would be using it — a control nobody can find is a
    // control that is not there.
    let track = colour(theme::DIM);
    let steps = 48;
    for i in 0..steps {
        let a0 = std::f32::consts::TAU * i as f32 / steps as f32;
        let a1 = std::f32::consts::TAU * (i as f32 + 0.55) / steps as f32;
        gizmos.line(
            on_ring(centre, radius, a0, z),
            on_ring(centre, radius, a1, z),
            track,
        );
    }

    // The arc from twelve o'clock to the angle, in whichever direction the
    // angle went: an arc that always ran one way would show +10 and -10 the
    // same.
    if live {
        let arc_steps = 40;
        for i in 0..arc_steps {
            let t0 = angle * i as f32 / arc_steps as f32;
            let t1 = angle * (i as f32 + 1.0) / arc_steps as f32;
            gizmos.line(
                on_ring(centre, radius, up(t0), z),
                on_ring(centre, radius, up(t1), z),
                ink,
            );
        }
    }

    // The spoke and the handle: where the operator takes hold.
    let handle = on_ring(centre, radius, up(angle), z);
    gizmos.line(centre.extend(z), handle, ink);
    dot(
        &mut gizmos,
        handle,
        ROTATION_HANDLE_PX * world_per_pixel,
        ink,
    );
}

/// Screen-space radius of the rotation ring, in pixels.
pub const ROTATION_GIZMO_RADIUS_PX: f32 = 74.0;
/// Screen-space radius of its handle.
pub const ROTATION_HANDLE_PX: f32 = 6.0;

/// Zero at twelve o'clock, positive clockwise — the same convention the
/// panel's dial uses, and the same one image space implies with Y down.
fn up(angle: f32) -> f32 {
    angle - std::f32::consts::FRAC_PI_2
}

fn on_ring(centre: Vec2, radius: f32, angle: f32, z: f32) -> Vec3 {
    // World Y is up while image space is Y down, so the sine is negated:
    // without it the gizmo would turn the opposite way from the limb it is
    // attached to, which is the kind of wrongness that is obvious in motion
    // and invisible in a screenshot.
    Vec3::new(
        centre.x + angle.cos() * radius,
        centre.y - angle.sin() * radius,
        z,
    )
}

pub fn draw_selection_box(
    mut gizmos: Gizmos,
    doc: Res<DocumentRes>,
    scale: Res<RenderScale>,
    state: Res<EditorState>,
    target: Option<Res<crate::viewport::ViewportTarget>>,
) {
    if state.mode != EditMode::Rig {
        return;
    }
    let puppet = match state.selection {
        Selection::Puppet(p) => p,
        Selection::Joint(p, _) | Selection::Bone(p, _) => p,
        _ => return,
    };
    if !crate::hit::puppet_visible(&doc.0, puppet) {
        return;
    }
    let Some((lo, hi)) = crate::hit::selection_box(&doc.0, puppet, scale.ppu) else {
        return;
    };

    let world_per_pixel = target.as_ref().map(|t| t.world_per_pixel).unwrap_or(0.01);
    let ink = colour(theme::gizmo::SELECTED_RING);
    let z = 0.0;
    let corners = [
        Vec3::new(lo.x, lo.y, z),
        Vec3::new(hi.x, lo.y, z),
        Vec3::new(hi.x, hi.y, z),
        Vec3::new(lo.x, hi.y, z),
    ];
    for i in 0..4 {
        gizmos.line(corners[i], corners[(i + 1) % 4], ink);
    }

    // Handles at the size they can be grabbed, which is the size the hit test
    // uses. A handle drawn smaller than its target is a lie about where to
    // aim; drawn larger, it covers artwork it does not own.
    let r = crate::rig::JOINT_SCREEN_RADIUS_PX * world_per_pixel;
    for c in corners {
        gizmos.rect(Isometry3d::from_translation(c), Vec2::splat(r * 2.0), ink);
        gizmos.rect(Isometry3d::from_translation(c), Vec2::splat(r * 1.2), ink);
    }
}

/// Draw every puppet's rig from the live solver state.
#[allow(clippy::too_many_arguments)]
pub fn draw_rigs(
    mut gizmos: Gizmos,
    doc: Res<DocumentRes>,
    scale: Res<RenderScale>,
    state: Res<EditorState>,
    target: Option<Res<crate::viewport::ViewportTarget>>,
    targets: Option<Res<animus_runtime::JointTargets>>,
    roots: Query<(
        &PuppetRoot,
        &CompiledRigRef,
        &PuppetSolver,
        &GlobalTransform,
    )>,
    skins: Query<(&animus_runtime::PuppetMesh, &animus_runtime::MeshInfluences)>,
    mut wire_gizmos: Gizmos<WireGizmos>,
) {
    // A joint is drawn the size it can be grabbed, which means a size in
    // screen pixels converted to world units at the current zoom. The
    // previous `ppu * 0.04` was neither: it read as image pixels and was
    // world units, so every joint was drawn a hundred times too large — the
    // white discs that swamped the artwork and still could not be clicked.
    let world_per_pixel = target.as_ref().map(|t| t.world_per_pixel).unwrap_or(0.01);
    let joint_radius = crate::rig::JOINT_SCREEN_RADIUS_PX * world_per_pixel;

    for (root, rig, solver, xf) in &roots {
        let Some(puppet) = doc.0.puppets.get(&root.0) else {
            continue;
        };
        let PuppetKind::Mesh(mp) = &puppet.kind else {
            continue;
        };
        // A hidden layer takes its rig with it. The artwork is already gone —
        // `layer_visibility` hides the mesh entity — and leaving the
        // wireframe, bones and joints behind would draw a skeleton over an
        // empty stage.
        if !crate::hit::puppet_visible(&doc.0, root.0) {
            continue;
        }
        // The layer's full placement, not just its offset. Applying only the
        // translation left every gizmo at the unscaled size the moment a
        // puppet was resized: one puppet's artwork, another puppet's rig.
        let z = xf.translation().z;
        let to_stage = |img: Vec2| -> Option<Vec3> {
            crate::hit::img_to_stage(&doc.0, root.0, scale.ppu, img).map(|w| w.extend(z))
        };

        // In live mode the rig follows the solver; in edit mode it shows the
        // authored rest pose, because that is what the operator is editing.
        let joint_world = |dense: usize| -> Option<Vec3> {
            let img = match state.mode {
                // RIG shows the rest pose, because rest is what it edits.
                // EDIT and PERFORM both show the solver, because the pose on
                // screen is the thing being posed or performed.
                EditMode::Rig => rig.0.joint_rest(dense)?,
                EditMode::Edit | EditMode::Live => *solver.0.positions().get(dense)?,
            };
            to_stage(img)
        };

        // Wireframe, at the bottom of the ink ramp. Skipped beyond a budget:
        // spec §10.3 warns 10k vertices is 30k segments, and a rig editor
        // does not need the wireframe to stay correct at that density —
        // culling it beats stuttering.
        const WIREFRAME_TRIANGLE_BUDGET: usize = 8_000;
        if state.overlays.mesh && mp.mesh.triangles.len() / 3 <= WIREFRAME_TRIANGLE_BUDGET {
            // Skinned on the CPU with the same weights the GPU has, so the
            // wireframe sits on the artwork instead of beside it.
            //
            // Drawing it at the bind pose was cheaper and wrong in a way that
            // was easy to miss until something moved: the operator saw the
            // puppet twice, once deformed and once not, and read the still one
            // as "the mesh did not come back".
            let influences = skins
                .iter()
                .find(|(m, _)| m.0 == root.0)
                .map(|(_, inf)| inf);
            let posed: Option<Vec<Vec2>> = match (state.mode, influences) {
                (EditMode::Live, Some(inf)) => Some(skin_positions(
                    &mp.mesh.positions,
                    inf,
                    &rig.0,
                    solver.0.positions(),
                )),
                // Edit mode shows the authored rest pose, which the document
                // positions already are.
                _ => None,
            };
            let at = |i: u32| -> Vec2 {
                match &posed {
                    Some(p) => p[i as usize],
                    None => mp.mesh.positions[i as usize],
                }
            };

            let wire = colour(theme::gizmo::WIREFRAME);
            for tri in mp.mesh.triangles.chunks_exact(3) {
                let p = |i: u32| to_stage(at(i)).unwrap_or(Vec3::ZERO);
                let (a, b, c) = (p(tri[0]), p(tri[1]), p(tri[2]));
                wire_gizmos.line(a, b, wire);
                wire_gizmos.line(b, c, wire);
                wire_gizmos.line(c, a, wire);
            }
        }

        // Bones, from the same dense order the skinning uses.
        for b in 0..if state.overlays.bones {
            rig.0.bone_count()
        } else {
            0
        } {
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
            // The selected joint is drawn whatever the toggle says: turning
            // joints off to see the artwork must not also lose track of what
            // the inspector is describing.
            let selected = state.selection == Selection::Joint(root.0, joint.id);
            if !state.overlays.joints && !selected {
                continue;
            }
            let Some(dense) = rig.0.joint_index(joint.id) else {
                continue;
            };
            let Some(pos) = joint_world(dense as usize) else {
                continue;
            };
            // Being pulled right now — by a hand today, by a binding or a
            // clip in M2. The one place the Signal Rule spends coral on a
            // gizmo, and it spends it on the thing that is actually live.
            let driven = targets
                .as_ref()
                .is_some_and(|t| t.0.contains_key(&(root.0, joint.id)));

            let fill = if driven {
                theme::gizmo::DRIVEN
            } else {
                theme::gizmo::JOINT
            };

            if joint.pinned {
                let half = joint_radius * 0.9;
                gizmos.rect(
                    Isometry3d::from_translation(pos),
                    Vec2::splat(half * 2.0),
                    colour(fill),
                );
                gizmos.rect(
                    Isometry3d::from_translation(pos),
                    Vec2::splat(half),
                    colour(fill),
                );
            } else {
                dot(&mut gizmos, pos, joint_radius, colour(fill));
            }

            // Selection is a ring you cannot miss, drawn outside the dot so
            // it never hides the thing it marks. The old faint veil was
            // invisible against artwork, which left "did my click land?"
            // unanswerable — the question this ring exists to answer.
            if selected {
                for r in [1.7, 1.85, 2.0] {
                    gizmos.circle(
                        Isometry3d::from_translation(pos),
                        joint_radius * r,
                        colour(theme::gizmo::SELECTED_RING),
                    );
                }
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

/// Skin mesh vertices on the CPU, in **image space**.
///
/// Mirrors what the GPU does, from the same weights and the same joints. Each
/// bone contributes `current ∘ rest⁻¹` applied to the vertex, blended by
/// weight — the same composition `writeback_bones` builds for the render
/// palette, only expressed as a 2D similarity because that is all a bone is
/// here: an origin at joint A, an angle along A→B, and a stretch.
///
/// Working in image space rather than world is deliberate. Image Y points
/// down and world Y up, so an angle in one is the negative of the angle in
/// the other — but the rest-inverse cancels that consistently, and converting
/// the *result* with `img_to_world` lands exactly where the GPU puts it. Doing
/// half the work in each space is how this goes wrong.
fn skin_positions(
    rest: &[Vec2],
    inf: &animus_runtime::MeshInfluences,
    rig: &animus_core::solver::CompiledRig,
    now: &[Vec2],
) -> Vec<Vec2> {
    // One entry per bone: where it was, where it is.
    let bones: Vec<Option<(Vec2, Vec2, f32, f32)>> = (0..rig.bone_count())
        .map(|b| {
            let (ja, jb) = rig.bone_joints(b)?;
            let a_rest = rig.joint_rest(ja as usize)?;
            let b_rest = rig.joint_rest(jb as usize)?;
            let a_now = *now.get(ja as usize)?;
            let b_now = *now.get(jb as usize)?;
            let d_rest = b_rest - a_rest;
            let d_now = b_now - a_now;
            let len_rest = d_rest.length();
            if len_rest < 1e-6 {
                return None;
            }
            // Angle delta and stretch, both along the bone's own axis.
            let angle = d_now.y.atan2(d_now.x) - d_rest.y.atan2(d_rest.x);
            let stretch = d_now.length() / len_rest;
            Some((a_rest, a_now, angle, stretch))
        })
        .collect();

    rest.iter()
        .enumerate()
        .map(|(v, p)| {
            let idx = inf.joint_index.get(v).copied().unwrap_or([0; 4]);
            let wts = inf.joint_weight.get(v).copied().unwrap_or([0.0; 4]);
            let mut out = Vec2::ZERO;
            let mut total = 0.0_f32;
            for k in 0..4 {
                let w = wts[k];
                if w <= 0.0 {
                    continue;
                }
                let Some(Some((a_rest, a_now, angle, stretch))) =
                    bones.get(idx[k] as usize).copied()
                else {
                    continue;
                };
                // Into the bone's rest frame, stretch along its axis, then
                // out through where the bone is now.
                let local = *p - a_rest;
                let (sin, cos) = angle.sin_cos();
                let rotated =
                    Vec2::new(local.x * cos - local.y * sin, local.x * sin + local.y * cos);
                out += (a_now + rotated * stretch) * w;
                total += w;
            }
            // A vertex with no usable influence keeps its rest position
            // rather than collapsing to the origin.
            if total > 1e-6 { out / total } else { *p }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use animus_core::doc::{Bone, Joint, SkeletonData, SolverConfig};
    use animus_core::ids::{BoneId, JointId};
    use animus_core::solver::CompiledRig;
    use animus_runtime::MeshInfluences;

    /// One bone from the origin to (100, 0), and two vertices on it.
    fn rig_and_mesh() -> (CompiledRig, Vec<Vec2>, MeshInfluences) {
        let mut skel = SkeletonData::default();
        skel.joints.insert(
            JointId(1),
            Joint {
                id: JointId(1),
                name: "root".into(),
                rest: Vec2::ZERO,
                rest_angle: 0.0,
                inv_mass: 0.0,
                pinned: true,
            },
        );
        skel.joints.insert(
            JointId(2),
            Joint {
                id: JointId(2),
                name: "tip".into(),
                rest: Vec2::new(100.0, 0.0),
                rest_angle: 0.0,
                inv_mass: 1.0,
                pinned: false,
            },
        );
        skel.bones.insert(
            BoneId(1),
            Bone {
                id: BoneId(1),
                name: "bone".into(),
                a: JointId(1),
                b: JointId(2),
                rest_length: None,
                stiffness: 0.9,
                damping: 0.0,
                length_mul: 1.0,
                attach_radius: 50.0,
            },
        );
        let rig = CompiledRig::build(&skel, &SolverConfig::default());
        let positions = vec![Vec2::new(50.0, 0.0), Vec2::new(100.0, 20.0)];
        let influences = MeshInfluences {
            joint_index: vec![[0, 0, 0, 0]; 2],
            joint_weight: vec![[1.0, 0.0, 0.0, 0.0]; 2],
        };
        (rig, positions, influences)
    }

    /// **The rule, as arithmetic.** With the solver sitting on its rest pose,
    /// skinning must be the identity — every vertex exactly where the artist
    /// put it.
    ///
    /// This is what makes "nothing playing means the mesh is in its original
    /// position" a guarantee rather than an observation. Any drift here shows
    /// up as a puppet that never quite comes home, which is very hard to see
    /// and very easy to introduce.
    #[test]
    fn at_rest_the_skinned_mesh_is_the_original_mesh() {
        let (rig, positions, inf) = rig_and_mesh();
        // Walk out until the rig stops answering, rather than reaching for
        // a private count.
        let rest: Vec<Vec2> = (0..).map_while(|i| rig.joint_rest(i)).collect();
        assert_eq!(rest.len(), 2, "the fixture has two joints");

        let posed = skin_positions(&positions, &inf, &rig, &rest);

        for (before, after) in positions.iter().zip(posed.iter()) {
            assert!(
                before.distance(*after) < 1e-4,
                "a vertex moved at rest: {before:?} -> {after:?}"
            );
        }
    }

    /// And when the bone does move, the wireframe goes with it — the failure
    /// this whole function exists to fix was a wireframe left behind at the
    /// bind pose while the artwork walked away.
    #[test]
    fn a_rotated_bone_carries_its_vertices_round_with_it() {
        let (rig, positions, inf) = rig_and_mesh();
        // Swing the tip a quarter turn: (100,0) becomes (0,100).
        let now = vec![Vec2::ZERO, Vec2::new(0.0, 100.0)];

        let posed = skin_positions(&positions, &inf, &rig, &now);

        // The vertex halfway along the bone lands halfway along the new one.
        assert!(
            posed[0].distance(Vec2::new(0.0, 50.0)) < 1e-3,
            "midpoint went to {:?}, expected (0, 50)",
            posed[0]
        );
        // The vertex 20 off the bone's axis keeps its offset, rotated.
        assert!(
            posed[1].distance(Vec2::new(-20.0, 100.0)) < 1e-3,
            "offset vertex went to {:?}, expected (-20, 100)",
            posed[1]
        );
    }

    /// A vertex nothing is attached to stays put rather than collapsing to
    /// the origin — the same fallback the GPU path needed.
    #[test]
    fn an_unweighted_vertex_keeps_its_place() {
        let (rig, positions, _) = rig_and_mesh();
        let none = MeshInfluences {
            joint_index: vec![[0, 0, 0, 0]; 2],
            joint_weight: vec![[0.0, 0.0, 0.0, 0.0]; 2],
        };
        let now = vec![Vec2::ZERO, Vec2::new(0.0, 100.0)];

        let posed = skin_positions(&positions, &none, &rig, &now);

        assert_eq!(posed, positions, "an unweighted vertex must not move");
    }
}
