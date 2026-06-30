//! Host-machine port forwarding requests for OCI machine mode.

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};

use serde::{Deserialize, Serialize};

use crate::error::{Result, SandboxError};
use crate::spec::SandboxPortBinding;

use super::dto::MachinePortForwardRequest;
use super::{
    DEFAULT_MACHINE_FORWARDER_HOST, DEFAULT_MACHINE_FORWARDER_PATH, DEFAULT_MACHINE_FORWARDER_PORT,
    MACHINE_FORWARDER_TIMEOUT,
};

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

pub(super) fn machine_forward_remote(binding: &SandboxPortBinding) -> String {
    format!(":{}", binding.host_port)
}

fn trim_trailing_slash(path_prefix: &str) -> &str {
    path_prefix.trim_end_matches('/')
}
