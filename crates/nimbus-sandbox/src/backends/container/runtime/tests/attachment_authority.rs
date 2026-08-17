//! Production-route proof for the container durable attachment authority.

use super::support::*;
use super::*;

use nimbus_core::TenantId;
use nimbus_network::{
    LocalNetworkAttachmentAuthority, NetworkAttachmentSegmentAssociation, NetworkLeaseEpoch,
    NetworkPlan, NetworkPlanContentDigest, NetworkPlanId, NetworkProviderId,
    NetworkResourceGeneration, NetworkSegmentId,
};
use tempfile::TempDir;

use crate::backends::capabilities::{
    SandboxAttachmentRegistrationKind, host_managed_attachment_requirements,
};
use crate::backends::oci::network::{OciNetworkLayout, default_network_attachment_id};
use crate::backends::oci::port_lease::new_launch_reservation_claim;

fn foreign_plan(tenant_id: &TenantId, sandbox_id: &SandboxId) -> NetworkPlan {
    NetworkPlan::new(
        NetworkPlanId::for_tenant_workload_plan(tenant_id, sandbox_id.as_str()),
        NetworkResourceGeneration::new(1),
        NetworkPlanContentDigest::sha256(b"foreign-container-attachment-plan"),
        host_managed_attachment_requirements(SandboxAttachmentRegistrationKind::Container),
    )
}

fn foreign_association() -> NetworkAttachmentSegmentAssociation {
    NetworkAttachmentSegmentAssociation::new(
        new_launch_reservation_claim().expect("foreign claim should generate"),
        NetworkSegmentId::generate(),
        NetworkLeaseEpoch::new(1),
    )
}

#[test]
fn fresh_container_backend_fences_desired_without_provider_before_planning() {
    let root = TempDir::new().expect("container authority root should exist");
    let config = ContainerSandboxBackendConfig::under_root(root.path());
    let sandbox_id = SandboxId::new("container-durable-attachment-reopen");
    let spec = sample_spec();
    let authority = LocalNetworkAttachmentAuthority::open(&config.network_state_root)
        .expect("container attachment authority should initialize");
    authority
        .reserve(
            &spec.tenant_id,
            NetworkProviderId::for_registration_key("nimbus.test.foreign-container"),
            &foreign_plan(&spec.tenant_id, &sandbox_id),
            default_network_attachment_id(&sandbox_id),
            foreign_association(),
        )
        .expect("foreign selected-provider fixture should persist");
    let authority_path = authority.authority_path().to_path_buf();
    let authority_before =
        std::fs::read(&authority_path).expect("foreign desired authority should read");
    drop(authority);

    let layout = OciNetworkLayout::with_roots(
        &config.workload_state_root,
        &config.network_state_root,
        &spec.tenant_id,
        &sandbox_id,
    );
    let manifest_path = crate::artifact_paths::manifest_path(
        &config.workload_state_root,
        &spec.tenant_id,
        &sandbox_id,
    );
    let reopened = ContainerSandboxBackend::new(config);
    let error = reopened
        .plan_start_with_id(&sample_spec(), &sandbox_id, None, None)
        .expect_err("startup must fence desired authority without provider evidence");

    assert!(
        error
            .to_string()
            .contains("startup reconciliation did not complete")
            && error.to_string().contains("provider attempt missing"),
        "the real container route must name the incomplete cross-authority generation: {error}"
    );
    assert!(
        !manifest_path.exists() && !layout.netns_path.exists() && !layout.status_path.exists(),
        "startup authentication must precede planning, namespace, and provider effects"
    );
    assert_eq!(
        std::fs::read(authority_path).expect("foreign desired authority should re-read"),
        authority_before,
        "a reserved desired record without provider authority is preserved for later convergence"
    );
}

#[test]
fn corrupt_attachment_store_fences_container_construction_before_network_work() {
    let root = TempDir::new().expect("container corruption root should exist");
    let config = ContainerSandboxBackendConfig::under_root(root.path());
    let spec = sample_spec();
    let sandbox_id = SandboxId::new("container-corrupt-attachment-authority");
    let authority = LocalNetworkAttachmentAuthority::open(&config.network_state_root)
        .expect("container attachment authority should initialize");
    authority
        .reserve(
            &spec.tenant_id,
            NetworkProviderId::for_registration_key("nimbus.test.corrupt-container"),
            &foreign_plan(&spec.tenant_id, &sandbox_id),
            default_network_attachment_id(&sandbox_id),
            foreign_association(),
        )
        .expect("container corruption fixture should create an owner-mode state file");
    let authority_path = authority.authority_path().to_path_buf();
    drop(authority);
    std::fs::write(&authority_path, b"{").expect("authority fixture should corrupt");

    let backend = ContainerSandboxBackend::new(config.clone());
    let error = backend
        .plan_start_with_id(&spec, &sandbox_id, None, None)
        .expect_err("corrupt attachment authority must fence normal container planning");
    let layout = OciNetworkLayout::with_roots(
        &config.workload_state_root,
        &config.network_state_root,
        &spec.tenant_id,
        &sandbox_id,
    );

    assert!(
        error
            .to_string()
            .contains("attachment authority store failed")
            && error.to_string().contains("corrupt"),
        "the production constructor must retain the attachment corruption diagnostic: {error}"
    );
    assert!(backend.attachment_authority.is_none());
    assert_eq!(
        std::fs::read(&authority_path).expect("corrupt authority bytes should remain"),
        b"{",
        "constructor and refused planning must not rewrite corrupt authority"
    );
    assert!(
        !layout.netns_path.exists() && !layout.status_path.exists(),
        "corrupt construction must fail before namespace or provider effects"
    );
}
