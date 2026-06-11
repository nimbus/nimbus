//! Public facade for embedding Nimbus.
//!
//! This crate re-exports the stable, high-level surface so callers do not
//! need to depend on multiple internal workspace crates directly.

// Core data model and query surface.
pub use nimbus_core::{
    CommitEntry, CreateCronRequest, CronJob, CronSchedule, Cursor, Document, DocumentId, Error,
    FieldSchema, FieldType, Filter, FilterOp, IndexDefinition, JobId, Mutation, OrderBy,
    OrderDirection, Page, PaginatedQuery, Query, Result, ScheduleRequest, ScheduledJob,
    ScheduledJobOutcome, ScheduledJobResult, Schema, SequenceNumber, TableName, TableSchema,
    TenantId, Timestamp, WriteOp, WriteOpType,
};
// Engine coordination and persistence configuration.
pub use nimbus_engine::{
    AwsKmsConfig, ControlPlaneConfig, EncryptionConfigDescriptor, EncryptionStatus,
    EncryptionValidationError, Engine, EnginePersistenceConfig, InitializedKeyProvider,
    KeyDirectoryConfig, KeyProviderDescriptor, LocalEncryptionConfig, LocalKeyProviderConfig,
    LocalPersistenceFamily, MasterKeyFileConfig, MonthlyActiveUsersSnapshot, PersistenceDialect,
    PersistenceTopology, PoolConfig, ProviderCredentials, SubscriptionUpdate, TenantProviderConfig,
    TenantRoutingConfig, evaluate_paginated, evaluate_query, run_scheduler,
};
// Runtime execution contract and limits.
pub use nimbus_runtime::{
    HostBridge, HostBridgeFuture, HostCallRequest, InvocationKind, InvocationRequest,
    NimbusRuntime, NimbusRuntimeError, RuntimeBackendKind, RuntimeBackendLifecyclePolicy,
    RuntimeBackendLockdownProfile, RuntimeBackendTrustTier, RuntimeBundle, RuntimeExecutionModel,
    RuntimeExecutor, RuntimeInvocationContext, RuntimeLimits, RuntimePolicy, RuntimePoolKind,
    VerifiedUserIdentity, VerifiedUserIdentityKind,
};
// Sandbox orchestration surface.
pub use nimbus_sandbox::{
    CompiledSandboxEgressPolicy, PublishedEndpoint, PublishedEndpointProtocol,
    SANDBOX_EGRESS_ENFORCEMENT_ENV, SANDBOX_EGRESS_ENFORCEMENT_SCHEMA_VERSION,
    SANDBOX_EGRESS_LEGACY_POLICY_ENV, SANDBOX_EGRESS_RESERVED_ENV_KEYS, SandboxBackend,
    SandboxBackendKind, SandboxEgressAuthorization, SandboxEgressEnforcementMode,
    SandboxEgressEnforcementPlan, SandboxEgressPolicy, SandboxEgressReloadPolicy,
    SandboxEgressRequest, SandboxEgressRule, SandboxError, SandboxHandle, SandboxId,
    SandboxLifecycleSpec, SandboxMountSource, SandboxMountSpec, SandboxOciBuildSpec,
    SandboxOciImageReferenceSpec, SandboxOciImageSource, SandboxOciImageSpec, SandboxOwnerSpec,
    SandboxPortBinding, SandboxProcessSpec, SandboxResourceCharge, SandboxResourceLimits,
    SandboxResourceQuotaPolicy, SandboxRestartPolicy, SandboxRootSpec, SandboxRootfsSpec,
    SandboxSpec, SandboxStatus, validate_sandbox_mounts, validate_tenant_volume_name,
};
// Server integration and transport construction helpers.
pub use nimbus_server::{
    BuiltInServiceSpec, ConvexRegistry, EmptyServiceDefinitionCatalog, EmptyServiceInstanceCatalog,
    ExternalServiceSpec, LICENSE_FILE_ENV, LicenseDocument, LicenseEntitlements, LicenseKind,
    LicenseLoadError, LicenseSnapshot, LicenseSourceInfo, LicenseSourceKind, LicenseState,
    LicenseStatus, LicenseUsageSnapshot, RouterOptions, ServeOptions, ServiceBackend,
    ServiceDefinitionCatalog, ServiceInstanceCatalog, ServiceManager, build_router, serve,
};
#[cfg(feature = "aws-kms")]
pub use nimbus_storage::AwsKmsKeyProvider;
// Storage and encryption helpers.
pub use nimbus_storage::EmbeddedProviderKind;
pub use nimbus_storage::PointInTimeRestoreArchive;
pub use nimbus_storage::TenantStore;
pub use nimbus_storage::{
    KeyDirectoryProvider, KeyManifest, KeyManifestHeader, LOGICAL_PAGE_SIZE, LocalArtifactRole,
    LocalDatabaseRole, LocalKeyProvider, LocalKeySubject, LocalKeySubjectKind, ManifestCipher,
    MasterKeyFileProvider, PHYSICAL_PAGE_SIZE, generate_database_manifest,
    resolve_database_encryption_key, unwrap_database_manifest_key,
};
pub use nimbus_storage::{
    checkpoint_encrypted_database_at_path, export_encrypted_to_plaintext,
    export_plaintext_to_encrypted, migrate_encrypted_to_plaintext, migrate_plaintext_to_encrypted,
    rekey_encrypted_database, rekey_encrypted_database_at_path,
};
pub use nimbus_storage::{
    commit_staged_redb_dek_rotation, recover_interrupted_redb_dek_rotation,
    redb_dek_rotation_database_stage_path, redb_dek_rotation_manifest_stage_path,
};
