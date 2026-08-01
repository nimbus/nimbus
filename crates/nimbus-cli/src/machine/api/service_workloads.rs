use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
#[cfg(test)]
use std::sync::Mutex;
#[cfg(test)]
use std::{collections::BTreeMap, collections::btree_map::Entry};

use axum::http::StatusCode;
#[cfg(test)]
use nimbus::TenantId;
use nimbus::{
    Error, SandboxBackend, SandboxBackendKind, SandboxHandle, SandboxId, SandboxSpec, SandboxStatus,
};
use nimbus_node::{
    HostLifecycleBackend, HostLifecycleBackendKind, HostLifecyclePlan, HostLifecycleRequest,
    HostLifecycleStatus, NodeAgent, NodeAgentAssignment, NodeAssignmentDisposition,
    NodeBackendCapabilitySource, RunnerSpec, StatusEvidenceWriter, TenantWorkloadPhase,
};
use nimbus_sandbox::{
    MachinePortForwardReceipt, SandboxCleanupObservation, SandboxExecutionObservation,
    SandboxInspection, SandboxRestartAssessment, SandboxRestartIneligibility,
    backends::container::{
        ContainerSandboxBackend, ContainerSandboxStateView, MachinePortAbsenceEvidence,
    },
};
use nimbus_workloads::{LocalEnforcementBinding, TenantWorkloadSpec};

use crate::node_workload_executor::admit_workload_spec;

use super::state::container_state_error_to_http_error;
use super::{MachineApiHttpError, sandbox_error_to_http_error};

const SERVICE_WORKLOAD_DEFAULT_CPU_WEIGHT: u64 = 100;
const SERVICE_WORKLOAD_CPU_WEIGHT_PER_VCPU: u64 = 100;
const SERVICE_WORKLOAD_MAX_CPU_WEIGHT: u64 = 10_000;
const SERVICE_WORKLOAD_DEFAULT_TASKS_MAX: u64 = 512;

pub(super) type MachineApiServiceFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, MachineApiHttpError>> + Send + 'a>>;

pub(crate) trait MachineApiNodeWorkloadFacade: Send + Sync {
    fn kind(&self) -> SandboxBackendKind;
    fn service_execution_blockers(&self) -> Vec<String> {
        Vec::new()
    }
    fn start<'a>(
        &'a self,
        sandbox_id: SandboxId,
        spec: SandboxSpec,
    ) -> MachineApiServiceFuture<'a, SandboxHandle>;
    fn inspect<'a>(
        &'a self,
        id: &'a SandboxId,
    ) -> MachineApiServiceFuture<'a, Option<SandboxInspection>>;
    fn stop<'a>(&'a self, id: &'a SandboxId) -> MachineApiServiceFuture<'a, ()>;
    fn exposed_machine_port_receipts<'a>(
        &'a self,
        id: &'a SandboxId,
    ) -> MachineApiServiceFuture<'a, Vec<MachinePortForwardReceipt>>;
    fn absent_machine_port_receipts<'a>(
        &'a self,
        id: &'a SandboxId,
    ) -> MachineApiServiceFuture<'a, Option<MachinePortAbsenceEvidence>>;
}

pub(crate) struct GuestNodeWorkloadService<B, W> {
    node_agent: NodeAgent<B, W>,
    bundle_materializer: Arc<ContainerSandboxBackend>,
    state_view: ContainerSandboxStateView,
}

impl<B, W> GuestNodeWorkloadService<B, W> {
    pub(crate) fn new(
        node_agent: NodeAgent<B, W>,
        bundle_materializer: Arc<ContainerSandboxBackend>,
        state_root: impl Into<PathBuf>,
    ) -> Self {
        Self {
            node_agent,
            bundle_materializer,
            state_view: ContainerSandboxStateView::new(state_root),
        }
    }
}

impl<B, W> MachineApiNodeWorkloadFacade for GuestNodeWorkloadService<B, W>
where
    B: HostLifecycleBackend + NodeBackendCapabilitySource,
    W: StatusEvidenceWriter,
{
    fn kind(&self) -> SandboxBackendKind {
        SandboxBackendKind::Container
    }

    fn service_execution_blockers(&self) -> Vec<String> {
        let mut blockers = Vec::new();
        for capabilities in self
            .node_agent
            .capability_report()
            .backend_capabilities()
            .iter()
            .filter(|capabilities| {
                capabilities.backend() == HostLifecycleBackendKind::SystemdTransientUnit
                    && !capabilities.available()
            })
        {
            if capabilities.failure_reasons().is_empty() {
                blockers.push(
                    "guest node lifecycle backend unavailable: systemd transient unit backend is unavailable"
                        .to_owned(),
                );
            } else {
                blockers.extend(
                    capabilities.failure_reasons().iter().map(|reason| {
                        format!("guest node lifecycle backend unavailable: {reason}")
                    }),
                );
            }
        }
        blockers
    }

    fn start<'a>(
        &'a self,
        sandbox_id: SandboxId,
        spec: SandboxSpec,
    ) -> MachineApiServiceFuture<'a, SandboxHandle> {
        Box::pin(async move {
            let service_name = spec
                .service_name()
                .ok_or_else(|| MachineApiHttpError {
                    status: StatusCode::BAD_REQUEST,
                    message: "machine API node workload start requires service owner metadata"
                        .to_owned(),
                })?
                .to_owned();
            let resources = spec.resources.clone();
            let prepared = self
                .bundle_materializer
                .prepare_plan_only_service_workload_with_id(spec, sandbox_id)
                .map_err(sandbox_error_to_http_error)?;
            let status = self
                .reconcile_service_workload(
                    prepared.handle.tenant_id.as_str(),
                    &service_name,
                    prepared.bundle_dir.as_path(),
                    &resources,
                    false,
                )
                .await?;
            self.refresh_plan_only_manifest_status(
                &prepared.handle.id,
                sandbox_status_from_node_phase(status),
            )
            .await?
            .ok_or_else(|| MachineApiHttpError {
                status: StatusCode::NOT_FOUND,
                message: format!(
                    "prepared service workload disappeared before status publication: {}",
                    prepared.handle.id
                ),
            })
        })
    }

    fn inspect<'a>(
        &'a self,
        id: &'a SandboxId,
    ) -> MachineApiServiceFuture<'a, Option<SandboxInspection>> {
        Box::pin(async move {
            let Some(details) = self
                .state_view
                .inspect(id)
                .map_err(container_state_error_to_http_error)?
            else {
                return Ok(None);
            };
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
            let bundle_dir = bundle_dir_from_manifest_path(&details.manifest_path)?;
            let observed_phase = self
                .inspect_service_workload(
                    details.summary.tenant_id.as_str(),
                    &details.summary.service_name,
                    &bundle_dir,
                    &details.resources,
                )
                .await?;
            let provider_evidence = format!("{observed_phase:?}");
            let mut handle = base.handle.clone();
            let (execution, restart, cleanup) =
                if base.cleanup == SandboxCleanupObservation::Retained {
                    handle.status = SandboxStatus::Stopping;
                    (
                        base.execution,
                        base.restart,
                        SandboxCleanupObservation::Retained,
                    )
                } else {
                    match observed_phase {
                        Some(phase) => {
                            let observed_status = sandbox_status_from_node_phase(phase);
                            if matches!(
                                observed_status,
                                SandboxStatus::Stopped
                                    | SandboxStatus::Failed
                                    | SandboxStatus::Stopping
                            ) {
                                handle.status = SandboxStatus::Stopping;
                                (
                                    SandboxExecutionObservation::Present,
                                    SandboxRestartAssessment::Ineligible {
                                        reason: SandboxRestartIneligibility::CleanupPending,
                                    },
                                    SandboxCleanupObservation::Retained,
                                )
                            } else {
                                handle.status = observed_status;
                                (
                                    SandboxExecutionObservation::Present,
                                    SandboxRestartAssessment::Ineligible {
                                        reason: SandboxRestartIneligibility::RuntimePresent,
                                    },
                                    SandboxCleanupObservation::NotRequired,
                                )
                            }
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
            Ok(Some(base.with_provider_projection_evidence(
                handle,
                execution,
                restart,
                cleanup,
                provider_evidence.as_bytes(),
            )))
        })
    }

    fn stop<'a>(&'a self, id: &'a SandboxId) -> MachineApiServiceFuture<'a, ()> {
        Box::pin(async move {
            let Some(details) = self
                .state_view
                .inspect(id)
                .map_err(container_state_error_to_http_error)?
            else {
                return Err(MachineApiHttpError {
                    status: StatusCode::NOT_FOUND,
                    message: format!("sandbox instance was not found: {id}"),
                });
            };
            let bundle_dir = bundle_dir_from_manifest_path(&details.manifest_path)?;
            self.reconcile_service_workload(
                details.summary.tenant_id.as_str(),
                &details.summary.service_name,
                &bundle_dir,
                &details.resources,
                true,
            )
            .await?;
            self.bundle_materializer
                .stop(id)
                .await
                .map_err(sandbox_error_to_http_error)?;
            Ok(())
        })
    }

    fn exposed_machine_port_receipts<'a>(
        &'a self,
        id: &'a SandboxId,
    ) -> MachineApiServiceFuture<'a, Vec<MachinePortForwardReceipt>> {
        Box::pin(async move {
            self.bundle_materializer
                .exposed_machine_port_receipts(id)
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
}

impl<B, W> GuestNodeWorkloadService<B, W>
where
    B: HostLifecycleBackend,
    W: StatusEvidenceWriter,
{
    async fn reconcile_service_workload(
        &self,
        tenant_id: &str,
        service_name: &str,
        bundle_dir: &Path,
        resources: &nimbus::SandboxResourceLimits,
        stop: bool,
    ) -> Result<TenantWorkloadPhase, MachineApiHttpError> {
        let spec = service_tenant_workload_spec(
            tenant_id,
            service_name,
            self.node_agent.node_id().as_str(),
            stop,
        )?;
        let request = service_container_runner_request(bundle_dir, resources)?;
        let report = self
            .node_agent
            .reconcile_assignments([NodeAgentAssignment::from_spec(spec.clone(), request)])
            .await;
        let Some(disposition) = report.dispositions().first() else {
            return Err(MachineApiHttpError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: "node agent returned no disposition for service workload assignment"
                    .to_owned(),
            });
        };
        match disposition {
            NodeAssignmentDisposition::Reconciled { .. } => {}
            NodeAssignmentDisposition::Failed { reason, .. } => {
                return Err(MachineApiHttpError {
                    status: StatusCode::INTERNAL_SERVER_ERROR,
                    message: format!(
                        "node agent failed to reconcile service workload {tenant_id}/{service_name}: {reason}"
                    ),
                });
            }
        }
        let Some(outcome) = report.outcomes().first() else {
            return Err(MachineApiHttpError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: "node agent returned no outcome for service workload assignment"
                    .to_owned(),
            });
        };
        Ok(outcome.status().phase())
    }

    async fn inspect_service_workload(
        &self,
        tenant_id: &str,
        service_name: &str,
        bundle_dir: &Path,
        resources: &nimbus::SandboxResourceLimits,
    ) -> Result<Option<TenantWorkloadPhase>, MachineApiHttpError> {
        let spec = service_tenant_workload_spec(
            tenant_id,
            service_name,
            self.node_agent.node_id().as_str(),
            false,
        )?;
        let request = service_container_runner_request(bundle_dir, resources)?;
        match self.inspect_node_status(spec, request).await {
            Ok(status) => Ok(Some(status.phase())),
            Err(error) if error.status == StatusCode::NOT_FOUND => Ok(None),
            Err(error) => Err(error),
        }
    }

    async fn refresh_plan_only_manifest_status(
        &self,
        id: &SandboxId,
        status: SandboxStatus,
    ) -> Result<Option<SandboxHandle>, MachineApiHttpError> {
        self.bundle_materializer
            .refresh_plan_only_service_workload_status(id, status)
            .map_err(sandbox_error_to_http_error)
    }

    async fn inspect_node_status(
        &self,
        spec: TenantWorkloadSpec,
        request: HostLifecycleRequest,
    ) -> Result<HostLifecycleStatus, MachineApiHttpError> {
        let binding = LocalEnforcementBinding::from_spec(spec);
        let plan =
            HostLifecyclePlan::from_binding(&binding, request).map_err(core_error_to_http)?;
        self.node_agent
            .reconciler()
            .backend()
            .inspect(plan.workload_id().clone())
            .await
            .map_err(core_error_to_http)
    }
}

fn service_tenant_workload_spec(
    tenant_id: &str,
    service_name: &str,
    node_id: &str,
    stop: bool,
) -> Result<TenantWorkloadSpec, MachineApiHttpError> {
    admit_workload_spec(tenant_id, service_name, node_id, stop).map_err(|error| {
        MachineApiHttpError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: format!(
                "failed to admit machine API service workload {tenant_id}/{service_name}: {error}"
            ),
        }
    })
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

fn sandbox_status_from_node_phase(phase: TenantWorkloadPhase) -> SandboxStatus {
    match phase {
        TenantWorkloadPhase::Pending | TenantWorkloadPhase::Bound => SandboxStatus::Starting,
        TenantWorkloadPhase::Running | TenantWorkloadPhase::Ready => SandboxStatus::Ready,
        TenantWorkloadPhase::Deleting => SandboxStatus::Stopping,
        TenantWorkloadPhase::Degraded => SandboxStatus::NotReady,
        TenantWorkloadPhase::Denied => SandboxStatus::Failed,
    }
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
        identities: Mutex::new(BTreeMap::new()),
    })
}

#[cfg(test)]
struct TestSandboxBackendNodeWorkloadFacade {
    backend: Arc<dyn nimbus::SandboxBackend>,
    identities: Mutex<BTreeMap<String, TestSandboxIdentity>>,
}

#[cfg(test)]
#[derive(Clone)]
struct TestSandboxIdentity {
    backend_id: SandboxId,
    tenant_id: TenantId,
    has_publication_bindings: bool,
    observed_publication: TestPublicationObservation,
}

#[cfg(test)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum TestPublicationObservation {
    Exposed,
    Absent,
}

#[cfg(test)]
impl MachineApiNodeWorkloadFacade for TestSandboxBackendNodeWorkloadFacade {
    fn kind(&self) -> SandboxBackendKind {
        self.backend.kind()
    }

    fn start<'a>(
        &'a self,
        sandbox_id: SandboxId,
        spec: SandboxSpec,
    ) -> MachineApiServiceFuture<'a, SandboxHandle> {
        Box::pin(async move {
            let tenant_id = spec.tenant_id.clone();
            let has_publication_bindings = !spec.port_bindings.is_empty();
            let mut handle = self
                .backend
                .start(spec)
                .await
                .map_err(sandbox_error_to_http_error)?;
            let backend_id = handle.id.clone();
            match self
                .identities
                .lock()
                .expect("test sandbox identity lock")
                .entry(sandbox_id.as_str().to_owned())
            {
                Entry::Vacant(entry) => {
                    entry.insert(TestSandboxIdentity {
                        backend_id,
                        tenant_id,
                        has_publication_bindings,
                        observed_publication: TestPublicationObservation::Exposed,
                    });
                }
                Entry::Occupied(_) => {
                    return Err(MachineApiHttpError {
                        status: StatusCode::CONFLICT,
                        message: format!(
                            "test sandbox facade already owns caller-selected identity {sandbox_id}"
                        ),
                    });
                }
            }
            handle.id = sandbox_id;
            Ok(handle)
        })
    }

    fn inspect<'a>(
        &'a self,
        id: &'a SandboxId,
    ) -> MachineApiServiceFuture<'a, Option<SandboxInspection>> {
        Box::pin(async move {
            let backend_id = self
                .identities
                .lock()
                .expect("test sandbox identity lock")
                .get(id.as_str())
                .map(|identity| identity.backend_id.clone())
                .unwrap_or_else(|| id.clone());
            let inspection = self
                .backend
                .inspect(&backend_id)
                .await
                .map_err(sandbox_error_to_http_error)?;
            Ok(inspection.map(|inspection| {
                let mut handle = inspection.handle.clone();
                handle.id = id.clone();
                let execution = inspection.execution;
                let restart = inspection.restart;
                let cleanup = inspection.cleanup;
                inspection.with_provider_projection(handle, execution, restart, cleanup)
            }))
        })
    }

    fn stop<'a>(&'a self, id: &'a SandboxId) -> MachineApiServiceFuture<'a, ()> {
        Box::pin(async move {
            let backend_id = self
                .identities
                .lock()
                .expect("test sandbox identity lock")
                .get(id.as_str())
                .map(|identity| identity.backend_id.clone())
                .unwrap_or_else(|| id.clone());
            self.backend
                .stop(&backend_id)
                .await
                .map_err(sandbox_error_to_http_error)?;
            if let Some(identity) = self
                .identities
                .lock()
                .expect("test sandbox identity lock")
                .get_mut(id.as_str())
            {
                identity.observed_publication = TestPublicationObservation::Absent;
            }
            Ok(())
        })
    }

    fn exposed_machine_port_receipts<'a>(
        &'a self,
        id: &'a SandboxId,
    ) -> MachineApiServiceFuture<'a, Vec<MachinePortForwardReceipt>> {
        Box::pin(async move {
            self.empty_receipts_for_observed_fixture(id, TestPublicationObservation::Exposed)
        })
    }

    fn absent_machine_port_receipts<'a>(
        &'a self,
        id: &'a SandboxId,
    ) -> MachineApiServiceFuture<'a, Option<MachinePortAbsenceEvidence>> {
        Box::pin(async move {
            let tenant_id = self
                .identities
                .lock()
                .expect("test sandbox identity lock")
                .get(id.as_str())
                .map(|identity| identity.tenant_id.clone());
            let Some(tenant_id) = tenant_id else {
                return Ok(None);
            };
            let receipts =
                self.empty_receipts_for_observed_fixture(id, TestPublicationObservation::Absent)?;
            Ok(Some(MachinePortAbsenceEvidence {
                tenant_id,
                sandbox_id: id.clone(),
                receipts,
            }))
        })
    }
}

#[cfg(test)]
impl TestSandboxBackendNodeWorkloadFacade {
    fn empty_receipts_for_observed_fixture(
        &self,
        id: &SandboxId,
        expected: TestPublicationObservation,
    ) -> Result<Vec<MachinePortForwardReceipt>, MachineApiHttpError> {
        let identities = self.identities.lock().expect("test sandbox identity lock");
        let Some(identity) = identities.get(id.as_str()) else {
            return Err(MachineApiHttpError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: format!(
                    "test sandbox facade has no caller-selected identity record for {id}"
                ),
            });
        };
        if identity.has_publication_bindings {
            return Err(MachineApiHttpError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: format!(
                    "test sandbox facade cannot fabricate durable provider receipts for {id}"
                ),
            });
        }
        if identity.observed_publication != expected {
            return Err(MachineApiHttpError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: format!(
                    "test sandbox facade has no exact observed publication phase for {id}"
                ),
            });
        }
        Ok(Vec::new())
    }
}

#[cfg(test)]
pub(crate) fn machine_api_node_workload_facade_from_container_backend(
    backend: Arc<ContainerSandboxBackend>,
) -> Arc<dyn MachineApiNodeWorkloadFacade> {
    Arc::new(TestContainerBackendNodeWorkloadFacade {
        backend,
        zero_binding_observations: Mutex::new(BTreeMap::new()),
    })
}

#[cfg(test)]
struct TestContainerBackendNodeWorkloadFacade {
    backend: Arc<ContainerSandboxBackend>,
    zero_binding_observations: Mutex<BTreeMap<String, TestZeroBindingObservation>>,
}

#[cfg(test)]
#[derive(Clone)]
struct TestZeroBindingObservation {
    tenant_id: TenantId,
    phase: TestPublicationObservation,
}

#[cfg(test)]
impl MachineApiNodeWorkloadFacade for TestContainerBackendNodeWorkloadFacade {
    fn kind(&self) -> SandboxBackendKind {
        SandboxBackendKind::Container
    }

    fn start<'a>(
        &'a self,
        sandbox_id: SandboxId,
        spec: SandboxSpec,
    ) -> MachineApiServiceFuture<'a, SandboxHandle> {
        Box::pin(async move {
            let tenant_id = spec.tenant_id.clone();
            let has_publication_bindings = !spec.port_bindings.is_empty();
            let prepared = self
                .backend
                .prepare_plan_only_service_workload_with_id(spec, sandbox_id)
                .map_err(sandbox_error_to_http_error)?;
            if !has_publication_bindings {
                self.zero_binding_observations
                    .lock()
                    .expect("test zero-binding observation lock")
                    .insert(
                        prepared.handle.id.as_str().to_owned(),
                        TestZeroBindingObservation {
                            tenant_id,
                            phase: TestPublicationObservation::Exposed,
                        },
                    );
            }
            Ok(prepared.handle)
        })
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
                .map_err(sandbox_error_to_http_error)?;
            if let Some(observation) = self
                .zero_binding_observations
                .lock()
                .expect("test zero-binding observation lock")
                .get_mut(id.as_str())
            {
                observation.phase = TestPublicationObservation::Absent;
            }
            Ok(())
        })
    }

    fn exposed_machine_port_receipts<'a>(
        &'a self,
        id: &'a SandboxId,
    ) -> MachineApiServiceFuture<'a, Vec<MachinePortForwardReceipt>> {
        Box::pin(async move {
            if let Some(observation) = self
                .zero_binding_observations
                .lock()
                .expect("test zero-binding observation lock")
                .get(id.as_str())
                .cloned()
            {
                if observation.phase != TestPublicationObservation::Exposed {
                    return Err(MachineApiHttpError {
                        status: StatusCode::INTERNAL_SERVER_ERROR,
                        message: format!(
                            "test container facade has no exact exposed observation for {id}"
                        ),
                    });
                }
                return Ok(Vec::new());
            }
            self.backend
                .exposed_machine_port_receipts(id)
                .map_err(sandbox_error_to_http_error)
        })
    }

    fn absent_machine_port_receipts<'a>(
        &'a self,
        id: &'a SandboxId,
    ) -> MachineApiServiceFuture<'a, Option<MachinePortAbsenceEvidence>> {
        Box::pin(async move {
            if let Some(observation) = self
                .zero_binding_observations
                .lock()
                .expect("test zero-binding observation lock")
                .get(id.as_str())
                .cloned()
            {
                if observation.phase != TestPublicationObservation::Absent {
                    return Err(MachineApiHttpError {
                        status: StatusCode::INTERNAL_SERVER_ERROR,
                        message: format!(
                            "test container facade has no exact absent observation for {id}"
                        ),
                    });
                }
                return Ok(Some(MachinePortAbsenceEvidence {
                    tenant_id: observation.tenant_id,
                    sandbox_id: id.clone(),
                    receipts: Vec::new(),
                }));
            }
            self.backend
                .absent_machine_port_evidence(id)
                .map_err(sandbox_error_to_http_error)
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use nimbus::{
        SandboxBackendKind, SandboxOwnerSpec, SandboxProcessSpec, SandboxRootSpec, TenantId,
    };
    use nimbus_node::{
        HostBackendObservedState, HostLifecycleBackendCapabilities, HostLifecycleFuture,
        HostLifecyclePlan, HostLifecycleProperty, HostLifecycleStatus, StatusEvidenceWrite,
        TenantWorkloadId, TenantWorkloadStatus,
    };
    use nimbus_sandbox::backends::container::ContainerSandboxBackendConfig;
    use nimbus_workloads::NodeIdentity;

    use super::*;

    #[derive(Debug, Default, Clone)]
    struct RecordingLifecycleBackend {
        calls: Arc<Mutex<Vec<&'static str>>>,
        last_plan: Arc<Mutex<Option<HostLifecyclePlan>>>,
        workload_missing: Arc<Mutex<bool>>,
    }

    impl RecordingLifecycleBackend {
        fn calls(&self) -> Vec<&'static str> {
            self.calls.lock().expect("calls lock").clone()
        }

        fn last_plan(&self) -> HostLifecyclePlan {
            self.last_plan
                .lock()
                .expect("plan lock")
                .clone()
                .expect("plan should be recorded")
        }

        fn record(&self, call: &'static str) {
            self.calls.lock().expect("calls lock").push(call);
        }

        fn mark_workload_missing(&self) {
            *self.workload_missing.lock().expect("missing lock") = true;
        }

        fn mark_workload_present(&self) {
            *self.workload_missing.lock().expect("missing lock") = false;
        }

        fn workload_missing(&self) -> bool {
            *self.workload_missing.lock().expect("missing lock")
        }
    }

    impl HostLifecycleBackend for RecordingLifecycleBackend {
        fn validate(
            &self,
            binding: &LocalEnforcementBinding,
            request: HostLifecycleRequest,
        ) -> Result<HostLifecyclePlan, Error> {
            self.record("validate");
            let plan = HostLifecyclePlan::from_binding(binding, request)?;
            *self.last_plan.lock().expect("plan lock") = Some(plan.clone());
            Ok(plan)
        }

        fn start<'a>(
            &'a self,
            plan: HostLifecyclePlan,
        ) -> HostLifecycleFuture<'a, TenantWorkloadStatus> {
            Box::pin(async move {
                self.record("start");
                HostLifecycleStatus::from_backend_state(&plan, HostBackendObservedState::Ready)
                    .to_workload_status(&plan)
            })
        }

        fn stop<'a>(
            &'a self,
            _workload_id: TenantWorkloadId,
        ) -> HostLifecycleFuture<'a, HostLifecycleStatus> {
            Box::pin(async move {
                self.record("stop");
                self.mark_workload_missing();
                let plan = self.last_plan();
                Ok(HostLifecycleStatus::from_backend_state(
                    &plan,
                    HostBackendObservedState::Stopped,
                ))
            })
        }

        fn inspect<'a>(
            &'a self,
            _workload_id: TenantWorkloadId,
        ) -> HostLifecycleFuture<'a, HostLifecycleStatus> {
            Box::pin(async move {
                self.record("inspect");
                let plan = self.last_plan();
                if self.workload_missing() {
                    Err(Error::NotFound("unit not found".to_owned()))
                } else if self.calls().contains(&"start") {
                    Ok(HostLifecycleStatus::from_backend_state(
                        &plan,
                        HostBackendObservedState::Ready,
                    ))
                } else {
                    Err(Error::NotFound("unit not started yet".to_owned()))
                }
            })
        }
    }

    impl NodeBackendCapabilitySource for RecordingLifecycleBackend {
        fn node_backend_capabilities(&self) -> Vec<HostLifecycleBackendCapabilities> {
            vec![HostLifecycleBackendCapabilities::new(
                HostLifecycleBackendKind::SystemdTransientUnit,
                true,
            )]
        }
    }

    #[derive(Debug, Default, Clone)]
    struct RecordingStatusWriter {
        writes: Arc<Mutex<usize>>,
    }

    impl RecordingStatusWriter {
        fn write_count(&self) -> usize {
            *self.writes.lock().expect("writes lock")
        }
    }

    impl StatusEvidenceWriter for RecordingStatusWriter {
        fn write_status<'a>(
            &'a self,
            write: StatusEvidenceWrite<'a>,
        ) -> HostLifecycleFuture<'a, ()> {
            Box::pin(async move {
                nimbus_node::ensure_status_matches_projection(write.projection(), write.status())?;
                *self.writes.lock().expect("writes lock") += 1;
                Ok(())
            })
        }
    }

    #[tokio::test]
    async fn guest_node_workload_service_uses_node_agent_and_typed_container_runner() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let container_config = ContainerSandboxBackendConfig::plan_only(
            temp_dir.path().join("bundles"),
            temp_dir.path().join("state"),
        );
        let backend = RecordingLifecycleBackend::default();
        let writer = RecordingStatusWriter::default();
        let node_agent = NodeAgent::new(
            NodeIdentity::new("machine-os-guest-node").expect("node id"),
            backend.clone(),
            writer.clone(),
        );
        let service = GuestNodeWorkloadService::new(
            node_agent,
            Arc::new(ContainerSandboxBackend::new(container_config)),
            temp_dir.path().join("state"),
        );
        let tenant_id = TenantId::new("svc-demo").expect("tenant id");
        let spec = SandboxSpec::new(
            tenant_id,
            SandboxOwnerSpec::service("api"),
            SandboxBackendKind::Container,
            SandboxRootSpec::rootfs("/tmp/rootfs"),
            SandboxProcessSpec::new(["/bin/server"]),
        )
        .with_cpu_count(2)
        .with_memory_limit_bytes(64 * 1024 * 1024);

        let sandbox_id = SandboxId::new("service-api-01selected");
        let handle = service
            .start(sandbox_id.clone(), spec)
            .await
            .expect("start should reconcile");

        assert_eq!(handle.id, sandbox_id);
        assert_eq!(handle.status, SandboxStatus::Ready);
        let summary = service
            .state_view
            .inspect(&handle.id)
            .expect("state view should load")
            .expect("manifest should remain present")
            .summary;
        assert_eq!(
            summary.status,
            SandboxStatus::Ready,
            "machine API list/current routes should not reread a stale plan-only starting status"
        );
        assert_eq!(writer.write_count(), 1);
        assert!(
            backend
                .calls()
                .starts_with(&["validate", "inspect", "start", "inspect"]),
            "node reconciler should inspect, converge, and observe via lifecycle backend: {:?}",
            backend.calls()
        );
        let plan = backend.last_plan();
        assert_eq!(
            plan.backend(),
            HostLifecycleBackendKind::SystemdTransientUnit
        );
        assert_eq!(
            plan.executable().as_str(),
            "/usr/libexec/nimbus/nimbus-container-runner"
        );
        assert_eq!(plan.args()[0], "--bundle");
        assert!(
            plan.args()[1].contains("/sandboxes/"),
            "bundle path should point at the materialized service sandbox bundle: {:?}",
            plan.args()
        );
        assert!(plan.properties().properties().iter().any(|property| {
            matches!(
                property,
                HostLifecycleProperty::MemoryMaxBytes(bytes) if *bytes == 64 * 1024 * 1024
            )
        }));
        assert!(plan.properties().properties().iter().any(|property| {
            matches!(property, HostLifecycleProperty::CpuWeight(weight) if *weight == 200)
        }));
        assert!(plan.properties().properties().iter().any(|property| {
            matches!(property, HostLifecycleProperty::TasksMax(max) if *max == 512)
        }));
    }

    #[tokio::test]
    async fn guest_node_workload_service_projects_node_state_without_writing_container_state() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let container_config = ContainerSandboxBackendConfig::plan_only(
            temp_dir.path().join("bundles"),
            temp_dir.path().join("state"),
        );
        let backend = RecordingLifecycleBackend::default();
        let writer = RecordingStatusWriter::default();
        let node_agent = NodeAgent::new(
            NodeIdentity::new("machine-os-guest-node").expect("node id"),
            backend.clone(),
            writer,
        );
        let service = GuestNodeWorkloadService::new(
            node_agent,
            Arc::new(ContainerSandboxBackend::new(container_config)),
            temp_dir.path().join("state"),
        );
        let tenant_id = TenantId::new("svc-demo").expect("tenant id");
        let spec = SandboxSpec::new(
            tenant_id,
            SandboxOwnerSpec::service("api"),
            SandboxBackendKind::Container,
            SandboxRootSpec::rootfs("/tmp/rootfs"),
            SandboxProcessSpec::new(["/bin/server"]),
        );
        let sandbox_id = SandboxId::new("service-api-02selected");
        let handle = service
            .start(sandbox_id.clone(), spec)
            .await
            .expect("start should reconcile");

        assert_eq!(handle.id, sandbox_id);
        let initial_details = service
            .state_view
            .inspect(&handle.id)
            .expect("state view should load")
            .expect("manifest should remain present");
        let initial_manifest =
            std::fs::read(&initial_details.manifest_path).expect("manifest should be readable");
        assert_eq!(initial_details.summary.status, SandboxStatus::Ready);
        backend.mark_workload_missing();

        let inspected = service
            .inspect(&handle.id)
            .await
            .expect("missing node unit should inspect as retained")
            .expect("container manifest should remain present");
        assert_eq!(inspected.handle.status, SandboxStatus::Stopping);
        assert_eq!(
            inspected.execution,
            SandboxExecutionObservation::AbsentWithoutExit
        );
        assert_eq!(inspected.cleanup, SandboxCleanupObservation::Retained);
        let repeated_missing = service
            .inspect(&handle.id)
            .await
            .expect("repeated missing node observation should succeed")
            .expect("container manifest should remain present");
        assert_eq!(
            repeated_missing, inspected,
            "unchanged guest and container evidence must return an equal inspection and version"
        );
        let missing_details = service
            .state_view
            .inspect(&handle.id)
            .expect("state view should load")
            .expect("manifest should remain present");
        assert_eq!(missing_details.summary.status, SandboxStatus::Ready);
        assert_eq!(
            std::fs::read(&missing_details.manifest_path).expect("manifest should remain readable"),
            initial_manifest,
            "inspection must not persist the missing-node projection"
        );

        backend.mark_workload_present();
        let present = service
            .inspect(&handle.id)
            .await
            .expect("present node readiness should remain an observation")
            .expect("container manifest should remain present");
        assert_eq!(present.handle.status, SandboxStatus::Ready);
        assert_eq!(present.execution, SandboxExecutionObservation::Present);
        assert_eq!(present.cleanup, SandboxCleanupObservation::NotRequired);
        assert_ne!(
            present.version, inspected.version,
            "the comparison token must detect changed node-provider evidence"
        );
        let repeated_present = service
            .inspect(&handle.id)
            .await
            .expect("repeated present node observation should succeed")
            .expect("container manifest should remain present");
        assert_eq!(
            repeated_present, present,
            "unchanged present evidence must remain exactly repeatable"
        );
        let present_details = service
            .state_view
            .inspect(&handle.id)
            .expect("state view should reload")
            .expect("manifest should remain present");
        assert_eq!(
            std::fs::read(&present_details.manifest_path)
                .expect("manifest should remain readable after present observation"),
            initial_manifest,
            "inspection must not persist the present-node projection"
        );

        backend.mark_workload_missing();
        service
            .stop(&handle.id)
            .await
            .expect("missing node unit should stop idempotently");
        assert!(!backend.calls().contains(&"stop"));
        let finalized = service
            .inspect(&handle.id)
            .await
            .expect("finalized bundle should remain inspectable")
            .expect("finalized bundle evidence should remain present");
        assert_eq!(finalized.handle.status, SandboxStatus::Stopped);
        assert_eq!(
            finalized.cleanup,
            SandboxCleanupObservation::Finalized,
            "outer node absence must not regress exact bundle finality"
        );
        assert!(
            finalized.handle.published_endpoints.is_empty(),
            "a finalized guest workload cannot regain published endpoints"
        );
    }

    #[test]
    fn service_container_runner_request_rejects_relative_bundle_paths() {
        let error =
            service_container_runner_request(Path::new("relative/bundle"), &Default::default())
                .expect_err("relative bundle path must be rejected");

        assert_eq!(error.status, StatusCode::BAD_REQUEST);
        assert!(error.message.contains("absolute path"));
    }

    #[test]
    fn service_container_runner_request_rejects_zero_cpu_count() {
        let resources = nimbus::SandboxResourceLimits::default().with_cpu_count(0);
        let error = service_container_runner_request(Path::new("/run/nimbus/bundle"), &resources)
            .expect_err("zero cpu count must be rejected before systemd rendering");

        assert_eq!(error.status, StatusCode::BAD_REQUEST);
        assert!(
            error
                .message
                .contains("cpu_count must be greater than zero")
        );
    }
}
