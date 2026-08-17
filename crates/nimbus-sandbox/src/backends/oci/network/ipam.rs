//! Tenant-scoped static IPv4 allocation for OCI bridge networks.

use std::collections::BTreeSet;
use std::net::Ipv4Addr;
use std::path::Path;

use nimbus_core::TenantId;
#[cfg(test)]
use nimbus_network::{LocalNetworkStateStore, NetworkProviderHandle, NetworkProviderId};
use nimbus_network::{NetworkAttachmentId, NetworkReservationClaim, NetworkSegmentId};
use sha2::{Digest, Sha256};

use crate::error::{Result, SandboxError};
use crate::instance::{SandboxId, SandboxStatus};

#[cfg(test)]
use super::default_network_attachment_id;
use super::dto::{IpamAllocation, IpamState, NetavarkProviderOperation};
use super::layout::{OciNetworkConfig, OciNetworkLayout};
use super::provider_locator::{OciAttachmentProviderKind, OciAttachmentProviderLocator};

const IPAM_ALLOCATION_IDENTITY_DOMAIN: &[u8] = b"nimbus.sandbox.oci.ipam-allocation-identity.v1\0";

mod authority;
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "NNC5.2b stages typed evidence fields consumed by NNC5.2c classification"
    )
)]
mod evidence;
mod provider_operation;

pub(crate) use authority::OciIpamAuthority;
pub(in crate::backends::oci::network) use evidence::{
    OciAttachmentProviderEvidence, OciIpamEvidenceLifecycle,
};
#[cfg(test)]
pub(crate) use provider_operation::begin_netavark_setup_without_ack_for_test;
pub(super) use provider_operation::{
    NetavarkSetupClaim, NetavarkTeardownPlan, begin_netavark_setup, begin_netavark_setup_execution,
    begin_netavark_teardown, begin_netavark_teardown_execution, complete_netavark_setup,
    complete_netavark_teardown, confirm_netavark_absent_without_effect,
    confirm_netavark_provider_detached, inspect_netavark_provider_operation,
};

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

#[cfg(test)]
pub(crate) fn allocate_container_ips(
    authority: &OciIpamAuthority,
    layout: &OciNetworkLayout,
    config: &OciNetworkConfig,
    sandbox_id: &SandboxId,
) -> Result<Vec<Ipv4Addr>> {
    allocate_container_ips_on_first_available(
        authority,
        layout,
        std::slice::from_ref(config),
        sandbox_id,
        &config.reservation_claim,
    )
    .map(|allocation| allocation.ips)
}

#[cfg(test)]
fn test_reservation_claim(label: &str) -> NetworkReservationClaim {
    let provider = NetworkProviderId::for_registration_key("nimbus-sandbox.ipam-test-coordinator");
    NetworkReservationClaim::new(
        NetworkProviderHandle::new(provider, format!("attempt:{label}"))
            .expect("test reservation claim should validate"),
    )
}

/// One IPAM reservation together with the ordered block that owns it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct BlockIpAllocation {
    pub(super) block_index: usize,
    pub(super) segment_id: NetworkSegmentId,
    pub(super) ips: Vec<Ipv4Addr>,
}

/// Atomically choose and reserve the first available address across every
/// existing tenant block.
///
/// The caller supplies configs in durable segment-allocation order. The whole
/// scan and reservation occurs in one tenant-IPAM transaction under the shared
/// network authority lock, so concurrent placers cannot select the same
/// address. Existing idempotent reservations are mapped back to their owning
/// block and fail closed if that block is no longer in the supplied set.
#[cfg(test)]
pub(super) fn allocate_container_ips_on_first_available(
    authority: &OciIpamAuthority,
    layout: &OciNetworkLayout,
    configs: &[OciNetworkConfig],
    sandbox_id: &SandboxId,
    reservation_claim: &NetworkReservationClaim,
) -> Result<BlockIpAllocation> {
    let attachment_id = configs
        .first()
        .ok_or_else(|| SandboxError::OperationFailed {
            message: format!(
                "cannot allocate OCI IPs for sandbox {} without an exact attachment config",
                sandbox_id.as_str()
            ),
        })?
        .attachment_id
        .clone();
    if configs
        .iter()
        .any(|config| config.attachment_id != attachment_id)
    {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "OCI IPAM test configs disagree on exact attachment identity for sandbox {}",
                sandbox_id.as_str()
            ),
        });
    }
    allocate_container_ips_on_first_available_for_attachment(
        authority,
        layout,
        configs,
        sandbox_id,
        &attachment_id,
        reservation_claim,
    )
}

pub(super) fn allocate_container_ips_on_first_available_for_attachment(
    authority: &OciIpamAuthority,
    layout: &OciNetworkLayout,
    configs: &[OciNetworkConfig],
    sandbox_id: &SandboxId,
    attachment_id: &NetworkAttachmentId,
    reservation_claim: &NetworkReservationClaim,
) -> Result<BlockIpAllocation> {
    if configs.is_empty() {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "cannot allocate OCI IPs for sandbox {} without an existing tenant block",
                sandbox_id.as_str()
            ),
        });
    }

    let provider_kind = configs[0].provider_kind;
    if configs
        .iter()
        .any(|config| config.provider_kind != provider_kind)
    {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "OCI IPAM configs disagree on provider family for attachment {}",
                attachment_id.as_str()
            ),
        });
    }
    let provider_locator = OciAttachmentProviderLocator::new(
        &layout.workload_state_root,
        &layout.tenant_id,
        sandbox_id,
        provider_kind,
    )?;
    with_ipam_state(authority, layout, |state| {
        if let Some(assigned) = state.allocations.get(attachment_id.as_str()) {
            if &assigned.reservation_claim != reservation_claim {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "existing OCI IPAM reservation for attachment {} belongs to a different launch coordinator; refusing cross-generation adoption",
                        attachment_id.as_str()
                    ),
                });
            }
            if assigned.provider_locator != provider_locator {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "existing OCI IPAM reservation for attachment {} belongs to a different \
                         provider locator; refusing artifact-realm or backend substitution",
                        attachment_id.as_str()
                    ),
                });
            }
            let ips = assigned
                .ips
                .iter()
                .map(|ip| parse_ipv4_address(ip))
                .collect::<Result<Vec<_>>>()?;
            let segment_id = assigned
                .segment_id
                .parse::<NetworkSegmentId>()
                .map_err(|error| SandboxError::OperationFailed {
                    message: format!(
                        "existing OCI IPAM reservation for attachment {} contains invalid segment identity {:?}: {error}",
                        attachment_id.as_str(),
                        assigned.segment_id
                    ),
                })?;
            for (block_index, config) in configs.iter().enumerate() {
                if config.segment_id == segment_id.as_str()
                    && allocation_belongs_to_block(config, &ips)?
                {
                    return Ok(BlockIpAllocation {
                        block_index,
                        segment_id,
                        ips,
                    });
                }
            }
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "existing OCI IPAM reservation for attachment {} selects segment {} outside its current ordered tenant block set; refusing to remap the durable allocation",
                    attachment_id.as_str(),
                    segment_id
                ),
            });
        }

        for (block_index, config) in configs.iter().enumerate() {
            let segment_id = config
                .segment_id
                .parse::<NetworkSegmentId>()
                .map_err(|error| SandboxError::OperationFailed {
                    message: format!(
                        "OCI network config contains invalid stable segment identity {:?}: {error}",
                        config.segment_id
                    ),
                })?;
            match allocate_next_ipv4(config, state) {
                Ok(ip) => {
                    let identity_digest = ipam_allocation_identity_digest(
                        &layout.tenant_id,
                        attachment_id,
                        &segment_id,
                        reservation_claim,
                        &provider_locator,
                    )?;
                    state.released_allocations.remove(attachment_id.as_str());
                    state.allocations.insert(
                        attachment_id.as_str().to_owned(),
                        IpamAllocation {
                            segment_id: segment_id.as_str().to_owned(),
                            reservation_claim: reservation_claim.clone(),
                            provider_locator: provider_locator.clone(),
                            identity_digest,
                            ips: vec![ip.to_string()],
                            provider_operation: NetavarkProviderOperation::Reserved,
                        },
                    );
                    state.last_assigned_ip = Some(ip.to_string());
                    return Ok(BlockIpAllocation {
                        block_index,
                        segment_id,
                        ips: vec![ip],
                    });
                }
                Err(SandboxError::NetworkSubnetExhausted { .. }) => {}
                Err(error) => return Err(error),
            }
        }

        Err(SandboxError::NetworkSubnetExhausted {
            subnet: configs
                .iter()
                .map(|config| config.network_subnet.as_str())
                .collect::<Vec<_>>()
                .join(","),
        })
    })
}

#[cfg(test)]
pub(super) fn load_container_ips(
    authority: &OciIpamAuthority,
    layout: &OciNetworkLayout,
    sandbox_id: &SandboxId,
) -> Result<Vec<Ipv4Addr>> {
    let attachment_id = default_network_attachment_id(sandbox_id);
    read_ipam_state(authority, layout)?
        .allocations
        .get(attachment_id.as_str())
        .ok_or_else(|| SandboxError::OperationFailed {
            message: format!(
                "failed to find allocated container IPs for attachment {}",
                attachment_id.as_str()
            ),
        })?
        .ips
        .iter()
        .map(|ip| parse_ipv4_address(ip))
        .collect()
}

#[cfg(test)]
pub(super) fn load_released_container_ips(
    authority: &OciIpamAuthority,
    layout: &OciNetworkLayout,
    config: &OciNetworkConfig,
    _sandbox_id: &SandboxId,
) -> Result<Vec<Ipv4Addr>> {
    let attachment_id = config.attachment_id.clone();
    let state = read_ipam_state(authority, layout)?;
    let released = state
        .released_allocations
        .get(attachment_id.as_str())
        .ok_or_else(|| SandboxError::OperationFailed {
            message: format!(
                "failed to find released container IPs for attachment {}",
                attachment_id.as_str()
            ),
        })?;
    validate_ipam_generation(config, &attachment_id, released)
}

pub(super) fn load_container_ips_for_segment(
    authority: &OciIpamAuthority,
    layout: &OciNetworkLayout,
    config: &OciNetworkConfig,
    sandbox_id: &SandboxId,
) -> Result<Vec<Ipv4Addr>> {
    load_container_ips_for_segment_if_present(authority, layout, config, sandbox_id)?.ok_or_else(
        || SandboxError::OperationFailed {
            message: format!(
                "failed to find allocated container IPs for attachment {}",
                config.attachment_id.as_str()
            ),
        },
    )
}

/// Authenticate either the live allocation or the last exact terminal
/// generation. Returns the exact live addresses only while provider effects
/// may still exist, so callers cannot split authentication from observation.
pub(super) fn authenticate_container_network_generation_for_cleanup(
    authority: &OciIpamAuthority,
    layout: &OciNetworkLayout,
    config: &OciNetworkConfig,
    _sandbox_id: &SandboxId,
) -> Result<Option<Vec<Ipv4Addr>>> {
    let attachment_id = config.attachment_id.clone();
    let state = read_ipam_state(authority, layout)?;
    if let Some(assigned) = state.allocations.get(attachment_id.as_str()) {
        return validate_ipam_generation(config, &attachment_id, assigned).map(Some);
    }
    let released = state
        .released_allocations
        .get(attachment_id.as_str())
        .ok_or_else(|| SandboxError::OperationFailed {
            message: format!(
                "failed to authenticate live or terminal OCI IPAM generation for attachment {}",
                attachment_id.as_str()
            ),
        })?;
    validate_ipam_generation(config, &attachment_id, released)?;
    Ok(None)
}

/// Read-only state of one exact IPAM generation at terminal publication time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ContainerIpamAuthorityState {
    /// Exact addresses remain allocated and continue to fence connectivity.
    Live,
    /// The exact terminal generation remains as a retry/authentication witness.
    Released,
    /// Neither live authority nor a terminal witness remains.
    Absent,
}

/// Inspect one exact IPAM generation without creating, deleting, or retiring it.
pub(super) fn inspect_container_ipam_authority(
    authority: &OciIpamAuthority,
    layout: &OciNetworkLayout,
    config: &OciNetworkConfig,
    _sandbox_id: &SandboxId,
) -> Result<ContainerIpamAuthorityState> {
    let attachment_id = config.attachment_id.clone();
    let state = read_ipam_state(authority, layout)?;
    if let Some(assigned) = state.allocations.get(attachment_id.as_str()) {
        validate_ipam_generation(config, &attachment_id, assigned)?;
        return Ok(ContainerIpamAuthorityState::Live);
    }
    if let Some(released) = state.released_allocations.get(attachment_id.as_str()) {
        validate_ipam_generation(config, &attachment_id, released)?;
        return Ok(ContainerIpamAuthorityState::Released);
    }
    Ok(ContainerIpamAuthorityState::Absent)
}

pub(super) fn load_container_ips_for_segment_if_present(
    authority: &OciIpamAuthority,
    layout: &OciNetworkLayout,
    config: &OciNetworkConfig,
    _sandbox_id: &SandboxId,
) -> Result<Option<Vec<Ipv4Addr>>> {
    let attachment_id = config.attachment_id.clone();
    let state = read_ipam_state(authority, layout)?;
    state
        .allocations
        .get(attachment_id.as_str())
        .map(|assigned| validate_ipam_generation(config, &attachment_id, assigned))
        .transpose()
}

fn validate_ipam_generation(
    config: &OciNetworkConfig,
    attachment_id: &nimbus_network::NetworkAttachmentId,
    assigned: &IpamAllocation,
) -> Result<Vec<Ipv4Addr>> {
    let expected_segment_id = config
        .segment_id
        .parse::<NetworkSegmentId>()
        .map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "OCI network config contains invalid stable segment identity {:?}: {error}",
                config.segment_id
            ),
        })?;
    if assigned.reservation_claim != config.reservation_claim {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "OCI IPAM reservation for attachment {} belongs to a different launch coordinator; refusing stale provider work against a replacement generation",
                attachment_id.as_str()
            ),
        });
    }
    if assigned.segment_id != expected_segment_id.as_str() {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "OCI IPAM reservation for attachment {} selects segment {} but the provider operation requested {}; refusing to remap the durable allocation",
                attachment_id.as_str(),
                assigned.segment_id,
                expected_segment_id
            ),
        });
    }
    let ips = assigned
        .ips
        .iter()
        .map(|ip| parse_ipv4_address(ip))
        .collect::<Result<Vec<_>>>()?;
    if !allocation_belongs_to_block(config, &ips)? {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "OCI IPAM reservation for attachment {} contains addresses outside segment {}",
                attachment_id.as_str(),
                expected_segment_id
            ),
        });
    }
    Ok(ips)
}

/// Remove IPAM only after the provider and persistent namespace for this exact
/// sandbox incarnation are confirmed absent. The terminal generation remains
/// as a comparison-only tombstone until a replacement allocation atomically
/// overwrites it.
///
/// Pre-effect compensation must use [`deallocate_container_ips_for_claim`]
/// instead; both operations require the exact launch-coordinator fence.
pub(crate) fn deallocate_container_ips_after_confirmed_detach(
    authority: &OciIpamAuthority,
    layout: &OciNetworkLayout,
    sandbox_id: &SandboxId,
    attachment_id: &NetworkAttachmentId,
    reservation_claim: &NetworkReservationClaim,
    provider_kind: OciAttachmentProviderKind,
) -> Result<()> {
    with_ipam_state(authority, layout, |state| {
        if let Some(allocation) = state.allocations.get(attachment_id.as_str()) {
            if &allocation.reservation_claim != reservation_claim {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "confirmed-detach IPAM release for attachment {} carries a stale launch coordinator; refusing to delete a replacement generation",
                        attachment_id.as_str()
                    ),
                });
            }
            ensure_netavark_release_ready(
                layout,
                sandbox_id,
                provider_kind,
                attachment_id,
                allocation,
                "confirmed-detach IPAM release",
            )?;
            let allocation = state
                .allocations
                .remove(attachment_id.as_str())
                .expect("allocation inspected under the same transaction");
            state
                .released_allocations
                .insert(attachment_id.as_str().to_owned(), allocation);
            return Ok(());
        }
        authenticate_terminal_ipam_release(
            state,
            layout,
            sandbox_id,
            provider_kind,
            attachment_id,
            reservation_claim,
        )
    })
}

pub(super) fn deallocate_container_ips_for_claim(
    authority: &OciIpamAuthority,
    layout: &OciNetworkLayout,
    sandbox_id: &SandboxId,
    attachment_id: &NetworkAttachmentId,
    reservation_claim: &NetworkReservationClaim,
    provider_kind: OciAttachmentProviderKind,
) -> Result<()> {
    with_ipam_state(authority, layout, |state| {
        if let Some(allocation) = state.allocations.get(attachment_id.as_str()) {
            if &allocation.reservation_claim != reservation_claim {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "OCI IPAM release for attachment {} carries a stale launch coordinator; refusing to delete a newer generation",
                        attachment_id.as_str()
                    ),
                });
            }
            ensure_netavark_release_ready(
                layout,
                sandbox_id,
                provider_kind,
                attachment_id,
                allocation,
                "never-realized IPAM release",
            )?;
            let allocation = state
                .allocations
                .remove(attachment_id.as_str())
                .expect("allocation inspected under the same transaction");
            state
                .released_allocations
                .insert(attachment_id.as_str().to_owned(), allocation);
            return Ok(());
        }
        let Some(released) = state.released_allocations.get(attachment_id.as_str()) else {
            return Ok(());
        };
        ensure_netavark_release_ready(
            layout,
            sandbox_id,
            provider_kind,
            attachment_id,
            released,
            "never-realized terminal IPAM reconciliation",
        )?;
        if &released.reservation_claim == reservation_claim {
            return Ok(());
        }
        // The only caller has already authenticated this newer claim into the
        // segment authority's reservation-cleanup-pending state. With no live
        // allocation, a foreign terminal entry can therefore only be an older
        // completed generation that the newer launch never replaced because
        // it failed before its IPAM transaction committed.
        state.released_allocations.remove(attachment_id.as_str());
        Ok(())
    })
}

/// Compare-delete one terminal IPAM retry witness after its owning lifecycle
/// is durably final.
///
/// A live allocation or a foreign terminal generation is never mutated. The
/// boolean reports whether the exact tombstone was retired.
pub(crate) fn retire_terminal_container_ipam_release(
    authority: &OciIpamAuthority,
    layout: &OciNetworkLayout,
    sandbox_id: &SandboxId,
    attachment_id: &NetworkAttachmentId,
    reservation_claim: &NetworkReservationClaim,
    provider_kind: OciAttachmentProviderKind,
) -> Result<bool> {
    let observed = read_ipam_state(authority, layout)?;
    if observed.allocations.contains_key(attachment_id.as_str())
        || !observed
            .released_allocations
            .get(attachment_id.as_str())
            .is_some_and(|released| &released.reservation_claim == reservation_claim)
    {
        return Ok(false);
    }
    with_ipam_state(authority, layout, |state| {
        if state.allocations.contains_key(attachment_id.as_str()) {
            return Ok(false);
        }
        if state
            .released_allocations
            .get(attachment_id.as_str())
            .is_some_and(|released| &released.reservation_claim == reservation_claim)
        {
            let released = state
                .released_allocations
                .get(attachment_id.as_str())
                .expect("terminal allocation inspected under the same transaction");
            ensure_netavark_release_ready(
                layout,
                sandbox_id,
                provider_kind,
                attachment_id,
                released,
                "terminal IPAM retirement",
            )?;
            state.released_allocations.remove(attachment_id.as_str());
            return Ok(true);
        }
        Ok(false)
    })
}

/// Reclaim crash-left terminal IPAM witnesses before accepting new launches.
///
/// Manifests are the durable lifecycle authority. Only terminal container
/// manifests with an explicit completed-network-cleanup witness and terminal
/// krun manifests with `Released` authority qualify. Exact compare-delete
/// keeps a stale manifest from mutating a replacement live or terminal
/// generation.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "NNC8.3 owns explicit cleanup convergence; NNC5.2d removes retirement from startup admission"
    )
)]
pub(crate) fn reconcile_terminal_container_ipam_releases(
    authority: &OciIpamAuthority,
    workload_state_root: &Path,
) -> Result<usize> {
    let manifest_paths = crate::artifact_paths::all_manifest_paths(workload_state_root).map_err(
        |error| SandboxError::OperationFailed {
            message: format!(
                "failed to enumerate manifests for terminal IPAM reconciliation under {}: {error}",
                workload_state_root.display()
            ),
        },
    )?;
    let mut retired = 0usize;
    for manifest_path in manifest_paths {
        let metadata = std::fs::symlink_metadata(&manifest_path).map_err(|error| {
            SandboxError::OperationFailed {
                message: format!(
                    "failed to inspect manifest {} for terminal IPAM reconciliation: {error}",
                    manifest_path.display()
                ),
            }
        })?;
        if !metadata.file_type().is_file() {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "manifest {} is not a regular file during terminal IPAM reconciliation",
                    manifest_path.display()
                ),
            });
        }
        let bytes =
            std::fs::read(&manifest_path).map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to read manifest {} for terminal IPAM reconciliation: {error}",
                    manifest_path.display()
                ),
            })?;
        let manifest: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to parse manifest {} for terminal IPAM reconciliation: {error}",
                    manifest_path.display()
                ),
            })?;
        let sandbox_id = serde_json::from_value::<SandboxId>(
            manifest
                .pointer("/handle/id")
                .cloned()
                .ok_or_else(|| SandboxError::OperationFailed {
                    message: format!(
                        "manifest {} lacks sandbox identity during terminal IPAM reconciliation",
                        manifest_path.display()
                    ),
                })?,
        )
        .map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "manifest {} has invalid sandbox identity during terminal IPAM reconciliation: {error}",
                manifest_path.display()
            ),
        })?;
        let spec_tenant_id = serde_json::from_value::<TenantId>(
            manifest
                .pointer("/spec/tenant_id")
                .cloned()
                .ok_or_else(|| SandboxError::OperationFailed {
                    message: format!(
                        "manifest {} lacks spec tenant identity during terminal IPAM reconciliation",
                        manifest_path.display()
                    ),
                })?,
        )
        .map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "manifest {} has invalid spec tenant identity during terminal IPAM reconciliation: {error}",
                manifest_path.display()
            ),
        })?;
        let handle_tenant_id = serde_json::from_value::<TenantId>(
            manifest
                .pointer("/handle/tenant_id")
                .cloned()
                .ok_or_else(|| SandboxError::OperationFailed {
                    message: format!(
                        "manifest {} lacks handle tenant identity during terminal IPAM reconciliation",
                        manifest_path.display()
                    ),
                })?,
        )
        .map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "manifest {} has invalid handle tenant identity during terminal IPAM reconciliation: {error}",
                manifest_path.display()
            ),
        })?;
        if handle_tenant_id != spec_tenant_id {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "manifest {} crosses tenant identities during terminal IPAM reconciliation: \
                     handle tenant {} differs from spec tenant {}",
                    manifest_path.display(),
                    handle_tenant_id,
                    spec_tenant_id
                ),
            });
        }
        let expected_manifest_path =
            crate::artifact_paths::manifest_path(workload_state_root, &spec_tenant_id, &sandbox_id);
        if manifest_path != expected_manifest_path {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "manifest {} does not match trusted tenant/sandbox path {} during terminal IPAM \
                     reconciliation",
                    manifest_path.display(),
                    expected_manifest_path.display()
                ),
            });
        }
        let status =
            serde_json::from_value::<SandboxStatus>(manifest.get("status").cloned().ok_or_else(
                || SandboxError::OperationFailed {
                    message: format!(
                        "manifest {} lacks status during terminal IPAM reconciliation",
                        manifest_path.display()
                    ),
                },
            )?)
            .map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "manifest {} has invalid status during terminal IPAM reconciliation: {error}",
                    manifest_path.display()
                ),
            })?;
        if !matches!(status, SandboxStatus::Stopped | SandboxStatus::Failed)
            || !manifest
                .get("launch_artifact")
                .is_some_and(serde_json::Value::is_null)
        {
            continue;
        }
        let container_final = manifest
            .get("network_cleanup_complete")
            .and_then(serde_json::Value::as_bool)
            == Some(true);
        let krun_final = manifest
            .pointer("/launch_authority/phase")
            .and_then(serde_json::Value::as_str)
            == Some("released");
        if !container_final && !krun_final {
            continue;
        }
        let Some(network_config) = manifest.get("network_config") else {
            continue;
        };
        if network_config.is_null() {
            continue;
        }
        let network_config =
            serde_json::from_value::<OciNetworkConfig>(network_config.clone()).map_err(|error| {
                SandboxError::OperationFailed {
                    message: format!(
                        "manifest {} has invalid network config during terminal IPAM reconciliation: {error}",
                        manifest_path.display()
                    ),
                }
            })?;
        let network_layout = serde_json::from_value::<OciNetworkLayout>(
            manifest
                .get("network_layout")
                .cloned()
                .ok_or_else(|| SandboxError::OperationFailed {
                    message: format!(
                        "manifest {} lacks network layout during terminal IPAM reconciliation",
                        manifest_path.display()
                    ),
                })?,
        )
        .map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "manifest {} has invalid network layout during terminal IPAM reconciliation: {error}",
                manifest_path.display()
            ),
        })?;
        let expected_network_layout = OciNetworkLayout::with_roots(
            workload_state_root,
            authority.state_root(),
            &spec_tenant_id,
            &sandbox_id,
        );
        authority
            .authenticate_layout(&network_layout)
            .map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "manifest {} carries an untrusted network layout during terminal IPAM \
                     reconciliation: {error}",
                    manifest_path.display()
                ),
            })?;
        let mut authenticated_network_layout = network_layout.clone();
        authenticated_network_layout.network_state_root = authority.state_root().to_path_buf();
        if authenticated_network_layout != expected_network_layout {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "manifest {} carries an untrusted network layout during terminal IPAM \
                     reconciliation",
                    manifest_path.display()
                ),
            });
        }
        retired += usize::from(retire_terminal_container_ipam_release(
            authority,
            &network_layout,
            &sandbox_id,
            &network_config.attachment_id,
            &network_config.reservation_claim,
            network_config.provider_kind(),
        )?);
    }
    Ok(retired)
}

fn authenticate_terminal_ipam_release(
    state: &IpamState,
    layout: &OciNetworkLayout,
    sandbox_id: &SandboxId,
    provider_kind: OciAttachmentProviderKind,
    attachment_id: &nimbus_network::NetworkAttachmentId,
    reservation_claim: &NetworkReservationClaim,
) -> Result<()> {
    let Some(released) = state.released_allocations.get(attachment_id.as_str()) else {
        // No IPAM effect ever occurred, so there is nothing in this partition
        // to compensate. Callers still need their segment/port coordinator
        // evidence before mutating those independent authorities.
        return Ok(());
    };
    if &released.reservation_claim != reservation_claim {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "OCI IPAM release for attachment {} carries a stale launch coordinator; refusing to accept a replacement generation's terminal evidence",
                attachment_id.as_str()
            ),
        });
    }
    ensure_netavark_release_ready(
        layout,
        sandbox_id,
        provider_kind,
        attachment_id,
        released,
        "terminal IPAM release replay",
    )?;
    Ok(())
}

fn ensure_netavark_release_ready(
    layout: &OciNetworkLayout,
    sandbox_id: &SandboxId,
    provider_kind: OciAttachmentProviderKind,
    attachment_id: &NetworkAttachmentId,
    allocation: &IpamAllocation,
    operation: &str,
) -> Result<()> {
    let expected_locator = OciAttachmentProviderLocator::new(
        &layout.workload_state_root,
        &layout.tenant_id,
        sandbox_id,
        provider_kind,
    )?;
    if allocation.provider_locator != expected_locator {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "{operation} for attachment {} carries a different provider locator; refusing \
                 terminal IPAM mutation through a substituted tenant, sandbox, artifact realm, or \
                 backend",
                attachment_id.as_str()
            ),
        });
    }
    if allocation
        .provider_operation
        .permits_terminal_ipam_release()
    {
        return Ok(());
    }
    Err(SandboxError::OperationFailed {
        message: format!(
            "{operation} for attachment {} is fenced because Netavark provider operation remains {}; refusing IPAM release or replacement",
            attachment_id.as_str(),
            allocation.provider_operation.label()
        ),
    })
}

fn with_ipam_state<T>(
    authority: &OciIpamAuthority,
    layout: &OciNetworkLayout,
    mutator: impl FnOnce(&mut IpamState) -> Result<T>,
) -> Result<T> {
    authority.transaction(layout, |state| {
        authenticate_ipam_state(&layout.tenant_id, state)?;
        let result = mutator(state)?;
        authenticate_ipam_state(&layout.tenant_id, state)?;
        Ok(result)
    })
}

fn read_ipam_state(authority: &OciIpamAuthority, layout: &OciNetworkLayout) -> Result<IpamState> {
    let state = authority.read(layout)?;
    authenticate_ipam_state(&layout.tenant_id, &state)?;
    Ok(state)
}

fn authenticate_ipam_state(tenant_id: &TenantId, state: &IpamState) -> Result<()> {
    for (attachment_key, allocation) in state
        .allocations
        .iter()
        .chain(state.released_allocations.iter())
    {
        let attachment_id = attachment_key
            .parse::<NetworkAttachmentId>()
            .map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "OCI IPAM authority for tenant {} contains invalid attachment key \
                     {attachment_key:?}: {error}",
                    tenant_id.as_str()
                ),
            })?;
        authenticate_ipam_allocation_identity(tenant_id, &attachment_id, allocation)?;
    }
    Ok(())
}

pub(super) fn authenticate_ipam_allocation_identity(
    tenant_id: &TenantId,
    attachment_id: &NetworkAttachmentId,
    allocation: &IpamAllocation,
) -> Result<()> {
    let segment_id = allocation
        .segment_id
        .parse::<NetworkSegmentId>()
        .map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "OCI IPAM allocation for tenant {} attachment {} contains invalid segment \
                 identity {:?}: {error}",
                tenant_id.as_str(),
                attachment_id.as_str(),
                allocation.segment_id
            ),
        })?;
    let expected = ipam_allocation_identity_digest(
        tenant_id,
        attachment_id,
        &segment_id,
        &allocation.reservation_claim,
        &allocation.provider_locator,
    )?;
    if allocation.identity_digest != expected {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "OCI IPAM allocation identity digest mismatch for tenant {} attachment {}; \
                 refusing a substituted attachment, segment, coordinator, or provider locator",
                tenant_id.as_str(),
                attachment_id.as_str()
            ),
        });
    }
    Ok(())
}

fn ipam_allocation_identity_digest(
    tenant_id: &TenantId,
    attachment_id: &NetworkAttachmentId,
    segment_id: &NetworkSegmentId,
    reservation_claim: &NetworkReservationClaim,
    provider_locator: &OciAttachmentProviderLocator,
) -> Result<String> {
    let claim =
        serde_json::to_vec(reservation_claim).map_err(|error| SandboxError::OperationFailed {
            message: format!("failed to encode OCI IPAM reservation identity: {error}"),
        })?;
    let locator =
        serde_json::to_vec(provider_locator).map_err(|error| SandboxError::OperationFailed {
            message: format!("failed to encode OCI IPAM provider locator identity: {error}"),
        })?;
    let mut digest = Sha256::new();
    digest.update(IPAM_ALLOCATION_IDENTITY_DOMAIN);
    for field in [
        tenant_id.as_str().as_bytes(),
        attachment_id.as_str().as_bytes(),
        segment_id.as_str().as_bytes(),
        claim.as_slice(),
        locator.as_slice(),
    ] {
        digest.update((field.len() as u64).to_be_bytes());
        digest.update(field);
    }
    Ok(digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn ipam_store_error(error: impl std::fmt::Display) -> SandboxError {
    SandboxError::OperationFailed {
        message: format!("OCI IPAM network authority failed: {error}"),
    }
}

fn allocation_belongs_to_block(config: &OciNetworkConfig, allocation: &[Ipv4Addr]) -> Result<bool> {
    let subnet = parse_ipv4_bridge_subnet(&config.network_subnet)?;
    let gateway = ipv4_to_u32(subnet.gateway);
    let broadcast = ipv4_to_u32(subnet.broadcast);
    Ok(!allocation.is_empty()
        && allocation.iter().all(|ip| {
            let ip = ipv4_to_u32(*ip);
            ip > gateway && ip < broadcast
        }))
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
        .flat_map(|allocation| &allocation.ips)
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
mod tests;
