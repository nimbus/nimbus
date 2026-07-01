use std::time::Duration;

mod cluster;
mod dto;
mod egress_pin;
mod forwarding;
mod ipam;
mod layout;
mod netavark;
mod netns;
mod placement;
mod proxy;
mod reaper;
mod segment;

pub(crate) use egress_pin::pin_netns_egress_to_own_proxy;
pub use forwarding::OciMachinePortForwarderConfig;
pub(crate) use forwarding::{expose_machine_ports, unexpose_machine_ports};
pub(crate) use ipam::allocate_container_ips;
pub(crate) use layout::{
    OciNetworkConfig, OciNetworkDirectEgress, OciNetworkLayout, bridge_gateway_addr,
};
pub(crate) use netavark::{setup_container_network, teardown_container_network};
pub(crate) use netns::{create_persistent_network_namespace, remove_persistent_network_namespace};
pub(crate) use placement::place_sandbox_on_block;
pub(crate) use proxy::{MachinePortProxy, start_machine_port_proxies};
pub(crate) use reaper::{
    purge_legacy_nimbus0_once, reap_bridge_interface, reconcile_network_segment_orphans,
};
pub(crate) use segment::{
    DEFAULT_TENANT_PREFIX, NetworkSegmentAllocator, ReleaseOutcome, SingleNodeSegmentAllocator,
};

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
use forwarding::machine_forward_remote;
#[cfg(test)]
use ipam::{deallocate_container_ips, load_container_ips, parse_ipv4_subnet_and_gateway};
#[cfg(test)]
use netavark::{
    build_bridge_network, build_netavark_request, netavark_path_env, netavark_port_bindings,
    render_netavark_failure,
};
#[cfg(test)]
use proxy::machine_port_proxy_bind_addr;

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::io::{Read, Write};
    use std::net::{IpAddr, Ipv4Addr, TcpListener, TcpStream};
    use std::thread;
    use std::time::Duration;

    use nimbus_core::TenantId;
    use tempfile::tempdir;

    use super::{
        DEFAULT_MACHINE_FORWARDER_HOST, DEFAULT_MACHINE_FORWARDER_PATH,
        DEFAULT_MACHINE_FORWARDER_PORT, NETAVARK_OPTION_NO_DEFAULT_ROUTE,
        OciMachinePortForwarderConfig, OciNetworkConfig, OciNetworkDirectEgress, OciNetworkLayout,
        allocate_container_ips, build_bridge_network, build_netavark_request,
        deallocate_container_ips, load_container_ips, machine_forward_remote,
        machine_port_proxy_bind_addr, netavark_path_env, netavark_port_bindings,
        parse_ipv4_subnet_and_gateway, render_netavark_failure, start_machine_port_proxies,
    };
    use crate::backend::SandboxBackendKind;
    use crate::error::SandboxError;
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
        let forwarder = OciMachinePortForwarderConfig::gvproxy_default();

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
    fn machine_forwarder_default_matches_podman_shape() {
        let config = OciMachinePortForwarderConfig::gvproxy_default();
        assert_eq!(config.host, DEFAULT_MACHINE_FORWARDER_HOST);
        assert_eq!(config.port, DEFAULT_MACHINE_FORWARDER_PORT);
        assert_eq!(config.path_prefix, DEFAULT_MACHINE_FORWARDER_PATH);
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
    fn machine_port_proxy_binds_guest_wildcard_port() {
        let binding = SandboxPortBinding::tcp("http", 18080, 8080);

        assert_eq!(
            machine_port_proxy_bind_addr(&binding),
            "0.0.0.0:18080".parse().expect("socket addr should parse")
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
        let proxies = start_machine_port_proxies(&[Ipv4Addr::LOCALHOST], &[binding])
            .expect("machine port proxy should start");
        let mut stream = connect_with_retry(proxy_port);
        stream
            .write_all(b"ping")
            .expect("client should write request");
        let mut response = [0_u8; 4];
        stream
            .read_exact(&mut response)
            .expect("client should read response");

        assert_eq!(&response, b"pong");
        drop(proxies);
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
    fn allocate_container_ips_reserves_and_loads_podman_style_static_ips() {
        let temp_dir = tempdir().expect("temp dir should create");
        let config = OciNetworkConfig::default();
        let tenant_id = TenantId::new("tenant-a").expect("tenant should parse");
        let first_id = crate::instance::SandboxId::new("db-01");
        let second_id = crate::instance::SandboxId::new("db-02");
        let layout = OciNetworkLayout::new(temp_dir.path(), &tenant_id, &first_id);

        let first = allocate_container_ips(&layout, &config, &first_id)
            .expect("first allocation should succeed");
        let second = allocate_container_ips(&layout, &config, &second_id)
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
            load_container_ips(&layout, &second_id).expect("second allocation should load"),
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
        let layout = OciNetworkLayout::new(temp_dir.path(), &tenant_id, &first_id);

        let first = allocate_container_ips(&layout, &config, &first_id)
            .expect("single allocatable container address should succeed");
        let second = allocate_container_ips(&layout, &config, &second_id)
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
        let layout = OciNetworkLayout::new(temp_dir.path(), &tenant_id, &sandbox_id);

        let assigned = allocate_container_ips(&layout, &config, &sandbox_id)
            .expect("allocation should succeed");
        assert_eq!(assigned.len(), 1);

        deallocate_container_ips(&layout, &sandbox_id).expect("deallocation should succeed");
        assert!(
            load_container_ips(&layout, &sandbox_id).is_err(),
            "removed allocation should no longer load"
        );
    }

    #[test]
    fn network_layout_roots_mutable_state_by_tenant() {
        let temp_dir = tempdir().expect("temp dir should create");
        let tenant_a = TenantId::new("tenant-a").expect("tenant should parse");
        let tenant_b = TenantId::new("tenant-b").expect("tenant should parse");
        let sandbox_id = crate::instance::SandboxId::new("db-01");

        let layout_a = OciNetworkLayout::new(temp_dir.path(), &tenant_a, &sandbox_id);
        let layout_b = OciNetworkLayout::new(temp_dir.path(), &tenant_b, &sandbox_id);

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
        assert_ne!(
            layout_a.ipam_state_path, layout_b.ipam_state_path,
            "same sandbox id in different tenants must not share mutable IPAM state"
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
        let layout_a = OciNetworkLayout::new(state_root, &tenant_a, &sandbox_id);
        let layout_b = OciNetworkLayout::new(state_root, &tenant_b, &sandbox_id);
        let ips_a = allocate_container_ips(&layout_a, &config_a, &sandbox_id).expect("tenant-a IP");
        let ips_b = allocate_container_ips(&layout_b, &config_b, &sandbox_id).expect("tenant-b IP");

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
