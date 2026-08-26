use super::*;
use crate::{PointInTimeRestoreTarget, RetentionGcConfig};
use nimbus_core::{DocumentLocator, DocumentPath, ResourcePathBinding};

#[derive(Default)]
struct BlockingRetentionCommit {
    state: Mutex<(bool, bool)>,
    changed: Condvar,
}

impl BlockingRetentionCommit {
    fn wait_until_entered(&self) {
        let state = self.state.lock().expect("blocking fault lock should hold");
        let (state, _) = self
            .changed
            .wait_timeout_while(state, BLOCKING_TEST_RELEASE_TIMEOUT, |state| !state.0)
            .expect("blocking fault wait should hold");
        assert!(
            state.0,
            "retention compaction did not reach its pre-commit boundary within {BLOCKING_TEST_RELEASE_TIMEOUT:?}"
        );
    }

    fn release(&self) {
        let mut state = self.state.lock().expect("blocking fault lock should hold");
        state.1 = true;
        self.changed.notify_all();
    }
}

impl FaultInjector for BlockingRetentionCommit {
    fn check(&self, point: FaultPoint) -> nimbus_core::Result<()> {
        if point != FaultPoint::RetentionCheckpointBeforeCommit {
            return Ok(());
        }
        let mut state = self.state.lock().expect("blocking fault lock should hold");
        state.0 = true;
        self.changed.notify_all();
        let (state, _) = self
            .changed
            .wait_timeout_while(state, BLOCKING_TEST_RELEASE_TIMEOUT, |state| !state.1)
            .expect("blocking fault wait should hold");
        if !state.1 {
            return Err(Error::Internal(format!(
                "retention checkpoint test release did not arrive within {BLOCKING_TEST_RELEASE_TIMEOUT:?}"
            )));
        }
        Ok(())
    }
}

fn bounded_config() -> RetentionGcConfig {
    RetentionGcConfig::new(2).expect("bounded retention config should build")
}

fn insert_documents_redb(store: &TenantStore, count: usize) {
    for index in 0..count {
        store
            .insert(&sample_document(
                "retention_tasks",
                &format!("task-{index}"),
            ))
            .expect("redb insert should succeed");
    }
}

fn insert_documents_sqlite(store: &SqliteTenantStore, count: usize) {
    for index in 0..count {
        store
            .insert(&sample_document(
                "retention_tasks",
                &format!("task-{index}"),
            ))
            .expect("SQLite insert should succeed");
    }
}

fn insert_documents_memory(store: &MemoryTenantStore, count: usize) {
    for index in 0..count {
        store
            .insert(&sample_document(
                "retention_tasks",
                &format!("task-{index}"),
            ))
            .expect("memory insert should succeed");
    }
}

fn assert_retention_expired(error: Error) {
    assert!(matches!(
        error,
        Error::HistoricalRead {
            kind: nimbus_core::HistoricalReadErrorKind::RetentionExpired,
            ..
        }
    ));
}

fn checkpoint_binding(document: &Document) -> ResourcePathBinding {
    ResourcePathBinding::new(
        DocumentLocator::new(document.table.clone(), document.id.clone()),
        DocumentPath::from_segments(["cities", "SF", "landmarks", "checkpointed"])
            .expect("checkpoint document path should parse"),
    )
}

fn checkpoint_write(document: Document, binding: ResourcePathBinding) -> crate::ResolvedWrite {
    crate::ResolvedWrite::Insert {
        document,
        indexes: Vec::new(),
        resource_path_binding: Some(binding),
    }
}

fn assert_checkpoint_snapshot_sidecars(
    snapshot: &crate::MaterializedJournalSnapshot,
    binding: &ResourcePathBinding,
    cursor: TriggerDeliveryCursor,
) {
    assert_eq!(snapshot.resource_path_bindings, vec![binding.clone()]);
    assert_eq!(snapshot.trigger_delivery_cursor, cursor);
}

#[test]
fn embedded_retention_checkpoint_preserves_journaled_sidecars_through_restore() {
    let cursor = TriggerDeliveryCursor::new(SequenceNumber(2));

    let redb = TenantStore::create_in_memory().expect("redb tenant store should open");
    let redb_document = sample_document("retention_landmarks", "redb-landmark");
    let redb_binding = checkpoint_binding(&redb_document);
    redb.apply_execution_unit_batch(
        &[checkpoint_write(redb_document, redb_binding.clone())],
        &[],
    )
    .expect("redb execution-unit write should succeed");
    redb.set_trigger_delivery_cursor(cursor)
        .expect("redb trigger cursor should persist");
    insert_documents_redb(&redb, 3);
    redb.compact_retained_history(bounded_config())
        .expect("redb retained history should compact");
    let redb_archive = redb
        .export_point_in_time_restore_archive(
            PointInTimeRestoreTarget::Sequence(SequenceNumber(5)),
            bounded_config(),
        )
        .expect("redb retained archive should export");
    assert_eq!(
        redb_archive.base_snapshot.applied_sequence,
        SequenceNumber(3)
    );
    assert_checkpoint_snapshot_sidecars(&redb_archive.base_snapshot, &redb_binding, cursor);
    let mut tampered_archive = redb_archive.clone();
    tampered_archive.base_snapshot.trigger_delivery_cursor =
        TriggerDeliveryCursor::new(SequenceNumber(3));
    assert!(
        tampered_archive.validate().is_err(),
        "the archive checkpoint digest must bind journaled sidecar state"
    );
    let redb_restored = TenantStore::create_in_memory().expect("redb restore target should open");
    redb_restored
        .import_point_in_time_restore_archive(&redb_archive)
        .expect("redb retained archive should restore");
    assert_eq!(
        redb_restored
            .resource_path_binding(&redb_binding.locator)
            .expect("redb restored binding should read"),
        Some(redb_binding)
    );
    assert_eq!(
        redb_restored
            .trigger_delivery_cursor()
            .expect("redb restored cursor should read"),
        cursor
    );

    let dir = tempdir().expect("temporary directory should create");
    let sqlite = SqliteTenantStore::open(dir.path().join("sidecars.sqlite3"))
        .expect("SQLite tenant store should open");
    let sqlite_document = sample_document("retention_landmarks", "sqlite-landmark");
    let sqlite_binding = checkpoint_binding(&sqlite_document);
    sqlite
        .apply_execution_unit_batch(
            &[checkpoint_write(sqlite_document, sqlite_binding.clone())],
            &[],
        )
        .expect("SQLite execution-unit write should succeed");
    sqlite
        .set_trigger_delivery_cursor(cursor)
        .expect("SQLite trigger cursor should persist");
    insert_documents_sqlite(&sqlite, 3);
    sqlite
        .compact_retained_history(bounded_config())
        .expect("SQLite retained history should compact");
    let sqlite_archive = sqlite
        .export_point_in_time_restore_archive(
            PointInTimeRestoreTarget::Sequence(SequenceNumber(5)),
            bounded_config(),
        )
        .expect("SQLite retained archive should export");
    assert_checkpoint_snapshot_sidecars(&sqlite_archive.base_snapshot, &sqlite_binding, cursor);
    let sqlite_restored = SqliteTenantStore::open(dir.path().join("sidecars-restored.sqlite3"))
        .expect("SQLite restore target should open");
    sqlite_restored
        .import_point_in_time_restore_archive(&sqlite_archive)
        .expect("SQLite retained archive should restore");
    assert_eq!(
        sqlite_restored
            .resource_path_binding(&sqlite_binding.locator)
            .expect("SQLite restored binding should read"),
        Some(sqlite_binding)
    );
    assert_eq!(
        sqlite_restored
            .trigger_delivery_cursor()
            .expect("SQLite restored cursor should read"),
        cursor
    );

    let memory = MemoryTenantStore::new();
    let memory_document = sample_document("retention_landmarks", "memory-landmark");
    let memory_binding = checkpoint_binding(&memory_document);
    memory
        .apply_execution_unit_batch_with_origin(
            &[checkpoint_write(memory_document, memory_binding.clone())],
            &[],
            None,
            None,
        )
        .expect("memory execution-unit write should succeed");
    memory
        .set_trigger_delivery_cursor(cursor)
        .expect("memory trigger cursor should persist");
    insert_documents_memory(&memory, 3);
    memory
        .compact_retained_history(bounded_config())
        .expect("memory retained history should compact");
    let memory_archive = memory
        .export_point_in_time_restore_archive(
            PointInTimeRestoreTarget::Sequence(SequenceNumber(5)),
            bounded_config(),
        )
        .expect("memory retained archive should export");
    assert_checkpoint_snapshot_sidecars(&memory_archive.base_snapshot, &memory_binding, cursor);
    let memory_restored = MemoryTenantStore::new();
    memory_restored
        .import_point_in_time_restore_archive(&memory_archive)
        .expect("memory retained archive should restore");
    assert_eq!(
        memory_restored
            .resource_path_binding(&memory_binding.locator)
            .expect("memory restored binding should read"),
        Some(memory_binding)
    );
    assert_eq!(
        memory_restored
            .trigger_delivery_cursor()
            .expect("memory restored cursor should read"),
        cursor
    );
}

#[test]
fn concurrent_append_after_checkpoint_prepare_keeps_a_contiguous_retained_tail() {
    let fault = Arc::new(BlockingRetentionCommit::default());
    let store = Arc::new(
        TenantStore::create_in_memory_with_simulation(
            Arc::new(ManualWallClock::new(Timestamp(100))),
            fault.clone(),
        )
        .expect("redb tenant store should open"),
    );
    insert_documents_redb(&store, 5);

    let compacting = {
        let store = store.clone();
        std::thread::spawn(move || store.compact_retained_history(bounded_config()))
    };
    fault.wait_until_entered();
    let appending = {
        let store = store.clone();
        std::thread::spawn(move || {
            store.insert(&sample_document("retention_tasks", "concurrent-append"))
        })
    };
    fault.release();

    let summary = compacting
        .join()
        .expect("compaction thread should join")
        .expect("retained history should compact");
    let append = appending
        .join()
        .expect("append thread should join")
        .expect("concurrent append should commit");
    assert_eq!(summary.after.confirmed_floor, SequenceNumber(3));
    assert_eq!(append.sequence, SequenceNumber(6));
    assert_eq!(
        store
            .read_durable_journal_from(SequenceNumber(4))
            .expect("retained journal should read")
            .iter()
            .map(|record| record.sequence)
            .collect::<Vec<_>>(),
        vec![SequenceNumber(4), SequenceNumber(5), SequenceNumber(6)]
    );

    let archive = store
        .export_point_in_time_restore_archive(
            PointInTimeRestoreTarget::Sequence(SequenceNumber(6)),
            bounded_config(),
        )
        .expect("archive including concurrent append should export");
    let restored = TenantStore::create_in_memory().expect("restore target should open");
    restored
        .import_point_in_time_restore_archive(&archive)
        .expect("archive including concurrent append should restore");
    assert_eq!(
        restored
            .export_materialized_journal_snapshot()
            .expect("restored snapshot should export")
            .materialized_position()
            .expect("restored position should compute"),
        archive.target_position
    );
}

#[test]
fn durable_but_unapplied_record_stays_ahead_of_the_checkpoint_cut() {
    let store = TenantStore::create_in_memory().expect("redb tenant store should open");
    let document = sample_document("retention_tasks", "pending-replay");
    let record = TenantEventRecord::new(
        SequenceNumber(1),
        Timestamp(10),
        vec![WriteOp {
            table: document.table.clone(),
            table_id: TableId::new(),
            op_type: WriteOpType::Insert,
            doc_id: document.id.clone(),
            resource_path_binding: None,
            trigger_write_origin: None,
            previous: None,
            current: Some(document),
        }],
        None,
    )
    .expect("pending durable record should build");
    store
        .append_durable_records_batch(&[record])
        .expect("pending durable record should append");
    assert_eq!(
        store.journal_progress().expect("progress should read"),
        crate::JournalProgress {
            durable_head: SequenceNumber(1),
            applied_head: SequenceNumber(0),
        }
    );

    let summary = store
        .compact_retained_history(RetentionGcConfig::new(1).unwrap())
        .expect("retention should keep unapplied history");
    assert_eq!(summary.after.confirmed_floor, SequenceNumber(0));
    assert_eq!(summary.journal_records_pruned, 0);
    assert_eq!(
        store
            .read_durable_journal_from(SequenceNumber(1))
            .expect("pending durable record should remain")
            .len(),
        1
    );
}

#[test]
fn redb_retention_checkpoint_survives_restart_and_restores_from_retained_checkpoint() {
    let dir = tempdir().expect("temporary directory should create");
    let path = dir.path().join("tenant.redb");
    let store = TenantStore::open(&path).expect("redb tenant store should open");
    insert_documents_redb(&store, 5);

    let summary = store
        .compact_retained_history(bounded_config())
        .expect("redb retained history should compact");
    assert_eq!(summary.after.desired_floor, SequenceNumber(3));
    assert_eq!(summary.after.confirmed_floor, SequenceNumber(3));
    assert_eq!(summary.after.physical_floor, SequenceNumber(3));
    assert_eq!(summary.journal_records_pruned, 3);
    assert_eq!(
        store
            .read_durable_journal_from(SequenceNumber(1))
            .expect("retained redb journal should read")
            .iter()
            .map(|record| record.sequence)
            .collect::<Vec<_>>(),
        vec![SequenceNumber(4), SequenceNumber(5)]
    );
    assert_eq!(
        store
            .read_snapshot()
            .expect("redb snapshot should open")
            .durable_journal_cursor_floor()
            .expect("redb floor should read"),
        SequenceNumber(3)
    );

    let error = store
        .export_point_in_time_restore_archive(
            PointInTimeRestoreTarget::Sequence(SequenceNumber(2)),
            bounded_config(),
        )
        .expect_err("target before checkpoint must expire");
    assert_retention_expired(error);
    let archive = store
        .export_point_in_time_restore_archive(
            PointInTimeRestoreTarget::Sequence(SequenceNumber(5)),
            bounded_config(),
        )
        .expect("retained archive should export");
    assert_eq!(archive.base_snapshot.applied_sequence, SequenceNumber(3));
    assert_eq!(archive.journal_tail.len(), 2);
    let restored = TenantStore::create_in_memory().expect("restore target should open");
    restored
        .import_point_in_time_restore_archive(&archive)
        .expect("nonzero redb base should restore");
    assert_eq!(
        restored
            .export_materialized_journal_snapshot()
            .expect("restored state should export")
            .materialized_position()
            .expect("restored position should compute"),
        archive.target_position
    );
    assert_eq!(
        restored
            .read_snapshot()
            .expect("restored snapshot should open")
            .durable_journal_cursor_floor()
            .expect("restored physical floor should read"),
        SequenceNumber(3)
    );

    drop(store);
    let restarted = TenantStore::open(&path).expect("redb tenant store should restart");
    let state = restarted
        .retention_history_state(bounded_config())
        .expect("redb retention state should survive restart");
    assert_eq!(state.confirmed_floor, SequenceNumber(3));
    assert_eq!(state.physical_floor, SequenceNumber(3));
    restarted
        .insert(&sample_document("retention_tasks", "after-restart"))
        .expect("append after redb checkpoint should succeed");
    assert_eq!(
        restarted
            .read_durable_journal_from(SequenceNumber(4))
            .expect("redb suffix should remain contiguous")
            .iter()
            .map(|record| record.sequence)
            .collect::<Vec<_>>(),
        vec![SequenceNumber(4), SequenceNumber(5), SequenceNumber(6)]
    );
}

#[test]
fn sqlite_retention_checkpoint_survives_restart_and_restores_from_retained_checkpoint() {
    let dir = tempdir().expect("temporary directory should create");
    let path = dir.path().join("tenant.sqlite3");
    let store = SqliteTenantStore::open(&path).expect("SQLite tenant store should open");
    insert_documents_sqlite(&store, 5);

    let summary = store
        .compact_retained_history(bounded_config())
        .expect("SQLite retained history should compact");
    assert_eq!(summary.after.confirmed_floor, SequenceNumber(3));
    assert_eq!(summary.after.physical_floor, SequenceNumber(3));
    assert_eq!(summary.journal_records_pruned, 3);
    let archive = store
        .export_point_in_time_restore_archive(
            PointInTimeRestoreTarget::Sequence(SequenceNumber(3)),
            bounded_config(),
        )
        .expect("checkpoint-only SQLite archive should export");
    assert_eq!(archive.base_snapshot.applied_sequence, SequenceNumber(3));
    assert!(archive.journal_tail.is_empty());
    let restore_path = dir.path().join("restored.sqlite3");
    let restored =
        SqliteTenantStore::open(&restore_path).expect("SQLite restore target should open");
    restored
        .import_point_in_time_restore_archive(&archive)
        .expect("nonzero SQLite base should restore");
    assert_eq!(
        restored
            .read_snapshot()
            .expect("restored SQLite snapshot should open")
            .durable_journal_cursor_floor()
            .expect("restored SQLite physical floor should read"),
        SequenceNumber(3)
    );

    drop(store);
    let restarted = SqliteTenantStore::open(&path).expect("SQLite tenant store should restart");
    let state = restarted
        .retention_history_state(bounded_config())
        .expect("SQLite retention state should survive restart");
    assert_eq!(state.confirmed_floor, SequenceNumber(3));
    assert_eq!(state.physical_floor, SequenceNumber(3));
}

#[test]
fn memory_retention_checkpoint_survives_restart_and_restores_from_retained_checkpoint() {
    let store = MemoryTenantStore::new();
    insert_documents_memory(&store, 5);
    let summary = store
        .compact_retained_history(bounded_config())
        .expect("memory retained history should compact");
    assert_eq!(summary.after.confirmed_floor, SequenceNumber(3));
    assert_eq!(summary.after.physical_floor, SequenceNumber(3));
    let archive = store
        .export_point_in_time_restore_archive(
            PointInTimeRestoreTarget::Sequence(SequenceNumber(5)),
            bounded_config(),
        )
        .expect("memory retained archive should export");
    let restored = MemoryTenantStore::new();
    restored
        .import_point_in_time_restore_archive(&archive)
        .expect("nonzero memory base should restore");
    assert_eq!(
        restored
            .export_durable_journal_bootstrap()
            .expect("restored memory bootstrap should export")
            .cursor_floor,
        SequenceNumber(3)
    );
    let restarted = store
        .restart_from_durable_state()
        .expect("memory retention checkpoint restart should succeed");
    let state = restarted
        .retention_history_state(bounded_config())
        .expect("memory retention state should survive restart");
    assert_eq!(state.confirmed_floor, SequenceNumber(3));
    assert_eq!(state.physical_floor, SequenceNumber(3));
}

#[test]
fn embedded_retention_checkpoint_fault_before_commit_retains_history() {
    let fault = Arc::new(ScriptedFaultInjector::new([FaultOccurrence {
        point: FaultPoint::RetentionCheckpointBeforeCommit,
        visit: 1,
    }]));
    let redb = TenantStore::create_in_memory_with_simulation(
        Arc::new(ManualWallClock::new(Timestamp(100))),
        fault,
    )
    .expect("faulted redb store should open");
    insert_documents_redb(&redb, 3);
    assert!(
        redb.compact_retained_history(RetentionGcConfig::new(1).unwrap())
            .is_err()
    );
    assert_eq!(
        redb.read_durable_journal_from(SequenceNumber(1))
            .unwrap()
            .len(),
        3
    );
    assert_eq!(
        redb.retention_history_state(RetentionGcConfig::new(1).unwrap())
            .unwrap()
            .physical_floor,
        SequenceNumber(0)
    );

    let dir = tempdir().expect("temporary directory should create");
    let fault = Arc::new(ScriptedFaultInjector::new([FaultOccurrence {
        point: FaultPoint::RetentionCheckpointBeforeCommit,
        visit: 1,
    }]));
    let sqlite = SqliteTenantStore::open_with_simulation(
        dir.path().join("fault.sqlite3"),
        Arc::new(ManualWallClock::new(Timestamp(100))),
        fault,
    )
    .expect("faulted SQLite store should open");
    insert_documents_sqlite(&sqlite, 3);
    assert!(
        sqlite
            .compact_retained_history(RetentionGcConfig::new(1).unwrap())
            .is_err()
    );
    assert_eq!(
        sqlite
            .read_durable_journal_from(SequenceNumber(1))
            .unwrap()
            .len(),
        3
    );
    assert_eq!(
        sqlite
            .retention_history_state(RetentionGcConfig::new(1).unwrap())
            .unwrap()
            .physical_floor,
        SequenceNumber(0)
    );

    let fault = Arc::new(ScriptedFaultInjector::new([FaultOccurrence {
        point: FaultPoint::RetentionCheckpointBeforeCommit,
        visit: 1,
    }]));
    let memory =
        MemoryTenantStore::with_simulation(Arc::new(ManualWallClock::new(Timestamp(100))), fault);
    insert_documents_memory(&memory, 3);
    assert!(
        memory
            .compact_retained_history(RetentionGcConfig::new(1).unwrap())
            .is_err()
    );
    assert_eq!(
        memory
            .read_durable_journal_from(SequenceNumber(1))
            .unwrap()
            .len(),
        3
    );
    assert_eq!(
        memory
            .retention_history_state(RetentionGcConfig::new(1).unwrap())
            .unwrap()
            .physical_floor,
        SequenceNumber(0)
    );
}

#[test]
fn embedded_retention_checkpoint_fault_after_commit_exposes_atomic_floor() {
    let fault = Arc::new(ScriptedFaultInjector::new([FaultOccurrence {
        point: FaultPoint::RetentionCheckpointAfterCommit,
        visit: 1,
    }]));
    let redb = TenantStore::create_in_memory_with_simulation(
        Arc::new(ManualWallClock::new(Timestamp(100))),
        fault,
    )
    .expect("faulted redb store should open");
    insert_documents_redb(&redb, 3);
    assert!(
        redb.compact_retained_history(RetentionGcConfig::new(1).unwrap())
            .is_err()
    );
    let state = redb
        .retention_history_state(RetentionGcConfig::new(1).unwrap())
        .unwrap();
    assert_eq!(state.confirmed_floor, SequenceNumber(2));
    assert_eq!(state.physical_floor, SequenceNumber(2));
    assert_eq!(
        redb.read_durable_journal_from(SequenceNumber(1))
            .unwrap()
            .len(),
        1
    );

    let dir = tempdir().expect("temporary directory should create");
    let fault = Arc::new(ScriptedFaultInjector::new([FaultOccurrence {
        point: FaultPoint::RetentionCheckpointAfterCommit,
        visit: 1,
    }]));
    let sqlite = SqliteTenantStore::open_with_simulation(
        dir.path().join("fault-after.sqlite3"),
        Arc::new(ManualWallClock::new(Timestamp(100))),
        fault,
    )
    .expect("faulted SQLite store should open");
    insert_documents_sqlite(&sqlite, 3);
    assert!(
        sqlite
            .compact_retained_history(RetentionGcConfig::new(1).unwrap())
            .is_err()
    );
    let state = sqlite
        .retention_history_state(RetentionGcConfig::new(1).unwrap())
        .unwrap();
    assert_eq!(state.confirmed_floor, SequenceNumber(2));
    assert_eq!(state.physical_floor, SequenceNumber(2));
    assert_eq!(
        sqlite
            .read_durable_journal_from(SequenceNumber(1))
            .unwrap()
            .len(),
        1
    );

    let fault = Arc::new(ScriptedFaultInjector::new([FaultOccurrence {
        point: FaultPoint::RetentionCheckpointAfterCommit,
        visit: 1,
    }]));
    let memory =
        MemoryTenantStore::with_simulation(Arc::new(ManualWallClock::new(Timestamp(100))), fault);
    insert_documents_memory(&memory, 3);
    assert!(
        memory
            .compact_retained_history(RetentionGcConfig::new(1).unwrap())
            .is_err()
    );
    let state = memory
        .retention_history_state(RetentionGcConfig::new(1).unwrap())
        .unwrap();
    assert_eq!(state.confirmed_floor, SequenceNumber(2));
    assert_eq!(state.physical_floor, SequenceNumber(2));
    assert_eq!(
        memory
            .read_durable_journal_from(SequenceNumber(1))
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn retain_all_keeps_checkpoint_at_current_confirmed_floor() {
    let store = TenantStore::create_in_memory().expect("redb tenant store should open");
    insert_documents_redb(&store, 3);
    store
        .compact_retained_history(RetentionGcConfig::new(1).unwrap())
        .expect("bounded retention should compact");
    let summary = store
        .compact_retained_history(RetentionGcConfig::retain_all())
        .expect("retain-all maintenance should not move a floor backward");
    assert_eq!(summary.after.confirmed_floor, SequenceNumber(2));
    assert_eq!(summary.after.physical_floor, SequenceNumber(2));
    assert_eq!(summary.journal_records_pruned, 0);
}

#[test]
fn four_retention_windows_remain_resource_specific() {
    let config = RetentionGcConfig::with_windows(10, 20, 30, 40)
        .expect("four-window retention config should build");
    let floor = RetentionFloor::new();
    let watermarks = floor.gc_watermarks(SequenceNumber(100), config);
    assert_eq!(
        watermarks.document_versions.window_floor,
        SequenceNumber(90)
    );
    assert_eq!(watermarks.index_versions.window_floor, SequenceNumber(80));
    assert_eq!(watermarks.cdc_journal.window_floor, SequenceNumber(70));
    assert_eq!(watermarks.pitr_exports.window_floor, SequenceNumber(60));
    assert!(RetentionGcConfig::with_windows(0, 1, 1, 1).is_err());
    assert_eq!(
        RetentionGcConfig::retain_all().pitr_window_sequences,
        u64::MAX
    );
}

#[test]
fn checkpoint_format_rejects_a_mismatched_materialized_position() {
    let mut checkpoint =
        crate::MaterializedRetentionCheckpoint::genesis().expect("genesis checkpoint should build");
    checkpoint.position = crate::MaterializedPosition::new(SequenceNumber(0), "1".repeat(64))
        .expect("synthetic position should parse");
    assert!(checkpoint.validate().is_err());
}

#[test]
fn checkpoint_format_rejects_snapshot_sidecar_tamper() {
    let mut snapshot = crate::MaterializedJournalSnapshot::empty_for_point_in_time_base();
    snapshot.trigger_delivery_cursor = TriggerDeliveryCursor::new(SequenceNumber(7));
    let mut checkpoint = crate::MaterializedRetentionCheckpoint::new(snapshot, Timestamp(0))
        .expect("checkpoint with a trigger cursor should build");
    checkpoint.snapshot.trigger_delivery_cursor = TriggerDeliveryCursor::new(SequenceNumber(8));

    assert!(checkpoint.validate().is_err());
}
