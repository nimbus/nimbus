use nimbus_machine::{
    MachineConnectivityCapabilities, MachineConnectivityError, MachineConnectivityRequirements,
    MachineProvider,
};
use nimbus_network::{
    NetworkAttachmentCapabilitySet, NetworkAttachmentMode, NetworkCapabilityMismatch,
    NetworkControlPlaneLocality, NetworkExposure, NetworkExternalDependency, NetworkIsolationMode,
    NetworkManagementMode, NetworkSovereigntyCapabilities, NetworkSovereigntyRequirements,
};

fn requirements(
    management_mode: NetworkManagementMode,
    attachment_mode: NetworkAttachmentMode,
    isolation_mode: NetworkIsolationMode,
    exposure: NetworkExposure,
) -> MachineConnectivityRequirements {
    MachineConnectivityRequirements::new(
        NetworkAttachmentCapabilitySet::new(management_mode, [attachment_mode], [isolation_mode]),
        [exposure],
        NetworkSovereigntyRequirements::new(NetworkControlPlaneLocality::LocalOnly, [], true),
    )
}

#[test]
fn machine_network_management_modes_are_typed_and_exact() {
    assert_eq!(
        MachineProvider::Krunkit.network_management_mode(),
        NetworkManagementMode::NimbusHostManaged
    );
    assert_eq!(
        MachineProvider::Vfkit.network_management_mode(),
        NetworkManagementMode::NimbusHostManaged
    );
    assert_eq!(
        MachineProvider::Wsl2.network_management_mode(),
        NetworkManagementMode::ProviderManaged
    );
}

#[test]
fn host_managed_and_provider_managed_machine_modes_do_not_substitute() {
    let host_capabilities = MachineConnectivityCapabilities::new(
        NetworkAttachmentCapabilitySet::new(
            NetworkManagementMode::NimbusHostManaged,
            [NetworkAttachmentMode::VirtualMachineGuest],
            [NetworkIsolationMode::WorkloadNamespace],
        ),
        [NetworkExposure::Loopback],
        NetworkSovereigntyCapabilities::new(NetworkControlPlaneLocality::LocalOnly, [], true),
    );
    let provider_requirements = requirements(
        NetworkManagementMode::ProviderManaged,
        NetworkAttachmentMode::ProviderVirtualNetwork,
        NetworkIsolationMode::ProviderBoundary,
        NetworkExposure::Loopback,
    );

    let error = host_capabilities
        .ensure_satisfied(MachineProvider::Krunkit, &provider_requirements)
        .expect_err("host-managed evidence must not masquerade as provider-managed");

    assert_eq!(error.provider(), MachineProvider::Krunkit);
    assert_eq!(
        error.mismatches(),
        &[
            NetworkCapabilityMismatch::ManagementMode {
                required: NetworkManagementMode::ProviderManaged,
                offered: NetworkManagementMode::NimbusHostManaged,
            },
            NetworkCapabilityMismatch::AttachmentMode {
                required: NetworkAttachmentMode::ProviderVirtualNetwork,
            },
            NetworkCapabilityMismatch::IsolationMode {
                required: NetworkIsolationMode::ProviderBoundary,
            },
        ]
    );

    let provider_capabilities = MachineConnectivityCapabilities::new(
        NetworkAttachmentCapabilitySet::new(
            NetworkManagementMode::ProviderManaged,
            [NetworkAttachmentMode::ProviderVirtualNetwork],
            [NetworkIsolationMode::ProviderBoundary],
        ),
        [NetworkExposure::Loopback],
        NetworkSovereigntyCapabilities::new(NetworkControlPlaneLocality::LocalOnly, [], true),
    );
    let host_requirements = requirements(
        NetworkManagementMode::NimbusHostManaged,
        NetworkAttachmentMode::VirtualMachineGuest,
        NetworkIsolationMode::WorkloadNamespace,
        NetworkExposure::Loopback,
    );

    host_capabilities
        .ensure_satisfied(MachineProvider::Krunkit, &host_requirements)
        .expect("exact host-managed requirements should pass");
    let reverse_error = provider_capabilities
        .ensure_satisfied(MachineProvider::Wsl2, &host_requirements)
        .expect_err("provider-managed evidence must not masquerade as host-managed");
    assert_eq!(
        reverse_error.mismatches(),
        &[
            NetworkCapabilityMismatch::ManagementMode {
                required: NetworkManagementMode::NimbusHostManaged,
                offered: NetworkManagementMode::ProviderManaged,
            },
            NetworkCapabilityMismatch::AttachmentMode {
                required: NetworkAttachmentMode::VirtualMachineGuest,
            },
            NetworkCapabilityMismatch::IsolationMode {
                required: NetworkIsolationMode::WorkloadNamespace,
            },
        ]
    );
}

#[test]
fn machine_connectivity_rejects_exposure_isolation_and_sovereignty_in_order() {
    let insufficient = MachineConnectivityCapabilities::new(
        NetworkAttachmentCapabilitySet::new(
            NetworkManagementMode::NimbusHostManaged,
            [NetworkAttachmentMode::VirtualMachineGuest],
            [NetworkIsolationMode::WorkloadNamespace],
        ),
        [NetworkExposure::Loopback],
        NetworkSovereigntyCapabilities::new(
            NetworkControlPlaneLocality::OperatorLocal,
            [NetworkExternalDependency::ExternalControlPlane],
            false,
        ),
    );
    let required = MachineConnectivityRequirements::new(
        NetworkAttachmentCapabilitySet::new(
            NetworkManagementMode::NimbusHostManaged,
            [NetworkAttachmentMode::VirtualMachineGuest],
            [NetworkIsolationMode::TenantSegment],
        ),
        [NetworkExposure::Public],
        NetworkSovereigntyRequirements::new(NetworkControlPlaneLocality::LocalOnly, [], true),
    );

    let error = insufficient
        .ensure_satisfied(MachineProvider::Vfkit, &required)
        .expect_err("unproven connectivity and sovereignty must reject");

    assert_eq!(
        error.mismatches(),
        &[
            NetworkCapabilityMismatch::IsolationMode {
                required: NetworkIsolationMode::TenantSegment,
            },
            NetworkCapabilityMismatch::Exposure {
                required: NetworkExposure::Public,
            },
            NetworkCapabilityMismatch::ControlPlaneLocality {
                maximum_allowed: NetworkControlPlaneLocality::LocalOnly,
                offered: NetworkControlPlaneLocality::OperatorLocal,
            },
            NetworkCapabilityMismatch::ExternalDependency {
                disallowed: NetworkExternalDependency::ExternalControlPlane,
            },
            NetworkCapabilityMismatch::OfflineRestart {
                required: true,
                offered: false,
            },
        ]
    );
}

#[cfg(target_os = "macos")]
#[test]
fn krunkit_and_vfkit_report_equal_conservative_macos_capabilities() {
    let krunkit = MachineProvider::Krunkit
        .connectivity_capabilities()
        .expect("krunkit connectivity evidence should be available on macOS");
    let vfkit = MachineProvider::Vfkit
        .connectivity_capabilities()
        .expect("vfkit connectivity evidence should be available on macOS");

    assert_eq!(krunkit, vfkit);
    assert_eq!(
        krunkit.attachment().management_mode(),
        NetworkManagementMode::NimbusHostManaged
    );
    assert_eq!(
        krunkit.attachment().attachment_modes(),
        &[NetworkAttachmentMode::VirtualMachineGuest]
            .into_iter()
            .collect()
    );
    assert_eq!(
        krunkit.attachment().isolation_modes(),
        &[NetworkIsolationMode::WorkloadNamespace]
            .into_iter()
            .collect()
    );
    assert_eq!(
        krunkit.exposures(),
        &[NetworkExposure::Loopback].into_iter().collect()
    );
    assert_eq!(
        krunkit.sovereignty().control_plane_locality(),
        NetworkControlPlaneLocality::LocalOnly
    );
    assert!(
        krunkit
            .sovereignty()
            .required_external_dependencies()
            .is_empty()
    );
    assert!(krunkit.sovereignty().offline_restart_supported());
}

#[cfg(not(target_os = "macos"))]
#[test]
fn apple_vmm_connectivity_evidence_is_unavailable_off_macos() {
    for provider in [MachineProvider::Krunkit, MachineProvider::Vfkit] {
        let error = provider
            .connectivity_capabilities()
            .expect_err("Apple VMM connectivity evidence must not be advertised off macOS");
        assert!(matches!(
            error,
            MachineConnectivityError::ProviderUnavailable { provider: rejected }
                if rejected == provider
        ));
    }
}

#[test]
fn wsl2_has_provider_managed_topology_but_no_capability_evidence() {
    let error = MachineProvider::Wsl2
        .connectivity_capabilities()
        .expect_err("WSL2 has no available Nimbus networking adapter yet");

    assert!(matches!(
        error,
        MachineConnectivityError::ProviderUnavailable {
            provider: MachineProvider::Wsl2
        }
    ));
    assert_eq!(
        error.to_string(),
        "the WSL2 machine provider has no available connectivity capability evidence on this host"
    );
}
