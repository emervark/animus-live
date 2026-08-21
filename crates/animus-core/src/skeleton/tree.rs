//! Which joints hang off which — the tree a rig only implicitly has.
//!
//! A skeleton is stored as a **graph**: bones connect pairs of joints and
//! nothing names a parent. That is the right storage for a mass-spring
//! solver, where forces run both ways along every bone and no joint is
//! privileged. But forward kinematics needs a direction: turning a shoulder
//! has to carry the elbow, the wrist and the hand, and must leave the hip
//! alone. So the direction is *derived* here rather than stored, and it is
//! derived the same way every time.
//!
//! ## Where the root comes from
//!
//! **A pinned joint is the anchor.** It is already the joint the solver
//! refuses to move, so it is already the thing the rest of the puppet hangs
//! from; making it the tree's root means FK agrees with physics instead of
//! contradicting it. With no pin, the first joint in insertion order takes
//! the job — that is the joint the rigger placed first, which for a
//! character is nearly always the torso.
//!
//! A rig can be in several disconnected pieces, and a piece with no pin in
//! it still has to be rotatable. Each component therefore gets its own root,
//! chosen the same way, so every joint ends up somewhere in some tree.
//!
//! ## Why breadth-first
//!
//! Depth-first would work equally well for correctness and badly for
//! surprise: with a limb that loops back on itself — a hand bound to a hip,
//! say — depth-first can thread the parent chain the long way round the
//! loop, so rotating the shoulder drags the torso. Breadth-first always
//! attaches a joint to its *nearest* ancestor, which is the one the operator
//! would have drawn.

use std::collections::{HashMap, HashSet, VecDeque};

use crate::doc::SkeletonData;
use crate::ids::JointId;

/// A parent-per-joint view of a skeleton, derived from its bones.
#[derive(Debug, Clone, Default)]
pub struct RigTree {
    parent: HashMap<JointId, JointId>,
    children: HashMap<JointId, Vec<JointId>>,
    roots: Vec<JointId>,
}

impl RigTree {
    /// The joint this one hangs from, or `None` at a root.
    pub fn parent(&self, joint: JointId) -> Option<JointId> {
        self.parent.get(&joint).copied()
    }

    /// The joints hanging directly off this one, nearest first.
    pub fn children(&self, joint: JointId) -> &[JointId] {
        self.children.get(&joint).map_or(&[], |v| v.as_slice())
    }

    /// Every root, one per connected piece of the rig.
    pub fn roots(&self) -> &[JointId] {
        &self.roots
    }

    /// Every joint below this one, nearest first. Excludes the joint itself.
    ///
    /// This is what a rotation moves. The order is breadth-first, which is
    /// also the order to *report* them in: "head, shoulder.R, shoulder.L +4
    /// below" reads as the rig looks.
    pub fn descendants(&self, joint: JointId) -> Vec<JointId> {
        let mut out = Vec::new();
        let mut queue: VecDeque<JointId> = self.children(joint).iter().copied().collect();
        // A cycle in the source graph cannot survive tree construction, but
        // guarding costs one set and makes this function safe to call on a
        // hand-built `RigTree`.
        let mut seen: HashSet<JointId> = HashSet::from([joint]);
        while let Some(next) = queue.pop_front() {
            if !seen.insert(next) {
                continue;
            }
            out.push(next);
            queue.extend(self.children(next).iter().copied());
        }
        out
    }
}

/// Derive the tree. See the module docs for the root rule.
pub fn rig_tree(skel: &SkeletonData) -> RigTree {
    // Adjacency in bone insertion order, so the tree is a function of the
    // document and not of hash iteration.
    let mut adjacent: HashMap<JointId, Vec<JointId>> = HashMap::new();
    for bone in skel.bones.values() {
        if bone.a == bone.b {
            // A bone from a joint to itself names no relationship.
            continue;
        }
        adjacent.entry(bone.a).or_default().push(bone.b);
        adjacent.entry(bone.b).or_default().push(bone.a);
    }

    let mut tree = RigTree::default();
    let mut seen: HashSet<JointId> = HashSet::new();

    // Pinned joints first, in insertion order, then the rest: that makes the
    // pin the root of its own component without needing a second pass to
    // find out which component it is in.
    let pinned = skel
        .joints
        .values()
        .filter(|j| j.pinned)
        .map(|j| j.id)
        .collect::<Vec<_>>();
    let everything = skel.joints.values().map(|j| j.id).collect::<Vec<_>>();

    for start in pinned.into_iter().chain(everything) {
        if !seen.insert(start) {
            continue;
        }
        tree.roots.push(start);

        let mut queue = VecDeque::from([start]);
        while let Some(at) = queue.pop_front() {
            let Some(neighbours) = adjacent.get(&at) else {
                continue;
            };
            for &next in neighbours {
                if !seen.insert(next) {
                    continue;
                }
                tree.parent.insert(next, at);
                tree.children.entry(at).or_default().push(next);
                queue.push_back(next);
            }
        }
    }

    tree
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::{Bone, Joint};
    use crate::ids::BoneId;
    use glam::Vec2;
    use indexmap::IndexMap;

    fn joint(id: u64, pinned: bool) -> Joint {
        Joint {
            id: JointId(id),
            name: format!("j{id}"),
            rest: Vec2::new(id as f32, 0.0),
            rest_angle: 0.0,
            inv_mass: 1.0,
            pinned,
        }
    }

    fn bone(id: u64, a: u64, b: u64) -> Bone {
        Bone {
            id: BoneId(id),
            name: format!("b{id}"),
            a: JointId(a),
            b: JointId(b),
            rest_length: None,
            stiffness: 0.8,
            damping: 0.1,
            length_mul: 1.0,
            attach_radius: 40.0,
        }
    }

    /// 1—2—3—4, with a branch 2—5.
    fn skeleton(pinned: Option<u64>) -> SkeletonData {
        let mut joints = IndexMap::new();
        for id in [1, 2, 3, 4, 5] {
            joints.insert(JointId(id), joint(id, pinned == Some(id)));
        }
        let mut bones = IndexMap::new();
        for (i, (a, b)) in [(1, 2), (2, 3), (3, 4), (2, 5)].into_iter().enumerate() {
            bones.insert(BoneId(i as u64), bone(i as u64, a, b));
        }
        SkeletonData { joints, bones }
    }

    #[test]
    fn with_no_pin_the_first_joint_placed_is_the_root() {
        let tree = rig_tree(&skeleton(None));
        assert_eq!(tree.roots(), &[JointId(1)]);
        assert_eq!(tree.parent(JointId(2)), Some(JointId(1)));
    }

    /// **The pin is the anchor.** A pinned joint is the one the solver
    /// refuses to move, so hanging the rig off anything else would make FK
    /// and physics disagree about which end of a limb is fixed.
    #[test]
    fn a_pinned_joint_takes_the_root() {
        let tree = rig_tree(&skeleton(Some(3)));
        assert_eq!(tree.roots(), &[JointId(3)]);
        assert_eq!(tree.parent(JointId(2)), Some(JointId(3)));
        assert_eq!(tree.parent(JointId(1)), Some(JointId(2)));
    }

    /// Turning a shoulder carries the arm and leaves the torso alone.
    #[test]
    fn descendants_are_everything_below_and_nothing_above() {
        let tree = rig_tree(&skeleton(None));
        let below = tree.descendants(JointId(2));
        assert!(below.contains(&JointId(3)));
        assert!(below.contains(&JointId(4)));
        assert!(below.contains(&JointId(5)));
        assert!(
            !below.contains(&JointId(1)),
            "rotating a joint must not move the thing it hangs from"
        );
        assert!(!below.contains(&JointId(2)), "nor itself");
    }

    #[test]
    fn a_leaf_has_nothing_below_it() {
        let tree = rig_tree(&skeleton(None));
        assert!(tree.descendants(JointId(4)).is_empty());
    }

    /// A rig in two pieces is still a rig, and the loose piece still has to
    /// be rotatable — so it gets a root of its own.
    #[test]
    fn every_disconnected_piece_gets_its_own_root() {
        let mut skel = skeleton(None);
        skel.joints.insert(JointId(9), joint(9, false));
        skel.joints.insert(JointId(10), joint(10, false));
        skel.bones.insert(BoneId(90), bone(90, 9, 10));

        let tree = rig_tree(&skel);
        assert_eq!(tree.roots(), &[JointId(1), JointId(9)]);
        assert_eq!(tree.descendants(JointId(9)), vec![JointId(10)]);
    }

    /// **A loop must not make a joint its own ancestor.** Breadth-first
    /// attaches every joint to its nearest ancestor, so closing 1—2—3—4 into
    /// a ring leaves the chain hanging off 1 rather than threading the long
    /// way round.
    #[test]
    fn a_loop_in_the_rig_does_not_reverse_the_chain() {
        let mut skel = skeleton(None);
        skel.bones.insert(BoneId(99), bone(99, 4, 1));

        let tree = rig_tree(&skel);
        assert_eq!(tree.parent(JointId(4)), Some(JointId(1)));
        assert!(
            !tree.descendants(JointId(4)).contains(&JointId(1)),
            "a joint became its own descendant through a loop"
        );
    }
}
