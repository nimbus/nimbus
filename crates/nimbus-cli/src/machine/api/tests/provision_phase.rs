use std::sync::atomic::{AtomicUsize, Ordering};

use nimbus_machine::api::{
    MACHINE_API_WORKLOAD_PROVISION_PHASE_PATH, MachineApiWorkloadProvisionObservation,
};
use nimbus_workloads::{WorkloadProvisionCommandMode, WorkloadProvisionStep};

use super::super::service_workloads::MachineApiServiceFuture;
use super::super::service_workloads::provision::tests::request_fixture;
use super::*;
use crate::machine::client::MachineApiClient;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn machine_api_and_guest_node_use_fenced_commands() {
    let temp_dir = short_socket_tempdir();
    let socket_path = temp_dir.path().join("nimbus.sock");
    let listener = bind_direct_listener(&socket_path).expect("listener should bind");
    let prepare = request_fixture(
        'a',
        WorkloadProvisionStep::PrepareWorkload,
        WorkloadProvisionCommandMode::Execute,
    );
    let current_authority = prepare.forwarder_authority().clone();
    let facade = Arc::new(RecordingProvisionFacade::default());
    let state = MachineApiState {
        control_data_dir: temp_dir.path().join("control"),
        listen_mode: MachineApiListenMode::DirectSocket,
        binary_lookup_path: None,
        helper_binary_dirs: Vec::new(),
        service_workloads: Some(facade.clone()),
        machine_port_forwarder: None,
        forwarder_authority: Some(current_authority.clone()),
    };
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(serve_machine_api(listener, state, async move {
        let _ = shutdown_rx.await;
    }));
    wait_for_socket_path(&socket_path);

    let mut unknown = serde_json::to_value(&prepare).expect("request should serialize");
    unknown["unexpected"] = serde_json::json!(true);
    let response = unix_http_post_json(
        &socket_path,
        MACHINE_API_WORKLOAD_PROVISION_PHASE_PATH,
        &unknown.to_string(),
    );
    assert!(response.contains("422 Unprocessable Entity"), "{response}");
    assert_eq!(facade.calls(), 0);

    let mut crossed = serde_json::to_value(&prepare).expect("request should serialize");
    let other = serde_json::to_value(request_fixture(
        'b',
        WorkloadProvisionStep::PrepareWorkload,
        WorkloadProvisionCommandMode::Execute,
    ))
    .expect("other request should serialize");
    crossed["command"]["desired_digest"] = other["command"]["desired_digest"].clone();
    let response = unix_http_post_json(
        &socket_path,
        MACHINE_API_WORKLOAD_PROVISION_PHASE_PATH,
        &crossed.to_string(),
    );
    assert!(response.contains("422 Unprocessable Entity"), "{response}");
    assert_eq!(facade.calls(), 0);

    let stale = request_fixture(
        'b',
        WorkloadProvisionStep::PrepareWorkload,
        WorkloadProvisionCommandMode::Execute,
    );
    let response = unix_http_post_json(
        &socket_path,
        MACHINE_API_WORKLOAD_PROVISION_PHASE_PATH,
        &serde_json::to_string(&stale).expect("stale request should serialize"),
    );
    assert!(response.contains("409 Conflict"), "{response}");
    assert_eq!(facade.calls(), 0);
    assert!(
        !temp_dir.path().join("control").exists(),
        "wire and authority rejection must precede guest artifact creation"
    );

    let steps = [
        (
            WorkloadProvisionStep::ReserveNetwork,
            WorkloadProvisionCommandMode::Execute,
        ),
        (
            WorkloadProvisionStep::PrepareWorkload,
            WorkloadProvisionCommandMode::Execute,
        ),
        (
            WorkloadProvisionStep::AttachNetwork,
            WorkloadProvisionCommandMode::Execute,
        ),
        (
            WorkloadProvisionStep::InspectActivationPrerequisites,
            WorkloadProvisionCommandMode::Inspect,
        ),
        (
            WorkloadProvisionStep::ActivateWorkload,
            WorkloadProvisionCommandMode::Execute,
        ),
        (
            WorkloadProvisionStep::InspectWorkloadReadiness,
            WorkloadProvisionCommandMode::Inspect,
        ),
        (
            WorkloadProvisionStep::Publish,
            WorkloadProvisionCommandMode::Execute,
        ),
        (
            WorkloadProvisionStep::ObservePublication,
            WorkloadProvisionCommandMode::Inspect,
        ),
    ];
    for (index, (step, mode)) in steps.into_iter().enumerate() {
        let request = request_fixture('a', step, mode);
        if index == 1 {
            let client = MachineApiClient::new_for_test(&socket_path)
                .with_forwarder_authority(current_authority.clone());
            let response = client
                .provision_workload_phase(request.command().clone())
                .expect("strict client should validate the correlated response");
            assert_eq!(response.forwarder_authority(), &current_authority);
            assert_eq!(response.command_id(), request.command().command_id());
            assert_eq!(response.attempt_id(), request.command().attempt_id());
            assert_eq!(
                response.dispatch_epoch(),
                request.command().dispatch_epoch()
            );
            assert_eq!(
                response.provider_target(),
                request.command().provider_target()
            );
        } else {
            let response = unix_http_post_json(
                &socket_path,
                MACHINE_API_WORKLOAD_PROVISION_PHASE_PATH,
                &serde_json::to_string(&request).expect("request should serialize"),
            );
            assert!(response.contains("200 OK"), "{step:?}: {response}");
        }
    }
    assert_eq!(facade.calls(), 8);
    assert_eq!(facade.steps(), steps.into_iter().collect::<Vec<_>>());

    let _ = shutdown_tx.send(());
    server
        .await
        .expect("machine API server task should join")
        .expect("machine API server should shut down");
}

#[derive(Default)]
struct RecordingProvisionFacade {
    calls: AtomicUsize,
    steps: Mutex<Vec<(WorkloadProvisionStep, WorkloadProvisionCommandMode)>>,
}

impl RecordingProvisionFacade {
    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    fn steps(&self) -> Vec<(WorkloadProvisionStep, WorkloadProvisionCommandMode)> {
        self.steps.lock().expect("steps lock").clone()
    }
}

impl MachineApiNodeWorkloadFacade for RecordingProvisionFacade {
    fn kind(&self) -> SandboxBackendKind {
        SandboxBackendKind::Container
    }

    fn inspect<'a>(
        &'a self,
        _id: &'a SandboxId,
    ) -> MachineApiServiceFuture<'a, Option<nimbus_sandbox::SandboxInspection>> {
        Box::pin(async { Err(unexpected_legacy_call("inspect")) })
    }

    fn stop<'a>(&'a self, _id: &'a SandboxId) -> MachineApiServiceFuture<'a, ()> {
        Box::pin(async { Err(unexpected_legacy_call("stop")) })
    }

    fn absent_machine_port_receipts<'a>(
        &'a self,
        _id: &'a SandboxId,
    ) -> MachineApiServiceFuture<
        'a,
        Option<nimbus_sandbox::backends::container::MachinePortAbsenceEvidence>,
    > {
        Box::pin(async { Err(unexpected_legacy_call("absence inspect")) })
    }

    fn provision_phase<'a>(
        &'a self,
        command: &'a nimbus_machine::api::MachineApiWorkloadProvisionCommandEnvelope,
        _forwarder_authority: &'a MachineForwarderAuthority,
    ) -> MachineApiServiceFuture<'a, MachineApiWorkloadProvisionObservation> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.steps
            .lock()
            .expect("steps lock")
            .push((command.claim().attempt().step(), command.mode()));
        Box::pin(async {
            Ok(MachineApiWorkloadProvisionObservation::Succeeded {
                evidence: b"recorded-exact-phase".to_vec(),
            })
        })
    }
}

fn unexpected_legacy_call(operation: &str) -> MachineApiHttpError {
    MachineApiHttpError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        message: format!("strict provision test unexpectedly invoked legacy {operation}"),
    }
}
