//! Editor state, and the layout that outlives a session.
//!
//! The dock layout is a **user preference, not project data** — it lives in
//! `%APPDATA%/animus/layout.json`, never in the `.animus` directory. Two
//! people opening the same show should each get their own arrangement, and a
//! project file should not carry someone else's panel widths.

use std::path::PathBuf;

use animus_core::doc::UndoStack;
use animus_core::ids::{BoneId, JointId, LayerId, PuppetId};
use bevy::prelude::*;
use egui_dock::{DockState, NodeIndex};
use serde::{Deserialize, Serialize};

/// The panels. `Channels` and `Bindings` exist from the start but stay inert
/// until the signal bus lands in M2 — reserving the space now means the
/// layout does not shift under people who have already learned it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TabKind {
    Viewport,
    Layers,
    Assets,
    Tools,
    Inspector,
    Solver,
    Output,
    Channels,
    Bindings,
}

impl TabKind {
    /// Every panel, in the order the View menu lists them: the two that
    /// describe the scene, the two that act on it, the two that inspect it,
    /// then the two that are still promises.
    pub const ALL: [TabKind; 9] = [
        TabKind::Viewport,
        TabKind::Layers,
        TabKind::Assets,
        TabKind::Tools,
        TabKind::Inspector,
        TabKind::Solver,
        TabKind::Output,
        TabKind::Channels,
        TabKind::Bindings,
    ];

    pub fn title(self) -> &'static str {
        match self {
            TabKind::Viewport => "Viewport",
            TabKind::Layers => "Layers",
            TabKind::Assets => "Assets",
            TabKind::Tools => "Tools",
            TabKind::Inspector => "Inspector",
            TabKind::Solver => "Solver",
            TabKind::Output => "Output",
            TabKind::Channels => "Channels",
            TabKind::Bindings => "Bindings",
        }
    }

    /// Panels that do nothing yet. They render an honest note rather than an
    /// empty box, because an empty panel reads as a bug.
    pub fn is_stub(self) -> bool {
        matches!(self, TabKind::Channels | TabKind::Bindings)
    }
}

/// What the operator is editing right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Selection {
    #[default]
    None,
    Layer(LayerId),
    Puppet(PuppetId),
    Joint(PuppetId, JointId),
    Bone(PuppetId, BoneId),
}

/// The active tool. Keyboard-first, one letter each.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tool {
    #[default]
    Select,
    Joint,
    Bone,
    Vertex,
}

impl Tool {
    pub fn label(self) -> &'static str {
        match self {
            Tool::Select => "Select",
            Tool::Joint => "Joint",
            Tool::Bone => "Bone",
            Tool::Vertex => "Vertex",
        }
    }

    pub fn key_hint(self) -> &'static str {
        match self {
            Tool::Select => "V",
            Tool::Joint => "J",
            Tool::Bone => "B",
            Tool::Vertex => "P",
        }
    }

    pub const ALL: [Tool; 4] = [Tool::Select, Tool::Joint, Tool::Bone, Tool::Vertex];
}

/// Edit mode changes what dragging a joint *means*, which is the single most
/// consequential mode in the application. Spec §8.6.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EditMode {
    /// Build the skeleton. Dragging a joint changes its **rest position** and
    /// is undoable; the mesh and the rig are what this mode edits.
    #[default]
    Rig,
    /// Pose the puppet into the selected step, frame by frame. Dragging pulls
    /// the live puppet and the result is written into that step — the rig is
    /// not touched.
    Edit,
    /// The show. Dragging pulls; nothing is written anywhere.
    Live,
}

#[derive(Resource)]
pub struct EditorState {
    pub dock: DockState<TabKind>,
    pub selection: Selection,
    pub tool: Tool,
    pub mode: EditMode,
    pub undo: UndoStack,
    /// Set by the viewport once its render target exists (Task 7).
    pub viewport_image: Option<Handle<Image>>,
    /// Developer-facing ECS inspector, off by default. `bevy-inspector-egui`
    /// renders arbitrary reflected data, which is a debugging tool and not
    /// something to put in front of an artist (spec §10.4).
    pub show_dev_inspector: bool,
    /// The clip launcher folded down to its transport row.
    ///
    /// A view preference, not document state: the audit asks for the launcher
    /// to get out of the way while mesh or rig editing needs the height.
    pub clips_collapsed: bool,
    /// How far the selected joint has been pulled from its rest position,
    /// in image pixels — the inspector's LIVE read-out.
    ///
    /// Projected here once a frame by [`crate::mode::track_selection_live`]
    /// rather than queried from the panel, because the panel runs inside
    /// egui's pass where the solver's components are not in scope. Same
    /// one-way shape as `PendingChanges`: one writer, many readers.
    pub live_offset: Option<glam::Vec2>,
    /// The selected joint's live rotation in radians, projected in the same
    /// way and for the same reason as [`Self::live_offset`].
    pub live_rotation: f32,
}

impl Default for EditorState {
    fn default() -> Self {
        Self {
            dock: load_layout().unwrap_or_else(default_layout),
            selection: Selection::None,
            tool: Tool::default(),
            mode: EditMode::default(),
            undo: UndoStack::new(),
            viewport_image: None,
            show_dev_inspector: false,
            clips_collapsed: false,
            live_offset: None,
            live_rotation: 0.0,
        }
    }
}

/// Three columns: what is in the scene, the thing itself, what it is made of.
/// Is this panel currently anywhere in the dock?
pub fn tab_is_open(dock: &DockState<TabKind>, tab: TabKind) -> bool {
    dock.iter_all_tabs().any(|(_, t)| *t == tab)
}

/// Open a panel, or close it if it is already open.
///
/// **The reason this exists**: closing a panel used to be permanent. The tab
/// bar has an X on every panel but the viewport, and nothing anywhere put one
/// back — an operator who tidied away the Solver panel had to delete their
/// saved layout from `%APPDATA%` to see it again.
///
/// A reopened panel joins the focused leaf rather than trying to remember
/// where it used to live. Remembering would mean storing a shadow layout
/// beside the real one, and the operator can drag it where they want in one
/// gesture anyway.
pub fn toggle_tab(dock: &mut DockState<TabKind>, tab: TabKind) {
    if let Some(found) = dock.find_tab(&tab) {
        // The viewport is the one surface that must not be closed: without it
        // there is nothing to edit. The menu greys it out, and this is the
        // second door.
        if tab != TabKind::Viewport {
            dock.remove_tab(found);
        }
        return;
    }
    dock.push_to_focused_leaf(tab);
}

pub fn default_layout() -> DockState<TabKind> {
    let mut dock = DockState::new(vec![TabKind::Viewport]);
    let surface = dock.main_surface_mut();

    let [viewport, left] = surface.split_left(
        NodeIndex::root(),
        0.20,
        vec![TabKind::Layers, TabKind::Assets],
    );
    surface.split_below(left, 0.55, vec![TabKind::Tools]);

    let [_viewport, right] =
        surface.split_right(viewport, 0.78, vec![TabKind::Inspector, TabKind::Solver]);
    // Output sits with Channels and Bindings: all three are the outside world
    // rather than the document. Three tabs also fit this column only because
    // these names are short — "Inspector | Solver | Output" did not, and the
    // third one rendered as "Outp".
    surface.split_below(
        right,
        0.55,
        vec![TabKind::Output, TabKind::Channels, TabKind::Bindings],
    );

    dock
}

fn layout_path() -> Option<PathBuf> {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("animus").join("layout.json"))
}

/// Read the saved layout, or `None` if there is not a usable one.
///
/// **Every failure here is silent on purpose.** A corrupt or
/// version-mismatched layout file must never stop the editor from opening —
/// the worst outcome of ignoring it is that the operator rearranges their
/// panels once, and the worst outcome of honouring it is that a show cannot
/// be opened at all.
pub fn load_layout() -> Option<DockState<TabKind>> {
    let path = layout_path()?;
    let text = std::fs::read_to_string(&path).ok()?;
    match serde_json::from_str(&text) {
        Ok(state) => Some(state),
        Err(e) => {
            warn!("ignoring unreadable layout at {}: {e}", path.display());
            None
        }
    }
}

/// Write the layout. Failures are logged and dropped for the same reason.
pub fn save_layout(dock: &DockState<TabKind>) {
    let Some(path) = layout_path() else { return };
    if let Err(e) = write_layout(&path, dock) {
        warn!("layout not saved: {e}");
    }
}

#[derive(Debug, thiserror::Error)]
pub enum LayoutError {
    #[error("could not create {0}: {1}")]
    CreateDir(String, std::io::Error),
    #[error("could not serialize: {0}")]
    Serialize(serde_json::Error),
    #[error(
        "the layout contains non-finite rectangles, which JSON cannot represent;          writing it would produce a file that can never be read back"
    )]
    NotReadableBack,
    #[error("could not write {0}: {1}")]
    Write(String, std::io::Error),
}

/// Write `dock` to `path`, refusing to write a file that could not be loaded.
///
/// **The guard is load-bearing, not defensive programming.** A `DockState`
/// that has never been laid out carries `Rect::NOTHING` — infinities — and
/// `serde_json` cannot represent a non-finite float: it writes `null` and
/// then refuses to read `null` back as an `f32`. Writing that file would
/// leave the operator with a layout that silently fails to load on every
/// launch from then on, and `load_layout` swallows errors by design, so
/// nobody would ever see why.
///
/// This is the same rule the project format already applies to documents: a
/// non-finite float is rejected at serialization time rather than persisted.
pub fn write_layout(path: &std::path::Path, dock: &DockState<TabKind>) -> Result<(), LayoutError> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| LayoutError::CreateDir(dir.display().to_string(), e))?;
    }
    let text = serde_json::to_string_pretty(dock).map_err(LayoutError::Serialize)?;
    if serde_json::from_str::<DockState<TabKind>>(&text).is_err() {
        return Err(LayoutError::NotReadableBack);
    }
    std::fs::write(path, text).map_err(|e| LayoutError::Write(path.display().to_string(), e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_layout_contains_every_panel_exactly_once() {
        let dock = default_layout();
        let mut seen: Vec<TabKind> = dock.iter_all_tabs().map(|(_, t)| *t).collect();
        seen.sort_by_key(|t| format!("{t:?}"));

        // Straight from `ALL`, so adding a panel updates the list in one
        // place. It was written out by hand and the copy went stale the first
        // time a panel was added — which is the failure this test exists to
        // catch, so it should not be able to happen to the test itself.
        let mut expected = TabKind::ALL.to_vec();
        expected.sort_by_key(|t| format!("{t:?}"));

        assert_eq!(
            seen, expected,
            "a panel that is not in the default layout is unreachable for anyone \
             who has never saved a layout"
        );
    }

    /// Documents a real limitation rather than asserting a wish.
    ///
    /// A freshly built `DockState` has `Rect::NOTHING` rects — infinities —
    /// and `serde_json` writes those as `null` and then cannot read them
    /// back. Once egui has laid the dock out for a frame the rects are
    /// finite and it round-trips. This test exists so that behaviour is
    /// written down instead of being rediscovered as "layouts never load".
    #[test]
    fn a_never_laid_out_layout_does_not_round_trip_through_json() {
        let dock = default_layout();
        let text = serde_json::to_string(&dock).expect("serializes, with nulls");
        assert!(
            text.contains("null"),
            "the rects should be non-finite before the first layout pass"
        );
        assert!(
            serde_json::from_str::<DockState<TabKind>>(&text).is_err(),
            "and JSON cannot read them back"
        );
    }

    #[test]
    fn writing_a_layout_that_could_not_be_loaded_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("animus").join("layout.json");

        let err = write_layout(&path, &default_layout()).expect_err("must refuse");
        assert!(matches!(err, LayoutError::NotReadableBack));
        assert!(
            !path.exists(),
            "nothing may be written: a file that cannot load is worse than none"
        );
    }

    #[test]
    fn a_layout_with_finite_rects_is_written_and_reads_back() {
        // egui_dock exposes `set_rect` but not `set_viewport`, so a fully
        // laid-out state cannot be built through the API. Rebuilding it from
        // its own JSON with the non-finite values filled in produces exactly
        // the shape egui leaves behind after a frame, which is what this
        // needs to exercise.
        let text = serde_json::to_string(&default_layout()).unwrap();
        // Only the point coordinates, not every null: `active_tab` is an
        // `Option<usize>` and blanket-replacing would corrupt it.
        let finite = text.replace(r#"{"x":null,"y":null}"#, r#"{"x":0.0,"y":0.0}"#);
        let dock: DockState<TabKind> =
            serde_json::from_str(&finite).expect("a laid-out layout deserializes");

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("animus").join("layout.json");

        write_layout(&path, &dock).expect("writes");
        assert!(path.exists(), "and the parent directory was created");

        let round_tripped: DockState<TabKind> =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).expect("reads back");
        assert_eq!(
            round_tripped.iter_all_tabs().count(),
            dock.iter_all_tabs().count()
        );
    }

    #[test]
    fn a_corrupt_layout_file_is_ignored_rather_than_fatal() {
        // The path is process-global, so this drives the parse directly
        // rather than fighting over the real file.
        let broken: Result<DockState<TabKind>, _> = serde_json::from_str("{ not json");
        assert!(broken.is_err());
        // and the editor's fallback is a usable layout, not an empty one
        assert!(default_layout().iter_all_tabs().count() >= 8);
    }

    /// The trap this closes: a panel closed by its X had nowhere to come
    /// back from, and the only cure was deleting the saved layout file.
    #[test]
    fn a_closed_panel_can_be_reopened() {
        let mut dock = default_layout();
        assert!(tab_is_open(&dock, TabKind::Solver));

        toggle_tab(&mut dock, TabKind::Solver);
        assert!(!tab_is_open(&dock, TabKind::Solver), "it closed");

        toggle_tab(&mut dock, TabKind::Solver);
        assert!(tab_is_open(&dock, TabKind::Solver), "and it came back");
    }

    /// The viewport is the one surface that must survive the menu, the X, and
    /// anything else: without it there is nothing to edit.
    #[test]
    fn the_viewport_cannot_be_toggled_away() {
        let mut dock = default_layout();
        toggle_tab(&mut dock, TabKind::Viewport);
        assert!(tab_is_open(&dock, TabKind::Viewport));
    }

    /// Every panel must be reachable from the menu, or closing it is still a
    /// one-way door for whichever one got left off the list.
    #[test]
    fn every_panel_in_the_default_layout_is_listed_in_the_menu() {
        let dock = default_layout();
        for (_, tab) in dock.iter_all_tabs() {
            assert!(
                TabKind::ALL.contains(tab),
                "{tab:?} is in the layout but not in TabKind::ALL, so nothing can reopen it"
            );
        }
    }
}
