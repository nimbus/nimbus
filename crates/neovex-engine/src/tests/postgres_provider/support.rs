pub(super) use super::*;

use std::env;
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

pub(super) use neovex_core::{
    Document, DocumentId, Mutation, ScheduleRequest, ScheduledJobOutcome, Schema, Timestamp,
};
pub(super) use neovex_storage::{PostgresProvider, PostgresProviderConfig};
use testcontainers_modules::{
    postgres,
    testcontainers::{ContainerAsync, runners::AsyncRunner},
};
use tokio_postgres::NoTls;

use crate::{
    ControlPlaneConfig, LocalEncryptionConfig, PersistenceDialect, PersistenceTopology, PoolConfig,
    ProviderCredentials, TenantProviderConfig, TenantRoutingConfig,
};

const TEST_POSTGRES_URL_ENV: &str = "NEOVEX_TEST_POSTGRES_URL";
static TEST_SUFFIX_COUNTER: AtomicU64 = AtomicU64::new(0);

pub(super) async fn with_postgres_service_config<F, Fut>(test: F)
where
    F: FnOnce(ServicePersistenceConfig, PostgresProviderConfig) -> Fut,
    Fut: Future<Output = ()>,
{
    with_shared_postgres_service_configs(|service_config, _unused, provider_config| async move {
        test(service_config, provider_config).await;
    })
    .await;
}

pub(super) async fn with_shared_postgres_service_configs<F, Fut>(test: F)
where
    F: FnOnce(ServicePersistenceConfig, ServicePersistenceConfig, PostgresProviderConfig) -> Fut,
    Fut: Future<Output = ()>,
{
    let connection = match test_connection().await {
        Some(connection) => connection,
        None => return,
    };
    let suffix = unique_suffix();
    let metadata_schema = format!("neovex_test_{}", &suffix[..24.min(suffix.len())]);
    let tenant_schema_prefix = format!("tenant_{}_", &suffix[..12.min(suffix.len())]);
    let provider_config = PostgresProviderConfig {
        connection_string: connection.connection_string().to_string(),
        metadata_schema: metadata_schema.clone(),
        tenant_schema_prefix: tenant_schema_prefix.clone(),
        min_connections: Some(1),
        max_connections: Some(4),
    };
    let control_dir_a = tempdir().expect("first temporary control dir should create");
    let control_dir_b = tempdir().expect("second temporary control dir should create");
    let service_config_a = ServicePersistenceConfig {
        tenant_provider: TenantProviderConfig {
            dialect: PersistenceDialect::Postgres,
            topology: PersistenceTopology::ExternalPrimary,
            routing: TenantRoutingConfig::SchemaPerTenant {
                metadata_schema: metadata_schema.clone(),
                tenant_schema_prefix: tenant_schema_prefix.clone(),
            },
            pool: PoolConfig {
                min_connections: Some(1),
                max_connections: Some(4),
            },
            credentials: ProviderCredentials::ConnectionString(
                provider_config.connection_string.clone(),
            ),
        },
        control_plane: ControlPlaneConfig::embedded_redb(control_dir_a.path()),
        local_encryption: LocalEncryptionConfig::Disabled,
    };
    let service_config_b = ServicePersistenceConfig {
        tenant_provider: service_config_a.tenant_provider.clone(),
        control_plane: ControlPlaneConfig::embedded_redb(control_dir_b.path()),
        local_encryption: LocalEncryptionConfig::Disabled,
    };

    test(service_config_a, service_config_b, provider_config.clone()).await;

    PostgresProvider::connect(provider_config.clone())
        .await
        .expect("postgres provider should connect for cleanup")
        .drop_metadata_schema_for_test()
        .await
        .expect("test metadata schema should drop");
    drop(connection);
    drop(control_dir_a);
    drop(control_dir_b);
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

    require_explicit_external_provider_fixture_envs("Postgres engine", &[TEST_POSTGRES_URL_ENV]);

    let container = match postgres::Postgres::default().start().await {
        Ok(container) => container,
        Err(error) => {
            eprintln!(
                "skipping postgres engine test because no explicit Postgres URL was provided and container startup failed: {error}"
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

pub(super) async fn terminate_postgres_hint_listeners(
    config: &PostgresProviderConfig,
) -> neovex_core::Result<()> {
    let terminated = with_postgres_activity_client(
        config,
        PostgresProvider::notification_listener_application_name,
        |client, application_name| async move {
            client
                .execute(
                    "SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE application_name = $1",
                    &[&application_name],
                )
                .await
                .map_err(|error| neovex_core::Error::Internal(error.to_string()))
        },
    )
    .await?;
    assert!(
        terminated > 0,
        "expected at least one listener backend to terminate"
    );
    Ok(())
}

pub(super) async fn list_postgres_hint_listener_pids(
    config: &PostgresProviderConfig,
) -> neovex_core::Result<Vec<i32>> {
    with_postgres_activity_client(
        config,
        PostgresProvider::notification_listener_application_name,
        |client, application_name| async move {
            let rows = client
                .query(
                    "SELECT pid FROM pg_stat_activity WHERE application_name = $1 ORDER BY pid",
                    &[&application_name],
                )
                .await
                .map_err(|error| neovex_core::Error::Internal(error.to_string()))?;
            Ok(rows.into_iter().map(|row| row.get::<_, i32>(0)).collect())
        },
    )
    .await
}

pub(super) async fn terminate_postgres_pool_backends(
    config: &PostgresProviderConfig,
) -> neovex_core::Result<()> {
    let terminated = with_postgres_activity_client(
        config,
        PostgresProvider::pool_application_name,
        |client, application_name| async move {
            client
                .execute(
                    "SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE application_name = $1",
                    &[&application_name],
                )
                .await
                .map_err(|error| neovex_core::Error::Internal(error.to_string()))
        },
    )
    .await?;
    assert!(
        terminated > 0,
        "expected at least one pooled backend to terminate"
    );
    Ok(())
}

pub(super) async fn list_postgres_pool_backend_pids(
    config: &PostgresProviderConfig,
) -> neovex_core::Result<Vec<i32>> {
    with_postgres_activity_client(
        config,
        PostgresProvider::pool_application_name,
        |client, application_name| async move {
            let rows = client
                .query(
                    "SELECT pid FROM pg_stat_activity WHERE application_name = $1 ORDER BY pid",
                    &[&application_name],
                )
                .await
                .map_err(|error| neovex_core::Error::Internal(error.to_string()))?;
            Ok(rows.into_iter().map(|row| row.get::<_, i32>(0)).collect())
        },
    )
    .await
}

pub(super) async fn postgres_schema_exists(
    config: &PostgresProviderConfig,
    schema_name: &str,
) -> neovex_core::Result<bool> {
    let schema_name = schema_name.to_string();
    with_postgres_activity_client(
        config,
        PostgresProvider::pool_application_name,
        move |client, _application_name| async move {
            client
                .query_opt(
                    "SELECT 1 FROM information_schema.schemata WHERE schema_name = $1",
                    &[&schema_name],
                )
                .await
                .map(|row| row.is_some())
                .map_err(|error| neovex_core::Error::Internal(error.to_string()))
        },
    )
    .await
}

async fn with_postgres_activity_client<F, Fut, T>(
    config: &PostgresProviderConfig,
    application_name_selector: fn(&PostgresProvider) -> &str,
    action: F,
) -> neovex_core::Result<T>
where
    F: FnOnce(tokio_postgres::Client, String) -> Fut,
    Fut: Future<Output = neovex_core::Result<T>>,
{
    let provider = PostgresProvider::connect(config.clone()).await?;
    let application_name = application_name_selector(&provider).to_string();
    let (client, connection) = tokio_postgres::connect(&config.connection_string, NoTls)
        .await
        .map_err(|error| neovex_core::Error::Internal(error.to_string()))?;
    let connection_task = tokio::spawn(async move {
        let _ = connection.await;
    });
    let result = action(client, application_name).await;
    connection_task.abort();
    result
}

pub(super) fn unique_suffix() -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after unix epoch")
        .as_nanos();
    let counter = TEST_SUFFIX_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{counter:08x}{:x}{timestamp:x}", std::process::id())
}

pub(super) fn tasks_schema() -> TableSchema {
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
