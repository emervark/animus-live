//! Schema migrations.
//!
//! Migrations operate on the **raw JSON**, before any typed construction.
//! That way we never have to keep old versions of the document structs
//! around, and a migration can restructure freely.
//!
//! To add schema version N+1:
//!   1. Bump `CURRENT_SCHEMA_VERSION` to N+1.
//!   2. Add `vN_to_vN1.rs` and append it to `MIGRATIONS`.
//!   3. Add `spec/fixtures/vN_sample/` containing a project at version N.
//!
//! Step 3 is not optional — CI fails a schema bump with no fixture.

mod v1_to_v2;

use crate::doc::CURRENT_SCHEMA_VERSION;
use serde_json::Value;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum MigrateError {
    #[error("project schema version {found} is newer than this build supports ({supported})")]
    FromTheFuture { found: u32, supported: u32 },
    #[error("migration {from} -> {to} failed: {reason}")]
    Failed { from: u32, to: u32, reason: String },
    /// `0` is never a valid `schema_version` — versions start at 1 — so a
    /// file claiming version 0 is malformed (hand-edited or truncated),
    /// not merely old. Rejected explicitly here rather than reaching
    /// `MIGRATIONS[(v - 1) as usize]` below with `v == 0`: `v - 1`
    /// underflows a `u32` (panics in debug, wraps to a huge index that
    /// then panics on out-of-bounds access in release) once `MIGRATIONS`
    /// is non-empty. A malformed file must produce an error, not a panic.
    #[error("project schema version {found} is not valid: schema versions start at 1")]
    InvalidVersion { found: u32 },
}

pub type Migration = fn(&mut Value) -> Result<(), MigrateError>;

/// Index `i` migrates schema version `i + 1` to `i + 2`.
///
/// So for a chain covering versions 1..=N, `MIGRATIONS` has `N - 1`
/// entries, and `MIGRATIONS[0]` is the 1->2 step. In `run` below, the loop
/// variable `v` ranges over the *source* version of each step
/// (`from..CURRENT_SCHEMA_VERSION`), so the step that migrates version `v`
/// to `v + 1` lives at `MIGRATIONS[v - 1]`. Worked example for a
/// two-step chain (`CURRENT_SCHEMA_VERSION == 3`, `MIGRATIONS.len() == 2`):
/// starting from `v1`, the loop runs `v = 1` then `v = 2`; `v = 1` picks
/// `MIGRATIONS[0]` (1->2) and `v = 2` picks `MIGRATIONS[1]` (2->3) — which
/// is exactly the v1_to_v2, v2_to_v3 order the migrations were appended in.
pub const MIGRATIONS: &[Migration] = &[];

pub fn run(value: &mut Value, from: u32) -> Result<(), MigrateError> {
    if from == 0 {
        return Err(MigrateError::InvalidVersion { found: from });
    }
    if from > CURRENT_SCHEMA_VERSION {
        return Err(MigrateError::FromTheFuture {
            found: from,
            supported: CURRENT_SCHEMA_VERSION,
        });
    }
    for v in from..CURRENT_SCHEMA_VERSION {
        let step = MIGRATIONS[(v - 1) as usize];
        step(value)?;
        value["schema_version"] = Value::from(v + 1);
    }
    Ok(())
}
