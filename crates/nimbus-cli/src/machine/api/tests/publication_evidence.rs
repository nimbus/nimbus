//! Machine API publication evidence and parent-selected identity boundaries.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use super::super::service_workloads::MachineApiServiceFuture;
use super::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn machine_api_start_refuses_to_echo_requested_bindings_as_provider_evidence() {
    let temp_dir = short_socket_tempdir();
    let socket_path = temp_dir.path().join("nimbus.sock");
    let listener = bind_direct_listener(&socket_path).expect("listener should bind");
    let forwarder_authority = test_forwarder_authority("receiptless-start-forwarder");
    let state = MachineApiState {
        control_data_dir: temp_dir.path().join("control"),
        listen_mode: MachineApiListenMode::DirectSocket,
        binary_lookup_path: Some(fake_runtime_path(&temp_dir)),
        helper_binary_dirs: Vec::new(),
        service_workloads: Some(Arc::new(ReceiptlessNodeWorkloadFacade::default())),
        machine_port_forwarder: None,
        forwarder_authority: Some(forwarder_authority.clone()),
    };
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(serve_machine_api(listener, state, async move {
        let _ = shutdown_rx.await;
    }));
    wait_for_socket_path(&socket_path);

    let tenant_id = TenantId::new("svc-demo").expect("tenant id should be valid");
    let sandbox_id = SandboxId::new(format!(
        "machine-api:{}",
        NetworkPlanId::for_tenant_workload_plan(&tenant_id, "receiptless-start")
    ));
    let request = MachineApiServiceSandboxImageStartRequest {
        sandbox_id,
        forwarder_authority,
        spec: machine_api_image_spec(&tenant_id, "api"),
    };
    let response = unix_http_post_json(
        &socket_path,
        "/v1/machine-api/service-sandboxes/image-start",
        &serde_json::to_string(&request).expect("start request should serialize"),
    );

    assert!(
        response.contains("500 Internal Server Error"),
        "a ready workload without durable provider receipts must not produce start success: \
         {response}"
    );
    assert!(
        !response.contains("\"publication_evidence\""),
        "the requested binding vector is desired state, not provider evidence: {response}"
    );

    let _ = shutdown_tx.send(());
    server
        .await
        .expect("machine API server task should join")
        .expect("machine API server should shut down cleanly");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn machine_api_rejects_non_network_plan_sandbox_identity_before_guest_effects() {
    let temp_dir = short_socket_tempdir();
    let socket_path = temp_dir.path().join("nimbus.sock");
    let listener = bind_direct_listener(&socket_path).expect("listener should bind");
    let forwarder_authority = test_forwarder_authority("invalid-plan-id-forwarder");
    let workloads = Arc::new(ReceiptlessNodeWorkloadFacade::default());
    let state = MachineApiState {
        control_data_dir: temp_dir.path().join("control"),
        listen_mode: MachineApiListenMode::DirectSocket,
        binary_lookup_path: Some(fake_runtime_path(&temp_dir)),
        helper_binary_dirs: Vec::new(),
        service_workloads: Some(workloads.clone()),
        machine_port_forwarder: None,
        forwarder_authority: Some(forwarder_authority.clone()),
    };
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(serve_machine_api(listener, state, async move {
        let _ = shutdown_rx.await;
    }));
    wait_for_socket_path(&socket_path);

    let tenant_id = TenantId::new("svc-demo").expect("tenant id should be valid");
    let request = MachineApiServiceSandboxImageStartRequest {
        sandbox_id: SandboxId::new("machine-api:not-a-network-plan"),
        forwarder_authority,
        spec: machine_api_image_spec(&tenant_id, "api"),
    };
    let response = unix_http_post_json(
        &socket_path,
        "/v1/machine-api/service-sandboxes/image-start",
        &serde_json::to_string(&request).expect("start request should serialize"),
    );

    assert!(response.contains("400 Bad Request"), "{response}");
    assert!(
        response.contains("valid parent-issued NetworkPlanId"),
        "{response}"
    );
    assert!(
        !workloads.has_handle(),
        "invalid parent identity must be rejected before workload materialization"
    );
    assert!(
        !temp_dir.path().join("control").exists(),
        "invalid identity admission must not create guest network or workload artifacts"
    );

    let _ = shutdown_tx.send(());
    server
        .await
        .expect("machine API server task should join")
        .expect("machine API server should shut down cleanly");
}

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

    let request = MachineApiServiceSandboxStopRequest {
        forwarder_authority,
    };
    let response = unix_http_post_json(
        &socket_path,
        &format!("/v1/machine-api/service-sandboxes/{sandbox_id}/stop"),
        &serde_json::to_string(&request).expect("stop request should serialize"),
    );

    assert!(
        response.contains("500 Internal Server Error"),
        "a stopped workload without durable provider absence receipts must remain fenced: \
         {response}"
    );
    assert!(
        !response.contains("\"confirmed_absent_evidence\""),
        "desired manifest bindings cannot be upgraded into observed absence: {response}"
    );

    let _ = shutdown_tx.send(());
    server
        .await
        .expect("machine API server task should join")
        .expect("machine API server should shut down cleanly");
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
    let binding = SandboxPortBinding::tcp("http", 18_080, 8_080);
    let receipt = nimbus_sandbox::MachinePortForwardReceipt {
        outcome: nimbus_sandbox::MachinePortForwardOutcome::ExactAlreadyAbsent,
        tenant_id: tenant_id.clone(),
        sandbox_id: sandbox_id.clone(),
        binding,
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

    let request = MachineApiServiceSandboxStopRequest {
        forwarder_authority: authority,
    };
    let body = serde_json::to_string(&request).expect("stop request should serialize");
    let response = unix_http_post_json(
        &socket_path,
        &format!("/v1/machine-api/service-sandboxes/{sandbox_id}/stop"),
        &body,
    );
    assert!(response.contains("200 OK"), "{response}");
    assert!(
        response.contains("\"outcome\":\"exact_already_absent\""),
        "{response}"
    );
    assert!(
        response.contains(&format!("\"tenant_id\":\"{tenant_id}\"")),
        "{response}"
    );
    assert_eq!(
        workloads.stop_calls.load(Ordering::SeqCst),
        0,
        "a missing workload retry must read durable absence without repeating stop effects"
    );

    let unrelated = SandboxId::new(format!(
        "machine-api:{}",
        NetworkPlanId::for_tenant_workload_plan(&tenant_id, "unrelated")
    ));
    let unrelated_response = unix_http_post_json(
        &socket_path,
        &format!("/v1/machine-api/service-sandboxes/{unrelated}/stop"),
        &body,
    );
    assert!(
        unrelated_response.contains("404 Not Found"),
        "{unrelated_response}"
    );

    let _ = shutdown_tx.send(());
    server
        .await
        .expect("machine API server task should join")
        .expect("machine API server should shut down cleanly");
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
    assert!(
        response.contains(&format!("\"tenant_id\":\"{tenant_id}\"")),
        "{response}"
    );
    assert!(
        response.contains("\"confirmed_absent_evidence\":[]"),
        "{response}"
    );

    let _ = shutdown_tx.send(());
    server
        .await
        .expect("machine API server task should join")
        .expect("machine API server should shut down cleanly");
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

    fn start<'a>(
        &'a self,
        _sandbox_id: SandboxId,
        _spec: SandboxSpec,
    ) -> MachineApiServiceFuture<'a, SandboxHandle> {
        Box::pin(async move {
            Err(MachineApiHttpError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: "absence retry fixture does not start workloads".to_owned(),
            })
        })
    }

    fn inspect<'a>(
        &'a self,
        _id: &'a SandboxId,
    ) -> MachineApiServiceFuture<'a, Option<SandboxHandle>> {
        Box::pin(async move { Ok(None) })
    }

    fn stop<'a>(&'a self, _id: &'a SandboxId) -> MachineApiServiceFuture<'a, ()> {
        Box::pin(async move {
            self.stop_calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
    }

    fn exposed_machine_port_receipts<'a>(
        &'a self,
        _id: &'a SandboxId,
    ) -> MachineApiServiceFuture<'a, Vec<nimbus_sandbox::MachinePortForwardReceipt>> {
        Box::pin(async move {
            Err(MachineApiHttpError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: "absence retry fixture has no exposed evidence".to_owned(),
            })
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

#[derive(Default)]
struct ReceiptlessNodeWorkloadFacade {
    handle: Mutex<Option<SandboxHandle>>,
}

impl ReceiptlessNodeWorkloadFacade {
    fn with_handle(handle: SandboxHandle) -> Self {
        Self {
            handle: Mutex::new(Some(handle)),
        }
    }

    fn has_handle(&self) -> bool {
        self.handle
            .lock()
            .expect("receiptless handle lock")
            .is_some()
    }
}

impl MachineApiNodeWorkloadFacade for ReceiptlessNodeWorkloadFacade {
    fn kind(&self) -> SandboxBackendKind {
        SandboxBackendKind::Container
    }

    fn start<'a>(
        &'a self,
        sandbox_id: SandboxId,
        spec: SandboxSpec,
    ) -> MachineApiServiceFuture<'a, SandboxHandle> {
        Box::pin(async move {
            let service_name = spec.display_name().to_owned();
            let handle = SandboxHandle::new(
                spec.tenant_id,
                sandbox_id,
                service_name,
                SandboxBackendKind::Container,
                SandboxStatus::Ready,
                Vec::new(),
            );
            *self.handle.lock().expect("receiptless handle lock") = Some(handle.clone());
            Ok(handle)
        })
    }

    fn inspect<'a>(
        &'a self,
        id: &'a SandboxId,
    ) -> MachineApiServiceFuture<'a, Option<SandboxHandle>> {
        Box::pin(async move {
            Ok(self
                .handle
                .lock()
                .expect("receiptless handle lock")
                .clone()
                .filter(|handle| &handle.id == id))
        })
    }

    fn stop<'a>(&'a self, _id: &'a SandboxId) -> MachineApiServiceFuture<'a, ()> {
        Box::pin(async move { Ok(()) })
    }

    fn exposed_machine_port_receipts<'a>(
        &'a self,
        _id: &'a SandboxId,
    ) -> MachineApiServiceFuture<'a, Vec<nimbus_sandbox::MachinePortForwardReceipt>> {
        Box::pin(async move {
            Err(MachineApiHttpError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: "no exposed provider receipt was recorded".to_owned(),
            })
        })
    }

    fn absent_machine_port_receipts<'a>(
        &'a self,
        _id: &'a SandboxId,
    ) -> MachineApiServiceFuture<
        'a,
        Option<nimbus_sandbox::backends::container::MachinePortAbsenceEvidence>,
    > {
        Box::pin(async move {
            Err(MachineApiHttpError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: "no absent provider receipt was recorded".to_owned(),
            })
        })
    }
}
