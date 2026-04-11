use std::env;
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use neovex_core::{
    CronJob, CronSchedule, Mutation, ScheduledJob, ScheduledJobOutcome, ScheduledJobResult, Schema,
    SequenceNumber, TableName, TenantId, Timestamp,
};
use testcontainers_modules::{
    postgres,
    testcontainers::{ContainerAsync, runners::AsyncRunner},
};

use super::{
    DurableMutationRecord, Duration, PostgresProvider, PostgresProviderConfig, TableSchema,
    WriteOp, WriteOpType, timeout,
};
use crate::{ResolvedScheduleOp, ResolvedWrite};

const TEST_POSTGRES_URL_ENV: &str = "NEOVEX_TEST_POSTGRES_URL";
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
            Err(neovex_core::Error::AlreadyExists(_))
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
                    &neovex_core::DocumentId::new(),
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
        assert!(!schema_hint.journal_changed);
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
            id: scheduled_job.id,
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
async fn postgres_durable_journal_recovery_applies_pending_records() {
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
                    doc_id: first.id,
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
                    doc_id: second.id,
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
    let metadata_schema = format!("neovex_test_{}", &suffix[..24.min(suffix.len())]);
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

fn scheduled_insert_job(run_at: Timestamp, title: &str) -> ScheduledJob {
    ScheduledJob {
        id: neovex_core::DocumentId::new(),
        run_at,
        mutation: Mutation::Insert {
            table: TableName::new("tasks").expect("table name should build"),
            fields: serde_json::Map::from_iter([("title".to_string(), serde_json::json!(title))]),
        },
        created_at: Timestamp(100),
    }
}
