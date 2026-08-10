use nimbus_machine::api::MACHINE_API_WORKLOAD_TEARDOWN_PHASE_OPERATION;
use nimbus_node::{SystemdTransientUnitBackend, UnavailableSystemdDbusClient};
use nimbus_sandbox::backends::container::{ContainerSandboxBackend, ContainerSandboxBackendConfig};

use super::*;
use crate::machine::api::capabilities::machine_api_capability_response;
use crate::machine::api::state::machine_systemd_teardown_state_root;
use crate::machine::api::{MachineApiListenMode, MachineApiState};

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
fn guest_systemd_composition_reports_teardown_unavailable_until_the_sink_exists() {
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
    assert!(
        teardown
            .blockers
            .iter()
            .any(|blocker| blocker.contains("no strict teardown-phase sink"))
    );
    assert!(
        teardown
            .blockers
            .iter()
            .any(|blocker| blocker.contains("systemd D-Bus is unavailable"))
    );
}
