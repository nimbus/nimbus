#[cfg(target_os = "linux")]
use std::collections::BTreeSet;

#[cfg(target_os = "linux")]
use nimbus_network::{
    LocalNetworkStateStore, NetworkAddressFamily, NetworkAttachmentMode,
    NetworkControlPlaneLocality, NetworkIsolationMode, NetworkLifecycleFeature,
    NetworkManagementMode, NetworkProviderId, NetworkResourceGeneration,
};
use nimbus_sandbox::backends::container::{ContainerSandboxBackend, ContainerSandboxBackendConfig};
#[cfg(target_os = "linux")]
use nimbus_sandbox::backends::container::{ContainerStartMode, OciMachinePortForwarderConfig};
#[cfg(target_os = "linux")]
use nimbus_sandbox::backends::krun::KrunStartMode;
use nimbus_sandbox::backends::krun::{KrunSandboxBackend, KrunSandboxBackendConfig};
use nimbus_sandbox::backends::{
    CONTAINER_HOST_MANAGED_ATTACHMENT_PROVIDER_KEY, KRUN_HOST_MANAGED_ATTACHMENT_PROVIDER_KEY,
    SandboxAttachmentRegistrationError,
};
use tempfile::tempdir;

#[test]
fn plan_only_backends_do_not_register_execute_attachment_capabilities() {
    let root = tempdir().expect("temporary sandbox root");
    let container = ContainerSandboxBackend::new(ContainerSandboxBackendConfig::plan_only(
        root.path().join("container-bundles"),
        root.path().join("container-state"),
    ));
    let krun = KrunSandboxBackend::new(KrunSandboxBackendConfig::plan_only(
        root.path().join("krun-bundles"),
        root.path().join("krun-state"),
    ));

    let container_error = container
        .host_managed_attachment_registration()
        .expect_err("container PlanOnly must not advertise Execute capabilities");
    assert_eq!(
        container_error,
        SandboxAttachmentRegistrationError::PlanOnly {
            provider_key: CONTAINER_HOST_MANAGED_ATTACHMENT_PROVIDER_KEY,
        }
    );
    assert_eq!(
        container_error.to_string(),
        "host-managed attachment registration nimbus-sandbox.container.host-managed-attachment is unavailable: PlanOnly does not own Execute effects"
    );

    let krun_error = krun
        .host_managed_attachment_registration()
        .expect_err("krun PlanOnly must not advertise Execute capabilities");
    assert_eq!(
        krun_error,
        SandboxAttachmentRegistrationError::PlanOnly {
            provider_key: KRUN_HOST_MANAGED_ATTACHMENT_PROVIDER_KEY,
        }
    );
    assert_eq!(
        krun_error.to_string(),
        "host-managed attachment registration nimbus-sandbox.krun.host-managed-attachment is unavailable: PlanOnly does not own Execute effects"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn execute_backends_register_distinct_conservative_attachment_facts() {
    let root = tempdir().expect("temporary sandbox root");
    let mut container_config =
        ContainerSandboxBackendConfig::under_root(root.path().join("container"));
    container_config.start_mode = ContainerStartMode::Execute;
    let mut krun_config = KrunSandboxBackendConfig::under_root(root.path().join("krun"));
    krun_config.start_mode = KrunStartMode::Execute;

    let container = ContainerSandboxBackend::new(container_config)
        .host_managed_attachment_registration()
        .expect("container Execute mode should register on Linux");
    let krun = KrunSandboxBackend::new(krun_config)
        .host_managed_attachment_registration()
        .expect("krun Execute mode should register on Linux");

    assert_eq!(
        container.provider_id(),
        &NetworkProviderId::for_registration_key(CONTAINER_HOST_MANAGED_ATTACHMENT_PROVIDER_KEY)
    );
    assert_eq!(
        krun.provider_id(),
        &NetworkProviderId::for_registration_key(KRUN_HOST_MANAGED_ATTACHMENT_PROVIDER_KEY)
    );
    assert_ne!(container.provider_id(), krun.provider_id());

    assert_eq!(
        container.attachment().management_mode(),
        NetworkManagementMode::NimbusHostManaged
    );
    assert_eq!(
        container.attachment().attachment_modes(),
        &BTreeSet::from([NetworkAttachmentMode::IsolatedNamespace])
    );
    assert_eq!(
        krun.attachment().management_mode(),
        NetworkManagementMode::NimbusHostManaged
    );
    assert_eq!(
        krun.attachment().attachment_modes(),
        &BTreeSet::from([
            NetworkAttachmentMode::IsolatedNamespace,
            NetworkAttachmentMode::VirtualMachineGuest,
        ])
    );
    let expected_isolation = BTreeSet::from([
        NetworkIsolationMode::WorkloadNamespace,
        NetworkIsolationMode::TenantSegment,
    ]);
    assert_eq!(
        container.attachment().isolation_modes(),
        &expected_isolation
    );
    assert_eq!(krun.attachment().isolation_modes(), &expected_isolation);

    let expected_address_families = BTreeSet::from([NetworkAddressFamily::Ipv4]);
    assert_eq!(container.address_families(), &expected_address_families);
    assert_eq!(krun.address_families(), &expected_address_families);

    let expected_lifecycle = BTreeSet::from([
        NetworkLifecycleFeature::DurableInspect,
        NetworkLifecycleFeature::Reconcile,
        NetworkLifecycleFeature::Delete,
    ]);
    assert_eq!(container.lifecycle().features(), &expected_lifecycle);
    assert_eq!(krun.lifecycle().features(), &expected_lifecycle);

    for registration in [&container, &krun] {
        assert_eq!(
            registration.sovereignty().control_plane_locality(),
            NetworkControlPlaneLocality::LocalOnly
        );
        assert!(
            registration
                .sovereignty()
                .required_external_dependencies()
                .is_empty()
        );
        assert!(
            registration.sovereignty().offline_restart_supported(),
            "already-materialized local attachment state must reconcile offline"
        );
    }
}

#[cfg(target_os = "linux")]
#[test]
fn container_machine_forwarder_mode_is_a_different_unregistered_composition() {
    let root = tempdir().expect("temporary sandbox root");
    let mut config = ContainerSandboxBackendConfig::under_root(root.path());
    config.machine_port_forwarder = Some(
        OciMachinePortForwarderConfig::gvproxy_for_provider_instance(
            "capability-registration-test",
            NetworkResourceGeneration::new(1),
        )
        .expect("fixture provider identity should be valid"),
    );
    let backend = ContainerSandboxBackend::new(config);

    let error = backend
        .host_managed_attachment_registration()
        .expect_err("machine forwarding must not inherit the local attachment report");
    assert_eq!(
        error,
        SandboxAttachmentRegistrationError::MachinePortForwarderConfigured {
            provider_key: CONTAINER_HOST_MANAGED_ATTACHMENT_PROVIDER_KEY,
        }
    );
    assert_eq!(
        error.to_string(),
        "host-managed attachment registration nimbus-sandbox.container.host-managed-attachment is unavailable: container machine forwarding is a different attachment composition"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn cached_startup_reconciliation_failures_refuse_backend_registrations() {
    let root = tempdir().expect("temporary sandbox root");
    let container_config = ContainerSandboxBackendConfig::under_root(root.path().join("container"));
    let krun_config = KrunSandboxBackendConfig::under_root(root.path().join("krun"));
    for state_root in [
        &container_config.network_state_root,
        &krun_config.network_state_root,
    ] {
        let authority_path = LocalNetworkStateStore::authority_path_for(state_root);
        std::fs::create_dir_all(
            authority_path
                .parent()
                .expect("network authority path should have a parent"),
        )
        .expect("network authority parent should create");
        std::fs::write(&authority_path, b"{")
            .expect("corrupt network authority fixture should write");
    }

    let container = ContainerSandboxBackend::new(container_config);
    let krun = KrunSandboxBackend::new(krun_config);

    for (result, provider_key) in [
        (
            container.host_managed_attachment_registration(),
            CONTAINER_HOST_MANAGED_ATTACHMENT_PROVIDER_KEY,
        ),
        (
            krun.host_managed_attachment_registration(),
            KRUN_HOST_MANAGED_ATTACHMENT_PROVIDER_KEY,
        ),
    ] {
        let error = result.expect_err("cached reconciliation failure must refuse registration");
        match error {
            SandboxAttachmentRegistrationError::StartupReconciliationFailed {
                provider_key: actual_provider_key,
                reason,
            } => {
                assert_eq!(actual_provider_key, provider_key);
                assert!(
                    reason.contains("startup network reconciliation failed"),
                    "refusal must retain the cached reconciliation diagnostic: {reason}"
                );
                assert!(
                    reason.contains("network authority"),
                    "refusal must name the failed durable authority boundary: {reason}"
                );
            }
            other => panic!("expected startup reconciliation refusal, got {other:?}"),
        }
    }
}

#[cfg(not(target_os = "linux"))]
#[test]
fn execute_backends_do_not_advertise_linux_attachment_effects_on_this_target() {
    let root = tempdir().expect("temporary sandbox root");
    let container = ContainerSandboxBackend::new(ContainerSandboxBackendConfig::under_root(
        root.path().join("container"),
    ));
    let krun = KrunSandboxBackend::new(KrunSandboxBackendConfig::under_root(
        root.path().join("krun"),
    ));

    let container_error = container
        .host_managed_attachment_registration()
        .expect_err("container Execute must not advertise Linux effects on this target");
    assert_eq!(
        container_error,
        SandboxAttachmentRegistrationError::UnsupportedTarget {
            provider_key: CONTAINER_HOST_MANAGED_ATTACHMENT_PROVIDER_KEY,
            target_os: std::env::consts::OS,
        }
    );
    assert_eq!(
        container_error.to_string(),
        format!(
            "host-managed attachment registration nimbus-sandbox.container.host-managed-attachment is unavailable on target {}: Execute attachments require Linux",
            std::env::consts::OS
        )
    );

    let krun_error = krun
        .host_managed_attachment_registration()
        .expect_err("krun Execute must not advertise Linux effects on this target");
    assert_eq!(
        krun_error,
        SandboxAttachmentRegistrationError::UnsupportedTarget {
            provider_key: KRUN_HOST_MANAGED_ATTACHMENT_PROVIDER_KEY,
            target_os: std::env::consts::OS,
        }
    );
    assert_eq!(
        krun_error.to_string(),
        format!(
            "host-managed attachment registration nimbus-sandbox.krun.host-managed-attachment is unavailable on target {}: Execute attachments require Linux",
            std::env::consts::OS
        )
    );
}
