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
#[cfg(feature = "libsql")]
pub mod libsql;
pub mod materializer;
pub mod memory;
#[cfg(feature = "mysql")]
pub mod mysql;
pub mod object_placement;
#[cfg(feature = "postgres")]
pub mod postgres;
#[cfg(any(test, feature = "test-hooks"))]
#[doc(hidden)]
pub mod provider_test_fixtures;
pub mod query_read;
mod range_bound;
pub mod retention;
// Bridging a blocking call out of an async runtime is only needed by the remote
// providers; the embedded backends run on the blocking executor already.
#[cfg(any(test, feature = "libsql", feature = "mysql", feature = "postgres"))]
mod runtime_bridge;
pub mod scheduler;
pub mod schema_store;
pub mod simulation;
#[cfg(any(feature = "libsql", feature = "mysql", feature = "postgres"))]
pub(crate) mod sql;
pub mod sqlite;
pub mod store;
mod table_identity;
pub mod tenant_incarnation;
pub mod traits;
mod trigger_invocation_transition;
pub mod usage_store;

pub use async_storage::{
    EmbeddedProviderKind, EmbeddedRedbControlPlaneProvider, EmbeddedRedbProvider,
    EmbeddedSqliteProvider, RedbTenantStorage, RedbUsageStorage, SqliteTenantStorage,
    TenantReadStorage, TenantWriteOutcome, TenantWriteStorage, UsageStorage,
};
pub use changefeed::{
    ChangefeedBootstrap, ChangefeedCursor, ChangefeedEvent, ChangefeedHandle, ChangefeedPage,
};
pub use diagnostics::{
    AdapterSupportDiagnostic, BackendParityDiagnostic, BackendParityState,
    DocumentVersionStorageDiagnostic, HistoricalQueryAdmissionDiagnostic,
    HistoricalQueryAdmissionRequest, HistoricalQueryAdmissionState, IndexVersionStorageDiagnostic,
    MvccOperatorDiagnostic, MvccVersionCountsDiagnostic, ProviderWritePipelineDiagnostic,
    StorageCapabilities, StorageFeature, StorageFeatureSupport, StorageFeatureSupportState,
    StorageHealthDiagnostic, StorageOperatorState, StoragePressureDiagnostic, StoragePressureState,
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
#[cfg(feature = "libsql")]
pub use libsql::{
    LibsqlReplicaBarrierPath, LibsqlReplicaFreshnessStats, LibsqlReplicaProvider,
    LibsqlReplicaProviderConfig, LibsqlReplicaRefreshCause, LibsqlReplicaRefreshPath,
    LibsqlReplicaTenantRegistration, LibsqlReplicaTenantStorage, LibsqlReplicaTenantStore,
    LibsqlReplicaWriteTransaction,
};
pub use materializer::{ShadowMaterializer, ShadowMaterializerConfig, ShadowMaterializerManifest};
pub use memory::{
    MemoryTenantProvider, MemoryTenantSnapshot, MemoryTenantStorage, MemoryTenantStore,
    MemoryWriteTransaction, OpenedMemoryTenant,
};
#[cfg(feature = "mysql")]
pub use mysql::{
    MySqlProvider, MySqlProviderConfig, MySqlReadSnapshot, MySqlTenantRegistration,
    MySqlTenantStorage, MySqlTenantStore, MySqlWriteTransaction,
};
pub use object_placement::{
    ObjectPlacement, ObjectPlacementStore, ObjectStorePlacementTarget,
    ObjectStoreProviderCredentials, ObjectStoreProviderKind, PlacementPolicy,
};
#[cfg(feature = "postgres")]
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
pub use scheduler::{
    PreparedScheduleBatch, PreparedSchedulerWrite, ScheduleBatchReconciliation, SchedulerWrite,
    SchedulerWriteOutcomeStore, SchedulerWriteReconciliation, SchedulerWriteResult,
    SchedulerWriteStore,
};
pub use simulation::{
    DeterministicHarness, DurableApplyKind, FaultInjector, FaultOccurrence, FaultPoint,
    GeneratedTaskHistory, GeneratedTaskHistoryModel, GeneratedTaskHistorySeedCase,
    GeneratedTaskHistoryStep, GeneratedTaskPageExpectation, GeneratedTaskRecord, NoopFaultInjector,
    RestartBoundary, RestartPoint, ScenarioMetadata, ScenarioSignal, ScenarioSignalKind,
    ScriptedFaultInjector, ScriptedRestartSchedule, SeededFaultInjector,
    VERIFICATION_CASE_FILTER_ENV, VerificationHarnessMode,
    filter_generated_task_history_seed_corpus, generated_task_history_seed_corpus,
    replay_generated_task_history, replay_generated_task_history_async,
    selected_generated_task_history_seed_corpus,
};
#[cfg(any(test, feature = "test-hooks"))]
#[doc(hidden)]
pub use sqlite::{
    SqlitePassiveCheckpointProbe, SqliteWalCheckpointObservationSnapshot,
    disable_sqlite_wal_checkpoint_observation, probe_sqlite_passive_checkpoint,
    reset_sqlite_wal_checkpoint_observation, sqlite_wal_checkpoint_observation_snapshot,
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
    CanonicalMaterializedState, DEFAULT_DURABLE_JOURNAL_STREAM_LIMIT, DirectWriteAssignment,
    DurableJournalBootstrap, DurableJournalPage, HistoricalIndexDocumentPage, JournalProgress,
    MATERIALIZED_JOURNAL_SNAPSHOT_VERSION, MATERIALIZED_POSITION_VERSION,
    MAX_DURABLE_JOURNAL_STREAM_LIMIT, MaterializedJournalSnapshot, MaterializedPosition,
    PointInTimeRestoreArchive, PointInTimeRestoreTarget, ResolvedScheduleOp, ResolvedWrite,
    TenantReadSnapshot, TenantStore, TenantWriteCommit, TenantWriteTransaction,
};
pub use table_identity::{
    TableBackendLayout, TableIdentityDiagnostic, TableIdentitySnapshotEntry,
    TableLifecycleStateMachine, TableLifecycleTransition, TableSummaryStatus,
    apply_table_lifecycle_transition,
};
pub use tenant_incarnation::TenantIncarnationStore;
pub use traits::{
    CommitterLease, CommitterLeaseError, CommitterLeaseResult, CommitterLeaseStore,
    ControlPlaneUsage, DurableJournal, KeyProviderSurface, KvBatchOp, KvBatchOutcome, KvEntry,
    KvMutation, KvPut, KvScanPage, KvStorageEngine, KvSweepOutcome, MaterializedRebuild,
    OBJECT_MANIFEST_TABLE, OBJECT_MULTIPART_TABLE, ObjectBlobLayout, ObjectChecksums,
    ObjectChunkRef, ObjectConditionOutcome, ObjectExpectedState, ObjectManifest,
    ObjectManifestAttributes, ObjectMetaRead, ObjectMultipartPart, ObjectMultipartUpload,
    ObjectUploadConditionOutcome, ObjectUploadExpectedState, ReadCapabilities, ResourcePathScan,
    ResourcePathSnapshot, SchedulerStore, StorageEngine, TenantKvStore, TenantLifecycle,
    TenantPointRead, TenantPointWrite, TenantRangeScan, multipart_upload_document_id,
    object_manifest_document_id,
};
pub use trigger_invocation_transition::TriggerInvocationTransitionStore;
pub use usage_store::{MonthlyActiveUsersSnapshot, UsageStore};

#[cfg(test)]
mod tests;
