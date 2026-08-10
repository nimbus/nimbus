//! End-to-end route proofs for the exact guest teardown-phase transport.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use nimbus_machine::api::{
    MACHINE_API_WORKLOAD_TEARDOWN_PHASE_PATH, MachineApiWorkloadTeardownCommandEnvelope,
    MachineApiWorkloadTeardownExecuteObservation, MachineApiWorkloadTeardownInspectObservation,
    MachineApiWorkloadTeardownObservation, MachineApiWorkloadTeardownPhaseRequest,
    MachineApiWorkloadTeardownPhaseResponse,
};
use nimbus_workloads::{WorkloadTeardownCommandMode, WorkloadTeardownStep};

use super::*;
use crate::machine::api::routes::MAX_WORKLOAD_TEARDOWN_REQUEST_BODY_BYTES;

#[derive(Default)]
struct RecordingTeardownFacade {
    calls: Mutex<Vec<MachineApiWorkloadTeardownCommandEnvelope>>,
    provider_effects: AtomicUsize,
    cross_next_response: AtomicBool,
}

impl RecordingTeardownFacade {
    fn call_count(&self) -> usize {
        self.calls.lock().expect("call log should be healthy").len()
    }

    fn commands(&self) -> Vec<MachineApiWorkloadTeardownCommandEnvelope> {
        self.calls
            .lock()
            .expect("call log should be healthy")
            .clone()
    }

    fn cross_next_response(&self) {
        self.cross_next_response.store(true, Ordering::SeqCst);
    }
}

impl MachineApiNodeWorkloadFacade for RecordingTeardownFacade {
    fn kind(&self) -> SandboxBackendKind {
        SandboxBackendKind::Container
    }

    fn teardown_execution_blockers(&self) -> Vec<String> {
        Vec::new()
    }

    fn teardown_provider_blockers(&self) -> Vec<String> {
        Vec::new()
    }

    fn inspect<'a>(
        &'a self,
        _id: &'a SandboxId,
    ) -> super::super::service_workloads::MachineApiServiceFuture<
        'a,
        Option<nimbus_sandbox::SandboxInspection>,
    > {
        Box::pin(async move {
            Err(MachineApiHttpError {
                status: StatusCode::SERVICE_UNAVAILABLE,
                message: "teardown route fixture does not expose coarse inspection".to_owned(),
            })
        })
    }

    fn stop<'a>(
        &'a self,
        _id: &'a SandboxId,
    ) -> super::super::service_workloads::MachineApiServiceFuture<'a, ()> {
        Box::pin(async move {
            Err(MachineApiHttpError {
                status: StatusCode::SERVICE_UNAVAILABLE,
                message: "teardown route fixture does not expose coarse stop".to_owned(),
            })
        })
    }

    fn absent_machine_port_receipts<'a>(
        &'a self,
        _id: &'a SandboxId,
    ) -> super::super::service_workloads::MachineApiServiceFuture<
        'a,
        Option<nimbus_sandbox::backends::container::MachinePortAbsenceEvidence>,
    > {
        Box::pin(async move {
            Err(MachineApiHttpError {
                status: StatusCode::SERVICE_UNAVAILABLE,
                message: "teardown route fixture has no coarse absence probe".to_owned(),
            })
        })
    }

    fn teardown_phase<'a>(
        &'a self,
        command: &'a MachineApiWorkloadTeardownCommandEnvelope,
        _forwarder: &'a MachineForwarderAuthority,
    ) -> super::super::service_workloads::MachineApiServiceFuture<
        'a,
        nimbus_machine::api::MachineApiWorkloadTeardownPhaseResult,
    > {
        let command = command.clone();
        Box::pin(async move {
            self.calls
                .lock()
                .expect("call log should be healthy")
                .push(command.clone());
            self.provider_effects.fetch_add(1, Ordering::SeqCst);
            let cross = self.cross_next_response.swap(false, Ordering::SeqCst);
            let observation = match (command.mode(), cross) {
                (WorkloadTeardownCommandMode::Execute, false)
                | (WorkloadTeardownCommandMode::Inspect, true) => {
                    MachineApiWorkloadTeardownObservation::Execute(
                        MachineApiWorkloadTeardownExecuteObservation::Ambiguous,
                    )
                }
                (WorkloadTeardownCommandMode::Inspect, false)
                | (WorkloadTeardownCommandMode::Execute, true) => {
                    MachineApiWorkloadTeardownObservation::Inspect(
                        MachineApiWorkloadTeardownInspectObservation::Ambiguous,
                    )
                }
            };
            nimbus_machine::api::MachineApiWorkloadTeardownPhaseResult::new(
                &command,
                observation,
                None,
            )
            .map_err(|error| MachineApiHttpError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: error.to_string(),
            })
        })
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn machine_api_workload_teardown_route_authenticates_before_facade_and_correlates_all_guest_steps_and_modes()
 {
    let (authority, _) = teardown_wire_fixture(
        WorkloadTeardownStep::DrainExecution,
        WorkloadTeardownCommandMode::Execute,
    );
    let temp_dir = short_socket_tempdir();
    let socket_path = temp_dir.path().join("nimbus.sock");
    let control_data_dir = temp_dir.path().join("control");
    let listener = bind_direct_listener(&socket_path).expect("listener should bind");
    let facade = Arc::new(RecordingTeardownFacade::default());
    let state = MachineApiState {
        control_data_dir: control_data_dir.clone(),
        listen_mode: MachineApiListenMode::DirectSocket,
        binary_lookup_path: None,
        helper_binary_dirs: Vec::new(),
        service_workloads: Some(facade.clone()),
        machine_port_forwarder: None,
        forwarder_authority: Some(authority.clone()),
    };
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(serve_machine_api(listener, state, async move {
        let _ = shutdown_rx.await;
    }));
    wait_for_socket_path(&socket_path);

    let guest_steps = [
        WorkloadTeardownStep::DrainExecution,
        WorkloadTeardownStep::StopExecution,
        WorkloadTeardownStep::DetachNetwork,
        WorkloadTeardownStep::ReleaseNetwork,
    ];
    let modes = [
        WorkloadTeardownCommandMode::Execute,
        WorkloadTeardownCommandMode::Inspect,
    ];
    let mut expected_commands = Vec::new();
    for step in guest_steps {
        for mode in modes {
            let (request_authority, command) = teardown_wire_fixture(step, mode);
            assert_eq!(request_authority, authority);
            let request =
                MachineApiWorkloadTeardownPhaseRequest::new(request_authority, command.clone())
                    .expect("exact route request should validate");
            let response = post_teardown(&socket_path, &request);
            response
                .validate_for_request(&request)
                .expect("route response must correlate every request fence");
            assert_eq!(response.mode(), mode);
            expected_commands.push(command);
        }
    }
    assert_eq!(facade.commands(), expected_commands);

    let valid = exact_request(
        WorkloadTeardownStep::DrainExecution,
        WorkloadTeardownCommandMode::Execute,
    );
    let valid_value = serde_json::to_value(&valid).expect("request should serialize");
    for invalid in [
        serde_json::json!({}),
        serde_json::Value::Null,
        serde_json::json!({ "unexpected": true }),
    ] {
        assert_rejected_before_facade(
            &socket_path,
            &facade,
            &control_data_dir,
            invalid,
            "422 Unprocessable Entity",
        );
    }
    let mut unknown = valid_value.clone();
    unknown["unexpected"] = serde_json::json!(true);
    assert_rejected_before_facade(
        &socket_path,
        &facade,
        &control_data_dir,
        unknown,
        "422 Unprocessable Entity",
    );
    let mut null_digest = valid_value.clone();
    null_digest["requestDigest"] = serde_json::Value::Null;
    assert_rejected_before_facade(
        &socket_path,
        &facade,
        &control_data_dir,
        null_digest,
        "422 Unprocessable Entity",
    );
    let mut crossed_digest = valid_value;
    let digest = crossed_digest["requestDigest"]
        .as_str()
        .expect("request digest should be canonical text");
    crossed_digest["requestDigest"] = serde_json::json!(crossed_text(digest));
    assert_rejected_before_facade(
        &socket_path,
        &facade,
        &control_data_dir,
        crossed_digest,
        "422 Unprocessable Entity",
    );

    for (provider_instance, generation) in [
        (
            "foreign-forwarder-instance",
            authority.generation().as_u64(),
        ),
        (
            "guest-teardown-forwarder-instance",
            authority.generation().as_u64() + 1,
        ),
    ] {
        let (crossed_authority, command) = teardown_wire_fixture_for_forwarder(
            WorkloadTeardownStep::DrainExecution,
            WorkloadTeardownCommandMode::Execute,
            provider_instance,
            generation,
        );
        let request = MachineApiWorkloadTeardownPhaseRequest::new(crossed_authority, command)
            .expect("crossed route fixture must remain a valid self-correlated request");
        assert_rejected_before_facade(
            &socket_path,
            &facade,
            &control_data_dir,
            serde_json::to_value(request).expect("crossed request should serialize"),
            "409 Conflict",
        );
    }

    let body = serde_json::to_string(&valid).expect("valid request should serialize");
    assert!(body.len() < MAX_WORKLOAD_TEARDOWN_REQUEST_BODY_BYTES);
    let exact_limit = format!(
        "{body}{}",
        " ".repeat(MAX_WORKLOAD_TEARDOWN_REQUEST_BODY_BYTES - body.len())
    );
    let before_exact_limit = facade.call_count();
    let exact_response = unix_http_post_json(
        &socket_path,
        MACHINE_API_WORKLOAD_TEARDOWN_PHASE_PATH,
        &exact_limit,
    );
    assert!(exact_response.contains("200 OK"), "{exact_response}");
    assert_eq!(facade.call_count(), before_exact_limit + 1);

    let before_oversized = facade.call_count();
    let oversized_response = unix_http_post_json(
        &socket_path,
        MACHINE_API_WORKLOAD_TEARDOWN_PHASE_PATH,
        &format!("{exact_limit} "),
    );
    assert!(
        oversized_response.contains("413 Payload Too Large"),
        "{oversized_response}"
    );
    assert_eq!(facade.call_count(), before_oversized);

    let before_method_checks = facade.call_count();
    let get_response = unix_http_get(&socket_path, MACHINE_API_WORKLOAD_TEARDOWN_PHASE_PATH);
    assert!(
        get_response.contains("405 Method Not Allowed"),
        "{get_response}"
    );
    let coarse_response =
        unix_http_post_json(&socket_path, "/v1/machine-api/workload-teardown", "{}");
    assert!(
        coarse_response.contains("404 Not Found"),
        "{coarse_response}"
    );
    assert_eq!(facade.call_count(), before_method_checks);

    facade.cross_next_response();
    let before_crossed_response = facade.call_count();
    let crossed_response = unix_http_post_json(
        &socket_path,
        MACHINE_API_WORKLOAD_TEARDOWN_PHASE_PATH,
        &body,
    );
    assert!(
        crossed_response.contains("500 Internal Server Error"),
        "{crossed_response}"
    );
    assert_eq!(facade.call_count(), before_crossed_response + 1);

    let _ = shutdown_tx.send(());
    server
        .await
        .expect("machine API server task should join")
        .expect("machine API server should shut down");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn machine_api_workload_teardown_route_requires_boot_authority_before_facade() {
    let request = exact_request(
        WorkloadTeardownStep::DrainExecution,
        WorkloadTeardownCommandMode::Execute,
    );
    let temp_dir = short_socket_tempdir();
    let socket_path = temp_dir.path().join("nimbus.sock");
    let control_data_dir = temp_dir.path().join("control");
    let listener = bind_direct_listener(&socket_path).expect("listener should bind");
    let facade = Arc::new(RecordingTeardownFacade::default());
    let state = MachineApiState {
        control_data_dir: control_data_dir.clone(),
        listen_mode: MachineApiListenMode::DirectSocket,
        binary_lookup_path: None,
        helper_binary_dirs: Vec::new(),
        service_workloads: Some(facade.clone()),
        machine_port_forwarder: None,
        forwarder_authority: None,
    };
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(serve_machine_api(listener, state, async move {
        let _ = shutdown_rx.await;
    }));
    wait_for_socket_path(&socket_path);

    let response = unix_http_post_json(
        &socket_path,
        MACHINE_API_WORKLOAD_TEARDOWN_PHASE_PATH,
        &serde_json::to_string(&request).expect("request should serialize"),
    );
    assert!(response.contains("503 Service Unavailable"), "{response}");
    assert_eq!(facade.call_count(), 0);
    assert!(!control_data_dir.exists());

    let _ = shutdown_tx.send(());
    server
        .await
        .expect("machine API server task should join")
        .expect("machine API server should shut down");
}

fn exact_request(
    step: WorkloadTeardownStep,
    mode: WorkloadTeardownCommandMode,
) -> MachineApiWorkloadTeardownPhaseRequest {
    let (authority, command) = teardown_wire_fixture(step, mode);
    MachineApiWorkloadTeardownPhaseRequest::new(authority, command)
        .expect("exact teardown request should validate")
}

fn post_teardown(
    socket_path: &std::path::Path,
    request: &MachineApiWorkloadTeardownPhaseRequest,
) -> MachineApiWorkloadTeardownPhaseResponse {
    let body = serde_json::to_string(request).expect("teardown request should serialize");
    let response =
        unix_http_post_json(socket_path, MACHINE_API_WORKLOAD_TEARDOWN_PHASE_PATH, &body);
    assert!(response.contains("200 OK"), "{response}");
    let (_, body) = response
        .split_once("\r\n\r\n")
        .expect("HTTP response should contain a body separator");
    serde_json::from_str(body).expect("teardown response should be typed JSON")
}

fn assert_rejected_before_facade(
    socket_path: &std::path::Path,
    facade: &RecordingTeardownFacade,
    control_data_dir: &std::path::Path,
    request: serde_json::Value,
    status: &str,
) {
    let calls_before = facade.call_count();
    let effects_before = facade.provider_effects.load(Ordering::SeqCst);
    let response = unix_http_post_json(
        socket_path,
        MACHINE_API_WORKLOAD_TEARDOWN_PHASE_PATH,
        &serde_json::to_string(&request).expect("test request should serialize"),
    );
    assert!(response.contains(status), "{response}");
    assert_eq!(facade.call_count(), calls_before);
    assert_eq!(
        facade.provider_effects.load(Ordering::SeqCst),
        effects_before
    );
    assert!(
        !control_data_dir.exists(),
        "wire and authority rejection must precede guest artifact creation"
    );
}

fn crossed_text(exact: &str) -> String {
    let mut crossed = exact.as_bytes().to_vec();
    let last = crossed
        .last_mut()
        .expect("derived identity should not be empty");
    *last = if *last == b'a' { b'b' } else { b'a' };
    String::from_utf8(crossed).expect("crossed identity should remain UTF-8")
}
