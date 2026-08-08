//! Retirement evidence and parent-selected identity boundaries.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use super::super::service_workloads::MachineApiServiceFuture;
use super::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn machine_api_stop_refuses_to_precompute_absence_from_desired_bindings() {
    let temp_dir = short_socket_tempdir();
    let control_data_dir = temp_dir.path().join("control");
    let tenant_id = TenantId::new("svc-demo").expect("tenant id should be valid");
    let sandbox_id = SandboxId::new(format!(
        "machine-api:{}",
        NetworkPlanId::for_tenant_workload_plan(&tenant_id, "receiptless-stop")
    ));
    let handle = SandboxHandle::new(
        tenant_id.clone(),
        sandbox_id.clone(),
        "api",
        SandboxBackendKind::Container,
        SandboxStatus::Ready,
        Vec::new(),
    );
    write_container_manifest(
        &machine_container_state_root(&control_data_dir),
        sandbox_id.as_str(),
        tenant_id.as_str(),
        "api",
        SandboxStatus::Ready,
        Vec::new(),
    );
    let socket_path = temp_dir.path().join("nimbus.sock");
    let listener = bind_direct_listener(&socket_path).expect("listener should bind");
    let forwarder_authority = test_forwarder_authority("receiptless-stop-forwarder");
    let state = MachineApiState {
        control_data_dir,
        listen_mode: MachineApiListenMode::DirectSocket,
        binary_lookup_path: Some(fake_runtime_path(&temp_dir)),
        helper_binary_dirs: Vec::new(),
        service_workloads: Some(Arc::new(ReceiptlessNodeWorkloadFacade::with_handle(handle))),
        machine_port_forwarder: None,
        forwarder_authority: Some(forwarder_authority.clone()),
    };
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(serve_machine_api(listener, state, async move {
        let _ = shutdown_rx.await;
    }));
    wait_for_socket_path(&socket_path);

    let response = unix_http_post_json(
        &socket_path,
        &format!("/v1/machine-api/service-sandboxes/{sandbox_id}/stop"),
        &serde_json::to_string(&MachineApiServiceSandboxStopRequest {
            forwarder_authority,
        })
        .expect("stop request should serialize"),
    );
    assert!(response.contains("500 Internal Server Error"), "{response}");
    assert!(!response.contains("\"confirmed_absent_evidence\""));

    let _ = shutdown_tx.send(());
    server
        .await
        .expect("server task should join")
        .expect("server should stop");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn missing_sandbox_stop_retry_returns_exact_durable_absence_and_unrelated_ids_stay_missing() {
    let temp_dir = short_socket_tempdir();
    let socket_path = temp_dir.path().join("nimbus.sock");
    let listener = bind_direct_listener(&socket_path).expect("listener should bind");
    let tenant_id = TenantId::new("svc-retry").expect("tenant id should be valid");
    let sandbox_id = SandboxId::new(format!(
        "machine-api:{}",
        NetworkPlanId::for_tenant_workload_plan(&tenant_id, "lost-stop-response")
    ));
    let authority = test_forwarder_authority("retry-forwarder");
    let receipt = nimbus_sandbox::MachinePortForwardReceipt {
        outcome: nimbus_sandbox::MachinePortForwardOutcome::ExactAlreadyAbsent,
        tenant_id: tenant_id.clone(),
        sandbox_id: sandbox_id.clone(),
        binding: SandboxPortBinding::tcp("http", 18_080, 8_080),
        provider_instance: authority.provider_instance().clone(),
        provider_generation: authority.generation(),
    };
    let workloads = Arc::new(AbsentRetryNodeWorkloadFacade::new(
        nimbus_sandbox::backends::container::MachinePortAbsenceEvidence {
            tenant_id: tenant_id.clone(),
            sandbox_id: sandbox_id.clone(),
            receipts: vec![receipt],
        },
    ));
    let state = MachineApiState {
        control_data_dir: temp_dir.path().join("control"),
        listen_mode: MachineApiListenMode::DirectSocket,
        binary_lookup_path: Some(fake_runtime_path(&temp_dir)),
        helper_binary_dirs: Vec::new(),
        service_workloads: Some(workloads.clone()),
        machine_port_forwarder: None,
        forwarder_authority: Some(authority.clone()),
    };
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(serve_machine_api(listener, state, async move {
        let _ = shutdown_rx.await;
    }));
    wait_for_socket_path(&socket_path);

    let body = serde_json::to_string(&MachineApiServiceSandboxStopRequest {
        forwarder_authority: authority,
    })
    .expect("stop request should serialize");
    let response = unix_http_post_json(
        &socket_path,
        &format!("/v1/machine-api/service-sandboxes/{sandbox_id}/stop"),
        &body,
    );
    assert!(response.contains("200 OK"), "{response}");
    assert!(response.contains("\"outcome\":\"exact_already_absent\""));
    assert!(response.contains(&format!("\"tenant_id\":\"{tenant_id}\"")));
    assert_eq!(workloads.stop_calls.load(Ordering::SeqCst), 0);

    let unrelated = SandboxId::new(format!(
        "machine-api:{}",
        NetworkPlanId::for_tenant_workload_plan(&tenant_id, "unrelated")
    ));
    let unrelated_response = unix_http_post_json(
        &socket_path,
        &format!("/v1/machine-api/service-sandboxes/{unrelated}/stop"),
        &body,
    );
    assert!(unrelated_response.contains("404 Not Found"));

    let _ = shutdown_tx.send(());
    server
        .await
        .expect("server task should join")
        .expect("server should stop");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn missing_zero_binding_sandbox_retry_preserves_authenticated_header_identity() {
    let temp_dir = short_socket_tempdir();
    let socket_path = temp_dir.path().join("nimbus.sock");
    let listener = bind_direct_listener(&socket_path).expect("listener should bind");
    let tenant_id = TenantId::new("svc-empty-retry").expect("tenant id should be valid");
    let sandbox_id = SandboxId::new(format!(
        "machine-api:{}",
        NetworkPlanId::for_tenant_workload_plan(&tenant_id, "lost-empty-stop-response")
    ));
    let authority = test_forwarder_authority("empty-retry-forwarder");
    let workloads = Arc::new(AbsentRetryNodeWorkloadFacade::new(
        nimbus_sandbox::backends::container::MachinePortAbsenceEvidence {
            tenant_id: tenant_id.clone(),
            sandbox_id: sandbox_id.clone(),
            receipts: Vec::new(),
        },
    ));
    let state = MachineApiState {
        control_data_dir: temp_dir.path().join("control"),
        listen_mode: MachineApiListenMode::DirectSocket,
        binary_lookup_path: Some(fake_runtime_path(&temp_dir)),
        helper_binary_dirs: Vec::new(),
        service_workloads: Some(workloads),
        machine_port_forwarder: None,
        forwarder_authority: Some(authority.clone()),
    };
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(serve_machine_api(listener, state, async move {
        let _ = shutdown_rx.await;
    }));
    wait_for_socket_path(&socket_path);

    let response = unix_http_post_json(
        &socket_path,
        &format!("/v1/machine-api/service-sandboxes/{sandbox_id}/stop"),
        &serde_json::to_string(&MachineApiServiceSandboxStopRequest {
            forwarder_authority: authority,
        })
        .expect("stop request should serialize"),
    );
    assert!(response.contains("200 OK"), "{response}");
    assert!(response.contains(&format!("\"tenant_id\":\"{tenant_id}\"")));
    assert!(response.contains("\"confirmed_absent_evidence\":[]"));

    let _ = shutdown_tx.send(());
    server
        .await
        .expect("server task should join")
        .expect("server should stop");
}

struct AbsentRetryNodeWorkloadFacade {
    evidence: nimbus_sandbox::backends::container::MachinePortAbsenceEvidence,
    stop_calls: AtomicUsize,
}

impl AbsentRetryNodeWorkloadFacade {
    fn new(evidence: nimbus_sandbox::backends::container::MachinePortAbsenceEvidence) -> Self {
        Self {
            evidence,
            stop_calls: AtomicUsize::new(0),
        }
    }
}

impl MachineApiNodeWorkloadFacade for AbsentRetryNodeWorkloadFacade {
    fn kind(&self) -> SandboxBackendKind {
        SandboxBackendKind::Container
    }

    fn inspect<'a>(
        &'a self,
        _id: &'a SandboxId,
    ) -> MachineApiServiceFuture<'a, Option<nimbus_sandbox::SandboxInspection>> {
        Box::pin(async { Ok(None) })
    }

    fn stop<'a>(&'a self, _id: &'a SandboxId) -> MachineApiServiceFuture<'a, ()> {
        Box::pin(async move {
            self.stop_calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
    }

    fn absent_machine_port_receipts<'a>(
        &'a self,
        id: &'a SandboxId,
    ) -> MachineApiServiceFuture<
        'a,
        Option<nimbus_sandbox::backends::container::MachinePortAbsenceEvidence>,
    > {
        Box::pin(
            async move { Ok((self.evidence.sandbox_id == *id).then(|| self.evidence.clone())) },
        )
    }
}

struct ReceiptlessNodeWorkloadFacade {
    handle: Mutex<Option<SandboxHandle>>,
}

impl ReceiptlessNodeWorkloadFacade {
    fn with_handle(handle: SandboxHandle) -> Self {
        Self {
            handle: Mutex::new(Some(handle)),
        }
    }
}

impl MachineApiNodeWorkloadFacade for ReceiptlessNodeWorkloadFacade {
    fn kind(&self) -> SandboxBackendKind {
        SandboxBackendKind::Container
    }

    fn inspect<'a>(
        &'a self,
        id: &'a SandboxId,
    ) -> MachineApiServiceFuture<'a, Option<nimbus_sandbox::SandboxInspection>> {
        Box::pin(async move {
            Ok(self
                .handle
                .lock()
                .expect("receiptless handle lock")
                .clone()
                .filter(|handle| &handle.id == id)
                .map(nimbus_sandbox::SandboxInspection::provider_reported))
        })
    }

    fn stop<'a>(&'a self, _id: &'a SandboxId) -> MachineApiServiceFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }

    fn absent_machine_port_receipts<'a>(
        &'a self,
        _id: &'a SandboxId,
    ) -> MachineApiServiceFuture<
        'a,
        Option<nimbus_sandbox::backends::container::MachinePortAbsenceEvidence>,
    > {
        Box::pin(async {
            Err(MachineApiHttpError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: "no absent provider receipt was recorded".to_owned(),
            })
        })
    }
}
