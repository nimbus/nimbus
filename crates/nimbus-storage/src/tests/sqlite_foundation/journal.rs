use super::support::*;
use crate::RetentionGcConfig;
use std::ops::Bound;

mod observability;
mod ppsc;

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
fn sqlite_pending_prefix_blocks_generic_zero_write() {
    let dir = tempdir().expect("temporary directory should create");
    let store = SqliteTenantStore::open(dir.path().join("tenant.sqlite3"))
        .expect("sqlite tenant store should open");
    exercise_pending_prefix_blocks_generic_zero_write(&store, "sqlite_pending_prefix", || {
        store.set_trigger_delivery_cursor(TriggerDeliveryCursor::new(SequenceNumber(1)))
    });
}

#[test]
fn sqlite_durable_update_guard_reports_corruption() {
    let dir = tempdir().expect("temporary directory should create");
    let missing = SqliteTenantStore::open(dir.path().join("missing.sqlite3"))
        .expect("sqlite tenant store should open");
    exercise_durable_update_guard_is_corruption(&missing, "sqlite_missing_preimage", false);
    let mismatched = SqliteTenantStore::open(dir.path().join("mismatched.sqlite3"))
        .expect("sqlite tenant store should open");
    exercise_durable_update_guard_is_corruption(&mismatched, "sqlite_mismatched_preimage", true);
}

#[test]
fn sqlite_document_versions_track_insert_update_delete_history() {
    let dir = tempdir().expect("temporary directory should create");
    let store = SqliteTenantStore::open(dir.path().join("tenant.sqlite3"))
        .expect("sqlite tenant store should open");
    let document = sample_document("versioned_tasks", "v1");
    let insert = store.insert(&document).expect("insert should succeed");
    let table_id = insert.writes[0].table_id.clone();
    let patch = serde_json::Map::from_iter([("title".to_string(), json!("v2"))]);
    let update = store
        .update(&document.table, &document.id, &patch)
        .expect("update should succeed");
    let delete = store
        .delete(&document.table, &document.id)
        .expect("delete should succeed");

    let at_insert = store
        .get_document_version_at(&document.table, &table_id, &document.id, insert.sequence)
        .expect("insert version should load")
        .expect("insert version should exist");
    let at_update = store
        .get_document_version_at(&document.table, &table_id, &document.id, update.sequence)
        .expect("update version should load")
        .expect("update version should exist");
    let at_delete = store
        .get_document_version_at(&document.table, &table_id, &document.id, delete.sequence)
        .expect("delete version should load");

    assert_eq!(at_insert.fields.get("title"), Some(&json!("v1")));
    assert_eq!(at_update.fields.get("title"), Some(&json!("v2")));
    assert_eq!(at_delete, None);
    assert!(
        store
            .get(&document.table, &document.id)
            .expect("current row get should succeed")
            .is_none(),
        "current row should still reflect latest delete"
    );
}

#[test]
fn sqlite_prepared_record_materializes_document_version_once() {
    let dir = tempdir().expect("temporary directory should create");
    let store = SqliteTenantStore::open(dir.path().join("tenant.sqlite3"))
        .expect("sqlite tenant store should open");
    let document = sample_document("prepared_versioned_tasks", "prepared");
    let table_id = TableId::new();
    let record = TenantEventRecord::new(
        SequenceNumber(1),
        Timestamp(100),
        vec![WriteOp {
            table: document.table.clone(),
            table_id,
            op_type: WriteOpType::Insert,
            doc_id: document.id.clone(),
            resource_path_binding: None,
            trigger_write_origin: None,
            previous: None,
            current: Some(document),
        }],
        None,
    )
    .expect("prepared record should build");

    store
        .apply_prepared_write_batch(&record, &[], None)
        .expect("prepared record should apply")
        .expect("prepared record should commit");

    assert_eq!(
        store
            .journal_progress()
            .expect("prepared record progress should load"),
        crate::JournalProgress {
            durable_head: record.sequence,
            applied_head: record.sequence,
        },
        "prepared apply must advance both journal progress heads"
    );

    let diagnostic = store
        .storage_health_diagnostic()
        .expect("document-version diagnostic should load");
    assert_eq!(diagnostic.document_versions.version_count, 1);
    assert_eq!(
        diagnostic.document_versions.min_sequence,
        Some(record.sequence)
    );
    assert_eq!(
        diagnostic.document_versions.max_sequence,
        Some(record.sequence)
    );
}

#[test]
fn sqlite_document_versions_are_materialized_during_durable_recovery() {
    let dir = tempdir().expect("temporary directory should create");
    let store = SqliteTenantStore::open(dir.path().join("tenant.sqlite3"))
        .expect("sqlite tenant store should open");
    let table = TableName::new("versioned_replay_tasks").expect("table name should be valid");
    let table_id = TableId::new();
    let inserted = sample_document("versioned_replay_tasks", "v1");
    let mut updated = inserted.clone();
    updated.fields.insert("title".to_string(), json!("v2"));
    updated.update_time = Timestamp(updated.update_time.0.saturating_add(1));
    let records = vec![
        TenantEventRecord::new(
            SequenceNumber(1),
            Timestamp(100),
            vec![WriteOp {
                table: table.clone(),
                table_id: table_id.clone(),
                op_type: WriteOpType::Insert,
                doc_id: inserted.id.clone(),
                resource_path_binding: None,
                trigger_write_origin: None,
                previous: None,
                current: Some(inserted.clone()),
            }],
            None,
        )
        .expect("insert durable record should build"),
        TenantEventRecord::new(
            SequenceNumber(2),
            Timestamp(101),
            vec![WriteOp {
                table: table.clone(),
                table_id: table_id.clone(),
                op_type: WriteOpType::Update,
                doc_id: inserted.id.clone(),
                resource_path_binding: None,
                trigger_write_origin: None,
                previous: Some(inserted.clone()),
                current: Some(updated.clone()),
            }],
            None,
        )
        .expect("update durable record should build"),
        TenantEventRecord::new(
            SequenceNumber(3),
            Timestamp(102),
            vec![WriteOp {
                table: table.clone(),
                table_id: table_id.clone(),
                op_type: WriteOpType::Delete,
                doc_id: inserted.id.clone(),
                resource_path_binding: None,
                trigger_write_origin: None,
                previous: Some(updated.clone()),
                current: None,
            }],
            None,
        )
        .expect("delete durable record should build"),
    ];

    store
        .append_durable_records_batch(&records)
        .expect("durable append should succeed");
    assert!(
        store
            .get_document_version_at(&table, &table_id, &inserted.id, SequenceNumber(3))
            .expect("unapplied version lookup should succeed")
            .is_none(),
        "durable-only records must not materialize historical versions before recovery"
    );

    store
        .recover_durable_journal()
        .expect("durable recovery should succeed");

    let at_insert = store
        .get_document_version_at(&table, &table_id, &inserted.id, SequenceNumber(1))
        .expect("insert replay version should load")
        .expect("insert replay version should exist");
    let at_update = store
        .get_document_version_at(&table, &table_id, &inserted.id, SequenceNumber(2))
        .expect("update replay version should load")
        .expect("update replay version should exist");
    let at_delete = store
        .get_document_version_at(&table, &table_id, &inserted.id, SequenceNumber(3))
        .expect("delete replay version should load");

    assert_eq!(at_insert.fields.get("title"), Some(&json!("v1")));
    assert_eq!(at_update.fields.get("title"), Some(&json!("v2")));
    assert_eq!(at_delete, None);
    assert!(
        store
            .get(&table, &inserted.id)
            .expect("current row get should succeed")
            .is_none(),
        "replayed current row should still reflect latest delete"
    );
}

#[test]
fn sqlite_document_versions_storage_diagnostic_reports_format_and_range() {
    let dir = tempdir().expect("temporary directory should create");
    let store = SqliteTenantStore::open(dir.path().join("tenant.sqlite3"))
        .expect("sqlite tenant store should open");
    let document = sample_document("versioned_diagnostic_tasks", "v1");
    let insert = store.insert(&document).expect("insert should succeed");
    let patch = serde_json::Map::from_iter([("title".to_string(), json!("v2"))]);
    let update = store
        .update(&document.table, &document.id, &patch)
        .expect("update should succeed");
    let delete = store
        .delete(&document.table, &document.id)
        .expect("delete should succeed");

    let health = store
        .storage_health_diagnostic()
        .expect("health diagnostic should load");

    assert_eq!(
        health.document_versions.format_version,
        Some(crate::CURRENT_DOCUMENT_VERSION_STORAGE_FORMAT)
    );
    assert_eq!(health.document_versions.version_count, 3);
    assert_eq!(health.document_versions.min_sequence, Some(insert.sequence));
    assert_eq!(health.document_versions.max_sequence, Some(delete.sequence));
    assert!(update.sequence.0 > insert.sequence.0);
}

#[test]
fn sqlite_retention_gc_preserves_document_anchor_and_respects_pins() {
    let dir = tempdir().expect("temporary directory should create");
    let store = SqliteTenantStore::open(dir.path().join("tenant.sqlite3"))
        .expect("sqlite tenant store should open");
    let schema = ranked_tasks_schema();
    let index = schema.indexes[0].clone();
    store
        .replace_table_schema(&schema)
        .expect("schema should persist");
    let document = ranked_document(&schema.table, "v1", 1);
    let insert = store.insert(&document).expect("insert should succeed");
    let table_id = insert.writes[0].table_id.clone();
    let patch_v2 = serde_json::Map::from_iter([
        ("title".to_string(), json!("v2")),
        ("rank".to_string(), json!(2)),
    ]);
    let update_v2 = store
        .update(&document.table, &document.id, &patch_v2)
        .expect("first update should succeed");
    let patch_v3 = serde_json::Map::from_iter([
        ("title".to_string(), json!("v3")),
        ("rank".to_string(), json!(3)),
    ]);
    let update_v3 = store
        .update(&document.table, &document.id, &patch_v3)
        .expect("second update should succeed");
    let delete = store
        .delete(&document.table, &document.id)
        .expect("delete should succeed");

    let pin = store.pin_retention_participant(
        RetentionParticipant::TransactionSession,
        update_v2.sequence,
        Some(table_id.clone()),
        "repeatable-read transaction",
    );
    let pinned_health = store
        .storage_health_diagnostic()
        .expect("health diagnostic should load");
    assert_eq!(pinned_health.retention_pins.len(), 1);
    assert_eq!(
        pinned_health
            .retention_gc
            .document_versions
            .active_pin_count,
        1
    );

    let pinned_summary = store
        .compact_retained_versions(RetentionGcConfig::new(1).expect("config should build"))
        .expect("pinned compaction should succeed");
    assert_eq!(
        pinned_summary
            .watermarks
            .document_versions
            .safe_prune_before,
        update_v2.sequence
    );
    assert_eq!(pinned_summary.document_versions_pruned, 1);
    assert_eq!(pinned_summary.index_versions_pruned, 1);
    let at_pin = store
        .get_document_version_at(&document.table, &table_id, &document.id, update_v2.sequence)
        .expect("pinned version should load")
        .expect("pinned version should remain");
    assert_eq!(at_pin.fields.get("title"), Some(&json!("v2")));

    drop(pin);
    let released_summary = store
        .compact_retained_versions(RetentionGcConfig::new(1).expect("config should build"))
        .expect("released compaction should succeed");
    assert_eq!(
        released_summary
            .watermarks
            .document_versions
            .safe_prune_before,
        SequenceNumber(delete.sequence.0.saturating_sub(1))
    );
    assert_eq!(released_summary.document_versions_pruned, 1);
    assert_eq!(released_summary.index_versions_pruned, 1);
    assert_eq!(
        store
            .load_retention_checkpoint()
            .expect("retention floors should load after version compaction")
            .1,
        crate::RetentionReadFloors::new(update_v3.sequence, update_v3.sequence, SequenceNumber(0))
    );
    assert_eq!(
        store.retention_floor().published_read_floors(),
        crate::RetentionReadFloors::new(update_v3.sequence, update_v3.sequence, SequenceNumber(0))
    );
    let at_floor = store
        .get_document_version_at(&document.table, &table_id, &document.id, update_v3.sequence)
        .expect("floor version should load")
        .expect("floor anchor should remain");
    assert_eq!(at_floor.fields.get("title"), Some(&json!("v3")));
    let intervals = store
        .index_version_intervals_for_testing(&table_id, &index.id)
        .expect("index versions should load after GC");
    assert_eq!(intervals.len(), 1);
    assert_eq!(intervals[0].visible_from, update_v3.sequence);
    assert_eq!(intervals[0].visible_until, Some(delete.sequence));
}

#[test]
fn sqlite_document_versions_reject_unknown_future_storage_format() {
    let dir = tempdir().expect("temporary directory should create");
    let store = SqliteTenantStore::open(dir.path().join("tenant.sqlite3"))
        .expect("sqlite tenant store should open");
    let document = sample_document("versioned_format_tasks", "v1");
    let insert = store.insert(&document).expect("insert should succeed");
    let table_id = insert.writes[0].table_id.clone();
    let future_format = u64::from(crate::CURRENT_DOCUMENT_VERSION_STORAGE_FORMAT.0) + 1;

    store
        .execute_write(|transaction| {
            transaction.put_metadata(
                crate::DOCUMENT_VERSION_STORAGE_FORMAT_METADATA_KEY,
                future_format.to_be_bytes().as_slice(),
            )
        })
        .expect("format marker should update");

    let err = store
        .get_document_version_at(&document.table, &table_id, &document.id, insert.sequence)
        .expect_err("future document-version format must fail closed");
    assert!(
        err.to_string()
            .contains("unknown future document-version storage format version")
    );
}

#[test]
fn sqlite_index_versions_track_update_delete_visibility_intervals() {
    let dir = tempdir().expect("temporary directory should create");
    let store = SqliteTenantStore::open(dir.path().join("tenant.sqlite3"))
        .expect("sqlite tenant store should open");
    let schema = ranked_tasks_schema();
    let index = schema.indexes[0].clone();
    store
        .replace_table_schema(&schema)
        .expect("schema should persist");
    let document = ranked_document(&schema.table, "v1", 1);
    let insert = store.insert(&document).expect("insert should succeed");
    let table_id = insert.writes[0].table_id.clone();
    let patch = serde_json::Map::from_iter([
        ("title".to_string(), json!("v2")),
        ("rank".to_string(), json!(2)),
    ]);
    let update = store
        .update(&document.table, &document.id, &patch)
        .expect("update should succeed");
    let delete = store
        .delete(&document.table, &document.id)
        .expect("delete should succeed");

    let intervals = store
        .index_version_intervals_for_testing(&table_id, &index.id)
        .expect("index versions should load");

    assert_eq!(intervals.len(), 2);
    assert!(
        intervals
            .iter()
            .all(|interval| interval.document_id == document.id)
    );
    assert_eq!(intervals[0].visible_from, insert.sequence);
    assert_eq!(intervals[0].visible_until, Some(update.sequence));
    assert_eq!(intervals[1].visible_from, update.sequence);
    assert_eq!(intervals[1].visible_until, Some(delete.sequence));
}

#[test]
fn sqlite_index_versions_are_materialized_during_durable_recovery() {
    let dir = tempdir().expect("temporary directory should create");
    let store = SqliteTenantStore::open(dir.path().join("tenant.sqlite3"))
        .expect("sqlite tenant store should open");
    let schema = ranked_tasks_schema();
    let index = schema.indexes[0].clone();
    store
        .replace_table_schema(&schema)
        .expect("schema should persist");
    let table_id = sqlite_active_table_id(&store, &schema.table);
    let inserted = ranked_document(&schema.table, "v1", 1);
    let mut updated = inserted.clone();
    updated.fields.insert("title".to_string(), json!("v2"));
    updated.fields.insert("rank".to_string(), json!(2));
    updated.update_time = Timestamp(updated.update_time.0.saturating_add(1));
    let records = vec![
        sqlite_durable_write_record(
            SequenceNumber(2),
            Timestamp(100),
            &schema.table,
            &table_id,
            WriteOpType::Insert,
            inserted.id.clone(),
            None,
            Some(inserted.clone()),
        ),
        sqlite_durable_write_record(
            SequenceNumber(3),
            Timestamp(101),
            &schema.table,
            &table_id,
            WriteOpType::Update,
            inserted.id.clone(),
            Some(inserted.clone()),
            Some(updated.clone()),
        ),
        sqlite_durable_write_record(
            SequenceNumber(4),
            Timestamp(102),
            &schema.table,
            &table_id,
            WriteOpType::Delete,
            inserted.id.clone(),
            Some(updated),
            None,
        ),
    ];

    store
        .append_durable_records_batch(&records)
        .expect("durable append should succeed");
    assert!(
        store
            .index_version_intervals_for_testing(&table_id, &index.id)
            .expect("unapplied index versions should load")
            .is_empty(),
        "durable-only records must not materialize index versions before recovery"
    );

    store
        .recover_durable_journal()
        .expect("durable recovery should succeed");

    let intervals = store
        .index_version_intervals_for_testing(&table_id, &index.id)
        .expect("index versions should load after recovery");
    assert_eq!(intervals.len(), 2);
    assert_eq!(intervals[0].visible_from, SequenceNumber(2));
    assert_eq!(intervals[0].visible_until, Some(SequenceNumber(3)));
    assert_eq!(intervals[1].visible_from, SequenceNumber(3));
    assert_eq!(intervals[1].visible_until, Some(SequenceNumber(4)));
}

#[test]
fn sqlite_index_versions_reject_unknown_future_storage_format() {
    let dir = tempdir().expect("temporary directory should create");
    let store = SqliteTenantStore::open(dir.path().join("tenant.sqlite3"))
        .expect("sqlite tenant store should open");
    let schema = ranked_tasks_schema();
    let index = schema.indexes[0].clone();
    store
        .replace_table_schema(&schema)
        .expect("schema should persist");
    let document = ranked_document(&schema.table, "v1", 1);
    let insert = store.insert(&document).expect("insert should succeed");
    let table_id = insert.writes[0].table_id.clone();
    let future_format = u64::from(crate::CURRENT_INDEX_VERSION_STORAGE_FORMAT.0) + 1;

    store
        .execute_write(|transaction| {
            transaction.put_metadata(
                crate::INDEX_VERSION_STORAGE_FORMAT_METADATA_KEY,
                future_format.to_be_bytes().as_slice(),
            )
        })
        .expect("format marker should update");

    let err = store
        .index_version_intervals_for_testing(&table_id, &index.id)
        .expect_err("future index-version format must fail closed");
    assert!(
        err.to_string()
            .contains("unknown future index-version storage format version")
    );
}

#[test]
fn sqlite_historical_index_scan_eq_and_range_use_versioned_visibility() {
    let dir = tempdir().expect("temporary directory should create");
    let store = SqliteTenantStore::open(dir.path().join("tenant.sqlite3"))
        .expect("sqlite tenant store should open");
    let schema = ranked_tasks_schema();
    store
        .replace_table_schema(&schema)
        .expect("schema should persist");
    let document = ranked_document(&schema.table, "v1", 1);
    let insert = store.insert(&document).expect("insert should succeed");
    let table_id = insert.writes[0].table_id.clone();
    let patch = serde_json::Map::from_iter([
        ("title".to_string(), json!("v2")),
        ("rank".to_string(), json!(2)),
    ]);
    let update = store
        .update(&document.table, &document.id, &patch)
        .expect("update should succeed");
    let delete = store
        .delete(&document.table, &document.id)
        .expect("delete should succeed");
    let snapshot = store.read_snapshot().expect("snapshot should open");

    let at_insert =
        sqlite_historical_read_shape(&schema.table, &table_id, &schema, insert.sequence);
    let rank_one = snapshot
        .historical_index_scan_eq_cancellable(&at_insert, "by_rank", &json!(1), &mut || Ok(()))
        .expect("historical rank=1 scan should succeed");
    assert_eq!(sqlite_document_titles(&rank_one), vec!["v1"]);
    assert_eq!(
        sqlite_document_title_strings(&rank_one),
        sqlite_rank_full_scan_oracle_titles(
            &snapshot,
            &schema.table,
            &table_id,
            &[&document],
            insert.sequence,
            1
        )
    );
    assert!(
        snapshot
            .historical_index_scan_eq_cancellable(&at_insert, "by_rank", &json!(2), &mut || Ok(()))
            .expect("historical rank=2 scan should succeed")
            .is_empty()
    );

    let at_update =
        sqlite_historical_read_shape(&schema.table, &table_id, &schema, update.sequence);
    let rank_two = snapshot
        .historical_index_scan_range_cancellable(
            &at_update,
            "by_rank",
            Bound::Included(&json!(2)),
            Bound::Included(&json!(2)),
            &mut || Ok(()),
        )
        .expect("historical rank range scan should succeed");
    assert_eq!(sqlite_document_titles(&rank_two), vec!["v2"]);
    assert_eq!(
        sqlite_document_title_strings(&rank_two),
        sqlite_rank_full_scan_oracle_titles(
            &snapshot,
            &schema.table,
            &table_id,
            &[&document],
            update.sequence,
            2
        )
    );
    assert!(
        snapshot
            .historical_index_scan_eq_cancellable(&at_update, "by_rank", &json!(1), &mut || Ok(()))
            .expect("historical stale rank scan should succeed")
            .is_empty()
    );

    let at_delete =
        sqlite_historical_read_shape(&schema.table, &table_id, &schema, delete.sequence);
    let deleted_rank_two = snapshot
        .historical_index_scan_eq_cancellable(&at_delete, "by_rank", &json!(2), &mut || Ok(()))
        .expect("historical deleted rank scan should succeed");
    assert_eq!(
        sqlite_document_title_strings(&deleted_rank_two),
        sqlite_rank_full_scan_oracle_titles(
            &snapshot,
            &schema.table,
            &table_id,
            &[&document],
            delete.sequence,
            2
        )
    );
}

#[test]
fn sqlite_historical_index_prefix_composite_range_and_pagination_are_stable() {
    let dir = tempdir().expect("temporary directory should create");
    let store = SqliteTenantStore::open(dir.path().join("tenant.sqlite3"))
        .expect("sqlite tenant store should open");
    let schema = sqlite_status_rank_schema();
    store
        .replace_table_schema(&schema)
        .expect("schema should persist");
    let first = sqlite_status_rank_document(&schema.table, "first", "open", 1);
    let second = sqlite_status_rank_document(&schema.table, "second", "open", 2);
    let third = sqlite_status_rank_document(&schema.table, "third", "closed", 3);
    let first_insert = store.insert(&first).expect("first insert should succeed");
    let table_id = first_insert.writes[0].table_id.clone();
    store.insert(&second).expect("second insert should succeed");
    let third_insert = store.insert(&third).expect("third insert should succeed");

    let read_shape =
        sqlite_historical_read_shape(&schema.table, &table_id, &schema, third_insert.sequence);
    let snapshot = store.read_snapshot().expect("snapshot should open");
    let open_docs = snapshot
        .historical_index_scan_prefix_cancellable(
            &read_shape,
            "by_status_rank",
            &[json!("open")],
            &mut || Ok(()),
        )
        .expect("historical prefix scan should succeed");
    assert_eq!(sqlite_document_titles(&open_docs), vec!["first", "second"]);
    assert_eq!(
        sqlite_document_title_strings(&open_docs),
        sqlite_status_rank_full_scan_oracle_titles(
            &snapshot,
            &table_id,
            &[&first, &second, &third],
            third_insert.sequence,
            "open",
            None,
            None
        )
    );

    let exact_rank_two = snapshot
        .historical_index_scan_composite_range_cancellable(
            &read_shape,
            "by_status_rank",
            &[json!("open")],
            Bound::Included(&json!(2)),
            Bound::Included(&json!(2)),
            &mut || Ok(()),
        )
        .expect("historical composite range scan should succeed");
    assert_eq!(sqlite_document_titles(&exact_rank_two), vec!["second"]);
    assert_eq!(
        sqlite_document_title_strings(&exact_rank_two),
        sqlite_status_rank_full_scan_oracle_titles(
            &snapshot,
            &table_id,
            &[&first, &second, &third],
            third_insert.sequence,
            "open",
            Some(2),
            Some(2)
        )
    );

    let first_page = snapshot
        .historical_index_scan_prefix_page_cancellable(
            &read_shape,
            "by_status_rank",
            &[json!("open")],
            None,
            1,
            &mut || Ok(()),
        )
        .expect("first historical page should succeed");
    assert_eq!(sqlite_document_titles(&first_page.documents), vec!["first"]);
    let cursor = first_page
        .next_cursor
        .as_ref()
        .expect("first page should return a cursor");
    let second_page = snapshot
        .historical_index_scan_prefix_page_cancellable(
            &read_shape,
            "by_status_rank",
            &[json!("open")],
            Some(cursor),
            1,
            &mut || Ok(()),
        )
        .expect("second historical page should succeed");
    assert_eq!(
        sqlite_document_titles(&second_page.documents),
        vec!["second"]
    );

    let mismatch = snapshot
        .historical_index_scan_prefix_page_cancellable(
            &read_shape,
            "by_status_rank",
            &[json!("closed")],
            Some(cursor),
            1,
            &mut || Ok(()),
        )
        .expect_err("cursor from a different prefix must fail closed");
    assert_eq!(
        mismatch.historical_read_kind(),
        Some(nimbus_core::HistoricalReadErrorKind::CursorMismatch)
    );
}

#[test]
fn sqlite_durable_journal_batch_append_enforces_no_holes() {
    let dir = tempdir().expect("temporary directory should create");
    let store = SqliteTenantStore::open(dir.path().join("tenant.sqlite3"))
        .expect("sqlite tenant store should open");
    let table_id = TableId::new();
    let first = TenantEventRecord::new(
        SequenceNumber(1),
        Timestamp(10),
        vec![WriteOp {
            table: TableName::new("tasks").expect("table name should be valid"),
            table_id: table_id.clone(),
            op_type: WriteOpType::Insert,
            doc_id: DocumentId::new(),
            resource_path_binding: None,
            trigger_write_origin: None,
            previous: None,
            current: Some(sample_document("tasks", "First")),
        }],
        None,
    )
    .expect("first durable record should build");
    let second = TenantEventRecord::new(
        SequenceNumber(2),
        Timestamp(11),
        vec![WriteOp {
            table: TableName::new("tasks").expect("table name should be valid"),
            table_id: table_id.clone(),
            op_type: WriteOpType::Insert,
            doc_id: DocumentId::new(),
            resource_path_binding: None,
            trigger_write_origin: None,
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
        .append_durable_records_batch(&[TenantEventRecord::new(
            SequenceNumber(4),
            Timestamp(12),
            vec![WriteOp {
                table: TableName::new("tasks").expect("table name should be valid"),
                table_id: table_id.clone(),
                op_type: WriteOpType::Insert,
                doc_id: DocumentId::new(),
                resource_path_binding: None,
                trigger_write_origin: None,
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
fn sqlite_replica_journal_reconciliation_accepts_identical_overlap_and_missing_suffix() {
    let dir = tempdir().expect("temporary directory should create");
    let store = SqliteTenantStore::open(dir.path().join("tenant.sqlite3"))
        .expect("sqlite tenant store should open");
    let table_id = TableId::new();
    let first = TenantEventRecord::new(
        SequenceNumber(1),
        Timestamp(10),
        vec![WriteOp {
            table: TableName::new("tasks").expect("table name should be valid"),
            table_id: table_id.clone(),
            op_type: WriteOpType::Insert,
            doc_id: DocumentId::new(),
            resource_path_binding: None,
            trigger_write_origin: None,
            previous: None,
            current: Some(sample_document("tasks", "First")),
        }],
        None,
    )
    .expect("first durable record should build");
    let second = TenantEventRecord::new(
        SequenceNumber(2),
        Timestamp(11),
        vec![WriteOp {
            table: TableName::new("tasks").expect("table name should be valid"),
            table_id,
            op_type: WriteOpType::Insert,
            doc_id: DocumentId::new(),
            resource_path_binding: None,
            trigger_write_origin: None,
            previous: None,
            current: Some(sample_document("tasks", "Second")),
        }],
        None,
    )
    .expect("second durable record should build");

    store
        .append_durable_records_batch(std::slice::from_ref(&first))
        .expect("the competing refresh should append the shared prefix");
    store
        .reconcile_replica_durable_records_batch(&[first.clone(), second.clone()])
        .expect("an identical overlapping prefix plus missing suffix should reconcile");
    store
        .reconcile_replica_durable_records_batch(&[first.clone(), second.clone()])
        .expect("a fully overlapping identical replay should be idempotent");

    assert_eq!(
        store
            .read_durable_journal_from(SequenceNumber(1))
            .expect("reconciled journal should read"),
        vec![first.clone(), second]
    );
    assert_eq!(
        store
            .journal_progress()
            .expect("reconciled journal progress should read"),
        crate::store::JournalProgress {
            durable_head: SequenceNumber(2),
            applied_head: SequenceNumber(0),
        }
    );

    let divergent =
        TenantEventRecord::new(SequenceNumber(1), Timestamp(99), first.writes.clone(), None)
            .expect("divergent replay record should build");
    let error = store
        .reconcile_replica_durable_records_batch(&[divergent])
        .expect_err("different-content sequence reuse must fail closed");
    assert_eq!(
        error.storage_kind(),
        Some(nimbus_core::StorageErrorKind::Corruption)
    );
    assert!(error.to_string().contains("already-applied sequence 1"));
    assert_eq!(
        store
            .latest_sequence()
            .expect("failed reconciliation must retain the durable head"),
        SequenceNumber(2)
    );
}

fn sqlite_active_table_id(store: &SqliteTenantStore, table: &TableName) -> TableId {
    store
        .read_snapshot()
        .expect("snapshot should open")
        .table_identities()
        .expect("table identities should load")
        .into_iter()
        .find(|identity| {
            identity.table == *table
                && identity.namespace == crate::table_identity::DEFAULT_TABLE_NAMESPACE
                && identity.state == nimbus_core::TableState::Active
        })
        .expect("active table identity should exist")
        .table_id
}

fn sqlite_status_rank_schema() -> TableSchema {
    let table = TableName::new("composite_tasks").expect("table name should be valid");
    TableSchema {
        table,
        fields: vec![
            FieldSchema {
                name: "status".to_string(),
                field_type: FieldType::String,
                required: true,
            },
            FieldSchema {
                name: "rank".to_string(),
                field_type: FieldType::Number,
                required: true,
            },
        ],
        indexes: vec![IndexDefinition {
            id: nimbus_core::IndexId::new(),
            state: nimbus_core::IndexState::Enabled,
            name: "by_status_rank".to_string(),
            fields: vec!["status".to_string(), "rank".to_string()],
        }],
        access_policy: None,
    }
}

fn sqlite_status_rank_document(
    table: &TableName,
    title: &str,
    status: &str,
    rank: u64,
) -> Document {
    Document::new(
        table.clone(),
        serde_json::Map::from_iter([
            ("title".to_string(), json!(title)),
            ("status".to_string(), json!(status)),
            ("rank".to_string(), json!(rank)),
        ]),
    )
}

fn sqlite_rank_full_scan_oracle_titles(
    snapshot: &crate::sqlite::SqliteReadSnapshot,
    table: &TableName,
    table_id: &TableId,
    corpus: &[&Document],
    sequence: SequenceNumber,
    rank: u64,
) -> Vec<String> {
    let mut titles = corpus
        .iter()
        .filter_map(|document| {
            snapshot
                .get_document_version_at(table, table_id, &document.id, sequence)
                .expect("document version oracle should load")
        })
        .filter(|document| {
            document.fields.get("rank").and_then(|value| value.as_u64()) == Some(rank)
        })
        .map(|document| sqlite_document_title_string(&document))
        .collect::<Vec<_>>();
    titles.sort();
    titles
}

fn sqlite_status_rank_full_scan_oracle_titles(
    snapshot: &crate::sqlite::SqliteReadSnapshot,
    table_id: &TableId,
    corpus: &[&Document],
    sequence: SequenceNumber,
    status: &str,
    start_rank: Option<u64>,
    end_rank: Option<u64>,
) -> Vec<String> {
    let mut rows = corpus
        .iter()
        .filter_map(|document| {
            snapshot
                .get_document_version_at(&document.table, table_id, &document.id, sequence)
                .expect("document version oracle should load")
        })
        .filter_map(|document| {
            let document_status = document.fields.get("status")?.as_str()?;
            let rank = document.fields.get("rank")?.as_u64()?;
            if document_status == status
                && start_rank.is_none_or(|start| rank >= start)
                && end_rank.is_none_or(|end| rank <= end)
            {
                Some((rank, sqlite_document_title_string(&document)))
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    rows.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
    rows.into_iter().map(|(_, title)| title).collect()
}

fn sqlite_historical_read_shape(
    table: &TableName,
    table_id: &TableId,
    schema: &TableSchema,
    sequence: SequenceNumber,
) -> nimbus_core::HistoricalReadShape {
    let registry =
        nimbus_core::VersionedRegistry::from_records([TenantEventRecord::schema_change(
            SequenceNumber(1),
            Timestamp(100),
            SchemaChangeEvent::SetTable {
                table: table.clone(),
                table_id: table_id.clone(),
                previous: None,
                current: schema.clone(),
            },
        )
        .expect("schema change event should build")])
        .expect("registry should build");
    registry
        .read_shape_at(table, sqlite_historical_snapshot(sequence))
        .expect("read shape should load")
        .expect("table should exist at historical read")
}

fn sqlite_historical_snapshot(sequence: SequenceNumber) -> nimbus_core::HistoricalReadSnapshot {
    let timestamp = Timestamp(sequence.0.saturating_mul(100));
    nimbus_core::HistoricalReadSnapshot::new(
        nimbus_core::ReadTimestamp::new(timestamp),
        nimbus_core::CommitSequence::new(sequence),
        nimbus_core::CommitTimestamp::new(timestamp),
    )
}

fn sqlite_document_titles(documents: &[Document]) -> Vec<&str> {
    documents
        .iter()
        .map(|document| {
            document
                .fields
                .get("title")
                .and_then(|value| value.as_str())
                .expect("document should have a string title")
        })
        .collect()
}

fn sqlite_document_title_strings(documents: &[Document]) -> Vec<String> {
    documents.iter().map(sqlite_document_title_string).collect()
}

fn sqlite_document_title_string(document: &Document) -> String {
    document
        .fields
        .get("title")
        .and_then(|value| value.as_str())
        .expect("document should have a string title")
        .to_string()
}

// Test-only helper mirroring `WriteOp` field-by-field; call sites pass
// distinctly-typed newtypes positionally, so a wrapper struct would only add
// call-site ceremony without reducing risk of mixups.
#[allow(clippy::too_many_arguments)]
fn sqlite_durable_write_record(
    sequence: SequenceNumber,
    timestamp: Timestamp,
    table: &TableName,
    table_id: &TableId,
    op_type: WriteOpType,
    doc_id: DocumentId,
    previous: Option<Document>,
    current: Option<Document>,
) -> TenantEventRecord {
    TenantEventRecord::new(
        sequence,
        timestamp,
        vec![WriteOp {
            table: table.clone(),
            table_id: table_id.clone(),
            op_type,
            doc_id,
            resource_path_binding: None,
            trigger_write_origin: None,
            previous,
            current,
        }],
        None,
    )
    .expect("durable record should build")
}

#[test]
fn sqlite_recovery_replays_durable_but_unapplied_records() {
    let dir = tempdir().expect("temporary directory should create");
    let store = SqliteTenantStore::open(dir.path().join("tenant.sqlite3"))
        .expect("sqlite tenant store should open");
    let first = sample_document("tasks", "First");
    let second = sample_document("tasks", "Second");
    let table_id = TableId::new();
    let records = vec![
        TenantEventRecord::new(
            SequenceNumber(1),
            Timestamp(100),
            vec![WriteOp {
                table: first.table.clone(),
                table_id: table_id.clone(),
                op_type: WriteOpType::Insert,
                doc_id: first.id.clone(),
                resource_path_binding: None,
                trigger_write_origin: None,
                previous: None,
                current: Some(first.clone()),
            }],
            None,
        )
        .expect("first durable record should build"),
        TenantEventRecord::new(
            SequenceNumber(2),
            Timestamp(101),
            vec![WriteOp {
                table: second.table.clone(),
                table_id: table_id.clone(),
                op_type: WriteOpType::Insert,
                doc_id: second.id.clone(),
                resource_path_binding: None,
                trigger_write_origin: None,
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
fn sqlite_tenant_event_journal_replays_mixed_history() {
    let dir = tempdir().expect("temporary directory should create");
    let store = SqliteTenantStore::open(dir.path().join("tenant.sqlite3"))
        .expect("sqlite tenant store should open");
    let table_schema = ranked_tasks_schema();
    let table = table_schema.table.clone();
    let table_id = TableId::new();
    let document = ranked_document(&table, "First", 1);
    let record_schema = TenantEventRecord::from_events(
        SequenceNumber(1),
        Timestamp(10),
        vec![
            TenantEventKind::SchemaChange {
                change: Box::new(SchemaChangeEvent::SetTable {
                    table: table.clone(),
                    table_id: table_id.clone(),
                    previous: None,
                    current: table_schema.clone(),
                }),
            },
            TenantEventKind::IndexLifecycle {
                index: IndexLifecycleEvent {
                    table: table.clone(),
                    table_id: table_id.clone(),
                    index_id: table_schema.indexes[0].id.clone(),
                    state: table_schema.indexes[0].state,
                    definition: table_schema.indexes[0].clone(),
                },
            },
        ],
    )
    .expect("schema tenant event should build");
    let record_document = TenantEventRecord::new(
        SequenceNumber(2),
        Timestamp(11),
        vec![WriteOp {
            table: table.clone(),
            table_id: table_id.clone(),
            op_type: WriteOpType::Insert,
            doc_id: document.id.clone(),
            resource_path_binding: None,
            trigger_write_origin: None,
            previous: None,
            current: Some(document.clone()),
        }],
        None,
    )
    .expect("document tenant event should build");
    let record_trigger = TenantEventRecord::trigger_delivery(
        SequenceNumber(3),
        Timestamp(12),
        TriggerDeliveryCursor::new(SequenceNumber(2)),
    )
    .expect("trigger tenant event should build");

    store
        .append_durable_records_batch(&[record_schema, record_document, record_trigger])
        .expect("mixed tenant events should append");
    let progress = store
        .recover_durable_journal()
        .expect("mixed tenant events should recover");

    assert_eq!(progress.applied_head, SequenceNumber(3));
    assert_eq!(
        store.load_schema().expect("schema should replay"),
        Schema {
            tables: std::collections::HashMap::from_iter([(table.clone(), table_schema.clone())]),
        }
    );
    assert_eq!(
        store
            .scan_table(&table)
            .expect("documents should replay through tenant event"),
        vec![document]
    );
    assert_eq!(
        store
            .index_scan_eq(&table, "by_rank", &json!(1))
            .expect("index should replay"),
        store.scan_table(&table).expect("scan should replay")
    );
    assert_eq!(
        store
            .trigger_delivery_cursor()
            .expect("trigger cursor should replay"),
        TriggerDeliveryCursor::new(SequenceNumber(2))
    );
}

#[test]
fn sqlite_durable_replay_retires_recreated_table_identity() {
    let dir = tempdir().expect("temporary directory should create");
    let store = SqliteTenantStore::open(dir.path().join("tenant.sqlite3"))
        .expect("sqlite tenant store should open");
    let table = TableName::new("tasks_replayed_lifecycle").expect("table should parse");
    let old_table_id = TableId::new();
    let new_table_id = TableId::new();
    let old_document = nimbus_core::Document::new(
        table.clone(),
        serde_json::Map::from_iter([("title".to_string(), json!("old"))]),
    );
    let new_document = nimbus_core::Document::new(
        table.clone(),
        serde_json::Map::from_iter([("title".to_string(), json!("new"))]),
    );
    let records = vec![
        TenantEventRecord::new(
            SequenceNumber(1),
            Timestamp(1),
            vec![WriteOp {
                table: table.clone(),
                table_id: old_table_id.clone(),
                op_type: WriteOpType::Insert,
                doc_id: old_document.id.clone(),
                resource_path_binding: None,
                trigger_write_origin: None,
                previous: None,
                current: Some(old_document.clone()),
            }],
            None,
        )
        .expect("old durable record should build"),
        TenantEventRecord::new(
            SequenceNumber(2),
            Timestamp(2),
            vec![WriteOp {
                table: table.clone(),
                table_id: new_table_id.clone(),
                op_type: WriteOpType::Insert,
                doc_id: new_document.id.clone(),
                resource_path_binding: None,
                trigger_write_origin: None,
                previous: None,
                current: Some(new_document.clone()),
            }],
            None,
        )
        .expect("new durable record should build"),
    ];

    store
        .apply_durable_records_batch(&records)
        .expect("durable replay should infer table recreation");

    assert_eq!(
        store
            .table_id(&table)
            .expect("active table id should resolve"),
        Some(new_table_id.clone())
    );
    assert!(
        store
            .get(&table, &old_document.id)
            .expect("logical get should use active replacement")
            .is_none()
    );
    assert_eq!(
        store
            .scan_table(&table)
            .expect("scan should use active replacement"),
        vec![new_document]
    );
    let identities = store
        .read_snapshot()
        .expect("snapshot should open")
        .table_identities()
        .expect("table identities should export");
    assert!(identities.iter().any(|identity| {
        identity.namespace == crate::table_identity::DEFAULT_TABLE_NAMESPACE
            && identity.table == table
            && identity.table_id == new_table_id
            && identity.state == nimbus_core::TableState::Active
    }));
    assert!(identities.iter().any(|identity| {
        identity.namespace == crate::table_identity::deleting_table_namespace(&old_table_id)
            && identity.table == table
            && identity.table_id == old_table_id
            && identity.state == nimbus_core::TableState::Deleting
    }));
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
    assert_eq!(
        error.historical_read_kind(),
        Some(nimbus_core::HistoricalReadErrorKind::RetentionExpired)
    );
    assert!(error.to_string().contains("behind the retention floor 1"));

    let page = store
        .stream_durable_journal(SequenceNumber(1), 10)
        .expect("cursor at the retained floor should succeed");
    assert_eq!(page.cursor_floor, SequenceNumber(1));
    assert_eq!(page.latest_sequence, SequenceNumber(2));
    assert_eq!(page.next_cursor, SequenceNumber(2));
    assert_eq!(page.records.len(), 1);
    assert_eq!(page.records[0].sequence, SequenceNumber(2));
}

#[test]
fn sqlite_durable_journal_page_rejects_concurrent_prune_after_rows_are_read() {
    let dir = tempdir().expect("temporary directory should create");
    let (fault, rows_read_rx, resume_tx) = pause_after_retention_read_page();
    let store = Arc::new(
        SqliteTenantStore::open_with_simulation(
            dir.path().join("concurrent-retention-page.sqlite3"),
            Arc::new(nimbus_core::SystemWallClock),
            fault,
        )
        .expect("SQLite store should open"),
    );
    for title in ["first", "second", "third"] {
        store
            .insert(&sample_document("sqlite_retention_page", title))
            .expect("insert should succeed");
    }

    let reader_store = Arc::clone(&store);
    let reader =
        std::thread::spawn(move || reader_store.stream_durable_journal(SequenceNumber(0), 1));
    rows_read_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("SQLite reader should reach the post-page boundary");
    let compact_result = store.compact_retained_history(
        RetentionGcConfig::new(1).expect("retention config should be valid"),
    );
    resume_tx
        .send(())
        .expect("SQLite reader should still wait at the page boundary");
    compact_result.expect("concurrent SQLite retention should commit");

    let error = reader
        .join()
        .expect("SQLite reader should not panic")
        .expect_err("a concurrent SQLite prune must invalidate the page");
    assert_eq!(
        error.historical_read_kind(),
        Some(nimbus_core::HistoricalReadErrorKind::RetentionExpired)
    );
}

#[test]
fn sqlite_changefeed_stream_reports_retention_expired_after_journal_floor_cut() {
    let dir = tempdir().expect("temporary directory should create");
    let path = dir.path().join("tenant.sqlite3");
    let store = SqliteTenantStore::open(&path).expect("sqlite tenant store should open");
    let bootstrap = store
        .export_changefeed_bootstrap()
        .expect("changefeed bootstrap should export");
    assert_eq!(bootstrap.cursor.after, SequenceNumber(0));

    let first = sample_document("tasks", "first");
    let second = sample_document("tasks", "second");
    store.insert(&first).expect("first insert should succeed");
    store.insert(&second).expect("second insert should succeed");

    rusqlite::Connection::open(&path)
        .expect("raw sqlite connection should open")
        .execute("DELETE FROM commit_log WHERE sequence = 1", [])
        .expect("first journal row should delete");

    let error = store
        .stream_changefeed(&bootstrap.cursor, 10)
        .expect_err("changefeed cursor behind floor should fail");
    assert!(matches!(
        error,
        Error::HistoricalRead {
            kind: nimbus_core::HistoricalReadErrorKind::RetentionExpired,
            ..
        }
    ));
}

#[test]
// Shares the sqlite write-observation serial group: these open their own
// stores and would otherwise add write load to the concurrency probes.
#[serial_test::serial(sqlite_write_observation)]
fn sqlite_journal_progress_round_trips_through_insert_update_delete() {
    let dir = tempdir().expect("temporary directory should create");
    let store = SqliteTenantStore::open(dir.path().join("tenant.sqlite3"))
        .expect("sqlite tenant store should open");
    crate::tests::contract_scenarios::exercise_journal_progress_round_trip(
        &store,
        "sqlite_progress_tasks",
    );
}

#[test]
// Shares the sqlite write-observation serial group: these open their own
// stores and would otherwise add write load to the concurrency probes.
#[serial_test::serial(sqlite_write_observation)]
fn sqlite_materialized_position_matches_the_provider_independent_reference() {
    let dir = tempdir().expect("temporary directory should create");
    let store = SqliteTenantStore::open(dir.path().join("tenant.sqlite3"))
        .expect("sqlite tenant store should open");
    assert_eq!(
        crate::tests::contract_scenarios::exercise_materialized_position_is_provider_independent(
            &store
        ),
        crate::tests::contract_scenarios::reference_materialized_position()
    );
}

#[test]
#[serial_test::serial(sqlite_write_observation)]
fn sqlite_materialized_verification_root_is_provider_independent() {
    let dir = tempdir().expect("temporary directory should create");
    let store = SqliteTenantStore::open(dir.path().join("root.sqlite3"))
        .expect("sqlite tenant store should open");
    assert_eq!(
        crate::tests::contract_scenarios::exercise_materialized_verification_root_is_provider_independent(
            &store
        ),
        crate::tests::contract_scenarios::reference_materialized_verification_root()
    );
}
