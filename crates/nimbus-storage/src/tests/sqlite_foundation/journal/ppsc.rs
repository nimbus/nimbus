use super::*;

#[test]
fn sqlite_ppsc_identical_replay_is_idempotent_for_all_write_shapes() {
    let dir = tempdir().expect("temporary directory should create");
    let store = SqliteTenantStore::open(dir.path().join("tenant.sqlite3"))
        .expect("sqlite tenant store should open");
    exercise_ppsc_identical_applied_sequence_replay(&store, "sqlite_duplicate_replay");
}

#[test]
fn sqlite_ppsc_different_content_sequence_reuse_is_rejected_for_all_write_shapes() {
    let dir = tempdir().expect("temporary directory should create");
    let store = SqliteTenantStore::open(dir.path().join("tenant.sqlite3"))
        .expect("sqlite tenant store should open");
    exercise_ppsc_different_content_applied_sequence_reuse_rejection(
        &store,
        "sqlite_duplicate_corruption",
    );
}
