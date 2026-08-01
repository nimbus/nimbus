mod direct_process;
mod host_lifecycle;
mod memory_pressure;
mod reconciler;
mod status;
mod systemd_transient;

pub use direct_process::{DirectProcessBackend, DirectProcessEvidence};
pub use host_lifecycle::{
    HostBackendObservedState, HostExecutable, HostLifecycleBackend,
    HostLifecycleBackendCapabilities, HostLifecycleBackendKind, HostLifecycleFuture,
    HostLifecycleJournalSelectorEvidence, HostLifecyclePlan, HostLifecycleProperty,
    HostLifecyclePropertySet, HostLifecycleRequest, HostLifecycleStatus, HostLifecycleStatusReason,
    HostRestartPolicy, RunnerKind, RunnerSpec, RuntimePoolTrustClass, RuntimePoolTrustState,
    SystemdUnitKind, SystemdUnitName, TenantWorkloadId, TenantWorkloadLifecycleEvidence,
};
pub use memory_pressure::{
    CgroupV2CpuPressureThresholds, CgroupV2HostPressureSource, CgroupV2MemoryPressureSource,
    HostCpuPressureObservation, HostMemoryPressureObservation, HostPressureObservation,
};
pub use nimbus_workloads::NodeIdentity;
pub(crate) use nimbus_workloads::{
    LocalEnforcementBinding, TenantSystemEvidenceProjection, TenantWorkloadDeletionState,
    TenantWorkloadSpec,
};
#[cfg(test)]
pub(crate) use nimbus_workloads::{TenantFinalizerRecord, TenantWorkloadGeneration};
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
    SystemdTransientCapabilities, SystemdTransientUnitBackend, SystemdUnitStatus,
    UnavailableSystemdDbusClient,
};

#[cfg(test)]
mod tests;
