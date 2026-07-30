//! Portable desired-state compilation and durable attachment mutations.

use nimbus_network::{
    DurableNetworkAttachmentState, LocalNetworkAttachmentAuthority, NetworkAttachmentId,
    NetworkAttachmentStateError, NetworkLeaseEpoch, NetworkPlan, NetworkPlanContentDigest,
    NetworkPlanId, NetworkProviderHandle, NetworkProviderId, NetworkResourceGeneration,
    NetworkResourcePhase, NetworkResourceVersion, NetworkStateTransition,
    NetworkTransitionEvidence,
};

use super::{AttachmentBackendKind, OciAttachmentContext};
use crate::backends::capabilities::{
    SandboxAttachmentRegistrationKind, host_managed_attachment_provider_id,
    host_managed_attachment_requirements,
};
use crate::backends::oci::network::default_network_attachment_id;
use crate::error::{Result, SandboxError};

const TRANSITIONAL_ATTACHMENT_GENERATION: NetworkResourceGeneration =
    NetworkResourceGeneration::new(1);
const TRANSITIONAL_ATTACHMENT_LEASE_EPOCH: NetworkLeaseEpoch = NetworkLeaseEpoch::new(1);
const CONTENT_DOMAIN: &[u8] = b"nimbus.sandbox.oci.attachment-plan.v1\0";

pub(super) struct OciAttachmentDurableState<'a> {
    authority: &'a LocalNetworkAttachmentAuthority,
    tenant_id: &'a nimbus_core::TenantId,
    plan: NetworkPlan,
    attachment_id: NetworkAttachmentId,
    provider_id: NetworkProviderId,
    stable_handle: NetworkProviderHandle,
}

impl<'a> OciAttachmentDurableState<'a> {
    pub(super) fn compile(
        authority: Option<&'a LocalNetworkAttachmentAuthority>,
        context: &'a OciAttachmentContext<'_>,
    ) -> Result<Self> {
        let authority = authority.ok_or_else(|| SandboxError::OperationFailed {
            message: format!(
                "{} attachment {} has no valid manager-derived durable attachment authority",
                context.provider_label, context.sandbox_id
            ),
        })?;
        let attachment_id = default_network_attachment_id(context.sandbox_id);
        let registration_kind = match context.backend {
            AttachmentBackendKind::Container => SandboxAttachmentRegistrationKind::Container,
            AttachmentBackendKind::Krun => SandboxAttachmentRegistrationKind::Krun,
        };
        let provider_id = host_managed_attachment_provider_id(registration_kind);
        let mut content = Vec::with_capacity(
            CONTENT_DOMAIN.len() + attachment_id.as_str().len() + context.provider_label.len() + 16,
        );
        content.extend_from_slice(CONTENT_DOMAIN);
        push_framed(&mut content, attachment_id.as_str().as_bytes());
        push_framed(&mut content, context.provider_label.as_bytes());
        let plan = NetworkPlan::new(
            NetworkPlanId::for_tenant_workload_plan(context.tenant_id, context.sandbox_id.as_str()),
            TRANSITIONAL_ATTACHMENT_GENERATION,
            NetworkPlanContentDigest::sha256(content),
            host_managed_attachment_requirements(registration_kind),
        );
        let stable_handle =
            stable_attachment_handle(provider_id.clone(), plan.plan_id(), &attachment_id).map_err(
                |error| SandboxError::OperationFailed {
                    message: format!(
                        "{} attachment {} could not construct its stable provider handle: {error}",
                        context.provider_label, context.sandbox_id
                    ),
                },
            )?;
        Ok(Self {
            authority,
            tenant_id: context.tenant_id,
            plan,
            attachment_id,
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
                TRANSITIONAL_ATTACHMENT_LEASE_EPOCH,
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
        let expected = NetworkResourceVersion::for_plan(
            &self.plan,
            self.attachment_id.clone().into(),
            TRANSITIONAL_ATTACHMENT_LEASE_EPOCH,
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

fn stable_attachment_handle(
    provider_id: NetworkProviderId,
    plan_id: &NetworkPlanId,
    attachment_id: &NetworkAttachmentId,
) -> std::result::Result<NetworkProviderHandle, nimbus_network::NetworkProviderHandleError> {
    NetworkProviderHandle::new(provider_id, format!("attachment:{plan_id}:{attachment_id}"))
}

fn push_framed(output: &mut Vec<u8>, value: &[u8]) {
    output.extend_from_slice(&(value.len() as u64).to_be_bytes());
    output.extend_from_slice(value);
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

    #[test]
    fn stable_provider_handle_is_tenant_qualified_for_equal_local_attachment_ids() {
        let tenant_a = TenantId::new("tenant-handle-a").expect("tenant A should validate");
        let tenant_b = TenantId::new("tenant-handle-b").expect("tenant B should validate");
        let attachment_id =
            NetworkAttachmentId::for_workload_attachment("same-local-workload", "default");
        let provider_id = NetworkProviderId::for_registration_key("nimbus.test.host-managed");
        let plan_a = NetworkPlanId::for_tenant_workload_plan(&tenant_a, "same-local-workload");
        let plan_b = NetworkPlanId::for_tenant_workload_plan(&tenant_b, "same-local-workload");

        let handle_a = stable_attachment_handle(provider_id.clone(), &plan_a, &attachment_id)
            .expect("tenant A handle should validate");
        let handle_b = stable_attachment_handle(provider_id, &plan_b, &attachment_id)
            .expect("tenant B handle should validate");

        assert_ne!(
            handle_a.expose_to_provider(),
            handle_b.expose_to_provider(),
            "equal local workload and attachment names in different tenants need distinct stable \
             provider handles"
        );
    }
}
