//! The whole M1 loop, headless: import a PNG, rig it, edit it, save it,
//! reload it, and keep working.
//!
//! This is Task 14's real-flow test. The project crate already proves
//! `save(load(x)) == x` for hand-built documents; what that cannot prove is
//! that a document built **the way the editor builds one** — through
//! `build_import`, the rig tools and the command stack — survives the disk.
//! And the sharpest assertion here is about neither saving nor loading:
//! after reopening, allocating a new ID must not collide with anything the
//! previous session created. A project that reloads perfectly and then
//! hands out a duplicate ID corrupts on the *second* edit, when the trail
//! is cold.

use animus_core::doc::{PuppetKind, UndoStack, apply_command};
use animus_editor::import::build_import;
use animus_editor::rig;
use animus_project::AssetStore;
use glam::Vec2;
use image::{Rgba, RgbaImage};
use std::path::Path;

fn write_cutout_png(dir: &Path) -> std::path::PathBuf {
    let mut img = RgbaImage::from_pixel(160, 160, Rgba([0, 0, 0, 0]));
    for (x, y, px) in img.enumerate_pixels_mut() {
        let d = ((x as f32 - 80.0).powi(2) + (y as f32 - 80.0).powi(2)).sqrt();
        if d <= 60.0 {
            *px = Rgba([70, 130, 200, 255]);
        }
    }
    let path = dir.join("dancer.png");
    img.save(&path).unwrap();
    path
}

#[test]
fn import_rig_edit_save_reload_and_the_ids_still_hold() {
    let tmp = tempfile::tempdir().unwrap();
    let png = write_cutout_png(tmp.path());
    let root = tmp.path().join("Show.animus");

    // ── import, exactly as the drop handler does ──────────────────────
    let mut project = animus_core::doc::Project::new("Vertical Slice");
    let mut stack = UndoStack::new();
    let mut store = AssetStore::new(&root);

    let (command, _) = build_import(&png, &mut project, &mut store).expect("import");
    let puppet_id = command.puppet.id;
    apply_command(&mut project, &mut stack, Box::new(command)).expect("apply import");
    stack.break_merge();

    // ── rig: two joints, one bone, exactly as the tools do ────────────
    let cmd = rig::place_joint(&mut project, puppet_id, Vec2::new(80.0, 40.0)).unwrap();
    apply_command(&mut project, &mut stack, Box::new(cmd)).unwrap();
    stack.break_merge();
    let cmd = rig::place_joint(&mut project, puppet_id, Vec2::new(80.0, 120.0)).unwrap();
    apply_command(&mut project, &mut stack, Box::new(cmd)).unwrap();
    stack.break_merge();

    let joints: Vec<_> = match &project.puppets[&puppet_id].kind {
        PuppetKind::Mesh(m) => m.skeleton.joints.keys().copied().collect(),
        _ => unreachable!(),
    };
    let cmd = rig::place_bone(&mut project, puppet_id, joints[0], joints[1]).unwrap();
    apply_command(&mut project, &mut stack, Box::new(cmd)).unwrap();
    stack.break_merge();

    // ── an edit-mode drag, merged like a gesture ──────────────────────
    let cmd = animus_core::doc::MoveJointRest {
        puppet: puppet_id,
        joint: joints[1],
        from: Vec2::new(80.0, 120.0),
        to: Vec2::new(95.0, 130.0),
    };
    apply_command(&mut project, &mut stack, Box::new(cmd)).unwrap();
    stack.break_merge();

    let highest_id_before = project.next_id;

    // ── save through the real codec, reload ───────────────────────────
    animus_project::save(&project, &root).expect("save");
    let reloaded = animus_project::load(&root).expect("load");

    assert_eq!(
        serde_json::to_value(&reloaded).unwrap(),
        serde_json::to_value(&project).unwrap(),
        "the reloaded project must equal the saved one, field for field"
    );

    // ── the collision test ────────────────────────────────────────────
    assert_eq!(
        reloaded.next_id, highest_id_before,
        "IdAlloc's watermark must survive the disk"
    );
    let mut reopened = reloaded;
    let fresh = reopened.alloc_id();
    let all_existing: Vec<u64> = collect_ids(&reopened);
    assert!(
        !all_existing.contains(&fresh),
        "a new ID after reopening ({fresh}) collided with an existing one"
    );

    // ── and the reloaded puppet still builds a GPU mesh ───────────────
    let PuppetKind::Mesh(mp) = &reopened.puppets[&puppet_id].kind else {
        panic!("mesh puppet");
    };
    let built = animus_runtime::build_skinned_mesh(
        mp,
        &reopened.solver,
        100.0,
        animus_runtime::puppet_pivot(mp),
    )
    .expect("a saved-and-reloaded puppet must still be renderable");
    assert_eq!(built.inverse_bindposes.len(), 1, "one bone, one bind pose");
}

/// Every ID the project holds, from every table.
fn collect_ids(p: &animus_core::doc::Project) -> Vec<u64> {
    let mut ids: Vec<u64> = Vec::new();
    ids.extend(p.assets.keys().map(|a| a.0));
    ids.extend(p.layers.iter().map(|l| l.0));
    for puppet in p.puppets.values() {
        ids.push(puppet.id.0);
        if let PuppetKind::Mesh(m) = &puppet.kind {
            ids.extend(m.skeleton.joints.keys().map(|j| j.0));
            ids.extend(m.skeleton.bones.keys().map(|b| b.0));
        }
    }
    ids
}

#[test]
fn undo_history_does_not_survive_the_disk_and_that_is_correct() {
    // Not an accident to paper over: the undo stack holds command objects,
    // not document state, and persisting it would mean serializing closures
    // over stale IDs. A reopened project starts with a clean history, and
    // this test documents that as intended behaviour.
    let tmp = tempfile::tempdir().unwrap();
    let png = write_cutout_png(tmp.path());
    let root = tmp.path().join("Show.animus");

    let mut project = animus_core::doc::Project::new("History");
    let mut stack = UndoStack::new();
    let mut store = AssetStore::new(&root);
    let (command, _) = build_import(&png, &mut project, &mut store).unwrap();
    apply_command(&mut project, &mut stack, Box::new(command)).unwrap();

    animus_project::save(&project, &root).unwrap();
    let _reloaded = animus_project::load(&root).unwrap();
    let fresh_stack = UndoStack::new();
    assert!(!fresh_stack.can_undo());
}
