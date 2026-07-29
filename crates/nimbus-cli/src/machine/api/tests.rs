use super::binaries::{STANDARD_CONTAINER_BINARY_REQUIREMENTS, apply_resolved_runtime_paths};
use super::capabilities::machine_api_capability_response;
use super::network_composition::GuestMachineNetworkComposition;
use super::state::machine_container_state_root;
use super::*;

use std::fs;
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use nimbus::{
    EndpointProtocol, PublishedEndpoint, SandboxBackend, SandboxBackendKind, SandboxError,
    SandboxHandle, SandboxId, SandboxOwnerSpec, SandboxPortBinding, SandboxProcessSpec,
    SandboxRootSpec, SandboxSpec, SandboxStatus, TenantId,
};
use nimbus_machine::MachineBootAuthorityEvidence;
use nimbus_network::{
    LocalNetworkManager, NetworkPlanId, NetworkProviderHandle, NetworkProviderId,
    NetworkResourceGeneration,
};
use nimbus_sandbox::SandboxFuture;
use serde_json::json;
use tempfile::{Builder, TempDir};

mod publication_evidence;

fn test_forwarder_authority(provider_instance: &str) -> MachineForwarderAuthority {
    let config = OciMachinePortForwarderConfig::for_provider_instance(
        "127.0.0.1",
        9,
        "/services/forwarder",
        provider_instance,
        NetworkResourceGeneration::new(1),
    )
    .expect("test machine-forwarder identity should validate");
    MachineForwarderAuthority::new(
        config.provider_instance().clone(),
        config.provider_generation(),
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial_test::serial]
async fn guest_rejects_guest_minted_or_stale_parent_provider_authority_before_effects() {
    let temp_dir = short_socket_tempdir();
    let control_data_dir = temp_dir.path().join("control");
    let authority_path = temp_dir.path().join("machine-api-authority.json");
    let socket_path = temp_dir.path().join("nimbus.sock");
    let guest_minted = MachineBootAuthorityEvidence::new(
        "default",
        MachineForwarderAuthority::new(
            NetworkProviderHandle::new(
                NetworkProviderId::for_registration_key("guest-minted-forwarder"),
                "guest-node-derived-opaque-handle",
            )
            .expect("guest-minted fixture handle should validate structurally"),
            NetworkResourceGeneration::new(7),
        ),
    )
    .expect("guest-minted evidence fixture should serialize");
    fs::write(
        &authority_path,
        serde_json::to_vec_pretty(&guest_minted)
            .expect("guest-minted authority fixture should serialize"),
    )
    .expect("guest-minted authority fixture should write");
    let roots = MachineRootLayout::test_sibling_roots(
        temp_dir.path().join("config"),
        temp_dir.path().join("state"),
        temp_dir.path().join("runtime"),
    );
    let command = MachineApiCommand {
        socket_path: socket_path.clone(),
        control_data_dir: Some(control_data_dir.clone()),
        guest_node_id: "machine-os-guest-node".to_owned(),
    };

    let result = tokio::time::timeout(
        Duration::from_millis(250),
        run_machine_api_command(command, &roots),
    )
    .await;
    let error = match result {
        Ok(Err(error)) => error,
        Ok(Ok(())) => panic!("machine API unexpectedly exited successfully"),
        Err(_) => panic!(
            "machine API ignored guest-minted authority and began serving; control_exists={}, socket_exists={}",
            control_data_dir.exists(),
            socket_path.exists()
        ),
    };

    assert!(
        error.to_string().contains("forwarder provider"),
        "rejection must identify provider authority without exposing opaque material: {error}"
    );
    assert!(
        !control_data_dir.exists() && !socket_path.exists(),
        "authority rejection must precede manager, filesystem, listener, workload, and provider effects"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial_test::serial]
async fn guest_machine_api_claims_manager_before_listener_and_splits_workload_artifacts() {
    let temp_dir = short_socket_tempdir();
    let control_data_dir = temp_dir.path().join("control");
    let authority_path = temp_dir.path().join("machine-api-authority.json");
    let socket_path = temp_dir.path().join("nimbus.sock");
    let authority = test_forwarder_authority("manager-before-listener");
    let evidence = MachineBootAuthorityEvidence::new("default", authority.clone())
        .expect("boot authority fixture should validate");
    fs::write(
        &authority_path,
        serde_json::to_vec_pretty(&evidence).expect("boot authority fixture should serialize"),
    )
    .expect("boot authority fixture should write");
    let held_manager = LocalNetworkManager::bootstrap(&control_data_dir)
        .expect("fixture should hold the guest manager composition");
    let roots = MachineRootLayout::test_sibling_roots(
        temp_dir.path().join("config"),
        temp_dir.path().join("state"),
        temp_dir.path().join("runtime"),
    );
    let command = MachineApiCommand {
        socket_path: socket_path.clone(),
        control_data_dir: Some(control_data_dir.clone()),
        guest_node_id: "machine-os-guest-node".to_owned(),
    };

    let result = tokio::time::timeout(
        Duration::from_millis(250),
        run_machine_api_command(command, &roots),
    )
    .await;
    let error = match result {
        Ok(Err(error)) => error,
        Ok(Ok(())) => panic!("machine API unexpectedly exited successfully"),
        Err(_) => panic!(
            "machine API bound or served without claiming the guest manager first; socket_exists={}",
            socket_path.exists()
        ),
    };

    assert!(
        error
            .to_string()
            .contains("already owns network composition"),
        "the manager claim must fail with typed duplicate-composition evidence: {error}"
    );
    assert!(
        !socket_path.exists(),
        "a failed manager claim must precede listener binding"
    );
    assert!(
        !control_data_dir.join("service-sandboxes").exists(),
        "a failed manager claim must precede workload artifact creation"
    );
    drop(held_manager);
    let workload_root = control_data_dir.join("service-sandboxes").join("container");
    let mut container_config = ContainerSandboxBackendConfig::plan_only(
        workload_root.join("bundles"),
        workload_root.join("state"),
    );
    container_config.machine_port_forwarder = Some(
        OciMachinePortForwarderConfig::gvproxy_for_provider_instance(
            authority.provider_instance().expose_to_provider(),
            authority.generation(),
        )
        .expect("fixture provider should reconstruct through the owning adapter"),
    );
    let composition = GuestMachineNetworkComposition::claim(&control_data_dir, container_config)
        .expect("the guest should compose after the competing manager drops");
    let (network_root, workload_state_root) = composition.state_roots();
    assert_eq!(
        fs::canonicalize(network_root).expect("network root should exist"),
        fs::canonicalize(&control_data_dir).expect("control root should exist")
    );
    assert_eq!(workload_state_root, workload_root.join("state"));
    assert_ne!(network_root, workload_state_root);
    drop(composition);
    assert!(
        LocalNetworkManager::bootstrap(&control_data_dir).is_ok(),
        "final manager drop must permit deterministic reopen"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn machine_api_serves_health_and_capabilities_over_unix_socket() {
    let temp_dir = short_socket_tempdir();
    let socket_path = temp_dir.path().join("nimbus.sock");
    let listener = bind_direct_listener(&socket_path).expect("listener should bind");
    assert_eq!(
        fs::metadata(&socket_path)
            .expect("socket metadata should exist")
            .permissions()
            .mode()
            & 0o777,
        0o600,
        "the direct Machine API listener must remain owner-only after removing socket activation"
    );
    let state = MachineApiState {
        control_data_dir: temp_dir.path().join("control"),
        listen_mode: MachineApiListenMode::DirectSocket,
        binary_lookup_path: Some(fake_runtime_path(&temp_dir)),
        helper_binary_dirs: Vec::new(),
        service_workloads: None,
        machine_port_forwarder: None,
        forwarder_authority: None,
    };
    for requirement in STANDARD_CONTAINER_BINARY_REQUIREMENTS {
        write_fake_binary(&temp_dir, requirement.name);
    }
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

    let server = tokio::spawn(serve_machine_api(listener, state, async move {
        let _ = shutdown_rx.await;
    }));
    wait_for_socket_path(&socket_path);

    let health = wait_for_http_response_contains(&socket_path, "/healthz", "\"status\":\"ok\"");
    assert!(health.contains("200 OK"), "{health}");
    assert!(health.contains("\"status\":\"ok\""), "{health}");
    assert!(
        health.contains("\"role\":\"guest-machine-api\""),
        "{health}"
    );

    let capabilities = unix_http_get(&socket_path, "/v1/machine-api/capabilities");
    assert!(capabilities.contains("200 OK"), "{capabilities}");
    assert!(
        capabilities.contains("\"service_execution_ready\":false"),
        "{capabilities}"
    );
    assert!(
        capabilities.contains("\"service_execution_mode\":\"standard_containers\""),
        "{capabilities}"
    );
    assert!(
        capabilities.contains("\"service_execution_driver\":\"unavailable\""),
        "{capabilities}"
    );
    assert!(
        capabilities.contains("\"supported_service_backends\":[\"container\"]"),
        "{capabilities}"
    );
    assert!(
        capabilities.contains("\"service_execution_blockers\":["),
        "{capabilities}"
    );
    assert!(
        capabilities
            .contains("\"guest machine API does not yet expose service lifecycle operations\""),
        "{capabilities}"
    );

    let _ = shutdown_tx.send(());
    server
        .await
        .expect("machine API server task should join")
        .expect("machine API server should shut down cleanly");
}

#[test]
fn capability_response_reports_binary_statuses_and_explicit_blockers() {
    let temp_dir = short_socket_tempdir();
    write_fake_binary(&temp_dir, "conmon");
    write_fake_binary(&temp_dir, "crun");

    let state = MachineApiState {
        control_data_dir: temp_dir.path().join("control"),
        listen_mode: MachineApiListenMode::DirectSocket,
        binary_lookup_path: Some(fake_runtime_path(&temp_dir)),
        helper_binary_dirs: Vec::new(),
        service_workloads: None,
        machine_port_forwarder: None,
        forwarder_authority: None,
    };
    let capabilities = machine_api_capability_response(&state);

    assert_eq!(
        capabilities.service_execution_mode,
        MachineApiServiceExecutionMode::StandardContainers
    );
    assert_eq!(
        capabilities.service_execution_driver,
        MachineApiServiceExecutionDriver::Unavailable
    );
    assert_eq!(
        capabilities.supported_service_backends,
        vec![SandboxBackendKind::Container]
    );
    assert_eq!(
        capabilities.supported_operations,
        vec!["healthz".to_owned(), "capabilities".to_owned()]
    );
    assert!(!capabilities.service_execution_ready);
    assert!(
        capabilities
            .service_execution_blockers
            .iter()
            .any(|blocker| blocker == MACHINE_API_OPERATION_BLOCKER)
    );
    assert!(capabilities.binary_statuses.iter().any(|binary| {
        binary.name == "netavark"
            && !binary.present
            && binary.required_for_operations
                == vec![
                    MACHINE_API_IMAGE_START_OPERATION.to_owned(),
                    MACHINE_API_BUILD_START_OPERATION.to_owned(),
                ]
    }));
    assert!(
        capabilities
            .service_execution_blockers
            .iter()
            .any(|blocker| blocker
                == "missing guest binary required for service-sandboxes.image-start: netavark")
    );
    assert!(capabilities.operation_statuses.iter().any(|status| {
        status.name == MACHINE_API_BUILD_START_OPERATION
            && !status.available
            && status
                .blockers
                .iter()
                .any(|blocker| blocker == MACHINE_API_OPERATION_BLOCKER)
    }));
}

#[test]
fn capability_response_reports_machine_port_forwarder_blocker_when_unreachable() {
    let temp_dir = short_socket_tempdir();
    for requirement in STANDARD_CONTAINER_BINARY_REQUIREMENTS {
        write_fake_binary(&temp_dir, requirement.name);
    }
    let machine_port_forwarder = OciMachinePortForwarderConfig::for_provider_instance(
        "127.0.0.1",
        9,
        "/services/forwarder",
        "unreachable-test-forwarder",
        NetworkResourceGeneration::new(1),
    )
    .expect("test machine-forwarder identity should validate");
    let forwarder_authority = MachineForwarderAuthority::new(
        machine_port_forwarder.provider_instance().clone(),
        machine_port_forwarder.provider_generation(),
    );

    let state = MachineApiState {
        control_data_dir: temp_dir.path().join("control"),
        listen_mode: MachineApiListenMode::DirectSocket,
        binary_lookup_path: Some(fake_runtime_path(&temp_dir)),
        helper_binary_dirs: Vec::new(),
        service_workloads: Some(machine_api_node_workload_facade_from_sandbox_backend(
            Arc::new(ContainerSandboxBackend::new(
                ContainerSandboxBackendConfig::plan_only(
                    temp_dir.path().join("bundles"),
                    temp_dir.path().join("state"),
                ),
            )),
        )),
        machine_port_forwarder: Some(machine_port_forwarder),
        forwarder_authority: Some(forwarder_authority),
    };

    let capabilities = machine_api_capability_response(&state);
    assert!(!capabilities.service_execution_ready);
    assert_eq!(
        capabilities.service_execution_driver,
        MachineApiServiceExecutionDriver::GuestNodeAgentSystemdTransientUnit
    );
    assert_eq!(
        capabilities.supported_operations,
        vec![
            "healthz".to_owned(),
            "capabilities".to_owned(),
            MACHINE_API_BOOTC_STATUS_OPERATION.to_owned(),
            MACHINE_API_BOOTC_SWITCH_OPERATION.to_owned(),
            MACHINE_API_BOOTC_UPGRADE_OPERATION.to_owned(),
            MACHINE_API_BOOTC_ROLLBACK_OPERATION.to_owned(),
            "service-sandboxes.list".to_owned(),
            "service-sandboxes.inspect".to_owned(),
            "service-sandboxes.inspect-current".to_owned(),
            "service-sandboxes.logs".to_owned(),
            "service-sandboxes.ps".to_owned(),
        ]
    );
    assert!(
        capabilities
            .service_execution_blockers
            .iter()
            .any(|blocker| blocker
                .contains("guest machine port forwarder is not reachable at 127.0.0.1:9")),
        "{:?}",
        capabilities.service_execution_blockers
    );
}

#[test]
fn capability_response_reports_unavailable_node_workload_driver_when_lifecycle_is_blocked() {
    let temp_dir = short_socket_tempdir();
    for requirement in STANDARD_CONTAINER_BINARY_REQUIREMENTS {
        write_fake_binary(&temp_dir, requirement.name);
    }

    let state = MachineApiState {
        control_data_dir: temp_dir.path().join("control"),
        listen_mode: MachineApiListenMode::DirectSocket,
        binary_lookup_path: Some(fake_runtime_path(&temp_dir)),
        helper_binary_dirs: Vec::new(),
        service_workloads: Some(Arc::new(BlockedNodeWorkloadFacade)),
        machine_port_forwarder: None,
        forwarder_authority: None,
    };

    let capabilities = machine_api_capability_response(&state);

    assert!(!capabilities.service_execution_ready);
    assert_eq!(
        capabilities.service_execution_driver,
        MachineApiServiceExecutionDriver::Unavailable
    );
    assert_eq!(
        capabilities.service_execution_blockers,
        vec!["guest node lifecycle backend unavailable: systemd D-Bus is unavailable".to_owned()]
    );
    assert!(
        !capabilities
            .supported_operations
            .iter()
            .any(|operation| operation == MACHINE_API_IMAGE_START_OPERATION)
    );
    assert!(capabilities.operation_statuses.iter().any(|status| {
        status.name == MACHINE_API_IMAGE_START_OPERATION
            && !status.available
            && status.blockers == capabilities.service_execution_blockers
    }));
}

#[test]
fn capability_response_resolves_helper_binaries_from_podman_dirs() {
    let temp_dir = short_socket_tempdir();
    let helper_dir = temp_dir.path().join("podman-helpers");
    fs::create_dir_all(&helper_dir).expect("helper dir should create");
    write_fake_binary(&temp_dir, "conmon");
    write_fake_binary(&temp_dir, "crun");
    write_fake_binary_at(&helper_dir, "netavark");
    write_fake_binary_at(&helper_dir, "aardvark-dns");

    let state = MachineApiState {
        control_data_dir: temp_dir.path().join("control"),
        listen_mode: MachineApiListenMode::DirectSocket,
        binary_lookup_path: Some(fake_runtime_path(&temp_dir)),
        helper_binary_dirs: vec![helper_dir.clone()],
        service_workloads: None,
        machine_port_forwarder: None,
        forwarder_authority: None,
    };

    let capabilities = machine_api_capability_response(&state);
    let netavark_path = helper_dir.join("netavark").display().to_string();
    let aardvark_path = helper_dir.join("aardvark-dns").display().to_string();
    assert!(capabilities.binary_statuses.iter().any(|binary| {
        binary.name == "netavark"
            && binary.present
            && binary.resolved_path.as_deref() == Some(netavark_path.as_str())
    }));
    assert!(capabilities.binary_statuses.iter().any(|binary| {
        binary.name == "aardvark-dns"
            && binary.present
            && binary.resolved_path.as_deref() == Some(aardvark_path.as_str())
    }));
    assert!(
        !capabilities
            .service_execution_blockers
            .iter()
            .any(|blocker| blocker.contains("netavark") || blocker.contains("aardvark-dns"))
    );
}

#[test]
fn capability_response_keeps_build_start_available_without_buildah_or_fuse_overlayfs() {
    let temp_dir = short_socket_tempdir();
    let helper_dir = temp_dir.path().join("podman-helpers");
    fs::create_dir_all(&helper_dir).expect("helper dir should create");
    write_fake_binary(&temp_dir, "conmon");
    write_fake_binary(&temp_dir, "crun");
    write_fake_binary_at(&helper_dir, "netavark");
    write_fake_binary_at(&helper_dir, "aardvark-dns");

    let state = MachineApiState {
        control_data_dir: temp_dir.path().join("control"),
        listen_mode: MachineApiListenMode::DirectSocket,
        binary_lookup_path: Some(fake_runtime_path(&temp_dir)),
        helper_binary_dirs: vec![helper_dir],
        service_workloads: Some(machine_api_node_workload_facade_from_sandbox_backend(
            Arc::new(ContainerSandboxBackend::new(
                ContainerSandboxBackendConfig::plan_only(
                    temp_dir.path().join("bundles"),
                    temp_dir.path().join("state"),
                ),
            )),
        )),
        machine_port_forwarder: None,
        forwarder_authority: None,
    };

    let capabilities = machine_api_capability_response(&state);

    assert!(capabilities.service_execution_ready);
    assert_eq!(
        capabilities.service_execution_driver,
        MachineApiServiceExecutionDriver::GuestNodeAgentSystemdTransientUnit
    );
    assert!(
        capabilities
            .supported_operations
            .iter()
            .any(|operation| operation == MACHINE_API_IMAGE_START_OPERATION)
    );
    assert!(
        capabilities
            .supported_operations
            .iter()
            .any(|operation| operation == MACHINE_API_BUILD_START_OPERATION)
    );
    assert!(
        capabilities
            .binary_statuses
            .iter()
            .all(|binary| binary.name != "buildah" && binary.name != "fuse-overlayfs")
    );
    assert!(capabilities.operation_statuses.iter().any(|status| {
        status.name == MACHINE_API_BUILD_START_OPERATION
            && status.available
            && status.blockers.is_empty()
    }));
}

#[test]
fn apply_resolved_runtime_paths_updates_backend_config_from_helper_dirs() {
    let temp_dir = short_socket_tempdir();
    let helper_dir = temp_dir.path().join("podman-helpers");
    fs::create_dir_all(&helper_dir).expect("helper dir should create");
    write_fake_binary(&temp_dir, "buildah");
    write_fake_binary(&temp_dir, "conmon");
    write_fake_binary(&temp_dir, "crun");
    write_fake_binary_at(&helper_dir, "netavark");
    write_fake_binary_at(&helper_dir, "aardvark-dns");

    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path().join("root"));
    let runtime_path = fake_runtime_path(&temp_dir);
    apply_resolved_runtime_paths(
        &mut config,
        Some(runtime_path.as_os_str()),
        std::slice::from_ref(&helper_dir),
    );

    assert_eq!(config.buildah_path, temp_dir.path().join("buildah"));
    assert_eq!(config.conmon_path, temp_dir.path().join("conmon"));
    assert_eq!(config.runtime_path, temp_dir.path().join("crun"));
    assert_eq!(config.netavark_path, helper_dir.join("netavark"));
    assert_eq!(config.aardvark_dns_path, helper_dir.join("aardvark-dns"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn machine_api_list_and_current_refresh_persisted_service_state_before_reply() {
    let temp_dir = short_socket_tempdir();
    let control_data_dir = temp_dir.path().join("control");
    let state_root = machine_container_state_root(&control_data_dir);
    let tenant_id = TenantId::new("svc-demo").expect("tenant id should be valid");
    let sandbox_id = SandboxId::new("demo-01aaa");
    let stopped_sandbox_id = SandboxId::new("demo-01old");
    write_container_manifest(
        &state_root,
        sandbox_id.as_str(),
        tenant_id.as_str(),
        "demo",
        SandboxStatus::Starting,
        Vec::new(),
    );
    write_container_manifest(
        &state_root,
        stopped_sandbox_id.as_str(),
        tenant_id.as_str(),
        "demo",
        SandboxStatus::Stopped,
        Vec::new(),
    );

    let backend = RefreshingInspectBackend::new(state_root.clone());
    let inspected_ids = backend.inspected_ids();

    let socket_path = temp_dir.path().join("nimbus.sock");
    let listener = bind_direct_listener(&socket_path).expect("listener should bind");
    let state = MachineApiState {
        control_data_dir,
        listen_mode: MachineApiListenMode::DirectSocket,
        binary_lookup_path: Some(fake_runtime_path(&temp_dir)),
        helper_binary_dirs: Vec::new(),
        service_workloads: Some(machine_api_node_workload_facade_from_sandbox_backend(
            Arc::new(backend),
        )),
        machine_port_forwarder: None,
        forwarder_authority: None,
    };
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(serve_machine_api(listener, state, async move {
        let _ = shutdown_rx.await;
    }));
    wait_for_socket_path(&socket_path);

    let list_response = wait_for_http_response_contains(
        &socket_path,
        &format!("/v1/machine-api/service-sandboxes?tenant_id={tenant_id}"),
        "\"status\":\"ready\"",
    );
    assert!(
        list_response.contains("\"published_endpoints\":[{\"name\":\"default\""),
        "{list_response}"
    );

    let current_response = wait_for_http_response_contains(
        &socket_path,
        &format!(
            "/v1/machine-api/service-sandboxes/current?tenant_id={tenant_id}&service_name=demo"
        ),
        "\"status\":\"ready\"",
    );
    assert!(
        current_response.contains("\"published_endpoints\":[{\"name\":\"default\""),
        "{current_response}"
    );
    let inspected_ids = inspected_ids.lock().expect("lock should acquire").clone();
    assert_eq!(
        inspected_ids,
        vec![
            sandbox_id.as_str().to_owned(),
            sandbox_id.as_str().to_owned()
        ]
    );
    assert!(
        !inspected_ids
            .iter()
            .any(|id| id == stopped_sandbox_id.as_str()),
        "{inspected_ids:?}"
    );

    let _ = shutdown_tx.send(());
    server
        .await
        .expect("machine API server task should join")
        .expect("machine API server should shut down cleanly");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn machine_api_start_routes_reject_cross_wired_root_kinds_before_backend_start() {
    let temp_dir = short_socket_tempdir();
    let socket_path = temp_dir.path().join("nimbus.sock");
    let listener = bind_direct_listener(&socket_path).expect("listener should bind");
    let backend = RecordingStartBackend::default();
    let started_sandboxes = backend.started_sandboxes();
    let forwarder_authority = test_forwarder_authority("cross-wired-root-test-forwarder");
    let state = MachineApiState {
        control_data_dir: temp_dir.path().join("control"),
        listen_mode: MachineApiListenMode::DirectSocket,
        binary_lookup_path: Some(fake_runtime_path(&temp_dir)),
        helper_binary_dirs: Vec::new(),
        service_workloads: Some(machine_api_node_workload_facade_from_sandbox_backend(
            Arc::new(backend),
        )),
        machine_port_forwarder: None,
        forwarder_authority: Some(forwarder_authority.clone()),
    };
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(serve_machine_api(listener, state, async move {
        let _ = shutdown_rx.await;
    }));
    wait_for_socket_path(&socket_path);

    let tenant_id = TenantId::new("svc-demo").expect("tenant id should be valid");
    let build_body = serde_json::to_string(&MachineApiServiceSandboxImageStartRequest {
        sandbox_id: machine_api_sandbox_id(&tenant_id, "cross-wired-image"),
        forwarder_authority: forwarder_authority.clone(),
        spec: machine_api_build_spec(&tenant_id, "api"),
    })
    .expect("build request should serialize");
    let image_response = unix_http_post_json(
        &socket_path,
        "/v1/machine-api/service-sandboxes/image-start",
        &build_body,
    );
    assert!(
        image_response.contains("400 Bad Request"),
        "{image_response}"
    );
    assert!(
        image_response.contains(MACHINE_API_IMAGE_START_OPERATION),
        "{image_response}"
    );
    assert!(
        image_response.contains("requires OCI image reference"),
        "{image_response}"
    );
    assert!(
        image_response.contains("received OCI image build"),
        "{image_response}"
    );

    let image_body = serde_json::to_string(&MachineApiServiceSandboxBuildStartRequest {
        sandbox_id: machine_api_sandbox_id(&tenant_id, "cross-wired-build"),
        forwarder_authority,
        spec: machine_api_image_spec(&tenant_id, "db"),
    })
    .expect("image request should serialize");
    let build_response = unix_http_post_json(
        &socket_path,
        "/v1/machine-api/service-sandboxes/build-start",
        &image_body,
    );
    assert!(
        build_response.contains("400 Bad Request"),
        "{build_response}"
    );
    assert!(
        build_response.contains(MACHINE_API_BUILD_START_OPERATION),
        "{build_response}"
    );
    assert!(
        build_response.contains("requires OCI image build"),
        "{build_response}"
    );
    assert!(
        build_response.contains("received OCI image reference"),
        "{build_response}"
    );

    assert!(
        started_sandboxes
            .lock()
            .expect("lock should acquire")
            .is_empty(),
        "mismatched route/spec roots must be rejected before backend start"
    );

    let _ = shutdown_tx.send(());
    server
        .await
        .expect("machine API server task should join")
        .expect("machine API server should shut down cleanly");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn machine_api_start_routes_reject_standalone_specs_before_backend_start() {
    let temp_dir = short_socket_tempdir();
    let socket_path = temp_dir.path().join("nimbus.sock");
    let listener = bind_direct_listener(&socket_path).expect("listener should bind");
    let backend = RecordingStartBackend::default();
    let started_sandboxes = backend.started_sandboxes();
    let forwarder_authority = test_forwarder_authority("standalone-start-test-forwarder");
    let state = MachineApiState {
        control_data_dir: temp_dir.path().join("control"),
        listen_mode: MachineApiListenMode::DirectSocket,
        binary_lookup_path: Some(fake_runtime_path(&temp_dir)),
        helper_binary_dirs: Vec::new(),
        service_workloads: Some(machine_api_node_workload_facade_from_sandbox_backend(
            Arc::new(backend),
        )),
        machine_port_forwarder: None,
        forwarder_authority: Some(forwarder_authority.clone()),
    };
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(serve_machine_api(listener, state, async move {
        let _ = shutdown_rx.await;
    }));
    wait_for_socket_path(&socket_path);

    let tenant_id = TenantId::new("svc-demo").expect("tenant id should be valid");
    let mut image_spec = machine_api_image_spec(&tenant_id, "db");
    image_spec.owner = SandboxOwnerSpec::standalone_named("scratch-db");
    let image_body = serde_json::to_string(&MachineApiServiceSandboxImageStartRequest {
        sandbox_id: machine_api_sandbox_id(&tenant_id, "standalone-image"),
        forwarder_authority: forwarder_authority.clone(),
        spec: image_spec,
    })
    .expect("image request should serialize");
    let image_response = unix_http_post_json(
        &socket_path,
        "/v1/machine-api/service-sandboxes/image-start",
        &image_body,
    );
    assert!(
        image_response.contains("400 Bad Request"),
        "{image_response}"
    );
    assert!(
        image_response.contains("requires service-owned sandbox metadata"),
        "{image_response}"
    );

    let mut build_spec = machine_api_build_spec(&tenant_id, "api");
    build_spec.owner = SandboxOwnerSpec::standalone_named("scratch-api");
    let build_body = serde_json::to_string(&MachineApiServiceSandboxBuildStartRequest {
        sandbox_id: machine_api_sandbox_id(&tenant_id, "standalone-build"),
        forwarder_authority,
        spec: build_spec,
    })
    .expect("build request should serialize");
    let build_response = unix_http_post_json(
        &socket_path,
        "/v1/machine-api/service-sandboxes/build-start",
        &build_body,
    );
    assert!(
        build_response.contains("400 Bad Request"),
        "{build_response}"
    );
    assert!(
        build_response.contains("requires service-owned sandbox metadata"),
        "{build_response}"
    );

    assert!(
        started_sandboxes
            .lock()
            .expect("lock should acquire")
            .is_empty(),
        "standalone specs must be rejected before backend start"
    );

    let _ = shutdown_tx.send(());
    server
        .await
        .expect("machine API server task should join")
        .expect("machine API server should shut down cleanly");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn service_sandbox_id_routes_ignore_standalone_sandbox_records() {
    let temp_dir = short_socket_tempdir();
    let control_data_dir = temp_dir.path().join("control");
    let state_root = machine_container_state_root(&control_data_dir);
    let sandbox_id = SandboxId::new("standalone-01aaa");
    write_standalone_container_manifest(
        &state_root,
        sandbox_id.as_str(),
        "svc-demo",
        "scratch",
        SandboxStatus::Ready,
    );

    let backend = RecordingStartBackend::default();
    let stopped_sandboxes = backend.stopped_sandboxes();
    let forwarder_authority = test_forwarder_authority("standalone-stop-test-forwarder");
    let socket_path = temp_dir.path().join("nimbus.sock");
    let listener = bind_direct_listener(&socket_path).expect("listener should bind");
    let state = MachineApiState {
        control_data_dir,
        listen_mode: MachineApiListenMode::DirectSocket,
        binary_lookup_path: Some(fake_runtime_path(&temp_dir)),
        helper_binary_dirs: Vec::new(),
        service_workloads: Some(machine_api_node_workload_facade_from_sandbox_backend(
            Arc::new(backend),
        )),
        machine_port_forwarder: None,
        forwarder_authority: Some(forwarder_authority.clone()),
    };
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(serve_machine_api(listener, state, async move {
        let _ = shutdown_rx.await;
    }));
    wait_for_socket_path(&socket_path);

    let inspect_response = unix_http_get(
        &socket_path,
        &format!("/v1/machine-api/service-sandboxes/{sandbox_id}"),
    );
    assert!(inspect_response.contains("200 OK"), "{inspect_response}");
    assert!(
        inspect_response.contains("\"handle\":null"),
        "{inspect_response}"
    );

    let stop_body = serde_json::to_string(&MachineApiServiceSandboxStopRequest {
        forwarder_authority,
    })
    .expect("stop request should serialize");
    let stop_response = unix_http_post_json(
        &socket_path,
        &format!("/v1/machine-api/service-sandboxes/{sandbox_id}/stop"),
        &stop_body,
    );
    assert!(stop_response.contains("404 Not Found"), "{stop_response}");
    assert!(
        stopped_sandboxes
            .lock()
            .expect("lock should acquire")
            .is_empty(),
        "standalone records must be rejected before backend stop"
    );

    let _ = shutdown_tx.send(());
    server
        .await
        .expect("machine API server task should join")
        .expect("machine API server should shut down cleanly");
}

fn unix_http_get(socket_path: &Path, path: &str) -> String {
    unix_http_request(
        socket_path,
        &format!("GET {path} HTTP/1.0\r\nHost: localhost\r\n\r\n"),
    )
}

fn unix_http_post_json(socket_path: &Path, path: &str, body: &str) -> String {
    unix_http_request(
        socket_path,
        &format!(
            "POST {path} HTTP/1.0\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        ),
    )
}

/// One-shot HTTP/1.0 exchange over the unix socket. The server accepts
/// each connection for a single request; under parallel test load the
/// accept can race the client's write, surfacing as a transient
/// `BrokenPipe`/`ConnectionReset` (or an empty response). A real
/// one-shot client retries on a fresh connection, so this helper does
/// too — bounded, with real errors still failing loudly.
fn unix_http_request(socket_path: &Path, request: &str) -> String {
    let mut last_failure = String::new();
    for _ in 0..5 {
        let mut stream = UnixStream::connect(socket_path).expect("unix socket should accept");
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("read timeout should set");
        match stream.write_all(request.as_bytes()) {
            Ok(()) => {
                let response =
                    read_unix_http_response(stream).expect("response should be valid utf-8");
                if !response.is_empty() {
                    return response;
                }
                last_failure = "empty response".to_string();
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::BrokenPipe | std::io::ErrorKind::ConnectionReset
                ) =>
            {
                last_failure = error.to_string();
            }
            Err(error) => panic!("request should write: {error:?}"),
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("machine API request kept failing after retries: {last_failure}");
}

fn read_unix_http_response(mut stream: UnixStream) -> Result<String, std::io::Error> {
    let mut response = Vec::new();
    let mut chunk = [0_u8; 4096];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(read) => response.extend_from_slice(&chunk[..read]),
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                break;
            }
            Err(error) => return Err(error),
        }
    }
    String::from_utf8(response)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))
}

fn wait_for_http_response_contains(socket_path: &Path, path: &str, needle: &str) -> String {
    let start = std::time::Instant::now();
    loop {
        let response = try_unix_http_get(socket_path, path).unwrap_or_default();
        if response.contains(needle) {
            return response;
        }
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "timed out waiting for machine API response on {}{}; last response: {}",
            socket_path.display(),
            path,
            response
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn try_unix_http_get(socket_path: &Path, path: &str) -> Result<String, std::io::Error> {
    let mut stream = UnixStream::connect(socket_path)?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    write!(stream, "GET {path} HTTP/1.0\r\nHost: localhost\r\n\r\n")?;
    read_unix_http_response(stream)
}

fn wait_for_socket_path(path: &Path) {
    let start = std::time::Instant::now();
    while !path.exists() {
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "timed out waiting for socket {}",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn short_socket_tempdir() -> TempDir {
    Builder::new()
        .prefix("nimbus-ma-")
        .tempdir_in("/tmp")
        .expect("short temp dir should exist")
}

fn fake_runtime_path(temp_dir: &TempDir) -> OsString {
    temp_dir.path().as_os_str().to_owned()
}

fn write_fake_binary(temp_dir: &TempDir, name: &str) {
    write_fake_binary_at(temp_dir.path(), name);
}

fn write_fake_binary_at(root: &Path, name: &str) {
    let path = root.join(name);
    crate::test_support::write_executable_stub(&path, "#!/bin/sh\nexit 0\n");
}

fn write_container_manifest(
    state_root: &Path,
    sandbox_id: &str,
    tenant_id: &str,
    service_name: &str,
    status: SandboxStatus,
    published_endpoints: Vec<PublishedEndpoint>,
) {
    write_container_manifest_with_owner(
        state_root,
        sandbox_id,
        tenant_id,
        service_name,
        json!({
            "kind": "service",
            "name": service_name
        }),
        status,
        published_endpoints,
    );
}

fn write_standalone_container_manifest(
    state_root: &Path,
    sandbox_id: &str,
    tenant_id: &str,
    display_name: &str,
    status: SandboxStatus,
) {
    write_container_manifest_with_owner(
        state_root,
        sandbox_id,
        tenant_id,
        display_name,
        json!({
            "kind": "standalone",
            "display_name": display_name
        }),
        status,
        Vec::new(),
    );
}

fn write_container_manifest_with_owner(
    state_root: &Path,
    sandbox_id: &str,
    tenant_id: &str,
    handle_name: &str,
    owner: serde_json::Value,
    status: SandboxStatus,
    published_endpoints: Vec<PublishedEndpoint>,
) {
    let container_dir = state_root
        .join("tenants")
        .join(tenant_id)
        .join("sandboxes")
        .join(sandbox_id)
        .join("state")
        .join("containers")
        .join(sandbox_id);
    fs::create_dir_all(&container_dir).expect("container manifest directory should exist");

    let handle = SandboxHandle::new(
        nimbus::TenantId::new(tenant_id).expect("tenant id should parse"),
        SandboxId::new(sandbox_id),
        handle_name,
        SandboxBackendKind::Container,
        status,
        published_endpoints,
    );
    let manifest = json!({
        "handle": handle,
        "spec": {
            "tenant_id": tenant_id,
            "owner": owner,
            "backend": "container",
            "root": {
                "kind": "rootfs",
                "rootfs": "/tmp/rootfs",
                "readonly": true
            },
            "process": {
                "args": ["/bin/server"],
                "env": ["PATH=/usr/bin"],
                "cwd": "/",
                "terminal": false
            },
            "resources": nimbus::SandboxResourceLimits::default(),
            "lifecycle": {
                "restart_policy": "never"
            },
            "port_bindings": [SandboxPortBinding::tcp("default", 18080, 8080)]
        },
        "conmon_layout": {
            "container_state_dir": container_dir,
            "ctr_log": container_dir.join("ctr.log"),
            "oci_log": container_dir.join("oci.log")
        },
        "last_exit_code": null,
        "shutdown_requested": false,
        "status": status
    });

    fs::write(
        container_dir.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest).expect("manifest should serialize"),
    )
    .expect("manifest should write");
}

fn machine_api_image_spec(tenant_id: &TenantId, service_name: &str) -> SandboxSpec {
    let mut spec = machine_api_rootfs_spec(tenant_id, service_name);
    spec.root = SandboxRootSpec::oci_image_reference("docker.io/library/busybox:latest");
    spec
}

fn machine_api_build_spec(tenant_id: &TenantId, service_name: &str) -> SandboxSpec {
    let mut spec = machine_api_rootfs_spec(tenant_id, service_name);
    spec.root = SandboxRootSpec::oci_image_build(
        format!("{service_name}:dev"),
        "/tmp/Dockerfile",
        "/tmp/context",
    );
    spec
}

fn machine_api_rootfs_spec(tenant_id: &TenantId, service_name: &str) -> SandboxSpec {
    SandboxSpec::new(
        tenant_id.clone(),
        SandboxOwnerSpec::service(service_name),
        SandboxBackendKind::Container,
        SandboxRootSpec::rootfs("/tmp/rootfs"),
        SandboxProcessSpec::new(["/bin/server"]),
    )
    .with_port_binding(SandboxPortBinding::tcp("default", 18080, 8080))
}

fn machine_api_sandbox_id(tenant_id: &TenantId, incarnation: &str) -> SandboxId {
    SandboxId::new(format!(
        "machine-api:{}",
        NetworkPlanId::for_tenant_workload_plan(tenant_id, incarnation)
    ))
}

#[derive(Debug, Default)]
struct RecordingStartBackend {
    started_sandboxes: Arc<Mutex<Vec<String>>>,
    stopped_sandboxes: Arc<Mutex<Vec<String>>>,
}

impl RecordingStartBackend {
    fn started_sandboxes(&self) -> Arc<Mutex<Vec<String>>> {
        Arc::clone(&self.started_sandboxes)
    }

    fn stopped_sandboxes(&self) -> Arc<Mutex<Vec<String>>> {
        Arc::clone(&self.stopped_sandboxes)
    }
}

impl SandboxBackend for RecordingStartBackend {
    fn kind(&self) -> SandboxBackendKind {
        SandboxBackendKind::Container
    }

    fn start(&self, spec: SandboxSpec) -> SandboxFuture<SandboxHandle> {
        let started_sandboxes = Arc::clone(&self.started_sandboxes);
        let tenant_id = spec.tenant_id.clone();
        let backend = spec.backend;
        let service_name = spec.display_name().to_owned();
        Box::pin(async move {
            started_sandboxes
                .lock()
                .expect("lock should acquire")
                .push(service_name.clone());
            Ok(SandboxHandle::new(
                tenant_id,
                SandboxId::new(format!("{service_name}-01aaa")),
                service_name,
                backend,
                SandboxStatus::Ready,
                Vec::new(),
            ))
        })
    }

    fn inspect(&self, _id: &SandboxId) -> SandboxFuture<Option<SandboxHandle>> {
        Box::pin(async move { Ok(None) })
    }

    fn stop(&self, id: &SandboxId) -> SandboxFuture<()> {
        let stopped_sandboxes = Arc::clone(&self.stopped_sandboxes);
        let sandbox_id = id.clone();
        Box::pin(async move {
            stopped_sandboxes
                .lock()
                .expect("lock should acquire")
                .push(sandbox_id.as_str().to_owned());
            Ok(())
        })
    }
}

#[derive(Debug, Clone)]
struct RefreshingInspectBackend {
    state_root: PathBuf,
    inspected_ids: Arc<Mutex<Vec<String>>>,
}

impl RefreshingInspectBackend {
    fn new(state_root: PathBuf) -> Self {
        Self {
            state_root,
            inspected_ids: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn inspected_ids(&self) -> Arc<Mutex<Vec<String>>> {
        Arc::clone(&self.inspected_ids)
    }
}

impl SandboxBackend for RefreshingInspectBackend {
    fn kind(&self) -> SandboxBackendKind {
        SandboxBackendKind::Container
    }

    fn start(&self, spec: SandboxSpec) -> SandboxFuture<SandboxHandle> {
        let message = format!(
            "test refresh backend expects inspect only, not start for {}",
            spec.display_name()
        );
        Box::pin(async move { Err(SandboxError::InvalidSpec { message }) })
    }

    fn inspect(&self, id: &SandboxId) -> SandboxFuture<Option<SandboxHandle>> {
        let state_root = self.state_root.clone();
        let sandbox_id = id.clone();
        let inspected_ids = Arc::clone(&self.inspected_ids);
        Box::pin(async move {
            inspected_ids
                .lock()
                .expect("lock should acquire")
                .push(sandbox_id.as_str().to_owned());
            let endpoints = vec![PublishedEndpoint::new(
                "default",
                EndpointProtocol::Tcp,
                SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 18080),
            )];
            write_container_manifest(
                &state_root,
                sandbox_id.as_str(),
                "svc-demo",
                "demo",
                SandboxStatus::Ready,
                endpoints.clone(),
            );
            Ok(Some(SandboxHandle::new(
                nimbus::TenantId::new("svc-demo").expect("tenant id should parse"),
                sandbox_id,
                "demo",
                SandboxBackendKind::Container,
                SandboxStatus::Ready,
                endpoints,
            )))
        })
    }

    fn stop(&self, _id: &SandboxId) -> SandboxFuture<()> {
        Box::pin(async move { Ok(()) })
    }
}

struct BlockedNodeWorkloadFacade;

impl MachineApiNodeWorkloadFacade for BlockedNodeWorkloadFacade {
    fn kind(&self) -> SandboxBackendKind {
        SandboxBackendKind::Container
    }

    fn service_execution_blockers(&self) -> Vec<String> {
        vec!["guest node lifecycle backend unavailable: systemd D-Bus is unavailable".to_owned()]
    }

    fn start<'a>(
        &'a self,
        _sandbox_id: SandboxId,
        _spec: SandboxSpec,
    ) -> super::service_workloads::MachineApiServiceFuture<'a, SandboxHandle> {
        Box::pin(async move {
            Err(MachineApiHttpError {
                status: StatusCode::SERVICE_UNAVAILABLE,
                message: "blocked test facade should not start workloads".to_owned(),
            })
        })
    }

    fn inspect<'a>(
        &'a self,
        _id: &'a SandboxId,
    ) -> super::service_workloads::MachineApiServiceFuture<'a, Option<SandboxHandle>> {
        Box::pin(async move {
            Err(MachineApiHttpError {
                status: StatusCode::SERVICE_UNAVAILABLE,
                message: "blocked test facade should not inspect workloads".to_owned(),
            })
        })
    }

    fn stop<'a>(
        &'a self,
        _id: &'a SandboxId,
    ) -> super::service_workloads::MachineApiServiceFuture<'a, ()> {
        Box::pin(async move {
            Err(MachineApiHttpError {
                status: StatusCode::SERVICE_UNAVAILABLE,
                message: "blocked test facade should not stop workloads".to_owned(),
            })
        })
    }

    fn exposed_machine_port_receipts<'a>(
        &'a self,
        _id: &'a SandboxId,
    ) -> super::service_workloads::MachineApiServiceFuture<
        'a,
        Vec<nimbus_sandbox::MachinePortForwardReceipt>,
    > {
        Box::pin(async move {
            Err(MachineApiHttpError {
                status: StatusCode::SERVICE_UNAVAILABLE,
                message: "blocked test facade has no provider evidence".to_owned(),
            })
        })
    }

    fn absent_machine_port_receipts<'a>(
        &'a self,
        _id: &'a SandboxId,
    ) -> super::service_workloads::MachineApiServiceFuture<
        'a,
        Option<nimbus_sandbox::backends::container::MachinePortAbsenceEvidence>,
    > {
        Box::pin(async move {
            Err(MachineApiHttpError {
                status: StatusCode::SERVICE_UNAVAILABLE,
                message: "blocked test facade has no provider evidence".to_owned(),
            })
        })
    }
}
