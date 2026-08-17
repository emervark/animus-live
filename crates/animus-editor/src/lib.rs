//! The Animus Live editor: dock, panels, tools and theme.
//!
//! The visual system is Showmesh's, lifted rather than reinterpreted — see
//! [`theme`] and spec §10.6.
#![forbid(unsafe_code)]

pub mod dock;
pub mod plugin;
pub mod state;
pub mod theme;
pub mod viewport;

pub use plugin::{EditorPlugin, EditorSet};
pub use state::{EditMode, EditorState, Selection, TabKind, Tool};
pub use viewport::{ViewportInput, ViewportTarget};
