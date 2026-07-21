pub(super) use super::*;

use std::env;
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

pub(super) use nimbus_core::{
    Document, DocumentId, Mutation, ScheduleRequest, ScheduledJobOutcome, Schema, Timestamp,
};
pub(super) use nimbus_storage::{PostgresProvider, PostgresProviderConfig};
use tokio_postgres::NoTls;

use crate::{
    ControlPlaneConfig, LocalEncryptionConfig, PersistenceDialect, PersistenceTopology, PoolConfig,
    ProviderCredentials, TenantProviderConfig, TenantRoutingConfig,
};

const TEST_POSTGRES_URL_ENV: &str = "NIMBUS_TEST_POSTGRES_URL";
static TEST_SUFFIX_COUNTER: AtomicU64 = AtomicU64::new(0);

pub(super) async fn with_postgres_engine_config<F, Fut>(test: F)
where
    F: FnOnce(EnginePersistenceConfig, PostgresProviderConfig) -> Fut,
    Fut: Future<Output = ()>,
{
    with_shared_postgres_engine_configs(|engine_config, _unused, provider_config| async move {
        test(engine_config, provider_config).await;
    })
    .await;
}

pub(super) async fn with_shared_postgres_engine_configs<F, Fut>(test: F)
where
    F: FnOnce(EnginePersistenceConfig, EnginePersistenceConfig, PostgresProviderConfig) -> Fut,
    Fut: Future<Output = ()>,
{
    let connection = match test_connection().await {
        Some(connection) => connection,
        None => return,
    };
    let suffix = unique_suffix();
    let metadata_schema = format!("nimbus_test_{}", &suffix[..24.min(suffix.len())]);
    let tenant_schema_prefix = format!("tenant_{}_", &suffix[..12.min(suffix.len())]);
    let provider_config = PostgresProviderConfig {
        connection_string: connection,
        metadata_schema: metadata_schema.clone(),
        tenant_schema_prefix: tenant_schema_prefix.clone(),
        min_connections: Some(1),
        max_connections: Some(4),
    };
    let control_dir_a = tempdir().expect("first temporary control dir should create");
    let control_dir_b = tempdir().expect("second temporary control dir should create");
    let engine_config_a = EnginePersistenceConfig {
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
    let engine_config_b = EnginePersistenceConfig {
        tenant_provider: engine_config_a.tenant_provider.clone(),
        control_plane: ControlPlaneConfig::embedded_redb(control_dir_b.path()),
        local_encryption: LocalEncryptionConfig::Disabled,
    };

    test(engine_config_a, engine_config_b, provider_config.clone()).await;

    PostgresProvider::connect(provider_config.clone())
        .await
        .expect("postgres provider should connect for cleanup")
        .drop_metadata_schema_for_test()
        .await
        .expect("test metadata schema should drop");
    drop(control_dir_a);
    drop(control_dir_b);
}

async fn test_connection() -> Option<String> {
    match external_provider_fixture_mode(
        "postgres",
        "PostgreSQL engine provider",
        &[TEST_POSTGRES_URL_ENV],
    ) {
        ExternalProviderFixtureMode::UseExplicit => Some(
            env::var(TEST_POSTGRES_URL_ENV)
                .expect("fixture policy should require the PostgreSQL URL"),
        ),
        ExternalProviderFixtureMode::Omit => None,
    }
}

pub(super) async fn terminate_postgres_hint_listeners(
    config: &PostgresProviderConfig,
) -> nimbus_core::Result<()> {
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
                .map_err(|error| nimbus_core::Error::Internal(error.to_string()))
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
) -> nimbus_core::Result<Vec<i32>> {
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
                .map_err(|error| nimbus_core::Error::Internal(error.to_string()))?;
            Ok(rows.into_iter().map(|row| row.get::<_, i32>(0)).collect())
        },
    )
    .await
}

pub(super) async fn terminate_postgres_pool_backends(
    config: &PostgresProviderConfig,
) -> nimbus_core::Result<()> {
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
                .map_err(|error| nimbus_core::Error::Internal(error.to_string()))
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
) -> nimbus_core::Result<Vec<i32>> {
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
                .map_err(|error| nimbus_core::Error::Internal(error.to_string()))?;
            Ok(rows.into_iter().map(|row| row.get::<_, i32>(0)).collect())
        },
    )
    .await
}

pub(super) async fn postgres_schema_exists(
    config: &PostgresProviderConfig,
    schema_name: &str,
) -> nimbus_core::Result<bool> {
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
                .map_err(|error| nimbus_core::Error::Internal(error.to_string()))
        },
    )
    .await
}

pub(super) async fn expire_postgres_committer_lease(
    config: &PostgresProviderConfig,
    tenant_id: &TenantId,
) -> nimbus_core::Result<()> {
    let provider = PostgresProvider::connect(config.clone()).await?;
    let schema_name = provider.tenant_schema_name(tenant_id)?;
    let (client, connection) = tokio_postgres::connect(&config.connection_string, NoTls)
        .await
        .map_err(|error| nimbus_core::Error::Internal(error.to_string()))?;
    let connection_task = tokio::spawn(async move {
        let _ = connection.await;
    });
    let query = format!(
        "UPDATE \"{schema_name}\".\"committer_lease\" \
         SET expires_at = CURRENT_TIMESTAMP - INTERVAL '1 second' \
         WHERE singleton = TRUE"
    );
    let updated = client
        .execute(query.as_str(), &[])
        .await
        .map_err(|error| nimbus_core::Error::Internal(error.to_string()));
    connection_task.abort();
    let updated = updated?;
    if updated != 1 {
        return Err(nimbus_core::Error::Internal(format!(
            "expected one committer lease row to expire, updated {updated}"
        )));
    }
    Ok(())
}

async fn with_postgres_activity_client<F, Fut, T>(
    config: &PostgresProviderConfig,
    application_name_selector: fn(&PostgresProvider) -> &str,
    action: F,
) -> nimbus_core::Result<T>
where
    F: FnOnce(tokio_postgres::Client, String) -> Fut,
    Fut: Future<Output = nimbus_core::Result<T>>,
{
    let provider = PostgresProvider::connect(config.clone()).await?;
    let application_name = application_name_selector(&provider).to_string();
    let (client, connection) = tokio_postgres::connect(&config.connection_string, NoTls)
        .await
        .map_err(|error| nimbus_core::Error::Internal(error.to_string()))?;
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
