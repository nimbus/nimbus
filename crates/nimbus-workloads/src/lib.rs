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
    MAX_WORKLOAD_EXECUTABLE_CONTENT_BYTES, ProposedWorkloadTeardownTransition,
    WORKLOAD_EXECUTABLE_FORMAT_VERSION, WORKLOAD_SAGA_FORMAT_VERSION, WORKLOAD_SAGA_RECOVERY_ORDER,
    WorkloadActivationIntent, WorkloadAdmissionEvidence, WorkloadCleanupPendingDetail,
    WorkloadDesiredDigest, WorkloadEffectReferences, WorkloadExecutableContentDigest,
    WorkloadExecutableEncoding, WorkloadExecutableIntent, WorkloadExecutionAttemptId,
    WorkloadExecutionId, WorkloadExecutionProviderId, WorkloadExecutionReference,
    WorkloadFailureEvidence, WorkloadGeneration, WorkloadInspectionRequirement,
    WorkloadInspectionVersion, WorkloadNetworkIntent, WorkloadNetworkReference,
    WorkloadOwnerEvidenceDigest, WorkloadOwnerObservation, WorkloadPhaseDetail,
    WorkloadProvisionAbsenceEvidence, WorkloadProvisionAttempt, WorkloadProvisionAttemptId,
    WorkloadProvisionAttemptInput, WorkloadProvisionCommandId, WorkloadProvisionCommandMode,
    WorkloadProvisionDetail, WorkloadProvisionDispatchAuthorization,
    WorkloadProvisionDispatchClaim, WorkloadProvisionDispatchEpoch, WorkloadProvisionDisposition,
    WorkloadProvisionEffectResult, WorkloadProvisionInspectionResult,
    WorkloadProvisionPrerequisiteEvidence, WorkloadProvisionProviderTarget,
    WorkloadProvisionSourceDigest, WorkloadProvisionSourceEvidence,
    WorkloadProvisionSourceGeneration, WorkloadProvisionSourceIdentity,
    WorkloadProvisionSourceKind, WorkloadProvisionSourceResourceVersion, WorkloadProvisionStep,
    WorkloadProvisionSubjects, WorkloadProvisionSuccessEvidence, WorkloadProvisionTeardownAbsence,
    WorkloadPublicationIntent, WorkloadPublicationReference, WorkloadRecordedDetail,
    WorkloadRestartAbsenceEvidence, WorkloadRestartAdmission, WorkloadRestartAdmissionInput,
    WorkloadRestartAdmissionUpdate, WorkloadRestartCommandClaim, WorkloadRestartCommandId,
    WorkloadRestartCommandReceipt, WorkloadRestartDispatchAuthorization,
    WorkloadRestartDispatchEpoch, WorkloadRestartDisposition, WorkloadRestartEffectResult,
    WorkloadRestartEpoch, WorkloadRestartEvidenceDigest, WorkloadRestartHistory,
    WorkloadRestartNotBeforeUnixMillis, WorkloadRestartPhase, WorkloadRestartPolicy,
    WorkloadRestartRecoveryDecision, WorkloadRestartRequestId, WorkloadRestartState,
    WorkloadRestartStep, WorkloadRestartTeardownSettlement, WorkloadRestartTrigger,
    WorkloadSagaError, WorkloadSagaId, WorkloadSagaIntent, WorkloadSagaIntentUpdate,
    WorkloadSagaKey, WorkloadSagaPhase, WorkloadSagaRecord, WorkloadSagaRevision,
    WorkloadSagaTransition, WorkloadSagaTransitionId, WorkloadTeardownAttempt,
    WorkloadTeardownAttemptId, WorkloadTeardownAttemptInput, WorkloadTeardownCause,
    WorkloadTeardownClaim, WorkloadTeardownCommandId, WorkloadTeardownCommandMode,
    WorkloadTeardownContext, WorkloadTeardownDecision, WorkloadTeardownDetail,
    WorkloadTeardownDispatchAuthorization, WorkloadTeardownDispatchEpoch,
    WorkloadTeardownDisposition, WorkloadTeardownEffectResult, WorkloadTeardownInspectionResult,
    WorkloadTeardownProviderTarget, WorkloadTeardownReceipt, WorkloadTeardownResultConfirmation,
    WorkloadTeardownRetryEvidence, WorkloadTeardownStep, WorkloadTeardownSubjects,
    WorkloadTeardownSuccessEvidence, WorkloadTeardownSuccessorFence,
    WorkloadTerminalEvidenceDigest, WorkloadTerminalObservation,
};
pub use scheduling::{
    NodeCapacity, PlacementPlan, SchedulingExplanation, WorkloadPlacementEngine, WorkloadScheduler,
};
pub use store::{
    MAX_WORKLOAD_SAGA_PAGE_SIZE, WorkloadRestartCandidateCursor, WorkloadRestartCandidatePage,
    WorkloadRestartCandidatePageRequest, WorkloadSagaCommit, WorkloadSagaExpected,
    WorkloadSagaFuture, WorkloadSagaPage, WorkloadSagaPageRequest, WorkloadSagaRecoveryCursor,
    WorkloadSagaStore, WorkloadSagaStoreError, WorkloadSagaTenantCursor, WorkloadSagaTenantPage,
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
