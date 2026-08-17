use std::fs;
use std::path::PathBuf;

use nimbus_core::TenantId;
use nimbus_network::{
    LocalNetworkAttachmentAuthority, LocalNetworkStateStore, NetworkAttachmentReservationState,
    NetworkResourcePhase, NetworkSegmentAllocator, NetworkStateTransition,
    NetworkTransitionEvidence,
};
use tempfile::TempDir;

use super::*;
use crate::backends::capabilities::{
    SandboxAttachmentRegistrationKind, host_managed_attachment_provider_id,
};
use crate::backends::oci::network::attachment_lifecycle::{
    AttachmentBackendKind, OciAttachmentLifecycle, oci_attachment_plan,
    oci_attachment_provider_handle,
};
use crate::backends::oci::network::ipam::{
    begin_netavark_setup, begin_netavark_setup_execution, complete_netavark_setup,
};
use crate::backends::oci::network::orphan_evidence::test_support::{
    EvidenceFixture, reservation_claim,
};
use crate::backends::oci::network::{
    OciNetworkLayout, OciPlacementAuthority, OciPlacementProvider, RecordingSegmentAllocator,
    SegmentAllocatorOperation, default_network_attachment_id, place_sandbox_on_block,
};
use crate::instance::SandboxId;

fn ready_fixture(label: &str) -> EvidenceFixture {
    let fixture = EvidenceFixture::new(label, AttachmentBackendKind::Container, false);
    prepare_ready_evidence(
        &fixture.workload_root,
        &fixture.tenant_id,
        &fixture.sandbox_id,
        &fixture.layout,
        &fixture.config,
        &fixture.attachments,
        &fixture.ipam,
        &fixture.allocator,
        &fixture.claim,
    );
    fixture
}

#[allow(clippy::too_many_arguments)]
fn prepare_ready_evidence(
    workload_root: &Path,
    tenant_id: &TenantId,
    sandbox_id: &SandboxId,
    layout: &OciNetworkLayout,
    config: &super::super::OciNetworkConfig,
    attachments: &LocalNetworkAttachmentAuthority,
    ipam: &OciIpamAuthority,
    allocator: &OciSegmentAllocator,
    claim: &NetworkReservationClaim,
) {
    let attachment_id = default_network_attachment_id(sandbox_id);
    allocator
        .adopt_reserved_attachment(tenant_id, &attachment_id, claim)
        .expect("exact allocator reservation should adopt");
    transition_desired_to_ready(attachments, tenant_id, sandbox_id);
    let (_, setup_claim) = begin_netavark_setup(ipam, layout, config, sandbox_id)
        .expect("provider setup attempt should prepare");
    begin_netavark_setup_execution(ipam, layout, config, sandbox_id, &setup_claim)
        .expect("provider setup should cross its pre-effect fence");
    complete_netavark_setup(ipam, layout, &setup_claim)
        .expect("provider setup should become ready");
    publish_exact_artifacts(workload_root, tenant_id, sandbox_id, layout);
}

fn transition_desired_to_ready(
    attachments: &LocalNetworkAttachmentAuthority,
    tenant_id: &TenantId,
    sandbox_id: &SandboxId,
) {
    let attachment_id = default_network_attachment_id(sandbox_id);
    let mut desired = attachments
        .get(tenant_id, &attachment_id)
        .expect("desired authority should inspect")
        .expect("desired record should exist");
    let (_, provisioning) = attachments
        .apply_transition(
            tenant_id,
            &NetworkStateTransition::new(
                desired.resource().version().clone(),
                NetworkResourcePhase::Provisioning,
                NetworkTransitionEvidence::Progress,
            ),
        )
        .expect("desired attachment should enter provisioning");
    desired = provisioning;
    let stable_handle =
        oci_attachment_provider_handle(tenant_id, sandbox_id, AttachmentBackendKind::Container)
            .expect("stable provider handle should validate");
    let (_, with_handle) = attachments
        .record_provider_handle(tenant_id, desired.resource().version(), stable_handle)
        .expect("provisioning should retain its stable provider handle");
    let (_, ready) = attachments
        .apply_transition(
            tenant_id,
            &NetworkStateTransition::new(
                with_handle.resource().version().clone(),
                NetworkResourcePhase::Ready,
                NetworkTransitionEvidence::Progress,
            ),
        )
        .expect("desired attachment should enter ready");
    assert_eq!(ready.resource().phase(), NetworkResourcePhase::Ready);
}

fn publish_exact_artifacts(
    workload_root: &Path,
    tenant_id: &TenantId,
    sandbox_id: &SandboxId,
    layout: &OciNetworkLayout,
) {
    fs::write(&layout.netns_path, b"netns-observation")
        .expect("network namespace observation should write");
    fs::write(&layout.status_path, b"status-observation")
        .expect("provider status observation should write");
    let manifest_path = crate::artifact_paths::manifest_path(workload_root, tenant_id, sandbox_id);
    fs::create_dir_all(
        manifest_path
            .parent()
            .expect("manifest should have a parent"),
    )
    .expect("manifest parent should create");
    fs::write(manifest_path, b"manifest-observation").expect("manifest observation should write");
}

#[test]
fn exact_adoption_is_byte_preserving_across_every_durable_authority() {
    let fixture = ready_fixture("startup-adopt-read-only");
    let authority_before = fixture.authority_bytes();
    let artifacts_before = [
        fs::read(crate::artifact_paths::manifest_path(
            &fixture.workload_root,
            &fixture.tenant_id,
            &fixture.sandbox_id,
        ))
        .expect("manifest bytes should read"),
        fs::read(&fixture.layout.netns_path).expect("netns bytes should read"),
        fs::read(&fixture.layout.status_path).expect("status bytes should read"),
    ];

    reconcile_startup_network_state(
        &fixture.workload_root,
        &fixture.attachments,
        &fixture.ipam,
        &fixture.allocator,
    )
    .expect("exact ready evidence should be adopted read-only");

    assert_eq!(
        fixture.authority_bytes(),
        authority_before,
        "adoption must not rewrite desired, allocator, or IPAM authority"
    );
    assert_eq!(
        [
            fs::read(crate::artifact_paths::manifest_path(
                &fixture.workload_root,
                &fixture.tenant_id,
                &fixture.sandbox_id,
            ))
            .expect("manifest bytes should reread"),
            fs::read(&fixture.layout.netns_path).expect("netns bytes should reread"),
            fs::read(&fixture.layout.status_path).expect("status bytes should reread"),
        ],
        artifacts_before,
        "adoption must not rewrite provider artifacts"
    );
}

#[test]
fn missing_namespace_quarantines_exact_authorities_without_cleanup_or_reuse() {
    let fixture = ready_fixture("startup-missing-netns");
    fs::remove_file(&fixture.layout.netns_path).expect("netns absence should be installed");
    let provider_before = fixture
        .ipam
        .get_attachment_provider_evidence(
            &fixture.tenant_id,
            &default_network_attachment_id(&fixture.sandbox_id),
        )
        .expect("provider evidence should inspect");

    let error = reconcile_startup_network_state(
        &fixture.workload_root,
        &fixture.attachments,
        &fixture.ipam,
        &fixture.allocator,
    )
    .expect_err("missing namespace must keep startup admission fenced");
    assert!(
        error.to_string().contains("network namespace missing"),
        "the deterministic fence must name the classified evidence: {error}"
    );

    let attachment_id = default_network_attachment_id(&fixture.sandbox_id);
    let desired = fixture
        .attachments
        .get(&fixture.tenant_id, &attachment_id)
        .expect("desired authority should inspect")
        .expect("desired record should remain");
    assert_eq!(
        desired.resource().phase(),
        NetworkResourcePhase::CleanupPending,
        "the exact desired generation must retain ambiguous-effect authority"
    );
    let allocator = fixture
        .allocator
        .inspect_attachment_reservation(&fixture.tenant_id, &attachment_id, &fixture.claim)
        .expect("allocator authority should inspect");
    assert_eq!(
        allocator.state(),
        NetworkAttachmentReservationState::ProviderCleanupPending,
        "the exact adopted hold must remain fenced from capacity reuse"
    );
    assert_eq!(
        fixture
            .ipam
            .get_attachment_provider_evidence(&fixture.tenant_id, &attachment_id)
            .expect("provider evidence should reinspect"),
        provider_before,
        "startup quarantine must not retire or rewrite IPAM provider authority"
    );
    assert!(
        !fixture.layout.netns_path.exists()
            && fixture.layout.status_path.is_file()
            && crate::artifact_paths::manifest_path(
                &fixture.workload_root,
                &fixture.tenant_id,
                &fixture.sandbox_id
            )
            .is_file(),
        "startup quarantine must preserve every surviving artifact"
    );

    let authority_after_first = fixture.authority_bytes();
    let replay = reconcile_startup_network_state(
        &fixture.workload_root,
        &fixture.attachments,
        &fixture.ipam,
        &fixture.allocator,
    )
    .expect_err("quarantined evidence remains a startup fence until cleanup convergence");
    assert!(
        replay.to_string().contains("desired phase not adoptable"),
        "replay should deterministically classify the retained fence: {replay}"
    );
    assert_eq!(
        fixture.authority_bytes(),
        authority_after_first,
        "replaying exact quarantine must be byte-preserving"
    );
}

#[test]
fn desired_then_allocator_partial_failure_converges_after_restart() {
    let temp_dir = TempDir::new().expect("temporary root should exist");
    let workload_root = temp_dir.path().join("workloads");
    let network_root = temp_dir.path().join("network");
    fs::create_dir_all(&workload_root).expect("workload root should create");
    fs::create_dir_all(&network_root).expect("network root should create");
    let tenant_id = TenantId::new("nnc52d-partial-quarantine").expect("tenant should validate");
    let sandbox_id = SandboxId::new("sandbox-partial-quarantine");
    let layout =
        OciNetworkLayout::with_roots(&workload_root, &network_root, &tenant_id, &sandbox_id);
    layout
        .ensure_directories()
        .expect("network directories should create");
    let ipam =
        OciIpamAuthority::reconstruct_for_direct_test(&layout).expect("IPAM authority should open");
    let allocator = RecordingSegmentAllocator::new(tenant_id.clone(), "10.93.0.0/24", 93)
        .with_quarantine_failure("injected allocator quarantine interruption");
    let attachments = LocalNetworkAttachmentAuthority::open(&network_root)
        .expect("attachment authority should open");
    let claim = reservation_claim("partial-quarantine");
    let attachment_id = default_network_attachment_id(&sandbox_id);
    let config = place_sandbox_on_block(
        &allocator,
        &ipam,
        &tenant_id,
        &layout,
        &sandbox_id,
        OciPlacementAuthority::new(&attachment_id, &claim),
        OciPlacementProvider::new(
            AttachmentBackendKind::Container.provider_kind(),
            |segment, reservation_claim| {
                OciAttachmentLifecycle::config_from_segment(
                    AttachmentBackendKind::Container,
                    PathBuf::from("netavark-not-executed"),
                    PathBuf::from("aardvark-not-executed"),
                    segment,
                    &attachment_id,
                    reservation_claim,
                )
            },
        ),
    )
    .expect("placement should reserve exact authority");
    let association = allocator
        .inspect_attachment_reservation(&tenant_id, &attachment_id, &claim)
        .expect("allocator reservation should inspect")
        .association()
        .expect("placement should bind an association")
        .clone();
    attachments
        .reserve(
            &tenant_id,
            host_managed_attachment_provider_id(SandboxAttachmentRegistrationKind::Container),
            &oci_attachment_plan(&tenant_id, &sandbox_id, AttachmentBackendKind::Container),
            attachment_id.clone(),
            association,
        )
        .expect("desired authority should reserve");
    prepare_ready_evidence(
        &workload_root,
        &tenant_id,
        &sandbox_id,
        &layout,
        &config,
        &attachments,
        &ipam,
        &allocator,
        &claim,
    );
    fs::remove_file(&layout.netns_path).expect("missing netns should trigger quarantine");

    let first = reconcile_startup_network_state(&workload_root, &attachments, &ipam, &allocator)
        .expect_err("the injected second authority failure must fence startup");
    assert!(
        first
            .to_string()
            .contains("exact quarantine application failed")
            && first
                .to_string()
                .contains("injected allocator quarantine interruption"),
        "the partial boundary should remain observable: {first}"
    );
    assert_eq!(
        attachments
            .get(&tenant_id, &attachment_id)
            .expect("desired authority should inspect")
            .expect("desired record should remain")
            .resource()
            .phase(),
        NetworkResourcePhase::CleanupPending,
        "the first durable authority must commit before the interrupted second authority"
    );
    assert_eq!(
        allocator
            .inspect_attachment_reservation(&tenant_id, &attachment_id, &claim)
            .expect("allocator should inspect after interruption")
            .state(),
        NetworkAttachmentReservationState::Adopted,
        "allocator interruption must retain the exact adopted hold"
    );

    allocator.clear_quarantine_failure();
    let retry = reconcile_startup_network_state(&workload_root, &attachments, &ipam, &allocator)
        .expect_err("successful quarantine remains fenced for later cleanup");
    assert!(
        !retry
            .to_string()
            .contains("exact quarantine application failed"),
        "the restart must converge the interrupted allocator transition: {retry}"
    );
    assert_eq!(
        allocator
            .inspect_attachment_reservation(&tenant_id, &attachment_id, &claim)
            .expect("allocator should inspect after retry")
            .state(),
        NetworkAttachmentReservationState::ProviderCleanupPending,
        "restart must converge to the exact allocator cleanup fence"
    );
    let bytes_after_retry = fs::read(LocalNetworkStateStore::authority_path_for(&network_root))
        .expect("authority bytes should read");
    reconcile_startup_network_state(&workload_root, &attachments, &ipam, &allocator)
        .expect_err("replayed cleanup-pending evidence remains admission-fenced");
    assert_eq!(
        fs::read(LocalNetworkStateStore::authority_path_for(&network_root))
            .expect("authority bytes should reread"),
        bytes_after_retry,
        "converged restart replay must be byte-preserving"
    );
    assert!(
        allocator.operations().iter().any(|operation| matches!(
            operation,
            SegmentAllocatorOperation::Quarantine(observed_tenant, observed_attachment)
                if observed_tenant == &tenant_id && observed_attachment == &attachment_id
        )),
        "the retry must target the exact tenant-qualified attachment"
    );
}

#[test]
fn conflicting_claim_evidence_never_receives_allocator_quarantine_authority() {
    let fixture = EvidenceFixture::new(
        "startup-conflicting-claim",
        AttachmentBackendKind::Container,
        true,
    );
    prepare_ready_evidence(
        &fixture.workload_root,
        &fixture.tenant_id,
        &fixture.sandbox_id,
        &fixture.layout,
        &fixture.config,
        &fixture.attachments,
        &fixture.ipam,
        &fixture.allocator,
        &fixture.claim,
    );

    let error = reconcile_startup_network_state(
        &fixture.workload_root,
        &fixture.attachments,
        &fixture.ipam,
        &fixture.allocator,
    )
    .expect_err("conflicting desired/provider claims must fence startup");
    assert!(
        error.to_string().contains("stale generation evidence"),
        "the deterministic conflict reason should remain observable: {error}"
    );
    let attachment_id = default_network_attachment_id(&fixture.sandbox_id);
    assert_eq!(
        fixture
            .allocator
            .inspect_attachment_reservation(&fixture.tenant_id, &attachment_id, &fixture.claim,)
            .expect("winning allocator claim should inspect")
            .state(),
        NetworkAttachmentReservationState::Adopted,
        "a conflicting desired claim must not authorize allocator quarantine"
    );
    assert_eq!(
        fixture
            .attachments
            .get(&fixture.tenant_id, &attachment_id)
            .expect("desired authority should inspect")
            .expect("desired record should remain")
            .resource()
            .phase(),
        NetworkResourcePhase::CleanupPending,
        "the independently exact desired generation should still retain its own fence"
    );
    let after_first = fixture.authority_bytes();
    reconcile_startup_network_state(
        &fixture.workload_root,
        &fixture.attachments,
        &fixture.ipam,
        &fixture.allocator,
    )
    .expect_err("conflicting evidence remains fenced on replay");
    assert_eq!(
        fixture.authority_bytes(),
        after_first,
        "conflicting evidence replay must not mutate either authority"
    );
}

#[test]
fn stale_desired_version_is_rejected_without_mutating_replacement_authority() {
    let source = ready_fixture("startup-stale-version");
    let report = collect_oci_orphan_evidence(
        &source.workload_root,
        &source.attachments,
        &source.ipam,
        &source.allocator,
    )
    .expect("source evidence should collect");
    let stale_candidate = report
        .candidates()
        .first()
        .expect("source candidate should exist")
        .clone();
    let replacement = EvidenceFixture::new_with_selected_provider(
        "startup-stale-version",
        AttachmentBackendKind::Krun,
        SandboxAttachmentRegistrationKind::Krun,
        false,
    );
    assert_eq!(source.tenant_id, replacement.tenant_id);
    assert_eq!(source.sandbox_id, replacement.sandbox_id);
    let replacement_before = replacement.authority_bytes();

    let error = quarantine_desired_generation(
        &replacement.attachments,
        &stale_candidate,
        stale_candidate
            .desired()
            .expect("stale candidate should carry desired evidence"),
    )
    .expect_err("a different plan generation must reject the stale version");
    assert!(
        error.to_string().contains("version")
            || error.to_string().contains("digest")
            || error.to_string().contains("plan"),
        "the CAS rejection should name the stale version fence: {error}"
    );
    assert_eq!(
        replacement.authority_bytes(),
        replacement_before,
        "stale desired evidence must be byte-preserving for replacement authority"
    );
}

#[test]
fn reserved_evidence_fences_deterministically_without_fabricating_mutation_authority() {
    let fixture = EvidenceFixture::new(
        "startup-reserved-no-effect",
        AttachmentBackendKind::Container,
        false,
    );
    let before = fixture.authority_bytes();
    let first = reconcile_startup_network_state(
        &fixture.workload_root,
        &fixture.attachments,
        &fixture.ipam,
        &fixture.allocator,
    )
    .expect_err("reserved provider evidence cannot be adopted at startup");
    let second = reconcile_startup_network_state(
        &fixture.workload_root,
        &fixture.attachments,
        &fixture.ipam,
        &fixture.allocator,
    )
    .expect_err("fresh reconciliation must retain the same reserved fence");
    assert!(
        first.to_string().contains("desired phase not adoptable"),
        "the reserved fence should be deterministic: {first}"
    );
    assert_eq!(first.to_string(), second.to_string());
    assert_eq!(
        fixture.authority_bytes(),
        before,
        "reserved evidence cannot authorize desired or allocator quarantine"
    );
    assert_eq!(
        fixture
            .allocator
            .inspect_attachment_reservation(
                &fixture.tenant_id,
                &default_network_attachment_id(&fixture.sandbox_id),
                &fixture.claim,
            )
            .expect("reserved allocator evidence should inspect")
            .state(),
        NetworkAttachmentReservationState::Reserved
    );
}

#[test]
fn unmatched_provider_without_a_hold_remains_durable_and_fences_every_restart() {
    let temp_dir = TempDir::new().expect("temporary root should exist");
    let workload_root = temp_dir.path().join("source-workloads");
    let foreign_workload_root = temp_dir.path().join("foreign-workloads");
    let network_root = temp_dir.path().join("network");
    fs::create_dir_all(&workload_root).expect("source workload root should create");
    fs::create_dir_all(&foreign_workload_root).expect("foreign workload root should create");
    fs::create_dir_all(&network_root).expect("network root should create");
    let tenant_id = TenantId::new("nnc52d-unmatched-provider").expect("tenant should validate");
    let sandbox_id = SandboxId::new("sandbox-unmatched-provider");
    let layout =
        OciNetworkLayout::with_roots(&workload_root, &network_root, &tenant_id, &sandbox_id);
    layout
        .ensure_directories()
        .expect("source artifact directories should create");
    let ipam =
        OciIpamAuthority::reconstruct_for_direct_test(&layout).expect("IPAM authority should open");
    let allocator = RecordingSegmentAllocator::new(tenant_id.clone(), "10.95.0.0/24", 95);
    let claim = reservation_claim("unmatched-provider");
    let attachment_id = default_network_attachment_id(&sandbox_id);
    place_sandbox_on_block(
        &allocator,
        &ipam,
        &tenant_id,
        &layout,
        &sandbox_id,
        OciPlacementAuthority::new(&attachment_id, &claim),
        OciPlacementProvider::new(
            AttachmentBackendKind::Container.provider_kind(),
            |segment, reservation_claim| {
                OciAttachmentLifecycle::config_from_segment(
                    AttachmentBackendKind::Container,
                    PathBuf::from("netavark-not-executed"),
                    PathBuf::from("aardvark-not-executed"),
                    segment,
                    &attachment_id,
                    reservation_claim,
                )
            },
        ),
    )
    .expect("provider authority should persist");
    allocator
        .release_reserved_attachment_without_effect(&tenant_id, &attachment_id, &claim)
        .expect("test cut should move allocator authority toward removal");
    allocator
        .finalize_reserved_attachment_without_effect(&tenant_id, &attachment_id, &claim)
        .expect("test cut should remove the allocator hold");
    assert_eq!(
        allocator
            .inspect_attachment_reservation(&tenant_id, &attachment_id, &claim)
            .expect("allocator absence should inspect")
            .state(),
        NetworkAttachmentReservationState::Absent,
        "precondition: unmatched provider evidence has no allocator hold"
    );
    let attachments = LocalNetworkAttachmentAuthority::open(&network_root)
        .expect("empty attachment authority should open");
    let provider_before = ipam
        .get_attachment_provider_evidence(&tenant_id, &attachment_id)
        .expect("provider evidence should inspect")
        .expect("provider evidence should remain live");
    let authority_path = LocalNetworkStateStore::authority_path_for(&network_root);
    let authority_before = fs::read(&authority_path).expect("network authority should read");

    let first =
        reconcile_startup_network_state(&foreign_workload_root, &attachments, &ipam, &allocator)
            .expect_err("foreign-realm provider evidence must fence startup");
    let second =
        reconcile_startup_network_state(&foreign_workload_root, &attachments, &ipam, &allocator)
            .expect_err("foreign-realm provider evidence must fence every restart");
    assert!(
        first.to_string().contains("provider realm mismatch"),
        "the unmatched provider fence should be explicit: {first}"
    );
    assert_eq!(first.to_string(), second.to_string());
    assert_eq!(
        ipam.get_attachment_provider_evidence(&tenant_id, &attachment_id)
            .expect("provider evidence should reinspect")
            .expect("provider evidence should remain"),
        provider_before,
        "startup must not retire unmatched provider authority"
    );
    assert_eq!(
        fs::read(authority_path).expect("network authority should reread"),
        authority_before,
        "unmatched provider quarantine must be byte-preserving"
    );
}

#[test]
fn artifact_scan_unknown_is_preserved_and_fences_deterministically() {
    let temp_dir = TempDir::new().expect("temporary root should exist");
    let workload_root = temp_dir.path().join("workloads");
    let network_root = temp_dir.path().join("network");
    fs::create_dir_all(&workload_root).expect("workload root should create");
    fs::create_dir_all(&network_root).expect("network root should create");
    fs::write(workload_root.join("tenants"), b"not-a-directory")
        .expect("unexpected artifact type should install");
    let tenant_id = TenantId::new("nnc52d-scan-unknown").expect("tenant should validate");
    let sandbox_id = SandboxId::new("sandbox-scan-unknown");
    let layout =
        OciNetworkLayout::with_roots(&workload_root, &network_root, &tenant_id, &sandbox_id);
    let ipam = OciIpamAuthority::reconstruct_for_direct_test(&layout)
        .expect("empty IPAM authority should open");
    let attachments = LocalNetworkAttachmentAuthority::open(&network_root)
        .expect("empty attachment authority should open");
    let allocator = crate::backends::oci::network::SingleNodeSegmentAllocator::single_node_default(
        &network_root,
    );
    let authority_path = LocalNetworkStateStore::authority_path_for(&network_root);
    assert!(
        !authority_path.exists(),
        "precondition: opening empty authorities should remain read-only"
    );

    let first = reconcile_startup_network_state(&workload_root, &attachments, &ipam, &allocator)
        .expect_err("unknown artifact scan evidence must fence startup");
    let second = reconcile_startup_network_state(&workload_root, &attachments, &ipam, &allocator)
        .expect_err("unknown artifact scan evidence must fence every restart");
    assert!(
        first
            .to_string()
            .contains("enumerate tenant artifact roots")
            && first.to_string().contains("unknown inspection"),
        "the exact unknown observation should remain diagnosable: {first}"
    );
    assert_eq!(first.to_string(), second.to_string());
    assert!(
        workload_root.join("tenants").is_file(),
        "startup quarantine must preserve the unknown artifact"
    );
    assert!(
        !authority_path.exists(),
        "unknown inspection cannot create or mutate durable authority"
    );
}
