//! Scenario bodies shared by the three remote-provider test modules.
//!
//! Each function here is the provider-independent core of a behaviour that
//! every remote provider must implement identically. Providers keep their own
//! `#[tokio::test]` wrappers so the test names stay discoverable per provider,
//! and a wrapper is free to assert provider-specific effects after the shared
//! body returns -- the libSQL replica cache is the usual reason.

use std::ops::Bound;

use super::provider_support::{
    DocumentVersionOracle, HistoricalIndexScanOracle, StorageHealthProbe, document_title_strings,
    document_titles, historical_read_shape, indexed_rank_schema, rank_full_scan_oracle_titles,
    ranked_document, status_rank_document, status_rank_full_scan_oracle_titles, status_rank_schema,
};
use super::{
    Document, DocumentId, SequenceNumber, TableId, TableName, TenantEventRecord, Timestamp,
    WriteOp, WriteOpType,
};
use crate::sql::store_core::SqlStoreCore;
use crate::{DurableJournal, TenantPointRead};

/// Identifiers minted inside a shared scenario that a provider wrapper needs in
/// order to assert its own follow-on effects.
///
/// Only the libSQL wrapper has such an effect to assert -- it re-reads the
/// replayed versions out of the refreshed replica cache -- so with that feature
/// off nothing reads these fields.
#[cfg_attr(not(feature = "libsql"), allow(dead_code))]
pub(crate) struct DurableRecoveryFixture {
    pub(crate) table: TableName,
    pub(crate) table_id: TableId,
    pub(crate) document_id: DocumentId,
}

/// Durable-only records must stay invisible to historical reads until
/// `recover_durable_journal` replays them, after which every version in the
/// insert/update/delete chain resolves and the current row reflects the delete.
pub(crate) fn exercise_document_versions_are_materialized_during_durable_recovery<S>(
    store: &S,
) -> DurableRecoveryFixture
where
    S: DurableJournal + TenantPointRead + DocumentVersionOracle,
{
    let table = TableName::new("versioned_replay_tasks").expect("table name should be valid");
    let table_id = TableId::new();
    let inserted = super::sample_document("versioned_replay_tasks", "v1");
    let mut updated = inserted.clone();
    updated
        .fields
        .insert("title".to_string(), serde_json::json!("v2"));
    updated.update_time = Timestamp(updated.update_time.0.saturating_add(1));
    let records = vec![
        durable_document_record(
            SequenceNumber(1),
            Timestamp(100),
            &table,
            &table_id,
            WriteOpType::Insert,
            &inserted.id,
            None,
            Some(inserted.clone()),
        ),
        durable_document_record(
            SequenceNumber(2),
            Timestamp(101),
            &table,
            &table_id,
            WriteOpType::Update,
            &inserted.id,
            Some(inserted.clone()),
            Some(updated.clone()),
        ),
        durable_document_record(
            SequenceNumber(3),
            Timestamp(102),
            &table,
            &table_id,
            WriteOpType::Delete,
            &inserted.id,
            Some(updated.clone()),
            None,
        ),
    ];

    store
        .append_durable_records_batch(&records)
        .expect("durable append should succeed");
    assert!(
        store
            .document_version_at(&table, &table_id, &inserted.id, SequenceNumber(3))
            .expect("unapplied version lookup should succeed")
            .is_none(),
        "durable-only records must not materialize historical versions before recovery"
    );

    store
        .recover_durable_journal()
        .expect("durable recovery should succeed");

    let at_insert = store
        .document_version_at(&table, &table_id, &inserted.id, SequenceNumber(1))
        .expect("insert replay version should load")
        .expect("insert replay version should exist");
    let at_update = store
        .document_version_at(&table, &table_id, &inserted.id, SequenceNumber(2))
        .expect("update replay version should load")
        .expect("update replay version should exist");
    let at_delete = store
        .document_version_at(&table, &table_id, &inserted.id, SequenceNumber(3))
        .expect("delete replay version should load");

    assert_eq!(
        at_insert.fields.get("title"),
        Some(&serde_json::json!("v1"))
    );
    assert_eq!(
        at_update.fields.get("title"),
        Some(&serde_json::json!("v2"))
    );
    assert_eq!(at_delete, None);
    assert!(
        store
            .get(&table, &inserted.id)
            .expect("current row get should succeed")
            .is_none(),
        "replayed current row should still reflect latest delete"
    );

    DurableRecoveryFixture {
        table,
        table_id,
        document_id: inserted.id,
    }
}

/// An insert/update/delete chain leaves three document versions behind, and the
/// health diagnostic must report them under the current storage format with the
/// sequence range spanning exactly that chain.
pub(crate) fn exercise_document_versions_storage_diagnostic_reports_format_and_range<S>(store: &S)
where
    S: SqlStoreCore + StorageHealthProbe,
{
    let document = super::sample_document("versioned_diagnostic_tasks", "v1");
    let insert = store.insert(&document).expect("insert should succeed");
    let update = store
        .update_validated(
            &document.table,
            &document.id,
            &serde_json::Map::from_iter([("title".to_string(), serde_json::json!("v2"))]),
            |_, _| Ok(()),
        )
        .expect("update should succeed");
    let (delete, _) = store
        .delete_validated_returning_document(&document.table, &document.id, |_| Ok(()))
        .expect("delete should succeed");

    let health = store.health().expect("health diagnostic should load");

    assert_eq!(
        health.document_versions.format_version,
        Some(crate::CURRENT_DOCUMENT_VERSION_STORAGE_FORMAT)
    );
    assert_eq!(health.document_versions.version_count, 3);
    assert_eq!(health.document_versions.min_sequence, Some(insert.sequence));
    assert_eq!(health.document_versions.max_sequence, Some(delete.sequence));
    assert!(update.sequence.0 > insert.sequence.0);
}

/// Historical equality and range scans must see the index as of the requested
/// sequence: the pre-update value at the insert, the post-update value at the
/// update, and nothing at all once the row is deleted. Each indexed result is
/// cross-checked against a full-scan oracle so an index that agrees with itself
/// but not with the documents still fails.
pub(crate) fn exercise_historical_index_scan_eq_and_range_use_versioned_visibility<S>(store: &S)
where
    S: SqlStoreCore + DocumentVersionOracle + HistoricalIndexScanOracle,
{
    let table = TableName::new("historical_indexed_tasks").expect("table name should be valid");
    let (schema, _) = indexed_rank_schema(&table);
    store
        .replace_table_schema(&schema)
        .expect("schema should persist");
    let document = ranked_document(&table, "v1", 1);
    let insert = store.insert(&document).expect("insert should succeed");
    let table_id = insert.writes[0].table_id.clone();
    let update = store
        .update_validated(
            &document.table,
            &document.id,
            &serde_json::Map::from_iter([
                ("title".to_string(), serde_json::json!("v2")),
                ("rank".to_string(), serde_json::json!(2)),
            ]),
            |_, _| Ok(()),
        )
        .expect("update should succeed");
    let (delete, _) = store
        .delete_validated_returning_document(&document.table, &document.id, |_| Ok(()))
        .expect("delete should succeed");

    let at_insert = historical_read_shape(&table, &table_id, &schema, insert.sequence);
    let rank_one = store
        .scan_eq(&at_insert, "by_rank", &serde_json::json!(1))
        .expect("historical rank=1 scan should succeed");
    assert_eq!(document_titles(&rank_one), vec!["v1"]);
    assert_eq!(
        document_title_strings(&rank_one),
        rank_full_scan_oracle_titles(store, &table, &table_id, &[&document], insert.sequence, 1)
    );
    assert!(
        store
            .scan_eq(&at_insert, "by_rank", &serde_json::json!(2))
            .expect("historical rank=2 scan should succeed")
            .is_empty()
    );

    let at_update = historical_read_shape(&table, &table_id, &schema, update.sequence);
    let rank_two = store
        .scan_range(
            &at_update,
            "by_rank",
            Bound::Included(&serde_json::json!(2)),
            Bound::Included(&serde_json::json!(2)),
        )
        .expect("historical rank range scan should succeed");
    assert_eq!(document_titles(&rank_two), vec!["v2"]);
    assert_eq!(
        document_title_strings(&rank_two),
        rank_full_scan_oracle_titles(store, &table, &table_id, &[&document], update.sequence, 2)
    );

    let at_delete = historical_read_shape(&table, &table_id, &schema, delete.sequence);
    let deleted_rank_two = store
        .scan_eq(&at_delete, "by_rank", &serde_json::json!(2))
        .expect("historical deleted rank scan should succeed");
    assert_eq!(
        document_title_strings(&deleted_rank_two),
        rank_full_scan_oracle_titles(store, &table, &table_id, &[&document], delete.sequence, 2)
    );
}

/// Composite-index reads must stay stable across their three access shapes --
/// prefix, prefix plus a range on the trailing field, and paginated prefix --
/// and a cursor minted under one prefix must fail closed when replayed under
/// another rather than silently returning the wrong page.
pub(crate) fn exercise_historical_index_prefix_composite_range_and_pagination_are_stable<S>(
    store: &S,
) where
    S: SqlStoreCore + DocumentVersionOracle + HistoricalIndexScanOracle,
{
    let table = TableName::new("historical_composite_tasks").expect("table name should be valid");
    let schema = status_rank_schema(&table);
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
    let open_docs = store
        .scan_prefix(&read_shape, "by_status_rank", &[serde_json::json!("open")])
        .expect("historical prefix scan should succeed");
    assert_eq!(document_titles(&open_docs), vec!["first", "second"]);
    assert_eq!(
        document_title_strings(&open_docs),
        status_rank_full_scan_oracle_titles(
            store,
            &table_id,
            &[&first, &second, &third],
            third_insert.sequence,
            "open",
            None,
            None
        )
    );

    let exact_rank_two = store
        .scan_composite_range(
            &read_shape,
            "by_status_rank",
            &[serde_json::json!("open")],
            Bound::Included(&serde_json::json!(2)),
            Bound::Included(&serde_json::json!(2)),
        )
        .expect("historical composite range scan should succeed");
    assert_eq!(document_titles(&exact_rank_two), vec!["second"]);
    assert_eq!(
        document_title_strings(&exact_rank_two),
        status_rank_full_scan_oracle_titles(
            store,
            &table_id,
            &[&first, &second, &third],
            third_insert.sequence,
            "open",
            Some(2),
            Some(2)
        )
    );

    let first_page = store
        .scan_prefix_page(
            &read_shape,
            "by_status_rank",
            &[serde_json::json!("open")],
            None,
            1,
        )
        .expect("first historical page should succeed");
    assert_eq!(document_titles(&first_page.documents), vec!["first"]);
    let cursor = first_page
        .next_cursor
        .as_ref()
        .expect("first page should return a cursor");
    let second_page = store
        .scan_prefix_page(
            &read_shape,
            "by_status_rank",
            &[serde_json::json!("open")],
            Some(cursor),
            1,
        )
        .expect("second historical page should succeed");
    assert_eq!(document_titles(&second_page.documents), vec!["second"]);

    let mismatch = store
        .scan_prefix_page(
            &read_shape,
            "by_status_rank",
            &[serde_json::json!("closed")],
            Some(cursor),
            1,
        )
        .expect_err("cursor from a different prefix must fail closed");
    assert_eq!(
        mismatch.historical_read_kind(),
        Some(nimbus_core::HistoricalReadErrorKind::CursorMismatch)
    );
}

#[allow(clippy::too_many_arguments)]
fn durable_document_record(
    sequence: SequenceNumber,
    timestamp: Timestamp,
    table: &TableName,
    table_id: &TableId,
    op_type: WriteOpType,
    doc_id: &DocumentId,
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
            doc_id: doc_id.clone(),
            resource_path_binding: None,
            trigger_write_origin: None,
            previous,
            current,
        }],
        None,
    )
    .expect("durable record should build")
}
