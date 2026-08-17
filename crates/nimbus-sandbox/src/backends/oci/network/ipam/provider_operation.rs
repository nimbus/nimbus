//! Durable Netavark provider-attempt state machine.
//!
//! IPAM remains the single sandbox provider-attempt authority. Prepared and
//! executing phases are distinct so a fresh process can adopt a no-effect
//! attempt while an ambiguous external effect can never be executed twice.

use std::net::Ipv4Addr;

use nimbus_core::TenantId;
use nimbus_network::{
    NetworkAttachmentId, NetworkProviderHandle, NetworkProviderId, NetworkReservationClaim,
    NetworkSegmentId,
};
use sha2::{Digest, Sha256};
use ulid::Ulid;

use super::{OciIpamAuthority, read_ipam_state, validate_ipam_generation, with_ipam_state};
use crate::backends::oci::network::dto::{IpamAllocation, IpamState, NetavarkProviderOperation};
use crate::backends::oci::network::layout::{OciNetworkConfig, OciNetworkLayout};
use crate::backends::oci::network::provider_locator::OciAttachmentProviderLocator;
use crate::error::{Result, SandboxError};
use crate::instance::SandboxId;

const NETAVARK_OPERATION_PROVIDER_KEY: &str = "nimbus-sandbox.oci.netavark-operation";
const NETAVARK_GENERATION_DOMAIN: &[u8] = b"nimbus.sandbox.oci.netavark-operation-generation.v1\0";

/// Exact durable generation to which one provider attempt is confined.
#[derive(Debug, Clone, PartialEq, Eq)]
struct NetavarkOperationGeneration {
    attachment_id: NetworkAttachmentId,
    reservation_claim: NetworkReservationClaim,
    segment_id: NetworkSegmentId,
    provider_locator: OciAttachmentProviderLocator,
}

/// Attempt-specific capability for completing one durable Netavark setup.
///
/// The capability is returned while the journal remains `SetupPrepared`.
/// Crossing the final pre-effect fence changes the journal to `Provisioning`,
/// so a concurrent or restarted caller cannot execute the provider twice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::backends::oci::network) struct NetavarkSetupClaim {
    generation: NetavarkOperationGeneration,
    operation_attempt: NetworkProviderHandle,
}

impl NetavarkSetupClaim {
    pub(in crate::backends::oci::network) fn operation_attempt(&self) -> &NetworkProviderHandle {
        &self.operation_attempt
    }

    #[cfg(test)]
    pub(in crate::backends::oci::network) fn with_operation_attempt_for_test(
        &self,
        operation_attempt: NetworkProviderHandle,
    ) -> Self {
        Self {
            generation: self.generation.clone(),
            operation_attempt,
        }
    }
}

/// Attempt-specific capability for completing one durable Netavark teardown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::backends::oci::network) struct NetavarkTeardownClaim {
    generation: NetavarkOperationGeneration,
    setup_attempt: NetworkProviderHandle,
    operation_attempt: NetworkProviderHandle,
}

impl NetavarkTeardownClaim {
    pub(in crate::backends::oci::network) fn attachment_id(&self) -> &NetworkAttachmentId {
        &self.generation.attachment_id
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
            generation: self.generation.clone(),
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
    let attachment_id = config.attachment_id.clone();
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
        authenticate_provider_locator(layout, config, sandbox_id, &attachment_id, allocation)?;
        validate_netavark_provider_operation_evidence(
            &layout.tenant_id,
            &attachment_id,
            allocation,
        )?;
        let generation =
            netavark_operation_generation(&layout.tenant_id, &attachment_id, allocation)?;
        let operation_attempt = match &allocation.provider_operation {
            NetavarkProviderOperation::Reserved | NetavarkProviderOperation::Detached => {
                let operation_attempt =
                    new_netavark_operation_attempt("setup", &layout.tenant_id, &generation)?;
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
                generation,
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
    let attachment_id = config.attachment_id.clone();
    validate_setup_claim_identity(claim, &attachment_id, &config.reservation_claim)?;
    with_ipam_state(authority, layout, |state| {
        let allocation = exact_live_allocation_for_setup_claim(state, layout, claim)?;
        let assigned_ips = validate_ipam_generation(config, &attachment_id, allocation)?;
        authenticate_provider_locator(layout, config, sandbox_id, &attachment_id, allocation)?;
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
                &claim.generation.attachment_id,
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
    let attachment_id = config.attachment_id.clone();
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
    authenticate_provider_locator(layout, config, sandbox_id, &attachment_id, allocation)?;
    validate_netavark_provider_operation_evidence(&layout.tenant_id, &attachment_id, allocation)?;
    Ok(allocation.provider_operation.clone())
}

pub(in crate::backends::oci::network) fn complete_netavark_setup(
    authority: &OciIpamAuthority,
    layout: &OciNetworkLayout,
    claim: &NetavarkSetupClaim,
) -> Result<()> {
    with_ipam_state(authority, layout, |state| {
        let allocation = exact_live_allocation_for_setup_claim(state, layout, claim)?;
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
                &claim.generation.attachment_id,
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
    let attachment_id = config.attachment_id.clone();
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
            authenticate_provider_locator(layout, config, sandbox_id, &attachment_id, released)?;
            validate_netavark_provider_operation_evidence(
                &layout.tenant_id,
                &attachment_id,
                released,
            )?;
            if !released.provider_operation.permits_terminal_ipam_release() {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "terminal OCI IPAM generation for attachment {} carries provider phase {}; refusing to treat a possibly live provider effect as detached",
                        attachment_id.as_str(),
                        released.provider_operation.label()
                    ),
                });
            }
            return Ok(NetavarkTeardownPlan::AlreadyDetached);
        };
        let assigned_ips = validate_ipam_generation(config, &attachment_id, allocation)?;
        authenticate_provider_locator(layout, config, sandbox_id, &attachment_id, allocation)?;
        validate_netavark_provider_operation_evidence(
            &layout.tenant_id,
            &attachment_id,
            allocation,
        )?;
        let generation =
            netavark_operation_generation(&layout.tenant_id, &attachment_id, allocation)?;
        if let Some(setup_claim) = setup_claim
            && setup_claim.generation != generation
        {
            return Err(netavark_generation_mismatch(
                &attachment_id,
                "setup compensation",
            ));
        }
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
                    &layout.tenant_id,
                    &generation,
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
                    &layout.tenant_id,
                    &generation,
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
                    &layout.tenant_id,
                    &generation,
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
                        &generation,
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
                        &generation,
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
                        &generation,
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
                        &generation,
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
    tenant_id: &TenantId,
    generation: &NetavarkOperationGeneration,
    setup_attempt: NetworkProviderHandle,
) -> Result<NetavarkTeardownPlan> {
    let operation_attempt = new_netavark_operation_attempt("teardown", tenant_id, generation)?;
    allocation.provider_operation = NetavarkProviderOperation::TeardownPrepared {
        setup_attempt: setup_attempt.clone(),
        operation_attempt: operation_attempt.clone(),
    };
    Ok(NetavarkTeardownPlan::Run {
        assigned_ips,
        claim: teardown_claim(generation, setup_attempt, operation_attempt),
    })
}

fn prepare_no_effect_teardown(
    allocation: &mut IpamAllocation,
    tenant_id: &TenantId,
    generation: &NetavarkOperationGeneration,
    setup_attempt: NetworkProviderHandle,
) -> Result<NetavarkTeardownPlan> {
    let operation_attempt = new_netavark_operation_attempt("teardown", tenant_id, generation)?;
    allocation.provider_operation = NetavarkProviderOperation::NoEffectTeardownPrepared {
        setup_attempt: setup_attempt.clone(),
        operation_attempt: operation_attempt.clone(),
    };
    Ok(NetavarkTeardownPlan::ConfirmNoEffect {
        claim: teardown_claim(generation, setup_attempt, operation_attempt),
    })
}

fn teardown_claim(
    generation: &NetavarkOperationGeneration,
    setup_attempt: NetworkProviderHandle,
    operation_attempt: NetworkProviderHandle,
) -> NetavarkTeardownClaim {
    NetavarkTeardownClaim {
        generation: generation.clone(),
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
        let allocation = exact_live_allocation_for_teardown_claim(state, layout, claim)?;
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
                &claim.generation.attachment_id,
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
        let allocation = exact_live_allocation_for_teardown_claim(state, layout, claim)?;
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
                &claim.generation.attachment_id,
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
        let allocation = exact_live_allocation_for_teardown_claim(state, layout, claim)?;
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
                &claim.generation.attachment_id,
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
        let allocation = exact_live_allocation_for_teardown_claim(state, layout, claim)?;
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
                &claim.generation.attachment_id,
                "teardown completion",
                current,
            )),
        }
    })
}

fn exact_live_allocation_for_setup_claim<'a>(
    state: &'a mut IpamState,
    layout: &OciNetworkLayout,
    claim: &NetavarkSetupClaim,
) -> Result<&'a mut IpamAllocation> {
    exact_live_allocation_for_operation(state, layout, &claim.generation, "setup")
}

fn exact_live_allocation_for_teardown_claim<'a>(
    state: &'a mut IpamState,
    layout: &OciNetworkLayout,
    claim: &NetavarkTeardownClaim,
) -> Result<&'a mut IpamAllocation> {
    exact_live_allocation_for_operation(state, layout, &claim.generation, "teardown")
}

fn exact_live_allocation_for_operation<'a>(
    state: &'a mut IpamState,
    layout: &OciNetworkLayout,
    generation: &NetavarkOperationGeneration,
    operation: &str,
) -> Result<&'a mut IpamAllocation> {
    authenticate_claim_layout(layout, generation, operation)?;
    let attachment_id = &generation.attachment_id;
    let allocation = state
        .allocations
        .get_mut(attachment_id.as_str())
        .ok_or_else(|| SandboxError::OperationFailed {
            message: format!(
                "OCI Netavark {operation} claim for attachment {} has no live IPAM generation",
                attachment_id.as_str()
            ),
        })?;
    let durable_generation =
        netavark_operation_generation(&layout.tenant_id, attachment_id, allocation)?;
    if durable_generation != *generation {
        return Err(netavark_generation_mismatch(attachment_id, operation));
    }
    validate_netavark_provider_operation_evidence(&layout.tenant_id, attachment_id, allocation)?;
    Ok(allocation)
}

fn validate_setup_claim_identity(
    claim: &NetavarkSetupClaim,
    attachment_id: &NetworkAttachmentId,
    reservation_claim: &NetworkReservationClaim,
) -> Result<()> {
    if &claim.generation.attachment_id == attachment_id
        && &claim.generation.reservation_claim == reservation_claim
    {
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
    tenant_id: &TenantId,
    generation: &NetavarkOperationGeneration,
) -> Result<NetworkProviderHandle> {
    let generation_digest = netavark_generation_digest(tenant_id, generation);
    NetworkProviderHandle::new(
        NetworkProviderId::for_registration_key(NETAVARK_OPERATION_PROVIDER_KEY),
        format!(
            "v1:{action}:{}:{}:{generation_digest}:{}",
            tenant_id.as_str(),
            generation.attachment_id.as_str(),
            Ulid::new()
        ),
    )
    .map_err(|error| SandboxError::OperationFailed {
        message: format!(
            "failed to mint Netavark {action} operation capability for attachment {}: {error}",
            generation.attachment_id.as_str()
        ),
    })
}

pub(in crate::backends::oci::network) fn validate_netavark_provider_operation_evidence(
    tenant_id: &TenantId,
    attachment_id: &NetworkAttachmentId,
    allocation: &IpamAllocation,
) -> Result<()> {
    let generation = netavark_operation_generation(tenant_id, attachment_id, allocation)?;
    match &allocation.provider_operation {
        NetavarkProviderOperation::Reserved | NetavarkProviderOperation::Detached => Ok(()),
        NetavarkProviderOperation::SetupPrepared { operation_attempt }
        | NetavarkProviderOperation::Provisioning { operation_attempt } => {
            validate_netavark_operation_attempt(operation_attempt, "setup", tenant_id, &generation)
        }
        NetavarkProviderOperation::Ready { setup_attempt } => {
            validate_netavark_operation_attempt(setup_attempt, "setup", tenant_id, &generation)
        }
        NetavarkProviderOperation::TeardownPrepared {
            setup_attempt,
            operation_attempt,
        }
        | NetavarkProviderOperation::NoEffectTeardownPrepared {
            setup_attempt,
            operation_attempt,
        }
        | NetavarkProviderOperation::Deleting {
            setup_attempt,
            operation_attempt,
        }
        | NetavarkProviderOperation::DetachedProjectionPending {
            setup_attempt,
            operation_attempt,
        } => {
            validate_netavark_operation_attempt(setup_attempt, "setup", tenant_id, &generation)?;
            validate_netavark_operation_attempt(
                operation_attempt,
                "teardown",
                tenant_id,
                &generation,
            )
        }
    }
}

fn validate_netavark_operation_attempt(
    attempt: &NetworkProviderHandle,
    expected_action: &str,
    tenant_id: &TenantId,
    generation: &NetavarkOperationGeneration,
) -> Result<()> {
    let attachment_id = &generation.attachment_id;
    let expected_provider =
        NetworkProviderId::for_registration_key(NETAVARK_OPERATION_PROVIDER_KEY);
    if attempt.provider_id() != &expected_provider {
        return Err(invalid_netavark_operation_attempt(
            expected_action,
            tenant_id,
            attachment_id,
            "foreign provider identity",
        ));
    }
    let mut parts = attempt.expose_to_provider().split(':');
    let version = parts.next();
    let action = parts.next();
    let tenant = parts.next();
    let attachment = parts.next();
    let generation_digest = parts.next();
    let attempt_id = parts.next();
    let expected_generation_digest = netavark_generation_digest(tenant_id, generation);
    if parts.next().is_some()
        || version != Some("v1")
        || action != Some(expected_action)
        || tenant != Some(tenant_id.as_str())
        || attachment != Some(attachment_id.as_str())
        || generation_digest != Some(expected_generation_digest.as_str())
        || attempt_id
            .and_then(|value| Ulid::from_string(value).ok())
            .is_none()
    {
        return Err(invalid_netavark_operation_attempt(
            expected_action,
            tenant_id,
            attachment_id,
            "version, action, tenant, attachment, generation binding, or attempt identity mismatch",
        ));
    }
    Ok(())
}

fn netavark_operation_generation(
    tenant_id: &TenantId,
    attachment_id: &NetworkAttachmentId,
    allocation: &IpamAllocation,
) -> Result<NetavarkOperationGeneration> {
    allocation.provider_locator.validate()?;
    if allocation.provider_locator.tenant_id() != tenant_id {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "OCI Netavark generation binding for attachment {} carries provider locator tenant {} instead of {}",
                attachment_id.as_str(),
                allocation.provider_locator.tenant_id().as_str(),
                tenant_id.as_str()
            ),
        });
    }
    let segment_id =
        allocation
            .segment_id
            .parse::<NetworkSegmentId>()
            .map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "OCI Netavark generation binding for attachment {} contains invalid segment identity {:?}: {error}",
                    attachment_id.as_str(),
                    allocation.segment_id
                ),
            })?;
    Ok(NetavarkOperationGeneration {
        attachment_id: attachment_id.clone(),
        reservation_claim: allocation.reservation_claim.clone(),
        segment_id,
        provider_locator: allocation.provider_locator.clone(),
    })
}

fn netavark_generation_digest(
    tenant_id: &TenantId,
    generation: &NetavarkOperationGeneration,
) -> String {
    let mut digest = Sha256::new();
    digest.update(NETAVARK_GENERATION_DOMAIN);
    for component in [
        tenant_id.as_str(),
        generation.attachment_id.as_str(),
        generation
            .reservation_claim
            .coordinator_attempt()
            .provider_id()
            .as_str(),
        generation
            .reservation_claim
            .coordinator_attempt()
            .expose_to_provider(),
        generation.segment_id.as_str(),
        generation.provider_locator.tenant_id().as_str(),
        generation.provider_locator.sandbox_id().as_str(),
        generation.provider_locator.provider_kind().as_str(),
        generation.provider_locator.artifact_realm_id().as_str(),
    ] {
        digest.update(
            u64::try_from(component.len())
                .expect("a Rust string length always fits u64 on supported targets")
                .to_be_bytes(),
        );
        digest.update(component.as_bytes());
    }
    digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn authenticate_provider_locator(
    layout: &OciNetworkLayout,
    config: &OciNetworkConfig,
    sandbox_id: &SandboxId,
    attachment_id: &NetworkAttachmentId,
    allocation: &IpamAllocation,
) -> Result<()> {
    let expected = OciAttachmentProviderLocator::new(
        &layout.workload_state_root,
        &layout.tenant_id,
        sandbox_id,
        config.provider_kind,
    )?;
    if allocation.provider_locator == expected {
        return Ok(());
    }
    Err(SandboxError::OperationFailed {
        message: format!(
            "OCI Netavark provider locator for attachment {} does not authenticate the supplied tenant, sandbox, artifact realm, and provider kind",
            attachment_id.as_str()
        ),
    })
}

fn authenticate_claim_layout(
    layout: &OciNetworkLayout,
    generation: &NetavarkOperationGeneration,
    operation: &str,
) -> Result<()> {
    let locator = &generation.provider_locator;
    let authenticates_root = locator.authenticates_workload_root(&layout.workload_state_root)?;
    if locator.tenant_id() == &layout.tenant_id && authenticates_root {
        return Ok(());
    }
    Err(SandboxError::OperationFailed {
        message: format!(
            "OCI Netavark {operation} provider locator for attachment {} does not authenticate the supplied tenant and artifact realm",
            generation.attachment_id.as_str()
        ),
    })
}

fn netavark_generation_mismatch(
    attachment_id: &NetworkAttachmentId,
    operation: &str,
) -> SandboxError {
    SandboxError::OperationFailed {
        message: format!(
            "OCI Netavark {operation} for attachment {} carries a foreign generation binding",
            attachment_id.as_str()
        ),
    }
}

fn invalid_netavark_operation_attempt(
    action: &str,
    tenant_id: &TenantId,
    attachment_id: &NetworkAttachmentId,
    reason: &str,
) -> SandboxError {
    SandboxError::OperationFailed {
        message: format!(
            "OCI Netavark {action} attempt for tenant {} attachment {} has {reason}",
            tenant_id.as_str(),
            attachment_id.as_str()
        ),
    }
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
