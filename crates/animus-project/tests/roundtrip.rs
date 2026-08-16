use animus_core::doc::*;
use animus_core::ids::{AssetId, LayerId};
use animus_project::{AssetStore, ProjectError, load, save, to_json};
use std::fs;
use tempfile::tempdir;

fn sample() -> Project {
    let mut p = Project::new("Sample Show");
    let lid = LayerId(p.alloc_id());
    p.layers.push(lid);
    p.layer_data.insert(lid, Layer::new(lid, "Background"));
    p
}

#[test]
fn save_then_load_reproduces_the_document() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("MyShow.animus");
    let p = sample();
    save(&p, &root).unwrap();
    let back = load(&root).unwrap();
    assert_eq!(to_json(&back).unwrap(), to_json(&p).unwrap());
}

#[test]
fn saving_twice_produces_byte_identical_json() {
    // Key ordering must be stable, or every save churns the git diff.
    let p = sample();
    assert_eq!(to_json(&p).unwrap(), to_json(&p).unwrap());
}

#[test]
fn save_is_atomic_and_leaves_no_temp_file() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("MyShow.animus");
    save(&sample(), &root).unwrap();
    assert!(root.join("project.json").exists());
    assert!(!root.join("project.json.tmp").exists());
}

#[test]
fn an_existing_project_survives_a_second_save() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("MyShow.animus");
    save(&sample(), &root).unwrap();
    let mut p2 = sample();
    p2.meta.name = "Renamed".into();
    save(&p2, &root).unwrap();
    assert_eq!(load(&root).unwrap().meta.name, "Renamed");
}

#[test]
fn non_finite_floats_are_rejected_at_write_time() {
    let mut p = sample();
    p.solver.global_damping = f32::NAN;
    let err = to_json(&p).unwrap_err();
    assert!(matches!(err, ProjectError::NonFiniteFloat { .. }));
}

#[test]
fn a_newer_schema_version_is_refused_with_a_clear_error() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("MyShow.animus");
    save(&sample(), &root).unwrap();
    let path = root.join("project.json");
    let text = fs::read_to_string(&path)
        .unwrap()
        .replace("\"schema_version\": 1", "\"schema_version\": 99");
    fs::write(&path, text).unwrap();

    match load(&root) {
        Err(ProjectError::SchemaTooNew {
            found: 99,
            supported: 1,
        }) => {}
        other => panic!("expected SchemaTooNew, got {other:?}"),
    }
}

#[test]
fn truncated_json_is_an_error_not_a_panic() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("MyShow.animus");
    save(&sample(), &root).unwrap();
    fs::write(root.join("project.json"), "{ \"schema_version\": 1, \"me").unwrap();
    assert!(load(&root).is_err());
}

#[test]
fn assets_are_stored_by_content_hash() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("MyShow.animus");
    fs::create_dir_all(&root).unwrap();
    let src = dir.path().join("pic.png");
    fs::write(&src, b"not really a png, but bytes are bytes").unwrap();

    let mut project = sample();
    let mut store = AssetStore::new(&root);
    let a = store
        .import(&src, AssetKind::Image, AssetId(project.alloc_id()))
        .unwrap();
    assert_eq!(a.sha256.len(), 64);
    assert!(store.path_for(&a).exists());
    assert_eq!(a.original_name, "pic.png");
}

#[test]
fn importing_identical_bytes_twice_stores_one_file() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("MyShow.animus");
    fs::create_dir_all(&root).unwrap();
    let a_path = dir.path().join("a.png");
    let b_path = dir.path().join("b.png");
    fs::write(&a_path, b"same bytes").unwrap();
    fs::write(&b_path, b"same bytes").unwrap();

    let mut project = sample();
    let mut store = AssetStore::new(&root);
    let a = store
        .import(&a_path, AssetKind::Image, AssetId(project.alloc_id()))
        .unwrap();
    let b = store
        .import(&b_path, AssetKind::Image, AssetId(project.alloc_id()))
        .unwrap();
    assert_eq!(a.sha256, b.sha256);
    assert_eq!(store.path_for(&a), store.path_for(&b));

    let count = walkdir_count_files(&root.join("assets"));
    assert_eq!(count, 1, "identical bytes must be stored once");
}

fn walkdir_count_files(p: &std::path::Path) -> usize {
    fn rec(p: &std::path::Path, n: &mut usize) {
        if let Ok(rd) = std::fs::read_dir(p) {
            for e in rd.flatten() {
                let path = e.path();
                if path.is_dir() {
                    rec(&path, n)
                } else {
                    *n += 1
                }
            }
        }
    }
    let mut n = 0;
    rec(p, &mut n);
    n
}
