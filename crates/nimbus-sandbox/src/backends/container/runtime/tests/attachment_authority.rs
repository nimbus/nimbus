//! Production-route proof for the container durable attachment authority.

use super::support::*;
use super::*;

use nimbus_core::TenantId;
use nimbus_network::{
    LocalNetworkAttachmentAuthority, NetworkLeaseEpoch, NetworkPlan, NetworkPlanContentDigest,
    NetworkPlanId, NetworkProviderId, NetworkResourceGeneration,
};
use tempfile::TempDir;

use crate::backends::capabilities::{
    SandboxAttachmentRegistrationKind, host_managed_attachment_requirements,
};
use crate::backends::oci::network::{OciNetworkLayout, default_network_attachment_id};

fn foreign_plan(tenant_id: &TenantId, sandbox_id: &SandboxId) -> NetworkPlan {
    NetworkPlan::new(
        NetworkPlanId::for_tenant_workload_plan(tenant_id, sandbox_id.as_str()),
        NetworkResourceGeneration::new(1),
        NetworkPlanContentDigest::sha256(b"foreign-container-attachment-plan"),
        host_managed_attachment_requirements(SandboxAttachmentRegistrationKind::Container),
    )
}

#[test]
fn fresh_container_backend_reopens_attachment_authority_on_its_production_route() {
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
            NetworkLeaseEpoch::new(1),
        )
        .expect("foreign selected-provider fixture should persist");
    drop(authority);

    let reopened = ContainerSandboxBackend::new(config);
    let manifest = reopened
        .plan_start_with_id(&sample_spec(), &sandbox_id, None, None)
        .expect("container execute plan should reserve launch authority")
        .manifest;
    let claim = manifest
        .launch_reservation_claim
        .clone()
        .expect("container execute manifest should retain its claim");
    reopened
        .segment_allocator
        .adopt_reserved_attachment(
            &manifest.spec.tenant_id,
            &default_network_attachment_id(&manifest.handle.id),
            &claim,
        )
        .expect("container production route fixture should adopt its exact reservation");

    let error = reopened
        .configure_network(
            &manifest,
            AttachmentAttachAuthority::FreshLaunch(&claim),
            MachinePortPreparationReleaseAuthority::FreshLaunch(&claim),
        )
        .expect_err("the production route must authenticate reopened provider authority");

    assert!(
        error.to_string().contains("selected provider"),
        "the real container route must observe the reopened provider conflict: {error}"
    );
    assert!(
        !manifest.network_layout.netns_path.exists()
            && !manifest.network_layout.status_path.exists()
            && !reopened
                .config
                .workload_state_root
                .join("networks/.legacy-nimbus0-purged")
                .exists(),
        "durable authority authentication must precede namespace, provider, and migration effects"
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
            NetworkLeaseEpoch::new(1),
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
        !layout.netns_path.exists()
            && !layout.status_path.exists()
            && !config
                .workload_state_root
                .join("networks/.legacy-nimbus0-purged")
                .exists(),
        "corrupt construction must fail before namespace, provider, or migration effects"
    );
}
