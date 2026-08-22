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
}

/// How far a bound position offset moves a model node, per image pixel.
///
/// Bindings are quoted in image pixels because that is what a 2D puppet
/// speaks, and a model is in metres. A hundred pixels of fader travel
/// reading as a hundred metres of shoulder would be the single most
/// startling thing in the application.
pub const PX_TO_MODEL: f32 = 0.01;

/// Find the scene entities the document's nodes name.
pub fn discover_model_nodes(
    mut commands: Commands,
    doc: Res<DocumentRes>,
    roots: Query<(Entity, &PuppetRoot, Option<&ModelNodes>), With<ModelRoot>>,
    children: Query<&Children>,
    named: Query<(&Name, &Transform)>,
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
        let mut stack = vec![entity];
        while let Some(at) = stack.pop() {
            if let Ok((name, transform)) = named.get(at)
                && let Some(node) = model.node_named(name.as_str())
            {
                by_joint.insert(node.id, at);
                rest.insert(node.id, *transform);
            }
            if let Ok(kids) = children.get(at) {
                stack.extend(kids.iter());
            }
        }

        if by_joint.is_empty() {
            continue;
        }
        let located = by_joint.len();
        commands
            .entity(entity)
            .insert(ModelNodes { by_joint, rest });
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

            let angle = rotations.get(root.0, node.id);
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
            // About Z, because Z is the axis facing the audience: turning a
            // limb on a stage means turning it in the plane the audience
            // sees, whatever the model's own idea of up happens to be.
            transform.rotation = rest.rotation * Quat::from_rotation_z(angle);
            transform.translation = rest.translation + Vec3::new(offset.x, offset.y, 0.0);
        }
    }
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

    #[test]
    fn a_node_can_be_found_by_its_name() {
        let m = chain();
        assert_eq!(m.node_named("spine").map(|n| n.id), Some(JointId(2)));
        assert!(m.node_named("nothing").is_none());
    }
}
