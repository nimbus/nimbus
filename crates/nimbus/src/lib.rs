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
    TenantId, Timestamp, VerifiedUserIdentity, VerifiedUserIdentityKind, WriteOp, WriteOpType,
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
    EffectiveRuntimeScalingPlan, HostBridge, HostBridgeFuture, HostCallRequest, InvocationKind,
    InvocationRequest, NimbusRuntime, NimbusRuntimeError, RequestedRuntimeScalingTarget,
    RuntimeAdaptiveCanaryPolicy, RuntimeAdaptiveControllerMode, RuntimeAdaptiveControllerSettings,
    RuntimeBackendKind, RuntimeBackendLifecyclePolicy, RuntimeBackendLockdownProfile,
    RuntimeBackendTrustTier, RuntimeBundle, RuntimeBundleContent,
    RuntimeBundleWasmComponentContent, RuntimeComponentWorld, RuntimeControllerReplayConfig,
    RuntimeDensityBudget, RuntimeDensityMeasurement, RuntimeDensityMeasurementMethod,
    RuntimeDensityPlan, RuntimeExecutionModel, RuntimeExecutor, RuntimeHostAdmissionAction,
    RuntimeHostAdmissionDecision, RuntimeHostPressureLevel, RuntimeHostPressureSample,
    RuntimeHostPressureSourceStatus, RuntimeHostResourceBudget, RuntimeHostResourceDecision,
    RuntimeHostWorkClass, RuntimeInvocationContext, RuntimeIsolateGroupFfiStatus, RuntimeLimits,
    RuntimeMemoryPressureDecision, RuntimeMemoryPressureLevel, RuntimeMemoryPressureSample,
    RuntimeMemoryPressureSourceStatus, RuntimePolicy, RuntimePoolKind,
    RuntimePrewarmScheduleDecision, RuntimeProfile, RuntimeScalingAdjustmentReason,
    RuntimeScalingLimit, RuntimeScalingPlanSet, RuntimeScalingPreset, RuntimeScalingTarget,
};
// Tenant egress policy decision point.
pub use nimbus_egress::{
    CompiledEgressPolicy, EGRESS_ENFORCEMENT_ENV, EGRESS_ENFORCEMENT_SCHEMA_VERSION,
    EGRESS_LEGACY_POLICY_ENV, EGRESS_PROXY_URL_ENV, EGRESS_RESERVED_ENV_KEYS, EgressAuthorization,
    EgressEnforcementMode, EgressEnforcementPlan, EgressPolicy, EgressProtocol, EgressReloadPolicy,
    EgressRequest, EgressRule,
};
// Sandbox orchestration surface.
pub use nimbus_sandbox::{
    PublishedEndpoint, PublishedEndpointProtocol, SandboxBackend, SandboxBackendKind, SandboxError,
    SandboxHandle, SandboxId, SandboxLifecycleSpec, SandboxMountSource, SandboxMountSpec,
    SandboxOciBuildSpec, SandboxOciImageReferenceSpec, SandboxOciImageSource, SandboxOciImageSpec,
    SandboxOwnerSpec, SandboxPortBinding, SandboxProcessSpec, SandboxResourceCharge,
    SandboxResourceLimits, SandboxResourceQuotaPolicy, SandboxRestartPolicy, SandboxRootSpec,
    SandboxRootfsSpec, SandboxSpec, SandboxStatus, validate_sandbox_mounts,
    validate_tenant_volume_name,
};
// Server integration and transport construction helpers.
#[cfg(feature = "aws-kms")]
pub use nimbus_crypto::AwsKmsKeyProvider;
pub use nimbus_crypto::{
    GeneratedDataKey, KeyDirectoryProvider, KeyManifest, KeyManifestHeader, LocalArtifactRole,
    LocalDatabaseRole, LocalKeyProvider, LocalKeyProviderError, LocalKeySubject,
    LocalKeySubjectKind, ManifestCipher, ManifestError, ManifestReadError, ManifestWriteError,
    MasterKeyFileProvider, WrappedDataKey, commit_staged_dek_rotation,
    dek_rotation_data_stage_path, dek_rotation_manifest_stage_path, generate_key_manifest,
    recover_interrupted_dek_rotation, resolve_subject_encryption_key, unwrap_key_manifest,
};
pub use nimbus_license::{
    LICENSE_FILE_ENV, LicenseDocument, LicenseEntitlements, LicenseKind, LicenseLoadError,
    LicenseSnapshot, LicenseSourceInfo, LicenseSourceKind, LicenseState, LicenseStatus,
    LicenseUsageSnapshot,
};
pub use nimbus_server::{RouterOptions, ServeOptions, build_router, serve};
pub use nimbus_services::{
    BuiltInServiceSpec, EmptyServiceDefinitionCatalog, EmptyServiceInstanceCatalog,
    ExternalServiceSpec, LocalBuildAdmission, ServiceBackend, ServiceDefinitionCatalog,
    ServiceInstanceCatalog, ServiceManager,
};
// Storage and encryption helpers.
pub use nimbus_blob::{
    BackupBundle, BackupRequest, BlobHash, KeyEscrow, LocalPackStore, ObjectBackup,
};
pub use nimbus_object_storage::{
    ObjectStorageConfig, ObjectStorageEnv, ObjectStorageResolver, ObjectStoreCredentialResolver,
    ObjectStoreSecret, object_backup_roots, object_blob_key_path, object_blob_root,
    object_master_key_path,
};
pub use nimbus_storage::EmbeddedProviderKind;
pub use nimbus_storage::PointInTimeRestoreArchive;
pub use nimbus_storage::TenantStore;
pub use nimbus_storage::{LOGICAL_PAGE_SIZE, PHYSICAL_PAGE_SIZE};
pub use nimbus_storage::{
    ObjectPlacement, ObjectPlacementStore, ObjectStorePlacementTarget,
    ObjectStoreProviderCredentials, ObjectStoreProviderKind, PlacementPolicy,
};
pub use nimbus_storage::{
    checkpoint_encrypted_database_at_path, export_encrypted_to_plaintext,
    export_plaintext_to_encrypted, migrate_encrypted_to_plaintext, migrate_plaintext_to_encrypted,
    rekey_encrypted_database, rekey_encrypted_database_at_path,
};
