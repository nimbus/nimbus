pub(super) use std::env;
pub(super) use std::future::Future;
pub(super) use std::sync::Arc;
pub(super) use std::sync::atomic::{AtomicU64, Ordering};
pub(super) use std::time::{SystemTime, UNIX_EPOCH};

pub(super) use super::super::provider_support::*;
pub(super) use super::super::{
    Duration, ExternalProviderFixtureMode, LibsqlReplicaProvider, LibsqlReplicaProviderConfig,
    SqliteTenantStore, external_provider_fixture_mode, tempdir, timeout,
};
pub(super) use crate::async_storage::TenantReadStorage;
pub(super) use crate::async_storage::TenantWriteStorage;
pub(super) use crate::libsql::libsql_transport_connector;
pub(super) use crate::tests::BlockingFaultInjector;
pub(super) use crate::{
    FaultInjector, FaultOccurrence, FaultPoint, LibsqlReplicaBarrierPath,
    LibsqlReplicaRefreshCause, LibsqlReplicaRefreshPath, NoopFaultInjector, ResolvedScheduleOp,
    ResolvedWrite, ScriptedFaultInjector, TenantWriteOutcome,
};
pub(super) use libsql::{Builder, Database};
pub(super) use nimbus_core::{
    CollectionName, CronJob, CronSchedule, Document, DocumentId, DocumentLocator, DocumentPath,
    Error, FieldSchema, FieldType, IndexDefinition, Mutation, ResourcePathBinding,
    ScheduledJobOutcome, ScheduledJobResult, SchemaChangeEvent, SequenceNumber, StorageErrorKind,
    SystemWallClock, TableId, TableName, TableSchema, TableState, TenantEventKind,
    TenantEventRecord, TenantId, Timestamp, TriggerDeliveryCursor, WriteOp, WriteOpType,
};
pub(super) use serial_test::serial;

pub(super) const LIBSQL_URL_ENV: &str = "NIMBUS_LIBSQL_URL";
pub(super) const LIBSQL_AUTH_TOKEN_ENV: &str = "NIMBUS_LIBSQL_AUTH_TOKEN";
pub(super) const LIBSQL_ADMIN_URL_ENV: &str = "NIMBUS_LIBSQL_ADMIN_URL";
pub(super) const LIBSQL_ADMIN_AUTH_HEADER_ENV: &str = "NIMBUS_LIBSQL_ADMIN_AUTH_HEADER";
pub(super) static TEST_SUFFIX_COUNTER: AtomicU64 = AtomicU64::new(0);

pub(super) async fn with_test_provider<F, Fut>(test: F)
where
    F: FnOnce(LibsqlReplicaProvider, LibsqlReplicaProviderConfig) -> Fut,
    Fut: Future<Output = ()>,
{
    with_test_provider_with_faults(Arc::new(NoopFaultInjector), test).await;
}

pub(super) async fn with_test_provider_with_faults<F, Fut>(faults: Arc<dyn FaultInjector>, test: F)
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

    let provider = LibsqlReplicaProvider::connect_with_simulation_faults(
        config.clone(),
        tokio::runtime::Handle::current(),
        Arc::new(SystemWallClock),
        faults,
        Arc::new(NoopFaultInjector),
    )
    .await
    .expect("provider should connect");
    test(provider.clone(), config).await;
    provider
        .drop_provider_namespaces_for_test()
        .await
        .expect("provider namespaces should clean up");
    drop(connection);
}

pub(super) struct TestConnection {
    primary_url: String,
    auth_token: Option<String>,
    admin_api_url: String,
    admin_auth_header: Option<String>,
}

impl TestConnection {
    fn primary_url(&self) -> &str {
        &self.primary_url
    }

    fn auth_token(&self) -> Option<&str> {
        self.auth_token.as_deref()
    }

    fn admin_api_url(&self) -> &str {
        &self.admin_api_url
    }

    fn admin_auth_header(&self) -> Option<&str> {
        self.admin_auth_header.as_deref()
    }
}

pub(super) async fn test_connection() -> Option<TestConnection> {
    match external_provider_fixture_mode(
        "libsql",
        "libSQL storage provider",
        &[LIBSQL_URL_ENV, LIBSQL_ADMIN_URL_ENV],
    ) {
        ExternalProviderFixtureMode::UseExplicit => Some(TestConnection {
            primary_url: env::var(LIBSQL_URL_ENV)
                .expect("fixture policy should require the libSQL primary URL"),
            auth_token: env::var(LIBSQL_AUTH_TOKEN_ENV).ok(),
            admin_api_url: env::var(LIBSQL_ADMIN_URL_ENV)
                .expect("fixture policy should require the libSQL admin URL"),
            admin_auth_header: env::var(LIBSQL_ADMIN_AUTH_HEADER_ENV).ok(),
        }),
        ExternalProviderFixtureMode::Omit => None,
    }
}

pub(super) fn unique_suffix() -> String {
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

pub(super) async fn seed_remote_namespace(
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

pub(super) async fn open_remote_namespace_database(
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
