pub(super) use std::env;
pub(super) use std::future::Future;
pub(super) use std::sync::atomic::{AtomicU64, Ordering};
pub(super) use std::time::{SystemTime, UNIX_EPOCH};

pub(super) use super::super::provider_support::*;
pub(super) use super::super::{
    Document, Duration, ExternalProviderFixtureMode, FieldSchema, FieldType, IndexDefinition,
    PostgresProvider, PostgresProviderConfig, TableSchema, TenantEventRecord, WriteOp, WriteOpType,
    external_provider_fixture_mode, timeout,
};
pub(super) use crate::{FaultInjector, FaultPoint, ResolvedScheduleOp, ResolvedWrite};
pub(super) use nimbus_core::{
    CronJob, CronSchedule, Mutation, ScheduledJobOutcome, ScheduledJobResult, Schema,
    SchemaChangeEvent, SequenceNumber, SystemWallClock, TableId, TableName, TableState,
    TenantEventKind, TenantId, Timestamp, TriggerDeliveryCursor,
};

pub(super) const TEST_POSTGRES_URL_ENV: &str = "NIMBUS_TEST_POSTGRES_URL";
pub(super) static TEST_SUFFIX_COUNTER: AtomicU64 = AtomicU64::new(0);

pub(super) async fn with_test_provider<F, Fut>(test: F)
where
    F: FnOnce(PostgresProvider, PostgresProviderConfig) -> Fut,
    Fut: Future<Output = ()>,
{
    with_test_provider_and_fault_injector(std::sync::Arc::new(crate::NoopFaultInjector), test)
        .await;
}

pub(super) async fn with_test_provider_and_fault_injector<F, Fut>(
    fault_injector: std::sync::Arc<dyn FaultInjector>,
    test: F,
) where
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
        connection_string: connection,
        metadata_schema,
        tenant_schema_prefix,
        min_connections: Some(1),
        max_connections: Some(4),
    };
    let provider = PostgresProvider::connect_with_simulation(
        config.clone(),
        tokio::runtime::Handle::current(),
        std::sync::Arc::new(SystemWallClock),
        fault_injector,
    )
    .await
    .expect("provider should connect");
    test(provider.clone(), config).await;
    provider
        .drop_metadata_schema_for_test()
        .await
        .expect("test metadata schema should drop");
}

pub(super) async fn test_connection() -> Option<String> {
    match external_provider_fixture_mode(
        "postgres",
        "PostgreSQL storage provider",
        &[TEST_POSTGRES_URL_ENV],
    ) {
        ExternalProviderFixtureMode::UseExplicit => Some(
            env::var(TEST_POSTGRES_URL_ENV)
                .expect("fixture policy should require the PostgreSQL URL"),
        ),
        ExternalProviderFixtureMode::Omit => None,
    }
}

pub(super) fn unique_suffix() -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after unix epoch")
        .as_nanos();
    let counter = TEST_SUFFIX_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{counter:08x}{:x}{timestamp:x}", std::process::id())
}
