//! Active-attachment process-lifetime reconciliation.
//!
//! The durable attachment and provider effect may survive a process owner.
//! This module restores only the exact process-local listener lifetime and pin
//! composition, without recreating Netavark or changing attachment identity.

use super::*;

pub(super) struct AttachmentPublicationRecovery {
    pub(super) record: nimbus_network::DurableNetworkAttachmentState,
    pub(super) assigned_ips: Vec<Ipv4Addr>,
}

pub(super) struct PresentAttachmentRecovery<'a> {
    pub(super) record: nimbus_network::DurableNetworkAttachmentState,
    pub(super) provider_observation: recovery::AttachmentProviderObservation,
    pub(super) attach_authority: AttachmentAttachAuthority<'a>,
}

impl OciAttachmentLifecycle<'_> {
    /// Read-only authentication before fencing cleanup-only or ambiguous
    /// provider evidence. This accepts every exact uniform listener phase but
    /// rejects substituted identities before changing portable attachment
    /// bytes.
    pub(super) fn authenticate_attachment_recovery_authority(
        &self,
        context: &OciAttachmentContext<'_>,
        attach_authority: AttachmentAttachAuthority<'_>,
    ) -> Result<()> {
        self.authenticate_active_attach_authority(context, attach_authority)?;
        if !context.publication.owns_netavark_bindings() {
            return self.ports.require_binding_leases(
                context.tenant_id,
                context.sandbox_id,
                context.bindings,
                context.leases,
            );
        }
        self.ports.classify_netavark_cleanup_batch(
            context.tenant_id,
            context.sandbox_id,
            context.bindings,
            context.leases,
            context.launch_claim,
        )?;
        if let Some(auxiliary) = context.auxiliary_listener {
            self.ports.require_internal_listener_authority(
                context.tenant_id,
                context.sandbox_id,
                auxiliary.bind_addr()?,
                auxiliary.request(),
            )?;
        }
        Ok(())
    }

    /// Authenticate durable authority for a provider-present attachment before
    /// any portable phase or process-lifetime transition.
    pub(super) fn authenticate_present_attachment_authority(
        &self,
        context: &OciAttachmentContext<'_>,
        attach_authority: AttachmentAttachAuthority<'_>,
    ) -> Result<()> {
        self.authenticate_active_attach_authority(context, attach_authority)?;
        if !context.publication.owns_netavark_bindings() {
            return self.ports.require_binding_leases(
                context.tenant_id,
                context.sandbox_id,
                context.bindings,
                context.leases,
            );
        }
        let state = self.ports.classify_netavark_cleanup_batch(
            context.tenant_id,
            context.sandbox_id,
            context.bindings,
            context.leases,
            context.launch_claim,
        )?;
        if matches!(
            state,
            LaunchPortBatchState::ProviderOwned | LaunchPortBatchState::NetavarkClaimed(_)
        ) || (context.leases.is_empty()
            && matches!(
                state,
                LaunchPortBatchState::NeverBound | LaunchPortBatchState::TerminalNoEffect
            ))
        {
            if let Some(auxiliary) = context.auxiliary_listener {
                self.ports.require_internal_listener_authority(
                    context.tenant_id,
                    context.sandbox_id,
                    auxiliary.bind_addr()?,
                    auxiliary.request(),
                )?;
            }
            Ok(())
        } else {
            Err(SandboxError::OperationFailed {
                message: format!(
                    "{} attachment {} provider-present publication has incompatible durable \
                     listener authority {state:?}",
                    context.provider_label, context.sandbox_id
                ),
            })
        }
    }

    /// Adopt one exact provider-present observation without allowing the
    /// composition root to grow a second recovery switchboard.
    pub(super) fn recover_present_attachment(
        &self,
        context: &OciAttachmentContext<'_>,
        durable: &state::OciAttachmentDurableState<'_>,
        recovery: PresentAttachmentRecovery<'_>,
        host: &impl AttachmentHostEffects,
        observer: &mut impl AttachmentPhaseObserver,
        after_provider_setup: impl FnOnce(&[Ipv4Addr]) -> Result<()>,
    ) -> Result<Vec<Ipv4Addr>> {
        let PresentAttachmentRecovery {
            record,
            provider_observation,
            attach_authority,
        } = recovery;
        if !context.publication.owns_netavark_bindings()
            && record.resource().phase() == nimbus_network::NetworkResourcePhase::Active
        {
            self.authenticate_active_attach_authority(context, attach_authority)?;
            observer.checkpoint(AttachmentAttachPhase::AuthorityAuthenticated)?;
            return match recovery::prepare_attach(durable, record, provider_observation)? {
                recovery::AttachmentAttachRecovery::AlreadyActive { assigned_ips } => self
                    .reconcile_active_attachment(
                        context,
                        observer,
                        assigned_ips,
                        after_provider_setup,
                    ),
                _ => Err(SandboxError::OperationFailed {
                    message: format!(
                        "{} machine-forwarded attachment {} has inconsistent Active provider \
                         evidence",
                        context.provider_label, context.sandbox_id
                    ),
                }),
            };
        }
        self.authenticate_present_attachment_authority(context, attach_authority)?;
        observer.checkpoint(AttachmentAttachPhase::AuthorityAuthenticated)?;
        match recovery::prepare_attach(durable, record, provider_observation)? {
            recovery::AttachmentAttachRecovery::ResumePublication {
                record,
                assigned_ips,
            } => {
                observer.checkpoint(AttachmentAttachPhase::ProviderAttemptAuthenticated)?;
                self.resume_attachment_publication(
                    context,
                    durable,
                    AttachmentPublicationRecovery {
                        record,
                        assigned_ips,
                    },
                    host,
                    observer,
                    after_provider_setup,
                )
            }
            recovery::AttachmentAttachRecovery::AlreadyActive { assigned_ips } => self
                .reconcile_active_attachment(context, observer, assigned_ips, after_provider_setup),
            recovery::AttachmentAttachRecovery::Create { .. } => {
                Err(SandboxError::OperationFailed {
                    message: format!(
                        "{} attachment {} cannot recreate an authenticated present provider",
                        context.provider_label, context.sandbox_id
                    ),
                })
            }
        }
    }

    /// Resume an exact provider-present publication without recreating the
    /// provider or re-entering the never-bound launch path.
    pub(super) fn resume_attachment_publication(
        &self,
        context: &OciAttachmentContext<'_>,
        durable: &state::OciAttachmentDurableState<'_>,
        recovery: AttachmentPublicationRecovery,
        host: &impl AttachmentHostEffects,
        observer: &mut impl AttachmentPhaseObserver,
        after_provider_setup: impl FnOnce(&[Ipv4Addr]) -> Result<()>,
    ) -> Result<Vec<Ipv4Addr>> {
        let AttachmentPublicationRecovery {
            record: durable_record,
            assigned_ips,
        } = recovery;
        let durable_record = recovery::mark_publishing(durable, &durable_record)?;
        observer.checkpoint(AttachmentAttachPhase::Publishing)?;
        match self.reconcile_active_attachment(
            context,
            observer,
            assigned_ips,
            after_provider_setup,
        ) {
            Ok(assigned_ips) => {
                let active_record = match recovery::mark_active(durable, &durable_record) {
                    Ok(record) => record,
                    Err(primary) => {
                        let _ = recovery::mark_cleanup_pending(durable, &durable_record);
                        return Err(self.compensate_registered_failure(
                            context,
                            host,
                            "durable active publication recovery",
                            primary,
                        ));
                    }
                };
                if let Err(primary) = observer.checkpoint(AttachmentAttachPhase::Active) {
                    let _ = recovery::mark_cleanup_pending(durable, &active_record);
                    return Err(self.compensate_registered_failure(
                        context,
                        host,
                        "active publication recovery checkpoint",
                        primary,
                    ));
                }
                Ok(assigned_ips)
            }
            Err(primary) => {
                let _ = recovery::mark_cleanup_pending(durable, &durable_record);
                Err(self.compensate_registered_failure(
                    context,
                    host,
                    "publication recovery",
                    primary,
                ))
            }
        }
    }

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
        if context.publication.owns_netavark_bindings() {
            self.ports
                .reconcile_active_netavark_bindings_with_lifetimes(
                    self.lifetimes,
                    context.tenant_id,
                    context.sandbox_id,
                    context.bindings,
                    context.leases,
                )?;
        }
        observer.checkpoint(AttachmentAttachPhase::ListenerBindingsActive)?;
        after_provider_setup(&assigned_ips)?;
        observer.checkpoint(AttachmentAttachPhase::BackendPublicationComplete)?;
        observer.checkpoint(AttachmentAttachPhase::LifetimeRegistered)?;
        self.allocator
            .acquire(context.tenant_id, &context.config.attachment_id)?;
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
