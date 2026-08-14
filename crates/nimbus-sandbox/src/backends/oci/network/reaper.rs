//! Tenant-bridge reaper and identity-fenced allocation finalization.
//!
//! netavark creates the per-tenant bridge on first-sandbox setup but does NOT
//! remove it on last-sandbox teardown, so the crash-safe reaper removes the
//! bridge after the last attachment hold releases into durable
//! cleanup-pending state, then identity-fenced finalization frees the
//! allocation. Obsolete shared-bridge migration is deliberately absent: this
//! pre-launch tree supports only the per-tenant routed model.

use nimbus_core::TenantId;
use nimbus_network::{
    NetworkAttachmentId, NetworkReservationClaim, NetworkSegmentFinalizeOutcome,
    NetworkSegmentQuarantineOutcome, NetworkSegmentReleaseOutcome,
};

use crate::error::{Result, SandboxError};
use crate::instance::SandboxId;

use super::{
    OciNetworkLayout, OciSegmentAllocator, OciSegmentRealization, ipam::OciIpamAuthority,
    provider_locator::OciAttachmentProviderKind,
};

/// Exact retained authorities and identities for one never-realized launch.
#[derive(Clone, Copy)]
pub(crate) struct ReservedNetworkLaunchIdentity<'a> {
    layout: &'a OciNetworkLayout,
    tenant_id: &'a TenantId,
    sandbox_id: &'a SandboxId,
    attachment_id: &'a NetworkAttachmentId,
    reservation_claim: &'a NetworkReservationClaim,
}

impl<'a> ReservedNetworkLaunchIdentity<'a> {
    pub(crate) fn new(
        layout: &'a OciNetworkLayout,
        tenant_id: &'a TenantId,
        sandbox_id: &'a SandboxId,
        attachment_id: &'a NetworkAttachmentId,
        reservation_claim: &'a NetworkReservationClaim,
    ) -> Self {
        Self {
            layout,
            tenant_id,
            sandbox_id,
            attachment_id,
            reservation_claim,
        }
    }

    pub(crate) fn reservation_claim(self) -> &'a NetworkReservationClaim {
        self.reservation_claim
    }
}

/// Portable authority plus the exact launch identity needed for compensation.
#[derive(Clone, Copy)]
pub(crate) struct ReservedNetworkLaunchAuthority<'a> {
    allocator: &'a OciSegmentAllocator,
    ipam_authority: &'a OciIpamAuthority,
    layout: &'a OciNetworkLayout,
    tenant_id: &'a TenantId,
    sandbox_id: &'a SandboxId,
    attachment_id: &'a NetworkAttachmentId,
    reservation_claim: &'a NetworkReservationClaim,
    provider_kind: OciAttachmentProviderKind,
}

impl<'a> ReservedNetworkLaunchAuthority<'a> {
    pub(crate) fn new(
        allocator: &'a OciSegmentAllocator,
        ipam_authority: &'a OciIpamAuthority,
        identity: ReservedNetworkLaunchIdentity<'a>,
        provider_kind: OciAttachmentProviderKind,
    ) -> Self {
        Self {
            allocator,
            ipam_authority,
            layout: identity.layout,
            tenant_id: identity.tenant_id,
            sandbox_id: identity.sandbox_id,
            attachment_id: identity.attachment_id,
            reservation_claim: identity.reservation_claim,
            provider_kind,
        }
    }
}

/// Remove a tenant block-bridge interface by name once its last attachment has
/// drained (netavark won't auto-GC it). Idempotent / best-effort: a bridge that
/// is already gone is success.
pub(crate) fn reap_bridge_interface(interface: &str) -> Result<()> {
    delete_bridge(interface)
}

/// Fence one sandbox attachment before the first provider detach effect.
///
/// The hold remains authoritative until the caller has confirmed provider and
/// persistent-netns deletion, then calls [`release_network_segment_hold`].
pub(crate) fn quarantine_network_segment_hold(
    allocator: &OciSegmentAllocator,
    tenant_id: &TenantId,
    attachment_id: &NetworkAttachmentId,
    adoption_receipt: &NetworkReservationClaim,
) -> Result<NetworkSegmentQuarantineOutcome> {
    allocator.quarantine(tenant_id, attachment_id, Some(adoption_receipt))
}

/// Drop one quarantined sandbox hold after provider/netns deletion, reap every
/// bridge returned when the tenant drains, then finalize the exact allocation.
/// Any failed bridge deletion leaves durable cleanup-pending authority intact.
pub(crate) fn release_network_segment_hold(
    allocator: &OciSegmentAllocator,
    tenant_id: &TenantId,
    attachment_id: &NetworkAttachmentId,
    adoption_receipt: &NetworkReservationClaim,
) -> Vec<SandboxError> {
    release_network_segment_hold_with(
        allocator,
        tenant_id,
        attachment_id,
        Some(adoption_receipt),
        |segment| reap_bridge_interface(segment.network_interface()),
    )
}

/// Compensate one claim-authenticated launch before any provider effect.
///
/// The separate durable partitions form a reverse-order safe-leak saga:
/// callers must release the complete port batch first, then this function
/// first claim-authenticates the attachment into cleanup-pending authority,
/// then removes IPAM, and finally identity-fenced allocation authority. If
/// IPAM deletion or finalization fails, the exact cleanup claim remains fenced
/// for reconciliation.
pub(crate) fn compensate_reserved_network_launch_without_effect(
    authority: ReservedNetworkLaunchAuthority<'_>,
    planning_error: SandboxError,
) -> SandboxError {
    let errors = release_reserved_network_launch_without_effect(authority);
    if errors.is_empty() {
        planning_error
    } else {
        SandboxError::OperationFailed {
            message: format!(
                "sandbox launch planning failed: {planning_error}; \
                 claimed network launch compensation also failed: {}",
                errors
                    .into_iter()
                    .map(|error| error.to_string())
                    .collect::<Vec<_>>()
                    .join("; ")
            ),
        }
    }
}

/// Continue launch compensation only after complete port compensation succeeds.
///
/// A failed or ambiguous port release retains IPAM and segment authority as a
/// deliberate safe leak; releasing later resources would let another launch
/// reuse connectivity beneath still-fenced listeners.
pub(crate) fn compensate_reserved_network_launch_after_ports(
    authority: ReservedNetworkLaunchAuthority<'_>,
    planning_error: SandboxError,
    port_compensation: Result<()>,
) -> SandboxError {
    match release_reserved_network_launch_after_ports(authority, port_compensation) {
        Ok(()) => planning_error,
        Err(compensation_error) => SandboxError::OperationFailed {
            message: format!(
                "sandbox launch planning failed: {planning_error}; \
                 claimed network launch compensation also failed: {compensation_error}"
            ),
        },
    }
}

/// Release a never-realized launch in reverse reservation order.
///
/// The caller supplies the complete port compensation result. A failure stops
/// before IPAM or segment mutation and explicitly records that those later
/// resources remain fenced.
pub(crate) fn release_reserved_network_launch_after_ports(
    authority: ReservedNetworkLaunchAuthority<'_>,
    port_compensation: Result<()>,
) -> Result<()> {
    release_reserved_network_launch_after_ports_with_terminal_publication(
        authority,
        port_compensation,
        || Ok(()),
    )
}

/// Continue never-effected cleanup while publishing the backend manifest
/// before the exact terminal IPAM retry witness is retired.
///
/// The callback runs only after ports, IPAM, and segment authority are
/// terminal. If publication fails or its outcome is ambiguous, the terminal
/// IPAM witness remains as exact startup-reconciliation evidence.
pub(crate) fn release_reserved_network_launch_after_ports_with_terminal_publication(
    authority: ReservedNetworkLaunchAuthority<'_>,
    port_compensation: Result<()>,
    publish_terminal: impl FnOnce() -> Result<()>,
) -> Result<()> {
    if let Err(error) = port_compensation {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "never-bound port reservation compensation failed: {error}; \
                 IPAM and segment reservation remain fenced"
            ),
        });
    }
    let errors = release_reserved_network_launch_without_effect_with_terminal_publication(
        authority,
        publish_terminal,
    );
    if errors.is_empty() {
        Ok(())
    } else {
        Err(SandboxError::OperationFailed {
            message: errors
                .into_iter()
                .map(|error| error.to_string())
                .collect::<Vec<_>>()
                .join("; "),
        })
    }
}

fn release_reserved_network_launch_without_effect(
    authority: ReservedNetworkLaunchAuthority<'_>,
) -> Vec<SandboxError> {
    release_reserved_network_launch_without_effect_with_terminal_publication(authority, || Ok(()))
}

fn release_reserved_network_launch_without_effect_with_terminal_publication(
    authority: ReservedNetworkLaunchAuthority<'_>,
    publish_terminal: impl FnOnce() -> Result<()>,
) -> Vec<SandboxError> {
    match authority
        .allocator
        .release_reserved_attachment_without_effect(
            authority.tenant_id,
            authority.attachment_id,
            authority.reservation_claim,
        ) {
        Ok(
            NetworkSegmentReleaseOutcome::AttachmentCleanupPending
            | NetworkSegmentReleaseOutcome::CleanupPending(_),
        ) => {}
        Ok(NetworkSegmentReleaseOutcome::StillLive) => {
            return vec![SandboxError::OperationFailed {
                message: "never-realized attachment release skipped its durable IPAM cleanup fence"
                    .to_owned(),
            }];
        }
        Ok(NetworkSegmentReleaseOutcome::AlreadyReleased) => {
            return publish_then_retire_terminal_ipam(authority, publish_terminal);
        }
        Err(error) => return vec![error],
    };
    if let Err(error) = super::ipam::deallocate_container_ips_for_claim(
        authority.ipam_authority,
        authority.layout,
        authority.sandbox_id,
        authority.attachment_id,
        authority.reservation_claim,
        authority.provider_kind,
    ) {
        return vec![error];
    }
    let cleanup = match authority
        .allocator
        .finalize_reserved_attachment_without_effect(
            authority.tenant_id,
            authority.attachment_id,
            authority.reservation_claim,
        ) {
        Ok(NetworkSegmentReleaseOutcome::CleanupPending(cleanup)) => cleanup,
        Ok(
            NetworkSegmentReleaseOutcome::StillLive | NetworkSegmentReleaseOutcome::AlreadyReleased,
        ) => {
            return publish_then_retire_terminal_ipam(authority, publish_terminal);
        }
        Ok(NetworkSegmentReleaseOutcome::AttachmentCleanupPending) => {
            return vec![SandboxError::OperationFailed {
                message:
                    "never-realized attachment remained cleanup-pending after IPAM confirmation"
                        .to_owned(),
            }];
        }
        Err(error) => return vec![error],
    };
    match authority.allocator.finalize_release(&cleanup) {
        Ok(
            NetworkSegmentFinalizeOutcome::Released
            | NetworkSegmentFinalizeOutcome::AlreadyReleased,
        ) => publish_then_retire_terminal_ipam(authority, publish_terminal),
        Err(error) => vec![error],
    }
}

fn publish_then_retire_terminal_ipam(
    authority: ReservedNetworkLaunchAuthority<'_>,
    publish_terminal: impl FnOnce() -> Result<()>,
) -> Vec<SandboxError> {
    if let Err(error) = publish_terminal() {
        return vec![error];
    }
    super::ipam::retire_terminal_container_ipam_release(
        authority.ipam_authority,
        authority.layout,
        authority.sandbox_id,
        authority.attachment_id,
        authority.reservation_claim,
        authority.provider_kind,
    )
    .err()
    .into_iter()
    .collect()
}

fn release_network_segment_hold_with(
    allocator: &OciSegmentAllocator,
    tenant_id: &TenantId,
    attachment_id: &NetworkAttachmentId,
    adoption_receipt: Option<&NetworkReservationClaim>,
    mut reap: impl FnMut(&OciSegmentRealization) -> Result<()>,
) -> Vec<SandboxError> {
    if let Err(error) = allocator.quarantine(tenant_id, attachment_id, adoption_receipt) {
        return vec![error];
    }
    let cleanup = match allocator.release(tenant_id, attachment_id, adoption_receipt) {
        Ok(NetworkSegmentReleaseOutcome::CleanupPending(cleanup)) => cleanup,
        Ok(NetworkSegmentReleaseOutcome::StillLive) => return Vec::new(),
        Ok(NetworkSegmentReleaseOutcome::AlreadyReleased) => return Vec::new(),
        Ok(NetworkSegmentReleaseOutcome::AttachmentCleanupPending) => {
            return vec![SandboxError::OperationFailed {
                message:
                    "generic segment release encountered exact reservation IPAM cleanup authority"
                        .to_owned(),
            }];
        }
        Err(error) => return vec![error],
    };
    let errors: Vec<SandboxError> = cleanup
        .segments()
        .iter()
        .filter_map(|segment| reap(segment).err())
        .collect();
    if !errors.is_empty() {
        return errors;
    }
    match allocator.finalize_release(&cleanup) {
        Ok(
            NetworkSegmentFinalizeOutcome::Released
            | NetworkSegmentFinalizeOutcome::AlreadyReleased,
        ) => Vec::new(),
        Err(error) => vec![error],
    }
}

#[cfg(target_os = "linux")]
fn delete_bridge(interface: &str) -> Result<()> {
    use std::process::Command;

    let output = Command::new("ip")
        .args(["link", "del", interface])
        .output()
        .map_err(|error| SandboxError::OperationFailed {
            message: format!("failed to run `ip link del {interface}`: {error}"),
        })?;
    // A missing interface ("Cannot find device") is success — teardown is
    // idempotent and a crash may have already removed the bridge.
    if output.status.success()
        || String::from_utf8_lossy(&output.stderr).contains("Cannot find device")
    {
        Ok(())
    } else {
        Err(SandboxError::OperationFailed {
            message: format!(
                "`ip link del {interface}` failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        })
    }
}

#[cfg(not(target_os = "linux"))]
fn delete_bridge(_interface: &str) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests;
