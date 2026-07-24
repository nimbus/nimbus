//! Tenant-scoped static IPv4 allocation for OCI bridge networks.

use std::collections::BTreeSet;
use std::net::Ipv4Addr;

use nimbus_network::{LocalNetworkStateStore, NetworkStatePartition, NetworkStateTransactionError};

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

pub(crate) fn allocate_container_ips(
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
    let store = LocalNetworkStateStore::open(&layout.state_root).map_err(ipam_store_error)?;
    match store.transaction(
        &NetworkStatePartition::TenantIpam(layout.tenant_id.clone()),
        mutator,
    ) {
        Ok(result) => Ok(result),
        Err(NetworkStateTransactionError::Operation(error)) => Err(error),
        Err(NetworkStateTransactionError::Store(error)) => Err(ipam_store_error(error)),
    }
}

fn ipam_store_error(error: impl std::fmt::Display) -> SandboxError {
    SandboxError::OperationFailed {
        message: format!("OCI IPAM network authority failed: {error}"),
    }
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
    // The last-assigned cursor is per-tenant and SHARED across the tenant's block
    // subnets (MTN6 on-demand blocks). When allocating in a freshly-grown block,
    // the cursor left by a PREVIOUS block can fall OUTSIDE this block's range —
    // above it OR below it. Clamp to the block: only trust the cursor when it lands
    // within [range_start, range_end], else start at range_start. Without the
    // lower bound a grown block would hand out an address from another block's
    // subnet (e.g. cursor .2 from block 0 -> .3 returned for block 1 10.0.0.4/30),
    // so the sandbox's veth/route mismatch its PEP/pin gateway and egress is denied
    // — the KVM grow proof caught exactly this.
    let start_ip = state
        .last_assigned_ip
        .as_deref()
        .map(parse_ipv4_address)
        .transpose()?
        .map(ipv4_to_u32)
        .and_then(|last| last.checked_add(1))
        .filter(|candidate| *candidate >= range_start && *candidate <= range_end)
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
            // The block's /24 is full — a typed signal so block-aware placement
            // grows an additional block bridge instead of failing the launch.
            return Err(SandboxError::NetworkSubnetExhausted {
                subnet: config.network_subnet.clone(),
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

#[cfg(test)]
mod tests {
    use nimbus_core::TenantId;
    use std::fs;
    use tempfile::tempdir;

    use super::*;

    fn fixture() -> (
        tempfile::TempDir,
        OciNetworkLayout,
        OciNetworkConfig,
        SandboxId,
    ) {
        let dir = tempdir().expect("temp dir");
        let tenant = TenantId::new("tenant-original").expect("tenant should parse");
        let sandbox = SandboxId::new("sandbox-original");
        let layout = OciNetworkLayout::new(dir.path(), &tenant, &sandbox);
        (dir, layout, OciNetworkConfig::default(), sandbox)
    }

    #[test]
    fn torn_ipam_state_fails_closed_with_the_authority_path() {
        let (_dir, layout, config, sandbox) = fixture();
        allocate_container_ips(&layout, &config, &sandbox).expect("original IP should allocate");
        let authority_path = LocalNetworkStateStore::authority_path_for(&layout.state_root);
        fs::write(&authority_path, b"{").expect("torn state should be installed");

        let error =
            load_container_ips(&layout, &sandbox).expect_err("torn IPAM JSON must fail closed");
        let rendered = error.to_string();
        assert!(
            rendered.contains("network authority state") && rendered.contains("corrupt"),
            "the failure must reach the checksummed authority boundary: {rendered}"
        );
        assert!(
            rendered.contains(&authority_path.display().to_string()),
            "the corruption diagnostic must name the affected authority path: {rendered}"
        );
    }

    #[test]
    fn semantically_valid_ipam_state_corruption_must_not_reissue_a_live_ip() {
        let (_dir, layout, config, original_sandbox) = fixture();
        let original = allocate_container_ips(&layout, &config, &original_sandbox)
            .expect("original IP should allocate");
        let authority_path = LocalNetworkStateStore::authority_path_for(&layout.state_root);
        let mut envelope: serde_json::Value =
            serde_json::from_slice(&fs::read(&authority_path).expect("authority should read"))
                .expect("authority envelope should parse");
        envelope["body"]["records"]["tenant-ipam/tenant-original"]["allocations"] =
            serde_json::json!({});
        envelope["body"]["records"]["tenant-ipam/tenant-original"]["last_assigned_ip"] =
            serde_json::Value::Null;
        fs::write(
            &authority_path,
            serde_json::to_vec_pretty(&envelope).expect("tampered envelope should render"),
        )
        .expect("semantically corrupt IPAM state should be installed without checksum update");

        let replacement =
            allocate_container_ips(&layout, &config, &SandboxId::new("sandbox-replacement"));
        match replacement.as_ref() {
            Ok(ips) => assert_eq!(
                ips, &original,
                "the unchecked corruption must expose the audited live-IP reuse"
            ),
            Err(error) => {
                let rendered = error.to_string();
                assert!(
                    ["checksum", "corrupt", "integrity", "version"]
                        .iter()
                        .any(|needle| rendered.to_ascii_lowercase().contains(needle)),
                    "a fixed store must reject corruption with a named integrity error: {rendered}"
                );
            }
        }
        assert!(
            replacement.is_err(),
            "semantically valid corruption must fail closed instead of reissuing a live IP"
        );
    }
}
