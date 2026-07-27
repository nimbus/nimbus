//! Tenant-scoped static IPv4 allocation for OCI bridge networks.

use std::collections::BTreeSet;
use std::net::Ipv4Addr;
use std::path::Path;

use nimbus_core::TenantId;
use nimbus_network::{
    LocalNetworkStateStore, NetworkAttachmentId, NetworkProviderHandle, NetworkProviderId,
    NetworkReservationClaim, NetworkSegmentId, NetworkStatePartition, NetworkStateTransactionError,
};
use ulid::Ulid;

use crate::error::{Result, SandboxError};
use crate::instance::{SandboxId, SandboxStatus};

use super::default_network_attachment_id;
use super::dto::{IpamAllocation, IpamState, NetavarkProviderOperation};
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

#[cfg(test)]
pub(crate) fn allocate_container_ips(
    layout: &OciNetworkLayout,
    config: &OciNetworkConfig,
    sandbox_id: &SandboxId,
) -> Result<Vec<Ipv4Addr>> {
    allocate_container_ips_on_first_available(
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

/// Attempt-specific capability for completing one durable Netavark setup.
///
/// The capability is returned only to the call stack that published the
/// `Provisioning` transition. A same-generation concurrent caller therefore
/// cannot take over a live or ambiguous provider effect merely by replaying the
/// launch reservation claim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct NetavarkSetupClaim {
    attachment_id: NetworkAttachmentId,
    reservation_claim: NetworkReservationClaim,
    operation_attempt: NetworkProviderHandle,
}

/// Attempt-specific capability for completing one durable Netavark teardown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct NetavarkTeardownClaim {
    attachment_id: NetworkAttachmentId,
    reservation_claim: NetworkReservationClaim,
    operation_attempt: NetworkProviderHandle,
}

/// Provider work selected atomically from current IPAM authority.
pub(super) enum NetavarkTeardownPlan {
    /// Run Netavark teardown, then publish provider absence.
    Run {
        assigned_ips: Vec<Ipv4Addr>,
        claim: NetavarkTeardownClaim,
    },
    /// Provider absence is already durable; only the observed projection remains.
    RemoveProjection { claim: NetavarkTeardownClaim },
    /// Exact live or terminal authority already proves no provider work remains.
    AlreadyDetached,
}

/// Atomically choose and reserve the first available address across every
/// existing tenant block.
///
/// The caller supplies configs in durable segment-allocation order. The whole
/// scan and reservation occurs in one tenant-IPAM transaction under the shared
/// network authority lock, so concurrent placers cannot select the same
/// address. Existing idempotent reservations are mapped back to their owning
/// block and fail closed if that block is no longer in the supplied set.
pub(super) fn allocate_container_ips_on_first_available(
    layout: &OciNetworkLayout,
    configs: &[OciNetworkConfig],
    sandbox_id: &SandboxId,
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

    let attachment_id = default_network_attachment_id(sandbox_id);
    with_ipam_state(layout, |state| {
        if let Some(assigned) = state.allocations.get(attachment_id.as_str()) {
            if &assigned.reservation_claim != reservation_claim {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "existing OCI IPAM reservation for attachment {} belongs to a different launch coordinator; refusing cross-generation adoption",
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
                    state.released_allocations.remove(attachment_id.as_str());
                    state.allocations.insert(
                        attachment_id.as_str().to_owned(),
                        IpamAllocation {
                            segment_id: segment_id.as_str().to_owned(),
                            reservation_claim: reservation_claim.clone(),
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
    layout: &OciNetworkLayout,
    sandbox_id: &SandboxId,
) -> Result<Vec<Ipv4Addr>> {
    let attachment_id = default_network_attachment_id(sandbox_id);
    read_ipam_state(layout)?
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

pub(super) fn load_container_ips_for_segment(
    layout: &OciNetworkLayout,
    config: &OciNetworkConfig,
    sandbox_id: &SandboxId,
) -> Result<Vec<Ipv4Addr>> {
    load_container_ips_for_segment_if_present(layout, config, sandbox_id)?.ok_or_else(|| {
        SandboxError::OperationFailed {
            message: format!(
                "failed to find allocated container IPs for attachment {}",
                default_network_attachment_id(sandbox_id).as_str()
            ),
        }
    })
}

pub(super) fn begin_netavark_setup(
    layout: &OciNetworkLayout,
    config: &OciNetworkConfig,
    sandbox_id: &SandboxId,
) -> Result<(Vec<Ipv4Addr>, NetavarkSetupClaim)> {
    let attachment_id = default_network_attachment_id(sandbox_id);
    let operation_attempt = new_netavark_operation_attempt("setup", &attachment_id)?;
    with_ipam_state(layout, |state| {
        let allocation = state
            .allocations
            .get_mut(attachment_id.as_str())
            .ok_or_else(|| SandboxError::OperationFailed {
                message: format!(
                    "failed to find allocated container IPs for attachment {}",
                    attachment_id.as_str()
                ),
            })?;
        let assigned_ips = validate_ipam_generation(config, &attachment_id, allocation)?;
        match &allocation.provider_operation {
            NetavarkProviderOperation::Reserved | NetavarkProviderOperation::Detached => {}
            current => {
                return Err(netavark_operation_pending(&attachment_id, "setup", current));
            }
        }
        allocation.provider_operation = NetavarkProviderOperation::Provisioning {
            operation_attempt: operation_attempt.clone(),
        };
        Ok((
            assigned_ips,
            NetavarkSetupClaim {
                attachment_id: attachment_id.clone(),
                reservation_claim: config.reservation_claim.clone(),
                operation_attempt: operation_attempt.clone(),
            },
        ))
    })
}

pub(super) fn complete_netavark_setup(
    layout: &OciNetworkLayout,
    claim: &NetavarkSetupClaim,
) -> Result<()> {
    with_ipam_state(layout, |state| {
        let allocation = exact_live_allocation_for_setup_claim(state, claim)?;
        match &allocation.provider_operation {
            NetavarkProviderOperation::Provisioning { operation_attempt }
                if operation_attempt == &claim.operation_attempt =>
            {
                allocation.provider_operation = NetavarkProviderOperation::Ready {
                    setup_attempt: claim.operation_attempt.clone(),
                };
                Ok(())
            }
            NetavarkProviderOperation::Ready { setup_attempt }
                if setup_attempt == &claim.operation_attempt =>
            {
                Ok(())
            }
            current => Err(netavark_claim_mismatch(
                &claim.attachment_id,
                "setup completion",
                current,
            )),
        }
    })
}

pub(super) fn begin_netavark_teardown(
    layout: &OciNetworkLayout,
    config: &OciNetworkConfig,
    sandbox_id: &SandboxId,
    setup_claim: Option<&NetavarkSetupClaim>,
) -> Result<NetavarkTeardownPlan> {
    let attachment_id = default_network_attachment_id(sandbox_id);
    if let Some(claim) = setup_claim {
        validate_setup_claim_identity(claim, &attachment_id, &config.reservation_claim)?;
    }
    with_ipam_state(layout, |state| {
        let Some(allocation) = state.allocations.get_mut(attachment_id.as_str()) else {
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
            return Ok(NetavarkTeardownPlan::AlreadyDetached);
        };
        let assigned_ips = validate_ipam_generation(config, &attachment_id, allocation)?;
        match &allocation.provider_operation {
            NetavarkProviderOperation::Reserved => Ok(NetavarkTeardownPlan::AlreadyDetached),
            NetavarkProviderOperation::Ready { .. } => {
                if let (Some(setup_claim), NetavarkProviderOperation::Ready { setup_attempt }) =
                    (setup_claim, &allocation.provider_operation)
                    && setup_attempt != &setup_claim.operation_attempt
                {
                    return Err(netavark_claim_mismatch(
                        &attachment_id,
                        "setup compensation",
                        &allocation.provider_operation,
                    ));
                }
                let new_attempt = new_netavark_operation_attempt("teardown", &attachment_id)?;
                allocation.provider_operation = NetavarkProviderOperation::Deleting {
                    operation_attempt: new_attempt.clone(),
                };
                Ok(NetavarkTeardownPlan::Run {
                    assigned_ips,
                    claim: NetavarkTeardownClaim {
                        attachment_id: attachment_id.clone(),
                        reservation_claim: config.reservation_claim.clone(),
                        operation_attempt: new_attempt.clone(),
                    },
                })
            }
            NetavarkProviderOperation::Provisioning { operation_attempt } => {
                let Some(setup_claim) = setup_claim else {
                    return Err(netavark_operation_pending(
                        &attachment_id,
                        "teardown",
                        &allocation.provider_operation,
                    ));
                };
                if operation_attempt != &setup_claim.operation_attempt {
                    return Err(netavark_claim_mismatch(
                        &attachment_id,
                        "setup compensation",
                        &allocation.provider_operation,
                    ));
                }
                let new_attempt = new_netavark_operation_attempt("teardown", &attachment_id)?;
                allocation.provider_operation = NetavarkProviderOperation::Deleting {
                    operation_attempt: new_attempt.clone(),
                };
                Ok(NetavarkTeardownPlan::Run {
                    assigned_ips,
                    claim: NetavarkTeardownClaim {
                        attachment_id: attachment_id.clone(),
                        reservation_claim: config.reservation_claim.clone(),
                        operation_attempt: new_attempt.clone(),
                    },
                })
            }
            NetavarkProviderOperation::Deleting { .. } => Err(netavark_operation_pending(
                &attachment_id,
                "teardown",
                &allocation.provider_operation,
            )),
            NetavarkProviderOperation::DetachedProjectionPending { operation_attempt } => {
                Ok(NetavarkTeardownPlan::RemoveProjection {
                    claim: NetavarkTeardownClaim {
                        attachment_id: attachment_id.clone(),
                        reservation_claim: config.reservation_claim.clone(),
                        operation_attempt: operation_attempt.clone(),
                    },
                })
            }
            NetavarkProviderOperation::Detached => Ok(NetavarkTeardownPlan::AlreadyDetached),
        }
    })
}

pub(super) fn confirm_netavark_provider_detached(
    layout: &OciNetworkLayout,
    claim: &NetavarkTeardownClaim,
) -> Result<()> {
    with_ipam_state(layout, |state| {
        let allocation = exact_live_allocation_for_teardown_claim(state, claim)?;
        match &allocation.provider_operation {
            NetavarkProviderOperation::Deleting { operation_attempt }
                if operation_attempt == &claim.operation_attempt =>
            {
                allocation.provider_operation =
                    NetavarkProviderOperation::DetachedProjectionPending {
                        operation_attempt: claim.operation_attempt.clone(),
                    };
                Ok(())
            }
            NetavarkProviderOperation::DetachedProjectionPending { operation_attempt }
                if operation_attempt == &claim.operation_attempt =>
            {
                Ok(())
            }
            current => Err(netavark_claim_mismatch(
                &claim.attachment_id,
                "provider-detach confirmation",
                current,
            )),
        }
    })
}

pub(super) fn complete_netavark_teardown(
    layout: &OciNetworkLayout,
    claim: &NetavarkTeardownClaim,
) -> Result<()> {
    with_ipam_state(layout, |state| {
        let allocation = exact_live_allocation_for_teardown_claim(state, claim)?;
        match &allocation.provider_operation {
            NetavarkProviderOperation::DetachedProjectionPending { operation_attempt }
                if operation_attempt == &claim.operation_attempt =>
            {
                allocation.provider_operation = NetavarkProviderOperation::Detached;
                Ok(())
            }
            NetavarkProviderOperation::Detached => Ok(()),
            current => Err(netavark_claim_mismatch(
                &claim.attachment_id,
                "teardown completion",
                current,
            )),
        }
    })
}

/// Authenticate either the live allocation or the last exact terminal
/// generation. Returns the exact live addresses only while provider effects
/// may still exist, so callers cannot split authentication from observation.
pub(super) fn authenticate_container_network_generation_for_cleanup(
    layout: &OciNetworkLayout,
    config: &OciNetworkConfig,
    sandbox_id: &SandboxId,
) -> Result<Option<Vec<Ipv4Addr>>> {
    let attachment_id = default_network_attachment_id(sandbox_id);
    let state = read_ipam_state(layout)?;
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

fn load_container_ips_for_segment_if_present(
    layout: &OciNetworkLayout,
    config: &OciNetworkConfig,
    sandbox_id: &SandboxId,
) -> Result<Option<Vec<Ipv4Addr>>> {
    let attachment_id = default_network_attachment_id(sandbox_id);
    let state = read_ipam_state(layout)?;
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

fn exact_live_allocation_for_setup_claim<'a>(
    state: &'a mut IpamState,
    claim: &NetavarkSetupClaim,
) -> Result<&'a mut IpamAllocation> {
    exact_live_allocation_for_operation(
        state,
        &claim.attachment_id,
        &claim.reservation_claim,
        "setup",
    )
}

fn exact_live_allocation_for_teardown_claim<'a>(
    state: &'a mut IpamState,
    claim: &NetavarkTeardownClaim,
) -> Result<&'a mut IpamAllocation> {
    exact_live_allocation_for_operation(
        state,
        &claim.attachment_id,
        &claim.reservation_claim,
        "teardown",
    )
}

fn exact_live_allocation_for_operation<'a>(
    state: &'a mut IpamState,
    attachment_id: &NetworkAttachmentId,
    reservation_claim: &NetworkReservationClaim,
    operation: &str,
) -> Result<&'a mut IpamAllocation> {
    let allocation = state
        .allocations
        .get_mut(attachment_id.as_str())
        .ok_or_else(|| SandboxError::OperationFailed {
            message: format!(
                "OCI Netavark {operation} claim for attachment {} has no live IPAM generation",
                attachment_id.as_str()
            ),
        })?;
    if &allocation.reservation_claim != reservation_claim {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "OCI Netavark {operation} claim for attachment {} belongs to a stale launch coordinator",
                attachment_id.as_str()
            ),
        });
    }
    Ok(allocation)
}

fn validate_setup_claim_identity(
    claim: &NetavarkSetupClaim,
    attachment_id: &NetworkAttachmentId,
    reservation_claim: &NetworkReservationClaim,
) -> Result<()> {
    if &claim.attachment_id == attachment_id && &claim.reservation_claim == reservation_claim {
        return Ok(());
    }
    Err(SandboxError::OperationFailed {
        message: format!(
            "OCI Netavark setup compensation for attachment {} carries a foreign operation capability",
            attachment_id.as_str()
        ),
    })
}

fn new_netavark_operation_attempt(
    action: &str,
    attachment_id: &NetworkAttachmentId,
) -> Result<NetworkProviderHandle> {
    let provider_id =
        NetworkProviderId::for_registration_key("nimbus-sandbox.oci.netavark-operation");
    NetworkProviderHandle::new(
        provider_id,
        format!("{action}:{}:{}", attachment_id.as_str(), Ulid::new()),
    )
    .map_err(|error| SandboxError::OperationFailed {
        message: format!(
            "failed to mint Netavark {action} operation capability for attachment {}: {error}",
            attachment_id.as_str()
        ),
    })
}

fn netavark_operation_pending(
    attachment_id: &NetworkAttachmentId,
    requested_operation: &str,
    current: &NetavarkProviderOperation,
) -> SandboxError {
    SandboxError::OperationFailed {
        message: format!(
            "OCI Netavark {requested_operation} for attachment {} found provider operation {} already {}; inspect-before-retry reconciliation is required",
            attachment_id.as_str(),
            current.label(),
            if matches!(
                current,
                NetavarkProviderOperation::Provisioning { .. }
                    | NetavarkProviderOperation::Deleting { .. }
            ) {
                "pending"
            } else {
                "durable"
            }
        ),
    }
}

fn netavark_claim_mismatch(
    attachment_id: &NetworkAttachmentId,
    operation: &str,
    current: &NetavarkProviderOperation,
) -> SandboxError {
    SandboxError::OperationFailed {
        message: format!(
            "OCI Netavark {operation} for attachment {} does not own the durable {} operation capability",
            attachment_id.as_str(),
            current.label()
        ),
    }
}

/// Remove IPAM only after the provider and persistent namespace for this exact
/// sandbox incarnation are confirmed absent. The terminal generation remains
/// as a comparison-only tombstone until a replacement allocation atomically
/// overwrites it.
///
/// Pre-effect compensation must use [`deallocate_container_ips_for_claim`]
/// instead; both operations require the exact launch-coordinator fence.
pub(crate) fn deallocate_container_ips_after_confirmed_detach(
    layout: &OciNetworkLayout,
    sandbox_id: &SandboxId,
    reservation_claim: &NetworkReservationClaim,
) -> Result<()> {
    let attachment_id = default_network_attachment_id(sandbox_id);
    with_ipam_state(layout, |state| {
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
                &attachment_id,
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
        authenticate_terminal_ipam_release(state, &attachment_id, reservation_claim)
    })
}

pub(super) fn deallocate_container_ips_for_claim(
    layout: &OciNetworkLayout,
    sandbox_id: &SandboxId,
    reservation_claim: &NetworkReservationClaim,
) -> Result<()> {
    let attachment_id = default_network_attachment_id(sandbox_id);
    with_ipam_state(layout, |state| {
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
                &attachment_id,
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
    layout: &OciNetworkLayout,
    sandbox_id: &SandboxId,
    reservation_claim: &NetworkReservationClaim,
) -> Result<bool> {
    let attachment_id = default_network_attachment_id(sandbox_id);
    let observed = read_ipam_state(layout)?;
    if observed.allocations.contains_key(attachment_id.as_str())
        || !observed
            .released_allocations
            .get(attachment_id.as_str())
            .is_some_and(|released| &released.reservation_claim == reservation_claim)
    {
        return Ok(false);
    }
    with_ipam_state(layout, |state| {
        if state.allocations.contains_key(attachment_id.as_str()) {
            return Ok(false);
        }
        if state
            .released_allocations
            .get(attachment_id.as_str())
            .is_some_and(|released| &released.reservation_claim == reservation_claim)
        {
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
pub(crate) fn reconcile_terminal_container_ipam_releases(state_root: &Path) -> Result<usize> {
    let manifest_paths = crate::artifact_paths::all_manifest_paths(state_root).map_err(
        |error| SandboxError::OperationFailed {
            message: format!(
                "failed to enumerate manifests for terminal IPAM reconciliation under {}: {error}",
                state_root.display()
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
            crate::artifact_paths::manifest_path(state_root, &spec_tenant_id, &sandbox_id);
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
        let expected_network_layout =
            OciNetworkLayout::new(state_root, &spec_tenant_id, &sandbox_id);
        if network_layout != expected_network_layout {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "manifest {} carries an untrusted network layout during terminal IPAM \
                     reconciliation",
                    manifest_path.display()
                ),
            });
        }
        retired += usize::from(retire_terminal_container_ipam_release(
            &network_layout,
            &sandbox_id,
            &network_config.reservation_claim,
        )?);
    }
    Ok(retired)
}

fn authenticate_terminal_ipam_release(
    state: &IpamState,
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
    Ok(())
}

fn ensure_netavark_release_ready(
    attachment_id: &NetworkAttachmentId,
    allocation: &IpamAllocation,
    operation: &str,
) -> Result<()> {
    if matches!(
        &allocation.provider_operation,
        NetavarkProviderOperation::Reserved | NetavarkProviderOperation::Detached
    ) {
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

fn read_ipam_state(layout: &OciNetworkLayout) -> Result<IpamState> {
    LocalNetworkStateStore::open(&layout.state_root)
        .map_err(ipam_store_error)?
        .read(&NetworkStatePartition::TenantIpam(layout.tenant_id.clone()))
        .map_err(ipam_store_error)
        .map(Option::unwrap_or_default)
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

    #[test]
    fn stale_claim_cannot_load_or_delete_reallocated_same_attachment_ipam() {
        let (_dir, layout, mut config, sandbox) = fixture();
        config.network_subnet = "10.89.0.0/30".to_owned();
        let first_claim = test_reservation_claim("first-generation");
        let second_claim = test_reservation_claim("second-generation");
        config.reservation_claim = first_claim.clone();

        let first = allocate_container_ips_on_first_available(
            &layout,
            std::slice::from_ref(&config),
            &sandbox,
            &first_claim,
        )
        .expect("first generation should reserve IPAM");
        deallocate_container_ips_for_claim(&layout, &sandbox, &first_claim)
            .expect("first generation should compare-delete its own IPAM");
        let mut replacement_config = config.clone();
        replacement_config.reservation_claim = second_claim.clone();
        let second = allocate_container_ips_on_first_available(
            &layout,
            std::slice::from_ref(&replacement_config),
            &sandbox,
            &second_claim,
        )
        .expect("second generation should reserve replacement IPAM");

        let stale_load = load_container_ips_for_segment(&layout, &config, &sandbox)
            .expect_err("stale first-generation provider work must not load replacement IPAM");
        assert!(
            stale_load
                .to_string()
                .contains("different launch coordinator"),
            "the rejected provider observation must name its generation fence: {stale_load}"
        );
        let stale_error = deallocate_container_ips_for_claim(&layout, &sandbox, &first_claim)
            .expect_err("stale first-generation cleanup must not delete replacement IPAM");
        assert!(
            stale_error.to_string().contains("stale launch coordinator"),
            "the rejected ABA cleanup must name its generation fence: {stale_error}"
        );
        let stale_confirmed_detach =
            deallocate_container_ips_after_confirmed_detach(&layout, &sandbox, &first_claim)
                .expect_err("stale confirmed-detach cleanup must not delete replacement IPAM");
        assert!(
            stale_confirmed_detach
                .to_string()
                .contains("stale launch coordinator"),
            "confirmed-detach ABA rejection must name its generation fence: {stale_confirmed_detach}"
        );
        assert_eq!(
            load_container_ips_for_segment(&layout, &replacement_config, &sandbox)
                .expect("replacement IPAM should remain loadable"),
            second.ips,
            "stale cleanup must leave the replacement allocation byte-for-byte authoritative"
        );
        assert_eq!(
            first.ips, second.ips,
            "the ABA proof must reuse the same address so IP can never masquerade as generation identity"
        );

        deallocate_container_ips_after_confirmed_detach(&layout, &sandbox, &second_claim)
            .expect("replacement generation should publish exact terminal evidence");
        assert!(
            authenticate_container_network_generation_for_cleanup(
                &layout,
                &replacement_config,
                &sandbox,
            )
            .expect("replacement terminal generation should authenticate")
            .is_none(),
            "terminal evidence must not imply that provider effects remain live"
        );
        let stale_terminal =
            authenticate_container_network_generation_for_cleanup(&layout, &config, &sandbox)
                .expect_err("an old generation must not borrow a replacement's terminal tombstone");
        assert!(
            stale_terminal
                .to_string()
                .contains("different launch coordinator"),
            "terminal ABA rejection must name its generation fence: {stale_terminal}"
        );
        deallocate_container_ips_after_confirmed_detach(&layout, &sandbox, &first_claim)
            .expect_err("stale cleanup must not accept a replacement terminal tombstone");
        let authority_path = LocalNetworkStateStore::authority_path_for(&layout.state_root);
        let before_retirement = fs::read(&authority_path).expect("authority bytes should read");
        assert!(
            !retire_terminal_container_ipam_release(&layout, &sandbox, &first_claim)
                .expect("stale retirement should inspect"),
            "a stale generation must not retire replacement terminal evidence"
        );
        assert_eq!(
            fs::read(&authority_path).expect("authority bytes should reread"),
            before_retirement,
            "rejected retirement must leave replacement authority byte-for-byte unchanged"
        );
        assert!(
            retire_terminal_container_ipam_release(&layout, &sandbox, &second_claim)
                .expect("exact terminal retirement should succeed")
        );
    }

    #[test]
    fn newer_never_realized_claim_supersedes_older_terminal_generation() {
        let (_dir, layout, mut config, sandbox) = fixture();
        let first_claim = test_reservation_claim("completed-first-generation");
        let second_claim = test_reservation_claim("never-realized-second-generation");
        config.reservation_claim = first_claim.clone();
        allocate_container_ips_on_first_available(
            &layout,
            std::slice::from_ref(&config),
            &sandbox,
            &first_claim,
        )
        .expect("first generation should reserve IPAM");
        deallocate_container_ips_after_confirmed_detach(&layout, &sandbox, &first_claim)
            .expect("first generation should publish terminal evidence");

        deallocate_container_ips_for_claim(&layout, &sandbox, &second_claim)
            .expect("authenticated newer no-effect cleanup should supersede old terminal evidence");
        let state = read_ipam_state(&layout).expect("IPAM authority should inspect");
        assert!(state.allocations.is_empty());
        assert!(
            state.released_allocations.is_empty(),
            "the newer generation never committed IPAM and must not inherit old retry history"
        );
        deallocate_container_ips_for_claim(&layout, &sandbox, &second_claim)
            .expect("no-effect cleanup replay should be idempotent");
    }

    #[test]
    fn completed_unique_attachment_churn_does_not_accumulate_terminal_ipam() {
        let dir = tempdir().expect("temp dir");
        let tenant = TenantId::new("tenant-ipam-churn").expect("tenant should parse");
        for index in 0..256 {
            let sandbox = SandboxId::new(format!("sandbox-ipam-churn-{index}"));
            let layout = OciNetworkLayout::new(dir.path(), &tenant, &sandbox);
            let claim = test_reservation_claim(&format!("churn-{index}"));
            let config = OciNetworkConfig {
                reservation_claim: claim.clone(),
                ..OciNetworkConfig::default()
            };
            allocate_container_ips_on_first_available(
                &layout,
                std::slice::from_ref(&config),
                &sandbox,
                &claim,
            )
            .expect("churn generation should reserve IPAM");
            deallocate_container_ips_after_confirmed_detach(&layout, &sandbox, &claim)
                .expect("provider-confirmed detach should publish retry evidence");
            assert!(
                retire_terminal_container_ipam_release(&layout, &sandbox, &claim)
                    .expect("durably final lifecycle should retire exact evidence")
            );
            let state = read_ipam_state(&layout).expect("IPAM authority should inspect");
            assert!(state.allocations.is_empty());
            assert!(
                state.released_allocations.is_empty(),
                "completed attachment churn must leave no historical retry ledger"
            );
        }
    }

    #[test]
    fn startup_reconciliation_retires_only_terminal_manifest_ipam_evidence() {
        let (_dir, layout, mut config, sandbox) = fixture();
        let claim = test_reservation_claim("startup-terminal-reconciliation");
        config.reservation_claim = claim.clone();
        allocate_container_ips_on_first_available(
            &layout,
            std::slice::from_ref(&config),
            &sandbox,
            &claim,
        )
        .expect("generation should reserve IPAM");
        deallocate_container_ips_after_confirmed_detach(&layout, &sandbox, &claim)
            .expect("provider detach should publish terminal retry evidence");
        let manifest_path =
            crate::artifact_paths::manifest_path(&layout.state_root, &layout.tenant_id, &sandbox);
        fs::create_dir_all(manifest_path.parent().expect("manifest parent"))
            .expect("manifest parent should create");
        fs::write(
            &manifest_path,
            serde_json::to_vec_pretty(&serde_json::json!({
                "handle": {"id": &sandbox, "tenant_id": &layout.tenant_id},
                "spec": {"tenant_id": &layout.tenant_id},
                "network_layout": &layout,
                "network_config": &config,
                "network_cleanup_complete": true,
                "launch_artifact": null,
                "launch_reservation_claim": null,
                "status": "failed"
            }))
            .expect("manifest projection should render"),
        )
        .expect("terminal manifest should write");

        assert_eq!(
            reconcile_terminal_container_ipam_releases(&layout.state_root)
                .expect("startup reconciliation should succeed"),
            1
        );
        let state = read_ipam_state(&layout).expect("IPAM authority should inspect");
        assert!(state.released_allocations.is_empty());
        assert_eq!(
            reconcile_terminal_container_ipam_releases(&layout.state_root)
                .expect("startup reconciliation replay should succeed"),
            0
        );
    }

    #[test]
    fn startup_reconciliation_retains_ipam_until_explicit_network_cleanup_finality() {
        let (_dir, layout, mut config, sandbox) = fixture();
        let claim = test_reservation_claim("startup-incomplete-network-cleanup");
        config.reservation_claim = claim.clone();
        allocate_container_ips_on_first_available(
            &layout,
            std::slice::from_ref(&config),
            &sandbox,
            &claim,
        )
        .expect("generation should reserve IPAM");
        deallocate_container_ips_after_confirmed_detach(&layout, &sandbox, &claim)
            .expect("provider detach should publish terminal retry evidence");
        let manifest_path =
            crate::artifact_paths::manifest_path(&layout.state_root, &layout.tenant_id, &sandbox);
        fs::create_dir_all(manifest_path.parent().expect("manifest parent"))
            .expect("manifest parent should create");
        fs::write(
            &manifest_path,
            serde_json::to_vec_pretty(&serde_json::json!({
                "handle": {"id": &sandbox, "tenant_id": &layout.tenant_id},
                "spec": {"tenant_id": &layout.tenant_id},
                "network_layout": &layout,
                "network_config": &config,
                "network_cleanup_complete": false,
                "launch_artifact": null,
                "launch_reservation_claim": null,
                "status": "failed"
            }))
            .expect("manifest projection should render"),
        )
        .expect("terminal projection should write");

        assert_eq!(
            reconcile_terminal_container_ipam_releases(&layout.state_root)
                .expect("incomplete cleanup should be a successful no-op"),
            0
        );
        let state = read_ipam_state(&layout).expect("IPAM authority should inspect");
        assert_eq!(
            state.released_allocations.len(),
            1,
            "terminal observed status must not retire retry evidence without durable cleanup finality"
        );
    }

    #[test]
    fn startup_reconciliation_rejects_cross_root_manifest_without_mutation() {
        let trusted = tempdir().expect("trusted state root");
        let foreign = tempdir().expect("foreign state root");
        let tenant = TenantId::new("tenant-cross-root").expect("tenant should parse");
        let sandbox = SandboxId::new("sandbox-cross-root");
        let foreign_layout = OciNetworkLayout::new(foreign.path(), &tenant, &sandbox);
        let claim = test_reservation_claim("cross-root");
        let config = OciNetworkConfig {
            reservation_claim: claim.clone(),
            ..OciNetworkConfig::default()
        };
        allocate_container_ips_on_first_available(
            &foreign_layout,
            std::slice::from_ref(&config),
            &sandbox,
            &claim,
        )
        .expect("foreign generation should reserve IPAM");
        deallocate_container_ips_after_confirmed_detach(&foreign_layout, &sandbox, &claim)
            .expect("foreign detach should publish terminal evidence");
        let authority_path = LocalNetworkStateStore::authority_path_for(foreign.path());
        let before = fs::read(&authority_path).expect("foreign authority should read");

        let copied_manifest_path =
            crate::artifact_paths::manifest_path(trusted.path(), &tenant, &sandbox);
        fs::create_dir_all(
            copied_manifest_path
                .parent()
                .expect("copied manifest parent"),
        )
        .expect("copied manifest parent should create");
        fs::write(
            &copied_manifest_path,
            serde_json::to_vec_pretty(&serde_json::json!({
                "handle": {"id": &sandbox, "tenant_id": &tenant},
                "spec": {"tenant_id": &tenant},
                "network_layout": &foreign_layout,
                "network_config": &config,
                "network_cleanup_complete": true,
                "launch_artifact": null,
                "launch_reservation_claim": null,
                "status": "failed"
            }))
            .expect("copied manifest should render"),
        )
        .expect("copied manifest should write");

        let error = reconcile_terminal_container_ipam_releases(trusted.path())
            .expect_err("embedded foreign state root must fail closed");
        assert!(
            error.to_string().contains("untrusted network layout"),
            "the rejected authority redirection must be explicit: {error}"
        );
        assert_eq!(
            fs::read(&authority_path).expect("foreign authority should reread"),
            before,
            "a copied manifest must not mutate another state root's authority"
        );
        assert_eq!(
            read_ipam_state(&foreign_layout)
                .expect("foreign IPAM should inspect")
                .released_allocations
                .len(),
            1,
            "foreign terminal evidence must remain intact"
        );
    }

    #[test]
    fn existing_ipam_requires_the_exact_reservation_claim() {
        let (_dir, layout, config, sandbox) = fixture();
        let owner = test_reservation_claim("owner");
        let stale = test_reservation_claim("stale");
        allocate_container_ips_on_first_available(
            &layout,
            std::slice::from_ref(&config),
            &sandbox,
            &owner,
        )
        .expect("owner should reserve IPAM");
        let authority_path = LocalNetworkStateStore::authority_path_for(&layout.state_root);
        let before = fs::read(&authority_path).expect("authority bytes should read");

        let error = allocate_container_ips_on_first_available(
            &layout,
            std::slice::from_ref(&config),
            &sandbox,
            &stale,
        )
        .expect_err("a different coordinator must not adopt the existing allocation");
        assert!(
            error.to_string().contains("cross-generation adoption"),
            "claim mismatch must fail at the generation fence: {error}"
        );
        assert_eq!(
            fs::read(&authority_path).expect("authority bytes should reread"),
            before,
            "rejected cross-generation adoption must not rewrite authority state"
        );
    }

    #[test]
    fn ipam_load_is_byte_stable() {
        let (_dir, layout, mut config, sandbox) = fixture();
        let claim = test_reservation_claim("read-only-load");
        config.reservation_claim = claim.clone();
        let allocation = allocate_container_ips_on_first_available(
            &layout,
            std::slice::from_ref(&config),
            &sandbox,
            &claim,
        )
        .expect("fixture should reserve IPAM");
        let authority_path = LocalNetworkStateStore::authority_path_for(&layout.state_root);
        let before = fs::read(&authority_path).expect("authority bytes should read");

        assert_eq!(
            load_container_ips(&layout, &sandbox).expect("generic load should succeed"),
            allocation.ips
        );
        assert_eq!(
            load_container_ips_for_segment(&layout, &config, &sandbox)
                .expect("segment-fenced load should succeed"),
            allocation.ips
        );
        assert_eq!(
            fs::read(&authority_path).expect("authority bytes should reread"),
            before,
            "IPAM observation must not advance revision or rewrite durable state"
        );
    }

    #[test]
    fn ipam_allocation_requires_a_reservation_claim() {
        let error = serde_json::from_value::<IpamState>(serde_json::json!({
            "allocations": {
                "netattach_01ARZ3NDEKTSV4RRFFQ69G5FAV": {
                    "segment_id": "netsegment_01ARZ3NDEKTSV4RRFFQ69G5FAV",
                    "ips": ["10.89.0.2"]
                }
            },
            "last_assigned_ip": "10.89.0.2"
        }))
        .expect_err("claim-less durable IPAM must fail closed");
        assert!(
            error.to_string().contains("reservation_claim"),
            "schema rejection must name the missing generation fence: {error}"
        );
    }
}
