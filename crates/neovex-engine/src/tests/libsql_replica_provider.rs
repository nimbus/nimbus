use std::env;
use std::future::Future;
use std::net::TcpListener;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use libsql::{Builder, Database};
use neovex_core::{
    DocumentId, FieldSchema, FieldType, Mutation, ScheduleRequest, ScheduledJobOutcome,
    SequenceNumber, TableSchema, TenantId, Timestamp,
};
use neovex_storage::libsql::libsql_transport_connector;
use neovex_storage::{LibsqlReplicaProvider, LibsqlReplicaProviderConfig};
use testcontainers_modules::testcontainers::core::{IntoContainerPort, WaitFor};
use testcontainers_modules::testcontainers::{
    ContainerAsync, GenericImage, ImageExt, runners::AsyncRunner,
};

use super::*;
use crate::{
    ControlPlaneConfig, LibsqlReplicaBarrierPath, LibsqlReplicaRefreshPath, PersistenceDialect,
    PersistenceTopology, PoolConfig, ProviderCredentials, TenantProviderConfig,
    TenantRoutingConfig,
};

const LIBSQL_URL_ENV: &str = "NEOVEX_LIBSQL_URL";
const LIBSQL_AUTH_TOKEN_ENV: &str = "NEOVEX_LIBSQL_AUTH_TOKEN";
const LIBSQL_ADMIN_URL_ENV: &str = "NEOVEX_LIBSQL_ADMIN_URL";
const LIBSQL_ADMIN_AUTH_HEADER_ENV: &str = "NEOVEX_LIBSQL_ADMIN_AUTH_HEADER";
static TEST_SUFFIX_COUNTER: AtomicU64 = AtomicU64::new(0);

#[tokio::test(flavor = "multi_thread")]
#[serial_test::serial(libsql_replica_provider)]
async fn typed_libsql_replica_config_reads_seeded_remote_state_and_reopens() {
    with_shared_libsql_replica_service_configs(
        |service_config_a, service_config_b, provider_config| async move {
            let provider = LibsqlReplicaProvider::connect(provider_config.clone())
                .await
                .expect("replica provider should connect");
            let tenant_id = TenantId::new("libsql-replica-tenant").expect("tenant id should build");
            let registration = provider
                .create_tenant(&tenant_id)
                .await
                .expect("tenant should create through provider");
            let document_id = DocumentId::new();
            seed_remote_namespace(
                &provider_config,
                &registration.namespace,
                &tasks_schema(),
                document_id,
                serde_json::json!({
                    "title": "from-primary"
                }),
            )
            .await;
            drop(provider);

            let service = Arc::new(
                Service::new_with_persistence_config(service_config_a.clone())
                    .await
                    .expect("replica-backed service should create"),
            );
            assert_eq!(
                service
                    .list_tenants_async()
                    .await
                    .expect("tenant list should load from provider metadata"),
                vec![tenant_id.clone()]
            );
            service
                .ensure_tenant_exists_async(tenant_id.clone())
                .await
                .expect("tenant should lazy load through the replica provider");
            let documents = service
                .query_documents_async(tenant_id.clone(), query_for("tasks"))
                .await
                .expect("replica-backed query should succeed");
            assert_eq!(documents.len(), 1);
            assert_eq!(documents[0].id, document_id);
            assert_eq!(
                documents[0]
                    .fields
                    .get("title")
                    .and_then(|value| value.as_str()),
                Some("from-primary")
            );

            service.quiesce().await;
            drop(service);

            let reopened = Arc::new(
                Service::new_with_persistence_config(service_config_b)
                    .await
                    .expect("replica-backed service should reopen"),
            );
            let reopened_documents = reopened
                .query_documents_async(tenant_id.clone(), query_for("tasks"))
                .await
                .expect("reopened replica-backed query should succeed");
            assert_eq!(reopened_documents.len(), 1);
            assert_eq!(reopened_documents[0].id, document_id);
            reopened.quiesce().await;
        },
    )
    .await;
}

#[tokio::test(flavor = "multi_thread")]
#[serial_test::serial(libsql_replica_provider)]
async fn typed_libsql_replica_config_supports_async_schema_mutation_journal_and_scheduler_paths() {
    with_libsql_replica_service_config(|service_config, _provider_config| async move {
        let tenant_id = TenantId::new("libsql-replica-mutations").expect("tenant id should build");
        let service = Arc::new(
            Service::new_with_persistence_config(service_config)
                .await
                .expect("replica-backed service should create"),
        );

        service
            .create_tenant_async(tenant_id.clone())
            .await
            .expect("tenant should create");
        service
            .set_table_schema_async(tenant_id.clone(), tasks_schema())
            .await
            .expect("schema write should succeed");

        let inserted_id = service
            .insert_document_async(
                tenant_id.clone(),
                tasks_table(),
                serde_json::Map::from_iter([("title".to_string(), json!("First"))]),
            )
            .await
            .expect("insert should succeed");
        service
            .update_document_async(
                tenant_id.clone(),
                tasks_table(),
                inserted_id,
                serde_json::Map::from_iter([("title".to_string(), json!("Renamed"))]),
            )
            .await
            .expect("update should succeed");

        let scheduled_job_id = service
            .schedule_mutation_async(
                tenant_id.clone(),
                ScheduleRequest {
                    run_after_ms: 5_000,
                    mutation: Mutation::Insert {
                        table: tasks_table(),
                        fields: serde_json::Map::from_iter([(
                            "title".to_string(),
                            json!("Scheduled"),
                        )]),
                    },
                },
            )
            .await
            .expect("scheduled mutation should persist");
        assert_eq!(
            service
                .list_scheduled_jobs_async(tenant_id.clone())
                .await
                .expect("pending jobs should load")
                .len(),
            1
        );

        let claimed = service
            .claim_due_jobs_async(tenant_id.clone(), Timestamp(u64::MAX))
            .await
            .expect("claim should succeed");
        assert_eq!(claimed.len(), 1);
        service
            .record_scheduled_job_result_async(
                tenant_id.clone(),
                neovex_core::ScheduledJobResult {
                    id: scheduled_job_id,
                    run_at: claimed[0].run_at,
                    finished_at: Timestamp(claimed[0].run_at.0.saturating_add(1)),
                    mutation: claimed[0].mutation.clone(),
                    outcome: ScheduledJobOutcome::Completed,
                    error: None,
                },
            )
            .await
            .expect("scheduled result should persist");
        service
            .complete_scheduled_job_async(tenant_id.clone(), scheduled_job_id)
            .await
            .expect("scheduled completion should persist");
        assert_eq!(
            service
                .get_scheduled_job_result_async(tenant_id.clone(), scheduled_job_id)
                .await
                .expect("scheduled result should load")
                .outcome,
            ScheduledJobOutcome::Completed
        );

        let documents = service
            .query_documents_async(tenant_id.clone(), query_for("tasks"))
            .await
            .expect("query should succeed");
        assert_eq!(documents.len(), 1);
        assert_eq!(
            documents[0]
                .fields
                .get("title")
                .and_then(|value| value.as_str()),
            Some("Renamed")
        );
        assert_eq!(
            service
                .latest_sequence_async(tenant_id.clone())
                .await
                .expect("latest sequence should track journaled mutations"),
            SequenceNumber(2)
        );

        let bootstrap = service
            .export_durable_journal_bootstrap_async(tenant_id.clone())
            .await
            .expect("bootstrap should export");
        assert_eq!(bootstrap.bootstrap_cut, SequenceNumber(2));
        assert_eq!(bootstrap.resume_after, SequenceNumber(2));

        service.quiesce().await;
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
#[serial_test::serial(libsql_replica_provider)]
async fn libsql_replica_background_poll_refreshes_loaded_runtime_schema_and_journal_state() {
    with_shared_libsql_replica_service_configs(
        |service_config_a, service_config_b, _provider_config| async move {
            let tenant_id =
                TenantId::new("libsql-replica-poll-journal").expect("tenant id should build");
            let service_a = Arc::new(
                Service::new_with_persistence_config(service_config_a)
                    .await
                    .expect("first replica-backed service should create"),
            );
            let service_b = Arc::new(
                Service::new_with_persistence_config(service_config_b)
                    .await
                    .expect("second replica-backed service should create"),
            );

            service_a
                .create_tenant_async(tenant_id.clone())
                .await
                .expect("tenant should create");
            service_b
                .ensure_tenant_exists_async(tenant_id.clone())
                .await
                .expect("second service should load tenant");
            assert_eq!(
                service_b
                    .get_schema_async(tenant_id.clone())
                    .await
                    .expect("empty schema should load"),
                neovex_core::Schema::default()
            );

            service_a
                .set_table_schema_async(tenant_id.clone(), tasks_schema())
                .await
                .expect("schema write should succeed");
            service_a
                .insert_document_async(
                    tenant_id.clone(),
                    tasks_table(),
                    serde_json::Map::from_iter([("title".to_string(), json!("External"))]),
                )
                .await
                .expect("insert should succeed");

            wait_for_value(
                "replica poll should refresh loaded schema",
                Duration::from_secs(3),
                Duration::from_millis(25),
                || {
                    let service = service_b.clone();
                    let tenant_id = tenant_id.clone();
                    async move {
                        service
                            .get_schema_async(tenant_id)
                            .await
                            .expect("schema should load")
                    }
                },
                |schema| schema.get_table(&tasks_table()).is_some(),
            )
            .await;
            wait_for_mutation_journal_stats(
                &service_b,
                &tenant_id,
                "replica poll should catch up journal heads",
                |stats| {
                    stats.durable_head == SequenceNumber(1)
                        && stats.applied_head == SequenceNumber(1)
                },
            )
            .await;

            let documents = service_b
                .query_documents_async(tenant_id.clone(), query_for("tasks"))
                .await
                .expect("caught-up query should succeed");
            assert_eq!(documents.len(), 1);
            assert_eq!(
                documents[0]
                    .fields
                    .get("title")
                    .and_then(|value| value.as_str()),
                Some("External")
            );
            let diagnostics = service_b
                .tenant_engine_diagnostics_async(tenant_id.clone())
                .await
                .expect("tenant diagnostics should surface replica freshness");
            let freshness = diagnostics
                .libsql_replica_freshness
                .expect("libsql-replica diagnostics should include freshness stats");
            assert_eq!(freshness.required_sequence, SequenceNumber(1));
            assert_eq!(freshness.local_applied_sequence, SequenceNumber(1));
            assert_eq!(freshness.refresh_error_count, 0);
            assert!(
                freshness.incremental_refresh_count
                    + freshness.full_snapshot_refresh_count
                    + freshness.incremental_fallback_to_snapshot_count
                    >= 1,
                "replica diagnostics should show at least one refresh path"
            );
            assert_ne!(
                freshness.last_barrier_path,
                LibsqlReplicaBarrierPath::Unknown
            );
            assert_ne!(
                freshness.last_refresh_path,
                LibsqlReplicaRefreshPath::Unknown
            );

            service_a.quiesce().await;
            service_b.quiesce().await;
        },
    )
    .await;
}

#[tokio::test(flavor = "multi_thread")]
#[serial_test::serial(libsql_replica_provider)]
async fn libsql_replica_background_poll_loads_unloaded_tenants_with_scheduled_work() {
    with_shared_libsql_replica_service_configs(
        |service_config_a, service_config_b, _provider_config| async move {
            let tenant_id =
                TenantId::new("libsql-replica-poll-scheduler").expect("tenant id should build");
            let service_a = Arc::new(
                Service::new_with_persistence_config(service_config_a)
                    .await
                    .expect("first replica-backed service should create"),
            );
            let service_b = Arc::new(
                Service::new_with_persistence_config(service_config_b)
                    .await
                    .expect("second replica-backed service should create"),
            );

            service_b
                .load_tenants_with_scheduled_work_async()
                .await
                .expect("initial scheduled-work preload should succeed");
            service_a
                .create_tenant_async(tenant_id.clone())
                .await
                .expect("tenant should create");

            let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
            let scheduler_handle =
                tokio::spawn(crate::run_scheduler(service_b.clone(), shutdown_rx));
            service_a
                .schedule_mutation_async(
                    tenant_id.clone(),
                    ScheduleRequest {
                        run_after_ms: 0,
                        mutation: Mutation::Insert {
                            table: tasks_table(),
                            fields: serde_json::Map::from_iter([(
                                "title".to_string(),
                                json!("Scheduled externally"),
                            )]),
                        },
                    },
                )
                .await
                .expect("scheduled mutation should persist");

            wait_for_value(
                "replica poll should load the scheduled tenant into the second service",
                Duration::from_secs(3),
                Duration::from_millis(25),
                || {
                    let service = service_b.clone();
                    async move { service.loaded_tenant_ids() }
                },
                |tenant_ids| tenant_ids.contains(&tenant_id),
            )
            .await;
            wait_for_value(
                "replica poll should execute scheduled work on the second service",
                Duration::from_secs(3),
                Duration::from_millis(25),
                || {
                    let service = service_b.clone();
                    let tenant_id = tenant_id.clone();
                    async move {
                        service
                            .query_documents_async(tenant_id, query_for("tasks"))
                            .await
                            .expect("query should succeed")
                    }
                },
                |documents| {
                    documents.iter().any(|document| {
                        document
                            .fields
                            .get("title")
                            .and_then(|value| value.as_str())
                            == Some("Scheduled externally")
                    })
                },
            )
            .await;

            let _ = shutdown_tx.send(true);
            scheduler_handle.await.expect("scheduler should shut down");
            service_a.quiesce().await;
            service_b.quiesce().await;
        },
    )
    .await;
}

async fn with_libsql_replica_service_config<F, Fut>(test: F)
where
    F: FnOnce(ServicePersistenceConfig, LibsqlReplicaProviderConfig) -> Fut,
    Fut: Future<Output = ()>,
{
    with_shared_libsql_replica_service_configs(
        |service_config, _unused, provider_config| async move {
            test(service_config, provider_config).await;
        },
    )
    .await;
}

async fn with_shared_libsql_replica_service_configs<F, Fut>(test: F)
where
    F: FnOnce(
        ServicePersistenceConfig,
        ServicePersistenceConfig,
        LibsqlReplicaProviderConfig,
    ) -> Fut,
    Fut: Future<Output = ()>,
{
    let connection = match test_connection().await {
        Some(connection) => connection,
        None => return,
    };
    let suffix = unique_suffix();
    let metadata_namespace = format!("neovex_meta_{}", &suffix[..16.min(suffix.len())]);
    let tenant_namespace_prefix = format!("tenant_{}_", &suffix[..12.min(suffix.len())]);
    let replica_cache_dir_a = tempdir().expect("first replica cache dir should create");
    let replica_cache_dir_b = tempdir().expect("second replica cache dir should create");
    let control_dir_a = tempdir().expect("first control tempdir should build");
    let control_dir_b = tempdir().expect("second control tempdir should build");

    let provider_config = LibsqlReplicaProviderConfig {
        primary_url: connection.primary_url().to_string(),
        auth_token: connection.auth_token().map(ToOwned::to_owned),
        admin_api_url: connection.admin_api_url().to_string(),
        admin_auth_header: connection.admin_auth_header().map(ToOwned::to_owned),
        metadata_namespace: metadata_namespace.clone(),
        tenant_namespace_prefix: tenant_namespace_prefix.clone(),
        replica_cache_dir: replica_cache_dir_a.path().to_path_buf(),
    };

    let service_config_a = ServicePersistenceConfig {
        tenant_provider: TenantProviderConfig {
            dialect: PersistenceDialect::Sqlite,
            topology: PersistenceTopology::ExternalPrimaryWithReplicas,
            routing: TenantRoutingConfig::NamespacePerTenant {
                metadata_namespace: metadata_namespace.clone(),
                tenant_namespace_prefix: tenant_namespace_prefix.clone(),
                replica_cache_dir: replica_cache_dir_a.path().to_path_buf(),
            },
            pool: PoolConfig::default(),
            credentials: ProviderCredentials::LibsqlReplica {
                primary_url: connection.primary_url().to_string(),
                auth_token: connection.auth_token().map(ToOwned::to_owned),
                admin_api_url: connection.admin_api_url().to_string(),
                admin_auth_header: connection.admin_auth_header().map(ToOwned::to_owned),
            },
        },
        control_plane: ControlPlaneConfig::embedded_redb(control_dir_a.path()),
    };
    let service_config_b = ServicePersistenceConfig {
        tenant_provider: TenantProviderConfig {
            routing: TenantRoutingConfig::NamespacePerTenant {
                metadata_namespace,
                tenant_namespace_prefix,
                replica_cache_dir: replica_cache_dir_b.path().to_path_buf(),
            },
            ..service_config_a.tenant_provider.clone()
        },
        control_plane: ControlPlaneConfig::embedded_redb(control_dir_b.path()),
    };

    test(service_config_a, service_config_b, provider_config.clone()).await;

    LibsqlReplicaProvider::connect(provider_config)
        .await
        .expect("cleanup provider should connect")
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
                "{LIBSQL_ADMIN_URL_ENV} is required when {LIBSQL_URL_ENV} is set for libsql-replica engine tests"
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
        "libsql replica engine",
        &[LIBSQL_URL_ENV, LIBSQL_ADMIN_URL_ENV],
    );

    let image = GenericImage::new("ghcr.io/tursodatabase/libsql-server", "latest")
        .with_wait_for(WaitFor::seconds(1))
        // The container entrypoint already appends --http-listen-addr from
        // SQLD_HTTP_LISTEN_ADDR, so the harness only overrides the admin bind
        // and feature flags here.
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
                "skipping libsql-replica engine test because no explicit libsql URL was provided and container startup failed: {error}"
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
        eprintln!(
            "skipping libsql-replica engine test because the libsql container never became ready"
        );
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

fn allocate_host_port() -> u16 {
    TcpListener::bind(("127.0.0.1", 0))
        .expect("temporary port probe should bind")
        .local_addr()
        .expect("temporary port probe should resolve")
        .port()
}

fn tasks_schema() -> TableSchema {
    TableSchema {
        table: tasks_table(),
        fields: vec![FieldSchema {
            name: "title".to_string(),
            field_type: FieldType::String,
            required: true,
        }],
        indexes: Vec::new(),
        access_policy: None,
    }
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
