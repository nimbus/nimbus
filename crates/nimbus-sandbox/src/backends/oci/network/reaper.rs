//! Tenant-bridge reaper + legacy shared-bridge migration.
//!
//! netavark creates the per-tenant bridge on first-sandbox setup but does NOT
//! remove it on last-sandbox teardown, so the crash-safe reaper removes the
//! bridge when the allocator reports the tenant drained (the last sandbox hold
//! released). The one-shot legacy purge removes the pre-MTN shared `nimbus0`
//! bridge before the first per-tenant setup, since the routed per-tenant model
//! deletes the shared bridge (pre-launch, breaking — no compat path).

use std::path::Path;

use crate::error::{Result, SandboxError};

use super::OciNetworkConfig;

/// Remove the tenant's bridge interface once its last sandbox has drained
/// (netavark won't auto-GC it). Idempotent / best-effort: a bridge that is
/// already gone is success.
pub(crate) fn reap_tenant_bridge(network_config: &OciNetworkConfig) -> Result<()> {
    delete_bridge(&network_config.network_interface)
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
