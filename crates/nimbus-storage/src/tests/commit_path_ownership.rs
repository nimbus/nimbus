//! Source-scanning gates over provider commit-path fault points. Deliberately
//! imports nothing from the test prelude: these gates read provider source as
//! text, so pulling storage types in would only couple them to a surface they
//! must not depend on.
//!
//! Two gates, because the two fault-point families have opposite ownership
//! stories. `StorageCommit*` is shared logic, so it is **banned** everywhere
//! outside the core. `Journal*` placement is a genuine dialect axis, so it is
//! **pinned** to its known owners by exact count instead.
//!
//! SIC3 adds a third gate in the child modules: the writer ownership matrix
//! and the source checks that keep it honest.

mod effect_gate;
mod effect_matrix;

/// The two write-transaction commit-sequence fault points, named without a path
/// prefix so the scan catches `FaultPoint::X`, `crate::FaultPoint::X`, and a
/// directly imported `X` alike.
const COMMIT_SEQUENCE_FAULT_POINTS: [&str; 2] = [
    "StorageCommitBeforeVisibility",
    "StorageCommitAfterVisibilityBeforeReturn",
];

/// Every journal fault point, named the same prefix-free way.
const JOURNAL_FAULT_POINTS: [&str; 3] = [
    "JournalAppendBeforeDurableFlush",
    "JournalFlushBeforeVisibility",
    "JournalDurableAppendBeforeApply",
];

/// Journal fault-point owners inside the scanned provider directories, with the
/// exact number of `JOURNAL_FAULT_POINTS` occurrences each holds.
///
/// Paths are relative to `crates/nimbus-storage/src` and use `/` separators;
/// the scan normalizes platform separators before matching.
const JOURNAL_OWNERS: [(&str, usize); 3] = [
    ("postgres/write_pipeline.rs", 4),
    ("mysql/write_pipeline.rs", 2),
    ("libsql/write.rs", 2),
];

/// One scanned provider source file: path relative to `src`, `/`-separated.
struct ProviderSource {
    display: String,
    contents: String,
}

/// Recursively read every `.rs` file under `src/{postgres,mysql,libsql}/`.
///
/// The walk is recursive so a future provider subdirectory cannot silently
/// escape the scan while a file-count floor stays green. `libsql.rs` at the
/// module root is outside these directories and therefore outside both gates:
/// its fault points sit on the replica's remote durable-batch round-trips,
/// which are a different surface from the write transaction the gates own.
fn scan_provider_sources() -> Vec<ProviderSource> {
    let src_dir = std::env::var_os("CARGO_MANIFEST_DIR")
        .map(std::path::PathBuf::from)
        .expect("CARGO_MANIFEST_DIR should be set by Cargo/nextest for nimbus-storage tests")
        .join("src");

    let mut sources = Vec::new();
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
            let contents = std::fs::read_to_string(&path).expect("source file must be readable");
            let display = path
                .strip_prefix(&src_dir)
                .unwrap_or(&path)
                .components()
                .map(|component| component.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            sources.push(ProviderSource { display, contents });
        }
    }
    sources
}

fn read_shared_core() -> String {
    let core_path = std::env::var_os("CARGO_MANIFEST_DIR")
        .map(std::path::PathBuf::from)
        .expect("CARGO_MANIFEST_DIR should be set by Cargo/nextest for nimbus-storage tests")
        .join("src")
        .join("sql")
        .join("write_core.rs");
    std::fs::read_to_string(&core_path)
        .expect("sql/write_core.rs must exist — it owns the shared commit sequence")
}

/// Guard both gates against vacuousness: 50 provider modules are scanned today,
/// with a five-file margin for module consolidation.
fn assert_scan_set_is_intact(sources: &[ProviderSource]) {
    let scanned = sources.len();
    assert!(
        scanned >= 45,
        "commit-path ownership gate scanned only {scanned} provider files; scan set is broken"
    );
}

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
#[test]
fn u4_commit_sequence_fault_points_live_only_in_the_shared_sql_core() {
    let sources = scan_provider_sources();
    assert_scan_set_is_intact(&sources);

    let mut violations = Vec::new();
    for source in &sources {
        for needle in COMMIT_SEQUENCE_FAULT_POINTS {
            if source.contents.contains(needle) {
                violations.push(format!("{} checks {needle}", source.display));
            }
        }
    }

    let core_src = read_shared_core();
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

/// U4, journal half: journal fault points stay with their pinned owners.
///
/// Journal placement is deliberately *not* unified. The libsql replica's
/// journal append and its flush to the primary are one statement batch observed
/// on the write transaction, while PostgreSQL and MySQL observe theirs inside
/// their own write pipelines, whose accounting boundaries
/// `SqlDurableJournalTransaction::append_and_apply_fenced_durable_batch`
/// documents as intentionally not unified. Banning these points outside a
/// shared core would therefore be wrong — there is no shared core to move them
/// to.
///
/// So this gate pins ownership instead of forbidding it. Each owner declares an
/// exact occurrence count. A **new** file that starts checking a journal fault
/// point fails, and so does a **count drift** in a pinned file. Neither is
/// presumed to be a defect: the gate's job is to make journal fault-point
/// placement impossible to change silently, so an intentional change updates
/// `JOURNAL_OWNERS` in the same commit and a reviewer sees it.
///
/// `sql/write_core.rs` holds no journal fault point and is intentionally absent
/// from the pin list — the shared commit sequence does not observe the journal.
/// If journal batching is ever unified into the shared core, this gate becomes
/// a ban like its sibling above.
#[test]
fn u4_journal_fault_points_stay_with_their_pinned_owners() {
    let sources = scan_provider_sources();
    assert_scan_set_is_intact(&sources);

    let mut drifted = Vec::new();
    let mut unpinned = Vec::new();
    let mut seen = Vec::new();

    for source in &sources {
        let count: usize = JOURNAL_FAULT_POINTS
            .iter()
            .map(|needle| source.contents.matches(needle).count())
            .sum();
        let pin = JOURNAL_OWNERS
            .iter()
            .find(|(owner, _)| *owner == source.display);
        match (pin, count) {
            (Some((owner, expected)), found) => {
                seen.push(*owner);
                if found != *expected {
                    drifted.push(format!(
                        "{owner} holds {found} journal fault-point token(s), pinned at {expected}"
                    ));
                }
            }
            (None, 0) => {}
            (None, found) => unpinned.push(format!(
                "{} holds {found} journal fault-point token(s) but is not a pinned owner",
                source.display
            )),
        }
    }

    let missing: Vec<&str> = JOURNAL_OWNERS
        .iter()
        .map(|(owner, _)| *owner)
        .filter(|owner| !seen.contains(owner))
        .collect();
    assert!(
        missing.is_empty(),
        "pinned journal fault-point owner(s) {missing:?} were not found by the scan; the file \
         moved or was renamed, so update JOURNAL_OWNERS in \
         crates/nimbus-storage/src/tests/commit_path_ownership.rs"
    );

    assert!(
        unpinned.is_empty() && drifted.is_empty(),
        "journal fault-point ownership changed. Owners are {JOURNAL_OWNERS:?} \
         (paths relative to crates/nimbus-storage/src). Journal placement follows each dialect's \
         physical journal write and is not shared, so this is a pin, not a ban: if the change is \
         intentional, update JOURNAL_OWNERS in \
         crates/nimbus-storage/src/tests/commit_path_ownership.rs in the same commit. \
         Unpinned: {unpinned:?}. Drifted: {drifted:?}"
    );
}
