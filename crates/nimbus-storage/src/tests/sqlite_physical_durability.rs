//! Physical durability faults against the real SQLite store.
//!
//! The logical `FaultInjector` seam proves what the store does when a *logical*
//! step refuses. These cases prove what it does when the *device* refuses: a
//! write that cannot land, an `fsync` that cannot confirm, a write-ahead log
//! that cannot take a frame, and a process that stops existing between the
//! commit and the reply.
//!
//! Every case follows the same shape. Reach a state the store acknowledged,
//! record that acknowledgement, arm one physical failure, drive one more write,
//! then reopen the database from disk and hold the reopened store against the
//! acknowledgement. The rule is in [`check_acknowledgement_survives`], and
//! [`physical_durability_checker_detects_a_broken_acknowledgement_rule`] proves
//! that rule is not vacuous.
//!
//! The fault machinery is test-only. It lives in [`fault_vfs`], is compiled
//! under `cfg(test)`, and reaches SQLite through a pass-through VFS rather than
//! through any production switch. No production module gains a way to fail a
//! physical operation.

use super::*;
use crate::store::JournalProgress;
use crate::{MaterializedPosition, ResolvedWrite};
use fault_vfs::{PhysicalFault, arm, fault_fired, install};

mod fault_vfs;

/// Environment variable naming the database the crash child must write.
const CRASH_DB_ENV: &str = "NIMBUS_SIC6_CRASH_DB";

/// How long the crash test waits for its child to report before failing.
const CRASH_CHILD_REPORT_TIMEOUT: Duration = BLOCKING_TEST_RELEASE_TIMEOUT;

/// The libtest name of the child case the crash test re-executes.
///
/// Derived from `module_path!` rather than written out, so renaming or moving
/// this module cannot leave the parent spawning a case that no longer exists.
fn crash_child_test_name() -> String {
    let module = module_path!();
    let without_crate = module.split_once("::").map_or(module, |(_, rest)| rest);
    format!("{without_crate}::crash_child_writes_and_parks")
}

/// The last result the store handed back to a caller.
///
/// Durable head, applied head, and materialized position together: a bare
/// sequence cannot tell a recovered database from one whose content drifted at
/// the same sequence.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct Acknowledgement {
    durable_head: SequenceNumber,
    applied_head: SequenceNumber,
    position: MaterializedPosition,
}

fn observe(store: &SqliteTenantStore) -> Acknowledgement {
    let JournalProgress {
        durable_head,
        applied_head,
    } = store
        .journal_progress()
        .expect("journal progress should read");
    let position = store
        .export_materialized_journal_snapshot()
        .expect("materialized snapshot should export")
        .materialized_position()
        .expect("materialized position should compute");
    Acknowledgement {
        durable_head,
        applied_head,
        position,
    }
}

/// The rule a physical fault must never break.
///
/// A fault may lose an *unacknowledged* write; that is the whole point of not
/// acknowledging it. It may never lose an acknowledged one, never leave reads
/// ahead of durability, and never leave a different state sitting at the same
/// applied sequence.
fn check_acknowledgement_survives(
    reopened: &Acknowledgement,
    acknowledged: &Acknowledgement,
) -> std::result::Result<(), String> {
    if reopened.durable_head.0 < acknowledged.durable_head.0 {
        return Err(format!(
            "durable head fell from acknowledged {} to {} after reopening",
            acknowledged.durable_head.0, reopened.durable_head.0
        ));
    }
    if reopened.applied_head.0 < acknowledged.applied_head.0 {
        return Err(format!(
            "applied head fell from acknowledged {} to {} after reopening",
            acknowledged.applied_head.0, reopened.applied_head.0
        ));
    }
    if reopened.applied_head.0 > reopened.durable_head.0 {
        return Err(format!(
            "reopened store applies sequence {} past durable head {}",
            reopened.applied_head.0, reopened.durable_head.0
        ));
    }
    if reopened.applied_head == acknowledged.applied_head
        && reopened.position != acknowledged.position
    {
        return Err(format!(
            "state digest diverged at unchanged applied sequence {}: acknowledged {}, reopened {}",
            acknowledged.applied_head.0,
            acknowledged.position.state_digest(),
            reopened.position.state_digest()
        ));
    }
    Ok(())
}

/// Opens the store with the fault shim already installed.
///
/// A SQLite connection binds its VFS at open time, so installing the shim
/// later would leave this connection on the untouched default and no fault
/// could ever reach it.
fn open_through_shim(path: &std::path::Path) -> SqliteTenantStore {
    install();
    SqliteTenantStore::open(path).expect("sqlite store should open")
}

/// Seeds `count` acknowledged inserts and returns what the store reported.
fn seed_acknowledged(store: &SqliteTenantStore, table: &str, count: usize) -> Acknowledgement {
    for index in 0..count {
        store
            .insert(&sample_document(table, &format!("seeded-{index}")))
            .expect("seeded insert should be acknowledged");
    }
    observe(store)
}

/// Reopens the database on disk and reports where it landed.
fn reopen(path: &std::path::Path) -> Acknowledgement {
    let store = open_through_shim(path);
    let observed = observe(&store);
    drop(store);
    observed
}

#[test]
#[serial_test::serial(sqlite_physical_faults)]
fn sqlite_disk_full_preserves_last_acknowledged_position() {
    let directory = tempdir().expect("temporary directory should create");
    let path = directory.path().join("sic6-disk-full.sqlite3");
    let store = open_through_shim(&path);
    let acknowledged = seed_acknowledged(&store, "tasks", 3);

    let guard = arm("sic6-disk-full", PhysicalFault::DiskFull, 0);
    let rejected = store.insert(&sample_document("tasks", "beyond-the-device"));
    let fired = fault_fired();
    drop(guard);
    drop(store);

    assert!(
        rejected.is_err(),
        "a write the device could not take must never be acknowledged"
    );
    assert!(fired, "the disk-full fault should have reached a write");

    let reopened = reopen(&path);
    check_acknowledgement_survives(&reopened, &acknowledged)
        .expect("disk-full must not disturb the acknowledged position");
    assert_eq!(
        reopened, acknowledged,
        "a write rejected before it reached durable storage must leave no trace"
    );
}

#[test]
#[serial_test::serial(sqlite_physical_faults)]
fn sqlite_sync_failure_is_not_acknowledged() {
    let directory = tempdir().expect("temporary directory should create");
    let path = directory.path().join("sic6-sync-failure.sqlite3");
    let store = open_through_shim(&path);
    let acknowledged = seed_acknowledged(&store, "tasks", 3);

    let guard = arm("sic6-sync-failure", PhysicalFault::SyncFailure, 0);
    let rejected = store.insert(&sample_document("tasks", "unconfirmed"));
    let fired = fault_fired();
    drop(guard);
    drop(store);

    assert!(
        rejected.is_err(),
        "a commit whose fsync failed is not durable knowledge, so it must not be acknowledged"
    );
    assert!(fired, "the sync fault should have reached an fsync");

    let reopened = reopen(&path);
    check_acknowledgement_survives(&reopened, &acknowledged)
        .expect("a failed fsync must not disturb the acknowledged position");
    // The bytes may or may not have reached the platter before `fsync` failed.
    // Either outcome is sound because the caller was told the write failed;
    // what the store may not do is land somewhere further ahead than the one
    // unacknowledged commit, or come back short of the acknowledged head.
    assert!(
        reopened.durable_head.0 == acknowledged.durable_head.0
            || reopened.durable_head.0 == acknowledged.durable_head.0 + 1,
        "reopened durable head {} is neither the acknowledged head {} nor the single \
         unacknowledged commit past it",
        reopened.durable_head.0,
        acknowledged.durable_head.0
    );
    assert_eq!(
        reopened.applied_head, reopened.durable_head,
        "recovery must apply exactly what it found durable"
    );
}

#[test]
#[serial_test::serial(sqlite_physical_faults)]
fn sqlite_wal_failure_never_exposes_partial_effects() {
    let directory = tempdir().expect("temporary directory should create");
    let path = directory.path().join("sic6-wal-failure.sqlite3");
    let store = open_through_shim(&path);
    let acknowledged = seed_acknowledged(&store, "tasks", 3);

    let batch: Vec<ResolvedWrite> = (0..4)
        .map(|index| ResolvedWrite::Insert {
            document: sample_document("tasks", &format!("batched-{index}")),
            indexes: Vec::new(),
            resource_path_binding: None,
        })
        .collect();

    let guard = arm("sic6-wal-failure", PhysicalFault::WalWriteFailure, 0);
    let rejected = store.apply_resolved_write_batch(&batch);
    let fired = fault_fired();
    drop(guard);
    drop(store);

    assert!(
        rejected.is_err(),
        "a batch whose write-ahead log refused a frame must not be acknowledged"
    );
    assert!(fired, "the WAL fault should have reached a log write");

    let reopened = reopen(&path);
    check_acknowledgement_survives(&reopened, &acknowledged)
        .expect("a failed write-ahead log write must not disturb the acknowledged position");

    let reopened_store = open_through_shim(&path);
    let survivors = batch
        .iter()
        .filter(|write| {
            let ResolvedWrite::Insert { document, .. } = write else {
                unreachable!("the batch holds inserts only")
            };
            reopened_store
                .get(&document.table, &document.id)
                .expect("get should succeed after recovery")
                .is_some()
        })
        .count();
    drop(reopened_store);

    assert!(
        survivors == 0 || survivors == batch.len(),
        "the batch was torn: {survivors} of {} documents are visible, so a failed \
         write-ahead log write exposed part of a transaction",
        batch.len()
    );
}

#[test]
#[serial_test::serial(sqlite_physical_faults)]
fn sqlite_crash_after_durable_commit_recovers_matching_position() {
    let directory = tempdir().expect("temporary directory should create");
    let path = directory.path().join("sic6-crash.sqlite3");

    let mut child = std::process::Command::new(
        std::env::current_exe().expect("the test binary should have a path"),
    )
    .args([
        "--exact",
        &crash_child_test_name(),
        "--ignored",
        "--nocapture",
    ])
    .env(CRASH_DB_ENV, &path)
    .stdout(std::process::Stdio::piped())
    .spawn()
    .expect("the crash child should spawn");

    // Bounded: a child that never reports must fail this test, not hang it.
    let stdout = child.stdout.take().expect("child stdout should be piped");
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        use std::io::BufRead;
        let mut reader = std::io::BufReader::new(stdout);
        let mut line = String::new();
        while reader.read_line(&mut line).unwrap_or(0) > 0 {
            if let Some(payload) = line.trim().strip_prefix("ACK ") {
                let _ = sender.send(payload.to_string());
                return;
            }
            line.clear();
        }
    });
    let reported = receiver.recv_timeout(CRASH_CHILD_REPORT_TIMEOUT);

    // SIGKILL: no unwinding, no `Drop`, no close, no checkpoint. The database
    // on disk is exactly what the operating system already held.
    child.kill().expect("the crash child should be killable");
    child.wait().expect("the crash child should reap");

    let acknowledged: Acknowledgement = serde_json::from_str(
        &reported.expect("the child should acknowledge its commits before the bounded wait ends"),
    )
    .expect("the child should report its acknowledgement as an Acknowledgement");
    let reopened = reopen(&path);
    check_acknowledgement_survives(&reopened, &acknowledged)
        .expect("process loss must not lose an acknowledged commit");
    assert_eq!(
        reopened, acknowledged,
        "recovery after process loss must land on the acknowledged position exactly"
    );
}

/// Runs inside the child process the crash test spawns, and never returns.
#[test]
#[ignore = "spawned by sqlite_crash_after_durable_commit_recovers_matching_position"]
fn crash_child_writes_and_parks() {
    let Some(path) = std::env::var_os(CRASH_DB_ENV) else {
        return;
    };
    let store = open_through_shim(&std::path::PathBuf::from(path));
    let acknowledged = seed_acknowledged(&store, "tasks", 5);
    println!(
        "ACK {}",
        serde_json::to_string(&acknowledged).expect("acknowledgement should serialize")
    );
    use std::io::Write;
    std::io::stdout()
        .flush()
        .expect("child stdout should flush");
    // Hold the connection open with its commits acknowledged and wait to be
    // killed. The parent's SIGKILL is the crash.
    loop {
        std::thread::park();
    }
}

#[test]
fn physical_durability_checker_detects_a_broken_acknowledgement_rule() {
    let directory = tempdir().expect("temporary directory should create");

    let honest_path = directory.path().join("honest.sqlite3");
    let honest = SqliteTenantStore::open(&honest_path).expect("sqlite store should open");
    let acknowledged = seed_acknowledged(&honest, "tasks", 3);
    drop(honest);
    check_acknowledgement_survives(&reopen(&honest_path), &acknowledged)
        .expect("an undisturbed database must satisfy the rule");

    // Mutation one: the acknowledged commits are gone. A checker that only
    // compared sequence shapes, or that trusted the store's own report, would
    // let this through.
    let empty_path = directory.path().join("lost.sqlite3");
    let empty = SqliteTenantStore::open(&empty_path).expect("sqlite store should open");
    let lost = observe(&empty);
    drop(empty);
    let error = check_acknowledgement_survives(&lost, &acknowledged)
        .expect_err("losing acknowledged commits must fail the rule");
    assert!(
        error.contains("durable head fell"),
        "the checker must name the lost durable head, said: {error}"
    );

    // Mutation two: the same applied sequence carries different content. This
    // is the failure a bare sequence comparison cannot see, which is why the
    // acknowledgement binds a materialized position.
    let diverged_path = directory.path().join("diverged.sqlite3");
    let diverged = SqliteTenantStore::open(&diverged_path).expect("sqlite store should open");
    for index in 0..3 {
        diverged
            .insert(&sample_document("tasks", &format!("divergent-{index}")))
            .expect("insert should be acknowledged");
    }
    let same_sequence_other_state = observe(&diverged);
    drop(diverged);
    assert_eq!(
        same_sequence_other_state.applied_head, acknowledged.applied_head,
        "the mutation must hold the applied sequence fixed to be meaningful"
    );
    let error = check_acknowledgement_survives(&same_sequence_other_state, &acknowledged)
        .expect_err("different state at the same applied sequence must fail the rule");
    assert!(
        error.contains("state digest diverged"),
        "the checker must name the digest divergence, said: {error}"
    );

    // Mutation three: reads run ahead of durability.
    let ahead = Acknowledgement {
        durable_head: acknowledged.durable_head,
        applied_head: SequenceNumber(acknowledged.durable_head.0 + 1),
        position: acknowledged.position.clone(),
    };
    let error = check_acknowledgement_survives(&ahead, &acknowledged)
        .expect_err("applying past the durable head must fail the rule");
    assert!(
        error.contains("past durable head"),
        "the checker must name the durability inversion, said: {error}"
    );
}
