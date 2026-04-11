use std::env;
use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use neovex_core::{Mutation, ScheduleRequest, ScheduledJobOutcome};
use neovex_storage::{MySqlProvider, MySqlProviderConfig};
use testcontainers_modules::{
    mysql,
    testcontainers::{ContainerAsync, runners::AsyncRunner},
};

use super::*;
use crate::{
    ControlPlaneConfig, PersistenceDialect, PersistenceTopology, PoolConfig, ProviderCredentials,
    TenantProviderConfig, TenantRoutingConfig,
};

const MYSQL_URL_ENV: &str = "NEOVEX_MYSQL_URL";
static TEST_SUFFIX_COUNTER: AtomicU64 = AtomicU64::new(0);

#[tokio::test(flavor = "multi_thread")]
#[serial_test::serial(mysql_provider)]
async fn typed_mysql_config_supports_async_tenant_lifecycle_and_empty_read_paths() {
    with_mysql_service_config(|service_config, _provider_config| async move {
        let tenant_id = TenantId::new("mysql-tenant").expect("tenant id should build");
        let service = Arc::new(
            Service::new_with_persistence_config(service_config.clone())
                .await
                .expect("mysql-backed service should create"),
        );

        service
            .create_tenant_async(tenant_id.clone())
            .await
            .expect("tenant should create");
        assert_eq!(
            service
                .list_tenants_async()
                .await
                .expect("tenant list should load"),
            vec![tenant_id.clone()]
        );
        service
            .ensure_tenant_exists_async(tenant_id.clone())
            .await
            .expect("tenant existence should verify");
        assert_eq!(
            service
                .latest_sequence_async(tenant_id.clone())
                .await
                .expect("latest sequence should read"),
            SequenceNumber(0)
        );
        assert!(
            service
                .query_documents_async(tenant_id.clone(), query_for("tasks"))
                .await
                .expect("empty query should succeed")
                .is_empty()
        );
        let bootstrap = service
            .export_durable_journal_bootstrap_async(tenant_id.clone())
            .await
            .expect("bootstrap should export");
        assert_eq!(bootstrap.bootstrap_cut, SequenceNumber(0));
        assert_eq!(bootstrap.resume_after, SequenceNumber(0));
        assert_eq!(bootstrap.cursor_floor, SequenceNumber(0));
        assert!(bootstrap.snapshot.documents.is_empty());

        service.quiesce().await;
        drop(service);

        let reopened = Arc::new(
            Service::new_with_persistence_config(service_config)
                .await
                .expect("mysql-backed service should reopen"),
        );
        assert_eq!(
            reopened
                .list_tenants_async()
                .await
                .expect("reopened tenant list should load"),
            vec![tenant_id.clone()]
        );
        reopened
            .ensure_tenant_exists_async(tenant_id.clone())
            .await
            .expect("tenant should lazy load after reopen");
        assert!(
            reopened
                .query_documents_async(tenant_id.clone(), query_for("tasks"))
                .await
                .expect("reopened empty query should succeed")
                .is_empty()
        );
        reopened.quiesce().await;
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
#[serial_test::serial(mysql_provider)]
async fn typed_mysql_config_supports_async_schema_mutation_journal_and_scheduler_paths() {
    with_mysql_service_config(|service_config, _provider_config| async move {
        let tenant_id = TenantId::new("mysql-mutations").expect("tenant id should build");
        let service = Arc::new(
            Service::new_with_persistence_config(service_config)
                .await
                .expect("mysql-backed service should create"),
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
#[serial_test::serial(mysql_provider)]
async fn mysql_background_poll_refreshes_loaded_runtime_schema_and_journal_state() {
    with_shared_mysql_service_configs(
        |service_config_a, service_config_b, _provider_config| async move {
            let tenant_id = TenantId::new("mysql-poll-journal").expect("tenant id should build");
            let service_a = Arc::new(
                Service::new_with_persistence_config(service_config_a)
                    .await
                    .expect("first mysql-backed service should create"),
            );
            let service_b = Arc::new(
                Service::new_with_persistence_config(service_config_b)
                    .await
                    .expect("second mysql-backed service should create"),
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
                "mysql poll should refresh loaded schema",
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
                "mysql poll should catch up journal heads",
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

            service_a.quiesce().await;
            service_b.quiesce().await;
        },
    )
    .await;
}

#[tokio::test(flavor = "multi_thread")]
#[serial_test::serial(mysql_provider)]
async fn mysql_background_poll_loads_unloaded_tenants_with_scheduled_work() {
    with_shared_mysql_service_configs(
        |service_config_a, service_config_b, _provider_config| async move {
            let tenant_id = TenantId::new("mysql-poll-scheduler").expect("tenant id should build");
            let service_a = Arc::new(
                Service::new_with_persistence_config(service_config_a)
                    .await
                    .expect("first mysql-backed service should create"),
            );
            let service_b = Arc::new(
                Service::new_with_persistence_config(service_config_b)
                    .await
                    .expect("second mysql-backed service should create"),
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
                "mysql poll should load the scheduled tenant into the second service",
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
                "mysql poll should execute scheduled work on the second service",
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

async fn with_mysql_service_config<F, Fut>(test: F)
where
    F: FnOnce(ServicePersistenceConfig, MySqlProviderConfig) -> Fut,
    Fut: Future<Output = ()>,
{
    with_shared_mysql_service_configs(|service_config, _unused, provider_config| async move {
        test(service_config, provider_config).await;
    })
    .await;
}

async fn with_shared_mysql_service_configs<F, Fut>(test: F)
where
    F: FnOnce(ServicePersistenceConfig, ServicePersistenceConfig, MySqlProviderConfig) -> Fut,
    Fut: Future<Output = ()>,
{
    let connection = match test_connection().await {
        Some(connection) => connection,
        None => return,
    };
    let suffix = unique_suffix();
    let metadata_database = format!("neovex_meta_{}", &suffix[..16.min(suffix.len())]);
    let tenant_database_prefix = format!("tenant_{}_", &suffix[..12.min(suffix.len())]);
    let provider_config = MySqlProviderConfig {
        connection_string: connection.connection_string().to_string(),
        metadata_database: metadata_database.clone(),
        tenant_database_prefix: tenant_database_prefix.clone(),
        min_connections: Some(1),
        max_connections: Some(4),
    };
    let control_dir_a = tempdir().expect("first control tempdir should build");
    let control_dir_b = tempdir().expect("second control tempdir should build");
    let service_config_a = ServicePersistenceConfig {
        tenant_provider: TenantProviderConfig {
            dialect: PersistenceDialect::MySql,
            topology: PersistenceTopology::ExternalPrimary,
            routing: TenantRoutingConfig::DatabasePerTenant {
                metadata_database: metadata_database.clone(),
                tenant_database_prefix: tenant_database_prefix.clone(),
            },
            pool: PoolConfig {
                min_connections: Some(1),
                max_connections: Some(4),
            },
            credentials: ProviderCredentials::ConnectionString(
                connection.connection_string().to_string(),
            ),
        },
        control_plane: ControlPlaneConfig::embedded_redb(control_dir_a.path()),
    };
    let service_config_b = ServicePersistenceConfig {
        tenant_provider: service_config_a.tenant_provider.clone(),
        control_plane: ControlPlaneConfig::embedded_redb(control_dir_b.path()),
    };

    test(service_config_a, service_config_b, provider_config.clone()).await;

    MySqlProvider::connect(provider_config)
        .await
        .expect("cleanup provider should connect")
        .drop_provider_databases_for_test()
        .await
        .expect("provider databases should drop");
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

    let container = match mysql::Mysql::default().start().await {
        Ok(container) => container,
        Err(error) => {
            eprintln!(
                "skipping mysql engine test because no explicit MySQL URL was provided and container startup failed: {error}"
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
        eprintln!("skipping mysql engine test because the MySQL container never became ready");
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
