use nimbus_machine::api::{
    MACHINE_API_WORKLOAD_TEARDOWN_PHASE_OPERATION, MachineApiCapabilityResponse,
    MachineApiOperationStatus,
};
use nimbus_network::NetworkResourceGeneration;
use nimbus_node::{
    HostLifecycleFuture, SystemdDbusClient, SystemdInspectUnitRequest,
    SystemdStartTransientUnitRequest, SystemdStartTransientUnitResponse, SystemdStopUnitRequest,
    SystemdStopUnitResponse, SystemdTransientCapabilities, SystemdTransientUnitBackend,
    SystemdUnitStatus, UnavailableSystemdDbusClient,
};
use nimbus_sandbox::backends::container::{
    ContainerSandboxBackend, ContainerSandboxBackendConfig, OciMachinePortForwarderConfig,
};

use super::*;
use crate::machine::api::capabilities::machine_api_capability_response;
use crate::machine::api::state::machine_systemd_teardown_state_root;
use crate::machine::api::{MachineApiListenMode, MachineApiState};

#[derive(Clone, Copy)]
struct CapabilitySystemdClient;

impl SystemdDbusClient for CapabilitySystemdClient {
    fn capabilities(&self) -> SystemdTransientCapabilities {
        SystemdTransientCapabilities::available()
    }

    fn start_transient_unit<'a>(
        &'a self,
        _request: SystemdStartTransientUnitRequest,
    ) -> HostLifecycleFuture<'a, SystemdStartTransientUnitResponse> {
        Box::pin(async move {
            Err(Error::Internal(
                "capability-only systemd fixture must not start units".to_owned(),
            ))
        })
    }

    fn stop_unit<'a>(
        &'a self,
        _request: SystemdStopUnitRequest,
    ) -> HostLifecycleFuture<'a, SystemdStopUnitResponse> {
        Box::pin(async move {
            Err(Error::Internal(
                "capability-only systemd fixture must not stop units".to_owned(),
            ))
        })
    }

    fn inspect_unit<'a>(
        &'a self,
        _request: SystemdInspectUnitRequest,
    ) -> HostLifecycleFuture<'a, SystemdUnitStatus> {
        Box::pin(async move {
            Err(Error::Internal(
                "capability-only systemd fixture must not inspect units".to_owned(),
            ))
        })
    }
}

fn service_with_backend<C>(
    root: &Path,
    backend: SystemdTransientUnitBackend<C>,
) -> GuestNodeWorkloadService
where
    C: SystemdDbusClient,
{
    let container_root = root.join("container");
    let bundle_materializer = Arc::new(ContainerSandboxBackend::new(
        ContainerSandboxBackendConfig::plan_only(
            container_root.join("bundles"),
            container_root.join("state"),
        ),
    ));
    GuestNodeWorkloadService::new(
        NodeIdentity::new("guest-systemd-composition")
            .expect("test guest node identity should be valid"),
        backend,
        bundle_materializer,
        container_root.join("state"),
    )
}

#[test]
fn guest_systemd_teardown_state_root_is_deterministic_and_scoped() {
    let control_data_dir = Path::new("/var/lib/nimbus/machines/default/control");
    let first = machine_systemd_teardown_state_root(control_data_dir);
    let second = machine_systemd_teardown_state_root(control_data_dir);

    assert_eq!(first, second);
    assert_eq!(
        first,
        control_data_dir
            .join("service-sandboxes")
            .join("systemd")
            .join("teardown")
    );
    assert!(first.starts_with(control_data_dir));
}

#[test]
fn guest_systemd_provider_views_share_one_concrete_backend() {
    let root = tempfile::tempdir().expect("guest control root should create");
    let teardown_root = machine_systemd_teardown_state_root(root.path());
    let backend = SystemdTransientUnitBackend::new_with_teardown_state_root(
        UnavailableSystemdDbusClient::new("composition identity test"),
        &teardown_root,
    )
    .expect("durable teardown store should open");
    let service = service_with_backend(root.path(), backend);

    assert!(service.provider_views_share_one_backend());
    assert!(
        service
            .teardown_provider_blockers()
            .iter()
            .all(|blocker| !blocker.contains("teardown state store")),
        "a durable store must remove only the store-specific blocker"
    );
}

#[test]
fn guest_systemd_storeless_composition_reports_exact_teardown_blocker() {
    let root = tempfile::tempdir().expect("guest control root should create");
    let backend = SystemdTransientUnitBackend::unavailable("test host has no systemd manager");
    let service = service_with_backend(root.path(), backend);

    assert!(
        service
            .teardown_provider_blockers()
            .iter()
            .any(|blocker| blocker.contains("durable systemd teardown state store is unavailable"))
    );
    assert!(
        service
            .teardown_provider_blockers()
            .iter()
            .any(|blocker| blocker.contains("systemd D-Bus is unavailable"))
    );
    assert!(
        service
            .service_execution_blockers()
            .iter()
            .all(|blocker| !blocker.contains("teardown state store")),
        "teardown state must not disable otherwise-independent lifecycle reporting"
    );
}

#[test]
fn guest_teardown_capability_names_each_missing_or_crossed_composition_authority() {
    let root = tempfile::tempdir().expect("guest control root should create");
    let teardown_root = machine_systemd_teardown_state_root(root.path());
    let backend = SystemdTransientUnitBackend::new_with_teardown_state_root(
        UnavailableSystemdDbusClient::new("capability advertisement test"),
        teardown_root,
    )
    .expect("durable teardown store should open");
    let service = Arc::new(service_with_backend(root.path(), backend));
    let capabilities = machine_api_capability_response(&MachineApiState {
        control_data_dir: root.path().to_path_buf(),
        listen_mode: MachineApiListenMode::DirectSocket,
        binary_lookup_path: None,
        helper_binary_dirs: Vec::new(),
        service_workloads: Some(service),
        machine_port_forwarder: None,
        forwarder_authority: None,
    });

    assert!(
        capabilities
            .supported_operations
            .iter()
            .all(|operation| operation != MACHINE_API_WORKLOAD_TEARDOWN_PHASE_OPERATION)
    );
    let teardown = capabilities
        .operation_statuses
        .iter()
        .find(|status| status.name == MACHINE_API_WORKLOAD_TEARDOWN_PHASE_OPERATION)
        .expect("teardown readiness should be reported without advertising support");
    assert!(!teardown.available);
    assert!(teardown.blockers.iter().any(|blocker| {
        blocker == "workload-teardown.phase requires parent-issued machine forwarder authority"
    }));
    assert!(teardown.blockers.iter().any(|blocker| {
        blocker == "workload-teardown.phase requires installed machine port forwarder configuration"
    }));
    assert!(
        teardown
            .blockers
            .iter()
            .any(|blocker| blocker.contains("systemd D-Bus is unavailable"))
    );

    let listener = std::net::TcpListener::bind(("127.0.0.1", 0))
        .expect("capability forwarder probe should bind");
    let port = listener
        .local_addr()
        .expect("capability listener should have an address")
        .port();
    let config = OciMachinePortForwarderConfig::for_provider_instance(
        "127.0.0.1",
        port,
        "/services/forwarder",
        "capability-forwarder",
        NetworkResourceGeneration::new(3),
    )
    .expect("capability forwarder should validate");
    let authority = MachineForwarderAuthority::new(
        config.provider_instance().clone(),
        config.provider_generation(),
    );
    let available_backend = SystemdTransientUnitBackend::new_with_teardown_state_root(
        CapabilitySystemdClient,
        machine_systemd_teardown_state_root(root.path()),
    )
    .expect("durable available systemd composition should open");
    let available_service = Arc::new(service_with_backend(root.path(), available_backend));
    let ready_state = MachineApiState {
        control_data_dir: root.path().to_path_buf(),
        listen_mode: MachineApiListenMode::DirectSocket,
        binary_lookup_path: None,
        helper_binary_dirs: Vec::new(),
        service_workloads: Some(available_service),
        machine_port_forwarder: Some(config.clone()),
        forwarder_authority: Some(authority.clone()),
    };
    let ready = machine_api_capability_response(&ready_state);
    let ready_status = teardown_status(&ready);
    assert!(ready_status.available, "{:?}", ready_status.blockers);
    assert!(
        ready
            .supported_operations
            .iter()
            .any(|operation| operation == MACHINE_API_WORKLOAD_TEARDOWN_PHASE_OPERATION)
    );

    let mut missing_config = ready_state.clone();
    missing_config.machine_port_forwarder = None;
    assert_teardown_blocked_by(
        &missing_config,
        "requires installed machine port forwarder configuration",
    );

    let mut missing_authority = ready_state.clone();
    missing_authority.forwarder_authority = None;
    assert_teardown_blocked_by(
        &missing_authority,
        "requires parent-issued machine forwarder authority",
    );

    let mut crossed = ready_state;
    crossed.forwarder_authority = Some(MachineForwarderAuthority::new(
        authority.provider_instance().clone(),
        NetworkResourceGeneration::new(authority.generation().as_u64() + 1),
    ));
    assert_teardown_blocked_by(&crossed, "configuration is crossed");
}

#[test]
fn guest_teardown_capability_reports_container_journal_unavailable_without_effects() {
    let root = tempfile::tempdir().expect("guest control root should create");
    let backend = SystemdTransientUnitBackend::new_with_teardown_state_root(
        CapabilitySystemdClient,
        machine_systemd_teardown_state_root(root.path()),
    )
    .expect("durable systemd teardown store should open");
    let bundle_materializer = Arc::new(ContainerSandboxBackend::new(
        ContainerSandboxBackendConfig::plan_only(root.path().join("bundles"), "/"),
    ));
    let service = GuestNodeWorkloadService::new(
        NodeIdentity::new("guest-container-journal-capability")
            .expect("test guest node identity should validate"),
        backend,
        bundle_materializer,
        root.path().join("state"),
    );

    assert!(service.teardown_execution_blockers().iter().any(|blocker| {
        blocker.contains("guest Container provider journal unavailable")
            && blocker.contains("cannot be the filesystem root")
    }));
    assert!(
        !root.path().join("bundles").exists(),
        "capability inspection must not create Container artifacts"
    );
}

fn teardown_status(capabilities: &MachineApiCapabilityResponse) -> &MachineApiOperationStatus {
    capabilities
        .operation_statuses
        .iter()
        .find(|status| status.name == MACHINE_API_WORKLOAD_TEARDOWN_PHASE_OPERATION)
        .expect("teardown capability status should exist")
}

fn assert_teardown_blocked_by(state: &MachineApiState, expected: &str) {
    let capabilities = machine_api_capability_response(state);
    let status = teardown_status(&capabilities);
    assert!(!status.available);
    assert!(
        status
            .blockers
            .iter()
            .any(|blocker| blocker.contains(expected)),
        "expected blocker {expected:?} in {:?}",
        status.blockers
    );
    assert!(
        capabilities
            .supported_operations
            .iter()
            .all(|operation| operation != MACHINE_API_WORKLOAD_TEARDOWN_PHASE_OPERATION)
    );
}
