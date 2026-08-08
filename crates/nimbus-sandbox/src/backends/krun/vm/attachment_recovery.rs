//! Claim-fenced recovery of the krun attachment-adoption crash window.
//!
//! The portable allocator reports only its exact durable reservation state.
//! Krun composes that observation with its own launch intent; it never infers
//! adoption from an error string or retries an ambiguous provider effect.

use nimbus_network::NetworkAttachmentReservationState;

use super::readiness::synchronize_handle_status;
use super::*;

impl KrunSandboxBackend {
    pub(super) fn stop_adopting_launch(&self, manifest: &mut KrunSandboxManifest) -> Result<()> {
        let reservation_claim = manifest.require_reserved_claim()?.clone();
        manifest.shutdown_requested = true;
        manifest.next_restart_at_millis = None;
        synchronize_handle_status(manifest, SandboxStatus::Stopping);
        self.persist_effect_barrier(manifest, "adopting krun stop intent")?;

        let attachment_id = manifest.require_network_config()?.attachment_id.clone();
        let observed = self.segment_allocator.inspect_attachment_reservation(
            &manifest.spec.tenant_id,
            &attachment_id,
            &reservation_claim,
        )?;
        match observed.state() {
            NetworkAttachmentReservationState::Absent
            | NetworkAttachmentReservationState::Reserved
            | NetworkAttachmentReservationState::ReservationCleanupPending => {
                // No adopted hold exists. Exact claim-authenticated
                // compensation is idempotent across a partially completed
                // reservation cleanup.
                self.stop_reserved_launch(manifest)
            }
            NetworkAttachmentReservationState::Adopted
            | NetworkAttachmentReservationState::ProviderCleanupPending => {
                // Allocator adoption committed before the manifest
                // acknowledgement. Promote the exact same claim before any
                // provider-aware cleanup so a fresh process can never fall
                // back to never-realized compensation.
                manifest.launch_authority = KrunLaunchAuthority::Adopted { reservation_claim };
                manifest.provider_failure_cleanup = KrunProviderFailureCleanupState::Requested;
                self.persist_effect_barrier(
                    manifest,
                    "krun recovered adopted attachment authority",
                )?;
                self.resume_provider_failure_cleanup(manifest)
            }
        }
    }
}
