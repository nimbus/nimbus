//! Tenant-bridge reaper and identity-fenced allocation finalization.
//!
//! netavark creates the per-tenant bridge on first-sandbox setup but does NOT
//! remove it on last-sandbox teardown, so the crash-safe reaper removes the
//! bridge after the last attachment hold releases into durable
//! cleanup-pending state, then identity-fenced finalization frees the
//! allocation. Obsolete shared-bridge migration is deliberately absent: this
//! pre-launch tree supports only the per-tenant routed model.

use std::collections::BTreeSet;
use std::path::Path;

use nimbus_core::TenantId;
use nimbus_network::{
    NetworkReservationClaim, NetworkSegmentFinalizeOutcome, NetworkSegmentQuarantineOutcome,
    NetworkSegmentReleaseOutcome,
};

use crate::error::{Result, SandboxError};
use crate::instance::SandboxId;

use super::{
    OciNetworkLayout, OciSegmentAllocator, OciSegmentRealization, default_network_attachment_id,
    ipam::OciIpamAuthority,
};

/// Exact retained authorities and identities for one never-realized launch.
#[derive(Clone, Copy)]
pub(crate) struct ReservedNetworkLaunchAuthority<'a> {
    allocator: &'a OciSegmentAllocator,
    ipam_authority: &'a OciIpamAuthority,
    layout: &'a OciNetworkLayout,
    tenant_id: &'a TenantId,
    sandbox_id: &'a SandboxId,
    reservation_claim: &'a NetworkReservationClaim,
}

impl<'a> ReservedNetworkLaunchAuthority<'a> {
    pub(crate) fn new(
        allocator: &'a OciSegmentAllocator,
        ipam_authority: &'a OciIpamAuthority,
        layout: &'a OciNetworkLayout,
        tenant_id: &'a TenantId,
        sandbox_id: &'a SandboxId,
        reservation_claim: &'a NetworkReservationClaim,
    ) -> Self {
        Self {
            allocator,
            ipam_authority,
            layout,
            tenant_id,
            sandbox_id,
            reservation_claim,
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
    sandbox_id: &SandboxId,
    adoption_receipt: &NetworkReservationClaim,
) -> Result<NetworkSegmentQuarantineOutcome> {
    allocator.quarantine(
        tenant_id,
        &default_network_attachment_id(sandbox_id),
        Some(adoption_receipt),
    )
}

/// Drop one quarantined sandbox hold after provider/netns deletion, reap every
/// bridge returned when the tenant drains, then finalize the exact allocation.
/// Any failed bridge deletion leaves durable cleanup-pending authority intact.
pub(crate) fn release_network_segment_hold(
    allocator: &OciSegmentAllocator,
    tenant_id: &TenantId,
    sandbox_id: &SandboxId,
    adoption_receipt: &NetworkReservationClaim,
) -> Vec<SandboxError> {
    release_network_segment_hold_with(
        allocator,
        tenant_id,
        sandbox_id,
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
    if let Err(error) = port_compensation {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "never-bound port reservation compensation failed: {error}; \
                 IPAM and segment reservation remain fenced"
            ),
        });
    }
    let errors = release_reserved_network_launch_without_effect(authority);
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
    match authority
        .allocator
        .release_reserved_attachment_without_effect(
            authority.tenant_id,
            &default_network_attachment_id(authority.sandbox_id),
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
            return super::ipam::retire_terminal_container_ipam_release(
                authority.ipam_authority,
                authority.layout,
                authority.sandbox_id,
                authority.reservation_claim,
            )
            .err()
            .into_iter()
            .collect();
        }
        Err(error) => return vec![error],
    };
    if let Err(error) = super::ipam::deallocate_container_ips_for_claim(
        authority.ipam_authority,
        authority.layout,
        authority.sandbox_id,
        authority.reservation_claim,
    ) {
        return vec![error];
    }
    let cleanup = match authority
        .allocator
        .finalize_reserved_attachment_without_effect(
            authority.tenant_id,
            &default_network_attachment_id(authority.sandbox_id),
            authority.reservation_claim,
        ) {
        Ok(NetworkSegmentReleaseOutcome::CleanupPending(cleanup)) => cleanup,
        Ok(
            NetworkSegmentReleaseOutcome::StillLive | NetworkSegmentReleaseOutcome::AlreadyReleased,
        ) => {
            return super::ipam::retire_terminal_container_ipam_release(
                authority.ipam_authority,
                authority.layout,
                authority.sandbox_id,
                authority.reservation_claim,
            )
            .err()
            .into_iter()
            .collect();
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
        ) => super::ipam::retire_terminal_container_ipam_release(
            authority.ipam_authority,
            authority.layout,
            authority.sandbox_id,
            authority.reservation_claim,
        )
        .err()
        .into_iter()
        .collect(),
        Err(error) => vec![error],
    }
}

fn release_network_segment_hold_with(
    allocator: &OciSegmentAllocator,
    tenant_id: &TenantId,
    sandbox_id: &SandboxId,
    adoption_receipt: Option<&NetworkReservationClaim>,
    mut reap: impl FnMut(&OciSegmentRealization) -> Result<()>,
) -> Vec<SandboxError> {
    let attachment_id = default_network_attachment_id(sandbox_id);
    if let Err(error) = allocator.quarantine(tenant_id, &attachment_id, adoption_receipt) {
        return vec![error];
    }
    let cleanup = match allocator.release(tenant_id, &attachment_id, adoption_receipt) {
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

/// Startup orphan scan: quarantine segment holds whose sandbox netns no longer
/// exists. The live-hold set is read
/// directly from the persistent-netns tree
/// (`<state_root>/tenants/<tenant>/networks/netns/<sandbox>`) — a live sandbox has
/// a netns; absence is only incomplete orphan evidence, not provider-deletion
/// proof. A crash-leaked hold therefore remains authoritative and unavailable
/// until the later evidence-aware reconciler inspects/detaches it. Best-effort
/// and idempotent. Returns the number of provider segment realizations covered
/// by quarantined allocations (a multi-block tenant contributes each block).
pub(crate) fn reconcile_network_segment_orphans(
    state_root: &Path,
    allocator: &OciSegmentAllocator,
) -> Result<usize> {
    let live = live_netns_holds(state_root)?;
    let quarantined = allocator.reconcile_orphans(&live)?;
    Ok(quarantined.len())
}

/// Enumerate the `(tenant_id, sandbox_id)` pairs that currently hold a persistent
/// netns. A missing tree (fresh node) yields the empty set.
fn live_netns_holds(
    state_root: &Path,
) -> Result<BTreeSet<(TenantId, nimbus_network::NetworkAttachmentId)>> {
    let mut holds = BTreeSet::new();
    let tenants_root = state_root.join("tenants");
    let Ok(tenants) = std::fs::read_dir(&tenants_root) else {
        return Ok(holds);
    };
    for tenant in tenants.flatten() {
        let netns_dir = tenant.path().join("networks").join("netns");
        let Ok(sandboxes) = std::fs::read_dir(&netns_dir) else {
            continue;
        };
        let tenant_name = tenant.file_name().to_string_lossy().into_owned();
        let tenant_id =
            TenantId::new(&tenant_name).map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "persistent network namespace tree contains invalid tenant id {tenant_name:?}: {error}"
                ),
            })?;
        for sandbox in sandboxes.flatten() {
            let sandbox_id = sandbox.file_name().to_string_lossy().into_owned();
            holds.insert((
                tenant_id.clone(),
                default_network_attachment_id(&SandboxId::new(sandbox_id)),
            ));
        }
    }
    Ok(holds)
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
mod tests {
    use std::collections::BTreeMap;

    use super::super::OciNetworkConfig;
    use super::*;
    use crate::backends::oci::network::{SingleNodeSegmentAllocator, direct_test_ipam_authority};
    use nimbus_core::TenantId;
    use nimbus_network::{
        LocalNetworkStateStore, NetworkProviderHandle, NetworkProviderId, NetworkReservationClaim,
        NetworkSegmentAllocator,
    };
    use tempfile::tempdir;

    use crate::instance::SandboxId;

    fn touch_netns(root: &Path, tenant: &str, sandbox: &str) {
        let dir = root
            .join("tenants")
            .join(tenant)
            .join("networks")
            .join("netns");
        std::fs::create_dir_all(&dir).expect("netns dir");
        std::fs::write(dir.join(sandbox), b"").expect("netns file");
    }

    fn touch_evidence(root: &Path, directory: &str, tenant: &str, sandbox: &str, value: &str) {
        let dir = root
            .join("tenants")
            .join(tenant)
            .join("networks")
            .join(directory);
        std::fs::create_dir_all(&dir).expect("evidence directory");
        std::fs::write(dir.join(format!("{sandbox}.json")), value).expect("evidence file");
    }

    fn touch_manifest(root: &Path, tenant: &str, sandbox: &str) {
        let dir = root.join("tenants").join(tenant).join("sandboxes");
        std::fs::create_dir_all(&dir).expect("manifest directory");
        std::fs::write(dir.join(format!("{sandbox}.json")), "{}").expect("manifest file");
    }

    fn evidence_exists(root: &Path, directory: &str, tenant: &str, sandbox: &str) -> bool {
        root.join("tenants")
            .join(tenant)
            .join("networks")
            .join(directory)
            .join(format!("{sandbox}.json"))
            .exists()
    }

    fn manifest_exists(root: &Path, tenant: &str, sandbox: &str) -> bool {
        root.join("tenants")
            .join(tenant)
            .join("sandboxes")
            .join(format!("{sandbox}.json"))
            .exists()
    }

    fn reservation_claim(attempt: &str) -> NetworkReservationClaim {
        let provider = NetworkProviderId::for_registration_key(
            "nimbus-sandbox.network-launch-coordinator.test",
        );
        NetworkReservationClaim::new(
            NetworkProviderHandle::new(provider, format!("attempt:{attempt}"))
                .expect("claim fixture should validate"),
        )
    }

    fn allocator_has_hold(root: &Path, tenant: &str, sandbox: &str) -> bool {
        SingleNodeSegmentAllocator::single_node_default(root).has_hold(tenant, sandbox)
    }

    #[test]
    fn exact_pre_effect_compensation_removes_ipam_before_segment_authority() {
        let dir = tempdir().expect("state root");
        let tenant = TenantId::new("tenant-exact-compensation").expect("tenant fixture");
        let sandbox = SandboxId::new("sandbox-exact-compensation");
        let layout = OciNetworkLayout::under_root(dir.path(), &tenant, &sandbox);
        let ipam_authority = direct_test_ipam_authority(&layout);
        layout
            .ensure_directories()
            .expect("layout should initialize");
        let allocator = SingleNodeSegmentAllocator::single_node_default(dir.path());
        let claim = reservation_claim("exact-compensation");
        super::super::placement::place_sandbox_on_block(
            &allocator,
            &ipam_authority,
            &tenant,
            &layout,
            &sandbox,
            &claim,
            |segment, reservation_claim| OciNetworkConfig {
                network_name: segment.network_name().to_owned(),
                network_interface: segment.network_interface().to_owned(),
                network_subnet: segment.cidr().to_string(),
                segment_id: segment.segment_id().as_str().to_owned(),
                reservation_claim: reservation_claim.clone(),
                network_id: segment.network_id().as_str().to_owned(),
                ..OciNetworkConfig::default()
            },
        )
        .expect("placement should reserve attachment and IPAM");
        assert!(
            super::super::ipam::load_container_ips(&ipam_authority, &layout, &sandbox).is_ok(),
            "placement must create durable IPAM before port reservation"
        );

        release_reserved_network_launch_after_ports(
            ReservedNetworkLaunchAuthority::new(
                &allocator,
                &ipam_authority,
                &layout,
                &tenant,
                &sandbox,
                &claim,
            ),
            Ok(()),
        )
        .expect("exact compensation should complete");

        assert!(
            super::super::ipam::load_container_ips(&ipam_authority, &layout, &sandbox).is_err(),
            "IPAM must be gone before segment authority is reusable"
        );
        assert!(
            allocator
                .inspect_segments(&tenant)
                .expect("segment authority should inspect")
                .is_none(),
            "last exact hold should finalize the tenant segment"
        );
    }

    #[test]
    fn exact_pre_effect_compensation_removes_ipam_while_sibling_hold_remains() {
        let dir = tempdir().expect("state root");
        let tenant = TenantId::new("tenant-sibling-compensation").expect("tenant fixture");
        let cancelled = SandboxId::new("sandbox-cancelled");
        let sibling = SandboxId::new("sandbox-sibling");
        let cancelled_layout = OciNetworkLayout::under_root(dir.path(), &tenant, &cancelled);
        let sibling_layout = OciNetworkLayout::under_root(dir.path(), &tenant, &sibling);
        let ipam_authority = direct_test_ipam_authority(&cancelled_layout);
        cancelled_layout
            .ensure_directories()
            .expect("cancelled layout should initialize");
        sibling_layout
            .ensure_directories()
            .expect("sibling layout should initialize");
        let allocator = SingleNodeSegmentAllocator::single_node_default(dir.path());
        let cancelled_claim = reservation_claim("cancelled");
        let sibling_claim = reservation_claim("sibling");
        for (layout, sandbox, claim) in [
            (&cancelled_layout, &cancelled, &cancelled_claim),
            (&sibling_layout, &sibling, &sibling_claim),
        ] {
            super::super::placement::place_sandbox_on_block(
                &allocator,
                &ipam_authority,
                &tenant,
                layout,
                sandbox,
                claim,
                |segment, reservation_claim| OciNetworkConfig {
                    network_name: segment.network_name().to_owned(),
                    network_interface: segment.network_interface().to_owned(),
                    network_subnet: segment.cidr().to_string(),
                    segment_id: segment.segment_id().as_str().to_owned(),
                    reservation_claim: reservation_claim.clone(),
                    network_id: segment.network_id().as_str().to_owned(),
                    ..OciNetworkConfig::default()
                },
            )
            .expect("placement should reserve attachment and IPAM");
        }

        release_reserved_network_launch_after_ports(
            ReservedNetworkLaunchAuthority::new(
                &allocator,
                &ipam_authority,
                &cancelled_layout,
                &tenant,
                &cancelled,
                &cancelled_claim,
            ),
            Ok(()),
        )
        .expect("exact compensation should remove only the cancelled launch");

        assert!(
            super::super::ipam::load_container_ips(&ipam_authority, &cancelled_layout, &cancelled,)
                .is_err(),
            "the cancelled launch must not leak IPAM merely because a sibling retains the segment"
        );
        assert!(
            super::super::ipam::load_container_ips(&ipam_authority, &sibling_layout, &sibling)
                .is_ok(),
            "the live sibling's IPAM allocation must remain intact"
        );
        assert!(
            !allocator.has_hold(tenant.as_str(), cancelled.as_str()),
            "the exact cancelled attachment must be released"
        );
        assert!(
            allocator.has_hold(tenant.as_str(), sibling.as_str()),
            "the sibling attachment must continue to hold the tenant segment"
        );
        assert!(
            allocator
                .inspect_segments(&tenant)
                .expect("segment authority should inspect")
                .is_some(),
            "a sibling attachment must keep the tenant segment live"
        );
    }

    #[test]
    fn foreign_pre_effect_claim_preserves_ipam_and_segment_authority_byte_for_byte() {
        let dir = tempdir().expect("state root");
        let tenant = TenantId::new("tenant-foreign-compensation").expect("tenant fixture");
        let sandbox = SandboxId::new("sandbox-foreign-compensation");
        let layout = OciNetworkLayout::under_root(dir.path(), &tenant, &sandbox);
        let ipam_authority = direct_test_ipam_authority(&layout);
        layout
            .ensure_directories()
            .expect("layout should initialize");
        let allocator = SingleNodeSegmentAllocator::single_node_default(dir.path());
        let winner = reservation_claim("winner");
        super::super::placement::place_sandbox_on_block(
            &allocator,
            &ipam_authority,
            &tenant,
            &layout,
            &sandbox,
            &winner,
            |segment, reservation_claim| OciNetworkConfig {
                network_name: segment.network_name().to_owned(),
                network_interface: segment.network_interface().to_owned(),
                network_subnet: segment.cidr().to_string(),
                segment_id: segment.segment_id().as_str().to_owned(),
                reservation_claim: reservation_claim.clone(),
                network_id: segment.network_id().as_str().to_owned(),
                ..OciNetworkConfig::default()
            },
        )
        .expect("winner should reserve attachment and IPAM");
        let authority_path = LocalNetworkStateStore::authority_path_for(dir.path());
        let before = std::fs::read(&authority_path).expect("authority should be durable");

        let error = release_reserved_network_launch_after_ports(
            ReservedNetworkLaunchAuthority::new(
                &allocator,
                &ipam_authority,
                &layout,
                &tenant,
                &sandbox,
                &reservation_claim("foreign"),
            ),
            Ok(()),
        )
        .expect_err("a foreign coordinator must not compensate the winner");
        assert!(
            error
                .to_string()
                .contains("different launch reservation coordinator"),
            "claim rejection should remain explicit: {error}"
        );
        assert_eq!(
            std::fs::read(&authority_path).expect("rejected authority should remain readable"),
            before,
            "claim authentication must precede every IPAM or segment mutation"
        );
        assert!(
            super::super::ipam::load_container_ips(&ipam_authority, &layout, &sandbox).is_ok(),
            "the winning coordinator's IPAM evidence must remain intact"
        );
        assert!(
            allocator.has_hold(tenant.as_str(), sandbox.as_str()),
            "the winning coordinator's reserved attachment must remain intact"
        );
    }

    #[test]
    fn reconcile_quarantines_holds_whose_netns_is_gone_and_keeps_live_ones() {
        let dir = tempdir().expect("temp dir");
        let root = dir.path();
        let allocator = SingleNodeSegmentAllocator::single_node_default(root);

        // tenant-live (index 0) holds a sandbox that still has a netns.
        allocator
            .acquire(
                &TenantId::new("tenant-live").unwrap(),
                &default_network_attachment_id(&SandboxId::new("sb-live")),
            )
            .expect("acquire live");
        touch_netns(root, "tenant-live", "sb-live");
        // tenant-dead (index 1) holds a sandbox whose netns is gone (crash-leaked).
        allocator
            .acquire(
                &TenantId::new("tenant-dead").unwrap(),
                &default_network_attachment_id(&SandboxId::new("sb-dead")),
            )
            .expect("acquire dead");

        let quarantined = reconcile_network_segment_orphans(root, &allocator).expect("reconcile");
        assert_eq!(quarantined, 1, "only the netns-less tenant is quarantined");
        assert!(
            allocator.has_hold("tenant-dead", "sb-dead")
                && allocator.has_pending_hold("tenant-dead", "sb-dead"),
            "netns absence alone must preserve and quarantine the durable hold"
        );

        // tenant-dead's uncertain index 1 remains unavailable.
        let fresh = allocator
            .acquire(
                &TenantId::new("tenant-new").unwrap(),
                &default_network_attachment_id(&SandboxId::new("sb-new")),
            )
            .expect("acquire new");
        assert_eq!(fresh.cidr().to_string(), "10.0.2.0/24");
        // tenant-live still holds its original index 0.
        let live = allocator
            .acquire(
                &TenantId::new("tenant-live").unwrap(),
                &default_network_attachment_id(&SandboxId::new("sb-live")),
            )
            .expect("re-acquire live");
        assert_eq!(live.cidr().to_string(), "10.0.0.0/24");
    }

    #[test]
    fn startup_reconciliation_reads_live_netns_from_the_workload_root() {
        let dir = tempdir().expect("temp dir");
        let workload_root = dir.path().join("project-state");
        let network_root = dir.path().join("node-network-state");
        let allocator = SingleNodeSegmentAllocator::single_node_default(&network_root);
        let tenant = TenantId::new("tenant-split-root-live").expect("tenant should parse");
        let sandbox = SandboxId::new("sandbox-split-root-live");
        let layout = OciNetworkLayout::with_roots(&workload_root, &network_root, &tenant, &sandbox);
        let ipam_authority = direct_test_ipam_authority(&layout);
        allocator
            .acquire(&tenant, &default_network_attachment_id(&sandbox))
            .expect("live attachment should allocate");
        touch_netns(&workload_root, tenant.as_str(), sandbox.as_str());

        super::super::reconcile_startup_network_state(&workload_root, &ipam_authority, &allocator)
            .expect("startup reconciliation must retain workload-root netns evidence");

        assert!(
            allocator.has_hold(tenant.as_str(), sandbox.as_str())
                && !allocator.has_pending_hold(tenant.as_str(), sandbox.as_str()),
            "a live workload-root netns must not be quarantined as a network-root orphan"
        );
    }

    #[test]
    fn reconcile_ignores_non_tenant_siblings_without_a_netns_tree() {
        let dir = tempdir().expect("temp dir");
        let root = dir.path();
        let allocator = SingleNodeSegmentAllocator::single_node_default(root);
        let tenant = TenantId::new("tenant-dead").expect("tenant should parse");
        allocator
            .acquire(
                &tenant,
                &default_network_attachment_id(&SandboxId::new("sb-dead")),
            )
            .expect("orphan hold should allocate");
        let tenants_root = root.join("tenants");
        std::fs::create_dir_all(&tenants_root).expect("tenants root should exist");
        std::fs::write(tenants_root.join(".DS_Store"), b"foreign metadata")
            .expect("foreign sibling fixture should write");

        let quarantined = reconcile_network_segment_orphans(root, &allocator)
            .expect("a non-tenant sibling without a netns tree must be ignored");

        assert_eq!(quarantined, 1, "the real orphan must still be quarantined");
        assert!(
            allocator.has_hold(tenant.as_str(), "sb-dead")
                && allocator.has_pending_hold(tenant.as_str(), "sb-dead"),
            "foreign siblings must not suppress durable hold quarantine"
        );
    }

    #[derive(Clone, Copy)]
    struct OrphanEvidenceCase {
        name: &'static str,
        hold: bool,
        desired: bool,
        netns: bool,
        manifest: bool,
        effect: bool,
        desired_generation: u64,
        effect_generation: u64,
        inspection_unknown: bool,
    }

    #[derive(Debug, PartialEq, Eq)]
    struct OrphanObservation {
        reclaimed_segments: usize,
        hold: bool,
        desired: bool,
        netns: bool,
        manifest: bool,
        effect: bool,
        classifier_result: &'static str,
    }

    fn observe_orphan_case(case: OrphanEvidenceCase) -> OrphanObservation {
        let dir = tempdir().expect("temp dir");
        let root = dir.path();
        let allocator = SingleNodeSegmentAllocator::single_node_default(root);
        let tenant = format!("tenant-{}", case.name);
        let sandbox = format!("sandbox-{}", case.name);
        let tenant_id = TenantId::new(&tenant).expect("tenant id should parse");
        let sandbox_id = SandboxId::new(&sandbox);

        if case.hold {
            allocator
                .acquire(&tenant_id, &default_network_attachment_id(&sandbox_id))
                .expect("hold should persist");
        }
        if case.desired {
            touch_evidence(
                root,
                "attachments",
                &tenant,
                &sandbox,
                &format!(r#"{{"generation":{}}}"#, case.desired_generation),
            );
        }
        if case.netns {
            touch_netns(root, &tenant, &sandbox);
        }
        if case.manifest {
            touch_manifest(root, &tenant, &sandbox);
        }
        if case.effect {
            touch_evidence(
                root,
                "provider-effects",
                &tenant,
                &sandbox,
                &format!(
                    r#"{{"generation":{},"inspection":"{}"}}"#,
                    case.effect_generation,
                    if case.inspection_unknown {
                        "unknown"
                    } else {
                        "present"
                    }
                ),
            );
        }

        let reclaimed_segments =
            reconcile_network_segment_orphans(root, &allocator).expect("reconcile should run");
        let hold = allocator_has_hold(root, &tenant, &sandbox);
        let desired = evidence_exists(root, "attachments", &tenant, &sandbox);
        let netns = root
            .join("tenants")
            .join(&tenant)
            .join("networks")
            .join("netns")
            .join(&sandbox)
            .exists();
        let manifest = manifest_exists(root, &tenant, &sandbox);
        let effect = evidence_exists(root, "provider-effects", &tenant, &sandbox);
        let classifier_result = if hold {
            "retained-by-netns-filename"
        } else if desired || netns || manifest || effect {
            "unowned-evidence-left-behind"
        } else {
            "fully-removed"
        };

        OrphanObservation {
            reclaimed_segments,
            hold,
            desired,
            netns,
            manifest,
            effect,
            classifier_result,
        }
    }

    #[test]
    // This is the NNC0.7 fail-before executable baseline for the exact
    // `provider effect -> allocator hold` crash window in both OCI-family
    // backends. NNC0.1b already proves exact-boundary process killing and
    // same-root recovery; this test materializes the durable recovery image
    // left by that cut without duplicating the upper-layer subprocess harness
    // (which cannot be a dependency of this low-level crate).
    #[ignore = "NNC0.7 expected red until provider attempts precede effects and reconcile removes or quarantines unowned effects"]
    fn nnc0_7_effect_before_hold_crash_must_not_leave_an_unowned_provider_effect() {
        let observed = observe_orphan_case(OrphanEvidenceCase {
            name: "crash-after-effect-before-hold",
            hold: false,
            desired: false,
            netns: true,
            manifest: true,
            effect: true,
            desired_generation: 0,
            effect_generation: 7,
            inspection_unknown: false,
        });

        assert!(!observed.hold, "the crash cut precedes allocator acquire");
        assert!(
            observed.netns && observed.effect,
            "the exact provider-effect boundary must be present before the safety assertion"
        );
        assert_eq!(
            observed.classifier_result, "fully-removed",
            "NNCF8: recovery must remove or durably quarantine the provider effect and netns \
             when no desired attachment/provider attempt owns them"
        );
    }

    #[test]
    // This is the complete NNC0.7 fail-before evidence matrix for NNCF8. It
    // intentionally uses durable desired/effect/generation/inspection markers
    // that the current filename-only reaper cannot read. NNC5.2a owns the
    // classifier and must turn this green by adopting, removing, or
    // quarantining every row; NNC8.3 owns restart convergence.
    #[ignore = "NNC0.7 expected red until orphan recovery classifies durable intent, provider attempts, generations, and unknown inspection"]
    fn nnc0_7_orphan_recovery_must_classify_the_complete_evidence_matrix() {
        let cases = [
            (
                OrphanEvidenceCase {
                    name: "hold-desired-effect",
                    hold: true,
                    desired: true,
                    netns: true,
                    manifest: true,
                    effect: true,
                    desired_generation: 7,
                    effect_generation: 7,
                    inspection_unknown: false,
                },
                "adopted",
            ),
            (
                OrphanEvidenceCase {
                    name: "hold-no-desired-effect",
                    hold: true,
                    desired: false,
                    netns: true,
                    manifest: true,
                    effect: true,
                    desired_generation: 0,
                    effect_generation: 7,
                    inspection_unknown: false,
                },
                "removed-or-quarantined",
            ),
            (
                OrphanEvidenceCase {
                    name: "hold-no-netns",
                    hold: true,
                    desired: true,
                    netns: false,
                    manifest: true,
                    effect: false,
                    desired_generation: 7,
                    effect_generation: 0,
                    inspection_unknown: false,
                },
                "removed-or-quarantined",
            ),
            (
                OrphanEvidenceCase {
                    name: "effect-no-hold",
                    hold: false,
                    desired: false,
                    netns: true,
                    manifest: true,
                    effect: true,
                    desired_generation: 0,
                    effect_generation: 7,
                    inspection_unknown: false,
                },
                "removed-or-quarantined",
            ),
            (
                OrphanEvidenceCase {
                    name: "manifest-no-hold",
                    hold: false,
                    desired: false,
                    netns: false,
                    manifest: true,
                    effect: false,
                    desired_generation: 0,
                    effect_generation: 0,
                    inspection_unknown: false,
                },
                "removed-or-quarantined",
            ),
            (
                OrphanEvidenceCase {
                    name: "hold-netns-no-manifest",
                    hold: true,
                    desired: true,
                    netns: true,
                    manifest: false,
                    effect: true,
                    desired_generation: 7,
                    effect_generation: 7,
                    inspection_unknown: false,
                },
                "adopted",
            ),
            (
                OrphanEvidenceCase {
                    name: "stale-generation",
                    hold: true,
                    desired: true,
                    netns: true,
                    manifest: true,
                    effect: true,
                    desired_generation: 8,
                    effect_generation: 7,
                    inspection_unknown: false,
                },
                "removed-or-quarantined",
            ),
            (
                OrphanEvidenceCase {
                    name: "unknown-inspection",
                    hold: true,
                    desired: true,
                    netns: true,
                    manifest: true,
                    effect: true,
                    desired_generation: 7,
                    effect_generation: 7,
                    inspection_unknown: true,
                },
                "cleanup-pending",
            ),
        ];

        let observed: BTreeMap<&str, (&str, OrphanObservation)> = cases
            .into_iter()
            .map(|(case, expected)| (case.name, (expected, observe_orphan_case(case))))
            .collect();
        assert_eq!(observed.len(), 8, "every required evidence arm must run");
        assert!(
            observed.values().all(|(_, observation)| {
                observation.classifier_result == "retained-by-netns-filename"
                    || observation.classifier_result == "unowned-evidence-left-behind"
            }),
            "precondition: the current reaper must expose its filename/hold-only behavior"
        );

        let mismatches: BTreeMap<&str, (&str, &str)> = observed
            .iter()
            .filter_map(|(name, (expected, observation))| {
                (*expected != observation.classifier_result)
                    .then_some((*name, (*expected, observation.classifier_result)))
            })
            .collect();
        assert!(
            mismatches.is_empty(),
            "NNCF8: every restart state must be adopted, removed, quarantined, or held \
             cleanup-pending from durable ownership evidence; current mismatches: {mismatches:#?}; \
             full observations: {observed:#?}"
        );
    }

    #[test]
    // NNC0.3 pass-after: bridge cleanup failure leaves the exact allocation
    // cleanup-pending; a later successful retry finalizes it exactly once.
    fn failed_bridge_cleanup_must_fence_segment_from_reuse() {
        let dir = tempdir().expect("temp dir");
        let allocator = SingleNodeSegmentAllocator::single_node_default(dir.path());
        let original_tenant = TenantId::new("tenant-original").expect("tenant should parse");
        let original_sandbox = SandboxId::new("sandbox-original");
        let original = allocator
            .acquire(
                &original_tenant,
                &default_network_attachment_id(&original_sandbox),
            )
            .expect("original segment should allocate");

        let mut surviving_bridges = Vec::new();
        let cleanup_errors = release_network_segment_hold_with(
            &allocator,
            &original_tenant,
            &original_sandbox,
            None,
            |segment| {
                surviving_bridges.push(segment.network_interface().to_owned());
                Err(SandboxError::OperationFailed {
                    message: "forced bridge provider cleanup failure".to_owned(),
                })
            },
        );
        assert_eq!(cleanup_errors.len(), 1);
        assert!(
            cleanup_errors[0]
                .to_string()
                .contains("forced bridge provider cleanup failure")
        );
        assert_eq!(
            surviving_bridges,
            [original.network_interface().to_owned()],
            "the failed provider cleanup leaves the original bridge effect present"
        );

        let replacement = allocator
            .acquire(
                &TenantId::new("tenant-replacement").expect("tenant should parse"),
                &default_network_attachment_id(&SandboxId::new("sandbox-replacement")),
            )
            .expect("replacement segment should allocate");
        assert_ne!(
            replacement.cidr(),
            original.cidr(),
            "a segment with a surviving provider effect must remain fenced from reuse"
        );

        let mut successful_reaps = 0usize;
        let retry_errors = release_network_segment_hold_with(
            &allocator,
            &original_tenant,
            &original_sandbox,
            None,
            |segment| {
                successful_reaps += 1;
                assert_eq!(segment.segment_id(), original.segment_id());
                Ok(())
            },
        );
        assert!(retry_errors.is_empty(), "cleanup retry should succeed");
        assert_eq!(successful_reaps, 1, "the bridge should be deleted once");

        let recovered = allocator
            .acquire(
                &TenantId::new("tenant-recovered").expect("tenant should parse"),
                &default_network_attachment_id(&SandboxId::new("sandbox-recovered")),
            )
            .expect("confirmed cleanup should make the slot reusable");
        assert_eq!(recovered.cidr(), original.cidr());
        assert_ne!(
            recovered.segment_id(),
            original.segment_id(),
            "reused location must receive a new stable identity"
        );

        let repeated_errors = release_network_segment_hold_with(
            &allocator,
            &original_tenant,
            &original_sandbox,
            None,
            |_| {
                successful_reaps += 1;
                Ok(())
            },
        );
        assert!(
            repeated_errors.is_empty(),
            "repeated finalization should be idempotent"
        );
        assert_eq!(
            successful_reaps, 1,
            "already-finalized cleanup must not repeat provider deletion"
        );
        let next = allocator
            .acquire(
                &TenantId::new("tenant-next").expect("tenant should parse"),
                &default_network_attachment_id(&SandboxId::new("sandbox-next")),
            )
            .expect("next segment should allocate");
        assert_eq!(
            next.cidr().to_string(),
            "10.0.2.0/24",
            "the recovered slot is owned exactly once, not handed out twice"
        );
    }
}
