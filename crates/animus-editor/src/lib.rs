//! The Animus Live editor: dock, panels, tools and theme.
//!
//! The visual system is Showmesh's, lifted rather than reinterpreted — see
//! [`theme`] and spec §10.6.
#![forbid(unsafe_code)]

pub mod chrome;
pub mod dock;
pub mod drag;
pub mod files;
pub mod fit;
pub mod gizmos;
pub mod hit;
pub mod icons;
pub mod import;
pub mod inspect;
pub mod interact;
pub mod mode;
pub mod plugin;
pub mod rig;
pub mod rotate;
pub mod state;
pub mod theme;
pub mod viewport;
pub mod widgets;

pub use import::{ImportStatus, ProjectRoot};
pub use plugin::{EditorPlugin, EditorSet};
pub use state::{EditMode, EditorState, LeftTab, RightTab, Selection, Tool};
pub use viewport::{ViewportInput, ViewportTarget};
