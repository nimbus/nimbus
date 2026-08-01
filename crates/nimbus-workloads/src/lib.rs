//! Workload admission, desired-state, placement, and execution-control seams.

mod assignment;
mod desired;
mod saga;
mod scheduling;
mod store;
mod tenant;

pub use assignment::{NodeAssignment, WorkloadStatusUpdate};
pub use desired::{
    DesiredWorkload, DesiredWorkloadKind, DesiredWorkloadState, EmbeddedNodeClient,
    WorkloadChannelDescriptor, WorkloadEvaluation, WorkloadEventQueue, WorkloadExecutionPhase,
    WorkloadExecutionStatus, WorkloadExecutor,
};
pub use saga::{
    WORKLOAD_SAGA_FORMAT_VERSION, WorkloadActivationIntent, WorkloadAdmissionEvidence,
    WorkloadCleanupPendingDetail, WorkloadDesiredDigest, WorkloadEffectReferences,
    WorkloadExecutionId, WorkloadExecutionReference, WorkloadFailureEvidence, WorkloadGeneration,
    WorkloadInspectionRequirement, WorkloadNetworkIntent, WorkloadNetworkReference,
    WorkloadOwnerEvidenceDigest, WorkloadOwnerObservation, WorkloadPhaseDetail,
    WorkloadProvisionDetail, WorkloadPublicationIntent, WorkloadPublicationReference,
    WorkloadRecordedDetail, WorkloadSagaError, WorkloadSagaId, WorkloadSagaIntent,
    WorkloadSagaIntentUpdate, WorkloadSagaKey, WorkloadSagaPhase, WorkloadSagaRecord,
    WorkloadSagaRevision, WorkloadSagaTransition, WorkloadSagaTransitionId, WorkloadTeardownDetail,
    WorkloadTerminalEvidenceDigest, WorkloadTerminalObservation,
};
pub use scheduling::{
    NodeCapacity, PlacementPlan, SchedulingExplanation, WorkloadPlacementEngine, WorkloadScheduler,
};
pub use store::{
    MAX_WORKLOAD_SAGA_PAGE_SIZE, WorkloadSagaCommit, WorkloadSagaExpected, WorkloadSagaFuture,
    WorkloadSagaPage, WorkloadSagaPageRequest, WorkloadSagaRecoveryCursor, WorkloadSagaStore,
    WorkloadSagaStoreError,
};
pub use tenant::{
    LocalEnforcementBinding, NodeIdentity, TenantCredentialProjectionBinding,
    TenantCredentialProjectionPolicy, TenantCredentialProjectionRequest,
    TenantCredentialProjectionScope, TenantEgressReloadRequest, TenantFinalizerRecord,
    TenantPolicyArea, TenantPolicyLifecycle, TenantServiceProjection, TenantStorageProjection,
    TenantSystemEvidenceProjection, TenantWorkloadDeletionState, TenantWorkloadResourcePolicy,
    TenantWorkloadSpec, TenantWorkloadUid, policy_lifecycle,
};

pub(crate) use desired::validate_component;
