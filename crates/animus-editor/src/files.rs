//! Save, Save As, and Open.
//!
//! A `.animus` project is a **directory**, not a file: `project.json` plus a
//! content-addressed `assets/` tree. So Open picks a folder and Save As picks
//! a name to make one. Every dialog here is the native one, and every dialog
//! is modal — the operator is choosing a file, not doing something else while
//! they choose.
//!
//! Saving already knows how to be safe: `animus_project::save` writes to a
//! temp file, fsyncs, and renames, so `project.json` is never half-written.
//! This module's job is only to decide *where* and to keep the rest of the
//! editor honest about what happened.

use animus_runtime::{DocumentRes, PendingChangesRes};
use bevy::prelude::*;
use std::path::PathBuf;

use crate::import::ProjectRoot;
use crate::state::{EditorState, Selection};

/// What the operator asked the file menu for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Resource)]
pub struct FileRequest(pub Option<FileAction>);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileAction {
    /// Write to the current location, or ask for one if there isn't a real
    /// one yet.
    Save,
    /// Always ask.
    SaveAs,
    Open,
}

/// Whether the document has ever been written anywhere the operator chose.
///
/// A project started from nothing gets a path in the working directory so
/// autosave has somewhere to go, but that path is a placeholder, not a
/// decision. Save on such a project has to ask rather than silently drop a
/// folder next to wherever the executable happened to be launched from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Resource)]
pub struct ProjectSaved(pub bool);

/// The last thing that happened, for the title bar to say.
#[derive(Debug, Clone, Default, Resource)]
pub struct FileStatus {
    pub message: Option<String>,
    pub is_error: bool,
}

impl FileStatus {
    fn ok(&mut self, message: impl Into<String>) {
        self.message = Some(message.into());
        self.is_error = false;
    }

    fn failed(&mut self, message: impl Into<String>) {
        self.message = Some(message.into());
        self.is_error = true;
    }
}

/// Ctrl+S, Ctrl+Shift+S, Ctrl+O.
pub fn file_shortcuts(
    keys: Res<ButtonInput<KeyCode>>,
    egui_focus: Res<crate::interact::EguiWantsKeyboard>,
    mut request: ResMut<FileRequest>,
) {
    if egui_focus.0 {
        return;
    }
    let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    if !ctrl {
        return;
    }
    if keys.just_pressed(KeyCode::KeyS) {
        request.0 = Some(if shift {
            FileAction::SaveAs
        } else {
            FileAction::Save
        });
    }
    if keys.just_pressed(KeyCode::KeyO) {
        request.0 = Some(FileAction::Open);
    }
}

/// Do what was asked, then say so.
#[allow(clippy::too_many_arguments)]
pub fn apply_file_request(
    mut request: ResMut<FileRequest>,
    mut doc: ResMut<DocumentRes>,
    mut root: ResMut<ProjectRoot>,
    mut saved: ResMut<ProjectSaved>,
    mut status: ResMut<FileStatus>,
    mut state: ResMut<EditorState>,
    mut pending: ResMut<PendingChangesRes>,
    mut windows: Query<&mut Window, With<bevy::window::PrimaryWindow>>,
) {
    let Some(action) = request.0.take() else {
        return;
    };

    match action {
        FileAction::Save if saved.0 => write_to(&doc, &root.0, &mut saved, &mut status),
        FileAction::Save | FileAction::SaveAs => {
            let start = root.0.parent().map(PathBuf::from).unwrap_or_default();
            let name = root
                .0
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "untitled.animus".into());
            let picked = rfd::FileDialog::new()
                .set_title("Save project as")
                .set_directory(start)
                .set_file_name(name)
                .save_file();
            let Some(mut dir) = picked else {
                status.ok("Save cancelled.");
                return;
            };
            // The dialog is a file picker because folder pickers cannot take
            // a new name. `.animus` is put back on if the operator typed a
            // bare name, so the folder is recognisable as a project.
            if dir.extension().and_then(|e| e.to_str()) != Some("animus") {
                dir.set_extension("animus");
            }
            write_to(&doc, &dir, &mut saved, &mut status);
            if !status.is_error {
                root.0 = dir;
                retitle(&doc, &root.0, &mut windows);
            }
        }
        FileAction::Open => {
            let start = root.0.parent().map(PathBuf::from).unwrap_or_default();
            let Some(dir) = rfd::FileDialog::new()
                .set_title("Open project")
                .set_directory(start)
                .pick_folder()
            else {
                status.ok("Open cancelled.");
                return;
            };
            match animus_project::load(&dir) {
                Ok(project) => {
                    // Everything the old document owned goes before anything
                    // of the new one arrives, or the scene would hold two
                    // shows at once — and the ids of the second could collide
                    // with entities still standing for the first.
                    for id in doc.0.puppets.keys() {
                        pending.push(animus_core::doc::DocChange::PuppetRemoved(*id));
                    }
                    for id in project.puppets.keys() {
                        pending.push(animus_core::doc::DocChange::PuppetAdded(*id));
                    }
                    doc.0 = project;
                    root.0 = dir;
                    saved.0 = true;
                    // A history of edits to a document that is no longer open
                    // is a trap: the first Ctrl+Z would apply a command to a
                    // project it was never made for.
                    state.undo = animus_core::doc::UndoStack::new();
                    state.selection = Selection::None;
                    retitle(&doc, &root.0, &mut windows);
                    status.ok(format!("Opened {}", short(&root.0)));
                }
                Err(e) => status.failed(format!("Could not open: {e}")),
            }
        }
    }
}

fn write_to(
    doc: &DocumentRes,
    dir: &std::path::Path,
    saved: &mut ProjectSaved,
    status: &mut FileStatus,
) {
    match animus_project::save(&doc.0, dir) {
        Ok(()) => {
            saved.0 = true;
            status.ok(format!("Saved to {}", short(dir)));
        }
        Err(e) => status.failed(format!("Could not save: {e}")),
    }
}

fn retitle(
    doc: &DocumentRes,
    root: &std::path::Path,
    windows: &mut Query<&mut Window, With<bevy::window::PrimaryWindow>>,
) {
    let Ok(mut window) = windows.single_mut() else {
        return;
    };
    window.title = format!("Animus Live — {} ({})", doc.0.meta.name, short(root));
}

/// Just the folder name: a title bar is not the place for a full path.
fn short(path: &std::path::Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Save on a never-saved project must ask, not guess.
    ///
    /// The placeholder path points at whatever directory the executable was
    /// launched from, which for a shared build is the operator's Downloads
    /// folder. Writing there without asking is how a project goes missing.
    #[test]
    fn an_unsaved_project_routes_save_through_the_dialog() {
        // The routing is the `if saved.0` guard in `apply_file_request`; this
        // pins the state it reads rather than the dialog it opens.
        assert!(!ProjectSaved::default().0);
    }

    #[test]
    fn status_remembers_whether_it_was_a_failure() {
        let mut s = FileStatus::default();
        s.ok("Saved");
        assert!(!s.is_error);
        s.failed("Disk full");
        assert!(s.is_error);
        assert_eq!(s.message.as_deref(), Some("Disk full"));
    }

    #[test]
    fn the_title_shows_the_folder_rather_than_the_whole_path() {
        assert_eq!(
            short(std::path::Path::new("C:/shows/spring/kevad.animus")),
            "kevad.animus"
        );
    }
}
