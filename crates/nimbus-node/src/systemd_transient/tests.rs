use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use nimbus_testing::AdmittedDecisionScenario;
use nimbus_workloads::{
    WorkloadExecutionAttemptId, WorkloadExecutionReference, WorkloadGeneration,
    WorkloadProvisionDispatchClaim, WorkloadProvisionProviderTarget,
    WorkloadProvisionSourceGeneration, WorkloadRestartCommandId, WorkloadRestartDispatchEpoch,
    WorkloadRestartEpoch, WorkloadRestartRequestId, WorkloadRestartStep, WorkloadSagaRevision,
    WorkloadSagaTransitionId, WorkloadTeardownCommandMode, WorkloadTeardownStep,
};
use serde_json::json;

use super::*;
use crate::host_lifecycle::{
    HostRestartProviderClaim, HostRestartProviderClaimInput,
    teardown_fail_before_tests::{fixture as teardown_fixture, input as teardown_input},
    test_support::activation_command_for_plan,
};
use crate::{HostExecutable, HostLifecyclePropertySet, HostRestartPolicy, TenantWorkloadPhase};
use crate::{HostExecutionDrainProvider, HostTeardownExecuteClaim, HostTeardownExecuteObservation};

#[derive(Clone)]
struct FakeSystemdDbusClient {
    capabilities: SystemdTransientCapabilities,
    last_start: Arc<Mutex<Option<SystemdStartTransientUnitRequest>>>,
    last_stop: Arc<Mutex<Option<SystemdStopUnitRequest>>>,
    status: Arc<Mutex<Option<SystemdUnitStatus>>>,
    start_effects: Arc<AtomicUsize>,
    stop_effects: Arc<AtomicUsize>,
    lose_next_start_response: Arc<AtomicUsize>,
    lose_next_stop_response: Arc<AtomicUsize>,
    collect_unit_after_lost_stop_response: Arc<AtomicUsize>,
}

impl FakeSystemdDbusClient {
    fn available() -> Self {
        Self {
            capabilities: SystemdTransientCapabilities::available(),
            last_start: Arc::new(Mutex::new(None)),
            last_stop: Arc::new(Mutex::new(None)),
            status: Arc::new(Mutex::new(None)),
            start_effects: Arc::new(AtomicUsize::new(0)),
            stop_effects: Arc::new(AtomicUsize::new(0)),
            lose_next_start_response: Arc::new(AtomicUsize::new(0)),
            lose_next_stop_response: Arc::new(AtomicUsize::new(0)),
            collect_unit_after_lost_stop_response: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn with_capabilities(capabilities: SystemdTransientCapabilities) -> Self {
        Self {
            capabilities,
            last_start: Arc::new(Mutex::new(None)),
            last_stop: Arc::new(Mutex::new(None)),
            status: Arc::new(Mutex::new(None)),
            start_effects: Arc::new(AtomicUsize::new(0)),
            stop_effects: Arc::new(AtomicUsize::new(0)),
            lose_next_start_response: Arc::new(AtomicUsize::new(0)),
            lose_next_stop_response: Arc::new(AtomicUsize::new(0)),
            collect_unit_after_lost_stop_response: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn last_start(&self) -> SystemdStartTransientUnitRequest {
        self.last_start
            .lock()
            .expect("fake client lock should not be poisoned")
            .clone()
            .expect("start should have been called")
    }

    fn start_effect_count(&self) -> usize {
        self.start_effects.load(Ordering::SeqCst)
    }

    fn stop_effect_count(&self) -> usize {
        self.stop_effects.load(Ordering::SeqCst)
    }

    fn clear_unit(&self) {
        *self
            .status
            .lock()
            .expect("fake client lock should not be poisoned") = None;
    }

    fn lose_next_start_response(&self) {
        self.lose_next_start_response.store(1, Ordering::SeqCst);
    }

    fn lose_next_stop_response(&self) {
        self.lose_next_stop_response.store(1, Ordering::SeqCst);
    }

    fn lose_next_stop_response_and_collect_unit(&self) {
        self.collect_unit_after_lost_stop_response
            .store(1, Ordering::SeqCst);
        self.lose_next_stop_response();
    }
}

impl SystemdDbusClient for FakeSystemdDbusClient {
    fn capabilities(&self) -> SystemdTransientCapabilities {
        self.capabilities.clone()
    }

    fn start_transient_unit<'a>(
        &'a self,
        request: SystemdStartTransientUnitRequest,
    ) -> HostLifecycleFuture<'a, SystemdStartTransientUnitResponse> {
        Box::pin(async move {
            if request.mode() == StartTransientMode::Fail
                && self
                    .status
                    .lock()
                    .expect("fake client lock should not be poisoned")
                    .as_ref()
                    .is_some_and(|status| !status.is_absent())
            {
                return Err(Error::ResourceExhausted(format!(
                    "systemd unit {} already exists",
                    request.unit_name().as_str()
                )));
            }
            let response = SystemdStartTransientUnitResponse::new(
                request.unit_name().clone(),
                "/org/freedesktop/systemd1/job/42",
            )?;
            let mut status = SystemdUnitStatus::new(
                request.execution_id().clone(),
                request.unit_name().clone(),
                "activating",
                "start",
            )?
            .with_job_path(response.job_path())?;
            let log_extra_fields = request
                .properties()
                .iter()
                .find_map(|property| match property {
                    SystemdDbusProperty::LogExtraFields(fields) => Some(
                        fields
                            .iter()
                            .map(|field| field.as_bytes().to_vec())
                            .collect::<Vec<_>>(),
                    ),
                    _ => None,
                })
                .expect("fake start request should retain LogExtraFields");
            if let Some(fence) = HostActivationFence::from_log_extra_fields(&log_extra_fields)? {
                status = status.with_activation_fence(fence);
            }
            self.start_effects.fetch_add(1, Ordering::SeqCst);
            *self
                .last_start
                .lock()
                .expect("fake client lock should not be poisoned") = Some(request);
            *self
                .status
                .lock()
                .expect("fake client lock should not be poisoned") = Some(status);
            if self.lose_next_start_response.swap(0, Ordering::SeqCst) == 1 {
                return Err(Error::ResourceExhausted(
                    "systemd start response was lost after the effect".to_owned(),
                ));
            }
            Ok(response)
        })
    }

    fn stop_unit<'a>(
        &'a self,
        request: SystemdStopUnitRequest,
    ) -> HostLifecycleFuture<'a, SystemdStopUnitResponse> {
        Box::pin(async move {
            let activation_fence = self
                .status
                .lock()
                .expect("fake client lock should not be poisoned")
                .as_ref()
                .and_then(SystemdUnitStatus::activation_fence)
                .cloned();
            *self
                .last_stop
                .lock()
                .expect("fake client lock should not be poisoned") = Some(request.clone());
            let mut status = SystemdUnitStatus::new(
                request.execution_id().clone(),
                request.unit_name().clone(),
                "inactive",
                "dead",
            )?;
            if let Some(activation_fence) = activation_fence {
                status = status.with_activation_fence(activation_fence);
            }
            self.stop_effects.fetch_add(1, Ordering::SeqCst);
            *self
                .status
                .lock()
                .expect("fake client lock should not be poisoned") = Some(status.clone());
            if self.lose_next_stop_response.swap(0, Ordering::SeqCst) == 1 {
                if self
                    .collect_unit_after_lost_stop_response
                    .swap(0, Ordering::SeqCst)
                    == 1
                {
                    self.clear_unit();
                }
                return Err(Error::ResourceExhausted(
                    "systemd stop response was lost after the effect".to_owned(),
                ));
            }
            SystemdStopUnitResponse::new("/org/freedesktop/systemd1/job/43", status)
        })
    }

    fn inspect_unit<'a>(
        &'a self,
        request: SystemdInspectUnitRequest,
    ) -> HostLifecycleFuture<'a, SystemdUnitStatus> {
        Box::pin(async move {
            let status = self
                .status
                .lock()
                .expect("fake client lock should not be poisoned")
                .clone()
                .unwrap_or_else(|| {
                    SystemdUnitStatus::explicitly_absent(
                        request.execution_id().clone(),
                        request.unit_name().clone(),
                    )
                    .expect("absent status should build")
                });
            Ok(status)
        })
    }
}

fn binding() -> LocalEnforcementBinding {
    AdmittedDecisionScenario::new()
        .with_surface("systemd.transient")
        .with_generation(12)
        .with_workload_name("service:run")
        .with_invocation_id("invoke-systemd")
        .binding()
}

fn request() -> HostLifecycleRequest {
    HostLifecycleRequest::new(
        HostLifecycleBackendKind::SystemdTransientUnit,
        HostExecutable::trusted("/usr/libexec/nimbus/conmon-crun-launcher")
            .expect("trusted executable should parse"),
    )
    .with_args(["--bundle", "/run/nimbus/bundles/workload"])
    .expect("args should parse")
    .with_properties(
        HostLifecyclePropertySet::from_raw_systemd_properties([
            ("Description", "Nimbus workload"),
            ("Restart", "no"),
            ("RestartSec", "2"),
            ("MemoryMax", "536870912"),
            ("CPUWeight", "100"),
            ("TasksMax", "128"),
        ])
        .expect("properties should parse"),
    )
}

fn execution_for_restart_epoch(
    source: &WorkloadExecutionReference,
    restart_epoch: WorkloadRestartEpoch,
) -> WorkloadExecutionReference {
    let mut value = serde_json::to_value(source).expect("execution reference should serialize");
    value["restartEpoch"] = json!(restart_epoch.to_string());
    value["attemptId"] = json!(
        WorkloadExecutionAttemptId::for_execution(source.execution_id(), restart_epoch).to_string()
    );
    serde_json::from_value(value).expect("restart execution reference should validate")
}

fn restart_claim_input(
    source: &WorkloadExecutionReference,
    provision_claim: &WorkloadProvisionDispatchClaim,
    restart_epoch: WorkloadRestartEpoch,
    step: WorkloadRestartStep,
    seed: u8,
) -> HostRestartProviderClaimInput {
    let execution = execution_for_restart_epoch(source, restart_epoch);
    let WorkloadProvisionProviderTarget::Execution { provider_id, .. } =
        provision_claim.provider_target()
    else {
        panic!("activation fixture should select one execution provider");
    };
    let digest = |offset: u8| format!("{:02x}", seed.wrapping_add(offset)).repeat(32);
    HostRestartProviderClaimInput {
        saga_id: provision_claim.attempt().saga_id().clone(),
        transition_id: format!("wst_{}", digest(1))
            .parse::<WorkloadSagaTransitionId>()
            .expect("restart transition ID should validate"),
        command_id: format!("wrc_{}", digest(2))
            .parse::<WorkloadRestartCommandId>()
            .expect("restart command ID should validate"),
        request_id: format!("wrr_{}", digest(3))
            .parse::<WorkloadRestartRequestId>()
            .expect("restart request ID should validate"),
        source_execution: source.clone(),
        execution,
        restart_epoch,
        dispatch_epoch: WorkloadRestartDispatchEpoch::new(0),
        issuing_revision: WorkloadSagaRevision::new(10),
        confirmed_revision: WorkloadSagaRevision::new(11),
        source_generation: WorkloadProvisionSourceGeneration::new(1),
        source_digest: provision_claim.attempt().source_digest(),
        network_plan_digest: provision_claim.attempt().network_plan_digest().to_string(),
        provider_selection: provider_id.clone(),
        step,
        mode: HostRestartProviderClaimInput::execute_mode(),
    }
}

fn restart_claim(
    source: &WorkloadExecutionReference,
    provision_claim: &WorkloadProvisionDispatchClaim,
    restart_epoch: WorkloadRestartEpoch,
    step: WorkloadRestartStep,
    seed: u8,
) -> HostRestartProviderClaim {
    HostRestartProviderClaim::new(restart_claim_input(
        source,
        provision_claim,
        restart_epoch,
        step,
        seed,
    ))
    .expect("restart provider claim should validate")
}

#[test]
fn host_restart_claim_requires_mode_exact_confirmation_revision() {
    let plan = HostLifecyclePlan::from_binding(&binding(), request()).expect("plan should build");
    let (source, provision_claim) = activation_command_for_plan(&plan, 0x40);
    let mut equal = restart_claim_input(
        &source,
        &provision_claim,
        WorkloadRestartEpoch::new(1),
        WorkloadRestartStep::QuiesceExecution,
        0x41,
    );
    equal.confirmed_revision = equal.issuing_revision;
    let error = HostRestartProviderClaim::new(equal)
        .expect_err("an unconfirmed restart claim must fail closed");
    assert!(error.to_string().contains("confirmation revision"));

    let mut skipped = restart_claim_input(
        &source,
        &provision_claim,
        WorkloadRestartEpoch::new(1),
        WorkloadRestartStep::QuiesceExecution,
        0x42,
    );
    skipped.confirmed_revision = WorkloadSagaRevision::new(13);
    let error = HostRestartProviderClaim::new(skipped)
        .expect_err("a skipped confirmation revision must fail closed");
    assert!(error.to_string().contains("confirmation revision"));
}

#[test]
fn host_restart_claim_accepts_later_successor_veto_inspection_revision() {
    let plan = HostLifecyclePlan::from_binding(&binding(), request()).expect("plan should build");
    let (source, provision_claim) = activation_command_for_plan(&plan, 0x43);
    let mut input = restart_claim_input(
        &source,
        &provision_claim,
        WorkloadRestartEpoch::new(1),
        WorkloadRestartStep::QuiesceExecution,
        0x44,
    );
    input.confirmed_revision = WorkloadSagaRevision::new(13);
    input.mode = HostRestartProviderClaimInput::inspect_mode_after_successor_veto(
        source
            .generation()
            .checked_next()
            .expect("fixture generation should have a successor"),
    );
    HostRestartProviderClaim::new(input)
        .expect("a later durable successor-veto inspection should validate");
}

#[test]
fn host_restart_claim_rejects_later_execute_and_unauthenticated_inspection() {
    let plan = HostLifecyclePlan::from_binding(&binding(), request()).expect("plan should build");
    let (source, provision_claim) = activation_command_for_plan(&plan, 0x45);
    let later_revision = WorkloadSagaRevision::new(13);

    let mut execute = restart_claim_input(
        &source,
        &provision_claim,
        WorkloadRestartEpoch::new(1),
        WorkloadRestartStep::ActivateExecution,
        0x46,
    );
    execute.confirmed_revision = later_revision;
    let execute_error = HostRestartProviderClaim::new(execute)
        .expect_err("a later revision must not grant execute authority");
    assert!(execute_error.to_string().contains("confirmation revision"));

    let mut missing_veto = restart_claim_input(
        &source,
        &provision_claim,
        WorkloadRestartEpoch::new(1),
        WorkloadRestartStep::ActivateExecution,
        0x47,
    );
    missing_veto.confirmed_revision = later_revision;
    missing_veto.mode = HostRestartProviderClaimInput::inspect_mode();
    let missing_veto_error = HostRestartProviderClaim::new(missing_veto)
        .expect_err("a later inspection requires durable successor-veto evidence");
    assert!(
        missing_veto_error
            .to_string()
            .contains("confirmation revision")
    );

    let mut crossed_veto = restart_claim_input(
        &source,
        &provision_claim,
        WorkloadRestartEpoch::new(1),
        WorkloadRestartStep::ActivateExecution,
        0x48,
    );
    crossed_veto.confirmed_revision = later_revision;
    crossed_veto.mode =
        HostRestartProviderClaimInput::inspect_mode_after_successor_veto(source.generation());
    let crossed_veto_error = HostRestartProviderClaim::new(crossed_veto)
        .expect_err("a successor veto crossed with the active generation must fail closed");
    assert!(
        crossed_veto_error
            .to_string()
            .contains("later workload generation")
    );

    let mut stale_inspection = restart_claim_input(
        &source,
        &provision_claim,
        WorkloadRestartEpoch::new(1),
        WorkloadRestartStep::ActivateExecution,
        0x49,
    );
    stale_inspection.confirmed_revision = WorkloadSagaRevision::new(11);
    stale_inspection.mode = HostRestartProviderClaimInput::inspect_mode_after_successor_veto(
        WorkloadGeneration::new(source.generation().as_u64() + 1),
    );
    let stale_error = HostRestartProviderClaim::new(stale_inspection)
        .expect_err("an inspection revision before the inspection transition must fail closed");
    assert!(stale_error.to_string().contains("confirmation revision"));
}

#[tokio::test]
async fn host_restart_inspection_authority_cannot_apply_provider_effects() {
    let client = FakeSystemdDbusClient::available();
    let backend = SystemdTransientUnitBackend::new(client.clone());
    let plan = backend
        .validate(&binding(), request())
        .expect("systemd plan should validate");
    let (source, provision_claim) = activation_command_for_plan(&plan, 0x43);
    backend
        .activate_exact(source.clone(), provision_claim.clone(), request())
        .await
        .expect("initial activation should start");

    let mut quiesce_input = restart_claim_input(
        &source,
        &provision_claim,
        WorkloadRestartEpoch::new(1),
        WorkloadRestartStep::QuiesceExecution,
        0x44,
    );
    quiesce_input.confirmed_revision = WorkloadSagaRevision::new(13);
    quiesce_input.mode = HostRestartProviderClaimInput::inspect_mode_after_successor_veto(
        source
            .generation()
            .checked_next()
            .expect("fixture generation should have a successor"),
    );
    let quiesce = HostRestartProviderClaim::new(quiesce_input)
        .expect("exact inspection authority should validate");
    let error = backend
        .quiesce_restart_exact(quiesce)
        .await
        .expect_err("inspection authority must not invoke StopUnit");
    assert!(error.to_string().contains("execute authority"));
    assert_eq!(client.stop_effect_count(), 0);

    let mut activate_input = restart_claim_input(
        &source,
        &provision_claim,
        WorkloadRestartEpoch::new(1),
        WorkloadRestartStep::ActivateExecution,
        0x45,
    );
    activate_input.confirmed_revision = WorkloadSagaRevision::new(12);
    activate_input.mode = HostRestartProviderClaimInput::inspect_mode();
    let activate = HostRestartProviderClaim::new(activate_input)
        .expect("exact inspection authority should validate");
    let error = backend
        .activate_restart_exact(activate, request())
        .await
        .expect_err("inspection authority must not invoke StartTransientUnit");
    assert!(error.to_string().contains("execute authority"));
    assert_eq!(
        client.start_effect_count(),
        1,
        "only the initial provision activation may create a unit"
    );
}

#[test]
fn start_transient_unit_request_uses_trusted_exec_and_allowlisted_properties() {
    let binding = binding();
    let plan = HostLifecyclePlan::from_binding(&binding, request()).expect("plan should build");
    let request = SystemdStartTransientUnitRequest::from_plan(&plan).expect("request should build");

    assert_eq!(request.unit_name(), plan.unit_name());
    assert_eq!(request.mode().as_dbus_str(), "fail");
    assert_eq!(request.execution_id(), plan.execution_id());
    assert!(
        request.cgroup_path().contains(plan.unit_name().as_str()),
        "cgroup path should correlate to unit"
    );
    assert!(request.journal_selectors().iter().any(|selector| {
        selector.field() == "_SYSTEMD_UNIT" && selector.value() == plan.unit_name().as_str()
    }));
    assert!(request.journal_selectors().iter().any(|selector| {
        selector.field() == "NIMBUS_WORKLOAD_EXECUTION_ID"
            && selector.value() == plan.execution_id().as_str()
    }));
    assert!(
        request.properties().iter().any(|property| matches!(
            property,
            SystemdDbusProperty::LogExtraFields(fields)
                if fields == &[format!(
                    "{WORKLOAD_EXECUTION_JOURNAL_FIELD}={}",
                    plan.execution_id().as_str()
                )]
        )),
        "the execution-id selector must be materialized as unit journal metadata"
    );

    let exec = request
        .properties()
        .iter()
        .find_map(|property| match property {
            SystemdDbusProperty::ExecStart(exec) => Some(exec),
            _ => None,
        })
        .expect("ExecStart property should be generated by Nimbus");
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
    assert!(!exec.ignore_failure());
    assert!(request.properties().iter().any(|property| {
        matches!(
            property,
            SystemdDbusProperty::Restart(HostRestartPolicy::No)
        )
    }));
    assert!(
        request
            .properties()
            .iter()
            .any(|property| { matches!(property, SystemdDbusProperty::MemoryMax(536870912)) })
    );
}

#[test]
fn activation_fence_is_complete_in_inspectable_systemd_properties() {
    let binding = binding();
    let plan = HostLifecyclePlan::from_binding(&binding, request())
        .expect("plan should build")
        .with_test_activation_fence(0x31, 7);
    let expected_fence = plan
        .activation_fence()
        .expect("test plan should retain its fence");
    let expected_fields = expected_fence.journal_fields();
    let request = SystemdStartTransientUnitRequest::from_plan(&plan).expect("request should build");

    let retained_fields = request
        .properties()
        .iter()
        .find_map(|property| match property {
            SystemdDbusProperty::LogExtraFields(fields) => Some(fields),
            _ => None,
        })
        .expect("LogExtraFields should retain the activation fence");
    assert_eq!(retained_fields, &expected_fields);
    assert_eq!(request.activation_fence(), Some(expected_fence));
    for field in &expected_fields {
        let (name, value) = field
            .split_once('=')
            .expect("fence field should use NAME=value form");
        assert!(
            request
                .journal_selectors()
                .iter()
                .any(|selector| { selector.field() == name && selector.value() == value })
        );
    }
    assert!(expected_fields.iter().any(|field| {
        field == &format!("NIMBUS_WORKLOAD_EXECUTION_ID={}", plan.execution_id())
    }));
    for required in [
        "NIMBUS_WORKLOAD_UID=",
        "NIMBUS_NODE_IDENTITY=",
        "NIMBUS_WORKLOAD_EXECUTION_ATTEMPT_ID=",
        "NIMBUS_WORKLOAD_EXECUTION_PROVIDER_ID=",
        "NIMBUS_PROVISION_ATTEMPT_ID=",
        "NIMBUS_PROVISION_DISPATCH_EPOCH=",
        "NIMBUS_PROVISION_CLAIMED_REVISION=",
        "NIMBUS_WORKLOAD_GENERATION=",
        "NIMBUS_WORKLOAD_DESIRED_DIGEST=",
        "NIMBUS_WORKLOAD_SOURCE_DIGEST=",
        "NIMBUS_NETWORK_PLAN_DIGEST=",
    ] {
        assert!(
            expected_fields
                .iter()
                .any(|field| field.starts_with(required)),
            "missing inspectable fence field {required}"
        );
    }
}

#[test]
fn systemd_backend_rejects_disallowed_properties_and_wrong_backend_plan() {
    let binding = binding();
    let backend = SystemdTransientUnitBackend::new(FakeSystemdDbusClient::available());
    let denied =
        HostLifecyclePropertySet::from_raw_systemd_properties([("ExecStart", "/bin/sh -c escape")])
            .expect_err("raw ExecStart should fail before backend validation");
    assert!(denied.to_string().contains("not allowlisted"));

    let wrong_request = HostLifecycleRequest::new(
        HostLifecycleBackendKind::DirectProcess,
        HostExecutable::trusted("/usr/libexec/nimbus/conmon-crun-launcher")
            .expect("trusted executable should parse"),
    );
    let error = backend
        .validate(&binding, wrong_request)
        .expect_err("systemd backend should reject direct process plan");
    assert!(
        error
            .to_string()
            .contains("requires a systemd_transient_unit plan"),
        "error should name backend mismatch: {error}"
    );
}

#[tokio::test]
async fn backend_exact_activation_maps_stop_and_inspect_status() {
    let client = FakeSystemdDbusClient::available();
    let backend = SystemdTransientUnitBackend::new(client.clone());
    let binding = binding();
    let plan = backend
        .validate(&binding, request())
        .expect("systemd plan should validate");
    let execution_id = plan.execution_id().clone();

    let (execution, claim) = activation_command_for_plan(&plan, 0x40);
    let activated = backend
        .activate_exact(execution.clone(), claim.clone(), request())
        .await
        .expect("systemd exact activation should submit transient unit");
    assert_eq!(activated.phase(), TenantWorkloadPhase::Bound);
    let start = client.last_start();
    assert_eq!(start.unit_name(), activated.unit_name());

    let inspected = backend
        .inspect_activation(execution, claim, request())
        .await
        .expect("exact inspection should map fake D-Bus status");
    assert_eq!(
        inspected.reason(),
        super::super::HostLifecycleStatusReason::Submitted
    );

    let stopped = backend
        .stop(execution_id)
        .await
        .expect("stop should map fake D-Bus status");
    assert_eq!(
        stopped.reason(),
        super::super::HostLifecycleStatusReason::Stopped
    );
}

#[tokio::test]
async fn fresh_backend_systemd_exact_replay_adopts_without_another_start_effect() {
    let client = FakeSystemdDbusClient::available();
    let first_backend = SystemdTransientUnitBackend::new(client.clone());
    let plan = first_backend
        .validate(&binding(), request())
        .expect("systemd plan should validate");
    let (execution, claim) = activation_command_for_plan(&plan, 0x41);

    let first = first_backend
        .activate_exact(execution.clone(), claim.clone(), request())
        .await
        .expect("first exact activation should start");
    drop(first_backend);

    let recovered_backend = SystemdTransientUnitBackend::new(client.clone());
    let replay = recovered_backend
        .activate_exact(execution.clone(), claim.clone(), request())
        .await
        .expect("fresh backend exact replay should adopt the inspected unit");
    let inspected = recovered_backend
        .inspect_activation(execution, claim, request())
        .await
        .expect("exact inspection should adopt the retained fence");

    assert_eq!(replay.phase(), first.phase());
    assert_eq!(
        replay.lifecycle_evidence().correlation_ids(),
        first.lifecycle_evidence().correlation_ids()
    );
    assert_eq!(inspected.reason(), HostLifecycleStatusReason::Submitted);
    assert_eq!(client.start_effect_count(), 1);
}

#[tokio::test]
async fn restart_quiescence_rejects_crossed_source_before_stop() {
    let client = FakeSystemdDbusClient::available();
    let backend = SystemdTransientUnitBackend::new(client.clone());
    let plan = backend
        .validate(&binding(), request())
        .expect("systemd plan should validate");
    let (source, provision_claim) = activation_command_for_plan(&plan, 0x42);
    backend
        .activate_exact(source.clone(), provision_claim.clone(), request())
        .await
        .expect("initial activation should start");

    let crossed_source = execution_for_restart_epoch(&source, WorkloadRestartEpoch::new(1));
    let crossed = restart_claim(
        &crossed_source,
        &provision_claim,
        WorkloadRestartEpoch::new(2),
        WorkloadRestartStep::QuiesceExecution,
        0x43,
    );
    let error = backend
        .quiesce_restart_exact(crossed)
        .await
        .expect_err("crossed source attempt should fail closed");

    assert!(error.to_string().contains("crossed"));
    assert_eq!(
        client.stop_effect_count(),
        0,
        "crossed source authority must fail before StopUnit"
    );
}

#[tokio::test]
async fn restart_quiescence_and_activation_replay_each_provider_effect_once() {
    let client = FakeSystemdDbusClient::available();
    let backend = SystemdTransientUnitBackend::new(client.clone());
    let plan = backend
        .validate(&binding(), request())
        .expect("systemd plan should validate");
    let (source, provision_claim) = activation_command_for_plan(&plan, 0x44);
    backend
        .activate_exact(source.clone(), provision_claim.clone(), request())
        .await
        .expect("initial activation should start");

    let quiesce = restart_claim(
        &source,
        &provision_claim,
        WorkloadRestartEpoch::new(1),
        WorkloadRestartStep::QuiesceExecution,
        0x45,
    );
    backend
        .quiesce_restart_exact(quiesce.clone())
        .await
        .expect("first exact quiescence should stop the source");
    backend
        .quiesce_restart_exact(quiesce)
        .await
        .expect("equal quiescence replay should adopt the stopped source");
    assert_eq!(client.stop_effect_count(), 1);

    client.clear_unit();
    let activate = restart_claim(
        &source,
        &provision_claim,
        WorkloadRestartEpoch::new(1),
        WorkloadRestartStep::ActivateExecution,
        0x45,
    );
    backend
        .activate_restart_exact(activate.clone(), request())
        .await
        .expect("first exact restart activation should start the target");
    backend
        .activate_restart_exact(activate, request())
        .await
        .expect("equal restart activation replay should adopt the target");
    assert_eq!(
        client.start_effect_count(),
        2,
        "initial and restarted attempts should each create one unit"
    );
}

#[tokio::test]
async fn restart_ambiguous_provider_results_are_inspected_without_retry() {
    let client = FakeSystemdDbusClient::available();
    let backend = SystemdTransientUnitBackend::new(client.clone());
    let plan = backend
        .validate(&binding(), request())
        .expect("systemd plan should validate");
    let (source, provision_claim) = activation_command_for_plan(&plan, 0x4d);
    backend
        .activate_exact(source.clone(), provision_claim.clone(), request())
        .await
        .expect("initial activation should start");

    let quiesce = restart_claim(
        &source,
        &provision_claim,
        WorkloadRestartEpoch::new(1),
        WorkloadRestartStep::QuiesceExecution,
        0x4e,
    );
    client.lose_next_stop_response();
    backend
        .quiesce_restart_exact(quiesce)
        .await
        .expect("ambiguous stop should adopt the exact stopped source");
    assert_eq!(
        client.stop_effect_count(),
        1,
        "ambiguous stop recovery must inspect without another StopUnit"
    );

    client.clear_unit();
    let activate = restart_claim(
        &source,
        &provision_claim,
        WorkloadRestartEpoch::new(1),
        WorkloadRestartStep::ActivateExecution,
        0x4e,
    );
    client.lose_next_start_response();
    backend
        .activate_restart_exact(activate, request())
        .await
        .expect("ambiguous start should adopt the exact target unit");
    assert_eq!(
        client.start_effect_count(),
        2,
        "ambiguous start recovery must inspect without another StartTransientUnit"
    );
}

#[tokio::test]
async fn fresh_backend_restart_quiescence_authenticates_gc_absence_after_ambiguous_stop() {
    let client = FakeSystemdDbusClient::available();
    let backend = SystemdTransientUnitBackend::new(client.clone());
    let plan = backend
        .validate(&binding(), request())
        .expect("systemd plan should validate");
    let (source, provision_claim) = activation_command_for_plan(&plan, 0x54);
    backend
        .activate_exact(source.clone(), provision_claim.clone(), request())
        .await
        .expect("initial activation should start");
    let quiesce = restart_claim(
        &source,
        &provision_claim,
        WorkloadRestartEpoch::new(1),
        WorkloadRestartStep::QuiesceExecution,
        0x55,
    );

    client.lose_next_stop_response_and_collect_unit();
    let stopped = backend
        .quiesce_restart_exact(quiesce)
        .await
        .expect("authoritative systemd absence should resolve the lost stop response");

    assert_eq!(stopped.phase(), TenantWorkloadPhase::Deleting);
    assert_eq!(client.stop_effect_count(), 1);
}

#[tokio::test]
async fn restart_activation_rejects_crossed_target_before_another_start() {
    let client = FakeSystemdDbusClient::available();
    let backend = SystemdTransientUnitBackend::new(client.clone());
    let plan = backend
        .validate(&binding(), request())
        .expect("systemd plan should validate");
    let (source, provision_claim) = activation_command_for_plan(&plan, 0x46);
    let first = restart_claim(
        &source,
        &provision_claim,
        WorkloadRestartEpoch::new(1),
        WorkloadRestartStep::ActivateExecution,
        0x47,
    );
    backend
        .activate_restart_exact(first, request())
        .await
        .expect("first restart target should start");

    let crossed = restart_claim(
        &source,
        &provision_claim,
        WorkloadRestartEpoch::new(1),
        WorkloadRestartStep::ActivateExecution,
        0x48,
    );
    let error = backend
        .activate_restart_exact(crossed, request())
        .await
        .expect_err("crossed target claim should fail closed");

    assert!(error.to_string().contains("crossed"));
    assert_eq!(
        client.start_effect_count(),
        1,
        "crossed target authority must fail before another StartTransientUnit"
    );
}

#[tokio::test]
async fn fresh_backend_adopts_exact_restart_target_with_restart_disabled() {
    let client = FakeSystemdDbusClient::available();
    let first_backend = SystemdTransientUnitBackend::new(client.clone());
    let plan = first_backend
        .validate(&binding(), request())
        .expect("systemd plan should validate");
    let (source, provision_claim) = activation_command_for_plan(&plan, 0x49);
    let claim = restart_claim(
        &source,
        &provision_claim,
        WorkloadRestartEpoch::new(1),
        WorkloadRestartStep::ActivateExecution,
        0x4a,
    );
    first_backend
        .activate_restart_exact(claim.clone(), request())
        .await
        .expect("first restart activation should start");
    let start = client.last_start();
    assert!(start.properties().iter().any(|property| matches!(
        property,
        SystemdDbusProperty::Restart(HostRestartPolicy::No)
    )));
    drop(first_backend);

    let recovered_backend = SystemdTransientUnitBackend::new(client.clone());
    let mut inspection_input = restart_claim_input(
        &source,
        &provision_claim,
        WorkloadRestartEpoch::new(1),
        WorkloadRestartStep::ActivateExecution,
        0x4a,
    );
    inspection_input.confirmed_revision = WorkloadSagaRevision::new(12);
    inspection_input.mode = HostRestartProviderClaimInput::inspect_mode();
    inspection_input.transition_id = format!("wst_{}", "ab".repeat(32))
        .parse()
        .expect("inspection transition ID should validate");
    let inspection_claim = HostRestartProviderClaim::new(inspection_input)
        .expect("exact inspection claim should validate");
    recovered_backend
        .inspect_restart_activation(inspection_claim, request())
        .await
        .expect("fresh backend should inspect the stable target effect identity");
    recovered_backend
        .activate_restart_exact(claim, request())
        .await
        .expect("fresh backend replay should adopt the target");
    assert_eq!(client.start_effect_count(), 1);
}

#[tokio::test]
async fn restart_activation_fence_decoder_rejects_partial_mixed_and_noncanonical_fields() {
    let client = FakeSystemdDbusClient::available();
    let backend = SystemdTransientUnitBackend::new(client.clone());
    let plan = backend
        .validate(&binding(), request())
        .expect("systemd plan should validate");
    let (source, provision_claim) = activation_command_for_plan(&plan, 0x4b);
    let claim = restart_claim(
        &source,
        &provision_claim,
        WorkloadRestartEpoch::new(1),
        WorkloadRestartStep::ActivateExecution,
        0x4c,
    );
    backend
        .activate_restart_exact(claim, request())
        .await
        .expect("restart activation should start");
    let start = client.last_start();
    let fields = start
        .properties()
        .iter()
        .find_map(|property| match property {
            SystemdDbusProperty::LogExtraFields(fields) => Some(fields.clone()),
            _ => None,
        })
        .expect("restart activation should retain LogExtraFields");

    let partial = fields[..4]
        .iter()
        .map(|field| field.as_bytes().to_vec())
        .collect::<Vec<_>>();
    let partial_error = HostActivationFence::from_log_extra_fields(&partial)
        .expect_err("partial restart metadata must fail closed");
    assert!(partial_error.to_string().contains("incomplete"));

    let mut mixed = fields
        .iter()
        .map(|field| field.as_bytes().to_vec())
        .collect::<Vec<_>>();
    mixed.push(b"NIMBUS_PROVISION_ATTEMPT_ID=wpa_crossed".to_vec());
    let mixed_error = HostActivationFence::from_log_extra_fields(&mixed)
        .expect_err("mixed provision and restart metadata must fail closed");
    assert!(mixed_error.to_string().contains("mixed"));

    let mut noncanonical = fields
        .iter()
        .map(|field| field.as_bytes().to_vec())
        .collect::<Vec<_>>();
    let restart_epoch = fields
        .iter()
        .position(|field| field.starts_with("NIMBUS_RESTART_EPOCH="))
        .expect("restart fence should retain its epoch");
    noncanonical[restart_epoch] = b"NIMBUS_RESTART_EPOCH=01".to_vec();
    let noncanonical_error = HostActivationFence::from_log_extra_fields(&noncanonical)
        .expect_err("noncanonical restart metadata must fail closed");
    assert!(noncanonical_error.to_string().contains("canonical"));

    let unknown = vec![b"NIMBUS_RESTART_UNKNOWN=value".to_vec()];
    let unknown_error = HostActivationFence::from_log_extra_fields(&unknown)
        .expect_err("an unknown restart marker must not become an unfenced unit");
    assert!(unknown_error.to_string().contains("incomplete"));
}

#[tokio::test]
async fn systemd_crossed_fence_fails_before_start_transient_unit() {
    let client = FakeSystemdDbusClient::available();
    let backend = SystemdTransientUnitBackend::new(client.clone());
    let base = backend
        .validate(&binding(), request())
        .expect("systemd plan should validate");
    let (first_execution, first_claim) = activation_command_for_plan(&base, 0x51);

    backend
        .activate_exact(first_execution, first_claim, request())
        .await
        .expect("first exact activation should start");
    let (crossed_execution, crossed_claim) = activation_command_for_plan(&base, 0x61);
    let inspect_error = backend
        .inspect_activation(crossed_execution.clone(), crossed_claim.clone(), request())
        .await
        .expect_err("crossed exact inspection should fail closed");
    assert!(inspect_error.to_string().contains("crossed"));
    let error = backend
        .activate_exact(crossed_execution, crossed_claim, request())
        .await
        .expect_err("crossed exact activation should fail closed");

    assert!(error.to_string().contains("crossed"));
    assert_eq!(
        client.start_effect_count(),
        1,
        "crossed authority must fail during inspection before another start effect"
    );
}

#[tokio::test]
async fn concurrent_equal_systemd_activation_creates_one_unit() {
    let client = FakeSystemdDbusClient::available();
    let backend = SystemdTransientUnitBackend::new(client.clone());
    let plan = backend
        .validate(&binding(), request())
        .expect("systemd plan should validate");
    let (execution, claim) = activation_command_for_plan(&plan, 0x71);

    let (left, right) = tokio::join!(
        backend.activate_exact(execution.clone(), claim.clone(), request()),
        backend.activate_exact(execution, claim, request()),
    );
    left.expect("first concurrent activation should succeed");
    right.expect("equal concurrent activation should adopt");
    assert_eq!(client.start_effect_count(), 1);
}

#[tokio::test]
async fn systemd_closed_drain_barrier_blocks_restart_activation() {
    let client = FakeSystemdDbusClient::available();
    let state = tempfile::tempdir().expect("teardown state root should create");
    let backend =
        SystemdTransientUnitBackend::new_with_teardown_state_root(client.clone(), state.path())
            .expect("durable systemd backend should construct");
    let fixture = teardown_fixture(WorkloadTeardownStep::DrainExecution);
    backend
        .activate_exact(
            fixture.execution.clone(),
            fixture.activation_claim.clone(),
            request(),
        )
        .await
        .expect("initial exact activation should start");
    let drain = HostTeardownExecuteClaim::new(teardown_input(
        &fixture,
        WorkloadTeardownCommandMode::Execute,
    ))
    .expect("exact drain claim should validate");
    assert!(matches!(
        backend.execute_drain(drain).await,
        HostTeardownExecuteObservation::Succeeded(_)
    ));
    client.clear_unit();

    let restart = restart_claim(
        &fixture.execution,
        &fixture.activation_claim,
        WorkloadRestartEpoch::new(1),
        WorkloadRestartStep::ActivateExecution,
        0x72,
    );
    let error = backend
        .activate_restart_exact(restart, request())
        .await
        .expect_err("closed drain barrier must reject restart activation");

    assert!(error.to_string().contains("admission is closed"));
    assert_eq!(
        client.start_effect_count(),
        1,
        "restart activation must fail before StartTransientUnit"
    );
}

#[test]
fn systemd_execution_teardown_capability_requires_durable_state() {
    let storeless = SystemdTransientUnitBackend::new(FakeSystemdDbusClient::available());
    let storeless_capabilities = storeless.execution_teardown_capabilities();

    assert!(storeless.backend_capabilities().available());
    assert!(!storeless_capabilities.available());
    assert_eq!(
        storeless_capabilities
            .features()
            .get("durable_teardown_state"),
        Some(&false)
    );
    assert_eq!(
        storeless_capabilities.failure_reasons(),
        ["durable systemd teardown state store is unavailable"]
    );

    let state = tempfile::tempdir().expect("teardown state root should create");
    let durable = SystemdTransientUnitBackend::new_with_teardown_state_root(
        FakeSystemdDbusClient::available(),
        state.path(),
    )
    .expect("durable systemd backend should construct");
    let durable_capabilities = durable.execution_teardown_capabilities();
    assert!(durable_capabilities.available());
    assert_eq!(
        durable_capabilities
            .features()
            .get("durable_teardown_state"),
        Some(&true)
    );
    assert!(durable_capabilities.failure_reasons().is_empty());
}

#[test]
fn systemd_execution_teardown_capability_preserves_backend_blockers() {
    let state = tempfile::tempdir().expect("teardown state root should create");
    let unavailable = SystemdTransientUnitBackend::new_with_teardown_state_root(
        UnavailableSystemdDbusClient::new("test host has no system bus"),
        state.path(),
    )
    .expect("durable state should not hide an unavailable systemd client");
    let capabilities = unavailable.execution_teardown_capabilities();

    assert!(!capabilities.available());
    assert_eq!(
        capabilities.features().get("durable_teardown_state"),
        Some(&true)
    );
    assert!(
        capabilities
            .failure_reasons()
            .iter()
            .any(|reason| reason.contains("D-Bus is unavailable"))
    );
    assert!(
        !capabilities
            .failure_reasons()
            .iter()
            .any(|reason| reason.contains("teardown state store"))
    );
}

#[test]
fn systemd_teardown_store_open_failure_prevents_backend_construction() {
    let parent = tempfile::tempdir().expect("temporary parent should create");
    let not_a_directory = parent.path().join("plain-file");
    std::fs::write(&not_a_directory, b"not a directory").expect("plain file fixture should write");

    let result = SystemdTransientUnitBackend::new_with_teardown_state_root(
        FakeSystemdDbusClient::available(),
        not_a_directory.join("teardown"),
    );
    assert!(result.is_err(), "an unusable state root must fail closed");
}

#[test]
fn systemd_backend_fails_closed_when_dbus_or_features_are_unavailable() {
    let binding = binding();
    for (capabilities, expected) in [
        (
            SystemdTransientCapabilities::available().without_dbus(),
            "D-Bus is unavailable",
        ),
        (
            SystemdTransientCapabilities::available().without_transient_units(),
            "transient units are unavailable",
        ),
        (
            SystemdTransientCapabilities::available().without_service_units(),
            "service units are unavailable",
        ),
    ] {
        let backend = SystemdTransientUnitBackend::new(FakeSystemdDbusClient::with_capabilities(
            capabilities,
        ));
        let error = backend
            .validate(&binding, request())
            .expect_err("unavailable systemd feature should fail closed");
        assert!(
            error.to_string().contains(expected),
            "expected `{expected}` in error, got {error}"
        );
    }

    let backend = SystemdTransientUnitBackend::unavailable("not linux");
    let error = backend
        .validate(&binding, request())
        .expect_err("unavailable default client should fail closed");
    assert!(error.to_string().contains("D-Bus is unavailable"));
}
