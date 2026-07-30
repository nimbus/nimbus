pub(super) use std::env;
pub(super) use std::future::Future;
pub(super) use std::sync::atomic::{AtomicU64, Ordering};
pub(super) use std::time::{SystemTime, UNIX_EPOCH};

pub(super) use super::super::provider_support::*;
pub(super) use super::super::{
    ExternalProviderFixtureMode, FieldSchema, FieldType, MySqlProvider, MySqlProviderConfig,
    TenantEventRecord, TenantReadStorage, external_provider_fixture_mode,
};
pub(super) use crate::{FaultInjector, FaultPoint, ResolvedScheduleOp, ResolvedWrite};
pub(super) use mysql_async::prelude::Queryable;
pub(super) use mysql_async::{Opts, Pool};
pub(super) use nimbus_core::{
    CronJob, CronSchedule, Document, Mutation, ScheduledJobOutcome, ScheduledJobResult, Schema,
    SchemaChangeEvent, SequenceNumber, SystemWallClock, TableId, TableName, TableSchema,
    TableState, TenantEventKind, TenantId, Timestamp, TriggerDeliveryCursor, WriteOp, WriteOpType,
};

pub(super) const MYSQL_URL_ENV: &str = "NIMBUS_MYSQL_URL";
pub(super) static TEST_SUFFIX_COUNTER: AtomicU64 = AtomicU64::new(0);

pub(super) async fn with_test_provider<F, Fut>(test: F)
where
    F: FnOnce(MySqlProvider, MySqlProviderConfig) -> Fut,
    Fut: Future<Output = ()>,
{
    with_test_provider_and_fault_injector(std::sync::Arc::new(crate::NoopFaultInjector), test)
        .await;
}

pub(super) async fn with_test_provider_and_fault_injector<F, Fut>(
    fault_injector: std::sync::Arc<dyn FaultInjector>,
    test: F,
) where
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
        connection_string: connection,
        metadata_database,
        tenant_database_prefix,
        min_connections: Some(1),
        max_connections: Some(4),
    };
    let provider = MySqlProvider::connect_with_simulation(
        config.clone(),
        tokio::runtime::Handle::current(),
        std::sync::Arc::new(SystemWallClock),
        fault_injector,
    )
    .await
    .expect("provider should connect");
    test(provider.clone(), config).await;
    provider
        .drop_provider_databases_for_test()
        .await
        .expect("test provider databases should drop");
}

pub(super) async fn test_connection() -> Option<String> {
    match external_provider_fixture_mode("mysql", "MySQL storage provider", &[MYSQL_URL_ENV]) {
        ExternalProviderFixtureMode::UseExplicit => {
            Some(env::var(MYSQL_URL_ENV).expect("fixture policy should require the MySQL URL"))
        }
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

pub(super) async fn document_index_counts(
    connection_string: &str,
    database_name: &str,
) -> (u64, u64) {
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

pub(super) async fn document_generated_column_expressions(
    connection_string: &str,
    database_name: &str,
) -> Vec<String> {
    let opts = Opts::from_url(connection_string).expect("connection string should parse");
    let pool = Pool::new(opts);
    let mut conn = pool.get_conn().await.expect("mysql connection should open");
    let expressions = conn
        .exec::<String, _, _>(
            "SELECT GENERATION_EXPRESSION \
             FROM INFORMATION_SCHEMA.COLUMNS \
             WHERE TABLE_SCHEMA = ? \
               AND TABLE_NAME = 'documents' \
               AND COLUMN_NAME LIKE 'gcol\\_%' \
             ORDER BY COLUMN_NAME",
            (database_name,),
        )
        .await
        .expect("generated column expressions should query");
    conn.disconnect()
        .await
        .expect("mysql connection should close");
    pool.disconnect().await.expect("mysql pool should close");
    expressions
}
