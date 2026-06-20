use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, Shutdown, SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::Duration;

use fs2::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
#[cfg(target_os = "linux")]
use std::ffi::CString;
#[cfg(target_os = "linux")]
use std::fs::File;
#[cfg(target_os = "linux")]
use std::os::unix::ffi::OsStrExt;

use nimbus_core::TenantId;

use crate::artifact_paths;
use crate::backends::oci::command::render_command_failure;
use crate::error::{Result, SandboxError};
use crate::instance::SandboxId;
use crate::spec::SandboxPortBinding;

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
const MACHINE_FORWARDER_TIMEOUT: Duration = Duration::from_secs(2);
const MACHINE_PORT_PROXY_ACCEPT_SLEEP: Duration = Duration::from_millis(50);
const MACHINE_PORT_PROXY_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct OciNetworkLayout {
    pub network_root: PathBuf,
    pub run_root: PathBuf,
    pub netns_root: PathBuf,
    pub container_network_dir: PathBuf,
    pub netns_path: PathBuf,
    pub status_path: PathBuf,
    pub ipam_state_path: PathBuf,
    pub ipam_lock_path: PathBuf,
}

impl OciNetworkLayout {
    pub(crate) fn new(
        state_root: impl Into<PathBuf>,
        tenant_id: &TenantId,
        sandbox_id: &SandboxId,
    ) -> Self {
        let state_root = state_root.into();
        let network_root = artifact_paths::tenant_root(&state_root, tenant_id).join("networks");
        let run_root = network_root.join("run");
        let netns_root = network_root.join("netns");
        let container_network_dir = network_root.join("containers").join(sandbox_id.as_str());
        Self {
            status_path: container_network_dir.join("status.json"),
            ipam_state_path: run_root.join("ipam-state.json"),
            ipam_lock_path: run_root.join("ipam.lock"),
            netns_path: netns_root.join(sandbox_id.as_str()),
            network_root,
            run_root,
            netns_root,
            container_network_dir,
        }
    }

    pub(crate) fn ensure_directories(&self) -> Result<()> {
        for path in [
            &self.run_root,
            &self.netns_root,
            &self.container_network_dir,
        ] {
            fs::create_dir_all(path).map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to create OCI network directory {}: {error}",
                    path.display()
                ),
            })?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct OciNetworkConfig {
    pub netavark_path: PathBuf,
    pub aardvark_dns_path: PathBuf,
    pub network_name: String,
    pub network_interface: String,
    pub network_subnet: String,
    pub direct_egress: OciNetworkDirectEgress,
}

impl Default for OciNetworkConfig {
    fn default() -> Self {
        Self {
            netavark_path: PathBuf::from(DEFAULT_NETAVARK_BINARY),
            aardvark_dns_path: PathBuf::from(DEFAULT_AARDVARK_DNS_BINARY),
            network_name: DEFAULT_NETWORK_NAME.to_owned(),
            network_interface: DEFAULT_NETWORK_INTERFACE.to_owned(),
            network_subnet: DEFAULT_NETWORK_SUBNET.to_owned(),
            direct_egress: OciNetworkDirectEgress::Deny,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum OciNetworkDirectEgress {
    Allow,
    Deny,
}

impl OciNetworkDirectEgress {
    fn is_denied(self) -> bool {
        matches!(self, Self::Deny)
    }

    fn label(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Deny => "deny",
        }
    }
}

pub(crate) fn bridge_gateway_addr(config: &OciNetworkConfig) -> Result<Ipv4Addr> {
    let (_, gateway) = parse_ipv4_subnet_and_gateway(&config.network_subnet)?;
    parse_ipv4_address(&gateway)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OciMachinePortForwarderConfig {
    pub host: String,
    pub port: u16,
    pub path_prefix: String,
}

impl OciMachinePortForwarderConfig {
    pub fn gvproxy_default() -> Self {
        Self {
            host: DEFAULT_MACHINE_FORWARDER_HOST.to_owned(),
            port: DEFAULT_MACHINE_FORWARDER_PORT,
            path_prefix: DEFAULT_MACHINE_FORWARDER_PATH.to_owned(),
        }
    }
}

pub(crate) fn create_persistent_network_namespace(path: &Path) -> Result<()> {
    #[cfg(not(target_os = "linux"))]
    {
        let _ = path;
        Err(SandboxError::BackendUnavailable {
            message: "persistent OCI network namespaces require Linux".to_owned(),
        })
    }

    #[cfg(target_os = "linux")]
    {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to create network-namespace parent {}: {error}",
                    parent.display()
                ),
            })?;
        }
        if path.exists() {
            remove_persistent_network_namespace(path)?;
        }
        File::create(path).map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to create network-namespace file {}: {error}",
                path.display()
            ),
        })?;

        let target = path.to_path_buf();
        let join = thread::spawn(move || -> Result<()> {
            let target_c = cstring_path(&target)?;
            let source = CString::new("/proc/thread-self/ns/net").map_err(|error| {
                SandboxError::OperationFailed {
                    message: format!("failed to encode network-namespace source path: {error}"),
                }
            })?;
            // SAFETY: unshare and mount are called with validated constant flags and
            // NUL-terminated C strings owned for the duration of the calls.
            unsafe {
                if libc::unshare(libc::CLONE_NEWNET) != 0 {
                    return Err(last_os_error("failed to unshare network namespace"));
                }
                if libc::mount(
                    source.as_ptr(),
                    target_c.as_ptr(),
                    std::ptr::null(),
                    libc::MS_BIND as libc::c_ulong,
                    std::ptr::null(),
                ) != 0
                {
                    return Err(last_os_error("failed to persist network namespace"));
                }
            }
            Ok(())
        });
        join.join().map_err(|_| SandboxError::OperationFailed {
            message: format!(
                "network-namespace setup thread panicked for {}",
                path.display()
            ),
        })?
    }
}

pub(crate) fn remove_persistent_network_namespace(path: &Path) -> Result<()> {
    #[cfg(not(target_os = "linux"))]
    {
        let _ = path;
        Ok(())
    }

    #[cfg(target_os = "linux")]
    {
        if !path.exists() {
            return Ok(());
        }
        let target_c = cstring_path(path)?;
        // SAFETY: umount2 is called with a valid filesystem path encoded as a
        // NUL-terminated C string owned for the duration of the call.
        unsafe {
            if libc::umount2(target_c.as_ptr(), libc::MNT_DETACH) != 0 {
                let error = std::io::Error::last_os_error();
                if !matches!(
                    error.raw_os_error(),
                    Some(libc::EINVAL) | Some(libc::ENOENT)
                ) {
                    return Err(SandboxError::OperationFailed {
                        message: format!(
                            "failed to remove network namespace {}: {error}",
                            path.display()
                        ),
                    });
                }
            }
        }
        fs::remove_file(path)
            .or_else(ignore_not_found)
            .map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to delete network-namespace file {}: {error}",
                    path.display()
                ),
            })?;
        Ok(())
    }
}

pub(crate) fn setup_container_network(
    layout: &OciNetworkLayout,
    config: &OciNetworkConfig,
    sandbox_id: &SandboxId,
    sandbox_name: &str,
    hostname: &str,
    port_bindings: &[SandboxPortBinding],
    machine_port_forwarder: Option<&OciMachinePortForwarderConfig>,
) -> Result<Vec<Ipv4Addr>> {
    let assigned_ips = allocate_container_ips(layout, config, sandbox_id)?;
    let netavark_port_bindings = netavark_port_bindings(port_bindings, machine_port_forwarder);
    let response = run_netavark(
        "setup",
        layout,
        config,
        sandbox_id,
        sandbox_name,
        hostname,
        &assigned_ips,
        netavark_port_bindings,
        machine_port_forwarder.is_some(),
    )
    .inspect_err(|_| {
        let _ = deallocate_container_ips(layout, sandbox_id);
    })?;
    let rendered =
        serde_json::to_vec_pretty(&response).map_err(|error| SandboxError::OperationFailed {
            message: format!("failed to serialize netavark status response: {error}"),
        })?;
    fs::write(&layout.status_path, rendered).map_err(|error| SandboxError::OperationFailed {
        message: format!(
            "failed to write netavark status {}: {error}",
            layout.status_path.display()
        ),
    })?;
    Ok(assigned_ips)
}

pub(crate) fn teardown_container_network(
    layout: &OciNetworkLayout,
    config: &OciNetworkConfig,
    sandbox_id: &SandboxId,
    sandbox_name: &str,
    hostname: &str,
    port_bindings: &[SandboxPortBinding],
    machine_port_forwarder: Option<&OciMachinePortForwarderConfig>,
) -> Result<()> {
    if !layout.netns_path.exists() {
        let _ = fs::remove_file(&layout.status_path);
        let _ = deallocate_container_ips(layout, sandbox_id);
        return Ok(());
    }
    let assigned_ips = load_container_ips(layout, sandbox_id)?;
    let netavark_port_bindings = netavark_port_bindings(port_bindings, machine_port_forwarder);
    let _ = run_netavark(
        "teardown",
        layout,
        config,
        sandbox_id,
        sandbox_name,
        hostname,
        &assigned_ips,
        netavark_port_bindings,
        machine_port_forwarder.is_some(),
    )?;
    let _ = fs::remove_file(&layout.status_path);
    let _ = deallocate_container_ips(layout, sandbox_id);
    Ok(())
}

pub(crate) fn expose_machine_ports(
    config: &OciMachinePortForwarderConfig,
    port_bindings: &[SandboxPortBinding],
) -> Result<()> {
    request_machine_port_forwarding(config, "expose", port_bindings)
}

pub(crate) fn unexpose_machine_ports(
    config: &OciMachinePortForwarderConfig,
    port_bindings: &[SandboxPortBinding],
) -> Result<()> {
    request_machine_port_forwarding(config, "unexpose", port_bindings)
}

pub(crate) struct MachinePortProxy {
    bind_addr: SocketAddr,
    shutdown: Arc<AtomicBool>,
    join: Option<thread::JoinHandle<()>>,
}

impl MachinePortProxy {
    fn start(binding: &SandboxPortBinding, container_ip: Ipv4Addr) -> Result<Self> {
        let bind_addr = machine_port_proxy_bind_addr(binding);
        let target_addr = SocketAddr::new(IpAddr::V4(container_ip), binding.guest_port);
        let listener =
            TcpListener::bind(bind_addr).map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to bind machine port proxy {} -> {} for {}:{}: {error}",
                    bind_addr, target_addr, binding.host_address, binding.host_port
                ),
            })?;
        listener
            .set_nonblocking(true)
            .map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to configure machine port proxy listener {}: {error}",
                    bind_addr
                ),
            })?;

        let shutdown = Arc::new(AtomicBool::new(false));
        let thread_shutdown = Arc::clone(&shutdown);
        let join = thread::Builder::new()
            .name(format!("nimbus-machine-port-{}", binding.host_port))
            .spawn(move || accept_machine_port_proxy(listener, target_addr, thread_shutdown))
            .map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to spawn machine port proxy {} -> {}: {error}",
                    bind_addr, target_addr
                ),
            })?;

        Ok(Self {
            bind_addr,
            shutdown,
            join: Some(join),
        })
    }

    fn stop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect_timeout(
            &machine_port_proxy_wake_addr(self.bind_addr),
            Duration::from_millis(100),
        );
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

impl Drop for MachinePortProxy {
    fn drop(&mut self) {
        self.stop();
    }
}

pub(crate) fn start_machine_port_proxies(
    assigned_ips: &[Ipv4Addr],
    port_bindings: &[SandboxPortBinding],
) -> Result<Vec<MachinePortProxy>> {
    if port_bindings.is_empty() {
        return Ok(Vec::new());
    }
    let Some(container_ip) = assigned_ips.first().copied() else {
        return Err(SandboxError::OperationFailed {
            message: "cannot start machine port proxies without a container IPv4 address"
                .to_owned(),
        });
    };
    port_bindings
        .iter()
        .map(|binding| MachinePortProxy::start(binding, container_ip))
        .collect()
}

fn accept_machine_port_proxy(
    listener: TcpListener,
    target_addr: SocketAddr,
    shutdown: Arc<AtomicBool>,
) {
    while !shutdown.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _)) => {
                let _ = thread::Builder::new()
                    .name("nimbus-machine-port-connection".to_owned())
                    .spawn(move || proxy_machine_port_connection(stream, target_addr));
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(MACHINE_PORT_PROXY_ACCEPT_SLEEP);
            }
            Err(_) => break,
        }
    }
}

fn proxy_machine_port_connection(mut inbound: TcpStream, target_addr: SocketAddr) {
    let Ok(mut outbound) =
        TcpStream::connect_timeout(&target_addr, MACHINE_PORT_PROXY_CONNECT_TIMEOUT)
    else {
        return;
    };
    let Ok(mut inbound_reader) = inbound.try_clone() else {
        return;
    };
    let Ok(mut outbound_writer) = outbound.try_clone() else {
        return;
    };
    let client_to_target = thread::spawn(move || {
        let _ = std::io::copy(&mut inbound_reader, &mut outbound_writer);
        let _ = outbound_writer.shutdown(Shutdown::Write);
    });
    let target_to_client = thread::spawn(move || {
        let _ = std::io::copy(&mut outbound, &mut inbound);
        let _ = inbound.shutdown(Shutdown::Write);
    });
    let _ = client_to_target.join();
    let _ = target_to_client.join();
}

fn machine_port_proxy_bind_addr(binding: &SandboxPortBinding) -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), binding.host_port)
}

fn machine_port_proxy_wake_addr(bind_addr: SocketAddr) -> SocketAddr {
    if bind_addr.ip().is_unspecified() {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), bind_addr.port())
    } else {
        bind_addr
    }
}

#[allow(clippy::too_many_arguments)]
fn run_netavark(
    action: &str,
    layout: &OciNetworkLayout,
    config: &OciNetworkConfig,
    sandbox_id: &SandboxId,
    sandbox_name: &str,
    hostname: &str,
    assigned_ips: &[Ipv4Addr],
    port_bindings: &[SandboxPortBinding],
    strip_host_ip: bool,
) -> Result<Value> {
    let request = build_netavark_request(
        config,
        sandbox_id,
        sandbox_name,
        hostname,
        assigned_ips,
        port_bindings,
        strip_host_ip,
    )?;
    let request_bytes =
        serde_json::to_vec(&request).map_err(|error| SandboxError::OperationFailed {
            message: format!("failed to serialize netavark request: {error}"),
        })?;
    let output = std::process::Command::new(&config.netavark_path)
        .arg("--config")
        .arg(&layout.run_root)
        .arg("--rootless=false")
        .arg(format!(
            "--aardvark-binary={}",
            config.aardvark_dns_path.display()
        ))
        .arg(action)
        .arg(&layout.netns_path)
        .env("PATH", netavark_path_env(std::env::var_os("PATH")))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            if let Some(mut stdin) = child.stdin.take() {
                stdin.write_all(&request_bytes)?;
            }
            child.wait_with_output()
        })
        .map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to run netavark {} for sandbox {}: {error}",
                action,
                sandbox_id.as_str()
            ),
        })?;
    if !output.status.success() {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "netavark {} failed for sandbox {}: {}",
                action,
                sandbox_id.as_str(),
                render_netavark_failure(&output.stdout, &output.stderr)
            ),
        });
    }
    if output.stdout.is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_slice(&output.stdout).map_err(|error| SandboxError::OperationFailed {
        message: format!(
            "failed to parse netavark {} response for sandbox {}: {error}",
            action,
            sandbox_id.as_str()
        ),
    })
}

fn build_netavark_request(
    config: &OciNetworkConfig,
    sandbox_id: &SandboxId,
    sandbox_name: &str,
    hostname: &str,
    assigned_ips: &[Ipv4Addr],
    port_bindings: &[SandboxPortBinding],
    strip_host_ip: bool,
) -> Result<NetavarkRequest> {
    let network = build_bridge_network(config)?;
    let networks = BTreeMap::from([(
        config.network_name.clone(),
        NetavarkPerNetworkOptions {
            interface_name: DEFAULT_CONTAINER_INTERFACE_NAME.to_owned(),
            static_ips: assigned_ips.iter().map(ToString::to_string).collect(),
        },
    )]);
    let network_info = BTreeMap::from([(config.network_name.clone(), network)]);
    let port_mappings = port_bindings
        .iter()
        .map(|binding| NetavarkPortMapping {
            host_ip: if strip_host_ip {
                String::new()
            } else {
                binding.host_address.to_string()
            },
            host_port: binding.host_port,
            container_port: binding.guest_port,
            range: 1,
            protocol: "tcp".to_owned(),
        })
        .collect();
    Ok(NetavarkRequest {
        container_id: sandbox_id.as_str().to_owned(),
        container_name: sandbox_name.to_owned(),
        port_mappings,
        networks,
        dns_servers: Vec::new(),
        container_hostname: hostname.to_owned(),
        network_info,
    })
}

fn build_bridge_network(config: &OciNetworkConfig) -> Result<NetavarkNetwork> {
    let (subnet, gateway) = parse_ipv4_subnet_and_gateway(&config.network_subnet)?;
    let mut options = BTreeMap::new();
    if config.direct_egress.is_denied() {
        options.insert(
            NETAVARK_OPTION_NO_DEFAULT_ROUTE.to_owned(),
            "true".to_owned(),
        );
    }
    Ok(NetavarkNetwork {
        name: config.network_name.clone(),
        id: DEFAULT_NETWORK_ID.to_owned(),
        driver: "bridge".to_owned(),
        network_interface: config.network_interface.clone(),
        created: None,
        subnets: vec![NetavarkSubnet { subnet, gateway }],
        ipv6_enabled: false,
        internal: false,
        dns_enabled: true,
        network_dns_servers: Vec::new(),
        labels: BTreeMap::from([(
            "io.nimbus.egress.direct".to_owned(),
            config.direct_egress.label().to_owned(),
        )]),
        options,
        ipam_options: BTreeMap::from([("driver".to_owned(), "host-local".to_owned())]),
    })
}

fn parse_ipv4_subnet_and_gateway(subnet_cidr: &str) -> Result<(String, String)> {
    let subnet = parse_ipv4_bridge_subnet(subnet_cidr)?;
    Ok((subnet.cidr, subnet.gateway.to_string()))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Ipv4BridgeSubnet {
    cidr: String,
    network: Ipv4Addr,
    gateway: Ipv4Addr,
    broadcast: Ipv4Addr,
}

fn parse_ipv4_bridge_subnet(subnet_cidr: &str) -> Result<Ipv4BridgeSubnet> {
    let (ip, prefix) = subnet_cidr
        .split_once('/')
        .ok_or_else(|| SandboxError::InvalidSpec {
            message: format!("invalid container bridge subnet {subnet_cidr:?}: missing prefix"),
        })?;
    let prefix = prefix
        .parse::<u8>()
        .map_err(|_| SandboxError::InvalidSpec {
            message: format!("invalid container bridge subnet {subnet_cidr:?}: bad prefix"),
        })?;
    if prefix > 32 {
        return Err(SandboxError::InvalidSpec {
            message: format!("invalid container bridge subnet {subnet_cidr:?}: bad prefix"),
        });
    }
    if prefix > 30 {
        return Err(SandboxError::InvalidSpec {
            message: format!(
                "invalid container bridge subnet {subnet_cidr:?}: bridge subnet must leave room for gateway and container addresses"
            ),
        });
    }

    let configured_ip = ip
        .trim()
        .parse::<Ipv4Addr>()
        .map_err(|_| SandboxError::InvalidSpec {
            message: format!("invalid container bridge subnet {subnet_cidr:?}: bad IPv4 address"),
        })?;
    let configured = ipv4_to_u32(configured_ip);
    let mask = if prefix == 0 {
        0
    } else {
        u32::MAX << (32 - prefix)
    };
    let network = configured & mask;
    if configured != network {
        return Err(SandboxError::InvalidSpec {
            message: format!(
                "invalid container bridge subnet {subnet_cidr:?}: address must be the network address for /{prefix}"
            ),
        });
    }

    let broadcast = network | !mask;
    let gateway = network
        .checked_add(1)
        .filter(|gateway| *gateway < broadcast)
        .ok_or_else(|| SandboxError::InvalidSpec {
            message: format!(
                "invalid container bridge subnet {subnet_cidr:?}: bridge subnet must leave room for a gateway address"
            ),
        })?;
    gateway
        .checked_add(1)
        .filter(|first_container| *first_container < broadcast)
        .ok_or_else(|| SandboxError::InvalidSpec {
            message: format!(
                "invalid container bridge subnet {subnet_cidr:?}: bridge subnet must leave room for container addresses"
            ),
        })?;

    Ok(Ipv4BridgeSubnet {
        cidr: format!("{}/{}", u32_to_ipv4(network), prefix),
        network: u32_to_ipv4(network),
        gateway: u32_to_ipv4(gateway),
        broadcast: u32_to_ipv4(broadcast),
    })
}

fn allocate_container_ips(
    layout: &OciNetworkLayout,
    config: &OciNetworkConfig,
    sandbox_id: &SandboxId,
) -> Result<Vec<Ipv4Addr>> {
    with_ipam_state(layout, |state| {
        if let Some(assigned) = state.allocations.get(sandbox_id.as_str()) {
            return assigned
                .iter()
                .map(|ip| parse_ipv4_address(ip))
                .collect::<Result<Vec<_>>>();
        }

        let allocation = allocate_next_ipv4(config, state)?;
        state
            .allocations
            .insert(sandbox_id.as_str().to_owned(), vec![allocation.to_string()]);
        state.last_assigned_ip = Some(allocation.to_string());
        Ok(vec![allocation])
    })
}

fn load_container_ips(layout: &OciNetworkLayout, sandbox_id: &SandboxId) -> Result<Vec<Ipv4Addr>> {
    with_ipam_state(layout, |state| {
        state
            .allocations
            .get(sandbox_id.as_str())
            .ok_or_else(|| SandboxError::OperationFailed {
                message: format!(
                    "failed to find allocated container IPs for sandbox {}",
                    sandbox_id.as_str()
                ),
            })?
            .iter()
            .map(|ip| parse_ipv4_address(ip))
            .collect()
    })
}

fn deallocate_container_ips(layout: &OciNetworkLayout, sandbox_id: &SandboxId) -> Result<()> {
    with_ipam_state(layout, |state| {
        state.allocations.remove(sandbox_id.as_str());
        Ok(())
    })
}

fn with_ipam_state<T>(
    layout: &OciNetworkLayout,
    mutator: impl FnOnce(&mut IpamState) -> Result<T>,
) -> Result<T> {
    fs::create_dir_all(&layout.run_root).map_err(|error| SandboxError::OperationFailed {
        message: format!(
            "failed to create OCI network run directory {}: {error}",
            layout.run_root.display()
        ),
    })?;
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&layout.ipam_lock_path)
        .map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to open OCI IPAM lock {}: {error}",
                layout.ipam_lock_path.display()
            ),
        })?;
    lock.lock_exclusive()
        .map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to lock OCI IPAM state {}: {error}",
                layout.ipam_lock_path.display()
            ),
        })?;

    let mut state = if layout.ipam_state_path.exists() {
        let contents =
            fs::read(&layout.ipam_state_path).map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to read OCI IPAM state {}: {error}",
                    layout.ipam_state_path.display()
                ),
            })?;
        serde_json::from_slice::<IpamState>(&contents).map_err(|error| {
            SandboxError::OperationFailed {
                message: format!(
                    "failed to parse OCI IPAM state {}: {error}",
                    layout.ipam_state_path.display()
                ),
            }
        })?
    } else {
        IpamState::default()
    };

    let result = mutator(&mut state);
    let persist = result.is_ok();
    if persist {
        let rendered =
            serde_json::to_vec_pretty(&state).map_err(|error| SandboxError::OperationFailed {
                message: format!("failed to serialize OCI IPAM state: {error}"),
            })?;
        fs::write(&layout.ipam_state_path, rendered).map_err(|error| {
            SandboxError::OperationFailed {
                message: format!(
                    "failed to persist OCI IPAM state {}: {error}",
                    layout.ipam_state_path.display()
                ),
            }
        })?;
    }

    lock.unlock()
        .map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to unlock OCI IPAM state {}: {error}",
                layout.ipam_lock_path.display()
            ),
        })?;
    result
}

fn allocate_next_ipv4(config: &OciNetworkConfig, state: &IpamState) -> Result<Ipv4Addr> {
    let subnet = parse_ipv4_bridge_subnet(&config.network_subnet)?;
    let network_base = ipv4_to_u32(subnet.network);
    let broadcast = ipv4_to_u32(subnet.broadcast);
    let range_start = network_base
        .checked_add(1)
        .ok_or_else(|| SandboxError::OperationFailed {
            message: format!(
                "failed to derive OCI IP allocation range start from subnet {}",
                config.network_subnet
            ),
        })?;
    let range_end = broadcast
        .checked_sub(1)
        .ok_or_else(|| SandboxError::OperationFailed {
            message: format!(
                "failed to derive OCI IP allocation range end from subnet {}",
                config.network_subnet
            ),
        })?;
    if range_start > range_end {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "OCI bridge subnet {} does not contain any allocatable IPv4 addresses",
                config.network_subnet
            ),
        });
    }

    let used_ips = state
        .allocations
        .values()
        .flatten()
        .map(|ip| parse_ipv4_address(ip).map(ipv4_to_u32))
        .collect::<Result<BTreeSet<_>>>()?;
    let gateway = ipv4_to_u32(subnet.gateway);
    let start_ip = state
        .last_assigned_ip
        .as_deref()
        .map(parse_ipv4_address)
        .transpose()?
        .map(ipv4_to_u32)
        .and_then(|last| last.checked_add(1))
        .filter(|candidate| *candidate <= range_end)
        .unwrap_or(range_start);

    let mut current = start_ip;
    loop {
        if current != gateway && !used_ips.contains(&current) {
            return Ok(u32_to_ipv4(current));
        }
        current = if current >= range_end {
            range_start
        } else {
            current + 1
        };
        if current == start_ip {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "failed to find free OCI IPv4 address in subnet {}",
                    config.network_subnet
                ),
            });
        }
    }
}

fn parse_ipv4_address(value: &str) -> Result<Ipv4Addr> {
    value
        .parse::<Ipv4Addr>()
        .map_err(|_| SandboxError::InvalidSpec {
            message: format!("invalid IPv4 address {value:?}"),
        })
}

fn ipv4_to_u32(ip: Ipv4Addr) -> u32 {
    u32::from(ip)
}

fn u32_to_ipv4(value: u32) -> Ipv4Addr {
    Ipv4Addr::from(value)
}

fn request_machine_port_forwarding(
    config: &OciMachinePortForwarderConfig,
    action: &str,
    port_bindings: &[SandboxPortBinding],
) -> Result<()> {
    for binding in port_bindings {
        let request = MachinePortForwardRequest {
            local: format!("{}:{}", binding.host_address, binding.host_port),
            remote: (action == "expose").then(|| machine_forward_remote(binding)),
            protocol: "tcp".to_owned(),
        };
        let body = serde_json::to_vec(&request).map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to encode machine port-forward request for {}:{}: {error}",
                binding.host_address, binding.host_port
            ),
        })?;
        let mut addresses = (config.host.as_str(), config.port)
            .to_socket_addrs()
            .map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to resolve machine forwarder {}:{}: {error}",
                    config.host, config.port
                ),
            })?;
        let address = addresses
            .next()
            .ok_or_else(|| SandboxError::OperationFailed {
                message: format!(
                    "machine forwarder {}:{} did not resolve to an address",
                    config.host, config.port
                ),
            })?;
        let mut stream =
            TcpStream::connect_timeout(&address, MACHINE_FORWARDER_TIMEOUT).map_err(|error| {
                SandboxError::OperationFailed {
                    message: format!(
                        "failed to connect to machine forwarder {}:{}: {error}",
                        config.host, config.port
                    ),
                }
            })?;
        stream
            .set_read_timeout(Some(MACHINE_FORWARDER_TIMEOUT))
            .map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to configure machine forwarder timeout {}:{}: {error}",
                    config.host, config.port
                ),
            })?;
        let request = format!(
            "POST {}{} HTTP/1.0\r\nHost: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
            trim_trailing_slash(&config.path_prefix),
            if action == "expose" {
                "/expose"
            } else {
                "/unexpose"
            },
            config.host,
            body.len(),
        );
        stream
            .write_all(request.as_bytes())
            .and_then(|()| stream.write_all(&body))
            .map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to send machine forwarder {} request for {}:{}: {error}",
                    action, binding.host_address, binding.host_port
                ),
            })?;

        let mut response = Vec::new();
        let mut chunk = [0_u8; 1024];
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
                Err(error) => {
                    return Err(SandboxError::OperationFailed {
                        message: format!(
                            "failed to read machine forwarder {} response for {}:{}: {error}",
                            action, binding.host_address, binding.host_port
                        ),
                    });
                }
            }
        }

        let status_line = String::from_utf8_lossy(&response)
            .lines()
            .next()
            .unwrap_or("<empty-response>")
            .to_owned();
        if !status_line.contains("200 OK") {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "machine forwarder {} request for {}:{} failed: {}",
                    action, binding.host_address, binding.host_port, status_line
                ),
            });
        }
    }
    Ok(())
}

fn machine_forward_remote(binding: &SandboxPortBinding) -> String {
    format!(":{}", binding.host_port)
}

fn netavark_port_bindings<'a>(
    port_bindings: &'a [SandboxPortBinding],
    machine_port_forwarder: Option<&OciMachinePortForwarderConfig>,
) -> &'a [SandboxPortBinding] {
    if machine_port_forwarder.is_some() {
        // In machine mode gvproxy publishes host ports to the guest, and this
        // runner-owned guest listener bridges into the default-deny container
        // network. Netavark host-port DNAT would route gvproxy traffic directly
        // to the container, which needs a return route outside the service
        // bridge and violates the no-default-route posture.
        &[]
    } else {
        port_bindings
    }
}

fn trim_trailing_slash(path_prefix: &str) -> &str {
    path_prefix.trim_end_matches('/')
}

#[cfg(target_os = "linux")]
fn cstring_path(path: &Path) -> Result<CString> {
    CString::new(path.as_os_str().as_bytes()).map_err(|error| SandboxError::OperationFailed {
        message: format!(
            "failed to encode filesystem path {}: {error}",
            path.display()
        ),
    })
}

#[cfg(target_os = "linux")]
fn last_os_error(context: &str) -> SandboxError {
    SandboxError::OperationFailed {
        message: format!("{context}: {}", std::io::Error::last_os_error()),
    }
}

#[cfg(target_os = "linux")]
fn ignore_not_found(error: std::io::Error) -> std::io::Result<()> {
    if error.kind() == std::io::ErrorKind::NotFound {
        Ok(())
    } else {
        Err(error)
    }
}

fn netavark_path_env(current_path: Option<OsString>) -> OsString {
    let path = current_path
        .and_then(|path| path.into_string().ok())
        .unwrap_or_default();
    if path.split(':').any(|segment| segment == "/usr/sbin") {
        return OsString::from(path);
    }
    if path.is_empty() {
        OsString::from("/usr/sbin")
    } else {
        OsString::from(format!("{path}:/usr/sbin"))
    }
}

fn render_netavark_failure(stdout: &[u8], stderr: &[u8]) -> String {
    if let Ok(payload) = serde_json::from_slice::<NetavarkErrorResponse>(stdout) {
        let message = payload.error.trim();
        if !message.is_empty() {
            return message.to_owned();
        }
    }

    let stdout_rendered = String::from_utf8_lossy(stdout).trim().to_owned();
    if !stdout_rendered.is_empty() {
        return stdout_rendered;
    }

    render_command_failure(stdout, stderr)
}

#[derive(Debug, Serialize)]
struct NetavarkRequest {
    container_id: String,
    container_name: String,
    port_mappings: Vec<NetavarkPortMapping>,
    networks: BTreeMap<String, NetavarkPerNetworkOptions>,
    dns_servers: Vec<String>,
    container_hostname: String,
    network_info: BTreeMap<String, NetavarkNetwork>,
}

#[derive(Debug, Serialize)]
struct NetavarkPortMapping {
    host_ip: String,
    container_port: u16,
    host_port: u16,
    range: u16,
    protocol: String,
}

#[derive(Debug, Serialize)]
struct NetavarkPerNetworkOptions {
    interface_name: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    static_ips: Vec<String>,
}

#[derive(Debug, Serialize)]
struct NetavarkNetwork {
    name: String,
    id: String,
    driver: String,
    network_interface: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    created: Option<String>,
    subnets: Vec<NetavarkSubnet>,
    ipv6_enabled: bool,
    internal: bool,
    dns_enabled: bool,
    network_dns_servers: Vec<String>,
    labels: BTreeMap<String, String>,
    options: BTreeMap<String, String>,
    ipam_options: BTreeMap<String, String>,
}

#[derive(Debug, Serialize)]
struct NetavarkSubnet {
    subnet: String,
    gateway: String,
}

#[derive(Debug, Serialize)]
struct MachinePortForwardRequest {
    local: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    remote: Option<String>,
    protocol: String,
}

#[derive(Debug, Deserialize)]
struct NetavarkErrorResponse {
    error: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct IpamState {
    allocations: BTreeMap<String, Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_assigned_ip: Option<String>,
}

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
        allocate_container_ips, build_netavark_request, deallocate_container_ips,
        load_container_ips, machine_forward_remote, machine_port_proxy_bind_addr,
        netavark_path_env, netavark_port_bindings, parse_ipv4_subnet_and_gateway,
        render_netavark_failure, start_machine_port_proxies,
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
        assert!(matches!(
            second,
            SandboxError::OperationFailed { message }
                if message.contains("failed to find free OCI IPv4 address")
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
    fn tenant_network_ipam_state_isolated_for_same_sandbox_id() {
        let temp_dir = tempdir().expect("temp dir should create");
        let config = OciNetworkConfig::default();
        let tenant_a = TenantId::new("tenant-a").expect("tenant should parse");
        let tenant_b = TenantId::new("tenant-b").expect("tenant should parse");
        let sandbox_id = crate::instance::SandboxId::new("db-01");
        let layout_a = OciNetworkLayout::new(temp_dir.path(), &tenant_a, &sandbox_id);
        let layout_b = OciNetworkLayout::new(temp_dir.path(), &tenant_b, &sandbox_id);

        let tenant_a_ips = allocate_container_ips(&layout_a, &config, &sandbox_id)
            .expect("tenant-a allocation should succeed");
        let tenant_b_ips = allocate_container_ips(&layout_b, &config, &sandbox_id)
            .expect("tenant-b allocation should succeed");

        assert_eq!(
            tenant_a_ips,
            vec!["10.89.0.2".parse::<Ipv4Addr>().expect("IPv4 should parse")]
        );
        assert_eq!(
            tenant_b_ips,
            vec!["10.89.0.2".parse::<Ipv4Addr>().expect("IPv4 should parse")],
            "each tenant gets an independent network/IPAM namespace"
        );
        assert_ne!(
            layout_a.ipam_state_path, layout_b.ipam_state_path,
            "tenant IPAM files must be distinct"
        );
    }
}
