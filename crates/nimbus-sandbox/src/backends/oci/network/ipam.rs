//! Tenant-scoped static IPv4 allocation for OCI bridge networks.

use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::net::Ipv4Addr;

use fs2::FileExt;

use crate::error::{Result, SandboxError};
use crate::instance::SandboxId;

use super::dto::IpamState;
use super::layout::{OciNetworkConfig, OciNetworkLayout};

pub(super) fn parse_ipv4_subnet_and_gateway(subnet_cidr: &str) -> Result<(String, String)> {
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

pub(super) fn allocate_container_ips(
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

pub(super) fn load_container_ips(
    layout: &OciNetworkLayout,
    sandbox_id: &SandboxId,
) -> Result<Vec<Ipv4Addr>> {
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

pub(super) fn deallocate_container_ips(
    layout: &OciNetworkLayout,
    sandbox_id: &SandboxId,
) -> Result<()> {
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

pub(super) fn parse_ipv4_address(value: &str) -> Result<Ipv4Addr> {
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
