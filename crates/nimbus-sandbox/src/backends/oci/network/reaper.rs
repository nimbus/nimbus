//! Tenant-bridge reaper + legacy shared-bridge migration.
//!
//! netavark creates the per-tenant bridge on first-sandbox setup but does NOT
//! remove it on last-sandbox teardown, so the crash-safe reaper removes the
//! bridge when the allocator reports the tenant drained (the last sandbox hold
//! released). The one-shot legacy purge removes the pre-MTN shared `nimbus0`
//! bridge before the first per-tenant setup, since the routed per-tenant model
//! deletes the shared bridge (pre-launch, breaking — no compat path).

use std::collections::BTreeSet;
use std::path::Path;

use crate::error::{Result, SandboxError};

use super::{OciNetworkConfig, SingleNodeSegmentAllocator};

/// Remove the tenant's bridge interface once its last sandbox has drained
/// (netavark won't auto-GC it). Idempotent / best-effort: a bridge that is
/// already gone is success.
pub(crate) fn reap_tenant_bridge(network_config: &OciNetworkConfig) -> Result<()> {
    reap_bridge_interface(&network_config.network_interface)
}

/// Remove a bridge interface by name (idempotent / best-effort).
pub(crate) fn reap_bridge_interface(interface: &str) -> Result<()> {
    delete_bridge(interface)
}

/// Startup orphan GC: reclaim segment holds whose sandbox netns no longer exists,
/// and reap the tenant bridges that drain as a result. The live-hold set is read
/// directly from the persistent-netns tree
/// (`<state_root>/tenants/<tenant>/networks/netns/<sandbox>`) — a live sandbox has
/// a netns; a cleanly-torn-down one does not — so no manifest parsing is needed
/// and a crash that leaked a hold (netns gone, allocator entry stranded) is
/// reclaimed while a still-live sandbox is conservatively kept. Best-effort +
/// idempotent. Returns the number of tenant bridges reclaimed (the reclaimed
/// metric).
pub(crate) fn reconcile_network_segment_orphans(
    state_root: &Path,
    allocator: &SingleNodeSegmentAllocator,
) -> Result<usize> {
    let live = live_netns_holds(state_root);
    let drained = allocator.reconcile_orphans(&live)?;
    for segment in &drained {
        reap_bridge_interface(segment.network_interface())?;
    }
    Ok(drained.len())
}

/// Enumerate the `(tenant_id, sandbox_id)` pairs that currently hold a persistent
/// netns. A missing tree (fresh node) yields the empty set.
fn live_netns_holds(state_root: &Path) -> BTreeSet<(String, String)> {
    let mut holds = BTreeSet::new();
    let tenants_root = state_root.join("tenants");
    let Ok(tenants) = std::fs::read_dir(&tenants_root) else {
        return holds;
    };
    for tenant in tenants.flatten() {
        let tenant_id = tenant.file_name().to_string_lossy().into_owned();
        let netns_dir = tenant.path().join("networks").join("netns");
        let Ok(sandboxes) = std::fs::read_dir(&netns_dir) else {
            continue;
        };
        for sandbox in sandboxes.flatten() {
            let sandbox_id = sandbox.file_name().to_string_lossy().into_owned();
            holds.insert((tenant_id.clone(), sandbox_id));
        }
    }
    holds
}

/// One-shot migration: remove the legacy shared `nimbus0` bridge from the pre-MTN
/// single-bridge scheme, guarded by a marker under `<networks_root>` so it runs
/// at most once per node. Best-effort / idempotent.
pub(crate) fn purge_legacy_nimbus0_once(networks_root: &Path) -> Result<()> {
    let marker = networks_root.join(".legacy-nimbus0-purged");
    if marker.exists() {
        return Ok(());
    }
    delete_bridge(super::DEFAULT_NETWORK_INTERFACE)?;
    std::fs::create_dir_all(networks_root).map_err(|error| SandboxError::OperationFailed {
        message: format!(
            "failed to create networks root {} for the legacy-purge marker: {error}",
            networks_root.display()
        ),
    })?;
    std::fs::write(&marker, b"legacy nimbus0 bridge purged by MTN migration\n").map_err(|error| {
        SandboxError::OperationFailed {
            message: format!(
                "failed to write legacy-purge marker {}: {error}",
                marker.display()
            ),
        }
    })
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
    use super::*;
    use nimbus_core::TenantId;
    use tempfile::tempdir;

    use crate::backends::oci::network::NetworkSegmentAllocator;
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

    #[test]
    fn reconcile_reclaims_holds_whose_netns_is_gone_and_keeps_live_ones() {
        let dir = tempdir().expect("temp dir");
        let root = dir.path();
        let allocator = SingleNodeSegmentAllocator::single_node_default(root);

        // tenant-live (index 0) holds a sandbox that still has a netns.
        allocator
            .acquire(
                &TenantId::new("tenant-live").unwrap(),
                &SandboxId::new("sb-live"),
            )
            .expect("acquire live");
        touch_netns(root, "tenant-live", "sb-live");
        // tenant-dead (index 1) holds a sandbox whose netns is gone (crash-leaked).
        allocator
            .acquire(
                &TenantId::new("tenant-dead").unwrap(),
                &SandboxId::new("sb-dead"),
            )
            .expect("acquire dead");

        let reclaimed = reconcile_network_segment_orphans(root, &allocator).expect("reconcile");
        assert_eq!(reclaimed, 1, "only the netns-less tenant is reclaimed");

        // tenant-dead's index 1 was freed -> reused by the next new tenant.
        let reused = allocator
            .acquire(
                &TenantId::new("tenant-new").unwrap(),
                &SandboxId::new("sb-new"),
            )
            .expect("acquire new");
        assert_eq!(reused.cidr().to_string(), "10.0.1.0/24");
        // tenant-live still holds its original index 0.
        let live = allocator
            .acquire(
                &TenantId::new("tenant-live").unwrap(),
                &SandboxId::new("sb-live"),
            )
            .expect("re-acquire live");
        assert_eq!(live.cidr().to_string(), "10.0.0.0/24");
    }
}
