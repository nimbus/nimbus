//! Portable desired-state compilation and durable attachment mutations.

use nimbus_network::{
    DurableNetworkAttachmentState, LocalNetworkAttachmentAuthority, NetworkAttachmentId,
    NetworkAttachmentSegmentAssociation, NetworkAttachmentStateError, NetworkPlan,
    NetworkProviderHandle, NetworkProviderId, NetworkResourcePhase, NetworkResourceVersion,
    NetworkStateTransition, NetworkTransitionEvidence,
};

use super::OciAttachmentContext;
use super::plan::{
    oci_attachment_plan, oci_attachment_provider_handle, oci_attachment_registration_kind,
};
use crate::backends::capabilities::host_managed_attachment_provider_id;
use crate::backends::oci::network::default_network_attachment_id;
use crate::error::{Result, SandboxError};

pub(super) struct OciAttachmentDurableState<'a> {
    authority: &'a LocalNetworkAttachmentAuthority,
    tenant_id: &'a nimbus_core::TenantId,
    plan: NetworkPlan,
    attachment_id: NetworkAttachmentId,
    association: NetworkAttachmentSegmentAssociation,
    provider_id: NetworkProviderId,
    stable_handle: NetworkProviderHandle,
}

impl<'a> OciAttachmentDurableState<'a> {
    pub(super) fn compile(
        authority: Option<&'a LocalNetworkAttachmentAuthority>,
        context: &'a OciAttachmentContext<'_>,
        association: NetworkAttachmentSegmentAssociation,
    ) -> Result<Self> {
        let authority = authority.ok_or_else(|| SandboxError::OperationFailed {
            message: format!(
                "{} attachment {} has no valid manager-derived durable attachment authority",
                context.provider_label, context.sandbox_id
            ),
        })?;
        let attachment_id = default_network_attachment_id(context.sandbox_id);
        let registration_kind = oci_attachment_registration_kind(context.backend);
        let provider_id = host_managed_attachment_provider_id(registration_kind);
        let plan = oci_attachment_plan(context.tenant_id, context.sandbox_id, context.backend);
        let stable_handle =
            oci_attachment_provider_handle(context.tenant_id, context.sandbox_id, context.backend)
                .map_err(|error| SandboxError::OperationFailed {
                    message: format!(
                        "{} attachment {} could not construct its stable provider handle: {error}",
                        context.provider_label, context.sandbox_id
                    ),
                })?;
        Ok(Self {
            authority,
            tenant_id: context.tenant_id,
            plan,
            attachment_id,
            association,
            provider_id,
            stable_handle,
        })
    }

    pub(super) fn reserve(&self) -> Result<DurableNetworkAttachmentState> {
        self.authority
            .reserve(
                self.tenant_id,
                self.provider_id.clone(),
                &self.plan,
                self.attachment_id.clone(),
                self.association.clone(),
            )
            .map_err(attachment_state_error)
    }

    pub(super) fn inspect(&self) -> Result<Option<DurableNetworkAttachmentState>> {
        let Some(record) = self
            .authority
            .get(self.tenant_id, &self.attachment_id)
            .map_err(attachment_state_error)?
        else {
            return Ok(None);
        };
        if record.selected_provider_id() != &self.provider_id {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "durable attachment authority selected provider {} for {}, not {}",
                    record.selected_provider_id(),
                    self.attachment_id,
                    self.provider_id
                ),
            });
        }
        if record.association() != &self.association {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "durable attachment authority rejected the exact allocator association for {}",
                    self.attachment_id
                ),
            });
        }
        let expected = NetworkResourceVersion::for_plan(
            &self.plan,
            self.attachment_id.clone().into(),
            self.association.lease_epoch(),
        );
        record
            .resource()
            .authenticate_version(&expected)
            .map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "durable attachment authority rejected the expected version for {}: {error}",
                    self.attachment_id
                ),
            })?;
        Ok(Some(record))
    }

    pub(super) fn authenticate_stable_handle(
        &self,
        record: &DurableNetworkAttachmentState,
    ) -> Result<()> {
        if record.resource().provider_handle() != Some(&self.stable_handle) {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "durable attachment {} does not carry its exact stable provider handle",
                    self.attachment_id
                ),
            });
        }
        Ok(())
    }

    pub(super) fn transition(
        &self,
        record: &DurableNetworkAttachmentState,
        target: NetworkResourcePhase,
        evidence: NetworkTransitionEvidence,
    ) -> Result<DurableNetworkAttachmentState> {
        self.authority
            .apply_transition(
                self.tenant_id,
                &NetworkStateTransition::new(record.resource().version().clone(), target, evidence),
            )
            .map(|(_, record)| record)
            .map_err(attachment_state_error)
    }

    pub(super) fn record_stable_handle(
        &self,
        record: &DurableNetworkAttachmentState,
    ) -> Result<DurableNetworkAttachmentState> {
        self.authority
            .record_provider_handle(
                self.tenant_id,
                record.resource().version(),
                self.stable_handle.clone(),
            )
            .map(|(_, record)| record)
            .map_err(attachment_state_error)
    }
}

fn attachment_state_error(error: NetworkAttachmentStateError) -> SandboxError {
    SandboxError::OperationFailed {
        message: format!("durable attachment authority rejected operation: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use nimbus_core::TenantId;

    use super::*;
    use crate::backends::oci::network::attachment_lifecycle::AttachmentBackendKind;
    use crate::instance::SandboxId;

    #[test]
    fn stable_provider_handle_is_tenant_qualified_for_equal_local_attachment_ids() {
        let tenant_a = TenantId::new("tenant-handle-a").expect("tenant A should validate");
        let tenant_b = TenantId::new("tenant-handle-b").expect("tenant B should validate");
        let sandbox_id = SandboxId::new("same-local-workload");
        let handle_a = oci_attachment_provider_handle(
            &tenant_a,
            &sandbox_id,
            AttachmentBackendKind::Container,
        )
        .expect("tenant A handle should validate");
        let handle_b = oci_attachment_provider_handle(
            &tenant_b,
            &sandbox_id,
            AttachmentBackendKind::Container,
        )
        .expect("tenant B handle should validate");

        assert_ne!(
            handle_a.expose_to_provider(),
            handle_b.expose_to_provider(),
            "equal local workload and attachment names in different tenants need distinct stable \
             provider handles"
        );
    }
}
