//! Atomic project save.

use crate::error::ProjectError;
use crate::json::to_json;
use animus_core::doc::Project;
use std::fs::{self, File};
use std::path::Path;

/// Write `project` to `dir/project.json`, atomically.
///
/// `save` serializes exactly what it is given — it never touches
/// `project.meta.modified_utc` or any other field. Stamping the modified
/// time is the caller's job, done before calling `save`.
///
/// The write itself is tmp-then-rename:
/// 1. `dir` and `dir/assets` are created if missing.
/// 2. The JSON text is written to `dir/project.json.tmp`.
/// 3. The temp file is `fsync`ed (`File::sync_all`) so its bytes are on
///    disk, not just buffered.
/// 4. `dir/project.json.tmp` is renamed onto `dir/project.json`.
///
/// `rename` is atomic on both NTFS and POSIX, so at every instant
/// `project.json` either doesn't exist yet, or is the previous complete
/// save, or is this complete save — never a truncated or partial write. A
/// crash between steps 2 and 4 leaves the previous `project.json` (or none,
/// on a first save) untouched; `project.json` is never written in place.
pub fn save(project: &Project, dir: &Path) -> Result<(), ProjectError> {
    fs::create_dir_all(dir)?;
    fs::create_dir_all(dir.join("assets"))?;

    let json = to_json(project)?;

    let tmp_path = dir.join("project.json.tmp");
    let final_path = dir.join("project.json");

    {
        let mut tmp = File::create(&tmp_path)?;
        use std::io::Write;
        tmp.write_all(json.as_bytes())?;
        tmp.sync_all()?;
    }

    fs::rename(&tmp_path, &final_path)?;
    Ok(())
}
