use nimbus_engine::Engine;
use nimbus_network::{
    NetworkAddressFamily, NetworkBindRealmKind, NetworkControlPlaneLocality, NetworkExposure,
    NetworkForwardingFeature, NetworkIngressFeature, NetworkLifecycleFeature,
    NetworkPortAssignmentMode, NetworkProviderId, PortProtocol,
};
#[cfg(target_os = "linux")]
use nimbus_network::{
    NetworkAttachmentCapabilitySet, NetworkAttachmentMode, NetworkCapabilityBundle,
    NetworkCapabilityRegistry, NetworkCapabilityRequirements, NetworkEndpointCapabilitySet,
    NetworkForwardingCapabilitySet, NetworkIngressCapabilitySet, NetworkIsolationMode,
    NetworkLifecycleCapabilitySet, NetworkManagementMode, NetworkSovereigntyRequirements,
};
#[cfg(target_os = "linux")]
use nimbus_sandbox::backends::container::{
    ContainerSandboxBackend, ContainerSandboxBackendConfig, ContainerStartMode,
};
#[cfg(target_os = "linux")]
use nimbus_sandbox::backends::krun::{KrunSandboxBackend, KrunSandboxBackendConfig, KrunStartMode};
use nimbus_server::{ServeOptions, TlsConfig, nimbus_owned_local_ingress_registration};
use nimbus_testing::EngineFixture;
#[cfg(target_os = "linux")]
use tempfile::tempdir;

fn set<T: Ord, const N: usize>(values: [T; N]) -> std::collections::BTreeSet<T> {
    values.into_iter().collect()
}

#[test]
fn local_ingress_registration_reuses_listener_identity_and_is_conservative() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let registration = ServeOptions::reconstruct_direct(fixture.engine())
        .expect("test server network authority should reconstruct once")
        .nimbus_owned_local_ingress_registration();
    assert_eq!(
        registration,
        nimbus_owned_local_ingress_registration(false),
        "ServeOptions must delegate to the same effect-free server-owned capability source"
    );

    assert_eq!(
        registration.provider_id(),
        &NetworkProviderId::for_registration_key("nimbus-server.tcp-listener")
    );
    assert_eq!(
        registration.endpoint().address_families(),
        &set([NetworkAddressFamily::Ipv4, NetworkAddressFamily::Ipv6])
    );
    assert_eq!(
        registration.endpoint().bind_realms(),
        &set([NetworkBindRealmKind::Host])
    );
    assert_eq!(
        registration.endpoint().exposures(),
        &set([
            NetworkExposure::Loopback,
            NetworkExposure::Private,
            NetworkExposure::Public,
        ])
    );
    assert_eq!(
        registration.endpoint().protocols(),
        &set([PortProtocol::Tcp])
    );
    assert_eq!(
        registration.endpoint().port_assignment_modes(),
        &set([
            NetworkPortAssignmentMode::Exact,
            NetworkPortAssignmentMode::ProviderAssigned,
        ])
    );
    assert_eq!(
        registration.ingress().features(),
        &set([
            NetworkIngressFeature::PathRouting,
            NetworkIngressFeature::WebSocket,
            NetworkIngressFeature::Streaming,
        ])
    );
    assert!(
        !registration
            .ingress()
            .features()
            .contains(&NetworkIngressFeature::TlsTermination)
    );
    assert!(
        !registration
            .ingress()
            .features()
            .contains(&NetworkIngressFeature::HostRouting)
    );
    assert!(registration.forwarding().features().is_empty());
    assert!(
        !registration
            .forwarding()
            .features()
            .contains(&NetworkForwardingFeature::PortForwarding)
    );
    assert!(
        !registration
            .forwarding()
            .features()
            .contains(&NetworkForwardingFeature::ConnectionDrain)
    );
    assert_eq!(
        registration.lifecycle().features(),
        &set([
            NetworkLifecycleFeature::DurableInspect,
            NetworkLifecycleFeature::Reconcile,
            NetworkLifecycleFeature::Delete,
        ])
    );
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
    assert!(registration.sovereignty().offline_restart_supported());

    assert!(
        !registration
            .endpoint()
            .protocols()
            .contains(&PortProtocol::Udp)
    );
    assert!(
        !registration
            .endpoint()
            .bind_realms()
            .contains(&NetworkBindRealmKind::ProvenIsolated)
    );
    assert!(
        !registration
            .endpoint()
            .port_assignment_modes()
            .contains(&NetworkPortAssignmentMode::NimbusAllocatedRange)
    );
}

#[test]
fn tls_is_advertised_only_by_the_configured_local_ingress_instance() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let registration = ServeOptions::reconstruct_direct(fixture.engine())
        .expect("test server network authority should reconstruct once")
        .with_tls(TlsConfig::new("local-cert.pem", "local-key.pem"))
        .nimbus_owned_local_ingress_registration();

    assert_eq!(
        registration.ingress().features(),
        &set([
            NetworkIngressFeature::PathRouting,
            NetworkIngressFeature::TlsTermination,
            NetworkIngressFeature::WebSocket,
            NetworkIngressFeature::Streaming,
        ])
    );
}

#[cfg(target_os = "linux")]
#[test]
fn real_container_and_krun_pairs_select_with_real_server_ingress() {
    let root = tempdir().expect("temporary sandbox root");
    let mut container_config =
        ContainerSandboxBackendConfig::under_root(root.path().join("container"));
    container_config.start_mode = ContainerStartMode::Execute;
    let mut krun_config = KrunSandboxBackendConfig::under_root(root.path().join("krun"));
    krun_config.start_mode = KrunStartMode::Execute;

    let container = ContainerSandboxBackend::new(container_config)
        .host_managed_attachment_registration()
        .expect("real container Execute composition should register on Linux");
    let krun = KrunSandboxBackend::new(krun_config)
        .host_managed_attachment_registration()
        .expect("real krun Execute composition should register on Linux");
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let ingress = ServeOptions::reconstruct_direct(fixture.engine())
        .expect("test server network authority should reconstruct once")
        .nimbus_owned_local_ingress_registration();

    let container_bundle = NetworkCapabilityBundle::new(container, ingress.clone());
    let krun_bundle = NetworkCapabilityBundle::new(krun, ingress);
    let container_selection = container_bundle.selection();
    let krun_selection = krun_bundle.selection();
    let registry = NetworkCapabilityRegistry::new([container_bundle, krun_bundle])
        .expect("both real local provider pairs should register");
    let requirements = NetworkCapabilityRequirements::new(
        NetworkAttachmentCapabilitySet::new(
            NetworkManagementMode::NimbusHostManaged,
            [NetworkAttachmentMode::IsolatedNamespace],
            [
                NetworkIsolationMode::WorkloadNamespace,
                NetworkIsolationMode::TenantSegment,
            ],
        ),
        NetworkEndpointCapabilitySet::new(
            [NetworkAddressFamily::Ipv4],
            [NetworkBindRealmKind::Host],
            [NetworkExposure::Loopback],
            [PortProtocol::Tcp],
            [NetworkPortAssignmentMode::ProviderAssigned],
        ),
        NetworkIngressCapabilitySet::new([
            NetworkIngressFeature::PathRouting,
            NetworkIngressFeature::WebSocket,
            NetworkIngressFeature::Streaming,
        ]),
        NetworkForwardingCapabilitySet::new([]),
        NetworkLifecycleCapabilitySet::new([
            NetworkLifecycleFeature::DurableInspect,
            NetworkLifecycleFeature::Reconcile,
            NetworkLifecycleFeature::Delete,
        ]),
        NetworkSovereigntyRequirements::new(NetworkControlPlaneLocality::LocalOnly, [], true),
    );

    for selection in [container_selection, krun_selection] {
        let selected = registry
            .select_exact(&selection, &requirements)
            .expect("the exact real local provider pair should satisfy");
        assert_eq!(selected.selection(), selection);
    }
}
