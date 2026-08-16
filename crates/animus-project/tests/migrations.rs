use animus_core::doc::CURRENT_SCHEMA_VERSION;
use animus_core::migrate::{MIGRATIONS, MigrateError, run};
use serde_json::json;

#[test]
fn the_chain_has_one_step_per_version_gap() {
    assert_eq!(
        MIGRATIONS.len() as u32,
        CURRENT_SCHEMA_VERSION - 1,
        "every schema bump needs exactly one migration"
    );
}

#[test]
fn migrating_from_the_current_version_is_a_no_op() {
    let mut v = json!({ "schema_version": CURRENT_SCHEMA_VERSION, "x": 1 });
    let before = v.clone();
    run(&mut v, CURRENT_SCHEMA_VERSION).unwrap();
    assert_eq!(v, before);
}

#[test]
fn migrating_from_a_future_version_is_an_error() {
    let mut v = json!({ "schema_version": 99 });
    assert!(matches!(
        run(&mut v, 99),
        Err(MigrateError::FromTheFuture { .. })
    ));
}

#[test]
fn migrating_from_schema_version_zero_is_an_error_not_a_panic() {
    // 0 is never a valid schema_version (versions start at 1); a
    // hand-edited or truncated file claiming it must be rejected
    // cleanly, not crash `MIGRATIONS[(v - 1) as usize]`'s `v - 1`.
    let mut v = json!({ "schema_version": 0 });
    assert!(matches!(
        run(&mut v, 0),
        Err(MigrateError::InvalidVersion { found: 0 })
    ));
}

#[test]
fn every_committed_fixture_migrates_to_the_current_version_and_loads() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../spec/fixtures");
    let mut checked = 0;
    for entry in std::fs::read_dir(root).unwrap().flatten() {
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        let p = animus_project::load(&dir)
            .unwrap_or_else(|e| panic!("fixture {dir:?} failed to load: {e:?}"));
        assert_eq!(p.schema_version, CURRENT_SCHEMA_VERSION);
        checked += 1;
    }
    assert!(
        checked > 0,
        "no fixtures found — the migration guard is not guarding anything"
    );
}
