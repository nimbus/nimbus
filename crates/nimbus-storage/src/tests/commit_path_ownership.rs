//! Source-scanning gate. Deliberately imports nothing from the test prelude:
//! the gate reads provider source as text, so pulling storage types in would
//! only couple it to a surface it must not depend on.

/// The two write-transaction commit-sequence fault points, named without a path
/// prefix so the scan catches `FaultPoint::X`, `crate::FaultPoint::X`, and a
/// directly imported `X` alike.
const COMMIT_SEQUENCE_FAULT_POINTS: [&str; 2] = [
    "StorageCommitBeforeVisibility",
    "StorageCommitAfterVisibilityBeforeReturn",
];

/// U4: the SQL write-transaction commit sequence is owned by `sql/write_core.rs`,
/// and its fault points may be checked nowhere else in a provider.
///
/// PostgreSQL, MySQL and the libsql replica share one commit sequence —
/// apply, `StorageCommitBeforeVisibility`, commit, schema-cache invalidation,
/// `StorageCommitAfterVisibilityBeforeReturn`, `after_visibility`. A provider
/// that checks either point from its own module has forked that sequence, which
/// is exactly how the three dialects drifted before they were unified: the
/// ambiguous-outcome and acknowledgement-loss semantics diverged silently
/// because each dialect placed its own checks.
///
/// Scope note — this gate covers `StorageCommit*` and deliberately does not
/// cover `Journal*`. Journal fault-point placement is a genuine dialect axis
/// rather than forked shared logic: the libsql replica's journal append and its
/// flush to the primary are one statement batch observed on the write
/// transaction, while PostgreSQL and MySQL observe theirs inside their own
/// write pipelines, whose accounting boundaries `SqlDurableJournalTransaction::
/// append_and_apply_fenced_durable_batch` documents as intentionally not
/// unified. Gating `Journal*` here would require allowlisting every provider
/// file that legitimately holds one, which enforces nothing. If journal batching
/// is ever unified into the shared core, extend this gate to match.
#[test]
fn u4_commit_sequence_fault_points_live_only_in_the_shared_sql_core() {
    let src_dir = std::env::var_os("CARGO_MANIFEST_DIR")
        .map(std::path::PathBuf::from)
        .expect("CARGO_MANIFEST_DIR should be set by Cargo/nextest for nimbus-storage tests")
        .join("src");
    let core_path = src_dir.join("sql").join("write_core.rs");

    let mut violations = Vec::new();
    let mut scanned = 0usize;
    // Recursive walk: a future provider subdirectory must not silently escape
    // the scan while the scanned-count floor stays green.
    let mut pending: Vec<std::path::PathBuf> = ["postgres", "mysql", "libsql"]
        .iter()
        .map(|provider| src_dir.join(provider))
        .collect();
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(&dir).expect("provider src dir must be readable") {
            let path = entry.expect("dir entry").path();
            if path.is_dir() {
                pending.push(path);
                continue;
            }
            if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
                continue;
            }
            scanned += 1;
            let contents = std::fs::read_to_string(&path).expect("source file must be readable");
            let display = path
                .strip_prefix(&src_dir)
                .unwrap_or(&path)
                .display()
                .to_string();
            for needle in COMMIT_SEQUENCE_FAULT_POINTS {
                if contents.contains(needle) {
                    violations.push(format!("{display} checks {needle}"));
                }
            }
        }
    }

    // Guard the gate itself against vacuousness: 50 provider modules are
    // scanned today, with a five-file margin for module consolidation.
    assert!(
        scanned >= 45,
        "commit-path ownership gate scanned only {scanned} provider files; scan set is broken"
    );
    let core_src = std::fs::read_to_string(&core_path)
        .expect("sql/write_core.rs must exist — it owns the shared commit sequence");
    for needle in COMMIT_SEQUENCE_FAULT_POINTS {
        assert!(
            core_src.contains(needle),
            "gate needle {needle} no longer appears in sql/write_core.rs; the commit sequence \
             moved and this gate must be updated to follow it"
        );
    }

    assert!(
        violations.is_empty(),
        "U4 violation — a provider checks a write-transaction commit-sequence fault point \
         outside the shared core. These points belong to sql_commit in \
         crates/nimbus-storage/src/sql/write_core.rs; move the check there so all three \
         dialects observe it identically: {violations:?}"
    );
}
