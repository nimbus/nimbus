//! Scenario bodies shared by the PostgreSQL and MySQL test modules.
//!
//! These two providers already share their whole write path through
//! `SqlStoreCore`, `SqlDurableJournalStore`, and `SqlHistoricalIndexStore`, so
//! the behaviours below were literally the same test written twice. The libSQL
//! replica provider does not run them -- it has no equivalent surface -- which
//! is why they live here rather than in `provider_scenarios`.

use std::ops::Bound;
use std::sync::atomic::Ordering;

use super::provider_support::{DocumentVersionOracle, indexed_rank_schema, ranked_document};
use crate::sql::store_core::SqlStoreCore;
use crate::{
    CommitterLeaseStore, DurableJournal, MaterializedRebuild, ResolvedWrite, ResourcePathScan,
    TenantPointRead, TenantRangeScan,
};
use nimbus_core::{
    CollectionName, Document, DocumentId, DocumentLocator, DocumentPath, Error, FieldSchema,
    FieldType, IndexDefinition, ResourcePathBinding, Result, SequenceNumber, TableId, TableName,
    TableSchema, TenantEventRecord, Timestamp, TriggerDeliveryCursor, WriteOp, WriteOpType,
};

/// Barrier-only journal records used to exercise batch append paths.
///
/// The label is opaque to the behaviour under test; both providers previously
/// spelled it with their own name.
fn pipeline_barriers(count: u64) -> Vec<TenantEventRecord> {
    (1..=count)
        .map(|sequence| {
            TenantEventRecord::barrier(
                SequenceNumber(sequence),
                Timestamp(sequence.saturating_mul(100)),
                format!("sql-pipeline-{sequence}"),
            )
            .expect("barrier record should build")
        })
        .collect()
}

fn binding(table: &str, id: &str, path: &[&str]) -> ResourcePathBinding {
    ResourcePathBinding::new(
        DocumentLocator::new(
            TableName::new(table).expect("table name should parse"),
            DocumentId::from_key(id).expect("document id should parse"),
        ),
        DocumentPath::from_segments(path.iter().copied()).expect("document path should parse"),
    )
}

/// The pair-only inherent surface these scenarios probe.
///
/// Each method below is inherent on both `PostgresTenantStore` and
/// `MySqlTenantStore` and backed by no shared trait, so a generic scenario
/// cannot reach it without this forwarding layer.
pub(crate) trait SqlPairProbe {
    fn write_pipeline_diagnostic(&self) -> crate::ProviderWritePipelineDiagnostic;

    /// Index-version intervals flattened to `(document, visible_from,
    /// visible_until)`. Each provider defines its own `IndexVersionInterval`
    /// struct, so the shared scenarios compare the fields rather than the type.
    fn index_version_intervals(
        &self,
        table_id: &TableId,
        index_id: &nimbus_core::IndexId,
    ) -> Result<Vec<(DocumentId, SequenceNumber, Option<SequenceNumber>)>>;

    fn locator_for_document_path(
        &self,
        document_path: &DocumentPath,
    ) -> Result<Option<DocumentLocator>>;

    fn resource_path_binding(
        &self,
        locator: &DocumentLocator,
    ) -> Result<Option<ResourcePathBinding>>;

    fn upsert_resource_path_binding(&self, binding: &ResourcePathBinding) -> Result<()>;

    fn trigger_delivery_cursor(&self) -> Result<TriggerDeliveryCursor>;

    fn set_trigger_delivery_cursor(&self, cursor: TriggerDeliveryCursor) -> Result<()>;
}

macro_rules! impl_sql_pair_probe {
    ($store:ty) => {
        impl SqlPairProbe for $store {
            fn write_pipeline_diagnostic(&self) -> crate::ProviderWritePipelineDiagnostic {
                <$store>::write_pipeline_diagnostic(self)
            }

            fn index_version_intervals(
                &self,
                table_id: &TableId,
                index_id: &nimbus_core::IndexId,
            ) -> Result<Vec<(DocumentId, SequenceNumber, Option<SequenceNumber>)>> {
                Ok(
                    <$store>::index_version_intervals_for_testing(self, table_id, index_id)?
                        .into_iter()
                        .map(|interval| {
                            (
                                interval.document_id,
                                interval.visible_from,
                                interval.visible_until,
                            )
                        })
                        .collect(),
                )
            }

            fn locator_for_document_path(
                &self,
                document_path: &DocumentPath,
            ) -> Result<Option<DocumentLocator>> {
                <$store>::locator_for_document_path(self, document_path)
            }

            fn resource_path_binding(
                &self,
                locator: &DocumentLocator,
            ) -> Result<Option<ResourcePathBinding>> {
                <$store>::resource_path_binding(self, locator)
            }

            fn upsert_resource_path_binding(&self, binding: &ResourcePathBinding) -> Result<()> {
                <$store>::upsert_resource_path_binding(self, binding)
            }

            fn trigger_delivery_cursor(&self) -> Result<TriggerDeliveryCursor> {
                <$store>::trigger_delivery_cursor(self)
            }

            fn set_trigger_delivery_cursor(&self, cursor: TriggerDeliveryCursor) -> Result<()> {
                <$store>::set_trigger_delivery_cursor(self, cursor)
            }
        }
    };
}

#[cfg(feature = "postgres")]
impl_sql_pair_probe!(crate::PostgresTenantStore);
#[cfg(feature = "mysql")]
impl_sql_pair_probe!(crate::MySqlTenantStore);

/// Everything a shared PostgreSQL/MySQL scenario needs from its store, bundled
/// so each scenario signature stays readable.
pub(crate) trait SqlPairStore:
    SqlStoreCore
    + DurableJournal
    + MaterializedRebuild
    + TenantPointRead
    + TenantRangeScan
    + ResourcePathScan
    + CommitterLeaseStore
    + DocumentVersionOracle
    + SqlPairProbe
{
}

impl<T> SqlPairStore for T where
    T: SqlStoreCore
        + DurableJournal
        + MaterializedRebuild
        + TenantPointRead
        + TenantRangeScan
        + ResourcePathScan
        + CommitterLeaseStore
        + DocumentVersionOracle
        + SqlPairProbe
{
}

/// Records appended durably but never applied stay invisible to reads until
/// `recover_durable_journal` replays them, after which journal progress and the
/// materialized rows agree.
pub(crate) fn exercise_durable_journal_recovery_applies_pending_records<S: SqlPairStore>(
    store: &S,
) {
    let first = crate::tests::sample_document("tasks", "First");
    let second = crate::tests::sample_document("tasks", "Second");
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
            Timestamp(200),
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

    SqlStoreCore::append_durable_records_batch(store, &records)
        .expect("durable append should succeed");
    assert_eq!(
        SqlStoreCore::journal_progress(store).expect("journal progress should read"),
        crate::store::JournalProgress {
            durable_head: SequenceNumber(2),
            applied_head: SequenceNumber(0),
        }
    );
    assert!(
        store
            .get(&first.table, &first.id)
            .expect("first lookup should succeed")
            .is_none()
    );

    let progress = SqlStoreCore::recover_durable_journal(store)
        .expect("recovery should apply pending durable records");
    assert_eq!(
        progress,
        crate::store::JournalProgress {
            durable_head: SequenceNumber(2),
            applied_head: SequenceNumber(2),
        }
    );
    assert_eq!(
        store
            .get(&first.table, &first.id)
            .expect("first lookup should succeed")
            .as_ref(),
        Some(&first)
    );
    assert_eq!(
        store
            .get(&second.table, &second.id)
            .expect("second lookup should succeed")
            .as_ref(),
        Some(&second)
    );
}

/// An execution-unit batch must land its resource-path bindings in the same
/// transaction as its document writes, and drop them again when the owning
/// document is deleted.
pub(crate) fn exercise_execution_unit_batch_persists_and_removes_resource_path_bindings_atomically<
    S: SqlPairStore,
>(
    store: &S,
) {
    let table = TableName::new("landmarks_store").expect("table name should parse");
    let document = crate::tests::sample_document("landmarks_store", "golden-gate");
    let binding = ResourcePathBinding::new(
        DocumentLocator::new(table.clone(), document.id.clone()),
        DocumentPath::from_segments(["cities", "SF", "landmarks", "golden-gate"])
            .expect("document path should parse"),
    );

    let commit = store
        .apply_execution_unit_batch(
            &[ResolvedWrite::Insert {
                document: document.clone(),
                indexes: Vec::new(),
                resource_path_binding: Some(binding.clone()),
            }],
            &[],
        )
        .expect("insert batch should succeed")
        .expect("insert batch should emit a commit");
    assert_eq!(commit.sequence, SequenceNumber(1));
    assert_eq!(
        store
            .locator_for_document_path(&binding.document_path)
            .expect("path lookup should succeed"),
        Some(binding.locator.clone())
    );

    let delete_commit = store
        .apply_execution_unit_batch(
            &[ResolvedWrite::Delete {
                previous: document,
                indexes: Vec::new(),
            }],
            &[],
        )
        .expect("delete batch should succeed")
        .expect("delete batch should emit a commit");
    assert_eq!(delete_commit.sequence, SequenceNumber(2));
    assert!(
        store
            .resource_path_binding(&binding.locator)
            .expect("binding lookup should succeed")
            .is_none(),
        "delete batch should remove the sidecar binding in the same transaction"
    );
    assert!(
        store
            .scan_collection_group_bindings(
                &CollectionName::new("landmarks").expect("collection group should parse"),
            )
            .expect("collection-group scan should succeed")
            .is_empty(),
        "delete batch should remove collection-group metadata too"
    );
}

/// Direct (non-journal) writes must leave the same document-version chain behind
/// as replayed ones, resolvable at each sequence in the insert/update/delete
/// history.
pub(crate) fn exercise_document_versions_track_direct_write_history<S: SqlPairStore>(store: &S) {
    let document = crate::tests::sample_document("versioned_tasks", "v1");
    let insert = store.insert(&document).expect("insert should succeed");
    let table_id = insert.writes[0].table_id.clone();
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

    let at_insert = store
        .document_version_at(&document.table, &table_id, &document.id, insert.sequence)
        .expect("insert version should load")
        .expect("insert version should exist");
    let at_update = store
        .document_version_at(&document.table, &table_id, &document.id, update.sequence)
        .expect("update version should load")
        .expect("update version should exist");
    let at_delete = store
        .document_version_at(&document.table, &table_id, &document.id, delete.sequence)
        .expect("delete version should load");

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
            .get(&document.table, &document.id)
            .expect("current row get should succeed")
            .is_none(),
        "current row should still reflect latest delete"
    );
}

/// A cancelled fenced batch must roll back completely: no journal progress, no
/// visible rows, and the committer lease left where it was.
pub(crate) fn exercise_sql_pipeline_cancellation_rolls_back<S: SqlPairStore>(store: &S) {
    let lease = store
        .acquire_committer_lease("cancel-owner", std::time::Duration::from_secs(30))
        .expect("lease should be acquired");
    let records = pipeline_barriers(3);
    let checks = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let checks_for_cancel = checks.clone();

    let error = store
        .fenced_append_and_apply_durable_records_batch_cancellable(
            &lease.owner_id,
            lease.epoch,
            SequenceNumber(0),
            &records,
            move || {
                if checks_for_cancel.fetch_add(1, Ordering::SeqCst) >= 3 {
                    Err(Error::Cancelled)
                } else {
                    Ok(())
                }
            },
        )
        .expect_err("cancellation should abort the provider transaction");
    assert!(matches!(
        error,
        crate::CommitterLeaseError::Storage(Error::Cancelled)
    ));
    assert!(checks.load(Ordering::SeqCst) >= 4);
    let diagnostic = store.write_pipeline_diagnostic();
    assert_eq!(diagnostic.journal_statement_count, 1);
    assert_eq!(diagnostic.cancellation_count, 1);
    assert_eq!(diagnostic.error_count, 1);
    assert_eq!(
        SqlStoreCore::journal_progress(store).expect("progress should read"),
        crate::store::JournalProgress {
            durable_head: SequenceNumber(0),
            applied_head: SequenceNumber(0),
        }
    );
    assert_eq!(
        store
            .read_committer_lease()
            .expect("lease should read")
            .expect("lease should exist")
            .durable_sequence,
        SequenceNumber(0)
    );
}

/// Direct writes must also maintain index-version intervals, one closed interval
/// per indexed value the document held.
pub(crate) fn exercise_index_versions_track_direct_write_history<S: SqlPairStore>(store: &S) {
    let table = TableName::new("indexed_versioned_tasks").expect("table name should be valid");
    let (schema, index) = indexed_rank_schema(&table);
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

    let intervals = store
        .index_version_intervals(&table_id, &index.id)
        .expect("index versions should load");

    assert_eq!(
        intervals,
        vec![
            (document.id.clone(), insert.sequence, Some(update.sequence)),
            (document.id.clone(), update.sequence, Some(delete.sequence)),
        ]
    );
}

/// Resource-path keys must stay unambiguous when a table or path segment contains
/// the delimiter the key encoding uses, so lookups cannot collide across tables.
pub(crate) fn exercise_resource_path_bindings_round_trip_without_table_name_delimiter_tricks<
    S: SqlPairStore,
>(
    store: &S,
) {
    let bindings = vec![
        binding("reserved_store", "loc_reserved", &["__meta__", "doc-1"]),
        binding("dotted_store", "loc_dotted", &["cities.v2", "SF"]),
        binding("unicode_store", "loc_unicode", &["日本語", "東京"]),
        binding("deep_store", "loc_deep", &["a", "1", "b", "2", "c", "3"]),
    ];

    for binding in &bindings {
        store
            .upsert_resource_path_binding(binding)
            .expect("binding should persist");
    }

    for binding in &bindings {
        assert_eq!(
            store
                .resource_path_binding(&binding.locator)
                .expect("binding lookup should succeed"),
            Some(binding.clone())
        );
        assert_eq!(
            store
                .locator_for_document_path(&binding.document_path)
                .expect("path lookup should succeed"),
            Some(binding.locator.clone())
        );
    }

    assert_eq!(
        store
            .scan_collection_group_bindings(
                &CollectionName::new("c").expect("collection group should parse"),
            )
            .expect("collection-group scan should succeed"),
        vec![bindings[3].clone()]
    );
}

/// A batch append must reach the provider as a single statement rather than one
/// per record, which is what the pipeline diagnostic counts.
pub(crate) fn exercise_batch_journal_insert_uses_one_provider_statement<S: SqlPairStore>(
    store: &S,
) {
    let records = pipeline_barriers(8);

    SqlStoreCore::append_durable_records_batch(store, &records)
        .expect("batch append should succeed");

    let diagnostic = store.write_pipeline_diagnostic();
    assert_eq!(diagnostic.batch_attempt_count, 1);
    assert_eq!(diagnostic.journal_record_count, 8);
    assert_eq!(diagnostic.journal_statement_count, 1);
    assert_eq!(diagnostic.provider_operation_count, 1);
    assert_eq!(diagnostic.max_observed_in_flight, 1);
    assert_eq!(
        SqlStoreCore::journal_progress(store).expect("progress should read"),
        crate::store::JournalProgress {
            durable_head: SequenceNumber(8),
            applied_head: SequenceNumber(0),
        }
    );
}

/// Indexes declared by a schema write must be usable by prefix and composite-range
/// reads immediately, and agree with a full table scan.
pub(crate) fn exercise_index_reads_round_trip_after_schema_write<S: SqlPairStore>(store: &S) {
    let table_schema = TableSchema {
        table: TableName::new("tasks").expect("table name should build"),
        fields: vec![
            FieldSchema {
                name: "team".to_string(),
                field_type: FieldType::String,
                required: true,
            },
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
            name: "by_team_status_rank".to_string(),
            fields: vec!["team".to_string(), "status".to_string(), "rank".to_string()],
        }],
        access_policy: None,
    };
    store
        .replace_table_schema(&table_schema)
        .expect("schema write should succeed");

    let first = Document::new(
        table_schema.table.clone(),
        serde_json::Map::from_iter([
            ("team".to_string(), serde_json::json!("alpha")),
            ("status".to_string(), serde_json::json!("open")),
            ("rank".to_string(), serde_json::json!(1)),
        ]),
    );
    let second = Document::new(
        table_schema.table.clone(),
        serde_json::Map::from_iter([
            ("team".to_string(), serde_json::json!("alpha")),
            ("status".to_string(), serde_json::json!("open")),
            ("rank".to_string(), serde_json::json!(3)),
        ]),
    );
    let third = Document::new(
        table_schema.table.clone(),
        serde_json::Map::from_iter([
            ("team".to_string(), serde_json::json!("beta")),
            ("status".to_string(), serde_json::json!("closed")),
            ("rank".to_string(), serde_json::json!(2)),
        ]),
    );
    store.insert(&first).expect("first insert should succeed");
    store.insert(&second).expect("second insert should succeed");
    store.insert(&third).expect("third insert should succeed");

    let direct = store
        .get(&first.table, &first.id)
        .expect("direct point read should succeed")
        .expect("first document should exist");
    assert_eq!(direct, first);

    let mut check_cancel = || Ok(());
    let scanned = store
        .scan_table_matching_cancellable(&table_schema.table, &mut check_cancel, |document| {
            Ok(document.fields.get("team").and_then(|value| value.as_str()) == Some("alpha"))
        })
        .expect("table scan should succeed");
    assert_eq!(scanned.len(), 2);
    assert!(scanned.iter().any(|document| document.id == first.id));
    assert!(scanned.iter().any(|document| document.id == second.id));

    let mut check_cancel = || Ok(());
    let prefix = store
        .index_scan_prefix_cancellable(
            &table_schema.table,
            "by_team_status_rank",
            &[serde_json::json!("alpha"), serde_json::json!("open")],
            &mut check_cancel,
        )
        .expect("prefix index scan should succeed");
    assert_eq!(prefix.len(), 2);
    assert!(prefix.iter().any(|document| document.id == first.id));
    assert!(prefix.iter().any(|document| document.id == second.id));

    let mut check_cancel = || Ok(());
    let ranged = store
        .index_scan_composite_range_cancellable(
            &table_schema.table,
            "by_team_status_rank",
            &[serde_json::json!("alpha"), serde_json::json!("open")],
            Bound::Included(&serde_json::json!(2)),
            Bound::Excluded(&serde_json::json!(4)),
            &mut check_cancel,
        )
        .expect("composite range index scan should succeed");
    assert_eq!(ranged.len(), 1);
    assert_eq!(ranged[0].id, second.id);
}

/// A repeated `insert_once` must dedupe rather than double-write, and the commit
/// log must record exactly the writes that happened.
pub(crate) fn exercise_direct_writes_dedupe_and_journal_progress_round_trip<S: SqlPairStore>(
    store: &S,
) {
    let document = crate::tests::sample_document("tasks", "First");

    let first_commit = store
        .insert_once(&document, Some("exec-1"))
        .expect("first deduplicated insert should succeed")
        .expect("first deduplicated insert should commit");
    assert_eq!(first_commit.sequence, SequenceNumber(1));
    assert!(
        store
            .insert_once(&document, Some("exec-1"))
            .expect("duplicate deduplicated insert should succeed")
            .is_none()
    );

    let updated_title = "Renamed";
    let second_commit = store
        .update_validated(
            &document.table,
            &document.id,
            &serde_json::Map::from_iter([("title".to_string(), serde_json::json!(updated_title))]),
            |_, _| Ok(()),
        )
        .expect("update should succeed");
    assert_eq!(second_commit.sequence, SequenceNumber(2));

    let updated = store
        .get(&document.table, &document.id)
        .expect("document lookup should succeed")
        .expect("updated document should exist");
    assert_eq!(
        updated.fields.get("title").and_then(|value| value.as_str()),
        Some(updated_title)
    );

    let (third_commit, removed) = store
        .delete_validated_returning_document(&document.table, &document.id, |_| Ok(()))
        .expect("delete should succeed");
    assert_eq!(third_commit.sequence, SequenceNumber(3));
    assert_eq!(removed.id, document.id);
    assert_eq!(
        SqlStoreCore::journal_progress(store).expect("journal progress should read"),
        crate::store::JournalProgress {
            durable_head: SequenceNumber(3),
            applied_head: SequenceNumber(3),
        }
    );

    let commits = store
        .read_commit_log_from(SequenceNumber(1))
        .expect("commit log should read");
    assert_eq!(commits.len(), 3);
    assert_eq!(commits[0].writes[0].op_type, WriteOpType::Insert);
    assert_eq!(commits[1].writes[0].op_type, WriteOpType::Update);
    assert_eq!(commits[2].writes[0].op_type, WriteOpType::Delete);
}

/// The trigger-delivery cursor lives in provider metadata and must survive a
/// write/read round trip unchanged.
pub(crate) fn exercise_trigger_delivery_cursor_round_trips_in_metadata<S: SqlPairStore>(store: &S) {
    assert_eq!(
        store.trigger_delivery_cursor().expect("cursor should load"),
        nimbus_core::TriggerDeliveryCursor::default()
    );

    store
        .set_trigger_delivery_cursor(nimbus_core::TriggerDeliveryCursor::new(SequenceNumber(19)))
        .expect("cursor should persist");

    assert_eq!(
        store
            .trigger_delivery_cursor()
            .expect("cursor should round trip"),
        nimbus_core::TriggerDeliveryCursor::new(SequenceNumber(19))
    );
}
