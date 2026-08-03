//! Workload admission, desired-state, placement, and execution-control seams.

mod assignment;
mod desired;
mod network_plan;
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
pub use network_plan::{
    CompiledWorkloadNetworkPlan, WORKLOAD_NETWORK_PLAN_FORMAT_VERSION,
    WorkloadNetworkAttachmentBlueprint, WorkloadNetworkDependencyListenerBlueprint,
    WorkloadNetworkEndpointSemantics, WorkloadNetworkForwardingBehavior,
    WorkloadNetworkListenerBlueprint, WorkloadNetworkPlanContent, WorkloadNetworkPlanError,
    WorkloadNetworkPlanIdentity, WorkloadNetworkPortRequestMode, WorkloadNetworkRouteBlueprint,
};
pub use saga::{
    MAX_WORKLOAD_EXECUTABLE_CONTENT_BYTES, WORKLOAD_EXECUTABLE_FORMAT_VERSION,
    WORKLOAD_SAGA_FORMAT_VERSION, WORKLOAD_SAGA_RECOVERY_ORDER, WorkloadActivationIntent,
    WorkloadAdmissionEvidence, WorkloadCleanupPendingDetail, WorkloadDesiredDigest,
    WorkloadEffectReferences, WorkloadExecutableContentDigest, WorkloadExecutableEncoding,
    WorkloadExecutableIntent, WorkloadExecutionId, WorkloadExecutionReference,
    WorkloadFailureEvidence, WorkloadGeneration, WorkloadInspectionRequirement,
    WorkloadNetworkIntent, WorkloadNetworkReference, WorkloadOwnerEvidenceDigest,
    WorkloadOwnerObservation, WorkloadPhaseDetail, WorkloadProvisionAttempt,
    WorkloadProvisionAttemptId, WorkloadProvisionAttemptInput, WorkloadProvisionDetail,
    WorkloadProvisionDisposition, WorkloadProvisionEffectResult,
    WorkloadProvisionPrerequisiteEvidence, WorkloadProvisionSourceDigest,
    WorkloadProvisionSourceEvidence, WorkloadProvisionSourceGeneration,
    WorkloadProvisionSourceIdentity, WorkloadProvisionSourceKind,
    WorkloadProvisionSourceResourceVersion, WorkloadProvisionStep, WorkloadProvisionSubjects,
    WorkloadProvisionSuccessEvidence, WorkloadPublicationIntent, WorkloadPublicationReference,
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
    WorkloadSagaStoreError, WorkloadSagaTenantCursor, WorkloadSagaTenantPage,
    WorkloadSagaTenantPageRequest,
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
