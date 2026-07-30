//! Durable Netavark provider-attempt state machine.
//!
//! IPAM remains the single sandbox provider-attempt authority. Prepared and
//! executing phases are distinct so a fresh process can adopt a no-effect
//! attempt while an ambiguous external effect can never be executed twice.

use std::net::Ipv4Addr;

use nimbus_network::{
    NetworkAttachmentId, NetworkProviderHandle, NetworkProviderId, NetworkReservationClaim,
};
use ulid::Ulid;

use super::{OciIpamAuthority, read_ipam_state, validate_ipam_generation, with_ipam_state};
use crate::backends::oci::network::default_network_attachment_id;
use crate::backends::oci::network::dto::{IpamAllocation, IpamState, NetavarkProviderOperation};
use crate::backends::oci::network::layout::{OciNetworkConfig, OciNetworkLayout};
use crate::error::{Result, SandboxError};
use crate::instance::SandboxId;

/// Attempt-specific capability for completing one durable Netavark setup.
///
/// The capability is returned while the journal remains `SetupPrepared`.
/// Crossing the final pre-effect fence changes the journal to `Provisioning`,
/// so a concurrent or restarted caller cannot execute the provider twice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::backends::oci::network) struct NetavarkSetupClaim {
    attachment_id: NetworkAttachmentId,
    reservation_claim: NetworkReservationClaim,
    operation_attempt: NetworkProviderHandle,
}

impl NetavarkSetupClaim {
    #[cfg(test)]
    pub(in crate::backends::oci::network) fn operation_attempt(&self) -> &NetworkProviderHandle {
        &self.operation_attempt
    }

    #[cfg(test)]
    pub(in crate::backends::oci::network) fn with_operation_attempt_for_test(
        &self,
        operation_attempt: NetworkProviderHandle,
    ) -> Self {
        Self {
            attachment_id: self.attachment_id.clone(),
            reservation_claim: self.reservation_claim.clone(),
            operation_attempt,
        }
    }
}

/// Attempt-specific capability for completing one durable Netavark teardown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::backends::oci::network) struct NetavarkTeardownClaim {
    attachment_id: NetworkAttachmentId,
    reservation_claim: NetworkReservationClaim,
    setup_attempt: NetworkProviderHandle,
    operation_attempt: NetworkProviderHandle,
}

impl NetavarkTeardownClaim {
    pub(in crate::backends::oci::network) fn attachment_id(&self) -> &NetworkAttachmentId {
        &self.attachment_id
    }

    #[cfg(test)]
    pub(in crate::backends::oci::network) fn setup_attempt(&self) -> &NetworkProviderHandle {
        &self.setup_attempt
    }

    #[cfg(test)]
    pub(in crate::backends::oci::network) fn operation_attempt(&self) -> &NetworkProviderHandle {
        &self.operation_attempt
    }

    #[cfg(test)]
    pub(in crate::backends::oci::network) fn with_operation_attempt_for_test(
        &self,
        operation_attempt: NetworkProviderHandle,
    ) -> Self {
        Self {
            attachment_id: self.attachment_id.clone(),
            reservation_claim: self.reservation_claim.clone(),
            setup_attempt: self.setup_attempt.clone(),
            operation_attempt,
        }
    }
}

/// Provider work selected atomically from current IPAM authority.
pub(in crate::backends::oci::network) enum NetavarkTeardownPlan {
    /// Run Netavark teardown, then publish provider absence.
    Run {
        assigned_ips: Vec<Ipv4Addr>,
        claim: NetavarkTeardownClaim,
    },
    /// Complete cleanup without calling Netavark because setup never crossed
    /// its pre-effect fence.
    ConfirmNoEffect { claim: NetavarkTeardownClaim },
    /// A delete crossed its pre-effect fence before the previous owner died.
    ///
    /// Inspection may confirm absence and complete it, but must never rerun
    /// the provider delete while its effect remains possible.
    InspectDeleting { claim: NetavarkTeardownClaim },
    /// Provider absence is already durable; only the observed projection remains.
    RemoveProjection { claim: NetavarkTeardownClaim },
    /// Exact live or terminal authority already proves no provider work remains.
    AlreadyDetached,
}

pub(in crate::backends::oci::network) fn begin_netavark_setup(
    authority: &OciIpamAuthority,
    layout: &OciNetworkLayout,
    config: &OciNetworkConfig,
    sandbox_id: &SandboxId,
) -> Result<(Vec<Ipv4Addr>, NetavarkSetupClaim)> {
    let attachment_id = default_network_attachment_id(sandbox_id);
    with_ipam_state(authority, layout, |state| {
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
        let operation_attempt = match &allocation.provider_operation {
            NetavarkProviderOperation::Reserved | NetavarkProviderOperation::Detached => {
                let operation_attempt = new_netavark_operation_attempt("setup", &attachment_id)?;
                allocation.provider_operation = NetavarkProviderOperation::SetupPrepared {
                    operation_attempt: operation_attempt.clone(),
                };
                operation_attempt
            }
            NetavarkProviderOperation::SetupPrepared { operation_attempt } => {
                operation_attempt.clone()
            }
            current => {
                return Err(netavark_operation_pending(&attachment_id, "setup", current));
            }
        };
        Ok((
            assigned_ips,
            NetavarkSetupClaim {
                attachment_id,
                reservation_claim: config.reservation_claim.clone(),
                operation_attempt,
            },
        ))
    })
}

/// Atomically authenticate and fence one prepared setup immediately before
/// the external provider effect.
pub(in crate::backends::oci::network) fn begin_netavark_setup_execution(
    authority: &OciIpamAuthority,
    layout: &OciNetworkLayout,
    config: &OciNetworkConfig,
    sandbox_id: &SandboxId,
    claim: &NetavarkSetupClaim,
) -> Result<Vec<Ipv4Addr>> {
    let attachment_id = default_network_attachment_id(sandbox_id);
    validate_setup_claim_identity(claim, &attachment_id, &config.reservation_claim)?;
    with_ipam_state(authority, layout, |state| {
        let allocation = exact_live_allocation_for_setup_claim(state, claim)?;
        let assigned_ips = validate_ipam_generation(config, &attachment_id, allocation)?;
        match &allocation.provider_operation {
            NetavarkProviderOperation::SetupPrepared { operation_attempt }
                if operation_attempt == &claim.operation_attempt =>
            {
                allocation.provider_operation = NetavarkProviderOperation::Provisioning {
                    operation_attempt: claim.operation_attempt.clone(),
                };
                Ok(assigned_ips)
            }
            current => Err(netavark_claim_mismatch(
                &claim.attachment_id,
                "setup execution",
                current,
            )),
        }
    })
}

#[cfg(test)]
pub(crate) fn begin_netavark_setup_without_ack_for_test(
    authority: &OciIpamAuthority,
    layout: &OciNetworkLayout,
    config: &OciNetworkConfig,
    sandbox_id: &SandboxId,
) -> Result<()> {
    begin_netavark_setup(authority, layout, config, sandbox_id).map(|_| ())
}

pub(in crate::backends::oci::network) fn inspect_netavark_provider_operation(
    authority: &OciIpamAuthority,
    layout: &OciNetworkLayout,
    config: &OciNetworkConfig,
    sandbox_id: &SandboxId,
) -> Result<NetavarkProviderOperation> {
    let attachment_id = default_network_attachment_id(sandbox_id);
    let state = read_ipam_state(authority, layout)?;
    let allocation = state
        .allocations
        .get(attachment_id.as_str())
        .ok_or_else(|| SandboxError::OperationFailed {
            message: format!(
                "failed to inspect live Netavark operation for attachment {}",
                attachment_id.as_str()
            ),
        })?;
    validate_ipam_generation(config, &attachment_id, allocation)?;
    Ok(allocation.provider_operation.clone())
}

pub(in crate::backends::oci::network) fn complete_netavark_setup(
    authority: &OciIpamAuthority,
    layout: &OciNetworkLayout,
    claim: &NetavarkSetupClaim,
) -> Result<()> {
    with_ipam_state(authority, layout, |state| {
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

pub(in crate::backends::oci::network) fn begin_netavark_teardown(
    authority: &OciIpamAuthority,
    layout: &OciNetworkLayout,
    config: &OciNetworkConfig,
    sandbox_id: &SandboxId,
    setup_claim: Option<&NetavarkSetupClaim>,
) -> Result<NetavarkTeardownPlan> {
    let attachment_id = default_network_attachment_id(sandbox_id);
    if let Some(claim) = setup_claim {
        validate_setup_claim_identity(claim, &attachment_id, &config.reservation_claim)?;
    }
    with_ipam_state(authority, layout, |state| {
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
            NetavarkProviderOperation::Ready { setup_attempt } => {
                if let Some(setup_claim) = setup_claim
                    && setup_attempt != &setup_claim.operation_attempt
                {
                    return Err(netavark_claim_mismatch(
                        &attachment_id,
                        "setup compensation",
                        &allocation.provider_operation,
                    ));
                }
                prepare_teardown(
                    allocation,
                    assigned_ips,
                    &attachment_id,
                    &config.reservation_claim,
                    setup_attempt.clone(),
                )
            }
            NetavarkProviderOperation::SetupPrepared { operation_attempt } => {
                if let Some(setup_claim) = setup_claim
                    && operation_attempt != &setup_claim.operation_attempt
                {
                    return Err(netavark_claim_mismatch(
                        &attachment_id,
                        "setup compensation",
                        &allocation.provider_operation,
                    ));
                }
                prepare_no_effect_teardown(
                    allocation,
                    &attachment_id,
                    &config.reservation_claim,
                    operation_attempt.clone(),
                )
            }
            NetavarkProviderOperation::Provisioning { operation_attempt } => {
                if let Some(setup_claim) = setup_claim
                    && operation_attempt != &setup_claim.operation_attempt
                {
                    return Err(netavark_claim_mismatch(
                        &attachment_id,
                        "setup compensation",
                        &allocation.provider_operation,
                    ));
                }
                prepare_teardown(
                    allocation,
                    assigned_ips,
                    &attachment_id,
                    &config.reservation_claim,
                    operation_attempt.clone(),
                )
            }
            NetavarkProviderOperation::TeardownPrepared {
                setup_attempt,
                operation_attempt,
            } => {
                if let Some(setup_claim) = setup_claim
                    && setup_attempt != &setup_claim.operation_attempt
                {
                    return Err(netavark_claim_mismatch(
                        &attachment_id,
                        "setup compensation",
                        &allocation.provider_operation,
                    ));
                }
                Ok(NetavarkTeardownPlan::Run {
                    assigned_ips,
                    claim: teardown_claim(
                        &attachment_id,
                        &config.reservation_claim,
                        setup_attempt.clone(),
                        operation_attempt.clone(),
                    ),
                })
            }
            NetavarkProviderOperation::NoEffectTeardownPrepared {
                setup_attempt,
                operation_attempt,
            } => {
                if let Some(setup_claim) = setup_claim
                    && setup_attempt != &setup_claim.operation_attempt
                {
                    return Err(netavark_claim_mismatch(
                        &attachment_id,
                        "setup compensation",
                        &allocation.provider_operation,
                    ));
                }
                Ok(NetavarkTeardownPlan::ConfirmNoEffect {
                    claim: teardown_claim(
                        &attachment_id,
                        &config.reservation_claim,
                        setup_attempt.clone(),
                        operation_attempt.clone(),
                    ),
                })
            }
            NetavarkProviderOperation::Deleting {
                setup_attempt,
                operation_attempt,
            } => {
                if let Some(setup_claim) = setup_claim
                    && setup_attempt != &setup_claim.operation_attempt
                {
                    return Err(netavark_claim_mismatch(
                        &attachment_id,
                        "setup compensation",
                        &allocation.provider_operation,
                    ));
                }
                Ok(NetavarkTeardownPlan::InspectDeleting {
                    claim: teardown_claim(
                        &attachment_id,
                        &config.reservation_claim,
                        setup_attempt.clone(),
                        operation_attempt.clone(),
                    ),
                })
            }
            NetavarkProviderOperation::DetachedProjectionPending {
                setup_attempt,
                operation_attempt,
            } => {
                if let Some(setup_claim) = setup_claim
                    && setup_attempt != &setup_claim.operation_attempt
                {
                    return Err(netavark_claim_mismatch(
                        &attachment_id,
                        "setup compensation",
                        &allocation.provider_operation,
                    ));
                }
                Ok(NetavarkTeardownPlan::RemoveProjection {
                    claim: teardown_claim(
                        &attachment_id,
                        &config.reservation_claim,
                        setup_attempt.clone(),
                        operation_attempt.clone(),
                    ),
                })
            }
            NetavarkProviderOperation::Detached => Ok(NetavarkTeardownPlan::AlreadyDetached),
        }
    })
}

fn prepare_teardown(
    allocation: &mut IpamAllocation,
    assigned_ips: Vec<Ipv4Addr>,
    attachment_id: &NetworkAttachmentId,
    reservation_claim: &NetworkReservationClaim,
    setup_attempt: NetworkProviderHandle,
) -> Result<NetavarkTeardownPlan> {
    let operation_attempt = new_netavark_operation_attempt("teardown", attachment_id)?;
    allocation.provider_operation = NetavarkProviderOperation::TeardownPrepared {
        setup_attempt: setup_attempt.clone(),
        operation_attempt: operation_attempt.clone(),
    };
    Ok(NetavarkTeardownPlan::Run {
        assigned_ips,
        claim: teardown_claim(
            attachment_id,
            reservation_claim,
            setup_attempt,
            operation_attempt,
        ),
    })
}

fn prepare_no_effect_teardown(
    allocation: &mut IpamAllocation,
    attachment_id: &NetworkAttachmentId,
    reservation_claim: &NetworkReservationClaim,
    setup_attempt: NetworkProviderHandle,
) -> Result<NetavarkTeardownPlan> {
    let operation_attempt = new_netavark_operation_attempt("teardown", attachment_id)?;
    allocation.provider_operation = NetavarkProviderOperation::NoEffectTeardownPrepared {
        setup_attempt: setup_attempt.clone(),
        operation_attempt: operation_attempt.clone(),
    };
    Ok(NetavarkTeardownPlan::ConfirmNoEffect {
        claim: teardown_claim(
            attachment_id,
            reservation_claim,
            setup_attempt,
            operation_attempt,
        ),
    })
}

fn teardown_claim(
    attachment_id: &NetworkAttachmentId,
    reservation_claim: &NetworkReservationClaim,
    setup_attempt: NetworkProviderHandle,
    operation_attempt: NetworkProviderHandle,
) -> NetavarkTeardownClaim {
    NetavarkTeardownClaim {
        attachment_id: attachment_id.clone(),
        reservation_claim: reservation_claim.clone(),
        setup_attempt,
        operation_attempt,
    }
}

/// Atomically authenticate and fence one prepared teardown immediately before
/// the external provider effect.
pub(in crate::backends::oci::network) fn begin_netavark_teardown_execution(
    authority: &OciIpamAuthority,
    layout: &OciNetworkLayout,
    claim: &NetavarkTeardownClaim,
) -> Result<()> {
    with_ipam_state(authority, layout, |state| {
        let allocation = exact_live_allocation_for_teardown_claim(state, claim)?;
        match &allocation.provider_operation {
            NetavarkProviderOperation::TeardownPrepared {
                setup_attempt,
                operation_attempt,
            } if setup_attempt == &claim.setup_attempt
                && operation_attempt == &claim.operation_attempt =>
            {
                allocation.provider_operation = NetavarkProviderOperation::Deleting {
                    setup_attempt: claim.setup_attempt.clone(),
                    operation_attempt: claim.operation_attempt.clone(),
                };
                Ok(())
            }
            current => Err(netavark_claim_mismatch(
                &claim.attachment_id,
                "teardown execution",
                current,
            )),
        }
    })
}

pub(in crate::backends::oci::network) fn confirm_netavark_absent_without_effect(
    authority: &OciIpamAuthority,
    layout: &OciNetworkLayout,
    claim: &NetavarkTeardownClaim,
) -> Result<()> {
    with_ipam_state(authority, layout, |state| {
        let allocation = exact_live_allocation_for_teardown_claim(state, claim)?;
        match &allocation.provider_operation {
            NetavarkProviderOperation::TeardownPrepared {
                setup_attempt,
                operation_attempt,
            }
            | NetavarkProviderOperation::NoEffectTeardownPrepared {
                setup_attempt,
                operation_attempt,
            } if setup_attempt == &claim.setup_attempt
                && operation_attempt == &claim.operation_attempt =>
            {
                allocation.provider_operation =
                    NetavarkProviderOperation::DetachedProjectionPending {
                        setup_attempt: claim.setup_attempt.clone(),
                        operation_attempt: claim.operation_attempt.clone(),
                    };
                Ok(())
            }
            current => Err(netavark_claim_mismatch(
                &claim.attachment_id,
                "no-effect teardown confirmation",
                current,
            )),
        }
    })
}

pub(in crate::backends::oci::network) fn confirm_netavark_provider_detached(
    authority: &OciIpamAuthority,
    layout: &OciNetworkLayout,
    claim: &NetavarkTeardownClaim,
) -> Result<()> {
    with_ipam_state(authority, layout, |state| {
        let allocation = exact_live_allocation_for_teardown_claim(state, claim)?;
        match &allocation.provider_operation {
            NetavarkProviderOperation::Deleting {
                setup_attempt,
                operation_attempt,
            } if setup_attempt == &claim.setup_attempt
                && operation_attempt == &claim.operation_attempt =>
            {
                allocation.provider_operation =
                    NetavarkProviderOperation::DetachedProjectionPending {
                        setup_attempt: claim.setup_attempt.clone(),
                        operation_attempt: claim.operation_attempt.clone(),
                    };
                Ok(())
            }
            NetavarkProviderOperation::DetachedProjectionPending {
                setup_attempt,
                operation_attempt,
            } if setup_attempt == &claim.setup_attempt
                && operation_attempt == &claim.operation_attempt =>
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

pub(in crate::backends::oci::network) fn complete_netavark_teardown(
    authority: &OciIpamAuthority,
    layout: &OciNetworkLayout,
    claim: &NetavarkTeardownClaim,
) -> Result<()> {
    with_ipam_state(authority, layout, |state| {
        let allocation = exact_live_allocation_for_teardown_claim(state, claim)?;
        match &allocation.provider_operation {
            NetavarkProviderOperation::DetachedProjectionPending {
                setup_attempt,
                operation_attempt,
            } if setup_attempt == &claim.setup_attempt
                && operation_attempt == &claim.operation_attempt =>
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
    NetworkProviderHandle::new(
        NetworkProviderId::for_registration_key("nimbus-sandbox.oci.netavark-operation"),
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
                NetavarkProviderOperation::SetupPrepared { .. }
                    | NetavarkProviderOperation::Provisioning { .. }
                    | NetavarkProviderOperation::TeardownPrepared { .. }
                    | NetavarkProviderOperation::NoEffectTeardownPrepared { .. }
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
