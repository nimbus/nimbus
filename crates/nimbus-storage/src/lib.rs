//! Storage layer for Nimbus persistence providers.

pub mod async_storage;
pub mod changefeed;
pub mod commit_log;
pub mod diagnostics;
pub mod document_codec;
pub mod encrypted_redb;
pub mod format;
pub mod index;
pub mod keys;
pub mod kv;
pub mod libsql;
pub mod materializer;
pub mod mysql;
pub mod postgres;
pub mod query_read;
mod range_bound;
pub mod retention;
mod runtime_bridge;
pub mod scheduler;
pub mod schema_store;
pub mod simulation;
pub mod sqlite;
pub mod store;
mod table_identity;
pub mod traits;
pub mod usage_store;

pub use async_storage::{
    EmbeddedPersistenceProvider, EmbeddedProviderKind, EmbeddedRedbControlPlaneProvider,
    EmbeddedRedbProvider, EmbeddedSqliteProvider, RedbTenantStorage, RedbUsageStorage,
    SqliteTenantStorage, TenantReadStorage, TenantWriteOutcome, TenantWriteStorage, UsageStorage,
};
pub use changefeed::{
    ChangefeedBootstrap, ChangefeedCursor, ChangefeedEvent, ChangefeedHandle, ChangefeedPage,
};
pub use diagnostics::{
    AdapterSupportDiagnostic, BackendParityDiagnostic, BackendParityState,
    DocumentVersionStorageDiagnostic, HistoricalQueryAdmissionDiagnostic,
    HistoricalQueryAdmissionRequest, HistoricalQueryAdmissionState, IndexVersionStorageDiagnostic,
    MvccOperatorDiagnostic, MvccVersionCountsDiagnostic, StorageCapabilities, StorageFeature,
    StorageFeatureSupport, StorageFeatureSupportState, StorageHealthDiagnostic,
    StorageOperatorState, StoragePressureDiagnostic, StoragePressureState,
};
pub use encrypted_redb::{
    ENCRYPTED_FORMAT_VERSION, EncryptedFileBackend, EncryptedMemoryBackend, LOGICAL_PAGE_SIZE,
    PHYSICAL_PAGE_SIZE,
};
pub use format::{
    CURRENT_DOCUMENT_VERSION_STORAGE_FORMAT, CURRENT_INDEX_VERSION_STORAGE_FORMAT,
    CURRENT_STORAGE_FORMAT_VERSION, DOCUMENT_VERSION_STORAGE_FORMAT_METADATA_KEY,
    INDEX_VERSION_STORAGE_FORMAT_METADATA_KEY, StorageFormatVersion, storage_format_version,
    storage_format_version_from_u64, validate_document_version_storage_format,
    validate_document_version_storage_format_state, validate_index_version_storage_format,
    validate_index_version_storage_format_state, validate_storage_format_version,
};
pub use kv::{FJALL_KV_ENGINE_NAME, RedbTenantKvStore, fjall_kv_engine_type_marker};
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
pub use range_bound::IndexRangeBound;
pub use retention::{
    HardDeleteDecision, RetentionFloor, RetentionGcConfig, RetentionGcResource, RetentionGcSummary,
    RetentionGcWatermark, RetentionGcWatermarks, RetentionParticipant, RetentionPin,
    RetentionPinGuard,
};
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
    HistoricalIndexDocumentPage, JournalProgress, MAX_DURABLE_JOURNAL_STREAM_LIMIT,
    MaterializedJournalSnapshot, PointInTimeRestoreArchive, PointInTimeRestoreTarget,
    ResolvedScheduleOp, ResolvedWrite, TenantReadSnapshot, TenantStore, TenantWriteCommit,
    TenantWriteTransaction,
};
pub use table_identity::{
    TableBackendLayout, TableIdentityDiagnostic, TableIdentitySnapshotEntry,
    TableLifecycleStateMachine, TableLifecycleTransition, TableSummaryStatus,
    apply_table_lifecycle_transition,
};
pub use traits::{
    ControlPlaneUsage, DurableJournal, KeyProviderSurface, KvBatchOp, KvBatchOutcome, KvEntry,
    KvMutation, KvPut, KvScanPage, KvStorageEngine, KvSweepOutcome, OBJECT_MANIFEST_TABLE,
    OBJECT_MULTIPART_TABLE, ObjectBlobLayout, ObjectChecksums, ObjectChunkRef, ObjectManifest,
    ObjectManifestAttributes, ObjectMetaStore, ObjectMultipartPart, ObjectMultipartUpload,
    SchedulerStore, StorageEngine, TenantKvStore, TenantLifecycle, TenantPointRead,
    TenantPointWrite, TenantRangeScan,
};
pub use usage_store::{MonthlyActiveUsersSnapshot, UsageStore};

#[cfg(test)]
mod tests;
