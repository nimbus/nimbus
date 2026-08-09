//! Guest-owned sinks for exact compute-confirmed workload phases.
//!
//! This module deliberately has no desired-state admission, saga store, retry
//! loop, or phase coordinator. The parent compute saga chooses one phase; the
//! authenticated Machine API transports it; this guest adapter performs or
//! inspects only that phase behind provider-local fencing.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;

use axum::http::StatusCode;
use nimbus::{Error, SandboxBackendKind, SandboxId, SandboxStatus};
use nimbus_machine::{
    MachineForwarderAuthority,
    api::{
        MachineApiWorkloadProvisionCommandEnvelope, MachineApiWorkloadProvisionObservation,
        MachineApiWorkloadRestartCommandEnvelope, MachineApiWorkloadRestartObservation,
    },
};
use nimbus_node::{
    HostLifecycleBackend, HostLifecycleBackendKind, HostLifecycleRequest,
    NodeBackendCapabilitySource, NodeIdentity, RunnerSpec, TenantWorkloadPhase,
};
use nimbus_sandbox::{
    SandboxBackend, SandboxCleanupObservation, SandboxExecutionObservation, SandboxInspection,
    SandboxRestartAssessment, SandboxRestartIneligibility,
    backends::container::{
        ContainerSandboxBackend, ContainerSandboxStateView, MachinePortAbsenceEvidence,
    },
};
use nimbus_workloads::WorkloadExecutionId;

use super::{MachineApiHttpError, sandbox_error_to_http_error};

pub(super) mod provision;
pub(super) mod restart;

const SERVICE_WORKLOAD_DEFAULT_CPU_WEIGHT: u64 = 100;
const SERVICE_WORKLOAD_CPU_WEIGHT_PER_VCPU: u64 = 100;
const SERVICE_WORKLOAD_MAX_CPU_WEIGHT: u64 = 10_000;
const SERVICE_WORKLOAD_DEFAULT_TASKS_MAX: u64 = 512;

pub(super) type MachineApiServiceFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, MachineApiHttpError>> + Send + 'a>>;

/// Narrow Machine API surface implemented by the guest workload owner.
///
/// `inspect` and `stop` are retained read/retirement operations. Provisioning
/// and restart are possible only through their exact phase commands; there
/// are intentionally no coarse `start` or `restart` operations.
pub(crate) trait MachineApiNodeWorkloadFacade: Send + Sync {
    fn kind(&self) -> SandboxBackendKind;

    fn service_execution_blockers(&self) -> Vec<String> {
        Vec::new()
    }

    /// Operation-specific blockers for the strict restart phase sink.
    ///
    /// A generic facade must fail closed. Only the real guest owner overrides
    /// this method together with `restart_phase`.
    fn restart_execution_blockers(&self) -> Vec<String> {
        vec!["machine API workload facade has no strict restart-phase sink".to_owned()]
    }

    fn inspect<'a>(
        &'a self,
        id: &'a SandboxId,
    ) -> MachineApiServiceFuture<'a, Option<SandboxInspection>>;

    fn stop<'a>(&'a self, id: &'a SandboxId) -> MachineApiServiceFuture<'a, ()>;

    fn absent_machine_port_receipts<'a>(
        &'a self,
        id: &'a SandboxId,
    ) -> MachineApiServiceFuture<'a, Option<MachinePortAbsenceEvidence>>;

    fn provision_phase<'a>(
        &'a self,
        _command: &'a MachineApiWorkloadProvisionCommandEnvelope,
        _forwarder_authority: &'a MachineForwarderAuthority,
    ) -> MachineApiServiceFuture<'a, MachineApiWorkloadProvisionObservation> {
        Box::pin(async move {
            Err(MachineApiHttpError {
                status: StatusCode::SERVICE_UNAVAILABLE,
                message: "machine API workload facade has no strict provision-phase sink"
                    .to_owned(),
            })
        })
    }

    fn restart_phase<'a>(
        &'a self,
        _command: &'a MachineApiWorkloadRestartCommandEnvelope,
    ) -> MachineApiServiceFuture<'a, MachineApiWorkloadRestartObservation> {
        Box::pin(async move {
            Err(MachineApiHttpError {
                status: StatusCode::SERVICE_UNAVAILABLE,
                message: "machine API workload facade has no strict restart-phase sink".to_owned(),
            })
        })
    }
}

pub(crate) struct GuestNodeWorkloadService {
    node_id: NodeIdentity,
    lifecycle_backend: Arc<dyn HostLifecycleBackend>,
    lifecycle_blockers: Vec<String>,
    bundle_materializer: Arc<ContainerSandboxBackend>,
    state_view: ContainerSandboxStateView,
}

impl GuestNodeWorkloadService {
    pub(crate) fn new<B>(
        node_id: NodeIdentity,
        lifecycle_backend: B,
        bundle_materializer: Arc<ContainerSandboxBackend>,
        state_root: impl Into<PathBuf>,
    ) -> Self
    where
        B: HostLifecycleBackend + NodeBackendCapabilitySource,
    {
        let lifecycle_blockers = lifecycle_backend
            .node_backend_capabilities()
            .into_iter()
            .filter(|capabilities| {
                capabilities.backend() == HostLifecycleBackendKind::SystemdTransientUnit
                    && !capabilities.available()
            })
            .flat_map(|capabilities| {
                if capabilities.failure_reasons().is_empty() {
                    vec![
                        "guest node lifecycle backend unavailable: systemd transient unit backend is unavailable"
                            .to_owned(),
                    ]
                } else {
                    capabilities
                        .failure_reasons()
                        .iter()
                        .map(|reason| {
                            format!("guest node lifecycle backend unavailable: {reason}")
                        })
                        .collect()
                }
            })
            .collect();
        Self {
            node_id,
            lifecycle_backend: Arc::new(lifecycle_backend),
            lifecycle_blockers,
            bundle_materializer,
            state_view: ContainerSandboxStateView::new(state_root),
        }
    }
}

impl MachineApiNodeWorkloadFacade for GuestNodeWorkloadService {
    fn kind(&self) -> SandboxBackendKind {
        SandboxBackendKind::Container
    }

    fn service_execution_blockers(&self) -> Vec<String> {
        self.lifecycle_blockers.clone()
    }

    fn restart_execution_blockers(&self) -> Vec<String> {
        self.lifecycle_blockers.clone()
    }

    fn inspect<'a>(
        &'a self,
        id: &'a SandboxId,
    ) -> MachineApiServiceFuture<'a, Option<SandboxInspection>> {
        Box::pin(async move {
            let Some(base) = self
                .bundle_materializer
                .inspect(id)
                .await
                .map_err(sandbox_error_to_http_error)?
            else {
                return Ok(None);
            };
            if base.cleanup == SandboxCleanupObservation::Finalized {
                return Ok(Some(base));
            }

            let execution_id =
                WorkloadExecutionId::try_from(id.as_str().to_owned()).map_err(|error| {
                    MachineApiHttpError {
                        status: StatusCode::BAD_REQUEST,
                        message: format!(
                            "machine API inspection requires an execution identity: {error}"
                        ),
                    }
                })?;
            let (observed_phase, provider_evidence) =
                match self.lifecycle_backend.inspect(execution_id).await {
                    Ok(status) => {
                        let evidence =
                            serde_json::to_vec(&status).map_err(|error| MachineApiHttpError {
                                status: StatusCode::INTERNAL_SERVER_ERROR,
                                message: format!(
                                    "failed to encode guest lifecycle inspection evidence: {error}"
                                ),
                            })?;
                        (Some(status.phase()), evidence)
                    }
                    Err(Error::NotFound(_)) => (None, b"guest lifecycle execution absent".to_vec()),
                    Err(error) => return Err(core_error_to_http(error)),
                };
            Ok(Some(project_live_lifecycle(
                base,
                observed_phase,
                &provider_evidence,
            )))
        })
    }

    fn stop<'a>(&'a self, id: &'a SandboxId) -> MachineApiServiceFuture<'a, ()> {
        Box::pin(async move {
            let execution_id =
                WorkloadExecutionId::try_from(id.as_str().to_owned()).map_err(|error| {
                    MachineApiHttpError {
                        status: StatusCode::BAD_REQUEST,
                        message: format!(
                            "machine API retirement requires an execution identity: {error}"
                        ),
                    }
                })?;
            match self.lifecycle_backend.stop(execution_id).await {
                Ok(_) | Err(Error::NotFound(_)) => {}
                Err(error) => return Err(core_error_to_http(error)),
            }
            self.bundle_materializer
                .stop(id)
                .await
                .map_err(sandbox_error_to_http_error)
        })
    }

    fn absent_machine_port_receipts<'a>(
        &'a self,
        id: &'a SandboxId,
    ) -> MachineApiServiceFuture<'a, Option<MachinePortAbsenceEvidence>> {
        Box::pin(async move {
            self.bundle_materializer
                .absent_machine_port_evidence(id)
                .map_err(sandbox_error_to_http_error)
        })
    }

    fn provision_phase<'a>(
        &'a self,
        command: &'a MachineApiWorkloadProvisionCommandEnvelope,
        forwarder_authority: &'a MachineForwarderAuthority,
    ) -> MachineApiServiceFuture<'a, MachineApiWorkloadProvisionObservation> {
        Box::pin(provision::dispatch(self, command, forwarder_authority))
    }

    fn restart_phase<'a>(
        &'a self,
        command: &'a MachineApiWorkloadRestartCommandEnvelope,
    ) -> MachineApiServiceFuture<'a, MachineApiWorkloadRestartObservation> {
        Box::pin(restart::dispatch(self, command))
    }
}

fn project_live_lifecycle(
    base: SandboxInspection,
    observed_phase: Option<TenantWorkloadPhase>,
    provider_evidence: &[u8],
) -> SandboxInspection {
    let mut handle = base.handle.clone();
    let (execution, restart, cleanup) = if base.cleanup == SandboxCleanupObservation::Retained {
        handle.status = SandboxStatus::Stopping;
        (
            base.execution,
            base.restart,
            SandboxCleanupObservation::Retained,
        )
    } else {
        match observed_phase {
            Some(TenantWorkloadPhase::Deleting | TenantWorkloadPhase::Denied) => {
                handle.status = SandboxStatus::Stopping;
                (
                    SandboxExecutionObservation::Present,
                    SandboxRestartAssessment::Ineligible {
                        reason: SandboxRestartIneligibility::CleanupPending,
                    },
                    SandboxCleanupObservation::Retained,
                )
            }
            Some(phase) => {
                handle.status = sandbox_status_from_node_phase(phase);
                (
                    SandboxExecutionObservation::Present,
                    SandboxRestartAssessment::Ineligible {
                        reason: SandboxRestartIneligibility::RuntimePresent,
                    },
                    SandboxCleanupObservation::NotRequired,
                )
            }
            None => {
                handle.status = SandboxStatus::Stopping;
                (
                    SandboxExecutionObservation::AbsentWithoutExit,
                    SandboxRestartAssessment::Ineligible {
                        reason: SandboxRestartIneligibility::RuntimeAbsenceUnproven,
                    },
                    SandboxCleanupObservation::Retained,
                )
            }
        }
    };
    if handle.status != SandboxStatus::Ready {
        handle.published_endpoints.clear();
    }
    base.with_provider_projection_evidence(handle, execution, restart, cleanup, provider_evidence)
}

fn sandbox_status_from_node_phase(phase: TenantWorkloadPhase) -> SandboxStatus {
    match phase {
        TenantWorkloadPhase::Pending | TenantWorkloadPhase::Bound => SandboxStatus::Starting,
        TenantWorkloadPhase::Running | TenantWorkloadPhase::Degraded => SandboxStatus::NotReady,
        TenantWorkloadPhase::Ready => SandboxStatus::Ready,
        TenantWorkloadPhase::Deleting => SandboxStatus::Stopping,
        TenantWorkloadPhase::Denied => SandboxStatus::Failed,
    }
}

fn service_container_runner_request(
    bundle_dir: &Path,
    resources: &nimbus::SandboxResourceLimits,
) -> Result<HostLifecycleRequest, MachineApiHttpError> {
    let bundle_path = bundle_dir.to_str().ok_or_else(|| MachineApiHttpError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        message: format!(
            "container runner bundle path is not valid UTF-8: {}",
            bundle_dir.display()
        ),
    })?;
    let mut runner = RunnerSpec::container(bundle_path).map_err(core_error_to_http)?;
    if let Some(memory_limit_bytes) = resources.memory_limit_bytes {
        runner = runner.with_memory_max_bytes(memory_limit_bytes);
    }
    runner = runner
        .with_cpu_weight(service_workload_cpu_weight(resources)?)
        .with_tasks_max(SERVICE_WORKLOAD_DEFAULT_TASKS_MAX);
    runner
        .into_host_lifecycle_request(HostLifecycleBackendKind::SystemdTransientUnit)
        .map_err(core_error_to_http)
}

fn service_workload_cpu_weight(
    resources: &nimbus::SandboxResourceLimits,
) -> Result<u64, MachineApiHttpError> {
    let Some(cpu_count) = resources.cpu_count else {
        return Ok(SERVICE_WORKLOAD_DEFAULT_CPU_WEIGHT);
    };
    if cpu_count == 0 {
        return Err(MachineApiHttpError {
            status: StatusCode::BAD_REQUEST,
            message: "service workload cpu_count must be greater than zero".to_owned(),
        });
    }
    Ok(
        (u64::from(cpu_count) * SERVICE_WORKLOAD_CPU_WEIGHT_PER_VCPU)
            .min(SERVICE_WORKLOAD_MAX_CPU_WEIGHT),
    )
}

fn bundle_dir_from_manifest_path(manifest_path: &Path) -> Result<PathBuf, MachineApiHttpError> {
    let sandbox_root = manifest_path
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .and_then(Path::parent)
        .ok_or_else(|| MachineApiHttpError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: format!(
                "container manifest path is not under a sandbox state root: {}",
                manifest_path.display()
            ),
        })?;
    Ok(sandbox_root.join("bundle"))
}

fn core_error_to_http(error: Error) -> MachineApiHttpError {
    let status = match &error {
        Error::InvalidInput(_) => StatusCode::BAD_REQUEST,
        Error::PermissionDenied(_) => StatusCode::FORBIDDEN,
        Error::NotFound(_) => StatusCode::NOT_FOUND,
        Error::ResourceExhausted(_) => StatusCode::TOO_MANY_REQUESTS,
        Error::Conflict { .. } | Error::PreconditionFailed(_) => StatusCode::CONFLICT,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    MachineApiHttpError {
        status,
        message: error.to_string(),
    }
}

#[cfg(test)]
pub(crate) fn machine_api_node_workload_facade_from_sandbox_backend(
    backend: Arc<dyn nimbus::SandboxBackend>,
) -> Arc<dyn MachineApiNodeWorkloadFacade> {
    Arc::new(TestSandboxBackendNodeWorkloadFacade {
        backend,
        confirmed_absence: None,
    })
}

#[cfg(test)]
pub(crate) fn machine_api_node_workload_facade_from_sandbox_backend_with_absence(
    backend: Arc<dyn nimbus::SandboxBackend>,
    confirmed_absence: MachinePortAbsenceEvidence,
) -> Arc<dyn MachineApiNodeWorkloadFacade> {
    Arc::new(TestSandboxBackendNodeWorkloadFacade {
        backend,
        confirmed_absence: Some(confirmed_absence),
    })
}

#[cfg(test)]
struct TestSandboxBackendNodeWorkloadFacade {
    backend: Arc<dyn nimbus::SandboxBackend>,
    confirmed_absence: Option<MachinePortAbsenceEvidence>,
}

#[cfg(test)]
impl MachineApiNodeWorkloadFacade for TestSandboxBackendNodeWorkloadFacade {
    fn kind(&self) -> SandboxBackendKind {
        self.backend.kind()
    }

    fn inspect<'a>(
        &'a self,
        id: &'a SandboxId,
    ) -> MachineApiServiceFuture<'a, Option<SandboxInspection>> {
        Box::pin(async move {
            self.backend
                .inspect(id)
                .await
                .map_err(sandbox_error_to_http_error)
        })
    }

    fn stop<'a>(&'a self, id: &'a SandboxId) -> MachineApiServiceFuture<'a, ()> {
        Box::pin(async move {
            self.backend
                .stop(id)
                .await
                .map_err(sandbox_error_to_http_error)
        })
    }

    fn absent_machine_port_receipts<'a>(
        &'a self,
        _id: &'a SandboxId,
    ) -> MachineApiServiceFuture<'a, Option<MachinePortAbsenceEvidence>> {
        let confirmed_absence = self.confirmed_absence.clone();
        Box::pin(async move { Ok(confirmed_absence) })
    }
}

#[cfg(test)]
mod lifecycle_projection_tests {
    use super::*;

    fn plan_only_ready_inspection() -> SandboxInspection {
        serde_json::from_value(serde_json::json!({
            "handle": {
                "tenant_id": "tenant-machine-lifecycle",
                "id": "wex_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "name": "service",
                "backend": "container",
                "status": "ready",
                "published_endpoints": [],
            },
            "execution_attempt": { "state": "plan_only" },
            "execution": { "state": "plan_only" },
            "restart": { "assessment": "ineligible", "reason": "plan_only" },
            "cleanup": "not_required",
            "version": vec![0_u8; 32],
        }))
        .expect("plan-only inspection fixture should decode")
    }

    #[test]
    fn guest_inspection_requires_live_node_lifecycle_evidence() {
        let missing = project_live_lifecycle(
            plan_only_ready_inspection(),
            None,
            b"guest lifecycle execution absent",
        );
        assert_eq!(missing.handle.status, SandboxStatus::Stopping);
        assert_eq!(
            missing.execution,
            SandboxExecutionObservation::AbsentWithoutExit
        );
        assert_eq!(missing.cleanup, SandboxCleanupObservation::Retained);

        let running = project_live_lifecycle(
            plan_only_ready_inspection(),
            Some(TenantWorkloadPhase::Running),
            b"running",
        );
        assert_eq!(running.handle.status, SandboxStatus::NotReady);
        assert_eq!(running.execution, SandboxExecutionObservation::Present);

        let ready = project_live_lifecycle(
            plan_only_ready_inspection(),
            Some(TenantWorkloadPhase::Ready),
            b"ready",
        );
        assert_eq!(ready.handle.status, SandboxStatus::Ready);
        assert_eq!(ready.execution, SandboxExecutionObservation::Present);
    }
}
