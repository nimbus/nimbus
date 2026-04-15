//! Storage layer for Neovex persistence providers.

pub mod async_storage;
pub mod commit_log;
pub mod index;
pub mod keys;
pub mod libsql;
pub mod materializer;
pub mod mysql;
pub mod postgres;
pub mod query_read;
mod runtime_bridge;
pub mod scheduler;
pub mod schema_store;
pub mod simulation;
pub mod sqlite;
pub mod store;
pub mod usage_store;

pub use async_storage::{
    EmbeddedPersistenceProvider, EmbeddedProviderKind, EmbeddedRedbControlPlaneProvider,
    EmbeddedRedbProvider, EmbeddedSqliteProvider, OpenedEmbeddedRedbTenant,
    OpenedEmbeddedSqliteTenant, RedbTenantStorage, RedbUsageStorage, SqliteTenantStorage,
    TenantReadStorage, TenantWriteOutcome, TenantWriteStorage, UsageStorage,
};
pub use libsql::{
    LibsqlReplicaBarrierPath, LibsqlReplicaFreshnessStats, LibsqlReplicaProvider,
    LibsqlReplicaProviderConfig, LibsqlReplicaRefreshCause, LibsqlReplicaRefreshPath,
    LibsqlReplicaTenantRegistration, LibsqlReplicaTenantStorage, LibsqlReplicaTenantStore,
    LibsqlReplicaWriteTransaction, OpenedLibsqlReplicaTenant,
};
pub use materializer::{ShadowMaterializer, ShadowMaterializerConfig, ShadowMaterializerManifest};
pub use mysql::{
    MySqlProvider, MySqlProviderConfig, MySqlReadSnapshot, MySqlTenantRegistration,
    MySqlTenantStorage, MySqlTenantStore, MySqlWriteTransaction, OpenedMySqlTenant,
};
pub use postgres::{
    OpenedPostgresTenant, PostgresNotificationListener, PostgresProvider, PostgresProviderConfig,
    PostgresProviderNotification, PostgresReadSnapshot, PostgresTenantRegistration,
    PostgresTenantStorage, PostgresTenantStore, PostgresWriteTransaction,
};
pub use query_read::QueryReadStore;
pub use simulation::{
    Clock, DeterministicHarness, FaultInjector, FaultOccurrence, FaultPoint, GeneratedTaskHistory,
    GeneratedTaskHistoryModel, GeneratedTaskHistorySeedCase, GeneratedTaskHistoryStep,
    GeneratedTaskPageExpectation, GeneratedTaskRecord, ManualClock, NoopFaultInjector,
    RestartBoundary, RestartPoint, ScenarioMetadata, ScenarioSignal, ScenarioSignalKind,
    ScriptedFaultInjector, ScriptedRestartSchedule, SeededFaultInjector, SystemClock,
    VERIFICATION_CASE_FILTER_ENV, VerificationHarnessMode,
    filter_generated_task_history_seed_corpus, generated_task_history_seed_corpus,
    replay_generated_task_history, replay_generated_task_history_async,
    selected_generated_task_history_seed_corpus,
};
pub use sqlite::{
    SqliteReadSnapshot, SqliteTenantStore, SqliteWriteTransaction,
    sqlite_index_scan_composite_range_query_sql, sqlite_index_scan_prefix_query_sql,
    sqlite_init_sql,
};
pub use store::{
    DEFAULT_DURABLE_JOURNAL_STREAM_LIMIT, DurableJournalBootstrap, DurableJournalPage,
    JournalProgress, MAX_DURABLE_JOURNAL_STREAM_LIMIT, MaterializedJournalSnapshot,
    ResolvedScheduleOp, ResolvedWrite, TenantReadSnapshot, TenantStore, TenantWriteCommit,
    TenantWriteTransaction,
};
pub use usage_store::{MonthlyActiveUsersSnapshot, UsageStore};

#[cfg(test)]
mod tests;
