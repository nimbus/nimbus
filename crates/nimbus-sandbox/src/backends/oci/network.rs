use std::path::Path;
use std::time::Duration;

use nimbus_network::{NetworkAttachmentId, NetworkSegmentAllocator};

use crate::error::SandboxError;
use crate::instance::SandboxId;

mod attachment_lifecycle;
mod cluster;
mod dto;
mod egress_pin;
mod finality;
mod forwarding;
mod ipam;
mod layout;
mod netavark;
mod netns;
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "NNC5.2b stages the read-only collector consumed by NNC5.2d startup fencing"
    )
)]
mod orphan_evidence;
mod placement;
mod process;
mod provider_locator;
mod proxy;
mod realization;
mod reaper;
mod segment;
#[cfg(test)]
mod test_support;

pub(crate) use attachment_lifecycle::{
    AttachmentAttachAuthority, AttachmentAuxiliaryDisposition, AttachmentBackendKind,
    AttachmentDetachFailure, AttachmentDetachFailureStage, AttachmentTeardownMode,
    OciAttachmentAdapter, OciAttachmentAuxiliaryListener, OciAttachmentInput,
    OciAttachmentLifecycle, OciHostManagedAttachmentBackend, OciMachineForwardedAttachmentBackend,
};
pub(crate) use egress_pin::pin_netns_egress_to_own_proxy;
pub(crate) use finality::{TerminalNetworkAuthoritySet, TerminalNetworkFinalityEvidence};
pub use forwarding::{
    MachinePortForwardOutcome, MachinePortForwardReceipt, OciMachinePortForwarderConfig,
};
pub(crate) use forwarding::{expose_machine_ports, unexpose_machine_ports};
pub(crate) use ipam::{
    OciIpamAuthority, deallocate_container_ips_after_confirmed_detach,
    reconcile_terminal_container_ipam_releases, retire_terminal_container_ipam_release,
};
#[cfg(test)]
pub(crate) use ipam::{allocate_container_ips, begin_netavark_setup_without_ack_for_test};
pub(crate) use layout::{
    OciNetworkConfig, OciNetworkDirectEgress, OciNetworkLayout, bridge_gateway_addr,
};
pub(crate) use netavark::{
    OciNetavarkOperation, authenticate_container_network_generation,
    authenticate_container_network_generation_for_cleanup,
};
#[cfg(test)]
pub(crate) use netavark::{setup_container_network, teardown_container_network};
pub(crate) use netns::{create_persistent_network_namespace, remove_persistent_network_namespace};
pub(crate) use placement::{OciPlacementProvider, place_sandbox_on_block};
pub(crate) use process::{
    MachinePortProxyCleanupDisposition, MachinePortProxyCleanupState, MachinePortProxyEntries,
    MachinePortProxyEntry, MachinePortProxyKey, MachinePortProxyLeaseAuthority,
    MachinePortProxyLifetimeRegistry, MachinePortProxyRegistration,
};
pub use process::{OciNetworkProcess, OciNetworkProcessError};
pub(crate) use proxy::{
    MachinePortPreparationReleaseAuthority, MachinePortProxy, MachinePortProxyRoute,
    machine_port_proxy_routes, prepare_machine_port_proxies_with_release_authority,
    start_machine_port_proxies_with_recovery,
};
#[cfg(test)]
pub(crate) use proxy::{
    panicking_machine_port_proxy_for_test, prepare_machine_port_proxies, start_machine_port_proxies,
};
pub(crate) use realization::OciSegmentRealization;
pub(crate) use reaper::{
    ReservedNetworkLaunchAuthority, compensate_reserved_network_launch_after_ports,
    quarantine_network_segment_hold, reconcile_network_segment_orphans,
    release_network_segment_hold, release_reserved_network_launch_after_ports,
};
#[cfg(test)]
pub(crate) use segment::SingleNodeSegmentAllocator;
pub(crate) use segment::{ConfiguredSegmentAllocator, DEFAULT_TENANT_PREFIX};
#[cfg(test)]
pub(crate) use test_support::{
    RecordingSegmentAllocator, SegmentAllocatorOperation, direct_test_ipam_authority,
    direct_test_port_authority,
};

/// OCI-family specialization of the portable segment allocation capability.
pub(crate) type OciSegmentAllocator =
    dyn NetworkSegmentAllocator<Segment = OciSegmentRealization, Error = SandboxError>;

/// Stable name of the sole OCI workload attachment currently realized.
///
/// Multi-homing can add named siblings without changing workload identity or
/// inheriting a previous incarnation's attachment hold.
pub(crate) const DEFAULT_ATTACHMENT_NAME: &str = "default";

pub(crate) fn default_network_attachment_id(sandbox_id: &SandboxId) -> NetworkAttachmentId {
    NetworkAttachmentId::for_workload_attachment(sandbox_id.as_str(), DEFAULT_ATTACHMENT_NAME)
}

/// Reconcile every node-local network authority before admitting new work.
///
/// Both passes run so independent cleanup can still converge, but any failure
/// is returned as one fail-closed admission diagnostic. Backends retain that
/// diagnostic for their lifetime; cleanup and inspection remain available,
/// while planning and provider launch effects require a fresh backend whose
/// startup reconciliation completed.
pub(crate) fn reconcile_startup_network_state(
    workload_state_root: &Path,
    ipam_authority: &OciIpamAuthority,
    allocator: &OciSegmentAllocator,
) -> crate::error::Result<()> {
    let mut errors = Vec::new();
    if let Err(error) =
        reconcile_terminal_container_ipam_releases(ipam_authority, workload_state_root)
    {
        errors.push(error.to_string());
    }
    if let Err(error) = reconcile_network_segment_orphans(workload_state_root, allocator) {
        errors.push(error.to_string());
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(SandboxError::OperationFailed {
            message: format!(
                "startup network reconciliation failed under {}: {}",
                ipam_authority.state_root().display(),
                errors.join("; ")
            ),
        })
    }
}

#[cfg(test)]
pub(crate) fn inspect_container_ips(
    ipam_authority: &OciIpamAuthority,
    layout: &OciNetworkLayout,
    sandbox_id: &SandboxId,
) -> crate::error::Result<Vec<std::net::Ipv4Addr>> {
    ipam::load_container_ips(ipam_authority, layout, sandbox_id)
}

pub(crate) const DEFAULT_NETAVARK_BINARY: &str = "netavark";
pub(crate) const DEFAULT_AARDVARK_DNS_BINARY: &str = "aardvark-dns";
pub(crate) const DEFAULT_NETWORK_NAME: &str = "nimbus";
pub(crate) const DEFAULT_NETWORK_INTERFACE: &str = "nimbus0";
pub(crate) const DEFAULT_NETWORK_SUBNET: &str = "10.89.0.0/24";
pub(crate) const DEFAULT_MACHINE_FORWARDER_HOST: &str = "gateway.containers.internal";
pub(crate) const DEFAULT_MACHINE_FORWARDER_PORT: u16 = 80;
pub(crate) const DEFAULT_MACHINE_FORWARDER_PATH: &str = "/services/forwarder";

const DEFAULT_CONTAINER_INTERFACE_NAME: &str = "eth0";
const DEFAULT_NETWORK_ID: &str = "5e9b4c62f9f3e8b8d2c74b7388d8451f5e9b4c62f9f3e8b8d2c74b7388d8451f";
const NETAVARK_OPTION_NO_DEFAULT_ROUTE: &str = "no_default_route";
const NETAVARK_OPTION_ISOLATE: &str = "isolate";
const MACHINE_FORWARDER_TIMEOUT: Duration = Duration::from_secs(2);
const MACHINE_PORT_PROXY_ACCEPT_SLEEP: Duration = Duration::from_millis(50);
const MACHINE_PORT_PROXY_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

#[cfg(test)]
use crate::backends::oci::port_lifecycle::machine_port_proxy_guest_listener_addr;
#[cfg(test)]
use forwarding::machine_forward_remote;
#[cfg(test)]
use ipam::{load_container_ips, parse_ipv4_subnet_and_gateway};
#[cfg(test)]
use netavark::{
    build_bridge_network, build_netavark_request, netavark_path_env, netavark_port_bindings,
    render_netavark_failure,
};

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::fs;
    use std::io::{Read, Write};
    use std::net::{IpAddr, Ipv4Addr, TcpListener, TcpStream};
    use std::thread;
    use std::time::Duration;

    use nimbus_core::TenantId;
    use nimbus_network::{NetworkResourceGeneration, NetworkSegmentId};
    use tempfile::tempdir;

    use super::{
        DEFAULT_MACHINE_FORWARDER_HOST, DEFAULT_MACHINE_FORWARDER_PATH,
        DEFAULT_MACHINE_FORWARDER_PORT, NETAVARK_OPTION_NO_DEFAULT_ROUTE,
        OciMachinePortForwarderConfig, OciNetavarkOperation, OciNetworkConfig,
        OciNetworkDirectEgress, OciNetworkLayout, allocate_container_ips,
        authenticate_container_network_generation, build_bridge_network, build_netavark_request,
        deallocate_container_ips_after_confirmed_detach, direct_test_ipam_authority,
        load_container_ips, machine_forward_remote, machine_port_proxy_guest_listener_addr,
        netavark_path_env, netavark_port_bindings, parse_ipv4_subnet_and_gateway,
        prepare_machine_port_proxies, render_netavark_failure, setup_container_network,
        start_machine_port_proxies, teardown_container_network,
    };
    use crate::backend::SandboxBackendKind;
    use crate::backends::oci::port_lifecycle::OciPortLeaseCoordinator;
    use crate::error::SandboxError;
    use crate::instance::SandboxId;
    use crate::spec::{
        SandboxOwnerSpec, SandboxPortBinding, SandboxProcessSpec, SandboxRootSpec,
        SandboxRootfsSpec, SandboxSpec,
    };

    fn sample_spec() -> SandboxSpec {
        SandboxSpec::new(
            TenantId::new("svc-demo").expect("tenant should parse"),
            SandboxOwnerSpec::service("db"),
            SandboxBackendKind::Container,
            SandboxRootSpec::Rootfs(SandboxRootfsSpec::new("/tmp/rootfs")),
            SandboxProcessSpec::new(["/bin/sh", "-c", "sleep 60"]),
        )
    }

    fn netavark_operation<'a>(
        layout: &'a OciNetworkLayout,
        config: &'a OciNetworkConfig,
        sandbox_id: &'a SandboxId,
        name: &'a str,
    ) -> OciNetavarkOperation<'a> {
        OciNetavarkOperation::new(layout, config, sandbox_id, name, name, &[], None)
    }

    #[test]
    fn netavark_request_preserves_host_ip_without_machine_forwarding() {
        let request = build_netavark_request(
            &OciNetworkConfig::default(),
            &crate::instance::SandboxId::new("db-01"),
            "db",
            "db",
            &[],
            &[SandboxPortBinding::tcp("http", 18080, 8080)],
            false,
        )
        .expect("request should build");

        assert_eq!(request.port_mappings.len(), 1);
        assert_eq!(request.port_mappings[0].host_ip, "127.0.0.1");
        assert_eq!(request.port_mappings[0].host_port, 18080);
        assert_eq!(request.port_mappings[0].container_port, 8080);
        assert!(request.network_info.contains_key("nimbus"));
        assert!(
            !request.network_info["nimbus"].internal,
            "default-deny networks must stay non-internal so netavark can install published-port firewall rules"
        );
        assert_eq!(
            request.network_info["nimbus"].options[NETAVARK_OPTION_NO_DEFAULT_ROUTE], "true",
            "default-deny networks should omit the container default route instead of disabling netavark firewall setup"
        );
        assert_eq!(
            request.network_info["nimbus"].labels["io.nimbus.egress.direct"],
            "deny"
        );
    }

    #[test]
    fn krun_deny_network_route_denies_offsubnet_dns_and_carries_no_resolver_stub() {
        // DNS-containment invariant for the krun microVM network. Two layers
        // hold the line:
        //
        //  1. `no_default_route` leaves the netns with a route only to its own
        //     bridge subnet, so a guest's direct UDP/TCP :53 to an arbitrary
        //     external resolver (e.g. 8.8.8.8) has no route and is denied at the
        //     kernel before any packet leaves — off-subnet DNS-exfil cannot
        //     leave the namespace.
        //  2. `enable_dns: false` (the krun backend's `network_config`) stops
        //     netavark from starting the in-subnet aardvark-dns stub on the
        //     bridge gateway `:53`. That stub was the residual DNS-exfil channel
        //     KME5 flagged and would also collide when two krun sandboxes share
        //     a subnet; with no resolver present at all, "DNS contained"
        //     strengthens from route-deny-only to no-resolver-present.
        //
        // Legitimate name resolution flows through the HTTP_PROXY (the host-side
        // PEP resolves), never the guest's own stub.
        let config = OciNetworkConfig {
            direct_egress: OciNetworkDirectEgress::Deny,
            enable_dns: false,
            ..OciNetworkConfig::default()
        };
        let request = build_netavark_request(
            &config,
            &crate::instance::SandboxId::new("dns-deny-01"),
            "db",
            "db",
            &[],
            &[],
            false,
        )
        .expect("request should build");

        assert_eq!(
            request.network_info["nimbus"].options[NETAVARK_OPTION_NO_DEFAULT_ROUTE], "true",
            "a deny-by-default network must omit the container default route so off-subnet :53 has no route"
        );
        assert!(
            !request.network_info["nimbus"].dns_enabled,
            "the krun network must not start an in-subnet aardvark-dns resolver stub on the bridge gateway :53"
        );
        assert!(
            request.network_info["nimbus"]
                .network_dns_servers
                .is_empty(),
            "a deny-by-default network must not advertise external DNS servers the guest could exfil through"
        );
        assert!(
            request.dns_servers.is_empty(),
            "a deny-by-default network must not hand the guest external resolvers"
        );
    }

    #[test]
    fn build_bridge_network_dns_enabled_mirrors_enable_dns_flag() {
        // `build_bridge_network` must thread `enable_dns` straight through to the
        // netavark `dns_enabled` toggle: false suppresses the aardvark stub (the
        // krun shape), true keeps it (the container default).
        let krun_shaped = OciNetworkConfig {
            enable_dns: false,
            ..OciNetworkConfig::default()
        };
        let krun_network =
            build_bridge_network(&krun_shaped).expect("krun bridge network should build");
        assert!(
            !krun_network.dns_enabled,
            "enable_dns=false must emit dns_enabled=false so netavark starts no aardvark stub"
        );

        let container_shaped = OciNetworkConfig::default();
        assert!(
            container_shaped.enable_dns,
            "the default config keeps DNS on so container behavior is unchanged"
        );
        let container_network =
            build_bridge_network(&container_shaped).expect("container bridge network should build");
        assert!(
            container_network.dns_enabled,
            "enable_dns=true must emit dns_enabled=true so container DNS behavior is unchanged"
        );
    }

    #[test]
    fn build_bridge_network_isolates_every_tenant_bridge() {
        // MTN5: every per-tenant bridge sets the netavark `isolate` option, which
        // installs a FORWARD DROP between networks — a guest cannot route to a
        // sibling tenant's /24 even though all tenant bridges share the host root
        // netns with ip_forward on. Carried regardless of DNS shape.
        for config in [
            OciNetworkConfig::default(),
            OciNetworkConfig {
                enable_dns: false,
                ..OciNetworkConfig::default()
            },
        ] {
            let network = build_bridge_network(&config).expect("bridge network should build");
            assert_eq!(
                network.options.get("isolate").map(String::as_str),
                Some("true"),
                "every tenant bridge must set the netavark isolate option"
            );
        }
    }

    #[test]
    fn netavark_port_bindings_are_omitted_when_machine_forwarding_is_enabled() {
        let bindings = vec![SandboxPortBinding::tcp("http", 18080, 8080)];
        let forwarder = OciMachinePortForwarderConfig::gvproxy_for_provider_instance(
            "network-test-gvproxy",
            NetworkResourceGeneration::new(1),
        )
        .expect("test gvproxy lifecycle identity should validate");

        assert!(
            netavark_port_bindings(&bindings, Some(&forwarder)).is_empty(),
            "machine mode publishes through the runner-owned guest listener, not netavark host-port DNAT"
        );
        assert_eq!(netavark_port_bindings(&bindings, None), bindings);
    }

    #[test]
    fn krun_inbound_dnat_publishes_only_on_operator_requested_host_address() {
        // KME4 inbound DNAT reconciliation. The krun backend stands up its
        // published-port DNAT through `setup_container_network` with
        // `machine_port_forwarder = None`, i.e. `strip_host_ip = false`. That
        // makes netavark bind the host side of the DNAT on exactly the
        // operator-requested `host_address` (default 127.0.0.1) — the same
        // address the libkrun TSI inbound bind defaults to (the `15bcf49`
        // host_address=127.0.0.1 default) — and never a wildcard the operator
        // did not request. The container-side DNAT target is the bridge IP at
        // the guest port, so a published port is reachable from the host via
        // DNAT and only at the requested address.
        let config = OciNetworkConfig::default();
        let assigned = [Ipv4Addr::new(10, 89, 0, 5)];
        let id = crate::instance::SandboxId::new("db-01");

        // Default binding: published on host loopback only, DNAT'd to the bridge
        // IP at the guest port.
        let default_binding = vec![SandboxPortBinding::tcp("pg", 15432, 5432)];
        let request =
            build_netavark_request(&config, &id, "db", "db", &assigned, &default_binding, false)
                .expect("krun (machine_port_forwarder=None) request should build");
        assert_eq!(request.port_mappings.len(), 1);
        assert_eq!(
            request.port_mappings[0].host_ip, "127.0.0.1",
            "krun DNAT host side must bind the operator-requested loopback default"
        );
        assert_eq!(request.port_mappings[0].host_port, 15432);
        assert_eq!(
            request.port_mappings[0].container_port, 5432,
            "the DNAT target is the bridge IP at the guest port"
        );

        // An operator-chosen host address flows through verbatim and is never
        // blanked (blank host_ip == 0.0.0.0 == every host interface).
        let explicit = vec![
            SandboxPortBinding::tcp("http", 18080, 8080)
                .with_host_address(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 5))),
        ];
        let request = build_netavark_request(&config, &id, "db", "db", &assigned, &explicit, false)
            .expect("explicit-host request should build");
        assert_eq!(request.port_mappings[0].host_ip, "127.0.0.5");
        assert!(
            !request.port_mappings[0].host_ip.is_empty(),
            "krun DNAT must never publish on a host address the operator did not request"
        );

        // Contrast: only the machine-forwarder path (`strip_host_ip = true`)
        // blanks the host_ip to a wildcard, and the krun backend never selects it.
        let stripped =
            build_netavark_request(&config, &id, "db", "db", &assigned, &default_binding, true)
                .expect("machine-forwarder request should build");
        assert_eq!(
            stripped.port_mappings[0].host_ip, "",
            "the wildcard host_ip is reachable only via the machine forwarder, which krun never uses"
        );
    }

    #[test]
    fn netavark_request_preserves_explicit_direct_egress_allow_when_requested() {
        let config = OciNetworkConfig {
            direct_egress: OciNetworkDirectEgress::Allow,
            ..OciNetworkConfig::default()
        };

        let request = build_netavark_request(
            &config,
            &crate::instance::SandboxId::new("db-01"),
            "db",
            "db",
            &[],
            &[],
            false,
        )
        .expect("request should build");

        assert!(
            !request.network_info["nimbus"].internal,
            "explicit direct egress allow should keep the bridge non-internal"
        );
        assert!(
            !request.network_info["nimbus"]
                .options
                .contains_key(NETAVARK_OPTION_NO_DEFAULT_ROUTE),
            "explicit direct egress allow should keep the container default route"
        );
        assert_eq!(
            request.network_info["nimbus"].labels["io.nimbus.egress.direct"],
            "allow"
        );
    }

    #[test]
    fn bridge_subnet_parser_rejects_broadcast_base_without_overflow() {
        let error = parse_ipv4_subnet_and_gateway("10.0.0.255/24")
            .expect_err("broadcast-address subnet base should be rejected");

        assert!(matches!(
            error,
            SandboxError::InvalidSpec { message }
                if message.contains("address must be the network address for /24")
        ));
    }

    #[test]
    fn bridge_subnet_parser_rejects_prefixes_without_gateway_and_container_space() {
        let error = parse_ipv4_subnet_and_gateway("10.0.0.0/31")
            .expect_err("/31 bridge subnet should not have enough host space");

        assert!(matches!(
            error,
            SandboxError::InvalidSpec { message }
                if message.contains("must leave room for gateway and container addresses")
        ));
    }

    #[test]
    fn bridge_subnet_parser_accepts_smallest_gateway_and_container_subnet() {
        let (subnet, gateway) = parse_ipv4_subnet_and_gateway("10.0.0.0/30")
            .expect("/30 has one gateway and one container address");

        assert_eq!(subnet, "10.0.0.0/30");
        assert_eq!(gateway, "10.0.0.1");
    }

    #[test]
    fn machine_forwarder_requires_explicit_lifecycle_provider_context() {
        let config = OciMachinePortForwarderConfig::gvproxy_for_provider_instance(
            "machine-alpha-gvproxy",
            NetworkResourceGeneration::new(7),
        )
        .expect("lifecycle-issued provider context should validate");
        let reconstructed = OciMachinePortForwarderConfig::gvproxy_for_provider_instance(
            "machine-alpha-gvproxy",
            NetworkResourceGeneration::new(7),
        )
        .expect("same lifecycle-issued provider context should validate");
        assert_eq!(config.host, DEFAULT_MACHINE_FORWARDER_HOST);
        assert_eq!(config.port, DEFAULT_MACHINE_FORWARDER_PORT);
        assert_eq!(config.path_prefix, DEFAULT_MACHINE_FORWARDER_PATH);
        assert_eq!(config.provider_generation().as_u64(), 7);
        assert_eq!(
            config.provider_instance(),
            reconstructed.provider_instance(),
            "backend construction must not mint identity for the shared gvproxy endpoint"
        );
        let durable = serde_json::to_vec(&config).expect("forwarder config should serialize");
        assert_eq!(
            serde_json::from_slice::<OciMachinePortForwarderConfig>(&durable)
                .expect("forwarder config should deserialize"),
            config,
            "the manifest-carried config must preserve exact provider instance and generation"
        );
    }

    #[test]
    fn machine_forwarder_uses_gvproxy_inferred_vm_remote_for_loopback_bindings() {
        let binding = SandboxPortBinding::tcp("http", 18080, 8080);

        assert_eq!(machine_forward_remote(&binding), ":18080");
    }

    #[test]
    fn machine_forwarder_preserves_gvproxy_inferred_remote_for_non_loopback_bindings() {
        let binding = SandboxPortBinding::tcp("http", 18080, 8080)
            .with_host_address(IpAddr::V4(Ipv4Addr::UNSPECIFIED));

        assert_eq!(machine_forward_remote(&binding), ":18080");
    }

    #[test]
    fn machine_port_proxy_binds_internal_ipv4_wildcard() {
        let binding = SandboxPortBinding::tcp("http", 18080, 8080);

        assert_eq!(
            machine_port_proxy_guest_listener_addr(&binding),
            "0.0.0.0:18080".parse().expect("socket addr should parse"),
            "gvproxy targets the guest VM address, so the guest listener must not reuse the \
             external publication address"
        );
    }

    #[test]
    fn machine_port_proxy_forwards_tcp_to_container_endpoint() {
        let target =
            TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("target listener should bind");
        let target_port = target
            .local_addr()
            .expect("target address should be available")
            .port();
        let proxy_port = unused_local_port();
        let target_thread = thread::spawn(move || {
            let (mut stream, _) = target.accept().expect("target should accept connection");
            let mut request = [0_u8; 4];
            stream
                .read_exact(&mut request)
                .expect("target should read proxy request");
            assert_eq!(&request, b"ping");
            stream
                .write_all(b"pong")
                .expect("target should write proxy response");
        });

        let binding = SandboxPortBinding::tcp("http", proxy_port, target_port);
        let state = tempdir().expect("network state root");
        let manager = OciPortLeaseCoordinator::new(state.path(), proxy_port..=proxy_port)
            .with_machine_port_proxy_bindings();
        let tenant = TenantId::new("machine-proxy-test").expect("tenant id");
        let sandbox_id = SandboxId::new("machine-proxy-test");
        let reservation_claim = crate::backends::oci::port_lease::new_launch_reservation_claim()
            .expect("machine proxy test claim should mint");
        let mut reservations = manager
            .reserve_launch_ports_for_sandbox(
                crate::backends::oci::port_lifecycle::SandboxLaunchPortPlan::new(
                    &tenant,
                    &sandbox_id,
                    std::slice::from_ref(&binding),
                    &[],
                ),
                &reservation_claim,
            )
            .expect("machine port should reserve");
        reservations
            .confirm_manifest_published()
            .expect("fixture should publish its exact launch request set");
        let prepared = prepare_machine_port_proxies(
            &tenant,
            &sandbox_id,
            &[Ipv4Addr::LOCALHOST],
            std::slice::from_ref(&binding),
            &reservations.published_leases,
            &manager,
        )
        .expect("machine port proxy socket should prepare");
        manager
            .activate_machine_bindings_with_lifetimes(
                &tenant,
                &sandbox_id,
                std::slice::from_ref(&binding),
                &reservations.published_leases,
                prepared.bind_authority(),
            )
            .expect("machine port proxy binding should activate");
        let proxies = start_machine_port_proxies(
            &tenant,
            &sandbox_id,
            std::slice::from_ref(&binding),
            &reservations.published_leases,
            &manager,
            prepared,
        )
        .expect("active machine port proxy should start");
        let mut stream = connect_with_retry(proxy_port);
        stream
            .write_all(b"ping")
            .expect("client should write request");
        let mut response = [0_u8; 4];
        stream
            .read_exact(&mut response)
            .expect("client should read response");

        assert_eq!(&response, b"pong");
        let (mut proxies, _bind_authority) = proxies.into_parts();
        for proxy in &mut proxies {
            proxy
                .shutdown()
                .expect("machine port proxy should stop after draining connections");
        }
        target_thread
            .join()
            .expect("target thread should finish cleanly");
    }

    fn unused_local_port() -> u16 {
        let listener =
            TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("ephemeral listener should bind");
        listener
            .local_addr()
            .expect("ephemeral address should be available")
            .port()
    }

    fn connect_with_retry(port: u16) -> TcpStream {
        let address = (Ipv4Addr::LOCALHOST, port);
        let mut last_error = None;
        for _ in 0..20 {
            match TcpStream::connect(address) {
                Ok(stream) => return stream,
                Err(error) => {
                    last_error = Some(error);
                    thread::sleep(Duration::from_millis(25));
                }
            }
        }
        panic!(
            "proxy listener on 127.0.0.1:{port} did not accept connections: {:?}",
            last_error
        );
    }

    #[test]
    fn sample_spec_still_builds_cleanly() {
        let spec = sample_spec().with_port_binding(SandboxPortBinding::tcp("http", 18080, 8080));
        assert_eq!(spec.port_bindings.len(), 1);
    }

    #[test]
    fn netavark_failure_prefers_structured_stdout_error() {
        let rendered =
            render_netavark_failure(br#"{"error":"iptables helper binary not found"}"#, b"");
        assert_eq!(rendered, "iptables helper binary not found");
    }

    #[test]
    fn netavark_path_env_appends_usr_sbin_when_missing() {
        let rendered = netavark_path_env(Some(OsString::from("/usr/bin:/bin")));
        assert_eq!(rendered, OsString::from("/usr/bin:/bin:/usr/sbin"));
    }

    #[test]
    fn netavark_path_env_preserves_existing_usr_sbin() {
        let rendered = netavark_path_env(Some(OsString::from("/usr/bin:/usr/sbin:/bin")));
        assert_eq!(rendered, OsString::from("/usr/bin:/usr/sbin:/bin"));
    }

    #[test]
    fn failed_netavark_setup_retains_ipam_for_ambiguous_detach_reconciliation() {
        let temp_dir = tempdir().expect("temp dir should create");
        let tenant_id = TenantId::new("tenant-netavark-failure").expect("tenant should parse");
        let sandbox_id = SandboxId::new("netavark-failure");
        let layout = OciNetworkLayout::under_root(temp_dir.path(), &tenant_id, &sandbox_id);
        let ipam_authority = direct_test_ipam_authority(&layout);
        layout
            .ensure_directories()
            .expect("network layout should exist");
        let config = OciNetworkConfig {
            netavark_path: "/usr/bin/false".into(),
            ..OciNetworkConfig::default()
        };
        allocate_container_ips(&ipam_authority, &layout, &config, &sandbox_id)
            .expect("placement should reserve IPAM before Netavark setup");

        setup_container_network(
            &ipam_authority,
            &netavark_operation(&layout, &config, &sandbox_id, "netavark-failure"),
        )
        .expect_err("injected Netavark setup failure should remain ambiguous");

        assert!(
            load_container_ips(&ipam_authority, &layout, &sandbox_id).is_ok(),
            "ambiguous provider setup must retain exact IPAM until detach is confirmed"
        );
    }

    #[test]
    fn status_removal_failure_keeps_projection_and_ipam_release_fenced() {
        let temp_dir = tempdir().expect("temp dir should create");
        let tenant_id = TenantId::new("tenant-netavark-status-fence").expect("tenant should parse");
        let sandbox_id = SandboxId::new("netavark-status-fence");
        let layout = OciNetworkLayout::under_root(temp_dir.path(), &tenant_id, &sandbox_id);
        let ipam_authority = direct_test_ipam_authority(&layout);
        layout
            .ensure_directories()
            .expect("network layout should exist");
        let config = OciNetworkConfig {
            netavark_path: "/usr/bin/true".into(),
            ..OciNetworkConfig::default()
        };
        let assigned = allocate_container_ips(&ipam_authority, &layout, &config, &sandbox_id)
            .expect("placement should reserve IPAM before provider cleanup");
        setup_container_network(
            &ipam_authority,
            &netavark_operation(&layout, &config, &sandbox_id, "netavark-status-fence"),
        )
        .expect("fixture setup should publish Ready provider authority");
        fs::remove_file(&layout.status_path).expect("setup status should remove");
        fs::create_dir(&layout.status_path)
            .expect("a directory at the status path should make file removal fail");

        let error = teardown_container_network(
            &ipam_authority,
            &netavark_operation(&layout, &config, &sandbox_id, "netavark-status-fence"),
        )
        .expect_err("status removal failure must keep projection completion fenced");
        assert!(
            error
                .to_string()
                .contains("failed to remove Netavark status projection"),
            "cleanup must name the failed observed-projection removal: {error}"
        );
        assert_eq!(
            load_container_ips(&ipam_authority, &layout, &sandbox_id)
                .expect("live IPAM must remain fenced"),
            assigned,
            "projection failure must not permit terminal IPAM release"
        );

        fs::remove_dir(&layout.status_path).expect("test status directory should remove");
        teardown_container_network(
            &ipam_authority,
            &netavark_operation(&layout, &config, &sandbox_id, "netavark-status-fence"),
        )
        .expect("retry should complete teardown after projection removal is possible");
        deallocate_container_ips_after_confirmed_detach(
            &ipam_authority,
            &layout,
            &sandbox_id,
            &config.reservation_claim,
            config.provider_kind(),
        )
        .expect("completed teardown may publish exact terminal IPAM evidence");
        teardown_container_network(
            &ipam_authority,
            &netavark_operation(&layout, &config, &sandbox_id, "netavark-status-fence"),
        )
        .expect("terminal replay should remain idempotent only while status stays absent");

        fs::write(&layout.status_path, b"replacement-status")
            .expect("a conflicting replacement status should create");
        let error = teardown_container_network(
            &ipam_authority,
            &netavark_operation(&layout, &config, &sandbox_id, "netavark-status-fence"),
        )
        .expect_err("terminal replay must reject a conflicting status projection");
        assert!(
            error
                .to_string()
                .contains("terminal OCI IPAM authority conflicts"),
            "terminal cleanup must name the conflicting observed projection: {error}"
        );
    }

    #[test]
    fn netns_metadata_error_cannot_confirm_netavark_detach() {
        let temp_dir = tempdir().expect("temp dir should create");
        let tenant_id =
            TenantId::new("tenant-netavark-netns-observation").expect("tenant should parse");
        let sandbox_id = SandboxId::new("netavark-netns-observation");
        let layout = OciNetworkLayout::under_root(temp_dir.path(), &tenant_id, &sandbox_id);
        let ipam_authority = direct_test_ipam_authority(&layout);
        layout
            .ensure_directories()
            .expect("network layout should exist");
        let config = OciNetworkConfig {
            netavark_path: "/usr/bin/true".into(),
            ..OciNetworkConfig::default()
        };
        let assigned = allocate_container_ips(&ipam_authority, &layout, &config, &sandbox_id)
            .expect("placement should reserve IPAM before provider cleanup");
        setup_container_network(
            &ipam_authority,
            &netavark_operation(&layout, &config, &sandbox_id, "netavark-netns-observation"),
        )
        .expect("fixture setup should publish Ready provider authority");
        fs::write(&layout.status_path, b"provider-status-sentinel")
            .expect("provider status should exist");
        fs::remove_dir(&layout.netns_root).expect("empty netns root should remove");
        fs::write(&layout.netns_root, b"not-a-directory")
            .expect("a non-directory parent should make namespace observation fail");

        let error = teardown_container_network(
            &ipam_authority,
            &netavark_operation(&layout, &config, &sandbox_id, "netavark-netns-observation"),
        )
        .expect_err("namespace metadata failure must keep provider detach unconfirmed");
        assert!(
            error
                .to_string()
                .contains("failed to inspect persistent network namespace"),
            "cleanup must distinguish observation failure from explicit absence: {error}"
        );
        assert_eq!(
            fs::read(&layout.status_path).expect("provider status should remain"),
            b"provider-status-sentinel",
            "observation failure must not remove the provider status projection"
        );
        assert_eq!(
            load_container_ips(&ipam_authority, &layout, &sandbox_id)
                .expect("live IPAM must remain fenced"),
            assigned,
            "observation failure must not permit terminal IPAM release"
        );
    }

    #[test]
    fn teardown_rejects_same_ip_from_different_stable_segment_before_provider_effect() {
        let temp_dir = tempdir().expect("temp dir should create");
        let tenant_id =
            TenantId::new("tenant-netavark-teardown-fence").expect("tenant should parse");
        let sandbox_id = SandboxId::new("netavark-teardown-fence");
        let layout = OciNetworkLayout::under_root(temp_dir.path(), &tenant_id, &sandbox_id);
        let ipam_authority = direct_test_ipam_authority(&layout);
        layout
            .ensure_directories()
            .expect("network layout should exist");
        let config = OciNetworkConfig::default();
        let assigned = allocate_container_ips(&ipam_authority, &layout, &config, &sandbox_id)
            .expect("placement should reserve exact stable-segment IPAM");
        fs::write(&layout.netns_path, b"netns").expect("netns marker should exist");
        fs::write(&layout.status_path, b"provider-status-sentinel")
            .expect("provider status should exist");
        let authority_before =
            load_container_ips(&ipam_authority, &layout, &sandbox_id).expect("IPAM should inspect");
        let mut stale = config.clone();
        stale.segment_id = NetworkSegmentId::generate().as_str().to_owned();
        stale.netavark_path = temp_dir.path().join("must-not-run-netavark-teardown");

        let error = teardown_container_network(
            &ipam_authority,
            &netavark_operation(&layout, &stale, &sandbox_id, "netavark-teardown-fence"),
        )
        .expect_err("stable segment mismatch must fail before provider teardown");
        let message = error.to_string();
        assert!(
            message.contains(&config.segment_id)
                && message.contains(&stale.segment_id)
                && message.contains("refusing to remap"),
            "teardown must identify both stable segments: {message}"
        );
        assert!(
            !message.contains("failed to run netavark teardown"),
            "stable identity rejection must precede the provider process: {message}"
        );
        assert_eq!(
            fs::read(&layout.status_path).expect("status should remain"),
            b"provider-status-sentinel"
        );
        assert_eq!(
            load_container_ips(&ipam_authority, &layout, &sandbox_id).expect("IPAM should remain"),
            authority_before,
            "failed stale teardown must not mutate durable IPAM"
        );
        assert_eq!(assigned, authority_before);
    }

    #[test]
    fn stale_generation_setup_and_teardown_fail_before_provider_or_projection_effects() {
        let temp_dir = tempdir().expect("temp dir should create");
        let tenant_id =
            TenantId::new("tenant-netavark-generation-fence").expect("tenant should parse");
        let sandbox_id = SandboxId::new("netavark-generation-fence");
        let layout = OciNetworkLayout::under_root(temp_dir.path(), &tenant_id, &sandbox_id);
        let ipam_authority = direct_test_ipam_authority(&layout);
        layout
            .ensure_directories()
            .expect("network layout should exist");
        let stale = OciNetworkConfig::default();
        allocate_container_ips(&ipam_authority, &layout, &stale, &sandbox_id)
            .expect("first generation should reserve IPAM");
        deallocate_container_ips_after_confirmed_detach(
            &ipam_authority,
            &layout,
            &sandbox_id,
            &stale.reservation_claim,
            stale.provider_kind(),
        )
        .expect("first generation should release exact IPAM");
        let mut current = stale.clone();
        current.reservation_claim =
            crate::backends::oci::port_lease::new_launch_reservation_claim()
                .expect("replacement claim should validate");
        allocate_container_ips(&ipam_authority, &layout, &current, &sandbox_id)
            .expect("replacement generation should reserve the same attachment");

        fs::write(&layout.netns_path, b"replacement-netns")
            .expect("replacement netns marker should exist");
        fs::write(&layout.status_path, b"replacement-status")
            .expect("replacement status should exist");
        let authority_path =
            nimbus_network::LocalNetworkStateStore::authority_path_for(&layout.network_state_root);
        let authority_before =
            fs::read(&authority_path).expect("replacement authority should be durable");
        let mut stale_provider = stale.clone();
        stale_provider.netavark_path = temp_dir.path().join("must-not-run-stale-netavark");

        for error in [
            setup_container_network(
                &ipam_authority,
                &netavark_operation(&layout, &stale_provider, &sandbox_id, "stale-generation"),
            )
            .expect_err("stale setup must fail before Netavark"),
            teardown_container_network(
                &ipam_authority,
                &netavark_operation(&layout, &stale_provider, &sandbox_id, "stale-generation"),
            )
            .expect_err("stale teardown must fail before Netavark"),
        ] {
            let message = error.to_string();
            assert!(
                message.contains("different launch coordinator"),
                "stale provider work must name the generation fence: {message}"
            );
            assert!(
                !message.contains("failed to run netavark"),
                "generation rejection must precede the provider process: {message}"
            );
        }
        assert_eq!(
            fs::read(&layout.netns_path).expect("replacement netns should remain"),
            b"replacement-netns"
        );
        assert_eq!(
            fs::read(&layout.status_path).expect("replacement status should remain"),
            b"replacement-status"
        );
        assert_eq!(
            fs::read(&authority_path).expect("authority should remain readable"),
            authority_before,
            "stale provider work must not rewrite network authority"
        );

        fs::remove_file(&layout.netns_path).expect("test should model absent old namespace");
        teardown_container_network(
            &ipam_authority,
            &netavark_operation(&layout, &stale_provider, &sandbox_id, "stale-generation"),
        )
        .expect_err("stale no-netns teardown must authenticate before projection removal");
        assert_eq!(
            fs::read(&layout.status_path).expect("replacement status should remain"),
            b"replacement-status",
            "the no-netns fast path must not delete a replacement projection"
        );

        authenticate_container_network_generation(&ipam_authority, &layout, &current, &sandbox_id)
            .expect("the exact replacement generation should authenticate");
        let conflict = teardown_container_network(
            &ipam_authority,
            &netavark_operation(&layout, &current, &sandbox_id, "current-generation"),
        )
        .expect_err("Reserved no-effect authority must reject a conflicting status projection");
        assert!(
            conflict.to_string().contains("no-effect")
                && conflict.to_string().contains("status projection"),
            "the exact generation must still fail closed on contradictory observed state: \
             {conflict}"
        );
        assert_eq!(
            fs::read(&layout.status_path).expect("conflicting status should remain"),
            b"replacement-status",
            "no-effect authority must not delete an unowned projection"
        );
        fs::remove_file(&layout.status_path)
            .expect("the fixture should explicitly reconcile contradictory observed state");
        teardown_container_network(
            &ipam_authority,
            &netavark_operation(&layout, &current, &sandbox_id, "current-generation"),
        )
        .expect("exact Reserved generation should converge after status reconciliation");
        assert!(
            !layout.status_path.exists(),
            "reconciled no-effect teardown must preserve status absence"
        );
    }

    #[test]
    fn netavark_setup_requires_pre_reserved_ipam_without_creating_it() {
        let temp_dir = tempdir().expect("temp dir should create");
        let tenant_id = TenantId::new("tenant-netavark-no-ipam").expect("tenant should parse");
        let sandbox_id = SandboxId::new("netavark-no-ipam");
        let layout = OciNetworkLayout::under_root(temp_dir.path(), &tenant_id, &sandbox_id);
        let ipam_authority = direct_test_ipam_authority(&layout);
        layout
            .ensure_directories()
            .expect("network layout should exist");
        let config = OciNetworkConfig::default();
        let store = nimbus_network::LocalNetworkStateStore::open(&layout.network_state_root)
            .expect("network authority should open");
        let authority_path = store.authority_path().to_path_buf();
        let before = std::fs::read(&authority_path).ok();

        let error = setup_container_network(
            &ipam_authority,
            &netavark_operation(&layout, &config, &sandbox_id, "netavark-no-ipam"),
        )
        .expect_err("provider setup must not allocate IPAM on demand");
        assert!(
            error
                .to_string()
                .contains("failed to find allocated container IPs"),
            "missing placement authority must fail before Netavark: {error}"
        );
        assert_eq!(
            std::fs::read(&authority_path).ok(),
            before,
            "failed setup observation must not create or rewrite an IPAM partition"
        );
        assert!(
            !layout.status_path.exists(),
            "provider status must remain absent when IPAM was never reserved"
        );
    }

    #[test]
    fn allocate_container_ips_reserves_and_loads_podman_style_static_ips() {
        let temp_dir = tempdir().expect("temp dir should create");
        let config = OciNetworkConfig::default();
        let tenant_id = TenantId::new("tenant-a").expect("tenant should parse");
        let first_id = crate::instance::SandboxId::new("db-01");
        let second_id = crate::instance::SandboxId::new("db-02");
        let layout = OciNetworkLayout::under_root(temp_dir.path(), &tenant_id, &first_id);
        let ipam_authority = direct_test_ipam_authority(&layout);

        let first = allocate_container_ips(&ipam_authority, &layout, &config, &first_id)
            .expect("first allocation should succeed");
        let second = allocate_container_ips(&ipam_authority, &layout, &config, &second_id)
            .expect("second allocation should succeed");

        assert_eq!(
            first,
            vec!["10.89.0.2".parse::<Ipv4Addr>().expect("IPv4 should parse")]
        );
        assert_eq!(
            second,
            vec!["10.89.0.3".parse::<Ipv4Addr>().expect("IPv4 should parse")]
        );
        assert_eq!(
            load_container_ips(&ipam_authority, &layout, &second_id)
                .expect("second allocation should load"),
            second
        );
    }

    #[test]
    fn allocate_container_ips_uses_only_container_slot_in_smallest_bridge_subnet() {
        let temp_dir = tempdir().expect("temp dir should create");
        let config = OciNetworkConfig {
            network_subnet: "10.0.0.0/30".to_owned(),
            ..OciNetworkConfig::default()
        };
        let tenant_id = TenantId::new("tenant-a").expect("tenant should parse");
        let first_id = crate::instance::SandboxId::new("db-01");
        let second_id = crate::instance::SandboxId::new("db-02");
        let layout = OciNetworkLayout::under_root(temp_dir.path(), &tenant_id, &first_id);
        let ipam_authority = direct_test_ipam_authority(&layout);

        let first = allocate_container_ips(&ipam_authority, &layout, &config, &first_id)
            .expect("single allocatable container address should succeed");
        let second = allocate_container_ips(&ipam_authority, &layout, &config, &second_id)
            .expect_err("gateway plus one container should exhaust a /30 subnet");

        assert_eq!(
            first,
            vec!["10.0.0.2".parse::<Ipv4Addr>().expect("IPv4 should parse")]
        );
        // Exhaustion is now a typed signal so block-aware placement (MTN6) can
        // grow an additional block instead of failing the launch.
        assert!(matches!(
            second,
            SandboxError::NetworkSubnetExhausted { subnet } if subnet == "10.0.0.0/30"
        ));
    }

    #[test]
    fn grown_block_allocates_within_its_own_subnet_not_the_shared_cursor() {
        // Regression for the KVM-found grow-egress bug: the per-tenant last-assigned
        // cursor is shared across a tenant's block subnets, so when a sandbox grows
        // onto a new block the cursor from the PREVIOUS block can point BELOW the new
        // block's range. Without a lower-bound clamp, allocate_next_ipv4 returned an
        // address outside the grown block (e.g. .3 of 10.0.0.0/30 for a 10.0.0.4/30
        // block), so the sandbox's veth/route mismatched its PEP/pin gateway and
        // egress was denied on the grown block.
        let temp_dir = tempdir().expect("temp dir should create");
        let tenant_id = TenantId::new("tenant-a").expect("tenant should parse");
        let first_id = crate::instance::SandboxId::new("db-01");
        let second_id = crate::instance::SandboxId::new("db-02");
        let layout = OciNetworkLayout::under_root(temp_dir.path(), &tenant_id, &first_id);
        let ipam_authority = direct_test_ipam_authority(&layout);

        // Block 0 (10.0.0.0/30): the first sandbox takes the single host .2, leaving
        // the shared per-tenant cursor at .2.
        let block0 = OciNetworkConfig {
            network_subnet: "10.0.0.0/30".to_owned(),
            ..OciNetworkConfig::default()
        };
        let first = allocate_container_ips(&ipam_authority, &layout, &block0, &first_id)
            .expect("block 0 host");
        assert_eq!(first, vec!["10.0.0.2".parse::<Ipv4Addr>().unwrap()]);

        // Block 1 (10.0.0.4/30) shares the same tenant ipam-state; the cursor .2 is
        // BELOW this block's range (.5–.6). The grown block MUST allocate its own
        // host .6, never .3 (block 0's broadcast, below block 1).
        let block1 = OciNetworkConfig {
            network_subnet: "10.0.0.4/30".to_owned(),
            ..OciNetworkConfig::default()
        };
        let second = allocate_container_ips(&ipam_authority, &layout, &block1, &second_id)
            .expect("block 1 host");
        assert_eq!(
            second,
            vec!["10.0.0.6".parse::<Ipv4Addr>().unwrap()],
            "a grown block must allocate within its own subnet, not below it from the shared cursor"
        );
    }

    #[test]
    fn build_netavark_request_includes_allocated_static_ips() {
        let request = build_netavark_request(
            &OciNetworkConfig::default(),
            &crate::instance::SandboxId::new("db-01"),
            "db",
            "db",
            &["10.89.0.2".parse::<Ipv4Addr>().expect("IPv4 should parse")],
            &[],
            false,
        )
        .expect("request should build");

        assert_eq!(
            request.networks["nimbus"].static_ips,
            vec!["10.89.0.2".to_owned()]
        );
    }

    #[test]
    fn deallocate_container_ips_removes_persisted_assignment() {
        let temp_dir = tempdir().expect("temp dir should create");
        let config = OciNetworkConfig::default();
        let tenant_id = TenantId::new("tenant-a").expect("tenant should parse");
        let sandbox_id = crate::instance::SandboxId::new("db-01");
        let layout = OciNetworkLayout::under_root(temp_dir.path(), &tenant_id, &sandbox_id);
        let ipam_authority = direct_test_ipam_authority(&layout);

        let assigned = allocate_container_ips(&ipam_authority, &layout, &config, &sandbox_id)
            .expect("allocation should succeed");
        assert_eq!(assigned.len(), 1);

        deallocate_container_ips_after_confirmed_detach(
            &ipam_authority,
            &layout,
            &sandbox_id,
            &config.reservation_claim,
            config.provider_kind(),
        )
        .expect("deallocation should succeed");
        assert!(
            load_container_ips(&ipam_authority, &layout, &sandbox_id).is_err(),
            "removed allocation should no longer load"
        );
    }

    #[test]
    fn network_layout_roots_mutable_state_by_tenant() {
        let temp_dir = tempdir().expect("temp dir should create");
        let tenant_a = TenantId::new("tenant-a").expect("tenant should parse");
        let tenant_b = TenantId::new("tenant-b").expect("tenant should parse");
        let sandbox_id = crate::instance::SandboxId::new("db-01");

        let layout_a = OciNetworkLayout::under_root(temp_dir.path(), &tenant_a, &sandbox_id);
        let layout_b = OciNetworkLayout::under_root(temp_dir.path(), &tenant_b, &sandbox_id);

        assert_eq!(
            layout_a.network_root,
            temp_dir
                .path()
                .join("tenants")
                .join("tenant-a")
                .join("networks")
        );
        assert_eq!(
            layout_a.netns_path,
            temp_dir
                .path()
                .join("tenants")
                .join("tenant-a")
                .join("networks")
                .join("netns")
                .join("db-01")
        );
        assert_eq!(
            layout_a.network_state_root, layout_b.network_state_root,
            "one node-local authority owns every network partition"
        );
        assert_ne!(
            layout_a.tenant_id, layout_b.tenant_id,
            "same sandbox id in different tenants must use distinct IPAM partitions"
        );
        assert_ne!(
            layout_a.status_path, layout_b.status_path,
            "same sandbox id in different tenants must not share netavark status"
        );
    }

    #[test]
    fn per_tenant_segments_give_distinct_subnets_so_two_tenants_never_collide() {
        // Audit M1: two DIFFERENT tenants must land on DISTINCT per-tenant subnets
        // and bridges. They used to BOTH allocate 10.89.0.2 on the one shared
        // `nimbus0` bridge (a real L3 collision proven on KVM); now the allocator
        // carves 10.0.0.0/24 and 10.0.1.0/24, so the first sandbox in each is
        // 10.0.0.2 and 10.0.1.2 — no shared address, no shared L2.
        use super::{NetworkSegmentAllocator, SingleNodeSegmentAllocator};

        let temp_dir = tempdir().expect("temp dir should create");
        let state_root = temp_dir.path();
        let tenant_a = TenantId::new("tenant-a").expect("tenant should parse");
        let tenant_b = TenantId::new("tenant-b").expect("tenant should parse");
        let sandbox_id = crate::instance::SandboxId::new("db-01");

        let allocator = SingleNodeSegmentAllocator::single_node_default(state_root);
        let seg_a = allocator.segment_for(&tenant_a).expect("tenant-a segment");
        let seg_b = allocator.segment_for(&tenant_b).expect("tenant-b segment");

        // Distinct subnets, bridge interfaces, and netavark ids — no aliasing.
        assert_ne!(seg_a.cidr(), seg_b.cidr());
        assert!(
            !seg_a.cidr().overlaps(&seg_b.cidr()),
            "per-tenant subnets must not overlap"
        );
        assert_ne!(seg_a.network_interface(), seg_b.network_interface());
        assert_ne!(seg_a.network_id().as_str(), seg_b.network_id().as_str());

        // The per-tenant configs yield distinct first-sandbox IPs, not the old
        // colliding 10.89.0.2/10.89.0.2.
        let config_a = OciNetworkConfig {
            network_subnet: seg_a.cidr().to_string(),
            ..OciNetworkConfig::default()
        };
        let config_b = OciNetworkConfig {
            network_subnet: seg_b.cidr().to_string(),
            ..OciNetworkConfig::default()
        };
        let layout_a = OciNetworkLayout::under_root(state_root, &tenant_a, &sandbox_id);
        let layout_b = OciNetworkLayout::under_root(state_root, &tenant_b, &sandbox_id);
        let ipam_authority = direct_test_ipam_authority(&layout_a);
        let ips_a = allocate_container_ips(&ipam_authority, &layout_a, &config_a, &sandbox_id)
            .expect("tenant-a IP");
        let ips_b = allocate_container_ips(&ipam_authority, &layout_b, &config_b, &sandbox_id)
            .expect("tenant-b IP");

        assert_eq!(
            ips_a,
            vec!["10.0.0.2".parse::<Ipv4Addr>().expect("IPv4 should parse")]
        );
        assert_eq!(
            ips_b,
            vec!["10.0.1.2".parse::<Ipv4Addr>().expect("IPv4 should parse")]
        );
        assert_ne!(
            ips_a, ips_b,
            "two tenants must never collide on one address (audit M1)"
        );

        // MTN5: the egress PEP binds on the bridge gateway (allocate_egress_proxy
        // derives its host from bridge_gateway_addr), so each tenant's PEP URL is
        // distinct — no cross-tenant proxy aliasing.
        let gw_a = super::bridge_gateway_addr(&config_a).expect("tenant-a gateway");
        let gw_b = super::bridge_gateway_addr(&config_b).expect("tenant-b gateway");
        assert_eq!(gw_a.to_string(), "10.0.0.1");
        assert_eq!(gw_b.to_string(), "10.0.1.1");
        assert_ne!(
            gw_a, gw_b,
            "each tenant's egress PEP binds a distinct gateway (no PEP URL aliasing)"
        );
    }
}
