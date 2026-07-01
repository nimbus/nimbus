//! Netavark request construction, execution, and status persistence.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::io::Write;
use std::net::Ipv4Addr;

use serde_json::Value;

use crate::backends::oci::command::render_command_failure;
use crate::error::{Result, SandboxError};
use crate::instance::SandboxId;
use crate::spec::SandboxPortBinding;

use super::dto::{
    NetavarkErrorResponse, NetavarkNetwork, NetavarkPerNetworkOptions, NetavarkPortMapping,
    NetavarkRequest, NetavarkSubnet,
};
use super::forwarding::OciMachinePortForwarderConfig;
use super::ipam::{
    allocate_container_ips, deallocate_container_ips, load_container_ips,
    parse_ipv4_subnet_and_gateway,
};
use super::layout::{OciNetworkConfig, OciNetworkLayout};
use super::{
    DEFAULT_CONTAINER_INTERFACE_NAME, NETAVARK_OPTION_ISOLATE, NETAVARK_OPTION_NO_DEFAULT_ROUTE,
};

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

pub(super) fn build_netavark_request(
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

pub(super) fn build_bridge_network(config: &OciNetworkConfig) -> Result<NetavarkNetwork> {
    let (subnet, gateway) = parse_ipv4_subnet_and_gateway(&config.network_subnet)?;
    let mut options = BTreeMap::new();
    if config.direct_egress.is_denied() {
        options.insert(
            NETAVARK_OPTION_NO_DEFAULT_ROUTE.to_owned(),
            "true".to_owned(),
        );
    }
    // Isolate every per-tenant bridge from the others: netavark installs a
    // FORWARD DROP between isolated networks, so a guest cannot route to a
    // sibling tenant's /24 even though all tenant bridges live in the host root
    // netns with ip_forward on (audit M1 / MTN5). The per-netns H1 pin remains
    // the intra-tenant sibling-PEP barrier; this closes the cross-tenant L3 path.
    options.insert(NETAVARK_OPTION_ISOLATE.to_owned(), "true".to_owned());
    Ok(NetavarkNetwork {
        name: config.network_name.clone(),
        id: config.network_id.clone(),
        driver: "bridge".to_owned(),
        network_interface: config.network_interface.clone(),
        created: None,
        subnets: vec![NetavarkSubnet { subnet, gateway }],
        ipv6_enabled: false,
        internal: false,
        dns_enabled: config.enable_dns,
        network_dns_servers: Vec::new(),
        labels: BTreeMap::from([(
            "io.nimbus.egress.direct".to_owned(),
            config.direct_egress.label().to_owned(),
        )]),
        options,
        ipam_options: BTreeMap::from([("driver".to_owned(), "host-local".to_owned())]),
    })
}

pub(super) fn netavark_port_bindings<'a>(
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

pub(super) fn netavark_path_env(current_path: Option<OsString>) -> OsString {
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

pub(super) fn render_netavark_failure(stdout: &[u8], stderr: &[u8]) -> String {
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
