//! The only path that mutates a [`Project`].
//!
//! Every edit in the application is a [`DocCommand`]: it knows how to apply
//! itself, how to take itself back, and what it changed. Systems read
//! `Project`; exactly one place writes it, through [`apply`]. Spec §8.6,
//! §10.5.
//!
//! ## Two kinds of command, for one reason
//!
//! **Inverse-pair** commands store the old and new value of something small
//! ([`MoveJointRest`], [`SetLayerOpacity`]). They are cheap, and they
//! [`merge`](DocCommand::merge), so a slider drag that emits 200 events
//! collapses into one undo step.
//!
//! **Snapshot** commands ([`ReplacePuppet`], [`AddPuppet`], [`RemovePuppet`])
//! keep whole values. Writing a correct inverse for "retriangulate this
//! puppet" is a bug factory; keeping the old puppet is provably right, and
//! these operations happen a handful of times per session rather than a
//! hundred times per second.
//!
//! ## Merging is bounded by the caller, not by a clock
//!
//! `animus-core` has no clock, and a time window would be the wrong tool
//! anyway: a drag is bounded by the mouse coming up, not by 500 ms passing.
//! The editor calls [`UndoStack::break_merge`](super::undo::UndoStack::break_merge)
//! when a gesture ends, and commands merge freely until it does.

use std::any::Any;

use glam::Vec2;
use thiserror::Error;

use super::{AssetRef, AttachmentTable, Layer, Puppet, PuppetKind, SkeletonData, Transform2Or3};
use crate::ids::{BoneId, JointId, LayerId, PuppetId};

/// What a command changed, at the finest granularity it knows.
///
/// Granularity is load-bearing: the runtime rebuilds a `Mesh` asset when it
/// sees [`MeshRebuilt`](DocChange::MeshRebuilt) and merely moves a transform
/// when it sees [`JointMoved`](DocChange::JointMoved). A command that reports
/// more than it did makes dragging a joint as expensive as retriangulating.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocChange {
    LayerAdded(LayerId),
    LayerRemoved(LayerId),
    LayerOrderChanged,
    LayerPropsChanged(LayerId),
    PuppetAdded(PuppetId),
    PuppetRemoved(PuppetId),
    /// A joint's rest position moved. Does **not** imply a mesh rebuild.
    JointMoved(PuppetId, JointId),
    /// A joint or bone was added, removed, or had a solver parameter
    /// changed: the `CompiledRig` must be rebuilt, the mesh need not be.
    SkeletonChanged(PuppetId),
    /// Mesh topology changed: the GPU mesh must be rebuilt.
    MeshRebuilt(PuppetId),
    MaterialChanged(PuppetId),
    SolverConfigChanged,
}

/// The changes produced by one command, drained by the sync system.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PendingChanges(pub Vec<DocChange>);

impl PendingChanges {
    pub fn one(c: DocChange) -> Self {
        Self(vec![c])
    }

    pub fn none() -> Self {
        Self(Vec::new())
    }

    pub fn extend(&mut self, other: PendingChanges) {
        self.0.extend(other.0);
    }

    pub fn contains(&self, c: DocChange) -> bool {
        self.0.contains(&c)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CommandError {
    #[error("no layer with id {0:?}")]
    NoSuchLayer(LayerId),
    #[error("no puppet with id {0:?}")]
    NoSuchPuppet(PuppetId),
    #[error("puppet {0:?} is not a mesh puppet")]
    NotAMeshPuppet(PuppetId),
    #[error("no joint with id {0:?} in puppet {1:?}")]
    NoSuchJoint(JointId, PuppetId),
    #[error("no bone with id {0:?} in puppet {1:?}")]
    NoSuchBone(BoneId, PuppetId),
    #[error("puppet {0:?} already exists")]
    PuppetExists(PuppetId),
}

/// A reversible edit to the document.
pub trait DocCommand: Send + Sync + 'static {
    /// Shown in the undo history. Human-written, so Inter, not mono.
    fn label(&self) -> &str;

    fn apply(&mut self, p: &mut super::Project) -> Result<PendingChanges, CommandError>;

    fn revert(&mut self, p: &mut super::Project) -> Result<PendingChanges, CommandError>;

    /// Fold `next` into `self` if they are the same continuing gesture.
    /// Returning `true` means `next` must be dropped: `self` now represents
    /// both. The default is never to merge, which is always safe.
    fn merge(&mut self, _next: &dyn DocCommand) -> bool {
        false
    }

    /// Roughly what this command is holding, for the undo stack's memory
    /// cap. Snapshot commands must override this or the cap is a lie.
    fn memory_bytes(&self) -> usize {
        std::mem::size_of_val(self)
    }

    fn as_any(&self) -> &dyn Any;
}

/// Apply `cmd` to `p`. The single mutation path.
///
/// Takes the command boxed because the caller hands ownership to the undo
/// stack immediately afterwards; see [`UndoStack::push_applied`](super::undo::UndoStack::push_applied).
pub fn apply(
    p: &mut super::Project,
    cmd: &mut dyn DocCommand,
) -> Result<PendingChanges, CommandError> {
    cmd.apply(p)
}

// ── inverse-pair commands ──────────────────────────────────────────────

/// Move a joint's rest position. The high-frequency editing command.
///
/// `PartialEq` so tool layers can treat proposed commands as values and
/// assert on them.
#[derive(Debug, Clone, PartialEq)]
pub struct MoveJointRest {
    pub puppet: PuppetId,
    pub joint: JointId,
    pub from: Vec2,
    pub to: Vec2,
}

fn joint_mut(
    p: &mut super::Project,
    puppet: PuppetId,
    joint: JointId,
) -> Result<&mut super::Joint, CommandError> {
    let pup = p
        .puppets
        .get_mut(&puppet)
        .ok_or(CommandError::NoSuchPuppet(puppet))?;
    let mp = match &mut pup.kind {
        PuppetKind::Mesh(m) => m,
        _ => return Err(CommandError::NotAMeshPuppet(puppet)),
    };
    mp.skeleton
        .joints
        .get_mut(&joint)
        .ok_or(CommandError::NoSuchJoint(joint, puppet))
}

fn bone_mut(
    p: &mut super::Project,
    puppet: PuppetId,
    bone: BoneId,
) -> Result<&mut super::Bone, CommandError> {
    let pup = p
        .puppets
        .get_mut(&puppet)
        .ok_or(CommandError::NoSuchPuppet(puppet))?;
    let mp = match &mut pup.kind {
        PuppetKind::Mesh(m) => m,
        _ => return Err(CommandError::NotAMeshPuppet(puppet)),
    };
    mp.skeleton
        .bones
        .get_mut(&bone)
        .ok_or(CommandError::NoSuchBone(bone, puppet))
}

impl DocCommand for MoveJointRest {
    fn label(&self) -> &str {
        "Move joint"
    }

    fn apply(&mut self, p: &mut super::Project) -> Result<PendingChanges, CommandError> {
        joint_mut(p, self.puppet, self.joint)?.rest = self.to;
        Ok(PendingChanges::one(DocChange::JointMoved(
            self.puppet,
            self.joint,
        )))
    }

    fn revert(&mut self, p: &mut super::Project) -> Result<PendingChanges, CommandError> {
        joint_mut(p, self.puppet, self.joint)?.rest = self.from;
        Ok(PendingChanges::one(DocChange::JointMoved(
            self.puppet,
            self.joint,
        )))
    }

    fn merge(&mut self, next: &dyn DocCommand) -> bool {
        match next.as_any().downcast_ref::<MoveJointRest>() {
            Some(n) if n.puppet == self.puppet && n.joint == self.joint => {
                // keep our `from`, take their `to`: the pair still inverts
                self.to = n.to;
                true
            }
            _ => false,
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Turn a joint, and everything hanging off it, about that joint.
///
/// Forward kinematics on a rig that has no stored hierarchy: the parent
/// relation is derived from the bone graph by
/// [`rig_tree`](crate::skeleton::rig_tree), so turning a shoulder carries the
/// elbow, the wrist and the hand and leaves the torso where it was.
///
/// **The angle is stored, the positions are what move.** Nothing downstream
/// reads [`Joint::rest_angle`](super::Joint::rest_angle) — the solver works
/// on positions alone — so the angle exists to be shown in the inspector and
/// to give the next rotation something to be relative to. Recording only the
/// angle and rotating at compile time would be the other design; it would
/// also mean a rig whose joint positions on disk are not where the puppet
/// actually is, which is the kind of gap that costs a day.
///
/// Angles are radians in the image's own space, where Y runs **down**, so a
/// positive angle turns clockwise on screen. That matches what the operator
/// sees when they drag the dial to the right.
#[derive(Debug, Clone, PartialEq)]
pub struct RotateJoint {
    pub puppet: PuppetId,
    pub joint: JointId,
    pub from: f32,
    pub to: f32,
}

impl RotateJoint {
    fn turn(&self, p: &mut super::Project, by: f32) -> Result<PendingChanges, CommandError> {
        let pup = p
            .puppets
            .get_mut(&self.puppet)
            .ok_or(CommandError::NoSuchPuppet(self.puppet))?;
        let mp = match &mut pup.kind {
            PuppetKind::Mesh(m) => m,
            _ => return Err(CommandError::NotAMeshPuppet(self.puppet)),
        };
        let pivot = mp
            .skeleton
            .joints
            .get(&self.joint)
            .ok_or(CommandError::NoSuchJoint(self.joint, self.puppet))?
            .rest;

        let below = crate::skeleton::rig_tree(&mp.skeleton).descendants(self.joint);
        let (sin, cos) = by.sin_cos();
        let mut changes = PendingChanges::none();
        for id in below {
            let Some(j) = mp.skeleton.joints.get_mut(&id) else {
                continue;
            };
            let d = j.rest - pivot;
            j.rest = pivot + Vec2::new(d.x * cos - d.y * sin, d.x * sin + d.y * cos);
            // Each joint's own stored angle turns with it, so a limb rotated
            // as part of a shoulder still reports its own orientation.
            j.rest_angle += by;
            changes.extend(PendingChanges::one(DocChange::JointMoved(self.puppet, id)));
        }
        Ok(changes)
    }
}

impl DocCommand for RotateJoint {
    fn label(&self) -> &str {
        "Rotate joint"
    }

    fn apply(&mut self, p: &mut super::Project) -> Result<PendingChanges, CommandError> {
        let changes = self.turn(p, self.to - self.from)?;
        joint_mut(p, self.puppet, self.joint)?.rest_angle = self.to;
        Ok(changes)
    }

    fn revert(&mut self, p: &mut super::Project) -> Result<PendingChanges, CommandError> {
        let changes = self.turn(p, self.from - self.to)?;
        joint_mut(p, self.puppet, self.joint)?.rest_angle = self.from;
        Ok(changes)
    }

    fn merge(&mut self, next: &dyn DocCommand) -> bool {
        match next.as_any().downcast_ref::<RotateJoint>() {
            Some(n) if n.puppet == self.puppet && n.joint == self.joint => {
                // Each command applied its own delta as it arrived, so the
                // document is already correct; widening `to` is what keeps
                // the single merged entry invertible across the whole drag.
                self.to = n.to;
                true
            }
            _ => false,
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Set a joint's inverse mass — how readily it is thrown about.
///
/// Stored as the inverse because that is what the solver integrates with;
/// the inspector shows the mass itself, since "2 kg" is a thing an operator
/// can picture and "0.5" is not. Pinning is a separate flag and does not go
/// through here: a pinned joint is immovable regardless of what it weighs,
/// so unpinning restores the weight the operator chose rather than leaving
/// them to remember it.
#[derive(Debug, Clone, PartialEq)]
pub struct SetJointMass {
    pub puppet: PuppetId,
    pub joint: JointId,
    /// Inverse mass, not mass.
    pub from: f32,
    pub to: f32,
}

impl DocCommand for SetJointMass {
    fn label(&self) -> &str {
        "Set joint mass"
    }

    fn apply(&mut self, p: &mut super::Project) -> Result<PendingChanges, CommandError> {
        joint_mut(p, self.puppet, self.joint)?.inv_mass = self.to;
        Ok(PendingChanges::one(DocChange::SkeletonChanged(self.puppet)))
    }

    fn revert(&mut self, p: &mut super::Project) -> Result<PendingChanges, CommandError> {
        joint_mut(p, self.puppet, self.joint)?.inv_mass = self.from;
        Ok(PendingChanges::one(DocChange::SkeletonChanged(self.puppet)))
    }

    fn merge(&mut self, next: &dyn DocCommand) -> bool {
        match next.as_any().downcast_ref::<SetJointMass>() {
            Some(n) if n.puppet == self.puppet && n.joint == self.joint => {
                self.to = n.to;
                true
            }
            _ => false,
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Which scalar on a bone a [`SetBoneParam`] is driving.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoneParam {
    Stiffness,
    Damping,
    LengthMul,
    AttachRadius,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SetBoneParam {
    pub puppet: PuppetId,
    pub bone: BoneId,
    pub param: BoneParam,
    pub from: f32,
    pub to: f32,
}

impl SetBoneParam {
    fn write(&self, p: &mut super::Project, v: f32) -> Result<PendingChanges, CommandError> {
        let b = bone_mut(p, self.puppet, self.bone)?;
        match self.param {
            BoneParam::Stiffness => b.stiffness = v,
            BoneParam::Damping => b.damping = v,
            BoneParam::LengthMul => b.length_mul = v,
            BoneParam::AttachRadius => b.attach_radius = v,
        }
        Ok(PendingChanges::one(DocChange::SkeletonChanged(self.puppet)))
    }
}

impl DocCommand for SetBoneParam {
    fn label(&self) -> &str {
        "Set bone parameter"
    }

    fn apply(&mut self, p: &mut super::Project) -> Result<PendingChanges, CommandError> {
        self.write(p, self.to)
    }

    fn revert(&mut self, p: &mut super::Project) -> Result<PendingChanges, CommandError> {
        self.write(p, self.from)
    }

    fn merge(&mut self, next: &dyn DocCommand) -> bool {
        match next.as_any().downcast_ref::<SetBoneParam>() {
            Some(n) if n.puppet == self.puppet && n.bone == self.bone && n.param == self.param => {
                self.to = n.to;
                true
            }
            _ => false,
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SetJointPinned {
    pub puppet: PuppetId,
    pub joint: JointId,
    pub from: bool,
    pub to: bool,
}

impl DocCommand for SetJointPinned {
    fn label(&self) -> &str {
        "Pin joint"
    }

    fn apply(&mut self, p: &mut super::Project) -> Result<PendingChanges, CommandError> {
        let j = joint_mut(p, self.puppet, self.joint)?;
        j.pinned = self.to;
        j.inv_mass = if self.to { 0.0 } else { 1.0 };
        Ok(PendingChanges::one(DocChange::SkeletonChanged(self.puppet)))
    }

    fn revert(&mut self, p: &mut super::Project) -> Result<PendingChanges, CommandError> {
        let j = joint_mut(p, self.puppet, self.joint)?;
        j.pinned = self.from;
        j.inv_mass = if self.from { 0.0 } else { 1.0 };
        Ok(PendingChanges::one(DocChange::SkeletonChanged(self.puppet)))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Which solver setting a [`SetSolverParam`] is driving.
///
/// One command for all of them rather than one per setting: they share an
/// inverse, they share a change, and every one of them ends in the same rig
/// recompile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SolverParam {
    GravityX,
    GravityY,
    Damping,
    Iterations,
    /// How hard a released joint is pulled home. See
    /// [`SolverConfig::rest_pull`](super::SolverConfig).
    RestPull,
}

/// Change one solver setting for the whole show.
///
/// Solver settings are baked into `CompiledRig` when a rig is compiled, so
/// this emits [`DocChange::SolverConfigChanged`] and the runtime recompiles
/// every rig from it. Without that recompile the document would say one
/// thing and the springs would keep doing another — which is what gravity
/// did before this command existed: it was in the file, shown in the panel,
/// and read by nothing after startup.
#[derive(Debug, Clone, PartialEq)]
pub struct SetSolverParam {
    pub param: SolverParam,
    pub from: f32,
    pub to: f32,
}

impl SetSolverParam {
    fn write(&self, p: &mut super::Project, value: f32) {
        let cfg = &mut p.solver;
        match self.param {
            SolverParam::GravityX => cfg.gravity.x = value,
            SolverParam::GravityY => cfg.gravity.y = value,
            // A damping of zero is a puppet that never settles and one above
            // 1.0 is a puppet that gains energy every tick, so the range is
            // enforced here rather than trusted from the caller.
            SolverParam::Damping => cfg.global_damping = value.clamp(0.0, 1.0),
            SolverParam::Iterations => cfg.iterations = (value.round() as u32).clamp(1, 32),
            SolverParam::RestPull => cfg.rest_pull = value.clamp(0.0, 1.0),
        }
    }
}

impl DocCommand for SetSolverParam {
    fn label(&self) -> &str {
        "Solver setting"
    }

    fn apply(&mut self, p: &mut super::Project) -> Result<PendingChanges, CommandError> {
        self.write(p, self.to);
        Ok(PendingChanges::one(DocChange::SolverConfigChanged))
    }

    fn revert(&mut self, p: &mut super::Project) -> Result<PendingChanges, CommandError> {
        self.write(p, self.from);
        Ok(PendingChanges::one(DocChange::SolverConfigChanged))
    }

    /// Dragging a slider is one undo entry, like every other slider here.
    fn merge(&mut self, next: &dyn DocCommand) -> bool {
        let Some(other) = next.as_any().downcast_ref::<Self>() else {
            return false;
        };
        if other.param != self.param {
            return false;
        }
        self.to = other.to;
        true
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Which scalar on a layer a [`SetLayerScalar`] is driving.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayerScalar {
    Opacity,
    Depth,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SetLayerScalar {
    pub layer: LayerId,
    pub which: LayerScalar,
    pub from: f32,
    pub to: f32,
}

impl SetLayerScalar {
    fn write(&self, p: &mut super::Project, v: f32) -> Result<PendingChanges, CommandError> {
        let l = p
            .layer_data
            .get_mut(&self.layer)
            .ok_or(CommandError::NoSuchLayer(self.layer))?;
        match self.which {
            LayerScalar::Opacity => l.opacity = v,
            LayerScalar::Depth => l.depth = v,
        }
        Ok(PendingChanges::one(DocChange::LayerPropsChanged(
            self.layer,
        )))
    }
}

impl DocCommand for SetLayerScalar {
    fn label(&self) -> &str {
        "Set layer property"
    }

    fn apply(&mut self, p: &mut super::Project) -> Result<PendingChanges, CommandError> {
        self.write(p, self.to)
    }

    fn revert(&mut self, p: &mut super::Project) -> Result<PendingChanges, CommandError> {
        self.write(p, self.from)
    }

    fn merge(&mut self, next: &dyn DocCommand) -> bool {
        match next.as_any().downcast_ref::<SetLayerScalar>() {
            Some(n) if n.layer == self.layer && n.which == self.which => {
                self.to = n.to;
                true
            }
            _ => false,
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RenameLayer {
    pub layer: LayerId,
    pub from: String,
    pub to: String,
}

impl DocCommand for RenameLayer {
    fn label(&self) -> &str {
        "Rename layer"
    }

    fn apply(&mut self, p: &mut super::Project) -> Result<PendingChanges, CommandError> {
        p.layer_data
            .get_mut(&self.layer)
            .ok_or(CommandError::NoSuchLayer(self.layer))?
            .name = self.to.clone();
        Ok(PendingChanges::one(DocChange::LayerPropsChanged(
            self.layer,
        )))
    }

    fn revert(&mut self, p: &mut super::Project) -> Result<PendingChanges, CommandError> {
        p.layer_data
            .get_mut(&self.layer)
            .ok_or(CommandError::NoSuchLayer(self.layer))?
            .name = self.from.clone();
        Ok(PendingChanges::one(DocChange::LayerPropsChanged(
            self.layer,
        )))
    }

    fn merge(&mut self, next: &dyn DocCommand) -> bool {
        match next.as_any().downcast_ref::<RenameLayer>() {
            Some(n) if n.layer == self.layer => {
                self.to = n.to.clone();
                true
            }
            _ => false,
        }
    }

    fn memory_bytes(&self) -> usize {
        std::mem::size_of_val(self) + self.from.len() + self.to.len()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Reorder the paint list. Snapshot of the whole order: it is a handful of
/// `LayerId`s, and an index-swap inverse is easy to get subtly wrong.
#[derive(Debug, Clone)]
pub struct ReorderLayers {
    pub from: Vec<LayerId>,
    pub to: Vec<LayerId>,
}

impl DocCommand for ReorderLayers {
    fn label(&self) -> &str {
        "Reorder layers"
    }

    fn apply(&mut self, p: &mut super::Project) -> Result<PendingChanges, CommandError> {
        p.layers = self.to.clone();
        Ok(PendingChanges::one(DocChange::LayerOrderChanged))
    }

    fn revert(&mut self, p: &mut super::Project) -> Result<PendingChanges, CommandError> {
        p.layers = self.from.clone();
        Ok(PendingChanges::one(DocChange::LayerOrderChanged))
    }

    fn memory_bytes(&self) -> usize {
        std::mem::size_of_val(self)
            + (self.from.len() + self.to.len()) * std::mem::size_of::<LayerId>()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

// ── snapshot commands ──────────────────────────────────────────────────

fn puppet_bytes(p: &Puppet) -> usize {
    let base = std::mem::size_of::<Puppet>() + p.name.len();
    match &p.kind {
        PuppetKind::Mesh(m) => {
            base + m.mesh.positions.len() * 8
                + m.mesh.uvs.len() * 8
                + m.mesh.triangles.len() * 4
                + m.skeleton.joints.len() * 64
                + m.skeleton.bones.len() * 64
                + m.attachments.entries.len() * 32
        }
        PuppetKind::Model(_) => base,
    }
}

/// Replace a puppet wholesale: retriangulate, auto-rig, delete vertices.
///
/// The caller computes the new value; this command owns the swap and the
/// way back. `change` says what the runtime must rebuild — pass
/// [`DocChange::MeshRebuilt`] when topology moved, [`DocChange::SkeletonChanged`]
/// when only the rig did.
#[derive(Debug, Clone)]
pub struct ReplacePuppet {
    pub id: PuppetId,
    pub label: String,
    pub change: DocChange,
    pub to: Puppet,
    /// Captured by `apply`, so a command can be built before it is run.
    pub from: Option<Puppet>,
}

impl ReplacePuppet {
    pub fn new(id: PuppetId, label: impl Into<String>, change: DocChange, to: Puppet) -> Self {
        Self {
            id,
            label: label.into(),
            change,
            to,
            from: None,
        }
    }
}

impl DocCommand for ReplacePuppet {
    fn label(&self) -> &str {
        &self.label
    }

    fn apply(&mut self, p: &mut super::Project) -> Result<PendingChanges, CommandError> {
        let slot = p
            .puppets
            .get_mut(&self.id)
            .ok_or(CommandError::NoSuchPuppet(self.id))?;
        if self.from.is_none() {
            self.from = Some(slot.clone());
        }
        *slot = self.to.clone();
        Ok(PendingChanges::one(self.change))
    }

    fn revert(&mut self, p: &mut super::Project) -> Result<PendingChanges, CommandError> {
        let old = self
            .from
            .clone()
            .expect("revert before apply: the undo stack only reverts applied commands");
        let slot = p
            .puppets
            .get_mut(&self.id)
            .ok_or(CommandError::NoSuchPuppet(self.id))?;
        *slot = old;
        Ok(PendingChanges::one(self.change))
    }

    fn memory_bytes(&self) -> usize {
        std::mem::size_of_val(self)
            + puppet_bytes(&self.to)
            + self.from.as_ref().map(puppet_bytes).unwrap_or(0)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Add a puppet and put it in a layer.
#[derive(Debug, Clone)]
pub struct AddPuppet {
    pub puppet: Puppet,
    pub layer: LayerId,
}

impl DocCommand for AddPuppet {
    fn label(&self) -> &str {
        "Add puppet"
    }

    fn apply(&mut self, p: &mut super::Project) -> Result<PendingChanges, CommandError> {
        let id = self.puppet.id;
        if p.puppets.contains_key(&id) {
            return Err(CommandError::PuppetExists(id));
        }
        let layer = p
            .layer_data
            .get_mut(&self.layer)
            .ok_or(CommandError::NoSuchLayer(self.layer))?;
        layer.contents.push(id);
        p.puppets.insert(id, self.puppet.clone());
        Ok(PendingChanges::one(DocChange::PuppetAdded(id)))
    }

    fn revert(&mut self, p: &mut super::Project) -> Result<PendingChanges, CommandError> {
        let id = self.puppet.id;
        p.puppets.shift_remove(&id);
        if let Some(layer) = p.layer_data.get_mut(&self.layer) {
            layer.contents.retain(|c| *c != id);
        }
        Ok(PendingChanges::one(DocChange::PuppetRemoved(id)))
    }

    fn memory_bytes(&self) -> usize {
        std::mem::size_of_val(self) + puppet_bytes(&self.puppet)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Remove a puppet, remembering where in its layer it sat so undo puts it
/// back in the same paint position rather than on top.
#[derive(Debug, Clone)]
pub struct RemovePuppet {
    pub id: PuppetId,
    removed: Option<(Puppet, LayerId, usize, usize)>,
}

impl RemovePuppet {
    pub fn new(id: PuppetId) -> Self {
        Self { id, removed: None }
    }
}

impl DocCommand for RemovePuppet {
    fn label(&self) -> &str {
        "Delete puppet"
    }

    fn apply(&mut self, p: &mut super::Project) -> Result<PendingChanges, CommandError> {
        let index = p
            .puppets
            .get_index_of(&self.id)
            .ok_or(CommandError::NoSuchPuppet(self.id))?;
        let puppet = p.puppets.shift_remove(&self.id).unwrap();

        let mut where_ = None;
        for (lid, layer) in p.layer_data.iter_mut() {
            if let Some(pos) = layer.contents.iter().position(|c| *c == self.id) {
                layer.contents.remove(pos);
                where_ = Some((*lid, pos));
                break;
            }
        }
        let (layer_id, slot) = where_.unwrap_or((LayerId(0), 0));
        self.removed = Some((puppet, layer_id, slot, index));
        Ok(PendingChanges::one(DocChange::PuppetRemoved(self.id)))
    }

    fn revert(&mut self, p: &mut super::Project) -> Result<PendingChanges, CommandError> {
        let (puppet, layer_id, slot, index) = self
            .removed
            .clone()
            .expect("revert before apply: the undo stack only reverts applied commands");
        p.puppets.insert(puppet.id, puppet);
        let last = p.puppets.len() - 1;
        p.puppets.move_index(last, index.min(last));
        if let Some(layer) = p.layer_data.get_mut(&layer_id) {
            let at = slot.min(layer.contents.len());
            layer.contents.insert(at, self.id);
        }
        Ok(PendingChanges::one(DocChange::PuppetAdded(self.id)))
    }

    fn memory_bytes(&self) -> usize {
        std::mem::size_of_val(self)
            + self
                .removed
                .as_ref()
                .map(|(p, ..)| puppet_bytes(p))
                .unwrap_or(0)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Replace a puppet's skeleton and attachments together.
///
/// **One command for every rig edit, and a snapshot rather than an inverse
/// pair.** Adding a joint is small, but deleting one cascades: every bone
/// that names it goes too, and every attachment those bones held has to be
/// recomputed. Writing an inverse for that is four rules that must agree
/// with each other forever; a skeleton is tens of joints, so keeping both
/// copies is provably correct and costs nothing measurable.
///
/// The mesh is deliberately *not* part of this. Rig edits do not touch
/// vertices, so the expensive half of a puppet is never snapshotted and the
/// runtime is told `SkeletonChanged`, which does not rebuild the GPU mesh.
#[derive(Debug, Clone)]
pub struct SetSkeleton {
    pub puppet: PuppetId,
    pub label: String,
    pub after: (SkeletonData, AttachmentTable),
    /// Captured by `apply`, so the caller can build the command before
    /// deciding to run it.
    pub before: Option<(SkeletonData, AttachmentTable)>,
}

impl SetSkeleton {
    pub fn new(
        puppet: PuppetId,
        label: impl Into<String>,
        skeleton: SkeletonData,
        attachments: AttachmentTable,
    ) -> Self {
        Self {
            puppet,
            label: label.into(),
            after: (skeleton, attachments),
            before: None,
        }
    }
}

fn skeleton_slot(
    p: &mut super::Project,
    id: PuppetId,
) -> Result<&mut super::MeshPuppet, CommandError> {
    let puppet = p
        .puppets
        .get_mut(&id)
        .ok_or(CommandError::NoSuchPuppet(id))?;
    match &mut puppet.kind {
        PuppetKind::Mesh(m) => Ok(m),
        _ => Err(CommandError::NotAMeshPuppet(id)),
    }
}

impl DocCommand for SetSkeleton {
    fn label(&self) -> &str {
        &self.label
    }

    fn apply(&mut self, p: &mut super::Project) -> Result<PendingChanges, CommandError> {
        let mp = skeleton_slot(p, self.puppet)?;
        if self.before.is_none() {
            self.before = Some((mp.skeleton.clone(), mp.attachments.clone()));
        }
        mp.skeleton = self.after.0.clone();
        mp.attachments = self.after.1.clone();
        Ok(PendingChanges::one(DocChange::SkeletonChanged(self.puppet)))
    }

    fn revert(&mut self, p: &mut super::Project) -> Result<PendingChanges, CommandError> {
        let (skeleton, attachments) = self
            .before
            .clone()
            .expect("revert before apply: the undo stack only reverts applied commands");
        let mp = skeleton_slot(p, self.puppet)?;
        mp.skeleton = skeleton;
        mp.attachments = attachments;
        Ok(PendingChanges::one(DocChange::SkeletonChanged(self.puppet)))
    }

    fn memory_bytes(&self) -> usize {
        let one = |s: &(SkeletonData, AttachmentTable)| {
            s.0.joints.len() * 64 + s.0.bones.len() * 64 + s.1.entries.len() * 32
        };
        std::mem::size_of_val(self) + one(&self.after) + self.before.as_ref().map(one).unwrap_or(0)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Where an imported puppet should land.
#[derive(Debug, Clone)]
pub enum ImportTarget {
    /// Into a layer that already exists.
    Existing(LayerId),
    /// Into a new layer, created by this same command.
    NewLayer(Layer),
}

/// One import: the asset, the puppet, and the layer it lands in.
///
/// **Deliberately one command rather than three.** An import that produced a
/// bad silhouette is undone by one Ctrl+Z, not by three — and three separate
/// commands could be interleaved with something else and then partially
/// undone, leaving a puppet whose texture no longer exists.
#[derive(Debug, Clone)]
pub struct ImportImage {
    pub asset: AssetRef,
    pub puppet: Puppet,
    pub target: ImportTarget,
}

impl DocCommand for ImportImage {
    fn label(&self) -> &str {
        "Import image"
    }

    fn apply(&mut self, p: &mut super::Project) -> Result<PendingChanges, CommandError> {
        let puppet_id = self.puppet.id;
        if p.puppets.contains_key(&puppet_id) {
            return Err(CommandError::PuppetExists(puppet_id));
        }
        p.assets.insert(self.asset.id, self.asset.clone());

        let mut changes = PendingChanges::none();
        let layer_id = match &self.target {
            ImportTarget::Existing(id) => *id,
            ImportTarget::NewLayer(layer) => {
                p.layer_data.insert(layer.id, layer.clone());
                p.layers.push(layer.id);
                changes.extend(PendingChanges::one(DocChange::LayerAdded(layer.id)));
                layer.id
            }
        };

        p.layer_data
            .get_mut(&layer_id)
            .ok_or(CommandError::NoSuchLayer(layer_id))?
            .contents
            .push(puppet_id);
        p.puppets.insert(puppet_id, self.puppet.clone());

        changes.extend(PendingChanges::one(DocChange::PuppetAdded(puppet_id)));
        Ok(changes)
    }

    fn revert(&mut self, p: &mut super::Project) -> Result<PendingChanges, CommandError> {
        let puppet_id = self.puppet.id;
        p.puppets.shift_remove(&puppet_id);
        p.assets.shift_remove(&self.asset.id);

        let mut changes = PendingChanges::one(DocChange::PuppetRemoved(puppet_id));
        match &self.target {
            ImportTarget::Existing(id) => {
                if let Some(layer) = p.layer_data.get_mut(id) {
                    layer.contents.retain(|c| *c != puppet_id);
                }
            }
            ImportTarget::NewLayer(layer) => {
                p.layer_data.shift_remove(&layer.id);
                p.layers.retain(|l| *l != layer.id);
                changes.extend(PendingChanges::one(DocChange::LayerRemoved(layer.id)));
            }
        }
        Ok(changes)
    }

    fn memory_bytes(&self) -> usize {
        std::mem::size_of_val(self) + puppet_bytes(&self.puppet) + self.asset.original_name.len()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// A layer's flat placement on the stage: where it is and how big.
///
/// Translation and scale travel together because they are not independent in
/// the gesture that changes them. Dragging a corner handle keeps the opposite
/// corner still, which moves the origin as well as resizing — writing them as
/// two commands would let undo land halfway between, with the puppet the new
/// size at the old position.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LayerPlacement {
    pub translation: Vec2,
    pub scale: Vec2,
}

/// The smallest a layer may be scaled to.
///
/// Not zero: a zero scale is a puppet that vanishes and cannot be grabbed
/// again, because its handles collapse onto each other.
pub const MIN_LAYER_SCALE: f32 = 0.01;

/// Move or resize a layer on the stage.
///
/// Translation is in **world units** — the same space the stage frame and the
/// output camera are in, so dragging a puppet to the edge of the frame means
/// what it looks like it means.
///
/// Nothing clamps it to the stage. A puppet half off the frame is a shot, not
/// a mistake: an operator placing a character so the audience sees one
/// shoulder is doing it on purpose.
#[derive(Debug, Clone, PartialEq)]
pub struct TransformLayer {
    pub layer: LayerId,
    pub from: LayerPlacement,
    pub to: LayerPlacement,
}

impl TransformLayer {
    /// A pure move, leaving the size alone.
    pub fn moved(layer: LayerId, from: Vec2, to: Vec2, scale: Vec2) -> Self {
        Self {
            layer,
            from: LayerPlacement {
                translation: from,
                scale,
            },
            to: LayerPlacement {
                translation: to,
                scale,
            },
        }
    }

    fn write(
        &self,
        p: &mut super::Project,
        v: LayerPlacement,
    ) -> Result<PendingChanges, CommandError> {
        let l = p
            .layer_data
            .get_mut(&self.layer)
            .ok_or(CommandError::NoSuchLayer(self.layer))?;
        let s = Vec2::new(
            v.scale.x.abs().max(MIN_LAYER_SCALE) * v.scale.x.signum(),
            v.scale.y.abs().max(MIN_LAYER_SCALE) * v.scale.y.signum(),
        );
        match &mut l.transform {
            Transform2Or3::Flat {
                translation, scale, ..
            } => {
                *translation = v.translation;
                *scale = s;
            }
            Transform2Or3::Spatial {
                translation, scale, ..
            } => {
                translation.x = v.translation.x;
                translation.y = v.translation.y;
                scale.x = s.x;
                scale.y = s.y;
            }
        }
        Ok(PendingChanges::one(DocChange::LayerPropsChanged(
            self.layer,
        )))
    }
}

impl DocCommand for TransformLayer {
    fn label(&self) -> &str {
        "Place layer"
    }

    fn apply(&mut self, p: &mut super::Project) -> Result<PendingChanges, CommandError> {
        self.write(p, self.to)
    }

    fn revert(&mut self, p: &mut super::Project) -> Result<PendingChanges, CommandError> {
        self.write(p, self.from)
    }

    /// A drag is one undo step, not one per mouse-move.
    fn merge(&mut self, next: &dyn DocCommand) -> bool {
        match next.as_any().downcast_ref::<TransformLayer>() {
            Some(n) if n.layer == self.layer => {
                self.to = n.to;
                true
            }
            _ => false,
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Resize the stage: what the audience sees, in pixels.
///
/// Document state, not a preference. The stage is the frame the show is
/// composed against — every placement an operator makes is relative to it —
/// so changing it after the fact re-crops the whole show and has to be as
/// undoable as moving a puppet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetStageCanvas {
    pub from: [u32; 2],
    pub to: [u32; 2],
}

impl DocCommand for SetStageCanvas {
    fn label(&self) -> &str {
        "Set output resolution"
    }

    fn apply(&mut self, p: &mut super::Project) -> Result<PendingChanges, CommandError> {
        // Never zero on either axis: the projector camera divides by these,
        // and a zero-width stage is a division by zero on the show's own path.
        p.stage.canvas = [self.to[0].max(1), self.to[1].max(1)];
        Ok(PendingChanges::one(DocChange::LayerOrderChanged))
    }

    fn revert(&mut self, p: &mut super::Project) -> Result<PendingChanges, CommandError> {
        p.stage.canvas = [self.from[0].max(1), self.from[1].max(1)];
        Ok(PendingChanges::one(DocChange::LayerOrderChanged))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Show or hide a layer.
///
/// Its own command rather than a `SetLayerScalar` variant because visibility
/// is a switch, not a value on a scale: merging two hides into one would be
/// wrong, and a slider that snaps between 0 and 1 is not what an eye icon is.
#[derive(Debug, Clone, PartialEq)]
pub struct SetLayerVisible {
    pub layer: LayerId,
    pub from: bool,
    pub to: bool,
}

impl SetLayerVisible {
    fn write(&self, p: &mut super::Project, v: bool) -> Result<PendingChanges, CommandError> {
        let l = p
            .layer_data
            .get_mut(&self.layer)
            .ok_or(CommandError::NoSuchLayer(self.layer))?;
        l.visible = v;
        Ok(PendingChanges::one(DocChange::LayerPropsChanged(
            self.layer,
        )))
    }
}

impl DocCommand for SetLayerVisible {
    fn label(&self) -> &str {
        "Hide layer"
    }

    fn apply(&mut self, p: &mut super::Project) -> Result<PendingChanges, CommandError> {
        self.write(p, self.to)
    }

    fn revert(&mut self, p: &mut super::Project) -> Result<PendingChanges, CommandError> {
        self.write(p, self.from)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Delete a layer, everything on it, and its place in the paint order.
///
/// A layer owns its puppets, so deleting one has to take them with it — an
/// orphaned puppet would still be in `Project::puppets`, still projected into
/// the scene, and no longer reachable from any panel. Undo has to put all
/// three back: the layer, its position in the order, and each puppet at its
/// own index, or an undo would quietly reorder the show.
/// What a delete took, so undo can put every piece back where it was.
#[derive(Debug, Clone)]
struct RemovedLayer {
    layer: Layer,
    /// Its index in the paint order.
    order: usize,
    /// Its puppets, each with the index it held in `Project::puppets`.
    puppets: Vec<(usize, Puppet)>,
}

#[derive(Debug, Clone)]
pub struct RemoveLayer {
    pub layer: LayerId,
    removed: Option<RemovedLayer>,
}

impl RemoveLayer {
    pub fn new(layer: LayerId) -> Self {
        Self {
            layer,
            removed: None,
        }
    }
}

impl DocCommand for RemoveLayer {
    fn label(&self) -> &str {
        "Delete layer"
    }

    fn apply(&mut self, p: &mut super::Project) -> Result<PendingChanges, CommandError> {
        let order = p
            .layers
            .iter()
            .position(|l| *l == self.layer)
            .ok_or(CommandError::NoSuchLayer(self.layer))?;
        let layer = p
            .layer_data
            .shift_remove(&self.layer)
            .ok_or(CommandError::NoSuchLayer(self.layer))?;
        p.layers.remove(order);

        let mut changes = vec![DocChange::LayerRemoved(self.layer)];
        let mut taken: Vec<(usize, Puppet)> = Vec::new();
        // Descending, so each removal cannot disturb the index of the next.
        let mut wanted: Vec<PuppetId> = layer.contents.clone();
        wanted.sort_by_key(|id| std::cmp::Reverse(p.puppets.get_index_of(id)));
        for id in wanted {
            if let Some(index) = p.puppets.get_index_of(&id)
                && let Some(puppet) = p.puppets.shift_remove(&id)
            {
                taken.push((index, puppet));
                changes.push(DocChange::PuppetRemoved(id));
            }
        }

        self.removed = Some(RemovedLayer {
            layer,
            order,
            puppets: taken,
        });
        Ok(PendingChanges(changes))
    }

    fn revert(&mut self, p: &mut super::Project) -> Result<PendingChanges, CommandError> {
        let Some(RemovedLayer {
            layer,
            order,
            puppets: taken,
        }) = self.removed.take()
        else {
            return Ok(PendingChanges::default());
        };
        let id = layer.id;
        let mut changes = Vec::new();

        // Ascending, so every insertion lands at the index it was taken from.
        for (index, puppet) in taken.into_iter().rev() {
            let pid = puppet.id;
            p.puppets.insert(pid, puppet);
            let last = p.puppets.len() - 1;
            p.puppets.move_index(last, index.min(last));
            changes.push(DocChange::PuppetAdded(pid));
        }

        p.layer_data.insert(id, layer);
        let last = p.layer_data.len() - 1;
        p.layer_data.move_index(last, order.min(last));
        p.layers.insert(order.min(p.layers.len()), id);
        changes.push(DocChange::LayerAdded(id));
        Ok(PendingChanges(changes))
    }

    fn memory_bytes(&self) -> usize {
        let held = self
            .removed
            .as_ref()
            .map(|r| {
                r.layer.name.len()
                    + r.puppets
                        .iter()
                        .map(|(_, p)| puppet_bytes(p))
                        .sum::<usize>()
            })
            .unwrap_or(0);
        std::mem::size_of_val(self) + held
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Copy a layer and everything on it, directly above the original.
///
/// **New identities are minted once and then reused.** Redo must not allocate
/// a fresh set: bindings, selections and clip targets all refer to puppets by
/// ID, so an undo-redo cycle that renamed everything would silently break
/// every reference the operator had already made to the copy.
#[derive(Debug, Clone)]
pub struct DuplicateLayer {
    pub source: LayerId,
    /// `(the copy, its puppets, where it goes in the paint order)`.
    made: Option<(Layer, Vec<Puppet>, usize)>,
}

impl DuplicateLayer {
    pub fn new(source: LayerId) -> Self {
        Self { source, made: None }
    }
}

impl DocCommand for DuplicateLayer {
    fn label(&self) -> &str {
        "Duplicate layer"
    }

    fn apply(&mut self, p: &mut super::Project) -> Result<PendingChanges, CommandError> {
        if self.made.is_none() {
            let source = p
                .layer_data
                .get(&self.source)
                .ok_or(CommandError::NoSuchLayer(self.source))?
                .clone();
            let order = p
                .layers
                .iter()
                .position(|l| *l == self.source)
                .ok_or(CommandError::NoSuchLayer(self.source))?;

            let mut layer = source.clone();
            layer.id = LayerId(p.alloc_id());
            layer.name = format!("{} copy", source.name);
            layer.contents.clear();

            let mut puppets = Vec::new();
            for pid in &source.contents {
                let Some(original) = p.puppets.get(pid) else {
                    continue;
                };
                let mut copy = original.clone();
                copy.id = PuppetId(p.alloc_id());
                layer.contents.push(copy.id);
                puppets.push(copy);
            }
            self.made = Some((layer, puppets, order + 1));
        }

        let (layer, puppets, order) = self.made.clone().expect("just filled");
        let mut changes = Vec::new();
        for puppet in puppets {
            let id = puppet.id;
            p.puppets.insert(id, puppet);
            changes.push(DocChange::PuppetAdded(id));
        }
        let id = layer.id;
        p.layer_data.insert(id, layer);
        p.layers.insert(order.min(p.layers.len()), id);
        changes.push(DocChange::LayerAdded(id));
        Ok(PendingChanges(changes))
    }

    fn revert(&mut self, p: &mut super::Project) -> Result<PendingChanges, CommandError> {
        let Some((layer, puppets, _)) = self.made.as_ref() else {
            return Ok(PendingChanges::default());
        };
        let mut changes = Vec::new();
        let id = layer.id;
        p.layer_data.shift_remove(&id);
        p.layers.retain(|l| *l != id);
        changes.push(DocChange::LayerRemoved(id));
        for puppet in puppets {
            p.puppets.shift_remove(&puppet.id);
            changes.push(DocChange::PuppetRemoved(puppet.id));
        }
        Ok(PendingChanges(changes))
    }

    fn memory_bytes(&self) -> usize {
        let held = self
            .made
            .as_ref()
            .map(|(l, ps, _)| l.name.len() + ps.iter().map(puppet_bytes).sum::<usize>())
            .unwrap_or(0);
        std::mem::size_of_val(self) + held
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Add a layer at the top of the paint order.
#[derive(Debug, Clone)]
pub struct AddLayer {
    pub layer: Layer,
}

impl DocCommand for AddLayer {
    fn label(&self) -> &str {
        "Add layer"
    }

    fn apply(&mut self, p: &mut super::Project) -> Result<PendingChanges, CommandError> {
        let id = self.layer.id;
        p.layer_data.insert(id, self.layer.clone());
        p.layers.push(id);
        Ok(PendingChanges::one(DocChange::LayerAdded(id)))
    }

    fn revert(&mut self, p: &mut super::Project) -> Result<PendingChanges, CommandError> {
        let id = self.layer.id;
        p.layer_data.shift_remove(&id);
        p.layers.retain(|l| *l != id);
        Ok(PendingChanges::one(DocChange::LayerRemoved(id)))
    }

    fn memory_bytes(&self) -> usize {
        std::mem::size_of_val(self) + self.layer.name.len()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
