//! Wiring, and the schedule positions that matter. Spec §8.2.

use animus_core::doc::Project;
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
            .init_resource::<DocRevision>()
            .init_resource::<PendingChangesRes>()
            .init_resource::<EntityIndex>()
            .init_resource::<BuildWarnings>()
            .add_systems(Update, sync_document.in_set(SyncSet::Apply));
    }
}
