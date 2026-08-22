//! Driving a glTF model's own skeleton.
//!
//! A mesh puppet's joints are point masses the solver integrates. A model's
//! nodes are transforms written straight onto the scene — the physics does
//! not follow the model in, because the solver is two-dimensional and a glTF
//! skeleton is not.
//!
//! Everything *above* this is shared: a model's node carries a `JointId`,
//! appears in the rig tree, can be selected, rotated, bound to a channel and
//! shaped by an envelope, exactly as a mesh puppet's joint can. This module
//! is the one place that knows the difference.
//!
//! ## Finding the nodes
//!
//! A scene arrives asynchronously, so the entities do not exist when the
//! puppet is spawned. Discovery therefore runs until it succeeds rather than
//! once: it walks the root's descendants looking for `Name`s the document
//! already knows, and stops looking once it has them all.
//!
//! **Matched by name, not by spawn order.** Order is not something glTF
//! promises and not something Bevy promises either; a name is what the file
//! actually carries, and it is what the importer recorded ids against.

use bevy::platform::collections::HashMap;
use bevy::prelude::*;

use animus_core::doc::PuppetKind;
use animus_core::ids::{JointId, PuppetId};

use crate::components::PuppetRoot;
use crate::project::{DocumentRes, ModelRoot};
use crate::solve::{HeldJoint, JointTargets, LiveRotations};

/// The scene entity behind each of a model's nodes, once it has spawned.
#[derive(Component, Debug, Default)]
pub struct ModelNodes {
    pub by_joint: HashMap<JointId, Entity>,
    /// Each node's own local transform, as the file authored it.
    ///
    /// Kept because a driven rotation is composed **on top of** the pose the
    /// model came with, never in place of it. Writing an absolute rotation
    /// would throw away the bind pose — a shoulder that is meant to sit at
    /// forty degrees would snap to zero the moment anything drove it.
    pub rest: HashMap<JointId, Transform>,
    /// Each node's rest orientation in world terms, accumulated down the
    /// hierarchy as the nodes are found.
    ///
    /// **Needed because a bone's own axes are not the stage's axes.** A rig's
    /// local frames run along the bones, so "rotate about Z" means something
    /// different at every joint: on a spine it folds the torso forward, on an
    /// upper arm it twists. What an operator asks for on a stage is a turn in
    /// the plane the audience sees, which is a rotation about *world* Z — and
    /// converting that into a local one needs to know how the node is
    /// oriented to begin with.
    ///
    /// Accumulated here rather than read from `GlobalTransform`, because a
    /// scene that spawned this frame has not been through transform
    /// propagation yet, and a rest orientation captured as identity is a bone
    /// that turns the wrong way for the rest of the session.
    pub world_rest: HashMap<JointId, Quat>,
}

/// How far a bound position offset moves a model node, per image pixel.
///
/// Bindings are quoted in image pixels because that is what a 2D puppet
/// speaks, and a model is in metres. A hundred pixels of fader travel
/// reading as a hundred metres of shoulder would be the single most
/// startling thing in the application.
pub const PX_TO_MODEL: f32 = 0.01;

/// One node's swing: how far it is turned, and how fast.
#[derive(Debug, Clone, Copy, Default)]
pub struct Swing {
    pub angle: f32,
    pub vel: f32,
}

/// What the sequencer's hits have done to a model, and what is left of them.
///
/// **A model needs this and a cutout puppet does not.** The sequencer's whole
/// design rests on a hit being velocity rather than a pose — the mass-spring
/// solver takes the energy back out, so nothing anywhere decays anything and
/// two differently-tuned puppets read as two different characters. A glTF
/// skeleton has no solver behind it: the physics is two-dimensional and a rig
/// is not. Left alone, a hit on a model would be a limb that snapped somewhere
/// and stayed there.
///
/// So a model gets the smallest honest substitute: one damped spring per node,
/// in one dimension — the angle. It is deliberately *under*damped, because a
/// limb that returns to rest without ever passing it does not read as a limb.
#[derive(Resource, Debug, Default)]
pub struct ModelSwings(pub HashMap<(PuppetId, JointId), Swing>);

/// How hard the swing is pulled back to rest, in radians per second squared
/// per radian. `sqrt` of it is the natural frequency: about eleven radians a
/// second, so a swing takes a little over half a second to come and go.
const SWING_STIFFNESS: f32 = 120.0;
/// And how fast it loses energy, per second. Critical damping here would be
/// about twenty-two; this is a third of that, which is one clear overshoot and
/// a small second one — a limb, rather than a door closer.
const SWING_DAMPING: f32 = 8.0;
/// Below this the swing is over, and the entry goes rather than being carried
/// forever at a millionth of a degree.
const SWING_ASLEEP: f32 = 1e-4;

/// How far a swing released at one radian a second actually gets.
///
/// **A kick is speed and a caller means distance.** For an undamped spring the
/// two are related by the natural frequency alone, but this one is damped on
/// the way out, so it peaks short of `vel / ω`. The shortfall is fixed by the
/// two constants above and is measured from them by the test below rather than
/// guessed — the point of the number is that "swing about twenty-five degrees"
/// produces about twenty-five degrees, and a caller should not have to know
/// what a spring is to say it.
const SWING_REACH: f32 = 0.63 / 10.954;

impl ModelSwings {
    /// Give a node's swing more speed. Velocity, not a new angle — the same
    /// distinction `SolverState::kick` makes and for the same reason.
    pub fn kick(&mut self, puppet: PuppetId, joint: JointId, by: f32) {
        self.0.entry((puppet, joint)).or_default().vel += by;
    }

    /// Kick a node hard enough that its swing reaches about `peak` radians.
    ///
    /// The conversion lives here, beside the constants that decide it, so that
    /// a caller can ask for a gesture in the unit a gesture is measured in.
    pub fn swing_to(&mut self, puppet: PuppetId, joint: JointId, peak: f32) {
        self.kick(puppet, joint, peak / SWING_REACH);
    }

    pub fn angle(&self, puppet: PuppetId, joint: JointId) -> f32 {
        self.0.get(&(puppet, joint)).map(|s| s.angle).unwrap_or(0.0)
    }

    pub fn clear(&mut self) {
        self.0.clear();
    }
}

/// Let every swing swing.
pub fn settle_model_swings(time: Res<Time>, mut swings: ResMut<ModelSwings>) {
    // Clamped because a frame that took a quarter of a second — a window
    // dragged, a shader compiled — would otherwise integrate the spring past
    // stability and fling every limb off the stage.
    let dt = time.delta_secs().min(1.0 / 30.0);
    if dt <= 0.0 {
        return;
    }
    swings.0.retain(|_, s| {
        s.vel += (-SWING_STIFFNESS * s.angle - SWING_DAMPING * s.vel) * dt;
        s.angle += s.vel * dt;
        s.angle.abs() > SWING_ASLEEP || s.vel.abs() > SWING_ASLEEP
    });
}

/// Find the scene entities the document's nodes name.
pub fn discover_model_nodes(
    mut commands: Commands,
    doc: Res<DocumentRes>,
    roots: Query<(Entity, &PuppetRoot, Option<&ModelNodes>), With<ModelRoot>>,
    children: Query<&Children>,
    named: Query<(&Name, &Transform)>,
    transforms: Query<&Transform>,
) {
    for (entity, root, found) in &roots {
        let Some(PuppetKind::Model(model)) = doc.0.puppets.get(&root.0).map(|p| &p.kind) else {
            continue;
        };
        // Done once every node the document knows has been located. A model
        // whose file disagrees with the document — an edit outside, a
        // partial export — never completes, and retrying costs one walk of a
        // scene that is not going to change.
        if found.is_some_and(|f| f.by_joint.len() >= model.nodes.len()) {
            continue;
        }

        let mut by_joint = HashMap::default();
        let mut rest = HashMap::default();
        let mut world_rest = HashMap::default();
        // The orientation each entity inherits travels down with it, so a
        // node's rest-in-world falls out of the same walk that finds it.
        let mut stack = vec![(entity, Quat::IDENTITY)];
        while let Some((at, above)) = stack.pop() {
            let here = match transforms.get(at) {
                Ok(t) => above * t.rotation,
                Err(_) => above,
            };
            if let Ok((name, transform)) = named.get(at)
                && let Some(node) = model.node_named(name.as_str())
            {
                by_joint.insert(node.id, at);
                rest.insert(node.id, *transform);
                world_rest.insert(node.id, here);
            }
            if let Ok(kids) = children.get(at) {
                stack.extend(kids.iter().map(|k| (k, here)));
            }
        }

        if by_joint.is_empty() {
            continue;
        }
        let located = by_joint.len();
        commands.entity(entity).insert(ModelNodes {
            by_joint,
            rest,
            world_rest,
        });
        if located < model.nodes.len() {
            // Said once, when it settles: a model that is half-findable is a
            // model half its rig will silently refuse to drive.
            debug!(
                "model {:?}: located {located} of {} nodes so far",
                root.0,
                model.nodes.len()
            );
        }
    }
}

/// Apply rotations and offsets to the nodes they name.
///
/// Composed on the file's own pose rather than replacing it, and skipping
/// whatever is held — the same rule the sequencer and the bindings follow,
/// because a model's node is a joint like any other.
pub fn drive_model_nodes(
    rotations: Res<LiveRotations>,
    swings: Res<ModelSwings>,
    targets: Res<JointTargets>,
    held: Res<HeldJoint>,
    doc: Res<DocumentRes>,
    roots: Query<(&PuppetRoot, &ModelNodes)>,
    mut transforms: Query<&mut Transform>,
) {
    for (root, nodes) in &roots {
        let Some(PuppetKind::Model(model)) = doc.0.puppets.get(&root.0).map(|p| &p.kind) else {
            continue;
        };
        for node in &model.nodes {
            let Some(&entity) = nodes.by_joint.get(&node.id) else {
                continue;
            };
            if held.0 == Some((root.0, node.id)) {
                continue;
            }
            let Some(&rest) = nodes.rest.get(&node.id) else {
                continue;
            };

            // Added, not chosen between: a bound channel says where the limb
            // is held and a hit says what just happened to it, and a pattern
            // playing under a fader that is being ridden has to be both.
            let angle = rotations.get(root.0, node.id) + swings.angle(root.0, node.id);
            // A target is an absolute image-space position for a mesh
            // puppet; for a model node there is no image space, so what is
            // meaningful is how far it is from the node's own rest — which
            // is what a binding writes.
            let offset = targets
                .0
                .get(&(root.0, node.id))
                .map(|t| Vec2::new(t.x, t.y) * PX_TO_MODEL)
                .unwrap_or(Vec2::ZERO);

            if angle.abs() < 1e-4 && offset.length_squared() < 1e-8 {
                continue;
            }
            let Ok(mut transform) = transforms.get_mut(entity) else {
                continue;
            };
            // About world Z — the axis facing the audience — expressed in
            // this node's own frame. Turning a limb on a stage means turning
            // it in the plane the audience sees; using the node's local Z
            // instead means a spine folds forward and an arm twists, because
            // a rig's axes run along its bones and not along the room.
            let world_rest = nodes
                .world_rest
                .get(&node.id)
                .copied()
                .unwrap_or(Quat::IDENTITY);
            transform.rotation = rest.rotation * stage_turn(world_rest, angle);
            transform.translation = rest.translation + Vec3::new(offset.x, offset.y, 0.0);
        }
    }
}

/// A turn about the stage's own Z, written in a node's local frame.
///
/// Post-multiplying a node's rest rotation by this is the same as turning it
/// about world Z: `world_rest * stage_turn(world_rest, a) == Rz(a) *
/// world_rest`. That identity is the whole point, and the test below is its
/// statement — it is the difference between an arm swinging across the frame
/// and an arm twisting inside its own sleeve.
pub fn stage_turn(world_rest: Quat, angle: f32) -> Quat {
    let axis = (world_rest.inverse() * Vec3::Z).normalize_or(Vec3::Z);
    Quat::from_axis_angle(axis, angle)
}

/// The nodes a rotation on this joint would carry, for the inspector's
/// sentence and for anything else that needs the chain.
pub fn model_descendants(model: &animus_core::doc::ModelPuppet, joint: JointId) -> Vec<JointId> {
    model.descendants(joint)
}

/// Whether this puppet is a model, for the panels that ask.
pub fn is_model(doc: &DocumentRes, puppet: PuppetId) -> bool {
    matches!(
        doc.0.puppets.get(&puppet).map(|p| &p.kind),
        Some(PuppetKind::Model(_))
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use animus_core::doc::{ModelNode, ModelPuppet};
    use animus_core::ids::AssetId;

    fn chain() -> ModelPuppet {
        let mut m = ModelPuppet::new(AssetId(1));
        m.nodes = vec![
            ModelNode {
                id: JointId(1),
                name: "hips".into(),
                parent: None,
            },
            ModelNode {
                id: JointId(2),
                name: "spine".into(),
                parent: Some(JointId(1)),
            },
            ModelNode {
                id: JointId(3),
                name: "head".into(),
                parent: Some(JointId(2)),
            },
            ModelNode {
                id: JointId(4),
                name: "tail".into(),
                parent: Some(JointId(1)),
            },
        ];
        m
    }

    /// **A rotation carries everything below it**, on a model exactly as on
    /// a mesh puppet. A shoulder that turned while its own hand stayed put
    /// is not a shoulder.
    #[test]
    fn descendants_are_everything_below_and_nothing_above() {
        let m = chain();
        let below = model_descendants(&m, JointId(2));
        assert!(below.contains(&JointId(3)));
        assert!(!below.contains(&JointId(1)), "not the thing it hangs from");
        assert!(!below.contains(&JointId(4)), "nor a sibling");
        assert!(!below.contains(&JointId(2)), "nor itself");
    }

    #[test]
    fn a_root_carries_the_whole_model() {
        let below = model_descendants(&chain(), JointId(1));
        assert_eq!(below.len(), 3);
    }

    #[test]
    fn a_leaf_carries_nothing() {
        assert!(model_descendants(&chain(), JointId(3)).is_empty());
    }

    /// A model is in metres and a binding is in image pixels; a hundred
    /// pixels of fader travel reading as a hundred metres of shoulder would
    /// be the most startling thing in the application.
    #[test]
    fn pixel_offsets_arrive_at_a_scale_a_model_can_use() {
        const { assert!(PX_TO_MODEL > 0.0 && PX_TO_MODEL < 0.1) };
        assert!((60.0 * PX_TO_MODEL - 0.6).abs() < 1e-5, "60px is 60cm");
    }

    /// **A bone's axes are not the stage's axes**, and this is the identity
    /// that reconciles them. A spine bone in a biped rig points up its own X;
    /// turning it about its local Z folds the torso forward, which is not
    /// what "rotate" means to someone watching from the front.
    #[test]
    fn a_turn_reads_as_a_turn_whatever_way_the_bone_points() {
        let angle = 0.6;
        for rest in [
            Quat::IDENTITY,
            Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
            Quat::from_rotation_y(1.1),
            Quat::from_euler(EulerRot::XYZ, 0.3, -1.2, 2.0),
        ] {
            let composed = rest * stage_turn(rest, angle);
            let wanted = Quat::from_rotation_z(angle) * rest;
            // Compared by where they send a point rather than by
            // `angle_between`, which is `acos` of a dot product very close to
            // one and so reports a few ten-thousandths of noise for two
            // rotations that are bit-for-bit the same.
            for v in [Vec3::X, Vec3::Y, Vec3::Z] {
                assert!(
                    (composed * v).distance(wanted * v) < 1e-4,
                    "a turn about the stage's Z, not the bone's: rest {rest:?}"
                );
            }
        }
    }

    /// And it is still a turn of the size that was asked for — an axis
    /// conversion that quietly changed the angle would be worse than one that
    /// changed the plane.
    #[test]
    fn the_turn_keeps_its_size() {
        let rest = Quat::from_euler(EulerRot::XYZ, 0.3, -1.2, 2.0);
        let turn = stage_turn(rest, 0.6);
        assert!((turn.to_axis_angle().1 - 0.6).abs() < 1e-4);
    }

    /// Half a degree: below this the limb has stopped as far as anyone
    /// watching is concerned. [`SWING_ASLEEP`] is much smaller because it
    /// decides when the bookkeeping entry goes, which is a different question
    /// and costs nothing to answer late.
    const VISIBLE: f32 = 0.0087;

    /// Integrate the swing the way the system does, and report where it got
    /// to and when it stopped being visible.
    fn swing_out(peak: f32) -> (f32, f32) {
        let mut s = Swing {
            angle: 0.0,
            vel: peak / SWING_REACH,
        };
        let dt = 1.0 / 120.0;
        let (mut furthest, mut t, mut settled) = (0.0f32, 0.0f32, 0.0f32);
        while t < 5.0 {
            s.vel += (-SWING_STIFFNESS * s.angle - SWING_DAMPING * s.vel) * dt;
            s.angle += s.vel * dt;
            furthest = furthest.max(s.angle.abs());
            t += dt;
            if s.angle.abs() > VISIBLE {
                settled = t;
            }
        }
        (furthest, settled)
    }

    /// **A hit asks for a gesture and gets one of that size.** `swing_to` is
    /// only useful if the number it takes is the number that arrives; a
    /// conversion that was quietly a tenth of what it claimed is how a
    /// sequencer ends up looking like it does nothing.
    #[test]
    fn a_swing_reaches_about_as_far_as_it_was_asked_to() {
        for asked in [0.1, 0.44, 1.0] {
            let (got, _) = swing_out(asked);
            let error = (got - asked).abs() / asked;
            assert!(error < 0.08, "asked {asked}, reached {got}");
        }
    }

    /// And it comes back. A hit that left a limb somewhere would turn a
    /// pattern into a drift, and by the second bar the puppet would be facing
    /// the wrong way.
    #[test]
    fn a_swing_is_over_within_a_bar() {
        let (_, settled) = swing_out(0.44);
        assert!(
            (0.3..1.6).contains(&settled),
            "a gesture, not a twitch and not a drift: {settled}s"
        );
    }

    /// Underdamped on purpose: a limb that returns to rest without ever
    /// passing it does not read as a limb, it reads as a door closer.
    #[test]
    fn a_swing_passes_rest_on_its_way_back() {
        let mut s = Swing {
            angle: 0.0,
            vel: 0.44 / SWING_REACH,
        };
        let dt = 1.0 / 120.0;
        let mut overshot = false;
        for _ in 0..600 {
            s.vel += (-SWING_STIFFNESS * s.angle - SWING_DAMPING * s.vel) * dt;
            s.angle += s.vel * dt;
            if s.angle < -1e-3 {
                overshot = true;
            }
        }
        assert!(overshot, "it should swing past rest at least once");
    }

    #[test]
    fn a_node_can_be_found_by_its_name() {
        let m = chain();
        assert_eq!(m.node_named("spine").map(|n| n.id), Some(JointId(2)));
        assert!(m.node_named("nothing").is_none());
    }
}
