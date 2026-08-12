//! End-to-end route proofs for the exact guest restart-phase transport.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};

use nimbus_compute::workload_saga::{ConfirmedWorkloadRestartCommand, WorkloadRestartCommandMode};
use nimbus_machine::api::{
    MACHINE_API_WORKLOAD_RESTART_PHASE_PATH, MachineApiWorkloadRestartCommandEnvelope,
    MachineApiWorkloadRestartCommandMode, MachineApiWorkloadRestartObservation,
    MachineApiWorkloadRestartPhaseRequest, MachineApiWorkloadRestartPhaseResponse,
};
use nimbus_workloads::{WorkloadRestartEvidenceDigest, WorkloadRestartStep};

use super::*;

#[derive(Default)]
struct RecordingRestartFacade {
    calls: Mutex<Vec<MachineApiWorkloadRestartCommandEnvelope>>,
    journal: Mutex<BTreeMap<String, WorkloadRestartEvidenceDigest>>,
    provider_effects: AtomicUsize,
}

impl RecordingRestartFacade {
    fn call_count(&self) -> usize {
        self.calls.lock().expect("call log should be healthy").len()
    }

    fn journal_count(&self) -> usize {
        self.journal
            .lock()
            .expect("restart journal should be healthy")
            .len()
    }

    fn provider_effect_count(&self) -> usize {
        self.provider_effects.load(Ordering::SeqCst)
    }
}

impl MachineApiNodeWorkloadFacade for RecordingRestartFacade {
    fn kind(&self) -> SandboxBackendKind {
        SandboxBackendKind::Container
    }

    fn restart_execution_blockers(&self) -> Vec<String> {
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
                message: "restart route fixture does not expose coarse inspection".to_owned(),
            })
        })
    }

    fn restart_phase<'a>(
        &'a self,
        command: &'a MachineApiWorkloadRestartCommandEnvelope,
    ) -> super::super::service_workloads::MachineApiServiceFuture<
        'a,
        MachineApiWorkloadRestartObservation,
    > {
        let command = command.clone();
        Box::pin(async move {
            self.calls
                .lock()
                .expect("call log should be healthy")
                .push(command.clone());
            let command_id = command.command_id().as_str().to_owned();
            let evidence = {
                let mut journal = self
                    .journal
                    .lock()
                    .expect("restart journal should be healthy");
                *journal.entry(command_id.clone()).or_insert_with(|| {
                    self.provider_effects.fetch_add(1, Ordering::SeqCst);
                    WorkloadRestartEvidenceDigest::sha256(format!(
                        "guest-route-provider-effect-{command_id}"
                    ))
                })
            };
            Ok(MachineApiWorkloadRestartObservation::Succeeded { evidence })
        })
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn exact_restart_route_accepts_both_triggers_and_rejects_crossed_fences_before_effects() {
    let authority = crate::machine::backend::provision::forwarder_authority_for_test();
    let automatic = restart_request(
        crate::machine::backend::provision::confirmed_automatic_restart_command_for_test(
            WorkloadRestartStep::QuiesceExecution,
            false,
        )
        .await,
        &authority,
    );
    let explicit = restart_request(
        crate::machine::backend::provision::confirmed_restart_command_for_test(
            "guest-route-explicit",
            WorkloadRestartStep::QuiesceExecution,
            false,
        )
        .await,
        &authority,
    );
    assert!(automatic.command().inspection_version().is_some());
    assert_eq!(explicit.command().inspection_version(), None);
    assert_same_envelope_shape(&automatic, &explicit);

    let temp_dir = short_socket_tempdir();
    let socket_path = temp_dir.path().join("nimbus.sock");
    let control_data_dir = temp_dir.path().join("control");
    let listener = bind_direct_listener(&socket_path).expect("listener should bind");
    let facade = Arc::new(RecordingRestartFacade::default());
    let state = MachineApiState {
        control_data_dir: control_data_dir.clone(),
        listen_mode: MachineApiListenMode::DirectSocket,
        binary_lookup_path: None,
        helper_binary_dirs: Vec::new(),
        service_workloads: Some(facade.clone()),
        machine_port_forwarder: None,
        forwarder_authority: Some(authority),
    };
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(serve_machine_api(listener, state, async move {
        let _ = shutdown_rx.await;
    }));
    wait_for_socket_path(&socket_path);

    for (index, request) in [&automatic, &explicit].into_iter().enumerate() {
        let effects_before = facade.provider_effect_count();
        for _ in 0..2 {
            let response = post_restart(&socket_path, request);
            response
                .validate_for_request(request)
                .expect("typed response must correlate to every restart fence");
            assert!(matches!(
                response.observation(),
                MachineApiWorkloadRestartObservation::Succeeded { .. }
            ));
        }
        assert_eq!(
            facade.provider_effect_count(),
            effects_before + 1,
            "exact replay for case {index} must adopt one journaled provider effect"
        );
        assert_eq!(facade.journal_count(), index + 1);
    }
    assert_eq!(facade.call_count(), 4, "both exact replays reach the sink");

    let valid = serde_json::to_value(&automatic).expect("restart request should serialize");
    let mut crossed_attempt = valid.clone();
    let attempt = crossed_attempt["command"]["attempt_id"]
        .as_str()
        .expect("target attempt should be text")
        .to_owned();
    crossed_attempt["command"]["attempt_id"] = serde_json::json!(crossed_derived_id(&attempt));
    assert_rejected_before_effects(&socket_path, &facade, &control_data_dir, crossed_attempt);

    let mut crossed_epoch = valid;
    let restart_epoch = crossed_epoch["command"]["restart_epoch"]
        .as_str()
        .expect("restart epoch should use canonical decimal text")
        .parse::<u64>()
        .expect("restart epoch should be numeric");
    crossed_epoch["command"]["restart_epoch"] = serde_json::json!((restart_epoch + 1).to_string());
    assert_rejected_before_effects(&socket_path, &facade, &control_data_dir, crossed_epoch);

    let coarse = unix_http_post_json(
        &socket_path,
        "/v1/machine-api/service-sandboxes/restart",
        "{}",
    );
    assert!(coarse.contains("405 Method Not Allowed"), "{coarse}");
    assert_eq!(
        facade.call_count(),
        4,
        "no coarse restart peer may reach the sink"
    );
    assert_eq!(facade.provider_effect_count(), 2);
    assert_eq!(facade.journal_count(), 2);
    assert!(
        !control_data_dir.exists(),
        "crossed and coarse requests must not create guest artifacts"
    );

    let _ = shutdown_tx.send(());
    server
        .await
        .expect("machine API server task should join")
        .expect("machine API server should shut down");
}

fn restart_request(
    command: ConfirmedWorkloadRestartCommand,
    authority: &nimbus_machine::MachineForwarderAuthority,
) -> MachineApiWorkloadRestartPhaseRequest {
    let mode = match command.mode() {
        WorkloadRestartCommandMode::Execute => MachineApiWorkloadRestartCommandMode::Execute,
        WorkloadRestartCommandMode::Inspect => MachineApiWorkloadRestartCommandMode::Inspect,
    };
    let envelope = MachineApiWorkloadRestartCommandEnvelope::new(
        command.command_id().clone(),
        command.key().clone(),
        command.saga_id().clone(),
        command.transition_id().clone(),
        command.generation(),
        command.desired_digest(),
        command.source().clone(),
        command.source_execution().clone(),
        command.execution().clone(),
        command.source_attempt_id().clone(),
        command.attempt_id().clone(),
        command.restart_epoch(),
        command.dispatch_epoch(),
        command.request_id().clone(),
        command.issuing_revision(),
        command.confirmed_revision(),
        command.inspection_version(),
        command.provider_selection().clone(),
        command.step(),
        mode,
        command.successor_veto_generation(),
        command.claim().clone(),
        command.executable().clone(),
        command.network_plan_digest(),
        command.compiled_network_plan().clone(),
        authority.clone(),
        authority.generation(),
    )
    .expect("compute-confirmed restart command should lower without changing a fence");
    MachineApiWorkloadRestartPhaseRequest::new(authority.clone(), envelope)
        .expect("exact restart request should authenticate")
}

fn assert_same_envelope_shape(
    automatic: &MachineApiWorkloadRestartPhaseRequest,
    explicit: &MachineApiWorkloadRestartPhaseRequest,
) {
    let automatic = serde_json::to_value(automatic).expect("automatic request should serialize");
    let explicit = serde_json::to_value(explicit).expect("explicit request should serialize");
    let automatic_command = automatic["command"]
        .as_object()
        .expect("automatic command should be an object");
    let explicit_command = explicit["command"]
        .as_object()
        .expect("explicit command should be an object");
    assert_eq!(
        automatic_command.keys().collect::<Vec<_>>(),
        explicit_command.keys().collect::<Vec<_>>(),
        "automatic and explicit triggers must use one wire envelope shape"
    );
    assert!(!automatic_command["inspection_version"].is_null());
    assert!(
        explicit_command.contains_key("inspection_version"),
        "explicit restart must carry the required-on-wire field"
    );
    assert!(explicit_command["inspection_version"].is_null());
}

fn post_restart(
    socket_path: &std::path::Path,
    request: &MachineApiWorkloadRestartPhaseRequest,
) -> MachineApiWorkloadRestartPhaseResponse {
    let body = serde_json::to_string(request).expect("restart request should serialize");
    let response = unix_http_post_json(socket_path, MACHINE_API_WORKLOAD_RESTART_PHASE_PATH, &body);
    assert!(response.contains("200 OK"), "{response}");
    let (_, body) = response
        .split_once("\r\n\r\n")
        .expect("HTTP response should contain a body separator");
    serde_json::from_str(body).expect("restart response should be typed JSON")
}

fn assert_rejected_before_effects(
    socket_path: &std::path::Path,
    facade: &RecordingRestartFacade,
    control_data_dir: &std::path::Path,
    crossed: serde_json::Value,
) {
    let calls_before = facade.call_count();
    let effects_before = facade.provider_effect_count();
    let journal_before = facade.journal_count();
    let response = unix_http_post_json(
        socket_path,
        MACHINE_API_WORKLOAD_RESTART_PHASE_PATH,
        &serde_json::to_string(&crossed).expect("crossed request should serialize"),
    );
    assert!(response.contains("422 Unprocessable Entity"), "{response}");
    assert_eq!(facade.call_count(), calls_before);
    assert_eq!(facade.provider_effect_count(), effects_before);
    assert_eq!(facade.journal_count(), journal_before);
    assert!(
        !control_data_dir.exists(),
        "crossed wire rejection must precede journal and provider artifacts"
    );
}

fn crossed_derived_id(exact: &str) -> String {
    let mut crossed = exact.as_bytes().to_vec();
    let last = crossed
        .last_mut()
        .expect("derived restart identity should not be empty");
    *last = if *last == b'a' { b'b' } else { b'a' };
    String::from_utf8(crossed).expect("crossed identity should remain UTF-8")
}
