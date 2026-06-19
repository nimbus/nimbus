use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;

use axum::http::StatusCode;
use nimbus::{
    Error, SandboxBackend, SandboxBackendKind, SandboxHandle, SandboxId, SandboxSpec, SandboxStatus,
};
use nimbus_sandbox::backends::container::{ContainerSandboxBackend, ContainerSandboxStateView};
use nimbus_server::local_enforcement::{
    HostLifecycleBackend, HostLifecycleBackendKind, HostLifecyclePlan, HostLifecycleRequest,
    HostLifecycleStatus, NodeAgent, NodeAgentAssignment, NodeAssignmentDisposition,
    NodeBackendCapabilitySource, RunnerSpec, StatusEvidenceWriter, TenantWorkloadPhase,
    TenantWorkloadSpec,
};

use crate::node_workload_executor::admit_workload_spec;

use super::state::container_state_error_to_http_error;
use super::{MachineApiHttpError, sandbox_error_to_http_error};

pub(super) type MachineApiServiceFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, MachineApiHttpError>> + Send + 'a>>;

pub(crate) trait MachineApiNodeWorkloadFacade: Send + Sync {
    fn kind(&self) -> SandboxBackendKind;
    fn service_execution_blockers(&self) -> Vec<String> {
        Vec::new()
    }
    fn start<'a>(&'a self, spec: SandboxSpec) -> MachineApiServiceFuture<'a, SandboxHandle>;
    fn inspect<'a>(
        &'a self,
        id: &'a SandboxId,
    ) -> MachineApiServiceFuture<'a, Option<SandboxHandle>>;
    fn stop<'a>(&'a self, id: &'a SandboxId) -> MachineApiServiceFuture<'a, ()>;
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

    fn start<'a>(&'a self, spec: SandboxSpec) -> MachineApiServiceFuture<'a, SandboxHandle> {
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
                .prepare_plan_only_service_workload(spec)
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
            .await?;
            Ok(handle_with_node_phase(prepared.handle, Some(status)))
        })
    }

    fn inspect<'a>(
        &'a self,
        id: &'a SandboxId,
    ) -> MachineApiServiceFuture<'a, Option<SandboxHandle>> {
        Box::pin(async move {
            let Some(details) = self
                .state_view
                .inspect(id)
                .map_err(container_state_error_to_http_error)?
            else {
                return Ok(None);
            };
            let bundle_dir = bundle_dir_from_manifest_path(&details.manifest_path)?;
            let status = self
                .inspect_service_workload(
                    details.summary.tenant_id.as_str(),
                    &details.summary.service_name,
                    &bundle_dir,
                    &details.resources,
                )
                .await?;
            let handle = SandboxHandle::new(
                details.summary.tenant_id,
                details.summary.sandbox_id,
                details.summary.service_name,
                SandboxBackendKind::Container,
                details.summary.status,
                details.summary.published_endpoints,
            );
            match status {
                Some(phase) => {
                    self.refresh_plan_only_manifest_status(
                        &handle.id,
                        sandbox_status_from_node_phase(phase),
                    )
                    .await?;
                }
                None => {
                    self.mark_plan_only_manifest_stopped(&handle.id).await?;
                }
            }
            Ok(Some(handle_with_node_phase(handle, status)))
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

    async fn mark_plan_only_manifest_stopped(
        &self,
        id: &SandboxId,
    ) -> Result<(), MachineApiHttpError> {
        self.bundle_materializer
            .mark_plan_only_service_workload_stopped(id)
            .map_err(sandbox_error_to_http_error)?;
        Ok(())
    }

    async fn refresh_plan_only_manifest_status(
        &self,
        id: &SandboxId,
        status: SandboxStatus,
    ) -> Result<(), MachineApiHttpError> {
        self.bundle_materializer
            .refresh_plan_only_service_workload_status(id, status)
            .map_err(sandbox_error_to_http_error)?;
        Ok(())
    }

    async fn inspect_node_status(
        &self,
        spec: TenantWorkloadSpec,
        request: HostLifecycleRequest,
    ) -> Result<HostLifecycleStatus, MachineApiHttpError> {
        let binding = nimbus_server::local_enforcement::LocalEnforcementBinding::from_spec(spec);
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
    runner
        .into_host_lifecycle_request(HostLifecycleBackendKind::SystemdTransientUnit)
        .map_err(core_error_to_http)
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

fn handle_with_node_phase(
    mut handle: SandboxHandle,
    phase: Option<TenantWorkloadPhase>,
) -> SandboxHandle {
    handle.status = phase
        .map(sandbox_status_from_node_phase)
        .unwrap_or(SandboxStatus::Stopped);
    handle
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
        Error::Conflict(_) | Error::PreconditionFailed(_) => StatusCode::CONFLICT,
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
    Arc::new(TestSandboxBackendNodeWorkloadFacade { backend })
}

#[cfg(test)]
struct TestSandboxBackendNodeWorkloadFacade {
    backend: Arc<dyn nimbus::SandboxBackend>,
}

#[cfg(test)]
impl MachineApiNodeWorkloadFacade for TestSandboxBackendNodeWorkloadFacade {
    fn kind(&self) -> SandboxBackendKind {
        self.backend.kind()
    }

    fn start<'a>(&'a self, spec: SandboxSpec) -> MachineApiServiceFuture<'a, SandboxHandle> {
        Box::pin(async move {
            self.backend
                .start(spec)
                .await
                .map_err(sandbox_error_to_http_error)
        })
    }

    fn inspect<'a>(
        &'a self,
        id: &'a SandboxId,
    ) -> MachineApiServiceFuture<'a, Option<SandboxHandle>> {
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
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use nimbus::{
        SandboxBackendKind, SandboxOwnerSpec, SandboxProcessSpec, SandboxRootSpec, TenantId,
    };
    use nimbus_sandbox::backends::container::ContainerSandboxBackendConfig;
    use nimbus_server::local_enforcement::{
        HostBackendObservedState, HostLifecycleBackendCapabilities, HostLifecycleFuture,
        HostLifecyclePlan, HostLifecycleProperty, HostLifecycleStatus, NodeIdentity,
        StatusEvidenceWrite, TenantWorkloadId, TenantWorkloadStatus,
    };

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

        fn workload_missing(&self) -> bool {
            *self.workload_missing.lock().expect("missing lock")
        }
    }

    impl HostLifecycleBackend for RecordingLifecycleBackend {
        fn validate(
            &self,
            binding: &nimbus_server::local_enforcement::LocalEnforcementBinding,
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
                write.projection().ensure_status_matches(write.status())?;
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
        .with_memory_limit_bytes(64 * 1024 * 1024);

        let handle = service.start(spec).await.expect("start should reconcile");

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
    }

    #[tokio::test]
    async fn guest_node_workload_service_marks_missing_units_stopped_in_container_state() {
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
        let handle = service.start(spec).await.expect("start should reconcile");

        backend.mark_workload_missing();

        let inspected = service
            .inspect(&handle.id)
            .await
            .expect("missing node unit should inspect as stopped")
            .expect("container manifest should remain present");
        assert_eq!(inspected.status, SandboxStatus::Stopped);
        let summary = service
            .state_view
            .inspect(&handle.id)
            .expect("state view should load")
            .expect("manifest should remain present")
            .summary;
        assert_eq!(summary.status, SandboxStatus::Stopped);

        service
            .stop(&handle.id)
            .await
            .expect("missing node unit should stop idempotently");
        assert!(!backend.calls().contains(&"stop"));
    }

    #[test]
    fn service_container_runner_request_rejects_relative_bundle_paths() {
        let error =
            service_container_runner_request(Path::new("relative/bundle"), &Default::default())
                .expect_err("relative bundle path must be rejected");

        assert_eq!(error.status, StatusCode::BAD_REQUEST);
        assert!(error.message.contains("absolute path"));
    }
}
