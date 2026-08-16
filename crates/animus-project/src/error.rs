//! Errors for the project codec, save/load, and asset store.

use thiserror::Error;

/// Everything that can go wrong reading or writing an Animus project.
#[derive(Debug, Error)]
pub enum ProjectError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// `serde_json` silently serializes `f32::NAN` and `f32::INFINITY` as
    /// `null`, which would corrupt the file without any error at all. This
    /// is raised by an explicit walk over the typed value, before it is
    /// handed to `serde_json` at all, so a bad float is caught at the
    /// point of failure instead of surfacing as a baffling type error the
    /// next time the file loads. See `json::to_json`'s doc comment for why
    /// the walk has to happen before serialization, not over the resulting
    /// `serde_json::Value`.
    #[error("non-finite float at {path}")]
    NonFiniteFloat { path: String },

    /// The file was written by a newer version of Animus Live than this
    /// build understands. Refused outright rather than guessed at, because
    /// guessing at an unknown format is how shows get corrupted.
    #[error("project schema version {found} is newer than the {supported} this build supports")]
    SchemaTooNew { found: u32, supported: u32 },

    #[error("migration from schema {from} to {to} failed: {reason}")]
    Migration { from: u32, to: u32, reason: String },
}
