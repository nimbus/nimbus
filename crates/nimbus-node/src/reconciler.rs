use std::sync::Arc;

use nimbus_core::{Error, PrincipalContext, Result};

use super::{
    DirectProcessBackend, HostBackendObservedState, HostLifecycleBackend,
    HostLifecycleBackendCapabilities, HostLifecycleFuture, HostLifecyclePlan, HostLifecycleRequest,
    HostLifecycleStatus, HostLifecycleStatusReason, LocalEnforcementBinding, NodeIdentity,
    SystemdDbusClient, SystemdTransientUnitBackend, TenantSystemEvidenceProjection,
    TenantWorkloadDeletionState, TenantWorkloadSpec, TenantWorkloadStatus, WorkloadExecutionId,
    ensure_status_matches_projection,
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
        ensure_status_matches_projection(projection, status)?;
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
    ObservedStopped,
    Stopped,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeWorkloadReconcileOutcome {
    execution_id: WorkloadExecutionId,
    desired_state: NodeWorkloadDesiredState,
    action: NodeWorkloadReconcileAction,
    status: TenantWorkloadStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeAgentAssignment {
    binding: LocalEnforcementBinding,
    request: HostLifecycleRequest,
}

impl NodeAgentAssignment {
    pub fn new(binding: LocalEnforcementBinding, request: HostLifecycleRequest) -> Self {
        Self { binding, request }
    }

    pub fn from_spec(spec: TenantWorkloadSpec, request: HostLifecycleRequest) -> Self {
        Self::new(LocalEnforcementBinding::from_spec(spec), request)
    }

    pub fn binding(&self) -> &LocalEnforcementBinding {
        &self.binding
    }

    pub fn request(&self) -> &HostLifecycleRequest {
        &self.request
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeAssignmentDisposition {
    Reconciled {
        execution_id: WorkloadExecutionId,
        action: NodeWorkloadReconcileAction,
    },
    Failed {
        workload_uid: String,
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeAgentReconcileReport {
    node_id: NodeIdentity,
    outcomes: Vec<NodeWorkloadReconcileOutcome>,
    dispositions: Vec<NodeAssignmentDisposition>,
}

impl NodeAgentReconcileReport {
    fn new(
        node_id: NodeIdentity,
        outcomes: Vec<NodeWorkloadReconcileOutcome>,
        dispositions: Vec<NodeAssignmentDisposition>,
    ) -> Self {
        Self {
            node_id,
            outcomes,
            dispositions,
        }
    }

    pub fn node_id(&self) -> &NodeIdentity {
        &self.node_id
    }

    pub fn outcomes(&self) -> &[NodeWorkloadReconcileOutcome] {
        &self.outcomes
    }

    pub fn dispositions(&self) -> &[NodeAssignmentDisposition] {
        &self.dispositions
    }
}

pub trait NodeBackendCapabilitySource: Send + Sync + 'static {
    fn node_backend_capabilities(&self) -> Vec<HostLifecycleBackendCapabilities>;
}

pub trait NodeWorkloadReconcileCapability: Send + Sync + 'static {
    fn backend_capabilities(&self) -> Vec<HostLifecycleBackendCapabilities>;

    fn reconcile_assignment<'a>(
        &'a self,
        assignment: NodeAgentAssignment,
    ) -> HostLifecycleFuture<'a, NodeWorkloadReconcileOutcome>;

    fn reconcile_assignments<'a>(
        &'a self,
        assignments: Vec<NodeAgentAssignment>,
    ) -> HostLifecycleFuture<'a, NodeAgentReconcileReport>;

    fn inspect_assignment<'a>(
        &'a self,
        assignment: &'a NodeAgentAssignment,
    ) -> HostLifecycleFuture<'a, HostLifecycleStatus>;
}

impl NodeBackendCapabilitySource for DirectProcessBackend {
    fn node_backend_capabilities(&self) -> Vec<HostLifecycleBackendCapabilities> {
        vec![HostLifecycleBackendCapabilities::direct_process()]
    }
}

impl<C> NodeBackendCapabilitySource for SystemdTransientUnitBackend<C>
where
    C: SystemdDbusClient,
{
    fn node_backend_capabilities(&self) -> Vec<HostLifecycleBackendCapabilities> {
        vec![self.backend_capabilities()]
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeAgentCapabilityReport {
    node_id: NodeIdentity,
    backend_capabilities: Vec<HostLifecycleBackendCapabilities>,
}

impl NodeAgentCapabilityReport {
    fn new(
        node_id: NodeIdentity,
        backend_capabilities: Vec<HostLifecycleBackendCapabilities>,
    ) -> Self {
        Self {
            node_id,
            backend_capabilities,
        }
    }

    pub fn node_id(&self) -> &NodeIdentity {
        &self.node_id
    }

    pub fn backend_capabilities(&self) -> &[HostLifecycleBackendCapabilities] {
        &self.backend_capabilities
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeAgentTransportAdmission {
    node_id: NodeIdentity,
    principal: PrincipalContext,
}

impl NodeAgentTransportAdmission {
    pub fn authorize(node_id: NodeIdentity, principal: PrincipalContext) -> Result<Self> {
        if !principal.authenticated {
            return Err(Error::PermissionDenied(
                "node-agent transport requires an authenticated principal".to_owned(),
            ));
        }
        let Some(principal_node_id) = principal_string_claim(
            &principal,
            &["node_id", "nodeId", "nimbus_node_id", "nimbusNodeId"],
        ) else {
            return Err(Error::PermissionDenied(format!(
                "node-agent transport principal is missing node id claim for `{}`",
                node_id.as_str()
            )));
        };
        if principal_node_id != node_id.as_str() {
            return Err(Error::PermissionDenied(format!(
                "node-agent transport principal authorized node `{principal_node_id}`, but agent is `{}`",
                node_id.as_str()
            )));
        }
        Ok(Self { node_id, principal })
    }

    pub fn node_id(&self) -> &NodeIdentity {
        &self.node_id
    }

    pub fn principal(&self) -> &PrincipalContext {
        &self.principal
    }
}

#[derive(Debug)]
pub struct NodeAgent<B, W> {
    node_id: NodeIdentity,
    reconciler: NodeWorkloadReconciler<B, W>,
}

impl<B, W> NodeAgent<B, W> {
    pub fn new(node_id: NodeIdentity, backend: B, writer: W) -> Self {
        Self {
            node_id,
            reconciler: NodeWorkloadReconciler::new(backend, writer),
        }
    }

    pub fn node_id(&self) -> &NodeIdentity {
        &self.node_id
    }

    pub fn reconciler(&self) -> &NodeWorkloadReconciler<B, W> {
        &self.reconciler
    }

    pub fn authorize_transport(
        &self,
        principal: PrincipalContext,
    ) -> Result<NodeAgentTransportAdmission> {
        NodeAgentTransportAdmission::authorize(self.node_id.clone(), principal)
    }
}

impl<B, W> NodeAgent<B, W>
where
    B: NodeBackendCapabilitySource,
{
    pub fn capability_report(&self) -> NodeAgentCapabilityReport {
        NodeAgentCapabilityReport::new(
            self.node_id.clone(),
            self.reconciler.backend().node_backend_capabilities(),
        )
    }
}

fn principal_string_claim<'a>(principal: &'a PrincipalContext, names: &[&str]) -> Option<&'a str> {
    for claims in [&principal.verified_claims, &principal.claims] {
        for name in names {
            if let Some(value) = claims.get(*name).and_then(|value| value.as_str()) {
                return Some(value);
            }
        }
    }
    None
}

impl<B, W> NodeAgent<B, W>
where
    B: HostLifecycleBackend,
    W: StatusEvidenceWriter,
{
    pub async fn reconcile_assignment(
        &self,
        assignment: NodeAgentAssignment,
    ) -> Result<NodeWorkloadReconcileOutcome> {
        assignment
            .binding()
            .spec()
            .ensure_assigned_node_matches(&self.node_id, "node workload reconciliation")?;
        let NodeAgentAssignment { binding, request } = assignment;
        self.reconciler.reconcile_binding(&binding, request).await
    }

    pub async fn reconcile_assignments(
        &self,
        assignments: impl IntoIterator<Item = NodeAgentAssignment>,
    ) -> NodeAgentReconcileReport {
        let mut outcomes = Vec::new();
        let mut dispositions = Vec::new();
        for assignment in assignments {
            let workload_uid = assignment.binding.spec().workload_uid().as_str().to_owned();
            match self.reconcile_assignment(assignment).await {
                Ok(outcome) => {
                    dispositions.push(NodeAssignmentDisposition::Reconciled {
                        execution_id: outcome.execution_id().clone(),
                        action: outcome.action(),
                    });
                    outcomes.push(outcome);
                }
                Err(error) => {
                    dispositions.push(NodeAssignmentDisposition::Failed {
                        workload_uid,
                        reason: error.to_string(),
                    });
                }
            }
        }
        NodeAgentReconcileReport::new(self.node_id.clone(), outcomes, dispositions)
    }

    pub async fn inspect_assignment(
        &self,
        assignment: &NodeAgentAssignment,
    ) -> Result<HostLifecycleStatus> {
        assignment
            .binding()
            .spec()
            .ensure_assigned_node_matches(&self.node_id, "node workload inspection")?;
        self.reconciler
            .inspect_binding(assignment.binding(), assignment.request().clone())
            .await
    }
}

impl<B, W> NodeWorkloadReconcileCapability for NodeAgent<B, W>
where
    B: HostLifecycleBackend + NodeBackendCapabilitySource,
    W: StatusEvidenceWriter,
{
    fn backend_capabilities(&self) -> Vec<HostLifecycleBackendCapabilities> {
        self.capability_report().backend_capabilities().to_vec()
    }

    fn reconcile_assignment<'a>(
        &'a self,
        assignment: NodeAgentAssignment,
    ) -> HostLifecycleFuture<'a, NodeWorkloadReconcileOutcome> {
        Box::pin(async move { NodeAgent::reconcile_assignment(self, assignment).await })
    }

    fn reconcile_assignments<'a>(
        &'a self,
        assignments: Vec<NodeAgentAssignment>,
    ) -> HostLifecycleFuture<'a, NodeAgentReconcileReport> {
        Box::pin(async move { Ok(NodeAgent::reconcile_assignments(self, assignments).await) })
    }

    fn inspect_assignment<'a>(
        &'a self,
        assignment: &'a NodeAgentAssignment,
    ) -> HostLifecycleFuture<'a, HostLifecycleStatus> {
        Box::pin(async move { NodeAgent::inspect_assignment(self, assignment).await })
    }
}

impl NodeWorkloadReconcileOutcome {
    fn new(
        execution_id: WorkloadExecutionId,
        desired_state: NodeWorkloadDesiredState,
        action: NodeWorkloadReconcileAction,
        status: TenantWorkloadStatus,
    ) -> Self {
        Self {
            execution_id,
            desired_state,
            action,
            status,
        }
    }

    pub fn execution_id(&self) -> &WorkloadExecutionId {
        &self.execution_id
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
        request.ensure_external_restart_disabled()?;
        let desired_state = NodeWorkloadDesiredState::from_spec(binding.spec());
        let plan = self.backend.validate(binding, request)?;
        let execution_id = plan.execution_id().clone();
        let (action, status) = match desired_state {
            NodeWorkloadDesiredState::Running => {
                let observed = self.backend.inspect_exact(plan.clone()).await?;
                if !is_running_enough(&observed) {
                    return Err(Error::InvalidInput(format!(
                        "node observed workload {} in {:?}, but compute has not authorized a provider activation",
                        plan.execution_id().as_str(),
                        observed.reason(),
                    )));
                }
                (
                    NodeWorkloadReconcileAction::ObservedRunning,
                    observed.to_workload_status(&plan)?,
                )
            }
            NodeWorkloadDesiredState::Stopped => self.reconcile_stopped(&plan).await?,
        };
        let projection = binding.system_evidence_projection();
        let write = StatusEvidenceWrite::new(&projection, &status)?;
        self.writer.write_status(write).await?;
        Ok(NodeWorkloadReconcileOutcome::new(
            execution_id,
            desired_state,
            action,
            status,
        ))
    }

    pub async fn inspect_binding(
        &self,
        binding: &LocalEnforcementBinding,
        request: HostLifecycleRequest,
    ) -> Result<HostLifecycleStatus> {
        request.ensure_external_restart_disabled()?;
        let plan = self.backend.validate(binding, request)?;
        self.backend.inspect(plan.execution_id().clone()).await
    }

    async fn reconcile_stopped(
        &self,
        plan: &HostLifecyclePlan,
    ) -> Result<(NodeWorkloadReconcileAction, TenantWorkloadStatus)> {
        let execution_id = plan.execution_id().clone();
        let inspected = match self.backend.inspect(execution_id.clone()).await {
            Ok(status) => status,
            Err(Error::NotFound(_)) => {
                let observed = HostLifecycleStatus::from_backend_state(
                    plan,
                    HostBackendObservedState::Stopped,
                );
                return Ok((
                    NodeWorkloadReconcileAction::ObservedStopped,
                    observed.to_workload_status(plan)?,
                ));
            }
            Err(error) => return Err(error),
        };
        if is_stopped(&inspected) {
            return Ok((
                NodeWorkloadReconcileAction::ObservedStopped,
                inspected.to_workload_status(plan)?,
            ));
        }
        self.backend.stop(execution_id.clone()).await?;
        let observed = self.backend.inspect(execution_id).await?;
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
    use crate::host_lifecycle::test_support::activation_command_for_plan;
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

        fn stop<'a>(
            &'a self,
            execution_id: WorkloadExecutionId,
        ) -> HostLifecycleFuture<'a, HostLifecycleStatus> {
            self.record("stop");
            self.inner.stop(execution_id)
        }

        fn inspect<'a>(
            &'a self,
            execution_id: WorkloadExecutionId,
        ) -> HostLifecycleFuture<'a, HostLifecycleStatus> {
            self.record("inspect");
            self.inner.inspect(execution_id)
        }
    }

    impl<B> NodeBackendCapabilitySource for CountingBackend<B>
    where
        B: NodeBackendCapabilitySource,
    {
        fn node_backend_capabilities(&self) -> Vec<HostLifecycleBackendCapabilities> {
            self.inner.node_backend_capabilities()
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
                ensure_status_matches_projection(write.projection(), write.status())?;
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
                    request.execution_id().clone(),
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
                    request.execution_id().clone(),
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
                        SystemdUnitStatus::explicitly_absent(
                            request.execution_id().clone(),
                            request.unit_name().clone(),
                        )
                        .expect("absent status should build")
                    }))
            })
        }
    }

    fn admitted_decision_for_location(
        workload_name: &str,
        invocation_id: &str,
        generation: u64,
        workload_location: WorkloadLocation,
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
        .with_workload_location(workload_location);
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

    fn admitted_decision(
        workload_name: &str,
        invocation_id: &str,
        generation: u64,
    ) -> TenantIsolationDecision {
        admitted_decision_for_location(
            workload_name,
            invocation_id,
            generation,
            WorkloadLocation::new().with_node_id("node-a"),
        )
    }

    fn binding() -> LocalEnforcementBinding {
        LocalEnforcementBinding::from_decision(&admitted_decision("messages:send", "invoke-1", 7))
            .expect("binding should materialize")
    }

    fn node_agent_principal(authenticated: bool, node_id: &str) -> PrincipalContext {
        PrincipalContext {
            authenticated,
            claims: serde_json::Map::from_iter([(
                "node_id".to_string(),
                serde_json::Value::String(node_id.to_string()),
            )]),
            verified_claims: serde_json::Map::new(),
        }
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

    async fn activate_direct(
        backend: &DirectProcessBackend,
        binding: &LocalEnforcementBinding,
        seed: u8,
    ) {
        let plan = backend
            .validate(binding, direct_request())
            .expect("direct-process activation plan should validate");
        let (execution, claim) = activation_command_for_plan(&plan, seed);
        backend
            .activate_exact(execution, claim, direct_request())
            .await
            .expect("compute-authorized direct-process activation should succeed");
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
                ("Restart", "no"),
                ("RestartSec", "3"),
                ("MemoryMax", "536870912"),
                ("CPUWeight", "100"),
                ("TasksMax", "128"),
            ])
            .expect("allowlisted systemd properties should parse"),
        )
    }

    #[tokio::test]
    async fn direct_process_reconciler_observes_stops_and_writes_status() {
        let backend = CountingBackend::new(DirectProcessBackend::new());
        let writer = RecordingStatusEvidenceWriter::default();
        let reconciler = NodeWorkloadReconciler::new(backend.clone(), writer.clone());
        let binding = binding();
        let original_spec = binding.spec().clone();
        activate_direct(&backend.inner, &binding, 0x91).await;

        let observed = reconciler
            .reconcile_binding(&binding, direct_request())
            .await
            .expect("direct-process reconciler should observe an activated workload");
        assert_eq!(observed.desired_state(), NodeWorkloadDesiredState::Running);
        assert_eq!(
            observed.action(),
            NodeWorkloadReconcileAction::ObservedRunning
        );
        assert_eq!(observed.status().phase(), TenantWorkloadPhase::Running);
        assert_eq!(backend.calls(), vec!["validate", "inspect"]);
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
                "validate", "inspect", "validate", "inspect", "stop", "inspect",
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
    async fn running_reconciliation_never_activates_an_absent_workload() {
        let backend = CountingBackend::new(DirectProcessBackend::new());
        let writer = RecordingStatusEvidenceWriter::default();
        let reconciler = NodeWorkloadReconciler::new(backend.clone(), writer.clone());

        let error = reconciler
            .reconcile_binding(&binding(), direct_request())
            .await
            .expect_err("an absent workload requires compute-issued activation authority");

        assert!(matches!(error, Error::NotFound(_)));
        assert_eq!(backend.calls(), vec!["validate", "inspect"]);
        assert!(
            writer.writes().is_empty(),
            "absence must not fabricate observed status evidence"
        );
    }

    #[tokio::test]
    async fn reconciler_treats_missing_workload_as_already_stopped() {
        let backend = CountingBackend::new(DirectProcessBackend::new());
        let writer = RecordingStatusEvidenceWriter::default();
        let reconciler = NodeWorkloadReconciler::new(backend.clone(), writer.clone());
        let binding = binding();
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
            .expect("missing workload should reconcile as already stopped");

        assert_eq!(stopped.desired_state(), NodeWorkloadDesiredState::Stopped);
        assert_eq!(
            stopped.action(),
            NodeWorkloadReconcileAction::ObservedStopped
        );
        assert_eq!(stopped.status().phase(), TenantWorkloadPhase::Deleting);
        assert_eq!(backend.calls(), vec!["validate", "inspect"]);
        assert_eq!(writer.writes().len(), 1);
    }

    #[tokio::test]
    async fn node_agent_reconciles_multiple_workloads_idempotently() {
        let backend = CountingBackend::new(DirectProcessBackend::new());
        let writer = RecordingStatusEvidenceWriter::default();
        let first = LocalEnforcementBinding::from_decision(&admitted_decision(
            "messages:send",
            "invoke-1",
            7,
        ))
        .expect("first binding should materialize");
        let second = LocalEnforcementBinding::from_decision(&admitted_decision(
            "jobs:compact",
            "invoke-2",
            7,
        ))
        .expect("second binding should materialize");
        activate_direct(&backend.inner, &first, 0x92).await;
        activate_direct(&backend.inner, &second, 0x93).await;
        let node_agent = NodeAgent::new(
            NodeIdentity::new("node-a").expect("node id should parse"),
            backend,
            writer.clone(),
        );
        let assignments = vec![
            NodeAgentAssignment::new(first, direct_request()),
            NodeAgentAssignment::new(second, direct_request()),
        ];

        let initial = node_agent.reconcile_assignments(assignments.clone()).await;
        assert_eq!(initial.node_id().as_str(), "node-a");
        assert_eq!(initial.outcomes().len(), 2);
        assert!(
            initial
                .outcomes()
                .iter()
                .all(|outcome| outcome.action() == NodeWorkloadReconcileAction::ObservedRunning),
            "first pass should observe both compute-activated workloads: {:?}",
            initial.outcomes()
        );
        assert!(
            initial.dispositions().iter().all(|disposition| matches!(
                disposition,
                NodeAssignmentDisposition::Reconciled { .. }
            )),
            "first pass should record successful assignment dispositions"
        );

        let second_pass = node_agent.reconcile_assignments(assignments).await;
        assert_eq!(second_pass.outcomes().len(), 2);
        assert!(
            second_pass
                .outcomes()
                .iter()
                .all(|outcome| outcome.action() == NodeWorkloadReconcileAction::ObservedRunning),
            "second pass should observe the already-running workloads: {:?}",
            second_pass.outcomes()
        );
        assert_eq!(
            writer.writes().len(),
            4,
            "each reconcile pass should write status evidence for each assignment"
        );
    }

    #[tokio::test]
    async fn node_capability_inspection_validates_and_observes_without_status_write() {
        let backend = CountingBackend::new(DirectProcessBackend::new());
        let writer = RecordingStatusEvidenceWriter::default();
        let node_agent = NodeAgent::new(
            NodeIdentity::new("node-a").expect("node id should parse"),
            backend.clone(),
            writer.clone(),
        );
        let assignment = NodeAgentAssignment::new(binding(), direct_request());
        activate_direct(&backend.inner, assignment.binding(), 0x94).await;
        let capability: Arc<dyn NodeWorkloadReconcileCapability> = Arc::new(node_agent);

        capability
            .reconcile_assignments(vec![assignment.clone()])
            .await
            .expect("compute-issued reconcile should complete");
        let writes_before_inspect = writer.writes().len();
        let observed = capability
            .inspect_assignment(&assignment)
            .await
            .expect("read-only capability inspection should observe the workload");

        assert_eq!(observed.reason(), HostLifecycleStatusReason::Running);
        assert_eq!(writer.writes().len(), writes_before_inspect);
        assert_eq!(
            backend.calls(),
            vec!["validate", "inspect", "validate", "inspect"],
            "capability inspection must add only validation and observation"
        );
    }

    #[tokio::test]
    async fn node_agent_rejects_missing_or_crossed_assignment_before_backend_or_status_effects() {
        let backend = CountingBackend::new(DirectProcessBackend::new());
        let writer = RecordingStatusEvidenceWriter::default();
        let node_agent = NodeAgent::new(
            NodeIdentity::new("node-b").expect("node id should parse"),
            backend.clone(),
            writer.clone(),
        );
        let missing_node_binding =
            LocalEnforcementBinding::from_decision(&admitted_decision_for_location(
                "messages:send",
                "invoke-without-node",
                7,
                WorkloadLocation::new(),
            ))
            .expect("binding without a node should materialize");
        let missing_node_assignment =
            NodeAgentAssignment::new(missing_node_binding, direct_request());
        let assignment = NodeAgentAssignment::new(binding(), direct_request());

        let missing_reconcile_error = node_agent
            .reconcile_assignment(missing_node_assignment.clone())
            .await
            .expect_err("missing assignment must fail before reconciliation");
        assert!(matches!(
            missing_reconcile_error,
            Error::PermissionDenied(_)
        ));
        assert!(
            missing_reconcile_error
                .to_string()
                .contains("admitted spec has no assigned node")
        );

        let missing_inspect_error = node_agent
            .inspect_assignment(&missing_node_assignment)
            .await
            .expect_err("missing assignment must fail before inspection");
        assert!(matches!(missing_inspect_error, Error::PermissionDenied(_)));
        assert!(
            missing_inspect_error
                .to_string()
                .contains("admitted spec has no assigned node")
        );

        let reconcile_error = node_agent
            .reconcile_assignment(assignment.clone())
            .await
            .expect_err("crossed assignment must fail before reconciliation");
        assert!(matches!(reconcile_error, Error::PermissionDenied(_)));
        assert!(
            reconcile_error
                .to_string()
                .contains("assigned to node node-a")
        );

        let inspect_error = node_agent
            .inspect_assignment(&assignment)
            .await
            .expect_err("crossed assignment must fail before inspection");
        assert!(matches!(inspect_error, Error::PermissionDenied(_)));
        assert!(
            inspect_error
                .to_string()
                .contains("assigned to node node-a")
        );
        assert!(
            backend.calls().is_empty(),
            "crossed assignment must not validate or invoke the backend"
        );
        assert!(
            writer.writes().is_empty(),
            "crossed assignment must not write status evidence"
        );
    }

    #[test]
    fn both_real_node_backends_implement_the_type_erased_capability() {
        let direct: Arc<dyn NodeWorkloadReconcileCapability> = Arc::new(NodeAgent::new(
            NodeIdentity::new("direct-node").expect("node id should parse"),
            DirectProcessBackend::new(),
            RecordingStatusEvidenceWriter::default(),
        ));
        let systemd: Arc<dyn NodeWorkloadReconcileCapability> = Arc::new(NodeAgent::new(
            NodeIdentity::new("systemd-node").expect("node id should parse"),
            SystemdTransientUnitBackend::unavailable("test host"),
            RecordingStatusEvidenceWriter::default(),
        ));

        assert_eq!(direct.backend_capabilities().len(), 1);
        assert_eq!(systemd.backend_capabilities().len(), 1);
        assert!(direct.backend_capabilities()[0].available());
        assert!(!systemd.backend_capabilities()[0].available());
    }

    #[test]
    fn node_agent_reports_capabilities() {
        let node_agent = NodeAgent::new(
            NodeIdentity::new("node-a").expect("node id should parse"),
            DirectProcessBackend::new(),
            RecordingStatusEvidenceWriter::default(),
        );

        let report = node_agent.capability_report();
        assert_eq!(report.node_id().as_str(), "node-a");
        assert_eq!(report.backend_capabilities().len(), 1);
        let capabilities = &report.backend_capabilities()[0];
        assert_eq!(
            capabilities.backend(),
            HostLifecycleBackendKind::DirectProcess
        );
        assert!(
            capabilities.available(),
            "direct-process backend should report available capabilities"
        );
        assert_eq!(capabilities.features().get("pid1"), Some(&false));
        assert_eq!(capabilities.features().get("dbus"), Some(&false));
    }

    #[test]
    fn node_agent_rejects_unauthenticated_transport() {
        let node_agent = NodeAgent::new(
            NodeIdentity::new("node-a").expect("node id should parse"),
            DirectProcessBackend::new(),
            RecordingStatusEvidenceWriter::default(),
        );

        let unauthenticated = node_agent
            .authorize_transport(node_agent_principal(false, "node-a"))
            .expect_err("node-agent transport must reject unauthenticated principals");
        assert!(
            unauthenticated.to_string().contains("authenticated"),
            "unauthenticated rejection should name the auth boundary: {unauthenticated}"
        );

        let wrong_node = node_agent
            .authorize_transport(node_agent_principal(true, "node-b"))
            .expect_err("node-agent transport must reject another node's principal");
        assert!(
            wrong_node.to_string().contains("node-b"),
            "node mismatch should name the presented node: {wrong_node}"
        );

        let admitted = node_agent
            .authorize_transport(node_agent_principal(true, "node-a"))
            .expect("matching authenticated node principal should be admitted");
        assert_eq!(admitted.node_id().as_str(), "node-a");
        assert!(admitted.principal().authenticated);
    }

    #[tokio::test]
    async fn node_state_transition_assignment_disposition() {
        let backend = CountingBackend::new(DirectProcessBackend::new());
        let writer = RecordingStatusEvidenceWriter::default();
        let valid = LocalEnforcementBinding::from_decision(&admitted_decision(
            "messages:send",
            "invoke-1",
            7,
        ))
        .expect("valid binding should materialize");
        let invalid = LocalEnforcementBinding::from_decision(&admitted_decision(
            "jobs:compact",
            "invoke-2",
            7,
        ))
        .expect("invalid binding should materialize");
        activate_direct(&backend.inner, &valid, 0x95).await;
        let node_agent = NodeAgent::new(
            NodeIdentity::new("node-a").expect("node id should parse"),
            backend,
            writer.clone(),
        );

        let capability: &dyn NodeWorkloadReconcileCapability = &node_agent;
        let typed_error = capability
            .reconcile_assignment(NodeAgentAssignment::new(invalid.clone(), systemd_request()))
            .await
            .expect_err("single-assignment capability should preserve the backend error");
        let Error::InvalidInput(message) = typed_error else {
            panic!("expected the original InvalidInput error, got {typed_error:?}");
        };
        assert!(
            message.contains("DirectProcessBackend requires a direct_process plan"),
            "typed error should preserve the validation reason: {message}"
        );

        let report = node_agent
            .reconcile_assignments([
                NodeAgentAssignment::new(valid, direct_request()),
                NodeAgentAssignment::new(invalid, systemd_request()),
            ])
            .await;

        assert_eq!(report.outcomes().len(), 1);
        assert_eq!(report.dispositions().len(), 2);
        assert!(
            matches!(
                &report.dispositions()[0],
                NodeAssignmentDisposition::Reconciled {
                    action: NodeWorkloadReconcileAction::ObservedRunning,
                    ..
                }
            ),
            "valid assignment should reconcile"
        );
        let NodeAssignmentDisposition::Failed { reason, .. } = &report.dispositions()[1] else {
            panic!("invalid assignment should have a failed disposition");
        };
        assert!(
            reason.contains("DirectProcessBackend requires a direct_process plan"),
            "failed disposition should preserve the validation reason: {reason}"
        );
        assert_eq!(
            writer.writes().len(),
            1,
            "failed assignment must not write status evidence"
        );
    }

    /// Backend that validates and stops via an inner DirectProcess backend but
    /// always fails `inspect` with `InvalidInput`, modelling a live
    /// systemd `.Failed` inspect fault (NoSuchUnit → NotFound, but a failed-state
    /// inspect maps to InvalidInput).
    #[derive(Clone)]
    struct InspectInvalidInputBackend {
        inner: DirectProcessBackend,
    }

    impl HostLifecycleBackend for InspectInvalidInputBackend {
        fn validate(
            &self,
            binding: &LocalEnforcementBinding,
            request: HostLifecycleRequest,
        ) -> Result<HostLifecyclePlan> {
            self.inner.validate(binding, request)
        }

        fn stop<'a>(
            &'a self,
            execution_id: WorkloadExecutionId,
        ) -> HostLifecycleFuture<'a, HostLifecycleStatus> {
            self.inner.stop(execution_id)
        }

        fn inspect<'a>(
            &'a self,
            _execution_id: WorkloadExecutionId,
        ) -> HostLifecycleFuture<'a, HostLifecycleStatus> {
            Box::pin(async move {
                Err(Error::InvalidInput(
                    "systemd inspect reports unit entered failed state".to_string(),
                ))
            })
        }
    }

    #[tokio::test]
    async fn running_observation_propagates_inspect_invalid_input_without_effects() {
        let backend = CountingBackend::new(InspectInvalidInputBackend {
            inner: DirectProcessBackend::new(),
        });
        let writer = RecordingStatusEvidenceWriter::default();
        let reconciler = NodeWorkloadReconciler::new(backend.clone(), writer.clone());
        let binding = binding();

        let error = reconciler
            .reconcile_binding(&binding, direct_request())
            .await
            .expect_err("a genuine inspect fault must propagate, not trigger a redundant start");
        assert!(
            matches!(error, Error::InvalidInput(_)),
            "inspect InvalidInput must surface unchanged: {error:?}"
        );
        assert_eq!(
            backend.calls(),
            vec!["validate", "inspect"],
            "running observation must not call any activation effect after an inspect fault"
        );
        assert!(
            writer.writes().is_empty(),
            "no status evidence should be written when inspect faults"
        );
    }

    #[tokio::test]
    async fn systemd_reconciler_observes_compute_activated_transient_unit() {
        let client = ReconcilerSystemdClient::available();
        let provider = SystemdTransientUnitBackend::new(client.clone());
        let writer = RecordingStatusEvidenceWriter::default();
        let binding = binding();
        let plan = provider
            .validate(&binding, systemd_request())
            .expect("systemd activation plan should validate");
        let (execution, claim) = activation_command_for_plan(&plan, 0x96);
        provider
            .activate_exact(execution, claim, systemd_request())
            .await
            .expect("compute-authorized systemd activation should succeed");
        let backend = CountingBackend::new(provider);
        let reconciler = NodeWorkloadReconciler::new(backend.clone(), writer.clone());

        let observed = reconciler
            .reconcile_spec(binding.spec().clone(), systemd_request())
            .await
            .expect("systemd reconciler should observe the activated workload");
        assert_eq!(
            observed.action(),
            NodeWorkloadReconcileAction::ObservedRunning
        );
        assert_eq!(observed.status().phase(), TenantWorkloadPhase::Running);
        assert_eq!(backend.calls(), vec!["validate", "inspect"]);
        assert_eq!(
            client.calls(),
            vec!["inspect_unit", "start_transient_unit", "inspect_unit"]
        );

        let start = client.last_start();
        assert_eq!(start.mode(), StartTransientMode::Fail);
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
                .contains("nimbus-wex_")
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

    #[tokio::test]
    async fn reconciler_rejects_provider_restart_and_duplicates_before_backend_validation() {
        for policy in [HostRestartPolicy::OnFailure, HostRestartPolicy::Always] {
            let backend = CountingBackend::new(DirectProcessBackend::new());
            let writer = RecordingStatusEvidenceWriter::default();
            let reconciler = NodeWorkloadReconciler::new(backend.clone(), writer.clone());
            let request = HostLifecycleRequest::new(
                HostLifecycleBackendKind::DirectProcess,
                HostExecutable::trusted("/usr/libexec/nimbus/direct-workload")
                    .expect("trusted executable should parse"),
            )
            .with_properties(HostLifecyclePropertySet::new([
                HostLifecycleProperty::Restart(policy),
            ]));

            let inspect_error = reconciler
                .inspect_binding(&binding(), request.clone())
                .await
                .expect_err("provider restart must fail before read-only backend inspection");
            assert!(matches!(inspect_error, Error::PermissionDenied(_)));
            let error = reconciler
                .reconcile_binding(&binding(), request)
                .await
                .expect_err("provider restart must fail before backend validation");
            assert!(matches!(error, Error::PermissionDenied(_)));
            assert!(error.to_string().contains("compute owns restart decisions"));
            assert!(backend.calls().is_empty());
            assert!(writer.writes().is_empty());
        }

        let backend = CountingBackend::new(DirectProcessBackend::new());
        let writer = RecordingStatusEvidenceWriter::default();
        let reconciler = NodeWorkloadReconciler::new(backend.clone(), writer.clone());
        let duplicate = HostLifecycleRequest::new(
            HostLifecycleBackendKind::DirectProcess,
            HostExecutable::trusted("/usr/libexec/nimbus/direct-workload")
                .expect("trusted executable should parse"),
        )
        .with_properties(HostLifecyclePropertySet::new([
            HostLifecycleProperty::Restart(HostRestartPolicy::No),
            HostLifecycleProperty::Restart(HostRestartPolicy::No),
        ]));

        let inspect_error = reconciler
            .inspect_binding(&binding(), duplicate.clone())
            .await
            .expect_err("duplicate restart properties must fail before backend inspection");
        assert!(matches!(inspect_error, Error::InvalidInput(_)));
        let error = reconciler
            .reconcile_binding(&binding(), duplicate)
            .await
            .expect_err("duplicate restart properties must fail before backend validation");
        assert!(matches!(error, Error::InvalidInput(_)));
        assert!(error.to_string().contains("duplicate Restart"));
        assert!(backend.calls().is_empty());
        assert!(writer.writes().is_empty());
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
                    crate::WorkloadGeneration::new(spec.generation().as_u64() - 1),
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
