//! Project load, with a schema-version gate ahead of full deserialization.

use crate::error::ProjectError;
use animus_core::doc::{CURRENT_SCHEMA_VERSION, Project};
use animus_core::migrate::{self, MigrateError};
use std::fs;
use std::path::Path;

/// Read and parse `dir/project.json` into a `Project`.
///
/// The raw JSON is parsed to a `serde_json::Value` first so `schema_version`
/// can be read and checked *before* any attempt to deserialize into
/// `Project`. A file from a newer, unknown schema is refused outright —
/// `SchemaTooNew` — rather than deserialized and hoped for the best: a
/// future format could add or repurpose fields in a way that would
/// otherwise produce a confusing `serde` type error, or worse, silently
/// misinterpreted data, deep inside an unrelated field.
pub fn load(dir: &Path) -> Result<Project, ProjectError> {
    let text = fs::read_to_string(dir.join("project.json"))?;
    let mut value: serde_json::Value = serde_json::from_str(&text)?;

    let found = value
        .get("schema_version")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| ProjectError::Migration {
            from: 0,
            to: CURRENT_SCHEMA_VERSION,
            reason: "project.json has no schema_version field".to_string(),
        })? as u32;

    migrate::run(&mut value, found).map_err(|e| match e {
        MigrateError::FromTheFuture { found, supported } => {
            ProjectError::SchemaTooNew { found, supported }
        }
        MigrateError::Failed { from, to, reason } => ProjectError::Migration { from, to, reason },
        MigrateError::InvalidVersion { found } => ProjectError::Migration {
            from: found,
            to: CURRENT_SCHEMA_VERSION,
            reason: "schema versions start at 1; 0 is not a valid schema_version".to_string(),
        },
    })?;

    Ok(serde_json::from_value(value)?)
}
