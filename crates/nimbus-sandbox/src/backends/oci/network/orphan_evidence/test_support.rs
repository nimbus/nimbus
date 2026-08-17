use std::fs;
use std::path::PathBuf;

use nimbus_core::TenantId;
use nimbus_network::{
    LocalNetworkAttachmentAuthority, LocalNetworkStateStore, NetworkAttachmentId,
    NetworkAttachmentSegmentAssociation, NetworkProviderHandle, NetworkProviderId,
    NetworkReservationClaim, NetworkSegmentAllocator,
};
use tempfile::TempDir;

use super::super::attachment_lifecycle::{
    AttachmentBackendKind, OciAttachmentLifecycle, oci_attachment_plan,
};
use super::super::ipam::OciIpamAuthority;
use super::super::{
    OciNetworkConfig, OciNetworkLayout, OciPlacementAuthority, OciPlacementProvider,
    SingleNodeSegmentAllocator, default_network_attachment_id, place_sandbox_on_block,
};
use crate::backends::capabilities::{
    SandboxAttachmentRegistrationKind, host_managed_attachment_provider_id,
};
use crate::instance::SandboxId;

pub(in crate::backends::oci::network) struct EvidenceFixture {
    pub(in crate::backends::oci::network) _temp_dir: TempDir,
    pub(in crate::backends::oci::network) workload_root: PathBuf,
    pub(in crate::backends::oci::network) network_root: PathBuf,
    pub(in crate::backends::oci::network) tenant_id: TenantId,
    pub(in crate::backends::oci::network) sandbox_id: SandboxId,
    pub(in crate::backends::oci::network) layout: OciNetworkLayout,
    pub(in crate::backends::oci::network) ipam: OciIpamAuthority,
    pub(in crate::backends::oci::network) allocator: SingleNodeSegmentAllocator,
    pub(in crate::backends::oci::network) attachments: LocalNetworkAttachmentAuthority,
    pub(in crate::backends::oci::network) claim: NetworkReservationClaim,
    pub(in crate::backends::oci::network) config: OciNetworkConfig,
}

impl EvidenceFixture {
    pub(in crate::backends::oci::network) fn new(
        label: &str,
        backend: AttachmentBackendKind,
        desired_claim_substitution: bool,
    ) -> Self {
        let registration_kind = match backend {
            AttachmentBackendKind::Container => SandboxAttachmentRegistrationKind::Container,
            AttachmentBackendKind::Krun => SandboxAttachmentRegistrationKind::Krun,
        };
        Self::new_with_selected_provider(
            label,
            backend,
            registration_kind,
            desired_claim_substitution,
        )
    }

    pub(in crate::backends::oci::network) fn new_with_selected_provider(
        label: &str,
        backend: AttachmentBackendKind,
        registration_kind: SandboxAttachmentRegistrationKind,
        desired_claim_substitution: bool,
    ) -> Self {
        let temp_dir = TempDir::new().expect("temporary evidence root should exist");
        let workload_root = temp_dir.path().join("workloads");
        let network_root = temp_dir.path().join("network-authority");
        fs::create_dir_all(&workload_root).expect("workload root should exist");
        fs::create_dir_all(&network_root).expect("network root should exist");
        let tenant_id = TenantId::new(format!("nnc52b-{label}")).expect("tenant should validate");
        let sandbox_id = SandboxId::new(format!("sandbox-{label}"));
        let layout =
            OciNetworkLayout::with_roots(&workload_root, &network_root, &tenant_id, &sandbox_id);
        layout
            .ensure_directories()
            .expect("provider artifact directories should exist");
        let ipam = OciIpamAuthority::reconstruct_for_direct_test(&layout)
            .expect("IPAM authority should open");
        let allocator = SingleNodeSegmentAllocator::single_node_default(&network_root);
        let attachments = LocalNetworkAttachmentAuthority::open(&network_root)
            .expect("attachment authority should open");
        let claim = reservation_claim(&format!("{label}-winner"));
        let attachment_id = default_network_attachment_id(&sandbox_id);
        let config = place_sandbox_on_block(
            &allocator,
            &ipam,
            &tenant_id,
            &layout,
            &sandbox_id,
            OciPlacementAuthority::new(&attachment_id, &claim),
            OciPlacementProvider::new(backend.provider_kind(), |segment, reservation_claim| {
                OciAttachmentLifecycle::config_from_segment(
                    backend,
                    PathBuf::from("netavark-not-executed"),
                    PathBuf::from("aardvark-not-executed"),
                    segment,
                    &attachment_id,
                    reservation_claim,
                )
            }),
        )
        .expect("placement should persist IPAM evidence before effects");
        let allocator_association = allocator
            .inspect_attachment_reservation(&tenant_id, &attachment_id, &claim)
            .expect("allocator reservation should inspect")
            .association()
            .expect("placement should bind the selected segment")
            .clone();
        let desired_association = if desired_claim_substitution {
            NetworkAttachmentSegmentAssociation::new(
                reservation_claim(&format!("{label}-foreign-desired")),
                allocator_association.segment_id().clone(),
                allocator_association.lease_epoch(),
            )
        } else {
            allocator_association
        };
        let desired_backend = match registration_kind {
            SandboxAttachmentRegistrationKind::Container => AttachmentBackendKind::Container,
            SandboxAttachmentRegistrationKind::Krun => AttachmentBackendKind::Krun,
        };
        let plan = oci_attachment_plan(&tenant_id, &sandbox_id, desired_backend);
        attachments
            .reserve(
                &tenant_id,
                host_managed_attachment_provider_id(registration_kind),
                &plan,
                attachment_id,
                desired_association,
            )
            .expect("desired attachment should reserve");
        Self {
            _temp_dir: temp_dir,
            workload_root,
            network_root,
            tenant_id,
            sandbox_id,
            layout,
            ipam,
            allocator,
            attachments,
            claim,
            config,
        }
    }

    pub(in crate::backends::oci::network) fn publish_exact_artifacts(&self) {
        fs::write(&self.layout.netns_path, b"netns-observation")
            .expect("netns observation should write");
        fs::write(&self.layout.status_path, b"status-observation")
            .expect("status observation should write");
        let manifest_path = crate::artifact_paths::manifest_path(
            &self.workload_root,
            &self.tenant_id,
            &self.sandbox_id,
        );
        fs::create_dir_all(
            manifest_path
                .parent()
                .expect("manifest should have a parent"),
        )
        .expect("manifest parent should exist");
        fs::write(manifest_path, b"manifest-observation")
            .expect("manifest observation should write");
    }

    pub(in crate::backends::oci::network) fn authority_bytes(&self) -> Vec<u8> {
        fs::read(LocalNetworkStateStore::authority_path_for(
            &self.network_root,
        ))
        .expect("authority bytes should read")
    }
}

pub(in crate::backends::oci::network) fn reservation_claim(label: &str) -> NetworkReservationClaim {
    NetworkReservationClaim::new(
        NetworkProviderHandle::new(
            NetworkProviderId::for_registration_key(
                "nimbus-sandbox.network-launch-coordinator.nnc5-2b-test",
            ),
            format!("attempt:{label}"),
        )
        .expect("reservation claim should validate"),
    )
}

pub(in crate::backends::oci::network) fn netavark_attempt(
    provider_key: &str,
    version: &str,
    action: &str,
    tenant_id: &TenantId,
    attachment_id: &NetworkAttachmentId,
    generation_digest: &str,
    attempt_id: &str,
) -> NetworkProviderHandle {
    NetworkProviderHandle::new(
        NetworkProviderId::for_registration_key(provider_key),
        format!(
            "{version}:{action}:{}:{}:{generation_digest}:{attempt_id}",
            tenant_id.as_str(),
            attachment_id.as_str()
        ),
    )
    .expect("provider attempt fixture should validate")
}
