use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use nimbus_core::{Error, Result};
use nimbus_workloads::{WorkloadExecutionReference, WorkloadProvisionDispatchClaim};
use serde::Serialize;

use super::{
    HostBackendObservedState, HostLifecycleBackend, HostLifecycleBackendKind, HostLifecycleFuture,
    HostLifecyclePlan, HostLifecycleRequest, HostLifecycleStatus, HostLifecycleStatusReason,
    LocalEnforcementBinding, TenantWorkloadLifecycleEvidence, WorkloadExecutionId,
};
use crate::host_lifecycle::HostProviderPlan;

#[path = "direct_process/teardown.rs"]
mod teardown;
use teardown::DirectProcessTeardownState;

#[cfg(test)]
#[path = "direct_process/teardown/tests.rs"]
mod teardown_fail_before_tests;

#[derive(Debug, Clone, Default)]
pub struct DirectProcessBackend {
    state: Arc<Mutex<DirectProcessState>>,
}

impl DirectProcessBackend {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(DirectProcessState::default())),
        }
    }

    pub fn logs(&self, execution_id: &WorkloadExecutionId) -> Result<Vec<String>> {
        let state = self
            .state
            .lock()
            .expect("direct process backend lock should not be poisoned");
        let record = state.record(execution_id)?;
        Ok(record.logs.clone())
    }

    pub fn evidence(&self, execution_id: &WorkloadExecutionId) -> Result<DirectProcessEvidence> {
        let state = self
            .state
            .lock()
            .expect("direct process backend lock should not be poisoned");
        let record = state.record(execution_id)?;
        Ok(record.evidence.clone())
    }

    fn activate_provider_exact(&self, plan: HostProviderPlan) -> Result<HostLifecycleStatus> {
        let mut state = self
            .state
            .lock()
            .expect("direct process backend lock should not be poisoned");
        if let Some(existing) = state.records.get(plan.execution_id()) {
            if existing.plan == plan {
                return Ok(existing.status.clone());
            }
            return Err(Error::PermissionDenied(format!(
                "direct process activation for {} is crossed with the retained provider fence",
                plan.execution_id().as_str()
            )));
        }
        let process_id = state.allocate_process_id();
        let lifecycle_evidence = TenantWorkloadLifecycleEvidence::from_provider_plan(
            &plan,
            HostLifecycleStatusReason::Running,
        )
        .with_process_id(process_id);
        let status =
            HostLifecycleStatus::from_provider_state(&plan, HostBackendObservedState::Running)
                .with_lifecycle_evidence(lifecycle_evidence);
        let evidence = DirectProcessEvidence::from_plan(&plan, process_id);
        let logs = vec![
            format!("direct-process:{}:validated", plan.execution_id().as_str()),
            format!(
                "direct-process:{}:started:{}",
                plan.execution_id().as_str(),
                process_id
            ),
        ];
        state.records.insert(
            plan.execution_id().clone(),
            DirectProcessRecord {
                plan,
                status: status.clone(),
                process_id,
                logs,
                evidence,
                teardown: DirectProcessTeardownState::default(),
            },
        );
        Ok(status)
    }

    fn inspect_provider(&self, plan: &HostProviderPlan) -> Result<HostLifecycleStatus> {
        let state = self
            .state
            .lock()
            .expect("direct process backend lock should not be poisoned");
        let record = state.record(plan.execution_id())?;
        if &record.plan != plan {
            return Err(Error::PermissionDenied(format!(
                "direct process inspection for {} is crossed with the retained provider fence",
                plan.execution_id().as_str()
            )));
        }
        Ok(record.status.clone())
    }

    fn inspect_lifecycle(&self, plan: &HostLifecyclePlan) -> Result<HostLifecycleStatus> {
        let state = self
            .state
            .lock()
            .expect("direct process backend lock should not be poisoned");
        let record = state.record(plan.execution_id())?;
        if !record.plan.matches_lifecycle_projection(plan) {
            return Err(Error::PermissionDenied(format!(
                "direct process inspection for {} is crossed with the retained lifecycle projection",
                plan.execution_id().as_str()
            )));
        }
        Ok(record.status.clone())
    }
}

impl HostLifecycleBackend for DirectProcessBackend {
    fn validate(
        &self,
        binding: &LocalEnforcementBinding,
        request: HostLifecycleRequest,
    ) -> Result<HostLifecyclePlan> {
        let plan = HostLifecyclePlan::from_binding(binding, request)?;
        if plan.backend() != HostLifecycleBackendKind::DirectProcess {
            return Err(Error::InvalidInput(format!(
                "DirectProcessBackend requires a direct_process plan, got {:?}",
                plan.backend()
            )));
        }
        Ok(plan)
    }

    fn inspect<'a>(
        &'a self,
        execution_id: WorkloadExecutionId,
    ) -> HostLifecycleFuture<'a, HostLifecycleStatus> {
        let state = Arc::clone(&self.state);
        Box::pin(async move {
            let state = state
                .lock()
                .expect("direct process backend lock should not be poisoned");
            Ok(state.record(&execution_id)?.status.clone())
        })
    }

    fn inspect_exact<'a>(
        &'a self,
        plan: HostLifecyclePlan,
    ) -> HostLifecycleFuture<'a, HostLifecycleStatus> {
        Box::pin(async move { self.inspect_lifecycle(&plan) })
    }

    fn activate_exact<'a>(
        &'a self,
        execution: WorkloadExecutionReference,
        claim: WorkloadProvisionDispatchClaim,
        request: HostLifecycleRequest,
    ) -> HostLifecycleFuture<'a, HostLifecycleStatus> {
        Box::pin(async move {
            let plan = HostProviderPlan::from_execution(&execution, &claim, request)?;
            if plan.backend() != HostLifecycleBackendKind::DirectProcess {
                return Err(Error::InvalidInput(
                    "DirectProcessBackend exact activation requires a direct_process request"
                        .to_owned(),
                ));
            }
            self.activate_provider_exact(plan)
        })
    }

    fn inspect_activation<'a>(
        &'a self,
        execution: WorkloadExecutionReference,
        claim: WorkloadProvisionDispatchClaim,
        request: HostLifecycleRequest,
    ) -> HostLifecycleFuture<'a, HostLifecycleStatus> {
        Box::pin(async move {
            let plan = HostProviderPlan::from_execution(&execution, &claim, request)?;
            if plan.backend() != HostLifecycleBackendKind::DirectProcess {
                return Err(Error::InvalidInput(
                    "DirectProcessBackend exact inspection requires a direct_process request"
                        .to_owned(),
                ));
            }
            self.inspect_provider(&plan)
        })
    }
}

#[derive(Debug, Default)]
struct DirectProcessState {
    next_process_id: u64,
    records: BTreeMap<WorkloadExecutionId, DirectProcessRecord>,
}

impl DirectProcessState {
    fn allocate_process_id(&mut self) -> u64 {
        self.next_process_id += 1;
        10_000 + self.next_process_id
    }

    fn record(&self, execution_id: &WorkloadExecutionId) -> Result<&DirectProcessRecord> {
        self.records.get(execution_id).ok_or_else(|| {
            // A missing workload is `NotFound`, not `InvalidInput`: the request
            // was well-formed, the unit simply does not exist yet. The
            // coordinator relies on this distinction to authorize a same-attempt
            // retry only after exact absence, while crossed observations remain
            // hard failures.
            Error::NotFound(format!(
                "direct process backend has no workload {}",
                execution_id.as_str()
            ))
        })
    }
}

#[derive(Debug, Clone)]
struct DirectProcessRecord {
    plan: HostProviderPlan,
    status: HostLifecycleStatus,
    process_id: u64,
    logs: Vec<String>,
    evidence: DirectProcessEvidence,
    teardown: DirectProcessTeardownState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DirectProcessEvidence {
    process_id: u64,
    execution_id: WorkloadExecutionId,
    unit_name: String,
    executable: String,
    args: Vec<String>,
}

impl DirectProcessEvidence {
    fn from_plan(plan: &HostProviderPlan, process_id: u64) -> Self {
        Self {
            process_id,
            execution_id: plan.execution_id().clone(),
            unit_name: plan.unit_name().as_str().to_string(),
            executable: plan.executable().as_str().to_string(),
            args: plan.args().to_vec(),
        }
    }

    pub fn process_id(&self) -> u64 {
        self.process_id
    }

    pub fn execution_id(&self) -> &WorkloadExecutionId {
        &self.execution_id
    }

    pub fn unit_name(&self) -> &str {
        &self.unit_name
    }

    pub fn executable(&self) -> &str {
        &self.executable
    }

    pub fn args(&self) -> &[String] {
        &self.args
    }
}

#[cfg(test)]
mod tests {
    use nimbus_testing::AdmittedDecisionScenario;

    use super::*;
    use crate::host_lifecycle::test_support::activation_command_for_plan;
    use crate::{
        HostExecutable, HostLifecycleProperty, HostLifecyclePropertySet, HostLifecycleStatusReason,
        HostRestartPolicy, RuntimePoolTrustClass, TenantWorkloadPhase,
    };

    fn binding() -> LocalEnforcementBinding {
        AdmittedDecisionScenario::new()
            .with_surface("direct.process")
            .with_generation(11)
            .with_workload_name("smoke:run")
            .with_invocation_id("invoke-direct")
            .binding()
    }

    fn request() -> HostLifecycleRequest {
        HostLifecycleRequest::new(
            HostLifecycleBackendKind::DirectProcess,
            HostExecutable::trusted("/bin/nimbus-direct-test")
                .expect("trusted executable should parse"),
        )
        .with_args(["--mode", "smoke"])
        .expect("args should parse")
        .with_properties(HostLifecyclePropertySet::new([
            HostLifecycleProperty::Description("direct process smoke".to_string()),
            HostLifecycleProperty::Restart(HostRestartPolicy::No),
        ]))
        .with_trust_class(RuntimePoolTrustClass::SingleTenant)
    }

    #[tokio::test]
    async fn direct_process_backend_activates_and_inspects_workloads() {
        let backend = DirectProcessBackend::new();
        let binding = binding();
        let plan = backend
            .validate(&binding, request())
            .expect("direct process plan should validate from binding");
        let execution_id = plan.execution_id().clone();

        let (execution, claim) = activation_command_for_plan(&plan, 0x01);
        let activated = backend
            .activate_exact(execution.clone(), claim.clone(), request())
            .await
            .expect("direct process exact activation should succeed");
        assert_eq!(activated.phase(), TenantWorkloadPhase::Running);

        let inspected = backend
            .inspect_activation(execution, claim, request())
            .await
            .expect("exactly activated workload should inspect");
        assert_eq!(inspected.reason(), HostLifecycleStatusReason::Running);

        let inspected = backend
            .inspect(execution_id)
            .await
            .expect("started workload should inspect");
        assert_eq!(inspected.reason(), HostLifecycleStatusReason::Running);
    }

    #[tokio::test]
    async fn direct_process_inspect_exact_accepts_retained_fenced_activation() {
        let backend = DirectProcessBackend::new();
        let binding = binding();
        let plan = backend
            .validate(&binding, request())
            .expect("direct process plan should validate");
        let (execution, claim) = activation_command_for_plan(&plan, 0x7a);
        backend
            .activate_exact(execution, claim, request())
            .await
            .expect("exact activation should retain its dispatch fence");

        let inspected = backend
            .inspect_exact(plan.clone())
            .await
            .expect("lifecycle inspection should match without reproducing the dispatch fence");
        assert_eq!(inspected.reason(), HostLifecycleStatusReason::Running);

        let crossed_request = HostLifecycleRequest::new(
            HostLifecycleBackendKind::DirectProcess,
            HostExecutable::trusted("/bin/nimbus-direct-crossed")
                .expect("crossed executable should parse"),
        )
        .with_args(["--crossed"])
        .expect("crossed args should parse")
        .with_trust_class(RuntimePoolTrustClass::SingleTenant);
        let crossed_plan = backend
            .validate(&binding, crossed_request)
            .expect("crossed lifecycle plan should validate structurally");
        let error = backend
            .inspect_exact(crossed_plan)
            .await
            .expect_err("crossed lifecycle inputs must not observe the retained effect");
        assert!(error.to_string().contains("crossed"));
    }

    #[tokio::test]
    async fn direct_process_backend_emits_deterministic_logs_and_evidence() {
        let backend = DirectProcessBackend::new();
        let binding = binding();
        let plan = backend
            .validate(&binding, request())
            .expect("direct process plan should validate");
        let execution_id = plan.execution_id().clone();
        let (execution, claim) = activation_command_for_plan(&plan, 0x02);
        backend
            .activate_exact(execution, claim, request())
            .await
            .expect("direct process exact activation should succeed");

        let evidence = backend
            .evidence(&execution_id)
            .expect("evidence should be recorded");
        assert_eq!(evidence.process_id(), 10_001);
        assert_eq!(evidence.execution_id(), &execution_id);
        assert!(evidence.unit_name().starts_with("nimbus-wex_"));
        assert_eq!(evidence.executable(), "/bin/nimbus-direct-test");
        assert_eq!(
            evidence.args(),
            &["--mode".to_string(), "smoke".to_string()]
        );

        let logs = backend
            .logs(&execution_id)
            .expect("logs should be recorded");
        assert_eq!(
            logs,
            vec![
                format!("direct-process:{}:validated", execution_id.as_str()),
                format!("direct-process:{}:started:10001", execution_id.as_str()),
            ]
        );
    }

    #[tokio::test]
    async fn direct_process_exact_replay_adopts_the_original_process() {
        let backend = DirectProcessBackend::new();
        let binding = binding();
        let plan = backend
            .validate(&binding, request())
            .expect("direct process plan should validate");
        let execution_id = plan.execution_id().clone();

        let (execution, claim) = activation_command_for_plan(&plan, 0x03);
        backend
            .activate_exact(execution.clone(), claim.clone(), request())
            .await
            .expect("first direct process exact activation should succeed");
        let first = backend
            .evidence(&execution_id)
            .expect("first process evidence should exist");

        backend
            .activate_exact(execution, claim, request())
            .await
            .expect("exact direct process replay should adopt");
        let replay = backend
            .evidence(&execution_id)
            .expect("replayed process evidence should exist");

        assert_eq!(
            replay, first,
            "exact replay must not allocate a new process"
        );
    }

    #[tokio::test]
    async fn direct_process_crossed_fence_fails_before_process_allocation() {
        let backend = DirectProcessBackend::new();
        let binding = binding();
        let base = backend
            .validate(&binding, request())
            .expect("direct process plan should validate");
        let execution_id = base.execution_id().clone();
        let (first_execution, first_claim) = activation_command_for_plan(&base, 0x11);
        let (crossed_execution, crossed_claim) = activation_command_for_plan(&base, 0x22);

        backend
            .activate_exact(first_execution, first_claim, request())
            .await
            .expect("first exact activation should succeed");
        let error = backend
            .activate_exact(crossed_execution.clone(), crossed_claim.clone(), request())
            .await
            .expect_err("crossed activation must fail closed");
        assert!(error.to_string().contains("crossed"));
        assert_eq!(
            backend
                .evidence(&execution_id)
                .expect("original evidence should remain")
                .process_id(),
            10_001,
            "a crossed fence must not allocate another process"
        );
        let inspect_error = backend
            .inspect_activation(crossed_execution, crossed_claim, request())
            .await
            .expect_err("crossed exact inspection must fail closed");
        assert!(inspect_error.to_string().contains("crossed"));
    }

    #[tokio::test]
    async fn concurrent_equal_direct_process_activation_creates_one_process() {
        let backend = DirectProcessBackend::new();
        let binding = binding();
        let plan = backend
            .validate(&binding, request())
            .expect("direct process plan should validate");
        let execution_id = plan.execution_id().clone();
        let (execution, claim) = activation_command_for_plan(&plan, 0x33);

        let (left, right) = tokio::join!(
            backend.activate_exact(execution.clone(), claim.clone(), request()),
            backend.activate_exact(execution, claim, request()),
        );
        let left = left.expect("first concurrent activation should succeed");
        let right = right.expect("equal concurrent activation should adopt");

        assert_eq!(left, right);
        assert_eq!(
            backend
                .evidence(&execution_id)
                .expect("one process evidence should remain")
                .process_id(),
            10_001
        );
        assert_eq!(
            backend
                .logs(&execution_id)
                .expect("one process log should remain")
                .iter()
                .filter(|line| line.contains(":started:"))
                .count(),
            1,
            "equal concurrent activation must create one physical effect"
        );
    }

    #[test]
    fn direct_process_backend_rejects_non_direct_process_plans() {
        let backend = DirectProcessBackend::new();
        let binding = binding();
        let request = HostLifecycleRequest::new(
            HostLifecycleBackendKind::SystemdTransientUnit,
            HostExecutable::trusted("/bin/nimbus-direct-test")
                .expect("trusted executable should parse"),
        );
        let error = backend
            .validate(&binding, request)
            .expect_err("direct backend should reject systemd plans");
        assert!(
            error.to_string().contains("requires a direct_process plan"),
            "error should name backend mismatch: {error}"
        );
    }

    #[tokio::test]
    async fn direct_process_backend_fails_closed_for_unknown_workload() {
        let backend = DirectProcessBackend::new();
        let binding = binding();
        let plan = backend
            .validate(&binding, request())
            .expect("direct process plan should validate");
        let unknown = plan.execution_id().clone();

        let error = backend
            .inspect(unknown.clone())
            .await
            .expect_err("unknown workload inspect should fail closed");
        assert!(
            matches!(error, Error::NotFound(_)),
            "missing workload must be NotFound so the coordinator can distinguish exact absence: {error:?}"
        );
        assert!(
            error
                .to_string()
                .contains(&format!("has no workload {}", unknown.as_str())),
            "inspect error should name missing workload: {error}"
        );
    }
}
