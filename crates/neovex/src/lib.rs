//! Public facade for embedding Neovex.
//!
//! This crate re-exports the stable, high-level surface so callers do not
//! need to depend on multiple internal workspace crates directly.

// Core data model and query surface.
pub use neovex_core::{
    CommitEntry, CreateCronRequest, CronJob, CronSchedule, Cursor, Document, DocumentId, Error,
    FieldSchema, FieldType, Filter, FilterOp, IndexDefinition, JobId, Mutation, OrderBy,
    OrderDirection, Page, PaginatedQuery, Query, Result, ScheduleRequest, ScheduledJob,
    ScheduledJobOutcome, ScheduledJobResult, Schema, SequenceNumber, TableName, TableSchema,
    TenantId, Timestamp, WriteOp, WriteOpType,
};
// Engine coordination and persistence configuration.
pub use neovex_engine::{
    AwsKmsConfig, ControlPlaneConfig, EncryptionConfigDescriptor, EncryptionStatus,
    EncryptionValidationError, InitializedKeyProvider, KeyDirectoryConfig, KeyProviderDescriptor,
    LocalEncryptionConfig, LocalKeyProviderConfig, LocalPersistenceFamily, MasterKeyFileConfig,
    MonthlyActiveUsersSnapshot, PersistenceDialect, PersistenceTopology, PoolConfig,
    ProviderCredentials, Service, ServicePersistenceConfig, SubscriptionUpdate,
    TenantProviderConfig, TenantRoutingConfig, evaluate_paginated, evaluate_query, run_scheduler,
};
// Runtime execution contract and limits.
pub use neovex_runtime::{
    HostBridge, HostBridgeFuture, HostCallRequest, InvocationKind, InvocationRequest,
    NeovexRuntime, NeovexRuntimeError, RuntimeBackendKind, RuntimeBundle, RuntimeExecutionModel,
    RuntimeExecutor, RuntimeInvocationContext, RuntimeLimits, RuntimePolicy, VerifiedUserIdentity,
    VerifiedUserIdentityKind,
};
// Sandbox orchestration surface.
pub use neovex_sandbox::{
    PublishedEndpoint, PublishedEndpointProtocol, SandboxBackend, SandboxBackendKind,
    SandboxBuildLaunchSpec, SandboxError, SandboxFilesystemSpec, SandboxHandle, SandboxId,
    SandboxImageLaunchSpec, SandboxImageProcessOverrides, SandboxLifecycleSpec, SandboxPortBinding,
    SandboxProcessSpec, SandboxResourceLimits, SandboxRestartPolicy, SandboxSpec, SandboxStatus,
};
// Server integration and transport construction helpers.
pub use neovex_server::{
    ConvexRegistry, EmptySandboxCatalog, EmptySandboxServiceCatalog, LICENSE_FILE_ENV,
    LicenseDocument, LicenseEntitlements, LicenseKind, LicenseLoadError, LicenseSnapshot,
    LicenseSourceInfo, LicenseSourceKind, LicenseState, LicenseStatus, LicenseUsageSnapshot,
    SandboxCatalog, SandboxServiceCatalog, SandboxServiceLaunch, SandboxServiceManager,
    ServeOptions, serve, serve_with_convex, serve_with_convex_and_license,
    serve_with_convex_and_license_and_sandbox_service_manager, serve_with_license,
    serve_with_license_and_sandbox_catalog, serve_with_options,
};
#[cfg(feature = "aws-kms")]
pub use neovex_storage::AwsKmsKeyProvider;
// Storage and encryption helpers.
pub use neovex_storage::EmbeddedProviderKind;
pub use neovex_storage::TenantStore;
pub use neovex_storage::{
    KeyDirectoryProvider, KeyManifest, KeyManifestHeader, LOGICAL_PAGE_SIZE, LocalArtifactRole,
    LocalDatabaseRole, LocalKeyProvider, LocalKeySubject, LocalKeySubjectKind, ManifestCipher,
    MasterKeyFileProvider, PHYSICAL_PAGE_SIZE, generate_database_manifest,
    resolve_database_encryption_key, unwrap_database_manifest_key,
};
pub use neovex_storage::{
    checkpoint_encrypted_database_at_path, export_encrypted_to_plaintext,
    export_plaintext_to_encrypted, migrate_encrypted_to_plaintext, migrate_plaintext_to_encrypted,
    rekey_encrypted_database, rekey_encrypted_database_at_path,
};
