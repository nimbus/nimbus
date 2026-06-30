//! Restart decision state machine for supervised containers.

use crate::backends::conmon::lifecycle::{
    read_exit_code, restart_backoff_delay, restart_policy_allows_restart,
};
use crate::error::Result;
use crate::instance::SandboxStatus;

use super::manifest::ContainerSandboxManifest;
use super::status::synchronize_handle_status;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ContainerRestartDecision {
    NotRestarting,
    WaitingForBackoff,
    RestartNow,
}

pub(super) fn mark_restart_decision_after_exit(
    manifest: &mut ContainerSandboxManifest,
    now_millis: u64,
) -> Result<ContainerRestartDecision> {
    if manifest.shutdown_requested || !manifest.conmon_layout.exit_status_file.exists() {
        return Ok(ContainerRestartDecision::NotRestarting);
    }

    let exit_code = read_exit_code(&manifest.conmon_layout.exit_status_file)?;
    if !restart_policy_allows_restart(
        manifest.spec.lifecycle.restart_policy,
        exit_code,
        manifest.restart_count,
    ) {
        return Ok(ContainerRestartDecision::NotRestarting);
    }

    manifest.last_exit_code = Some(exit_code);
    let next_restart_at_millis = manifest.next_restart_at_millis.get_or_insert_with(|| {
        now_millis.saturating_add(restart_backoff_delay(manifest.restart_count).as_millis() as u64)
    });
    if now_millis < *next_restart_at_millis {
        synchronize_handle_status(manifest, SandboxStatus::Starting);
        return Ok(ContainerRestartDecision::WaitingForBackoff);
    }

    manifest.restart_count += 1;
    manifest.next_restart_at_millis = None;
    synchronize_handle_status(manifest, SandboxStatus::Starting);
    Ok(ContainerRestartDecision::RestartNow)
}
