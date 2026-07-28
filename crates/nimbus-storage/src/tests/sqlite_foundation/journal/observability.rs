use super::*;
use crate::sqlite::SqliteWriteStatementConcept;

#[test]
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
        observation.writer_opens, 2,
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

    assert_eq!(indexed_observation.writer_opens, 2);
    assert_eq!(indexed_observation.format_checks, 3);
    assert_eq!(indexed_observation.schema_checks, 3);
    assert_eq!(indexed_observation.table_identity_checks, 3);
    assert_eq!(
        indexed_observation.current_document_encodes, 4,
        "insert and update each encode version and live projections"
    );
    let indexed_statement_counts = [
        (SqliteWriteStatementConcept::JournalNextSequenceRead, 1),
        (SqliteWriteStatementConcept::JournalInsert, 3),
        (SqliteWriteStatementConcept::NextSequenceWrite, 1),
        (SqliteWriteStatementConcept::AppliedSequenceRead, 1),
        (SqliteWriteStatementConcept::AppliedSequenceWrite, 1),
        (SqliteWriteStatementConcept::DurableRecordRead, 0),
        (SqliteWriteStatementConcept::DocumentVersionFormatRead, 3),
        (SqliteWriteStatementConcept::DocumentVersionFormatWrite, 1),
        (SqliteWriteStatementConcept::DocumentVersionInsert, 3),
        (SqliteWriteStatementConcept::IndexSchemaRead, 3),
        (SqliteWriteStatementConcept::IndexVersionFormatRead, 3),
        (SqliteWriteStatementConcept::IndexVersionFormatWrite, 1),
        (SqliteWriteStatementConcept::IndexVersionClose, 2),
        (SqliteWriteStatementConcept::IndexVersionOpen, 2),
        (SqliteWriteStatementConcept::TableIdentityCheck, 3),
        (SqliteWriteStatementConcept::DocumentPreimageRead, 3),
        (SqliteWriteStatementConcept::LiveDocumentInsert, 1),
        (SqliteWriteStatementConcept::LiveDocumentUpdate, 1),
        (SqliteWriteStatementConcept::LiveDocumentDelete, 1),
        (SqliteWriteStatementConcept::ResourceBindingUpsert, 0),
        (SqliteWriteStatementConcept::ResourceBindingDelete, 1),
    ];
    for (concept, expected) in indexed_statement_counts {
        assert_eq!(
            indexed_observation.statement_prepares(concept),
            expected,
            "unexpected indexed prepare count for {concept:?}"
        );
        assert_eq!(
            indexed_observation.statement_executes(concept),
            expected,
            "unexpected indexed execute count for {concept:?}"
        );
    }
    indexed_store.reset_write_test_observation();
    assert_eq!(indexed_store.write_test_observation(), Default::default());
}

#[test]
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
