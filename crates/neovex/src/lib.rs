//! Public facade for embedding Neovex.
//!
//! This crate re-exports the stable, high-level surface so callers do not
//! need to depend on multiple internal workspace crates directly.

pub use neovex_core::{
    CommitEntry, CreateCronRequest, CronJob, CronSchedule, Cursor, Document, DocumentId, Error,
    FieldSchema, FieldType, Filter, FilterOp, IndexDefinition, JobId, Mutation, OrderBy,
    OrderDirection, Page, PaginatedQuery, Query, Result, ScheduleRequest, ScheduledJob,
    ScheduledJobOutcome, ScheduledJobResult, Schema, SequenceNumber, TableName, TableSchema,
    TenantId, Timestamp, WriteOp, WriteOpType,
};
pub use neovex_engine::{
    ControlPlaneConfig, MonthlyActiveUsersSnapshot, PersistenceDialect, PersistenceTopology,
    PoolConfig, ProviderCredentials, Service, ServicePersistenceConfig, SubscriptionUpdate,
    TenantProviderConfig, TenantRoutingConfig, evaluate_paginated, evaluate_query, run_scheduler,
};
pub use neovex_runtime::{
    ConvexRuntime, ConvexRuntimeError, HostBridge, HostBridgeFuture, HostCallRequest,
    InvocationKind, InvocationRequest, NeovexRuntime, NeovexRuntimeError, RuntimeBackendKind,
    RuntimeBundle, RuntimeExecutionModel, RuntimeExecutor, RuntimeInvocationContext, RuntimeLimits,
    RuntimePolicy, VerifiedUserIdentity, VerifiedUserIdentityKind,
};
pub use neovex_server::{
    ConvexRegistry, DEFAULT_LICENSE_PATH, LICENSE_FILE_ENV, LicenseDocument, LicenseEntitlements,
    LicenseKind, LicenseLoadError, LicenseSnapshot, LicenseSourceInfo, LicenseSourceKind,
    LicenseState, LicenseStatus, LicenseUsageSnapshot, build_router, build_router_with_convex,
    build_router_with_convex_and_license, build_router_with_license, serve, serve_with_convex,
    serve_with_convex_and_license, serve_with_license,
};
pub use neovex_storage::EmbeddedProviderKind;
pub use neovex_storage::TenantStore;
