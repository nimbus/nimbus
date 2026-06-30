//! Workload admission, desired-state, placement, and execution-control seams.

mod assignment;
mod desired;
mod scheduling;
mod tenant;

pub use assignment::{NodeAssignment, WorkloadStatusUpdate};
pub use desired::{
    DesiredWorkload, DesiredWorkloadKind, DesiredWorkloadSnapshot, DesiredWorkloadState,
    DesiredWorkloadStore, EmbeddedNodeClient, InMemoryDesiredWorkloadStore,
    WorkloadChannelDescriptor, WorkloadController, WorkloadEvaluation, WorkloadEventQueue,
    WorkloadExecutionPhase, WorkloadExecutionStatus, WorkloadExecutor,
};
pub use scheduling::{
    NodeCapacity, PlacementPlan, SchedulingExplanation, WorkloadPlacementEngine, WorkloadScheduler,
};
pub use tenant::{
    LocalEnforcementBinding, NodeIdentity, TenantCredentialProjectionBinding,
    TenantCredentialProjectionPolicy, TenantCredentialProjectionRequest,
    TenantCredentialProjectionScope, TenantEgressReloadRequest, TenantFinalizerRecord,
    TenantPolicyArea, TenantPolicyLifecycle, TenantServiceProjection, TenantStorageProjection,
    TenantSystemEvidenceProjection, TenantWorkloadDeletionState, TenantWorkloadGeneration,
    TenantWorkloadResourcePolicy, TenantWorkloadSpec, TenantWorkloadUid, policy_lifecycle,
};

pub(crate) use desired::validate_component;
