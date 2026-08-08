use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use nimbus_testing::AdmittedDecisionScenario;

use super::*;
use crate::host_lifecycle::test_support::activation_command_for_plan;
use crate::{HostExecutable, HostLifecyclePropertySet, HostRestartPolicy, TenantWorkloadPhase};

#[derive(Clone)]
struct FakeSystemdDbusClient {
    capabilities: SystemdTransientCapabilities,
    last_start: Arc<Mutex<Option<SystemdStartTransientUnitRequest>>>,
    last_stop: Arc<Mutex<Option<SystemdStopUnitRequest>>>,
    status: Arc<Mutex<Option<SystemdUnitStatus>>>,
    start_effects: Arc<AtomicUsize>,
}

impl FakeSystemdDbusClient {
    fn available() -> Self {
        Self {
            capabilities: SystemdTransientCapabilities::available(),
            last_start: Arc::new(Mutex::new(None)),
            last_stop: Arc::new(Mutex::new(None)),
            status: Arc::new(Mutex::new(None)),
            start_effects: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn with_capabilities(capabilities: SystemdTransientCapabilities) -> Self {
        Self {
            capabilities,
            last_start: Arc::new(Mutex::new(None)),
            last_stop: Arc::new(Mutex::new(None)),
            status: Arc::new(Mutex::new(None)),
            start_effects: Arc::new(AtomicUsize::new(0)),
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
            Ok(response)
        })
    }

    fn stop_unit<'a>(
        &'a self,
        request: SystemdStopUnitRequest,
    ) -> HostLifecycleFuture<'a, SystemdStopUnitResponse> {
        Box::pin(async move {
            *self
                .last_stop
                .lock()
                .expect("fake client lock should not be poisoned") = Some(request.clone());
            let status = SystemdUnitStatus::new(
                request.execution_id().clone(),
                request.unit_name().clone(),
                "inactive",
                "dead",
            )?;
            *self
                .status
                .lock()
                .expect("fake client lock should not be poisoned") = Some(status.clone());
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
                    SystemdUnitStatus::new(
                        request.execution_id().clone(),
                        request.unit_name().clone(),
                        "inactive",
                        "dead",
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
