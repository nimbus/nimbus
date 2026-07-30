//! Pure compilation of the desired portable state for one OCI attachment.

use nimbus_core::TenantId;
use nimbus_network::{
    NetworkPlan, NetworkPlanContentDigest, NetworkPlanId, NetworkProviderHandle,
    NetworkProviderHandleError, NetworkResourceGeneration,
};

use super::AttachmentBackendKind;
use crate::backends::capabilities::{
    SandboxAttachmentRegistrationKind, host_managed_attachment_provider_id,
    host_managed_attachment_requirements,
};
use crate::backends::oci::network::default_network_attachment_id;
use crate::instance::SandboxId;

const TRANSITIONAL_ATTACHMENT_GENERATION: NetworkResourceGeneration =
    NetworkResourceGeneration::new(1);
const CONTENT_DOMAIN: &[u8] = b"nimbus.sandbox.oci.attachment-plan.v1\0";

/// Reconstruct the exact provider-neutral desired plan for one OCI attachment.
///
/// Startup classification uses the same pure compiler as live attachment
/// creation so plan ID, generation, and digest cannot drift into a second
/// recovery-only definition.
pub(in crate::backends::oci::network) fn oci_attachment_plan(
    tenant_id: &TenantId,
    sandbox_id: &SandboxId,
    backend: AttachmentBackendKind,
) -> NetworkPlan {
    let attachment_id = default_network_attachment_id(sandbox_id);
    let provider_label = backend.provider_label();
    let registration_kind = oci_attachment_registration_kind(backend);
    let mut content = Vec::with_capacity(
        CONTENT_DOMAIN.len() + attachment_id.as_str().len() + provider_label.len() + 16,
    );
    content.extend_from_slice(CONTENT_DOMAIN);
    push_framed(&mut content, attachment_id.as_str().as_bytes());
    push_framed(&mut content, provider_label.as_bytes());
    NetworkPlan::new(
        NetworkPlanId::for_tenant_workload_plan(tenant_id, sandbox_id.as_str()),
        TRANSITIONAL_ATTACHMENT_GENERATION,
        NetworkPlanContentDigest::sha256(content),
        host_managed_attachment_requirements(registration_kind),
    )
}

/// Reconstruct the exact durable provider handle for one OCI attachment.
///
/// Live creation and startup classification share this pure compiler so a
/// stale or substituted provider handle cannot be mistaken for current state.
pub(in crate::backends::oci::network) fn oci_attachment_provider_handle(
    tenant_id: &TenantId,
    sandbox_id: &SandboxId,
    backend: AttachmentBackendKind,
) -> Result<NetworkProviderHandle, NetworkProviderHandleError> {
    let attachment_id = default_network_attachment_id(sandbox_id);
    let plan = oci_attachment_plan(tenant_id, sandbox_id, backend);
    NetworkProviderHandle::new(
        host_managed_attachment_provider_id(oci_attachment_registration_kind(backend)),
        format!("attachment:{}:{attachment_id}", plan.plan_id()),
    )
}

pub(super) const fn oci_attachment_registration_kind(
    backend: AttachmentBackendKind,
) -> SandboxAttachmentRegistrationKind {
    match backend {
        AttachmentBackendKind::Container => SandboxAttachmentRegistrationKind::Container,
        AttachmentBackendKind::Krun => SandboxAttachmentRegistrationKind::Krun,
    }
}

fn push_framed(output: &mut Vec<u8>, value: &[u8]) {
    output.extend_from_slice(&(value.len() as u64).to_be_bytes());
    output.extend_from_slice(value);
}
