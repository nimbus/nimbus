use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use nimbus_core::{Error, Result};
use serde::Serialize;

use super::{
    HostBackendObservedState, HostLifecycleBackend, HostLifecycleBackendKind, HostLifecycleFuture,
    HostLifecyclePlan, HostLifecycleRequest, HostLifecycleStatus, HostLifecycleStatusReason,
    LocalEnforcementBinding, TenantWorkloadLifecycleEvidence, TenantWorkloadStatus,
    WorkloadExecutionId,
};

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

    fn start<'a>(
        &'a self,
        plan: HostLifecyclePlan,
    ) -> HostLifecycleFuture<'a, TenantWorkloadStatus> {
        let state = Arc::clone(&self.state);
        Box::pin(async move {
            let mut state = state
                .lock()
                .expect("direct process backend lock should not be poisoned");
            let process_id = state.allocate_process_id();
            let lifecycle_evidence = TenantWorkloadLifecycleEvidence::from_plan(
                &plan,
                HostLifecycleStatusReason::Running,
            )
            .with_process_id(process_id);
            let status =
                HostLifecycleStatus::from_backend_state(&plan, HostBackendObservedState::Running)
                    .with_lifecycle_evidence(lifecycle_evidence);
            let workload_status = status.to_workload_status(&plan)?;
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
                    status,
                    process_id,
                    logs,
                    evidence,
                },
            );
            Ok(workload_status)
        })
    }

    fn stop<'a>(
        &'a self,
        execution_id: WorkloadExecutionId,
    ) -> HostLifecycleFuture<'a, HostLifecycleStatus> {
        let state = Arc::clone(&self.state);
        Box::pin(async move {
            let mut state = state
                .lock()
                .expect("direct process backend lock should not be poisoned");
            let record = state.record_mut(&execution_id)?;
            let status = HostLifecycleStatus::from_backend_state(
                &record.plan,
                HostBackendObservedState::Stopped,
            );
            record.status = status.clone();
            record.logs.push(format!(
                "direct-process:{}:stopped:{}",
                execution_id.as_str(),
                record.process_id
            ));
            Ok(status)
        })
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
            // reconciler relies on this distinction to start an absent workload
            // (NotFound) while propagating a genuine inspect fault
            // (InvalidInput) instead of masking it with a redundant start.
            Error::NotFound(format!(
                "direct process backend has no workload {}",
                execution_id.as_str()
            ))
        })
    }

    fn record_mut(
        &mut self,
        execution_id: &WorkloadExecutionId,
    ) -> Result<&mut DirectProcessRecord> {
        self.records.get_mut(execution_id).ok_or_else(|| {
            Error::NotFound(format!(
                "direct process backend has no workload {}",
                execution_id.as_str()
            ))
        })
    }
}

#[derive(Debug, Clone)]
struct DirectProcessRecord {
    plan: HostLifecyclePlan,
    status: HostLifecycleStatus,
    process_id: u64,
    logs: Vec<String>,
    evidence: DirectProcessEvidence,
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
    fn from_plan(plan: &HostLifecyclePlan, process_id: u64) -> Self {
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
    async fn direct_process_backend_starts_inspects_and_stops_workloads() {
        let backend = DirectProcessBackend::new();
        let binding = binding();
        let plan = backend
            .validate(&binding, request())
            .expect("direct process plan should validate from binding");
        let execution_id = plan.execution_id().clone();

        let started = backend
            .start(plan)
            .await
            .expect("direct process start should succeed");
        assert_eq!(started.phase(), TenantWorkloadPhase::Running);

        let inspected = backend
            .inspect(execution_id.clone())
            .await
            .expect("started workload should inspect");
        assert_eq!(inspected.reason(), HostLifecycleStatusReason::Running);

        let stopped = backend
            .stop(execution_id.clone())
            .await
            .expect("started workload should stop");
        assert_eq!(stopped.reason(), HostLifecycleStatusReason::Stopped);

        let inspected = backend
            .inspect(execution_id)
            .await
            .expect("stopped workload should inspect");
        assert_eq!(inspected.reason(), HostLifecycleStatusReason::Stopped);
    }

    #[tokio::test]
    async fn direct_process_backend_emits_deterministic_logs_and_evidence() {
        let backend = DirectProcessBackend::new();
        let binding = binding();
        let plan = backend
            .validate(&binding, request())
            .expect("direct process plan should validate");
        let execution_id = plan.execution_id().clone();
        backend
            .start(plan)
            .await
            .expect("direct process start should succeed");

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
            "missing workload must be NotFound (not InvalidInput) so the reconciler starts it: {error:?}"
        );
        assert!(
            error
                .to_string()
                .contains(&format!("has no workload {}", unknown.as_str())),
            "inspect error should name missing workload: {error}"
        );

        let error = backend
            .stop(unknown)
            .await
            .expect_err("unknown workload stop should fail closed");
        assert!(
            matches!(error, Error::NotFound(_)),
            "missing workload stop must be NotFound: {error:?}"
        );
        assert!(
            error.to_string().contains("has no workload"),
            "stop error should name missing workload: {error}"
        );
    }
}
