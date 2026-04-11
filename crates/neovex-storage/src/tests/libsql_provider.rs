use std::env;
use std::future::Future;
use std::net::TcpListener;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use libsql::{Builder, Database};
use neovex_core::{
    CronJob, CronSchedule, DocumentId, DurableMutationRecord, FieldSchema, FieldType,
    IndexDefinition, Mutation, ScheduledJob, ScheduledJobOutcome, ScheduledJobResult,
    SequenceNumber, TableName, TableSchema, TenantId, Timestamp, WriteOp, WriteOpType,
};
use serial_test::serial;
use testcontainers_modules::testcontainers::core::{IntoContainerPort, WaitFor};
use testcontainers_modules::testcontainers::{
    ContainerAsync, GenericImage, ImageExt, runners::AsyncRunner,
};

use super::{Duration, LibsqlReplicaProvider, LibsqlReplicaProviderConfig, tempdir, timeout};
use crate::async_storage::TenantReadStorage;
use crate::libsql::libsql_transport_connector;
use crate::{
    LibsqlReplicaBarrierPath, LibsqlReplicaRefreshCause, LibsqlReplicaRefreshPath,
    ResolvedScheduleOp, ResolvedWrite,
};

const LIBSQL_URL_ENV: &str = "NEOVEX_LIBSQL_URL";
const LIBSQL_AUTH_TOKEN_ENV: &str = "NEOVEX_LIBSQL_AUTH_TOKEN";
const LIBSQL_ADMIN_URL_ENV: &str = "NEOVEX_LIBSQL_ADMIN_URL";
const LIBSQL_ADMIN_AUTH_HEADER_ENV: &str = "NEOVEX_LIBSQL_ADMIN_AUTH_HEADER";
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
            document_id,
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
    let metadata_namespace = format!("neovex_meta_{}", &suffix[..16.min(suffix.len())]);
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
    conn.execute(
        "INSERT INTO documents (table_name, id, data_json, creation_time) VALUES (?, ?, ?, ?)",
        libsql::params![
            table_schema.table.as_str(),
            document_id.to_string(),
            fields.to_string(),
            7_i64
        ],
    )
    .await
    .expect("remote document insert should succeed");
}

async fn open_remote_namespace_database(
    config: &LibsqlReplicaProviderConfig,
    namespace: &str,
) -> neovex_core::Result<Database> {
    let builder = Builder::new_remote(
        config.primary_url.clone(),
        config.auth_token.clone().unwrap_or_default(),
    )
    .namespace(namespace.to_string())
    .connector(libsql_transport_connector()?);
    builder.build().await.map_err(|error| {
        neovex_core::Error::storage(neovex_core::StorageErrorKind::Other, error.to_string())
    })
}
