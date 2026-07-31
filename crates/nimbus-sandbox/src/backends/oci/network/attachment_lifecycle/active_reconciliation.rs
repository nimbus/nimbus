//! Active-attachment process-lifetime reconciliation.
//!
//! The durable attachment and provider effect may survive a process owner.
//! This module restores only the exact process-local listener lifetime and pin
//! composition, without recreating Netavark or changing attachment identity.

use super::*;

impl OciAttachmentLifecycle<'_> {
    pub(super) fn authenticate_active_attach_authority(
        &self,
        context: &OciAttachmentContext<'_>,
        attach_authority: AttachmentAttachAuthority<'_>,
    ) -> Result<()> {
        match attach_authority {
            AttachmentAttachAuthority::FreshLaunch(claim) => {
                let Some(context_claim) = context.launch_claim else {
                    return Err(SandboxError::OperationFailed {
                        message: format!(
                            "{} Active attachment {} is missing its fresh launch reservation claim",
                            context.provider_label, context.sandbox_id
                        ),
                    });
                };
                if context_claim != claim || &context.config.reservation_claim != claim {
                    return Err(SandboxError::OperationFailed {
                        message: format!(
                            "{} Active attachment {} launch reservation claim does not match its \
                             exact attachment generation",
                            context.provider_label, context.sandbox_id
                        ),
                    });
                }
            }
            AttachmentAttachAuthority::RestartRetained => {
                if context.launch_claim.is_some() {
                    return Err(SandboxError::OperationFailed {
                        message: format!(
                            "{} Active attachment {} restart still carries a launch reservation \
                             claim",
                            context.provider_label, context.sandbox_id
                        ),
                    });
                }
            }
        }
        Ok(())
    }

    pub(super) fn reconcile_active_attachment(
        &self,
        context: &OciAttachmentContext<'_>,
        observer: &mut impl AttachmentPhaseObserver,
        assigned_ips: Vec<Ipv4Addr>,
        after_provider_setup: impl FnOnce(&[Ipv4Addr]) -> Result<()>,
    ) -> Result<Vec<Ipv4Addr>> {
        if !context.publication.owns_netavark_bindings() {
            return Ok(assigned_ips);
        }
        self.ports
            .reconcile_active_netavark_bindings_with_lifetimes(
                self.lifetimes,
                context.tenant_id,
                context.sandbox_id,
                context.bindings,
                context.leases,
            )?;
        observer.checkpoint(AttachmentAttachPhase::ListenerBindingsActive)?;
        after_provider_setup(&assigned_ips)?;
        observer.checkpoint(AttachmentAttachPhase::BackendPublicationComplete)?;
        observer.checkpoint(AttachmentAttachPhase::LifetimeRegistered)?;
        self.allocator.acquire(
            context.tenant_id,
            &default_network_attachment_id(context.sandbox_id),
        )?;
        observer.checkpoint(AttachmentAttachPhase::AttachmentConfirmed)?;
        Ok(assigned_ips)
    }

    pub(super) fn take_registered_lifetime(
        &self,
        context: &OciAttachmentContext<'_>,
        checkpoint: &str,
    ) -> Result<Option<OciPortBindLifetimeBatch>> {
        if !context.publication.owns_netavark_bindings() {
            return Ok(None);
        }
        if context.leases.is_empty() {
            return Ok(None);
        }
        self.lifetimes
            .take(context.tenant_id, context.sandbox_id)?
            .map(Some)
            .ok_or_else(|| SandboxError::OperationFailed {
                message: format!(
                    "{} attachment {checkpoint} for {} lost its exact live Netavark lifetime \
                     batch",
                    context.provider_label, context.sandbox_id
                ),
            })
    }
}
