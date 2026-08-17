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

use super::{AssetRef, AttachmentTable, Layer, Puppet, PuppetKind, SkeletonData};
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
