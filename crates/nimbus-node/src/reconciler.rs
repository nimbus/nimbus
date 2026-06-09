use std::sync::Arc;

use nimbus_core::{Error, Result};

use super::{
    HostLifecycleBackend, HostLifecycleFuture, HostLifecyclePlan, HostLifecycleRequest,
    HostLifecycleStatus, HostLifecycleStatusReason, LocalEnforcementBinding,
    TenantSystemEvidenceProjection, TenantWorkloadDeletionState, TenantWorkloadId,
    TenantWorkloadSpec, TenantWorkloadStatus,
};

pub trait StatusEvidenceWriter: Send + Sync + 'static {
    fn write_status<'a>(&'a self, write: StatusEvidenceWrite<'a>) -> HostLifecycleFuture<'a, ()>;
}

impl<T> StatusEvidenceWriter for Arc<T>
where
    T: StatusEvidenceWriter + ?Sized,
{
    fn write_status<'a>(&'a self, write: StatusEvidenceWrite<'a>) -> HostLifecycleFuture<'a, ()> {
        (**self).write_status(write)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct StatusEvidenceWrite<'a> {
    projection: &'a TenantSystemEvidenceProjection,
    status: &'a TenantWorkloadStatus,
}

impl<'a> StatusEvidenceWrite<'a> {
    pub fn new(
        projection: &'a TenantSystemEvidenceProjection,
        status: &'a TenantWorkloadStatus,
    ) -> Result<Self> {
        projection.ensure_status_matches(status)?;
        Ok(Self { projection, status })
    }

    pub fn projection(&self) -> &'a TenantSystemEvidenceProjection {
        self.projection
    }

    pub fn status(&self) -> &'a TenantWorkloadStatus {
        self.status
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeWorkloadDesiredState {
    Running,
    Stopped,
}

impl NodeWorkloadDesiredState {
    pub fn from_spec(spec: &TenantWorkloadSpec) -> Self {
        match spec.deletion() {
            TenantWorkloadDeletionState::Active => Self::Running,
            TenantWorkloadDeletionState::Deleting { .. } => Self::Stopped,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeWorkloadReconcileAction {
    ObservedRunning,
    Started,
    ObservedStopped,
    Stopped,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeWorkloadReconcileOutcome {
    workload_id: TenantWorkloadId,
    desired_state: NodeWorkloadDesiredState,
    action: NodeWorkloadReconcileAction,
    status: TenantWorkloadStatus,
}

impl NodeWorkloadReconcileOutcome {
    fn new(
        workload_id: TenantWorkloadId,
        desired_state: NodeWorkloadDesiredState,
        action: NodeWorkloadReconcileAction,
        status: TenantWorkloadStatus,
    ) -> Self {
        Self {
            workload_id,
            desired_state,
            action,
            status,
        }
    }

    pub fn workload_id(&self) -> &TenantWorkloadId {
        &self.workload_id
    }

    pub fn desired_state(&self) -> NodeWorkloadDesiredState {
        self.desired_state
    }

    pub fn action(&self) -> NodeWorkloadReconcileAction {
        self.action
    }

    pub fn status(&self) -> &TenantWorkloadStatus {
        &self.status
    }
}

#[derive(Debug)]
pub struct NodeWorkloadReconciler<B, W> {
    backend: B,
    writer: W,
}

impl<B, W> NodeWorkloadReconciler<B, W> {
    pub fn new(backend: B, writer: W) -> Self {
        Self { backend, writer }
    }

    pub fn backend(&self) -> &B {
        &self.backend
    }

    pub fn writer(&self) -> &W {
        &self.writer
    }
}

impl<B, W> NodeWorkloadReconciler<B, W>
where
    B: HostLifecycleBackend,
    W: StatusEvidenceWriter,
{
    pub async fn reconcile_spec(
        &self,
        spec: TenantWorkloadSpec,
        request: HostLifecycleRequest,
    ) -> Result<NodeWorkloadReconcileOutcome> {
        let binding = LocalEnforcementBinding::from_spec(spec);
        self.reconcile_binding(&binding, request).await
    }

    pub async fn reconcile_binding(
        &self,
        binding: &LocalEnforcementBinding,
        request: HostLifecycleRequest,
    ) -> Result<NodeWorkloadReconcileOutcome> {
        let desired_state = NodeWorkloadDesiredState::from_spec(binding.spec());
        let plan = self.backend.validate(binding, request)?;
        let workload_id = plan.workload_id().clone();
        let (action, status) = match desired_state {
            NodeWorkloadDesiredState::Running => self.reconcile_running(&plan).await?,
            NodeWorkloadDesiredState::Stopped => self.reconcile_stopped(&plan).await?,
        };
        let projection = binding.system_evidence_projection();
        let write = StatusEvidenceWrite::new(&projection, &status)?;
        self.writer.write_status(write).await?;
        Ok(NodeWorkloadReconcileOutcome::new(
            workload_id,
            desired_state,
            action,
            status,
        ))
    }

    async fn reconcile_running(
        &self,
        plan: &HostLifecyclePlan,
    ) -> Result<(NodeWorkloadReconcileAction, TenantWorkloadStatus)> {
        let workload_id = plan.workload_id().clone();
        match self.backend.inspect(workload_id.clone()).await {
            Ok(status) if is_running_enough(&status) => Ok((
                NodeWorkloadReconcileAction::ObservedRunning,
                status.to_workload_status(plan)?,
            )),
            Ok(_) | Err(Error::InvalidInput(_)) => {
                self.backend.start(plan.clone()).await?;
                let observed = self.backend.inspect(workload_id).await?;
                Ok((
                    NodeWorkloadReconcileAction::Started,
                    observed.to_workload_status(plan)?,
                ))
            }
            Err(error) => Err(error),
        }
    }

    async fn reconcile_stopped(
        &self,
        plan: &HostLifecyclePlan,
    ) -> Result<(NodeWorkloadReconcileAction, TenantWorkloadStatus)> {
        let workload_id = plan.workload_id().clone();
        let inspected = self.backend.inspect(workload_id.clone()).await?;
        if is_stopped(&inspected) {
            return Ok((
                NodeWorkloadReconcileAction::ObservedStopped,
                inspected.to_workload_status(plan)?,
            ));
        }
        self.backend.stop(workload_id.clone()).await?;
        let observed = self.backend.inspect(workload_id).await?;
        Ok((
            NodeWorkloadReconcileAction::Stopped,
            observed.to_workload_status(plan)?,
        ))
    }
}

fn is_running_enough(status: &HostLifecycleStatus) -> bool {
    matches!(
        status.reason(),
        HostLifecycleStatusReason::Submitted
            | HostLifecycleStatusReason::Running
            | HostLifecycleStatusReason::Ready
    )
}

fn is_stopped(status: &HostLifecycleStatus) -> bool {
    matches!(status.reason(), HostLifecycleStatusReason::Stopped)
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use nimbus_core::{PrincipalContext, TenantId};
    use nimbus_runtime::{RuntimeLimits, RuntimePolicy};

    use super::*;
    use crate::{
        DirectProcessBackend, HostExecutable, HostLifecycleBackendKind, HostLifecycleProperty,
        HostLifecyclePropertySet, HostRestartPolicy, StartTransientMode, SystemdDbusClient,
        SystemdDbusProperty, SystemdInspectUnitRequest, SystemdStartTransientUnitRequest,
        SystemdStartTransientUnitResponse, SystemdStopUnitRequest, SystemdStopUnitResponse,
        SystemdTransientCapabilities, SystemdTransientUnitBackend, SystemdUnitStatus,
        TenantFinalizerRecord, TenantWorkloadPhase, TenantWorkloadStatusPatchTarget,
    };
    use nimbus_tenant::{
        RuntimeIsolationTier, TenantIsolationContext, TenantIsolationDecision, TenantIsolationMode,
        TenantIsolationPolicyInput, TenantServiceGrantPolicyDecision, TenantStoragePolicyDecision,
        WorkloadAttributes, WorkloadLocation,
    };

    #[derive(Clone)]
    struct CountingBackend<B> {
        inner: B,
        calls: Arc<Mutex<Vec<&'static str>>>,
    }

    impl<B> CountingBackend<B> {
        fn new(inner: B) -> Self {
            Self {
                inner,
                calls: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn calls(&self) -> Vec<&'static str> {
            self.calls
                .lock()
                .expect("counting backend lock should not be poisoned")
                .clone()
        }

        fn record(&self, call: &'static str) {
            self.calls
                .lock()
                .expect("counting backend lock should not be poisoned")
                .push(call);
        }
    }

    impl<B> HostLifecycleBackend for CountingBackend<B>
    where
        B: HostLifecycleBackend,
    {
        fn validate(
            &self,
            binding: &LocalEnforcementBinding,
            request: HostLifecycleRequest,
        ) -> Result<HostLifecyclePlan> {
            self.record("validate");
            self.inner.validate(binding, request)
        }

        fn start<'a>(
            &'a self,
            plan: HostLifecyclePlan,
        ) -> HostLifecycleFuture<'a, TenantWorkloadStatus> {
            self.record("start");
            self.inner.start(plan)
        }

        fn stop<'a>(
            &'a self,
            workload_id: TenantWorkloadId,
        ) -> HostLifecycleFuture<'a, HostLifecycleStatus> {
            self.record("stop");
            self.inner.stop(workload_id)
        }

        fn inspect<'a>(
            &'a self,
            workload_id: TenantWorkloadId,
        ) -> HostLifecycleFuture<'a, HostLifecycleStatus> {
            self.record("inspect");
            self.inner.inspect(workload_id)
        }
    }

    #[derive(Debug, Clone)]
    struct RecordedStatusWrite {
        tenant_id: String,
        workload_uid: String,
        status: TenantWorkloadStatus,
    }

    #[derive(Debug, Default, Clone)]
    struct RecordingStatusEvidenceWriter {
        writes: Arc<Mutex<Vec<RecordedStatusWrite>>>,
    }

    impl RecordingStatusEvidenceWriter {
        fn writes(&self) -> Vec<RecordedStatusWrite> {
            self.writes
                .lock()
                .expect("recording writer lock should not be poisoned")
                .clone()
        }
    }

    impl StatusEvidenceWriter for RecordingStatusEvidenceWriter {
        fn write_status<'a>(
            &'a self,
            write: StatusEvidenceWrite<'a>,
        ) -> HostLifecycleFuture<'a, ()> {
            Box::pin(async move {
                write.projection().ensure_status_matches(write.status())?;
                self.writes
                    .lock()
                    .expect("recording writer lock should not be poisoned")
                    .push(RecordedStatusWrite {
                        tenant_id: write.projection().tenant_id().as_str().to_string(),
                        workload_uid: write.projection().workload_uid().as_str().to_string(),
                        status: write.status().clone(),
                    });
                Ok(())
            })
        }
    }

    #[derive(Clone)]
    struct ReconcilerSystemdClient {
        capabilities: SystemdTransientCapabilities,
        calls: Arc<Mutex<Vec<&'static str>>>,
        last_start: Arc<Mutex<Option<SystemdStartTransientUnitRequest>>>,
        status: Arc<Mutex<Option<SystemdUnitStatus>>>,
    }

    impl ReconcilerSystemdClient {
        fn available() -> Self {
            Self {
                capabilities: SystemdTransientCapabilities::available(),
                calls: Arc::new(Mutex::new(Vec::new())),
                last_start: Arc::new(Mutex::new(None)),
                status: Arc::new(Mutex::new(None)),
            }
        }

        fn last_start(&self) -> SystemdStartTransientUnitRequest {
            self.last_start
                .lock()
                .expect("fake systemd client lock should not be poisoned")
                .clone()
                .expect("systemd start should have been called")
        }

        fn calls(&self) -> Vec<&'static str> {
            self.calls
                .lock()
                .expect("fake systemd client lock should not be poisoned")
                .clone()
        }

        fn record(&self, call: &'static str) {
            self.calls
                .lock()
                .expect("fake systemd client lock should not be poisoned")
                .push(call);
        }
    }

    impl SystemdDbusClient for ReconcilerSystemdClient {
        fn capabilities(&self) -> SystemdTransientCapabilities {
            self.capabilities.clone()
        }

        fn start_transient_unit<'a>(
            &'a self,
            request: SystemdStartTransientUnitRequest,
        ) -> HostLifecycleFuture<'a, SystemdStartTransientUnitResponse> {
            Box::pin(async move {
                self.record("start_transient_unit");
                let response = SystemdStartTransientUnitResponse::new(
                    request.unit_name().clone(),
                    "/org/freedesktop/systemd1/job/101",
                )?;
                let status = SystemdUnitStatus::new(
                    request.workload_id().clone(),
                    request.unit_name().clone(),
                    "active",
                    "running",
                )?
                .with_job_path(response.job_path())?
                .with_main_pid(4101);
                *self
                    .last_start
                    .lock()
                    .expect("fake systemd client lock should not be poisoned") = Some(request);
                *self
                    .status
                    .lock()
                    .expect("fake systemd client lock should not be poisoned") = Some(status);
                Ok(response)
            })
        }

        fn stop_unit<'a>(
            &'a self,
            request: SystemdStopUnitRequest,
        ) -> HostLifecycleFuture<'a, SystemdStopUnitResponse> {
            Box::pin(async move {
                self.record("stop_unit");
                let status = SystemdUnitStatus::new(
                    request.workload_id().clone(),
                    request.unit_name().clone(),
                    "inactive",
                    "dead",
                )?;
                *self
                    .status
                    .lock()
                    .expect("fake systemd client lock should not be poisoned") =
                    Some(status.clone());
                SystemdStopUnitResponse::new("/org/freedesktop/systemd1/job/102", status)
            })
        }

        fn inspect_unit<'a>(
            &'a self,
            request: SystemdInspectUnitRequest,
        ) -> HostLifecycleFuture<'a, SystemdUnitStatus> {
            Box::pin(async move {
                self.record("inspect_unit");
                Ok(self
                    .status
                    .lock()
                    .expect("fake systemd client lock should not be poisoned")
                    .clone()
                    .unwrap_or_else(|| {
                        SystemdUnitStatus::new(
                            request.workload_id().clone(),
                            request.unit_name().clone(),
                            "inactive",
                            "dead",
                        )
                        .expect("inactive status should build")
                    }))
            })
        }
    }

    fn admitted_decision(
        workload_name: &str,
        invocation_id: &str,
        generation: u64,
    ) -> TenantIsolationDecision {
        let context = TenantIsolationContext::application(
            TenantId::new("tenant-a").expect("tenant id should parse"),
            PrincipalContext {
                authenticated: true,
                claims: serde_json::Map::from_iter([(
                    "tenant_id".to_string(),
                    serde_json::Value::String("tenant-a".to_string()),
                )]),
                verified_claims: serde_json::Map::new(),
            },
            "node.reconciler",
        )
        .with_deployment_generation(generation)
        .with_workload_location(WorkloadLocation::new().with_node_id("node-a"));
        let policy = RuntimePolicy::new(RuntimeLimits::application_web_standard());
        let workload = WorkloadAttributes::runtime_function(
            workload_name,
            RuntimeIsolationTier::InProcessUntrusted,
        )
        .with_invocation_id(invocation_id);
        let input = TenantIsolationPolicyInput::new(workload)
            .with_runtime_policy(
                &context,
                &policy,
                RuntimeIsolationTier::InProcessUntrusted,
                TenantIsolationMode::Production,
            )
            .with_services(TenantServiceGrantPolicyDecision::new(["db"]))
            .with_storage(TenantStoragePolicyDecision::namespace("tenant-a"));

        context
            .admit_decision(input)
            .expect("decision should admit matching tenant authority")
    }

    fn binding() -> LocalEnforcementBinding {
        LocalEnforcementBinding::from_decision(&admitted_decision("messages:send", "invoke-1", 7))
            .expect("binding should materialize")
    }

    fn direct_request() -> HostLifecycleRequest {
        HostLifecycleRequest::new(
            HostLifecycleBackendKind::DirectProcess,
            HostExecutable::trusted("/usr/libexec/nimbus/direct-workload")
                .expect("trusted executable should parse"),
        )
        .with_args(["--mode", "node-reconcile"])
        .expect("args should parse")
        .with_properties(HostLifecyclePropertySet::new([
            HostLifecycleProperty::Description("Nimbus direct reconciler workload".to_string()),
            HostLifecycleProperty::Restart(HostRestartPolicy::No),
        ]))
    }

    fn systemd_request() -> HostLifecycleRequest {
        HostLifecycleRequest::new(
            HostLifecycleBackendKind::SystemdTransientUnit,
            HostExecutable::trusted("/usr/libexec/nimbus/conmon-crun-launcher")
                .expect("trusted executable should parse"),
        )
        .with_args(["--bundle", "/run/nimbus/bundles/workload"])
        .expect("args should parse")
        .with_properties(
            HostLifecyclePropertySet::from_raw_systemd_properties([
                ("Description", "Nimbus reconciled workload"),
                ("Restart", "on-failure"),
                ("RestartSec", "3"),
                ("MemoryMax", "536870912"),
                ("CPUWeight", "100"),
                ("TasksMax", "128"),
            ])
            .expect("allowlisted systemd properties should parse"),
        )
    }

    #[tokio::test]
    async fn direct_process_reconciler_starts_stops_and_writes_observed_status() {
        let backend = CountingBackend::new(DirectProcessBackend::new());
        let writer = RecordingStatusEvidenceWriter::default();
        let reconciler = NodeWorkloadReconciler::new(backend.clone(), writer.clone());
        let binding = binding();
        let original_spec = binding.spec().clone();

        let started = reconciler
            .reconcile_binding(&binding, direct_request())
            .await
            .expect("direct-process reconciler should start missing workload");
        assert_eq!(started.desired_state(), NodeWorkloadDesiredState::Running);
        assert_eq!(started.action(), NodeWorkloadReconcileAction::Started);
        assert_eq!(started.status().phase(), TenantWorkloadPhase::Running);
        assert_eq!(
            backend.calls(),
            vec!["validate", "inspect", "start", "inspect"]
        );
        assert_eq!(
            binding.spec(),
            &original_spec,
            "reconcile must not mutate spec"
        );

        let writes = writer.writes();
        assert_eq!(writes.len(), 1);
        assert_eq!(writes[0].tenant_id, "tenant-a");
        assert_eq!(
            writes[0].workload_uid,
            binding.spec().workload_uid().as_str()
        );
        assert_eq!(
            writes[0].status.target(),
            TenantWorkloadStatusPatchTarget::Status
        );
        assert_eq!(
            writes[0].status.observed_generation(),
            binding.spec().generation()
        );

        let deleting = LocalEnforcementBinding::from_spec(
            binding
                .spec()
                .clone()
                .mark_deleting_server_owned([TenantFinalizerRecord::new(
                    "local_enforcement",
                    "host-lifecycle-stop",
                )
                .expect("finalizer should parse")]),
        );
        let stopped = reconciler
            .reconcile_binding(&deleting, direct_request())
            .await
            .expect("direct-process reconciler should stop deleting workload");
        assert_eq!(stopped.desired_state(), NodeWorkloadDesiredState::Stopped);
        assert_eq!(stopped.action(), NodeWorkloadReconcileAction::Stopped);
        assert_eq!(stopped.status().phase(), TenantWorkloadPhase::Deleting);
        assert_eq!(
            backend.calls(),
            vec![
                "validate", "inspect", "start", "inspect", "validate", "inspect", "stop",
                "inspect",
            ]
        );
        assert_eq!(writer.writes().len(), 2);
        assert_eq!(
            deleting.spec().resources().admitted_quotas(),
            binding.spec().resources().admitted_quotas(),
            "observed reconcile writes must not mutate admitted quota policy"
        );
    }

    #[tokio::test]
    async fn systemd_reconciler_uses_transient_units_and_trusted_execstart() {
        let client = ReconcilerSystemdClient::available();
        let backend = CountingBackend::new(SystemdTransientUnitBackend::new(client.clone()));
        let writer = RecordingStatusEvidenceWriter::default();
        let reconciler = NodeWorkloadReconciler::new(backend.clone(), writer.clone());
        let binding = binding();

        let started = reconciler
            .reconcile_spec(binding.spec().clone(), systemd_request())
            .await
            .expect("systemd reconciler should start inactive workload");
        assert_eq!(started.action(), NodeWorkloadReconcileAction::Started);
        assert_eq!(started.status().phase(), TenantWorkloadPhase::Running);
        assert_eq!(
            backend.calls(),
            vec!["validate", "inspect", "start", "inspect"]
        );
        assert_eq!(
            client.calls(),
            vec!["inspect_unit", "start_transient_unit", "inspect_unit"]
        );

        let start = client.last_start();
        assert_eq!(start.mode(), StartTransientMode::Replace);
        assert!(start.cgroup_path().contains(start.unit_name().as_str()));
        assert!(start.journal_selectors().iter().any(|selector| {
            selector.field() == "_SYSTEMD_UNIT" && selector.value() == start.unit_name().as_str()
        }));
        let exec = start
            .properties()
            .iter()
            .find_map(|property| match property {
                SystemdDbusProperty::ExecStart(exec) => Some(exec),
                _ => None,
            })
            .expect("systemd request should generate Nimbus-owned ExecStart");
        assert_eq!(
            exec.executable(),
            "/usr/libexec/nimbus/conmon-crun-launcher"
        );
        assert_eq!(
            exec.args(),
            &[
                "--bundle".to_string(),
                "/run/nimbus/bundles/workload".to_string()
            ]
        );
        assert!(
            start
                .properties()
                .iter()
                .all(|property| property.name() != "EnvironmentFile"),
            "raw systemd escape hatches must not pass through reconciler wiring"
        );

        let deleting =
            binding
                .spec()
                .clone()
                .mark_deleting_server_owned([TenantFinalizerRecord::new(
                    "local_enforcement",
                    "systemd-stop",
                )
                .expect("finalizer should parse")]);
        let stopped = reconciler
            .reconcile_spec(deleting, systemd_request())
            .await
            .expect("systemd reconciler should stop deleting workload");
        assert_eq!(stopped.action(), NodeWorkloadReconcileAction::Stopped);
        assert_eq!(stopped.status().phase(), TenantWorkloadPhase::Deleting);
        assert!(
            stopped
                .status()
                .lifecycle_evidence()
                .expect("systemd status should carry lifecycle evidence")
                .cgroup_path()
                .expect("systemd evidence should include cgroup")
                .contains("nimbus-tw_")
        );
        assert_eq!(writer.writes().len(), 2);
    }

    #[tokio::test]
    async fn reconciler_fails_closed_before_write_when_systemd_is_unavailable() {
        let backend = SystemdTransientUnitBackend::unavailable("not linux");
        let writer = RecordingStatusEvidenceWriter::default();
        let reconciler = NodeWorkloadReconciler::new(backend, writer.clone());
        let binding = binding();

        let error = reconciler
            .reconcile_binding(&binding, systemd_request())
            .await
            .expect_err("unavailable systemd backend should fail closed");
        assert!(
            error.to_string().contains("D-Bus is unavailable"),
            "error should explain unavailable backend: {error}"
        );
        assert!(
            writer.writes().is_empty(),
            "failed validation must not produce observed _nimbus status evidence"
        );
    }

    #[test]
    fn status_evidence_write_rejects_mismatched_generation_before_persistence() {
        let binding = binding();
        let spec = binding.spec();
        let projection = binding.system_evidence_projection();
        let stale_status = crate::NodeStatusAuthorizer
            .authorize(
                spec,
                crate::TenantWorkloadStatusPatch::observed_status(spec).with_observed_generation(
                    crate::TenantWorkloadGeneration::new(spec.generation().as_u64() - 1),
                ),
            )
            .expect_err("stale generation should fail before status write");
        assert!(
            stale_status.to_string().contains("referenced generation"),
            "stale generation error should name generation mismatch: {stale_status}"
        );

        let live_status = crate::NodeStatusAuthorizer
            .authorize(
                spec,
                crate::TenantWorkloadStatusPatch::observed_status(spec)
                    .with_phase(TenantWorkloadPhase::Running),
            )
            .expect("matching status should authorize");
        StatusEvidenceWrite::new(&projection, &live_status)
            .expect("matching projection/status should build a narrow write request");
    }

    #[test]
    fn desired_state_is_derived_from_server_owned_deletion_state() {
        let binding = binding();
        assert_eq!(
            NodeWorkloadDesiredState::from_spec(binding.spec()),
            NodeWorkloadDesiredState::Running
        );
        let deleting =
            binding
                .spec()
                .clone()
                .mark_deleting_server_owned([TenantFinalizerRecord::new(
                    "local_enforcement",
                    "desired-stop",
                )
                .expect("finalizer should parse")]);
        assert_eq!(
            NodeWorkloadDesiredState::from_spec(&deleting),
            NodeWorkloadDesiredState::Stopped
        );
    }
}
