//! Wiring, and the schedule positions that matter. Spec §8.2.

use animus_core::doc::{DocChange, Project};
use bevy::prelude::*;

use crate::index::EntityIndex;
use crate::project::{
    BuildWarnings, DocRevision, DocumentRes, PendingChangesRes, RenderScale, sync_document,
};

/// Where document projection happens, in `Update`.
///
/// It runs **before** the editor's UI so a command applied this frame is
/// visible in the same frame's viewport, and before anything reads
/// `EntityIndex` expecting it to describe the current document.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SyncSet {
    Apply,
}

pub struct RuntimePlugin {
    pub document: Project,
    pub scale: RenderScale,
}

impl RuntimePlugin {
    pub fn new(document: Project) -> Self {
        Self {
            document,
            scale: RenderScale::default(),
        }
    }
}

impl Plugin for RuntimePlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(DocumentRes(self.document.clone()))
            .insert_resource(self.scale)
            .insert_resource(announce(&self.document))
            .init_resource::<DocRevision>()
            .init_resource::<EntityIndex>()
            .init_resource::<crate::project::PuppetTextures>()
            .init_resource::<BuildWarnings>()
            .add_systems(Update, sync_document.in_set(SyncSet::Apply));
    }
}

/// The puppets a document arrives holding, stated as changes.
///
/// [`sync_document`] projects *changes*, so a document that arrives whole —
/// which is every project opened from disk — describes puppets that nothing
/// ever asked it to spawn. Without this, opening a saved show filled the
/// layer list from `DocumentRes` and left the stage empty: no mesh, no
/// gizmos, nothing on the projector, and no error to explain it.
///
/// Stated here, at build time, rather than from a `Startup` system: the
/// announcement is then the *first* thing in the queue by construction,
/// ahead of anything a later frame pushes, instead of landing wherever the
/// schedule happened to put it.
fn announce(document: &Project) -> PendingChangesRes {
    let mut pending = PendingChangesRes::default();
    pending.extend(document.puppets.keys().copied().map(DocChange::PuppetAdded));
    pending
}
