use super::support::*;

#[test]
fn sqlite_materialized_snapshot_plus_journal_tail_rebuild_matches_live_state() {
    let live_dir = tempdir().expect("temporary directory should create");
    let live = SqliteTenantStore::open(live_dir.path().join("live.sqlite3"))
        .expect("sqlite tenant store should open");
    let table_schema = ranked_tasks_schema();
    let table = table_schema.table.clone();
    live.replace_table_schema(&table_schema)
        .expect("table schema should persist");

    let first = ranked_document(&table, "First", 1);
    live.insert_with_indexes(&first, &table_schema.indexes)
        .expect("first insert should succeed");
    let snapshot = live
        .export_materialized_journal_snapshot()
        .expect("snapshot export should succeed");
    assert_eq!(snapshot.version, 1);
    assert_eq!(snapshot.applied_sequence, SequenceNumber(1));
    assert_eq!(snapshot.durable_head, SequenceNumber(1));

    let second = ranked_document(&table, "Second", 3);
    live.insert_with_indexes(&second, &table_schema.indexes)
        .expect("second insert should succeed");
    live.update_with_indexes(
        &table,
        &first.id,
        &serde_json::Map::from_iter([("rank".to_string(), json!(2))]),
        &table_schema.indexes,
    )
    .expect("update should succeed");

    let bootstrap = live
        .export_durable_journal_bootstrap()
        .expect("bootstrap export should succeed");
    assert_eq!(bootstrap.resume_after, SequenceNumber(3));
    assert_eq!(bootstrap.bootstrap_cut, SequenceNumber(3));
    assert_eq!(bootstrap.cursor_floor, SequenceNumber(0));

    let tail = live
        .read_durable_journal_from(SequenceNumber(snapshot.applied_sequence.0 + 1))
        .expect("journal tail should read");
    let rebuilt_dir = tempdir().expect("temporary directory should create");
    let rebuilt = SqliteTenantStore::open(rebuilt_dir.path().join("rebuilt.sqlite3"))
        .expect("rebuilt sqlite store should open");
    let progress = rebuilt
        .rebuild_materialized_journal_from_snapshot(&snapshot, &tail, None)
        .expect("snapshot plus tail rebuild should succeed");

    assert_eq!(
        progress,
        live.journal_progress()
            .expect("live journal progress should read")
    );
    assert_eq!(
        rebuilt.load_schema().expect("rebuilt schema should load"),
        live.load_schema().expect("live schema should load")
    );
    assert_eq!(
        rebuilt
            .scan_table(&table)
            .expect("rebuilt scan should succeed"),
        live.scan_table(&table).expect("live scan should succeed")
    );
    assert_eq!(
        rebuilt
            .index_scan_eq(&table, "by_rank", &json!(2))
            .expect("rebuilt rank scan should succeed"),
        live.index_scan_eq(&table, "by_rank", &json!(2))
            .expect("live rank scan should succeed")
    );
    assert_eq!(
        rebuilt
            .index_scan_eq(&table, "by_rank", &json!(3))
            .expect("rebuilt rank scan should succeed"),
        live.index_scan_eq(&table, "by_rank", &json!(3))
            .expect("live rank scan should succeed")
    );
}

#[test]
fn sqlite_materialized_snapshot_records_durable_boundary_and_rejects_incomplete_tail() {
    let dir = tempdir().expect("temporary directory should create");
    let store = SqliteTenantStore::open(dir.path().join("tenant.sqlite3"))
        .expect("sqlite tenant store should open");
    let document = sample_document("tasks", "durable-only");
    let record = DurableMutationRecord::new(
        SequenceNumber(1),
        Timestamp(100),
        vec![WriteOp {
            table: document.table.clone(),
            op_type: WriteOpType::Insert,
            doc_id: document.id,
            previous: None,
            current: Some(document.clone()),
        }],
        None,
    )
    .expect("durable record should build");
    store
        .append_durable_records_batch(std::slice::from_ref(&record))
        .expect("durable append should succeed");

    let snapshot = store
        .export_materialized_journal_snapshot()
        .expect("snapshot export should succeed");
    assert_eq!(snapshot.applied_sequence, SequenceNumber(0));
    assert_eq!(snapshot.durable_head, SequenceNumber(1));

    let rebuilt_dir = tempdir().expect("temporary directory should create");
    let rebuilt = SqliteTenantStore::open(rebuilt_dir.path().join("rebuilt.sqlite3"))
        .expect("rebuilt sqlite store should open");
    let error = rebuilt
        .rebuild_materialized_journal_from_snapshot(&snapshot, &[], None)
        .expect_err("rebuild should reject a missing tail");
    assert!(matches!(
        error,
        Error::InvalidInput(message)
            if message.contains("available head 0 is behind snapshot durable head 1")
    ));

    let rebuilt_dir = tempdir().expect("temporary directory should create");
    let rebuilt = SqliteTenantStore::open(rebuilt_dir.path().join("rebuilt.sqlite3"))
        .expect("rebuilt sqlite store should open");
    let progress = rebuilt
        .rebuild_materialized_journal_from_snapshot(&snapshot, &[record], None)
        .expect("rebuild should succeed once the tail is present");
    assert_eq!(
        progress,
        crate::store::JournalProgress {
            durable_head: SequenceNumber(1),
            applied_head: SequenceNumber(1),
        }
    );
    assert_eq!(
        rebuilt
            .scan_table(&document.table)
            .expect("rebuilt scan should succeed"),
        vec![document]
    );
}
