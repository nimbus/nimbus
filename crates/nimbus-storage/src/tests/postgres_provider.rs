use std::env;
use std::future::Future;
use std::ops::Bound;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use nimbus_core::{
    CollectionName, CronJob, CronSchedule, DocumentLocator, DocumentPath, Mutation,
    ResourcePathBinding, ScheduledJob, ScheduledJobOutcome, ScheduledJobResult, Schema,
    SchemaChangeEvent, SequenceNumber, TableId, TableName, TableState, TenantEventKind, TenantId,
    Timestamp, TriggerDeliveryCursor,
};
use testcontainers_modules::{
    postgres,
    testcontainers::{ContainerAsync, runners::AsyncRunner},
};

use super::{
    Document, DurableMutationRecord, Duration, FieldSchema, FieldType, IndexDefinition,
    PostgresProvider, PostgresProviderConfig, TableSchema, WriteOp, WriteOpType,
    implicit_external_provider_fixtures_disabled, require_explicit_external_provider_fixture_envs,
    timeout,
};
use crate::{ResolvedScheduleOp, ResolvedWrite};

const TEST_POSTGRES_URL_ENV: &str = "NIMBUS_TEST_POSTGRES_URL";
static TEST_SUFFIX_COUNTER: AtomicU64 = AtomicU64::new(0);

#[tokio::test(flavor = "multi_thread")]
async fn postgres_provider_manages_tenant_registry_and_schemas() {
    with_test_provider(|provider, _config| async move {
        let alpha = TenantId::new("alpha").expect("tenant id should build");
        let beta = TenantId::new("beta").expect("tenant id should build");

        assert_eq!(
            provider.list_tenants().await.expect("tenants should list"),
            Vec::<TenantId>::new()
        );

        let created_alpha = provider
            .create_tenant(&alpha)
            .await
            .expect("tenant should create");
        assert_eq!(
            created_alpha.schema_name,
            provider
                .tenant_schema_name(&alpha)
                .expect("tenant schema should derive")
        );
        assert!(
            provider
                .tenant_exists(&alpha)
                .await
                .expect("tenant existence should query")
        );

        let duplicate = provider.create_tenant(&alpha).await;
        assert!(matches!(
            duplicate,
            Err(nimbus_core::Error::AlreadyExists(_))
        ));

        provider
            .create_tenant(&beta)
            .await
            .expect("second tenant should create");
        assert_eq!(
            provider.list_tenants().await.expect("tenants should list"),
            vec![alpha.clone(), beta.clone()]
        );

        let reopened = provider
            .open_existing_tenant(&alpha)
            .await
            .expect("tenant should open")
            .expect("tenant should exist");
        assert_eq!(reopened.schema_name, created_alpha.schema_name);

        provider
            .delete_tenant(&alpha)
            .await
            .expect("tenant should delete");
        assert!(
            !provider
                .tenant_exists(&alpha)
                .await
                .expect("tenant existence should query")
        );
        assert!(
            provider
                .open_existing_tenant(&alpha)
                .await
                .expect("tenant open should succeed")
                .is_none()
        );
        assert_eq!(
            provider.list_tenants().await.expect("tenants should list"),
            vec![beta]
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn postgres_provider_reloads_registry_after_reconnect() {
    with_test_provider(|provider, config| async move {
        let tenant = TenantId::new("reload").expect("tenant id should build");
        let created = provider
            .create_tenant(&tenant)
            .await
            .expect("tenant should create");

        let reopened = PostgresProvider::connect(config)
            .await
            .expect("provider should reconnect");
        assert_eq!(
            reopened.list_tenants().await.expect("tenants should list"),
            vec![tenant.clone()]
        );
        assert_eq!(
            reopened
                .open_existing_tenant(&tenant)
                .await
                .expect("tenant should open")
                .expect("tenant should exist")
                .schema_name,
            created.schema_name
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn postgres_tenant_store_exposes_empty_read_foundation_after_create() {
    with_test_provider(|provider, _config| async move {
        let tenant = TenantId::new("foundation").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");

        assert_eq!(
            opened.store.load_schema().expect("schema should load"),
            Schema::default()
        );
        assert_eq!(
            opened
                .store
                .journal_progress()
                .expect("journal progress should load"),
            crate::store::JournalProgress {
                durable_head: SequenceNumber(0),
                applied_head: SequenceNumber(0),
            }
        );
        assert_eq!(
            opened
                .store
                .get(
                    &TableName::new("tasks").expect("table should build"),
                    &nimbus_core::DocumentId::new(),
                )
                .expect("point read should succeed"),
            None
        );

        let bootstrap = opened
            .store
            .export_durable_journal_bootstrap()
            .expect("bootstrap should export");
        assert_eq!(bootstrap.resume_after, SequenceNumber(0));
        assert_eq!(bootstrap.bootstrap_cut, SequenceNumber(0));
        assert_eq!(bootstrap.cursor_floor, SequenceNumber(0));
        assert_eq!(bootstrap.snapshot.schema, Schema::default());
        assert!(bootstrap.snapshot.documents.is_empty());
        assert!(bootstrap.snapshot.scheduled_execution_ids.is_empty());

        let snapshot = opened.store.read_snapshot().expect("snapshot should load");
        assert_eq!(
            snapshot
                .applied_sequence()
                .expect("snapshot applied sequence should load"),
            SequenceNumber(0)
        );
        assert!(
            snapshot
                .scan_table_matching_with_filters_cancellable(
                    &TableName::new("tasks").expect("table should build"),
                    &[],
                    &mut || Ok(()),
                    |_document| Ok(true),
                )
                .expect("snapshot scan should succeed")
                .is_empty()
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn postgres_direct_writes_dedupe_and_journal_progress_round_trip() {
    with_test_provider(|provider, _config| async move {
        let tenant = TenantId::new("writes").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");
        let document = super::sample_document("tasks", "First");

        let first_commit = opened
            .store
            .insert_once(&document, Some("exec-1"))
            .expect("first deduplicated insert should succeed")
            .expect("first deduplicated insert should commit");
        assert_eq!(first_commit.sequence, SequenceNumber(1));
        assert!(
            opened
                .store
                .insert_once(&document, Some("exec-1"))
                .expect("duplicate deduplicated insert should succeed")
                .is_none()
        );

        let updated_title = "Renamed";
        let second_commit = opened
            .store
            .update_validated(
                &document.table,
                &document.id,
                &serde_json::Map::from_iter([(
                    "title".to_string(),
                    serde_json::json!(updated_title),
                )]),
                |_, _| Ok(()),
            )
            .expect("update should succeed");
        assert_eq!(second_commit.sequence, SequenceNumber(2));

        let updated = opened
            .store
            .get(&document.table, &document.id)
            .expect("document lookup should succeed")
            .expect("updated document should exist");
        assert_eq!(
            updated.fields.get("title").and_then(|value| value.as_str()),
            Some(updated_title)
        );

        let (third_commit, removed) = opened
            .store
            .delete_validated_returning_document(&document.table, &document.id, |_| Ok(()))
            .expect("delete should succeed");
        assert_eq!(third_commit.sequence, SequenceNumber(3));
        assert_eq!(removed.id, document.id);
        assert_eq!(
            opened
                .store
                .journal_progress()
                .expect("journal progress should read"),
            crate::store::JournalProgress {
                durable_head: SequenceNumber(3),
                applied_head: SequenceNumber(3),
            }
        );

        let commits = opened
            .store
            .read_commit_log_from(SequenceNumber(1))
            .expect("commit log should read");
        assert_eq!(commits.len(), 3);
        assert_eq!(commits[0].writes[0].op_type, WriteOpType::Insert);
        assert_eq!(commits[1].writes[0].op_type, WriteOpType::Update);
        assert_eq!(commits[2].writes[0].op_type, WriteOpType::Delete);
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn postgres_document_versions_track_direct_write_history() {
    with_test_provider(|provider, _config| async move {
        let tenant = TenantId::new("document-versions").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");
        let document = super::sample_document("versioned_tasks", "v1");
        let insert = opened
            .store
            .insert(&document)
            .expect("insert should succeed");
        let table_id = insert.writes[0].table_id.clone();
        let update = opened
            .store
            .update_validated(
                &document.table,
                &document.id,
                &serde_json::Map::from_iter([("title".to_string(), serde_json::json!("v2"))]),
                |_, _| Ok(()),
            )
            .expect("update should succeed");
        let (delete, _) = opened
            .store
            .delete_validated_returning_document(&document.table, &document.id, |_| Ok(()))
            .expect("delete should succeed");

        let at_insert = opened
            .store
            .get_document_version_at(&document.table, &table_id, &document.id, insert.sequence)
            .expect("insert version should load")
            .expect("insert version should exist");
        let at_update = opened
            .store
            .get_document_version_at(&document.table, &table_id, &document.id, update.sequence)
            .expect("update version should load")
            .expect("update version should exist");
        let at_delete = opened
            .store
            .get_document_version_at(&document.table, &table_id, &document.id, delete.sequence)
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
            opened
                .store
                .get(&document.table, &document.id)
                .expect("current row get should succeed")
                .is_none(),
            "current row should still reflect latest delete"
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn postgres_document_versions_storage_diagnostic_reports_format_and_range() {
    with_test_provider(|provider, _config| async move {
        let tenant = TenantId::new("document-version-diagnostics").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");
        let document = super::sample_document("versioned_diagnostic_tasks", "v1");
        let insert = opened
            .store
            .insert(&document)
            .expect("insert should succeed");
        let update = opened
            .store
            .update_validated(
                &document.table,
                &document.id,
                &serde_json::Map::from_iter([("title".to_string(), serde_json::json!("v2"))]),
                |_, _| Ok(()),
            )
            .expect("update should succeed");
        let (delete, _) = opened
            .store
            .delete_validated_returning_document(&document.table, &document.id, |_| Ok(()))
            .expect("delete should succeed");

        let health = opened
            .store
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
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn postgres_document_versions_are_materialized_during_durable_recovery() {
    with_test_provider(|provider, _config| async move {
        let tenant = TenantId::new("document-version-recovery").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");
        let table = TableName::new("versioned_replay_tasks").expect("table name should be valid");
        let table_id = TableId::new();
        let inserted = super::sample_document("versioned_replay_tasks", "v1");
        let mut updated = inserted.clone();
        updated
            .fields
            .insert("title".to_string(), serde_json::json!("v2"));
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

        opened
            .store
            .append_durable_records_batch(&records)
            .expect("durable append should succeed");
        assert!(
            opened
                .store
                .get_document_version_at(&table, &table_id, &inserted.id, SequenceNumber(3))
                .expect("unapplied version lookup should succeed")
                .is_none(),
            "durable-only records must not materialize historical versions before recovery"
        );

        opened
            .store
            .recover_durable_journal()
            .expect("durable recovery should succeed");

        let at_insert = opened
            .store
            .get_document_version_at(&table, &table_id, &inserted.id, SequenceNumber(1))
            .expect("insert replay version should load")
            .expect("insert replay version should exist");
        let at_update = opened
            .store
            .get_document_version_at(&table, &table_id, &inserted.id, SequenceNumber(2))
            .expect("update replay version should load")
            .expect("update replay version should exist");
        let at_delete = opened
            .store
            .get_document_version_at(&table, &table_id, &inserted.id, SequenceNumber(3))
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
            opened
                .store
                .get(&table, &inserted.id)
                .expect("current row get should succeed")
                .is_none(),
            "replayed current row should still reflect latest delete"
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn postgres_index_versions_track_direct_write_history() {
    with_test_provider(|provider, _config| async move {
        let tenant = TenantId::new("index-versions").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");
        let table = TableName::new("indexed_versioned_tasks").expect("table name should be valid");
        let (schema, index) = indexed_rank_schema(&table);
        opened
            .store
            .replace_table_schema(&schema)
            .expect("schema should persist");
        let document = ranked_document(&table, "v1", 1);
        let insert = opened
            .store
            .insert(&document)
            .expect("insert should succeed");
        let table_id = insert.writes[0].table_id.clone();
        let update = opened
            .store
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
        let (delete, _) = opened
            .store
            .delete_validated_returning_document(&document.table, &document.id, |_| Ok(()))
            .expect("delete should succeed");

        let intervals = opened
            .store
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
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn postgres_index_versions_are_materialized_during_durable_recovery() {
    with_test_provider(|provider, _config| async move {
        let tenant = TenantId::new("index-version-recovery").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");
        let table = TableName::new("indexed_replay_tasks").expect("table name should be valid");
        let (schema, index) = indexed_rank_schema(&table);
        opened
            .store
            .replace_table_schema(&schema)
            .expect("schema should persist");
        let table_id = active_table_id_for_diagnostic(
            &opened
                .store
                .table_identity_diagnostics()
                .expect("table identity diagnostics should load"),
            &table,
        );
        let inserted = ranked_document(&table, "v1", 1);
        let mut updated = inserted.clone();
        updated
            .fields
            .insert("title".to_string(), serde_json::json!("v2"));
        updated
            .fields
            .insert("rank".to_string(), serde_json::json!(2));
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

        opened
            .store
            .append_durable_records_batch(&records)
            .expect("durable append should succeed");
        assert!(
            opened
                .store
                .index_version_intervals_for_testing(&table_id, &index.id)
                .expect("unapplied index versions should load")
                .is_empty(),
            "durable-only records must not materialize index versions before recovery"
        );

        opened
            .store
            .recover_durable_journal()
            .expect("durable recovery should succeed");

        let intervals = opened
            .store
            .index_version_intervals_for_testing(&table_id, &index.id)
            .expect("index versions should load after recovery");
        assert_eq!(intervals.len(), 2);
        assert_eq!(intervals[0].visible_from, SequenceNumber(2));
        assert_eq!(intervals[0].visible_until, Some(SequenceNumber(3)));
        assert_eq!(intervals[1].visible_from, SequenceNumber(3));
        assert_eq!(intervals[1].visible_until, Some(SequenceNumber(4)));
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn postgres_historical_index_scan_eq_and_range_use_versioned_visibility() {
    with_test_provider(|provider, _config| async move {
        let tenant = TenantId::new("historical-index").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");
        let table = TableName::new("historical_indexed_tasks").expect("table name should be valid");
        let (schema, _) = indexed_rank_schema(&table);
        opened
            .store
            .replace_table_schema(&schema)
            .expect("schema should persist");
        let document = ranked_document(&table, "v1", 1);
        let insert = opened
            .store
            .insert(&document)
            .expect("insert should succeed");
        let table_id = insert.writes[0].table_id.clone();
        let update = opened
            .store
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
        let (delete, _) = opened
            .store
            .delete_validated_returning_document(&document.table, &document.id, |_| Ok(()))
            .expect("delete should succeed");

        let at_insert = postgres_historical_read_shape(&table, &table_id, &schema, insert.sequence);
        let rank_one = opened
            .store
            .historical_index_scan_eq_cancellable(
                &at_insert,
                "by_rank",
                &serde_json::json!(1),
                &mut || Ok(()),
            )
            .expect("historical rank=1 scan should succeed");
        assert_eq!(postgres_document_titles(&rank_one), vec!["v1"]);
        assert_eq!(
            postgres_document_title_strings(&rank_one),
            postgres_rank_full_scan_oracle_titles(
                &opened.store,
                &table,
                &table_id,
                &[&document],
                insert.sequence,
                1
            )
        );
        assert!(
            opened
                .store
                .historical_index_scan_eq_cancellable(
                    &at_insert,
                    "by_rank",
                    &serde_json::json!(2),
                    &mut || Ok(())
                )
                .expect("historical rank=2 scan should succeed")
                .is_empty()
        );

        let at_update = postgres_historical_read_shape(&table, &table_id, &schema, update.sequence);
        let rank_two = opened
            .store
            .historical_index_scan_range_cancellable(
                &at_update,
                "by_rank",
                Bound::Included(&serde_json::json!(2)),
                Bound::Included(&serde_json::json!(2)),
                &mut || Ok(()),
            )
            .expect("historical rank range scan should succeed");
        assert_eq!(postgres_document_titles(&rank_two), vec!["v2"]);
        assert_eq!(
            postgres_document_title_strings(&rank_two),
            postgres_rank_full_scan_oracle_titles(
                &opened.store,
                &table,
                &table_id,
                &[&document],
                update.sequence,
                2
            )
        );

        let at_delete = postgres_historical_read_shape(&table, &table_id, &schema, delete.sequence);
        let deleted_rank_two = opened
            .store
            .historical_index_scan_eq_cancellable(
                &at_delete,
                "by_rank",
                &serde_json::json!(2),
                &mut || Ok(()),
            )
            .expect("historical deleted rank scan should succeed");
        assert_eq!(
            postgres_document_title_strings(&deleted_rank_two),
            postgres_rank_full_scan_oracle_titles(
                &opened.store,
                &table,
                &table_id,
                &[&document],
                delete.sequence,
                2
            )
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn postgres_historical_index_prefix_composite_range_and_pagination_are_stable() {
    with_test_provider(|provider, _config| async move {
        let tenant = TenantId::new("historical-composite").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");
        let table =
            TableName::new("historical_composite_tasks").expect("table name should be valid");
        let schema = postgres_status_rank_schema(&table);
        opened
            .store
            .replace_table_schema(&schema)
            .expect("schema should persist");
        let first = postgres_status_rank_document(&table, "first", "open", 1);
        let second = postgres_status_rank_document(&table, "second", "open", 2);
        let third = postgres_status_rank_document(&table, "third", "closed", 3);
        let first_insert = opened
            .store
            .insert(&first)
            .expect("first insert should succeed");
        let table_id = first_insert.writes[0].table_id.clone();
        opened
            .store
            .insert(&second)
            .expect("second insert should succeed");
        let third_insert = opened
            .store
            .insert(&third)
            .expect("third insert should succeed");

        let read_shape =
            postgres_historical_read_shape(&table, &table_id, &schema, third_insert.sequence);
        let open_docs = opened
            .store
            .historical_index_scan_prefix_cancellable(
                &read_shape,
                "by_status_rank",
                &[serde_json::json!("open")],
                &mut || Ok(()),
            )
            .expect("historical prefix scan should succeed");
        assert_eq!(
            postgres_document_titles(&open_docs),
            vec!["first", "second"]
        );
        assert_eq!(
            postgres_document_title_strings(&open_docs),
            postgres_status_rank_full_scan_oracle_titles(
                &opened.store,
                &table_id,
                &[&first, &second, &third],
                third_insert.sequence,
                "open",
                None,
                None
            )
        );

        let exact_rank_two = opened
            .store
            .historical_index_scan_composite_range_cancellable(
                &read_shape,
                "by_status_rank",
                &[serde_json::json!("open")],
                Bound::Included(&serde_json::json!(2)),
                Bound::Included(&serde_json::json!(2)),
                &mut || Ok(()),
            )
            .expect("historical composite range scan should succeed");
        assert_eq!(postgres_document_titles(&exact_rank_two), vec!["second"]);
        assert_eq!(
            postgres_document_title_strings(&exact_rank_two),
            postgres_status_rank_full_scan_oracle_titles(
                &opened.store,
                &table_id,
                &[&first, &second, &third],
                third_insert.sequence,
                "open",
                Some(2),
                Some(2)
            )
        );

        let first_page = opened
            .store
            .historical_index_scan_prefix_page_cancellable(
                &read_shape,
                "by_status_rank",
                &[serde_json::json!("open")],
                None,
                1,
                &mut || Ok(()),
            )
            .expect("first historical page should succeed");
        assert_eq!(
            postgres_document_titles(&first_page.documents),
            vec!["first"]
        );
        let cursor = first_page
            .next_cursor
            .as_ref()
            .expect("first page should return a cursor");
        let second_page = opened
            .store
            .historical_index_scan_prefix_page_cancellable(
                &read_shape,
                "by_status_rank",
                &[serde_json::json!("open")],
                Some(cursor),
                1,
                &mut || Ok(()),
            )
            .expect("second historical page should succeed");
        assert_eq!(
            postgres_document_titles(&second_page.documents),
            vec!["second"]
        );

        let mismatch = opened
            .store
            .historical_index_scan_prefix_page_cancellable(
                &read_shape,
                "by_status_rank",
                &[serde_json::json!("closed")],
                Some(cursor),
                1,
                &mut || Ok(()),
            )
            .expect_err("cursor from a different prefix must fail closed");
        assert_eq!(
            mismatch.historical_read_kind(),
            Some(nimbus_core::HistoricalReadErrorKind::CursorMismatch)
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn postgres_notification_listener_reports_schema_journal_and_scheduler_hints() {
    with_test_provider(|provider, _config| async move {
        let tenant = TenantId::new("notifications").expect("tenant id should build");
        let mut listener = provider
            .connect_notification_listener()
            .await
            .expect("notification listener should connect");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");

        opened
            .store
            .replace_table_schema(&TableSchema {
                table: TableName::new("tasks").expect("table name should build"),
                fields: vec![super::FieldSchema {
                    name: "title".to_string(),
                    field_type: super::FieldType::String,
                    required: true,
                }],
                indexes: Vec::new(),
                access_policy: None,
            })
            .expect("schema write should succeed");
        let schema_hint = timeout(Duration::from_secs(2), listener.recv())
            .await
            .expect("schema hint should arrive")
            .expect("listener should stay open")
            .expect("schema hint should decode");
        assert_eq!(schema_hint.tenant_id, tenant);
        assert!(schema_hint.schema_changed);
        assert!(
            schema_hint.journal_changed,
            "schema changes now append tenant events and must wake journal consumers"
        );
        assert!(!schema_hint.scheduler_changed);

        opened
            .store
            .insert(&super::sample_document("tasks", "journaled"))
            .expect("direct write should succeed");
        let journal_hint = timeout(Duration::from_secs(2), listener.recv())
            .await
            .expect("journal hint should arrive")
            .expect("listener should stay open")
            .expect("journal hint should decode");
        assert_eq!(journal_hint.tenant_id, tenant);
        assert!(journal_hint.journal_changed);
        assert!(!journal_hint.schema_changed);

        opened
            .store
            .insert_scheduled_job(&scheduled_insert_job(Timestamp(5_000), "queued"))
            .expect("scheduled job write should succeed");
        let scheduler_hint = timeout(Duration::from_secs(2), listener.recv())
            .await
            .expect("scheduler hint should arrive")
            .expect("listener should stay open")
            .expect("scheduler hint should decode");
        assert_eq!(scheduler_hint.tenant_id, tenant);
        assert!(scheduler_hint.scheduler_changed);
        assert!(!scheduler_hint.journal_changed);
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn postgres_resource_path_bindings_round_trip_without_table_name_delimiter_tricks() {
    with_test_provider(|provider, _config| async move {
        let tenant = TenantId::new("resource-paths").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");
        let bindings = vec![
            binding("reserved_store", "loc_reserved", &["__meta__", "doc-1"]),
            binding("dotted_store", "loc_dotted", &["cities.v2", "SF"]),
            binding("unicode_store", "loc_unicode", &["日本語", "東京"]),
            binding("deep_store", "loc_deep", &["a", "1", "b", "2", "c", "3"]),
        ];

        for binding in &bindings {
            opened
                .store
                .upsert_resource_path_binding(binding)
                .expect("binding should persist");
        }

        for binding in &bindings {
            assert_eq!(
                opened
                    .store
                    .resource_path_binding(&binding.locator)
                    .expect("binding lookup should succeed"),
                Some(binding.clone())
            );
            assert_eq!(
                opened
                    .store
                    .locator_for_document_path(&binding.document_path)
                    .expect("path lookup should succeed"),
                Some(binding.locator.clone())
            );
        }

        assert_eq!(
            opened
                .store
                .scan_collection_group_bindings(
                    &CollectionName::new("c").expect("collection group should parse"),
                )
                .expect("collection-group scan should succeed"),
            vec![bindings[3].clone()]
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn postgres_trigger_delivery_cursor_round_trips_in_metadata() {
    with_test_provider(|provider, _config| async move {
        let tenant = TenantId::new("trigger-cursor").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");

        assert_eq!(
            opened
                .store
                .trigger_delivery_cursor()
                .expect("cursor should load"),
            nimbus_core::TriggerDeliveryCursor::default()
        );

        opened
            .store
            .set_trigger_delivery_cursor(nimbus_core::TriggerDeliveryCursor::new(SequenceNumber(
                19,
            )))
            .expect("cursor should persist");

        assert_eq!(
            opened
                .store
                .trigger_delivery_cursor()
                .expect("cursor should round trip"),
            nimbus_core::TriggerDeliveryCursor::new(SequenceNumber(19))
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn postgres_execution_unit_batch_and_scheduler_state_round_trip() {
    with_test_provider(|provider, _config| async move {
        let tenant = TenantId::new("batch").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");
        let document = super::sample_document("tasks", "batched");
        let scheduled_job = scheduled_insert_job(Timestamp(5_000), "queued");

        let commit = opened
            .store
            .apply_execution_unit_batch(
                &[ResolvedWrite::Insert {
                    document: document.clone(),
                    indexes: Vec::new(),
                    resource_path_binding: None,
                }],
                &[ResolvedScheduleOp::Insert {
                    job: scheduled_job.clone(),
                }],
            )
            .expect("batch should succeed")
            .expect("batch with writes should emit a commit");
        assert_eq!(commit.sequence, SequenceNumber(1));
        assert_eq!(
            opened
                .store
                .get(&document.table, &document.id)
                .expect("document lookup should succeed")
                .as_ref(),
            Some(&document)
        );
        assert_eq!(
            opened
                .store
                .list_scheduled_jobs()
                .expect("pending jobs should read"),
            vec![scheduled_job.clone()]
        );

        let claimed = opened
            .store
            .claim_due_jobs(Timestamp(5_000), usize::MAX)
            .expect("claim should succeed");
        assert_eq!(claimed, vec![scheduled_job.clone()]);

        opened
            .store
            .recover_running_jobs(Timestamp(6_000))
            .expect("running-job recovery should succeed");
        let recovered = opened
            .store
            .list_scheduled_jobs()
            .expect("pending jobs should read");
        assert_eq!(recovered.len(), 1);
        assert_eq!(recovered[0].id, scheduled_job.id);
        assert_eq!(recovered[0].run_at, Timestamp(6_000));

        let claimed = opened
            .store
            .claim_due_jobs(Timestamp(6_000), usize::MAX)
            .expect("second claim should succeed");
        let result = ScheduledJobResult {
            id: scheduled_job.id.clone(),
            run_at: Timestamp(6_000),
            finished_at: Timestamp(6_500),
            mutation: claimed[0].mutation.clone(),
            outcome: ScheduledJobOutcome::Completed,
            error: None,
        };
        opened
            .store
            .record_scheduled_job_result(&result)
            .expect("result should persist");
        opened
            .store
            .complete_scheduled_job(&scheduled_job.id)
            .expect("complete should succeed");
        assert_eq!(
            opened
                .store
                .get_scheduled_job_result(&scheduled_job.id)
                .expect("result lookup should succeed"),
            Some(result)
        );

        let cron = CronJob {
            name: "heartbeat".to_string(),
            schedule: CronSchedule::Interval { seconds: 10 },
            mutation: Mutation::Insert {
                table: TableName::new("tasks").expect("table name should build"),
                id: None,
                fields: serde_json::Map::from_iter([(
                    "title".to_string(),
                    serde_json::json!("heartbeat"),
                )]),
            },
            enabled: true,
            last_run: None,
            next_run: Timestamp(7_000),
            created_at: Timestamp(500),
        };
        opened
            .store
            .save_cron_job(&cron)
            .expect("cron save should succeed");
        assert_eq!(
            opened
                .store
                .load_cron_jobs()
                .expect("cron load should succeed"),
            vec![cron.clone()]
        );
        assert_eq!(
            opened
                .store
                .next_scheduled_work_at()
                .expect("next scheduled work should read"),
            Some(Timestamp(7_000))
        );
        assert!(
            opened
                .store
                .has_scheduled_work()
                .expect("scheduler work should be present")
        );
        opened
            .store
            .delete_cron_job(&cron.name)
            .expect("cron delete should succeed");
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn postgres_execution_unit_batch_persists_and_removes_resource_path_bindings_atomically() {
    with_test_provider(|provider, _config| async move {
        let tenant = TenantId::new("resource-batch").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");
        let table = TableName::new("landmarks_store").expect("table name should parse");
        let document = super::sample_document("landmarks_store", "golden-gate");
        let binding = ResourcePathBinding::new(
            DocumentLocator::new(table.clone(), document.id.clone()),
            DocumentPath::from_segments(["cities", "SF", "landmarks", "golden-gate"])
                .expect("document path should parse"),
        );

        let commit = opened
            .store
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
            opened
                .store
                .locator_for_document_path(&binding.document_path)
                .expect("path lookup should succeed"),
            Some(binding.locator.clone())
        );

        let delete_commit = opened
            .store
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
            opened
                .store
                .resource_path_binding(&binding.locator)
                .expect("binding lookup should succeed")
                .is_none(),
            "delete batch should remove the sidecar binding in the same transaction"
        );
        assert!(
            opened
                .store
                .scan_collection_group_bindings(
                    &CollectionName::new("landmarks").expect("collection group should parse"),
                )
                .expect("collection-group scan should succeed")
                .is_empty(),
            "delete batch should remove collection-group metadata too"
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn postgres_durable_journal_recovery_applies_pending_records() {
    with_test_provider(|provider, _config| async move {
        let tenant = TenantId::new("recovery").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");
        let first = super::sample_document("tasks", "First");
        let second = super::sample_document("tasks", "Second");
        let table_id = TableId::new();
        let records = vec![
            DurableMutationRecord::new(
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
            DurableMutationRecord::new(
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

        opened
            .store
            .append_durable_records_batch(&records)
            .expect("durable append should succeed");
        assert_eq!(
            opened
                .store
                .journal_progress()
                .expect("journal progress should read"),
            crate::store::JournalProgress {
                durable_head: SequenceNumber(2),
                applied_head: SequenceNumber(0),
            }
        );
        assert!(
            opened
                .store
                .get(&first.table, &first.id)
                .expect("first lookup should succeed")
                .is_none()
        );

        let progress = opened
            .store
            .recover_durable_journal()
            .expect("recovery should apply pending durable records");
        assert_eq!(
            progress,
            crate::store::JournalProgress {
                durable_head: SequenceNumber(2),
                applied_head: SequenceNumber(2),
            }
        );
        assert_eq!(
            opened
                .store
                .get(&first.table, &first.id)
                .expect("first lookup should succeed")
                .as_ref(),
            Some(&first)
        );
        assert_eq!(
            opened
                .store
                .get(&second.table, &second.id)
                .expect("second lookup should succeed")
                .as_ref(),
            Some(&second)
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn postgres_tenant_event_journal_replays_mixed_history() {
    with_test_provider(|provider, _config| async move {
        let tenant = TenantId::new("tenant-event-mixed").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");
        let table = TableName::new("tasks_tenant_event").expect("table name should build");
        let table_id = TableId::new();
        let schema = TableSchema {
            table: table.clone(),
            fields: vec![FieldSchema {
                name: "rank".to_string(),
                field_type: FieldType::Number,
                required: false,
            }],
            indexes: vec![IndexDefinition {
                id: nimbus_core::IndexId::new(),
                state: nimbus_core::IndexState::Enabled,
                name: "by_rank".to_string(),
                fields: vec!["rank".to_string()],
            }],
            access_policy: None,
        };
        let document = Document::new(
            table.clone(),
            serde_json::Map::from_iter([
                ("title".to_string(), serde_json::json!("evented")),
                ("rank".to_string(), serde_json::json!(7)),
            ]),
        );
        let records = vec![
            DurableMutationRecord::from_events(
                SequenceNumber(1),
                Timestamp(100),
                vec![TenantEventKind::TableLifecycle {
                    lifecycle: nimbus_core::TableLifecycleEvent::StageHidden {
                        table: table.clone(),
                        table_id: table_id.clone(),
                    },
                }],
            )
            .expect("stage-hidden event should build"),
            DurableMutationRecord::from_events(
                SequenceNumber(2),
                Timestamp(200),
                vec![TenantEventKind::TableLifecycle {
                    lifecycle: nimbus_core::TableLifecycleEvent::ActivateHidden {
                        table: table.clone(),
                        table_id: table_id.clone(),
                        replaced_table_id: None,
                    },
                }],
            )
            .expect("activate-hidden event should build"),
            DurableMutationRecord::from_events(
                SequenceNumber(3),
                Timestamp(300),
                vec![
                    TenantEventKind::SchemaChange {
                        change: Box::new(SchemaChangeEvent::SetTable {
                            table: table.clone(),
                            table_id: table_id.clone(),
                            previous: None,
                            current: schema.clone(),
                        }),
                    },
                    TenantEventKind::IndexLifecycle {
                        index: nimbus_core::IndexLifecycleEvent {
                            table: table.clone(),
                            table_id: table_id.clone(),
                            index_id: schema.indexes[0].id.clone(),
                            state: schema.indexes[0].state,
                            definition: schema.indexes[0].clone(),
                        },
                    },
                ],
            )
            .expect("schema event should build"),
            DurableMutationRecord::from_events(
                SequenceNumber(4),
                Timestamp(400),
                vec![TenantEventKind::DocumentWrite {
                    writes: vec![WriteOp {
                        table: table.clone(),
                        table_id: table_id.clone(),
                        op_type: WriteOpType::Insert,
                        doc_id: document.id.clone(),
                        resource_path_binding: None,
                        trigger_write_origin: None,
                        previous: None,
                        current: Some(document.clone()),
                    }],
                }],
            )
            .expect("document event should build"),
            DurableMutationRecord::from_events(
                SequenceNumber(5),
                Timestamp(500),
                vec![TenantEventKind::TriggerDelivery {
                    cursor: TriggerDeliveryCursor::new(SequenceNumber(4)),
                }],
            )
            .expect("trigger cursor event should build"),
        ];

        opened
            .store
            .apply_durable_records_batch(&records)
            .expect("mixed tenant event replay should apply");

        assert_eq!(
            opened.store.table_id(&table).expect("table id should load"),
            Some(table_id)
        );
        let loaded_schema = opened.store.load_schema().expect("schema should load");
        assert_eq!(loaded_schema.get_table(&table), Some(&schema));
        assert_eq!(
            opened
                .store
                .get(&table, &document.id)
                .expect("document lookup should succeed")
                .as_ref(),
            Some(&document)
        );
        assert_eq!(
            opened
                .store
                .trigger_delivery_cursor()
                .expect("trigger cursor should load"),
            TriggerDeliveryCursor::new(SequenceNumber(4))
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn postgres_durable_replay_retires_recreated_table_identity() {
    with_test_provider(|provider, _config| async move {
        let tenant = TenantId::new("durable-recreate").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");
        let table = TableName::new("tasks_durable_recreate").expect("table name should build");
        let old_table_id = TableId::new();
        let new_table_id = TableId::new();
        let old_document = Document::new(
            table.clone(),
            serde_json::Map::from_iter([("title".to_string(), serde_json::json!("old"))]),
        );
        let new_document = Document::new(
            table.clone(),
            serde_json::Map::from_iter([("title".to_string(), serde_json::json!("new"))]),
        );
        let records = vec![
            DurableMutationRecord::new(
                SequenceNumber(1),
                Timestamp(100),
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
                Timestamp(200),
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

        opened
            .store
            .apply_durable_records_batch(&records)
            .expect("durable replay should infer table recreation");

        assert_eq!(
            opened.store.table_id(&table).expect("table id should load"),
            Some(new_table_id.clone())
        );
        assert!(
            opened
                .store
                .get(&table, &old_document.id)
                .expect("old logical lookup should succeed")
                .is_none()
        );
        assert_eq!(
            opened
                .store
                .get(&table, &new_document.id)
                .expect("new logical lookup should succeed")
                .as_ref(),
            Some(&new_document)
        );
        let mut check_cancel = || Ok(());
        assert_eq!(
            opened
                .store
                .scan_table_matching_cancellable(&table, &mut check_cancel, |_| Ok(true))
                .expect("active table scan should succeed"),
            vec![new_document]
        );
        let diagnostics = opened
            .store
            .table_identity_diagnostics()
            .expect("diagnostics should load after replay");
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.table_name == table
                && diagnostic.table_id == new_table_id
                && diagnostic.state == TableState::Active
                && diagnostic.document_count == Some(1)
        }));
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.table_name == table
                && diagnostic.table_id == old_table_id
                && diagnostic.state == TableState::Deleting
                && diagnostic.document_count.is_none()
        }));
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn postgres_index_reads_round_trip_after_schema_write() {
    with_test_provider(|provider, _config| async move {
        let tenant = TenantId::new("indexed-reads").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");
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
        opened
            .store
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
        opened
            .store
            .insert(&first)
            .expect("first insert should succeed");
        opened
            .store
            .insert(&second)
            .expect("second insert should succeed");
        opened
            .store
            .insert(&third)
            .expect("third insert should succeed");

        let direct = opened
            .store
            .get(&first.table, &first.id)
            .expect("direct point read should succeed")
            .expect("first document should exist");
        assert_eq!(direct, first);

        let mut check_cancel = || Ok(());
        let scanned = opened
            .store
            .scan_table_matching_cancellable(&table_schema.table, &mut check_cancel, |document| {
                Ok(document.fields.get("team").and_then(|value| value.as_str()) == Some("alpha"))
            })
            .expect("table scan should succeed");
        assert_eq!(scanned.len(), 2);
        assert!(scanned.iter().any(|document| document.id == first.id));
        assert!(scanned.iter().any(|document| document.id == second.id));

        let mut check_cancel = || Ok(());
        let prefix = opened
            .store
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
        let ranged = opened
            .store
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
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn postgres_table_lifecycle_activates_hidden_identity_and_diagnostics_track_layout() {
    with_test_provider(|provider, _config| async move {
        let tenant = TenantId::new("table-lifecycle").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");
        let table = TableName::new("tasks_lifecycle").expect("table name should build");
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
        opened
            .store
            .replace_table_schema(&schema)
            .expect("schema write should succeed");

        let old_document = Document::new(
            table.clone(),
            serde_json::Map::from_iter([("title".to_string(), serde_json::json!("old"))]),
        );
        let old_commit = opened
            .store
            .insert(&old_document)
            .expect("old document should insert");
        let old_table_id = old_commit.writes[0].table_id.clone();
        let replacement_table_id = TableId::new();

        opened
            .store
            .stage_hidden_table_identity(&table, &replacement_table_id)
            .expect("hidden replacement identity should stage");
        let staged = opened
            .store
            .table_identity_diagnostics()
            .expect("diagnostics should load after staging");
        assert!(staged.iter().any(|diagnostic| {
            diagnostic.table_name == table
                && diagnostic.table_id == replacement_table_id
                && diagnostic.state == TableState::Hidden
                && diagnostic.backend_layout == crate::TableBackendLayout::SharedDocumentsByTableId
                && diagnostic.summary_status == crate::TableSummaryStatus::Unsupported
                && diagnostic.document_count.is_none()
        }));

        let retired = opened
            .store
            .activate_hidden_table_identity(&table, &replacement_table_id)
            .expect("hidden identity should activate");
        assert_eq!(retired.as_ref(), Some(&old_table_id));
        assert_eq!(
            opened.store.table_id(&table).expect("table id should load"),
            Some(replacement_table_id.clone())
        );
        assert!(
            opened
                .store
                .get(&table, &old_document.id)
                .expect("logical get should use active replacement")
                .is_none()
        );

        let new_document = Document::new(
            table.clone(),
            serde_json::Map::from_iter([("title".to_string(), serde_json::json!("new"))]),
        );
        let new_commit = opened
            .store
            .insert(&new_document)
            .expect("new document should insert under replacement identity");
        assert_eq!(new_commit.writes[0].table_id, replacement_table_id);

        let diagnostics = opened
            .store
            .table_identity_diagnostics()
            .expect("diagnostics should load after activation");
        let active = diagnostics
            .iter()
            .find(|diagnostic| {
                diagnostic.table_name == table && diagnostic.table_id == replacement_table_id
            })
            .expect("active replacement diagnostic should exist");
        assert_eq!(active.state, TableState::Active);
        assert_eq!(active.document_count, Some(1));
        assert_eq!(
            active.summary_status,
            crate::TableSummaryStatus::ExactDocumentCount
        );
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.table_name == table
                && diagnostic.table_id == old_table_id
                && diagnostic.state == TableState::Deleting
                && diagnostic.document_count.is_none()
        }));

        assert!(
            opened
                .store
                .hard_delete_table_identity(&old_table_id)
                .expect("hard delete should succeed")
        );
        let diagnostics = opened
            .store
            .table_identity_diagnostics()
            .expect("diagnostics should load after hard delete");
        assert!(
            diagnostics
                .iter()
                .all(|diagnostic| diagnostic.table_id != old_table_id),
            "hard delete should remove retired catalog identity: {diagnostics:?}"
        );
        let mut check_cancel = || Ok(());
        assert_eq!(
            opened
                .store
                .index_scan_eq_cancellable(
                    &table,
                    "by_title",
                    &serde_json::json!("new"),
                    &mut check_cancel,
                )
                .expect("active replacement index scan should succeed"),
            vec![new_document]
        );
    })
    .await;
}

async fn with_test_provider<F, Fut>(test: F)
where
    F: FnOnce(PostgresProvider, PostgresProviderConfig) -> Fut,
    Fut: Future<Output = ()>,
{
    let connection = match test_connection().await {
        Some(connection) => connection,
        None => return,
    };
    let suffix = unique_suffix();
    let metadata_schema = format!("nimbus_test_{}", &suffix[..24.min(suffix.len())]);
    let tenant_schema_prefix = format!("tenant_{}_", &suffix[..12.min(suffix.len())]);
    let config = PostgresProviderConfig {
        connection_string: connection.connection_string().to_string(),
        metadata_schema,
        tenant_schema_prefix,
        min_connections: Some(1),
        max_connections: Some(4),
    };
    let provider = PostgresProvider::connect(config.clone())
        .await
        .expect("provider should connect");
    test(provider.clone(), config).await;
    provider
        .drop_metadata_schema_for_test()
        .await
        .expect("test metadata schema should drop");
    drop(connection);
}

enum TestConnection {
    External(String),
    Container {
        connection_string: String,
        _container: Box<ContainerAsync<postgres::Postgres>>,
    },
}

impl TestConnection {
    fn connection_string(&self) -> &str {
        match self {
            Self::External(connection_string) => connection_string,
            Self::Container {
                connection_string, ..
            } => connection_string,
        }
    }
}

async fn test_connection() -> Option<TestConnection> {
    if let Ok(connection_string) = env::var(TEST_POSTGRES_URL_ENV) {
        return Some(TestConnection::External(connection_string));
    }

    require_explicit_external_provider_fixture_envs("Postgres provider", &[TEST_POSTGRES_URL_ENV]);
    if implicit_external_provider_fixtures_disabled("Postgres provider") {
        return None;
    }

    let container = match postgres::Postgres::default().start().await {
        Ok(container) => container,
        Err(error) => {
            eprintln!(
                "skipping postgres provider test because no explicit Postgres URL was provided and container startup failed: {error}"
            );
            return None;
        }
    };
    let host = container
        .get_host()
        .await
        .expect("container host should resolve");
    let port = container
        .get_host_port_ipv4(5432)
        .await
        .expect("container port should resolve");
    Some(TestConnection::Container {
        connection_string: format!(
            "host={host} port={port} user=postgres password=postgres dbname=postgres"
        ),
        _container: Box::new(container),
    })
}

fn unique_suffix() -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after unix epoch")
        .as_nanos();
    let counter = TEST_SUFFIX_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{counter:08x}{:x}{timestamp:x}", std::process::id())
}

fn indexed_rank_schema(table: &TableName) -> (TableSchema, IndexDefinition) {
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
            ("title".to_string(), serde_json::json!(title)),
            ("rank".to_string(), serde_json::json!(rank)),
        ]),
    )
}

fn postgres_status_rank_schema(table: &TableName) -> TableSchema {
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
        indexes: vec![IndexDefinition {
            id: nimbus_core::IndexId::new(),
            state: nimbus_core::IndexState::Enabled,
            name: "by_status_rank".to_string(),
            fields: vec!["status".to_string(), "rank".to_string()],
        }],
        access_policy: None,
    }
}

fn postgres_status_rank_document(
    table: &TableName,
    title: &str,
    status: &str,
    rank: u64,
) -> Document {
    Document::new(
        table.clone(),
        serde_json::Map::from_iter([
            ("title".to_string(), serde_json::json!(title)),
            ("status".to_string(), serde_json::json!(status)),
            ("rank".to_string(), serde_json::json!(rank)),
        ]),
    )
}

fn postgres_historical_read_shape(
    table: &TableName,
    table_id: &TableId,
    schema: &TableSchema,
    sequence: SequenceNumber,
) -> nimbus_core::HistoricalReadShape {
    let registry = nimbus_core::VersionedRegistry::from_records([
        nimbus_core::TenantEventRecord::schema_change(
            SequenceNumber(1),
            Timestamp(100),
            SchemaChangeEvent::SetTable {
                table: table.clone(),
                table_id: table_id.clone(),
                previous: None,
                current: schema.clone(),
            },
        )
        .expect("schema change event should build"),
    ])
    .expect("registry should build");
    registry
        .read_shape_at(table, postgres_historical_snapshot(sequence))
        .expect("read shape should load")
        .expect("table should exist at historical read")
}

fn postgres_historical_snapshot(sequence: SequenceNumber) -> nimbus_core::HistoricalReadSnapshot {
    let timestamp = Timestamp(sequence.0.saturating_mul(100));
    nimbus_core::HistoricalReadSnapshot::new(
        nimbus_core::ReadTimestamp::new(timestamp),
        nimbus_core::CommitSequence::new(sequence),
        nimbus_core::CommitTimestamp::new(timestamp),
    )
}

fn postgres_rank_full_scan_oracle_titles(
    store: &crate::PostgresTenantStore,
    table: &TableName,
    table_id: &TableId,
    corpus: &[&Document],
    sequence: SequenceNumber,
    rank: u64,
) -> Vec<String> {
    let mut titles = corpus
        .iter()
        .filter_map(|document| {
            store
                .get_document_version_at(table, table_id, &document.id, sequence)
                .expect("document version oracle should load")
        })
        .filter(|document| {
            document.fields.get("rank").and_then(|value| value.as_u64()) == Some(rank)
        })
        .map(|document| postgres_document_title_string(&document))
        .collect::<Vec<_>>();
    titles.sort();
    titles
}

fn postgres_status_rank_full_scan_oracle_titles(
    store: &crate::PostgresTenantStore,
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
            store
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
                Some((rank, postgres_document_title_string(&document)))
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    rows.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
    rows.into_iter().map(|(_, title)| title).collect()
}

fn postgres_document_titles(documents: &[Document]) -> Vec<&str> {
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

fn postgres_document_title_strings(documents: &[Document]) -> Vec<String> {
    documents
        .iter()
        .map(postgres_document_title_string)
        .collect()
}

fn postgres_document_title_string(document: &Document) -> String {
    document
        .fields
        .get("title")
        .and_then(|value| value.as_str())
        .expect("document should have a string title")
        .to_string()
}

fn active_table_id_for_diagnostic(
    diagnostics: &[crate::TableIdentityDiagnostic],
    table: &TableName,
) -> TableId {
    diagnostics
        .iter()
        .find(|diagnostic| {
            diagnostic.table_name == *table && diagnostic.state == TableState::Active
        })
        .expect("active table identity should exist")
        .table_id
        .clone()
}

#[allow(clippy::too_many_arguments)]
fn durable_write_record(
    sequence: SequenceNumber,
    timestamp: Timestamp,
    table: &TableName,
    table_id: &TableId,
    op_type: WriteOpType,
    doc_id: nimbus_core::DocumentId,
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

fn binding(table: &str, id: &str, path: &[&str]) -> ResourcePathBinding {
    ResourcePathBinding::new(
        DocumentLocator::new(
            TableName::new(table).expect("table name should parse"),
            nimbus_core::DocumentId::from_key(id).expect("document id should parse"),
        ),
        DocumentPath::from_segments(path.iter().copied()).expect("document path should parse"),
    )
}

fn scheduled_insert_job(run_at: Timestamp, title: &str) -> ScheduledJob {
    ScheduledJob {
        id: nimbus_core::DocumentId::new(),
        run_at,
        mutation: Mutation::Insert {
            table: TableName::new("tasks").expect("table name should build"),
            id: None,
            fields: serde_json::Map::from_iter([("title".to_string(), serde_json::json!(title))]),
        },
        created_at: Timestamp(100),
    }
}
