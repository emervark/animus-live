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
use serde::{Deserialize, Serialize};

/// What the viewport draws over the artwork.
///
/// **Toggles, not a mode.** An operator rigging a hand wants the mesh and
/// the joints and nothing else; one framing a shot wants the safe area and
/// none of the rig. Bundling these into presets would mean the one
/// combination someone needs is always the one that does not exist.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Overlays {
    pub mesh: bool,
    pub bones: bool,
    pub joints: bool,
    /// The output frame, dashed: what the audience will actually see.
    pub safe: bool,
    /// The camera rectangle, for when the stage is larger than the frame.
    pub camera: bool,
}

impl Default for Overlays {
    fn default() -> Self {
        // The rig on, the framing guides off: the first thing anyone does
        // with this tool is build a skeleton, and the guides are for later.
        Self {
            mesh: true,
            bones: true,
            joints: true,
            safe: false,
            camera: false,
        }
    }
}

impl Overlays {
    /// Name, tooltip, and whether there is anything behind it yet.
    ///
    /// `CAMERA` is listed and disabled on purpose. There is no framing
    /// camera in the document — the viewport camera is a view, not a shot —
    /// so a toggle here would draw a rectangle that means nothing. Reserving
    /// the space is honest; drawing the rectangle would not be.
    pub const ALL: [(&'static str, &'static str, bool); 5] = [
        ("MESH", "Show the triangle mesh over the artwork", true),
        ("BONES", "Show the bones connecting joints", true),
        ("JOINTS", "Show the joints themselves", true),
        (
            "SAFE",
            "Show the title-safe inset: what survives a projector's overscan",
            true,
        ),
        (
            "CAMERA",
            "A framing camera would go here. The document has no such camera yet.",
            false,
        ),
    ];

    pub fn get(&self, i: usize) -> bool {
        match i {
            0 => self.mesh,
            1 => self.bones,
            2 => self.joints,
            3 => self.safe,
            _ => self.camera,
        }
    }

    pub fn toggle(&mut self, i: usize) {
        match i {
            0 => self.mesh = !self.mesh,
            1 => self.bones = !self.bones,
            2 => self.joints = !self.joints,
            3 => self.safe = !self.safe,
            _ => self.camera = !self.camera,
        }
    }
}

/// The left sidebar's three tabs: what is in the show, what it is made
/// from, and what acts on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum LeftTab {
    #[default]
    Scene,
    Assets,
    Tools,
}

impl LeftTab {
    pub const ALL: [LeftTab; 3] = [LeftTab::Scene, LeftTab::Assets, LeftTab::Tools];

    pub fn label(self) -> &'static str {
        match self {
            LeftTab::Scene => "SCENE",
            LeftTab::Assets => "ASSETS",
            LeftTab::Tools => "TOOLS",
        }
    }
}

/// The right sidebar's four tabs: the selected thing, the physics acting on
/// it, what is arriving from outside, and what that is wired to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum RightTab {
    #[default]
    Inspect,
    Physics,
    Channels,
    Bind,
}

impl RightTab {
    pub const ALL: [RightTab; 4] = [
        RightTab::Inspect,
        RightTab::Physics,
        RightTab::Channels,
        RightTab::Bind,
    ];

    pub fn label(self) -> &'static str {
        match self {
            RightTab::Inspect => "INSPECT",
            RightTab::Physics => "PHYSICS",
            RightTab::Channels => "CHANNELS",
            RightTab::Bind => "BIND",
        }
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
    pub left_tab: LeftTab,
    pub right_tab: RightTab,
    /// Sidebar widths and sequencer height, in points.
    pub panels: PanelSizes,
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
    /// What the viewport draws over the artwork.
    pub overlays: Overlays,
    /// Which binding has its envelope editor open, if any.
    ///
    /// One at a time: two curves on screen at once is two curves the
    /// operator has to tell apart, and the panel is 300px wide.
    pub open_envelope: Option<usize>,
    /// Whether the output settings are showing.
    ///
    /// Opened from the title bar's output chip rather than living in a panel:
    /// the chip already reports where the show is going, and the operator who
    /// wants to know and the one who wants to change it are reaching for the
    /// same thing.
    pub output_menu_open: bool,
}

impl Default for EditorState {
    fn default() -> Self {
        Self {
            left_tab: LeftTab::default(),
            right_tab: RightTab::default(),
            panels: load_layout().unwrap_or_default(),
            selection: Selection::None,
            tool: Tool::default(),
            mode: EditMode::default(),
            undo: UndoStack::new(),
            viewport_image: None,
            show_dev_inspector: false,
            clips_collapsed: false,
            live_offset: None,
            live_rotation: 0.0,
            overlays: Overlays::default(),
            open_envelope: None,
            output_menu_open: false,
        }
    }
}

/// How wide the sidebars are and how tall the sequencer is.
///
/// **This is the whole of the saved layout now.** The editor used to carry a
/// `DockState` — a tree of splits with every panel's rectangle in it — which
/// bought rearrangeable panels and cost a class of bug the comp does not
/// have: a panel closed by its X had nowhere to come back from, and a
/// never-laid-out tree serialized to infinities that could never be read
/// back. The layout is fixed now, so what is left to remember is three
/// numbers.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PanelSizes {
    pub left: f32,
    pub right: f32,
    pub sequencer: f32,
}

impl Default for PanelSizes {
    fn default() -> Self {
        Self {
            left: 268.0,
            right: 300.0,
            sequencer: 232.0,
        }
    }
}

impl PanelSizes {
    /// Bounds, applied on load as well as on drag.
    ///
    /// A saved file is not a trusted input: a width of zero or of NaN would
    /// leave the operator with a sidebar they cannot see and cannot grab to
    /// bring back.
    pub const LEFT: std::ops::RangeInclusive<f32> = 200.0..=420.0;
    pub const RIGHT: std::ops::RangeInclusive<f32> = 240.0..=460.0;
    /// The sequencer's ceiling is generous because the whole point of
    /// dragging it up is a pattern with more tracks than fit.
    pub const SEQUENCER: std::ops::RangeInclusive<f32> = 44.0..=900.0;

    fn sane(self) -> Self {
        let fix = |v: f32, r: &std::ops::RangeInclusive<f32>| {
            if v.is_finite() {
                v.clamp(*r.start(), *r.end())
            } else {
                *r.start()
            }
        };
        Self {
            left: fix(self.left, &Self::LEFT),
            right: fix(self.right, &Self::RIGHT),
            sequencer: fix(self.sequencer, &Self::SEQUENCER),
        }
    }
}

fn layout_path() -> Option<PathBuf> {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("animus").join("layout.json"))
}

/// Read the saved sizes, or `None` if there is not a usable file.
///
/// **Every failure here is silent on purpose.** A corrupt or
/// version-mismatched layout file must never stop the editor from opening —
/// the worst outcome of ignoring it is that the operator drags a sidebar
/// once, and the worst outcome of honouring it is that a show cannot be
/// opened at all.
pub fn load_layout() -> Option<PanelSizes> {
    let path = layout_path()?;
    let text = std::fs::read_to_string(&path).ok()?;
    match serde_json::from_str::<PanelSizes>(&text) {
        Ok(sizes) => Some(sizes.sane()),
        Err(e) => {
            warn!("ignoring unreadable layout at {}: {e}", path.display());
            None
        }
    }
}

/// Write the sizes. Failures are logged and dropped for the same reason.
pub fn save_layout(sizes: &PanelSizes) {
    let Some(path) = layout_path() else { return };
    if let Err(e) = write_layout(&path, sizes) {
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
        "the layout contains a non-finite size, which JSON cannot represent;          writing it would produce a file that can never be read back"
    )]
    NotReadableBack,
    #[error("could not write {0}: {1}")]
    Write(String, std::io::Error),
}

/// Write `sizes` to `path`, refusing to write a file that could not be read.
///
/// **The guard is load-bearing, not defensive programming.** `serde_json`
/// cannot represent a non-finite float: it writes `null` and then refuses to
/// read `null` back as an `f32`. Writing that file would leave the operator
/// with a layout that silently fails to load on every launch from then on,
/// and `load_layout` swallows errors by design, so nobody would ever see why.
///
/// This is the same rule the project format already applies to documents.
pub fn write_layout(path: &std::path::Path, sizes: &PanelSizes) -> Result<(), LayoutError> {
    if !sizes.left.is_finite() || !sizes.right.is_finite() || !sizes.sequencer.is_finite() {
        return Err(LayoutError::NotReadableBack);
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| LayoutError::CreateDir(dir.display().to_string(), e))?;
    }
    let text = serde_json::to_string_pretty(sizes).map_err(LayoutError::Serialize)?;
    std::fs::write(path, text).map_err(|e| LayoutError::Write(path.display().to_string(), e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_sizes_are_inside_their_own_bounds() {
        let d = PanelSizes::default();
        assert!(PanelSizes::LEFT.contains(&d.left));
        assert!(PanelSizes::RIGHT.contains(&d.right));
        assert!(PanelSizes::SEQUENCER.contains(&d.sequencer));
    }

    /// **A saved file is not a trusted input.** A zero width would leave the
    /// operator with a sidebar they can neither see nor grab to bring back —
    /// which is the same trap the old dock had when a panel was closed, in a
    /// new place.
    #[test]
    fn absurd_saved_sizes_are_clamped_rather_than_obeyed() {
        let sizes = PanelSizes {
            left: 0.0,
            right: 100_000.0,
            sequencer: f32::NAN,
        }
        .sane();

        assert_eq!(sizes.left, *PanelSizes::LEFT.start());
        assert_eq!(sizes.right, *PanelSizes::RIGHT.end());
        assert!(
            sizes.sequencer.is_finite(),
            "a NaN height would collapse the sequencer with no way back"
        );
    }

    #[test]
    fn sizes_are_written_and_read_back() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("animus").join("layout.json");
        let sizes = PanelSizes {
            left: 300.0,
            right: 320.0,
            sequencer: 400.0,
        };

        write_layout(&path, &sizes).expect("writes");
        assert!(path.exists(), "and the parent directory was created");

        let back: PanelSizes =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).expect("reads back");
        assert_eq!(back, sizes);
    }

    /// JSON has no way to spell infinity: it writes `null` and then refuses
    /// to read it back as an `f32`. Writing that file would break every
    /// later launch silently, because `load_layout` ignores errors by design.
    #[test]
    fn writing_a_layout_that_could_not_be_loaded_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("animus").join("layout.json");
        let broken = PanelSizes {
            left: f32::INFINITY,
            ..PanelSizes::default()
        };

        let err = write_layout(&path, &broken).expect_err("must refuse");
        assert!(matches!(err, LayoutError::NotReadableBack));
        assert!(
            !path.exists(),
            "nothing may be written: a file that cannot load is worse than none"
        );
    }

    #[test]
    fn a_corrupt_layout_file_is_ignored_rather_than_fatal() {
        // The path is process-global, so this drives the parse directly
        // rather than fighting over the real file.
        let broken: Result<PanelSizes, _> = serde_json::from_str("{ not json");
        assert!(broken.is_err());
    }

    /// Every tab has to be reachable, because there is no longer a menu that
    /// could bring a missing one back.
    #[test]
    fn every_sidebar_tab_has_a_label() {
        for t in LeftTab::ALL {
            assert!(!t.label().is_empty(), "{t:?}");
        }
        for t in RightTab::ALL {
            assert!(!t.label().is_empty(), "{t:?}");
        }
    }
}
