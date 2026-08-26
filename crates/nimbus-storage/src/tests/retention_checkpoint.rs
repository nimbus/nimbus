use super::*;
use crate::{
    PointInTimeRestoreTarget, RetentionGcConfig, RetentionParticipant, RetentionReadFloors,
};
use nimbus_core::{DocumentId, DocumentLocator, DocumentPath, ResourcePathBinding};

use super::historical_fixtures::{historical_read_shape, indexed_rank_schema, ranked_document};

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

#[derive(Clone)]
struct IndexedNonzeroBaseArchive {
    archive: crate::PointInTimeRestoreArchive,
    table: TableName,
    table_id: TableId,
    document_id: DocumentId,
    base_document: Document,
    target_document: Document,
    schema: TableSchema,
}

fn indexed_nonzero_base_archive() -> IndexedNonzeroBaseArchive {
    let clock = Arc::new(ManualWallClock::new(Timestamp(100)));
    let source = TenantStore::create_in_memory_with_simulation(
        clock.clone(),
        Arc::new(crate::NoopFaultInjector),
    )
    .expect("PITR source should open");
    let table = TableName::new("indexed_restore_tasks").expect("table name should be valid");
    let (schema, _) = indexed_rank_schema(&table);
    source
        .replace_table_schema(&schema)
        .expect("indexed schema should persist");

    clock.set(Timestamp(200));
    let document = ranked_document(&table, "base", 1);
    let insert = source
        .insert(&document)
        .expect("base document should insert");
    let table_id = insert.writes[0].table_id.clone();

    clock.set(Timestamp(300));
    source
        .insert(&ranked_document(&table, "checkpoint filler", 10))
        .expect("checkpoint filler should insert");
    clock.set(Timestamp(400));
    source
        .insert(&ranked_document(&table, "tail filler", 11))
        .expect("tail filler should insert");
    clock.set(Timestamp(500));
    let update = source
        .update(
            &table,
            &document.id,
            &serde_json::Map::from_iter([
                ("title".to_string(), json!("target")),
                ("rank".to_string(), json!(2)),
            ]),
        )
        .expect("base document should update in the retained tail");

    let config = RetentionGcConfig::new(2).expect("two-sequence window should build");
    let summary = source
        .compact_retained_history(config)
        .expect("source history should compact");
    assert_eq!(summary.after.confirmed_floor, SequenceNumber(3));
    let archive = source
        .export_point_in_time_restore_archive(
            PointInTimeRestoreTarget::Sequence(update.sequence),
            config,
        )
        .expect("nonzero-base archive should export");
    assert_eq!(archive.base_snapshot.applied_sequence, SequenceNumber(3));
    assert_eq!(archive.journal_tail.len(), 2);

    let base_document = archive
        .base_snapshot
        .documents
        .iter()
        .find(|candidate| candidate.id == document.id)
        .cloned()
        .expect("base snapshot should contain the indexed document");
    let target_document = archive
        .journal_tail
        .iter()
        .flat_map(|record| &record.writes)
        .filter(|write| write.doc_id == document.id)
        .filter_map(|write| write.current.clone())
        .next_back()
        .expect("retained tail should contain the document update");

    IndexedNonzeroBaseArchive {
        archive,
        table,
        table_id,
        document_id: document.id,
        base_document,
        target_document,
        schema,
    }
}

fn assert_redb_imported_history(store: &TenantStore, fixture: &IndexedNonzeroBaseArchive) {
    let snapshot = store.read_snapshot().expect("redb snapshot should open");
    for sequence in [SequenceNumber(3), SequenceNumber(4)] {
        assert_eq!(
            snapshot
                .get_document_version_at(&fixture.table_id, &fixture.document_id, sequence)
                .expect("redb base document version should read"),
            Some(fixture.base_document.clone())
        );
        let shape =
            historical_read_shape(&fixture.table, &fixture.table_id, &fixture.schema, sequence);
        let documents = snapshot
            .historical_index_scan_eq_cancellable(&shape, "by_rank", &json!(1), &mut || Ok(()))
            .expect("redb base index version should read");
        assert_eq!(documents, vec![fixture.base_document.clone()]);
    }
    assert_eq!(
        snapshot
            .get_document_version_at(&fixture.table_id, &fixture.document_id, SequenceNumber(5),)
            .expect("redb target document version should read"),
        Some(fixture.target_document.clone())
    );
    let target_shape = historical_read_shape(
        &fixture.table,
        &fixture.table_id,
        &fixture.schema,
        SequenceNumber(5),
    );
    assert_eq!(
        snapshot
            .historical_index_scan_eq_cancellable(&target_shape, "by_rank", &json!(2), &mut || Ok(
                ()
            ),)
            .expect("redb target index version should read"),
        vec![fixture.target_document.clone()]
    );
}

fn assert_sqlite_imported_history(store: &SqliteTenantStore, fixture: &IndexedNonzeroBaseArchive) {
    let snapshot = store.read_snapshot().expect("SQLite snapshot should open");
    for sequence in [SequenceNumber(3), SequenceNumber(4)] {
        assert_eq!(
            snapshot
                .get_document_version_at(
                    &fixture.table,
                    &fixture.table_id,
                    &fixture.document_id,
                    sequence,
                )
                .expect("SQLite base document version should read"),
            Some(fixture.base_document.clone())
        );
        let shape =
            historical_read_shape(&fixture.table, &fixture.table_id, &fixture.schema, sequence);
        let documents = snapshot
            .historical_index_scan_eq_cancellable(&shape, "by_rank", &json!(1), &mut || Ok(()))
            .expect("SQLite base index version should read");
        assert_eq!(documents, vec![fixture.base_document.clone()]);
    }
    assert_eq!(
        snapshot
            .get_document_version_at(
                &fixture.table,
                &fixture.table_id,
                &fixture.document_id,
                SequenceNumber(5),
            )
            .expect("SQLite target document version should read"),
        Some(fixture.target_document.clone())
    );
    let target_shape = historical_read_shape(
        &fixture.table,
        &fixture.table_id,
        &fixture.schema,
        SequenceNumber(5),
    );
    assert_eq!(
        snapshot
            .historical_index_scan_eq_cancellable(&target_shape, "by_rank", &json!(2), &mut || Ok(
                ()
            ),)
            .expect("SQLite target index version should read"),
        vec![fixture.target_document.clone()]
    );
}

fn assert_empty_import_target(snapshot: crate::MaterializedJournalSnapshot) {
    assert_eq!(snapshot.applied_sequence, SequenceNumber(0));
    assert_eq!(snapshot.durable_head, SequenceNumber(0));
    assert!(snapshot.documents.is_empty());
    assert!(snapshot.schema.tables.is_empty());
    assert!(snapshot.table_identities.is_empty());
}

#[test]
fn embedded_pitr_import_fault_rolls_back_and_same_archive_retries() {
    let fixture = indexed_nonzero_base_archive();

    let fault = Arc::new(ScriptedFaultInjector::new([FaultOccurrence {
        point: FaultPoint::JournalAppendBeforeDurableFlush,
        visit: 1,
    }]));
    let redb = TenantStore::create_in_memory_with_simulation(
        Arc::new(ManualWallClock::new(Timestamp(1_000))),
        fault,
    )
    .expect("faulted redb restore target should open");
    redb.import_point_in_time_restore_archive(&fixture.archive)
        .expect_err("redb import fault should abort the complete state change");
    assert_empty_import_target(
        redb.export_materialized_journal_snapshot()
            .expect("redb target snapshot should export"),
    );
    assert_eq!(
        redb.retention_history_state(RetentionGcConfig::retain_all())
            .expect("redb retention state should remain readable")
            .physical_floor,
        SequenceNumber(0)
    );
    assert_eq!(
        redb.document_version_storage_diagnostic()
            .expect("redb document-version diagnostic should read")
            .version_count,
        0
    );
    assert_eq!(
        redb.index_version_storage_diagnostic()
            .expect("redb index-version diagnostic should read")
            .version_count,
        0
    );
    redb.import_point_in_time_restore_archive(&fixture.archive)
        .expect("the same redb archive should retry immediately");

    let dir = tempdir().expect("temporary directory should create");
    let fault = Arc::new(ScriptedFaultInjector::new([FaultOccurrence {
        point: FaultPoint::JournalAppendBeforeDurableFlush,
        visit: 1,
    }]));
    let sqlite = SqliteTenantStore::open_with_simulation(
        dir.path().join("faulted-import.sqlite3"),
        Arc::new(ManualWallClock::new(Timestamp(1_000))),
        fault,
    )
    .expect("faulted SQLite restore target should open");
    sqlite
        .import_point_in_time_restore_archive(&fixture.archive)
        .expect_err("SQLite import fault should abort the complete state change");
    assert_empty_import_target(
        sqlite
            .export_materialized_journal_snapshot()
            .expect("SQLite target snapshot should export"),
    );
    assert_eq!(
        sqlite
            .retention_history_state(RetentionGcConfig::retain_all())
            .expect("SQLite retention state should remain readable")
            .physical_floor,
        SequenceNumber(0)
    );
    assert_eq!(
        sqlite
            .document_version_storage_diagnostic()
            .expect("SQLite document-version diagnostic should read")
            .version_count,
        0
    );
    assert_eq!(
        sqlite
            .index_version_storage_diagnostic()
            .expect("SQLite index-version diagnostic should read")
            .version_count,
        0
    );
    sqlite
        .import_point_in_time_restore_archive(&fixture.archive)
        .expect("the same SQLite archive should retry immediately");

    let fault = Arc::new(ScriptedFaultInjector::new([FaultOccurrence {
        point: FaultPoint::JournalAppendBeforeDurableFlush,
        visit: 1,
    }]));
    let memory =
        MemoryTenantStore::with_simulation(Arc::new(ManualWallClock::new(Timestamp(1_000))), fault);
    memory
        .import_point_in_time_restore_archive(&fixture.archive)
        .expect_err("memory import fault should abort the complete state change");
    assert_empty_import_target(
        memory
            .export_materialized_journal_snapshot()
            .expect("memory target snapshot should export"),
    );
    assert_eq!(
        memory
            .retention_history_state(RetentionGcConfig::retain_all())
            .expect("memory retention state should remain readable")
            .physical_floor,
        SequenceNumber(0)
    );
    memory
        .import_point_in_time_restore_archive(&fixture.archive)
        .expect("the same memory archive should retry immediately");
}

#[test]
fn embedded_pitr_import_seeds_base_history_and_survives_restart() {
    let fixture = indexed_nonzero_base_archive();
    let dir = tempdir().expect("temporary directory should create");

    let redb_path = dir.path().join("imported.redb");
    let redb = TenantStore::open(&redb_path).expect("redb restore target should open");
    redb.import_point_in_time_restore_archive(&fixture.archive)
        .expect("redb archive should import");
    assert_redb_imported_history(&redb, &fixture);
    drop(redb);
    let redb = TenantStore::open(&redb_path).expect("redb restore target should restart");
    assert_redb_imported_history(&redb, &fixture);

    let sqlite_path = dir.path().join("imported.sqlite3");
    let sqlite = SqliteTenantStore::open(&sqlite_path).expect("SQLite restore target should open");
    sqlite
        .import_point_in_time_restore_archive(&fixture.archive)
        .expect("SQLite archive should import");
    assert_sqlite_imported_history(&sqlite, &fixture);
    drop(sqlite);
    let sqlite =
        SqliteTenantStore::open(&sqlite_path).expect("SQLite restore target should restart");
    assert_sqlite_imported_history(&sqlite, &fixture);
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
        redb.load_retention_checkpoint().unwrap().1,
        RetentionReadFloors::new(SequenceNumber(2), SequenceNumber(2), SequenceNumber(2),)
    );
    assert_eq!(
        redb.retention_floor().published_read_floors(),
        RetentionReadFloors::new(SequenceNumber(2), SequenceNumber(2), SequenceNumber(2),)
    );
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
        sqlite.load_retention_checkpoint().unwrap().1,
        RetentionReadFloors::new(SequenceNumber(2), SequenceNumber(2), SequenceNumber(2),)
    );
    assert_eq!(
        sqlite.retention_floor().published_read_floors(),
        RetentionReadFloors::new(SequenceNumber(2), SequenceNumber(2), SequenceNumber(2),)
    );
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
        memory.retention_floor().published_read_floors(),
        RetentionReadFloors::new(SequenceNumber(2), SequenceNumber(2), SequenceNumber(2),)
    );
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
fn prepared_retention_conflicts_when_a_new_pin_lowers_the_safe_floor() {
    let config = RetentionGcConfig::new(1).expect("bounded retention config should build");

    let redb = TenantStore::create_in_memory().expect("redb tenant store should open");
    insert_documents_redb(&redb, 4);
    let prepared = redb
        .prepare_retained_history(config)
        .expect("redb retention should prepare");
    let _redb_pin = redb.pin_retention_participant(
        RetentionParticipant::CdcSubscription,
        SequenceNumber(1),
        None,
        "concurrent CDC reader",
    );
    let error = redb
        .finalize_retained_history(prepared)
        .expect_err("redb must reject a cut invalidated by a new pin");
    assert!(error.to_string().contains("invalidated"));
    assert_eq!(
        redb.retention_history_state(config)
            .expect("redb retention state should load")
            .physical_floor,
        SequenceNumber(0)
    );

    let dir = tempdir().expect("temporary directory should create");
    let sqlite = SqliteTenantStore::open(dir.path().join("pin-race.sqlite3"))
        .expect("SQLite tenant store should open");
    insert_documents_sqlite(&sqlite, 4);
    let prepared = sqlite
        .prepare_retained_history(config)
        .expect("SQLite retention should prepare");
    let _sqlite_pin = sqlite.pin_retention_participant(
        RetentionParticipant::CdcSubscription,
        SequenceNumber(1),
        None,
        "concurrent CDC reader",
    );
    let error = sqlite
        .finalize_retained_history(prepared)
        .expect_err("SQLite must reject a cut invalidated by a new pin");
    assert!(error.to_string().contains("invalidated"));
    assert_eq!(
        sqlite
            .retention_history_state(config)
            .expect("SQLite retention state should load")
            .physical_floor,
        SequenceNumber(0)
    );

    let memory = MemoryTenantStore::new();
    insert_documents_memory(&memory, 4);
    let prepared = memory
        .prepare_retained_history(config)
        .expect("memory retention should prepare");
    let _memory_pin = memory.pin_retention_participant(
        RetentionParticipant::CdcSubscription,
        SequenceNumber(1),
        None,
        "concurrent CDC reader",
    );
    let error = memory
        .finalize_retained_history(prepared)
        .expect_err("memory must reject a cut invalidated by a new pin");
    assert!(error.to_string().contains("invalidated"));
    assert_eq!(
        memory
            .retention_history_state(config)
            .expect("memory retention state should load")
            .physical_floor,
        SequenceNumber(0)
    );
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
