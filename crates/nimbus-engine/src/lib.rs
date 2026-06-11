//! Nimbus engine crate.

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
    AsyncMutationContext, CommittedMutationEvent, CommittedMutationObserver, EncryptionStatus,
    Engine, InitializedKeyProvider, MutationActor, MutationExecutionUnit,
    SubscriptionBootstrapCancellation, TableSchemaChangeEvent, TableSchemaChangeObserver,
};
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
    LibsqlReplicaRefreshPath, MaterializedJournalSnapshot, PointInTimeRestoreArchive,
    ShadowMaterializer, ShadowMaterializerConfig, ShadowMaterializerManifest,
    TableIdentitySnapshotEntry,
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
pub use tenant::{
    MaterializedReadSurfaceStats, MutationAdmissionPhase, MutationAdmissionStats,
    MutationJournalStats, PinnedServingReadSnapshot, QueryPlanningStats,
    ServingSnapshotManagerStats, SubscriptionDeliveryStats, TenantEngineDiagnosticsSnapshot,
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
