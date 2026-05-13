use std::env;
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use mysql_async::prelude::Queryable;
use mysql_async::{Opts, Pool};
use nimbus_core::{
    CollectionName, CronJob, CronSchedule, DocumentLocator, DocumentPath, Mutation,
    ResourcePathBinding, ScheduledJob, ScheduledJobOutcome, ScheduledJobResult, Schema,
    SequenceNumber, TableName, TableSchema, TenantId, Timestamp, WriteOp, WriteOpType,
};
use testcontainers_modules::{
    mysql,
    testcontainers::{ContainerAsync, runners::AsyncRunner},
};

use super::{
    DurableMutationRecord, Duration, FieldSchema, FieldType, MySqlProvider, MySqlProviderConfig,
    TenantReadStorage, implicit_external_provider_fixtures_disabled,
    require_explicit_external_provider_fixture_envs, timeout,
};
use crate::{ResolvedScheduleOp, ResolvedWrite};

const MYSQL_URL_ENV: &str = "NIMBUS_MYSQL_URL";
static TEST_SUFFIX_COUNTER: AtomicU64 = AtomicU64::new(0);

#[tokio::test(flavor = "multi_thread")]
async fn mysql_provider_manages_tenant_registry_and_databases() {
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
            created_alpha.database_name,
            provider
                .tenant_database_name(&alpha)
                .expect("tenant database should derive")
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
        assert_eq!(reopened.database_name, created_alpha.database_name);

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
async fn mysql_provider_reloads_registry_after_reconnect() {
    with_test_provider(|provider, config| async move {
        let tenant = TenantId::new("reload").expect("tenant id should build");
        let created = provider
            .create_tenant(&tenant)
            .await
            .expect("tenant should create");

        let reopened = MySqlProvider::connect(config)
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
                .database_name,
            created.database_name
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn mysql_opened_tenant_exposes_store_identity_and_read_storage() {
    with_test_provider(|provider, _config| async move {
        let tenant = TenantId::new("opened").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");

        assert_eq!(opened.store.tenant_id(), &tenant);
        assert_eq!(
            opened.store.database_name(),
            provider
                .tenant_database_name(&tenant)
                .expect("tenant database should derive")
        );
        assert_eq!(
            opened
                .read_storage
                .execute(|store| Ok((store.tenant_id().clone(), store.database_name().to_string())))
                .await
                .expect("read storage should execute"),
            (
                tenant.clone(),
                provider
                    .tenant_database_name(&tenant)
                    .expect("tenant database should derive")
            )
        );

        let reopened = provider
            .open_existing_opened_tenant(&tenant)
            .await
            .expect("tenant reopen should succeed")
            .expect("tenant should exist");
        assert_eq!(reopened.store.tenant_id(), &tenant);
        assert_eq!(reopened.store.database_name(), opened.store.database_name());
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn mysql_direct_writes_dedupe_and_journal_progress_round_trip() {
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
async fn mysql_resource_path_bindings_round_trip_without_table_name_delimiter_tricks() {
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
async fn mysql_trigger_delivery_cursor_round_trips_in_metadata() {
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
                23,
            )))
            .expect("cursor should persist");

        assert_eq!(
            opened
                .store
                .trigger_delivery_cursor()
                .expect("cursor should round trip"),
            nimbus_core::TriggerDeliveryCursor::new(SequenceNumber(23))
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn mysql_execution_unit_batch_and_scheduler_state_round_trip() {
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
async fn mysql_execution_unit_batch_persists_and_removes_resource_path_bindings_atomically() {
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
async fn mysql_durable_journal_recovery_applies_pending_records() {
    with_test_provider(|provider, _config| async move {
        let tenant = TenantId::new("recovery").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");
        let first = super::sample_document("tasks", "First");
        let second = super::sample_document("tasks", "Second");
        let records = vec![
            DurableMutationRecord::new(
                SequenceNumber(1),
                Timestamp(100),
                vec![WriteOp {
                    table: first.table.clone(),
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
async fn mysql_index_reads_round_trip_after_schema_write() {
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
            indexes: vec![nimbus_core::IndexDefinition {
                name: "by_team_status_rank".to_string(),
                fields: vec!["team".to_string(), "status".to_string(), "rank".to_string()],
            }],
            access_policy: None,
        };

        opened
            .store
            .replace_table_schema(&table_schema)
            .expect("schema write should succeed");

        let first = super::Document::new(
            table_schema.table.clone(),
            serde_json::Map::from_iter([
                ("team".to_string(), serde_json::json!("alpha")),
                ("status".to_string(), serde_json::json!("open")),
                ("rank".to_string(), serde_json::json!(1)),
            ]),
        );
        let second = super::Document::new(
            table_schema.table.clone(),
            serde_json::Map::from_iter([
                ("team".to_string(), serde_json::json!("alpha")),
                ("status".to_string(), serde_json::json!("open")),
                ("rank".to_string(), serde_json::json!(3)),
            ]),
        );
        let third = super::Document::new(
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
                Some(&serde_json::json!(2)),
                Some(&serde_json::json!(4)),
                true,
                false,
                &mut check_cancel,
            )
            .expect("composite range index scan should succeed");
        assert_eq!(ranged.len(), 1);
        assert_eq!(ranged[0].id, second.id);
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn mysql_schema_write_creates_and_drops_generated_index_columns() {
    with_test_provider(|provider, config| async move {
        let tenant = TenantId::new("schema").expect("tenant id should build");
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
                    required: false,
                },
                FieldSchema {
                    name: "rank".to_string(),
                    field_type: FieldType::Number,
                    required: false,
                },
            ],
            indexes: vec![nimbus_core::IndexDefinition {
                name: "by_team_status_rank".to_string(),
                fields: vec!["team".to_string(), "status".to_string(), "rank".to_string()],
            }],
            access_policy: None,
        };

        opened
            .store
            .replace_table_schema(&table_schema)
            .expect("schema write should succeed");
        let (generated_columns, secondary_indexes) =
            document_index_counts(&config.connection_string, opened.store.database_name()).await;
        assert_eq!(generated_columns, 3);
        assert_eq!(secondary_indexes, 1);

        opened
            .store
            .delete_table_schema(&table_schema.table)
            .expect("schema delete should succeed");
        let (generated_columns, secondary_indexes) =
            document_index_counts(&config.connection_string, opened.store.database_name()).await;
        assert_eq!(generated_columns, 0);
        assert_eq!(secondary_indexes, 0);
        assert_eq!(
            opened.store.load_schema().expect("schema should load"),
            Schema::default()
        );
    })
    .await;
}

async fn with_test_provider<F, Fut>(test: F)
where
    F: FnOnce(MySqlProvider, MySqlProviderConfig) -> Fut,
    Fut: Future<Output = ()>,
{
    let connection = match test_connection().await {
        Some(connection) => connection,
        None => return,
    };
    let suffix = unique_suffix();
    let metadata_database = format!("nimbus_meta_{}", &suffix[..16.min(suffix.len())]);
    let tenant_database_prefix = format!("tenant_{}_", &suffix[..12.min(suffix.len())]);
    let config = MySqlProviderConfig {
        connection_string: connection.connection_string().to_string(),
        metadata_database,
        tenant_database_prefix,
        min_connections: Some(1),
        max_connections: Some(4),
    };
    let provider = MySqlProvider::connect(config.clone())
        .await
        .expect("provider should connect");
    test(provider.clone(), config).await;
    provider
        .drop_provider_databases_for_test()
        .await
        .expect("test provider databases should drop");
    drop(connection);
}

enum TestConnection {
    External(String),
    Container {
        connection_string: String,
        _container: Box<ContainerAsync<mysql::Mysql>>,
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
    if let Ok(connection_string) = env::var(MYSQL_URL_ENV) {
        return Some(TestConnection::External(connection_string));
    }

    require_explicit_external_provider_fixture_envs("MySQL provider", &[MYSQL_URL_ENV]);
    if implicit_external_provider_fixtures_disabled("MySQL provider") {
        return None;
    }

    let container = match mysql::Mysql::default().start().await {
        Ok(container) => container,
        Err(error) => {
            eprintln!(
                "skipping mysql provider test because no explicit MySQL URL was provided and container startup failed: {error}"
            );
            return None;
        }
    };
    let host = container
        .get_host()
        .await
        .expect("container host should resolve");
    let port = container
        .get_host_port_ipv4(3306)
        .await
        .expect("container port should resolve");

    let url = format!("mysql://root@{host}:{port}/test");
    if timeout(Duration::from_secs(20), async {
        loop {
            if MySqlProvider::connect(MySqlProviderConfig::new(url.clone()))
                .await
                .is_ok()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    })
    .await
    .is_err()
    {
        eprintln!("skipping mysql provider test because the MySQL container never became ready");
        return None;
    }

    Some(TestConnection::Container {
        connection_string: url,
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

async fn document_index_counts(connection_string: &str, database_name: &str) -> (u64, u64) {
    let opts = Opts::from_url(connection_string).expect("connection string should parse");
    let pool = Pool::new(opts);
    let mut conn = pool.get_conn().await.expect("mysql connection should open");
    let generated_columns = conn
        .exec_first::<u64, _, _>(
            "SELECT COUNT(*) \
             FROM INFORMATION_SCHEMA.COLUMNS \
             WHERE TABLE_SCHEMA = ? \
               AND TABLE_NAME = 'documents' \
               AND COLUMN_NAME LIKE 'gcol\\_%' \
               AND EXTRA LIKE '%GENERATED%'",
            (database_name,),
        )
        .await
        .expect("generated column count should query")
        .expect("generated column count should return a row");
    let secondary_indexes = conn
        .exec_first::<u64, _, _>(
            "SELECT COUNT(DISTINCT INDEX_NAME) \
             FROM INFORMATION_SCHEMA.STATISTICS \
             WHERE TABLE_SCHEMA = ? \
               AND TABLE_NAME = 'documents' \
               AND INDEX_NAME LIKE 'idx\\_%'",
            (database_name,),
        )
        .await
        .expect("secondary index count should query")
        .expect("secondary index count should return a row");
    conn.disconnect()
        .await
        .expect("mysql connection should close");
    pool.disconnect().await.expect("mysql pool should close");
    (generated_columns, secondary_indexes)
}
