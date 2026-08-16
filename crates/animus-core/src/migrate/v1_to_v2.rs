//! Template for the first real migration.
//!
//! There is no version 2 yet, so this function is never called and never
//! appended to `MIGRATIONS` — it exists only so the shape of a migration
//! (a `Migration` function operating on raw JSON, one `mod.rs` per step)
//! is visible to whoever writes the first real one, instead of them
//! inventing the pattern from scratch. `#[allow(dead_code)]` because CI
//! builds with `-D warnings`, and an unused-but-intentional function would
//! otherwise fail the build.
#![allow(dead_code)]

use super::MigrateError;
use serde_json::Value;

fn migrate(_value: &mut Value) -> Result<(), MigrateError> {
    // Example shape for a real migration:
    //
    //   let obj = value.as_object_mut().ok_or_else(|| MigrateError::Failed {
    //       from: 1,
    //       to: 2,
    //       reason: "project root is not a JSON object".to_string(),
    //   })?;
    //   obj.insert("new_field".to_string(), Value::from(default_value));
    //
    // `run` sets `schema_version` after this returns `Ok`, so this
    // function should not touch it itself.
    Ok(())
}
