//! Project load, with a schema-version gate ahead of full deserialization.

use crate::error::ProjectError;
use animus_core::doc::{CURRENT_SCHEMA_VERSION, Project};
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
    let value: serde_json::Value = serde_json::from_str(&text)?;

    let found = value
        .get("schema_version")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| ProjectError::Migration {
            from: 0,
            to: CURRENT_SCHEMA_VERSION,
            reason: "project.json has no schema_version field".to_string(),
        })? as u32;

    if found > CURRENT_SCHEMA_VERSION {
        return Err(ProjectError::SchemaTooNew {
            found,
            supported: CURRENT_SCHEMA_VERSION,
        });
    }

    if found < CURRENT_SCHEMA_VERSION {
        // Task 14 fills in the migration chain. Until then, an older
        // schema can't be read: refuse it rather than guess at how its
        // fields map onto the current shape.
        return Err(ProjectError::Migration {
            from: found,
            to: CURRENT_SCHEMA_VERSION,
            reason: "no migrations are implemented yet".to_string(),
        });
    }

    Ok(serde_json::from_value(value)?)
}
