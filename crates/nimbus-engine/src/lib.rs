//! Nimbus engine crate.

mod config;
mod engine;
mod evaluator;
mod persistence;
mod persistence_config;
mod replica;
pub mod scheduler;
mod subscriptions;
mod tenant;
mod triggers;
mod verification;

pub use engine::{
    AsyncMutationContext, CommitPhaseMetricsSnapshot, CommittedMutationEvent,
    CommittedMutationObserver, CommittedMutationObserverWorkStats, EncryptionStatus, Engine,
    InitializedKeyProvider, MutationActor, MutationExecutionUnit, MutationIsolatePermit,
    ProjectionReconciliationSnapshot, ProjectionToken, SubscribeOptions,
    SubscriptionBootstrapCancellation, TableSchemaChangeEvent, TableSchemaChangeObserver,
    TenantObjectMeta, TenantRuntimeLoadedEvent, TenantRuntimeObserver,
    TenantRuntimeObserverIdentity,
};
#[cfg(any(test, feature = "test-hooks"))]
pub use engine::{CommitFaultHandle, Fault, commit_fault_labels};
pub use evaluator::{
    encode_cursor, evaluate_paginated, evaluate_paginated_with_docs, evaluate_query,
    evaluate_query_with_docs,
};
pub use nimbus_storage::EmbeddedProviderKind;
pub use nimbus_storage::MonthlyActiveUsersSnapshot;
pub use nimbus_storage::{
    ChangefeedBootstrap, ChangefeedCursor, ChangefeedEvent, ChangefeedHandle, ChangefeedPage,
    DEFAULT_DURABLE_JOURNAL_STREAM_LIMIT, DurableJournalBootstrap, DurableJournalPage,
    LibsqlReplicaBarrierPath, LibsqlReplicaFreshnessStats, LibsqlReplicaRefreshCause,
    LibsqlReplicaRefreshPath, MaterializedJournalSnapshot, ObjectBlobLayout, ObjectChecksums,
    ObjectChunkRef, ObjectManifest, ObjectManifestAttributes, ObjectMultipartPart,
    ObjectMultipartUpload, ObjectPlacement, ObjectPlacementStore, ObjectStorePlacementTarget,
    ObjectStoreProviderCredentials, ObjectStoreProviderKind, PlacementPolicy,
    PointInTimeRestoreArchive, ShadowMaterializer, ShadowMaterializerConfig,
    ShadowMaterializerManifest, TableIdentitySnapshotEntry,
};
pub use persistence_config::{
    AwsKmsConfig, ControlPlaneConfig, EncryptionConfigDescriptor, EncryptionValidationError,
    EnginePersistenceConfig, KeyDirectoryConfig, KeyProviderDescriptor, LocalEncryptionConfig,
    LocalKeyProviderConfig, LocalPersistenceFamily, MasterKeyFileConfig, PersistenceDialect,
    PersistenceTopology, PoolConfig, ProviderCredentials, TenantProviderConfig,
    TenantRoutingConfig,
};
pub use replica::EmbeddedReplica;
pub use scheduler::run_scheduler;
pub use subscriptions::{
    DEFAULT_SUBSCRIPTION_CHANNEL_CAPACITY, SubscriptionCleanupHandle, SubscriptionRegistration,
    SubscriptionUpdate,
};
#[cfg(any(test, feature = "test-hooks"))]
pub use tenant::MutationJournalPauseHandle;
pub use tenant::{
    CommitterArm, MaterializedReadSurfaceStats, MutationAdmissionPhase, MutationAdmissionStats,
    MutationIsolateAdmissionStats, MutationJournalStats, PinnedServingReadSnapshot,
    QueryPlanningStats, ServingSnapshotManagerStats, SubscriptionDeliveryStats,
    TenantEngineDiagnosticsSnapshot, TenantOperationGuard,
};
pub use triggers::{
    TriggerInvocationExecution, TriggerInvocationExecutor, TriggerLookupMatch, TriggerRegistration,
    TriggerRegistry,
};
pub use verification::{
    BootstrapFingerprint, ConsistencyMismatch, ConsistencyScope, ConsistencyVerificationReport,
    SnapshotFingerprint,
};

#[cfg(test)]
mod test_support;

#[cfg(test)]
mod tests;
