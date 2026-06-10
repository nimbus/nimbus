use std::ops::Bound;
use std::time::{Duration as StdDuration, Instant};

use super::*;
use crate::RetentionGcConfig;
use nimbus_core::{
    CommitSequence, CommitTimestamp, HistoricalReadErrorKind, HistoricalReadSnapshot,
    PinnedServingSnapshot, ReadTimestamp, ReadVisibility, RequiredSequence, TableState,
    VersionedRegistry,
};

#[test]
fn key_helpers_create_prefix_scannable_ranges() {
    let table_id = nimbus_core::TableId::new();
    let id = DocumentId::new();
    let key = document_key(&table_id, &id);
    let prefix = table_prefix(&table_id);
    let end = prefix_end(&prefix).expect("prefix end should exist");

    assert!(key.starts_with(&prefix));
    assert!(key.as_slice() < end.as_slice());
}

#[test]
fn insert_then_get_roundtrip() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let document = sample_document("tasks", "Hello");

    let commit = store.insert(&document).expect("insert should succeed");
    let fetched = store
        .get(&document.table, &document.id)
        .expect("get should succeed")
        .expect("document should exist");

    assert_eq!(commit.sequence, SequenceNumber(1));
    assert_eq!(fetched.fields.get("title"), Some(&json!("Hello")));
}

#[test]
fn redb_document_versions_track_insert_update_delete_history() {
    let store = TenantStore::create_in_memory().expect("store should open");
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
        .get_document_version_at(&table_id, &document.id, insert.sequence)
        .expect("insert version should load")
        .expect("insert version should exist");
    let at_update = store
        .get_document_version_at(&table_id, &document.id, update.sequence)
        .expect("update version should load")
        .expect("update version should exist");
    let at_delete = store
        .get_document_version_at(&table_id, &document.id, delete.sequence)
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
fn redb_document_versions_are_materialized_during_durable_recovery() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let table = TableName::new("versioned_replay_tasks").expect("table name should be valid");
    let table_id = TableId::new();
    let inserted = sample_document("versioned_replay_tasks", "v1");
    let mut updated = inserted.clone();
    updated.fields.insert("title".to_string(), json!("v2"));
    updated.update_time = Timestamp(updated.update_time.0.saturating_add(1));
    let records = vec![
        DurableMutationRecord::new(
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
        DurableMutationRecord::new(
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
        DurableMutationRecord::new(
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
            .get_document_version_at(&table_id, &inserted.id, SequenceNumber(3))
            .expect("unapplied version lookup should succeed")
            .is_none(),
        "durable-only records must not materialize historical versions before recovery"
    );

    store
        .recover_durable_journal()
        .expect("durable recovery should succeed");

    let at_insert = store
        .get_document_version_at(&table_id, &inserted.id, SequenceNumber(1))
        .expect("insert replay version should load")
        .expect("insert replay version should exist");
    let at_update = store
        .get_document_version_at(&table_id, &inserted.id, SequenceNumber(2))
        .expect("update replay version should load")
        .expect("update replay version should exist");
    let at_delete = store
        .get_document_version_at(&table_id, &inserted.id, SequenceNumber(3))
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
fn redb_document_versions_storage_diagnostic_reports_format_and_range() {
    let store = TenantStore::create_in_memory().expect("store should open");
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
fn redb_retention_gc_preserves_document_anchor_and_respects_pins() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let table = TableName::new("retained_indexed_tasks").expect("table should be valid");
    let (schema, index) = ranked_schema(&table);
    store
        .replace_table_schema(&schema)
        .expect("schema should persist");
    let document = ranked_document(&table, "v1", 1);
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
        .get_document_version_at(&table_id, &document.id, update_v2.sequence)
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
    let at_floor = store
        .get_document_version_at(&table_id, &document.id, update_v3.sequence)
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
fn redb_document_versions_reject_unknown_future_storage_format() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let document = sample_document("versioned_format_tasks", "v1");
    let insert = store.insert(&document).expect("insert should succeed");
    let table_id = insert.writes[0].table_id.clone();
    let future_format = u64::from(crate::CURRENT_DOCUMENT_VERSION_STORAGE_FORMAT.0) + 1;

    let write_txn = store.db.begin_write().expect("metadata write should start");
    {
        let mut metadata = write_txn
            .open_table(crate::store::METADATA)
            .expect("metadata table should open");
        metadata
            .insert(
                crate::DOCUMENT_VERSION_STORAGE_FORMAT_METADATA_KEY,
                future_format.to_be_bytes().as_slice(),
            )
            .expect("format marker should update");
    }
    write_txn.commit().expect("metadata write should commit");

    let err = store
        .get_document_version_at(&table_id, &document.id, insert.sequence)
        .expect_err("future document-version format must fail closed");
    assert!(
        err.to_string()
            .contains("unknown future document-version storage format version")
    );
}

#[test]
fn redb_index_versions_track_update_delete_visibility_intervals() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let table = TableName::new("indexed_versioned_tasks").expect("table should be valid");
    let (schema, index) = ranked_schema(&table);
    store
        .replace_table_schema(&schema)
        .expect("schema should persist");
    let document = ranked_document(&table, "v1", 1);
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
fn redb_index_versions_are_materialized_during_durable_recovery() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let table = TableName::new("indexed_versioned_replay_tasks").expect("table should be valid");
    let (schema, index) = ranked_schema(&table);
    store
        .replace_table_schema(&schema)
        .expect("schema should persist");
    let table_id = active_table_id(&store, &table);
    let inserted = ranked_document(&table, "v1", 1);
    let mut updated = inserted.clone();
    updated.fields.insert("title".to_string(), json!("v2"));
    updated.fields.insert("rank".to_string(), json!(2));
    updated.update_time = Timestamp(updated.update_time.0.saturating_add(1));
    let records = vec![
        durable_write_record(
            SequenceNumber(2),
            Timestamp(100),
            &table,
            &table_id,
            WriteOpType::Insert,
            inserted.id.clone(),
            None,
            Some(inserted.clone()),
        ),
        durable_write_record(
            SequenceNumber(3),
            Timestamp(101),
            &table,
            &table_id,
            WriteOpType::Update,
            inserted.id.clone(),
            Some(inserted.clone()),
            Some(updated.clone()),
        ),
        durable_write_record(
            SequenceNumber(4),
            Timestamp(102),
            &table,
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
fn redb_index_versions_reject_unknown_future_storage_format() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let table = TableName::new("indexed_format_tasks").expect("table should be valid");
    let (schema, index) = ranked_schema(&table);
    store
        .replace_table_schema(&schema)
        .expect("schema should persist");
    let document = ranked_document(&table, "v1", 1);
    let insert = store.insert(&document).expect("insert should succeed");
    let table_id = insert.writes[0].table_id.clone();
    let future_format = u64::from(crate::CURRENT_INDEX_VERSION_STORAGE_FORMAT.0) + 1;

    let write_txn = store.db.begin_write().expect("metadata write should start");
    {
        let mut metadata = write_txn
            .open_table(crate::store::METADATA)
            .expect("metadata table should open");
        metadata
            .insert(
                crate::INDEX_VERSION_STORAGE_FORMAT_METADATA_KEY,
                future_format.to_be_bytes().as_slice(),
            )
            .expect("format marker should update");
    }
    write_txn.commit().expect("metadata write should commit");

    let err = store
        .index_version_intervals_for_testing(&table_id, &index.id)
        .expect_err("future index-version format must fail closed");
    assert!(
        err.to_string()
            .contains("unknown future index-version storage format version")
    );
}

#[test]
fn redb_historical_index_scan_eq_and_range_use_versioned_visibility() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let table = TableName::new("historical_indexed_tasks").expect("table should be valid");
    let (schema, _) = ranked_schema(&table);
    store
        .replace_table_schema(&schema)
        .expect("schema should persist");
    let document = ranked_document(&table, "v1", 1);
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

    let at_insert = historical_read_shape(&table, &table_id, &schema, insert.sequence);
    let rank_one = snapshot
        .historical_index_scan_eq_cancellable(&at_insert, "by_rank", &json!(1), &mut || Ok(()))
        .expect("historical rank=1 scan should succeed");
    assert_eq!(document_titles(&rank_one), vec!["v1"]);
    assert_eq!(
        document_title_strings(&rank_one),
        redb_rank_full_scan_oracle_titles(&snapshot, &table_id, &[&document], insert.sequence, 1)
    );
    assert!(
        snapshot
            .historical_index_scan_eq_cancellable(&at_insert, "by_rank", &json!(2), &mut || Ok(()))
            .expect("historical rank=2 scan should succeed")
            .is_empty()
    );

    let at_update = historical_read_shape(&table, &table_id, &schema, update.sequence);
    let rank_two = snapshot
        .historical_index_scan_range_cancellable(
            &at_update,
            "by_rank",
            Bound::Included(&json!(2)),
            Bound::Included(&json!(2)),
            &mut || Ok(()),
        )
        .expect("historical rank range scan should succeed");
    assert_eq!(document_titles(&rank_two), vec!["v2"]);
    assert_eq!(
        document_title_strings(&rank_two),
        redb_rank_full_scan_oracle_titles(&snapshot, &table_id, &[&document], update.sequence, 2)
    );
    assert!(
        snapshot
            .historical_index_scan_eq_cancellable(&at_update, "by_rank", &json!(1), &mut || Ok(()))
            .expect("historical stale rank scan should succeed")
            .is_empty()
    );

    let at_delete = historical_read_shape(&table, &table_id, &schema, delete.sequence);
    let deleted_rank_two = snapshot
        .historical_index_scan_eq_cancellable(&at_delete, "by_rank", &json!(2), &mut || Ok(()))
        .expect("historical deleted rank scan should succeed");
    assert_eq!(
        document_title_strings(&deleted_rank_two),
        redb_rank_full_scan_oracle_titles(&snapshot, &table_id, &[&document], delete.sequence, 2)
    );
}

#[test]
fn redb_historical_index_prefix_composite_range_and_pagination_are_stable() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let table = TableName::new("historical_composite_tasks").expect("table should be valid");
    let (schema, _) = status_rank_schema(&table);
    store
        .replace_table_schema(&schema)
        .expect("schema should persist");
    let first = status_rank_document(&table, "first", "open", 1);
    let second = status_rank_document(&table, "second", "open", 2);
    let third = status_rank_document(&table, "third", "closed", 3);
    let first_insert = store.insert(&first).expect("first insert should succeed");
    let table_id = first_insert.writes[0].table_id.clone();
    store.insert(&second).expect("second insert should succeed");
    let third_insert = store.insert(&third).expect("third insert should succeed");

    let read_shape = historical_read_shape(&table, &table_id, &schema, third_insert.sequence);
    let snapshot = store.read_snapshot().expect("snapshot should open");
    let open_docs = snapshot
        .historical_index_scan_prefix_cancellable(
            &read_shape,
            "by_status_rank",
            &[json!("open")],
            &mut || Ok(()),
        )
        .expect("historical prefix scan should succeed");
    assert_eq!(document_titles(&open_docs), vec!["first", "second"]);
    assert_eq!(
        document_title_strings(&open_docs),
        redb_status_rank_full_scan_oracle_titles(
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
    assert_eq!(document_titles(&exact_rank_two), vec!["second"]);
    assert_eq!(
        document_title_strings(&exact_rank_two),
        redb_status_rank_full_scan_oracle_titles(
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
    assert_eq!(document_titles(&first_page.documents), vec!["first"]);
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
    assert_eq!(document_titles(&second_page.documents), vec!["second"]);

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
        Some(HistoricalReadErrorKind::CursorMismatch)
    );
}

#[test]
fn redb_storage_engine_quality_performance_budget_covers_latest_historical_cdc_pitr_and_gc() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let changefeed_bootstrap = store
        .export_changefeed_bootstrap()
        .expect("changefeed bootstrap should export");
    let table = TableName::new("seq13_performance_tasks").expect("table should be valid");
    let (schema, _) = ranked_schema(&table);
    store
        .replace_table_schema(&schema)
        .expect("schema should persist");

    let mut documents = Vec::new();
    for rank in 0..64_u64 {
        let document = ranked_document(&table, format!("task-{rank}").as_str(), rank);
        let commit = store.insert(&document).expect("insert should commit");
        documents.push((document, commit.sequence));
    }
    let table_id = store
        .table_id(&table)
        .expect("table id lookup should succeed")
        .expect("table id should exist");

    for (offset, (document, _)) in documents.iter().take(16).enumerate() {
        let patch = serde_json::Map::from_iter([
            ("title".to_string(), json!(format!("task-{offset}-updated"))),
            ("rank".to_string(), json!(1000_u64 + offset as u64)),
        ]);
        store
            .update(&table, &document.id, &patch)
            .expect("update should commit");
    }
    let final_sequence = store
        .latest_sequence()
        .expect("latest sequence should load after seed");

    let started = Instant::now();
    for (document, _) in &documents {
        let _ = store
            .get(&table, &document.id)
            .expect("latest point read should succeed");
    }
    assert_seq13_budget("latest point reads", started.elapsed(), 200);

    let started = Instant::now();
    for (document, insert_sequence) in &documents {
        let version = store
            .get_document_version_at(&table_id, &document.id, *insert_sequence)
            .expect("historical point read should succeed");
        assert!(version.is_some());
    }
    assert_seq13_budget("historical point reads", started.elapsed(), 300);

    let read_shape = historical_read_shape(&table, &table_id, &schema, final_sequence);
    let snapshot = store.read_snapshot().expect("read snapshot should open");
    let started = Instant::now();
    let mut cursor = None;
    let mut historical_index_rows = 0_usize;
    loop {
        let mut check_cancel = || Ok(());
        let page = snapshot
            .historical_index_scan_range_page_cancellable(
                &read_shape,
                "by_rank",
                Bound::Included(&json!(0)),
                Bound::Included(&json!(2000)),
                crate::index::history_scan::HistoricalIndexPageRequest {
                    after: cursor.as_ref(),
                    limit: 8,
                    check_cancel: &mut check_cancel,
                },
            )
            .expect("historical index page should read");
        historical_index_rows += page.documents.len();
        cursor = page.next_cursor;
        if cursor.is_none() {
            break;
        }
    }
    assert_eq!(historical_index_rows, documents.len());
    assert_seq13_budget("historical index pagination", started.elapsed(), 500);

    let started = Instant::now();
    let mut cursor = changefeed_bootstrap.cursor;
    let mut cdc_events = 0_usize;
    loop {
        let page = store
            .stream_changefeed(&cursor, 16)
            .expect("changefeed should stream");
        cdc_events += page.events.len();
        cursor = page.next_cursor;
        if !page.has_more && cursor.after.0 >= page.latest_sequence.0 {
            break;
        }
    }
    assert!(cdc_events >= documents.len());
    assert_seq13_budget("CDC stream", started.elapsed(), 300);

    let started = Instant::now();
    let archive = store
        .export_point_in_time_restore_archive(
            crate::PointInTimeRestoreTarget::Sequence(final_sequence),
            RetentionGcConfig::retain_all(),
        )
        .expect("PITR archive should export");
    let restored = TenantStore::create_in_memory().expect("restore store should open");
    restored
        .import_point_in_time_restore_archive(&archive)
        .expect("PITR archive should import");
    assert_seq13_budget("PITR export/import", started.elapsed(), 1_000);

    let started = Instant::now();
    let summary = store
        .compact_retained_versions(RetentionGcConfig::new(8).expect("retention config parses"))
        .expect("retention compaction should run");
    assert!(summary.total_pruned() > 0);
    assert_seq13_budget("retention compaction", started.elapsed(), 500);

    let diagnostic = store
        .storage_health_diagnostic()
        .expect("storage diagnostic should load");
    let total_write_commits = 64_u64 + 16_u64;
    assert!(
        diagnostic.document_versions.version_count <= total_write_commits,
        "document-version write amplification should stay bounded by one row per document write"
    );
    assert!(
        diagnostic.index_versions.version_count <= total_write_commits.saturating_mul(2),
        "index-version write amplification should stay bounded by close/open rows per indexed write"
    );
}

#[test]
fn redb_table_identity_diagnostics_are_read_only_and_count_documents() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let first = sample_document("diagnostic_tasks", "first");
    let second = sample_document("diagnostic_tasks", "second");
    store.insert(&first).expect("first insert should succeed");
    store.insert(&second).expect("second insert should succeed");

    let diagnostics = store
        .table_identity_diagnostics()
        .expect("diagnostics should load");

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].table_name, first.table);
    assert_eq!(diagnostics[0].state, nimbus_core::TableState::Active);
    assert_eq!(
        diagnostics[0].backend_layout,
        crate::TableBackendLayout::RedbKeyspaceByTableId
    );
    assert_eq!(diagnostics[0].document_count, Some(2));
    assert_eq!(
        diagnostics[0].summary_status,
        crate::TableSummaryStatus::ExactDocumentCount
    );
}

fn ranked_schema(table: &TableName) -> (TableSchema, IndexDefinition) {
    let index = IndexDefinition {
        id: nimbus_core::IndexId::new(),
        state: nimbus_core::IndexState::Enabled,
        name: "by_rank".to_string(),
        fields: vec!["rank".to_string()],
    };
    (
        TableSchema {
            table: table.clone(),
            fields: vec![FieldSchema {
                name: "rank".to_string(),
                field_type: FieldType::Number,
                required: true,
            }],
            indexes: vec![index.clone()],
            access_policy: None,
        },
        index,
    )
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

fn status_rank_schema(table: &TableName) -> (TableSchema, IndexDefinition) {
    let index = IndexDefinition {
        id: nimbus_core::IndexId::new(),
        state: nimbus_core::IndexState::Enabled,
        name: "by_status_rank".to_string(),
        fields: vec!["status".to_string(), "rank".to_string()],
    };
    (
        TableSchema {
            table: table.clone(),
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
            indexes: vec![index.clone()],
            access_policy: None,
        },
        index,
    )
}

fn status_rank_document(table: &TableName, title: &str, status: &str, rank: u64) -> Document {
    Document::new(
        table.clone(),
        serde_json::Map::from_iter([
            ("title".to_string(), json!(title)),
            ("status".to_string(), json!(status)),
            ("rank".to_string(), json!(rank)),
        ]),
    )
}

fn redb_rank_full_scan_oracle_titles(
    snapshot: &crate::store::TenantReadSnapshot,
    table_id: &TableId,
    corpus: &[&Document],
    sequence: SequenceNumber,
    rank: u64,
) -> Vec<String> {
    let mut titles = corpus
        .iter()
        .filter_map(|document| {
            snapshot
                .get_document_version_at(table_id, &document.id, sequence)
                .expect("document version oracle should load")
        })
        .filter(|document| {
            document.fields.get("rank").and_then(|value| value.as_u64()) == Some(rank)
        })
        .map(|document| document_title_string(&document))
        .collect::<Vec<_>>();
    titles.sort();
    titles
}

fn redb_status_rank_full_scan_oracle_titles(
    snapshot: &crate::store::TenantReadSnapshot,
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
                .get_document_version_at(table_id, &document.id, sequence)
                .expect("document version oracle should load")
        })
        .filter_map(|document| {
            let document_status = document.fields.get("status")?.as_str()?;
            let rank = document.fields.get("rank")?.as_u64()?;
            if document_status == status
                && start_rank.is_none_or(|start| rank >= start)
                && end_rank.is_none_or(|end| rank <= end)
            {
                Some((rank, document_title_string(&document)))
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    rows.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
    rows.into_iter().map(|(_, title)| title).collect()
}

fn document_title_strings(documents: &[Document]) -> Vec<String> {
    documents.iter().map(document_title_string).collect()
}

fn document_title_string(document: &Document) -> String {
    document
        .fields
        .get("title")
        .and_then(|value| value.as_str())
        .expect("document should have a string title")
        .to_string()
}

fn assert_seq13_budget(label: &str, elapsed: StdDuration, budget_ms: u64) {
    let budget = StdDuration::from_millis(budget_ms);
    println!("seq13 performance budget: {label}: {elapsed:?} <= {budget:?}");
    assert!(
        elapsed <= budget,
        "SEQ13 {label} exceeded budget: {elapsed:?} > {budget:?}"
    );
}

fn historical_read_shape(
    table: &TableName,
    table_id: &TableId,
    schema: &TableSchema,
    sequence: SequenceNumber,
) -> nimbus_core::HistoricalReadShape {
    let registry = VersionedRegistry::from_records([TenantEventRecord::schema_change(
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
        .read_shape_at(table, historical_snapshot(sequence))
        .expect("read shape should load")
        .expect("table should exist at historical read")
}

fn historical_snapshot(sequence: SequenceNumber) -> HistoricalReadSnapshot {
    let timestamp = Timestamp(sequence.0.saturating_mul(100));
    HistoricalReadSnapshot::new(
        ReadTimestamp::new(timestamp),
        CommitSequence::new(sequence),
        CommitTimestamp::new(timestamp),
    )
}

fn document_titles(documents: &[Document]) -> Vec<&str> {
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

fn active_table_id(store: &TenantStore, table: &TableName) -> TableId {
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

#[allow(clippy::too_many_arguments)]
fn durable_write_record(
    sequence: SequenceNumber,
    timestamp: Timestamp,
    table: &TableName,
    table_id: &TableId,
    op_type: WriteOpType,
    doc_id: DocumentId,
    previous: Option<Document>,
    current: Option<Document>,
) -> DurableMutationRecord {
    DurableMutationRecord::new(
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
fn read_visibility_waits_for_required_sequence() {
    let latest = SequenceNumber(9);
    let required =
        ReadVisibility::AtLeast(RequiredSequence::new(SequenceNumber(7))).required_sequence(latest);
    assert_eq!(required.sequence(), SequenceNumber(7));

    let pinned = ReadVisibility::Pinned(PinnedServingSnapshot::new(SequenceNumber(5)))
        .required_sequence(latest);
    assert_eq!(pinned.sequence(), SequenceNumber(5));
    assert_eq!(
        ReadVisibility::Latest.required_sequence(latest).sequence(),
        latest
    );
}

#[test]
fn uses_shared_table_lifecycle_transition() {
    let next = crate::table_identity::apply_table_lifecycle_transition(
        None,
        crate::table_identity::TableLifecycleTransition::StageHidden,
    )
    .expect("shared lifecycle transition should stage hidden state");
    assert_eq!(next, Some(TableState::Hidden));
}

#[test]
fn native_documents_and_indexes_are_physically_keyed_by_table_id() {
    use redb::ReadableTable;

    let store = TenantStore::create_in_memory().expect("store should open");
    let document = sample_document("tasks_physical_identity", "Hello");
    let schema = TableSchema {
        table: document.table.clone(),
        fields: Vec::new(),
        indexes: vec![IndexDefinition {
            id: nimbus_core::IndexId::new(),
            state: nimbus_core::IndexState::Enabled,
            name: "by_title".to_string(),
            fields: vec!["title".to_string()],
        }],
        access_policy: None,
    };

    store
        .replace_table_schema(&schema)
        .expect("schema should persist");
    let commit = store
        .insert_with_indexes(&document, &schema.indexes)
        .expect("insert should succeed");

    let read_txn = store.db.begin_read().expect("read transaction");
    let table_id = {
        let catalog = read_txn
            .open_table(crate::store::TABLE_CATALOG)
            .expect("table catalog should exist");
        let value = catalog
            .get("default\0tasks_physical_identity")
            .expect("catalog read should succeed")
            .expect("catalog row should exist")
            .value()
            .to_string();
        serde_json::from_str::<serde_json::Value>(&value)
            .expect("catalog value should decode")
            .get("table_id")
            .and_then(|value| value.as_str())
            .expect("catalog value should include table_id")
            .to_string()
    };
    let table_id = nimbus_core::TableId::try_from(table_id).expect("table id should parse");
    assert_eq!(
        commit.writes[0].table_id, table_id,
        "commit records should carry the durable table identity used by physical storage"
    );

    let documents = read_txn
        .open_table(crate::store::DOCUMENTS)
        .expect("documents table should exist");
    let table_id_key = document_key(&table_id, &document.id);
    assert!(
        documents
            .get(table_id_key.as_slice())
            .expect("table-id document lookup should succeed")
            .is_some(),
        "native document storage should be keyed by table_id"
    );

    let mut table_name_key = document.table.as_str().as_bytes().to_vec();
    table_name_key.push(0);
    table_name_key.extend_from_slice(document.id.as_str().as_bytes());
    assert!(
        documents
            .get(table_name_key.as_slice())
            .expect("old table-name document lookup should succeed")
            .is_none(),
        "native document storage must not keep using table_name as its physical key"
    );
    drop(documents);

    let indexes = read_txn
        .open_table(crate::store::INDEXES)
        .expect("indexes table should exist");
    let table_id_index_prefix = crate::index::table_index_prefix(&table_id);
    let mut table_name_index_prefix = document.table.as_str().as_bytes().to_vec();
    table_name_index_prefix.push(0);
    let mut saw_table_id_index = false;
    let mut saw_table_name_index = false;
    for item in indexes.iter().expect("index iteration should start") {
        let (key, _) = item.expect("index row should decode");
        saw_table_id_index |= key.value().starts_with(table_id_index_prefix.as_slice());
        saw_table_name_index |= key.value().starts_with(table_name_index_prefix.as_slice());
    }
    assert!(
        saw_table_id_index,
        "index entries should use table_id prefixes"
    );
    assert!(
        !saw_table_name_index,
        "index entries must not use table_name prefixes"
    );

    let fetched = store
        .get(&document.table, &document.id)
        .expect("logical get should succeed")
        .expect("logical document should exist");
    assert_eq!(fetched, document);
    assert_eq!(
        store
            .index_scan_eq(&document.table, "by_title", &json!("Hello"))
            .expect("logical index scan should succeed"),
        vec![document]
    );
}

#[test]
fn native_writes_reject_deleting_table_identity() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let table = TableName::new("tasks_deleting").expect("table should parse");
    let table_id = TableId::new();
    let catalog_key = format!("default\0{}", table.as_str());
    let catalog_value = serde_json::json!({
        "table_id": table_id.as_str(),
        "state": "deleting"
    })
    .to_string();
    let write_txn = store.db.begin_write().expect("write transaction");
    {
        let mut catalog = write_txn
            .open_table(crate::store::TABLE_CATALOG)
            .expect("table catalog should open");
        catalog
            .insert(catalog_key.as_str(), catalog_value.as_str())
            .expect("deleting catalog state should insert");
    }
    write_txn.commit().expect("catalog state should commit");

    let document = Document::new(
        table,
        serde_json::Map::from_iter([("title".to_string(), json!("blocked"))]),
    );
    let error = store
        .insert(&document)
        .expect_err("writes to deleting tables should fail");

    assert!(
        error.to_string().contains("deleting lifecycle state"),
        "deleting table rejection should be explicit: {error:?}"
    );
}

#[test]
fn native_table_lifecycle_activates_hidden_identity_and_hard_deletes_old_data() {
    use redb::ReadableTable;

    let store = TenantStore::create_in_memory().expect("store should open");
    let table = TableName::new("tasks_lifecycle").expect("table should parse");
    let schema = TableSchema {
        table: table.clone(),
        fields: Vec::new(),
        indexes: vec![IndexDefinition {
            id: nimbus_core::IndexId::new(),
            state: nimbus_core::IndexState::Enabled,
            name: "by_title".to_string(),
            fields: vec!["title".to_string()],
        }],
        access_policy: None,
    };
    store
        .replace_table_schema(&schema)
        .expect("schema should persist");
    let old_document = Document::new(
        table.clone(),
        serde_json::Map::from_iter([("title".to_string(), json!("old"))]),
    );
    let old_commit = store
        .insert_with_indexes(&old_document, &schema.indexes)
        .expect("old document should insert");
    let old_table_id = old_commit.writes[0].table_id.clone();
    let replacement_table_id = TableId::new();

    store
        .stage_hidden_table_identity(&table, &replacement_table_id)
        .expect("hidden replacement identity should stage");
    let staged = store
        .read_snapshot()
        .expect("snapshot should open")
        .table_identities()
        .expect("table identities should export");
    assert!(
        staged.iter().any(|identity| {
            identity.namespace
                == crate::table_identity::hidden_table_namespace(&replacement_table_id)
                && identity.table == table
                && identity.table_id == replacement_table_id
                && identity.state == nimbus_core::TableState::Hidden
        }),
        "hidden replacement identity should be visible in catalog snapshots: {staged:?}"
    );

    let retired = store
        .activate_hidden_table_identity(&table, &replacement_table_id)
        .expect("hidden identity should activate");
    assert_eq!(
        retired.as_ref(),
        Some(&old_table_id),
        "activating a hidden replacement should retire the previous active identity"
    );
    assert_eq!(
        store.table_id(&table).expect("table id should resolve"),
        Some(replacement_table_id.clone())
    );
    assert!(
        store
            .get(&table, &old_document.id)
            .expect("logical get should resolve against replacement identity")
            .is_none(),
        "old rows must not be reachable through the recreated logical table name"
    );

    let new_document = Document::new(
        table.clone(),
        serde_json::Map::from_iter([("title".to_string(), json!("new"))]),
    );
    let new_commit = store
        .insert_with_indexes(&new_document, &schema.indexes)
        .expect("new document should insert under replacement identity");
    assert_eq!(new_commit.writes[0].table_id, replacement_table_id);

    {
        let read_txn = store.db.begin_read().expect("read transaction");
        let documents = read_txn
            .open_table(crate::store::DOCUMENTS)
            .expect("documents table should open");
        assert!(
            documents
                .get(document_key(&old_table_id, &old_document.id).as_slice())
                .expect("old physical document lookup should succeed")
                .is_some(),
            "retired table data should remain until hard delete"
        );
    }

    assert!(
        store
            .hard_delete_table_identity(&old_table_id)
            .expect("hard delete should succeed"),
        "hard delete should report that it removed the retiring table"
    );

    let read_txn = store.db.begin_read().expect("read transaction");
    let documents = read_txn
        .open_table(crate::store::DOCUMENTS)
        .expect("documents table should open");
    assert!(
        documents
            .get(document_key(&old_table_id, &old_document.id).as_slice())
            .expect("old physical document lookup should succeed")
            .is_none(),
        "hard delete should remove retired table documents"
    );
    drop(documents);

    let indexes = read_txn
        .open_table(crate::store::INDEXES)
        .expect("indexes table should open");
    let old_index_prefix = crate::index::table_index_prefix(&old_table_id);
    for item in indexes.iter().expect("index iteration should start") {
        let (key, _) = item.expect("index row should decode");
        assert!(
            !key.value().starts_with(old_index_prefix.as_slice()),
            "hard delete should remove retired table index rows"
        );
    }

    let identities = store
        .read_snapshot()
        .expect("snapshot should open")
        .table_identities()
        .expect("table identities should export");
    assert!(
        !identities
            .iter()
            .any(|identity| identity.table_id == old_table_id),
        "hard delete should remove the retired catalog identity: {identities:?}"
    );
    assert_eq!(
        store
            .index_scan_eq(&table, "by_title", &json!("new"))
            .expect("active replacement index scan should succeed"),
        vec![new_document]
    );
}

#[test]
fn redb_durable_replay_retires_recreated_table_identity() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let table = TableName::new("tasks_replayed_lifecycle").expect("table should parse");
    let old_table_id = TableId::new();
    let new_table_id = TableId::new();
    let old_document = Document::new(
        table.clone(),
        serde_json::Map::from_iter([("title".to_string(), json!("old"))]),
    );
    let new_document = Document::new(
        table.clone(),
        serde_json::Map::from_iter([("title".to_string(), json!("new"))]),
    );
    let records = vec![
        DurableMutationRecord::new(
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
        DurableMutationRecord::new(
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
            .is_none(),
        "old-generation rows must not be visible through the recreated table name"
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
fn seeded_fault_injector_reproduces_the_same_schedule_for_the_same_seed() {
    let left = SeededFaultInjector::new(7, NonZeroU64::new(3).expect("period should be non-zero"));
    let right = SeededFaultInjector::new(7, NonZeroU64::new(3).expect("period should be non-zero"));

    let left_results = [
        FaultPoint::StorageCommitBeforeVisibility,
        FaultPoint::JournalAppendBeforeDurableFlush,
        FaultPoint::StorageCommitBeforeVisibility,
        FaultPoint::CheckpointPublishBeforeManifestUpdate,
        FaultPoint::StorageCommitBeforeVisibility,
        FaultPoint::CompactionStartBeforePublish,
    ]
    .into_iter()
    .map(|point| left.check(point).is_err())
    .collect::<Vec<_>>();
    let right_results = [
        FaultPoint::StorageCommitBeforeVisibility,
        FaultPoint::JournalAppendBeforeDurableFlush,
        FaultPoint::StorageCommitBeforeVisibility,
        FaultPoint::CheckpointPublishBeforeManifestUpdate,
        FaultPoint::StorageCommitBeforeVisibility,
        FaultPoint::CompactionStartBeforePublish,
    ]
    .into_iter()
    .map(|point| right.check(point).is_err())
    .collect::<Vec<_>>();

    assert_eq!(left_results, right_results);
}

#[test]
fn injected_fault_before_visibility_rolls_back_the_write_deterministically() {
    let harness = DeterministicHarness::scripted(
        "storage-before-visibility",
        10,
        Timestamp(10_000),
        [FaultOccurrence {
            point: FaultPoint::StorageCommitBeforeVisibility,
            visit: 1,
        }],
    );
    let store =
        TenantStore::create_in_memory_with_simulation(harness.clock(), harness.fault_injector())
            .expect("store should open with simulation seams");
    let document = sample_document("tasks", "Hello");

    let error = store
        .insert(&document)
        .expect_err("first insert should fail before visibility");
    assert!(
        matches!(error, Error::Internal(message) if message.contains("storage_commit_before_visibility"))
    );
    assert!(
        store
            .get(&document.table, &document.id)
            .expect("get should succeed after injected failure")
            .is_none()
    );
    assert_eq!(
        store
            .latest_sequence()
            .expect("latest sequence should remain unchanged"),
        SequenceNumber(0)
    );

    let commit = store
        .insert(&document)
        .expect("second insert should commit");
    assert_eq!(commit.timestamp, Timestamp(10_000));
    assert_eq!(harness.describe(), "storage-before-visibility (seed 10)");
}

#[test]
fn scheduled_execution_marker_deduplicates_insert_commit() {
    let store = TenantStore::create_in_memory().expect("store should open");
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
    assert_eq!(
        store.latest_sequence().expect("latest sequence"),
        SequenceNumber(1)
    );
    let tasks = store
        .scan_table(&TableName::new("tasks").expect("table name should be valid"))
        .expect("scan should succeed");
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].fields.get("title"), Some(&json!("Hello once")));
}

#[test]
fn commit_log_sequences_increment() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let first = sample_document("tasks", "First");
    let second = sample_document("tasks", "Second");

    let first_commit = store.insert(&first).expect("first insert should succeed");
    let second_commit = store.insert(&second).expect("second insert should succeed");
    let entries = store
        .read_commit_log_from(SequenceNumber(1))
        .expect("commit log read should succeed");

    assert_eq!(first_commit.sequence, SequenceNumber(1));
    assert_eq!(second_commit.sequence, SequenceNumber(2));
    assert_eq!(entries.len(), 2);
    assert_eq!(
        store.latest_sequence().expect("latest sequence"),
        SequenceNumber(2)
    );
}

#[test]
fn durable_journal_serialization_preserves_payload_and_metadata() {
    let table = TableName::new("tasks").expect("table name should be valid");
    let before = Document::new(
        table.clone(),
        serde_json::Map::from_iter([("title".to_string(), json!("Before"))]),
    );
    let mut after = before.clone();
    after.fields.insert("title".to_string(), json!("After"));

    let record = DurableMutationRecord::new(
        SequenceNumber(7),
        Timestamp(42),
        vec![WriteOp {
            table: table.clone(),
            table_id: TableId::new(),
            op_type: WriteOpType::Update,
            doc_id: before.id.clone(),
            resource_path_binding: None,
            trigger_write_origin: None,
            previous: Some(before.clone()),
            current: Some(after.clone()),
        }],
        Some("scheduled:job-7".to_string()),
    )
    .expect("durable record should build");

    let encoded =
        crate::commit_log::serialize_durable_record(&record).expect("record should serialize");
    let decoded =
        crate::commit_log::deserialize_durable_record(&encoded).expect("record should deserialize");

    assert_eq!(decoded, record);
    assert_eq!(decoded.writes[0].table, table);
    assert_eq!(decoded.writes[0].doc_id, before.id);
    assert_eq!(
        decoded.writes[0]
            .current
            .as_ref()
            .and_then(|document| document.fields.get("title")),
        Some(&json!("After"))
    );
    assert_eq!(
        decoded.scheduled_execution_id.as_deref(),
        Some("scheduled:job-7")
    );
}

#[test]
fn durable_journal_metadata_supports_dependency_intersection_checks() {
    let table = TableName::new("tasks").expect("table name should be valid");
    let before = Document::new(
        table.clone(),
        serde_json::Map::from_iter([
            ("rank".to_string(), json!(3)),
            ("status".to_string(), json!("open")),
        ]),
    );
    let mut after = before.clone();
    after.fields.insert("rank".to_string(), json!(8));

    let table_id = TableId::new();
    let record = DurableMutationRecord::new(
        SequenceNumber(3),
        Timestamp(12),
        vec![WriteOp {
            table: table.clone(),
            table_id: table_id.clone(),
            op_type: WriteOpType::Update,
            doc_id: before.id.clone(),
            resource_path_binding: None,
            trigger_write_origin: None,
            previous: Some(before.clone()),
            current: Some(after.clone()),
        }],
        None,
    )
    .expect("durable record should build");
    let mut document_dependency = DependencySet::default();
    document_dependency.record_document(&table, &table_id, before.id.clone());
    assert!(durable_record_intersects_dependency_set(
        &record,
        &document_dependency,
        &[],
        |_, _| Ok(None)
    ));

    let mut table_dependency = DependencySet::default();
    table_dependency.record_table(&table, &table_id);
    assert!(durable_record_intersects_dependency_set(
        &record,
        &table_dependency,
        &[],
        |_, _| Ok(None)
    ));

    let mut index_range_dependency = DependencySet::default();
    index_range_dependency.record_index_range(IndexRangeDependency {
        table: table.clone(),
        table_id: table_id.clone(),
        index_id: nimbus_core::IndexId::new(),
        index_name: "by_rank".to_string(),
        field: "rank".to_string(),
        start: Some(json!(5)),
        end: Some(json!(10)),
        start_inclusive: true,
        end_inclusive: true,
    });
    assert!(durable_record_intersects_dependency_set(
        &record,
        &index_range_dependency,
        &[],
        |_, _| Ok(None)
    ));

    let mut unrelated = DependencySet::default();
    unrelated.record_table(
        &TableName::new("users").expect("table name should be valid"),
        &TableId::new(),
    );
    assert!(!durable_record_intersects_dependency_set(
        &record,
        &unrelated,
        &[],
        |_, _| Ok(None)
    ));
}
