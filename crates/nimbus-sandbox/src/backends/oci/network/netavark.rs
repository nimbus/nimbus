//! Netavark request construction, execution, and status persistence.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::io::{self, Write};
use std::net::Ipv4Addr;
use std::path::Path;

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
    NetavarkSetupClaim, NetavarkTeardownPlan, OciIpamAuthority,
    authenticate_container_network_generation_for_cleanup as authenticate_ipam_generation_for_cleanup,
    begin_netavark_setup, begin_netavark_setup_execution, begin_netavark_teardown,
    begin_netavark_teardown_execution, complete_netavark_setup, complete_netavark_teardown,
    confirm_netavark_absent_without_effect, confirm_netavark_provider_detached,
    load_container_ips_for_segment, parse_ipv4_subnet_and_gateway,
};
use super::layout::{OciNetworkConfig, OciNetworkLayout};
use super::{
    DEFAULT_CONTAINER_INTERFACE_NAME, NETAVARK_OPTION_ISOLATE, NETAVARK_OPTION_NO_DEFAULT_ROUTE,
};

#[cfg(test)]
#[path = "netavark/recovery_tests.rs"]
mod recovery_tests;
#[cfg(test)]
#[path = "netavark/tests.rs"]
mod tests;

/// Authenticate the immutable attachment generation before any provider,
/// namespace, status-projection, port, or segment mutation.
pub(crate) fn authenticate_container_network_generation(
    ipam_authority: &OciIpamAuthority,
    layout: &OciNetworkLayout,
    config: &OciNetworkConfig,
    sandbox_id: &SandboxId,
) -> Result<Vec<Ipv4Addr>> {
    load_container_ips_for_segment(ipam_authority, layout, config, sandbox_id)
}

/// Authenticate cleanup against either the exact live allocation or its
/// terminal tombstone. A terminal witness authorizes idempotent continuation
/// of the owning cleanup saga but never provider setup.
pub(crate) fn authenticate_container_network_generation_for_cleanup(
    ipam_authority: &OciIpamAuthority,
    layout: &OciNetworkLayout,
    config: &OciNetworkConfig,
    sandbox_id: &SandboxId,
) -> Result<()> {
    authenticate_ipam_generation_for_cleanup(ipam_authority, layout, config, sandbox_id).map(drop)
}

/// Immutable input for one exact Netavark provider operation.
pub(crate) struct OciNetavarkOperation<'a> {
    layout: &'a OciNetworkLayout,
    config: &'a OciNetworkConfig,
    sandbox_id: &'a SandboxId,
    sandbox_name: &'a str,
    hostname: &'a str,
    port_bindings: &'a [SandboxPortBinding],
    machine_port_forwarder: Option<&'a OciMachinePortForwarderConfig>,
}

impl<'a> OciNetavarkOperation<'a> {
    pub(crate) fn new(
        layout: &'a OciNetworkLayout,
        config: &'a OciNetworkConfig,
        sandbox_id: &'a SandboxId,
        sandbox_name: &'a str,
        hostname: &'a str,
        port_bindings: &'a [SandboxPortBinding],
        machine_port_forwarder: Option<&'a OciMachinePortForwarderConfig>,
    ) -> Self {
        Self {
            layout,
            config,
            sandbox_id,
            sandbox_name,
            hostname,
            port_bindings,
            machine_port_forwarder,
        }
    }
}

/// Exact durable setup attempt prepared before the first attachment effect.
///
/// The IPAM journal owns this capability. Namespace, listener, and Netavark
/// adapters may execute only the attempt selected here; they cannot mint a
/// replacement after an earlier host effect.
pub(super) struct PreparedNetavarkSetup {
    assigned_ips: Vec<Ipv4Addr>,
    claim: NetavarkSetupClaim,
}

pub(super) fn prepare_container_network_setup(
    ipam_authority: &OciIpamAuthority,
    operation: &OciNetavarkOperation<'_>,
) -> Result<PreparedNetavarkSetup> {
    let (assigned_ips, claim) = begin_netavark_setup(
        ipam_authority,
        operation.layout,
        operation.config,
        operation.sandbox_id,
    )?;
    Ok(PreparedNetavarkSetup {
        assigned_ips,
        claim,
    })
}

/// Exact durable teardown attempt prepared before attachment cleanup effects.
///
/// Runtime, PEP, machine-forwarding, namespace, and Netavark cleanup may begin
/// only after the IPAM journal has selected this plan. Execution cannot mint a
/// replacement attempt.
pub(super) struct PreparedNetavarkTeardown {
    plan: NetavarkTeardownPlan,
}

pub(super) fn prepare_container_network_teardown(
    ipam_authority: &OciIpamAuthority,
    operation: &OciNetavarkOperation<'_>,
) -> Result<PreparedNetavarkTeardown> {
    begin_netavark_teardown(
        ipam_authority,
        operation.layout,
        operation.config,
        operation.sandbox_id,
        None,
    )
    .map(|plan| PreparedNetavarkTeardown { plan })
}

pub(super) fn execute_prepared_container_network_teardown(
    ipam_authority: &OciIpamAuthority,
    operation: &OciNetavarkOperation<'_>,
    prepared: PreparedNetavarkTeardown,
) -> Result<()> {
    execute_prepared_container_network_teardown_with_runner(
        ipam_authority,
        operation.layout,
        prepared.plan,
        |action, assigned_ips| run_netavark(action, operation, assigned_ips),
    )
}

fn execute_prepared_container_network_teardown_with_runner(
    ipam_authority: &OciIpamAuthority,
    layout: &OciNetworkLayout,
    plan: NetavarkTeardownPlan,
    mut runner: impl FnMut(&str, &[Ipv4Addr]) -> Result<Value>,
) -> Result<()> {
    execute_teardown_plan(ipam_authority, layout, plan, &mut runner)
}

#[cfg(test)]
pub(super) fn execute_prepared_container_network_teardown_for_test(
    ipam_authority: &OciIpamAuthority,
    layout: &OciNetworkLayout,
    prepared: PreparedNetavarkTeardown,
) -> Result<()> {
    execute_prepared_container_network_teardown_with_runner(
        ipam_authority,
        layout,
        prepared.plan,
        |_, _| Ok(Value::Null),
    )
}

#[cfg(test)]
pub(super) fn execute_prepared_container_network_teardown_ambiguously_for_test(
    ipam_authority: &OciIpamAuthority,
    layout: &OciNetworkLayout,
    prepared: PreparedNetavarkTeardown,
    message: &str,
) -> Result<()> {
    execute_prepared_container_network_teardown_with_runner(
        ipam_authority,
        layout,
        prepared.plan,
        |_, _| {
            Err(SandboxError::OperationFailed {
                message: message.to_owned(),
            })
        },
    )
}

#[cfg(test)]
pub(crate) fn setup_container_network(
    ipam_authority: &OciIpamAuthority,
    operation: &OciNetavarkOperation<'_>,
) -> Result<Vec<Ipv4Addr>> {
    let prepared = prepare_container_network_setup(ipam_authority, operation)?;
    execute_prepared_container_network_setup(ipam_authority, operation, prepared)
}

pub(super) fn execute_prepared_container_network_setup(
    ipam_authority: &OciIpamAuthority,
    operation: &OciNetavarkOperation<'_>,
    prepared: PreparedNetavarkSetup,
) -> Result<Vec<Ipv4Addr>> {
    execute_prepared_container_network_setup_with_runner(
        ipam_authority,
        operation.layout,
        operation.config,
        operation.sandbox_id,
        prepared,
        |action, assigned_ips| run_netavark(action, operation, assigned_ips),
    )
}

#[cfg(test)]
pub(super) fn execute_prepared_container_network_setup_for_test(
    ipam_authority: &OciIpamAuthority,
    operation: &OciNetavarkOperation<'_>,
    prepared: PreparedNetavarkSetup,
) -> Result<Vec<Ipv4Addr>> {
    execute_prepared_container_network_setup_with_runner(
        ipam_authority,
        operation.layout,
        operation.config,
        operation.sandbox_id,
        prepared,
        |_, _| Ok(Value::Null),
    )
}

fn execute_prepared_container_network_setup_with_runner(
    ipam_authority: &OciIpamAuthority,
    layout: &OciNetworkLayout,
    config: &OciNetworkConfig,
    sandbox_id: &SandboxId,
    prepared: PreparedNetavarkSetup,
    mut runner: impl FnMut(&str, &[Ipv4Addr]) -> Result<Value>,
) -> Result<Vec<Ipv4Addr>> {
    let PreparedNetavarkSetup {
        assigned_ips,
        claim: setup_claim,
    } = prepared;
    let authenticated_ips =
        begin_netavark_setup_execution(ipam_authority, layout, config, sandbox_id, &setup_claim)?;
    if authenticated_ips != assigned_ips {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "prepared Netavark setup for attachment {} carries addresses that differ from \
                 its exact durable IPAM generation",
                config.attachment_id
            ),
        });
    }
    let setup = (|| {
        let response = runner("setup", &assigned_ips)?;
        let projection = super::dto::NetavarkStatusProjection {
            schema_version: super::dto::NetavarkStatusProjection::SCHEMA_VERSION,
            tenant_id: layout.tenant_id.clone(),
            attachment_id: config.attachment_id.clone(),
            setup_attempt: setup_claim.operation_attempt().clone(),
            assigned_ips: assigned_ips.clone(),
            response,
        };
        let rendered = serde_json::to_vec_pretty(&projection).map_err(|error| {
            SandboxError::OperationFailed {
                message: format!("failed to serialize netavark status response: {error}"),
            }
        })?;
        fs::write(&layout.status_path, rendered).map_err(|error| {
            SandboxError::OperationFailed {
                message: format!(
                    "failed to write netavark status {}: {error}",
                    layout.status_path.display()
                ),
            }
        })?;
        complete_netavark_setup(ipam_authority, layout, &setup_claim)
    })();
    if let Err(primary) = setup {
        let cleanup = compensate_failed_setup(
            ipam_authority,
            layout,
            config,
            sandbox_id,
            &setup_claim,
            &mut runner,
        )
        .err();
        return Err(match cleanup {
            None => primary,
            Some(cleanup) => SandboxError::OperationFailed {
                message: format!(
                    "Netavark setup failed: {primary}; same-attempt detach compensation also \
                     failed and provider authority remains fenced: {cleanup}"
                ),
            },
        });
    }
    Ok(assigned_ips)
}

#[cfg(test)]
fn setup_container_network_with_runner(
    ipam_authority: &OciIpamAuthority,
    layout: &OciNetworkLayout,
    config: &OciNetworkConfig,
    sandbox_id: &SandboxId,
    runner: impl FnMut(&str, &[Ipv4Addr]) -> Result<Value>,
) -> Result<Vec<Ipv4Addr>> {
    let (assigned_ips, claim) = begin_netavark_setup(ipam_authority, layout, config, sandbox_id)?;
    execute_prepared_container_network_setup_with_runner(
        ipam_authority,
        layout,
        config,
        sandbox_id,
        PreparedNetavarkSetup {
            assigned_ips,
            claim,
        },
        runner,
    )
}

#[cfg(test)]
pub(crate) fn teardown_container_network(
    ipam_authority: &OciIpamAuthority,
    operation: &OciNetavarkOperation<'_>,
) -> Result<()> {
    let prepared = prepare_container_network_teardown(ipam_authority, operation)?;
    execute_prepared_container_network_teardown(ipam_authority, operation, prepared)
}

#[cfg(test)]
fn teardown_container_network_with_runner(
    ipam_authority: &OciIpamAuthority,
    layout: &OciNetworkLayout,
    config: &OciNetworkConfig,
    sandbox_id: &SandboxId,
    mut runner: impl FnMut(&str, &[Ipv4Addr]) -> Result<Value>,
) -> Result<()> {
    let plan = begin_netavark_teardown(ipam_authority, layout, config, sandbox_id, None)?;
    execute_teardown_plan(ipam_authority, layout, plan, &mut runner)
}

fn compensate_failed_setup(
    ipam_authority: &OciIpamAuthority,
    layout: &OciNetworkLayout,
    config: &OciNetworkConfig,
    sandbox_id: &SandboxId,
    setup_claim: &NetavarkSetupClaim,
    runner: &mut impl FnMut(&str, &[Ipv4Addr]) -> Result<Value>,
) -> Result<()> {
    let plan = begin_netavark_teardown(
        ipam_authority,
        layout,
        config,
        sandbox_id,
        Some(setup_claim),
    )?;
    execute_teardown_plan(ipam_authority, layout, plan, runner)
}

fn execute_teardown_plan(
    ipam_authority: &OciIpamAuthority,
    layout: &OciNetworkLayout,
    plan: NetavarkTeardownPlan,
    runner: &mut impl FnMut(&str, &[Ipv4Addr]) -> Result<Value>,
) -> Result<()> {
    let claim = match plan {
        NetavarkTeardownPlan::AlreadyDetached => {
            require_netavark_status_absent(&layout.status_path)?;
            return Ok(());
        }
        NetavarkTeardownPlan::ConfirmNoEffect { claim } => {
            require_netavark_status_absent(&layout.status_path)?;
            confirm_netavark_absent_without_effect(ipam_authority, layout, &claim)?;
            claim
        }
        NetavarkTeardownPlan::RemoveProjection { claim } => claim,
        NetavarkTeardownPlan::InspectDeleting { claim } => {
            match fs::symlink_metadata(&layout.netns_path) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    confirm_netavark_provider_detached(ipam_authority, layout, &claim)?;
                }
                Err(error) => {
                    return Err(SandboxError::OperationFailed {
                        message: format!(
                            "failed to inspect persistent network namespace {} before reconciling \
                             an ambiguous delete: {error}",
                            layout.netns_path.display()
                        ),
                    });
                }
                Ok(_) => {
                    return Err(SandboxError::OperationFailed {
                        message: format!(
                            "Netavark teardown for attachment {} already crossed its pre-effect \
                             fence while provider evidence remains present; refusing a duplicate \
                             delete",
                            claim.attachment_id()
                        ),
                    });
                }
            }
            claim
        }
        NetavarkTeardownPlan::Run {
            assigned_ips,
            claim,
        } => {
            match fs::symlink_metadata(&layout.netns_path) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    confirm_netavark_absent_without_effect(ipam_authority, layout, &claim)?;
                }
                Err(error) => {
                    return Err(SandboxError::OperationFailed {
                        message: format!(
                            "failed to inspect persistent network namespace {} before confirming \
                             detach: {error}",
                            layout.netns_path.display()
                        ),
                    });
                }
                Ok(_) => {
                    begin_netavark_teardown_execution(ipam_authority, layout, &claim)?;
                    let _ = runner("teardown", &assigned_ips)?;
                    confirm_netavark_provider_detached(ipam_authority, layout, &claim)?;
                }
            }
            claim
        }
    };
    remove_netavark_status(&layout.status_path)?;
    complete_netavark_teardown(ipam_authority, layout, &claim)
}

fn remove_netavark_status(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(SandboxError::OperationFailed {
            message: format!(
                "failed to remove Netavark status projection {} after provider absence was \
                 recorded: {error}",
                path.display()
            ),
        }),
    }
}

fn require_netavark_status_absent(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(SandboxError::OperationFailed {
            message: format!(
                "failed to verify durable no-effect or terminal Netavark status projection \
                 absence at {}: {error}",
                path.display()
            ),
        }),
        Ok(_) => Err(SandboxError::OperationFailed {
            message: format!(
                "durable no-effect or terminal OCI IPAM authority conflicts with an existing \
                 Netavark status projection at {}; refusing to report cleanup success",
                path.display()
            ),
        }),
    }
}

fn run_netavark(
    action: &str,
    operation: &OciNetavarkOperation<'_>,
    assigned_ips: &[Ipv4Addr],
) -> Result<Value> {
    let request = build_netavark_request(
        operation.config,
        operation.sandbox_id,
        operation.sandbox_name,
        operation.hostname,
        assigned_ips,
        netavark_port_bindings(operation.port_bindings, operation.machine_port_forwarder),
        operation.machine_port_forwarder.is_some(),
    )?;
    let request_bytes =
        serde_json::to_vec(&request).map_err(|error| SandboxError::OperationFailed {
            message: format!("failed to serialize netavark request: {error}"),
        })?;
    let output = std::process::Command::new(&operation.config.netavark_path)
        .arg("--config")
        .arg(&operation.layout.run_root)
        .arg("--rootless=false")
        .arg(format!(
            "--aardvark-binary={}",
            operation.config.aardvark_dns_path.display()
        ))
        .arg(action)
        .arg(&operation.layout.netns_path)
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
                operation.sandbox_id.as_str()
            ),
        })?;
    if !output.status.success() {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "netavark {} failed for sandbox {}: {}",
                action,
                operation.sandbox_id.as_str(),
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
            operation.sandbox_id.as_str()
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
