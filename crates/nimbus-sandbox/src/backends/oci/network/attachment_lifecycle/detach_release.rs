//! Terminal release ordering after exact provider and namespace absence.

use super::*;

impl OciAttachmentLifecycle<'_> {
    pub(super) fn release_terminal_authority(
        &self,
        context: &OciAttachmentContext<'_>,
        published_batch_state: Result<LaunchPortBatchState>,
        auxiliary_batch_state: Result<LaunchPortBatchState>,
        auxiliary_requests: &[PortLeaseRequest],
        errors: &mut Vec<String>,
        observer: &mut impl AttachmentDetachPhaseObserver,
    ) {
        match published_batch_state {
            Ok(LaunchPortBatchState::NeverBound) => {
                if let Some(claim) = context.launch_claim
                    && let Err(error) = self
                        .ports
                        .release_never_bound_requests(context.leases, claim)
                {
                    errors.push(error.to_string());
                    return;
                }
            }
            Ok(LaunchPortBatchState::RestartRetained) => {
                if let Err(error) = self.ports.release_restart_retained_bindings(
                    context.tenant_id,
                    context.sandbox_id,
                    context.bindings,
                    context.leases,
                ) {
                    errors.push(error.to_string());
                    return;
                }
            }
            Ok(
                LaunchPortBatchState::NetavarkClaimed(_)
                | LaunchPortBatchState::ProviderOwned
                | LaunchPortBatchState::TerminalNoEffect,
            ) => {}
            Err(_) => return,
        }

        if matches!(auxiliary_batch_state, Ok(LaunchPortBatchState::NeverBound))
            && let Some(claim) = context.launch_claim
            && let Err(error) = self
                .ports
                .release_never_bound_requests(auxiliary_requests, claim)
        {
            errors.push(error.to_string());
            return;
        }

        if let Err(error) = deallocate_container_ips_after_confirmed_detach(
            self.ipam,
            context.layout,
            context.sandbox_id,
            &context.config.attachment_id,
            &context.config.reservation_claim,
            context.config.provider_kind(),
        ) {
            errors.push(error.to_string());
            return;
        }
        observer.checkpoint(AttachmentDetachPhase::IpamReleased);
        let release_errors = release_network_segment_hold(
            self.allocator,
            context.tenant_id,
            &context.config.attachment_id,
            &context.config.reservation_claim,
        );
        if release_errors.is_empty() {
            observer.checkpoint(AttachmentDetachPhase::SegmentReleased);
        }
        errors.extend(release_errors.into_iter().map(|error| error.to_string()));
    }
}
