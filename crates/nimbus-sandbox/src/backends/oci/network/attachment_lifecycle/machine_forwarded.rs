//! Container-only machine-forwarded detach composition.
//!
//! Machine proxy and PEP effects remain container-owned callbacks. This module
//! keeps Netavark, namespace, IPAM, segment, and portable attachment state on
//! the same shared lifecycle used by host-managed publication.

use super::host::{AttachmentHostEffects, RealAttachmentHostEffects};
use super::{
    AttachmentDetachResult, AttachmentTeardownMode, OciAttachmentContext, OciAttachmentLifecycle,
    authority, recovery, state,
};
use crate::backends::oci::network::{
    authenticate_container_network_generation_for_cleanup,
    deallocate_container_ips_after_confirmed_detach, quarantine_network_segment_hold,
    release_network_segment_hold,
};
use crate::error::{Result, SandboxError};

impl OciAttachmentLifecycle<'_> {
    pub(super) fn detach_machine_forwarded<T>(
        &self,
        context: &OciAttachmentContext<'_>,
        mode: AttachmentTeardownMode,
        before_provider_detach: impl FnOnce() -> Result<T>,
        after_provider_detach: impl FnOnce(T) -> Result<()>,
    ) -> AttachmentDetachResult {
        self.detach_machine_forwarded_with(
            context,
            mode,
            &RealAttachmentHostEffects,
            before_provider_detach,
            after_provider_detach,
        )
    }

    pub(super) fn detach_machine_forwarded_with<T>(
        &self,
        context: &OciAttachmentContext<'_>,
        mode: AttachmentTeardownMode,
        host: &impl AttachmentHostEffects,
        before_provider_detach: impl FnOnce() -> Result<T>,
        after_provider_detach: impl FnOnce(T) -> Result<()>,
    ) -> AttachmentDetachResult {
        context
            .validate_backend_publication()
            .map_err(recovery::before_provider_detach_failure)?;
        if context.publication.owns_netavark_bindings() {
            return Err(recovery::before_provider_detach_failure(
                SandboxError::OperationFailed {
                    message: format!(
                        "{} attachment {} cannot use machine-forwarded detach for host-managed \
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
                Err(error) => error,
                Ok(_) => SandboxError::OperationFailed {
                    message: format!(
                        "{} attachment {} unexpectedly authorized teardown from ambiguous \
                         provider evidence",
                        context.provider_label, context.sandbox_id
                    ),
                },
            };
            return Err(recovery::before_provider_detach_failure(rejection));
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
        let remove_namespace_after_detach =
            !durable_detach.provider_absent || durable_detach.namespace_cleanup_required;
        let prepared_teardown = if durable_detach.provider_absent {
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

        let publication = match before_provider_detach() {
            Ok(publication) => publication,
            Err(error) => {
                let _ = recovery::mark_cleanup_pending(&durable, &durable_record);
                return Err(recovery::before_provider_detach_failure(error));
            }
        };

        if mode.releases_authority() {
            quarantine_network_segment_hold(
                self.allocator,
                context.tenant_id,
                context.sandbox_id,
                &context.config.reservation_claim,
            )
            .map_err(recovery::before_provider_detach_failure)?;
        }

        let provider_detach = if let Some(prepared_teardown) = prepared_teardown {
            host.teardown_provider(self.ipam, context, prepared_teardown)
        } else {
            Ok(())
        };
        let provider_detach = provider_detach.and_then(|()| {
            if remove_namespace_after_detach {
                host.remove_namespace(context)
            } else {
                Ok(())
            }
        });
        if let Err(error) = provider_detach {
            let _ = recovery::mark_cleanup_pending(&durable, &durable_record);
            return Err(recovery::cleanup_pending_failure(error));
        }
        if let Err(error) = after_provider_detach(publication) {
            let _ = recovery::mark_cleanup_pending(&durable, &durable_record);
            return Err(recovery::cleanup_pending_failure(error));
        }

        if mode.releases_authority() {
            let mut errors = Vec::new();
            if let Err(error) = deallocate_container_ips_after_confirmed_detach(
                self.ipam,
                context.layout,
                context.sandbox_id,
                &context.config.reservation_claim,
                context.config.provider_kind(),
            ) {
                errors.push(error.to_string());
            } else {
                errors.extend(
                    release_network_segment_hold(
                        self.allocator,
                        context.tenant_id,
                        context.sandbox_id,
                        &context.config.reservation_claim,
                    )
                    .into_iter()
                    .map(|error| error.to_string()),
                );
            }
            if !errors.is_empty() {
                let _ = recovery::mark_cleanup_pending(&durable, &durable_record);
                return Err(recovery::cleanup_pending_failure(recovery::detach_error(
                    context, errors,
                )));
            }
        }

        recovery::finish_detach(&durable, &durable_record, mode)
            .map(|_| ())
            .map_err(recovery::cleanup_pending_failure)
    }
}
