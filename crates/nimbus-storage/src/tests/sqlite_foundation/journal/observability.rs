use super::*;
use crate::sqlite::SqliteWriteStatementConcept;

#[test]
#[serial_test::serial(sqlite_write_observation)]
fn sqlite_queued_batch_fail_before_observes_repeated_write_work() {
    let dir = tempdir().expect("temporary directory should create");
    let store = SqliteTenantStore::open(dir.path().join("tenant.sqlite3"))
        .expect("sqlite tenant store should open");
    let document = sample_document("observed_tasks", "observed");
    let table = document.table.clone();
    let record = sqlite_durable_write_record(
        SequenceNumber(1),
        Timestamp(100),
        &table,
        &TableId::new(),
        WriteOpType::Insert,
        document.id.clone(),
        None,
        Some(document),
    );

    store.reset_write_test_observation();
    store
        .append_durable_records_batch(std::slice::from_ref(&record))
        .expect("queued durable append should succeed");
    store
        .apply_durable_records_batch(std::slice::from_ref(&record))
        .expect("queued durable apply should succeed");
    let observation = store.write_test_observation();

    assert_eq!(
        observation.writer_opens, 1,
        "queued append and apply currently open separate writers"
    );
    assert_eq!(
        observation.format_checks, 1,
        "document-version format is checked once per durable record"
    );
    assert_eq!(
        observation.schema_checks, 1,
        "index schema is checked once per durable record"
    );
    assert_eq!(
        observation.table_identity_checks, 1,
        "table identity is checked once per durable record"
    );
    assert_eq!(
        observation.current_document_encodes, 2,
        "the current document is encoded for version and live projections"
    );

    let expected_statement_counts = [
        (SqliteWriteStatementConcept::JournalNextSequenceRead, 1),
        (SqliteWriteStatementConcept::JournalInsert, 1),
        (SqliteWriteStatementConcept::NextSequenceWrite, 1),
        (SqliteWriteStatementConcept::AppliedSequenceRead, 1),
        (SqliteWriteStatementConcept::AppliedSequenceWrite, 1),
        (SqliteWriteStatementConcept::DurableRecordRead, 0),
        (SqliteWriteStatementConcept::DocumentVersionFormatRead, 1),
        (SqliteWriteStatementConcept::DocumentVersionFormatWrite, 1),
        (SqliteWriteStatementConcept::DocumentVersionInsert, 1),
        (
            SqliteWriteStatementConcept::DocumentVersionTombstoneInsert,
            0,
        ),
        (SqliteWriteStatementConcept::IndexSchemaRead, 1),
        (SqliteWriteStatementConcept::IndexVersionFormatRead, 0),
        (SqliteWriteStatementConcept::IndexVersionFormatWrite, 0),
        (SqliteWriteStatementConcept::IndexVersionClose, 0),
        (SqliteWriteStatementConcept::IndexVersionOpen, 0),
        (SqliteWriteStatementConcept::TableIdentityCheck, 1),
        (SqliteWriteStatementConcept::DocumentPreimageRead, 1),
        (SqliteWriteStatementConcept::LiveDocumentInsert, 1),
        (SqliteWriteStatementConcept::LiveDocumentUpdate, 0),
        (SqliteWriteStatementConcept::LiveDocumentDelete, 0),
        (SqliteWriteStatementConcept::ResourceBindingUpsert, 0),
        (SqliteWriteStatementConcept::ResourceBindingDelete, 0),
    ];
    for (concept, expected) in expected_statement_counts {
        assert_eq!(
            observation.statement_prepares(concept),
            expected,
            "unexpected prepare count for {concept:?}"
        );
        assert_eq!(
            observation.statement_executes(concept),
            expected,
            "unexpected execute count for {concept:?}"
        );
    }

    store.reset_write_test_observation();
    assert_eq!(
        store.write_test_observation(),
        Default::default(),
        "write counters must be resettable"
    );

    let indexed_store = SqliteTenantStore::open(dir.path().join("indexed.sqlite3"))
        .expect("indexed sqlite tenant store should open");
    let schema = ranked_tasks_schema();
    indexed_store
        .replace_table_schema(&schema)
        .expect("indexed schema should persist");
    let table_id = sqlite_active_table_id(&indexed_store, &schema.table);
    let inserted = ranked_document(&schema.table, "v1", 1);
    let mut updated = inserted.clone();
    updated.fields.insert("title".to_string(), json!("v2"));
    updated.fields.insert("rank".to_string(), json!(2));
    updated.update_time = Timestamp(updated.update_time.0.saturating_add(1));
    let first_sequence = indexed_store
        .journal_progress()
        .expect("indexed journal progress should load")
        .durable_head
        .0
        .saturating_add(1);
    let indexed_records = vec![
        sqlite_durable_write_record(
            SequenceNumber(first_sequence),
            Timestamp(200),
            &schema.table,
            &table_id,
            WriteOpType::Insert,
            inserted.id.clone(),
            None,
            Some(inserted.clone()),
        ),
        sqlite_durable_write_record(
            SequenceNumber(first_sequence.saturating_add(1)),
            Timestamp(201),
            &schema.table,
            &table_id,
            WriteOpType::Update,
            inserted.id.clone(),
            Some(inserted.clone()),
            Some(updated.clone()),
        ),
        sqlite_durable_write_record(
            SequenceNumber(first_sequence.saturating_add(2)),
            Timestamp(202),
            &schema.table,
            &table_id,
            WriteOpType::Delete,
            inserted.id,
            Some(updated),
            None,
        ),
    ];

    indexed_store.reset_write_test_observation();
    indexed_store
        .append_durable_records_batch(&indexed_records)
        .expect("indexed durable append should succeed");
    indexed_store
        .apply_durable_records_batch(&indexed_records)
        .expect("indexed durable apply should succeed");
    let indexed_observation = indexed_store.write_test_observation();

    // Resident writer: the schema write before the observation window made
    // the connection resident, so the observed batch pair opens nothing.
    assert_eq!(indexed_observation.writer_opens, 0);
    // Batch-invariant hoisting: one format check per apply transaction, one
    // schema plan and one identity verification per distinct table key.
    assert_eq!(indexed_observation.format_checks, 1);
    assert_eq!(indexed_observation.schema_checks, 1);
    assert_eq!(indexed_observation.table_identity_checks, 1);
    assert_eq!(
        indexed_observation.current_document_encodes, 4,
        "insert and update each encode version and live projections"
    );
    // (concept, expected prepares, expected executes). Executes must stay
    // byte-identical to the SWT0 fail-before census: statement caching may
    // never change what runs. Prepares are the SWT1.1 bound: at most one
    // first-use prepare per concept per writer connection (an upper bound on
    // real parses, since several concepts share one SQL text and therefore
    // one cache entry).
    let indexed_statement_counts = [
        (SqliteWriteStatementConcept::JournalNextSequenceRead, 1, 1),
        (SqliteWriteStatementConcept::JournalInsert, 1, 3),
        (SqliteWriteStatementConcept::NextSequenceWrite, 1, 1),
        (SqliteWriteStatementConcept::AppliedSequenceRead, 1, 1),
        (SqliteWriteStatementConcept::AppliedSequenceWrite, 1, 1),
        (SqliteWriteStatementConcept::DurableRecordRead, 0, 0),
        (SqliteWriteStatementConcept::DocumentVersionFormatRead, 1, 1),
        (
            SqliteWriteStatementConcept::DocumentVersionFormatWrite,
            1,
            1,
        ),
        (SqliteWriteStatementConcept::DocumentVersionInsert, 1, 2),
        (
            SqliteWriteStatementConcept::DocumentVersionTombstoneInsert,
            1,
            1,
        ),
        (SqliteWriteStatementConcept::IndexSchemaRead, 1, 1),
        (SqliteWriteStatementConcept::IndexVersionFormatRead, 1, 1),
        (SqliteWriteStatementConcept::IndexVersionFormatWrite, 1, 1),
        (SqliteWriteStatementConcept::IndexVersionClose, 1, 2),
        (SqliteWriteStatementConcept::IndexVersionOpen, 1, 2),
        (SqliteWriteStatementConcept::TableIdentityCheck, 1, 1),
        (SqliteWriteStatementConcept::DocumentPreimageRead, 1, 3),
        (SqliteWriteStatementConcept::LiveDocumentInsert, 1, 1),
        (SqliteWriteStatementConcept::LiveDocumentUpdate, 1, 1),
        (SqliteWriteStatementConcept::LiveDocumentDelete, 1, 1),
        (SqliteWriteStatementConcept::ResourceBindingUpsert, 0, 0),
        (SqliteWriteStatementConcept::ResourceBindingDelete, 1, 1),
    ];
    for (concept, expected_prepares, expected_executes) in indexed_statement_counts {
        assert_eq!(
            indexed_observation.statement_prepares(concept),
            expected_prepares,
            "unexpected indexed prepare count for {concept:?}"
        );
        assert_eq!(
            indexed_observation.statement_executes(concept),
            expected_executes,
            "unexpected indexed execute count for {concept:?}"
        );
    }
    indexed_store.reset_write_test_observation();
    assert_eq!(indexed_store.write_test_observation(), Default::default());
}

#[test]
#[serial_test::serial(sqlite_write_observation)]
fn sqlite_wal_observation_separates_foreground_and_post_run_passive_work() {
    let dir = tempdir().expect("temporary directory should create");
    let path = dir.path().join("tenant.sqlite3");
    let store =
        SqliteTenantStore::open(&path).expect("sqlite tenant store should open for observation");
    let mut document = sample_document("checkpoint_tasks", "large");
    document
        .fields
        .insert("payload".to_string(), json!("x".repeat(5 * 1024 * 1024)));
    let table = document.table.clone();
    let record = sqlite_durable_write_record(
        SequenceNumber(1),
        Timestamp(100),
        &table,
        &TableId::new(),
        WriteOpType::Insert,
        document.id.clone(),
        None,
        Some(document),
    );

    crate::reset_sqlite_wal_checkpoint_observation(&path);
    store
        .append_durable_records_batch(std::slice::from_ref(&record))
        .expect("observed durable append should succeed");
    store
        .apply_durable_records_batch(std::slice::from_ref(&record))
        .expect("observed durable apply should succeed");
    let foreground = crate::sqlite_wal_checkpoint_observation_snapshot(&path);
    let passive = crate::probe_sqlite_passive_checkpoint(&path)
        .expect("post-run passive checkpoint probe should succeed");
    let after_passive = crate::sqlite_wal_checkpoint_observation_snapshot(&path);
    crate::reset_sqlite_wal_checkpoint_observation(&path);
    let reset = crate::sqlite_wal_checkpoint_observation_snapshot(&path);
    crate::disable_sqlite_wal_checkpoint_observation();

    assert_eq!(foreground.foreground_commit_count, 2);
    assert_eq!(foreground.observation_probe_count, 2);
    assert_eq!(foreground.observation_probe_error_count, 0);
    assert!(foreground.foreground_commit_nanos > 0);
    assert!(foreground.observation_probe_nanos > 0);
    assert!(foreground.auto_checkpoint_pages > 0);
    assert!(
        foreground.wal_high_water_frames >= foreground.auto_checkpoint_pages,
        "large queued write should cross the automatic checkpoint threshold"
    );
    assert!(
        foreground.automatic_checkpoint_count > 0,
        "foreground observation should classify automatic checkpoint work"
    );
    assert!(
        foreground.automatic_checkpoint_commit_upper_bound_nanos > 0,
        "automatic checkpoint commits should retain a timing upper bound"
    );
    assert_eq!(foreground.post_run_passive_probe_count, 0);

    assert_eq!(after_passive.foreground_commit_count, 2);
    assert_eq!(
        after_passive.automatic_checkpoint_count, foreground.automatic_checkpoint_count,
        "the post-run passive probe must not be classified as foreground work"
    );
    assert_eq!(after_passive.post_run_passive_probe_count, 1);
    assert_eq!(after_passive.post_run_passive_busy, passive.busy);
    assert_eq!(
        after_passive.post_run_passive_wal_frames,
        passive.wal_frames
    );
    assert_eq!(
        after_passive.post_run_passive_checkpointed_frames,
        passive.checkpointed_frames
    );
    assert_eq!(
        after_passive.post_run_passive_probe_nanos,
        passive.elapsed_nanos
    );
    assert_eq!(reset, Default::default());
}

/// Guards the status-only contract of the foreground observation probe.
///
/// The probe issues `PRAGMA wal_checkpoint(NOOP)`, which the workspace's
/// bundled SQLite (3.50+) parses as `SQLITE_CHECKPOINT_NOOP` — "do no work at
/// all". Older SQLite builds do not know the keyword and silently fall
/// through to a real PASSIVE checkpoint, which would make the observer
/// checkpoint the database it claims to only watch. This test fails loudly if
/// the runtime ever degrades that way: a sub-threshold workload must leave
/// the main database file untouched with its whole backlog still in the WAL.
#[test]
#[serial_test::serial(sqlite_write_observation)]
fn sqlite_wal_observation_probe_does_not_checkpoint() {
    let dir = tempdir().expect("temporary directory should create");
    let path = dir.path().join("tenant.sqlite3");
    let store = SqliteTenantStore::open(&path)
        .expect("sqlite tenant store should open for the non-perturbation probe");
    let db_len_before = std::fs::metadata(&path)
        .expect("main database file should exist before observation")
        .len();

    crate::reset_sqlite_wal_checkpoint_observation(&path);
    let table_id = TableId::new();
    for sequence in 1..=4_u64 {
        let document = sample_document("probe_tasks", &format!("doc-{sequence}"));
        let table = document.table.clone();
        let record = sqlite_durable_write_record(
            SequenceNumber(sequence),
            Timestamp(100 + sequence),
            &table,
            &table_id,
            WriteOpType::Insert,
            document.id.clone(),
            None,
            Some(document),
        );
        store
            .append_durable_records_batch(std::slice::from_ref(&record))
            .expect("observed small append should succeed");
        store
            .apply_durable_records_batch(std::slice::from_ref(&record))
            .expect("observed small apply should succeed");
    }
    let db_len_after = std::fs::metadata(&path)
        .expect("main database file should exist after observation")
        .len();
    let foreground = crate::sqlite_wal_checkpoint_observation_snapshot(&path);
    let passive = crate::probe_sqlite_passive_checkpoint(&path)
        .expect("post-run passive checkpoint probe should succeed");
    crate::disable_sqlite_wal_checkpoint_observation();

    assert_eq!(
        foreground.observation_probe_error_count, 0,
        "every foreground probe must succeed for this guard to be meaningful"
    );
    assert_eq!(
        foreground.checkpointed_high_water_frames, 0,
        "a status-only probe over a fresh sub-threshold store must observe \
         zero checkpointed frames; any other value means a probe checkpointed"
    );
    assert_eq!(
        db_len_after, db_len_before,
        "a sub-threshold observed workload must leave every page in the WAL; \
         main-database growth means the observation probe itself checkpointed"
    );
    assert!(
        passive.checkpointed_frames > 0,
        "the WAL backlog must still be checkpointable after the observed run"
    );
}

#[test]
#[serial_test::serial(sqlite_write_observation)]
fn sqlite_batch_apply_context_reloads_schema_after_mid_batch_change() {
    let dir = tempdir().expect("temporary directory should create");
    let store = SqliteTenantStore::open(dir.path().join("tenant.sqlite3"))
        .expect("sqlite tenant store should open");
    let schema = ranked_tasks_schema();
    let table = schema.table.clone();
    let table_id = TableId::new();
    let before_change = ranked_document(&table, "before", 1);
    let after_change = ranked_document(&table, "after", 2);

    let records = vec![
        sqlite_durable_write_record(
            SequenceNumber(1),
            Timestamp(100),
            &table,
            &table_id,
            WriteOpType::Insert,
            before_change.id.clone(),
            None,
            Some(before_change.clone()),
        ),
        TenantEventRecord::from_events(
            SequenceNumber(2),
            Timestamp(101),
            vec![TenantEventKind::SchemaChange {
                change: Box::new(SchemaChangeEvent::SetTable {
                    table: table.clone(),
                    table_id: table_id.clone(),
                    previous: None,
                    current: schema.clone(),
                }),
            }],
        )
        .expect("schema change record should build"),
        sqlite_durable_write_record(
            SequenceNumber(3),
            Timestamp(102),
            &table,
            &table_id,
            WriteOpType::Insert,
            after_change.id.clone(),
            None,
            Some(after_change.clone()),
        ),
    ];

    store.reset_write_test_observation();
    store
        .append_durable_records_batch(&records)
        .expect("mid-batch schema change append should succeed");
    store
        .apply_durable_records_batch(&records)
        .expect("mid-batch schema change apply should succeed");
    let observation = store.write_test_observation();

    // The pre-change write planned against the schemaless table, the schema
    // change invalidated the cached plan, and the post-change write planned
    // against the new indexed schema: two schema loads, not one and not three.
    assert_eq!(
        observation.schema_checks, 2,
        "schema change must invalidate the cached plan at its sequence boundary"
    );
    assert_eq!(
        observation.statement_executes(SqliteWriteStatementConcept::IndexSchemaRead),
        2
    );

    let index = &schema.indexes[0];
    let intervals = store
        .index_version_intervals_for_testing(&table_id, &index.id)
        .expect("index intervals should load");
    assert_eq!(
        intervals.len(),
        1,
        "only the post-change write may open a maintained-index interval"
    );
    assert_eq!(intervals[0].document_id, after_change.id);
    assert_eq!(intervals[0].visible_from, SequenceNumber(3));
    store.reset_write_test_observation();
}

#[test]
#[serial_test::serial(sqlite_write_observation)]
fn sqlite_batch_apply_context_checks_each_distinct_table_once() {
    let dir = tempdir().expect("temporary directory should create");
    let store = SqliteTenantStore::open(dir.path().join("tenant.sqlite3"))
        .expect("sqlite tenant store should open");
    let first_table = TableName::new("first_tasks").expect("first table name");
    let second_table = TableName::new("second_tasks").expect("second table name");
    let first_id = TableId::new();
    let second_id = TableId::new();

    let mut records = Vec::new();
    let mut sequence = 0_u64;
    for (table, table_id, keys) in [
        (&first_table, &first_id, ["a", "b"]),
        (&second_table, &second_id, ["c", "d"]),
    ] {
        for key in keys {
            sequence += 1;
            let document = sample_document(table.as_str(), key);
            records.push(sqlite_durable_write_record(
                SequenceNumber(sequence),
                Timestamp(100 + sequence),
                table,
                table_id,
                WriteOpType::Insert,
                document.id.clone(),
                None,
                Some(document),
            ));
        }
    }

    store.reset_write_test_observation();
    store
        .append_durable_records_batch(&records)
        .expect("multi-table append should succeed");
    store
        .apply_durable_records_batch(&records)
        .expect("multi-table apply should succeed");
    let observation = store.write_test_observation();

    assert_eq!(observation.format_checks, 1, "one format check per batch");
    assert_eq!(
        observation.schema_checks, 2,
        "one schema plan per distinct table"
    );
    assert_eq!(
        observation.table_identity_checks, 2,
        "one identity verification per distinct (table, table_id)"
    );
    assert_eq!(
        observation.statement_executes(SqliteWriteStatementConcept::DocumentPreimageRead),
        4,
        "every write keeps its own preimage read"
    );
    store.reset_write_test_observation();
}

#[test]
#[serial_test::serial(sqlite_write_observation)]
fn sqlite_resident_writer_reuses_connection_across_batches() {
    let dir = tempdir().expect("temporary directory should create");
    let path = dir.path().join("tenant.sqlite3");
    let store = SqliteTenantStore::open(&path).expect("sqlite tenant store should open");
    store.reset_write_test_observation();
    let table_id = TableId::new();
    for sequence in 1..=3_u64 {
        let document = sample_document("resident_tasks", &format!("doc-{sequence}"));
        let table = document.table.clone();
        let record = sqlite_durable_write_record(
            SequenceNumber(sequence),
            Timestamp(100 + sequence),
            &table,
            &table_id,
            WriteOpType::Insert,
            document.id.clone(),
            None,
            Some(document),
        );
        store
            .append_durable_records_batch(std::slice::from_ref(&record))
            .expect("resident append should succeed");
        store
            .apply_durable_records_batch(std::slice::from_ref(&record))
            .expect("resident apply should succeed");
    }
    let observation = store.write_test_observation();
    assert_eq!(
        observation.writer_opens, 1,
        "three queued batch pairs must share one physical writer connection"
    );
    let progress = store.journal_progress().expect("progress should load");
    assert_eq!(progress.durable_head, SequenceNumber(3));
    assert_eq!(progress.applied_head, SequenceNumber(3));
    store.reset_write_test_observation();
}

struct FailNthFaultInjector {
    target: crate::simulation::FaultPoint,
    remaining: std::sync::Mutex<u32>,
}

impl crate::simulation::FaultInjector for FailNthFaultInjector {
    fn check(&self, point: crate::simulation::FaultPoint) -> nimbus_core::Result<()> {
        if point == self.target {
            let mut remaining = self.remaining.lock().expect("fault counter lock");
            if *remaining > 0 {
                *remaining -= 1;
                return Err(Error::Internal("injected storage fault".to_string()));
            }
        }
        Ok(())
    }
}

#[test]
#[serial_test::serial(sqlite_write_observation)]
fn sqlite_resident_writer_reopens_after_write_error() {
    let dir = tempdir().expect("temporary directory should create");
    let path = dir.path().join("tenant.sqlite3");
    let store = SqliteTenantStore::open_with_simulation(
        &path,
        std::sync::Arc::new(nimbus_core::SystemWallClock),
        std::sync::Arc::new(FailNthFaultInjector {
            target: crate::simulation::FaultPoint::StorageCommitBeforeVisibility,
            remaining: std::sync::Mutex::new(1),
        }),
    )
    .expect("sqlite tenant store should open with fault injector");
    store.reset_write_test_observation();
    let table_id = TableId::new();
    let document = sample_document("poison_tasks", "doc-1");
    let table = document.table.clone();
    let record = sqlite_durable_write_record(
        SequenceNumber(1),
        Timestamp(100),
        &table,
        &table_id,
        WriteOpType::Insert,
        document.id.clone(),
        None,
        Some(document.clone()),
    );
    store
        .append_durable_records_batch(std::slice::from_ref(&record))
        .expect("append before injected fault should succeed");
    store
        .apply_durable_records_batch(std::slice::from_ref(&record))
        .expect_err("first apply must fail on the injected fault");
    store
        .apply_durable_records_batch(std::slice::from_ref(&record))
        .expect("retry after the injected fault should succeed");
    let observation = store.write_test_observation();
    assert_eq!(
        observation.writer_opens, 2,
        "an errored transaction must drop its connection so the retry reopens"
    );
    let progress = store.journal_progress().expect("progress should load");
    assert_eq!(progress.applied_head, SequenceNumber(1));
    assert_eq!(
        store.scan_table(&table).expect("scan should succeed").len(),
        1,
        "the retried apply must land exactly once"
    );
    store.reset_write_test_observation();
}

#[test]
#[serial_test::serial(sqlite_write_observation)]
fn sqlite_resident_writer_reuses_encrypted_connection() {
    let dir = tempdir().expect("temporary directory should create");
    let path = dir.path().join("tenant.sqlite3");
    let store = SqliteTenantStore::open_encrypted(&path, &[7u8; 32])
        .expect("encrypted sqlite tenant store should open");
    store.reset_write_test_observation();
    let table_id = TableId::new();
    for sequence in 1..=2_u64 {
        let document = sample_document("secret_tasks", &format!("doc-{sequence}"));
        let table = document.table.clone();
        let record = sqlite_durable_write_record(
            SequenceNumber(sequence),
            Timestamp(100 + sequence),
            &table,
            &table_id,
            WriteOpType::Insert,
            document.id.clone(),
            None,
            Some(document),
        );
        store
            .append_durable_records_batch(std::slice::from_ref(&record))
            .expect("encrypted append should succeed");
        store
            .apply_durable_records_batch(std::slice::from_ref(&record))
            .expect("encrypted apply should succeed");
    }
    assert_eq!(
        store.write_test_observation().writer_opens,
        1,
        "the encrypted writer must key and verify once, then stay resident"
    );
    store.reset_write_test_observation();
}

#[test]
#[serial_test::serial(sqlite_write_observation)]
fn sqlite_resident_writer_coexists_with_concurrent_point_writers() {
    use crate::TenantPointWrite;

    let dir = tempdir().expect("temporary directory should create");
    let path = dir.path().join("tenant.sqlite3");
    let store = std::sync::Arc::new(
        SqliteTenantStore::open(&path).expect("sqlite tenant store should open"),
    );

    // Four concurrent point writers model non-committer traffic (object
    // manifests, replica reconciliation): whoever misses the resident slot
    // opens its own connection, and SQLite's busy handling serializes the
    // transactions. Every write must make progress within the busy timeout.
    let mut workers = Vec::new();
    for worker in 0..4_u64 {
        let store = store.clone();
        workers.push(std::thread::spawn(move || {
            for item in 0..25_u64 {
                let document = sample_document("overlap_tasks", &format!("w{worker}-doc-{item}"));
                store
                    .insert_document(&document)
                    .expect("concurrent point write should make progress");
            }
        }));
    }
    for worker in workers {
        worker.join().expect("point-writer thread should not panic");
    }

    let live = store
        .scan_table(&TableName::new("overlap_tasks").expect("table name"))
        .expect("scan should succeed");
    assert_eq!(live.len(), 100, "every concurrent point write must land");
    let progress = store.journal_progress().expect("progress should load");
    assert_eq!(
        progress.durable_head, progress.applied_head,
        "point writes commit journal and effects atomically"
    );
    assert_eq!(
        progress.durable_head,
        SequenceNumber(100),
        "the commit log must stay dense under concurrent writers"
    );

    // The queued route still works on the same store afterward.
    let table_id = sqlite_active_table_id(&store, &TableName::new("overlap_tasks").expect("t"));
    let document = sample_document("overlap_tasks", "queued-after");
    let record = sqlite_durable_write_record(
        SequenceNumber(101),
        Timestamp(500),
        &TableName::new("overlap_tasks").expect("t"),
        &table_id,
        WriteOpType::Insert,
        document.id.clone(),
        None,
        Some(document),
    );
    store
        .append_durable_records_batch(std::slice::from_ref(&record))
        .expect("queued append after overlap should succeed");
    store
        .apply_durable_records_batch(std::slice::from_ref(&record))
        .expect("queued apply after overlap should succeed");
    assert_eq!(
        store
            .journal_progress()
            .expect("progress should load")
            .applied_head,
        SequenceNumber(101)
    );
}
