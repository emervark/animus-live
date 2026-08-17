//! Autosave: the show survives the crash.
//!
//! Every interval, and only when the document actually changed, the project
//! is written to a **sidecar** (`autosave/` inside the project directory) —
//! never over the operator's own save. Overwriting the real file on a timer
//! means one bad state mid-drag becomes the saved state; a sidecar means
//! recovery is offered, not imposed.
//!
//! On startup, a sidecar newer than the main save is reported so the
//! operator decides. M1 surfaces this in the log and import status line;
//! a dialog is M5 polish.

use std::path::{Path, PathBuf};
use std::time::Duration;

use animus_runtime::{DocRevision, DocumentRes};
use bevy::prelude::*;

use animus_editor::ProjectRoot;

/// How often to consider saving. Long enough not to matter next to a
/// 120Hz solver; short enough that a crash costs minutes, not an evening.
pub const INTERVAL: Duration = Duration::from_secs(120);

#[derive(Resource, Debug)]
pub struct AutosaveState {
    pub timer: Timer,
    /// The revision last written, so an idle editor writes nothing.
    pub saved_revision: u64,
    /// The last outcome, for the status bar.
    pub last_result: Option<String>,
}

impl Default for AutosaveState {
    fn default() -> Self {
        Self {
            timer: Timer::new(INTERVAL, TimerMode::Repeating),
            saved_revision: 0,
            last_result: None,
        }
    }
}

pub fn sidecar_dir(project_root: &Path) -> PathBuf {
    project_root.join("autosave")
}

/// Write the sidecar when the interval elapses and the document moved.
pub fn autosave(
    time: Res<Time>,
    mut state: ResMut<AutosaveState>,
    doc: Res<DocumentRes>,
    revision: Res<DocRevision>,
    root: Res<ProjectRoot>,
) {
    state.timer.tick(time.delta());
    if !state.timer.just_finished() {
        return;
    }
    if revision.global == state.saved_revision {
        return; // nothing changed; write nothing
    }

    let dir = sidecar_dir(&root.0);
    match animus_project::save(&doc.0, &dir) {
        Ok(()) => {
            state.saved_revision = revision.global;
            state.last_result = Some(format!("autosaved to {}", dir.display()));
            info!("autosaved to {}", dir.display());
        }
        Err(e) => {
            // An autosave failure must be visible but never fatal: the show
            // goes on, the operator sees the warning.
            state.last_result = Some(format!("autosave FAILED: {e}"));
            warn!("autosave failed: {e}");
        }
    }
}

/// At startup: does a sidecar exist that the operator should know about?
pub fn report_existing_sidecar(root: &Path) -> Option<String> {
    let dir = sidecar_dir(root);
    if dir.join("project.json").exists() {
        Some(format!(
            "an autosave exists at {} — if the last session ended badly, open it",
            dir.display()
        ))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use animus_core::doc::Project;

    #[test]
    fn the_sidecar_lives_inside_the_project_and_not_over_it() {
        let dir = sidecar_dir(Path::new("MyShow.animus"));
        assert_eq!(dir, Path::new("MyShow.animus").join("autosave"));
    }

    #[test]
    fn a_sidecar_round_trips_through_the_real_codec() {
        // The autosave is only worth anything if load() accepts it.
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("Show.animus");
        let project = Project::new("Autosave Test");

        animus_project::save(&project, &sidecar_dir(&root)).expect("saves");
        let loaded = animus_project::load(&sidecar_dir(&root)).expect("loads back");
        assert_eq!(loaded.meta.name, "Autosave Test");

        assert!(report_existing_sidecar(&root).is_some());
        assert!(report_existing_sidecar(tmp.path()).is_none());
    }
}
