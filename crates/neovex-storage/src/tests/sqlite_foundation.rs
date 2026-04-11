use super::*;
use neovex_core::{
    CronJob, CronSchedule, Mutation, ScheduledJob, ScheduledJobOutcome, ScheduledJobResult,
};

use crate::{ResolvedScheduleOp, ResolvedWrite};

#[test]
fn sqlite_store_initializes_wal_foundation_and_empty_progress() {
    let dir = tempdir().expect("temporary directory should create");
    let store = SqliteTenantStore::open(dir.path().join("tenant.sqlite3"))
        .expect("sqlite tenant store should open");

    assert_eq!(
        store.journal_mode().expect("journal mode should read"),
        "wal",
        "sqlite foundation should enable WAL mode for tenant files"
    );
    assert_eq!(
        store
            .journal_progress()
            .expect("journal progress should read"),
        crate::store::JournalProgress {
            durable_head: SequenceNumber(0),
            applied_head: SequenceNumber(0),
        }
    );
    assert!(
        store
            .metadata_blob("missing")
            .expect("metadata read should succeed")
            .is_none(),
        "new sqlite foundations should start with empty metadata"
    );
}

#[tokio::test]
async fn sqlite_async_read_cancellation_still_prevents_queued_execution() {
    let dir = tempdir().expect("temporary directory should create");
    let store = Arc::new(
        SqliteTenantStore::open(dir.path().join("tenant.sqlite3"))
            .expect("sqlite tenant store should open"),
    );
    let storage =
        SqliteTenantStorage::with_max_concurrent_reads(store, tokio::runtime::Handle::current(), 1);
    let first_gate = BlockingReadGate::new();
    let first_gate_for_task = first_gate.clone();
    let first_storage = storage.clone();
    let first = tokio::spawn(async move {
        first_storage
            .execute(move |_store| {
                first_gate_for_task.block();
                Ok(())
            })
            .await
    });

    timeout(Duration::from_secs(1), first_gate.wait_until_entered())
        .await
        .expect("first read should acquire the only permit");

    let started = Arc::new(AtomicBool::new(false));
    let cancel = Arc::new(Notify::new());
    let started_for_task = started.clone();
    let cancel_for_wait = cancel.clone();
    let queued_storage = storage.clone();
    let second = tokio::spawn(async move {
        queued_storage
            .execute_cancellable(
                async move {
                    cancel_for_wait.notified().await;
                },
                || Ok(()),
                move |_store, _check_cancel| {
                    started_for_task.store(true, Ordering::SeqCst);
                    Ok(())
                },
            )
            .await
    });

    cancel.notify_one();
    let error = timeout(Duration::from_secs(1), second)
        .await
        .expect("queued sqlite read should resolve after cancellation")
        .expect("queued sqlite read task should join successfully")
        .expect_err("queued sqlite read should cancel");
    assert!(matches!(error, Error::Cancelled));
    assert!(
        !started.load(Ordering::SeqCst),
        "queued sqlite read should not begin executing once canceled"
    );

    first_gate.release();
    first
        .await
        .expect("first sqlite read task should join successfully")
        .expect("first sqlite read should complete");
}

#[test]
fn sqlite_store_enforces_direct_read_connection_limit() {
    let dir = tempdir().expect("temporary directory should create");
    let store =
        SqliteTenantStore::open_with_max_read_connections(dir.path().join("tenant.sqlite3"), 1)
            .expect("sqlite tenant store should open with explicit read limit");

    let first_snapshot = store
        .read_snapshot()
        .expect("first direct sqlite read snapshot should acquire the only connection");
    let error = match store.read_snapshot() {
        Ok(_) => {
            panic!("second direct sqlite read snapshot should exhaust the explicit pool limit")
        }
        Err(error) => error,
    };
    assert!(
        matches!(error, Error::ResourceExhausted(message) if message.contains("sqlite read connection pool exhausted")),
        "direct callers should get an explicit resource-exhausted error once the store-level pool limit is hit"
    );

    drop(first_snapshot);

    store
        .read_snapshot()
        .expect("released sqlite read connection should be reusable after the snapshot drops");
}

#[tokio::test]
async fn sqlite_async_write_schema_change_persists_after_reopen() {
    let dir = tempdir().expect("temporary directory should create");
    let path = dir.path().join("tenant.sqlite3");
    let store = Arc::new(SqliteTenantStore::open(&path).expect("sqlite tenant store should open"));
    let first = Document::new(
        TableName::new("tasks").expect("table name should build"),
        serde_json::Map::from_iter([("rank".to_string(), serde_json::json!(7))]),
    );
    let second = Document::new(
        TableName::new("tasks").expect("table name should build"),
        serde_json::Map::from_iter([("rank".to_string(), serde_json::json!(9))]),
    );
    store
        .insert(&first)
        .expect("seed insert before async schema write should succeed");
    store
        .insert(&second)
        .expect("second seed insert before async schema write should succeed");
    let storage =
        SqliteTenantStorage::with_max_concurrent_reads(store, tokio::runtime::Handle::current(), 1);
    let schema = TableSchema {
        table: TableName::new("tasks").expect("table name should build"),
        fields: vec![FieldSchema {
            name: "rank".to_string(),
            field_type: FieldType::Number,
            required: false,
        }],
        indexes: vec![IndexDefinition {
            name: "by_rank".to_string(),
            fields: vec!["rank".to_string()],
        }],
        access_policy: None,
    };

    let schema_for_task = schema.clone();
    storage
        .execute_write(move |transaction| transaction.replace_table_schema(&schema_for_task))
        .await
        .expect("async schema write should succeed");

    let reopened = SqliteTenantStore::open(&path).expect("sqlite tenant store should reopen");
    let persisted = reopened
        .load_schema()
        .expect("schema should read after reopen");
    assert!(
        persisted.get_table(&schema.table).is_some(),
        "async sqlite schema writes should persist schema rows before the store reopens"
    );
    assert_eq!(
        reopened
            .index_scan_eq(&schema.table, "by_rank", &serde_json::json!(7))
            .expect("index scan should succeed after reopen")
            .len(),
        1,
        "async sqlite schema writes should also rebuild durable index entries for existing rows"
    );
}

#[tokio::test]
async fn sqlite_async_write_schema_change_updates_live_schema_cache() {
    let dir = tempdir().expect("temporary directory should create");
    let store = Arc::new(
        SqliteTenantStore::open(dir.path().join("tenant.sqlite3"))
            .expect("sqlite tenant store should open"),
    );
    let document = Document::new(
        TableName::new("tasks").expect("table name should build"),
        serde_json::Map::from_iter([
            ("rank".to_string(), serde_json::json!(7)),
            ("title".to_string(), serde_json::json!("alpha")),
        ]),
    );
    store
        .insert(&document)
        .expect("seed insert before async schema write should succeed");
    let storage = SqliteTenantStorage::with_max_concurrent_reads(
        store.clone(),
        tokio::runtime::Handle::current(),
        1,
    );
    let rank_schema = TableSchema {
        table: TableName::new("tasks").expect("table name should build"),
        fields: vec![
            FieldSchema {
                name: "rank".to_string(),
                field_type: FieldType::Number,
                required: false,
            },
            FieldSchema {
                name: "title".to_string(),
                field_type: FieldType::String,
                required: false,
            },
        ],
        indexes: vec![IndexDefinition {
            name: "by_rank".to_string(),
            fields: vec!["rank".to_string()],
        }],
        access_policy: None,
    };

    let rank_schema_for_task = rank_schema.clone();
    storage
        .execute_write(move |transaction| transaction.replace_table_schema(&rank_schema_for_task))
        .await
        .expect("async schema write should succeed");

    assert_eq!(
        store
            .load_schema()
            .expect("live schema cache should read")
            .get_table(&rank_schema.table),
        Some(&rank_schema)
    );
    assert_eq!(
        store
            .index_scan_eq(&rank_schema.table, "by_rank", &serde_json::json!(7))
            .expect("rank index scan should succeed after live cache refresh"),
        vec![document.clone()]
    );

    let title_schema = TableSchema {
        table: rank_schema.table.clone(),
        fields: rank_schema.fields.clone(),
        indexes: vec![IndexDefinition {
            name: "by_title".to_string(),
            fields: vec!["title".to_string()],
        }],
        access_policy: None,
    };
    let title_schema_for_task = title_schema.clone();
    storage
        .execute_write(move |transaction| transaction.replace_table_schema(&title_schema_for_task))
        .await
        .expect("second async schema write should succeed");

    assert_eq!(
        store
            .load_schema()
            .expect("live schema cache should refresh after second write")
            .get_table(&title_schema.table),
        Some(&title_schema)
    );
    assert_eq!(
        store
            .index_scan_eq(&title_schema.table, "by_title", &serde_json::json!("alpha"))
            .expect("new title index scan should succeed"),
        vec![document.clone()]
    );
    let error = store
        .index_scan_eq(&title_schema.table, "by_rank", &serde_json::json!(7))
        .expect_err("old index lookup should fail after schema replacement");
    assert!(
        matches!(error, Error::InvalidInput(_)),
        "old index lookups should fail once the live schema cache refreshes: {error:?}"
    );
}

#[tokio::test]
async fn sqlite_async_write_precommit_cancellation_leaves_no_state() {
    let dir = tempdir().expect("temporary directory should create");
    let store = Arc::new(
        SqliteTenantStore::open(dir.path().join("tenant.sqlite3"))
            .expect("sqlite tenant store should open"),
    );
    let storage = SqliteTenantStorage::with_max_concurrent_reads(
        store.clone(),
        tokio::runtime::Handle::current(),
        2,
    );
    let gate = BlockingReadGate::new();
    let gate_for_task = gate.clone();
    let cancel = Arc::new(Notify::new());
    let cancel_for_wait = cancel.clone();
    let handle = tokio::spawn({
        let storage = storage.clone();
        async move {
            storage
                .execute_write_cancellable(
                    async move {
                        cancel_for_wait.notified().await;
                    },
                    || Ok(()),
                    move |transaction| {
                        transaction.put_metadata("marker", b"before")?;
                        gate_for_task.block();
                        Ok("marker".to_string())
                    },
                )
                .await
        }
    });

    timeout(Duration::from_secs(1), gate.wait_until_entered())
        .await
        .expect("sqlite write should reach the pre-commit gate");
    cancel.notify_one();
    tokio::time::sleep(Duration::from_millis(25)).await;
    gate.release();

    let outcome = timeout(Duration::from_secs(1), handle)
        .await
        .expect("sqlite async write should resolve after cancellation")
        .expect("sqlite write task should join successfully")
        .expect("sqlite write executor should return an outcome");
    assert!(matches!(outcome, TenantWriteOutcome::CancelledBeforeCommit));
    assert!(
        store
            .metadata_blob("marker")
            .expect("metadata read should succeed")
            .is_none(),
        "pre-commit cancellation should roll back sqlite metadata writes"
    );
}

#[tokio::test]
async fn sqlite_async_write_after_commit_still_reports_committed() {
    let dir = tempdir().expect("temporary directory should create");
    let faults = BlockingFaultInjector::new(FaultPoint::StorageCommitAfterVisibilityBeforeReturn);
    let store = Arc::new(
        SqliteTenantStore::open_with_simulation(
            dir.path().join("tenant.sqlite3"),
            Arc::new(ManualClock::new(Timestamp(10_000))),
            faults.clone(),
        )
        .expect("sqlite tenant store should open with simulation seams"),
    );
    let storage = SqliteTenantStorage::with_max_concurrent_reads(
        store.clone(),
        tokio::runtime::Handle::current(),
        2,
    );
    let cancel = Arc::new(Notify::new());
    let cancel_for_wait = cancel.clone();
    let handle = tokio::spawn({
        let storage = storage.clone();
        async move {
            storage
                .execute_write_cancellable(
                    async move {
                        cancel_for_wait.notified().await;
                    },
                    || Ok(()),
                    move |transaction| {
                        transaction.put_metadata("marker", b"after")?;
                        Ok("marker".to_string())
                    },
                )
                .await
        }
    });

    timeout(Duration::from_secs(1), faults.wait_until_entered())
        .await
        .expect("sqlite write should block after the durable commit point");
    cancel.notify_one();
    faults.release();

    let outcome = timeout(Duration::from_secs(1), handle)
        .await
        .expect("sqlite async write should resolve after post-commit cancellation")
        .expect("sqlite write task should join successfully")
        .expect("sqlite write executor should return an outcome");
    match outcome {
        TenantWriteOutcome::Committed(committed) => {
            assert_eq!(committed.value, "marker".to_string());
            assert!(
                committed.commit.is_none(),
                "foundation writes do not emit logical commit entries yet"
            );
        }
        TenantWriteOutcome::CancelledBeforeCommit => {
            panic!("post-commit cancellation must not downgrade a committed sqlite write")
        }
    }
    assert_eq!(
        store
            .metadata_blob("marker")
            .expect("metadata read should succeed"),
        Some(b"after".to_vec())
    );
}

#[test]
fn sqlite_store_round_trips_schema_get_and_index_scans() {
    let dir = tempdir().expect("temporary directory should create");
    let store = SqliteTenantStore::open(dir.path().join("tenant.sqlite3"))
        .expect("sqlite tenant store should open");
    let table = TableName::new("tasks").expect("table should build");
    let schema = TableSchema {
        table: table.clone(),
        fields: Vec::new(),
        indexes: vec![IndexDefinition {
            name: "by_status_rank".to_string(),
            fields: vec!["status".to_string(), "rank".to_string()],
        }],
        access_policy: None,
    };
    store
        .replace_table_schema(&schema)
        .expect("sqlite schema should save");
    assert_eq!(
        store
            .load_schema()
            .expect("schema should load")
            .get_table(&table),
        Some(&schema)
    );

    let open_one = Document {
        id: DocumentId::new(),
        table: table.clone(),
        creation_time: Timestamp(1),
        fields: serde_json::Map::from_iter([
            ("status".to_string(), json!("open")),
            ("rank".to_string(), json!(1)),
        ]),
    };
    let open_three = Document {
        id: DocumentId::new(),
        table: table.clone(),
        creation_time: Timestamp(2),
        fields: serde_json::Map::from_iter([
            ("status".to_string(), json!("open")),
            ("rank".to_string(), json!(3)),
        ]),
    };
    let closed_two = Document {
        id: DocumentId::new(),
        table: table.clone(),
        creation_time: Timestamp(3),
        fields: serde_json::Map::from_iter([
            ("status".to_string(), json!("closed")),
            ("rank".to_string(), json!(2)),
        ]),
    };
    for document in [&open_one, &open_three, &closed_two] {
        store
            .insert_document_for_testing(document)
            .expect("document should insert");
    }

    assert_eq!(
        store
            .get(&table, &open_one.id)
            .expect("get should succeed")
            .as_ref(),
        Some(&open_one)
    );

    let exact = store
        .index_scan_eq(&table, "by_status_rank", &json!("open"))
        .expect("exact scan should succeed");
    assert_eq!(
        exact
            .iter()
            .map(|document| document
                .get_field("rank")
                .cloned()
                .expect("rank should exist"))
            .collect::<Vec<_>>(),
        vec![json!(1), json!(3)]
    );

    let prefix = store
        .index_scan_prefix(&table, "by_status_rank", &[json!("open"), json!(3)])
        .expect("prefix scan should succeed");
    assert_eq!(prefix, vec![open_three.clone()]);

    let composite = store
        .index_scan_composite_range_cancellable(
            &table,
            "by_status_rank",
            &[json!("open")],
            Some(&json!(2)),
            Some(&json!(4)),
            true,
            true,
            &mut || Ok(()),
        )
        .expect("composite range scan should succeed");
    assert_eq!(composite, vec![open_three.clone()]);
}

#[test]
fn sqlite_index_query_plan_builders_match_runtime_sql_shape() {
    let exact = crate::sqlite_index_scan_prefix_query_sql(&["status"], 1)
        .expect("single-field indexed query SQL should build");
    assert_eq!(
        exact,
        "SELECT id, creation_time, data_json
         FROM documents
         WHERE table_name = ?1 AND json_extract(data_json, '$.\"status\"') = ?2
         ORDER BY id"
    );

    let composite = crate::sqlite_index_scan_composite_range_query_sql(
        &["team", "status", "rank"],
        2,
        true,
        true,
        true,
        false,
    )
    .expect("composite indexed query SQL should build");
    assert_eq!(
        composite,
        "SELECT id, creation_time, data_json
         FROM documents
         WHERE table_name = ?1 AND json_extract(data_json, '$.\"team\"') = ?2 AND json_extract(data_json, '$.\"status\"') = ?3 AND json_extract(data_json, '$.\"rank\"') >= ?4 AND json_extract(data_json, '$.\"rank\"') < ?5
         ORDER BY json_extract(data_json, '$.\"rank\"'), id"
    );
}

#[test]
fn sqlite_index_query_plans_elide_temp_btree_for_equality_prefixes() {
    let dir = tempdir().expect("temporary directory should create");
    let path = dir.path().join("tenant.sqlite3");
    let store = SqliteTenantStore::open(&path).expect("sqlite tenant store should open");
    let schema = TableSchema {
        table: TableName::new("tasks").expect("table name should build"),
        fields: vec![
            FieldSchema {
                name: "team".to_string(),
                field_type: FieldType::String,
                required: false,
            },
            FieldSchema {
                name: "status".to_string(),
                field_type: FieldType::String,
                required: false,
            },
            FieldSchema {
                name: "rank".to_string(),
                field_type: FieldType::Number,
                required: false,
            },
        ],
        indexes: vec![
            IndexDefinition {
                name: "by_status".to_string(),
                fields: vec!["status".to_string()],
            },
            IndexDefinition {
                name: "by_team_status_rank".to_string(),
                fields: vec!["team".to_string(), "status".to_string(), "rank".to_string()],
            },
        ],
        access_policy: None,
    };
    store
        .replace_table_schema(&schema)
        .expect("sqlite schema should save");

    let conn = rusqlite::Connection::open(&path).expect("raw sqlite connection should open");
    let exact_plan = explain_query_plan(
        &conn,
        &crate::sqlite_index_scan_prefix_query_sql(&["status"], 1)
            .expect("single-field indexed query SQL should build"),
        rusqlite::params!["tasks", "open"],
    );
    assert!(
        exact_plan
            .iter()
            .any(|detail| detail.contains("USING INDEX idx_tasks_by_status")),
        "single-field scan should use the intended index: {exact_plan:?}"
    );
    assert!(
        exact_plan
            .iter()
            .all(|detail| !detail.contains("USE TEMP B-TREE")),
        "single-field scan should avoid a temp B-tree once equality-constrained order fields are elided: {exact_plan:?}"
    );

    let composite_plan = explain_query_plan(
        &conn,
        &crate::sqlite_index_scan_composite_range_query_sql(
            &["team", "status", "rank"],
            2,
            true,
            true,
            true,
            false,
        )
        .expect("composite indexed query SQL should build"),
        rusqlite::params!["tasks", "alpha", "open", 500_i64, 2_500_i64],
    );
    assert!(
        composite_plan
            .iter()
            .any(|detail| detail.contains("USING INDEX idx_tasks_by_team_status_rank")),
        "composite scan should use the intended index: {composite_plan:?}"
    );
    assert!(
        composite_plan
            .iter()
            .all(|detail| !detail.contains("USE TEMP B-TREE")),
        "composite scan should avoid a temp B-tree once equality-constrained order fields are elided: {composite_plan:?}"
    );
}

fn explain_query_plan<P>(conn: &rusqlite::Connection, statement: &str, params: P) -> Vec<String>
where
    P: rusqlite::Params,
{
    let explain = format!("EXPLAIN QUERY PLAN {statement}");
    let mut stmt = conn
        .prepare(explain.as_str())
        .expect("query plan statement should prepare");
    let mut rows = stmt
        .query(params)
        .expect("query plan statement should execute");
    let mut detail_rows = Vec::new();
    while let Some(row) = rows.next().expect("query plan row should advance") {
        detail_rows.push(
            row.get::<_, String>(3)
                .expect("query plan detail should read"),
        );
    }
    detail_rows
}

#[test]
fn sqlite_direct_writes_emit_commit_entries_and_round_trip_journal_reads() {
    let dir = tempdir().expect("temporary directory should create");
    let store = SqliteTenantStore::open(dir.path().join("tenant.sqlite3"))
        .expect("sqlite tenant store should open");
    let document = sample_document("tasks", "Hello");

    let insert_commit = store.insert(&document).expect("insert should succeed");
    let patch = serde_json::Map::from_iter([("title".to_string(), json!("Updated"))]);
    let update_commit = store
        .update(&document.table, &document.id, &patch)
        .expect("update should succeed");
    let (delete_commit, removed_document) = store
        .delete_returning_document(&document.table, &document.id)
        .expect("delete should succeed");

    assert_eq!(insert_commit.sequence, SequenceNumber(1));
    assert_eq!(update_commit.sequence, SequenceNumber(2));
    assert_eq!(delete_commit.sequence, SequenceNumber(3));
    assert_eq!(
        removed_document.fields.get("title"),
        Some(&json!("Updated"))
    );
    assert!(
        store
            .get(&document.table, &document.id)
            .expect("get should succeed after delete")
            .is_none()
    );
    assert_eq!(
        store
            .journal_progress()
            .expect("journal progress should read"),
        crate::store::JournalProgress {
            durable_head: SequenceNumber(3),
            applied_head: SequenceNumber(3),
        }
    );

    let entries = store
        .read_commit_log_from(SequenceNumber(1))
        .expect("commit log should read");
    assert_eq!(entries.len(), 3);
    assert_eq!(entries[0].writes[0].op_type, WriteOpType::Insert);
    assert_eq!(entries[1].writes[0].op_type, WriteOpType::Update);
    assert_eq!(entries[2].writes[0].op_type, WriteOpType::Delete);
}

#[test]
fn sqlite_scheduled_execution_marker_deduplicates_insert_commit() {
    let dir = tempdir().expect("temporary directory should create");
    let store = SqliteTenantStore::open(dir.path().join("tenant.sqlite3"))
        .expect("sqlite tenant store should open");
    let document = sample_document("tasks", "Hello once");

    let first = store
        .insert_once(&document, Some("scheduled:test-job"))
        .expect("first insert should succeed");
    let second = store
        .insert_once(&document, Some("scheduled:test-job"))
        .expect("second insert should succeed");

    assert!(first.is_some(), "first scheduled execution should commit");
    assert!(
        second.is_none(),
        "second scheduled execution should be skipped"
    );
    assert!(
        store
            .scheduled_execution_exists("scheduled:test-job")
            .expect("scheduled execution marker should read")
    );
    assert_eq!(
        store
            .latest_sequence()
            .expect("latest sequence should read"),
        SequenceNumber(1)
    );
    let tasks = store
        .scan_table_matching_with_filters_cancellable(
            &TableName::new("tasks").expect("table name should be valid"),
            &[],
            &mut || Ok(()),
            |_| Ok(true),
        )
        .expect("scan should succeed");
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].fields.get("title"), Some(&json!("Hello once")));
}

#[test]
fn sqlite_durable_journal_batch_append_enforces_no_holes() {
    let dir = tempdir().expect("temporary directory should create");
    let store = SqliteTenantStore::open(dir.path().join("tenant.sqlite3"))
        .expect("sqlite tenant store should open");
    let first = DurableMutationRecord::new(
        SequenceNumber(1),
        Timestamp(10),
        vec![WriteOp {
            table: TableName::new("tasks").expect("table name should be valid"),
            op_type: WriteOpType::Insert,
            doc_id: DocumentId::new(),
            previous: None,
            current: Some(sample_document("tasks", "First")),
        }],
        None,
    )
    .expect("first durable record should build");
    let second = DurableMutationRecord::new(
        SequenceNumber(2),
        Timestamp(11),
        vec![WriteOp {
            table: TableName::new("tasks").expect("table name should be valid"),
            op_type: WriteOpType::Insert,
            doc_id: DocumentId::new(),
            previous: None,
            current: Some(sample_document("tasks", "Second")),
        }],
        None,
    )
    .expect("second durable record should build");

    store
        .append_durable_records_batch(&[first.clone(), second.clone()])
        .expect("initial batch append should succeed");
    assert_eq!(
        store
            .journal_progress()
            .expect("journal progress should read"),
        crate::store::JournalProgress {
            durable_head: SequenceNumber(2),
            applied_head: SequenceNumber(0),
        }
    );

    let error = store
        .append_durable_records_batch(&[DurableMutationRecord::new(
            SequenceNumber(4),
            Timestamp(12),
            vec![WriteOp {
                table: TableName::new("tasks").expect("table name should be valid"),
                op_type: WriteOpType::Insert,
                doc_id: DocumentId::new(),
                previous: None,
                current: Some(sample_document("tasks", "Gap")),
            }],
            None,
        )
        .expect("gap record should build")])
        .expect_err("batch append should reject sequence holes");
    assert!(
        matches!(error, Error::Internal(message) if message.contains("expected sequence 3, got 4"))
    );
    assert_eq!(
        store
            .latest_sequence()
            .expect("latest sequence should stay stable"),
        SequenceNumber(2)
    );
    assert_eq!(
        store
            .read_durable_journal_from(SequenceNumber(1))
            .expect("durable journal should read")
            .into_iter()
            .map(|record| record.sequence)
            .collect::<Vec<_>>(),
        vec![SequenceNumber(1), SequenceNumber(2)]
    );
}

#[test]
fn sqlite_recovery_replays_durable_but_unapplied_records() {
    let dir = tempdir().expect("temporary directory should create");
    let store = SqliteTenantStore::open(dir.path().join("tenant.sqlite3"))
        .expect("sqlite tenant store should open");
    let first = sample_document("tasks", "First");
    let second = sample_document("tasks", "Second");
    let records = vec![
        DurableMutationRecord::new(
            SequenceNumber(1),
            Timestamp(100),
            vec![WriteOp {
                table: first.table.clone(),
                op_type: WriteOpType::Insert,
                doc_id: first.id,
                previous: None,
                current: Some(first.clone()),
            }],
            None,
        )
        .expect("first durable record should build"),
        DurableMutationRecord::new(
            SequenceNumber(2),
            Timestamp(101),
            vec![WriteOp {
                table: second.table.clone(),
                op_type: WriteOpType::Insert,
                doc_id: second.id,
                previous: None,
                current: Some(second.clone()),
            }],
            None,
        )
        .expect("second durable record should build"),
    ];

    store
        .append_durable_records_batch(&records)
        .expect("durable append should succeed");
    assert_eq!(
        store
            .journal_progress()
            .expect("journal progress should read"),
        crate::store::JournalProgress {
            durable_head: SequenceNumber(2),
            applied_head: SequenceNumber(0),
        }
    );
    assert!(
        store
            .scan_table(&TableName::new("tasks").expect("table name should be valid"))
            .expect("scan should succeed")
            .is_empty(),
        "unapplied durable records must not become visible through table scans"
    );

    let progress = store
        .recover_durable_journal()
        .expect("recovery should apply pending durable records");
    assert_eq!(
        progress,
        crate::store::JournalProgress {
            durable_head: SequenceNumber(2),
            applied_head: SequenceNumber(2),
        }
    );

    let documents = store
        .scan_table(&TableName::new("tasks").expect("table name should be valid"))
        .expect("scan should succeed after recovery");
    assert_eq!(documents.len(), 2);
    let mut titles = documents
        .iter()
        .map(|document| {
            document
                .fields
                .get("title")
                .and_then(|value| value.as_str())
                .expect("recovered document title should exist")
        })
        .collect::<Vec<_>>();
    titles.sort_unstable();
    assert_eq!(titles, vec!["First", "Second"]);
}

#[test]
fn sqlite_execution_unit_batch_rolls_back_when_schedule_ops_fail() {
    let dir = tempdir().expect("temporary directory should create");
    let store = SqliteTenantStore::open(dir.path().join("tenant.sqlite3"))
        .expect("sqlite tenant store should open");
    let document = sample_document("tasks", "batched");

    let error = store
        .apply_execution_unit_batch(
            &[ResolvedWrite::Insert {
                document: document.clone(),
                indexes: Vec::new(),
            }],
            &[ResolvedScheduleOp::Cancel {
                job_id: DocumentId::new(),
            }],
        )
        .expect_err("batch should fail when a scheduled cancel misses");
    assert!(matches!(error, Error::ScheduledJobNotFound(_)));
    assert!(
        store
            .get(&document.table, &document.id)
            .expect("document lookup should succeed")
            .is_none(),
        "failed batches must roll back document writes"
    );
    assert!(
        store
            .list_scheduled_jobs()
            .expect("pending jobs should read")
            .is_empty()
    );
    assert_eq!(
        store
            .latest_sequence()
            .expect("latest sequence should remain empty"),
        SequenceNumber(0)
    );
}

#[test]
fn sqlite_execution_unit_batch_commits_documents_and_schedule_ops_together() {
    let dir = tempdir().expect("temporary directory should create");
    let store = SqliteTenantStore::open(dir.path().join("tenant.sqlite3"))
        .expect("sqlite tenant store should open");
    let document = sample_document("tasks", "batched");
    let scheduled_job = scheduled_insert_job(Timestamp(5_000), "queued");

    let commit = store
        .apply_execution_unit_batch(
            &[ResolvedWrite::Insert {
                document: document.clone(),
                indexes: Vec::new(),
            }],
            &[ResolvedScheduleOp::Insert {
                job: scheduled_job.clone(),
            }],
        )
        .expect("batch should succeed")
        .expect("batch with writes should emit a commit");

    assert_eq!(commit.sequence, SequenceNumber(1));
    assert_eq!(commit.writes.len(), 1);
    assert_eq!(commit.writes[0].op_type, WriteOpType::Insert);
    assert_eq!(
        store
            .get(&document.table, &document.id)
            .expect("document lookup should succeed")
            .as_ref(),
        Some(&document)
    );
    assert_eq!(
        store
            .list_scheduled_jobs()
            .expect("pending jobs should read"),
        vec![scheduled_job]
    );
}

#[test]
fn sqlite_scheduler_state_round_trips_results_crons_and_recovery() {
    let dir = tempdir().expect("temporary directory should create");
    let store = SqliteTenantStore::open(dir.path().join("tenant.sqlite3"))
        .expect("sqlite tenant store should open");
    let job = scheduled_insert_job(Timestamp(1_000), "due");

    store
        .insert_scheduled_job(&job)
        .expect("scheduled insert should succeed");
    assert_eq!(
        store
            .next_scheduled_work_at()
            .expect("next scheduled work should read"),
        Some(Timestamp(1_000))
    );
    assert!(
        store
            .has_scheduled_work()
            .expect("pending work should count"),
    );

    let claimed = store
        .claim_due_jobs(Timestamp(1_000))
        .expect("claim should succeed");
    assert_eq!(claimed, vec![job.clone()]);
    assert!(
        store
            .list_scheduled_jobs()
            .expect("pending jobs should read")
            .is_empty()
    );

    store
        .recover_running_jobs(Timestamp(2_000))
        .expect("running-job recovery should succeed");
    let recovered = store
        .list_scheduled_jobs()
        .expect("pending jobs should read");
    assert_eq!(recovered.len(), 1);
    assert_eq!(recovered[0].id, job.id);
    assert_eq!(recovered[0].run_at, Timestamp(2_000));

    let claimed = store
        .claim_due_jobs(Timestamp(2_000))
        .expect("second claim should succeed");
    assert_eq!(claimed.len(), 1);
    let result = ScheduledJobResult {
        id: job.id,
        run_at: Timestamp(2_000),
        finished_at: Timestamp(2_500),
        mutation: claimed[0].mutation.clone(),
        outcome: ScheduledJobOutcome::Completed,
        error: None,
    };
    store
        .record_scheduled_job_result(&result)
        .expect("result should persist");
    store
        .complete_scheduled_job(&job.id)
        .expect("complete should succeed");
    assert_eq!(
        store
            .get_scheduled_job_result(&job.id)
            .expect("result lookup should succeed"),
        Some(result)
    );

    let cron = CronJob {
        name: "heartbeat".to_string(),
        schedule: CronSchedule::Interval { seconds: 10 },
        mutation: Mutation::Insert {
            table: TableName::new("tasks").expect("table name should be valid"),
            fields: serde_json::Map::from_iter([("title".to_string(), json!("heartbeat"))]),
        },
        enabled: true,
        last_run: None,
        next_run: Timestamp(3_000),
        created_at: Timestamp(500),
    };
    store
        .save_cron_job(&cron)
        .expect("cron save should succeed");
    assert_eq!(
        store.load_cron_jobs().expect("cron load should succeed"),
        vec![cron.clone()]
    );
    assert_eq!(
        store
            .next_scheduled_work_at()
            .expect("next scheduled work should read"),
        Some(Timestamp(3_000))
    );
    assert!(
        store
            .has_scheduled_work()
            .expect("cron should count as work")
    );
    store
        .delete_cron_job(&cron.name)
        .expect("cron delete should succeed");
    assert!(
        !store
            .has_scheduled_work()
            .expect("no work should remain after cleanup"),
    );
}

#[test]
fn sqlite_claim_due_jobs_includes_u64_max_boundary() {
    let dir = tempdir().expect("temporary directory should create");
    let store = SqliteTenantStore::open(dir.path().join("tenant.sqlite3"))
        .expect("sqlite tenant store should open");
    let job = scheduled_insert_job(Timestamp(u64::MAX), "max");
    store
        .insert_scheduled_job(&job)
        .expect("scheduled insert should succeed");

    let claimed = store
        .claim_due_jobs(Timestamp(u64::MAX))
        .expect("claim should succeed");
    assert_eq!(claimed, vec![job]);
}

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

#[test]
fn sqlite_durable_journal_stream_uses_cursor_floor_after_retention_cut() {
    let dir = tempdir().expect("temporary directory should create");
    let path = dir.path().join("tenant.sqlite3");
    let store = SqliteTenantStore::open(&path).expect("sqlite tenant store should open");
    let first = sample_document("tasks", "first");
    let second = sample_document("tasks", "second");
    store.insert(&first).expect("first insert should succeed");
    store.insert(&second).expect("second insert should succeed");

    rusqlite::Connection::open(&path)
        .expect("raw sqlite connection should open")
        .execute("DELETE FROM commit_log WHERE sequence = 1", [])
        .expect("first journal row should delete");

    let error = store
        .stream_durable_journal(SequenceNumber(0), 10)
        .expect_err("cursor behind the retained floor should fail");
    assert!(matches!(
        error,
        Error::InvalidInput(message) if message.contains("behind the retention floor 1")
    ));

    let page = store
        .stream_durable_journal(SequenceNumber(1), 10)
        .expect("cursor at the retained floor should succeed");
    assert_eq!(page.cursor_floor, SequenceNumber(1));
    assert_eq!(page.latest_sequence, SequenceNumber(2));
    assert_eq!(page.next_cursor, SequenceNumber(2));
    assert_eq!(page.records.len(), 1);
    assert_eq!(page.records[0].sequence, SequenceNumber(2));
}

fn scheduled_insert_job(run_at: Timestamp, title: &str) -> ScheduledJob {
    ScheduledJob {
        id: DocumentId::new(),
        run_at,
        mutation: Mutation::Insert {
            table: TableName::new("tasks").expect("table name should be valid"),
            fields: serde_json::Map::from_iter([("title".to_string(), json!(title))]),
        },
        created_at: Timestamp(1_000),
    }
}

fn ranked_tasks_schema() -> TableSchema {
    TableSchema {
        table: TableName::new("tasks").expect("table name should be valid"),
        fields: vec![FieldSchema {
            name: "rank".to_string(),
            field_type: FieldType::Number,
            required: true,
        }],
        indexes: vec![IndexDefinition {
            name: "by_rank".to_string(),
            fields: vec!["rank".to_string()],
        }],
        access_policy: None,
    }
}

fn ranked_document(table: &TableName, title: &str, rank: u64) -> Document {
    Document::new(
        table.clone(),
        serde_json::Map::from_iter([
            ("title".to_string(), json!(title)),
            ("rank".to_string(), json!(rank)),
        ]),
    )
}
