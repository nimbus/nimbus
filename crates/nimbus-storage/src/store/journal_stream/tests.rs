use std::sync::Arc;
use std::time::Duration;

use nimbus_core::{SequenceNumber, SystemWallClock};
use serde_json::json;

use crate::tests::pause_after_retention_read_page;
use crate::tests::provider_support::{historical_read_shape, indexed_rank_schema, ranked_document};
use crate::{PointInTimeRestoreTarget, RetentionGcConfig, TenantStore};

fn sample_document(table: &str, title: &str) -> nimbus_core::Document {
    nimbus_core::Document::new(
        nimbus_core::TableName::new(table).expect("table name should be valid"),
        serde_json::Map::from_iter([("title".to_string(), json!(title))]),
    )
}

#[test]
fn durable_journal_stream_uses_cursor_floor_after_retention_cut() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let first = sample_document("tasks", "first");
    let second = sample_document("tasks", "second");
    store.insert(&first).expect("first insert should succeed");
    store.insert(&second).expect("second insert should succeed");

    let write_txn = store.db.begin_write().expect("write txn should open");
    {
        let mut journal = write_txn
            .open_table(super::super::COMMIT_LOG)
            .expect("commit log table should open");
        journal
            .remove(1)
            .expect("first durable journal entry should be removable");
    }
    store
        .commit_write_txn(write_txn)
        .expect("retention-cut transaction should commit");

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
fn durable_journal_page_rejects_concurrent_prune_after_rows_are_read() {
    let (fault, rows_read_rx, resume_tx) = pause_after_retention_read_page();
    let store = Arc::new(
        TenantStore::create_in_memory_with_simulation(Arc::new(SystemWallClock), fault)
            .expect("store should open"),
    );
    for title in ["first", "second", "third"] {
        store
            .insert(&sample_document("tasks", title))
            .expect("insert should succeed");
    }

    let reader_store = Arc::clone(&store);
    let reader =
        std::thread::spawn(move || reader_store.stream_durable_journal(SequenceNumber(0), 1));
    rows_read_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("journal reader should reach the post-page boundary");
    let compact_result = store.compact_retained_history(
        RetentionGcConfig::new(1).expect("retention config should be valid"),
    );
    resume_tx
        .send(())
        .expect("journal reader should still wait at the page boundary");
    compact_result.expect("concurrent retention should commit");

    let error = reader
        .join()
        .expect("journal reader should not panic")
        .expect_err("a concurrent prune after the page read must invalidate the page");
    assert_eq!(
        error.historical_read_kind(),
        Some(nimbus_core::HistoricalReadErrorKind::RetentionExpired)
    );
}

#[test]
fn point_in_time_archive_rejects_concurrent_prune_after_tail_read() {
    let (fault, rows_read_rx, resume_tx) = pause_after_retention_read_page();
    let store = Arc::new(
        TenantStore::create_in_memory_with_simulation(Arc::new(SystemWallClock), fault)
            .expect("store should open"),
    );
    for title in ["first", "second", "third"] {
        store
            .insert(&sample_document("pitr_tasks", title))
            .expect("insert should succeed");
    }

    let reader_store = Arc::clone(&store);
    let reader = std::thread::spawn(move || {
        reader_store.export_point_in_time_restore_archive(
            PointInTimeRestoreTarget::Sequence(SequenceNumber(3)),
            RetentionGcConfig::new(1).expect("retention config should be valid"),
        )
    });
    rows_read_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("PITR reader should reach the post-tail boundary");
    let compact_result = store.compact_retained_history(
        RetentionGcConfig::new(1).expect("retention config should be valid"),
    );
    resume_tx
        .send(())
        .expect("PITR reader should still wait at the page boundary");
    compact_result.expect("concurrent retention should commit");

    let error = reader
        .join()
        .expect("PITR reader should not panic")
        .expect_err("a concurrent prune after the tail read must invalidate the archive");
    assert_eq!(
        error.historical_read_kind(),
        Some(nimbus_core::HistoricalReadErrorKind::RetentionExpired)
    );
}

#[test]
fn historical_index_page_rejects_concurrent_prune_after_rows_are_read() {
    let (fault, rows_read_rx, resume_tx) = pause_after_retention_read_page();
    let store = Arc::new(
        TenantStore::create_in_memory_with_simulation(Arc::new(SystemWallClock), fault)
            .expect("store should open"),
    );
    let table =
        nimbus_core::TableName::new("retention_index_tasks").expect("table name should be valid");
    let (schema, _) = indexed_rank_schema(&table);
    store
        .replace_table_schema(&schema)
        .expect("schema should persist");
    let first = store
        .insert(&ranked_document(&table, "first", 1))
        .expect("first insert should succeed");
    store
        .insert(&ranked_document(&table, "second", 2))
        .expect("second insert should succeed");
    let third = store
        .insert(&ranked_document(&table, "third", 3))
        .expect("third insert should succeed");
    let table_id = first.writes[0].table_id.clone();
    let read_shape = historical_read_shape(&table, &table_id, &schema, first.sequence);
    let expired_read_shape = read_shape.clone();
    let retained_read_shape = historical_read_shape(&table, &table_id, &schema, third.sequence);

    let reader_store = Arc::clone(&store);
    let reader = std::thread::spawn(move || {
        reader_store
            .read_snapshot()?
            .historical_index_scan_eq_page_cancellable(
                &read_shape,
                "by_rank",
                &json!(1),
                None,
                1,
                &mut || Ok(()),
            )
    });
    rows_read_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("historical reader should reach the post-page boundary");
    let compact_result = store.compact_retained_history(
        RetentionGcConfig::new(1).expect("retention config should be valid"),
    );
    resume_tx
        .send(())
        .expect("historical reader should still wait at the page boundary");
    compact_result.expect("concurrent retention should commit");

    let error = reader
        .join()
        .expect("historical reader should not panic")
        .expect_err("a concurrent prune after the page read must invalidate the page");
    assert_eq!(
        error.historical_read_kind(),
        Some(nimbus_core::HistoricalReadErrorKind::RetentionExpired)
    );

    let expired_error = store
        .read_snapshot()
        .expect("snapshot should open")
        .historical_index_scan_eq_page_cancellable(
            &expired_read_shape,
            "by_rank",
            &json!(1),
            None,
            1,
            &mut || Ok(()),
        )
        .expect_err("a historical read below the published floor must fail before data returns");
    assert_eq!(
        expired_error.historical_read_kind(),
        Some(nimbus_core::HistoricalReadErrorKind::RetentionExpired)
    );

    let retained_page = store
        .read_snapshot()
        .expect("snapshot should open")
        .historical_index_scan_eq_page_cancellable(
            &retained_read_shape,
            "by_rank",
            &json!(3),
            None,
            1,
            &mut || Ok(()),
        )
        .expect("a historical read inside the retained window should succeed");
    assert_eq!(retained_page.documents.len(), 1);
}
