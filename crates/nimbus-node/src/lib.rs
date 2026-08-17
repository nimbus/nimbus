mod direct_process;
mod host_lifecycle;
mod memory_pressure;
mod reconciler;
mod status;
mod systemd_transient;

pub use direct_process::{DirectProcessBackend, DirectProcessEvidence};
pub use host_lifecycle::{
    HostBackendObservedState, HostExecutable, HostExecutionDrainProvider,
    HostExecutionStopProvider, HostLifecycleBackend, HostLifecycleBackendCapabilities,
    HostLifecycleBackendKind, HostLifecycleFuture, HostLifecycleJournalSelectorEvidence,
    HostLifecyclePlan, HostLifecycleProperty, HostLifecyclePropertySet, HostLifecycleRequest,
    HostLifecycleStatus, HostLifecycleStatusReason, HostRestartPolicy, HostRestartProviderClaim,
    HostRestartProviderClaimInput, HostTeardownExecuteClaim, HostTeardownExecuteObservation,
    HostTeardownFuture, HostTeardownInspectClaim, HostTeardownInspectObservation,
    HostTeardownProviderClaimInput, RunnerKind, RunnerSpec, RuntimePoolTrustClass,
    RuntimePoolTrustState, SystemdUnitKind, SystemdUnitName, TenantWorkloadLifecycleEvidence,
};
pub use memory_pressure::{
    CgroupV2CpuPressureThresholds, CgroupV2HostPressureSource, CgroupV2MemoryPressureSource,
    HostCpuPressureObservation, HostMemoryPressureObservation, HostPressureObservation,
};
pub(crate) use nimbus_workloads::{
    LocalEnforcementBinding, TenantSystemEvidenceProjection, TenantWorkloadDeletionState,
    TenantWorkloadSpec,
};
pub use nimbus_workloads::{NodeIdentity, WorkloadExecutionId};
#[cfg(test)]
pub(crate) use nimbus_workloads::{TenantFinalizerRecord, WorkloadGeneration};
pub use reconciler::{
    NodeAgent, NodeAgentAssignment, NodeAgentCapabilityReport, NodeAgentReconcileReport,
    NodeAgentTransportAdmission, NodeAssignmentDisposition, NodeBackendCapabilitySource,
    NodeWorkloadDesiredState, NodeWorkloadReconcileAction, NodeWorkloadReconcileCapability,
    NodeWorkloadReconcileOutcome, NodeWorkloadReconciler, StatusEvidenceWrite,
    StatusEvidenceWriter,
};
pub use status::{
    NodeStatusAuthorizer, TenantNodeObservationIds, TenantObservedResourceUsage,
    TenantWorkloadCleanupProgress, TenantWorkloadCondition, TenantWorkloadConditionStatus,
    TenantWorkloadConditionType, TenantWorkloadDiagnostics, TenantWorkloadMetricLabels,
    TenantWorkloadPhase, TenantWorkloadStatus, TenantWorkloadStatusPatch,
    TenantWorkloadStatusPatchTarget, ensure_status_matches_projection,
};
#[cfg(all(target_os = "linux", feature = "systemd-dbus"))]
pub use systemd_transient::zbus_client::{BusKind, ZbusSystemdClient};
pub use systemd_transient::{
    StartTransientMode, SystemdDbusClient, SystemdDbusProperty, SystemdExecStart,
    SystemdInspectUnitRequest, SystemdJournalSelector, SystemdStartTransientUnitRequest,
    SystemdStartTransientUnitResponse, SystemdStopUnitRequest, SystemdStopUnitResponse,
    SystemdStopUnitSubmission, SystemdTransientCapabilities, SystemdTransientUnitBackend,
    SystemdUnitJobStatus, SystemdUnitStatus, UnavailableSystemdDbusClient,
};

#[cfg(test)]
mod tests;
