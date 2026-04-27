//! Storage layer for Neovex persistence providers.

pub mod async_storage;
pub mod commit_log;
pub mod document_codec;
pub mod encrypted_redb;
pub mod encryption;
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
    EmbeddedRedbProvider, EmbeddedSqliteProvider, RedbTenantStorage, RedbUsageStorage,
    SqliteTenantStorage, TenantReadStorage, TenantWriteOutcome, TenantWriteStorage, UsageStorage,
};
pub use encrypted_redb::{
    ENCRYPTED_FORMAT_VERSION, EncryptedFileBackend, EncryptedMemoryBackend, LOGICAL_PAGE_SIZE,
    PHYSICAL_PAGE_SIZE,
};
#[cfg(feature = "aws-kms")]
pub use encryption::AwsKmsKeyProvider;
pub use encryption::{
    GeneratedDatabaseKey, KeyDirectoryProvider, KeyManifest, KeyManifestHeader, LocalArtifactRole,
    LocalDatabaseRole, LocalKeyProvider, LocalKeyProviderError, LocalKeySubject,
    LocalKeySubjectKind, ManifestCipher, ManifestError, ManifestReadError, ManifestWriteError,
    MasterKeyFileProvider, WrappedDatabaseKey, generate_database_manifest,
    resolve_database_encryption_key, unwrap_database_manifest_key,
};
pub use libsql::{
    LibsqlReplicaBarrierPath, LibsqlReplicaFreshnessStats, LibsqlReplicaProvider,
    LibsqlReplicaProviderConfig, LibsqlReplicaRefreshCause, LibsqlReplicaRefreshPath,
    LibsqlReplicaTenantRegistration, LibsqlReplicaTenantStorage, LibsqlReplicaTenantStore,
    LibsqlReplicaWriteTransaction,
};
pub use materializer::{ShadowMaterializer, ShadowMaterializerConfig, ShadowMaterializerManifest};
pub use mysql::{
    MySqlProvider, MySqlProviderConfig, MySqlReadSnapshot, MySqlTenantRegistration,
    MySqlTenantStorage, MySqlTenantStore, MySqlWriteTransaction,
};
pub use postgres::{
    PostgresNotificationListener, PostgresProvider, PostgresProviderConfig,
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
    encryption::{
        checkpoint_encrypted_database_at_path, export_encrypted_to_plaintext,
        export_plaintext_to_encrypted, migrate_encrypted_to_plaintext,
        migrate_plaintext_to_encrypted, rekey_encrypted_database, rekey_encrypted_database_at_path,
    },
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
