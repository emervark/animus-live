//! Files written before a field existed must still load.
//!
//! The M1 plan's "adding an alpha source later needs no migration" rests
//! entirely on this property, and after both committed fixtures were
//! regenerated there was nothing left in the corpus that lacked the field —
//! so the guarantee had no test at all. This is that test.
//!
//! It works by *removing* the key from a real committed project rather than
//! by hand-writing JSON, so it keeps testing the real shape as the format
//! grows.

use animus_core::doc::{MatteMode, PuppetKind};
use animus_project::load;
use std::path::{Path, PathBuf};

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("spec")
        .join("fixtures")
        .join("sample-project")
}

/// Recursively copy a directory. `fs::copy` on Windows fails with
/// "access is denied" when handed a directory, so the recursion has to be
/// explicit rather than implied.
fn copy_tree(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for entry in std::fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let target = dst.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).unwrap();
        }
    }
}

/// Copy the committed fixture into a temp dir, minus one key.
fn fixture_without(key: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let project_dir = dir.path().join("Sample.animus");
    copy_tree(&fixture_dir(), &project_dir);

    let path = project_dir.join("project.json");
    let text = std::fs::read_to_string(&path).unwrap();
    let mut value: serde_json::Value = serde_json::from_str(&text).unwrap();

    let mut removed = false;
    if let Some(puppets) = value.get_mut("puppets").and_then(|p| p.as_object_mut()) {
        for (_, puppet) in puppets.iter_mut() {
            if let Some(kind) = puppet.get_mut("kind").and_then(|k| k.as_object_mut())
                && kind.remove(key).is_some()
            {
                removed = true;
            }
        }
    }
    assert!(
        removed,
        "the fixture no longer contains `{key}`, so this test is not testing anything"
    );

    std::fs::write(&path, serde_json::to_string_pretty(&value).unwrap()).unwrap();
    (dir, project_dir)
}

#[test]
fn a_project_written_before_matte_existed_still_loads() {
    let (_guard, project_dir) = fixture_without("matte");
    let project = load(&project_dir).expect("a v1 file without `matte` loads");

    let puppet = project.puppets.values().next().expect("one puppet");
    match &puppet.kind {
        PuppetKind::Mesh(m) => assert_eq!(
            m.matte.mode,
            MatteMode::UseImageAlpha,
            "a missing matte must default to the image's own alpha"
        ),
        _ => panic!("the fixture puppet is a mesh puppet"),
    }
}
