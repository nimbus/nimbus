//! Host-managed attachment teardown and retained-authority ordering.

use super::*;

mod detached_release;
#[path = "host_teardown/state.rs"]
mod progress;
mod retained_detach;

#[cfg(test)]
pub(crate) use progress::{
    HostManagedAttachmentCheckpointTestProbe, HostManagedAttachmentTeardownCheckpoint,
};
pub(crate) use progress::{
    HostManagedAttachmentCommandInspection, HostManagedAttachmentCommandInspectionError,
    HostManagedAttachmentDetachPhase, HostManagedAttachmentReleasePhase,
    HostManagedAttachmentTeardownState,
};

impl OciAttachmentLifecycle<'_> {
    /// Detach a host-managed attachment after the backend proves that its
    /// runtime/creator and PEP prerequisites permit provider teardown.
    pub(super) fn detach_host_managed(
        &self,
        context: &OciAttachmentContext<'_>,
        mode: AttachmentTeardownMode,
        before_provider_detach: impl FnOnce(AttachmentAuxiliaryDisposition) -> Result<()>,
    ) -> AttachmentDetachResult {
        self.detach_host_managed_with(
            context,
            mode,
            &RealAttachmentHostEffects,
            before_provider_detach,
        )
    }

    pub(super) fn detach_host_managed_with(
        &self,
        context: &OciAttachmentContext<'_>,
        mode: AttachmentTeardownMode,
        host: &impl AttachmentHostEffects,
        before_provider_detach: impl FnOnce(AttachmentAuxiliaryDisposition) -> Result<()>,
    ) -> AttachmentDetachResult {
        self.detach_host_managed_observed_with(
            context,
            mode,
            host,
            &mut NoopAttachmentDetachPhaseObserver,
            before_provider_detach,
        )
    }

    pub(super) fn detach_host_managed_observed_with(
        &self,
        context: &OciAttachmentContext<'_>,
        mode: AttachmentTeardownMode,
        host: &impl AttachmentHostEffects,
        observer: &mut impl AttachmentDetachPhaseObserver,
        before_provider_detach: impl FnOnce(AttachmentAuxiliaryDisposition) -> Result<()>,
    ) -> AttachmentDetachResult {
        context
            .validate_backend_publication()
            .map_err(recovery::before_provider_detach_failure)?;
        if !context.publication.owns_netavark_bindings() {
            return Err(recovery::before_provider_detach_failure(
                SandboxError::OperationFailed {
                    message: format!(
                        "{} attachment {} cannot use host-managed detach for machine-forwarded \
                     publication",
                        context.provider_label, context.sandbox_id
                    ),
                },
            ));
        }
        authenticate_container_network_generation_for_cleanup(
            self.ipam,
            context.layout,
            context.config,
            context.sandbox_id,
        )
        .map_err(recovery::before_provider_detach_failure)?;
        let association = authority::authenticate_detach_association(
            self.attachments,
            self.allocator,
            context,
            mode,
        )
        .map_err(recovery::before_provider_detach_failure)?;
        let durable =
            state::OciAttachmentDurableState::compile(self.attachments, context, association)
                .map_err(recovery::before_provider_detach_failure)?;
        let durable_record = durable
            .inspect()
            .map_err(recovery::before_provider_detach_failure)?;
        let provider_observation = host.inspect_provider(self.ipam, context);
        if durable_record
            .as_ref()
            .is_some_and(|record| record.resource().phase().is_terminal())
        {
            let terminal = recovery::prepare_detach(
                &durable,
                durable_record.expect("terminal attachment record was just authenticated"),
                provider_observation,
            )
            .map_err(recovery::before_provider_detach_failure)?;
            debug_assert!(terminal.already_terminal);
            return Ok(());
        }
        if matches!(
            &provider_observation,
            recovery::AttachmentProviderObservation::Unknown { .. }
        ) {
            let Some(record) = durable_record else {
                return Err(recovery::before_provider_detach_failure(
                    SandboxError::OperationFailed {
                        message: format!(
                            "{} attachment {} has ambiguous provider evidence without durable \
                             attachment authority; refusing teardown",
                            context.provider_label, context.sandbox_id
                        ),
                    },
                ));
            };
            let rejection = match recovery::prepare_detach(&durable, record, provider_observation) {
                Ok(_) => SandboxError::OperationFailed {
                    message: format!(
                        "{} attachment {} unexpectedly authorized teardown from ambiguous \
                             provider evidence",
                        context.provider_label, context.sandbox_id
                    ),
                },
                Err(error) => error,
            };
            return Err(recovery::before_provider_detach_failure(rejection));
        }

        let mut errors = Vec::new();
        let mut detach_permitted = true;
        let published_batch_state = self.ports.classify_netavark_cleanup_batch(
            context.tenant_id,
            context.sandbox_id,
            context.bindings,
            context.leases,
            context.launch_claim,
        );
        let auxiliary_requests = context
            .auxiliary_listener
            .map(OciAttachmentAuxiliaryListener::request)
            .map(std::slice::from_ref)
            .unwrap_or_default();
        let auxiliary_batch_state = if mode.releases_authority()
            && let Some(claim) = context.launch_claim
        {
            self.ports
                .classify_launch_port_batch(auxiliary_requests, claim)
        } else {
            Ok(LaunchPortBatchState::ProviderOwned)
        };
        if let Err(error) = &published_batch_state {
            detach_permitted = false;
            errors.push(error.to_string());
        }
        if let Err(error) = &auxiliary_batch_state {
            detach_permitted = false;
            errors.push(error.to_string());
        }
        if mode == AttachmentTeardownMode::Restart
            && let Ok(state) = &published_batch_state
            && let Err(error) = recovery::validate_restart_publication_state(context, state)
        {
            detach_permitted = false;
            errors.push(error.to_string());
        }
        let auxiliary_disposition = match &auxiliary_batch_state {
            Ok(LaunchPortBatchState::ProviderOwned) => {
                AttachmentAuxiliaryDisposition::ProviderOwned
            }
            Ok(
                LaunchPortBatchState::NeverBound
                | LaunchPortBatchState::RestartRetained
                | LaunchPortBatchState::TerminalNoEffect,
            ) => AttachmentAuxiliaryDisposition::NoEffect,
            Ok(LaunchPortBatchState::NetavarkClaimed(_)) | Err(_) => {
                AttachmentAuxiliaryDisposition::Unknown
            }
        };

        if !detach_permitted {
            return Err(recovery::before_provider_detach_failure(
                recovery::detach_error(context, errors),
            ));
        }

        let durable_record = match durable_record {
            Some(record) => record,
            None => durable
                .reserve()
                .map_err(recovery::before_provider_detach_failure)?,
        };
        let durable_detach =
            recovery::prepare_detach(&durable, durable_record, provider_observation)
                .map_err(recovery::before_provider_detach_failure)?;
        debug_assert!(!durable_detach.already_terminal);
        let durable_record = durable_detach.record;
        let provider_was_absent = durable_detach.provider_absent;
        let remove_namespace_after_detach =
            !provider_was_absent || durable_detach.namespace_cleanup_required;
        observer.checkpoint(AttachmentDetachPhase::AttemptPrepared);
        let prepared_teardown = if provider_was_absent {
            None
        } else {
            match host.prepare_provider_teardown(self.ipam, context) {
                Ok(prepared) => Some(prepared),
                Err(error) => {
                    let _ = recovery::mark_cleanup_pending(&durable, &durable_record);
                    return Err(recovery::before_provider_detach_failure(error));
                }
            }
        };

        if let Err(error) = before_provider_detach(auxiliary_disposition) {
            let _ = recovery::mark_cleanup_pending(&durable, &durable_record);
            return Err(recovery::before_provider_detach_failure(error));
        }
        observer.checkpoint(AttachmentDetachPhase::BackendWithdrawn);

        if mode.releases_authority()
            && let Err(error) = quarantine_network_segment_hold(
                self.allocator,
                context.tenant_id,
                &context.config.attachment_id,
                &context.config.reservation_claim,
            )
        {
            detach_permitted = false;
            errors.push(error.to_string());
        }
        if mode.releases_authority() && detach_permitted {
            observer.checkpoint(AttachmentDetachPhase::SegmentQuarantined);
        }

        let mut cleanup = None;
        let mut recoveries = None;
        if matches!(
            &published_batch_state,
            Ok(LaunchPortBatchState::ProviderOwned)
        ) {
            match self.ports.begin_netavark_cleanup(
                self.lifetimes,
                context.tenant_id,
                context.sandbox_id,
                context.bindings,
                context.leases,
            ) {
                Ok(authority) => cleanup = authority,
                Err(error) => {
                    detach_permitted = false;
                    errors.push(error.to_string());
                }
            }
        } else if let Ok(LaunchPortBatchState::NetavarkClaimed(claims)) = &published_batch_state {
            match self.ports.recover_netavark_claims_after_owner_death(
                context.tenant_id,
                context.sandbox_id,
                context.bindings,
                context.leases,
                claims,
            ) {
                Ok(authority) => recoveries = Some(authority),
                Err(error) => {
                    detach_permitted = false;
                    errors.push(error.to_string());
                }
            }
        }

        if !detach_permitted {
            if let Err(error) = self.ports.retain_ambiguous_netavark_cleanup(
                self.lifetimes,
                context.tenant_id,
                context.sandbox_id,
                cleanup.take(),
            ) {
                errors.push(error.to_string());
            }
            return Err(recovery::before_provider_detach_failure(
                recovery::detach_error(context, errors),
            ));
        }
        observer.checkpoint(AttachmentDetachPhase::ListenerCleanupPrepared);

        let mut provider_detached = provider_was_absent;
        if let Some(prepared_teardown) = prepared_teardown {
            provider_detached = match host.teardown_provider(self.ipam, context, prepared_teardown)
            {
                Ok(()) => true,
                Err(error) => {
                    errors.push(error.to_string());
                    false
                }
            };
            if provider_detached {
                observer.checkpoint(AttachmentDetachPhase::ProviderDetached);
            }
        }
        if provider_detached
            && remove_namespace_after_detach
            && let Err(error) = host.remove_namespace(context)
        {
            provider_detached = false;
            errors.push(error.to_string());
        } else if provider_detached && remove_namespace_after_detach {
            observer.checkpoint(AttachmentDetachPhase::NamespaceRemoved);
        }

        if provider_detached
            && matches!(
                &published_batch_state,
                Ok(LaunchPortBatchState::ProviderOwned)
            )
        {
            if let Err(error) = self.ports.complete_netavark_cleanup(
                context.leases,
                cleanup.as_ref(),
                mode.releases_authority(),
            ) {
                provider_detached = false;
                errors.push(error.to_string());
            } else {
                cleanup = None;
            }
        }

        if provider_detached
            && let Some(recoveries) = recoveries.take()
            && let Err(error) = if mode.releases_authority() {
                self.ports
                    .release_recovered_netavark_bindings(context.leases, &recoveries)
            } else {
                self.ports
                    .prepare_recovered_netavark_claims_for_rebind(context.leases, &recoveries)
            }
        {
            provider_detached = false;
            errors.push(error.to_string());
        }
        if provider_detached {
            observer.checkpoint(AttachmentDetachPhase::ListenersSettled);
        }

        if !provider_detached {
            let _ = recovery::mark_cleanup_pending(&durable, &durable_record);
            if let Err(error) = self.ports.retain_ambiguous_netavark_cleanup(
                self.lifetimes,
                context.tenant_id,
                context.sandbox_id,
                cleanup.take(),
            ) {
                errors.push(error.to_string());
            }
            return Err(recovery::cleanup_pending_failure(recovery::detach_error(
                context, errors,
            )));
        }

        if mode.releases_authority() {
            self.release_terminal_authority(
                context,
                published_batch_state,
                auxiliary_batch_state,
                auxiliary_requests,
                &mut errors,
                observer,
            );
        }

        if errors.is_empty() {
            recovery::finish_detach(&durable, &durable_record, mode)
                .map(|_| observer.checkpoint(AttachmentDetachPhase::AttachmentTerminal))
                .map_err(recovery::cleanup_pending_failure)
        } else {
            let _ = recovery::mark_cleanup_pending(&durable, &durable_record);
            Err(recovery::cleanup_pending_failure(recovery::detach_error(
                context, errors,
            )))
        }
    }
}
