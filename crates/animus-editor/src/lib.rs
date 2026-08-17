//! The Animus Live editor: dock, panels, tools and theme.
//!
//! The visual system is Showmesh's, lifted rather than reinterpreted — see
//! [`theme`] and spec §10.6.
#![forbid(unsafe_code)]

pub mod dock;
pub mod drag;
pub mod gizmos;
pub mod import;
pub mod interact;
pub mod plugin;
pub mod rig;
pub mod state;
pub mod theme;
pub mod viewport;

pub use import::{ImportStatus, ProjectRoot};
pub use plugin::{EditorPlugin, EditorSet};
pub use state::{EditMode, EditorState, Selection, TabKind, Tool};
pub use viewport::{ViewportInput, ViewportTarget};
