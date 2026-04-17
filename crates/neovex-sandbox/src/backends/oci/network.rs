use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::time::Duration;

use fs2::FileExt;
#[cfg(target_os = "linux")]
use std::ffi::CString;
#[cfg(target_os = "linux")]
use std::fs::File;
#[cfg(target_os = "linux")]
use std::os::unix::ffi::OsStrExt;
#[cfg(target_os = "linux")]
use std::thread;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{Result, SandboxError};
use crate::instance::SandboxId;
use crate::spec::SandboxPortBinding;

pub(crate) const DEFAULT_NETAVARK_BINARY: &str = "netavark";
pub(crate) const DEFAULT_AARDVARK_DNS_BINARY: &str = "aardvark-dns";
pub(crate) const DEFAULT_NETWORK_NAME: &str = "neovex";
pub(crate) const DEFAULT_NETWORK_INTERFACE: &str = "neovex0";
pub(crate) const DEFAULT_NETWORK_SUBNET: &str = "10.89.0.0/24";
pub(crate) const DEFAULT_MACHINE_FORWARDER_HOST: &str = "gateway.containers.internal";
pub(crate) const DEFAULT_MACHINE_FORWARDER_PORT: u16 = 80;
pub(crate) const DEFAULT_MACHINE_FORWARDER_PATH: &str = "/services/forwarder";

const DEFAULT_CONTAINER_INTERFACE_NAME: &str = "eth0";
const DEFAULT_NETWORK_ID: &str = "5e9b4c62f9f3e8b8d2c74b7388d8451f5e9b4c62f9f3e8b8d2c74b7388d8451f";
const MACHINE_FORWARDER_TIMEOUT: Duration = Duration::from_secs(2);

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
    pub(crate) fn new(state_root: impl Into<PathBuf>, sandbox_id: &SandboxId) -> Self {
        let network_root = state_root.into().join("networks");
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
}

impl Default for OciNetworkConfig {
    fn default() -> Self {
        Self {
            netavark_path: PathBuf::from(DEFAULT_NETAVARK_BINARY),
            aardvark_dns_path: PathBuf::from(DEFAULT_AARDVARK_DNS_BINARY),
            network_name: DEFAULT_NETWORK_NAME.to_owned(),
            network_interface: DEFAULT_NETWORK_INTERFACE.to_owned(),
            network_subnet: DEFAULT_NETWORK_SUBNET.to_owned(),
        }
    }
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
) -> Result<()> {
    let assigned_ips = allocate_container_ips(layout, config, sandbox_id)?;
    let response = run_netavark(
        "setup",
        layout,
        config,
        sandbox_id,
        sandbox_name,
        hostname,
        &assigned_ips,
        port_bindings,
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
    Ok(())
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
    let _ = run_netavark(
        "teardown",
        layout,
        config,
        sandbox_id,
        sandbox_name,
        hostname,
        &assigned_ips,
        port_bindings,
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
        labels: BTreeMap::new(),
        options: BTreeMap::new(),
        ipam_options: BTreeMap::from([("driver".to_owned(), "host-local".to_owned())]),
    })
}

fn parse_ipv4_subnet_and_gateway(subnet_cidr: &str) -> Result<(String, String)> {
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
    let octets = ip
        .split('.')
        .map(str::trim)
        .map(|segment| segment.parse::<u8>())
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|_| SandboxError::InvalidSpec {
            message: format!("invalid container bridge subnet {subnet_cidr:?}: bad IPv4 address"),
        })?;
    if octets.len() != 4 {
        return Err(SandboxError::InvalidSpec {
            message: format!("invalid container bridge subnet {subnet_cidr:?}: bad IPv4 address"),
        });
    }
    let gateway = format!(
        "{}.{}.{}.{}",
        octets[0],
        octets[1],
        octets[2],
        octets[3] + 1
    );
    Ok((subnet_cidr.to_owned(), gateway))
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
    let (network_cidr, gateway) = parse_ipv4_subnet_and_gateway(&config.network_subnet)?;
    let (network_ip, prefix) =
        network_cidr
            .split_once('/')
            .ok_or_else(|| SandboxError::InvalidSpec {
                message: format!(
                    "invalid container bridge subnet {:?}: missing prefix",
                    config.network_subnet
                ),
            })?;
    let network_ip = parse_ipv4_address(network_ip)?;
    let gateway = parse_ipv4_address(&gateway)?;
    let prefix = prefix
        .parse::<u8>()
        .map_err(|_| SandboxError::InvalidSpec {
            message: format!(
                "invalid container bridge subnet {:?}: bad prefix",
                config.network_subnet
            ),
        })?;
    if prefix > 32 {
        return Err(SandboxError::InvalidSpec {
            message: format!(
                "invalid container bridge subnet {:?}: bad prefix",
                config.network_subnet
            ),
        });
    }

    let network_base = ipv4_to_u32(network_ip);
    let broadcast = if prefix == 32 {
        network_base
    } else {
        let mask = u32::MAX << (32 - prefix);
        network_base | !mask
    };
    let range_start = network_base
        .checked_add(1)
        .ok_or_else(|| SandboxError::OperationFailed {
            message: format!(
                "failed to derive OCI IP allocation range start from subnet {}",
                config.network_subnet
            ),
        })?;
    let range_end = if prefix == 32 {
        broadcast
    } else {
        broadcast
            .checked_sub(1)
            .ok_or_else(|| SandboxError::OperationFailed {
                message: format!(
                    "failed to derive OCI IP allocation range end from subnet {}",
                    config.network_subnet
                ),
            })?
    };
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
    let gateway = ipv4_to_u32(gateway);
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
            remote: (action == "expose").then(|| format!(":{}", binding.host_port)),
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

fn render_command_failure(stderr: &[u8]) -> String {
    let rendered = String::from_utf8_lossy(stderr).trim().to_owned();
    if rendered.is_empty() {
        "stderr was empty".to_owned()
    } else {
        rendered
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

    let stderr_rendered = render_command_failure(stderr);
    if stderr_rendered != "stderr was empty" {
        return stderr_rendered;
    }

    "stdout and stderr were empty".to_owned()
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
    use std::net::Ipv4Addr;

    use neovex_core::TenantId;
    use tempfile::tempdir;

    use super::{
        DEFAULT_MACHINE_FORWARDER_HOST, DEFAULT_MACHINE_FORWARDER_PATH,
        DEFAULT_MACHINE_FORWARDER_PORT, OciMachinePortForwarderConfig, OciNetworkConfig,
        OciNetworkLayout, allocate_container_ips, build_netavark_request, deallocate_container_ips,
        load_container_ips, netavark_path_env, render_netavark_failure,
    };
    use crate::backend::SandboxBackendKind;
    use crate::spec::{SandboxFilesystemSpec, SandboxPortBinding, SandboxProcessSpec, SandboxSpec};

    fn sample_spec() -> SandboxSpec {
        SandboxSpec::new(
            TenantId::new("svc-demo").expect("tenant should parse"),
            "db",
            SandboxBackendKind::Container,
            SandboxFilesystemSpec::new("/tmp/rootfs"),
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
        assert!(request.network_info.contains_key("neovex"));
    }

    #[test]
    fn netavark_request_strips_host_ip_when_machine_forwarding_is_enabled() {
        let request = build_netavark_request(
            &OciNetworkConfig::default(),
            &crate::instance::SandboxId::new("db-01"),
            "db",
            "db",
            &[],
            &[SandboxPortBinding::tcp("http", 18080, 8080)],
            true,
        )
        .expect("request should build");

        assert_eq!(request.port_mappings[0].host_ip, "");
    }

    #[test]
    fn machine_forwarder_default_matches_podman_shape() {
        let config = OciMachinePortForwarderConfig::gvproxy_default();
        assert_eq!(config.host, DEFAULT_MACHINE_FORWARDER_HOST);
        assert_eq!(config.port, DEFAULT_MACHINE_FORWARDER_PORT);
        assert_eq!(config.path_prefix, DEFAULT_MACHINE_FORWARDER_PATH);
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
        let first_id = crate::instance::SandboxId::new("db-01");
        let second_id = crate::instance::SandboxId::new("db-02");
        let layout = OciNetworkLayout::new(temp_dir.path(), &first_id);

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
            request.networks["neovex"].static_ips,
            vec!["10.89.0.2".to_owned()]
        );
    }

    #[test]
    fn deallocate_container_ips_removes_persisted_assignment() {
        let temp_dir = tempdir().expect("temp dir should create");
        let config = OciNetworkConfig::default();
        let sandbox_id = crate::instance::SandboxId::new("db-01");
        let layout = OciNetworkLayout::new(temp_dir.path(), &sandbox_id);

        let assigned = allocate_container_ips(&layout, &config, &sandbox_id)
            .expect("allocation should succeed");
        assert_eq!(assigned.len(), 1);

        deallocate_container_ips(&layout, &sandbox_id).expect("deallocation should succeed");
        assert!(
            load_container_ips(&layout, &sandbox_id).is_err(),
            "removed allocation should no longer load"
        );
    }
}
