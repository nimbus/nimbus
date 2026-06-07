use std::env;
use std::future::Future;
use std::net::TcpListener;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use libsql::{Builder, Database};
use nimbus_core::{
    CollectionName, CronJob, CronSchedule, Document, DocumentId, DocumentLocator, DocumentPath,
    DurableMutationRecord, FieldSchema, FieldType, IndexDefinition, Mutation, ResourcePathBinding,
    ScheduledJob, ScheduledJobOutcome, ScheduledJobResult, SchemaChangeEvent, SequenceNumber,
    TableId, TableName, TableSchema, TableState, TenantEventKind, TenantId, Timestamp,
    TriggerDeliveryCursor, WriteOp, WriteOpType,
};
use serial_test::serial;
use testcontainers_modules::testcontainers::core::{IntoContainerPort, WaitFor};
use testcontainers_modules::testcontainers::{
    ContainerAsync, GenericImage, ImageExt, runners::AsyncRunner,
};

use super::{
    Duration, LibsqlReplicaProvider, LibsqlReplicaProviderConfig, SqliteTenantStore,
    implicit_external_provider_fixtures_disabled, require_explicit_external_provider_fixture_envs,
    tempdir, timeout,
};
use crate::async_storage::TenantReadStorage;
use crate::libsql::libsql_transport_connector;
use crate::{
    LibsqlReplicaBarrierPath, LibsqlReplicaRefreshCause, LibsqlReplicaRefreshPath,
    ResolvedScheduleOp, ResolvedWrite,
};

const LIBSQL_URL_ENV: &str = "NIMBUS_LIBSQL_URL";
const LIBSQL_AUTH_TOKEN_ENV: &str = "NIMBUS_LIBSQL_AUTH_TOKEN";
const LIBSQL_ADMIN_URL_ENV: &str = "NIMBUS_LIBSQL_ADMIN_URL";
const LIBSQL_ADMIN_AUTH_HEADER_ENV: &str = "NIMBUS_LIBSQL_ADMIN_AUTH_HEADER";
static TEST_SUFFIX_COUNTER: AtomicU64 = AtomicU64::new(0);

#[tokio::test(flavor = "multi_thread")]
#[serial]
async fn libsql_provider_manages_tenant_registry_and_namespaces() {
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
            created_alpha.namespace,
            provider
                .tenant_namespace(&alpha)
                .expect("tenant namespace should derive")
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
        assert_eq!(reopened.namespace, created_alpha.namespace);

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
            vec![beta.clone()]
        );

        let recreated_alpha = provider
            .create_tenant(&alpha)
            .await
            .expect("tenant should recreate after delete");
        assert_eq!(
            recreated_alpha.namespace,
            provider
                .tenant_namespace(&alpha)
                .expect("tenant namespace should derive")
        );
        assert_eq!(
            provider.list_tenants().await.expect("tenants should list"),
            vec![alpha, beta]
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
#[serial]
async fn libsql_provider_reloads_registry_after_reconnect() {
    with_test_provider(|provider, config| async move {
        let tenant = TenantId::new("reload").expect("tenant id should build");
        let created = provider
            .create_tenant(&tenant)
            .await
            .expect("tenant should create");

        let reopened = LibsqlReplicaProvider::connect(config)
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
                .namespace,
            created.namespace
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
#[serial]
async fn libsql_opened_tenant_materializes_local_sqlite_snapshot() {
    with_test_provider(|provider, config| async move {
        let tenant = TenantId::new("opened").expect("tenant id should build");
        let registration = provider
            .create_tenant(&tenant)
            .await
            .expect("tenant should create");
        let table = TableName::new("tasks").expect("table name should build");
        let table_schema = TableSchema {
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
        let document_id = DocumentId::new();
        seed_remote_namespace(
            &config,
            &registration.namespace,
            &table_schema,
            document_id.clone(),
            serde_json::json!({
                "rank": 5,
                "title": "from-primary"
            }),
        )
        .await;

        let refreshed_path = provider
            .refresh_tenant_snapshot(&tenant)
            .await
            .expect("tenant snapshot should refresh");
        assert!(
            refreshed_path.exists(),
            "refreshed replica path should exist"
        );

        let opened = provider
            .open_existing_opened_tenant(&tenant)
            .await
            .expect("opened tenant should sync and open")
            .expect("tenant should exist");
        assert_eq!(opened.tenant_id(), &tenant);
        assert_eq!(opened.namespace(), registration.namespace);
        assert_eq!(opened.primary_url(), config.primary_url);
        assert_eq!(opened.replica_path(), refreshed_path.as_path());
        assert_eq!(
            opened
                .store
                .read_snapshot()
                .expect("snapshot should open")
                .journal_mode()
                .expect("journal mode should read"),
            "wal"
        );

        let table_for_read = table.clone();
        let indexed = opened
            .read_storage
            .execute(move |store| {
                let snapshot = store.read_snapshot()?;
                let mut check_cancel = || Ok(());
                snapshot.index_scan_eq_cancellable(
                    &table_for_read,
                    "by_rank",
                    &serde_json::json!(5),
                    &mut check_cancel,
                )
            })
            .await
            .expect("async indexed read should succeed");
        assert_eq!(indexed.len(), 1);
        assert_eq!(indexed[0].id, document_id);
        assert_eq!(
            indexed[0].fields.get("title").expect("field should exist"),
            &serde_json::json!("from-primary")
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
#[serial]
async fn libsql_direct_writes_refresh_derivative_cache_and_round_trip_journal_progress() {
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
        assert_eq!(
            opened
                .store
                .get(&document.table, &document.id)
                .expect("document lookup should succeed")
                .as_ref(),
            Some(&document)
        );

        let second_commit = opened
            .store
            .update_validated(
                &document.table,
                &document.id,
                &serde_json::Map::from_iter([("title".to_string(), serde_json::json!("Renamed"))]),
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
            Some("Renamed")
        );

        let (third_commit, removed) = opened
            .store
            .delete_validated_returning_document(&document.table, &document.id, |_| Ok(()))
            .expect("delete should succeed");
        assert_eq!(third_commit.sequence, SequenceNumber(3));
        assert_eq!(removed.id, document.id);

        timeout(Duration::from_secs(5), async {
            loop {
                if opened
                    .store
                    .journal_progress()
                    .expect("journal progress should load during background refresh")
                    == (crate::store::JournalProgress {
                        durable_head: SequenceNumber(3),
                        applied_head: SequenceNumber(3),
                    })
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        })
        .await
        .expect("background refresh should catch the derivative cache up without a read-triggered refresh");

        let freshness = opened
            .store
            .replica_freshness_stats()
            .expect("freshness stats should load after background refresh");
        assert_eq!(freshness.required_sequence, SequenceNumber(3));
        assert_eq!(freshness.local_applied_sequence, SequenceNumber(3));
        assert_eq!(
            freshness.last_refresh_cause,
            LibsqlReplicaRefreshCause::CommitBarrier
        );
        assert_eq!(
            freshness.last_refresh_path,
            LibsqlReplicaRefreshPath::IncrementalCatchUp
        );
        assert!(
            freshness.incremental_refresh_count >= 1,
            "incremental refresh count should record the commit-barrier catch-up"
        );
        assert_eq!(freshness.refresh_error_count, 0);

        assert!(
            opened
                .store
                .get(&document.table, &document.id)
                .expect("deleted lookup should succeed")
                .is_none()
        );
        let after_read = opened
            .store
            .replica_freshness_stats()
            .expect("freshness stats should load after a current-cache read");
        assert_eq!(
            after_read.last_barrier_path,
            LibsqlReplicaBarrierPath::AlreadyCurrentCache
        );
        assert!(
            after_read.barrier_current_count >= 1,
            "a current-cache read should increment the already-current barrier counter"
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
#[serial]
async fn libsql_document_versions_track_direct_write_history_and_snapshot_cache() {
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

        let replica_path = provider
            .refresh_tenant_snapshot(&tenant)
            .await
            .expect("tenant snapshot should refresh after version writes");
        let local = SqliteTenantStore::open(&replica_path)
            .expect("refreshed local replica cache should open");
        let local_at_update = local
            .get_document_version_at(&document.table, &table_id, &document.id, update.sequence)
            .expect("local cache update version should load")
            .expect("local cache update version should exist");
        assert_eq!(
            local_at_update.fields.get("title"),
            Some(&serde_json::json!("v2"))
        );
        assert!(
            local
                .get_document_version_at(&document.table, &table_id, &document.id, delete.sequence)
                .expect("local cache delete version should load")
                .is_none(),
            "local cache should copy delete tombstones"
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
#[serial]
async fn libsql_document_versions_storage_diagnostic_reports_format_and_range() {
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
#[serial]
async fn libsql_document_versions_are_materialized_during_durable_recovery() {
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

        let replica_path = provider
            .refresh_tenant_snapshot(&tenant)
            .await
            .expect("tenant snapshot should refresh after replayed version writes");
        let local = SqliteTenantStore::open(&replica_path)
            .expect("refreshed local replica cache should open");
        let local_at_insert = local
            .get_document_version_at(&table, &table_id, &inserted.id, SequenceNumber(1))
            .expect("local cache insert version should load")
            .expect("local cache insert version should exist");
        assert_eq!(
            local_at_insert.fields.get("title"),
            Some(&serde_json::json!("v1"))
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
#[serial]
async fn libsql_index_versions_track_direct_write_history_and_snapshot_cache() {
    with_test_provider(|provider, _config| async move {
        let tenant = TenantId::new("index-versions").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");
        let table = TableName::new("indexed_versioned_tasks").expect("table name should be valid");
        let (schema, index) = libsql_indexed_rank_schema(&table);
        opened
            .store
            .replace_table_schema(&schema)
            .expect("schema should persist");
        let document = libsql_ranked_document(&table, "v1", 1);
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

        let replica_path = provider
            .refresh_tenant_snapshot(&tenant)
            .await
            .expect("tenant snapshot should refresh after index version writes");
        let local = SqliteTenantStore::open(&replica_path)
            .expect("refreshed local replica cache should open");
        let local_intervals = local
            .index_version_intervals_for_testing(&table_id, &index.id)
            .expect("local cache index versions should load");
        assert_eq!(local_intervals.len(), intervals.len());
        for (local_interval, remote_interval) in local_intervals.iter().zip(intervals.iter()) {
            assert_eq!(local_interval.document_id, remote_interval.document_id);
            assert_eq!(local_interval.visible_from, remote_interval.visible_from);
            assert_eq!(local_interval.visible_until, remote_interval.visible_until);
        }
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
#[serial]
async fn libsql_index_versions_are_materialized_during_durable_recovery() {
    with_test_provider(|provider, _config| async move {
        let tenant = TenantId::new("index-version-recovery").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");
        let table = TableName::new("indexed_replay_tasks").expect("table name should be valid");
        let (schema, index) = libsql_indexed_rank_schema(&table);
        opened
            .store
            .replace_table_schema(&schema)
            .expect("schema should persist");
        let diagnostics = opened
            .store
            .table_identity_diagnostics()
            .expect("table identity diagnostics should load");
        let table_id = libsql_active_table_id_for_diagnostic(&diagnostics, &table);
        let inserted = libsql_ranked_document(&table, "v1", 1);
        let mut updated = inserted.clone();
        updated
            .fields
            .insert("title".to_string(), serde_json::json!("v2"));
        updated
            .fields
            .insert("rank".to_string(), serde_json::json!(2));
        updated.update_time = Timestamp(updated.update_time.0.saturating_add(1));
        let records = vec![
            libsql_durable_write_record(
                SequenceNumber(2),
                Timestamp(100),
                &table,
                &table_id,
                WriteOpType::Insert,
                inserted.id.clone(),
                None,
                Some(inserted.clone()),
            ),
            libsql_durable_write_record(
                SequenceNumber(3),
                Timestamp(101),
                &table,
                &table_id,
                WriteOpType::Update,
                inserted.id.clone(),
                Some(inserted.clone()),
                Some(updated.clone()),
            ),
            libsql_durable_write_record(
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
#[serial]
async fn libsql_historical_index_scan_eq_and_range_use_versioned_visibility() {
    with_test_provider(|provider, _config| async move {
        let tenant = TenantId::new("historical-index").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");
        let table = TableName::new("historical_indexed_tasks").expect("table name should be valid");
        let (schema, _) = libsql_indexed_rank_schema(&table);
        opened
            .store
            .replace_table_schema(&schema)
            .expect("schema should persist");
        let document = libsql_ranked_document(&table, "v1", 1);
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

        let at_insert = libsql_historical_read_shape(&table, &table_id, &schema, insert.sequence);
        let rank_one = opened
            .store
            .historical_index_scan_eq_cancellable(
                &at_insert,
                "by_rank",
                &serde_json::json!(1),
                &mut || Ok(()),
            )
            .expect("historical rank=1 scan should succeed");
        assert_eq!(libsql_document_titles(&rank_one), vec!["v1"]);
        assert_eq!(
            libsql_document_title_strings(&rank_one),
            libsql_rank_full_scan_oracle_titles(
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

        let at_update = libsql_historical_read_shape(&table, &table_id, &schema, update.sequence);
        let rank_two = opened
            .store
            .historical_index_scan_range_cancellable(
                &at_update,
                "by_rank",
                Some(&serde_json::json!(2)),
                Some(&serde_json::json!(2)),
                true,
                true,
                &mut || Ok(()),
            )
            .expect("historical rank range scan should succeed");
        assert_eq!(libsql_document_titles(&rank_two), vec!["v2"]);
        assert_eq!(
            libsql_document_title_strings(&rank_two),
            libsql_rank_full_scan_oracle_titles(
                &opened.store,
                &table,
                &table_id,
                &[&document],
                update.sequence,
                2
            )
        );

        let at_delete = libsql_historical_read_shape(&table, &table_id, &schema, delete.sequence);
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
            libsql_document_title_strings(&deleted_rank_two),
            libsql_rank_full_scan_oracle_titles(
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
#[serial]
async fn libsql_historical_index_prefix_composite_range_and_pagination_are_stable() {
    with_test_provider(|provider, _config| async move {
        let tenant = TenantId::new("historical-composite").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");
        let table =
            TableName::new("historical_composite_tasks").expect("table name should be valid");
        let schema = libsql_status_rank_schema(&table);
        opened
            .store
            .replace_table_schema(&schema)
            .expect("schema should persist");
        let first = libsql_status_rank_document(&table, "first", "open", 1);
        let second = libsql_status_rank_document(&table, "second", "open", 2);
        let third = libsql_status_rank_document(&table, "third", "closed", 3);
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
            libsql_historical_read_shape(&table, &table_id, &schema, third_insert.sequence);
        let open_docs = opened
            .store
            .historical_index_scan_prefix_cancellable(
                &read_shape,
                "by_status_rank",
                &[serde_json::json!("open")],
                &mut || Ok(()),
            )
            .expect("historical prefix scan should succeed");
        assert_eq!(libsql_document_titles(&open_docs), vec!["first", "second"]);
        assert_eq!(
            libsql_document_title_strings(&open_docs),
            libsql_status_rank_full_scan_oracle_titles(
                &opened.store,
                &table,
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
                Some(&serde_json::json!(2)),
                Some(&serde_json::json!(2)),
                true,
                true,
                &mut || Ok(()),
            )
            .expect("historical composite range scan should succeed");
        assert_eq!(libsql_document_titles(&exact_rank_two), vec!["second"]);
        assert_eq!(
            libsql_document_title_strings(&exact_rank_two),
            libsql_status_rank_full_scan_oracle_titles(
                &opened.store,
                &table,
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
        assert_eq!(libsql_document_titles(&first_page.documents), vec!["first"]);
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
            libsql_document_titles(&second_page.documents),
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
#[serial]
async fn libsql_trigger_delivery_cursor_round_trips_in_remote_metadata() {
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
                17,
            )))
            .expect("cursor should persist");

        assert_eq!(
            opened
                .store
                .trigger_delivery_cursor()
                .expect("cursor should round trip"),
            nimbus_core::TriggerDeliveryCursor::new(SequenceNumber(17))
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
#[serial]
async fn libsql_execution_unit_batch_and_scheduler_state_round_trip() {
    with_test_provider(|provider, _config| async move {
        let tenant = TenantId::new("batch").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");
        let table_schema = TableSchema {
            table: TableName::new("tasks").expect("table name should build"),
            fields: vec![FieldSchema {
                name: "title".to_string(),
                field_type: FieldType::String,
                required: false,
            }],
            indexes: Vec::new(),
            access_policy: None,
        };
        opened
            .store
            .replace_table_schema(&table_schema)
            .expect("schema write should succeed");
        timeout(Duration::from_secs(5), async {
            loop {
                let freshness = opened
                    .store
                    .replica_freshness_stats()
                    .expect("freshness stats should load while schema refresh runs");
                if freshness.full_snapshot_refresh_count >= 1 {
                    assert_eq!(
                        freshness.last_refresh_cause,
                        LibsqlReplicaRefreshCause::SchemaWrite
                    );
                    assert_eq!(
                        freshness.last_refresh_path,
                        LibsqlReplicaRefreshPath::FullSnapshotRebuild
                    );
                    break;
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        })
        .await
        .expect("schema write should trigger a full snapshot refresh");
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
        assert_eq!(commit.sequence, SequenceNumber(2));
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
            .claim_due_jobs(Timestamp(5_000))
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
            .claim_due_jobs(Timestamp(6_000))
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
#[serial]
async fn libsql_execution_unit_batch_round_trips_resource_path_bindings() {
    with_test_provider(|provider, _config| async move {
        let tenant = TenantId::new("resource-paths").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");
        let table = TableName::new("landmarks_store").expect("table name should build");
        let document = Document::new(
            table.clone(),
            serde_json::Map::from_iter([("rank".to_string(), serde_json::json!(1))]),
        );
        let binding = ResourcePathBinding::new(
            DocumentLocator::new(table.clone(), document.id.clone()),
            DocumentPath::from_segments(["cities", "SF", "landmarks", "golden-gate"])
                .expect("document path should parse"),
        );

        let insert_commit = opened
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
        assert_eq!(insert_commit.sequence, SequenceNumber(1));

        let snapshot = opened
            .store
            .read_snapshot()
            .expect("replica snapshot should open after insert");
        assert_eq!(
            snapshot
                .locator_for_document_path(&binding.document_path)
                .expect("path lookup should succeed"),
            Some(binding.locator.clone())
        );
        assert_eq!(
            snapshot
                .scan_collection_group_bindings(
                    &CollectionName::new("landmarks").expect("collection group should parse"),
                )
                .expect("collection-group scan should succeed"),
            vec![binding.clone()]
        );
        drop(snapshot);

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

        let snapshot = opened
            .store
            .read_snapshot()
            .expect("replica snapshot should open after delete");
        assert!(
            snapshot
                .resource_path_binding(&binding.locator)
                .expect("binding lookup should succeed")
                .is_none(),
            "delete batch should remove the sidecar binding"
        );
        assert!(
            snapshot
                .scan_collection_group_bindings(
                    &CollectionName::new("landmarks").expect("collection group should parse"),
                )
                .expect("collection-group scan should succeed")
                .is_empty(),
            "delete batch should clear the collection-group index row"
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
#[serial]
async fn libsql_durable_journal_recovery_refreshes_local_cache_from_remote_records() {
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

        assert_eq!(
            opened
                .store
                .get(&first.table, &first.id)
                .expect("first lookup should succeed")
                .as_ref(),
            None
        );

        let progress = opened
            .store
            .recover_durable_journal()
            .expect("recovery should apply pending durable records and refresh the cache");
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
                .get(&second.table, &second.id)
                .expect("second lookup should succeed")
                .as_ref(),
            Some(&second)
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
#[serial]
async fn libsql_tenant_event_journal_replays_mixed_history() {
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
        provider
            .refresh_tenant_snapshot(&tenant)
            .await
            .expect("mixed replay should refresh to the local cache");
        let opened = provider
            .open_existing_opened_tenant(&tenant)
            .await
            .expect("tenant should reopen after replay")
            .expect("tenant should still exist");

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
#[serial]
async fn libsql_durable_replay_retires_recreated_table_identity() {
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
        provider
            .refresh_tenant_snapshot(&tenant)
            .await
            .expect("replayed table identity should refresh to the local cache");
        let opened = provider
            .open_existing_opened_tenant(&tenant)
            .await
            .expect("tenant should reopen after replay")
            .expect("tenant should still exist");

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
#[serial]
async fn libsql_table_lifecycle_activates_hidden_identity_and_diagnostics_track_layout() {
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
        provider
            .refresh_tenant_snapshot(&tenant)
            .await
            .expect("staged identity should refresh to the local cache");
        let opened = provider
            .open_existing_opened_tenant(&tenant)
            .await
            .expect("tenant should reopen after staging")
            .expect("tenant should still exist");
        let staged = opened
            .store
            .table_identity_diagnostics()
            .expect("diagnostics should load after staging refresh");
        assert!(
            staged.iter().any(|diagnostic| {
                diagnostic.table_name == table
                    && diagnostic.table_id == replacement_table_id
                    && diagnostic.state == TableState::Hidden
                    && diagnostic.backend_layout
                        == crate::TableBackendLayout::LibsqlReplicaSharedDocumentsByTableId
                    && diagnostic.summary_status == crate::TableSummaryStatus::Unsupported
                    && diagnostic.document_count.is_none()
            }),
            "hidden replacement diagnostic should be visible after refresh: {staged:?}"
        );

        let retired = opened
            .store
            .activate_hidden_table_identity(&table, &replacement_table_id)
            .expect("hidden identity should activate");
        assert_eq!(retired.as_ref(), Some(&old_table_id));
        assert_eq!(
            opened.store.table_id(&table).expect("table id should load"),
            Some(replacement_table_id.clone())
        );
        provider
            .refresh_tenant_snapshot(&tenant)
            .await
            .expect("activated identity should refresh to the local cache");
        let opened = provider
            .open_existing_opened_tenant(&tenant)
            .await
            .expect("tenant should reopen after activation")
            .expect("tenant should still exist");
        let activated = opened
            .store
            .table_identity_diagnostics()
            .expect("diagnostics should load after activation refresh");
        assert!(activated.iter().any(|diagnostic| {
            diagnostic.table_name == table
                && diagnostic.table_id == replacement_table_id
                && diagnostic.state == TableState::Active
        }));
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
        provider
            .refresh_tenant_snapshot(&tenant)
            .await
            .expect("new document should refresh to the local cache");
        let opened = provider
            .open_existing_opened_tenant(&tenant)
            .await
            .expect("tenant should reopen after new insert")
            .expect("tenant should still exist");
        let diagnostics = opened
            .store
            .table_identity_diagnostics()
            .expect("diagnostics should load after insert refresh");
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

        assert!(
            opened
                .store
                .hard_delete_table_identity(&old_table_id)
                .expect("hard delete should succeed")
        );
        provider
            .refresh_tenant_snapshot(&tenant)
            .await
            .expect("hard delete should refresh to the local cache");
        let opened = provider
            .open_existing_opened_tenant(&tenant)
            .await
            .expect("tenant should reopen after hard delete")
            .expect("tenant should still exist");
        let diagnostics = opened
            .store
            .table_identity_diagnostics()
            .expect("diagnostics should load after hard-delete refresh");
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
                .index_scan_prefix_cancellable(
                    &table,
                    "by_title",
                    &[serde_json::json!("new")],
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
    F: FnOnce(LibsqlReplicaProvider, LibsqlReplicaProviderConfig) -> Fut,
    Fut: Future<Output = ()>,
{
    let connection = match test_connection().await {
        Some(connection) => connection,
        None => return,
    };
    let replica_cache_dir = tempdir().expect("replica cache dir should create");
    let suffix = unique_suffix();
    let metadata_namespace = format!("nimbus_meta_{}", &suffix[..16.min(suffix.len())]);
    let tenant_namespace_prefix = format!("tenant_{}_", &suffix[..12.min(suffix.len())]);
    let mut config = LibsqlReplicaProviderConfig::new(
        connection.primary_url().to_string(),
        connection.admin_api_url().to_string(),
        replica_cache_dir.path(),
    );
    config.auth_token = connection.auth_token().map(ToOwned::to_owned);
    config.admin_auth_header = connection.admin_auth_header().map(ToOwned::to_owned);
    config.metadata_namespace = metadata_namespace;
    config.tenant_namespace_prefix = tenant_namespace_prefix;

    let provider = LibsqlReplicaProvider::connect(config.clone())
        .await
        .expect("provider should connect");
    test(provider.clone(), config).await;
    provider
        .drop_provider_namespaces_for_test()
        .await
        .expect("provider namespaces should clean up");
    drop(connection);
}

enum TestConnection {
    External {
        primary_url: String,
        auth_token: Option<String>,
        admin_api_url: String,
        admin_auth_header: Option<String>,
    },
    Container {
        primary_url: String,
        auth_token: Option<String>,
        admin_api_url: String,
        admin_auth_header: Option<String>,
        _container: Box<ContainerAsync<GenericImage>>,
    },
}

impl TestConnection {
    fn primary_url(&self) -> &str {
        match self {
            Self::External { primary_url, .. } => primary_url,
            Self::Container { primary_url, .. } => primary_url,
        }
    }

    fn auth_token(&self) -> Option<&str> {
        match self {
            Self::External { auth_token, .. } => auth_token.as_deref(),
            Self::Container { auth_token, .. } => auth_token.as_deref(),
        }
    }

    fn admin_api_url(&self) -> &str {
        match self {
            Self::External { admin_api_url, .. } => admin_api_url,
            Self::Container { admin_api_url, .. } => admin_api_url,
        }
    }

    fn admin_auth_header(&self) -> Option<&str> {
        match self {
            Self::External {
                admin_auth_header, ..
            } => admin_auth_header.as_deref(),
            Self::Container {
                admin_auth_header, ..
            } => admin_auth_header.as_deref(),
        }
    }
}

async fn test_connection() -> Option<TestConnection> {
    if let Ok(primary_url) = env::var(LIBSQL_URL_ENV) {
        let admin_api_url = env::var(LIBSQL_ADMIN_URL_ENV).unwrap_or_else(|_| {
            panic!(
                "{LIBSQL_ADMIN_URL_ENV} is required when {LIBSQL_URL_ENV} is set for libsql provider tests"
            )
        });
        return Some(TestConnection::External {
            primary_url,
            auth_token: env::var(LIBSQL_AUTH_TOKEN_ENV).ok(),
            admin_api_url,
            admin_auth_header: env::var(LIBSQL_ADMIN_AUTH_HEADER_ENV).ok(),
        });
    }

    require_explicit_external_provider_fixture_envs(
        "libsql provider",
        &[LIBSQL_URL_ENV, LIBSQL_ADMIN_URL_ENV],
    );
    if implicit_external_provider_fixtures_disabled("libsql provider") {
        return None;
    }

    let image = GenericImage::new("ghcr.io/tursodatabase/libsql-server", "latest")
        // The image's wrapper/log stream is not a stable readiness seam for
        // testcontainers. We do a short startup delay here, then use a live
        // provider connect loop below as the authoritative readiness check.
        .with_wait_for(WaitFor::seconds(1))
        // The image entrypoint already injects --http-listen-addr from
        // SQLD_HTTP_LISTEN_ADDR; passing that flag again makes current images
        // exit with a duplicate-argument error before readiness probing starts.
        .with_env_var("SQLD_ADMIN_LISTEN_ADDR", "0.0.0.0:8081")
        .with_cmd(vec![
            "/bin/sqld".to_string(),
            "--enable-namespaces".to_string(),
            "--no-welcome".to_string(),
        ]);
    let host_http_port = allocate_host_port();
    let host_admin_port = allocate_host_port();
    let image = image
        .with_mapped_port(host_http_port, 8080.tcp())
        .with_mapped_port(host_admin_port, 8081.tcp());
    let container = match image.start().await {
        Ok(container) => container,
        Err(error) => {
            eprintln!(
                "skipping libsql provider test because no explicit libsql URL was provided and container startup failed: {error}"
            );
            return None;
        }
    };
    let host = container
        .get_host()
        .await
        .expect("container host should resolve");
    let primary_url = format!("http://{host}:{host_http_port}");
    let admin_api_url = format!("http://{host}:{host_admin_port}");

    if timeout(Duration::from_secs(60), async {
        loop {
            let replica_cache_dir = tempdir().expect("temporary replica cache dir should create");
            let config = LibsqlReplicaProviderConfig::new(
                primary_url.clone(),
                admin_api_url.clone(),
                replica_cache_dir.keep(),
            );
            if LibsqlReplicaProvider::connect(config).await.is_ok() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    })
    .await
    .is_err()
    {
        eprintln!("skipping libsql provider test because the libsql container never became ready");
        return None;
    }

    Some(TestConnection::Container {
        primary_url,
        auth_token: None,
        admin_api_url,
        admin_auth_header: None,
        _container: Box::new(container),
    })
}

fn unique_suffix() -> String {
    format!(
        "{:x}{:x}{:016x}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after the unix epoch")
            .as_nanos(),
        TEST_SUFFIX_COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}

fn libsql_indexed_rank_schema(table: &TableName) -> (TableSchema, IndexDefinition) {
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

fn libsql_ranked_document(table: &TableName, title: &str, rank: u64) -> Document {
    Document::new(
        table.clone(),
        serde_json::Map::from_iter([
            ("title".to_string(), serde_json::json!(title)),
            ("rank".to_string(), serde_json::json!(rank)),
        ]),
    )
}

fn libsql_status_rank_schema(table: &TableName) -> TableSchema {
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

fn libsql_status_rank_document(
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

fn libsql_historical_read_shape(
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
        .read_shape_at(table, libsql_historical_snapshot(sequence))
        .expect("read shape should load")
        .expect("table should exist at historical read")
}

fn libsql_historical_snapshot(sequence: SequenceNumber) -> nimbus_core::HistoricalReadSnapshot {
    let timestamp = Timestamp(sequence.0.saturating_mul(100));
    nimbus_core::HistoricalReadSnapshot::new(
        nimbus_core::ReadTimestamp::new(timestamp),
        nimbus_core::CommitSequence::new(sequence),
        nimbus_core::CommitTimestamp::new(timestamp),
    )
}

fn libsql_rank_full_scan_oracle_titles(
    store: &crate::LibsqlReplicaTenantStore,
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
        .map(|document| libsql_document_title_string(&document))
        .collect::<Vec<_>>();
    titles.sort();
    titles
}

fn libsql_status_rank_full_scan_oracle_titles(
    store: &crate::LibsqlReplicaTenantStore,
    table: &TableName,
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
                .get_document_version_at(table, table_id, &document.id, sequence)
                .expect("document version oracle should load")
        })
        .filter_map(|document| {
            let document_status = document.fields.get("status")?.as_str()?;
            let rank = document.fields.get("rank")?.as_u64()?;
            if document_status == status
                && start_rank.is_none_or(|start| rank >= start)
                && end_rank.is_none_or(|end| rank <= end)
            {
                Some((rank, libsql_document_title_string(&document)))
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    rows.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
    rows.into_iter().map(|(_, title)| title).collect()
}

fn libsql_document_titles(documents: &[Document]) -> Vec<&str> {
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

fn libsql_document_title_strings(documents: &[Document]) -> Vec<String> {
    documents.iter().map(libsql_document_title_string).collect()
}

fn libsql_document_title_string(document: &Document) -> String {
    document
        .fields
        .get("title")
        .and_then(|value| value.as_str())
        .expect("document should have a string title")
        .to_string()
}

fn libsql_active_table_id_for_diagnostic(
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
fn libsql_durable_write_record(
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

fn allocate_host_port() -> u16 {
    TcpListener::bind(("127.0.0.1", 0))
        .expect("temporary port probe should bind")
        .local_addr()
        .expect("temporary port probe should resolve")
        .port()
}

async fn seed_remote_namespace(
    config: &LibsqlReplicaProviderConfig,
    namespace: &str,
    table_schema: &TableSchema,
    document_id: DocumentId,
    fields: serde_json::Value,
) {
    let database = open_remote_namespace_database(config, namespace)
        .await
        .expect("remote namespace database should open");
    let conn = database
        .connect()
        .expect("remote namespace connection should open");
    conn.execute(
        "INSERT INTO schemas (table_name, schema_json) VALUES (?, ?)",
        libsql::params![
            table_schema.table.as_str(),
            serde_json::to_string(table_schema).expect("schema should serialize")
        ],
    )
    .await
    .expect("remote schema insert should succeed");
    let table_id = TableId::new();
    conn.execute(
        "INSERT INTO table_catalog (namespace, table_name, table_id) VALUES (?, ?, ?)",
        libsql::params!["default", table_schema.table.as_str(), table_id.as_str()],
    )
    .await
    .expect("remote table catalog insert should succeed");
    conn.execute(
        "INSERT INTO documents (table_id, id, data_json, typed_fields_json, creation_time, update_time) VALUES (?, ?, ?, ?, ?, ?)",
        libsql::params![
            table_id.as_str(),
            document_id.to_string(),
            fields.to_string(),
            "{}",
            7_i64,
            7_i64
        ],
    )
    .await
    .expect("remote document insert should succeed");
}

async fn open_remote_namespace_database(
    config: &LibsqlReplicaProviderConfig,
    namespace: &str,
) -> nimbus_core::Result<Database> {
    let builder = Builder::new_remote(
        config.primary_url.clone(),
        config.auth_token.clone().unwrap_or_default(),
    )
    .namespace(namespace.to_string())
    .connector(libsql_transport_connector()?);
    builder.build().await.map_err(|error| {
        nimbus_core::Error::storage(nimbus_core::StorageErrorKind::Other, error.to_string())
    })
}
